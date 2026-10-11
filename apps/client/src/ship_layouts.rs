//! Dimensioned design reference. These fixtures are proposals, never live telemetry.
use super::*;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Default)]
pub(super) struct LayoutViewer {
    ship: usize,
    deck: u64,
    room: usize,
    fixture: usize,
    medium: String,
}
const NAMES: [&str; 7] = [
    "Pioneer",
    "Prospector",
    "Hauler",
    "Vanguard",
    "Borealis",
    "Stratum",
    "Monolith",
];
fn fleet() -> &'static Vec<Value> {
    static DATA: OnceLock<Vec<Value>> = OnceLock::new();
    DATA.get_or_init(|| {
        [
            include_str!("../../../docs/ships/pioneer/rooms/rooms.json"),
            include_str!("../../../docs/ships/prospector/rooms/rooms.json"),
            include_str!("../../../docs/ships/hauler/rooms/rooms.json"),
            include_str!("../../../docs/ships/vanguard/rooms/rooms.json"),
            include_str!("../../../docs/ships/borealis/rooms/rooms.json"),
            include_str!("../../../docs/ships/stratum/rooms/rooms.json"),
            include_str!("../../../docs/ships/monolith/rooms/rooms.json"),
        ]
        .iter()
        .map(|s| serde_json::from_str(s).expect("bundled ship layout"))
        .collect()
    })
}
fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
fn num(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}
fn color(medium: &str) -> Color32 {
    match medium {
        "power" => Color32::from_rgb(255, 209, 93),
        "emergency_power" => Color32::from_rgb(255, 151, 87),
        "fire_suppression" => Color32::from_rgb(255, 111, 149),
        "air_supply" | "air_return" => Color32::from_rgb(112, 239, 189),
        _ => Color32::from_rgb(98, 215, 255),
    }
}
impl Cockpit {
    pub(super) fn ship_layout_panel(&mut self, ui: &mut egui::Ui) {
        let own_name = self.own().map(|p| p.ship_class.spec().name);
        let state = &mut self.ship_layout;
        ui.heading("Ship layouts");
        ui.colored_label(
            AMBER,
            "DESIGN REFERENCE · proposed equipment, not live sensor data",
        );
        ui.small("Dimensioned human-core rooms. Full hull interiors, equipment footprints and physical routing remain in development.");
        ui.horizontal_wrapped(|ui| {
            let previous = state.ship;
            egui::ComboBox::from_id_salt("layout-hull")
                .selected_text(NAMES[state.ship])
                .show_ui(ui, |ui| {
                    for (i, name) in NAMES.iter().enumerate() {
                        ui.selectable_value(&mut state.ship, i, *name);
                    }
                });
            if ui.button("My ship").clicked() {
                if let Some(i) = NAMES.iter().position(|n| Some(*n) == own_name) {
                    state.ship = i;
                }
            }
            if previous != state.ship {
                state.deck = 1;
                state.room = 0;
                state.fixture = 0;
            }
        });
        let rooms = fleet()[state.ship]["rooms"].as_array().unwrap();
        let decks = rooms
            .iter()
            .map(|r| r["deck"].as_u64().unwrap())
            .max()
            .unwrap();
        state.deck = state.deck.clamp(1, decks);
        ui.horizontal_wrapped(|ui| {
            ui.label("Deck");
            for d in 1..=decks {
                if ui
                    .selectable_label(state.deck == d, d.to_string())
                    .clicked()
                {
                    state.deck = d;
                    state.room = 0;
                    state.fixture = 0;
                }
            }
        });
        let deck: Vec<_> = rooms
            .iter()
            .filter(|r| r["deck"].as_u64() == Some(state.deck))
            .collect();
        state.room = state.room.min(deck.len() - 1);
        ui.horizontal_wrapped(|ui| {
            for (i, room) in deck.iter().enumerate() {
                if ui
                    .selectable_label(state.room == i, text(&room["room_id"]))
                    .on_hover_text(text(&room["role"]))
                    .clicked()
                {
                    state.room = i;
                    state.fixture = 0;
                }
            }
        });
        let room = deck[state.room];
        ui.strong(format!(
            "{} · {}",
            text(&room["room_id"]),
            text(&room["role"])
        ));
        let dims = &room["room_clear_lwh_m"];
        ui.small(format!(
            "{:.2} × {:.2} m · {:.2} m clear height · top view: forward → / starboard ↓",
            num(&dims[0]),
            num(&dims[1]),
            num(&dims[2])
        ));
        egui::ComboBox::from_id_salt("layout-system")
            .selected_text(if state.medium.is_empty() {
                "All systems"
            } else {
                &state.medium
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.medium, String::new(), "All systems");
                for medium in [
                    "power",
                    "emergency_power",
                    "data",
                    "fire_suppression",
                    "air_supply",
                    "air_return",
                    "potable",
                    "graywater",
                    "sealed_waste",
                ] {
                    ui.selectable_value(
                        &mut state.medium,
                        medium.to_owned(),
                        medium.replace('_', " "),
                    );
                }
            });
        let fixtures = room["fixtures"].as_array().unwrap();
        let visible: Vec<usize> = fixtures
            .iter()
            .enumerate()
            .filter(|(_, f)| state.medium.is_empty() || state.medium == text(&f["medium"]))
            .map(|(i, _)| i)
            .collect();
        if !visible.contains(&state.fixture) {
            state.fixture = visible.first().copied().unwrap_or(0);
        }
        let width = ui.available_width().max(100.0);
        let (canvas, _) = ui.allocate_exact_size(Vec2::new(width, 280.0), egui::Sense::hover());
        let scale = ((width - 40.0) / num(&dims[0])).min(235.0 / num(&dims[1]));
        let origin = canvas.min + Vec2::new(20.0, 15.0);
        let bounds = egui::Rect::from_min_size(
            origin,
            Vec2::new(num(&dims[0]) * scale, num(&dims[1]) * scale),
        );
        ui.painter()
            .rect_filled(bounds, 0.0, Color32::from_rgb(5, 24, 18));
        ui.painter().rect_stroke(
            bounds,
            0.0,
            egui::Stroke::new(1.0, Color32::from_rgb(0, 220, 130)),
            egui::StrokeKind::Inside,
        );
        let door = &room["door_center_local_xz_m"];
        let dp = origin + Vec2::new(num(&door[0]) * scale, num(&door[1]) * scale);
        ui.painter().line_segment(
            [
                dp - Vec2::new(0.7 * scale, 0.0),
                dp + Vec2::new(0.7 * scale, 0.0),
            ],
            egui::Stroke::new(4.0, Color32::WHITE),
        );
        for (i, f) in fixtures.iter().enumerate() {
            let medium = text(&f["medium"]);
            if !state.medium.is_empty() && state.medium != medium {
                continue;
            }
            let xyz = &f["local_xyz_m"];
            let point = origin + Vec2::new(num(&xyz[0]) * scale, num(&xyz[2]) * scale);
            ui.painter().circle_filled(
                point,
                if state.fixture == i { 6.0 } else { 4.0 },
                color(medium),
            );
            ui.interact(
                egui::Rect::from_center_size(point, Vec2::splat(13.0)),
                ui.id().with(("fixture", i)),
                egui::Sense::hover(),
            )
            .on_hover_text(format!(
                "{} · {} · height {:.2} m",
                text(&f["id"]),
                text(&f["label"]),
                num(&xyz[1])
            ));
        }
        ui.small("Select a fitting below; stacked wall fittings share a top-view position. White line: door opening.");
        egui::ScrollArea::vertical()
            .id_salt("fixture-list")
            .max_height(160.0)
            .show(ui, |ui| {
                for (i, f) in fixtures.iter().enumerate() {
                    if !state.medium.is_empty() && state.medium != text(&f["medium"]) {
                        continue;
                    }
                    if ui
                        .selectable_label(
                            state.fixture == i,
                            egui::RichText::new(format!(
                                "{} · {}",
                                text(&f["id"]),
                                text(&f["label"])
                            ))
                            .color(color(text(&f["medium"]))),
                        )
                        .clicked()
                    {
                        state.fixture = i;
                    }
                }
            });
        if visible.is_empty() {
            ui.label("No proposed fittings for this system in this room.");
            return;
        }
        let f = &fixtures[state.fixture];
        ui.separator();
        ui.strong(text(&f["id"]));
        for (label, key) in [
            ("Room XYZ (m)", "local_xyz_m"),
            ("Ship XYZ (m)", "ship_xyz_m"),
        ] {
            let p = &f[key];
            ui.label(format!(
                "{label}: {:.3} / {:.3} / {:.3}",
                num(&p[0]),
                num(&p[1]),
                num(&p[2])
            ));
        }
        ui.label(format!("Connected system: {}", text(&f["upstream"])));
        ui.label(format!("Isolation: {}", text(&f["isolator"])));
        ui.small("Proposed placement only. Suppression agent/coverage, electrical ratings and equipment clearance are unresolved.");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_layouts_match_fleet_and_fit_rooms() {
        let mut count = 0;
        for (i, ship) in fleet().iter().enumerate() {
            assert_eq!(text(&ship["ship"]), NAMES[i]);
            for room in ship["rooms"].as_array().unwrap() {
                count += 1;
                let d = &room["room_clear_lwh_m"];
                for f in room["fixtures"].as_array().unwrap() {
                    for (axis, bound) in [(0, 0), (1, 2), (2, 1)] {
                        assert!(
                            num(&f["local_xyz_m"][axis]) >= 0.0
                                && num(&f["local_xyz_m"][axis]) <= num(&d[bound])
                        );
                    }
                }
            }
        }
        assert_eq!(count, 240);
    }
    #[test]
    fn filtered_details_follow_visible_fittings_and_empty_rooms_are_honest() {
        let ctx = egui::Context::default();
        let mut cockpit = super::super::tests::view_test_cockpit(&ctx);
        let rooms = fleet()[0]["rooms"].as_array().unwrap();
        let wet = rooms
            .iter()
            .find(|r| {
                r["fixtures"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| text(&f["medium"]) == "potable")
            })
            .unwrap();
        cockpit.ship_layout.deck = wet["deck"].as_u64().unwrap();
        cockpit.ship_layout.room = rooms
            .iter()
            .filter(|r| r["deck"] == wet["deck"])
            .position(|r| r["room_id"] == wet["room_id"])
            .unwrap();
        for medium in ["potable", "graywater", "sealed_waste", "data", "power"] {
            cockpit.ship_layout.medium = medium.into();
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| cockpit.ship_layout_panel(ui));
            });
            let selected = &wet["fixtures"][cockpit.ship_layout.fixture];
            assert_eq!(text(&selected["medium"]), medium);
        }
        cockpit.ship_layout.deck = 1;
        cockpit.ship_layout.room = 0;
        cockpit.ship_layout.medium = "potable".into();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| cockpit.ship_layout_panel(ui));
        });
        let text: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => Some(t.galley.text()),
                _ => None,
            })
            .collect();
        assert!(text.iter().any(|t| t.contains("No proposed fittings")));
        assert!(!text.iter().any(|t| t.starts_with("Connected system:")));
    }
}
