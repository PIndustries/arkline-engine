use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
fn setup(s: &mut Server, class: ShipClass) -> Option<String> {
    let mut p = None;
    ok(
        s,
        &mut p,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: "Pilot".into(),
            resume_token: None,
        },
        0,
    );
    ok(s, &mut p, Request::SelectShip { ship_class: class }, 0);
    ok(s, &mut p, Request::Undock, 0);
    p
}
fn norm(v: [f64; 3]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(std::array::from_fn(|i| a[i] - b[i]))
}
#[test]
fn stop_preserves_momentum_then_brakes_with_hull_mass_and_actual_acceleration() {
    for class in [ShipClass::Pioneer, ShipClass::Monolith] {
        let mut s = Server::open(":memory:").unwrap();
        let mut p = setup(&mut s, class);
        ok(
            &mut s,
            &mut p,
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3],
            },
            0,
        );
        let moving = ok(&mut s, &mut p, Request::Snapshot, 400);
        let before = moving.pilots[0].flight.as_ref().unwrap();
        assert!((before.velocity[0] - ship_acceleration(class, 0) * 0.4).abs() < 1e-6);
        assert!((before.acceleration[0] - ship_acceleration(class, 0)).abs() < 1e-6);
        let stopping = ok(
            &mut s,
            &mut p,
            Request::Navigate {
                mode: NavigationMode::Stop,
            },
            400,
        );
        assert_eq!(
            stopping.pilots[0].flight.as_ref().unwrap().velocity,
            before.velocity
        );
        let drift = ok(&mut s, &mut p, Request::Snapshot, 500);
        let f = drift.pilots[0].flight.as_ref().unwrap();
        assert!(f.velocity[0] > 0.0 && f.velocity[0] < before.velocity[0]);
        assert!(f.position[0] > before.position[0]);
        assert!((f.acceleration[0] + ship_braking_acceleration(class, 0)).abs() < 1e-6);
        let stopped = ok(&mut s, &mut p, Request::Snapshot, 2000);
        assert_eq!(
            stopped.pilots[0].flight.as_ref().unwrap().velocity,
            [0.0; 3]
        );
        assert_eq!(stopped.ledger_hash, moving.ledger_hash);
    }
    assert!(ship_acceleration(ShipClass::Monolith, 0) < ship_acceleration(ShipClass::Pioneer, 0));
    assert!(ship_acceleration(ShipClass::Pioneer, 1000) < ship_acceleration(ShipClass::Pioneer, 0));
}
#[test]
fn explicit_drone_orbit_moves_continuously_in_range_and_hold_stops_it() {
    let mut s = Server::open(":memory:").unwrap();
    let mut p = setup(&mut s, ShipClass::Pioneer);
    let initial = ok(
        &mut s,
        &mut p,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        0,
    );
    let d = &initial.pilots[0].drones[0];
    let id = d.id;
    let mut position = d.position;
    let center = initial.pilots[0].flight.as_ref().unwrap().position;
    let mut travelled = 0.0;
    assert!(
        !s.handle_at(
            &mut p,
            Request::OrbitDrone {
                id,
                target: NavigationTarget::Station
            },
            0
        )
        .ok
    );
    assert!(
        !s.handle_at(
            &mut p,
            Request::OrbitDrone {
                id: id + 1,
                target: NavigationTarget::Pilot {
                    pilot: "Pilot".into()
                }
            },
            0
        )
        .ok
    );
    ok(
        &mut s,
        &mut p,
        Request::OrbitDrone {
            id,
            target: NavigationTarget::Pilot {
                pilot: "Pilot".into(),
            },
        },
        0,
    );
    for t in (100..=20000).step_by(100) {
        s.tick_at(t).unwrap();
        let snap = ok(&mut s, &mut p, Request::Snapshot, t);
        let d = &snap.pilots[0].drones[0];
        let step = distance(position, d.position);
        assert!(step <= drone_speed(d.class) * 0.1 + 0.001);
        travelled += step;
        position = d.position;
        if t >= 10000 {
            assert!(
                (distance(position, center) - (hull_radius_m(ShipClass::Pioneer) + 300.0)).abs()
                    < 3.0
            );
        }
    }
    assert!(travelled > 1000.0);
    let held = ok(&mut s, &mut p, Request::DroneHoldById { id }, 20000);
    assert_eq!(held.pilots[0].drones[0].velocity, [0.0; 3]);
    let held_position = held.pilots[0].drones[0].position;
    assert_eq!(
        ok(&mut s, &mut p, Request::Snapshot, 21000).pilots[0].drones[0].position,
        held_position
    );
    assert_eq!(held.ledger_hash, initial.ledger_hash);
    assert!(Request::OrbitDrone {
        id,
        target: NavigationTarget::Pilot {
            pilot: "Pilot".into()
        }
    }
    .is_remote_gameplay());
}
