//! Fresh-world external Pioneer berths. Shared trajectory sampling is presentation,
//! not permission to complete a phase: only the server advances operations.
use crate::{hangar_layout, ShipClass, STATION_POSITION};
use serde::{Deserialize, Serialize};

pub const DOCKING_VERSION: u32 = 1;
pub const DOCKING_BERTH_COUNT: u16 = 8;
pub const DOCKING_HULL_RADIUS_M: f64 = 44.0;
pub const DOCKING_GATE_DISTANCE_M: f64 = 250.0;
/// Inward extent of the apron edge rail; shared by vessel and walking clearance.
pub const DOCKING_APRON_RAIL_HALF_WIDTH_M: f64 = 0.2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockingPhase {
    Free,
    Approaching,
    Aligning,
    Berthing,
    Clamping,
    Secured,
    Releasing,
    Departing,
    Braking,
    Held,
}
impl DockingPhase {
    pub fn busy(self) -> bool {
        !matches!(self, Self::Free | Self::Secured)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Free flight",
            Self::Approaching => "Approaching assigned berth",
            Self::Aligning => "Aligning with berth",
            Self::Berthing => "Moving into berth",
            Self::Clamping => "Securing docking clamps",
            Self::Secured => "Docking clamps secured",
            Self::Releasing => "Releasing docking clamps",
            Self::Departing => "Backing clear of berth",
            Self::Braking => "Stopping docking movement",
            Self::Held => "Docking held · resume required",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockingSegment {
    pub source_m: [f64; 3],
    pub target_m: [f64; 3],
    pub initial_velocity_mps: [f64; 3],
    pub source_orientation: [f64; 4],
    pub target_orientation: [f64; 4],
    pub started_at_ms: u64,
    pub duration_ms: u64,
}
impl DockingSegment {
    pub fn ends_at_ms(&self) -> u64 {
        self.started_at_ms.saturating_add(self.duration_ms)
    }
    pub fn progress(&self, now: u64) -> f64 {
        (now.saturating_sub(self.started_at_ms) as f64 / self.duration_ms.max(1) as f64)
            .clamp(0.0, 1.0)
    }
    /// Cubic Hermite translation preserves the accepted initial velocity and
    /// reaches the exact endpoint at zero velocity. Attitude has zero endpoint rate.
    pub fn sample(&self, now: u64) -> ([f64; 3], [f64; 3], [f64; 4]) {
        let t = self.progress(now);
        let seconds = self.duration_ms.max(1) as f64 / 1000.0;
        let smooth = t * t * (3.0 - 2.0 * t);
        let tangent = t * (1.0 - t) * (1.0 - t);
        let position = std::array::from_fn(|i| {
            self.source_m[i]
                + (self.target_m[i] - self.source_m[i]) * smooth
                + self.initial_velocity_mps[i] * seconds * tangent
        });
        let velocity = std::array::from_fn(|i| {
            (self.target_m[i] - self.source_m[i]) * 6.0 * t * (1.0 - t) / seconds
                + self.initial_velocity_mps[i] * (1.0 - 4.0 * t + 3.0 * t * t)
        });
        let dot: f64 = self
            .source_orientation
            .iter()
            .zip(self.target_orientation)
            .map(|(a, b)| a * b)
            .sum();
        let target = self
            .target_orientation
            .map(|v| if dot < 0.0 { -v } else { v });
        let mut orientation = std::array::from_fn::<_, 4, _>(|i| {
            self.source_orientation[i] * (1.0 - smooth) + target[i] * smooth
        });
        let length = orientation.iter().map(|v| v * v).sum::<f64>().sqrt();
        orientation
            .iter_mut()
            .for_each(|v| *v /= length.max(1.0e-12));
        (position, velocity, orientation)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DockingState {
    pub version: u32,
    pub host_vessel_id: String,
    pub berth_id: u16,
    /// Monotonically increasing per vessel; cancel/resume must name this operation.
    pub operation_id: u64,
    pub phase: DockingPhase,
    pub segment: Option<DockingSegment>,
    pub clamps: f64,
    pub updated_at_ms: u64,
    pub hold_reason: Option<String>,
    pub departing: bool,
}
impl DockingState {
    pub fn secured(&self) -> bool {
        self.phase == DockingPhase::Secured && self.clamps == 1.0
    }
}

pub fn docking_berth_position(id: u16) -> [f64; 3] {
    [
        STATION_POSITION[0] + 200.0,
        hangar_layout(ShipClass::Pioneer).ship_center[1] + 4.0,
        (f64::from(id) - f64::from(DOCKING_BERTH_COUNT - 1) / 2.0) * 600.0,
    ]
}
pub fn docking_gate_position(id: u16) -> [f64; 3] {
    let mut p = docking_berth_position(id);
    p[0] += DOCKING_GATE_DISTANCE_M;
    p
}
pub fn docking_berth_orientation() -> [f64; 4] {
    [0.0, 1.0, 0.0, 0.0]
}

/// Exact solid station geometry for the open docking apron. Coordinates are
/// sector metres; the private registered walking frame is rigidly mapped here.
/// There is no enclosed hangar or implied private fleet-space entrance.
pub fn docking_platform_boxes(id: u16) -> Vec<([f64; 3], [f64; 3])> {
    let origin = docking_berth_position(id);
    let local = hangar_layout(ShipClass::Pioneer);
    let mut boxes = vec![(
        [-local.half_length, -0.6, -local.half_width],
        [local.half_length, 0., local.half_width],
    )];
    for obstacle in crate::station_obstacles(ShipClass::Pioneer) {
        boxes.push((obstacle.min, obstacle.max));
    }
    // Knee-high edge barriers share vessel-route collision and rendering bounds.
    // Avatar clearance still uses the registered traversal extent (separate acceptance).
    for x in [-local.half_length, local.half_length] {
        boxes.push((
            [x - DOCKING_APRON_RAIL_HALF_WIDTH_M, 0., -local.half_width],
            [x + DOCKING_APRON_RAIL_HALF_WIDTH_M, 1., local.half_width],
        ));
    }
    for z in [-local.half_width, local.half_width] {
        boxes.push((
            [-local.half_length, 0., z - DOCKING_APRON_RAIL_HALF_WIDTH_M],
            [local.half_length, 1., z + DOCKING_APRON_RAIL_HALF_WIDTH_M],
        ));
    }
    let mut world = boxes
        .into_iter()
        .map(|(min, max)| {
            (
                [
                    origin[0] - max[0],
                    origin[1] + min[1] - local.ship_center[1],
                    origin[2] - max[2],
                ],
                [
                    origin[0] - min[0],
                    origin[1] + max[1] - local.ship_center[1],
                    origin[2] - min[2],
                ],
            )
        })
        .collect::<Vec<_>>();
    world.push((
        [STATION_POSITION[0], 3.4, origin[2] - 3.],
        [origin[0] - local.half_length, 4., origin[2] + 3.],
    ));
    world
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trajectory_preserves_endpoints_velocity_and_finite_attitude() {
        let segment = DockingSegment {
            source_m: [1., 2., 3.],
            target_m: [81., 2., 3.],
            initial_velocity_mps: [4., 0., 0.],
            source_orientation: [0., 0., 0., 1.],
            target_orientation: docking_berth_orientation(),
            started_at_ms: 1000,
            duration_ms: 6000,
        };
        assert_eq!(segment.sample(1000).0, segment.source_m);
        assert_eq!(segment.sample(1000).1, segment.initial_velocity_mps);
        assert_eq!(segment.sample(7000).0, segment.target_m);
        assert_eq!(segment.sample(7000).1, [0.; 3]);
        for now in 1000..=7000 {
            let (p, v, q) = segment.sample(now);
            assert!(p.iter().chain(&v).chain(&q).all(|v| v.is_finite()));
            assert!((q.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-12);
        }
    }
}
