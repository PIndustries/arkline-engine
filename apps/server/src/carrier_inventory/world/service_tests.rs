//! Real request/tick mining and source socket service in disposable stationary
//! worlds. Only catalog asteroid POSITION is relocated; IDs/reserves/ore/HP and
//! drones are not injected. Normal-world admission remains off.
use super::*;

impl Fixture {
    /// Optional immutable actual Response + independently replayable ledger
    /// samples. Each test invocation receives a fresh directory from the runner;
    /// multiple same-clock requests remain separate files, never overwritten.
    fn service_evidence(&mut self) {
        self.snapshot_valid();
        let Ok(directory) = std::env::var("ARKLINE_SERVICE_EVIDENCE") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        assert!(directory.is_dir(), "Fresh evidence directory required");
        let sequence = std::fs::read_dir(&directory).unwrap().count() / 2;
        let response = self.ok(Request::Snapshot);
        let export = self.ok(Request::ExportLedger).ledger.unwrap();
        arkline_verifier::verify(&export, None).unwrap();
        for (kind, bytes) in [
            ("response", serde_json::to_vec(&response).unwrap()),
            ("ledger", serde_json::to_vec(&export).unwrap()),
        ] {
            use std::io::Write;
            let path = directory.join(format!("{sequence:04}-{}-{kind}.json", self.now));
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap()
                .write_all(&bytes)
                .unwrap();
        }
    }
}
pub(super) fn launch(f: &mut Fixture, id: u64) {
    let op = state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .next_operation_id()
        .unwrap();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: op,
        drone_id: id,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..60 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.runs.is_empty() && s.departures.is_empty() {
            return;
        }
    }
    panic!("Launch did not clear");
}
pub(super) fn mine(f: &mut Fixture, id: u64, units: u64, sign: f64) -> u64 {
    let mut w = f.world();
    let a = w
        .asteroids
        .iter_mut()
        .filter(|a| a.system_id == HOME_SYSTEM_ID)
        .nth(usize::from(sign > 0.))
        .unwrap();
    a.position = [-1200., 100., sign * 1000.];
    let target = a.id;
    f.write(&w);
    f.ok(Request::LockAsteroid { id: target });
    for _ in 0..200 {
        if f.world().pilots["Pilot"]
            .view
            .locked_asteroids
            .contains(&target)
        {
            break;
        }
        f.step();
    }
    f.ok(Request::AssignMiningDrone {
        id,
        target_id: target,
    });
    for tick in 0..4000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.ore
            .holds
            .iter()
            .find(|h| h.drone_id == id)
            .unwrap()
            .ore_units
            >= units
        {
            let live = w.pilots["Pilot"]
                .view
                .drones
                .iter()
                .find(|d| d.id == id)
                .unwrap();
            let local = s.drone_poses[&id];
            eprintln!("MINED_DIAGNOSTIC actual_q={:?} local_q={:?} recomposed_q={:?} actual_p={:?} local_p={:?}",live.orientation_xyzw,local.orientation_xyzw,parent(&w,"Pilot").unwrap().compose(local).unwrap().orientation_xyzw,live.position,local.position_m);
            f.ok(Request::DroneHoldById { id });
            f.service_evidence();
            let r = f.ok(Request::Snapshot);
            let pilot = &r.snapshot.as_ref().unwrap().pilots[0];
            let mut wrong = pilot.carrier_inventory.clone().unwrap();
            wrong
                .drone_poses
                .iter_mut()
                .find(|(drone, _)| *drone == id)
                .unwrap()
                .1
                .position_m[0] += 0.01;
            assert!(
                wrong.validate_projection(pilot).is_err(),
                "Wrong free body position cannot hide in quaternion roundoff policy"
            );
            return target;
        }
        if tick % 200 == 0 {
            eprintln!(
                "mine {id} tick{tick} {:?} ore{} blocks{:?}",
                w.pilots["Pilot"]
                    .view
                    .drones
                    .iter()
                    .find(|d| d.id == id)
                    .unwrap()
                    .position,
                s.ore.ore_units().unwrap(),
                s.mining_blocks
            );
        }
    }
    panic!("Actual mining failed");
}
pub(super) fn service(f: &mut Fixture, id: u64, resume: bool) -> u64 {
    let op = state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .next_operation_id()
        .unwrap();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: op,
        drone_id: id,
        action: CarrierInventoryAction::ReturnToUnload {
            resume_mining: resume,
        },
    });
    op
}
#[test]
fn actual_1703_mine_payload_service_reopen_and_receipt_replay() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    mine(&mut f, 1, 40, -1.);
    let before = f.world();
    let asteroid_before = before.belt_ore;
    assert_eq!(before.pilots["Pilot"].view.ore, 0);
    let op = service(&mut f, 1, false);
    let mut held = false;
    let mut committed = vec![];
    for tick in 0..4000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if tick % 200 == 0 {
            eprintln!(
                "service tick{tick} phase{:?} runs{:?} ore{}",
                s.inventory.operation(op).unwrap().phase,
                s.runs
                    .values()
                    .map(|r| (r.target_index, r.held, r.hold_reason.clone()))
                    .collect::<Vec<_>>(),
                w.pilots["Pilot"].view.ore
            );
        }
        if !held && w.pilots["Pilot"].view.ore == 25 {
            let reached = &s.runs[&op];
            assert!(
                reached.waiting_for_transfer
                    && reached.reached.ore_coupled
                    && reached.confirmed == CarrierMotionFrame::from(&reached.route[5])
            );
            eprintln!("ACTUAL_FIRST_BATCH25 source_frame5 confirmed={} segment_finish={} ore_remaining={}",reached.confirmed_at_ms,reached.started_at_ms+reached.duration_ms,s.ore.ore_units().unwrap());
            let mut drifting = w.clone();
            drifting
                .pilots
                .get_mut("Pilot")
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == 1)
                .unwrap()
                .velocity = [0.01, 0., 0.];
            drones::sync(drifting.pilots.get_mut("Pilot").unwrap());
            assert!(
                validate(&drifting).is_err(),
                "Reached roots alone cannot authorize a drifting flight-held drone"
            );
            f.service_evidence();
            let response = f.ok(Request::Snapshot);
            let p = &response.snapshot.as_ref().unwrap().pilots[0];
            let mut wrong = p.carrier_inventory.clone().unwrap();
            let m = wrong
                .motions
                .iter_mut()
                .find(|m| m.operation_id == op)
                .unwrap();
            if let CarrierPresentationRoute::Hardware { target_index, .. } = &mut m.route {
                *target_index = 4;
            } else {
                panic!("Missing service source motion");
            }
            assert!(
                wrong.validate_projection(p).is_err(),
                "Uncoupled target cannot impersonate reached contact"
            );
            f.request(CarrierInventoryRequest::Hold {
                binding: f.binding(),
                operation_id: op,
            });
            let retained = state(&f.world(), "Pilot").unwrap().clone();
            f.reopen();
            for _ in 0..30 {
                f.coarse_step();
            }
            f.service_evidence();
            let after = f.world();
            let current = state(&after, "Pilot").unwrap();
            assert_eq!(after.pilots["Pilot"].view.ore, 25);
            assert_eq!(current.ore.ore_units(), retained.ore.ore_units());
            assert_eq!(
                current.ore.last_committed_at_ms,
                retained.ore.last_committed_at_ms
            );
            assert!(current
                .ore
                .holds
                .iter()
                .all(|h| h.cycle.is_none() && h.mining_intent.is_none()));
            assert_eq!(current.machinery, retained.machinery);
            f.request(CarrierInventoryRequest::Resume {
                binding: f.binding(),
                operation_id: op,
            });
            held = true;
        }
        committed = s
            .ore
            .services
            .iter()
            .flat_map(|a| a.receipts.clone())
            .collect();
        if s.inventory.operation(op).unwrap().phase == CarrierOperationPhase::Completed {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(held);
    assert_eq!(w.pilots["Pilot"].view.ore, 40);
    assert_eq!(w.belt_ore, asteroid_before);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(
        s.inventory.operation(op).unwrap().phase,
        CarrierOperationPhase::Completed
    );
    assert!(s.runs.is_empty() && s.inventory.reservations.is_empty());
    assert_eq!(
        committed.iter().map(|r| r.units).collect::<Vec<_>>(),
        vec![25, 15]
    );
    assert!(committed[1].committed_at_ms - committed[0].committed_at_ms >= 500);
    validate_history(&w, &f.server.db).unwrap();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: op,
        drone_id: 1,
        action: CarrierInventoryAction::ReturnToUnload {
            resume_mining: false,
        },
    });
    assert_eq!(f.world().pilots["Pilot"].view.ore, 40);
    f.service_evidence();
    f.reopen();
    f.service_evidence();
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    verify_economic_refusals(&f);
}

