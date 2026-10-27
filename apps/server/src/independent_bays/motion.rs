//! Source-bound semantic lane executor. This advances accepted poses, not stock,
//! economic balances or lifecycle completion. The enclosing transaction resolves
//! combat before consuming terminal milestones and committing capture/release.
use anyhow::{ensure, Context, Result};
use arkline_protocol::drone_bay_geometry::{BayMotionSegment, BayPose};
use arkline_protocol::independent_bay_geometry::{
    self, IndependentBayDefinition, IndependentBayKeyframe, IndependentBayMilestones,
};
use arkline_protocol::*;
use serde::{Deserialize, Serialize};

/// Existing handling translation cap, not an invented faster gantry.
const HANDLING_SPEED_MPS: f64 = 2.;
/// Existing exterior constrained-flight cap; class speed can be lower.
const EXTERIOR_SPEED_MPS: f64 = 40.;
const MIN_SEGMENT_MS: u64 = 750;
const MAX_ADVANCE_GAP_MS: u64 = 2000;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayFrame {
    pub drone_pose: BayPose,
    pub mechanisms: Vec<(String, BayPose)>,
}
impl From<&IndependentBayKeyframe> for IndependentBayFrame {
    fn from(frame: &IndependentBayKeyframe) -> Self {
        Self {
            drone_pose: frame.drone_pose,
            mechanisms: frame.mechanism_poses.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndependentBayRuntime {
    pub binding: IndependentBayBinding,
    pub operation_id: u64,
    pub drone_id: u64,
    pub bay_id: u8,
    /// A loaded storage command may first execute ReturnToUnload, retaining its
    /// physical command ID, then begin the separate checked storage route.
    pub route_action: IndependentBayAction,
    pub parent: BayPose,
    pub system_id: u32,
    pub class: DroneClass,
    pub route: Vec<IndependentBayKeyframe>,
    pub target_index: usize,
    pub segment_source: IndependentBayFrame,
    pub confirmed: IndependentBayFrame,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub confirmed_at_ms: u64,
    pub held: bool,
    pub hold_reason: String,
    pub waiting_for_transfer: bool,
    pub reached: IndependentBayMilestones,
    /// Must be finalized after combat in the same transaction, never persisted.
    pub ready_to_finalize: bool,
}
#[derive(Clone, Debug)]
pub struct IndependentBayMotionStart {
    pub binding: IndependentBayBinding,
    pub operation_id: u64,
    pub drone_id: u64,
    pub bay_id: u8,
    pub action: IndependentBayAction,
    pub parent: BayPose,
    pub system_id: u32,
    pub class: DroneClass,
    pub actual_body_pose: BayPose,
    pub actual_mechanism_poses: Vec<(String, BayPose)>,
    pub now: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndependentBayMotionProgress {
    Held,
    Moving,
    TransferBarrier,
    Milestone { index: usize },
    FinalizationRequired,
}
fn same_attitude(a: BayPose, b: BayPose) -> bool {
    a.orientation_xyzw == b.orientation_xyzw || a.orientation_xyzw == b.orientation_xyzw.map(|v| -v)
}
fn distance(a: BayPose, b: BayPose) -> f64 {
    (a.position_m[0] - b.position_m[0])
        .hypot(a.position_m[1] - b.position_m[1])
        .hypot(a.position_m[2] - b.position_m[2])
}
fn segment(
    source: BayPose,
    target: BayPose,
    started_at_ms: u64,
    duration_ms: u64,
) -> BayMotionSegment {
    BayMotionSegment {
        source,
        target,
        started_at_ms,
        duration_ms,
    }
}
fn frame_valid(frame: &IndependentBayFrame) -> bool {
    let mut names = std::collections::BTreeSet::new();
    frame.drone_pose.valid()
        && !frame.mechanisms.is_empty()
        && frame
            .mechanisms
            .iter()
            .all(|(name, pose)| !name.is_empty() && names.insert(name) && pose.valid())
}
fn duration(
    source: &IndependentBayFrame,
    target: &IndependentBayFrame,
    class: DroneClass,
) -> Result<u64> {
    ensure!(
        frame_valid(source)
            && frame_valid(target)
            && source.mechanisms.len() == target.mechanisms.len(),
        "Invalid independent bay motion frame"
    );
    ensure!(
        same_attitude(source.drone_pose, target.drone_pose),
        "Unproved rotating independent drone route"
    );
    let mut seconds = distance(source.drone_pose, target.drone_pose) * 1.5
        / drone_speed(class).min(EXTERIOR_SPEED_MPS);
    for ((id, a), (other, b)) in source.mechanisms.iter().zip(&target.mechanisms) {
        ensure!(
            id == other && same_attitude(*a, *b),
            "Independent mechanism identity/attitude changed"
        );
        seconds = seconds.max(distance(*a, *b) * 1.5 / HANDLING_SPEED_MPS);
    }
    let millis = (seconds * 1000.).ceil();
    ensure!(
        millis.is_finite() && millis >= 0. && millis < u64::MAX as f64,
        "Independent bay duration overflow"
    );
    Ok((millis as u64).max(MIN_SEGMENT_MS))
}
fn on_original_segment(
    frame: &IndependentBayFrame,
    source: &IndependentBayFrame,
    target: &IndependentBayFrame,
) -> bool {
    if frame.mechanisms.len() != source.mechanisms.len()
        || source.mechanisms.len() != target.mechanisms.len()
    {
        return false;
    }
    let poses = std::iter::once((frame.drone_pose, source.drone_pose, target.drone_pose))
        .chain(
            frame
                .mechanisms
                .iter()
                .zip(&source.mechanisms)
                .zip(&target.mechanisms)
                .map(|(((id, a), (sid, b)), (tid, c))| {
                    if id != sid || id != tid {
                        return (
                            *a,
                            BayPose {
                                position_m: [f64::NAN; 3],
                                ..*b
                            },
                            *c,
                        );
                    }
                    (*a, *b, *c)
                }),
        )
        .collect::<Vec<_>>();
    let mut selected = (0., 0.);
    for (a, b, c) in &poses {
        if !a.valid()
            || !b.valid()
            || !c.valid()
            || !same_attitude(*a, *b)
            || !same_attitude(*b, *c)
        {
            return false;
        }
        for i in 0..3 {
            let delta = c.position_m[i] - b.position_m[i];
            if delta.abs() > selected.0 {
                selected = (delta.abs(), (a.position_m[i] - b.position_m[i]) / delta);
            }
        }
    }
    let t = selected.1;
    if !t.is_finite() || !(-1e-10..=1. + 1e-10).contains(&t) {
        return false;
    }
    poses.iter().all(|(a, b, c)| {
        (0..3).all(|i| {
            (a.position_m[i] - (b.position_m[i] + (c.position_m[i] - b.position_m[i]) * t)).abs()
                <= 1e-8
        })
    })
}

impl IndependentBayRuntime {
    pub fn begin(start: IndependentBayMotionStart) -> Result<Self> {
        let definition = independent_bay_geometry::lookup(&start.binding.registration)
            .context("Independent bay source is not registered")?;
        let route = definition
            .route(start.bay_id, start.action)
            .context("No accepted independent bay route")?;
        Self::from_route(start, definition, route)
    }
    pub(super) fn from_route(
        start: IndependentBayMotionStart,
        definition: &IndependentBayDefinition,
        route: Vec<IndependentBayKeyframe>,
    ) -> Result<Self> {
        ensure!(
            start.binding.registration == definition.registration
                && start.binding.well_formed()
                && start.operation_id > 0
                && start.drone_id > 0
                && start.system_id > 0
                && start.parent.valid(),
            "Invalid independent bay motion binding"
        );
        let bay = definition
            .bays
            .iter()
            .find(|bay| bay.bay_id == start.bay_id)
            .context("Unknown registered independent bay")?;
        ensure!(
            bay.compatible_classes.contains(&start.class),
            "Drone class is incompatible with bay"
        );
        ensure!(
            !matches!(start.action, IndependentBayAction::ReturnToUnload { .. })
                || bay.ore_transfer_capable,
            "This bay has no working ore mount"
        );
        ensure!(
            route.len() >= 2
                && route.iter().all(|frame| !frame.phase.terminal()
                    && frame.phase != IndependentBayPhase::Held
                    && frame_valid(&IndependentBayFrame::from(frame))),
            "Invalid registered independent route"
        );
        for frame in &route {
            ensure!(
                frame.mechanism_poses.len() == bay.mechanisms.len()
                    && frame
                        .mechanism_poses
                        .iter()
                        .zip(&bay.mechanisms)
                        .all(|((name, _), mechanism)| name == &mechanism.semantic_id),
                "Registered route mechanism order differs from definition"
            );
            ensure!(
                !frame.milestones.clamped || frame.milestones.supported,
                "Clamped milestone without support"
            );
        }
        ensure!(
            start.actual_body_pose == route[0].drone_pose,
            "Actual drone is not at accepted route entry; approach must be continuous"
        );
        ensure!(
            start.actual_mechanism_poses == route[0].mechanism_poses,
            "Actual hardware is not at accepted route entry; reset must be continuous"
        );
        let final_frame = route.last().unwrap();
        ensure!(
            final_frame.milestones.hardware_safe,
            "Route lacks safe hardware endpoint"
        );
        match start.action {
            IndependentBayAction::Launch | IndependentBayAction::ReturnToUnload { .. } => ensure!(
                final_frame.milestones.exterior_clear,
                "Route lacks exterior-clear endpoint"
            ),
            IndependentBayAction::ReturnToStorage => ensure!(
                final_frame.milestones.supported
                    && final_frame.milestones.clamped
                    && final_frame.drone_pose == bay.stored_pose,
                "Route lacks supported clamped storage endpoint"
            ),
        }
        for pair in route.windows(2) {
            duration(
                &IndependentBayFrame::from(&pair[0]),
                &IndependentBayFrame::from(&pair[1]),
                start.class,
            )?;
        }
        let confirmed = IndependentBayFrame::from(&route[0]);
        let duration_ms = duration(
            &confirmed,
            &IndependentBayFrame::from(&route[1]),
            start.class,
        )?;
        ensure!(
            start.now.checked_add(duration_ms).is_some(),
            "Independent segment clock overflow"
        );
        Ok(Self {
            binding: start.binding,
            operation_id: start.operation_id,
            drone_id: start.drone_id,
            bay_id: start.bay_id,
            route_action: start.action,
            parent: start.parent,
            system_id: start.system_id,
            class: start.class,
            reached: route[0].milestones,
            route,
            target_index: 1,
            segment_source: confirmed.clone(),
            confirmed,
            started_at_ms: start.now,
            duration_ms,
            confirmed_at_ms: start.now,
            held: false,
            hold_reason: String::new(),
            waiting_for_transfer: false,
            ready_to_finalize: false,
        })
    }
    pub fn validate_registered(&self, actual_parent: BayPose, actual_system: u32) -> Result<()> {
        let definition = independent_bay_geometry::lookup(&self.binding.registration)
            .context("Independent bay source is not registered")?;
        let expected = definition
            .route(self.bay_id, self.route_action)
            .context("Registered route unavailable")?;
        self.validate_expected(&expected, actual_parent, actual_system)
    }
    pub(super) fn validate_expected(
        &self,
        expected: &[IndependentBayKeyframe],
        actual_parent: BayPose,
        actual_system: u32,
    ) -> Result<()> {
        ensure!(
            self.route == expected,
            "Persisted future independent route differs from exact source"
        );
        self.validate_shape(actual_parent, actual_system)
    }
    fn validate_shape(&self, actual_parent: BayPose, actual_system: u32) -> Result<()> {
        ensure!(
            self.parent == actual_parent && self.system_id == actual_system && self.parent.valid(),
            "Independent route parent/system changed"
        );
        ensure!(
            !self.ready_to_finalize
                && self.route.len() >= 2
                && (1..self.route.len()).contains(&self.target_index),
            "Invalid persisted independent route cursor"
        );
        ensure!(
            self.operation_id > 0
                && self.drone_id > 0
                && self.binding.well_formed()
                && frame_valid(&self.confirmed)
                && frame_valid(&self.segment_source),
            "Invalid independent runtime identity/frame"
        );
        ensure!(
            self.started_at_ms <= self.confirmed_at_ms
                && self.started_at_ms.checked_add(self.duration_ms).is_some()
                && self.duration_ms > 0,
            "Invalid independent runtime clock"
        );
        ensure!(
            self.hold_reason.len() <= 512,
            "Invalid independent hold reason"
        );
        ensure!(
            self.reached == self.route[self.target_index - 1].milestones,
            "Independent semantic milestone differs from accepted route cursor"
        );
        if !self.waiting_for_transfer {
            let target = IndependentBayFrame::from(&self.route[self.target_index]);
            ensure!(
                on_original_segment(
                    &self.segment_source,
                    &IndependentBayFrame::from(&self.route[self.target_index - 1]),
                    &target
                ),
                "Independent resumed source is outside original accepted segment"
            );
            ensure!(
                self.duration_ms == duration(&self.segment_source, &target, self.class)?
                    && self.confirmed_at_ms <= self.started_at_ms + self.duration_ms,
                "Independent duration or confirmed clock changed"
            );
        }
        if self.waiting_for_transfer {
            ensure!(
                matches!(
                    self.route_action,
                    IndependentBayAction::ReturnToUnload { .. }
                ) && self.reached.coupled
                    && self.confirmed
                        == IndependentBayFrame::from(&self.route[self.target_index - 1]),
                "Invalid transfer barrier"
            );
        } else {
            ensure!(
                self.sample(self.confirmed_at_ms)? == self.confirmed,
                "Confirmed independent pose differs from accepted clock"
            );
        }
        Ok(())
    }
    fn sample(&self, now: u64) -> Result<IndependentBayFrame> {
        let target = self
            .route
            .get(self.target_index)
            .context("Independent route target missing")?;
        let sample = |a, b| {
            segment(a, b, self.started_at_ms, self.duration_ms)
                .sample(now)
                .map(|(pose, _)| pose)
                .context("Invalid independent pose segment")
        };
        let drone_pose = sample(self.segment_source.drone_pose, target.drone_pose)?;
        ensure!(
            self.segment_source.mechanisms.len() == target.mechanism_poses.len(),
            "Independent mechanism count changed"
        );
        let mechanisms = self
            .segment_source
            .mechanisms
            .iter()
            .zip(&target.mechanism_poses)
            .map(|((name, a), (other, b))| {
                ensure!(name == other, "Independent mechanism identity changed");
                Ok((name.clone(), sample(*a, *b)?))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(IndependentBayFrame {
            drone_pose,
            mechanisms,
        })
    }
    pub fn hold(&mut self, reason: &str) -> Result<()> {
        ensure!(reason.len() <= 512, "Independent hold reason too long");
        self.held = true;
        self.hold_reason = reason.into();
        Ok(())
    }
    pub fn resume(&mut self, now: u64, actual_parent: BayPose, actual_system: u32) -> Result<bool> {
        self.validate_registered(actual_parent, actual_system)?;
        if !self.held {
            return Ok(false);
        }
        self.resume_validated(now)
    }
    pub(super) fn resume_validated(&mut self, now: u64) -> Result<bool> {
        ensure!(
            now >= self.confirmed_at_ms,
            "Independent resume clock moved backwards"
        );
        if !self.held {
            return Ok(false);
        }
        if !self.waiting_for_transfer {
            self.start_remaining(now)?;
        } else {
            // Resume a stationary coupled wait without an immediate stale-clock
            // hold. Economic attempt/cooldown is owned elsewhere and unchanged.
            self.confirmed_at_ms = now;
        }
        self.held = false;
        self.hold_reason.clear();
        Ok(true)
    }
    fn start_remaining(&mut self, now: u64) -> Result<()> {
        let duration_ms = duration(
            &self.confirmed,
            &IndependentBayFrame::from(&self.route[self.target_index]),
            self.class,
        )?;
        ensure!(
            now.checked_add(duration_ms).is_some(),
            "Independent segment clock overflow"
        );
        self.segment_source = self.confirmed.clone();
        self.started_at_ms = now;
        self.duration_ms = duration_ms;
        self.confirmed_at_ms = now;
        Ok(())
    }
    /// Only after actual empty payload or explicit safe cancellation; the caller
    /// atomically closes the manifold attempt first. No inventory changes here.
    pub fn release_transfer_barrier(&mut self, now: u64) -> Result<()> {
        ensure!(
            self.waiting_for_transfer && !self.held,
            "Independent transfer barrier is not releasable"
        );
        ensure!(
            now >= self.confirmed_at_ms,
            "Independent clock moved backwards"
        );
        self.start_remaining(now)?;
        self.waiting_for_transfer = false;
        Ok(())
    }
    /// Dynamic admission checks the complete proposed interval before calling
    /// this. At most one semantic boundary advances; long gaps hold, never catch up.
    pub fn advance(
        &mut self,
        now: u64,
        clear: bool,
        actual_parent: BayPose,
        actual_system: u32,
    ) -> Result<IndependentBayMotionProgress> {
        ensure!(
            !self.ready_to_finalize,
            "Independent route awaiting transaction finalization"
        );
        ensure!(
            now >= self.confirmed_at_ms,
            "Independent clock moved backwards"
        );
        if self.held {
            return Ok(IndependentBayMotionProgress::Held);
        }
        if actual_parent != self.parent || actual_system != self.system_id {
            self.hold("Carrier pose/system changed")?;
            return Ok(IndependentBayMotionProgress::Held);
        }
        if now - self.confirmed_at_ms > MAX_ADVANCE_GAP_MS {
            self.hold("Simulation gap; resume required")?;
            return Ok(IndependentBayMotionProgress::Held);
        }
        if !clear {
            self.hold("Accepted lane is obstructed")?;
            return Ok(IndependentBayMotionProgress::Held);
        }
        if self.waiting_for_transfer {
            self.confirmed_at_ms = now;
            return Ok(IndependentBayMotionProgress::TransferBarrier);
        }
        self.confirmed = self.sample(now)?;
        self.confirmed_at_ms = now;
        if now < self.started_at_ms + self.duration_ms {
            return Ok(IndependentBayMotionProgress::Moving);
        }
        let index = self.target_index;
        self.reached = self.route[index].milestones;
        if index + 1 == self.route.len() {
            self.ready_to_finalize = true;
            return Ok(IndependentBayMotionProgress::FinalizationRequired);
        }
        self.target_index += 1;
        if self.reached.coupled
            && matches!(
                self.route_action,
                IndependentBayAction::ReturnToUnload { .. }
            )
        {
            self.waiting_for_transfer = true;
            return Ok(IndependentBayMotionProgress::TransferBarrier);
        }
        self.start_remaining(now)?;
        Ok(IndependentBayMotionProgress::Milestone { index })
    }
    pub fn view(
        &self,
        phase: IndependentBayPhase,
        cleanup_only: bool,
    ) -> Result<IndependentBayMotionView> {
        let target = self
            .route
            .get(self.target_index)
            .context("Independent target missing")?;
        ensure!(
            self.confirmed.mechanisms.len() == self.segment_source.mechanisms.len()
                && self.confirmed.mechanisms.len() == target.mechanism_poses.len(),
            "Independent mechanism projection count mismatch"
        );
        let moving = !self.held && !self.waiting_for_transfer && !self.ready_to_finalize;
        let pose = |confirmed, source, target| IndependentBayPoseView {
            confirmed,
            segment: moving.then_some(segment(
                source,
                target,
                self.started_at_ms,
                self.duration_ms,
            )),
        };
        let mechanisms = self
            .confirmed
            .mechanisms
            .iter()
            .zip(&self.segment_source.mechanisms)
            .zip(&target.mechanism_poses)
            .map(
                |(((name, confirmed), (source_name, source)), (target_name, target))| {
                    ensure!(
                        name == source_name && name == target_name,
                        "Independent mechanism projection identity mismatch"
                    );
                    Ok(IndependentBayMechanismView {
                        semantic_id: name.clone(),
                        pose: pose(*confirmed, *source, *target),
                    })
                },
            )
            .collect::<Result<Vec<_>>>()?;
        let view = IndependentBayMotionView {
            operation_id: self.operation_id,
            drone_id: self.drone_id,
            bay_id: self.bay_id,
            phase: if self.held {
                IndependentBayPhase::Held
            } else {
                phase
            },
            parent: self.parent,
            drone: pose(
                self.confirmed.drone_pose,
                self.segment_source.drone_pose,
                target.drone_pose,
            ),
            mechanisms,
            confirmed_at_ms: self.confirmed_at_ms,
            cleanup_only,
        };
        ensure!(view.valid_shape(), "Invalid independent motion projection");
        Ok(view)
    }
}

#[cfg(test)]
mod tests;
