//! Timed ore movement. Every batch uses the existing economic transfer and is
//! committed with its exact ledger receipt inside the caller's SQLite transaction.
use super::*;
pub(super) fn view(w: &World, name: &str) -> Option<CargoUnloadingState> {
    docking::enabled(w).then(|| w.cargo_unloading.get(name).cloned().unwrap_or_default())
}
pub(super) fn active<'a>(w: &'a World, name: &str) -> Option<&'a CargoUnloadOperation> {
    w.cargo_unloading.get(name)?.active()
}
fn secured<'a>(w: &'a World, name: &str) -> Result<&'a DockingState> {
    ensure!(
        docking::enabled(w),
        "timed unloading requires the DockingV1 capability"
    );
    let p = w.pilots.get(name).context("source pilot missing")?;
    let d = docking::state(p).context("assigned berth missing")?;
    ensure!(
        p.view.location == Location::Docked && d.secured(),
        "unloading requires confirmed Secured docking"
    );
    ensure!(
        p.view.drones.is_empty(),
        "recall all drones before reserving the cargo hold"
    );
    Ok(d)
}
fn same_destination(
    a: &CargoUnloadOperation,
    destination: CargoUnloadDestination,
    owner: &str,
    system: u32,
) -> bool {
    a.phase.active()
        && a.destination == destination
        && a.destination_owner == owner
        && a.system_id == system
}
fn check_binding(w: &World, op: &CargoUnloadOperation) -> Result<()> {
    let d = secured(w, &op.pilot)?;
    let p = &w.pilots[&op.pilot].view;
    ensure!(
        p.ship_class == op.ship_class
            && p.system_id == op.system_id
            && d.berth_id == op.berth_id
            && w.accounts[&op.pilot].vessel_id(p.ship_class) == Some(op.vessel_id.as_str()),
        "unload source vessel or berth changed"
    );
    let (owner, stored) = assets::unload_destination(w, &op.pilot, op.destination)?;
    ensure!(
        owner == op.destination_owner,
        "unload destination ownership changed"
    );
    ensure!(p.ore >= op.reserved_units, "reserved cargo is unavailable");
    ensure!(
        stored.checked_add(op.reserved_units).is_some(),
        "destination storage overflow"
    );
    Ok(())
}
pub(super) fn begin(
    w: &mut World,
    name: &str,
    id: u64,
    units: u64,
    destination: CargoUnloadDestination,
    now: u64,
    automated: bool,
) -> Result<()> {
    ensure!(
        docking::enabled(w),
        "timed unloading requires the DockingV1 capability"
    );
    if let Some(state) = w.cargo_unloading.get(name) {
        if let Some(op) = state.operations.iter().find(|op| op.operation_id == id) {
            ensure!(
                op.requested_units == units && op.destination == destination,
                "operation ID reused with different unload payload"
            );
            // A retry never resumes, starts a second transfer or revises its binding.
            return Ok(());
        }
    }
    if !automated {
        ensure!(
            w.resource_jobs
                .get(name)
                .is_none_or(|j| j.view.phase.is_terminal()),
            "resource job owns cargo; cancel the job first"
        );
    }
    let high = w.cargo_unloading.get(name).map_or(0, |s| s.high_watermark);
    ensure!(
        id == high
            .checked_add(1)
            .context("unload operation IDs exhausted")?,
        "unload operation ID must follow confirmed high-water mark"
    );
    ensure!(
        active(w, name).is_none(),
        "cargo hold already reserved by an unload operation"
    );
    let berth_id = secured(w, name)?.berth_id;
    let p = &w.pilots[name].view;
    ensure!(
        units > 0 && units <= p.ore,
        "unload quantity exceeds available cargo or is zero"
    );
    ensure!(
        p.ore <= cargo_capacity(p.ship_class),
        "cargo capacity exceeded"
    );
    let (owner, stored) = assets::unload_destination(w, name, destination)?;
    ensure!(
        stored.checked_add(units).is_some(),
        "destination storage overflow"
    );
    ensure!(
        !w.cargo_unloading
            .values()
            .flat_map(|s| &s.operations)
            .any(|op| same_destination(op, destination, &owner, p.system_id)),
        "destination storage reserved by another unload operation"
    );
    let op = CargoUnloadOperation {
        operation_id: id,
        pilot: name.into(),
        vessel_id: w.accounts[name]
            .vessel_id(p.ship_class)
            .context("source vessel missing")?
            .into(),
        ship_class: p.ship_class,
        system_id: p.system_id,
        berth_id,
        destination,
        destination_owner: owner,
        requested_units: units,
        committed_units: 0,
        reserved_units: units,
        phase: CargoUnloadPhase::Unloading,
        next_batch_at_ms: now
            .checked_add(CARGO_UNLOAD_BATCH_MS)
            .context("unload clock overflow")?,
        job_id: if automated {
            Some(
                w.resource_jobs
                    .get(name)
                    .context("delivery job missing")?
                    .view
                    .id,
            )
        } else {
            None
        },
        message: "Unloading reserved; awaiting first confirmed batch".into(),
        receipts: vec![],
    };
    let state = w.cargo_unloading.entry(name.into()).or_default();
    state.high_watermark = id;
    state.operations.push(op);
    Ok(())
}
pub(super) fn cancel(w: &mut World, name: &str, id: u64) -> Result<()> {
    let op = w
        .cargo_unloading
        .get_mut(name)
        .and_then(|s| s.operations.iter_mut().find(|op| op.operation_id == id))
        .context("unload operation not found for this pilot")?;
    if op.phase.active() {
        op.phase = CargoUnloadPhase::Cancelled;
        op.reserved_units = 0;
        op.message =
            "Cancelled; confirmed batches retained, uncommitted reservation released".into();
    }
    Ok(())
}
pub(super) fn cancel_job(w: &mut World, name: &str) {
    if let Some(id) = active(w, name)
        .filter(|op| op.job_id.is_some())
        .map(|op| op.operation_id)
    {
        let _ = cancel(w, name, id);
    }
}
pub(super) fn hold(w: &mut World, name: &str, reason: &str) {
    if let Some(state) = w.cargo_unloading.get_mut(name) {
        for op in &mut state.operations {
            if op.phase == CargoUnloadPhase::Unloading {
                op.phase = CargoUnloadPhase::Held;
                op.message = reason.into();
            }
        }
    }
}
pub(super) fn resume(w: &mut World, name: &str, id: u64, now: u64) -> Result<()> {
    let op = w
        .cargo_unloading
        .get(name)
        .and_then(|s| s.operations.iter().find(|op| op.operation_id == id))
        .context("unload operation not found for this pilot")?;
    if op.phase == CargoUnloadPhase::Unloading {
        return Ok(());
    }
    ensure!(
        op.phase == CargoUnloadPhase::Held,
        "only a held unload operation can resume"
    );
    check_binding(w, op)?;
    let job_active = op.job_id.is_some_and(|id| {
        w.resource_jobs
            .get(name)
            .is_some_and(|j| j.view.id == id && j.view.phase == ResourceJobPhase::Depositing)
    });
    let op = w
        .cargo_unloading
        .get_mut(name)
        .unwrap()
        .operations
        .iter_mut()
        .find(|op| op.operation_id == id)
        .unwrap();
    if !job_active {
        op.job_id = None;
    }
    op.phase = CargoUnloadPhase::Unloading;
    op.next_batch_at_ms = now
        .checked_add(CARGO_UNLOAD_BATCH_MS)
        .context("unload clock overflow")?;
    op.message =
        "Manual unloading resumed; only an active linked job receives delivery progress".into();
    Ok(())
}
/// Central interlock covers all alternative source mutations and departure. The
/// destination lock also covers other corporation members, not just the source.
pub(super) fn authorize(w: &World, name: &str, request: &Request) -> Result<()> {
    if active(w, name).is_some() {
        ensure!(
            !request.is_remote_gameplay()
                && !matches!(
                    request,
                    Request::DepositOre { .. }
                        | Request::WithdrawOre { .. }
                        | Request::DepositCorporationOre { .. }
                        | Request::WithdrawCorporationOre { .. }
                        | Request::SelectShip { .. }
                        | Request::BoardShip { .. }
                        | Request::PlaceSell { .. }
                        | Request::Buy { .. }
                        | Request::CancelSell { .. }
                        | Request::StartResourceJob { .. }
                        | Request::BeginRemoteControl { .. }
                ),
            "cargo hold reserved; finish or cancel unloading before this command"
        );
    }
    if matches!(
        request,
        Request::DepositCorporationOre { .. } | Request::WithdrawCorporationOre { .. }
    ) {
        if let Ok((owner, _)) =
            assets::unload_destination(w, name, CargoUnloadDestination::Corporation)
        {
            ensure!(
                !w.cargo_unloading
                    .values()
                    .flat_map(|s| &s.operations)
                    .any(|op| same_destination(
                        op,
                        CargoUnloadDestination::Corporation,
                        &owner,
                        w.pilots[name].view.system_id
                    )),
                "corporation storage reserved by unloading"
            );
        }
    }
    Ok(())
}
/// Called after ordinary cycle events were appended; receipt hashes reference the
/// actual ledger entry, never a predicted sequence. At most one batch per call,
/// scheduled from now, so delayed ticks never fast-forward a visible transfer.
pub(super) fn advance(
    w: &mut World,
    tx: &rusqlite::Transaction<'_>,
    entries: &mut Vec<LedgerEntry>,
    now: u64,
) -> Result<()> {
    let due: Vec<_> = w
        .cargo_unloading
        .iter()
        .filter_map(|(name, s)| {
            s.active()
                .filter(|op| op.phase == CargoUnloadPhase::Unloading && now >= op.next_batch_at_ms)
                .map(|op| (name.clone(), op.clone()))
        })
        .collect();
    for (name, old) in due {
        if let Err(error) = check_binding(w, &old) {
            hold(w, &name, &format!("Unloading held: {error}"));
            continue;
        }
        let units = old.reserved_units.min(CARGO_UNLOAD_BATCH_UNITS);
        let request = match old.destination {
            CargoUnloadDestination::Personal => Request::DepositOre { units },
            CargoUnloadDestination::Corporation => Request::DepositCorporationOre { units },
        };
        let event =
            assets::execute(w, &name, request, now)?.context("unload batch event missing")?;
        append_event(tx, entries, event, now, drone_cargo::ruleset(w))?;
        let entry = entries.last().unwrap();
        let final_batch = units == old.reserved_units;
        if let Some(job_id) = old.job_id {
            if w.resource_jobs.get(&name).is_some_and(|j| {
                j.view.id == job_id && j.view.phase == ResourceJobPhase::Depositing
            }) {
                resource_jobs::record_delivery_batch(w, &name, units, now, final_batch)?;
            }
        }
        let op = w
            .cargo_unloading
            .get_mut(&name)
            .unwrap()
            .operations
            .iter_mut()
            .find(|op| op.operation_id == old.operation_id)
            .unwrap();
        op.committed_units += units;
        op.reserved_units -= units;
        op.receipts.push(CargoUnloadBatchReceipt {
            sequence: op.receipts.len() as u64 + 1,
            units,
            committed_at_ms: now,
            ledger_sequence: entry.sequence,
            ledger_hash: entry.hash.clone(),
        });
        op.next_batch_at_ms = now
            .checked_add(CARGO_UNLOAD_BATCH_MS)
            .context("unload clock overflow")?;
        if final_batch {
            op.phase = CargoUnloadPhase::Completed;
        }
        op.message = format!(
            "{}: {} / {} ore units confirmed",
            op.phase.label(),
            op.committed_units,
            op.requested_units
        );
    }
    Ok(())
}
pub(super) fn validate(w: &World) -> Result<()> {
    let mut destinations = std::collections::BTreeSet::new();
    for (name, state) in &w.cargo_unloading {
        ensure!(
            docking::enabled(w)
                && w.pilots.contains_key(name)
                && state.schema_version == CARGO_UNLOADING_SCHEMA_VERSION,
            "invalid unload capability or owner"
        );
        ensure!(
            state.high_watermark == state.operations.len() as u64,
            "unload high-water mark mismatch"
        );
        ensure!(
            state
                .operations
                .iter()
                .filter(|op| op.phase.active())
                .count()
                <= 1,
            "multiple active cargo reservations"
        );
        for (i, op) in state.operations.iter().enumerate() {
            ensure!(
                op.operation_id == i as u64 + 1
                    && op.pilot == *name
                    && op.requested_units > 0
                    && op.committed_units <= op.requested_units,
                "invalid unload identity or quantity"
            );
            ensure!(
                op.reserved_units
                    == if op.phase.active() {
                        op.requested_units - op.committed_units
                    } else {
                        0
                    },
                "unload reservation mismatch"
            );
            ensure!(
                op.phase != CargoUnloadPhase::Completed || op.committed_units == op.requested_units,
                "incomplete unload marked complete"
            );
            let mut total = 0u64;
            let mut last_ledger = 0;
            for (i, r) in op.receipts.iter().enumerate() {
                ensure!(
                    r.sequence == i as u64 + 1
                        && r.units > 0
                        && r.units <= CARGO_UNLOAD_BATCH_UNITS
                        && r.ledger_sequence > last_ledger
                        && r.ledger_hash.len() == 64,
                    "invalid unload batch receipt"
                );
                total = total
                    .checked_add(r.units)
                    .context("receipt total overflow")?;
                last_ledger = r.ledger_sequence;
            }
            ensure!(
                total == op.committed_units,
                "unload receipts do not match committed inventory"
            );
            if op.phase.active() {
                ensure!(
                    w.pilots[name].view.ore >= op.reserved_units,
                    "reserved source ore unavailable"
                );
                ensure!(
                    destinations.insert((
                        format!("{:?}", op.destination),
                        op.destination_owner.clone(),
                        op.system_id
                    )),
                    "duplicate destination reservation"
                );
            }
        }
    }
    Ok(())
}
