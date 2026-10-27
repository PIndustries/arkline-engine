//! One vessel-global manifold, backed by exact reached socket contact and the
//! same SQLite transaction as ship/hold mutation. Geometry never grants ore.
use super::*;

pub(crate) fn begin_automatic(w: &mut World, name: &str, id: u64, now: u64) -> Result<()> {
    let s = state(w, name).context("Carrier missing")?;
    if s.inventory.active_operation(id).is_some()
        || s.departures.contains_key(&id)
        || s.inventory.active_deployment().is_some()
        || s.inventory.active_recall().is_some()
    {
        return Ok(());
    }
    let request = CarrierInventoryRequest::Begin {
        binding: s.inventory.binding.clone(),
        operation_id: s
            .inventory
            .next_operation_id()
            .context("Physical operation IDs exhausted")?,
        drone_id: id,
        action: CarrierInventoryAction::ReturnToUnload {
            resume_mining: true,
        },
    };
    execute(w, name, &request, now)
}
pub(super) fn close_attempt(s: &mut OwnedCarrier, id: u64, now: u64) -> Result<()> {
    s.ore.waiting.retain(|x| *x != id);
    if let Some(service_id) = s.ore.granted_service_id {
        let service = s
            .ore
            .services
            .iter_mut()
            .find(|a| a.service_id == service_id)
            .context("Manifold grant lost its attempt")?;
        if service.physical_operation_id == id {
            ensure!(
                service.closed_at_ms.is_none() && now >= service.accepted_at_ms,
                "Invalid service close clock"
            );
            service.closed_at_ms = Some(now);
            s.ore.granted_service_id = None;
        }
    }
    Ok(())
}
fn coupled(s: &OwnedCarrier, id: u64) -> bool {
    let Some(r) = s.runs.get(&id) else {
        return false;
    };
    let Some(op) = s.inventory.operation(id) else {
        return false;
    };
    matches!(op.action, CarrierInventoryAction::ReturnToUnload { .. })
        && op.phase == CarrierOperationPhase::Servicing
        && r.waiting_for_transfer
        && !r.held
        && r.reached.ore_coupled
        && r.route
            .get(r.target_index)
            .is_some_and(|f| f.milestones.ore_coupled && CarrierMotionFrame::from(f) == r.confirmed)
        && s.inventory.entry(op.drone_id).is_some_and(|e| {
            e.hit_points > 0
                && matches!(e.location, CarrierDroneLocation::Servicing { operation_id, berth_id }
            if operation_id == id && Some(berth_id) == op.berth_id)
        })
}
pub(super) fn reconcile(s: &mut OwnedCarrier, now: u64) -> Result<()> {
    let eligible: BTreeSet<_> = s
        .runs
        .keys()
        .copied()
        .filter(|id| coupled(s, *id))
        .collect();
    if let Some(service_id) = s.ore.granted_service_id {
        let physical = s
            .ore
            .services
            .iter()
            .find(|a| a.service_id == service_id)
            .context("Missing grant attempt")?
            .physical_operation_id;
        if !eligible.contains(&physical) {
            close_attempt(s, physical, now)?;
        }
    }
    s.ore.waiting.retain(|id| eligible.contains(id));
    for id in eligible {
        let granted = s.ore.granted_service_id.is_some_and(|sid| {
            s.ore
                .services
                .iter()
                .any(|a| a.service_id == sid && a.physical_operation_id == id)
        });
        if !granted && !s.ore.waiting.contains(&id) {
            s.ore.waiting.push(id);
        }
    }
    Ok(())
}
fn bound_contact(w: &World, name: &str, s: &OwnedCarrier, id: u64, now: u64) -> Result<bool> {
    if !coupled(s, id) {
        return Ok(false);
    }
    let r = &s.runs[&id];
    let def = definition(w)?.context("Service source missing")?;
    let op = s.inventory.operation(id).unwrap();
    let berth = op.berth_id.context("Service berth missing")?;
    ensure!(
        def.supports_ore_service(berth, r.class),
        "Service source unsupported"
    );
    if r.confirmed_at_ms != now
        || r.parent != parent(w, name)?
        || actual(s, &r.confirmed, r.drone_id)? != r.confirmed
    {
        return Ok(false);
    }
    let body = r
        .confirmed
        .poses
        .iter()
        .find(|(key, _)| *key == CarrierPoseKey::Drone)
        .context("Service body absent")?
        .1;
    let world = r.parent.compose(body).context("Service body transform")?;
    let live = w.pilots[name].view.drones.iter().any(|d| {
        d.id == r.drone_id
            && d.hit_points > 0
            && d.updated_at_ms == now
            && d.position == world.position_m
            && d.orientation_xyzw == Some(world.orientation_xyzw)
            && d.velocity == [0.; 3]
    });
    Ok(live)
}
fn actual_contact(w: &World, name: &str, s: &OwnedCarrier, id: u64, now: u64) -> Result<bool> {
    Ok(bound_contact(w, name, s, id, now)? && simulation::clear(w, name, s, &s.runs[&id], now)?)
}
pub(crate) fn commit_transfers(
    w: &mut World,
    tx: &rusqlite::Transaction<'_>,
    entries: &mut Vec<LedgerEntry>,
    now: u64,
) -> Result<()> {
    let Some(def) = definition(w)? else {
        return Ok(());
    };
    if !cargo::source_supports_service(def) {
        return Ok(());
    }
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let mut s = state(w, &name).context("Carrier missing")?.clone();
        reconcile(&mut s, now)?;
        // Contact is rechecked after combat for EVERY candidate, even an old
        // grant. A stale pose/velocity/obstruction never consumes this tick.
        let active: Vec<_> = s
            .runs
            .keys()
            .copied()
            .filter(|id| coupled(&s, *id))
            .collect();
        for id in active {
            if !actual_contact(w, &name, &s, id, now)? {
                close_attempt(&mut s, id, now)?;
                s.runs
                    .get_mut(&id)
                    .unwrap()
                    .hold("Service contact or clearance lost");
                hold_stage(&mut s.inventory, id, now)?;
            }
        }
        if s.ore.granted_service_id.is_none() {
            if let Some(id) = s.ore.waiting.first().copied() {
                let op = s
                    .inventory
                    .operation(id)
                    .context("Queued service missing")?
                    .clone();
                let hold = s
                    .ore
                    .holds
                    .iter()
                    .find(|h| h.drone_id == op.drone_id)
                    .context("Mining payload absent")?;
                let units = hold.ore_units;
                if units == 0 {
                    s.ore.waiting.remove(0);
                    s.runs.get_mut(&id).unwrap().release_transfer_barrier(now)?;
                } else {
                    ensure!(
                        units <= MINING_DRONE_CARGO_CAPACITY && hold.cycle.is_none(),
                        "Unbounded coupled payload"
                    );
                    let service_id = s
                        .ore
                        .service_high_watermark
                        .checked_add(1)
                        .context("Service IDs exhausted")?;
                    s.ore.services.push(CarrierOreService {
                        service_id,
                        physical_operation_id: id,
                        drone_id: op.drone_id,
                        berth_id: op.berth_id.unwrap(),
                        accepted_at_ms: now,
                        closed_at_ms: None,
                        requested_units: units,
                        receipts: vec![],
                    });
                    s.ore.service_high_watermark = service_id;
                    s.ore.granted_service_id = Some(service_id);
                    s.ore.waiting.remove(0);
                    s.inventory
                        .operations
                        .iter_mut()
                        .find(|o| o.operation_id == id)
                        .unwrap()
                        .cargo_service_operation_ids
                        .push(service_id);
                }
            }
        }
        if let Some(service_id) = s.ore.granted_service_id {
            let service = s
                .ore
                .services
                .iter()
                .find(|a| a.service_id == service_id)
                .unwrap()
                .clone();
            let due = service
                .accepted_at_ms
                .checked_add(DRONE_DROPOFF_BATCH_MS)
                .is_some_and(|t| now >= t)
                && s.ore.last_committed_at_ms.is_none_or(|last| {
                    last.checked_add(DRONE_DROPOFF_BATCH_MS)
                        .is_some_and(|t| now >= t)
                });
            if due {
                ensure!(
                    actual_contact(w, &name, &s, service.physical_operation_id, now)?,
                    "Grant lost actual stable socket contact"
                );
                let remaining = service
                    .requested_units
                    .checked_sub(
                        service
                            .committed_units()
                            .context("Transfer receipt overflow")?,
                    )
                    .context("Transfer overcommitted")?;
                let hold = s
                    .ore
                    .holds
                    .iter_mut()
                    .find(|h| h.drone_id == service.drone_id)
                    .unwrap();
                ensure!(
                    remaining > 0
                        && hold.ore_units == remaining
                        && hold.cycle.is_none()
                        && hold.destroyed_at_ms.is_none(),
                    "Live service payload changed"
                );
                let units = remaining.min(DRONE_DROPOFF_BATCH_UNITS);
                let ship = &mut w.pilots.get_mut(&name).unwrap().view;
                let next = ship.ore.checked_add(units).context("Ship cargo overflow")?;
                ensure!(
                    next <= cargo_capacity(ship.ship_class),
                    "Ship headroom unavailable"
                );
                hold.ore_units -= units;
                ship.ore = next;
                let sequence = service.receipts.len() as u64 + 1;
                crate::append_event(
                    tx,
                    entries,
                    EconomicEvent::CarrierInventory {
                        record: CarrierInventoryLedgerEvent::Transferred {
                            pilot: name.clone(),
                            vessel_id: s.inventory.binding.vessel_id.clone(),
                            drone_id: service.drone_id,
                            physical_operation_id: service.physical_operation_id,
                            berth_id: service.berth_id,
                            service_id,
                            service_started_at_ms: service.accepted_at_ms,
                            requested_units: service.requested_units,
                            sequence,
                            units,
                        },
                    },
                    now,
                    CARRIER_INVENTORY_RULESET_VERSION,
                )?;
                let ledger = entries.last().unwrap();
                s.ore
                    .services
                    .iter_mut()
                    .find(|a| a.service_id == service_id)
                    .unwrap()
                    .receipts
                    .push(DroneOreTransferReceipt {
                        sequence,
                        units,
                        committed_at_ms: now,
                        ledger_sequence: ledger.sequence,
                        ledger_hash: ledger.hash.clone(),
                    });
                s.ore.last_committed_at_ms = Some(now);
                if units == remaining {
                    close_attempt(&mut s, service.physical_operation_id, now)?;
                    s.runs
                        .get_mut(&service.physical_operation_id)
                        .unwrap()
                        .release_transfer_barrier(now)?;
                }
            }
        }
        ensure!(
            s.ore.validate_shape(&s.inventory),
            "Transfer produced invalid finite payload state"
        );
        w.accounts.get_mut(&name).unwrap().carrier_inventory = Some(s);
    }
    Ok(())
}

