//! Arkline v0.1 wire format and canonical economic event encoding.
//! One JSON request/response per line over a local TCP connection.
mod editor_scene;
pub use editor_scene::*;
mod world_layout;
pub use world_layout::*;
mod fleet_layout;
pub use fleet_layout::*;
mod editor_graph;
pub use editor_graph::*;
mod power_accounting;
pub use power_accounting::*;
mod room_equipment;
pub use room_equipment::*;
mod exterior_effects;
pub use exterior_effects::*;
mod pioneer_interior;
pub use pioneer_interior::*;
pub mod pioneer_docked;
pub use pioneer_docked::*;
mod reactor_configuration;
pub use reactor_configuration::*;
mod turret_stow;
pub use turret_stow::*;
mod cockpit_mechanisms;
pub mod parallel_turret_aim;
pub mod parallel_turret_binding;
pub mod parallel_turret_ray_geometry;
pub mod turret_stow_geometry;
pub use cockpit_mechanisms::*;
pub mod carrier_camera_devices;
pub mod carrier_exterior;
mod carrier_inventory;
pub mod carrier_inventory_geometry;
mod drone_bay;
mod drone_bay_batches;
mod drone_cargo;
pub mod independent_bay_geometry;
mod independent_bays;
pub mod parallel_exterior;
pub use carrier_inventory::*;
pub use drone_bay_batches::*;
pub use drone_cargo::*;
pub use independent_bays::*;
pub mod drone_bay_definition;
pub mod drone_bay_geometry;
pub mod drone_bay_layout;
pub mod drone_dropoff_geometry;
pub use drone_bay::*;
mod cargo_unloading;
pub use cargo_unloading::*;
mod docking;
pub use docking::*;
mod docked_traversal;
pub use docked_traversal::*;
mod room_layout;
pub use room_layout::*;
pub mod material_compile;
pub mod material_graph;
pub use material_compile::{
    lower_material, LoweredMaterial, MaterialCosts, MaterialSourceSpan, MaterialUniformSlot,
};
pub use material_graph::{
    AssetMaterialBinding, MaterialDiagnostic, MaterialDiagnosticCode, MaterialGraphDocument,
    MaterialNode, MaterialNodeId, MaterialNodeOp, MaterialParameter, MaterialParameterId,
    MaterialParameterValue, MaterialValueType, PIONEER_MATERIAL_SHA256,
};
mod navigation;
pub use navigation::*;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum total ore requested across repeated resource-job hauls.
pub const MAX_RESOURCE_JOB_UNITS: u64 = 1_000_000;
/// User-facing year.month.day.build identity; trim the trailing file newline.
pub const BUILD_VERSION: &str = include_str!("../../../BUILD_VERSION");

pub const PROTOCOL_VERSION: u32 = 1;
// Exterior effects remain additive v1: an optional snapshot advertises support,
// and its independently versioned definition is required before new commands.
pub const RULESET_VERSION: u32 = 1;
pub const STARTER_CREDITS: u64 = 1_000;
pub const INITIAL_BELT_ORE: u64 = 100_000;
pub const CARGO_CAPACITY: u64 = 1_000;
pub const MAX_MINE_UNITS: u64 = 100;
/// Authoritative f64 coordinates are meters. Subtract a local origin before f32 rendering.
pub const AU_METERS: f64 = 149_597_870_700.0;
pub const STATION_POSITION: [f64; 3] = [-4500.0, 0.0, 0.0];
/// Station geometry surrounds a docking port on its positive-X surface.
pub const STATION_DIAMETER_M: f64 = 20_000.0;
pub const STATION_HEIGHT_M: f64 = 6_000.0;
pub const STATION_CENTER_POSITION: [f64; 3] =
    [STATION_POSITION[0] - STATION_DIAMETER_M / 2.0, 0.0, 0.0];

