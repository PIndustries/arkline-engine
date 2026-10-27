//! Fresh-world-only durable queue. Members use the existing identified physical
//! operations; this module never moves a pose, captures stock, or changes rates.
use super::*;

pub(super) fn active<'a>(w: &'a World, name: &str) -> Option<&'a DroneBayBatchOperation> {
    state(w, name)?.batches.as_ref()?.active()
}
fn journal_mut<'a>(w: &'a mut World, name: &str) -> Result<&'a mut DroneBayBatchJournal> {
    let id = state(w, name)
        .context("physical bay unavailable")?
        .binding
        .vessel_id
        .clone();
    w.accounts
        .get_mut(name)
        .and_then(|a| a.drone_bays.get_mut(&id))
        .and_then(|s| s.batches.as_mut())
        .context("batch operations require a fresh batch-capable world")
}
fn batch_mut<'a>(w: &'a mut World, name: &str, id: u64) -> Result<&'a mut DroneBayBatchOperation> {
    journal_mut(w, name)?
        .operations
        .iter_mut()
        .find(|b| b.batch_id == id)
        .context("bay batch missing")
}
pub(super) fn handles(r: &DroneBayRequest) -> bool {
    matches!(
        r,
        DroneBayRequest::BeginBatch { .. }
            | DroneBayRequest::HoldBatch { .. }
            | DroneBayRequest::ResumeBatch { .. }
            | DroneBayRequest::CancelBatch { .. }
    )
}
fn ready(w: &World, name: &str) -> Result<()> {
    runtime::parent(w, name)?;
    ensure!(
        w.resource_jobs
            .get(name)
            .is_none_or(|j| j.view.phase.is_terminal()),
        "Cancel the active resource job before starting or resuming a manual bay batch"
    );
    Ok(())
}
pub(super) fn execute(
    w: &mut World,
    name: &str,
    request: &DroneBayRequest,
    now: u64,
) -> Result<()> {
    ensure!(w.drone_bay_batch_version == Some(DRONE_BAY_BATCH_VERSION),
        "Batch operations are unavailable in this existing world; use individual physical operations");
    let s = state(w, name).context("owned physical bay missing")?;
    ensure!(
        w.drone_bay_registration == runtime::registration()
            && runtime::registration().map(|r| r.bind(s.binding.vessel_id.clone()))
                == Some(s.binding.clone()),
        "batch geometry/source registration is unsupported"
    );
    ensure!(
        s.schema_version == DRONE_BAY_SCHEMA_VERSION && &s.binding == request.binding(),
        "batch names a different owned vessel or unsupported source"
    );
    let journal = s
        .batches
        .as_ref()
        .context("batch initialization missing; never recreate it")?;
    match request {
        DroneBayRequest::BeginBatch {
            binding,
            batch_id,
            batch_kind,
        } => {
            if let Some(old) = journal.operation(*batch_id) {
                ensure!(
                    &old.binding == binding && &old.kind == batch_kind,
                    "batch ID reused with different intent"
                );
                return Ok(());
            }
            ensure!(
                *batch_id
                    == journal
                        .high_watermark
                        .checked_add(1)
                        .context("batch ID exhausted")?,
                "new batch ID must be confirmed high watermark plus one"
            );
            ensure!(
                journal.active().is_none(),
                "Another bay batch still owns the queue"
            );
            ensure!(
                !busy(w, name),
                "Finish, resume or cancel the current physical movement before starting a batch"
            );
            ready(w, name)?;
            let members: Vec<_> = s
                .stock
                .iter()
                .filter(|entry| match batch_kind {
                    DroneBayBatchKind::DeployClass { class } => {
                        entry.class == *class
                            && matches!(entry.status, DroneStockStatus::Stored { .. })
                    }
                    DroneBayBatchKind::RecallAll => entry.status == DroneStockStatus::Active,
                })
                .map(|entry| DroneBayBatchMember {
                    drone_id: entry.drone_id,
                    home_cradle: entry.home_cradle,
                    service_operation_ids: vec![],
                    operation_id: None,
                    status: DroneBayBatchMemberStatus::Pending,
                })
                .collect();
            ensure!(
                !members.is_empty(),
                "No eligible owned drones for this batch"
            );
            if matches!(batch_kind, DroneBayBatchKind::RecallAll) {
                drone_cargo::clear_intents(w, name);
                for d in &mut w.pilots.get_mut(name).unwrap().view.drones {
                    if members.iter().any(|m| m.drone_id == d.id) {
                        d.mode = DroneMode::Hold;
                        d.velocity = [0.; 3];
                        d.mining_cycle = None;
                        d.attack_ready_at_ms = None;
                        d.updated_at_ms = now;
                    }
                }
                drones::sync(w.pilots.get_mut(name).unwrap());
            }
            let journal = journal_mut(w, name)?;
            journal.high_watermark = *batch_id;
            journal.operations.push(DroneBayBatchOperation {
                batch_id: *batch_id,
                binding: binding.clone(),
                kind: batch_kind.clone(),
                phase: DroneBayBatchPhase::Running,
                held: false,
                members,
                current_service_operation_id: None,
                current_operation_id: None,
                accepted_at_ms: now,
                updated_at_ms: now,
                message: "Queued owned drones for the single physical bay".into(),
            });
            advance_one(w, name, now)?;
        }
        DroneBayRequest::HoldBatch { batch_id, .. } => {
            let b = journal
                .operation(*batch_id)
                .context("bay batch missing")?
                .clone();
            if !b.active() {
                return Ok(());
            }
            hold(w, name, now, "Bay batch paused; resume explicitly");
            if let Some(operation_id) = b.current_service_operation_id {
                let binding = drone_cargo::state(w, name)
                    .context("cargo batch service missing")?
                    .binding
                    .clone();
                drone_cargo::receiver::execute_queued(
                    w,
                    name,
                    &DroneCargoRequest::Hold {
                        binding,
                        operation_id,
                    },
                    now,
                )?;
            }
            if let Some(operation_id) = b.current_operation_id {
                runtime::execute_queued(
                    w,
                    name,
                    &DroneBayRequest::Hold {
                        binding: b.binding,
                        operation_id,
                    },
                    now,
                )?;
            }
        }
        DroneBayRequest::ResumeBatch { batch_id, .. } => {
            let b = journal
                .operation(*batch_id)
                .context("bay batch missing")?
                .clone();
            if !b.active() || !b.held {
                return Ok(());
            }
            ready(w, name)?;
            if let Some(operation_id) = b.current_service_operation_id {
                let binding = drone_cargo::state(w, name)
                    .context("cargo batch service missing")?
                    .binding
                    .clone();
                drone_cargo::receiver::execute_queued(
                    w,
                    name,
                    &DroneCargoRequest::Resume {
                        binding,
                        operation_id,
                    },
                    now,
                )?;
            }
            if let Some(operation_id) = b.current_operation_id {
                runtime::execute_queued(
                    w,
                    name,
                    &DroneBayRequest::Resume {
                        binding: b.binding,
                        operation_id,
                    },
                    now,
                )?;
            }
            let b = batch_mut(w, name, *batch_id)?;
            b.held = false;
            b.updated_at_ms = now;
            b.message = if b.phase == DroneBayBatchPhase::Cancelling {
                "Finishing current movement; remaining members cancelled"
            } else {
                "Resumed physical bay batch"
            }
            .into();
            advance_one(w, name, now)?;
        }
        DroneBayRequest::CancelBatch { batch_id, .. } => {
            let b = journal
                .operation(*batch_id)
                .context("bay batch missing")?
                .clone();
            if !b.active() {
                return Ok(());
            }
            let b = batch_mut(w, name, *batch_id)?;
            b.phase = DroneBayBatchPhase::Cancelling;
            b.updated_at_ms = now;
            for member in &mut b.members {
                if member.status == DroneBayBatchMemberStatus::Pending {
                    member.status = DroneBayBatchMemberStatus::Cancelled;
                }
            }
            b.message =
                "Remaining members cancelled; current accepted movement must finish safely".into();
            reconcile_one(w, name, now)?;
        }
        _ => bail!("not a batch request"),
    }
    Ok(())
}
pub(super) fn hold(w: &mut World, name: &str, now: u64, reason: &str) {
    let Some(id) = active(w, name).map(|b| b.batch_id) else {
        return;
    };
    if let Ok(b) = batch_mut(w, name, id) {
        b.held = true;
        b.updated_at_ms = now;
        b.message = reason.into();
    }
}
pub(super) fn individual_control(w: &mut World, name: &str, request: &DroneBayRequest, now: u64) {
    let operation_id = match request {
        DroneBayRequest::Hold { operation_id, .. }
        | DroneBayRequest::Cancel { operation_id, .. } => *operation_id,
        _ => return,
    };
    if active(w, name).is_some_and(|b| b.current_operation_id == Some(operation_id)) {
        hold(
            w,
            name,
            now,
            if matches!(request, DroneBayRequest::Cancel { .. }) {
                "Current operation is cancelling safely; remaining batch is paused"
            } else {
                "Current operation held; remaining batch is paused"
            },
        );
    }
}
pub(super) fn reconcile(w: &mut World, now: u64) -> Result<()> {
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        reconcile_one(w, &name, now)?;
    }
    Ok(())
}
fn reconcile_one(w: &mut World, name: &str, now: u64) -> Result<()> {
    let Some(b) = active(w, name).cloned() else {
        return Ok(());
    };
    let s = state(w, name).unwrap();
    let stock = s.stock.clone();
    let op = b
        .current_operation_id
        .map(|id| {
            s.operation(id)
                .cloned()
                .context("batch member operation missing")
        })
        .transpose()?;
    let service = b
        .current_service_operation_id
        .map(|id| {
            drone_cargo::state(w, name)
                .and_then(|s| s.operation(id))
                .cloned()
                .context("batch service receipt missing")
        })
        .transpose()?;
    let receiver_busy = drone_cargo::busy(w, name);
    let service_held = drone_cargo::state(w, name)
        .and_then(|s| s.cleanup.as_ref())
        .is_some_and(|c| c.held);
    let hardware_busy = busy(w, name);
    let b = batch_mut(w, name, b.batch_id)?;
    let before = b.clone();
    for member in &mut b.members {
        if matches!(
            member.status,
            DroneBayBatchMemberStatus::Pending
                | DroneBayBatchMemberStatus::Active
                | DroneBayBatchMemberStatus::Servicing
        ) && stock.iter().any(|e| {
            e.drone_id == member.drone_id && matches!(e.status, DroneStockStatus::Destroyed { .. })
        }) {
            member.status = DroneBayBatchMemberStatus::Destroyed;
        }
    }
    if let Some(op) = op {
        let member = b
            .members
            .iter_mut()
            .find(|m| m.operation_id == Some(op.operation_id))
            .context("batch current member missing")?;
        if op.phase == DroneBayPhase::Held {
            b.held = true;
            b.message = op.message.clone();
        }
        if op.phase.terminal() {
            member.status = match op.phase {
                DroneBayPhase::Free => DroneBayBatchMemberStatus::Launched,
                DroneBayPhase::Stored => DroneBayBatchMemberStatus::Stored,
                DroneBayPhase::Destroyed => DroneBayBatchMemberStatus::Destroyed,
                DroneBayPhase::Cancelled => DroneBayBatchMemberStatus::Cancelled,
                _ => unreachable!(),
            };
            if !hardware_busy {
                b.current_operation_id = None;
            }
        }
    }
    if let Some(service) = service {
        let member = b
            .members
            .iter_mut()
            .find(|m| m.service_operation_ids.last() == Some(&service.operation_id))
            .context("batch current service member missing")?;
        if service.phase == DroneDropoffPhase::Held || service_held {
            b.held = true;
            b.message = service.message.clone();
        }
        if service.phase.terminal() && !receiver_busy {
            member.status = match service.phase {
                DroneDropoffPhase::Completed if b.phase == DroneBayBatchPhase::Running => {
                    DroneBayBatchMemberStatus::Pending
                }
                DroneDropoffPhase::Destroyed => DroneBayBatchMemberStatus::Destroyed,
                _ => DroneBayBatchMemberStatus::Cancelled,
            };
            b.current_service_operation_id = None;
        }
    }
    if b.current_operation_id.is_none()
        && b.current_service_operation_id.is_none()
        && !receiver_busy
        && !hardware_busy
        && !b
            .members
            .iter()
            .any(|m| m.status == DroneBayBatchMemberStatus::Pending)
    {
        b.phase = if b.phase == DroneBayBatchPhase::Cancelling {
            DroneBayBatchPhase::Cancelled
        } else {
            DroneBayBatchPhase::Completed
        };
        b.held = false;
        b.message = if b.phase == DroneBayBatchPhase::Cancelled {
            "Batch cancelled; accepted movement and machinery are safe"
        } else {
            "Batch finished; inspect each retained member result"
        }
        .into();
    }
    // Changes are persisted atomically with the same world/individual operation.
    if *b != before {
        b.updated_at_ms = b.updated_at_ms.max(now);
    }
    Ok(())
}

