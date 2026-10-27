//! Exact, opt-in physical-source geometry. This module does not enable normal
//! admission, flight envelopes, volume mutation or other source revisions.
use super::*;
use crate::physical_cargo::source::physical_source_configuration;
use sha2::{Digest, Sha256};

// Populated only after the source compiler emits and verifies the complete new
// fixture. An unfinished extractor must fail closed, not inherit 1703's receipt.
const FIXTURE_BYTES: &[u8] = include_bytes!("stationary_v1704.json");
const FIXTURE_SHA256: Option<&str> =
    Some("0c1c62c55330d236df9fae60d551e6840ece044e761be2749829a3d2c7bcbd35");
const EXPECTED_COLLISION_COVERAGE: Option<(usize, usize, &str)> = Some((
    500,
    4890,
    "64d118a386c4ec0562f73c543a9c13a9ec29bc0c7417430a2b9f077a9f583e48",
));

#[derive(Deserialize)]
struct PhysicalFixture {
    #[serde(flatten)]
    geometry: StationaryFixture,
    physical_source_identity_sha256: String,
    receiver_motion_contract_sha256: String,
    receiver_panel_travel_mm: u32,
    services: Vec<PhysicalServiceRoute>,
    captures: Vec<LaunchFixture>,
}

/// Inspection-only synchronized source pose. The inlet panels translate by
/// opposite signs along drone-local Z, from zero to exactly 250 millimetres.
/// This vocabulary is not accepted by the existing gameplay motion executor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalServiceFrame {
    pub frame: CarrierInventoryKeyframe,
    pub drone_inlet_travel_mm: u16,
}
#[derive(Deserialize)]
struct PhysicalServiceRoute {
    berth_id: u8,
    cassette_id: u64,
    keyframes: Vec<PhysicalServiceFrame>,
}

fn service_frames_valid(route: &PhysicalServiceRoute) -> bool {
    route.keyframes.len() >= 2
        && route
            .keyframes
            .first()
            .is_some_and(|f| f.drone_inlet_travel_mm == 0)
        && route
            .keyframes
            .last()
            .is_some_and(|f| f.drone_inlet_travel_mm == 0)
        && route.keyframes.iter().all(|f| {
            f.drone_inlet_travel_mm <= 250
                && (!f.frame.milestones.ore_coupled || f.drone_inlet_travel_mm == 250)
        })
}

pub(super) fn expected_collision_coverage() -> Option<(usize, usize, &'static str)> {
    EXPECTED_COLLISION_COVERAGE
}

fn provenance_valid(candidate: &PhysicalFixture) -> bool {
    let Some(source) = physical_source_configuration() else {
        return false;
    };
    let Some(ship) = source.assets.get("ship_glb") else {
        return false;
    };
    let Some(motion) = source.assets.get("receiver_motion_contract") else {
        return false;
    };
    let registration = &candidate.geometry.definition.topology.registration;
    registration.assembly_revision == 1704
        && registration.assembly_source_sha256 == ship.sha256
        && candidate.physical_source_identity_sha256 == source.identity_sha256().unwrap_or_default()
        && candidate.receiver_motion_contract_sha256 == motion.sha256
        && candidate.receiver_panel_travel_mm == source.receiver_panel_travel_mm
        && candidate.receiver_panel_travel_mm == 94
        && candidate.geometry.compound_collision_valid()
        && candidate.geometry.idle_machinery_valid()
        && candidate.services.iter().all(service_frames_valid)
}

fn parse_fixture(bytes: &[u8]) -> Option<PhysicalFixture> {
    if Some(hex::encode(Sha256::digest(bytes)).as_str()) != FIXTURE_SHA256 {
        return None;
    }
    let fixture: PhysicalFixture = serde_json::from_slice(bytes).ok()?;
    provenance_valid(&fixture).then_some(fixture)
}

fn physical_fixture() -> Option<&'static PhysicalFixture> {
    static FIXTURE: OnceLock<Option<PhysicalFixture>> = OnceLock::new();
    FIXTURE
        .get_or_init(|| parse_fixture(FIXTURE_BYTES))
        .as_ref()
}

