//! Independently registered additive receiver; never weakens the Pioneer loader.
use super::*;
use arkline_protocol::drone_bay_geometry::BayPose;
use arkline_protocol::drone_dropoff_geometry as geometry;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct Output {
    sha256: String,
    bytes: usize,
    triangles: usize,
    primitives: usize,
    body_bounds_m: [[f64; 3]; 2],
}
#[derive(Deserialize)]
struct Part {
    source_id: String,
    render_primitive: usize,
    first_index: usize,
    index_count: usize,
    retained_triangles: usize,
    material: String,
    group: Option<String>,
}
#[derive(Deserialize)]
struct Receipt {
    schema: String,
    receiver_revision: u32,
    base_registration: u32,
    base_glb_sha256: String,
    output: Output,
    source_parts: Vec<Part>,
    mechanism_reference_poses: BTreeMap<String, BayPose>,
}

pub(super) struct Mesh {
    pub primitive: usize,
    pub group: Option<String>,
    pub reference_pose: BayPose,
    pub vertices: Vec<Vertex>,
}
pub(super) struct Source {
    pub primitives: Vec<ship_asset::Primitive>,
    pub meshes: Vec<Mesh>,
}

fn decode(bytes: &[u8], document: &str) -> Result<Source, String> {
    use sha2::{Digest, Sha256};
    let receipt: Receipt = serde_json::from_str(document).map_err(|e| e.to_string())?;
    if receipt.schema != "arkline.pioneer-ore-receiver-export/v1"
        || receipt.receiver_revision != geometry::DROPOFF_RECEIVER_REVISION
        || receipt.base_registration != arkline_protocol::PIONEER_DOCKED_REGISTRATION_REVISION
        || receipt.base_glb_sha256 != geometry::DROPOFF_BASE_SOURCE_SHA256
        || receipt.output.sha256 != geometry::DROPOFF_RECEIVER_SOURCE_SHA256
        || receipt.output.bytes != bytes.len()
        || hex::encode(Sha256::digest(bytes)) != geometry::DROPOFF_RECEIVER_SOURCE_SHA256
    {
        return Err("Ore receiver source registration mismatch".into());
    }
    let mut primitives = ship_asset::decode(bytes)?;
    // The shared strict ship decoder returns scene units (body metres / 100).
    // This additive module is retained in body metres so
    // machinery root deltas apply once, before the common carrier scene scale.
    for primitive in &mut primitives {
        for vertex in &mut primitive.vertices {
            vertex.p *= 100.0;
        }
    }
    if primitives.len() != receipt.output.primitives || primitives.len() != 5 {
        return Err("Ore receiver primitive count mismatch".into());
    }
    let expected: BTreeMap<_, _> = geometry::mechanism_ids()
        .into_iter()
        .zip(geometry::reference_poses())
        .map(|(id, pose)| (id.semantic_id().to_owned(), pose))
        .collect();
    if receipt.mechanism_reference_poses != expected {
        return Err("Ore receiver mechanism roots mismatch".into());
    }
    let mut used: Vec<Vec<bool>> = primitives
        .iter()
        .map(|p| vec![false; p.vertices.len()])
        .collect();
    let mut ids = BTreeSet::new();
    let mut grouped: BTreeMap<(Option<String>, usize), Vec<Vertex>> = BTreeMap::new();
    for part in receipt.source_parts {
        if part.source_id.is_empty()
            || !ids.insert(part.source_id)
            || part.index_count == 0
            || part.first_index % 3 != 0
            || part.index_count % 3 != 0
            || part.retained_triangles != part.index_count / 3
            || part
                .group
                .as_ref()
                .is_some_and(|id| !expected.contains_key(id))
        {
            return Err("Invalid ore receiver source part".into());
        }
        let primitive = primitives
            .get(part.render_primitive)
            .ok_or("Receiver primitive out of bounds")?;
        if part.material != primitive.material_name {
            return Err("Receiver source material identity mismatch".into());
        }
        let end = part
            .first_index
            .checked_add(part.index_count)
            .ok_or("Receiver range overflow")?;
        let vertices = primitive
            .vertices
            .get(part.first_index..end)
            .ok_or("Receiver range out of bounds")?;
        let flags = &mut used[part.render_primitive][part.first_index..end];
        if flags.iter().any(|v| *v) {
            return Err("Receiver source ranges overlap".into());
        }
        flags.fill(true);
        grouped
            .entry((part.group, part.render_primitive))
            .or_default()
            .extend_from_slice(vertices);
    }
    let triangles: usize = primitives.iter().map(|p| p.vertices.len() / 3).sum();
    if used.iter().flatten().any(|v| !v)
        || triangles != receipt.output.triangles
        || triangles != 4488
    {
        return Err("Receiver source coverage mismatch".into());
    }
    let actual_groups: BTreeSet<_> = grouped
        .keys()
        .filter_map(|(group, _)| group.as_ref())
        .collect();
    if actual_groups != expected.keys().collect() {
        return Err("Receiver mechanism source coverage mismatch".into());
    }
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for primitive in &primitives {
        for vertex in &primitive.vertices {
            for (axis, value) in vertex.p.to_array().iter().enumerate() {
                let value = f64::from(*value);
                if !value.is_finite() {
                    return Err("Nonfinite receiver source vertex".into());
                }
                bounds[0][axis] = bounds[0][axis].min(value);
                bounds[1][axis] = bounds[1][axis].max(value);
            }
        }
    }
    for (actual, expected) in bounds.iter().zip(&receipt.output.body_bounds_m) {
        for (actual, expected) in actual.iter().zip(expected) {
            if !expected.is_finite() || (actual - expected).abs() > 0.0001 {
                return Err("Receiver body-space bounds mismatch".into());
            }
        }
    }
    let meshes = grouped
        .into_iter()
        .map(|((group, primitive), vertices)| {
            let reference_pose = group.as_ref().map_or(
                BayPose {
                    position_m: [0.0; 3],
                    orientation_xyzw: [0.0, 0.0, 0.0, 1.0],
                },
                |id| expected[id],
            );
            Mesh {
                primitive,
                group,
                reference_pose,
                vertices,
            }
        })
        .collect();
    Ok(Source { primitives, meshes })
}

