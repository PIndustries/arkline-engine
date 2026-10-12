mod common;
use arkline_protocol::*;
use arkline_server::Server;
fn run(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r.snapshot.unwrap()
}
fn pilot(s: &mut Server, name: &str) -> Option<String> {
    let mut p = None;
    run(
        s,
        &mut p,
        Request::Connect {
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: None,
        },
        0,
    );
    run(s, &mut p, Request::Undock, 0);
    p
}
fn flight(s: &mut Server, name: &str, t: u64) -> FlightState {
    run(s, &mut None, Request::Snapshot, t)
        .pilots
        .into_iter()
        .find(|p| p.name == name)
        .unwrap()
        .flight
        .unwrap()
}
fn norm(v: [f64; 3]) -> f64 {
    v.iter().map(|x| x * x).sum::<f64>().sqrt()
}
fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    norm(std::array::from_fn(|i| a[i] - b[i]))
}
#[test]
fn flight_accelerates_brakes_validates_and_last_logout_stops() {
    let mut s = Server::open(":memory:").unwrap();
    let mut p = pilot(&mut s, "Mover");
    let baseline = run(&mut s, &mut p, Request::Snapshot, 0).ledger_hash;
    for mode in [
        NavigationMode::FlyTo {
            position: [f64::NAN, 0.0, 0.0],
        },
        NavigationMode::FlyTo {
            position: [SPACE_BOUND + 1.0, 0.0, 0.0],
        },
        NavigationMode::Orbit {
            target: NavigationTarget::Station,
            radius: 0.0,
        },
        NavigationMode::Approach {
            target: NavigationTarget::Pilot {
                pilot: "Mover".into(),
            },
        },
    ] {
        assert!(!s.handle_at(&mut p, Request::Navigate { mode }, 0).ok);
    }
    run(
        &mut s,
        &mut p,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: [-20000.0, 0.0, 1000.0],
            },
        },
        0,
    );
    let mut previous = flight(&mut s, "Mover", 0);
    for t in (100..=10000).step_by(100) {
        s.tick_at(t).unwrap();
        let f = flight(&mut s, "Mover", t);
        assert!(norm(f.velocity) <= MAX_SHIP_SPEED + 1e-6);
        assert!(dist(f.velocity, previous.velocity) <= SHIP_ACCELERATION * 0.1 + 1e-5);
        previous = f;
    }
    assert!(dist(previous.position, STATION_POSITION) > DOCK_RANGE);
    assert!(!s.handle_at(&mut p, Request::Dock, 10000).ok);
    assert!(!s.handle_at(&mut p, Request::Mine { units: 1 }, 10000).ok);
    run(
        &mut s,
        &mut p,
        Request::Navigate {
            mode: NavigationMode::Stop,
        },
        10000,
    );
    for t in (10100..=21000).step_by(100) {
        s.tick_at(t).unwrap();
    }
    let stopped = flight(&mut s, "Mover", 21000);
    assert!(norm(stopped.velocity) < 0.001);
    run(
        &mut s,
        &mut p,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: [0.0, 0.0, 0.0],
            },
        },
        21000,
    );
    s.tick_at(22000).unwrap();
    let at_logout = flight(&mut s, "Mover", 22000).position;
    s.disconnect(&mut p);
    s.tick_at(23000).unwrap();
    let after = flight(&mut s, "Mover", 23000);
    assert_eq!(at_logout, after.position);
    assert_eq!(after.velocity, [0.0; 3]);
    assert_eq!(
        baseline,
        run(&mut s, &mut None, Request::Snapshot, 23000).ledger_hash
    );
}
#[test]
fn orbit_settles_and_keep_range_tracks_a_moving_ship() {
    let mut s = Server::open(":memory:").unwrap();
    let mut orbiter = pilot(&mut s, "Orbiter");
    run(
        &mut s,
        &mut orbiter,
        Request::Navigate {
            mode: NavigationMode::Orbit {
                target: NavigationTarget::Station,
                radius: 1500.0,
            },
        },
        0,
    );
    for t in (100..=120000).step_by(100) {
        s.tick_at(t).unwrap();
    }
    let orbit = flight(&mut s, "Orbiter", 120000);
    let radius = dist(orbit.position, STATION_POSITION);
    assert!((radius - 1500.0).abs() < 150.0, "orbit radius {radius}");
    assert!(norm(orbit.velocity) > 50.0);
    let mut lead = pilot(&mut s, "Lead");
    let mut follow = pilot(&mut s, "Follow");
    run(
        &mut s,
        &mut lead,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: [-4500.0, 0.0, 16000.0],
            },
        },
        120000,
    );
    run(
        &mut s,
        &mut follow,
        Request::Navigate {
            mode: NavigationMode::KeepRange {
                target: NavigationTarget::Pilot {
                    pilot: "Lead".into(),
                },
                distance: 1000.0,
            },
        },
        120000,
    );
    for t in (120100..=180000).step_by(100) {
        s.tick_at(t).unwrap();
        if t == 135000 {
            let leader = flight(&mut s, "Lead", t);
            let follower = flight(&mut s, "Follow", t);
            assert!(
                norm(leader.velocity) > 500.0 && norm(follower.velocity) > 500.0,
                "both ships must actually be moving while tracking"
            );
            assert!(
                dist(leader.position, follower.position) < 2000.0,
                "moving target tracking diverged"
            );
        }
    }
    let a = flight(&mut s, "Lead", 180000);
    let b = flight(&mut s, "Follow", 180000);
    let gap = dist(a.position, b.position);
    assert!((gap - 1000.0).abs() < 100.0, "keep range gap {gap}");
}

