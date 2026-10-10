use super::*;
pub(super) fn handles(request: &Request) -> bool {
    matches!(
        request,
        Request::LaunchDrone
            | Request::RecallDrone
            | Request::DroneHold
            | Request::DroneNavigate { .. }
    )
}
pub(super) fn execute(world: &mut World, name: &str, request: Request, now: u64) -> Result<()> {
    let pilot = world.pilots.get_mut(name).context("pilot missing")?;
    ensure!(
        universe::normal(pilot),
        "drone operations require undocked normal space"
    );
    let flight = pilot
        .view
        .flight
        .as_ref()
        .context("ship position unavailable")?;
    match request {
        Request::LaunchDrone => {
            ensure!(pilot.view.drone.is_none(), "recon drone already deployed");
            let mut position = flight.position;
            let hull = pilot.view.ship_class.spec();
            for (coordinate, heading) in position.iter_mut().zip(flight.heading) {
                *coordinate += heading * (hull.length_m / 2.0 + 100.0);
            }
            position[1] += hull.height_m / 2.0 + 50.0;
            pilot.view.drone = Some(DroneView {
                position,
                velocity: [0.0; 3],
                heading: flight.heading,
                mode: DroneMode::Hold,
                updated_at_ms: now,
            });
        }
        Request::RecallDrone => {
            let drone = pilot.view.drone.as_mut().context("no deployed drone")?;
            drone.mode = DroneMode::Returning;
            drone.updated_at_ms = now;
        }
        Request::DroneHold => {
            let drone = pilot.view.drone.as_mut().context("no deployed drone")?;
            drone.mode = DroneMode::Hold;
            drone.velocity = [0.0; 3];
            drone.updated_at_ms = now;
        }
        Request::DroneNavigate { position } => {
            ensure!(
                position
                    .iter()
                    .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND),
                "invalid drone destination"
            );
            ensure!(
                flight::distance(position, flight.position) <= DRONE_MAX_RANGE,
                "drone destination outside control range"
            );
            let drone = pilot.view.drone.as_mut().context("no deployed drone")?;
            drone.mode = DroneMode::FlyTo { position };
            drone.updated_at_ms = now;
        }
        _ => unreachable!(),
    }
    Ok(())
}
pub(super) fn advance(world: &mut World, now: u64) {
    for pilot in world.pilots.values_mut() {
        let Some(mut drone) = pilot.view.drone.take() else {
            continue;
        };
        let Some(ship) = pilot.view.flight.as_ref() else {
            continue;
        };
        // Prototype automatic recovery prevents orphaned drones across transitions.
        if !universe::normal(pilot)
            || flight::distance(drone.position, ship.position) > DRONE_MAX_RANGE
        {
            continue;
        }
        let target = match drone.mode {
            DroneMode::Hold => {
                pilot.view.drone = Some(drone);
                continue;
            }
            DroneMode::FlyTo { position } => position,
            DroneMode::Returning => ship.position,
        };
        let distance = flight::distance(drone.position, target);
        if matches!(drone.mode, DroneMode::Returning) && distance <= 100.0 {
            continue;
        }
        let dt = (now.saturating_sub(drone.updated_at_ms) as f64 / 1000.0).min(10.0);
        if distance > 0.001 {
            let direction =
                std::array::from_fn::<_, 3, _>(|i| (target[i] - drone.position[i]) / distance);
            let step = (DRONE_SPEED * dt).min(distance);
            for (i, d) in direction.iter().enumerate() {
                drone.position[i] += d * step;
            }
            drone.heading = direction;
            drone.velocity = direction.map(|d| d * DRONE_SPEED);
            if step >= distance {
                if matches!(drone.mode, DroneMode::Returning) {
                    continue;
                }
                drone.position = target;
                drone.velocity = [0.0; 3];
                drone.mode = DroneMode::Hold;
            }
        } else {
            drone.velocity = [0.0; 3];
            drone.mode = DroneMode::Hold;
        }
        drone.updated_at_ms = now;
        if flight::distance(drone.position, ship.position) <= DRONE_MAX_RANGE {
            pilot.view.drone = Some(drone);
        }
    }
}

pub(super) fn validate(world: &World) -> Result<()> {
    for pilot in world.pilots.values() {
        if let Some(drone) = &pilot.view.drone {
            ensure!(
                drone
                    .position
                    .iter()
                    .chain(drone.velocity.iter())
                    .chain(drone.heading.iter())
                    .all(|v| v.is_finite()),
                "nonfinite drone state"
            );
            ensure!(
                drone.velocity.iter().map(|v| v * v).sum::<f64>().sqrt() <= DRONE_SPEED + 1e-6,
                "drone speed exceeds limit"
            );
            if let DroneMode::FlyTo { position } = drone.mode {
                ensure!(
                    position
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND),
                    "invalid durable drone destination"
                );
            }
        }
    }
    Ok(())
}
