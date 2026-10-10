//! Body-relative instruments rendered into the same physical screen faces as the cockpit.
use super::*;

pub(super) fn fleet(pilot: &PilotView) -> Vec<&arkline_protocol::DroneView> {
    if pilot.drones.is_empty() {
        pilot.drone.iter().collect()
    } else {
        pilot.drones.iter().collect()
    }
}

pub(super) fn drone_view_matrix(
    drone: &arkline_protocol::DroneView,
    space: Space,
    aspect: f32,
    orbit: bool,
    now: u64,
) -> (Vec3, Mat4) {
    if !orbit {
        return drone_camera_matrix(drone, space, aspect);
    }
    let target = space.scene(drone.position);
    let phase = (now % 120_000) as f32 / 120_000.0 * std::f32::consts::TAU;
    let eye = target + Vec3::new(phase.cos() * 0.35, 0.16, phase.sin() * 0.35);
    (
        eye,
        Mat4::perspective_rh_gl(1.05, aspect.max(0.1), 0.002, 1200.0)
            * Mat4::look_at_rh(eye, target, Vec3::Y),
    )
}

pub(super) fn paint_camera_signal(
    painter: &egui::Painter,
    screen: egui::Rect,
    drone: Option<&arkline_protocol::DroneView>,
    now: u64,
    fresh: bool,
) {
    let painter = painter.with_clip_rect(screen);
    let signal = if fresh {
        drone.map_or(0.0, |d| d.signal_strength.clamp(0.0, 1.0)) as f32
    } else {
        0.0
    };
    let loss = 1.0 - signal;
    if drone.is_none() || !fresh || signal <= 0.0 {
        painter.rect_filled(screen, 0.0, egui::Color32::from_rgb(1, 8, 5));
    }
    if drone.is_some() && fresh && loss > 0.01 {
        // Bounded screen-space noise: no extra world pass, allocation or random generator.
        for line in 0..(loss * 90.0) as u32 {
            let hash = line
                .wrapping_mul(747796405)
                .wrapping_add((now / 65) as u32)
                .wrapping_mul(2891336453);
            let y = screen.top() + (hash & 65535) as f32 / 65535.0 * screen.height();
            let x = screen.left() + (hash >> 16) as f32 / 65535.0 * screen.width();
            let alpha = (loss * 160.0) as u8;
            painter.line_segment(
                [
                    egui::pos2(x, y),
                    egui::pos2((x + screen.width() * 0.35).min(screen.right()), y),
                ],
                egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgba_unmultiplied(160, 205, 175, alpha),
                ),
            );
        }
        painter.rect_filled(
            screen,
            0.0,
            egui::Color32::from_black_alpha((loss * 165.0) as u8),
        );
    }
    if signal <= 0.0 {
        painter.text(
            screen.center(),
            egui::Align2::CENTER_CENTER,
            if !fresh {
                "TELEMETRY STALE"
            } else if drone.is_none() {
                "NO CAMERA SELECTED"
            } else {
                "SIGNAL LOST / RETURNING"
            },
            egui::FontId::monospace(11.0),
            super::super::AMBER,
        );
    }
}