pub const BELT_POSITION: [f64; 3] = [2.0 * AU_METERS, 0.0, 0.0];
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

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Request {
    DockedTraversal {
        request: DockedTraversalRequest,
    },
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
        #[serde(default)]
        sprint: bool,
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
    CarrierInventory {
        request: CarrierInventoryRequest,
    },
    IndependentBays {
        request: IndependentBayRequest,
    },
    DroneBay {
        request: DroneBayRequest,
    },
    DroneCargo {
        request: DroneCargoRequest,
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
    StartCargoUnload {
        operation_id: u64,
        units: u64,
        destination: CargoUnloadDestination,
    },
    CancelCargoUnload {
        operation_id: u64,
    },
    ResumeCargoUnload {
        operation_id: u64,
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
    SetTurretDeployment {
        vessel_id: String,
        deployed: bool,
    },
    SetCockpitScreenMode {
        vessel_id: String,
        target: CockpitScreenTarget,
        mode: CockpitScreenMode,
    },
    SetPioneerExterior {
        enabled: bool,
        expected_revision: u64,
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
        /// Highest independently versioned world layout this client can render.
        #[serde(default)]
        world_layout_version: u32,
        #[serde(default)]
        supports_room_controls: bool,
        #[serde(default)]
        supports_fleet_hangar: bool,
        #[serde(default)]
        supports_pioneer_interior: bool,
        #[serde(default)]
        supports_docked_traversal_v2: bool,
    },
    Connect {
        /// Missing on older clients, which can only use legacy world geometry.
        #[serde(default)]
        world_layout_version: u32,
        #[serde(default)]
        supports_room_controls: bool,
        #[serde(default)]
        supports_fleet_hangar: bool,
        /// Missing on older clients; only visual clients with interior support opt in.
        #[serde(default)]
        supports_pioneer_interior: bool,
        #[serde(default)]
        supports_docked_traversal_v2: bool,
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
    CancelDocking {
        operation_id: u64,
    },
    ResumeDocking {
        operation_id: u64,
    },
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
            Self::Pioneer => ("Pioneer", 72.0, 38.0, 22.8),
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
    /// Full world attitude for source-bound physical drones. Legacy fleets use heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orientation_xyzw: Option<[f64; 4]>,
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
    /// Source-bound finite carrier locations and confirmed machinery; absent on old worlds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_inventory: Option<CarrierInventoryPublicState>,
    /// Fresh cargo-world public payload/receiver projection; absent means unreported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone_cargo: Option<DroneCargoPublicState>,
    /// Additive response-only accounting of the existing electrical model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_accounting: Option<PowerAccountingView>,
    /// Additive public body/machinery projection for a source-registered bay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone_bay: Option<DroneBayPublicState>,
    #[serde(default)]
    pub cockpit_mechanisms: Option<PioneerCockpitMechanisms>,
    #[serde(default)]
    pub reactor_configuration: Option<PioneerReactorConfiguration>,
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
pub const SPRINT_SPEED_MPS: f64 = 6.0;
pub const STATION_JUMP_SPEED_MPS: f64 = 4.5;
pub const STATION_GRAVITY_MPS2: f64 = 9.81;
pub const STATION_WALK_LEASE_MS: u64 = 500;
pub const STATION_BOARD_RANGE_M: f64 = 3.0;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StationAvatar {
    /// Negotiated canonical station-world feet. Legacy fields remain a stopped hangar projection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docked_traversal: Option<DockedTraversalState>,
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
    #[serde(default)]
    pub sprinting: bool,
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
    /// Additive response-only accounting of the existing electrical model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_accounting: Option<PowerAccountingView>,
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
    /// Authenticated owner journal; never borrow this counter for a viewed remote vessel.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_inventory: Option<CarrierInventoryState>,
    /// Authenticated owner's cargo journal, never a remote viewed vessel's counter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone_cargo: Option<DroneCargoState>,
    /// Owned finite stock and accepted operation journal; absent on legacy realms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone_bay: Option<DroneBayState>,
    /// Additive v1 capability and private receipts; absent on legacy realms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cargo_unloading: Option<CargoUnloadingState>,
    #[serde(default)]
    pub pioneer_room: Option<PioneerRoomState>,
    /// Host-authoritative, validated room geometry when a project layout is active.
    #[serde(default)]
    pub pioneer_room_layout: Option<RoomLayoutState>,
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
    /// Fresh explicit-roster world marker, including empty worlds; never inferred from a class quota.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carrier_inventory_registration: Option<CarrierInventoryRegistration>,
    /// Durable feature advertisement remains present even before any pilot exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone_cargo_registration: Option<DroneCargoRegistration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone_cargo_totals: Option<DroneOreTotals>,
    /// Authority supports the additive sprint flag on both walking routes.
    #[serde(default)]
    pub walk_sprint_v1: bool,
    /// Absent on authorities without this exact additive registration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docked_traversal_capability: Option<DockedTraversalCapability>,
    /// The authority's immutable realm layout. Absence means exact legacy v1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_layout: Option<WorldLayoutDefinition>,
    /// Presence advertises additive exterior schema support; older servers omit it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exterior: Option<ExteriorWorldState>,
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
    PhysicalCargo {
        record: physical_cargo::ledger::PhysicalCargoEvent,
    },
    CarrierInventory {
        record: CarrierInventoryLedgerEvent,
    },
    DroneCargo {
        record: DroneCargoLedgerEvent,
    },
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
    pub physical_cargo: Option<physical_cargo::control::PhysicalControlPublicView>,
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
            physical_cargo: None,
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
        assert!(snapshot.exterior.is_none());
        assert!(snapshot.world_layout.is_none());
    }
    #[test]
    fn old_admission_and_fleet_snapshots_keep_legacy_layout_capabilities() {
        let connect: Request = serde_json::from_value(serde_json::json!({
            "command": "connect", "protocol_version": 1,
            "pilot": "legacy", "resume_token": null
        }))
        .unwrap();
        assert!(matches!(
            connect,
            Request::Connect {
                world_layout_version: 0,
                ..
            }
        ));
        let account: Request = serde_json::from_value(serde_json::json!({
            "command": "account_connect", "protocol_version": 1,
            "pilot": "legacy", "access_token": "disposable-test-token"
        }))
        .unwrap();
        assert!(matches!(
            account,
            Request::AccountConnect {
                world_layout_version: 0,
                ..
            }
        ));
        let fleet: HangarView = serde_json::from_value(serde_json::json!({
            "layout": fleet_hangar_layout(), "berths": []
        }))
        .unwrap();
        assert_eq!(fleet.layout_revision, FleetLayoutRevision::LinearV1);
    }
    #[test]
    fn exterior_commands_require_revision_and_are_not_remotely_delegable() {
        assert!(serde_json::from_value::<Request>(
            serde_json::json!({"command":"set_pioneer_exterior","enabled":true})
        )
        .is_err());
        let request:Request=serde_json::from_value(serde_json::json!({"command":"set_pioneer_exterior","enabled":true,"expected_revision":0})).unwrap();
        assert!(!request.is_remote_gameplay());
        assert_eq!(PROTOCOL_VERSION, 1);
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
/// Authored asymmetric body bounds. The class height is a conservative,
/// origin-centred 22.8m envelope; the actual hull is 20.35m tall.
pub const PIONEER_BODY_BOUNDS_M: [[f64; 3]; 2] = [[-36.0, -9.0, -19.0], [36.0, 11.35, 19.0]];

/// Raised-bridge refit sockets in body metres, shared by gun and camera poses.
/// These are geometry anchors, not new weapon or structural simulation.
pub const PIONEER_MINING_TURRET_MOUNTS_M: [[f64; 3]; 2] = [[10.8, 9.864, -5.2], [10.8, 9.864, 5.2]];

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drone_cargo: Option<DroneCargoJobProgress>,
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
            Self::CarrierInventory { .. }
                | Self::IndependentBays { .. }
                | Self::DroneBay { .. }
                | Self::DroneCargo { .. }
                | Self::OrbitDrone { .. }
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
    /// Missing on old snapshots; their fleet uses the original linear concourse.
    #[serde(default)]
    pub layout_revision: FleetLayoutRevision,
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

#[cfg(test)]
mod navigation_organization_tests {
    use super::*;

    #[test]
    #[ignore = "writes exact navigation wire and numeric organization baseline"]
    fn export_navigation_organization_fingerprint() {
        use serde_json::json;
        let targets = [
            NavigationTarget::Station,
            NavigationTarget::Belt,
            NavigationTarget::Pilot {
                pilot: "fixture-pilot".into(),
            },
            NavigationTarget::Asteroid { id: 20 },
            NavigationTarget::Gate { id: 2 },
        ];
        let modes = [
            NavigationMode::Stop,
            NavigationMode::Manual,
            NavigationMode::Approach {
                target: targets[0].clone(),
            },
            NavigationMode::Orbit {
                target: targets[3].clone(),
                radius: 500.0,
            },
            NavigationMode::KeepRange {
                target: targets[2].clone(),
                distance: 1200.0,
            },
            NavigationMode::FlyTo {
                position: [1.0, -2.0, 3.0],
            },
        ];
        let destinations = [
            Destination::Station,
            Destination::Belt,
            Destination::Gate { id: 2 },
        ];
        let locations = [
            Location::Docked,
            Location::Jumping {
                gate_id: 2,
                arrives_at_ms: 5000,
            },
            Location::Station,
            Location::Belt,
            Location::InTransit {
                destination: Destination::Gate { id: 2 },
                arrives_at_ms: 7000,
            },
        ];
        let flight_json = r#"{"position":[1.0,2.0,3.0],"velocity":[4.0,5.0,6.0],"heading":[1.0,0.0,0.0],"mode":{"mode":"stop"},"updated_at_ms":1000}"#;
        let flight: FlightState = serde_json::from_str(flight_json).unwrap();
        assert_eq!(flight.orientation, identity_orientation());
        assert_eq!(flight.acceleration, [0.0; 3]);
        assert_eq!(flight.angular_velocity, [0.0; 3]);
        assert!(flight.warp.is_none() && flight.docking.is_none());
        let old_warp_json = r#"{"source":[0.0,0.0,0.0],"target":[1000.0,0.0,0.0],"started_at_ms":1000,"warmup_ms":2000,"acceleration_ms":1000,"cruise_ms":0,"arrives_at_ms":5000}"#;
        let old: WarpTrajectory = serde_json::from_str(old_warp_json).unwrap();
        assert_eq!((old.launch_ms, old.arrival_ms), (0, 0));
        assert!(old.initial_heading.is_none());
        let mut paths = vec![old];
        for (source, target) in [
            ([0.0; 3], [0.0; 3]),
            ([0.0; 3], [1.0, 0.0, 0.0]),
            ([0.0; 3], [30_000.0, 0.0, 0.0]),
            ([0.0; 3], [2.0 * AU_METERS, 0.0, 0.0]),
            ([0.0; 3], [12.0 * AU_METERS, 0.0, 0.0]),
            (
                [2.0 * AU_METERS, -150.0, 1000.0],
                [-AU_METERS, 2.0 * AU_METERS, 3000.0],
            ),
        ] {
            for heading in [
                None,
                Some([1.0, 0.0, 0.0]),
                Some([-1.0, 0.0, 0.0]),
                Some([0.0, 1.0, 0.0]),
            ] {
                let mut p = plan_warp(source, target, 1000);
                p.initial_heading = heading;
                paths.push(p);
            }
        }
        let trajectories=paths.iter().map(|p| {
            let start=p.started_at_ms;
            let warm=start+p.warmup_ms;
            let launch=warm+p.launch_ms;
            let accel=launch+p.acceleration_ms;
            let cruise=accel+p.cruise_ms;
            let arrival=p.arrives_at_ms.saturating_sub(p.arrival_ms);
            let boundaries=[start,warm,launch,accel,cruise,arrival,p.arrives_at_ms];
            let mut times=std::collections::BTreeSet::new();
            for t in boundaries {times.extend([t.saturating_sub(1),t,t.saturating_add(1)]);}
            for pair in boundaries.windows(2) {times.insert(pair[0]+pair[1].saturating_sub(pair[0])/2);}
            let samples=times.into_iter().map(|t|{let (position,velocity)=sample_warp(p,t);json!({"time_ms":t,"position_bits":position.map(f64::to_bits),"velocity_bits":velocity.map(f64::to_bits),"heading_bits":sample_warp_heading(p,t).map(f64::to_bits)})}).collect::<Vec<_>>();
            json!({"serialized":serde_json::to_string(p).unwrap(),"samples":samples})
        }).collect::<Vec<_>>();
        let report = json!({"format":1,"targets":targets.iter().map(|x|serde_json::to_string(x).unwrap()).collect::<Vec<_>>(),"modes":modes.iter().map(|x|serde_json::to_string(x).unwrap()).collect::<Vec<_>>(),"destinations":destinations.iter().map(|x|serde_json::to_string(x).unwrap()).collect::<Vec<_>>(),"locations":locations.iter().map(|x|serde_json::to_string(x).unwrap()).collect::<Vec<_>>(),"legacy_flight_input":flight_json,"legacy_flight_output":serde_json::to_string(&flight).unwrap(),"legacy_warp_input":old_warp_json,"constants":{"warp_duration":WARP_DURATION_MS,"warp_warmup":WARP_WARMUP_MS,"max_warp_speed_bits":MAX_WARP_SPEED.to_bits(),"warp_acceleration_bits":WARP_ACCELERATION.to_bits(),"max_ship_speed_bits":MAX_SHIP_SPEED.to_bits(),"ship_acceleration_bits":SHIP_ACCELERATION.to_bits(),"pilot_lease":PILOT_INPUT_LEASE_MS,"angular_speed_bits":MAX_ANGULAR_SPEED.to_bits()},"trajectories":trajectories});
        std::fs::write(
            std::env::var_os("ARKLINE_NAVIGATION_FINGERPRINT").expect("explicit output"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
    }
}

/// Fresh measured-volume foundation; no source factory or legacy migration.
pub mod physical_cargo;
