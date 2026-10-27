//! Versioned candidate Pioneer room layout and the shared live geometry projection.
//! The projection describes room shells and fixed prototype obstacles only; it
//! does not create equipment or utility behavior.
use crate::{
    pioneer_rooms, StationObstacle, StationObstacleKind, PIONEER_CONNECTED_ROOM,
    PIONEER_INTERIOR_SIZE,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const MAX_ROOMS: usize = 24;
const ROOM_LENGTH_M: f64 = 8.9333333333;
const ROOM_DEPTH_M: f64 = 5.4;
const ROOM_CLEAR_HEIGHT_M: f64 = 3.2;
const DOOR_WIDTH_M: f64 = 1.4;
const DOOR_HEIGHT_M: f64 = 2.4;
const DOOR_EDGE_TOLERANCE_M: f64 = 0.25;
const ROOM_SIDE_WALL_M: f64 = 0.15;
const DOOR_POSITIVE_EDGE_OFFSET_M: f64 = 0.05;
const DOOR_NEGATIVE_EDGE_OFFSET_M: f64 = 0.20;
const ACCESS_CORE_X_MAX_M: f64 = 4.0;
const CORRIDOR_MIN_Z_M: f64 = 5.8;
const CORRIDOR_MAX_Z_M: f64 = 8.1;
const SOUTH_ROOM_MAX_Z_M: f64 = CORRIDOR_MIN_Z_M - 0.20;
const NORTH_ROOM_MIN_Z_M: f64 = CORRIDOR_MAX_Z_M + 0.30;
const LIVE_MIN_ROOM_LENGTH_M: f64 = 3.0;
const LIVE_MIN_ROOM_DEPTH_M: f64 = 2.4;
const LIVE_MIN_ROOM_HEIGHT_M: f64 = 2.0;
const LIVE_MAX_ROOM_HEIGHT_M: f64 = 3.2;
const LIVE_MIN_DOOR_WIDTH_M: f64 = 0.8;
const LIVE_MIN_DOOR_HEIGHT_M: f64 = 1.9;
const LIVE_GEOMETRY_EPSILON_M: f64 = 1.0e-6;

/// `source_room_id` uses the source proposal IDs (D1-P1, etc.). `id` is this
/// document's stable local object ID; added rooms have no source ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomLayout {
    pub id: u64,
    pub source_room_id: Option<String>,
    /// Current protocol room identifier, when the runtime defines one.
    pub runtime_room_id: Option<String>,
    pub source_role: Option<String>,
    pub name: String,
    pub deck: u8,
    /// Deck-plan origin, meters from the current interior instance origin.
    pub origin_xz_m: [f64; 2],
    /// Candidate clear length, depth, and height in meters.
    pub clear_lwh_m: [f64; 3],
    /// Door center X and authored wall-plane Z, in deck-local meters.
    pub door_center_xz_m: [f64; 2],
    /// Door clear width and height in meters.
    pub door_clear_wh_m: [f64; 2],
}

