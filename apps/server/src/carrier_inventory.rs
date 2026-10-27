//! Atomic finite inventory ownership. These helpers do not enable an assembly.
//! The eventual world executor must bind exact geometry and confirmed motion,
//! and persist inventory/cargo/ledger together in its existing SQL transaction.
use anyhow::{ensure, Context, Result};
use arkline_protocol::*;
mod motion;

pub fn begin_stage(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    request: &CarrierInventoryRequest,
    now: u64,
) -> Result<bool> {
    let CarrierInventoryRequest::Begin {
        binding,
        operation_id,
        drone_id,
        action,
    } = request
    else {
        anyhow::bail!("Expected identified Stage request");
    };
    ensure!(
        matches!(
            action,
            CarrierInventoryAction::Stage { .. } | CarrierInventoryAction::StoreReady { .. }
        ),
        "Expected cassette exchange request"
    );
    ensure!(
        binding == &state.binding,
        "Carrier inventory binding mismatch"
    );
    if let Some(op) = state.operation(*operation_id) {
        ensure!(
            op.drone_id == *drone_id && op.action == *action,
            "Operation payload conflict"
        );
        return Ok(false);
    }
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    ensure!(
        state.next_operation_id() == Some(*operation_id),
        "Expected next carrier operation ID"
    );
    ensure!(
        state.active_operation(*drone_id).is_none(),
        "Drone already has an operation"
    );
    let entry = state
        .entry(*drone_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown finite drone ID"))?;
    let (slot_id, berth_id) = match (action, &entry.location) {
        (
            CarrierInventoryAction::Stage { berth_id },
            CarrierDroneLocation::Protected { slot_id },
        ) => (*slot_id, *berth_id),
        (
            CarrierInventoryAction::StoreReady { slot_id },
            CarrierDroneLocation::Ready { berth_id },
        ) => (*slot_id, *berth_id),
        _ => anyhow::bail!("Exchange requires actual loaded source dock"),
    };
    ensure!(
        topology
            .stage_loader(slot_id, berth_id, entry.class)
            .is_some(),
        "No compatible same-side loader route"
    );
    ensure!(
        now >= state.initialized_at_ms,
        "Carrier clock moved backwards"
    );
    state.operations.push(CarrierInventoryOperation {
        recall_batch_id: None,
        operation_id: *operation_id,
        drone_id: *drone_id,
        action: *action,
        phase: CarrierOperationPhase::Queued,
        held_phase: None,
        stage: None,
        berth_id: None,
        accepted_at_ms: now,
        updated_at_ms: now,
        message: if matches!(action, CarrierInventoryAction::StoreReady { .. }) {
            "Waiting for empty protected cassette and compatible loader"
        } else {
            "Waiting for empty ready cassette and compatible loader"
        }
        .into(),
        cargo_service_operation_ids: vec![],
    });
    state.high_watermark = *operation_id;
    Ok(true)
}

/// Atomic ownership only: caller must first prove the complete source-bound
/// exchange clear. A reserved empty cassette is not additional drone capacity.
pub fn reserve_stage(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    operation_id: u64,
    now: u64,
) -> Result<bool> {
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    let op = state
        .operation(operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown operation"))?
        .clone();
    ensure!(
        op.phase == CarrierOperationPhase::Queued && now >= op.updated_at_ms,
        "Stage not queued or clock reversed"
    );
    let entry = state.entry(op.drone_id).unwrap();
    let (direction, slot_id, berth_id) = match (op.action, &entry.location) {
        (
            CarrierInventoryAction::Stage { berth_id },
            CarrierDroneLocation::Protected { slot_id },
        ) => (CarrierStageDirection::ToReady, *slot_id, berth_id),
        (
            CarrierInventoryAction::StoreReady { slot_id },
            CarrierDroneLocation::Ready { berth_id },
        ) => (CarrierStageDirection::ToProtected, slot_id, *berth_id),
        _ => anyhow::bail!("Exchange source no longer at accepted dock"),
    };
    let loader_id = topology
        .stage_loader(slot_id, berth_id, entry.class)
        .ok_or_else(|| anyhow::anyhow!("No compatible loader"))?;
    let buffer_id = topology.loader(loader_id).unwrap().transit_buffer_id;
    let loaded = state
        .docked_cassette(direction.loaded_source(slot_id, berth_id))
        .context("Loaded source cassette missing")?;
    let Some(empty) = state.docked_cassette(direction.loaded_destination(slot_id, berth_id)) else {
        return Ok(false);
    };
    if empty.drone_id.is_some()
        || state
            .docked_cassette(CarrierCassetteDock::TransitBuffer { buffer_id })
            .is_some()
    {
        return Ok(false);
    }
    let exchange = CarrierStageExchange {
        direction,
        loaded_cassette_id: loaded.cassette_id,
        empty_cassette_id: empty.cassette_id,
        source_slot_id: slot_id,
        destination_berth_id: berth_id,
        loader_id,
        buffer_id,
        completed_legs: 0,
        loaded_handoff_at_ms: None,
    };
    if exchange.resources().iter().any(|r| state.reserved(*r)) {
        return Ok(false);
    }
    for resource in exchange.resources() {
        state.reservations.push(CarrierResourceReservation {
            resource,
            operation_id,
            drone_id: op.drone_id,
        });
    }
    let retained = state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == operation_id)
        .unwrap();
    retained.stage = Some(exchange);
    retained.berth_id = Some(berth_id);
    retained.phase = CarrierOperationPhase::Loading;
    retained.updated_at_ms = now;
    retained.message = if direction.is_forward() {
        "Exchanging protected and empty ready cassettes"
    } else {
        "Exchanging loaded ready and empty protected cassettes"
    }
    .into();
    state
        .entries
        .iter_mut()
        .find(|e| e.drone_id == op.drone_id)
        .unwrap()
        .location = CarrierDroneLocation::Loading { operation_id };
    debug_assert!(state.validate_shape(topology));
    Ok(true)
}

fn current_leg(
    state: &CarrierInventoryState,
    operation_id: u64,
) -> Result<(u64, CarrierCassetteDock, CarrierCassetteDock)> {
    let op = state
        .operation(operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown operation"))?;
    ensure!(
        matches!(
            op.phase,
            CarrierOperationPhase::Loading | CarrierOperationPhase::Securing
        ) || (op.phase == CarrierOperationPhase::Destroyed
            && state
                .cleanups
                .iter()
                .any(|c| c.operation_id == operation_id && !c.held)),
        "Exchange is not advancing"
    );
    let x = op
        .stage
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("No retained exchange"))?;
    x.current_leg().context("Exchange legs already complete")
}
/// Called only after the motion executor accepts the exact current leg at its
/// actual entry pose. It is not a request handler and grants no endpoint shortcut.
pub fn start_stage_leg(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    operation_id: u64,
) -> Result<()> {
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    let (id, from, to) = current_leg(state, operation_id)?;
    let cassette = state
        .cassettes
        .iter_mut()
        .find(|c| c.cassette_id == id)
        .unwrap();
    ensure!(
        cassette.location == (CarrierCassetteLocation::Docked { dock: from }),
        "Cassette is not at the accepted source"
    );
    cassette.location = CarrierCassetteLocation::InLoader {
        operation_id,
        from,
        to,
    };
    debug_assert!(state.validate_shape(topology));
    Ok(())
}
/// Record one already source-confirmed physical endpoint. Runtime must call this
/// only after authoritative drone/cassette/loader support and clocks agree.
pub fn record_stage_endpoint(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    operation_id: u64,
    now: u64,
) -> Result<()> {
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    let (id, from, to) = current_leg(state, operation_id)?;
    ensure!(
        now >= state.operation(operation_id).unwrap().updated_at_ms,
        "Carrier clock reversed"
    );
    let cassette = state
        .cassettes
        .iter_mut()
        .find(|c| c.cassette_id == id)
        .unwrap();
    ensure!(
        cassette.location
            == (CarrierCassetteLocation::InLoader {
                operation_id,
                from,
                to
            }),
        "No accepted cassette leg to complete"
    );
    cassette.location = CarrierCassetteLocation::Docked { dock: to };
    let op = state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == operation_id)
        .unwrap();
    op.stage.as_mut().unwrap().completed_legs += 1;
    op.updated_at_ms = now;
    debug_assert!(state.validate_shape(topology));
    Ok(())
}
/// Release only the loaded cassette and ready berth after the exact source head.
/// Empty cassette/loader/source dock/buffer remain owned by this same journal ID.
fn record_stage_handoff(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    operation_id: u64,
    now: u64,
) -> Result<()> {
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    let op = state
        .operation(operation_id)
        .context("Missing handoff owner")?
        .clone();
    let x = op.stage.as_ref().context("No Stage handoff")?;
    if let Some(at) = x.loaded_handoff_at_ms {
        ensure!(at <= now, "Handoff clock reversed");
        return Ok(());
    }
    ensure!(
        state.binding.registration.assembly_revision == 1702
            && x.direction.is_forward()
            && x.completed_legs == 2
            && op.phase == CarrierOperationPhase::Loading
            && now >= op.updated_at_ms,
        "Stage is not at its reviewed handoff"
    );
    let retained = state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == operation_id)
        .unwrap();
    retained.stage.as_mut().unwrap().loaded_handoff_at_ms = Some(now);
    retained.phase = CarrierOperationPhase::Securing;
    retained.updated_at_ms = now;
    retained.message = "Loaded cassette Ready; empty cassette/loader securing separately".into();
    let retained_resources = retained.stage.as_ref().unwrap().retained_resources();
    state
        .reservations
        .retain(|r| r.operation_id != operation_id || retained_resources.contains(&r.resource));
    state
        .entries
        .iter_mut()
        .find(|e| e.drone_id == op.drone_id)
        .unwrap()
        .location = CarrierDroneLocation::Ready {
        berth_id: x.destination_berth_id,
    };
    ensure!(
        state.validate_shape(topology),
        "Invalid released Stage ownership"
    );
    Ok(())
}

