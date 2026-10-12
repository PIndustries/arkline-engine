use arkline_protocol::*;
use arkline_server::Server;

fn avatar(server: &mut Server, now: u64) -> StationAvatar {
    server
        .handle_at(&mut None, Request::Snapshot, now)
        .snapshot
        .unwrap()
        .pilots[0]
        .station_avatar
        .clone()
        .unwrap()
}

#[test]
fn jump_is_authoritative_bounded_and_cleared_on_disconnect_and_restart() {
    let path = std::env::temp_dir().join(format!("arkline-jump-{}.sqlite", rand::random::<u64>()));
    let mut server = Server::open(&path).unwrap();
    let mut pilot = None;
    assert!(!server.handle_at(&mut pilot, Request::StationJump, 0).ok);
    let connected = server.handle_at(
        &mut pilot,
        Request::Connect {
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Jumper".into(),
            resume_token: None,
        },
        0,
    );
    assert!(connected.ok);
    assert!(!server.handle_at(&mut pilot, Request::StationJump, 0).ok);
    assert!(server.handle_at(&mut pilot, Request::DisembarkShip, 0).ok);
    assert!(server.handle_at(&mut pilot, Request::StationJump, 100).ok);
    assert!(!server.handle_at(&mut pilot, Request::StationJump, 200).ok);
    assert!(!server.handle_at(&mut pilot, Request::EnterShip, 200).ok);
    assert!(
        !server
            .handle_at(&mut pilot, Request::StationSit { seated: false }, 200)
            .ok
    );
    let high = avatar(&mut server, 550);
    assert!(high.position[1] > 1.0 && high.position[1] < 1.04);
    assert_eq!(high.jump_started_ms, Some(100));
    let landed = avatar(&mut server, 1100);
    assert_eq!(landed.position[1], 0.0);
    assert_eq!(landed.jump_started_ms, None);
    assert!(server.handle_at(&mut pilot, Request::StationJump, 1200).ok);
    server.disconnect(&mut pilot);
    assert_eq!(avatar(&mut server, 1300).position[1], 0.0);
    assert!(
        server
            .handle_at(
                &mut pilot,
                Request::Connect {
                    supports_room_controls: false,
                    supports_fleet_hangar: false,
                    supports_pioneer_interior: true,
                    protocol_version: PROTOCOL_VERSION,
                    pilot: "Jumper".into(),
                    resume_token: connected.resume_token,
                },
                1400
            )
            .ok
    );
    assert!(server.handle_at(&mut pilot, Request::StationJump, 1500).ok);
    assert!(avatar(&mut server, 1800).position[1] > 0.0);
    drop(server);
    let mut reopened = Server::open(&path).unwrap();
    let recovered = avatar(&mut reopened, 1900);
    assert_eq!(recovered.position[1], 0.0);
    assert_eq!(recovered.jump_started_ms, None);
    drop(reopened);
    for suffix in ["", ".lock", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn legacy_avatar_defaults_to_grounded() {
    let avatar: StationAvatar = serde_json::from_value(serde_json::json!({
        "position": [0.0, 0.0, 0.0], "heading": 0.0, "moving": false, "updated_at_ms": 0
    }))
    .unwrap();
    assert_eq!(avatar.jump_started_ms, None);
}
