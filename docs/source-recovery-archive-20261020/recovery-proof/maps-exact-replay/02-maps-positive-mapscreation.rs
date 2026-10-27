//! Navigation, radar and camera feeds on the cockpit's physical display mounts.
use super::*;

pub(super) const NAV_CANVAS_SIZE: egui::Vec2 = egui::vec2(256.0, 128.0);

/// The exact display-face plane used by the cockpit renderer. The caller passes
/// its actual mount matrix; this helper never attaches a mount to the camera.
#[derive(Clone)]
pub(super) struct CockpitSurface {
    viewport: egui::Rect,
    canvas: egui::Rect,
    local_to_clip: Mat4,
    plane_x: f32,
    center: Vec3,
    half: Vec3,
    bounds: egui::Rect,
    index: usize,
}

#[derive(Clone, Copy)]
struct SurfaceVertex {
    clip: glam::Vec4,
    uv: egui::Pos2,
    color: egui::Color32,
}

impl SurfaceVertex {
    fn lerp(self, other: Self, t: f32) -> Self {
        let a = self.color.to_array();
        let b = other.color.to_array();
        let color: [u8; 4] = std::array::from_fn(|i| {
            (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8
        });
        Self {
            clip: self.clip.lerp(other.clip, t),
            uv: self.uv.lerp(other.uv, t),
            color: egui::Color32::from_rgba_premultiplied(color[0], color[1], color[2], color[3]),
        }
    }
}

fn clip_surface_polygon(
    input: Vec<SurfaceVertex>,
    distance: impl Fn(SurfaceVertex) -> f32,
) -> Vec<SurfaceVertex> {
    let Some(mut previous) = input.last().copied() else {
        return input;
    };
    let mut output = Vec::with_capacity(input.len() + 1);
    let mut before = distance(previous);
    for current in input {
        let after = distance(current);
        if (before >= 0.0) != (after >= 0.0) {
            output.push(previous.lerp(current, before / (before - after)));
        }
        if after >= 0.0 {
            output.push(current);
        }
        previous = current;
        before = after;
    }
    output
}

fn clip_surface_frustum(mut polygon: Vec<SurfaceVertex>) -> Vec<SurfaceVertex> {
    for plane in 0..7 {
        polygon = clip_surface_polygon(polygon, |v| match plane {
            0 => v.clip.w - 1e-6,
            1 => v.clip.x + v.clip.w,
            2 => v.clip.w - v.clip.x,
            3 => v.clip.y + v.clip.w,
            4 => v.clip.w - v.clip.y,
            5 => v.clip.z + v.clip.w,
            _ => v.clip.w - v.clip.z,
        });
        if polygon.is_empty() {
            break;
        }
    }
    polygon
}

impl CockpitSurface {
    pub(super) fn new(
        viewport: egui::Rect,
        local_to_clip: Mat4,
        class: ShipClass,
        index: usize,
        canvas_size: egui::Vec2,
    ) -> Option<Self> {
        if !viewport.is_finite()
            || viewport.width() <= 0.0
            || viewport.height() <= 0.0
            || !local_to_clip.is_finite()
            || !canvas_size.is_finite()
            || canvas_size.min_elem() <= 0.0
        {
            return None;
        }
        let (center, size) = *cockpit_screens(class).get(index)?;
        let plane_x = center.x - 0.025 - 0.006;
        // A perspective projection maps its camera center to (0,0,z,0).
        // Recover it in mount-local coordinates to reject the display's back.
        let camera = local_to_clip.inverse() * glam::Vec4::new(0.0, 0.0, 1.0, 0.0);
        if !camera.is_finite() || camera.w.abs() < 1e-8 || camera.x / camera.w >= plane_x {
            return None;
        }
        let mut result = Self {
            viewport,
            canvas: egui::Rect::from_min_size(egui::Pos2::ZERO, canvas_size),
            local_to_clip,
            plane_x,
            center,
            half: Vec3::new(0.0, size.y - 0.008, size.z - 0.008),
            bounds: egui::Rect::NOTHING,
            index,
        };
        let corners = [
            result.canvas.left_top(),
            result.canvas.right_top(),
            result.canvas.right_bottom(),
            result.canvas.left_bottom(),
        ];
        let polygon = clip_surface_frustum(
            corners
                .into_iter()
                .map(|p| result.vertex(p, egui::Pos2::ZERO, egui::Color32::WHITE))
                .collect(),
        );
        for vertex in polygon {
            result
                .bounds
                .extend_with(result.screen_position(vertex.clip)?);
        }
        if !result.bounds.is_finite() || result.bounds.width() < 1.0 || result.bounds.height() < 1.0
        {
            return None;
        }
        Some(result)
    }

