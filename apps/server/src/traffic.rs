//! Deterministic background traffic, separate from player accounts and economy.
//!
//! These actors follow a small repeating station-service schedule. They neither
//! dock into inventories nor create trades, mine ore, or count as online pilots.
use arkline_protocol::{
    systems, ShipClass, TrafficActor, TrafficPhase, TrafficRole, STATION_POSITION,
};

const LEG_MS: u64 = 180_000;
const HOLD_MS: u64 = 30_000;
const PERIOD_MS: u64 = 2 * (LEG_MS + HOLD_MS);

pub(super) fn actors(now: u64) -> Vec<TrafficActor> {
    let mut actors = Vec::with_capacity(9);
    for system in systems() {
        for lane in 0..3_u64 {
            // Bound arithmetic before adding offsets, including at u64::MAX.
            let elapsed =
                (now % PERIOD_MS + lane * 117_000 + u64::from(system.id) * 31_000) % PERIOD_MS;
            let (role, ship_class) = match lane {
                0 => (TrafficRole::StationShuttle, ShipClass::Pioneer),
                1 => (TrafficRole::FreightTender, ShipClass::Hauler),
                _ => (TrafficRole::Patrol, ShipClass::Prospector),
            };
            // All hulls remain outside the station's front surface and away from
            // the player berth (+Z 1000m, ±450m X/Y). Lanes never cross the port.
            let near = [
                STATION_POSITION[0] + 2500.0 + lane as f64 * 1000.0,
                (lane as f64 - 1.0) * 1800.0,
                4500.0 + lane as f64 * 1600.0,
            ];
            let far = [near[0] + 14_000.0, near[1] + 1800.0, near[2] + 8000.0];
            let (phase, position, velocity, heading) = if elapsed < LEG_MS {
                sample_leg(far, near, elapsed, TrafficPhase::Arriving)
            } else if elapsed < LEG_MS + HOLD_MS {
                (
                    TrafficPhase::Holding,
                    near,
                    [0.0; 3],
                    turn_heading(direction(far, near), elapsed - LEG_MS),
                )
            } else if elapsed < 2 * LEG_MS + HOLD_MS {
                sample_leg(
                    near,
                    far,
                    elapsed - LEG_MS - HOLD_MS,
                    TrafficPhase::Departing,
                )
            } else {
                (
                    TrafficPhase::Holding,
                    far,
                    [0.0; 3],
                    turn_heading(direction(near, far), elapsed - 2 * LEG_MS - HOLD_MS),
                )
            };
            actors.push(TrafficActor {
                id: u64::from(system.id) * 100 + lane + 1,
                system_id: system.id,
                role,
                ship_class,
                position,
                velocity,
                heading,
                phase,
            });
        }
    }
    actors
}
fn direction(from: [f64; 3], to: [f64; 3]) -> [f64; 3] {
    let delta = std::array::from_fn::<_, 3, _>(|i| to[i] - from[i]);
    let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
    delta.map(|v| v / length)
}
fn turn_heading(direction: [f64; 3], elapsed: u64) -> [f64; 3] {
    let side = [direction[2], 0.0, -direction[0]];
    let length = side.iter().map(|v| v * v).sum::<f64>().sqrt();
    let t = elapsed as f64 / HOLD_MS as f64;
    let angle = std::f64::consts::PI * t * t * (3.0 - 2.0 * t);
    std::array::from_fn(|i| direction[i] * angle.cos() + side[i] / length * angle.sin())
}
fn sample_leg(
    from: [f64; 3],
    to: [f64; 3],
    elapsed: u64,
    phase: TrafficPhase,
) -> (TrafficPhase, [f64; 3], [f64; 3], [f64; 3]) {
    let t = elapsed as f64 / LEG_MS as f64;
    let progress = t * t * (3.0 - 2.0 * t);
    let derivative = 6.0 * t * (1.0 - t) / (LEG_MS as f64 / 1000.0);
    (
        phase,
        std::array::from_fn(|i| from[i] + (to[i] - from[i]) * progress),
        std::array::from_fn(|i| (to[i] - from[i]) * derivative),
        direction(from, to),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schedule_is_finite_periodic_continuous_and_avoids_station_and_berths() {
        assert_eq!(actors(0), actors(PERIOD_MS));
        assert_eq!(actors(u64::MAX).len(), 9);
        for now in (0..PERIOD_MS).step_by(100) {
            let current = actors(now);
            let next = actors(now + 100);
            for (a, b) in current.iter().zip(next) {
                assert_eq!(a.id, b.id);
                let heading_dot = a
                    .heading
                    .iter()
                    .zip(b.heading)
                    .map(|(a, b)| a * b)
                    .sum::<f64>();
                assert!(
                    heading_dot > 0.9998,
                    "traffic heading jumped at schedule boundary"
                );
                assert!((a.heading.iter().map(|v| v * v).sum::<f64>() - 1.0).abs() < 1e-9);
                assert!(a
                    .position
                    .iter()
                    .chain(&a.velocity)
                    .chain(&a.heading)
                    .all(|v| v.is_finite()));
                assert!(
                    a.position[0] - a.ship_class.spec().length_m / 2.0
                        > STATION_POSITION[0] + 2000.0
                );
                assert!(a.position[2] - a.ship_class.spec().width_m / 2.0 > 4000.0);
                let moved = a
                    .position
                    .iter()
                    .zip(b.position)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                assert!(
                    moved < 15.0,
                    "traffic exceeded150m/s or jumped at phase boundary"
                );
            }
        }
    }
}