#[test]
fn rolled_free_pose_round_trip_keeps_real_attitude_and_rejects_wrong_angle() {
    let host = BayPose {
        position_m: [3e11, -4e11, 1e11],
        orientation_xyzw: [0.3f64.sin(), 0., 0., 0.3f64.cos()],
    };
    let local = BayPose {
        position_m: [400., -70., 80.],
        orientation_xyzw: [0., 0.6f64.sin(), 0., 0.6f64.cos()],
    };
    let live = host
        .compose(local)
        .unwrap()
        .with_forward([0.321, -0.55, 0.07])
        .unwrap();
    let retained = crate::independent_bays::independent_world_to_body(host, live).unwrap();
    let round = host.compose(retained).unwrap();
    assert!(carrier_composed_attitude_matches(
        live.orientation_xyzw,
        round.orientation_xyzw
    ));
    assert!(carrier_composed_attitude_matches(
        live.orientation_xyzw,
        round.orientation_xyzw.map(|v| -v)
    ));
    let mut wrong = round.orientation_xyzw;
    wrong[0] += 1e-10;
    assert!(!carrier_composed_attitude_matches(
        live.orientation_xyzw,
        wrong
    ));
}

fn rejected_copy(f: &Fixture, label: &str, mutate: impl FnOnce(&rusqlite::Connection)) {
    let path = f
        .path
        .parent()
        .unwrap()
        .join(format!("malformed-{label}.sqlite"));
    assert!(!path.exists());
    f.server
        .db
        .execute("VACUUM INTO ?1", [path.to_str().unwrap()])
        .unwrap();
    let db = rusqlite::Connection::open(&path).unwrap();
    mutate(&db);
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let result = Server::open_editor_realm_with_scene(
        &path,
        default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1,
    );
    assert!(result.is_err(), "{label} was accepted");
    eprintln!("no-write rejection {label}: {}", result.err().unwrap());
    assert_eq!(
        std::fs::read(path).unwrap(),
        before,
        "{label} mutated before refusal"
    );
}
fn verify_economic_refusals(f: &Fixture) {
    let original = f.world();
    validate_history(&original, &f.server.db).unwrap();
    let mut wrong = original.clone();
    wrong.pilots.get_mut("Pilot").unwrap().view.ore += 1;
    wrong.asteroids.iter_mut().find(|a| a.ore > 0).unwrap().ore -= 1;
    wrong.belt_ore -= 1;
    crate::validate(&wrong, Some(&default_pioneer_room_layout())).unwrap();
    rejected_copy(f, "conserved-ship-mint", |db| {
        db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&wrong).unwrap()],
        )
        .unwrap();
    });
    let mut wrong = original.clone();
    wrong.pilots.get_mut("Pilot").unwrap().view.ore -= 1;
    wrong
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .ore
        .holds
        .iter_mut()
        .find(|h| h.drone_id == 1)
        .unwrap()
        .ore_units += 1;
    crate::validate(&wrong, Some(&default_pioneer_room_layout())).unwrap();
    rejected_copy(f, "conserved-wrong-hold", |db| {
        db.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&wrong).unwrap()],
        )
        .unwrap();
    });
    rejected_copy(f, "orphan-ledger", |db| {
        let last: String = db
            .query_row(
                "SELECT json FROM ledger ORDER BY sequence DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let mut e: LedgerEntry = serde_json::from_str(&last).unwrap();
        e.sequence += 1;
        e.previous_hash = e.hash.clone();
        e.event = EconomicEvent::CarrierInventory {
            record: CarrierInventoryLedgerEvent::Extracted {
                pilot: "Unowned".into(),
                vessel_id: state(&original, "Pilot")
                    .unwrap()
                    .inventory
                    .binding
                    .vessel_id
                    .clone(),
                drone_id: 1,
                cycle_id: 999,
                target_id: original.asteroids[0].id,
                cycle_started_at_ms: e.timestamp_ms.saturating_sub(8000),
                units: 1,
                job_id: None,
            },
        };
        e.hash = e.calculate_hash();
        db.execute(
            "INSERT INTO ledger(sequence,json) VALUES(?1,?2)",
            rusqlite::params![e.sequence, serde_json::to_string(&e).unwrap()],
        )
        .unwrap();
    });
}

