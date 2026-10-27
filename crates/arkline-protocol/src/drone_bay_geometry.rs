//! Shared metre-space presentation of accepted bay motion. Sampling never advances
//! lifecycle, releases a cradle, or proves swept hardware clearance.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BayPose {
    pub position_m: [f64; 3],
    /// Unit quaternion, x/y/z/w, matching ship attitude. No heading-only fallback.
    pub orientation_xyzw: [f64; 4],
}
impl BayPose {
    pub fn valid(self) -> bool {
        self.position_m.iter().all(|v| v.is_finite())
            && self.orientation_xyzw.iter().all(|v| v.is_finite())
            && (self.orientation_xyzw.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-6
    }
    fn unit_orientation(self) -> [f64; 4] {
        let length = self
            .orientation_xyzw
            .iter()
            .map(|v| v * v)
            .sum::<f64>()
            .sqrt();
        self.orientation_xyzw.map(|v| v / length)
    }
    pub fn transform_vector(self, vector: [f64; 3]) -> Option<[f64; 3]> {
        if !self.valid() || !vector.iter().all(|v| v.is_finite()) {
            return None;
        }
        let q = self.unit_orientation();
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let u = [q[0], q[1], q[2]];
        let t = cross(u, vector).map(|v| 2. * v);
        let second = cross(u, t);
        let result = std::array::from_fn::<_, 3, _>(|i| vector[i] + q[3] * t[i] + second[i]);
        result.iter().all(|v| v.is_finite()).then_some(result)
    }
    /// Align body +X with an already-authoritative steering direction using the
    /// minimal world rotation. This preserves bank rather than rebuilding attitude
    /// from global up when a drone leaves a rolled carrier.
    pub fn with_forward(self, direction: [f64; 3]) -> Option<Self> {
        if !self.valid() || !direction.iter().all(|v| v.is_finite()) {
            return None;
        }
        let length = direction[0].hypot(direction[1]).hypot(direction[2]);
        if !length.is_finite() || length <= 1e-12 {
            return None;
        }
        let target = direction.map(|v| v / length);
        let forward = self.transform_vector([1., 0., 0.])?;
        let dot = forward
            .iter()
            .zip(target)
            .map(|(a, b)| a * b)
            .sum::<f64>()
            .clamp(-1., 1.);
        let cross = [
            forward[1] * target[2] - forward[2] * target[1],
            forward[2] * target[0] - forward[0] * target[2],
            forward[0] * target[1] - forward[1] * target[0],
        ];
        let delta = if dot < 0. && cross[0].hypot(cross[1]).hypot(cross[2]) < 1e-12 {
            let up = self.transform_vector([0., 1., 0.])?;
            [up[0], up[1], up[2], 0.]
        } else {
            let mut q = [cross[0], cross[1], cross[2], 1. + dot];
            let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
            q.iter_mut().for_each(|v| *v /= norm);
            q
        };
        let rotation = Self {
            position_m: [0.; 3],
            orientation_xyzw: delta,
        }
        .compose(Self {
            position_m: [0.; 3],
            ..self
        })?;
        Some(Self {
            orientation_xyzw: rotation.orientation_xyzw,
            ..self
        })
    }

    /// Rigid child-to-world composition; both inputs must be authoritative,
    /// finite unit poses. The stationary-carrier policy is enforced by server.
    pub fn compose(self, child: Self) -> Option<Self> {
        if !child.valid() {
            return None;
        }
        let rotated = self.transform_vector(child.position_m)?;
        let [ax, ay, az, aw] = self.unit_orientation();
        let [bx, by, bz, bw] = child.unit_orientation();
        let result = Self {
            position_m: std::array::from_fn(|i| self.position_m[i] + rotated[i]),
            orientation_xyzw: [
                aw * bx + ax * bw + ay * bz - az * by,
                aw * by - ax * bz + ay * bw + az * bx,
                aw * bz + ax * by - ay * bx + az * bw,
                aw * bw - ax * bx - ay * by - az * bz,
            ],
        };
        result.valid().then_some(result)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BayMotionSegment {
    pub source: BayPose,
    pub target: BayPose,
    pub started_at_ms: u64,
    pub duration_ms: u64,
}
impl BayMotionSegment {
    /// Smooth bounded translation and shortest-arc normalized attitude blend.
    /// Zero endpoint velocity permits safe stationary holds between axis routes.
    /// Invalid or zero-duration segments are unavailable, never teleported.
    pub fn sample(self, now: u64) -> Option<(BayPose, [f64; 3])> {
        if !self.source.valid()
            || !self.target.valid()
            || self.duration_ms == 0
            || self.started_at_ms.checked_add(self.duration_ms).is_none()
        {
            return None;
        }
        if now <= self.started_at_ms {
            return Some((self.source, [0.; 3]));
        }
        if now >= self.started_at_ms + self.duration_ms {
            return Some((self.target, [0.; 3]));
        }
        let t = (now - self.started_at_ms) as f64 / self.duration_ms as f64;
        let smooth = t * t * (3. - 2. * t);
        let rate = 6. * t * (1. - t) / (self.duration_ms as f64 / 1000.);
        let dot = self
            .source
            .orientation_xyzw
            .iter()
            .zip(self.target.orientation_xyzw)
            .map(|(a, b)| a * b)
            .sum::<f64>();
        let target = self
            .target
            .orientation_xyzw
            .map(|v| if dot < 0. { -v } else { v });
        let mut q = std::array::from_fn::<_, 4, _>(|i| {
            self.source.orientation_xyzw[i] * (1. - smooth) + target[i] * smooth
        });
        let length = q.iter().map(|v| v * v).sum::<f64>().sqrt();
        q.iter_mut().for_each(|v| *v /= length);
        let pose = BayPose {
            position_m: std::array::from_fn(|i| {
                self.source.position_m[i]
                    + (self.target.position_m[i] - self.source.position_m[i]) * smooth
            }),
            orientation_xyzw: q,
        };
        let velocity =
            std::array::from_fn(|i| (self.target.position_m[i] - self.source.position_m[i]) * rate);
        (pose.valid() && velocity.iter().all(|v| v.is_finite())).then_some((pose, velocity))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pose(p: [f64; 3]) -> BayPose {
        BayPose {
            position_m: p,
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    #[test]
    fn segment_clamps_without_claiming_completion_and_has_continuous_endpoints() {
        let s = BayMotionSegment {
            source: pose([0., 0., 0.]),
            target: pose([7., 2., -6.]),
            started_at_ms: 100,
            duration_ms: 2000,
        };
        assert_eq!(s.sample(0), Some((s.source, [0.; 3])));
        assert_eq!(s.sample(u64::MAX), Some((s.target, [0.; 3])));
        let (mid, v) = s.sample(1100).unwrap();
        assert_eq!(mid.position_m, [3.5, 1., -3.]);
        assert_eq!(v, [5.25, 1.5, -4.5]);
        for now in 100..=2100 {
            let (p, _) = s.sample(now).unwrap();
            assert!((0. ..=7.).contains(&p.position_m[0]));
            assert!((-6. ..=0.).contains(&p.position_m[2]));
        }
        let (_, v) = s.sample(2099).unwrap();
        assert!(v.iter().all(|x| x.abs() < 0.02));
    }
    #[test]
    fn translated_rolled_parent_transforms_body_and_attitude_once() {
        let a = std::f64::consts::FRAC_PI_4;
        let parent = BayPose {
            position_m: [100., 200., 300.],
            orientation_xyzw: [a.sin(), 0., 0., a.cos()],
        };
        let world = parent.compose(pose([2., 3., 4.])).unwrap();
        for (actual, expected) in world.position_m.into_iter().zip([102., 196., 303.]) {
            assert!((actual - expected).abs() < 1e-10);
        }
        assert_eq!(world.orientation_xyzw, parent.orientation_xyzw);
        assert!(parent.transform_vector([0., 1.7e308, 1.7e308]).is_none());
        let approximate = BayPose {
            orientation_xyzw: [0., 0., 0., (1.0_f64 + 0.75e-6).sqrt()],
            ..pose([0.; 3])
        };
        assert!(approximate.valid());
        assert!(approximate.compose(approximate).unwrap().valid());
        assert!(parent
            .compose(BayPose {
                orientation_xyzw: [0.; 4],
                ..pose([0.; 3])
            })
            .is_none());
    }
    #[test]
    fn free_heading_updates_retain_bank_and_handle_antiparallel_turns() {
        let a = std::f64::consts::FRAC_PI_4;
        let rolled = BayPose {
            position_m: [12., 34., 56.],
            orientation_xyzw: [a.sin(), 0., 0., a.cos()],
        };
        assert_eq!(rolled.with_forward([1., 0., 0.]).unwrap(), rolled);
        let turned = rolled.with_forward([0., 0., 1.]).unwrap();
        for (actual, expected) in turned
            .transform_vector([1., 0., 0.])
            .unwrap()
            .into_iter()
            .zip([0., 0., 1.])
        {
            assert!((actual - expected).abs() < 1e-9);
        }
        assert_eq!(turned.position_m, rolled.position_m);
        let reverse = rolled.with_forward([-1., 0., 0.]).unwrap();
        for (actual, expected) in reverse
            .transform_vector([0., 1., 0.])
            .unwrap()
            .into_iter()
            .zip(rolled.transform_vector([0., 1., 0.]).unwrap())
        {
            assert!((actual - expected).abs() < 1e-9);
        }
        assert!(rolled.with_forward([0.; 3]).is_none());
        assert!(rolled.with_forward([f64::NAN, 0., 0.]).is_none());
    }

    #[test]
    fn invalid_segments_fail_closed_and_antipodal_attitudes_do_not_spin() {
        let s = BayMotionSegment {
            source: pose([0.; 3]),
            target: BayPose {
                orientation_xyzw: [0., 0., 0., -1.],
                ..pose([1.; 3])
            },
            started_at_ms: 0,
            duration_ms: 100,
        };
        assert_eq!(s.sample(50).unwrap().0.orientation_xyzw, [0., 0., 0., 1.]);
        assert!(BayMotionSegment {
            target: pose([1e308, 0., 0.]),
            duration_ms: 2,
            ..s
        }
        .sample(1)
        .is_none());
        assert!(BayMotionSegment {
            duration_ms: 0,
            ..s
        }
        .sample(1)
        .is_none());
        assert!(BayMotionSegment {
            started_at_ms: u64::MAX,
            ..s
        }
        .sample(1)
        .is_none());
        assert!(BayMotionSegment {
            source: pose([f64::NAN, 0., 0.]),
            ..s
        }
        .sample(1)
        .is_none());
    }
}
