use arkline_protocol::*;
use arkline_server::Server;
use std::sync::{Arc, Mutex};
fn request(
    server: &mut Server,
    session: &mut Option<String>,
    request: Request,
    now: u64,
) -> Response {
    let r = server.handle_at(session, request, now);
    assert!(r.ok, "{}", r.message);
    r
}
fn connect(server: &mut Server, name: &str) -> (Option<String>, String) {
    let mut session = None;
    let response = request(
        server,
        &mut session,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: None,
        },
        100,
    );
    (session, response.resume_token.unwrap())
}
fn stock(server: &mut Server, session: &mut Option<String>) {
    request(server, session, Request::Undock, 101);
    request(
        server,
        session,
        Request::Warp {
            destination: Destination::Belt,
        },
        102,
    );
    request(
        server,
        session,
        Request::Mine { units: 100 },
        route_ms() + 102,
    );
    assert!(
        !server
            .handle_at(session, Request::Mine { units: 1 }, route_ms() + 103)
            .ok
    );
    request(
        server,
        session,
        Request::Warp {
            destination: Destination::Station,
        },
        route_ms() + 103,
    );
    request(server, session, Request::Dock, 2 * route_ms() + 103);
    request(
        server,
        session,
        Request::PlaceSell {
            units: 100,
            unit_price: 10,
        },
        2 * route_ms() + 104,
    );
}
#[test]
fn unauthorized_and_failed_trade_leave_ledger_unchanged() {
    let mut server = Server::open(":memory:").unwrap();
    let (mut alice, token) = connect(&mut server, "Alice");
    let (mut bob, _) = connect(&mut server, "Bob");
    stock(&mut server, &mut alice);
    let mut stranger = None;
    assert!(
        !server
            .handle_at(
                &mut stranger,
                Request::Connect {
                    protocol_version: 1,
                    pilot: "Alice".into(),
                    resume_token: Some("incorrect".into())
                },
                2 * route_ms() + 200
            )
            .ok
    );
    assert!(stranger.is_none());
    assert!(
        !server
            .handle_at(
                &mut stranger,
                Request::CancelSell { order_id: 1 },
                2 * route_ms() + 200
            )
            .ok
    );
    assert!(
        !server
            .handle_at(
                &mut bob,
                Request::CancelSell { order_id: 1 },
                2 * route_ms() + 200
            )
            .ok
    );
    assert!(
        !server
            .handle_at(
                &mut alice,
                Request::Buy {
                    order_id: 1,
                    units: 1
                },
                2 * route_ms() + 200
            )
            .ok
    );
    let before = request(
        &mut server,
        &mut stranger,
        Request::ExportLedger,
        2 * route_ms() + 200,
    )
    .ledger
    .unwrap();
    assert!(
        !server
            .handle_at(
                &mut bob,
                Request::Buy {
                    order_id: 1,
                    units: 101
                },
                2 * route_ms() + 200
            )
            .ok
    );
    let after = request(
        &mut server,
        &mut stranger,
        Request::ExportLedger,
        2 * route_ms() + 200,
    )
    .ledger
    .unwrap();
    assert_eq!(before.snapshot.ledger_hash, after.snapshot.ledger_hash);
    assert_eq!(before.snapshot.orders, after.snapshot.orders);
    assert_eq!(before.snapshot.pilots, after.snapshot.pilots);
    assert!(!serde_json::to_string(&after).unwrap().contains(&token));
    request(
        &mut server,
        &mut bob,
        Request::Buy {
            order_id: 1,
            units: 40,
        },
        2 * route_ms() + 300,
    );
    request(
        &mut server,
        &mut alice,
        Request::CancelSell { order_id: 1 },
        2 * route_ms() + 301,
    );
    let result = request(
        &mut server,
        &mut stranger,
        Request::ExportLedger,
        2 * route_ms() + 302,
    )
    .ledger
    .unwrap();
    assert!(result.snapshot.orders.is_empty());
    assert_eq!(result.snapshot.pilots[0].ore, 60);
    assert_eq!(result.snapshot.pilots[0].credits, 1400);
    assert_eq!(result.snapshot.pilots[1].ore, 40);
    assert_eq!(result.snapshot.pilots[1].credits, 600);
    let mut prior = GENESIS_HASH.to_string();
    for entry in result.entries {
        assert_eq!(entry.previous_hash, prior);
        assert_eq!(entry.hash, entry.calculate_hash());
        prior = entry.hash;
    }
}
#[test]
fn concurrent_buyers_cannot_duplicate_fill() {
    let mut server = Server::open(":memory:").unwrap();
    let (mut seller, _) = connect(&mut server, "Seller");
    let (buyer_a, _) = connect(&mut server, "BuyerA");
    let (buyer_b, _) = connect(&mut server, "BuyerB");
    stock(&mut server, &mut seller);
    let server = Arc::new(Mutex::new(server));
    let handles: Vec<_> = [buyer_a, buyer_b]
        .into_iter()
        .map(|mut session| {
            let server = server.clone();
            std::thread::spawn(move || {
                server
                    .lock()
                    .unwrap()
                    .handle_at(
                        &mut session,
                        Request::Buy {
                            order_id: 1,
                            units: 100,
                        },
                        2 * route_ms() + 500,
                    )
                    .ok
            })
        })
        .collect();
    assert_eq!(
        handles
            .into_iter()
            .map(|h| h.join().unwrap() as usize)
            .sum::<usize>(),
        1
    );
    let snapshot = request(
        &mut server.lock().unwrap(),
        &mut None,
        Request::Snapshot,
        2 * route_ms() + 600,
    )
    .snapshot
    .unwrap();
    assert!(snapshot.orders.is_empty());
    assert_eq!(snapshot.pilots.iter().map(|p| p.ore).sum::<u64>(), 100);
    assert_eq!(snapshot.pilots.iter().map(|p| p.credits).sum::<u64>(), 3000);
}
#[test]
fn restart_preserves_auth_travel_and_cooldown() {
    let path = std::env::temp_dir().join(format!(
        "arkline-server-test-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let token;
    {
        let mut server = Server::open(&path).unwrap();
        let (mut session, issued) = connect(&mut server, "Traveler");
        token = issued;
        request(&mut server, &mut session, Request::Undock, 1000);
        request(
            &mut server,
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
            1001,
        );
    }
    {
        let mut server = Server::open(&path).unwrap();
        let mut session = None;
        request(
            &mut server,
            &mut session,
            Request::Connect {
                protocol_version: 1,
                pilot: "Traveler".into(),
                resume_token: Some(token.clone()),
            },
            2000,
        );
        assert!(
            !server
                .handle_at(
                    &mut session,
                    Request::Mine { units: 100 },
                    route_ms() + 1000
                )
                .ok
        );
        request(
            &mut server,
            &mut session,
            Request::Mine { units: 100 },
            route_ms() + 1001,
        );
    }
    {
        let mut server = Server::open(&path).unwrap();
        let mut session = None;
        request(
            &mut server,
            &mut session,
            Request::Connect {
                protocol_version: 1,
                pilot: "Traveler".into(),
                resume_token: Some(token),
            },
            route_ms() + 1002,
        );
        assert!(
            !server
                .handle_at(
                    &mut session,
                    Request::Mine { units: 100 },
                    route_ms() + 1002
                )
                .ok
        );
        request(
            &mut server,
            &mut session,
            Request::Mine { units: 100 },
            route_ms() + 2001,
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn database_has_one_lifetime_owner() {
    let path = std::env::temp_dir().join(format!(
        "arkline-ownership-test-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let first = Server::open(&path).unwrap();
    let second = Server::open(&path);
    assert!(second.is_err());
    assert!(second.err().unwrap().to_string().contains("already owned"));
    drop(first);
    let reopened = Server::open(&path).expect("ownership released when server drops");
    drop(reopened);
    // The empty lock sidecar deliberately survives server shutdown; stale
    // files are harmless because the OS owns the actual lifetime lock.
    let mut sidecar = path.as_os_str().to_os_string();
    sidecar.push(".lock");
    assert!(std::path::Path::new(&sidecar).exists());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(sidecar).unwrap();
}

#[test]
fn independent_in_memory_worlds_do_not_share_ownership_lock() {
    let _first = Server::open(":memory:").unwrap();
    let _second = Server::open(":memory:").unwrap();
}

fn route_ms() -> u64 {
    plan_warp(STATION_POSITION, BELT_POSITION, 0).arrives_at_ms
}
