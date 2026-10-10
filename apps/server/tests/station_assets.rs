mod common;
// Real command acceptance with fake clock; no direct mutation of gameplay state.
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
    t: u64,
) -> Response {
    ok(
        s,
        p,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: token,
        },
        t,
    )
}
fn travel(s: &mut Server, p: &mut Option<String>, to: Destination, t: &mut u64) -> Snapshot {
    let response = ok(s, p, Request::Warp { destination: to }, *t);
    let own = response
        .snapshot
        .unwrap()
        .pilots
        .into_iter()
        .find(|v| Some(&v.name) == p.as_ref())
        .unwrap();
    let Location::InTransit { arrives_at_ms, .. } = own.location else {
        panic!("not in transit")
    };
    *t = arrives_at_ms + 1;
    ok(s, p, Request::Snapshot, *t).snapshot.unwrap()
}
fn audit(s: &mut Server, t: u64) -> LedgerExport {
    let response = ok(s, &mut None, Request::ExportLedger, t);
    assert!(response.assets.is_none());
    let export = response.ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    export
}
fn deny_unchanged(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) {
    let before = audit(s, t);
    let response = s.handle_at(p, r, t);
    assert!(!response.ok, "command unexpectedly succeeded");
    let after = audit(s, t);
    assert_eq!(before.snapshot.ledger_hash, after.snapshot.ledger_hash);
    assert_eq!(before.snapshot.stored_ore, after.snapshot.stored_ore);
}

