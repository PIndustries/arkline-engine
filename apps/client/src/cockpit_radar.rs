//! Ship-relative contacts drawn on the starboard physical display.
use super::*;

pub(super) const RADAR_CANVAS_SIZE: egui::Vec2 = egui::vec2(300., 180.);

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    ui: &egui::Ui,
    surface: &CockpitSurface,
    own: &PilotView,
    snapshot: &Snapshot,
    now: u64,
    connected: bool,
    range: f64,
) {
    let origin = display_position(own, now);
    let rotation = ship_orientation_at(own, now).inverse();
    let range = range.max(1.0);
    let project = |position: [f64; 3]| -> Option<(egui::Vec2, egui::Vec2)> {
        let relative = glam::DVec3::from_array(std::array::from_fn(|i| position[i] - origin[i]));
        if !relative.is_finite() || relative.length() > range {
            return None;
        }
        let body = rotation * (relative / range).as_vec3();
        Some((
            egui::vec2(body.z, -body.x * 0.55 - body.y * 0.8),
            egui::vec2(body.z, -body.x * 0.55),
        ))
    };
    surface.paint(ui, |painter| {
        let rect = surface.canvas_rect().shrink(4.);
        painter.rect_filled(rect, 0., egui::Color32::from_rgb(0, 20, 15));
        let color = if connected && now.saturating_sub(snapshot.server_time_ms) <= 1500 {
            super::super::TEAL
        } else {
            super::super::AMBER
        };
        painter.text(
            rect.left_top() + egui::vec2(4., 2.),
            egui::Align2::LEFT_TOP,
            "HOLO / SHIP RELATIVE",
            egui::FontId::monospace(10.),
            color,
        );
        let center = rect.center();
        let radius = rect.width().min(rect.height()) * 0.38;
        for fraction in [0.25, 0.5, 1.] {
            let points = (0..=64)
                .map(|n| {
                    let angle = n as f32 / 64. * std::f32::consts::TAU;
                    center + egui::vec2(angle.sin(), angle.cos() * 0.55) * radius * fraction
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(0.8, color.gamma_multiply(0.4)),
            ));
        }
        let contact = |position, drone| {
            if let Some((point, baseline)) = project(position) {
                let point = center + point * radius;
                painter.line_segment(
                    [center + baseline * radius, point],
                    egui::Stroke::new(0.7, color.gamma_multiply(0.5)),
                );
                if drone {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(point, egui::vec2(4., 4.)),
                        0.,
                        egui::Stroke::new(1., super::super::AMBER),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.circle_filled(point, 2., color);
                }
            }
        };
        for asteroid in snapshot
            .asteroids
            .iter()
            .filter(|a| a.system_id == own.system_id && a.ore > 0)
        {
            contact(asteroid.position, false);
        }
        for pilot in snapshot
            .pilots
            .iter()
            .filter(|p| p.system_id == own.system_id)
        {
            if pilot.name != own.name && pilot.location != Location::Docked {
                contact(display_position(pilot, now), false);
            }
            for drone in fleet(pilot) {
                contact(drone.position, true);
            }
        }
        painter.circle_filled(center, 3., color);
        painter.text(
            rect.left_bottom() + egui::vec2(4., -2.),
            egui::Align2::LEFT_BOTTOM,
            super::super::format_distance(range),
            egui::FontId::monospace(9.),
            color,
        );
    });
}
