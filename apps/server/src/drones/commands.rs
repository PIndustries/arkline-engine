//! Validated legacy/fleet command selection and mode changes.

use super::super::*;
use super::lifecycle::sync;
use super::motion::orbit_target;

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
            orientation_xyzw: None,
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
