//! Fresh-preview cargo authority. The economic owner is a persistent finite
//! Mining identity, never a rendered body or an animated client counter.
use super::*;
pub(super) mod receiver;
pub(super) use receiver::{
    advance, authorize, busy, commit_transfers, constrained, execute, finish, hold, Runtime,
};

pub(super) fn registration() -> Result<DroneCargoRegistration> {
    let expected = supported_drone_cargo_registration();
    ensure!(
        drone_bay::creation_registration(SceneLayoutRevision::DockingV1)
            == Some(expected.base.clone())
            && expected.base.source_sha256
                == arkline_protocol::drone_dropoff_geometry::DROPOFF_BASE_SOURCE_SHA256,
        "ore receiver base source registration is unsupported"
    );
    Ok(expected)
}
fn expected_registration(w: &World) -> Result<DroneCargoRegistration> {
    if let Some(def) = independent_bays::world::definition(w)? {
        Ok(independent_bays::world::cargo_registration(def))
    } else {
        registration()
    }
}
pub(super) fn state<'a>(w: &'a World, name: &str) -> Option<&'a DroneCargoState> {
    w.drone_cargo_registration.as_ref()?;
    let p = w.pilots.get(name)?;
    let a = w.accounts.get(name)?;
    a.drone_cargo.get(a.vessel_id(p.view.ship_class)?)
}
fn state_mut<'a>(w: &'a mut World, name: &str) -> Result<&'a mut DroneCargoState> {
    let id = state(w, name)
        .context("owned cargo initialization missing")?
        .binding
        .vessel_id
        .clone();
    w.accounts
        .get_mut(name)
        .and_then(|a| a.drone_cargo.get_mut(&id))
        .context("cargo vessel missing")
}
pub(super) fn reserved_for_account(account: &assets::Account, class: ShipClass) -> u64 {
    if class == ShipClass::Pioneer {
        if let Some(s) = &account.carrier_inventory {
            return s.ore.reserved_incoming().unwrap_or(u64::MAX);
        }
    }
    account
        .vessel_id(class)
        .and_then(|id| account.drone_cargo.get(id))
        .and_then(DroneCargoState::reserved_incoming)
        .unwrap_or(0)
}
pub(super) fn reserved(w: &World, name: &str) -> u64 {
    if let Some(s) = carrier_inventory::world::state(w, name) {
        return s.ore.reserved_incoming().unwrap_or(u64::MAX);
    }
    state(w, name)
        .and_then(DroneCargoState::reserved_incoming)
        .unwrap_or(0)
}
pub(super) fn hold_capacity(w: &World, name: &str) -> u64 {
    cargo_capacity(w.pilots[name].view.ship_class).saturating_sub(reserved(w, name))
}
pub(super) fn payload(w: &World, name: &str, id: u64) -> u64 {
    if let Some(s) = carrier_inventory::world::state(w, name) {
        return s
            .ore
            .holds
            .iter()
            .find(|h| h.drone_id == id)
            .map_or(0, |h| h.ore_units);
    }
    state(w, name)
        .and_then(|s| s.holds.iter().find(|h| h.drone_id == id))
        .map_or(0, |h| h.ore_units)
}
pub(super) fn ruleset(w: &World) -> u32 {
    if w.carrier_inventory_registration.is_some() {
        CARRIER_INVENTORY_RULESET_VERSION
    } else if w.drone_cargo_registration.is_some() {
        DRONE_CARGO_RULESET_VERSION
    } else {
        RULESET_VERSION
    }
}
pub(super) fn initialize(w: &mut World, name: &str, now: u64) -> Result<Option<EconomicEvent>> {
    let Some(reg) = w.drone_cargo_registration.clone() else {
        return Ok(None);
    };
    ensure!(
        reg == expected_registration(w)?,
        "unsupported cargo creation registration"
    );
    let bay = drone_bay::state(w, name).context("finite stock must precede cargo")?;
    let binding = reg.bind(bay.binding.vessel_id.clone());
    let ids: Vec<_> = bay
        .stock
        .iter()
        .filter(|d| d.class == DroneClass::Mining)
        .map(|d| d.drone_id)
        .collect();
    ensure!(ids.len() == DRONE_CLASS_CAP, "finite Mining roster missing");
    let a = w.accounts.get_mut(name).unwrap();
    ensure!(
        !a.drone_cargo.contains_key(&binding.vessel_id),
        "cargo initialization already exists; never reseed"
    );
    a.drone_cargo.insert(
        binding.vessel_id.clone(),
        DroneCargoState {
            schema_version: DRONE_CARGO_SCHEMA_VERSION,
            binding: binding.clone(),
            initialized_at_ms: now,
            holds: ids
                .iter()
                .map(|id| DroneOreHold {
                    drone_id: *id,
                    ore_units: 0,
                    destroyed_at_ms: None,
                    intent_generation: 0,
                    mining_intent: None,
                    cycle_high_watermark: 0,
                    cycle: None,
                })
                .collect(),
            lost_ore_units: 0,
            high_watermark: 0,
            operations: vec![],
            receiver_operation: None,
            cleanup: None,
        },
    );
    Ok(Some(EconomicEvent::DroneCargo {
        record: DroneCargoLedgerEvent::Initialized {
            pilot: name.into(),
            vessel_id: binding.vessel_id,
            registration: binding.registration,
            drone_ids: ids,
            capacity_units: MINING_DRONE_CARGO_CAPACITY,
        },
    }))
}
fn event(
    name: &str,
    vessel: &str,
    record: impl FnOnce(String, String) -> DroneCargoLedgerEvent,
) -> EconomicEvent {
    EconomicEvent::DroneCargo {
        record: record(name.into(), vessel.into()),
    }
}
fn invalidate_intent(hold: &mut DroneOreHold) -> Result<()> {
    hold.intent_generation = hold
        .intent_generation
        .checked_add(1)
        .context("mining intent generation exhausted")?;
    hold.mining_intent = None;
    hold.cycle = None;
    Ok(())
}
pub(super) fn clear_intents(w: &mut World, name: &str) {
    if let Ok(s) = state_mut(w, name) {
        for h in &mut s.holds {
            h.mining_intent = None;
            h.cycle = None;
        }
    }
}
/// Release only claims that an already-authorized change invalidated. Persisted
/// corruption is rejected by validate before this reconciler is ever called.
pub(super) fn prune_claims(w: &mut World) {
    carrier_inventory::world::cargo::prune_claims(w);
    if w.drone_cargo_registration.is_none() {
        return;
    }
    for (name, a) in &mut w.accounts {
        let Some(p) = w.pilots.get(name) else {
            continue;
        };
        for s in a.drone_cargo.values_mut() {
            for h in &mut s.holds {
                if h.cycle.as_ref().is_some_and(|c|!p.view.drones.iter().any(|d|d.id==h.drone_id&&d.hit_points>0
                &&matches!(d.mode,DroneMode::Mining{target_id} if target_id==c.target_id)
                &&d.mining_cycle.as_ref().is_some_and(|m|m.started_at_ms==c.started_at_ms&&m.completes_at_ms==c.completes_at_ms)))
            {h.cycle=None;}
            }
        }
    }
}
pub(super) fn stop_all(w: &mut World, name: &str, now: u64, reason: &str) -> Result<()> {
    if state(w, name).is_none() {
        return Ok(());
    }
    hold(w, name, now, reason)?;
    let s = state_mut(w, name)?;
    for h in &mut s.holds {
        invalidate_intent(h)?;
    }
    if let Some(p) = w.pilots.get_mut(name) {
        for d in &mut p.view.drones {
            if d.class == DroneClass::Mining {
                d.mining_cycle = None;
                d.mode = DroneMode::Hold;
                d.velocity = [0.; 3];
                d.updated_at_ms = now;
            }
        }
        drones::sync(p);
    }
    Ok(())
}
/// A successful explicit replacement changes generation even when it names the
/// same asteroid. A delayed drop-off cannot resurrect the old intent.
pub(super) fn after_request(
    w: &mut World,
    name: &str,
    request: &Request,
    now: u64,
    automated: bool,
) -> Result<()> {
    if w.carrier_inventory_registration.is_some() {
        return carrier_inventory::world::cargo::after_request(w, name, request, now, automated);
    }
    if state(w, name).is_none() {
        return Ok(());
    }
    let ids: Vec<u64> = match request {
        Request::AssignMiningDrone { id, .. }
        | Request::DroneHoldById { id }
        | Request::DroneNavigateById { id, .. }
        | Request::AssignAttackDrone { id, .. }
        | Request::OrbitDrone { id, .. } => vec![*id],
        Request::DroneHold | Request::DroneNavigate { .. } => w.pilots[name]
            .view
            .drones
            .first()
            .map(|d| vec![d.id])
            .unwrap_or_default(),
        _ => vec![],
    };
    let job_id = automated
        .then(|| {
            w.resource_jobs
                .get(name)
                .filter(|j| !j.view.phase.is_terminal())
                .map(|j| j.view.id)
        })
        .flatten();
    for id in ids {
        let target = w.pilots[name]
            .view
            .drones
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| match d.mode {
                DroneMode::Mining { target_id } => Some(target_id),
                _ => None,
            });
        if let Some(h) = state_mut(w, name)?
            .holds
            .iter_mut()
            .find(|h| h.drone_id == id)
        {
            invalidate_intent(h)?;
            if let Some(target_id) = target {
                h.mining_intent = Some(DroneMiningIntent {
                    generation: h.intent_generation,
                    target_id,
                    job_id,
                });
            }
        }
        if let Some(d) = w
            .pilots
            .get_mut(name)
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == id)
        {
            d.mining_cycle = None;
            d.updated_at_ms = now;
        }
    }
    Ok(())
}
pub(super) fn cancel_job(w: &mut World, name: &str, id: u64) {
    if state(w, name).is_none() {
        return;
    }
    if let Ok(s) = state_mut(w, name) {
        for h in &mut s.holds {
            if h.mining_intent
                .as_ref()
                .is_some_and(|i| i.job_id == Some(id))
            {
                h.mining_intent = None;
                h.cycle = None;
            }
        }
    }
}

