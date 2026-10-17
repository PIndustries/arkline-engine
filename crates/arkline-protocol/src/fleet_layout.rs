//! Immutable, metre-scaled fleet layouts shared by authority and presentation.
//! Circular slots belong to the full catalog, not the current inventory subset.
use crate::{
    fleet_berth, fleet_hangar_layout, fleet_quarters_layout, FleetLayoutRevision, HangarLayout,
    ShipClass, ALL_SHIP_CLASSES, STATION_AVATAR_RADIUS_M,
};

const CONCOURSE_RADIUS: f64 = 680.0;
const PLATFORM_MARGIN: f64 = 12.0;
const SLOT_GAP_RADIANS: f64 = 4.0 * std::f64::consts::PI / 180.0;
const HOVER_GAP: f64 = 4.0;
const BRIDGE_HALF_WIDTH: f64 = 4.0;
const WALL_CLEARANCE: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FleetBerthPose {
    pub position: [f64; 3],
    /// Radians: +X bow turns toward +Z. Render with a negative Y-axis rotation.
    pub yaw: f64,
    pub boarding_point: [f64; 3],
    pub platform_radius_m: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FleetLayoutGeometry {
    pub center: [f64; 3],
    pub concourse_radius_m: f64,
    pub outer_radius_m: f64,
    pub ceiling_y: f64,
    pub bridge_half_width_m: f64,
    pub hover_gap_m: f64,
}

fn platform_radius(class: ShipClass) -> f64 {
    let spec = class.spec();
    spec.length_m.hypot(spec.width_m) / 2.0 + PLATFORM_MARGIN
}

fn pioneer_anchor() -> f64 {
    CONCOURSE_RADIUS + platform_radius(ShipClass::Pioneer)
        - ShipClass::Pioneer.spec().width_m / 2.0
        - 6.0
}

fn circular_poses() -> &'static [FleetBerthPose; 7] {
    static POSES: std::sync::OnceLock<[FleetBerthPose; 7]> = std::sync::OnceLock::new();
    POSES.get_or_init(|| {
        // Every platform's bounding disk lies in its own angular wedge. This is conservative
        // for the actual rotated hull rectangle and guarantees space around every class.
        let pioneer_radius = platform_radius(ShipClass::Pioneer);
        let mut angle = std::f64::consts::FRAC_PI_2
            - (pioneer_radius / (CONCOURSE_RADIUS + pioneer_radius)).asin();
        std::array::from_fn(|index| {
            let class = ALL_SHIP_CLASSES[index];
            let spec = class.spec();
            let platform_radius_m = platform_radius(class);
            let radius = CONCOURSE_RADIUS + platform_radius_m;
            let half_wedge = (platform_radius_m / radius).asin();
            let theta = angle + half_wedge;
            angle += 2.0 * half_wedge + SLOT_GAP_RADIANS;
            let (sin, cos) = theta.sin_cos();
            let board_radius = radius - spec.width_m / 2.0 - 6.0;
            let mut pose = FleetBerthPose {
                position: [
                    radius * cos,
                    spec.height_m / 2.0 + HOVER_GAP,
                    radius * sin - pioneer_anchor(),
                ],
                yaw: (theta - std::f64::consts::FRAC_PI_2).rem_euclid(std::f64::consts::TAU),
                boarding_point: [
                    board_radius * cos,
                    0.0,
                    board_radius * sin - pioneer_anchor(),
                ],
                platform_radius_m,
            };
            // Preserve the familiar Pioneer boarding origin exactly, without round-off drift.
            if class == ShipClass::Pioneer {
                pose.position = [
                    0.0,
                    spec.height_m / 2.0 + HOVER_GAP,
                    spec.width_m / 2.0 + 6.0,
                ];
                pose.boarding_point = [0.0, 0.0, 0.0];
                pose.yaw = 0.0;
            }
            pose
        })
    })
}

pub fn fleet_berth_pose(revision: FleetLayoutRevision, class: ShipClass) -> FleetBerthPose {
    match revision {
        FleetLayoutRevision::LinearV1 => {
            let (position, boarding_point) = fleet_berth(class);
            FleetBerthPose {
                position,
                yaw: 0.0,
                boarding_point,
                platform_radius_m: platform_radius(class),
            }
        }
        FleetLayoutRevision::CircularV1 => circular_poses()[ALL_SHIP_CLASSES
            .iter()
            .position(|item| *item == class)
            .unwrap()],
    }
}