    pub(super) fn canvas_rect(&self) -> egui::Rect {
        self.canvas
    }
    pub(super) fn bounds(&self) -> egui::Rect {
        self.bounds
    }
    pub(super) fn contains(&self, point: egui::Pos2) -> bool {
        self.unproject(point).is_some()
    }
    #[cfg(test)]
    pub(super) fn project(&self, point: egui::Pos2) -> Option<egui::Pos2> {
        if !self.canvas.contains(point) {
            return None;
        }
        let clip = self
            .vertex(point, egui::Pos2::ZERO, egui::Color32::WHITE)
            .clip;
        if clip.w <= 1e-6 || clip.x.abs() > clip.w || clip.y.abs() > clip.w || clip.z.abs() > clip.w
        {
            return None;
        }
        self.screen_position(clip)
    }

    pub(super) fn unproject(&self, point: egui::Pos2) -> Option<egui::Pos2> {
        let (origin, direction) = screen_ray(self.local_to_clip, self.viewport, point)?;
        if direction.x.abs() < 1e-7 {
            return None;
        }
        let t = (self.plane_x - origin.x) / direction.x;
        if !t.is_finite() || t < 0.0 {
            return None;
        }
        let hit = origin + direction * t;
        let clip = self.local_to_clip * hit.extend(1.0);
        if !clip.is_finite() || clip.w <= 0.0 || clip.z < -clip.w || clip.z > clip.w {
            return None;
        }
        let u = (hit.z - self.center.z) / (2.0 * self.half.z) + 0.5;
        let v = 0.5 - (hit.y - self.center.y) / (2.0 * self.half.y);
        if !(-1e-5..=1.0 + 1e-5).contains(&u) || !(-1e-5..=1.0 + 1e-5).contains(&v) {
            return None;
        }
        Some(
            self.canvas.min
                + egui::vec2(
                    u.clamp(0.0, 1.0) * self.canvas.width(),
                    v.clamp(0.0, 1.0) * self.canvas.height(),
                ),
        )
    }

    fn vertex(&self, point: egui::Pos2, uv: egui::Pos2, color: egui::Color32) -> SurfaceVertex {
        let u = (point.x - self.canvas.left()) / self.canvas.width();
        let v = (point.y - self.canvas.top()) / self.canvas.height();
        let local = Vec3::new(
            self.plane_x,
            self.center.y + (0.5 - v) * self.half.y * 2.0,
            self.center.z + (u - 0.5) * self.half.z * 2.0,
        );
        SurfaceVertex {
            clip: self.local_to_clip * local.extend(1.0),
            uv,
            color,
        }
    }

    fn screen_position(&self, clip: glam::Vec4) -> Option<egui::Pos2> {
        if !clip.is_finite() || clip.w <= 1e-7 {
            return None;
        }
        Some(
            self.viewport.center()
                + egui::vec2(
                    clip.x / clip.w * self.viewport.width() * 0.5,
                    -clip.y / clip.w * self.viewport.height() * 0.5,
                ),
        )
    }

    fn project_mesh(&self, mesh: egui::epaint::Mesh, local_clip: egui::Rect) -> egui::epaint::Mesh {
        let clip = local_clip.intersect(self.canvas);
        let mut output = egui::epaint::Mesh::with_texture(mesh.texture_id);
        for triangle in mesh.indices.chunks_exact(3) {
            // Clip local content first, including glyphs that cross the bezel.
            let mut polygon: Vec<_> = triangle
                .iter()
                .map(|i| {
                    let v = mesh.vertices[*i as usize];
                    SurfaceVertex {
                        clip: glam::Vec4::new(v.pos.x, v.pos.y, 0.0, 1.0),
                        uv: v.uv,
                        color: v.color,
                    }
                })
                .collect();
            for edge in 0..4 {
                polygon = clip_surface_polygon(polygon, |v| match edge {
                    0 => v.clip.x - clip.left(),
                    1 => clip.right() - v.clip.x,
                    2 => v.clip.y - clip.top(),
                    _ => clip.bottom() - v.clip.y,
                });
            }
            let polygon = clip_surface_frustum(
                polygon
                    .into_iter()
                    .map(|v| self.vertex(egui::pos2(v.clip.x, v.clip.y), v.uv, v.color))
                    .collect(),
            );
            let start = output.vertices.len() as u32;
            for v in &polygon {
                if let Some(pos) = self.screen_position(v.clip) {
                    output.vertices.push(egui::epaint::Vertex {
                        pos,
                        uv: v.uv,
                        color: v.color,
                    });
                }
            }
            for index in 1..polygon.len().saturating_sub(1) {
                output.add_triangle(start, start + index as u32, start + index as u32 + 1);
            }
        }
        output
    }

