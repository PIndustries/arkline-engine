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
            supports_fleet_hangar: false,
            supports_pioneer_interior: true,
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

fn ready_to_deposit() -> (Fixture, Server, Option<String>, u64, Snapshot) {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut pilot = connect(&mut s, "Worker");
    start(&mut s, &mut pilot, 100, 0);
    for t in (100..=240_000).step_by(100) {
        s.tick_at(t).unwrap();
        let snap = snapshot(&mut s, t);
        assert_ne!(snap.resource_jobs[0].phase, ResourceJobPhase::Blocked);
        if snap.resource_jobs[0].phase == ResourceJobPhase::Depositing {
            return (f, s, pilot, t, snap);
        }
    }
    panic!("job never reached deposit");
}
#[test]
fn failed_delivery_write_rolls_back_ore_ledger_and_progress_across_restart() {
    let (f, mut s, mut pilot, t, before) = ready_to_deposit();
    let ledger = ok(&mut s, &mut pilot, Request::ExportLedger, t)
        .ledger
        .unwrap();
    let db = rusqlite::Connection::open(&f.0).unwrap();
    // Fail precisely when the durable job counter would become nonzero. With
    // the old two-commit implementation this failed only after ore had moved.
    db.execute_batch(
        "CREATE TRIGGER fail_delivery BEFORE UPDATE ON world
        WHEN CAST(json_extract(NEW.json, '$.resource_jobs.Worker.view.stored_units') AS INTEGER) > 0
        BEGIN SELECT RAISE(ABORT, 'injected delivery write failure'); END;",
    )
    .unwrap();
    s.tick_at(t + 100).unwrap();
    let after = snapshot(&mut s, t + 100);
    assert_eq!(after.pilots[0].ore, before.pilots[0].ore);
    assert_eq!(after.stored_ore, before.stored_ore);
    assert_eq!(after.resource_jobs[0].stored_units, 0);
    assert_eq!(after.resource_jobs[0].phase, ResourceJobPhase::Blocked);
    assert!(after.resource_jobs[0]
        .message
        .contains("injected delivery write failure"));
    let exported = ok(&mut s, &mut pilot, Request::ExportLedger, t + 100)
        .ledger
        .unwrap();
    assert_eq!(exported.entries.len(), ledger.entries.len());
    arkline_verifier::verify(&exported, None).unwrap();
    db.execute_batch("DROP TRIGGER fail_delivery").unwrap();
    drop(db);
    drop(s);
    let mut s = Server::open(&f.0).unwrap();
    let recovered = snapshot(&mut s, t + 200);
    assert_eq!(recovered.pilots[0].ore, before.pilots[0].ore);
    assert_eq!(recovered.resource_jobs[0].stored_units, 0);
    assert_eq!(recovered.stored_ore, before.stored_ore);
}
#[test]
fn completed_delivery_and_job_counter_survive_reopening_together() {
    let (f, mut s, mut pilot, t, before) = ready_to_deposit();
    s.tick_at(t + 100).unwrap();
    let exported = ok(&mut s, &mut pilot, Request::ExportLedger, t + 100)
        .ledger
        .unwrap();
    arkline_verifier::verify(&exported, None).unwrap();
    drop(s);
    let mut s = Server::open(&f.0).unwrap();
    let recovered = snapshot(&mut s, t + 200);
    assert_eq!(recovered.pilots[0].ore, 0);
    assert_eq!(
        recovered.stored_ore,
        before.stored_ore + before.pilots[0].ore
    );
    assert_eq!(
        recovered.resource_jobs[0].stored_units,
        before.pilots[0].ore
    );
    assert_eq!(
        recovered.resource_jobs[0].phase,
        ResourceJobPhase::Completed
    );
}

#[test]
fn lost_mining_drone_blocks_job_with_explicit_reason() {
    let f = Fixture::new();
    let mut s = Server::open(&f.0).unwrap();
    let mut pilot = connect(&mut s, "Worker");
    start(&mut s, &mut pilot, 700, 0);
    for t in (100..=240_000).step_by(100) {
        s.tick_at(t).unwrap();
        let before = snapshot(&mut s, t);
        if before.pilots[0].drones.len() != 5 {
            continue;
        }
        // Destroy one non-economic drone in an isolated durable fixture.
        f.edit(|world| {
            world["pilots"]["Worker"]["view"]["drones"]
                .as_array_mut()
                .unwrap()
                .pop();
        });
        s.tick_at(t + 100).unwrap();
        let after = snapshot(&mut s, t + 100);
        assert_eq!(after.resource_jobs[0].phase, ResourceJobPhase::Blocked);
        assert!(after.resource_jobs[0].message.contains("lost a drone"));
        assert!(after.pilots[0].drones.len() <= 4); // Nearby survivors may finish recall immediately.
        assert!(after.pilots[0]
            .drones
            .iter()
            .all(|d| d.mode == DroneMode::Returning));
        assert!(after.pilots[0]
            .mining_modules
            .iter()
            .all(|m| m.target_id.is_none()));
        return;
    }
    panic!("job never launched mining fleet");
}
