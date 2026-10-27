use anyhow::{ensure, Result};
use arkline_protocol::*;

/// Called only after the registered contact/clamp pose is physically confirmed.
/// This records scheduling intent; it never credits ore or proves contact itself.
pub fn wait_for_manifold(
    state: &mut IndependentBayState,
    operation_id: u64,
    now: u64,
) -> Result<()> {
    let op = state
        .operation(operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown independent bay operation"))?;
    ensure!(
        matches!(
            op.phase,
            IndependentBayPhase::Coupling | IndependentBayPhase::WaitingForManifold
        ),
        "Operation is not confirmed coupled"
    );
    ensure!(
        op.action != IndependentBayAction::Launch,
        "Launch cannot own ore manifold"
    );
    ensure!(
        now >= op.updated_at_ms,
        "Independent bay clock moved backwards"
    );
    let bay = op
        .assigned_bay
        .ok_or_else(|| anyhow::anyhow!("Missing assigned bay"))?;
    ensure!(
        state.lanes[usize::from(bay)].operation_id == Some(operation_id),
        "Bay ownership mismatch"
    );
    if !state.manifold.waiting.contains(&operation_id) {
        state.manifold.waiting.push(operation_id);
    }
    let op = state
        .operations
        .iter_mut()
        .find(|op| op.operation_id == operation_id)
        .unwrap();
    op.phase = IndependentBayPhase::WaitingForManifold;
    op.updated_at_ms = now;
    op.message = "Coupled, waiting for shared transfer".into();
    Ok(())
}

/// Stable FIFO among currently eligible coupled lanes. A held/unavailable lane
/// cannot monopolize the pump. Current grant stays finite (one <=100-unit payload
/// attempt); the existing cargo journal supplies its unique next service ID.
pub fn next_manifold_waiter(state: &IndependentBayState, eligible: &[u64]) -> Option<u64> {
    if state.manifold.grant.is_some() {
        return None;
    }
    state.manifold.waiting.iter().copied().find(|id| {
        eligible.contains(id)
            && state.operation(*id).is_some_and(|op| {
                op.phase == IndependentBayPhase::WaitingForManifold
                    && op.assigned_bay.is_some_and(|bay| {
                        let lane = &state.lanes[usize::from(bay)];
                        lane.operation_id == Some(*id) && lane.components.transfer_available()
                    })
            })
    })
}

/// Caller atomically creates this exact attempt in DroneCargoState first (on a
/// transaction candidate) and then records its link here. No ore changes here.
pub fn grant_manifold(
    state: &mut IndependentBayState,
    operation_id: u64,
    service_id: u64,
    now: u64,
) -> Result<()> {
    ensure!(
        state.manifold.grant.is_none(),
        "Shared ore manifold already owned"
    );
    ensure!(
        service_id > 0
            && state
                .operations
                .iter()
                .flat_map(|op| &op.cargo_service_operation_ids)
                .all(|old| *old < service_id),
        "Cargo service ID must advance monotonically"
    );
    let op = state
        .operation(operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown independent bay operation"))?;
    ensure!(
        op.phase == IndependentBayPhase::WaitingForManifold
            && state.manifold.waiting.contains(&operation_id),
        "Operation is not waiting for manifold"
    );
    ensure!(
        now >= op.updated_at_ms,
        "Independent bay clock moved backwards"
    );
    let bay = op
        .assigned_bay
        .ok_or_else(|| anyhow::anyhow!("Missing assigned bay"))?;
    ensure!(
        state.lanes[usize::from(bay)].operation_id == Some(operation_id)
            && state.lanes[usize::from(bay)]
                .components
                .transfer_available(),
        "Bay cannot own manifold"
    );
    state.manifold.waiting.retain(|id| *id != operation_id);
    state.manifold.grant = Some(IndependentBayManifoldGrant {
        physical_operation_id: operation_id,
        cargo_service_operation_id: service_id,
        bay_id: bay,
        granted_at_ms: now,
    });
    let op = state
        .operations
        .iter_mut()
        .find(|op| op.operation_id == operation_id)
        .unwrap();
    op.cargo_service_operation_ids.push(service_id);
    op.phase = IndependentBayPhase::Transferring;
    op.updated_at_ms = now;
    op.message = "Transferring confirmed ore".into();
    Ok(())
}

/// The first batch waits a full500ms from actual grant, and no handoff can evade
/// the vessel-global cooldown. Long gaps never multiply batches or catch up.
pub fn manifold_batch_due(state: &IndependentBayState, now: u64) -> bool {
    state.manifold.grant.as_ref().is_some_and(|grant| {
        grant
            .granted_at_ms
            .checked_add(DRONE_DROPOFF_BATCH_MS)
            .is_some_and(|due| now >= due)
            && state.manifold.last_committed_at_ms.is_none_or(|last| {
                last.checked_add(DRONE_DROPOFF_BATCH_MS)
                    .is_some_and(|due| now >= due)
            })
    })
}

/// Records the cooldown only after an actual <=25-unit transfer, inventory
/// mutation and ledger/receipt append succeeded in the same candidate transaction.
pub fn record_manifold_commit(
    state: &mut IndependentBayState,
    operation_id: u64,
    units: u64,
    now: u64,
) -> Result<()> {
    ensure!(
        state
            .manifold
            .grant
            .as_ref()
            .is_some_and(|g| g.physical_operation_id == operation_id),
        "Wrong manifold owner"
    );
    ensure!(
        (1..=DRONE_DROPOFF_BATCH_UNITS).contains(&units) && manifold_batch_due(state, now),
        "Shared manifold batch is not due or exceeds25units"
    );
    state.manifold.last_committed_at_ms = Some(now);
    Ok(())
}

pub fn finish_manifold_attempt(
    state: &mut IndependentBayState,
    operation_id: u64,
    now: u64,
) -> Result<()> {
    ensure!(
        state
            .manifold
            .grant
            .as_ref()
            .is_some_and(|g| g.physical_operation_id == operation_id),
        "Wrong manifold owner"
    );
    let op = state.operation(operation_id).unwrap();
    ensure!(
        now >= op.updated_at_ms,
        "Independent bay clock moved backwards"
    );
    state.manifold.grant = None;
    let op = state
        .operations
        .iter_mut()
        .find(|op| op.operation_id == operation_id)
        .unwrap();
    op.phase = IndependentBayPhase::Uncoupling;
    op.updated_at_ms = now;
    op.message = "Disconnecting ore mount".into();
    Ok(())
}
