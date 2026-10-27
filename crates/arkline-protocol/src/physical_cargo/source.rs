//! Exact opt-in geometry configuration, not carrier/flight or operating admission.
//!
//! The receipts establish conservative geometric volume for one immutable source
//! pair. They do not establish density, load/pressure/flow ratings, operating
//! rates, concurrent handling, return routes or warp acceptance. Legacy sources
//! and prototype units are not accepted by this registry.
use super::catalog::CargoSourceMeasurements;
use super::{MeasuredCargoHold, UsableCargoCellMm, VolumeCm3};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::OnceLock;

pub const PHYSICAL_SOURCE_SCHEMA_VERSION: u32 = 1;
pub const PHYSICAL_CANDIDATE_REVISION: u32 = 1704;
pub const ASSEMBLY_MANIFEST_SHA256: &str =
    "5315dedb22972d1429cb23cc2d53b81836c416d30ff106fa266523e1671f22f2";
pub const SHIP_GEOMETRIC_CAPACITY_CM3: VolumeCm3 = VolumeCm3(214_121_238);
pub const DRONE_GEOMETRIC_CAPACITY_CM3: VolumeCm3 = VolumeCm3(893_833);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalSourceAsset {
    /// Original proof-package-relative path. This is provenance, not an allowed
    /// arbitrary filesystem path for a loader to open.
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhysicalCargoSourceConfiguration {
    pub schema_version: u32,
    /// Dedicated candidate identity; never a declaration that carrier geometry
    /// revision 1703 may be reused for this changed source.
    pub candidate_revision: u32,
    pub assembly_manifest_sha256: String,
    pub assets: BTreeMap<String, PhysicalSourceAsset>,
    pub receiver_panel_travel_mm: u32,
    pub measured_holds: CargoSourceMeasurements,
}

impl PhysicalCargoSourceConfiguration {
    /// Exact complete equality, not shape or caller-reported hash acceptance.
    pub fn validates_exact(&self) -> bool {
        physical_source_configuration() == Some(self)
    }

    pub fn identity_sha256(&self) -> Option<String> {
        self.validates_exact()
            .then(|| digest(&serde_json::to_vec(self).expect("integer source configuration")))
    }

    pub fn measurements(&self) -> Option<CargoSourceMeasurements> {
        self.validates_exact().then(|| self.measured_holds.clone())
    }

    /// Consumers must verify actual source/GLB bytes before installing them.
    /// A valid configuration alone never verifies a file supplied separately.
    pub fn validates_asset_bytes(&self, role: &str, bytes: &[u8]) -> bool {
        self.validates_exact()
            && self
                .assets
                .get(role)
                .is_some_and(|asset| digest(bytes) == asset.sha256)
    }
}

/// This is an explicit geometry lookup only. Normal carrier admission remains
/// separate and fail-closed; no world factory selects this configuration by default.
pub fn physical_source_configuration() -> Option<&'static PhysicalCargoSourceConfiguration> {
    static SOURCE: OnceLock<Option<PhysicalCargoSourceConfiguration>> = OnceLock::new();
    SOURCE.get_or_init(build_configuration).as_ref()
}

const ASSEMBLY: &[u8] = include_bytes!("fixtures/pioneer_geometry_1704/assembly-manifest.json");
const SHIP_CELLS: &[u8] = include_bytes!("fixtures/pioneer_geometry_1704/ship_capacity_cells.json");
const DRONE_PROOF: &[u8] =
    include_bytes!("fixtures/pioneer_geometry_1704/drone_full_cell_path_valve.json");
const MOTION: &[u8] =
    include_bytes!("fixtures/pioneer_geometry_1704/receiver_motion_contract.json");
