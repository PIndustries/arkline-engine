use super::*;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct WarpPath {
    source: [f64; 3],
    target: [f64; 3],
    started_ms: u64,
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn mul(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|x| x * s)
}
pub(super) fn length(a: [f64; 3]) -> f64 {
    a.iter().map(|x| x * x).sum::<f64>().sqrt()
}
fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = length(a);
    if n > 1e-9 {
        mul(a, 1.0 / n)
    } else {
        [1.0, 0.0, 0.0]
    }
}
fn limit(a: [f64; 3], max: f64) -> [f64; 3] {
    let n = length(a);
    if n > max {
        mul(a, max / n)
    } else {
        a
    }
}
pub(super) fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    length(sub(a, b))
}
pub(super) fn berth(name: &str, destination: Destination) -> [f64; 3] {
    // Stable visual separation, not collision avoidance or physical docking.
    let hash = name
        .bytes()
        .fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619));
    let x = (hash % 7) as f64 * 150.0 - 450.0;
    let y = ((hash / 7) % 7) as f64 * 150.0 - 450.0;
    add(
        match destination {
            Destination::Station => STATION_POSITION,
            Destination::Belt => BELT_POSITION,
            Destination::Gate { id } => gates()
                .into_iter()
                .find(|g| g.id == id)
                .map_or(STATION_POSITION, |g| g.position),
        },
        [x, y, 1000.0],
    )
}
fn initial(name: &str, location: &Location) -> [f64; 3] {
    match location {
        Location::Docked => STATION_POSITION,
        Location::Belt => berth(name, Destination::Belt),
        Location::InTransit {
            destination: Destination::Station,
            ..
        } => berth(name, Destination::Belt),
        _ => berth(name, Destination::Station),
    }
}
pub(super) fn ensure_flight(p: &mut Pilot, now: u64) {
    if p.view.flight.is_none() {
        p.view.flight = Some(FlightState {
            position: initial(&p.view.name, &p.view.location),
            velocity: [0.0; 3],
            heading: [1.0, 0.0, 0.0],
            mode: NavigationMode::Stop,
            updated_at_ms: now,
            warp: None,
        });
    }
}
pub(super) fn validate_mode(mode: &NavigationMode, name: &str, world: &World) -> Result<()> {
    let target = match mode {
        NavigationMode::Stop => None,
        NavigationMode::FlyTo { position } => {
            ensure!(
                position
                    .iter()
                    .all(|x| x.is_finite() && x.abs() <= SPACE_BOUND),
                "flight destination must be finite and within sector bounds"
            );
            None
        }
        NavigationMode::Approach { target } => Some(target),
        NavigationMode::Orbit { target, radius } => {
            ensure!(
                radius.is_finite() && (100.0..=10000.0).contains(radius),
                "orbit radius must be 100-10000 meters"
            );
            Some(target)
        }
        NavigationMode::KeepRange { target, distance } => {
            ensure!(
                distance.is_finite() && (100.0..=10000.0).contains(distance),
                "range must be 100-10000 meters"
            );
            Some(target)
        }
    };
    if let Some(NavigationTarget::Pilot { pilot }) = target {
        ensure!(pilot != name, "cannot navigate relative to yourself");
        let p = world.pilots.get(pilot).context("target pilot not found")?;
        ensure!(
            p.view.system_id == world.pilots[name].view.system_id,
            "target pilot is in another system"
        );
        ensure!(
            !matches!(
                p.view.location,
                Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
            ),
            "target ship is not in normal space"
        );
    }
    if let Some(NavigationTarget::Asteroid { id }) = target {
        let a = world
            .asteroids
            .iter()
            .find(|a| a.id == *id)
            .context("asteroid not found")?;
        ensure!(
            a.system_id == world.pilots[name].view.system_id,
            "asteroid is in another system"
        );
    }
    if let Some(NavigationTarget::Gate { id }) = target {
        ensure!(
            universe::gate(*id)?.system_id == world.pilots[name].view.system_id,
            "gate is in another system"
        );
    }
    Ok(())
}
fn target(
    target: &NavigationTarget,
    views: &BTreeMap<String, Pilot>,
    asteroids: &[Asteroid],
    system: u32,
) -> Option<([f64; 3], [f64; 3])> {
    match target {
        NavigationTarget::Station => Some((STATION_POSITION, [0.0; 3])),
        NavigationTarget::Belt => Some((BELT_POSITION, [0.0; 3])),
        NavigationTarget::Asteroid { id } => {
            let a = asteroids
                .iter()
                .find(|a| a.id == *id && a.system_id == system)?;
            Some((a.position, [0.0; 3]))
        }
        NavigationTarget::Gate { id } => {
            let g = gates()
                .into_iter()
                .find(|g| g.id == *id && g.system_id == system)?;
            Some((g.position, [0.0; 3]))
        }
        NavigationTarget::Pilot { pilot } => {
            let p = views.get(pilot)?;
            if p.view.system_id != system {
                return None;
            }
            if matches!(
                p.view.location,
                Location::Docked | Location::InTransit { .. } | Location::Jumping { .. }
            ) {
                return None;
            }
            let f = p.view.flight.as_ref()?;
            Some((f.position, f.velocity))
        }
    }
}
pub(super) fn begin_warp(p: &mut Pilot, destination: Destination, now: u64) {
    let f = p.view.flight.as_mut().unwrap();
    p.warp = Some(WarpPath {
        source: f.position,
        target: berth(&p.view.name, destination),
        started_ms: now,
    });
    f.warp = Some(plan_warp(f.position, berth(&p.view.name, destination), now));
    f.mode = NavigationMode::Stop;
    f.velocity = [0.0; 3];
    f.updated_at_ms = now;
}
pub(super) fn advance(world: &mut World, now: u64) {
    for p in world.pilots.values_mut() {
        ensure_flight(p, now);
    }
    let views = world.pilots.clone();
    for p in world.pilots.values_mut() {
        let f = p.view.flight.as_mut().unwrap();
        if let Location::Jumping {
            gate_id,
            arrives_at_ms,
        } = p.view.location
        {
            if now >= arrives_at_ms {
                if let Ok(gate) = universe::gate(gate_id) {
                    p.view.system_id = gate.destination_system_id;
                    f.position = berth(
                        &p.view.name,
                        Destination::Gate {
                            id: gate.arrival_gate_id,
                        },
                    );
                    f.velocity = [0.0; 3];
                    f.mode = NavigationMode::Stop;
                    f.updated_at_ms = now;
                    p.view.location = Location::Station;
                    p.view.locked_asteroid = None;
                }
            }
            continue;
        }
        if let Location::InTransit {
            destination,
            arrives_at_ms,
        } = p.view.location
        {
            let path = p.warp.get_or_insert_with(|| WarpPath {
                source: f.position,
                target: berth(&p.view.name, destination),
                started_ms: arrives_at_ms.saturating_sub(WARP_DURATION_MS),
            });
            let trajectory = f
                .warp
                .get_or_insert_with(|| plan_warp(path.source, path.target, path.started_ms));
            let (position, velocity) = sample_warp(trajectory, now);
            f.position = position;
            f.velocity = velocity;
            f.heading = unit(sub(trajectory.target, trajectory.source));
            f.updated_at_ms = now;
            if now >= arrives_at_ms {
                f.position = path.target;
                f.velocity = [0.0; 3];
                p.warp = None;
                f.warp = None;
                p.view.location = match destination {
                    Destination::Station => Location::Station,
                    Destination::Belt => Location::Belt,
                    Destination::Gate { .. } => Location::Station,
                };
            }
            continue;
        }
        if p.view.location == Location::Docked {
            continue;
        }
        if matches!(f.mode, NavigationMode::Stop) && length(f.velocity) < 0.001 {
            continue;
        }
        let mut remaining = (now.saturating_sub(f.updated_at_ms) as f64 / 1000.0).min(10.0);
        while remaining > 1e-6 {
            let dt = remaining.min(0.05);
            remaining -= dt;
            let mut destination = None;
            let desired = match &f.mode {
                NavigationMode::Stop => [0.0; 3],
                NavigationMode::FlyTo { position } => {
                    destination = Some(*position);
                    let d = sub(*position, f.position);
                    mul(
                        unit(d),
                        (2.0 * SHIP_ACCELERATION * length(d))
                            .sqrt()
                            .min(MAX_SHIP_SPEED),
                    )
                }
                mode => {
                    let (selected, radius, orbit) = match mode {
                        NavigationMode::Approach { target } => (target, 150.0, false),
                        NavigationMode::Orbit { target, radius } => (target, *radius, true),
                        NavigationMode::KeepRange { target, distance } => {
                            (target, *distance, false)
                        }
                        _ => unreachable!(),
                    };
                    if let Some((point, target_velocity)) =
                        target(selected, &views, &world.asteroids, p.view.system_id)
                    {
                        let away = sub(f.position, point);
                        let radial = unit(away);
                        let error = length(away) - radius;
                        let correction = if orbit {
                            (error * 0.9).clamp(-MAX_SHIP_SPEED, MAX_SHIP_SPEED)
                        } else {
                            error.signum()
                                * (2.0 * SHIP_ACCELERATION * error.abs())
                                    .sqrt()
                                    .min(MAX_SHIP_SPEED)
                        };
                        let mut v = sub(target_velocity, mul(radial, correction));
                        if orbit {
                            let tangent = unit([-radial[2], 0.0, radial[0]]);
                            v = add(
                                v,
                                mul(
                                    tangent,
                                    (SHIP_ACCELERATION * radius).sqrt().min(MAX_SHIP_SPEED) * 0.55,
                                ),
                            );
                        }
                        limit(v, MAX_SHIP_SPEED)
                    } else {
                        f.mode = NavigationMode::Stop;
                        [0.0; 3]
                    }
                }
            };
            let old = f.velocity;
            f.velocity = add(old, limit(sub(desired, old), SHIP_ACCELERATION * dt));
            f.position = add(f.position, mul(add(old, f.velocity), dt * 0.5));
            if let Some(point) = destination {
                if distance(f.position, point) < 0.6 && length(f.velocity) < 12.0 {
                    f.position = point;
                    f.velocity = [0.0; 3];
                    f.mode = NavigationMode::Stop;
                }
            }
            if matches!(f.mode, NavigationMode::Stop) && length(f.velocity) < 0.001 {
                f.velocity = [0.0; 3];
            }
            for i in 0..3 {
                if f.position[i].abs() > SPACE_BOUND {
                    f.position[i] = f.position[i].clamp(-SPACE_BOUND, SPACE_BOUND);
                    f.velocity = [0.0; 3];
                    f.mode = NavigationMode::Stop;
                }
            }
            if length(f.velocity) > 0.01 {
                f.heading = unit(f.velocity);
            }
        }
        f.updated_at_ms = now;
        p.view.location =
            if distance(f.position, BELT_POSITION) < distance(f.position, STATION_POSITION) {
                Location::Belt
            } else {
                Location::Station
            };
    }
}

