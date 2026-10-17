use arkline_protocol::*;
use arkline_server::Server;
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let response = s.handle_at(p, r, t);
    assert!(response.ok, "{}", response.message);
    response
}
fn setup(s: &mut Server) -> (Option<String>, u64, u64) {
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
            pilot: "Miner".into(),
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
    )
    .snapshot
    .unwrap();
    let Location::InTransit { arrives_at_ms, .. } = snap.pilots[0].location else {
        panic!()
    };
    let snap = ok(s, &mut p, Request::Snapshot, arrives_at_ms)
        .snapshot
        .unwrap();
    let pos = snap.pilots[0].flight.as_ref().unwrap().position;
    let a = snap
        .asteroids
        .iter()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .min_by(|a, b| distance(pos, a.position).total_cmp(&distance(pos, b.position)))
        .unwrap();
    let locked = ok(s, &mut p, Request::LockAsteroid { id: a.id }, arrives_at_ms)
        .snapshot
        .unwrap();
    let ready = locked.pilots[0]
        .target_lock
        .as_ref()
        .unwrap()
        .completes_at_ms;
    ok(s, &mut p, Request::Snapshot, ready);
    (p, ready, a.id)
}
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
fn snap(s: &mut Server, p: &mut Option<String>, t: u64) -> Snapshot {
    ok(s, p, Request::Snapshot, t).snapshot.unwrap()
}
#[test]
fn two_modules_repeat_exactly_at_deadline_and_stop_without_partial_award() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, t, id) = setup(&mut s);
    let spec = mining_spec(ShipClass::Pioneer);
    let initial = snap(&mut s, &mut p, t);
    assert_eq!(initial.pilots[0].mining_modules.len(), 2);
    for module_id in MINING_MODULE_IDS {
        ok(
            &mut s,
            &mut p,
            Request::StartMining {
                module_id,
                target_id: id,
            },
            t,
        );
    }
    let first = snap(&mut s, &mut p, t);
    // Repeated activation never resets or accelerates a running cycle.
    let again = ok(
        &mut s,
        &mut p,
        Request::StartMining {
            module_id: 0,
            target_id: id,
        },
        t + 500,
    )
    .snapshot
    .unwrap();
    assert_eq!(
        again.pilots[0].mining_modules[0],
        first.pilots[0].mining_modules[0]
    );
    assert_eq!(snap(&mut s, &mut p, t + spec.cycle_ms - 1).pilots[0].ore, 0);
    s.tick_at(t + spec.cycle_ms).unwrap();
    let completed = snap(&mut s, &mut p, t + spec.cycle_ms);
    assert_eq!(completed.pilots[0].ore, 2 * spec.yield_units);
    assert_eq!(completed.ledger_sequence, initial.ledger_sequence + 2);
    // Polling at the same timestamp cannot award either cycle again.
    assert_eq!(
        snap(&mut s, &mut p, t + spec.cycle_ms).pilots[0].ore,
        2 * spec.yield_units
    );
    s.tick_at(t + 2 * spec.cycle_ms).unwrap();
    assert_eq!(
        snap(&mut s, &mut p, t + 2 * spec.cycle_ms).pilots[0].ore,
        4 * spec.yield_units
    );
    for module_id in MINING_MODULE_IDS {
        ok(
            &mut s,
            &mut p,
            Request::StopMining { module_id },
            t + 2 * spec.cycle_ms + 1,
        );
    }
    let stopped = snap(&mut s, &mut p, t + 10 * spec.cycle_ms);
    assert_eq!(stopped.pilots[0].ore, 4 * spec.yield_units);
    assert!(stopped.pilots[0]
        .mining_modules
        .iter()
        .all(|m| m.target_id.is_none() && m.stop_reason.as_deref() == Some("Stopped by pilot")));
    let export = ok(
        &mut s,
        &mut p,
        Request::ExportLedger,
        t + 10 * spec.cycle_ms,
    )
    .ledger
    .unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}
