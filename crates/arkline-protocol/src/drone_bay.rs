//! Persistent finite drone stock for the explicit, source-registered bay preview.
//! This schema does not grant authority, define inventory prices or invent battery
//! ratings. A physical geometry revision must be supported before using requests.
use crate::drone_bay_geometry::{BayMotionSegment, BayPose};
use crate::{
    DroneBayBatchJournal, DroneBayBatchKind, DroneBayBatchOperation, DroneClass, DroneView,
    DRONE_BAY_BATCH_VERSION, DRONE_CLASS_CAP, DRONE_HIT_POINTS, MAX_DRONES, SPACE_BOUND,
};
use serde::{Deserialize, Serialize};

pub const DRONE_BAY_LEGACY_SCHEMA_VERSION: u32 = 1;
pub const DRONE_BAY_SCHEMA_VERSION: u32 = 2;
pub fn drone_bay_schema_supported(version: u32) -> bool {
    matches!(
        version,
        DRONE_BAY_LEGACY_SCHEMA_VERSION | DRONE_BAY_SCHEMA_VERSION
    )
}
/// Existing Connect/viewer world capability field; no new credentials or roles.
pub const DRONE_BAY_CLIENT_CAPABILITY_VERSION: u32 = 3;
pub const DRONE_BAY_INITIAL_STOCK: usize = MAX_DRONES;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayRegistration {
    pub geometry_revision: u32,
    pub source_sha256: String,
}
impl DroneBayRegistration {
    pub fn bind(&self, vessel_id: String) -> DroneBayBinding {
        DroneBayBinding {
            vessel_id,
            geometry_revision: self.geometry_revision,
            source_sha256: self.source_sha256.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayBinding {
    pub vessel_id: String,
    pub geometry_revision: u32,
    pub source_sha256: String,
}
impl DroneBayBinding {
    pub fn well_formed(&self) -> bool {
        !self.vessel_id.is_empty()
            && self.vessel_id.len() <= 256
            && self.geometry_revision > 0
            && self.source_sha256.len() == 64
            && self.source_sha256.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

/// The home cradle is reserved for this identity even while its body is away.
/// Current HP exists in exactly one place: Stored here, or the active DroneView.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DroneStockStatus {
    Stored { hit_points: u32 },
    Active,
    Destroyed { at_ms: u64 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneStockEntry {
    pub drone_id: u64,
    pub class: DroneClass,
    pub home_cradle: u8,
    pub status: DroneStockStatus,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayInitialization {
    /// Explicit one-time preview fixture, not a purchase or replacement policy.
    pub fixture_version: u32,
    pub seeded_at_ms: u64,
    pub first_drone_id: u64,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneStockCounts {
    pub stored: usize,
    pub active: usize,
    pub destroyed: usize,
}
impl DroneStockCounts {
    pub fn total(self) -> usize {
        self.stored + self.active + self.destroyed
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneBayOperationKind {
    Launch,
    Recover,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneBayPhase {
    ReleasingCradle,
    TransferringOut,
    OpeningDoor,
    Departing,
    Free,
    Returning,
    WaitingForBay,
    Aligning,
    Entering,
    TransferringIn,
    SecuringCradle,
    Stored,
    Held,
    Cancelled,
    Destroyed,
}
impl DroneBayPhase {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Free | Self::Stored | Self::Cancelled | Self::Destroyed
        )
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::ReleasingCradle => "Releasing cradle",
            Self::TransferringOut => "Transferring to launch throat",
            Self::OpeningDoor => "Opening bay door",
            Self::Departing => "Clearing drone bay",
            Self::Free => "Deployed",
            Self::Returning => "Returning to drone bay",
            Self::WaitingForBay => "Waiting for drone bay",
            Self::Aligning => "Aligning with drone bay",
            Self::Entering => "Entering drone bay",
            Self::TransferringIn => "Returning to home cradle",
            Self::SecuringCradle => "Securing home cradle",
            Self::Stored => "Docked in home cradle",
            Self::Held => "Drone bay operation held",
            Self::Cancelled => "Drone bay operation cancelled",
            Self::Destroyed => "Drone destroyed",
        }
    }
}

/// Durable accepted intent. Geometry motion is added only from the independently
/// validated body-frame sampler and source registration, never guessed anchors.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayOperation {
    pub operation_id: u64,
    pub drone_id: u64,
    pub kind: DroneBayOperationKind,
    pub binding: DroneBayBinding,
    pub phase: DroneBayPhase,
    pub held_phase: Option<DroneBayPhase>,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
    pub job_id: Option<u64>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batches: Option<DroneBayBatchJournal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<DroneBayCleanupView>,
    pub schema_version: u32,
    pub binding: DroneBayBinding,
    pub initialization: DroneBayInitialization,
    pub stock: Vec<DroneStockEntry>,
    pub high_watermark: u64,
    /// No eviction in this preview. A retained ID is never reused or reexecuted.
    pub operations: Vec<DroneBayOperation>,
    /// One shared transfer route; this is not a five-drone capacity limit.
    pub handling_operation: Option<u64>,
}
impl DroneBayState {
    pub fn counts(&self, class: Option<DroneClass>) -> DroneStockCounts {
        let mut counts = DroneStockCounts::default();
        for entry in self
            .stock
            .iter()
            .filter(|e| class.is_none_or(|c| e.class == c))
        {
            match entry.status {
                DroneStockStatus::Stored { .. } => counts.stored += 1,
                DroneStockStatus::Active => counts.active += 1,
                DroneStockStatus::Destroyed { .. } => counts.destroyed += 1,
            }
        }
        counts
    }
    pub fn operation(&self, id: u64) -> Option<&DroneBayOperation> {
        self.operations.iter().find(|op| op.operation_id == id)
    }
    pub fn active_operation(&self, drone_id: u64) -> Option<&DroneBayOperation> {
        self.operations
            .iter()
            .find(|op| op.drone_id == drone_id && !op.phase.terminal())
    }
    pub fn expected_class(cradle: u8) -> Option<DroneClass> {
        match usize::from(cradle) / DRONE_CLASS_CAP {
            0 => Some(DroneClass::Recon),
            1 => Some(DroneClass::Mining),
            2 if usize::from(cradle) < DRONE_BAY_INITIAL_STOCK => Some(DroneClass::Attack),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DroneBayRequest {
    BeginBatch {
        binding: DroneBayBinding,
        batch_id: u64,
        batch_kind: DroneBayBatchKind,
    },
    HoldBatch {
        binding: DroneBayBinding,
        batch_id: u64,
    },
    ResumeBatch {
        binding: DroneBayBinding,
        batch_id: u64,
    },
    CancelBatch {
        binding: DroneBayBinding,
        batch_id: u64,
    },
    Launch {
        binding: DroneBayBinding,
        drone_id: u64,
        operation_id: u64,
    },
    Recover {
        binding: DroneBayBinding,
        drone_id: u64,
        operation_id: u64,
    },
    Hold {
        binding: DroneBayBinding,
        operation_id: u64,
    },
    Resume {
        binding: DroneBayBinding,
        operation_id: u64,
    },
    Cancel {
        binding: DroneBayBinding,
        operation_id: u64,
    },
}
impl DroneBayRequest {
    pub fn binding(&self) -> &DroneBayBinding {
        match self {
            Self::BeginBatch { binding, .. }
            | Self::HoldBatch { binding, .. }
            | Self::ResumeBatch { binding, .. }
            | Self::CancelBatch { binding, .. }
            | Self::Launch { binding, .. }
            | Self::Recover { binding, .. }
            | Self::Hold { binding, .. }
            | Self::Resume { binding, .. }
            | Self::Cancel { binding, .. } => binding,
        }
    }
}

/// Public read-only projections. Canonical stock and active DroneView state are
/// joined by ID server-side; clients never transfer inventory or complete motion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneBayPresence {
    Stored,
    Active,
    Destroyed,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneCradleView {
    pub drone_id: u64,
    pub class: DroneClass,
    pub hit_points: u32,
    pub home_cradle: u8,
    pub presence: DroneBayPresence,
    /// Actual home anchor, not the current location of an active drone.
    pub body_pose: BayPose,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneBayMotionFrame {
    Body,
    World,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneBayMotionView {
    pub drone_id: u64,
    pub operation_id: u64,
    pub phase: DroneBayPhase,
    pub frame: DroneBayMotionFrame,
    pub confirmed_pose: BayPose,
    pub segment: Option<BayMotionSegment>,
    pub updated_at_ms: u64,
}
impl DroneBayMotionView {
    /// Presentation only: endpoints never change presence or complete a phase.
    pub fn sample(&self, now: u64) -> Option<(BayPose, [f64; 3])> {
        if self.phase == DroneBayPhase::Held {
            return self
                .confirmed_pose
                .valid()
                .then_some((self.confirmed_pose, [0.; 3]));
        }
        match self.segment {
            Some(segment) => segment.sample(now),
            None => self
                .confirmed_pose
                .valid()
                .then_some((self.confirmed_pose, [0.; 3])),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneBayScalarSegment {
    pub from: f64,
    pub to: f64,
    pub started_at_ms: u64,
    pub duration_ms: u64,
}
impl DroneBayScalarSegment {
    pub fn sample(self, now: u64) -> Option<f64> {
        if !self.from.is_finite()
            || !self.to.is_finite()
            || !(0.0..=1.0).contains(&self.from)
            || !(0.0..=1.0).contains(&self.to)
            || self.duration_ms == 0
            || self.started_at_ms.checked_add(self.duration_ms).is_none()
        {
            return None;
        }
        let t =
            (now.saturating_sub(self.started_at_ms) as f64 / self.duration_ms as f64).clamp(0., 1.);
        Some(self.from + (self.to - self.from) * t * t * (3. - 2. * t))
    }
}
/// Actual authored dynamic group root in body space. The renderer applies this
/// accepted pose to the registered group, not a separately inferred animation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneBayMechanismView {
    pub semantic_id: String,
    pub confirmed_pose: BayPose,
    pub segment: Option<BayMotionSegment>,
}
impl DroneBayMechanismView {
    pub fn sample(&self, now: u64) -> Option<BayPose> {
        match self.segment {
            Some(segment) => segment.sample(now).map(|(pose, _)| pose),
            None => self.confirmed_pose.valid().then_some(self.confirmed_pose),
        }
    }
}
/// Empty hardware may still need to return after a terminal destruction. This
/// is not a live drone operation and never revives or replaces its stock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayCleanupView {
    pub operation_id: u64,
    pub held: bool,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneBayMachineryView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cleanup: Option<DroneBayCleanupView>,
    pub handling_operation: Option<u64>,
    pub door_fraction: f64,
    pub door_segment: Option<DroneBayScalarSegment>,
    pub groups: Vec<DroneBayMechanismView>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneBayPublicState {
    /// Absent in unchanged single-operation worlds; Some(1) also advertises an idle queue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_capability: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<DroneBayBatchOperation>,
    pub schema_version: u32,
    pub binding: DroneBayBinding,
    pub cradles: Vec<DroneCradleView>,
    /// Only constrained active entities; stored bodies appear in cradles alone.
    pub motions: Vec<DroneBayMotionView>,
    pub machinery: DroneBayMachineryView,
}
impl DroneBayPublicState {
    /// `expected_binding`, home poses and mechanism IDs MUST come from the
    /// client's supported authored registration + actual parent vessel identity,
    /// never be copied back from this untrusted projection as their own proof.
    pub fn validate_projection(
        &self,
        expected_binding: &DroneBayBinding,
        home_poses: &[BayPose],
        mechanism_ids: &[&str],
        active: &[DroneView],
    ) -> Result<(), String> {
        let fail = |message: &str| Err(message.to_owned());
        if !drone_bay_schema_supported(self.schema_version)
            || !expected_binding.well_formed()
            || self.binding != *expected_binding
        {
            return fail("Unsupported drone bay source or parent vessel");
        }
        if (self.schema_version == DRONE_BAY_LEGACY_SCHEMA_VERSION
            && (self.batch_capability.is_some() || self.batch.is_some()))
            || (self.schema_version == DRONE_BAY_SCHEMA_VERSION
                && self.batch_capability != Some(DRONE_BAY_BATCH_VERSION))
            || self.batch.as_ref().is_some_and(|batch| {
                !batch.validate_shape(expected_binding)
                    || batch.members.iter().any(|member| {
                        !self.cradles.iter().any(|cradle| {
                            cradle.drone_id == member.drone_id
                                && cradle.home_cradle == member.home_cradle
                        })
                    })
            })
        {
            return fail("Invalid or unsupported physical bay batch projection");
        }
        if let Some(batch) = self.batch.as_ref().filter(|b| b.active()) {
            if batch.current_operation_id
                != self
                    .machinery
                    .handling_operation
                    .or_else(|| self.machinery.cleanup.as_ref().map(|c| c.operation_id))
                || batch.members.iter().any(|member| {
                    let Some(cradle) = self.cradles.iter().find(|c| c.drone_id == member.drone_id)
                    else {
                        return true;
                    };
                    match member.status {
                        crate::DroneBayBatchMemberStatus::Pending => match batch.kind {
                            DroneBayBatchKind::DeployClass { class } => {
                                cradle.class != class || cradle.presence != DroneBayPresence::Stored
                            }
                            DroneBayBatchKind::RecallAll => {
                                cradle.presence != DroneBayPresence::Active
                            }
                        },
                        crate::DroneBayBatchMemberStatus::Active
                        | crate::DroneBayBatchMemberStatus::Servicing => {
                            cradle.presence != DroneBayPresence::Active
                        }
                        crate::DroneBayBatchMemberStatus::Destroyed => {
                            cradle.presence != DroneBayPresence::Destroyed
                        }
                        _ => false,
                    }
                })
            {
                return fail("Batch reservation does not match actual bay machinery/stock");
            }
        }
        if self.cradles.len() != DRONE_BAY_INITIAL_STOCK
            || home_poses.len() != DRONE_BAY_INITIAL_STOCK
            || active.len() > MAX_DRONES
        {
            return fail("Invalid finite drone roster");
        }
        let valid_pose =
            |pose: BayPose| pose.valid() && pose.position_m.iter().all(|v| v.abs() <= SPACE_BOUND);
        let valid_segment = |segment: BayMotionSegment| {
            valid_pose(segment.source)
                && valid_pose(segment.target)
                && segment
                    .started_at_ms
                    .checked_add(segment.duration_ms)
                    .is_some_and(|end| {
                        segment.sample(segment.started_at_ms).is_some()
                            && segment
                                .sample(segment.started_at_ms + segment.duration_ms / 2)
                                .is_some()
                            && segment.sample(end).is_some()
                    })
        };
        let mut ids = std::collections::BTreeSet::new();
        let mut cradles = std::collections::BTreeSet::new();
        for entry in &self.cradles {
            if entry.drone_id == 0
                || !ids.insert(entry.drone_id)
                || !cradles.insert(entry.home_cradle)
                || DroneBayState::expected_class(entry.home_cradle) != Some(entry.class)
                || !valid_pose(entry.body_pose)
                || home_poses.get(usize::from(entry.home_cradle)) != Some(&entry.body_pose)
            {
                return fail("Invalid or duplicate drone cradle identity/pose");
            }
            let matches: Vec<_> = active.iter().filter(|d| d.id == entry.drone_id).collect();
            match entry.presence {
                DroneBayPresence::Stored
                    if matches.is_empty() && (1..=DRONE_HIT_POINTS).contains(&entry.hit_points) => {
                }
                DroneBayPresence::Destroyed if matches.is_empty() && entry.hit_points == 0 => {}
                DroneBayPresence::Active
                    if matches.len() == 1
                        && (1..=DRONE_HIT_POINTS).contains(&entry.hit_points)
                        && matches[0].hit_points == entry.hit_points
                        && matches[0].class == entry.class => {}
                _ => return fail("Drone body or health has conflicting ownership"),
            }
        }
        if active.iter().any(|drone| {
            !self
                .cradles
                .iter()
                .any(|c| c.drone_id == drone.id && c.presence == DroneBayPresence::Active)
        }) {
            return fail("Active drone is outside the finite roster");
        }
        let mut moving_ids = std::collections::BTreeSet::new();
        let mut operations = std::collections::BTreeSet::new();
        for motion in &self.motions {
            if motion.operation_id == 0
                || !operations.insert(motion.operation_id)
                || !moving_ids.insert(motion.drone_id)
                || motion.phase.terminal()
                || !self.cradles.iter().any(|c| {
                    c.drone_id == motion.drone_id && c.presence == DroneBayPresence::Active
                })
                || !valid_pose(motion.confirmed_pose)
                || motion.segment.is_some_and(|s| !valid_segment(s))
            {
                return fail("Invalid constrained drone motion");
            }
        }
        if self
            .machinery
            .handling_operation
            .is_some_and(|id| !operations.contains(&id))
            || !self.machinery.door_fraction.is_finite()
            || !(0.0..=1.0).contains(&self.machinery.door_fraction)
            || self
                .machinery
                .door_segment
                .is_some_and(|s| s.sample(s.started_at_ms).is_none())
        {
            return fail("Invalid drone bay machinery state");
        }
        if let Some(cleanup) = &self.machinery.cleanup {
            if cleanup.operation_id == 0
                || self.machinery.handling_operation.is_some()
                || self
                    .motions
                    .iter()
                    .any(|m| m.operation_id == cleanup.operation_id)
                || (cleanup.held
                    && (self.machinery.door_segment.is_some()
                        || self.machinery.groups.iter().any(|g| g.segment.is_some())))
            {
                return fail("Invalid terminal hardware cleanup");
            }
        }
        if self.machinery.handling_operation.is_some_and(|id| {
            self.motions
                .iter()
                .any(|m| m.operation_id == id && m.phase == DroneBayPhase::Held)
        }) && (self.machinery.door_segment.is_some()
            || self.machinery.groups.iter().any(|g| g.segment.is_some()))
        {
            return fail("Held bay machinery must not keep advancing");
        }
        let expected: std::collections::BTreeSet<_> = mechanism_ids.iter().copied().collect();
        let mut actual = std::collections::BTreeSet::new();
        if expected.len() != mechanism_ids.len() || self.machinery.groups.len() != expected.len() {
            return fail("Unsupported drone bay machinery groups");
        }
        for group in &self.machinery.groups {
            if !expected.contains(group.semantic_id.as_str())
                || !actual.insert(group.semantic_id.as_str())
                || !valid_pose(group.confirmed_pose)
                || group.segment.is_some_and(|s| !valid_segment(s))
            {
                return fail("Invalid drone bay machinery pose");
            }
        }
        Ok(())
    }

    pub fn constrained(&self, drone_id: u64) -> Option<&DroneBayMotionView> {
        drone_bay_schema_supported(self.schema_version).then_some(())?;
        self.motions
            .iter()
            .find(|motion| motion.drone_id == drone_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_stock_capacity_is_not_the_five_display_count() {
        assert_eq!(DRONE_BAY_INITIAL_STOCK, 15);
        for (cradle, class) in (0..15).zip(
            [DroneClass::Recon; 5]
                .into_iter()
                .chain([DroneClass::Mining; 5])
                .chain([DroneClass::Attack; 5]),
        ) {
            assert_eq!(DroneBayState::expected_class(cradle), Some(class));
        }
        assert!(DroneBayState::expected_class(15).is_none());
    }
    #[test]
    fn stock_is_one_mutually_exclusive_state_without_duplicate_health() {
        for status in [
            DroneStockStatus::Stored { hit_points: 37 },
            DroneStockStatus::Active,
            DroneStockStatus::Destroyed { at_ms: 9 },
        ] {
            let wire = serde_json::to_value(&status).unwrap();
            assert_eq!(
                wire.get("hit_points").is_some(),
                matches!(status, DroneStockStatus::Stored { .. })
            );
            assert_eq!(
                serde_json::from_value::<DroneStockStatus>(wire).unwrap(),
                status
            );
        }
    }
    #[test]
    fn identified_requests_roundtrip_without_new_credentials() {
        let binding = DroneBayBinding {
            vessel_id: "actual-owned-id".into(),
            geometry_revision: 12,
            source_sha256: "a".repeat(64),
        };
        assert!(binding.well_formed());
        for request in [
            DroneBayRequest::Launch {
                binding: binding.clone(),
                drone_id: 23,
                operation_id: 4,
            },
            DroneBayRequest::Recover {
                binding: binding.clone(),
                drone_id: 23,
                operation_id: 5,
            },
            DroneBayRequest::Hold {
                binding: binding.clone(),
                operation_id: 5,
            },
            DroneBayRequest::Resume {
                binding: binding.clone(),
                operation_id: 5,
            },
            DroneBayRequest::Cancel {
                binding,
                operation_id: 5,
            },
        ] {
            let wire = serde_json::to_vec(&request).unwrap();
            assert_eq!(
                serde_json::from_slice::<DroneBayRequest>(&wire).unwrap(),
                request
            );
        }
    }
    #[test]
    fn destroyed_and_stored_are_terminal_but_holding_does_not_complete_motion() {
        for phase in [
            DroneBayPhase::Free,
            DroneBayPhase::Stored,
            DroneBayPhase::Cancelled,
            DroneBayPhase::Destroyed,
        ] {
            assert!(phase.terminal());
        }
        for phase in [
            DroneBayPhase::Held,
            DroneBayPhase::Entering,
            DroneBayPhase::SecuringCradle,
        ] {
            assert!(!phase.terminal());
        }
    }
    fn projection() -> (DroneBayPublicState, Vec<BayPose>) {
        let homes: Vec<_> = (0..15)
            .map(|i| BayPose {
                position_m: [f64::from(i) * 10., 0., 0.],
                orientation_xyzw: [0., 0., 0., 1.],
            })
            .collect();
        let state = DroneBayPublicState {
            batch_capability: None,
            batch: None,
            schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
            binding: DroneBayBinding {
                vessel_id: "test-owned-vessel".into(),
                geometry_revision: 1,
                source_sha256: "b".repeat(64),
            },
            cradles: (0..15)
                .map(|i| DroneCradleView {
                    drone_id: 100 + u64::from(i),
                    class: DroneBayState::expected_class(i).unwrap(),
                    hit_points: 100,
                    home_cradle: i,
                    presence: DroneBayPresence::Stored,
                    body_pose: homes[usize::from(i)],
                })
                .collect(),
            motions: vec![],
            machinery: DroneBayMachineryView {
                cleanup: None,
                handling_operation: None,
                door_fraction: 0.,
                door_segment: None,
                groups: vec![DroneBayMechanismView {
                    semantic_id: "test-group".into(),
                    confirmed_pose: homes[0],
                    segment: None,
                }],
            },
        };
        (state, homes)
    }
    fn active_drone() -> DroneView {
        DroneView {
            orientation_xyzw: None,
            id: 100,
            class: DroneClass::Recon,
            hit_points: 37,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [0.; 3],
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            mode: crate::DroneMode::Hold,
            updated_at_ms: 1000,
        }
    }
    #[test]
    fn batch_capability_distinguishes_legacy_supported_idle_and_malformed() {
        let (mut state, homes) = projection();
        let check = |s: &DroneBayPublicState| {
            s.validate_projection(&s.binding, &homes, &["test-group"], &[])
        };
        assert!(check(&state).is_ok());
        state.batch_capability = Some(DRONE_BAY_BATCH_VERSION);
        assert!(check(&state).is_err());
        state.schema_version = DRONE_BAY_SCHEMA_VERSION;
        assert!(check(&state).is_ok());
        state.batch_capability = None;
        assert!(check(&state).is_err());
        state.batch_capability = Some(99);
        assert!(check(&state).is_err());
    }
    #[test]
    fn public_projection_rejects_wrong_geometry_duplicates_and_conflicting_ownership() {
        let (valid, homes) = projection();
        assert!(valid
            .validate_projection(&valid.binding, &homes, &["test-group"], &[])
            .is_ok());
        for bad in [
            "schema",
            "binding",
            "id",
            "cradle",
            "class",
            "health",
            "dead-health",
            "home",
            "nan",
            "group",
            "missing-group",
            "door",
            "unknown-active",
            "stored-active",
        ] {
            let mut state = valid.clone();
            let mut active = vec![];
            match bad {
                "schema" => state.schema_version = 99,
                "binding" => state.binding.source_sha256 = "c".repeat(64),
                "id" => state.cradles[1].drone_id = 100,
                "cradle" => state.cradles[1].home_cradle = 0,
                "class" => state.cradles[0].class = DroneClass::Attack,
                "health" => state.cradles[0].hit_points = 101,
                "dead-health" => state.cradles[0].presence = DroneBayPresence::Destroyed,
                "home" => state.cradles[0].body_pose.position_m[0] += 1.,
                "nan" => state.cradles[0].body_pose.position_m[0] = f64::NAN,
                "group" => state.machinery.groups[0].semantic_id = "foreign".into(),
                "missing-group" => state.machinery.groups.clear(),
                "door" => state.machinery.door_fraction = 1.1,
                "unknown-active" => {
                    let mut d = active_drone();
                    d.id = 999;
                    active.push(d);
                }
                "stored-active" => active.push(active_drone()),
                _ => unreachable!(),
            }
            assert!(
                state
                    .validate_projection(&valid.binding, &homes, &["test-group"], &active)
                    .is_err(),
                "{bad}"
            );
        }
    }
    #[test]
    fn public_constrained_motion_requires_active_identity_and_held_pose_stays_confirmed() {
        let (mut state, homes) = projection();
        state.cradles[0].presence = DroneBayPresence::Active;
        state.cradles[0].hit_points = 37;
        let drone = active_drone();
        let confirmed = BayPose {
            position_m: [3., 0., 0.],
            ..homes[0]
        };
        state.motions.push(DroneBayMotionView {
            drone_id: drone.id,
            operation_id: 1,
            phase: DroneBayPhase::Held,
            frame: DroneBayMotionFrame::Body,
            confirmed_pose: confirmed,
            segment: Some(BayMotionSegment {
                source: homes[0],
                target: homes[1],
                started_at_ms: 1000,
                duration_ms: 1000,
            }),
            updated_at_ms: 1500,
        });
        state.machinery.handling_operation = Some(1);
        assert!(state
            .validate_projection(
                &state.binding,
                &homes,
                &["test-group"],
                std::slice::from_ref(&drone)
            )
            .is_ok());
        assert_eq!(
            state.constrained(100).unwrap().sample(u64::MAX),
            Some((confirmed, [0.; 3]))
        );
        state.motions.push(state.motions[0].clone());
        assert!(state
            .validate_projection(
                &state.binding,
                &homes,
                &["test-group"],
                std::slice::from_ref(&drone)
            )
            .is_err());
        state.motions.pop();
        state.motions[0].drone_id = 101;
        assert!(state
            .validate_projection(&state.binding, &homes, &["test-group"], &[drone])
            .is_err());
    }
}
