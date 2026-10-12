//! First room-control slice. Ratings and pressure rates are gameplay parameters,
//! not real vessel engineering specifications or a gas/thermal solver.
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VentMode {
    Hold,
    Equalize,
    Exhaust,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PioneerRoomCommand {
    Breaker { closed: bool },
    Light { on: bool },
    Vent { mode: VentMode },
    Door { open: bool, manual: bool },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PioneerRoomState {
    pub vessel_id: String,
    pub commissioned: bool,
    pub breaker_closed: bool,
    pub light_commanded: bool,
    pub vent_commanded: VentMode,
    pub door_commanded_open: bool,
    pub door_manual: bool,
    pub light_on: bool,
    pub vent_running: bool,
    pub door_open: bool,
    pub emergency_light_on: bool,
    pub pressure_pa: u32,
    pub emergency_remaining_ms: u64,
    pub updated_at_ms: u64,
    pub interlock: Option<String>,
}
impl PioneerRoomState {
    pub fn uncommissioned(vessel_id: String) -> Self {
        Self {
            vessel_id,
            commissioned: false,
            breaker_closed: false,
            light_commanded: true,
            vent_commanded: VentMode::Hold,
            door_commanded_open: true,
            door_manual: false,
            light_on: false,
            vent_running: false,
            door_open: true,
            emergency_light_on: false,
            pressure_pa: ROOM_PRESSURE_PA,
            emergency_remaining_ms: ROOM_EMERGENCY_MS,
            updated_at_ms: 0,
            interlock: Some("Room controls not commissioned".into()),
        }
    }
}
pub const ROOM_PRESSURE_PA: u32 = 101_000;
pub const ROOM_DOOR_DELTA_PA: u32 = 5_000;
pub const ROOM_EMERGENCY_MS: u64 = 600_000;
pub const ROOM_LIGHT_W: u64 = 24;
pub const ROOM_VENT_W: u64 = 96;
pub const ROOM_CONTROLLER_W: u64 = 4;
#[derive(Debug, Clone, Copy)]
pub struct PioneerRoomDefinition {
    pub room_id: &'static str,
    pub deck: u8,
    pub door_min: [f64; 3],
    pub door_max: [f64; 3],
    pub breaker_position: [f64; 3],
    pub light_position: [f64; 3],
    pub vent_position: [f64; 3],
    pub emergency_position: [f64; 3],
}
pub const PIONEER_CONNECTED_ROOM: PioneerRoomDefinition = PioneerRoomDefinition {
    room_id: "pioneer.deck1.airlock",
    deck: 1,
    door_min: [7.9666666667, 0.0, 5.65],
    door_max: [9.3666666667, 3.2, 5.8],
    breaker_position: [4.25, 1.4, 3.0],
    light_position: [8.6666666667, 3.05, 3.0],
    vent_position: [10.8, 2.7, 0.3],
    emergency_position: [8.6666666667, 2.7, 5.55],
};
