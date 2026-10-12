use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let response = s.handle_at(p, r, t);
    assert!(response.ok, "{}", response.message);
    response
}
fn avatar(s: &mut Server, t: u64) -> StationAvatar {
    ok(s, &mut None, Request::Snapshot, t)
        .snapshot
        .unwrap()
        .pilots[0]
        .station_avatar
        .clone()
        .unwrap()
}
#[test]
fn walking_is_authoritative_bounded_and_stops_on_expiry_logout_and_restart() {
    let path = std::env::temp_dir().join(format!("arkline-walk-{}.sqlite", rand::random::<u64>()));
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    let token = ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Walker".into(),
            resume_token: None,
        },
        0,
    )
    .resume_token;
    assert!(!s.handle_at(&mut None, Request::DisembarkShip, 0).ok);
    ok(&mut s, &mut p, Request::DisembarkShip, 100);
    let initial = avatar(&mut s, 100);
    assert_eq!(
        initial.position,
        hangar_layout(ShipClass::Pioneer).boarding_point
    );
    for r in [
        Request::Undock,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        Request::StartResourceJob {
            target_units: 100,
            pilot: None,
        },
        Request::PilotInput {
            translation: [1., 0., 0.],
            rotation: [0.; 3],
        },
    ] {
        assert!(!s.handle_at(&mut p, r, 100).ok);
    }
    for bad in [f64::NAN, f64::INFINITY, 1.1] {
        assert!(
            !s.handle_at(
                &mut p,
                Request::StationWalk {
                    forward: bad,
                    strafe: 0.,
                    turn: 0.
                },
                100
            )
            .ok
        );
    }
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: 1.,
            strafe: 0.,
            turn: 0.,
        },
        100,
    );
    s.tick_at(600).unwrap();
    let expired = avatar(&mut s, 600);
    assert!((expired.position[0] - 2.).abs() < 1e-9 && !expired.moving);
    s.tick_at(1000).unwrap();
    assert_eq!(avatar(&mut s, 1000).position, expired.position);
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: 1.,
            strafe: 0.,
            turn: 0.,
        },
        1000,
    );
    s.tick_at(1500).unwrap();
    assert!(!s.handle_at(&mut p, Request::EnterShip, 1500).ok);
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: -1.,
            strafe: 0.,
            turn: 0.,
        },
        1500,
    );
    s.tick_at(2000).unwrap();
    ok(&mut s, &mut p, Request::EnterShip, 2000);
    ok(&mut s, &mut p, Request::Undock, 2000);
    assert!(!s.handle_at(&mut p, Request::DisembarkShip, 2000).ok);
    ok(&mut s, &mut p, Request::Dock, 2000);
    ok(&mut s, &mut p, Request::DisembarkShip, 2000);
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: 0.,
            strafe: -1.,
            turn: 0.,
        },
        2000,
    );
    s.tick_at(2500).unwrap();
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: 0.,
            strafe: -1.,
            turn: 0.,
        },
        2500,
    );
    s.tick_at(3000).unwrap();
    assert!(
        avatar(&mut s, 3000).position[2] >= ShipClass::Pioneer.spec().width_m / 2. + 0.3 - 1e-9
    );
    ok(
        &mut s,
        &mut p,
        Request::StationWalk {
            forward: 1.,
            strafe: 0.,
            turn: 1.,
        },
        3000,
    );
    s.disconnect(&mut p);
    let stopped = avatar(&mut s, 3100);
    assert!(!stopped.moving);
    drop(s);
    let mut s = Server::open(&path).unwrap();
    let a = avatar(&mut s, 3200);
    assert_eq!(a.position, stopped.position);
    assert!(!a.moving);
    ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Walker".into(),
            resume_token: token,
        },
        3200,
    );
    let ledger = ok(&mut s, &mut p, Request::ExportLedger, 3200)
        .ledger
        .unwrap();
    assert_eq!(ledger.entries.len(), 1);
    arkline_verifier::verify(&ledger, None).unwrap();
    drop(s);
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(format!("{}.lock", path.display()));
}

#[test]
fn walking_cannot_impersonate_worker_and_disembarking_releases_remote_control() {
    let mut s = Server::open(":memory:").unwrap();
    let mut owner = None;
    let mut worker = None;
    for (session, name) in [(&mut owner, "Owner"), (&mut worker, "Worker")] {
        ok(
            &mut s,
            session,
            Request::Connect {
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: name.into(),
                resume_token: None,
            },
            0,
        );
    }
    ok(
        &mut s,
        &mut worker,
        Request::SetFleetController {
            controller: Some("Owner".into()),
        },
        0,
    );
    ok(
        &mut s,
        &mut owner,
        Request::BeginRemoteControl {
            pilot: "Worker".into(),
        },
        0,
    );
    assert!(
        !s.handle_at(
            &mut owner,
            Request::RemoteControl {
                pilot: "Worker".into(),
                request: Box::new(Request::DisembarkShip)
            },
            0
        )
        .ok
    );
    let response = ok(&mut s, &mut owner, Request::DisembarkShip, 0);
    let snap = response.snapshot.unwrap();
    assert!(snap.remote_controls.is_empty());
    assert!(snap
        .pilots
        .iter()
        .find(|p| p.name == "Worker")
        .unwrap()
        .station_avatar
        .is_none());
    assert!(
        !s.handle_at(
            &mut owner,
            Request::BeginRemoteControl {
                pilot: "Worker".into()
            },
            0
        )
        .ok
    );
    ok(&mut s, &mut owner, Request::EnterShip, 0);
    ok(&mut s, &mut worker, Request::DisembarkShip, 0);
    assert!(
        !s.handle_at(
            &mut owner,
            Request::BeginRemoteControl {
                pilot: "Worker".into()
            },
            0
        )
        .ok
    );
    assert!(
        !s.handle_at(
            &mut owner,
            Request::StartResourceJob {
                pilot: Some("Worker".into()),
                target_units: 100
            },
            0
        )
        .ok
    );
}
