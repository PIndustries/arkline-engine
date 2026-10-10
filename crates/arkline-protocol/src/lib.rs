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
/// Legacy fixed interval used only when migrating older flight records.
pub const WARP_DURATION_MS: u64 = 6_000;
/// Initial part of transit spent charging before visual travel begins.
pub const WARP_WARMUP_MS: u64 = 2_000;
/// Authoritative f64 coordinates are meters. Subtract a local origin before f32 rendering.
pub const AU_METERS: f64 = 149_597_870_700.0;
pub const MAX_WARP_SPEED: f64 = AU_METERS;
pub const WARP_ACCELERATION: f64 = AU_METERS * 0.5;
pub const STATION_POSITION: [f64; 3] = [-4500.0, 0.0, 0.0];
pub const BELT_POSITION: [f64; 3] = [2.0 * AU_METERS, 0.0, 0.0];
pub const MAX_SHIP_SPEED: f64 = 600.0;
pub const SHIP_ACCELERATION: f64 = 150.0;
pub const DOCK_RANGE: f64 = 1800.0;
pub const MINING_RANGE: f64 = 4500.0;
pub const SPACE_BOUND: f64 = 100.0 * AU_METERS;
pub const HOME_SYSTEM_ID: u32 = 1;
pub const GATE_RANGE: f64 = 1800.0;
pub const JUMP_DURATION_MS: u64 = 3000;
pub const LOCK_RANGE: f64 = 12000.0;
pub fn default_system_id() -> u32 {
    HOME_SYSTEM_ID
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StarSystem {
    pub id: u32,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stargate {
    pub id: u64,
    pub system_id: u32,
    pub destination_system_id: u32,
    pub arrival_gate_id: u64,
    pub name: String,
    pub position: [f64; 3],
    pub radius: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asteroid {
    pub id: u64,
    pub system_id: u32,
    pub name: String,
    pub position: [f64; 3],
    pub radius: f64,
    pub ore: u64,
}
pub fn systems() -> Vec<StarSystem> {
    [(1, "Junction"), (2, "Ember Reach"), (3, "Glassway")]
        .into_iter()
        .map(|(id, name)| StarSystem {
            id,
            name: name.into(),
        })
        .collect()
}
pub fn gates() -> Vec<Stargate> {
    [
        (
            101,
            1,
            2,
            201,
            "Ember Gate",
            [8.0 * AU_METERS, 0.0, -4.0 * AU_METERS],
        ),
        (
            201,
            2,
            1,
            101,
            "Junction Gate",
            [-8.0 * AU_METERS, 0.0, -4.0 * AU_METERS],
        ),
        (
            202,
            2,
            3,
            301,
            "Glassway Gate",
            [8.0 * AU_METERS, 0.0, 4.0 * AU_METERS],
        ),
        (
            301,
            3,
            2,
            202,
            "Ember Gate",
            [-8.0 * AU_METERS, 0.0, 4.0 * AU_METERS],
        ),
    ]
    .into_iter()
    .map(
        |(id, system_id, destination_system_id, arrival_gate_id, name, position)| Stargate {
            id,
            system_id,
            destination_system_id,
            arrival_gate_id,
            name: name.into(),
            position,
            radius: 400.0,
        },
    )
    .collect()
}
/// Allocate exactly the supplied remaining global reserve during initial migration.
pub fn asteroid_catalog(total_ore: u64) -> Vec<Asteroid> {
    (0..108u64)
        .map(|index| {
            let local = index % 36;
            let angle = local as f64 * 2.399963;
            let ring = 500.0 + (local % 6) as f64 * 300.0;
            Asteroid {
                id: index + 1,
                system_id: (index / 36 + 1) as u32,
                name: format!("Ore asteroid {}", index + 1),
                position: [
                    BELT_POSITION[0] + angle.cos() * ring,
                    ((local * 7) % 9) as f64 * 100.0 - 400.0,
                    angle.sin() * ring,
                ],
                radius: 80.0 + (local % 5) as f64 * 25.0,
                ore: total_ore / 108 + u64::from(index < total_ore % 108),
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NavigationTarget {
    Station,
    Belt,
    Pilot { pilot: String },
    Asteroid { id: u64 },
    Gate { id: u64 },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum NavigationMode {
    Stop,
    Approach {
        target: NavigationTarget,
    },
    Orbit {
        target: NavigationTarget,
        radius: f64,
    },
    KeepRange {
        target: NavigationTarget,
        distance: f64,
    },
    FlyTo {
        position: [f64; 3],
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightState {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub heading: [f64; 3],
    pub mode: NavigationMode,
    pub updated_at_ms: u64,
    #[serde(default)]
    pub warp: Option<WarpTrajectory>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WarpTrajectory {
    pub source: [f64; 3],
    pub target: [f64; 3],
    pub started_at_ms: u64,
    pub warmup_ms: u64,
    pub acceleration_ms: u64,
    pub cruise_ms: u64,
    pub arrives_at_ms: u64,
}
/// A symmetric acceleration/cruise/deceleration trajectory with finite warmup.
/// Durations round upward; sampling derives speed from the exact distance so
/// positions meet precisely at the persisted deadline without discontinuity.
pub fn plan_warp(source: [f64; 3], target: [f64; 3], started_at_ms: u64) -> WarpTrajectory {
    let distance = source
        .iter()
        .zip(target)
        .map(|(a, b)| (b - a).powi(2))
        .sum::<f64>()
        .sqrt();
    let acceleration_seconds = (distance / WARP_ACCELERATION)
        .sqrt()
        .min(MAX_WARP_SPEED / WARP_ACCELERATION);
    let acceleration_ms = (acceleration_seconds * 1000.0).ceil().max(1.0) as u64;
    let cruise_ms =
        ((distance / MAX_WARP_SPEED - acceleration_seconds).max(0.0) * 1000.0).ceil() as u64;
    let arrives_at_ms = started_at_ms
        .saturating_add(WARP_WARMUP_MS)
        .saturating_add(2 * acceleration_ms)
        .saturating_add(cruise_ms);
    WarpTrajectory {
        source,
        target,
        started_at_ms,
        warmup_ms: WARP_WARMUP_MS,
        acceleration_ms,
        cruise_ms,
        arrives_at_ms,
    }
}
/// Sample authoritative meters and meters/second without f32 precision loss.
pub fn sample_warp(path: &WarpTrajectory, now_ms: u64) -> ([f64; 3], [f64; 3]) {
    if now_ms >= path.arrives_at_ms {
        return (path.target, [0.0; 3]);
    }
    let elapsed = now_ms
        .saturating_sub(path.started_at_ms)
        .saturating_sub(path.warmup_ms) as f64
        / 1000.0;
    let acceleration = path.acceleration_ms.max(1) as f64 / 1000.0;
    let cruise = path.cruise_ms as f64 / 1000.0;
    let total = 2.0 * acceleration + cruise;
    let (fraction, rate) = if elapsed < acceleration {
        (
            0.5 * elapsed * elapsed / acceleration / (acceleration + cruise),
            elapsed / acceleration / (acceleration + cruise),
        )
    } else if elapsed < acceleration + cruise {
        (
            (0.5 * acceleration + elapsed - acceleration) / (acceleration + cruise),
            1.0 / (acceleration + cruise),
        )
    } else {
        let remaining = (total - elapsed).max(0.0);
        (
            1.0 - 0.5 * remaining * remaining / acceleration / (acceleration + cruise),
            remaining / acceleration / (acceleration + cruise),
        )
    };
    let delta: [f64; 3] = std::array::from_fn(|i| path.target[i] - path.source[i]);
    (
        std::array::from_fn(|i| path.source[i] + delta[i] * fraction.clamp(0.0, 1.0)),
        delta.map(|d| d * rate),
    )
}

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
    LockAsteroid {
        id: u64,
    },
    MineAsteroid {
        id: u64,
        units: u64,
    },
    Jump {
        gate_id: u64,
    },
    Navigate {
        mode: NavigationMode,
    },
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
    Gate { id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Location {
    Docked,
    Jumping {
        gate_id: u64,
        arrives_at_ms: u64,
    },
    Station,
    Belt,
    InTransit {
        destination: Destination,
        arrives_at_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PilotView {
    pub name: String,
    pub credits: u64,
    pub ore: u64,
    pub location: Location,
    #[serde(default)]
    pub flight: Option<FlightState>,
    #[serde(default = "default_system_id")]
    pub system_id: u32,
    #[serde(default)]
    pub locked_asteroid: Option<u64>,
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
    #[serde(default)]
    pub asteroids: Vec<Asteroid>,
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

#[cfg(test)]
mod warp_tests {
    use super::*;
    fn norm(v: [f64; 3]) -> f64 {
        v.iter().map(|x| x * x).sum::<f64>().sqrt()
    }
    #[test]
    fn physical_trajectory_is_continuous_bounded_and_distance_dependent() {
        let source = [AU_METERS * 20.0 + 0.125, 10.0, 20.0];
        for distance in [1000.0, AU_METERS * 0.1, AU_METERS * 2.0, AU_METERS * 12.0] {
            let target = [source[0] + distance, source[1], source[2]];
            let path = plan_warp(source, target, 1000);
            assert_eq!(sample_warp(&path, 1000).0, source);
            assert_eq!(sample_warp(&path, 1000 + path.warmup_ms).0, source);
            assert_eq!(sample_warp(&path, path.arrives_at_ms), (target, [0.0; 3]));
            let mut previous = sample_warp(&path, 1000);
            for t in (1100..path.arrives_at_ms).step_by(100) {
                let sample = sample_warp(&path, t);
                assert!(sample.0[0] >= previous.0[0]);
                assert!(norm(sample.1) <= MAX_WARP_SPEED * 1.00000001);
                let dv = std::array::from_fn(|i| sample.1[i] - previous.1[i]);
                assert!(norm(dv) <= WARP_ACCELERATION * 0.10000001);
                previous = sample;
            }
            for boundary in [
                1000 + path.warmup_ms + path.acceleration_ms,
                1000 + path.warmup_ms + path.acceleration_ms + path.cruise_ms,
            ] {
                let a = sample_warp(&path, boundary - 1);
                let b = sample_warp(&path, boundary + 1);
                assert!(
                    norm(std::array::from_fn(|i| b.0[i] - a.0[i])) <= MAX_WARP_SPEED * 0.00200001
                );
            }
        }
        assert!(
            plan_warp([0.0; 3], [12.0 * AU_METERS, 0.0, 0.0], 0).arrives_at_ms
                > plan_warp([0.0; 3], [2.0 * AU_METERS, 0.0, 0.0], 0).arrives_at_ms
        );
    }
}
