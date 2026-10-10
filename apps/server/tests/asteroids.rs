use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
#[test]
fn explicit_asteroid_lock_depletion_and_legacy_migration_conserve_ore() {
    let path = std::env::temp_dir().join(format!(
        "arkline-asteroids-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    ok(
        &mut s,
        &mut p,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: "Miner".into(),
            resume_token: None,
        },
        0,
    );
    ok(&mut s, &mut p, Request::Undock, 0);
    ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    );
    let initial = ok(&mut s, &mut p, Request::Snapshot, WARP_DURATION_MS);
    assert_eq!(
        initial.asteroids.iter().map(|a| a.ore).sum::<u64>(),
        INITIAL_BELT_ORE
    );
    let asteroid = initial
        .asteroids
        .iter()
        .find(|a| a.system_id == HOME_SYSTEM_ID)
        .unwrap()
        .clone();
    assert!(
        !s.handle_at(
            &mut p,
            Request::MineAsteroid {
                id: asteroid.id,
                units: 1
            },
            WARP_DURATION_MS
        )
        .ok
    );
    assert!(
        !s.handle_at(&mut p, Request::LockAsteroid { id: 73 }, WARP_DURATION_MS)
            .ok
    );
    ok(
        &mut s,
        &mut p,
        Request::LockAsteroid { id: asteroid.id },
        WARP_DURATION_MS,
    );
    let mut remaining = asteroid.ore;
    let mut time = WARP_DURATION_MS;
    while remaining > 0 {
        let units = remaining.min(MAX_MINE_UNITS);
        ok(
            &mut s,
            &mut p,
            Request::MineAsteroid {
                id: asteroid.id,
                units,
            },
            time,
        );
        remaining -= units;
        time += 1000;
    }
    let depleted = ok(&mut s, &mut p, Request::Snapshot, time);
    assert_eq!(
        depleted
            .asteroids
            .iter()
            .find(|a| a.id == asteroid.id)
            .unwrap()
            .ore,
        0
    );
    assert_eq!(depleted.pilots[0].ore, asteroid.ore);
    assert!(
        !s.handle_at(
            &mut p,
            Request::MineAsteroid {
                id: asteroid.id,
                units: 1
            },
            time
        )
        .ok
    );
    assert_eq!(depleted.belt_ore, INITIAL_BELT_ORE - asteroid.ore);
    let export = s
        .handle_at(&mut p, Request::ExportLedger, time)
        .ledger
        .unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    drop(s);
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let json: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value.as_object_mut().unwrap().remove("asteroids");
        value["pilots"]["Miner"]["view"]
            .as_object_mut()
            .unwrap()
            .remove("system_id");
        value["pilots"]["Miner"]["view"]
            .as_object_mut()
            .unwrap()
            .remove("locked_asteroid");
        db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&value).unwrap()],
        )
        .unwrap();
    }
    let mut s = Server::open(&path).unwrap();
    let restored = ok(&mut s, &mut None, Request::Snapshot, time);
    assert_eq!(
        restored.asteroids.iter().map(|a| a.ore).sum::<u64>(),
        depleted.belt_ore
    );
    assert_eq!(restored.pilots[0].ore, depleted.pilots[0].ore);
    assert_eq!(restored.ledger_hash, depleted.ledger_hash);
    assert_eq!(restored.pilots[0].system_id, HOME_SYSTEM_ID);
    drop(s);
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
