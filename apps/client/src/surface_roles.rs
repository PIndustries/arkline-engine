//! Typed procedural surface identity carried without changing mesh geometry.
//! Bits 24..28 retain existing paint/emission roles; bits 28..32 carry this role.
//! No role authorizes a light or equipment output.
use super::materials::Material;

const SURFACE_MASK: usize = 0xf000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum SurfaceRole {
    CoatedPanel = 1,
    GripDeck = 2,
    Composite = 3,
    ExposedAlloy = 4,
    EnamelTrim = 5,
    InstrumentGlass = 6,
    FixtureDiffuser = 7,
    ServiceCeiling = 8,
    ArchitecturalPanel = 9,
}
impl SurfaceRole {
    /// A tagged instance of the existing cube upload, never a new mesh slot.
    pub(super) const fn cube(self) -> usize {
        (self as usize) << 28
    }
    pub(super) const fn material(self) -> Material {
        match self {
            Self::ArchitecturalPanel => Material::ArchitecturalPanel,
            Self::CoatedPanel => Material::CoatedPanel,
            Self::GripDeck => Material::GripDeck,
            Self::Composite => Material::Composite,
            Self::ServiceCeiling => Material::ServiceCeiling,
            Self::ExposedAlloy => Material::BareMetal,
            Self::EnamelTrim => Material::PaintedHull,
            Self::InstrumentGlass => Material::Glass,
            Self::FixtureDiffuser => Material::FixtureDiffuser,
        }
    }
}
pub(super) fn material(mesh: usize) -> Option<Material> {
    // Only typed cube instances participate. Embedded source and other geometry
    // retain their own surface contract even if given unrelated high bits.
    if super::mesh_index(mesh) != 0 {
        return None;
    }
    let role = match (mesh & SURFACE_MASK) >> 28 {
        1 => SurfaceRole::CoatedPanel,
        2 => SurfaceRole::GripDeck,
        3 => SurfaceRole::Composite,
        4 => SurfaceRole::ExposedAlloy,
        5 => SurfaceRole::EnamelTrim,
        6 => SurfaceRole::InstrumentGlass,
        7 => SurfaceRole::FixtureDiffuser,
        8 => SurfaceRole::ServiceCeiling,
        9 => SurfaceRole::ArchitecturalPanel,
        _ => return None,
    };
    Some(role.material())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight3d::{
        cockpit_occlusion::{DepthDraw, DepthMeshCatalog},
        *,
    };
    #[test]
    fn typed_roles_retain_exact_cube_upload_depth_and_fallback_independence() {
        let vertices: Arc<[Vertex]> = Arc::from(cube());
        let catalog = DepthMeshCatalog::new(vec![vertices.clone()]);
        let base = DepthDraw::whole(&catalog, 0, Mat4::IDENTITY).unwrap();
        for role in [
            SurfaceRole::CoatedPanel,
            SurfaceRole::ArchitecturalPanel,
            SurfaceRole::GripDeck,
            SurfaceRole::Composite,
            SurfaceRole::ServiceCeiling,
            SurfaceRole::ExposedAlloy,
            SurfaceRole::EnamelTrim,
            SurfaceRole::InstrumentGlass,
            SurfaceRole::FixtureDiffuser,
        ] {
            let mesh = role.cube();
            assert_eq!(mesh_index(mesh), 0);
            assert_eq!(catalog.vertex_count(mesh), vertices.len());
            let actual = DepthDraw::whole(&catalog, mesh, Mat4::IDENTITY).unwrap();
            assert_eq!(actual.mesh, base.mesh);
            assert_eq!(actual.model, base.model);
            assert_eq!(actual.vertices, base.vertices);
            let expected = role.material().parameters();
            for fallback in [Material::PaintedHull, Material::BareMetal] {
                assert_eq!(resolved_material_parameters(mesh, fallback), expected);
            }
            assert!(mesh_vertices(mesh)
                .iter()
                .zip(vertices.iter())
                .all(|(a, b)| a.p == b.p && a.n == b.n));
        }
        assert_eq!(material(0), None);
        assert_eq!(material(SURFACE_MASK), None);
        assert_eq!(material(SurfaceRole::ExposedAlloy.cube() | 10), None);
    }
    #[test]
    fn material_tags_preserve_independent_paint_and_emission_preview() {
        let preview = crate::EmbeddedMaterialPreview {
            hull_rgb: [0.8, 0.1, 0.2],
            trim_rgb: [0.2, 0.1, 0.8],
            emissive_strength: 2.,
            ..Default::default()
        };
        let tagged = SurfaceRole::CoatedPanel.cube();
        assert_eq!(paint_role(tagged | HULL_ROLE), HULL_ROLE);
        assert_eq!(paint_role(tagged | TRIM_ROLE), TRIM_ROLE);
        assert_eq!(
            preview_material(tagged, Vec3::ONE, 0.5, Some(preview)),
            (Vec3::ONE, 0.5)
        );
        assert_eq!(
            preview_material(tagged | HULL_ROLE, Vec3::ONE, 0., Some(preview)).0,
            Vec3::from_array(preview.hull_rgb)
        );
        assert_eq!(
            preview_material(tagged | EMISSION_ROLE, Vec3::ONE, 0.5, Some(preview)).1,
            1.
        );
    }
}
