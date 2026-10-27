use super::*;

fn fixture() -> (IndependentBayState, Vec<DroneStockEntry>) {
    let binding = IndependentBayRegistration {
        schema_version: INDEPENDENT_BAY_SCHEMA_VERSION,
        host: crate::drone_bay_definition::PIONEER_BAY_REG12.registration(),
        assembly_revision: 1,
        // Structural fixture only. This is deliberately not a registered asset.
        assembly_source_sha256: "1".repeat(64),
    }
    .bind("pioneer/fixture".into());
    let stock: Vec<_> = (0..MAX_DRONES)
        .map(|index| DroneStockEntry {
            drone_id: index as u64 + 1,
            class: DroneBayState::expected_class(index as u8).unwrap(),
            home_cradle: index as u8,
            status: DroneStockStatus::Stored {
                hit_points: crate::DRONE_HIT_POINTS,
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
fn operation(
    state: &IndependentBayState,
    id: u64,
    drone: u64,
    bay: Option<u8>,
    phase: IndependentBayPhase,
) -> IndependentBayOperation {
    IndependentBayOperation {
        operation_id: id,
        drone_id: drone,
        action: IndependentBayAction::Launch,
        binding: state.binding.clone(),
        assigned_bay: bay,
        phase,
        held_phase: None,
        accepted_at_ms: 10,
        updated_at_ms: 10,
        job_id: None,
        message: String::new(),
        cargo_service_operation_ids: vec![],
    }
}
#[test]
fn schema_roundtrip_and_finite_stock_shape_fail_closed() {
    let (state, stock) = fixture();
    assert!(state.validate_shape(&stock));
    assert!(!state.carrier_locked());
    assert_eq!(
        serde_json::from_str::<IndependentBayState>(&serde_json::to_string(&state).unwrap())
            .unwrap(),
        state
    );
    let mut bad = state.clone();
    bad.schema_version += 1;
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.binding.registration.schema_version += 1;
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.binding.registration.assembly_source_sha256 = "unknown".into();
    assert!(!bad.validate_shape(&stock));
    let mut bad = stock.clone();
    bad[1].drone_id = bad[0].drone_id;
    assert!(!state.validate_shape(&bad));
    let mut bad = stock.clone();
    bad[1].home_cradle = bad[0].home_cradle;
    assert!(!state.validate_shape(&bad));
    let mut bad = stock.clone();
    bad[0].status = DroneStockStatus::Stored { hit_points: 0 };
    assert!(!state.validate_shape(&bad));
    let mut bad = state.clone();
    bad.lanes[1].stored_drone_id = bad.lanes[0].stored_drone_id;
    assert!(!bad.validate_shape(&stock));
}
#[test]
fn actual_berth_is_not_immutable_preferred_home() {
    let (mut state, stock) = fixture();
    state.lanes[0].stored_drone_id = Some(2);
    state.lanes[1].stored_drone_id = Some(1);
    assert!(state.validate_shape(&stock));
    assert_eq!(stock[0].home_cradle, 0);
    assert_eq!(state.lanes[1].stored_drone_id, Some(1));
}
#[test]
fn two_lanes_own_distinct_operations_and_hold_is_not_terminal() {
    let (mut state, stock) = fixture();
    state.operations = vec![
        operation(&state, 1, 1, Some(0), IndependentBayPhase::Opening),
        operation(&state, 2, 2, Some(1), IndependentBayPhase::Opening),
    ];
    state.high_watermark = 2;
    state.lanes[0].operation_id = Some(1);
    state.lanes[1].operation_id = Some(2);
    assert!(state.validate_shape(&stock));
    assert!(state.carrier_locked());
    state.operations[0].phase = IndependentBayPhase::Held;
    state.operations[0].held_phase = Some(IndependentBayPhase::Opening);
    assert!(state.validate_shape(&stock));
    assert_eq!(state.operations[1].phase, IndependentBayPhase::Opening);
    let mut bad = state.clone();
    bad.lanes[1].operation_id = Some(1);
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.operations[0].held_phase = None;
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.operations[0].held_phase = Some(IndependentBayPhase::Stored);
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.operations[1].binding.vessel_id = "foreign".into();
    assert!(!bad.validate_shape(&stock));
}
#[test]
fn queued_cancellation_and_destroyed_cleanup_remain_distinct() {
    let (mut state, mut stock) = fixture();
    let mut cancelled = operation(&state, 1, 1, None, IndependentBayPhase::Cancelled);
    cancelled.message = "Cancelled before admission".into();
    state.operations.push(cancelled);
    state.high_watermark = 1;
    assert!(state.validate_shape(&stock));
    assert!(!state.carrier_locked());
    state.operations.push(operation(
        &state,
        2,
        1,
        Some(0),
        IndependentBayPhase::Destroyed,
    ));
    state.high_watermark = 2;
    stock[0].status = DroneStockStatus::Destroyed { at_ms: 10 };
    state.lanes[0].stored_drone_id = None;
    state.lanes[0].cleanup = Some(IndependentBayCleanup {
        operation_id: 2,
        drone_id: 1,
        held: true,
        reason: "Restart held hardware".into(),
    });
    assert!(state.validate_shape(&stock));
    assert!(state.carrier_locked());
    let mut bad = state.clone();
    bad.lanes[0].cleanup.as_mut().unwrap().drone_id = 2;
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.lanes[0].operation_id = Some(2);
    assert!(!bad.validate_shape(&stock));
}
#[test]
fn manifold_owner_queue_and_typed_attempt_links_are_consistent() {
    let (mut state, mut stock) = fixture();
    for (index, phase) in [
        (5, IndependentBayPhase::Transferring),
        (6, IndependentBayPhase::WaitingForManifold),
    ] {
        stock[index].status = DroneStockStatus::Active;
        state.lanes[index].stored_drone_id = None;
        let id = state.operations.len() as u64 + 1;
        let mut op = operation(&state, id, index as u64 + 1, Some(index as u8), phase);
        op.action = IndependentBayAction::ReturnToUnload {
            resume_mining: true,
        };
        if id == 1 {
            op.cargo_service_operation_ids = vec![4, 8];
        }
        state.operations.push(op);
        state.lanes[index].operation_id = Some(id);
    }
    state.high_watermark = 2;
    state.manifold.waiting = vec![2];
    state.manifold.grant = Some(IndependentBayManifoldGrant {
        physical_operation_id: 1,
        cargo_service_operation_id: 8,
        bay_id: 5,
        granted_at_ms: 10,
    });
    assert!(state.validate_shape(&stock));
    let mut bad = state.clone();
    bad.manifold.waiting.push(2);
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.manifold.waiting.clear();
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.manifold
        .grant
        .as_mut()
        .unwrap()
        .cargo_service_operation_id = 4;
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.operations[1].cargo_service_operation_ids = vec![8];
    assert!(!bad.validate_shape(&stock));
    let mut bad = state.clone();
    bad.operations[0].phase = IndependentBayPhase::Held;
    bad.operations[0].held_phase = Some(IndependentBayPhase::Transferring);
    assert!(!bad.validate_shape(&stock));
}
#[test]
fn historical_terminal_result_does_not_rebind_to_current_stock() {
    let (mut state, mut stock) = fixture();
    state
        .operations
        .push(operation(&state, 1, 1, Some(0), IndependentBayPhase::Free));
    state.high_watermark = 1;
    // The same ID may have returned, launched again or died after this receipt.
    assert!(state.validate_shape(&stock));
    stock[0].status = DroneStockStatus::Destroyed { at_ms: 100 };
    state.lanes[0].stored_drone_id = None;
    assert!(state.validate_shape(&stock));
    assert_eq!(state.operations[0].phase, IndependentBayPhase::Free);
}

#[test]
fn persisted_queued_work_cannot_bypass_begin_presence_or_class_rules() {
    let (mut state, mut stock) = fixture();
    state
        .operations
        .push(operation(&state, 1, 1, None, IndependentBayPhase::Queued));
    state.high_watermark = 1;
    assert!(state.validate_shape(&stock));
    stock[0].status = DroneStockStatus::Active;
    state.lanes[0].stored_drone_id = None;
    assert!(!state.validate_shape(&stock));
    state.operations[0].action = IndependentBayAction::ReturnToUnload {
        resume_mining: false,
    };
    assert!(!state.validate_shape(&stock));
    state.operations[0].action = IndependentBayAction::ReturnToStorage;
    assert!(state.validate_shape(&stock));
    stock[0].status = DroneStockStatus::Destroyed { at_ms: 10 };
    assert!(!state.validate_shape(&stock));
}
