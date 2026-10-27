//! Real commands, disposable saved worlds and independent economic replay.
use arkline_protocol::*;
use arkline_server::editor::EditorWorld;
use std::{path::PathBuf, sync::OnceLock};
fn directory() -> PathBuf {
    std::env::temp_dir().join(format!(
        "arkline-unload-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ))
}
fn ok(w: &mut EditorWorld, s: &mut Option<String>, r: Request) -> Response {
    let r = w.handle(s, r);
    assert!(r.ok, "{}", r.message);
    r
}
fn connect(
    w: &mut EditorWorld,
    s: &mut Option<String>,
    name: &str,
    token: Option<String>,
) -> Response {
    ok(
        w,
        s,
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            supports_room_controls: true,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: true,
            pilot: name.into(),
            resume_token: token,
        },
    )
}
fn advance(w: &mut EditorWorld, ms: u64) {
    for _ in 0..ms / 50 {
        w.advance(50).unwrap();
    }
}
fn snap(w: &mut EditorWorld, s: &mut Option<String>) -> Response {
    ok(w, s, Request::Snapshot)
}
fn operation(r: &Response) -> &CargoUnloadOperation {
    r.assets
        .as_ref()
        .unwrap()
        .cargo_unloading
        .as_ref()
        .unwrap()
        .latest()
        .unwrap()
}
fn start(w: &mut EditorWorld, s: &mut Option<String>, id: u64, units: u64) -> Response {
    ok(
        w,
        s,
        Request::StartCargoUnload {
            operation_id: id,
            units,
            destination: CargoUnloadDestination::Personal,
        },
    )
}
struct Seed {
    bytes: Vec<u8>,
    token: String,
}
fn seeded() -> (EditorWorld, Option<String>, PathBuf, String) {
    static SEED: OnceLock<Seed> = OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let d = directory();
        let mut w =
            EditorWorld::open_with_creation_scene(&d, SceneLayoutRevision::DockingV1).unwrap();
        let mut s = None;
        let token = connect(&mut w, &mut s, "Pilot", None).resume_token.unwrap();
        w.play();
        ok(
            &mut w,
            &mut s,
            Request::StartResourceJob {
                target_units: 100,
                pilot: None,
            },
        );
        let mut ready = false;
        // Fresh registered worlds now launch/recover five real stored miners
        // serially. Observe the actual routine, not the former instant fleet.
        for _ in 0..24000 {
            advance(&mut w, 50);
            let r = snap(&mut w, &mut s);
            let j = &r.snapshot.as_ref().unwrap().resource_jobs[0];
            assert_ne!(j.phase, ResourceJobPhase::Blocked, "{}", j.message);
            if j.phase == ResourceJobPhase::Depositing {
                ready = true;
                break;
            }
        }
        assert!(ready);
        ok(&mut w, &mut s, Request::CancelResourceJob { pilot: None });
        let r = snap(&mut w, &mut s);
        assert!(r.snapshot.as_ref().unwrap().pilots[0].ore >= 100);
        let l = ok(&mut w, &mut s, Request::ExportLedger).ledger.unwrap();
        arkline_verifier::verify(&l, None).unwrap();
        drop(w);
        let bytes = std::fs::read(d.join("world.sqlite")).unwrap();
        Seed { bytes, token }
    });
    let d = directory();
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join("world.sqlite");
    std::fs::write(&path, &seed.bytes).unwrap();
    let mut w = EditorWorld::open_with_creation_scene(&d, SceneLayoutRevision::DockingV1).unwrap();
    let mut s = None;
    connect(&mut w, &mut s, "Pilot", Some(seed.token.clone()));
    w.play();
    (w, s, d, seed.token.clone())
}
fn ledger(w: &mut EditorWorld, s: &mut Option<String>) -> LedgerExport {
    let l = ok(w, s, Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&l, None).unwrap();
    l
}
fn db(d: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(d.join("world.sqlite")).unwrap()
}
fn durable(db: &rusqlite::Connection) -> (String, u64) {
    (
        db.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap(),
        db.query_row("SELECT count(*) FROM ledger", [], |r| r.get(0))
            .unwrap(),
    )
}
#[test]
fn confirmed_partial_batches_receipts_and_duplicate_ids_conserve_ore() {
    let (mut w, mut s, _d, _) = seeded();
    let before = snap(&mut w, &mut s);
    let ore = before.snapshot.as_ref().unwrap().pilots[0].ore;
    let r = start(&mut w, &mut s, 1, 57);
    assert_eq!(operation(&r).committed_units, 0);
    assert_eq!(operation(&r).reserved_units, 57);
    advance(&mut w, 450);
    assert_eq!(operation(&snap(&mut w, &mut s)).committed_units, 0);
    for expected in [25, 50, 57] {
        advance(&mut w, if expected == 25 { 50 } else { 500 });
        let r = snap(&mut w, &mut s);
        assert_eq!(operation(&r).committed_units, expected);
        assert_eq!(r.snapshot.as_ref().unwrap().pilots[0].ore, ore - expected);
        let retry = start(&mut w, &mut s, 1, 57);
        assert_eq!(operation(&retry), operation(&r));
    }
    let r = snap(&mut w, &mut s);
    let op = operation(&r);
    assert_eq!(op.phase, CargoUnloadPhase::Completed);
    assert_eq!(
        op.receipts.iter().map(|r| r.units).collect::<Vec<_>>(),
        [25, 25, 7]
    );
    let l = ledger(&mut w, &mut s);
    for receipt in &op.receipts {
        let e = l
            .entries
            .iter()
            .find(|e| e.sequence == receipt.ledger_sequence)
            .unwrap();
        assert_eq!(e.hash, receipt.ledger_hash);
        assert!(
            matches!(e.event,EconomicEvent::PersonalOreDeposited{units,..} if units==receipt.units)
        );
    }
    assert!(
        !w.handle(
            &mut s,
            Request::StartCargoUnload {
                operation_id: 1,
                units: 58,
                destination: CargoUnloadDestination::Personal
            }
        )
        .ok
    );
    assert!(
        !w.handle(
            &mut s,
            Request::StartCargoUnload {
                operation_id: 3,
                units: 1,
                destination: CargoUnloadDestination::Personal
            }
        )
        .ok
    );
}
#[test]
fn reservation_blocks_competing_mutations_cancel_keeps_only_committed_ore() {
    let (mut w, mut s, _d, _) = seeded();
    start(&mut w, &mut s, 1, 57);
    advance(&mut w, 500);
    for request in [
        Request::Undock,
        Request::DepositOre { units: 1 },
        Request::WithdrawOre { units: 1 },
        Request::PlaceSell {
            units: 1,
            unit_price: 1,
        },
        Request::Buy {
            order_id: 1,
            units: 1,
        },
        Request::CancelSell { order_id: 1 },
        Request::StartResourceJob {
            target_units: 1,
            pilot: None,
        },
        Request::SelectShip {
            ship_class: ShipClass::Hauler,
        },
    ] {
        assert!(!w.handle(&mut s, request).ok);
    }
    let r = ok(
        &mut w,
        &mut s,
        Request::CancelCargoUnload { operation_id: 1 },
    );
    assert_eq!(operation(&r).committed_units, 25);
    assert_eq!(operation(&r).reserved_units, 0);
    assert_eq!(operation(&r).phase, CargoUnloadPhase::Cancelled);
    advance(&mut w, 2000);
    assert_eq!(operation(&start(&mut w, &mut s, 1, 57)).committed_units, 25);
    assert!(
        !w.handle(&mut s, Request::ResumeCargoUnload { operation_id: 1 })
            .ok
    );
    ok(
        &mut w,
        &mut s,
        Request::CancelCargoUnload { operation_id: 1 },
    );
    ok(&mut w, &mut s, Request::WithdrawOre { units: 25 });
    ledger(&mut w, &mut s);
}
#[test]
fn disconnect_and_reopen_hold_without_offline_catchup_or_retry_duplication() {
    let (mut w, mut s, d, token) = seeded();
    start(&mut w, &mut s, 1, 57);
    advance(&mut w, 500);
    w.disconnect(&mut s);
    advance(&mut w, 2000);
    connect(&mut w, &mut s, "Pilot", Some(token.clone()));
    let r = snap(&mut w, &mut s);
    assert_eq!(operation(&r).phase, CargoUnloadPhase::Held);
    assert_eq!(operation(&r).committed_units, 25);
    ok(
        &mut w,
        &mut s,
        Request::ResumeCargoUnload { operation_id: 1 },
    );
    advance(&mut w, 450);
    let due = operation(&snap(&mut w, &mut s)).next_batch_at_ms;
    ok(
        &mut w,
        &mut s,
        Request::ResumeCargoUnload { operation_id: 1 },
    );
    assert_eq!(operation(&snap(&mut w, &mut s)).next_batch_at_ms, due);
    advance(&mut w, 50);
    drop(w);
    let mut w = EditorWorld::open_with_creation_scene(&d, SceneLayoutRevision::DockingV1).unwrap();
    let mut s = None;
    connect(&mut w, &mut s, "Pilot", Some(token));
    w.play();
    advance(&mut w, 1000);
    let retry = start(&mut w, &mut s, 1, 57);
    assert_eq!(operation(&retry).committed_units, 50);
    assert_eq!(operation(&retry).phase, CargoUnloadPhase::Held);
    ok(
        &mut w,
        &mut s,
        Request::ResumeCargoUnload { operation_id: 1 },
    );
    advance(&mut w, 500);
    assert_eq!(operation(&snap(&mut w, &mut s)).committed_units, 57);
    ledger(&mut w, &mut s);
}
#[test]
fn ledger_append_then_failed_world_write_rolls_back_tick_batch_and_retry() {
    let (mut w, mut s, d, _) = seeded();
    start(&mut w, &mut s, 1, 57);
    advance(&mut w, 450);
    let c = db(&d);
    let before = durable(&c);
    c.execute_batch("CREATE TRIGGER reject_unload BEFORE UPDATE ON world WHEN CAST(json_extract(NEW.json,'$.cargo_unloading.Pilot.operations[0].committed_units') AS INTEGER)>0 BEGIN SELECT RAISE(ABORT,'injected unload world failure'); END;").unwrap();
    assert!(w.advance(50).is_err());
    assert_eq!(durable(&c), before);
    c.execute_batch("DROP TRIGGER reject_unload").unwrap();
    w.play();
    w.advance(50).unwrap();
    assert_eq!(operation(&snap(&mut w, &mut s)).committed_units, 25);
    ledger(&mut w, &mut s);
}
#[test]
fn request_driven_batch_fault_rolls_back_exact_world_and_ledger() {
    let (mut w, mut s, d, _) = seeded();
    start(&mut w, &mut s, 1, 57);
    let c = db(&d);
    // Make the next batch due at the current editor clock, without changing any economic value.
    c.execute("UPDATE world SET json=json_set(json,'$.cargo_unloading.Pilot.operations[0].next_batch_at_ms',0) WHERE id=1",[]).unwrap();
    let before = durable(&c);
    c.execute_batch("CREATE TRIGGER reject_unload BEFORE UPDATE ON world WHEN CAST(json_extract(NEW.json,'$.cargo_unloading.Pilot.operations[0].committed_units') AS INTEGER)>0 BEGIN SELECT RAISE(ABORT,'injected unload request failure'); END;").unwrap();
    assert!(!w.handle(&mut s, Request::Snapshot).ok);
    assert_eq!(durable(&c), before);
    c.execute_batch("DROP TRIGGER reject_unload").unwrap();
    assert_eq!(operation(&snap(&mut w, &mut s)).committed_units, 25);
    assert_eq!(operation(&snap(&mut w, &mut s)).committed_units, 25);
    ledger(&mut w, &mut s);
}
#[test]
fn private_owner_capability_quantity_and_secured_checks_are_enforced() {
    let (mut w, mut s, _d, _) = seeded();
    assert!(
        !w.handle(
            &mut s,
            Request::StartCargoUnload {
                operation_id: 1,
                units: 0,
                destination: CargoUnloadDestination::Personal
            }
        )
        .ok
    );
    assert!(
        !w.handle(
            &mut s,
            Request::StartCargoUnload {
                operation_id: 1,
                units: u64::MAX,
                destination: CargoUnloadDestination::Personal
            }
        )
        .ok
    );
    assert!(
        !w.handle(
            &mut s,
            Request::StartCargoUnload {
                operation_id: 1,
                units: 1,
                destination: CargoUnloadDestination::Corporation
            }
        )
        .ok
    );
    start(&mut w, &mut s, 1, 57);
    let mut other = None;
    let r = connect(&mut w, &mut other, "Other", None);
    assert!(r
        .assets
        .unwrap()
        .cargo_unloading
        .unwrap()
        .operations
        .is_empty());
    assert!(
        !w.handle(&mut other, Request::CancelCargoUnload { operation_id: 1 })
            .ok
    );
    let public = snap(&mut w, &mut None);
    assert!(public.assets.is_none());
    ok(
        &mut w,
        &mut s,
        Request::CancelCargoUnload { operation_id: 1 },
    );
    ok(&mut w, &mut s, Request::Undock);
    assert!(
        !w.handle(
            &mut s,
            Request::StartCargoUnload {
                operation_id: 2,
                units: 1,
                destination: CargoUnloadDestination::Personal
            }
        )
        .ok
    );
    let d = directory();
    let mut old = EditorWorld::open(&d).unwrap();
    let mut p = None;
    let r = connect(&mut old, &mut p, "Legacy", None);
    assert!(r.assets.unwrap().cargo_unloading.is_none());
    assert!(
        !old.handle(
            &mut p,
            Request::StartCargoUnload {
                operation_id: 1,
                units: 1,
                destination: CargoUnloadDestination::Personal
            }
        )
        .ok
    );
}
#[test]
fn corporation_destination_lock_and_original_identity_survive_partial_commit() {
    let (mut w, mut s, _d, _) = seeded();
    ok(
        &mut w,
        &mut s,
        Request::CreateCorporation {
            name: "Dockers".into(),
        },
    );
    let mut other = None;
    connect(&mut w, &mut other, "Other", None);
    ok(
        &mut w,
        &mut s,
        Request::InviteCorporation {
            pilot: "Other".into(),
        },
    );
    ok(
        &mut w,
        &mut other,
        Request::AcceptCorporation {
            corporation: "Dockers".into(),
        },
    );
    ok(
        &mut w,
        &mut s,
        Request::StartCargoUnload {
            operation_id: 1,
            units: 57,
            destination: CargoUnloadDestination::Corporation,
        },
    );
    advance(&mut w, 500);
    assert!(
        !w.handle(&mut other, Request::DepositCorporationOre { units: 1 })
            .ok
    );
    assert!(
        !w.handle(&mut other, Request::WithdrawCorporationOre { units: 1 })
            .ok
    );
    advance(&mut w, 1000);
    let r = snap(&mut w, &mut s);
    assert_eq!(operation(&r).destination_owner, "Dockers");
    assert_eq!(r.assets.unwrap().corporation.unwrap().storage[0].ore, 57);
    ledger(&mut w, &mut s);
}
#[test]
fn job_batches_count_once_and_manual_takeover_holds_the_remainder() {
    let (mut w, mut s, _d, _) = seeded();
    ok(
        &mut w,
        &mut s,
        Request::StartResourceJob {
            target_units: 57,
            pilot: None,
        },
    );
    advance(&mut w, 50);
    advance(&mut w, 500);
    let r = snap(&mut w, &mut s);
    assert_eq!(operation(&r).committed_units, 25);
    assert_eq!(r.snapshot.unwrap().resource_jobs[0].stored_units, 25);
    ok(
        &mut w,
        &mut s,
        Request::CreateCorporation {
            name: "ManualTakeover".into(),
        },
    );
    advance(&mut w, 1000);
    let r = snap(&mut w, &mut s);
    assert_eq!(operation(&r).phase, CargoUnloadPhase::Held);
    assert_eq!(operation(&r).committed_units, 25);
    assert_eq!(
        r.snapshot.unwrap().resource_jobs[0].phase,
        ResourceJobPhase::Cancelled
    );
    ok(
        &mut w,
        &mut s,
        Request::ResumeCargoUnload { operation_id: 1 },
    );
    advance(&mut w, 3000);
    let r = snap(&mut w, &mut s);
    assert_eq!(operation(&r).phase, CargoUnloadPhase::Completed);
    assert_eq!(r.snapshot.unwrap().resource_jobs[0].stored_units, 25);
    ledger(&mut w, &mut s);
}
#[test]
fn job_inventory_receipt_ledger_and_delivery_counter_commit_together() {
    let (mut w, mut s, d, _) = seeded();
    ok(
        &mut w,
        &mut s,
        Request::StartResourceJob {
            target_units: 57,
            pilot: None,
        },
    );
    advance(&mut w, 50);
    advance(&mut w, 450);
    let c = db(&d);
    let before = durable(&c);
    c.execute_batch("CREATE TRIGGER reject_delivery BEFORE UPDATE ON world WHEN CAST(json_extract(NEW.json,'$.resource_jobs.Pilot.view.stored_units') AS INTEGER)>0 BEGIN SELECT RAISE(ABORT,'injected batch counter failure'); END;").unwrap();
    assert!(w.advance(50).is_err());
    assert_eq!(durable(&c), before);
    c.execute_batch("DROP TRIGGER reject_delivery").unwrap();
    w.play();
    w.advance(50).unwrap();
    advance(&mut w, 3000);
    let r = snap(&mut w, &mut s);
    let op = operation(&r).clone();
    assert_eq!(op.phase, CargoUnloadPhase::Completed);
    let j = &r.snapshot.as_ref().unwrap().resource_jobs[0];
    assert_eq!(j.stored_units, op.committed_units);
    assert_eq!(j.phase, ResourceJobPhase::Completed);
    let retry = start(&mut w, &mut s, op.operation_id, op.requested_units);
    assert_eq!(operation(&retry), &op);
    ledger(&mut w, &mut s);
}
#[test]
fn destination_overflow_rejects_before_reservation_and_later_revocation_holds() {
    let (mut w, mut s, d, _) = seeded();
    let c = db(&d);
    let r = snap(&mut w, &mut s);
    let system = r.snapshot.unwrap().pilots[0].system_id;
    let (mut raw, _) = durable(&c);
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    value["accounts"]["Pilot"]["storage"][system.to_string()] = serde_json::json!(u64::MAX);
    raw = value.to_string();
    c.execute("UPDATE world SET json=?1 WHERE id=1", [raw])
        .unwrap();
    assert!(
        !w.handle(
            &mut s,
            Request::StartCargoUnload {
                operation_id: 1,
                units: 1,
                destination: CargoUnloadDestination::Personal
            }
        )
        .ok
    );
    // Restore this deliberately adverse fixture before checking real economic replay.
    value["accounts"]["Pilot"]["storage"] = serde_json::json!({});
    c.execute("UPDATE world SET json=?1 WHERE id=1", [value.to_string()])
        .unwrap();
    ok(
        &mut w,
        &mut s,
        Request::CreateCorporation {
            name: "Dockers".into(),
        },
    );
    ok(
        &mut w,
        &mut s,
        Request::StartCargoUnload {
            operation_id: 1,
            units: 57,
            destination: CargoUnloadDestination::Corporation,
        },
    );
    advance(&mut w, 500);
    // A future membership-revocation path must not redirect the reserved operation.
    let (raw, _) = durable(&c);
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    value["accounts"]["Pilot"]["corporation"] = serde_json::Value::Null;
    value["corporations"]
        .as_object_mut()
        .unwrap()
        .remove("Dockers");
    // Keep the committed25units in owned personal storage so total ore remains conserved.
    value["accounts"]["Pilot"]["storage"][system.to_string()] = serde_json::json!(25);
    c.execute("UPDATE world SET json=?1 WHERE id=1", [value.to_string()])
        .unwrap();
    advance(&mut w, 500);
    let r = snap(&mut w, &mut s);
    assert_eq!(operation(&r).phase, CargoUnloadPhase::Held);
    assert_eq!(operation(&r).committed_units, 25);
    assert!(
        !w.handle(&mut s, Request::ResumeCargoUnload { operation_id: 1 })
            .ok
    );
    // This synthetic access-removal fixture is not a ledger-backed corporate policy change.
}

