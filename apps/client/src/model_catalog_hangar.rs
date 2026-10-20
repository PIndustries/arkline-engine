//! Native inspection adapters for the actual meter-space hangar recipes.
use super::super::model_catalog::{InspectionDraw, RuntimeModelDescriptor};
use super::*;
use arkline_protocol::{EquipmentKind, FleetLayoutRevision, StationObstacleKind, ALL_SHIP_CLASSES};

fn descriptor(
    id: String,
    name: String,
    category: &str,
    source: &str,
    notes: &[&str],
    dimensions: Option<[f64; 3]>,
) -> RuntimeModelDescriptor {
    RuntimeModelDescriptor {
        id,
        name,
        category: category.into(),
        source: source.into(),
        notes: notes.iter().map(|note| (*note).into()).collect(),
        unavailable: None,
        spec_dimensions_m: dimensions,
    }
}

fn class_id(class: ShipClass) -> String {
    class.spec().name.to_ascii_lowercase()
}

pub(in crate::flight3d) fn descriptors() -> Vec<RuntimeModelDescriptor> {
    let mut entries = Vec::new();
    for class in ALL_SHIP_CLASSES {
        let layout = hangar_layout(class);
        entries.push(descriptor(
            format!("bay/{}", class_id(class)),
            format!("{} private bay", class.spec().name),
            "Hangars",
            "apps/client/src/hangar.rs::cached_bay_parts",
            &["Shared runtime cutaway bay and hull assembly.", "Dimensions describe the protocol bay layout; measured geometry can include markings and fixtures."],
            Some([layout.half_length * 2.0, layout.height, layout.half_width * 2.0]),
        ));
        let room = arkline_protocol::quarters_layout(class);
        entries.push(descriptor(
            format!("quarters/{}", class_id(class)),
            format!("{} station quarters", class.spec().name),
            "Interiors",
            "apps/client/src/quarters.rs::room_parts",
            &[
                "Shared furnished room design at this class's protocol placement.",
                "Table hologram is frozen at presentation time zero.",
            ],
            Some(std::array::from_fn(|i| room.max[i] - room.min[i])),
        ));
    }
    for (id, label) in [
        ("linear_v1", "Linear station fleet"),
        ("circular_v1", "Circular station fleet"),
    ] {
        entries.push(descriptor(
            format!("fleet/{id}"),
            label.into(),
            "Hangars",
            "apps/client/src/fleet_hangar.rs::parts_except_hulls",
            &[
                "Shared layout and all seven catalog berth hulls, with neutral turret poses.",
                "Catalog composition, not a statement of the current user's vessel ownership.",
            ],
            None,
        ));
    }
    for deck in [1, 2] {
        entries.push(descriptor(
            format!("interior/pioneer/deck_{deck}"), format!("Pioneer interior · deck {deck}"),
            "Interiors", "apps/client/src/pioneer_interior.rs::cached_parts",
            &["Default shared runtime room layout; six room sources on this deck.", "Includes installed fixtures in a frozen commissioned visual state; no commands are issued."],
            Some([32.0, 3.2, 14.0]),
        ));
    }
    for equipment in arkline_protocol::pioneer_room_catalog().equipment {
        let mut entry = descriptor(
            format!("equipment/{}", equipment.id), equipment.label.into(), "Equipment",
            "crates/arkline-protocol/src/room_equipment.rs::pioneer_room_catalog",
            &["Stable equipment identity from the connected Pioneer airlock catalog.", "Physical entries reuse installed bodies and output geometry at a frozen visual state."],
            None,
        );
        if equipment.position.is_none() {
            entry.unavailable = Some(
                "Logical catalog endpoint; no separately installed physical model exists.".into(),
            );
        }
        if equipment.kind == EquipmentKind::Door {
            entry
                .notes
                .push("Shown closed using the runtime door geometry.".into());
        }
        entries.push(entry);
    }
    for (id, label) in [
        ("standing", "Standing"),
        ("walking", "Walking pose"),
        ("seated", "Seated"),
    ] {
        entries.push(descriptor(
            format!("avatar/{id}"), format!("Station avatar · {label}"), "Characters",
            "apps/client/src/hangar.rs::avatar_parts",
            &["Shared runtime suit geometry and pose recipe.", "Nominal standing avatar height is 1.8 m in the protocol; displayed bounds are measured from this pose."],
            None,
        ));
    }
    for (id, label, _) in furnishings() {
        let mut entry = descriptor(
            format!("furniture/{id}"),
            format!("Station {label}"),
            "Furniture",
            "apps/client/src/quarters.rs::obstacle_parts",
            &["Exact shared furnishing recipe at its Pioneer quarters placement."],
            None,
        );
        if id == "table" {
            entry
                .notes
                .push("Includes the existing frozen Pioneer hologram.".into());
        }
        entries.push(entry);
    }
    entries
}

