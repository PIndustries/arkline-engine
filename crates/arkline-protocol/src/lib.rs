//! Arkline v0.1 wire format and canonical economic event encoding.
//! One JSON request/response per line over a local TCP connection.
mod room_equipment;
pub use room_equipment::*;
mod pioneer_interior;
pub use pioneer_interior::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum total ore requested across repeated resource-job hauls.
pub const MAX_RESOURCE_JOB_UNITS: u64 = 1_000_000;
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
/// Station geometry surrounds a docking port on its positive-X surface.
pub const STATION_DIAMETER_M: f64 = 20_000.0;
pub const STATION_HEIGHT_M: f64 = 6_000.0;
pub const STATION_CENTER_POSITION: [f64; 3] =
    [STATION_POSITION[0] - STATION_DIAMETER_M / 2.0, 0.0, 0.0];

pub const BELT_POSITION: [f64; 3] = [2.0 * AU_METERS, 0.0, 0.0];
pub const MAX_SHIP_SPEED: f64 = 600.0;
pub const SHIP_ACCELERATION: f64 = 150.0;
pub const PILOT_INPUT_LEASE_MS: u64 = 500;
pub const MAX_ANGULAR_SPEED: f64 = std::f64::consts::FRAC_PI_2;
pub fn identity_orientation() -> [f64; 4] {
    [0.0, 0.0, 0.0, 1.0]
}
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
    Manual,
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
    /// Actual thruster acceleration from the last normal-space integration step.
    #[serde(default)]
    pub acceleration: [f64; 3],
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub heading: [f64; 3],
    /// Unit quaternion [x,y,z,w], body axes +X forward, +Y up, +Z right.
    #[serde(default = "identity_orientation")]
    pub orientation: [f64; 4],
    /// Body-axis angular velocity (roll X, yaw Y, pitch Z), radians/second.
    #[serde(default)]
    pub angular_velocity: [f64; 3],
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
    #[serde(default)]
    pub arrival_ms: u64,
    #[serde(default)]
    pub arrival_distance_m: f64,
    #[serde(default)]
    pub launch_ms: u64,
    #[serde(default)]
    pub launch_distance_m: f64,
    #[serde(default)]
    pub initial_heading: Option<[f64; 3]>,
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
    let arrival_ms = if distance > 300_000.0 { 6_000 } else { 0 };
    let arrival_distance_m = if arrival_ms > 0 { 30_000.0 } else { 0.0 };
    let launch_ms = arrival_ms;
    let launch_distance_m = arrival_distance_m;
    let main_distance = distance - arrival_distance_m - launch_distance_m;
    let acceleration_seconds = (main_distance / WARP_ACCELERATION)
        .sqrt()
        .min(MAX_WARP_SPEED / WARP_ACCELERATION);
    let acceleration_ms = (acceleration_seconds * 1000.0).ceil().max(1.0) as u64;
    let cruise_ms =
        ((main_distance / MAX_WARP_SPEED - acceleration_seconds).max(0.0) * 1000.0).ceil() as u64;
    let arrives_at_ms = started_at_ms
        .saturating_add(WARP_WARMUP_MS)
        .saturating_add(2 * acceleration_ms)
        .saturating_add(cruise_ms)
        .saturating_add(arrival_ms)
        .saturating_add(launch_ms);
    WarpTrajectory {
        source,
        target,
        started_at_ms,
        warmup_ms: WARP_WARMUP_MS,
        acceleration_ms,
        cruise_ms,
        arrives_at_ms,
        arrival_ms,
        arrival_distance_m,
        launch_ms,
        launch_distance_m,
        initial_heading: None,
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
    if path.arrival_ms > 0 && path.arrival_distance_m > 0.0 {
        let delta: [f64; 3] = std::array::from_fn(|i| path.target[i] - path.source[i]);
        let distance = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
        if distance <= 0.0 {
            return (path.target, [0.0; 3]);
        }
        let direction = delta.map(|v| v / distance);
        let terminal = path.arrival_distance_m.min(distance);
        let launch = path.launch_distance_m.min((distance - terminal).max(0.0));
        let launch_duration = path.launch_ms as f64 / 1000.0;
        let launch_speed = if launch_duration > 0.0 {
            2.0 * launch / launch_duration
        } else {
            0.0
        };
        if elapsed < launch_duration {
            let u = (elapsed / launch_duration).clamp(0.0, 1.0);
            let traveled = launch * (2.0 * u.powi(3) - u.powi(4));
            let speed = launch_speed * (3.0 * u * u - 2.0 * u.powi(3));
            return (
                std::array::from_fn(|i| path.source[i] + direction[i] * traveled),
                direction.map(|v| v * speed),
            );
        }
        let elapsed = elapsed - launch_duration;
        let duration = path.arrival_ms as f64 / 1000.0;
        let entry_speed = 2.0 * terminal / duration;
        if elapsed >= total {
            let u = ((elapsed - total) / duration).clamp(0.0, 1.0);
            let traveled = terminal * (2.0 * u - 2.0 * u.powi(3) + u.powi(4));
            let speed = entry_speed * (1.0 - 3.0 * u * u + 2.0 * u.powi(3));
            // Target-relative computation preserves sub-meter local precision.
            return (
                std::array::from_fn(|i| path.target[i] - direction[i] * (terminal - traveled)),
                direction.map(|v| v * speed),
            );
        }
        let main_distance = distance - terminal - launch;
        let peak = (main_distance - 0.5 * (entry_speed + launch_speed) * acceleration)
            / (acceleration + cruise);
        let (traveled, speed) = if elapsed < acceleration {
            (
                launch_speed * elapsed
                    + 0.5 * (peak - launch_speed) * elapsed * elapsed / acceleration,
                launch_speed + (peak - launch_speed) * elapsed / acceleration,
            )
        } else if elapsed < acceleration + cruise {
            (
                0.5 * (peak + launch_speed) * acceleration + peak * (elapsed - acceleration),
                peak,
            )
        } else {
            let remaining = total - elapsed;
            let decel = (peak - entry_speed) / acceleration;
            (
                main_distance - entry_speed * remaining - 0.5 * decel * remaining * remaining,
                entry_speed + decel * remaining,
            )
        };
        return (
            std::array::from_fn(|i| path.source[i] + direction[i] * (launch + traveled)),
            direction.map(|v| v * speed),
        );
    }
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

/// Fixed-duration alignment uses a continuous spherical turn, including 180 degrees.
pub fn sample_warp_heading(path: &WarpTrajectory, now_ms: u64) -> [f64; 3] {
    fn normalize(v: [f64; 3]) -> [f64; 3] {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        if n < 1e-12 {
            [1.0, 0.0, 0.0]
        } else {
            v.map(|x| x / n)
        }
    }
    let target = normalize(std::array::from_fn(|i| path.target[i] - path.source[i]));
    let Some(initial) = path.initial_heading else {
        return target;
    };
    let initial = normalize(initial);
    let t = (now_ms.saturating_sub(path.started_at_ms) as f64 / path.warmup_ms.max(1) as f64)
        .clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let dot = initial
        .iter()
        .zip(target)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .clamp(-1.0, 1.0);
    if dot > 0.9995 {
        return normalize(std::array::from_fn(|i| {
            initial[i] * (1.0 - t) + target[i] * t
        }));
    }
    if dot < -0.9995 {
        let basis = if initial[1].abs() < 0.9 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let perpendicular = normalize([
            initial[1] * basis[2] - initial[2] * basis[1],
            initial[2] * basis[0] - initial[0] * basis[2],
            initial[0] * basis[1] - initial[1] * basis[0],
        ]);
        return normalize(std::array::from_fn(|i| {
            initial[i] * (std::f64::consts::PI * t).cos()
                + perpendicular[i] * (std::f64::consts::PI * t).sin()
        }));
    }
    let angle = dot.acos();
    normalize(std::array::from_fn(|i| {
        (initial[i] * (angle * (1.0 - t)).sin() + target[i] * (angle * t).sin()) / angle.sin()
    }))
}

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    StationSit {
        seated: bool,
    },
    DisembarkShip,
    EnterShip,
    EnterInterior,
    ExitInterior,
    InteriorLift {
        deck: u8,
    },
    StationJump,
    StationWalk {
        forward: f64,
        strafe: f64,
        turn: f64,
    },
    OrbitDrone {
        id: u64,
        target: NavigationTarget,
    },
    BeginRemoteControl {
        pilot: String,
    },
    EndRemoteControl,
    RemoteControl {
        pilot: String,
        request: Box<Request>,
    },
    StartResourceJob {
        target_units: u64,
        #[serde(default)]
        pilot: Option<String>,
    },
    CancelResourceJob {
        #[serde(default)]
        pilot: Option<String>,
    },
    SetFleetController {
        controller: Option<String>,
    },
    SpawnSurfaceProp {
        kind: SurfacePropKind,
        position: [f64; 3],
    },
    MoveSurfaceProp {
        id: u64,
        position: [f64; 3],
    },
    RemoveSurfaceProp {
        id: u64,
    },
    LaunchDroneClass {
        class: DroneClass,
    },
    RecallDroneById {
        id: u64,
    },
    DroneHoldById {
        id: u64,
    },
    DroneNavigateById {
        id: u64,
        position: [f64; 3],
    },
    AssignMiningDrone {
        id: u64,
        target_id: u64,
    },
    AssignAttackDrone {
        id: u64,
        target_pilot: String,
        target_drone_id: u64,
    },
    UnlockAsteroid {
        id: u64,
    },
    LaunchDrone,
    RecallDrone,
    DroneHold,
    DroneNavigate {
        position: [f64; 3],
    },
    StartMining {
        module_id: u8,
        target_id: u64,
    },
    StopMining {
        module_id: u8,
    },
    DepositOre {
        units: u64,
    },
    WithdrawOre {
        units: u64,
    },
    CreateCorporation {
        name: String,
    },
    InviteCorporation {
        pilot: String,
    },
    AcceptCorporation {
        corporation: String,
    },
    DepositCorporationOre {
        units: u64,
    },
    WithdrawCorporationOre {
        units: u64,
    },
    SetPioneerRoom {
        action: PioneerRoomCommand,
    },
    BoardShip {
        id: String,
    },
    AuthCapabilities,
    AccountConnect {
        access_token: String,
        pilot: String,
        #[serde(default)]
        legacy_resume_token: Option<String>,
        #[serde(default)]
        link_existing: bool,
        protocol_version: u32,
        #[serde(default)]
        supports_room_controls: bool,
        #[serde(default)]
        supports_fleet_hangar: bool,
        #[serde(default)]
        supports_pioneer_interior: bool,
    },
    Connect {
        #[serde(default)]
        supports_room_controls: bool,
        #[serde(default)]
        supports_fleet_hangar: bool,
        /// Missing on older clients; only visual clients with interior support opt in.
        #[serde(default)]
        supports_pioneer_interior: bool,
        protocol_version: u32,
        pilot: String,
        resume_token: Option<String>,
    },
    Snapshot,
    UnlockTarget,
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
    PilotInput {
        translation: [f64; 3],
        rotation: [f64; 3],
    },
    SelectShip {
        ship_class: ShipClass,
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

/// Prototype hull specifications; movement dynamics are shared for now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShipClass {
    #[default]
    Pioneer,
    Prospector,
    Hauler,
    Vanguard,
    Borealis,
    Stratum,
    Monolith,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShipSpec {
    pub name: &'static str,
    pub cargo_capacity: u64,
    pub mining_tier: Option<u8>,
    pub length_m: f64,
    pub width_m: f64,
    pub height_m: f64,
}

pub const ALL_SHIP_CLASSES: [ShipClass; 7] = [
    ShipClass::Pioneer,
    ShipClass::Prospector,
    ShipClass::Hauler,
    ShipClass::Vanguard,
    ShipClass::Borealis,
    ShipClass::Stratum,
    ShipClass::Monolith,
];

impl ShipClass {
    pub const fn spec(self) -> ShipSpec {
        let (name, length_m, width_m, height_m) = match self {
            Self::Pioneer => ("Pioneer", 72.0, 38.0, 18.0),
            Self::Prospector => ("Prospector", 180.0, 92.0, 64.0),
            Self::Hauler => ("Hauler", 420.0, 180.0, 140.0),
            Self::Vanguard => ("Vanguard", 1100.0, 380.0, 260.0),
            Self::Borealis => ("Borealis", 640.0, 260.0, 180.0),
            Self::Stratum => ("Stratum", 1800.0, 800.0, 480.0),
            Self::Monolith => ("Monolith", 5200.0, 2200.0, 1400.0),
        };
        ShipSpec {
            cargo_capacity: cargo_capacity(self),
            mining_tier: match self {
                Self::Pioneer => Some(1),
                Self::Prospector => Some(2),
                Self::Borealis => Some(3),
                Self::Stratum => Some(4),
                Self::Monolith => Some(5),
                _ => None,
            },
            name,
            length_m,
            width_m,
            height_m,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetLock {
    pub asteroid_id: u64,
    pub started_at_ms: u64,
    pub completes_at_ms: u64,
}

/// Larger signatures acquire faster; heavier hull sensors acquire more slowly.
pub fn lock_duration_ms(ship: ShipClass, radius: f64) -> u64 {
    let sensor_ms = match ship {
        ShipClass::Pioneer => 4000.0,
        ShipClass::Prospector => 3200.0,
        ShipClass::Hauler => 5600.0,
        ShipClass::Vanguard => 6400.0,
        ShipClass::Borealis => 3600.0,
        ShipClass::Stratum => 4200.0,
        ShipClass::Monolith => 5000.0,
    };
    let radius = if radius.is_finite() {
        radius.max(1.0)
    } else {
        1.0
    };
    (sensor_ms * (100.0 / radius).sqrt())
        .clamp(2000.0, 8000.0)
        .ceil() as u64
}

/// Tunable prototype energy values. Charge is joules; reactor output is watts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerSpec {
    pub capacity_j: u64,
    pub regen_w: u64,
    pub warp_cost_j: u64,
    pub mining_cycle_cost_j: u64,
    pub drone_mining_cycle_cost_j: u64,
    pub drone_attack_cost_j: u64,
}
pub fn power_spec(class: ShipClass) -> PowerSpec {
    let (capacity_j, regen_w, warp_cost_j, mining_cycle_cost_j) = match class {
        ShipClass::Pioneer => (240_000, 1200, 30_000, 4000),
        ShipClass::Prospector => (360_000, 1600, 45_000, 6000),
        ShipClass::Hauler => (600_000, 2200, 70_000, 5000),
        ShipClass::Vanguard => (1_200_000, 4000, 120_000, 8000),
        ShipClass::Borealis => (900_000, 3000, 90_000, 9000),
        ShipClass::Stratum => (2_400_000, 7000, 240_000, 16000),
        ShipClass::Monolith => (8_000_000, 16000, 800_000, 24000),
    };
    PowerSpec {
        capacity_j,
        regen_w,
        warp_cost_j,
        mining_cycle_cost_j,
        drone_mining_cycle_cost_j: 1500,
        drone_attack_cost_j: 2000,
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerState {
    pub charge_j: u64,
    pub capacity_j: u64,
    pub regen_w: u64,
    pub updated_at_ms: u64,
    /// Fractional joule carry, always less than 1000. Prevents polling-dependent regeneration.
    pub regen_remainder_mj: u64,
}

pub const MAX_TARGET_LOCKS: usize = 5;
pub const DRONE_CLASS_CAP: usize = 5;
pub const MAX_DRONES: usize = 15;
pub const DRONE_HIT_POINTS: u32 = 100;
pub const DRONE_ATTACK_DAMAGE: u32 = 10;
pub const DRONE_ATTACK_INTERVAL_MS: u64 = 1000;
pub const DRONE_ATTACK_RANGE: f64 = 1000.0;
pub const DRONE_MINING_CYCLE_MS: u64 = 8000;
pub const DRONE_MINING_YIELD: u64 = 10;
pub const DRONE_MINING_RANGE: f64 = 500.0;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DroneClass {
    #[default]
    Recon,
    Mining,
    Attack,
}
pub fn drone_speed(class: DroneClass) -> f64 {
    match class {
        DroneClass::Recon => 150.0,
        DroneClass::Mining => 100.0,
        DroneClass::Attack => 200.0,
    }
}
pub fn drone_signal_strength(drone: [f64; 3], ship: [f64; 3]) -> f64 {
    let distance = drone
        .iter()
        .zip(ship)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt();
    (1.0 - distance / DRONE_MAX_RANGE).clamp(0.0, 1.0)
}
fn default_drone_hit_points() -> u32 {
    DRONE_HIT_POINTS
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroneMiningCycle {
    pub started_at_ms: u64,
    pub completes_at_ms: u64,
}
pub const DRONE_MAX_RANGE: f64 = 10_000.0;
pub const DRONE_SPEED: f64 = 150.0;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DroneMode {
    Orbit {
        target: NavigationTarget,
    },
    Mining {
        target_id: u64,
    },
    Attacking {
        target_pilot: String,
        target_drone_id: u64,
    },
    Hold,
    FlyTo {
        position: [f64; 3],
    },
    Returning,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroneView {
    #[serde(default)]
    pub id: u64,
    #[serde(default)]
    pub class: DroneClass,
    #[serde(default = "default_drone_hit_points")]
    pub hit_points: u32,
    #[serde(default)]
    pub signal_strength: f64,
    #[serde(default)]
    pub mining_cycle: Option<DroneMiningCycle>,
    #[serde(default)]
    pub attack_ready_at_ms: Option<u64>,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub heading: [f64; 3],
    pub mode: DroneMode,
    pub updated_at_ms: u64,
}

pub const MINING_MODULE_IDS: [u8; 2] = [0, 1];
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MiningSpec {
    pub cycle_ms: u64,
    pub yield_units: u64,
}
pub fn mining_spec(class: ShipClass) -> MiningSpec {
    let (cycle_ms, yield_units) = match class {
        ShipClass::Pioneer => (6000, 25),
        ShipClass::Prospector => (5000, 40),
        ShipClass::Hauler => (8000, 20),
        ShipClass::Vanguard => (7000, 30),
        ShipClass::Borealis => (5000, 75),
        ShipClass::Stratum => (4000, 100),
        ShipClass::Monolith => (3000, 100),
    };
    MiningSpec {
        cycle_ms,
        yield_units,
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiningModule {
    pub module_id: u8,
    pub target_id: Option<u64>,
    pub cycle_started_at_ms: Option<u64>,
    pub cycle_completes_at_ms: Option<u64>,
    pub stop_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PilotView {
    #[serde(default)]
    pub station_avatar: Option<StationAvatar>,
    #[serde(default)]
    pub surface: Option<SurfaceState>,
    #[serde(default)]
    pub power: Option<PowerState>,
    #[serde(default)]
    pub drones: Vec<DroneView>,
    #[serde(default)]
    pub target_locks: Vec<TargetLock>,
    #[serde(default)]
    pub locked_asteroids: Vec<u64>,
    #[serde(default)]
    pub drone: Option<DroneView>,
    #[serde(default)]
    pub mining_modules: Vec<MiningModule>,
    #[serde(default)]
    pub target_lock: Option<TargetLock>,
    #[serde(default)]
    pub ship_class: ShipClass,
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

pub const AVATAR_HEIGHT_M: f64 = 1.8;
pub const WALK_SPEED_MPS: f64 = 4.0;
pub const STATION_JUMP_SPEED_MPS: f64 = 4.5;
pub const STATION_GRAVITY_MPS2: f64 = 9.81;
pub const STATION_WALK_LEASE_MS: u64 = 500;
pub const STATION_BOARD_RANGE_M: f64 = 3.0;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StationAvatar {
    /// Pioneer deck-one local coordinates when true; private hangar otherwise.
    #[serde(default)]
    pub onboard_interior: bool,
    /// Interior deck identity; feet height remains local to that deck.
    #[serde(default = "default_interior_deck")]
    pub interior_deck: u8,
    /// Authoritative start time; absent when grounded. Older saves default to grounded.
    #[serde(default)]
    pub jump_started_ms: Option<u64>,
    #[serde(default)]
    pub seated: bool,
    /// Feet position in the private hangar, meters; floor is y=0.
    pub position: [f64; 3],
    /// Radians; zero faces +X, positive turns toward +Z.
    pub heading: f64,
    pub moving: bool,
    pub updated_at_ms: u64,
}
fn default_interior_deck() -> u8 {
    1
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct HangarLayout {
    pub half_length: f64,
    pub half_width: f64,
    pub height: f64,
    pub ship_center: [f64; 3],
    pub boarding_point: [f64; 3],
}
pub fn hangar_layout(class: ShipClass) -> HangarLayout {
    let spec = class.spec();
    HangarLayout {
        half_length: spec.length_m / 2.0 + 20.0,
        half_width: spec.width_m / 2.0 + 20.0,
        height: spec.height_m + 20.0,
        ship_center: [0.0, spec.height_m / 2.0, 0.0],
        boarding_point: [0.0, 0.0, spec.width_m / 2.0 + 3.0],
    }
}

/// A human-scale room in the positive-Z margin of each private hangar.
#[derive(Debug, Clone, Copy)]
pub struct QuartersLayout {
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub doorway_center: [f64; 3],
    pub doorway_width: f64,
    pub seat_point: [f64; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StationObstacleKind {
    Wall,
    Bed,
    Desk,
    Sofa,
    Table,
    Storage,
}
#[derive(Debug, Clone, Copy)]
pub struct StationObstacle {
    pub kind: StationObstacleKind,
    pub min: [f64; 3],
    pub max: [f64; 3],
}
pub const STATION_AVATAR_RADIUS_M: f64 = 0.3;
pub fn quarters_layout(class: ShipClass) -> QuartersLayout {
    let z = hangar_layout(class).half_width;
    QuartersLayout {
        min: [-9.0, 0.0, z - 13.0],
        max: [9.0, 3.2, z - 1.0],
        doorway_center: [0.0, 0.0, z - 13.0],
        doorway_width: 4.0,
        seat_point: [6.0, 0.0, z - 13.0 + 3.6],
    }
}
pub fn station_obstacles(class: ShipClass) -> Vec<StationObstacle> {
    use StationObstacleKind::*;
    let q = quarters_layout(class);
    let front = q.min[2];
    let back = q.max[2];
    let box_at = |kind, x0, x1, z0, z1, h| StationObstacle {
        kind,
        min: [x0, 0.0, z0],
        max: [x1, h, z1],
    };
    vec![
        box_at(Wall, -9.0, -2.0, front, front + 0.3, 3.2),
        box_at(Wall, 2.0, 9.0, front, front + 0.3, 3.2),
        box_at(Wall, -9.0, -8.7, front, back, 3.2),
        box_at(Wall, 8.7, 9.0, front, back, 3.2),
        box_at(Wall, -9.0, 9.0, back - 0.3, back, 3.2),
        box_at(Bed, -8.0, -5.0, back - 5.0, back - 1.0, 0.65),
        box_at(Desk, -7.0, -3.0, front + 1.5, front + 2.4, 0.9),
        box_at(Sofa, 4.0, 8.0, front + 2.0, front + 3.2, 1.1),
        box_at(Table, 4.5, 7.5, front + 4.0, front + 5.4, 0.6),
        box_at(Storage, 6.5, 8.2, back - 3.0, back - 0.4, 2.2),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SellOrder {
    pub id: u64,
    pub seller: String,
    pub units: u64,
    pub unit_price: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HangarShip {
    #[serde(default)]
    pub power: Option<PowerState>,
    pub ship_class: ShipClass,
    pub system_id: u32,
    pub ore: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationStorage {
    pub system_id: u32,
    pub ore: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorporationView {
    pub name: String,
    pub owner: String,
    pub members: Vec<String>,
    pub storage: Vec<StationStorage>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetsView {
    #[serde(default)]
    pub pioneer_room: Option<PioneerRoomState>,
    #[serde(default)]
    pub hangar: Option<HangarView>,
    #[serde(default)]
    pub pioneer_interior_available: bool,
    #[serde(default)]
    pub pioneer_interior_decks: u8,
    /// Inactive owned hulls only; active hull and cargo remain in PilotView.
    pub ships: Vec<HangarShip>,
    pub storage: Vec<StationStorage>,
    pub corporation: Option<CorporationView>,
    pub invitations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub remote_controls: Vec<RemoteControlLease>,
    #[serde(default)]
    pub fleet_links: Vec<FleetLink>,
    #[serde(default)]
    pub resource_jobs: Vec<ResourceJob>,
    #[serde(default)]
    pub surface_props: Vec<SurfaceProp>,
    #[serde(default)]
    pub traffic: Vec<TrafficActor>,
    #[serde(default)]
    pub stored_ore: u64,
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
    ShipBoarded {
        from_ship_class: ShipClass,
        pilot: String,
        ship_class: ShipClass,
        system_id: u32,
        stored_units: u64,
        loaded_units: u64,
    },
    PersonalOreDeposited {
        pilot: String,
        system_id: u32,
        units: u64,
    },
    PersonalOreWithdrawn {
        pilot: String,
        system_id: u32,
        units: u64,
    },
    CorporationCreated {
        corporation: String,
        owner: String,
    },
    CorporationInvited {
        inviter: String,
        corporation: String,
        pilot: String,
    },
    CorporationJoined {
        corporation: String,
        pilot: String,
    },
    CorporationOreDeposited {
        corporation: String,
        pilot: String,
        system_id: u32,
        units: u64,
    },
    CorporationOreWithdrawn {
        corporation: String,
        pilot: String,
        system_id: u32,
        units: u64,
    },
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
pub struct AuthCapabilities {
    pub issuer: String,
    pub audience: String,
    pub profile: String,
    pub required_scope: String,
    pub auth_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthCapabilities>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<AssetsView>,
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
            auth: None,
            assets: None,
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
                path.started_at_ms + path.warmup_ms + path.launch_ms,
                path.started_at_ms + path.warmup_ms + path.launch_ms + path.acceleration_ms,
                path.started_at_ms
                    + path.warmup_ms
                    + path.launch_ms
                    + path.acceleration_ms
                    + path.cruise_ms,
                path.arrives_at_ms - path.arrival_ms,
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

#[cfg(test)]
mod local_warp_tests {
    use super::*;
    fn norm(v: [f64; 3]) -> f64 {
        v.iter().map(|x| x * x).sum::<f64>().sqrt()
    }
    #[test]
    fn launch_and_arrival_are_visible_continuous_nonstop_legs() {
        let path = plan_warp([0.0; 3], [2.0 * AU_METERS, 0.0, 0.0], 1000);
        assert_eq!(path.launch_ms, 6000);
        assert_eq!(path.arrival_ms, 6000);
        let launch_start = path.started_at_ms + path.warmup_ms;
        let launch_end = launch_start + path.launch_ms;
        let arrival_start = path.arrives_at_ms - path.arrival_ms;
        let launch = sample_warp(&path, launch_end);
        assert!((launch.0[0] - 30_000.0).abs() < 0.001);
        assert!((launch.1[0] - 10_000.0).abs() < 0.001);
        let arrival = sample_warp(&path, arrival_start);
        assert!((path.target[0] - arrival.0[0] - 30_000.0).abs() < 0.001);
        assert!((arrival.1[0] - 10_000.0).abs() < 0.001);
        let departure_middle = sample_warp(&path, launch_start + 3000);
        assert!(departure_middle.0[0] > 5000.0 && departure_middle.0[0] < 6000.0);
        let arrival_middle = sample_warp(&path, arrival_start + 3000);
        assert!((path.target[0] - arrival_middle.0[0] - 5625.0).abs() < 0.001);
        for t in (launch_start + 100..path.arrives_at_ms).step_by(100) {
            assert!(
                norm(sample_warp(&path, t).1) > 0.0,
                "ship stops between phases at {t}"
            );
        }
        assert_eq!(
            sample_warp(&path, path.arrives_at_ms),
            (path.target, [0.0; 3])
        );
        // Main-profile one-sided velocity extrapolation lands on the same
        // 10 km/s boundary value; no hidden zero-speed stop/restart.
        let before = sample_warp(&path, arrival_start - 1);
        let peak = sample_warp(&path, launch_end + path.acceleration_ms).1[0];
        let decel = (peak - 10_000.0) / (path.acceleration_ms as f64 / 1000.0);
        assert!((before.1[0] - decel * 0.001 - arrival.1[0]).abs() < 0.1);
    }
    #[test]
    fn old_paths_remain_legacy_and_alignment_handles_opposite_headings() {
        let old = serde_json::json!({"source":[0.0,0.0,0.0],"target":[1000.0,0.0,0.0],"started_at_ms":0,"warmup_ms":2000,"acceleration_ms":1000,"cruise_ms":0,"arrives_at_ms":4000});
        let path: WarpTrajectory = serde_json::from_value(old).unwrap();
        assert_eq!(path.arrival_ms, 0);
        assert_eq!(path.launch_ms, 0);
        assert_eq!(
            sample_warp(&path, 3000),
            ([500.0, 0.0, 0.0], [1000.0, 0.0, 0.0])
        );
        let mut path = plan_warp([0.0; 3], [AU_METERS, 0.0, 0.0], 0);
        path.initial_heading = Some([-1.0, 0.0, 0.0]);
        assert_eq!(sample_warp_heading(&path, 0), [-1.0, 0.0, 0.0]);
        let middle = sample_warp_heading(&path, 1000);
        assert!((norm(middle) - 1.0).abs() < 1e-10);
        assert!(middle[0].abs() < 0.001);
        let end = sample_warp_heading(&path, 2000);
        assert!((end[0] - 1.0).abs() < 1e-10);
    }
}

pub const MINING_SHIP_CLASSES: [ShipClass; 5] = [
    ShipClass::Pioneer,
    ShipClass::Prospector,
    ShipClass::Borealis,
    ShipClass::Stratum,
    ShipClass::Monolith,
];
pub const fn cargo_capacity(class: ShipClass) -> u64 {
    match class {
        ShipClass::Pioneer => 1000,
        ShipClass::Prospector => 2500,
        ShipClass::Hauler => 16000,
        ShipClass::Vanguard => 4000,
        ShipClass::Borealis => 8000,
        ShipClass::Stratum => 24000,
        ShipClass::Monolith => 80000,
    }
}
pub const PLANET_CENTER_POSITION: [f64; 3] = [0.0, -6_120_000.0, 0.0];
pub const PLANET_RADIUS_M: f64 = 6_000_000.0;
pub const PLANET_ATMOSPHERE_HEIGHT_M: f64 = 60_000.0;
pub const PLANET_APPROACH_POSITION: [f64; 3] = [0.0, -115_000.0, 0.0];
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Planet {
    pub id: u64,
    pub system_id: u32,
    pub name: String,
    pub center: [f64; 3],
    pub radius_m: f64,
    pub atmosphere_height_m: f64,
}
pub fn planets() -> Vec<Planet> {
    vec![Planet {
        id: 1,
        system_id: HOME_SYSTEM_ID,
        name: "Verdant".into(),
        center: PLANET_CENTER_POSITION,
        radius_m: PLANET_RADIUS_M,
        atmosphere_height_m: PLANET_ATMOSPHERE_HEIGHT_M,
    }]
}
pub fn planet_altitude(position: [f64; 3]) -> f64 {
    position
        .iter()
        .zip(PLANET_CENTER_POSITION)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
        - PLANET_RADIUS_M
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceState {
    pub planet_id: u64,
    pub altitude_m: f64,
    pub ground_clearance_m: f64,
    pub grounded: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfacePropKind {
    Crate,
    Pylon,
    Platform,
}
impl SurfacePropKind {
    pub fn size(self) -> [f64; 3] {
        match self {
            Self::Crate => [8.0, 8.0, 8.0],
            Self::Pylon => [4.0, 20.0, 4.0],
            Self::Platform => [40.0, 2.0, 40.0],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SurfaceProp {
    pub id: u64,
    pub owner: String,
    pub system_id: u32,
    pub kind: SurfacePropKind,
    pub position: [f64; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficRole {
    StationShuttle,
    FreightTender,
    Patrol,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficPhase {
    Arriving,
    Departing,
    Holding,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrafficActor {
    pub id: u64,
    pub system_id: u32,
    pub role: TrafficRole,
    pub ship_class: ShipClass,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub heading: [f64; 3],
    pub phase: TrafficPhase,
}

/// Conservative enclosing sphere for interaction distances; not collision geometry.
pub fn hull_radius_m(class: ShipClass) -> f64 {
    let s = class.spec();
    (s.length_m * s.length_m + s.width_m * s.width_m + s.height_m * s.height_m).sqrt() / 2.0
}
pub fn hull_surface_distance(class: ShipClass, position: [f64; 3], target: [f64; 3]) -> f64 {
    (position
        .iter()
        .zip(target)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
        - hull_radius_m(class))
    .max(0.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceJobPhase {
    Undocking,
    TravelToBelt,
    AcquiringTarget,
    Mining,
    ReturningToStation,
    Docking,
    Depositing,
    Completed,
    Blocked,
    Cancelled,
}
impl ResourceJobPhase {
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Blocked | Self::Cancelled)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceJob {
    pub controller: String,
    pub id: u64,
    pub pilot: String,
    pub target_units: u64,
    pub mined_units: u64,
    pub stored_units: u64,
    pub target_id: Option<u64>,
    pub phase: ResourceJobPhase,
    pub message: String,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FleetLink {
    pub pilot: String,
    pub controller: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteControlLease {
    pub controller: String,
    pub pilot: String,
}

impl Request {
    /// Gameplay commands available under an explicit remote piloting lease.
    pub fn is_remote_gameplay(&self) -> bool {
        matches!(
            self,
            Self::OrbitDrone { .. }
                | Self::PilotInput { .. }
                | Self::Navigate { .. }
                | Self::Warp { .. }
                | Self::Jump { .. }
                | Self::Dock
                | Self::Undock
                | Self::LockAsteroid { .. }
                | Self::UnlockAsteroid { .. }
                | Self::UnlockTarget
                | Self::StartMining { .. }
                | Self::StopMining { .. }
                | Self::LaunchDroneClass { .. }
                | Self::RecallDroneById { .. }
                | Self::DroneHoldById { .. }
                | Self::DroneNavigateById { .. }
                | Self::AssignMiningDrone { .. }
                | Self::AssignAttackDrone { .. }
                | Self::LaunchDrone
                | Self::RecallDrone
                | Self::DroneHold
                | Self::DroneNavigate { .. }
        )
    }
}

/// Tunable inertial mass ratios; ore load adds up to half the empty hull mass.
pub fn ship_acceleration(class: ShipClass, ore: u64) -> f64 {
    let ratio = match class {
        ShipClass::Pioneer => 1.0,
        ShipClass::Prospector => 1.6,
        ShipClass::Hauler => 3.0,
        ShipClass::Vanguard => 6.0,
        ShipClass::Borealis => 4.0,
        ShipClass::Stratum => 9.0,
        ShipClass::Monolith => 20.0,
    };
    SHIP_ACCELERATION
        / (ratio * (1.0 + 0.5 * (ore as f64 / cargo_capacity(class) as f64).clamp(0.0, 1.0)))
}
pub fn ship_braking_acceleration(class: ShipClass, ore: u64) -> f64 {
    ship_acceleration(class, ore) * 0.4
}

/// Private, authoritative station inventory. Corporation vessels are not yet supported.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HangarView {
    pub layout: HangarLayout,
    pub berths: Vec<HangarBerth>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HangarBerth {
    pub id: String,
    pub ship_class: ShipClass,
    pub position: [f64; 3],
    pub yaw: f64,
    pub boarding_point: [f64; 3],
    pub active: bool,
    pub can_board: bool,
}
/// Fixed catalog berths preserve coordinates when boarding or moving a hull elsewhere.
pub fn fleet_berth(class: ShipClass) -> ([f64; 3], [f64; 3]) {
    let mut x = 0.0;
    for current in ALL_SHIP_CLASSES {
        let spec = current.spec();
        if current == class {
            return (
                [x, spec.height_m / 2.0, spec.width_m / 2.0 + 6.0],
                [x, 0.0, 0.0],
            );
        }
        x += spec.length_m / 2.0 + 20.0;
        let next = ALL_SHIP_CLASSES.iter().position(|c| *c == current).unwrap() + 1;
        if let Some(next) = ALL_SHIP_CLASSES.get(next) {
            x += next.spec().length_m / 2.0;
        }
    }
    unreachable!()
}
pub fn fleet_hangar_layout() -> HangarLayout {
    let last = ALL_SHIP_CLASSES.last().unwrap();
    HangarLayout {
        half_length: fleet_berth(*last).0[0] + last.spec().length_m / 2.0 + 20.0,
        half_width: 50.0,
        height: 1500.0,
        ship_center: fleet_berth(ShipClass::Pioneer).0,
        boarding_point: fleet_berth(ShipClass::Pioneer).1,
    }
}
/// Quarters use the same human-scale furnishing arrangement translated behind the concourse.
pub fn fleet_quarters_layout() -> QuartersLayout {
    let mut q = quarters_layout(ShipClass::Pioneer);
    let dz = -24.0 - q.max[2];
    q.min[2] += dz;
    q.max[2] += dz;
    q.doorway_center[2] += dz;
    q.seat_point[2] += dz;
    q
}
pub fn fleet_station_obstacles() -> Vec<StationObstacle> {
    let dz = -24.0 - quarters_layout(ShipClass::Pioneer).max[2];
    station_obstacles(ShipClass::Pioneer)
        .into_iter()
        .map(|mut o| {
            o.min[2] += dz;
            o.max[2] += dz;
            o
        })
        .collect()
}