/// Loader-safe confirmation is deliberately separate from reaching the ready
/// berth: after leg two the drone remains Loading while the empty cassette exits.
pub fn record_stage_secured(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    operation_id: u64,
    now: u64,
) -> Result<()> {
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    let op = state
        .operation(operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown operation"))?
        .clone();
    ensure!(
        matches!(
            op.phase,
            CarrierOperationPhase::Loading | CarrierOperationPhase::Securing
        ) && op.stage.as_ref().is_some_and(|s| s.completed_legs == 3)
            && now >= op.updated_at_ms,
        "Exchange not fully secure"
    );
    if !op.released_loaded_owner() {
        state
            .entries
            .iter_mut()
            .find(|e| e.drone_id == op.drone_id)
            .unwrap()
            .location = match op.stage.as_ref().unwrap().loaded_destination() {
            CarrierCassetteDock::Ready { berth_id } => CarrierDroneLocation::Ready { berth_id },
            CarrierCassetteDock::Protected { slot_id } => {
                CarrierDroneLocation::Protected { slot_id }
            }
            CarrierCassetteDock::TransitBuffer { .. } => unreachable!(),
        };
    }
    let op = state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == operation_id)
        .unwrap();
    op.phase = CarrierOperationPhase::Completed;
    op.updated_at_ms = now;
    op.message = "Destination cassette locked; loader clear".into();
    state
        .reservations
        .retain(|r| r.operation_id != operation_id);
    debug_assert!(state.validate_shape(topology));
    Ok(())
}

