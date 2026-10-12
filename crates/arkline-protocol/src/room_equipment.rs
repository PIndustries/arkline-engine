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

/// Emergency output/recharge power; reserve duration is tracked by the controller.
pub const ROOM_EMERGENCY_W: u64 = 10;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum EquipmentKind {
    Supply,
    Controller,
    Breaker,
    Light,
    EmergencyLight,
    Vent,
    PressureSensor,
    Door,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PortKind {
    Power,
    Command,
    Telemetry,
    Interlock,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PortDirection {
    Input,
    Output,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EquipmentPort {
    pub id: &'static str,
    pub kind: PortKind,
    pub direction: PortDirection,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EquipmentDefinition {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: EquipmentKind,
    /// None denotes a logical model endpoint, not an installed physical fixture.
    pub position: Option<[f64; 3]>,
    /// Conditional operating load; None means not separately metered, not zero.
    pub active_load_w: Option<u64>,
    pub reserve_j: Option<u64>,
    pub ports: &'static [EquipmentPort],
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct EquipmentConnection {
    pub from_equipment: &'static str,
    pub from_port: &'static str,
    pub to_equipment: &'static str,
    pub to_port: &'static str,
    pub kind: PortKind,
}
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RoomEquipmentCatalog {
    pub schema_version: u32,
    pub room_id: &'static str,
    pub equipment: &'static [EquipmentDefinition],
    pub connections: &'static [EquipmentConnection],
}
const fn input(id: &'static str, kind: PortKind) -> EquipmentPort {
    EquipmentPort {
        id,
        kind,
        direction: PortDirection::Input,
    }
}
const fn output(id: &'static str, kind: PortKind) -> EquipmentPort {
    EquipmentPort {
        id,
        kind,
        direction: PortDirection::Output,
    }
}
pub const PIONEER_ROOM_EQUIPMENT: &[EquipmentDefinition] = &[
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.supply",
        label: "Ship reactor / capacitor endpoint",
        kind: EquipmentKind::Supply,
        position: None,
        active_load_w: None,
        reserve_j: None,
        ports: &[output("power", PortKind::Power)],
    },
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.breaker",
        label: "Room mains breaker",
        kind: EquipmentKind::Breaker,
        position: Some(PIONEER_CONNECTED_ROOM.breaker_position),
        active_load_w: None,
        reserve_j: None,
        ports: &[
            input("supply", PortKind::Power),
            output("switched", PortKind::Power),
            output("status", PortKind::Telemetry),
        ],
    },
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.controller",
        label: "Bounded room controller",
        kind: EquipmentKind::Controller,
        position: None,
        active_load_w: Some(ROOM_CONTROLLER_W),
        reserve_j: None,
        ports: &[
            input("power", PortKind::Power),
            input("breaker", PortKind::Telemetry),
            input("pressure", PortKind::Telemetry),
            output("command", PortKind::Command),
            output("safe_open", PortKind::Interlock),
        ],
    },
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.light",
        label: "Main room light",
        kind: EquipmentKind::Light,
        position: Some(PIONEER_CONNECTED_ROOM.light_position),
        active_load_w: Some(ROOM_LIGHT_W),
        reserve_j: None,
        ports: &[
            input("power", PortKind::Power),
            input("command", PortKind::Command),
        ],
    },
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.emergency",
        label: "Emergency light and reserve",
        kind: EquipmentKind::EmergencyLight,
        position: Some(PIONEER_CONNECTED_ROOM.emergency_position),
        active_load_w: Some(ROOM_EMERGENCY_W),
        reserve_j: Some(ROOM_EMERGENCY_MS * ROOM_EMERGENCY_W / 1000),
        ports: &[
            input("charge", PortKind::Power),
            input("mains_status", PortKind::Telemetry),
        ],
    },
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.vent",
        label: "Scalar equalize / exhaust control",
        kind: EquipmentKind::Vent,
        position: Some(PIONEER_CONNECTED_ROOM.vent_position),
        active_load_w: Some(ROOM_VENT_W),
        reserve_j: None,
        ports: &[
            input("power", PortKind::Power),
            input("command", PortKind::Command),
        ],
    },
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.pressure",
        label: "Scalar room pressure measurement",
        kind: EquipmentKind::PressureSensor,
        position: None,
        active_load_w: None,
        reserve_j: None,
        ports: &[output("pressure", PortKind::Telemetry)],
    },
    EquipmentDefinition {
        id: "pioneer.deck1.airlock.door",
        label: "Interlocked corridor door",
        kind: EquipmentKind::Door,
        position: Some([
            (PIONEER_CONNECTED_ROOM.door_min[0] + PIONEER_CONNECTED_ROOM.door_max[0]) / 2.0,
            (PIONEER_CONNECTED_ROOM.door_min[1] + PIONEER_CONNECTED_ROOM.door_max[1]) / 2.0,
            (PIONEER_CONNECTED_ROOM.door_min[2] + PIONEER_CONNECTED_ROOM.door_max[2]) / 2.0,
        ]),
        active_load_w: None,
        reserve_j: None,
        ports: &[
            input("power", PortKind::Power),
            input("command", PortKind::Command),
            input("safe_open", PortKind::Interlock),
        ],
    },
];
macro_rules! link {
    ($a:literal,$ap:literal,$b:literal,$bp:literal,$kind:ident) => {
        EquipmentConnection {
            from_equipment: concat!("pioneer.deck1.airlock.", $a),
            from_port: $ap,
            to_equipment: concat!("pioneer.deck1.airlock.", $b),
            to_port: $bp,
            kind: PortKind::$kind,
        }
    };
}
pub const PIONEER_ROOM_CONNECTIONS: &[EquipmentConnection] = &[
    link!("supply", "power", "breaker", "supply", Power),
    link!("breaker", "switched", "controller", "power", Power),
    link!("breaker", "switched", "light", "power", Power),
    link!("breaker", "switched", "vent", "power", Power),
    link!("breaker", "switched", "door", "power", Power),
    link!("breaker", "switched", "emergency", "charge", Power),
    link!("breaker", "status", "controller", "breaker", Telemetry),
    link!("breaker", "status", "emergency", "mains_status", Telemetry),
    link!("pressure", "pressure", "controller", "pressure", Telemetry),
    link!("controller", "command", "light", "command", Command),
    link!("controller", "command", "vent", "command", Command),
    link!("controller", "command", "door", "command", Command),
    link!("controller", "safe_open", "door", "safe_open", Interlock),
];
pub fn pioneer_room_catalog() -> RoomEquipmentCatalog {
    RoomEquipmentCatalog {
        schema_version: 1,
        room_id: PIONEER_CONNECTED_ROOM.room_id,
        equipment: PIONEER_ROOM_EQUIPMENT,
        connections: PIONEER_ROOM_CONNECTIONS,
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    #[test]
    fn generated_json_matches_authoritative_catalog() {
        let checked: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/pioneer-room-catalog.json")).unwrap();
        assert_eq!(
            checked,
            serde_json::to_value(pioneer_room_catalog()).unwrap()
        );
    }
    #[test]
    fn catalog_ids_and_typed_connections_are_complete_and_unique() {
        let c = pioneer_room_catalog();
        let mut ids = std::collections::BTreeSet::new();
        for e in c.equipment {
            assert!(ids.insert(e.id));
            assert!(e.id.starts_with(c.room_id));
            let mut ports = std::collections::BTreeSet::new();
            for p in e.ports {
                assert!(ports.insert(p.id));
            }
        }
        let mut edges = std::collections::BTreeSet::new();
        for e in c.connections {
            assert!(edges.insert((e.from_equipment, e.from_port, e.to_equipment, e.to_port)));
            for (id, port, direction) in [
                (e.from_equipment, e.from_port, PortDirection::Output),
                (e.to_equipment, e.to_port, PortDirection::Input),
            ] {
                let node = c
                    .equipment
                    .iter()
                    .find(|n| n.id == id)
                    .expect("connection node");
                let p = node
                    .ports
                    .iter()
                    .find(|p| p.id == port)
                    .expect("connection port");
                assert_eq!(p.kind, e.kind);
                assert_eq!(p.direction, direction);
            }
        }
        for node in c.equipment {
            for p in node.ports {
                assert!(
                    c.connections
                        .iter()
                        .any(|e| (e.from_equipment == node.id && e.from_port == p.id)
                            || (e.to_equipment == node.id && e.to_port == p.id)),
                    "unconnected {}.{}",
                    node.id,
                    p.id
                );
            }
        }
    }
    #[test]
    fn catalog_ratings_and_positions_reference_implemented_definitions() {
        let c = pioneer_room_catalog();
        let get = |kind| c.equipment.iter().find(|e| e.kind == kind).unwrap();
        assert_eq!(get(EquipmentKind::Light).active_load_w, Some(ROOM_LIGHT_W));
        assert_eq!(get(EquipmentKind::Vent).active_load_w, Some(ROOM_VENT_W));
        assert_eq!(
            get(EquipmentKind::Controller).active_load_w,
            Some(ROOM_CONTROLLER_W)
        );
        assert_eq!(
            get(EquipmentKind::EmergencyLight).reserve_j,
            Some(ROOM_EMERGENCY_MS * ROOM_EMERGENCY_W / 1000)
        );
        assert_eq!(
            get(EquipmentKind::Light).position,
            Some(PIONEER_CONNECTED_ROOM.light_position)
        );
        assert_eq!(
            get(EquipmentKind::Vent).position,
            Some(PIONEER_CONNECTED_ROOM.vent_position)
        );
        for e in c.equipment {
            if let Some(p) = e.position {
                assert!(p.iter().all(|v| v.is_finite()));
                assert!(
                    (0.0..=32.0).contains(&p[0])
                        && (0.0..=3.2).contains(&p[1])
                        && (0.0..=14.0).contains(&p[2])
                );
            }
        }
        assert!(get(EquipmentKind::Door).active_load_w.is_none());
    }
}
