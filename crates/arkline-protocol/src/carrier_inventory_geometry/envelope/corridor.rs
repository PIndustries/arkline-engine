//! Narrow geometric refinement only. This does not grant movement permission:
//! authority must independently prove the actual inventory/turret is secured.
//! The ordinary whole-source envelope remains the broadphase everywhere else.
use super::*;
use crate::{
    docking_berth_orientation, docking_berth_position, docking_gate_position, DockingSegment,
    SceneLayoutRevision, DOCKING_BERTH_COUNT,
};
use sha2::{Digest, Sha256};

/// Source-certified union, NOT a smaller substitute hull. Whole triangles whose
/// minimum Y is below -3.55 occupy Z +/-25.649999618530273; the first component
/// encloses them with 1mm padding. Every other whole triangle and all supported
/// authored hardware/body/turret domains occupy Y >= -3.55 and the second box.
/// Both components retain the global source X and upperY extents.
fn compound(registration: &CarrierInventoryRegistration) -> Option<[CarrierEnvelope; 2]> {
    let outer = source_envelope(registration, CarrierEnvelopeState::Operational)?;
    let [lo, hi] = outer.bounds_m;
    Some([
        CarrierEnvelope {
            bounds_m: [
                [lo[0], lo[1], -25.650999618530274],
                [hi[0], hi[1], 25.650999618530274],
            ],
        },
        CarrierEnvelope {
            bounds_m: [[lo[0], -3.551, lo[2]], hi],
        },
    ])
}

// Populated only from the actual Rust-exported scene, not the Python mirror.
const STATION_SHA256: &str = "300c7464c5950b793c632c3c56fcd3eb08fd0e487aea34dc28fc2ff0e3298c4b";
const WALL_MARGIN_M: f64 = 0.001;

fn station_fingerprint(boxes: &[([f64; 3], [f64; 3])]) -> Option<String> {
    if boxes.len() != 128
        || boxes.iter().any(|(lo, hi)| {
            (0..3).any(|i| !lo[i].is_finite() || !hi[i].is_finite() || lo[i] > hi[i])
        })
    {
        return None;
    }
    let mut hash = Sha256::new();
    hash.update(b"arkline-carrier-corridor-station-v1\0DockingV1\0");
    hash.update(DOCKING_BERTH_COUNT.to_le_bytes());
    for id in 0..DOCKING_BERTH_COUNT {
        hash.update(id.to_le_bytes());
        for v in docking_berth_position(id)
            .into_iter()
            .chain(docking_gate_position(id))
            .chain(docking_berth_orientation())
        {
            if !v.is_finite() {
                return None;
            }
            hash.update(v.to_bits().to_le_bytes());
        }
    }
    hash.update((boxes.len() as u64).to_le_bytes());
    for (lo, hi) in boxes {
        for v in lo.iter().chain(hi) {
            hash.update(v.to_bits().to_le_bytes());
        }
    }
    Some(hex::encode(hash.finalize()))
}
fn aligned(q: [f64; 4]) -> bool {
    // No rotation, even an apparently small tilt, is certified by this route.
    q == [0., 1., 0., 0.] || q == [0., -1., 0., 0.]
}
fn on_line(position: [f64; 3], berth: u16) -> bool {
    let port = docking_berth_position(berth);
    let gate = docking_gate_position(berth);
    position.iter().all(|v| v.is_finite())
        && position[1] == port[1]
        && position[2] == port[2]
        && position[0] >= port[0]
        && position[0] <= gate[0]
}

