use anyhow::{bail, ensure, Result};
use arkline_protocol::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BeginDisposition {
    Accepted,
    Retained,
}

/// Deduplication precedes current stock/eligibility. Authorization and exact
/// registration binding remain caller obligations before invoking this helper.
pub fn begin(
    state: &mut IndependentBayState,
    stock: &[DroneStockEntry],
    request: &IndependentBayRequest,
    now: u64,
    job_id: Option<u64>,
) -> Result<BeginDisposition> {
    let IndependentBayRequest::Begin {
        binding,
        operation_id,
        drone_id,
        action,
    } = request
    else {
        bail!("Expected identified independent bay Begin");
    };
    ensure!(
        binding == &state.binding,
        "Independent bay vessel/source mismatch"
    );
    if let Some(op) = state.operation(*operation_id) {
        ensure!(
            op.drone_id == *drone_id
                && op.action == *action
                && op.binding == *binding
                && op.job_id == job_id,
            "Independent bay operation ID payload conflict"
        );
        return Ok(BeginDisposition::Retained);
    }
    ensure!(
        state.validate_shape(stock),
        "Invalid independent bay ownership state"
    );
    ensure!(
        state.next_operation_id() == Some(*operation_id),
        "Expected next independent bay operation ID"
    );
    ensure!(
        state.active_operation(*drone_id).is_none(),
        "Drone already has an independent bay operation"
    );
    let entry = stock
        .iter()
        .find(|entry| entry.drone_id == *drone_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown finite drone identity"))?;
    match (action, &entry.status) {
        (IndependentBayAction::Launch, DroneStockStatus::Stored { .. }) => (),
        (
            IndependentBayAction::ReturnToUnload { .. } | IndependentBayAction::ReturnToStorage,
            DroneStockStatus::Active,
        ) => (),
        _ => bail!("Drone stock presence does not permit requested bay action"),
    }
    ensure!(
        !matches!(action, IndependentBayAction::ReturnToUnload { .. })
            || entry.class == DroneClass::Mining,
        "Only Mining drones have ore payloads"
    );
    state.operations.push(IndependentBayOperation {
        operation_id: *operation_id,
        drone_id: *drone_id,
        action: *action,
        binding: binding.clone(),
        assigned_bay: None,
        phase: IndependentBayPhase::Queued,
        held_phase: None,
        accepted_at_ms: now,
        updated_at_ms: now,
        job_id,
        message: "Waiting for a compatible clear bay".into(),
        cargo_service_operation_ids: vec![],
    });
    state.high_watermark = *operation_id;
    Ok(BeginDisposition::Accepted)
}

/// A candidate list is supplied only after the exact registered class mask,
/// complete route and same-tick conflict reservation checks succeed. This helper
/// does not reinterpret a boolean or endpoint as proof of geometric clearance.
/// The chosen resource is reserved atomically; callers must serialize conflicting
/// candidates using one tick's snapshot and stable operation-ID ordering.
pub fn reserve_lane(
    state: &mut IndependentBayState,
    stock: &[DroneStockEntry],
    operation_id: u64,
    checked_bays: &[u8],
    requires_ore_mount: bool,
    now: u64,
) -> Result<Option<u8>> {
    ensure!(
        state.validate_shape(stock),
        "Invalid independent bay ownership state"
    );
    let op = state
        .operation(operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown independent bay operation"))?;
    ensure!(
        op.phase == IndependentBayPhase::Queued,
        "Only queued operations may acquire a bay"
    );
    ensure!(
        now >= op.updated_at_ms,
        "Independent bay clock moved backwards"
    );
    ensure!(
        checked_bays
            .iter()
            .all(|bay| usize::from(*bay) < MAX_DRONES),
        "Invalid checked bay ID"
    );
    let launch = op.action == IndependentBayAction::Launch;
    // The source provider maps immutable home to physical bay. Preserve the
    // caller's checked preferred-first order; bay_id need not equal home_cradle.
    let selected = checked_bays.iter().copied().find(|bay| {
        let lane = &state.lanes[usize::from(*bay)];
        lane.operation_id.is_none()
            && lane.cleanup.is_none()
            && if requires_ore_mount {
                lane.components.transfer_available()
            } else {
                lane.components.handling_available()
            }
            && if launch {
                lane.stored_drone_id == Some(op.drone_id)
            } else {
                lane.stored_drone_id.is_none()
            }
    });
    let Some(bay) = selected else {
        return Ok(None);
    };
    let op = state
        .operations
        .iter_mut()
        .find(|op| op.operation_id == operation_id)
        .unwrap();
    op.assigned_bay = Some(bay);
    op.phase = if launch {
        IndependentBayPhase::Preparing
    } else {
        IndependentBayPhase::Approaching
    };
    op.updated_at_ms = now;
    op.message = "Assigned compatible clear bay".into();
    state.lanes[usize::from(bay)].operation_id = Some(operation_id);
    Ok(Some(bay))
}

/// Holding a physical operation releases its pump grant but not its physical bay.
/// Committed economic receipts and the vessel cooldown remain untouched. The
/// caller closes that typed cargo attempt in the same transaction.
pub fn hold_operation(
    state: &mut IndependentBayState,
    id: u64,
    now: u64,
    reason: &str,
) -> Result<()> {
    ensure!(reason.len() <= 512, "Hold reason is too long");
    let op = state
        .operation(id)
        .ok_or_else(|| anyhow::anyhow!("Unknown independent bay operation"))?;
    if op.phase.terminal() || op.phase == IndependentBayPhase::Held {
        return Ok(());
    }
    ensure!(
        now >= op.updated_at_ms,
        "Independent bay clock moved backwards"
    );
    let phase = op.phase;
    state.manifold.waiting.retain(|queued| *queued != id);
    if state
        .manifold
        .grant
        .as_ref()
        .is_some_and(|grant| grant.physical_operation_id == id)
    {
        state.manifold.grant = None;
    }
    let op = state
        .operations
        .iter_mut()
        .find(|op| op.operation_id == id)
        .unwrap();
    op.phase = IndependentBayPhase::Held;
    op.held_phase = Some(phase);
    op.updated_at_ms = now;
    op.message = reason.into();
    Ok(())
}

/// Only unstarted commands can become immediately terminal. A route-owning
/// cancellation must instead run the source-bound safe return/cleanup executor.
pub fn cancel_unstarted(state: &mut IndependentBayState, id: u64, now: u64) -> Result<bool> {
    let op = state
        .operations
        .iter_mut()
        .find(|op| op.operation_id == id)
        .ok_or_else(|| anyhow::anyhow!("Unknown independent bay operation"))?;
    if op.phase.terminal() {
        return Ok(true);
    }
    ensure!(
        now >= op.updated_at_ms,
        "Independent bay clock moved backwards"
    );
    if op.effective_phase() != IndependentBayPhase::Queued {
        return Ok(false);
    }
    op.phase = IndependentBayPhase::Cancelled;
    op.held_phase = None;
    op.updated_at_ms = now;
    op.message = "Cancelled before physical admission".into();
    Ok(true)
}
