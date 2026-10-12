//! Docked Pioneer decks, rendered from the server's shared collision plan.
use super::*;
use arkline_protocol::{pioneer_interior_obstacles, StationObstacleKind, PIONEER_INTERIOR_ENTRY};
fn block(p: &mut Parts, min: Vec3, max: Vec3, color: Vec3, emission: f32) {
    p.draw(
        0,
        transform((min + max) * 0.5, (max - min) * 0.5, Quat::IDENTITY),
        color,
        emission,
    );
}
pub(super) fn parts(deck: u8) -> Vec<ModelPart> {
    cached_parts(deck).to_vec()
}
fn cached_parts(deck: u8) -> &'static [ModelPart] {
    static DECKS: std::sync::OnceLock<[Vec<ModelPart>; 2]> = std::sync::OnceLock::new();
    &DECKS.get_or_init(|| [build_parts(1), build_parts(2)])[usize::from(deck == 2)]
}
fn build_parts(deck: u8) -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    let steel = Vec3::new(0.045, 0.065, 0.072);
    block(
        &mut p,
        Vec3::new(0.0, -0.1, 0.0),
        Vec3::new(32.0, 0.0, 14.0),
        steel,
        0.0,
    );
    block(
        &mut p,
        Vec3::new(0.0, 3.2, 0.0),
        Vec3::new(32.0, 3.3, 14.0),
        steel,
        0.0,
    );
    for o in pioneer_interior_obstacles() {
        let min = Vec3::from_array(o.min.map(|v| v as f32));
        let max = Vec3::from_array(o.max.map(|v| v as f32));
        let fixture = matches!(o.kind, StationObstacleKind::Storage);
        block(
            &mut p,
            min,
            max,
            if fixture {
                Vec3::new(0.09, 0.13, 0.14)
            } else {
                steel
            },
            0.0,
        );
        if fixture {
            block(
                &mut p,
                Vec3::new(min.x + 0.1, max.y - 0.5, max.z + 0.005),
                Vec3::new(max.x - 0.1, max.y - 0.05, max.z + 0.02),
                Vec3::new(0.015, 0.25, 0.1),
                0.4,
            );
        }
    }
    for &(_, x, z) in arkline_protocol::pioneer_rooms(deck) {
        let x = x as f32;
        let z = z as f32;
        let door = x + 8.933333 / 2.0;
        let face = if z < 7.0 { 5.65 } else { 8.2 };
        for side in [-1.0, 1.0] {
            block(
                &mut p,
                Vec3::new(door + side * 0.76 - 0.025, 0.0, face - 0.01),
                Vec3::new(door + side * 0.76 + 0.025, 2.45, face + 0.17),
                Vec3::new(0.025, 0.35, 0.14),
                0.5,
            );
        }
        block(
            &mut p,
            Vec3::new(x + 0.6, 3.15, z + 2.2),
            Vec3::new(x + 8.2, 3.18, z + 2.4),
            Vec3::new(0.5, 0.65, 0.55),
            0.7,
        );
    }
    // Raised ceiling ribs and recessed light strips make the access path readable.
    for x in (2..32).step_by(3) {
        block(
            &mut p,
            Vec3::new(x as f32, 3.05, 5.9),
            Vec3::new(x as f32 + 0.10, 3.2, 8.1),
            Vec3::new(0.10, 0.14, 0.15),
            0.0,
        );
        block(
            &mut p,
            Vec3::new(x as f32 + 0.15, 3.17, 6.4),
            Vec3::new(x as f32 + 1.4, 3.19, 7.6),
            Vec3::new(0.40, 0.55, 0.45),
            0.6,
        );
        block(
            &mut p,
            Vec3::new(x as f32, 0.001, 5.9),
            Vec3::new(x as f32 + 0.015, 0.003, 8.1),
            Vec3::new(0.015, 0.025, 0.03),
            0.0,
        );
    }
    for x in (1..32).step_by(2) {
        block(
            &mut p,
            Vec3::new(x as f32, 0.008, 6.95),
            Vec3::new(x as f32 + 0.7, 0.012, 7.05),
            Vec3::new(0.02, 0.38, 0.15),
            0.35,
        );
    }
    let lift = Vec3::from_array(arkline_protocol::PIONEER_LIFT_POINT.map(|v| v as f32));
    block(
        &mut p,
        lift + Vec3::new(-0.9, 0.005, -0.9),
        lift + Vec3::new(0.9, 0.01, 0.9),
        Vec3::new(0.30, 0.23, 0.05),
        0.3,
    );
    let entry = if deck == 1 {
        PIONEER_INTERIOR_ENTRY
    } else {
        arkline_protocol::PIONEER_LIFT_POINT
    };
    let e = Vec3::from_array(entry.map(|v| v as f32));
    block(
        &mut p,
        e + Vec3::new(-0.6, 0.01, -0.6),
        e + Vec3::new(0.6, 0.02, 0.6),
        Vec3::new(0.15, 0.5, 0.25),
        0.3,
    );
    if deck == arkline_protocol::PIONEER_CONNECTED_ROOM.deck {
        let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
        // Wall-flush service panel and ceiling conduit do not change walkable clearance.
        let panel = Vec3::from_array(d.breaker_position.map(|v| v as f32));
        block(
            &mut p,
            panel - Vec3::new(0.04, 0.5, 0.35),
            panel + Vec3::new(-0.01, 0.5, 0.35),
            steel * 1.8,
            0.,
        );
        for z in [2.72, 2.9, 3.08, 3.26] {
            block(
                &mut p,
                Vec3::new(4.241, 1.15, z),
                Vec3::new(4.245, 1.65, z + 0.025),
                steel * 0.3,
                0.,
            );
        }
        block(
            &mut p,
            Vec3::new(4.28, 2.9, 0.3),
            Vec3::new(4.32, 2.94, 5.3),
            Vec3::new(0.1, 0.14, 0.12),
            0.,
        );
        block(
            &mut p,
            Vec3::new(4.3, 2.9, 0.28),
            Vec3::new(10.8, 2.94, 0.32),
            Vec3::new(0.1, 0.14, 0.12),
            0.,
        );
        let vent = Vec3::from_array(d.vent_position.map(|v| v as f32));
        block(
            &mut p,
            vent - Vec3::new(0.42, 0.25, 0.04),
            vent + Vec3::new(0.42, 0.25, 0.04),
            steel * 1.5,
            0.,
        );
        for i in 0..6 {
            let x = vent.x - 0.35 + i as f32 * 0.13;
            block(
                &mut p,
                Vec3::new(x, vent.y - 0.2, vent.z + 0.04),
                Vec3::new(x + 0.045, vent.y + 0.2, vent.z + 0.06),
                steel * 0.3,
                0.,
            );
        }
    }
    p.0
}
/// State-dependent parts are tiny; the detailed room shell stays cached.
pub(super) fn parts_with_room(
    deck: u8,
    room: Option<&arkline_protocol::PioneerRoomState>,
) -> Vec<ModelPart> {
    let mut parts = parts(deck);
    let Some(s) = room.filter(|_| deck == arkline_protocol::PIONEER_CONNECTED_ROOM.deck) else {
        return parts;
    };
    // Suppress the legacy decorative airlock strips: only actual outputs emit here.
    parts.retain(|(_, m, _, e)| {
        let c = m.w_axis;
        !(*e > 0. && c.x >= 4.2 && c.x <= 13.14 && c.z < 5.9)
    });
    let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
    let mut p = Parts(parts);
    for (point, half, color, on) in [
        (
            d.light_position,
            Vec3::new(1.5, 0.025, 0.16),
            Vec3::new(0.50, 0.65, 0.55),
            s.light_on,
        ),
        (
            d.emergency_position,
            Vec3::new(0.2, 0.08, 0.03),
            Vec3::new(1., 0.55, 0.27),
            s.emergency_light_on,
        ),
        (
            d.breaker_position,
            Vec3::new(0.012, 0.10, 0.12),
            Vec3::new(0.03, 0.6, 0.2),
            s.commissioned && s.breaker_closed,
        ),
        (
            d.vent_position,
            Vec3::new(0.05, 0.03, 0.07),
            Vec3::new(0.03, 0.5, 0.25),
            s.vent_running,
        ),
    ] {
        let center = Vec3::from_array(point.map(|v| v as f32));
        block(
            &mut p,
            center - half,
            center + half,
            if on { color } else { Vec3::splat(0.025) },
            if on { 0.8 } else { 0. },
        );
    }
    if !s.door_open {
        block(
            &mut p,
            Vec3::from_array(d.door_min.map(|v| v as f32)),
            Vec3::from_array(d.door_max.map(|v| v as f32)),
            Vec3::new(0.10, 0.14, 0.15),
            0.,
        );
    }
    p.0
}
pub(super) fn camera_with_room(
    target: Vec3,
    desired: Vec3,
    deck: u8,
    room: Option<&arkline_protocol::PioneerRoomState>,
) -> Vec3 {
    let desired = camera(target, desired);
    if deck == 1 && room.is_some_and(|s| !s.door_open) {
        let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
        if let Some(t) = quarters::segment_box(
            target,
            desired,
            Vec3::from_array(d.door_min.map(|v| v as f32)) - Vec3::splat(0.12),
            Vec3::from_array(d.door_max.map(|v| v as f32)) + Vec3::splat(0.12),
        ) {
            return target.lerp(desired, (t - 0.03).max(0.));
        }
    }
    desired
}
pub(super) fn device_labels(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vp: Mat4,
    pilot: &PilotView,
    room: Option<&arkline_protocol::PioneerRoomState>,
    live: bool,
) {
    let Some(a) = pilot
        .station_avatar
        .as_ref()
        .filter(|a| a.interior_deck == 1)
    else {
        return;
    };
    let Some(s) = room else {
        return;
    };
    let at = Vec3::from_array(
        arkline_protocol::PIONEER_CONNECTED_ROOM
            .breaker_position
            .map(|v| v as f32),
    ) + Vec3::new(0.08, 0.65, 0.);
    let eye = Vec3::from_array(a.position.map(|v| v as f32)) + Vec3::Y * 1.65;
    if eye.distance(at) > 5. || camera_with_room(eye, at, 1, room).distance(at) > 0.1 {
        return;
    }
    if let Some(screen) = project_point(vp, rect, at) {
        ui.painter_at(rect).text(
            screen,
            egui::Align2::CENTER_BOTTOM,
            format!(
                "AIRLOCK ENGINEERING\n{:.1} kPa · {}\nStation controls → engineering",
                s.pressure_pa as f64 / 1000.,
                if live { "SERVER STATE" } else { "LAST KNOWN" }
            ),
            egui::FontId::proportional(12.),
            egui::Color32::LIGHT_GREEN,
        );
    }
}
pub(super) fn camera(target: Vec3, desired: Vec3) -> Vec3 {
    let desired = Vec3::new(
        desired.x.clamp(0.25, 31.75),
        desired.y.clamp(0.4, 2.95),
        desired.z.clamp(0.25, 13.75),
    );
    let mut limit = 1.0f32;
    for o in pioneer_interior_obstacles() {
        if let Some(t) = quarters::segment_box(
            target,
            desired,
            Vec3::from_array(o.min.map(|v| v as f32)) - Vec3::splat(0.12),
            Vec3::from_array(o.max.map(|v| v as f32)) + Vec3::splat(0.12),
        ) {
            limit = limit.min((t - 0.03).max(0.0));
        }
    }
    target.lerp(desired, limit)
}
pub(super) fn labels(ui: &mut egui::Ui, rect: egui::Rect, vp: Mat4, pilot: &PilotView, live: bool) {
    let Some(a) = pilot.station_avatar.as_ref() else {
        return;
    };
    let pos = Vec3::from_array(a.position.map(|v| v as f32));
    let room = arkline_protocol::pioneer_rooms(a.interior_deck)
        .iter()
        .find(|(_, x, z)| {
            a.position[0] >= *x
                && a.position[0] <= *x + 8.933334
                && a.position[2] >= *z
                && a.position[2] <= *z + 5.4
        })
        .map_or("ACCESS CORRIDOR", |(name, _, _)| *name);
    let text = if room == "POWER CONTROL" {
        pilot
            .power
            .as_ref()
            .map_or("Power telemetry unavailable".into(), |p| {
                format!(
                    "Capacitor {} / {} J · reactor {} W",
                    p.charge_j, p.capacity_j, p.regen_w
                )
            })
    } else if room == "CARGO ACCESS" {
        format!(
            "Confirmed cargo {} ore / {} capacity",
            pilot.ore,
            arkline_protocol::cargo_capacity(pilot.ship_class)
        )
    } else {
        "Other fixtures remain proposed. Airlock device status is in Station engineering; lift is in the access core.".into()
    };
    ui.painter().text(
        rect.left_top() + egui::vec2(20.0, 135.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{room}\n{} · {text}",
            if live {
                "LIVE SHIP DATA"
            } else {
                "LAST KNOWN DATA"
            }
        ),
        egui::FontId::proportional(14.0),
        egui::Color32::from_rgb(165, 230, 185),
    );
    for &(name, x, z) in arkline_protocol::pioneer_rooms(a.interior_deck) {
        let face = if z < 7.0 { 5.65 } else { 8.2 };
        let at = Vec3::new((x + 8.933333 / 2.0) as f32, 2.7, face as f32);
        let blocked = pioneer_interior_obstacles().iter().any(|o| {
            quarters::segment_box(
                pos + Vec3::Y * 1.6,
                at,
                Vec3::from_array(o.min.map(|v| v as f32)),
                Vec3::from_array(o.max.map(|v| v as f32)),
            )
            .is_some_and(|t| t < 0.98)
        });
        if pos.distance(at) < 8.0 && !blocked {
            if let Some(screen) = project_point(vp, rect, at) {
                ui.painter().text(
                    screen,
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::proportional(12.0),
                    egui::Color32::LIGHT_GREEN,
                );
            }
        }
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn confirmed_device_outputs_control_light_emission_and_door_geometry() {
        let mut s = arkline_protocol::PioneerRoomState::uncommissioned("owned".into());
        let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
        let lamp = Vec3::from_array(d.light_position.map(|v| v as f32));
        let emission = |parts: Vec<ModelPart>| {
            parts
                .into_iter()
                .find(|(_, m, _, _)| m.w_axis.truncate().distance(lamp) < 0.001)
                .unwrap()
                .3
        };
        assert_eq!(emission(parts_with_room(1, Some(&s))), 0.);
        s.light_on = true;
        assert!(emission(parts_with_room(1, Some(&s))) > 0.7);
        let open = parts_with_room(1, Some(&s));
        s.door_open = false;
        assert_eq!(parts_with_room(1, Some(&s)).len(), open.len() + 1);
        let target = Vec3::new(8.66, 1.5, 4.5);
        let eye = Vec3::new(8.66, 1.5, 7.);
        assert!(camera_with_room(target, eye, 1, Some(&s)).z < 5.65);
        assert_eq!(parts_with_room(2, Some(&s)), parts(2));
        s.light_on = false;
        s.emergency_light_on = true;
        let emergency = Vec3::from_array(d.emergency_position.map(|v| v as f32));
        assert!(parts_with_room(1, Some(&s)).iter().any(|(_, m, c, e)| m
            .w_axis
            .truncate()
            .distance(emergency)
            < 0.001
            && c.x > c.y
            && *e > 0.7));
    }
    #[test]
    fn cached_decks_preserve_geometry_and_materials() {
        for deck in [1, 2] {
            assert_eq!(super::parts(deck), super::build_parts(deck));
            assert!(std::ptr::eq(
                super::cached_parts(deck),
                super::cached_parts(deck)
            ));
        }
    }
    #[test]
    #[ignore = "Explicit CPU preparation measurement, not a GPU frame-rate benchmark"]
    fn static_deck_preparation_profile() {
        use std::hint::black_box;
        black_box(super::parts(1));
        let start = std::time::Instant::now();
        for _ in 0..2000 {
            black_box(super::build_parts(1));
        }
        let rebuilt = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..2000 {
            black_box(super::parts(1));
        }
        eprintln!(
            "2000 actual deck preparation calls: rebuild={rebuilt:?}, cached_copy={:?}, parts={}",
            start.elapsed(),
            super::parts(1).len()
        );
    }
    use super::*;
    #[test]
    fn camera_stays_below_ceiling_and_on_same_side_of_bulkhead() {
        let target = Vec3::new(10.0, 1.25, 7.0);
        let eye = camera(target, Vec3::new(10.0, 6.0, 2.0));
        assert!(eye.y <= 2.95 && eye.z > 5.8);
        assert!(!parts(1).is_empty());
    }
}
