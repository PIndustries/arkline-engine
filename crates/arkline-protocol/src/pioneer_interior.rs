//! Two implemented decks, local meters (X=plan U, Z=plan V, floor Y=0).
//! Derived from the approved Pioneer room/door bounds. The first airlock controller is defined separately in room_equipment; other utilities remain proposed.
use crate::StationObstacle;
pub const PIONEER_INTERIOR_SIZE: [f64; 2] = [32.0, 14.0];
pub const PIONEER_INTERIOR_ENTRY: [f64; 3] = [8.6666666667, 0.0, 3.0];
pub const PIONEER_ROOMS: [(&str, f64, f64); 6] = [
    ("AIRLOCK / EVA", 4.2, 0.2),
    ("POWER CONTROL", 4.2, 8.4),
    ("DRONE WORKSHOP", 13.5333333333, 0.2),
    ("CARGO ACCESS", 13.5333333333, 8.4),
    ("STORES / SERVICE", 22.8666666667, 0.2),
    ("WORKSHOP", 22.8666666667, 8.4),
];
pub fn pioneer_interior_obstacles() -> Vec<StationObstacle> {
    crate::default_pioneer().obstacles(1)
}

/// Fixed landing in the reserved access core; the user cannot choose arbitrary coordinates.
pub const PIONEER_LIFT_POINT: [f64; 3] = [2.0, 0.0, 7.0];
pub const PIONEER_LIFT_RANGE: f64 = 1.2;
pub const PIONEER_DECK_TWO_ROOMS: [(&str, f64, f64); 6] = [
    ("NAV / PLANNING", 4.2, 0.2),
    ("CREW / GALLEY", 4.2, 8.4),
    ("SENSORS / COMMS", 13.5333333333, 0.2),
    ("MEDICAL / REST", 13.5333333333, 8.4),
    ("OPERATIONS", 22.8666666667, 0.2),
    ("LIFE SUPPORT", 22.8666666667, 8.4),
];
pub fn pioneer_rooms(deck: u8) -> &'static [(&'static str, f64, f64); 6] {
    if deck == 2 {
        &PIONEER_DECK_TWO_ROOMS
    } else {
        &PIONEER_ROOMS
    }
}
