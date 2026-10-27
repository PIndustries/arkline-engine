//! Actual exact1703 stationary source controls. Does NOT waive travel admission.
use super::*;
fn turret(f: &Fixture) -> TurretDeployment {
    f.world().accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets
        .clone()
}
fn command(f: &mut Fixture, deployed: bool) {
    let vessel = f.world().accounts["Pilot"]
        .vessel_id(ShipClass::Pioneer)
        .unwrap()
        .to_owned();
    f.ok(Request::SetTurretDeployment {
        vessel_id: vessel,
        deployed,
    });
}
#[test]
fn actual_1703_turret_stow_repeat_stop_resume_and_departure_gate() {
    let mut f = parallel_tests::fixture();
    assert!(turret(&f).source_admitted);
    command(&mut f, false);
    let first = turret(&f);
    command(&mut f, false);
    assert_eq!(turret(&f).actuators, first.actuators);
    for _ in 0..6 {
        f.step();
    }
    let mid = turret(&f);
    assert!(
        mid.fractions_at(f.now).iter().all(|v| *v > 0. && *v < 1.),
        "first={first:?} mid={mid:?} now={}",
        f.now
    );
    f.ok(Request::Navigate {
        mode: NavigationMode::Stop,
    });
    let held = turret(&f);
    for _ in 0..30 {
        f.step();
    }
    assert_eq!(
        turret(&f).fractions_at(f.now),
        held.fractions_at(held.updated_at_ms)
    );
    assert!(!turret(&f).secure_at(f.now));
    command(&mut f, false);
    for _ in 0..24 {
        f.step();
    }
    assert!(turret(&f).secure_at(f.now));
    let before = f.world().pilots["Pilot"].view.power.clone();
    let response = f.server.handle_at(
        &mut f.session,
        Request::Warp {
            destination: Destination::Belt,
        },
        f.now,
    );
    assert!(
        !response.ok,
        "source stow alone MUST NOT admit stationary-world warp"
    );
    assert_eq!(f.world().pilots["Pilot"].view.power, before);
    assert!(turret(&f).pending_warp.is_none());
    let response = f
        .server
        .handle_at(&mut f.session, Request::Jump { gate_id: 1 }, f.now);
    assert!(!response.ok);
}
#[test]
fn actual_1703_restart_cancels_intent_without_replaying_or_advancing_held_pose() {
    let mut f = parallel_tests::fixture();
    command(&mut f, false);
    for _ in 0..5 {
        f.step();
    }
    let mut w = f.world();
    // Inject only retained intent to exercise reopen policy; this is NOT an
    // accepted warp command or transit/debit proof.
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .cockpit_mechanisms
        .as_mut()
        .unwrap()
        .turrets
        .pending_warp = Some(Destination::Belt);
    crate::turret_stow::stop_on_restart(&mut w, f.now);
    let state = &w.accounts["Pilot"]
        .cockpit_mechanisms
        .as_ref()
        .unwrap()
        .turrets;
    assert!(state.pending_warp.is_none());
    assert!(state.actuators.iter().all(|a| a.blocked.is_some()));
    let fractions = state.fractions_at(f.now);
    crate::turret_stow::advance(&mut w, f.now + 10_000);
    assert_eq!(
        w.accounts["Pilot"]
            .cockpit_mechanisms
            .as_ref()
            .unwrap()
            .turrets
            .fractions_at(f.now + 10_000),
        fractions
    );
}
#[test]
fn actual_1703_invalid_numeric_state_and_foreign_vessel_fail_closed() {
    let mut f = parallel_tests::fixture();
    let response = f.server.handle_at(
        &mut f.session,
        Request::SetTurretDeployment {
            vessel_id: "other".into(),
            deployed: false,
        },
        f.now,
    );
    assert!(!response.ok);
    let mut w = f.world();
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .cockpit_mechanisms
        .as_mut()
        .unwrap()
        .turrets
        .actuators[0]
        .from_fraction = 2.;
    assert!(crate::turret_stow::validate(&w, f.now).is_err());
}
#[test]
fn actual_1703_obstruction_holds_instead_of_claiming_closed_secure() {
    let mut f = parallel_tests::fixture();
    let mut w = f.world();
    let system = w.pilots["Pilot"].view.system_id;
    // Intentional obstacle negative control, not a normal source placement.
    let obstacle = w.asteroids.first_mut().expect("fixture asteroid");
    obstacle.system_id = system;
    obstacle.position = [12., 9., -5.2];
    obstacle.radius = 1.;
    f.write(&w);
    command(&mut f, false);
    let held = turret(&f);
    assert!(held.actuators[0].blocked.is_some());
    for _ in 0..30 {
        f.step();
    }
    assert_eq!(turret(&f).actuators[0].fraction_at(f.now), 1.);
    assert!(!turret(&f).secure_at(f.now));
}
#[test]
fn actual_1703_current_projection_rejects_stale_active_turret_clock() {
    let mut f = parallel_tests::fixture();
    command(&mut f, false);
    f.step();
    let response = f.ok(Request::Snapshot);
    let snapshot = response.snapshot.unwrap();
    let mut pilot = snapshot
        .pilots
        .into_iter()
        .find(|p| p.name == "Pilot")
        .unwrap();
    assert!(arkline_protocol::parallel_turret_binding::confirmed_state(
        &pilot,
        snapshot.server_time_ms
    )
    .is_ok());
    pilot
        .cockpit_mechanisms
        .as_mut()
        .unwrap()
        .turrets
        .updated_at_ms -= 1;
    assert!(arkline_protocol::parallel_turret_binding::confirmed_state(
        &pilot,
        snapshot.server_time_ms
    )
    .is_err());
}
#[test]
fn turret_obstacle_exclusion_uses_arrival_drone_identity_not_operation_key() {
    let f = parallel_tests::fixture();
    let mut w = f.world();
    let s = w
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap();
    let id = s.inventory.entries[0].drone_id;
    assert!(crate::turret_stow::source_bound_body(s, id));
    let operation = id + 1000;
    let pose = BayPose {
        position_m: [0.; 3],
        orientation_xyzw: [0., 0., 0., 1.],
    };
    // Deliberately conflicting ownership record for identity-filter regression,
    // not a validated world/route or accepted approach command.
    s.arrivals.insert(
        operation,
        arrival::Arrival {
            operation_id: operation,
            drone_id: id,
            cassette_id: 1,
            berth_id: 0,
            parent: pose,
            system_id: 0,
            class: DroneClass::Mining,
            original_body_pose: pose,
            admitted_at_ms: f.now,
            path: vec![pose],
            asteroid_obstacles: vec![],
            peer_obstacles: vec![],
            target_index: 0,
            started_at_ms: f.now,
            duration_ms: 0,
            confirmed: pose,
            confirmed_at_ms: f.now,
            held: true,
            hold_reason: "test".into(),
            ready_to_handoff: false,
        },
    );
    assert_ne!(operation, id);
    assert!(!crate::turret_stow::source_bound_body(s, id));
}

