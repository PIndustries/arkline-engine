//! Ordinary commands only: no seeded ore, teleports or database edits.
use arkline_protocol::*;
use arkline_server::Server;

fn call(s: &mut Server, p: &mut Option<String>, r: Request, now: u64) -> Response {
    let reply = s.handle_at(p, r, now);
    assert!(reply.ok, "{}", reply.message);
    reply
}
fn snap(s: &mut Server, now: u64) -> Snapshot {
    call(s, &mut None, Request::Snapshot, now).snapshot.unwrap()
}
#[test]
fn multi_trip_delivery_survives_takeover_disconnect_and_crash_without_economic_replay() {
    let path = std::env::temp_dir().join(format!(
        "arkline-loop-{}-{}.sqlite",
        std::process::id(),
        rand::random::<u64>()
    ));
    let mut s = Server::open(&path).unwrap();
    let mut session = None;
    let token = call(
        &mut s,
        &mut session,
        Request::Connect {
            supports_pioneer_interior: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "LoopMiner".into(),
            resume_token: None,
        },
        0,
    )
    .resume_token
    .unwrap();
    call(
        &mut s,
        &mut session,
        Request::StartResourceJob {
            pilot: None,
            target_units: 1200,
        },
        0,
    );
    let mut saw_full = false;
    let mut saw_mining_drone = false;
    let mut saw_two_turrets = false;
    let mut now = 0;
    for t in (100..=600_000).step_by(100) {
        now = t;
        s.tick_at(t).unwrap();
        let world = snap(&mut s, t);
        let pilot = &world.pilots[0];
        assert!(pilot.ore <= cargo_capacity(pilot.ship_class));
        saw_full |= pilot.ore == cargo_capacity(pilot.ship_class);
        saw_mining_drone |= pilot.drones.iter().any(|d| d.mining_cycle.is_some());
        saw_two_turrets |= pilot.mining_modules.len() == 2
            && pilot.mining_modules.iter().all(|m| m.target_id.is_some());
        let job = &world.resource_jobs[0];
        if job.phase.is_terminal() {
            assert_eq!(job.phase, ResourceJobPhase::Completed, "{}", job.message);
            break;
        }
    }
    let result = call(&mut s, &mut session, Request::ExportLedger, now);
    let world = result.snapshot.unwrap();
    let job = &world.resource_jobs[0];
    assert_eq!(job.phase, ResourceJobPhase::Completed);
    assert!(saw_full && saw_mining_drone && saw_two_turrets);
    assert!(job.stored_units >= 1200);
    assert_eq!(job.mined_units, job.stored_units);
    assert_eq!(world.pilots[0].ore, 0);
    assert_eq!(world.pilots[0].location, Location::Docked);
    let total = result
        .assets
        .unwrap()
        .storage
        .iter()
        .map(|s| s.ore)
        .sum::<u64>();
    assert_eq!(total, job.stored_units);
    let export = result.ledger.unwrap();
    let deposits = export
        .entries
        .iter()
        .filter(|e| matches!(e.event, EconomicEvent::PersonalOreDeposited { .. }))
        .count();
    assert!(
        deposits >= 2,
        "target greater than cargo capacity must require multiple real deliveries"
    );
    for units in [
        mining_spec(ShipClass::Pioneer).yield_units,
        DRONE_MINING_YIELD,
    ] {
        assert!(export.entries.iter().any(|e| matches!(e.event, EconomicEvent::OreMined { units: awarded, .. } if awarded == units)), "both turret and drone fixed-yield extraction must occur");
    }
    arkline_verifier::verify(&export, None).unwrap();
    let head = (
        export.snapshot.ledger_sequence,
        export.snapshot.ledger_hash.clone(),
    );
    // Manual successful mutation cancels the controller; no subsequent cycle award.
    call(
        &mut s,
        &mut session,
        Request::StartResourceJob {
            pilot: None,
            target_units: 100,
        },
        now,
    );
    call(&mut s, &mut session, Request::Undock, now);
    assert_eq!(
        snap(&mut s, now).resource_jobs[0].phase,
        ResourceJobPhase::Cancelled
    );
    call(&mut s, &mut session, Request::Dock, now);
    call(
        &mut s,
        &mut session,
        Request::StartResourceJob {
            pilot: None,
            target_units: 100,
        },
        now,
    );
    s.disconnect(&mut session);
    call(
        &mut s,
        &mut session,
        Request::Connect {
            supports_pioneer_interior: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "LoopMiner".into(),
            resume_token: Some(token.clone()),
        },
        now,
    );
    assert_eq!(
        snap(&mut s, now).resource_jobs[0].phase,
        ResourceJobPhase::Cancelled
    );
    // Drop/reopen with an active job, before graceful cancellation.
    call(
        &mut s,
        &mut session,
        Request::StartResourceJob {
            pilot: None,
            target_units: 100,
        },
        now,
    );
    drop(s);
    let mut s = Server::open(&path).unwrap();
    session = None;
    call(
        &mut s,
        &mut session,
        Request::Connect {
            supports_pioneer_interior: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "LoopMiner".into(),
            resume_token: Some(token),
        },
        now + 100,
    );
    let r = call(&mut s, &mut session, Request::ExportLedger, now + 100);
    let w = r.snapshot.unwrap();
    assert_eq!(w.resource_jobs[0].phase, ResourceJobPhase::Cancelled);
    assert_eq!((w.ledger_sequence, w.ledger_hash), head);
    assert_eq!(
        r.assets.unwrap().storage.iter().map(|s| s.ore).sum::<u64>(),
        total
    );
    arkline_verifier::verify(&r.ledger.unwrap(), None).unwrap();
    drop(s);
    for suffix in ["", ".lock", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn disconnect_during_active_extraction_stops_all_awards_and_resumes_owned_cargo() {
    let mut s = Server::open(":memory:").unwrap();
    let mut session = None;
    let token = call(
        &mut s,
        &mut session,
        Request::Connect {
            supports_pioneer_interior: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Interrupted".into(),
            resume_token: None,
        },
        0,
    )
    .resume_token
    .unwrap();
    call(
        &mut s,
        &mut session,
        Request::StartResourceJob {
            pilot: None,
            target_units: 1200,
        },
        0,
    );
    let mut now = 0;
    for t in (100..=120_000).step_by(100) {
        now = t;
        s.tick_at(t).unwrap();
        let w = snap(&mut s, t);
        if w.pilots[0].ore > 0 {
            break;
        }
    }
    let before = snap(&mut s, now);
    assert!(before.pilots[0].ore > 0);
    assert!(before.pilots[0]
        .mining_modules
        .iter()
        .any(|m| m.target_id.is_some()));
    s.disconnect(&mut session);
    s.tick_at(now + 30_000).unwrap();
    let after = snap(&mut s, now + 30_000);
    assert_eq!(after.pilots[0].ore, before.pilots[0].ore);
    assert_eq!(after.ledger_sequence, before.ledger_sequence);
    assert_eq!(after.resource_jobs[0].phase, ResourceJobPhase::Cancelled);
    assert!(after.pilots[0]
        .mining_modules
        .iter()
        .all(|m| m.target_id.is_none()));
    assert!(after.pilots[0].drones.is_empty());
    let denied = s.handle_at(
        &mut None,
        Request::Connect {
            supports_pioneer_interior: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Interrupted".into(),
            resume_token: Some("wrong-token".into()),
        },
        now + 30_000,
    );
    assert!(!denied.ok);
    let resumed = call(
        &mut s,
        &mut session,
        Request::Connect {
            supports_pioneer_interior: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Interrupted".into(),
            resume_token: Some(token),
        },
        now + 30_000,
    )
    .snapshot
    .unwrap();
    assert_eq!(resumed.pilots[0].ore, before.pilots[0].ore);
    let export = call(&mut s, &mut session, Request::ExportLedger, now + 30_000)
        .ledger
        .unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}