#[cfg(test)]
mod tests;

pub fn hold_stage(state: &mut CarrierInventoryState, operation_id: u64, now: u64) -> Result<()> {
    let op = state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown operation"))?;
    ensure!(now >= op.updated_at_ms, "Carrier clock reversed");
    if let Some(cleanup) = state
        .cleanups
        .iter_mut()
        .find(|c| c.operation_id == operation_id)
    {
        cleanup.held = true;
        cleanup.reason = "Empty cassette cleanup held".into();
        return Ok(());
    }
    if op.phase.terminal() || op.phase == CarrierOperationPhase::Held {
        return Ok(());
    }
    op.held_phase = Some(op.phase);
    op.phase = CarrierOperationPhase::Held;
    op.updated_at_ms = now;
    op.message = "Physical cassette exchange held".into();
    Ok(())
}
pub fn resume_stage(state: &mut CarrierInventoryState, operation_id: u64, now: u64) -> Result<()> {
    let op = state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown operation"))?;
    ensure!(now >= op.updated_at_ms, "Carrier clock reversed");
    if let Some(cleanup) = state
        .cleanups
        .iter_mut()
        .find(|c| c.operation_id == operation_id)
    {
        cleanup.held = false;
        cleanup.reason.clear();
        return Ok(());
    }
    if op.phase != CarrierOperationPhase::Held {
        return Ok(());
    }
    op.phase = op.held_phase.take().unwrap();
    op.updated_at_ms = now;
    op.message = "Resuming retained exchange after route revalidation".into();
    Ok(())
}
/// The owning world transaction must debit any payload to its destruction sink
/// exactly once before persisting this same-identity physical transition.
#[cfg(test)] // Protected-body damage integration is not enabled in this launch milestone.
pub fn mark_carried_destroyed(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    operation_id: u64,
    now: u64,
) -> Result<()> {
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    let op = state
        .operation(operation_id)
        .ok_or_else(|| anyhow::anyhow!("Unknown operation"))?
        .clone();
    if op.phase == CarrierOperationPhase::Destroyed {
        return Ok(());
    }
    ensure!(
        matches!(
            op.effective_phase(),
            CarrierOperationPhase::Loading | CarrierOperationPhase::Securing
        ) && op.stage.is_some()
            && now >= op.updated_at_ms,
        "No carried drone operation"
    );
    let entry = state
        .entries
        .iter_mut()
        .find(|e| e.drone_id == op.drone_id)
        .unwrap();
    entry.hit_points = 0;
    entry.location = CarrierDroneLocation::Destroyed { at_ms: now };
    for cassette in &mut state.cassettes {
        if cassette.drone_id == Some(op.drone_id) {
            cassette.drone_id = None;
        }
    }
    let retained = state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == operation_id)
        .unwrap();
    retained.phase = CarrierOperationPhase::Destroyed;
    retained.held_phase = None;
    retained.updated_at_ms = now;
    retained.message = "Destroyed; empty hardware cleanup retains resources".into();
    state.cleanups.push(CarrierHardwareCleanup {
        operation_id,
        held: op.phase == CarrierOperationPhase::Held,
        reason: "Empty cassette cleanup".into(),
    });
    debug_assert!(state.validate_shape(topology));
    Ok(())
}
pub fn record_cleanup_secured(
    state: &mut CarrierInventoryState,
    topology: &CarrierInventoryTopology,
    operation_id: u64,
) -> Result<()> {
    ensure!(state.validate_shape(topology), "Invalid carrier inventory");
    ensure!(
        state
            .operation(operation_id)
            .is_some_and(|op| op.phase == CarrierOperationPhase::Destroyed
                && op.stage.as_ref().is_some_and(|s| s.completed_legs == 3))
            && state
                .cleanups
                .iter()
                .any(|c| c.operation_id == operation_id && !c.held),
        "Cleanup not fully secure"
    );
    state.cleanups.retain(|c| c.operation_id != operation_id);
    state
        .reservations
        .retain(|r| r.operation_id != operation_id);
    debug_assert!(state.validate_shape(topology));
    Ok(())
}

pub(crate) mod world;
