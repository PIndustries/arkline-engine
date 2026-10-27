//! Immutable receipt for a bounded, fixed-world selected-bay inspection.
//!
//! This checks the compiled certificate and current source configuration. It
//! does not rerun offline geometry proofs, admit arbitrary external obstacles,
//! certify mechanics ratings, or open normal gameplay source lookup.
use super::{PhysicalBayBinding, PHYSICAL_BAY_VERSION};
use crate::{
    carrier_inventory_geometry::{stationary_fixture_for_registration, CarrierInventoryDefinition},
    physical_cargo::source::{physical_source_configuration, ASSEMBLY_MANIFEST_SHA256},
    CarrierInventoryRegistration,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

const FIXTURE: &[u8] = include_bytes!("../../carrier_inventory_geometry/stationary_v1704.json");
const FIXTURE_SHA256: &str = "0c1c62c55330d236df9fae60d551e6840ece044e761be2749829a3d2c7bcbd35";
const CERTIFICATE: &[u8] = include_bytes!("selected-bay-source-certificate.json");
const CERTIFICATE_SHA256: &str = "86cae005624e1f528e8a48d8a2422634506d4dba8f81c67da991d1275eeff688";
const TARGET_MM: [i64; 3] = [-26_000, -100, -48_800];
const TARGET_RADIUS_MM: u32 = 1_000;
const MINING_RANGE_MM: u32 = 12_000;
// Bounded inspection scheduling input, not a measured actuator speed/rating.
const MAX_SEGMENT_MS: u64 = 60_000;

pub fn physical_definition() -> Option<&'static CarrierInventoryDefinition> {
    static REGISTRATION: OnceLock<Option<CarrierInventoryRegistration>> = OnceLock::new();
    let registration = REGISTRATION
        .get_or_init(|| {
            if hex::encode(Sha256::digest(FIXTURE)) != FIXTURE_SHA256 {
                return None;
            }
            let value: Value = serde_json::from_slice(FIXTURE).ok()?;
            serde_json::from_value(
                value
                    .get("definition")?
                    .get("topology")?
                    .get("registration")?
                    .clone(),
            )
            .ok()
        })
        .as_ref()?;
    stationary_fixture_for_registration(registration)
}

fn certificate_from_bytes(bytes: &[u8]) -> Option<Value> {
    if hex::encode(Sha256::digest(bytes)) != CERTIFICATE_SHA256 {
        return None;
    }
    let value: Value = serde_json::from_slice(bytes).ok()?;
    let source = physical_source_configuration()?;
    let text = |key: &str| value.get(key).and_then(Value::as_str);
    let number = |key: &str| value.get(key).and_then(Value::as_u64);
    let yes = |key: &str| value.get(key).and_then(Value::as_bool) == Some(true);
    if number("schema_version") != Some(1)
        || number("candidate_revision") != Some(1704)
        || number("bay_id") != Some(0)
        || number("cassette_id") != Some(0)
        || text("class") != Some("mining")
        || text("fixture_sha256") != Some(FIXTURE_SHA256)
        || text("assembly_manifest_sha256") != Some(ASSEMBLY_MANIFEST_SHA256)
        || text("source_configuration_sha256") != source.identity_sha256().as_deref()
        || text("ship_glb_sha256") != Some(source.assets.get("ship_glb")?.sha256.as_str())
        || text("donor_glb_sha256") != Some(source.assets.get("drone_glb")?.sha256.as_str())
        || value.get("normal_gameplay_admitted")?.as_bool() != Some(false)
        || !yes("partition_complete")
        || !yes("selected_material_sweep_beam_join_pass")
        || number("unclassified_export_components") != Some(0)
        || number("total_export_triangles") != Some(199_632)
        || number("dynamic_export_triangles") != Some(62_220)
        || !value
            .get("local_static_mesh_failures")?
            .as_array()?
            .is_empty()
        || value.get("route_joins")?.as_object()?.len() != 4
        || value
            .get("route_joins")?
            .as_object()?
            .values()
            .any(|v| v.as_bool() != Some(true))
    {
        return None;
    }
    let host = value.get("host_constraints")?;
    if host.get("all_carrier_roots")?.as_u64() != Some(500)
        || host.get("frozen_carrier_root_count")?.as_u64() != Some(481)
        || host.get("active_roots")?.as_array()?.len() != 19
        || host
            .get("other_bays_classes_reserve_staging_warp_closed")?
            .as_bool()
            != Some(true)
    {
        return None;
    }
    let mining = value.get("mining_fixture")?;
    if mining.get("target_center_mm")? != &serde_json::json!(TARGET_MM)
        || mining.get("target_radius_mm")?.as_u64() != Some(u64::from(TARGET_RADIUS_MM))
        || mining.get("mining_range_mm")?.as_u64() != Some(u64::from(MINING_RANGE_MM))
        || !mining
            .get("target_and_ray_obstruction_failures")?
            .as_array()?
            .is_empty()
    {
        return None;
    }
    physical_definition()?;
    Some(value)
}
fn certificate() -> Option<&'static Value> {
    static CERTIFICATE_VALUE: OnceLock<Option<Value>> = OnceLock::new();
    CERTIFICATE_VALUE
        .get_or_init(|| certificate_from_bytes(CERTIFICATE))
        .as_ref()
}