#[test]
fn warp_holds_source_during_warmup_and_arrives_in_range() {
    let mut s = Server::open(":memory:").unwrap();
    let mut p = pilot(&mut s, "WarpPilot");
    let start = flight(&mut s, "WarpPilot", 0).position;
    let mut other = pilot(&mut s, "OtherPilot");
    assert_ne!(start, flight(&mut s, "OtherPilot", 0).position);
    run(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    );
    assert_eq!(
        flight(&mut s, "WarpPilot", WARP_WARMUP_MS - 1).position,
        start
    );
    let middle = flight(&mut s, "WarpPilot", WARP_WARMUP_MS + 1000).position;
    assert!(dist(middle, start) > 100.0);
    assert!(
        !s.handle_at(
            &mut p,
            Request::Navigate {
                mode: NavigationMode::Stop
            },
            WARP_WARMUP_MS + 1000
        )
        .ok
    );
    let arrived = flight(&mut s, "WarpPilot", route_ms());
    assert!(dist(arrived.position, BELT_POSITION) < MINING_RANGE);
    acquire(&mut s, &mut p, route_ms());
    common::mine_fixed_cycles(&mut s, &mut p, 1, &mut (route_ms() + 8000));
    // Docking remains permitted from the separated station berth.
    run(&mut s, &mut other, Request::Dock, route_ms());
}

