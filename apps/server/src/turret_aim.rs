//! Source-bound aim intent and mining-cycle admission. A target selection is
//! not extraction: authority must settle and prove the actual emitted ray.
use super::*;
use arkline_protocol::parallel_turret_aim::{target_angles, TurretAim, TurretAimStatus};
fn advertised(w: &World, name: &str) -> bool {
    w.pilots.get(name).is_some_and(|p| {
        !turret_stow::mining_aim_available(&p.view, w.carrier_inventory_registration.is_some())
    })
}
fn state<'a>(w: &'a World, name: &str) -> Option<&'a TurretDeployment> {
    w.accounts
        .get(name)?
        .cockpit_mechanisms
        .as_ref()
        .map(|m| &m.turrets)
}
fn store(w: &mut World, name: &str, s: TurretDeployment) {
    if let Some(m) = w
        .accounts
        .get_mut(name)
        .and_then(|a| a.cockpit_mechanisms.as_mut())
    {
        m.turrets = s.clone();
    }
    if let Some(m) = w
        .pilots
        .get_mut(name)
        .and_then(|p| p.view.cockpit_mechanisms.as_mut())
    {
        m.turrets = s;
    }
}
fn basic_target(w: &World, name: &str, id: u64) -> Result<()> {
    let p = w.pilots.get(name).context("pilot missing")?;
    ensure!(universe::normal(p), "Ship left normal space");
    let a = w
        .asteroids
        .iter()
        .find(|a| a.id == id)
        .context("Target missing")?;
    ensure!(
        a.system_id == p.view.system_id && a.ore > 0,
        "Target unavailable"
    );
    ensure!(p.view.locked_asteroids.contains(&id), "Target lock lost");
    ensure!(
        universe::surface(p, a) <= MINING_RANGE,
        "Target outside mining range"
    );
    ensure!(
        drone_cargo::hold_capacity(w, name) > p.view.ore,
        "Cargo headroom unavailable"
    );
    let spec = power_spec(p.view.ship_class);
    ensure!(
        p.view
            .power
            .as_ref()
            .is_some_and(|power| power.capacity_j == spec.capacity_j
                && power.regen_w == spec.regen_w
                && power.charge_j <= power.capacity_j
                && power.regen_remainder_mj < 1000
                && power.charge_j > 0),
        "Turret power unavailable"
    );
    Ok(())
}
pub(super) fn request(
    w: &mut World,
    name: &str,
    module: u8,
    target: u64,
    now: u64,
) -> Result<bool> {
    if !advertised(w, name) {
        return Ok(false);
    }
    ensure!(
        module < 2 && turret_stow::enabled(w, name),
        "Turret source unavailable"
    );
    basic_target(w, name, target)?;
    let mut s = state(w, name).context("Turret state unavailable")?.clone();
    ensure!(
        s.updated_at_ms == now,
        "Sample physical turret state before aim request"
    );
    ensure!(
        s.source_admitted && s.operational_at(now),
        "Physical turrets unavailable"
    );
    let delta = turret_aim_collision::target_delta(w, name, usize::from(module), target)
        .context("Target pose unavailable")?;
    ensure!(
        target_angles(delta).is_some(),
        "Target outside source-proved turret arc"
    );
    let aims = s.aims.get_or_insert_with(|| {
        std::array::from_fn(|_| TurretAim {
            sample_ms: now,
            ..Default::default()
        })
    });
    aims[usize::from(module)]
        .request(target, now)
        .map_err(anyhow::Error::msg)?;
    store(w, name, s);
    Ok(true)
}
pub(super) fn stop(w: &mut World, name: &str, module: u8, now: u64) {
    if let Some(mut s) = state(w, name).cloned() {
        if let Some(aim) = s.aims.as_mut().and_then(|a| a.get_mut(usize::from(module))) {
            aim.hold(now);
        }
        store(w, name, s);
    }
}
fn stop_cycle(p: &mut Pilot, slot: usize, reason: &str) {
    let Some(m) = p
        .view
        .mining_modules
        .iter_mut()
        .find(|m| usize::from(m.module_id) == slot)
    else {
        return;
    };
    m.target_id = None;
    m.cycle_started_at_ms = None;
    m.cycle_completes_at_ms = None;
    m.stop_reason = Some(reason.into());
}
/// Called after accepted physical deployment sampling, before mining invalidation
/// and extraction. All10 loader/service clocks remain owned by existing code.
pub(super) fn advance(w: &mut World, now: u64) {
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    for name in names {
        if !turret_stow::enabled(w, &name) {
            continue;
        }
        let Some(mut s) = state(w, &name).cloned() else {
            continue;
        };
        if s.validate_at(now).is_err() {
            continue;
        }
        let operational = s.source_admitted && s.operational_at(now);
        let aims = s.aims.get_or_insert_with(|| {
            std::array::from_fn(|_| TurretAim {
                sample_ms: now,
                ..Default::default()
            })
        });
        for (slot, aim) in aims.iter_mut().enumerate() {
            let Some(id) = aim.target_id else {
                aim.sample_ms = now;
                continue;
            };
            let outcome = (|| -> Result<TurretAimStatus> {
                ensure!(operational, "Physical turret unavailable");
                basic_target(w, &name, id)?;
                let delta = turret_aim_collision::target_delta(w, &name, slot, id)
                    .context("Target pose unavailable")?;
                if target_angles(delta).is_none() {
                    aim.sample_ms = now;
                    aim.settled_since_ms = None;
                    return Ok(TurretAimStatus::OutOfArc);
                }
                if turret_stow::obstructed(w, &name, slot, 1., 1., now) {
                    aim.sample_ms = now;
                    aim.settled_since_ms = None;
                    return Ok(TurretAimStatus::Obstructed);
                }
                aim.advance(delta, now).map_err(anyhow::Error::msg)?;
                if !aim.aligned_at(delta, now) {
                    return Ok(TurretAimStatus::Aiming);
                }
                if turret_aim_collision::clear_target(w, &name, slot, aim, now).is_none() {
                    return Ok(TurretAimStatus::Obstructed);
                }
                Ok(TurretAimStatus::Ready)
            })();
            match outcome {
                Ok(status) => aim.status = status,
                Err(_) => aim.hold(now),
            }
            let pilot = w.pilots.get_mut(&name).unwrap();
            if aim.status != TurretAimStatus::Ready {
                stop_cycle(
                    pilot,
                    slot,
                    match aim.status {
                        TurretAimStatus::OutOfArc => "OUT OF ARC",
                        TurretAimStatus::Obstructed => "OBSTRUCTED",
                        TurretAimStatus::Aiming => "AIMING",
                        _ => "Turret held",
                    },
                );
            } else {
                let Some(module) = pilot
                    .view
                    .mining_modules
                    .iter_mut()
                    .find(|m| usize::from(m.module_id) == slot)
                else {
                    aim.hold(now);
                    continue;
                };
                if module.target_id != Some(id) {
                    if let Some(end) = now.checked_add(mining_spec(pilot.view.ship_class).cycle_ms)
                    {
                        module.target_id = Some(id);
                        module.cycle_started_at_ms = Some(now);
                        module.cycle_completes_at_ms = Some(end);
                        module.stop_reason = None;
                    } else {
                        aim.hold(now);
                        stop_cycle(pilot, slot, "Clock limit reached");
                    }
                }
            }
        }
        store(w, &name, s);
    }
}
/// Fresh extraction revalidation, not permission based only on an old Ready tag.
pub(super) fn extraction_ready(w: &World, name: &str, slot: usize, now: u64) -> bool {
    if !advertised(w, name) {
        return true;
    }
    let Some(s) = state(w, name) else {
        return false;
    };
    let Some(aim) = s.aims.as_ref().and_then(|a| a.get(slot)) else {
        return false;
    };
    let Some(target) = aim.target_id else {
        return false;
    };
    w.pilots.get(name).is_some_and(|p| {
        p.view
            .mining_modules
            .iter()
            .any(|m| usize::from(m.module_id) == slot && m.target_id == Some(target))
    }) && s.operational_at(now)
        && aim.status == TurretAimStatus::Ready
        && basic_target(w, name, target).is_ok()
        && turret_aim_collision::target_delta(w, name, slot, target)
            .is_some_and(|d| aim.aligned_at(d, now))
        && turret_aim_collision::clear_target(w, name, slot, aim, now).is_some()
}
