use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierDeploymentPhase {
    Running,
    Cancelling,
    Completed,
    Cancelled,
}
impl CarrierDeploymentPhase {
    pub fn active(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierDeploymentMember {
    pub drone_id: u64,
    pub stage_operation_id: Option<u64>,
    pub launch_operation_id: Option<u64>,
    pub cancelled: bool,
    pub blocked_reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierDeploymentBatch {
    pub batch_id: u64,
    /// Fixed server-selected eligible roster at acceptance; later stock is not included.
    pub members: Vec<CarrierDeploymentMember>,
    pub phase: CarrierDeploymentPhase,
    pub held: bool,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
}
impl CarrierInventoryState {
    pub fn active_deployment(&self) -> Option<&CarrierDeploymentBatch> {
        self.deployments.iter().find(|b| b.phase.active())
    }
    pub fn deployment_owner(&self, operation_id: u64) -> Option<&CarrierDeploymentBatch> {
        self.deployments.iter().find(|b| {
            b.phase.active()
                && b.members.iter().any(|m| {
                    m.stage_operation_id == Some(operation_id)
                        || m.launch_operation_id == Some(operation_id)
                })
        })
    }
    pub fn next_deployment_id(&self) -> Option<u64> {
        self.deployment_high_watermark.checked_add(1)
    }
    pub(super) fn validate_deployments(&self) -> bool {
        if self.deployment_high_watermark != self.deployments.len() as u64
            || self.deployments.iter().filter(|b| b.phase.active()).count() > 1
        {
            return false;
        }
        let mut operation_ids = BTreeSet::new();
        for (i, b) in self.deployments.iter().enumerate() {
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
                {
                    return false;
                }
                for (id, stage) in [(m.stage_operation_id, true), (m.launch_operation_id, false)] {
                    if let Some(id) = id {
                        let Some(op) = self.operation(id) else {
                            return false;
                        };
                        if !operation_ids.insert(id)
                            || op.drone_id != m.drone_id
                            || op.accepted_at_ms < b.accepted_at_ms
                            || (stage && !matches!(op.action, CarrierInventoryAction::Stage { .. }))
                            || (!stage && op.action != CarrierInventoryAction::Launch)
                        {
                            return false;
                        }
                        if m.cancelled && !op.phase.terminal() && !op.released_loaded_owner() {
                            return false;
                        }
                    }
                }
                if let (Some(stage), Some(launch)) = (m.stage_operation_id, m.launch_operation_id) {
                    let a = self.operation(stage).unwrap();
                    let z = self.operation(launch).unwrap();
                    let released = a.stage.as_ref().and_then(|x| x.loaded_handoff_at_ms);
                    if (a.phase != CarrierOperationPhase::Completed && released.is_none())
                        || stage >= launch
                        || released.unwrap_or(a.updated_at_ms) > z.accepted_at_ms
                    {
                        return false;
                    }
                }
                if !b.phase.active() {
                    if [m.stage_operation_id, m.launch_operation_id]
                        .into_iter()
                        .flatten()
                        .any(|id| self.operation(id).is_none_or(|o| !o.phase.terminal()))
                    {
                        return false;
                    }
                    let done = m
                        .launch_operation_id
                        .and_then(|id| self.operation(id))
                        .is_some_and(|o| {
                            o.phase == CarrierOperationPhase::Completed
                                || o.phase == CarrierOperationPhase::Destroyed
                        })
                        || m.stage_operation_id
                            .and_then(|id| self.operation(id))
                            .is_some_and(|o| o.phase == CarrierOperationPhase::Destroyed);
                    if !done && !m.cancelled {
                        return false;
                    }
                    if b.phase == CarrierDeploymentPhase::Completed && m.cancelled {
                        return false;
                    }
                    if [m.stage_operation_id, m.launch_operation_id]
                        .into_iter()
                        .flatten()
                        .any(|id| self.cleanups.iter().any(|c| c.operation_id == id))
                    {
                        return false;
                    }
                }
            }
        }
        true
    }
}