#[test]
fn legacy_world_gains_flight_without_changing_inventory_or_history() {
    let path = std::env::temp_dir().join(format!(
        "arkline-flight-migration-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let before;
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = pilot(&mut s, "Legacy");
        run(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Belt,
            },
            0,
        );
        acquire(&mut s, &mut p, route_ms());
        before = common::mine_fixed_cycles(&mut s, &mut p, 1, &mut (route_ms() + 8000));
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let json: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old["pilots"]["Legacy"]["view"]
            .as_object_mut()
            .unwrap()
            .remove("flight");
        old["pilots"]["Legacy"]
            .as_object_mut()
            .unwrap()
            .remove("warp");
        db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
    }
    {
        let mut s = Server::open(&path).unwrap();
        let restored = run(&mut s, &mut None, Request::Snapshot, route_ms() + 100);
        assert_eq!(restored.pilots[0].ore, 25);
        assert_eq!(restored.pilots[0].credits, before.pilots[0].credits);
        assert_eq!(restored.ledger_hash, before.ledger_hash);
        assert_eq!(restored.belt_ore, before.belt_ore);
        assert!(
            dist(
                restored.pilots[0].flight.as_ref().unwrap().position,
                BELT_POSITION
            ) < MINING_RANGE
        );
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}

fn route_ms() -> u64 {
    plan_warp(STATION_POSITION, BELT_POSITION, 0).arrives_at_ms
}

#[test]
fn astronomical_migration_preserves_local_offset_and_economic_history() {
    let path = std::env::temp_dir().join(format!(
        "arkline-au-migration-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let before;
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = pilot(&mut s, "ScalePilot");
        run(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Belt,
            },
            0,
        );
        acquire(&mut s, &mut p, route_ms());
        before = common::mine_fixed_cycles(&mut s, &mut p, 1, &mut (route_ms() + 8000));
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let json: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old.as_object_mut().unwrap().remove("spatial_version");
        old["pilots"]["ScalePilot"]["view"]["flight"]["position"] =
            serde_json::json!([6215.0, 22.0, 900.0]);
        for asteroid in old["asteroids"].as_array_mut().unwrap() {
            let x = asteroid["position"][0].as_f64().unwrap();
            asteroid["position"][0] = serde_json::json!(x - BELT_POSITION[0] + 6200.0);
        }
        db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&old).unwrap()],
        )
        .unwrap();
    }
    for _ in 0..2 {
        let mut s = Server::open(&path).unwrap();
        let after = run(&mut s, &mut None, Request::Snapshot, route_ms() + 10);
        assert_eq!(
            after.pilots[0].flight.as_ref().unwrap().position,
            [BELT_POSITION[0] + 15.0, 22.0, 900.0]
        );
        assert_eq!(after.pilots[0].ore, before.pilots[0].ore);
        assert_eq!(after.ledger_hash, before.ledger_hash);
        assert_eq!(after.belt_ore, before.belt_ore);
        assert_eq!(after.asteroids, before.asteroids);
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}

#[test]
fn persisted_public_trajectory_samples_exactly_after_restart() {
    let path = std::env::temp_dir().join(format!(
        "arkline-trajectory-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let trajectory;
    {
        let mut s = Server::open(&path).unwrap();
        let mut p = pilot(&mut s, "WarpResume");
        let snapshot = run(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Gate { id: 101 },
            },
            1000,
        );
        trajectory = snapshot.pilots[0]
            .flight
            .as_ref()
            .unwrap()
            .warp
            .clone()
            .unwrap();
        s.tick_at(4500).unwrap();
    }
    {
        let mut s = Server::open(&path).unwrap();
        let f = flight(&mut s, "WarpResume", 4600);
        assert_eq!(f.warp.as_ref(), Some(&trajectory));
        assert_eq!((f.position, f.velocity), sample_warp(&trajectory, 4600));
        let arrived = flight(&mut s, "WarpResume", trajectory.arrives_at_ms);
        assert_eq!(arrived.position, trajectory.target);
        assert!(arrived.warp.is_none());
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}

fn acquire(s: &mut Server, p: &mut Option<String>, now: u64) {
    let snapshot = run(s, p, Request::Snapshot, now);
    let position = snapshot
        .pilots
        .iter()
        .find(|v| Some(&v.name) == p.as_ref())
        .unwrap()
        .flight
        .as_ref()
        .unwrap()
        .position;
    let asteroid = snapshot
        .asteroids
        .iter()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .min_by(|a, b| dist(position, a.position).total_cmp(&dist(position, b.position)))
        .unwrap();
    run(s, p, Request::LockAsteroid { id: asteroid.id }, now);
    run(
        s,
        p,
        Request::Snapshot,
        now + lock_duration_ms(ShipClass::Pioneer, asteroid.radius),
    );
}
