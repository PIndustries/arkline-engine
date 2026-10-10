use super::*;
pub(super) fn ensure_asteroids(world: &mut World) {
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
    !matches!(
        p.view.location,
        Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
    )
}
pub(super) fn surface(p: &Pilot, a: &Asteroid) -> f64 {
    (flight::distance(p.view.flight.as_ref().unwrap().position, a.position) - a.radius).max(0.0)
}
/// Validate both pending and completed targets after authoritative movement.
pub(super) fn advance_locks(world: &mut World, now: u64) {
    for pilot in world.pilots.values_mut() {
        let id = pilot
            .view
            .target_lock
            .as_ref()
            .map(|l| l.asteroid_id)
            .or(pilot.view.locked_asteroid);
        let valid = id.is_some_and(|id| {
            normal(pilot)
                && world.asteroids.iter().any(|a| {
                    a.id == id
                        && a.system_id == pilot.view.system_id
                        && a.ore > 0
                        && surface(pilot, a) <= LOCK_RANGE
                })
        });
        if !valid {
            pilot.view.target_lock = None;
            pilot.view.locked_asteroid = None;
        } else if pilot
            .view
            .target_lock
            .as_ref()
            .is_some_and(|l| now >= l.completes_at_ms)
        {
            pilot.view.locked_asteroid = pilot.view.target_lock.take().map(|l| l.asteroid_id);
        }
    }
}
