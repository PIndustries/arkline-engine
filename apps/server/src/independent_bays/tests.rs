use super::*;
use arkline_protocol::*;

fn fixture() -> (IndependentBayState, Vec<DroneStockEntry>) {
    let binding = IndependentBayRegistration {
        schema_version: INDEPENDENT_BAY_SCHEMA_VERSION,
        host: arkline_protocol::drone_bay_definition::PIONEER_BAY_REG12.registration(),
        assembly_revision: 1,
        assembly_source_sha256: "1".repeat(64),
    }
    .bind("pioneer/fixture".into());
    let stock: Vec<_> = (0..MAX_DRONES)
        .map(|i| DroneStockEntry {
            drone_id: i as u64 + 1,
            class: DroneBayState::expected_class(i as u8).unwrap(),
            home_cradle: i as u8,
            status: DroneStockStatus::Stored {
                hit_points: DRONE_HIT_POINTS,
            },
        })
        .collect();
    let lanes = stock
        .iter()
        .map(|e| IndependentBayLane {
            bay_id: e.home_cradle,
            components: IndependentBayComponents {
                door: IndependentBayAvailability::Available,
                support: IndependentBayAvailability::Available,
                clamp: IndependentBayAvailability::Available,
                ore_mount: IndependentBayAvailability::Available,
            },
            stored_drone_id: Some(e.drone_id),
            operation_id: None,
            cleanup: None,
        })
        .collect();
    (
        IndependentBayState {
            schema_version: INDEPENDENT_BAY_SCHEMA_VERSION,
            binding,
            lanes,
            high_watermark: 0,
            operations: vec![],
            manifold: IndependentBayManifold::default(),
        },
        stock,
    )
}
fn request(
    state: &IndependentBayState,
    id: u64,
    drone_id: u64,
    action: IndependentBayAction,
) -> IndependentBayRequest {
    IndependentBayRequest::Begin {
        binding: state.binding.clone(),
        operation_id: id,
        drone_id,
        action,
    }
}
fn deploy_fixture(state: &mut IndependentBayState, stock: &mut [DroneStockEntry], index: usize) {
    // Ownership-state unit fixture only, not a physical launch/capture claim.
    stock[index].status = DroneStockStatus::Active;
    state.lanes[index].stored_drone_id = None;
}
fn coupled_pair() -> (IndependentBayState, Vec<DroneStockEntry>) {
    let (mut state, mut stock) = fixture();
    for (id, index) in [(1, 5), (2, 6)] {
        deploy_fixture(&mut state, &mut stock, index);
        let req = request(
            &state,
            id,
            index as u64 + 1,
            IndependentBayAction::ReturnToUnload {
                resume_mining: true,
            },
        );
        begin(&mut state, &stock, &req, 0, None).unwrap();
        assert_eq!(
            reserve_lane(&mut state, &stock, id, &[index as u8], true, 0).unwrap(),
            Some(index as u8)
        );
        // Confirmed contact is supplied by this state fixture; real route proof
        // belongs to the separately source-bound executor integration tests.
        state.operations[id as usize - 1].phase = IndependentBayPhase::Coupling;
        wait_for_manifold(&mut state, id, 0).unwrap();
    }
    assert!(state.validate_shape(&stock));
    (state, stock)
}
#[test]
fn identified_retry_retains_original_and_does_not_touch_newer_work() {
    let (mut state, stock) = fixture();
    let req = request(&state, 1, 1, IndependentBayAction::Launch);
    assert_eq!(
        begin(&mut state, &stock, &req, 10, None).unwrap(),
        BeginDisposition::Accepted
    );
    assert!(cancel_unstarted(&mut state, 1, 11).unwrap());
    let newer = request(&state, 2, 2, IndependentBayAction::Launch);
    begin(&mut state, &stock, &newer, 12, Some(99)).unwrap();
    let before = state.clone();
    assert_eq!(
        begin(&mut state, &stock, &req, 999, None).unwrap(),
        BeginDisposition::Retained
    );
    assert_eq!(state, before);
    let conflict = request(&state, 1, 2, IndependentBayAction::Launch);
    assert!(begin(&mut state, &stock, &conflict, 999, None).is_err());
    assert_eq!(state, before);
    assert!(begin(&mut state, &stock, &req, 999, Some(99)).is_err());
    assert_eq!(state, before);
    assert!(state.validate_shape(&stock));
}
#[test]
fn finite_stock_rejects_destroyed_or_wrong_presence_and_no_free_spawn() {
    let (mut state, mut stock) = fixture();
    stock[0].status = DroneStockStatus::Destroyed { at_ms: 0 };
    state.lanes[0].stored_drone_id = None;
    for (drone, action) in [
        (1, IndependentBayAction::Launch),
        (1, IndependentBayAction::ReturnToStorage),
        (1000, IndependentBayAction::Launch),
        (2, IndependentBayAction::ReturnToStorage),
    ] {
        let before = state.clone();
        let req = request(&state, 1, drone, action);
        assert!(begin(&mut state, &stock, &req, 1, None).is_err());
        assert_eq!(state, before);
    }
    assert_eq!(stock.len(), 15);
    assert_eq!(state.high_watermark, 0);
}
#[test]
fn fifteen_distinct_resources_reserve_without_global_gantry_lock() {
    let (mut state, stock) = fixture();
    for id in 1..=15 {
        let req = request(&state, id, id, IndependentBayAction::Launch);
        begin(&mut state, &stock, &req, 0, None).unwrap();
    }
    for id in (1..=15).rev() {
        assert_eq!(
            reserve_lane(&mut state, &stock, id, &[id as u8 - 1], false, 1).unwrap(),
            Some(id as u8 - 1)
        );
        assert!(state.validate_shape(&stock));
    }
    assert_eq!(
        state
            .lanes
            .iter()
            .filter(|l| l.operation_id.is_some())
            .count(),
        15
    );
    // This proves ownership concurrency only, not geometric route disjointness.
    hold_operation(&mut state, 1, 2, "Injected lane fault").unwrap();
    assert_eq!(
        state
            .operations
            .iter()
            .filter(|o| o.phase == IndependentBayPhase::Preparing)
            .count(),
        14
    );
    assert!(!cancel_unstarted(&mut state, 2, 2).unwrap());
    assert!(state.carrier_locked());
    assert!(state.validate_shape(&stock));
}
#[test]
fn fallback_reservation_never_evicts_and_two_returns_one_spare_has_one_owner() {
    let (mut state, mut stock) = fixture();
    for index in [5, 6, 7] {
        deploy_fixture(&mut state, &mut stock, index);
    }
    for index in [5, 6] {
        state.lanes[index].components.door = IndependentBayAvailability::Unavailable {
            reason: "Injected unavailable door".into(),
        };
    }
    for (id, drone) in [(1, 6), (2, 7)] {
        let req = request(&state, id, drone, IndependentBayAction::ReturnToStorage);
        begin(&mut state, &stock, &req, 0, None).unwrap();
    }
    // Only these routes were admitted by the caller; occupied bay8 must not be
    // taken even when listed, and the second return cannot steal bay7.
    assert_eq!(
        reserve_lane(&mut state, &stock, 1, &[5, 6, 7, 8], false, 1).unwrap(),
        Some(7)
    );
    assert_eq!(
        reserve_lane(&mut state, &stock, 2, &[5, 6, 7, 8], false, 1).unwrap(),
        None
    );
    assert_eq!(state.lanes[8].stored_drone_id, Some(9));
    assert_eq!(stock[5].home_cradle, 5);
    assert_eq!(state.operations[0].assigned_bay, Some(7));
    assert_eq!(state.operations[1].phase, IndependentBayPhase::Queued);
    assert!(state.validate_shape(&stock));
}
#[test]
fn launch_uses_actual_fallback_berth_not_preferred_home() {
    let (mut state, stock) = fixture();
    state.lanes[0].stored_drone_id = Some(2);
    state.lanes[1].stored_drone_id = Some(1);
    let req = request(&state, 1, 1, IndependentBayAction::Launch);
    begin(&mut state, &stock, &req, 0, None).unwrap();
    assert_eq!(
        reserve_lane(&mut state, &stock, 1, &[0, 1], false, 0).unwrap(),
        Some(1)
    );
    assert_eq!(stock[0].home_cradle, 0);
    assert!(state.validate_shape(&stock));
}
#[test]
fn shared_manifold_cannot_multiply_rate_and_cooldown_survives_handoff() {
    let (mut state, stock) = coupled_pair();
    assert_eq!(next_manifold_waiter(&state, &[1, 2]), Some(1));
    grant_manifold(&mut state, 1, 1, 0).unwrap();
    assert_eq!(next_manifold_waiter(&state, &[2]), None);
    assert!(grant_manifold(&mut state, 2, 2, 0).is_err());
    assert!(!manifold_batch_due(&state, 499));
    for time in [500, 1000, 1500, 2000] {
        record_manifold_commit(&mut state, 1, 25, time).unwrap();
        let before = state.clone();
        assert!(record_manifold_commit(&mut state, 1, 25, time).is_err());
        assert_eq!(state, before);
    }
    finish_manifold_attempt(&mut state, 1, 2000).unwrap();
    assert_eq!(next_manifold_waiter(&state, &[2]), Some(2));
    grant_manifold(&mut state, 2, 2, 2000).unwrap();
    assert!(!manifold_batch_due(&state, 2499));
    record_manifold_commit(&mut state, 2, 7, 2500).unwrap();
    assert!(state.validate_shape(&stock));
    assert_eq!(state.manifold.last_committed_at_ms, Some(2500));
    // Reopen preserves ownership/cooldown; restart's explicit hold is separate.
    let reopened: IndependentBayState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(reopened, state);
    assert!(!manifold_batch_due(&reopened, 2999));
}
#[test]
fn held_grant_releases_pump_and_reentry_uses_new_attempt_without_replay() {
    let (mut state, stock) = coupled_pair();
    grant_manifold(&mut state, 1, 7, 0).unwrap();
    record_manifold_commit(&mut state, 1, 25, 500).unwrap();
    hold_operation(&mut state, 1, 501, "Disconnected").unwrap();
    assert!(state.manifold.grant.is_none());
    assert_eq!(state.manifold.last_committed_at_ms, Some(500));
    assert_eq!(next_manifold_waiter(&state, &[2]), Some(2));
    grant_manifold(&mut state, 2, 8, 501).unwrap();
    assert!(!manifold_batch_due(&state, 1000));
    record_manifold_commit(&mut state, 2, 25, 1001).unwrap();
    finish_manifold_attempt(&mut state, 2, 1001).unwrap();
    // Route executor revalidates contact before a held physical operation can
    // requeue. Here only the ownership transition is exercised.
    state.operations[0].phase = IndependentBayPhase::Coupling;
    state.operations[0].held_phase = None;
    wait_for_manifold(&mut state, 1, 1001).unwrap();
    let before = state.clone();
    assert!(grant_manifold(&mut state, 1, 7, 1001).is_err());
    assert_eq!(state, before);
    grant_manifold(&mut state, 1, 9, 1001).unwrap();
    assert_eq!(state.operations[0].cargo_service_operation_ids, [7, 9]);
    assert!(state.validate_shape(&stock));
}
#[test]
fn unavailable_waiter_does_not_starve_other_and_failed_batch_is_unchanged() {
    let (mut state, stock) = coupled_pair();
    state.lanes[5].components.ore_mount = IndependentBayAvailability::Unavailable {
        reason: "Injected mount fault".into(),
    };
    assert_eq!(next_manifold_waiter(&state, &[1, 2]), Some(2));
    grant_manifold(&mut state, 2, 1, 0).unwrap();
    for (id, units, now) in [(1, 25, 500), (2, 0, 500), (2, 26, 500), (2, 25, 499)] {
        let before = state.clone();
        assert!(record_manifold_commit(&mut state, id, units, now).is_err());
        assert_eq!(state, before);
    }
    record_manifold_commit(&mut state, 2, 25, 50_000).unwrap();
    assert!(!manifold_batch_due(&state, 50_000)); // No offline multi-batch catch-up.
    assert!(state.validate_shape(&stock));
}