/// Called after every simultaneous incoming shot, before bay capture and dead
/// body removal. Old worlds never enter this path or change extraction order.
pub(super) fn after_combat(w: &mut World, now: u64) -> Result<Vec<EconomicEvent>> {
    if w.drone_cargo_registration.is_none() {
        return Ok(vec![]);
    }
    let mut events = Vec::new();
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    for name in names {
        let id = state(w, &name)
            .context("cargo initialization missing")?
            .binding
            .vessel_id
            .clone();
        let mut s = w
            .accounts
            .get_mut(&name)
            .unwrap()
            .drone_cargo
            .remove(&id)
            .unwrap();
        let mut auto = Vec::new();
        let mut dead = Vec::new();
        let mut job_extractions = Vec::new();
        for i in 0..s.holds.len() {
            let drone_id = s.holds[i].drone_id;
            let d = w.pilots[&name]
                .view
                .drones
                .iter()
                .find(|d| d.id == drone_id);
            let destroyed = d.is_some_and(|d| d.hit_points == 0)
                || drone_bay::state(w, &name).unwrap().stock.iter().any(|e| {
                    e.drone_id == drone_id && matches!(e.status, DroneStockStatus::Destroyed { .. })
                });
            if destroyed && s.holds[i].destroyed_at_ms.is_none() {
                let h = &mut s.holds[i];
                let lost = h.ore_units;
                h.ore_units = 0;
                h.destroyed_at_ms = Some(now);
                invalidate_intent(h)?;
                s.lost_ore_units = s
                    .lost_ore_units
                    .checked_add(lost)
                    .context("lost ore overflow")?;
                events.push(event(&name, &id, |pilot, vessel_id| {
                    DroneCargoLedgerEvent::Destroyed {
                        pilot,
                        vessel_id,
                        drone_id,
                        destroyed_at_ms: now,
                        lost_units: lost,
                    }
                }));
                dead.push(drone_id);
                continue;
            }
            let Some(d) = d.filter(|d| d.hit_points > 0) else {
                continue;
            };
            if s.operations
                .iter()
                .any(|o| o.drone_id == drone_id && o.active())
            {
                continue;
            }
            let target = match d.mode {
                DroneMode::Mining { target_id } => Some(target_id),
                _ => None,
            };
            let intent = s.holds[i].mining_intent.clone();
            let job_valid = intent
                .as_ref()
                .and_then(|intent| intent.job_id)
                .is_none_or(|job_id| {
                    w.resource_jobs
                        .get(&name)
                        .is_some_and(|j| j.view.id == job_id && !j.view.phase.is_terminal())
                });
            if !job_valid {
                invalidate_intent(&mut s.holds[i])?;
                continue;
            }
            let valid_target = target
                .and_then(|target| w.asteroids.iter().find(|a| a.id == target))
                .filter(|a| {
                    a.ore > 0
                        && a.system_id == w.pilots[&name].view.system_id
                        && w.pilots[&name].view.locked_asteroids.contains(&a.id)
                        && flight::distance(d.position, a.position) <= a.radius + DRONE_MINING_RANGE
                        && d.signal_strength > 0.
                })
                .map(|a| a.id);
            let cycle_matches = s.holds[i].cycle.as_ref().is_some_and(|c| {
                d.mining_cycle.as_ref().is_some_and(|dc| {
                    dc.started_at_ms == c.started_at_ms && dc.completes_at_ms == c.completes_at_ms
                })
            });
            if !cycle_matches || valid_target.is_none() {
                s.holds[i].cycle = None;
            }
            if let Some(target_id) = valid_target {
                if s.holds[i]
                    .mining_intent
                    .as_ref()
                    .is_none_or(|intent| intent.target_id != target_id)
                {
                    invalidate_intent(&mut s.holds[i])?;
                    let h = &mut s.holds[i];
                    h.mining_intent = Some(DroneMiningIntent {
                        generation: h.intent_generation,
                        target_id,
                        job_id: None,
                    });
                }
                if let Some(claim) = s.holds[i]
                    .cycle
                    .clone()
                    .filter(|c| now >= c.completes_at_ms)
                {
                    s.holds[i].cycle = None;
                    let p = w.pilots.get_mut(&name).unwrap();
                    if now.saturating_sub(claim.completes_at_ms) < DRONE_MINING_CYCLE_MS
                        && power::spend(
                            &mut p.view.power,
                            power_spec(p.view.ship_class).drone_mining_cycle_cost_j,
                            now,
                        )
                    {
                        let a = w.asteroids.iter_mut().find(|a| a.id == target_id).unwrap();
                        let units = claim.reserved_units.min(a.ore);
                        a.ore -= units;
                        w.belt_ore -= units;
                        s.holds[i].ore_units += units;
                        if units > 0 {
                            events.push(event(&name, &id, |pilot, vessel_id| {
                                DroneCargoLedgerEvent::Extracted {
                                    pilot,
                                    vessel_id,
                                    drone_id,
                                    cycle_id: claim.cycle_id,
                                    target_id,
                                    cycle_started_at_ms: claim.started_at_ms,
                                    units,
                                    job_id: claim.job_id,
                                }
                            }));
                            if let Some(job) = claim.job_id {
                                job_extractions.push((job, units));
                            }
                        }
                    } else {
                        invalidate_intent(&mut s.holds[i])?;
                    }
                }
                if s.holds[i].cycle.is_none() && s.holds[i].mining_intent.is_some() {
                    let incoming = s.reserved_incoming().context("cargo claim overflow")?;
                    let p = &w.pilots[&name].view;
                    let available = cargo_capacity(p.ship_class)
                        .checked_sub(p.ore)
                        .and_then(|v| v.checked_sub(incoming))
                        .context("cargo headroom overcommitted")?;
                    let units = DRONE_MINING_YIELD
                        .min(MINING_DRONE_CARGO_CAPACITY - s.holds[i].ore_units)
                        .min(available)
                        .min(w.asteroids.iter().find(|a| a.id == target_id).unwrap().ore);
                    if units > 0 {
                        let h = &mut s.holds[i];
                        h.cycle_high_watermark = h
                            .cycle_high_watermark
                            .checked_add(1)
                            .context("cycle ID exhausted")?;
                        let intent = h.mining_intent.as_ref().unwrap();
                        h.cycle = Some(DroneCargoCycleClaim {
                            cycle_id: h.cycle_high_watermark,
                            target_id,
                            reserved_units: units,
                            started_at_ms: now,
                            completes_at_ms: now
                                .checked_add(DRONE_MINING_CYCLE_MS)
                                .context("cycle clock overflow")?,
                            job_id: intent.job_id,
                            intent_generation: intent.generation,
                        });
                    }
                }
            }
            let incoming = s.reserved_incoming().context("cargo claim overflow")?;
            let no_headroom = w.pilots[&name].view.ore.checked_add(incoming)
                == Some(cargo_capacity(w.pilots[&name].view.ship_class));
            if s.holds[i].cycle.is_none()
                && s.holds[i].ore_units > 0
                && s.holds[i].mining_intent.is_some()
                && (s.holds[i].ore_units == MINING_DRONE_CARGO_CAPACITY
                    || valid_target.is_none()
                    || no_headroom)
            {
                auto.push(drone_id);
            }
            let p = w.pilots.get_mut(&name).unwrap();
            if let Some(d) = p.view.drones.iter_mut().find(|d| d.id == drone_id) {
                d.mining_cycle = s.holds[i].cycle.as_ref().map(|c| DroneMiningCycle {
                    started_at_ms: c.started_at_ms,
                    completes_at_ms: c.completes_at_ms,
                });
                if s.holds[i].mining_intent.is_none() && matches!(d.mode, DroneMode::Mining { .. })
                {
                    d.mode = DroneMode::Hold;
                    d.velocity = [0.; 3];
                }
            }
            drones::sync(p);
        }
        w.accounts.get_mut(&name).unwrap().drone_cargo.insert(id, s);
        for drone in dead {
            receiver::destroyed(w, &name, drone, now)?;
        }
        for drone in auto {
            if w.independent_bay_registration.is_some() {
                independent_bays::world::begin_automatic(w, &name, drone, now)?;
            } else {
                receiver::begin_automatic(w, &name, drone, None, now)?;
            }
        }
        for (job, units) in job_extractions {
            resource_jobs::record_drone_extraction(w, &name, job, units, now)?;
        }
    }
    Ok(events)
}

