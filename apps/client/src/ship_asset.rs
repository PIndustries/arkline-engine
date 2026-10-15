//! Trusted, embedded original ship assets. No filesystem/network resource resolution.
use super::{Mat4, Vec3, Vertex};

pub(super) struct Primitive {
    pub vertices: Vec<Vertex>,
    /// Source glTF identity; renderer mesh offsets and display names are not bindings.
    pub material_index: Option<usize>,
    pub material_name: String,
    pub color: Vec3,
    pub emission: f32,
    pub emissive_color: Vec3,
    pub roughness: f32,
    pub metallic: f32,
}

pub(super) fn decode(bytes: &[u8]) -> Result<Vec<Primitive>, String> {
    let fail = |s: &str| s.to_owned();
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(fail("Ship asset exceeds 16 MiB"));
    }
    let document = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
    if document.nodes().len() > 4096 || document.meshes().len() > 128 {
        return Err("Ship object budget exceeded".into());
    }
    if document
        .extensions_used()
        .any(|e| e != "KHR_materials_emissive_strength")
    {
        return Err("Unsupported ship extension".into());
    }
    for material in document.materials() {
        let pbr = material.pbr_metallic_roughness();
        if material.alpha_mode() != gltf::material::AlphaMode::Opaque
            || pbr.base_color_texture().is_some()
            || pbr.metallic_roughness_texture().is_some()
            || material.normal_texture().is_some()
            || material.occlusion_texture().is_some()
            || material.emissive_texture().is_some()
        {
            return Err("Only opaque untextured ship materials supported".into());
        }
    }
    let blob = document
        .blob
        .as_deref()
        .ok_or_else(|| fail("Missing GLB buffer"))?;
    if document.buffers().len() != 1
        || document
            .buffers()
            .any(|b| !matches!(b.source(), gltf::buffer::Source::Bin) || b.length() > blob.len())
        || document.images().len() != 0
        || document.animations().len() != 0
        || document.skins().len() != 0
    {
        return Err(fail(
            "Ship requires one embedded buffer, no images, skins or animation",
        ));
    }
    for a in document.accessors() {
        let v = a.view().ok_or_else(|| fail("Missing accessor view"))?;
        let stride = v.stride().unwrap_or(a.size());
        if a.sparse().is_some()
            || a.count() == 0
            || a.count() > 600_000
            || stride < a.size()
            || a.offset() % a.data_type().size() != 0
            || stride % a.data_type().size() != 0
            || v.offset()
                .checked_add(v.length())
                .is_none_or(|n| n > document.buffers().next().unwrap().length())
            || (a.count() - 1)
                .checked_mul(stride)
                .and_then(|n| n.checked_add(a.offset()))
                .and_then(|n| n.checked_add(a.size()))
                .is_none_or(|n| n > v.length())
        {
            return Err(fail("Invalid or oversized accessor"));
        }
    }
    let scene = document
        .default_scene()
        .ok_or_else(|| fail("Missing default scene"))?;
    let mut result = Vec::new();
    let mut budget = 600_000usize;
    let mut node_visits = 4096usize;
    for node in scene.nodes() {
        visit(
            node,
            Mat4::IDENTITY,
            blob,
            &mut result,
            &mut budget,
            &mut node_visits,
            0,
        )?;
    }
    if result.is_empty() {
        return Err(fail("Empty ship"));
    }
    Ok(result)
}
fn visit(
    node: gltf::Node<'_>,
    parent: Mat4,
    blob: &[u8],
    out: &mut Vec<Primitive>,
    budget: &mut usize,
    node_visits: &mut usize,
    depth: usize,
) -> Result<(), String> {
    *node_visits = node_visits
        .checked_sub(1)
        .ok_or("Ship node visit budget exceeded")?;
    if depth > 32 || out.len() > 128 {
        return Err("Ship hierarchy/primitive budget exceeded".into());
    }
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    if !transform.is_finite() || transform.determinant().abs() < 1e-10 {
        return Err("Singular ship transform".into());
    }
    let normal_matrix = transform.inverse().transpose();
    if let Some(mesh) = node.mesh() {
        for p in mesh.primitives() {
            if out.len() >= 128 {
                return Err("Ship primitive budget exceeded".into());
            }
            if p.mode() != gltf::mesh::Mode::Triangles || p.morph_targets().len() != 0 {
                return Err("Only static triangle meshes supported".into());
            }
            let material = p.material();
            let pbr = material.pbr_metallic_roughness();
            let reader = p.reader(|_| Some(blob));
            let positions: Vec<_> = reader
                .read_positions()
                .ok_or("Missing ship positions")?
                .collect();
            let normals: Vec<_> = reader
                .read_normals()
                .ok_or("Missing ship normals")?
                .collect();
            if positions.len() != normals.len() {
                return Err("Ship normal count mismatch".into());
            }
            let indices: Vec<u32> = reader
                .read_indices()
                .ok_or("Missing ship indices")?
                .into_u32()
                .collect();
            if indices.len() % 3 != 0 || indices.len() > *budget {
                return Err("Ship triangle budget exceeded".into());
            }
            *budget -= indices.len();
            let mut vertices = Vec::with_capacity(indices.len());
            for i in indices {
                let point = Vec3::from_array(
                    *positions
                        .get(i as usize)
                        .ok_or("Ship index out of bounds")?,
                );
                let normal = Vec3::from_array(normals[i as usize]);
                if !point.is_finite() || !normal.is_finite() || normal.length_squared() < 0.1 {
                    return Err("Invalid ship vertex".into());
                }
                let p = transform.transform_point3(point) / 100.0;
                let n = normal_matrix.transform_vector3(normal).normalize();
                if !p.is_finite() || !n.is_finite() {
                    return Err("Non-finite transformed ship vertex".into());
                }
                vertices.push(Vertex { p, n });
            }
            if vertices
                .chunks_exact(3)
                .any(|v| (v[1].p - v[0].p).cross(v[2].p - v[0].p).length_squared() <= 1e-20)
            {
                return Err("Degenerate ship triangle".into());
            }
            if transform.determinant() < 0.0 {
                for tri in vertices.chunks_exact_mut(3) {
                    tri.swap(1, 2);
                }
            }
            let rgba = pbr.base_color_factor();
            let color = Vec3::new(rgba[0], rgba[1], rgba[2]);
            let strength = material.emissive_strength().unwrap_or(1.0);
            if !strength.is_finite() || !(0.0..=64.0).contains(&strength) {
                return Err("Unsupported emissive intensity".into());
            }
            let emissive = Vec3::from_array(material.emissive_factor()) * strength;
            let emission = emissive.max_element();
            let emissive_color = if emission > 0.0 {
                emissive / emission
            } else {
                Vec3::ZERO
            };
            out.push(Primitive {
                material_index: material.index(),
                material_name: material.name().unwrap_or("").to_owned(),
                vertices,
                color,
                emission,
                emissive_color,
                roughness: pbr.roughness_factor().max(0.08),
                metallic: pbr.metallic_factor(),
            });
        }
    }
    for child in node.children() {
        visit(child, transform, blob, out, budget, node_visits, depth + 1)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Explicit exact-hash saved-master candidate through the real Rust ship decoder"]
    fn reviewed_pioneer_candidate_decodes_at_exact_hash() {
        use sha2::{Digest, Sha256};
        let path = std::env::var_os("ARKLINE_PIONEER_CANDIDATE")
            .expect("set ARKLINE_PIONEER_CANDIDATE to the reviewed GLB");
        let expected = std::env::var("ARKLINE_PIONEER_CANDIDATE_SHA256")
            .expect("set ARKLINE_PIONEER_CANDIDATE_SHA256 from the reviewed receipt");
        let bytes = std::fs::read(path).unwrap();
        let actual = hex::encode(Sha256::digest(&bytes));
        assert_eq!(
            actual, expected,
            "staging must use the reviewed exact bytes"
        );
        let primitives = decode(&bytes).expect("real Rust decoder must accept the candidate");
        let names = [
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
        assert_eq!(primitives.len(), names.len());
        for (index, (part, name)) in primitives.iter().zip(names).enumerate() {
            assert_eq!(part.material_index, Some(index));
            assert_eq!(part.material_name, name);
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for point in primitives
            .iter()
            .flat_map(|part| &part.vertices)
            .map(|v| v.p)
        {
            min = min.min(point);
            max = max.max(point);
        }
        assert!(min.distance(Vec3::new(-0.36, -0.09, -0.19)) < 0.00001);
        assert!(max.distance(Vec3::new(0.36, 0.09, 0.19)) < 0.00001);
        let triangles = primitives
            .iter()
            .map(|p| p.vertices.len() / 3)
            .sum::<usize>();
        assert!((70_000..100_000).contains(&triangles));
        eprintln!(
            "PIONEER_CANDIDATE {}",
            serde_json::json!({
                "sha256":actual,"bytes":bytes.len(),"triangles":triangles,
                "material_batches":primitives.len(),"min_body_m":(min*100.).to_array(),
                "max_body_m":(max*100.).to_array(),"decoder":"arkline-client ship_asset::decode",
            })
        );
    }

    fn fixture(index: u16, scale: [f32; 3]) -> Vec<u8> {
        let mut binary = Vec::new();
        for v in [
            [0., 0., 0.],
            [1., 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [0., 0., 1.],
            [0., 0., 1.],
        ] {
            for f in v {
                binary.extend_from_slice(&f32::to_le_bytes(f));
            }
        }
        for i in [0u16, 1, index] {
            binary.extend_from_slice(&i.to_le_bytes());
        }
        let len = binary.len();
        while binary.len() % 4 != 0 {
            binary.push(0);
        }
        let json = serde_json::json!({"asset":{"version":"2.0"},"extensionsUsed":["KHR_materials_emissive_strength"],"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0,"scale":scale,"translation":[10,20,30]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0,"NORMAL":1},"indices":2,"material":0}]}],"materials":[{"extensions":{"KHR_materials_emissive_strength":{"emissiveStrength":2.0}},"emissiveFactor":[0.1,0.8,0.4],"pbrMetallicRoughness":{"baseColorFactor":[0.1,0.3,0.1,1],"roughnessFactor":0.6,"metallicFactor":0.8}}],"buffers":[{"byteLength":len}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36},{"buffer":0,"byteOffset":36,"byteLength":36},{"buffer":0,"byteOffset":72,"byteLength":6}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5126,"count":3,"type":"VEC3"},{"bufferView":2,"componentType":5123,"count":3,"type":"SCALAR"}]});
        let mut json = serde_json::to_vec(&json).unwrap();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut glb = Vec::new();
        for n in [
            0x46546c67u32,
            2,
            (12 + 8 + json.len() + 8 + binary.len()) as u32,
            json.len() as u32,
            0x4e4f534a,
        ] {
            glb.extend_from_slice(&n.to_le_bytes());
        }
        glb.extend(json);
        glb.extend_from_slice(&(binary.len() as u32).to_le_bytes());
        glb.extend_from_slice(&0x004e4942u32.to_le_bytes());
        glb.extend(binary);
        glb
    }
    #[test]
    fn embedded_geometry_preserves_meters_transforms_normals_and_materials() {
        let parts = decode(&fixture(2, [2., 3., 1.])).unwrap();
        assert_eq!(parts.len(), 1);
        let p = &parts[0];
        assert!((p.vertices[1].p - Vec3::new(0.12, 0.2, 0.3)).length() < 1e-6);
        assert_eq!(p.vertices[0].n, Vec3::Z);
        assert_eq!(p.roughness, 0.6);
        assert_eq!(p.metallic, 0.8);
        assert_eq!(p.color, Vec3::new(0.1, 0.3, 0.1));
        assert_eq!(p.emission, 1.6);
        assert_eq!(p.emissive_color, Vec3::new(0.125, 1.0, 0.5));
    }
    #[test]
    fn every_texture_channel_fails_closed_before_image_resolution() {
        for channel in [
            "baseColorTexture",
            "metallicRoughnessTexture",
            "normalTexture",
            "occlusionTexture",
            "emissiveTexture",
        ] {
            let original = fixture(2, [1.0; 3]);
            let json_len = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
            let mut document: serde_json::Value =
                serde_json::from_slice(&original[20..20 + json_len]).unwrap();
            // An external resource must never be resolved by this embedded-only decoder.
            document["images"] = serde_json::json!([{"uri": "not-resolved.png"}]);
            document["textures"] = serde_json::json!([{"source": 0}]);
            let material = &mut document["materials"][0];
            if matches!(channel, "baseColorTexture" | "metallicRoughnessTexture") {
                material["pbrMetallicRoughness"][channel] = serde_json::json!({"index": 0});
            } else {
                material[channel] = serde_json::json!({"index": 0});
            }
            let mut json = serde_json::to_vec(&document).unwrap();
            while json.len() % 4 != 0 {
                json.push(b' ');
            }
            let binary_chunk = &original[20 + json_len..];
            let mut bytes = Vec::new();
            for n in [
                0x46546c67u32,
                2,
                (20 + json.len() + binary_chunk.len()) as u32,
                json.len() as u32,
                0x4e4f534a,
            ] {
                bytes.extend_from_slice(&n.to_le_bytes());
            }
            bytes.extend(json);
            bytes.extend(binary_chunk);
            assert_eq!(
                decode(&bytes).err().as_deref(),
                Some("Only opaque untextured ship materials supported"),
                "{channel} must fail explicitly rather than lose its appearance"
            );
        }
    }
    #[test]
    fn mirrored_nodes_keep_outward_winding() {
        let parts = decode(&fixture(2, [-1., 1., 1.])).unwrap();
        let v = &parts[0].vertices;
        assert!((v[1].p - v[0].p).cross(v[2].p - v[0].p).dot(v[0].n) > 0.);
    }
    #[test]
    fn repeated_meshless_children_cannot_expand_traversal_exponentially() {
        let original = fixture(2, [1.0; 3]);
        let json_len = u32::from_le_bytes(original[12..16].try_into().unwrap()) as usize;
        let mut document: serde_json::Value =
            serde_json::from_slice(&original[20..20 + json_len]).unwrap();
        // Only 22 distinct nodes, but repeated children otherwise expand into
        // millions of visits. No primitives exist to consume the triangle cap.
        document["nodes"] = serde_json::Value::Array(
            (0..22)
                .map(|i| {
                    if i == 21 {
                        serde_json::json!({})
                    } else {
                        serde_json::json!({"children": [i + 1, i + 1]})
                    }
                })
                .collect(),
        );
        let mut json = serde_json::to_vec(&document).unwrap();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let binary_chunk = &original[20 + json_len..];
        let mut bytes = Vec::new();
        for n in [
            0x46546c67u32,
            2,
            (20 + json.len() + binary_chunk.len()) as u32,
            json.len() as u32,
            0x4e4f534a,
        ] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        bytes.extend(json);
        bytes.extend(binary_chunk);
        assert_eq!(
            decode(&bytes).err().as_deref(),
            Some("Ship node visit budget exceeded")
        );
    }

    #[test]
    fn bad_indices_and_singular_transforms_fail_closed() {
        assert!(decode(&fixture(6, [1., 1., 1.])).is_err());
        assert!(decode(&fixture(1, [1., 1., 1.])).is_err());
        assert!(decode(&fixture(2, [0., 1., 1.])).is_err());
        assert!(decode(b"not glb").is_err());
    }
}