#[test]
fn actual_1703_both_banks_share_one_fifo_manifold_and_preserve_all_payload() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    launch(&mut f, 11);
    mine(&mut f, 1, 40, -1.);
    mine(&mut f, 11, 40, 1.);
    let first = service(&mut f, 1, false);
    let second = service(&mut f, 11, false);
    let mut held = None;
    let mut two_coupled = false;
    for _ in 0..4000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if !two_coupled {
            let ready = s
                .runs
                .iter()
                .filter(|(_, r)| r.waiting_for_transfer && !r.held)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>();
            if held.is_none() {
                if let Some(id) = ready.first() {
                    f.request(CarrierInventoryRequest::Hold {
                        binding: f.binding(),
                        operation_id: *id,
                    });
                    held = Some(*id);
                }
            } else if let Some(held_id) =
                held.filter(|held_id| ready.iter().any(|id| id != held_id))
            {
                f.request(CarrierInventoryRequest::Resume {
                    binding: f.binding(),
                    operation_id: held_id,
                });
                let w = f.world();
                let s = state(&w, "Pilot").unwrap();
                assert_eq!(
                    s.runs
                        .values()
                        .filter(|r| r.waiting_for_transfer && !r.held)
                        .count(),
                    2
                );
                assert_eq!(
                    s.inventory
                        .operations
                        .iter()
                        .filter(|o| o.operation_id == first || o.operation_id == second)
                        .map(|o| o.berth_id.unwrap() / 5)
                        .collect::<BTreeSet<_>>(),
                    BTreeSet::from([0, 1])
                );
                two_coupled = true;
                f.service_evidence();
            }
        }
        if s.inventory.operation(first).unwrap().phase == CarrierOperationPhase::Completed
            && s.inventory.operation(second).unwrap().phase == CarrierOperationPhase::Completed
        {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(two_coupled);
    assert_eq!(w.pilots["Pilot"].view.ore, 80);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(s.ore.lost_ore_units, 0);
    let mut receipts = s
        .ore
        .services
        .iter()
        .flat_map(|a| a.receipts.iter())
        .collect::<Vec<_>>();
    receipts.sort_by_key(|r| r.committed_at_ms);
    assert_eq!(receipts.iter().map(|r| r.units).sum::<u64>(), 80);
    assert_eq!(receipts.len(), 4);
    assert!(receipts
        .windows(2)
        .all(|p| p[1].committed_at_ms - p[0].committed_at_ms >= 500));
    assert!(s.runs.is_empty() && s.inventory.reservations.is_empty());
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    f.reopen();
    f.service_evidence();
    for id in [1, 11] {
        let op = state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .next_operation_id()
            .unwrap();
        f.request(CarrierInventoryRequest::Begin {
            binding: f.binding(),
            operation_id: op,
            drone_id: id,
            action: CarrierInventoryAction::RecallToStorage,
        });
    }
    for _ in 0..500 {
        f.step();
        if f.world().pilots["Pilot"].view.drones.is_empty() {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| e.location.dock().is_some())
            .count(),
        20
    );
    assert_eq!(w.pilots["Pilot"].view.ore, 80);
    f.service_evidence();
}

