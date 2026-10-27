//! Physical mining-gun deployment, independent of cockpit monitor preferences.
//! The actuator is authoritative; clients may interpolate only this admitted path.
use crate::{CockpitScreenActuator, Destination};
use serde::{Deserialize, Serialize};

pub const TURRET_TRAVEL_MS: u64 = 2_400;
/// Above this fraction the carriage is fully raised while the barrel homes.
pub const TURRET_HOME_FRACTION: f64 = 0.75;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurretDeployment {
    /// Server-confirmed exact new-hull admission; legacy decks remain unavailable.
    #[serde(default)]
    pub source_admitted: bool,
    /// Absent in historical stow-only recordings; never infer firing authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aims: Option<[crate::parallel_turret_aim::TurretAim; 2]>,
    pub actuators: [CockpitScreenActuator; 2],
    /// Retained intent, not a warp reservation or proof of transit.
    pub pending_warp: Option<Destination>,
    pub status: String,
    pub updated_at_ms: u64,
}
impl Default for TurretDeployment {
    fn default() -> Self {
        // Legacy guns were physically raised. Never migrate them to a falsely
        // secured pose or debit energy merely by reading an old save.
        Self {
            source_admitted: false,
            aims: None,
            actuators: std::array::from_fn(|_| CockpitScreenActuator {
                from_fraction: 1.0,
                target_deployed: true,
                started_at_ms: 0,
                duration_ms: 0,
                blocked: None,
            }),
            pending_warp: None,
            status: "Deployed".into(),
            updated_at_ms: 0,
        }
    }
}
impl TurretDeployment {
    pub fn is_legacy_default(&self) -> bool {
        self == &Self::default()
    }