pub fn fleet_layout_geometry(revision: FleetLayoutRevision) -> FleetLayoutGeometry {
    match revision {
        FleetLayoutRevision::LinearV1 => {
            let layout = fleet_hangar_layout();
            FleetLayoutGeometry {
                center: [0.0; 3],
                concourse_radius_m: 0.0,
                outer_radius_m: layout.half_length.hypot(layout.half_width),
                ceiling_y: layout.height,
                bridge_half_width_m: 0.0,
                hover_gap_m: 0.0,
            }
        }
        FleetLayoutRevision::CircularV1 => {
            let outer_radius_m = circular_poses()
                .iter()
                .map(|pose| {
                    (pose.position[0]).hypot(pose.position[2] + pioneer_anchor())
                        + pose.platform_radius_m
                })
                .fold(0.0, f64::max)
                + WALL_CLEARANCE;
            let ceiling_y = ALL_SHIP_CLASSES
                .iter()
                .map(|class| class.spec().height_m)
                .fold(0.0, f64::max)
                + HOVER_GAP
                + 60.0;
            FleetLayoutGeometry {
                center: [0.0, 0.0, -pioneer_anchor()],
                concourse_radius_m: CONCOURSE_RADIUS,
                outer_radius_m,
                ceiling_y,
                bridge_half_width_m: BRIDGE_HALF_WIDTH,
                hover_gap_m: HOVER_GAP,
            }
        }
    }
}

pub fn fleet_hangar_layout_for(revision: FleetLayoutRevision) -> HangarLayout {
    if revision == FleetLayoutRevision::LinearV1 {
        return fleet_hangar_layout();
    }
    let geometry = fleet_layout_geometry(revision);
    let pioneer = fleet_berth_pose(revision, ShipClass::Pioneer);
    HangarLayout {
        half_length: geometry.outer_radius_m + geometry.center[0].abs(),
        half_width: geometry.outer_radius_m + geometry.center[2].abs(),
        height: geometry.ceiling_y,
        ship_center: pioneer.position,
        boarding_point: pioneer.boarding_point,
    }
}

/// The complete catalog's hull footprints are restricted service zones, even when a vessel is
/// away. This keeps routes stable and never lets a returning hull appear on top of a walker.
pub fn fleet_hull_clear(revision: FleetLayoutRevision, point: [f64; 3]) -> bool {
    if !point.iter().all(|value| value.is_finite()) {
        return false;
    }
    if revision == FleetLayoutRevision::LinearV1 {
        return true;
    }
    ALL_SHIP_CLASSES.iter().all(|class| {
        let pose = fleet_berth_pose(revision, *class);
        let spec = class.spec();
        let (sin, cos) = pose.yaw.sin_cos();
        let dx = point[0] - pose.position[0];
        let dz = point[2] - pose.position[2];
        let local_x = dx * cos + dz * sin;
        let local_z = -dx * sin + dz * cos;
        local_x.abs() >= spec.length_m / 2.0 + STATION_AVATAR_RADIUS_M
            || local_z.abs() >= spec.width_m / 2.0 + STATION_AVATAR_RADIUS_M
    })
}

