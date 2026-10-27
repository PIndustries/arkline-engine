//! Real finite loaded recall, reusing source service before actual empty capture.
//! Happy paths relocate catalog asteroid positions only. No cargo is injected.
use super::*;

fn recall(f: &mut Fixture) -> CarrierInventoryRequest {
    CarrierInventoryRequest::RecallAll {
        binding: f.binding(),
        batch_id: 1,
    }
}
fn refuse_completed_service_unlink(f: &Fixture) {
    let original = f.world();
    validate_history(&original, &f.server.db).unwrap();
    for (index, member) in state(&original, "Pilot").unwrap().inventory.recalls[0]
        .members
        .iter()
        .enumerate()
    {
        if member.service_operation_id.is_none() {
            continue;
        }
        let mut bad = original.clone();
        bad.accounts
            .get_mut("Pilot")
            .unwrap()
            .carrier_inventory
            .as_mut()
            .unwrap()
            .inventory
            .recalls[0]
            .members[index]
            .service_operation_id = None;
        assert!(
            validate(&bad).is_err(),
            "Terminal captured/stored history must retain unloading prefix"
        );
        rejected_copy(f, &format!("terminal-service-link-{index}"), |db| {
            db.execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&bad).unwrap()],
            )
            .unwrap();
        });
    }
}

fn assert_no_loaded_capture(w: &World) {
    let s = state(w, "Pilot").unwrap();
    for m in &s.inventory.recalls[0].members {
        if let Some(id) = m.capture_operation_id {
            assert_eq!(
                s.ore
                    .holds
                    .iter()
                    .find(|h| h.drone_id == m.drone_id)
                    .map(|h| h.ore_units),
                Some(0)
            );
            if let Some(service) = m.service_operation_id {
                let a = s.inventory.operation(service).unwrap();
                let b = s.inventory.operation(id).unwrap();
                assert_eq!(
                    a.action,
                    CarrierInventoryAction::ReturnToUnload {
                        resume_mining: false
                    }
                );
                assert_eq!(a.phase, CarrierOperationPhase::Completed);
                assert!(a.updated_at_ms <= b.accepted_at_ms && service < id);
            }
        }
    }
}
#[test]
fn actual_loaded_recall_two_banks_retains_prefix_hold_restart_and_empty_capture() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    launch(&mut f, 11);
    mine(&mut f, 1, 40, -1.);
    mine(&mut f, 11, 40, 1.);
    let original = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    let request = recall(&mut f);
    f.request(request.clone());
    let accepted = state(&f.world(), "Pilot").unwrap().clone();
    assert_eq!(accepted.inventory.recalls[0].members.len(), 20);
    assert_eq!(
        accepted.inventory.recalls[0]
            .members
            .iter()
            .filter(|m| m.service_operation_id.is_some())
            .count(),
        2
    );
    assert!(accepted.inventory.recalls[0]
        .members
        .iter()
        .all(|m| m.capture_operation_id.is_none()));
    // Removing or cross-linking a physical service child cannot evade ownership.
    let members: Vec<_> = accepted.inventory.recalls[0]
        .members
        .iter()
        .enumerate()
        .filter(|(_, m)| m.service_operation_id.is_some())
        .map(|(i, _)| i)
        .collect();
    let topology = definition(&f.world()).unwrap().unwrap().topology.clone();
    for kind in 0..3 {
        let mut bad = accepted.inventory.clone();
        match kind {
            0 => bad.recalls[0].members[members[0]].service_operation_id = None,
            1 => {
                bad.recalls[0].members[members[0]].service_operation_id =
                    bad.recalls[0].members[members[1]].service_operation_id
            }
            _ => {
                bad.recalls[0].members[members[0]].capture_operation_id =
                    bad.recalls[0].members[members[0]].service_operation_id
            }
        }
        assert!(!bad.validate_shape(&topology));
    }
    let service = accepted.inventory.recalls[0].members[members[0]]
        .service_operation_id
        .unwrap();
    let binding = f.binding();
    let denied = f.server.handle_at(
        &mut f.session,
        Request::CarrierInventory {
            request: CarrierInventoryRequest::Hold {
                binding,
                operation_id: service,
            },
        },
        f.now,
    );
    assert!(!denied.ok && denied.message.contains("queue owns"));
    let mut held = false;
    for tick in 0..5000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.roster, original);
        assert_no_loaded_capture(&w);
        if !held && w.pilots["Pilot"].view.ore == 25 {
            assert!(s.inventory.recalls[0].phase.active());
            f.request(CarrierInventoryRequest::HoldRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            let before = state(&f.world(), "Pilot").unwrap().clone();
            assert!(before.ore.granted_service_id.is_none() && before.ore.waiting.is_empty());
            f.reopen();
            for _ in 0..5 {
                f.coarse_step();
            }
            assert_eq!(f.world().pilots["Pilot"].view.ore, 25);
            assert_eq!(
                state(&f.world(), "Pilot").unwrap().ore.ore_units(),
                Some(55)
            );
            let high = state(&f.world(), "Pilot").unwrap().inventory.high_watermark;
            f.request(request.clone());
            assert_eq!(
                state(&f.world(), "Pilot").unwrap().inventory.high_watermark,
                high
            );
            f.service_evidence();
            f.request(CarrierInventoryRequest::ResumeRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            held = true;
        }
        if tick % 500 == 0 {
            eprintln!(
                "loaded recall tick{tick} ship{} ore{} phase{:?} runs{:?}",
                w.pilots["Pilot"].view.ore,
                s.ore.ore_units().unwrap(),
                s.inventory.recalls[0].phase,
                s.runs
                    .values()
                    .map(|r| (r.operation_id, r.target_index, r.held))
                    .collect::<Vec<_>>()
            );
        }
        if state(&f.world(), "Pilot").unwrap().inventory.recalls[0].phase
            == CarrierRecallPhase::Completed
        {
            break;
        }
    }
    assert!(held);
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.recalls[0].phase, CarrierRecallPhase::Completed);
    assert_eq!(w.pilots["Pilot"].view.ore, 80);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(s.ore.lost_ore_units, 0);
    assert!(s
        .inventory
        .entries
        .iter()
        .all(|e| e.hit_points == 100 && e.location.dock().is_some()));
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Protected { .. }))
            .count(),
        10
    );
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
            .count(),
        10
    );
    assert!(
        s.runs.is_empty()
            && s.arrivals.is_empty()
            && s.departures.is_empty()
            && s.inventory.cleanups.is_empty()
            && s.inventory.reservations.is_empty()
    );
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    f.service_evidence();
    f.reopen();
    f.service_evidence();
    refuse_completed_service_unlink(&f);
}

