//! Black-box acceptance: two clients, mining, escrow trade, abrupt restart.
use arkline_protocol::*;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Server {
    process: Child,
    address: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
impl Server {
    fn start(db: &Path, address: String) -> Self {
        let process = Command::new(env!("CARGO_BIN_EXE_arkline-server"))
            .args(["--bind", &address, "--db", db.to_str().unwrap()])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut server = Self { process, address };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if TcpStream::connect(&server.address).is_ok() {
                return server;
            }
            assert!(
                server.process.try_wait().unwrap().is_none(),
                "server exited before listening"
            );
            assert!(Instant::now() < deadline, "server never became ready");
            thread::sleep(Duration::from_millis(25));
        }
    }
}
struct Client(BufReader<TcpStream>);
impl Client {
    fn new(address: &str) -> Self {
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        Self(BufReader::new(stream))
    }
    fn send(&mut self, request: Request) -> Response {
        let mut bytes = serde_json::to_vec(&request).unwrap();
        bytes.push(b'\n');
        self.0.get_mut().write_all(&bytes).unwrap();
        let mut line = String::new();
        self.0.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn ok(&mut self, request: Request) -> Response {
        let response = self.send(request);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn login(&mut self, pilot: &str, token: Option<String>) -> Response {
        self.ok(Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: pilot.into(),
            resume_token: token,
        })
    }
    fn travel(&mut self, destination: Destination, pilot: &str) {
        self.ok(Request::Warp { destination });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snap = self.ok(Request::Snapshot).snapshot.unwrap();
            if !matches!(
                snap.pilots
                    .iter()
                    .find(|p| p.name == pilot)
                    .unwrap()
                    .location,
                Location::InTransit { .. }
            ) {
                break;
            }
            assert!(Instant::now() < deadline, "warp never completed");
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[test]
fn two_pilots_trade_recover_and_export_consistent_history() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("arkline-journey-{}-{unique}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let db = directory.join("world.sqlite");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let server = Server::start(&db, address.clone());
    let mut alice = Client::new(&address);
    let alice_token = alice.login("Alice", None).resume_token.unwrap();
    let mut bob = Client::new(&address);
    let bob_token = bob.login("Bob", None).resume_token.unwrap();
    let mut attacker = Client::new(&address);
    assert!(
        !attacker
            .send(Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                pilot: "Alice".into(),
                resume_token: None
            })
            .ok
    );
    assert!(!attacker.send(Request::Mine { units: 1 }).ok);
    assert!(
        !alice.send(Request::Mine { units: 100 }).ok,
        "docked mining must fail"
    );
    alice.ok(Request::Undock);
    alice.travel(Destination::Belt, "Alice");
    alice.ok(Request::Mine { units: 100 });
    alice.travel(Destination::Station, "Alice");
    alice.ok(Request::Dock);
    let order = alice
        .ok(Request::PlaceSell {
            units: 60,
            unit_price: 7,
        })
        .snapshot
        .unwrap()
        .orders[0]
        .id;
    assert!(
        !alice
            .send(Request::Buy {
                order_id: order,
                units: 1
            })
            .ok,
        "self trading must fail"
    );
    assert!(
        !bob.send(Request::Buy {
            order_id: order,
            units: 61
        })
        .ok,
        "overselling must fail"
    );
    bob.ok(Request::Buy {
        order_id: order,
        units: 35,
    });
    let before = alice.ok(Request::Snapshot).snapshot.unwrap();
    let observed = bob.ok(Request::Snapshot).snapshot.unwrap();
    assert_eq!(before.pilots, observed.pilots);
    assert_eq!(before.orders, observed.orders);
    assert_eq!(
        before
            .pilots
            .iter()
            .find(|p| p.name == "Alice")
            .unwrap()
            .credits,
        1245
    );
    assert_eq!(
        before
            .pilots
            .iter()
            .find(|p| p.name == "Bob")
            .unwrap()
            .credits,
        755
    );
    assert_eq!(before.orders[0].units, 25);
    drop(alice);
    drop(bob);
    drop(attacker);
    drop(server);

    // Abrupt kill, not a graceful shutdown: committed state must survive.
    let server = Server::start(&db, address.clone());
    let mut alice = Client::new(&address);
    alice.login("Alice", Some(alice_token));
    let mut bob = Client::new(&address);
    bob.login("Bob", Some(bob_token));
    let after = bob.ok(Request::Snapshot).snapshot.unwrap();
    assert_eq!(before.pilots, after.pilots);
    assert_eq!(before.orders, after.orders);
    assert_eq!(before.ledger_hash, after.ledger_hash);
    alice.ok(Request::CancelSell { order_id: order });
    let export = bob.ok(Request::ExportLedger).ledger.unwrap();
    assert!(export.snapshot.orders.is_empty());
    assert_eq!(
        export
            .snapshot
            .pilots
            .iter()
            .find(|p| p.name == "Alice")
            .unwrap()
            .ore,
        65
    );
    assert_eq!(
        export
            .snapshot
            .pilots
            .iter()
            .find(|p| p.name == "Bob")
            .unwrap()
            .ore,
        35
    );
    assert_eq!(export.snapshot.belt_ore, INITIAL_BELT_ORE - 100);
    let mut previous = GENESIS_HASH.to_string();
    for (index, entry) in export.entries.iter().enumerate() {
        assert_eq!(entry.sequence, index as u64 + 1);
        assert_eq!(entry.previous_hash, previous);
        assert_eq!(entry.hash, entry.calculate_hash());
        previous.clone_from(&entry.hash);
    }
    assert_eq!(export.snapshot.ledger_hash, previous);
    assert_eq!(export.entries.len(), 6);
    let report = arkline_verifier::verify(&export, None).unwrap();
    assert_eq!(report.trades, 1);
    assert_eq!(report.total_credits, 2 * STARTER_CREDITS);
    let mut tampered = export.clone();
    tampered.snapshot.pilots[0].credits += 1;
    assert!(arkline_verifier::verify(&tampered, None).is_err());
    drop(alice);
    drop(bob);
    drop(server);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn transport_presence_counts_sessions_without_removing_persistent_ships() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("arkline-presence-{}-{unique}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let db = directory.join("world.sqlite");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    drop(listener);
    let server = Server::start(&db, address.clone());
    let mut observer = Client::new(&address);
    assert!(observer
        .ok(Request::Snapshot)
        .snapshot
        .unwrap()
        .online_pilots
        .is_empty());
    let mut first = Client::new(&address);
    let response = first.login("PresencePilot", None);
    let token = response.resume_token.unwrap();
    assert_eq!(
        response.snapshot.unwrap().online_pilots,
        vec!["PresencePilot"]
    );
    first.ok(Request::Undock);
    let mut second = Client::new(&address);
    second.login("PresencePilot", Some(token.clone()));
    // Reauthentication on the same connection must not leak an extra reference.
    second.login("PresencePilot", Some(token.clone()));
    let rejected = second.send(Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        pilot: "PresencePilot".into(),
        resume_token: Some("wrong".into()),
    });
    assert!(!rejected.ok);
    drop(first);
    assert_eq!(
        observer
            .ok(Request::Snapshot)
            .snapshot
            .unwrap()
            .online_pilots,
        vec!["PresencePilot"]
    );
    drop(second);
    let deadline = Instant::now() + Duration::from_secs(5);
    let offline = loop {
        let snapshot = observer.ok(Request::Snapshot).snapshot.unwrap();
        if snapshot.online_pilots.is_empty() {
            break snapshot;
        }
        assert!(
            Instant::now() < deadline,
            "closed transports remained online"
        );
        thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(offline.pilots[0].name, "PresencePilot");
    assert_eq!(offline.pilots[0].location, Location::Station);
    assert_eq!(
        offline.ledger_sequence, 1,
        "presence must not append economic events"
    );
    let export = observer.ok(Request::ExportLedger).ledger.unwrap();
    assert!(export.snapshot.online_pilots.is_empty());
    assert_eq!(export.snapshot.pilots, offline.pilots);
    drop(observer);
    drop(server);
    // Restart clears transport presence, but keeps undocked ship and credential.
    let server = Server::start(&db, address.clone());
    let mut observer = Client::new(&address);
    assert!(observer
        .ok(Request::Snapshot)
        .snapshot
        .unwrap()
        .online_pilots
        .is_empty());
    let mut resumed = Client::new(&address);
    let response = resumed.login("PresencePilot", Some(token));
    let snapshot = response.snapshot.unwrap();
    assert_eq!(snapshot.online_pilots, vec!["PresencePilot"]);
    assert_eq!(snapshot.pilots[0].location, Location::Station);
    assert_eq!(snapshot.ledger_sequence, 1);
    drop(resumed);
    drop(observer);
    drop(server);
    fs::remove_dir_all(directory).unwrap();
}
