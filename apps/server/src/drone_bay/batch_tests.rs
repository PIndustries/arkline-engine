//! Real transaction boundary tests for finite, serialized batch operations.
use super::*;

fn begin(f: &Fixture, id: u64, kind: DroneBayBatchKind) -> Request {
    Request::DroneBay {
        request: DroneBayRequest::BeginBatch {
            binding: state(&f.world(), "Pilot").unwrap().binding.clone(),
            batch_id: id,
            batch_kind: kind,
        },
    }
}
fn control(f: &Fixture, id: u64, action: &str) -> Request {
    let binding = state(&f.world(), "Pilot").unwrap().binding.clone();
    Request::DroneBay {
        request: match action {
            "hold" => DroneBayRequest::HoldBatch {
                binding,
                batch_id: id,
            },
            "resume" => DroneBayRequest::ResumeBatch {
                binding,
                batch_id: id,
            },
            _ => DroneBayRequest::CancelBatch {
                binding,
                batch_id: id,
            },
        },
    }
}
fn batch(f: &Fixture, id: u64) -> DroneBayBatchOperation {
    state(&f.world(), "Pilot")
        .unwrap()
        .batches
        .as_ref()
        .unwrap()
        .operation(id)
        .unwrap()
        .clone()
}
fn save(f: &Fixture, w: &World) {
    f.server
        .db
        .execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(w).unwrap()],
        )
        .unwrap();
}
fn reopen(f: &mut Fixture) {
    // Replace only the disposable fixture's server, preserving the real file.
    let placeholder = Server::open(":memory:").unwrap();
    drop(std::mem::replace(&mut f.server, placeholder));
    f.server = Server::open_editor_realm_with_scene(
        &f.path,
        default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1,
    )
    .unwrap();
    f.session = None;
    f.ok(connect(Some(f.token.clone())));
}
fn check(f: &Fixture) {
    let w = f.world();
    validate(&w).unwrap();
    let s = state(&w, "Pilot").unwrap();
    let c = s.counts(None);
    assert_eq!(c.stored + c.active + c.destroyed, 15);
    w.pilots["Pilot"]
        .view
        .drone_bay
        .as_ref()
        .unwrap()
        .validate_projection(
            &s.binding,
            &arkline_protocol::drone_bay_layout::home_poses(),
            &arkline_protocol::drone_bay_layout::mechanism_ids(),
            &w.pilots["Pilot"].view.drones,
        )
        .unwrap();
}
fn finish_batch(f: &mut Fixture, id: u64) {
    for _ in 0..900 {
        check(f);
        let b = batch(f, id);
        if !b.active() {
            return;
        }
        assert!(!b.held, "{}", b.message);
        f.now += 1_000;
        f.server.tick_at(f.now).unwrap();
    }
    panic!("batch did not finish: {:?}", batch(f, id));
}
fn mining() -> DroneBayBatchKind {
    DroneBayBatchKind::DeployClass {
        class: DroneClass::Mining,
    }
}

