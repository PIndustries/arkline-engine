use arkline_protocol::*;
use arkline_server::Server;
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "arkline-job-{}-{}.sqlite",
            std::process::id(),
            rand::random::<u64>()
        )))
    }
    fn edit(&self, f: impl FnOnce(&mut serde_json::Value)) {
        let db = rusqlite::Connection::open(&self.0).unwrap();
        let raw: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let mut w = serde_json::from_str(&raw).unwrap();
        f(&mut w);
        db.execute("UPDATE world SET json=?1 WHERE id=1", [w.to_string()])
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
        for s in [".lock", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{s}", self.0.display()));
        }
    }
}
fn ok(s: &mut Server, p: &mut Option<String>, r: Request, t: u64) -> Response {
    let r = s.handle_at(p, r, t);
    assert!(r.ok, "{}", r.message);
    r
}
fn connect(s: &mut Server, name: &str) -> Option<String> {
    let mut p = None;
    ok(
        s,
        &mut p,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: name.into(),
            resume_token: None,
        },
        0,
    );
    p
}
fn snapshot(s: &mut Server, t: u64) -> Snapshot {
    ok(s, &mut None, Request::Snapshot, t).snapshot.unwrap()
}
fn start(s: &mut Server, p: &mut Option<String>, target_units: u64, t: u64) {
    ok(
        s,
        p,
        Request::StartResourceJob {
            target_units,
            pilot: None,
        },
        t,
    );
}
#[test]
fn job_uses_real_turrets_drones_travel_and_storage_with_verifiable_ledger() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = connect(&mut s, "Worker");
    start(&mut s, &mut p, 700, 0);
    let mut saw_turrets = false;
    let mut saw_five_drones = false;
    let mut saw_drone_mining = false;
    let mut done = None;
    for t in (100..=240_000).step_by(100) {
        s.tick_at(t).unwrap();
        let snap = snapshot(&mut s, t);
        let ship = &snap.pilots[0];
        saw_turrets |= ship.mining_modules.iter().all(|m| m.target_id.is_some());
        saw_five_drones |= ship.drones.len() == 5;
        saw_drone_mining |= ship.drones.iter().any(|d| d.mining_cycle.is_some());
        if snap.resource_jobs[0].phase.is_terminal() {
            done = Some((t, snap));
            break;
        }
    }
    let (t, snap) = done.expect("bounded job completion");
    let j = &snap.resource_jobs[0];
    assert_eq!(j.phase, ResourceJobPhase::Completed, "{}", j.message);
    assert!(saw_turrets && saw_five_drones && saw_drone_mining);
    assert!(j.stored_units >= 700);
    assert_eq!(j.mined_units, j.stored_units);
    assert_eq!(snap.pilots[0].ore, 0);
    assert_eq!(snap.pilots[0].location, Location::Docked);
    assert!(snap.pilots[0].drones.is_empty());
    let response = ok(&mut s, &mut p, Request::ExportLedger, t);
    assert_eq!(
        response
            .assets
            .unwrap()
            .storage
            .iter()
            .map(|s| s.ore)
            .sum::<u64>(),
        j.stored_units
    );
    let ledger = response.ledger.unwrap();
    assert!(
        ledger.entries.iter().any(|e| matches!(
            e.event,
            EconomicEvent::OreMined {
                units: DRONE_MINING_YIELD,
                ..
            }
        )),
        "assigned drones must actually extract ore during this haul"
    );
    arkline_verifier::verify(&ledger, None).unwrap();
}
#[test]
fn jobs_require_permission_and_cancel_on_manual_takeover_disconnect_and_restart() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut owner = connect(&mut s, "Owner");
    let mut worker = connect(&mut s, "Worker");
    let mut stranger = connect(&mut s, "Stranger");
    assert!(
        !s.handle_at(
            &mut owner,
            Request::StartResourceJob {
                pilot: Some("Worker".into()),
                target_units: 100
            },
            0
        )
        .ok
    );
    ok(
        &mut s,
        &mut worker,
        Request::SetFleetController {
            controller: Some("Owner".into()),
        },
        0,
    );
    ok(
        &mut s,
        &mut owner,
        Request::StartResourceJob {
            pilot: Some("Worker".into()),
            target_units: 100,
        },
        0,
    );
    assert!(
        !s.handle_at(
            &mut stranger,
            Request::CancelResourceJob {
                pilot: Some("Worker".into())
            },
            0
        )
        .ok
    );
    s.disconnect(&mut worker);
    s.tick_at(100).unwrap();
    assert!(!snapshot(&mut s, 100).resource_jobs[0].phase.is_terminal());
    s.disconnect(&mut owner);
    assert_eq!(
        snapshot(&mut s, 100).resource_jobs[0].phase,
        ResourceJobPhase::Cancelled
    );
    // An independent self-owned job cancels on a successful manual mutation.
    start(&mut s, &mut stranger, 100, 100);
    ok(&mut s, &mut stranger, Request::Undock, 100);
    assert_eq!(
        snapshot(&mut s, 100)
            .resource_jobs
            .iter()
            .find(|j| j.pilot == "Stranger")
            .unwrap()
            .phase,
        ResourceJobPhase::Cancelled
    );
    ok(&mut s, &mut stranger, Request::Dock, 100);
    start(&mut s, &mut stranger, 100, 100);
    drop(s);
    let mut s = Server::open(&f.0).unwrap();
    assert!(snapshot(&mut s, 200)
        .resource_jobs
        .iter()
        .all(|j| j.phase == ResourceJobPhase::Cancelled));
}
#[test]
fn depleted_belt_returns_without_fabricating_ore_and_charge_wait_is_bounded() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = connect(&mut s, "Empty");
    f.edit(|w| {
        let asteroids = w["asteroids"].as_array_mut().unwrap();
        let mut removed = 0;
        for a in asteroids.iter_mut() {
            if a["system_id"].as_u64() == Some(HOME_SYSTEM_ID as u64) {
                removed += a["ore"].as_u64().unwrap();
                a["ore"] = 0.into();
            }
        }
        let other = asteroids
            .iter_mut()
            .find(|a| a["system_id"].as_u64() != Some(HOME_SYSTEM_ID as u64))
            .unwrap();
        other["ore"] = (other["ore"].as_u64().unwrap() + removed).into();
    });
    start(&mut s, &mut p, 50, 0);
    let mut done = None;
    for t in (100..=100_000).step_by(100) {
        s.tick_at(t).unwrap();
        let snap = snapshot(&mut s, t);
        if snap.resource_jobs[0].phase.is_terminal() {
            done = Some((t, snap));
            break;
        }
    }
    let (mut t, snap) = done.unwrap();
    assert_eq!(snap.resource_jobs[0].phase, ResourceJobPhase::Blocked);
    assert_eq!(snap.resource_jobs[0].stored_units, 0);
    assert_eq!(snap.ledger_sequence, 1);
    assert_eq!(snap.pilots[0].location, Location::Docked);
    f.edit(|w| {
        let power = &mut w["pilots"]["Empty"]["view"]["power"];
        power["charge_j"] = 0.into();
        power["updated_at_ms"] = u64::MAX.into();
    });
    start(&mut s, &mut p, 50, t);
    t += 100;
    s.tick_at(t).unwrap();
    t += 100;
    s.tick_at(t).unwrap();
    let waiting = snapshot(&mut s, t);
    assert!(waiting.resource_jobs[0].message.contains("recharge"));
    assert_eq!(waiting.pilots[0].location, Location::Station);
    t += 300_001;
    s.tick_at(t).unwrap();
    assert_eq!(
        snapshot(&mut s, t).resource_jobs[0].phase,
        ResourceJobPhase::Blocked
    );
}
#[test]
fn revocation_reconnect_explicit_cancel_and_admission_are_authoritative() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut boss = connect(&mut s, "Boss");
    let mut worker = None;
    let login = ok(
        &mut s,
        &mut worker,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Worker".into(),
            resume_token: None,
        },
        0,
    );
    let token = login.resume_token.unwrap();
    for target_units in [0, MAX_RESOURCE_JOB_UNITS + 1] {
        assert!(
            !s.handle_at(
                &mut worker,
                Request::StartResourceJob {
                    target_units,
                    pilot: None
                },
                0
            )
            .ok
        );
    }
    ok(
        &mut s,
        &mut worker,
        Request::SetFleetController {
            controller: Some("Boss".into()),
        },
        0,
    );
    let delegate = || Request::StartResourceJob {
        pilot: Some("Worker".into()),
        target_units: 50,
    };
    ok(&mut s, &mut boss, delegate(), 0);
    let first = snapshot(&mut s, 0).resource_jobs[0].id;
    assert!(!s.handle_at(&mut boss, delegate(), 0).ok);
    ok(
        &mut s,
        &mut worker,
        Request::SetFleetController { controller: None },
        0,
    );
    let snap = snapshot(&mut s, 0);
    assert!(snap.fleet_links.is_empty());
    assert_eq!(snap.resource_jobs[0].phase, ResourceJobPhase::Cancelled);
    assert!(!s.handle_at(&mut boss, delegate(), 0).ok);
    ok(
        &mut s,
        &mut worker,
        Request::SetFleetController {
            controller: Some("Boss".into()),
        },
        0,
    );
    ok(&mut s, &mut boss, delegate(), 0);
    let mut second_session = None;
    ok(
        &mut s,
        &mut second_session,
        Request::Connect {
            supports_room_controls: false,
            world_layout_version: 1,
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: false,
            protocol_version: PROTOCOL_VERSION,
            pilot: "Worker".into(),
            resume_token: Some(token),
        },
        0,
    );
    assert_eq!(
        snapshot(&mut s, 0).resource_jobs[0].phase,
        ResourceJobPhase::Cancelled
    );
    ok(&mut s, &mut boss, delegate(), 0);
    ok(
        &mut s,
        &mut boss,
        Request::CancelResourceJob {
            pilot: Some("Worker".into()),
        },
        0,
    );
    let snap = snapshot(&mut s, 0);
    assert!(snap.resource_jobs[0].id > first);
    assert_eq!(snap.resource_jobs[0].phase, ResourceJobPhase::Cancelled);
    assert!(
        !s.handle_at(
            &mut boss,
            Request::CancelResourceJob {
                pilot: Some("Worker".into())
            },
            0
        )
        .ok
    );
}
#[test]
fn two_delegated_workers_finish_parallel_hauls_with_only_controller_online() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut boss = connect(&mut s, "Director");
    for name in ["WorkerOne", "WorkerTwo"] {
        let mut worker = connect(&mut s, name);
        ok(
            &mut s,
            &mut worker,
            Request::SetFleetController {
                controller: Some("Director".into()),
            },
            0,
        );
        ok(
            &mut s,
            &mut boss,
            Request::StartResourceJob {
                pilot: Some(name.into()),
                target_units: 100,
            },
            0,
        );
        s.disconnect(&mut worker);
    }
    let mut completed = None;
    for t in (100..=180_000).step_by(100) {
        s.tick_at(t).unwrap();
        let snap = snapshot(&mut s, t);
        if snap.resource_jobs.iter().all(|j| j.phase.is_terminal()) {
            completed = Some((t, snap));
            break;
        }
    }
    let (t, snap) = completed.expect("both worker jobs terminate");
    assert_eq!(snap.online_pilots, vec!["Director"]);
    assert_eq!(snap.resource_jobs.len(), 2);
    for job in &snap.resource_jobs {
        assert_eq!(job.phase, ResourceJobPhase::Completed, "{}", job.message);
        assert_eq!(job.controller, "Director");
        assert!(job.stored_units >= 100);
    }
    assert!(snap
        .pilots
        .iter()
        .all(|p| p.ore == 0 && p.location == Location::Docked));
    let export = ok(&mut s, &mut boss, Request::ExportLedger, t)
        .ledger
        .unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}

