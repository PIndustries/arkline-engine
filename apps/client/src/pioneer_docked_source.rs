//! The raised bridge and boarding openings are authored in the canonical hull.
//!
//! All views use the same saved geometry. There is no view-dependent polygon
//! subtraction, source-index deletion or old-bridge mesh hidden at runtime.
use super::*;

const PRIMITIVES: [usize; 3] = [0, 1, 6];

/// Preserve the renderer's three material-batch slots while using exact source
/// triangles. The catalog verifies the reviewed GLB, saved-master receipt,
/// source identity, full triangle partition, materials and body-space bounds.
pub(super) fn source_meshes() -> Result<[Vec<Vertex>; 3], String> {
    static MESHES: std::sync::OnceLock<Result<[Vec<Vertex>; 3], String>> =
        std::sync::OnceLock::new();
    MESHES.get_or_init(build).clone()
}

fn build() -> Result<[Vec<Vertex>; 3], String> {
    model_catalog::validate_pioneer_source_snapshot()?;
    let asset = pioneer_asset()?;
    Ok(std::array::from_fn(|slot| {
        asset[PRIMITIVES[slot]].vertices.clone()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(vertex: &Vertex) -> [u32; 6] {
        [
            vertex.p.x.to_bits(),
            vertex.p.y.to_bits(),
            vertex.p.z.to_bits(),
            vertex.n.x.to_bits(),
            vertex.n.y.to_bits(),
            vertex.n.z.to_bits(),
        ]
    }

    #[test]
    fn canonical_buffers_are_identical_in_docked_flight_and_catalog_views() {
        let original = pioneer_asset().unwrap();
        let first = build().unwrap();
        let second = build().unwrap();
        for (slot, primitive) in PRIMITIVES.into_iter().enumerate() {
            let source: Vec<_> = original[primitive].vertices.iter().map(bits).collect();
            assert_eq!(first[slot].iter().map(bits).collect::<Vec<_>>(), source);
            assert_eq!(second[slot].iter().map(bits).collect::<Vec<_>>(), source);
        }
    }

    #[test]
    fn canonical_source_has_finite_unit_normals_and_valid_triangle_winding() {
        for vertices in build().unwrap() {
            assert_eq!(vertices.len() % 3, 0);
            for triangle in vertices.chunks_exact(3) {
                let cross = (triangle[1].p - triangle[0].p).cross(triangle[2].p - triangle[0].p);
                assert!(cross.is_finite() && cross.length_squared() > 1e-20);
                for vertex in triangle {
                    assert!(vertex.p.is_finite() && vertex.n.is_finite());
                    assert!((vertex.n.length_squared() - 1.0).abs() < 1e-4);
                }
            }
        }
    }

    #[test]
    fn raised_primary_aperture_is_clear_in_the_actual_opaque_source() {
        let eye = Vec3::from_array(arkline_protocol::pilot_joints(0.).eye_m.map(|v| v as f32));
        let source = pioneer_asset().unwrap();
        let metres = Mat4::from_scale(Vec3::splat(100.));
        for yaw in [-0.08_f32, 0., 0.08] {
            let direction = Vec3::new(yaw.cos(), 0., yaw.sin());
            for (primitive, part) in source.iter().enumerate() {
                if primitive == 6 {
                    continue;
                } // Actual authored navigation pane, not opaque armor.
                let hit = ray_mesh_hit(eye, direction, metres, &part.vertices);
                assert!(
                    hit.is_none(),
                    "opaque source primitive {primitive} blocks raised bridge at {hit:?}"
                );
            }
        }
    }
}