#[test]
fn batch_five_deploy_and_recall_preserve_all_ids_health_and_safe_storage() {
    let mut f = Fixture::new();
    f.space();
    let initial = state(&f.world(), "Pilot").unwrap().clone();
    let original = initial
        .stock
        .iter()
        .map(|s| (s.drone_id, s.home_cradle, s.class))
        .collect::<Vec<_>>();
    let ledger_before: i64 = f
        .server
        .db
        .query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get(0))
        .unwrap();
    let request = begin(&f, 1, mining());
    f.ok(request.clone());
    let b = batch(&f, 1);
    assert_eq!(b.members.len(), 5);
    assert_eq!(
        b.members
            .iter()
            .filter(|m| m.status == DroneBayBatchMemberStatus::Active)
            .count(),
        1
    );
    assert_eq!(
        b.members
            .iter()
            .filter(|m| m.status == DroneBayBatchMemberStatus::Pending)
            .count(),
        4
    );
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).stored, 14);
    f.ok(request);
    finish_batch(&mut f, 1);
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).active, 5);
    assert!(batch(&f, 1)
        .members
        .iter()
        .all(|m| m.status == DroneBayBatchMemberStatus::Launched));
    f.ok(begin(&f, 2, DroneBayBatchKind::RecallAll));
    // Acceptance never stores an approaching drone, including queued members.
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).stored, 10);
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).active, 5);
    assert!(f.world().pilots["Pilot"]
        .view
        .drones
        .iter()
        .all(|d| d.mining_cycle.is_none() && d.attack_ready_at_ms.is_none()));
    finish_batch(&mut f, 2);
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.counts(None).stored, 15);
    assert_eq!(
        s.stock
            .iter()
            .map(|s| (s.drone_id, s.home_cradle, s.class))
            .collect::<Vec<_>>(),
        original
    );
    assert!(s
        .stock
        .iter()
        .all(|e| e.status == DroneStockStatus::Stored { hit_points: 100 }));
    assert_eq!(s.high_watermark, 10);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(w.pilots["Pilot"].view.ore, 0);
    assert_eq!(
        f.server
            .db
            .query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        ledger_before
    );
    // Historical outcomes survive a later launch; validation compares receipts,
    // not mutable stock presence. No batch or individual ID is reused.
    let id = s.stock[5].drone_id;
    f.ok(f.request(DroneBayOperationKind::Launch, id));
    check(&f);
    assert!(batch(&f, 2)
        .members
        .iter()
        .all(|m| m.status == DroneBayBatchMemberStatus::Stored));
}

#[test]
fn batch_hold_cancel_remaining_and_resume_current_are_actionable_and_idempotent() {
    let mut f = Fixture::new();
    f.space();
    f.ok(begin(&f, 1, mining()));
    for _ in 0..20 {
        f.step();
    }
    f.ok(control(&f, 1, "hold"));
    let held = f.world().pilots["Pilot"].view.drones[0].position;
    f.ok(control(&f, 1, "hold"));
    f.ok(control(&f, 1, "cancel"));
    let b = batch(&f, 1);
    assert_eq!(b.phase, DroneBayBatchPhase::Cancelling);
    assert!(b.held);
    assert!(b.current_operation_id.is_some());
    assert_eq!(
        b.members
            .iter()
            .filter(|m| m.status == DroneBayBatchMemberStatus::Cancelled)
            .count(),
        4
    );
    for _ in 0..10 {
        f.step();
    }
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].position, held);
    f.ok(control(&f, 1, "resume"));
    let raw = f.raw();
    f.ok(control(&f, 1, "resume"));
    assert_eq!(f.raw(), raw);
    finish_batch(&mut f, 1);
    assert_eq!(batch(&f, 1).phase, DroneBayBatchPhase::Cancelled);
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).active, 1);
    let old = begin(&f, 1, mining());
    f.ok(begin(&f, 2, DroneBayBatchKind::RecallAll));
    let current = batch(&f, 2);
    let stock = state(&f.world(), "Pilot").unwrap().clone();
    for request in [
        old,
        control(&f, 1, "hold"),
        control(&f, 1, "resume"),
        control(&f, 1, "cancel"),
    ] {
        f.ok(request);
    }
    assert_eq!(batch(&f, 2), current);
    assert_eq!(state(&f.world(), "Pilot").unwrap(), &stock);
    let conflict = begin(&f, 1, DroneBayBatchKind::RecallAll);
    let raw = f.raw();
    assert!(!f.server.handle_at(&mut f.session, conflict, f.now).ok);
    assert_eq!(f.raw(), raw);
    // Cancel Remaining on recovery still physically completes the current body.
    f.ok(control(&f, 2, "cancel"));
    finish_batch(&mut f, 2);
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).stored, 15);
    assert_eq!(batch(&f, 2).phase, DroneBayBatchPhase::Cancelled);
}

