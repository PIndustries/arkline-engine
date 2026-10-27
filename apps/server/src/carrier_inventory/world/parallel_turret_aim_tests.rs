//! Exact actual-source world, direct authority helper tests; does not bypass the
//! independent normal-world dispatch admission or claim captain mining enabled.
use super::*;
fn world_target(yaw: f64) -> (World, u64, u64) {
    let f = parallel_tests::fixture();
    place_target(f.world(), f.now, yaw)
}
fn place_target(mut w: World, now: u64, yaw: f64) -> (World, u64, u64) {
    let p = &mut w.pilots.get_mut("Pilot").unwrap().view;
    let flight = p.flight.as_ref().unwrap();
    let parent = BayPose {
        position_m: flight.position,
        orientation_xyzw: flight.orientation,
    };
    let dir = arkline_protocol::parallel_turret_aim::direction(yaw.to_radians(), 0.);
    let local = std::array::from_fn(|i| PIONEER_MINING_TURRET_MOUNTS_M[0][i] + 1000. * dir[i]);
    let offset = parent.transform_vector(local).unwrap();
    let position = std::array::from_fn(|i| parent.position_m[i] + offset[i]);
    let id = 900_001;
    p.locked_asteroids = vec![id];
    w.asteroids = vec![Asteroid {
        id,
        system_id: p.system_id,
        name: "Aim regression target".into(),
        position,
        radius: 5.,
        ore: 100_000,
    }];
    w.belt_ore = 100_000;
    (w, now, id)
}
fn sample(w: &mut World, now: u64) {
    crate::turret_stow::advance(w, now);
    crate::turret_aim::advance(w, now);
}
#[test]
fn forward_source_target_slews_settles_then_extracts_once() {
    let (mut w, start, id) = world_target(0.);
    sample(&mut w, start);
    assert!(crate::turret_aim::request(&mut w, "Pilot", 0, id, start).unwrap());
    let ore = w.pilots["Pilot"].view.ore;
    let power = w.pilots["Pilot"].view.power.as_ref().unwrap().charge_j;
    sample(&mut w, start + 100);
    assert!(crate::mining::advance_source(&mut w, start + 100).is_empty());
    assert_eq!(w.pilots["Pilot"].view.ore, ore);
    for delta in (200..=400).step_by(100) {
        sample(&mut w, start + delta);
        crate::mining::advance_source(&mut w, start + delta);
    }
    eprintln!(
        "forward aim={:?} delta={:?} obstruction={} ray={:?}",
        w.accounts["Pilot"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .turrets
            .aims,
        crate::turret_aim_collision::target_delta(&w, "Pilot", 0, id),
        crate::turret_stow::obstructed(&w, "Pilot", 0, 1., 1., start + 400),
        crate::turret_aim_collision::clear_target(
            &w,
            "Pilot",
            0,
            &w.accounts["Pilot"]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .turrets
                .aims
                .as_ref()
                .unwrap()[0],
            start + 400
        )
    );
    let due = w.pilots["Pilot"].view.mining_modules[0]
        .cycle_completes_at_ms
        .expect("forward source ray must admit a cycle after settling");
    sample(&mut w, due);
    let events = crate::mining::advance_source(&mut w, due);
    assert_eq!(events.len(), 1);
    assert!(w.pilots["Pilot"].view.ore > ore);
    assert_eq!(
        w.pilots["Pilot"].view.power.as_ref().unwrap().charge_j,
        power - power_spec(ShipClass::Pioneer).mining_cycle_cost_j
    );
    assert!(crate::mining::advance_source(&mut w, due).is_empty());
}
#[test]
fn inward_bridge_blocks_ore_and_debit_and_stop_cancels_aim() {
    let (mut w, start, id) = world_target(25.);
    sample(&mut w, start);
    assert!(crate::turret_aim::request(&mut w, "Pilot", 0, id, start).unwrap());
    let ore = w.pilots["Pilot"].view.ore;
    let power = w.pilots["Pilot"].view.power.clone();
    for delta in (100..=2000).step_by(100) {
        sample(&mut w, start + delta);
        assert!(crate::mining::advance_source(&mut w, start + delta).is_empty());
    }
    let a = &w.accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .aims
        .as_ref()
        .unwrap()[0];
    assert_eq!(
        a.status,
        arkline_protocol::parallel_turret_aim::TurretAimStatus::Obstructed
    );
    assert_eq!(w.pilots["Pilot"].view.ore, ore);
    assert_eq!(w.pilots["Pilot"].view.power, power);
    crate::mining::stop(&mut w, "Pilot", 0).unwrap();
    assert_eq!(
        w.accounts["Pilot"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .turrets
            .aims
            .as_ref()
            .unwrap()[0]
            .target_id,
        None
    );
}
fn ready_world() -> (World, u64, u64) {
    let (mut w, start, id) = world_target(0.);
    sample(&mut w, start);
    crate::turret_aim::request(&mut w, "Pilot", 0, id, start).unwrap();
    for delta in (100..=400).step_by(100) {
        sample(&mut w, start + delta);
    }
    let due = w.pilots["Pilot"].view.mining_modules[0]
        .cycle_completes_at_ms
        .unwrap();
    sample(&mut w, due);
    (w, due, id)
}
#[test]
fn actual_current_ray_and_power_rechecked_before_due_extraction() {
    for cause in [
        "nearer asteroid",
        "power lost",
        "missing root",
        "wrong target",
        "stale clock",
    ] {
        let (mut w, due, id) = ready_world();
        let ore = w.pilots["Pilot"].view.ore;
        match cause {
            "nearer asteroid" => {
                let target = w.asteroids[0].clone();
                w.asteroids.push(Asteroid {
                    id: id + 1,
                    position: [500., target.position[1], target.position[2]],
                    ..target
                });
            }
            "power lost" => {
                w.pilots
                    .get_mut("Pilot")
                    .unwrap()
                    .view
                    .power
                    .as_mut()
                    .unwrap()
                    .charge_j = 0
            }
            "missing root" => {
                let s = w
                    .accounts
                    .get_mut("Pilot")
                    .unwrap()
                    .carrier_inventory
                    .as_mut()
                    .unwrap();
                let key = s.machinery.keys().next().unwrap().clone();
                s.machinery.remove(&key);
            }
            "wrong target" => {
                w.pilots.get_mut("Pilot").unwrap().view.mining_modules[0].target_id = Some(id + 1)
            }
            _ => {}
        }
        let power = w.pilots["Pilot"].view.power.clone();
        assert!(
            crate::mining::advance_source(&mut w, due + u64::from(cause == "stale clock"))
                .is_empty(),
            "{cause}"
        );
        assert_eq!(w.pilots["Pilot"].view.ore, ore, "{cause}");
        assert_eq!(w.pilots["Pilot"].view.power, power, "{cause}");
    }
}
#[test]
fn source_aim_stow_restart_and_same_clock_request_are_cancellable() {
    let (mut w, start, id) = world_target(-20.);
    sample(&mut w, start);
    crate::turret_aim::request(&mut w, "Pilot", 0, id, start).unwrap();
    sample(&mut w, start);
    let aim = &w.accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .aims
        .as_ref()
        .unwrap()[0];
    assert_eq!((aim.yaw_rad, aim.pitch_rad), (0., 0.));
    sample(&mut w, start + 100);
    assert!(
        w.accounts["Pilot"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .turrets
            .aims
            .as_ref()
            .unwrap()[0]
            .yaw_rad
            < 0.
    );
    let vessel = w.accounts["Pilot"]
        .vessel_id(ShipClass::Pioneer)
        .unwrap()
        .to_owned();
    crate::turret_stow::set_deployment(&mut w, "Pilot", &vessel, false, start + 100).unwrap();
    sample(&mut w, start + 100);
    crate::turret_stow::stop_on_restart(&mut w, start + 100);
    let aim = &w.accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .aims
        .as_ref()
        .unwrap()[0];
    assert_eq!(aim.target_id, None);
    let pose = (aim.yaw_rad, aim.pitch_rad);
    sample(&mut w, start + 1000);
    let aim = &w.accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .aims
        .as_ref()
        .unwrap()[0];
    assert_eq!((aim.yaw_rad, aim.pitch_rad), pose);
}

#[test]
fn actual_second_carrier_peer_blocks_only_its_current_conservative_envelope() {
    let mut f = parallel_tests::fixture();
    let mut request = connect();
    if let Request::Connect { pilot, .. } = &mut request {
        *pilot = "Peer".into();
    }
    let mut peer_session = None;
    let response = f.server.handle_at(&mut peer_session, request, f.now);
    assert!(response.ok, "{}", response.message);
    let (base, start, id) = place_target(f.world(), f.now, 0.);
    for (position, blocked) in [([500., 9.864, -5.2], true), ([500., 2000., -5.2], false)] {
        let mut w = base.clone();
        let peer = w.pilots.get_mut("Peer").unwrap();
        peer.view.station_avatar = None;
        let flight = peer.view.flight.as_mut().unwrap();
        flight.position = position;
        flight.orientation = [0., 0., 0., 1.];
        flight.heading = [1., 0., 0.];
        flight.velocity = [0.; 3];
        flight.acceleration = [0.; 3];
        flight.angular_velocity = [0.; 3];
        flight.mode = NavigationMode::Stop;
        let dock = flight.docking.as_mut().unwrap();
        dock.phase = DockingPhase::Free;
        dock.clamps = 0.;
        dock.segment = None;
        validate(&w).unwrap();
        sample(&mut w, start);
        crate::turret_aim::request(&mut w, "Pilot", 0, id, start).unwrap();
        for delta in (100..=500).step_by(100) {
            sample(&mut w, start + delta);
        }
        let aim = &w.accounts["Pilot"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .turrets
            .aims
            .as_ref()
            .unwrap()[0];
        assert_eq!(
            aim.status == arkline_protocol::parallel_turret_aim::TurretAimStatus::Ready,
            !blocked,
            "{position:?} {aim:?}"
        );
        assert_eq!(
            crate::turret_aim_collision::clear_target(&w, "Pilot", 0, aim, start + 500).is_none(),
            blocked
        );
    }
}

#[test]
fn real_start_tick_export_and_reopen_preserve_atomic_turret_extraction() {
    let mut f = parallel_tests::fixture();
    let mut w = f.world();
    let asteroid = w
        .asteroids
        .iter_mut()
        .find(|a| a.system_id == HOME_SYSTEM_ID)
        .unwrap();
    asteroid.position = [1010.8, 9.864, -5.2];
    let target = asteroid.id;
    // Fixture-only physical placement, no ore/energy/ledger quantity injection.
    f.write(&w);
    f.ok(Request::LockAsteroid { id: target });
    for _ in 0..200 {
        if f.world().pilots["Pilot"]
            .view
            .locked_asteroids
            .contains(&target)
        {
            break;
        }
        f.step();
    }
    let ore = f.world().pilots["Pilot"].view.ore;
    let accepted = f.ok(Request::StartMining {
        module_id: 0,
        target_id: target,
    });
    assert!(accepted.message.contains("aim accepted"));
    let first = f.world().accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .aims
        .clone();
    f.ok(Request::Snapshot);
    assert_eq!(
        f.world().accounts["Pilot"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .turrets
            .aims,
        first
    );
    assert_eq!(f.world().pilots["Pilot"].view.ore, ore);
    for _ in 0..100 {
        f.step();
        if f.world().pilots["Pilot"].view.ore > ore {
            break;
        }
    }
    assert_eq!(
        f.world().pilots["Pilot"].view.ore,
        ore + mining_spec(ShipClass::Pioneer).yield_units
    );
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    let entries = export.entries.len();
    let after = f.world().pilots["Pilot"].view.ore;
    f.reopen();
    assert_eq!(f.world().pilots["Pilot"].view.ore, after);
    assert!(f.world().accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .aims
        .as_ref()
        .unwrap()
        .iter()
        .all(|a| a.target_id.is_none()));
    for _ in 0..80 {
        f.step();
    }
    assert_eq!(f.world().pilots["Pilot"].view.ore, after);
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    assert_eq!(export.entries.len(), entries);
}

#[test]
fn real_due_tick_moves_free_body_into_ray_before_any_turret_credit() {
    let mut f = parallel_tests::fixture();
    super::service_tests::launch(&mut f, 1);
    let mut w = f.world();
    let ids: Vec<_> = w
        .asteroids
        .iter()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .take(2)
        .map(|a| a.id)
        .collect();
    assert_eq!(ids.len(), 2);
    w.asteroids
        .iter_mut()
        .find(|a| a.id == ids[0])
        .unwrap()
        .position = [3010.8, 9.864, -5.2];
    w.asteroids
        .iter_mut()
        .find(|a| a.id == ids[1])
        .unwrap()
        .position = [2000., -1000., -5.2];
    let d = w
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == 1)
        .unwrap();
    d.position = [2000., 16.864, -5.2];
    d.velocity = [0.; 3];
    d.mode = DroneMode::Hold;
    d.updated_at_ms = f.now;
    let body = BayPose {
        position_m: d.position,
        orientation_xyzw: d
            .orientation_xyzw
            .expect("source launched drone retains attitude"),
    };
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .drone_poses
        .insert(1, body);
    validate(&w).unwrap();
    f.write(&w);
    for id in &ids {
        f.ok(Request::LockAsteroid { id: *id });
    }
    for _ in 0..200 {
        if ids
            .iter()
            .all(|id| f.world().pilots["Pilot"].view.locked_asteroids.contains(id))
        {
            break;
        }
        f.step();
    }
    f.ok(Request::StartMining {
        module_id: 0,
        target_id: ids[0],
    });
    for _ in 0..10 {
        f.step();
        if f.world().pilots["Pilot"].view.mining_modules[0]
            .cycle_completes_at_ms
            .is_some()
        {
            break;
        }
    }
    let due = f.world().pilots["Pilot"].view.mining_modules[0]
        .cycle_completes_at_ms
        .unwrap();
    while f.now + 100 < due {
        f.step();
    }
    f.ok(Request::AssignMiningDrone {
        id: 1,
        target_id: ids[1],
    });
    let before = f.world();
    let aim = &before.accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .aims
        .as_ref()
        .unwrap()[0];
    assert!(
        crate::turret_aim_collision::clear_target(&before, "Pilot", 0, aim, f.now).is_some(),
        "old body pose must leave ray clear"
    );
    let ore = before.pilots["Pilot"].view.ore;
    let power = before.pilots["Pilot"].view.power.as_ref().unwrap().charge_j;
    f.step();
    assert_eq!(f.now, due);
    let after = f.world();
    let body = after.pilots["Pilot"]
        .view
        .drones
        .iter()
        .find(|d| d.id == 1)
        .unwrap();
    assert!(
        (body.position[1] - 9.864).abs() < 4.0,
        "actual accepted body must cross ray: {:?}",
        body.position
    );
    assert_eq!(after.pilots["Pilot"].view.ore, ore);
    assert!(
        after.pilots["Pilot"].view.power.as_ref().unwrap().charge_j >= power,
        "no turret debit at blocked due tick"
    );
    assert_eq!(
        after.accounts["Pilot"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .turrets
            .aims
            .as_ref()
            .unwrap()[0]
            .status,
        arkline_protocol::parallel_turret_aim::TurretAimStatus::Obstructed
    );
    arkline_verifier::verify(&f.ok(Request::ExportLedger).ledger.unwrap(), None).unwrap();
}

#[test]
#[ignore = "Bounded actual Request/Tick/SQLite mixed ship-gun/drone/service capacity replay"]
fn real_two_turrets_drone_claim_and_coupled_service_share_ship_headroom() {
    let mut f = parallel_tests::fixture();
    super::service_tests::launch(&mut f, 1);
    super::service_tests::mine(&mut f, 1, 40, -1.);
    super::service_tests::launch(&mut f, 11);
    let drone_target = super::service_tests::mine(&mut f, 11, 10, 1.);
    let mut w = f.world();
    let gun_target = w
        .asteroids
        .iter_mut()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .nth(2)
        .unwrap();
    gun_target.position = [1010.8, 9.864, -5.2];
    let target = gun_target.id;
    f.write(&w);
    f.ok(Request::LockAsteroid { id: target });
    for _ in 0..200 {
        if f.world().pilots["Pilot"]
            .view
            .locked_asteroids
            .contains(&target)
        {
            break;
        }
        f.step();
    }
    f.ok(Request::StartMining {
        module_id: 0,
        target_id: target,
    });
    f.ok(Request::StartMining {
        module_id: 1,
        target_id: target,
    });
    // Reach real near-boundary ship ore while both genuinely mined drone
    // payloads remain reserved. No balances or reserve quantities are injected.
    for tick in 0..200 {
        f.coarse_step();
        let w = f.world();
        assert!(
            w.pilots["Pilot"]
                .view
                .ore
                .checked_add(state(&w, "Pilot").unwrap().ore.reserved_incoming().unwrap())
                .unwrap()
                <= cargo_capacity(ShipClass::Pioneer)
        );
        if tick % 30 == 0 {
            eprintln!(
                "actual gun prefill tick{tick} ship{} aims{:?}",
                w.pilots["Pilot"].view.ore,
                w.accounts["Pilot"]
                    .cockpit_mechanisms
                    .as_ref()
                    .unwrap()
                    .turrets
                    .aims
            );
        }
        if w.pilots["Pilot"].view.ore >= 700 {
            break;
        }
    }
    assert!(
        f.world().pilots["Pilot"].view.ore >= 700,
        "real ship-gun events must establish near-boundary prefill"
    );
    let service = super::service_tests::service(&mut f, 1, false);
    f.ok(Request::AssignMiningDrone {
        id: 11,
        target_id: drone_target,
    });
    let mut simultaneous = false;
    let mut full = false;
    for tick in 0..500 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        let total = w.pilots["Pilot"]
            .view
            .ore
            .checked_add(s.ore.reserved_incoming().unwrap())
            .unwrap();
        assert!(total <= cargo_capacity(ShipClass::Pioneer));
        simultaneous |= w.pilots["Pilot"]
            .view
            .mining_modules
            .iter()
            .filter(|m| m.target_id.is_some())
            .count()
            == 2
            && s.ore
                .holds
                .iter()
                .any(|h| h.drone_id == 11 && h.cycle.is_some())
            && s.inventory.operations.iter().any(|op| {
                op.operation_id == service
                    && op.drone_id == 1
                    && !op.phase.terminal()
                    && matches!(op.action, CarrierInventoryAction::ReturnToUnload { .. })
            })
            && s.ore
                .holds
                .iter()
                .any(|h| h.drone_id == 1 && h.ore_units == 40);
        if tick % 30 == 0 {
            eprintln!(
                "mixed gun/drone/service tick{tick} ship{} reserved{} total{total}",
                w.pilots["Pilot"].view.ore,
                s.ore.reserved_incoming().unwrap()
            );
        }
        full |= total == cargo_capacity(ShipClass::Pioneer);
        if full
            && s.ore.services.iter().any(|x| {
                x.physical_operation_id == service
                    && x.closed_at_ms.is_some()
                    && x.receipts.iter().map(|r| r.units).sum::<u64>() == 40
            })
        {
            break;
        }
    }
    assert!(
        simultaneous,
        "must observe both active ship guns, a separate drone claim, and an actually owned incoming40-unit ReturnToUnload reservation"
    );
    assert!(
        full,
        "actual events must fill the shared headroom without quantity injection"
    );
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.ore
            .services
            .iter()
            .filter(|x| x.physical_operation_id == service)
            .flat_map(|x| &x.receipts)
            .map(|r| r.units)
            .sum::<u64>(),
        40
    );
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    let turret_events = export
        .entries
        .iter()
        .filter(|e| matches!(e.event, EconomicEvent::OreMined { .. }))
        .count();
    f.reopen();
    for _ in 0..10 {
        f.coarse_step();
        let w = f.world();
        assert!(
            w.pilots["Pilot"].view.ore
                + state(&w, "Pilot").unwrap().ore.reserved_incoming().unwrap()
                <= cargo_capacity(ShipClass::Pioneer)
        );
    }
    let reopened = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&reopened, None).unwrap();
    assert_eq!(reopened.entries.iter().filter(|e| matches!(e.event, EconomicEvent::OreMined { .. })).count(), turret_events,
        "restart must not replay ship-gun yield; legitimate reserved service transfers remain allowed");
}

#[test]
fn malformed_persisted_turret_state_is_refused_before_any_reopen_write() {
    let mut f = parallel_tests::fixture();
    f.step();
    let original = f.world();
    validate_history(&original, &f.server.db).unwrap();
    for label in ["numeric", "stale-aim", "foreign-vessel"] {
        let mut wrong = original.clone();
        let mechanisms = wrong
            .accounts
            .get_mut("Pilot")
            .unwrap()
            .cockpit_mechanisms
            .as_mut()
            .unwrap();
        match label {
            "numeric" => mechanisms.turrets.actuators[0].from_fraction = 1.01,
            "stale-aim" => {
                let aim = arkline_protocol::parallel_turret_aim::TurretAim {
                    sample_ms: mechanisms.turrets.updated_at_ms.saturating_sub(1),
                    ..Default::default()
                };
                mechanisms.turrets.aims = Some([aim.clone(), aim]);
            }
            "foreign-vessel" => mechanisms.vessel_id = "foreign-turret-vessel".into(),
            _ => unreachable!(),
        }
        let path = f
            .path
            .parent()
            .unwrap()
            .join(format!("turret-invalid-{label}.sqlite"));
        f.server
            .db
            .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
            .unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&wrong).unwrap()],
        )
        .unwrap();
        drop(db);
        let before = std::fs::read(&path).unwrap();
        let reopened = Server::open_editor_realm_with_scene(
            &path,
            default_pioneer_room_layout(),
            SceneLayoutRevision::DockingV1,
        );
        assert!(
            reopened.is_err(),
            "invalid {label} turret state was accepted"
        );
        eprintln!("turret preflight {label}: {}", reopened.err().unwrap());
        assert_eq!(
            std::fs::read(&path).unwrap(),
            before,
            "reopen wrote before rejecting {label}"
        );
    }
}