#[test]
fn free_mining_guard_blocks_crossing_peers_without_serializing_disjoint_paths() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    launch(&mut f, 11);
    // Deliberate geometric proposal fixtures, not additional mined units.
    for crossing in [true, false] {
        let mut before = f.world();
        let mut paths = BTreeMap::new();
        for (id, x) in [(1, -1.), (11, 1.)] {
            let d = before
                .pilots
                .get_mut("Pilot")
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == id)
                .unwrap();
            d.position = [x * if crossing { 10. } else { 100. }, 0., -600.];
            d.velocity = [0.; 3];
            d.mode = DroneMode::Mining {
                target_id: before.asteroids[0].id,
            };
            d.updated_at_ms = f.now;
            let target = if crossing {
                [-d.position[0], 0., -600.]
            } else {
                [d.position[0], 0., -620.]
            };
            paths.insert(("Pilot".into(), id), vec![d.position, target]);
        }
        for reverse_order in [false, true] {
            let mut proposed = before.clone();
            if reverse_order {
                proposed
                    .pilots
                    .get_mut("Pilot")
                    .unwrap()
                    .view
                    .drones
                    .reverse();
            }
            for d in &mut proposed.pilots.get_mut("Pilot").unwrap().view.drones {
                d.position = *paths[&("Pilot".into(), d.id)].last().unwrap();
                d.updated_at_ms = f.now + 200;
            }
            cargo::guard_free_motion(&before, &mut proposed, &paths, f.now + 200).unwrap();
            for d in &proposed.pilots["Pilot"].view.drones {
                let old = before.pilots["Pilot"]
                    .view
                    .drones
                    .iter()
                    .find(|old| old.id == d.id)
                    .unwrap();
                assert_eq!(d.hit_points, old.hit_points);
                if crossing {
                    assert_eq!(d.position, old.position);
                    assert_eq!(d.orientation_xyzw, old.orientation_xyzw);
                    assert_eq!(d.velocity, [0.; 3]);
                    assert_eq!(d.mode, DroneMode::Hold);
                    assert!(state(&proposed, "Pilot")
                        .unwrap()
                        .mining_blocks
                        .contains_key(&d.id));
                } else {
                    assert_eq!(d.position, *paths[&("Pilot".into(), d.id)].last().unwrap());
                    assert!(state(&proposed, "Pilot").unwrap().mining_blocks.is_empty());
                }
            }
            assert_eq!(state(&proposed, "Pilot").unwrap().ore.ore_units(), Some(0));
        }
    }
}

