//! Offline, bounded GLB submission inspection. Does not install or execute assets.
use anyhow::{bail, ensure, Context, Result};
use glam::{Mat4, Vec3};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs::File, io::Read, path::Path};
const MAX_FILE: usize = 32 * 1024 * 1024;
const MAX_VERTICES: usize = 1_000_000;
const MAX_INDICES: usize = 3_000_000;
fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    File::open(path)?
        .take((MAX_FILE + 1) as u64)
        .read_to_end(&mut data)?;
    ensure!(
        data.len() <= MAX_FILE,
        "GLB exceeds 32 MiB submission inspection limit"
    );
    Ok(data)
}
fn inspect(bytes: &[u8]) -> Result<Value> {
    ensure!(bytes.len() <= MAX_FILE, "GLB exceeds size limit");
    ensure!(
        bytes.get(..4) == Some(b"glTF"),
        "Expected binary glTF 2.0 (.glb)"
    );
    let header_length = bytes.get(8..12).context("Truncated GLB header")?;
    ensure!(
        u32::from_le_bytes(header_length.try_into().unwrap()) as usize == bytes.len(),
        "GLB declared length does not match file length"
    );
    let glb = gltf::Gltf::from_slice(bytes).context("Invalid glTF document")?;
    let blob = glb.blob.as_deref().context("GLB has no embedded buffer")?;
    ensure!(
        glb.buffers().len() == 1,
        "Exactly one embedded GLB buffer is supported"
    );
    for b in glb.buffers() {
        ensure!(
            matches!(b.source(), gltf::buffer::Source::Bin),
            "External buffer references are not accepted"
        );
        ensure!(b.length() <= blob.len(), "Truncated binary buffer");
    }
    let declared_buffer_len = glb.buffers().next().unwrap().length();
    for view in glb.views() {
        ensure!(
            view.offset()
                .checked_add(view.length())
                .is_some_and(|n| n <= declared_buffer_len),
            "Buffer view exceeds binary payload"
        );
    }
    ensure!(
        glb.nodes().len() <= 4096 && glb.meshes().len() <= 4096,
        "Too many scene objects"
    );
    for accessor in glb.accessors() {
        ensure!(
            accessor.count() > 0 && accessor.count() <= MAX_INDICES,
            "Empty accessor or accessor exceeds inspection budget"
        );
        // gltf utility iterators assume valid arithmetic/layout; reject malformed
        // layouts before constructing them rather than catching panics afterward.
        ensure!(
            accessor.sparse().is_none(),
            "Expand sparse accessors before submission inspection"
        );
        let view = accessor.view().context("Accessor has no buffer view")?;
        let size = accessor.size();
        let stride = view.stride().unwrap_or(size);
        ensure!(
            stride >= size,
            "Accessor stride is smaller than one element"
        );
        let component = accessor.data_type().size();
        ensure!(
            accessor.offset() % component == 0 && stride % component == 0,
            "Misaligned accessor"
        );
        let end = (accessor.count() - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(accessor.offset()))
            .and_then(|n| n.checked_add(size))
            .context("Accessor byte range overflow")?;
        ensure!(end <= view.length(), "Accessor exceeds its buffer view");
    }
    for image in glb.images() {
        ensure!(
            matches!(image.source(), gltf::image::Source::View { .. }),
            "Images must be embedded; no external files or URLs are fetched"
        );
    }
    let mut vertices = 0usize;
    let mut triangles = 0usize;
    let mut meshes = Vec::new();
    let mut bounds = Vec::new();
    for mesh in glb.meshes() {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for primitive in mesh.primitives() {
            ensure!(
                primitive.mode() == gltf::mesh::Mode::Triangles,
                "Submission profile requires triangle primitives"
            );
            let position = primitive
                .get(&gltf::Semantic::Positions)
                .context("Missing POSITION accessor")?;
            ensure!(
                position.data_type() == gltf::accessor::DataType::F32
                    && position.dimensions() == gltf::accessor::Dimensions::Vec3
                    && !position.normalized(),
                "POSITION must be unnormalized FLOAT VEC3"
            );
            if let Some(index) = primitive.indices() {
                ensure!(
                    index.dimensions() == gltf::accessor::Dimensions::Scalar
                        && matches!(
                            index.data_type(),
                            gltf::accessor::DataType::U8
                                | gltf::accessor::DataType::U16
                                | gltf::accessor::DataType::U32
                        )
                        && !index.normalized(),
                    "Indices must be unnormalized unsigned scalars"
                );
            }
            let reader = primitive.reader(|buffer| match buffer.source() {
                gltf::buffer::Source::Bin => Some(blob),
                _ => None,
            });
            let pos = reader
                .read_positions()
                .context("Missing or unreadable POSITION data")?;
            let count = pos.len();
            vertices = vertices
                .checked_add(count)
                .context("Vertex count overflow")?;
            ensure!(
                count > 0 && vertices <= MAX_VERTICES,
                "Vertex budget exceeded or empty mesh"
            );
            for p in pos {
                let p = Vec3::from_array(p);
                ensure!(p.is_finite(), "Non-finite vertex coordinate");
                lo = lo.min(p);
                hi = hi.max(p);
            }
            let n = if let Some(indices) = reader.read_indices() {
                let mut n = 0;
                for index in indices.into_u32() {
                    ensure!(
                        (index as usize) < count,
                        "Triangle index outside vertex buffer"
                    );
                    n += 1;
                    ensure!(n <= MAX_INDICES, "Index budget exceeded");
                }
                n
            } else {
                ensure!(primitive.indices().is_none(), "Unreadable index data");
                count
            };
            ensure!(
                n % 3 == 0,
                "Triangle index count must be divisible by three"
            );
            triangles += n / 3;
            ensure!(triangles <= MAX_INDICES / 3, "Triangle budget exceeded");
        }
        ensure!(lo.is_finite() && hi.is_finite(), "Empty mesh");
        bounds.push((lo, hi));
        meshes.push(json!({"index":mesh.index(),"name":mesh.name(),"local_bounds_m":[lo.to_array(),hi.to_array()]}));
    }
    ensure!(!meshes.is_empty(), "No meshes to inspect");
    let mut scene_lo = Vec3::splat(f32::INFINITY);
    let mut scene_hi = Vec3::splat(f32::NEG_INFINITY);
    let mut active = HashSet::new();
    let mut visited = 0;
    fn visit(
        node: gltf::Node<'_>,
        parent: Mat4,
        bounds: &[(Vec3, Vec3)],
        active: &mut HashSet<usize>,
        visited: &mut usize,
        lo: &mut Vec3,
        hi: &mut Vec3,
    ) -> Result<()> {
        ensure!(
            active.len() < 128 && active.insert(node.index()),
            "Scene cycle or excessive depth"
        );
        *visited += 1;
        ensure!(*visited <= 16384, "Scene traversal budget exceeded");
        let matrix = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        ensure!(matrix.is_finite(), "Invalid node transform");
        if let Some(mesh) = node.mesh() {
            let (a, b) = bounds[mesh.index()];
            for x in [a.x, b.x] {
                for y in [a.y, b.y] {
                    for z in [a.z, b.z] {
                        let p = matrix.transform_point3(Vec3::new(x, y, z));
                        ensure!(p.is_finite(), "Transformed bounds are not finite");
                        *lo = lo.min(p);
                        *hi = hi.max(p);
                    }
                }
            }
        }
        for child in node.children() {
            visit(child, matrix, bounds, active, visited, lo, hi)?;
        }
        active.remove(&node.index());
        Ok(())
    }
    let scene = glb
        .default_scene()
        .or_else(|| glb.scenes().next())
        .context("No scene; export an explicit scene")?;
    for node in scene.nodes() {
        visit(
            node,
            Mat4::IDENTITY,
            &bounds,
            &mut active,
            &mut visited,
            &mut scene_lo,
            &mut scene_hi,
        )?;
    }
    ensure!(scene_lo.is_finite(), "Selected scene has no mesh geometry");
    let extent = scene_hi - scene_lo;
    ensure!(
        extent.is_finite(),
        "Scene extent exceeds representable range"
    );
    let nodes: Vec<_> = glb
        .nodes()
        .filter_map(|n| n.name().map(|name| json!({"index":n.index(),"name":name})))
        .collect();
    Ok(json!({
        "profile":"arkline-submission-inspection-v1", "sha256":hex::encode(Sha256::digest(bytes)),
        "file_bytes":bytes.len(),"vertices":vertices,"triangles":triangles,"meshes":meshes,
        "scene_index":scene.index(),"scene_aabb_m":[scene_lo.to_array(),scene_hi.to_array()],
        "scene_extent_m":extent.to_array(),"named_nodes":nodes,
        "materials":glb.materials().len(),"images":glb.images().len(),"animations":glb.animations().len(),"skins":glb.skins().len(),
        "runtime_import_supported":false,
        "limitations":["Inspection does not install a game model or prove renderer compatibility.","Bounds are transformed mesh bounding boxes, not collision or walkability proof.","Textures, animation, skins and material behavior are counted but not runtime-validated.","Limits bound this inspection tool; they are not published game performance budgets.","Author, license, source project and gameplay attachment review are still required."]
    }))
}
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || args[0] != "inspect" {
        bail!("Usage: arkline-asset-tool inspect MODEL.glb\nPrints an offline submission report. Does not install game assets.");
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&inspect(&bounded_read(Path::new(&args[1]))?)?)?
    );
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("Asset inspection failed: {e:#}");
        std::process::exit(1);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(mutate: impl FnOnce(&mut Value)) -> Vec<u8> {
        let mut j = json!({"asset":{"version":"2.0"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0,"name":"test_hull"}],"meshes":[{"primitives":[{"attributes":{"POSITION":0}}]}],"buffers":[{"byteLength":36}],"bufferViews":[{"buffer":0,"byteLength":36}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]}]});
        mutate(&mut j);
        let mut json = serde_json::to_vec(&j).unwrap();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let positions = [0f32, 0., 0., 1., 0., 0., 0., 1., 0.];
        let bin: Vec<_> = positions.iter().flat_map(|x| x.to_le_bytes()).collect();
        let mut out = b"glTF".to_vec();
        out.extend(2u32.to_le_bytes());
        out.extend(((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend((json.len() as u32).to_le_bytes());
        out.extend(b"JSON");
        out.extend(json);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(b"BIN\0");
        out.extend(bin);
        out
    }
    #[test]
    fn reads_geometry_and_transform() {
        let r = inspect(&fixture(|j| {
            j["nodes"][0]["translation"] = json!([2, 3, 4])
        }))
        .unwrap();
        assert_eq!(r["triangles"], 1);
        assert_eq!(r["scene_aabb_m"][0], json!([2., 3., 4.]));
        assert_eq!(r["runtime_import_supported"], false);
    }
    #[test]
    fn rejects_external_and_bad_views() {
        for bytes in [
            fixture(|j| j["buffers"][0]["uri"] = json!("https://invalid.test/model.bin")),
            fixture(|j| j["bufferViews"][0]["byteLength"] = json!(9000)),
        ] {
            assert!(inspect(&bytes).is_err());
        }
    }
    #[test]
    fn rejects_cycles_and_budget_abuse() {
        assert!(inspect(&fixture(|j| j["nodes"][0]["children"] = json!([0]))).is_err());
        assert!(inspect(&fixture(|j| j["accessors"][0]["count"] = json!(9_000_000))).is_err());
    }
    #[test]
    fn rejects_truncation() {
        let f = fixture(|_| {});
        for n in [0, 4, 12, f.len() - 4] {
            assert!(inspect(&f[..n]).is_err());
        }
    }
    #[test]
    fn malformed_accessor_layout_returns_error_without_panicking() {
        for bytes in [
            fixture(|j| j["accessors"][0]["byteOffset"] = json!(usize::MAX)),
            fixture(|j| j["accessors"][0]["count"] = json!(0)),
            fixture(|j| j["bufferViews"][0]["byteStride"] = json!(4)),
            fixture(|j| j["accessors"][0]["type"] = json!("VEC2")),
        ] {
            assert!(inspect(&bytes).is_err());
        }
    }

    #[test]
    fn rejects_declared_buffer_overrun_sparse_and_unrepresentable_extent() {
        for bytes in [
            fixture(|j| j["buffers"][0]["byteLength"] = json!(4)),
            fixture(
                |j| j["accessors"][0]["sparse"] = json!({"count":1,"indices":{"bufferView":0,"componentType":5121},"values":{"bufferView":0}}),
            ),
            fixture(|j| {
                j["scenes"][0]["nodes"] = json!([0, 1]);
                j["nodes"] =
                    json!([{"mesh":0,"scale":[2e38,1,1]},{"mesh":0,"translation":[-2e38,0,0]}]);
            }),
        ] {
            assert!(inspect(&bytes).is_err());
        }
        let mut extra = fixture(|_| {});
        extra.extend([0; 4]);
        assert!(inspect(&extra).is_err());
    }
}