#[test]
fn batch_disconnect_restart_holds_and_lost_response_retry_does_not_restart() {
    let mut f = Fixture::new();
    f.space();
    let command = begin(&f, 1, mining());
    f.ok(command.clone());
    for _ in 0..25 {
        f.step();
    }
    let position = f.world().pilots["Pilot"].view.drones[0].position;
    f.server.disconnect(&mut f.session);
    assert!(batch(&f, 1).held);
    reopen(&mut f);
    assert!(batch(&f, 1).held);
    f.now += 20_000;
    f.server.tick_at(f.now).unwrap();
    f.ok(command);
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].position, position);
    assert_eq!(state(&f.world(), "Pilot").unwrap().high_watermark, 1);
    f.ok(control(&f, 1, "cancel"));
    f.ok(control(&f, 1, "resume"));
    finish_batch(&mut f, 1);
    check(&f);
}

#[test]
fn batch_accept_and_next_member_start_roll_back_at_world_commit() {
    let mut f = Fixture::new();
    f.space();
    let command = begin(&f, 1, mining());
    let raw = f.raw();
    f.server.db.execute_batch("CREATE TRIGGER fail_batch BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'batch commit fault'); END;").unwrap();
    assert!(f
        .server
        .execute(&mut f.session, command.clone(), f.now, false, None)
        .is_err());
    assert_eq!(f.raw(), raw);
    f.server
        .db
        .execute_batch("DROP TRIGGER fail_batch")
        .unwrap();
    f.ok(command);
    // Observe the committed gap: first member is finished, next remains Pending.
    loop {
        let b = batch(&f, 1);
        if b.current_operation_id.is_none() {
            break;
        }
        f.now += 1_000;
        f.server.tick_at(f.now).unwrap();
        assert!(!batch(&f, 1).held);
    }
    let w = f.world();
    assert_eq!(state(&w, "Pilot").unwrap().high_watermark, 1);
    let raw = f.raw();
    let at = f.now + 100;
    f.server.db.execute_batch("CREATE TRIGGER fail_batch BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'next member fault'); END;").unwrap();
    assert!(f.server.tick_at(at).is_err());
    assert_eq!(f.raw(), raw);
    f.server
        .db
        .execute_batch("DROP TRIGGER fail_batch")
        .unwrap();
    f.now = at;
    f.server.tick_at(at).unwrap();
    assert_eq!(state(&f.world(), "Pilot").unwrap().high_watermark, 2);
    assert_eq!(batch(&f, 1).members[1].operation_id, Some(2));
    f.ok(control(&f, 1, "cancel"));
    finish_batch(&mut f, 1);
}

#[test]
fn batch_malformed_outcomes_cross_bindings_and_missing_journal_reject_without_write() {
    let mut f = Fixture::new();
    f.space();
    f.ok(begin(&f, 1, mining()));
    let good = f.world();
    for case in 0..6 {
        let mut w = good.clone();
        let s = w
            .accounts
            .get_mut("Pilot")
            .unwrap()
            .drone_bays
            .values_mut()
            .next()
            .unwrap();
        match case {
            0 => {
                let b = &mut s.batches.as_mut().unwrap().operations[0];
                b.members[0].status = DroneBayBatchMemberStatus::Stored;
            }
            1 => {
                s.batches.as_mut().unwrap().operations[0].members[0].home_cradle = 14;
            }
            2 => {
                s.batches.as_mut().unwrap().operations[0].current_operation_id = None;
            }
            3 => {
                s.batches = None;
            }
            4 => {
                s.batches.as_mut().unwrap().operations[0].binding.vessel_id = "foreign".into();
            }
            _ => {
                s.batches.as_mut().unwrap().operations[0].members[1].status =
                    DroneBayBatchMemberStatus::Stored;
            }
        }
        save(&f, &w);
        let raw = f.raw();
        assert!(
            !f.server
                .handle_at(&mut f.session, Request::Snapshot, f.now)
                .ok,
            "case{case}"
        );
        assert_eq!(f.raw(), raw, "case{case}");
    }
    save(&f, &good);
    check(&f);
}

