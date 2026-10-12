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
    p.0
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
        "Fixtures: proposed / non-operational. Lift landing is in the access core.".into()
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
