//! Confirmed independent-lane rigid poses. Sampling presents accepted segments;
//! it never claims capture, cargo credit, clamp completion or source support.
use super::IndependentBayPhase;
use crate::drone_bay_geometry::{BayMotionSegment, BayPose};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayPoseView {
    pub confirmed: BayPose,
    pub segment: Option<BayMotionSegment>,
}
impl IndependentBayPoseView {
    pub fn sample(&self, now: u64) -> Option<BayPose> {
        if !self.confirmed.valid() {
            return None;
        }
        self.segment.map_or(Some(self.confirmed), |segment| {
            segment.sample(now).map(|(pose, _)| pose)
        })
    }
    fn valid_at(&self, at: u64) -> bool {
        if !self.confirmed.valid() {
            return false;
        }
        self.segment.is_none_or(|segment| {
            segment
                .started_at_ms
                .checked_add(segment.duration_ms)
                .is_some_and(|end| at >= segment.started_at_ms && at <= end)
                && segment
                    .sample(at)
                    .is_some_and(|(pose, _)| pose == self.confirmed)
        })
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayMechanismView {
    pub semantic_id: String,
    /// Resolved body-space root, not a local transform to inherit a second time.
    pub pose: IndependentBayPoseView,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayMotionView {
    pub operation_id: u64,
    pub drone_id: u64,
    pub bay_id: u8,
    pub phase: IndependentBayPhase,
    /// The actual stationary full carrier pose accepted for this operation.
    pub parent: BayPose,
    pub drone: IndependentBayPoseView,
    pub mechanisms: Vec<IndependentBayMechanismView>,
    pub confirmed_at_ms: u64,
    pub cleanup_only: bool,
}
impl IndependentBayMotionView {
    /// Clock/finite-shape validation only. Source-specific group names, original
    /// route frames, stock/fleet and vessel binding must be validated separately.
    pub fn valid_shape(&self) -> bool {
        if self.operation_id == 0
            || self.drone_id == 0
            || usize::from(self.bay_id) >= crate::MAX_DRONES
            || !self.parent.valid()
            || !self.drone.valid_at(self.confirmed_at_ms)
            || self.mechanisms.is_empty()
            || self.phase.terminal()
        {
            return false;
        }
        let mut names = std::collections::BTreeSet::new();
        let clock = self.drone.segment.map(|s| (s.started_at_ms, s.duration_ms));
        if self.phase == IndependentBayPhase::Held && clock.is_some() {
            return false;
        }
        self.mechanisms.iter().all(|group| {
            !group.semantic_id.is_empty()
                && group.semantic_id.len() <= 256
                && names.insert(&group.semantic_id)
                && group.pose.valid_at(self.confirmed_at_ms)
                && group.pose.segment.map(|s| (s.started_at_ms, s.duration_ms)) == clock
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> IndependentBayMotionView {
        let source = BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let target = BayPose {
            position_m: [5., 0., 0.],
            ..source
        };
        let segment = BayMotionSegment {
            source,
            target,
            started_at_ms: 100,
            duration_ms: 1000,
        };
        let pose = IndependentBayPoseView {
            confirmed: segment.sample(600).unwrap().0,
            segment: Some(segment),
        };
        IndependentBayMotionView {
            operation_id: 1,
            drone_id: 6,
            bay_id: 5,
            phase: IndependentBayPhase::Launching,
            parent: source,
            drone: pose.clone(),
            mechanisms: vec![IndependentBayMechanismView {
                semantic_id: "bay/5/cradle".into(),
                pose,
            }],
            confirmed_at_ms: 600,
            cleanup_only: false,
        }
    }
    #[test]
    fn drone_and_hardware_share_confirmed_clock_and_holds_cannot_animate() {
        let motion = fixture();
        assert!(motion.valid_shape());
        let mut bad = motion.clone();
        bad.mechanisms[0]
            .pose
            .segment
            .as_mut()
            .unwrap()
            .started_at_ms += 1;
        assert!(!bad.valid_shape());
        let mut held = motion;
        held.phase = IndependentBayPhase::Held;
        assert!(!held.valid_shape());
        held.drone.segment = None;
        held.mechanisms[0].pose.segment = None;
        assert!(held.valid_shape());
        assert_eq!(held.drone.sample(u64::MAX), Some(held.drone.confirmed));
    }
    #[test]
    fn wrong_confirmed_pose_duplicate_groups_and_terminal_motion_fail_closed() {
        let motion = fixture();
        let mut bad = motion.clone();
        bad.drone.confirmed.position_m[0] += 1.;
        assert!(!bad.valid_shape());
        let mut bad = motion.clone();
        bad.mechanisms.push(bad.mechanisms[0].clone());
        assert!(!bad.valid_shape());
        let mut bad = motion;
        bad.phase = IndependentBayPhase::Stored;
        assert!(!bad.valid_shape());
    }
}