/// Feet must remain on the central concourse, a radial bridge, a berth platform, or the
/// unchanged quarters floor. Decorative lower machinery is not a walkable substitute.
pub fn fleet_walkable(revision: FleetLayoutRevision, point: [f64; 3]) -> bool {
    if !point.iter().all(|value| value.is_finite()) {
        return false;
    }
    if revision == FleetLayoutRevision::LinearV1 {
        let layout = fleet_hangar_layout();
        return point[0].abs() <= layout.half_length
            && point[2] >= -layout.half_width
            && point[2] <= 3.0;
    }
    if !fleet_hull_clear(revision, point) {
        return false;
    }
    let geometry = fleet_layout_geometry(revision);
    let x = point[0] - geometry.center[0];
    let z = point[2] - geometry.center[2];
    let inset = STATION_AVATAR_RADIUS_M;
    if x.hypot(z) <= CONCOURSE_RADIUS - inset {
        return true;
    }
    let quarters = fleet_quarters_layout();
    if point[0] >= quarters.min[0] + inset
        && point[0] <= quarters.max[0] - inset
        && point[2] >= quarters.min[2] + inset
        && point[2] <= quarters.max[2] - inset
    {
        return true;
    }
    circular_poses().iter().any(|pose| {
        let cx = pose.position[0] - geometry.center[0];
        let cz = pose.position[2] - geometry.center[2];
        let radius = cx.hypot(cz);
        let along = (x * cx + z * cz) / radius;
        let across = (-x * cz + z * cx) / radius;
        let board_radius = (pose.boarding_point[0] - geometry.center[0])
            .hypot(pose.boarding_point[2] - geometry.center[2]);
        (point[0] - pose.position[0]).hypot(point[2] - pose.position[2])
            <= pose.platform_radius_m - inset
            || (along >= CONCOURSE_RADIUS - 1.0
                && along <= board_radius + 3.0
                && across.abs() <= BRIDGE_HALF_WIDTH - inset)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn circular_platforms_clear_every_full_size_hull_and_each_other() {
        let revision = FleetLayoutRevision::CircularV1;
        let geometry = fleet_layout_geometry(revision);
        for (index, class) in ALL_SHIP_CLASSES.iter().enumerate() {
            let pose = fleet_berth_pose(revision, *class);
            let spec = class.spec();
            assert!((pose.position[1] - spec.height_m / 2.0 - 4.0).abs() < 1e-9);
            assert!(pose.platform_radius_m >= spec.length_m.hypot(spec.width_m) / 2.0 + 11.999);
            assert!(pose.position[1] + spec.height_m / 2.0 + 59.999 <= geometry.ceiling_y);
            assert!(fleet_walkable(revision, pose.boarding_point));
            assert!(!fleet_hull_clear(revision, pose.position));
            for other in ALL_SHIP_CLASSES.iter().skip(index + 1) {
                let b = fleet_berth_pose(revision, *other);
                let distance =
                    (pose.position[0] - b.position[0]).hypot(pose.position[2] - b.position[2]);
                assert!(distance > pose.platform_radius_m + b.platform_radius_m + 20.0);
            }
        }
        let pioneer = fleet_berth_pose(revision, ShipClass::Pioneer);
        assert_eq!(pioneer.position, [0.0, 13.0, 25.0]);
        assert_eq!(pioneer.boarding_point, [0.0; 3]);
        assert_eq!(pioneer.yaw, 0.0);
    }
    #[test]
    fn every_boarding_marker_connects_to_the_concourse_without_crossing_a_hull() {
        let revision = FleetLayoutRevision::CircularV1;
        let geometry = fleet_layout_geometry(revision);
        for class in ALL_SHIP_CLASSES {
            let pose = fleet_berth_pose(revision, class);
            for step in 0..=100 {
                let t = step as f64 / 100.0;
                let point = [
                    geometry.center[0] + (pose.boarding_point[0] - geometry.center[0]) * t,
                    0.0,
                    geometry.center[2] + (pose.boarding_point[2] - geometry.center[2]) * t,
                ];
                assert!(fleet_walkable(revision, point), "{class:?}: {point:?}");
            }
        }
        assert!(!fleet_walkable(revision, [f64::NAN, 0.0, 0.0]));
        assert!(!fleet_walkable(
            revision,
            [geometry.outer_radius_m * 2.0, 0.0, 0.0]
        ));
    }
    #[test]
    fn linear_poses_remain_exactly_legacy_without_hull_or_floor_changes() {
        for class in ALL_SHIP_CLASSES {
            let pose = fleet_berth_pose(FleetLayoutRevision::LinearV1, class);
            assert_eq!((pose.position, pose.boarding_point), fleet_berth(class));
            assert_eq!(pose.yaw, 0.0);
        }
        assert_eq!(
            fleet_hangar_layout_for(FleetLayoutRevision::LinearV1).height,
            fleet_hangar_layout().height
        );
    }
}