#[test]
fn persisted_cooldown_and_service_identity_bind_exact_committed_ledger() {
    let (mut state, stock) = coupled_pair();
    grant_manifold(&mut state, 1, 1, 0).unwrap();
    record_manifold_commit(&mut state, 1, 25, 500).unwrap();
    let binding = supported_drone_cargo_registration().bind(state.binding.vessel_id.clone());
    let mut entry = LedgerEntry {
        sequence: 1,
        ruleset_version: DRONE_CARGO_RULESET_VERSION,
        timestamp_ms: 500,
        previous_hash: GENESIS_HASH.into(),
        hash: String::new(),
        event: EconomicEvent::DroneCargo {
            record: DroneCargoLedgerEvent::Transferred {
                pilot: "FixturePilot".into(),
                vessel_id: binding.vessel_id.clone(),
                drone_id: 6,
                operation_id: 1,
                operation_started_at_ms: 0,
                requested_units: 100,
                sequence: 1,
                units: 25,
            },
        },
    };
    entry.hash = entry.calculate_hash();
    let cargo = DroneCargoState {
        schema_version: DRONE_CARGO_SCHEMA_VERSION,
        binding: binding.clone(),
        initialized_at_ms: 0,
        holds: (6..=10)
            .map(|id| DroneOreHold {
                drone_id: id,
                ore_units: if id == 6 { 75 } else { 0 },
                destroyed_at_ms: None,
                intent_generation: 0,
                mining_intent: None,
                cycle_high_watermark: 0,
                cycle: None,
            })
            .collect(),
        lost_ore_units: 0,
        high_watermark: 1,
        receiver_operation: Some(1),
        cleanup: None,
        operations: vec![DroneDropoffOperation {
            operation_id: 1,
            binding,
            drone_id: 6,
            requested_resume_mining: false,
            cancel_requested: false,
            followup: DroneDropoffFollowup::Hold,
            requested_units: 100,
            committed_units: 25,
            remaining_units: 75,
            phase: DroneDropoffPhase::Transferring,
            held_phase: None,
            accepted_at_ms: 0,
            updated_at_ms: 500,
            next_batch_at_ms: Some(1000),
            message: String::new(),
            receipts: vec![DroneOreTransferReceipt {
                sequence: 1,
                units: 25,
                committed_at_ms: 500,
                ledger_sequence: 1,
                ledger_hash: entry.hash.clone(),
            }],
        }],
    };
    let ledger = vec![entry];
    assert!(state.validate_shape(&stock));
    validate_manifold_history("FixturePilot", &state, &cargo, &ledger).unwrap();
    let mut bad = state.clone();
    bad.manifold.last_committed_at_ms = None;
    assert!(validate_manifold_history("FixturePilot", &bad, &cargo, &ledger).is_err());
    let mut bad = state.clone();
    bad.manifold.last_committed_at_ms = Some(0);
    assert!(validate_manifold_history("FixturePilot", &bad, &cargo, &ledger).is_err());
    let mut bad = cargo.clone();
    bad.operations[0].drone_id = 7;
    assert!(validate_manifold_history("FixturePilot", &state, &bad, &ledger).is_err());
    let mut bad = ledger.clone();
    bad[0].timestamp_ms = 499;
    bad[0].hash = bad[0].calculate_hash();
    assert!(validate_manifold_history("FixturePilot", &state, &cargo, &bad).is_err());
    assert!(validate_manifold_history("OtherPilot", &state, &cargo, &ledger).is_err());
    assert!(validate_manifold_history("FixturePilot", &state, &cargo, &[]).is_err());
    let mut bad = state.clone();
    bad.operations[0].cargo_service_operation_ids.clear();
    assert!(validate_manifold_history("FixturePilot", &bad, &cargo, &ledger).is_err());
}