#[test]
fn batch_feature_is_fresh_only_and_old_serializer_cannot_drop_it() {
    let mut old = Fixture::with_prior_world(Some(true));
    assert_eq!(old.world().drone_bay_batch_version, None);
    let s = state(&old.world(), "Pilot").unwrap().clone();
    assert_eq!(s.schema_version, DRONE_BAY_LEGACY_SCHEMA_VERSION);
    assert!(s.batches.is_none());
    let mut request = connect(Some(old.token.clone()));
    if let Request::Connect {
        world_layout_version,
        ..
    } = &mut request
    {
        *world_layout_version = 3;
    }
    old.ok(request);
    old.space();
    let raw = old.raw();
    let request = begin(&old, 1, mining());
    assert!(!old.server.handle_at(&mut old.session, request, old.now).ok);
    assert_eq!(old.raw(), raw);
    let id = s.stock[5].drone_id;
    old.ok(old.request(DroneBayOperationKind::Launch, id));
    old.finish();
    assert_eq!(state(&old.world(), "Pilot").unwrap().counts(None).active, 1);
    let fresh = Fixture::new();
    let raw = fresh.raw();
    let mut value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .remove("drone_bay_batch_version");
    for account in value["accounts"].as_object_mut().unwrap().values_mut() {
        for bay in account["drone_bays"].as_object_mut().unwrap().values_mut() {
            bay.as_object_mut().unwrap().remove("batches");
        }
    }
    assert!(fresh
        .server
        .db
        .execute("UPDATE world SET json=?1", [value.to_string()])
        .is_err());
    assert_eq!(fresh.raw(), raw);
    let count:i64=old.server.db.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name='preserve_drone_bay_batch_v1'",[],|r|r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn batch_gap_cancel_does_not_admit_next_member_and_restart_gap_stays_held() {
    for restart in [false, true] {
        let mut f = Fixture::new();
        f.space();
        f.ok(begin(&f, 1, mining()));
        while batch(&f, 1).current_operation_id.is_some() {
            f.now += 1_000;
            f.server.tick_at(f.now).unwrap();
        }
        assert_eq!(state(&f.world(), "Pilot").unwrap().high_watermark, 1);
        if restart {
            reopen(&mut f);
            assert!(batch(&f, 1).held);
        }
        // Even if this request is the next authority event, cancellation observes
        // committed Pending entries instead of starting one in execute's prelude.
        f.now += 100;
        f.ok(control(&f, 1, "cancel"));
        assert_eq!(batch(&f, 1).phase, DroneBayBatchPhase::Cancelled);
        assert_eq!(state(&f.world(), "Pilot").unwrap().high_watermark, 1);
        assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).active, 1);
        check(&f);
    }
}

#[test]
fn batch_individual_cancel_pauses_remaining_without_requeue_or_teleport() {
    let mut f = Fixture::new();
    f.space();
    f.ok(begin(&f, 1, mining()));
    for _ in 0..40 {
        f.step();
    }
    let position = f.world().pilots["Pilot"].view.drones[0].position;
    f.ok(f.control("cancel"));
    assert!(batch(&f, 1).held);
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].position, position);
    f.finish();
    check(&f);
    assert!(batch(&f, 1).held);
    assert_eq!(
        batch(&f, 1).members[0].status,
        DroneBayBatchMemberStatus::Cancelled
    );
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).stored, 15);
    assert_eq!(state(&f.world(), "Pilot").unwrap().high_watermark, 1);
    f.ok(control(&f, 1, "cancel"));
    assert_eq!(batch(&f, 1).phase, DroneBayBatchPhase::Cancelled);
}