#[test]
fn activation_validation_interruptions_and_suspended_clock_do_not_award() {
    for reason in ["unlock", "warp", "logout", "expired"] {
        let mut s = Server::open(":memory:").unwrap();
        let (mut p, t, id) = setup(&mut s);
        let spec = mining_spec(ShipClass::Pioneer);
        assert!(
            !s.handle_at(
                &mut p,
                Request::StartMining {
                    module_id: 9,
                    target_id: id
                },
                t
            )
            .ok
        );
        assert!(
            !s.handle_at(
                &mut p,
                Request::StartMining {
                    module_id: 0,
                    target_id: 99999
                },
                t
            )
            .ok
        );
        assert!(!s.handle_at(&mut p, Request::Mine { units: 100 }, t).ok);
        assert!(
            !s.handle_at(&mut p, Request::MineAsteroid { id, units: 1 }, t)
                .ok
        );
        ok(
            &mut s,
            &mut p,
            Request::StartMining {
                module_id: 0,
                target_id: id,
            },
            t,
        );
        match reason {
            "unlock" => {
                ok(&mut s, &mut p, Request::UnlockTarget, t + 1);
            }
            "warp" => {
                ok(
                    &mut s,
                    &mut p,
                    Request::Warp {
                        destination: Destination::Station,
                    },
                    t + 1,
                );
            }
            "logout" => s.disconnect(&mut p),
            _ => {}
        }
        s.tick_at(t + 3 * spec.cycle_ms).unwrap();
        let state = snap(&mut s, &mut p, t + 3 * spec.cycle_ms);
        assert_eq!(state.pilots[0].ore, 0, "{reason}");
        assert_eq!(state.ledger_sequence, 1);
        assert!(state.pilots[0].mining_modules[0].target_id.is_none());
        assert!(state.pilots[0].mining_modules[0].stop_reason.is_some());
    }
}
#[test]
fn restart_cancels_cycles_without_offline_mining() {
    let path = std::env::temp_dir().join(format!(
        "arkline-mining-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    {
        let mut s = Server::open(&path).unwrap();
        let (mut p, t, id) = setup(&mut s);
        ok(
            &mut s,
            &mut p,
            Request::StartMining {
                module_id: 0,
                target_id: id,
            },
            t,
        );
    }
    {
        let mut s = Server::open(&path).unwrap();
        let state = snap(&mut s, &mut None, 1_000_000);
        assert_eq!(state.pilots[0].ore, 0);
        assert_eq!(state.ledger_sequence, 1);
        assert_eq!(
            state.pilots[0].mining_modules[0].stop_reason.as_deref(),
            Some("Server restarted")
        );
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
#[test]
fn depletion_and_capacity_clip_only_the_terminal_cycle_and_stop() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, mut t, id) = setup(&mut s);
    let spec = mining_spec(ShipClass::Pioneer);
    let initial = snap(&mut s, &mut p, t);
    let ore = initial.asteroids.iter().find(|a| a.id == id).unwrap().ore;
    ok(
        &mut s,
        &mut p,
        Request::StartMining {
            module_id: 0,
            target_id: id,
        },
        t,
    );
    loop {
        t += spec.cycle_ms;
        s.tick_at(t).unwrap();
        let state = snap(&mut s, &mut p, t);
        if state.pilots[0].mining_modules[0].target_id.is_none() {
            assert_eq!(state.pilots[0].ore, ore);
            assert_eq!(
                state.pilots[0].mining_modules[0].stop_reason.as_deref(),
                Some("Asteroid depleted")
            );
            break;
        }
    }
    let state = snap(&mut s, &mut p, t);
    let pos = state.pilots[0].flight.as_ref().unwrap().position;
    let other = state
        .asteroids
        .iter()
        .find(|a| {
            a.system_id == HOME_SYSTEM_ID
                && a.ore > 0
                && distance(pos, a.position) - a.radius < MINING_RANGE
        })
        .unwrap();
    let lock = ok(&mut s, &mut p, Request::LockAsteroid { id: other.id }, t)
        .snapshot
        .unwrap();
    t = lock.pilots[0].target_lock.as_ref().unwrap().completes_at_ms;
    ok(
        &mut s,
        &mut p,
        Request::StartMining {
            module_id: 1,
            target_id: other.id,
        },
        t,
    );
    loop {
        t += spec.cycle_ms;
        s.tick_at(t).unwrap();
        let state = snap(&mut s, &mut p, t);
        if state.pilots[0].mining_modules[1].target_id.is_none() {
            assert_eq!(state.pilots[0].ore, CARGO_CAPACITY);
            assert_eq!(
                state.pilots[0].mining_modules[1].stop_reason.as_deref(),
                Some("Cargo full")
            );
            break;
        }
    }
    arkline_verifier::verify(
        &ok(&mut s, &mut p, Request::ExportLedger, t).ledger.unwrap(),
        None,
    )
    .unwrap();
}

#[test]
fn pending_lock_cannot_activate_and_leaving_range_stops_repeat() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, mut t, id) = setup(&mut s);
    ok(&mut s, &mut p, Request::UnlockTarget, t);
    let pending = ok(&mut s, &mut p, Request::LockAsteroid { id }, t)
        .snapshot
        .unwrap();
    assert!(
        !s.handle_at(
            &mut p,
            Request::StartMining {
                module_id: 0,
                target_id: id
            },
            t
        )
        .ok
    );
    t = pending.pilots[0]
        .target_lock
        .as_ref()
        .unwrap()
        .completes_at_ms;
    let active = ok(
        &mut s,
        &mut p,
        Request::StartMining {
            module_id: 0,
            target_id: id,
        },
        t,
    )
    .snapshot
    .unwrap();
    let mut destination = active.pilots[0].flight.as_ref().unwrap().position;
    destination[1] += 20_000.0;
    ok(
        &mut s,
        &mut p,
        Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: destination,
            },
        },
        t,
    );
    let stopped = loop {
        t += 100;
        s.tick_at(t).unwrap();
        let state = snap(&mut s, &mut p, t);
        if state.pilots[0].mining_modules[0].target_id.is_none() {
            break state;
        }
        assert!(
            t < active.server_time_ms + 30_000,
            "mining did not stop outside range"
        );
    };
    assert_eq!(
        stopped.pilots[0].mining_modules[0].stop_reason.as_deref(),
        Some("Target outside mining range")
    );
    s.tick_at(t + 60_000).unwrap();
    let later = snap(&mut s, &mut p, t + 60_000);
    assert_eq!(later.pilots[0].ore, stopped.pilots[0].ore);
    assert_eq!(later.ledger_hash, stopped.ledger_hash);
}

