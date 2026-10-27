//! Source-bound Pioneer bay geometry and resolved route poses, not authority.
//!
//! All mechanism roots are already resolved in ship-body metres. Render each
//! exact source group with current_root * inverse(reference_root), ONCE, then
//! apply the full carrier pose. Do not apply a second inheritance transform.
//! Every pose within one segment must share the same progress/duration; the
//! checked linear paths remain valid under one common smoothstep time map.
use crate::drone_bay_definition::{OfficialBayDefinition, PIONEER_BAY_REG12};
use crate::drone_bay_geometry::{BayMotionSegment, BayPose};

pub const DRONE_BAY_REGISTRATION_REVISION: u32 = 12;
pub const DRONE_BAY_SOURCE_SHA256: &str =
    "7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd";
pub const DRONE_BAY_BLEND_SHA256: &str =
    "ee43c05cb1d4ebf907d609b5338b4d8a911b812d8fb05914c96ca48dbb06b1d9";
pub const DRONE_BAY_SLOT_COUNT: usize = 15;
pub const DRONE_BAY_MECHANISM_COUNT: usize = 22;
/// Authority must hold the carrier stationary for this bounded choreography.
/// This module neither enforces a flight lock nor invents velocity tolerances.
pub const DRONE_BAY_REQUIRES_STATIONARY_CARRIER: bool = true;
pub const DRONE_BAY_DRONE_HALF_EXTENTS_M: [f64; 3] = [2.5, 0.9, 3.0];
pub const DRONE_BAY_APERTURE_BOUNDS_M: [[f64; 3]; 2] = [[-36.0, -2.6, -7.5], [-35.25, 0.6, 0.5]];
pub const DRONE_BAY_AISLE_Z_M: f64 = -3.5;
pub const DRONE_BAY_EXIT_Y_M: f64 = -1.0;
pub const DRONE_BAY_STAGING_X_M: f64 = -30.5;
pub const DRONE_BAY_RELEASE_OFFSET_Y_M: f64 = -0.20;
pub const DRONE_BAY_DOOR_TRAVEL_M: f64 = 1.70;
pub const DRONE_BAY_STATIONS_X_M: [f64; 3] = [-30.5, -24.5, -18.5];
pub const DRONE_BAY_TIERS_Y_M: [f64; 5] = [-5.4, -3.2, -1.0, 1.2, 3.4];
pub const DRONE_BAY_EXTERNAL_HOLD: BayPose = rear_pose([-40.0, -1.0, -3.5]);
pub const DRONE_BAY_RELEASE_POSE: BayPose = rear_pose([-30.5, -1.0, -3.5]);
pub const DRONE_BAY_LOWERED_TRAY_POSE: BayPose = root_pose([-30.5, -1.2, -3.5]);

const fn root_pose(position_m: [f64; 3]) -> BayPose {
    BayPose {
        position_m,
        orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
    }
}
const fn rear_pose(position_m: [f64; 3]) -> BayPose {
    BayPose {
        position_m,
        orientation_xyzw: [0.0, 1.0, 0.0, 0.0],
    }
}

/// Stable semantic ordering. Values are local geometry identifiers, not a wire
/// protocol or authority lifecycle enum. Exact source names are in the receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BayMechanismId {
    Tray00,
    Tray01,
    Tray02,
    Tray03,
    Tray04,
    Tray05,
    Tray06,
    Tray07,
    Tray08,
    Tray09,
    Tray10,
    Tray11,
    Tray12,
    Tray13,
    Tray14,
    CarriageX,
    LiftXy,
    ForkStage0,
    ForkStage1,
    ForkStage2,
    DoorUpper,
    DoorLower,
}
pub const DRONE_BAY_MECHANISMS: [BayMechanismId; DRONE_BAY_MECHANISM_COUNT] = [
    BayMechanismId::Tray00,
    BayMechanismId::Tray01,
    BayMechanismId::Tray02,
    BayMechanismId::Tray03,
    BayMechanismId::Tray04,
    BayMechanismId::Tray05,
    BayMechanismId::Tray06,
    BayMechanismId::Tray07,
    BayMechanismId::Tray08,
    BayMechanismId::Tray09,
    BayMechanismId::Tray10,
    BayMechanismId::Tray11,
    BayMechanismId::Tray12,
    BayMechanismId::Tray13,
    BayMechanismId::Tray14,
    BayMechanismId::CarriageX,
    BayMechanismId::LiftXy,
    BayMechanismId::ForkStage0,
    BayMechanismId::ForkStage1,
    BayMechanismId::ForkStage2,
    BayMechanismId::DoorUpper,
    BayMechanismId::DoorLower,
];
impl BayMechanismId {
    pub const fn index(self) -> usize {
        self as usize
    }
    pub fn source_group(self) -> &'static str {
        PIONEER_BAY_REG12.mechanism(self).semantic_id
    }
    pub fn reference_pose(self) -> BayPose {
        PIONEER_BAY_REG12.mechanism(self).reference_pose
    }
}

