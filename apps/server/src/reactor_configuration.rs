//! Rigid collar configuration authority. Existing scalar energy is unchanged;
//! this is not motor, reactor-output, fuel, heat, mass, or warp-interlock simulation.
use super::*;
use arkline_protocol::pioneer_docked as geometry;

fn reconcile(
    state: &mut PioneerReactorConfiguration,
    compact: bool,
    now: u64,
    mut obstructed: impl FnMut(usize, f64, f64) -> bool,
) {
    if now < state.updated_at_ms {
        return;
    }
    let before = state.clone();
    let current = state.fraction_at(now);
    if state.target_compact != compact || state.blocked.is_some() {
        let target = f64::from(compact);
        state.from_fraction = current;
        state.target_compact = compact;
        state.started_at_ms = now;
        state.duration_ms =
            ((target - current).abs() * REACTOR_CONFIGURATION_TRAVEL_MS as f64).ceil() as u64;
        state.blocked = if (target - current).abs() > 1e-9 {
            (0..4)
                .find(|&collar| obstructed(collar, current, target))
                .map(|collar| {
                    format!(
                        "Occupied travel envelope: {}",
                        REACTOR_COLLAR_COMPONENT_IDS[collar]
                    )
                })
        } else {
            None
        };
    }
    if *state != before || before.moving_at(before.updated_at_ms) {
        state.updated_at_ms = now;
    }
}
fn hold(state: &mut PioneerReactorConfiguration, now: u64, reason: &str) {
    if !state.moving_at(state.updated_at_ms) {
        return;
    }
    state.from_fraction = state.fraction_at(state.updated_at_ms);
    state.started_at_ms = now;
    state.duration_ms = 0;
    state.blocked = Some(reason.into());
    state.updated_at_ms = now;
}
pub(super) fn stop_on_restart(w: &mut World, now: u64, document: Option<&RoomLayoutDocument>) {
    for a in w.accounts.values_mut() {
        if let Some(state) = &mut a.reactor_configuration {
            hold(
                state,
                now,
                "Server restarted; awaiting configuration reconciliation",
            );
        }
    }
    refresh_views(w);
    let default = default_pioneer_room_layout();
    if !geometry::reactor_configuration_supported(document.unwrap_or(&default)) {
        for p in w.pilots.values_mut() {
            p.view.reactor_configuration = None;
        }
    }
}
fn refresh_views(w: &mut World) {
    for (name, p) in &mut w.pilots {
        p.view.reactor_configuration = if p.view.ship_class == ShipClass::Pioneer {
            w.accounts
                .get(name)
                .and_then(|a| a.reactor_configuration.clone())
        } else {
            None
        };
    }
}
pub(super) fn advance(
    w: &mut World,
    now: u64,
    document: Option<&RoomLayoutDocument>,
) -> Result<()> {
    let default = default_pioneer_room_layout();
    if !geometry::reactor_configuration_supported(document.unwrap_or(&default)) {
        for a in w.accounts.values_mut() {
            if let Some(state) = &mut a.reactor_configuration {
                hold(
                    state,
                    now,
                    "Reactor module not installed in this room layout",
                );
            }
        }
        for p in w.pilots.values_mut() {
            p.view.reactor_configuration = None;
        }
        return Ok(());
    }
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    for name in names {
        let p = &w.pilots[&name].view;
        if p.ship_class != ShipClass::Pioneer {
            if let Some(state) = w
                .accounts
                .get_mut(&name)
                .and_then(|a| a.reactor_configuration.as_mut())
            {
                hold(
                    state,
                    now,
                    "Inactive vessel; awaiting configuration reconciliation",
                );
            }
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
        let compact = matches!(
            p.location,
            Location::InTransit { .. } | Location::Jumping { .. }
        );
        let mut state = w.accounts[&name]
            .reactor_configuration
            .clone()
            .filter(|s| s.vessel_id == vessel)
            .unwrap_or_else(|| PioneerReactorConfiguration::open(vessel.clone(), now));
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
        reconcile(&mut state, compact, now, |collar, from, to| {
            geometry::reactor_configuration_sweep(collar, from, to).is_none_or(|bounds| {
                occupants
                    .iter()
                    .any(|feet| geometry::box_intersects_capsule(bounds, *feet, 1.8, 0.3))
            })
        });
        w.accounts.get_mut(&name).unwrap().reactor_configuration = Some(state);
    }
    refresh_views(w);
    Ok(())
}
/// Actual segmented parts collide at rest; moving collars additionally reserve
/// their full remaining swept envelope so later walking cannot enter the path.
pub(super) fn with_collision(
    w: &World,
    name: &str,
    base: &PioneerDockedGeometry,
    now: u64,
    document: Option<&RoomLayoutDocument>,
) -> PioneerDockedGeometry {
    let mut out = base.clone();
    let default = default_pioneer_room_layout();
    if !geometry::reactor_configuration_supported(document.unwrap_or(&default)) {
        return out;
    }
    if let Some(state) = w
        .accounts
        .get(name)
        .and_then(|a| a.reactor_configuration.as_ref())
    {
        let fraction = state.fraction_at(now);
        if let Ok(parts) =
            geometry::reactor_configuration_geometry(PioneerDockedPose::private(), fraction)
        {
            let cell = parts
                .panels
                .first()
                .map(|p| p.cell)
                .unwrap_or(PioneerDockedCell::Cockpit);
            out.panels.extend(parts.panels);
            if state.moving_at(now) {
                for (collar, component_id) in REACTOR_COLLAR_COMPONENT_IDS.iter().enumerate() {
                    if let Some(bounds) = geometry::reactor_configuration_sweep(
                        collar,
                        fraction,
                        f64::from(state.target_compact),
                    ) {
                        out.panels.push(PioneerDockedPanel {
                            id: format!("{component_id}/reserved"),
                            bounds,
                            material: PioneerDockedMaterial::Graphite,
                            collision: PioneerDockedCollision::Solid,
                            cell,
                            kind: PioneerDockedPanelKind::Furniture,
                        });
                    }
                }
            }
        }
    }
    out
}
pub(super) fn validate(w: &World) -> Result<()> {
    for a in w.accounts.values() {
        if let Some(s) = &a.reactor_configuration {
            ensure!(
                a.vessel_id(ShipClass::Pioneer) == Some(s.vessel_id.as_str())
                    && s.component_id == REACTOR_ASSEMBLY_COMPONENT_ID,
                "reactor configuration identity mismatch"
            );
            ensure!(
                s.from_fraction.is_finite()
                    && (0.0..=1.0).contains(&s.from_fraction)
                    && s.duration_ms <= REACTOR_CONFIGURATION_TRAVEL_MS,
                "invalid reactor configuration actuator"
            );
        }
    }
    Ok(())
}

// Recovery replacement: production logic transcribed; missing regression tests rebuilt.
#[cfg(test)]
mod tests {
    use super::*;
    fn connect(token: Option<String>) -> Request {
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            pilot: "Reactor".into(),
            resume_token: token,
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: true,
        }
    }
    fn ok(
        server: &mut Server,
        session: &mut Option<String>,
        request: Request,
        now: u64,
    ) -> Response {
        let response = server.handle_at(session, request, now);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn fixture() -> (Server, Option<String>, String) {
        let mut server = Server::open(":memory:").unwrap();
        let mut session = None;
        let token = ok(&mut server, &mut session, connect(None), 1000)
            .resume_token
            .unwrap();
        (server, session, token)
    }
    fn saved(server: &Server) -> World {
        let json: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        serde_json::from_str(&json).unwrap()
    }
    #[test]
    fn stationary_restart_is_exact_and_midstroke_holds_last_persisted_pose() {
        let (server, _, _) = fixture();
        let original = saved(&server);
        for fraction in [0., 1.] {
            let mut w = original.clone();
            let s = w
                .accounts
                .get_mut("Reactor")
                .unwrap()
                .reactor_configuration
                .as_mut()
                .unwrap();
            s.from_fraction = fraction;
            s.target_compact = fraction == 1.;
            let before = serde_json::to_vec(s).unwrap();
            stop_on_restart(&mut w, 9000, None);
            assert_eq!(
                serde_json::to_vec(
                    w.accounts["Reactor"]
                        .reactor_configuration
                        .as_ref()
                        .unwrap()
                )
                .unwrap(),
                before
            );
        }
        let mut w = original;
        let s = w
            .accounts
            .get_mut("Reactor")
            .unwrap()
            .reactor_configuration
            .as_mut()
            .unwrap();
        reconcile(s, true, 1000, |_, _, _| false);
        reconcile(s, true, 1600, |_, _, _| false);
        stop_on_restart(&mut w, 9000, None);
        let s = w.accounts["Reactor"]
            .reactor_configuration
            .as_ref()
            .unwrap();
        assert_eq!(s.fraction_at(u64::MAX), 0.5);
        assert!(s.blocked.is_some());
    }
    #[test]
    fn inactive_vessel_holds_midstroke_until_active_reconciliation() {
        let (server, _, _) = fixture();
        let mut w = saved(&server);
        let state = w
            .accounts
            .get_mut("Reactor")
            .unwrap()
            .reactor_configuration
            .as_mut()
            .unwrap();
        reconcile(state, true, 1000, |_, _, _| false);
        reconcile(state, true, 1600, |_, _, _| false);
        w.pilots.get_mut("Reactor").unwrap().view.ship_class = ShipClass::Prospector;
        advance(&mut w, 1600, None).unwrap();
        advance(&mut w, 10000, None).unwrap();
        let held = w.accounts["Reactor"]
            .reactor_configuration
            .as_ref()
            .unwrap();
        assert_eq!(held.fraction_at(10000), 0.5);
        assert!(w.pilots["Reactor"].view.reactor_configuration.is_none());
        w.pilots.get_mut("Reactor").unwrap().view.ship_class = ShipClass::Pioneer;
        advance(&mut w, 10000, None).unwrap();
        let restored = w.pilots["Reactor"]
            .view
            .reactor_configuration
            .as_ref()
            .unwrap();
        assert_eq!(restored.fraction_at(10000), 0.5);
        // This fixture is in normal space: restoring it reconciles toward open.
        assert_eq!(restored.fraction_at(10300), 0.25);
        assert_eq!(restored.fraction_at(10600), 0.0);
    }
    #[test]
    fn changed_room_layout_has_no_installed_configuration_or_dynamic_colliders() {
        let (server, _, _) = fixture();
        let mut w = saved(&server);
        let mut document = default_pioneer_room_layout();
        let room = document
            .rooms
            .iter_mut()
            .find(|r| r.source_room_id.as_deref() == Some("D1-S1"))
            .unwrap();
        room.name.push_str(" naming alone should not move geometry");
        // Removing the source binding explicitly makes this custom room unsupported.
        room.source_room_id = None;
        advance(&mut w, 1100, Some(&document)).unwrap();
        assert!(w.pilots["Reactor"].view.reactor_configuration.is_none());
        let held = serde_json::to_vec(&w.accounts["Reactor"].reactor_configuration).unwrap();
        stop_on_restart(&mut w, 9000, Some(&document));
        assert!(
            w.pilots["Reactor"].view.reactor_configuration.is_none(),
            "Reopen must not republish an uninstalled module"
        );
        assert_eq!(
            serde_json::to_vec(&w.accounts["Reactor"].reactor_configuration).unwrap(),
            held
        );
        let g = with_collision(
            &w,
            "Reactor",
            &PioneerDockedGeometry::default(),
            1100,
            Some(&document),
        );
        assert!(g.panels.is_empty());
    }

    fn config(response: &Response) -> &PioneerReactorConfiguration {
        response
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Reactor")
            .unwrap()
            .reactor_configuration
            .as_ref()
            .unwrap()
    }
    #[test]
    fn actual_warp_compresses_and_normal_arrival_reopens_without_extra_energy_debit() {
        let (mut server, mut session, _) = fixture();
        let proof = std::env::var_os("ARKLINE_REACTOR_AUTHORITY_PROOF_DIR").map(|p| {
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
            std::fs::create_dir(&p).expect("proof directory must be new");
            p
        });
        let capture = |name: &str, r: &Response| {
            if let Some(path) = &proof {
                std::fs::write(
                    path.join(format!("{name}.json")),
                    serde_json::to_vec_pretty(r.snapshot.as_ref().unwrap()).unwrap(),
                )
                .unwrap();
                std::fs::write(
                    path.join(format!("{name}-response.json")),
                    serde_json::to_vec_pretty(r).unwrap(),
                )
                .unwrap();
            }
        };
        let open = ok(&mut server, &mut session, Request::Undock, 1001);
        capture("01-open", &open);
        let before = open.snapshot.as_ref().unwrap().pilots[0]
            .power
            .as_ref()
            .unwrap()
            .charge_j;
        let warp = ok(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            1001,
        );
        capture("02-accepted-warp", &warp);
        assert!(config(&warp).target_compact);
        assert_eq!(config(&warp).fraction_at(1001), 0.);
        assert_eq!(
            before
                - warp.snapshot.as_ref().unwrap().pilots[0]
                    .power
                    .as_ref()
                    .unwrap()
                    .charge_j,
            power_spec(ShipClass::Pioneer).warp_cost_j
        );
        let Location::InTransit { arrives_at_ms, .. } =
            warp.snapshot.as_ref().unwrap().pilots[0].location
        else {
            panic!("accepted warp required")
        };
        for step in 0..=12 {
            let now = 1001 + step * 100;
            let r = ok(&mut server, &mut session, Request::Snapshot, now);
            assert!((config(&r).fraction_at(now) - step as f64 / 12.).abs() < 1e-9);
            capture(&format!("compress-{step:02}"), &r);
        }
        let arrived = ok(&mut server, &mut session, Request::Snapshot, arrives_at_ms);
        assert!(!config(&arrived).target_compact);
        capture("03-normal-arrival", &arrived);
        for step in 0..=12 {
            let now = arrives_at_ms + step * 100;
            let r = ok(&mut server, &mut session, Request::Snapshot, now);
            assert!((config(&r).fraction_at(now) - (1. - step as f64 / 12.)).abs() < 1e-9);
            capture(&format!("open-{step:02}"), &r);
        }
        if let Some(path) = &proof {
            std::fs::write(path.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
                "provenance":"newly rebuilt authority test; not original lost checkpoint",
                "pilot":"Reactor", "vessel_id":"v1:Reactor:Pioneer", "warp_start_ms":1001,
                "sample_interval_ms":100,"events":["Connect","Undock","Warp(Belt)","Snapshot through compression","Snapshot at normal arrival","Snapshot through expansion"],
                "snapshots_are_unmodified_authoritative_responses":true,
                "energy_claim":"existing scalar warp debit only; motor accounting unmodeled"
            })).unwrap()).unwrap();
        }
    }
    #[test]
    fn rejected_warp_does_not_change_configuration_or_energy() {
        let (mut server, mut session, _) = fixture();
        let before = serde_json::to_value(saved(&server)).unwrap();
        let r = server.handle_at(
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            1000,
        );
        assert!(!r.ok);
        assert_eq!(serde_json::to_value(saved(&server)).unwrap(), before);
    }
    #[test]
    fn obstruction_freezes_entire_synchronized_assembly_then_retries_and_reverses() {
        let mut s = PioneerReactorConfiguration::open("vessel".into(), 1000);
        reconcile(&mut s, true, 1000, |collar, _, _| collar == 2);
        assert!(s
            .blocked
            .as_ref()
            .unwrap()
            .contains(REACTOR_COLLAR_COMPONENT_IDS[2]));
        assert_eq!(s.fraction_at(9999), 0.);
        reconcile(&mut s, true, 2000, |_, _, _| false);
        assert_eq!(s.fraction_at(2600), 0.5);
        reconcile(&mut s, false, 2600, |_, _, _| false);
        assert_eq!(s.fraction_at(2900), 0.25);
        assert_eq!(s.fraction_at(3200), 0.);
    }
    #[test]
    fn editor_pause_and_reconnect_preserve_actual_midstroke() {
        let path = std::env::temp_dir().join(format!(
            "arkline-rebuilt-reactor-clock-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let mut world = crate::editor::EditorWorld::open_with_creation_scene(
            &path,
            SceneLayoutRevision::LegacyV1,
        )
        .unwrap();
        let mut session = None;
        let r = world.handle(&mut session, connect(None));
        assert!(r.ok, "{}", r.message);
        let token = r.resume_token.unwrap();
        assert!(world.handle(&mut session, Request::Undock).ok);
        assert!(
            world
                .handle(
                    &mut session,
                    Request::Warp {
                        destination: Destination::Belt
                    }
                )
                .ok
        );
        for _ in 0..3 {
            world.step(200).unwrap();
        }
        let now = world.time();
        let before = world.handle(&mut session, Request::Snapshot);
        assert_eq!(config(&before).fraction_at(now), 0.5);
        world.pause();
        std::thread::sleep(std::time::Duration::from_millis(30));
        world.advance(u64::MAX).unwrap();
        let paused = world.handle(&mut session, Request::Snapshot);
        assert_eq!(now, world.time());
        assert_eq!(config(&paused), config(&before));
        world.disconnect(&mut session);
        let reconnect = world.handle(&mut session, connect(Some(token)));
        assert!(reconnect.ok, "{}", reconnect.message);
        assert_eq!(config(&reconnect), config(&paused));
        world.play();
        world.advance(200).unwrap();
        let resumed = world.handle(&mut session, Request::Snapshot);
        assert!((config(&resumed).fraction_at(world.time()) - 2. / 3.).abs() < 1e-9);
    }
    #[test]
    fn real_collar_sweep_reserves_previously_clear_capsule_space() {
        let (mut server, mut session, _) = fixture();
        let binding = docked_traversal_binding(
            "v1:Reactor:Pioneer".into(),
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
            1001,
        );
        let mut w = saved(&server);
        let feet = [-5.55, -3.70, 5.10];
        let base = geometry::registered_geometry(&default_pioneer_room_layout()).unwrap();
        let open_geometry = with_collision(&w, "Reactor", &base, 1001, None);
        let state = w
            .pilots
            .get_mut("Reactor")
            .unwrap()
            .view
            .station_avatar
            .as_mut()
            .unwrap()
            .docked_traversal
            .as_mut()
            .unwrap();
        let pose = docked_binding_pose(&state.binding).unwrap();
        state.feet_m = pose.body_to_station(feet);
        assert!(
            geometry::capsule_clear_station(
                &open_geometry,
                &PioneerDockedGeometry::default(),
                pose,
                state.feet_m
            ),
            "candidate must clear the entire registered room and open collars"
        );
        // Real registered avatar fixture; use real occupant collection through advance.
        // Only authority state setup is synthetic for this collision unit test.
        let mut fresh = Server::open(":memory:").unwrap();
        let mut fresh_session = None;
        ok(&mut fresh, &mut fresh_session, connect(None), 1000);
        ok(&mut fresh, &mut fresh_session, Request::Undock, 1001);
        let warp = ok(
            &mut fresh,
            &mut fresh_session,
            Request::Warp {
                destination: Destination::Belt,
            },
            1001,
        );
        w.pilots.get_mut("Reactor").unwrap().view.location =
            warp.snapshot.unwrap().pilots[0].location.clone();
        advance(&mut w, 1001, None).unwrap();
        let held = w.accounts["Reactor"]
            .reactor_configuration
            .as_ref()
            .unwrap();
        assert!(held.blocked.is_some());
        assert_eq!(held.fraction_at(9999), 0.);
        w.pilots.get_mut("Reactor").unwrap().view.station_avatar = None;
        advance(&mut w, 1200, None).unwrap();
        assert!(w.accounts["Reactor"]
            .reactor_configuration
            .as_ref()
            .unwrap()
            .blocked
            .is_none());
        let reserved = with_collision(&w, "Reactor", &base, 1200, None);
        assert!(!geometry::capsule_clear_station(
            &reserved,
            &PioneerDockedGeometry::default(),
            pose,
            pose.body_to_station(feet)
        ));
    }
    #[test]
    fn sqlite_reopen_preserves_midstroke_without_offline_travel() {
        let (server, _, _) = fixture();
        let mut w = saved(&server);
        let s = w
            .accounts
            .get_mut("Reactor")
            .unwrap()
            .reactor_configuration
            .as_mut()
            .unwrap();
        reconcile(s, true, 1000, |_, _, _| false);
        reconcile(s, true, 1600, |_, _, _| false);
        let directory = std::env::temp_dir().join(format!(
            "arkline-rebuilt-reactor-restart-{}-{}",
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
        let restored = saved(&disk);
        let s = restored.accounts["Reactor"]
            .reactor_configuration
            .as_ref()
            .unwrap();
        assert_eq!(s.fraction_at(u64::MAX), 0.5);
        assert!(s.blocked.is_some());
        assert_eq!(
            restored.pilots["Reactor"].view.reactor_configuration,
            restored.accounts["Reactor"].reactor_configuration
        );
    }
    #[test]
    fn absent_optional_reactor_field_preserves_older_pilot_payloads() {
        let (server, _, _) = fixture();
        let w = saved(&server);
        let original = &w.pilots["Reactor"].view;
        let mut value = serde_json::to_value(original).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("reactor_configuration");
        let decoded: PilotView = serde_json::from_value(value).unwrap();
        assert!(decoded.reactor_configuration.is_none());
        assert_eq!(decoded.cockpit_mechanisms, original.cockpit_mechanisms);
        assert_eq!(decoded.power, original.power);
    }
}