#[test]
fn actual_1703_full_hold_auto_service_resumes_same_mining_intent_then_unloads_and_stores() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    let mut w = f.world();
    let a = w
        .asteroids
        .iter_mut()
        .find(|a| a.system_id == HOME_SYSTEM_ID)
        .unwrap();
    a.position = [-1200., 100., -1000.];
    let target = a.id;
    f.write(&w);
    f.ok(Request::LockAsteroid { id: target });
    for _ in 0..200 {
        if f.world().pilots["Pilot"]
            .view
            .locked_asteroids
            .contains(&target)
        {
            break;
        }
        f.step();
    }
    f.ok(Request::AssignMiningDrone {
        id: 1,
        target_id: target,
    });
    let original = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    let mut reached_full = false;
    let mut resumed = false;
    for tick in 0..6000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        let h = s.ore.holds.iter().find(|h| h.drone_id == 1).unwrap();
        assert!(h.ore_units <= 100);
        reached_full |= h.ore_units == 100;
        if w.pilots["Pilot"].view.ore == 100
            && s.inventory.operations.iter().any(|o| {
                matches!(
                    o.action,
                    CarrierInventoryAction::ReturnToUnload {
                        resume_mining: true
                    }
                ) && o.phase == CarrierOperationPhase::Completed
            })
        {
            assert!(h
                .mining_intent
                .as_ref()
                .is_some_and(|i| i.target_id == target));
            assert!(
                matches!(w.pilots["Pilot"].view.drones[0].mode,DroneMode::Mining{target_id} if target_id==target)
            );
            resumed = true;
            if h.ore_units >= 10 {
                break;
            }
        }
        if tick % 500 == 0 {
            eprintln!(
                "auto loop tick{tick} payload{} ship{} phase{:?} blocks{:?}",
                h.ore_units,
                w.pilots["Pilot"].view.ore,
                s.inventory.active_operation(1).map(|o| o.phase),
                s.mining_blocks
            );
        }
    }
    assert!(reached_full && resumed);
    f.ok(Request::DroneHoldById { id: 1 });
    let w = f.world();
    assert_eq!(w.pilots["Pilot"].view.ore, 100);
    assert_eq!(state(&w, "Pilot").unwrap().ore.ore_units(), Some(10));
    let op = service(&mut f, 1, false);
    for _ in 0..3000 {
        f.step();
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .operation(op)
            .unwrap()
            .phase
            == CarrierOperationPhase::Completed
        {
            break;
        }
    }
    let capture = state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .next_operation_id()
        .unwrap();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: capture,
        drone_id: 1,
        action: CarrierInventoryAction::RecallToStorage,
    });
    for _ in 0..500 {
        f.step();
        if f.world().pilots["Pilot"].view.drones.is_empty() {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.roster, original);
    assert_eq!(w.pilots["Pilot"].view.ore, 110);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(s.ore.lost_ore_units, 0);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert!(matches!(
        s.inventory.entry(1).unwrap().location,
        CarrierDroneLocation::Ready { .. }
    ));
    f.service_evidence();
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
    f.reopen();
    f.service_evidence();
}

