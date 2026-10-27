//! Finite inventory for fresh, source-bound carrier assemblies. Ready berths are
//! attachment positions, not class quotas. Transit buffers add no stock capacity.
//! This structural contract alone never admits an assembly or proves a route.
use crate::{DroneBayRegistration, DroneClass, DRONE_HIT_POINTS};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const CARRIER_INVENTORY_SCHEMA_VERSION: u32 = 1;
pub const CARRIER_INVENTORY_CLIENT_CAPABILITY: u32 = 7;
pub const CARRIER_INVENTORY_RULESET_VERSION: u32 = 3;
pub const CARRIER_READY_BERTH_COUNT: usize = 10;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierInventoryRegistration {
    pub schema_version: u32,
    pub host: DroneBayRegistration,
    pub assembly_revision: u32,
    pub assembly_source_sha256: String,
}
impl CarrierInventoryRegistration {
    pub fn well_formed(&self) -> bool {
        let hash = |s: &str| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit());
        self.schema_version == CARRIER_INVENTORY_SCHEMA_VERSION
            && self.host.geometry_revision > 0
            && hash(&self.host.source_sha256)
            && self.assembly_revision > 0
            && hash(&self.assembly_source_sha256)
    }
    pub fn bind(&self, vessel_id: String) -> CarrierInventoryBinding {
        CarrierInventoryBinding {
            vessel_id,
            registration: self.clone(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierInventoryBinding {
    pub vessel_id: String,
    pub registration: CarrierInventoryRegistration,
}
impl CarrierInventoryBinding {
    pub fn well_formed(&self) -> bool {
        !self.vessel_id.is_empty() && self.vessel_id.len() <= 256 && self.registration.well_formed()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierSide {
    Port,
    Starboard,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierReadyBerthSpec {
    pub berth_id: u8,
    pub side: CarrierSide,
    pub compatible_classes: Vec<DroneClass>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierMagazineSlotSpec {
    pub slot_id: u16,
    pub side: CarrierSide,
    pub compatible_classes: Vec<DroneClass>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierTransitBufferSpec {
    pub buffer_id: u8,
    pub side: CarrierSide,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierLoaderSpec {
    pub loader_id: u8,
    pub side: CarrierSide,
    pub magazine_slots: Vec<u16>,
    pub ready_berths: Vec<u8>,
    pub transit_buffer_id: u8,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierInventoryTopology {
    pub registration: CarrierInventoryRegistration,
    pub ready_berths: Vec<CarrierReadyBerthSpec>,
    pub magazine_slots: Vec<CarrierMagazineSlotSpec>,
    pub loaders: Vec<CarrierLoaderSpec>,
    pub transit_buffers: Vec<CarrierTransitBufferSpec>,
    pub cassette_count: u16,
}
impl CarrierInventoryTopology {
    pub fn capacity(&self) -> usize {
        self.ready_berths.len() + self.magazine_slots.len()
    }
    pub fn berth(&self, id: u8) -> Option<&CarrierReadyBerthSpec> {
        self.ready_berths.iter().find(|b| b.berth_id == id)
    }
    pub fn slot(&self, id: u16) -> Option<&CarrierMagazineSlotSpec> {
        self.magazine_slots.iter().find(|s| s.slot_id == id)
    }
    pub fn loader(&self, id: u8) -> Option<&CarrierLoaderSpec> {
        self.loaders.iter().find(|l| l.loader_id == id)
    }
    pub fn buffer(&self, id: u8) -> Option<&CarrierTransitBufferSpec> {
        self.transit_buffers.iter().find(|b| b.buffer_id == id)
    }
    pub fn stage_loader(&self, slot: u16, berth: u8, class: DroneClass) -> Option<u8> {
        let from = self.slot(slot)?;
        let to = self.berth(berth)?;
        if from.side != to.side
            || !from.compatible_classes.contains(&class)
            || !to.compatible_classes.contains(&class)
        {
            return None;
        }
        self.loaders
            .iter()
            .filter(|l| {
                l.side == from.side
                    && l.magazine_slots.contains(&slot)
                    && l.ready_berths.contains(&berth)
            })
            .map(|l| l.loader_id)
            .min()
    }
    pub fn accepts(&self, dock: CarrierCassetteDock, class: DroneClass) -> bool {
        match dock {
            CarrierCassetteDock::Protected { slot_id } => self
                .slot(slot_id)
                .is_some_and(|s| s.compatible_classes.contains(&class)),
            CarrierCassetteDock::Ready { berth_id } => self
                .berth(berth_id)
                .is_some_and(|b| b.compatible_classes.contains(&class)),
            CarrierCassetteDock::TransitBuffer { .. } => false,
        }
    }
    pub fn contains_dock(&self, dock: CarrierCassetteDock) -> bool {
        match dock {
            CarrierCassetteDock::Protected { slot_id } => self.slot(slot_id).is_some(),
            CarrierCassetteDock::Ready { berth_id } => self.berth(berth_id).is_some(),
            CarrierCassetteDock::TransitBuffer { buffer_id } => self.buffer(buffer_id).is_some(),
        }
    }
    /// Physical reachability follows the registered loader graph, not a side or
    /// immutable drone-home quota. Transit buffers connect paths but add no stock.
    pub fn same_dock_component(&self, from: CarrierCassetteDock, to: CarrierCassetteDock) -> bool {
        if !self.valid_shape() || !self.contains_dock(from) || !self.contains_dock(to) {
            return false;
        }
        let mut reached = BTreeSet::from([from]);
        loop {
            if reached.contains(&to) {
                return true;
            }
            let old_len = reached.len();
            for loader in &self.loaders {
                let docks: Vec<_> = loader
                    .ready_berths
                    .iter()
                    .map(|id| CarrierCassetteDock::Ready { berth_id: *id })
                    .chain(
                        loader
                            .magazine_slots
                            .iter()
                            .map(|id| CarrierCassetteDock::Protected { slot_id: *id }),
                    )
                    .chain(std::iter::once(CarrierCassetteDock::TransitBuffer {
                        buffer_id: loader.transit_buffer_id,
                    }))
                    .collect();
                if docks.iter().any(|dock| reached.contains(dock)) {
                    reached.extend(docks);
                }
            }
            if reached.len() == old_len {
                return false;
            }
        }
    }
    pub fn valid_shape(&self) -> bool {
        let classes = |v: &Vec<DroneClass>| {
            !v.is_empty() && v.iter().enumerate().all(|(i, c)| !v[..i].contains(c))
        };
        self.registration.well_formed()
            && self.ready_berths.len() == CARRIER_READY_BERTH_COUNT
            && usize::from(self.cassette_count) == self.capacity()
            && [CarrierSide::Port, CarrierSide::Starboard]
                .iter()
                .all(|s| self.ready_berths.iter().filter(|b| b.side == *s).count() == 5)
            && unique(self.ready_berths.iter().map(|b| b.berth_id))
            && unique(self.magazine_slots.iter().map(|s| s.slot_id))
            && unique(self.loaders.iter().map(|l| l.loader_id))
            && unique(self.transit_buffers.iter().map(|b| b.buffer_id))
            && self
                .ready_berths
                .iter()
                .all(|b| classes(&b.compatible_classes))
            && self
                .magazine_slots
                .iter()
                .all(|s| classes(&s.compatible_classes))
            && self.loaders.iter().all(|l| {
                !l.magazine_slots.is_empty()
                    && !l.ready_berths.is_empty()
                    && unique(l.magazine_slots.iter().copied())
                    && unique(l.ready_berths.iter().copied())
                    && l.magazine_slots
                        .iter()
                        .all(|id| self.slot(*id).is_some_and(|s| s.side == l.side))
                    && l.ready_berths
                        .iter()
                        .all(|id| self.berth(*id).is_some_and(|b| b.side == l.side))
                    && self
                        .buffer(l.transit_buffer_id)
                        .is_some_and(|b| b.side == l.side)
            })
            && self.magazine_slots.iter().all(|s| {
                self.loaders
                    .iter()
                    .any(|l| l.magazine_slots.contains(&s.slot_id))
            })
    }
}
fn unique<T: Ord>(items: impl Iterator<Item = T>) -> bool {
    let mut seen = BTreeSet::new();
    items.into_iter().all(|x| seen.insert(x))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierCassetteDock {
    Protected { slot_id: u16 },
    Ready { berth_id: u8 },
    TransitBuffer { buffer_id: u8 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierCassetteLocation {
    Docked {
        dock: CarrierCassetteDock,
    },
    InLoader {
        operation_id: u64,
        from: CarrierCassetteDock,
        to: CarrierCassetteDock,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierCassette {
    pub cassette_id: u64,
    pub drone_id: Option<u64>,
    pub location: CarrierCassetteLocation,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierDroneLocation {
    Protected { slot_id: u16 },
    Ready { berth_id: u8 },
    Loading { operation_id: u64 },
    Launching { operation_id: u64, berth_id: u8 },
    Returning { operation_id: u64, berth_id: u8 },
    Servicing { operation_id: u64, berth_id: u8 },
    Deployed,
    Destroyed { at_ms: u64 },
}
impl CarrierDroneLocation {
    pub fn dock(&self) -> Option<CarrierCassetteDock> {
        match self {
            Self::Protected { slot_id } => {
                Some(CarrierCassetteDock::Protected { slot_id: *slot_id })
            }
            Self::Ready { berth_id } => Some(CarrierCassetteDock::Ready {
                berth_id: *berth_id,
            }),
            _ => None,
        }
    }
    pub fn operation_id(&self) -> Option<u64> {
        match self {
            Self::Loading { operation_id }
            | Self::Launching { operation_id, .. }
            | Self::Returning { operation_id, .. }
            | Self::Servicing { operation_id, .. } => Some(*operation_id),
            _ => None,
        }
    }
    pub fn has_active_space_body(&self) -> bool {
        matches!(
            self,
            Self::Launching { .. }
                | Self::Returning { .. }
                | Self::Servicing { .. }
                | Self::Deployed
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierRosterIdentity {
    pub drone_id: u64,
    pub class: DroneClass,
    pub preferred_magazine_slot: Option<u16>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierDroneEntry {
    pub drone_id: u64,
    pub class: DroneClass,
    pub hit_points: u32,
    pub location: CarrierDroneLocation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierInventoryAction {
    Stage {
        berth_id: u8,
    },
    /// Physically exchange a loaded Ready cassette into an empty protected slot.
    StoreReady {
        slot_id: u16,
    },
    Launch,
    ReturnToUnload {
        resume_mining: bool,
    },
    RecallToStorage,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierOperationPhase {
    Queued,
    Loading,
    Launching,
    Returning,
    Servicing,
    Securing,
    Held,
    Completed,
    Cancelled,
    Destroyed,
}
impl CarrierOperationPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Destroyed)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierStageLeg {
    EmptyToBuffer,
    LoadedToReady,
    EmptyToMagazine,
    LoadedToProtected,
    EmptyToReady,
}
/// The retained slot/berth fields name physical docks, not immutable drone homes.
/// Omission is the historical Protected -> Ready route, byte-compatible on write.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CarrierStageDirection {
    #[default]
    ToReady,
    ToProtected,
}
impl CarrierStageDirection {
    pub fn is_forward(&self) -> bool {
        *self == Self::ToReady
    }
    pub fn loaded_source(self, slot_id: u16, berth_id: u8) -> CarrierCassetteDock {
        match self {
            Self::ToReady => CarrierCassetteDock::Protected { slot_id },
            Self::ToProtected => CarrierCassetteDock::Ready { berth_id },
        }
    }
    pub fn loaded_destination(self, slot_id: u16, berth_id: u8) -> CarrierCassetteDock {
        match self {
            Self::ToReady => CarrierCassetteDock::Ready { berth_id },
            Self::ToProtected => CarrierCassetteDock::Protected { slot_id },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierStageExchange {
    #[serde(default, skip_serializing_if = "CarrierStageDirection::is_forward")]
    pub direction: CarrierStageDirection,
    pub loaded_cassette_id: u64,
    pub empty_cassette_id: u64,
    pub source_slot_id: u16,
    pub destination_berth_id: u8,
    pub loader_id: u8,
    pub buffer_id: u8,
    /// Number of complete, source-confirmed legs; an in-flight leg is not complete.
    pub completed_legs: u8,
    /// Immutable source-confirmed transfer of loaded cassette/body ownership.
    /// The empty-cassette tail remains live until leg three actually completes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded_handoff_at_ms: Option<u64>,
}
impl CarrierStageExchange {
    pub fn loaded_source(&self) -> CarrierCassetteDock {
        self.direction
            .loaded_source(self.source_slot_id, self.destination_berth_id)
    }
    pub fn loaded_destination(&self) -> CarrierCassetteDock {
        self.direction
            .loaded_destination(self.source_slot_id, self.destination_berth_id)
    }
    pub fn current_leg(&self) -> Option<(u64, CarrierCassetteDock, CarrierCassetteDock)> {
        let buffer = CarrierCassetteDock::TransitBuffer {
            buffer_id: self.buffer_id,
        };
        match self.completed_legs {
            0 => Some((self.empty_cassette_id, self.loaded_destination(), buffer)),
            1 => Some((
                self.loaded_cassette_id,
                self.loaded_source(),
                self.loaded_destination(),
            )),
            2 => Some((self.empty_cassette_id, buffer, self.loaded_source())),
            _ => None,
        }
    }
    pub fn leg(&self) -> Option<CarrierStageLeg> {
        match self.completed_legs {
            0 => Some(CarrierStageLeg::EmptyToBuffer),
            1 => Some(if self.direction.is_forward() {
                CarrierStageLeg::LoadedToReady
            } else {
                CarrierStageLeg::LoadedToProtected
            }),
            2 => Some(if self.direction.is_forward() {
                CarrierStageLeg::EmptyToMagazine
            } else {
                CarrierStageLeg::EmptyToReady
            }),
            _ => None,
        }
    }
    pub fn retained_resources(&self) -> Vec<CarrierResource> {
        self.resources()
            .into_iter()
            .filter(|r| {
                self.loaded_handoff_at_ms.is_none()
                    || *r
                        != (CarrierResource::ReadyBerth {
                            berth_id: self.destination_berth_id,
                        })
                        && *r
                            != (CarrierResource::Cassette {
                                cassette_id: self.loaded_cassette_id,
                            })
            })
            .collect()
    }
    pub fn resources(&self) -> [CarrierResource; 6] {
        [
            CarrierResource::MagazineSlot {
                slot_id: self.source_slot_id,
            },
            CarrierResource::ReadyBerth {
                berth_id: self.destination_berth_id,
            },
            CarrierResource::Loader {
                loader_id: self.loader_id,
            },
            CarrierResource::TransitBuffer {
                buffer_id: self.buffer_id,
            },
            CarrierResource::Cassette {
                cassette_id: self.loaded_cassette_id,
            },
            CarrierResource::Cassette {
                cassette_id: self.empty_cassette_id,
            },
        ]
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierResource {
    MagazineSlot { slot_id: u16 },
    ReadyBerth { berth_id: u8 },
    Loader { loader_id: u8 },
    TransitBuffer { buffer_id: u8 },
    Cassette { cassette_id: u64 },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierResourceReservation {
    pub resource: CarrierResource,
    pub operation_id: u64,
    pub drone_id: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierInventoryOperation {
    /// Immutable typed provenance for new Recall children, including instant losses.
    /// Historical journals omit this field unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_batch_id: Option<u64>,
    pub operation_id: u64,
    pub drone_id: u64,
    pub action: CarrierInventoryAction,
    pub phase: CarrierOperationPhase,
    pub held_phase: Option<CarrierOperationPhase>,
    pub stage: Option<CarrierStageExchange>,
    pub berth_id: Option<u8>,
    pub accepted_at_ms: u64,
    pub updated_at_ms: u64,
    pub message: String,
    pub cargo_service_operation_ids: Vec<u64>,
}
impl CarrierInventoryOperation {
    pub fn released_loaded_owner(&self) -> bool {
        self.stage
            .as_ref()
            .is_some_and(|x| x.loaded_handoff_at_ms.is_some())
    }
    pub fn effective_phase(&self) -> CarrierOperationPhase {
        self.held_phase.unwrap_or(self.phase)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierHardwareCleanup {
    pub operation_id: u64,
    pub held: bool,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierInventoryState {
    pub schema_version: u32,
    pub binding: CarrierInventoryBinding,
    pub initialized_at_ms: u64,
    /// Immutable original finite identity/class roster, independently bound by the genesis ledger.
    pub roster: Vec<CarrierRosterIdentity>,
    pub entries: Vec<CarrierDroneEntry>,
    pub cassettes: Vec<CarrierCassette>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub recall_high_watermark: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recalls: Vec<CarrierRecallBatch>,
    pub deployment_high_watermark: u64,
    pub deployments: Vec<CarrierDeploymentBatch>,
    pub high_watermark: u64,
    pub operations: Vec<CarrierInventoryOperation>,
    pub reservations: Vec<CarrierResourceReservation>,
    pub cleanups: Vec<CarrierHardwareCleanup>,
}
impl CarrierInventoryState {
    pub fn entry(&self, id: u64) -> Option<&CarrierDroneEntry> {
        self.entries.iter().find(|e| e.drone_id == id)
    }
    pub fn cassette(&self, id: u64) -> Option<&CarrierCassette> {
        self.cassettes.iter().find(|c| c.cassette_id == id)
    }
    pub fn operation(&self, id: u64) -> Option<&CarrierInventoryOperation> {
        self.operations.iter().find(|o| o.operation_id == id)
    }
    pub fn next_operation_id(&self) -> Option<u64> {
        self.high_watermark.checked_add(1)
    }
    pub fn active_operation(&self, id: u64) -> Option<&CarrierInventoryOperation> {
        self.operations
            .iter()
            .find(|o| o.drone_id == id && !o.phase.terminal() && !o.released_loaded_owner())
    }
    pub fn reserved(&self, resource: CarrierResource) -> bool {
        self.reservations.iter().any(|r| r.resource == resource)
    }
    pub fn docked_cassette(&self, dock: CarrierCassetteDock) -> Option<&CarrierCassette> {
        self.cassettes
            .iter()
            .find(|c| c.location == CarrierCassetteLocation::Docked { dock })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierInventoryRequest {
    RecallAll {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    HoldRecall {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    ResumeRecall {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    CancelRecallRemaining {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    DeployAll {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    HoldDeployment {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    ResumeDeployment {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    CancelDeploymentRemaining {
        binding: CarrierInventoryBinding,
        batch_id: u64,
    },
    Begin {
        binding: CarrierInventoryBinding,
        operation_id: u64,
        drone_id: u64,
        action: CarrierInventoryAction,
    },
    Hold {
        binding: CarrierInventoryBinding,
        operation_id: u64,
    },
    Resume {
        binding: CarrierInventoryBinding,
        operation_id: u64,
    },
    Cancel {
        binding: CarrierInventoryBinding,
        operation_id: u64,
    },
}
mod cargo;
#[cfg(test)]
mod tests;
pub use cargo::*;
mod validation;

mod deployment;
pub use deployment::*;

mod presentation;
pub use presentation::*;

fn is_zero(v: &u64) -> bool {
    *v == 0
}
mod recall;
pub use recall::*;