#[test]
fn real_bridge_obstruction_and_out_of_arc_requests_never_extract() {
    for yaw in [25_f64, 30.] {
        let mut f = parallel_tests::fixture();
        let mut w = f.world();
        let a = w
            .asteroids
            .iter_mut()
            .find(|a| a.system_id == HOME_SYSTEM_ID)
            .unwrap();
        a.position = [
            10.8 + 1000. * yaw.to_radians().cos(),
            9.864,
            -5.2 + 1000. * yaw.to_radians().sin(),
        ];
        let target = a.id;
        f.write(&w);
        f.ok(Request::LockAsteroid { id: target });
        for _ in 0..200 {
            if f.world().pilots["Pilot"]
                .view
                .locked_asteroids
                .contains(&target)
            {
                break;
            }
            f.step();
        }
        let before = f.world();
        let ore = before.pilots["Pilot"].view.ore;
        let power = before.pilots["Pilot"].view.power.as_ref().unwrap().charge_j;
        let response = f.server.handle_at(
            &mut f.session,
            Request::StartMining {
                module_id: 0,
                target_id: target,
            },
            f.now,
        );
        assert_eq!(response.ok, yaw == 25., "{}", response.message);
        if response.ok {
            let accepted = f.world().accounts["Pilot"]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .turrets
                .aims
                .clone();
            f.ok(Request::Snapshot);
            assert_eq!(
                f.world().accounts["Pilot"]
                    .cockpit_mechanisms
                    .as_ref()
                    .unwrap()
                    .turrets
                    .aims,
                accepted,
                "a new non-neutral intent cannot spend pre-request elapsed time"
            );
        }
        for _ in 0..100 {
            f.step();
        }
        let after = f.world();
        assert_eq!(after.pilots["Pilot"].view.ore, ore);
        assert!(after.pilots["Pilot"].view.power.as_ref().unwrap().charge_j >= power);
        if yaw == 25. {
            assert_eq!(
                after.accounts["Pilot"]
                    .cockpit_mechanisms
                    .as_ref()
                    .unwrap()
                    .turrets
                    .aims
                    .as_ref()
                    .unwrap()[0]
                    .status,
                arkline_protocol::parallel_turret_aim::TurretAimStatus::Obstructed
            );
            f.ok(Request::StopMining { module_id: 0 });
            assert!(f.world().accounts["Pilot"]
                .cockpit_mechanisms
                .as_ref()
                .unwrap()
                .turrets
                .aims
                .as_ref()
                .unwrap()[0]
                .target_id
                .is_none());
        }
        arkline_verifier::verify(&f.ok(Request::ExportLedger).ledger.unwrap(), None).unwrap();
    }
}