pub(super) fn installed() -> Result<&'static Source, String> {
    static SOURCE: std::sync::OnceLock<Result<Source, String>> = std::sync::OnceLock::new();
    SOURCE
        .get_or_init(|| {
            use sha2::{Digest, Sha256};
            let receipt =
                include_str!("../../../assets/ships/pioneer/ore-receiver-v1/receipt.json");
            if hex::encode(Sha256::digest(receipt.as_bytes()))
                != "de49efffbbeb97a08aa4ed5937c177df1a28b021dcd43267efca7a25539f2ed1"
            {
                return Err("Receiver source partition receipt mismatch".into());
            }
            decode(
                include_bytes!("../../../assets/ships/pioneer/ore-receiver-v1/ore-receiver.glb"),
                receipt,
            )
        })
        .as_ref()
        .map_err(Clone::clone)
}

#[cfg(test)]
mod tests {
    use super::*;
    const BYTES: &[u8] =
        include_bytes!("../../../assets/ships/pioneer/ore-receiver-v1/ore-receiver.glb");
    const RECEIPT: &str =
        include_str!("../../../assets/ships/pioneer/ore-receiver-v1/receipt.json");

    #[test]
    fn additive_receiver_retains_every_source_triangle_and_material() {
        let source = installed().unwrap();
        assert_eq!(
            source
                .meshes
                .iter()
                .map(|p| p.vertices.len() / 3)
                .sum::<usize>(),
            4488
        );
        let groups: BTreeSet<_> = source
            .meshes
            .iter()
            .filter_map(|m| m.group.as_deref())
            .collect();
        assert_eq!(
            groups,
            geometry::mechanism_ids()
                .iter()
                .map(|id| id.semantic_id())
                .collect()
        );
        for mesh in &source.meshes {
            assert!(mesh.reference_pose.valid());
            assert!(source.primitives[mesh.primitive].roughness.is_finite());
        }
    }

    #[test]
    fn receiver_rejects_overlap_omission_foreign_root_and_material() {
        let baseline: serde_json::Value = serde_json::from_str(RECEIPT).unwrap();
        for mutation in 0..5 {
            let mut value = baseline.clone();
            match mutation {
                0 => {
                    value["source_parts"].as_array_mut().unwrap().pop();
                }
                1 => {
                    let duplicate = value["source_parts"][0].clone();
                    value["source_parts"]
                        .as_array_mut()
                        .unwrap()
                        .push(duplicate);
                }
                2 => value["source_parts"][0]["material"] = "foreign".into(),
                3 => value["mechanism_reference_poses"]["stage/1"]["position_m"][0] = 0.into(),
                _ => value["receiver_revision"] = 99.into(),
            }
            assert!(
                decode(BYTES, &value.to_string()).is_err(),
                "mutation {mutation}"
            );
        }
        let mut altered = BYTES.to_vec();
        *altered.last_mut().unwrap() ^= 1;
        assert!(decode(&altered, RECEIPT).is_err());
    }
}