#[test]
fn batch_destroyed_current_retains_held_cleanup_through_restart_without_refill() {
    let mut f = Fixture::new();
    f.space();
    f.ok(begin(&f, 1, mining()));
    for _ in 0..35 {
        f.step();
    }
    f.ok(control(&f, 1, "hold"));
    let mut w = f.world();
    let id = w.pilots["Pilot"].view.drones[0].id;
    // Explicit combat-boundary injection; separate existing weapon tests prove
    // incoming combat ordering. This test isolates queue/cleanup persistence.
    w.pilots.get_mut("Pilot").unwrap().view.drones[0].hit_points = 0;
    finish(&mut w, f.now).unwrap();
    validate(&w).unwrap();
    save(&f, &w);
    assert_eq!(
        batch(&f, 1).members[0].status,
        DroneBayBatchMemberStatus::Destroyed
    );
    assert_eq!(batch(&f, 1).current_operation_id, Some(1));
    assert!(
        state(&f.world(), "Pilot")
            .unwrap()
            .cleanup
            .as_ref()
            .unwrap()
            .held
    );
    reopen(&mut f);
    check(&f);
    f.ok(control(&f, 1, "cancel"));
    assert_eq!(batch(&f, 1).phase, DroneBayBatchPhase::Cancelling);
    f.ok(control(&f, 1, "resume"));
    finish_batch(&mut f, 1);
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.counts(None).destroyed, 1);
    assert_eq!(s.counts(None).stored, 14);
    assert!(s.cleanup.is_none());
    assert!(w.pilots["Pilot"].view.drones.iter().all(|d| d.id != id));
    assert_eq!(s.high_watermark, 1);
    assert_eq!(w.next_drone_id, 16);
}

#[test]
fn batch_completed_retries_do_not_cancel_new_resource_job_or_replace_intent() {
    let mut f = Fixture::new();
    f.space();
    let old = begin(&f, 1, mining());
    f.ok(old.clone());
    f.ok(f.control("cancel"));
    f.finish();
    f.ok(control(&f, 1, "cancel"));
    assert!(!batch(&f, 1).active());
    f.ok(Request::StartResourceJob {
        pilot: None,
        target_units: 100,
    });
    let before = f.world().resource_jobs["Pilot"].view.clone();
    assert!(!before.phase.is_terminal());
    for request in [
        old,
        control(&f, 1, "hold"),
        control(&f, 1, "resume"),
        control(&f, 1, "cancel"),
    ] {
        f.ok(request);
    }
    let after = f.world().resource_jobs["Pilot"].view.clone();
    assert_eq!(after, before);
    assert_eq!(state(&f.world(), "Pilot").unwrap().high_watermark, 1);
    let raw = f.raw();
    let command = begin(&f, 2, mining());
    assert!(!f.server.handle_at(&mut f.session, command, f.now).ok);
    assert_eq!(f.raw(), raw);
}

