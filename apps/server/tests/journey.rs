//! Black-box acceptance: two clients, mining, escrow trade, abrupt restart.
use arkline_protocol::*;
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
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
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut server = Self { process, address };
        let stdout = server.process.stdout.take().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = sender.send(result);
        });
        let line = receiver
            .recv_timeout(Duration::from_secs(45))
            .expect("server did not report its bound address")
            .expect("server readiness output failed");
        server.address = line
            .trim()
            .strip_prefix("Arkline server listening on ")
            .expect("server exited before binding its socket")
            .to_string();
        server
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
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: pilot.into(),
            resume_token: token,
        })
    }
    fn mine_cycles(&mut self, pilot: &str, cycles: u64) -> Snapshot {
        let snapshot = self.ok(Request::Snapshot).snapshot.unwrap();
        let p = snapshot.pilots.iter().find(|p| p.name == pilot).unwrap();
        let before = p.ore;
        let spec = mining_spec(p.ship_class);
        self.ok(Request::StartMining {
            module_id: 0,
            target_id: p.locked_asteroid.unwrap(),
        });
        let deadline = Instant::now() + Duration::from_millis(spec.cycle_ms * cycles + 10000);
        loop {
            let snapshot = self.ok(Request::Snapshot).snapshot.unwrap();
            if snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot)
                .unwrap()
                .ore
                >= before + cycles * spec.yield_units
            {
                return self
                    .ok(Request::StopMining { module_id: 0 })
                    .snapshot
                    .unwrap();
            }
            assert!(Instant::now() < deadline, "mining cycles did not complete");
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn acquire(&mut self, id: u64, pilot: &str) {
        let response = self.ok(Request::LockAsteroid { id });
        let initial = response.snapshot.unwrap();
        let pending = initial.pilots.iter().find(|p| p.name == pilot).unwrap();
        assert_eq!(pending.target_lock.as_ref().unwrap().asteroid_id, id);
        assert_eq!(pending.locked_asteroid, None);
        assert!(
            !self
                .send(Request::StartMining {
                    module_id: 0,
                    target_id: id
                })
                .ok
        );
        assert!(!self.send(Request::Mine { units: 1 }).ok);
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let snapshot = self.ok(Request::Snapshot).snapshot.unwrap();
            let own = snapshot.pilots.iter().find(|p| p.name == pilot).unwrap();
            if own.locked_asteroid == Some(id) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "target acquisition never completed"
            );
            thread::sleep(Duration::from_millis(50));
        }
    }
    fn travel(&mut self, destination: Destination, pilot: &str) {
        self.ok(Request::Warp { destination });
        let deadline = Instant::now() + Duration::from_secs(45);
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
    // Let the child own its ephemeral port from bind onward. Reserving then
    // dropping a parent socket allowed another test/connection to take it.
    let server = Server::start(&db, "127.0.0.1:0".to_string());
    let address = server.address.clone();
    let mut alice = Client::new(&address);
    let alice_token = alice.login("Alice", None).resume_token.unwrap();
    let mut bob = Client::new(&address);
    let bob_token = bob.login("Bob", None).resume_token.unwrap();
    let mut attacker = Client::new(&address);
    assert!(
        !attacker
            .send(Request::Connect {
                supports_room_controls: false,
                world_layout_version: 1,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
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
    alice.acquire(1, "Alice");
    alice.mine_cycles("Alice", 4);
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
    assert_same_pilots_with_regeneration(&before, &observed);
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
    assert_same_pilots_with_regeneration(&before, &after);
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
    assert_eq!(export.entries.len(), 9); // Four fixed mining cycles plus creation and trade events.
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
    // Let the child own its ephemeral port from bind onward. Reserving then
    // dropping a parent socket allowed another test/connection to take it.
    let server = Server::start(&db, "127.0.0.1:0".to_string());
    let address = server.address.clone();
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
        supports_room_controls: false,
        world_layout_version: 1,
        supports_fleet_hangar: false,
        supports_pioneer_interior: true,
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

#[test]
fn gates_connect_three_systems_survive_restart_and_preserve_targeted_mining() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("arkline-gates-{}-{unique}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let db = directory.join("world.sqlite");
    // Let the child own its ephemeral port from bind onward. Reserving then
    // dropping a parent socket allowed another test/connection to take it.
    let server = Server::start(&db, "127.0.0.1:0".to_string());
    let address = server.address.clone();
    let mut traveler = Client::new(&address);
    let token = traveler.login("GateTraveler", None).resume_token.unwrap();
    traveler.ok(Request::Undock);
    assert!(
        !traveler.send(Request::Jump { gate_id: 101 }).ok,
        "gate requires proximity"
    );
    assert!(
        !traveler
            .send(Request::Navigate {
                mode: NavigationMode::Approach {
                    target: NavigationTarget::Asteroid { id: 73 }
                }
            })
            .ok
    );
    traveler.travel(Destination::Gate { id: 101 }, "GateTraveler");
    let charging = traveler
        .ok(Request::Jump { gate_id: 101 })
        .snapshot
        .unwrap();
    assert!(matches!(
        charging.pilots[0].location,
        Location::Jumping { .. }
    ));
    assert_eq!(charging.pilots[0].system_id, 1);
    drop(traveler);
    drop(server);
    let server = Server::start(&db, address.clone());
    let mut traveler = Client::new(&address);
    traveler.login("GateTraveler", Some(token.clone()));
    fn arrive(client: &mut Client, system: u32) {
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            let s = client.ok(Request::Snapshot).snapshot.unwrap();
            if s.pilots
                .iter()
                .find(|p| p.name == "GateTraveler")
                .unwrap()
                .system_id
                == system
            {
                break;
            }
            assert!(Instant::now() < deadline, "gate jump did not complete");
            thread::sleep(Duration::from_millis(50));
        }
    }
    arrive(&mut traveler, 2);
    traveler.travel(Destination::Gate { id: 202 }, "GateTraveler");
    traveler.ok(Request::Jump { gate_id: 202 });
    arrive(&mut traveler, 3);
    traveler.travel(Destination::Belt, "GateTraveler");
    let s = traveler.ok(Request::Snapshot).snapshot.unwrap();
    let asteroid = s
        .asteroids
        .iter()
        .find(|a| a.system_id == 3 && a.ore >= 10)
        .unwrap()
        .clone();
    traveler.acquire(asteroid.id, "GateTraveler");
    let mined = traveler.mine_cycles("GateTraveler", 1);
    assert_eq!(
        mined
            .asteroids
            .iter()
            .find(|a| a.id == asteroid.id)
            .unwrap()
            .ore,
        asteroid.ore - mining_spec(ShipClass::Pioneer).yield_units
    );
    let mut home = Client::new(&address);
    home.login("HomePilot", None);
    home.ok(Request::Undock);
    assert!(!home.send(Request::LockAsteroid { id: asteroid.id }).ok);
    assert!(
        !home
            .send(Request::Navigate {
                mode: NavigationMode::Approach {
                    target: NavigationTarget::Pilot {
                        pilot: "GateTraveler".into()
                    }
                }
            })
            .ok
    );
    let export = traveler.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    assert_eq!(
        export.snapshot.asteroids.iter().map(|a| a.ore).sum::<u64>(),
        export.snapshot.belt_ore
    );
    drop(home);
    drop(traveler);
    drop(server);
    let server = Server::start(&db, address.clone());
    let mut traveler = Client::new(&address);
    let s = traveler
        .login("GateTraveler", Some(token))
        .snapshot
        .unwrap();
    let pilot = s.pilots.iter().find(|p| p.name == "GateTraveler").unwrap();
    assert_eq!(pilot.system_id, 3);
    assert_eq!(pilot.ore, mining_spec(ShipClass::Pioneer).yield_units);
    assert_eq!(
        s.asteroids
            .iter()
            .find(|a| a.id == asteroid.id)
            .unwrap()
            .ore,
        asteroid.ore - mining_spec(ShipClass::Pioneer).yield_units
    );
    drop(traveler);
    drop(server);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn direct_piloting_over_tcp_expires_without_losing_roll() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("arkline-direct-{}-{unique}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let db = directory.join("world.sqlite");
    // Let the child own its ephemeral port from bind onward. Reserving then
    // dropping a parent socket allowed another test/connection to take it.
    let server = Server::start(&db, "127.0.0.1:0".to_string());
    let address = server.address.clone();
    let mut client = Client::new(&address);
    client.login("DirectPilot", None);
    let initial = client.ok(Request::Undock).snapshot.unwrap();
    let start = initial.pilots[0].flight.as_ref().unwrap().position;
    // Refresh live controls until the server confirms meaningful motion. Request
    // latency is part of the lease interval, so do not infer speed from sleeps.
    let movement_deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let moving = client
            .ok(Request::PilotInput {
                translation: [1.0, 0.0, 0.0],
                rotation: [1.0, 0.0, 0.0],
            })
            .snapshot
            .unwrap();
        let flight = moving.pilots[0].flight.as_ref().unwrap();
        if flight.position[0] > start[0] + 10.0 && flight.orientation[0].abs() > 0.2 {
            break;
        }
        assert!(
            Instant::now() < movement_deadline,
            "live input did not produce forward motion and roll"
        );
        thread::sleep(Duration::from_millis(50));
    }
    // Keep the connection open without refreshing controls. Expiry must stop
    // rotation immediately, while existing linear velocity brakes physically.
    let expiry_deadline = Instant::now() + Duration::from_millis(PILOT_INPUT_LEASE_MS + 2000);
    let expired = loop {
        let snapshot = client.ok(Request::Snapshot).snapshot.unwrap();
        let flight = snapshot.pilots[0].flight.as_ref().unwrap();
        if flight.mode == NavigationMode::Stop {
            assert_eq!(flight.angular_velocity, [0.0; 3]);
            assert!(flight.orientation[0].abs() > 0.2);
            break snapshot;
        }
        assert!(
            Instant::now() < expiry_deadline,
            "stale controls retained their lease"
        );
        thread::sleep(Duration::from_millis(50));
    };
    let flight = expired.pilots[0].flight.as_ref().unwrap();
    let stopped_orientation = flight.orientation;
    let speed = |velocity: [f64; 3]| velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
    let mut previous_speed = speed(flight.velocity);
    let braking_deadline = Instant::now()
        + Duration::from_secs_f64(
            previous_speed
                / ship_braking_acceleration(expired.pilots[0].ship_class, expired.pilots[0].ore)
                + 2.0,
        );
    loop {
        let snapshot = client.ok(Request::Snapshot).snapshot.unwrap();
        let flight = snapshot.pilots[0].flight.as_ref().unwrap();
        let current_speed = speed(flight.velocity);
        assert_eq!(flight.mode, NavigationMode::Stop);
        assert_eq!(flight.angular_velocity, [0.0; 3]);
        assert!(
            flight
                .orientation
                .iter()
                .zip(stopped_orientation)
                .all(|(a, b)| (a - b).abs() < 1e-9),
            "roll changed after input expiry"
        );
        assert!(
            current_speed <= previous_speed + 1e-6,
            "stale thrust accelerated the ship"
        );
        assert_eq!(snapshot.ledger_hash, initial.ledger_hash);
        if current_speed < 0.01 {
            break;
        }
        assert!(
            Instant::now() < braking_deadline,
            "ship did not finish braking after lease expiry"
        );
        previous_speed = current_speed;
        thread::sleep(Duration::from_millis(50));
    }
    drop(client);
    drop(server);
    fs::remove_dir_all(directory).unwrap();
}

/// A reactor can advance between network observations or during restart. Every
/// other persisted pilot field remains exactly equal; power must conserve the
/// elapsed interval and remain within the configured capacitor capacity.
fn assert_same_pilots_with_regeneration(before: &Snapshot, after: &Snapshot) {
    assert_eq!(before.pilots.len(), after.pilots.len());
    for (old, new) in before.pilots.iter().zip(&after.pilots) {
        let mut old = old.clone();
        let mut new = new.clone();
        let old_power = old.power.take().expect("authoritative power");
        let new_power = new.power.take().expect("authoritative power");
        assert_eq!(old, new);
        assert_eq!(old_power.capacity_j, new_power.capacity_j);
        assert_eq!(old_power.regen_w, new_power.regen_w);
        assert!(new_power.charge_j >= old_power.charge_j);
        assert!(new_power.charge_j <= new_power.capacity_j);
        let elapsed = after.server_time_ms.saturating_sub(before.server_time_ms);
        let maximum_gain = (elapsed as u128 * old_power.regen_w as u128
            + old_power.regen_remainder_mj as u128)
            / 1000;
        assert!(new_power.charge_j as u128 <= old_power.charge_j as u128 + maximum_gain);
    }
}