/// Compatibility wrapper for the unchanged authored registration12.
pub fn home_pose(slot: u8) -> Option<BayPose> {
    PIONEER_BAY_REG12.slot(slot).map(|slot| slot.home_pose)
}

/// One rear-facing physical home per finite cradle; no entities are created.
pub fn home_poses() -> [BayPose; DRONE_BAY_SLOT_COUNT] {
    PIONEER_BAY_REG12.home_poses()
}
pub fn mechanism_ids() -> [&'static str; DRONE_BAY_MECHANISM_COUNT] {
    PIONEER_BAY_REG12.mechanism_ids()
}
pub fn reference_poses() -> [BayPose; DRONE_BAY_MECHANISM_COUNT] {
    PIONEER_BAY_REG12.reference_poses()
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BayEnvelope {
    pub min_m: [f64; 3],
    pub max_m: [f64; 3],
}
impl BayEnvelope {
    /// Conservative world AABB of all eight corners under a full rigid parent
    /// quaternion. Never rotate only the centre or use a heading-only fallback.
    pub fn transformed(self, parent: BayPose) -> Option<Self> {
        if !self
            .min_m
            .iter()
            .chain(self.max_m.iter())
            .all(|v| v.is_finite())
            || (0..3).any(|i| self.min_m[i] > self.max_m[i])
        {
            return None;
        }
        let mut result = Self {
            min_m: [f64::INFINITY; 3],
            max_m: [f64::NEG_INFINITY; 3],
        };
        for corner in 0..8 {
            let p = std::array::from_fn(|i| {
                if corner & (1 << i) == 0 {
                    self.min_m[i]
                } else {
                    self.max_m[i]
                }
            });
            let point = parent.compose(root_pose(p))?.position_m;
            for (i, v) in point.into_iter().enumerate() {
                result.min_m[i] = result.min_m[i].min(v);
                result.max_m[i] = result.max_m[i].max(v);
            }
        }
        Some(result)
    }
}
/// Conservative authored working hardware bounds, NOT a solid walkable room.
pub const DRONE_BAY_WORKING_ENVELOPE: BayEnvelope = BayEnvelope {
    min_m: [-36.0, -6.72, -7.71],
    max_m: [-15.30, 5.25, 7.15],
};
pub const DRONE_BAY_CARRIER_ENVELOPE: BayEnvelope = BayEnvelope {
    min_m: [-36.0, -9.0, -19.0],
    max_m: [36.0, 11.35, 19.0],
};
/// Endpoints must actually be committed by authority; a client clock/sample
/// reaching these indices does not authorize any lifecycle transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BayRouteMilestones {
    pub drone_hull_clear_keyframe: Option<usize>,
    pub incoming_alignment_keyframe: Option<usize>,
    pub capture_supported_keyframe: Option<usize>,
    pub drone_home_keyframe: Option<usize>,
    pub doors_secured_keyframe: usize,
    pub handling_idle_keyframe: usize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayDroneRouteBinding {
    /// Same authoritative body must use the route, never a replacement mesh.
    RouteDriven,
    /// Recovery drone must already align continuously at external hold before
    /// insertion. This nominal pose does not teleport an approaching drone.
    ExternalAlignment,
    /// Launch body is hull-clear; authority may detach it while retaining the
    /// handling reservation. Ignore nominal external-hold samples after detach.
    MayDetach,
}
/// Endpoint support geometry, not an authoritative Free/Stored lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayDroneSupport {
    Cradle,
    TransferTray,
    Unseated,
    External,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaySegmentContract {
    pub drone_binding: BayDroneRouteBinding,
    pub endpoint_support: [BayDroneSupport; 2],
    pub may_detach_after_endpoint: bool,
    pub handling_required_after_endpoint: bool,
    pub throat_swept: bool,
    pub door_open_fraction: [f64; 2],
    /// Conservative full-size body sweep, suitable for further parent/world
    /// transforms and peer/asteroid checks; not authority collision admission.
    pub drone_swept_envelope: BayEnvelope,
    pub machinery_working_envelope: BayEnvelope,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BayRouteState {
    pub drone_body_pose: BayPose,
    /// Index with BayMechanismId::index(); these are resolved body roots.
    pub mechanism_body_roots: [BayPose; DRONE_BAY_MECHANISM_COUNT],
}
impl BayRouteState {
    pub fn mechanism(self, id: BayMechanismId) -> BayPose {
        self.mechanism_body_roots[id.index()]
    }
    pub fn valid(self) -> bool {
        self.drone_body_pose.valid() && self.mechanism_body_roots.iter().all(|p| p.valid())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayRouteDirection {
    Launch,
    Recovery,
}
/// Geometric movement only. The server chooses nonzero durations, admission,
/// blocking, lifecycle transitions and when an endpoint is truly committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BayRouteSegmentKind {
    CarriageX,
    SupportY,
    ForkExtension,
    TrayAndForkLateral,
    Door,
    SupportReleaseOrSeat,
    DroneThroat,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BayRoute {
    definition: &'static OfficialBayDefinition,
    pub slot: u8,
    pub direction: BayRouteDirection,
    pub keyframes: Vec<BayRouteState>,
    /// Segment i joins keyframes[i] -> keyframes[i+1]. No shortcut/teleport.
    pub segment_kinds: Vec<BayRouteSegmentKind>,
}
impl BayRoute {
    pub fn milestones(&self) -> BayRouteMilestones {
        match self.direction {
            BayRouteDirection::Launch => BayRouteMilestones {
                drone_hull_clear_keyframe: Some(9),
                incoming_alignment_keyframe: None,
                capture_supported_keyframe: None,
                drone_home_keyframe: None,
                doors_secured_keyframe: 10,
                handling_idle_keyframe: 17,
            },
            BayRouteDirection::Recovery => BayRouteMilestones {
                drone_hull_clear_keyframe: None,
                incoming_alignment_keyframe: Some(8),
                capture_supported_keyframe: Some(10),
                drone_home_keyframe: Some(14),
                doors_secured_keyframe: 11,
                handling_idle_keyframe: 17,
            },
        }
    }
    pub fn segment_contract(&self, index: usize) -> Option<BaySegmentContract> {
        if index >= 17 {
            return None;
        }
        let a = self.keyframes.get(index)?;
        let b = self.keyframes.get(index.checked_add(1)?)?;
        let kind = *self.segment_kinds.get(index)?;
        let door = |s: &BayRouteState| {
            (s.mechanism(BayMechanismId::DoorUpper).position_m[1]
                - self
                    .definition
                    .mechanism(BayMechanismId::DoorUpper)
                    .reference_pose
                    .position_m[1])
                / self.definition.rig.door_travel_m
        };
        let binding = match self.direction {
            BayRouteDirection::Launch if index >= 9 => BayDroneRouteBinding::MayDetach,
            BayRouteDirection::Recovery if index < 8 => BayDroneRouteBinding::ExternalAlignment,
            _ => BayDroneRouteBinding::RouteDriven,
        };
        let support = |keyframe: usize| {
            let launch_index = match self.direction {
                BayRouteDirection::Launch => keyframe,
                BayRouteDirection::Recovery => 17 - keyframe,
            };
            match launch_index {
                0..=3 => BayDroneSupport::Cradle,
                4..=7 => BayDroneSupport::TransferTray,
                8 => BayDroneSupport::Unseated,
                _ => BayDroneSupport::External,
            }
        };
        Some(BaySegmentContract {
            drone_binding: binding,
            endpoint_support: [support(index), support(index + 1)],
            may_detach_after_endpoint: self.direction == BayRouteDirection::Launch && index >= 8,
            handling_required_after_endpoint: index + 1 < 17,
            throat_swept: kind == BayRouteSegmentKind::DroneThroat,
            door_open_fraction: [door(a), door(b)],
            drone_swept_envelope: BayEnvelope {
                min_m: std::array::from_fn(|i| {
                    a.drone_body_pose.position_m[i].min(b.drone_body_pose.position_m[i])
                        - self.definition.drone_half_extents_m[i]
                }),
                max_m: std::array::from_fn(|i| {
                    a.drone_body_pose.position_m[i].max(b.drone_body_pose.position_m[i])
                        + self.definition.drone_half_extents_m[i]
                }),
            },
            machinery_working_envelope: self.definition.working_envelope,
        })
    }
    /// Sample all roots and the same drone with one timing parameter. This does
    /// not advance authority, reserve/release a slot or finish a lifecycle phase.
    pub fn sample_segment(
        &self,
        index: usize,
        started_at_ms: u64,
        duration_ms: u64,
        now: u64,
    ) -> Option<BayRouteState> {
        let source = *self.keyframes.get(index)?;
        let target = *self.keyframes.get(index.checked_add(1)?)?;
        self.segment_kinds.get(index)?;
        let sample = |a, b| {
            BayMotionSegment {
                source: a,
                target: b,
                started_at_ms,
                duration_ms,
            }
            .sample(now)
            .map(|v| v.0)
        };
        let mut result = source;
        result.drone_body_pose = sample(source.drone_body_pose, target.drone_body_pose)?;
        for (i, pose) in result.mechanism_body_roots.iter_mut().enumerate() {
            *pose = sample(
                source.mechanism_body_roots[i],
                target.mechanism_body_roots[i],
            )?;
        }
        result.valid().then_some(result)
    }
}

#[derive(Clone, Copy)]
struct Controls {
    carriage_x: f64,
    support_y: f64,
    fork_fraction: f64,
    door_fraction: f64,
    tray: [f64; 3],
    drone: [f64; 3],
}
impl Controls {
    fn resolve(self, definition: &OfficialBayDefinition, slot: u8) -> BayRouteState {
        let mut roots = definition.reference_poses();
        roots[usize::from(slot)] = root_pose(self.tray);
        roots[BayMechanismId::CarriageX.index()].position_m[0] = self.carriage_x;
        for id in [
            BayMechanismId::LiftXy,
            BayMechanismId::ForkStage0,
            BayMechanismId::ForkStage1,
            BayMechanismId::ForkStage2,
        ] {
            let p = &mut roots[id.index()].position_m;
            p[0] = self.carriage_x;
            p[1] = self.support_y;
            if id.index() >= BayMechanismId::ForkStage0.index() {
                p[2] += definition.rig.fork_stage_travel_m
                    * (id.index() - BayMechanismId::ForkStage0.index()) as f64
                    * self.fork_fraction;
            }
        }
        roots[BayMechanismId::DoorUpper.index()].position_m[1] +=
            definition.rig.door_travel_m * self.door_fraction;
        roots[BayMechanismId::DoorLower.index()].position_m[1] -=
            definition.rig.door_travel_m * self.door_fraction;
        BayRouteState {
            drone_body_pose: rear_pose(self.drone),
            mechanism_body_roots: roots,
        }
    }
}

/// Complete launch geometry, including return of the empty tray and carriage.
/// The drone at the final external hold is outside the rear hull, not deleted.
/// Other drone identities/occupancy are not owned or fabricated by this module.
pub fn launch_route(slot: u8) -> Option<BayRoute> {
    PIONEER_BAY_REG12.launch_route(slot)
}
pub(crate) fn launch_route_for(
    definition: &'static OfficialBayDefinition,
    slot: u8,
) -> Option<BayRoute> {
    use BayRouteSegmentKind as K;
    let home = definition.slot(slot)?.home_pose.position_m;
    let idle = definition.rig.idle_support.position_m;
    let mut c = Controls {
        carriage_x: idle[0],
        support_y: idle[1],
        fork_fraction: 0.0,
        door_fraction: 0.0,
        tray: home,
        drone: home,
    };
    let mut route = BayRoute {
        definition,
        slot,
        direction: BayRouteDirection::Launch,
        keyframes: vec![c.resolve(definition, slot)],
        segment_kinds: Vec::new(),
    };
    let mut push = |kind, c: Controls| {
        route.segment_kinds.push(kind);
        route.keyframes.push(c.resolve(definition, slot));
    };
    c.carriage_x = home[0];
    push(K::CarriageX, c);
    c.support_y = home[1];
    push(K::SupportY, c);
    c.fork_fraction = 1.0;
    push(K::ForkExtension, c);
    c.tray[2] = definition.rig.aisle_z_m;
    c.drone = c.tray;
    c.fork_fraction = 0.0;
    push(K::TrayAndForkLateral, c);
    c.support_y = idle[1];
    c.tray[1] = idle[1];
    c.drone = c.tray;
    push(K::SupportY, c);
    c.carriage_x = idle[0];
    c.tray[0] = idle[0];
    c.drone = c.tray;
    push(K::CarriageX, c);
    c.door_fraction = 1.0;
    push(K::Door, c);
    // Keep the drone fixed while the actual tray/forks/lift clear the end lip.
    c.support_y = idle[1] + definition.rig.support_release_y_m;
    c.tray[1] = idle[1] + definition.rig.support_release_y_m;
    push(K::SupportReleaseOrSeat, c);
    c.drone = definition.external_hold.position_m;
    push(K::DroneThroat, c);
    c.door_fraction = 0.0;
    push(K::Door, c);
    // Return the empty tray by the same checked axes, not a snap to its slot.
    c.support_y = idle[1];
    c.tray[1] = idle[1];
    push(K::SupportY, c);
    c.carriage_x = home[0];
    c.tray[0] = home[0];
    push(K::CarriageX, c);
    c.support_y = home[1];
    c.tray[1] = home[1];
    push(K::SupportY, c);
    c.fork_fraction = 1.0;
    c.tray = home;
    push(K::TrayAndForkLateral, c);
    c.fork_fraction = 0.0;
    push(K::ForkExtension, c);
    c.support_y = idle[1];
    push(K::SupportY, c);
    c.carriage_x = idle[0];
    push(K::CarriageX, c);
    Some(route)
}
/// Reverse the complete checked launch path: acquire the empty tray before the
/// incoming drone enters, raise the support to seat, store visibly, return idle.
pub fn recovery_route(slot: u8) -> Option<BayRoute> {
    PIONEER_BAY_REG12.recovery_route(slot)
}
pub(crate) fn recovery_route_for(
    definition: &'static OfficialBayDefinition,
    slot: u8,
) -> Option<BayRoute> {
    let mut route = launch_route_for(definition, slot)?;
    route.direction = BayRouteDirection::Recovery;
    route.keyframes.reverse();
    route.segment_kinds.reverse();
    Some(route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::HashSet;
    type Bounds = [[f64; 3]; 2];
    struct Part {
        name: String,
        group: usize,
        bounds: Bounds,
    }
    fn fixture() -> Value {
        serde_json::from_str(include_str!("drone_bay_layout_fixture.json")).unwrap()
    }
    fn point(v: &Value) -> [f64; 3] {
        std::array::from_fn(|i| v[i].as_f64().unwrap())
    }
    fn parts(f: &Value) -> Vec<Part> {
        f["dynamic_parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| Part {
                name: p["name"].as_str().unwrap().into(),
                group: p["group_index"].as_u64().unwrap() as usize,
                bounds: [point(&p["bounds_body_m"][0]), point(&p["bounds_body_m"][1])],
            })
            .collect()
    }
    fn shifted(b: Bounds, d: [f64; 3]) -> Bounds {
        b.map(|p| std::array::from_fn(|i| p[i] + d[i]))
    }
    fn boxes(state: BayRouteState, slot: u8, parts: &[Part]) -> Vec<(usize, Bounds)> {
        let refs = reference_poses();
        let mut b: Vec<_> = parts
            .iter()
            .map(|p| {
                let delta = std::array::from_fn(|i| {
                    state.mechanism_body_roots[p.group].position_m[i] - refs[p.group].position_m[i]
                });
                (p.group, shifted(p.bounds, delta))
            })
            .collect();
        for i in 0..15 {
            let pose = if i == usize::from(slot) {
                state.drone_body_pose
            } else {
                home_pose(i as u8).unwrap()
            };
            b.push((
                22 + i,
                [
                    std::array::from_fn(|j| pose.position_m[j] - DRONE_BAY_DRONE_HALF_EXTENTS_M[j]),
                    std::array::from_fn(|j| pose.position_m[j] + DRONE_BAY_DRONE_HALF_EXTENTS_M[j]),
                ],
            ));
        }
        b
    }
    // Solve exact time intervals of positive overlap for two translating boxes.
    // All bodies in one segment share the same smoothstep, a monotone common
    // reparameterization of t. This is continuous, not a sampled-pose check.
    fn overlap(a0: Bounds, a1: Bounds, b0: Bounds, b1: Bounds) -> bool {
        let mut lo: f64 = 0.;
        let mut hi: f64 = 1.;
        for axis in 0..3 {
            for (c0, c1) in [
                (
                    a0[1][axis] - b0[0][axis] - 1e-5,
                    a1[1][axis] - b1[0][axis] - 1e-5,
                ),
                (
                    b0[1][axis] - a0[0][axis] - 1e-5,
                    b1[1][axis] - a1[0][axis] - 1e-5,
                ),
            ] {
                let slope = c1 - c0;
                if slope.abs() < 1e-12 {
                    if c0 <= 0. {
                        return false;
                    }
                } else if slope > 0. {
                    lo = lo.max(-c0 / slope);
                } else {
                    hi = hi.min(-c0 / slope);
                }
                if lo >= hi {
                    return false;
                }
            }
        }
        true
    }
    fn route_hits(route: &BayRoute, parts: &[Part]) -> Vec<(usize, usize, usize)> {
        let mut hits = Vec::new();
        for (segment, pair) in route.keyframes.windows(2).enumerate() {
            let a = boxes(pair[0], route.slot, parts);
            let b = boxes(pair[1], route.slot, parts);
            for i in 0..a.len() {
                for j in i + 1..a.len() {
                    if a[i].0 != a[j].0 && overlap(a[i].1, b[i].1, a[j].1, b[j].1) {
                        hits.push((segment, i, j));
                    }
                }
            }
        }
        hits
    }
    #[test]
    fn exact_source_fixture_homes_groups_and_reference_roots_match() {
        let f = fixture();
        assert_eq!(f["glb_sha256"].as_str().unwrap(), DRONE_BAY_SOURCE_SHA256);
        assert_eq!(f["blend_sha256"].as_str().unwrap(), DRONE_BAY_BLEND_SHA256);
        assert_eq!(DRONE_BAY_REGISTRATION_REVISION, 12);
        let ids = mechanism_ids();
        assert_eq!(ids.into_iter().collect::<HashSet<_>>().len(), 22);
        for (i, p) in home_poses().into_iter().enumerate() {
            assert_eq!(p.position_m, point(&f["slots"][i]));
            assert_eq!(p.transform_vector([1., 0., 0.]).unwrap(), [-1., 0., 0.]);
            assert!(p.valid());
        }
        for (i, p) in reference_poses().into_iter().enumerate() {
            assert_eq!(ids[i], f["mechanisms"][i]["id"].as_str().unwrap());
            assert_eq!(
                p.position_m,
                point(&f["mechanisms"][i]["reference_pose_body_m"]["translation"])
            );
            assert_eq!(p.orientation_xyzw, [0., 0., 0., 1.]);
        }
        assert_eq!(parts(&f).len(), 71);
    }
    #[test]
    fn all_fifteen_launch_and_recovery_routes_are_continuously_clear_including_empty_return() {
        let p = parts(&fixture());
        for slot in 0..15 {
            for route in [launch_route(slot).unwrap(), recovery_route(slot).unwrap()] {
                let hits = route_hits(&route, &p);
                assert!(
                    hits.is_empty(),
                    "slot {slot} {:?}, first hits {:?}",
                    route.direction,
                    &hits[..hits.len().min(4)]
                );
                assert_eq!(route.keyframes.len(), 18);
                assert_eq!(route.segment_kinds.len(), 17);
                assert!(route.keyframes.iter().all(|s| s.valid()));
                for s in &route.keyframes {
                    for other in 0..15 {
                        if other != slot {
                            assert_eq!(
                                s.mechanism_body_roots[usize::from(other)],
                                reference_poses()[usize::from(other)]
                            );
                        }
                    }
                }
                assert_eq!(
                    route.keyframes.last().unwrap().mechanism_body_roots,
                    reference_poses()
                );
            }
        }
    }
    #[test]
    fn recovery_is_exact_full_reverse_without_teleporting_empty_acquisition() {
        for slot in 0..15 {
            let launch = launch_route(slot).unwrap();
            let recovery = recovery_route(slot).unwrap();
            assert_eq!(
                launch.keyframes.iter().rev().copied().collect::<Vec<_>>(),
                recovery.keyframes
            );
            assert_eq!(
                recovery.keyframes[0].drone_body_pose,
                DRONE_BAY_EXTERNAL_HOLD
            );
            assert_eq!(
                recovery.keyframes[17].drone_body_pose,
                home_pose(slot).unwrap()
            );
            for s in &recovery.keyframes[..=8] {
                assert_eq!(s.drone_body_pose, DRONE_BAY_EXTERNAL_HOLD);
            }
            assert_ne!(
                recovery.keyframes[6].mechanism_body_roots[usize::from(slot)],
                reference_poses()[usize::from(slot)]
            );
        }
    }
    #[test]
    fn negative_closed_door_and_missing_disengagement_detect_actual_body_collisions() {
        let p = parts(&fixture());
        for slot in 0..15 {
            let mut closed = launch_route(slot).unwrap();
            for s in &mut closed.keyframes {
                for id in [BayMechanismId::DoorUpper, BayMechanismId::DoorLower] {
                    s.mechanism_body_roots[id.index()] = id.reference_pose();
                }
            }
            let hits = route_hits(&closed, &p);
            assert!(
                hits.iter()
                    .any(|&(seg, i, j)| seg == 8 && (i >= p.len() || j >= p.len())),
                "closed doors must block slot{slot}"
            );
            let mut no_release = launch_route(slot).unwrap();
            for s in &mut no_release.keyframes[8..=9] {
                for id in [
                    DRONE_BAY_MECHANISMS[usize::from(slot)],
                    BayMechanismId::LiftXy,
                    BayMechanismId::ForkStage0,
                    BayMechanismId::ForkStage1,
                    BayMechanismId::ForkStage2,
                ] {
                    s.mechanism_body_roots[id.index()].position_m[1] += 0.2;
                }
            }
            let hits = route_hits(&no_release, &p);
            assert!(
                hits.iter().any(|&(seg, i, j)| seg == 8
                    && ((i < p.len() && p[i].name.contains("retainer/aft") && j >= p.len())
                        || (j < p.len() && p[j].name.contains("retainer/aft") && i >= p.len()))),
                "missing disengagement must hit aft lip slot{slot}"
            );
        }
    }
    #[test]
    fn milestones_bind_clear_supported_and_secured_endpoints_not_client_time() {
        for slot in 0..15 {
            let l = launch_route(slot).unwrap();
            let r = recovery_route(slot).unwrap();
            let lm = l.milestones();
            let rm = r.milestones();
            assert_eq!(
                l.keyframes[lm.drone_hull_clear_keyframe.unwrap()].drone_body_pose,
                DRONE_BAY_EXTERNAL_HOLD
            );
            assert!(
                l.keyframes[lm.drone_hull_clear_keyframe.unwrap()]
                    .drone_body_pose
                    .position_m[0]
                    + DRONE_BAY_DRONE_HALF_EXTENTS_M[0]
                    < DRONE_BAY_CARRIER_ENVELOPE.min_m[0]
            );
            assert_eq!(
                r.keyframes[rm.capture_supported_keyframe.unwrap()].drone_body_pose,
                DRONE_BAY_RELEASE_POSE
            );
            assert_eq!(
                r.keyframes[rm.capture_supported_keyframe.unwrap()].mechanism_body_roots
                    [usize::from(slot)]
                .position_m,
                DRONE_BAY_RELEASE_POSE.position_m
            );
            assert_eq!(
                r.keyframes[9].mechanism_body_roots[usize::from(slot)],
                DRONE_BAY_LOWERED_TRAY_POSE
            );
            assert_eq!(
                r.keyframes[rm.drone_home_keyframe.unwrap()].drone_body_pose,
                home_pose(slot).unwrap()
            );
            for (route, m) in [(&l, lm), (&r, rm)] {
                for id in [BayMechanismId::DoorUpper, BayMechanismId::DoorLower] {
                    assert_eq!(
                        route.keyframes[m.doors_secured_keyframe].mechanism(id),
                        id.reference_pose()
                    );
                }
            }
            assert!(l.segment_contract(8).unwrap().may_detach_after_endpoint);
            assert!(
                l.segment_contract(8)
                    .unwrap()
                    .handling_required_after_endpoint
            );
            assert_eq!(
                l.segment_contract(9).unwrap().drone_binding,
                BayDroneRouteBinding::MayDetach
            );
            assert_eq!(
                r.segment_contract(7).unwrap().drone_binding,
                BayDroneRouteBinding::ExternalAlignment
            );
            assert_eq!(
                l.segment_contract(7).unwrap().endpoint_support,
                [BayDroneSupport::TransferTray, BayDroneSupport::Unseated]
            );
            assert_eq!(
                r.segment_contract(9).unwrap().endpoint_support,
                [BayDroneSupport::Unseated, BayDroneSupport::TransferTray]
            );
            assert_eq!(
                r.segment_contract(8).unwrap().drone_binding,
                BayDroneRouteBinding::RouteDriven
            );
            assert!(
                !l.segment_contract(16)
                    .unwrap()
                    .handling_required_after_endpoint
            );
            for route in [&l, &r] {
                let c = route.segment_contract(8).unwrap();
                assert!(c.throat_swept);
                assert!(
                    (c.door_open_fraction[0] - 1.).abs() < 1e-12
                        && (c.door_open_fraction[1] - 1.).abs() < 1e-12
                );
            }
        }
    }
    #[test]
    fn common_smoothstep_preserves_all_relative_paths_and_rejects_invalid_sampling() {
        for slot in 0..15 {
            for route in [launch_route(slot).unwrap(), recovery_route(slot).unwrap()] {
                for i in 0..17 {
                    assert_eq!(
                        route.sample_segment(i, 100, 400, 100).unwrap(),
                        route.keyframes[i]
                    );
                    assert_eq!(
                        route.sample_segment(i, 100, 400, 500).unwrap(),
                        route.keyframes[i + 1]
                    );
                    let sampled = route.sample_segment(i, 100, 400, 200).unwrap();
                    let a = route.keyframes[i];
                    let b = route.keyframes[i + 1];
                    for (actual, (source, target)) in sampled.mechanism_body_roots.iter().zip(
                        a.mechanism_body_roots
                            .iter()
                            .zip(b.mechanism_body_roots.iter()),
                    ) {
                        for j in 0..3 {
                            assert!(
                                (actual.position_m[j]
                                    - (source.position_m[j]
                                        + (target.position_m[j] - source.position_m[j]) * 0.15625))
                                    .abs()
                                    < 1e-12
                            );
                        }
                    }
                }
                assert!(route.sample_segment(usize::MAX, 0, 1, 0).is_none());
                assert!(route.sample_segment(0, 0, 0, 0).is_none());
                assert!(route.sample_segment(0, u64::MAX, 1, 0).is_none());
            }
        }
        assert!(home_pose(15).is_none());
        assert!(launch_route(255).is_none());
        assert!(recovery_route(15).is_none());
    }
    #[test]
    fn swept_envelope_uses_full_parent_quaternion_and_rejects_invalid_bounds() {
        let box_m = BayEnvelope {
            min_m: [1., 2., 3.],
            max_m: [4., 6., 8.],
        };
        let a = std::f64::consts::FRAC_PI_4;
        let parent = BayPose {
            position_m: [10., 20., 30.],
            orientation_xyzw: [a.sin(), 0., 0., a.cos()],
        };
        let world = box_m.transformed(parent).unwrap();
        for (v, e) in world.min_m.into_iter().zip([11., 12., 32.]) {
            assert!((v - e).abs() < 1e-12);
        }
        for (v, e) in world.max_m.into_iter().zip([14., 17., 36.]) {
            assert!((v - e).abs() < 1e-12);
        }
        assert!(BayEnvelope {
            min_m: [f64::NAN, 0., 0.],
            ..box_m
        }
        .transformed(parent)
        .is_none());
        assert!(BayEnvelope {
            min_m: [5., 2., 3.],
            ..box_m
        }
        .transformed(parent)
        .is_none());
        assert!(box_m
            .transformed(BayPose {
                orientation_xyzw: [0.; 4],
                ..parent
            })
            .is_none());
    }
}