pub(super) fn validate(w: &World, name: &str, s: &OwnedCarrier) -> Result<()> {
    let mut eligible: BTreeSet<_> = s
        .runs
        .keys()
        .copied()
        .filter(|id| coupled(s, *id))
        .collect();
    for id in &eligible {
        ensure!(
            bound_contact(w, name, s, *id, s.runs[id].confirmed_at_ms)?,
            "Retained coupled service lacks actual stable body/root contact"
        );
    }
    for a in &s.ore.services {
        let op = s
            .inventory
            .operation(a.physical_operation_id)
            .context("Service history owner missing")?;
        ensure!(
            a.accepted_at_ms <= op.updated_at_ms
                && a.closed_at_ms.is_none_or(|at| at <= op.updated_at_ms),
            "Service attempt clock exceeds physical history"
        );
    }
    if let Some(service_id) = s.ore.granted_service_id {
        let attempt = s
            .ore
            .services
            .iter()
            .find(|a| a.service_id == service_id)
            .context("Missing grant")?;
        ensure!(
            eligible.remove(&attempt.physical_operation_id),
            "Grant is not at actual reached source barrier"
        );
    }
    ensure!(
        s.ore.waiting.iter().copied().collect::<BTreeSet<_>>() == eligible,
        "Coupled FIFO and actual source barriers differ"
    );
    Ok(())
}
