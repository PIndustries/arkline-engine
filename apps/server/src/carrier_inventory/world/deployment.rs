//! One finite, retained Deploy All command. Physical children remain independently
//! identified; the queue never invents a second wave or imports later inventory.
use super::*;
fn members_done(s: &OwnedCarrier, b: &CarrierDeploymentBatch) -> bool {
    b.members.iter().all(|m| {
        if s.departures.contains_key(&m.drone_id) {
            return false;
        }
        let ids = [m.stage_operation_id, m.launch_operation_id];
        if ids.into_iter().flatten().any(|id| {
            s.runs.contains_key(&id) || s.inventory.cleanups.iter().any(|c| c.operation_id == id)
        }) {
            return false;
        }
        m.cancelled
            || m.launch_operation_id
                .and_then(|id| s.inventory.operation(id))
                .is_some_and(|o| {
                    matches!(
                        o.phase,
                        CarrierOperationPhase::Completed | CarrierOperationPhase::Destroyed
                    )
                })
            || m.stage_operation_id
                .and_then(|id| s.inventory.operation(id))
                .is_some_and(|o| o.phase == CarrierOperationPhase::Destroyed)
    })
}
pub(super) fn execute(
    w: &mut World,
    name: &str,
    request: &CarrierInventoryRequest,
    now: u64,
) -> Result<bool> {
    let (binding, id, kind) = match request {
        CarrierInventoryRequest::DeployAll { binding, batch_id } => (binding, *batch_id, 0),
        CarrierInventoryRequest::HoldDeployment { binding, batch_id } => (binding, *batch_id, 1),
        CarrierInventoryRequest::ResumeDeployment { binding, batch_id } => (binding, *batch_id, 2),
        CarrierInventoryRequest::CancelDeploymentRemaining { binding, batch_id } => {
            (binding, *batch_id, 3)
        }
        _ => return Ok(false),
    };
    let mut s = state(w, name).context("Owned carrier missing")?.clone();
    ensure!(
        *binding == s.inventory.binding,
        "Deployment vessel/source mismatch"
    );
    let index = s
        .inventory
        .deployments
        .iter()
        .position(|b| b.batch_id == id);
    if kind == 0 {
        if index.is_some() {
            return Ok(true);
        }
        ensure!(
            s.inventory.next_deployment_id() == Some(id)
                && s.inventory.active_deployment().is_none()
                && s.inventory.active_recall().is_none(),
            "Expected next idle deployment ID"
        );
        ensure!(
            !w.resource_jobs.contains_key(name)
                && s.inventory.operations.iter().all(|o| o.phase.terminal())
                && s.runs.is_empty(),
            "Finish existing routine/physical work before Deploy All"
        );
        let members = s
            .inventory
            .entries
            .iter()
            .filter(|e| {
                matches!(
                    e.location,
                    CarrierDroneLocation::Ready { .. } | CarrierDroneLocation::Protected { .. }
                )
            })
            .map(|e| CarrierDeploymentMember {
                drone_id: e.drone_id,
                stage_operation_id: None,
                launch_operation_id: None,
                cancelled: false,
                blocked_reason: String::new(),
            })
            .collect::<Vec<_>>();
        ensure!(
            !members.is_empty(),
            "No finite stored inventory remains to deploy"
        );
        s.inventory.deployments.push(CarrierDeploymentBatch {
            batch_id: id,
            members,
            phase: CarrierDeploymentPhase::Running,
            held: false,
            accepted_at_ms: now,
            updated_at_ms: now,
        });
        s.inventory.deployment_high_watermark = id;
    } else {
        let index = index.context("Unknown retained deployment")?;
        let b = s.inventory.deployments[index].clone();
        if !b.phase.active() {
            return Ok(true);
        }
        ensure!(now >= b.updated_at_ms, "Deployment clock reversed");
        let children = b
            .members
            .iter()
            .flat_map(|m| [m.stage_operation_id, m.launch_operation_id])
            .flatten()
            .collect::<Vec<_>>();
        if kind == 1 {
            for op in children {
                hold_stage(&mut s.inventory, op, now)?;
                if let Some(r) = s.runs.get_mut(&op) {
                    r.hold("Deploy All held");
                }
            }
            for m in &b.members {
                if let Some(d) = s.departures.get_mut(&m.drone_id) {
                    d.held = true;
                    d.hold_reason = "Deploy All held".into();
                }
            }
            s.inventory.deployments[index].held = true;
        } else if kind == 2 {
            for op in children {
                if let Some(r) = s.runs.get_mut(&op) {
                    if r.held {
                        r.resume_validated(now)?;
                    }
                }
                resume_stage(&mut s.inventory, op, now)?;
            }
            for m in &b.members {
                if let Some(d) = s.departures.get_mut(&m.drone_id) {
                    d.resume(now)?;
                }
            }
            s.inventory.deployments[index].held = false;
        } else {
            s.inventory.deployments[index].phase = CarrierDeploymentPhase::Cancelling;
            for op in children {
                if s.inventory
                    .operation(op)
                    .is_some_and(|o| o.effective_phase() == CarrierOperationPhase::Queued)
                {
                    phase(
                        &mut s,
                        op,
                        CarrierOperationPhase::Cancelled,
                        now,
                        "Deployment cancelled before reservation",
                    );
                }
            }
        }
        s.inventory.deployments[index].updated_at_ms = now;
    }
    for d in &mut w.pilots.get_mut(name).unwrap().view.drones {
        if s.departures.get(&d.id).is_some_and(|r| r.held) {
            d.velocity = [0.; 3];
        }
    }
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    drive(w, name, now)?;
    simulation::admit(w, name, now)?;
    Ok(true)
}
pub(super) fn drive(w: &mut World, name: &str, now: u64) -> Result<()> {
    let Some(def) = definition(w)? else {
        return Ok(());
    };
    let current = state(w, name).context("Owned carrier missing")?;
    let Some(index) = current
        .inventory
        .deployments
        .iter()
        .position(|b| b.phase.active())
    else {
        return Ok(());
    };
    let mut s = current.clone();
    let mut batch = s.inventory.deployments[index].clone();
    for m in &mut batch.members {
        if m.cancelled {
            continue;
        }
        let e = s
            .inventory
            .entry(m.drone_id)
            .context("Deployment identity missing")?
            .clone();
        if matches!(
            e.location,
            CarrierDroneLocation::Destroyed { .. } | CarrierDroneLocation::Deployed
        ) {
            continue;
        }
        if let Some(op) = s.inventory.active_operation(m.drone_id) {
            m.blocked_reason = if op.effective_phase() == CarrierOperationPhase::Queued
                || op.phase == CarrierOperationPhase::Held
            {
                op.message.clone()
            } else {
                String::new()
            };
            continue;
        }
        if batch.phase == CarrierDeploymentPhase::Cancelling {
            m.cancelled = true;
            m.blocked_reason = "Cancelled remaining deployment".into();
            continue;
        }
        if batch.held {
            continue;
        }
        let action = match e.location {
            CarrierDroneLocation::Ready { .. } => CarrierInventoryAction::Launch,
            CarrierDroneLocation::Protected { slot_id } => {
                // Deterministic compatible empty berth, never a home/class quota.
                let berth = def
                    .topology
                    .ready_berths
                    .iter()
                    .filter(|b| {
                        def.topology
                            .stage_loader(slot_id, b.berth_id, e.class)
                            .is_some()
                    })
                    .filter(|b| {
                        !s.inventory.reserved(CarrierResource::ReadyBerth {
                            berth_id: b.berth_id,
                        })
                    })
                    .find(|b| {
                        s.inventory
                            .docked_cassette(CarrierCassetteDock::Ready {
                                berth_id: b.berth_id,
                            })
                            .is_some_and(|c| c.drone_id.is_none())
                            && !s.inventory.operations.iter().any(|o| {
                                !o.phase.terminal()
                                    && o.action
                                        == (CarrierInventoryAction::Stage {
                                            berth_id: b.berth_id,
                                        })
                            })
                    });
                let Some(berth) = berth else {
                    m.blocked_reason =
                        "Waiting for a compatible empty same-side ready berth".into();
                    continue;
                };
                CarrierInventoryAction::Stage {
                    berth_id: berth.berth_id,
                }
            }
            _ => {
                m.blocked_reason = "Waiting for retained physical handling".into();
                continue;
            }
        };
        let id = s
            .inventory
            .next_operation_id()
            .context("Operation identity exhausted")?;
        if matches!(action, CarrierInventoryAction::Stage { .. }) {
            let binding = s.inventory.binding.clone();
            begin_stage(
                &mut s.inventory,
                &def.topology,
                &CarrierInventoryRequest::Begin {
                    binding,
                    operation_id: id,
                    drone_id: m.drone_id,
                    action,
                },
                now,
            )?;
            m.stage_operation_id = Some(id);
        } else {
            s.inventory.operations.push(CarrierInventoryOperation {
                recall_batch_id: None,
                operation_id: id,
                drone_id: m.drone_id,
                action,
                phase: CarrierOperationPhase::Queued,
                held_phase: None,
                stage: None,
                berth_id: None,
                accepted_at_ms: now,
                updated_at_ms: now,
                message: "Deploy All: queued launch".into(),
                cargo_service_operation_ids: vec![],
            });
            s.inventory.high_watermark = id;
            m.launch_operation_id = Some(id);
        }
        m.blocked_reason.clear();
    }
    if members_done(&s, &batch) {
        batch.phase = if batch.phase == CarrierDeploymentPhase::Cancelling {
            CarrierDeploymentPhase::Cancelled
        } else {
            CarrierDeploymentPhase::Completed
        };
        batch.held = false;
    }
    batch.updated_at_ms = now;
    s.inventory.deployments[index] = batch;
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    Ok(())
}
