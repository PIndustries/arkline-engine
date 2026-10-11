//! Presentation gait driven by displacement and bounded frame deltas, never epoch floats.
use glam::Vec3;
#[derive(Default)]
pub(super) struct Gait {
    last_ms: Option<u64>,
    last_position: Option<Vec3>,
    phase: f32,
    weight: f32,
    direction: Vec3,
}
#[derive(Clone, Copy)]
pub(super) struct Pose {
    pub phase: f32,
    pub weight: f32,
    pub direction: Vec3,
}
impl Default for Pose {
    fn default() -> Self {
        Self {
            phase: 0.0,
            weight: 0.0,
            direction: Vec3::X,
        }
    }
}
impl Gait {
    pub fn update(
        &mut self,
        now: u64,
        position: Vec3,
        heading: f64,
        moving: bool,
        seated: bool,
    ) -> Pose {
        let dt = self
            .last_ms
            .map(|t| now.saturating_sub(t).min(100) as f32 / 1000.0)
            .unwrap_or(0.0);
        self.last_ms = Some(now);
        let delta = self
            .last_position
            .map(|p| position - p)
            .unwrap_or(Vec3::ZERO);
        self.last_position = Some(position);
        let speed = if dt > 0.0 {
            Vec3::new(delta.x, 0.0, delta.z).length() / dt
        } else {
            0.0
        };
        let active = moving && !seated && speed > 0.015 && speed < 20.0;
        let desired = if active {
            (speed / 4.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.weight += (desired - self.weight) * (1.0 - (-dt * 12.0).exp());
        if active {
            let (sin, cos) = (heading as f32).sin_cos();
            let local = Vec3::new(
                delta.x * cos + delta.z * sin,
                0.0,
                -delta.x * sin + delta.z * cos,
            )
            .normalize_or_zero();
            self.direction = self
                .direction
                .lerp(local, 1.0 - (-dt * 16.0).exp())
                .normalize_or_zero();
        }
        if self.direction.length_squared() < 0.1 {
            self.direction = Vec3::X;
        }
        if self.weight > 0.001 {
            self.phase = (self.phase + dt * (speed.min(4.0) / 1.6).max(0.75)).rem_euclid(1.0);
        } else {
            self.weight = 0.0;
        }
        if seated {
            self.weight = 0.0;
        }
        Pose {
            phase: self.phase,
            weight: self.weight,
            direction: self.direction,
        }
    }
}
impl Pose {
    /// A stance half-cycle travels backward on the floor; swing returns with foot lift.
    pub fn foot(self, side: f32) -> Vec3 {
        let t = (self.phase + if side > 0.0 { 0.5 } else { 0.0 }).rem_euclid(1.0);
        let (travel, lift) = if t < 0.5 {
            (1.0 - 4.0 * t, 0.0)
        } else {
            let u = (t - 0.5) * 2.0;
            (
                -1.0 + 2.0 * (u * u * (3.0 - 2.0 * u)),
                (u * std::f32::consts::PI).sin() * 0.16,
            )
        };
        Vec3::new(0.0, 0.075, side * 0.13)
            + self.direction * travel * 0.4 * self.weight
            + Vec3::Y * lift * self.weight
    }
    pub fn knee(self, side: f32) -> Vec3 {
        let foot = self.foot(side);
        Vec3::new(0.0, 0.94, side * 0.13).lerp(foot, 0.5)
            + Vec3::X * (0.04 + self.weight * 0.10)
            + Vec3::Y * (foot.y - 0.075) * 0.4
    }
    pub fn hand(self, side: f32) -> Vec3 {
        let foot = self.foot(side) - Vec3::new(0.0, 0.075, side * 0.13);
        Vec3::new(0.0, 0.87, side * 0.31) - Vec3::new(foot.x, 0.0, foot.z) * 0.6
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn epoch_size_does_not_change_gait() {
        let mut a = Gait::default();
        let mut b = Gait::default();
        for i in 0..200 {
            let p = Vec3::X * i as f32 * 0.04;
            let x = a.update(i * 10, p, 0.0, true, false);
            let y = b.update(1_795_000_000_000 + i * 10, p, 0.0, true, false);
            assert_eq!(x.phase, y.phase);
            assert_eq!(x.foot(-1.0), y.foot(-1.0));
        }
    }
    #[test]
    fn cycle_alternates_contact_and_lift_and_repeats() {
        let pose = Pose {
            phase: 0.25,
            weight: 1.0,
            direction: Vec3::X,
        };
        assert_eq!(pose.foot(-1.0).y, 0.075);
        assert!(pose.foot(1.0).y > 0.20);
        assert!(
            pose.foot(1.0).distance(
                Pose {
                    phase: 1.25,
                    ..pose
                }
                .foot(1.0)
            ) < 1e-6
        );
    }
    #[test]
    fn zero_displacement_turn_is_idle_and_stop_settles_restart_and_seat() {
        let mut g = Gait::default();
        for i in 0..100 {
            assert_eq!(
                g.update(i * 16, Vec3::ZERO, i as f64, true, false).weight,
                0.0
            );
        }
        let mut p = Vec3::ZERO;
        for i in 100..160 {
            p.x += 0.064;
            g.update(i * 16, p, 0.0, true, false);
        }
        assert!(g.weight > 0.9);
        let before = g.weight;
        g.update(160 * 16, p, 0.0, false, false);
        assert!(g.weight > 0.0 && g.weight < before);
        for i in 161..240 {
            g.update(i * 16, p, 0.0, false, false);
        }
        assert_eq!(g.weight, 0.0);
        p.z += 0.064;
        assert!(g.update(240 * 16, p, 0.0, true, false).weight > 0.0);
        assert_eq!(g.update(241 * 16, p, 0.0, false, true).weight, 0.0);
    }
}
