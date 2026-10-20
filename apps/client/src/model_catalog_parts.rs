//! Exact source-object ranges in the canonical, material-batched Pioneer GLB.
//! These identities describe one saved source snapshot, not an authored assembly.
use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const EXPORT: &str = include_str!("../../../assets/ships/pioneer/pioneer-export.json");
const EXPORT_HASH: &str = "fadece504f99169f645559f5c903636b6555f8d8b3f3c3b856cc267b639d5429";
const SCHEMA: &str = "arkline.pioneer-saved-master-export/v1";
const ASSET_HASH: &str = "8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123";
const SOURCE_HASH: &str = "61eb70e1978668f5ca7256f237d9df14c47140b9bbbbce327875b68efcbe57c9";
const PART_COUNT: usize = 686;
const TRIANGLE_COUNT: usize = 103_466;
const MATERIALS: [&str; 9] = [
    "Graphite ceramic armor",
    "Titanium machinery",
    "Recessed carbon",
    "Emerald structural frames",
    "Drive emission",
    "Working lights",
    "Navigation glass",
    "Safety amber",
    "Hull identification ivory",
];

struct SourcePart {
    id: String,
    name: String,
    source_id: String,
    source_mesh: String,
    primitive: usize,
    first: usize,
    count: usize,
    min: Vec3,
    max: Vec3,
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Source mapping has no valid {key}"))
}

fn number(value: &Value, key: &str) -> Result<usize, String> {
    value[key]
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| format!("Source mapping has no valid {key}"))
}

fn snapshot_id(kind: &str, name: &str) -> String {
    // This is the saved-master export schema's identity algorithm. The separate
    // Blender assembly-inspection schema uses a different kind-prefixed recipe.
    let mut digest = Sha256::new();
    digest.update(SOURCE_HASH.as_bytes());
    digest.update([0]);
    digest.update(name.as_bytes());
    format!("{kind}:{}", hex::encode(digest.finalize()))
}

