//! Independent saved-export receipt oracle for every individually selectable piece.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[path = "model_components_context_tests.rs"]
mod context;

fn source_buffer_hash(uploads: &[Arc<[Vertex]>]) -> String {
    let mut hash = Sha256::new();
    for vertices in &uploads[10..19] {
        hash.update((vertices.len() as u64).to_le_bytes());
        for vertex in vertices.iter() {
            for number in vertex.p.to_array().into_iter().chain(vertex.n.to_array()) {
                hash.update(number.to_le_bytes());
            }
        }
    }
    hex::encode(hash.finalize())
}

#[test]
fn all_686_source_pieces_independently_match_saved_receipt_and_partition_source() {
    let receipt: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../assets/ships/pioneer/pioneer-export.json"
    ))
    .unwrap();
    assert_eq!(receipt["schema"], "arkline.pioneer-saved-master-export/v1");
    let source_bytes = include_bytes!("../../../assets/ships/pioneer/pioneer.glb");
    assert_eq!(
        receipt["output"]["sha256"].as_str().unwrap(),
        hex::encode(Sha256::digest(source_bytes))
    );
    let rows = receipt["source_parts"].as_array().unwrap();
    assert_eq!(rows.len(), 686);
    let geometry = flight_geometry().unwrap();
    let uploads = geometry.catalog.uploads();
    let before = source_buffer_hash(uploads);
    let identities = uploads[10..19].to_vec();
    let catalog = model_catalog::runtime_model_catalog();
    let components = catalog
        .iter()
        .filter(|entry| entry.id.starts_with("pioneer/part/"))
        .map(|entry| (entry.id.as_str(), entry))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(components.len(), 686);
    assert_eq!(catalog.len() - components.len(), 111);
    let mut ids = BTreeSet::new();
    let mut partitions: BTreeMap<usize, Vec<(usize, usize)>> = BTreeMap::new();
    let mut triangles = 0;
    let mut materials = BTreeSet::new();
    for row in rows {
        let integer = |key: &str| usize::try_from(row[key].as_u64().unwrap()).unwrap();
        let source_id = row["source_id"].as_str().unwrap();
        let suffix = source_id.strip_prefix("part:").unwrap();
        assert_eq!(suffix.len(), 64);
        assert!(suffix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)));
        let id = format!("pioneer/part/{suffix}");
        assert!(ids.insert(id.clone()), "Receipt IDs must be unique");
        let descriptor = components
            .get(id.as_str())
            .expect("Every saved source object is selectable");
        assert_eq!(descriptor.name, row["name"].as_str().unwrap());
        assert_eq!(descriptor.category, "Source parts");
        assert!(descriptor.unavailable.is_none());
        assert!(
            !descriptor.hierarchy.is_empty(),
            "Source hierarchy must be available without changing object identity"
        );
        let primitive = integer("render_primitive");
        let first = integer("first_index");
        let count = integer("index_count");
        let retained = integer("retained_triangles");
        assert!(primitive < 9 && count > 0);
        assert_eq!((first % 3, count % 3, count / 3), (0, 0, retained));
        assert_eq!(
            integer("evaluated_triangles"),
            retained + integer("dropped_triangles")
        );
        assert_eq!(integer("render_material_index"), primitive);
        partitions
            .entry(primitive)
            .or_default()
            .push((first, count));
        materials.insert(row["material"].as_str().unwrap());
        let model = model_catalog::load_runtime_model(&id).unwrap();
        assert!(Arc::ptr_eq(
            &model,
            &model_catalog::load_runtime_model(&id).unwrap()
        ));
        assert_eq!(
            (model.parts.len(), model.info.batches, model.info.triangles),
            (1, 1, retained)
        );
        let draw = &model.parts[0];
        assert_eq!(draw.part.0, 10 + primitive);
        assert_eq!(draw.range, Some((first, count)));
        assert_eq!(draw.part.1, Mat4::IDENTITY);
        assert_eq!(draw.material, Material::BareMetal);
        let source = &pioneer_asset().unwrap()[primitive];
        assert_eq!(draw.part.2, source.color);
        assert_eq!(draw.part.3, source.emission);
        assert_eq!(source.material_name, row["material"].as_str().unwrap());
        assert_eq!(
            source.material_index,
            Some(integer("render_material_index"))
        );
        // Slice the unchanged upload directly from the raw receipt, independently
        // of the production range selector and its measured metadata.
        let expected = &uploads[10 + primitive][first..first.checked_add(count).unwrap()];
        let selected = draw.selected_vertices(&uploads[10 + primitive]).unwrap();
        assert_eq!(selected.as_ptr(), expected.as_ptr());
        assert_eq!(selected.len(), expected.len());
        let (min, max) = expected.iter().fold(
            (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
            |(min, max), vertex| {
                assert!(vertex.p.is_finite() && vertex.n.is_finite());
                (min.min(vertex.p), max.max(vertex.p))
            },
        );
        assert_eq!((model.min, model.max), (min, max));
        assert_eq!(model.info.min_m, (min * 100.0).to_array());
        assert_eq!(model.info.max_m, (max * 100.0).to_array());
        triangles += retained;
    }
    assert_eq!(materials.len(), 9);
    assert_eq!(triangles, 103_466);
    assert_eq!(
        triangles as u64,
        receipt["output"]["triangles"].as_u64().unwrap()
    );
    assert_eq!(partitions.len(), 9);
    for (primitive, mut ranges) in partitions {
        ranges.sort_unstable();
        let mut next = 0;
        for (first, count) in ranges {
            assert_eq!(first, next, "Primitive {primitive} source pieces must have no gaps, overlap or duplicate triangles");
            next += count;
        }
        assert_eq!(next, uploads[10 + primitive].len());
    }
    let after = flight_geometry().unwrap();
    assert!(Arc::ptr_eq(&geometry, &after));
    assert_eq!(before, source_buffer_hash(after.catalog.uploads()));
    for (primitive, original) in identities.iter().enumerate() {
        assert!(Arc::ptr_eq(
            original,
            &after.catalog.uploads()[10 + primitive]
        ));
    }
}
