//! Finite persistent drone ownership. Physical routes are a separate, registered
//! geometry contract. All mutation functions run inside the caller's world tx.
use super::*;
mod batches;
#[cfg(test)]
mod integration_tests;
pub(super) mod runtime;
pub(super) fn batch_active(w: &World, name: &str) -> bool {
    batches::active(w, name).is_some()
}
pub(super) fn hold_service_batch(
    w: &mut World,
    name: &str,
    operation: u64,
    now: u64,
    reason: &str,
) {
    if batches::active(w, name).is_some_and(|b| b.current_service_operation_id == Some(operation)) {
        batches::hold(w, name, now, reason);
    }
}
use arkline_protocol::drone_bay_geometry::BayPose;
pub(super) use runtime::execute;
pub(super) use runtime::{
    advance, advance_with_deferred_batch, authorize, busy, carrier_origins, constrained,
    create_pilot_stock, creation_registration, finish, hold, project, state, stop_all, validate,
    Runtime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BeginOutcome {
    Accepted,
    Replayed,
}

/// Only the explicit fresh-preview creation path may call this. Reconnect and
/// cleanup must never manufacture replacement identities from an empty count.
pub(super) fn initialize(
    binding: DroneBayBinding,
    first_id: u64,
    now: u64,
) -> Result<(DroneBayState, u64)> {
    ensure!(binding.well_formed(), "invalid drone bay binding");
    ensure!(first_id > 0, "invalid initial drone identity");
    let next_id = first_id
        .checked_add(DRONE_BAY_INITIAL_STOCK as u64)
        .context("drone identity overflow")?;
    let state = DroneBayState {
        batches: None,
        cleanup: None,
        schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
        binding,
        initialization: DroneBayInitialization {
            fixture_version: 1,
            seeded_at_ms: now,
            first_drone_id: first_id,
        },
        stock: (0..DRONE_BAY_INITIAL_STOCK)
            .map(|slot| DroneStockEntry {
                drone_id: first_id + slot as u64,
                class: DroneBayState::expected_class(slot as u8).unwrap(),
                home_cradle: slot as u8,
                status: DroneStockStatus::Stored {
                    hit_points: DRONE_HIT_POINTS,
                },
            })
            .collect(),
        high_watermark: 0,
        operations: vec![],
        handling_operation: None,
    };
    validate_inventory(&state, &[], next_id)?;
    Ok((state, next_id))
}

/// Idempotent initialization is called only for a newly-created owned vessel in
/// an opted-in fresh world. Presence of a receipt prohibits replacement seeding.
pub(super) fn initialize_once(
    inventories: &mut BTreeMap<String, DroneBayState>,
    binding: DroneBayBinding,
    next_id: &mut u64,
    now: u64,
) -> Result<bool> {
    if let Some(existing) = inventories.get(&binding.vessel_id) {
        ensure!(
            existing.binding == binding && existing.stock.len() == DRONE_BAY_INITIAL_STOCK,
            "existing drone initialization receipt is incompatible or corrupt"
        );
        return Ok(false);
    }
    let (inventory, next) = initialize(binding.clone(), *next_id, now)?;
    inventories.insert(binding.vessel_id, inventory);
    *next_id = next;
    Ok(true)
}

fn retry(
    state: &DroneBayState,
    binding: &DroneBayBinding,
    id: u64,
    drone: u64,
    kind: DroneBayOperationKind,
) -> Result<bool> {
    ensure!(
        drone_bay_schema_supported(state.schema_version) && state.binding == *binding,
        "drone bay source or vessel binding mismatch"
    );
    if let Some(op) = state.operation(id) {
        ensure!(
            op.drone_id == drone && op.kind == kind && op.binding == *binding,
            "drone bay operation ID reused with different payload"
        );
        return Ok(true);
    }
    ensure!(
        id == state
            .high_watermark
            .checked_add(1)
            .context("drone bay operation ID exhausted")?,
        "new drone bay operation ID must be the confirmed high watermark plus one"
    );
    Ok(false)
}
fn accept(
    state: &mut DroneBayState,
    id: u64,
    drone: u64,
    kind: DroneBayOperationKind,
    phase: DroneBayPhase,
    job_id: Option<u64>,
    now: u64,
) {
    state.operations.push(DroneBayOperation {
        operation_id: id,
        drone_id: drone,
        kind,
        binding: state.binding.clone(),
        phase,
        held_phase: None,
        accepted_at_ms: now,
        updated_at_ms: now,
        job_id,
        message: phase.label().into(),
    });
    state.high_watermark = id;
}

/// Claim the already-owned entity only after the caller verifies source geometry,
/// stationary parent, path and authorization. No new ID or HP is allocated here.
pub(super) fn launch(
    state: &mut DroneBayState,
    active: &mut Vec<DroneView>,
    request: &DroneBayRequest,
    pose: BayPose,
    ship_position: [f64; 3],
    job_id: Option<u64>,
    now: u64,
) -> Result<BeginOutcome> {
    let DroneBayRequest::Launch {
        binding,
        drone_id,
        operation_id,
    } = request
    else {
        bail!("expected identified launch");
    };
    // Deduplication precedes current availability/job checks. An old accepted
    // request must not claim another drone or cancel a newer linked job.
    if retry(
        state,
        binding,
        *operation_id,
        *drone_id,
        DroneBayOperationKind::Launch,
    )? {
        return Ok(BeginOutcome::Replayed);
    }
    ensure!(
        state.handling_operation.is_none(),
        "drone bay handling route is busy"
    );
    ensure!(
        state.active_operation(*drone_id).is_none(),
        "drone already has an active bay operation"
    );
    let index = state
        .stock
        .iter()
        .position(|s| s.drone_id == *drone_id)
        .context("drone is not owned by this vessel")?;
    let entry = &state.stock[index];
    let DroneStockStatus::Stored { hit_points } = entry.status else {
        bail!("selected drone is not stored; destroyed drones cannot be replaced");
    };
    ensure!(
        (1..=DRONE_HIT_POINTS).contains(&hit_points),
        "invalid stored drone health"
    );
    ensure!(
        active.len() < MAX_DRONES
            && active.iter().filter(|d| d.class == entry.class).count() < DRONE_CLASS_CAP,
        "deployed drone fleet cap reached"
    );
    ensure!(
        !active.iter().any(|d| d.id == *drone_id),
        "drone identity is already active"
    );
    let heading = pose
        .transform_vector([1., 0., 0.])
        .context("invalid accepted launch pose")?;
    ensure!(
        pose.position_m
            .iter()
            .chain(ship_position.iter())
            .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND),
        "invalid accepted launch position"
    );
    let drone = DroneView {
        orientation_xyzw: Some(pose.orientation_xyzw),
        id: *drone_id,
        class: entry.class,
        hit_points,
        signal_strength: drone_signal_strength(pose.position_m, ship_position),
        mining_cycle: None,
        attack_ready_at_ms: None,
        position: pose.position_m,
        velocity: [0.; 3],
        heading,
        mode: DroneMode::Hold,
        updated_at_ms: now,
    };
    state.stock[index].status = DroneStockStatus::Active;
    active.push(drone);
    accept(
        state,
        *operation_id,
        *drone_id,
        DroneBayOperationKind::Launch,
        DroneBayPhase::ReleasingCradle,
        job_id,
        now,
    );
    state.handling_operation = Some(*operation_id);
    Ok(BeginOutcome::Accepted)
}

