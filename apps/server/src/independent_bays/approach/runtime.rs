//! Exterior entry remains a separate retained operation stage. It can complete
//! only at the exact registered body+hardware entry; it never captures inventory.
use super::super::motion::{IndependentBayMotionProgress, IndependentBayMotionStart};
use super::*;
use arkline_protocol::drone_bay_geometry::BayMotionSegment;
use arkline_protocol::independent_bay_geometry::{self, IndependentBayDefinition};
use arkline_protocol::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayEntryRuntime {
    pub binding: IndependentBayBinding,
    pub operation_id: u64,
    pub drone_id: u64,
    pub bay_id: u8,
    pub action: IndependentBayAction,
    pub parent: BayPose,
    pub system_id: u32,
    pub class: DroneClass,
    pub original_body_pose: BayPose,
    #[serde(default)]
    pub static_obstacles: Vec<[[f64; 3]; 2]>,
    pub frames: Vec<IndependentBayKeyframe>,
    pub target_index: usize,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub confirmed: BayPose,
    pub confirmed_at_ms: u64,
    pub held: bool,
    pub reason: String,
    pub ready_to_handoff: bool,
}
fn duration(a: BayPose, b: BayPose, class: DroneClass) -> Result<u64> {
    let milliseconds =
        (distance(a.position_m, b.position_m) * 1.5 / drone_speed(class).min(40.) * 1000.).ceil();
    ensure!(
        milliseconds.is_finite() && milliseconds >= 0. && milliseconds < u64::MAX as f64,
        "Independent exterior entry duration overflow"
    );
    // The existing constrained-motion minimum also covers stationary alignment.
    Ok((milliseconds as u64).max(750))
}
fn plan(
    start: &IndependentBayMotionStart,
    definition: &IndependentBayDefinition,
    canonical: &[IndependentBayKeyframe],
    obstacles: &[[[f64; 3]; 2]],
) -> Result<Vec<IndependentBayKeyframe>> {
    ensure!(
        start.action != IndependentBayAction::Launch,
        "Stored launch cannot invent an exterior entry"
    );
    ensure!(
        start.binding.registration == definition.registration
            && start.binding.well_formed()
            && start.parent.valid()
            && start.operation_id > 0
            && start.drone_id > 0
            && start.system_id > 0,
        "Invalid independent entry identity"
    );
    let bay = definition
        .bays
        .iter()
        .find(|bay| bay.bay_id == start.bay_id)
        .context("Unknown independent entry bay")?;
    ensure!(
        bay.compatible_classes.contains(&start.class),
        "Independent entry class mismatch"
    );
    ensure!(
        !matches!(start.action, IndependentBayAction::ReturnToUnload { .. })
            || bay.ore_transfer_capable,
        "Independent bay has no ore mount"
    );
    ensure!(
        canonical
            .first()
            .is_some_and(|frame| frame.mechanism_poses == start.actual_mechanism_poses),
        "Entry hardware must already match source; no reset snap"
    );
    ensure!(
        canonical
            .first()
            .is_some_and(|frame| frame.drone_pose != start.actual_body_pose),
        "Drone is already at source entry; use handling executor"
    );
    let full = independent_entry_route_with_obstacles(
        start.actual_body_pose,
        bay,
        std::array::from_fn(|side| {
            std::array::from_fn(|axis| {
                if side == 0 {
                    definition.carrier_bounds_m[0][axis]
                        .min(definition.deployed_hardware_bounds_m[0][axis])
                } else {
                    definition.carrier_bounds_m[1][axis]
                        .max(definition.deployed_hardware_bounds_m[1][axis])
                }
            })
        }),
        canonical,
        obstacles,
    )?;
    let prefix = full
        .len()
        .checked_sub(canonical.len())
        .and_then(|n| n.checked_add(1))
        .context("Independent entry prefix overflow")?;
    ensure!(
        prefix >= 2,
        "Drone is already at source entry; use handling executor"
    );
    Ok(full[..prefix].to_vec())
}
impl IndependentBayEntryRuntime {
    pub fn begin(start: IndependentBayMotionStart) -> Result<Self> {
        let definition = independent_bay_geometry::lookup(&start.binding.registration)
            .context("Independent bay source is not registered")?;
        let canonical = definition
            .route(start.bay_id, start.action)
            .context("Independent source route unavailable")?;
        Self::from_definition(start, definition, &canonical)
    }
    pub(crate) fn from_definition(
        start: IndependentBayMotionStart,
        definition: &IndependentBayDefinition,
        canonical: &[IndependentBayKeyframe],
    ) -> Result<Self> {
        Self::from_definition_with_obstacles(start, definition, canonical, vec![])
    }
    pub(crate) fn from_definition_with_obstacles(
        start: IndependentBayMotionStart,
        definition: &IndependentBayDefinition,
        canonical: &[IndependentBayKeyframe],
        static_obstacles: Vec<[[f64; 3]; 2]>,
    ) -> Result<Self> {
        let frames = plan(&start, definition, canonical, &static_obstacles)?;
        let duration_ms = duration(frames[0].drone_pose, frames[1].drone_pose, start.class)?;
        ensure!(
            start.now.checked_add(duration_ms).is_some(),
            "Independent entry clock overflow"
        );
        Ok(Self {
            binding: start.binding,
            operation_id: start.operation_id,
            drone_id: start.drone_id,
            bay_id: start.bay_id,
            action: start.action,
            parent: start.parent,
            system_id: start.system_id,
            class: start.class,
            original_body_pose: start.actual_body_pose,
            static_obstacles,
            frames,
            target_index: 1,
            started_at_ms: start.now,
            duration_ms,
            confirmed: start.actual_body_pose,
            confirmed_at_ms: start.now,
            held: false,
            reason: String::new(),
            ready_to_handoff: false,
        })
    }
    fn segment(&self) -> Result<BayMotionSegment> {
        ensure!(
            (1..self.frames.len()).contains(&self.target_index),
            "Invalid independent entry cursor"
        );
        Ok(BayMotionSegment {
            source: self.frames[self.target_index - 1].drone_pose,
            target: self.frames[self.target_index].drone_pose,
            started_at_ms: self.started_at_ms,
            duration_ms: self.duration_ms,
        })
    }
    pub fn validate_registered(&self, actual_parent: BayPose, actual_system: u32) -> Result<()> {
        let definition = independent_bay_geometry::lookup(&self.binding.registration)
            .context("Independent bay source is not registered")?;
        let canonical = definition
            .route(self.bay_id, self.action)
            .context("Independent source route unavailable")?;
        self.validate_definition(definition, &canonical, actual_parent, actual_system)
    }
    pub(crate) fn validate_definition(
        &self,
        definition: &IndependentBayDefinition,
        canonical: &[IndependentBayKeyframe],
        actual_parent: BayPose,
        actual_system: u32,
    ) -> Result<()> {
        ensure!(
            !self.ready_to_handoff
                && self.parent == actual_parent
                && self.system_id == actual_system,
            "Independent entry parent/system or finalization changed"
        );
        let start = IndependentBayMotionStart {
            binding: self.binding.clone(),
            operation_id: self.operation_id,
            drone_id: self.drone_id,
            bay_id: self.bay_id,
            action: self.action,
            parent: self.parent,
            system_id: self.system_id,
            class: self.class,
            actual_body_pose: self.original_body_pose,
            actual_mechanism_poses: canonical
                .first()
                .context("Missing independent entry")?
                .mechanism_poses
                .clone(),
            now: 0,
        };
        ensure!(
            self.frames == plan(&start, definition, canonical, &self.static_obstacles)?,
            "Independent exterior path differs from exact source/envelope"
        );
        let segment = self.segment()?;
        ensure!(
            self.duration_ms == duration(segment.source, segment.target, self.class)?
                && self.started_at_ms <= self.confirmed_at_ms
                && self
                    .started_at_ms
                    .checked_add(self.duration_ms)
                    .is_some_and(|end| self.confirmed_at_ms <= end)
                && segment
                    .sample(self.confirmed_at_ms)
                    .is_some_and(|(pose, _)| pose == self.confirmed),
            "Invalid independent entry accepted clock/pose"
        );
        ensure!(self.reason.len() <= 512, "Invalid independent entry reason");
        Ok(())
    }
    pub fn hold(&mut self, reason: &str) -> Result<()> {
        ensure!(
            reason.len() <= 512,
            "Independent entry hold reason too long"
        );
        self.held = true;
        self.reason = reason.into();
        Ok(())
    }
    pub fn resume(&mut self, now: u64, actual_parent: BayPose, actual_system: u32) -> Result<bool> {
        self.validate_registered(actual_parent, actual_system)?;
        self.resume_validated(now)
    }
    pub(crate) fn resume_validated(&mut self, now: u64) -> Result<bool> {
        if !self.held {
            return Ok(false);
        }
        ensure!(
            now >= self.confirmed_at_ms,
            "Independent entry clock moved backwards"
        );
        let shifted = self
            .started_at_ms
            .checked_add(now - self.confirmed_at_ms)
            .context("Independent entry resume overflow")?;
        ensure!(
            shifted.checked_add(self.duration_ms).is_some(),
            "Independent entry deadline overflow"
        );
        self.started_at_ms = shifted;
        self.confirmed_at_ms = now;
        self.held = false;
        self.reason.clear();
        Ok(true)
    }
    pub fn advance(
        &mut self,
        now: u64,
        clear: bool,
        actual_parent: BayPose,
        actual_system: u32,
    ) -> Result<IndependentBayMotionProgress> {
        ensure!(
            !self.ready_to_handoff && now >= self.confirmed_at_ms,
            "Independent entry finalization/clock invalid"
        );
        if self.held {
            return Ok(IndependentBayMotionProgress::Held);
        }
        if self.parent != actual_parent
            || self.system_id != actual_system
            || now - self.confirmed_at_ms > 2000
            || !clear
        {
            self.hold("Exterior entry blocked, stale, or carrier changed")?;
            return Ok(IndependentBayMotionProgress::Held);
        }
        let segment = self.segment()?;
        self.confirmed = segment
            .sample(now)
            .context("Invalid independent entry sample")?
            .0;
        self.confirmed_at_ms = now;
        if now < segment.started_at_ms + segment.duration_ms {
            return Ok(IndependentBayMotionProgress::Moving);
        }
        if self.target_index + 1 == self.frames.len() {
            self.ready_to_handoff = true;
            return Ok(IndependentBayMotionProgress::FinalizationRequired);
        }
        let index = self.target_index;
        self.target_index += 1;
        self.started_at_ms = now;
        self.duration_ms = duration(
            self.frames[index].drone_pose,
            self.frames[self.target_index].drone_pose,
            self.class,
        )?;
        ensure!(
            now.checked_add(self.duration_ms).is_some(),
            "Independent entry next clock overflow"
        );
        Ok(IndependentBayMotionProgress::Milestone { index })
    }
    pub fn view(&self) -> Result<IndependentBayMotionView> {
        let segment = self.segment()?;
        let moving = !self.held && !self.ready_to_handoff;
        let drone = IndependentBayPoseView {
            confirmed: self.confirmed,
            segment: moving.then_some(segment),
        };
        let mechanisms = self.frames[0]
            .mechanism_poses
            .iter()
            .map(|(name, pose)| IndependentBayMechanismView {
                semantic_id: name.clone(),
                pose: IndependentBayPoseView {
                    confirmed: *pose,
                    segment: moving.then_some(BayMotionSegment {
                        source: *pose,
                        target: *pose,
                        ..segment
                    }),
                },
            })
            .collect();
        let view = IndependentBayMotionView {
            operation_id: self.operation_id,
            drone_id: self.drone_id,
            bay_id: self.bay_id,
            phase: if self.held {
                IndependentBayPhase::Held
            } else {
                self.frames[self.target_index].phase
            },
            parent: self.parent,
            drone,
            mechanisms,
            confirmed_at_ms: self.confirmed_at_ms,
            cleanup_only: false,
        };
        ensure!(view.valid_shape(), "Invalid independent entry projection");
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        IndependentBayMotionStart,
        IndependentBayDefinition,
        Vec<IndependentBayKeyframe>,
    ) {
        let (bay, canonical) = super::super::tests::fixture();
        let registration = IndependentBayRegistration {
            schema_version: INDEPENDENT_BAY_SCHEMA_VERSION,
            host: arkline_protocol::drone_bay_definition::PIONEER_BAY_REG12.registration(),
            assembly_revision: 1,
            assembly_source_sha256: "1".repeat(64),
        };
        let definition = IndependentBayDefinition {
            registration: registration.clone(),
            bays: vec![bay],
            carrier_bounds_m: [[-5.; 3], [5.; 3]],
            deployed_hardware_bounds_m: [[-5.; 3], [5.; 3]],
        };
        let start = IndependentBayMotionStart {
            binding: registration.bind("fixture-vessel".into()),
            operation_id: 1,
            drone_id: 6,
            bay_id: 5,
            action: IndependentBayAction::ReturnToStorage,
            parent: BayPose {
                position_m: [0.; 3],
                orientation_xyzw: [0., 0., 0., 1.],
            },
            system_id: 1,
            class: DroneClass::Mining,
            actual_body_pose: BayPose {
                position_m: [-20., 0., 0.],
                orientation_xyzw: [0., 1., 0., 0.],
            },
            actual_mechanism_poses: canonical[0].mechanism_poses.clone(),
            now: 0,
        };
        (start, definition, canonical)
    }
    #[test]
    fn real_pose_moves_around_hull_then_rotates_at_entry_without_snap() {
        let (start, definition, canonical) = fixture();
        let parent = start.parent;
        let initial = start.actual_body_pose;
        let mut run =
            IndependentBayEntryRuntime::from_definition(start, &definition, &canonical).unwrap();
        assert_eq!(run.confirmed, initial);
        let mut last = run.confirmed;
        let mut ready = false;
        for now in (100..=30000).step_by(100) {
            let result = run.advance(now, true, parent, 1).unwrap();
            assert!(distance(last.position_m, run.confirmed.position_m) <= 4.00001);
            assert!(run.view().unwrap().valid_shape());
            if result == IndependentBayMotionProgress::FinalizationRequired {
                ready = true;
                break;
            }
            run.validate_definition(&definition, &canonical, parent, 1)
                .unwrap();
            last = run.confirmed;
        }
        assert!(ready);
        assert_eq!(run.confirmed, canonical[0].drone_pose);
        assert_eq!(run.drone_id, 6);
        assert!(run
            .validate_definition(&definition, &canonical, parent, 1)
            .is_err());
    }
    #[test]
    fn held_alignment_resumes_same_pose_and_cannot_retime_without_hold() {
        let (mut start, definition, canonical) = fixture();
        start.actual_body_pose.position_m = canonical[0].drone_pose.position_m;
        let parent = start.parent;
        let mut run =
            IndependentBayEntryRuntime::from_definition(start, &definition, &canonical).unwrap();
        // First zero-distance approach step precedes the explicit alignment.
        for now in (100..=1000).step_by(100) {
            run.advance(now, true, parent, 1).unwrap();
        }
        let held = run.confirmed;
        run.hold("Disconnected during alignment").unwrap();
        run.resume_validated(100000).unwrap();
        assert_eq!(run.confirmed, held);
        let clock = (run.started_at_ms, run.duration_ms);
        assert!(!run.resume_validated(100001).unwrap());
        assert_eq!((run.started_at_ms, run.duration_ms), clock);
        run.validate_definition(&definition, &canonical, parent, 1)
            .unwrap();
        assert!(run.view().unwrap().valid_shape());
        assert_eq!(
            run.advance(100100, false, parent, 1).unwrap(),
            IndependentBayMotionProgress::Held
        );
        assert_eq!(run.confirmed, held);
    }
}
