//! Additive, authority-only ore unloading, advertised only by DockingV1 assets.
//! Quantities are generic ore units, not kilograms or physical loader telemetry.
use crate::ShipClass;
use serde::{Deserialize, Serialize};
pub const CARGO_UNLOADING_SCHEMA_VERSION: u32 = 1;
pub const CARGO_UNLOAD_BATCH_UNITS: u64 = 25;
pub const CARGO_UNLOAD_BATCH_MS: u64 = 500;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CargoUnloadDestination {
    Personal,
    Corporation,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CargoUnloadPhase {
    Unloading,
    Held,
    Completed,
    Cancelled,
}
impl CargoUnloadPhase {
    pub fn active(self) -> bool {
        matches!(self, Self::Unloading | Self::Held)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Unloading => "Unloading",
            Self::Held => "Unloading held",
            Self::Completed => "Unloading complete",
            Self::Cancelled => "Unloading cancelled",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoUnloadBatchReceipt {
    pub sequence: u64,
    pub units: u64,
    pub committed_at_ms: u64,
    pub ledger_sequence: u64,
    pub ledger_hash: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoUnloadOperation {
    pub operation_id: u64,
    pub pilot: String,
    pub vessel_id: String,
    pub ship_class: ShipClass,
    pub system_id: u32,
    pub berth_id: u16,
    pub destination: CargoUnloadDestination,
    /// Personal pilot identity or exact corporation identity, fixed on acceptance.
    pub destination_owner: String,
    pub requested_units: u64,
    pub committed_units: u64,
    /// Still in the source hold; never counted as a second inventory.
    pub reserved_units: u64,
    pub phase: CargoUnloadPhase,
    pub next_batch_at_ms: u64,
    pub job_id: Option<u64>,
    pub message: String,
    pub receipts: Vec<CargoUnloadBatchReceipt>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoUnloadingState {
    pub schema_version: u32,
    /// Accepted operation IDs must be exactly this value + 1. Never rewound.
    pub high_watermark: u64,
    /// Durable journal; no eviction in this bounded preview. Exact retries remain inspectable.
    pub operations: Vec<CargoUnloadOperation>,
}
impl Default for CargoUnloadingState {
    fn default() -> Self {
        Self {
            schema_version: CARGO_UNLOADING_SCHEMA_VERSION,
            high_watermark: 0,
            operations: vec![],
        }
    }
}
impl CargoUnloadingState {
    pub fn active(&self) -> Option<&CargoUnloadOperation> {
        self.operations.iter().find(|op| op.phase.active())
    }
    pub fn latest(&self) -> Option<&CargoUnloadOperation> {
        self.operations.last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetsView, Request};
    #[test]
    fn legacy_assets_omit_capability_and_new_requests_roundtrip() {
        let assets: AssetsView = serde_json::from_value(
            serde_json::json!({"ships":[],"storage":[],"corporation":null,"invitations":[]}),
        )
        .unwrap();
        assert!(assets.cargo_unloading.is_none());
        assert!(serde_json::to_value(assets)
            .unwrap()
            .get("cargo_unloading")
            .is_none());
        for request in [
            Request::StartCargoUnload {
                operation_id: 9,
                units: 57,
                destination: CargoUnloadDestination::Corporation,
            },
            Request::CancelCargoUnload { operation_id: 9 },
            Request::ResumeCargoUnload { operation_id: 9 },
        ] {
            let wire = serde_json::to_value(&request).unwrap();
            let decoded: Request = serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
        }
        assert_eq!(
            CargoUnloadingState::default().schema_version,
            CARGO_UNLOADING_SCHEMA_VERSION
        );
    }
    #[test]
    fn only_held_and_unloading_phases_reserve_inventory() {
        assert!(CargoUnloadPhase::Held.active());
        assert!(CargoUnloadPhase::Unloading.active());
        assert!(!CargoUnloadPhase::Completed.active());
        assert!(!CargoUnloadPhase::Cancelled.active());
    }
}