pub(super) fn recover(
    state: &mut DroneBayState,
    active: &mut [DroneView],
    request: &DroneBayRequest,
    job_id: Option<u64>,
    now: u64,
) -> Result<BeginOutcome> {
    let DroneBayRequest::Recover {
        binding,
        drone_id,
        operation_id,
    } = request
    else {
        bail!("expected identified recovery");
    };
    if retry(
        state,
        binding,
        *operation_id,
        *drone_id,
        DroneBayOperationKind::Recover,
    )? {
        return Ok(BeginOutcome::Replayed);
    }
    ensure!(
        state.active_operation(*drone_id).is_none(),
        "drone already has an active bay operation"
    );
    ensure!(
        state
            .stock
            .iter()
            .any(|e| e.drone_id == *drone_id && e.status == DroneStockStatus::Active),
        "selected drone is not an active owned entity"
    );
    let drone = active
        .iter_mut()
        .find(|d| d.id == *drone_id)
        .context("active owned drone missing")?;
    ensure!(drone.hit_points > 0, "destroyed drone cannot recover");
    drone.mode = DroneMode::Returning;
    drone.mining_cycle = None;
    drone.attack_ready_at_ms = None;
    // Preserve its actual world pose and velocity; the physical controller must
    // bring it to the approach gate, not snap it to a cradle or zero its motion.
    accept(
        state,
        *operation_id,
        *drone_id,
        DroneBayOperationKind::Recover,
        DroneBayPhase::Returning,
        job_id,
        now,
    );
    Ok(BeginOutcome::Accepted)
}

