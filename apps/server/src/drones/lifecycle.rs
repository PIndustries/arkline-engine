//! Fleet normalization, transient lifecycle transitions, invalidation and durable validation.

use super::super::*;
use super::motion::{orbit_catalog, orbit_key, targets};

pub(super) fn sync(p: &mut Pilot) {
    p.view.drone = p.view.drones.first().cloned();
}

pub(super) fn clear(p: &mut Pilot) {
    p.view.drones.clear();
    sync(p);
}

pub(super) fn ensure_fleets(world: &mut World) -> Result<()> {
    for p in world.pilots.values_mut() {
        if p.view.drones.is_empty() {
            if let Some(mut legacy) = p.view.drone.take() {
                if legacy.id == 0 {
                    legacy.id = world.next_drone_id;
                    world.next_drone_id = world
                        .next_drone_id
                        .checked_add(1)
                        .context("drone identity overflow")?;
                }
                p.view.drones.push(legacy);
            }
        }
        sync(p);
    }
    Ok(())
}

pub(super) fn idle(d: &mut DroneView) {
    d.mode = DroneMode::Hold;
    d.velocity = [0.0; 3];
    d.mining_cycle = None;
    d.attack_ready_at_ms = None;
}

pub(super) fn returning(d: &mut DroneView) {
    d.mode = DroneMode::Returning;
    d.mining_cycle = None;
    d.attack_ready_at_ms = None;
}

pub(super) fn invalidate(world: &mut World) {
    let targets = targets(world);
    let orbit_targets = orbit_catalog(world);
    for p in world.pilots.values_mut() {
        if !universe::normal(p) {
            if p.view.drone_bay.is_some()
                || world.independent_bay_registration.is_some()
                || world.carrier_inventory_registration.is_some()
            {
                for d in &mut p.view.drones {
                    idle(d);
                }
                sync(p);
                continue;
            }
            clear(p);
            continue;
        }
        let Some(ship) = p.view.flight.as_ref() else {
            if p.view.drone_bay.is_some()
                || world.independent_bay_registration.is_some()
                || world.carrier_inventory_registration.is_some()
            {
                for d in &mut p.view.drones {
                    idle(d);
                }
                sync(p);
                continue;
            }
            clear(p);
            continue;
        };
        for d in &mut p.view.drones {
            d.signal_strength = drone_signal_strength(d.position, ship.position);
            if d.signal_strength <= 0.0 {
                returning(d);
            }
            let invalid = match &d.mode {
                DroneMode::Orbit { target } => orbit_targets
                    .get(&orbit_key(target))
                    .is_none_or(|(system, _, _)| *system != p.view.system_id),
                DroneMode::Mining { target_id } => {
                    !p.view.locked_asteroids.contains(target_id)
                        || p.view.ore >= cargo_capacity(p.view.ship_class)
                        || !world.asteroids.iter().any(|a| {
                            a.id == *target_id && a.system_id == p.view.system_id && a.ore > 0
                        })
                }
                DroneMode::Attacking {
                    target_pilot,
                    target_drone_id,
                } => targets
                    .get(&(target_pilot.clone(), *target_drone_id))
                    .is_none_or(|(system, _)| *system != p.view.system_id),
                _ => false,
            };
            if invalid {
                idle(d);
            }
        }
        sync(p);
    }
}

pub(super) fn validate(world: &World) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for p in world.pilots.values() {
        if world.carrier_inventory_registration.is_none() {
            ensure!(
                p.view.drones.len() <= MAX_DRONES,
                "drone fleet exceeds total cap"
            );
            for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
                ensure!(
                    p.view.drones.iter().filter(|d| d.class == class).count() <= DRONE_CLASS_CAP,
                    "drone fleet exceeds class cap"
                );
            }
        }
        for d in &p.view.drones {
            ensure!(
                d.id > 0 && d.id < world.next_drone_id && ids.insert(d.id),
                "invalid drone identity"
            );
            ensure!(
                d.hit_points > 0 && d.hit_points <= DRONE_HIT_POINTS,
                "invalid drone hit points"
            );
            ensure!(
                d.position
                    .iter()
                    .chain(d.velocity.iter())
                    .chain(d.heading.iter())
                    .all(|v| v.is_finite()),
                "nonfinite drone state"
            );
            ensure!(
                d.orientation_xyzw.is_none_or(|q| {
                    (arkline_protocol::drone_bay_geometry::BayPose {
                        position_m: d.position,
                        orientation_xyzw: q,
                    })
                    .valid()
                }),
                "invalid full drone attitude"
            );
            ensure!(
                d.signal_strength.is_finite() && (0.0..=1.0).contains(&d.signal_strength),
                "invalid drone signal"
            );
            ensure!(
                d.velocity.iter().map(|v| v * v).sum::<f64>().sqrt() <= drone_speed(d.class) + 1e-6,
                "drone speed exceeds limit"
            );
            if let DroneMode::FlyTo { position } = d.mode {
                ensure!(
                    position
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND),
                    "invalid durable drone destination"
                );
            }
            if let Some(cycle) = &d.mining_cycle {
                ensure!(
                    matches!(d.mode, DroneMode::Mining { .. })
                        && cycle.completes_at_ms > cycle.started_at_ms,
                    "invalid drone mining phase"
                );
            }
        }
    }
    Ok(())
}
