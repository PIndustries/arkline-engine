//! Exact source-object ranges in the canonical, material-batched Pioneer GLB.
//! These identities describe one saved source snapshot, not an authored assembly.
use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::OnceLock;

pub(super) const EXPORT: &str = include_str!("../../../assets/ships/pioneer/pioneer-export.json");
const EXPORT_HASH: &str = "979748932dfe690cf1a48347286bbad4ee9e73b0ecfaa4505003237c9d9a2ed9";
const SCHEMA: &str = "arkline.pioneer-saved-master-export/v1";
pub(super) const ASSET_HASH: &str =
    "7970f096a6aac9d95b6250b75cd19ea2e54046c9deb77f3592b82efa879112fd";
pub(super) const SOURCE_HASH: &str =
    "ee43c05cb1d4ebf907d609b5338b4d8a911b812d8fb05914c96ca48dbb06b1d9";
pub(super) const PART_COUNT: usize = 953;
pub(super) const TRIANGLE_COUNT: usize = 131_438;
pub(super) const MATERIALS: [&str; 9] = [
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

#[cfg(not(target_arch = "wasm32"))]
pub(super) struct SourcePart {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) source_id: String,
    pub(super) source_mesh: String,
    pub(super) primitive: usize,
    pub(super) first: usize,
    pub(super) count: usize,
    pub(super) min: Vec3,
    pub(super) max: Vec3,
}

// Browser rendering needs the identical rejection checks, but not the native
// inspector's display metadata. Retain one validated marker per source part.
#[cfg(target_arch = "wasm32")]
type SourcePart = ();

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

pub(super) fn validate(
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
        || number(&receipt["source"], "size_bytes")? != 709_268
        || text(&receipt["source"], "name")? != "pioneer-drone-bay.blend"
        || text(&receipt["output"], "file")? != "pioneer.glb"
        || number(&receipt["output"], "bytes")? != 11_050_628
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
        return Err("Source mapping must contain all 953 canonical source parts".into());
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
        #[cfg(not(target_arch = "wasm32"))]
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
        #[cfg(target_arch = "wasm32")]
        parts.push(());
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
    let expected = [Vec3::new(-36.0, -9.0, -19.0), Vec3::new(36.0, 11.35, 19.0)];
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
    #[cfg(not(target_arch = "wasm32"))]
    parts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(parts)
}

pub(super) fn mappings() -> Result<&'static Vec<SourcePart>, String> {
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
