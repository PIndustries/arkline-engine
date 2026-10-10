use super::*;
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
pub(super) fn handles(r: &Request) -> bool {
    matches!(
        r,
        Request::OrbitDrone { .. }
            | Request::LaunchDrone
            | Request::LaunchDroneClass { .. }
            | Request::RecallDrone
            | Request::DroneHold
            | Request::DroneNavigate { .. }
            | Request::RecallDroneById { .. }
            | Request::DroneHoldById { .. }
            | Request::DroneNavigateById { .. }
            | Request::AssignMiningDrone { .. }
            | Request::AssignAttackDrone { .. }
    )
}
fn idle(d: &mut DroneView) {
    d.mode = DroneMode::Hold;
    d.velocity = [0.0; 3];
    d.mining_cycle = None;
    d.attack_ready_at_ms = None;
}
fn returning(d: &mut DroneView) {
    d.mode = DroneMode::Returning;
    d.mining_cycle = None;
    d.attack_ready_at_ms = None;
}
pub(super) fn execute(world: &mut World, name: &str, request: Request, now: u64) -> Result<()> {
    let p = world.pilots.get(name).context("pilot missing")?;
    ensure!(
        universe::normal(p),
        "drone operations require undocked normal space"
    );
    let ship = p
        .view
        .flight
        .as_ref()
        .context("ship position unavailable")?
        .clone();
    if matches!(
        request,
        Request::LaunchDrone | Request::LaunchDroneClass { .. }
    ) {
        let class = if let Request::LaunchDroneClass { class } = request {
            class
        } else {
            ensure!(
                !p.view.drones.iter().any(|d| d.class == DroneClass::Recon),
                "recon drone already deployed; use fleet launch for additional drones"
            );
            DroneClass::Recon
        };
        ensure!(
            p.view.drones.len() < MAX_DRONES
                && p.view.drones.iter().filter(|d| d.class == class).count() < DRONE_CLASS_CAP,
            "five drones per class maximum"
        );
        let id = world.next_drone_id;
        world.next_drone_id = id.checked_add(1).context("drone identity overflow")?;
        let hull = p.view.ship_class.spec();
        // Use a body-relative formation, selecting a free geometric slot rather
        // than fleet length so recalling a middle drone cannot stack new hulls.
        let position = (0..MAX_DRONES)
            .find_map(|slot| {
                let column = [0.0, 1.0, -1.0, 2.0, -2.0][slot % 5];
                let body_offset = [
                    hull.length_m / 2.0 + 100.0,
                    hull.height_m / 2.0 + 50.0 + (slot / 5) as f64 * 24.0,
                    column * 24.0,
                ];
                let offset = flight::rotate(ship.orientation, body_offset);
                let candidate = std::array::from_fn(|i| ship.position[i] + offset[i]);
                p.view
                    .drones
                    .iter()
                    .all(|drone| flight::distance(candidate, drone.position) >= 10.0)
                    .then_some(candidate)
            })
            .context("drone launch formation is occupied")?;
        let d = DroneView {
            id,
            class,
            hit_points: DRONE_HIT_POINTS,
            signal_strength: drone_signal_strength(position, ship.position),
            mining_cycle: None,
            attack_ready_at_ms: None,
            position,
            velocity: [0.0; 3],
            heading: ship.heading,
            mode: DroneMode::Hold,
            updated_at_ms: now,
        };
        let p = world.pilots.get_mut(name).unwrap();
        p.view.drones.push(d);
        sync(p);
        return Ok(());
    }
    let id = match &request {
        Request::OrbitDrone { id, .. }
        | Request::RecallDroneById { id }
        | Request::DroneHoldById { id }
        | Request::DroneNavigateById { id, .. }
        | Request::AssignMiningDrone { id, .. }
        | Request::AssignAttackDrone { id, .. } => *id,
        _ => p.view.drones.first().context("no deployed drone")?.id,
    };
    let d = p
        .view
        .drones
        .iter()
        .find(|d| d.id == id)
        .context("drone is not owned by this pilot")?;
    let recall = matches!(
        request,
        Request::RecallDrone | Request::RecallDroneById { .. }
    );
    ensure!(
        recall || drone_signal_strength(d.position, ship.position) > 0.0,
        "drone control signal lost; autonomous return active"
    );
    let mode = match request {
        Request::OrbitDrone { target, .. } => {
            let (system, position, _) = orbit_target(world, &target)
                .context("orbit requires an available asteroid or pilot")?;
            ensure!(
                system == p.view.system_id,
                "orbit target is in another system"
            );
            ensure!(
                flight::distance(position, ship.position) < DRONE_MAX_RANGE,
                "orbit target outside control range"
            );
            DroneMode::Orbit { target }
        }
        Request::RecallDrone | Request::RecallDroneById { .. } => DroneMode::Returning,
        Request::DroneHold | Request::DroneHoldById { .. } => DroneMode::Hold,
        Request::DroneNavigate { position } | Request::DroneNavigateById { position, .. } => {
            ensure!(
                position
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND),
                "invalid drone destination"
            );
            ensure!(
                flight::distance(position, ship.position) <= DRONE_MAX_RANGE,
                "drone destination outside control range"
            );
            DroneMode::FlyTo { position }
        }
        Request::AssignMiningDrone { target_id, .. } => {
            ensure!(
                d.class == DroneClass::Mining,
                "only mining drones can extract ore"
            );
            ensure!(
                p.view.locked_asteroids.contains(&target_id),
                "confirmed target lock required"
            );
            let a = world
                .asteroids
                .iter()
                .find(|a| a.id == target_id)
                .context("asteroid missing")?;
            ensure!(
                a.system_id == p.view.system_id && a.ore > 0,
                "asteroid unavailable"
            );
            ensure!(p.view.ore < cargo_capacity(p.view.ship_class), "cargo full");
            DroneMode::Mining { target_id }
        }
        Request::AssignAttackDrone {
            target_pilot,
            target_drone_id,
            ..
        } => {
            ensure!(
                d.class == DroneClass::Attack,
                "only attack drones can engage"
            );
            ensure!(target_pilot != name, "cannot attack own drone");
            let target = world
                .pilots
                .get(&target_pilot)
                .context("target pilot missing")?;
            ensure!(
                target.view.system_id == p.view.system_id && universe::normal(target),
                "target not in this system's normal space"
            );
            ensure!(
                target
                    .view
                    .drones
                    .iter()
                    .any(|d| d.id == target_drone_id && d.hit_points > 0),
                "target drone missing"
            );
            DroneMode::Attacking {
                target_pilot,
                target_drone_id,
            }
        }
        _ => unreachable!(),
    };
    let p = world.pilots.get_mut(name).unwrap();
    let d = p.view.drones.iter_mut().find(|d| d.id == id).unwrap();
    if d.mode != mode {
        d.mode = mode;
        d.mining_cycle = None;
        d.attack_ready_at_ms = None;
        d.velocity = [0.0; 3];
        d.updated_at_ms = now;
    }
    sync(p);
    Ok(())
}
/// Move no faster than class speed; return true once inside the requested radius.
fn approach(d: &mut DroneView, target: [f64; 3], radius: f64, now: u64) -> bool {
    let distance = flight::distance(d.position, target);
    let dt = (now.saturating_sub(d.updated_at_ms) as f64 / 1000.0).min(10.0);
    if distance > 0.001 {
        let direction = std::array::from_fn::<_, 3, _>(|i| (target[i] - d.position[i]) / distance);
        d.heading = direction;
        let step = (drone_speed(d.class) * dt).min((distance - radius).max(0.0));
        for (coordinate, dir) in d.position.iter_mut().zip(direction) {
            *coordinate += dir * step;
        }
        d.velocity = if distance - step > radius + 0.001 {
            direction.map(|v| v * drone_speed(d.class))
        } else {
            [0.0; 3]
        };
    } else {
        d.velocity = [0.0; 3];
    }
    d.updated_at_ms = now;
    flight::distance(d.position, target) <= radius + 0.001
}
fn targets(world: &World) -> BTreeMap<(String, u64), (u32, [f64; 3])> {
    world
        .pilots
        .iter()
        .filter(|(_, p)| universe::normal(p))
        .flat_map(|(name, p)| {
            p.view
                .drones
                .iter()
                .filter(|d| d.hit_points > 0)
                .map(move |d| ((name.clone(), d.id), (p.view.system_id, d.position)))
        })
        .collect()
}
pub(super) fn invalidate(world: &mut World) {
    let targets = targets(world);
    let orbit_targets = orbit_catalog(world);
    for p in world.pilots.values_mut() {
        if !universe::normal(p) {
            clear(p);
            continue;
        }
        let Some(ship) = p.view.flight.as_ref() else {
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
pub(super) fn advance(world: &mut World, now: u64) -> Vec<EconomicEvent> {
    invalidate(world);
    let positions = targets(world);
    let orbit_targets = orbit_catalog(world);
    let mut attacks = Vec::new();
    let mut events = Vec::new();
    for (name, p) in &mut world.pilots {
        let Some(ship) = p.view.flight.as_ref() else {
            continue;
        };
        let ship_position = ship.position;
        let mut fleet = std::mem::take(&mut p.view.drones);
        for mut d in fleet.drain(..) {
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
                    if p.view.ore >= cargo_capacity(p.view.ship_class) {
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
            p.view.drones.push(d);
        }
        sync(p);
    }
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
    for p in world.pilots.values_mut() {
        p.view.drones.retain(|d| d.hit_points > 0);
        sync(p);
    }
    invalidate(world);
    events
}
pub(super) fn validate(world: &World) -> Result<()> {
    let mut ids = std::collections::BTreeSet::new();
    for p in world.pilots.values() {
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

fn orbit_key(target: &NavigationTarget) -> String {
    match target {
        NavigationTarget::Asteroid { id } => format!("asteroid:{id}"),
        NavigationTarget::Pilot { pilot } => format!("pilot:{pilot}"),
        _ => String::new(),
    }
}
fn orbit_catalog(w: &World) -> BTreeMap<String, (u32, [f64; 3], f64)> {
    let mut result = BTreeMap::new();
    for a in &w.asteroids {
        result.insert(
            format!("asteroid:{}", a.id),
            (a.system_id, a.position, a.radius + 300.0),
        );
    }
    for (name, p) in &w.pilots {
        if universe::normal(p) {
            if let Some(f) = &p.view.flight {
                result.insert(
                    format!("pilot:{name}"),
                    (
                        p.view.system_id,
                        f.position,
                        hull_radius_m(p.view.ship_class) + 300.0,
                    ),
                );
            }
        }
    }
    result
}
fn orbit_target(w: &World, target: &NavigationTarget) -> Option<(u32, [f64; 3], f64)> {
    orbit_catalog(w).remove(&orbit_key(target))
}
/// Continuous speed-limited tangent motion with radial correction. Never snaps to a ring.
fn orbit(d: &mut DroneView, target: [f64; 3], radius: f64, now: u64) -> bool {
    let mut remaining = (now.saturating_sub(d.updated_at_ms) as f64 / 1000.0).min(10.0);
    let speed = drone_speed(d.class);
    while remaining > 0.000001 {
        let dt = remaining.min(0.05);
        remaining -= dt;
        let delta = std::array::from_fn::<_, 3, _>(|i| d.position[i] - target[i]);
        let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
        let radial = if distance > 0.001 {
            delta.map(|v| v / distance)
        } else {
            [1.0, 0.0, 0.0]
        };
        let mut tangent = [-radial[2], 0.0, radial[0]];
        let length = tangent.iter().map(|v| v * v).sum::<f64>().sqrt();
        if length > 0.001 {
            tangent = tangent.map(|v| v / length);
        } else {
            tangent = [1.0, 0.0, 0.0];
        }
        let correction = ((radius - distance) * 1.2).clamp(-speed, speed);
        let tangential = if distance > radius + 200.0 {
            0.0
        } else {
            speed * 0.7
        };
        let velocity =
            std::array::from_fn::<_, 3, _>(|i| radial[i] * correction + tangent[i] * tangential);
        let magnitude = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
        d.velocity = velocity.map(|v| {
            v * if magnitude > speed {
                speed / magnitude
            } else {
                1.0
            }
        });
        for i in 0..3 {
            d.position[i] += d.velocity[i] * dt;
        }
        let actual = d.velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
        if actual > 0.001 {
            d.heading = d.velocity.map(|v| v / actual);
        }
    }
    d.updated_at_ms = now;
    flight::distance(d.position, target) <= radius + 100.0
}