/// One-time migration from compact sector coordinates to astronomical anchors.
/// Move local offsets, never balances, reserves, ledger entries, or identities.
pub(super) fn migrate_spatial(world: &mut World) {
    if world.spatial_version >= 2 {
        return;
    }
    fn relocate(position: [f64; 3], system: u32) -> [f64; 3] {
        let mut anchors = vec![
            (STATION_POSITION, STATION_POSITION),
            ([6200.0, 0.0, 0.0], BELT_POSITION),
        ];
        for gate in gates().into_iter().filter(|g| g.system_id == system) {
            let old = match gate.id {
                101 => [12000.0, 0.0, -8000.0],
                201 => [-12000.0, 0.0, -8000.0],
                202 => [12000.0, 0.0, 8000.0],
                _ => [-12000.0, 0.0, 8000.0],
            };
            anchors.push((old, gate.position));
        }
        let (old, new) = anchors
            .into_iter()
            .min_by(|a, b| distance(position, a.0).total_cmp(&distance(position, b.0)))
            .unwrap();
        add(new, sub(position, old))
    }
    for asteroid in &mut world.asteroids {
        asteroid.position = add(BELT_POSITION, sub(asteroid.position, [6200.0, 0.0, 0.0]));
    }
    for pilot in world.pilots.values_mut() {
        if let Some(f) = &mut pilot.view.flight {
            f.position = relocate(f.position, pilot.view.system_id);
            f.velocity = [0.0; 3];
            f.mode = NavigationMode::Stop;
            f.warp = None;
        }
        if let Location::InTransit { destination, .. } = pilot.view.location {
            let now = now_ms();
            ensure_flight(pilot, now);
            begin_warp(pilot, destination, now);
            pilot.view.location = Location::InTransit {
                destination,
                arrives_at_ms: pilot
                    .view
                    .flight
                    .as_ref()
                    .unwrap()
                    .warp
                    .as_ref()
                    .unwrap()
                    .arrives_at_ms,
            };
        }
    }
    world.spatial_version = 2;
}
