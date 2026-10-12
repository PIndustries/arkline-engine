use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let response = s.handle_at(p, r, t);
    assert!(response.ok, "{}", response.message);
    response.snapshot.unwrap()
}
#[test]
fn legacy_lock_migrates_once_completed_locks_persist_pending_and_drone_cancel() {
    let path = std::env::temp_dir().join(format!(
        "arkline-lock-migration-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let completed;
    let hash;
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = None;
        ok(
            &mut s,
            &mut p,
            Request::Connect {
                supports_room_controls: false,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "LegacyLocks".into(),
                resume_token: None,
            },
            0,
        );
        assert!(!s.handle_at(&mut p, Request::LaunchDrone, 0).ok);
        ok(&mut s, &mut p, Request::Undock, 0);
        let warp = ok(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Belt,
            },
            0,
        );
        let Location::InTransit { arrives_at_ms, .. } = warp.pilots[0].location else {
            panic!()
        };
        let arrived = ok(&mut s, &mut p, Request::Snapshot, arrives_at_ms);
        let targets: Vec<_> = arrived
            .asteroids
            .iter()
            .filter(|a| a.system_id == HOME_SYSTEM_ID)
            .take(2)
            .collect();
        completed = targets[0].id;
        let pending = ok(
            &mut s,
            &mut p,
            Request::LockAsteroid { id: completed },
            arrives_at_ms,
        );
        let ready = pending.pilots[0].target_locks[0].completes_at_ms;
        ok(&mut s, &mut p, Request::Snapshot, ready);
        ok(
            &mut s,
            &mut p,
            Request::LockAsteroid { id: targets[1].id },
            ready,
        );
        let launched = ok(&mut s, &mut p, Request::LaunchDrone, ready);
        hash = launched.ledger_hash;
        assert!(launched.pilots[0].drone.is_some());
        assert_eq!(launched.pilots[0].locked_asteroids, vec![completed]);
    }
    // Emulate the old persisted schema: only the two legacy scalar mirrors exist.
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let json: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        let view = old["pilots"]["LegacyLocks"]["view"]
            .as_object_mut()
            .unwrap();
        view.remove("target_locks");
        view.remove("locked_asteroids");
        db.execute("UPDATE world SET json=?1 WHERE id=1", [old.to_string()])
            .unwrap();
    }
    for _ in 0..3 {
        let mut s = Server::open(&path).unwrap();
        let snapshot = ok(&mut s, &mut None, Request::Snapshot, 100_000);
        let p = &snapshot.pilots[0];
        assert_eq!(p.locked_asteroids, vec![completed]);
        assert_eq!(p.locked_asteroid, Some(completed));
        assert!(p.target_locks.is_empty());
        assert!(p.target_lock.is_none());
        assert!(p.drone.is_none());
        assert_eq!(snapshot.ledger_hash, hash);
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
#[test]
fn drone_last_logout_and_transition_cleanup_preserve_economy() {
    let mut s = Server::open(":memory:").unwrap();
    let mut p = None;
    let initial = ok(
        &mut s,
        &mut p,
        Request::Connect {
            supports_room_controls: false,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "DronePilot".into(),
            resume_token: None,
        },
        0,
    );
    ok(&mut s, &mut p, Request::Undock, 0);
    ok(&mut s, &mut p, Request::LaunchDrone, 0);
    s.disconnect(&mut p);
    let offline = ok(&mut s, &mut None, Request::Snapshot, 100);
    assert!(offline.pilots[0].drone.is_none());
    assert_eq!(offline.ledger_hash, initial.ledger_hash);
    // Authenticated test session drives subsequent normal-space lifecycle checks.
    p = Some("DronePilot".into());
    ok(&mut s, &mut p, Request::LaunchDrone, 101);
    let docked = ok(&mut s, &mut p, Request::Dock, 102);
    assert!(docked.pilots[0].drone.is_none());
    ok(&mut s, &mut p, Request::Undock, 103);
    ok(&mut s, &mut p, Request::LaunchDrone, 103);
    let warp = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        104,
    );
    assert!(warp.pilots[0].drone.is_none());
    assert_eq!(warp.ledger_hash, initial.ledger_hash);
}
