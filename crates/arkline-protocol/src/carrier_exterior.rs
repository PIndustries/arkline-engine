//! Explicit per-vessel exterior binding for the UNADMITTED composed source.
//! Legacy definitions, electrical values and validation are unchanged.
use crate::{default_pioneer_exterior, ExteriorDefinition, ExteriorDefinitionState};
use serde::{Deserialize, Serialize};

pub const CARRIER_EXTERIOR_SOURCE: &str =
    "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113";
pub const CARRIER_EXTERIOR_ASSEMBLY_REVISION: u32 = 1702;
pub const CARRIER_EXTERIOR_DEVICE_REVISION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarrierExteriorDefinition {
    pub assembly_revision: u32,
    pub source_sha256: String,
    pub device_revision: u32,
    pub base_definition_revision: u64,
    pub document: ExteriorDefinition,
}

fn canonical_positions(document: &ExteriorDefinition) -> Result<(), String> {
    let canonical = default_pioneer_exterior();
    if document.fixtures.len() != canonical.fixtures.len() {
        return Err("carrier exterior requires the complete proved fixture topology".into());
    }
    for expected in &canonical.fixtures {
        let Some(actual) = document.fixtures.iter().find(|f| f.id == expected.id) else {
            return Err("carrier exterior fixture identity is missing".into());
        };
        if actual.kind != expected.kind || actual.position_m != expected.position_m {
            return Err("edited exterior geometry requires a new source clearance proof".into());
        }
    }
    Ok(())
}
fn nav_position(id: &str) -> Option<[f64; 3]> {
    match id {
        "pioneer.exterior.nav.port" => Some([-24., 7.2, -33.97999923706055]),
        "pioneer.exterior.nav.starboard" => Some([-24., 7.2, 33.97999923706055]),
        _ => None,
    }
}

/// The caller must independently validate the complete vessel registration and
/// actual installed source. This provider is not normal-world source admission.
pub fn for_exact_source(
    assembly_revision: u32,
    source_sha256: &str,
    base: &ExteriorDefinitionState,
) -> Result<CarrierExteriorDefinition, String> {
    if assembly_revision != CARRIER_EXTERIOR_ASSEMBLY_REVISION
        || source_sha256 != CARRIER_EXTERIOR_SOURCE
    {
        return Err("unsupported exact carrier exterior source".into());
    }
    base.document.validate()?;
    canonical_positions(&base.document)?;
    let mut document = base.document.clone();
    document.hull_asset_sha256 = source_sha256.into();
    for fixture in &mut document.fixtures {
        if let Some(position) = nav_position(&fixture.id) {
            fixture.position_m = position;
        }
    }
    let result = CarrierExteriorDefinition {
        assembly_revision,
        source_sha256: source_sha256.into(),
        device_revision: CARRIER_EXTERIOR_DEVICE_REVISION,
        base_definition_revision: base.revision,
        document,
    };
    result.validate_against(base)?;
    Ok(result)
}
impl CarrierExteriorDefinition {
    pub fn validate_against(&self, base: &ExteriorDefinitionState) -> Result<(), String> {
        if self.assembly_revision != CARRIER_EXTERIOR_ASSEMBLY_REVISION
            || self.source_sha256 != CARRIER_EXTERIOR_SOURCE
            || self.document.hull_asset_sha256 != self.source_sha256
            || self.device_revision != CARRIER_EXTERIOR_DEVICE_REVISION
            || self.base_definition_revision != base.revision
        {
            return Err("carrier exterior source/device/base-revision binding mismatch".into());
        }
        base.document.validate()?;
        canonical_positions(&base.document)?;
        let mut restored = self.document.clone();
        restored.hull_asset_sha256 = base.document.hull_asset_sha256.clone();
        for fixture in &mut restored.fixtures {
            if let Some(expected) = nav_position(&fixture.id) {
                if fixture.position_m != expected {
                    return Err("carrier navigation lamp differs from proved attachment".into());
                }
                fixture.position_m = base
                    .document
                    .fixtures
                    .iter()
                    .find(|f| f.id == fixture.id)
                    .ok_or("unknown carrier exterior device")?
                    .position_m;
            }
        }
        restored.validate()?;
        if restored != base.document {
            return Err("carrier binding must preserve authoritative fixture semantics".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_vessel_wire_defaults_to_no_variant_definition() {
        let mut value = serde_json::json!({"pilot":"P","ship_class":"Pioneer","vessel_id":"v1:P:Pioneer","enabled":true,"powered":true,"drive":"idle","load_w":124});
        // Reuse the canonical enum serialization, independent of its naming convention.
        value["ship_class"] = serde_json::to_value(crate::ShipClass::Pioneer).unwrap();
        let old: crate::ExteriorVesselState = serde_json::from_value(value.clone()).unwrap();
        assert!(old.source_definition.is_none());
        assert_eq!(serde_json::to_value(old).unwrap(), value);
    }
    #[test]
    fn exact_variant_changes_only_two_positions_and_keeps_legacy_valid() {
        let base = ExteriorDefinitionState::default();
        let original = base.clone();
        let result = for_exact_source(1702, CARRIER_EXTERIOR_SOURCE, &base).unwrap();
        assert_eq!(base, original);
        assert!(base.document.validate().is_ok());
        assert!(result.document.validate().is_err()); // legacy validator is not widened
        assert!(result.validate_against(&base).is_ok());
        assert_eq!(result.document.load_w(), base.document.load_w());
        assert_eq!(
            result
                .document
                .fixtures
                .iter()
                .zip(&base.document.fixtures)
                .filter(|(a, b)| a.position_m != b.position_m)
                .count(),
            2
        );
    }
    #[test]
    fn source_revision_custom_geometry_and_forged_semantics_fail_closed() {
        let base = ExteriorDefinitionState::default();
        assert!(for_exact_source(1701, CARRIER_EXTERIOR_SOURCE, &base).is_err());
        assert!(for_exact_source(1702, "legacy", &base).is_err());
        let mut edited = base.clone();
        edited.document.fixtures[0].position_m[0] += 1.;
        assert!(for_exact_source(1702, CARRIER_EXTERIOR_SOURCE, &edited).is_err());
        let valid = for_exact_source(1702, CARRIER_EXTERIOR_SOURCE, &base).unwrap();
        for kind in 0..5 {
            let mut bad = valid.clone();
            match kind {
                0 => bad.device_revision += 1,
                1 => bad.base_definition_revision += 1,
                2 => bad.document.fixtures[0].position_m[0] += 1.,
                3 => bad.document.fixtures[0].load_w += 1,
                _ => bad.document.fixtures.pop().map(|_| ()).unwrap(),
            }
            assert!(bad.validate_against(&base).is_err());
        }
    }
    #[test]
    fn legitimate_color_and_power_configuration_is_preserved_not_invented() {
        let mut base = ExteriorDefinitionState {
            revision: 9,
            ..ExteriorDefinitionState::default()
        };
        base.document.fixtures[0].color = [0.1, 0.2, 0.3];
        base.document.fixtures[0].load_w = 5;
        let result = for_exact_source(1702, CARRIER_EXTERIOR_SOURCE, &base).unwrap();
        assert_eq!(result.document.fixtures[0].color, [0.1, 0.2, 0.3]);
        assert_eq!(result.document.fixtures[0].load_w, 5);
        assert_eq!(result.base_definition_revision, 9);
    }
}
