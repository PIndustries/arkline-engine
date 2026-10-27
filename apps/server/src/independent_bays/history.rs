//! Reopen-time cross-binding of physical grants, economic attempts and committed
//! ledger evidence. This does not replace the independent economic verifier or
//! prove physical contact. It prevents a mutable pump timestamp from erasing the
//! real vessel-global cooldown after a handoff or restart.
use anyhow::{ensure, Context, Result};
use arkline_protocol::*;
use std::collections::{BTreeMap, BTreeSet};

pub fn validate_manifold_history(
    pilot: &str,
    state: &IndependentBayState,
    cargo: &DroneCargoState,
    ledger: &[LedgerEntry],
) -> Result<()> {
    ensure!(
        cargo.valid_shape(),
        "Invalid cargo journal for independent manifold"
    );
    ensure!(
        cargo.binding.vessel_id == state.binding.vessel_id
            && cargo.binding.registration.base == state.binding.registration.host,
        "Manifold cargo vessel/host mismatch"
    );
    ensure!(
        cargo.cleanup.is_none(),
        "Independent bay cannot hide legacy receiver cleanup"
    );
    let mut linked = BTreeSet::new();
    for physical in &state.operations {
        for id in &physical.cargo_service_operation_ids {
            ensure!(
                linked.insert(*id),
                "Economic attempt linked to multiple physical operations"
            );
            let service = cargo
                .operation(*id)
                .context("Retained manifold attempt is missing")?;
            ensure!(
                service.drone_id == physical.drone_id && service.binding == cargo.binding,
                "Physical/economic attempt identity mismatch"
            );
            ensure!(
                service.accepted_at_ms >= physical.accepted_at_ms,
                "Economic attempt predates physical operation"
            );
            ensure!(
                !physical.phase.terminal() || service.updated_at_ms <= physical.updated_at_ms,
                "Terminal physical result predates linked economic service"
            );
        }
    }
    ensure!(
        linked.len() == cargo.operations.len(),
        "Orphan economic service outside independent lanes"
    );
    let grant_id = state
        .manifold
        .grant
        .as_ref()
        .map(|g| g.cargo_service_operation_id);
    ensure!(
        cargo.receiver_operation == grant_id,
        "Physical/economic pump ownership mismatch"
    );
    for op in &cargo.operations {
        ensure!(
            op.phase.terminal() || Some(op.operation_id) == grant_id,
            "Unowned active economic service"
        );
    }
    if let Some(grant) = &state.manifold.grant {
        let op = cargo
            .operation(grant.cargo_service_operation_id)
            .context("Granted economic attempt missing")?;
        ensure!(
            op.phase == DroneDropoffPhase::Transferring && op.accepted_at_ms == grant.granted_at_ms,
            "Grant clock/phase differs from economic attempt"
        );
    }
    let entries: BTreeMap<_, _> = ledger.iter().map(|entry| (entry.sequence, entry)).collect();
    ensure!(entries.len() == ledger.len(), "Duplicate ledger sequence");
    let mut receipts = BTreeSet::new();
    let mut last = None;
    for op in &cargo.operations {
        for receipt in &op.receipts {
            ensure!(
                receipts.insert(receipt.ledger_sequence),
                "Transfer receipt reused"
            );
            let entry = entries
                .get(&receipt.ledger_sequence)
                .context("Transfer receipt ledger entry missing")?;
            ensure!(
                entry.ruleset_version == DRONE_CARGO_RULESET_VERSION
                    && entry.hash == receipt.ledger_hash
                    && entry.hash == entry.calculate_hash()
                    && entry.timestamp_ms == receipt.committed_at_ms,
                "Transfer receipt ledger hash/time mismatch"
            );
            ensure!(
                matches!(&entry.event, EconomicEvent::DroneCargo { record: DroneCargoLedgerEvent::Transferred {
                pilot: owner, vessel_id, drone_id, operation_id, operation_started_at_ms, requested_units, sequence, units
            }} if owner == pilot && vessel_id == &cargo.binding.vessel_id && *drone_id == op.drone_id
                && *operation_id == op.operation_id && *operation_started_at_ms == op.accepted_at_ms
                && *requested_units == op.requested_units && *sequence == receipt.sequence && *units == receipt.units),
                "Transfer receipt payload differs from committed ledger"
            );
            last = Some(last.map_or(receipt.committed_at_ms, |old: u64| {
                old.max(receipt.committed_at_ms)
            }));
        }
    }
    let mut previous = None;
    for entry in entries.values() {
        if let EconomicEvent::DroneCargo {
            record:
                DroneCargoLedgerEvent::Transferred {
                    pilot: owner,
                    vessel_id,
                    operation_id,
                    ..
                },
        } = &entry.event
        {
            if owner != pilot || vessel_id != &cargo.binding.vessel_id {
                continue;
            }
            ensure!(
                receipts.contains(&entry.sequence),
                "Committed manifold transfer erased from retained history"
            );
            if let Some((time, id)) = previous {
                ensure!(
                    entry
                        .timestamp_ms
                        .checked_sub(time)
                        .is_some_and(|elapsed| elapsed >= DRONE_DROPOFF_BATCH_MS)
                        && *operation_id >= id,
                    "Vessel-global manifold cadence or attempt order changed"
                );
            }
            previous = Some((entry.timestamp_ms, *operation_id));
        }
    }
    ensure!(
        state.manifold.last_committed_at_ms == last,
        "Shared manifold cooldown differs from committed receipts"
    );
    Ok(())
}
