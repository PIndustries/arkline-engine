//! Fresh finite-roster economics. Legacy rulesets and fixed five-hold state are
//! intentionally not reused or rewritten. Actual transfer lives in service.rs.
use super::*;
use crate::{flight, power};

pub(super) fn source_supports_service(def: &CarrierInventoryDefinition) -> bool {
    def.topology.registration.assembly_revision == 1703
        && def.cassettes.iter().any(|c| match c.initial_dock {
            CarrierCassetteDock::Ready { berth_id } => def
                .bay_route(
                    berth_id,
                    DroneClass::Mining,
                    c.cassette_id,
                    CarrierInventoryAction::ReturnToUnload {
                        resume_mining: false,
                    },
                )
                .is_some(),
            _ => false,
        })
}
pub(super) fn invalidate(h: &mut DroneOreHold) -> Result<()> {
    h.intent_generation = h
        .intent_generation
        .checked_add(1)
        .context("Mining intent generation exhausted")?;
    h.mining_intent = None;
    h.cycle = None;
    Ok(())
}
pub(super) fn stop(s: &mut OwnedCarrier) -> Result<()> {
    for h in &mut s.ore.holds {
        if h.mining_intent.is_some() || h.cycle.is_some() {
            invalidate(h)?;
        }
    }
    Ok(())
}
pub(crate) fn after_request(
    w: &mut World,
    name: &str,
    request: &Request,
    now: u64,
    automated: bool,
) -> Result<()> {
    let Some(mut s) = state(w, name).cloned() else {
        return Ok(());
    };
    let ids: Vec<_> = match request {
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
    ensure!(
        !automated || ids.is_empty(),
        "Carrier resource routines are not admitted"
    );
    for id in ids {
        s.mining_blocks.remove(&id);
        let target = w.pilots[name]
            .view
            .drones
            .iter()
            .find(|d| d.id == id)
            .and_then(|d| match d.mode {
                DroneMode::Mining { target_id } => Some(target_id),
                _ => None,
            });
        if let Some(h) = s.ore.holds.iter_mut().find(|h| h.drone_id == id) {
            invalidate(h)?;
            if let Some(target_id) = target {
                h.mining_intent = Some(DroneMiningIntent {
                    generation: h.intent_generation,
                    target_id,
                    job_id: None,
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
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    Ok(())
}
pub(crate) fn prune_claims(w: &mut World) {
    if w.carrier_inventory_registration.is_none() {
        return;
    }
    for (name, account) in &mut w.accounts {
        let Some(s) = account.carrier_inventory.as_mut() else {
            continue;
        };
        let Some(p) = w.pilots.get(name) else {
            continue;
        };
        for h in &mut s.ore.holds {
            if h.cycle.as_ref().is_some_and(|c| !p.view.drones.iter().any(|d| d.id == h.drone_id && d.hit_points > 0
                && matches!(d.mode, DroneMode::Mining { target_id } if target_id == c.target_id)
                && d.mining_cycle.as_ref().is_some_and(|m| m.started_at_ms == c.started_at_ms && m.completes_at_ms == c.completes_at_ms))) { h.cycle = None; }
        }
    }
}
/// Called AFTER combat and finite-identity destruction reconciliation. A dead
/// drone can neither complete a due cycle nor transfer a due ore batch.
pub(crate) fn after_combat(w: &mut World, now: u64) -> Result<Vec<EconomicEvent>> {
    let Some(def) = definition(w)? else {
        return Ok(vec![]);
    };
    if !source_supports_service(def) {
        return Ok(vec![]);
    }
    let mut events = Vec::new();
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let mut s = state(w, &name)
            .context("Carrier economics missing")?
            .clone();
        let mut automatic = Vec::new();
        for i in 0..s.ore.holds.len() {
            let id = s.ore.holds[i].drone_id;
            let entry = s.inventory.entry(id).context("Foreign Mining hold")?;
            if entry.hit_points == 0
                || entry.location != CarrierDroneLocation::Deployed
                || s.departures.contains_key(&id)
            {
                s.ore.holds[i].cycle = None;
                continue;
            }
            let Some(d) = w.pilots[&name]
                .view
                .drones
                .iter()
                .find(|d| d.id == id && d.hit_points > 0)
                .cloned()
            else {
                continue;
            };
            if s.inventory.active_operation(id).is_some() {
                continue;
            }
            let target = match d.mode {
                DroneMode::Mining { target_id } => Some(target_id),
                _ => None,
            };
            let target_available = target.is_some_and(|id| {
                w.asteroids.iter().any(|a| {
                    a.id == id
                        && a.ore > 0
                        && a.system_id == w.pilots[&name].view.system_id
                        && w.pilots[&name].view.locked_asteroids.contains(&id)
                })
            });
            let valid = target
                .and_then(|id| w.asteroids.iter().find(|a| a.id == id))
                .filter(|a| {
                    a.ore > 0
                        && a.system_id == w.pilots[&name].view.system_id
                        && w.pilots[&name].view.locked_asteroids.contains(&a.id)
                        && flight::distance(d.position, a.position) <= a.radius + DRONE_MINING_RANGE
                        && d.signal_strength > 0.
                })
                .map(|a| a.id);
            let cycle_matches = s.ore.holds[i].cycle.as_ref().is_some_and(|c| {
                d.mining_cycle.as_ref().is_some_and(|m| {
                    m.started_at_ms == c.started_at_ms && m.completes_at_ms == c.completes_at_ms
                })
            });
            if !cycle_matches || valid.is_none() {
                s.ore.holds[i].cycle = None;
            }
            if let Some(target_id) = valid {
                if s.ore.holds[i]
                    .mining_intent
                    .as_ref()
                    .is_none_or(|intent| intent.target_id != target_id)
                {
                    invalidate(&mut s.ore.holds[i])?;
                    let h = &mut s.ore.holds[i];
                    h.mining_intent = Some(DroneMiningIntent {
                        generation: h.intent_generation,
                        target_id,
                        job_id: None,
                    });
                }
                if let Some(claim) = s.ore.holds[i]
                    .cycle
                    .clone()
                    .filter(|c| now >= c.completes_at_ms)
                {
                    s.ore.holds[i].cycle = None;
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
                        s.ore.holds[i].ore_units = s.ore.holds[i]
                            .ore_units
                            .checked_add(units)
                            .context("Drone payload overflow")?;
                        if units > 0 {
                            events.push(EconomicEvent::CarrierInventory {
                                record: CarrierInventoryLedgerEvent::Extracted {
                                    pilot: name.clone(),
                                    vessel_id: s.inventory.binding.vessel_id.clone(),
                                    drone_id: id,
                                    cycle_id: claim.cycle_id,
                                    target_id,
                                    cycle_started_at_ms: claim.started_at_ms,
                                    units,
                                    job_id: None,
                                },
                            });
                        }
                    } else {
                        invalidate(&mut s.ore.holds[i])?;
                    }
                }
                if s.ore.holds[i].cycle.is_none() && s.ore.holds[i].mining_intent.is_some() {
                    let incoming = s
                        .ore
                        .reserved_incoming()
                        .context("Incoming cargo overflow")?;
                    let p = &w.pilots[&name].view;
                    let available = cargo_capacity(p.ship_class)
                        .checked_sub(p.ore)
                        .and_then(|v| v.checked_sub(incoming))
                        .context("Incoming cargo overcommitted")?;
                    let units = DRONE_MINING_YIELD
                        .min(MINING_DRONE_CARGO_CAPACITY - s.ore.holds[i].ore_units)
                        .min(available)
                        .min(w.asteroids.iter().find(|a| a.id == target_id).unwrap().ore);
                    if units > 0 {
                        let h = &mut s.ore.holds[i];
                        h.cycle_high_watermark = h
                            .cycle_high_watermark
                            .checked_add(1)
                            .context("Mining cycle ID exhausted")?;
                        h.cycle = Some(DroneCargoCycleClaim {
                            cycle_id: h.cycle_high_watermark,
                            target_id,
                            reserved_units: units,
                            started_at_ms: now,
                            completes_at_ms: now
                                .checked_add(DRONE_MINING_CYCLE_MS)
                                .context("Mining clock overflow")?,
                            job_id: None,
                            intent_generation: h.intent_generation,
                        });
                    }
                }
            }
            let no_headroom = w.pilots[&name].view.ore.checked_add(
                s.ore
                    .reserved_incoming()
                    .context("Incoming cargo overflow")?,
            ) == Some(cargo_capacity(w.pilots[&name].view.ship_class));
            let h = &s.ore.holds[i];
            if h.cycle.is_none()
                && h.ore_units > 0
                && h.mining_intent.is_some()
                && (h.ore_units == MINING_DRONE_CARGO_CAPACITY || !target_available || no_headroom)
            {
                automatic.push(id);
            }
            let d = w
                .pilots
                .get_mut(&name)
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == id)
                .unwrap();
            d.mining_cycle = h.cycle.as_ref().map(|c| DroneMiningCycle {
                started_at_ms: c.started_at_ms,
                completes_at_ms: c.completes_at_ms,
            });
            if h.mining_intent.is_none() && matches!(d.mode, DroneMode::Mining { .. }) {
                d.mode = DroneMode::Hold;
                d.velocity = [0.; 3];
            }
        }
        w.accounts.get_mut(&name).unwrap().carrier_inventory = Some(s);
        for id in automatic {
            super::service::begin_automatic(w, &name, id, now)?;
        }
        drones::sync(w.pilots.get_mut(&name).unwrap());
    }
    Ok(events)
}

pub(super) fn validate(w: &World, name: &str, s: &OwnedCarrier) -> Result<()> {
    ensure!(
        s.ore
            .available_ship_headroom(
                w.pilots[name].view.ore,
                cargo_capacity(w.pilots[name].view.ship_class)
            )
            .is_some(),
        "Carrier payload/claims exceed aggregate ship headroom"
    );
    for (id, reason) in &s.mining_blocks {
        ensure!(
            !reason.is_empty()
                && reason.len() <= 512
                && s.inventory
                    .entry(*id)
                    .is_some_and(|e| e.class == DroneClass::Mining
                        && e.location == CarrierDroneLocation::Deployed)
                && w.pilots[name]
                    .view
                    .drones
                    .iter()
                    .any(|d| d.id == *id && d.mode == DroneMode::Hold),
            "Invalid retained mining blockage"
        );
    }
    for h in &s.ore.holds {
        if let Some(c) = &h.cycle {
            ensure!(s.inventory.entry(h.drone_id).is_some_and(|e| e.location == CarrierDroneLocation::Deployed)
                && !s.departures.contains_key(&h.drone_id) && s.inventory.active_operation(h.drone_id).is_none()
                && w.pilots[name].view.drones.iter().any(|d| d.id == h.drone_id && d.hit_points > 0
                    && matches!(d.mode,DroneMode::Mining { target_id } if target_id == c.target_id)
                    && d.mining_cycle.as_ref().is_some_and(|m| m.started_at_ms == c.started_at_ms && m.completes_at_ms == c.completes_at_ms)), "Carrier claim differs from actual free mining cycle");
        }
    }
    if let Some(batch) = s.inventory.active_recall() {
        for member in &batch.members {
            if member.capture_operation_id.is_some()
                || member.service_operation_id.is_some_and(|id| {
                    s.inventory
                        .operation(id)
                        .is_some_and(|op| op.phase == CarrierOperationPhase::Completed)
                })
            {
                ensure!(
                    s.ore
                        .holds
                        .iter()
                        .find(|h| h.drone_id == member.drone_id)
                        .is_none_or(|h| h.ore_units == 0 && h.cycle.is_none()),
                    "Recall capture/completed service retains payload or extraction claim"
                );
            }
        }
    }
    service::validate(w, name, s)
}

/// Preserve the existing orbit integrator but certify every actual 50 ms
/// substep with a conservative rotation-independent corner cube. Each spatial
/// segment envelope is tested across the WHOLE authority interval, so changing
/// peer clocks cannot make a crossing disappear. Rejection keeps the prior pose.
pub(crate) fn guard_free_motion(
    before: &World,
    w: &mut World,
    traces: &BTreeMap<(String, u64), Vec<[f64; 3]>>,
    now: u64,
) -> Result<()> {
    use crate::independent_bays::{
        independent_bay_sweeps_clear, independent_world_to_body, IndependentBaySweptBody,
    };
    let Some(def) = definition(before)? else {
        return Ok(());
    };
    for ((name, id), path) in traces {
        let old = before.pilots[name]
            .view
            .drones
            .iter()
            .find(|d| d.id == *id)
            .context("Free trace identity missing")?;
        let s = state(before, name).context("Free trace owner missing")?;
        let host = parent(before, name)?;
        let start = old.updated_at_ms;
        let mut clear = now >= start
            && now - start <= 2000
            && path.first() == Some(&old.position)
            && !path.is_empty()
            && path.last()
                == w.pilots[name]
                    .view
                    .drones
                    .iter()
                    .find(|d| d.id == *id)
                    .map(|d| &d.position);
        let pose = |p| {
            independent_world_to_body(
                host,
                BayPose {
                    position_m: p,
                    orientation_xyzw: [0., 0., 0., 1.],
                },
            )
            .map(|p| p.position_m)
            .context("Free trace transform")
        };
        let radius = 2.5f64.hypot(0.9).hypot(3.);
        let fixed = |p, bounds| IndependentBaySweptBody {
            segment: BayMotionSegment {
                source: BayPose {
                    position_m: p,
                    orientation_xyzw: [0., 0., 0., 1.],
                },
                target: BayPose {
                    position_m: p,
                    orientation_xyzw: [0., 0., 0., 1.],
                },
                started_at_ms: start,
                duration_ms: 1,
            },
            local_bounds_m: bounds,
        };
        for pair in path.windows(2) {
            if !clear {
                break;
            }
            let a = pose(pair[0])?;
            let b = pose(pair[1])?;
            let bounds = [
                std::array::from_fn(|i| a[i].min(b[i]) - radius),
                std::array::from_fn(|i| a[i].max(b[i]) + radius),
            ];
            let ours = fixed([0.; 3], bounds);
            let margin = super::simulation::rounding_margin(host.position_m, pair[0]);
            clear = independent_bay_sweeps_clear(
                ours,
                fixed([0.; 3], def.deployed_hardware_bounds_m),
                start,
                now,
                margin,
            ) && simulation::clear_bodies(
                before,
                name,
                s,
                *id,
                None,
                host,
                &[ours],
                start,
                now,
            )?;
            // Hardware/other drone endpoints may already have advanced before
            // free flight. Bound their whole possible preceding interval too.
            for p in before
                .pilots
                .values()
                .filter(|p| p.view.system_id == before.pilots[name].view.system_id)
            {
                for d in &p.view.drones {
                    if d.id == *id {
                        continue;
                    }
                    let reach = radius
                        + drone_speed(d.class).max(40.)
                            * (now.saturating_sub(start) as f64 / 1000.);
                    clear &= independent_bay_sweeps_clear(
                        ours,
                        fixed(pose(d.position)?, [[-reach; 3], [reach; 3]]),
                        start,
                        now,
                        super::simulation::rounding_margin(host.position_m, d.position),
                    );
                }
            }
        }
        if !clear {
            let d = w
                .pilots
                .get_mut(name)
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == *id)
                .unwrap();
            *d = old.clone();
            d.mode = DroneMode::Hold;
            d.velocity = [0.; 3];
            d.mining_cycle = None;
            d.updated_at_ms = now;
            let s = w
                .accounts
                .get_mut(name)
                .unwrap()
                .carrier_inventory
                .as_mut()
                .unwrap();
            s.mining_blocks.insert(*id,"Mining flight blocked by actual hull/traffic/obstacle or simulation gap; explicit reassign required".into());
            if let Some(h) = s.ore.holds.iter_mut().find(|h| h.drone_id == *id) {
                invalidate(h)?;
            }
        }
    }
    Ok(())
}

/// Legacy free-flight signal loss must never enter its proximity-based Returning
/// branch in a finite carrier. Hold the last collision-accepted pose and payload;
/// a subsequent source-bound recovery is a separate retained operation.
pub(crate) fn settle_free_modes(w: &mut World, now: u64) -> Result<()> {
    if w.carrier_inventory_registration.is_none() {
        return Ok(());
    }
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let mut s = state(w, &name)
            .context("Free recovery owner missing")?
            .clone();
        for d in &mut w.pilots.get_mut(&name).unwrap().view.drones {
            if d.mode == DroneMode::Returning
                && s.inventory
                    .entry(d.id)
                    .is_some_and(|e| e.location == CarrierDroneLocation::Deployed)
                && !s.departures.contains_key(&d.id)
                && s.inventory.active_operation(d.id).is_none()
            {
                d.mode = DroneMode::Hold;
                d.velocity = [0.; 3];
                d.mining_cycle = None;
                d.updated_at_ms = now;
                if let Some(h) = s.ore.holds.iter_mut().find(|h| h.drone_id == d.id) {
                    if h.mining_intent.is_some() || h.cycle.is_some() {
                        invalidate(h)?;
                    }
                }
                if d.class == DroneClass::Mining {
                    s.mining_blocks.insert(d.id, "Control signal lost; holding actual pose and payload for source-bound recovery".into());
                }
            }
        }
        drones::sync(w.pilots.get_mut(&name).unwrap());
        w.accounts.get_mut(&name).unwrap().carrier_inventory = Some(s);
    }
    Ok(())
}