#[test]
fn actual_1703_signal_loss_holds_empty_and_loaded_identity_without_legacy_return() {
    for loaded in [false, true] {
        let mut f = parallel_tests::fixture();
        launch(&mut f, 1);
        if loaded {
            mine(&mut f, 1, 40, -1.);
        }
        let mut w = f.world();
        let a = w
            .asteroids
            .iter_mut()
            .find(|a| a.system_id == HOME_SYSTEM_ID)
            .unwrap();
        a.position = [-26., 0., -10500.];
        let target = a.id;
        f.write(&w);
        f.ok(Request::LockAsteroid { id: target });
        for _ in 0..200 {
            if f.world().pilots["Pilot"]
                .view
                .locked_asteroids
                .contains(&target)
            {
                break;
            }
            f.step();
        }
        f.ok(Request::AssignMiningDrone {
            id: 1,
            target_id: target,
        });
        let payload = if loaded { 40 } else { 0 };
        let mut blocked = false;
        for _ in 0..1800 {
            f.step();
            let w = f.world();
            let s = state(&w, "Pilot").unwrap();
            let d = &w.pilots["Pilot"].view.drones[0];
            assert_eq!(s.ore.ore_units(), Some(payload));
            if d.signal_strength == 0.
                && s.mining_blocks
                    .get(&1)
                    .is_some_and(|r| r.contains("signal"))
            {
                assert_eq!(d.mode, DroneMode::Hold);
                assert_eq!(d.velocity, [0.; 3]);
                assert_eq!(d.hit_points, 100);
                blocked = true;
                break;
            }
        }
        assert!(blocked, "Natural control-range loss was not reached");
        let held = f.world().pilots["Pilot"].view.drones[0].clone();
        f.reopen();
        f.coarse_step();
        f.service_evidence();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(w.pilots["Pilot"].view.drones[0].position, held.position);
        assert_eq!(s.ore.ore_units(), Some(payload));
        assert_eq!(
            s.inventory.entry(1).unwrap().location,
            CarrierDroneLocation::Deployed
        );
        assert!(s
            .ore
            .holds
            .iter()
            .all(|h| h.cycle.is_none() && h.mining_intent.is_none()));
        let export = f.ok(Request::ExportLedger).ledger.unwrap();
        arkline_verifier::verify(&export, None).unwrap();
    }
}