fn furnishings() -> [(&'static str, &'static str, StationObstacleKind); 5] {
    [
        ("bed", "bed", StationObstacleKind::Bed),
        ("desk", "desk", StationObstacleKind::Desk),
        ("sofa", "sofa", StationObstacleKind::Sofa),
        ("table", "table", StationObstacleKind::Table),
        ("storage", "storage cabinet", StationObstacleKind::Storage),
    ]
}

fn fixture_state() -> arkline_protocol::PioneerRoomState {
    let mut state = arkline_protocol::PioneerRoomState::uncommissioned("inspection-only".into());
    state.commissioned = true;
    state.breaker_closed = true;
    state.light_on = true;
    state.door_open = false;
    state
}

fn fleet(revision: FleetLayoutRevision) -> arkline_protocol::HangarView {
    arkline_protocol::HangarView {
        layout_revision: revision,
        layout: arkline_protocol::fleet_hangar_layout_for(revision),
        berths: ALL_SHIP_CLASSES
            .into_iter()
            .map(|class| {
                let pose = arkline_protocol::fleet_berth_pose(revision, class);
                arkline_protocol::HangarBerth {
                    id: format!("inspection/{}", class_id(class)),
                    ship_class: class,
                    position: pose.position,
                    yaw: pose.yaw,
                    boarding_point: pose.boarding_point,
                    active: false,
                    can_board: false,
                }
            })
            .collect(),
    }
}

fn avatar(pose: &str) -> Option<Vec<ModelPart>> {
    let mut avatar = StationAvatar {
        onboard_interior: false,
        interior_deck: 1,
        jump_started_ms: None,
        seated: false,
        position: [0.0; 3],
        heading: 0.0,
        moving: false,
        updated_at_ms: 0,
    };
    let mut gait = avatar_animation::Pose::default();
    match pose {
        "standing" => {}
        "walking" => {
            avatar.moving = true;
            gait.phase = 0.2;
            gait.weight = 1.0;
        }
        "seated" => avatar.seated = true,
        _ => return None,
    }
    Some(avatar_parts(&avatar, gait, None))
}

pub(in crate::flight3d) fn room_parts(source_id: &str) -> Result<Vec<ModelPart>, String> {
    pioneer_interior::inspection_room_parts(source_id)
}

