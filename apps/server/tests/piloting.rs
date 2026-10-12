use arkline_protocol::*;
use arkline_server::Server;
fn run(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
fn create(s: &mut Server) -> (Option<String>, String) {
    let mut p = None;
    let r = s.handle_at(
        &mut p,
        Request::Connect {
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Pilot".into(),
            resume_token: None,
        },
        0,
    );
    assert!(r.ok);
    let token = r.resume_token.unwrap();
    run(s, &mut p, Request::Undock, 0);
    (p, token)
}
fn state(s: &mut Server, t: u64) -> FlightState {
    run(s, &mut None, Request::Snapshot, t).pilots[0]
        .flight
        .clone()
        .unwrap()
}
fn input(
    s: &mut Server,
    p: &mut Option<String>,
    translation: [f64; 3],
    rotation: [f64; 3],
    t: u64,
) {
    run(
        s,
        p,
        Request::PilotInput {
            translation,
            rotation,
        },
        t,
    );
}
fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}
#[test]
fn all_six_translation_axes_preserve_attitude_and_obey_acceleration() {
    for axis in 0..3 {
        for sign in [-1.0, 1.0] {
            let mut s = Server::open(":memory:").unwrap();
            let (mut p, _) = create(&mut s);
            let before = state(&mut s, 0);
            let mut thrust = [0.0; 3];
            thrust[axis] = sign;
            input(&mut s, &mut p, thrust, [0.0; 3], 0);
            s.tick_at(400).unwrap();
            let f = state(&mut s, 400);
            close(f.velocity[axis], sign * SHIP_ACCELERATION * 0.4);
            close(
                f.position[axis] - before.position[axis],
                sign * SHIP_ACCELERATION * 0.4 * 0.4 * 0.5,
            );
            assert_eq!(f.orientation, before.orientation);
            assert_eq!(f.heading, [1.0, 0.0, 0.0]);
        }
    }
}
#[test]
fn body_rotations_include_roll_and_forward_thrust_follows_rotated_axes() {
    for axis in 0..3 {
        let mut s = Server::open(":memory:").unwrap();
        let (mut p, _) = create(&mut s);
        let mut rotation = [0.0; 3];
        rotation[axis] = 1.0;
        for t in (0..1000).step_by(100) {
            input(&mut s, &mut p, [0.0; 3], rotation, t);
        }
        let f = state(&mut s, 1000);
        close(f.orientation[axis].abs(), std::f64::consts::FRAC_1_SQRT_2);
        close(f.orientation[3].abs(), std::f64::consts::FRAC_1_SQRT_2);
        if axis == 0 {
            assert_eq!(f.heading, [1.0, 0.0, 0.0]);
        }
    }
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, _) = create(&mut s);
    for t in (0..1000).step_by(100) {
        input(&mut s, &mut p, [0.0; 3], [0.0, 1.0, 0.0], t);
    }
    input(&mut s, &mut p, [1.0, 0.0, 0.0], [0.0; 3], 1000);
    let f = state(&mut s, 1400);
    assert!(f.velocity[2] < -59.9);
    assert!(f.velocity[0].abs() < 1e-6);
}
#[test]
fn lease_expiry_neutral_and_invalid_inputs_are_safe() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, _) = create(&mut s);
    let hash = run(&mut s, &mut p, Request::Snapshot, 0).ledger_hash;
    for translation in [
        [f64::NAN, 0.0, 0.0],
        [1.1, 0.0, 0.0],
        [f64::INFINITY, 0.0, 0.0],
    ] {
        assert!(
            !s.handle_at(
                &mut p,
                Request::PilotInput {
                    translation,
                    rotation: [0.0; 3]
                },
                0
            )
            .ok
        );
    }
    input(&mut s, &mut p, [1.0, 0.0, 0.0], [1.0, 0.0, 0.0], 0);
    let expired = state(&mut s, PILOT_INPUT_LEASE_MS);
    let orientation = expired.orientation;
    assert_eq!(expired.angular_velocity, [0.0; 3]);
    assert_eq!(expired.mode, NavigationMode::Stop);
    let stopped = state(&mut s, 2000);
    for (a, b) in stopped.orientation.into_iter().zip(orientation) {
        close(a, b);
    }
    assert_eq!(stopped.velocity, [0.0; 3]);
    input(&mut s, &mut p, [0.0; 3], [0.0; 3], 2000);
    for (a, b) in state(&mut s, 2100).orientation.into_iter().zip(orientation) {
        close(a, b);
    }
    run(&mut s, &mut p, Request::Dock, 2100);
    assert!(
        !s.handle_at(
            &mut p,
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3]
            },
            2100
        )
        .ok
    );
    run(&mut s, &mut p, Request::Undock, 2100);
    run(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        2100,
    );
    assert!(
        !s.handle_at(
            &mut p,
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3]
            },
            2101
        )
        .ok
    );
    assert_eq!(
        run(&mut s, &mut p, Request::Snapshot, 2101).ledger_hash,
        hash
    );
}
#[test]
fn logout_and_restart_preserve_full_quaternion_and_do_not_resume_thrust() {
    let path = std::env::temp_dir().join(format!(
        "arkline-piloting-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let token;
    let saved;
    {
        let mut s = Server::open(&path).unwrap();
        let (mut p, t) = create(&mut s);
        token = t;
        input(&mut s, &mut p, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0], 0);
        saved = state(&mut s, 400);
        s.disconnect(&mut p);
        let stopped = state(&mut s, 800);
        assert_eq!(stopped.position, saved.position);
        assert_eq!(stopped.orientation, saved.orientation);
        assert_eq!(stopped.angular_velocity, [0.0; 3]);
    }
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = None;
        let snap = run(
            &mut s,
            &mut p,
            Request::Connect {
                supports_room_controls: false,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Pilot".into(),
                resume_token: Some(token),
            },
            1000,
        );
        let f = snap.pilots[0].flight.as_ref().unwrap();
        assert_eq!(f.orientation, saved.orientation);
        assert_eq!(f.position, saved.position);
        assert_eq!(f.velocity, [0.0; 3]);
        assert_eq!(f.angular_velocity, [0.0; 3]);
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}

#[test]
fn abrupt_restart_clears_live_input_and_migrates_legacy_heading() {
    let path = std::env::temp_dir().join(format!(
        "arkline-piloting-reset-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let saved;
    {
        let mut s = Server::open(&path).unwrap();
        let (mut p, _) = create(&mut s);
        input(&mut s, &mut p, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0);
        saved = state(&mut s, 400);
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let json: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old["pilots"]["Pilot"]["view"]["flight"]
            .as_object_mut()
            .unwrap()
            .remove("orientation");
        db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
    }
    {
        let mut s = Server::open(&path).unwrap();
        let restored = state(&mut s, 1000);
        assert_eq!(restored.mode, NavigationMode::Stop);
        assert_eq!(restored.velocity, [0.0; 3]);
        assert_eq!(restored.angular_velocity, [0.0; 3]);
        for (a, b) in restored.orientation.into_iter().zip(saved.orientation) {
            close(a, b)
        }
        for (a, b) in restored.heading.into_iter().zip(saved.heading) {
            close(a, b)
        }
        // The legacy JSON fixture is decoded and re-encoded once more than
        // normal recovery; allow sub-nanometre floating-point roundoff only.
        for (a, b) in restored.position.into_iter().zip(saved.position) {
            assert!((a - b).abs() < 1e-9, "position changed: {a} != {b}");
        }
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
#[test]
fn diagonal_inputs_are_normalized_and_jump_rejects_controls() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, _) = create(&mut s);
    assert!(
        !s.handle_at(
            &mut p,
            Request::PilotInput {
                translation: [0.0; 3],
                rotation: [0.0, f64::NAN, 0.0]
            },
            0
        )
        .ok
    );
    input(&mut s, &mut p, [1.0, 1.0, 1.0], [1.0, 1.0, 1.0], 0);
    let f = state(&mut s, 100);
    assert!(f.velocity.iter().map(|v| v * v).sum::<f64>().sqrt() <= SHIP_ACCELERATION * 0.100001);
    close(
        f.angular_velocity.iter().map(|v| v * v).sum::<f64>().sqrt(),
        MAX_ANGULAR_SPEED,
    );
    let warp = run(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Gate { id: 101 },
        },
        100,
    );
    let arrival = warp.pilots[0]
        .flight
        .as_ref()
        .unwrap()
        .warp
        .as_ref()
        .unwrap()
        .arrives_at_ms;
    run(&mut s, &mut p, Request::Jump { gate_id: 101 }, arrival);
    assert!(
        !s.handle_at(
            &mut p,
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3]
            },
            arrival + 1
        )
        .ok
    );
}
