//! Exact authored carrier cassette/berth geometry. No candidate is admitted by
//! shape, capacity, class or a caller-supplied coordinate. Authority owns clocks,
//! resource reservations and all terminal inventory/economic transitions.
use crate::drone_bay_geometry::BayPose;
use crate::{
    CarrierCassetteDock, CarrierInventoryAction, CarrierInventoryRegistration,
    CarrierInventoryTopology, CarrierOperationPhase, DroneClass,
};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
mod envelope;
mod physical_variant;
pub use envelope::{
    source_corridor_geometry_clear, source_envelope, CarrierEnvelope, CarrierEnvelopeState,
};
pub use physical_variant::PhysicalServiceFrame;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierInventoryDefinition {
    pub topology: CarrierInventoryTopology,
    pub carrier_bounds_m: [[f64; 3]; 2],
    pub deployed_hardware_bounds_m: [[f64; 3]; 2],
    pub cassettes: Vec<CarrierCassetteGeometry>,
    pub docks: Vec<CarrierDockGeometry>,
    pub berths: Vec<CarrierReadyGeometry>,
    pub mechanisms: Vec<CarrierMechanismGeometry>,
}
/// A conservative source-authored solid box under an existing rigid root.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierCollisionPart {
    pub source_part_id: String,
    pub local_bounds_m: [[f64; 3]; 2],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierCassetteGeometry {
    pub cassette_id: u64,
    pub initial_dock: CarrierCassetteDock,
    pub reference_pose: BayPose,
    pub bounds_local_m: [[f64; 3]; 2],
    pub mechanism_ids: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierDockGeometry {
    pub dock: CarrierCassetteDock,
    /// Confirmed supported/locked cassette body origin, not its transit height.
    pub cassette_pose: BayPose,
    pub working_bounds_m: [[f64; 3]; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierReadyGeometry {
    pub berth_id: u8,
    pub support_pose: BayPose,
    pub contact_pose: BayPose,
    pub gate_pose: BayPose,
    pub clear_pose: BayPose,
    pub ore_transfer_capable: bool,
    pub drone_bounds_local_m: [[f64; 3]; 2],
    pub working_bounds_m: [[f64; 3]; 2],
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierMechanismOwner {
    Berth { berth_id: u8 },
    Cassette { cassette_id: u64 },
    Loader { loader_id: u8 },
    Dock { dock: CarrierCassetteDock },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierMechanismGeometry {
    pub semantic_id: String,
    pub owner: CarrierMechanismOwner,
    pub reference_pose: BayPose,
    pub local_bounds_m: [[f64; 3]; 2],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierInventoryMilestones {
    pub drone_clamped: bool,
    /// The handled cassette rests on its destination's actual support surfaces.
    /// A crane-carried cassette uses crane_latched instead of this flag.
    pub cassette_supported: bool,
    pub cassette_locked: bool,
    pub crane_latched: bool,
    pub door_clear: bool,
    pub ore_coupled: bool,
    pub exterior_clear: bool,
    pub loader_clear: bool,
    pub hardware_safe: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierInventoryKeyframe {
    pub label: String,
    /// Nonterminal physical phase only. Authority alone changes stock presence.
    pub phase: CarrierOperationPhase,
    pub drone_pose: Option<BayPose>,
    pub handled_cassette_id: Option<u64>,
    pub carried_cassette_id: Option<u64>,
    pub cassette_poses: Vec<(u64, BayPose)>,
    pub docked_cassettes: Vec<(u64, CarrierCassetteDock)>,
    pub machinery: Vec<(String, BayPose)>,
    pub completed_stage_legs: u8,
    pub milestones: CarrierInventoryMilestones,
}

/// Candidate optimized exchange. The authority must atomically transfer the
/// released drone/ready resources at the exact shared boundary; merely changing
/// a stock label while retaining the old writer is not safe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierStageHandoffRoute {
    pub handoff_index: usize,
    pub head: Vec<CarrierInventoryKeyframe>,
    pub empty_tail: Vec<CarrierInventoryKeyframe>,
}

/// Deliberately unavailable until the exact composed source and all associated
/// loader/contact/clearance and travel-envelope consumers have passed review.
pub fn lookup(
    _registration: &CarrierInventoryRegistration,
) -> Option<&'static CarrierInventoryDefinition> {
    None
}
impl CarrierInventoryDefinition {
    /// Source-owned three-leg exchange: empty ready cassette to transit buffer,
    /// loaded reserve to ready, then empty buffer cassette to vacated reserve.
    /// No fallback to caller-defined poses or a permanent class/home mapping.
    fn cassette_reaches_dock(&self, cassette_id: u64, destination: CarrierCassetteDock) -> bool {
        self.cassettes
            .iter()
            .find(|c| c.cassette_id == cassette_id)
            .is_some_and(|c| {
                self.topology
                    .same_dock_component(c.initial_dock, destination)
            })
    }
    pub fn stage_route(
        &self,
        class: DroneClass,
        source_slot: u16,
        destination_berth: u8,
        loaded_cassette_id: u64,
        empty_cassette_id: u64,
    ) -> Option<Vec<CarrierInventoryKeyframe>> {
        let fixture = fixture_for_registration(&self.topology.registration)?;
        if self != &fixture.definition || loaded_cassette_id == empty_cassette_id {
            return None;
        }
        let side = self.topology.slot(source_slot)?.side;
        self.topology
            .stage_loader(source_slot, destination_berth, class)?;
        if !self.cassette_on_side(loaded_cassette_id, side)
            || !self.cassette_on_side(empty_cassette_id, side)
            || !self.cassette_reaches_dock(
                loaded_cassette_id,
                CarrierCassetteDock::Protected {
                    slot_id: source_slot,
                },
            )
            || !self.cassette_reaches_dock(
                empty_cassette_id,
                CarrierCassetteDock::Ready {
                    berth_id: destination_berth,
                },
            )
        {
            return None;
        }
        let route = fixture
            .stages
            .iter()
            .find(|r| r.source_slot == source_slot && r.destination_berth == destination_berth)?;
        Some(remap(
            &route.keyframes,
            &[
                (route.loaded_cassette_id, loaded_cassette_id),
                (route.empty_cassette_id, empty_cassette_id),
            ],
        ))
    }
    /// Split the unchanged accepted route at its confirmed leg-two handoff.
    /// The tail never writes the released loaded cassette, drone, ready dock or
    /// berth machinery. It retains only the empty cassette and its cleanup path.
    pub fn stage_handoff_route(
        &self,
        class: DroneClass,
        source_slot: u16,
        destination_berth: u8,
        loaded_cassette_id: u64,
        empty_cassette_id: u64,
    ) -> Option<CarrierStageHandoffRoute> {
        // Local lower-pocket reloaders retain the launch corridor until the
        // complete exchange; the historical 1702 early release is not inherited.
        if matches!(self.topology.registration.assembly_revision, 1703 | 1704) {
            return None;
        }
        let original = self.stage_route(
            class,
            source_slot,
            destination_berth,
            loaded_cassette_id,
            empty_cassette_id,
        )?;
        let index = original.iter().position(|f| f.completed_stage_legs == 2)?;
        let boundary = &original[index];
        if boundary.handled_cassette_id != Some(loaded_cassette_id)
            || !boundary.milestones.cassette_supported
            || !boundary.milestones.cassette_locked
            || !boundary.milestones.loader_clear
            || boundary.carried_cassette_id.is_some()
            || !boundary.docked_cassettes.contains(&(
                loaded_cassette_id,
                CarrierCassetteDock::Ready {
                    berth_id: destination_berth,
                },
            ))
        {
            return None;
        }
        let loader_id = self
            .topology
            .stage_loader(source_slot, destination_berth, class)?;
        let loader = self
            .topology
            .loaders
            .iter()
            .find(|l| l.loader_id == loader_id)?;
        let retained: Vec<_> = self
            .mechanisms
            .iter()
            .filter(|m| match m.owner {
                CarrierMechanismOwner::Cassette { cassette_id } => cassette_id == empty_cassette_id,
                CarrierMechanismOwner::Loader { loader_id } => loader_id == loader.loader_id,
                CarrierMechanismOwner::Dock {
                    dock: CarrierCassetteDock::Protected { slot_id },
                } => slot_id == source_slot,
                CarrierMechanismOwner::Dock {
                    dock: CarrierCassetteDock::TransitBuffer { buffer_id },
                } => buffer_id == loader.transit_buffer_id,
                _ => false,
            })
            .map(|m| m.semantic_id.as_str())
            .collect();
        let mut tail = original[index..].to_vec();
        for f in &mut tail {
            if f.carried_cassette_id == Some(loaded_cassette_id) {
                return None;
            }
            f.drone_pose = None;
            f.phase = CarrierOperationPhase::Securing;
            f.milestones.drone_clamped = false;
            f.cassette_poses.retain(|(id, _)| *id == empty_cassette_id);
            f.docked_cassettes
                .retain(|(id, _)| *id == empty_cassette_id);
            f.machinery
                .retain(|(id, _)| retained.contains(&id.as_str()));
            if f.handled_cassette_id == Some(loaded_cassette_id) {
                f.handled_cassette_id = None;
                f.milestones.cassette_supported = false;
                f.milestones.cassette_locked = false;
            }
        }
        Some(CarrierStageHandoffRoute {
            handoff_index: index,
            head: original[..=index].to_vec(),
            empty_tail: tail,
        })
    }

    pub fn bay_route(
        &self,
        berth_id: u8,
        class: DroneClass,
        cassette_id: u64,
        action: CarrierInventoryAction,
    ) -> Option<Vec<CarrierInventoryKeyframe>> {
        if self.topology.registration.assembly_revision == 1704 {
            return physical_variant::bay_route(self, berth_id, class, cassette_id, action);
        }
        if self.topology.registration.assembly_revision == 1703
            && matches!(action, CarrierInventoryAction::ReturnToUnload { .. })
        {
            return self.parallel_service_route(berth_id, class, cassette_id);
        }
        if self.topology.registration.assembly_revision == 1703
            && action == CarrierInventoryAction::RecallToStorage
        {
            return self.parallel_capture_route(berth_id, class, cassette_id);
        }
        let fixture = fixture_for_registration(&self.topology.registration)?;
        let berth = self.topology.berth(berth_id)?;
        if self != &fixture.definition
            || !(action == CarrierInventoryAction::Launch
                || (self.topology.registration.assembly_revision == 1702
                    && action == CarrierInventoryAction::RecallToStorage))
            || !berth.compatible_classes.contains(&class)
            || !self.cassette_on_side(cassette_id, berth.side)
            || !self.cassette_reaches_dock(cassette_id, CarrierCassetteDock::Ready { berth_id })
        {
            return None;
        }
        let route = fixture.launches.iter().find(|r| r.berth_id == berth_id)?;
        let mut frames = remap(&route.keyframes, &[(route.cassette_id, cassette_id)]);
        if action == CarrierInventoryAction::RecallToStorage {
            // Source poses are the exact reverse of the proved launch sweeps.
            // Authority must approach the entry continuously, reserve an empty
            // actual cassette, and reject payload until unloading is supported.
            frames.reverse();
            let last = frames.len() - 1;
            for (index, frame) in frames.iter_mut().enumerate() {
                frame.label = format!("ready capture/{}", frame.label);
                frame.phase = CarrierOperationPhase::Returning;
                frame.milestones.hardware_safe = index == last;
            }
        }
        Some(frames)
    }
    /// Only the exact compiled parallel source has certified complete leaf coverage.
    /// A semantic cassette pose is not an additional solid when ALL of its
    /// actual mechanism roots are present in the authority collision scene.
    pub fn has_complete_compound_collision(&self) -> bool {
        if self.topology.registration.assembly_revision == 1704 {
            return physical_variant::has_complete_compound_collision(self);
        }
        if self.topology.registration.assembly_revision != 1703 {
            return false;
        }
        let Some(fixture) = fixture_for_registration(&self.topology.registration) else {
            return false;
        };
        static VALID: OnceLock<bool> = OnceLock::new();
        (std::ptr::eq(self, &fixture.definition) || self == &fixture.definition)
            && *VALID.get_or_init(|| fixture.compound_collision_valid())
    }
    pub fn mechanism_collision_parts(
        &self,
        semantic_id: &str,
    ) -> Option<&'static [CarrierCollisionPart]> {
        if !self.has_complete_compound_collision() {
            return None;
        }
        fixture_for_registration(&self.topology.registration)?
            .mechanism_collision_parts
            .iter()
            .find(|group| group.semantic_id == semantic_id)
            .map(|group| group.parts.as_slice())
    }
    pub fn idle_roots(&self, cassette_poses: &[(u64, BayPose)]) -> Option<Vec<(String, BayPose)>> {
        let fixture = fixture_for_registration(&self.topology.registration)?;
        if self != &fixture.definition
            || cassette_poses.len() != self.cassettes.len()
            || !fixture.idle_machinery_valid()
        {
            return None;
        }
        for cassette in &self.cassettes {
            if cassette_poses
                .iter()
                .filter(|(id, p)| {
                    *id == cassette.cassette_id
                        && p.valid()
                        && p.orientation_xyzw == cassette.reference_pose.orientation_xyzw
                })
                .count()
                != 1
            {
                return None;
            }
        }
        self.mechanisms
            .iter()
            .map(|m| {
                let mut pose = if fixture.idle_machinery.is_empty() {
                    m.reference_pose
                } else {
                    fixture
                        .idle_machinery
                        .iter()
                        .find(|(id, _)| id == &m.semantic_id)?
                        .1
                };
                if let CarrierMechanismOwner::Cassette { cassette_id } = m.owner {
                    let original = self
                        .cassettes
                        .iter()
                        .find(|c| c.cassette_id == cassette_id)?;
                    let (_, current) = cassette_poses.iter().find(|(id, _)| *id == cassette_id)?;
                    for i in 0..3 {
                        pose.position_m[i] +=
                            current.position_m[i] - original.reference_pose.position_m[i];
                    }
                }
                Some((m.semantic_id.clone(), pose))
            })
            .collect()
    }
    fn cassette_on_side(&self, id: u64, side: crate::CarrierSide) -> bool {
        self.cassettes
            .iter()
            .find(|c| c.cassette_id == id)
            .is_some_and(|c| match c.initial_dock {
                CarrierCassetteDock::Ready { berth_id } => self
                    .topology
                    .berth(berth_id)
                    .is_some_and(|b| b.side == side),
                CarrierCassetteDock::Protected { slot_id } => {
                    self.topology.slot(slot_id).is_some_and(|s| s.side == side)
                }
                CarrierCassetteDock::TransitBuffer { .. } => false,
            })
    }
}

/// Exact stationary test fixture only; never production admission.
pub fn stationary_fixture_definition() -> Option<&'static CarrierInventoryDefinition> {
    Some(&fixture()?.definition)
}

/// Exact compiled inspection fixtures. This is deliberately distinct from
/// normal source admission; unknown hashes never select a nearby variant.
pub fn stationary_fixture_for_registration(
    registration: &CarrierInventoryRegistration,
) -> Option<&'static CarrierInventoryDefinition> {
    Some(&fixture_for_registration(registration)?.definition)
}
fn fixture_for_registration(
    registration: &CarrierInventoryRegistration,
) -> Option<&'static StationaryFixture> {
    let f = match registration.assembly_revision {
        1701 => fixture()?,
        1702 => composed_fixture()?,
        1703 => parallel_fixture()?,
        1704 => physical_variant::fixture()?,
        _ => return None,
    };
    (f.definition.topology.registration == *registration).then_some(f)
}
fn composed_fixture() -> Option<&'static StationaryFixture> {
    static FIXTURE: OnceLock<Option<StationaryFixture>> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "carrier_inventory_geometry/stationary_v1702.json"
            ))
            .ok()
        })
        .as_ref()
}

