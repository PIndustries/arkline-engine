use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let response = s.handle_at(p, r, t);
    assert!(response.ok, "{}", response.message);
    response
}
#[test]
fn restart_clears_deployed_fleet_without_reusing_ids_or_changing_economy() {
    let path = std::env::temp_dir().join(format!(
        "arkline-fleet-restart-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let token;
    let last_id;
    let hash;
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = None;
        token = ok(
            &mut s,
            &mut p,
            Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Owner".into(),
                resume_token: None,
            },
            0,
        )
        .resume_token
        .unwrap();
        ok(&mut s, &mut p, Request::Undock, 0);
        for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
            for _ in 0..DRONE_CLASS_CAP {
                ok(&mut s, &mut p, Request::LaunchDroneClass { class }, 0);
            }
        }
        let snapshot = ok(&mut s, &mut p, Request::Snapshot, 0).snapshot.unwrap();
        last_id = snapshot.pilots[0]
            .drones
            .iter()
            .map(|d| d.id)
            .max()
            .unwrap();
        hash = snapshot.ledger_hash;
    }
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = None;
        let recovered = ok(
            &mut s,
            &mut p,
            Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Owner".into(),
                resume_token: Some(token),
            },
            100_000,
        )
        .snapshot
        .unwrap();
        assert!(recovered.pilots[0].drones.is_empty());
        assert!(recovered.pilots[0].drone.is_none());
        assert_eq!(recovered.ledger_hash, hash);
        assert_eq!(recovered.pilots[0].ore, 0);
        let launched = ok(
            &mut s,
            &mut p,
            Request::LaunchDroneClass {
                class: DroneClass::Mining,
            },
            100_001,
        )
        .snapshot
        .unwrap();
        assert!(launched.pilots[0].drones[0].id > last_id);
        s.disconnect(&mut p);
        assert!(ok(&mut s, &mut None, Request::Snapshot, 100_002)
            .snapshot
            .unwrap()
            .pilots[0]
            .drones
            .is_empty());
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
#[test]
fn legacy_singleton_schema_recovers_safely_once() {
    let path = std::env::temp_dir().join(format!(
        "arkline-fleet-legacy-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let hash;
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = None;
        ok(
            &mut s,
            &mut p,
            Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Legacy".into(),
                resume_token: None,
            },
            0,
        );
        ok(&mut s, &mut p, Request::Undock, 0);
        hash = ok(&mut s, &mut p, Request::LaunchDrone, 0)
            .snapshot
            .unwrap()
            .ledger_hash;
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let json: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old.as_object_mut().unwrap().remove("next_drone_id");
        let view = old["pilots"]["Legacy"]["view"].as_object_mut().unwrap();
        view.remove("drones");
        let drone = view["drone"].as_object_mut().unwrap();
        for field in [
            "id",
            "class",
            "hit_points",
            "signal_strength",
            "mining_cycle",
            "attack_ready_at_ms",
        ] {
            drone.remove(field);
        }
        db.execute("UPDATE world SET json=?1 WHERE id=1", [old.to_string()])
            .unwrap();
    }
    for _ in 0..3 {
        let mut s = Server::open(&path).unwrap();
        let snapshot = ok(&mut s, &mut None, Request::Snapshot, 100_000)
            .snapshot
            .unwrap();
        assert!(snapshot.pilots[0].drones.is_empty());
        assert!(snapshot.pilots[0].drone.is_none());
        assert_eq!(snapshot.ledger_hash, hash);
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
