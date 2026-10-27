//! Exact complete fixed-source ray blockers. Dynamic source roots use the
//! existing compound collision provider at current authority poses.
use crate::{parallel_turret_binding, CarrierInventoryRegistration};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
#[derive(Deserialize)]
pub struct FixedSolid {
    pub source_id: String,
    pub name: String,
    pub bounds_m: [[f64; 3]; 2],
}
#[derive(Deserialize)]
struct Fixture {
    glb_sha256: String,
    manifest_sha256: String,
    triangles: usize,
    fixed: Vec<FixedSolid>,
}
pub fn fixed_solids(registration: &CarrierInventoryRegistration) -> Option<&'static [FixedSolid]> {
    if !parallel_turret_binding::exact_registration(registration) {
        return None;
    }
    static DATA: OnceLock<Option<Fixture>> = OnceLock::new();
    DATA.get_or_init(|| {
        let bytes = include_bytes!("fixtures/parallel_turret_ray_solids.json");
        if format!("{:x}", Sha256::digest(bytes))
            != "505f483dff0c3656be2711cc234c0295468de055953c8b434bb865e253db494e"
        {
            return None;
        }
        let f: Fixture = serde_json::from_slice(bytes).ok()?;
        if f.glb_sha256 != parallel_turret_binding::SOURCE_SHA256
            || f.manifest_sha256
                != "e0a93d13de11df83ee7e370c87685eca48cbc25921fe6b894e1f44020f3059e9"
            || f.triangles != 193192
            || f.fixed.len() != 1027
            || f.fixed.iter().any(|s| {
                s.source_id.is_empty()
                    || s.name.is_empty()
                    || s.bounds_m.iter().flatten().any(|v| !v.is_finite())
                    || (0..3).any(|i| s.bounds_m[0][i] > s.bounds_m[1][i])
            })
        {
            return None;
        }
        Some(f)
    })
    .as_ref()
    .map(|f| f.fixed.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parallel_turret_aim::{direction, ray_box};
    #[test]
    fn exact_complete_source_blocks_inward_bridge_but_has_forward_arc() {
        let mut reg = crate::carrier_inventory_geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
        reg.assembly_revision = 1703;
        reg.assembly_source_sha256 = parallel_turret_binding::SOURCE_SHA256.into();
        let solids = fixed_solids(&reg).unwrap();
        for (yaw, blocked) in [(-25_f64, false), (0_f64, false), (25_f64, true)] {
            let axis = direction(yaw.to_radians(), 0.);
            let origin = std::array::from_fn(|i| {
                crate::PIONEER_MINING_TURRET_MOUNTS_M[0][i] + 5.285 * axis[i]
            });
            assert_eq!(
                solids
                    .iter()
                    .any(|s| ray_box(origin, axis, s.bounds_m).is_some()),
                blocked
            );
        }
        let mut wrong = reg.clone();
        wrong.assembly_source_sha256 = "unknown".into();
        assert!(fixed_solids(&wrong).is_none());
    }
}
