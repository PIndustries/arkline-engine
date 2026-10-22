//! Navigation, radar and camera feeds on the cockpit's physical display mounts.
use super::cockpit_displays::{CockpitDisplayRenderer, DisplayBatch, DisplayVertex};
use super::cockpit_occlusion::CockpitOcclusion;
use super::*;

pub(super) const NAV_CANVAS_SIZE: egui::Vec2 = egui::vec2(256.0, 128.0);

/// The exact display-face plane used by the cockpit renderer. The caller passes
/// its actual mount matrix; this helper never attaches a mount to the camera.
#[derive(Clone)]
pub(crate) struct CockpitSurface {
    viewport: egui::Rect,
    canvas: egui::Rect,
    local_to_clip: Mat4,
    plane_x: f32,
    center: Vec3,
    half: Vec3,
    bounds: egui::Rect,
    index: usize,
    renderer: Option<Arc<Mutex<CockpitDisplayRenderer>>>,
    occlusion: Option<(Arc<CockpitOcclusion>, Mat4, Mat4)>,
    registered_occlusion: bool,
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
        let (center, size) = *cockpit_screens(class).get(index)?;
        Self::from_plane(
            viewport,
            local_to_clip,
            center,
            Vec3::new(0., size.y - 0.008, size.z - 0.008),
            center.x - 0.025 - 0.006 - 0.0001,
            index,
            canvas_size,
        )
    }

    /// Physical panel coordinates in one registered body frame. The same plane
    /// supplies emitted UI triangles and pointer rays, without a camera mount.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn registered(
        viewport: egui::Rect,
        vp: Mat4,
        mount: Mat4,
        center: Vec3,
        half: Vec3,
        index: usize,
        canvas_size: egui::Vec2,
    ) -> Option<Self> {
        Self::from_plane(
            viewport,
            vp * mount,
            center,
            Vec3::new(0., half.y - 0.008, half.z - 0.008),
            // A 1 mm physical separation from the opaque glass backing keeps
            // the canted UI readable at the flight depth range. It remains
            // recessed behind the case lip; pointer and emitted plane agree.
            center.x - 0.025 - 0.006 - 0.001,
            index,
            canvas_size,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn from_plane(
        viewport: egui::Rect,
        local_to_clip: Mat4,
        center: Vec3,
        half: Vec3,
        plane_x: f32,
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
        if !center.is_finite()
            || !half.is_finite()
            || half.y <= 0.
            || half.z <= 0.
            || !plane_x.is_finite()
        {
            return None;
        }
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
            half,
            bounds: egui::Rect::NOTHING,
            index,
            renderer: None,
            occlusion: None,
            registered_occlusion: false,
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
    pub(super) fn with_renderer(mut self, renderer: Arc<Mutex<CockpitDisplayRenderer>>) -> Self {
        self.renderer = Some(renderer);
        self
    }
    pub(super) fn with_occlusion(
        mut self,
        scene: Arc<CockpitOcclusion>,
        vp: Mat4,
        mount: Mat4,
    ) -> Self {
        self.occlusion = Some((scene, vp, mount));
        self
    }
    pub(super) fn with_registered_occlusion(
        mut self,
        scene: Arc<CockpitOcclusion>,
        vp: Mat4,
        mount: Mat4,
    ) -> Self {
        self.occlusion = Some((scene, vp, mount));
        self.registered_occlusion = true;
        self
    }
    /// Exact glass-plane geometry for a cached video tile. Coordinates are
    /// display-local; the caller's GPU shader applies the same mount MVP.
    pub(super) fn feed_geometry(&self, tile: egui::Rect) -> Option<(Mat4, Vec3, Vec3)> {
        if !tile.is_finite() || tile.width() <= 0.0 || tile.height() <= 0.0 {
            return None;
        }
        let tile = tile.intersect(self.canvas);
        if tile.width() <= 0.0 || tile.height() <= 0.0 {
            return None;
        }
        Some((
            self.local_to_clip,
            self.local_point(tile.left_top()),
            self.local_point(tile.right_bottom()),
        ))
    }
    /// Platform-only projection for IME/caret placement; drawing uses 3D vertices.
    pub(crate) fn project(&self, point: egui::Pos2) -> Option<egui::Pos2> {
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
        if !self.registered_occlusion {
            if let Some((scene, vp, mount)) = &self.occlusion {
                return scene.hit_surface(
                    *vp,
                    self.viewport,
                    point,
                    self.index,
                    *mount,
                    self.canvas,
                );
            }
        }
        if !self.viewport.contains(point)
            || self.viewport.width() <= 0.0 || self.viewport.height() <= 0.0 {
            return None;
        }
        // Keep the physical plane intersection in double precision. Casting
        // the ray to f32 before intersecting a tilted plane near body X=35 m
        // can move an exact glass edge outside the existing UV allowance.
        // This does not change the viewport, UV allowance or scene-ray gate.
        let inverse = glam::DMat4::from_cols_array(
            &self.local_to_clip.to_cols_array().map(f64::from)).inverse();
        let x = (f64::from(point.x) - f64::from(self.viewport.center().x)) * 2.0
            / f64::from(self.viewport.width());
        let y = (f64::from(self.viewport.center().y) - f64::from(point.y)) * 2.0
            / f64::from(self.viewport.height());
        let near = inverse * glam::DVec4::new(x, y, -1.0, 1.0);
        let far = inverse * glam::DVec4::new(x, y, 1.0, 1.0);
        if !near.is_finite() || !far.is_finite()
            || near.w.abs() < 1e-7 || far.w.abs() < 1e-7 {
            return None;
        }
        let origin = near.truncate() / near.w;
        let direction = (far.truncate() / far.w - origin).normalize_or_zero();
        if !direction.is_finite() || direction.length_squared() <= 0.5
            || direction.x.abs() < 1e-7 {
            return None;
        }
        let t = (f64::from(self.plane_x) - origin.x) / direction.x;
        if !t.is_finite() || t < 0.0 {
            return None;
        }
        let hit = origin + direction * t;
        if self.registered_occlusion {
            let (scene, vp, mount) = self.occlusion.as_ref()?;
            if !scene.point_unblocked(*vp, self.viewport, point, mount.transform_point3(hit.as_vec3())) {
                return None;
            }
        }
        let clip = self.local_to_clip * hit.as_vec3().extend(1.0);
        if !clip.is_finite() || clip.w <= 0.0 || clip.z < -clip.w || clip.z > clip.w {
            return None;
        }
        let u = (hit.z - f64::from(self.center.z)) / (2.0 * f64::from(self.half.z)) + 0.5;
        let v = 0.5 - (hit.y - f64::from(self.center.y)) / (2.0 * f64::from(self.half.y));
        if !(-1e-5..=1.0 + 1e-5).contains(&u) || !(-1e-5..=1.0 + 1e-5).contains(&v) {
            return None;
        }
        Some(
            self.canvas.min
                + egui::vec2(
                    u.clamp(0.0, 1.0) as f32 * self.canvas.width(),
                    v.clamp(0.0, 1.0) as f32 * self.canvas.height(),
                ),
        )
    }

    fn local_point(&self, point: egui::Pos2) -> Vec3 {
        let u = (point.x - self.canvas.left()) / self.canvas.width();
        let v = (point.y - self.canvas.top()) / self.canvas.height();
        Vec3::new(
            self.plane_x,
            self.center.y + (0.5 - v) * self.half.y * 2.0,
            self.center.z + (u - 0.5) * self.half.z * 2.0,
        )
    }

    fn vertex(&self, point: egui::Pos2, uv: egui::Pos2, color: egui::Color32) -> SurfaceVertex {
        SurfaceVertex {
            clip: self.local_to_clip * self.local_point(point).extend(1.0),
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

    fn mesh_geometry(
        &self,
        mesh: &egui::epaint::Mesh,
        local_clip: egui::Rect,
    ) -> Vec<DisplayVertex> {
        let clip = local_clip.intersect(self.canvas);
        let mut output = Vec::new();
        if !clip.is_finite() || !clip.is_positive() || !mesh.is_valid() {
            return output;
        }
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
            if polygon
                .iter()
                .any(|vertex| !vertex.clip.is_finite() || !vertex.uv.is_finite())
            {
                continue;
            }
            for edge in 0..4 {
                polygon = clip_surface_polygon(polygon, |v| match edge {
                    0 => v.clip.x - clip.left(),
                    1 => clip.right() - v.clip.x,
                    2 => v.clip.y - clip.top(),
                    _ => clip.bottom() - v.clip.y,
                });
            }
            for index in 1..polygon.len().saturating_sub(1) {
                for vertex in [polygon[0], polygon[index], polygon[index + 1]] {
                    output.push(DisplayVertex {
                        position: self.local_point(egui::pos2(vertex.clip.x, vertex.clip.y)),
                        uv: vertex.uv,
                        color: vertex.color,
                    });
                }
            }
        }
        output
    }

    fn geometry(&self, primitives: &[egui::ClippedPrimitive]) -> Vec<DisplayBatch> {
        primitives
            .iter()
            .filter_map(|primitive| {
                let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                    return None;
                };
                let vertices = self.mesh_geometry(mesh, primitive.clip_rect);
                (!vertices.is_empty()).then_some(DisplayBatch {
                    texture: mesh.texture_id,
                    vertices,
                })
            })
            .collect()
    }

    /// Submit canonical UI meshes to a full-viewport GL callback. The GPU applies
    /// homogeneous clipping, perspective interpolation, and retained scene depth.
    pub(crate) fn submit(
        &self,
        ctx: &egui::Context,
        layer: egui::LayerId,
        primitives: Vec<egui::ClippedPrimitive>,
    ) {
        let Some(renderer) = self.renderer.clone() else {
            return;
        };
        let batches = self.geometry(&primitives);
        if batches.is_empty() {
            return;
        }
        let matrix = self.local_to_clip;
        #[cfg(target_arch = "wasm32")]
        let renderer = send_wrapper::SendWrapper::new(renderer);
        egui::Painter::new(ctx.clone(), layer, self.viewport).add(egui::PaintCallback {
            rect: self.viewport,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                if let Ok(mut renderer) = renderer.lock() {
                    unsafe {
                        renderer.paint(info, painter, matrix, &batches);
                    }
                }
            })),
        });
    }

    fn submit_shapes(
        &self,
        ctx: &egui::Context,
        layer: egui::LayerId,
        shapes: Vec<egui::epaint::ClippedShape>,
        pixels_per_point: f32,
    ) {
        let Some(renderer) = self.renderer.clone() else {
            return;
        };
        let surface = self.clone();
        let context = ctx.clone();
        #[cfg(target_arch = "wasm32")]
        let renderer = send_wrapper::SendWrapper::new(renderer);
        #[cfg(target_arch = "wasm32")]
        let surface = send_wrapper::SendWrapper::new(surface);
        egui::Painter::new(ctx.clone(), layer, self.viewport).add(egui::PaintCallback {
            rect: self.viewport,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                // All parent glyph layout and texture uploads have completed.
                // Tessellation now sees the final atlas dimensions for this pass.
                let primitives = context.tessellate(shapes.clone(), pixels_per_point);
                let batches = surface.geometry(&primitives);
                if let Ok(mut renderer) = renderer.lock() {
                    unsafe {
                        renderer.paint(info, painter, surface.local_to_clip, &batches);
                    }
                }
            })),
        });
    }

    /// Collect ordinary egui shapes in display-local coordinates. The guard
    /// drains them into the same real 3D display pipeline as interactive controls.
    pub(super) fn begin_paint(&self, ui: &egui::Ui) -> SurfacePaint {
        let layer = egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new(("cockpit_surface_canvas", self.index)),
        );
        SurfacePaint {
            surface: self.clone(),
            output_layer: ui.layer_id(),
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
    output_layer: egui::LayerId,
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
        self.surface.submit_shapes(
            context,
            self.output_layer,
            shapes,
            context.pixels_per_point(),
        );
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
            } else if drone
                .is_some_and(|drone| matches!(drone.mode, arkline_protocol::DroneMode::Returning))
            {
                "SIGNAL LOST / RETURNING"
            } else {
                "SIGNAL LOST"
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
    paused: bool,
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
        let color = if connected && !paused && now.saturating_sub(snapshot.server_time_ms) <= 1500 {
            super::super::TEAL
        } else {
            super::super::AMBER
        };
        let authority = map_authority(
            connected,
            paused,
            own.flight.is_some() || own.location == Location::Docked,
            now.saturating_sub(snapshot.server_time_ms) <= 1500,
        );
        let selected_readout = contacts
            .iter()
            .find(|(object, _, _)| object == selected)
            .map(|(_, position, name)| {
                (name.as_str(), super::super::distance_m(*position, origin))
            });
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
        paint_map_readouts(&painter, rect, authority, selected_readout, *range, color);
        // Reserve independent text bands; contact geometry stays in the map field.
        let map = egui::Rect::from_min_max(
            rect.min + egui::vec2(9.0, 29.0),
            rect.max - egui::vec2(9.0, 20.0),
        );
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
        for (object, position, _) in &contacts {
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
    }
    picked
}