#[test]
fn shared_departure_security_uses_confirmed_pose_and_exact_idle_inventory() {
    let mut f = parallel_tests::fixture();
    assert!(inventory_secured(&f.world(), "Pilot").is_ok());
    assert!(secured_for_departure(&f.world(), "Pilot", f.now).is_err());
    command(&mut f, false);
    let unadvanced = f.world();
    assert!(turret(&f).secure_at(f.now + TURRET_TRAVEL_MS));
    assert!(
        secured_for_departure(&unadvanced, "Pilot", f.now + TURRET_TRAVEL_MS).is_err(),
        "Predicted motor endpoint cannot authorize departure"
    );
    for _ in 0..24 {
        f.step();
    }
    assert!(secured_for_departure(&f.world(), "Pilot", f.now).is_ok());
    let mut bad = f.world();
    let s = bad
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap();
    s.machinery.values_mut().next().unwrap().position_m[0] += 0.01;
    assert!(secured_for_departure(&bad, "Pilot", f.now).is_err());
}

#[test]
fn exact_exterior_definition_preserves_real_power_and_rejects_foreign_owner() {
    let f = parallel_tests::fixture();
    let mut w = f.world();
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .pioneer_exterior_enabled = true;
    let original = w.exterior_definition.clone();
    let first = crate::exterior_effects::view(&w, f.now);
    let vessel = &first.vessels[0];
    let source = vessel.source_definition.as_ref().unwrap();
    arkline_protocol::parallel_exterior::validate_against(source, &original).unwrap();
    assert!(vessel.powered);
    assert_eq!(vessel.load_w, original.document.load_w());
    assert_eq!(first.definition, original);
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .power
        .as_mut()
        .unwrap()
        .charge_j = 0;
    let unpowered = crate::exterior_effects::view(&w, f.now);
    assert!(!unpowered.vessels[0].powered && unpowered.vessels[0].load_w == 0);
    assert_eq!(
        unpowered.vessels[0].source_definition,
        vessel.source_definition
    );
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .inventory
        .binding
        .vessel_id
        .push_str("-foreign");
    assert!(crate::exterior_effects::view(&w, f.now).vessels[0]
        .source_definition
        .is_none());
}

#[test]
fn stow_schema_alone_cannot_resume_a_restored_source_mining_cycle() {
    let f = parallel_tests::fixture();
    let mut w = f.world();
    let id = w.asteroids[0].id;
    let p = w.pilots.get_mut("Pilot").unwrap();
    p.view.locked_asteroids.push(id);
    let spec = mining_spec(p.view.ship_class);
    let due = f.now + spec.cycle_ms;
    p.view.mining_modules[0].target_id = Some(id);
    p.view.mining_modules[0].cycle_started_at_ms = Some(f.now);
    p.view.mining_modules[0].cycle_completes_at_ms = Some(due);
    let power = p.view.power.clone();
    let ore = p.view.ore;
    let asteroid = w.asteroids.clone();
    let belt = w.belt_ore;
    assert!(crate::mining::advance(&mut w, due).is_empty());
    assert_eq!(w.pilots["Pilot"].view.power, power);
    assert_eq!(w.pilots["Pilot"].view.ore, ore);
    assert_eq!(w.asteroids, asteroid);
    assert_eq!(w.belt_ore, belt);
    assert!(w.pilots["Pilot"]
        .view
        .mining_modules
        .iter()
        .all(|m| m.target_id.is_none()));
}
