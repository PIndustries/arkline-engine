//! Test-only stable numeric fingerprint. Shared with the exact pre-refactor
//! source exporter; never used to authorize movement or replace source binding.
use crate::drone_bay_geometry::BayPose;
use crate::drone_bay_layout::BayRoute;
use sha2::{Digest, Sha256};
pub fn route_fingerprint(route: &BayRoute) -> String {
    fn pose(hash: &mut Sha256, p: BayPose) {
        for v in p.position_m.into_iter().chain(p.orientation_xyzw) {
            hash.update(v.to_bits().to_le_bytes());
        }
    }
    let mut hash = Sha256::new();
    hash.update([route.slot, route.direction as u8]);
    hash.update((route.keyframes.len() as u64).to_le_bytes());
    for frame in &route.keyframes {
        pose(&mut hash, frame.drone_body_pose);
        for p in frame.mechanism_body_roots {
            pose(&mut hash, p);
        }
    }
    for (i, kind) in route.segment_kinds.iter().enumerate() {
        hash.update([*kind as u8]);
        let c = route.segment_contract(i).unwrap();
        hash.update([
            c.drone_binding as u8,
            c.endpoint_support[0] as u8,
            c.endpoint_support[1] as u8,
            c.may_detach_after_endpoint as u8,
            c.handling_required_after_endpoint as u8,
            c.throat_swept as u8,
        ]);
        for v in c
            .door_open_fraction
            .into_iter()
            .chain(c.drone_swept_envelope.min_m)
            .chain(c.drone_swept_envelope.max_m)
            .chain(c.machinery_working_envelope.min_m)
            .chain(c.machinery_working_envelope.max_m)
        {
            hash.update(v.to_bits().to_le_bytes());
        }
        for elapsed in [0, 1, 257, 500, 999, 1000, 1001] {
            let frame = route
                .sample_segment(i, 10000, 1000, 10000 + elapsed)
                .unwrap();
            pose(&mut hash, frame.drone_body_pose);
            for p in frame.mechanism_body_roots {
                pose(&mut hash, p);
            }
        }
    }
    let m = route.milestones();
    for i in [
        m.drone_hull_clear_keyframe,
        m.incoming_alignment_keyframe,
        m.capture_supported_keyframe,
        m.drone_home_keyframe,
        Some(m.doors_secured_keyframe),
        Some(m.handling_idle_keyframe),
    ] {
        hash.update(i.map_or(u64::MAX, |v| v as u64).to_le_bytes());
    }
    hex::encode(hash.finalize())
}