fn parallel_fixture() -> Option<&'static StationaryFixture> {
    static FIXTURE: OnceLock<Option<StationaryFixture>> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "carrier_inventory_geometry/stationary_v1703.json"
            ))
            .ok()
        })
        .as_ref()
}

#[derive(Deserialize)]
struct StationaryFixture {
    definition: CarrierInventoryDefinition,
    stages: Vec<StageFixture>,
    launches: Vec<LaunchFixture>,
    #[serde(default)]
    idle_machinery: Vec<(String, BayPose)>,
    #[serde(default)]
    mechanism_collision_parts: Vec<CarrierMechanismCollisionParts>,
    #[serde(default)]
    collision_coverage: Option<CarrierCollisionCoverage>,
}
#[derive(Clone, Deserialize)]
struct CarrierMechanismCollisionParts {
    semantic_id: String,
    parts: Vec<CarrierCollisionPart>,
}
#[derive(Clone, Deserialize)]
struct CarrierCollisionCoverage {
    assembly_source_sha256: String,
    mechanism_count: usize,
    logical_part_count: usize,
    parts_sha256: String,
}
impl StationaryFixture {
    fn compound_collision_valid(&self) -> bool {
        use sha2::{Digest, Sha256};
        use std::collections::BTreeSet;
        let expected = match self.definition.topology.registration.assembly_revision {
            1703 => Some((
                480,
                4430,
                "9394bf9b62aa611d3cf1e2d1e41bd120f0914b6adaa9e6f3ce4e2a3f97729e51",
            )),
            1704 => physical_variant::expected_collision_coverage(),
            _ => None,
        };
        let Some((mechanisms, logical_parts, parts_sha256)) = expected else {
            return false;
        };
        let Some(coverage) = &self.collision_coverage else {
            return false;
        };
        if coverage.assembly_source_sha256
            != self.definition.topology.registration.assembly_source_sha256
            || coverage.mechanism_count != mechanisms
            || coverage.logical_part_count != logical_parts
            || coverage.parts_sha256 != parts_sha256
            || self.mechanism_collision_parts.len() != self.definition.mechanisms.len()
            || self.mechanism_collision_parts.len() != coverage.mechanism_count
        {
            return false;
        }
        let mut hash = Sha256::new();
        hash.update(b"arkline-carrier-compound-v1\0");
        hash.update((self.mechanism_collision_parts.len() as u32).to_le_bytes());
        let mut seen = BTreeSet::new();
        let mut prior_group = "";
        for group in &self.mechanism_collision_parts {
            let Some(mechanism) = self
                .definition
                .mechanisms
                .iter()
                .find(|m| m.semantic_id == group.semantic_id)
            else {
                return false;
            };
            if group.semantic_id.as_str() <= prior_group || group.parts.is_empty() {
                return false;
            }
            prior_group = &group.semantic_id;
            hash.update((group.semantic_id.len() as u32).to_le_bytes());
            hash.update(group.semantic_id.as_bytes());
            hash.update((group.parts.len() as u32).to_le_bytes());
            let mut prior_part = "";
            for part in &group.parts {
                if part.source_part_id.as_str() <= prior_part || !seen.insert(&part.source_part_id)
                {
                    return false;
                }
                prior_part = &part.source_part_id;
                let bounds = part.local_bounds_m;
                for axis in 0..3 {
                    if !bounds[0][axis].is_finite()
                        || !bounds[1][axis].is_finite()
                        || bounds[0][axis] >= bounds[1][axis]
                        || bounds[0][axis] < mechanism.local_bounds_m[0][axis] - 1e-7
                        || bounds[1][axis] > mechanism.local_bounds_m[1][axis] + 1e-7
                    {
                        return false;
                    }
                }
                hash.update((part.source_part_id.len() as u32).to_le_bytes());
                hash.update(part.source_part_id.as_bytes());
                for value in bounds.into_iter().flatten() {
                    hash.update(value.to_le_bytes());
                }
            }
        }
        seen.len() == coverage.logical_part_count
            && hex::encode(hash.finalize()) == coverage.parts_sha256
    }
    fn idle_machinery_valid(&self) -> bool {
        if self.idle_machinery.is_empty() {
            return matches!(
                self.definition.topology.registration.assembly_revision,
                1701 | 1702
            );
        }
        self.idle_machinery.len() == self.definition.mechanisms.len()
            && self.definition.mechanisms.iter().all(|m| {
                self.idle_machinery
                    .iter()
                    .filter(|(id, pose)| id == &m.semantic_id && pose.valid())
                    .count()
                    == 1
            })
    }
}
#[derive(Deserialize)]
struct StageFixture {
    source_slot: u16,
    destination_berth: u8,
    loaded_cassette_id: u64,
    empty_cassette_id: u64,
    keyframes: Vec<CarrierInventoryKeyframe>,
}
#[derive(Deserialize)]
struct LaunchFixture {
    berth_id: u8,
    cassette_id: u64,
    keyframes: Vec<CarrierInventoryKeyframe>,
}
fn fixture() -> Option<&'static StationaryFixture> {
    static FIXTURE: OnceLock<Option<StationaryFixture>> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "carrier_inventory_geometry/stationary_v1701.json"
            ))
            .ok()
        })
        .as_ref()
}
fn remap(rows: &[CarrierInventoryKeyframe], ids: &[(u64, u64)]) -> Vec<CarrierInventoryKeyframe> {
    let map = |id| {
        ids.iter()
            .find(|(old, _)| *old == id)
            .map_or(id, |(_, new)| *new)
    };
    let mut rows = rows.to_vec();
    for row in &mut rows {
        row.handled_cassette_id = row.handled_cassette_id.map(map);
        row.carried_cassette_id = row.carried_cassette_id.map(map);
        for (id, _) in &mut row.cassette_poses {
            *id = map(*id);
        }
        for (id, _) in &mut row.docked_cassettes {
            *id = map(*id);
        }
        for (name, _) in &mut row.machinery {
            if let Some((head, tail)) = name.split_once('/') {
                if let Some(id) = head.strip_prefix('c').and_then(|s| s.parse::<u64>().ok()) {
                    *name = format!("c{:02}/{tail}", map(id));
                }
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_compiled_fixture_has_twenty_cassettes_and_all_same_side_routes() {
        let f = fixture().expect("source fixture parses");
        let d = &f.definition;
        assert!(d.topology.valid_shape());
        assert_eq!(d.cassettes.len(), 20);
        assert_eq!(f.stages.len(), 50);
        assert_eq!(f.launches.len(), 10);
        assert!(lookup(&d.topology.registration).is_none());
        for source in 0..10 {
            for berth in 0..10 {
                let r = d.stage_route(
                    DroneClass::Mining,
                    source,
                    berth,
                    u64::from(source) + 10,
                    u64::from(berth),
                );
                if source / 5 != u16::from(berth) / 5 {
                    assert!(r.is_none());
                    continue;
                }
                let r = r.unwrap();
                assert_eq!(r.first().unwrap().completed_stage_legs, 0);
                assert_eq!(r.last().unwrap().completed_stage_legs, 3);
                assert!(r.last().unwrap().milestones.hardware_safe);
                assert!(r.last().unwrap().milestones.loader_clear);
                for frame in r {
                    assert!(!frame.phase.terminal());
                    assert!(frame.drone_pose.unwrap().valid());
                    assert_eq!(frame.cassette_poses.len(), 2);
                    assert!(frame.machinery.iter().all(
                        |(g, p)| p.valid() && d.mechanisms.iter().any(|m| m.semantic_id == *g)
                    ));
                }
            }
        }
    }
    #[test]
    fn nonhome_cassette_ids_are_remapped_without_touching_unowned_cassettes() {
        let d = &fixture().unwrap().definition;
        let r = d.stage_route(DroneClass::Attack, 0, 4, 14, 10).unwrap();
        for f in r {
            assert!(f.cassette_poses.iter().all(|(id, _)| [14, 10].contains(id)));
            assert!(f
                .machinery
                .iter()
                .filter(|(g, _)| g.starts_with('c'))
                .all(|(g, _)| g.starts_with("c14/") || g.starts_with("c10/")));
        }
        assert!(d.stage_route(DroneClass::Mining, 0, 4, 15, 4).is_none());
        let launch = d
            .bay_route(4, DroneClass::Recon, 14, CarrierInventoryAction::Launch)
            .unwrap();
        assert!(launch.last().unwrap().milestones.exterior_clear);
        assert!(launch.last().unwrap().milestones.hardware_safe);
        assert!(launch
            .iter()
            .all(|f| f.cassette_poses.len() == 1 && f.cassette_poses[0].0 == 14));
        assert!(d
            .bay_route(
                4,
                DroneClass::Mining,
                14,
                CarrierInventoryAction::RecallToStorage
            )
            .is_none());
    }
    #[test]
    fn handoff_preserves_original_head_and_exact_retained_tail_for_all_routes() {
        let d = stationary_fixture_definition().unwrap();
        for source in 0..10 {
            for berth in (source / 5 * 5) as u8..(source / 5 * 5 + 5) as u8 {
                // Exercise swapped/non-home IDs as well as original placement.
                let loaded = u64::from(source / 5 * 5) + 10 + u64::from((source + 1) % 5);
                let empty = u64::from(source / 5 * 5) + u64::from((source + 2) % 5);
                let original = d
                    .stage_route(DroneClass::Mining, source, berth, loaded, empty)
                    .unwrap();
                let split = d
                    .stage_handoff_route(DroneClass::Mining, source, berth, loaded, empty)
                    .unwrap();
                assert_eq!(split.head, original[..=split.handoff_index]);
                assert_eq!(split.head.last().unwrap().completed_stage_legs, 2);
                assert_eq!(split.empty_tail.last().unwrap().completed_stage_legs, 3);
                for (tail, old) in split
                    .empty_tail
                    .iter()
                    .zip(&original[split.handoff_index..])
                {
                    assert!(tail.drone_pose.is_none());
                    assert_eq!(tail.phase, CarrierOperationPhase::Securing);
                    assert_eq!(
                        tail.machinery.iter().map(|p| &p.0).collect::<Vec<_>>(),
                        split.empty_tail[0]
                            .machinery
                            .iter()
                            .map(|p| &p.0)
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(
                        tail.cassette_poses.iter().map(|p| p.0).collect::<Vec<_>>(),
                        split.empty_tail[0]
                            .cassette_poses
                            .iter()
                            .map(|p| p.0)
                            .collect::<Vec<_>>()
                    );
                    assert_eq!(tail.cassette_poses.len(), 1);
                    assert_eq!(tail.cassette_poses[0].0, empty);
                    assert!(old.cassette_poses.contains(&tail.cassette_poses[0]));
                    assert!(tail.machinery.iter().all(|p| old.machinery.contains(p)));
                    assert!(!tail.machinery.iter().any(|(id, _)| {
                        id.starts_with(&format!("c{loaded:02}/"))
                            || id.starts_with(&format!("b{berth:02}/"))
                            || id.starts_with("dock/ready/")
                    }));
                    assert_eq!(tail.completed_stage_legs, old.completed_stage_legs);
                }
                let first = &split.empty_tail[0];
                assert!(first.machinery.iter().all(|p| split
                    .head
                    .last()
                    .unwrap()
                    .machinery
                    .contains(p)));
                assert!(first.handled_cassette_id.is_none());
                assert!(!first.milestones.cassette_supported);
                assert!(split.empty_tail.last().unwrap().milestones.hardware_safe);
            }
        }
    }
    #[test]
    fn composed_fixture_is_exact_fresh_registration_without_replacing_historical_routes() {
        let old = stationary_fixture_definition().unwrap();
        let new = &composed_fixture().unwrap().definition;
        assert_eq!(old.topology.registration.assembly_revision, 1701);
        assert_eq!(new.topology.registration.assembly_revision, 1702);
        assert_ne!(old.topology.registration, new.topology.registration);
        assert_eq!(old.carrier_bounds_m, new.carrier_bounds_m);
        assert!(new.deployed_hardware_bounds_m[1][1] > 15.15);
        for definition in [old, new] {
            assert_eq!(
                stationary_fixture_for_registration(&definition.topology.registration),
                Some(definition)
            );
            assert!(lookup(&definition.topology.registration).is_none());
            let mut bad = definition.topology.registration.clone();
            bad.assembly_source_sha256 = "0".repeat(64);
            assert!(stationary_fixture_for_registration(&bad).is_none());
        }
        for source in 0..10 {
            for berth in (source / 5 * 5) as u8..(source / 5 * 5 + 5) as u8 {
                assert_eq!(
                    old.stage_route(
                        DroneClass::Mining,
                        source,
                        berth,
                        u64::from(source) + 10,
                        u64::from(berth)
                    ),
                    new.stage_route(
                        DroneClass::Mining,
                        source,
                        berth,
                        u64::from(source) + 10,
                        u64::from(berth)
                    )
                );
            }
        }
        assert!(new
            .stage_handoff_route(DroneClass::Mining, 0, 4, 14, 10)
            .is_some());
    }
    #[test]
    fn composed_ready_capture_reverses_every_launch_pose_without_enabling_old_fixture() {
        let old = stationary_fixture_definition().unwrap();
        let new = &composed_fixture().unwrap().definition;
        for berth in 0..10 {
            for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
                let cid = u64::from(berth) + 10;
                let launch = new
                    .bay_route(berth, class, cid, CarrierInventoryAction::Launch)
                    .unwrap();
                let capture = new
                    .bay_route(berth, class, cid, CarrierInventoryAction::RecallToStorage)
                    .unwrap();
                assert!(old
                    .bay_route(berth, class, cid, CarrierInventoryAction::RecallToStorage)
                    .is_none());
                for (index, (actual, previous)) in
                    capture.iter().zip(launch.iter().rev()).enumerate()
                {
                    let mut expected = previous.clone();
                    expected.label = format!("ready capture/{}", expected.label);
                    expected.phase = CarrierOperationPhase::Returning;
                    expected.milestones.hardware_safe = index + 1 == capture.len();
                    assert_eq!(*actual, expected);
                }
                assert!(capture.first().unwrap().milestones.exterior_clear);
                assert!(!capture.first().unwrap().milestones.drone_clamped);
                let end = capture.last().unwrap();
                assert!(
                    end.milestones.hardware_safe
                        && end.milestones.drone_clamped
                        && end.milestones.cassette_supported
                        && end.milestones.cassette_locked
                );
                assert!(!end.milestones.door_clear);
                assert_eq!(end.drone_pose, launch.first().unwrap().drone_pose);
            }
        }
    }
    #[test]
    fn parallel_cassette_remap_respects_source_loader_components() {
        let d = &parallel_fixture().unwrap().definition;
        for berth in 0..10u8 {
            let ready = u64::from(berth);
            let protected = ready + 10;
            assert!(d
                .stage_route(
                    DroneClass::Mining,
                    u16::from(berth),
                    berth,
                    protected,
                    ready
                )
                .is_some());
            assert!(d
                .stage_route(
                    DroneClass::Mining,
                    u16::from(berth),
                    berth,
                    ready,
                    protected
                )
                .is_some());
            assert!(d
                .bay_route(
                    berth,
                    DroneClass::Mining,
                    protected,
                    CarrierInventoryAction::Launch
                )
                .is_some());
            let foreign = u64::from((berth / 5) * 5 + (berth % 5 + 1) % 5);
            assert!(d
                .stage_route(
                    DroneClass::Mining,
                    u16::from(berth),
                    berth,
                    foreign + 10,
                    ready
                )
                .is_none());
            assert!(d
                .stage_route(
                    DroneClass::Mining,
                    u16::from(berth),
                    berth,
                    protected,
                    foreign
                )
                .is_none());
            assert!(d
                .bay_route(
                    berth,
                    DroneClass::Mining,
                    foreign,
                    CarrierInventoryAction::Launch
                )
                .is_none());
        }
    }
    #[test]
    fn parallel_compound_collision_requires_exact_complete_source_coverage() {
        let bytes = include_str!("carrier_inventory_geometry/stationary_v1703.json");
        let read = || serde_json::from_str::<StationaryFixture>(bytes).unwrap();
        let fixture = read();
        assert!(fixture.compound_collision_valid());
        let definition = &fixture.definition;
        assert!(definition.has_complete_compound_collision());
        assert_eq!(
            definition
                .mechanisms
                .iter()
                .map(|m| definition
                    .mechanism_collision_parts(&m.semantic_id)
                    .unwrap()
                    .len())
                .sum::<usize>(),
            4430
        );
        assert!(definition
            .mechanism_collision_parts("unknown/root")
            .is_none());
        let mut bad = read();
        bad.mechanism_collision_parts[0].parts.pop();
        assert!(!bad.compound_collision_valid());
        let mut bad = read();
        bad.mechanism_collision_parts[0].parts[0].source_part_id = "unknown/part".into();
        assert!(!bad.compound_collision_valid());
        let mut bad = read();
        bad.mechanism_collision_parts[0].semantic_id = "unknown/root".into();
        assert!(!bad.compound_collision_valid());
        let mut bad = read();
        bad.mechanism_collision_parts[0].parts[0].local_bounds_m[0][0] = f64::NAN;
        assert!(!bad.compound_collision_valid());
        let mut bad = read();
        bad.mechanism_collision_parts[0].parts[0].local_bounds_m[1][0] += 100.;
        assert!(!bad.compound_collision_valid());
        let mut bad = read();
        bad.collision_coverage = None;
        assert!(!bad.compound_collision_valid());
        for revision in [1701, 1702] {
            let legacy = if revision == 1701 {
                fixture_for_registration(
                    &stationary_fixture_definition()
                        .unwrap()
                        .topology
                        .registration,
                )
                .unwrap()
            } else {
                composed_fixture().unwrap()
            };
            assert!(!legacy.definition.has_complete_compound_collision());
        }
    }
    #[test]
    fn parallel_idle_machinery_rejects_missing_duplicate_unknown_and_invalid_roots() {
        let bytes = include_str!("carrier_inventory_geometry/stationary_v1703.json");
        let mut fixture: StationaryFixture = serde_json::from_str(bytes).unwrap();
        assert!(fixture.idle_machinery_valid());
        let original = fixture.idle_machinery.clone();
        fixture.idle_machinery.clear();
        assert!(!fixture.idle_machinery_valid());
        fixture.idle_machinery = original.clone();
        fixture.idle_machinery.pop();
        assert!(!fixture.idle_machinery_valid());
        fixture.idle_machinery = original.clone();
        fixture.idle_machinery[0] = fixture.idle_machinery[1].clone();
        assert!(!fixture.idle_machinery_valid());
        fixture.idle_machinery = original.clone();
        fixture.idle_machinery[0].0 = "unknown/root".into();
        assert!(!fixture.idle_machinery_valid());
        fixture.idle_machinery = original;
        fixture.idle_machinery[0].1.position_m[0] = f64::NAN;
        assert!(!fixture.idle_machinery_valid());
        for revision in [1701, 1702] {
            fixture.definition.topology.registration.assembly_revision = revision;
            fixture.idle_machinery.clear();
            assert!(fixture.idle_machinery_valid());
        }
    }
    #[test]
    fn parallel_fixture_has_independent_aligned_loaders_and_exact_idle_entry() {
        let d = &parallel_fixture()
            .expect("parallel source fixture")
            .definition;
        assert_eq!(d.topology.registration.assembly_revision, 1703);
        assert_eq!(d.topology.loaders.len(), 10);
        assert_eq!(d.topology.transit_buffers.len(), 10);
        assert_eq!(d.cassettes.len(), 20);
        let poses: Vec<_> = d
            .cassettes
            .iter()
            .map(|c| (c.cassette_id, c.reference_pose))
            .collect();
        let idle = d.idle_roots(&poses).expect("source idle roots");
        for bid in 0..10u8 {
            for class in [DroneClass::Mining, DroneClass::Recon, DroneClass::Attack] {
                let frames = d
                    .stage_route(
                        class,
                        u16::from(bid),
                        bid,
                        u64::from(bid) + 10,
                        u64::from(bid),
                    )
                    .unwrap();
                assert_eq!(frames.len(), 42);
                for (id, pose) in &frames[0].machinery {
                    assert_eq!(idle.iter().find(|(other, _)| other == id).unwrap().1, *pose);
                }
                let end = frames.last().unwrap();
                assert_eq!(end.completed_stage_legs, 3);
                assert!(end.milestones.hardware_safe);
                let mut exchanged = poses.clone();
                for (id, pose) in &end.cassette_poses {
                    exchanged
                        .iter_mut()
                        .find(|(other, _)| other == id)
                        .unwrap()
                        .1 = *pose;
                }
                let rest = d.idle_roots(&exchanged).unwrap();
                for (id, pose) in &end.machinery {
                    assert_eq!(rest.iter().find(|(other, _)| other == id).unwrap().1, *pose);
                }
                assert!(d
                    .stage_handoff_route(
                        class,
                        u16::from(bid),
                        bid,
                        u64::from(bid) + 10,
                        u64::from(bid)
                    )
                    .is_none());
                for other in 0..10u8 {
                    if bid != other {
                        assert!(d
                            .stage_route(
                                class,
                                u16::from(bid),
                                other,
                                u64::from(bid) + 10,
                                u64::from(other)
                            )
                            .is_none());
                    }
                }
            }
        }
        assert!(d.berths.iter().all(|b| !b.ore_transfer_capable));
        let mut bad = d.topology.registration.clone();
        bad.assembly_source_sha256 = "0".repeat(64);
        assert!(stationary_fixture_for_registration(&bad).is_none());
        assert!(lookup(&d.topology.registration).is_none());
    }
}

mod reverse_transfer;

mod parallel_reverse;
mod service_route;