impl RoomLayout {
    /// Return the normalized Z coordinate where the doorway wall is projected.
    /// Positive-Z-facing rooms use the authored upper edge plus 5 cm; rooms
    /// facing from the negative-Z side use their lower edge minus 20 cm.
    pub fn door_wall_z(&self) -> f64 {
        let z = self.origin_xz_m[1];
        let depth = self.clear_lwh_m[1];
        if (self.door_center_xz_m[1] - (z + depth)).abs() <= (self.door_center_xz_m[1] - z).abs() {
            z + depth + DOOR_POSITIVE_EDGE_OFFSET_M
        } else {
            z - DOOR_NEGATIVE_EDGE_OFFSET_M
        }
    }
    /// Generate this room's exact wall and already-prototyped storage geometry.
    /// Deck boundaries and the shared access-core enclosure belong to the deck
    /// projection. Validate the containing document before live installation.
    pub fn obstacles(&self) -> Vec<StationObstacle> {
        let mut obstacles = Vec::new();
        let [x, z] = self.origin_xz_m;
        let [length, depth, height] = self.clear_lwh_m;
        let [door_x, _door_z] = self.door_center_xz_m;
        let [door_width, _door_height] = self.door_clear_wh_m;
        let normalized_door_plane = self.door_wall_z();
        let door_left = door_x - door_width / 2.0;
        let door_right = door_x + door_width / 2.0;
        push_if_area(
            &mut obstacles,
            wall(x, normalized_door_plane, door_left - x, 0.15, height),
        );
        push_if_area(
            &mut obstacles,
            wall(
                door_right,
                normalized_door_plane,
                x + length - door_right,
                0.15,
                height,
            ),
        );
        obstacles.push(wall(x - 0.15, z, 0.15, depth, height));
        obstacles.push(wall(x + length, z, 0.15, depth, height));
        let south = z + depth <= SOUTH_ROOM_MAX_Z_M + LIVE_GEOMETRY_EPSILON_M;
        let rear_wall_z = if south { z } else { z + depth - 0.15 };
        obstacles.push(wall(x, rear_wall_z, length, 0.15, height));

        if is_prototyped_storage(self) {
            obstacles.push(StationObstacle {
                kind: StationObstacleKind::Storage,
                min: [x + 1.0, 0.0, z + 1.0],
                max: [x + 2.8, 1.4, z + 1.8],
            });
        }
        obstacles.extend(self.furnishings());
        obstacles
    }