#[test]
fn actual_loaded_recall_cancel_remaining_finishes_service_without_false_storage() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    mine(&mut f, 1, 40, -1.);
    let request = recall(&mut f);
    f.request(request.clone());
    let mut stopped = false;
    for _ in 0..4000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if !stopped && w.pilots["Pilot"].view.ore == 25 {
            assert!(s.inventory.recalls[0].phase.active());
            f.request(CarrierInventoryRequest::HoldRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            f.request(CarrierInventoryRequest::CancelRecallRemaining {
                binding: f.binding(),
                batch_id: 1,
            });
            f.reopen();
            for _ in 0..3 {
                f.coarse_step();
            }
            assert_eq!(f.world().pilots["Pilot"].view.ore, 25);
            assert_eq!(
                state(&f.world(), "Pilot").unwrap().ore.ore_units(),
                Some(15)
            );
            f.service_evidence();
            f.request(CarrierInventoryRequest::ResumeRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            stopped = true;
        }
        if state(&f.world(), "Pilot").unwrap().inventory.recalls[0].phase
            == CarrierRecallPhase::Cancelled
        {
            break;
        }
    }
    assert!(stopped);
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.recalls[0].phase, CarrierRecallPhase::Cancelled);
    assert_eq!(
        s.inventory.entry(1).unwrap().location,
        CarrierDroneLocation::Deployed
    );
    let m = s.inventory.recalls[0]
        .members
        .iter()
        .find(|m| m.drone_id == 1)
        .unwrap();
    assert!(
        m.cancelled
            && m.completed_location.is_none()
            && m.capture_operation_id.is_none()
            && m.store_operation_id.is_none()
    );
    assert_eq!(
        s.inventory
            .operation(m.service_operation_id.unwrap())
            .unwrap()
            .phase,
        CarrierOperationPhase::Completed
    );
    assert_eq!(w.pilots["Pilot"].view.ore, 40);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(
        s.inventory.recalls[0]
            .members
            .iter()
            .filter(|m| m
                .completed_location
                .as_ref()
                .is_some_and(|l| l.dock().is_some()))
            .count(),
        19
    );
    f.service_evidence();
    let next = s.inventory.next_operation_id().unwrap();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: next,
        drone_id: 1,
        action: CarrierInventoryAction::RecallToStorage,
    });
    let before = state(&f.world(), "Pilot").unwrap().inventory.clone();
    f.request(request);
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().inventory,
        before,
        "Historical recall retry cannot seize newer capture"
    );
    for _ in 0..200 {
        f.coarse_step();
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .entry(1)
            .unwrap()
            .location
            .dock()
            .is_some()
        {
            break;
        }
    }
    assert!(state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .entry(1)
        .unwrap()
        .location
        .dock()
        .is_some());
    f.service_evidence();
    refuse_completed_service_unlink(&f);
}

