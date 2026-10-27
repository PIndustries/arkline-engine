//! One authoritative clock for the drone, portable cassettes and owned hardware.
//! This executor samples accepted source routes; it never creates stock or ore.
use anyhow::{ensure, Context, Result};
use arkline_protocol::carrier_inventory_geometry::{
    self as geometry, CarrierInventoryDefinition, CarrierInventoryKeyframe,
    CarrierInventoryMilestones, CarrierMechanismOwner,
};
use arkline_protocol::drone_bay_geometry::{BayMotionSegment, BayPose};
use arkline_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
const HANDLING_SPEED_MPS: f64 = 2.;
const EXTERIOR_SPEED_MPS: f64 = 40.;
const MIN_SEGMENT_MS: u64 = 750;
const MAX_ADVANCE_GAP_MS: u64 = 2000;

#[derive(Clone, Debug)]
pub struct CarrierMotionStart {
    pub binding: CarrierInventoryBinding,
    pub operation_id: u64,
    pub drone_id: u64,
    pub class: DroneClass,
    pub kind: CarrierRouteKind,
    pub parent: BayPose,
    pub system_id: u32,
    pub actual: CarrierMotionFrame,
    pub now: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarrierMotionRuntime {
    pub binding: CarrierInventoryBinding,
    pub operation_id: u64,
    pub drone_id: u64,
    pub class: DroneClass,
    pub kind: CarrierRouteKind,
    pub parent: BayPose,
    pub system_id: u32,
    pub route: Vec<CarrierInventoryKeyframe>,
    pub target_index: usize,
    pub source: CarrierMotionFrame,
    pub confirmed: CarrierMotionFrame,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub confirmed_at_ms: u64,
    pub held: bool,
    pub hold_reason: String,
    pub waiting_for_transfer: bool,
    pub reached: CarrierInventoryMilestones,
    pub completed_stage_legs: u8,
    /// Consumed after combat in this transaction; never valid persisted state.
    pub ready_to_finalize: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarrierMotionProgress {
    Held,
    Moving,
    TransferBarrier,
    Milestone { index: usize },
    FinalizationRequired,
}
fn attitude(a: BayPose, b: BayPose) -> bool {
    a.orientation_xyzw == b.orientation_xyzw || a.orientation_xyzw == b.orientation_xyzw.map(|x| -x)
}
fn frame_valid(frame: &CarrierMotionFrame) -> bool {
    let mut ids = BTreeSet::new();
    !frame.poses.is_empty()
        && frame.poses.iter().all(|(id, p)| {
            ids.insert(id) && p.valid() && p.position_m.iter().all(|v| v.abs() <= 1_000_000.)
        })
}
fn duration(a: &CarrierMotionFrame, b: &CarrierMotionFrame, class: DroneClass) -> Result<u64> {
    ensure!(
        frame_valid(a) && frame_valid(b) && a.poses.len() == b.poses.len(),
        "Invalid carrier motion frame"
    );
    let mut seconds = 0f64;
    for ((id, a), (other, b)) in a.poses.iter().zip(&b.poses) {
        ensure!(
            id == other && attitude(*a, *b),
            "Source motion changes identity or rotates unproved hardware"
        );
        let distance = (a.position_m[0] - b.position_m[0])
            .hypot(a.position_m[1] - b.position_m[1])
            .hypot(a.position_m[2] - b.position_m[2]);
        let speed = if *id == CarrierPoseKey::Drone {
            drone_speed(class).min(EXTERIOR_SPEED_MPS)
        } else {
            HANDLING_SPEED_MPS
        };
        seconds = seconds.max(1.5 * distance / speed);
    }
    let ms = (seconds * 1000.).ceil();
    ensure!(
        ms.is_finite() && ms >= 0. && ms < u64::MAX as f64,
        "Carrier route duration overflow"
    );
    Ok((ms as u64).max(MIN_SEGMENT_MS))
}
fn segment(a: BayPose, b: BayPose, start: u64, ms: u64) -> BayMotionSegment {
    BayMotionSegment {
        source: a,
        target: b,
        started_at_ms: start,
        duration_ms: ms,
    }
}
fn sample(
    a: &CarrierMotionFrame,
    b: &CarrierMotionFrame,
    start: u64,
    ms: u64,
    now: u64,
) -> Result<CarrierMotionFrame> {
    ensure!(
        a.poses.len() == b.poses.len(),
        "Motion identity count changed"
    );
    Ok(CarrierMotionFrame {
        poses: a
            .poses
            .iter()
            .zip(&b.poses)
            .map(|((id, a), (other, b))| {
                ensure!(id == other, "Motion identity changed");
                Ok((
                    id.clone(),
                    segment(*a, *b, start, ms)
                        .sample(now)
                        .context("Invalid source motion segment")?
                        .0,
                ))
            })
            .collect::<Result<_>>()?,
    })
}
fn on_segment(frame: &CarrierMotionFrame, a: &CarrierMotionFrame, b: &CarrierMotionFrame) -> bool {
    if !frame_valid(frame)
        || !frame_valid(a)
        || !frame_valid(b)
        || frame.poses.len() != a.poses.len()
        || a.poses.len() != b.poses.len()
    {
        return false;
    }
    let mut selected = (0f64, 0f64);
    for (((id, p), (aid, a)), (bid, b)) in frame.poses.iter().zip(&a.poses).zip(&b.poses) {
        if id != aid || id != bid || !attitude(*p, *a) || !attitude(*a, *b) {
            return false;
        }
        for axis in 0..3 {
            let delta = b.position_m[axis] - a.position_m[axis];
            if delta.abs() > selected.0 {
                selected = (
                    delta.abs(),
                    (p.position_m[axis] - a.position_m[axis]) / delta,
                );
            }
        }
    }
    let t = selected.1;
    if !t.is_finite() || !(-1e-10..=1. + 1e-10).contains(&t) {
        return false;
    }
    frame
        .poses
        .iter()
        .zip(&a.poses)
        .zip(&b.poses)
        .all(|(((_, p), (_, a)), (_, b))| {
            (0..3).all(|i| {
                (p.position_m[i] - (a.position_m[i] + (b.position_m[i] - a.position_m[i]) * t))
                    .abs()
                    <= 1e-8
            })
        })
}
fn owner_resource(owner: &CarrierMechanismOwner) -> CarrierResource {
    match *owner {
        CarrierMechanismOwner::Berth { berth_id } => CarrierResource::ReadyBerth { berth_id },
        CarrierMechanismOwner::Cassette { cassette_id } => {
            CarrierResource::Cassette { cassette_id }
        }
        CarrierMechanismOwner::Loader { loader_id } => CarrierResource::Loader { loader_id },
        CarrierMechanismOwner::Dock { dock } => match dock {
            CarrierCassetteDock::Protected { slot_id } => CarrierResource::MagazineSlot { slot_id },
            CarrierCassetteDock::Ready { berth_id } => CarrierResource::ReadyBerth { berth_id },
            CarrierCassetteDock::TransitBuffer { buffer_id } => {
                CarrierResource::TransitBuffer { buffer_id }
            }
        },
    }
}
impl CarrierMotionRuntime {
    pub fn begin(start: CarrierMotionStart) -> Result<Self> {
        let definition =
            geometry::lookup(&start.binding.registration).context("Carrier source not admitted")?;
        let route = start
            .kind
            .route(definition, start.class)
            .context("No source-proved carrier route")?;
        Self::from_route(start, definition, route)
    }
    pub(super) fn from_route(
        start: CarrierMotionStart,
        definition: &CarrierInventoryDefinition,
        route: Vec<CarrierInventoryKeyframe>,
    ) -> Result<Self> {
        ensure!(
            start.binding.well_formed()
                && start.binding.registration == definition.topology.registration
                && definition.topology.valid_shape()
                && start.operation_id > 0
                && start.drone_id > 0
                && start.system_id > 0
                && start.parent.valid(),
            "Invalid carrier route binding"
        );
        let resources = start
            .kind
            .resources(definition, start.class)
            .context("Incompatible carrier resources")?;
        ensure!(route.len() >= 2, "Carrier route needs actual motion");
        let expected_cassettes: BTreeSet<_> = resources
            .iter()
            .filter_map(|r| match r {
                CarrierResource::Cassette { cassette_id } => Some(*cassette_id),
                _ => None,
            })
            .collect();
        let expected_groups: BTreeSet<_> = definition
            .mechanisms
            .iter()
            .filter(|g| resources.contains(&owner_resource(&g.owner)))
            .map(|g| g.semantic_id.as_str())
            .collect();
        for (index, keyframe) in route.iter().enumerate() {
            let frame = CarrierMotionFrame::from(keyframe);
            let phase_ok = match start.kind {
                CarrierRouteKind::Stage { .. } => {
                    matches!(
                        keyframe.phase,
                        CarrierOperationPhase::Loading | CarrierOperationPhase::Securing
                    ) && !keyframe.milestones.ore_coupled
                }
                CarrierRouteKind::Bay {
                    action: CarrierInventoryAction::Launch,
                    ..
                } => {
                    matches!(
                        keyframe.phase,
                        CarrierOperationPhase::Launching | CarrierOperationPhase::Securing
                    ) && keyframe.completed_stage_legs == 0
                        && !keyframe.milestones.ore_coupled
                }
                CarrierRouteKind::Bay {
                    action: CarrierInventoryAction::ReturnToUnload { .. },
                    ..
                } => {
                    matches!(
                        keyframe.phase,
                        CarrierOperationPhase::Returning
                            | CarrierOperationPhase::Servicing
                            | CarrierOperationPhase::Securing
                    ) && keyframe.completed_stage_legs == 0
                }
                CarrierRouteKind::Bay {
                    action: CarrierInventoryAction::RecallToStorage,
                    ..
                } => {
                    matches!(
                        keyframe.phase,
                        CarrierOperationPhase::Returning | CarrierOperationPhase::Securing
                    ) && keyframe.completed_stage_legs == 0
                        && !keyframe.milestones.ore_coupled
                }
                _ => false,
            };
            ensure!(
                phase_ok,
                "Carrier phase does not describe accepted route action"
            );
            ensure!(
                keyframe.drone_pose.is_some() == start.kind.writes_drone()
                    && keyframe
                        .cassette_poses
                        .iter()
                        .map(|(id, _)| *id)
                        .collect::<BTreeSet<_>>()
                        == expected_cassettes,
                "Source route must retain actual drone and every owned cassette"
            );
            ensure!(
                frame_valid(&frame)
                    && !keyframe.phase.terminal()
                    && keyframe.phase != CarrierOperationPhase::Held
                    && keyframe.completed_stage_legs <= 3,
                "Invalid future carrier keyframe"
            );
            ensure!(
                keyframe
                    .machinery
                    .iter()
                    .map(|(id, _)| id.as_str())
                    .collect::<BTreeSet<_>>()
                    == expected_groups,
                "Route omits owned hardware or writes unrelated groups"
            );
            ensure!(
                keyframe
                    .cassette_poses
                    .iter()
                    .all(|(id, _)| resources
                        .contains(&CarrierResource::Cassette { cassette_id: *id }))
                    && keyframe.handled_cassette_id.is_none_or(
                        |id| resources.contains(&CarrierResource::Cassette { cassette_id: id })
                    )
                    && keyframe.carried_cassette_id.is_none_or(
                        |id| resources.contains(&CarrierResource::Cassette { cassette_id: id })
                    ),
                "Route moves unowned cassette"
            );
            let mut docked_ids = BTreeSet::new();
            for (id, dock) in &keyframe.docked_cassettes {
                ensure!(
                    docked_ids.insert(*id)
                        && resources.contains(&CarrierResource::Cassette { cassette_id: *id }),
                    "Duplicate/unowned docked cassette"
                );
                let source_dock = definition
                    .docks
                    .iter()
                    .find(|d| d.dock == *dock)
                    .context("Source dock absent")?;
                ensure!(
                    keyframe
                        .cassette_poses
                        .iter()
                        .any(|(c, p)| c == id && *p == source_dock.cassette_pose),
                    "Dock milestone differs from actual supported pose"
                );
            }
            ensure!(
                !(keyframe.milestones.cassette_supported || keyframe.milestones.cassette_locked)
                    || keyframe.handled_cassette_id.is_some(),
                "Support/lock milestone lacks handled cassette identity"
            );
            ensure!(
                !keyframe.milestones.crane_latched || keyframe.carried_cassette_id.is_some(),
                "Crane milestone lacks carried identity"
            );
            if index > 0 {
                ensure!(
                    keyframe.completed_stage_legs >= route[index - 1].completed_stage_legs
                        && keyframe.completed_stage_legs
                            <= route[index - 1].completed_stage_legs + 1,
                    "Stage endpoint order changed"
                );
                if keyframe.completed_stage_legs > route[index - 1].completed_stage_legs {
                    let CarrierRouteKind::Stage {
                        direction,
                        source_slot,
                        destination_berth,
                        loaded_cassette,
                        empty_cassette,
                        ..
                    } = start.kind
                    else {
                        anyhow::bail!("Non-stage route changes cassette exchange progress");
                    };
                    let loader = definition
                        .topology
                        .stage_loader(source_slot, destination_berth, start.class)
                        .context("Missing source loader")?;
                    let buffer_id = definition
                        .topology
                        .loader(loader)
                        .unwrap()
                        .transit_buffer_id;
                    let expected = match keyframe.completed_stage_legs {
                        1 => (
                            empty_cassette,
                            CarrierCassetteDock::TransitBuffer { buffer_id },
                        ),
                        2 => (
                            loaded_cassette,
                            direction.loaded_destination(source_slot, destination_berth),
                        ),
                        3 => (
                            empty_cassette,
                            direction.loaded_source(source_slot, destination_berth),
                        ),
                        _ => anyhow::bail!("Invalid cassette exchange completion"),
                    };
                    ensure!(keyframe.handled_cassette_id == Some(expected.0) && keyframe.docked_cassettes.contains(&expected) && keyframe.milestones.cassette_supported && keyframe.milestones.cassette_locked && keyframe.milestones.loader_clear, "Stage leg is not confirmed supported, locked and handed off at its exact destination");
                }
                duration(
                    &CarrierMotionFrame::from(&route[index - 1]),
                    &frame,
                    start.class,
                )?;
            }
        }
        ensure!(
            start.actual == CarrierMotionFrame::from(&route[0]),
            "Actual drone/cassette/hardware differs from route entry"
        );
        if let CarrierRouteKind::Stage {
            direction,
            source_slot,
            destination_berth,
            loaded_cassette,
            empty_cassette,
            segment: stage_segment,
        } = start.kind
        {
            let first = &route[0];
            let last = route.last().unwrap();
            ensure!(
                first.completed_stage_legs
                    == if stage_segment == CarrierStageSegment::EmptyTail {
                        2
                    } else {
                        0
                    },
                "Stage route cannot begin with fabricated completed legs"
            );
            ensure!(
                (stage_segment == CarrierStageSegment::EmptyTail
                    && first.docked_cassettes.contains(&(
                        empty_cassette,
                        CarrierCassetteDock::TransitBuffer {
                            buffer_id: definition
                                .topology
                                .loader(
                                    definition
                                        .topology
                                        .stage_loader(source_slot, destination_berth, start.class)
                                        .unwrap()
                                )
                                .unwrap()
                                .transit_buffer_id
                        }
                    )))
                    || (stage_segment != CarrierStageSegment::EmptyTail
                        && first.docked_cassettes.contains(&(
                            loaded_cassette,
                            direction.loaded_source(source_slot, destination_berth)
                        ))
                        && first.docked_cassettes.contains(&(
                            empty_cassette,
                            direction.loaded_destination(source_slot, destination_berth)
                        ))),
                "Stage entry cassette docks differ from retained source"
            );
            ensure!(
                (stage_segment == CarrierStageSegment::EmptyTail
                    || last.docked_cassettes.contains(&(
                        loaded_cassette,
                        direction.loaded_destination(source_slot, destination_berth)
                    )))
                    && (stage_segment == CarrierStageSegment::Head
                        || last.docked_cassettes.contains(&(
                            empty_cassette,
                            direction.loaded_source(source_slot, destination_berth)
                        ))),
                "Stage endpoint does not complete both physical dock handoffs"
            );
        }
        let end = route.last().unwrap();
        ensure!(
            end.milestones.hardware_safe
                || start.kind.stage_segment() == Some(CarrierStageSegment::Head),
            "No safe carrier route endpoint"
        );
        match start.kind {
            CarrierRouteKind::Stage {
                segment: stage_segment,
                ..
            } => ensure!(
                end.completed_stage_legs
                    == (if stage_segment == CarrierStageSegment::Head {
                        2
                    } else {
                        3
                    })
                    && end.milestones.loader_clear
                    && end.milestones.cassette_supported
                    && end.milestones.cassette_locked
                    && (stage_segment == CarrierStageSegment::EmptyTail
                        || end.milestones.drone_clamped),
                "Stage cannot finish before supported locked ready and clear loader"
            ),
            CarrierRouteKind::Bay {
                action:
                    CarrierInventoryAction::Launch | CarrierInventoryAction::ReturnToUnload { .. },
                ..
            } => ensure!(
                end.milestones.exterior_clear,
                "Missing actual exterior clear endpoint"
            ),
            CarrierRouteKind::Bay {
                action: CarrierInventoryAction::RecallToStorage,
                ..
            } => ensure!(
                end.milestones.cassette_supported
                    && end.milestones.cassette_locked
                    && end.milestones.drone_clamped,
                "Storage lacks actual supported clamped endpoint"
            ),
            _ => anyhow::bail!("Invalid carrier route kind"),
        }
        let confirmed = CarrierMotionFrame::from(&route[0]);
        let duration_ms = duration(
            &confirmed,
            &CarrierMotionFrame::from(&route[1]),
            start.class,
        )?;
        ensure!(
            start.now.checked_add(duration_ms).is_some(),
            "Carrier clock overflow"
        );
        Ok(Self {
            binding: start.binding,
            operation_id: start.operation_id,
            drone_id: start.drone_id,
            class: start.class,
            kind: start.kind,
            parent: start.parent,
            system_id: start.system_id,
            reached: route[0].milestones,
            completed_stage_legs: route[0].completed_stage_legs,
            route,
            target_index: 1,
            source: confirmed.clone(),
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
    /// A destroyed loaded body never hands ownership to Ready. Continue the
    /// original full source route with retained empty hardware and unchanged pose.
    pub(super) fn retain_full_destroyed_cleanup(
        &mut self,
        definition: &CarrierInventoryDefinition,
        now: u64,
    ) -> Result<()> {
        if self.waiting_for_transfer {
            self.waiting_for_transfer = false;
            self.next_segment(now)?;
        }
        let CarrierRouteKind::Stage {
            segment: CarrierStageSegment::Head,
            direction,
            source_slot,
            destination_berth,
            loaded_cassette,
            empty_cassette,
        } = self.kind
        else {
            return Ok(());
        };
        let kind = CarrierRouteKind::Stage {
            segment: CarrierStageSegment::Full,
            direction,
            source_slot,
            destination_berth,
            loaded_cassette,
            empty_cassette,
        };
        let full = kind
            .route(definition, self.class)
            .context("Destroyed Stage full cleanup source missing")?;
        ensure!(
            full.starts_with(&self.route) && self.target_index < full.len(),
            "Cleanup cannot replace accepted source prefix"
        );
        self.kind = kind;
        self.route = full;
        if self.ready_to_finalize {
            self.ready_to_finalize = false;
            self.start_remaining(now)?;
        }
        Ok(())
    }
    pub fn hold(&mut self, reason: &str) {
        self.held = true;
        self.hold_reason = reason.into();
    }
    fn start_remaining(&mut self, now: u64) -> Result<()> {
        self.source = self.confirmed.clone();
        self.started_at_ms = now;
        self.confirmed_at_ms = now;
        self.duration_ms = duration(
            &self.source,
            &CarrierMotionFrame::from(&self.route[self.target_index]),
            self.class,
        )?;
        ensure!(
            now.checked_add(self.duration_ms).is_some(),
            "Carrier clock overflow"
        );
        Ok(())
    }
    pub(super) fn resume_validated(&mut self, now: u64) -> Result<()> {
        ensure!(
            self.held && now >= self.confirmed_at_ms && !self.ready_to_finalize,
            "Invalid carrier resume"
        );
        if !self.waiting_for_transfer {
            self.start_remaining(now)?;
        } else {
            self.confirmed_at_ms = now;
        }
        self.held = false;
        self.hold_reason.clear();
        Ok(())
    }
    pub fn advance(&mut self, now: u64, clear: bool) -> Result<CarrierMotionProgress> {
        ensure!(now >= self.confirmed_at_ms, "Carrier clock reversed");
        if self.held {
            return Ok(CarrierMotionProgress::Held);
        }
        if self.ready_to_finalize {
            return Ok(CarrierMotionProgress::FinalizationRequired);
        }
        if now - self.confirmed_at_ms > MAX_ADVANCE_GAP_MS {
            self.hold("Simulation gap; explicit route revalidation required");
            return Ok(CarrierMotionProgress::Held);
        }
        if !clear {
            self.hold("Physical carrier route blocked");
            return Ok(CarrierMotionProgress::Held);
        }
        if self.waiting_for_transfer {
            self.confirmed_at_ms = now;
            return Ok(CarrierMotionProgress::TransferBarrier);
        }
        let target = &self.route[self.target_index];
        self.confirmed = sample(
            &self.source,
            &CarrierMotionFrame::from(target),
            self.started_at_ms,
            self.duration_ms,
            now,
        )?;
        self.confirmed_at_ms = now;
        if now < self.started_at_ms + self.duration_ms {
            return Ok(CarrierMotionProgress::Moving);
        }
        let index = self.target_index;
        self.reached = target.milestones;
        self.completed_stage_legs = target.completed_stage_legs;
        if self.reached.ore_coupled {
            self.waiting_for_transfer = true;
            return Ok(CarrierMotionProgress::TransferBarrier);
        }
        self.next_segment(now)?;
        Ok(if self.ready_to_finalize {
            CarrierMotionProgress::FinalizationRequired
        } else {
            CarrierMotionProgress::Milestone { index }
        })
    }
    fn next_segment(&mut self, now: u64) -> Result<()> {
        self.target_index += 1;
        if self.target_index == self.route.len() {
            self.ready_to_finalize = true;
            return Ok(());
        }
        self.start_remaining(now)
    }
    pub(super) fn release_transfer_barrier(&mut self, now: u64) -> Result<()> {
        ensure!(
            self.waiting_for_transfer && !self.held && now >= self.confirmed_at_ms,
            "No live confirmed transfer barrier"
        );
        self.waiting_for_transfer = false;
        self.next_segment(now)
    }
    pub(super) fn validate_expected(
        &self,
        expected: &[CarrierInventoryKeyframe],
        actual_parent: BayPose,
        actual_system: u32,
        actual: &CarrierMotionFrame,
    ) -> Result<()> {
        ensure!(
            self.route == expected
                && self.parent == actual_parent
                && self.system_id == actual_system
                && self.confirmed == *actual,
            "Persisted carrier source/parent/actual roots differ"
        );
        ensure!(
            !self.ready_to_finalize
                && self.target_index > 0
                && self.target_index < self.route.len()
                && self.started_at_ms <= self.confirmed_at_ms
                && self.duration_ms > 0
                && self.started_at_ms.checked_add(self.duration_ms).is_some(),
            "Invalid persisted carrier cursor/clock"
        );
        let prior = CarrierMotionFrame::from(&self.route[self.target_index - 1]);
        let target = CarrierMotionFrame::from(&self.route[self.target_index]);
        ensure!(
            on_segment(&self.source, &prior, &target)
                && on_segment(&self.confirmed, &self.source, &target),
            "Carrier shared progress differs between components"
        );
        ensure!(
            self.duration_ms == duration(&self.source, &target, self.class)?
                && (self.held != self.hold_reason.is_empty() && self.hold_reason.len() <= 512),
            "Invalid retained rate or hold reason"
        );
        let reached_index = if self.waiting_for_transfer {
            self.target_index
        } else {
            self.target_index - 1
        };
        ensure!(
            self.reached == self.route[reached_index].milestones
                && self.completed_stage_legs == self.route[reached_index].completed_stage_legs,
            "Retained milestones differ from confirmed endpoint"
        );
        if self.waiting_for_transfer {
            ensure!(
                self.confirmed == target && self.route[self.target_index].milestones.ore_coupled,
                "Uncoupled transfer barrier"
            );
        } else {
            ensure!(
                self.confirmed
                    == sample(
                        &self.source,
                        &target,
                        self.started_at_ms,
                        self.duration_ms,
                        self.confirmed_at_ms
                    )?,
                "Retained sample differs from shared clock"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
