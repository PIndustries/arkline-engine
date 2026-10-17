use arkline_protocol::*;
use arkline_server::Server;
struct Fixture {
    path: std::path::PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!(
                "arkline-power-{}-{}.sqlite",
                std::process::id(),
                rand::random::<u64>()
            )),
        }
    }
    // Inject a boundary charge into the persisted world, never through a client API.
    fn charge(&self, name: &str, amount: u64, now: u64) {
        let db = rusqlite::Connection::open(&self.path).unwrap();
        let raw: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut json: serde_json::Value = serde_json::from_str(&raw).unwrap();
        let p = &mut json["pilots"][name]["view"]["power"];
        p["charge_j"] = amount.into();
        p["updated_at_ms"] = now.into();
        p["regen_remainder_mj"] = 0.into();
        db.execute("UPDATE world SET json=?1 WHERE id=1", [json.to_string()])
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        for suffix in [".lock", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.path.display()));
        }
    }
}
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
fn connect(s: &mut Server) -> Option<String> {
    let mut p = None;
    ok(
        s,
        &mut p,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Power".into(),
            resume_token: None,
        },
        0,
    );
    p
}
#[test]
fn reactor_regenerates_caps_and_failed_warp_never_draws_charge() {
    let f = Fixture::new();
    let mut s = Server::open(&f.path).unwrap();
    let mut p = connect(&mut s);
    ok(&mut s, &mut p, Request::Undock, 0);
    let spec = power_spec(ShipClass::Pioneer);
    f.charge("Power", 100, 0);
    let state = ok(&mut s, &mut p, Request::Snapshot, 1000);
    assert_eq!(
        state.pilots[0].power.as_ref().unwrap().charge_j,
        100 + spec.regen_w - default_pioneer_exterior().load_w()
    );
    f.charge("Power", spec.warp_cost_j - 1, 1000);
    assert!(
        !s.handle_at(
            &mut p,
            Request::Warp {
                destination: Destination::Belt
            },
            1000
        )
        .ok
    );
    let state = ok(&mut s, &mut p, Request::Snapshot, 1000);
    assert_eq!(
        state.pilots[0].power.as_ref().unwrap().charge_j,
        spec.warp_cost_j - 1
    );
    assert!(!matches!(
        state.pilots[0].location,
        Location::InTransit { .. }
    ));
    f.charge("Power", spec.capacity_j, 1000);
    assert!(
        !s.handle_at(
            &mut p,
            Request::Warp {
                destination: Destination::Gate { id: u64::MAX }
            },
            1000
        )
        .ok
    );
    let state = ok(&mut s, &mut p, Request::Snapshot, 1000);
    assert_eq!(
        state.pilots[0].power.as_ref().unwrap().charge_j,
        spec.capacity_j
    );
    let state = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        1000,
    );
    assert_eq!(
        state.pilots[0].power.as_ref().unwrap().charge_j,
        spec.capacity_j - spec.warp_cost_j
    );
    let state = ok(&mut s, &mut p, Request::Snapshot, 1_000_000);
    assert_eq!(
        state.pilots[0].power.as_ref().unwrap().charge_j,
        spec.capacity_j
    );
}
#[test]
fn turret_completion_draws_once_and_insufficient_charge_stops_without_ore() {
    let f = Fixture::new();
    let mut s = Server::open(&f.path).unwrap();
    let mut p = connect(&mut s);
    ok(&mut s, &mut p, Request::Undock, 0);
    let state = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    );
    let Location::InTransit { arrives_at_ms, .. } = state.pilots[0].location else {
        panic!()
    };
    let state = ok(&mut s, &mut p, Request::Snapshot, arrives_at_ms);
    let pos = state.pilots[0].flight.as_ref().unwrap().position;
    let id = state
        .asteroids
        .iter()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .min_by(|a, b| {
            let dist = |a: &Asteroid| {
                a.position
                    .iter()
                    .zip(pos)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    - a.radius
            };
            dist(a).total_cmp(&dist(b))
        })
        .unwrap()
        .id;
    let state = ok(&mut s, &mut p, Request::LockAsteroid { id }, arrives_at_ms);
    let t = state.pilots[0].target_locks[0].completes_at_ms;
    ok(
        &mut s,
        &mut p,
        Request::StartMining {
            module_id: 0,
            target_id: id,
        },
        t,
    );
    let spec = power_spec(ShipClass::Pioneer);
    let cycle = mining_spec(ShipClass::Pioneer);
    let due = t + cycle.cycle_ms;
    f.charge("Power", spec.mining_cycle_cost_j, due);
    let first = ok(&mut s, &mut p, Request::Snapshot, due);
    assert_eq!(first.pilots[0].ore, cycle.yield_units);
    assert_eq!(first.pilots[0].power.as_ref().unwrap().charge_j, 0);
    let again = ok(&mut s, &mut p, Request::Snapshot, due);
    assert_eq!(again.ledger_hash, first.ledger_hash);
    assert_eq!(again.pilots[0].power.as_ref().unwrap().charge_j, 0);
    f.charge("Power", spec.mining_cycle_cost_j - 1, due + cycle.cycle_ms);
    let stopped = ok(&mut s, &mut p, Request::Snapshot, due + cycle.cycle_ms);
    assert_eq!(stopped.pilots[0].ore, cycle.yield_units);
    assert_eq!(stopped.ledger_hash, first.ledger_hash);
    assert!(stopped.pilots[0].mining_modules[0].target_id.is_none());
    assert_eq!(
        stopped.pilots[0].power.as_ref().unwrap().charge_j,
        spec.mining_cycle_cost_j - 1
    );
    arkline_verifier::verify(
        &s.handle_at(&mut p, Request::ExportLedger, due + cycle.cycle_ms)
            .ledger
            .unwrap(),
        None,
    )
    .unwrap();
}
#[test]
fn attack_drone_draws_only_for_actual_shot_and_holds_when_empty() {
    let f = Fixture::new();
    let mut s = Server::open(&f.path).unwrap();
    let mut p = connect(&mut s);
    ok(&mut s, &mut p, Request::Undock, 0);
    let mut b = None;
    ok(
        &mut s,
        &mut b,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Target".into(),
            resume_token: None,
        },
        0,
    );
    ok(&mut s, &mut b, Request::Undock, 0);
    let state = ok(
        &mut s,
        &mut p,
        Request::LaunchDroneClass {
            class: DroneClass::Attack,
        },
        0,
    );
    let id = state
        .pilots
        .iter()
        .find(|p| p.name == "Power")
        .unwrap()
        .drones[0]
        .id;
    let state = ok(
        &mut s,
        &mut b,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        0,
    );
    let victim = state
        .pilots
        .iter()
        .find(|p| p.name == "Target")
        .unwrap()
        .drones[0]
        .id;
    assert!(
        !s.handle_at(
            &mut p,
            Request::AssignAttackDrone {
                id,
                target_pilot: "Target".into(),
                target_drone_id: u64::MAX
            },
            0
        )
        .ok
    );
    let mut state = ok(
        &mut s,
        &mut p,
        Request::AssignAttackDrone {
            id,
            target_pilot: "Target".into(),
            target_drone_id: victim,
        },
        0,
    );
    let mut now = 0;
    let ready = loop {
        if let Some(t) = state
            .pilots
            .iter()
            .find(|p| p.name == "Power")
            .unwrap()
            .drones[0]
            .attack_ready_at_ms
        {
            break t;
        }
        now += 100;
        state = ok(&mut s, &mut p, Request::Snapshot, now);
        assert!(now < 30_000);
    };
    let cost = power_spec(ShipClass::Pioneer).drone_attack_cost_j;
    f.charge("Power", cost, ready);
    let shot = ok(&mut s, &mut p, Request::Snapshot, ready);
    let owner = shot.pilots.iter().find(|p| p.name == "Power").unwrap();
    assert_eq!(owner.power.as_ref().unwrap().charge_j, 0);
    assert_eq!(
        shot.pilots
            .iter()
            .find(|p| p.name == "Target")
            .unwrap()
            .drones[0]
            .hit_points,
        DRONE_HIT_POINTS - DRONE_ATTACK_DAMAGE
    );
    let next = owner.drones[0].attack_ready_at_ms.unwrap();
    f.charge("Power", cost - 1, next);
    let empty = ok(&mut s, &mut p, Request::Snapshot, next);
    assert_eq!(
        empty
            .pilots
            .iter()
            .find(|p| p.name == "Target")
            .unwrap()
            .drones[0]
            .hit_points,
        DRONE_HIT_POINTS - DRONE_ATTACK_DAMAGE
    );
    let owner = empty.pilots.iter().find(|p| p.name == "Power").unwrap();
    assert!(matches!(owner.drones[0].mode, DroneMode::Hold));
    assert_eq!(owner.power.as_ref().unwrap().charge_j, cost - 1);
    assert_eq!(empty.ledger_hash, shot.ledger_hash);
}
#[test]
fn saved_charge_survives_restart_without_reissuing_a_full_capacitor() {
    let f = Fixture::new();
    let future = u64::MAX / 2;
    {
        let mut s = Server::open(&f.path).unwrap();
        let mut p = connect(&mut s);
        f.charge("Power", 12345, future);
        let state = ok(&mut s, &mut p, Request::Snapshot, future);
        assert_eq!(state.pilots[0].power.as_ref().unwrap().charge_j, 12345);
    }
    let mut s = Server::open(&f.path).unwrap();
    let state = ok(&mut s, &mut None, Request::Snapshot, future);
    assert_eq!(state.pilots[0].power.as_ref().unwrap().charge_j, 12345);
}