fn map_authority(connected: bool, paused: bool, available: bool, fresh: bool) -> &'static str {
    if !connected {
        "NAV LOCAL / DISCONNECTED"
    } else if paused {
        "NAV LOCAL / PAUSED"
    } else if !available {
        "NAV LOCAL / DATA UNAVAILABLE"
    } else if !fresh {
        "NAV LOCAL / STALE"
    } else {
        "NAV LOCAL / LIVE"
    }
}

/// Bounded text bands over actual selected-contact data; no dispatch or scene work.
fn paint_map_readouts(
    painter: &egui::Painter,
    rect: egui::Rect,
    authority: &str,
    selected: Option<(&str, f64)>,
    range: f64,
    color: egui::Color32,
) {
    let line = |text: String, y: f32, size| {
        let mut job = egui::text::LayoutJob::simple(
            text,
            egui::FontId::monospace(size),
            color,
            rect.width() - 8.0,
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        painter.galley(
            rect.min + egui::vec2(4.0, y),
            painter.layout_job(job),
            color,
        );
    };
    line(authority.into(), 2.0, 10.0);
    let selection = selected.map_or_else(
        || "SELECTED CONTACT UNAVAILABLE".into(),
        |(name, distance)| {
            if distance.is_finite() {
                // Keep actual range first, so a long contact name never hides it.
                format!("{} · {name}", super::super::format_distance(distance))
            } else {
                format!("RANGE UNAVAILABLE · {name}")
            }
        },
    );
    line(selection, 16.0, 10.0);
    line(
        format!("RANGE {} / SCROLL", super::super::format_distance(range)),
        rect.height() - 13.0,
        9.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn map_paused_connected_paints_held_state_without_claiming_disconnection() {
        assert_eq!(map_authority(true, true, true, true), "NAV LOCAL / PAUSED");
        assert_eq!(
            map_authority(false, true, true, true),
            "NAV LOCAL / DISCONNECTED"
        );
        assert_eq!(
            map_authority(true, true, false, false),
            "NAV LOCAL / PAUSED"
        );
        assert_eq!(
            map_authority(true, false, false, false),
            "NAV LOCAL / DATA UNAVAILABLE"
        );
        assert_eq!(map_authority(true, false, true, false), "NAV LOCAL / STALE");
        assert_eq!(map_authority(true, false, true, true), "NAV LOCAL / LIVE");
        let ctx = egui::Context::default();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                paint_map_readouts(
                    ui.painter(),
                    egui::Rect::from_min_size(egui::pos2(8.0, 8.0), egui::vec2(256.0, 128.0)),
                    map_authority(true, true, true, true),
                    Some(("Station", 5000.0)),
                    10_000.0,
                    super::super::super::AMBER,
                );
            });
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(labels.contains(&"NAV LOCAL / PAUSED"));
        assert!(!labels
            .iter()
            .any(|label| label.contains("DISCONNECTED") || label.contains(" / LIVE")));
    }

    #[test]
    fn map_readouts_bound_actual_selection_and_range_at_both_canvas_sizes() {
        for size in [egui::vec2(256.0, 128.0), egui::vec2(300.0, 180.0)] {
            for selected in [
                None,
                Some((
                    "Asteroid Alpha with a deliberately long authoritative name",
                    5000.0,
                )),
            ] {
                let rect = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), size).shrink(3.0);
                let ctx = egui::Context::default();
                let output = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        paint_map_readouts(
                            &ui.painter().with_clip_rect(rect),
                            rect,
                            "NAV LOCAL / STALE",
                            selected,
                            10_000.0,
                            super::super::super::AMBER,
                        );
                    });
                });
                let labels: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            assert!(
                                rect.contains_rect(text.visual_bounding_rect()),
                                "{:?}",
                                text.galley.job.text
                            );
                            Some(text.galley.job.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(
                    labels.len(),
                    3,
                    "one authority, one selection, one calibrated range; no overlapping ring labels"
                );
                assert!(labels.contains(&"NAV LOCAL / STALE"));
                assert!(labels
                    .iter()
                    .any(|label| label.starts_with("RANGE ") && label.ends_with(" / SCROLL")));
                if let Some((name, distance)) = selected {
                    assert!(labels.contains(
                        &format!(
                            "{} · {name}",
                            super::super::super::format_distance(distance)
                        )
                        .as_str()
                    ));
                } else {
                    assert!(labels.contains(&"SELECTED CONTACT UNAVAILABLE"));
                }
            }
        }
    }

    #[test]
    fn camera_signal_labels_follow_actual_mode_and_freshness() {
        let ctx = egui::Context::default();
        let mut drone = arkline_protocol::DroneView {
            id: 1,
            class: arkline_protocol::DroneClass::Recon,
            hit_points: 100,
            signal_strength: 0.0,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [0.0; 3],
            velocity: [0.0; 3],
            heading: [1.0, 0.0, 0.0],
            mode: arkline_protocol::DroneMode::Hold,
            updated_at_ms: 1000,
        };
        let render = |drone: Option<&arkline_protocol::DroneView>, fresh| {
            let output = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    paint_camera_signal(
                        ui.painter(),
                        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(300.0, 180.0)),
                        drone,
                        1000,
                        fresh,
                    );
                });
            });
            output
                .shapes
                .iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = &shape.shape {
                        Some(text.galley.job.text.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(render(Some(&drone), true), ["SIGNAL LOST"]);
        drone.mode = arkline_protocol::DroneMode::Returning;
        assert_eq!(render(Some(&drone), true), ["SIGNAL LOST / RETURNING"]);
        assert_eq!(render(Some(&drone), false), ["TELEMETRY STALE"]);
        assert_eq!(render(None, true), ["NO CAMERA SELECTED"]);
        drone.signal_strength = 1.0;
        assert!(render(Some(&drone), true).is_empty());
    }

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
    // A worker's view cannot inherit another owner's installed room layout.
    // Its absent payload resolves the immutable default source recipe instead.
    if worker.name != main.own {
        feed.pioneer_parts = None;
    }
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
    fn cockpit_surface_feed_geometry_matches_exact_projected_glass_and_clamps_tiles() {
        let surface = CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, Quat::from_rotation_y(-0.2)),
            ShipClass::Pioneer,
            1,
            NAV_CANVAS_SIZE,
        )
        .unwrap();
        let tile = egui::Rect::from_min_max(egui::pos2(-20.0, 30.0), egui::pos2(180.0, 110.0));
        let clipped = tile.intersect(surface.canvas);
        let (mvp, top_left, bottom_right) = surface.feed_geometry(tile).unwrap();
        assert_eq!(mvp, surface.local_to_clip);
        assert!(
            (mvp * top_left.extend(1.0)
                - surface
                    .vertex(clipped.left_top(), egui::Pos2::ZERO, egui::Color32::WHITE)
                    .clip)
                .length()
                < 1e-6
        );
        assert!(
            (mvp * bottom_right.extend(1.0)
                - surface
                    .vertex(
                        clipped.right_bottom(),
                        egui::Pos2::ZERO,
                        egui::Color32::WHITE
                    )
                    .clip)
                .length()
                < 1e-6
        );
        assert_eq!(top_left.x, bottom_right.x);
        assert!(top_left.y > bottom_right.y && top_left.z < bottom_right.z);
        assert!(surface
            .feed_geometry(egui::Rect::from_min_size(
                egui::pos2(400.0, 200.0),
                egui::vec2(20.0, 20.0)
            ))
            .is_none());
        assert!(surface
            .feed_geometry(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::ZERO
            ))
            .is_none());
        assert!(surface
            .feed_geometry(egui::Rect::from_min_size(
                egui::pos2(f32::NAN, 0.0),
                egui::vec2(20.0, 20.0)
            ))
            .is_none());
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
    fn cockpit_surface_local_clip_retains_3d_vertices_for_gpu_frustum_and_depth() {
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
        let vertices = surface.mesh_geometry(&mesh, surface.canvas);
        assert!(!vertices.is_empty());
        assert_eq!(vertices.len() % 3, 0);
        assert!(vertices.iter().all(|v| {
            (v.position.x - surface.plane_x).abs() < 1e-6
                && (v.position.y - surface.center.y).abs() <= surface.half.y + 1e-6
                && (v.position.z - surface.center.z).abs() <= surface.half.z + 1e-6
        }));
        // The CPU clips only the canonical bezel. Geometry outside the visible
        // frustum remains homogeneous input to the GPU rather than flattened UI.
        assert!(vertices.iter().any(|v| {
            let clip = surface.local_to_clip * v.position.extend(1.0);
            clip.x.abs() > clip.w || clip.y.abs() > clip.w
        }));
        let depths: Vec<_> = vertices
            .iter()
            .map(|v| (surface.local_to_clip * v.position.extend(1.0)).w)
            .collect();
        assert!(depths.iter().any(|w| (w - depths[0]).abs() > 0.001));
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
    fn cockpit_surface_text_and_shapes_remain_canonical_3d_without_2d_fallback() {
        let surface = CockpitSurface::new(
            viewport(),
            matrix(Vec3::ZERO, Quat::from_rotation_y(-0.2)),
            ShipClass::Pioneer,
            5,
            NAV_CANVAS_SIZE,
        )
        .unwrap();
        let context = egui::Context::default();
        let mut canonical_shapes = Vec::new();
        let output = context.run(
            egui::RawInput {
                screen_rect: Some(viewport()),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let paint = surface.begin_paint(ui);
                    {
                        let painter = paint.painter();
                        painter.rect_filled(surface.canvas, 0.0, egui::Color32::BLACK);
                        painter.text(
                            egui::pos2(-20.0, 40.0),
                            egui::Align2::LEFT_TOP,
                            "RADAR TEXT CROSSES BEZEL",
                            egui::FontId::monospace(12.0),
                            egui::Color32::WHITE,
                        );
                    }
                    canonical_shapes = context.graphics_mut(|graphics| {
                        graphics.entry(paint.layer).all_entries().cloned().collect()
                    });
                });
            },
        );
        assert!(!output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Text(_))));
        assert!(!output
            .shapes
            .iter()
            .any(|s| matches!(&s.shape, egui::Shape::Mesh(_))));
        let primitives = context.tessellate(canonical_shapes, output.pixels_per_point);
        let batches = surface.geometry(&primitives);
        assert!(!batches.is_empty());
        assert!(batches.iter().flat_map(|batch| &batch.vertices).all(|v| {
            (v.position.x - surface.plane_x).abs() < 1e-6
                && (v.position.y - surface.center.y).abs() <= surface.half.y + 1e-6
                && (v.position.z - surface.center.z).abs() <= surface.half.z + 1e-6
        }));
    }
}

