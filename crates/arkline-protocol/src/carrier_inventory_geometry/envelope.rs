//! Source-qualified broadphase only, not collision leaves or travel admission.
use super::*;
mod corridor;
pub use corridor::source_corridor_geometry_clear;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierEnvelopeState {
    /// Caller must establish all hardware and carried bodies are closed/stowed.
    Closed,
    /// Includes held/intermediate Stage, Launch and exact reverse poses, and the
    /// complete turret rotation/drop/barrel union. External drone approach and
    /// dispersal routes still require their independent retained hazards.
    Operational,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CarrierEnvelope {
    pub bounds_m: [[f64; 3]; 2],
}
impl CarrierEnvelope {
    fn valid(self) -> bool {
        (0..3).all(|i| {
            self.bounds_m[0][i].is_finite()
                && self.bounds_m[1][i].is_finite()
                && self.bounds_m[0][i] <= self.bounds_m[1][i]
        })
    }
    // Conservative f64 budget for normalized quaternion arithmetic and final
    // world translation. Do not let AU-scale rounding shrink an endpoint box.
    fn rounding_margin_m(self, position: [f64; 3]) -> f64 {
        128. * f64::EPSILON
            * self
                .bounds_m
                .into_iter()
                .flatten()
                .chain(position)
                .map(f64::abs)
                .fold(1., f64::max)
    }
    pub fn center_m(self) -> [f64; 3] {
        std::array::from_fn(|i| (self.bounds_m[0][i] + self.bounds_m[1][i]) * 0.5)
    }
    pub fn half_extents_m(self) -> [f64; 3] {
        std::array::from_fn(|i| (self.bounds_m[1][i] - self.bounds_m[0][i]) * 0.5)
    }
    /// Radius about the actual parent origin, NOT the offset box center.
    pub fn origin_radius_m(self) -> Option<f64> {
        if !self.valid() {
            return None;
        }
        let radius = (0..3)
            .map(|i| {
                self.bounds_m[0][i]
                    .abs()
                    .max(self.bounds_m[1][i].abs())
                    .powi(2)
            })
            .sum::<f64>()
            .sqrt();
        let radius = radius + self.rounding_margin_m([0.; 3]);
        radius.is_finite().then_some(radius)
    }
    pub fn transformed_bounds(self, pose: BayPose) -> Option<[[f64; 3]; 2]> {
        if !self.valid() {
            return None;
        }
        let center = pose.transform_vector(self.center_m())?;
        let half = self.half_extents_m();
        let mut extent = [0.; 3];
        for (axis, half) in half.into_iter().enumerate() {
            let mut v = [0.; 3];
            v[axis] = half;
            let v = pose.transform_vector(v)?;
            for i in 0..3 {
                extent[i] += v[i].abs();
            }
        }
        let margin = self.rounding_margin_m(pose.position_m);
        let bounds = [
            std::array::from_fn(|i| pose.position_m[i] + center[i] - extent[i] - margin),
            std::array::from_fn(|i| pose.position_m[i] + center[i] + extent[i] + margin),
        ];
        Self { bounds_m: bounds }.valid().then_some(bounds)
    }
    /// Signed distance from parent origin to the lowest support plane along
    /// the supplied unit outward normal; preserves the asymmetric center.
    pub fn support_m(self, pose: BayPose, normal: [f64; 3]) -> Option<f64> {
        if !self.valid()
            || !normal.iter().all(|n| n.is_finite())
            || (normal.iter().map(|n| n * n).sum::<f64>() - 1.).abs() > 1e-6
        {
            return None;
        }
        let normal_length = normal.iter().map(|n| n * n).sum::<f64>().sqrt();
        let normal = normal.map(|n| n / normal_length);
        let center = pose.transform_vector(self.center_m())?;
        let mut support = -center.iter().zip(normal).map(|(a, b)| a * b).sum::<f64>();
        let half = self.half_extents_m();
        for (axis, half) in half.into_iter().enumerate() {
            let mut v = [0.; 3];
            v[axis] = half;
            support += pose
                .transform_vector(v)?
                .iter()
                .zip(normal)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                .abs();
        }
        let support = support + self.rounding_margin_m([0.; 3]);
        support.is_finite().then_some(support)
    }
}
/// Exact compiled 1703 source only. A class, approximate revision or replacement
/// hash never authorizes a fallback. This function does not change lookup().
pub fn source_envelope(
    registration: &CarrierInventoryRegistration,
    state: CarrierEnvelopeState,
) -> Option<CarrierEnvelope> {
    if registration.assembly_revision != 1703 {
        return None;
    }
    let definition = stationary_fixture_for_registration(registration)?;
    let mut bounds_m = match state {
        CarrierEnvelopeState::Closed => definition.carrier_bounds_m,
        CarrierEnvelopeState::Operational => definition.deployed_hardware_bounds_m,
    };
    if state == CarrierEnvelopeState::Operational {
        // Source launch/body exterior clear: ±38.8m body origin + 2.5m
        // rotated full-body support. Includes all source keyframes and their
        // fixed-attitude linear segments; exact reverse has the same domain.
        bounds_m[0][2] = bounds_m[0][2].min(-41.3);
        bounds_m[1][2] = bounds_m[1][2].max(41.3);
    }
    Some(CarrierEnvelope { bounds_m })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn exact(state: CarrierEnvelopeState) -> CarrierEnvelope {
        source_envelope(
            &parallel_fixture().unwrap().definition.topology.registration,
            state,
        )
        .unwrap()
    }
    fn identity() -> BayPose {
        BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    #[test]
    fn envelope_rejects_unknown_and_retains_historical_definitions() {
        let mut reg = parallel_fixture()
            .unwrap()
            .definition
            .topology
            .registration
            .clone();
        reg.assembly_source_sha256.replace_range(..1, "0");
        assert!(source_envelope(&reg, CarrierEnvelopeState::Operational).is_none());
        reg = parallel_fixture()
            .unwrap()
            .definition
            .topology
            .registration
            .clone();
        reg.host.source_sha256.replace_range(..1, "0");
        assert!(source_envelope(&reg, CarrierEnvelopeState::Operational).is_none());
        for f in [fixture().unwrap(), composed_fixture().unwrap()] {
            assert!(source_envelope(
                &f.definition.topology.registration,
                CarrierEnvelopeState::Closed
            )
            .is_none());
            assert_eq!(
                stationary_fixture_for_registration(&f.definition.topology.registration).unwrap(),
                &f.definition
            );
        }
        assert!(lookup(&parallel_fixture().unwrap().definition.topology.registration).is_none());
    }
    #[test]
    fn asymmetric_support_and_transform_preserve_origin_and_full_attitude() {
        let closed = exact(CarrierEnvelopeState::Closed);
        let operational = exact(CarrierEnvelopeState::Operational);
        assert_eq!(closed.bounds_m[0][1], -9.001);
        assert_eq!(closed.bounds_m[1][1], 11.351000381469726);
        assert!(closed.center_m()[1] > 1.17);
        assert!(closed.center_m()[0] < 0.);
        assert_ne!(closed.bounds_m[0][0].abs(), closed.bounds_m[1][0]);
        let support = closed.support_m(identity(), [0., 1., 0.]).unwrap();
        assert!((9.001..=9.001 + 1e-10).contains(&support));
        let rolled = BayPose {
            position_m: [100., 200., 300.],
            orientation_xyzw: [1., 0., 0., 0.],
        };
        let support = closed.support_m(rolled, [0., 1., 0.]).unwrap();
        assert!((closed.bounds_m[1][1]..=closed.bounds_m[1][1] + 1e-10).contains(&support));
        assert!(operational.support_m(rolled, [0., 1., 0.]).unwrap() > 15.15);
        let b = closed.transformed_bounds(rolled).unwrap();
        assert!(b[0][1] <= 200. - closed.bounds_m[1][1]);
        assert!((b[0][1] - (200. - closed.bounds_m[1][1])).abs() < 1e-10);
        assert!(operational.origin_radius_m().unwrap() > closed.origin_radius_m().unwrap());
        assert!(closed
            .transformed_bounds(BayPose {
                orientation_xyzw: [0.; 4],
                ..identity()
            })
            .is_none());
        assert!(closed.support_m(identity(), [0.; 3]).is_none());
        assert_eq!(
            closed.support_m(identity(), [0., 1. - 2e-7, 0.]),
            closed.support_m(identity(), [0., 1., 0.])
        );
        assert!(closed.support_m(identity(), [0., 0.99, 0.]).is_none());
    }
    #[test]
    fn full_attitude_box_contains_all_eight_actual_corners() {
        let e = exact(CarrierEnvelopeState::Operational);
        let pose = BayPose {
            position_m: [123., -456., 789.],
            orientation_xyzw: [0.5; 4],
        };
        let b = e.transformed_bounds(pose).unwrap();
        for mask in 0..8 {
            let corner = std::array::from_fn(|i| e.bounds_m[(mask >> i) & 1][i]);
            let r = pose.transform_vector(corner).unwrap();
            for (i, coordinate) in r.into_iter().enumerate() {
                let world = coordinate + pose.position_m[i];
                assert!(world >= b[0][i] && world <= b[1][i]);
            }
        }
    }
    #[test]
    fn au_scale_transformed_corners_are_contained_without_tolerance() {
        let e = exact(CarrierEnvelopeState::Operational);
        let pose = BayPose {
            position_m: [-249345559932.18948, 200524679690.15347, -299302132748.46234],
            orientation_xyzw: [
                0.3242145625984559,
                0.48221751267287766,
                0.493780200152925,
                0.6469407251113601,
            ],
        };
        for scale in [0., 1., 10., 100.] {
            let pose = BayPose {
                position_m: pose.position_m.map(|x| x * scale),
                ..pose
            };
            let b = e.transformed_bounds(pose).unwrap();
            for mask in 0..8 {
                let corner = std::array::from_fn(|i| e.bounds_m[(mask >> i) & 1][i]);
                let rotated = pose.transform_vector(corner).unwrap();
                for (i, x) in rotated.into_iter().enumerate() {
                    let world = pose.position_m[i] + x;
                    assert!(
                        world >= b[0][i] && world <= b[1][i],
                        "axis={i} scale={scale} mask={mask}: {world} outside {b:?}"
                    );
                }
            }
        }
    }
    #[test]
    fn malformed_or_overflowing_public_bounds_fail_closed() {
        for e in [
            CarrierEnvelope {
                bounds_m: [[f64::NAN; 3], [1.; 3]],
            },
            CarrierEnvelope {
                bounds_m: [[2.; 3], [1.; 3]],
            },
            CarrierEnvelope {
                bounds_m: [[f64::MAX; 3], [f64::MAX; 3]],
            },
        ] {
            assert!(e.transformed_bounds(identity()).is_none());
            assert!(e.support_m(identity(), [0., 1., 0.]).is_none());
            assert!(e.origin_radius_m().is_none());
        }
        let large = CarrierEnvelope {
            bounds_m: [[0.; 3], [f64::MAX / 2.; 3]],
        };
        assert!(large
            .transformed_bounds(BayPose {
                position_m: [f64::MAX; 3],
                ..identity()
            })
            .is_none());
    }
    #[test]
    fn operational_envelope_contains_every_held_handling_body_and_fixed_attitude_segment() {
        let f = parallel_fixture().unwrap();
        let e = exact(CarrierEnvelopeState::Operational);
        let check = |box_m, pose| {
            let b = CarrierEnvelope { bounds_m: box_m }
                .transformed_bounds(pose)
                .unwrap();
            assert!(
                (0..3)
                    .all(|i| b[0][i] >= e.bounds_m[0][i] - 1e-10
                        && b[1][i] <= e.bounds_m[1][i] + 1e-10),
                "{b:?}"
            );
        };
        let mut routes: Vec<Vec<CarrierInventoryKeyframe>> = f
            .stages
            .iter()
            .map(|r| r.keyframes.clone())
            .chain(f.launches.iter().map(|r| r.keyframes.clone()))
            .collect();
        for berth in 0..10u8 {
            for cassette in [u64::from(berth), u64::from(berth) + 10] {
                for class in [DroneClass::Mining, DroneClass::Recon, DroneClass::Attack] {
                    routes.push(
                        f.definition
                            .bay_route(
                                berth,
                                class,
                                cassette,
                                CarrierInventoryAction::RecallToStorage,
                            )
                            .unwrap(),
                    );
                }
                routes.push(
                    f.definition
                        .bay_route(
                            berth,
                            DroneClass::Mining,
                            cassette,
                            CarrierInventoryAction::ReturnToUnload {
                                resume_mining: false,
                            },
                        )
                        .unwrap(),
                );
            }
        }
        for route in &routes {
            let mut attitudes = std::collections::BTreeMap::new();
            for frame in route {
                for (id, pose) in &frame.machinery {
                    let body = f
                        .definition
                        .mechanisms
                        .iter()
                        .find(|m| &m.semantic_id == id)
                        .unwrap();
                    check(body.local_bounds_m, *pose);
                    if let Some(q) = attitudes.insert(id.clone(), pose.orientation_xyzw) {
                        assert_eq!(q, pose.orientation_xyzw);
                    }
                }
                for (id, pose) in &frame.cassette_poses {
                    let body = f
                        .definition
                        .cassettes
                        .iter()
                        .find(|c| c.cassette_id == *id)
                        .unwrap();
                    check(body.bounds_local_m, *pose);
                    if let Some(q) =
                        attitudes.insert(format!("cassette/{id}"), pose.orientation_xyzw)
                    {
                        assert_eq!(q, pose.orientation_xyzw);
                    }
                }
                if let Some(pose) = frame.drone_pose {
                    check(f.definition.berths[0].drone_bounds_local_m, pose);
                    if let Some(q) = attitudes.insert("drone".into(), pose.orientation_xyzw) {
                        assert_eq!(q, pose.orientation_xyzw);
                    }
                }
            }
        }
        assert_eq!(e.bounds_m[1][2], 41.3);
        assert_eq!(e.bounds_m[1][1], 15.156822309505582);
    }
}