fn validate(
    document: &str,
    asset_hash: &str,
    primitives: &[ship_asset::Primitive],
) -> Result<Vec<SourcePart>, String> {
    if document.len() > 1024 * 1024 {
        return Err("Pioneer source mapping exceeds its document budget".into());
    }
    let receipt: Value = serde_json::from_str(document)
        .map_err(|e| format!("Invalid Pioneer source mapping: {e}"))?;
    if asset_hash != ASSET_HASH || text(&receipt["output"], "sha256")? != ASSET_HASH {
        return Err("Pioneer source mapping does not match the installed GLB hash".into());
    }
    if text(&receipt, "schema")? != SCHEMA
        || text(&receipt, "status")? != "derived-review-candidate-not-adopted"
        || text(&receipt["source"], "sha256")? != SOURCE_HASH
        || number(&receipt["source"], "size_bytes")? != 592_067
        || text(&receipt["source"], "name")? != "pioneer-study05.blend"
        || text(&receipt["output"], "file")? != "pioneer.glb"
        || number(&receipt["output"], "bytes")? != 8_700_948
        || number(&receipt["output"], "triangles")? != TRIANGLE_COUNT
        || number(&receipt["output"], "primitives")? != MATERIALS.len()
        || number(&receipt["output"], "materials")? != MATERIALS.len()
        || primitives.len() != MATERIALS.len()
    {
        return Err("Pioneer source mapping has an unsupported snapshot schema or layout".into());
    }
    let identity = receipt["material_identity"]
        .as_array()
        .ok_or("Source mapping has no material identities")?;
    if identity.len() != MATERIALS.len() {
        return Err("Source mapping material identities are incomplete".into());
    }
    for (index, (primitive, name)) in primitives.iter().zip(MATERIALS).enumerate() {
        let row = &identity[index];
        if primitive.material_index != Some(index)
            || primitive.material_name != name
            || text(row, "name")? != name
            || number(row, "render_material_index")? != index
            || text(row, "source_id")? != snapshot_id("material", name)
            || !primitive.color.is_finite()
            || !primitive.emissive_color.is_finite()
            || !primitive.emission.is_finite()
            || !primitive.roughness.is_finite()
            || !primitive.metallic.is_finite()
        {
            return Err(format!(
                "Source mapping material mismatch for primitive {index}"
            ));
        }
    }
    let rows = receipt["source_parts"]
        .as_array()
        .ok_or("Source mapping has no source-part list")?;
    if rows.len() != PART_COUNT {
        return Err("Source mapping must contain all 686 canonical source parts".into());
    }
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut ranges: [Vec<(usize, usize)>; 9] = std::array::from_fn(|_| Vec::new());
    let mut parts = Vec::with_capacity(PART_COUNT);
    let mut all_min = Vec3::splat(f32::INFINITY);
    let mut all_max = Vec3::splat(f32::NEG_INFINITY);
    let mut total = 0usize;
    for row in rows {
        let name = text(row, "name")?;
        let source_id = text(row, "source_id")?;
        let source_mesh = text(row, "source_mesh_name")?;
        if name.is_empty()
            || name.len() > 1024
            || name.contains('\0')
            || source_mesh.is_empty()
            || source_mesh.len() > 1024
            || source_mesh.contains('\0')
            || !names.insert(name.to_owned())
            || !ids.insert(source_id.to_owned())
        {
            return Err(
                "Source mapping contains an invalid or duplicate source identity/name".into(),
            );
        }
        if source_id != snapshot_id("part", name) {
            return Err(format!(
                "Source mapping identity does not match the saved source snapshot: {name}"
            ));
        }
        let primitive = number(row, "render_primitive")?;
        let material = number(row, "render_material_index")?;
        let first = number(row, "first_index")?;
        let count = number(row, "index_count")?;
        let retained = number(row, "retained_triangles")?;
        let evaluated = number(row, "evaluated_triangles")?;
        let dropped = number(row, "dropped_triangles")?;
        if primitive >= MATERIALS.len()
            || material != primitive
            || text(row, "material")? != MATERIALS[primitive]
            || retained.checked_add(dropped) != Some(evaluated)
            || retained.checked_mul(3) != Some(count)
        {
            return Err(format!(
                "Source mapping material or triangle count mismatch: {name}"
            ));
        }
        let vertices = &primitives[primitive].vertices;
        let draw_count =
            i32::try_from(vertices.len()).map_err(|_| "Source primitive exceeds the GPU range")?;
        let (start, length) = triangle_draw_range(draw_count, Some((first, count)))
            .ok_or_else(|| format!("Source mapping has an invalid triangle range: {name}"))?;
        let selected = &vertices[start as usize..(start + length) as usize];
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for vertex in selected {
            if !vertex.p.is_finite() || !vertex.n.is_finite() || vertex.n.length_squared() < 0.1 {
                return Err(format!("Source part contains invalid geometry: {name}"));
            }
            min = min.min(vertex.p);
            max = max.max(vertex.p);
        }
        if !min.is_finite() || !max.is_finite() || (max - min).max_element() <= 0.0 {
            return Err(format!("Source part has invalid measured bounds: {name}"));
        }
        all_min = all_min.min(min);
        all_max = all_max.max(max);
        total = total
            .checked_add(retained)
            .ok_or("Source mapping triangle count overflow")?;
        ranges[primitive].push((first, count));
        parts.push(SourcePart {
            id: format!("pioneer/part/{}", &source_id[5..]),
            name: name.into(),
            source_id: source_id.into(),
            source_mesh: source_mesh.into(),
            primitive,
            first,
            count,
            min,
            max,
        });
    }
    for (index, range_list) in ranges.iter_mut().enumerate() {
        range_list.sort_unstable();
        let mut cursor = 0usize;
        for &(first, count) in range_list.iter() {
            if first != cursor {
                return Err(format!(
                    "Source part range partition has a gap or overlap in primitive {index}"
                ));
            }
            cursor = cursor
                .checked_add(count)
                .ok_or("Source range partition overflow")?;
        }
        if cursor != primitives[index].vertices.len() {
            return Err(format!(
                "Source part range partition does not cover primitive {index}"
            ));
        }
    }
    if total != TRIANGLE_COUNT {
        return Err("Source part ranges do not reproduce the canonical triangle count".into());
    }
    let reported: [[f32; 3]; 2] =
        serde_json::from_value(receipt["output"]["body_bounds_m"].clone())
            .map_err(|_| "Source mapping has no valid body-meter bounds")?;
    let expected = [Vec3::new(-36.0, -9.0, -19.0), Vec3::new(36.0, 9.0, 19.0)];
    for (actual, report, expected) in [
        (all_min * 100.0, reported[0], expected[0]),
        (all_max * 100.0, reported[1], expected[1]),
    ] {
        let report = Vec3::from_array(report);
        if !report.is_finite()
            || !actual.abs_diff_eq(report, 0.001)
            || !report.abs_diff_eq(expected, 0.001)
        {
            return Err(
                "Source mapping bounds do not match the canonical body-meter geometry".into(),
            );
        }
    }
    parts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(parts)
}

fn mappings() -> Result<&'static Vec<SourcePart>, String> {
    static PARTS: OnceLock<Result<Vec<SourcePart>, String>> = OnceLock::new();
    PARTS
        .get_or_init(|| {
            if hex::encode(Sha256::digest(EXPORT.as_bytes())) != EXPORT_HASH {
                return Err(
                    "Pioneer source mapping receipt differs from its reviewed snapshot".into(),
                );
            }
            validate(EXPORT, pioneer_asset_sha256(), pioneer_asset()?)
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(super) fn validate_installed() -> Result<(), String> {
    mappings().map(|_| ())
}

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
