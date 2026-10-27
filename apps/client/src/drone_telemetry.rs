//! Read-only drone telemetry. Receipt freshness is independent of pose age;
//! mining progress uses only the last accepted authority timestamp.
use arkline_protocol::{DroneMode, DroneView, DRONE_HIT_POINTS};

const FRESHNESS_MS: u64 = 1500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Availability {
    Live,
    Paused,
    Stale,
    Disconnected,
    Unavailable,
}

impl Availability {
    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "LIVE",
            Self::Paused => "PAUSED",
            Self::Stale => "STALE / LAST CONFIRMED",
            Self::Disconnected => "DISCONNECTED / LAST CONFIRMED",
            Self::Unavailable => "UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TelemetryTime {
    pub server_ms: u64,
    pub availability: Availability,
}

impl Default for TelemetryTime {
    fn default() -> Self {
        Self {
            server_ms: 0,
            availability: Availability::Unavailable,
        }
    }
}

impl TelemetryTime {
    /// `receipt_age_ms` must come from an accepted response's observation time,
    /// never a render frame or DroneView::updated_at_ms. `presentation_ms` is
    /// the client's authority-clock estimate, not an unrelated UI wall clock.
    /// Neither age advances `server_ms`: stale, paused and disconnected views
    /// retain the last confirmed cycle sample without inventing completion.
    /// An intentional embedded pause can stop updates; with a confirmed sample
    /// it remains PAUSED regardless of age, unless the session disconnects.
    pub fn from_observation(
        server_ms: Option<u64>,
        receipt_age_ms: Option<u64>,
        presentation_ms: u64,
        connected: bool,
        paused: bool,
    ) -> Self {
        let availability = if !connected {
            Availability::Disconnected
        } else {
            match (server_ms, receipt_age_ms) {
                (Some(_), Some(_)) if paused => Availability::Paused,
                (Some(server_ms), Some(receipt_age_ms))
                    if receipt_age_ms > FRESHNESS_MS
                        || presentation_ms.saturating_sub(server_ms) > FRESHNESS_MS =>
                {
                    Availability::Stale
                }
                (Some(_), Some(_)) => Availability::Live,
                _ => Availability::Unavailable,
            }
        };
        Self {
            server_ms: server_ms.unwrap_or_default(),
            availability,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct CycleReadout {
    pub fraction: f32,
    pub label: String,
}

/// Preserve the received value independently of whether it is safe to draw a
/// bounded gauge. Invalid telemetry must not be clamped into a healthy/full bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct GaugeReadout {
    pub value: f64,
    pub maximum: f64,
    pub fraction: Option<f32>,
}

impl GaugeReadout {
    pub fn new(value: f64, maximum: f64) -> Self {
        let fraction = (value.is_finite()
            && maximum.is_finite()
            && maximum > 0.
            && value >= 0.
            && value <= maximum)
            .then(|| (value / maximum) as f32);
        Self {
            value,
            maximum,
            fraction,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct DroneReadout {
    pub task: String,
    pub hit_points: u32,
    pub health: GaugeReadout,
    pub signal: GaugeReadout,
    pub speed: Option<f64>,
    /// Yaw from +X towards +Z; pitch towards +Y. Heading supplies no roll.
    /// At vertical pitch, azimuth is undefined: the numeric yaw is only atan2's
    /// convention, so a yaw dial must mark it unavailable rather than as forward.
    pub heading_yaw_pitch_deg: Option<[f64; 2]>,
    /// The single authority-reported mining cycle, not an independent timer.
    pub cycle: Option<CycleReadout>,
}

impl DroneReadout {
    pub fn new(drone: &DroneView, time: TelemetryTime) -> Self {
        let cycle = if matches!(drone.mode, DroneMode::Mining { .. })
            && time.availability != Availability::Unavailable
        {
            drone.mining_cycle.as_ref().and_then(|cycle| {
                if cycle.completes_at_ms <= cycle.started_at_ms
                    || time.server_ms < cycle.started_at_ms
                {
                    return None;
                }
                let duration = cycle.completes_at_ms - cycle.started_at_ms;
                let elapsed = time.server_ms.saturating_sub(cycle.started_at_ms);
                let finished = time.server_ms >= cycle.completes_at_ms;
                let fraction = if finished {
                    1.
                } else {
                    // Keep a rounded float from declaring an unfinished long
                    // interval complete; integer authority timestamps decide.
                    ((elapsed as f64 / duration as f64) as f32)
                        .clamp(0., f32::from_bits(1.0_f32.to_bits() - 1))
                };
                Some(CycleReadout {
                    fraction,
                    label: if finished {
                        "100% / AWAITING AUTHORITY".into()
                    } else {
                        // Floor rather than round: never label an unfinished
                        // confirmed interval as 100 percent complete.
                        let percent = u128::from(elapsed) * 100 / u128::from(duration);
                        format!("{percent}% / MINING CYCLE")
                    },
                })
            })
        } else {
            None
        };
        let task = match &drone.mode {
            DroneMode::Mining { .. } => match &cycle {
                Some(cycle) if cycle.fraction >= 1. => "MINING / AWAITING AUTHORITY",
                Some(_) => "MINING",
                None => "MINING / WAITING",
            },
            DroneMode::Returning => "RETURNING",
            DroneMode::Hold => "HOLD",
            DroneMode::Orbit { .. } => "ORBIT",
            DroneMode::FlyTo { .. } => "FLYING",
            DroneMode::Attacking { .. } => "ATTACK TASK",
        }
        .into();
        let speed = drone
            .velocity
            .iter()
            .all(|value| value.is_finite())
            .then(|| {
                drone.velocity[0]
                    .hypot(drone.velocity[1])
                    .hypot(drone.velocity[2])
            });
        Self {
            task,
            hit_points: drone.hit_points,
            health: GaugeReadout::new(f64::from(drone.hit_points), f64::from(DRONE_HIT_POINTS)),
            signal: GaugeReadout::new(drone.signal_strength, 1.),
            speed: speed.filter(|speed| speed.is_finite()),
            heading_yaw_pitch_deg: heading_angles(drone.heading),
            cycle,
        }
    }
}

fn heading_angles(heading: [f64; 3]) -> Option<[f64; 2]> {
    if !heading.iter().all(|value| value.is_finite()) {
        return None;
    }
    let scale = heading
        .iter()
        .fold(0.0_f64, |scale, value| scale.max(value.abs()));
    if scale == 0. {
        return None;
    }
    let [x, y, z] = heading.map(|value| value / scale);
    Some([z.atan2(x).to_degrees(), y.atan2(x.hypot(z)).to_degrees()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::{DroneClass, DroneMiningCycle};

    fn drone() -> DroneView {
        DroneView {
            orientation_xyzw: None,
            id: 42,
            class: DroneClass::Mining,
            hit_points: 73,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [0.; 3],
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            mode: DroneMode::Hold,
            updated_at_ms: 1,
        }
    }

    fn live(server_ms: u64) -> TelemetryTime {
        TelemetryTime::from_observation(Some(server_ms), Some(0), server_ms, true, false)
    }

    #[test]
    fn default_context_has_no_confirmed_cycle_time() {
        let time = TelemetryTime::default();
        assert_eq!(time.server_ms, 0);
        assert_eq!(time.availability, Availability::Unavailable);
        assert_eq!(time.availability.label(), "UNAVAILABLE");
        assert!(DroneReadout::new(&mining(), time).cycle.is_none());
    }

    fn mining() -> DroneView {
        DroneView {
            orientation_xyzw: None,
            mode: DroneMode::Mining { target_id: 15 },
            mining_cycle: Some(DroneMiningCycle {
                started_at_ms: 1000,
                completes_at_ms: 9000,
            }),
            ..drone()
        }
    }

    #[test]
    fn stationary_hold_is_live_on_fresh_response_not_pose_timestamp() {
        let drone = drone();
        let time = live(900_000);
        assert_eq!(time.availability, Availability::Live);
        assert_eq!(drone.updated_at_ms, 1);
        let readout = DroneReadout::new(&drone, time);
        assert_eq!(readout.task, "HOLD");
        assert_eq!(readout.hit_points, 73);
        assert_eq!(readout.speed, Some(0.));
        assert!(readout.cycle.is_none());
    }

    #[test]
    fn freshness_requires_real_receipt_and_authority_clock_freshness() {
        let sample = |age, presentation| {
            TelemetryTime::from_observation(Some(1000), age, presentation, true, false).availability
        };
        assert_eq!(sample(Some(1500), 2500), Availability::Live);
        assert_eq!(sample(Some(1501), 1000), Availability::Stale);
        assert_eq!(sample(Some(0), 2501), Availability::Stale);
        assert_eq!(sample(None, 1000), Availability::Unavailable);
        assert_eq!(
            TelemetryTime::from_observation(None, Some(0), 1000, true, false).availability,
            Availability::Unavailable
        );
    }

    #[test]
    fn pause_stale_disconnect_preserve_confirmed_cycle_sample() {
        let drone = mining();
        let expected = DroneReadout::new(&drone, live(5000));
        for (age, presentation, connected, paused, availability) in [
            (0, 5000, true, true, Availability::Paused),
            (1501, 5000, true, false, Availability::Stale),
            (0, 50_000, true, false, Availability::Stale),
            (1501, 50_000, true, true, Availability::Paused),
            (50_000, 50_000, false, false, Availability::Disconnected),
        ] {
            let time = TelemetryTime::from_observation(
                Some(5000),
                Some(age),
                presentation,
                connected,
                paused,
            );
            assert_eq!(time.availability, availability);
            assert_eq!(time.server_ms, 5000);
            assert_eq!(DroneReadout::new(&drone, time), expected);
        }
    }

    #[test]
    fn intentional_long_pause_is_explicit_but_cannot_hide_disconnect_or_missing_sample() {
        let paused =
            TelemetryTime::from_observation(Some(5000), Some(600_000), 605_000, true, true);
        assert_eq!(paused.availability, Availability::Paused);
        assert_eq!(paused.server_ms, 5000);
        assert_eq!(
            DroneReadout::new(&mining(), paused),
            DroneReadout::new(&mining(), live(5000))
        );
        assert_eq!(
            TelemetryTime::from_observation(Some(5000), Some(600_000), 605_000, false, true)
                .availability,
            Availability::Disconnected
        );
        for (server_ms, receipt_age_ms) in [(None, Some(0)), (Some(5000), None)] {
            assert_eq!(
                TelemetryTime::from_observation(server_ms, receipt_age_ms, 605_000, true, true)
                    .availability,
                Availability::Unavailable
            );
        }
    }

    #[test]
    fn one_cycle_start_half_end_and_late_samples_are_clamped() {
        let drone = mining();
        for (now, expected) in [(1000, 0.), (5000, 0.5), (9000, 1.), (u64::MAX, 1.)] {
            let readout = DroneReadout::new(&drone, live(now));
            assert_eq!(readout.cycle.as_ref().unwrap().fraction, expected);
            if now >= 9000 {
                assert_eq!(readout.task, "MINING / AWAITING AUTHORITY");
                assert_eq!(readout.cycle.unwrap().label, "100% / AWAITING AUTHORITY");
            }
        }
        assert_eq!(
            DroneReadout::new(&drone, live(8999)).cycle.unwrap().label,
            "99% / MINING CYCLE"
        );
    }

    #[test]
    fn missing_invalid_and_future_cycles_are_waiting_not_extraction() {
        for cycle in [
            None,
            Some((1000, 1000)),
            Some((9000, 1000)),
            Some((5001, 9000)),
        ] {
            let mut drone = mining();
            drone.mining_cycle = cycle.map(|(started_at_ms, completes_at_ms)| DroneMiningCycle {
                started_at_ms,
                completes_at_ms,
            });
            let readout = DroneReadout::new(&drone, live(5000));
            assert_eq!(readout.task, "MINING / WAITING");
            assert!(readout.cycle.is_none());
        }
    }

    #[test]
    fn float_rounding_never_declares_long_interval_finished_early() {
        let mut drone = mining();
        drone.mining_cycle = Some(DroneMiningCycle {
            started_at_ms: 0,
            completes_at_ms: u64::MAX,
        });
        let readout = DroneReadout::new(&drone, live(u64::MAX - 1));
        assert_eq!(readout.task, "MINING");
        let cycle = readout.cycle.unwrap();
        assert!(cycle.fraction < 1.);
        assert_eq!(cycle.label, "99% / MINING CYCLE");
    }

    #[test]
    fn same_time_distinct_authority_states_do_not_stick_or_replay() {
        let mining = mining();
        let mut returning = mining.clone();
        returning.mode = DroneMode::Returning;
        let time = live(5000);
        let first = DroneReadout::new(&mining, time);
        let second = DroneReadout::new(&returning, time);
        assert_eq!(second.task, "RETURNING");
        assert!(second.cycle.is_none());
        assert_eq!(DroneReadout::new(&mining, time), first);
    }

    #[test]
    fn cycle_readout_does_not_award_yield_mutate_state_or_begin_next_cycle() {
        let drone = mining();
        let original = drone.clone();
        let first = DroneReadout::new(&drone, live(9000));
        let later = DroneReadout::new(&drone, live(99_000));
        assert_eq!(first.cycle, later.cycle);
        assert_eq!(drone, original);
        assert!(later.task.contains("AWAITING AUTHORITY"));
    }

    #[test]
    fn heading_uses_yaw_pitch_without_invented_roll() {
        let mut drone = drone();
        for (heading, expected) in [
            ([1., 0., 0.], [0., 0.]),
            ([0., 0., 1.], [90., 0.]),
            ([-1., 0., 0.], [180., 0.]),
            ([0., 0., -1.], [-90., 0.]),
            ([0., 1., 0.], [0., 90.]),
            ([0., -1., 0.], [0., -90.]),
            ([1., -1., 0.], [0., -45.]),
            ([f64::MAX, 0., f64::MAX], [45., 0.]),
        ] {
            drone.heading = heading;
            let result = DroneReadout::new(&drone, live(5000))
                .heading_yaw_pitch_deg
                .unwrap();
            for axis in 0..2 {
                assert!((result[axis] - expected[axis]).abs() < 1e-9);
            }
        }
    }

    #[test]
    fn heading_yaw_wraps_at_signed_180_without_a_roll_value() {
        for yaw in [-180.0_f64, -179., 179., 180.] {
            let radians = yaw.to_radians();
            let angles = heading_angles([radians.cos(), 0., radians.sin()]).unwrap();
            assert!((-180. ..=180.).contains(&angles[0]));
            assert!((angles[0] - yaw).abs() < 1e-9);
            assert_eq!(angles[1], 0.);
        }
        assert_eq!(heading_angles([-1., 0., -0.]).unwrap(), [-180., 0.]);
    }

    #[test]
    fn gauges_preserve_actual_values_and_only_validate_bounded_fractions() {
        for (value, maximum, fraction) in [
            (0., 100., 0.),
            (-0., 100., 0.),
            (50., 100., 0.5),
            (100., 100., 1.),
        ] {
            let gauge = GaugeReadout::new(value, maximum);
            assert_eq!(gauge.value.to_bits(), value.to_bits());
            assert_eq!(gauge.maximum, maximum);
            assert_eq!(gauge.fraction, Some(fraction));
        }
        for (value, maximum) in [
            (-1., 100.),
            (101., 100.),
            (f64::NAN, 100.),
            (f64::INFINITY, 100.),
            (f64::NEG_INFINITY, 100.),
            (1., 0.),
            (1., -100.),
            (1., f64::NAN),
            (1., f64::INFINITY),
        ] {
            let gauge = GaugeReadout::new(value, maximum);
            assert_eq!(gauge.value.to_bits(), value.to_bits());
            assert_eq!(gauge.maximum.to_bits(), maximum.to_bits());
            assert_eq!(gauge.fraction, None);
        }
    }

    #[test]
    fn health_uses_protocol_max_and_signal_requires_zero_to_one() {
        let mut drone = drone();
        for (hit_points, expected) in [
            (0, Some(0.)),
            (DRONE_HIT_POINTS, Some(1.)),
            (DRONE_HIT_POINTS + 1, None),
            (u32::MAX, None),
        ] {
            drone.hit_points = hit_points;
            let readout = DroneReadout::new(&drone, live(5000));
            assert_eq!(readout.hit_points, hit_points);
            assert_eq!(readout.health.value, f64::from(hit_points));
            assert_eq!(readout.health.maximum, f64::from(DRONE_HIT_POINTS));
            assert_eq!(readout.health.fraction, expected);
        }
        for (signal, expected) in [
            (0., Some(0.)),
            (0.25, Some(0.25)),
            (1., Some(1.)),
            (-0.01, None),
            (1.01, None),
            (f64::NAN, None),
            (f64::INFINITY, None),
        ] {
            drone.signal_strength = signal;
            let readout = DroneReadout::new(&drone, live(5000));
            assert_eq!(readout.signal.value.to_bits(), signal.to_bits());
            assert_eq!(readout.signal.maximum, 1.);
            assert_eq!(readout.signal.fraction, expected);
        }
    }

    #[test]
    fn paused_stale_disconnected_keep_gauges_heading_and_cycle_from_same_confirmed_sample() {
        let mut drone = mining();
        drone.hit_points = 37;
        drone.signal_strength = 0.375;
        drone.heading = [1., -1., 1.];
        drone.velocity = [3., 4., 0.];
        let expected = DroneReadout::new(&drone, live(5000));
        for (connected, paused, availability) in [
            (true, true, Availability::Paused),
            (true, false, Availability::Stale),
            (false, false, Availability::Disconnected),
        ] {
            let time = TelemetryTime::from_observation(
                Some(5000),
                Some(600_000),
                605_000,
                connected,
                paused,
            );
            assert_eq!(time.availability, availability);
            assert_eq!(DroneReadout::new(&drone, time), expected);
        }
        assert_eq!(expected.health.fraction, Some(0.37));
        assert_eq!(expected.signal.fraction, Some(0.375));
        assert_eq!(expected.speed, Some(5.));
        assert_eq!(expected.cycle.unwrap().fraction, 0.5);
    }

    #[test]
    fn invalid_heading_and_velocity_are_unavailable_not_fabricated_zero() {
        let mut drone = drone();
        for heading in [[0.; 3], [f64::NAN, 0., 0.], [1., f64::INFINITY, 0.]] {
            drone.heading = heading;
            assert!(DroneReadout::new(&drone, live(5000))
                .heading_yaw_pitch_deg
                .is_none());
        }
        for velocity in [[f64::NAN, 0., 0.], [0., f64::INFINITY, 0.], [f64::MAX; 3]] {
            drone.velocity = velocity;
            assert_eq!(DroneReadout::new(&drone, live(5000)).speed, None);
        }
        drone.velocity = [3., 4., 12.];
        assert_eq!(DroneReadout::new(&drone, live(5000)).speed, Some(13.));
    }
}