/// Real 10Hz authority responses for native playback; no client-side awards.
#[test]
#[ignore = "writes actual authority capture input when explicitly requested"]
fn export_timed_unloading_motion_responses() {
    let output =
        std::env::var("ARKLINE_UNLOAD_CAPTURE_DIR").expect("explicit output directory required");
    let output = PathBuf::from(output);
    std::fs::create_dir_all(&output).unwrap();
    let (mut w, mut s, _d, token) = seeded();
    let mut manifest = vec![];
    let source = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    for frame in 0..131 {
        let action = match frame {
            10 => {
                start(&mut w, &mut s, 1, 100);
                "start_100"
            }
            22 => {
                w.disconnect(&mut s);
                "disconnect"
            }
            32 => {
                connect(&mut w, &mut s, "Pilot", Some(token.clone()));
                "reconnect_held"
            }
            40 => {
                ok(
                    &mut w,
                    &mut s,
                    Request::ResumeCargoUnload { operation_id: 1 },
                );
                "resume"
            }
            70 => {
                ok(&mut w, &mut s, Request::WithdrawOre { units: 57 });
                "load_57_legacy_loading"
            }
            80 => {
                start(&mut w, &mut s, 2, 57);
                "start_57"
            }
            86 => {
                ok(
                    &mut w,
                    &mut s,
                    Request::CancelCargoUnload { operation_id: 2 },
                );
                "cancel_remaining"
            }
            90 => {
                start(&mut w, &mut s, 2, 57);
                "retry_cancelled_noop"
            }
            _ => "snapshot",
        };
        // Offline frames use an unauthenticated public snapshot: do not synthesize private receipts.
        let response = snap(&mut w, &mut s);
        let file = format!("response-{frame:04}.json");
        std::fs::write(
            output.join(&file),
            serde_json::to_vec_pretty(&response).unwrap(),
        )
        .unwrap();
        manifest.push(serde_json::json!({"frame":frame,"action":action,"file":file,"server_ms":response.snapshot.as_ref().unwrap().server_time_ms,"connected":s.is_some()}));
        advance(&mut w, 100);
    }
    let l = ledger(&mut w, &mut s);
    std::fs::write(
        output.join("ledger.json"),
        serde_json::to_vec_pretty(&l).unwrap(),
    )
    .unwrap();
    std::fs::write(output.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"source_commit":String::from_utf8(source.stdout.clone()).unwrap().trim(),"cadence_ms":100,"frames":manifest,"description":"Actual EditorWorld responses; confirmed generic ore batches, not physical crane animation. Loading remains immediate. Offline responses contain no private asset view."})).unwrap()).unwrap();
    // Honest before comparison: retained OrbitalV1 immediate deposit path in this
    // same modified binary, not an invented old executable or interpolated numbers.
    let before_dir = output.join("before-retained-legacy");
    std::fs::create_dir_all(&before_dir).unwrap();
    let d = directory();
    let mut before = EditorWorld::open(&d).unwrap();
    let mut pilot = None;
    connect(&mut before, &mut pilot, "BeforePilot", None);
    before.play();
    ok(
        &mut before,
        &mut pilot,
        Request::StartResourceJob {
            target_units: 100,
            pilot: None,
        },
    );
    let mut ready = false;
    for _ in 0..6000 {
        advance(&mut before, 50);
        let r = snap(&mut before, &mut pilot);
        let j = &r.snapshot.as_ref().unwrap().resource_jobs[0];
        assert_ne!(j.phase, ResourceJobPhase::Blocked, "{}", j.message);
        if j.phase == ResourceJobPhase::Depositing {
            ready = true;
            break;
        }
    }
    assert!(ready);
    ok(
        &mut before,
        &mut pilot,
        Request::CancelResourceJob { pilot: None },
    );
    let mut frames = vec![];
    for frame in 0..31 {
        if frame == 10 {
            ok(&mut before, &mut pilot, Request::DepositOre { units: 100 });
        }
        let r = snap(&mut before, &mut pilot);
        let file = format!("response-{frame:04}.json");
        std::fs::write(
            before_dir.join(&file),
            serde_json::to_vec_pretty(&r).unwrap(),
        )
        .unwrap();
        frames.push(serde_json::json!({ "frame":frame, "file":file, "server_ms":r.snapshot.as_ref().unwrap().server_time_ms, "connected":true, "action":if frame==10 {"legacy_instant_deposit_100"} else {"snapshot"} }));
        advance(&mut before, 100);
    }
    let l = ledger(&mut before, &mut pilot);
    std::fs::write(
        before_dir.join("ledger.json"),
        serde_json::to_vec_pretty(&l).unwrap(),
    )
    .unwrap();
    std::fs::write(before_dir.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"source_commit":String::from_utf8(source.stdout.clone()).unwrap().trim(),"cadence_ms":100,"frames":frames,"description":"Retained legacy OrbitalV1 instant unload path in the same modified binary; not an exact baseline executable."})).unwrap()).unwrap();
}
#[test]
#[ignore = "subprocess crash fixture; invoked by the parent test only"]
fn crash_after_unload_commit_child() {
    let Ok(path) = std::env::var("ARKLINE_UNLOAD_CRASH_WORLD") else {
        return;
    };
    let token = std::env::var("ARKLINE_UNLOAD_CRASH_TOKEN").unwrap();
    let mut w =
        EditorWorld::open_with_creation_scene(&path, SceneLayoutRevision::DockingV1).unwrap();
    let mut s = None;
    connect(&mut w, &mut s, "Pilot", Some(token));
    w.play();
    start(&mut w, &mut s, 1, 57);
    advance(&mut w, 500);
    assert_eq!(operation(&snap(&mut w, &mut s)).committed_units, 25);
    // Exit without Drop/checkpoint/normal disconnect. WAL recovery must retain only
    // the already committed inventory, ledger entry and receipt.
    std::process::exit(0);
}
#[test]
fn abrupt_exit_after_commit_reopens_as_held_and_lost_response_retry_is_exact() {
    let (w, _s, d, token) = seeded();
    drop(w);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "crash_after_unload_commit_child",
            "--ignored",
            "--nocapture",
        ])
        .env("ARKLINE_UNLOAD_CRASH_WORLD", &d)
        .env("ARKLINE_UNLOAD_CRASH_TOKEN", &token)
        .status()
        .unwrap();
    assert!(child.success());
    let mut w = EditorWorld::open_with_creation_scene(&d, SceneLayoutRevision::DockingV1).unwrap();
    let mut s = None;
    connect(&mut w, &mut s, "Pilot", Some(token));
    w.play();
    advance(&mut w, 1000);
    let r = start(&mut w, &mut s, 1, 57);
    assert_eq!(operation(&r).committed_units, 25);
    assert_eq!(operation(&r).receipts.len(), 1);
    assert_eq!(operation(&r).phase, CargoUnloadPhase::Held);
    ledger(&mut w, &mut s);
}
#[test]
fn completed_operation_retry_does_not_cancel_a_new_resource_job() {
    let (mut w, mut s, _d, _) = seeded();
    start(&mut w, &mut s, 1, 25);
    advance(&mut w, 500);
    let receipt = operation(&snap(&mut w, &mut s)).clone();
    ok(
        &mut w,
        &mut s,
        Request::StartResourceJob {
            target_units: 1,
            pilot: None,
        },
    );
    let r = start(&mut w, &mut s, 1, 25);
    assert_eq!(operation(&r), &receipt);
    assert!(!r.snapshot.unwrap().resource_jobs[0].phase.is_terminal());
    ok(
        &mut w,
        &mut s,
        Request::CancelCargoUnload { operation_id: 1 },
    );
    assert!(!snap(&mut w, &mut s).snapshot.unwrap().resource_jobs[0]
        .phase
        .is_terminal());
}
#[test]
fn second_authenticated_session_retry_cannot_duplicate_an_existing_batch() {
    let (mut w, mut first, _d, token) = seeded();
    start(&mut w, &mut first, 1, 57);
    advance(&mut w, 500);
    let mut second = None;
    connect(&mut w, &mut second, "Pilot", Some(token));
    assert_eq!(
        operation(&start(&mut w, &mut second, 1, 57)).committed_units,
        25
    );
    assert_eq!(
        operation(&snap(&mut w, &mut first)).phase,
        CargoUnloadPhase::Held
    );
    ok(
        &mut w,
        &mut second,
        Request::ResumeCargoUnload { operation_id: 1 },
    );
    advance(&mut w, 500);
    assert_eq!(
        operation(&start(&mut w, &mut first, 1, 57)).committed_units,
        50
    );
    assert_eq!(
        operation(&start(&mut w, &mut second, 1, 57)).receipts.len(),
        2
    );
    ledger(&mut w, &mut first);
}
#[test]
fn last_source_disconnect_holds_unload_even_with_an_online_flight_delegate() {
    let (mut w, mut pilot, d, _token) = seeded();
    let mut controller = None;
    connect(&mut w, &mut controller, "Controller", None);
    ok(
        &mut w,
        &mut pilot,
        Request::SetFleetController {
            controller: Some("Controller".into()),
        },
    );
    ok(
        &mut w,
        &mut controller,
        Request::BeginRemoteControl {
            pilot: "Pilot".into(),
        },
    );
    start(&mut w, &mut pilot, 1, 57);
    assert!(
        !w.handle(
            &mut controller,
            Request::StartResourceJob {
                target_units: 1,
                pilot: Some("Pilot".into())
            }
        )
        .ok
    );
    advance(&mut w, 500);
    w.disconnect(&mut pilot);
    advance(&mut w, 1500);
    let (raw, _) = durable(&db(&d));
    let world: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        world["cargo_unloading"]["Pilot"]["operations"][0]["phase"],
        "Held"
    );
    assert_eq!(
        world["cargo_unloading"]["Pilot"]["operations"][0]["committed_units"],
        25
    );
    assert_eq!(world["remote_controls"]["Controller"], "Pilot");
    assert!(
        !w.handle(
            &mut controller,
            Request::RemoteControl {
                pilot: "Pilot".into(),
                request: Box::new(Request::ResumeCargoUnload { operation_id: 1 })
            }
        )
        .ok
    );
    ledger(&mut w, &mut controller);
}