    /// Newly fitted static galley furniture, not simulated kitchen equipment.
    /// This recipe owns both visible bodies and authoritative collision.
    pub fn furnishings(&self) -> Vec<StationObstacle> {
        if self.deck != 2
            || self.source_room_id.as_deref() != Some("D2-S1")
            || self.clear_lwh_m[0] < 7.0
            || self.clear_lwh_m[1] < 4.5
        {
            return Vec::new();
        }
        let [x, z] = self.origin_xz_m;
        let depth = self.clear_lwh_m[1];
        vec![
            StationObstacle {
                kind: StationObstacleKind::Storage,
                min: [x + 0.8, 0.0, z + depth - 0.85],
                max: [x + 3.6, 0.9, z + depth - 0.2],
            },
            StationObstacle {
                kind: StationObstacleKind::Table,
                min: [x + 5.0, 0.0, z + 2.0],
                max: [x + 6.4, 0.75, z + 3.0],
            },
            StationObstacle {
                kind: StationObstacleKind::Sofa,
                min: [x + 4.9, 0.0, z + 1.25],
                max: [x + 6.5, 0.7, z + 1.7],
            },
            StationObstacle {
                kind: StationObstacleKind::Sofa,
                min: [x + 4.9, 0.0, z + 3.3],
                max: [x + 6.5, 0.7, z + 3.75],
            },
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomLayoutDocument {
    pub schema_version: u32,
    pub next_id: u64,
    pub rooms: Vec<RoomLayout>,
}

/// The host-authoritative saved layout revision distributed with live assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomLayoutState {
    pub revision: u64,
    pub document: RoomLayoutDocument,
}

impl RoomLayoutDocument {
    /// Validates the editor draft schema and finite deck-plan geometry.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.next_id == 0 || self.rooms.len() > MAX_ROOMS {
            return Err(
                "Room layout schema must be 1, next_id positive, and contain at most 24 rooms"
                    .into(),
            );
        }
        let mut ids = HashSet::new();
        let mut source_ids = HashSet::new();
        for room in &self.rooms {
            if room.id == 0 || room.id >= self.next_id || !ids.insert(room.id) {
                return Err("Room IDs must be unique, positive, and below next_id".into());
            }
            if let Some(source_id) = &room.source_room_id {
                if source_id.trim().is_empty()
                    || source_id.len() > 32
                    || !source_ids.insert(source_id)
                {
                    return Err(format!(
                        "Room #{} has an invalid or duplicate source room ID",
                        room.id
                    ));
                }
            }
            if room
                .runtime_room_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty() || id.len() > 80)
            {
                return Err(format!(
                    "Room #{} has an invalid runtime room ID reference",
                    room.id
                ));
            }
            if room.name.trim().is_empty() || room.name.len() > 80 {
                return Err(format!(
                    "Room #{} needs a name of at most 80 bytes",
                    room.id
                ));
            }
            if room
                .source_role
                .as_ref()
                .is_some_and(|role| role.len() > 80)
            {
                return Err(format!("Room #{} source role exceeds 80 bytes", room.id));
            }
            if !(1..=2).contains(&room.deck) {
                return Err(format!("Room #{} deck must be 1 or 2", room.id));
            }
            let finite = room
                .origin_xz_m
                .iter()
                .chain(room.clear_lwh_m.iter())
                .chain(room.door_center_xz_m.iter())
                .chain(room.door_clear_wh_m.iter())
                .all(|value| value.is_finite());
            if !finite
                || room.clear_lwh_m.iter().any(|value| *value <= 0.0)
                || room.door_clear_wh_m.iter().any(|value| *value <= 0.0)
            {
                return Err(format!(
                    "Room #{} dimensions and positions must be finite; dimensions must be positive",
                    room.id
                ));
            }
            let [origin_x, origin_z] = room.origin_xz_m;
            let [length, depth, height] = room.clear_lwh_m;
            if origin_x < 0.0
                || origin_z < 0.0
                || origin_x + length > PIONEER_INTERIOR_SIZE[0]
                || origin_z + depth > PIONEER_INTERIOR_SIZE[1]
                || height > 12.0
            {
                return Err(format!(
                    "Room #{} clear box must fit the 32 × 14 m deck plan and remain at most 12 m high",
                    room.id
                ));
            }
            let [door_x, door_z] = room.door_center_xz_m;
            let [door_width, door_height] = room.door_clear_wh_m;
            let at_deck_boundary = (door_z - origin_z).abs() <= DOOR_EDGE_TOLERANCE_M
                || (door_z - (origin_z + depth)).abs() <= DOOR_EDGE_TOLERANCE_M;
            if door_x - door_width / 2.0 < origin_x
                || door_x + door_width / 2.0 > origin_x + length
                || !at_deck_boundary
                || door_height > height
            {
                return Err(format!(
                    "Room #{} door opening must fit the room width/height and lie on a front or rear room edge",
                    room.id
                ));
            }
            if self.rooms.iter().any(|other| {
                other.id != room.id
                    && other.deck == room.deck
                    && origin_x < other.origin_xz_m[0] + other.clear_lwh_m[0]
                    && other.origin_xz_m[0] < origin_x + length
                    && origin_z < other.origin_xz_m[1] + other.clear_lwh_m[1]
                    && other.origin_xz_m[1] < origin_z + depth
            }) {
                return Err(format!(
                    "Room #{} clear box overlaps another room on deck {}",
                    room.id, room.deck
                ));
            }
        }
        Ok(())
    }

    /// Additional constraints required before this design may drive live
    /// collision. The connected deck-one airlock geometry remains a runtime
    /// contract; its user-editable name is deliberately excluded from this check.
    pub fn validate_live(&self) -> Result<(), String> {
        self.validate()?;

        let airlocks: Vec<_> = self
            .rooms
            .iter()
            .filter(|room| {
                room.source_room_id.as_deref() == Some("D1-P1")
                    || room.runtime_room_id.as_deref() == Some(PIONEER_CONNECTED_ROOM.room_id)
            })
            .collect();
        if airlocks.len() != 1 {
            return Err(
                "Live layout must preserve exactly one source-linked Pioneer airlock".into(),
            );
        }
        let airlock = airlocks[0];
        let door_center_x =
            (PIONEER_CONNECTED_ROOM.door_min[0] + PIONEER_CONNECTED_ROOM.door_max[0]) / 2.0;
        let canonical = [
            (airlock.deck == 1, "connected airlock deck"),
            (
                airlock.runtime_room_id.as_deref() == Some(PIONEER_CONNECTED_ROOM.room_id),
                "connected airlock runtime ID",
            ),
            (
                close(airlock.origin_xz_m[0], 4.2),
                "connected airlock X origin",
            ),
            (
                close(airlock.origin_xz_m[1], 0.2),
                "connected airlock Z origin",
            ),
            (
                close(airlock.clear_lwh_m[0], ROOM_LENGTH_M)
                    && close(airlock.clear_lwh_m[1], ROOM_DEPTH_M)
                    && close(airlock.clear_lwh_m[2], ROOM_CLEAR_HEIGHT_M),
                "connected airlock clear dimensions",
            ),
            (
                close(airlock.door_center_xz_m[0], door_center_x)
                    && close(
                        airlock.door_center_xz_m[1],
                        PIONEER_CONNECTED_ROOM.door_min[2],
                    )
                    && close(airlock.door_clear_wh_m[0], DOOR_WIDTH_M)
                    && close(airlock.door_clear_wh_m[1], DOOR_HEIGHT_M),
                "connected airlock doorway",
            ),
        ];
        if let Some((_, description)) = canonical.iter().find(|(matches, _)| !matches) {
            return Err(format!("Live layout must retain the {description}"));
        }
        if self.rooms.iter().any(|room| {
            room.runtime_room_id
                .as_deref()
                .is_some_and(|id| id != PIONEER_CONNECTED_ROOM.room_id)
        }) {
            return Err(
                "Only the source Pioneer airlock currently has a runtime room binding".into(),
            );
        }

        let mut counts = [[0_u8; 2]; 2];
        for room in &self.rooms {
            let [x, z] = room.origin_xz_m;
            let [length, depth, height] = room.clear_lwh_m;
            let [door_x, door_z] = room.door_center_xz_m;
            let [door_width, door_height] = room.door_clear_wh_m;
            if x < ACCESS_CORE_X_MAX_M + ROOM_SIDE_WALL_M - LIVE_GEOMETRY_EPSILON_M
                || length < LIVE_MIN_ROOM_LENGTH_M
                || depth < LIVE_MIN_ROOM_DEPTH_M
                || !(LIVE_MIN_ROOM_HEIGHT_M..=LIVE_MAX_ROOM_HEIGHT_M).contains(&height)
                || door_width < LIVE_MIN_DOOR_WIDTH_M
                || door_height < LIVE_MIN_DOOR_HEIGHT_M
            {
                return Err(format!(
                    "Room #{} is too small for the live access frame, walls, or a walkable doorway",
                    room.id
                ));
            }

            let end_z = z + depth;
            let south = end_z <= SOUTH_ROOM_MAX_Z_M + LIVE_GEOMETRY_EPSILON_M;
            let north = z >= NORTH_ROOM_MIN_Z_M - LIVE_GEOMETRY_EPSILON_M;
            if !south && !north {
                return Err(format!(
                    "Room #{} enters the reserved 4 m access core or 5.8–8.1 m corridor",
                    room.id
                ));
            }
            let expected_door_plane = if south {
                z + depth + DOOR_POSITIVE_EDGE_OFFSET_M
            } else {
                z - DOOR_NEGATIVE_EDGE_OFFSET_M
            };
            if (door_z - expected_door_plane).abs() > DOOR_EDGE_TOLERANCE_M {
                return Err(format!(
                    "Room #{} doorway must face the access corridor at its normalized wall plane",
                    room.id
                ));
            }
            if door_x - door_width / 2.0 < ACCESS_CORE_X_MAX_M
                || door_x + door_width / 2.0 > PIONEER_INTERIOR_SIZE[0]
            {
                return Err(format!(
                    "Room #{} doorway must open into the reserved access corridor",
                    room.id
                ));
            }
            counts[usize::from(room.deck - 1)][if north { 1 } else { 0 }] += 1;
        }
        for (deck_index, sides) in counts.iter().enumerate() {
            if sides[0] == 0 || sides[1] == 0 {
                return Err(format!(
                    "Live deck {} needs at least one room and clear doorway on each side of the access corridor",
                    deck_index + 1
                ));
            }
        }

        // Leave room for both authored side walls. Draft boxes may be distinct
        // while their projected collision walls overlap.
        for room in &self.rooms {
            for other in &self.rooms {
                if room.id >= other.id || room.deck != other.deck {
                    continue;
                }
                let z_ranges_overlap = room.origin_xz_m[1]
                    < other.origin_xz_m[1] + other.clear_lwh_m[1]
                    && other.origin_xz_m[1] < room.origin_xz_m[1] + room.clear_lwh_m[1];
                if z_ranges_overlap {
                    let gap = if room.origin_xz_m[0] < other.origin_xz_m[0] {
                        other.origin_xz_m[0] - (room.origin_xz_m[0] + room.clear_lwh_m[0])
                    } else {
                        room.origin_xz_m[0] - (other.origin_xz_m[0] + other.clear_lwh_m[0])
                    };
                    if gap < ROOM_SIDE_WALL_M * 2.0 - LIVE_GEOMETRY_EPSILON_M {
                        return Err(format!(
                            "Rooms #{} and #{} leave too little space for their side walls",
                            room.id, other.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Generate the room shell and fixed, already-prototyped storage obstacles
    /// for one deck. Call `validate_live` before installing this geometry.
    pub fn obstacles(&self, deck: u8) -> Vec<StationObstacle> {
        let mut obstacles = vec![
            wall(0.0, 0.0, PIONEER_INTERIOR_SIZE[0], 0.15, 3.2),
            wall(
                0.0,
                PIONEER_INTERIOR_SIZE[1] - 0.15,
                PIONEER_INTERIOR_SIZE[0],
                0.15,
                3.2,
            ),
            wall(0.0, 0.0, 0.15, PIONEER_INTERIOR_SIZE[1], 3.2),
            wall(
                PIONEER_INTERIOR_SIZE[0] - 0.15,
                0.0,
                0.15,
                PIONEER_INTERIOR_SIZE[1],
                3.2,
            ),
        ];
        for room in self.rooms.iter().filter(|room| room.deck == deck) {
            obstacles.extend(room.obstacles());
        }
        // This existing access-core enclosure is part of the authored baseline.
        obstacles.push(StationObstacle {
            kind: StationObstacleKind::Storage,
            min: [0.7, 0.0, 6.45],
            max: [1.0, 1.3, 7.55],
        });
        obstacles
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= LIVE_GEOMETRY_EPSILON_M
}

fn wall(x: f64, z: f64, width: f64, depth: f64, height: f64) -> StationObstacle {
    StationObstacle {
        kind: StationObstacleKind::Wall,
        min: [x, 0.0, z],
        max: [x + width, height, z + depth],
    }
}

fn push_if_area(obstacles: &mut Vec<StationObstacle>, obstacle: StationObstacle) {
    if obstacle.max[0] > obstacle.min[0] && obstacle.max[2] > obstacle.min[2] {
        obstacles.push(obstacle);
    }
}

fn is_prototyped_storage(room: &RoomLayout) -> bool {
    room.deck == 1
        && matches!(
            room.source_room_id.as_deref(),
            Some("D1-S1" | "D1-P2" | "D1-S2" | "D1-P3" | "D1-S3")
        )
}

/// Editable baseline derived from the current twelve Pioneer room sources.
pub fn default_pioneer_room_layout() -> RoomLayoutDocument {
    let source_ids = [
        ["D1-P1", "D1-S1", "D1-P2", "D1-S2", "D1-P3", "D1-S3"],
        ["D2-P1", "D2-S1", "D2-P2", "D2-S2", "D2-P3", "D2-S3"],
    ];
    let mut rooms = Vec::with_capacity(12);
    for deck in 1..=2u8 {
        for (index, (role, x, z)) in pioneer_rooms(deck).iter().enumerate() {
            let door_z = if *z < 7.0 { 5.65 } else { 8.2 };
            rooms.push(RoomLayout {
                id: rooms.len() as u64 + 1,
                source_room_id: Some(source_ids[usize::from(deck - 1)][index].into()),
                runtime_room_id: (deck == 1 && index == 0)
                    .then(|| PIONEER_CONNECTED_ROOM.room_id.into()),
                source_role: Some((*role).into()),
                name: (*role).into(),
                deck,
                origin_xz_m: [*x, *z],
                clear_lwh_m: [ROOM_LENGTH_M, ROOM_DEPTH_M, ROOM_CLEAR_HEIGHT_M],
                door_center_xz_m: [*x + ROOM_LENGTH_M / 2.0, door_z],
                door_clear_wh_m: [DOOR_WIDTH_M, DOOR_HEIGHT_M],
            });
        }
    }
    RoomLayoutDocument {
        schema_version: 1,
        next_id: 13,
        rooms,
    }
}

/// Short name for callers loading the legacy default when no project document exists.
pub fn default_pioneer() -> RoomLayoutDocument {
    default_pioneer_room_layout()
}

/// Compatibility alias retained for the editor's existing baseline call site.
pub fn current_source_document() -> RoomLayoutDocument {
    default_pioneer_room_layout()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_layout_is_live_safe_and_preserves_open_airlock_passage() {
        let document = default_pioneer_room_layout();
        document.validate_live().unwrap();
        let obstacles = document.obstacles(1);
        let airlock = document
            .rooms
            .iter()
            .find(|room| room.source_room_id.as_deref() == Some("D1-P1"))
            .unwrap();
        let passage_x = airlock.door_center_xz_m[0];
        let passage_z = airlock.door_center_xz_m[1] + 0.075;
        assert!(!obstacles.iter().any(|obstacle| {
            obstacle.kind == StationObstacleKind::Wall
                && obstacle.min[0] < passage_x
                && obstacle.max[0] > passage_x
                && obstacle.min[2] < passage_z
                && obstacle.max[2] > passage_z
                && obstacle.max[1] > 1.0
        }));
        let upper = document.obstacles(2);
        let storage: Vec<_> = upper
            .iter()
            .filter(|o| o.kind == StationObstacleKind::Storage)
            .collect();
        assert_eq!(
            storage.len(),
            2,
            "Only the core cabinet and fitted galley counter are installed"
        );
        assert!(storage.iter().any(|o| o.min == [0.7, 0.0, 6.45]));
        assert!(storage.iter().any(|o| (o.min[0] - 5.0).abs() < 1e-9
            && (o.min[2] - 12.95).abs() < 1e-9
            && o.max[1] == 0.9));
        assert_eq!(
            upper
                .iter()
                .filter(|o| o.kind == StationObstacleKind::Table)
                .count(),
            1
        );
        assert_eq!(
            upper
                .iter()
                .filter(|o| o.kind == StationObstacleKind::Sofa)
                .count(),
            2
        );
    }

    #[test]
    fn moved_and_resized_non_airlock_room_changes_only_its_deck_projection() {
        let mut document = default_pioneer_room_layout();
        document.validate_live().unwrap();
        let original_deck_one = document.obstacles(1);
        let original_deck_two = document.obstacles(2);
        let room = document
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-S1"))
            .unwrap();
        room.origin_xz_m[0] += 0.5;
        room.clear_lwh_m[0] -= 0.5;
        room.door_center_xz_m[0] += 0.25;
        document.validate_live().unwrap();
        let changed_deck_one = document.obstacles(1);
        let changed_deck_two = document.obstacles(2);
        assert!(changed_deck_one.iter().any(|after| {
            after.kind == StationObstacleKind::Wall
                && !original_deck_one.iter().any(|before| {
                    before.kind == after.kind && before.min == after.min && before.max == after.max
                })
        }));
        assert_eq!(
            obstacle_values(&original_deck_two),
            obstacle_values(&changed_deck_two)
        );
    }

    #[test]
    fn live_validation_rejects_corridor_intrusion_and_airlock_drift() {
        let mut corridor = default_pioneer_room_layout();
        let room = corridor
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
            .unwrap();
        room.origin_xz_m[1] = 5.9;
        room.clear_lwh_m[1] = 2.4;
        room.door_center_xz_m[1] = 8.25;
        corridor.validate().unwrap();
        assert!(corridor.validate_live().is_err());

        let mut airlock = default_pioneer_room_layout();
        let room = airlock
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-P1"))
            .unwrap();
        room.door_center_xz_m[1] += 0.1;
        airlock.validate().unwrap();
        assert!(airlock.validate_live().is_err());
    }

    #[test]
    fn live_validation_rejects_doors_facing_away_from_corridor() {
        let mut south = default_pioneer_room_layout();
        let room = south
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
            .unwrap();
        room.door_center_xz_m[1] = room.origin_xz_m[1];
        south.validate().unwrap();
        assert!(south.validate_live().is_err());

        let mut north = default_pioneer_room_layout();
        let room = north
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-S1"))
            .unwrap();
        room.door_center_xz_m[1] = room.origin_xz_m[1] + room.clear_lwh_m[1];
        north.validate().unwrap();
        assert!(north.validate_live().is_err());
    }

    #[test]
    fn room_shells_have_closed_rear_walls_without_orphaned_room_fixtures() {
        let mut document = default_pioneer_room_layout();
        let south = document
            .rooms
            .iter()
            .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
            .unwrap();
        let north = document
            .rooms
            .iter()
            .find(|room| room.source_room_id.as_deref() == Some("D1-S1"))
            .unwrap();
        let obstacles = document.obstacles(1);
        for (room, rear_z) in [
            (south, south.origin_xz_m[1] + 0.075),
            (north, north.origin_xz_m[1] + north.clear_lwh_m[1] - 0.075),
        ] {
            let rear_center_x = room.origin_xz_m[0] + room.clear_lwh_m[0] / 2.0;
            assert!(obstacles.iter().any(|obstacle| {
                obstacle.kind == StationObstacleKind::Wall
                    && obstacle.min[0] < rear_center_x
                    && obstacle.max[0] > rear_center_x
                    && obstacle.min[2] < rear_z
                    && obstacle.max[2] > rear_z
            }));
        }

        document
            .rooms
            .retain(|room| room.source_room_id.as_deref() != Some("D1-S1"));
        assert!(!document.obstacles(1).iter().any(|obstacle| {
            obstacle.kind == StationObstacleKind::Storage
                && obstacle.min == [5.2, 0.0, 9.4]
                && obstacle.max == [7.0, 1.4, 10.2]
        }));
    }

    #[test]
    fn authored_doors_within_stage_tolerance_use_normalized_wall_planes() {
        let mut document = default_pioneer_room_layout();
        let room = document
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-S1"))
            .unwrap();
        room.door_center_xz_m[1] += 0.1;
        document.validate_live().unwrap();
        let baseline = default_pioneer_room_layout();
        assert_eq!(
            obstacle_values(&baseline.obstacles(1)),
            obstacle_values(&document.obstacles(1))
        );
    }

    #[test]
    fn room_projection_preserves_ordered_legacy_geometry_for_defaults_and_valid_edits() {
        let baseline = default_pioneer_room_layout();
        let mut edited = baseline.clone();
        let north = edited
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-S1"))
            .unwrap();
        north.origin_xz_m[0] += 0.5;
        north.clear_lwh_m[0] -= 0.5;
        north.door_center_xz_m[0] += 0.25;
        let south = edited
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D2-P2"))
            .unwrap();
        south.origin_xz_m[0] += 0.4;
        south.clear_lwh_m[0] -= 0.4;
        south.clear_lwh_m[2] = 2.9;
        south.door_center_xz_m[0] += 0.2;
        let mut edge = edited.clone();
        let room = edge
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D2-P2"))
            .unwrap();
        room.door_center_xz_m[0] = room.origin_xz_m[0] + room.door_clear_wh_m[0] / 2.0;
        for document in [baseline, edited, edge] {
            document.validate_live().unwrap();
            for deck in [1, 2] {
                assert_eq!(
                    obstacle_values(&document.obstacles(deck)),
                    obstacle_values(&legacy_deck_obstacles(&document, deck))
                );
                let expected = document.obstacles(deck);
                let mut cursor = 4; // The unchanged four outer deck walls.
                for room in document.rooms.iter().filter(|room| room.deck == deck) {
                    let parts = room.obstacles();
                    assert!(!parts.is_empty());
                    assert_eq!(
                        obstacle_values(&expected[cursor..cursor + parts.len()]),
                        obstacle_values(&parts)
                    );
                    cursor += parts.len();
                }
                assert_eq!(cursor + 1, expected.len()); // Unchanged access-core storage.
            }
        }
    }

    // Preserve the pre-extraction deck recipe as an ordered regression oracle.
    fn legacy_deck_obstacles(document: &RoomLayoutDocument, deck: u8) -> Vec<StationObstacle> {
        let mut obstacles = vec![
            wall(0.0, 0.0, PIONEER_INTERIOR_SIZE[0], 0.15, 3.2),
            wall(
                0.0,
                PIONEER_INTERIOR_SIZE[1] - 0.15,
                PIONEER_INTERIOR_SIZE[0],
                0.15,
                3.2,
            ),
            wall(0.0, 0.0, 0.15, PIONEER_INTERIOR_SIZE[1], 3.2),
            wall(
                PIONEER_INTERIOR_SIZE[0] - 0.15,
                0.0,
                0.15,
                PIONEER_INTERIOR_SIZE[1],
                3.2,
            ),
        ];
        for room in document.rooms.iter().filter(|room| room.deck == deck) {
            let [x, z] = room.origin_xz_m;
            let [length, depth, height] = room.clear_lwh_m;
            let [door_x, _door_z] = room.door_center_xz_m;
            let [door_width, _door_height] = room.door_clear_wh_m;
            let normalized_door_plane = room.door_wall_z();
            let door_left = door_x - door_width / 2.0;
            let door_right = door_x + door_width / 2.0;
            push_if_area(
                &mut obstacles,
                wall(x, normalized_door_plane, door_left - x, 0.15, height),
            );
            push_if_area(
                &mut obstacles,
                wall(
                    door_right,
                    normalized_door_plane,
                    x + length - door_right,
                    0.15,
                    height,
                ),
            );
            obstacles.push(wall(x - 0.15, z, 0.15, depth, height));
            obstacles.push(wall(x + length, z, 0.15, depth, height));
            let south = z + depth <= SOUTH_ROOM_MAX_Z_M + LIVE_GEOMETRY_EPSILON_M;
            let rear_wall_z = if south { z } else { z + depth - 0.15 };
            obstacles.push(wall(x, rear_wall_z, length, 0.15, height));

            if is_prototyped_storage(room) {
                obstacles.push(StationObstacle {
                    kind: StationObstacleKind::Storage,
                    min: [x + 1.0, 0.0, z + 1.0],
                    max: [x + 2.8, 1.4, z + 1.8],
                });
            }
            if room.source_room_id.as_deref() == Some("D2-S1") && length >= 7.0 && depth >= 4.5 {
                for (kind, min, max) in [
                    (
                        StationObstacleKind::Storage,
                        [x + 0.8, 0.0, z + depth - 0.85],
                        [x + 3.6, 0.9, z + depth - 0.2],
                    ),
                    (
                        StationObstacleKind::Table,
                        [x + 5.0, 0.0, z + 2.0],
                        [x + 6.4, 0.75, z + 3.0],
                    ),
                    (
                        StationObstacleKind::Sofa,
                        [x + 4.9, 0.0, z + 1.25],
                        [x + 6.5, 0.7, z + 1.7],
                    ),
                    (
                        StationObstacleKind::Sofa,
                        [x + 4.9, 0.0, z + 3.3],
                        [x + 6.5, 0.7, z + 3.75],
                    ),
                ] {
                    obstacles.push(StationObstacle { kind, min, max });
                }
            }
        }
        // This existing access-core enclosure is part of the authored baseline.
        obstacles.push(StationObstacle {
            kind: StationObstacleKind::Storage,
            min: [0.7, 0.0, 6.45],
            max: [1.0, 1.3, 7.55],
        });
        obstacles
    }

    fn obstacle_values(
        obstacles: &[StationObstacle],
    ) -> Vec<(StationObstacleKind, [f64; 3], [f64; 3])> {
        obstacles
            .iter()
            .map(|obstacle| (obstacle.kind, obstacle.min, obstacle.max))
            .collect()
    }
}
