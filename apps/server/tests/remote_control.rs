use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r
}
fn login(s: &mut Server, name: &str) -> (Option<String>, String) {
    let mut p = None;
    let r = ok(
        s,
        &mut p,
        Request::Connect {
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: None,
        },
        0,
    );
    (p, r.resume_token.unwrap())
}
fn wrap(pilot: &str, r: Request) -> Request {
    Request::RemoteControl {
        pilot: pilot.into(),
        request: Box::new(r),
    }
}
fn snap(s: &mut Server, t: u64) -> Snapshot {
    ok(s, &mut None, Request::Snapshot, t).snapshot.unwrap()
}
fn grant(s: &mut Server, p: &mut Option<String>) {
    ok(
        s,
        p,
        Request::SetFleetController {
            controller: Some("Boss".into()),
        },
        0,
    );
}
#[test]
fn remote_gameplay_preserves_identity_private_assets_and_rejects_unsafe_wrappers() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut boss, _) = login(&mut s, "Boss");
    let (mut worker, _) = login(&mut s, "Worker");
    assert!(
        !s.handle_at(
            &mut boss,
            Request::BeginRemoteControl {
                pilot: "Worker".into()
            },
            0
        )
        .ok
    );
    grant(&mut s, &mut worker);
    ok(
        &mut s,
        &mut worker,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        0,
    );
    ok(
        &mut s,
        &mut worker,
        Request::StartResourceJob {
            target_units: 100,
            pilot: None,
        },
        0,
    );
    ok(
        &mut s,
        &mut boss,
        Request::BeginRemoteControl {
            pilot: "Worker".into(),
        },
        0,
    );
    let state = snap(&mut s, 0);
    assert_eq!(state.resource_jobs[0].phase, ResourceJobPhase::Cancelled);
    assert_eq!(state.remote_controls[0].pilot, "Worker");
    for forbidden in [
        Request::DepositOre { units: 1 },
        Request::SelectShip {
            ship_class: ShipClass::Vanguard,
        },
        Request::ExportLedger,
        Request::StartResourceJob {
            target_units: 1,
            pilot: None,
        },
        Request::SetFleetController { controller: None },
        wrap("Worker", Request::Undock),
        Request::Connect {
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Intruder".into(),
            resume_token: None,
        },
    ] {
        assert!(!s.handle_at(&mut boss, wrap("Worker", forbidden), 0).ok);
        assert_eq!(boss.as_deref(), Some("Boss"));
        assert_eq!(snap(&mut s, 0).remote_controls.len(), 1);
    }
    assert!(
        !s.handle_at(
            &mut boss,
            Request::StartResourceJob {
                target_units: 100,
                pilot: Some("Worker".into())
            },
            0
        )
        .ok
    );
    let r = ok(&mut s, &mut boss, wrap("Worker", Request::Undock), 0);
    let own_assets = r.assets.unwrap();
    assert!(own_assets
        .ships
        .iter()
        .all(|h| h.ship_class != ShipClass::Pioneer));
    assert!(own_assets
        .ships
        .iter()
        .any(|h| h.ship_class == ShipClass::Prospector));
    ok(
        &mut s,
        &mut boss,
        wrap(
            "Worker",
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 1.0],
            },
        ),
        0,
    );
    let moved = snap(&mut s, 400);
    assert_eq!(
        moved
            .pilots
            .iter()
            .find(|p| p.name == "Boss")
            .unwrap()
            .location,
        Location::Docked
    );
    let worker = moved.pilots.iter().find(|p| p.name == "Worker").unwrap();
    assert!(worker
        .flight
        .as_ref()
        .unwrap()
        .velocity
        .iter()
        .any(|v| v.abs() > 1.0));
    ok(&mut s, &mut boss, Request::EndRemoteControl, 400);
    let ended = snap(&mut s, 400);
    assert!(ended.remote_controls.is_empty());
    let f = ended
        .pilots
        .iter()
        .find(|p| p.name == "Worker")
        .unwrap()
        .flight
        .as_ref()
        .unwrap();
    assert_eq!(f.mode, NavigationMode::Stop);
    assert_eq!(f.angular_velocity, [0.0; 3]);
    assert!(
        !s.handle_at(
            &mut boss,
            wrap(
                "Worker",
                Request::PilotInput {
                    translation: [1.0; 3],
                    rotation: [0.0; 3]
                }
            ),
            500
        )
        .ok
    );
}
#[test]
fn switching_rejects_stale_target_input_and_revocation_and_reconnect_release() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut boss, _) = login(&mut s, "Boss");
    let (mut a, token) = login(&mut s, "Alpha");
    let (mut b, _) = login(&mut s, "Beta");
    grant(&mut s, &mut a);
    grant(&mut s, &mut b);
    ok(
        &mut s,
        &mut boss,
        Request::BeginRemoteControl {
            pilot: "Alpha".into(),
        },
        0,
    );
    ok(&mut s, &mut boss, wrap("Alpha", Request::Undock), 0);
    ok(
        &mut s,
        &mut boss,
        wrap(
            "Alpha",
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3],
            },
        ),
        0,
    );
    ok(
        &mut s,
        &mut boss,
        Request::BeginRemoteControl {
            pilot: "Beta".into(),
        },
        100,
    );
    let state = snap(&mut s, 100);
    assert_eq!(state.remote_controls.len(), 1);
    assert_eq!(state.remote_controls[0].pilot, "Beta");
    assert_eq!(
        state
            .pilots
            .iter()
            .find(|p| p.name == "Alpha")
            .unwrap()
            .flight
            .as_ref()
            .unwrap()
            .mode,
        NavigationMode::Stop
    );
    assert!(
        !s.handle_at(&mut boss, wrap("Alpha", Request::Undock), 100)
            .ok
    );
    assert!(
        !s.handle_at(
            &mut boss,
            Request::BeginRemoteControl {
                pilot: "Missing".into()
            },
            100
        )
        .ok
    );
    assert_eq!(snap(&mut s, 100).remote_controls[0].pilot, "Beta");
    ok(
        &mut s,
        &mut b,
        Request::SetFleetController { controller: None },
        100,
    );
    assert!(snap(&mut s, 100).remote_controls.is_empty());
    ok(
        &mut s,
        &mut boss,
        Request::BeginRemoteControl {
            pilot: "Alpha".into(),
        },
        100,
    );
    let mut reconnected = None;
    ok(
        &mut s,
        &mut reconnected,
        Request::Connect {
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Alpha".into(),
            resume_token: Some(token),
        },
        100,
    );
    assert!(snap(&mut s, 100).remote_controls.is_empty());
    ok(
        &mut s,
        &mut boss,
        Request::BeginRemoteControl {
            pilot: "Alpha".into(),
        },
        100,
    );
    ok(
        &mut s,
        &mut a,
        Request::PilotInput {
            translation: [0.0; 3],
            rotation: [0.0; 3],
        },
        100,
    );
    assert!(snap(&mut s, 100).remote_controls.is_empty());
}
#[test]
fn worker_logout_retains_remote_lease_until_last_controller_socket_closes() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut boss, token) = login(&mut s, "Boss");
    let (mut worker, _) = login(&mut s, "Worker");
    grant(&mut s, &mut worker);
    let mut second = None;
    ok(
        &mut s,
        &mut second,
        Request::Connect {
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Boss".into(),
            resume_token: Some(token),
        },
        0,
    );
    ok(
        &mut s,
        &mut boss,
        Request::BeginRemoteControl {
            pilot: "Worker".into(),
        },
        0,
    );
    s.disconnect(&mut worker);
    assert_eq!(snap(&mut s, 0).remote_controls.len(), 1);
    ok(&mut s, &mut boss, wrap("Worker", Request::Undock), 0);
    ok(
        &mut s,
        &mut boss,
        wrap(
            "Worker",
            Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3],
            },
        ),
        0,
    );
    s.disconnect(&mut boss);
    assert_eq!(snap(&mut s, 0).remote_controls.len(), 1);
    s.disconnect(&mut second);
    let state = snap(&mut s, 0);
    assert!(state.remote_controls.is_empty());
    assert_eq!(
        state
            .pilots
            .iter()
            .find(|p| p.name == "Worker")
            .unwrap()
            .flight
            .as_ref()
            .unwrap()
            .mode,
        NavigationMode::Stop
    );
}
#[test]
fn restart_clears_lease_and_live_remote_input_but_keeps_permission_and_ship() {
    let path =
        std::env::temp_dir().join(format!("arkline-remote-{}.sqlite", rand::random::<u64>()));
    {
        let mut s = Server::open(&path).unwrap();
        let (mut boss, _) = login(&mut s, "Boss");
        let (mut worker, _) = login(&mut s, "Worker");
        grant(&mut s, &mut worker);
        ok(
            &mut s,
            &mut boss,
            Request::BeginRemoteControl {
                pilot: "Worker".into(),
            },
            0,
        );
        ok(&mut s, &mut boss, wrap("Worker", Request::Undock), 0);
        ok(
            &mut s,
            &mut boss,
            wrap(
                "Worker",
                Request::PilotInput {
                    translation: [1.0, 0.0, 0.0],
                    rotation: [1.0, 0.0, 0.0],
                },
            ),
            0,
        );
    }
    {
        let mut s = Server::open(&path).unwrap();
        let state = snap(&mut s, 100);
        assert!(state.remote_controls.is_empty());
        assert_eq!(state.fleet_links.len(), 1);
        let f = state
            .pilots
            .iter()
            .find(|p| p.name == "Worker")
            .unwrap()
            .flight
            .as_ref()
            .unwrap();
        assert_eq!(f.mode, NavigationMode::Stop);
        assert_eq!(f.velocity, [0.0; 3]);
        assert_eq!(f.angular_velocity, [0.0; 3]);
    }
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.lock", path.display()));
}