pub(super) fn advance(w: &mut World, now: u64, defer: Option<&str>) -> Result<()> {
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        if defer != Some(name.as_str()) {
            advance_one(w, &name, now)?;
        }
    }
    Ok(())
}
fn advance_one(w: &mut World, name: &str, now: u64) -> Result<()> {
    reconcile_one(w, name, now)?;
    let Some(b) = active(w, name).cloned() else {
        return Ok(());
    };
    if b.held
        || b.phase != DroneBayBatchPhase::Running
        || b.current_operation_id.is_some()
        || b.current_service_operation_id.is_some()
        || drone_cargo::busy(w, name)
        || busy(w, name)
    {
        return Ok(());
    }
    let Some(member) = b
        .members
        .iter()
        .find(|m| m.status == DroneBayBatchMemberStatus::Pending)
    else {
        return Ok(());
    };
    if let Err(error) = ready(w, name) {
        hold(w, name, now, &error.to_string());
        return Ok(());
    }
    if b.kind == DroneBayBatchKind::RecallAll && drone_cargo::payload(w, name, member.drone_id) > 0
    {
        let mut candidate = w.clone();
        let operation_id = drone_cargo::receiver::begin_automatic(
            &mut candidate,
            name,
            member.drone_id,
            Some(b.batch_id),
            now,
        )?;
        let service_held = drone_cargo::state(&candidate, name)
            .and_then(|s| s.operation(operation_id))
            .is_some_and(|o| o.phase == DroneDropoffPhase::Held);
        let current = batch_mut(&mut candidate, name, b.batch_id)?;
        current.held = service_held;
        let entry = current
            .members
            .iter_mut()
            .find(|m| m.drone_id == member.drone_id)
            .unwrap();
        entry.service_operation_ids.push(operation_id);
        entry.status = DroneBayBatchMemberStatus::Servicing;
        current.current_service_operation_id = Some(operation_id);
        current.updated_at_ms = now;
        current.message =
            "Unloading this member at the ore receiver before full cradle storage".into();
        *w = candidate;
        return Ok(());
    }
    let operation_id = state(w, name)
        .unwrap()
        .high_watermark
        .checked_add(1)
        .context("individual bay operation ID exhausted")?;
    let request = match b.kind {
        DroneBayBatchKind::DeployClass { .. } => DroneBayRequest::Launch {
            binding: b.binding.clone(),
            drone_id: member.drone_id,
            operation_id,
        },
        DroneBayBatchKind::RecallAll => DroneBayRequest::Recover {
            binding: b.binding.clone(),
            drone_id: member.drone_id,
            operation_id,
        },
    };
    // Failed admission cannot consume an operation ID or alter stock/poses.
    let mut candidate = w.clone();
    {
        let current = batch_mut(&mut candidate, name, b.batch_id)?;
        let entry = current
            .members
            .iter_mut()
            .find(|m| m.drone_id == member.drone_id)
            .unwrap();
        entry.operation_id = Some(operation_id);
        entry.status = DroneBayBatchMemberStatus::Active;
        current.current_operation_id = Some(operation_id);
        current.updated_at_ms = now;
        current.message = "One drone is using the shared route; remaining members wait".into();
    }
    match runtime::execute_queued(&mut candidate, name, &request, now) {
        Ok(()) => *w = candidate,
        Err(error) => hold(
            w,
            name,
            now,
            &format!("Batch held before starting next member: {error}"),
        ),
    }
    Ok(())
}
pub(super) fn validate_inventory(s: &DroneBayState) -> Result<()> {
    match (s.schema_version, &s.batches) {
        (DRONE_BAY_LEGACY_SCHEMA_VERSION, None) => return Ok(()),
        (DRONE_BAY_SCHEMA_VERSION, Some(j)) => {
            ensure!(
                j.version == DRONE_BAY_BATCH_VERSION
                    && j.high_watermark == j.operations.len() as u64,
                "invalid batch journal version/high watermark"
            );
            let mut operation_ids = std::collections::BTreeSet::new();
            let mut live = 0;
            for (index, b) in j.operations.iter().enumerate() {
                ensure!(
                    b.batch_id == index as u64 + 1 && b.validate_shape(&s.binding),
                    "invalid retained batch identity/state"
                );
                live += usize::from(b.active());
                if b.active() {
                    ensure!(
                        b.current_operation_id
                            == s.handling_operation
                                .or_else(|| s.cleanup.as_ref().map(|c| c.operation_id)),
                        "live batch does not own actual handling/cleanup"
                    );
                    if let Some(id) = b.current_operation_id {
                        let m = b
                            .members
                            .iter()
                            .find(|m| m.operation_id == Some(id))
                            .context("current batch member missing")?;
                        ensure!(
                            m.status == DroneBayBatchMemberStatus::Active
                                || m.status == DroneBayBatchMemberStatus::Destroyed
                                    && s.cleanup.as_ref().is_some_and(|c| c.operation_id == id),
                            "terminal member cannot retain hardware except destruction cleanup"
                        );
                    }
                }

                for m in &b.members {
                    let legal_outcome = match m.status {
                        DroneBayBatchMemberStatus::Pending => {
                            b.phase != DroneBayBatchPhase::Cancelling
                        }
                        DroneBayBatchMemberStatus::Cancelled if m.operation_id.is_none() => {
                            matches!(
                                b.phase,
                                DroneBayBatchPhase::Cancelling | DroneBayBatchPhase::Cancelled
                            )
                        }
                        DroneBayBatchMemberStatus::Launched => {
                            matches!(b.kind, DroneBayBatchKind::DeployClass { .. })
                        }
                        DroneBayBatchMemberStatus::Stored => b.kind == DroneBayBatchKind::RecallAll,
                        _ => true,
                    };
                    ensure!(
                        legal_outcome,
                        "batch outcome is incompatible with its accepted intent/cancellation"
                    );
                    let entry = s
                        .stock
                        .iter()
                        .find(|e| e.drone_id == m.drone_id && e.home_cradle == m.home_cradle)
                        .context("batch member outside finite roster")?;
                    if let DroneBayBatchKind::DeployClass { class } = b.kind {
                        ensure!(entry.class == class, "batch member class changed");
                    }
                    if b.active() && m.status == DroneBayBatchMemberStatus::Pending {
                        ensure!(
                            match b.kind {
                                DroneBayBatchKind::DeployClass { .. } =>
                                    matches!(entry.status, DroneStockStatus::Stored { .. }),
                                DroneBayBatchKind::RecallAll =>
                                    entry.status == DroneStockStatus::Active,
                            },
                            "pending member no longer has its accepted stock presence"
                        );
                    }
                    if m.status == DroneBayBatchMemberStatus::Destroyed {
                        ensure!(
                            matches!(entry.status, DroneStockStatus::Destroyed { .. }),
                            "destroyed batch member was revived"
                        );
                    }
                    if let Some(id) = m.operation_id {
                        let op = s.operation(id).context("batch operation receipt missing")?;
                        ensure!(
                            operation_ids.insert(id)
                                && op.drone_id == m.drone_id
                                && op.binding == b.binding
                                && op.kind
                                    == match b.kind {
                                        DroneBayBatchKind::DeployClass { .. } =>
                                            DroneBayOperationKind::Launch,
                                        DroneBayBatchKind::RecallAll =>
                                            DroneBayOperationKind::Recover,
                                    },
                            "batch individual operation binding changed"
                        );
                        ensure!(
                            !b.active()
                                || b.current_operation_id != Some(id)
                                || ((op.phase != DroneBayPhase::Held
                                    && !s
                                        .cleanup
                                        .as_ref()
                                        .is_some_and(|c| c.operation_id == id && c.held))
                                    || b.held),
                            "held current machinery requires an explicitly held batch"
                        );
                        ensure!(
                            match m.status {
                                DroneBayBatchMemberStatus::Pending
                                | DroneBayBatchMemberStatus::Servicing => false,
                                DroneBayBatchMemberStatus::Active =>
                                    !op.phase.terminal() && b.current_operation_id == Some(id),
                                DroneBayBatchMemberStatus::Launched =>
                                    op.phase == DroneBayPhase::Free,
                                DroneBayBatchMemberStatus::Stored =>
                                    op.phase == DroneBayPhase::Stored,
                                DroneBayBatchMemberStatus::Destroyed =>
                                    op.phase == DroneBayPhase::Destroyed,
                                DroneBayBatchMemberStatus::Cancelled =>
                                    op.phase == DroneBayPhase::Cancelled,
                            },
                            "batch outcome contradicts retained individual operation"
                        );
                    }
                }
            }
            ensure!(
                live <= 1
                    && j.operations
                        .iter()
                        .take(j.operations.len().saturating_sub(1))
                        .all(|b| !b.active()),
                "multiple or nonlatest live batches"
            );
        }
        _ => bail!("batch-capable state lost its journal, or legacy state was silently upgraded"),
    }
    Ok(())
}
