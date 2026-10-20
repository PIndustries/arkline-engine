//! Version-two registration for the docked Pioneer walking scene.
//!
//! Positions are body metres until explicitly transformed into station metres.
//! The dimensions below are presentation and walking geometry, not engineering
//! ratings. The original room document and the canonical hull remain separate
//! sources; this registration deliberately binds them to one measured snapshot.
use crate::{
    FleetLayoutRevision, RoomLayoutDocument, ShipClass, StationObstacleKind, AVATAR_HEIGHT_M,
    PIONEER_CONNECTED_ROOM, STATION_AVATAR_RADIUS_M,
};
use serde::{Deserialize, Serialize};

// Revision 3 adds retained moving hatch leaves to the fitted bay/workstation.
// Old saved bindings must
// remain with their matching build until an explicit migration is validated.
pub const PIONEER_DOCKED_REGISTRATION_REVISION: u32 = 3;
pub const PIONEER_DOCKED_SOURCE_SHA256: &str =
    "8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123";
const EPS: f64 = 1.0e-8;
const FLOOR: f64 = -0.32;
// Fitted interior authoring choices in the unchanged canonical body frame.
// Registration admission/recovery must be coordinated before accepting this recipe.
const COCKPIT_REAR_X_M: f64 = 31.6;
const PILOT_STATION_SHIFT_X_M: f64 = -1.0;
fn pilot_station_point(point: [f64; 3]) -> [f64; 3] {
    add(point, [PILOT_STATION_SHIFT_X_M, 0.0, 0.0])
}
const P: [f64; 3] = [32.985, 0.0, -9.815];
const T: [f64; 3] = [0.7903075524006599, 0.0, 0.6127103495719207];
const N: [f64; 3] = [0.6127103495719207, 0.0, -0.7903075524006599];
const VESTIBULE_START: [f64; 3] = [34.1, FLOOR, -7.8];
const VESTIBULE_END: [f64; 3] = [32.49483172034246, FLOOR, -9.182753958079472];
const X: [f64; 3] = [1.0, 0.0, 0.0];
const Y: [f64; 3] = [0.0, 1.0, 0.0];
const Z: [f64; 3] = [0.0, 0.0, 1.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedPose {
    pub origin_m: [f64; 3],
    /// +X turns towards +Z; the renderer uses a negative Y rotation.
    pub yaw: f64,
}

impl PioneerDockedPose {
    pub fn private() -> Self {
        Self {
            origin_m: crate::hangar_layout(ShipClass::Pioneer).ship_center,
            yaw: 0.0,
        }
    }
    pub fn fleet(revision: FleetLayoutRevision) -> Self {
        let berth = crate::fleet_berth_pose(revision, ShipClass::Pioneer);
        Self {
            origin_m: berth.position,
            yaw: berth.yaw,
        }
    }
    pub fn body_to_station(self, point: [f64; 3]) -> [f64; 3] {
        body_to_station(self, point)
    }
    pub fn station_to_body(self, point: [f64; 3]) -> [f64; 3] {
        station_to_body(self, point)
    }
    pub fn valid(self) -> bool {
        finite(self.origin_m) && self.yaw.is_finite() && self.origin_m[1] > -FLOOR
    }
}

pub fn deck_floor_y(deck: u8) -> Option<f64> {
    match deck {
        1 => Some(-4.12),
        2 => Some(FLOOR),
        _ => None,
    }
}
pub fn deck_to_body(deck: u8, point: [f64; 3]) -> Option<[f64; 3]> {
    finite(point).then_some([
        point[0] - 12.0,
        deck_floor_y(deck)? + point[1],
        point[2] - 7.0,
    ])
}
pub fn body_to_station(pose: PioneerDockedPose, p: [f64; 3]) -> [f64; 3] {
    let (s, c) = pose.yaw.sin_cos();
    add(
        pose.origin_m,
        [c * p[0] - s * p[2], p[1], s * p[0] + c * p[2]],
    )
}
pub fn station_to_body(pose: PioneerDockedPose, p: [f64; 3]) -> [f64; 3] {
    let (s, c) = pose.yaw.sin_cos();
    let p = sub(p, pose.origin_m);
    [c * p[0] + s * p[2], p[1], -s * p[0] + c * p[2]]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PioneerDockedSupport {
    Station,
    Deck1,
    Deck2,
    Cockpit,
    Connector,
    CoreStairs,
    BoardingLift,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PioneerDockedCell {
    Station,
    Deck1,
    Deck2,
    Core,
    Cockpit,
    Connector,
    Vestibule,
    Room { deck: u8, room_id: u64 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PioneerDockedMaterial {
    WallPanel,
    /// Passive fitted finish; the existing solid bounds remain authoritative.
    FittedBulkhead,
    GripDeck,
    ServiceCeiling,
    Graphite,
    Titanium,
    Carbon,
    Emerald,
    WorkingLight,
    Amber,
    Glass,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PioneerDockedCollision {
    Solid,
    Support,
    Decoration,
    TraversalBarrier,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PioneerDockedPanelKind {
    Structure,
    RoomObstacle,
    Furniture,
    Instrument,
    Door,
    Lift,
    Rail,
    Light,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PioneerDockedGate {
    RoomDoor,
    InnerHatch,
    OuterHatch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PioneerDockedHatch {
    Inner,
    Outer,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedBox {
    pub center_m: [f64; 3],
    pub half_m: [f64; 3],
    /// Orthonormal column vectors, in body metres.
    pub axes: [[f64; 3]; 3],
}
impl PioneerDockedBox {
    pub fn valid(self) -> bool {
        finite(self.center_m)
            && finite(self.half_m)
            && self.half_m.iter().all(|v| *v > 0.0)
            && self
                .axes
                .iter()
                .all(|a| finite(*a) && (dot(*a, *a) - 1.0).abs() < 1.0e-7)
            && dot(self.axes[0], self.axes[1]).abs() < 1.0e-7
            && dot(self.axes[0], self.axes[2]).abs() < 1.0e-7
            && dot(self.axes[1], self.axes[2]).abs() < 1.0e-7
    }
    fn local(self, point: [f64; 3]) -> [f64; 3] {
        let p = sub(point, self.center_m);
        self.axes.map(|axis| dot(p, axis))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PioneerDockedPanel {
    pub id: String,
    pub bounds: PioneerDockedBox,
    pub material: PioneerDockedMaterial,
    pub collision: PioneerDockedCollision,
    pub cell: PioneerDockedCell,
    pub kind: PioneerDockedPanelKind,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PioneerDockedSurface {
    pub id: String,
    pub support: PioneerDockedSupport,
    pub center_m: [f64; 3],
    pub half_m: [f64; 2],
    pub axes: [[f64; 3]; 2],
    /// Allowed centre inset. Stair treads require a side inset, not a radius
    /// inset in their short run direction; consecutive treads share support.
    pub inset_m: [f64; 2],
}
#[derive(Clone, Debug, PartialEq)]
pub struct PioneerDockedPortal {
    pub id: String,
    pub from: PioneerDockedCell,
    pub to: PioneerDockedCell,
    pub center_m: [f64; 3],
    pub half_m: [f64; 2],
    pub axes: [[f64; 3]; 2],
    pub normal_m: [f64; 3],
    pub gate: Option<PioneerDockedGate>,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PioneerDockedGeometry {
    pub panels: Vec<PioneerDockedPanel>,
    pub supports: Vec<PioneerDockedSurface>,
    pub portals: Vec<PioneerDockedPortal>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedActuators {
    pub hatch_fractions: Option<[f64; 2]>,
    pub room_door_open: bool,
    pub inner_hatch_open: bool,
    pub outer_hatch_open: bool,
    pub lift_floor_station_m: f64,
    /// 0 deployed, 1 stowed. The pan folds down into its explicit recess.
    pub chair_fold: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct PioneerDockedSupportHit {
    pub support: PioneerDockedSupport,
    pub surface_id: String,
    pub height_m: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedSeatAnchors {
    pub root_station_m: [f64; 3],
    pub egress_station_m: [f64; 3],
    pub eye_station_m: [f64; 3],
    pub heading: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedHatchAnchors {
    pub inner_inside_station_m: [f64; 3],
    pub inner_outside_station_m: [f64; 3],
    pub outer_inside_station_m: [f64; 3],
    pub outer_outside_station_m: [f64; 3],
    /// These two bounds retain the body frame for shared occupancy checks.
    pub inner_bounds: PioneerDockedBox,
    pub outer_bounds: PioneerDockedBox,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedLiftGeometry {
    pub center_station_m: [f64; 3],
    pub station_floor_m: f64,
    pub ship_floor_m: f64,
    pub half_m: [f64; 2],
    pub tangent_station: [f64; 3],
    pub normal_station: [f64; 3],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedLiftCallAnchors {
    pub station_station_m: [f64; 3],
    pub ship_station_m: [f64; 3],
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PioneerDockedPilotJoints {
    pub root_m: [f64; 3],
    pub eye_m: [f64; 3],
    pub head_m: [f64; 3],
    pub neck_m: [f64; 3],
    pub shoulders_m: [[f64; 3]; 2],
    pub elbows_m: [[f64; 3]; 2],
    pub hands_m: [[f64; 3]; 2],
    pub hips_m: [[f64; 3]; 2],
    pub knees_m: [[f64; 3]; 2],
    pub feet_m: [[f64; 3]; 2],
}

pub fn pilot_joints(standing_fraction: f64) -> PioneerDockedPilotJoints {
    let p = finite_fraction(standing_fraction);
    let sides = [-1.0, 1.0];
    let shoulders = sides.map(|s| {
        lerp(
            [34.22, 0.76, 3.0 + s * 0.28],
            [34.54, 1.08, 3.0 + s * 0.28],
            p,
        )
    });
    let hands = sides.map(|s| {
        lerp(
            [34.50, 0.55, 3.0 + s * 0.20],
            [34.54, 0.55, 3.0 + s * 0.31],
            p,
        )
    });
    PioneerDockedPilotJoints {
        root_m: pilot_station_point([34.54, FLOOR, 3.0]),
        eye_m: pilot_station_point(lerp([34.22, 1.0, 3.0], [34.54, 1.33, 3.0], p)),
        head_m: pilot_station_point(lerp([34.22, 1.0, 3.0], [34.54, 1.32, 3.0], p)),
        neck_m: pilot_station_point(lerp([34.22, 0.82, 3.0], [34.54, 1.14, 3.0], p)),
        shoulders_m: shoulders.map(pilot_station_point),
        elbows_m: std::array::from_fn(|i| {
            pilot_station_point(add(
                lerp(shoulders[i], hands[i], 0.55),
                [-0.04, -0.035, 0.0],
            ))
        }),
        hands_m: hands.map(pilot_station_point),
        hips_m: sides.map(|s| {
            pilot_station_point(lerp(
                [34.22, 0.31, 3.0 + s * 0.13],
                [34.54, 0.62, 3.0 + s * 0.13],
                p,
            ))
        }),
        knees_m: sides.map(|s| {
            pilot_station_point(lerp(
                [34.62, 0.23, 3.0 + s * 0.13],
                [34.58, 0.1875, 3.0 + s * 0.13],
                p,
            ))
        }),
        feet_m: sides.map(|s| {
            pilot_station_point(lerp(
                [34.62, -0.23, 3.0 + s * 0.13],
                [34.54, -0.245, 3.0 + s * 0.13],
                p,
            ))
        }),
    }
}
pub fn seat_transition_pose(leaving: bool, progress: f64) -> (f64, f64) {
    let p = if leaving {
        finite_fraction(progress)
    } else {
        1.0 - finite_fraction(progress)
    };
    ((p * 2.0).min(1.0), (p * 2.0 - 1.0).max(0.0))
}
pub fn seat_anchors(pose: PioneerDockedPose) -> PioneerDockedSeatAnchors {
    PioneerDockedSeatAnchors {
        root_station_m: body_to_station(pose, pilot_joints(0.0).root_m),
        egress_station_m: body_to_station(pose, pilot_station_point([34.36, FLOOR, 3.0])),
        eye_station_m: body_to_station(pose, pilot_joints(0.0).eye_m),
        heading: pose.yaw,
    }
}

pub fn hatch_anchors(pose: PioneerDockedPose) -> PioneerDockedHatchAnchors {
    let d = normalized(sub(VESTIBULE_END, VESTIBULE_START));
    let lateral = [-d[2], 0.0, d[0]];
    // Leave the bow walkway clear for its turn into the vestibule.
    let inner_center = add(VESTIBULE_START, mul(d, 0.75));
    PioneerDockedHatchAnchors {
        inner_inside_station_m: body_to_station(pose, add(inner_center, mul(d, -0.75))),
        inner_outside_station_m: body_to_station(pose, add(inner_center, mul(d, 0.75))),
        outer_inside_station_m: body_to_station(pose, add(at_p(0.0, -0.65), [0.0, FLOOR, 0.0])),
        outer_outside_station_m: body_to_station(pose, add(at_p(0.0, 0.80), [0.0, FLOOR, 0.0])),
        inner_bounds: PioneerDockedBox {
            center_m: add(inner_center, [0.0, 1.2, 0.0]),
            half_m: [0.6, 1.2, 0.04],
            axes: [lateral, Y, d],
        },
        outer_bounds: PioneerDockedBox {
            center_m: add(P, [0.0, FLOOR + 1.2, 0.0]),
            half_m: [0.7, 1.2, 0.04],
            axes: [T, Y, N],
        },
    }
}
pub fn lift_geometry(pose: PioneerDockedPose) -> PioneerDockedLiftGeometry {
    let center = body_to_station(pose, at_p(0.0, 2.4));
    let rotation = |v| sub(body_to_station(pose, v), pose.origin_m);
    PioneerDockedLiftGeometry {
        center_station_m: [center[0], 0.0, center[2]],
        station_floor_m: 0.0,
        ship_floor_m: pose.origin_m[1] + FLOOR,
        half_m: [1.0, 0.8],
        tangent_station: rotation(T),
        normal_station: rotation(N),
    }
}
pub fn lift_call_anchors(pose: PioneerDockedPose) -> PioneerDockedLiftCallAnchors {
    let station = body_to_station(pose, at_p(0.0, 4.0));
    PioneerDockedLiftCallAnchors {
        station_station_m: [station[0], 0.0, station[2]],
        ship_station_m: body_to_station(pose, add(at_p(0.0, 0.9), [0.0, FLOOR, 0.0])),
    }
}
pub fn portal_open(portal: &PioneerDockedPortal, state: PioneerDockedActuators) -> bool {
    match portal.gate {
        None => true,
        Some(PioneerDockedGate::RoomDoor) => state.room_door_open,
        Some(PioneerDockedGate::InnerHatch) => state.inner_hatch_open,
        Some(PioneerDockedGate::OuterHatch) => state.outer_hatch_open,
    }
}

/// Closed-leaf occupancy is independent of the requested open state.
pub fn hatch_occupied(
    pose: PioneerDockedPose,
    hatch: PioneerDockedHatch,
    feet_station_m: [f64; 3],
) -> bool {
    if !pose.valid() || !finite(feet_station_m) {
        return true;
    }
    let anchors = hatch_anchors(pose);
    let bounds = match hatch {
        PioneerDockedHatch::Inner => anchors.inner_bounds,
        PioneerDockedHatch::Outer => anchors.outer_bounds,
    };
    box_intersects_capsule(
        bounds,
        station_to_body(pose, feet_station_m),
        AVATAR_HEIGHT_M,
        STATION_AVATAR_RADIUS_M,
    )
}

#[derive(Clone, Debug)]
pub struct PioneerDockedSourceCut {
    pub source_id: &'static str,
    pub primitive: usize,
    pub source_first_vertex: usize,
    pub source_vertex_count: usize,
    pub affected_triangle_starts: &'static [usize],
    pub volumes: Vec<PioneerDockedBox>,
}
pub fn pioneer_docked_source_cuts() -> Vec<PioneerDockedSourceCut> {
    // Open the original rear face and its roof bevel/lining only inside the
    // fitted bay. The new shared ceiling replaces the clipped inner roof face;
    // the canonical outer hull, glazing and its envelope remain untouched.
    let door = axis_box([33.549, -0.441, -6.881], [34.901, 2.881, 5.881]);
    let recess = axis_box([33.549, -0.441, -0.901], [33.751, -0.319, 0.901]);
    let portdoor = pane_box([-0.701, -0.321, -0.801], [0.701, 2.081, 0.801]);
    let portrecess = pane_box([-0.901, -0.441, -0.801], [0.901, -0.319, 0.801]);
    vec![
        PioneerDockedSourceCut {
            source_id: "part:d9e8088471d3686fd783795df45dcc235b99ab7426033cf4d2a2cfcc5ade3309",
            primitive: 0,
            source_first_vertex: 18864,
            source_vertex_count: 13194,
            affected_triangle_starts: &[31368, 31371, 31377, 31383, 31422, 31425, 31437, 31440],
            volumes: vec![door, recess],
        },
        PioneerDockedSourceCut {
            source_id: "part:b200a9824e59e3c68bcb4a9726401968c5dc72286637d03904f40a58e81f60e3",
            primitive: 6,
            source_first_vertex: 216,
            source_vertex_count: 36,
            affected_triangle_starts: &[222, 225, 228, 237, 240, 243],
            volumes: vec![portdoor, portrecess],
        },
        PioneerDockedSourceCut {
            source_id: "part:68b45ea99185087e982152189d9fb7514c5ba1e3c156a25af5dc55011957b036",
            primitive: 1,
            source_first_vertex: 5796,
            source_vertex_count: 324,
            affected_triangle_starts: &[
                5844, 5847, 5850, 5874, 5877, 5880, 5883, 5892, 5895, 5898, 5901, 5907, 5910, 5952,
                5955, 5967, 5970, 5973, 5976, 5979, 5982, 5985, 5988, 5991,
            ],
            volumes: vec![portdoor, portrecess],
        },
    ]
}

impl PioneerDockedGeometry {
    fn panel(
        &mut self,
        id: impl Into<String>,
        bounds: PioneerDockedBox,
        material: PioneerDockedMaterial,
        collision: PioneerDockedCollision,
        cell: PioneerDockedCell,
        kind: PioneerDockedPanelKind,
    ) {
        self.panels.push(PioneerDockedPanel {
            id: id.into(),
            bounds,
            material,
            collision,
            cell,
            kind,
        });
    }
    #[allow(clippy::too_many_arguments)] // Keep geometry bounds and semantic tags explicit.
    fn block(
        &mut self,
        id: impl Into<String>,
        min: [f64; 3],
        max: [f64; 3],
        material: PioneerDockedMaterial,
        collision: PioneerDockedCollision,
        cell: PioneerDockedCell,
        kind: PioneerDockedPanelKind,
    ) {
        self.panel(id, axis_box(min, max), material, collision, cell, kind);
    }
    fn surface(
        &mut self,
        id: impl Into<String>,
        support: PioneerDockedSupport,
        center_m: [f64; 3],
        half_m: [f64; 2],
        axes: [[f64; 3]; 2],
        inset_m: [f64; 2],
    ) {
        self.supports.push(PioneerDockedSurface {
            id: id.into(),
            support,
            center_m,
            half_m,
            axes,
            inset_m,
        });
    }
    #[allow(clippy::too_many_arguments)]
    fn floor(
        &mut self,
        id: &str,
        min: [f64; 2],
        max: [f64; 2],
        height: f64,
        thickness: f64,
        support: PioneerDockedSupport,
        cell: PioneerDockedCell,
        inset: [f64; 2],
    ) {
        self.block(
            id,
            [min[0], height - thickness, min[1]],
            [max[0], height, max[1]],
            PioneerDockedMaterial::GripDeck,
            PioneerDockedCollision::Support,
            cell,
            PioneerDockedPanelKind::Structure,
        );
        self.surface(
            id,
            support,
            [(min[0] + max[0]) * 0.5, height, (min[1] + max[1]) * 0.5],
            [(max[0] - min[0]) * 0.5, (max[1] - min[1]) * 0.5],
            [X, Z],
            inset,
        );
    }
    #[allow(clippy::too_many_arguments)]
    fn oriented_floor(
        &mut self,
        id: &str,
        center: [f64; 3],
        half: [f64; 2],
        axes: [[f64; 3]; 2],
        support: PioneerDockedSupport,
        cell: PioneerDockedCell,
        inset: [f64; 2],
    ) {
        self.panel(
            id,
            PioneerDockedBox {
                center_m: add(center, [0.0, -0.06, 0.0]),
                half_m: [half[0], 0.06, half[1]],
                axes: [axes[0], Y, axes[1]],
            },
            PioneerDockedMaterial::GripDeck,
            PioneerDockedCollision::Support,
            cell,
            PioneerDockedPanelKind::Structure,
        );
        self.surface(id, support, center, half, axes, inset);
    }
    #[allow(clippy::too_many_arguments)]
    fn portal(
        &mut self,
        id: &str,
        from: PioneerDockedCell,
        to: PioneerDockedCell,
        center: [f64; 3],
        half: [f64; 2],
        axes: [[f64; 3]; 2],
        normal: [f64; 3],
        gate: Option<PioneerDockedGate>,
    ) {
        self.portals.push(PioneerDockedPortal {
            id: id.into(),
            from,
            to,
            center_m: center,
            half_m: half,
            axes,
            normal_m: normal,
            gate,
        });
    }
}

/// Subtract the two flights and their shared landing from the actual ceiling
/// and upper-floor slabs. The room document itself is never rewritten.
fn well_rectangles() -> [[f64; 4]; 3] {
    [
        [0.3, 1.68, 1.5, 5.2],
        [2.5, 1.68, 3.7, 3.92],
        [0.3, 0.2, 3.7, 1.68],
    ]
}
fn subtract_rect(rect: [f64; 4], hole: [f64; 4]) -> Vec<[f64; 4]> {
    let x0 = rect[0].max(hole[0]);
    let z0 = rect[1].max(hole[1]);
    let x1 = rect[2].min(hole[2]);
    let z1 = rect[3].min(hole[3]);
    if x1 <= x0 + EPS || z1 <= z0 + EPS {
        return vec![rect];
    }
    [
        [rect[0], rect[1], x0, rect[3]],
        [x1, rect[1], rect[2], rect[3]],
        [x0, rect[1], x1, z0],
        [x0, z1, x1, rect[3]],
    ]
    .into_iter()
    .filter(|r| r[2] > r[0] + EPS && r[3] > r[1] + EPS)
    .collect()
}
fn slab_rectangles(well: bool) -> Vec<[f64; 4]> {
    let mut result = vec![[0.0, 0.0, 32.0, 14.0]];
    if well {
        for hole in well_rectangles() {
            result = result
                .into_iter()
                .flat_map(|r| subtract_rect(r, hole))
                .collect();
        }
    }
    result
}

/// Build immutable layout geometry once per installed room-document hash.
pub fn registered_geometry(document: &RoomLayoutDocument) -> Result<PioneerDockedGeometry, String> {
    document.validate_live()?;
    let mut g = PioneerDockedGeometry::default();
    for deck in [1, 2] {
        let floor = deck_floor_y(deck).expect("two registered decks");
        let cell = if deck == 1 {
            PioneerDockedCell::Deck1
        } else {
            PioneerDockedCell::Deck2
        };
        let support = if deck == 1 {
            PioneerDockedSupport::Deck1
        } else {
            PioneerDockedSupport::Deck2
        };
        for (i, r) in slab_rectangles(deck == 2).into_iter().enumerate() {
            g.floor(
                &format!("deck{deck}/floor/{i}"),
                [r[0] - 12.0, r[1] - 7.0],
                [r[2] - 12.0, r[3] - 7.0],
                floor,
                0.1,
                support,
                cell,
                [0.0, 0.0],
            );
        }
        for (i, r) in slab_rectangles(deck == 1).into_iter().enumerate() {
            g.block(
                format!("deck{deck}/ceiling/{i}"),
                [r[0] - 12.0, floor + 3.2, r[1] - 7.0],
                [r[2] - 12.0, floor + 3.3, r[3] - 7.0],
                PioneerDockedMaterial::ServiceCeiling,
                PioneerDockedCollision::Solid,
                cell,
                PioneerDockedPanelKind::Structure,
            );
        }
        // Side and aft boundaries retain the live plan. Only deck two's
        // forward corridor receives the explicitly registered 1.4 m opening.
        let walls = [
            ([0.0, 0.0, 0.0], [32.0, 3.2, 0.15]),
            ([0.0, 0.0, 13.85], [32.0, 3.2, 14.0]),
            ([0.0, 0.0, 0.0], [0.15, 3.2, 14.0]),
        ];
        for (i, (min, max)) in walls.into_iter().enumerate() {
            g.block(
                format!("deck{deck}/boundary/{i}"),
                deck_to_body(deck, min).unwrap(),
                deck_to_body(deck, max).unwrap(),
                PioneerDockedMaterial::WallPanel,
                PioneerDockedCollision::Solid,
                cell,
                PioneerDockedPanelKind::Structure,
            );
        }
        let front = if deck == 2 {
            vec![
                ([31.85, 0.0, 0.0], [32.0, 3.2, 6.3]),
                ([31.85, 0.0, 7.7], [32.0, 3.2, 14.0]),
                ([31.85, 2.4, 6.3], [32.0, 3.2, 7.7]),
            ]
        } else {
            vec![([31.85, 0.0, 0.0], [32.0, 3.2, 14.0])]
        };
        for (i, (min, max)) in front.into_iter().enumerate() {
            g.block(
                format!("deck{deck}/forward/{i}"),
                deck_to_body(deck, min).unwrap(),
                deck_to_body(deck, max).unwrap(),
                PioneerDockedMaterial::WallPanel,
                PioneerDockedCollision::Solid,
                cell,
                PioneerDockedPanelKind::Structure,
            );
        }
        g.block(
            format!("deck{deck}/access-cabinet"),
            deck_to_body(deck, [0.7, 0.0, 6.45]).unwrap(),
            deck_to_body(deck, [1.0, 1.3, 7.55]).unwrap(),
            PioneerDockedMaterial::Carbon,
            PioneerDockedCollision::Solid,
            PioneerDockedCell::Core,
            PioneerDockedPanelKind::Structure,
        );
        g.portal(
            &format!("deck{deck}/core"),
            cell,
            PioneerDockedCell::Core,
            [-8.0, floor + 1.6, -0.05],
            [1.15, 1.6],
            [Z, Y],
            X,
            None,
        );
        for room in document.rooms.iter().filter(|room| room.deck == deck) {
            let roomcell = PioneerDockedCell::Room {
                deck,
                room_id: room.id,
            };
            for (i, o) in room.obstacles().into_iter().enumerate() {
                g.block(
                    format!("room/{}/{i}", room.id),
                    deck_to_body(deck, o.min).unwrap(),
                    deck_to_body(deck, o.max).unwrap(),
                    if o.kind == StationObstacleKind::Storage {
                        PioneerDockedMaterial::Carbon
                    } else {
                        PioneerDockedMaterial::WallPanel
                    },
                    PioneerDockedCollision::Solid,
                    roomcell,
                    PioneerDockedPanelKind::RoomObstacle,
                );
            }
            let x = room.door_center_xz_m[0];
            let z = room.door_wall_z();
            let [w, h] = room.door_clear_wh_m;
            // A real header closes the space above the advertised doorway.
            if room.clear_lwh_m[2] > h + EPS {
                g.block(
                    format!("room/{}/header", room.id),
                    deck_to_body(deck, [x - w * 0.5, h, z]).unwrap(),
                    deck_to_body(deck, [x + w * 0.5, room.clear_lwh_m[2], z + 0.15]).unwrap(),
                    PioneerDockedMaterial::WallPanel,
                    PioneerDockedCollision::Solid,
                    roomcell,
                    PioneerDockedPanelKind::Structure,
                );
            }
            g.portal(
                &format!("room/{}/door", room.id),
                roomcell,
                cell,
                deck_to_body(deck, [x, h * 0.5, z + 0.075]).unwrap(),
                [w * 0.5, h * 0.5],
                [X, Y],
                Z,
                (room.runtime_room_id.as_deref() == Some(PIONEER_CONNECTED_ROOM.room_id))
                    .then_some(PioneerDockedGate::RoomDoor),
            );
        }
    }
    add_stairs(&mut g);
    add_connector(&mut g);
    add_cockpit_furniture(&mut g);
    add_service_finish(&mut g);
    validate_geometry(&g)?;
    Ok(g)
}

/// Construction finish shares the registered geometry in every camera/route.
/// These static ceiling/service housings create no equipment or powered circuit.
fn add_service_finish(g: &mut PioneerDockedGeometry) {
    for deck in [1, 2] {
        let floor = deck_floor_y(deck).unwrap();
        let cell = if deck == 1 {
            PioneerDockedCell::Deck1
        } else {
            PioneerDockedCell::Deck2
        };
        // Service cassettes and separated longitudinal cable trays stay above
        // 2.9 m clear height; the aft stairwell retains its existing open well.
        for bay in 0..12 {
            let x = -5.5 + bay as f64 * 2.0;
            g.block(
                format!("finish/deck{deck}/cassette/{bay}"),
                [x, floor + 3.08, -0.68],
                [x + 1.82, floor + 3.18, 0.68],
                PioneerDockedMaterial::ServiceCeiling,
                PioneerDockedCollision::Decoration,
                cell,
                PioneerDockedPanelKind::Structure,
            );
            for side in [-1.0, 1.0] {
                let z = side * 0.82;
                g.block(
                    format!("finish/deck{deck}/tray/{bay}/{side}"),
                    [x, floor + 2.98, z - 0.08],
                    [x + 1.82, floor + 3.11, z + 0.08],
                    PioneerDockedMaterial::Titanium,
                    PioneerDockedCollision::Decoration,
                    cell,
                    PioneerDockedPanelKind::Structure,
                );
                g.block(
                    format!("finish/deck{deck}/tray-lid/{bay}/{side}"),
                    [x + 0.05, floor + 2.96, z - 0.055],
                    [x + 1.77, floor + 2.98, z + 0.055],
                    PioneerDockedMaterial::Carbon,
                    PioneerDockedCollision::Decoration,
                    cell,
                    PioneerDockedPanelKind::Structure,
                );
            }
        }
    }
    // The fitted bridge has its own service ceiling, not an exposed hull void.
    for bay in 0..3 {
        let x = 31.75 + bay as f64;
        for side in [-1.0, 1.0] {
            let z = side * 4.8;
            g.block(
                format!("finish/bridge/service/{bay}/{side}"),
                [x, 2.72, z - 0.25],
                [x + 0.82, 2.87, z + 0.25],
                PioneerDockedMaterial::ServiceCeiling,
                PioneerDockedCollision::Decoration,
                PioneerDockedCell::Cockpit,
                PioneerDockedPanelKind::Structure,
            );
        }
    }
}

fn add_stairs(g: &mut PioneerDockedGeometry) {
    let y = deck_floor_y(1).unwrap();
    let cell = PioneerDockedCell::Core;
    for i in 1..=11 {
        let z1 = 5.2 - (i - 1) as f64 * 0.32;
        let z0 = z1 - 0.32;
        let h = y + i as f64 * 0.19;
        g.block(
            format!("stairs/a/{i}"),
            [-11.7, y, z0 - 7.0],
            [-10.5, h, z1 - 7.0],
            PioneerDockedMaterial::Titanium,
            PioneerDockedCollision::Support,
            cell,
            PioneerDockedPanelKind::Structure,
        );
        g.surface(
            format!("stairs/a/{i}"),
            PioneerDockedSupport::CoreStairs,
            [-11.1, h, (z0 + z1) * 0.5 - 7.0],
            [0.6, 0.16],
            [X, Z],
            [0.3, 0.0],
        );
    }
    let mid = y + 2.28;
    g.floor(
        "stairs/midlanding",
        [-11.7, -6.8],
        [-8.3, -5.32],
        mid,
        0.12,
        PioneerDockedSupport::CoreStairs,
        cell,
        [0.3, 0.0],
    );
    g.block(
        "stairs/a/final-riser",
        [-11.7, y + 2.09, -5.34],
        [-10.5, mid, -5.30],
        PioneerDockedMaterial::Titanium,
        PioneerDockedCollision::Support,
        cell,
        PioneerDockedPanelKind::Structure,
    );
    for i in 1..=7 {
        let z0 = 1.68 + (i - 1) as f64 * 0.32;
        let z1 = z0 + 0.32;
        let h = mid + i as f64 * 0.19;
        g.block(
            format!("stairs/b/{i}"),
            [-9.5, mid, z0 - 7.0],
            [-8.3, h, z1 - 7.0],
            PioneerDockedMaterial::Titanium,
            PioneerDockedCollision::Support,
            cell,
            PioneerDockedPanelKind::Structure,
        );
        g.surface(
            format!("stairs/b/{i}"),
            PioneerDockedSupport::CoreStairs,
            [-8.9, h, (z0 + z1) * 0.5 - 7.0],
            [0.6, 0.16],
            [X, Z],
            [0.3, 0.0],
        );
    }
    g.block(
        "stairs/b/final-riser",
        [-9.5, mid + 1.33, -3.10],
        [-8.3, FLOOR, -3.06],
        PioneerDockedMaterial::Titanium,
        PioneerDockedCollision::Support,
        cell,
        PioneerDockedPanelKind::Structure,
    );
    // Rails lie outside the stated 1.2 m flight width. The central approach
    // remains 0.9 m wide after its two 5 cm rail faces.
    for (label, xs, z0, z1, h0, h1) in [
        ("a", [-11.725, -10.475], -5.32, -1.8, mid, y + 0.19),
        ("b", [-9.525, -8.275], -5.32, -3.08, mid + 0.19, FLOOR),
    ] {
        for (side, x) in xs.into_iter().enumerate() {
            rail(
                g,
                &format!("stairs/{label}/rail/{side}"),
                [x, h0 + 0.95, z0],
                [x, h1 + 0.95, z1],
                0.025,
                cell,
            );
            for j in 0..=3 {
                let f = j as f64 / 3.0;
                let z = z0 + (z1 - z0) * f;
                let h = h0 + (h1 - h0) * f;
                g.block(
                    format!("stairs/{label}/post/{side}/{j}"),
                    [x - 0.025, h - 0.03, z - 0.025],
                    [x + 0.025, h + 0.95, z + 0.025],
                    PioneerDockedMaterial::Emerald,
                    PioneerDockedCollision::Solid,
                    cell,
                    PioneerDockedPanelKind::Rail,
                );
            }
        }
    }
    rail(
        g,
        "stairs/mid/back-rail",
        [-11.7, mid + 0.95, -6.825],
        [-8.3, mid + 0.95, -6.825],
        0.025,
        cell,
    );
    for x in [-11.725, -8.275] {
        rail(
            g,
            &format!("stairs/mid/side/{x}"),
            [x, mid + 0.95, -6.8],
            [x, mid + 0.95, -5.32],
            0.025,
            cell,
        );
    }
    g.portal(
        "stairs/well",
        PioneerDockedCell::Deck1,
        PioneerDockedCell::Deck2,
        [-10.0, FLOOR, -5.99],
        [1.7, 0.74],
        [X, Z],
        Y,
        None,
    );
}

fn rail(
    g: &mut PioneerDockedGeometry,
    id: &str,
    a: [f64; 3],
    b: [f64; 3],
    radius: f64,
    cell: PioneerDockedCell,
) {
    let d = sub(b, a);
    let length = dot(d, d).sqrt();
    let u = mul(d, 1.0 / length);
    let helper = if u[0].abs() < 0.8 { X } else { Z };
    let w = normalized(cross(u, helper));
    let v = cross(w, u);
    g.panel(
        id,
        PioneerDockedBox {
            center_m: mul(add(a, b), 0.5),
            half_m: [length * 0.5, radius, radius],
            axes: [u, v, w],
        },
        PioneerDockedMaterial::Emerald,
        PioneerDockedCollision::Solid,
        cell,
        PioneerDockedPanelKind::Rail,
    );
}

fn add_connector(g: &mut PioneerDockedGeometry) {
    let cell = PioneerDockedCell::Connector;
    g.floor(
        "connector/floor",
        [20.0, -0.9],
        [COCKPIT_REAR_X_M, 0.9],
        FLOOR,
        0.12,
        PioneerDockedSupport::Connector,
        cell,
        [0.0, 0.3],
    );
    g.block(
        "connector/roof",
        [20.0, 2.08, -0.78],
        [COCKPIT_REAR_X_M, 2.20, 0.78],
        PioneerDockedMaterial::ServiceCeiling,
        PioneerDockedCollision::Solid,
        cell,
        PioneerDockedPanelKind::Structure,
    );
    for side in [-1.0, 1.0] {
        let z = side * 0.74;
        g.block(
            format!("connector/wall/{side}"),
            [20.0, FLOOR, z - 0.04],
            [COCKPIT_REAR_X_M, 2.08, z + 0.04],
            PioneerDockedMaterial::FittedBulkhead,
            PioneerDockedCollision::Solid,
            cell,
            PioneerDockedPanelKind::Structure,
        );
    }
    g.floor(
        "cockpit/aft-crosswalk",
        [COCKPIT_REAR_X_M, -6.8],
        [34.7, 5.8],
        FLOOR,
        0.12,
        PioneerDockedSupport::Cockpit,
        PioneerDockedCell::Cockpit,
        [0.0, 0.0],
    );
    g.floor(
        "cockpit/seat-floor",
        [34.7, -6.8],
        [34.9, 5.8],
        FLOOR,
        0.12,
        PioneerDockedSupport::Cockpit,
        PioneerDockedCell::Cockpit,
        [0.0, 0.0],
    );
    g.floor(
        "cockpit/port-walkway",
        [33.7, -8.4],
        [34.5, -6.8],
        FLOOR,
        0.12,
        PioneerDockedSupport::Cockpit,
        PioneerDockedCell::Cockpit,
        [0.0, 0.0],
    );
    // Real bay boundaries share the floor/collision recipe. The 0.8 m port
    // opening retains the existing measured walkway and boarding vestibule.
    for (i, (z0, z1)) in [(-6.88, -0.7), (0.7, 5.88)].into_iter().enumerate() {
        g.block(
            format!("cockpit/rear-bulkhead/{i}"),
            [COCKPIT_REAR_X_M - 0.12, FLOOR, z0],
            [COCKPIT_REAR_X_M, 2.88, z1],
            PioneerDockedMaterial::FittedBulkhead,
            PioneerDockedCollision::Solid,
            PioneerDockedCell::Cockpit,
            PioneerDockedPanelKind::Structure,
        );
    }
    g.block(
        "cockpit/rear-header",
        [COCKPIT_REAR_X_M - 0.12, 2.08, -0.7],
        [COCKPIT_REAR_X_M, 2.88, 0.7],
        PioneerDockedMaterial::Titanium,
        PioneerDockedCollision::Solid,
        PioneerDockedCell::Cockpit,
        PioneerDockedPanelKind::Structure,
    );
    for (id, min, max) in [
        (
            "ceiling",
            [COCKPIT_REAR_X_M - 0.12, 2.88, -6.88],
            [34.901, 2.98, 5.88],
        ),
        (
            "starboard-wall",
            [COCKPIT_REAR_X_M, FLOOR, 5.8],
            [34.9, 2.88, 5.88],
        ),
        (
            "port-aft-wall",
            [COCKPIT_REAR_X_M, FLOOR, -6.88],
            [33.7, 2.88, -6.8],
        ),
        (
            "port-forward-wall",
            [34.5, FLOOR, -6.88],
            [34.9, 2.88, -6.8],
        ),
        ("port-exit-header", [33.7, 2.08, -6.88], [34.5, 2.88, -6.8]),
    ] {
        g.block(
            format!("cockpit/{id}"),
            min,
            max,
            if id == "ceiling" {
                PioneerDockedMaterial::ServiceCeiling
            } else if id == "starboard-wall" || id == "port-aft-wall" {
                PioneerDockedMaterial::FittedBulkhead
            } else {
                PioneerDockedMaterial::Graphite
            },
            PioneerDockedCollision::Solid,
            PioneerDockedCell::Cockpit,
            PioneerDockedPanelKind::Structure,
        );
    }
    add_cockpit_exit_guidance(g);
    let d = normalized(sub(VESTIBULE_END, VESTIBULE_START));
    let lateral = [-d[2], 0.0, d[0]];
    let length = dot(
        sub(VESTIBULE_END, VESTIBULE_START),
        sub(VESTIBULE_END, VESTIBULE_START),
    )
    .sqrt();
    g.oriented_floor(
        "vestibule/floor",
        mul(add(VESTIBULE_START, VESTIBULE_END), 0.5),
        [0.7, length * 0.5 + 0.05],
        [lateral, d],
        PioneerDockedSupport::Connector,
        PioneerDockedCell::Vestibule,
        [0.3, 0.0],
    );
    // The bend into the outer doorway stays open. No pressure-volume or
    // engineering seal is claimed for these new walking vestibule surfaces.
    for side in [-1.0, 1.0] {
        let a = add(VESTIBULE_START, mul(lateral, side * 0.64));
        let b = add(add(VESTIBULE_END, mul(d, -0.75)), mul(lateral, side * 0.64));
        let c = mul(add(a, b), 0.5);
        let len = dot(sub(b, a), sub(b, a)).sqrt();
        g.panel(
            format!("vestibule/wall/{side}"),
            PioneerDockedBox {
                center_m: add(c, [0.0, 1.2, 0.0]),
                half_m: [0.04, 1.2, len * 0.5],
                axes: [lateral, Y, d],
            },
            PioneerDockedMaterial::Graphite,
            PioneerDockedCollision::Solid,
            PioneerDockedCell::Vestibule,
            PioneerDockedPanelKind::Structure,
        );
    }
    // This is the same physical floor as the vestibule and the ship-level
    // lift. Its support and solid top must coincide exactly at their joins.
    g.oriented_floor(
        "boarding/threshold",
        add(at_p(0.0, 0.4), [0.0, FLOOR, 0.0]),
        [0.9, 1.2],
        [T, N],
        PioneerDockedSupport::Connector,
        PioneerDockedCell::Vestibule,
        [0.3, 0.0],
    );
    let anchors = hatch_anchors(PioneerDockedPose::private());
    hatch_frame(
        g,
        "boarding/inner",
        anchors.inner_bounds,
        PioneerDockedCell::Vestibule,
    );
    hatch_frame(
        g,
        "boarding/outer",
        anchors.outer_bounds,
        PioneerDockedCell::Vestibule,
    );
    g.portal(
        "deck2/forward-portal",
        PioneerDockedCell::Deck2,
        cell,
        [20.0, FLOOR + 1.2, 0.0],
        [0.7, 1.2],
        [Z, Y],
        X,
        None,
    );
    g.portal(
        "cockpit/backwall-portal",
        cell,
        PioneerDockedCell::Cockpit,
        [COCKPIT_REAR_X_M, FLOOR + 1.2, 0.0],
        [0.7, 1.2],
        [Z, Y],
        X,
        None,
    );
    g.portal(
        "boarding/inner-portal",
        PioneerDockedCell::Cockpit,
        PioneerDockedCell::Vestibule,
        anchors.inner_bounds.center_m,
        [0.6, 1.2],
        [lateral, Y],
        d,
        Some(PioneerDockedGate::InnerHatch),
    );
    g.portal(
        "boarding/outer-portal",
        PioneerDockedCell::Vestibule,
        PioneerDockedCell::Station,
        anchors.outer_bounds.center_m,
        [0.7, 1.2],
        [T, Y],
        N,
        Some(PioneerDockedGate::OuterHatch),
    );
    for side in [-1.0, 1.0] {
        g.panel(
            format!("threshold/rail/{side}"),
            pane_box(
                [side * 0.82 - 0.04, 0.69, 0.85],
                [side * 0.82 + 0.04, 0.77, 1.6],
            ),
            PioneerDockedMaterial::Emerald,
            PioneerDockedCollision::Solid,
            PioneerDockedCell::Station,
            PioneerDockedPanelKind::Rail,
        );
        for n in [0.95, 1.5] {
            g.panel(
                format!("threshold/post/{side}/{n}"),
                pane_box(
                    [side * 0.82 - 0.04, FLOOR, n - 0.04],
                    [side * 0.82 + 0.04, 0.73, n + 0.04],
                ),
                PioneerDockedMaterial::Titanium,
                PioneerDockedCollision::Solid,
                PioneerDockedCell::Station,
                PioneerDockedPanelKind::Rail,
            );
        }
    }
}

/// A physical sign and floor arrow at the actual port boarding route. These
/// reflective markings do not add a lamp, powered device, collision or shortcut.
fn add_cockpit_exit_guidance(g: &mut PioneerDockedGeometry) {
    g.block(
        "guidance/cockpit-exit-to-hangar/backing",
        [33.69, 2.28, -6.797],
        [34.51, 2.66, -6.777],
        PioneerDockedMaterial::Carbon,
        PioneerDockedCollision::Decoration,
        PioneerDockedCell::Cockpit,
        PioneerDockedPanelKind::Structure,
    );
    for (line, text, y) in [(0, "EXIT TO", 2.49), (1, "HANGAR", 2.33)] {
        let advance = 0.092;
        let width = text.len() as f64 * advance - 0.017;
        for (letter, ch) in text.chars().enumerate() {
            let x = 34.1 - width * 0.5 + letter as f64 * advance;
            for (stroke, (a, b)) in exit_glyph(ch).iter().enumerate() {
                let p = [x + a[0] * 0.075, y + a[1] * 0.115, -6.773];
                let q = [x + b[0] * 0.075, y + b[1] * 0.115, -6.773];
                guidance_bar(
                    g,
                    format!("guidance/cockpit-exit-to-hangar/{line}/{letter}/{stroke}"),
                    p,
                    q,
                    0.006,
                );
            }
        }
    }
    for (i, (a, b)) in [
        ([34.1, FLOOR + 0.005, -5.55], [34.1, FLOOR + 0.005, -6.35]),
        ([33.93, FLOOR + 0.005, -6.15], [34.1, FLOOR + 0.005, -6.35]),
        ([34.27, FLOOR + 0.005, -6.15], [34.1, FLOOR + 0.005, -6.35]),
    ]
    .into_iter()
    .enumerate()
    {
        let direction = normalized(sub(b, a));
        g.panel(
            format!("guidance/cockpit-exit-arrow/{i}"),
            PioneerDockedBox {
                center_m: mul(add(a, b), 0.5),
                half_m: [dot(sub(b, a), sub(b, a)).sqrt() * 0.5, 0.002, 0.015],
                axes: [direction, Y, [-direction[2], 0.0, direction[0]]],
            },
            PioneerDockedMaterial::Amber,
            PioneerDockedCollision::Decoration,
            PioneerDockedCell::Cockpit,
            PioneerDockedPanelKind::Structure,
        );
    }
}
fn guidance_bar(g: &mut PioneerDockedGeometry, id: String, a: [f64; 3], b: [f64; 3], width: f64) {
    let direction = normalized(sub(b, a));
    g.panel(
        id,
        PioneerDockedBox {
            center_m: mul(add(a, b), 0.5),
            half_m: [dot(sub(b, a), sub(b, a)).sqrt() * 0.5, width * 0.5, 0.002],
            axes: [direction, [-direction[1], direction[0], 0.0], Z],
        },
        PioneerDockedMaterial::Amber,
        PioneerDockedCollision::Decoration,
        PioneerDockedCell::Cockpit,
        PioneerDockedPanelKind::Structure,
    );
}
fn exit_glyph(ch: char) -> &'static [([f64; 2], [f64; 2])] {
    match ch {
        'E' => &[
            ([0., 0.], [0., 1.]),
            ([0., 1.], [1., 1.]),
            ([0., 0.5], [0.8, 0.5]),
            ([0., 0.], [1., 0.]),
        ],
        'X' => &[([0., 0.], [1., 1.]), ([0., 1.], [1., 0.])],
        'I' => &[
            ([0., 1.], [1., 1.]),
            ([0.5, 0.], [0.5, 1.]),
            ([0., 0.], [1., 0.]),
        ],
        'T' => &[([0., 1.], [1., 1.]), ([0.5, 0.], [0.5, 1.])],
        'O' => &[
            ([0., 0.], [0., 1.]),
            ([0., 1.], [1., 1.]),
            ([1., 0.], [1., 1.]),
            ([0., 0.], [1., 0.]),
        ],
        'H' => &[
            ([0., 0.], [0., 1.]),
            ([1., 0.], [1., 1.]),
            ([0., 0.5], [1., 0.5]),
        ],
        'A' => &[
            ([0., 0.], [0.5, 1.]),
            ([0.5, 1.], [1., 0.]),
            ([0.2, 0.4], [0.8, 0.4]),
        ],
        'N' => &[
            ([0., 0.], [0., 1.]),
            ([1., 0.], [1., 1.]),
            ([0., 1.], [1., 0.]),
        ],
        'G' => &[
            ([0., 0.], [0., 1.]),
            ([0., 1.], [1., 1.]),
            ([0., 0.], [1., 0.]),
            ([1., 0.], [1., 0.5]),
            ([0.5, 0.5], [1., 0.5]),
        ],
        'R' => &[
            ([0., 0.], [0., 1.]),
            ([0., 1.], [1., 1.]),
            ([1., 0.5], [1., 1.]),
            ([0., 0.5], [1., 0.5]),
            ([0.5, 0.5], [1., 0.]),
        ],
        _ => &[],
    }
}

fn hatch_frame(
    g: &mut PioneerDockedGeometry,
    prefix: &str,
    b: PioneerDockedBox,
    cell: PioneerDockedCell,
) {
    for side in [-1.0, 1.0] {
        let c = add(b.center_m, mul(b.axes[0], side * (b.half_m[0] + 0.04)));
        g.panel(
            format!("{prefix}/jamb/{side}"),
            PioneerDockedBox {
                center_m: add(c, mul(b.axes[2], -0.12)),
                half_m: [0.04, b.half_m[1], 0.04],
                axes: b.axes,
            },
            PioneerDockedMaterial::Emerald,
            PioneerDockedCollision::Solid,
            cell,
            PioneerDockedPanelKind::Structure,
        );
    }
    g.panel(
        format!("{prefix}/header"),
        PioneerDockedBox {
            center_m: add(
                add(b.center_m, [0.0, b.half_m[1] + 0.04, 0.0]),
                mul(b.axes[2], -0.12),
            ),
            half_m: [b.half_m[0] + 0.08, 0.04, 0.04],
            axes: b.axes,
        },
        PioneerDockedMaterial::Titanium,
        PioneerDockedCollision::Solid,
        cell,
        PioneerDockedPanelKind::Structure,
    );
}

fn add_cockpit_furniture(g: &mut PioneerDockedGeometry) {
    use PioneerDockedMaterial::{Carbon, Graphite, Titanium};
    let cell = PioneerDockedCell::Cockpit;
    let solid = PioneerDockedCollision::Solid;
    for (i, (z0, z1)) in [(2.27, 2.53), (3.47, 3.73)].into_iter().enumerate() {
        g.block(
            format!("cockpit/console/{i}"),
            pilot_station_point([34.76, 0.47, z0]),
            pilot_station_point([35.08, 0.77, z1]),
            Graphite,
            solid,
            cell,
            PioneerDockedPanelKind::Furniture,
        );
        g.block(
            format!("cockpit/console-pedestal/{i}"),
            pilot_station_point([34.78, FLOOR, z0 + 0.04]),
            pilot_station_point([34.88, 0.47, z1 - 0.04]),
            Titanium,
            solid,
            cell,
            PioneerDockedPanelKind::Furniture,
        );
    }
    g.block(
        "pilot-chair/wallplate",
        pilot_station_point([33.70, -0.20, 2.60]),
        pilot_station_point([33.77, 1.05, 3.40]),
        Titanium,
        solid,
        cell,
        PioneerDockedPanelKind::Furniture,
    );
    g.block(
        "pilot-chair/backrest",
        pilot_station_point([33.80, 0.32, 2.70]),
        pilot_station_point([33.86, 1.05, 3.30]),
        Carbon,
        solid,
        cell,
        PioneerDockedPanelKind::Furniture,
    );
    for (i, (z0, z1)) in [(2.60, 2.68), (3.32, 3.40)].into_iter().enumerate() {
        g.block(
            format!("pilot-chair/hinge/{i}"),
            pilot_station_point([33.77, 0.12, z0]),
            pilot_station_point([34.0, 0.32, z1]),
            Titanium,
            solid,
            cell,
            PioneerDockedPanelKind::Furniture,
        );
    }
    // The former wall-mounted back-frame is now carried by a declared deck
    // support; it does not float in the new aft circulation aisle. The support
    // ends before the folded pan's measured minimum X=32.801 m.
    g.block(
        "pilot-chair/deck-support",
        [32.66, FLOOR, 2.60],
        [32.78, -0.19, 3.40],
        Titanium,
        solid,
        cell,
        PioneerDockedPanelKind::Furniture,
    );
    // Index identities mirror the existing six screen slots. Slot zero is
    // transparent telemetry, not a depth-writing console panel.
    for i in 1..6 {
        let (center, half) = cockpit_screen(i).expect("five physical panels");
        g.panel(
            format!("cockpit/screen/{i}/bezel"),
            PioneerDockedBox {
                center_m: center,
                half_m: half,
                axes: [X, Y, Z],
            },
            PioneerDockedMaterial::Emerald,
            solid,
            cell,
            PioneerDockedPanelKind::Instrument,
        );
        g.panel(
            format!("cockpit/screen/{i}/backing"),
            PioneerDockedBox {
                center_m: add(center, [-0.025, 0.0, 0.0]),
                half_m: [0.006, half[1] - 0.008, half[2] - 0.008],
                axes: [X, Y, Z],
            },
            PioneerDockedMaterial::Glass,
            solid,
            cell,
            PioneerDockedPanelKind::Instrument,
        );
    }
}

/// Existing screen slot identities, now fitted to the same physical console.
/// Returns body-space centre and half extents; zero is canopy telemetry only.
pub fn cockpit_screen(index: usize) -> Option<([f64; 3], [f64; 3])> {
    let screen = match index {
        0 => Some(([34.94, 1.112, 3.0], [0.020, 0.075, 0.18])),
        1 => Some(([35.02, 0.77, 2.4], [0.012, 0.13, 0.18])),
        2 => Some(([35.02, 0.77, 3.6], [0.012, 0.13, 0.18])),
        // Raise the central display mount 3 cm so its complete interactive face
        // fits the neutral seated view, preserving the console and pilot eye.
        3 => Some(([35.0, 0.80, 3.0], [0.020, 0.175, 0.27])),
        4 => Some(([35.02, 1.02, 2.4], [0.012, 0.085, 0.18])),
        5 => Some(([35.02, 1.02, 3.6], [0.012, 0.085, 0.18])),
        _ => None,
    };
    screen.map(|(center, half)| (pilot_station_point(center), half))
}

/// Paired inward-opening leaves, shared by rendering, collision and sweep checks.
/// Measured travel has no undeclared sliding pocket; open leaves remain physical.
pub fn hatch_leaf_bounds(
    hatch: PioneerDockedHatch,
    fraction: f64,
) -> Result<[PioneerDockedBox; 2], String> {
    if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
        return Err("Invalid hatch fraction".into());
    }
    let anchors = hatch_anchors(PioneerDockedPose::private());
    let b = match hatch {
        PioneerDockedHatch::Inner => anchors.inner_bounds,
        PioneerDockedHatch::Outer => anchors.outer_bounds,
    };
    let a = b.half_m[0];
    let m = (0.01 + a - 0.005) * 0.5;
    let w = (a - 0.005 - 0.01) * 0.5;
    let (sin, cos) = (fraction * std::f64::consts::FRAC_PI_2).sin_cos();
    Ok([-1.0, 1.0].map(|side| {
        let u = add(mul(b.axes[0], cos), mul(b.axes[2], -side * sin));
        let v = add(mul(b.axes[0], side * sin), mul(b.axes[2], cos));
        let hinge = add(
            b.center_m,
            add(mul(b.axes[0], side * a), mul(b.axes[2], 0.05)),
        );
        PioneerDockedBox {
            center_m: add(hinge, add(mul(u, side * (m - a)), mul(v, -0.05))),
            half_m: [w, b.half_m[1], 0.04],
            axes: [u, Y, v],
        }
    }))
}
/// Conservative boxes enclose every intermediate angle, not only sampled poses.
pub fn hatch_leaf_sweep_bounds(
    hatch: PioneerDockedHatch,
    from: f64,
    to: f64,
) -> Result<Vec<PioneerDockedBox>, String> {
    hatch_leaf_bounds(hatch, from)?;
    hatch_leaf_bounds(hatch, to)?;
    let steps = ((to - from).abs() * 90.0).ceil().max(1.0) as usize;
    let radius: f64 = match hatch {
        PioneerDockedHatch::Inner => 0.597,
        PioneerDockedHatch::Outer => 0.696,
    };
    let delta =
        2.0 * radius * ((to - from).abs() * std::f64::consts::FRAC_PI_2 / steps as f64 / 4.0).sin()
            + 1e-8;
    let mut result = Vec::with_capacity(steps * 2);
    for step in 0..steps {
        let f = from + (to - from) * (step as f64 + 0.5) / steps as f64;
        for mut b in hatch_leaf_bounds(hatch, f)? {
            b.half_m[0] += delta;
            b.half_m[2] += delta;
            result.push(b);
        }
    }
    Ok(result)
}

pub fn dynamic_geometry(
    pose: PioneerDockedPose,
    state: PioneerDockedActuators,
) -> Result<PioneerDockedGeometry, String> {
    let lift = lift_geometry(pose);
    if !pose.valid()
        || !state.lift_floor_station_m.is_finite()
        || state.lift_floor_station_m < -EPS
        || state.lift_floor_station_m > lift.ship_floor_m + EPS
        || !state.chair_fold.is_finite()
        || !(0.0..=1.0).contains(&state.chair_fold)
        || (state.inner_hatch_open && state.outer_hatch_open)
    {
        return Err(
            "Invalid Pioneer docked pose, actuator range, or boarding-hatch interlock".into(),
        );
    }
    let mut g = PioneerDockedGeometry::default();
    let a = hatch_anchors(pose);
    let fractions = state.hatch_fractions.unwrap_or([
        f64::from(state.inner_hatch_open),
        f64::from(state.outer_hatch_open),
    ]);
    for (index, (id, bounds, hatch)) in [
        (
            "boarding/inner-leaf",
            a.inner_bounds,
            PioneerDockedHatch::Inner,
        ),
        (
            "boarding/outer-leaf",
            a.outer_bounds,
            PioneerDockedHatch::Outer,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        for (leaf, bounds) in hatch_leaf_bounds(hatch, fractions[index])?
            .into_iter()
            .enumerate()
        {
            g.panel(
                format!("{id}/{leaf}"),
                bounds,
                PioneerDockedMaterial::Titanium,
                PioneerDockedCollision::Solid,
                PioneerDockedCell::Vestibule,
                PioneerDockedPanelKind::Door,
            );
        }
        if fractions[index] > 0.0 && fractions[index] < 1.0 {
            g.panel(
                format!("{id}/travel-barrier"),
                bounds,
                PioneerDockedMaterial::Graphite,
                PioneerDockedCollision::TraversalBarrier,
                PioneerDockedCell::Vestibule,
                PioneerDockedPanelKind::Door,
            );
        }
    }
    if !state.room_door_open {
        g.block(
            "room/connected/door",
            deck_to_body(1, PIONEER_CONNECTED_ROOM.door_min).unwrap(),
            deck_to_body(1, PIONEER_CONNECTED_ROOM.door_max).unwrap(),
            PioneerDockedMaterial::Graphite,
            PioneerDockedCollision::Solid,
            PioneerDockedCell::Deck1,
            PioneerDockedPanelKind::Door,
        );
    }
    // The pan is a tilted rectangular box: it starts 5 mm below the existing
    // thigh underside and rotates downward into a real unoccupied recess.
    let tilt = (-0.2_f64).atan();
    let fold = state.chair_fold * std::f64::consts::FRAC_PI_2;
    let rotation = tilt - fold;
    let (s, c) = rotation.sin_cos();
    let hinge = pilot_station_point([33.96, 0.265218, 3.0]);
    let length = 0.5_f64.hypot(0.1);
    let local = [length * 0.5, -0.03, 0.0];
    let tilt_axes = [[c, s, 0.0], [-s, c, 0.0], Z];
    let center = add(
        hinge,
        add(mul(tilt_axes[0], local[0]), mul(tilt_axes[1], local[1])),
    );
    g.panel(
        "pilot-chair/pan",
        PioneerDockedBox {
            center_m: center,
            half_m: [length * 0.5, 0.03, 0.28],
            axes: tilt_axes,
        },
        PioneerDockedMaterial::Carbon,
        PioneerDockedCollision::Solid,
        PioneerDockedCell::Cockpit,
        PioneerDockedPanelKind::Furniture,
    );
    let floor = state.lift_floor_station_m - pose.origin_m[1];
    let center = add(at_p(0.0, 2.4), [0.0, floor, 0.0]);
    g.oriented_floor(
        "boarding/lift-floor",
        center,
        [1.0, 0.8],
        [T, N],
        PioneerDockedSupport::BoardingLift,
        PioneerDockedCell::Station,
        [0.38, 0.0],
    );
    for side in [-1.0, 1.0] {
        let x = side * 0.96;
        g.panel(
            format!("boarding/lift-side/{side}"),
            pane_box(
                [x - 0.04, floor + 0.96, 1.64],
                [x + 0.04, floor + 1.04, 3.16],
            ),
            PioneerDockedMaterial::Emerald,
            PioneerDockedCollision::Solid,
            PioneerDockedCell::Station,
            PioneerDockedPanelKind::Rail,
        );
        for n in [1.68, 3.12] {
            g.panel(
                format!("boarding/lift-post/{side}/{n}"),
                pane_box(
                    [x - 0.04, floor, n - 0.04],
                    [x + 0.04, floor + 1.0, n + 0.04],
                ),
                PioneerDockedMaterial::Titanium,
                PioneerDockedCollision::Solid,
                PioneerDockedCell::Station,
                PioneerDockedPanelKind::Lift,
            );
        }
    }
    for (id, n, open) in [
        (
            "ship",
            1.64,
            (state.lift_floor_station_m - lift.ship_floor_m).abs() < 0.01,
        ),
        ("station", 3.16, state.lift_floor_station_m.abs() < 0.01),
    ] {
        if !open {
            g.panel(
                format!("boarding/lift-gate/{id}"),
                pane_box(
                    [-0.92, floor + 0.96, n - 0.04],
                    [0.92, floor + 1.04, n + 0.04],
                ),
                PioneerDockedMaterial::Amber,
                PioneerDockedCollision::Solid,
                PioneerDockedCell::Station,
                PioneerDockedPanelKind::Rail,
            );
        }
    }
    // The existing station slab supplies this walking surface; do not duplicate
    // it with a coplanar rendered floor or waive the station's other obstacles.
    let n0 = if state.lift_floor_station_m.abs() < 0.01 {
        2.4
    } else {
        3.5
    };
    g.surface(
        "boarding/station-approach",
        PioneerDockedSupport::Station,
        add(at_p(0.0, (n0 + 16.0) * 0.5), [0.0, -pose.origin_m[1], 0.0]),
        [0.6, (16.0 - n0) * 0.5],
        [T, N],
        [0.3, 0.0],
    );
    validate_geometry(&g)?;
    Ok(g)
}

fn validate_geometry(g: &PioneerDockedGeometry) -> Result<(), String> {
    let mut ids = std::collections::BTreeSet::new();
    if g.panels.len() > 2048 || g.supports.len() > 256 || g.portals.len() > 64 {
        return Err("Docked geometry exceeds its finite recipe budget".into());
    }
    for p in &g.panels {
        if !p.bounds.valid() || !ids.insert(p.id.as_str()) {
            return Err(format!("Invalid or duplicate docked panel: {}", p.id));
        }
    }
    for s in &g.supports {
        if !finite(s.center_m)
            || s.half_m.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || s.inset_m
                .iter()
                .zip(s.half_m)
                .any(|(a, b)| !a.is_finite() || *a < 0.0 || *a > b + EPS)
            || s.axes
                .iter()
                .any(|a| !finite(*a) || (dot(*a, *a) - 1.0).abs() > 1e-7 || a[1].abs() > EPS)
            || dot(s.axes[0], s.axes[1]).abs() > 1e-7
        {
            return Err(format!("Invalid docked support: {}", s.id));
        }
    }
    Ok(())
}

fn surface_contains(surface: &PioneerDockedSurface, p: [f64; 3]) -> bool {
    let d = sub(p, surface.center_m);
    (0..2).all(|i| {
        // The rounded foot meets a riser before its centre crosses the tread
        // edge. Retain that contact while the authority limits the actual rise.
        let contact = match surface.support {
            PioneerDockedSupport::CoreStairs if i == 1 => STATION_AVATAR_RADIUS_M,
            PioneerDockedSupport::Deck1 | PioneerDockedSupport::Deck2 => STATION_AVATAR_RADIUS_M,
            _ if surface.inset_m[i] == 0.0 && surface.center_m[1] > p[1] + EPS => {
                let rise = surface.center_m[1] - p[1];
                (2.0 * STATION_AVATAR_RADIUS_M * rise - rise * rise)
                    .max(0.0)
                    .sqrt()
            }
            _ => 0.0,
        };
        dot(d, surface.axes[i]).abs() <= surface.half_m[i] - surface.inset_m[i] + contact + EPS
    })
}

pub fn support_at_station(
    static_geometry: &PioneerDockedGeometry,
    dynamic: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    feet_station_m: [f64; 3],
    previous: Option<PioneerDockedSupport>,
    max_step_m: f64,
) -> Option<PioneerDockedSupportHit> {
    if !pose.valid() || !finite(feet_station_m) || !max_step_m.is_finite() || max_step_m < 0.0 {
        return None;
    }
    let feet = station_to_body(pose, feet_station_m);
    let mut best: Option<&PioneerDockedSurface> = None;
    for s in static_geometry.supports.iter().chain(&dynamic.supports) {
        if !surface_contains(s, feet) {
            continue;
        }
        let riding = previous == Some(PioneerDockedSupport::BoardingLift)
            && s.support == PioneerDockedSupport::BoardingLift;
        if !riding && (s.center_m[1] - feet[1]).abs() > max_step_m + EPS {
            continue;
        }
        if riding {
            best = Some(s);
            break;
        }
        if best.is_none_or(|b| s.center_m[1] > b.center_m[1]) {
            best = Some(s);
        }
    }
    best.map(|s| PioneerDockedSupportHit {
        support: s.support,
        surface_id: s.id.clone(),
        height_m: s.center_m[1] + pose.origin_m[1],
    })
}

/// This is a narrowly proved floor approach, not permission to ignore all hull
/// bounds. The authority must also find support and check station obstacles.
pub fn is_registered_station_approach(pose: PioneerDockedPose, feet_station_m: [f64; 3]) -> bool {
    if !pose.valid() || !finite(feet_station_m) || feet_station_m[1].abs() > 0.05 {
        return false;
    }
    let p = sub(station_to_body(pose, feet_station_m), P);
    let t = dot(p, T);
    let n = dot(p, N);
    t.abs() <= 0.3 + EPS && (2.4 - EPS..=16.0 + EPS).contains(&n)
}

pub fn capsule_clear_station(
    static_geometry: &PioneerDockedGeometry,
    dynamic: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    feet_station_m: [f64; 3],
) -> bool {
    if !pose.valid() || !finite(feet_station_m) {
        return false;
    }
    let feet = station_to_body(pose, feet_station_m);
    static_geometry
        .panels
        .iter()
        .chain(&dynamic.panels)
        .all(|p| {
            p.collision == PioneerDockedCollision::Decoration
                || !box_intersects_capsule(p.bounds, feet, AVATAR_HEIGHT_M, STATION_AVATAR_RADIUS_M)
        })
}

pub fn box_intersects_capsule(
    bounds: PioneerDockedBox,
    feet: [f64; 3],
    height: f64,
    radius: f64,
) -> bool {
    if !bounds.valid()
        || !finite(feet)
        || !height.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
        || height < radius * 2.0
    {
        return true;
    }
    let a = bounds.local(add(feet, [0.0, radius, 0.0]));
    let b = bounds.local(add(feet, [0.0, height - radius, 0.0]));
    segment_box_distance2(a, b, bounds.half_m) < radius * radius - EPS
}

/// Earliest hit of a swept camera sphere against the same solid/support boxes.
/// None means the complete segment is clear; a start inside a solid returns 0.
pub fn camera_segment_station(
    static_geometry: &PioneerDockedGeometry,
    dynamic: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    start: [f64; 3],
    end: [f64; 3],
    radius: f64,
) -> Option<f64> {
    if !pose.valid() || !finite(start) || !finite(end) || !radius.is_finite() || radius < 0.0 {
        return Some(0.0);
    }
    let a = station_to_body(pose, start);
    let b = station_to_body(pose, end);
    static_geometry
        .panels
        .iter()
        .chain(&dynamic.panels)
        .filter(|p| p.collision != PioneerDockedCollision::Decoration)
        .filter_map(|p| {
            segment_box_first_hit(
                p.bounds.local(a),
                p.bounds.local(b),
                p.bounds.half_m,
                radius,
            )
        })
        .min_by(f64::total_cmp)
}

fn segment_intervals(a: [f64; 3], b: [f64; 3], half: [f64; 3]) -> Vec<f64> {
    let d = sub(b, a);
    let mut times = vec![0.0, 1.0];
    for i in 0..3 {
        if d[i].abs() > EPS {
            for face in [-half[i], half[i]] {
                let t = (face - a[i]) / d[i];
                if t > 0.0 && t < 1.0 {
                    times.push(t);
                }
            }
        }
    }
    times.sort_by(f64::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < EPS);
    times
}
fn distance_coefficients(a: [f64; 3], d: [f64; 3], half: [f64; 3], time: f64) -> (f64, f64, f64) {
    let mut aa = 0.0;
    let mut bb = 0.0;
    let mut cc = 0.0;
    for i in 0..3 {
        let p = a[i] + d[i] * time;
        let bound = if p < -half[i] {
            -half[i]
        } else if p > half[i] {
            half[i]
        } else {
            continue;
        };
        let v = a[i] - bound;
        aa += d[i] * d[i];
        bb += 2.0 * d[i] * v;
        cc += v * v;
    }
    (aa, bb, cc)
}
fn segment_box_distance2(a: [f64; 3], b: [f64; 3], half: [f64; 3]) -> f64 {
    let d = sub(b, a);
    let times = segment_intervals(a, b, half);
    let mut best = f64::INFINITY;
    for w in times.windows(2) {
        let (aa, bb, cc) = distance_coefficients(a, d, half, (w[0] + w[1]) * 0.5);
        let t = if aa > EPS {
            (-bb / (2.0 * aa)).clamp(w[0], w[1])
        } else {
            w[0]
        };
        best = best.min((aa * t * t + bb * t + cc).max(0.0));
    }
    best
}
fn segment_box_first_hit(a: [f64; 3], b: [f64; 3], half: [f64; 3], radius: f64) -> Option<f64> {
    let d = sub(b, a);
    let times = segment_intervals(a, b, half);
    for w in times.windows(2) {
        let (aa, bb, mut cc) = distance_coefficients(a, d, half, (w[0] + w[1]) * 0.5);
        cc -= radius * radius;
        if aa * w[0] * w[0] + bb * w[0] + cc <= EPS {
            return Some(w[0]);
        }
        if aa > EPS {
            let disc = bb * bb - 4.0 * aa * cc;
            if disc >= 0.0 {
                let t = (-bb - disc.sqrt()) / (2.0 * aa);
                if t >= w[0] - EPS && t <= w[1] + EPS {
                    return Some(t.clamp(0.0, 1.0));
                }
            }
        } else if bb.abs() > EPS {
            let t = -cc / bb;
            if t >= w[0] - EPS && t <= w[1] + EPS {
                return Some(t.clamp(0.0, 1.0));
            }
        }
    }
    None
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn mul(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|v| v * s)
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn finite(a: [f64; 3]) -> bool {
    a.iter().all(|v| v.is_finite())
}
fn normalized(a: [f64; 3]) -> [f64; 3] {
    mul(a, 1.0 / dot(a, a).sqrt())
}
fn lerp(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    add(a, mul(sub(b, a), t))
}
fn finite_fraction(p: f64) -> f64 {
    if p.is_finite() {
        p.clamp(0.0, 1.0)
    } else {
        0.0
    }
}
fn at_p(t: f64, n: f64) -> [f64; 3] {
    add(P, add(mul(T, t), mul(N, n)))
}
fn axis_box(min: [f64; 3], max: [f64; 3]) -> PioneerDockedBox {
    PioneerDockedBox {
        center_m: mul(add(min, max), 0.5),
        half_m: mul(sub(max, min), 0.5),
        axes: [X, Y, Z],
    }
}
fn pane_box(min: [f64; 3], max: [f64; 3]) -> PioneerDockedBox {
    let c = mul(add(min, max), 0.5);
    PioneerDockedBox {
        center_m: add(at_p(c[0], c[2]), [0.0, c[1], 0.0]),
        half_m: mul(sub(max, min), 0.5),
        axes: [T, Y, N],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(pose: PioneerDockedPose) -> PioneerDockedActuators {
        PioneerDockedActuators {
            hatch_fractions: None,
            room_door_open: true,
            inner_hatch_open: true,
            outer_hatch_open: false,
            lift_floor_station_m: lift_geometry(pose).ship_floor_m,
            chair_fold: 1.0,
        }
    }
    fn blockers(
        g: &PioneerDockedGeometry,
        d: &PioneerDockedGeometry,
        feet: [f64; 3],
    ) -> Vec<String> {
        g.panels
            .iter()
            .chain(&d.panels)
            .filter(|p| {
                p.collision != PioneerDockedCollision::Decoration
                    && box_intersects_capsule(
                        p.bounds,
                        feet,
                        AVATAR_HEIGHT_M,
                        STATION_AVATAR_RADIUS_M,
                    )
            })
            .map(|p| p.id.clone())
            .collect()
    }
    fn walk(
        g: &PioneerDockedGeometry,
        d: &PioneerDockedGeometry,
        pose: PioneerDockedPose,
        feet: &mut [f64; 3],
        target: [f64; 2],
        previous: &mut Option<PioneerDockedSupport>,
    ) {
        let start = station_to_body(pose, *feet);
        let count = ((target[0] - start[0]).hypot(target[1] - start[2]) / 0.025)
            .ceil()
            .max(1.0) as usize;
        for i in 1..=count {
            let t = i as f64 / count as f64;
            let old = station_to_body(pose, *feet);
            let body = [
                start[0] + (target[0] - start[0]) * t,
                old[1],
                start[2] + (target[1] - start[2]) * t,
            ];
            let mut next = body_to_station(pose, body);
            let hit = support_at_station(g, d, pose, next, *previous, 0.24)
                .unwrap_or_else(|| panic!("No support at body {body:?}; previous={previous:?}"));
            assert!(
                (hit.height_m - feet[1]).abs() <= 0.24 + 1e-7
                    || *previous == Some(PioneerDockedSupport::BoardingLift)
            );
            next[1] = hit.height_m;
            let b = station_to_body(pose, next);
            assert!(
                capsule_clear_station(g, d, pose, next),
                "Blocked body {b:?}: {:?}",
                blockers(g, d, b)
            );
            *feet = next;
            *previous = Some(hit.support);
        }
    }

    #[test]
    fn transforms_keep_metres_room_documents_and_station_frames_distinct() {
        let source = crate::default_pioneer();
        let before = source.clone();
        let geometry = registered_geometry(&source).unwrap();
        assert_eq!(source, before);
        for pose in [
            PioneerDockedPose::private(),
            PioneerDockedPose::fleet(FleetLayoutRevision::LinearV1),
            PioneerDockedPose::fleet(FleetLayoutRevision::CircularV1),
            PioneerDockedPose {
                origin_m: [123.0, 13.0, -91.0],
                yaw: 1.27,
            },
        ] {
            for p in [[0.0; 3], [34.54, FLOOR, 3.0], [-12.0, -4.12, -7.0]] {
                let back = station_to_body(pose, body_to_station(pose, p));
                assert!((0..3).all(|i| (back[i] - p[i]).abs() < 1e-10));
            }
            assert_eq!(lift_geometry(pose).ship_floor_m, pose.origin_m[1] + FLOOR);
        }
        assert_eq!(deck_to_body(1, [0.0, 0.0, 0.0]), Some([-12.0, -4.12, -7.0]));
        assert_eq!(deck_to_body(2, [32.0, 0.0, 7.0]), Some([20.0, FLOOR, 0.0]));
        assert!(deck_to_body(3, [0.0; 3]).is_none());
        assert!(deck_to_body(1, [f64::NAN, 0.0, 0.0]).is_none());
        assert_eq!(geometry, registered_geometry(&source).unwrap());
        assert!(geometry.panels.len() < 1024 && geometry.supports.len() < 128);
        assert!(geometry.panels.iter().all(|p| p.bounds.valid()));
    }

    #[test]
    fn actual_capsule_walks_both_stair_flights_up_and_down_without_moving_cabinet() {
        let g = registered_geometry(&crate::default_pioneer()).unwrap();
        for pose in [
            PioneerDockedPose::private(),
            PioneerDockedPose::fleet(FleetLayoutRevision::LinearV1),
            PioneerDockedPose::fleet(FleetLayoutRevision::CircularV1),
            PioneerDockedPose {
                origin_m: [70.0, 13.0, -20.0],
                yaw: 0.81,
            },
        ] {
            let d = dynamic_geometry(pose, state(pose)).unwrap();
            let mut feet = body_to_station(pose, deck_to_body(1, [4.8, 0.0, 7.0]).unwrap());
            let mut previous = Some(PioneerDockedSupport::Deck1);
            let route = [
                [2.0, 7.0],
                [2.0, 5.8],
                [0.9, 5.8],
                [0.9, 1.0],
                [3.1, 1.0],
                [3.1, 5.8],
                [2.0, 5.8],
                [2.0, 7.0],
                [4.8, 7.0],
            ];
            for [x, z] in route {
                walk(&g, &d, pose, &mut feet, [x - 12.0, z - 7.0], &mut previous);
            }
            assert!((feet[1] - (pose.origin_m[1] + FLOOR)).abs() < 1e-8);
            for [x, z] in route
                .into_iter()
                .rev()
                .skip(1)
                .chain(std::iter::once([4.8, 7.0]))
            {
                walk(&g, &d, pose, &mut feet, [x - 12.0, z - 7.0], &mut previous);
            }
            assert!((feet[1] - (pose.origin_m[1] - 4.12)).abs() < 1e-8);
        }
        let cabinet = g
            .panels
            .iter()
            .find(|p| p.id == "deck1/access-cabinet")
            .unwrap();
        let expected = axis_box([-11.3, -4.12, -0.55], [-11.0, -2.82, 0.55]);
        assert!((0..3).all(
            |i| (cabinet.bounds.center_m[i] - expected.center_m[i]).abs() < EPS
                && (cabinet.bounds.half_m[i] - expected.half_m[i]).abs() < EPS
        ));
        // The real wells, rather than hidden whole slabs, leave the stair heads
        // clear while preserving ceiling geometry in the adjacent rooms.
        for hole in well_rectangles() {
            let x = (hole[0] + hole[2]) * 0.5 - 12.0;
            let z = (hole[1] + hole[3]) * 0.5 - 7.0;
            assert!(!g
                .panels
                .iter()
                .filter(|p| p.id.starts_with("deck1/ceiling/") || p.id.starts_with("deck2/floor/"))
                .any(|p| {
                    let q = p.bounds.local([x, p.bounds.center_m[1], z]);
                    q[0].abs() < p.bounds.half_m[0] - EPS && q[2].abs() < p.bounds.half_m[2] - EPS
                }));
        }
    }

    #[test]
    fn fitted_cockpit_has_continuous_rear_chair_circulation() {
        let g = registered_geometry(&crate::default_pioneer()).unwrap();
        for pose in [
            PioneerDockedPose::private(),
            PioneerDockedPose::fleet(FleetLayoutRevision::CircularV1),
        ] {
            let d = dynamic_geometry(pose, state(pose)).unwrap();
            let mut feet = seat_anchors(pose).root_station_m;
            let mut previous = Some(PioneerDockedSupport::Cockpit);
            // Walk from the folded seat around its port side, through the aft
            // aisle, around the starboard side, and back to the same seat root.
            // Every 25 mm sample uses full-height capsule and floor collision.
            for target in [
                [33.36, 3.0],
                [33.36, 0.0],
                [32.10, 0.0],
                [32.10, 4.2],
                [33.36, 4.2],
                [33.36, 3.0],
                [33.54, 3.0],
            ] {
                walk(&g, &d, pose, &mut feet, target, &mut previous);
            }
            assert!((0..3).all(|i| (feet[i] - seat_anchors(pose).root_station_m[i]).abs() < 1e-8));
        }
    }

    #[test]
    fn fitted_seat_egress_and_interlocked_boarding_route_have_continuous_support() {
        let g = registered_geometry(&crate::default_pioneer()).unwrap();
        for pose in [
            PioneerDockedPose::private(),
            PioneerDockedPose::fleet(FleetLayoutRevision::LinearV1),
            PioneerDockedPose::fleet(FleetLayoutRevision::CircularV1),
        ] {
            let mut a = state(pose);
            let mut d = dynamic_geometry(pose, a).unwrap();
            let mut feet = seat_anchors(pose).root_station_m;
            let mut previous = Some(PioneerDockedSupport::Cockpit);
            assert!(capsule_clear_station(&g, &d, pose, feet));
            for point in [[33.36, 3.0], [33.36, 0.0], [34.1, -0.5], [34.1, -7.8]] {
                walk(&g, &d, pose, &mut feet, point, &mut previous);
            }
            let anchors = hatch_anchors(pose);
            assert!((0..3).all(|i| (feet[i] - anchors.inner_inside_station_m[i]).abs() < 1e-8));
            let inner_out = station_to_body(pose, anchors.inner_outside_station_m);
            walk(
                &g,
                &d,
                pose,
                &mut feet,
                [inner_out[0], inner_out[2]],
                &mut previous,
            );
            assert!(!hatch_occupied(pose, PioneerDockedHatch::Inner, feet));
            a.inner_hatch_open = false;
            a.outer_hatch_open = true;
            d = dynamic_geometry(pose, a).unwrap();
            let outer_in = station_to_body(pose, anchors.outer_inside_station_m);
            walk(
                &g,
                &d,
                pose,
                &mut feet,
                [outer_in[0], outer_in[2]],
                &mut previous,
            );
            let outer_out = station_to_body(pose, anchors.outer_outside_station_m);
            walk(
                &g,
                &d,
                pose,
                &mut feet,
                [outer_out[0], outer_out[2]],
                &mut previous,
            );
            let center = station_to_body(pose, lift_geometry(pose).center_station_m);
            walk(
                &g,
                &d,
                pose,
                &mut feet,
                [center[0], center[2]],
                &mut previous,
            );
            assert_eq!(previous, Some(PioneerDockedSupport::BoardingLift));
            assert!((feet[1] - lift_geometry(pose).ship_floor_m).abs() < 1e-8);
            a.outer_hatch_open = false;
            for i in 0..=40 {
                a.lift_floor_station_m = lift_geometry(pose).ship_floor_m * (1.0 - i as f64 / 40.0);
                d = dynamic_geometry(pose, a).unwrap();
                let hit = support_at_station(&g, &d, pose, feet, previous, 0.24).unwrap();
                feet[1] = hit.height_m;
                assert_eq!(hit.support, PioneerDockedSupport::BoardingLift);
                assert!(capsule_clear_station(&g, &d, pose, feet));
            }
            let target = at_p(0.0, 16.0);
            walk(
                &g,
                &d,
                pose,
                &mut feet,
                [target[0], target[2]],
                &mut previous,
            );
            assert_eq!(previous, Some(PioneerDockedSupport::Station));
            assert!(feet[1].abs() < EPS && is_registered_station_approach(pose, feet));
            if pose == PioneerDockedPose::fleet(FleetLayoutRevision::LinearV1) {
                assert!(crate::fleet_walkable(FleetLayoutRevision::LinearV1, feet));
            }
        }
    }

    #[test]
    fn door_occupancy_lift_height_and_camera_queries_use_the_same_three_dimensional_boxes() {
        let pose = PioneerDockedPose::private();
        let g = registered_geometry(&crate::default_pioneer()).unwrap();
        let mut a = state(pose);
        a.inner_hatch_open = false;
        let d = dynamic_geometry(pose, a).unwrap();
        let h = hatch_anchors(pose);
        let middle = body_to_station(pose, sub(h.inner_bounds.center_m, [0.0, 1.2, 0.0]));
        assert!(hatch_occupied(pose, PioneerDockedHatch::Inner, middle));
        assert!(!capsule_clear_station(&g, &d, pose, middle));
        let start = add(h.inner_inside_station_m, [0.0, 1.65, 0.0]);
        let end = add(h.inner_outside_station_m, [0.0, 1.65, 0.0]);
        assert!(camera_segment_station(&g, &d, pose, start, end, 0.08).is_some());
        a.inner_hatch_open = true;
        let d = dynamic_geometry(pose, a).unwrap();
        assert!(camera_segment_station(&g, &d, pose, start, end, 0.08).is_none());
        a.outer_hatch_open = true;
        assert!(dynamic_geometry(pose, a).is_err());
        a.outer_hatch_open = false;
        a.lift_floor_station_m = 99.0;
        assert!(dynamic_geometry(pose, a).is_err());
        assert!(!capsule_clear_station(&g, &d, pose, [f64::NAN, 0.0, 0.0]));
        let floor = body_to_station(pose, [0.0, FLOOR, 0.0]);
        assert!(support_at_station(&g, &d, pose, floor, None, 1.5).is_some());
        assert!(support_at_station(&g, &d, pose, floor, None, f64::INFINITY).is_none());
    }

    #[test]
    fn pilot_joints_and_pan_have_fixed_roots_and_a_real_continuous_stow_recess() {
        let pose = PioneerDockedPose::private();
        let g = registered_geometry(&crate::default_pioneer()).unwrap();
        let mut a = state(pose);
        let frame: Vec<_> = g
            .panels
            .iter()
            .filter(|p| p.id.starts_with("pilot-chair/"))
            .collect();
        for i in 0..=1000 {
            a.chair_fold = i as f64 / 1000.0;
            let d = dynamic_geometry(pose, a).unwrap();
            let pan = &d
                .panels
                .iter()
                .find(|p| p.id == "pilot-chair/pan")
                .unwrap()
                .bounds;
            for sx in [-1.0, 1.0] {
                for sy in [-1.0, 1.0] {
                    for sz in [-1.0, 1.0] {
                        let p = add(
                            pan.center_m,
                            add(
                                mul(pan.axes[0], sx * pan.half_m[0]),
                                add(
                                    mul(pan.axes[1], sy * pan.half_m[1]),
                                    mul(pan.axes[2], sz * pan.half_m[2]),
                                ),
                            ),
                        );
                        assert!(p[1] > FLOOR + 0.06);
                        for panel in &frame {
                            let q = panel.bounds.local(p);
                            assert!(
                                !(0..3).all(|axis| q[axis].abs() < panel.bounds.half_m[axis] - EPS),
                                "{} intersects pan at {}",
                                panel.id,
                                a.chair_fold
                            );
                        }
                    }
                }
            }
            let (fold, standing) = seat_transition_pose(true, i as f64 / 1000.0);
            let joints = pilot_joints(standing);
            assert_eq!(joints.root_m, [33.54, FLOOR, 3.0]);
            assert!(standing == 0.0 || (fold - 1.0).abs() < EPS);
            assert!(joints.hands_m.iter().all(|p| p[1] > 0.48));
            let reverse = seat_transition_pose(false, 1.0 - i as f64 / 1000.0);
            assert!((reverse.0 - fold).abs() < EPS && (reverse.1 - standing).abs() < EPS);
        }
        assert_eq!(pilot_joints(0.0).eye_m, [33.22, 1.0, 3.0]);
        assert_eq!(pilot_joints(1.0).eye_m, [33.54, 1.33, 3.0]);
        assert_eq!(
            cockpit_screen(3),
            Some(([34.0, 0.80, 3.0], [0.020, 0.175, 0.27]))
        );
        assert!(cockpit_screen(6).is_none());
    }

    #[test]
    fn capsule_and_camera_use_exact_segment_to_oriented_box_distance() {
        let b = axis_box([-1.0, 0.0, -1.0], [1.0, 1.0, 1.0]);
        assert!(!box_intersects_capsule(b, [1.3, 0.0, 0.0], 1.8, 0.3));
        assert!(box_intersects_capsule(b, [1.29, 0.0, 0.0], 1.8, 0.3));
        assert!(!box_intersects_capsule(b, [0.0, 1.0, 0.0], 1.8, 0.3));
        assert!(!box_intersects_capsule(b, [0.0, 2.0, 0.0], 1.8, 0.3));
        assert!(box_intersects_capsule(b, [0.0, 0.9, 0.0], 1.8, 0.3));
        assert!(
            (segment_box_first_hit([-3.0, 0.0, 0.0], [3.0, 0.0, 0.0], [1.0; 3], 0.2).unwrap()
                - 0.3)
                .abs()
                < 1e-10
        );
        assert!(segment_box_first_hit([-3.0, 2.0, 0.0], [3.0, 2.0, 0.0], [1.0; 3], 0.2).is_none());
        assert!(box_intersects_capsule(b, [0.0; 3], f64::NAN, 0.3));
    }

    #[test]
    fn source_cut_contract_keeps_exact_snapshot_object_and_triangle_bindings() {
        let cuts = pioneer_docked_source_cuts();
        assert_eq!(cuts.len(), 3);
        assert_eq!(
            cuts.iter().map(|c| c.primitive).collect::<Vec<_>>(),
            vec![0, 6, 1]
        );
        assert_eq!(
            cuts[0].affected_triangle_starts,
            &[31368, 31371, 31377, 31383, 31422, 31425, 31437, 31440]
        );
        for c in cuts {
            assert!(c.source_id.starts_with("part:") && c.source_id.len() == 69);
            assert!(c.affected_triangle_starts.iter().all(|first| first % 3 == 0
                && *first >= c.source_first_vertex
                && *first + 3 <= c.source_first_vertex + c.source_vertex_count));
            assert!(c.volumes.iter().all(|b| b.valid()));
        }
    }
}

#[cfg(test)]
mod hatch_motion_geometry_tests {
    use super::*;
    #[test]
    fn paired_leaves_retain_open_geometry_and_conservatively_enclose_every_angle() {
        for hatch in [PioneerDockedHatch::Inner, PioneerDockedHatch::Outer] {
            assert!(hatch_leaf_bounds(hatch, f64::NAN).is_err());
            assert!(hatch_leaf_bounds(hatch, -0.01).is_err());
            let swept = hatch_leaf_sweep_bounds(hatch, 0.0, 1.0).unwrap();
            assert_eq!(swept.len(), 180);
            for tick in 0..=1800 {
                let fraction = tick as f64 / 1800.0;
                for (leaf_index, b) in hatch_leaf_bounds(hatch, fraction)
                    .unwrap()
                    .iter()
                    .enumerate()
                {
                    let i = ((fraction * 90.0).floor() as usize).min(89) * 2 + leaf_index;
                    for x in [-1.0, 1.0] {
                        for y in [-1.0, 1.0] {
                            for z in [-1.0, 1.0] {
                                let point = add(
                                    b.center_m,
                                    add(
                                        mul(b.axes[0], x * b.half_m[0]),
                                        add(
                                            mul(b.axes[1], y * b.half_m[1]),
                                            mul(b.axes[2], z * b.half_m[2]),
                                        ),
                                    ),
                                );
                                let local = swept[i].local(point);
                                assert!((0..3)
                                    .all(|axis| local[axis].abs() <= swept[i].half_m[axis] + 1e-7));
                            }
                        }
                    }
                }
            }
            let open = hatch_leaf_bounds(hatch, 1.0).unwrap();
            assert!(open.iter().all(|b| b.valid()));
            assert_ne!(open, hatch_leaf_bounds(hatch, 0.0).unwrap());
        }
    }
}
