//! One complete ordered drone simulation step, including combat and physical finish hooks.

use super::super::*;
use super::lifecycle::{idle, invalidate, returning, sync};
use super::motion::{approach, orbit, orbit_catalog, orbit_key, targets};

pub(super) fn advance(world: &mut World, now: u64) -> Result<Vec<EconomicEvent>> {
    let carrier_before = world
        .carrier_inventory_registration
        .as_ref()
        .map(|_| world.clone());
    let mut carrier_traces = BTreeMap::new();
    invalidate(world);
    carrier_inventory::world::cargo::settle_free_modes(world, now)?;
    let positions = targets(world);
    let orbit_targets = orbit_catalog(world);
    let cargo_mode =
        world.drone_cargo_registration.is_some() || world.carrier_inventory_registration.is_some();
    let constrained: std::collections::BTreeSet<_> = world
        .pilots
        .iter()
        .flat_map(|(name, p)| {
            p.view
                .drones
                .iter()
                .filter(|d| {
                    carrier_inventory::world::constrained(world, name, d.id)
                        || independent_bays::world::constrained(world, name, d.id)
                        || drone_bay::constrained(world, name, d.id)
                        || drone_cargo::constrained(world, name, d.id)
                })
                .map(|d| d.id)
        })
        .collect();
    let mut attacks = Vec::new();
    let mut events = Vec::new();
    for (name, p) in &mut world.pilots {
        let Some(ship) = p.view.flight.as_ref() else {
            continue;
        };
        let ship_position = ship.position;
        let mut fleet = std::mem::take(&mut p.view.drones);
        for mut d in fleet.drain(..) {
            if constrained.contains(&d.id) {
                d.mining_cycle = None;
                d.attack_ready_at_ms = None;
                p.view.drones.push(d);
                continue;
            }
            let mode = d.mode.clone();
            match mode {
                DroneMode::Orbit { target } => {
                    if let Some((system, position, radius)) = orbit_targets.get(&orbit_key(&target))
                    {
                        if *system == p.view.system_id {
                            orbit(&mut d, *position, *radius, now);
                        } else {
                            idle(&mut d);
                        }
                    } else {
                        idle(&mut d);
                    }
                }
                DroneMode::Hold => {}
                DroneMode::Returning
                    if p.view.drone_bay.is_some()
                        || world.independent_bay_registration.is_some()
                        || world.carrier_inventory_registration.is_some() =>
                {
                    // The physical bay controller must accept an identified
                    // continuous route. Never drop an owned body at 100 metres.
                    d.velocity = [0.; 3];
                    d.updated_at_ms = now;
                }
                DroneMode::Returning => {
                    if approach(&mut d, ship_position, 100.0, now) {
                        continue;
                    }
                }
                DroneMode::FlyTo { position } => {
                    if approach(&mut d, position, 0.0, now) {
                        idle(&mut d);
                    }
                }
                DroneMode::Mining { target_id } => {
                    let Some(a) = world
                        .asteroids
                        .iter_mut()
                        .find(|a| a.id == target_id && a.ore > 0)
                    else {
                        idle(&mut d);
                        p.view.drones.push(d);
                        continue;
                    };
                    if cargo_mode {
                        let reached = if carrier_before.is_some() {
                            let (reached, trace) = super::motion::orbit_traced(
                                &mut d,
                                a.position,
                                a.radius + DRONE_MINING_RANGE * 0.6,
                                now,
                            );
                            carrier_traces.insert((name.clone(), d.id), trace);
                            reached
                        } else {
                            orbit(&mut d, a.position, a.radius + DRONE_MINING_RANGE * 0.6, now)
                        };
                        if !reached {
                            d.mining_cycle = None;
                        }
                    } else if p.view.ore >= cargo_capacity(p.view.ship_class) {
                        idle(&mut d);
                    } else if orbit(&mut d, a.position, a.radius + DRONE_MINING_RANGE * 0.6, now) {
                        d.signal_strength = drone_signal_strength(d.position, ship_position);
                        if d.signal_strength <= 0.0 {
                            returning(&mut d);
                        } else if let Some(cycle) = d.mining_cycle.clone() {
                            if now >= cycle.completes_at_ms {
                                if now.saturating_sub(cycle.completes_at_ms)
                                    >= DRONE_MINING_CYCLE_MS
                                    || !power::spend(
                                        &mut p.view.power,
                                        power_spec(p.view.ship_class).drone_mining_cycle_cost_j,
                                        now,
                                    )
                                {
                                    idle(&mut d);
                                } else {
                                    let units = DRONE_MINING_YIELD
                                        .min(a.ore)
                                        .min(cargo_capacity(p.view.ship_class) - p.view.ore);
                                    a.ore -= units;
                                    world.belt_ore -= units;
                                    p.view.ore += units;
                                    events.push(EconomicEvent::OreMined {
                                        pilot: name.clone(),
                                        units,
                                    });
                                    if a.ore == 0 || p.view.ore == cargo_capacity(p.view.ship_class)
                                    {
                                        idle(&mut d);
                                    } else {
                                        d.mining_cycle = now
                                            .checked_add(DRONE_MINING_CYCLE_MS)
                                            .map(|completes_at_ms| DroneMiningCycle {
                                                started_at_ms: now,
                                                completes_at_ms,
                                            });
                                        if d.mining_cycle.is_none() {
                                            idle(&mut d);
                                        }
                                    }
                                }
                            }
                        } else {
                            d.mining_cycle =
                                now.checked_add(DRONE_MINING_CYCLE_MS)
                                    .map(|completes_at_ms| DroneMiningCycle {
                                        started_at_ms: now,
                                        completes_at_ms,
                                    });
                        }
                    } else {
                        d.mining_cycle = None;
                    }
                }
                DroneMode::Attacking {
                    target_pilot,
                    target_drone_id,
                } => {
                    if let Some((_, position)) =
                        positions.get(&(target_pilot.clone(), target_drone_id))
                    {
                        orbit(&mut d, *position, DRONE_ATTACK_RANGE * 0.6, now);
                        if flight::distance(d.position, *position) <= DRONE_ATTACK_RANGE
                            && drone_signal_strength(d.position, ship_position) > 0.0
                        {
                            if let Some(ready) = d.attack_ready_at_ms {
                                if now >= ready {
                                    attacks.push((
                                        name.clone(),
                                        d.id,
                                        target_pilot,
                                        target_drone_id,
                                        DRONE_ATTACK_DAMAGE,
                                    ));
                                    d.attack_ready_at_ms =
                                        now.checked_add(DRONE_ATTACK_INTERVAL_MS);
                                }
                            } else {
                                d.attack_ready_at_ms = now.checked_add(DRONE_ATTACK_INTERVAL_MS);
                            }
                        } else {
                            d.attack_ready_at_ms = None;
                        }
                    } else {
                        idle(&mut d);
                    }
                }
            }
            d.signal_strength = drone_signal_strength(d.position, ship_position);
            if d.signal_strength <= 0.0 {
                returning(&mut d);
            }
            if let Some(q) = d.orientation_xyzw {
                if let Some(pose) = (arkline_protocol::drone_bay_geometry::BayPose {
                    position_m: d.position,
                    orientation_xyzw: q,
                })
                .with_forward(d.heading)
                {
                    d.orientation_xyzw = Some(pose.orientation_xyzw);
                }
            }
            p.view.drones.push(d);
        }
        sync(p);
    }
    if let Some(before) = carrier_before.as_ref() {
        carrier_inventory::world::cargo::guard_free_motion(before, world, &carrier_traces, now)?;
    }
    carrier_inventory::world::cargo::settle_free_modes(world, now)?;
    // Resolve this tick's simultaneous shots against actual owned drone objects.
    for (attacker, attacker_id, pilot, id, damage) in attacks {
        let valid = world
            .pilots
            .get(&attacker)
            .and_then(|p| p.view.drones.iter().find(|d| d.id == attacker_id))
            .zip(
                world
                    .pilots
                    .get(&pilot)
                    .and_then(|p| p.view.drones.iter().find(|d| d.id == id)),
            )
            .is_some_and(|(source, target)| {
                source.signal_strength > 0.0
                    && flight::distance(source.position, target.position) <= DRONE_ATTACK_RANGE
            });
        if valid {
            let source = world.pilots.get_mut(&attacker).unwrap();
            if !power::spend(
                &mut source.view.power,
                power_spec(source.view.ship_class).drone_attack_cost_j,
                now,
            ) {
                if let Some(drone) = source.view.drones.iter_mut().find(|d| d.id == attacker_id) {
                    idle(drone);
                }
                continue;
            }
            if let Some(p) = world.pilots.get_mut(&pilot) {
                if let Some(d) = p.view.drones.iter_mut().find(|d| d.id == id) {
                    d.hit_points = d.hit_points.saturating_sub(damage);
                }
            }
        }
    }
    events.extend(drone_cargo::after_combat(world, now)?);
    drone_cargo::finish(world, now)?;
    independent_bays::world::finish(world, now)?;
    events.extend(carrier_inventory::world::finish(world, now)?);
    events.extend(carrier_inventory::world::cargo::after_combat(world, now)?);
    drone_bay::finish(world, now)?;
    for p in world.pilots.values_mut() {
        p.view.drones.retain(|d| d.hit_points > 0);
        sync(p);
    }
    invalidate(world);
    carrier_inventory::world::cargo::settle_free_modes(world, now)?;
    drone_cargo::prune_claims(world);
    Ok(events)
}
