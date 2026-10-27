//! Source-bound presentation vocabulary. A valid sample describes confirmed
//! authority state; it never advances lifecycle or grants physical clearance.
use super::*;
use crate::carrier_inventory_geometry::{
    self as geometry, CarrierInventoryDefinition, CarrierInventoryKeyframe,
};
use crate::drone_bay_geometry::{BayMotionSegment, BayPose};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierPoseKey {
    Drone,
    Cassette { cassette_id: u64 },
    Mechanism { semantic_id: String },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierMotionFrame {
    pub poses: Vec<(CarrierPoseKey, BayPose)>,
}
impl From<&CarrierInventoryKeyframe> for CarrierMotionFrame {
    fn from(f: &CarrierInventoryKeyframe) -> Self {
        Self {
            poses: f
                .drone_pose
                .into_iter()
                .map(|p| (CarrierPoseKey::Drone, p))
                .chain(
                    f.cassette_poses
                        .iter()
                        .map(|(id, p)| (CarrierPoseKey::Cassette { cassette_id: *id }, *p)),
                )
                .chain(f.machinery.iter().map(|(id, p)| {
                    (
                        CarrierPoseKey::Mechanism {
                            semantic_id: id.clone(),
                        },
                        *p,
                    )
                }))
                .collect(),
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierStageSegment {
    #[default]
    Full,
    Head,
    EmptyTail,
}
impl CarrierStageSegment {
    pub fn is_full(&self) -> bool {
        *self == Self::Full
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierRouteKind {
    Stage {
        #[serde(default, skip_serializing_if = "CarrierStageDirection::is_forward")]
        direction: CarrierStageDirection,
        #[serde(default, skip_serializing_if = "CarrierStageSegment::is_full")]
        segment: CarrierStageSegment,
        source_slot: u16,
        destination_berth: u8,
        loaded_cassette: u64,
        empty_cassette: u64,
    },
    Bay {
        berth_id: u8,
        cassette_id: u64,
        action: CarrierInventoryAction,
    },
}
impl CarrierRouteKind {
    pub fn stage_segment(&self) -> Option<CarrierStageSegment> {
        match self {
            Self::Stage { segment, .. } => Some(*segment),
            _ => None,
        }
    }
    pub fn writes_drone(&self) -> bool {
        self.stage_segment() != Some(CarrierStageSegment::EmptyTail)
    }

    pub fn route(
        &self,
        definition: &CarrierInventoryDefinition,
        class: DroneClass,
    ) -> Option<Vec<CarrierInventoryKeyframe>> {
        match *self {
            Self::Stage {
                direction,
                segment,
                source_slot,
                destination_berth,
                loaded_cassette,
                empty_cassette,
            } => match (direction, segment) {
                (CarrierStageDirection::ToProtected, CarrierStageSegment::Full) => definition
                    .reverse_stage_route(
                        class,
                        destination_berth,
                        source_slot,
                        loaded_cassette,
                        empty_cassette,
                    ),
                (CarrierStageDirection::ToProtected, _) => None,
                (CarrierStageDirection::ToReady, CarrierStageSegment::Full) => definition
                    .stage_route(
                        class,
                        source_slot,
                        destination_berth,
                        loaded_cassette,
                        empty_cassette,
                    ),
                (
                    CarrierStageDirection::ToReady,
                    CarrierStageSegment::Head | CarrierStageSegment::EmptyTail,
                ) => {
                    if definition.topology.registration.assembly_revision != 1702 {
                        return None;
                    }
                    let split = definition.stage_handoff_route(
                        class,
                        source_slot,
                        destination_berth,
                        loaded_cassette,
                        empty_cassette,
                    )?;
                    Some(if segment == CarrierStageSegment::Head {
                        split.head
                    } else {
                        split.empty_tail
                    })
                }
            },
            Self::Bay {
                berth_id,
                cassette_id,
                action,
            } => definition.bay_route(berth_id, class, cassette_id, action),
        }
    }
    pub fn resources(
        &self,
        definition: &CarrierInventoryDefinition,
        class: DroneClass,
    ) -> Option<BTreeSet<CarrierResource>> {
        Some(match *self {
            Self::Stage {
                direction,
                segment,
                source_slot,
                destination_berth,
                loaded_cassette,
                empty_cassette,
            } => {
                let loader =
                    definition
                        .topology
                        .stage_loader(source_slot, destination_berth, class)?;
                CarrierStageExchange {
                    direction,
                    source_slot_id: source_slot,
                    destination_berth_id: destination_berth,
                    loaded_cassette_id: loaded_cassette,
                    empty_cassette_id: empty_cassette,
                    loader_id: loader,
                    buffer_id: definition.topology.loader(loader)?.transit_buffer_id,
                    completed_legs: 0,
                    loaded_handoff_at_ms: (segment == CarrierStageSegment::EmptyTail).then_some(0),
                }
                .retained_resources()
                .into_iter()
                .collect()
            }
            Self::Bay {
                berth_id,
                cassette_id,
                ..
            } => [
                CarrierResource::ReadyBerth { berth_id },
                CarrierResource::Cassette { cassette_id },
            ]
            .into_iter()
            .collect(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierInventoryAdmission {
    /// Exact reviewed stationary documentary fixture, not a playable/travel capability.
    StationaryFixture,
    Enabled,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierPresentationRoute {
    Hardware {
        route: CarrierRouteKind,
        target_index: usize,
    },
    /// Accepted exterior path to the exact source capture entry. Translation and
    /// stationary attitude alignment are separate; no hardware is moved here.
    Approach {
        berth_id: u8,
        cassette_id: u64,
        path: Vec<BayPose>,
        target_index: usize,
    },
    /// Fixed-attitude source-clearance-to-stand-off path, retained after launch.
    Departure { berth_id: u8, cassette_id: u64 },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierInventoryMotionView {
    pub operation_id: u64,
    pub drone_id: u64,
    pub route: CarrierPresentationRoute,
    pub source: CarrierMotionFrame,
    pub confirmed: CarrierMotionFrame,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub confirmed_at_ms: u64,
    pub held: bool,
    pub hold_reason: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierInventoryPublicState {
    pub sample_time_ms: u64,
    pub parent: BayPose,
    /// Actual source-mounted cameras, explicitly unmetered and not damage-sensed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cameras: Option<crate::carrier_camera_devices::CarrierCameraPublicState>,
    /// Explicit finite IDs and physical locations, not an inferred class quota.
    pub inventory: CarrierInventoryState,
    pub ore: CarrierOreState,
    /// Actual free-flight obstruction, not a timed estimate or damage sensor.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mining_blocks: Vec<(u64, String)>,
    pub drone_poses: Vec<(u64, BayPose)>,
    pub cassette_poses: Vec<(u64, BayPose)>,
    pub machinery: Vec<(String, BayPose)>,
    pub motions: Vec<CarrierInventoryMotionView>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CarrierInventoryNextIds {
    pub operation_id: u64,
    pub deployment_id: u64,
}
fn fail(message: &str) -> String {
    message.to_owned()
}
// Free flight performs a world->body->world transform; composing unit
// quaternions normalizes again. Compare that representation only within a
// floating-point roundoff budget, accepting q/-q, never a different attitude.
pub fn carrier_composed_attitude_matches(a: [f64; 4], b: [f64; 4]) -> bool {
    if ![a, b].into_iter().all(|q| {
        BayPose {
            position_m: [0.; 3],
            orientation_xyzw: q,
        }
        .valid()
    }) {
        return false;
    }
    let unit = |q: [f64; 4]| {
        let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        q.map(|v| v / n)
    };
    let a = unit(a);
    let b = unit(b);
    [1., -1.]
        .into_iter()
        .any(|sign| (0..4).all(|i| (a[i] - sign * b[i]).abs() <= 64. * f64::EPSILON))
}
fn same_attitude(a: BayPose, b: BayPose) -> bool {
    a.orientation_xyzw == b.orientation_xyzw || a.orientation_xyzw == b.orientation_xyzw.map(|v| -v)
}
fn valid_frame(f: &CarrierMotionFrame) -> bool {
    let mut keys = BTreeSet::new();
    !f.poses.is_empty()
        && f.poses.iter().all(|(k, p)| {
            keys.insert(k) && p.valid() && p.position_m.iter().all(|v| v.abs() <= 1_000_000.)
        })
}
/// A shared interpolation fraction is required for EVERY owned root, not merely
/// each root lying somewhere on its individual trajectory.
fn on_source_segment(
    f: &CarrierMotionFrame,
    a: &CarrierMotionFrame,
    b: &CarrierMotionFrame,
) -> bool {
    if !valid_frame(f)
        || !valid_frame(a)
        || !valid_frame(b)
        || f.poses.len() != a.poses.len()
        || a.poses.len() != b.poses.len()
    {
        return false;
    }
    let mut selected = (0f64, 0f64);
    for (((id, p), (aid, a)), (bid, b)) in f.poses.iter().zip(&a.poses).zip(&b.poses) {
        if id != aid || id != bid || !same_attitude(*p, *a) || !same_attitude(*a, *b) {
            return false;
        }
        for i in 0..3 {
            let delta = b.position_m[i] - a.position_m[i];
            if delta.abs() > selected.0 {
                selected = (delta.abs(), (p.position_m[i] - a.position_m[i]) / delta)
            }
        }
    }
    let t = selected.1;
    t.is_finite()
        && (-1e-10..=1. + 1e-10).contains(&t)
        && f.poses
            .iter()
            .zip(&a.poses)
            .zip(&b.poses)
            .all(|(((_, p), (_, a)), (_, b))| {
                (0..3).all(|i| {
                    (p.position_m[i] - (a.position_m[i] + (b.position_m[i] - a.position_m[i]) * t))
                        .abs()
                        <= 1e-8
                })
            })
}
// Complete corner-radius inflation covers stationary alignment as well as all
// fixed-attitude translations. Closed slab intersection rejects tangency.
fn exterior_path_clear(path: &[BayPose], bounds: [[f64; 3]; 2]) -> bool {
    let radius = 2.5f64.hypot(0.9).hypot(3.) + 0.25;
    let blocked = [bounds[0].map(|v| v - radius), bounds[1].map(|v| v + radius)];
    if !path.iter().all(|p| {
        p.valid()
            && p.position_m.iter().all(|x| x.abs() <= 1e6)
            && (0..3).any(|i| p.position_m[i] < blocked[0][i] || p.position_m[i] > blocked[1][i])
    }) {
        return false;
    }
    path.windows(2).all(|pair| {
        let [a, b] = [pair[0], pair[1]];
        if !same_attitude(a, b) {
            return a.position_m == b.position_m;
        }
        let (mut lo, mut hi) = (0f64, 1f64);
        for i in 0..3 {
            let delta = b.position_m[i] - a.position_m[i];
            if delta == 0. {
                if a.position_m[i] < blocked[0][i] || a.position_m[i] > blocked[1][i] {
                    return true;
                }
            } else {
                let x = (blocked[0][i] - a.position_m[i]) / delta;
                let y = (blocked[1][i] - a.position_m[i]) / delta;
                lo = lo.max(x.min(y));
                hi = hi.min(x.max(y));
                if lo > hi {
                    return true;
                }
            }
        }
        false
    })
}
impl CarrierInventoryPublicState {
    pub fn binding(&self) -> &CarrierInventoryBinding {
        &self.inventory.binding
    }
    pub fn source_admission(&self) -> Option<CarrierInventoryAdmission> {
        if geometry::lookup(&self.binding().registration).is_some() {
            Some(CarrierInventoryAdmission::Enabled)
        } else {
            geometry::stationary_fixture_for_registration(&self.binding().registration)
                .map(|_| CarrierInventoryAdmission::StationaryFixture)
        }
    }
    fn definition(&self) -> Option<&'static CarrierInventoryDefinition> {
        geometry::lookup(&self.binding().registration)
            .or_else(|| geometry::stationary_fixture_for_registration(&self.binding().registration))
    }
    /// Owner-private identity must match exactly. A valid documentary fixture is
    /// inspectable but cannot advertise a normal playable command capability.
    pub fn validated_next_ids(
        &self,
        private: &CarrierInventoryState,
        pilot: &crate::PilotView,
    ) -> Option<CarrierInventoryNextIds> {
        if self.validate_projection(pilot).ok()? != CarrierInventoryAdmission::Enabled
            || private != &self.inventory
        {
            return None;
        }
        Some(CarrierInventoryNextIds {
            operation_id: private.next_operation_id()?,
            deployment_id: private.next_deployment_id()?,
        })
    }
    pub fn validate_projection(
        &self,
        pilot: &crate::PilotView,
    ) -> Result<CarrierInventoryAdmission, String> {
        let mut blocked = BTreeSet::new();
        for (id, reason) in &self.mining_blocks {
            if !blocked.insert(*id)
                || reason.is_empty()
                || reason.len() > 512
                || !self.inventory.entry(*id).is_some_and(|e| {
                    e.class == DroneClass::Mining && e.location == CarrierDroneLocation::Deployed
                })
                || !pilot
                    .drones
                    .iter()
                    .any(|d| d.id == *id && d.mode == crate::DroneMode::Hold)
            {
                return Err(fail("Invalid mining blockage identity/state"));
            }
        }
        let admission = self
            .source_admission()
            .ok_or_else(|| fail("Unregistered carrier source"))?;
        let def = self
            .definition()
            .ok_or_else(|| fail("Carrier definition missing"))?;
        let flight = pilot
            .flight
            .as_ref()
            .ok_or_else(|| fail("Physical carrier parent missing"))?;
        let host = flight
            .docking
            .as_ref()
            .ok_or_else(|| fail("Registered carrier vessel missing"))?;
        if pilot.ship_class != crate::ShipClass::Pioneer
            || host.version != crate::DOCKING_VERSION
            || host.host_vessel_id != self.binding().vessel_id
            || !self.binding().well_formed()
            || self.parent
                != (BayPose {
                    position_m: flight.position,
                    orientation_xyzw: flight.orientation,
                })
            || !self.parent.valid()
            || flight.mode != crate::NavigationMode::Stop
            || flight.velocity != [0.; 3]
            || flight.acceleration != [0.; 3]
            || flight.angular_velocity != [0.; 3]
            || pilot.drone_bay.is_some()
            || pilot.drone_cargo.is_some()
        {
            return Err(fail("Carrier source/actual stationary vessel mismatch"));
        }
        if !self.inventory.validate_shape(&def.topology)
            || !self.ore.validate_shape(&self.inventory)
            || self.sample_time_ms < self.inventory.initialized_at_ms
            || self
                .ore
                .available_ship_headroom(pilot.ore, crate::cargo_capacity(pilot.ship_class))
                .is_none()
        {
            return Err(fail("Finite inventory/cargo ownership or headroom invalid"));
        }
        let drones: BTreeMap<_, _> = self.drone_poses.iter().copied().collect();
        let cassettes: BTreeMap<_, _> = self.cassette_poses.iter().copied().collect();
        let roots: BTreeMap<_, _> = self
            .machinery
            .iter()
            .map(|(id, p)| (id.as_str(), *p))
            .collect();
        if drones.len() != self.drone_poses.len()
            || drones.len() != self.inventory.entries.len()
            || cassettes.len() != self.cassette_poses.len()
            || cassettes.len() != def.cassettes.len()
            || roots.len() != self.machinery.len()
            || roots.len() != def.mechanisms.len()
            || !drones
                .values()
                .chain(cassettes.values())
                .chain(roots.values())
                .all(|p| p.valid() && p.position_m.iter().all(|v| v.abs() <= 1_000_000.))
        {
            return Err(fail("Missing, duplicate or invalid physical roots"));
        }
        if let Some(cameras) = &self.cameras {
            let camera_def = crate::carrier_camera_devices::for_exact_source(
                self.binding().registration.assembly_revision,
                &self.binding().registration.assembly_source_sha256,
            )
            .ok_or_else(|| fail("Camera source is not registered"))?;
            let machinery = self.machinery.iter().cloned().collect();
            let expected = crate::carrier_camera_devices::project(
                camera_def,
                &self.binding().vessel_id,
                self.parent,
                &machinery,
                pilot.power.as_ref(),
                self.sample_time_ms,
            )?;
            if cameras != &expected {
                return Err(fail(
                    "Camera source, clock, current parent or actual supply mismatch",
                ));
            }
        }
        let idle = def
            .idle_roots(&self.cassette_poses)
            .ok_or_else(|| fail("Cassette source identity mismatch"))?;
        for c in &self.inventory.cassettes {
            let source = def
                .cassettes
                .iter()
                .find(|s| s.cassette_id == c.cassette_id)
                .ok_or_else(|| fail("Foreign cassette identity"))?;
            let pose = *cassettes
                .get(&c.cassette_id)
                .ok_or_else(|| fail("Cassette pose missing"))?;
            if pose.orientation_xyzw != source.reference_pose.orientation_xyzw {
                return Err(fail("Cassette source attitude changed"));
            }
            let reachable = |dock| def.topology.same_dock_component(source.initial_dock, dock);
            if !match c.location {
                CarrierCassetteLocation::Docked { dock } => reachable(dock),
                CarrierCassetteLocation::InLoader { from, to, .. } => {
                    reachable(from) && reachable(to)
                }
            } {
                return Err(fail(
                    "Cassette crossed unsupported physical loader component",
                ));
            }
            if let CarrierCassetteLocation::Docked { dock } = c.location {
                if def
                    .docks
                    .iter()
                    .find(|d| d.dock == dock)
                    .is_none_or(|d| d.cassette_pose != pose)
                {
                    return Err(fail("Cassette not at its actual supported dock"));
                }
            }
        }
        let mut active_ids = BTreeSet::new();
        for e in &self.inventory.entries {
            let pose = *drones
                .get(&e.drone_id)
                .ok_or_else(|| fail("Drone physical pose missing"))?;
            if matches!(
                e.location,
                CarrierDroneLocation::Launching { .. }
                    | CarrierDroneLocation::Returning { .. }
                    | CarrierDroneLocation::Servicing { .. }
                    | CarrierDroneLocation::Deployed
            ) {
                active_ids.insert(e.drone_id);
            }
            if matches!(
                e.location,
                CarrierDroneLocation::Ready { .. }
                    | CarrierDroneLocation::Protected { .. }
                    | CarrierDroneLocation::Loading { .. }
            ) {
                let c = self
                    .inventory
                    .cassettes
                    .iter()
                    .find(|c| c.drone_id == Some(e.drone_id))
                    .ok_or_else(|| fail("Stored/loaded body lacks its cassette"))?;
                if cassettes.get(&c.cassette_id) != Some(&pose) {
                    return Err(fail("Stored/loaded body detached from actual cassette"));
                }
            }
        }
        if pilot.drones.len() != active_ids.len() {
            return Err(fail(
                "Actual active fleet differs from finite physical locations",
            ));
        }
        let mut seen = BTreeSet::new();
        for d in &pilot.drones {
            let e = self
                .inventory
                .entry(d.id)
                .ok_or_else(|| fail("Foreign actual drone"))?;
            let pose = self
                .parent
                .compose(drones[&d.id])
                .ok_or_else(|| fail("Invalid actual body transform"))?;
            let error = 1e-7
                + 128.
                    * f64::EPSILON
                    * self
                        .parent
                        .position_m
                        .iter()
                        .map(|v| v.abs())
                        .fold(1., f64::max);
            if !active_ids.contains(&d.id)
                || !seen.insert(d.id)
                || d.class != e.class
                || d.hit_points != e.hit_points
                || !d.orientation_xyzw.is_some_and(|q| {
                    if (e.location == CarrierDroneLocation::Deployed
                        && !self.motions.iter().any(|m| m.drone_id == d.id))
                        || self.motions.iter().any(|m| {
                            m.drone_id == d.id
                                && matches!(m.route, CarrierPresentationRoute::Approach { .. })
                        })
                    {
                        carrier_composed_attitude_matches(q, pose.orientation_xyzw)
                    } else {
                        q == pose.orientation_xyzw
                    }
                })
                || (0..3).any(|i| (d.position[i] - pose.position_m[i]).abs() > error)
            {
                return Err(fail("Actual fleet identity/HP/full pose mismatch"));
            }
        }
        let mut owned = BTreeSet::new();
        let mut motion_ids = BTreeSet::new();
        let mut motion_drones = BTreeSet::new();
        for m in &self.motions {
            let writes_drone = m
                .confirmed
                .poses
                .iter()
                .any(|(k, _)| *k == CarrierPoseKey::Drone);
            if !motion_ids.insert(m.operation_id)
                || (writes_drone && !motion_drones.insert(m.drone_id))
            {
                return Err(fail("Duplicate physical motion owner"));
            }
            let op = self
                .inventory
                .operation(m.operation_id)
                .ok_or_else(|| fail("Orphan motion receipt"))?;
            let e = self
                .inventory
                .entry(m.drone_id)
                .ok_or_else(|| fail("Orphan motion identity"))?;
            if op.drone_id != m.drone_id
                || m.confirmed_at_ms > self.sample_time_ms
                || (!m.held && m.confirmed_at_ms != self.sample_time_ms)
                || m.held == m.hold_reason.is_empty()
                || m.hold_reason.len() > 512
            {
                return Err(fail("Motion owner/held clock mismatch"));
            }
            let target = self.motion_target(def, m)?;
            let finish = m
                .started_at_ms
                .checked_add(m.duration_ms)
                .ok_or_else(|| fail("Motion clock overflow"))?;
            if m.duration_ms == 0
                || m.started_at_ms > m.confirmed_at_ms
                || m.confirmed_at_ms > finish
                || !valid_frame(&m.source)
                || !valid_frame(&m.confirmed)
                || m.source.poses.len() != target.poses.len()
                || m.confirmed.poses.len() != target.poses.len()
            {
                return Err(fail("Invalid common motion frame/clock"));
            }
            for (((key, source), (other, target)), (confirmed_key, confirmed)) in m
                .source
                .poses
                .iter()
                .zip(&target.poses)
                .zip(&m.confirmed.poses)
            {
                let pose = BayMotionSegment {
                    source: *source,
                    target: *target,
                    started_at_ms: m.started_at_ms,
                    duration_ms: m.duration_ms,
                }
                .sample(m.confirmed_at_ms)
                .ok_or_else(|| fail("Invalid source segment"))?
                .0;
                if key != other || key != confirmed_key || pose != *confirmed {
                    return Err(fail("Motion roots do not share the confirmed clock"));
                }
                let global = match key {
                    CarrierPoseKey::Drone => drones.get(&m.drone_id),
                    CarrierPoseKey::Cassette { cassette_id } => cassettes.get(cassette_id),
                    CarrierPoseKey::Mechanism { semantic_id } => roots.get(semantic_id.as_str()),
                };
                if global != Some(confirmed) {
                    return Err(fail("Motion differs from global confirmed roots"));
                }
                if !matches!(key, CarrierPoseKey::Drone) && !owned.insert(key.clone()) {
                    return Err(fail("Two operations write one physical root"));
                }
            }
            let cleanup = self
                .inventory
                .cleanups
                .iter()
                .find(|c| c.operation_id == op.operation_id);
            if matches!(m.route, CarrierPresentationRoute::Hardware { .. })
                && (cleanup
                    .map(|c| c.held)
                    .unwrap_or(op.phase == CarrierOperationPhase::Held)
                    != m.held
                    || (op.phase == CarrierOperationPhase::Destroyed) != cleanup.is_some()
                    || (op.phase.terminal() && cleanup.is_none()))
            {
                return Err(fail("Hardware cleanup/active ownership mismatch"));
            }
            if matches!(e.location, CarrierDroneLocation::Destroyed { .. })
                && cleanup.is_none()
                && !op.released_loaded_owner()
            {
                return Err(fail("Destroyed motion lacks retained cleanup"));
            }
        }
        for op in &self.inventory.operations {
            let required = (!op.phase.terminal()
                && op.effective_phase() != CarrierOperationPhase::Queued)
                || self
                    .inventory
                    .cleanups
                    .iter()
                    .any(|c| c.operation_id == op.operation_id);
            if required && !motion_ids.contains(&op.operation_id) {
                return Err(fail("Accepted operation lacks physical motion"));
            }
        }
        for (id, p) in idle {
            if !owned.contains(&CarrierPoseKey::Mechanism {
                semantic_id: id.clone(),
            }) && roots.get(id.as_str()) != Some(&p)
            {
                return Err(fail("Unowned hardware is not source-safe"));
            }
        }
        Ok(admission)
    }
    fn motion_target(
        &self,
        def: &CarrierInventoryDefinition,
        m: &CarrierInventoryMotionView,
    ) -> Result<CarrierMotionFrame, String> {
        let op = self
            .inventory
            .operation(m.operation_id)
            .ok_or_else(|| fail("Missing motion operation"))?;
        let e = self
            .inventory
            .entry(m.drone_id)
            .ok_or_else(|| fail("Missing motion drone"))?;
        match &m.route {
            CarrierPresentationRoute::Hardware {
                route,
                target_index,
            } => {
                let matches = match route {
                    CarrierRouteKind::Stage {
                        direction,
                        segment,
                        source_slot,
                        destination_berth,
                        loaded_cassette,
                        empty_cassette,
                    } => op.stage.as_ref().is_some_and(|x| {
                        x.source_slot_id == *source_slot
                            && x.direction == *direction
                            && match segment {
                                CarrierStageSegment::Full => {
                                    x.loaded_handoff_at_ms.is_none()
                                        && (!direction.is_forward()
                                            || self.binding().registration.assembly_revision
                                                != 1702
                                            || op.phase == CarrierOperationPhase::Destroyed)
                                }
                                CarrierStageSegment::Head => {
                                    x.loaded_handoff_at_ms.is_none()
                                        && direction.is_forward()
                                        && self.binding().registration.assembly_revision == 1702
                                }
                                CarrierStageSegment::EmptyTail => x.loaded_handoff_at_ms.is_some(),
                            }
                            && x.destination_berth_id == *destination_berth
                            && x.loaded_cassette_id == *loaded_cassette
                            && x.empty_cassette_id == *empty_cassette
                    }),
                    CarrierRouteKind::Bay {
                        berth_id,
                        cassette_id,
                        action,
                    } => {
                        op.stage.is_none()
                            && op.action == *action
                            && op.berth_id == Some(*berth_id)
                            && self
                                .inventory
                                .docked_cassette(CarrierCassetteDock::Ready {
                                    berth_id: *berth_id,
                                })
                                .is_some_and(|c| c.cassette_id == *cassette_id)
                    }
                };
                if !matches {
                    return Err(fail("Motion route differs from retained resource owner"));
                }
                let path = route
                    .route(def, e.class)
                    .ok_or_else(|| fail("Unproved physical source route"))?;
                if *target_index == 0 || *target_index >= path.len() {
                    return Err(fail("Invalid source route index"));
                }
                if op.stage.as_ref().is_some_and(|x| {
                    x.completed_legs != path[*target_index - 1].completed_stage_legs
                        || x.loaded_handoff_at_ms
                            .is_some_and(|at| at > m.started_at_ms)
                }) {
                    return Err(fail("Stage progress/handoff differs from source clock"));
                }
                let a = CarrierMotionFrame::from(&path[*target_index - 1]);
                let b = CarrierMotionFrame::from(&path[*target_index]);
                if !on_source_segment(&m.source, &a, &b)
                    || (op.phase != CarrierOperationPhase::Destroyed
                        && op.effective_phase() != path[*target_index].phase)
                {
                    return Err(fail("Source root/phase differs from accepted route"));
                }
                Ok(b)
            }
            CarrierPresentationRoute::Approach {
                berth_id,
                cassette_id,
                path,
                target_index,
            } => {
                if !matches!(
                    op.action,
                    CarrierInventoryAction::RecallToStorage
                        | CarrierInventoryAction::ReturnToUnload { .. }
                ) || op.berth_id != Some(*berth_id)
                    || op.effective_phase() != CarrierOperationPhase::Returning
                    || (op.phase == CarrierOperationPhase::Held) != m.held
                    || !matches!(e.location,CarrierDroneLocation::Returning{operation_id,berth_id:b} if operation_id==op.operation_id && b==*berth_id)
                    || self
                        .inventory
                        .docked_cassette(CarrierCassetteDock::Ready {
                            berth_id: *berth_id,
                        })
                        .is_none_or(|c| c.cassette_id != *cassette_id || c.drone_id.is_some())
                    || path.len() < 2
                    || path.len() > 900
                    || *target_index == 0
                    || *target_index >= path.len()
                {
                    return Err(fail("Exterior capture owner/path mismatch"));
                }
                let canonical = def
                    .bay_route(*berth_id, e.class, *cassette_id, op.action)
                    .ok_or_else(|| fail("Capture source unavailable"))?;
                if path.last().copied() != canonical[0].drone_pose
                    || !exterior_path_clear(path, def.deployed_hardware_bounds_m)
                {
                    return Err(fail(
                        "Approach crosses complete carrier or misses exact entry",
                    ));
                }
                let expected = CarrierMotionFrame {
                    poses: vec![(CarrierPoseKey::Drone, path[*target_index - 1])],
                };
                if m.source != expected {
                    return Err(fail("Approach differs from retained segment"));
                }
                Ok(CarrierMotionFrame {
                    poses: vec![(CarrierPoseKey::Drone, path[*target_index])],
                })
            }
            CarrierPresentationRoute::Departure {
                berth_id,
                cassette_id,
            } => {
                if op.phase != CarrierOperationPhase::Completed
                    || op.action != CarrierInventoryAction::Launch
                    || op.berth_id != Some(*berth_id)
                    || !matches!(e.location, CarrierDroneLocation::Deployed)
                {
                    return Err(fail("Exterior departure owner mismatch"));
                }
                let path = def
                    .bay_route(
                        *berth_id,
                        e.class,
                        *cassette_id,
                        CarrierInventoryAction::Launch,
                    )
                    .ok_or_else(|| fail("Unknown departure source"))?;
                let initial = path
                    .last()
                    .and_then(|f| f.drone_pose)
                    .ok_or_else(|| fail("Missing exterior clear pose"))?;
                let ordinal = self
                    .inventory
                    .operations
                    .iter()
                    .filter(|o| {
                        o.operation_id <= op.operation_id
                            && o.action == CarrierInventoryAction::Launch
                            && o.berth_id == op.berth_id
                    })
                    .count();
                if ordinal == 0 || ordinal > self.inventory.roster.len() {
                    return Err(fail("No source-qualified departure slot"));
                }
                let distance = 40. + 12. * (self.inventory.roster.len() + 1 - ordinal) as f64;
                let offset = initial
                    .transform_vector([distance, 0., 0.])
                    .ok_or_else(|| fail("Invalid departure attitude"))?;
                let target = BayPose {
                    position_m: std::array::from_fn(|i| initial.position_m[i] + offset[i]),
                    ..initial
                };
                let a = CarrierMotionFrame {
                    poses: vec![(CarrierPoseKey::Drone, initial)],
                };
                let b = CarrierMotionFrame {
                    poses: vec![(CarrierPoseKey::Drone, target)],
                };
                if !on_source_segment(&m.source, &a, &b) {
                    return Err(fail("Departure leaves its source-bound exterior axis"));
                }
                Ok(b)
            }
        }
    }
    /// Rendering may sample only validated accepted segments and must clamp to the
    /// authoritative confirmed clock. It cannot extrapolate lifecycle or clearance.
    pub fn confirmed_pose(&self, drone_id: u64) -> Option<BayPose> {
        self.inventory
            .entry(drone_id)
            .filter(|e| !matches!(e.location, CarrierDroneLocation::Destroyed { .. }))?;
        self.drone_poses
            .iter()
            .find(|(id, _)| *id == drone_id)
            .map(|(_, p)| *p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn free_attitude_accepts_only_normalization_roundoff_and_quaternion_sign() {
        let q = [0.123456789, 0.234567891, 0.345678912, 0.9];
        let n = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        let q = q.map(|v| v / n);
        assert!(carrier_composed_attitude_matches(q, q.map(|v| -v)));
        assert!(carrier_composed_attitude_matches(
            q,
            q.map(|v| v * (1. + 2. * f64::EPSILON))
        ));
        for bad in [[0.; 4], [0., 0., 0., 2.], [f64::INFINITY, 0., 0., 1.]] {
            assert!(!carrier_composed_attitude_matches(q, bad));
        }
        let mut wrong = q;
        wrong[0] += 1e-10;
        assert!(!carrier_composed_attitude_matches(q, wrong));
        assert!(!carrier_composed_attitude_matches(
            q,
            [f64::NAN, 0., 0., 1.]
        ));
    }
    #[test]
    fn exterior_capture_projection_rejects_cross_hull_and_combined_rotation() {
        let pose = |position_m| BayPose {
            position_m,
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let bounds = [[-5.; 3], [5.; 3]];
        assert!(!exterior_path_clear(
            &[pose([-20., 0., 0.]), pose([20., 0., 0.])],
            bounds
        ));
        assert!(exterior_path_clear(
            &[
                pose([-20., 0., 0.]),
                pose([-20., 20., 0.]),
                pose([20., 20., 0.]),
                pose([20., 0., 0.])
            ],
            bounds
        ));
        let rotated = BayPose {
            orientation_xyzw: [0., 1., 0., 0.],
            ..pose([20., 0., 0.])
        };
        assert!(exterior_path_clear(&[pose([20., 0., 0.]), rotated], bounds));
        assert!(!exterior_path_clear(
            &[pose([21., 0., 0.]), rotated],
            bounds
        ));
        assert!(!exterior_path_clear(&[pose([0.; 3]), rotated], bounds));
    }

    fn fixture() -> (crate::PilotView, CarrierInventoryPublicState) {
        let def = geometry::stationary_fixture_definition().unwrap();
        let (_, mut inventory) = super::super::tests::fixture();
        inventory.binding = def.topology.registration.bind("fixture-vessel".into());
        let cassette_poses = def
            .cassettes
            .iter()
            .map(|c| (c.cassette_id, c.reference_pose))
            .collect::<Vec<_>>();
        let poses: BTreeMap<_, _> = cassette_poses.iter().copied().collect();
        let drone_poses = inventory
            .cassettes
            .iter()
            .map(|c| (c.drone_id.unwrap(), poses[&c.cassette_id]))
            .collect();
        let ore = CarrierOreState {
            binding: inventory.binding.clone(),
            holds: inventory
                .roster
                .iter()
                .map(|i| crate::DroneOreHold {
                    drone_id: i.drone_id,
                    ore_units: 0,
                    destroyed_at_ms: None,
                    intent_generation: 0,
                    mining_intent: None,
                    cycle_high_watermark: 0,
                    cycle: None,
                })
                .collect(),
            lost_ore_units: 0,
            service_high_watermark: 0,
            services: vec![],
            waiting: vec![],
            granted_service_id: None,
            last_committed_at_ms: None,
        };
        let pilot=serde_json::from_value(serde_json::json!({"name":"Pilot","credits":1000,"ore":0,"location":{"kind":"station"},"ship_class":"pioneer","system_id":1,"flight":{"position":[0,0,0],"velocity":[0,0,0],"acceleration":[0,0,0],"heading":[1,0,0],"orientation":[0,0,0,1],"angular_velocity":[0,0,0],"mode":{"mode":"stop"},"updated_at_ms":1000,"warp":null,"docking":{"version":1,"host_vessel_id":"fixture-vessel","berth_id":0,"phase":"free","operation_id":0,"clamps":0,"departing":false,"updated_at_ms":1000}}})).unwrap();
        let p = CarrierInventoryPublicState {
            sample_time_ms: 1000,
            cameras: None,
            parent: BayPose {
                position_m: [0.; 3],
                orientation_xyzw: [0., 0., 0., 1.],
            },
            inventory,
            ore,
            mining_blocks: vec![],
            drone_poses,
            machinery: def.idle_roots(&cassette_poses).unwrap(),
            cassette_poses,
            motions: vec![],
        };
        (pilot, p)
    }
    fn moving() -> (crate::PilotView, CarrierInventoryPublicState) {
        let (mut pilot, mut p) = fixture();
        let def = p.definition().unwrap();
        let route = CarrierRouteKind::Bay {
            berth_id: 0,
            cassette_id: 0,
            action: CarrierInventoryAction::Launch,
        };
        let path = route.route(def, DroneClass::Mining).unwrap();
        let source = CarrierMotionFrame::from(&path[0]);
        let target = CarrierMotionFrame::from(&path[1]);
        let confirmed = CarrierMotionFrame {
            poses: source
                .poses
                .iter()
                .zip(&target.poses)
                .map(|((k, a), (_, b))| {
                    (
                        k.clone(),
                        BayMotionSegment {
                            source: *a,
                            target: *b,
                            started_at_ms: 1000,
                            duration_ms: 1000,
                        }
                        .sample(1100)
                        .unwrap()
                        .0,
                    )
                })
                .collect(),
        };
        p.sample_time_ms = 1100;
        p.inventory.high_watermark = 1;
        p.inventory.entries[0].location = CarrierDroneLocation::Launching {
            operation_id: 1,
            berth_id: 0,
        };
        p.inventory.operations.push(CarrierInventoryOperation {
            recall_batch_id: None,
            operation_id: 1,
            drone_id: 1,
            action: CarrierInventoryAction::Launch,
            phase: path[1].phase,
            held_phase: None,
            stage: None,
            berth_id: Some(0),
            accepted_at_ms: 1000,
            updated_at_ms: 1100,
            message: "Confirmed source fixture".into(),
            cargo_service_operation_ids: vec![],
        });
        p.inventory.reservations = vec![
            CarrierResourceReservation {
                resource: CarrierResource::ReadyBerth { berth_id: 0 },
                operation_id: 1,
                drone_id: 1,
            },
            CarrierResourceReservation {
                resource: CarrierResource::Cassette { cassette_id: 0 },
                operation_id: 1,
                drone_id: 1,
            },
        ];
        for (k, pose) in &confirmed.poses {
            match k {
                CarrierPoseKey::Drone => {
                    p.drone_poses.iter_mut().find(|(id, _)| *id == 1).unwrap().1 = *pose
                }
                CarrierPoseKey::Cassette { cassette_id } => {
                    p.cassette_poses
                        .iter_mut()
                        .find(|(id, _)| id == cassette_id)
                        .unwrap()
                        .1 = *pose
                }
                CarrierPoseKey::Mechanism { semantic_id } => {
                    p.machinery
                        .iter_mut()
                        .find(|(id, _)| id == semantic_id)
                        .unwrap()
                        .1 = *pose
                }
            }
        }
        let pose = p.confirmed_pose(1).unwrap();
        pilot.drones.push(crate::DroneView {
            id: 1,
            class: DroneClass::Mining,
            hit_points: 100,
            orientation_xyzw: Some(pose.orientation_xyzw),
            position: pose.position_m,
            heading: pose.transform_vector([1., 0., 0.]).unwrap(),
            velocity: [0.; 3],
            mode: crate::DroneMode::Hold,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            updated_at_ms: 1100,
        });
        p.motions.push(CarrierInventoryMotionView {
            operation_id: 1,
            drone_id: 1,
            route: CarrierPresentationRoute::Hardware {
                route,
                target_index: 1,
            },
            source,
            confirmed,
            started_at_ms: 1000,
            duration_ms: 1000,
            confirmed_at_ms: 1100,
            held: false,
            hold_reason: String::new(),
        });
        (pilot, p)
    }
    #[test]
    fn exact_source_fixture_is_inspectable_but_never_normal_command_admission() {
        let (pilot, p) = fixture();
        assert_eq!(
            p.validate_projection(&pilot),
            Ok(CarrierInventoryAdmission::StationaryFixture)
        );
        assert!(p.validated_next_ids(&p.inventory, &pilot).is_none());
        for case in 0..6 {
            let mut bad = p.clone();
            let mut host = pilot.clone();
            match case {
                0 => bad.inventory.binding.registration.assembly_source_sha256 = "a".repeat(64),
                1 => bad.inventory.binding.vessel_id = "foreign".into(),
                2 => bad.cassette_poses[0].1.position_m[0] += 1.,
                3 => bad.machinery[0].1.position_m[0] += 1.,
                4 => host.flight.as_mut().unwrap().velocity[0] = 1.,
                _ => bad.drone_poses[0].1.position_m[0] += 1.,
            }
            assert!(bad.validate_projection(&host).is_err(), "case{case}")
        }
    }
    #[test]
    fn motion_projection_binds_source_phase_shared_clock_and_actual_body() {
        let (pilot, p) = moving();
        p.validate_projection(&pilot).unwrap();
        for case in 0..6 {
            let mut bad = p.clone();
            let mut host = pilot.clone();
            match case {
                0 => bad.motions[0].confirmed_at_ms += 1,
                1 => bad.motions[0].source.poses[0].1.position_m[0] += 1.,
                2 => bad.inventory.operations[0].phase = CarrierOperationPhase::Securing,
                3 => bad.motions.clear(),
                4 => host.drones[0].position[0] += 1.,
                _ => {
                    bad.motions[0]
                        .confirmed
                        .poses
                        .last_mut()
                        .unwrap()
                        .1
                        .position_m[0] += 1.
                }
            }
            assert!(bad.validate_projection(&host).is_err(), "case{case}")
        }
        let mut held = p.clone();
        held.sample_time_ms = 10_000;
        held.motions[0].held = true;
        held.motions[0].hold_reason = "Pilot held".into();
        held.inventory.operations[0].held_phase = Some(held.inventory.operations[0].phase);
        held.inventory.operations[0].phase = CarrierOperationPhase::Held;
        held.validate_projection(&pilot).unwrap();
    }
}
