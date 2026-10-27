//! Speed-limited movement and immutable target catalogs shared by commands and simulation.

use super::super::*;

/// Move no faster than class speed; return true once inside the requested radius.
pub(super) fn approach(d: &mut DroneView, target: [f64; 3], radius: f64, now: u64) -> bool {
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

pub(super) fn targets(world: &World) -> BTreeMap<(String, u64), (u32, [f64; 3])> {
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

pub(super) fn orbit_key(target: &NavigationTarget) -> String {
    match target {
        NavigationTarget::Asteroid { id } => format!("asteroid:{id}"),
        NavigationTarget::Pilot { pilot } => format!("pilot:{pilot}"),
        _ => String::new(),
    }
}

pub(super) fn orbit_catalog(w: &World) -> BTreeMap<String, (u32, [f64; 3], f64)> {
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

pub(super) fn orbit_target(w: &World, target: &NavigationTarget) -> Option<(u32, [f64; 3], f64)> {
    orbit_catalog(w).remove(&orbit_key(target))
}

/// Continuous speed-limited tangent motion with radial correction. Never snaps to a ring.
pub(super) fn orbit(d: &mut DroneView, target: [f64; 3], radius: f64, now: u64) -> bool {
    orbit_inner(d, target, radius, now, None)
}
pub(super) fn orbit_traced(
    d: &mut DroneView,
    target: [f64; 3],
    radius: f64,
    now: u64,
) -> (bool, Vec<[f64; 3]>) {
    let mut trace = vec![d.position];
    let reached = orbit_inner(d, target, radius, now, Some(&mut trace));
    (reached, trace)
}
fn orbit_inner(
    d: &mut DroneView,
    target: [f64; 3],
    radius: f64,
    now: u64,
    mut trace: Option<&mut Vec<[f64; 3]>>,
) -> bool {
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
        if let Some(path) = trace.as_deref_mut() {
            path.push(d.position);
        }
        let actual = d.velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
        if actual > 0.001 {
            d.heading = d.velocity.map(|v| v / actual);
        }
    }
    d.updated_at_ms = now;
    flight::distance(d.position, target) <= radius + 100.0
}