pub(super) fn fixture() -> Option<&'static StationaryFixture> {
    Some(&physical_fixture()?.geometry)
}

pub(super) fn has_complete_compound_collision(definition: &CarrierInventoryDefinition) -> bool {
    fixture()
        .is_some_and(|f| std::ptr::eq(definition, &f.definition) || definition == &f.definition)
}

/// Exact authored keyframes, not the old eight-root service approximation.
/// Only route rows included in this new source certificate can be selected.
pub(super) fn bay_route(
    definition: &CarrierInventoryDefinition,
    berth_id: u8,
    class: DroneClass,
    cassette_id: u64,
    action: CarrierInventoryAction,
) -> Option<Vec<CarrierInventoryKeyframe>> {
    let physical = physical_fixture()?;
    let source = &physical.geometry;
    if definition != &source.definition
        || class != DroneClass::Mining
        || !definition
            .topology
            .berth(berth_id)?
            .compatible_classes
            .contains(&class)
        || !definition.cassette_reaches_dock(cassette_id, CarrierCassetteDock::Ready { berth_id })
    {
        return None;
    }
    let rows = match action {
        CarrierInventoryAction::Launch => &source.launches,
        CarrierInventoryAction::RecallToStorage => &physical.captures,
        _ => return None,
    };
    let row = rows
        .iter()
        .find(|r| r.berth_id == berth_id && r.cassette_id == cassette_id)?;
    Some(remap(&row.keyframes, &[(row.cassette_id, cassette_id)]))
}

