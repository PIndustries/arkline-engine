//! Additive exact1703 exterior provider; retained1702 and legacy definitions are
//! not modified. This does not establish installation, power, or travel admission.
use crate::carrier_exterior::{self as retained, CarrierExteriorDefinition};
use crate::ExteriorDefinitionState;

pub const SOURCE_SHA256: &str = "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a";
pub const ASSEMBLY_REVISION: u32 = 1703;
pub const DEVICE_REVISION: u32 = 2;

pub fn for_exact_source(
    assembly_revision: u32,
    source_sha256: &str,
    base: &ExteriorDefinitionState,
) -> Result<CarrierExteriorDefinition, String> {
    if assembly_revision != ASSEMBLY_REVISION || source_sha256 != SOURCE_SHA256 {
        return Err("unsupported exact parallel carrier exterior source".into());
    }
    // Same independently re-proved two nav attachments; reuse the strict
    // complete base topology/custom-edit guard without changing old validation.
    let mut definition = retained::for_exact_source(
        retained::CARRIER_EXTERIOR_ASSEMBLY_REVISION,
        retained::CARRIER_EXTERIOR_SOURCE,
        base,
    )?;
    definition.assembly_revision = ASSEMBLY_REVISION;
    definition.source_sha256 = SOURCE_SHA256.into();
    definition.document.hull_asset_sha256 = SOURCE_SHA256.into();
    definition.device_revision = DEVICE_REVISION;
    validate_against(&definition, base)?;
    Ok(definition)
}

pub fn validate_against(
    definition: &CarrierExteriorDefinition,
    base: &ExteriorDefinitionState,
) -> Result<(), String> {
    if definition.assembly_revision != ASSEMBLY_REVISION
        || definition.source_sha256 != SOURCE_SHA256
        || definition.document.hull_asset_sha256 != SOURCE_SHA256
        || definition.device_revision != DEVICE_REVISION
    {
        return Err("parallel exterior exact source/device binding mismatch".into());
    }
    let mut retained_binding = definition.clone();
    retained_binding.assembly_revision = retained::CARRIER_EXTERIOR_ASSEMBLY_REVISION;
    retained_binding.source_sha256 = retained::CARRIER_EXTERIOR_SOURCE.into();
    retained_binding.document.hull_asset_sha256 = retained::CARRIER_EXTERIOR_SOURCE.into();
    retained_binding.device_revision = retained::CARRIER_EXTERIOR_DEVICE_REVISION;
    retained_binding.validate_against(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_variant_preserves_fixture_order_semantics_and_legacy_definition() {
        let mut base = ExteriorDefinitionState::default();
        base.document.fixtures[0].color = [0.1, 0.2, 0.3];
        base.document.fixtures[0].load_w = 5;
        let before = base.clone();
        let result = for_exact_source(1703, SOURCE_SHA256, &base).unwrap();
        validate_against(&result, &base).unwrap();
        assert_eq!(base, before);
        assert_eq!(result.document.fixtures.len(), 13);
        assert_eq!(result.document.fixtures[0].color, [0.1, 0.2, 0.3]);
        assert_eq!(result.document.fixtures[0].load_w, 5);
        for (a, b) in result.document.fixtures.iter().zip(&base.document.fixtures) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.kind, b.kind);
        }
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
        assert!(
            result.validate_against(&base).is_err(),
            "old validator must not silently admit1703"
        );
    }
    #[test]
    fn wrong_source_revision_custom_geometry_and_mutated_semantics_fail_closed() {
        let base = ExteriorDefinitionState::default();
        assert!(for_exact_source(1702, SOURCE_SHA256, &base).is_err());
        assert!(for_exact_source(1703, retained::CARRIER_EXTERIOR_SOURCE, &base).is_err());
        let valid = for_exact_source(1703, SOURCE_SHA256, &base).unwrap();
        for kind in 0..6 {
            let mut bad = valid.clone();
            match kind {
                0 => bad.device_revision += 1,
                1 => bad.base_definition_revision += 1,
                2 => bad.document.fixtures[0].position_m[0] += 1.,
                3 => bad.document.fixtures[0].load_w += 1,
                4 => bad.document.fixtures.swap(0, 1),
                _ => bad.source_sha256 = retained::CARRIER_EXTERIOR_SOURCE.into(),
            }
            assert!(validate_against(&bad, &base).is_err());
        }
        let mut edited = base;
        edited.document.fixtures[0].position_m[0] += 1.;
        assert!(for_exact_source(1703, SOURCE_SHA256, &edited).is_err());
    }
}