#[test]
fn space_jobs_count_carried_cargo_but_only_report_new_extraction() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = connect(&mut s, "SpaceWorker");
    let mut now = 0;
    ok(&mut s, &mut p, Request::Undock, now);
    ok(
        &mut s,
        &mut p,
        Request::LaunchDroneClass {
            class: DroneClass::Recon,
        },
        now,
    );
    start(&mut s, &mut p, 150, now);
    assert!(snapshot(&mut s, now).pilots[0]
        .drones
        .iter()
        .all(|d| matches!(d.mode, DroneMode::Returning)));
    fn finish(s: &mut Server, now: &mut u64) -> ResourceJob {
        let deadline = *now + 240_000;
        while *now < deadline {
            *now += 100;
            s.tick_at(*now).unwrap();
            let snap = snapshot(s, *now);
            let j = &snap.resource_jobs[0];
            if j.phase.is_terminal() {
                assert_eq!(j.phase, ResourceJobPhase::Completed, "{}", j.message);
                return j.clone();
            }
        }
        panic!("job did not finish");
    }
    let first = finish(&mut s, &mut now);
    assert_eq!(first.mined_units, first.stored_units);
    ok(&mut s, &mut p, Request::WithdrawOre { units: 50 }, now);
    ok(&mut s, &mut p, Request::Undock, now);
    let warp = ok(
        &mut s,
        &mut p,
        Request::Warp {
            destination: Destination::Belt,
        },
        now,
    );
    start(&mut s, &mut p, 100, now);
    s.tick_at(now + 100).unwrap();
    let queued = snapshot(&mut s, now + 100);
    assert!(!queued.resource_jobs[0].phase.is_terminal());
    assert!(matches!(
        queued.pilots[0].location,
        Location::InTransit { .. }
    ));
    let arrival = match warp.snapshot.unwrap().pilots[0].location {
        Location::InTransit { arrives_at_ms, .. } => arrives_at_ms,
        _ => panic!("expected warp"),
    };
    now = arrival;
    s.tick_at(now).unwrap();
    let partial = finish(&mut s, &mut now);
    assert!(partial.mined_units > 0);
    assert_eq!(partial.stored_units, partial.mined_units + 50);
    ok(&mut s, &mut p, Request::WithdrawOre { units: 100 }, now);
    ok(&mut s, &mut p, Request::Undock, now);
    start(&mut s, &mut p, 50, now);
    let carried = finish(&mut s, &mut now);
    assert_eq!(carried.mined_units, 0);
    assert_eq!(carried.stored_units, 100);
    ok(&mut s, &mut p, Request::WithdrawOre { units: 50 }, now);
    start(&mut s, &mut p, 50, now);
    let docked = finish(&mut s, &mut now);
    assert_eq!(docked.mined_units, 0);
    assert_eq!(docked.stored_units, 50);
    let ledger = ok(&mut s, &mut p, Request::ExportLedger, now)
        .ledger
        .unwrap();
    arkline_verifier::verify(&ledger, None).unwrap();
}

