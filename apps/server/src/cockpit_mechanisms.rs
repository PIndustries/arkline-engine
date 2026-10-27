//! Vessel-owned physical screen authority. Electrical/mechanical ratings are not
//! yet modeled. This controller never claims drone recovery during warp.
use super::*;
use arkline_protocol::pioneer_docked as geometry;
use std::collections::BTreeSet;

fn turret_active(p: &PilotView, now: u64) -> bool {
    matches!(p.location, Location::Station | Location::Belt)
        && p.mining_modules.iter().any(|m| {
            m.module_id < 2
                && m.target_id.is_some_and(|id| {
                    p.locked_asteroids.contains(&id) || p.locked_asteroid == Some(id)
                })
                && matches!((m.cycle_started_at_ms,m.cycle_completes_at_ms),
                (Some(start),Some(end)) if end > start && now >= start)
        })
}
fn reconcile(
    state: &mut PioneerCockpitMechanisms,
    p: &PilotView,
    now: u64,
    mut obstructed: impl FnMut(usize, f64, f64) -> bool,
) {
    if now < state.updated_at_ms {
        return;
    }
    let before = state.clone();
    state.warp_stowed = matches!(
        p.location,
        Location::InTransit { .. } | Location::Jumping { .. }
    );
    let ids: BTreeSet<_> = p
        .drones
        .iter()
        .map(|d| d.id)
        .chain(p.drone.iter().map(|d| d.id))
        .collect();
    for slot in 0..5 {
        if state.drone_ids[slot].is_some_and(|id| !ids.contains(&id))
            && state.screens[slot].fraction_at(now) <= 1e-9
        {
            state.drone_ids[slot] = None;
        }
    }
    if !state.warp_stowed {
        for id in &ids {
            if state.drone_ids.contains(&Some(*id)) {
                continue;
            }
            if let Some(slot) = state.drone_ids.iter().position(Option::is_none) {
                state.drone_ids[slot] = Some(*id);
                state.assignment_generations[slot] =
                    state.assignment_generations[slot].saturating_add(1);
            }
        }
    }
    state.unassigned_drone_ids = ids
        .iter()
        .filter(|id| !state.drone_ids.contains(&Some(**id)))
        .copied()
        .collect();
    state.retiring_drone_slots =
        std::array::from_fn(|slot| state.drone_ids[slot].is_some_and(|id| !ids.contains(&id)));
    let turrets = turret_active(p, now);
    for slot in 0..COCKPIT_TOTAL_MECHANISM_SLOTS {
        let active = if slot < 5 {
            state.drone_ids[slot].is_some_and(|id| ids.contains(&id))
        } else if slot < 7 {
            turrets
        } else {
            matches!(p.location, Location::Station | Location::Belt)
        };
        let desired = !(state.warp_stowed || slot < 5 && state.retiring_drone_slots[slot])
            && match state.mode(slot).unwrap() {
                CockpitScreenMode::Auto => active,
                CockpitScreenMode::Open => true,
                CockpitScreenMode::Stowed => false,
            };
        let screen = state.screen_mut(slot).unwrap();
        let current = screen.fraction_at(now);
        let target = f64::from(desired);
        if screen.target_deployed != desired || screen.blocked.is_some() {
            screen.from_fraction = current;
            screen.target_deployed = desired;
            screen.started_at_ms = now;
            screen.duration_ms =
                ((target - current).abs() * COCKPIT_SCREEN_TRAVEL_MS as f64).ceil() as u64;
            screen.blocked = ((target - current).abs() > 1e-9 && obstructed(slot, current, target))
                .then(|| "Occupied screen travel envelope".into());
        }
    }
    if *state != before
        || before
            .iter_screens()
            .any(|s| s.moving_at(before.updated_at_ms))
    {
        state.updated_at_ms = now;
    }
}

/// Captain means the authenticated owner of this active hull, not a crew role.
/// This modifies only monitor preferences. The normal authority tick performs
/// collision-checked travel, retirement and warp override in the same transaction.
pub(super) fn set_mode(
    w: &mut World,
    name: &str,
    vessel_id: &str,
    target: CockpitScreenTarget,
    mode: CockpitScreenMode,
    now: u64,
) -> Result<()> {
    let p = &w.pilots.get(name).context("pilot missing")?.view;
    ensure!(
        p.ship_class == ShipClass::Pioneer,
        "captain monitor controls require an active Pioneer"
    );
    let account = w.accounts.get(name).context("owner account missing")?;
    ensure!(
        account.vessel_id(ShipClass::Pioneer) == Some(vessel_id),
        "monitor vessel ownership changed"
    );
    if let Some(avatar) = &p.station_avatar {
        let traversal = avatar
            .docked_traversal
            .as_ref()
            .context("board your own vessel before using monitor controls")?;
        ensure!(
            traversal.binding.host_vessel_id == vessel_id,
            "monitor controls require presence aboard the owned vessel"
        );
    }
    let state = account
        .cockpit_mechanisms
        .as_ref()
        .context("monitor mechanisms unavailable")?;
    ensure!(
        state.vessel_id == vessel_id,
        "monitor vessel binding changed"
    );
    let slots: Vec<usize> = match target {
        CockpitScreenTarget::Slot {
            slot,
            expected_assignment_generation,
        } => {
            let slot = usize::from(slot);
            ensure!(
                slot < COCKPIT_TOTAL_MECHANISM_SLOTS,
                "invalid monitor hardware slot"
            );
            let expected = if slot < 5 {
                Some(state.assignment_generations[slot])
            } else {
                None
            };
            ensure!(
                expected_assignment_generation == expected,
                "monitor source binding changed; refresh controls"
            );
            vec![slot]
        }
        CockpitScreenTarget::All {
            expected_assignment_generations,
        } => {
            ensure!(
                expected_assignment_generations == state.assignment_generations,
                "monitor source bindings changed; refresh controls"
            );
            (0..COCKPIT_TOTAL_MECHANISM_SLOTS).collect()
        }
    };
    let state = w
        .accounts
        .get_mut(name)
        .unwrap()
        .cockpit_mechanisms
        .as_mut()
        .unwrap();
    for slot in slots {
        if state.mode(slot) != Some(mode) {
            *state.mode_mut(slot).unwrap() = mode;
            state.updated_at_ms = now;
        }
    }
    Ok(())
}

/// A restart holds the last persisted pose; elapsed offline wall time is not motor travel.
pub(super) fn stop_on_restart(w: &mut World, now: u64) {
    for a in w.accounts.values_mut() {
        if let Some(state) = &mut a.cockpit_mechanisms {
            let mut changed = false;
            let updated_at_ms = state.updated_at_ms;
            for screen in state.iter_screens_mut() {
                if !screen.moving_at(updated_at_ms) {
                    continue;
                }
                let fraction = screen.fraction_at(updated_at_ms);
                screen.from_fraction = fraction;
                screen.started_at_ms = now;
                screen.duration_ms = 0;
                screen.blocked = Some("Server restarted; awaiting authority reconciliation".into());
                changed = true;
            }
            // A stationary/previously held mechanism is already safe. Reopening
            // a paused realm must not invent a new timestamp or change its save.
            if changed {
                state.updated_at_ms = now;
            }
        }
    }
    for (name, p) in &mut w.pilots {
        p.view.cockpit_mechanisms = (p.view.ship_class == ShipClass::Pioneer)
            .then(|| {
                w.accounts
                    .get(name)
                    .and_then(|a| a.cockpit_mechanisms.clone())
            })
            .flatten();
    }
}

pub(super) fn advance(w: &mut World, now: u64) -> Result<()> {
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    for name in names {
        let p = &w.pilots[&name].view;
        if p.ship_class != ShipClass::Pioneer {
            if let Some(state) = w
                .accounts
                .get_mut(&name)
                .and_then(|a| a.cockpit_mechanisms.as_mut())
            {
                let updated_at_ms = state.updated_at_ms;
                for screen in state.iter_screens_mut() {
                    if screen.moving_at(updated_at_ms) {
                        screen.from_fraction = screen.fraction_at(updated_at_ms);
                        screen.started_at_ms = now;
                        screen.duration_ms = 0;
                        screen.blocked =
                            Some("Inactive vessel; awaiting authority reconciliation".into());
                    }
                }
            }
            w.pilots.get_mut(&name).unwrap().view.cockpit_mechanisms = None;
            continue;
        }
        let Some(vessel) = w
            .accounts
            .get(&name)
            .and_then(|a| a.vessel_id(ShipClass::Pioneer))
            .map(str::to_owned)
        else {
            continue;
        };
        let mut state = w.accounts[&name]
            .cockpit_mechanisms
            .clone()
            .filter(|s| s.vessel_id == vessel)
            .unwrap_or_else(|| PioneerCockpitMechanisms::stowed(vessel.clone(), now));
        let occupants: Vec<_> = w
            .pilots
            .values()
            .filter_map(|p| {
                p.view
                    .station_avatar
                    .as_ref()
                    .and_then(|a| a.docked_traversal.as_ref())
            })
            .filter(|s| s.binding.host_vessel_id == vessel)
            .filter_map(|s| {
                docked_binding_pose(&s.binding)
                    .ok()
                    .map(|pose| pose.station_to_body(s.feet_m))
            })
            .collect();
        reconcile(&mut state, p, now, |slot, from, to| {
            geometry::cockpit_mechanism_sweep(slot, from, to).is_none_or(|bounds| {
                occupants
                    .iter()
                    .any(|feet| geometry::box_intersects_capsule(bounds, *feet, 1.8, 0.3))
            })
        });
        w.accounts.get_mut(&name).unwrap().cockpit_mechanisms = Some(state.clone());
        w.pilots.get_mut(&name).unwrap().view.cockpit_mechanisms = Some(state);
    }
    Ok(())
}