#[test]
fn actual_twenty_deployed_mixed_payload_recall_drains_service_and_storage_without_traffic_holds() {
    let mut f = parallel_tests::fixture();
    launch(&mut f, 1);
    launch(&mut f, 11);
    mine(&mut f, 1, 40, -1.);
    mine(&mut f, 11, 40, 1.);
    // Deploy the other eighteen actual identities; the two loaded miners remain
    // held at their real mined positions, not moved or injected into a formation.
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..400 {
        f.coarse_step();
        if state(&f.world(), "Pilot").unwrap().inventory.deployments[0].phase
            == CarrierDeploymentPhase::Completed
        {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.inventory.deployments[0].phase,
        CarrierDeploymentPhase::Completed
    );
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| e.location == CarrierDroneLocation::Deployed)
            .count(),
        20
    );
    assert_eq!(s.ore.ore_units(), Some(80));
    let original = s.inventory.roster.clone();
    f.service_evidence();
    let request = recall(&mut f);
    f.request(request);
    let accepted = f.now;
    for tick in 0..700 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.roster, original);
        assert_no_loaded_capture(&w);
        assert!(
            s.runs.values().all(|r| !r.held),
            "Ordinary mixed return traffic must queue, not require manual resume: {:?}",
            s.runs
                .values()
                .filter(|r| r.held)
                .map(|r| (r.operation_id, r.hold_reason.clone()))
                .collect::<Vec<_>>()
        );
        assert!(
            s.arrivals.values().all(|a| !a.held),
            "Ordinary arrival traffic must remain fairly queued"
        );
        if tick % 20 == 0 {
            f.service_evidence();
        }
        if s.inventory.recalls[0].phase == CarrierRecallPhase::Completed {
            break;
        }
        if tick % 50 == 0 {
            eprintln!(
                "mixed20 recall tick{tick} ship{} ore{} active{} stored{}",
                w.pilots["Pilot"].view.ore,
                s.ore.ore_units().unwrap(),
                w.pilots["Pilot"].view.drones.len(),
                s.inventory
                    .entries
                    .iter()
                    .filter(|e| e.location.dock().is_some())
                    .count()
            );
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.recalls[0].phase, CarrierRecallPhase::Completed);
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Protected { .. }))
            .count(),
        10
    );
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
            .count(),
        10
    );
    assert_eq!(w.pilots["Pilot"].view.ore, 80);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(s.ore.lost_ore_units, 0);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert!(
        s.runs.is_empty()
            && s.arrivals.is_empty()
            && s.departures.is_empty()
            && s.inventory.reservations.is_empty()
            && s.inventory.cleanups.is_empty()
    );
    eprintln!(
        "MIXED20_RECALL elapsed_ms={} cadence_ms=1000 two_loaded40_eighteen_empty",
        f.now - accepted
    );
    f.service_evidence();
    f.reopen();
    f.service_evidence();
    refuse_completed_service_unlink(&f);
}