pub fn certified_binding(binding: &PhysicalBayBinding) -> bool {
    let Some(certificate) = certificate() else {
        return false;
    };
    binding.version == PHYSICAL_BAY_VERSION
        && binding.drone_id > 0
        && binding.berth_id == 0
        && binding.cassette_id == 0
        && (1..=MAX_SEGMENT_MS).contains(&binding.segment_duration_ms)
        && binding.fixture_sha256 == FIXTURE_SHA256
        && binding.selected_bay_certificate_sha256 == CERTIFICATE_SHA256
        && certificate
            .get("source_configuration_sha256")
            .and_then(Value::as_str)
            == Some(binding.source_configuration_sha256.as_str())
        && binding.target_center_mm == TARGET_MM
        && binding.target_radius_mm == TARGET_RADIUS_MM
        && binding.mining_range_mm == MINING_RANGE_MM
}

/// Authored inspection timing/energy inputs remain explicit; this does not set
/// a gameplay battery size or actuator rating. Target/source are not caller-set.
pub fn certified_inspection_binding(
    drone_id: u64,
    segment_duration_ms: u64,
    initial_energy_units: u64,
) -> Option<PhysicalBayBinding> {
    let certificate = certificate()?;
    let binding = PhysicalBayBinding {
        version: PHYSICAL_BAY_VERSION,
        source_configuration_sha256: certificate
            .get("source_configuration_sha256")?
            .as_str()?
            .to_owned(),
        fixture_sha256: FIXTURE_SHA256.into(),
        selected_bay_certificate_sha256: CERTIFICATE_SHA256.into(),
        drone_id,
        berth_id: 0,
        cassette_id: 0,
        segment_duration_ms,
        initial_energy_units,
        target_center_mm: TARGET_MM,
        target_radius_mm: TARGET_RADIUS_MM,
        mining_range_mm: MINING_RANGE_MM,
    };
    certified_binding(&binding).then_some(binding)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_receipt_opens_only_selected_fixed_world_inspection() {
        let good = certified_inspection_binding(1, 1000, 20).unwrap();
        assert!(certified_binding(&good));
        assert!(crate::carrier_inventory_geometry::lookup(
            &physical_definition().unwrap().topology.registration
        )
        .is_none());
        for field in [
            "version",
            "drone_id",
            "berth_id",
            "cassette_id",
            "segment_duration_ms",
            "target_radius_mm",
            "mining_range_mm",
        ] {
            let mut value = serde_json::to_value(&good).unwrap();
            value[field] =
                serde_json::json!(if field == "drone_id" || field == "segment_duration_ms" {
                    0
                } else {
                    99
                });
            let wrong: PhysicalBayBinding = serde_json::from_value(value).unwrap();
            assert!(!certified_binding(&wrong), "{field}");
        }
        for field in [
            "fixture_sha256",
            "selected_bay_certificate_sha256",
            "source_configuration_sha256",
        ] {
            let mut value = serde_json::to_value(&good).unwrap();
            value[field] = serde_json::json!("0".repeat(64));
            assert!(
                !certified_binding(&serde_json::from_value(value).unwrap()),
                "{field}"
            );
        }
        let mut wrong = good;
        wrong.target_center_mm[2] += 1;
        assert!(!certified_binding(&wrong));
        assert!(certified_inspection_binding(1, MAX_SEGMENT_MS + 1, 20).is_none());
    }
    #[test]
    fn certificate_bytes_and_claimed_proof_cannot_be_substituted() {
        assert!(certificate_from_bytes(CERTIFICATE).is_some());
        let mut bytes = CERTIFICATE.to_vec();
        bytes.push(b' ');
        assert!(certificate_from_bytes(&bytes).is_none());
        let mut value: Value = serde_json::from_slice(CERTIFICATE).unwrap();
        value["partition_complete"] = serde_json::json!(false);
        assert!(certificate_from_bytes(&serde_json::to_vec(&value).unwrap()).is_none());
        value["partition_complete"] = serde_json::json!(true);
        value["normal_gameplay_admitted"] = serde_json::json!(true);
        assert!(certificate_from_bytes(&serde_json::to_vec(&value).unwrap()).is_none());
    }
}
