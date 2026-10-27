//! Actual source inventory and captain stow, followed by an explicitly re-anchored
//! geometry fixture. These tests do NOT admit flight or execute a Dock request.
use super::*;

fn secured_fixture() -> Fixture {
    let mut f = parallel_tests::fixture();
    let vessel_id = f.binding().vessel_id;
    f.ok(Request::SetTurretDeployment {
        vessel_id,
        deployed: false,
    });
    for _ in 0..24 {
        f.step();
    }
    secured_for_departure(&f.world(), "Pilot", f.now).unwrap();
    f
}
fn anchored(f: &Fixture) -> (World, DockingSegment) {
    let mut w = f.world();
    let flight = w
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .flight
        .as_mut()
        .unwrap();
    let berth = flight.docking.as_ref().unwrap().berth_id;
    flight.position = docking_gate_position(berth);
    flight.orientation = docking_berth_orientation();
    flight.heading = [-1., 0., 0.];
    let s = DockingSegment {
        source_m: flight.position,
        target_m: docking_berth_position(berth),
        initial_velocity_mps: [0.; 3],
        source_orientation: flight.orientation,
        target_orientation: flight.orientation,
        started_at_ms: f.now,
        duration_ms: 10_000,
    };
    (w, s)
}
fn checks(w: &World, s: &DockingSegment) -> (bool, bool, bool) {
    crate::docking::source_route_checks_for_test(w, "Pilot", s)
}
fn prefix(s: &DockingSegment) -> DockingSegment {
    let mut p = s.clone();
    p.target_m[0] = p.source_m[0] - 10.;
    p
}

#[test]
fn exact_1703_secured_corridor_requires_owner_assignment_and_confirmed_idle() {
    let f = secured_fixture();
    let (w, s) = anchored(&f);
    secured_for_departure(&w, "Pilot", f.now).unwrap();
    assert_eq!(checks(&w, &s), (true, true, true));
    assert_eq!(checks(&w, &prefix(&s)), (true, true, true));
    assert!(geometry::lookup(w.carrier_inventory_registration.as_ref().unwrap()).is_none());

    let mut bad = w.clone();
    bad.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .flight
        .as_mut()
        .unwrap()
        .docking
        .as_mut()
        .unwrap()
        .host_vessel_id = "foreign-vessel".into();
    // A station-clear prefix is not an alternative path around ownership.
    assert_eq!(checks(&bad, &prefix(&s)), (false, false, false));
    let mut bad = w.clone();
    bad.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .flight
        .as_mut()
        .unwrap()
        .docking = None;
    assert_eq!(checks(&bad, &prefix(&s)), (false, false, false));
    let mut bad = w.clone();
    bad.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .flight
        .as_mut()
        .unwrap()
        .docking
        .as_mut()
        .unwrap()
        .berth_id = DOCKING_BERTH_COUNT;
    assert_eq!(checks(&bad, &prefix(&s)), (false, false, false));

    let mut bad = w.clone();
    bad.accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .machinery
        .values_mut()
        .next()
        .unwrap()
        .position_m[0] += 0.001;
    assert_eq!(checks(&bad, &s), (false, true, false));
    let mut bad = w.clone();
    let turret = &mut bad
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .cockpit_mechanisms
        .as_mut()
        .unwrap()
        .turrets;
    turret.actuators[0].blocked = Some("Fixture held motor".into());
    assert!(!checks(&bad, &s).2);
    let mut bad = w.clone();
    bad.accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .inventory
        .binding
        .vessel_id = "foreign-vessel".into();
    assert!(!checks(&bad, &s).2);

    let mut moving = w.clone();
    moving
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .flight
        .as_mut()
        .unwrap()
        .velocity[0] = -1.;
    assert!(
        !checks(&moving, &s).2,
        "Stationary validation is deliberately not waived"
    );
}

#[test]
fn exact_1703_secured_corridor_rechecks_external_hazards_on_clear_prefix() {
    let f = secured_fixture();
    let (w, s) = anchored(&f);
    let short = prefix(&s);
    let center = std::array::from_fn(|i| (short.source_m[i] + short.target_m[i]) * 0.5);
    let mut bad = w.clone();
    let system = bad.pilots["Pilot"].view.system_id;
    let asteroid = bad.asteroids.first_mut().unwrap();
    asteroid.system_id = system;
    asteroid.position = center;
    asteroid.radius = 1.;
    assert!(
        checks(&bad, &short).1,
        "Station source geometry remains certified"
    );
    assert!(
        !checks(&bad, &short).2,
        "Clear station geometry cannot bypass asteroids"
    );

    // Explicit external hazard fixtures, not newly accepted player identities.
    // No account is added: owned carrier state and all twenty IDs remain intact.
    let mut peer = w.pilots["Pilot"].clone();
    peer.view.ship_class = ShipClass::Prospector;
    peer.view.location = Location::Docked;
    peer.view.drones.clear();
    let flight = peer.view.flight.as_mut().unwrap();
    flight.position = std::array::from_fn(|i| (s.source_m[i] + s.target_m[i]) * 0.5);
    flight.docking = None;
    flight.updated_at_ms = f.now;
    let mut bad = w.clone();
    bad.pilots.insert("HazardFixture".into(), peer.clone());
    assert!(
        !checks(&bad, &s).2,
        "Current peer hull must remain obstructing"
    );
    peer.view.flight.as_mut().unwrap().position[1] += 10_000.;
    let mut clear = w.clone();
    clear.pilots.insert("HazardFixture".into(), peer.clone());
    assert_eq!(checks(&clear, &short), (true, true, true));
    let future_drone = [center[0], center[1] + 900., center[2]];
    let drone: DroneView = serde_json::from_value(serde_json::json!({
        "class":"mining", "position":future_drone, "velocity":[0.,0.,0.],
        "heading":[1.,0.,0.], "mode":{"kind":"hold"}, "updated_at_ms":f.now
    }))
    .unwrap();
    peer.view.drones.push(drone);
    let mut bad = w.clone();
    bad.pilots.insert("HazardFixture".into(), peer);
    secured_for_departure(&bad, "Pilot", f.now).unwrap();
    assert!(
        !checks(&bad, &short).2,
        "The full future free-body horizon is blocked even when current secure roots clear"
    );
}

#[test]
fn exact_1703_corridor_refuses_actual_deployed_id_and_pending_work() {
    let mut f = secured_fixture();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::Launch,
    });
    let (pending, s) = anchored(&f);
    assert!(!checks(&pending, &s).2);
    for _ in 0..60 {
        f.coarse_step();
        let w = f.world();
        let owned = state(&w, "Pilot").unwrap();
        if owned.runs.is_empty() && owned.departures.is_empty() {
            break;
        }
    }
    let w = f.world();
    let owned = state(&w, "Pilot").unwrap();
    assert_eq!(
        owned.inventory.entry(1).unwrap().location,
        CarrierDroneLocation::Deployed
    );
    assert!(owned.runs.is_empty() && owned.departures.is_empty());
    assert!(inventory_secured(&w, "Pilot").is_err());
    let (deployed, s) = anchored(&f);
    assert!(!checks(&deployed, &s).2);
}
