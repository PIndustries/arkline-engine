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
            !matches!(
                p.view.location,
                Location::Docked | Location::InTransit { .. }
            ),
            "target ship is not in normal space"
        );
    }
    Ok(())
}
fn target(
    target: &NavigationTarget,
    views: &BTreeMap<String, Pilot>,
) -> Option<([f64; 3], [f64; 3])> {
    match target {
        NavigationTarget::Station => Some((STATION_POSITION, [0.0; 3])),
        NavigationTarget::Belt => Some((BELT_POSITION, [0.0; 3])),
        NavigationTarget::Pilot { pilot } => {
            let p = views.get(pilot)?;
            if matches!(
                p.view.location,
                Location::Docked | Location::InTransit { .. }
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
            let elapsed = now.saturating_sub(path.started_ms);
            let t = (elapsed.saturating_sub(WARP_WARMUP_MS) as f64
                / (WARP_DURATION_MS - WARP_WARMUP_MS) as f64)
                .clamp(0.0, 1.0);
            let eased = t * t * (3.0 - 2.0 * t);
            let delta = sub(path.target, path.source);
            f.position = add(path.source, mul(delta, eased));
            f.heading = unit(delta);
            f.velocity = mul(
                delta,
                6.0 * t * (1.0 - t) / ((WARP_DURATION_MS - WARP_WARMUP_MS) as f64 / 1000.0),
            );
            f.updated_at_ms = now;
            if now >= arrives_at_ms {
                f.position = path.target;
                f.velocity = [0.0; 3];
                p.warp = None;
                p.view.location = match destination {
                    Destination::Station => Location::Station,
                    Destination::Belt => Location::Belt,
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
                    if let Some((point, target_velocity)) = target(selected, &views) {
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
