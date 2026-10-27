//! Fresh-preview drone-owned ore and source-bound quick receiver operations.
//! Ore units are not kilograms. This schema does not authorize a source asset.
use crate::drone_bay_geometry::{BayMotionSegment, BayPose};
use crate::{
    DroneBayCleanupView, DroneBayMechanismView, DroneBayMotionFrame, DroneBayRegistration,
    DRONE_CLASS_CAP, DRONE_MINING_CYCLE_MS, DRONE_MINING_YIELD,
};
use serde::{Deserialize, Serialize};

pub const DRONE_CARGO_SCHEMA_VERSION: u32 = 1;
pub const DRONE_CARGO_CLIENT_CAPABILITY_VERSION: u32 = 5;
pub const DRONE_CARGO_RULESET_VERSION: u32 = 2;
pub const MINING_DRONE_CARGO_CAPACITY: u64 = 100;
pub const DRONE_DROPOFF_BATCH_UNITS: u64 = 25;
pub const DRONE_DROPOFF_BATCH_MS: u64 = 500;
fn hash_valid(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneCargoRegistration {
    pub base: DroneBayRegistration,
    pub receiver_revision: u32,
    pub receiver_source_sha256: String,
}
impl DroneCargoRegistration {
    pub fn well_formed(&self) -> bool {
        self.base.geometry_revision > 0
            && hash_valid(&self.base.source_sha256)
            && self.receiver_revision > 0
            && hash_valid(&self.receiver_source_sha256)
    }
    pub fn supported(&self) -> bool {
        *self == supported_drone_cargo_registration()
    }
    pub fn bind(&self, vessel_id: String) -> DroneCargoBinding {
        DroneCargoBinding {
            vessel_id,
            registration: self.clone(),
        }
    }
}
/// Exact independently authored receiver and unchanged base hull. Well-formed
/// arbitrary hashes are not accepted as a runtime source registration.
pub fn supported_drone_cargo_registration() -> DroneCargoRegistration {
    use crate::{drone_bay_definition::PIONEER_BAY_REG12, drone_dropoff_geometry as receiver};
    DroneCargoRegistration {
        base: PIONEER_BAY_REG12.registration(),
        receiver_revision: receiver::DROPOFF_RECEIVER_REVISION,
        receiver_source_sha256: receiver::DROPOFF_RECEIVER_SOURCE_SHA256.into(),
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneCargoBinding {
    pub vessel_id: String,
    pub registration: DroneCargoRegistration,
}
impl DroneCargoBinding {
    pub fn well_formed(&self) -> bool {
        !self.vessel_id.is_empty() && self.vessel_id.len() <= 256 && self.registration.well_formed()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneCargoCycleClaim {
    pub cycle_id: u64,
    pub target_id: u64,
    /// Capacity promised to a real cycle; not extracted ore or a second balance.
    pub reserved_units: u64,
    pub started_at_ms: u64,
    pub completes_at_ms: u64,
    pub job_id: Option<u64>,
    pub intent_generation: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneMiningIntent {
    pub generation: u64,
    pub target_id: u64,
    pub job_id: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneOreHold {
    pub drone_id: u64,
    pub ore_units: u64,
    pub destroyed_at_ms: Option<u64>,
    pub intent_generation: u64,
    pub mining_intent: Option<DroneMiningIntent>,
    pub cycle_high_watermark: u64,
    pub cycle: Option<DroneCargoCycleClaim>,
}
impl DroneOreHold {
    pub fn reserved_incoming(&self) -> Option<u64> {
        self.ore_units
            .checked_add(self.cycle.as_ref().map_or(0, |c| c.reserved_units))
    }
    pub fn valid(&self) -> bool {
        self.drone_id > 0
            && (self.destroyed_at_ms.is_none()
                || (self.ore_units == 0 && self.cycle.is_none() && self.mining_intent.is_none()))
            && self.mining_intent.as_ref().is_none_or(|intent| {
                intent.generation > 0
                    && intent.generation == self.intent_generation
                    && intent.target_id > 0
            })
            && self
                .reserved_incoming()
                .is_some_and(|v| v <= MINING_DRONE_CARGO_CAPACITY)
            && self.cycle.as_ref().is_none_or(|c| {
                self.mining_intent.as_ref().is_some_and(|intent| {
                    c.intent_generation == intent.generation
                        && c.target_id == intent.target_id
                        && c.job_id == intent.job_id
                }) && c.cycle_id > 0
                    && c.cycle_id == self.cycle_high_watermark
                    && c.target_id > 0
                    && (1..=DRONE_MINING_YIELD).contains(&c.reserved_units)
                    && c.started_at_ms.checked_add(DRONE_MINING_CYCLE_MS) == Some(c.completes_at_ms)
            })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneDropoffPhase {
    Queued,
    Approaching,
    Aligning,
    Coupling,
    Transferring,
    Decoupling,
    Egress,
    Held,
    Completed,
    Cancelled,
    Destroyed,
}
impl DroneDropoffPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Destroyed)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Queued for ore receiver",
            Self::Approaching => "Approaching ore receiver",
            Self::Aligning => "Aligning for ore transfer",
            Self::Coupling => "Connecting ore coupler",
            Self::Transferring => "Transferring confirmed ore",
            Self::Decoupling => "Disconnecting ore coupler",
            Self::Egress => "Clearing ore receiver",
            Self::Held => "Ore drop-off held",
            Self::Completed => "Ore drop-off complete",
            Self::Cancelled => "Ore drop-off cancelled",
            Self::Destroyed => "Drone destroyed",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DroneDropoffFollowup {
    Hold,
    ResumeMining {
        target_id: u64,
        job_id: Option<u64>,
        intent_generation: u64,
    },
    RecallAll {
        batch_id: u64,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneOreTransferReceipt {
    pub sequence: u64,
    pub units: u64,
    pub committed_at_ms: u64,
    pub ledger_sequence: u64,
    pub ledger_hash: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneDropoffOperation {
    pub operation_id: u64,
    pub binding: DroneCargoBinding,
    pub drone_id: u64,
    /// Original request intent for exact retry comparison, not current mode.
    pub requested_resume_mining: bool,
    pub cancel_requested: bool,
    pub followup: DroneDropoffFollowup,
    pub requested_units: u64,
    pub committed_units: u64,
    /// Historical untransferred amount; actual ownership exists only in the hold.
    pub remaining_units: u64,
    pub phase: DroneDropoffPhase,
    pub held_phase: Option<DroneDropoffPhase>,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
    pub next_batch_at_ms: Option<u64>,
    pub message: String,
    pub receipts: Vec<DroneOreTransferReceipt>,
}
impl DroneDropoffOperation {
    pub fn active(&self) -> bool {
        !self.phase.terminal()
    }
    pub fn valid(&self, binding: &DroneCargoBinding) -> bool {
        if self.operation_id == 0
            || self.drone_id == 0
            || &self.binding != binding
            || !(1..=MINING_DRONE_CARGO_CAPACITY).contains(&self.requested_units)
            || self.committed_units.checked_add(self.remaining_units) != Some(self.requested_units)
            || self.updated_at_ms < self.accepted_at_ms
            || (self.phase == DroneDropoffPhase::Completed && self.remaining_units != 0)
            || (self.phase == DroneDropoffPhase::Held) != self.held_phase.is_some()
            || self
                .held_phase
                .is_some_and(|p| p == DroneDropoffPhase::Held || p.terminal())
        {
            return false;
        }
        let mut units = 0u64;
        let mut ledger = 0;
        let mut time: Option<u64> = None;
        for (i, r) in self.receipts.iter().enumerate() {
            if r.sequence != i as u64 + 1
                || !(1..=DRONE_DROPOFF_BATCH_UNITS).contains(&r.units)
                || r.ledger_sequence <= ledger
                || !hash_valid(&r.ledger_hash)
                || self
                    .accepted_at_ms
                    .checked_add(DRONE_DROPOFF_BATCH_MS)
                    .is_none_or(|earliest| r.committed_at_ms < earliest)
                || r.committed_at_ms > self.updated_at_ms
                || time.is_some_and(|previous| {
                    previous
                        .checked_add(DRONE_DROPOFF_BATCH_MS)
                        .is_none_or(|earliest| r.committed_at_ms < earliest)
                })
            {
                return false;
            }
            let Some(sum) = units.checked_add(r.units) else {
                return false;
            };
            units = sum;
            ledger = r.ledger_sequence;
            time = Some(r.committed_at_ms);
        }
        units == self.committed_units
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneCargoState {
    pub schema_version: u32,
    pub binding: DroneCargoBinding,
    pub initialized_at_ms: u64,
    /// Exactly the original five Mining identities, even after destruction.
    pub holds: Vec<DroneOreHold>,
    pub lost_ore_units: u64,
    pub high_watermark: u64,
    pub operations: Vec<DroneDropoffOperation>,
    pub receiver_operation: Option<u64>,
    pub cleanup: Option<DroneBayCleanupView>,
}
impl DroneCargoState {
    pub fn validated_next_id(&self) -> Option<u64> {
        self.valid_shape().then_some(())?;
        self.high_watermark.checked_add(1)
    }
    pub fn operation(&self, id: u64) -> Option<&DroneDropoffOperation> {
        self.operations.iter().find(|o| o.operation_id == id)
    }
    pub fn reserved_incoming(&self) -> Option<u64> {
        self.holds
            .iter()
            .try_fold(0u64, |sum, h| sum.checked_add(h.reserved_incoming()?))
    }
    pub fn ore_units(&self) -> Option<u64> {
        self.holds
            .iter()
            .try_fold(0u64, |sum, h| sum.checked_add(h.ore_units))
    }
    pub fn valid_shape(&self) -> bool {
        if self.schema_version != DRONE_CARGO_SCHEMA_VERSION
            || !self.binding.well_formed()
            || self.holds.len() != DRONE_CLASS_CAP
            || self.high_watermark != self.operations.len() as u64
            || self.lost_ore_units > MINING_DRONE_CARGO_CAPACITY * DRONE_CLASS_CAP as u64
        {
            return false;
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut live = std::collections::BTreeSet::new();
        if !self
            .holds
            .iter()
            .all(|h| h.valid() && ids.insert(h.drone_id))
        {
            return false;
        }
        for (i, o) in self.operations.iter().enumerate() {
            if o.operation_id != i as u64 + 1
                || !o.valid(&self.binding)
                || !ids.contains(&o.drone_id)
                || (o.active() && !live.insert(o.drone_id))
            {
                return false;
            }
        }
        if self.operations.iter().filter(|o| o.active()).any(|o| {
            let queued = o.phase == DroneDropoffPhase::Queued
                || (o.phase == DroneDropoffPhase::Held
                    && o.held_phase == Some(DroneDropoffPhase::Queued));
            !queued && self.receiver_operation != Some(o.operation_id)
        }) {
            return false;
        }
        if let Some(id) = self.receiver_operation {
            if !self.operation(id).is_some_and(|o| {
                o.active()
                    && o.phase != DroneDropoffPhase::Queued
                    && o.held_phase != Some(DroneDropoffPhase::Queued)
            }) {
                return false;
            }
        }
        if let Some(c) = &self.cleanup {
            if self.receiver_operation.is_some()
                || !self.operation(c.operation_id).is_some_and(|o| {
                    matches!(
                        o.phase,
                        DroneDropoffPhase::Cancelled | DroneDropoffPhase::Destroyed
                    )
                })
            {
                return false;
            }
        }
        true
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DroneCargoRequest {
    BeginDropoff {
        binding: DroneCargoBinding,
        operation_id: u64,
        drone_id: u64,
        resume_mining: bool,
    },
    Hold {
        binding: DroneCargoBinding,
        operation_id: u64,
    },
    Resume {
        binding: DroneCargoBinding,
        operation_id: u64,
    },
    Cancel {
        binding: DroneCargoBinding,
        operation_id: u64,
    },
}
impl DroneCargoRequest {
    pub fn binding(&self) -> &DroneCargoBinding {
        match self {
            Self::BeginDropoff { binding, .. }
            | Self::Hold { binding, .. }
            | Self::Resume { binding, .. }
            | Self::Cancel { binding, .. } => binding,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneOreHoldView {
    pub drone_id: u64,
    pub ore_units: u64,
    pub capacity_units: u64,
    pub cycle_reserved_units: u64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneDropoffMotionView {
    pub operation_id: u64,
    pub drone_id: u64,
    pub frame: DroneBayMotionFrame,
    pub confirmed_pose: BayPose,
    pub segment: Option<BayMotionSegment>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DroneCargoPublicState {
    pub schema_version: u32,
    pub binding: DroneCargoBinding,
    pub holds: Vec<DroneOreHoldView>,
    pub incoming_reserved_units: u64,
    /// Bounded projection: active operations plus latest terminal result, not the journal.
    pub operations: Vec<DroneDropoffOperation>,
    pub receiver_operation: Option<u64>,
    pub cleanup: Option<DroneBayCleanupView>,
    pub motion: Option<DroneDropoffMotionView>,
    pub groups: Vec<DroneBayMechanismView>,
}
impl DroneCargoPublicState {
    pub fn operation(&self, id: u64) -> Option<&DroneDropoffOperation> {
        self.operations.iter().find(|o| o.operation_id == id)
    }
    /// Presentation only; accepted endpoints cannot transfer ore or complete service.
    pub fn sample_motion(&self, now: u64) -> Option<(BayPose, [f64; 3])> {
        let motion = self.motion.as_ref()?;
        let operation = self.operation(motion.operation_id)?;
        if operation.phase == DroneDropoffPhase::Held || motion.segment.is_none() {
            return valid_pose(motion.confirmed_pose).then_some((motion.confirmed_pose, [0.; 3]));
        }
        motion.segment?.sample(now)
    }
    pub fn sample_group(&self, semantic_id: &str, now: u64) -> Option<BayPose> {
        let group = self.groups.iter().find(|g| g.semantic_id == semantic_id)?;
        let held = self.cleanup.as_ref().is_some_and(|c| c.held)
            || self
                .receiver_operation
                .and_then(|id| self.operation(id))
                .is_some_and(|o| o.phase == DroneDropoffPhase::Held);
        if held {
            valid_pose(group.confirmed_pose).then_some(group.confirmed_pose)
        } else {
            group.sample(now)
        }
    }
    pub fn available_for_new_ore(&self, ship_ore: u64, capacity: u64) -> Option<u64> {
        capacity
            .checked_sub(ship_ore)?
            .checked_sub(self.incoming_reserved_units)
    }
    /// Private data is usable only after the same public vessel/roster was
    /// validated. This cannot prove transport ownership; the caller must also
    /// require its own authenticated pilot, not a remote viewed worker.
    pub fn validated_next_id(
        &self,
        private: &DroneCargoState,
        pilot: &crate::PilotView,
    ) -> Option<u64> {
        self.validate_projection(pilot).ok()?;
        if private.binding != self.binding
            || private.receiver_operation != self.receiver_operation
            || private.cleanup != self.cleanup
            || private.reserved_incoming()? != self.incoming_reserved_units
            || self
                .operations
                .iter()
                .any(|op| private.operation(op.operation_id) != Some(op))
            || self.holds.iter().any(|hold| {
                !private.holds.iter().any(|p| {
                    p.drone_id == hold.drone_id
                        && p.ore_units == hold.ore_units
                        && p.cycle.as_ref().map_or(0, |c| c.reserved_units)
                            == hold.cycle_reserved_units
                })
            })
        {
            return None;
        }
        private.validated_next_id()
    }
    /// Validate against the actual viewed vessel and compiled source, never an
    /// owner-private counter applied to a remote-controlled worker's public body.
    pub fn validate_projection(&self, pilot: &crate::PilotView) -> Result<(), String> {
        use crate::{
            drone_bay_definition::official_bay_for_binding, drone_dropoff_geometry as receiver,
            DroneBayPresence, DroneClass, ShipClass,
        };
        let fail = |message: &str| Err(message.to_owned());
        let Some(host) = pilot.flight.as_ref().and_then(|f| f.docking.as_ref()) else {
            return fail("Ore receiver requires a registered physical vessel");
        };
        if self.schema_version != DRONE_CARGO_SCHEMA_VERSION
            || pilot.ship_class != ShipClass::Pioneer
            || host.version != crate::DOCKING_VERSION
            || !self.binding.well_formed()
            || self.binding
                != supported_drone_cargo_registration().bind(host.host_vessel_id.clone())
        {
            return fail("Unsupported ore receiver source or parent vessel");
        }
        let Some(stock) = &pilot.drone_bay else {
            return fail("Physical stock missing");
        };
        if stock.schema_version != crate::DRONE_BAY_SCHEMA_VERSION
            || stock.batch_capability != Some(crate::DRONE_BAY_BATCH_VERSION)
        {
            return fail("Ore receiver requires fresh batch-capable stock");
        }
        let expected = self
            .binding
            .registration
            .base
            .bind(self.binding.vessel_id.clone());
        let definition = official_bay_for_binding(&expected)
            .ok_or_else(|| "Unsupported ore receiver bay definition".to_owned())?;
        stock.validate_projection(
            &expected,
            &definition.home_poses(),
            &definition.mechanism_ids(),
            &pilot.drones,
        )?;
        if self.holds.len() != DRONE_CLASS_CAP {
            return fail("Finite Mining cargo roster missing");
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut incoming = 0u64;
        for hold in &self.holds {
            let Some(entry) = stock.cradles.iter().find(|s| s.drone_id == hold.drone_id) else {
                return fail("Cargo names an unowned drone");
            };
            if !ids.insert(hold.drone_id)
                || entry.class != DroneClass::Mining
                || hold.capacity_units != MINING_DRONE_CARGO_CAPACITY
                || hold.cycle_reserved_units > DRONE_MINING_YIELD
                || hold
                    .ore_units
                    .checked_add(hold.cycle_reserved_units)
                    .is_none_or(|v| v > MINING_DRONE_CARGO_CAPACITY)
                || (entry.presence != DroneBayPresence::Active
                    && (hold.ore_units != 0 || hold.cycle_reserved_units != 0))
            {
                return fail("Cargo conflicts with finite stock or capacity");
            }
            incoming = incoming
                .checked_add(hold.ore_units)
                .and_then(|v| v.checked_add(hold.cycle_reserved_units))
                .ok_or("Cargo total overflow")?;
        }
        if incoming != self.incoming_reserved_units
            || self
                .available_for_new_ore(pilot.ore, crate::cargo_capacity(pilot.ship_class))
                .is_none()
        {
            return fail("Incoming ore exceeds reserved ship headroom");
        }
        let mut operation_ids = std::collections::BTreeSet::new();
        let mut active_drones = std::collections::BTreeSet::new();
        if self.operations.len() > DRONE_CLASS_CAP + 2 {
            return fail("Unbounded receiver projection");
        }
        for op in &self.operations {
            if !op.valid(&self.binding)
                || !operation_ids.insert(op.operation_id)
                || !ids.contains(&op.drone_id)
            {
                return fail("Invalid receiver operation receipt");
            }
            if op.active() {
                let hold = self
                    .holds
                    .iter()
                    .find(|h| h.drone_id == op.drone_id)
                    .unwrap();
                let entry = stock
                    .cradles
                    .iter()
                    .find(|c| c.drone_id == op.drone_id)
                    .unwrap();
                if !active_drones.insert(op.drone_id)
                    || entry.presence != DroneBayPresence::Active
                    || hold.ore_units != op.remaining_units
                    || hold.cycle_reserved_units != 0
                {
                    return fail("Active receiver operation does not own its actual payload");
                }
                let queued = op.phase == DroneDropoffPhase::Queued
                    || (op.phase == DroneDropoffPhase::Held
                        && op.held_phase == Some(DroneDropoffPhase::Queued));
                if !queued && self.receiver_operation != Some(op.operation_id) {
                    return fail("Constrained operation lacks receiver ownership");
                }
            }
        }
        let receiver_op = self.receiver_operation.and_then(|id| self.operation(id));
        if self.receiver_operation.is_some()
            && receiver_op.is_none_or(|o| {
                !o.active()
                    || o.phase == DroneDropoffPhase::Queued
                    || o.held_phase == Some(DroneDropoffPhase::Queued)
            })
        {
            return fail("Receiver reservation is invalid");
        }
        let cleanup_op = self
            .cleanup
            .as_ref()
            .and_then(|c| self.operation(c.operation_id));
        if self.cleanup.is_some()
            && (self.receiver_operation.is_some()
                || cleanup_op.is_none_or(|o| {
                    !matches!(
                        o.phase,
                        DroneDropoffPhase::Cancelled | DroneDropoffPhase::Destroyed
                    )
                }))
        {
            return fail("Receiver cleanup lost its retained terminal receipt");
        }
        if (self.receiver_operation.is_some() || self.cleanup.is_some())
            && (stock.machinery.handling_operation.is_some() || stock.machinery.cleanup.is_some())
        {
            return fail("Storage and receiver handling cannot run concurrently");
        }
        let held = receiver_op.is_some_and(|o| o.phase == DroneDropoffPhase::Held)
            || self.cleanup.as_ref().is_some_and(|c| c.held);
        match (&self.motion, receiver_op) {
            (Some(m), Some(op))
                if m.operation_id == op.operation_id
                    && m.drone_id == op.drone_id
                    && m.frame == DroneBayMotionFrame::Body
                    && valid_pose(m.confirmed_pose)
                    && m.segment.is_none_or(valid_segment)
                    && (!held || m.segment.is_none())
                    && !stock
                        .motions
                        .iter()
                        .any(|other| other.drone_id == m.drone_id) => {}
            (None, None) => {}
            _ => return fail("Receiver motion conflicts with operation, hold or storage route"),
        }
        let expected_groups = receiver::mechanism_ids();
        if self.groups.len() != expected_groups.len() {
            return fail("Receiver machinery groups missing");
        }
        let mut roots = Vec::new();
        let mut segments = Vec::new();
        for expected_group in expected_groups {
            let matches: Vec<_> = self
                .groups
                .iter()
                .filter(|g| g.semantic_id == expected_group.semantic_id())
                .collect();
            if matches.len() != 1 {
                return fail("Receiver machinery group duplicated or unsupported");
            }
            let g = matches[0];
            if !valid_pose(g.confirmed_pose)
                || g.segment.is_some_and(|s| !valid_segment(s))
                || (held && g.segment.is_some())
            {
                return fail("Invalid or moving held receiver machinery");
            }
            roots.push(g.confirmed_pose);
            segments.push(g.segment);
        }
        if !valid_dropoff_mechanism_roots(&roots) {
            return fail("Receiver roots violate the registered kinematics");
        }
        if segments.iter().any(Option::is_some) {
            if segments.iter().any(Option::is_none) {
                return fail("Receiver machinery must share one clock");
            }
            let first = segments[0].unwrap();
            if segments.iter().flatten().any(|s| {
                s.started_at_ms != first.started_at_ms || s.duration_ms != first.duration_ms
            }) {
                return fail("Receiver machinery clocks disagree");
            }
            for time in [
                first.started_at_ms,
                first.started_at_ms + first.duration_ms / 2,
                first.started_at_ms + first.duration_ms,
            ] {
                let sampled: Vec<_> = segments
                    .iter()
                    .map(|s| s.unwrap().sample(time).unwrap().0)
                    .collect();
                if !valid_dropoff_mechanism_roots(&sampled) {
                    return fail("Receiver machinery segment violates its interlock");
                }
            }
        }
        if let Some(m) = &self.motion {
            let clock = |s: Option<BayMotionSegment>| s.map(|s| (s.started_at_ms, s.duration_ms));
            if clock(m.segment) != clock(segments[0]) {
                return fail("Receiver drone and machinery clocks disagree");
            }
        }
        if receiver_op.is_some_and(|o| {
            o.phase == DroneDropoffPhase::Transferring
                || o.held_phase == Some(DroneDropoffPhase::Transferring)
        }) && (roots
            != receiver::mechanism_poses(receiver::DropoffConfiguration::COUPLED).unwrap()
            || segments.iter().any(Option::is_some)
            || self.motion.as_ref().is_none_or(|m| {
                m.frame != DroneBayMotionFrame::Body
                    || m.confirmed_pose != receiver::coupled_pose()
                    || m.segment.is_some()
            }))
        {
            return fail("Transfer phase lacks confirmed stationary physical coupling");
        }
        if self.receiver_operation.is_none()
            && self.cleanup.is_none()
            && (roots != receiver::reference_poses() || segments.iter().any(Option::is_some))
        {
            return fail("Unowned receiver hardware is not safely stowed");
        }
        if let Some(batch) = stock.batch.as_ref().filter(|b| b.active()) {
            if let Some(id) = batch.current_service_operation_id {
                let Some(op) = self.operation(id) else {
                    return fail("Recall service receipt missing");
                };
                if op.followup
                    != (DroneDropoffFollowup::RecallAll {
                        batch_id: batch.batch_id,
                    })
                    || (!(op.phase == DroneDropoffPhase::Queued
                        || op.held_phase == Some(DroneDropoffPhase::Queued))
                        && self
                            .receiver_operation
                            .or_else(|| self.cleanup.as_ref().map(|c| c.operation_id))
                            != Some(id))
                    || !batch.members.iter().any(|m| {
                        m.drone_id == op.drone_id && m.service_operation_ids.last() == Some(&id)
                    })
                {
                    return fail("Recall service does not own the receiver");
                }
            }
        }
        Ok(())
    }
}
fn valid_pose(pose: BayPose) -> bool {
    pose.valid()
        && pose
            .position_m
            .iter()
            .all(|v| v.abs() <= crate::SPACE_BOUND)
}
fn valid_segment(s: BayMotionSegment) -> bool {
    valid_pose(s.source)
        && valid_pose(s.target)
        && s.started_at_ms
            .checked_add(s.duration_ms)
            .is_some_and(|end| {
                [s.started_at_ms, s.started_at_ms + s.duration_ms / 2, end]
                    .into_iter()
                    .all(|time| s.sample(time).is_some())
            })
}
/// Validate exact source-bound receiver group kinematics, including the latch interlock.
pub fn valid_dropoff_mechanism_roots(roots: &[BayPose]) -> bool {
    use crate::drone_dropoff_geometry as g;
    if roots.len() != g::DROPOFF_MECHANISM_COUNT {
        return false;
    }
    let extension = g::reference_pose().position_m[0] - roots[2].position_m[0];
    let latch = (g::reference_pose().position_m[2] - roots[3].position_m[2]) / 0.03;
    // The source transform reconstructs the same values with normal f64 rounding.
    let snap = |v: f64| {
        if v.abs() < 1e-10 {
            0.
        } else if (v - 1.).abs() < 1e-10 {
            1.
        } else {
            v
        }
    };
    let Some(expected) = g::mechanism_poses(g::DropoffConfiguration {
        extension: snap(extension),
        latch_closed: snap(latch),
    }) else {
        return false;
    };
    roots.iter().zip(expected).all(|(actual, expected)| {
        actual.orientation_xyzw == expected.orientation_xyzw
            && actual
                .position_m
                .iter()
                .zip(expected.position_m)
                .all(|(a, b)| (a - b).abs() < 1e-9)
    })
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneOreTotals {
    pub schema_version: u32,
    pub drone_ore_units: u64,
    pub lost_ore_units: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registration() -> DroneCargoRegistration {
        DroneCargoRegistration {
            base: DroneBayRegistration {
                geometry_revision: 12,
                source_sha256: "a".repeat(64),
            },
            receiver_revision: 1,
            receiver_source_sha256: "b".repeat(64),
        }
    }
    #[test]
    fn receiver_binding_pins_both_sources_and_actual_vessel() {
        let r = registration();
        assert!(r.well_formed());
        let b = r.bind("actual-vessel".into());
        assert!(b.well_formed());
        let mut bad = b.clone();
        bad.registration.receiver_source_sha256.clear();
        assert!(!bad.well_formed());
        assert_ne!(b, r.bind("other-vessel".into()));
    }
    #[test]
    fn cycle_claim_is_capacity_not_second_ore_balance() {
        let mut h = DroneOreHold {
            drone_id: 6,
            ore_units: 90,
            destroyed_at_ms: None,
            intent_generation: 1,
            mining_intent: Some(DroneMiningIntent {
                generation: 1,
                target_id: 20,
                job_id: None,
            }),
            cycle_high_watermark: 1,
            cycle: Some(DroneCargoCycleClaim {
                cycle_id: 1,
                target_id: 20,
                reserved_units: 10,
                started_at_ms: 1000,
                completes_at_ms: 9000,
                job_id: None,
                intent_generation: 1,
            }),
        };
        assert!(h.valid());
        assert_eq!(h.reserved_incoming(), Some(100));
        assert_eq!(h.ore_units, 90);
        h.ore_units = 91;
        assert!(!h.valid());
        h.ore_units = 90;
        h.cycle.as_mut().unwrap().cycle_id = 2;
        assert!(!h.valid());
    }
    #[test]
    fn empty_initialized_stock_is_finite_and_requests_roundtrip() {
        let b = registration().bind("actual-vessel".into());
        let s = DroneCargoState {
            schema_version: 1,
            binding: b.clone(),
            initialized_at_ms: 1000,
            holds: (6..11)
                .map(|drone_id| DroneOreHold {
                    drone_id,
                    ore_units: 0,
                    destroyed_at_ms: None,
                    intent_generation: 0,
                    mining_intent: None,
                    cycle_high_watermark: 0,
                    cycle: None,
                })
                .collect(),
            lost_ore_units: 0,
            high_watermark: 0,
            operations: vec![],
            receiver_operation: None,
            cleanup: None,
        };
        assert!(s.valid_shape());
        let mut bad = s.clone();
        bad.holds[1].drone_id = 6;
        assert!(!bad.valid_shape());
        for request in [
            DroneCargoRequest::BeginDropoff {
                binding: b.clone(),
                operation_id: 1,
                drone_id: 6,
                resume_mining: true,
            },
            DroneCargoRequest::Hold {
                binding: b.clone(),
                operation_id: 1,
            },
            DroneCargoRequest::Resume {
                binding: b.clone(),
                operation_id: 1,
            },
            DroneCargoRequest::Cancel {
                binding: b,
                operation_id: 1,
            },
        ] {
            let envelope = crate::Request::DroneCargo {
                request: request.clone(),
            };
            let decoded: crate::Request =
                serde_json::from_slice(&serde_json::to_vec(&envelope).unwrap()).unwrap();
            assert!(
                matches!(decoded,crate::Request::DroneCargo{request: ref value} if value==&request)
            );
            assert_eq!(
                serde_json::from_value::<DroneCargoRequest>(
                    serde_json::to_value(&request).unwrap()
                )
                .unwrap(),
                request
            );
        }
    }
    fn public_fixture() -> (crate::PilotView, DroneCargoPublicState) {
        use crate::{drone_bay_layout as bay, drone_dropoff_geometry as receiver, *};
        let binding = supported_drone_cargo_registration().bind("fixture-vessel".into());
        let mut pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"Pilot","credits":0,"ore":0,"location":{"kind":"station"},
            "flight":{"position":[0,0,0],"velocity":[0,0,0],"heading":[1,0,0],
                "mode":{"mode":"stop"},"updated_at_ms":1000,
                "docking":{"version":1,"host_vessel_id":"fixture-vessel","berth_id":0,
                "operation_id":0,"phase":"free","segment":null,"clamps":0.,
                "updated_at_ms":1000,"hold_reason":null,"departing":false}}
        }))
        .unwrap();
        pilot.drone_bay = Some(DroneBayPublicState {
            schema_version: DRONE_BAY_SCHEMA_VERSION,
            batch_capability: Some(DRONE_BAY_BATCH_VERSION),
            batch: None,
            binding: binding.registration.base.bind(binding.vessel_id.clone()),
            cradles: bay::home_poses()
                .into_iter()
                .enumerate()
                .map(|(i, pose)| DroneCradleView {
                    drone_id: i as u64 + 1,
                    home_cradle: i as u8,
                    class: DroneBayState::expected_class(i as u8).unwrap(),
                    hit_points: 100,
                    presence: DroneBayPresence::Stored,
                    body_pose: pose,
                })
                .collect(),
            motions: vec![],
            machinery: DroneBayMachineryView {
                cleanup: None,
                handling_operation: None,
                door_fraction: 0.,
                door_segment: None,
                groups: bay::mechanism_ids()
                    .into_iter()
                    .zip(bay::reference_poses())
                    .map(|(id, pose)| DroneBayMechanismView {
                        semantic_id: id.into(),
                        confirmed_pose: pose,
                        segment: None,
                    })
                    .collect(),
            },
        });
        let public = DroneCargoPublicState {
            schema_version: 1,
            binding,
            holds: (6..11)
                .map(|drone_id| DroneOreHoldView {
                    drone_id,
                    ore_units: 0,
                    capacity_units: 100,
                    cycle_reserved_units: 0,
                })
                .collect(),
            incoming_reserved_units: 0,
            operations: vec![],
            receiver_operation: None,
            cleanup: None,
            motion: None,
            groups: receiver::mechanism_ids()
                .into_iter()
                .zip(receiver::reference_poses())
                .map(|(id, pose)| DroneBayMechanismView {
                    semantic_id: id.semantic_id().into(),
                    confirmed_pose: pose,
                    segment: None,
                })
                .collect(),
        };
        (pilot, public)
    }
    fn activate(pilot: &mut crate::PilotView, id: u64) {
        pilot
            .drone_bay
            .as_mut()
            .unwrap()
            .cradles
            .iter_mut()
            .find(|c| c.drone_id == id)
            .unwrap()
            .presence = crate::DroneBayPresence::Active;
        pilot.drones.push(crate::DroneView {
            orientation_xyzw: Some([0., 0., 0., 1.]),
            id,
            class: crate::DroneClass::Mining,
            hit_points: 100,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [100.; 3],
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            mode: crate::DroneMode::Hold,
            updated_at_ms: 1000,
        });
    }
    fn queued(binding: DroneCargoBinding) -> DroneDropoffOperation {
        DroneDropoffOperation {
            operation_id: 1,
            binding,
            drone_id: 6,
            requested_resume_mining: false,
            cancel_requested: false,
            followup: DroneDropoffFollowup::Hold,
            requested_units: 50,
            committed_units: 0,
            remaining_units: 50,
            phase: DroneDropoffPhase::Queued,
            held_phase: None,
            accepted_at_ms: 1000,
            updated_at_ms: 1000,
            next_batch_at_ms: None,
            message: "Queued".into(),
            receipts: vec![],
        }
    }
    #[test]
    fn public_projection_checks_source_owner_stock_headroom_and_machinery() {
        let (pilot, public) = public_fixture();
        assert!(public.validate_projection(&pilot).is_ok());
        for case in [
            "hash",
            "vessel",
            "duplicate",
            "capacity",
            "stored-ore",
            "claim",
            "total",
            "group",
            "root",
            "clock",
            "schema",
        ] {
            let mut p = pilot.clone();
            let mut c = public.clone();
            match case {
                "hash" => c.binding.registration.receiver_source_sha256 = "a".repeat(64),
                "vessel" => c.binding.vessel_id = "foreign".into(),
                "duplicate" => c.holds[1].drone_id = 6,
                "capacity" => c.holds[0].capacity_units = 1000,
                "stored-ore" => {
                    c.holds[0].ore_units = 1;
                    c.incoming_reserved_units = 1;
                }
                "claim" => {
                    activate(&mut p, 6);
                    c.holds[0].cycle_reserved_units = 11;
                    c.incoming_reserved_units = 11;
                }
                "total" => c.incoming_reserved_units = 1,
                "group" => c.groups[0].semantic_id = "unknown".into(),
                "root" => c.groups[0].confirmed_pose.position_m[1] += 0.1,
                "clock" => {
                    c.groups[0].segment = Some(BayMotionSegment {
                        source: c.groups[0].confirmed_pose,
                        target: c.groups[0].confirmed_pose,
                        started_at_ms: 1000,
                        duration_ms: 500,
                    })
                }
                "schema" => c.schema_version = 99,
                _ => unreachable!(),
            }
            assert!(c.validate_projection(&p).is_err(), "{case}");
        }
        let mut p = pilot;
        let mut c = public;
        activate(&mut p, 6);
        c.holds[0].ore_units = 50;
        c.incoming_reserved_units = 50;
        p.ore = 950;
        assert!(c.validate_projection(&p).is_ok());
        assert_eq!(c.available_for_new_ore(p.ore, 1000), Some(0));
        p.ore = 951;
        assert!(c.validate_projection(&p).is_err());
    }
    #[test]
    fn held_projection_cannot_advance_drone_or_hardware_or_hide_payload() {
        let (mut p, mut c) = public_fixture();
        activate(&mut p, 6);
        c.holds[0].ore_units = 50;
        c.incoming_reserved_units = 50;
        c.operations.push(queued(c.binding.clone()));
        assert!(c.validate_projection(&p).is_ok());
        c.receiver_operation = Some(1);
        c.operations[0].phase = DroneDropoffPhase::Held;
        c.operations[0].held_phase = Some(DroneDropoffPhase::Approaching);
        let pose = crate::drone_dropoff_geometry::gate_pose();
        c.motion = Some(DroneDropoffMotionView {
            operation_id: 1,
            drone_id: 6,
            frame: DroneBayMotionFrame::Body,
            confirmed_pose: pose,
            segment: None,
        });
        assert!(c.validate_projection(&p).is_ok());
        c.motion.as_mut().unwrap().segment = Some(BayMotionSegment {
            source: pose,
            target: pose,
            started_at_ms: 1000,
            duration_ms: 500,
        });
        assert!(c.validate_projection(&p).is_err());
        c.motion.as_mut().unwrap().segment = None;
        c.holds[0].ore_units = 49;
        c.incoming_reserved_units = 49;
        assert!(c.validate_projection(&p).is_err());
    }
    #[test]
    fn committed_receipts_are_exact_and_cannot_accelerate() {
        let mut op = queued(supported_drone_cargo_registration().bind("vessel".into()));
        op.committed_units = 50;
        op.remaining_units = 0;
        op.updated_at_ms = 2000;
        op.phase = DroneDropoffPhase::Completed;
        op.receipts = vec![
            DroneOreTransferReceipt {
                sequence: 1,
                units: 25,
                committed_at_ms: 1500,
                ledger_sequence: 7,
                ledger_hash: "a".repeat(64),
            },
            DroneOreTransferReceipt {
                sequence: 2,
                units: 25,
                committed_at_ms: 2000,
                ledger_sequence: 9,
                ledger_hash: "b".repeat(64),
            },
        ];
        assert!(op.valid(&op.binding));
        op.receipts[1].committed_at_ms = 1999;
        assert!(!op.valid(&op.binding));
        op.receipts[1].committed_at_ms = 2000;
        op.receipts[1].ledger_sequence = 7;
        assert!(!op.valid(&op.binding));
    }
}

/// Economic evidence, not a proof of collision clearance or physical contact.
/// Fresh cargo worlds use ruleset2 for every event from genesis; legacy worlds
/// retain ruleset1 and cannot contain these variants.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DroneCargoLedgerEvent {
    Initialized {
        pilot: String,
        vessel_id: String,
        registration: DroneCargoRegistration,
        drone_ids: Vec<u64>,
        capacity_units: u64,
    },
    Extracted {
        pilot: String,
        vessel_id: String,
        drone_id: u64,
        cycle_id: u64,
        target_id: u64,
        cycle_started_at_ms: u64,
        units: u64,
        job_id: Option<u64>,
    },
    Transferred {
        pilot: String,
        vessel_id: String,
        drone_id: u64,
        operation_id: u64,
        operation_started_at_ms: u64,
        requested_units: u64,
        sequence: u64,
        units: u64,
    },
    /// Emitted once even for an empty hold, so a destroyed identity cannot mine
    /// or deliver again. Zero lost_units is not a minted or transferred balance.
    Destroyed {
        pilot: String,
        vessel_id: String,
        drone_id: u64,
        destroyed_at_ms: u64,
        lost_units: u64,
    },
}

/// Honest job accounting: destruction does not rewrite historical extraction,
/// and transfers between drone and ship do not count as another mined yield.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneCargoJobProgress {
    pub historical_extracted_units: u64,
    pub available_units: u64,
    pub confirmed_delivered_units: u64,
    pub lost_units: u64,
}
