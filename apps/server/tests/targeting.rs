use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Snapshot {
    let response = s.handle_at(p, r, t);
    assert!(response.ok, "{}", response.message);
    response.snapshot.unwrap()
}
fn setup(s: &mut Server) -> (Option<String>, u64, Vec<Asteroid>) {
    let mut p = None;
    ok(
        s,
        &mut p,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: "Targeter".into(),
            resume_token: None,
        },
        0,
    );
    ok(s, &mut p, Request::Undock, 0);
    let snap = ok(
        s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        0,
    );
    let Location::InTransit { arrives_at_ms, .. } = snap.pilots[0].location else {
        panic!()
    };
    let snap = ok(s, &mut p, Request::Snapshot, arrives_at_ms);
    let position = snap.pilots[0].flight.as_ref().unwrap().position;
    let targets = snap
        .asteroids
        .into_iter()
        .filter(|a| {
            a.system_id == HOME_SYSTEM_ID
                && position
                    .iter()
                    .zip(a.position)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt()
                    - a.radius
                    < MINING_RANGE
        })
        .collect();
    (p, arrives_at_ms, targets)
}
#[test]
fn acquisition_deadline_idempotence_retarget_unlock_and_early_mining() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, t, targets) = setup(&mut s);
    let a = &targets[0];
    let b = &targets[1];
    let first = ok(&mut s, &mut p, Request::LockAsteroid { id: a.id }, t);
    let lock = first.pilots[0].target_lock.clone().unwrap();
    assert_eq!(
        lock.completes_at_ms,
        t + lock_duration_ms(ShipClass::Pioneer, a.radius)
    );
    assert_eq!(first.pilots[0].locked_asteroid, None);
    for request in [
        Request::Mine { units: 1 },
        Request::MineAsteroid { id: a.id, units: 1 },
    ] {
        assert!(!s.handle_at(&mut p, request, lock.completes_at_ms - 1).ok);
    }
    let repeat = ok(&mut s, &mut p, Request::LockAsteroid { id: a.id }, t + 100);
    assert_eq!(repeat.pilots[0].target_lock, Some(lock.clone()));
    let changed = ok(&mut s, &mut p, Request::LockAsteroid { id: b.id }, t + 200);
    assert_eq!(
        changed.pilots[0].target_lock.as_ref().unwrap().asteroid_id,
        b.id
    );
    let cleared = ok(&mut s, &mut p, Request::UnlockTarget, t + 201);
    assert!(cleared.pilots[0].target_lock.is_none());
    ok(&mut s, &mut p, Request::LockAsteroid { id: a.id }, t + 202);
    let ready = ok(
        &mut s,
        &mut p,
        Request::Snapshot,
        lock.completes_at_ms + 202,
    );
    assert_eq!(ready.pilots[0].locked_asteroid, Some(a.id));
    assert!(ready.pilots[0].target_lock.is_none());
    assert_eq!(ready.ledger_hash, first.ledger_hash);
    let mined = ok(
        &mut s,
        &mut p,
        Request::Mine { units: 1 },
        lock.completes_at_ms + 202,
    );
    assert_eq!(mined.pilots[0].ore, 1);
    let cleared = ok(
        &mut s,
        &mut p,
        Request::UnlockTarget,
        lock.completes_at_ms + 203,
    );
    assert_eq!(cleared.pilots[0].locked_asteroid, None);
}
#[test]
fn invalidation_on_range_loss_warp_logout_and_restart() {
    let path = std::env::temp_dir().join(format!(
        "arkline-targeting-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let original_hash;
    {
        let mut s = Server::open(&path).unwrap();
        let (mut p, t, targets) = setup(&mut s);
        let a = &targets[0];
        original_hash = ok(&mut s, &mut p, Request::LockAsteroid { id: a.id }, t).ledger_hash;
        // Travel out of targeting range under ordinary authoritative navigation.
        let target = [
            BELT_POSITION[0],
            BELT_POSITION[1] + 50_000.0,
            BELT_POSITION[2],
        ];
        ok(
            &mut s,
            &mut p,
            Request::Navigate {
                mode: NavigationMode::FlyTo { position: target },
            },
            t,
        );
        for i in 1..=100 {
            s.tick_at(t + i * 1000).unwrap();
        }
        let far = ok(&mut s, &mut p, Request::Snapshot, t + 100_000);
        assert!(far.pilots[0].target_lock.is_none());
        assert!(far.pilots[0].locked_asteroid.is_none());
        let warp = ok(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Belt,
            },
            t + 100_000,
        );
        let Location::InTransit {
            arrives_at_ms: arrival,
            ..
        } = warp.pilots[0].location
        else {
            panic!()
        };
        ok(&mut s, &mut p, Request::LockAsteroid { id: a.id }, arrival);
        s.disconnect(&mut p);
        let offline = ok(&mut s, &mut None, Request::Snapshot, arrival);
        assert!(offline.pilots[0].target_lock.is_none());
        // A test session reuses the authenticated identity to isolate restart state.
        p = Some("Targeter".into());
        ok(
            &mut s,
            &mut p,
            Request::LockAsteroid { id: a.id },
            arrival + 1,
        );
        let warp = ok(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Station,
            },
            arrival + 2,
        );
        assert!(warp.pilots[0].target_lock.is_none());
        assert!(warp.pilots[0].locked_asteroid.is_none());
        let Location::InTransit {
            arrives_at_ms: station,
            ..
        } = warp.pilots[0].location
        else {
            panic!()
        };
        let warp = ok(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Belt,
            },
            station,
        );
        let Location::InTransit {
            arrives_at_ms: belt,
            ..
        } = warp.pilots[0].location
        else {
            panic!()
        };
        ok(&mut s, &mut p, Request::LockAsteroid { id: a.id }, belt);
    }
    {
        let mut s = Server::open(&path).unwrap();
        let snapshot = ok(&mut s, &mut None, Request::Snapshot, 1_000_000);
        assert!(snapshot.pilots[0].target_lock.is_none());
        assert!(snapshot.pilots[0].locked_asteroid.is_none());
        assert_eq!(snapshot.ledger_hash, original_hash);
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