    /// Collect ordinary egui shapes in display-local coordinates. The guard
    /// tessellates and projects them (including font glyphs) onto this real face.
    pub(super) fn begin_paint(&self, ui: &egui::Ui) -> SurfacePaint {
        let layer = egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new(("cockpit_surface_canvas", self.index)),
        );
        SurfacePaint {
            surface: self.clone(),
            output: ui.painter_at(self.viewport),
            painter: egui::Painter::new(ui.ctx().clone(), layer, self.canvas),
            layer,
        }
    }

    pub(super) fn paint(&self, ui: &egui::Ui, draw: impl FnOnce(&egui::Painter)) {
        let paint = self.begin_paint(ui);
        draw(paint.painter());
    }
}

pub(super) struct SurfacePaint {
    surface: CockpitSurface,
    output: egui::Painter,
    painter: egui::Painter,
    layer: egui::LayerId,
}
impl SurfacePaint {
    pub(super) fn painter(&self) -> &egui::Painter {
        &self.painter
    }
}
impl Drop for SurfacePaint {
    fn drop(&mut self) {
        let context = self.painter.ctx();
        let shapes = context.graphics_mut(|graphics| {
            let list = graphics.entry(self.layer);
            let shapes: Vec<_> = list.all_entries().cloned().collect();
            *list = Default::default();
            shapes
        });
        for primitive in context.tessellate(shapes, context.pixels_per_point()) {
            if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                let mesh = self.surface.project_mesh(mesh, primitive.clip_rect);
                if !mesh.indices.is_empty() {
                    self.output.add(egui::Shape::mesh(mesh));
                }
            }
        }
    }
}

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
    surface: &CockpitSurface,
    own: &PilotView,
    snapshot: &Snapshot,
    now: u64,
    connected: bool,
    selected: &super::super::SelectedObject,
    range: &mut f64,
) -> Option<ViewAction> {
    let world_layout = match super::super::snapshot_world_layout(snapshot) {
        Ok(layout) => layout,
        Err(error) => {
            surface.paint(ui, |painter| {
                painter.text(
                    surface.canvas_rect().center(),
                    egui::Align2::CENTER_CENTER,
                    error,
                    egui::FontId::monospace(10.0),
                    egui::Color32::RED,
                );
            });
            return None;
        }
    };
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
    for planet in world_layout
        .planets
        .iter()
        .filter(|p| p.system_id == own.system_id)
    {
        contacts.push((
            super::super::SelectedObject::Planet(planet.id),
            planet.approach_position,
            planet.name.clone(),
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
    let (index, tactical) = (4, false);
    {
        let rect = surface.canvas_rect().shrink(3.0);
        let paint = surface.begin_paint(ui);
        let painter = paint.painter();
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
        let physical_pointer = ui
            .input(|i| i.pointer.hover_pos())
            .and_then(|p| surface.unproject(p));
        let response = ui.interact(
            if physical_pointer.is_some() {
                surface.bounds()
            } else {
                egui::Rect::NOTHING
            },
            egui::Id::new(("cockpit_map", index)),
            egui::Sense::click(),
        );
        if physical_pointer.is_some() && response.hovered() {
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
            if let Some(pointer) = response
                .interact_pointer_pos()
                .and_then(|p| surface.unproject(p))
            {
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
        world_layout: main.scene.world_layout.clone(),
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
mod surface_tests {
    use super::*;

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(70.0, 40.0), egui::vec2(900.0, 650.0))
    }
    fn matrix(eye: Vec3, look: Quat) -> Mat4 {
        Mat4::perspective_rh_gl(COCKPIT_FOV, viewport().aspect_ratio(), 0.01, 100.0)
            * Mat4::look_at_rh(eye, eye + look * Vec3::X, look * Vec3::Y)
    }

    #[test]
    fn cockpit_surface_tracks_real_mount_camera_headlook_and_roll() {
        let neutral = CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, Quat::IDENTITY),
            ShipClass::Pioneer,
            5,
            NAV_CANVAS_SIZE,
        )
        .unwrap();
        let mut centers = Vec::new();
        for look in [
            Quat::IDENTITY,
            Quat::from_rotation_y(-0.25),
            Quat::from_rotation_x(0.4) * Quat::from_rotation_y(-0.2),
        ] {
            let surface = CockpitSurface::new(
                viewport(),
                matrix(Vec3::ZERO, look),
                ShipClass::Pioneer,
                5,
                NAV_CANVAS_SIZE,
            )
            .unwrap();
            let center = surface.project(surface.canvas.center()).unwrap();
            assert!(surface.contains(center));
            assert!(
                surface
                    .unproject(center)
                    .unwrap()
                    .distance(surface.canvas.center())
                    < 0.001
            );
            centers.push(center);
        }
        assert!(centers[0].distance(centers[1]) > 20.0);
        let mount = Mat4::from_scale_rotation_translation(
            Vec3::splat(0.7),
            Quat::IDENTITY,
            Vec3::new(0.3, 0.0, 0.0),
        );
        let mounted = CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, Quat::IDENTITY) * mount,
            ShipClass::Pioneer,
            5,
            NAV_CANVAS_SIZE,
        )
        .unwrap();
        assert!(
            neutral
                .bounds()
                .center()
                .distance(mounted.bounds().center())
                > 10.0
        );
    }

    #[test]
    fn cockpit_surface_inverse_hit_rejects_bounding_box_outside_actual_quad() {
        let look = Quat::from_rotation_x(0.5) * Quat::from_rotation_y(-0.2);
        let surface = CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, look),
            ShipClass::Pioneer,
            5,
            NAV_CANVAS_SIZE,
        )
        .unwrap();
        let bounds = surface.bounds();
        let outside = [
            bounds.left_top(),
            bounds.right_top(),
            bounds.left_bottom(),
            bounds.right_bottom(),
        ]
        .into_iter()
        .any(|point| !surface.contains(point));
        assert!(
            outside,
            "A tilted display is a quad, not its axis-aligned bounding box"
        );
        for local in [egui::pos2(32.0, 25.0), egui::pos2(220.0, 100.0)] {
            let point = surface.project(local).unwrap();
            assert!(surface.unproject(point).unwrap().distance(local) < 0.001);
        }
    }

    #[test]
    fn cockpit_surface_partially_visible_contents_clip_to_viewport_and_bezel() {
        let look = Quat::from_rotation_y(0.3);
        let surface = CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, look),
            ShipClass::Pioneer,
            5,
            NAV_CANVAS_SIZE,
        )
        .unwrap();
        assert!((surface.bounds().right() - viewport().right()).abs() < 0.01);
        let mut mesh = egui::epaint::Mesh::default();
        mesh.add_colored_rect(surface.canvas.expand(50.0), egui::Color32::WHITE);
        let projected = surface.project_mesh(mesh, surface.canvas);
        assert!(!projected.indices.is_empty());
        assert!(projected
            .vertices
            .iter()
            .all(|v| viewport().expand(0.01).contains(v.pos)));
        assert!(projected
            .vertices
            .iter()
            .all(|v| surface.unproject(v.pos).is_some()));
        assert!(projected
            .indices
            .iter()
            .all(|i| (*i as usize) < projected.vertices.len()));
    }

    #[test]
    fn cockpit_surface_naturally_rejects_backface_offscreen_and_near_clipped_mounts() {
        let canvas = NAV_CANVAS_SIZE;
        assert!(CockpitSurface::new(
            viewport(),
            matrix(
                Vec3::new(2.0, 0.0, 0.0),
                Quat::from_rotation_y(std::f32::consts::PI)
            ),
            ShipClass::Pioneer,
            5,
            canvas
        )
        .is_none());
        assert!(CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, Quat::from_rotation_y(std::f32::consts::PI)),
            ShipClass::Pioneer,
            5,
            canvas
        )
        .is_none());
        let behind_near =
            Mat4::perspective_rh_gl(COCKPIT_FOV, viewport().aspect_ratio(), 1.0, 100.0)
                * Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y);
        assert!(
            CockpitSurface::new(viewport(), behind_near, ShipClass::Pioneer, 5, canvas).is_none()
        );
    }

    #[test]
    fn cockpit_surface_text_and_shapes_are_real_projected_meshes_without_floating_text() {
        let surface = CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, Quat::from_rotation_y(-0.2)),
            ShipClass::Pioneer,
            5,
            NAV_CANVAS_SIZE,
        )
        .unwrap();
        let context = egui::Context::default();
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    surface.paint(ui, |painter| {
                        painter.rect_filled(surface.canvas, 0.0, egui::Color32::BLACK);
                        painter.text(
                            egui::pos2(-20.0, 40.0),
                            egui::Align2::LEFT_TOP,
                            "RADAR TEXT CROSSES BEZEL",
                            egui::FontId::monospace(12.0),
                            egui::Color32::WHITE,
                        );
                    });
                });
            },
        );
        assert!(!output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(_))));
        let meshes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(mesh) => Some(mesh),
                _ => None,
            })
            .collect();
        assert!(!meshes.is_empty());
        assert!(meshes
            .iter()
            .flat_map(|m| &m.vertices)
            .all(|v| surface.unproject(v.pos).is_some()));
    }
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
                world_layout: arkline_protocol::legacy_world_layout(),
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
        let mut main = main_frame();
        std::sync::Arc::get_mut(&mut main.scene)
            .unwrap()
            .world_layout = arkline_protocol::SceneLayoutRevision::OrbitalV1.definition();
        let feed = remote_ship_frame(&main, &s, worker, 1.5);
        assert!(!std::sync::Arc::ptr_eq(&feed.scene, &main.scene));
        assert_eq!(feed.scene.world_layout, main.scene.world_layout);
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
