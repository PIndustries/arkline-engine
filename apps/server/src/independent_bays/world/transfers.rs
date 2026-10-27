//! The one vessel manifold uses existing v2 economic attempts and exact appended
//! ledger receipts. Concurrent physical mounts cannot multiply economic cadence.
use super::*;
fn cargo_mut<'a>(w: &'a mut World, name: &str, vessel: &str) -> &'a mut DroneCargoState {
    w.accounts
        .get_mut(name)
        .unwrap()
        .drone_cargo
        .get_mut(vessel)
        .unwrap()
}
pub(super) fn close_attempt(
    w: &mut World,
    name: &str,
    s: &mut OwnedBays,
    id: u64,
    now: u64,
    complete: bool,
) -> Result<()> {
    let grant = s
        .state
        .manifold
        .grant
        .clone()
        .context("No retained manifold attempt")?;
    ensure!(
        grant.physical_operation_id == id,
        "Wrong physical pump owner"
    );
    let cargo = cargo_mut(w, name, &s.state.binding.vessel_id);
    let op = cargo
        .operations
        .iter_mut()
        .find(|o| o.operation_id == grant.cargo_service_operation_id)
        .context("Pump economic attempt missing")?;
    op.phase = if complete {
        ensure!(op.remaining_units == 0, "Cannot complete loaded transfer");
        DroneDropoffPhase::Completed
    } else if cargo
        .holds
        .iter()
        .find(|h| h.drone_id == op.drone_id)
        .is_some_and(|h| h.destroyed_at_ms.is_some())
    {
        DroneDropoffPhase::Destroyed
    } else {
        DroneDropoffPhase::Cancelled
    };
    op.held_phase = None;
    op.updated_at_ms = now;
    op.next_batch_at_ms = None;
    cargo.receiver_operation = None;
    finish_manifold_attempt(&mut s.state, id, now)
}
pub(crate) fn commit_transfers(
    w: &mut World,
    tx: &rusqlite::Transaction<'_>,
    entries: &mut Vec<LedgerEntry>,
    now: u64,
) -> Result<()> {
    if definition(w)?.is_none() {
        return Ok(());
    }
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let Some(mut s) = state(w, &name).cloned() else {
            continue;
        };
        let eligible: Vec<_> = s
            .runs
            .iter()
            .filter_map(|(id, run)| match &run.movement {
                Movement::Handling(r)
                    if r.waiting_for_transfer
                        && !r.held
                        && !run.cleanup_only
                        && !run.cancel_requested =>
                {
                    Some(*id)
                }
                _ => None,
            })
            .collect();
        if let Some(id) = next_manifold_waiter(&s.state, &eligible) {
            let op = s.state.operation(id).unwrap().clone();
            let cargo = cargo_mut(w, &name, &s.state.binding.vessel_id);
            let units = cargo
                .holds
                .iter()
                .find(|h| h.drone_id == op.drone_id)
                .unwrap()
                .ore_units;
            ensure!(units > 0, "Empty payload cannot claim manifold");
            let service = cargo
                .high_watermark
                .checked_add(1)
                .context("Cargo service ID exhausted")?;
            cargo.operations.push(DroneDropoffOperation {
                operation_id: service,
                binding: cargo.binding.clone(),
                drone_id: op.drone_id,
                requested_resume_mining: false,
                cancel_requested: false,
                followup: DroneDropoffFollowup::Hold,
                requested_units: units,
                committed_units: 0,
                remaining_units: units,
                phase: DroneDropoffPhase::Transferring,
                held_phase: None,
                accepted_at_ms: now,
                updated_at_ms: now,
                next_batch_at_ms: Some(
                    now.checked_add(DRONE_DROPOFF_BATCH_MS)
                        .context("Transfer clock exhausted")?,
                ),
                message: "Source-confirmed independent mount; shared manifold".into(),
                receipts: vec![],
            });
            cargo.high_watermark = service;
            cargo.receiver_operation = Some(service);
            grant_manifold(&mut s.state, id, service, now)?;
        }
        if manifold_batch_due(&s.state, now) {
            let grant = s.state.manifold.grant.clone().unwrap();
            let id = grant.physical_operation_id;
            let op = s.state.operation(id).unwrap().clone();
            let Movement::Handling(run) = &s.runs[&id].movement else {
                bail!("Transfer cannot occur during exterior entry")
            };
            ensure!(
                run.waiting_for_transfer
                    && !run.held
                    && run.reached.coupled
                    && !s.runs[&id].cleanup_only
                    && w.pilots[&name]
                        .view
                        .drones
                        .iter()
                        .any(|d| d.id == op.drone_id && d.hit_points > 0),
                "Transfer requires live exact confirmed contact after combat"
            );
            ensure!(
                route_clear(w, &name, &s, id, now, now)?,
                "Transfer contact is obstructed"
            );
            let old = drone_cargo::state(w, &name)
                .unwrap()
                .operation(grant.cargo_service_operation_id)
                .unwrap()
                .clone();
            let units = old.remaining_units.min(DRONE_DROPOFF_BATCH_UNITS);
            ensure!(
                units > 0 && drone_cargo::payload(w, &name, op.drone_id) == old.remaining_units,
                "Reserved physical payload changed"
            );
            let next = w.pilots[&name]
                .view
                .ore
                .checked_add(units)
                .context("Ship ore overflow")?;
            ensure!(
                next <= cargo_capacity(w.pilots[&name].view.ship_class),
                "Incoming ship headroom lost"
            );
            cargo_mut(w, &name, &s.state.binding.vessel_id)
                .holds
                .iter_mut()
                .find(|h| h.drone_id == op.drone_id)
                .unwrap()
                .ore_units -= units;
            w.pilots.get_mut(&name).unwrap().view.ore = next;
            let sequence = old.receipts.len() as u64 + 1;
            crate::append_event(
                tx,
                entries,
                EconomicEvent::DroneCargo {
                    record: DroneCargoLedgerEvent::Transferred {
                        pilot: name.clone(),
                        vessel_id: s.state.binding.vessel_id.clone(),
                        drone_id: op.drone_id,
                        operation_id: old.operation_id,
                        operation_started_at_ms: old.accepted_at_ms,
                        requested_units: old.requested_units,
                        sequence,
                        units,
                    },
                },
                now,
                DRONE_CARGO_RULESET_VERSION,
            )?;
            let entry = entries.last().unwrap();
            let cargo = cargo_mut(w, &name, &s.state.binding.vessel_id);
            let attempt = cargo
                .operations
                .iter_mut()
                .find(|o| o.operation_id == old.operation_id)
                .unwrap();
            attempt.committed_units += units;
            attempt.remaining_units -= units;
            attempt.updated_at_ms = now;
            attempt.next_batch_at_ms = Some(
                now.checked_add(DRONE_DROPOFF_BATCH_MS)
                    .context("Transfer deadline overflow")?,
            );
            attempt.receipts.push(DroneOreTransferReceipt {
                sequence,
                units,
                committed_at_ms: now,
                ledger_sequence: entry.sequence,
                ledger_hash: entry.hash.clone(),
            });
            let complete = attempt.remaining_units == 0;
            record_manifold_commit(&mut s.state, id, units, now)?;
            if complete {
                close_attempt(w, &name, &mut s, id, now, true)?;
                if let Movement::Handling(run) = &mut s.runs.get_mut(&id).unwrap().movement {
                    run.release_transfer_barrier(now)?;
                }
            }
        }
        validate_manifold_history(
            &name,
            &s.state,
            drone_cargo::state(w, &name).unwrap(),
            entries,
        )?;
        w.accounts.get_mut(&name).unwrap().independent_bays = Some(s);
    }
    Ok(())
}