pub(super) fn release(state: &mut DroneBayState, operation_id: u64, now: u64) -> Result<bool> {
    let index = state
        .operations
        .iter()
        .position(|op| op.operation_id == operation_id)
        .context("bay operation missing")?;
    if state.operations[index].phase == DroneBayPhase::Free {
        return Ok(false);
    }
    ensure!(
        state.operations[index].phase == DroneBayPhase::Departing
            && state.handling_operation == Some(operation_id),
        "drone has not cleared the bay"
    );
    let op = &mut state.operations[index];
    op.phase = DroneBayPhase::Free;
    op.updated_at_ms = now;
    op.message = op.phase.label().into();
    state.handling_operation = None;
    Ok(true)
}

/// Call only after the motion/collision controller confirms the exact reserved
/// home capture. Incoming damage resolves BEFORE this transfer in the same tx.
pub(super) fn capture(
    state: &mut DroneBayState,
    active: &mut Vec<DroneView>,
    operation_id: u64,
    cancelled: bool,
    now: u64,
) -> Result<bool> {
    let op_index = state
        .operations
        .iter()
        .position(|op| op.operation_id == operation_id)
        .context("bay operation missing")?;
    let op = &state.operations[op_index];
    if matches!(op.phase, DroneBayPhase::Stored | DroneBayPhase::Cancelled) {
        return Ok(false);
    }
    ensure!(
        op.phase == DroneBayPhase::SecuringCradle && state.handling_operation == Some(operation_id),
        "home cradle capture is not confirmed"
    );
    let stock_index = state
        .stock
        .iter()
        .position(|e| e.drone_id == op.drone_id)
        .context("owned home cradle missing")?;
    ensure!(
        state.stock[stock_index].status == DroneStockStatus::Active,
        "home drone is not active"
    );
    let drone_index = active
        .iter()
        .position(|d| d.id == op.drone_id)
        .context("recovering drone no longer exists")?;
    ensure!(
        active[drone_index].hit_points > 0 && active[drone_index].hit_points <= DRONE_HIT_POINTS,
        "destroyed drone cannot be captured or resurrected"
    );
    ensure!(
        active[drone_index].class == state.stock[stock_index].class,
        "recovering drone class changed"
    );
    let drone = active.remove(drone_index);
    state.stock[stock_index].status = DroneStockStatus::Stored {
        hit_points: drone.hit_points,
    };
    let op = &mut state.operations[op_index];
    op.phase = if cancelled {
        DroneBayPhase::Cancelled
    } else {
        DroneBayPhase::Stored
    };
    op.held_phase = None;
    op.updated_at_ms = now;
    op.message = op.phase.label().into();
    state.handling_operation = None;
    Ok(true)
}