#[test]
fn boarding_storage_corporation_authorization_and_restart_conserve_all_ore() {
    let dir = std::env::temp_dir().join(format!(
        "arkline-assets-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("world.sqlite");
    let mut s = Server::open(&path).unwrap();
    let (mut alice, mut bob, mut eve) = (None, None, None);
    let mut t = 1000;
    let token = connect(&mut s, &mut alice, "Alice", None, t)
        .resume_token
        .unwrap();
    connect(&mut s, &mut bob, "Bob", None, t);
    connect(&mut s, &mut eve, "Eve", None, t);
    ok(&mut s, &mut alice, Request::Undock, t);
    travel(&mut s, &mut alice, Destination::Belt, &mut t);
    let snapshot = ok(&mut s, &mut alice, Request::Snapshot, t)
        .snapshot
        .unwrap();
    let own = snapshot.pilots.iter().find(|p| p.name == "Alice").unwrap();
    let position = own.flight.as_ref().unwrap().position;
    let asteroid = snapshot
        .asteroids
        .iter()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .min_by(|a, b| {
            let d = |a: &Asteroid| {
                a.position
                    .iter()
                    .zip(position)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let lock = ok(
        &mut s,
        &mut alice,
        Request::LockAsteroid { id: asteroid.id },
        t,
    )
    .snapshot
    .unwrap();
    t = lock
        .pilots
        .iter()
        .find(|p| p.name == "Alice")
        .unwrap()
        .target_lock
        .as_ref()
        .unwrap()
        .completes_at_ms
        + 1;
    common::mine_fixed_cycles(&mut s, &mut alice, 3, &mut t);
    deny_unchanged(&mut s, &mut alice, Request::DepositOre { units: 1 }, t);
    travel(&mut s, &mut alice, Destination::Station, &mut t);
    ok(&mut s, &mut alice, Request::Dock, t);
    ok(&mut s, &mut alice, Request::DepositOre { units: 20 }, t);
    ok(
        &mut s,
        &mut alice,
        Request::CreateCorporation {
            name: "Builders".into(),
        },
        t,
    );
    deny_unchanged(
        &mut s,
        &mut bob,
        Request::AcceptCorporation {
            corporation: "Builders".into(),
        },
        t,
    );
    ok(
        &mut s,
        &mut alice,
        Request::InviteCorporation {
            pilot: "Bob".into(),
        },
        t,
    );
    ok(
        &mut s,
        &mut bob,
        Request::AcceptCorporation {
            corporation: "Builders".into(),
        },
        t,
    );
    ok(
        &mut s,
        &mut alice,
        Request::DepositCorporationOre { units: 10 },
        t,
    );
    deny_unchanged(
        &mut s,
        &mut bob,
        Request::WithdrawCorporationOre { units: 1 },
        t,
    );
    deny_unchanged(
        &mut s,
        &mut eve,
        Request::WithdrawCorporationOre { units: 1 },
        t,
    );
    deny_unchanged(
        &mut s,
        &mut bob,
        Request::InviteCorporation {
            pilot: "Eve".into(),
        },
        t,
    );
    deny_unchanged(&mut s, &mut alice, Request::DepositOre { units: 0 }, t);
    deny_unchanged(&mut s, &mut alice, Request::WithdrawOre { units: 21 }, t);
    let boarded = ok(
        &mut s,
        &mut alice,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        t,
    );
    assert_eq!(
        boarded
            .snapshot
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == "Alice")
            .unwrap()
            .ore,
        0
    );
    assert_eq!(
        boarded
            .assets
            .unwrap()
            .ships
            .iter()
            .find(|h| h.ship_class == ShipClass::Pioneer)
            .unwrap()
            .ore,
        45
    );
    ok(&mut s, &mut alice, Request::WithdrawOre { units: 5 }, t);
    ok(
        &mut s,
        &mut alice,
        Request::SelectShip {
            ship_class: ShipClass::Pioneer,
        },
        t,
    );
    ok(
        &mut s,
        &mut alice,
        Request::WithdrawCorporationOre { units: 4 },
        t,
    );
    let export = audit(&mut s, t);
    assert_eq!(export.snapshot.stored_ore, 26);
    let credits = export
        .snapshot
        .pilots
        .iter()
        .map(|p| p.credits)
        .sum::<u64>();
    assert_eq!(credits, 3 * STARTER_CREDITS);
    let outsider = ok(&mut s, &mut eve, Request::Snapshot, t).assets.unwrap();
    assert!(outsider.corporation.is_none());
    assert!(outsider.storage.is_empty());
    assert!(outsider.ships.iter().all(|h| h.ore == 0));
    // Inactive hulls and stores remain at their station, even across a gate.
    ok(&mut s, &mut alice, Request::Undock, t);
    travel(&mut s, &mut alice, Destination::Gate { id: 101 }, &mut t);
    let jumping = ok(&mut s, &mut alice, Request::Jump { gate_id: 101 }, t)
        .snapshot
        .unwrap();
    let Location::Jumping { arrives_at_ms, .. } = jumping
        .pilots
        .iter()
        .find(|p| p.name == "Alice")
        .unwrap()
        .location
    else {
        panic!()
    };
    t = arrives_at_ms + 1;
    ok(&mut s, &mut alice, Request::Snapshot, t);
    travel(&mut s, &mut alice, Destination::Station, &mut t);
    ok(&mut s, &mut alice, Request::Dock, t);
    deny_unchanged(&mut s, &mut alice, Request::WithdrawOre { units: 1 }, t);
    deny_unchanged(
        &mut s,
        &mut alice,
        Request::WithdrawCorporationOre { units: 1 },
        t,
    );
    deny_unchanged(
        &mut s,
        &mut alice,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        t,
    );
    let before = audit(&mut s, t);
    drop(s);
    let mut s = Server::open(&path).unwrap();
    let resumed = connect(&mut s, &mut None, "Alice", Some(token), t + 1);
    let assets = resumed.assets.unwrap();
    assert_eq!(assets.ships.len(), 3);
    assert_eq!(
        assets
            .ships
            .iter()
            .find(|h| h.ship_class == ShipClass::Prospector)
            .unwrap()
            .ore,
        5
    );
    assert_eq!(
        assets
            .storage
            .iter()
            .find(|s| s.system_id == HOME_SYSTEM_ID)
            .unwrap()
            .ore,
        15
    );
    let corp = assets.corporation.unwrap();
    assert_eq!(corp.members.len(), 2);
    assert_eq!(corp.storage[0].ore, 6);
    let after = audit(&mut s, t + 1);
    assert_eq!(before.snapshot.ledger_hash, after.snapshot.ledger_hash);
    assert_eq!(after.snapshot.stored_ore, 26);
    drop(s);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn legacy_nonledger_hull_selection_migrates_once_without_minting_cargo() {
    let dir = std::env::temp_dir().join(format!(
        "arkline-assets-migration-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("world.sqlite");
    let mut server = Server::open(&path).unwrap();
    let token = connect(&mut server, &mut None, "Legacy", None, 0)
        .resume_token
        .unwrap();
    drop(server);
    // Recreate the prior schema: hull choice persisted without an economic event.
    let db = rusqlite::Connection::open(&path).unwrap();
    let raw: String = db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap();
    let mut world: serde_json::Value = serde_json::from_str(&raw).unwrap();
    world.as_object_mut().unwrap().remove("accounts");
    world.as_object_mut().unwrap().remove("corporations");
    world["pilots"]["Legacy"]["view"]["ship_class"] = serde_json::json!("vanguard");
    db.execute("UPDATE world SET json=?1 WHERE id=1", [world.to_string()])
        .unwrap();
    drop(db);
    for cycle in 0..3 {
        let mut server = Server::open(&path).unwrap();
        let response = connect(&mut server, &mut None, "Legacy", Some(token.clone()), cycle);
        let assets = response.assets.unwrap();
        assert_eq!(assets.ships.len(), 3);
        assert!(assets
            .ships
            .iter()
            .all(|h| h.ship_class != ShipClass::Vanguard
                && h.system_id == HOME_SYSTEM_ID
                && h.ore == 0));
        assert!(assets.storage.is_empty());
        let report = audit(&mut server, cycle);
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.snapshot.stored_ore, 0);
        assert_eq!(report.snapshot.pilots[0].ship_class, ShipClass::Vanguard);
    }
    std::fs::remove_dir_all(dir).unwrap();
}