#[cfg(test)]
mod remote_tests {
    use super::*;
    fn snapshot() -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "observer_schema_version":2,"server_time_ms":1000,"pilots":[
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
            pioneer_parts: None,
            exterior: None,
            lighting_quality: Default::default(),
            cockpit_vp: Mat4::IDENTITY,
            cockpit_occlusion: None,
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

#[cfg(test)]
mod registered_display_tests {
    use super::super::cockpit_occlusion::{DepthDraw, DepthMeshCatalog};
    use super::*;
    #[test]
    fn registered_display_input_and_emitted_plane_share_real_scene_blocking() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 800.));
        let vp = Mat4::perspective_rh_gl(1.2, 1.25, 0.05, 100.)
            * Mat4::look_at_rh(Vec3::new(-2., 1., 0.), Vec3::new(0., 1., 0.), Vec3::Y);
        let catalog = Arc::new(DepthMeshCatalog::new(vec![Arc::from(super::super::cube())]));
        let clear = Arc::new(
            CockpitOcclusion::new(ShipClass::Pioneer, catalog.clone(), vec![], vec![]).unwrap(),
        );
        let surface = CockpitSurface::registered(
            viewport,
            vp,
            Mat4::IDENTITY,
            Vec3::new(0., 1., 0.),
            Vec3::new(0.02, 0.25, 0.4),
            3,
            egui::vec2(300., 180.),
        )
        .unwrap()
        .with_registered_occlusion(clear, vp, Mat4::IDENTITY);
        let point = surface.canvas_rect().center();
        let screen = surface.project(point).unwrap();
        assert!(surface.unproject(screen).unwrap().distance(point) < 0.001);
        let emitted = surface.local_point(point);
        assert!((emitted.x + 0.032).abs() < 0.000001);
        let wall = Mat4::from_scale_rotation_translation(
            Vec3::new(0.1, 1., 1.),
            Quat::IDENTITY,
            Vec3::new(-1., 1., 0.),
        );
        let draw = DepthDraw::whole(&catalog, 0, wall).unwrap();
        let blocked = Arc::new(
            CockpitOcclusion::new(ShipClass::Pioneer, catalog, vec![draw], vec![]).unwrap(),
        );
        let blocked = surface.with_registered_occlusion(blocked, vp, Mat4::IDENTITY);
        assert!(blocked.unproject(screen).is_none());
        assert!(
            blocked.project(point).is_some(),
            "projection alone must never grant input through geometry"
        );
    }
}
