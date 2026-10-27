//! Internal, team-authored assemblies only. The immutable source registration is
//! the lookup key; arbitrary coordinates/hashes supplied by a client or save are
//! not definitions. This first catalog contains only the existing registration12.
//! Slot/group counts and the serialized rear gantry topology are intentionally
//! fixed. Adding a source requires independent authored clearance and consumers.
use crate::drone_bay_geometry::BayPose;
use crate::drone_bay_layout::{self as layout, BayEnvelope, BayMechanismId, BayRoute};
use crate::{DroneBayBinding, DroneBayRegistration, DroneClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OfficialBayTopology {
    SerializedRearGantryV1,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BaySlotDefinition {
    pub home_cradle: u8,
    pub home_pose: BayPose,
    pub expected_class: DroneClass,
    pub external_clear_pose: BayPose,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BayMechanismDefinition {
    pub id: BayMechanismId,
    pub semantic_id: &'static str,
    pub reference_pose: BayPose,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BayRigDefinition {
    pub idle_support: BayPose,
    pub aisle_z_m: f64,
    pub support_release_y_m: f64,
    pub fork_stage_travel_m: f64,
    pub door_travel_m: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OfficialBayHost {
    pub geometry_revision: u32,
    pub source_sha256: &'static str,
}
#[derive(Debug, PartialEq)]
pub struct OfficialBayDefinition {
    /// Host traversal/source contract is distinct from an assembly identity.
    /// The only installed entry is still the exact legacy whole-source binding.
    pub compatible_host: OfficialBayHost,
    pub geometry_revision: u32,
    pub source_sha256: &'static str,
    pub blend_sha256: &'static str,
    pub topology: OfficialBayTopology,
    pub slots: [BaySlotDefinition; layout::DRONE_BAY_SLOT_COUNT],
    pub mechanisms: [BayMechanismDefinition; layout::DRONE_BAY_MECHANISM_COUNT],
    pub rig: BayRigDefinition,
    pub external_hold: BayPose,
    pub working_envelope: BayEnvelope,
    pub carrier_envelope: BayEnvelope,
    pub aperture_bounds_m: [[f64; 3]; 2],
    pub drone_half_extents_m: [f64; 3],
}
impl OfficialBayDefinition {
    pub fn registration(&self) -> DroneBayRegistration {
        DroneBayRegistration {
            geometry_revision: self.geometry_revision,
            source_sha256: self.source_sha256.into(),
        }
    }
    pub fn slot(&self, id: u8) -> Option<&BaySlotDefinition> {
        self.slots
            .get(usize::from(id))
            .filter(|slot| slot.home_cradle == id)
    }
    pub fn home_poses(&self) -> [BayPose; layout::DRONE_BAY_SLOT_COUNT] {
        self.slots.map(|slot| slot.home_pose)
    }
    pub fn mechanism_ids(&self) -> [&'static str; layout::DRONE_BAY_MECHANISM_COUNT] {
        self.mechanisms.map(|group| group.semantic_id)
    }
    pub fn reference_poses(&self) -> [BayPose; layout::DRONE_BAY_MECHANISM_COUNT] {
        self.mechanisms.map(|group| group.reference_pose)
    }
    pub fn mechanism(&self, id: BayMechanismId) -> &BayMechanismDefinition {
        &self.mechanisms[id.index()]
    }
    pub fn launch_route(&'static self, slot: u8) -> Option<BayRoute> {
        layout::launch_route_for(self, slot)
    }
    pub fn recovery_route(&'static self, slot: u8) -> Option<BayRoute> {
        layout::recovery_route_for(self, slot)
    }
}
/// Returns only a compiled, reviewed official entry. Well-formed unknown source
/// hashes/revisions fail closed; no fallback or default rebinding is permitted.
pub fn official_bay_definition(
    registration: &DroneBayRegistration,
) -> Option<&'static OfficialBayDefinition> {
    (registration.geometry_revision == PIONEER_BAY_REG12.geometry_revision
        && registration.source_sha256 == PIONEER_BAY_REG12.source_sha256)
        .then_some(&PIONEER_BAY_REG12)
}
pub fn official_bay_for_binding(
    binding: &DroneBayBinding,
) -> Option<&'static OfficialBayDefinition> {
    binding.well_formed().then_some(())?;
    official_bay_definition(&DroneBayRegistration {
        geometry_revision: binding.geometry_revision,
        source_sha256: binding.source_sha256.clone(),
    })
}
const fn root(position_m: [f64; 3]) -> BayPose {
    BayPose {
        position_m,
        orientation_xyzw: [0., 0., 0., 1.],
    }
}
const fn rear(position_m: [f64; 3]) -> BayPose {
    BayPose {
        position_m,
        orientation_xyzw: [0., 1., 0., 0.],
    }
}
const fn slot(id: u8, home: [f64; 3], class: DroneClass, clear: [f64; 3]) -> BaySlotDefinition {
    BaySlotDefinition {
        home_cradle: id,
        home_pose: rear(home),
        expected_class: class,
        external_clear_pose: rear(clear),
    }
}
const fn group(
    id: BayMechanismId,
    semantic_id: &'static str,
    position: [f64; 3],
) -> BayMechanismDefinition {
    BayMechanismDefinition {
        id,
        semantic_id,
        reference_pose: root(position),
    }
}
/// Existing authored source: coordinates and semantic identities are immutable.
/// In particular, this is not a newly seeded layout or a save migration.
pub static PIONEER_BAY_REG12: OfficialBayDefinition = OfficialBayDefinition {
    compatible_host: OfficialBayHost {
        geometry_revision: 12,
        source_sha256: "7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd",
    },
    geometry_revision: 12,
    source_sha256: "7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd",
    blend_sha256: "ee43c05cb1d4ebf907d609b5338b4d8a911b812d8fb05914c96ca48dbb06b1d9",
    topology: OfficialBayTopology::SerializedRearGantryV1,
    slots: [
        slot(
            0,
            [-30.5, -5.4, 3.5],
            DroneClass::Recon,
            [-100., -25.0, -51.5],
        ),
        slot(
            1,
            [-30.5, -3.2, 3.5],
            DroneClass::Recon,
            [-100., -25.0, -27.5],
        ),
        slot(
            2,
            [-30.5, -1.0, 3.5],
            DroneClass::Recon,
            [-100., -25.0, -3.5],
        ),
        slot(
            3,
            [-30.5, 1.2, 3.5],
            DroneClass::Recon,
            [-100., -25.0, 20.5],
        ),
        slot(
            4,
            [-30.5, 3.4, 3.5],
            DroneClass::Recon,
            [-100., -25.0, 44.5],
        ),
        slot(
            5,
            [-24.5, -5.4, 3.5],
            DroneClass::Mining,
            [-100., -1.0, -51.5],
        ),
        slot(
            6,
            [-24.5, -3.2, 3.5],
            DroneClass::Mining,
            [-100., -1.0, -27.5],
        ),
        slot(
            7,
            [-24.5, -1.0, 3.5],
            DroneClass::Mining,
            [-100., -1.0, -3.5],
        ),
        slot(
            8,
            [-24.5, 1.2, 3.5],
            DroneClass::Mining,
            [-100., -1.0, 20.5],
        ),
        slot(
            9,
            [-24.5, 3.4, 3.5],
            DroneClass::Mining,
            [-100., -1.0, 44.5],
        ),
        slot(
            10,
            [-18.5, -5.4, 3.5],
            DroneClass::Attack,
            [-100., 23.0, -51.5],
        ),
        slot(
            11,
            [-18.5, -3.2, 3.5],
            DroneClass::Attack,
            [-100., 23.0, -27.5],
        ),
        slot(
            12,
            [-18.5, -1.0, 3.5],
            DroneClass::Attack,
            [-100., 23.0, -3.5],
        ),
        slot(
            13,
            [-18.5, 1.2, 3.5],
            DroneClass::Attack,
            [-100., 23.0, 20.5],
        ),
        slot(
            14,
            [-18.5, 3.4, 3.5],
            DroneClass::Attack,
            [-100., 23.0, 44.5],
        ),
    ],
    mechanisms: [
        group(BayMechanismId::Tray00, "tray/s00", [-30.5, -5.4, 3.5]),
        group(BayMechanismId::Tray01, "tray/s01", [-30.5, -3.2, 3.5]),
        group(BayMechanismId::Tray02, "tray/s02", [-30.5, -1.0, 3.5]),
        group(BayMechanismId::Tray03, "tray/s03", [-30.5, 1.2, 3.5]),
        group(BayMechanismId::Tray04, "tray/s04", [-30.5, 3.4, 3.5]),
        group(BayMechanismId::Tray05, "tray/s05", [-24.5, -5.4, 3.5]),
        group(BayMechanismId::Tray06, "tray/s06", [-24.5, -3.2, 3.5]),
        group(BayMechanismId::Tray07, "tray/s07", [-24.5, -1.0, 3.5]),
        group(BayMechanismId::Tray08, "tray/s08", [-24.5, 1.2, 3.5]),
        group(BayMechanismId::Tray09, "tray/s09", [-24.5, 3.4, 3.5]),
        group(BayMechanismId::Tray10, "tray/s10", [-18.5, -5.4, 3.5]),
        group(BayMechanismId::Tray11, "tray/s11", [-18.5, -3.2, 3.5]),
        group(BayMechanismId::Tray12, "tray/s12", [-18.5, -1.0, 3.5]),
        group(BayMechanismId::Tray13, "tray/s13", [-18.5, 1.2, 3.5]),
        group(BayMechanismId::Tray14, "tray/s14", [-18.5, 3.4, 3.5]),
        group(BayMechanismId::CarriageX, "carriage_x", [-30.5, -1., -3.5]),
        group(BayMechanismId::LiftXy, "lift_xy", [-30.5, -1., -3.5]),
        group(
            BayMechanismId::ForkStage0,
            "fork_stage0",
            [-30.5, -1., -3.5],
        ),
        group(
            BayMechanismId::ForkStage1,
            "fork_stage1",
            [-30.5, -1., -3.5],
        ),
        group(
            BayMechanismId::ForkStage2,
            "fork_stage2",
            [-30.5, -1., -3.5],
        ),
        group(
            BayMechanismId::DoorUpper,
            "door/upper",
            [-35.58, -0.19, -3.5],
        ),
        group(
            BayMechanismId::DoorLower,
            "door/lower",
            [-35.58, -1.81, -3.5],
        ),
    ],
    rig: BayRigDefinition {
        idle_support: root([-30.5, -1., -3.5]),
        aisle_z_m: -3.5,
        support_release_y_m: -0.20,
        fork_stage_travel_m: 3.5,
        door_travel_m: 1.70,
    },
    external_hold: rear([-40., -1., -3.5]),
    working_envelope: BayEnvelope {
        min_m: [-36., -6.72, -7.71],
        max_m: [-15.30, 5.25, 7.15],
    },
    carrier_envelope: BayEnvelope {
        min_m: [-36., -9., -19.],
        max_m: [36., 11.35, 19.],
    },
    aperture_bounds_m: [[-36., -2.6, -7.5], [-35.25, 0.6, 0.5]],
    drone_half_extents_m: [2.5, 0.9, 3.0],
};

#[cfg(test)]
#[path = "drone_bay_route_fingerprint.rs"]
mod fingerprint;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_registration_catalog_rejects_unknown_or_rebound_sources() {
        let registration = PIONEER_BAY_REG12.registration();
        assert!(std::ptr::eq(
            official_bay_definition(&registration).unwrap(),
            &PIONEER_BAY_REG12
        ));
        let mut unknown = registration.clone();
        unknown.geometry_revision += 1;
        assert!(official_bay_definition(&unknown).is_none());
        unknown = registration.clone();
        unknown.source_sha256 = "0".repeat(64);
        assert!(official_bay_definition(&unknown).is_none());
        assert!(official_bay_for_binding(&registration.bind(String::new())).is_none());
        assert_eq!(PIONEER_BAY_REG12.compatible_host.geometry_revision, 12);
        assert_eq!(
            PIONEER_BAY_REG12.compatible_host.source_sha256,
            registration.source_sha256
        );
    }
    #[test]
    fn explicit_slot_group_and_legacy_wrapper_contract_is_identical() {
        let definition = &PIONEER_BAY_REG12;
        assert_eq!(definition.slots.len(), crate::MAX_DRONES);
        assert_eq!(definition.slots.len(), 15);
        assert_eq!(definition.mechanisms.len(), 22);
        for slot in &definition.slots {
            assert_eq!(
                crate::DroneBayState::expected_class(slot.home_cradle),
                Some(slot.expected_class)
            );
            assert_eq!(layout::home_pose(slot.home_cradle), Some(slot.home_pose));
            let id = usize::from(slot.home_cradle);
            assert_eq!(
                definition.mechanisms[id].reference_pose.position_m,
                slot.home_pose.position_m
            );
            assert_eq!(
                slot.external_clear_pose.position_m,
                [
                    -100.,
                    -1. + (f64::from(slot.home_cradle / 5) - 1.) * 24.,
                    -3.5 + (f64::from(slot.home_cradle % 5) - 2.) * 24.
                ]
            );
        }
        for (index, group) in definition.mechanisms.iter().enumerate() {
            assert_eq!(group.id.index(), index);
            assert_eq!(group.id.source_group(), group.semantic_id);
            assert_eq!(group.id.reference_pose(), group.reference_pose);
        }
        assert!(definition.slot(15).is_none());
        assert!(definition.launch_route(15).is_none());
        assert!(definition.recovery_route(255).is_none());
        assert_eq!(definition.home_poses(), layout::home_poses());
        assert_eq!(definition.mechanism_ids(), layout::mechanism_ids());
        assert_eq!(definition.reference_poses(), layout::reference_poses());
        assert_eq!(
            definition.working_envelope,
            layout::DRONE_BAY_WORKING_ENVELOPE
        );
        assert_eq!(
            definition.carrier_envelope,
            layout::DRONE_BAY_CARRIER_ENVELOPE
        );
        assert_eq!(definition.external_hold, layout::DRONE_BAY_EXTERNAL_HOLD);
        assert_eq!(
            definition.drone_half_extents_m,
            layout::DRONE_BAY_DRONE_HALF_EXTENTS_M
        );
        assert_eq!(
            definition.aperture_bounds_m,
            layout::DRONE_BAY_APERTURE_BOUNDS_M
        );
    }
    #[test]
    fn all_fifteen_routes_contracts_and_samples_match_pre_refactor_source() {
        let baseline: serde_json::Value =
            serde_json::from_str(include_str!("drone_bay_reg12_route_baseline.json")).unwrap();
        assert_eq!(
            baseline["producer_commit"],
            "e9ef19e550c89f131c2ab5d73d07ede7e7030cf2"
        );
        let rows = baseline["routes"].as_array().unwrap();
        assert_eq!(rows.len(), 30);
        for row in rows {
            let slot = row["slot"].as_u64().unwrap() as u8;
            let route = match row["direction"].as_str().unwrap() {
                "Launch" => PIONEER_BAY_REG12.launch_route(slot).unwrap(),
                "Recovery" => PIONEER_BAY_REG12.recovery_route(slot).unwrap(),
                direction => panic!("unexpected baseline direction {direction}"),
            };
            assert_eq!(
                fingerprint::route_fingerprint(&route),
                row["sha256"].as_str().unwrap(),
                "slot{slot} {:?}",
                route.direction
            );
        }
    }
}
