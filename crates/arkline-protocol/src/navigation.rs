//! Navigation vocabulary and authoritative warp trajectory sampling.
//! Root re-exports preserve the existing public and wire contracts.

use crate::{DockingState, AU_METERS};
use serde::{Deserialize, Serialize};

/// Legacy fixed interval used only when migrating older flight records.
pub const WARP_DURATION_MS: u64 = 6_000;

/// Initial part of transit spent charging before visual travel begins.
pub const WARP_WARMUP_MS: u64 = 2_000;

pub const MAX_WARP_SPEED: f64 = AU_METERS;

pub const WARP_ACCELERATION: f64 = AU_METERS * 0.5;

pub const MAX_SHIP_SPEED: f64 = 600.0;

pub const SHIP_ACCELERATION: f64 = 150.0;

pub const PILOT_INPUT_LEASE_MS: u64 = 500;

pub const MAX_ANGULAR_SPEED: f64 = std::f64::consts::FRAC_PI_2;

pub fn identity_orientation() -> [f64; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NavigationTarget {
    Station,
    Belt,
    Pilot { pilot: String },
    Asteroid { id: u64 },
    Gate { id: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NavigationMode {
    Stop,
    Manual,
    Approach {
        target: NavigationTarget,
    },
    Orbit {
        target: NavigationTarget,
        radius: f64,
    },
    KeepRange {
        target: NavigationTarget,
        distance: f64,
    },
    FlyTo {
        position: [f64; 3],
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docking: Option<DockingState>,
    /// Actual thruster acceleration from the last normal-space integration step.
    #[serde(default)]
    pub acceleration: [f64; 3],
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub heading: [f64; 3],
    /// Unit quaternion [x,y,z,w], body axes +X forward, +Y up, +Z right.
    #[serde(default = "identity_orientation")]
    pub orientation: [f64; 4],
    /// Body-axis angular velocity (roll X, yaw Y, pitch Z), radians/second.
    #[serde(default)]
    pub angular_velocity: [f64; 3],
    pub mode: NavigationMode,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub warp: Option<WarpTrajectory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarpTrajectory {
    pub source: [f64; 3],
    pub target: [f64; 3],
    pub started_at_ms: u64,
    pub warmup_ms: u64,
    pub acceleration_ms: u64,
    pub cruise_ms: u64,
    pub arrives_at_ms: u64,
    #[serde(default)]
    pub arrival_ms: u64,
    #[serde(default)]
    pub arrival_distance_m: f64,
    #[serde(default)]
    pub launch_ms: u64,
    #[serde(default)]
    pub launch_distance_m: f64,
    #[serde(default)]
    pub initial_heading: Option<[f64; 3]>,
}

/// A symmetric acceleration/cruise/deceleration trajectory with finite warmup.
/// Durations round upward; sampling derives speed from the exact distance so
/// positions meet precisely at the persisted deadline without discontinuity.
pub fn plan_warp(source: [f64; 3], target: [f64; 3], started_at_ms: u64) -> WarpTrajectory {
    let distance = source
        .iter()
        .zip(target)
        .map(|(a, b)| (b - a).powi(2))
        .sum::<f64>()
        .sqrt();
    let arrival_ms = if distance > 300_000.0 { 6_000 } else { 0 };
    let arrival_distance_m = if arrival_ms > 0 { 30_000.0 } else { 0.0 };
    let launch_ms = arrival_ms;
    let launch_distance_m = arrival_distance_m;
    let main_distance = distance - arrival_distance_m - launch_distance_m;
    let acceleration_seconds = (main_distance / WARP_ACCELERATION)
        .sqrt()
        .min(MAX_WARP_SPEED / WARP_ACCELERATION);
    let acceleration_ms = (acceleration_seconds * 1000.0).ceil().max(1.0) as u64;
    let cruise_ms =
        ((main_distance / MAX_WARP_SPEED - acceleration_seconds).max(0.0) * 1000.0).ceil() as u64;
    let arrives_at_ms = started_at_ms
        .saturating_add(WARP_WARMUP_MS)
        .saturating_add(2 * acceleration_ms)
        .saturating_add(cruise_ms)
        .saturating_add(arrival_ms)
        .saturating_add(launch_ms);
    WarpTrajectory {
        source,
        target,
        started_at_ms,
        warmup_ms: WARP_WARMUP_MS,
        acceleration_ms,
        cruise_ms,
        arrives_at_ms,
        arrival_ms,
        arrival_distance_m,
        launch_ms,
        launch_distance_m,
        initial_heading: None,
    }
}

/// Sample authoritative meters and meters/second without f32 precision loss.
pub fn sample_warp(path: &WarpTrajectory, now_ms: u64) -> ([f64; 3], [f64; 3]) {
    if now_ms >= path.arrives_at_ms {
        return (path.target, [0.0; 3]);
    }
    let elapsed = now_ms
        .saturating_sub(path.started_at_ms)
        .saturating_sub(path.warmup_ms) as f64
        / 1000.0;
    let acceleration = path.acceleration_ms.max(1) as f64 / 1000.0;
    let cruise = path.cruise_ms as f64 / 1000.0;
    let total = 2.0 * acceleration + cruise;
    if path.arrival_ms > 0 && path.arrival_distance_m > 0.0 {
        let delta: [f64; 3] = std::array::from_fn(|i| path.target[i] - path.source[i]);
        let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
        if distance <= 0.0 {
            return (path.target, [0.0; 3]);
        }
        let direction = delta.map(|v| v / distance);
        let terminal = path.arrival_distance_m.min(distance);
        let launch = path.launch_distance_m.min((distance - terminal).max(0.0));
        let launch_duration = path.launch_ms as f64 / 1000.0;
        let launch_speed = if launch_duration > 0.0 {
            2.0 * launch / launch_duration
        } else {
            0.0
        };
        if elapsed < launch_duration {
            let u = (elapsed / launch_duration).clamp(0.0, 1.0);
            let traveled = launch * (2.0 * u.powi(3) - u.powi(4));
            let speed = launch_speed * (3.0 * u * u - 2.0 * u.powi(3));
            return (
                std::array::from_fn(|i| path.source[i] + direction[i] * traveled),
                direction.map(|v| v * speed),
            );
        }
        let elapsed = elapsed - launch_duration;
        let duration = path.arrival_ms as f64 / 1000.0;
        let entry_speed = 2.0 * terminal / duration;
        if elapsed >= total {
            let u = ((elapsed - total) / duration).clamp(0.0, 1.0);
            let traveled = terminal * (2.0 * u - 2.0 * u.powi(3) + u.powi(4));
            let speed = entry_speed * (1.0 - 3.0 * u * u + 2.0 * u.powi(3));
            // Target-relative computation preserves sub-meter local precision.
            return (
                std::array::from_fn(|i| path.target[i] - direction[i] * (terminal - traveled)),
                direction.map(|v| v * speed),
            );
        }
        let main_distance = distance - terminal - launch;
        let peak = (main_distance - 0.5 * (entry_speed + launch_speed) * acceleration)
            / (acceleration + cruise);
        let (traveled, speed) = if elapsed < acceleration {
            (
                launch_speed * elapsed
                    + 0.5 * (peak - launch_speed) * elapsed * elapsed / acceleration,
                launch_speed + (peak - launch_speed) * elapsed / acceleration,
            )
        } else if elapsed < acceleration + cruise {
            (
                0.5 * (peak + launch_speed) * acceleration + peak * (elapsed - acceleration),
                peak,
            )
        } else {
            let remaining = total - elapsed;
            let decel = (peak - entry_speed) / acceleration;
            (
                main_distance - entry_speed * remaining - 0.5 * decel * remaining * remaining,
                entry_speed + decel * remaining,
            )
        };
        return (
            std::array::from_fn(|i| path.source[i] + direction[i] * (launch + traveled)),
            direction.map(|v| v * speed),
        );
    }
    let (fraction, rate) = if elapsed < acceleration {
        (
            0.5 * elapsed * elapsed / acceleration / (acceleration + cruise),
            elapsed / acceleration / (acceleration + cruise),
        )
    } else if elapsed < acceleration + cruise {
        (
            (0.5 * acceleration + elapsed - acceleration) / (acceleration + cruise),
            1.0 / (acceleration + cruise),
        )
    } else {
        let remaining = (total - elapsed).max(0.0);
        (
            1.0 - 0.5 * remaining * remaining / acceleration / (acceleration + cruise),
            remaining / acceleration / (acceleration + cruise),
        )
    };
    let delta: [f64; 3] = std::array::from_fn(|i| path.target[i] - path.source[i]);
    (
        std::array::from_fn(|i| path.source[i] + delta[i] * fraction.clamp(0.0, 1.0)),
        delta.map(|d| d * rate),
    )
}

/// Fixed-duration alignment uses a continuous spherical turn, including 180 degrees.
pub fn sample_warp_heading(path: &WarpTrajectory, now_ms: u64) -> [f64; 3] {
    fn normalize(v: [f64; 3]) -> [f64; 3] {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if n < 1e-12 {
            [1.0, 0.0, 0.0]
        } else {
            v.map(|x| x / n)
        }
    }
    let target = normalize(std::array::from_fn(|i| path.target[i] - path.source[i]));
    let Some(initial) = path.initial_heading else {
        return target;
    };
    let initial = normalize(initial);
    let t = (now_ms.saturating_sub(path.started_at_ms) as f64 / path.warmup_ms.max(1) as f64)
        .clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let dot = initial
        .iter()
        .zip(target)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .clamp(-1.0, 1.0);
    if dot > 0.9995 {
        return normalize(std::array::from_fn(|i| {
            initial[i] * (1.0 - t) + target[i] * t
        }));
    }
    if dot < -0.9995 {
        let basis = if initial[1].abs() < 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let perpendicular = normalize([
            initial[1] * basis[2] - initial[2] * basis[1],
            initial[2] * basis[0] - initial[0] * basis[2],
            initial[0] * basis[1] - initial[1] * basis[0],
        ]);
        return normalize(std::array::from_fn(|i| {
            initial[i] * (std::f64::consts::PI * t).cos()
                + perpendicular[i] * (std::f64::consts::PI * t).sin()
        }));
    }
    let angle = dot.acos();
    normalize(std::array::from_fn(|i| {
        (initial[i] * (angle * (1.0 - t)).sin() + target[i] * (angle * t).sin()) / angle.sin()
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Station,
    Belt,
    Gate { id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Location {
    Docked,
    Jumping {
        gate_id: u64,
        arrives_at_ms: u64,
    },
    Station,
    Belt,
    InTransit {
        destination: Destination,
        arrives_at_ms: u64,
    },
}
