use super::*;
pub(super) fn ensure_asteroids(world: &mut World) {
    for p in world.pilots.values_mut() {
        if p.view.target_locks.is_empty() {
            if let Some(lock) = p.view.target_lock.take() {
                p.view.target_locks.push(lock);
            }
        }
        if p.view.locked_asteroids.is_empty() {
            if let Some(id) = p.view.locked_asteroid.take() {
                p.view.locked_asteroids.push(id);
            }
        }
        sync_lock_mirrors(p);
    }
    if world.asteroids.is_empty() {
        world.asteroids = asteroid_catalog(world.belt_ore);
    }
}
pub(super) fn gate(id: u64) -> Result<Stargate> {
    gates()
        .into_iter()
        .find(|g| g.id == id)
        .context("gate not found")
}
pub(super) fn normal(p: &Pilot) -> bool {
    !docking::busy(p)
        && !matches!(
            p.view.location,
            Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
        )
}
pub(super) fn surface(p: &Pilot, a: &Asteroid) -> f64 {
    (flight::distance(p.view.flight.as_ref().unwrap().position, a.position) - a.radius).max(0.0)
}
pub(super) fn sync_lock_mirrors(p: &mut Pilot) {
    p.view.target_lock = p.view.target_locks.first().cloned();
    p.view.locked_asteroid = p.view.locked_asteroids.first().copied();
}
pub(super) fn clear_locks(p: &mut Pilot) {
    p.view.target_locks.clear();
    p.view.locked_asteroids.clear();
    sync_lock_mirrors(p);
}
/// Validate all pending and completed targets independently after movement.
pub(super) fn advance_locks(world: &mut World, now: u64) {
    for pilot in world.pilots.values_mut() {
        let valid = |id| {
            normal(pilot)
                && world.asteroids.iter().any(|a| {
                    a.id == id
                        && a.system_id == pilot.view.system_id
                        && a.ore > 0
                        && surface(pilot, a) <= LOCK_RANGE
                })
        };
        let mut completed: Vec<_> = pilot
            .view
            .locked_asteroids
            .iter()
            .copied()
            .filter(|id| valid(*id))
            .collect();
        let mut pending = Vec::new();
        for lock in &pilot.view.target_locks {
            if !valid(lock.asteroid_id) {
                continue;
            }
            if now >= lock.completes_at_ms {
                completed.push(lock.asteroid_id);
            } else {
                pending.push(lock.clone());
            }
        }
        pilot.view.locked_asteroids = completed;
        pilot.view.target_locks = pending;
        sync_lock_mirrors(pilot);
    }
}
pub(super) fn validate_locks(world: &World) -> Result<()> {
    for p in world.pilots.values() {
        let mut ids = std::collections::BTreeSet::new();
        for id in p
            .view
            .locked_asteroids
            .iter()
            .copied()
            .chain(p.view.target_locks.iter().map(|l| l.asteroid_id))
        {
            ensure!(ids.insert(id), "duplicate target lock");
        }
        ensure!(ids.len() <= MAX_TARGET_LOCKS, "target lock limit exceeded");
        ensure!(
            p.view
                .target_locks
                .iter()
                .all(|l| l.completes_at_ms >= l.started_at_ms),
            "invalid acquisition interval"
        );
    }
    Ok(())
}
