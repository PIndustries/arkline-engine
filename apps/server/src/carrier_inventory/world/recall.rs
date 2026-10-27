//! Finite capture/storage queue. No discard, ore shortcut, home-class quota or
//! buffer capacity. Physical children use the same actual-pose resource graph.
use super::*;
fn ids(m: &CarrierRecallMember) -> impl Iterator<Item = u64> {
    [
        m.service_operation_id,
        m.capture_operation_id,
        m.store_operation_id,
    ]
    .into_iter()
    .flatten()
}
fn busy(s: &OwnedCarrier, m: &CarrierRecallMember) -> bool {
    s.departures.contains_key(&m.drone_id)
        || ids(m).any(|id| {
            s.runs.contains_key(&id)
                || s.arrivals.contains_key(&id)
                || s.inventory.cleanups.iter().any(|c| c.operation_id == id)
                || s.inventory
                    .operation(id)
                    .is_some_and(|o| !o.phase.terminal())
        })
}
pub(super) fn execute(
    w: &mut World,
    name: &str,
    request: &CarrierInventoryRequest,
    now: u64,
) -> Result<bool> {
    let (binding, id, kind) = match request {
        CarrierInventoryRequest::RecallAll { binding, batch_id } => (binding, *batch_id, 0),
        CarrierInventoryRequest::HoldRecall { binding, batch_id } => (binding, *batch_id, 1),
        CarrierInventoryRequest::ResumeRecall { binding, batch_id } => (binding, *batch_id, 2),
        CarrierInventoryRequest::CancelRecallRemaining { binding, batch_id } => {
            (binding, *batch_id, 3)
        }
        _ => return Ok(false),
    };
    let def = definition(w)?.context("Carrier source missing")?;
    let mut s = state(w, name).context("Carrier missing")?.clone();
    ensure!(
        *binding == s.inventory.binding,
        "Recall vessel/source mismatch"
    );
    let index = s.inventory.recalls.iter().position(|b| b.batch_id == id);
    if kind == 0 {
        // Retry is resolved before current work/eligibility, never takes new stock.
        if index.is_some() {
            return Ok(true);
        }
        ensure!(
            matches!(def.topology.registration.assembly_revision, 1702 | 1703),
            "Recall/storage source not admitted"
        );
        ensure!(
            s.inventory.next_recall_id() == Some(id)
                && s.inventory.active_recall().is_none()
                && s.inventory.active_deployment().is_none(),
            "Expected next idle Recall All ID"
        );
        ensure!(
            !w.resource_jobs.contains_key(name)
                && s.inventory.operations.iter().all(|o| o.phase.terminal())
                && s.runs.is_empty()
                && s.arrivals.is_empty()
                && s.departures.is_empty()
                && s.inventory.cleanups.is_empty(),
            "Finish existing physical routine/departures before Recall All"
        );
        let members = s
            .inventory
            .entries
            .iter()
            .filter(|e| !matches!(e.location, CarrierDroneLocation::Destroyed { .. }))
            .map(|e| CarrierRecallMember {
                drone_id: e.drone_id,
                accepted_location: e.location.clone(),
                service_operation_id: None,
                capture_operation_id: None,
                store_operation_id: None,
                cancelled: false,
                blocked_reason: String::new(),
                completed_location: None,
            })
            .collect::<Vec<_>>();
        ensure!(
            !members.is_empty(),
            "No surviving finite inventory to recall"
        );
        // Stop accepted free mining intent/claims before a physical return is
        // queued. Payload remains same-ID owned; no extraction races the batch.
        cargo::stop(&mut s)?;
        for m in &members {
            s.mining_blocks.remove(&m.drone_id);
        }
        for body in &mut w.pilots.get_mut(name).unwrap().view.drones {
            if members.iter().any(|m| m.drone_id == body.id) {
                body.mode = DroneMode::Hold;
                body.velocity = [0.; 3];
                body.mining_cycle = None;
                body.attack_ready_at_ms = None;
                body.updated_at_ms = now;
            }
        }
        drones::sync(w.pilots.get_mut(name).unwrap());
        s.inventory.recalls.push(CarrierRecallBatch {
            batch_id: id,
            members,
            phase: CarrierRecallPhase::Running,
            held: false,
            accepted_at_ms: now,
            updated_at_ms: now,
        });
        s.inventory.recall_high_watermark = id;
    } else {
        let index = index.context("Unknown retained recall")?;
        let b = s.inventory.recalls[index].clone();
        if !b.phase.active() {
            return Ok(true);
        }
        ensure!(now >= b.updated_at_ms, "Recall clock reversed");
        for child in b.members.iter().flat_map(ids) {
            match kind {
                1 => {
                    service::close_attempt(&mut s, child, now)?;
                    hold_stage(&mut s.inventory, child, now)?;
                    if let Some(r) = s.runs.get_mut(&child) {
                        r.hold("Recall All held");
                    }
                    if let Some(a) = s.arrivals.get_mut(&child) {
                        a.hold("Recall All held");
                    }
                }
                2 => {
                    if let Some(r) = s.runs.get_mut(&child) {
                        if r.held {
                            r.resume_validated(now)?;
                        }
                    }
                    if let Some(a) = s.arrivals.get_mut(&child) {
                        if a.held {
                            a.resume(now)?;
                        }
                    }
                    resume_stage(&mut s.inventory, child, now)?;
                }
                _ => {
                    if s.inventory
                        .operation(child)
                        .is_some_and(|o| o.effective_phase() == CarrierOperationPhase::Queued)
                    {
                        phase(
                            &mut s,
                            child,
                            CarrierOperationPhase::Cancelled,
                            now,
                            "Recall cancelled before reservation",
                        );
                    }
                }
            }
        }
        if kind == 1 {
            s.inventory.recalls[index].held = true;
        }
        if kind == 2 {
            s.inventory.recalls[index].held = false;
        }
        if kind == 3 {
            s.inventory.recalls[index].phase = CarrierRecallPhase::Cancelling;
        }
        s.inventory.recalls[index].updated_at_ms = now;
    }
    service::reconcile(&mut s, now)?;
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    drive(w, name, now)?;
    simulation::admit(w, name, now)?;
    Ok(true)
}
pub(super) fn drive(w: &mut World, name: &str, now: u64) -> Result<()> {
    let Some(def) = definition(w)? else {
        return Ok(());
    };
    let current = state(w, name).context("Carrier missing")?;
    let Some(index) = current
        .inventory
        .recalls
        .iter()
        .position(|b| b.phase.active())
    else {
        return Ok(());
    };
    let mut s = current.clone();
    let mut batch = s.inventory.recalls[index].clone();
    for m in &mut batch.members {
        if m.cancelled {
            continue;
        }
        let e = s
            .inventory
            .entry(m.drone_id)
            .context("Recall identity missing")?
            .clone();
        if busy(&s, m) {
            m.blocked_reason = s
                .inventory
                .active_operation(m.drone_id)
                .filter(|o| {
                    o.effective_phase() == CarrierOperationPhase::Queued
                        || o.phase == CarrierOperationPhase::Held
                })
                .map(|o| o.message.clone())
                .unwrap_or_default();
            continue;
        }
        if matches!(e.location, CarrierDroneLocation::Destroyed { .. }) {
            continue;
        }
        if batch.phase == CarrierRecallPhase::Cancelling {
            // Cancel only remaining work; already secured identities retain
            // their actual successful endpoint, including a finished capture.
            if e.location.dock().is_some() {
                m.blocked_reason.clear();
                continue;
            }
            m.cancelled = true;
            m.blocked_reason = "Cancelled remaining recall".into();
            continue;
        }
        if batch.held {
            continue;
        }
        let action = match e.location {
            CarrierDroneLocation::Protected { .. } => {
                m.blocked_reason.clear();
                continue;
            }
            CarrierDroneLocation::Ready { berth_id } => {
                if m.store_operation_id.is_some() {
                    continue;
                }
                let slot = def.topology.magazine_slots.iter().find(|slot| {
                    def.topology
                        .stage_loader(slot.slot_id, berth_id, e.class)
                        .is_some()
                        && !s.inventory.reserved(CarrierResource::MagazineSlot {
                            slot_id: slot.slot_id,
                        })
                        && s.inventory
                            .docked_cassette(CarrierCassetteDock::Protected {
                                slot_id: slot.slot_id,
                            })
                            .is_some_and(|c| c.drone_id.is_none())
                        && !s.inventory.operations.iter().any(|o| {
                            !o.phase.terminal()
                                && o.action
                                    == CarrierInventoryAction::StoreReady {
                                        slot_id: slot.slot_id,
                                    }
                        })
                });
                let Some(slot) = slot else {
                    m.blocked_reason.clear();
                    continue;
                };
                CarrierInventoryAction::StoreReady {
                    slot_id: slot.slot_id,
                }
            }
            CarrierDroneLocation::Deployed => {
                if m.capture_operation_id.is_some() {
                    m.blocked_reason =
                        "Retained capture did not complete; no automatic replacement".into();
                    continue;
                }
                let payload = s.ore.holds.iter().find(|h| h.drone_id == m.drone_id);
                if payload.is_some_and(|h| h.ore_units > 0 || h.cycle.is_some()) {
                    if m.service_operation_id.is_some() {
                        m.blocked_reason =
                            "Retained unloading did not complete; no automatic replacement".into();
                        continue;
                    }
                    if !cargo::source_supports_service(def) {
                        m.blocked_reason =
                            "Loaded return requires source-admitted ore service".into();
                        continue;
                    }
                    CarrierInventoryAction::ReturnToUnload {
                        resume_mining: false,
                    }
                } else {
                    if m.service_operation_id.is_some_and(|id| {
                        s.inventory
                            .operation(id)
                            .is_none_or(|op| op.phase != CarrierOperationPhase::Completed)
                    }) {
                        m.blocked_reason =
                            "Retained unloading did not complete; capture remains blocked".into();
                        continue;
                    }
                    CarrierInventoryAction::RecallToStorage
                }
            }
            _ => {
                m.blocked_reason = "Waiting for actual physical ownership".into();
                continue;
            }
        };
        let id = s
            .inventory
            .next_operation_id()
            .context("Operation IDs exhausted")?;
        // Append child and its owner link atomically below. No intermediate
        // partially linked journal may pass durable validation or be published.
        s.inventory.operations.push(CarrierInventoryOperation {
            recall_batch_id: Some(batch.batch_id),
            operation_id: id,
            drone_id: m.drone_id,
            action,
            phase: CarrierOperationPhase::Queued,
            held_phase: None,
            stage: None,
            berth_id: None,
            accepted_at_ms: now,
            updated_at_ms: now,
            message: if matches!(action, CarrierInventoryAction::StoreReady { .. }) {
                "Recall All: queued empty protected cassette and compatible loader"
            } else if matches!(action, CarrierInventoryAction::ReturnToUnload { .. }) {
                "Recall All: queued physical ore service before storage"
            } else {
                "Recall All: queued compatible empty capture"
            }
            .into(),
            cargo_service_operation_ids: vec![],
        });
        s.inventory.high_watermark = id;
        if matches!(action, CarrierInventoryAction::StoreReady { .. }) {
            m.store_operation_id = Some(id);
        } else if matches!(action, CarrierInventoryAction::ReturnToUnload { .. }) {
            m.service_operation_id = Some(id);
        } else {
            m.capture_operation_id = Some(id);
        }
        m.blocked_reason.clear();
    }
    let done = batch.members.iter().all(|m| {
        !busy(&s, m)
            && (m.cancelled
                || s.inventory.entry(m.drone_id).is_some_and(|e| {
                    e.location.dock().is_some()
                        || matches!(e.location, CarrierDroneLocation::Destroyed { .. })
                }))
    });
    if done {
        batch.phase = if batch.phase == CarrierRecallPhase::Cancelling {
            CarrierRecallPhase::Cancelled
        } else {
            CarrierRecallPhase::Completed
        };
        batch.held = false;
        for m in &mut batch.members {
            if !m.cancelled {
                m.completed_location =
                    Some(s.inventory.entry(m.drone_id).unwrap().location.clone());
            }
        }
    }
    batch.updated_at_ms = now;
    s.inventory.recalls[index] = batch;
    ensure!(
        s.inventory.validate_shape(&def.topology),
        "Recall queue produced inconsistent child ownership"
    );
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    Ok(())
}
