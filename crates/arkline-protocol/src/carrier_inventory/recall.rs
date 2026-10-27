//! Retained finite Recall All. Capture and protected relocation are distinct
//! physical children; buffers and loaders never become extra storage capacity.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierRecallPhase {
    Running,
    Cancelling,
    Completed,
    Cancelled,
}
impl CarrierRecallPhase {
    pub fn active(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierRecallMember {
    pub drone_id: u64,
    pub accepted_location: CarrierDroneLocation,
    /// Optional retained physical unloading prefix. Historical empty recalls
    /// omit this field byte-for-byte; a completed prefix is not stored status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_operation_id: Option<u64>,
    pub capture_operation_id: Option<u64>,
    pub store_operation_id: Option<u64>,
    pub cancelled: bool,
    pub blocked_reason: String,
    /// Immutable actual final location, recorded only when the whole queue settles.
    pub completed_location: Option<CarrierDroneLocation>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierRecallBatch {
    pub batch_id: u64,
    pub members: Vec<CarrierRecallMember>,
    pub phase: CarrierRecallPhase,
    pub held: bool,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
}
impl CarrierInventoryState {
    pub fn active_recall(&self) -> Option<&CarrierRecallBatch> {
        self.recalls.iter().find(|b| b.phase.active())
    }
    pub fn recall_owner(&self, operation_id: u64) -> Option<&CarrierRecallBatch> {
        self.recalls.iter().find(|b| {
            b.phase.active()
                && b.members.iter().any(|m| {
                    m.service_operation_id == Some(operation_id)
                        || m.capture_operation_id == Some(operation_id)
                        || m.store_operation_id == Some(operation_id)
                })
        })
    }
    pub fn next_recall_id(&self) -> Option<u64> {
        self.recall_high_watermark.checked_add(1)
    }
    pub(super) fn validate_recalls(&self) -> bool {
        if self.recall_high_watermark != self.recalls.len() as u64
            || self.recalls.iter().filter(|b| b.phase.active()).count() > 1
            || (self.active_recall().is_some() && self.active_deployment().is_some())
        {
            return false;
        }
        let mut children = BTreeSet::new();
        for d in &self.deployments {
            for m in &d.members {
                children.extend(
                    [m.stage_operation_id, m.launch_operation_id]
                        .into_iter()
                        .flatten(),
                );
            }
        }
        for (i, b) in self.recalls.iter().enumerate() {
            if b.batch_id != i as u64 + 1
                || b.members.is_empty()
                || b.accepted_at_ms < self.initialized_at_ms
                || b.updated_at_ms < b.accepted_at_ms
                || (!b.phase.active() && b.held)
            {
                return false;
            }
            let mut ids = BTreeSet::new();
            for m in &b.members {
                if !ids.insert(m.drone_id)
                    || self.entry(m.drone_id).is_none()
                    || m.blocked_reason.len() > 512
                    || !matches!(
                        m.accepted_location,
                        CarrierDroneLocation::Protected { .. }
                            | CarrierDroneLocation::Ready { .. }
                            | CarrierDroneLocation::Deployed
                    )
                {
                    return false;
                }
                for (id, kind) in [
                    (m.service_operation_id, 0),
                    (m.capture_operation_id, 1),
                    (m.store_operation_id, 2),
                ] {
                    if let Some(id) = id {
                        let Some(op) = self.operation(id) else {
                            return false;
                        };
                        if !children.insert(id)
                            || op.drone_id != m.drone_id
                            || op.recall_batch_id.is_some_and(|id| id != b.batch_id)
                            || op.accepted_at_ms < b.accepted_at_ms
                            || (!b.phase.active() && op.updated_at_ms > b.updated_at_ms)
                            || (kind == 0
                                && (self.binding.registration.assembly_revision != 1703
                                    || op.action
                                        != CarrierInventoryAction::ReturnToUnload {
                                            resume_mining: false,
                                        }
                                    || m.accepted_location != CarrierDroneLocation::Deployed))
                            || (kind == 1
                                && (op.action != CarrierInventoryAction::RecallToStorage
                                    || m.accepted_location != CarrierDroneLocation::Deployed))
                            || (kind == 2
                                && !matches!(op.action, CarrierInventoryAction::StoreReady { .. }))
                        {
                            return false;
                        }
                        if m.cancelled && !op.phase.terminal() {
                            return false;
                        }
                        if !b.phase.active()
                            && (!op.phase.terminal()
                                || self.cleanups.iter().any(|c| c.operation_id == id))
                        {
                            return false;
                        }
                    }
                }
                // Completed physical unloading is immutable batch provenance,
                // not merely a live-control link. A later operation cannot enter
                // this closed lifetime: source service requires positive time.
                // A later same-clock manual operation may be destroyed before
                // moving; zero-duration destruction does not establish ownership.
                if self.operations.iter().any(|op| {
                    op.drone_id == m.drone_id
                        && op.action
                            == CarrierInventoryAction::ReturnToUnload {
                                resume_mining: false,
                            }
                        && matches!(
                            op.phase,
                            CarrierOperationPhase::Completed | CarrierOperationPhase::Destroyed
                        )
                        && op.accepted_at_ms >= b.accepted_at_ms
                        && op.updated_at_ms > op.accepted_at_ms
                        && op.updated_at_ms <= b.updated_at_ms
                        && m.service_operation_id != Some(op.operation_id)
                }) {
                    return false;
                }
                if let (Some(service), Some(capture)) =
                    (m.service_operation_id, m.capture_operation_id)
                {
                    let a = self.operation(service).unwrap();
                    let z = self.operation(capture).unwrap();
                    if service >= capture
                        || a.phase != CarrierOperationPhase::Completed
                        || a.updated_at_ms > z.accepted_at_ms
                    {
                        return false;
                    }
                }
                if let Some(store) = m.store_operation_id {
                    if let Some(capture) = m.capture_operation_id {
                        let a = self.operation(capture).unwrap();
                        let z = self.operation(store).unwrap();
                        if capture >= store
                            || a.phase != CarrierOperationPhase::Completed
                            || a.updated_at_ms > z.accepted_at_ms
                        {
                            return false;
                        }
                    } else {
                        let CarrierDroneLocation::Ready { berth_id } = m.accepted_location else {
                            return false;
                        };
                        let op = self.operation(store).unwrap();
                        if op.berth_id.is_some_and(|b| b != berth_id) {
                            return false;
                        }
                    }
                }
                if b.phase.active() {
                    if m.completed_location.is_some() {
                        return false;
                    }
                    let linked: Vec<_> = [
                        m.service_operation_id,
                        m.capture_operation_id,
                        m.store_operation_id,
                    ]
                    .into_iter()
                    .flatten()
                    .map(|id| self.operation(id).unwrap())
                    .collect();
                    let entry = self.entry(m.drone_id).unwrap();
                    if !linked.iter().any(|op| !op.phase.terminal())
                        && !matches!(entry.location, CarrierDroneLocation::Destroyed { .. })
                    {
                        let expected = linked
                            .iter()
                            .rev()
                            .find(|o| o.phase == CarrierOperationPhase::Completed)
                            .map(|op| match op.action {
                                CarrierInventoryAction::StoreReady { slot_id } => {
                                    Some(CarrierDroneLocation::Protected { slot_id })
                                }
                                CarrierInventoryAction::ReturnToUnload {
                                    resume_mining: false,
                                } => Some(CarrierDroneLocation::Deployed),
                                CarrierInventoryAction::RecallToStorage => op
                                    .berth_id
                                    .map(|berth_id| CarrierDroneLocation::Ready { berth_id }),
                                _ => None,
                            })
                            .unwrap_or_else(|| Some(m.accepted_location.clone()));
                        if expected.as_ref() != Some(&entry.location) {
                            return false;
                        }
                    }
                } else {
                    if b.phase == CarrierRecallPhase::Completed && m.cancelled {
                        return false;
                    }
                    if m.cancelled {
                        if m.completed_location.is_some() {
                            return false;
                        }
                        continue;
                    }
                    let Some(endpoint) = &m.completed_location else {
                        return false;
                    };
                    if matches!(endpoint, CarrierDroneLocation::Destroyed { .. }) {
                        if self
                            .entry(m.drone_id)
                            .is_none_or(|e| &e.location != endpoint)
                        {
                            return false;
                        }
                    } else {
                        let latest = [m.store_operation_id, m.capture_operation_id]
                            .into_iter()
                            .flatten()
                            .filter_map(|id| self.operation(id))
                            .find(|op| op.phase == CarrierOperationPhase::Completed);
                        let expected = match latest {
                            Some(op) if op.phase == CarrierOperationPhase::Completed => {
                                match op.action {
                                    CarrierInventoryAction::StoreReady { slot_id } => {
                                        CarrierDroneLocation::Protected { slot_id }
                                    }
                                    CarrierInventoryAction::RecallToStorage => {
                                        let Some(berth_id) = op.berth_id else {
                                            return false;
                                        };
                                        CarrierDroneLocation::Ready { berth_id }
                                    }
                                    _ => return false,
                                }
                            }
                            None => m.accepted_location.clone(),
                            _ => return false,
                        };
                        if *endpoint != expected || endpoint.dock().is_none() {
                            return false;
                        }
                    }
                }
            }
            if b.phase.active() {
                let linked: BTreeSet<_> = b
                    .members
                    .iter()
                    .flat_map(|m| {
                        [
                            m.service_operation_id,
                            m.capture_operation_id,
                            m.store_operation_id,
                        ]
                    })
                    .flatten()
                    .collect();
                if self
                    .operations
                    .iter()
                    .filter(|op| {
                        !op.phase.terminal()
                            || self
                                .cleanups
                                .iter()
                                .any(|c| c.operation_id == op.operation_id)
                    })
                    .any(|op| !linked.contains(&op.operation_id))
                {
                    return false;
                }
            }
        }
        // Typed provenance survives terminal batches and zero-duration death.
        // A manual operation remains owner=None even at an identical timestamp.
        if self.operations.iter().any(|op| {
            op.recall_batch_id.is_some_and(|batch_id| {
                !self.recalls.iter().any(|b| {
                    b.batch_id == batch_id
                        && b.members.iter().any(|m| {
                            m.drone_id == op.drone_id
                                && [
                                    m.service_operation_id,
                                    m.capture_operation_id,
                                    m.store_operation_id,
                                ]
                                .contains(&Some(op.operation_id))
                        })
                })
            })
        }) {
            return false;
        }
        true
    }
}