impl CarrierInventoryDefinition {
    /// Source-specific service poses for independent geometric inspection only.
    /// Unlike bay_route, this carries both the ship hardware and drone inlet
    /// valve. It never opens generic ore-service or authoritative transfer access.
    pub fn physical_inspection_service_route(
        &self,
        berth_id: u8,
        class: DroneClass,
        cassette_id: u64,
    ) -> Option<Vec<PhysicalServiceFrame>> {
        let physical = physical_fixture()?;
        // The source's launch route authenticates exact definition, graph,
        // selected bay, compatible body and current portable cassette identity.
        bay_route(
            self,
            berth_id,
            class,
            cassette_id,
            CarrierInventoryAction::Launch,
        )?;
        let route = physical.services.iter().find(|r| r.berth_id == berth_id)?;
        let mut frames = route.keyframes.clone();
        for typed in &mut frames {
            typed.frame = remap(
                std::slice::from_ref(&typed.frame),
                &[(route.cassette_id, cassette_id)],
            )
            .pop()?;
        }
        Some(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_physical_fixture_is_inspection_only_and_source_bound() {
        let physical = physical_fixture().expect("exact physical source compiler receipt");
        let definition = &physical.geometry.definition;
        let registration = &definition.topology.registration;
        assert_eq!(registration.assembly_revision, 1704);
        assert_eq!(physical.receiver_panel_travel_mm, 94);
        assert_eq!(
            stationary_fixture_for_registration(registration),
            Some(definition)
        );
        assert!(lookup(registration).is_none());
        assert!(source_envelope(registration, CarrierEnvelopeState::Closed).is_none());
        assert!(source_envelope(registration, CarrierEnvelopeState::Operational).is_none());
        assert!(definition.has_complete_compound_collision());
        let mut wrong = registration.clone();
        wrong.assembly_source_sha256 = "0".repeat(64);
        assert!(stationary_fixture_for_registration(&wrong).is_none());
        wrong = registration.clone();
        wrong.host.source_sha256 = "0".repeat(64);
        assert!(stationary_fixture_for_registration(&wrong).is_none());
        wrong = registration.clone();
        wrong.assembly_revision = 1703;
        assert!(stationary_fixture_for_registration(&wrong).is_none());
        // The historical cached validity must not be reused for the new source.
        assert!(parallel_fixture()
            .unwrap()
            .definition
            .has_complete_compound_collision());
        assert!(definition.has_complete_compound_collision());
    }

    #[test]
    fn physical_motion_identity_and_collision_tampering_reject() {
        let read = || serde_json::from_slice::<PhysicalFixture>(FIXTURE_BYTES).unwrap();
        assert!(provenance_valid(&read()));
        let mut candidate = read();
        candidate.receiver_panel_travel_mm = 95;
        assert!(!provenance_valid(&candidate));
        candidate = read();
        candidate.receiver_motion_contract_sha256 = "0".repeat(64);
        assert!(!provenance_valid(&candidate));
        candidate = read();
        candidate.physical_source_identity_sha256 = "0".repeat(64);
        assert!(!provenance_valid(&candidate));
        candidate = read();
        candidate.geometry.mechanism_collision_parts.pop();
        assert!(!provenance_valid(&candidate));
        candidate = read();
        candidate.geometry.mechanism_collision_parts[0].parts[0].local_bounds_m[0][0] += 0.01;
        assert!(!provenance_valid(&candidate));
        candidate = read();
        candidate
            .geometry
            .collision_coverage
            .as_mut()
            .unwrap()
            .parts_sha256 = "0".repeat(64);
        assert!(!provenance_valid(&candidate));
        candidate = read();
        candidate.services[0].keyframes[0].drone_inlet_travel_mm = 251;
        assert!(!provenance_valid(&candidate));
        candidate = read();
        let barrier = candidate.services[0]
            .keyframes
            .iter_mut()
            .find(|f| f.frame.milestones.ore_coupled)
            .expect("proved transfer aperture");
        barrier.drone_inlet_travel_mm = 0;
        assert!(!provenance_valid(&candidate));
        candidate = read();
        candidate.geometry.idle_machinery.pop();
        assert!(!provenance_valid(&candidate));
        let mut bytes = FIXTURE_BYTES.to_vec();
        bytes.push(b' ');
        assert!(parse_fixture(&bytes).is_none());
    }

    #[test]
    fn physical_routes_keep_unproved_bays_classes_and_staging_closed() {
        let physical = physical_fixture().unwrap();
        let definition = &physical.geometry.definition;
        assert!(physical.geometry.stages.is_empty());
        for class in [DroneClass::Mining, DroneClass::Recon, DroneClass::Attack] {
            for berth in 0..10 {
                assert!(definition
                    .stage_route(
                        class,
                        u16::from(berth),
                        berth,
                        u64::from(berth) + 10,
                        u64::from(berth)
                    )
                    .is_none());
                assert!(definition
                    .stage_handoff_route(
                        class,
                        u16::from(berth),
                        berth,
                        u64::from(berth) + 10,
                        u64::from(berth)
                    )
                    .is_none());
                assert!(definition
                    .reverse_stage_route(
                        class,
                        berth,
                        u16::from(berth),
                        u64::from(berth),
                        u64::from(berth) + 10
                    )
                    .is_none());
                for cassette in [u64::from(berth), u64::from(berth) + 10] {
                    assert_eq!(
                        definition
                            .physical_inspection_service_route(berth, class, cassette)
                            .is_some(),
                        berth == 0 && class == DroneClass::Mining && cassette == 0
                    );
                }
                assert!(!definition.supports_ore_service(berth, class));
                for action in [
                    CarrierInventoryAction::Launch,
                    CarrierInventoryAction::RecallToStorage,
                    CarrierInventoryAction::ReturnToUnload {
                        resume_mining: false,
                    },
                ] {
                    for cassette in [u64::from(berth), u64::from(berth) + 10] {
                        let route = definition.bay_route(berth, class, cassette, action);
                        assert_eq!(
                            route.is_some(),
                            berth == 0
                                && class == DroneClass::Mining
                                && cassette == 0
                                && !matches!(action, CarrierInventoryAction::ReturnToUnload { .. })
                        );
                    }
                }
            }
        }
        let mut changed = definition.clone();
        changed.mechanisms[0].reference_pose.position_m[0] += 0.01;
        assert!(!changed.has_complete_compound_collision());
        assert!(changed
            .bay_route(0, DroneClass::Mining, 0, CarrierInventoryAction::Launch)
            .is_none());
        assert!(definition
            .bay_route(0, DroneClass::Mining, 1, CarrierInventoryAction::Launch)
            .is_none());
    }
}