    /// Validate persisted numeric state before evaluating or authorizing motion.
    pub fn validate_at(&self, sample_ms: u64) -> Result<(), String> {
        if self.updated_at_ms > sample_ms || self.status.len() > 512 {
            return Err("invalid turret sample clock/status".into());
        }
        if let Some(aims) = &self.aims {
            for aim in aims {
                aim.validate_at(self.updated_at_ms)?;
            }
        }
        for a in &self.actuators {
            if !a.from_fraction.is_finite()
                || !(0. ..=1.).contains(&a.from_fraction)
                || a.duration_ms > TURRET_TRAVEL_MS
                || a.started_at_ms > self.updated_at_ms
                || a.started_at_ms.checked_add(a.duration_ms).is_none()
                || (a.blocked.is_none()
                    && a.duration_ms
                        != ((f64::from(a.target_deployed) - a.from_fraction).abs()
                            * TURRET_TRAVEL_MS as f64)
                            .ceil() as u64)
                || a.blocked.as_ref().is_some_and(|s| s.len() > 512)
            {
                return Err("invalid persisted turret actuator".into());
            }
        }
        Ok(())
    }
    pub fn fractions_at(&self, now: u64) -> [f64; 2] {
        self.actuators.each_ref().map(|a| a.fraction_at(now))
    }
    pub fn secure_at(&self, now: u64) -> bool {
        self.validate_at(now).is_ok()
            && self
                .actuators
                .iter()
                .all(|a| !a.target_deployed && a.blocked.is_none() && a.fraction_at(now) <= 1e-9)
    }
    pub fn operational_at(&self, now: u64) -> bool {
        self.validate_at(now).is_ok()
            && self.pending_warp.is_none()
            && self.actuators.iter().all(|a| {
                a.target_deployed && a.blocked.is_none() && a.fraction_at(now) >= 1.0 - 1e-9
            })
    }
    /// Idempotent repeated commands do not reset motor time. Explicit commands
    /// may resume a held path, but only after the authority rechecks its sweep.
    pub fn command(
        &mut self,
        deployed: bool,
        now: u64,
        mut obstructed: impl FnMut(usize, f64, f64) -> bool,
    ) {
        if now < self.updated_at_ms {
            return;
        }
        for (slot, a) in self.actuators.iter_mut().enumerate() {
            if a.target_deployed == deployed && a.blocked.is_none() {
                continue;
            }
            let current = a.fraction_at(now);
            let target = f64::from(deployed);
            a.from_fraction = current;
            a.target_deployed = deployed;
            a.started_at_ms = now;
            a.duration_ms = ((target - current).abs() * TURRET_TRAVEL_MS as f64).ceil() as u64;
            a.blocked = ((target - current).abs() > 1e-9 && obstructed(slot, current, target))
                .then(|| "Turret travel envelope occupied".into());
        }
        self.updated_at_ms = now;
        self.update_status(now);
    }
    /// Check the swept interval from the last persisted pose before admitting
    /// elapsed motion. A newly obstructed path holds at that last safe pose.
    pub fn advance(&mut self, now: u64, mut obstructed: impl FnMut(usize, f64, f64) -> bool) {
        if now < self.updated_at_ms {
            return;
        }
        for (slot, a) in self.actuators.iter_mut().enumerate() {
            let previous = a.fraction_at(self.updated_at_ms);
            let next = a.fraction_at(now);
            if a.blocked.is_none()
                && (previous - next).abs() > 1e-9
                && obstructed(slot, previous, next)
            {
                a.from_fraction = previous;
                a.started_at_ms = now;
                a.duration_ms = 0;
                a.blocked =
                    Some("Turret travel envelope occupied; command again after clearing".into());
            }
        }
        self.updated_at_ms = now;
        self.update_status(now);
    }
    pub fn hold(&mut self, now: u64, reason: &str) {
        if let Some(aims) = &mut self.aims {
            for aim in aims {
                aim.hold(now);
            }
        }
        for a in &mut self.actuators {
            if a.moving_at(self.updated_at_ms) {
                a.from_fraction = a.fraction_at(self.updated_at_ms);
                a.started_at_ms = now;
                a.duration_ms = 0;
                a.blocked = Some(reason.into());
            }
        }
        self.pending_warp = None;
        self.updated_at_ms = now;
        self.status = reason.into();
    }
    fn update_status(&mut self, now: u64) {
        self.status = if self.actuators.iter().any(|a| a.blocked.is_some()) {
            "Turret travel held; clear obstruction and command again"
        } else if self.pending_warp.is_some() {
            "Securing turrets for warp"
        } else if self.secure_at(now) {
            "Stowed and secured"
        } else if self.operational_at(now) {
            "Deployed"
        } else if self.actuators.iter().any(|a| a.target_deployed) {
            "Deploying"
        } else {
            "Stowing"
        }
        .into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_mechanism_json_preserves_deployed_pose_without_admitting_new_source() {
        let original = crate::PioneerCockpitMechanisms::stowed("owned".into(), 1000);
        let mut json = serde_json::to_value(original).unwrap();
        json.as_object_mut().unwrap().remove("turrets");
        let restored: crate::PioneerCockpitMechanisms = serde_json::from_value(json).unwrap();
        assert_eq!(restored.turrets.fractions_at(1000), [1.; 2]);
        assert!(!restored.turrets.source_admitted);
        assert!(restored.turrets.pending_warp.is_none());
    }
    #[test]
    fn repeated_stow_does_not_restart_travel_and_latch_requires_endpoint() {
        let mut s = TurretDeployment::default();
        s.command(false, 100, |_, _, _| false);
        s.command(false, 600, |_, _, _| false);
        assert_eq!(s.actuators[0].started_at_ms, 100);
        assert!(!s.secure_at(2499));
        s.advance(2500, |_, _, _| false);
        assert!(s.secure_at(2500));
    }
    #[test]
    fn obstruction_holds_last_safe_pose_and_requires_explicit_resume() {
        let mut s = TurretDeployment::default();
        s.command(false, 100, |_, _, _| false);
        s.advance(700, |_, _, _| false);
        s.advance(1300, |_, _, _| true);
        assert_eq!(s.fractions_at(9000), [0.75; 2]);
        assert!(!s.secure_at(9000));
        s.advance(9000, |_, _, _| false);
        assert_eq!(s.fractions_at(9000), [0.75; 2]);
        s.command(false, 9000, |_, _, _| false);
        s.advance(10800, |_, _, _| false);
        assert!(s.secure_at(10800));
    }
    #[test]
    fn restart_cancels_retained_warp_and_does_not_count_offline_motion() {
        let mut s = TurretDeployment {
            pending_warp: Some(Destination::Belt),
            ..Default::default()
        };
        s.command(false, 100, |_, _, _| false);
        s.advance(700, |_, _, _| false);
        s.hold(100000, "Server restarted");
        assert_eq!(s.pending_warp, None);
        assert_eq!(s.fractions_at(200000), [0.75; 2]);
        assert!(!s.secure_at(200000));
    }
    #[test]
    fn reversal_is_continuous_and_pending_warp_prevents_operation() {
        let mut s = TurretDeployment::default();
        s.command(false, 100, |_, _, _| false);
        s.command(true, 700, |_, _, _| false);
        assert_eq!(s.fractions_at(700), [0.75; 2]);
        assert!(s.operational_at(1300));
        s.pending_warp = Some(Destination::Belt);
        assert!(!s.operational_at(1300));
    }
}

#[cfg(test)]
mod numeric_tests {
    use super::*;
    #[test]
    fn malformed_persisted_numbers_never_authorize_secure_or_firing() {
        for bad in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
            let mut s = TurretDeployment::default();
            s.actuators[0].from_fraction = bad;
            assert!(s.validate_at(100).is_err());
            assert!(!s.secure_at(100));
            assert!(!s.operational_at(100));
        }
        let mut s = TurretDeployment::default();
        s.actuators[0].duration_ms = TURRET_TRAVEL_MS + 1;
        assert!(s.validate_at(100).is_err());
        s = TurretDeployment::default();
        s.actuators[0].target_deployed = false;
        assert!(
            s.validate_at(100).is_err(),
            "zero-duration partial pose cannot become secure"
        );
        s = TurretDeployment::default();
        s.updated_at_ms = 101;
        assert!(s.validate_at(100).is_err());
        s = TurretDeployment::default();
        s.actuators[0].started_at_ms = 1;
        assert!(s.validate_at(100).is_err());
    }
    #[test]
    fn old_default_is_raised_and_unadmitted_and_stow_is_not_timer_only_secure() {
        let mut s = TurretDeployment::default();
        assert!(!s.source_admitted);
        assert!(s.operational_at(0));
        s.command(false, 100, |_, _, _| true);
        assert!(!s.secure_at(100_000));
        s.command(false, 200, |_, _, _| false);
        assert!(!s.secure_at(201));
        assert!(s.secure_at(2600));
        s.hold(500, "stop");
        assert!(!s.secure_at(100_000));
        assert!(s.pending_warp.is_none());
    }
}
