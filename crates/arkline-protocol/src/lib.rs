//! Arkline v0.1 wire format and canonical economic event encoding.
//! One JSON request/response per line over a local TCP connection.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// User-facing year.month.day.build identity; trim the trailing file newline.
pub const BUILD_VERSION: &str = include_str!("../../../BUILD_VERSION");

pub const PROTOCOL_VERSION: u32 = 1;
pub const RULESET_VERSION: u32 = 1;
pub const STARTER_CREDITS: u64 = 1_000;
pub const INITIAL_BELT_ORE: u64 = 100_000;
pub const CARGO_CAPACITY: u64 = 1_000;
pub const MAX_MINE_UNITS: u64 = 100;
/// Total authoritative transit interval, including the initial warmup.
pub const WARP_DURATION_MS: u64 = 6_000;
/// Initial part of transit spent charging before visual travel begins.
pub const WARP_WARMUP_MS: u64 = 2_000;
pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    Connect {
        protocol_version: u32,
        pilot: String,
        resume_token: Option<String>,
    },
    Snapshot,
    Undock,
    Warp {
        destination: Destination,
    },
    Dock,
    Mine {
        units: u64,
    },
    PlaceSell {
        units: u64,
        unit_price: u64,
    },
    Buy {
        order_id: u64,
        units: u64,
    },
    CancelSell {
        order_id: u64,
    },
    ExportLedger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Destination {
    Station,
    Belt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Location {
    Docked,
    Station,
    Belt,
    InTransit {
        destination: Destination,
        arrives_at_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PilotView {
    pub name: String,
    pub credits: u64,
    pub ore: u64,
    pub location: Location,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SellOrder {
    pub id: u64,
    pub seller: String,
    pub units: u64,
    pub unit_price: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub server_time_ms: u64,
    /// Ephemeral authenticated transport presence; excluded from economic replay.
    #[serde(default)]
    pub online_pilots: Vec<String>,
    pub pilots: Vec<PilotView>,
    pub orders: Vec<SellOrder>,
    pub belt_ore: u64,
    pub ledger_sequence: u64,
    pub ledger_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum EconomicEvent {
    PilotCreated {
        pilot: String,
    },
    OreMined {
        pilot: String,
        units: u64,
    },
    SellPlaced {
        order_id: u64,
        seller: String,
        units: u64,
        unit_price: u64,
    },
    TradeExecuted {
        order_id: u64,
        buyer: String,
        units: u64,
    },
    SellCancelled {
        order_id: u64,
        seller: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub sequence: u64,
    pub ruleset_version: u32,
    pub timestamp_ms: u64,
    pub previous_hash: String,
    pub event: EconomicEvent,
    pub hash: String,
}

impl LedgerEntry {
    /// Ordered tuple encoding avoids map ordering differences. Schema changes
    /// require a new ruleset and explicit verifier support.
    pub fn calculate_hash(&self) -> String {
        let bytes = serde_json::to_vec(&(
            self.sequence,
            self.ruleset_version,
            self.timestamp_ms,
            &self.previous_hash,
            &self.event,
        ))
        .expect("economic event contains only JSON-safe primitives");
        hex::encode(Sha256::digest(bytes))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerExport {
    pub protocol_version: u32,
    pub entries: Vec<LedgerEntry>,
    pub snapshot: Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ledger: Option<LedgerExport>,
}

impl Response {
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
            resume_token: None,
            snapshot: None,
            ledger: None,
        }
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;

    #[test]
    fn older_snapshots_default_to_no_reported_presence() {
        let json = serde_json::json!({
            "server_time_ms": 0,
            "pilots": [],
            "orders": [],
            "belt_ore": INITIAL_BELT_ORE,
            "ledger_sequence": 0,
            "ledger_hash": GENESIS_HASH,
        });
        let snapshot: Snapshot = serde_json::from_value(json).unwrap();
        assert!(snapshot.online_pilots.is_empty());
    }
}
