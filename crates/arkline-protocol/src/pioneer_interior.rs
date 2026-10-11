//! First implemented deck, local meters (X=plan U, Z=plan V, floor Y=0).
//! Derived from Pioneer proposed deck-one room/door bounds; other decks remain design-only.
use crate::{StationObstacle, StationObstacleKind};
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
    let mut v = Vec::new();
    {
        let mut wall = |x, z, w, d| {
            v.push(StationObstacle {
                kind: StationObstacleKind::Wall,
                min: [x, 0.0, z],
                max: [x + w, 3.2, z + d],
            })
        };
        wall(0.0, 0.0, 32.0, 0.15);
        wall(0.0, 13.85, 32.0, 0.15);
        wall(0.0, 0.0, 0.15, 14.0);
        wall(31.85, 0.0, 0.15, 14.0);
        for (_, x, z) in PIONEER_ROOMS {
            // 1.4 m clear openings match the design; wall thickness fits room margins.
            let door = x + 8.9333333333 / 2.0;
            let face = if z < 7.0 { 5.65 } else { 8.2 };
            wall(x, face, door - 0.7 - x, 0.15);
            wall(door + 0.7, face, x + 8.9333333333 - door - 0.7, 0.15);
            wall(x - 0.15, z, 0.15, 5.4);
            wall(x + 8.9333333333, z, 0.15, 5.4);
        }
    }
    // Visible non-operational equipment enclosures; collision shares their exact boxes.
    for (_, x, z) in PIONEER_ROOMS.into_iter().skip(1) {
        v.push(StationObstacle {
            kind: StationObstacleKind::Storage,
            min: [x + 1.0, 0.0, z + 1.0],
            max: [x + 2.8, 1.4, z + 1.8],
        });
    }
    v
}