/// One irreversible terminal transition. Empty cradles never manufacture stock.
pub(super) fn destroy(
    state: &mut DroneBayState,
    active: &mut Vec<DroneView>,
    drone_id: u64,
    now: u64,
) -> Result<bool> {
    let index = state
        .stock
        .iter()
        .position(|e| e.drone_id == drone_id)
        .context("destroyed drone outside owned roster")?;
    if matches!(
        state.stock[index].status,
        DroneStockStatus::Destroyed { .. }
    ) {
        ensure!(
            !active.iter().any(|d| d.id == drone_id),
            "destroyed drone identity resurrected"
        );
        return Ok(false);
    }
    ensure!(
        state.stock[index].status == DroneStockStatus::Active,
        "only an active damaged drone can be destroyed"
    );
    let drone_index = active
        .iter()
        .position(|d| d.id == drone_id)
        .context("destroyed active drone missing")?;
    ensure!(
        active[drone_index].hit_points == 0,
        "healthy drone cannot be destroyed by cleanup"
    );
    active.remove(drone_index);
    state.stock[index].status = DroneStockStatus::Destroyed { at_ms: now };
    for op in state
        .operations
        .iter_mut()
        .filter(|op| op.drone_id == drone_id && !op.phase.terminal())
    {
        op.phase = DroneBayPhase::Destroyed;
        op.held_phase = None;
        op.updated_at_ms = now;
        op.message = "Drone destroyed; home cradle remains empty".into();
        if state.handling_operation == Some(op.operation_id) {
            state.handling_operation = None;
        }
    }
    Ok(true)
}