#[test]
fn batch_empty_fresh_world_retains_stamp_and_client_four_requirement() {
    let dir = std::env::temp_dir().join(format!("arkline-empty-batch-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut server = Server::open_editor_realm_with_scene(
        dir.join("world.sqlite"),
        default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1,
    )
    .unwrap();
    let before: String = server
        .db
        .query_row("SELECT json FROM world", [], |r| r.get(0))
        .unwrap();
    let mut old: serde_json::Value = serde_json::from_str(&before).unwrap();
    assert!(old["pilots"].as_object().unwrap().is_empty());
    old.as_object_mut()
        .unwrap()
        .remove("drone_bay_batch_version");
    assert!(server
        .db
        .execute("UPDATE world SET json=?1", [old.to_string()])
        .is_err());
    let now = server.editor_time.unwrap();
    let mut session = None;
    let mut request = connect(None);
    if let Request::Connect {
        world_layout_version,
        ..
    } = &mut request
    {
        *world_layout_version = 3;
    }
    assert!(!server.handle_at(&mut session, request, now).ok);
    assert!(session.is_none());
    assert_eq!(
        server
            .db
            .query_row("SELECT json FROM world", [], |r| r.get::<_, String>(0))
            .unwrap(),
        before
    );
    assert!(server.handle_at(&mut session, connect(None), now).ok);
}

#[test]
#[ignore = "exports actual 100ms Deploy Mining then Recall All authority Responses"]
fn export_actual_five_deploy_and_recall_all() {
    use sha2::{Digest, Sha256};
    let dir =
        PathBuf::from(std::env::var("ARKLINE_BATCH_EXPORT").expect("explicit export destination"));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(
        !dir.join("receipt.json").exists(),
        "never overwrite accepted evidence"
    );
    let producer =
        std::env::var("ARKLINE_BATCH_SOURCE_COMMIT").expect("exact clean producer commit");
    let mut f = Fixture::new();
    f.space();
    let start = f.now;
    let roster = state(&f.world(), "Pilot").unwrap().stock.clone();
    let miners: Vec<_> = roster
        .iter()
        .filter(|e| e.class == DroneClass::Mining)
        .map(|e| e.drone_id)
        .collect();
    assert_eq!(miners.len(), 5);
    let mut frames = Vec::new();
    let mut previous = std::collections::BTreeMap::<u64, [f64; 3]>::new();
    let mut max_step = 0f64;
    let mut record = |f: &mut Fixture, frames: &mut Vec<serde_json::Value>| {
        let mut response = f.snap();
        response.resume_token = None;
        let snapshot = response.snapshot.as_ref().unwrap();
        let pilot = snapshot.pilots.iter().find(|p| p.name == "Pilot").unwrap();
        let s = response
            .assets
            .as_ref()
            .unwrap()
            .drone_bay
            .as_ref()
            .unwrap();
        let public = pilot.drone_bay.as_ref().unwrap();
        assert_eq!(
            s.stock
                .iter()
                .map(|e| (e.drone_id, e.home_cradle, e.class))
                .collect::<Vec<_>>(),
            roster
                .iter()
                .map(|e| (e.drone_id, e.home_cradle, e.class))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            s.counts(None).stored + s.counts(None).active + s.counts(None).destroyed,
            15
        );
        assert_eq!(s.counts(None).destroyed, 0);
        let flight = pilot.flight.as_ref().unwrap();
        let parent = BayPose {
            position_m: flight.position,
            orientation_xyzw: flight.orientation,
        };
        for entry in &public.cradles {
            assert_eq!(entry.hit_points, 100);
            let position = if entry.presence == DroneBayPresence::Stored {
                parent.compose(entry.body_pose).unwrap().position_m
            } else {
                pilot
                    .drones
                    .iter()
                    .find(|d| d.id == entry.drone_id)
                    .unwrap()
                    .position
            };
            if let Some(old) = previous.insert(entry.drone_id, position) {
                let step = crate::flight::distance(position, old);
                max_step = max_step.max(step);
                assert!(
                    step <= 10.,
                    "unexpected discontinuity {step}m/100ms for {}",
                    entry.drone_id
                );
            }
        }
        let file = format!("frame-{:05}.json", frames.len());
        let row = serde_json::json!({"index":frames.len(),"file":file,"time_ms":f.now,"elapsed_ms":f.now-start,
            "stored":s.counts(None).stored,"active":s.counts(None).active,"destroyed":s.counts(None).destroyed,
            "batch":public.batch,"operations":s.operations,"handling_operation":s.handling_operation,
            "door_fraction":public.machinery.door_fraction});
        let bytes = serde_json::to_vec(&response).unwrap();
        std::fs::write(dir.join(&file), &bytes).unwrap();
        let mut row = row;
        row["response_sha256"] = hex::encode(Sha256::digest(&bytes)).into();
        frames.push(row);
    };
    record(&mut f, &mut frames);
    for _ in 0..20 {
        f.step();
        record(&mut f, &mut frames);
    }
    let deploy_frame = frames.len();
    f.now += 100;
    f.ok(begin(&f, 1, mining()));
    record(&mut f, &mut frames);
    for _ in 0..6_000 {
        if !batch(&f, 1).active() {
            break;
        }
        assert!(!batch(&f, 1).held, "{}", batch(&f, 1).message);
        f.step();
        record(&mut f, &mut frames);
    }
    assert_eq!(batch(&f, 1).phase, DroneBayBatchPhase::Completed);
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).active, 5);
    let deployed_frame = frames.len() - 1;
    for _ in 0..20 {
        f.step();
        record(&mut f, &mut frames);
    }
    let recall_frame = frames.len();
    f.now += 100;
    f.ok(begin(&f, 2, DroneBayBatchKind::RecallAll));
    record(&mut f, &mut frames);
    for _ in 0..6_000 {
        if !batch(&f, 2).active() {
            break;
        }
        assert!(!batch(&f, 2).held, "{}", batch(&f, 2).message);
        f.step();
        record(&mut f, &mut frames);
    }
    assert_eq!(batch(&f, 2).phase, DroneBayBatchPhase::Completed);
    let stored_frame = frames.len() - 1;
    for _ in 0..20 {
        f.step();
        record(&mut f, &mut frames);
    }
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).stored, 15);
    assert!(f.world().pilots["Pilot"].view.drones.is_empty());
    let ledger = f.ok(Request::ExportLedger).ledger.unwrap();
    assert!(
        !ledger
            .entries
            .iter()
            .any(|entry| matches!(entry.event, EconomicEvent::OreMined { .. })),
        "this proof deploys and recalls; it does not mine"
    );
    let receipt = serde_json::json!({"schema":"arkline.drone-batch-authority-sequence/v1","producer_commit":producer,"pilot":"Pilot","cadence_ms":100,
        "initial_time_ms":start,"final_time_ms":f.now,"duration_ms":f.now-start,"mining_drone_ids":miners,"initial_roster":roster,
        "deploy_frame":deploy_frame,"deployed_frame":deployed_frame,"recall_frame":recall_frame,"stored_frame":stored_frame,
        "maximum_same_id_step_m":max_step,"frames":frames,"credentials_removed":true,
        "fixture":"Fresh DockingV1 Server; actual Connect, Undock, BeginBatch DeployClass Mining, normal 100ms ticks, BeginBatch RecallAll. One unchanged shared route, no snapshot or inventory edits; initial/final 2-second holds.",
        "registration":12,"source_sha256":arkline_protocol::drone_bay_layout::DRONE_BAY_SOURCE_SHA256});
    std::fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn batch_remote_source_is_worker_owned_private_assets_stay_controller_and_loss_holds_queue() {
    let mut f = Fixture::new();
    f.space();
    let mut controller = None;
    let mut request = connect(None);
    if let Request::Connect { pilot, .. } = &mut request {
        *pilot = "Controller".into();
    }
    assert!(f.server.handle_at(&mut controller, request, f.now).ok);
    f.ok(Request::SetFleetController {
        controller: Some("Controller".into()),
    });
    assert!(
        f.server
            .handle_at(
                &mut controller,
                Request::BeginRemoteControl {
                    pilot: "Pilot".into()
                },
                f.now
            )
            .ok
    );
    let request = begin(&f, 1, mining());
    let response = f.server.handle_at(
        &mut controller,
        Request::RemoteControl {
            pilot: "Pilot".into(),
            request: Box::new(request),
        },
        f.now,
    );
    assert!(response.ok, "{}", response.message);
    assert_eq!(
        response
            .assets
            .unwrap()
            .drone_bay
            .unwrap()
            .binding
            .vessel_id,
        "v1:Controller:Pioneer"
    );
    assert!(batch(&f, 1).active());
    assert_eq!(state(&f.world(), "Controller").unwrap().high_watermark, 0);
    let mut wrong = begin(&f, 2, mining());
    if let Request::DroneBay {
        request: DroneBayRequest::BeginBatch { binding, .. },
    } = &mut wrong
    {
        *binding = state(&f.world(), "Controller").unwrap().binding.clone();
    }
    let raw = f.raw();
    assert!(
        !f.server
            .handle_at(
                &mut controller,
                Request::RemoteControl {
                    pilot: "Pilot".into(),
                    request: Box::new(wrong)
                },
                f.now
            )
            .ok
    );
    assert_eq!(f.raw(), raw);
    f.server.disconnect(&mut controller);
    assert!(batch(&f, 1).held);
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
    assert_eq!(
        state(&f.world(), "Controller").unwrap().counts(None).stored,
        15
    );
    check(&f);
}

