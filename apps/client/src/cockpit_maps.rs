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
    for planet in arkline_protocol::planets()
        .into_iter()
        .filter(|p| p.system_id == own.system_id)
    {
        contacts.push((
            super::super::SelectedObject::Planet(planet.id),
            arkline_protocol::PLANET_APPROACH_POSITION,
            planet.name,
        ));
    }
    for prop in snapshot
        .surface_props
        .iter()
        .filter(|p| p.system_id == own.system_id)
    {
        contacts.push((
            super::super::SelectedObject::Prop(prop.id),
            prop.position,
            format!("{:?} #{}", prop.kind, prop.id),
        ));
    }
    let mut picked = None;
    for (index, tactical) in [(4, false), (5, true)] {
        let (center, size) = cockpit_screens(own.ship_class)[index];
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

/// Delegated workers can remain simulated without an interactive player session.
/// Authorization and freshness are rechecked on every authoritative snapshot.
pub(super) fn remote_worker<'a>(
    snapshot: &'a Snapshot,
    controller: &str,
    target: &str,
    now: u64,
    connected: bool,
) -> Option<&'a PilotView> {
    if !connected || now.saturating_sub(snapshot.server_time_ms) > 1500 {
        return None;
    }
    if target != controller
        && !snapshot
            .fleet_links
            .iter()
            .any(|link| link.pilot == target && link.controller == controller)
    {
        return None;
    }
    snapshot.pilots.iter().find(|p| p.name == target)
}
pub(super) fn remote_ship_frame(
    main: &Frame,
    snapshot: &Snapshot,
    worker: &PilotView,
    aspect: f32,
) -> Frame {
    let mut feed = main.clone();
    feed.space = Space {
        origin: super::super::motion::position(worker, main.now),
    };
    feed.system_id = worker.system_id;
    feed.own = worker.name.clone();
    feed.ship_class = worker.ship_class;
    feed.rotation = ship_orientation_at(worker, main.now);
    let radius = (worker.ship_class.spec().length_m as f32 / 100.0 * 1.7).max(1.5);
    let phase = (main.now % 120_000) as f32 / 120_000.0 * std::f32::consts::TAU;
    feed.eye = Vec3::new(phase.cos() * radius, radius * 0.45, phase.sin() * radius);
    let view = Mat4::look_at_rh(feed.eye, Vec3::ZERO, Vec3::Y);
    feed.vp = Mat4::perspective_rh_gl(1.05, aspect.max(0.1), 0.002, 1200.0) * view;
    feed.planet_vp = Mat4::perspective_rh_gl(1.05, aspect.max(0.1), 1.0, 300_000.0) * view;
    feed.cockpit = false;
    feed.drone_camera = true;
    feed.camera_drone = None;
    feed.docking = None;
    feed.scene = std::sync::Arc::new(super::FrameScene {
        traffic: snapshot
            .traffic
            .iter()
            .filter(|a| a.system_id == worker.system_id)
            .cloned()
            .collect(),
        surface_props: snapshot
            .surface_props
            .iter()
            .filter(|p| p.system_id == worker.system_id)
            .cloned()
            .collect(),
        pilots: snapshot
            .pilots
            .iter()
            .filter(|p| {
                p.system_id == worker.system_id
                    && (p.location != Location::Docked || p.name == worker.name)
            })
            .cloned()
            .collect(),
        asteroids: snapshot
            .asteroids
            .iter()
            .filter(|a| a.system_id == worker.system_id)
            .cloned()
            .collect(),
        gates: arkline_protocol::gates()
            .into_iter()
            .filter(|g| g.system_id == worker.system_id)
            .collect(),
    });
    feed
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    fn snapshot() -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "server_time_ms":1000,"pilots":[
                {"name":"Controller","credits":1000,"ore":0,"location":Location::Station,"system_id":1},
                {"name":"Worker","credits":1000,"ore":0,"location":Location::Belt,"system_id":2,
                 "flight":{"position":[299195741400.0,2000.0,3000.0],"velocity":[0.0,0.0,0.0],"heading":[1.0,0.0,0.0],"mode":NavigationMode::Stop,"updated_at_ms":1000}}
            ],"orders":[],"belt_ore":100000,"ledger_sequence":0,"ledger_hash":"",
            "fleet_links":[{"pilot":"Worker","controller":"Controller"}],
            "asteroids":[{"id":1,"system_id":1,"name":"Other","position":[0.0,0.0,0.0],"radius":100.0,"ore":100},
                         {"id":2,"system_id":2,"name":"Local","position":[299195741400.0,0.0,0.0],"radius":100.0,"ore":100}]
        })).unwrap()
    }
    fn main_frame() -> Frame {
        Frame {
            exterior: None,
            lighting_quality: Default::default(),
            cockpit_vp: Mat4::IDENTITY,
            snapshot_time: 1000,
            planet_vp: Mat4::IDENTITY,
            system_id: 1,
            ship_class: ShipClass::Pioneer,
            drone_camera: false,
            camera_drone: None,
            scene: std::sync::Arc::new(super::FrameScene {
                traffic: vec![],
                surface_props: vec![],
                asteroids: vec![],
                gates: vec![],
                pilots: vec![],
            }),
            space: Space::default(),
            cockpit: true,
            rotation: Quat::IDENTITY,
            connected: true,
            vp: Mat4::IDENTITY,
            eye: Vec3::new(1.0, 2.0, 3.0),
            now: 1000,
            own: "Controller".into(),
            docking: None,
        }
    }
    #[test]
    fn remote_authorization_is_live_revocable_and_does_not_require_worker_session() {
        let mut s = snapshot();
        assert!(s.online_pilots.is_empty());
        assert!(remote_worker(&s, "Controller", "Worker", 1000, true).is_some());
        assert!(remote_worker(&s, "Stranger", "Worker", 1000, true).is_none());
        assert!(remote_worker(&s, "Controller", "Worker", 1000, false).is_none());
        assert!(remote_worker(&s, "Controller", "Worker", 2501, true).is_none());
        s.fleet_links.clear();
        assert!(remote_worker(&s, "Controller", "Worker", 1000, true).is_none());
        assert!(remote_worker(&s, "Controller", "Controller", 1000, true).is_some());
    }
    #[test]
    fn worker_camera_rebases_actual_pose_filters_system_and_leaves_main_unchanged() {
        let mut s = snapshot();
        s.pilots[1]
            .mining_modules
            .push(arkline_protocol::MiningModule {
                module_id: 0,
                target_id: Some(2),
                cycle_started_at_ms: Some(1000),
                cycle_completes_at_ms: Some(7000),
                stop_reason: None,
            });
        let worker = &s.pilots[1];
        let main = main_frame();
        let feed = remote_ship_frame(&main, &s, worker, 1.5);
        assert!(!std::sync::Arc::ptr_eq(&feed.scene, &main.scene));
        assert_eq!(feed.space.origin, worker.flight.as_ref().unwrap().position);
        assert_eq!(feed.system_id, 2);
        assert_eq!(feed.own, "Worker");
        assert_eq!(feed.scene.pilots.len(), 1);
        assert_eq!(feed.scene.pilots[0].mining_modules, worker.mining_modules);
        assert!(feed.scene.gates.iter().all(|g| g.system_id == 2));
        assert_eq!(feed.scene.asteroids.len(), 1);
        assert_eq!(feed.scene.asteroids[0].id, 2);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 200.0));
        assert!(
            project_point(feed.vp, rect, Vec3::ZERO)
                .unwrap()
                .distance(rect.center())
                < 0.001
        );
        let mut moved = worker.clone();
        moved.flight.as_mut().unwrap().position[1] += 5000.0;
        let moved_feed = remote_ship_frame(&main, &s, &moved, 1.5);
        assert_eq!(moved_feed.space.origin[1] - feed.space.origin[1], 5000.0);
        assert_eq!(main.own, "Controller");
        assert_eq!(main.system_id, 1);
        assert!(main.cockpit);
        assert_eq!(main.eye, Vec3::new(1.0, 2.0, 3.0));
        assert!(!feed.cockpit);
        assert!(feed.drone_camera);
    }
}