pub(in crate::flight3d) fn load(id: &str) -> Option<Result<(Vec<InspectionDraw>, f32), String>> {
    let (category, identity) = id.split_once('/')?;
    let parts = match category {
        "bay" | "quarters" => {
            let class = ALL_SHIP_CLASSES
                .into_iter()
                .find(|class| class_id(*class) == identity)?;
            if category == "bay" {
                cached_bay_parts(class).to_vec()
            } else {
                quarters::room_parts(class, 0)
            }
        }
        "fleet" => {
            let revision = match identity {
                "linear_v1" => FleetLayoutRevision::LinearV1,
                "circular_v1" => FleetLayoutRevision::CircularV1,
                _ => return None,
            };
            fleet_hangar::parts_except_hulls(&fleet(revision), Vec3::ZERO, &[])
        }
        "interior" => {
            let deck = match identity {
                "pioneer/deck_1" => 1,
                "pioneer/deck_2" => 2,
                _ => return None,
            };
            let mut parts = pioneer_interior::cached_parts(deck).as_ref().clone();
            parts.extend(pioneer_interior::state_parts(deck, Some(&fixture_state())));
            parts
        }
        "equipment" => {
            let entry = arkline_protocol::pioneer_room_catalog()
                .equipment
                .iter()
                .find(|entry| entry.id == identity)?;
            if entry.position.is_none() {
                return Some(Err(
                    "Logical catalog endpoint; no separately installed physical model exists."
                        .into(),
                ));
            }
            let Some(parts) = pioneer_interior::equipment_parts(identity, &fixture_state()) else {
                return Some(Err(
                    "Physical equipment has no registered shared geometry recipe.".into(),
                ));
            };
            parts
        }
        "avatar" => avatar(identity)?,
        "furniture" => {
            let (_, _, kind) = furnishings()
                .into_iter()
                .find(|(key, _, _)| *key == identity)?;
            let obstacle = arkline_protocol::station_obstacles(ShipClass::Pioneer)
                .into_iter()
                .find(|obstacle| obstacle.kind == kind)?;
            quarters::obstacle_parts(ShipClass::Pioneer, 0, obstacle)
        }
        _ => return None,
    };
    Some(Ok((
        parts
            .into_iter()
            .map(|part| InspectionDraw {
                part,
                material: Material::PaintedHull,
                range: None,
            })
            .collect(),
        1.0,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_hangar_catalog_record_is_unique_and_honest_about_geometry() {
        let entries = descriptors();
        assert_eq!(entries.len(), 34);
        let mut ids = std::collections::HashSet::new();
        let mut physical = 0;
        for entry in entries {
            assert!(ids.insert(entry.id.clone()));
            let loaded = load(&entry.id).expect("registered catalog ID");
            if entry.unavailable.is_some() {
                assert!(
                    loaded.is_err(),
                    "logical entry must not acquire proxy geometry"
                );
                continue;
            }
            let (draws, meters) = loaded.expect("registered shared recipe");
            assert_eq!(meters, 1.0);
            assert!(!draws.is_empty());
            for draw in draws {
                let (_, matrix, color, emission) = draw.part;
                assert!(matrix.is_finite() && matrix.determinant().abs() > 1e-20);
                assert!(color.is_finite() && emission.is_finite());
            }
            physical += 1;
        }
        assert_eq!(physical, 31);
        assert!(load("bay/unsupported").is_none());
        assert!(load("unregistered").is_none());
    }

    #[test]
    fn bay_quarters_and_furniture_preserve_runtime_recipe_tuples() {
        for class in ALL_SHIP_CLASSES {
            for (id, expected) in [
                (
                    format!("bay/{}", class_id(class)),
                    cached_bay_parts(class).to_vec(),
                ),
                (
                    format!("quarters/{}", class_id(class)),
                    quarters::room_parts(class, 0),
                ),
            ] {
                let (actual, _) = load(&id).unwrap().unwrap();
                assert_eq!(
                    actual.into_iter().map(|draw| draw.part).collect::<Vec<_>>(),
                    expected
                );
            }
        }
        let room = quarters::room_parts(ShipClass::Pioneer, 0);
        for (_, _, kind) in furnishings() {
            let obstacle = arkline_protocol::station_obstacles(ShipClass::Pioneer)
                .into_iter()
                .find(|o| o.kind == kind)
                .unwrap();
            let recipe = quarters::obstacle_parts(ShipClass::Pioneer, 0, obstacle);
            assert!(room
                .windows(recipe.len())
                .any(|parts| parts == recipe.as_slice()));
        }
    }

    #[test]
    fn physical_equipment_parts_match_installed_static_and_dynamic_geometry() {
        let state = fixture_state();
        let mut installed = pioneer_interior::cached_parts(1).as_ref().clone();
        installed.extend(pioneer_interior::state_parts(1, Some(&state)));
        for equipment in arkline_protocol::pioneer_room_catalog().equipment {
            let parts = pioneer_interior::equipment_parts(equipment.id, &state);
            if equipment.position.is_none() {
                assert!(parts.is_none());
                continue;
            }
            let parts = parts.expect("physical fixture recipe");
            assert!(!parts.is_empty());
            for part in parts {
                assert!(
                    installed.contains(&part),
                    "{} is not an installed runtime part",
                    equipment.id
                );
            }
        }
    }
}