#[test]
fn batch_rejects_competing_job_member_orders_and_carrier_motion_but_preserves_stop() {
    let mut f = Fixture::new();
    f.space();
    f.ok(begin(&f, 1, mining()));
    let id = batch(&f, 1).members[0].drone_id;
    for request in [
        Request::StartResourceJob {
            pilot: None,
            target_units: 100,
        },
        Request::DroneNavigateById {
            id,
            position: [0.; 3],
        },
        Request::Navigate {
            mode: NavigationMode::Approach {
                target: NavigationTarget::Station,
            },
        },
    ] {
        let raw = f.raw();
        assert!(!f.server.handle_at(&mut f.session, request, f.now).ok);
        assert_eq!(f.raw(), raw);
    }
    f.ok(Request::Navigate {
        mode: NavigationMode::Stop,
    });
    check(&f);
}

#[test]
#[ignore = "exports initial and running real batch Responses for early native UI proof"]
fn export_actual_batch_initial_and_running() {
    use sha2::{Digest, Sha256};
    let dir =
        PathBuf::from(std::env::var("ARKLINE_BATCH_INITIAL_EXPORT").expect("explicit output"));
    std::fs::create_dir_all(&dir).unwrap();
    let mut f = Fixture::new();
    let producer = std::env::var("ARKLINE_BATCH_SOURCE_COMMIT").expect("exact producer commit");
    let mut receipt = Vec::new();
    let record = |f: &mut Fixture, name: &str, receipt: &mut Vec<serde_json::Value>| {
        let mut response = f.snap();
        response.resume_token = None;
        let bytes = serde_json::to_vec_pretty(&response).unwrap();
        let s = response
            .assets
            .as_ref()
            .unwrap()
            .drone_bay
            .as_ref()
            .unwrap();
        assert_eq!(s.schema_version, 2);
        let file = format!("{name}.json");
        std::fs::write(dir.join(&file), &bytes).unwrap();
        receipt.push(serde_json::json!({"file":file,"time_ms":f.now,"response_sha256":hex::encode(Sha256::digest(&bytes)),"stored":s.counts(None).stored,"active":s.counts(None).active,"batch":s.batches.as_ref().unwrap().operations.last()}));
    };
    record(&mut f, "initial-docked", &mut receipt);
    f.space();
    record(&mut f, "ready-stationary", &mut receipt);
    f.ok(begin(&f, 1, mining()));
    for _ in 0..15 {
        f.step();
    }
    record(&mut f, "running-deploy", &mut receipt);
    f.ok(control(&f, 1, "hold"));
    record(&mut f, "held-deploy", &mut receipt);
    f.ok(control(&f, 1, "cancel"));
    record(&mut f, "cancelling-held", &mut receipt);
    let value = serde_json::json!({"schema":"arkline.drone-batch-initial-proof/v1","producer_commit":producer,"pilot":"Pilot","responses":receipt,"credentials_removed":true,"fixture":"Actual fresh Server Connect, Undock, BeginBatch, HoldBatch, CancelBatch; no inventory/pose edits"});
    std::fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec_pretty(&value).unwrap(),
    )
    .unwrap();
}
