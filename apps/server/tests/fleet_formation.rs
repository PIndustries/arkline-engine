use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let response = s.handle_at(p, r, t);
    assert!(response.ok, "{}", response.message);
    response.snapshot.unwrap()
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
fn assert_formation(p: &PilotView) {
    let ship = p.flight.as_ref().unwrap();
    for (i, d) in p.drones.iter().enumerate() {
        let forward_clearance = d
            .position
            .iter()
            .zip(ship.position)
            .zip(ship.heading)
            .map(|((a, b), heading)| (a - b) * heading)
            .sum::<f64>();
        assert!(
            forward_clearance > p.ship_class.spec().length_m / 2.0 + 99.0,
            "drone inside hull bow plane"
        );
        for other in &p.drones[..i] {
            assert!(
                distance(d.position, other.position) >= 10.0,
                "overlapping launch slots"
            );
        }
    }
    assert_eq!(
        p.drone,
        p.drones.first().cloned(),
        "legacy mirror differs from first fleet member"
    );
}
#[test]
fn full_formations_clear_every_hull_at_pitch_and_preserve_first_spawn_scale() {
    for class in ALL_SHIP_CLASSES {
        let mut s = Server::open(":memory:").unwrap();
        let mut p = None;
        ok(
            &mut s,
            &mut p,
            Request::Connect {
                supports_room_controls: false,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Formation".into(),
                resume_token: None,
            },
            0,
        );
        ok(&mut s, &mut p, Request::SelectShip { ship_class: class }, 0);
        ok(&mut s, &mut p, Request::Undock, 0);
        // Pitch down ninety degrees: world-up offsets would put large-hull
        // launches back inside the ship, so clearance must use its quaternion.
        for t in [0, 400, 800] {
            ok(
                &mut s,
                &mut p,
                Request::PilotInput {
                    translation: [0.0; 3],
                    rotation: [0.0, 0.0, -1.0],
                },
                t,
            );
        }
        ok(
            &mut s,
            &mut p,
            Request::Navigate {
                mode: NavigationMode::Stop,
            },
            1000,
        );
        for drone_class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
            for _ in 0..DRONE_CLASS_CAP {
                ok(
                    &mut s,
                    &mut p,
                    Request::LaunchDroneClass { class: drone_class },
                    1000,
                );
            }
        }
        let snapshot = ok(&mut s, &mut p, Request::Snapshot, 1000);
        let pilot = &snapshot.pilots[0];
        assert_eq!(pilot.drones.len(), 15);
        assert_formation(pilot);
        if class == ShipClass::Pioneer {
            let first_distance = distance(
                pilot.drones[0].position,
                pilot.flight.as_ref().unwrap().position,
            );
            assert!((first_distance - 148.246416483).abs() < 0.01);
        }
    }
}
#[test]
fn recalled_middle_slot_is_reused_without_overlap_and_first_mirror_tracks_removal() {
    let mut s = Server::open(":memory:").unwrap();
    let mut p = None;
    ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Slots".into(),
            resume_token: None,
        },
        0,
    );
    ok(&mut s, &mut p, Request::Undock, 0);
    for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
        for _ in 0..DRONE_CLASS_CAP {
            ok(&mut s, &mut p, Request::LaunchDroneClass { class }, 0);
        }
    }
    let original = ok(&mut s, &mut p, Request::Snapshot, 0);
    let middle = original.pilots[0].drones[3].id;
    ok(&mut s, &mut p, Request::RecallDroneById { id: middle }, 0);
    s.tick_at(10_000).unwrap();
    let replaced = ok(
        &mut s,
        &mut p,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        10_000,
    );
    assert_eq!(replaced.pilots[0].drones.len(), 15);
    assert_formation(&replaced.pilots[0]);
    let first = replaced.pilots[0].drones[0].id;
    let next = replaced.pilots[0].drones[1].id;
    ok(
        &mut s,
        &mut p,
        Request::RecallDroneById { id: first },
        10_000,
    );
    let removed = ok(&mut s, &mut p, Request::Snapshot, 20_000);
    assert_eq!(removed.pilots[0].drone.as_ref().unwrap().id, next);
    assert_formation(&removed.pilots[0]);
}
