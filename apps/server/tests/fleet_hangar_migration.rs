mod common;
use arkline_protocol::*;
use arkline_server::Server;
use serde_json::Value;
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
            supports_docked_traversal_v2: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: token,
        },
        t,
    )
}
fn travel(s: &mut Server, p: &mut Option<String>, destination: Destination, t: &mut u64) {
    let r = ok(s, p, Request::Warp { destination }, *t);
    let own = r
        .snapshot
        .unwrap()
        .pilots
        .into_iter()
        .find(|v| Some(&v.name) == p.as_ref())
        .unwrap();
    let Location::InTransit { arrives_at_ms, .. } = own.location else {
        panic!("expected transit")
    };
    *t = arrives_at_ms + 1;
    ok(s, p, Request::Snapshot, *t);
}
fn persisted(path: &std::path::Path) -> Value {
    let db = rusqlite::Connection::open(path).unwrap();
    let json: String = db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap();
    serde_json::from_str(&json).unwrap()
}
fn holdings(w: &Value) -> Value {
    let pilots=w["pilots"].as_object().unwrap().iter().map(|(n,p)|(n.clone(),serde_json::json!({"ore":p["view"]["ore"],"credits":p["view"]["credits"],"power":p["view"]["power"],"location":p["view"]["location"],"system_id":p["view"]["system_id"],"ship_class":p["view"]["ship_class"]}))).collect::<serde_json::Map<_,_>>();
    let accounts=w["accounts"].as_object().unwrap().iter().map(|(n,a)|(n.clone(),serde_json::json!({"ships":a["ships"],"storage":a["storage"],"corporation":a["corporation"]}))).collect::<serde_json::Map<_,_>>();
    serde_json::json!({"pilots":pilots,"accounts":accounts,"corporations":w["corporations"],"orders":w["orders"],"belt_ore":w["belt_ore"],"asteroids":w["asteroids"]})
}
#[test]
fn pre_fleet_sqlite_upgrade_preserves_nonempty_holdings_and_ledger_then_ids_survive_restart() {
    let path =
        std::env::temp_dir().join(format!("fleet-migration-{}.sqlite", rand::random::<u64>()));
    // Future clock keeps documented reactor regeneration out of this migration assertion.
    let mut t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 86_400_000;
    let mut s = Server::open(&path).unwrap();
    let (mut p, mut buyer) = (None, None);
    let token = connect(&mut s, &mut p, "Owner", None, false, t).resume_token;
    connect(&mut s, &mut buyer, "Buyer", None, false, t);
    ok(&mut s, &mut p, Request::Undock, t);
    travel(&mut s, &mut p, Destination::Belt, &mut t);
    let snap = ok(&mut s, &mut p, Request::Snapshot, t).snapshot.unwrap();
    let own = snap.pilots.iter().find(|p| p.name == "Owner").unwrap();
    let pos = own.flight.as_ref().unwrap().position;
    let rock = snap
        .asteroids
        .iter()
        .filter(|a| a.system_id == own.system_id)
        .min_by(|a, b| {
            let d = |a: &Asteroid| {
                a.position
                    .iter()
                    .zip(pos)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let lock = ok(&mut s, &mut p, Request::LockAsteroid { id: rock.id }, t)
        .snapshot
        .unwrap();
    t = lock
        .pilots
        .iter()
        .find(|p| p.name == "Owner")
        .unwrap()
        .target_lock
        .as_ref()
        .unwrap()
        .completes_at_ms
        + 1;
    common::mine_fixed_cycles(&mut s, &mut p, 3, &mut t);
    travel(&mut s, &mut p, Destination::Station, &mut t);
    ok(&mut s, &mut p, Request::Dock, t);
    ok(&mut s, &mut p, Request::DepositOre { units: 20 }, t);
    ok(
        &mut s,
        &mut p,
        Request::CreateCorporation {
            name: "Makers".into(),
        },
        t,
    );
    ok(
        &mut s,
        &mut p,
        Request::DepositCorporationOre { units: 10 },
        t,
    );
    let r = ok(
        &mut s,
        &mut p,
        Request::PlaceSell {
            units: 5,
            unit_price: 7,
        },
        t,
    );
    let order = r.snapshot.unwrap().orders[0].id;
    ok(
        &mut s,
        &mut buyer,
        Request::Buy {
            order_id: order,
            units: 2,
        },
        t,
    );
    ok(
        &mut s,
        &mut p,
        Request::SelectShip {
            ship_class: ShipClass::Prospector,
        },
        t,
    );
    ok(&mut s, &mut p, Request::WithdrawOre { units: 5 }, t);
    let before = ok(&mut s, &mut p, Request::ExportLedger, t).ledger.unwrap();
    arkline_verifier::verify(&before, None).unwrap();
    drop(s);
    // Persist the actual pre-upgrade JSON shape: no new account fields whatsoever.
    let mut legacy = persisted(&path);
    for a in legacy["accounts"].as_object_mut().unwrap().values_mut() {
        let a = a.as_object_mut().unwrap();
        assert!(a.remove("fleet_hangar").is_some());
        assert!(a.remove("vessel_ids").is_some());
    }
    let expected = holdings(&legacy);
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute("UPDATE world SET json=?1 WHERE id=1", [legacy.to_string()])
            .unwrap();
    }
    let mut s = Server::open(&path).unwrap();
    assert_eq!(holdings(&persisted(&path)), expected);
    let mut p = None;
    let r = connect(&mut s, &mut p, "Owner", token.clone(), true, t);
    let ids = r
        .assets
        .unwrap()
        .hangar
        .unwrap()
        .berths
        .into_iter()
        .map(|b| b.id)
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 7);
    let after = ok(&mut s, &mut p, Request::ExportLedger, t).ledger.unwrap();
    arkline_verifier::verify(&after, None).unwrap();
    assert_eq!(
        serde_json::to_value(&before.entries).unwrap(),
        serde_json::to_value(&after.entries).unwrap()
    );
    assert_eq!(before.snapshot.ledger_hash, after.snapshot.ledger_hash);
    assert_eq!(holdings(&persisted(&path)), expected);
    drop(s);
    let mut s = Server::open(&path).unwrap();
    let mut p = None;
    let r = connect(&mut s, &mut p, "Owner", token, true, t);
    assert_eq!(
        ids,
        r.assets
            .unwrap()
            .hangar
            .unwrap()
            .berths
            .into_iter()
            .map(|b| b.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(holdings(&persisted(&path)), expected);
    drop(s);
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