/// Certifies only a zero-tangent, constant-attitude Hermite segment on the
/// assigned 250m gate-to-berth line. Its interpolation is monotone in X, so the
/// endpoint AABB union encloses the ENTIRE motion, not a sampled chord.
/// Every unchanged station obstacle is tested with an additional 1mm margin.
/// Unknown source/scene/obstacle geometry or a noncertified trajectory rejects.
/// Caller must separately enforce berth ownership, secured handling, peer and
/// exterior-route hazards, and all existing normal-world admission policy.
pub fn source_corridor_geometry_clear(
    registration: &CarrierInventoryRegistration,
    scene: SceneLayoutRevision,
    assigned_berth: u16,
    segment: &DockingSegment,
    station_boxes: &[([f64; 3], [f64; 3])],
) -> bool {
    if scene != SceneLayoutRevision::DockingV1
        || assigned_berth >= DOCKING_BERTH_COUNT
        || segment.duration_ms == 0
        || segment
            .started_at_ms
            .checked_add(segment.duration_ms)
            .is_none()
        || segment.initial_velocity_mps != [0.; 3]
        || !aligned(segment.source_orientation)
        || !aligned(segment.target_orientation)
        || !on_line(segment.source_m, assigned_berth)
        || !on_line(segment.target_m, assigned_berth)
        || station_fingerprint(station_boxes).as_deref() != Some(STATION_SHA256)
    {
        return false;
    }
    let Some(parts) = compound(registration) else {
        return false;
    };
    parts.iter().all(|part| {
        let poses = [segment.source_m, segment.target_m].map(|position_m| BayPose {
            position_m,
            orientation_xyzw: segment.source_orientation,
        });
        let Some(a) = part.transformed_bounds(poses[0]) else {
            return false;
        };
        let Some(b) = part.transformed_bounds(poses[1]) else {
            return false;
        };
        let lo: [f64; 3] = std::array::from_fn(|i| a[0][i].min(b[0][i]));
        let hi: [f64; 3] = std::array::from_fn(|i| a[1][i].max(b[1][i]));
        station_boxes.iter().all(|(wall_lo, wall_hi)| {
            (0..3)
                .any(|i| hi[i] <= wall_lo[i] - WALL_MARGIN_M || lo[i] >= wall_hi[i] + WALL_MARGIN_M)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registration() -> CarrierInventoryRegistration {
        parallel_fixture()
            .unwrap()
            .definition
            .topology
            .registration
            .clone()
    }
    fn boxes() -> Vec<([f64; 3], [f64; 3])> {
        (0..DOCKING_BERTH_COUNT)
            .flat_map(crate::docking_platform_boxes)
            .collect()
    }
    fn segment(berth: u16) -> DockingSegment {
        DockingSegment {
            source_m: docking_gate_position(berth),
            target_m: docking_berth_position(berth),
            initial_velocity_mps: [0.; 3],
            source_orientation: docking_berth_orientation(),
            target_orientation: docking_berth_orientation(),
            started_at_ms: 1000,
            duration_ms: 10000,
        }
    }
    #[test]
    fn certified_corridor_all_assigned_berths_entire_zero_tangent_sweep() {
        let reg = registration();
        let boxes = boxes();
        assert_eq!(station_fingerprint(&boxes).as_deref(), Some(STATION_SHA256));
        for berth in 0..DOCKING_BERTH_COUNT {
            let mut s = segment(berth);
            assert!(source_corridor_geometry_clear(
                &reg,
                SceneLayoutRevision::DockingV1,
                berth,
                &s,
                &boxes
            ));
            std::mem::swap(&mut s.source_m, &mut s.target_m);
            s.target_orientation = s.target_orientation.map(|v| -v);
            assert!(source_corridor_geometry_clear(
                &reg,
                SceneLayoutRevision::DockingV1,
                berth,
                &s,
                &boxes
            ));
            s.target_m = s.source_m;
            assert!(source_corridor_geometry_clear(
                &reg,
                SceneLayoutRevision::DockingV1,
                berth,
                &s,
                &boxes
            ));
        }
        assert_eq!(
            source_envelope(&reg, CarrierEnvelopeState::Operational)
                .unwrap()
                .bounds_m[1][2],
            41.3
        );
    }
    #[test]
    fn certified_corridor_rejects_altered_station_source_scene_and_assignment() {
        let reg = registration();
        let boxes = boxes();
        let s = segment(0);
        let clear = |r: &CarrierInventoryRegistration, scene, berth, b: &[([f64; 3], [f64; 3])]| {
            source_corridor_geometry_clear(r, scene, berth, &s, b)
        };
        for scene in [
            SceneLayoutRevision::LegacyV1,
            SceneLayoutRevision::OrbitalV1,
        ] {
            assert!(!clear(&reg, scene, 0, &boxes));
        }
        assert!(!clear(&reg, SceneLayoutRevision::DockingV1, 1, &boxes));
        assert!(!clear(
            &reg,
            SceneLayoutRevision::DockingV1,
            DOCKING_BERTH_COUNT,
            &boxes
        ));
        let mut wrong = reg.clone();
        wrong.assembly_source_sha256.replace_range(..1, "0");
        assert!(!clear(&wrong, SceneLayoutRevision::DockingV1, 0, &boxes));
        wrong = reg.clone();
        wrong.host.geometry_revision += 1;
        assert!(!clear(&wrong, SceneLayoutRevision::DockingV1, 0, &boxes));
        for index in [0, 1, boxes.len() - 1] {
            for delta in [-0.01, 0.01] {
                let mut changed = boxes.clone();
                changed[index].1[2] += delta;
                assert!(!clear(&reg, SceneLayoutRevision::DockingV1, 0, &changed));
            }
        }
        assert!(!clear(
            &reg,
            SceneLayoutRevision::DockingV1,
            0,
            &boxes[..127]
        ));
        let mut changed = boxes.clone();
        changed[0].0[0] = f64::NAN;
        assert!(!clear(&reg, SceneLayoutRevision::DockingV1, 0, &changed));
    }
    #[test]
    fn certified_corridor_rejects_noncertified_rotation_tangent_offsets_and_overflow() {
        let reg = registration();
        let boxes = boxes();
        let base = segment(0);
        let reject = |s: &DockingSegment| {
            assert!(!source_corridor_geometry_clear(
                &reg,
                SceneLayoutRevision::DockingV1,
                0,
                s,
                &boxes
            ))
        };
        for axis in 0..3 {
            let mut s = base.clone();
            s.initial_velocity_mps[axis] = 0.001;
            reject(&s);
        }
        for axis in [1, 2] {
            let mut s = base.clone();
            s.source_m[axis] += 0.001;
            reject(&s);
            s = base.clone();
            s.target_m[axis] -= 0.001;
            reject(&s);
        }
        let mut s = base.clone();
        s.source_m[0] += 0.001;
        reject(&s);
        s = base.clone();
        s.target_m[0] -= 0.001;
        reject(&s);
        s = base.clone();
        s.source_orientation = [0., (1. - 1e-12f64).sqrt(), 0., 1e-6];
        reject(&s);
        s = base.clone();
        s.target_orientation = [0., 0., 0., 1.];
        reject(&s);
        s = base.clone();
        s.source_m[0] = f64::NAN;
        reject(&s);
        s = base.clone();
        s.duration_ms = 0;
        reject(&s);
        s = base;
        s.started_at_ms = u64::MAX;
        reject(&s);
    }
    #[test]
    fn certified_compound_contains_full_handling_boxes_not_only_origins() {
        let f = parallel_fixture().unwrap();
        let parts = compound(&registration()).unwrap();
        let identity = BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let bounds = parts.map(|p| p.transformed_bounds(identity).unwrap());
        let check = |body: [[f64; 3]; 2], pose: BayPose| {
            let corners: Vec<[f64; 3]> = (0..8)
                .map(|mask| {
                    let point = std::array::from_fn(|i| body[(mask >> i) & 1][i]);
                    let point = pose.transform_vector(point).unwrap();
                    std::array::from_fn(|i| point[i] + pose.position_m[i])
                })
                .collect();
            assert!(
                bounds.iter().any(|b| corners
                    .iter()
                    .all(|p| (0..3).all(|i| p[i] >= b[0][i] && p[i] <= b[1][i]))),
                "unsupported body {body:?} at {pose:?}"
            );
        };
        let mut routes: Vec<_> = f
            .stages
            .iter()
            .map(|r| r.keyframes.clone())
            .chain(f.launches.iter().map(|r| r.keyframes.clone()))
            .collect();
        for berth in 0..10u8 {
            for cid in [u64::from(berth), u64::from(berth) + 10] {
                for class in [DroneClass::Mining, DroneClass::Recon, DroneClass::Attack] {
                    routes.push(
                        f.definition
                            .bay_route(berth, class, cid, CarrierInventoryAction::RecallToStorage)
                            .unwrap(),
                    );
                }
                routes.push(
                    f.definition
                        .bay_route(
                            berth,
                            DroneClass::Mining,
                            cid,
                            CarrierInventoryAction::ReturnToUnload {
                                resume_mining: false,
                            },
                        )
                        .unwrap(),
                );
            }
        }
        for route in routes {
            for frame in route {
                for (id, pose) in frame.machinery {
                    check(
                        f.definition
                            .mechanisms
                            .iter()
                            .find(|m| m.semantic_id == id)
                            .unwrap()
                            .local_bounds_m,
                        pose,
                    );
                }
                for (id, pose) in frame.cassette_poses {
                    check(
                        f.definition
                            .cassettes
                            .iter()
                            .find(|c| c.cassette_id == id)
                            .unwrap()
                            .bounds_local_m,
                        pose,
                    );
                }
                if let Some(pose) = frame.drone_pose {
                    check(f.definition.berths[0].drone_bounds_local_m, pose);
                }
            }
        }
    }
}
