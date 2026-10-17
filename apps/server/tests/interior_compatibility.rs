use arkline_protocol::*;
use arkline_server::Server;

fn connect(
    s: &mut Server,
    session: &mut Option<String>,
    token: Option<String>,
    supports: bool,
) -> Response {
    s.handle_at(
        session,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: supports,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Compatible".into(),
            resume_token: token,
        },
        1000,
    )
}
fn avatar(s: &mut Server, session: &mut Option<String>) -> StationAvatar {
    s.handle_at(session, Request::Snapshot, 1000)
        .snapshot
        .unwrap()
        .pilots[0]
        .station_avatar
        .clone()
        .unwrap()
}
#[test]
fn authenticated_legacy_recovery_and_concurrent_latch_preserve_ownership() {
    let mut s = Server::open(":memory:").unwrap();
    let mut visual = None;
    let created = connect(&mut s, &mut visual, None, true);
    let token = created.resume_token.unwrap();
    assert!(s.handle_at(&mut visual, Request::EnterInterior, 1000).ok);
    let before = s
        .handle_at(&mut visual, Request::ExportLedger, 1000)
        .ledger
        .unwrap();
    let mut failed = None;
    assert!(!connect(&mut s, &mut failed, Some("wrong".into()), false).ok);
    assert!(failed.is_none());
    assert!(avatar(&mut s, &mut visual).onboard_interior);
    // Correct new-client reconnect preserves the interior and does not latch.
    let mut second_visual = None;
    assert!(connect(&mut s, &mut second_visual, Some(token.clone()), true).ok);
    assert!(avatar(&mut s, &mut visual).onboard_interior);
    let mut old = None;
    // Deserialize the actual old wire form with no capability field.
    let request: Request = serde_json::from_value(serde_json::json!({
        "command":"connect", "protocol_version":PROTOCOL_VERSION,
        "pilot":"Compatible", "resume_token":token
    }))
    .unwrap();
    assert!(s.handle_at(&mut old, request, 1000).ok);
    let a = avatar(&mut s, &mut old);
    assert!(!a.onboard_interior && !a.moving && !a.seated);
    assert!(a.jump_started_ms.is_none());
    assert_eq!(a.position, hangar_layout(ShipClass::Pioneer).boarding_point);
    let after = s
        .handle_at(&mut old, Request::ExportLedger, 1000)
        .ledger
        .unwrap();
    assert_eq!(
        serde_json::to_value(before.entries).unwrap(),
        serde_json::to_value(after.entries).unwrap()
    );
    assert!(s.handle_at(&mut old, Request::EnterShip, 1000).ok);
    assert!(s.handle_at(&mut old, Request::Undock, 1000).ok);
    assert!(s.handle_at(&mut old, Request::Dock, 1000).ok);
    assert!(!s.handle_at(&mut visual, Request::EnterInterior, 1000).ok);
    assert!(!Request::EnterInterior.is_remote_gameplay());
    assert!(
        !s.handle_at(
            &mut visual,
            Request::RemoteControl {
                pilot: "Compatible".into(),
                request: Box::new(Request::EnterInterior),
            },
            1000
        )
        .ok
    );
    assert!(s
        .handle_at(&mut visual, Request::Snapshot, 1000)
        .snapshot
        .unwrap()
        .pilots[0]
        .station_avatar
        .is_none());
    s.disconnect(&mut old);
    assert!(!s.handle_at(&mut visual, Request::EnterInterior, 1000).ok);
    // A capable re-Connect must not clear another connection's protection.
    assert!(connect(&mut s, &mut visual, Some(token.clone()), true).ok);
    assert!(!s.handle_at(&mut visual, Request::EnterInterior, 1000).ok);
    s.disconnect(&mut second_visual);
    assert!(!s.handle_at(&mut visual, Request::EnterInterior, 1000).ok);
    s.disconnect(&mut visual);
    assert!(connect(&mut s, &mut visual, Some(token), true).ok);
    assert!(s.handle_at(&mut visual, Request::EnterInterior, 1000).ok);
}

#[test]
fn rejected_legacy_authentication_never_installs_latch() {
    let mut s = Server::open(":memory:").unwrap();
    let mut visual = None;
    let token = connect(&mut s, &mut visual, None, true)
        .resume_token
        .unwrap();
    let mut rejected = None;
    assert!(!connect(&mut s, &mut rejected, Some("wrong".into()), false).ok);
    assert!(s.handle_at(&mut visual, Request::EnterInterior, 1000).ok);
    assert!(connect(&mut s, &mut visual, Some(token), true).ok);
    assert!(avatar(&mut s, &mut visual).onboard_interior);
}

#[test]
fn persisted_interior_recovers_for_old_client_after_restart() {
    let path =
        std::env::temp_dir().join(format!("arkline-compat-{}.sqlite", rand::random::<u64>()));
    let mut s = Server::open(&path).unwrap();
    let mut session = None;
    let token = connect(&mut s, &mut session, None, true)
        .resume_token
        .unwrap();
    assert!(s.handle_at(&mut session, Request::EnterInterior, 1000).ok);
    drop(s);
    let mut s = Server::open(&path).unwrap();
    let mut resumed = None;
    assert!(connect(&mut s, &mut resumed, Some(token), false).ok);
    let a = avatar(&mut s, &mut resumed);
    assert!(!a.onboard_interior);
    assert_eq!(a.position, hangar_layout(ShipClass::Pioneer).boarding_point);
    assert!(s.handle_at(&mut resumed, Request::EnterShip, 1000).ok);
    assert!(s.handle_at(&mut resumed, Request::Undock, 1000).ok);
    drop(s);
    for suffix in ["", "-wal", "-shm", ".lock"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}
