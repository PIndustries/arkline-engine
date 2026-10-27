//! Exact1703/79ab visual attachment binding. Geometry is numerically identical
//! to the separately retained1702 helper, but acceptance is NOT inherited:
//! check_parallel_thruster_clearance.py rechecks the full source and local paths.
//! This does not admit travel, simulate force/heat, or authorize emission near
//! arbitrary free bodies or ground. Runtime freshness/docking gates still apply.
use crate::carrier_thruster_geometry as retained;
pub(crate) use retained::{reaction_outward, Outward, Plume};
pub(crate) const SOURCE_SHA256: &str =
    "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a";
pub(crate) struct SourceGeometry(retained::SourceGeometry);
impl SourceGeometry {
    pub(crate) fn for_verified_glb_sha256(hash: &str) -> Option<Self> {
        (hash == SOURCE_SHA256).then(|| {
            Self(
                retained::SourceGeometry::for_verified_glb_sha256(retained::SOURCE_SHA256).unwrap(),
            )
        })
    }
    pub(crate) fn main(&self, length_m: f64, pulse: f64, layer: f64) -> Vec<Plume> {
        self.0.main(length_m, pulse, layer)
    }
    pub(crate) fn reaction(&self, outward: Outward, strength: f64) -> Vec<Plume> {
        self.0.reaction(outward, strength)
    }
    pub(crate) fn navigation_fixture_position(
        &self,
        id: &str,
        legacy: [f64; 3],
    ) -> Option<[f64; 3]> {
        self.0.navigation_fixture_position(id, legacy)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_binding_is_separate_and_old_source_path_unchanged() {
        assert!(SourceGeometry::for_verified_glb_sha256(retained::SOURCE_SHA256).is_none());
        assert!(retained::SourceGeometry::for_verified_glb_sha256(SOURCE_SHA256).is_none());
        for unknown in ["1703", "Pioneer", ""] {
            assert!(SourceGeometry::for_verified_glb_sha256(unknown).is_none());
        }
        let new = SourceGeometry::for_verified_glb_sha256(SOURCE_SHA256).unwrap();
        let old =
            retained::SourceGeometry::for_verified_glb_sha256(retained::SOURCE_SHA256).unwrap();
        for length in [1e-9, 1., 1e9] {
            for pulse in [0.9, 1., 1.1] {
                assert_eq!(new.main(length, pulse, 1.), old.main(length, pulse, 1.));
            }
        }
        for outward in [
            Outward::Forward,
            Outward::Down,
            Outward::Up,
            Outward::Port,
            Outward::Starboard,
        ] {
            assert_eq!(new.reaction(outward, 1.), old.reaction(outward, 1.));
        }
        assert_eq!(reaction_outward(0, -1.), Some(Outward::Forward));
        assert!(new.navigation_fixture_position("custom", [0.; 3]).is_none());
    }
}
