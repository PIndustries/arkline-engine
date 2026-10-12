use arkline_protocol::*;
use arkline_server::Server;
fn state(s: &mut Server, session: &mut Option<String>, now: u64) -> PilotView {
    s.handle_at(session, Request::Snapshot, now)
        .snapshot
        .unwrap()
        .pilots[0]
        .clone()
}
fn walk(s: &mut Server, session: &mut Option<String>, clock: &mut u64, target: [f64; 2]) {
    for _ in 0..300 {
        let a = state(s, session, *clock).station_avatar.unwrap();
        let dx = target[0] - a.position[0];
        let dz = target[1] - a.position[2];
        if dx.hypot(dz) < 0.06 {
            return;
        }
        let scale = (dx.hypot(dz) / 0.4).min(1.0);
        let norm = dx.hypot(dz);
        let r = s.handle_at(
            session,
            Request::StationWalk {
                forward: dz / norm * scale,
                strafe: -dx / norm * scale,
                turn: 0.0,
            },
            *clock,
        );
        assert!(r.ok, "{}", r.message);
        *clock += 100;
    }
    panic!("did not reach {target:?}");
}
#[test]
fn interior_rooms_doors_restart_and_boarding_lifecycle() {
    let path =
        std::env::temp_dir().join(format!("pioneer-interior-{}.sqlite", rand::random::<u64>()));
    let mut s = Server::open(&path).unwrap();
    let mut session = None;
    let mut now = 1000;
    let c = s.handle_at(
        &mut session,
        Request::Connect {
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Walker".into(),
            resume_token: None,
        },
        now,
    );
    assert!(c.assets.unwrap().pioneer_interior_available);
    let token = c.resume_token;
    assert!(s.handle_at(&mut session, Request::EnterInterior, now).ok);
    assert!(!s.handle_at(&mut session, Request::Undock, now).ok);
    assert!(!s.handle_at(&mut session, Request::EnterShip, now).ok);
    assert!(
        !s.handle_at(
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Hauler
            },
            now
        )
        .ok
    );
    walk(
        &mut s,
        &mut session,
        &mut now,
        [PIONEER_INTERIOR_ENTRY[0], 7.0],
    );
    for (_, x, z) in PIONEER_ROOMS {
        let door = x + 8.9333333333 / 2.0;
        walk(&mut s, &mut session, &mut now, [door, 7.0]);
        walk(&mut s, &mut session, &mut now, [door, z + 2.7]);
        walk(&mut s, &mut session, &mut now, [door, 7.0]);
    }
    assert!(
        !s.handle_at(&mut session, Request::InteriorLift { deck: 2 }, now)
            .ok
    );
    walk(&mut s, &mut session, &mut now, [2.0, 7.0]);
    assert!(
        !s.handle_at(&mut session, Request::InteriorLift { deck: 3 }, now)
            .ok
    );
    assert!(
        !s.handle_at(&mut session, Request::InteriorLift { deck: 1 }, now)
            .ok
    );
    assert!(s.handle_at(&mut session, Request::StationJump, now).ok);
    assert!(
        !s.handle_at(&mut session, Request::InteriorLift { deck: 2 }, now)
            .ok
    );
    now += 1000;
    assert!(
        s.handle_at(&mut session, Request::InteriorLift { deck: 2 }, now)
            .ok
    );
    assert_eq!(
        state(&mut s, &mut session, now)
            .station_avatar
            .unwrap()
            .interior_deck,
        2
    );
    for (_, x, z) in PIONEER_DECK_TWO_ROOMS {
        let door = x + 8.9333333333 / 2.0;
        walk(&mut s, &mut session, &mut now, [door, 7.0]);
        walk(&mut s, &mut session, &mut now, [door, z + 2.7]);
        walk(&mut s, &mut session, &mut now, [door, 7.0]);
    }
    // Matching XY airlock coordinates on deck two are not an exterior hatch.
    walk(
        &mut s,
        &mut session,
        &mut now,
        [PIONEER_INTERIOR_ENTRY[0], 7.0],
    );
    walk(
        &mut s,
        &mut session,
        &mut now,
        [PIONEER_INTERIOR_ENTRY[0], 3.0],
    );
    assert!(!s.handle_at(&mut session, Request::ExitInterior, now).ok);
    walk(
        &mut s,
        &mut session,
        &mut now,
        [PIONEER_INTERIOR_ENTRY[0], 7.0],
    );
    assert!(!s.handle_at(&mut session, Request::ExitInterior, now).ok);
    // A bulkhead blocks walking into a room away from its doorway.
    walk(&mut s, &mut session, &mut now, [25.0, 7.0]);
    for _ in 0..20 {
        assert!(
            s.handle_at(
                &mut session,
                Request::StationWalk {
                    forward: -1.0,
                    strafe: 0.0,
                    turn: 0.0
                },
                now
            )
            .ok
        );
        now += 100;
    }
    assert!(
        state(&mut s, &mut session, now)
            .station_avatar
            .unwrap()
            .position[2]
            >= 5.95
    );
    walk(&mut s, &mut session, &mut now, [25.0, 7.0]);
    assert!(s.handle_at(&mut session, Request::StationJump, now).ok);
    now += 400;
    let before = state(&mut s, &mut session, now).station_avatar.unwrap();
    assert!(before.position[1] > 0.0);
    drop(s);
    let mut s = Server::open(&path).unwrap();
    let mut session = None;
    now += 1000;
    assert!(
        s.handle_at(
            &mut session,
            Request::Connect {
                supports_room_controls: false,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Walker".into(),
                resume_token: token
            },
            now
        )
        .ok
    );
    let a = state(&mut s, &mut session, now).station_avatar.unwrap();
    assert!(a.onboard_interior);
    assert_eq!(a.interior_deck, 2);
    assert_eq!(a.position[1], 0.0);
    assert!(!a.moving);
    walk(&mut s, &mut session, &mut now, [2.0, 7.0]);
    assert!(
        s.handle_at(&mut session, Request::InteriorLift { deck: 1 }, now)
            .ok
    );
    walk(
        &mut s,
        &mut session,
        &mut now,
        [PIONEER_INTERIOR_ENTRY[0], 7.0],
    );
    walk(
        &mut s,
        &mut session,
        &mut now,
        [PIONEER_INTERIOR_ENTRY[0], 3.0],
    );
    assert!(s.handle_at(&mut session, Request::ExitInterior, now).ok);
    assert!(
        !state(&mut s, &mut session, now)
            .station_avatar
            .unwrap()
            .onboard_interior
    );
    assert!(s.handle_at(&mut session, Request::EnterShip, now).ok);
    assert!(
        s.handle_at(
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Hauler
            },
            now
        )
        .ok
    );
    assert!(!s.handle_at(&mut session, Request::EnterInterior, now).ok);
    assert!(s.handle_at(&mut session, Request::Undock, now).ok);
    assert!(!s.handle_at(&mut session, Request::EnterInterior, now).ok);
    drop(s);
    let _ = std::fs::remove_file(path);
}
#[test]
fn old_avatar_and_assets_never_claim_interior_support() {
    let a: StationAvatar = serde_json::from_value(
        serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":0}),
    )
    .unwrap();
    assert!(!a.onboard_interior);
    assert_eq!(a.interior_deck, 1);
    let a: AssetsView = serde_json::from_value(
        serde_json::json!({"ships":[],"storage":[],"corporation":null,"invitations":[]}),
    )
    .unwrap();
    assert!(!a.pioneer_interior_available);
    assert_eq!(a.pioneer_interior_decks, 0);
    let a:StationAvatar=serde_json::from_value(serde_json::json!({"onboard_interior":true,"position":[8.6666666667,0,3],"heading":0,"moving":false,"updated_at_ms":0})).unwrap();
    assert_eq!(a.interior_deck, 1);
}