fn map_offset(
    relative: [f64; 3],
    rotation: Quat,
    range: f64,
    radius: f32,
    tactical: bool,
) -> egui::Vec2 {
    let body = rotation.inverse() * Vec3::from_array(relative.map(|v| (v / range) as f32));
    if tactical {
        egui::vec2(body.z, -body.x * 0.55 - body.y * 0.8) * radius
    } else {
        egui::vec2(body.z, -body.x) * radius
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_cockpit_maps(
    ui: &mut egui::Ui,
    viewport: egui::Rect,
    own: &PilotView,
    snapshot: &Snapshot,
    now: u64,
    connected: bool,
    selected: &super::super::SelectedObject,
    range: &mut f64,
) -> Option<ViewAction> {
    let origin = display_position(own, now);
    let rotation = ship_orientation_at(own, now);
    let mut contacts = Vec::new();
    for asteroid in snapshot
        .asteroids
        .iter()
        .filter(|a| a.system_id == own.system_id && a.ore > 0)
    {
        contacts.push((
            super::super::SelectedObject::Asteroid(asteroid.id),
            asteroid.position,
            format!("Ore {}", asteroid.id),
        ));
    }
    for pilot in snapshot.pilots.iter().filter(|p| {
        p.system_id == own.system_id && p.name != own.name && p.location != Location::Docked
    }) {
        contacts.push((
            super::super::SelectedObject::Pilot(pilot.name.clone()),
            display_position(pilot, now),
            pilot.name.clone(),
        ));
    }
    contacts.push((
        super::super::SelectedObject::Station,
        STATION_POSITION,
        "Station".into(),
    ));
    contacts.push((
        super::super::SelectedObject::Belt,
        BELT_POSITION,
        "Belt".into(),
    ));
    for gate in gates().into_iter().filter(|g| g.system_id == own.system_id) {
        contacts.push((
            super::super::SelectedObject::Gate(gate.id),
            gate.position,
            gate.name,
        ));
    }
    let mut picked = None;
    for (index, tactical) in [(4, false), (5, true)] {
        let (center, size) = cockpit_screens()[index];
        let rect = instrument_rect(viewport, center, size).shrink(3.0);
        let painter = ui.painter_at(rect);
        painter.rect_filled(
            rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(0, 20, 15, 235),
        );
        let color = if connected && now.saturating_sub(snapshot.server_time_ms) <= 1500 {
            super::super::TEAL
        } else {
            super::super::AMBER
        };
        painter.text(
            rect.left_top() + egui::vec2(4.0, 2.0),
            egui::Align2::LEFT_TOP,
            if tactical {
                "HOLO / SHIP RELATIVE"
            } else {
                "NAV / 2D LOCAL"
            },
            egui::FontId::monospace(10.0),
            color,
        );
        let response = ui.interact(
            rect,
            egui::Id::new(("cockpit_map", index)),
            egui::Sense::click(),
        );
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                *range = (*range * (-scroll as f64 * 0.008).exp())
                    .clamp(1000.0, arkline_protocol::AU_METERS * 20.0);
            }
            response.clone().on_hover_text("Scroll to change sensor range. Click a contact to select it. Map axes follow ship heading and roll.");
        }
        let map = rect.shrink2(egui::vec2(9.0, 18.0));
        let radius = map.width().min(map.height()) * 0.46;
        let center = map.center();
        for fraction in [0.25, 0.5, 1.0] {
            let points = (0..=64)
                .map(|n| {
                    let a = n as f32 / 64.0 * std::f32::consts::TAU;
                    center
                        + egui::vec2(a.sin(), a.cos() * if tactical { 0.55 } else { 1.0 })
                            * radius
                            * fraction
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(0.8, color.gamma_multiply(0.45)),
            ));
            painter.text(
                center + egui::vec2(radius * fraction, 2.0),
                egui::Align2::LEFT_TOP,
                super::super::format_distance(*range * fraction as f64),
                egui::FontId::monospace(8.0),
                color.gamma_multiply(0.65),
            );
        }
        painter.line_segment(
            [
                center + egui::vec2(0.0, -radius),
                center + egui::vec2(0.0, radius),
            ],
            egui::Stroke::new(0.5, color.gamma_multiply(0.3)),
        );
        painter.line_segment(
            [
                center + egui::vec2(-radius, 0.0),
                center + egui::vec2(radius, 0.0),
            ],
            egui::Stroke::new(0.5, color.gamma_multiply(0.3)),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                center + egui::vec2(0.0, -5.0),
                center + egui::vec2(-3.0, 4.0),
                center + egui::vec2(3.0, 4.0),
            ],
            color,
            egui::Stroke::NONE,
        ));
        let mut nearest: Option<(f32, super::super::SelectedObject)> = None;
        for (object, position, label) in &contacts {
            let relative = std::array::from_fn(|i| position[i] - origin[i]);
            if relative.iter().map(|v| v * v).sum::<f64>().sqrt() > *range {
                continue;
            }
            let point = center + map_offset(relative, rotation, *range, radius, tactical);
            if !map.contains(point) {
                continue;
            }
            if tactical {
                let body =
                    rotation.inverse() * Vec3::from_array(relative.map(|v| (v / *range) as f32));
                let baseline = center + egui::vec2(body.z, -body.x * 0.55) * radius;
                painter.line_segment(
                    [baseline, point],
                    egui::Stroke::new(0.8, color.gamma_multiply(0.45)),
                );
            }
            painter.circle_filled(point, if object == selected { 3.5 } else { 2.0 }, color);
            if object == selected {
                painter.text(
                    point + egui::vec2(5.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::monospace(9.0),
                    color,
                );
            }
            if let Some(pointer) = response.interact_pointer_pos() {
                let distance = pointer.distance(point);
                if distance < 10.0 && nearest.as_ref().is_none_or(|(d, _)| distance < *d) {
                    nearest = Some((distance, object.clone()));
                }
            }
        }
        for drone in fleet(own) {
            let relative = std::array::from_fn(|i| drone.position[i] - origin[i]);
            if relative.iter().map(|v| v * v).sum::<f64>().sqrt() <= *range {
                let point = center + map_offset(relative, rotation, *range, radius, tactical);
                if map.contains(point) {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(point, egui::vec2(4.0, 4.0)),
                        0.0,
                        egui::Stroke::new(1.0, super::super::AMBER),
                        egui::StrokeKind::Inside,
                    );
                }
            }
        }
        if response.clicked() {
            if let Some((_, object)) = nearest {
                picked = Some(ViewAction::Select(object));
            }
        }
        painter.text(
            rect.left_bottom() + egui::vec2(4.0, -2.0),
            egui::Align2::LEFT_BOTTOM,
            format!("RANGE {} / SCROLL", super::super::format_distance(*range)),
            egui::FontId::monospace(9.0),
            color,
        );
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ship_relative_contacts_follow_heading_and_preserve_distance_scale() {
        let ahead = map_offset([1000.0, 0.0, 0.0], Quat::IDENTITY, 10_000.0, 100.0, false);
        assert_eq!(ahead, egui::vec2(0.0, -10.0));
        let turned = map_offset(
            [1000.0, 0.0, 0.0],
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            10_000.0,
            100.0,
            false,
        );
        assert!((turned.x - 10.0).abs() < 0.001);
        assert!(turned.y.abs() < 0.001);
        let high = map_offset([0.0, 1000.0, 0.0], Quat::IDENTITY, 10_000.0, 100.0, true);
        assert!((high.y + 8.0).abs() < 0.001);
    }
}
