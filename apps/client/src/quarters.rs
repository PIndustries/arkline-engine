//! Human-scale station quarters; collision shapes come from the server contract.
use super::*;
use arkline_protocol::{quarters_layout, station_obstacles, StationObstacleKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HangarAction {
    Overview,
    Ship,
    Inventory,
    Corporation,
    Ai,
    Sit,
}

fn box_part(parts: &mut Parts, min: Vec3, max: Vec3, color: Vec3, emission: f32) {
    parts.draw(
        0,
        transform((min + max) * 0.5, (max - min) * 0.5, Quat::IDENTITY),
        color,
        emission,
    );
}
pub(super) fn room_parts(class: ShipClass, now: u64) -> Vec<ModelPart> {
    let room = quarters_layout(class);
    let min = Vec3::from_array(room.min.map(|v| v as f32));
    let max = Vec3::from_array(room.max.map(|v| v as f32));
    let mut p = Parts(Vec::new());
    let charcoal = Vec3::new(0.035, 0.055, 0.06);
    let green = Vec3::new(0.015, 0.35, 0.12);
    box_part(
        &mut p,
        min - Vec3::Y * 0.06,
        Vec3::new(max.x, 0.005, max.z),
        charcoal,
        0.0,
    );
    box_part(
        &mut p,
        Vec3::new(min.x, 3.2, min.z),
        Vec3::new(max.x, 3.3, max.z),
        charcoal,
        0.0,
    );
    for x in [-6.0, 0.0, 6.0] {
        box_part(
            &mut p,
            Vec3::new(x - 0.04, 3.16, min.z + 0.5),
            Vec3::new(x + 0.04, 3.19, max.z - 0.5),
            Vec3::new(0.18, 0.32, 0.25),
            0.9,
        );
    }
    for obstacle in station_obstacles(class) {
        let a = Vec3::from_array(obstacle.min.map(|v| v as f32));
        let b = Vec3::from_array(obstacle.max.map(|v| v as f32));
        let c = (a + b) * 0.5;
        match obstacle.kind {
            StationObstacleKind::Wall => {
                box_part(&mut p, a, b, charcoal, 0.0);
                box_part(
                    &mut p,
                    Vec3::new(a.x, 2.9, a.z),
                    Vec3::new(b.x, 2.95, b.z),
                    green,
                    0.5,
                );
            }
            StationObstacleKind::Sofa | StationObstacleKind::Bed => {
                box_part(&mut p, a, b, Vec3::new(0.04, 0.085, 0.073), 0.0);
                box_part(
                    &mut p,
                    Vec3::new(a.x + 0.12, b.y, a.z + 0.12),
                    Vec3::new(b.x - 0.12, b.y + 0.09, b.z - 0.12),
                    Vec3::new(0.075, 0.115, 0.105),
                    0.0,
                );
                // Padded head/backrest keeps the silhouette furnished, not a crate.
                box_part(
                    &mut p,
                    Vec3::new(a.x + 0.1, b.y, b.z - 0.3),
                    Vec3::new(b.x - 0.1, b.y + 0.35, b.z - 0.06),
                    charcoal,
                    0.0,
                );
            }
            StationObstacleKind::Desk | StationObstacleKind::Table => {
                box_part(
                    &mut p,
                    Vec3::new(a.x, b.y - 0.10, a.z),
                    b,
                    Vec3::new(0.10, 0.14, 0.14),
                    0.0,
                );
                for x in [a.x + 0.12, b.x - 0.12] {
                    for z in [a.z + 0.12, b.z - 0.12] {
                        box_part(
                            &mut p,
                            Vec3::new(x - 0.07, a.y, z - 0.07),
                            Vec3::new(x + 0.07, b.y, z + 0.07),
                            charcoal,
                            0.0,
                        );
                    }
                }
                if obstacle.kind == StationObstacleKind::Table {
                    let hologram = Mat4::from_scale_rotation_translation(
                        Vec3::splat(0.85 / class.spec().length_m as f32 * 100.0),
                        Quat::from_rotation_y(now as f32 * 0.00015),
                        Vec3::new(c.x, b.y + 0.65, c.z),
                    );
                    p.0.extend(
                        ship_parts(class)
                            .into_iter()
                            .map(|(mesh, m, _, _)| (mesh, hologram * m, green, 0.75)),
                    );
                    p.draw(
                        1,
                        transform(
                            Vec3::new(c.x, b.y + 0.02, c.z),
                            Vec3::new(0.65, 0.1, 0.65),
                            Quat::IDENTITY,
                        ),
                        green,
                        0.5,
                    );
                }
            }
            StationObstacleKind::Storage => {
                box_part(&mut p, a, b, charcoal, 0.0);
                for y in [0.5, 1.1, 1.7] {
                    box_part(
                        &mut p,
                        Vec3::new(a.x + 0.12, y, a.z - 0.012),
                        Vec3::new(b.x - 0.12, y + 0.025, a.z),
                        green,
                        0.4,
                    );
                }
            }
        }
    }
    for x in [-8.0, -4.0, 0.0, 4.0, 8.0] {
        box_part(
            &mut p,
            Vec3::new(x - 0.012, 0.1, max.z - 0.315),
            Vec3::new(x + 0.012, 2.85, max.z - 0.305),
            Vec3::new(0.1, 0.16, 0.15),
            0.1,
        );
    }
    for x in [-5.5, 5.5] {
        box_part(
            &mut p,
            Vec3::new(x - 1.0, 3.13, min.z + 4.0),
            Vec3::new(x + 1.0, 3.16, min.z + 6.0),
            Vec3::new(0.3, 0.4, 0.34),
            1.0,
        );
    }
    // Rear-wall service displays remain physical, reachable objects.
    for (i, _) in actions().iter().enumerate() {
        let c = terminal_position(class, i);
        box_part(
            &mut p,
            c - Vec3::new(0.60, 0.48, 0.015),
            c + Vec3::new(0.60, 0.48, 0.015),
            Vec3::new(0.012, 0.07, 0.045),
            0.5,
        );
        box_part(
            &mut p,
            c - Vec3::new(0.64, 0.53, -0.02),
            c + Vec3::new(0.64, 0.53, 0.06),
            charcoal,
            0.0,
        );
    }
    p.0
}
fn actions() -> [(HangarAction, &'static str); 5] {
    [
        (HangarAction::Overview, "NAVIGATION"),
        (HangarAction::Ship, "VESSEL"),
        (HangarAction::Inventory, "INVENTORY"),
        (HangarAction::Corporation, "CORPORATION"),
        (HangarAction::Ai, "SHIP COMPUTER"),
    ]
}
fn terminal_position(class: ShipClass, index: usize) -> Vec3 {
    let r = quarters_layout(class);
    Vec3::new(-2.6 + index as f32 * 1.3, 1.65, r.max[2] as f32 - 0.35)
}
fn inside_quad(point: egui::Pos2, quad: [egui::Pos2; 4]) -> bool {
    let signs = std::array::from_fn::<_, 4, _>(|i| {
        let a = quad[(i + 1) % 4] - quad[i];
        let b = point - quad[i];
        a.x * b.y - a.y * b.x
    });
    signs.iter().all(|v| *v >= -0.01) || signs.iter().all(|v| *v <= 0.01)
}
// Parameter of first intersection along segment, with a small camera envelope.
pub(super) fn segment_box(a: Vec3, b: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let d = b - a;
    let mut near = 0.0f32;
    let mut far = 1.0f32;
    for axis in 0..3 {
        if d[axis].abs() < 1e-6 {
            if a[axis] < min[axis] || a[axis] > max[axis] {
                return None;
            }
        } else {
            let t0 = (min[axis] - a[axis]) / d[axis];
            let t1 = (max[axis] - a[axis]) / d[axis];
            near = near.max(t0.min(t1));
            far = far.min(t0.max(t1));
            if near > far {
                return None;
            }
        }
    }
    Some(near)
}
pub(super) fn camera_eye(class: ShipClass, target: Vec3, desired: Vec3) -> Vec3 {
    let mut limit = 1.0f32;
    for o in station_obstacles(class) {
        let min = Vec3::from_array(o.min.map(|v| v as f32)) - Vec3::splat(0.15);
        let max = Vec3::from_array(o.max.map(|v| v as f32)) + Vec3::splat(0.15);
        if let Some(t) = segment_box(target, desired, min, max) {
            limit = limit.min((t - 0.035).max(0.0));
        }
    }
    let room = quarters_layout(class);
    if let Some(t) = segment_box(
        target,
        desired,
        Vec3::new(room.min[0] as f32, 3.05, room.min[2] as f32),
        Vec3::new(room.max[0] as f32, 3.45, room.max[2] as f32),
    ) {
        limit = limit.min((t - 0.035).max(0.0));
    }
    let spec = class.spec();
    if let Some(t) = segment_box(
        target,
        desired,
        Vec3::new(-spec.length_m as f32 / 2.0, 0.0, -spec.width_m as f32 / 2.0),
        Vec3::new(
            spec.length_m as f32 / 2.0,
            spec.height_m as f32,
            spec.width_m as f32 / 2.0,
        ),
    ) {
        limit = limit.min((t - 0.035).max(0.0));
    }
    target.lerp(desired, limit.max(0.025))
}
pub(super) fn terminals(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vp: Mat4,
    eye: Vec3,
    pilot: &PilotView,
    unobstructed: bool,
    live: bool,
) -> Option<HangarAction> {
    let avatar = pilot.station_avatar.as_ref()?;
    let feet = Vec3::from_array(avatar.position.map(|v| v as f32));
    let mut result = None;
    let seat = Vec3::from_array(
        quarters_layout(pilot.ship_class)
            .seat_point
            .map(|v| v as f32),
    );
    if !avatar.seated && feet.distance(seat) < 2.0 {
        if let Some(at) = project_point(vp, rect, seat + Vec3::Y * 0.9) {
            let button = egui::Rect::from_center_size(at, egui::vec2(100.0, 28.0));
            let response = ui.put(button, egui::Button::new("Sit on sofa"));
            if live && unobstructed && response.clicked() {
                result = Some(HangarAction::Sit);
            }
        }
    }

    for (i, (action, label)) in actions().into_iter().enumerate() {
        let world = terminal_position(pilot.ship_class, i);
        let near = feet.distance(world) < 3.0;
        if eye.z > world.z {
            continue;
        }
        let blocked = station_obstacles(pilot.ship_class).into_iter().any(|o| {
            segment_box(
                eye,
                world,
                Vec3::from_array(o.min.map(|v| v as f32)),
                Vec3::from_array(o.max.map(|v| v as f32)),
            )
            .is_some_and(|t| t < 0.99)
        });
        if blocked {
            continue;
        }
        let corners = [
            Vec3::new(-0.55, 0.4, 0.0),
            Vec3::new(0.55, 0.4, 0.0),
            Vec3::new(0.55, -0.4, 0.0),
            Vec3::new(-0.55, -0.4, 0.0),
        ]
        .map(|offset| project_point(vp, rect, world + offset));
        let [Some(a), Some(b), Some(c), Some(d)] = corners else {
            continue;
        };
        let quad = [a, b, c, d];
        let mut screen = egui::Rect::from_points(&quad).intersect(rect);
        for _ in 0..25 {
            if [
                screen.left_top(),
                screen.right_top(),
                screen.right_bottom(),
                screen.left_bottom(),
            ]
            .into_iter()
            .all(|p| inside_quad(p, quad))
            {
                break;
            }
            screen = egui::Rect::from_center_size(screen.center(), screen.size() * 0.9);
        }
        if screen.width() < 65.0 || screen.height() < 48.0 {
            continue;
        }
        let painter = ui.painter().with_clip_rect(screen);
        let detail = match action {
            HangarAction::Ship => format!(
                "{}\n{}",
                pilot.ship_class.spec().name,
                pilot
                    .power
                    .as_ref()
                    .map(|p| format!("CAP {} / {} kJ", p.charge_j / 1000, p.capacity_j / 1000))
                    .unwrap_or_else(|| "Power unavailable".into())
            ),
            HangarAction::Inventory => format!(
                "Ore {} / {}\n{} credits",
                pilot.ore,
                pilot.ship_class.spec().cargo_capacity,
                pilot.credits
            ),
            HangarAction::Overview => format!("System {}", pilot.system_id),
            HangarAction::Ai => "Ask your ship computer".into(),
            HangarAction::Corporation => "Open corporation services".into(),
            HangarAction::Sit => unreachable!(),
        };
        painter.rect_filled(screen, 2.0, egui::Color32::from_rgb(5, 23, 18));
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            format!(
                "{label}\n{detail}\n{}",
                if !near {
                    "APPROACH TO USE"
                } else if live {
                    "CLICK TO OPEN"
                } else {
                    "LAST KNOWN DATA"
                }
            ),
            egui::FontId::proportional(12.0),
            egui::Color32::from_rgb(90, 220, 150),
        );
        let response = ui.interact(
            screen,
            ui.id().with(("quarters_terminal", i)),
            egui::Sense::click(),
        );
        if near && unobstructed && response.clicked() {
            result = Some(action);
        }
    }
    let room = quarters_layout(pilot.ship_class);
    let (text, z) = if eye.z < room.min[2] as f32 {
        ("CAPTAIN'S QUARTERS", room.min[2] as f32 - 0.05)
    } else {
        ("BOARDING ACCESS", room.min[2] as f32 + 0.35)
    };
    if let Some(at) = project_point(vp, rect, Vec3::new(0.0, 2.65, z)) {
        ui.painter().text(
            at,
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(14.0),
            egui::Color32::from_rgb(80, 200, 130),
        );
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projected_terminal_click_requires_nearby_avatar_and_clear_input_layer() {
        for (near, clear, expected) in [
            (true, true, true),
            (false, true, false),
            (true, false, false),
        ] {
            let ctx = egui::Context::default();
            let world = terminal_position(ShipClass::Pioneer, 2);
            let feet = world - Vec3::new(0.0, 1.65, if near { 1.5 } else { 9.0 });
            let pilot:PilotView=serde_json::from_value(serde_json::json!({"name":"RoomQA","credits":1,"ore":0,"location":Location::Docked,"station_avatar":{"position":feet.to_array(),"heading":0.0,"moving":false,"updated_at_ms":0}})).unwrap();
            let eye = world - Vec3::Z * 2.0;
            let vp = Mat4::perspective_rh_gl(1.0, 900.0 / 650.0, 0.08, 100.0)
                * Mat4::look_at_rh(eye, world, Vec3::Y);
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 650.0));
            let at = project_point(vp, rect, world).unwrap();
            let mut clicked = false;
            for events in [
                vec![],
                vec![egui::Event::PointerMoved(at)],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                }],
                vec![egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Default::default(),
                }],
            ] {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(rect),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            clicked |= terminals(ui, rect, vp, eye, &pilot, clear, true)
                                == Some(HangarAction::Inventory);
                        });
                    },
                );
            }
            assert_eq!(clicked, expected);
        }
    }
    #[test]
    fn segment_collision_stops_at_wall_and_passes_door_gap() {
        assert_eq!(
            segment_box(
                Vec3::ZERO,
                Vec3::X * 10.0,
                Vec3::new(5.0, -1.0, -1.0),
                Vec3::new(6.0, 1.0, 1.0)
            ),
            Some(0.5)
        );
        assert!(segment_box(
            Vec3::ZERO,
            Vec3::Z * 10.0,
            Vec3::new(2.0, -1.0, 5.0),
            Vec3::new(6.0, 1.0, 6.0)
        )
        .is_none());
    }
    #[test]
    fn room_geometry_finite_for_all_hulls() {
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            assert!(room_parts(class, 1000).iter().all(|p| p.1.is_finite()));
        }
    }
}