#[test]
#[ignore = "Actual low-load same-timestamp turret extraction and coupled transfer transaction"]
fn real_gun_and_coupled_transfer_commit_at_one_accepted_clock() {
    let mut f = parallel_tests::fixture();
    super::service_tests::launch(&mut f, 1);
    super::service_tests::mine(&mut f, 1, 40, -1.);
    let mut w = f.world();
    let a = w
        .asteroids
        .iter_mut()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .nth(2)
        .unwrap();
    a.position = [1010.8, 9.864, -5.2];
    let target = a.id;
    f.write(&w);
    f.ok(Request::LockAsteroid { id: target });
    for _ in 0..200 {
        if f.world().pilots["Pilot"]
            .view
            .locked_asteroids
            .contains(&target)
        {
            break;
        }
        f.step();
    }
    let operation = super::service_tests::service(&mut f, 1, false);
    for _ in 0..3000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.ore.granted_service_id.is_some() {
            assert!(s
                .ore
                .services
                .iter()
                .flat_map(|a| &a.receipts)
                .next()
                .is_none());
            break;
        }
    }
    assert!(state(&f.world(), "Pilot")
        .unwrap()
        .ore
        .granted_service_id
        .is_some());
    // Real captain Hold/Resume keeps reached source contact, but closes/reopens
    // the manifold attempt. Align its ordinary 500ms cadence with the ordinary
    // gun's six-second deadline; never alter either clock/rate or payload.
    f.request(CarrierInventoryRequest::Hold {
        binding: f.binding(),
        operation_id: operation,
    });
    f.ok(Request::StartMining {
        module_id: 0,
        target_id: target,
    });
    let mut accepted_due = None;
    for _ in 0..100 {
        f.step();
        accepted_due = f.world().pilots["Pilot"].view.mining_modules[0].cycle_completes_at_ms;
        if accepted_due.is_some() {
            break;
        }
    }
    let due = accepted_due.expect("held source-coupled payload must leave forward gun clear");
    while f.now < due - DRONE_DROPOFF_BATCH_MS {
        f.step();
    }
    assert_eq!(f.now, due - DRONE_DROPOFF_BATCH_MS);
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: operation,
    });
    while f.now < due {
        f.step();
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    let run = &s.runs[&operation];
    assert_eq!(run.confirmed_at_ms, due);
    assert!(run.waiting_for_transfer && run.reached.ore_coupled && !run.held);
    let body = run
        .confirmed
        .poses
        .iter()
        .find(|(key, _)| *key == CarrierPoseKey::Drone)
        .unwrap()
        .1;
    let expected = run.parent.compose(body).unwrap();
    let drone = w.pilots["Pilot"]
        .view
        .drones
        .iter()
        .find(|d| d.id == 1)
        .unwrap();
    assert_eq!(drone.position, expected.position_m);
    assert_eq!(drone.orientation_xyzw, Some(expected.orientation_xyzw));
    assert_eq!(drone.velocity, [0.; 3]);
    assert_eq!(drone.updated_at_ms, due);
    assert_eq!(
        w.pilots["Pilot"].view.ore,
        mining_spec(ShipClass::Pioneer).yield_units + DRONE_DROPOFF_BATCH_UNITS
    );
    assert!(
        w.pilots["Pilot"].view.ore + s.ore.reserved_incoming().unwrap()
            <= cargo_capacity(ShipClass::Pioneer)
    );
    f.snapshot_valid();
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    assert!(export
        .entries
        .iter()
        .any(|e| e.timestamp_ms == due && matches!(e.event, EconomicEvent::OreMined { .. })));
    assert!(export.entries.iter().any(|e| e.timestamp_ms == due && matches!(e.event,
        EconomicEvent::CarrierInventory { record: CarrierInventoryLedgerEvent::Transferred { physical_operation_id, .. } } if physical_operation_id == operation)));
    f.ok(Request::StopMining { module_id: 0 });
    for _ in 0..10 {
        f.step();
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    let mut receipts: Vec<_> = s.ore.services.iter().flat_map(|a| &a.receipts).collect();
    receipts.sort_by_key(|r| r.committed_at_ms);
    assert_eq!(receipts.iter().map(|r| r.units).sum::<u64>(), 40);
    assert!(receipts
        .windows(2)
        .all(|r| r[1].committed_at_ms - r[0].committed_at_ms >= DRONE_DROPOFF_BATCH_MS));
    assert_eq!(
        w.pilots["Pilot"].view.ore,
        40 + mining_spec(ShipClass::Pioneer).yield_units
    );
    let before = w.pilots["Pilot"].view.ore;
    f.reopen();
    for _ in 0..10 {
        f.step();
    }
    assert_eq!(f.world().pilots["Pilot"].view.ore, before);
    arkline_verifier::verify(&f.ok(Request::ExportLedger).ledger.unwrap(), None).unwrap();
    eprintln!("actual same-clock turret+coupled transfer accepted at {due};40 transfer units and one gun yield, no reopen replay");
}
