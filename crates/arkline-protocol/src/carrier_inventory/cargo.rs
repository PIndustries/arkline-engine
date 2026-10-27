//! Ruleset-three explicit roster accounting. This does not relax legacy v2's
//! fixed five-Mining roster or multiply the vessel-global manifold throughput.
use super::*;
use crate::{
    DroneOreHold, DroneOreTransferReceipt, DRONE_DROPOFF_BATCH_MS, DRONE_DROPOFF_BATCH_UNITS,
    MINING_DRONE_CARGO_CAPACITY,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierOreService {
    pub service_id: u64,
    pub physical_operation_id: u64,
    pub drone_id: u64,
    pub berth_id: u8,
    pub accepted_at_ms: u64,
    pub closed_at_ms: Option<u64>,
    pub requested_units: u64,
    pub receipts: Vec<DroneOreTransferReceipt>,
}
impl CarrierOreService {
    pub fn committed_units(&self) -> Option<u64> {
        self.receipts
            .iter()
            .try_fold(0u64, |sum, r| sum.checked_add(r.units))
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CarrierOreState {
    pub binding: CarrierInventoryBinding,
    pub holds: Vec<DroneOreHold>,
    pub lost_ore_units: u64,
    pub service_high_watermark: u64,
    pub services: Vec<CarrierOreService>,
    /// FIFO of physically confirmed, eligible coupled operation IDs. One grant
    /// retains at most one <=100-unit attempt. Paused/faulted entries relinquish.
    pub waiting: Vec<u64>,
    pub granted_service_id: Option<u64>,
    /// Independently reconstructed from every retained receipt on reopen.
    pub last_committed_at_ms: Option<u64>,
}
impl CarrierOreState {
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
    pub fn available_ship_headroom(&self, ship_ore: u64, ship_capacity: u64) -> Option<u64> {
        ship_capacity.checked_sub(ship_ore.checked_add(self.reserved_incoming()?)?)
    }
    pub fn validate_shape(&self, inventory: &CarrierInventoryState) -> bool {
        if self.binding != inventory.binding
            || self.service_high_watermark != self.services.len() as u64
            || !unique(self.holds.iter().map(|h| h.drone_id))
            || !unique(self.waiting.iter().copied())
        {
            return false;
        }
        let mining: BTreeSet<_> = inventory
            .roster
            .iter()
            .filter(|i| i.class == DroneClass::Mining)
            .map(|i| i.drone_id)
            .collect();
        if self
            .holds
            .iter()
            .map(|h| h.drone_id)
            .collect::<BTreeSet<_>>()
            != mining
            || self.lost_ore_units > MINING_DRONE_CARGO_CAPACITY.saturating_mul(mining.len() as u64)
        {
            return false;
        }
        for hold in &self.holds {
            let Some(entry) = inventory.entry(hold.drone_id) else {
                return false;
            };
            if !hold.valid()
                || matches!(entry.location, CarrierDroneLocation::Destroyed { .. })
                    != hold.destroyed_at_ms.is_some()
            {
                return false;
            }
            // Unload-first storage: protected/ready/loading never hides payload.
            if (matches!(
                entry.location,
                CarrierDroneLocation::Protected { .. }
                    | CarrierDroneLocation::Ready { .. }
                    | CarrierDroneLocation::Loading { .. }
            ) && hold.ore_units != 0)
                || (entry.location != CarrierDroneLocation::Deployed && hold.cycle.is_some())
            {
                return false;
            }
            if let (CarrierDroneLocation::Destroyed { at_ms }, Some(held_at)) =
                (&entry.location, hold.destroyed_at_ms)
            {
                if *at_ms != held_at {
                    return false;
                }
            }
        }
        let mut receipt_keys = BTreeSet::new();
        let mut last = None;
        let mut linked = BTreeSet::new();
        for op in &inventory.operations {
            for service in &op.cargo_service_operation_ids {
                if !linked.insert(*service)
                    || !self.services.iter().any(|s| {
                        s.service_id == *service
                            && s.physical_operation_id == op.operation_id
                            && s.drone_id == op.drone_id
                            && op.berth_id == Some(s.berth_id)
                            && s.accepted_at_ms >= op.accepted_at_ms
                            && (!op.phase.terminal()
                                || s.closed_at_ms.is_some_and(|time| time <= op.updated_at_ms))
                    })
                {
                    return false;
                }
            }
        }
        if linked.len() != self.services.len() {
            return false;
        }
        for (index, service) in self.services.iter().enumerate() {
            if service.service_id != index as u64 + 1
                || !mining.contains(&service.drone_id)
                || !(1..=MINING_DRONE_CARGO_CAPACITY).contains(&service.requested_units)
                || service
                    .committed_units()
                    .is_none_or(|units| units > service.requested_units)
                || service
                    .closed_at_ms
                    .is_some_and(|time| time < service.accepted_at_ms)
                || (service.closed_at_ms.is_none()
                    && self.granted_service_id != Some(service.service_id))
            {
                return false;
            }
            for (i, receipt) in service.receipts.iter().enumerate() {
                if receipt.sequence != i as u64 + 1
                    || !(1..=DRONE_DROPOFF_BATCH_UNITS).contains(&receipt.units)
                    || receipt.ledger_sequence == 0
                    || !receipt_keys.insert(receipt.ledger_sequence)
                    || receipt.ledger_hash.len() != 64
                    || !receipt.ledger_hash.bytes().all(|b| b.is_ascii_hexdigit())
                    || receipt
                        .committed_at_ms
                        .checked_sub(service.accepted_at_ms)
                        .is_none_or(|dt| dt < DRONE_DROPOFF_BATCH_MS)
                    || service
                        .closed_at_ms
                        .is_some_and(|time| receipt.committed_at_ms > time)
                    || last.is_some_and(|previous| {
                        receipt
                            .committed_at_ms
                            .checked_sub(previous)
                            .is_none_or(|dt| dt < DRONE_DROPOFF_BATCH_MS)
                    })
                {
                    return false;
                }
                last = Some(receipt.committed_at_ms);
            }
        }
        if last != self.last_committed_at_ms {
            return false;
        }
        if let Some(id) = self.granted_service_id {
            let Some(service) = self
                .services
                .iter()
                .find(|s| s.service_id == id && s.closed_at_ms.is_none())
            else {
                return false;
            };
            if !inventory
                .operation(service.physical_operation_id)
                .is_some_and(|op| {
                    op.phase == CarrierOperationPhase::Servicing
                        && op.berth_id == Some(service.berth_id)
                        && op.drone_id == service.drone_id
                })
                || !inventory.entry(service.drone_id).is_some_and(|entry| {
                    entry.hit_points > 0
                        && entry.location
                            == (CarrierDroneLocation::Servicing {
                                operation_id: service.physical_operation_id,
                                berth_id: service.berth_id,
                            })
                })
                || !self
                    .holds
                    .iter()
                    .find(|h| h.drone_id == service.drone_id)
                    .is_some_and(|hold| {
                        hold.ore_units
                            == service.requested_units - service.committed_units().unwrap()
                            && hold.cycle.is_none()
                            && hold.destroyed_at_ms.is_none()
                    })
                || self.waiting.contains(&service.physical_operation_id)
            {
                return false;
            }
        }
        self.waiting.iter().all(|id| {
            inventory.operation(*id).is_some_and(|op| {
                op.phase == CarrierOperationPhase::Servicing
                    && op.berth_id.is_some()
                    && mining.contains(&op.drone_id)
            })
        })
    }
}

/// New event vocabulary is deliberately distinct from old DroneCargo::Initialized
/// (exactly five Mining IDs). No event here is emitted in old rulesets/worlds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CarrierInventoryLedgerEvent {
    Initialized {
        pilot: String,
        binding: CarrierInventoryBinding,
        roster: Vec<CarrierRosterIdentity>,
        capacity_units_per_mining_drone: u64,
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
        physical_operation_id: u64,
        berth_id: u8,
        service_id: u64,
        service_started_at_ms: u64,
        requested_units: u64,
        sequence: u64,
        units: u64,
    },
    Destroyed {
        pilot: String,
        vessel_id: String,
        drone_id: u64,
        destroyed_at_ms: u64,
        lost_units: u64,
    },
}