#[test]
fn rejected_deadline_command_rolls_back_then_tick_awards_exactly_once() {
    let mut s = Server::open(":memory:").unwrap();
    let (mut p, t, id) = setup(&mut s);
    let spec = mining_spec(ShipClass::Pioneer);
    ok(
        &mut s,
        &mut p,
        Request::StartMining {
            module_id: 0,
            target_id: id,
        },
        t,
    );
    // This executes after due-cycle advancement but must roll the transaction back.
    assert!(
        !s.handle_at(
            &mut p,
            Request::StopMining { module_id: 99 },
            t + spec.cycle_ms
        )
        .ok
    );
    s.tick_at(t + spec.cycle_ms).unwrap();
    let once = snap(&mut s, &mut p, t + spec.cycle_ms);
    assert_eq!(once.pilots[0].ore, spec.yield_units);
    assert_eq!(once.ledger_sequence, 2);
    for offset in 0..100 {
        assert_eq!(
            snap(&mut s, &mut p, t + spec.cycle_ms + offset).pilots[0].ore,
            spec.yield_units
        );
    }
    // A late-but-not-expired cycle awards once and schedules a whole new interval.
    let late = t + 3 * spec.cycle_ms - 1;
    let second = snap(&mut s, &mut p, late);
    assert_eq!(second.pilots[0].ore, 2 * spec.yield_units);
    assert_eq!(
        second.pilots[0].mining_modules[0].cycle_completes_at_ms,
        Some(late + spec.cycle_ms)
    );
    assert_eq!(
        snap(&mut s, &mut p, late + 1).pilots[0].ore,
        2 * spec.yield_units
    );
}
#[test]
fn boarding_resets_module_reasons_and_legacy_missing_slots_migrate_idle() {
    let path = std::env::temp_dir().join(format!(
        "arkline-module-migration-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let before;
    {
        let mut s = Server::open(&path).unwrap();
        let (mut p, t, id) = setup(&mut s);
        ok(
            &mut s,
            &mut p,
            Request::StartMining {
                module_id: 0,
                target_id: id,
            },
            t,
        );
        let warp = ok(
            &mut s,
            &mut p,
            Request::Warp {
                destination: Destination::Station,
            },
            t + 1,
        )
        .snapshot
        .unwrap();
        let Location::InTransit { arrives_at_ms, .. } = warp.pilots[0].location else {
            panic!()
        };
        ok(&mut s, &mut p, Request::Dock, arrives_at_ms);
        before = ok(
            &mut s,
            &mut p,
            Request::SelectShip {
                ship_class: ShipClass::Prospector,
            },
            arrives_at_ms,
        )
        .snapshot
        .unwrap();
        assert!(before.pilots[0]
            .mining_modules
            .iter()
            .all(|m| m.target_id.is_none() && m.stop_reason.is_none()));
    }
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        let json: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old["pilots"]["Miner"]["view"]
            .as_object_mut()
            .unwrap()
            .remove("mining_modules");
        db.execute("UPDATE world SET json=?1 WHERE id=1", [old.to_string()])
            .unwrap();
    }
    {
        let mut s = Server::open(&path).unwrap();
        let migrated = snap(&mut s, &mut None, 1_000_000);
        assert_eq!(migrated.pilots[0].mining_modules.len(), 2);
        assert!(migrated.pilots[0]
            .mining_modules
            .iter()
            .all(|m| m.target_id.is_none() && m.stop_reason.is_none()));
        assert_eq!(migrated.ledger_hash, before.ledger_hash);
        assert_eq!(migrated.pilots[0].ore, before.pilots[0].ore);
    }
    std::fs::remove_file(&path).unwrap();
    let mut lock = path.as_os_str().to_os_string();
    lock.push(".lock");
    std::fs::remove_file(lock).unwrap();
}
