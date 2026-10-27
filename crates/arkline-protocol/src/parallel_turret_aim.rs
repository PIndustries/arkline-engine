//! Sampled authoritative aim for the exact-source carrier. No client-time
//! interpolation or target clamping: requests outside the proved cone fail.
use serde::{Deserialize, Serialize};

pub const MAX_YAW_RAD: f64 = 25. * std::f64::consts::PI / 180.;
pub const MAX_PITCH_RAD: f64 = 20. * std::f64::consts::PI / 180.;
pub const SLEW_RAD_PER_SECOND: f64 = 30. * std::f64::consts::PI / 180.;
pub const SETTLE_MS: u64 = 200;
pub const ALIGNMENT_RAD: f64 = 0.05 * std::f64::consts::PI / 180.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TurretAimStatus {
    #[default]
    Unavailable,
    Aiming,
    Ready,
    OutOfArc,
    Obstructed,
    Held,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TurretAim {
    pub status: TurretAimStatus,
    pub yaw_rad: f64,
    pub pitch_rad: f64,
    pub target_id: Option<u64>,
    pub sample_ms: u64,
    pub settled_since_ms: Option<u64>,
}
impl TurretAim {
    pub fn validate_at(&self, now: u64) -> Result<(), String> {
        if !angles_valid(self.yaw_rad, self.pitch_rad)
            || self.sample_ms > now
            || self.settled_since_ms.is_some_and(|t| t > self.sample_ms)
            || (self.target_id.is_none() && self.settled_since_ms.is_some())
            || (self.status == TurretAimStatus::Ready
                && (self.target_id.is_none()
                    || self
                        .settled_since_ms
                        .is_none_or(|t| self.sample_ms.saturating_sub(t) < SETTLE_MS)))
        {
            return Err("invalid authoritative turret aim".into());
        }
        Ok(())
    }
    pub fn forward(&self) -> [f64; 3] {
        direction(self.yaw_rad, self.pitch_rad)
    }
    /// Sole authoritative shortest +X swing and homing law. The barrel is
    /// neutral by fraction0.75, before the carriage starts its physical drop.
    pub fn orientation_xyzw(&self, deployment_fraction: f64) -> Option<[f64; 4]> {
        if !angles_valid(self.yaw_rad, self.pitch_rad)
            || !deployment_fraction.is_finite()
            || !(0. ..=1.).contains(&deployment_fraction)
        {
            return None;
        }
        let d = self.forward();
        let sine = (d[1] * d[1] + d[2] * d[2]).sqrt();
        if sine < 1e-15 {
            return Some([0., 0., 0., 1.]);
        }
        let g = ((deployment_fraction - 0.75) / 0.25).clamp(0., 1.);
        let half = 0.5 * d[0].clamp(-1., 1.).acos() * g;
        Some([
            0.,
            -d[2] / sine * half.sin(),
            d[1] / sine * half.sin(),
            half.cos(),
        ])
    }
    /// New intent resets settling, never instantaneously moves a physical gun.
    pub fn request(&mut self, target_id: u64, now: u64) -> Result<(), String> {
        self.validate_at(now)?;
        if self.target_id != Some(target_id) {
            self.status = TurretAimStatus::Aiming;
            self.sample_ms = now;
            self.target_id = Some(target_id);
            self.settled_since_ms = None;
        }
        Ok(())
    }
    /// Hold the last accepted pose. Restart/disconnect never replay aim intent.
    pub fn hold(&mut self, now: u64) {
        self.status = TurretAimStatus::Held;
        self.target_id = None;
        self.settled_since_ms = None;
        self.sample_ms = self.sample_ms.max(now);
    }
    /// Integrate only accepted authority time. Long stalls never catch up a
    /// whole slew in one frame. The full cone must remain collision admitted.
    pub fn advance(&mut self, target_body_delta: [f64; 3], now: u64) -> Result<(), String> {
        self.validate_at(now)?;
        if self.target_id.is_none() {
            self.sample_ms = now;
            return Ok(());
        }
        let (yaw, pitch) =
            target_angles(target_body_delta).ok_or("target outside source-proved turret cone")?;
        let dt = now.saturating_sub(self.sample_ms).min(100) as f64 / 1000.;
        let limit = SLEW_RAD_PER_SECOND * dt;
        self.yaw_rad += (yaw - self.yaw_rad).clamp(-limit, limit);
        self.pitch_rad += (pitch - self.pitch_rad).clamp(-limit, limit);
        self.sample_ms = now;
        if angular_error(self.forward(), direction(yaw, pitch)) <= ALIGNMENT_RAD {
            self.settled_since_ms.get_or_insert(now);
        } else {
            self.settled_since_ms = None;
        }
        Ok(())
    }
    pub fn aligned_at(&self, target_body_delta: [f64; 3], now: u64) -> bool {
        self.validate_at(now).is_ok()
            && self.sample_ms == now
            && self.target_id.is_some()
            && self
                .settled_since_ms
                .is_some_and(|t| now.saturating_sub(t) >= SETTLE_MS)
            && target_angles(target_body_delta).is_some_and(|(y, p)| {
                angular_error(self.forward(), direction(y, p)) <= ALIGNMENT_RAD
            })
    }
}
fn angles_valid(yaw: f64, pitch: f64) -> bool {
    yaw.is_finite()
        && pitch.is_finite()
        && (-MAX_YAW_RAD..=MAX_YAW_RAD).contains(&yaw)
        && (0. ..=MAX_PITCH_RAD).contains(&pitch)
}
pub fn direction(yaw: f64, pitch: f64) -> [f64; 3] {
    [
        pitch.cos() * yaw.cos(),
        pitch.sin(),
        pitch.cos() * yaw.sin(),
    ]
}
pub fn target_angles(v: [f64; 3]) -> Option<(f64, f64)> {
    if v.iter().any(|v| !v.is_finite()) || v[0] <= 0. {
        return None;
    }
    let yaw = v[2].atan2(v[0]);
    let pitch = v[1].atan2(v[0].hypot(v[2]));
    // Round-off only: world-to-body subtraction at astronomical coordinates
    // can put an exact boundary a few ulps outside. Never widen physical travel.
    const EPS: f64 = 1e-10;
    ((-MAX_YAW_RAD - EPS..=MAX_YAW_RAD + EPS).contains(&yaw)
        && (-EPS..=MAX_PITCH_RAD + EPS).contains(&pitch))
    .then_some((
        yaw.clamp(-MAX_YAW_RAD, MAX_YAW_RAD),
        pitch.clamp(0., MAX_PITCH_RAD),
    ))
}
fn angular_error(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .clamp(-1., 1.)
        .acos()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn target_cone_rejects_not_clamps() {
        assert!(target_angles([1., 0., 0.]).is_some());
        for v in [
            [-1., 0., 0.],
            [1., -0.01, 0.],
            [1., 1., 0.],
            [1., 0., 1.],
            [f64::NAN, 0., 0.],
        ] {
            assert!(target_angles(v).is_none());
        }
    }
    #[test]
    fn slew_settle_hold_and_clock_are_authoritative() {
        let mut a = TurretAim::default();
        a.request(7, 0).unwrap();
        let target = direction(20_f64.to_radians(), 10_f64.to_radians());
        a.advance(target, 10_000).unwrap();
        assert!(a.yaw_rad <= 3_f64.to_radians() + 1e-12);
        assert!(!a.aligned_at(target, 10_000));
        for now in (10_100..=11_000).step_by(100) {
            a.advance(target, now).unwrap();
        }
        assert!(a.aligned_at(target, 11_000));
        assert!(!a.aligned_at(target, 11_001));
        assert!(a.advance(target, 10_999).is_err());
        let pose = (a.yaw_rad, a.pitch_rad);
        a.hold(11_001);
        assert_eq!(pose, (a.yaw_rad, a.pitch_rad));
        assert_eq!(a.target_id, None);
        assert!(!a.aligned_at(target, 11_001));
    }
    #[test]
    fn retarget_resets_settle_and_corrupt_state_fails() {
        let mut a = TurretAim::default();
        a.request(1, 0).unwrap();
        a.advance([1., 0., 0.], 0).unwrap();
        a.advance([1., 0., 0.], 200).unwrap();
        assert!(a.aligned_at([1., 0., 0.], 200));
        a.request(2, 200).unwrap();
        assert!(!a.aligned_at([1., 0., 0.], 200));
        a.yaw_rad = f64::NAN;
        assert!(a.validate_at(200).is_err());
    }
}

/// Distance to the first closed solid hit along the ACTUAL emitted axis.
/// A start inside a solid is a hit at zero; no owner-wide exemption exists.
pub fn ray_box(origin: [f64; 3], axis: [f64; 3], bounds: [[f64; 3]; 2]) -> Option<f64> {
    if origin
        .iter()
        .chain(axis.iter())
        .chain(bounds.iter().flatten())
        .any(|v| !v.is_finite())
        || (axis.iter().map(|v| v * v).sum::<f64>() - 1.).abs() > 1e-8
        || (0..3).any(|i| bounds[0][i] > bounds[1][i])
    {
        return Some(0.);
    }
    let (mut near, mut far) = (0_f64, f64::INFINITY);
    for i in 0..3 {
        if axis[i].abs() < 1e-12 {
            if origin[i] < bounds[0][i] || origin[i] > bounds[1][i] {
                return None;
            }
        } else {
            let a = (bounds[0][i] - origin[i]) / axis[i];
            let b = (bounds[1][i] - origin[i]) / axis[i];
            if !a.is_finite() || !b.is_finite() {
                return Some(0.);
            }
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
    }
    Some(near)
}
pub fn ray_sphere(origin: [f64; 3], axis: [f64; 3], center: [f64; 3], radius: f64) -> Option<f64> {
    if !radius.is_finite()
        || radius < 0.
        || origin
            .iter()
            .chain(axis.iter())
            .chain(center.iter())
            .any(|v| !v.is_finite())
        || (axis.iter().map(|v| v * v).sum::<f64>() - 1.).abs() > 1e-8
    {
        return Some(0.);
    }
    let d: [f64; 3] = std::array::from_fn(|i| origin[i] - center[i]);
    let c = d.iter().map(|v| v * v).sum::<f64>() - radius * radius;
    if !c.is_finite() {
        return Some(0.);
    }
    if c <= 0. {
        return Some(0.);
    }
    let b = d.iter().zip(axis).map(|(d, a)| d * a).sum::<f64>();
    let discriminant = b * b - c;
    if !b.is_finite() || !discriminant.is_finite() {
        return Some(0.);
    }
    if discriminant < 0. {
        return None;
    }
    let t = -b - discriminant.sqrt();
    (t >= 0.).then_some(t)
}
#[cfg(test)]
mod ray_tests {
    use super::*;
    #[test]
    fn solid_origin_and_nearer_blockers_fail_closed() {
        let box_m = [[-1.; 3], [1.; 3]];
        assert_eq!(ray_box([0.; 3], [1., 0., 0.], box_m), Some(0.));
        assert_eq!(ray_box([2., 0., 0.], [1., 0., 0.], box_m), None);
        assert_eq!(ray_box([-2., 0., 0.], [1., 0., 0.], box_m), Some(1.));
        assert_eq!(
            ray_sphere([0.; 3], [1., 0., 0.], [10., 0., 0.], 2.),
            Some(8.)
        );
        assert_eq!(ray_sphere([0.; 3], [1., 0., 0.], [10., 3., 0.], 2.), None);
        assert_eq!(ray_box([0.; 3], [f64::NAN, 0., 0.], box_m), Some(0.));
        assert_eq!(
            ray_sphere([0.; 3], [1., 0., 0.], [1e308, 0., 0.], 1.),
            Some(0.)
        );
        assert_eq!(
            ray_box(
                [1e308, 0., 0.],
                [1., 0., 0.],
                [[-1e308, -1., -1.], [-1e307, 1., 1.]]
            ),
            Some(0.)
        );
    }
}
#[cfg(test)]
mod pose_tests {
    use super::*;
    #[test]
    fn shared_shortest_swing_is_neutral_before_any_drop() {
        for yaw in [-25_f64, -10., 0., 10., 25.] {
            for pitch in [0_f64, 10., 20.] {
                let a = TurretAim {
                    yaw_rad: yaw.to_radians(),
                    pitch_rad: pitch.to_radians(),
                    ..Default::default()
                };
                for f in [0., 0.15, 0.5, 0.75] {
                    assert_eq!(a.orientation_xyzw(f), Some([0., 0., 0., 1.]));
                }
                let [x, y, z, w] = a.orientation_xyzw(1.).unwrap();
                let forward = [
                    1. - 2. * (y * y + z * z),
                    2. * (x * y + w * z),
                    2. * (x * z - w * y),
                ];
                for (a, b) in forward.into_iter().zip(a.forward()) {
                    assert!((a - b).abs() < 1e-12);
                }
                let q = a.orientation_xyzw(0.875).unwrap();
                assert!((q.iter().map(|v| v * v).sum::<f64>() - 1.).abs() < 1e-12);
            }
        }
    }
}

#[cfg(test)]
mod request_clock_tests {
    use super::*;
    #[test]
    fn new_intent_never_spends_time_from_before_its_request() {
        let mut a = TurretAim::default();
        a.request(7, 10_000).unwrap();
        let target = direction(20_f64.to_radians(), 10_f64.to_radians());
        a.advance(target, 10_000).unwrap();
        assert_eq!((a.yaw_rad, a.pitch_rad), (0., 0.));
        a.advance(target, 10_100).unwrap();
        assert!(a.yaw_rad > 0. && a.yaw_rad <= 3_f64.to_radians() + 1e-12);
        let pose = (a.yaw_rad, a.pitch_rad);
        a.request(8, 20_000).unwrap();
        a.advance(target, 20_000).unwrap();
        assert_eq!((a.yaw_rad, a.pitch_rad), pose);
    }
}
