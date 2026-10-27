use super::*;
pub(super) fn ensure_modules(world: &mut World) {
    for pilot in world.pilots.values_mut() {
        if pilot.view.mining_modules.is_empty() {
            pilot.view.mining_modules = MINING_MODULE_IDS
                .into_iter()
                .map(|module_id| MiningModule {
                    module_id,
                    target_id: None,
                    cycle_started_at_ms: None,
                    cycle_completes_at_ms: None,
                    stop_reason: None,
                })
                .collect();
        }
    }
}
fn halt(module: &mut MiningModule, reason: &str) {
    module.target_id = None;
    module.cycle_started_at_ms = None;
    module.cycle_completes_at_ms = None;
    module.stop_reason = Some(reason.into());
}
pub(super) fn stop_all(pilot: &mut Pilot, reason: &str) {
    for module in &mut pilot.view.mining_modules {
        if module.target_id.is_some() {
            halt(module, reason);
        }
    }
}
fn invalid_reason(
    p: &Pilot,
    a: Option<&Asteroid>,
    id: u64,
    carrier_advertised: bool,
) -> Option<&'static str> {
    // Source-proved deployment is not authoritative aim or line-of-fire.
    // This invalidator preserves pending Ready cycles across body sampling.
    // Actual extraction separately requires current-clock aim AND ray clearance.
    if !turret_stow::mining_aim_available(&p.view, carrier_advertised)
        && !p.view.cockpit_mechanisms.as_ref().is_some_and(|m| {
            m.turrets.aims.as_ref().is_some_and(|aims| {
                aims.iter().any(|aim| {
                    aim.target_id == Some(id)
                        && aim.status
                            == arkline_protocol::parallel_turret_aim::TurretAimStatus::Ready
                })
            })
        })
    {
        return Some("Carrier turret aim authority unavailable");
    }
    if !universe::normal(p) {
        return Some("Ship left normal space");
    }
    let Some(a) = a else {
        return Some("Target missing");
    };
    if a.system_id != p.view.system_id {
        return Some("Target in another system");
    }
    if a.ore == 0 {
        return Some("Asteroid depleted");
    }
    if !p.view.locked_asteroids.contains(&id) {
        return Some("Target lock lost");
    }
    if universe::surface(p, a) > MINING_RANGE {
        return Some("Target outside mining range");
    }
    if p.view.ore >= cargo_capacity(p.view.ship_class) {
        return Some("Cargo full");
    }
    None
}
pub(super) fn start(
    world: &mut World,
    name: &str,
    module_id: u8,
    target_id: u64,
    now: u64,
) -> Result<()> {
    ensure!(
        MINING_MODULE_IDS.contains(&module_id),
        "unknown mining module"
    );
    ensure!(
        drone_cargo::hold_capacity(world, name) > world.pilots[name].view.ore,
        "Ship headroom is reserved by mining-drone cargo"
    );
    if turret_aim::request(world, name, module_id, target_id, now)? {
        return Ok(());
    }
    let p = world.pilots.get(name).context("pilot missing")?;
    ensure!(
        turret_stow::can_fire(&p.view, now),
        "Physical turrets unavailable"
    );
    if let Some(reason) = invalid_reason(
        p,
        world.asteroids.iter().find(|a| a.id == target_id),
        target_id,
        world.carrier_inventory_registration.is_some(),
    ) {
        bail!("{reason}");
    }
    let p = world.pilots.get_mut(name).unwrap();
    let spec = mining_spec(p.view.ship_class);
    let module = p
        .view
        .mining_modules
        .iter_mut()
        .find(|m| m.module_id == module_id)
        .context("module missing")?;
    if module.target_id == Some(target_id) {
        return Ok(());
    }
    module.target_id = Some(target_id);
    module.cycle_started_at_ms = Some(now);
    module.cycle_completes_at_ms = Some(now.checked_add(spec.cycle_ms).context("clock overflow")?);
    module.stop_reason = None;
    Ok(())
}
pub(super) fn stop(world: &mut World, name: &str, module_id: u8) -> Result<()> {
    let sample = world
        .accounts
        .get(name)
        .and_then(|a| a.cockpit_mechanisms.as_ref())
        .map_or(0, |m| m.turrets.updated_at_ms);
    turret_aim::stop(world, name, module_id, sample);
    let p = world.pilots.get_mut(name).context("pilot missing")?;
    let module = p
        .view
        .mining_modules
        .iter_mut()
        .find(|m| m.module_id == module_id)
        .context("unknown mining module")?;
    halt(module, "Stopped by pilot");
    Ok(())
}
pub(super) fn invalidate(world: &mut World) {
    for p in world.pilots.values_mut() {
        for index in 0..p.view.mining_modules.len() {
            let Some(id) = p.view.mining_modules[index].target_id else {
                continue;
            };
            if let Some(reason) = invalid_reason(
                p,
                world.asteroids.iter().find(|a| a.id == id),
                id,
                world.carrier_inventory_registration.is_some(),
            ) {
                halt(&mut p.view.mining_modules[index], reason);
            }
        }
    }
}
pub(super) fn advance(world: &mut World, now: u64) -> Vec<EconomicEvent> {
    advance_phase(world, now, false)
}
/// New source guns extract only after all source/free bodies reached this tick.
pub(super) fn advance_source(world: &mut World, now: u64) -> Vec<EconomicEvent> {
    advance_phase(world, now, true)
}
fn advance_phase(world: &mut World, now: u64, source_phase: bool) -> Vec<EconomicEvent> {
    invalidate(world);
    // Geometry and target are rechecked immediately before the extraction loop;
    // this loop mutates ore/power only, never positions or source mechanisms.
    let permissions: std::collections::BTreeMap<(String, u8), bool> = world
        .pilots
        .iter()
        .flat_map(|(name, p)| {
            p.view
                .mining_modules
                .iter()
                .map(|m| (name.clone(), m.module_id))
        })
        .map(|(name, id)| {
            let ready = turret_aim::extraction_ready(world, &name, usize::from(id), now);
            ((name, id), ready)
        })
        .collect();
    let mut events = Vec::new();
    for (name, p) in &mut world.pilots {
        let source = !turret_stow::mining_aim_available(
            &p.view,
            world.carrier_inventory_registration.is_some(),
        );
        if source != source_phase {
            continue;
        }
        let capacity = cargo_capacity(p.view.ship_class).saturating_sub(
            world.accounts.get(name).map_or(0, |a| {
                drone_cargo::reserved_for_account(a, p.view.ship_class)
            }),
        );
        for index in 0..p.view.mining_modules.len() {
            let Some(id) = p.view.mining_modules[index].target_id else {
                continue;
            };
            if !permissions
                .get(&(name.clone(), p.view.mining_modules[index].module_id))
                .copied()
                .unwrap_or(false)
            {
                halt(
                    &mut p.view.mining_modules[index],
                    "Turret aim/line-of-fire not confirmed",
                );
                continue;
            }
            let a = world.asteroids.iter_mut().find(|a| a.id == id).unwrap();
            if let Some(reason) = invalid_reason(
                p,
                Some(a),
                id,
                world.carrier_inventory_registration.is_some(),
            ) {
                halt(&mut p.view.mining_modules[index], reason);
                continue;
            }
            if p.view.ore >= capacity {
                halt(
                    &mut p.view.mining_modules[index],
                    "Ship headroom reserved by drone cargo",
                );
                continue;
            }
            let spec = mining_spec(p.view.ship_class);
            let module = &mut p.view.mining_modules[index];
            let Some(due) = module.cycle_completes_at_ms else {
                halt(module, "Invalid cycle state");
                continue;
            };
            if now < due {
                continue;
            }
            // Never replay a backlog after a stalled process or suspended machine.
            if now.saturating_sub(due) >= spec.cycle_ms {
                halt(module, "Cycle expired");
                continue;
            }
            if !power::spend(
                &mut p.view.power,
                power_spec(p.view.ship_class).mining_cycle_cost_j,
                now,
            ) {
                halt(module, "Insufficient capacitor energy");
                continue;
            }
            let units = spec.yield_units.min(a.ore).min(capacity - p.view.ore);
            a.ore -= units;
            world.belt_ore -= units;
            p.view.ore += units;
            events.push(EconomicEvent::OreMined {
                pilot: name.clone(),
                units,
            });
            if a.ore == 0 {
                halt(module, "Asteroid depleted");
            } else if p.view.ore == cargo_capacity(p.view.ship_class) {
                halt(module, "Cargo full");
            } else if let Some(next) = now.checked_add(spec.cycle_ms) {
                module.cycle_started_at_ms = Some(now);
                module.cycle_completes_at_ms = Some(next);
            } else {
                halt(module, "Clock limit reached");
            }
        }
    }
    invalidate(world);
    events
}

pub(super) fn validate(world: &World) -> Result<()> {
    for p in world.pilots.values() {
        ensure!(
            p.view.mining_modules.len() == MINING_MODULE_IDS.len(),
            "invalid mining module count"
        );
        for id in MINING_MODULE_IDS {
            ensure!(
                p.view
                    .mining_modules
                    .iter()
                    .filter(|m| m.module_id == id)
                    .count()
                    == 1,
                "invalid mining module identity"
            );
        }
        for module in &p.view.mining_modules {
            match (
                module.target_id,
                module.cycle_started_at_ms,
                module.cycle_completes_at_ms,
            ) {
                (None, None, None) => {}
                (Some(_), Some(start), Some(end)) => {
                    ensure!(end > start, "invalid mining cycle interval")
                }
                _ => bail!("incomplete mining module state"),
            }
        }
    }
    Ok(())
}
