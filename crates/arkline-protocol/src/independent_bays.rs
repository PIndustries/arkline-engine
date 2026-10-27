//! Fresh-preview independent physical lane ownership. This module deliberately
//! does not accept a source asset: a well-formed registration is not supported
//! geometry, and journal validity is not physical capture or transfer evidence.
use crate::{DroneBayRegistration, DroneBayState, DroneStockEntry, DroneStockStatus, MAX_DRONES};
use serde::{Deserialize, Serialize};
mod motion;
pub use motion::*;
use std::collections::BTreeSet;

pub const INDEPENDENT_BAY_SCHEMA_VERSION: u32 = 1;
pub const INDEPENDENT_BAY_CLIENT_CAPABILITY_VERSION: u32 = 6;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayRegistration {
    pub schema_version: u32,
    pub host: DroneBayRegistration,
    pub assembly_revision: u32,
    pub assembly_source_sha256: String,
}
impl IndependentBayRegistration {
    pub fn well_formed(&self) -> bool {
        let hash = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit());
        self.schema_version == INDEPENDENT_BAY_SCHEMA_VERSION
            && self.host.geometry_revision > 0
            && hash(&self.host.source_sha256)
            && self.assembly_revision > 0
            && hash(&self.assembly_source_sha256)
    }
    pub fn bind(&self, vessel_id: String) -> IndependentBayBinding {
        IndependentBayBinding {
            vessel_id,
            registration: self.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayBinding {
    pub vessel_id: String,
    pub registration: IndependentBayRegistration,
}
impl IndependentBayBinding {
    pub fn well_formed(&self) -> bool {
        !self.vessel_id.is_empty() && self.vessel_id.len() <= 256 && self.registration.well_formed()
    }
}

/// Availability is confirmed operational state, not an invented health, armor,
/// energy or damage rating. Fault-injected tests do not establish combat damage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndependentBayAvailability {
    Available,
    Unavailable { reason: String },
}
impl IndependentBayAvailability {
    pub fn available(&self) -> bool {
        matches!(self, Self::Available)
    }
    fn valid(&self) -> bool {
        match self {
            Self::Available => true,
            Self::Unavailable { reason } => !reason.is_empty() && reason.len() <= 512,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayComponents {
    pub door: IndependentBayAvailability,
    pub support: IndependentBayAvailability,
    pub clamp: IndependentBayAvailability,
    pub ore_mount: IndependentBayAvailability,
}
impl IndependentBayComponents {
    pub fn handling_available(&self) -> bool {
        self.door.available() && self.support.available() && self.clamp.available()
    }
    pub fn transfer_available(&self) -> bool {
        self.handling_available() && self.ore_mount.available()
    }
    fn valid(&self) -> bool {
        [&self.door, &self.support, &self.clamp, &self.ore_mount]
            .iter()
            .all(|v| v.valid())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndependentBayAction {
    Launch,
    ReturnToUnload { resume_mining: bool },
    ReturnToStorage,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndependentBayPhase {
    Queued,
    Preparing,
    Opening,
    Releasing,
    Launching,
    Approaching,
    Aligning,
    Supporting,
    Clamping,
    Coupling,
    WaitingForManifold,
    Transferring,
    Uncoupling,
    Stowing,
    Securing,
    Departing,
    Held,
    Cancelling,
    Free,
    Stored,
    Cancelled,
    Destroyed,
}
impl IndependentBayPhase {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Free | Self::Stored | Self::Cancelled | Self::Destroyed
        )
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Waiting for a compatible bay",
            Self::Preparing => "Preparing bay",
            Self::Opening => "Opening bay door",
            Self::Releasing => "Releasing cradle clamps",
            Self::Launching => "Launching from bay",
            Self::Approaching => "Approaching assigned bay",
            Self::Aligning => "Aligning with bay",
            Self::Supporting => "Seating on cradle support",
            Self::Clamping => "Securing cradle clamps",
            Self::Coupling => "Connecting ore mount",
            Self::WaitingForManifold => "Coupled, waiting for shared transfer",
            Self::Transferring => "Transferring confirmed ore",
            Self::Uncoupling => "Disconnecting ore mount",
            Self::Stowing => "Stowing in assigned bay",
            Self::Securing => "Securing bay hardware",
            Self::Departing => "Clearing assigned bay",
            Self::Held => "Bay operation held",
            Self::Cancelling => "Returning hardware to a safe state",
            Self::Free => "Deployed",
            Self::Stored => "Secured in bay",
            Self::Cancelled => "Bay operation cancelled",
            Self::Destroyed => "Drone destroyed",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayOperation {
    pub operation_id: u64,
    pub drone_id: u64,
    pub action: IndependentBayAction,
    pub binding: IndependentBayBinding,
    /// Preferred home remains on the canonical stock entry. This is a separate
    /// accepted destination, retained historically after completion.
    pub assigned_bay: Option<u8>,
    pub phase: IndependentBayPhase,
    pub held_phase: Option<IndependentBayPhase>,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
    pub job_id: Option<u64>,
    pub message: String,
    /// Typed links into the existing cargo journal. These are NOT ore balances.
    /// Interrupted grants retain their attempt ID and receipts; a resumed grant
    /// gets a new monotonic economic ID without replacing this physical identity.
    pub cargo_service_operation_ids: Vec<u64>,
}
impl IndependentBayOperation {
    pub fn effective_phase(&self) -> IndependentBayPhase {
        self.held_phase.unwrap_or(self.phase)
    }
    fn valid(&self, binding: &IndependentBayBinding) -> bool {
        self.operation_id > 0
            && self.drone_id > 0
            && &self.binding == binding
            && self
                .assigned_bay
                .is_none_or(|id| usize::from(id) < MAX_DRONES)
            && self.accepted_at_ms <= self.updated_at_ms
            && self.message.len() <= 512
            && match (self.phase, self.held_phase) {
                (IndependentBayPhase::Held, Some(p)) => {
                    p != IndependentBayPhase::Held && !p.terminal()
                }
                (IndependentBayPhase::Held, None) => false,
                (_, Some(_)) => false,
                (_, None) => true,
            }
            && (matches!(
                self.effective_phase(),
                IndependentBayPhase::Queued
                    | IndependentBayPhase::Cancelled
                    | IndependentBayPhase::Destroyed
            ) || self.assigned_bay.is_some())
            && (self.phase != IndependentBayPhase::Stored
                || self.action == IndependentBayAction::ReturnToStorage)
            && (self.phase != IndependentBayPhase::Free
                || self.action != IndependentBayAction::ReturnToStorage)
            && self.cargo_service_operation_ids.iter().all(|id| *id > 0)
            && self
                .cargo_service_operation_ids
                .windows(2)
                .all(|p| p[0] < p[1])
            && (self.action != IndependentBayAction::Launch
                || self.cargo_service_operation_ids.is_empty())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayCleanup {
    pub operation_id: u64,
    pub drone_id: u64,
    pub held: bool,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayLane {
    pub bay_id: u8,
    pub components: IndependentBayComponents,
    /// Only an actually captured Stored identity; an active constrained body is
    /// owned by operation_id instead and must never render a second stock body.
    pub stored_drone_id: Option<u64>,
    pub operation_id: Option<u64>,
    pub cleanup: Option<IndependentBayCleanup>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayManifoldGrant {
    pub physical_operation_id: u64,
    pub cargo_service_operation_id: u64,
    pub bay_id: u8,
    pub granted_at_ms: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayManifold {
    pub grant: Option<IndependentBayManifoldGrant>,
    /// FIFO of coupled physical operation IDs, never a second payload inventory.
    pub waiting: Vec<u64>,
    /// Vessel-global cooldown survives operation/bay handoff, holds and restart.
    pub last_committed_at_ms: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndependentBayState {
    pub schema_version: u32,
    pub binding: IndependentBayBinding,
    pub lanes: Vec<IndependentBayLane>,
    pub high_watermark: u64,
    /// No eviction. Exact historical retries never execute again.
    pub operations: Vec<IndependentBayOperation>,
    pub manifold: IndependentBayManifold,
}
impl IndependentBayState {
    pub fn operation(&self, id: u64) -> Option<&IndependentBayOperation> {
        self.operations.iter().find(|op| op.operation_id == id)
    }
    pub fn active_operation(&self, drone_id: u64) -> Option<&IndependentBayOperation> {
        self.operations
            .iter()
            .find(|op| op.drone_id == drone_id && !op.phase.terminal())
    }
    pub fn carrier_locked(&self) -> bool {
        self.lanes
            .iter()
            .any(|lane| lane.operation_id.is_some() || lane.cleanup.is_some())
    }
    pub fn next_operation_id(&self) -> Option<u64> {
        self.high_watermark.checked_add(1)
    }
    /// Structural ownership only. Callers must additionally validate exact source
    /// registration, accepted route/pose/clock, live fleet and cargo journal links.
    /// No arbitrary registration is made runtime-supported by passing this check.
    pub fn validate_shape(&self, stock: &[DroneStockEntry]) -> bool {
        if self.schema_version != INDEPENDENT_BAY_SCHEMA_VERSION
            || !self.binding.well_formed()
            || self.lanes.len() != MAX_DRONES
            || stock.len() != MAX_DRONES
            || u64::try_from(self.operations.len()).ok() != Some(self.high_watermark)
        {
            return false;
        }
        let mut ids = BTreeSet::new();
        let mut homes = BTreeSet::new();
        if !stock.iter().all(|e| {
            e.drone_id > 0
                && ids.insert(e.drone_id)
                && homes.insert(e.home_cradle)
                && DroneBayState::expected_class(e.home_cradle) == Some(e.class)
                && match e.status {
                    DroneStockStatus::Stored { hit_points } => {
                        (1..=crate::DRONE_HIT_POINTS).contains(&hit_points)
                    }
                    _ => true,
                }
        }) {
            return false;
        }
        let mut active_ids = BTreeSet::new();
        let mut service_ids = BTreeSet::new();
        if !self.operations.iter().enumerate().all(|(index, op)| {
            op.operation_id == index as u64 + 1
                && op.valid(&self.binding)
                && ids.contains(&op.drone_id)
                && (op.phase.terminal()
                    || (active_ids.insert(op.drone_id)
                        && stock.iter().any(|entry| {
                            entry.drone_id == op.drone_id
                                && (!matches!(
                                    op.action,
                                    IndependentBayAction::ReturnToUnload { .. }
                                ) || entry.class == crate::DroneClass::Mining)
                                && match (op.action, &entry.status) {
                                    (
                                        IndependentBayAction::Launch,
                                        DroneStockStatus::Stored { .. },
                                    ) => true,
                                    (IndependentBayAction::Launch, DroneStockStatus::Active) => {
                                        op.effective_phase() != IndependentBayPhase::Queued
                                    }
                                    (
                                        IndependentBayAction::ReturnToUnload { .. }
                                        | IndependentBayAction::ReturnToStorage,
                                        DroneStockStatus::Active,
                                    ) => true,
                                    _ => false,
                                }
                        })))
                && op
                    .cargo_service_operation_ids
                    .iter()
                    .all(|id| service_ids.insert(*id))
        }) {
            return false;
        }
        let mut occupied = BTreeSet::new();
        let mut owners = BTreeSet::new();
        for (index, lane) in self.lanes.iter().enumerate() {
            if usize::from(lane.bay_id) != index
                || !lane.components.valid()
                || (lane.operation_id.is_some() && lane.cleanup.is_some())
            {
                return false;
            }
            if let Some(id) = lane.stored_drone_id {
                if !occupied.insert(id)
                    || !stock.iter().any(|e| {
                        e.drone_id == id && matches!(e.status, DroneStockStatus::Stored { .. })
                    })
                {
                    return false;
                }
            }
            if let Some(id) = lane.operation_id {
                let Some(op) = self.operation(id) else {
                    return false;
                };
                if !owners.insert(id)
                    || op.phase.terminal()
                    || op.assigned_bay != Some(lane.bay_id)
                    || op.effective_phase() == IndependentBayPhase::Queued
                    || lane.stored_drone_id.is_some_and(|id| id != op.drone_id)
                    || stock.iter().any(|e| {
                        e.drone_id == op.drone_id
                            && matches!(e.status, DroneStockStatus::Destroyed { .. })
                    })
                {
                    return false;
                }
            }
            if let Some(cleanup) = &lane.cleanup {
                let Some(op) = self.operation(cleanup.operation_id) else {
                    return false;
                };
                if !owners.insert(op.operation_id)
                    || op.phase != IndependentBayPhase::Destroyed
                    || op.drone_id != cleanup.drone_id
                    || op.assigned_bay != Some(lane.bay_id)
                    || lane.stored_drone_id.is_some()
                    || cleanup.reason.len() > 512
                    || !stock.iter().any(|e| {
                        e.drone_id == cleanup.drone_id
                            && matches!(e.status, DroneStockStatus::Destroyed { .. })
                    })
                {
                    return false;
                }
            }
        }
        if !stock.iter().all(|e| {
            matches!(e.status, DroneStockStatus::Stored { .. }) == occupied.contains(&e.drone_id)
        }) {
            return false;
        }
        if !self
            .operations
            .iter()
            .filter(|op| !op.phase.terminal())
            .all(|op| {
                if op.effective_phase() == IndependentBayPhase::Queued {
                    op.assigned_bay.is_none() && !owners.contains(&op.operation_id)
                } else {
                    owners.contains(&op.operation_id)
                }
            })
        {
            return false;
        }
        let mut waiting = BTreeSet::new();
        if !self.manifold.waiting.iter().all(|id| {
            waiting.insert(*id)
                && self.operation(*id).is_some_and(|op| {
                    op.phase == IndependentBayPhase::WaitingForManifold
                        && op.action != IndependentBayAction::Launch
                        && op.assigned_bay.is_some_and(|bay| {
                            self.lanes[usize::from(bay)].operation_id == Some(*id)
                        })
                })
        }) {
            return false;
        }
        if !self.operations.iter().all(|op| {
            op.phase != IndependentBayPhase::WaitingForManifold
                || waiting.contains(&op.operation_id)
        }) {
            return false;
        }
        self.manifold.grant.as_ref().is_none_or(|grant| {
            !waiting.contains(&grant.physical_operation_id)
                && self
                    .operation(grant.physical_operation_id)
                    .is_some_and(|op| {
                        op.phase == IndependentBayPhase::Transferring
                            && op.assigned_bay == Some(grant.bay_id)
                            && op.cargo_service_operation_ids.last()
                                == Some(&grant.cargo_service_operation_id)
                            && op.accepted_at_ms <= grant.granted_at_ms
                            && grant.granted_at_ms <= op.updated_at_ms
                            && self.lanes[usize::from(grant.bay_id)].operation_id
                                == Some(op.operation_id)
                    })
        }) && self.operations.iter().all(|op| {
            op.phase != IndependentBayPhase::Transferring
                || self
                    .manifold
                    .grant
                    .as_ref()
                    .is_some_and(|grant| grant.physical_operation_id == op.operation_id)
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IndependentBayRequest {
    Begin {
        binding: IndependentBayBinding,
        operation_id: u64,
        drone_id: u64,
        action: IndependentBayAction,
    },
    Hold {
        binding: IndependentBayBinding,
        operation_id: u64,
    },
    Resume {
        binding: IndependentBayBinding,
        operation_id: u64,
    },
    Cancel {
        binding: IndependentBayBinding,
        operation_id: u64,
    },
}

#[cfg(test)]
mod tests;