const EMBEDDED_PROOFS: &[(&str, &[u8])] = &[
    ("ship_capacity_cells", SHIP_CELLS),
    ("drone_full_cell_path_valve", DRONE_PROOF),
    ("receiver_motion_contract", MOTION),
    (
        "ship_full_capacity_proof",
        include_bytes!("fixtures/pioneer_geometry_1704/ship_full_capacity_proof.json"),
    ),
    (
        "drone_receiver_interface",
        include_bytes!("fixtures/pioneer_geometry_1704/drone_receiver_interface.json"),
    ),
    (
        "receiver_motion_proof",
        include_bytes!("fixtures/pioneer_geometry_1704/receiver_motion_proof.json"),
    ),
    (
        "receiver_fixed_joint",
        include_bytes!("fixtures/pioneer_geometry_1704/receiver_fixed_joint.json"),
    ),
    (
        "all20_body_routes",
        include_bytes!("fixtures/pioneer_geometry_1704/all20_body_routes.json"),
    ),
    (
        "fixed_network_path",
        include_bytes!("fixtures/pioneer_geometry_1704/fixed_network_path.json"),
    ),
    (
        "export_preservation",
        include_bytes!("fixtures/pioneer_geometry_1704/export_preservation.json"),
    ),
];
fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn build_configuration() -> Option<PhysicalCargoSourceConfiguration> {
    if digest(ASSEMBLY) != ASSEMBLY_MANIFEST_SHA256 {
        return None;
    }
    let manifest: serde_json::Value = serde_json::from_slice(ASSEMBLY).ok()?;
    let assets: BTreeMap<String, PhysicalSourceAsset> =
        serde_json::from_value(manifest.get("inputs")?.clone()).ok()?;
    // All selected evidence bytes are checked, not merely their claimed status.
    for (role, bytes) in EMBEDDED_PROOFS {
        if digest(bytes) != assets.get(*role)?.sha256 {
            return None;
        }
    }
    let cells: serde_json::Value = serde_json::from_slice(SHIP_CELLS).ok()?;
    let ship_cells = cells
        .get("usable_cells_mm")?
        .as_array()?
        .iter()
        .map(|c| {
            Some(UsableCargoCellMm {
                min: serde_json::from_value(c.get("min")?.clone()).ok()?,
                max: serde_json::from_value(c.get("max")?.clone()).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let drone: serde_json::Value = serde_json::from_slice(DRONE_PROOF).ok()?;
    let box_m = drone.get("capacity")?.get("box_m")?.as_array()?;
    if box_m.len() != 2 {
        return None;
    }
    let drone_cell = UsableCargoCellMm {
        min: rational_metres_to_mm(box_m.first()?)?,
        max: rational_metres_to_mm(box_m.get(1)?)?,
    };
    let motion: serde_json::Value = serde_json::from_slice(MOTION).ok()?;
    let travel_mm = u32::try_from(motion.get("parameter")?.get("travel_mm")?.as_u64()?).ok()?;
    if travel_mm != 94
        || motion.get("geometry_glb_sha256")?.as_str()? != assets.get("ship_glb")?.sha256
        || motion.get("source_blend_sha256")?.as_str()? != assets.get("ship_source")?.sha256
        || cells.get("source_sha256")?.as_str()? != assets.get("ship_source")?.sha256
        || cells.get("glb_sha256")?.as_str()? != assets.get("ship_glb")?.sha256
        || drone.get("source_sha256")?.as_str()? != assets.get("drone_source")?.sha256
        || drone.get("glb_sha256")?.as_str()? != assets.get("drone_glb")?.sha256
    {
        return None;
    }
    let measured_holds = CargoSourceMeasurements {
        ship_source_sha256: assets.get("ship_source")?.sha256.clone(),
        mining_drone_source_sha256: assets.get("drone_source")?.sha256.clone(),
        ship_hold: MeasuredCargoHold {
            source_hold_id: "pioneer.physical-1704.ship-hold".into(),
            measurement_receipt_sha256: assets.get("ship_full_capacity_proof")?.sha256.clone(),
            usable_cells: ship_cells,
        },
        mining_drone_hold: MeasuredCargoHold {
            source_hold_id: "pioneer.physical-1704.mining-drone-hold".into(),
            measurement_receipt_sha256: assets.get("drone_full_cell_path_valve")?.sha256.clone(),
            usable_cells: vec![drone_cell],
        },
    };
    if measured_holds.ship_hold.usable_cells.len() != 43
        || measured_holds.ship_hold.capacity_cm3()? != SHIP_GEOMETRIC_CAPACITY_CM3
        || measured_holds.mining_drone_hold.capacity_cm3()? != DRONE_GEOMETRIC_CAPACITY_CM3
        || manifest
            .get("capacity")?
            .get("ship_geometric_cm3")?
            .as_u64()?
            != SHIP_GEOMETRIC_CAPACITY_CM3.0
        || manifest
            .get("capacity")?
            .get("drone_geometric_cm3")?
            .as_u64()?
            != DRONE_GEOMETRIC_CAPACITY_CM3.0
    {
        return None;
    }
    Some(PhysicalCargoSourceConfiguration {
        schema_version: PHYSICAL_SOURCE_SCHEMA_VERSION,
        candidate_revision: PHYSICAL_CANDIDATE_REVISION,
        assembly_manifest_sha256: ASSEMBLY_MANIFEST_SHA256.into(),
        assets,
        receiver_panel_travel_mm: travel_mm,
        measured_holds,
    })
}

/// Convert the proof's exact rational coordinates without float rounding or
/// silently changing capacity. Non-integer-mm coordinates are refused.
fn rational_metres_to_mm(value: &serde_json::Value) -> Option<[i64; 3]> {
    let values = value.as_array()?;
    if values.len() != 3 {
        return None;
    }
    let mut result = [0; 3];
    for (index, coordinate) in values.iter().enumerate() {
        let text = coordinate.as_str()?;
        let (n, d) = text.split_once('/').unwrap_or((text, "1"));
        let numerator = n.parse::<i128>().ok()?.checked_mul(1000)?;
        let denominator = d.parse::<i128>().ok()?;
        if denominator <= 0 || numerator % denominator != 0 {
            return None;
        }
        result[index] = i64::try_from(numerator / denominator).ok()?;
    }
    Some(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_provenance_uses_portable_proof_package_paths() {
        let receipt: serde_json::Value = serde_json::from_slice(include_bytes!(
            "fixtures/pioneer_geometry_1704/export_preservation.json"
        ))
        .unwrap();
        let files = receipt["files"].as_object().unwrap();
        assert_eq!(files.len(), 3);
        for path in files.keys() {
            let path = std::path::Path::new(path);
            assert!(!path.is_absolute());
            assert!(path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_))));
        }
        let source = physical_source_configuration().expect("pinned source proof bytes");
        assert!(files
            .values()
            .any(|hash| hash.as_str() == Some(source.assets["ship_glb"].sha256.as_str())));
    }

    #[test]
    fn exact_geometry_identity_preserves_units_cells_and_motion() {
        let s = physical_source_configuration().expect("pinned source proof bytes");
        assert!(s.validates_exact());
        assert_eq!(s.candidate_revision, 1704);
        assert_eq!(s.receiver_panel_travel_mm, 94);
        let m = s.measurements().unwrap();
        assert_eq!(m.ship_hold.capacity_cm3(), Some(VolumeCm3(214_121_238)));
        assert_eq!(m.mining_drone_hold.capacity_cm3(), Some(VolumeCm3(893_833)));
        assert_eq!(
            m.mining_drone_hold.usable_cells[0],
            UsableCargoCellMm {
                min: [-1340, -129, -639],
                max: [-40, 409, 639],
            }
        );
        // Per-cell flooring is intentionally18cm³ below aggregate flooring.
        let aggregate_mm3: u128 = m
            .ship_hold
            .usable_cells
            .iter()
            .map(|c| {
                (0..3)
                    .map(|i| (c.max[i] - c.min[i]) as u128)
                    .product::<u128>()
            })
            .sum();
        assert_eq!(aggregate_mm3 / 1000, 214_121_256);
        let encoded = serde_json::to_vec(s).unwrap();
        assert_eq!(
            serde_json::from_slice::<PhysicalCargoSourceConfiguration>(&encoded).unwrap(),
            *s
        );
        assert_eq!(s.identity_sha256(), Some(digest(&encoded)));
    }

    #[test]
    fn no_legacy_or_mutated_source_proof_capacity_or_valve_is_accepted() {
        let s = physical_source_configuration().unwrap();
        for change in 0..10 {
            let mut bad = s.clone();
            match change {
                0 => bad.candidate_revision = 1703,
                1 => bad.schema_version += 1,
                2 => bad.receiver_panel_travel_mm = 95,
                3 => bad.assembly_manifest_sha256 = "a".repeat(64),
                4 => bad.assets.get_mut("ship_glb").unwrap().sha256 = "b".repeat(64),
                5 => {
                    bad.assets
                        .get_mut("drone_full_cell_path_valve")
                        .unwrap()
                        .sha256 = "c".repeat(64)
                }
                6 => bad.measured_holds.ship_hold.usable_cells[0].max[0] += 1,
                7 => {
                    bad.measured_holds
                        .mining_drone_hold
                        .measurement_receipt_sha256 = "d".repeat(64)
                }
                8 => {
                    bad.assets.remove("receiver_motion_contract");
                }
                _ => bad.assets.get_mut("ship_glb").unwrap().path = "another.glb".into(),
            }
            assert!(!bad.validates_exact(), "mutation {change}");
            assert!(bad.identity_sha256().is_none());
            assert!(bad.measurements().is_none());
            assert!(!bad.validates_asset_bytes("ship_capacity_cells", SHIP_CELLS));
        }
    }

    #[test]
    fn supplied_bytes_must_match_the_bound_asset_not_just_a_claimed_hash() {
        let s = physical_source_configuration().unwrap();
        for (role, bytes) in EMBEDDED_PROOFS {
            assert!(s.validates_asset_bytes(role, bytes));
            let mut changed = bytes.to_vec();
            changed[0] ^= 1;
            assert!(!s.validates_asset_bytes(role, &changed));
        }
        assert!(!s.validates_asset_bytes("unknown", SHIP_CELLS));
        assert!(!s.validates_asset_bytes("ship_glb", SHIP_CELLS));
        assert!(!s.validates_asset_bytes("drone_glb", &[]));
    }

    #[test]
    fn exact_coordinate_conversion_refuses_fractional_mm_and_bad_shape() {
        for v in [
            serde_json::json!(["1/10000", "0", "0"]),
            serde_json::json!(["1/0", "0", "0"]),
            serde_json::json!(["1/-2", "0", "0"]),
            serde_json::json!(["1", "0"]),
            serde_json::json!([0, 0, 0]),
        ] {
            assert!(rational_metres_to_mm(&v).is_none());
        }
    }
}
