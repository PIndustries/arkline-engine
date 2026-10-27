//! Native catalog presentation of the shared, strictly validated source mapping.
pub(super) use super::super::pioneer_source_snapshot::validate_installed;
use super::super::pioneer_source_snapshot::{mappings, MATERIALS};
use super::*;

#[cfg(test)]
use super::super::pioneer_source_snapshot::{
    validate, ASSET_HASH, EXPORT, PART_COUNT, SOURCE_HASH, TRIANGLE_COUNT,
};
#[cfg(test)]
use serde_json::Value;
#[cfg(test)]
use sha2::{Digest, Sha256};

fn name_group(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if name == "Pressure hull" {
        "Hull (name-based)"
    } else if name.starts_with("Bridge") || lower.starts_with("study05 bridge") {
        "Bridge (name-based)"
    } else if lower.contains("drive") {
        "Drives (name-based)"
    } else if lower.contains("turret") {
        "Turret mounts (name-based)"
    } else if name.starts_with("Landing") {
        "Landing (name-based)"
    } else if name.starts_with("Pod") || lower.contains("nacelle") {
        "Pods and nacelles (name-based)"
    } else if name.starts_with("Service") {
        "Service (name-based)"
    } else if lower.starts_with("study05 roof") {
        "Roof (name-based)"
    } else if name.starts_with("Radiator") {
        "Radiators (name-based)"
    } else {
        "Other named parts"
    }
}

pub(super) fn descriptors() -> Vec<RuntimeModelDescriptor> {
    let parts = match mappings() {
        Ok(parts) => parts,
        Err(reason) => {
            let mut entry = descriptor(
                "pioneer/parts/unavailable",
                "Pioneer source parts unavailable",
                "Source parts",
                "assets/ships/pioneer/pioneer-export.json",
            );
            entry.hierarchy = vec!["Ships".into(), "Pioneer".into(), "Source parts".into()];
            entry.unavailable = Some(reason);
            return vec![entry];
        }
    };
    parts.iter().map(|part| {
        let mut entry = descriptor(&part.id, &part.name, "Source parts", "assets/ships/pioneer/pioneer-export.json");
        entry.hierarchy = ["Ships", "Pioneer", "Source parts", name_group(&part.name)].map(str::to_owned).to_vec();
        entry.notes = vec![
            format!("Exact source object: {} · mesh: {}", part.name, part.source_mesh),
            format!("Source snapshot identity: {}", part.source_id),
            format!("{} · primitive {} · expanded vertex range {}..{} · {} triangles", MATERIALS[part.primitive], part.primitive, part.first, part.first + part.count, part.count / 3),
            format!("Measured source-piece extent {:?} m XYZ; presentation bounds, not a physical specification.", ((part.max - part.min) * 100.0).to_array()),
            "Original body-meter placement and authored scalar materials are retained. Group folders organize source names; they are not an authored assembly hierarchy.".into(),
            "Mapping is derived review-candidate evidence for this exact GLB/source snapshot, not engineering approval. Renaming or changing the saved source changes snapshot identities.".into(),
        ];
        entry
    }).collect()
}

pub(super) fn draw(id: &str) -> Result<InspectionDraw, String> {
    let part = mappings()?
        .iter()
        .find(|part| part.id == id)
        .ok_or("Unknown canonical Pioneer source part")?;
    let primitive = &pioneer_asset()?[part.primitive];
    Ok(InspectionDraw {
        part: (
            10 + part.primitive,
            Mat4::IDENTITY,
            primitive.color,
            primitive.emission,
        ),
        material: Material::BareMetal,
        range: Some((part.first, part.count)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_source_parts_partition_actual_gpu_geometry_with_exact_bounds_and_materials() {
        let primitives = pioneer_asset().unwrap();
        let parts = mappings().unwrap();
        assert_eq!(parts.len(), PART_COUNT);
        assert_eq!(
            parts.iter().map(|part| part.count / 3).sum::<usize>(),
            TRIANGLE_COUNT
        );
        assert_eq!(
            hex::encode(Sha256::digest(include_bytes!(
                "../../../assets/ships/pioneer/pioneer.blend"
            ))),
            SOURCE_HASH
        );
        for part in parts {
            let draw = draw(&part.id).unwrap();
            let primitive = &primitives[part.primitive];
            let selected = draw.selected_vertices(&primitive.vertices).unwrap();
            assert!(std::ptr::eq(
                selected.as_ptr(),
                primitive.vertices[part.first..].as_ptr()
            ));
            assert_eq!(selected.len(), part.count);
            assert_eq!(
                draw.part,
                (
                    10 + part.primitive,
                    Mat4::IDENTITY,
                    primitive.color,
                    primitive.emission
                )
            );
            assert_eq!(draw.material, Material::BareMetal);
            let measured = measure(vec![draw], 100.0).unwrap();
            assert_eq!(measured.info.triangles, part.count / 3);
            assert_eq!(measured.min, part.min);
            assert_eq!(measured.max, part.max);
        }
    }

    #[test]
    fn invalid_source_mapping_fails_closed_without_full_batch_fallback() {
        let original: Value = serde_json::from_str(EXPORT).unwrap();
        let primitives = pioneer_asset().unwrap();
        assert!(validate(EXPORT, "wrong-hash", primitives).is_err());
        assert!(validate("{broken", ASSET_HASH, primitives).is_err());
        let mutations: &[fn(&mut Value)] = &[
            |r| r["schema"] = "unknown-schema".into(),
            |r| r["source"]["sha256"] = "0".repeat(64).into(),
            |r| r["output"]["sha256"] = "0".repeat(64).into(),
            |r| r["output"]["body_bounds_m"][0][0] = (-40).into(),
            |r| r["source_parts"][1] = r["source_parts"][0].clone(),
            |r| r["source_parts"][0]["source_id"] = "part:invalid".into(),
            |r| r["source_parts"][0]["material"] = "Navigation glass".into(),
            |r| r["source_parts"][0]["first_index"] = 3.into(),
            |r| r["source_parts"][1]["first_index"] = 0.into(),
            |r| r["source_parts"][0]["first_index"] = 1.into(),
            |r| r["source_parts"][0]["index_count"] = 0.into(),
            |r| r["source_parts"][0]["first_index"] = u64::MAX.into(),
            |r| {
                r["source_parts"].as_array_mut().unwrap().pop();
            },
        ];
        for mutate in mutations {
            let mut changed = original.clone();
            mutate(&mut changed);
            assert!(validate(&changed.to_string(), ASSET_HASH, primitives).is_err());
        }
        assert!(draw("pioneer/part/unknown").is_err());
    }

    #[test]
    fn source_part_hierarchy_preserves_exact_names_and_snapshot_ids() {
        let entries = descriptors();
        assert_eq!(entries.len(), PART_COUNT);
        assert!(entries.iter().all(|entry| entry.unavailable.is_none()));
        for (entry, source) in entries.iter().zip(mappings().unwrap()) {
            assert_eq!(entry.name, source.name);
            assert_eq!(entry.id, source.id);
            assert_eq!(&entry.hierarchy[..3], ["Ships", "Pioneer", "Source parts"]);
            assert!(
                entry.hierarchy.last().unwrap().contains("name-based")
                    || entry.hierarchy.last().unwrap() == "Other named parts"
            );
        }
    }
}
