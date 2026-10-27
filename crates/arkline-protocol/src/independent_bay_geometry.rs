//! Source-bound independent-bay geometry contract.
//!
//! A definition is not admitted until its exact authored source, contact and
//! continuous sweep receipts are accepted. This module never supplies authority
//! clocks, ore transfer, capture completion, or a generic caller-defined hull.
use crate::drone_bay_geometry::BayPose;
use crate::{DroneClass, IndependentBayAction, IndependentBayPhase, IndependentBayRegistration};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayDefinition {
    pub registration: IndependentBayRegistration,
    pub bays: Vec<IndependentBayGeometry>,
    pub carrier_bounds_m: [[f64; 3]; 2],
    pub deployed_hardware_bounds_m: [[f64; 3]; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayGeometry {
    pub bay_id: u8,
    pub compatible_classes: Vec<DroneClass>,
    pub preferred_home: u8,
    pub ore_transfer_capable: bool,
    pub stored_pose: BayPose,
    pub support_pose: BayPose,
    pub contact_pose: BayPose,
    pub gate_pose: BayPose,
    pub clear_pose: BayPose,
    pub mechanisms: Vec<IndependentBayMechanism>,
    pub working_bounds_m: [[f64; 3]; 2],
    pub drone_bounds_local_m: [[f64; 3]; 2],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayMechanism {
    pub semantic_id: String,
    pub reference_pose: BayPose,
    pub local_bounds_m: [[f64; 3]; 2],
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayMilestones {
    pub door_clear: bool,
    pub supported: bool,
    pub clamped: bool,
    pub coupled: bool,
    pub exterior_clear: bool,
    pub hardware_safe: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayKeyframe {
    pub label: String,
    pub phase: IndependentBayPhase,
    pub drone_pose: BayPose,
    pub mechanism_poses: Vec<(String, BayPose)>,
    pub milestones: IndependentBayMilestones,
}

/// No candidate is silently admitted while authored geometry is under review.
/// Exact registered entries are added only with source/contact/sweep receipts.
pub fn lookup(
    _registration: &IndependentBayRegistration,
) -> Option<&'static IndependentBayDefinition> {
    None
}
#[derive(Deserialize)]
struct Fixture {
    definition: IndependentBayDefinition,
    routes: Vec<FixtureRoutes>,
}
#[derive(Deserialize)]
struct FixtureRoutes {
    bay_id: u8,
    routes: std::collections::BTreeMap<String, Vec<IndependentBayKeyframe>>,
}
fn stationary_fixture() -> Option<&'static Fixture> {
    static FIXTURE: std::sync::OnceLock<Option<Fixture>> = std::sync::OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            serde_json::from_str(include_str!(
                "independent_bay_geometry/stationary_v1601.json"
            ))
            .ok()
        })
        .as_ref()
}
/// Explicit source-bound fixture for stationary mechanism integration evidence.
/// This does NOT admit a world, enable travel, or replace `lookup` validation.
/// The full-hull reaction mounts, travel envelope and entry consumers remain
/// separate acceptance gates before normal registered preview admission.
#[doc(hidden)]
pub fn stationary_fixture_definition() -> Option<&'static IndependentBayDefinition> {
    stationary_fixture().map(|fixture| &fixture.definition)
}
impl IndependentBayDefinition {
    /// Only the exact compiled stationary fixture can produce these routes.
    /// A caller cannot substitute coordinates or mutate a cloned definition.
    pub fn route(
        &self,
        bay_id: u8,
        action: IndependentBayAction,
    ) -> Option<Vec<IndependentBayKeyframe>> {
        let fixture = stationary_fixture()?;
        if self != &fixture.definition {
            return None;
        }
        let name = match action {
            IndependentBayAction::Launch => "launch",
            IndependentBayAction::ReturnToStorage => "return_to_storage",
            IndependentBayAction::ReturnToUnload { .. } => "return_to_unload",
        };
        fixture
            .routes
            .iter()
            .find(|row| row.bay_id == bay_id)?
            .routes
            .get(name)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stationary_source_fixture_is_not_general_admission() {
        let definition = stationary_fixture_definition().expect("compiled source fixture");
        assert!(lookup(&definition.registration).is_none());
        assert_eq!(definition.bays.len(), 15);
        assert_eq!(
            definition.registration.assembly_source_sha256,
            "c47aad375d2c56623230ca05e335fc2896ea6105318e8fdf6e1d499c5c7f4741"
        );
        assert!(definition.deployed_hardware_bounds_m[1][1] > definition.carrier_bounds_m[1][1]);
        for bay in &definition.bays {
            assert_eq!(bay.preferred_home, bay.bay_id);
            for action in [
                IndependentBayAction::Launch,
                IndependentBayAction::ReturnToStorage,
            ] {
                let route = definition.route(bay.bay_id, action).unwrap();
                assert!(route
                    .iter()
                    .all(|frame| !frame.phase.terminal() && frame.drone_pose.valid()));
                for frame in &route {
                    assert_eq!(frame.mechanism_poses.len(), bay.mechanisms.len());
                    assert!(frame.mechanism_poses.iter().all(|(id, pose)| pose.valid()
                        && bay.mechanisms.iter().any(|group| group.semantic_id == *id)));
                }
                assert!(route.last().unwrap().milestones.hardware_safe);
            }
            let service = definition.route(
                bay.bay_id,
                IndependentBayAction::ReturnToUnload {
                    resume_mining: true,
                },
            );
            assert_eq!(service.is_some(), bay.ore_transfer_capable);
            if let Some(service) = service {
                assert!(service.iter().any(|frame| frame.milestones.coupled));
                assert!(!service.iter().any(|frame| frame.milestones.supported));
            }
        }
        let mut changed = definition.clone();
        changed.bays[0].stored_pose.position_m[0] += 0.1;
        assert!(changed.route(0, IndependentBayAction::Launch).is_none());
    }
}
