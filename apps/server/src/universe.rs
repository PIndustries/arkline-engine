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
pub(super) fn mining_target(
    world: &World,
    name: &str,
    requested: Option<u64>,
    units: u64,
) -> Result<u64> {
    let p = world.pilots.get(name).context("pilot missing")?;
    ensure!(normal(p), "mining requires normal space");
    let asteroid = if let Some(id) = requested {
        ensure!(
            p.view.locked_asteroid == Some(id),
            "lock this asteroid before mining"
        );
        world
            .asteroids
            .iter()
            .find(|a| a.id == id)
            .context("asteroid not found")?
    } else {
        world
            .asteroids
            .iter()
            .filter(|a| {
                a.system_id == p.view.system_id && a.ore >= units && surface(p, a) <= MINING_RANGE
            })
            .min_by(|a, b| surface(p, a).total_cmp(&surface(p, b)))
            .context("no minable asteroid within range")?
    };
    ensure!(
        asteroid.system_id == p.view.system_id,
        "asteroid is in another system"
    );
    ensure!(
        surface(p, asteroid) <= MINING_RANGE,
        "asteroid is outside mining range"
    );
    ensure!(asteroid.ore >= units, "asteroid has insufficient ore");
    Ok(asteroid.id)
}