#[test]
fn four_thousand_goal_repeats_real_hauls_with_cumulative_progress() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut p = connect(&mut s, "Hauler");
    assert!(
        !s.handle_at(
            &mut p,
            Request::StartResourceJob {
                target_units: MAX_RESOURCE_JOB_UNITS + 1,
                pilot: None
            },
            0
        )
        .ok
    );
    start(&mut s, &mut p, 4000, 0);
    let mut previous_stored = 0;
    let mut previous_mined = 0;
    let mut deliveries = 0;
    let mut done = None;
    for t in (100..=900_000).step_by(100) {
        s.tick_at(t).unwrap();
        let snap = snapshot(&mut s, t);
        let j = &snap.resource_jobs[0];
        assert!(j.stored_units >= previous_stored);
        assert!(j.mined_units >= previous_mined);
        assert!(snap.pilots[0].ore <= cargo_capacity(ShipClass::Pioneer));
        if j.stored_units > previous_stored {
            assert!(j.stored_units - previous_stored <= cargo_capacity(ShipClass::Pioneer));
            deliveries += 1;
        }
        previous_stored = j.stored_units;
        previous_mined = j.mined_units;
        if j.phase.is_terminal() {
            assert_eq!(j.phase, ResourceJobPhase::Completed, "{}", j.message);
            assert!(j.stored_units >= 4000);
            assert_eq!(j.mined_units, j.stored_units);
            assert_eq!(snap.pilots[0].location, Location::Docked);
            assert_eq!(snap.pilots[0].ore, 0);
            done = Some(t);
            break;
        }
    }
    let t = done.expect("bounded multi-haul completion");
    assert_eq!(deliveries, 4);
    let result = ok(&mut s, &mut p, Request::ExportLedger, t);
    assert_eq!(
        result
            .assets
            .unwrap()
            .storage
            .iter()
            .map(|a| a.ore)
            .sum::<u64>(),
        previous_stored
    );
    arkline_verifier::verify(&result.ledger.unwrap(), None).unwrap();
}