/// Reserve moving travel, not only the instantaneous panel, so an avatar cannot
/// enter the future path after the authority has cleared and started a motor.
pub(super) fn with_collision(
    w: &World,
    name: &str,
    base: &PioneerDockedGeometry,
    now: u64,
    document: Option<&RoomLayoutDocument>,
) -> PioneerDockedGeometry {
    let mut out = base.clone();
    if let Some(state) = w
        .accounts
        .get(name)
        .and_then(|a| a.cockpit_mechanisms.as_ref())
    {
        for (slot, screen) in state.iter_screens().enumerate() {
            let current = screen.fraction_at(now);
            let bounds = if screen.moving_at(now) {
                geometry::cockpit_mechanism_sweep(slot, current, f64::from(screen.target_deployed))
            } else {
                geometry::cockpit_mechanism_box(slot, current)
            };
            if let Some(bounds) = bounds {
                out.panels.push(PioneerDockedPanel {
                    id: format!("cockpit/mechanism/{slot}/reserved"),
                    bounds,
                    material: PioneerDockedMaterial::Graphite,
                    collision: PioneerDockedCollision::Solid,
                    cell: PioneerDockedCell::Cockpit,
                    kind: PioneerDockedPanelKind::Instrument,
                });
            }
        }
    }
    reactor_configuration::with_collision(w, name, &out, now, document)
}

