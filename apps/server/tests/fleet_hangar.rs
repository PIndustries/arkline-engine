use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r
}
fn connect(
    s: &mut Server,
    p: &mut Option<String>,
    name: &str,
    token: Option<String>,
    fleet: bool,
    t: u64,
) -> Response {
    ok(
        s,
        p,
        Request::Connect {
            world_layout_version: 0,
            supports_room_controls: false,
            supports_fleet_hangar: fleet,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: token,
        },
        t,
    )
}
#[test]
fn walk_board_restart_identity_and_ownership() {
    let path = std::env::temp_dir().join(format!("fleet-{}.sqlite", rand::random::<u64>()));
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    let r = connect(&mut s, &mut p, "Walker", None, true, 0);
    let token = r.resume_token;
    let h = r.assets.unwrap().hangar.unwrap();
    assert_eq!(h.berths.len(), 7);
    let before = ok(&mut s, &mut p, Request::ExportLedger, 0).ledger.unwrap();
    assert!(
        !s.handle_at(
            &mut p,
            Request::SelectShip {
                ship_class: ShipClass::Prospector
            },
            0
        )
        .ok
    );
    let after = ok(&mut s, &mut p, Request::ExportLedger, 0).ledger.unwrap();
    assert_eq!(
        serde_json::to_value(before).unwrap(),
        serde_json::to_value(after).unwrap()
    );
    let target = h
        .berths
        .iter()
        .find(|b| b.ship_class == ShipClass::Prospector)
        .unwrap()
        .clone();
    let mut outsider = None;
    connect(&mut s, &mut outsider, "Other", None, true, 0);
    ok(
        &mut s,
        &mut p,
        Request::CreateCorporation {
            name: "Crew".into(),
        },
        0,
    );
    ok(
        &mut s,
        &mut p,
        Request::InviteCorporation {
            pilot: "Other".into(),
        },
        0,
    );
    ok(
        &mut s,
        &mut outsider,
        Request::AcceptCorporation {
            corporation: "Crew".into(),
        },
        0,
    );
    assert!(!Request::BoardShip {
        id: target.id.clone()
    }
    .is_remote_gameplay());
    ok(&mut s, &mut outsider, Request::DisembarkShip, 0);
    assert!(
        !s.handle_at(
            &mut outsider,
            Request::BoardShip {
                id: target.id.clone()
            },
            0
        )
        .ok
    );
    ok(&mut s, &mut p, Request::DisembarkShip, 0);
    assert!(
        !s.handle_at(
            &mut p,
            Request::BoardShip {
                id: target.id.clone()
            },
            0
        )
        .ok
    );
    assert!(
        !s.handle_at(
            &mut p,
            Request::BoardShip {
                id: "forged".into()
            },
            0
        )
        .ok
    );
    let mut now = 0;
    while (now as f64 / 1000.0) * WALK_SPEED_MPS < target.boarding_point[0] - 1.0 {
        ok(
            &mut s,
            &mut p,
            Request::StationWalk {
                forward: 1.0,
                strafe: 0.0,
                turn: 0.0,
            },
            now,
        );
        now += 250;
        s.tick_at(now).unwrap();
    }
    let boarded = ok(
        &mut s,
        &mut p,
        Request::BoardShip {
            id: target.id.clone(),
        },
        now,
    );
    assert_eq!(
        boarded
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Walker")
            .unwrap()
            .ship_class,
        ShipClass::Prospector
    );
    assert!(boarded
        .assets
        .as_ref()
        .unwrap()
        .hangar
        .as_ref()
        .unwrap()
        .berths
        .iter()
        .any(|b| b.id == target.id && b.active));
    // A stale simultaneous second claim cannot replay the boarding operation.
    let mut concurrent = Some("Walker".into());
    assert!(
        !s.handle_at(
            &mut concurrent,
            Request::BoardShip {
                id: target.id.clone()
            },
            now
        )
        .ok
    );
    let export = ok(&mut s, &mut p, Request::ExportLedger, now)
        .ledger
        .unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    drop(s);
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    let restored = connect(&mut s, &mut p, "Walker", token, true, now + 1);
    let restored = restored.assets.unwrap().hangar.unwrap();
    for b in h.berths {
        assert!(restored
            .berths
            .iter()
            .any(|r| r.id == b.id && r.position == b.position));
    }
    ok(&mut s, &mut p, Request::DisembarkShip, now + 2);
    let r = ok(&mut s, &mut p, Request::Snapshot, now + 2);
    assert_eq!(
        r.snapshot
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Walker")
            .unwrap()
            .station_avatar
            .as_ref()
            .unwrap()
            .position,
        target.boarding_point
    );
}
#[test]
fn old_client_recovery_is_authenticated_and_latched_until_all_sessions_close() {
    let path = std::env::temp_dir().join(format!("fleet-{}.sqlite", rand::random::<u64>()));
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    let token = connect(&mut s, &mut p, "Mixed", None, true, 0).resume_token;
    ok(&mut s, &mut p, Request::DisembarkShip, 0);
    let mut old = None;
    let bad = s.handle_at(
        &mut old,
        Request::Connect {
            world_layout_version: 0,
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Mixed".into(),
            resume_token: Some("bad".into()),
        },
        0,
    );
    assert!(!bad.ok);
    assert!(ok(&mut s, &mut p, Request::Snapshot, 0)
        .assets
        .unwrap()
        .hangar
        .is_some());
    let r = connect(&mut s, &mut old, "Mixed", token.clone(), false, 0);
    assert!(r.assets.unwrap().hangar.is_none());
    assert_eq!(
        r.snapshot.unwrap().pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position,
        hangar_layout(ShipClass::Pioneer).boarding_point
    );
    assert!(connect(&mut s, &mut p, "Mixed", token.clone(), true, 0)
        .assets
        .unwrap()
        .hangar
        .is_none());
    s.disconnect(&mut old);
    assert!(connect(&mut s, &mut p, "Mixed", token.clone(), true, 0)
        .assets
        .unwrap()
        .hangar
        .is_none());
    s.disconnect(&mut p);
    assert!(connect(&mut s, &mut p, "Mixed", token, true, 0)
        .assets
        .unwrap()
        .hangar
        .is_some());
}

#[test]
fn legacy_private_hangar_keeps_class_selection() {
    let path = std::env::temp_dir().join(format!("fleet-legacy-{}.sqlite", rand::random::<u64>()));
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    connect(&mut s, &mut p, "Legacy", None, false, 0);
    let r = ok(
        &mut s,
        &mut p,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        0,
    );
    assert_eq!(
        r.snapshot.unwrap().pilots[0].ship_class,
        ShipClass::Prospector
    );
    assert!(r.assets.unwrap().hangar.is_none());
}
