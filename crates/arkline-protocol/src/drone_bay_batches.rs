//! Durable, serialized requests for existing physical bay operations. A batch
//! never creates stock, changes route timing, or captures a drone by itself.
use crate::{DroneBayBinding, DroneClass, MAX_DRONES};
use serde::{Deserialize, Serialize};

pub const DRONE_BAY_BATCH_VERSION: u32 = 1;
pub const DRONE_BAY_BATCH_CLIENT_CAPABILITY_VERSION: u32 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DroneBayBatchKind {
    DeployClass { class: DroneClass },
    RecallAll,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneBayBatchPhase {
    Running,
    /// Pending members are cancelled; an already accepted route/cleanup still
    /// owns the channel until it is safely finished. Held work remains resumable.
    Cancelling,
    Completed,
    Cancelled,
}
impl DroneBayBatchPhase {
    pub fn active(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneBayBatchMemberStatus {
    Pending,
    Active,
    /// Quick ore service precedes full storage; it never implies Stored.
    Servicing,
    Launched,
    Stored,
    Destroyed,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayBatchMember {
    pub drone_id: u64,
    pub home_cradle: u8,
    /// Retained quick-service IDs in their own namespace, never storage IDs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub service_operation_ids: Vec<u64>,
    pub operation_id: Option<u64>,
    pub status: DroneBayBatchMemberStatus,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayBatchOperation {
    pub batch_id: u64,
    pub binding: DroneBayBinding,
    pub kind: DroneBayBatchKind,
    pub phase: DroneBayBatchPhase,
    pub held: bool,
    /// Immutable acceptance roster. Results evolve; identities never do.
    pub members: Vec<DroneBayBatchMember>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_service_operation_id: Option<u64>,
    pub current_operation_id: Option<u64>,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
    pub message: String,
}
impl DroneBayBatchOperation {
    pub fn active(&self) -> bool {
        self.phase.active()
    }
    pub fn validate_shape(&self, binding: &DroneBayBinding) -> bool {
        if self.batch_id == 0
            || &self.binding != binding
            || self.members.is_empty()
            || self.members.len() > MAX_DRONES
            || self.updated_at_ms < self.accepted_at_ms
            || (!self.active()
                && (self.held
                    || self.current_operation_id.is_some()
                    || self.current_service_operation_id.is_some()))
            || (self.current_operation_id.is_some() && self.current_service_operation_id.is_some())
        {
            return false;
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut cradles = std::collections::BTreeSet::new();
        let mut operations = std::collections::BTreeSet::new();
        let mut services = std::collections::BTreeSet::new();
        let mut active = 0;
        for member in &self.members {
            if member.drone_id == 0
                || usize::from(member.home_cradle) >= MAX_DRONES
                || !ids.insert(member.drone_id)
                || !cradles.insert(member.home_cradle)
                || member
                    .operation_id
                    .is_some_and(|id| id == 0 || !operations.insert(id))
            {
                return false;
            }
            if member
                .service_operation_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
                || member
                    .service_operation_ids
                    .iter()
                    .any(|id| *id == 0 || !services.insert(*id))
                || (!member.service_operation_ids.is_empty()
                    && self.kind != DroneBayBatchKind::RecallAll)
            {
                return false;
            }
            match member.status {
                DroneBayBatchMemberStatus::Pending if member.operation_id.is_some() => {
                    return false
                }
                DroneBayBatchMemberStatus::Servicing => {
                    active += 1;
                    if member.operation_id.is_some()
                        || self.current_service_operation_id.is_none()
                        || member.service_operation_ids.last().copied()
                            != self.current_service_operation_id
                    {
                        return false;
                    }
                }
                DroneBayBatchMemberStatus::Active => {
                    active += 1;
                    if member.operation_id.is_none()
                        || member.operation_id != self.current_operation_id
                    {
                        return false;
                    }
                }
                DroneBayBatchMemberStatus::Launched | DroneBayBatchMemberStatus::Stored
                    if member.operation_id.is_none() =>
                {
                    return false
                }
                _ => {}
            }
            if !self.active()
                && matches!(
                    member.status,
                    DroneBayBatchMemberStatus::Pending
                        | DroneBayBatchMemberStatus::Active
                        | DroneBayBatchMemberStatus::Servicing
                )
            {
                return false;
            }
        }
        active <= 1
            && self.current_service_operation_id.is_none_or(|id| {
                self.members.iter().any(|m| {
                    m.service_operation_ids.last() == Some(&id)
                        && matches!(
                            m.status,
                            DroneBayBatchMemberStatus::Servicing
                                | DroneBayBatchMemberStatus::Destroyed
                        )
                })
            })
            && self.current_operation_id.is_none_or(|id| {
                self.members.iter().any(|m| {
                    m.operation_id == Some(id)
                        && matches!(
                            m.status,
                            DroneBayBatchMemberStatus::Active
                                | DroneBayBatchMemberStatus::Destroyed
                        )
                })
            })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneBayBatchJournal {
    pub version: u32,
    pub high_watermark: u64,
    /// No eviction in this preview: old batch IDs retain exact intent/results.
    pub operations: Vec<DroneBayBatchOperation>,
}
impl Default for DroneBayBatchJournal {
    fn default() -> Self {
        Self {
            version: DRONE_BAY_BATCH_VERSION,
            high_watermark: 0,
            operations: vec![],
        }
    }
}
impl DroneBayBatchJournal {
    pub fn validated_next_id(&self, binding: &DroneBayBinding) -> Option<u64> {
        if self.version != DRONE_BAY_BATCH_VERSION
            || self.high_watermark != self.operations.len() as u64
            || self
                .operations
                .iter()
                .enumerate()
                .any(|(i, b)| b.batch_id != i as u64 + 1 || !b.validate_shape(binding))
            || self
                .operations
                .iter()
                .take(self.operations.len().saturating_sub(1))
                .any(|b| b.active())
        {
            return None;
        }
        self.high_watermark.checked_add(1)
    }
    pub fn operation(&self, id: u64) -> Option<&DroneBayBatchOperation> {
        self.operations.iter().find(|b| b.batch_id == id)
    }
    pub fn active(&self) -> Option<&DroneBayBatchOperation> {
        self.operations.iter().find(|b| b.active())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DroneBayRequest;
    fn binding() -> DroneBayBinding {
        DroneBayBinding {
            vessel_id: "owned-vessel".into(),
            geometry_revision: 12,
            source_sha256: "a".repeat(64),
        }
    }
    fn operation() -> DroneBayBatchOperation {
        DroneBayBatchOperation {
            batch_id: 1,
            binding: binding(),
            kind: DroneBayBatchKind::RecallAll,
            phase: DroneBayBatchPhase::Running,
            held: false,
            members: vec![DroneBayBatchMember {
                drone_id: 8,
                home_cradle: 7,
                service_operation_ids: vec![],
                operation_id: None,
                status: DroneBayBatchMemberStatus::Pending,
            }],
            current_service_operation_id: None,
            current_operation_id: None,
            accepted_at_ms: 10,
            updated_at_ms: 10,
            message: "Queued".into(),
        }
    }
    #[test]
    fn batch_wire_roundtrip_and_checked_next_id() {
        for request in [
            DroneBayRequest::BeginBatch {
                binding: binding(),
                batch_id: 1,
                batch_kind: DroneBayBatchKind::RecallAll,
            },
            DroneBayRequest::BeginBatch {
                binding: binding(),
                batch_id: 1,
                batch_kind: DroneBayBatchKind::DeployClass {
                    class: DroneClass::Mining,
                },
            },
            DroneBayRequest::HoldBatch {
                binding: binding(),
                batch_id: 1,
            },
            DroneBayRequest::ResumeBatch {
                binding: binding(),
                batch_id: 1,
            },
            DroneBayRequest::CancelBatch {
                binding: binding(),
                batch_id: 1,
            },
        ] {
            assert_eq!(
                serde_json::from_slice::<DroneBayRequest>(&serde_json::to_vec(&request).unwrap())
                    .unwrap(),
                request
            );
        }
        let mut journal = DroneBayBatchJournal::default();
        assert_eq!(journal.validated_next_id(&binding()), Some(1));
        journal.operations.push(operation());
        journal.high_watermark = 1;
        assert_eq!(journal.validated_next_id(&binding()), Some(2));
        journal.high_watermark = u64::MAX;
        assert_eq!(journal.validated_next_id(&binding()), None);
        journal.high_watermark = 1;
        journal.operations[0].binding.vessel_id = "foreign".into();
        assert_eq!(journal.validated_next_id(&binding()), None);
    }
    #[test]
    fn cancellation_with_current_work_is_active_and_terminal_state_cannot_hide_members() {
        let mut batch = operation();
        batch.phase = DroneBayBatchPhase::Cancelling;
        batch.held = true;
        batch.members[0].status = DroneBayBatchMemberStatus::Active;
        batch.members[0].operation_id = Some(4);
        batch.current_operation_id = Some(4);
        assert!(batch.active() && batch.validate_shape(&binding()));
        batch.phase = DroneBayBatchPhase::Cancelled;
        assert!(!batch.validate_shape(&binding()));
        batch.held = false;
        batch.current_operation_id = None;
        batch.members[0].status = DroneBayBatchMemberStatus::Stored;
        assert!(batch.validate_shape(&binding()));
        batch.members.push(batch.members[0].clone());
        assert!(!batch.validate_shape(&binding()));
    }
    #[test]
    fn recall_service_history_is_separate_from_storage_and_cleanup_stays_actionable() {
        let mut batch = operation();
        batch.members[0].status = DroneBayBatchMemberStatus::Servicing;
        batch.members[0].service_operation_ids = vec![7];
        batch.current_service_operation_id = Some(7);
        assert!(batch.validate_shape(&binding()));
        batch.current_operation_id = Some(7);
        assert!(!batch.validate_shape(&binding()));
        batch.current_operation_id = None;
        batch.phase = DroneBayBatchPhase::Cancelling;
        batch.held = true;
        assert!(batch.validate_shape(&binding()));
        batch.members[0].status = DroneBayBatchMemberStatus::Destroyed;
        assert!(
            batch.validate_shape(&binding()),
            "destroyed service retains hardware cleanup"
        );
        batch.phase = DroneBayBatchPhase::Cancelled;
        assert!(!batch.validate_shape(&binding()));
        batch.current_service_operation_id = None;
        batch.held = false;
        assert!(batch.validate_shape(&binding()));
        batch.members[0].service_operation_ids = vec![7, 7];
        assert!(!batch.validate_shape(&binding()));
        batch.members[0].service_operation_ids = vec![7];
        batch.kind = DroneBayBatchKind::DeployClass {
            class: DroneClass::Mining,
        };
        assert!(!batch.validate_shape(&binding()));
    }
}