#[test]
fn actual_1703_due_transfer_death_sinks_payload_once_and_secures_empty_hardware() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    mine(&mut f, 1, 40, -1.);
    let op = service(&mut f, 1, false);
    let mut due = None;
    for _ in 0..3000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if let Some(id) = s.ore.granted_service_id {
            due = Some(
                s.ore
                    .services
                    .iter()
                    .find(|a| a.service_id == id)
                    .unwrap()
                    .accepted_at_ms
                    + 500,
            );
            break;
        }
    }
    let due = due.expect("Actual reached socket must acquire a bounded grant");
    while f.now + 100 < due {
        f.step();
    }
    assert_eq!(f.now + 100, due);
    f.now = due;
    let mut w = f.world();
    advance(&mut w, f.now, None).unwrap();
    // Exact post-combat injection on the due-pump tick, not a weapon simulation.
    w.pilots.get_mut("Pilot").unwrap().view.drones[0].hit_points = 0;
    let mut events = finish(&mut w, f.now).unwrap();
    events.extend(cargo::after_combat(&mut w, f.now).unwrap());
    assert_eq!(events.len(), 1);
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .retain(|d| d.hit_points > 0);
    drones::sync(w.pilots.get_mut("Pilot").unwrap());
    {
        let tx = f.server.db.transaction().unwrap();
        let mut entries = crate::load_entries(&tx, true).unwrap();
        for event in events {
            crate::append_event(
                &tx,
                &mut entries,
                event,
                f.now,
                CARRIER_INVENTORY_RULESET_VERSION,
            )
            .unwrap();
        }
        service::commit_transfers(&mut w, &tx, &mut entries, f.now).unwrap();
        validate(&w).unwrap();
        assert_eq!(w.pilots["Pilot"].view.ore, 0);
        assert_eq!(state(&w, "Pilot").unwrap().ore.lost_ore_units, 40);
        tx.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
        tx.execute(
            "UPDATE editor_clock SET time_ms=?1 WHERE id=1",
            [f.now.to_string()],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    assert!(finish(&mut w, f.now).unwrap().is_empty());
    f.server.editor_time = Some(f.now);
    f.reopen();
    f.service_evidence();
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: op,
    });
    for _ in 0..300 {
        f.step();
        if state(&f.world(), "Pilot").unwrap().runs.is_empty() {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(
        s.runs.is_empty() && s.inventory.cleanups.is_empty() && s.inventory.reservations.is_empty()
    );
    assert_eq!(s.ore.lost_ore_units, 40);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(w.pilots["Pilot"].view.ore, 0);
    assert_eq!(
        s.ore
            .services
            .iter()
            .map(|a| a.receipts.len())
            .sum::<usize>(),
        0
    );
    f.service_evidence();
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}

#[test]
fn actual_1703_due_extraction_death_does_not_mint_or_debit_and_releases_claim() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    let target = mine(&mut f, 1, 40, -1.);
    f.ok(Request::AssignMiningDrone {
        id: 1,
        target_id: target,
    });
    let mut claim = None;
    for _ in 0..1000 {
        f.step();
        let w = f.world();
        if let Some(c) = state(&w, "Pilot")
            .unwrap()
            .ore
            .holds
            .iter()
            .find(|h| h.drone_id == 1)
            .unwrap()
            .cycle
            .clone()
        {
            claim = Some(c);
            break;
        }
    }
    let claim = claim.expect("Actual mining must reserve its next extraction");
    let mut w = f.world();
    let before_asteroids = w.asteroids.clone();
    let before_belt = w.belt_ore;
    let before_power = w.pilots["Pilot"].view.power.clone();
    assert_eq!(state(&w, "Pilot").unwrap().ore.ore_units(), Some(40));
    assert_eq!(
        state(&w, "Pilot").unwrap().ore.reserved_incoming(),
        Some(50)
    );
    f.now = claim.completes_at_ms;
    advance(&mut w, f.now, None).unwrap();
    // Explicit post-combat lethal injection at the real reserved cycle deadline;
    // this tests new-carrier reconciliation ordering, not weapon generation.
    w.pilots.get_mut("Pilot").unwrap().view.drones[0].hit_points = 0;
    let mut events = finish(&mut w, f.now).unwrap();
    events.extend(cargo::after_combat(&mut w, f.now).unwrap());
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        EconomicEvent::CarrierInventory {
            record: CarrierInventoryLedgerEvent::Destroyed { lost_units: 40, .. }
        }
    ));
    assert_eq!(w.asteroids, before_asteroids);
    assert_eq!(w.belt_ore, before_belt);
    assert_eq!(w.pilots["Pilot"].view.power, before_power);
    assert_eq!(w.pilots["Pilot"].view.ore, 0);
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(s.ore.reserved_incoming(), Some(0));
    assert_eq!(s.ore.lost_ore_units, 40);
    assert!(s.ore.holds.iter().all(|h| h.cycle.is_none()));
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .retain(|d| d.hit_points > 0);
    drones::sync(w.pilots.get_mut("Pilot").unwrap());
    {
        let tx = f.server.db.transaction().unwrap();
        let mut entries = crate::load_entries(&tx, true).unwrap();
        for event in events {
            crate::append_event(
                &tx,
                &mut entries,
                event,
                f.now,
                CARRIER_INVENTORY_RULESET_VERSION,
            )
            .unwrap();
        }
        service::commit_transfers(&mut w, &tx, &mut entries, f.now).unwrap();
        validate(&w).unwrap();
        tx.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
        tx.execute(
            "UPDATE editor_clock SET time_ms=?1 WHERE id=1",
            [f.now.to_string()],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    assert!(finish(&mut w, f.now).unwrap().is_empty());
    assert!(cargo::after_combat(&mut w, f.now).unwrap().is_empty());
    f.server.editor_time = Some(f.now);
    f.reopen();
    f.coarse_step();
    f.service_evidence();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.ore.lost_ore_units, 40);
    assert_eq!(s.ore.reserved_incoming(), Some(0));
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    let export = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&export, None).unwrap();
}

#[path = "loaded_recall_tests.rs"]
mod loaded_recall_tests;