pub(super) fn validate(w: &World) -> Result<()> {
    for a in w.accounts.values() {
        if let Some(s) = &a.cockpit_mechanisms {
            ensure!(
                a.vessel_id(ShipClass::Pioneer) == Some(s.vessel_id.as_str()),
                "cockpit mechanism vessel mismatch"
            );
            let ids: BTreeSet<_> = s.drone_ids.iter().flatten().collect();
            ensure!(
                ids.len() == s.drone_ids.iter().flatten().count(),
                "duplicate cockpit drone assignment"
            );
            for screen in s.iter_screens() {
                ensure!(
                    screen.from_fraction.is_finite()
                        && (0.0..=1.0).contains(&screen.from_fraction)
                        && screen.duration_ms <= COCKPIT_SCREEN_TRAVEL_MS,
                    "invalid cockpit actuator state"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Server, Option<String>, PilotView) {
        let mut server = Server::open(":memory:").unwrap();
        let mut session = None;
        let response = server.handle_at(
            &mut session,
            Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
                pilot: "Mechanism".into(),
                resume_token: None,
                supports_room_controls: false,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                supports_docked_traversal_v2: true,
            },
            1000,
        );
        assert!(response.ok, "{}", response.message);
        let p = response
            .snapshot
            .unwrap()
            .pilots
            .into_iter()
            .find(|p| p.name == "Mechanism")
            .unwrap();
        (server, session, p)
    }
    fn drone(id: u64) -> DroneView {
        DroneView {
            orientation_xyzw: None,
            id,
            class: DroneClass::default(),
            hit_points: 100,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [0.; 3],
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
        }
    }
    fn state() -> PioneerCockpitMechanisms {
        PioneerCockpitMechanisms::stowed("v1:Mechanism:Pioneer".into(), 1000)
    }
    #[test]
    fn sticky_five_slots_retire_before_reassignment_and_report_overflow() {
        let (_, _, mut p) = fixture();
        let mut s = state();
        p.drones = (1..=7).map(drone).collect();
        reconcile(&mut s, &p, 1000, |_, _, _| false);
        assert_eq!(s.drone_ids, [Some(1), Some(2), Some(3), Some(4), Some(5)]);
        assert_eq!(s.unassigned_drone_ids, vec![6, 7]);
        reconcile(&mut s, &p, 1600, |_, _, _| false);
        assert_eq!(s.screens[0].fraction_at(1600), 0.5);
        p.drones.retain(|d| d.id != 2);
        reconcile(&mut s, &p, 1600, |_, _, _| false);
        assert_eq!(s.drone_ids[1], Some(2));
        assert!(!s.screens[1].target_deployed);
        reconcile(&mut s, &p, 1900, |_, _, _| false);
        assert_eq!(s.drone_ids[1], Some(2));
        reconcile(&mut s, &p, 2200, |_, _, _| false);
        assert_eq!(s.drone_ids[1], Some(6));
        assert_eq!(s.assignment_generations[1], 2);
        assert_eq!(s.drone_ids[2], Some(3));
    }
    #[test]
    fn accepted_warp_reverses_all_seven_and_normal_state_reconciles() {
        let (_, _, mut p) = fixture();
        let mut s = state();
        p.location = Location::Belt;
        p.drones = (1..=5).map(drone).collect();
        p.locked_asteroids = vec![1];
        p.mining_modules = vec![MiningModule {
            module_id: 0,
            target_id: Some(1),
            cycle_started_at_ms: Some(1000),
            cycle_completes_at_ms: Some(7000),
            stop_reason: None,
        }];
        reconcile(&mut s, &p, 1000, |_, _, _| false);
        assert!(s.screens.iter().all(|s| s.target_deployed));
        p.location = Location::InTransit {
            destination: Destination::Station,
            arrives_at_ms: 5000,
        };
        reconcile(&mut s, &p, 1600, |_, _, _| false);
        assert!(s.warp_stowed);
        assert!(s
            .screens
            .iter()
            .all(|s| !s.target_deployed && s.fraction_at(1600) == 0.5));
        assert!(s.screens.iter().all(|s| s.fraction_at(2200) == 0.));
        p.location = Location::Station;
        reconcile(&mut s, &p, 2200, |_, _, _| false);
        assert!(!s.warp_stowed);
        assert!(s.screens.iter().all(|s| s.target_deployed));
    }
    #[test]
    fn obstruction_freezes_then_retries_without_jumping() {
        let (_, _, mut p) = fixture();
        let mut s = state();
        p.drones = vec![drone(7)];
        reconcile(&mut s, &p, 1000, |slot, _, _| slot == 0);
        assert!(s.screens[0].blocked.is_some());
        assert_eq!(s.screens[0].fraction_at(9000), 0.);
        reconcile(&mut s, &p, 3000, |_, _, _| false);
        assert_eq!(s.screens[0].fraction_at(3000), 0.);
        assert_eq!(s.screens[0].fraction_at(3600), 0.5);
    }
    #[test]
    fn equal_time_changes_apply_but_unchanged_poll_does_not_rewrite() {
        let (_, _, mut p) = fixture();
        let mut s = state();
        reconcile(&mut s, &p, 1000, |_, _, _| false);
        let before = s.clone();
        reconcile(&mut s, &p, 2000, |_, _, _| false);
        assert_eq!(s, before);
        p.drones = vec![drone(1)];
        reconcile(&mut s, &p, 2000, |_, _, _| false);
        p.drones.push(drone(2));
        reconcile(&mut s, &p, 2000, |_, _, _| false);
        assert_eq!(s.drone_ids[1], Some(2));
        let bytes = serde_json::to_vec(&s).unwrap();
        let restored: PioneerCockpitMechanisms = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.fractions_at(2600), s.fractions_at(2600));
    }
    #[test]
    fn moving_envelope_is_reserved_and_released_at_stow() {
        let (server, _, p) = fixture();
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut w: World = serde_json::from_str(&json).unwrap();
        let mut s = state();
        let mut p = p;
        p.drones = vec![drone(1)];
        reconcile(&mut s, &p, 1000, |_, _, _| false);
        w.accounts.get_mut("Mechanism").unwrap().cockpit_mechanisms = Some(s.clone());
        let g = with_collision(
            &w,
            "Mechanism",
            &PioneerDockedGeometry::default(),
            1100,
            None,
        );
        let bounds = g.panels[0].bounds;
        let deployed = geometry::cockpit_mechanism_box(0, 1.).unwrap();
        assert!(
            bounds.center_m[1] - bounds.half_m[1]
                <= deployed.center_m[1] - deployed.half_m[1] + 1e-9
        );
        p.drones.clear();
        reconcile(&mut s, &p, 2200, |_, _, _| false);
        w.accounts.get_mut("Mechanism").unwrap().cockpit_mechanisms = Some(s);
        let g = with_collision(
            &w,
            "Mechanism",
            &PioneerDockedGeometry::default(),
            3400,
            None,
        );
        assert!(g.panels[0].bounds.center_m[1] - g.panels[0].bounds.half_m[1] > 2.2);
    }
    #[test]
    fn rejected_warp_does_not_mutate_durable_mechanism() {
        let (mut server, mut session, _) = fixture();
        let read = |s: &Server| -> String {
            s.db.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
                .unwrap()
        };
        let before = read(&server);
        let response = server.handle_at(
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            1001,
        );
        assert!(!response.ok);
        assert_eq!(read(&server), before);
        assert!(server.handle_at(&mut session, Request::Undock, 1001).ok);
        assert!(
            server
                .handle_at(
                    &mut session,
                    Request::Warp {
                        destination: Destination::Belt
                    },
                    1002
                )
                .ok
        );
        let w: World = serde_json::from_str(&read(&server)).unwrap();
        assert!(
            w.accounts["Mechanism"]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .warp_stowed
        );
        let response = server.handle_at(&mut session, Request::Snapshot, 1002);
        assert!(
            response.snapshot.unwrap().pilots[0]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .warp_stowed
        );
    }
    #[test]
    fn sqlite_restart_holds_last_persisted_midstroke_pose() {
        let (server, _, mut p) = fixture();
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut w: World = serde_json::from_str(&json).unwrap();
        let mut s = state();
        p.drones = vec![drone(1)];
        reconcile(&mut s, &p, 1000, |_, _, _| false);
        reconcile(&mut s, &p, 1600, |_, _, _| false);
        w.accounts.get_mut("Mechanism").unwrap().cockpit_mechanisms = Some(s.clone());
        w.pilots
            .get_mut("Mechanism")
            .unwrap()
            .view
            .cockpit_mechanisms = Some(s);
        let directory = std::env::temp_dir().join(format!(
            "arkline-screen-restart-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("world.sqlite");
        let disk = Server::open(&path).unwrap();
        disk.db
            .execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&w).unwrap()],
            )
            .unwrap();
        drop(disk);
        let disk = Server::open(&path).unwrap();
        let json: String = disk
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let restored: World = serde_json::from_str(&json).unwrap();
        let screen = &restored.accounts["Mechanism"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .screens[0];
        assert_eq!(screen.from_fraction, 0.5);
        assert!(screen.blocked.is_some());
        assert_eq!(screen.fraction_at(u64::MAX), 0.5);
        assert_eq!(
            restored.pilots["Mechanism"].view.cockpit_mechanisms,
            restored.accounts["Mechanism"].cockpit_mechanisms
        );
    }
    #[test]
    fn constructed_registered_avatar_intrusion_blocks_authoritative_screen_sweep() {
        let (mut server, mut session, _) = fixture();
        let binding = docked_traversal_binding(
            "v1:Mechanism:Pioneer".into(),
            &default_pioneer_room_layout(),
            FleetLayoutRevision::LinearV1,
            false,
        );
        let response = server.handle_at(
            &mut session,
            Request::DockedTraversal {
                request: DockedTraversalRequest::LeaveSeat { binding },
            },
            1001,
        );
        assert!(response.ok, "{}", response.message);
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut w: World = serde_json::from_str(&json).unwrap();
        let p = &mut w.pilots.get_mut("Mechanism").unwrap().view;
        p.drones = vec![drone(1)];
        let state = p
            .station_avatar
            .as_mut()
            .unwrap()
            .docked_traversal
            .as_mut()
            .unwrap();
        let pose = docked_binding_pose(&state.binding).unwrap();
        let (center, _, _) = geometry::cockpit_mechanism_screen(0, 1.).unwrap();
        // Construct an intrusion in the registered mechanism envelope to isolate
        // stop/clear/restart behavior. This is not a reachable grounded walking
        // pose: the normal aisles keep capsules clear of the protected console.
        // Bind the probe to the screen, not the pre-raised bridge's absolute Y.
        state.feet_m = pose.body_to_station([center[0], center[1] - 1.45, center[2]]);
        advance(&mut w, 1001).unwrap();
        let held = w.accounts["Mechanism"].cockpit_mechanisms.as_ref().unwrap();
        assert!(held.screens[0].blocked.is_some());
        assert_eq!(held.screens[0].fraction_at(9999), 0.);
        w.pilots.get_mut("Mechanism").unwrap().view.station_avatar = None;
        advance(&mut w, 1200).unwrap();
        let clear = w.accounts["Mechanism"].cockpit_mechanisms.as_ref().unwrap();
        assert!(clear.screens[0].blocked.is_none());
        assert_eq!(clear.screens[0].fraction_at(1800), 0.5);
    }
    #[test]
    fn actual_five_mining_launches_two_module_cycles_and_warp_drive_seven_screens() {
        let (mut server, mut session, _) = fixture();
        let proof = std::env::var_os("ARKLINE_COCKPIT_AUTHORITY_PROOF_DIR").map(|path| {
            let path = std::path::PathBuf::from(path);
            let root = std::env::var_os("ARKLINE_PROOF_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .canonicalize()
                .unwrap();
            assert!(
                path.parent()
                    .unwrap()
                    .canonicalize()
                    .unwrap()
                    .starts_with(&root),
                "Proof export must be a fresh scratch directory"
            );
            std::fs::create_dir(&path).expect("Proof directory must not already exist");
            path
        });
        let capture = |name: &str, snapshot: &Snapshot| {
            if let Some(path) = &proof {
                std::fs::write(
                    path.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(snapshot).unwrap(),
                )
                .unwrap();
            }
        };
        fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
            let response = s.handle_at(p, r, t);
            assert!(response.ok, "{}", response.message);
            response.snapshot.unwrap()
        }
        ok(&mut server, &mut session, Request::Undock, 1001);
        let transit = ok(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            1002,
        );
        let Location::InTransit { arrives_at_ms, .. } = transit.pilots[0].location else {
            panic!()
        };
        let arrived = ok(&mut server, &mut session, Request::Snapshot, arrives_at_ms);
        let position = arrived.pilots[0].flight.as_ref().unwrap().position;
        let target = arrived
            .asteroids
            .iter()
            .filter(|a| a.system_id == HOME_SYSTEM_ID)
            .min_by(|a, b| {
                flight::distance(position, a.position)
                    .total_cmp(&flight::distance(position, b.position))
            })
            .unwrap()
            .id;
        let target = std::env::var("ARKLINE_COCKPIT_PROOF_TARGET_ASTEROID")
            .ok()
            .map(|id| {
                id.parse::<u64>()
                    .expect("Proof target must be an asteroid ID")
            })
            .unwrap_or(target);
        assert!(
            arrived.asteroids.iter().any(|a| a.id == target),
            "Proof target must exist in actual authority catalog"
        );
        let lock = ok(
            &mut server,
            &mut session,
            Request::LockAsteroid { id: target },
            arrives_at_ms,
        );
        let ready = lock.pilots[0].target_lock.as_ref().unwrap().completes_at_ms;
        let prelaunch = ok(&mut server, &mut session, Request::Snapshot, ready);
        capture("01-prelaunch-stowed", &prelaunch);
        for _ in 0..5 {
            ok(
                &mut server,
                &mut session,
                Request::LaunchDroneClass {
                    class: DroneClass::Mining,
                },
                ready,
            );
        }
        let launched = ok(&mut server, &mut session, Request::Snapshot, ready);
        for id in launched.pilots[0].drones.iter().map(|d| d.id) {
            ok(
                &mut server,
                &mut session,
                Request::AssignMiningDrone {
                    id,
                    target_id: target,
                },
                ready,
            );
        }
        for module_id in [0, 1] {
            ok(
                &mut server,
                &mut session,
                Request::StartMining {
                    module_id,
                    target_id: target,
                },
                ready,
            );
        }
        let launch = ok(&mut server, &mut session, Request::Snapshot, ready);
        capture("02-confirmed-launch-start", &launch);
        let deployment_frames: Vec<_> = (0..=12)
            .map(|frame| {
                let snapshot = ok(
                    &mut server,
                    &mut session,
                    Request::Snapshot,
                    ready + frame * 100,
                );
                capture(&format!("motion-deploy-{frame:02}"), &snapshot);
                snapshot
            })
            .collect();
        let middle = deployment_frames[6].clone();
        capture("03-deploy-half", &middle);
        let m = middle.pilots[0].cockpit_mechanisms.as_ref().unwrap();
        assert_eq!(m.drone_ids.iter().flatten().count(), 5);
        assert_eq!(m.fractions_at(ready + 600), [0.5; 7]);
        let deployed = deployment_frames[12].clone();
        capture("04-full-mining-deployed", &deployed);
        assert_eq!(
            deployed.pilots[0]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .fractions_at(ready + 1200),
            [1.; 7]
        );
        // Real accepted authority samples for capture-age/performance evidence;
        // no renderer-side retimestamping of a frozen world snapshot.
        for sample in 1..=60 {
            let steady = ok(
                &mut server,
                &mut session,
                Request::Snapshot,
                ready + 1200 + sample * 40,
            );
            capture(&format!("steady-{sample:02}"), &steady);
        }
        let warp_started = ready + 4000;
        let warp = ok(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Station,
            },
            warp_started,
        );
        capture("05-accepted-warp-start", &warp);
        let m = warp.pilots[0].cockpit_mechanisms.as_ref().unwrap();
        assert!(m.warp_stowed);
        assert!(m.screens.iter().all(|s| !s.target_deployed));
        let retraction_frames: Vec<_> = (0..=12)
            .map(|frame| {
                let snapshot = ok(
                    &mut server,
                    &mut session,
                    Request::Snapshot,
                    warp_started + frame * 100,
                );
                capture(&format!("motion-warp-{frame:02}"), &snapshot);
                snapshot
            })
            .collect();
        let warp_half = retraction_frames[6].clone();
        capture("06-warp-retract-half", &warp_half);
        assert_eq!(
            warp_half.pilots[0]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .fractions_at(warp_started + 600),
            [0.5; 7]
        );
        let stowed = retraction_frames[12].clone();
        capture("07-warp-stowed", &stowed);
        assert_eq!(
            stowed.pilots[0]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .fractions_at(warp_started + 1200),
            [0.; 7]
        );
        assert!(
            stowed.pilots[0].drones.is_empty(),
            "Existing warp invalidation is not a recovery guarantee"
        );

        let Location::InTransit {
            arrives_at_ms: returned_at,
            ..
        } = warp.pilots[0].location
        else {
            panic!()
        };
        let returned = ok(&mut server, &mut session, Request::Snapshot, returned_at);
        capture("08-normal-arrival-no-drones", &returned);
        assert_eq!(returned.pilots[0].location, Location::Station);
        assert!(
            !returned.pilots[0]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .warp_stowed
        );
        if let Some(path) = &proof {
            std::fs::write(path.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
                "scope":"Fresh in-memory authority fixture; actual accepted Requests; no production/user world",
                "registration_revision":PIONEER_DOCKED_REGISTRATION_REVISION,
                "pilot":"Mechanism", "target_asteroid":target, "deployment_started_at_ms":ready,
                "warp_started_at_ms":warp_started,"returned_at_ms":returned_at,
                "motion_sample_interval_ms":100,"motion_frames_per_phase":13,
                "events":["Undock","Warp(Belt)","LockAsteroid","5x LaunchDroneClass(Mining)","5x AssignMiningDrone","StartMining(0)","StartMining(1)","Warp(Station)"],
                "snapshots_are_unmodified_authoritative_responses":true,
                "caveat":"Existing warp invalidates drones, not secured recall; arrival has no deployed drone or active module request"
            })).unwrap()).unwrap();
            println!("Authoritative cockpit proof: {}", path.display());
        }
    }
    #[test]
    fn inactive_vessel_holds_instead_of_completing_unobserved_travel() {
        let (server, _, _) = fixture();
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut w: World = serde_json::from_str(&json).unwrap();
        w.pilots.get_mut("Mechanism").unwrap().view.drones = vec![drone(1)];
        advance(&mut w, 1000).unwrap();
        advance(&mut w, 1600).unwrap();
        w.pilots.get_mut("Mechanism").unwrap().view.ship_class = ShipClass::Prospector;
        advance(&mut w, 1600).unwrap();
        advance(&mut w, 10000).unwrap();
        let held = w.accounts["Mechanism"].cockpit_mechanisms.as_ref().unwrap();
        assert_eq!(held.screens[0].fraction_at(10000), 0.5);
        assert!(w.pilots["Mechanism"].view.cockpit_mechanisms.is_none());
        w.pilots.get_mut("Mechanism").unwrap().view.ship_class = ShipClass::Pioneer;
        advance(&mut w, 10000).unwrap();
        let restored = w.pilots["Mechanism"]
            .view
            .cockpit_mechanisms
            .as_ref()
            .unwrap();
        assert_eq!(restored.screens[0].fraction_at(10000), 0.5);
        assert_eq!(restored.screens[0].fraction_at(10300), 0.75);
    }
    fn editor_fixture() -> (crate::editor::EditorWorld, Option<String>, String) {
        let path = std::env::temp_dir().join(format!(
            "arkline-screen-clock-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut world = crate::editor::EditorWorld::open_with_creation_scene(
            &path,
            SceneLayoutRevision::LegacyV1,
        )
        .unwrap();
        let mut session = None;
        let response = world.handle(&mut session, connect_screen_test(None));
        assert!(response.ok, "{}", response.message);
        let token = response.resume_token.unwrap();
        assert!(world.handle(&mut session, Request::Undock).ok);
        assert!(
            world
                .handle(
                    &mut session,
                    Request::LaunchDroneClass {
                        class: DroneClass::Mining
                    }
                )
                .ok
        );
        (world, session, token)
    }
    fn connect_screen_test(token: Option<String>) -> Request {
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            pilot: "ScreenClock".into(),
            resume_token: token,
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: true,
        }
    }
    fn editor_screen_state(
        world: &mut crate::editor::EditorWorld,
        session: &mut Option<String>,
    ) -> PioneerCockpitMechanisms {
        let response = world.handle(session, Request::Snapshot);
        assert!(response.ok, "{}", response.message);
        response
            .snapshot
            .unwrap()
            .pilots
            .into_iter()
            .find(|p| p.name == "ScreenClock")
            .unwrap()
            .cockpit_mechanisms
            .unwrap()
    }
    #[test]
    fn mechanism_editor_pause_ignores_wall_clock_and_resume_advances_authority() {
        let (mut world, mut session, _) = editor_fixture();
        for _ in 0..3 {
            world.step(200).unwrap();
        }
        let paused_at = world.time();
        let before = editor_screen_state(&mut world, &mut session);
        assert_eq!(before.screens[0].fraction_at(paused_at), 0.5);
        world.pause();
        std::thread::sleep(std::time::Duration::from_millis(30));
        world.advance(u64::MAX).unwrap();
        let paused = editor_screen_state(&mut world, &mut session);
        assert_eq!(world.time(), paused_at);
        assert_eq!(paused, before);
        world.play();
        world.advance(200).unwrap();
        let resumed = editor_screen_state(&mut world, &mut session);
        assert_eq!(world.time(), paused_at + 200);
        assert!((resumed.screens[0].fraction_at(world.time()) - 2. / 3.).abs() < 1e-9);
        assert_eq!(resumed.drone_ids, before.drone_ids);
        assert_eq!(
            resumed.assignment_generations,
            before.assignment_generations
        );
        assert_eq!(
            resumed.screens[0].started_at_ms,
            before.screens[0].started_at_ms
        );
    }
    #[test]
    fn mechanism_reconnect_reconciles_actual_removed_drone_without_launch_replay() {
        let (mut world, mut session, token) = editor_fixture();
        for _ in 0..3 {
            world.step(200).unwrap();
        }
        let now = world.time();
        let before = editor_screen_state(&mut world, &mut session);
        assert_eq!(before.screens[0].fraction_at(now), 0.5);
        world.disconnect(&mut session);
        assert!(session.is_none());
        let response = world.handle(&mut session, connect_screen_test(Some(token)));
        assert!(response.ok, "{}", response.message);
        let p = response
            .snapshot
            .unwrap()
            .pilots
            .into_iter()
            .find(|p| p.name == "ScreenClock")
            .unwrap();
        // Existing disconnect semantics remove drones. Do not pretend it recalled
        // them, preserve retiring mount identity until its actual retraction ends.
        assert!(p.drones.is_empty());
        let restored = p.cockpit_mechanisms.unwrap();
        assert_eq!(restored.drone_ids, before.drone_ids);
        assert_eq!(
            restored.assignment_generations,
            before.assignment_generations
        );
        assert_eq!(restored.screens[0].fraction_at(now), 0.5);
        assert!(!restored.screens[0].target_deployed);
        let repeated = editor_screen_state(&mut world, &mut session);
        assert_eq!(repeated, restored);
        for _ in 0..2 {
            world.step(150).unwrap();
        }
        let midway = editor_screen_state(&mut world, &mut session);
        assert_eq!(midway.screens[0].fraction_at(world.time()), 0.25);
        for _ in 0..2 {
            world.step(150).unwrap();
        }
        let retracted = editor_screen_state(&mut world, &mut session);
        assert_eq!(retracted.screens[0].fraction_at(world.time()), 0.);
        assert!(retracted.drone_ids[0].is_none());
        assert_eq!(
            retracted.assignment_generations,
            before.assignment_generations
        );
    }
    #[test]
    fn real_owned_routine_allows_standing_with_seven_active_screen_sources() {
        let (mut server, mut session, _) = fixture();
        fn ok(
            server: &mut Server,
            session: &mut Option<String>,
            request: Request,
            time: u64,
        ) -> Snapshot {
            let response = server.handle_at(session, request, time);
            assert!(response.ok, "{}", response.message);
            response.snapshot.unwrap()
        }
        ok(&mut server, &mut session, Request::Undock, 1001);
        ok(
            &mut server,
            &mut session,
            Request::StartResourceJob {
                pilot: None,
                target_units: 5000,
            },
            1002,
        );
        let mut now = 1002;
        let mut active = None;
        for _ in 0..2400 {
            now += 250;
            server.tick_at(now).unwrap();
            let snapshot = ok(&mut server, &mut session, Request::Snapshot, now);
            let p = snapshot
                .pilots
                .iter()
                .find(|p| p.name == "Mechanism")
                .unwrap();
            if p.drones.len() == 5
                && p.mining_modules
                    .iter()
                    .filter(|m| m.target_id.is_some())
                    .count()
                    == 2
                && p.cockpit_mechanisms.as_ref().unwrap().fractions_at(now) == [1.; 7]
            {
                active = Some(snapshot);
                break;
            }
        }
        let active =
            active.expect("Actual owned resource routine must deploy five drones and both modules");
        let seated_response = server.handle_at(&mut session, Request::Snapshot, now);
        assert!(seated_response.ok);
        assert_eq!(
            serde_json::to_value(seated_response.snapshot.as_ref().unwrap()).unwrap(),
            serde_json::to_value(&active).unwrap()
        );
        let binding = docked_traversal_binding(
            "v1:Mechanism:Pioneer".into(),
            &default_pioneer_room_layout(),
            FleetLayoutRevision::LinearV1,
            false,
        );
        ok(
            &mut server,
            &mut session,
            Request::DockedTraversal {
                request: DockedTraversalRequest::LeaveSeat { binding },
            },
            now,
        );
        let mut standing = active.clone();
        for _ in 0..DOCKED_SEAT_TRANSITION_MS / 100 {
            now += 100;
            server.tick_at(now).unwrap();
            standing = ok(&mut server, &mut session, Request::Snapshot, now);
        }
        let p = standing
            .pilots
            .iter()
            .find(|p| p.name == "Mechanism")
            .unwrap();
        assert!(p
            .station_avatar
            .as_ref()
            .unwrap()
            .docked_traversal
            .as_ref()
            .unwrap()
            .seat_transition
            .is_none());
        assert_eq!(
            p.cockpit_mechanisms.as_ref().unwrap().fractions_at(now),
            [1.; 7]
        );
        assert_eq!(p.drones.len(), 5);
        assert_eq!(
            p.mining_modules
                .iter()
                .filter(|m| m.target_id.is_some())
                .count(),
            2
        );
        assert!(standing
            .resource_jobs
            .iter()
            .any(|j| j.pilot == "Mechanism" && !j.phase.is_terminal()));
        let standing_response = server.handle_at(&mut session, Request::Snapshot, now);
        assert!(standing_response.ok);
        assert_eq!(
            serde_json::to_value(standing_response.snapshot.as_ref().unwrap()).unwrap(),
            serde_json::to_value(&standing).unwrap()
        );
        if let Some(path) = std::env::var_os("ARKLINE_COCKPIT_ROUTINE_PROOF_DIR") {
            let path = std::path::PathBuf::from(path);
            let root = std::env::var_os("ARKLINE_PROOF_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .canonicalize()
                .unwrap();
            assert!(path
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .starts_with(root));
            std::fs::create_dir(&path).expect("Proof directory must be fresh");
            for (name, snapshot) in [
                ("routine-seated-seven", &active),
                ("routine-standing-seven", &standing),
            ] {
                std::fs::write(
                    path.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(snapshot).unwrap(),
                )
                .unwrap();
            }
            for (name, response) in [
                ("routine-seated-response", &seated_response),
                ("routine-standing-response", &standing_response),
            ] {
                std::fs::write(
                    path.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(response).unwrap(),
                )
                .unwrap();
            }
            std::fs::write(path.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
                "scope":"Fresh in-memory real authority Request routine, no synthetic avatar or source changes",
                "requests":["Undock","StartResourceJob(target_units=5000)","authority ticks until five drones/two modules/seven deployed","DockedTraversal::LeaveSeat","900ms authoritative seat transition"],
                "registration_revision":PIONEER_DOCKED_REGISTRATION_REVISION,
                "seated_time_ms":active.server_time_ms,"standing_time_ms":standing.server_time_ms,
                "manual_flight_interlock_unchanged":true
            })).unwrap()).unwrap();
            println!("Actual routine standing proof: {}", path.display());
        }
    }
    #[test]
    fn restart_preserves_stationary_and_already_blocked_mechanisms_exactly() {
        let (server, _, _) = fixture();
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        for mode in 0..3 {
            let mut w: World = serde_json::from_str(&json).unwrap();
            let mut state = state();
            if mode == 1 {
                state.screens[0].from_fraction = 1.;
                state.screens[0].target_deployed = true;
            }
            if mode == 2 {
                state.screens[0].from_fraction = 0.5;
                state.screens[0].target_deployed = true;
                state.screens[0].blocked = Some("Occupied screen travel envelope".into());
            }
            w.accounts.get_mut("Mechanism").unwrap().cockpit_mechanisms = Some(state.clone());
            w.pilots
                .get_mut("Mechanism")
                .unwrap()
                .view
                .cockpit_mechanisms = Some(state);
            let before = serde_json::to_vec(&w.accounts["Mechanism"].cockpit_mechanisms).unwrap();
            stop_on_restart(&mut w, 99999);
            assert_eq!(
                serde_json::to_vec(&w.accounts["Mechanism"].cockpit_mechanisms).unwrap(),
                before
            );
            assert_eq!(
                w.pilots["Mechanism"].view.cockpit_mechanisms,
                w.accounts["Mechanism"].cockpit_mechanisms
            );
        }
    }

    fn captain_world(server: &Server) -> World {
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        serde_json::from_str(&json).unwrap()
    }
    fn captain_request(
        state: &PioneerCockpitMechanisms,
        slot: Option<u8>,
        mode: CockpitScreenMode,
    ) -> Request {
        Request::SetCockpitScreenMode {
            vessel_id: state.vessel_id.clone(),
            target: match slot {
                Some(slot) => CockpitScreenTarget::Slot {
                    slot,
                    expected_assignment_generation: if slot < 5 {
                        Some(state.assignment_generations[usize::from(slot)])
                    } else {
                        None
                    },
                },
                None => CockpitScreenTarget::All {
                    expected_assignment_generations: state.assignment_generations,
                },
            },
            mode,
        }
    }
    fn captain_ok(
        server: &mut Server,
        session: &mut Option<String>,
        request: Request,
        now: u64,
    ) -> Response {
        let response = server.handle_at(session, request, now);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn captain_state(response: &Response) -> &PioneerCockpitMechanisms {
        response
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Mechanism")
            .unwrap()
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
    }
    /// Complete, unmodified authority receipts for matched native frame timing.
    #[test]
    #[ignore = "requires fresh ARKLINE_NINE_FEED_PROFILE_RESPONSES directory"]
    fn export_actual_all_nine_steady_mining_responses() {
        let path = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_NINE_FEED_PROFILE_RESPONSES")
                .expect("explicit fresh proof directory required"),
        );
        let root = std::env::var_os("ARKLINE_PROOF_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .canonicalize()
            .unwrap();
        assert!(path
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .starts_with(root));
        std::fs::create_dir(&path).expect("proof directory must be new");
        let (mut server, mut session, _) = fixture();
        captain_ok(&mut server, &mut session, Request::Undock, 1001);
        captain_ok(
            &mut server,
            &mut session,
            Request::StartResourceJob {
                pilot: None,
                target_units: 5000,
            },
            1002,
        );
        let mut now = 1002;
        let mut live = None;
        for _ in 0..2400 {
            now += 250;
            server.tick_at(now).unwrap();
            let response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            let own = response
                .snapshot
                .as_ref()
                .unwrap()
                .pilots
                .iter()
                .find(|p| p.name == "Mechanism")
                .unwrap();
            if own.drones.len() == 5
                && own
                    .mining_modules
                    .iter()
                    .filter(|m| m.target_id.is_some())
                    .count()
                    == 2
                && captain_state(&response).all_fractions_at(now) == [1.; 9]
            {
                live = Some(response);
                break;
            }
        }
        let live =
            live.expect("actual five-drone/two-turret mining with all nine displays deployed");
        let initial = live.snapshot.as_ref().unwrap();
        let own = initial
            .pilots
            .iter()
            .find(|p| p.name == "Mechanism")
            .unwrap();
        let mut drone_ids: Vec<_> = own.drones.iter().map(|d| d.id).collect();
        drone_ids.sort_unstable();
        let job_id = initial
            .resource_jobs
            .iter()
            .find(|j| !j.phase.is_terminal())
            .expect("active owned resource job")
            .id;
        let initial_ms = now;
        std::fs::write(
            path.join("initial-response.json"),
            serde_json::to_vec_pretty(&live).unwrap(),
        )
        .unwrap();
        let mut records = Vec::new();
        for index in 1..=80 {
            now += 40;
            server.tick_at(now).unwrap();
            let response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            let snapshot = response.snapshot.as_ref().unwrap();
            assert_eq!(snapshot.server_time_ms, now);
            let own = snapshot
                .pilots
                .iter()
                .find(|p| p.name == "Mechanism")
                .unwrap();
            let mut current_ids: Vec<_> = own.drones.iter().map(|d| d.id).collect();
            current_ids.sort_unstable();
            assert_eq!(
                current_ids, drone_ids,
                "same five actual drones at sample {index}"
            );
            assert_eq!(
                own.mining_modules
                    .iter()
                    .filter(|m| m.target_id.is_some())
                    .count(),
                2
            );
            assert_eq!(captain_state(&response).all_fractions_at(now), [1.; 9]);
            assert!(snapshot
                .resource_jobs
                .iter()
                .any(|j| j.id == job_id && !j.phase.is_terminal()));
            let file = format!("steady-{index:02}-response.json");
            std::fs::write(
                path.join(&file),
                serde_json::to_vec_pretty(&response).unwrap(),
            )
            .unwrap();
            records.push(serde_json::json!({"file":file,"server_ms":now}));
        }
        std::fs::write(path.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "pilot":"Mechanism", "registration":PIONEER_DOCKED_REGISTRATION_REVISION,
            "protocol_version":PROTOCOL_VERSION,"initial_ms":initial_ms,"final_ms":now,
            "sample_count":80,"step_ms":40,"drone_ids":drone_ids,"job_id":job_id,
            "all_nine_deployed_each_sample":true,"two_targeted_modules_each_sample":true,
            "unmodified_authoritative_responses":true,"records":records,
            "scope":"Fresh in-memory authority; ordinary Connect/Undock/StartResourceJob and actual ticks/Snapshot. No source world edits or response retimestamping. Replay timing is not live network cadence."
        })).unwrap()).unwrap();
        println!("All-nine steady authority responses: {}", path.display());
    }

    #[test]
    fn captain_modes_preserve_real_resource_job_equipment_and_warp_priority() {
        let (mut server, mut session, _) = fixture();
        captain_ok(&mut server, &mut session, Request::Undock, 1001);
        captain_ok(
            &mut server,
            &mut session,
            Request::StartResourceJob {
                pilot: None,
                target_units: 5000,
            },
            1002,
        );
        let mut now = 1002;
        let mut live = None;
        for _ in 0..2400 {
            now += 250;
            server.tick_at(now).unwrap();
            let response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            let p = &response.snapshot.as_ref().unwrap().pilots[0];
            if p.drones.len() == 5
                && p.mining_modules
                    .iter()
                    .filter(|m| m.target_id.is_some())
                    .count()
                    == 2
                && captain_state(&response).all_fractions_at(now) == [1.; 9]
            {
                live = Some(response);
                break;
            }
        }
        let mut live = live.expect("real owned routine reaches five drones and two modules");
        let proof = std::env::var_os("ARKLINE_CAPTAIN_AUTHORITY_PROOF_DIR").map(|p| {
            let p = std::path::PathBuf::from(p);
            let root = std::env::var_os("ARKLINE_PROOF_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .canonicalize()
                .unwrap();
            assert!(p
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .starts_with(root));
            std::fs::create_dir(&p).expect("proof path must be new");
            p
        });
        let capture = |label: &str, r: &Response| {
            if let Some(path) = &proof {
                std::fs::write(
                    path.join(format!("{label}-response.json")),
                    serde_json::to_vec_pretty(r).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    path.join(format!("{label}.json")),
                    serde_json::to_vec_pretty(r.snapshot.as_ref().unwrap()).unwrap(),
                )
                .unwrap();
            }
        };
        capture("01-auto-mining", &live);
        for (label, slot, mode) in [
            ("02-individual-stowed", Some(0), CockpitScreenMode::Stowed),
            ("03-individual-open", Some(0), CockpitScreenMode::Open),
            ("04-all-stowed", None, CockpitScreenMode::Stowed),
            ("05-all-open", None, CockpitScreenMode::Open),
            ("06-all-auto", None, CockpitScreenMode::Auto),
        ] {
            let before = captain_world(&server);
            let command = captain_request(captain_state(&live), slot, mode);
            live = captain_ok(&mut server, &mut session, command, now);
            let after = captain_world(&server);
            assert_eq!(
                serde_json::to_value(&after.resource_jobs).unwrap(),
                serde_json::to_value(&before.resource_jobs).unwrap(),
                "monitor control must not cancel or replace real job"
            );
            let a = &after.pilots["Mechanism"].view;
            let b = &before.pilots["Mechanism"].view;
            assert_eq!(
                serde_json::to_value(&a.drones).unwrap(),
                serde_json::to_value(&b.drones).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&a.mining_modules).unwrap(),
                serde_json::to_value(&b.mining_modules).unwrap()
            );
            assert_eq!(
                serde_json::to_value(&a.flight).unwrap(),
                serde_json::to_value(&b.flight).unwrap()
            );
            assert_eq!(a.power, b.power);
            assert_eq!(
                a.cockpit_mechanisms.as_ref().unwrap().drone_ids,
                b.cockpit_mechanisms.as_ref().unwrap().drone_ids
            );
            capture(label, &live);
        }
        // Prove actual travel while the same owned job continues, not only a mode flag.
        let command = captain_request(captain_state(&live), None, CockpitScreenMode::Stowed);
        live = captain_ok(&mut server, &mut session, command, now);
        let job_id = live.snapshot.as_ref().unwrap().resource_jobs[0].id;
        for step in 0..=12 {
            if step > 0 {
                now += 100;
                server.tick_at(now).unwrap();
            }
            live = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            assert!(live
                .snapshot
                .as_ref()
                .unwrap()
                .resource_jobs
                .iter()
                .any(|j| j.id == job_id && !j.phase.is_terminal()));
            capture(&format!("stow-{step:02}"), &live);
        }
        assert_eq!(captain_state(&live).all_fractions_at(now), [0.; 9]);
        let command = captain_request(captain_state(&live), None, CockpitScreenMode::Open);
        live = captain_ok(&mut server, &mut session, command, now);
        for step in 0..=12 {
            if step > 0 {
                now += 100;
                server.tick_at(now).unwrap();
            }
            live = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            capture(&format!("open-{step:02}"), &live);
        }
        assert_eq!(captain_state(&live).all_fractions_at(now), [1.; 9]);
        // Warp has its pre-existing job/drone semantics; this is not caused by monitor commands.
        live = captain_ok(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Station,
            },
            now,
        );
        assert!(captain_state(&live).warp_stowed);
        assert_eq!(captain_state(&live).modes, [CockpitScreenMode::Open; 7]);
        assert_eq!(
            captain_state(&live).external_modes,
            [CockpitScreenMode::Open; 2]
        );
        assert!(captain_state(&live)
            .iter_screens()
            .all(|s| !s.target_deployed));
        capture("07-accepted-warp", &live);
        let Location::InTransit { arrives_at_ms, .. } =
            live.snapshot.as_ref().unwrap().pilots[0].location
        else {
            panic!()
        };
        for step in 0..=12 {
            if step > 0 {
                now += 100;
            }
            live = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            capture(&format!("warp-{step:02}"), &live);
        }
        live = captain_ok(&mut server, &mut session, Request::Snapshot, arrives_at_ms);
        assert!(!captain_state(&live).warp_stowed);
        assert_eq!(captain_state(&live).modes, [CockpitScreenMode::Open; 7]);
        assert_eq!(
            captain_state(&live).external_modes,
            [CockpitScreenMode::Open; 2]
        );
        assert!(captain_state(&live)
            .iter_screens()
            .all(|s| s.target_deployed));
        capture("08-normal-reopen", &live);
        // Restore actual AUTO preferences, then observe the completed authority stroke.
        for slot in [7, 8] {
            let request =
                captain_request(captain_state(&live), Some(slot), CockpitScreenMode::Auto);
            live = captain_ok(&mut server, &mut session, request, arrives_at_ms);
        }
        live = captain_ok(
            &mut server,
            &mut session,
            Request::Snapshot,
            arrives_at_ms + 1200,
        );
        assert_eq!(
            captain_state(&live).external_modes,
            [CockpitScreenMode::Auto; 2]
        );
        assert_eq!(
            captain_state(&live).all_fractions_at(arrives_at_ms + 1200),
            [1.; 9]
        );
        capture("09-normal-external-auto-deployed", &live);
        if let Some(path) = proof {
            std::fs::write(path.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"pilot":"Mechanism","vessel_id":"v1:Mechanism:Pioneer","source":"actual Request responses; fresh registration10 fixture","monitor_controls_preserve_job_and_equipment":true,"warp_has_existing_job_cancellation_and_drone_invalidation":true})).unwrap()).unwrap();
        }
    }
    #[test]
    fn captain_stale_or_foreign_commands_are_atomic_and_failed_warp_keeps_preference() {
        let (mut server, mut session, _) = fixture();
        let r = captain_ok(&mut server, &mut session, Request::Snapshot, 1000);
        let state = captain_state(&r).clone();
        let bad = [
            Request::SetCockpitScreenMode {
                vessel_id: "foreign".into(),
                target: CockpitScreenTarget::All {
                    expected_assignment_generations: [0; 5],
                },
                mode: CockpitScreenMode::Stowed,
            },
            Request::SetCockpitScreenMode {
                vessel_id: state.vessel_id.clone(),
                target: CockpitScreenTarget::All {
                    expected_assignment_generations: [1; 5],
                },
                mode: CockpitScreenMode::Stowed,
            },
            Request::SetCockpitScreenMode {
                vessel_id: state.vessel_id.clone(),
                target: CockpitScreenTarget::Slot {
                    slot: 0,
                    expected_assignment_generation: None,
                },
                mode: CockpitScreenMode::Stowed,
            },
            Request::SetCockpitScreenMode {
                vessel_id: state.vessel_id.clone(),
                target: CockpitScreenTarget::Slot {
                    slot: 9,
                    expected_assignment_generation: None,
                },
                mode: CockpitScreenMode::Open,
            },
        ];
        for command in bad {
            let before = serde_json::to_value(captain_world(&server)).unwrap();
            let r = server.handle_at(&mut session, command, 1000);
            assert!(!r.ok);
            assert_eq!(
                serde_json::to_value(captain_world(&server)).unwrap(),
                before
            );
        }
        captain_ok(
            &mut server,
            &mut session,
            captain_request(&state, None, CockpitScreenMode::Open),
            1000,
        );
        let before = serde_json::to_value(captain_world(&server)).unwrap();
        assert!(
            !server
                .handle_at(
                    &mut session,
                    Request::Warp {
                        destination: Destination::Belt
                    },
                    1000
                )
                .ok
        );
        assert_eq!(
            serde_json::to_value(captain_world(&server)).unwrap(),
            before
        );
    }
    #[test]
    fn captain_open_still_retires_old_source_before_rebinding_and_obeys_obstruction() {
        let (_, _, mut p) = fixture();
        let mut s = state();
        s.modes = [CockpitScreenMode::Open; 7];
        p.drones = (1..=5).map(drone).collect();
        reconcile(&mut s, &p, 1000, |_, _, _| false);
        reconcile(&mut s, &p, 2200, |_, _, _| false);
        p.drones = (2..=6).map(drone).collect();
        reconcile(&mut s, &p, 2200, |slot, _, _| slot == 0);
        assert!(s.retiring_drone_slots[0]);
        assert_eq!(s.drone_ids[0], Some(1));
        assert!(s.screens[0].blocked.is_some());
        assert_eq!(s.screens[0].fraction_at(9999), 1.);
        reconcile(&mut s, &p, 2300, |_, _, _| false);
        reconcile(&mut s, &p, 2900, |_, _, _| false);
        assert_eq!(s.drone_ids[0], Some(1));
        assert_eq!(s.assignment_generations[0], 1);
        reconcile(&mut s, &p, 3500, |_, _, _| false);
        assert_eq!(s.drone_ids[0], Some(6));
        assert_eq!(s.assignment_generations[0], 2);
        assert!(!s.retiring_drone_slots[0]);
        assert!(s.screens[0].target_deployed);
        assert_eq!(s.screens[0].fraction_at(3500), 0.);
        assert_eq!(s.modes[0], CockpitScreenMode::Open);
    }
    #[test]
    fn captain_paused_controls_hold_pose_and_restart_preserves_preferences() {
        let (mut world, mut session, _) = editor_fixture();
        for _ in 0..3 {
            world.step(200).unwrap();
        }
        world.pause();
        let before = editor_screen_state(&mut world, &mut session);
        let now = world.time();
        let response = world.handle(
            &mut session,
            captain_request(&before, Some(0), CockpitScreenMode::Stowed),
        );
        assert!(response.ok, "{}", response.message);
        world.advance(u64::MAX).unwrap();
        let held = editor_screen_state(&mut world, &mut session);
        assert_eq!(world.time(), now);
        assert_eq!(held.fractions_at(now), before.fractions_at(now));
        assert_eq!(held.modes[0], CockpitScreenMode::Stowed);
        world.play();
        world.advance(200).unwrap();
        let moving = editor_screen_state(&mut world, &mut session);
        assert!(moving.screens[0].fraction_at(world.time()) < 0.5);
        let (server, _, _) = fixture();
        let mut w = captain_world(&server);
        let state = w
            .accounts
            .get_mut("Mechanism")
            .unwrap()
            .cockpit_mechanisms
            .as_mut()
            .unwrap();
        state.modes = [CockpitScreenMode::Open; 7];
        state.screens[0].target_deployed = true;
        state.screens[0].started_at_ms = 1000;
        state.screens[0].duration_ms = 1200;
        state.updated_at_ms = 1600;
        stop_on_restart(&mut w, 9000);
        let restored = w.accounts["Mechanism"].cockpit_mechanisms.as_ref().unwrap();
        assert_eq!(restored.modes, [CockpitScreenMode::Open; 7]);
        assert_eq!(restored.screens[0].fraction_at(u64::MAX), 0.5);
    }

    #[test]
    fn captain_controls_require_owner_onboard_and_survive_sqlite_reopen() {
        let (mut server, mut session, _) = fixture();
        let binding = docked_traversal_binding(
            "v1:Mechanism:Pioneer".into(),
            &default_pioneer_room_layout(),
            FleetLayoutRevision::LinearV1,
            false,
        );
        captain_ok(
            &mut server,
            &mut session,
            Request::DockedTraversal {
                request: DockedTraversalRequest::LeaveSeat { binding },
            },
            1001,
        );
        let r = captain_ok(&mut server, &mut session, Request::Snapshot, 1001);
        let open = captain_request(captain_state(&r), Some(5), CockpitScreenMode::Open);
        let r = captain_ok(&mut server, &mut session, open, 1001);
        assert_eq!(captain_state(&r).modes[5], CockpitScreenMode::Open);
        let mut w = captain_world(&server);
        let saved = w.clone();
        let vessel = "v1:Mechanism:Pioneer";
        w.pilots
            .get_mut("Mechanism")
            .unwrap()
            .view
            .station_avatar
            .as_mut()
            .unwrap()
            .docked_traversal
            .as_mut()
            .unwrap()
            .binding
            .host_vessel_id = "foreign".into();
        assert!(set_mode(
            &mut w,
            "Mechanism",
            vessel,
            CockpitScreenTarget::Slot {
                slot: 5,
                expected_assignment_generation: None
            },
            CockpitScreenMode::Stowed,
            1001
        )
        .is_err());
        w.pilots
            .get_mut("Mechanism")
            .unwrap()
            .view
            .station_avatar
            .as_mut()
            .unwrap()
            .docked_traversal = None;
        assert!(set_mode(
            &mut w,
            "Mechanism",
            vessel,
            CockpitScreenTarget::Slot {
                slot: 5,
                expected_assignment_generation: None
            },
            CockpitScreenMode::Stowed,
            1001
        )
        .is_err());
        let directory = std::env::temp_dir().join(format!(
            "arkline-captain-restart-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("world.sqlite");
        let disk = Server::open(&path).unwrap();
        disk.db
            .execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&saved).unwrap()],
            )
            .unwrap();
        drop(disk);
        let disk = Server::open(&path).unwrap();
        let restored = captain_world(&disk);
        assert_eq!(
            restored.accounts["Mechanism"]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .modes,
            saved.accounts["Mechanism"]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .modes
        );
    }

    /// Fresh-world cargo evidence obtained only through accepted requests and
    /// authority ticks. No account inventory or rendered response is fabricated.
    #[test]
    #[ignore = "requires explicit fresh ARKLINE_CARGO_AUTHORITY_PROOF_DIR"]
    fn export_actual_mined_cargo_empty_partial_full_and_deposited() {
        let path = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_CARGO_AUTHORITY_PROOF_DIR")
                .expect("explicit proof directory required"),
        );
        let root = std::env::var_os("ARKLINE_PROOF_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .canonicalize()
            .unwrap();
        assert!(path
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .starts_with(root));
        std::fs::create_dir(&path).expect("proof directory must be new");
        fn own(response: &Response) -> &PilotView {
            response
                .snapshot
                .as_ref()
                .unwrap()
                .pilots
                .iter()
                .find(|pilot| pilot.name == "Mechanism")
                .unwrap()
        }
        let mut records = Vec::new();
        let mut capture = |name: &str, response: &Response| {
            std::fs::write(
                path.join(format!("{name}-response.json")),
                serde_json::to_vec_pretty(response).unwrap(),
            )
            .unwrap();
            let snapshot = response.snapshot.as_ref().unwrap();
            std::fs::write(
                path.join(format!("{name}.json")),
                serde_json::to_vec_pretty(snapshot).unwrap(),
            )
            .unwrap();
            let pilot = own(response);
            records.push(serde_json::json!({
                "name":name,
                "server_time_ms":snapshot.server_time_ms,
                "ore":pilot.ore,
                "capacity":cargo_capacity(pilot.ship_class),
                "location":pilot.location,
                "drone_count":pilot.drones.len(),
                "active_mining_modules":pilot.mining_modules.iter().filter(|m|m.target_id.is_some()).count(),
                "monitor_modes":(0..COCKPIT_TOTAL_MECHANISM_SLOTS).map(|slot| captain_state(response).mode(slot).unwrap()).collect::<Vec<_>>(),
                "monitor_fractions":captain_state(response).all_fractions_at(snapshot.server_time_ms),
                "resource_jobs":snapshot.resource_jobs,
                "personal_storage":response.assets.as_ref().map(|assets| &assets.storage),
                "ledger_sequence":snapshot.ledger_sequence,
            }));
        };
        let (mut server, mut session, _) = fixture();
        let empty = captain_ok(&mut server, &mut session, Request::Undock, 1001);
        assert_eq!(own(&empty).ship_class, ShipClass::Pioneer);
        assert_eq!(own(&empty).ore, 0);
        let capacity = cargo_capacity(ShipClass::Pioneer);
        assert_eq!(capacity, 1000);
        capture("01-empty", &empty);
        captain_ok(
            &mut server,
            &mut session,
            Request::StartResourceJob {
                pilot: None,
                target_units: 5000,
            },
            1002,
        );
        let mut now = 1002;
        let mut partial = None;
        for _ in 0..4800 {
            now += 250;
            server.tick_at(now).unwrap();
            let response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            let pilot = own(&response);
            if (200..capacity).contains(&pilot.ore)
                && captain_state(&response).fractions_at(now) == [1.; 7]
            {
                partial = Some(response);
                break;
            }
        }
        let partial = partial.expect("actual resource job mines partial hold");
        capture("02-partial-mining", &partial);
        let job_id = partial.snapshot.as_ref().unwrap().resource_jobs[0].id;
        let command = captain_request(captain_state(&partial), None, CockpitScreenMode::Stowed);
        let before = captain_world(&server);
        let mut stowed = captain_ok(&mut server, &mut session, command, now);
        assert_eq!(captain_state(&stowed).modes, [CockpitScreenMode::Stowed; 7]);
        let after = captain_world(&server);
        assert_eq!(
            serde_json::to_value(&before.resource_jobs).unwrap(),
            serde_json::to_value(&after.resource_jobs).unwrap()
        );
        let a = &before.pilots["Mechanism"].view;
        let b = &after.pilots["Mechanism"].view;
        assert_eq!(a.ore, b.ore);
        assert_eq!(a.drones, b.drones);
        assert_eq!(
            serde_json::to_value(&a.mining_modules).unwrap(),
            serde_json::to_value(&b.mining_modules).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&a.flight).unwrap(),
            serde_json::to_value(&b.flight).unwrap()
        );
        assert_eq!(a.power, b.power);
        for _ in 0..12 {
            now += 100;
            server.tick_at(now).unwrap();
            stowed = captain_ok(&mut server, &mut session, Request::Snapshot, now);
        }
        assert_eq!(captain_state(&stowed).modes, [CockpitScreenMode::Stowed; 7]);
        assert_eq!(captain_state(&stowed).fractions_at(now), [0.; 7]);
        assert!(stowed
            .snapshot
            .as_ref()
            .unwrap()
            .resource_jobs
            .iter()
            .any(|job| job.id == job_id && !job.phase.is_terminal()));
        assert_eq!(own(&stowed).drones.len(), 5);
        assert!(own(&stowed)
            .mining_modules
            .iter()
            .any(|module| module.target_id.is_some()));
        capture("03-partial-all-stowed", &stowed);
        captain_ok(
            &mut server,
            &mut session,
            captain_request(captain_state(&stowed), None, CockpitScreenMode::Auto),
            now,
        );
        let mut full = None;
        let mut deposited = None;
        for _ in 0..4800 {
            now += 250;
            server.tick_at(now).unwrap();
            let response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            assert!(own(&response).ore <= capacity);
            if full.is_none() && own(&response).ore == capacity {
                capture("04-full", &response);
                full = Some(response.clone());
            }
            if full.is_some()
                && own(&response).ore == 0
                && response
                    .snapshot
                    .as_ref()
                    .unwrap()
                    .resource_jobs
                    .iter()
                    .any(|job| job.id == job_id && job.stored_units >= capacity)
            {
                assert_eq!(own(&response).location, Location::Docked);
                assert!(response
                    .assets
                    .as_ref()
                    .unwrap()
                    .storage
                    .iter()
                    .any(|stored| {
                        stored.system_id == HOME_SYSTEM_ID && stored.ore >= capacity
                    }));
                capture("05-deposited-empty", &response);
                deposited = Some(response);
                break;
            }
        }
        let full = full.expect("actual mining reaches full 1000-unit hold");
        let deposited = deposited.expect("actual job deposits first haul atomically");
        assert!(
            deposited.snapshot.as_ref().unwrap().ledger_sequence
                > full.snapshot.as_ref().unwrap().ledger_sequence
        );
        std::fs::write(
            path.join("receipt.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "pilot":"Mechanism",
                "vessel_id":"v1:Mechanism:Pioneer",
                "registration":PIONEER_DOCKED_REGISTRATION_REVISION,
                "setup":"fresh in-memory Server::open; normal Connect defaults only; no inventory, asteroid, location, pose, mechanism or Response edits",
                "requests":["Connect","Undock","StartResourceJob(target_units=5000)","authority ticks + Snapshot until partial ore","SetCockpitScreenMode(All, Stowed)","1200ms authority motion","SetCockpitScreenMode(All, Auto)","authority ticks + Snapshot until 1000 ore and first automatic docked DepositOre"],
                "all_inventory_from_actual_mining":true,
                "monitor_command_preserved_job_equipment_flight_power_and_ore_same_tick":true,
                "unmodified_authoritative_responses":true,
                "records":records,
            }))
            .unwrap(),
        )
        .unwrap();
        println!("Actual cargo authority proof: {}", path.display());
    }

    /// Fresh-world evidence helper: never changes production state or source fields.
    #[test]
    #[ignore = "requires explicit fresh ARKLINE_DRONE_TELEMETRY_PROOF_DIR"]
    fn export_actual_mining_hold_and_returning_telemetry() {
        let path = std::path::PathBuf::from(
            std::env::var_os("ARKLINE_DRONE_TELEMETRY_PROOF_DIR")
                .expect("explicit proof directory required"),
        );
        let root = std::env::var_os("ARKLINE_PROOF_ROOT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .canonicalize()
            .unwrap();
        assert!(path
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap()
            .starts_with(root));
        std::fs::create_dir(&path).expect("proof directory must be new");
        let capture = |name: &str, response: &Response| {
            std::fs::write(
                path.join(format!("{name}-response.json")),
                serde_json::to_vec_pretty(response).unwrap(),
            )
            .unwrap();
            std::fs::write(
                path.join(format!("{name}.json")),
                serde_json::to_vec_pretty(response.snapshot.as_ref().unwrap()).unwrap(),
            )
            .unwrap();
        };
        let (mut server, mut session, _) = fixture();
        captain_ok(&mut server, &mut session, Request::Undock, 1001);
        captain_ok(
            &mut server,
            &mut session,
            Request::StartResourceJob {
                pilot: None,
                target_units: 5000,
            },
            1002,
        );
        let mut now = 1002;
        let mut live = None;
        for _ in 0..2400 {
            now += 250;
            server.tick_at(now).unwrap();
            let response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            let own = response
                .snapshot
                .as_ref()
                .unwrap()
                .pilots
                .iter()
                .find(|p| p.name == "Mechanism")
                .unwrap();
            if own.drones.len() == 5
                && captain_state(&response).fractions_at(now) == [1.; 7]
                && own.drones.iter().any(|d| {
                    matches!(d.mode, DroneMode::Mining { .. })
                        && d.mining_cycle
                            .as_ref()
                            .is_some_and(|c| c.started_at_ms < now && now < c.completes_at_ms)
                })
            {
                live = Some(response);
                break;
            }
        }
        let live = live.expect("actual mining cycle required");
        let own = live
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Mechanism")
            .unwrap();
        let id = own
            .drones
            .iter()
            .find(|d| {
                d.mining_cycle
                    .as_ref()
                    .is_some_and(|c| c.started_at_ms < now && now < c.completes_at_ms)
            })
            .unwrap()
            .id;
        capture("01-mining", &live);
        let held = captain_ok(
            &mut server,
            &mut session,
            Request::DroneHoldById { id },
            now,
        );
        capture("02-hold-command", &held);
        let hold_drone = held
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Mechanism")
            .unwrap()
            .drones
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .clone();
        assert!(matches!(hold_drone.mode, DroneMode::Hold));
        let hold_since = hold_drone.updated_at_ms;
        now += 3000;
        let fresh = captain_ok(&mut server, &mut session, Request::Snapshot, now);
        let stationary = fresh
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Mechanism")
            .unwrap()
            .drones
            .iter()
            .find(|d| d.id == id)
            .unwrap();
        assert_eq!(stationary.updated_at_ms, hold_since);
        assert!(now.saturating_sub(stationary.updated_at_ms) > 1500);
        assert_eq!(stationary.position, hold_drone.position);
        assert_eq!(fresh.snapshot.as_ref().unwrap().server_time_ms, now);
        capture("03-hold-old-pose-fresh-snapshot", &fresh);
        let returning = captain_ok(
            &mut server,
            &mut session,
            Request::RecallDroneById { id },
            now,
        );
        let drone = returning
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Mechanism")
            .unwrap()
            .drones
            .iter()
            .find(|d| d.id == id)
            .unwrap();
        assert!(matches!(drone.mode, DroneMode::Returning));
        assert!(drone.mining_cycle.is_none());
        capture("04-returning", &returning);
        std::fs::write(path.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "pilot":"Mechanism","drone_id":id,"registration":PIONEER_DOCKED_REGISTRATION_REVISION,
            "requests":["Connect","Undock","StartResourceJob5000","ticks until actual mining cycle and deployed screens","DroneHoldById","Snapshot3000ms later","RecallDroneById"],
            "hold_pose_updated_at_ms":hold_since,"fresh_snapshot_ms":now,
            "unmodified_authoritative_responses":true,
            "caveat":"DroneHold is an equipment command: existing manual takeover cancels resource job and returns other drones; captain display-only commands are a separate non-cancelling feature."
        })).unwrap()).unwrap();
    }

    #[test]
    fn external_auto_normal_space_manual_preferences_and_actual_warp_export() {
        let (mut server, mut session, _) = fixture();
        let proof = std::env::var_os("ARKLINE_SIDE_CAMERA_AUTHORITY_PROOF_DIR").map(|p| {
            let p = std::path::PathBuf::from(p);
            let root = std::env::var_os("ARKLINE_PROOF_ROOT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .canonicalize()
                .unwrap();
            assert!(p
                .parent()
                .unwrap()
                .canonicalize()
                .unwrap()
                .starts_with(root));
            std::fs::create_dir(&p).expect("fresh proof directory required");
            p
        });
        let capture = |label: &str, response: &Response| {
            if let Some(path) = &proof {
                std::fs::write(
                    path.join(format!("{label}-response.json")),
                    serde_json::to_vec_pretty(response).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    path.join(format!("{label}.json")),
                    serde_json::to_vec_pretty(response.snapshot.as_ref().unwrap()).unwrap(),
                )
                .unwrap();
            }
        };
        let docked = captain_ok(&mut server, &mut session, Request::Snapshot, 1000);
        assert_eq!(captain_state(&docked).all_fractions_at(1000), [0.; 9]);
        capture("01-docked-auto", &docked);
        let mut response = captain_ok(&mut server, &mut session, Request::Undock, 1001);
        assert!(captain_state(&response)
            .external_screens
            .iter()
            .all(|s| s.target_deployed));
        for step in 0..=12 {
            let now = 1001 + step * 100;
            response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            let state = captain_state(&response);
            assert!((state.external_screens[0].fraction_at(now) - step as f64 / 12.).abs() < 1e-9);
            capture(&format!("external-open-{step:02}"), &response);
        }
        let command = captain_request(captain_state(&response), Some(7), CockpitScreenMode::Stowed);
        response = captain_ok(&mut server, &mut session, command, 2201);
        assert_eq!(
            captain_state(&response).external_modes,
            [CockpitScreenMode::Stowed, CockpitScreenMode::Auto]
        );
        assert!(!captain_state(&response).external_screens[0].target_deployed);
        assert!(captain_state(&response).external_screens[1].target_deployed);
        capture("02-port-manual-stowed", &response);
        response = captain_ok(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            2201,
        );
        let Location::InTransit { arrives_at_ms, .. } =
            response.snapshot.as_ref().unwrap().pilots[0].location
        else {
            panic!()
        };
        assert!(captain_state(&response)
            .iter_screens()
            .all(|s| !s.target_deployed));
        capture("03-warp-override", &response);
        for step in 0..=12 {
            let now = 2201 + step * 100;
            response = captain_ok(&mut server, &mut session, Request::Snapshot, now);
            capture(&format!("external-warp-{step:02}"), &response);
        }
        response = captain_ok(&mut server, &mut session, Request::Snapshot, arrives_at_ms);
        assert!(!captain_state(&response).external_screens[0].target_deployed);
        assert!(captain_state(&response).external_screens[1].target_deployed);
        assert_eq!(
            captain_state(&response).external_modes,
            [CockpitScreenMode::Stowed, CockpitScreenMode::Auto]
        );
        capture("04-confirmed-arrival", &response);
        let command = captain_request(captain_state(&response), None, CockpitScreenMode::Open);
        response = captain_ok(&mut server, &mut session, command, arrives_at_ms);
        assert!(
            (0..9).all(|slot| captain_state(&response).mode(slot) == Some(CockpitScreenMode::Open))
        );
        response = captain_ok(
            &mut server,
            &mut session,
            Request::Snapshot,
            arrives_at_ms + 1200,
        );
        assert_eq!(
            captain_state(&response).all_fractions_at(arrives_at_ms + 1200),
            [1.; 9]
        );
        capture("05-nine-manual-open", &response);
        response = captain_ok(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Station,
            },
            arrives_at_ms + 1200,
        );
        assert!(captain_state(&response)
            .iter_screens()
            .all(|s| !s.target_deployed));
        assert!(
            (0..9).all(|slot| captain_state(&response).mode(slot) == Some(CockpitScreenMode::Open))
        );
        capture("06-open-preference-warp-stowed", &response);
        if let Some(path) = proof {
            std::fs::write(path.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"pilot":"Mechanism","registration":PIONEER_DOCKED_REGISTRATION_REVISION,"vessel":"v1:Mechanism:Pioneer","sources":["pioneer.camera.exterior.port","pioneer.camera.exterior.starboard"],"unmodified_authoritative_responses":true,"notes":"Two external cameras have no power or damage simulation; empty manual-open drone/turret screens do not invent feeds."})).unwrap()).unwrap();
        }
    }
    #[test]
    fn external_retraction_obstruction_keeps_pose_and_preference_until_clear() {
        let (_, _, mut pilot) = fixture();
        let mut state = state();
        pilot.location = Location::Station;
        reconcile(&mut state, &pilot, 1000, |_, _, _| false);
        reconcile(&mut state, &pilot, 2200, |_, _, _| false);
        assert_eq!(state.external_screens[0].fraction_at(2200), 1.);
        state.external_modes[0] = CockpitScreenMode::Stowed;
        reconcile(&mut state, &pilot, 2200, |slot, _, _| slot == 7);
        assert!(state.external_screens[0].blocked.is_some());
        assert_eq!(state.external_screens[0].fraction_at(9999), 1.);
        assert_eq!(state.external_screens[1].fraction_at(2200), 1.);
        reconcile(&mut state, &pilot, 2300, |_, _, _| false);
        assert_eq!(state.external_screens[0].fraction_at(2900), 0.5);
        assert_eq!(state.external_modes[0], CockpitScreenMode::Stowed);
    }
    #[test]
    fn external_pair_pauses_and_persists_midstroke_without_offline_motion() {
        let (mut editor, mut session, _) = editor_fixture();
        for _ in 0..3 {
            editor.step(200).unwrap();
        }
        let before = editor_screen_state(&mut editor, &mut session);
        let now = editor.time();
        assert_eq!(before.external_screens[0].fraction_at(now), 0.5);
        editor.pause();
        editor.advance(u64::MAX).unwrap();
        assert_eq!(editor_screen_state(&mut editor, &mut session), before);
        editor.play();
        editor.advance(200).unwrap();
        let after = editor_screen_state(&mut editor, &mut session);
        assert!((after.external_screens[0].fraction_at(editor.time()) - 2. / 3.).abs() < 1e-9);
        let (mut server, mut session, _) = fixture();
        captain_ok(&mut server, &mut session, Request::Undock, 1001);
        captain_ok(&mut server, &mut session, Request::Snapshot, 1601);
        let original = captain_world(&server);
        let dir = std::env::temp_dir().join(format!(
            "arkline-external-camera-persist-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("world.sqlite");
        let disk = Server::open(&path).unwrap();
        disk.db
            .execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&original).unwrap()],
            )
            .unwrap();
        drop(disk);
        let disk = Server::open(&path).unwrap();
        let loaded = captain_world(&disk);
        let pair = &loaded.accounts["Mechanism"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .external_screens;
        assert!(pair
            .iter()
            .all(|s| s.blocked.is_some() && s.fraction_at(u64::MAX) == 0.5));
    }
    #[test]
    fn external_slot_generations_are_not_drone_bindings_and_failed_warp_is_atomic() {
        let (mut server, mut session, _) = fixture();
        let response = captain_ok(&mut server, &mut session, Request::Snapshot, 1000);
        let state = captain_state(&response).clone();
        let wrong = Request::SetCockpitScreenMode {
            vessel_id: state.vessel_id.clone(),
            target: CockpitScreenTarget::Slot {
                slot: 7,
                expected_assignment_generation: Some(0),
            },
            mode: CockpitScreenMode::Open,
        };
        let before = serde_json::to_value(captain_world(&server)).unwrap();
        assert!(!server.handle_at(&mut session, wrong, 1000).ok);
        assert_eq!(
            serde_json::to_value(captain_world(&server)).unwrap(),
            before
        );
        captain_ok(
            &mut server,
            &mut session,
            captain_request(&state, Some(7), CockpitScreenMode::Open),
            1000,
        );
        let before = serde_json::to_value(captain_world(&server)).unwrap();
        assert!(
            !server
                .handle_at(
                    &mut session,
                    Request::Warp {
                        destination: Destination::Belt
                    },
                    1000
                )
                .ok
        );
        assert_eq!(
            serde_json::to_value(captain_world(&server)).unwrap(),
            before
        );
    }
}
