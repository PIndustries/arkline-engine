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
    let turrets = turret_active(p, now);
    for slot in 0..7 {
        let desired = !state.warp_stowed
            && if slot < 5 {
                state.drone_ids[slot].is_some_and(|id| ids.contains(&id))
            } else {
                turrets
            };
        let screen = &mut state.screens[slot];
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
            .screens
            .iter()
            .any(|s| s.moving_at(before.updated_at_ms))
    {
        state.updated_at_ms = now;
    }
}

/// A restart holds the last persisted pose; elapsed offline wall time is not motor travel.
pub(super) fn stop_on_restart(w: &mut World, now: u64) {
    for a in w.accounts.values_mut() {
        if let Some(state) = &mut a.cockpit_mechanisms {
            let mut changed = false;
            for screen in &mut state.screens {
                if !screen.moving_at(state.updated_at_ms) {
                    continue;
                }
                let fraction = screen.fraction_at(state.updated_at_ms);
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
                for screen in &mut state.screens {
                    if screen.moving_at(state.updated_at_ms) {
                        screen.from_fraction = screen.fraction_at(state.updated_at_ms);
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
) -> PioneerDockedGeometry {
    let mut out = base.clone();
    if let Some(state) = w
        .accounts
        .get(name)
        .and_then(|a| a.cockpit_mechanisms.as_ref())
    {
        for (slot, screen) in state.screens.iter().enumerate() {
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
    out
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
            for screen in &s.screens {
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
        let g = with_collision(&w, "Mechanism", &PioneerDockedGeometry::default(), 1100);
        let bounds = g.panels[0].bounds;
        let deployed = geometry::cockpit_mechanism_box(0, 1.).unwrap();
        assert!(
            bounds.center_m[1] - bounds.half_m[1]
                <= deployed.center_m[1] - deployed.half_m[1] + 1e-9
        );
        p.drones.clear();
        reconcile(&mut s, &p, 2200, |_, _, _| false);
        w.accounts.get_mut("Mechanism").unwrap().cockpit_mechanisms = Some(s);
        let g = with_collision(&w, "Mechanism", &PioneerDockedGeometry::default(), 3400);
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
    fn real_registered_avatar_blocks_authoritative_screen_sweep() {
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
        state.feet_m = pose.body_to_station([center[0], 0.5, center[2]]);
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
            let root = std::path::Path::new("/workspace/scratch/0af25020075d")
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
        for sample in 1..=24 {
            let steady = ok(
                &mut server,
                &mut session,
                Request::Snapshot,
                ready + 1200 + sample * 40,
            );
            capture(&format!("steady-{sample:02}"), &steady);
        }
        let warp_started = ready + 2400;
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
            let root = std::path::Path::new("/workspace/scratch/0af25020075d")
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
}