pub(super) fn validate_inventory(
    state: &DroneBayState,
    active: &[DroneView],
    next_id: u64,
) -> Result<()> {
    batches::validate_inventory(state)?;
    ensure!(
        drone_bay_schema_supported(state.schema_version) && state.binding.well_formed(),
        "unsupported drone inventory binding"
    );
    ensure!(
        state.initialization.fixture_version == 1 && state.initialization.first_drone_id > 0,
        "invalid one-time drone initialization receipt"
    );
    let end = state
        .initialization
        .first_drone_id
        .checked_add(DRONE_BAY_INITIAL_STOCK as u64)
        .context("initial drone identity overflow")?;
    ensure!(
        end <= next_id && state.stock.len() == DRONE_BAY_INITIAL_STOCK,
        "finite drone stock was added or removed"
    );
    let mut cradles = std::collections::BTreeSet::new();
    for entry in &state.stock {
        ensure!(
            cradles.insert(entry.home_cradle)
                && DroneBayState::expected_class(entry.home_cradle) == Some(entry.class)
                && entry.drone_id
                    == state.initialization.first_drone_id + u64::from(entry.home_cradle),
            "drone home identity or initial class changed"
        );
        let deployed: Vec<_> = active.iter().filter(|d| d.id == entry.drone_id).collect();
        match entry.status {
            DroneStockStatus::Stored { hit_points } => ensure!(
                deployed.is_empty() && (1..=DRONE_HIT_POINTS).contains(&hit_points),
                "stored drone has duplicate body or invalid health"
            ),
            DroneStockStatus::Active => ensure!(
                deployed.len() == 1
                    && deployed[0].class == entry.class
                    && (1..=DRONE_HIT_POINTS).contains(&deployed[0].hit_points),
                "active drone lost identity or health"
            ),
            DroneStockStatus::Destroyed { .. } => {
                ensure!(deployed.is_empty(), "destroyed drone reappeared")
            }
        }
    }
    ensure!(
        active.len() <= MAX_DRONES
            && active.iter().all(|d| state
                .stock
                .iter()
                .any(|e| e.drone_id == d.id && e.status == DroneStockStatus::Active)),
        "active drone outside finite roster"
    );
    for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
        let counts = state.counts(Some(class));
        ensure!(
            counts.total() == DRONE_CLASS_CAP && counts.active <= DRONE_CLASS_CAP,
            "drone class conservation failed"
        );
    }
    ensure!(
        state.high_watermark == state.operations.len() as u64,
        "drone operation journal gap"
    );
    if let Some(c) = &state.cleanup {
        ensure!(
            state.handling_operation.is_none()
                && state
                    .operation(c.operation_id)
                    .is_some_and(|op| op.phase == DroneBayPhase::Destroyed),
            "cleanup cannot revive a live drone operation"
        );
    }
    let mut live = std::collections::BTreeSet::new();
    for (i, op) in state.operations.iter().enumerate() {
        ensure!(
            op.operation_id == i as u64 + 1
                && op.binding == state.binding
                && state.stock.iter().any(|s| s.drone_id == op.drone_id),
            "invalid durable drone operation identity"
        );
        ensure!(
            op.updated_at_ms >= op.accepted_at_ms,
            "drone operation clock moved backwards"
        );
        if op.phase == DroneBayPhase::Destroyed {
            ensure!(
                state.stock.iter().any(|e| e.drone_id == op.drone_id
                    && matches!(e.status, DroneStockStatus::Destroyed { .. })),
                "destroyed operation identity cannot reappear in stored or active stock"
            );
        }
        if !op.phase.terminal() {
            ensure!(
                live.insert(op.drone_id)
                    && state
                        .stock
                        .iter()
                        .any(|s| s.drone_id == op.drone_id && s.status == DroneStockStatus::Active),
                "multiple active operations or missing active drone"
            );
        }
        ensure!(
            (op.phase == DroneBayPhase::Held) == op.held_phase.is_some()
                && op
                    .held_phase
                    .is_none_or(|p| p != DroneBayPhase::Held && !p.terminal()),
            "invalid held bay phase"
        );
    }
    if let Some(id) = state.handling_operation {
        ensure!(
            state.operation(id).is_some_and(|op| !op.phase.terminal()),
            "orphaned drone handling reservation"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> DroneBayBinding {
        DroneBayBinding {
            vessel_id: "actual-fixture-vessel".into(),
            geometry_revision: 12,
            source_sha256: "a".repeat(64),
        }
    }
    fn pose() -> BayPose {
        BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    fn fixture() -> (DroneBayState, Vec<DroneView>, u64) {
        let (state, next) = initialize(binding(), 51, 1000).unwrap();
        (state, vec![], next)
    }
    fn launch_request(operation_id: u64, drone_id: u64) -> DroneBayRequest {
        DroneBayRequest::Launch {
            binding: binding(),
            drone_id,
            operation_id,
        }
    }
    fn launch_first(state: &mut DroneBayState, active: &mut Vec<DroneView>, op: u64, now: u64) {
        assert_eq!(
            launch(
                state,
                active,
                &launch_request(op, 51),
                pose(),
                [0.; 3],
                Some(7),
                now
            )
            .unwrap(),
            BeginOutcome::Accepted
        );
    }
    fn finish_launch(state: &mut DroneBayState, op: u64, now: u64) {
        state
            .operations
            .iter_mut()
            .find(|o| o.operation_id == op)
            .unwrap()
            .phase = DroneBayPhase::Departing;
        assert!(release(state, op, now).unwrap());
    }
    fn wire(state: &DroneBayState, active: &[DroneView]) -> serde_json::Value {
        serde_json::to_value((state, active)).unwrap()
    }
    #[test]
    fn initialization_seeds_exactly_fifteen_once_and_never_repairs_destroyed_stock() {
        let mut inventories = BTreeMap::new();
        let mut next = 51;
        assert!(initialize_once(&mut inventories, binding(), &mut next, 1000).unwrap());
        assert_eq!(next, 66);
        let state = inventories.get_mut(&binding().vessel_id).unwrap();
        assert_eq!(
            state.counts(None),
            DroneStockCounts {
                stored: 15,
                active: 0,
                destroyed: 0
            }
        );
        for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
            assert_eq!(state.counts(Some(class)).stored, 5);
        }
        state.stock[0].status = DroneStockStatus::Destroyed { at_ms: 1001 };
        state.stock[1].status = DroneStockStatus::Stored { hit_points: 23 };
        let before = serde_json::to_value(&inventories).unwrap();
        assert!(!initialize_once(&mut inventories, binding(), &mut next, 999999).unwrap());
        assert_eq!(before, serde_json::to_value(&inventories).unwrap());
        assert_eq!(next, 66);
        validate_inventory(&inventories[&binding().vessel_id], &[], next).unwrap();
        inventories
            .get_mut(&binding().vessel_id)
            .unwrap()
            .stock
            .clear();
        assert!(initialize_once(&mut inventories, binding(), &mut next, 999999).is_err());
        assert_eq!(next, 66);
    }
    #[test]
    fn initialization_overflow_and_invalid_binding_do_not_allocate_ids() {
        assert!(initialize(binding(), u64::MAX - 14, 0).is_err());
        assert!(initialize(binding(), 0, 0).is_err());
        let mut bad = binding();
        bad.source_sha256 = "wrong".into();
        assert!(initialize(bad, 1, 0).is_err());
    }
    #[test]
    fn launch_recovery_and_relaunch_preserve_same_health_identity_and_cradle() {
        let (mut state, mut active, next) = fixture();
        state.stock[0].status = DroneStockStatus::Stored { hit_points: 37 };
        launch_first(&mut state, &mut active, 1, 1001);
        assert_eq!(active[0].id, 51);
        assert_eq!(active[0].hit_points, 37);
        assert_eq!(state.stock[0].home_cradle, 0);
        validate_inventory(&state, &active, next).unwrap();
        finish_launch(&mut state, 1, 2000);
        active[0].hit_points = 23;
        let request = DroneBayRequest::Recover {
            binding: binding(),
            drone_id: 51,
            operation_id: 2,
        };
        active[0].velocity = [5., 1., 0.];
        let position = active[0].position;
        assert_eq!(
            recover(&mut state, &mut active, &request, None, 2100).unwrap(),
            BeginOutcome::Accepted
        );
        assert_eq!(active[0].position, position);
        assert_eq!(active[0].velocity, [5., 1., 0.]);
        state.handling_operation = Some(2);
        state.operations[1].phase = DroneBayPhase::SecuringCradle;
        assert!(capture(&mut state, &mut active, 2, false, 3000).unwrap());
        assert!(active.is_empty());
        assert_eq!(
            state.stock[0].status,
            DroneStockStatus::Stored { hit_points: 23 }
        );
        assert!(!capture(&mut state, &mut active, 2, false, 3001).unwrap());
        launch_first(&mut state, &mut active, 3, 4000);
        assert_eq!((active[0].id, active[0].hit_points), (51, 23));
        validate_inventory(&state, &active, next).unwrap();
        let serialized = serde_json::to_string(&(state.clone(), active.clone())).unwrap();
        let (restored, restored_active): (DroneBayState, Vec<DroneView>) =
            serde_json::from_str(&serialized).unwrap();
        validate_inventory(&restored, &restored_active, next).unwrap();
        assert_eq!(wire(&state, &active), wire(&restored, &restored_active));
    }
    #[test]
    fn exact_retry_precedes_busy_and_reused_identity_rejects_without_mutation() {
        let (mut state, mut active, next) = fixture();
        launch_first(&mut state, &mut active, 1, 1001);
        let before = wire(&state, &active);
        assert_eq!(
            launch(
                &mut state,
                &mut active,
                &launch_request(1, 51),
                pose(),
                [0.; 3],
                Some(999),
                1002
            )
            .unwrap(),
            BeginOutcome::Replayed
        );
        assert_eq!(wire(&state, &active), before);
        assert_eq!(state.operations[0].job_id, Some(7));
        for req in [
            launch_request(1, 52),
            launch_request(3, 52),
            launch_request(2, 52),
        ] {
            assert!(launch(&mut state, &mut active, &req, pose(), [0.; 3], None, 1003).is_err());
            assert_eq!(wire(&state, &active), before);
        }
        validate_inventory(&state, &active, next).unwrap();
    }
    #[test]
    fn damage_before_capture_is_terminal_and_releases_route_once_without_replacement() {
        let (mut state, mut active, next) = fixture();
        launch_first(&mut state, &mut active, 1, 1001);
        state.operations[0].phase = DroneBayPhase::SecuringCradle;
        active[0].hit_points = 0;
        assert!(capture(&mut state, &mut active, 1, true, 2000).is_err());
        assert!(destroy(&mut state, &mut active, 51, 2000).unwrap());
        assert!(!destroy(&mut state, &mut active, 51, 2001).unwrap());
        assert_eq!(state.handling_operation, None);
        assert_eq!(state.operations[0].phase, DroneBayPhase::Destroyed);
        assert_eq!(
            state.counts(None),
            DroneStockCounts {
                stored: 14,
                active: 0,
                destroyed: 1
            }
        );
        let before = wire(&state, &active);
        assert!(capture(&mut state, &mut active, 1, false, 3000).is_err());
        assert!(launch(
            &mut state,
            &mut active,
            &launch_request(2, 51),
            pose(),
            [0.; 3],
            None,
            3000
        )
        .is_err());
        assert_eq!(
            launch(
                &mut state,
                &mut active,
                &launch_request(1, 51),
                pose(),
                [0.; 3],
                None,
                3000
            )
            .unwrap(),
            BeginOutcome::Replayed
        );
        assert_eq!(wire(&state, &active), before);
        validate_inventory(&state, &active, next).unwrap();
    }
    #[test]
    fn complete_finite_fleet_still_supports_fifteen_active_and_five_per_class() {
        let (mut state, mut active, next) = fixture();
        for i in 0..15 {
            let id = 51 + i;
            let op = 1 + i;
            assert_eq!(
                launch(
                    &mut state,
                    &mut active,
                    &launch_request(op, id),
                    pose(),
                    [0.; 3],
                    None,
                    1000 + op
                )
                .unwrap(),
                BeginOutcome::Accepted
            );
            finish_launch(&mut state, op, 1000 + op);
            validate_inventory(&state, &active, next).unwrap();
        }
        assert_eq!(active.len(), 15);
        assert_eq!(state.counts(None).active, 15);
        for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
            assert_eq!(state.counts(Some(class)).active, 5);
        }
        let before = wire(&state, &active);
        assert!(launch(
            &mut state,
            &mut active,
            &launch_request(16, 999),
            pose(),
            [0.; 3],
            None,
            2000
        )
        .is_err());
        assert_eq!(wire(&state, &active), before);
    }
    #[test]
    fn inventory_validation_detects_duplicate_copy_loss_and_identity_substitution() {
        let (mut state, mut active, next) = fixture();
        launch_first(&mut state, &mut active, 1, 1001);
        let original = state.clone();
        state.stock[0].status = DroneStockStatus::Stored { hit_points: 100 };
        assert!(validate_inventory(&state, &active, next).is_err());
        state = original.clone();
        active.push(active[0].clone());
        assert!(validate_inventory(&state, &active, next).is_err());
        active.pop();
        state.stock[1].drone_id = 51;
        assert!(validate_inventory(&state, &active, next).is_err());
        state = original.clone();
        state.stock.remove(14);
        assert!(validate_inventory(&state, &active, next).is_err());
        assert!(validate_inventory(&original, &[], next).is_err());
    }
}