#[test]
fn drone_mining_pays_on_completion_and_empty_capacitor_cannot_create_ore() {
    let f = Fixture::new();
    let mut s = Server::open(&f.path).unwrap();
    let mut p = connect(&mut s);
    ok(&mut s, &mut p, Request::Undock, 0);
    let state = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    );
    let Location::InTransit { arrives_at_ms, .. } = state.pilots[0].location else {
        panic!()
    };
    let state = ok(&mut s, &mut p, Request::Snapshot, arrives_at_ms);
    let pos = state.pilots[0].flight.as_ref().unwrap().position;
    let target = state
        .asteroids
        .iter()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .min_by(|a, b| {
            let dist = |a: &Asteroid| {
                a.position
                    .iter()
                    .zip(pos)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    - a.radius
            };
            dist(a).total_cmp(&dist(b))
        })
        .unwrap()
        .id;
    let state = ok(
        &mut s,
        &mut p,
        Request::LockAsteroid { id: target },
        arrives_at_ms,
    );
    let mut t = state.pilots[0].target_locks[0].completes_at_ms;
    let state = ok(
        &mut s,
        &mut p,
        Request::LaunchDroneClass {
            class: DroneClass::Mining,
        },
        t,
    );
    let id = state.pilots[0].drones[0].id;
    let mut state = ok(
        &mut s,
        &mut p,
        Request::AssignMiningDrone {
            id,
            target_id: target,
        },
        t,
    );
    let deadline = t + 60_000;
    let due = loop {
        if let Some(c) = &state.pilots[0].drones[0].mining_cycle {
            break c.completes_at_ms;
        }
        t += 100;
        state = ok(&mut s, &mut p, Request::Snapshot, t);
        assert!(t < deadline);
    };
    let cost = power_spec(ShipClass::Pioneer).drone_mining_cycle_cost_j;
    f.charge("Power", cost, due);
    let first = ok(&mut s, &mut p, Request::Snapshot, due);
    assert_eq!(first.pilots[0].ore, DRONE_MINING_YIELD);
    assert_eq!(first.pilots[0].power.as_ref().unwrap().charge_j, 0);
    let next = first.pilots[0].drones[0]
        .mining_cycle
        .as_ref()
        .unwrap()
        .completes_at_ms;
    f.charge("Power", cost - 1, next);
    let stopped = ok(&mut s, &mut p, Request::Snapshot, next);
    assert_eq!(stopped.ledger_hash, first.ledger_hash);
    assert_eq!(stopped.pilots[0].ore, DRONE_MINING_YIELD);
    assert_eq!(stopped.asteroids, first.asteroids);
    assert!(matches!(stopped.pilots[0].drones[0].mode, DroneMode::Hold));
    assert_eq!(stopped.pilots[0].power.as_ref().unwrap().charge_j, cost - 1);
}