pub(super) fn totals(w: &World) -> Result<Option<DroneOreTotals>> {
    if w.drone_cargo_registration.is_none() {
        return Ok(None);
    }
    let (mut ore, mut lost) = (0u64, 0u64);
    for s in w.accounts.values().flat_map(|a| a.drone_cargo.values()) {
        ore = ore
            .checked_add(s.ore_units().context("cargo sum overflow")?)
            .context("cargo total overflow")?;
        lost = lost
            .checked_add(s.lost_ore_units)
            .context("cargo loss overflow")?;
    }
    Ok(Some(DroneOreTotals {
        schema_version: 1,
        drone_ore_units: ore,
        lost_ore_units: lost,
    }))
}
pub(super) fn validate(w: &World) -> Result<()> {
    let Some(reg) = &w.drone_cargo_registration else {
        ensure!(
            w.accounts
                .values()
                .all(|a| a.drone_cargo.is_empty() && a.drone_receivers.is_empty()),
            "legacy world cannot contain cargo feature state"
        );
        return Ok(());
    };
    ensure!(
        *reg == expected_registration(w)?
            && w.scene_layout == SceneLayoutRevision::DockingV1
            && w.drone_bay_batch_version == Some(DRONE_BAY_BATCH_VERSION),
        "unsupported cargo world binding"
    );
    for (name, p) in &w.pilots {
        let s = state(w, name).context("cargo initialization missing; never recreate")?;
        let bay = drone_bay::state(w, name).context("cargo stock missing")?;
        ensure!(
            p.view.ship_class == ShipClass::Pioneer
                && s.valid_shape()
                && s.binding == reg.bind(bay.binding.vessel_id.clone()),
            "invalid cargo state/binding"
        );
        ensure!(
            w.accounts[name].drone_cargo.len() == 1,
            "cargo preview cannot hide another vessel inventory"
        );
        ensure!(
            w.accounts[name]
                .drone_receivers
                .keys()
                .all(|id| id == &s.binding.vessel_id),
            "orphan receiver route outside actual vessel binding"
        );
        for h in &s.holds {
            let entry = bay
                .stock
                .iter()
                .find(|e| e.drone_id == h.drone_id && e.class == DroneClass::Mining)
                .context("cargo ID outside Mining roster")?;
            ensure!(
                matches!(entry.status, DroneStockStatus::Destroyed { .. })
                    == h.destroyed_at_ms.is_some(),
                "destroyed cargo receipt conflicts with stock"
            );
            if !matches!(entry.status, DroneStockStatus::Active) {
                ensure!(
                    h.ore_units == 0 && h.cycle.is_none(),
                    "stored/destroyed drone must not hide loaded cargo"
                );
            }
            if let Some(claim) = &h.cycle {
                let d = p
                    .view
                    .drones
                    .iter()
                    .find(|d| d.id == h.drone_id)
                    .context("claim drone missing")?;
                ensure!(
                    matches!(d.mode,DroneMode::Mining{target_id} if target_id==claim.target_id)
                        && d.mining_cycle
                            .as_ref()
                            .is_some_and(|c| c.started_at_ms == claim.started_at_ms
                                && c.completes_at_ms == claim.completes_at_ms),
                    "claim does not match a real mining cycle"
                );
            }
        }
        ensure!(
            p.view
                .ore
                .checked_add(s.reserved_incoming().context("reservation overflow")?)
                .is_some_and(|v| v <= cargo_capacity(p.view.ship_class)),
            "incoming ore headroom overcommitted"
        );
        if let Some(journal) = &bay.batches {
            let mut linked = std::collections::BTreeSet::new();
            for batch in &journal.operations {
                for member in &batch.members {
                    for (index, id) in member.service_operation_ids.iter().enumerate() {
                        let op = s
                            .operation(*id)
                            .context("retained recall service receipt missing")?;
                        ensure!(
                            linked.insert(*id)
                                && op.drone_id == member.drone_id
                                && op.followup
                                    == (DroneDropoffFollowup::RecallAll {
                                        batch_id: batch.batch_id
                                    }),
                            "recall service immutable binding mismatch"
                        );
                        let current = batch.current_service_operation_id == Some(*id);
                        ensure!(
                            current || op.phase.terminal(),
                            "historical service cannot remain active without batch ownership"
                        );
                        if current {
                            ensure!(
                                batch.active()
                                    && index + 1 == member.service_operation_ids.len()
                                    && matches!(
                                        member.status,
                                        DroneBayBatchMemberStatus::Servicing
                                            | DroneBayBatchMemberStatus::Destroyed
                                    ),
                                "current recall service outcome mismatch"
                            );
                            ensure!(
                                op.active()
                                    || s.cleanup.as_ref().is_some_and(|c| c.operation_id == *id),
                                "current service lost actionable route/cleanup"
                            );
                            ensure!(
                                op.phase != DroneDropoffPhase::Held || batch.held,
                                "held service must hold its batch"
                            );
                        }
                        if member.status == DroneBayBatchMemberStatus::Stored {
                            ensure!(
                                op.phase == DroneDropoffPhase::Completed,
                                "stored member lacks completed unload prefix"
                            );
                        }
                    }
                    ensure!(
                        member.status != DroneBayBatchMemberStatus::Servicing
                            || batch.current_service_operation_id
                                == member.service_operation_ids.last().copied(),
                        "servicing member lost its current service"
                    );
                }
            }
            for op in &s.operations {
                if let DroneDropoffFollowup::RecallAll { .. } = op.followup {
                    ensure!(
                        linked.contains(&op.operation_id),
                        "recall service has no retained typed member link"
                    );
                }
            }
        }
        if w.independent_bay_registration.is_none() {
            receiver::validate(w, name)?;
        }
    }
    Ok(())
}
pub(super) fn project(w: &mut World) -> Result<()> {
    resource_jobs::project_drone_cargo(w)?;
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let public = if w.independent_bay_registration.is_some() {
            None
        } else {
            receiver::projection(w, &name)?
        };
        w.pilots.get_mut(&name).unwrap().view.drone_cargo = public;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
