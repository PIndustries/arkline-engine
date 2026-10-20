//! Desktop and WebGL2 GPU flight view. Presentation only: never issues simulation commands.
use crate::material_program::{
    compile_material_program, MaterialShaderSource, SpatialMaterialUniforms,
};
use crate::spatial_material::{
    MaterialProgramDiagnostic, MaterialProgramErrorPhase, MaterialProgramState,
    MaterialProgramStatus, MaterialRevision, PreparedMaterialUpdate,
};
#[cfg(all(test, target_os = "macos"))]
#[path = "spatial_material_gpu_tests.rs"]
mod spatial_material_gpu_tests;
use arkline_protocol::{
    gates, systems, Asteroid, Destination, Location, NavigationMode, NavigationTarget, PilotView,
    ShipClass, Snapshot, Stargate, WorldLayoutDefinition, BELT_POSITION, HOME_SYSTEM_ID,
    JUMP_DURATION_MS, SPACE_BOUND, STATION_CENTER_POSITION, STATION_DIAMETER_M, STATION_HEIGHT_M,
    STATION_POSITION,
};
#[cfg(test)]
use arkline_protocol::{WARP_DURATION_MS, WARP_WARMUP_MS};
use eframe::{egui, egui_glow, glow};
use glam::{Mat4, Quat, Vec3};
use glow::HasContext;
use std::sync::{Arc, Mutex};

/// Inspect the current draw target without rebinding it. Desktop core OpenGL
/// removed DEPTH_BITS; querying it silently suppresses physical displays after
/// raising INVALID_ENUM. WebGL/GLES retain that default-framebuffer query.
unsafe fn draw_framebuffer_depth_bits(gl: &glow::Context) -> i32 {
    if gl.version().is_embedded {
        return gl.get_parameter_i32(glow::DEPTH_BITS);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let attachment = if gl
            .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING)
            .is_some()
        {
            glow::DEPTH_ATTACHMENT
        } else {
            glow::DEPTH
        };
        if gl.get_framebuffer_attachment_parameter_i32(
            glow::DRAW_FRAMEBUFFER,
            attachment,
            glow::FRAMEBUFFER_ATTACHMENT_OBJECT_TYPE,
        ) as u32
            == glow::NONE
        {
            return 0;
        }
        gl.get_framebuffer_attachment_parameter_i32(
            glow::DRAW_FRAMEBUFFER,
            attachment,
            glow::FRAMEBUFFER_ATTACHMENT_DEPTH_SIZE,
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        0
    }
}

#[path = "imported_asset.rs"]
pub mod imported_asset;

#[cfg(not(target_arch = "wasm32"))]
#[path = "drone_gallery.rs"]
pub mod drone_gallery;
#[path = "drone_models.rs"]
mod drone_models;
#[cfg(not(target_arch = "wasm32"))]
#[path = "model_catalog.rs"]
pub mod model_catalog;
#[cfg(not(target_arch = "wasm32"))]
#[path = "model_preview.rs"]
pub mod model_preview;
#[path = "pioneer_docked_source.rs"]
mod pioneer_docked_source;
#[path = "ship_asset.rs"]
mod ship_asset;
#[path = "view_visibility.rs"]
mod view_visibility;
use view_visibility::part_in_view;
#[cfg(not(target_arch = "wasm32"))]
pub use view_visibility::ViewRenderMetrics;
pub use view_visibility::ViewVisibility;
#[cfg(not(target_arch = "wasm32"))]
use view_visibility::{ViewProfileStart, ViewProfiler};

#[cfg(all(test, target_os = "linux"))]
#[path = "exterior_gpu_tests.rs"]
mod exterior_gpu_tests;
#[path = "exterior_visuals.rs"]
mod exterior_visuals;
#[path = "local_lighting.rs"]
mod local_lighting;

#[cfg(not(target_arch = "wasm32"))]
#[path = "asset_gallery.rs"]
pub mod asset_gallery;
#[path = "hangar.rs"]
pub mod hangar;

#[path = "cockpit_maps.rs"]
mod cockpit_maps;
pub(crate) use cockpit_maps::CockpitSurface;
use cockpit_maps::*;
#[path = "cockpit_displays.rs"]
mod cockpit_displays;
use cockpit_displays::CockpitDisplayRenderer;
#[path = "cockpit_occlusion.rs"]
mod cockpit_occlusion;
use cockpit_occlusion::{CockpitDepthProgram, CockpitOcclusion, DepthMeshCatalog};
#[path = "cockpit_computer.rs"]
pub(crate) mod cockpit_computer;
#[cfg(all(test, target_os = "macos"))]
#[path = "cockpit_enclosure_gpu_tests.rs"]
mod cockpit_enclosure_gpu_tests;
#[path = "cockpit_fit.rs"]
mod cockpit_fit;
#[path = "cockpit_occlusion_geometry.rs"]
mod cockpit_occlusion_geometry;
#[path = "cockpit_radar.rs"]
mod cockpit_radar;
#[path = "sun_shadows.rs"]
mod sun_shadows;
#[path = "world_visuals.rs"]
mod world_visuals;
use world_visuals::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DroneCameraMode {
    Pov,
    #[default]
    Raised,
    TopDown,
    Orbit,
}
impl DroneCameraMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pov => "POV",
            Self::Raised => "Raised",
            Self::TopDown => "Top-down",
            Self::Orbit => "Orbit",
        }
    }
}
pub enum ViewAction {
    Select(super::SelectedObject),
    FlyTo([f64; 3]),
    Command(arkline_protocol::Request),
}

#[cfg(test)]
pub fn route_progress(location: &Location, now: u64) -> f32 {
    match *location {
        Location::Docked | Location::Jumping { .. } | Location::Station => 0.0,
        Location::Belt => 1.0,
        Location::InTransit {
            destination,
            arrives_at_ms,
        } => {
            let elapsed =
                WARP_DURATION_MS - arrives_at_ms.saturating_sub(now).min(WARP_DURATION_MS);
            let travel = (elapsed.saturating_sub(WARP_WARMUP_MS) as f32
                / (WARP_DURATION_MS - WARP_WARMUP_MS) as f32)
                .clamp(0.0, 1.0);
            let fraction = travel * travel * (3.0 - 2.0 * travel);
            if destination == Destination::Belt {
                fraction
            } else {
                1.0 - fraction
            }
        }
    }
}
/// Presentation phases share authoritative transit timestamps with both views.
/// Progress is local to the named phase; no phase sends gameplay commands.
#[cfg(test)]
pub fn warp_phase(location: &Location, now: u64) -> Option<(&'static str, f32)> {
    let Location::InTransit { arrives_at_ms, .. } = *location else {
        return None;
    };
    let remaining = arrives_at_ms.saturating_sub(now).min(WARP_DURATION_MS);
    if !in_warp(location, now) {
        return None;
    }
    let elapsed = WARP_DURATION_MS - remaining;
    if elapsed < WARP_WARMUP_MS {
        Some(("WARP WARMUP", elapsed as f32 / WARP_WARMUP_MS as f32))
    } else if elapsed < WARP_WARMUP_MS + 700 {
        Some(("WARP LAUNCH", (elapsed - WARP_WARMUP_MS) as f32 / 700.0))
    } else if remaining <= 700 {
        Some(("WARP ARRIVAL", 1.0 - remaining as f32 / 700.0))
    } else {
        Some((
            "WARP CRUISE",
            (elapsed - WARP_WARMUP_MS - 700) as f32
                / (WARP_DURATION_MS - WARP_WARMUP_MS - 1400) as f32,
        ))
    }
}
pub fn heading(location: &Location) -> f32 {
    if matches!(
        location,
        Location::InTransit {
            destination: Destination::Station,
            ..
        } | Location::Belt
    ) {
        -1.0
    } else {
        1.0
    }
}
pub fn lane(name: &str) -> f32 {
    let hash = name.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    (hash % 11) as f32 - 5.0
}
pub fn display_position(pilot: &PilotView, now: u64) -> [f64; 3] {
    super::motion::position(pilot, now)
}
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Space {
    origin: [f64; 3],
}
fn editor_camera_scene_space(
    active: bool,
    frozen: Option<Space>,
    live: Space,
) -> (Space, Option<Space>) {
    if active {
        let fixed = frozen.unwrap_or(live);
        (fixed, Some(fixed))
    } else {
        (live, None)
    }
}
impl Space {
    fn scene(self, meters: [f64; 3]) -> Vec3 {
        Vec3::from_array(std::array::from_fn(|i| {
            ((meters[i] - self.origin[i]) / 100.0) as f32
        }))
    }
    fn world(self, point: Vec3) -> [f64; 3] {
        std::array::from_fn(|i| self.origin[i] + point[i] as f64 * 100.0)
    }
    fn position(self, pilot: &PilotView, now: u64) -> Vec3 {
        self.scene(display_position(pilot, now))
    }
}
fn orientation(forward: [f64; 3]) -> Option<Quat> {
    let direction = Vec3::from_array(forward.map(|value| value as f32));
    (direction.is_finite() && direction.length_squared() > 1e-6)
        .then(|| Quat::from_rotation_arc(Vec3::X, direction.normalize()))
}

fn ship_orientation(pilot: &PilotView) -> Quat {
    pilot
        .flight
        .as_ref()
        .and_then(|flight| {
            let quaternion = Quat::from_array(flight.orientation.map(|v| v as f32));
            if quaternion.is_finite() && quaternion.length_squared() > 1e-8 {
                Some(quaternion.normalize())
            } else {
                orientation(flight.heading).or_else(|| orientation(flight.velocity))
            }
        })
        .unwrap_or_else(|| {
            Quat::from_rotation_y(if heading(&pilot.location) < 0.0 {
                std::f32::consts::PI
            } else {
                0.0
            })
        })
}

fn ship_orientation_at(pilot: &PilotView, now: u64) -> Quat {
    if matches!(pilot.location, Location::InTransit { .. }) {
        if let Some(path) = pilot.flight.as_ref().and_then(|f| f.warp.as_ref()) {
            if let Some(rotation) = orientation(arkline_protocol::sample_warp_heading(path, now)) {
                let current = ship_orientation(pilot);
                return Quat::from_rotation_arc(current * Vec3::X, rotation * Vec3::X) * current;
            }
        }
    }
    ship_orientation(pilot)
}

fn navigation_destination(
    pilot: &PilotView,
    snapshot: &Snapshot,
    now: u64,
    space: Space,
) -> Option<(&'static str, Vec3)> {
    if matches!(
        pilot.location,
        Location::Docked | Location::Jumping { .. } | Location::InTransit { .. }
    ) {
        return None;
    }
    let (label, target) = match &pilot.flight.as_ref()?.mode {
        NavigationMode::Stop | NavigationMode::Manual => return None,
        NavigationMode::FlyTo { position } => return Some(("FLY TO", space.scene(*position))),
        NavigationMode::Approach { target } => ("APPROACH", target),
        NavigationMode::Orbit { target, .. } => ("ORBIT", target),
        NavigationMode::KeepRange { target, .. } => ("KEEP RANGE", target),
    };
    let target = match target {
        NavigationTarget::Station => space.scene(STATION_POSITION),
        NavigationTarget::Belt => space.scene(BELT_POSITION),
        NavigationTarget::Asteroid { id } => space.scene(
            snapshot
                .asteroids
                .iter()
                .find(|a| a.id == *id && a.system_id == pilot.system_id)?
                .position,
        ),
        NavigationTarget::Gate { id } => space.scene(
            gates()
                .iter()
                .find(|g| g.id == *id && g.system_id == pilot.system_id)?
                .position,
        ),
        NavigationTarget::Pilot { pilot: name } => space.position(
            snapshot
                .pilots
                .iter()
                .find(|p| &p.name == name && p.system_id == pilot.system_id)?,
            now,
        ),
    };
    Some((label, target))
}

#[derive(Clone)]
struct CockpitDisplayFrame {
    scene: Arc<CockpitOcclusion>,
    vp: Mat4,
    mount: Mat4,
    viewport: egui::Rect,
    class: ShipClass,
}

pub struct FlightView {
    pub lighting_quality: super::lighting_settings::LightingQuality,
    renderer: Result<Arc<Mutex<Renderer>>, String>,
    depth_geometry: Result<Arc<FlightGeometry>, String>,
    headless_turrets: std::collections::HashMap<(String, u8), TurretState>,
    display_renderer: Result<Arc<Mutex<CockpitDisplayRenderer>>, String>,
    computer_surface: Option<CockpitSurface>,
    cockpit_display_frame: Option<CockpitDisplayFrame>,
    cockpit_layer: Option<egui::LayerId>,
    instrument_surfaces: Vec<CockpitSurface>,
    computer_keyboard_owned: bool,
    head_look_input_allowed: bool,
    head_look: [f32; 2],
    yaw: f32,
    pitch: f32,
    distance: f32,
    target: Vec3,
    follow: bool,
    focus_selection: bool,
    context_target: Option<[f64; 3]>,
    context_object: Option<super::SelectedObject>,
    input_surface: Option<(egui::Rect, egui::LayerId)>,
    camera_ship_class: Option<ShipClass>,
    tactical: bool,
    other_camera: (f32, f32, f32),
    tactical_overlay: bool,
    drone_camera: bool,
    selected_drone: Option<u64>,
    drone_mode: DroneCameraMode,
    drone_feeds: Vec<u64>,
    drone_headings: std::collections::HashMap<u64, Vec3>,
    remote_ship: Option<String>,
    console_rect: Option<egui::Rect>,
    map_range: f64,
    space: Space,
    /// Editor-only camera, expressed in the rendered scene's local coordinates.
    free_camera: Option<(Vec3, f32, f32)>,
    editor_free_camera_active: bool,
    editor_free_camera_space: Option<Space>,
    last_camera_pose: Option<(Vec3, f32, f32)>,
}
impl FlightView {
    // egui callbacks share ownership on one browser thread; SendWrapper guards access.
    #[cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let renderer = gl
            .ok_or_else(|| {
                "OpenGL is unavailable. A graphics context is required for the cockpit.".into()
            })
            .and_then(|gl| unsafe { Renderer::new(gl.clone()) })
            .map(|r| Arc::new(Mutex::new(r)));
        let display_renderer = gl
            .ok_or_else(|| "OpenGL is unavailable for cockpit displays".into())
            .and_then(|gl| unsafe { CockpitDisplayRenderer::new(gl.clone()) })
            .map(|renderer| Arc::new(Mutex::new(renderer)));
        Self {
            lighting_quality: Default::default(),
            renderer,
            depth_geometry: flight_geometry(),
            headless_turrets: Default::default(),
            display_renderer,
            computer_surface: None,
            cockpit_display_frame: None,
            cockpit_layer: None,
            instrument_surfaces: Vec::new(),
            computer_keyboard_owned: false,
            head_look_input_allowed: true,
            head_look: [0.0; 2],
            yaw: 0.72,
            pitch: 0.35,
            distance: 27.0,
            target: Vec3::new(-45.0, 0.0, 20.0),
            follow: true,
            focus_selection: false,
            context_target: None,
            context_object: None,
            input_surface: None,
            camera_ship_class: None,
            tactical: false,
            other_camera: (0.72, 0.65, 350.0),
            tactical_overlay: true,
            drone_camera: true,
            selected_drone: None,
            drone_mode: DroneCameraMode::Raised,
            drone_feeds: Vec::new(),
            drone_headings: std::collections::HashMap::new(),
            remote_ship: None,
            console_rect: None,
            map_range: 1_000_000.0,
            space: Space::default(),
            free_camera: None,
            editor_free_camera_active: false,
            editor_free_camera_space: None,
            last_camera_pose: None,
        }
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn set_editor_free_camera(&mut self, active: bool, pose: Option<(Vec3, f32, f32)>) {
        let reseed = active
            && (!self.editor_free_camera_active || (pose.is_none() && self.free_camera.is_some()));
        if !active {
            self.editor_free_camera_space = None;
        } else if reseed {
            // An existing camera pose belongs to the last rendered origin,
            // even when the pilot travels before the next rendered frame.
            // A cleared pose lets a new scene establish its origin instead.
            self.editor_free_camera_space = pose.map(|_| self.space);
        }
        if active {
            self.clear_cockpit_presentation();
        }
        self.editor_free_camera_active = active;
        self.free_camera = pose;
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn editor_camera_pose(&self) -> Option<(Vec3, f32, f32)> {
        self.last_camera_pose
    }
    pub fn set_remote_ship(&mut self, pilot: Option<String>) {
        self.remote_ship = pilot;
    }
    pub fn set_drone_camera(&mut self, enabled: bool) {
        self.drone_camera = enabled;
    }
    pub fn set_drone_camera_mode(&mut self, mode: DroneCameraMode) {
        self.drone_mode = mode;
    }
    pub fn set_drone_feeds(&mut self, ids: &[u64]) {
        self.drone_feeds.clear();
        for id in ids.iter().take(4) {
            if !self.drone_feeds.contains(id) {
                self.drone_feeds.push(*id);
            }
        }
    }
    pub fn set_drone_view(&mut self, selected: Option<u64>, orbit: bool) {
        self.selected_drone = selected;
        self.drone_mode = if orbit {
            DroneCameraMode::Orbit
        } else {
            DroneCameraMode::Raised
        };
    }
    #[cfg(test)]
    pub fn cockpit_console_rect(&self) -> Option<egui::Rect> {
        self.console_rect
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_material_preview(&mut self, preview: Option<crate::EmbeddedMaterialPreview>) {
        if let Ok(renderer) = &self.renderer {
            if let Ok(mut renderer) = renderer.lock() {
                renderer.set_material_preview(preview);
            }
        }
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // Native editor authoring entry.
    pub fn set_spatial_material(&mut self, material: Option<Arc<MaterialRevision>>) {
        if let Ok(renderer) = &self.renderer {
            if let Ok(mut renderer) = renderer.lock() {
                let _ = renderer.spatial_material.queue(material);
            }
        }
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn spatial_material_status(&self) -> MaterialProgramStatus {
        self.renderer
            .as_ref()
            .ok()
            .and_then(|r| r.lock().ok().map(|r| r.spatial_material.status().clone()))
            .unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_debug_wireframe(&mut self, enabled: bool) {
        if let Ok(renderer) = &self.renderer {
            if let Ok(mut renderer) = renderer.lock() {
                renderer.debug_wireframe = enabled;
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn debug_render_metrics(&self) -> DebugRenderMetrics {
        self.renderer
            .as_ref()
            .ok()
            .and_then(|r| r.lock().ok().map(|r| r.debug_metrics.get()))
            .unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn debug_view_render_metrics(&self) -> Vec<ViewRenderMetrics> {
        self.renderer
            .as_ref()
            .ok()
            .and_then(|r| r.lock().ok().map(|r| r.view_profiler.borrow().samples()))
            .unwrap_or_default()
    }

    pub(crate) fn cockpit_presentation_active(&self, mode: super::ViewMode) -> bool {
        matches!(mode, super::ViewMode::Cockpit) && !self.editor_free_camera_active
    }
    pub(crate) fn clear_cockpit_presentation(&mut self) {
        self.input_surface = None;
        self.computer_surface = None;
        self.cockpit_display_frame = None;
        self.cockpit_layer = None;
        self.instrument_surfaces.clear();
        self.console_rect = None;
    }
    fn physical_surface(&self, index: usize, canvas: egui::Vec2) -> Option<CockpitSurface> {
        let frame = self.cockpit_display_frame.as_ref()?;
        let mut surface = CockpitSurface::new(
            frame.viewport,
            frame.vp * frame.mount,
            frame.class,
            index,
            canvas,
        )?
        .with_occlusion(frame.scene.clone(), frame.vp, frame.mount);
        if let Ok(renderer) = &self.display_renderer {
            surface = surface.with_renderer(renderer.clone());
        }
        Some(surface)
    }
    pub(crate) fn computer_surface(&self) -> Option<CockpitSurface> {
        self.computer_surface.clone()
    }
    /// Editor viewport ownership is separate from paused flight-command permission.
    #[cfg(any(not(target_arch = "wasm32"), test))]
    pub(crate) fn set_head_look_input_allowed(&mut self, allowed: bool) {
        self.head_look_input_allowed = allowed;
    }
    #[cfg(test)]
    pub(crate) fn head_look_angles(&self) -> [f32; 2] {
        self.head_look
    }
    pub(crate) fn set_computer_keyboard_owned(&mut self, owned: bool) {
        self.computer_keyboard_owned = owned;
    }
    pub(crate) fn computer_hit_tester(
        &self,
        ctx: &egui::Context,
    ) -> impl Fn(egui::Pos2) -> Option<egui::Pos2> + 'static {
        let surface = self.computer_surface.clone();
        let layer = self.cockpit_layer;
        let ctx = ctx.clone();
        move |point| {
            if layer.is_none() || ctx.layer_id_at(point) != layer {
                return None;
            }
            surface.as_ref()?.unproject(point)
        }
    }
    #[cfg(test)]
    pub(crate) fn computer_hit(
        &self,
        ctx: &egui::Context,
        point: egui::Pos2,
    ) -> Option<egui::Pos2> {
        self.computer_hit_tester(ctx)(point)
    }
    pub(crate) fn submit_computer(
        &self,
        ctx: &egui::Context,
        primitives: Vec<egui::ClippedPrimitive>,
    ) {
        if let (Some(surface), Some(layer)) = (&self.computer_surface, self.cockpit_layer) {
            surface.submit(ctx, layer, primitives);
        }
    }
    pub fn input_surface(&self) -> Option<(egui::Rect, egui::LayerId)> {
        self.input_surface
    }
    pub fn destroy(&mut self) {
        self.clear_cockpit_presentation();
        if let Ok(renderer) = &self.display_renderer {
            if let Ok(mut renderer) = renderer.lock() {
                unsafe {
                    renderer.destroy();
                }
            }
        }
        if let Ok(renderer) = &self.renderer {
            if let Ok(mut renderer) = renderer.lock() {
                unsafe {
                    renderer.destroy();
                }
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        pilot_name: &str,
        now: u64,
        connected: bool,
        effects: &super::effects::Effects,
        selected: &super::SelectedObject,
        mode: super::ViewMode,
        layout: &mut super::layout::Layout,
    ) -> Option<ViewAction> {
        self.clear_cockpit_presentation();
        let world_layout = match super::snapshot_world_layout(snapshot) {
            Ok(layout) => layout,
            Err(error) => {
                self.input_surface = None;
                ui.colored_label(egui::Color32::RED, error);
                return None;
            }
        };
        let mut picked = None;
        let own_pilot = snapshot.pilots.iter().find(|p| p.name == pilot_name);
        let camera_length = own_pilot.map_or(72.0, |p| p.ship_class.spec().length_m) as f32 / 100.0;
        let ship_distance = (camera_length * 2.5).max(1.8);
        if own_pilot.map(|p| p.ship_class) != self.camera_ship_class {
            self.camera_ship_class = own_pilot.map(|p| p.ship_class);
            if self.follow && !self.tactical {
                self.distance = ship_distance;
            }
        }
        let live_space = Space {
            origin: own_pilot.map_or(STATION_POSITION, |p| display_position(p, now)),
        };
        let (space, editor_free_camera_space) = editor_camera_scene_space(
            self.editor_free_camera_active,
            self.editor_free_camera_space,
            live_space,
        );
        self.editor_free_camera_space = editor_free_camera_space;
        let warp_tracking =
            own_pilot.is_some_and(|p| matches!(p.location, Location::InTransit { .. }));
        self.target = rebased_camera_target(self.target, self.space, space, warp_tracking);
        self.space = space;
        let cockpit_mode = matches!(mode, super::ViewMode::Cockpit);
        let cockpit = cockpit_mode && !self.editor_free_camera_active;
        let editor_free_camera = self.editor_free_camera_active;
        let tactical = mode == super::ViewMode::Tactical3d;
        if tactical != self.tactical {
            let next = self.other_camera;
            self.other_camera = (self.yaw, self.pitch, self.distance);
            (self.yaw, self.pitch, self.distance) = next;
            self.tactical = tactical;
            self.follow = true;
            self.focus_selection = false;
        }
        if tactical {
            self.follow = true;
            self.focus_selection = false;
        }

        let bounds = ui.max_rect();
        let (camera_position, camera_size) = layout.position(
            "flight-camera",
            bounds,
            [0.0, 1.0],
            egui::vec2(16.0, -180.0),
            egui::vec2(280.0, 95.0),
        );
        let mut camera_content = None;
        let camera_response = egui::Window::new("Flight controls")
            .open(&mut !cockpit)
            .id(egui::Id::new("flight_camera_overlay").with(layout.generation))
            .current_pos(camera_position)
            .default_size(camera_size)
            .resize(|resize| {
                if layout.reflow {
                    resize.min_size(camera_size).max_size(camera_size)
                } else {
                    resize
                }
            })
            .constrain_to(ui.max_rect().shrink(8.0))
            .frame(
                egui::Frame::window(&ui.ctx().style())
                    .fill(egui::Color32::from_rgba_unmultiplied(3, 9, 6, 212))
                    .stroke(egui::Stroke::new(1.0, super::TEAL.gamma_multiply(0.32)))
                    .inner_margin(8.0),
            )
            .movable(!layout.pinned)
            .resizable(!layout.pinned)
            .collapsible(true)
            .show(ui.ctx(), |ui| {
                if editor_free_camera {
                    ui.label("Editor free camera · WASD move · E/Q or Space/C vertical · RMB look");
                    camera_content = Some(ui.max_rect().size());
                    return;
                }
                if cockpit {
                    ui.label("Pilot seat · full six-axis flight");
                    ui.small("W/S: thrust · A/D: strafe · Space/C: vertical");
                    ui.small("Q/E: roll · Arrows: aim · RMB + mouse: aim");
                    ui.small("Hold Shift + arrows: look around · release: recenter");
                    ui.small("Left-click: select · Release controls: neutral");
                    camera_content = Some(ui.max_rect().size());
                    return;
                }
                if tactical {
                    ui.label("3D Tactical · ship-centered range plane");
                    ui.checkbox(&mut self.tactical_overlay, "Range rings and target guide");
                    if ui.button("Reset tactical camera").clicked() {
                        self.yaw = 0.72;
                        self.pitch = 0.65;
                        self.distance = 350.0;
                    }
                    ui.small("Drag: orbit · Scroll: zoom · Distances in meters/km");
                    ui.small("Double-click space: fly · Right-click: actions");
                    camera_content = Some(ui.max_rect().size());
                    return;
                }
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Focus my ship").clicked() {
                        self.follow = true;
                        self.focus_selection = false;
                        self.distance = ship_distance;
                    }
                    if ui.button("Sector overview").clicked() {
                        self.follow = false;
                        self.focus_selection = false;
                        self.distance = 150.0;
                        self.yaw = 0.25;
                        self.pitch = 0.65;
                    }
                    if ui.button("Focus selected").clicked() {
                        self.focus_selection = true;
                        self.follow = false;
                        self.distance = 42.0;
                    }
                    if ui.button("Reset camera").clicked() {
                        self.follow = true;
                        self.focus_selection = false;
                        self.distance = ship_distance;
                        self.yaw = 0.72;
                        self.pitch = 0.35;
                    }
                });
                ui.small("Drag: orbit · Scroll: zoom");
                ui.small("Double-click space: fly · Right-click: actions");
                camera_content = Some(ui.max_rect().size());
            });
        if let Some(response) = camera_response {
            layout.record(
                "flight-camera",
                bounds,
                response.response.rect,
                camera_content,
                ui.input(|i| i.pointer.any_down()),
            );
        }
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(100.0, 150.0)),
            egui::Sense::click_and_drag(),
        );
        // Resolve interaction against this frame's ship-fixed glass below,
        // once both the final camera pose and exact scene geometry are known.
        self.input_surface = Some((rect, ui.layer_id()));
        let mut world_input = !editor_free_camera
            && ui
                .input(|input| input.pointer.hover_pos())
                .is_some_and(|pointer| {
                    rect.contains(pointer) && ui.ctx().layer_id_at(pointer) == Some(ui.layer_id())
                });
        if !cockpit
            && !editor_free_camera
            && world_input
            && response.dragged_by(egui::PointerButton::Primary)
        {
            let delta = ui.input(|i| i.pointer.delta());
            self.yaw -= delta.x * 0.007;
            self.pitch = (self.pitch + delta.y * 0.006).clamp(-1.25, 1.25);
        }
        if !cockpit && !editor_free_camera && world_input && response.hovered() {
            self.distance =
                (self.distance * (-ui.input(|i| i.smooth_scroll_delta.y) * 0.002).exp()).clamp(
                    if tactical {
                        15.0
                    } else {
                        (camera_length * 0.8).max(0.5)
                    },
                    if tactical { 2500.0 } else { 260.0 },
                );
        }
        let system_id = snapshot
            .pilots
            .iter()
            .find(|p| p.name == pilot_name)
            .map_or(HOME_SYSTEM_ID, |p| p.system_id);
        let asteroids: Vec<_> = snapshot
            .asteroids
            .iter()
            .filter(|a| a.system_id == system_id)
            .cloned()
            .collect();
        let local_gates: Vec<_> = gates()
            .into_iter()
            .filter(|g| g.system_id == system_id)
            .collect();
        let selected_world = match selected {
            super::SelectedObject::Asteroid(id) => {
                asteroids.iter().find(|a| a.id == *id).map(|a| a.position)
            }
            super::SelectedObject::Gate(id) => {
                local_gates.iter().find(|g| g.id == *id).map(|g| g.position)
            }
            super::SelectedObject::Planet(id) => world_layout
                .planets
                .iter()
                .find(|p| p.id == *id && p.system_id == system_id)
                .map(|planet| planet.approach_position),
            super::SelectedObject::Prop(id) => snapshot
                .surface_props
                .iter()
                .find(|p| p.id == *id)
                .map(|p| p.position),
            super::SelectedObject::Station => Some(STATION_POSITION),
            super::SelectedObject::Belt => Some(BELT_POSITION),
            super::SelectedObject::Pilot(name) => snapshot
                .pilots
                .iter()
                .find(|p| &p.name == name && p.system_id == system_id)
                .map(|p| display_position(p, now)),
        };
        let selected_position = selected_world.map_or(self.target, |p| space.scene(p));
        let selected_distance = selected_world.zip(own_pilot).map(|(target, pilot)| {
            let own = display_position(pilot, now);
            target
                .iter()
                .zip(own)
                .map(|(a, b)| (a - b) * (a - b))
                .sum::<f64>()
                .sqrt()
        });
        let desired = if self.focus_selection && selected_position.length() < 1000.0 {
            selected_position
        } else if self.follow {
            snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot_name)
                .map(|p| space.position(p, now))
                .unwrap_or(self.target)
        } else {
            Vec3::ZERO
        };
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        self.target = if warp_tracking || tactical {
            Vec3::ZERO
        } else {
            self.target.lerp(desired, 1.0 - (-dt * 8.0).exp())
        };
        let external_eye = self.target
            + Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            ) * self.distance;
        let rotation = own_pilot.map_or(Quat::IDENTITY, |p| ship_orientation_at(p, now));
        let head_look_input = cockpit
            && self.head_look_input_allowed
            && !self.computer_keyboard_owned
            && !ui.ctx().wants_keyboard_input()
            && ui.input(|input| {
                input.focused && !input.pointer.any_down() && !input.pointer.any_pressed()
            })
            && ui
                .input(|input| input.pointer.hover_pos())
                .is_some_and(|point| {
                    rect.contains(point) && ui.ctx().layer_id_at(point) == Some(ui.layer_id())
                });
        let looking = ui.input(|input| input.modifiers.shift);
        let axes = ui.input(|i| {
            [
                (i.key_down(egui::Key::ArrowLeft) as i32 - i.key_down(egui::Key::ArrowRight) as i32)
                    as f32,
                (i.key_down(egui::Key::ArrowUp) as i32 - i.key_down(egui::Key::ArrowDown) as i32)
                    as f32,
            ]
        });
        // Denied ownership freezes the presentation pose; it must not be
        // interpreted as releasing Shift and recentering somebody else's view.
        if head_look_input {
            update_head_look(&mut self.head_look, looking, axes, dt);
        }
        let head_rotation = rotation
            * Quat::from_rotation_y(self.head_look[0])
            * Quat::from_rotation_z(self.head_look[1]);
        let class = own_pilot.map_or(ShipClass::Pioneer, |p| p.ship_class);
        let ship_origin = own_pilot.map_or(Vec3::ZERO, |p| space.position(p, now));
        let cockpit_mount = cockpit_fit::mount(ship_origin, rotation, class);
        let (mut eye, mut view) = if cockpit {
            let eye = ship_origin + rotation * (cockpit_fit::eye_body_meters(class) / 100.0);
            (
                eye,
                Mat4::look_at_rh(eye, eye + rotation * Vec3::X, rotation * Vec3::Y),
            )
        } else {
            (
                external_eye,
                Mat4::look_at_rh(external_eye, self.target, Vec3::Y),
            )
        };
        if cockpit {
            view = Mat4::look_at_rh(eye, eye + head_rotation * Vec3::X, head_rotation * Vec3::Y);
        }
        let player_direction = view
            .inverse()
            .transform_vector3(Vec3::NEG_Z)
            .normalize_or_zero();
        self.last_camera_pose = Some((
            eye,
            player_direction.z.atan2(player_direction.x),
            player_direction.y.clamp(-1.0, 1.0).asin(),
        ));
        if let Some((position, yaw, pitch)) = self.free_camera {
            let direction = hangar::editor_camera_direction(yaw, pitch);
            eye = position;
            view = Mat4::look_at_rh(eye, eye + direction, Vec3::Y);
        }

        let projection = Mat4::perspective_rh_gl(
            if tactical {
                1.22
            } else if cockpit {
                COCKPIT_FOV
            } else {
                0.85
            },
            rect.width() / rect.height().max(1.0),
            if tactical { 1.0 } else { 0.03 },
            if tactical { 10000.0 } else { 1200.0 },
        );
        let vp = projection * view;
        let cockpit_vp = Mat4::perspective_rh_gl(
            COCKPIT_FOV,
            rect.width() / rect.height().max(1.0),
            0.0002,
            10.0,
        ) * view;
        let mut frame = Frame {
            exterior: snapshot.exterior.clone().map(Arc::new),
            lighting_quality: self.lighting_quality,
            cockpit_vp,
            cockpit_occlusion: None,
            snapshot_time: snapshot.server_time_ms,
            planet_vp: Mat4::perspective_rh_gl(
                if tactical {
                    1.22
                } else if cockpit {
                    COCKPIT_FOV
                } else {
                    0.85
                },
                rect.aspect_ratio(),
                1.0,
                300_000.0,
            ) * view,
            system_id,
            ship_class: own_pilot.map_or(ShipClass::Pioneer, |p| p.ship_class),
            drone_camera: false,
            camera_drone: None,
            scene: Arc::new(FrameScene {
                world_layout: world_layout.clone(),
                traffic: snapshot
                    .traffic
                    .iter()
                    .filter(|a| a.system_id == system_id)
                    .cloned()
                    .collect(),
                surface_props: snapshot
                    .surface_props
                    .iter()
                    .filter(|p| p.system_id == system_id)
                    .cloned()
                    .collect(),
                asteroids: asteroids.clone(),
                gates: local_gates.clone(),
                pilots: snapshot
                    .pilots
                    .iter()
                    .filter(|p| p.system_id == system_id && p.location != Location::Docked)
                    .cloned()
                    .collect(),
            }),
            space,
            cockpit,
            rotation,
            connected,
            vp,
            eye,
            now,
            own: pilot_name.to_string(),

            docking: effects.events.iter().rev().find_map(|event| {
                let age = now.saturating_sub(event.started_ms);
                if !connected || age >= super::effects::EFFECT_DURATION_MS {
                    return None;
                }
                let docking = match event.kind {
                    super::effects::EffectKind::Docking => true,
                    super::effects::EffectKind::Undocking => false,
                    _ => return None,
                };
                Some((
                    age as f32 / super::effects::EFFECT_DURATION_MS as f32,
                    docking,
                ))
            }),
        };
        if cockpit {
            frame.cockpit_occlusion = if let Ok(renderer) = &self.renderer {
                renderer.lock().ok().and_then(|mut renderer| {
                    renderer.prepare_turrets(&frame);
                    let catalog = renderer.depth_meshes.clone();
                    let (external, own) = cockpit_occlusion_geometry::build(
                        &frame,
                        &catalog,
                        &renderer.station_batches,
                        &renderer.drone_parts,
                        &renderer.turrets,
                    )?;
                    CockpitOcclusion::new(class, catalog, external, own).map(Arc::new)
                })
            } else {
                prepare_turrets(&mut self.headless_turrets, &frame);
                self.depth_geometry.as_ref().ok().and_then(|geometry| {
                    let (external, own) = cockpit_occlusion_geometry::build(
                        &frame,
                        &geometry.catalog,
                        &geometry.station_batches,
                        &geometry.drone_parts,
                        &self.headless_turrets,
                    )?;
                    CockpitOcclusion::new(class, geometry.catalog.clone(), external, own)
                        .map(Arc::new)
                })
            };
            self.cockpit_display_frame =
                frame
                    .cockpit_occlusion
                    .clone()
                    .map(|scene| CockpitDisplayFrame {
                        scene,
                        vp: cockpit_vp,
                        mount: cockpit_mount,
                        viewport: rect,
                        class,
                    });
            self.cockpit_layer = self.cockpit_display_frame.as_ref().map(|_| ui.layer_id());
        }
        let instrument_surfaces: [Option<CockpitSurface>; 6] =
            std::array::from_fn(|index| self.physical_surface(index, egui::vec2(300., 180.)));
        self.instrument_surfaces = instrument_surfaces.iter().flatten().cloned().collect();
        self.computer_surface = self.physical_surface(3, cockpit_computer::COMPUTER_CANVAS_SIZE);
        self.console_rect = self.computer_surface.as_ref().map(CockpitSurface::bounds);
        let pointer_on_display = ui
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|point| {
                self.instrument_surfaces
                    .iter()
                    .any(|surface| surface.contains(point))
            });
        if pointer_on_display {
            self.input_surface = None;
        }
        world_input &= !pointer_on_display;
        let drone_surface = self.physical_surface(1, egui::vec2(344., 244.));
        match &self.renderer {
            Ok(renderer) => {
                let renderer = renderer.clone();
                let drone_screen = drone_surface.as_ref().map_or(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(344., 244.)),
                    |surface| surface.canvas_rect().shrink(8.),
                );
                let camera_drone = own_pilot.and_then(|p| {
                    fleet(p)
                        .into_iter()
                        .find(|d| Some(d.id) == self.selected_drone)
                        .or_else(|| fleet(p).first().copied())
                });
                let remote_worker = self
                    .remote_ship
                    .as_deref()
                    .and_then(|target| remote_worker(snapshot, pilot_name, target, now, connected));
                let remote_frame = remote_worker.filter(|_| cockpit).map(|worker| {
                    remote_ship_frame(&frame, snapshot, worker, drone_screen.aspect_ratio())
                });
                let mut camera_feeds = Vec::new();
                if cockpit && self.remote_ship.is_some() {
                    if let Some(feed) = remote_frame {
                        camera_feeds.push((drone_screen, feed, None));
                    }
                } else if cockpit
                    && self.drone_camera
                    && connected
                    && now.saturating_sub(snapshot.server_time_ms) <= 1500
                {
                    let fleet = own_pilot.map(fleet).unwrap_or_default();
                    self.drone_headings
                        .retain(|id, _| fleet.iter().any(|d| d.id == *id));
                    let chosen: Vec<_> = if self.drone_feeds.is_empty() {
                        camera_drone.into_iter().collect()
                    } else {
                        self.drone_feeds
                            .iter()
                            .filter_map(|id| fleet.iter().find(|d| d.id == *id).copied())
                            .take(4)
                            .collect()
                    };
                    let screens = feed_tiles(drone_screen, chosen.len());
                    for (drone, screen) in chosen.into_iter().zip(screens) {
                        let mut feed = frame.clone();
                        let mut camera_sample = drone.clone();
                        let heading =
                            Vec3::from_array(drone.heading.map(|v| v as f32)).normalize_or_zero();
                        let previous = self.drone_headings.entry(drone.id).or_insert(heading);
                        *previous = previous
                            .lerp(heading, 1.0 - (-dt * 14.0).exp())
                            .normalize_or_zero();
                        camera_sample.heading = previous.to_array().map(|v| v as f64);
                        let (eye, vp) = drone_mode_matrix(
                            &camera_sample,
                            space,
                            screen.aspect_ratio(),
                            self.drone_mode,
                            now,
                        );
                        feed.eye = eye;
                        feed.vp = vp;
                        feed.planet_vp = Mat4::perspective_rh_gl(
                            1.05,
                            screen.aspect_ratio().max(0.1),
                            1.0,
                            300_000.0,
                        ) * Mat4::perspective_rh_gl(
                            1.05,
                            screen.aspect_ratio().max(0.1),
                            0.002,
                            1200.0,
                        )
                        .inverse()
                            * vp;
                        feed.cockpit = false;
                        feed.drone_camera = true;
                        feed.camera_drone =
                            (self.drone_mode == DroneCameraMode::Pov).then_some(drone.id);
                        feed.docking = None;
                        camera_feeds.push((screen, feed, Some(drone)));
                    }
                }
                let drone_renderer = renderer.clone();
                // WebGL objects stay on the browser thread; guard callback access.
                #[cfg(target_arch = "wasm32")]
                let renderer = send_wrapper::SendWrapper::new(renderer);
                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(egui_glow::CallbackFn::new(move |_, painter| {
                        if let Ok(mut renderer) = renderer.lock() {
                            unsafe {
                                renderer.paint(&frame, painter.intermediate_fbo());
                            }
                        }
                    })),
                });
                for drone in own_pilot.map(fleet).unwrap_or_default() {
                    if let Some(point) = project_point(vp, rect, space.scene(drone.position)) {
                        let painter = ui.painter_at(rect);
                        painter.circle_stroke(point, 8.0, egui::Stroke::new(1.0, super::TEAL));
                        painter.text(
                            point + egui::vec2(0.0, -12.0),
                            egui::Align2::CENTER_BOTTOM,
                            format!("{:?} #{}", drone.class, drone.id),
                            egui::FontId::monospace(11.0),
                            super::TEAL,
                        );
                    }
                }
                if tactical && self.tactical_overlay {
                    paint_tactical_overlay(
                        ui.painter_at(rect),
                        vp,
                        rect,
                        self.distance,
                        selected_world.map(|p| space.scene(p)),
                    );
                }
                for pilot in snapshot.pilots.iter().filter(|p| {
                    p.system_id == system_id
                        && p.location != Location::Docked
                        && !(cockpit && p.name == pilot_name)
                }) {
                    let clip = vp
                        * (space.position(pilot, now)
                            + Vec3::Y * (pilot.ship_class.spec().height_m as f32 / 200.0 + 0.06))
                            .extend(1.0);
                    if clip.w > 0.0 {
                        let point = clip.truncate() / clip.w;
                        if point.x.abs() < 0.95 && point.y.abs() < 0.92 {
                            let screen = egui::pos2(
                                rect.center().x + point.x * rect.width() * 0.5,
                                rect.center().y - point.y * rect.height() * 0.5,
                            );
                            ui.painter_at(rect).text(
                                screen,
                                egui::Align2::CENTER_BOTTOM,
                                &pilot.name,
                                egui::FontId::proportional(12.0),
                                if pilot.name == pilot_name {
                                    super::TEAL
                                } else {
                                    super::AMBER
                                },
                            );
                        }
                    }
                }
                for actor in snapshot.traffic.iter().filter(|a| a.system_id == system_id) {
                    let pos = space.scene(actor.position);
                    if pos.distance(eye) < 300.0 {
                        if let Some(label) = project_point(vp, rect, pos) {
                            let role = match actor.role {
                                arkline_protocol::TrafficRole::StationShuttle => "Shuttle",
                                arkline_protocol::TrafficRole::FreightTender => "Freight tender",
                                arkline_protocol::TrafficRole::Patrol => "Patrol",
                            };
                            ui.painter_at(rect).text(
                                label + egui::vec2(0.0, -12.0),
                                egui::Align2::CENTER_BOTTOM,
                                format!("{role} · background traffic"),
                                egui::FontId::proportional(10.0),
                                super::MUTED,
                            );
                        }
                    }
                }
                let project = |point: Vec3| project_point(vp, rect, point);
                let own_pilot = snapshot
                    .pilots
                    .iter()
                    .find(|pilot| pilot.name == pilot_name);
                let can_navigate = !editor_free_camera
                    && connected
                    && own_pilot.is_some_and(|pilot| {
                        pilot.flight.is_some()
                            && !matches!(
                                pilot.location,
                                Location::Docked
                                    | Location::Jumping { .. }
                                    | Location::InTransit { .. }
                            )
                    });
                if world_input && (response.clicked() || (!cockpit && response.secondary_clicked()))
                {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let mut targets = Vec::new();
                        for asteroid in asteroids
                            .iter()
                            .filter(|a| space.scene(a.position).distance(eye) < 1100.0)
                        {
                            targets.push((
                                super::SelectedObject::Asteroid(asteroid.id),
                                rock_mesh(),
                                asteroid_model(asteroid, space, now),
                            ));
                        }
                        for prop in snapshot.surface_props.iter().filter(|p| {
                            p.system_id == system_id
                                && space.scene(p.position).distance(eye) < 1100.0
                        }) {
                            if let Some(model) = surface_prop_model(prop, space, world_layout) {
                                targets.push((super::SelectedObject::Prop(prop.id), cube(), model));
                            }
                        }
                        for gate in local_gates
                            .iter()
                            .filter(|g| space.scene(g.position).distance(eye) < 1100.0)
                        {
                            for (mesh, model) in gate_parts(gate, space) {
                                targets.push((super::SelectedObject::Gate(gate.id), mesh, model));
                            }
                        }
                        for (mesh, model, _, _) in station_parts(space)
                            .into_iter()
                            .filter(|_| space.scene(STATION_POSITION).distance(eye) < 1100.0)
                        {
                            targets.push((
                                super::SelectedObject::Station,
                                mesh_vertices(mesh),
                                model,
                            ));
                        }
                        for pilot in snapshot.pilots.iter().filter(|p| {
                            p.system_id == system_id
                                && p.location != Location::Docked
                                && !(cockpit && p.name == pilot_name)
                        }) {
                            let model = transform(
                                space.position(pilot, now),
                                Vec3::ONE,
                                ship_orientation_at(pilot, now),
                            );
                            for (mesh, part) in ship_geometry(pilot.ship_class, model) {
                                targets.push((
                                    super::SelectedObject::Pilot(pilot.name.clone()),
                                    mesh,
                                    part,
                                ));
                            }
                            if let Ok(renderer) = &self.renderer {
                                if let Ok(renderer) = renderer.lock() {
                                    for module_id in 0..2 {
                                        if let Some(state) =
                                            renderer.turrets.get(&(pilot.name.clone(), module_id))
                                        {
                                            for (mesh, part, _, _) in turret_parts(
                                                pilot.ship_class,
                                                state.rotation,
                                                module_id,
                                            ) {
                                                targets.push((
                                                    super::SelectedObject::Pilot(
                                                        pilot.name.clone(),
                                                    ),
                                                    mesh_vertices(mesh),
                                                    model * part,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        let object =
                            screen_ray(vp, rect, pointer).and_then(|(origin, direction)| {
                                targets
                                    .iter()
                                    .filter_map(|(object, mesh, model)| {
                                        ray_mesh_hit(origin, direction, *model, mesh)
                                            .map(|distance| (distance, object.clone()))
                                    })
                                    .min_by(|a, b| a.0.total_cmp(&b.0))
                                    .map(|(_, object)| object)
                            });
                        let destination = if object.is_none() && can_navigate {
                            own_pilot
                                .and_then(|pilot| {
                                    unproject_to_plane(
                                        vp,
                                        rect,
                                        pointer,
                                        space.position(pilot, now).y,
                                    )
                                })
                                .map(|point| space.world(point))
                                .filter(|point| {
                                    point
                                        .iter()
                                        .all(|v| v.is_finite() && v.abs() <= SPACE_BOUND)
                                })
                        } else {
                            None
                        };
                        if response.secondary_clicked() {
                            self.context_target = destination;
                            self.context_object = object.clone();
                        }
                        if let Some(object) = object {
                            picked = Some(ViewAction::Select(object));
                        } else if !cockpit && response.double_clicked() {
                            picked = destination.map(ViewAction::FlyTo);
                        }
                    }
                }
                if !cockpit
                    && !editor_free_camera
                    && (world_input || response.context_menu_opened())
                {
                    response.context_menu(|ui| {
                        ui.label("Navigation");
                        match &self.context_object {
                            Some(super::SelectedObject::Asteroid(id)) => {
                                if let Some(asteroid) = asteroids.iter().find(|a| a.id == *id) {
                                    ui.label(format!("{} · {} ore", asteroid.name, asteroid.ore));
                                    let lockable = can_navigate
                                        && own_pilot.is_some_and(|p| {
                                            space
                                                .position(p, now)
                                                .distance(space.scene(asteroid.position))
                                                * 100.0
                                                <= arkline_protocol::LOCK_RANGE as f32
                                        });
                                    if ui
                                        .add_enabled(lockable, egui::Button::new("Lock asteroid"))
                                        .clicked()
                                    {
                                        picked = Some(ViewAction::Command(
                                            arkline_protocol::Request::LockAsteroid { id: *id },
                                        ));
                                        ui.close_menu();
                                    }
                                    if ui
                                        .add_enabled(
                                            can_navigate,
                                            egui::Button::new("Approach asteroid"),
                                        )
                                        .clicked()
                                    {
                                        picked = Some(ViewAction::Command(
                                            arkline_protocol::Request::Navigate {
                                                mode: NavigationMode::Approach {
                                                    target: NavigationTarget::Asteroid { id: *id },
                                                },
                                            },
                                        ));
                                        ui.close_menu();
                                    }
                                }
                            }
                            Some(super::SelectedObject::Gate(id)) => {
                                if let Some(gate) = local_gates.iter().find(|g| g.id == *id) {
                                    ui.label(&gate.name);
                                    if ui
                                        .add_enabled(
                                            can_navigate,
                                            egui::Button::new("Warp to gate"),
                                        )
                                        .clicked()
                                    {
                                        picked = Some(ViewAction::Command(
                                            arkline_protocol::Request::Warp {
                                                destination: Destination::Gate { id: *id },
                                            },
                                        ));
                                        ui.close_menu();
                                    }
                                    let in_range = can_navigate
                                        && own_pilot.is_some_and(|p| {
                                            space
                                                .position(p, now)
                                                .distance(space.scene(gate.position))
                                                * 100.0
                                                <= arkline_protocol::GATE_RANGE as f32
                                        });
                                    if ui
                                        .add_enabled(
                                            in_range,
                                            egui::Button::new("Jump through gate"),
                                        )
                                        .clicked()
                                    {
                                        picked = Some(ViewAction::Command(
                                            arkline_protocol::Request::Jump { gate_id: *id },
                                        ));
                                        ui.close_menu();
                                    }
                                }
                            }
                            _ => {}
                        }

                        if ui
                            .add_enabled(
                                can_navigate && self.context_target.is_some(),
                                egui::Button::new("Fly here"),
                            )
                            .clicked()
                        {
                            picked = self.context_target.map(ViewAction::FlyTo);
                            ui.close_menu();
                        }
                        ui.small("Destination lies on your ship's horizontal plane.");
                    });
                }
                let painter = ui.painter_at(rect);
                for (label, meters) in std::iter::once(("Station".to_string(), STATION_POSITION))
                    .chain(std::iter::once((
                        "Resource belt".to_string(),
                        BELT_POSITION,
                    )))
                    .chain(local_gates.iter().map(|g| (g.name.clone(), g.position)))
                {
                    let relative = space.scene(meters) - eye;
                    if relative.length() > 1100.0 {
                        if let Some(point) = project(eye + relative.normalize_or_zero() * 900.0) {
                            let distance = meters
                                .iter()
                                .zip(space.origin)
                                .map(|(a, b)| (a - b) * (a - b))
                                .sum::<f64>()
                                .sqrt();
                            painter.circle_stroke(
                                point,
                                5.0,
                                egui::Stroke::new(1.0, super::TEAL.gamma_multiply(0.7)),
                            );
                            painter.text(
                                point + egui::vec2(0.0, 10.0),
                                egui::Align2::CENTER_TOP,
                                format!(
                                    "{label} · {:.2} AU",
                                    distance / arkline_protocol::AU_METERS
                                ),
                                egui::FontId::monospace(11.0),
                                super::TEAL,
                            );
                        }
                    }
                }
                if cockpit {
                    if let Some(own) = own_pilot {
                        paint_instruments(
                            ui,
                            &instrument_surfaces,
                            own,
                            snapshot,
                            selected_distance,
                            now,
                            connected,
                        );
                        let feed_count = camera_feeds.len();
                        if let Some(surface) = &drone_surface {
                            for (screen, feed, drone) in camera_feeds {
                                let fresh = connected
                                    && now.saturating_sub(snapshot.server_time_ms) <= 1500;
                                if drone.is_none_or(|d| d.signal_strength > 0.0) {
                                    let renderer = drone_renderer.clone();
                                    let surface = surface.clone();
                                    #[cfg(target_arch = "wasm32")]
                                    let renderer = send_wrapper::SendWrapper::new(renderer);
                                    #[cfg(target_arch = "wasm32")]
                                    let surface = send_wrapper::SendWrapper::new(surface);
                                    let key = egui::Id::new((
                                        pilot_name,
                                        drone.map(|d| d.id),
                                        self.remote_ship.as_deref(),
                                        self.drone_mode,
                                    ))
                                    .value();
                                    painter.add(egui::PaintCallback {
                                        rect,
                                        callback: Arc::new(egui_glow::CallbackFn::new(
                                            move |info, painter| {
                                                if let Ok(mut renderer) = renderer.lock() {
                                                    unsafe {
                                                        renderer.paint_cached_feed_on_surface(
                                                            &feed,
                                                            key,
                                                            info,
                                                            feed_count,
                                                            painter.intermediate_fbo(),
                                                            Some((&surface, screen)),
                                                        );
                                                    }
                                                }
                                            },
                                        )),
                                    });
                                }
                                surface.paint(ui, |overlay| {
                                    let overlay = overlay.with_clip_rect(screen);
                                    overlay.text(
                                        screen.left_top() + egui::vec2(3., 3.),
                                        egui::Align2::LEFT_TOP,
                                        if let Some(worker) = remote_worker {
                                            format!("{} / REMOTE SHIP", worker.name)
                                        } else {
                                            format!(
                                                "#{} / {}",
                                                drone.map_or(0, |d| d.id),
                                                self.drone_mode.label()
                                            )
                                        },
                                        egui::FontId::monospace(11.),
                                        super::TEAL,
                                    );
                                    if let Some(drone) = drone {
                                        paint_camera_signal(
                                            &overlay,
                                            screen,
                                            Some(drone),
                                            now,
                                            fresh,
                                        );
                                    }
                                });
                            }
                            if self.remote_ship.is_some() && remote_worker.is_none() {
                                surface.paint(ui, |overlay| {
                                    overlay.rect_filled(
                                        drone_screen,
                                        0.,
                                        egui::Color32::from_rgb(1, 8, 5),
                                    );
                                    overlay.text(
                                        drone_screen.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "REMOTE VIEW UNAVAILABLE / STALE",
                                        egui::FontId::monospace(11.),
                                        super::AMBER,
                                    );
                                });
                            } else if self.remote_ship.is_none()
                                && self.drone_camera
                                && feed_count == 0
                            {
                                surface.paint(ui, |overlay| {
                                    paint_camera_signal(
                                        overlay,
                                        drone_screen,
                                        camera_drone,
                                        now,
                                        connected
                                            && now.saturating_sub(snapshot.server_time_ms) <= 1500,
                                    )
                                });
                            }
                        }
                        if let Some(surface) = self.physical_surface(4, NAV_CANVAS_SIZE) {
                            if let Some(action) = paint_cockpit_maps(
                                ui,
                                &surface,
                                own,
                                snapshot,
                                now,
                                connected,
                                selected,
                                &mut self.map_range,
                            ) {
                                picked = Some(action);
                            }
                        }
                        if let Some(surface) =
                            self.physical_surface(5, cockpit_radar::RADAR_CANVAS_SIZE)
                        {
                            cockpit_radar::paint(
                                ui,
                                &surface,
                                own,
                                snapshot,
                                now,
                                connected,
                                self.map_range,
                            );
                        }
                        painter.circle_stroke(
                            rect.center(),
                            5.0,
                            egui::Stroke::new(1.0, super::TEAL.gamma_multiply(0.5)),
                        );
                    }
                }
                for gate in local_gates
                    .iter()
                    .filter(|g| space.scene(g.position).distance(eye) < 1100.0)
                {
                    if let Some(point) = project(
                        space.scene(gate.position) + Vec3::Y * (gate.radius as f32 / 100.0 + 1.0),
                    ) {
                        painter.text(
                            point,
                            egui::Align2::CENTER_BOTTOM,
                            &gate.name,
                            egui::FontId::proportional(13.0),
                            super::TEAL,
                        );
                    }
                }
                if let super::SelectedObject::Asteroid(id) = selected {
                    if let Some(a) = asteroids.iter().find(|a| a.id == *id) {
                        if let Some(point) = project(space.scene(a.position)) {
                            let ore = if a.ore == 0 {
                                "DEPLETED".into()
                            } else {
                                format!("{} ore", a.ore)
                            };
                            painter.text(
                                point - egui::vec2(0.0, 31.0),
                                egui::Align2::CENTER_BOTTOM,
                                format!("{} · {ore}", a.name),
                                egui::FontId::proportional(13.0),
                                super::TEAL,
                            );
                        }
                    }
                }

                if let Some(own) = own_pilot {
                    if let Some((label, destination)) =
                        navigation_destination(own, snapshot, now, space)
                    {
                        if let Some(point) = project(destination) {
                            let stroke = egui::Stroke::new(1.5, super::AMBER);
                            painter.circle_stroke(point, 12.0, stroke);
                            painter.line_segment(
                                [point - egui::vec2(18.0, 0.0), point + egui::vec2(18.0, 0.0)],
                                stroke,
                            );
                            painter.line_segment(
                                [point - egui::vec2(0.0, 18.0), point + egui::vec2(0.0, 18.0)],
                                stroke,
                            );
                            painter.text(
                                point + egui::vec2(0.0, -22.0),
                                egui::Align2::CENTER_BOTTOM,
                                label,
                                egui::FontId::monospace(12.0),
                                super::AMBER,
                            );
                            if let Some(ship) = project(space.position(own, now)) {
                                painter.line_segment(
                                    [ship, point],
                                    egui::Stroke::new(1.0, super::AMBER.gamma_multiply(0.35)),
                                );
                            }
                        }
                    }
                }
                if let Some(own) = own_pilot {
                    let mut targets = own.locked_asteroids.clone();
                    targets.extend(own.target_locks.iter().map(|l| l.asteroid_id));
                    targets.extend(own.locked_asteroid);
                    targets.extend(own.target_lock.iter().map(|l| l.asteroid_id));
                    targets.sort_unstable();
                    targets.dedup();
                    for asteroid in asteroids.iter().filter(|a| targets.contains(&a.id)) {
                        let pending = own
                            .target_locks
                            .iter()
                            .find(|l| l.asteroid_id == asteroid.id)
                            .or_else(|| {
                                own.target_lock
                                    .as_ref()
                                    .filter(|l| l.asteroid_id == asteroid.id)
                            });
                        if let Some(point) = project(space.scene(asteroid.position)) {
                            let (label, progress) = if let Some(lock) = pending {
                                let duration = lock
                                    .completes_at_ms
                                    .saturating_sub(lock.started_at_ms)
                                    .max(1);
                                (
                                    format!(
                                        "ACQUIRING {:.1}s",
                                        lock.completes_at_ms.saturating_sub(now) as f64 / 1000.0
                                    ),
                                    (now.saturating_sub(lock.started_at_ms) as f32
                                        / duration as f32)
                                        .clamp(0.0, 1.0),
                                )
                            } else {
                                ("LOCKED".into(), 1.0)
                            };
                            let color = if pending.is_some() {
                                super::AMBER
                            } else {
                                super::TEAL
                            };
                            painter.circle_stroke(
                                point,
                                35.0,
                                egui::Stroke::new(1.0, color.gamma_multiply(0.35)),
                            );
                            let points: Vec<_> = (0..=64)
                                .map(|i| {
                                    let a = -std::f32::consts::FRAC_PI_2
                                        + i as f32 / 64.0 * std::f32::consts::TAU * progress;
                                    point + egui::vec2(a.cos(), a.sin()) * 35.0
                                })
                                .collect();
                            painter.add(egui::Shape::line(points, egui::Stroke::new(2.0, color)));
                            painter.text(
                                point + egui::vec2(0.0, 49.0),
                                egui::Align2::CENTER_TOP,
                                label,
                                egui::FontId::monospace(12.0),
                                color,
                            );
                        }
                    }
                }
                let selected_visible = match selected {
                    super::SelectedObject::Asteroid(id) => asteroids.iter().any(|a| a.id == *id),
                    super::SelectedObject::Gate(id) => local_gates.iter().any(|g| g.id == *id),
                    super::SelectedObject::Pilot(name) => snapshot.pilots.iter().any(|pilot| {
                        &pilot.name == name
                            && pilot.system_id == system_id
                            && pilot.location != Location::Docked
                    }),
                    _ => true,
                };
                if selected_visible {
                    if let Some(point) = project(selected_position) {
                        let stroke = egui::Stroke::new(1.8, super::TEAL);
                        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                            let corner = point + egui::vec2(x * 23.0, y * 23.0);
                            painter
                                .line_segment([corner, corner - egui::vec2(x * 10.0, 0.0)], stroke);
                            painter
                                .line_segment([corner, corner - egui::vec2(0.0, y * 10.0)], stroke);
                        }
                        if let Some(own) = own_pilot {
                            let meters =
                                space.position(own, now).distance(selected_position) * 100.0;
                            painter.text(
                                point + egui::vec2(0.0, 31.0),
                                egui::Align2::CENTER_TOP,
                                if meters >= 1000.0 {
                                    format!("{:.2} km", meters / 1000.0)
                                } else {
                                    format!("{meters:.0} m")
                                },
                                egui::FontId::monospace(12.0),
                                super::TEAL,
                            );
                        }
                    }
                }

                if connected {
                    if let Some(own) = snapshot
                        .pilots
                        .iter()
                        .find(|pilot| pilot.name == pilot_name)
                    {
                        if let Location::Jumping { arrives_at_ms, .. } = own.location {
                            painter.text(
                                rect.center_top() + egui::vec2(0.0, 45.0),
                                egui::Align2::CENTER_TOP,
                                format!(
                                    "GATE TRANSFER  {:.1}s",
                                    arrives_at_ms.saturating_sub(now) as f32 / 1000.0
                                ),
                                egui::FontId::proportional(23.0),
                                super::TEAL,
                            );
                        }
                        if let Some((phase, progress)) = super::motion::warp_phase(own, now) {
                            painter.text(
                                rect.center_top() + egui::vec2(0.0, 45.0),
                                egui::Align2::CENTER_TOP,
                                format!("{phase}  {:02}%", (progress * 100.0) as u32),
                                egui::FontId::proportional(23.0),
                                super::TEAL,
                            );
                            let bar = egui::Rect::from_center_size(
                                rect.center_top() + egui::vec2(0.0, 83.0),
                                egui::vec2(240.0, 5.0),
                            );
                            painter.rect_filled(bar, 2.0, egui::Color32::from_rgb(24, 45, 55));
                            painter.rect_filled(
                                egui::Rect::from_min_size(
                                    bar.min,
                                    egui::vec2(bar.width() * progress, bar.height()),
                                ),
                                2.0,
                                super::TEAL,
                            );
                        } else if effects.mining(now).is_some()
                            && asteroids.iter().any(|a| {
                                Some(a.id) == effects.mining_target(now)
                                    && super::effects::can_extract_target(own, a)
                            })
                        {
                            painter.text(
                                rect.center_top() + egui::vec2(0.0, 45.0),
                                egui::Align2::CENTER_TOP,
                                "EXTRACTION CONFIRMED · ORE TRANSFER",
                                egui::FontId::proportional(20.0),
                                super::TEAL,
                            );
                        }
                    }
                }
                painter.text(
                    rect.left_top() + egui::vec2(18.0, 16.0),
                    egui::Align2::LEFT_TOP,
                    format!(
                        "{} / ARKLINE ORBITAL",
                        systems()
                            .iter()
                            .find(|s| s.id == system_id)
                            .map_or("UNKNOWN", |s| s.name.as_str())
                    ),
                    egui::FontId::monospace(12.0),
                    super::TEAL,
                );
                painter.text(
                    rect.left_bottom() + egui::vec2(18.0, -16.0),
                    egui::Align2::LEFT_BOTTOM,
                    if self.follow {
                        "CAMERA: SHIP TRACKING"
                    } else if self.focus_selection {
                        "CAMERA: SELECTED OBJECT"
                    } else {
                        "CAMERA: SECTOR OVERVIEW"
                    },
                    egui::FontId::monospace(11.0),
                    super::MUTED,
                );
                if !connected {
                    painter.text(
                        rect.center_top() + egui::vec2(0.0, 42.0),
                        egui::Align2::CENTER_TOP,
                        "CONNECTION LOST · LAST KNOWN STATE",
                        egui::FontId::proportional(15.0),
                        super::AMBER,
                    );
                }
            }
            Err(error) => {
                ui.painter_at(rect).text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    error,
                    egui::FontId::proportional(16.0),
                    super::AMBER,
                );
            }
        }
        picked
    }
}
#[derive(Clone)]
struct Frame {
    exterior: Option<Arc<arkline_protocol::ExteriorWorldState>>,
    lighting_quality: super::lighting_settings::LightingQuality,
    cockpit_vp: Mat4,
    cockpit_occlusion: Option<Arc<CockpitOcclusion>>,
    snapshot_time: u64,
    planet_vp: Mat4,
    system_id: u32,
    ship_class: ShipClass,
    drone_camera: bool,
    camera_drone: Option<u64>,
    scene: Arc<FrameScene>,
    space: Space,
    cockpit: bool,
    rotation: Quat,
    connected: bool,
    vp: Mat4,
    eye: Vec3,
    now: u64,
    own: String,

    docking: Option<(f32, bool)>,
}
#[derive(Clone)]
struct FrameScene {
    world_layout: WorldLayoutDefinition,
    traffic: Vec<arkline_protocol::TrafficActor>,
    surface_props: Vec<arkline_protocol::SurfaceProp>,
    asteroids: Vec<Asteroid>,
    gates: Vec<Stargate>,
    pilots: Vec<PilotView>,
}
#[derive(Clone, Copy)]
struct Vertex {
    p: Vec3,
    n: Vec3,
}
fn triangle(vertices: &mut Vec<Vertex>, a: Vec3, b: Vec3, c: Vec3) {
    let normal = (b - a).cross(c - a).normalize_or_zero();
    for p in [a, b, c] {
        vertices.push(Vertex { p, n: normal });
    }
}
fn cube() -> Vec<Vertex> {
    let mut out = vec![];
    for (normal, right, up) in [
        (Vec3::X, -Vec3::Z, Vec3::Y),
        (-Vec3::X, Vec3::Z, Vec3::Y),
        (Vec3::Y, Vec3::X, -Vec3::Z),
        (-Vec3::Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (-Vec3::Z, -Vec3::X, Vec3::Y),
    ] {
        let a = normal - right - up;
        let b = normal + right - up;
        let c = normal + right + up;
        let d = normal - right + up;
        triangle(&mut out, a, b, c);
        triangle(&mut out, a, c, d);
    }
    out
}
/// Chamfered armor block with the same [-1, 1] envelope as a cube.
/// Face, edge and corner normals produce real geometric highlight transitions.
fn armor_block() -> Vec<Vertex> {
    let mut out = Vec::new();
    let inset = 0.82;
    let mut face = |points: &[Vec3]| {
        for i in 1..points.len() - 1 {
            let (a, mut b, mut c) = (points[0], points[i], points[i + 1]);
            if (b - a).cross(c - a).dot(a + b + c) < 0.0 {
                std::mem::swap(&mut b, &mut c);
            }
            triangle(&mut out, a, b, c);
        }
    };
    let axes = [Vec3::X, Vec3::Y, Vec3::Z];
    for axis in 0..3 {
        let (u, v) = (axes[(axis + 1) % 3], axes[(axis + 2) % 3]);
        for sign in [-1.0, 1.0] {
            let n = axes[axis] * sign;
            face(&[
                n - u * inset - v * inset,
                n + u * inset - v * inset,
                n + u * inset + v * inset,
                n - u * inset + v * inset,
            ]);
        }
        for su in [-1.0, 1.0] {
            for sv in [-1.0, 1.0] {
                let (u, v, edge) = (u * su, v * sv, axes[axis] * inset);
                face(&[
                    u + v * inset - edge,
                    u * inset + v - edge,
                    u * inset + v + edge,
                    u + v * inset + edge,
                ]);
            }
        }
    }
    for x in [-1.0, 1.0] {
        for y in [-1.0, 1.0] {
            for z in [-1.0, 1.0] {
                face(&[
                    Vec3::new(x, y * inset, z * inset),
                    Vec3::new(x * inset, y, z * inset),
                    Vec3::new(x * inset, y * inset, z),
                ]);
            }
        }
    }
    out
}
fn industrial_shell() -> Vec<Vertex> {
    let mut out = Vec::new();
    let section = |x: f32, width: f32, height: f32| {
        [
            [-0.72, -1.0],
            [0.72, -1.0],
            [1.0, -0.62],
            [1.0, 0.62],
            [0.72, 1.0],
            [-0.72, 1.0],
            [-1.0, 0.62],
            [-1.0, -0.62],
        ]
        .map(|[z, y]| Vec3::new(x, y * height, z * width))
    };
    let rings = [
        section(-1.0, 0.82, 0.82),
        section(-0.78, 1.0, 1.0),
        section(0.50, 1.0, 1.0),
        section(1.0, 0.66, 0.70),
    ];
    let mut face = |a: Vec3, mut b: Vec3, mut c: Vec3| {
        if (b - a).cross(c - a).dot(a + b + c) < 0.0 {
            std::mem::swap(&mut b, &mut c);
        }
        triangle(&mut out, a, b, c);
    };
    for pair in rings.windows(2) {
        for i in 0..8 {
            let j = (i + 1) % 8;
            face(pair[0][i], pair[1][i], pair[1][j]);
            face(pair[0][i], pair[1][j], pair[0][j]);
        }
    }
    for ring in [rings[0], rings[3]] {
        for i in 1..7 {
            face(ring[0], ring[i], ring[i + 1]);
        }
    }
    out
}
fn torus(major: f32, minor: f32) -> Vec<Vertex> {
    let mut out = vec![];
    let point = |u: usize, v: usize| {
        let a = u as f32 / 64.0 * std::f32::consts::TAU;
        let b = v as f32 / 12.0 * std::f32::consts::TAU;
        let n = Vec3::new(a.cos() * b.cos(), b.sin(), a.sin() * b.cos());
        Vertex {
            p: Vec3::new(a.cos() * major, 0.0, a.sin() * major) + n * minor,
            n,
        }
    };
    for u in 0..64 {
        for v in 0..12 {
            for (a, b) in [
                (u, v),
                (u + 1, v + 1),
                (u + 1, v),
                (u, v),
                (u, v + 1),
                (u + 1, v + 1),
            ] {
                out.push(point(a, b));
            }
        }
    }
    out
}
fn sphere(rock: bool) -> Vec<Vertex> {
    sphere_with_resolution(rock, 18, 12)
}
fn sphere_with_resolution(rock: bool, slices: usize, stacks: usize) -> Vec<Vertex> {
    let mut out = vec![];
    let point = |u: usize, v: usize| {
        let a = u as f32 / slices as f32 * std::f32::consts::TAU;
        let b = v as f32 / stacks as f32 * std::f32::consts::PI;
        let n = Vec3::new(a.cos() * b.sin(), b.cos(), a.sin() * b.sin());
        let r = if rock {
            1.0 + 0.16 * (n.x * 7.0 + n.z * 4.0).sin() * (n.y * 6.0 - n.x * 3.0).cos()
        } else {
            1.0
        };
        Vertex { p: n * r, n }
    };
    for u in 0..slices {
        for v in 0..stacks {
            let p = [
                point(u, v),
                point(u + 1, v),
                point(u + 1, v + 1),
                point(u, v + 1),
            ];
            for indices in [[0, 1, 2], [0, 2, 3]] {
                if rock {
                    triangle(&mut out, p[indices[0]].p, p[indices[1]].p, p[indices[2]].p);
                } else {
                    for i in indices {
                        out.push(p[i]);
                    }
                }
            }
        }
    }
    // Pole triangles with zero area are omitted from GPU and validation alike.
    out.chunks_exact(3)
        .filter(|v| (v[1].p - v[0].p).cross(v[2].p - v[0].p).length_squared() > 1e-10)
        .flatten()
        .copied()
        .collect()
}
fn hull() -> Vec<Vertex> {
    let points = [
        Vec3::new(3.8, 0.0, 0.0),
        Vec3::new(-2.5, 0.7, -1.3),
        Vec3::new(-2.5, 0.7, 1.3),
        Vec3::new(-2.5, -0.6, -1.3),
        Vec3::new(-2.5, -0.6, 1.3),
    ];
    let mut out = vec![];
    for [a, b, c] in [
        [0, 1, 2],
        [0, 3, 1],
        [0, 2, 4],
        [0, 4, 3],
        [1, 3, 4],
        [1, 4, 2],
    ] {
        triangle(&mut out, points[a], points[b], points[c]);
    }
    out
}
#[path = "materials.rs"]
mod materials;
use materials::Material;

struct Mesh {
    vao: glow::VertexArray,
    buffer: glow::Buffer,
    count: i32,
}
struct Uniforms {
    model: Option<glow::UniformLocation>,
    color: Option<glow::UniformLocation>,
    emission: Option<glow::UniformLocation>,
    emissive_color: Option<glow::UniformLocation>,
    emission_power: Option<glow::UniformLocation>,
    emission_override: Option<glow::UniformLocation>,
    vp: Option<glow::UniformLocation>,
    eye: Option<glow::UniformLocation>,
    surface_opacity: Option<glow::UniformLocation>,
    material: Option<glow::UniformLocation>,
    material_params: Option<glow::UniformLocation>,
    hull_surface: Option<glow::UniformLocation>,
    environment_atmosphere: Option<glow::UniformLocation>,
    station_lighting: Option<glow::UniformLocation>,
    environment_up: Option<glow::UniformLocation>,
    sky_inverse: Option<glow::UniformLocation>,
    sky_eye: Option<glow::UniformLocation>,
    planet_center: Option<glow::UniformLocation>,
    atmosphere: Option<glow::UniformLocation>,
    planet_up: Option<glow::UniformLocation>,
}
impl Uniforms {
    unsafe fn new(gl: &glow::Context, program: glow::Program, background: glow::Program) -> Self {
        Self {
            planet_center: gl.get_uniform_location(program, "planet_center"),
            atmosphere: gl.get_uniform_location(background, "atmosphere"),
            planet_up: gl.get_uniform_location(background, "planet_up"),
            model: gl.get_uniform_location(program, "model"),
            color: gl.get_uniform_location(program, "base_color"),
            emission: gl.get_uniform_location(program, "emission"),
            emissive_color: gl.get_uniform_location(program, "emissive_color"),
            emission_power: gl.get_uniform_location(program, "emission_power"),
            emission_override: gl.get_uniform_location(program, "emission_override"),
            vp: gl.get_uniform_location(program, "view_projection"),
            eye: gl.get_uniform_location(program, "eye"),
            surface_opacity: gl.get_uniform_location(program, "surface_opacity"),
            material: gl.get_uniform_location(program, "material_kind"),
            material_params: gl.get_uniform_location(program, "material_params"),
            hull_surface: gl.get_uniform_location(program, "hull_surface"),
            environment_atmosphere: gl.get_uniform_location(program, "environment_atmosphere"),
            station_lighting: gl.get_uniform_location(program, "station_lighting"),
            environment_up: gl.get_uniform_location(program, "environment_up"),
            sky_inverse: gl.get_uniform_location(background, "inverse_view_projection"),
            sky_eye: gl.get_uniform_location(background, "camera_eye"),
        }
    }
}
struct CachedFeed {
    key: u64,
    framebuffer: glow::Framebuffer,
    texture: glow::Texture,
    depth: glow::Renderbuffer,
    width: i32,
    height: i32,
    painted_ms: Option<u64>,
}
impl CachedFeed {
    unsafe fn create(
        gl: &glow::Context,
        key: u64,
        width: i32,
        height: i32,
    ) -> Result<Self, String> {
        let framebuffer = gl.create_framebuffer()?;
        let texture = match gl.create_texture() {
            Ok(t) => t,
            Err(e) => {
                gl.delete_framebuffer(framebuffer);
                return Err(e);
            }
        };
        let depth = match gl.create_renderbuffer() {
            Ok(d) => d,
            Err(e) => {
                gl.delete_texture(texture);
                gl.delete_framebuffer(framebuffer);
                return Err(e);
            }
        };
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            width,
            height,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        gl.bind_renderbuffer(glow::RENDERBUFFER, Some(depth));
        gl.renderbuffer_storage(glow::RENDERBUFFER, glow::DEPTH_COMPONENT24, width, height);
        gl.framebuffer_renderbuffer(
            glow::FRAMEBUFFER,
            glow::DEPTH_ATTACHMENT,
            glow::RENDERBUFFER,
            Some(depth),
        );
        let cache = Self {
            key,
            framebuffer,
            texture,
            depth,
            width,
            height,
            painted_ms: None,
        };
        if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
            cache.destroy(gl);
            return Err("Camera framebuffer unavailable".into());
        }
        Ok(cache)
    }
    unsafe fn destroy(self, gl: &glow::Context) {
        gl.delete_framebuffer(self.framebuffer);
        gl.delete_texture(self.texture);
        gl.delete_renderbuffer(self.depth);
    }
}
/// Submitted mesh work for the most recent GPU scene pass; excludes sky and shadow passes.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DebugRenderMetrics {
    pub draw_calls: u64,
    pub triangles: u64,
    pub wireframe_draw_calls: u64,
}
fn prepare_turrets(
    turrets: &mut std::collections::HashMap<(String, u8), TurretState>,
    frame: &Frame,
) {
    let space = frame.space;
    // Main and remote passes may show different systems. Keep recently drawn
    // turret state between passes so neither camera resets the other's slew.
    turrets.retain(|(name, _), state| {
        frame.scene.pilots.iter().any(|p| &p.name == name)
            || frame.now.saturating_sub(state.last_ms) < 5000
    });
    for pilot in &frame.scene.pilots {
        for module_id in 0..2 {
            let state = turrets
                .entry((pilot.name.clone(), module_id))
                .or_insert(TurretState {
                    rotation: Quat::IDENTITY,
                    last_ms: frame.now,
                });
            let target = pilot
                .mining_modules
                .iter()
                .find(|m| m.module_id == module_id)
                .and_then(|m| m.target_id)
                .or_else(|| {
                    pilot
                        .target_lock
                        .as_ref()
                        .map(|lock| lock.asteroid_id)
                        .or(pilot.locked_asteroid)
                });
            let ship = transform(
                space.position(pilot, frame.now),
                Vec3::ONE,
                ship_orientation_at(pilot, frame.now),
            );
            let desired = target
                .and_then(|id| {
                    frame
                        .scene
                        .asteroids
                        .iter()
                        .find(|a| a.id == id && a.system_id == pilot.system_id)
                })
                .and_then(|a| {
                    let direction = ship.inverse().transform_point3(space.scene(a.position))
                        - turret_mount(pilot.ship_class, module_id).0;
                    direction
                        .try_normalize()
                        .map(|d| Quat::from_rotation_arc(Vec3::X, d))
                })
                .unwrap_or(Quat::IDENTITY);
            let seconds = frame.now.saturating_sub(state.last_ms).min(250) as f32 / 1000.0;
            state.rotation = slew_rotation(state.rotation, desired, 1.4 * seconds);
            state.last_ms = frame.now;
        }
    }
}

struct FlightGeometry {
    catalog: Arc<DepthMeshCatalog>,
    station_batches: Vec<(usize, Vec3, f32)>,
    drone_parts: [Arc<Vec<ModelPart>>; 3],
    docked_meshes: Option<[usize; 3]>,
    docked_parts: Result<Arc<Vec<ModelPart>>, String>,
}
fn flight_geometry() -> Result<Arc<FlightGeometry>, String> {
    static GEOMETRY: std::sync::OnceLock<Result<Arc<FlightGeometry>, String>> =
        std::sync::OnceLock::new();
    GEOMETRY
        .get_or_init(|| {
            let mut upload = vec![
                cube(),
                torus(1.0, 0.12),
                sphere(false),
                rock_mesh(),
                hull(),
                sphere_with_resolution(false, 72, 48),
                torus(1.0, 0.008),
                surface_patch(),
                armor_block(),
                industrial_shell(),
            ];
            upload.extend(pioneer_asset()?.iter().map(|p| p.vertices.clone()));
            upload.push(hangar::station_floor_vertices());
            let mut station_batches = Vec::new();
            for (vertices, color, emission) in station_batches_geometry() {
                station_batches.push((upload.len(), color, emission));
                upload.push(vertices);
            }
            // Append after all existing uploads so ship, floor and station mesh
            // indices stay stable. Flight, inspection, depth and pointer rays
            // retain these exact same baked vertices and material tuples.
            let drone_parts = drone_models::CLASSES.map(|class| {
                Arc::new(
                    drone_models::model(class)
                        .batches
                        .iter()
                        .map(|batch| {
                            let mesh = upload.len();
                            upload.push(batch.vertices.clone());
                            (mesh, Mat4::IDENTITY, batch.color, batch.emission)
                        })
                        .collect(),
                )
            });
            // Derived runtime cuts append new buffers. Inspection retains every
            // original upload, source ID, draw range and original material.
            let (docked_meshes, docked_parts) = match pioneer_docked_source::source_meshes() {
                Ok(source) => {
                    let slots = [upload.len(), upload.len() + 1, upload.len() + 2];
                    upload.extend(source);
                    let parts = Arc::new(
                        pioneer_asset()?
                            .iter()
                            .enumerate()
                            .map(|(i, p)| {
                                let mesh = [0, 1, 6]
                                    .iter()
                                    .position(|&v| v == i)
                                    .map_or(10 + i, |slot| slots[slot]);
                                (mesh, Mat4::IDENTITY, p.color, p.emission)
                            })
                            .collect(),
                    );
                    (Some(slots), Ok(parts))
                }
                Err(error) => (None, Err(error)),
            };
            Ok(Arc::new(FlightGeometry {
                catalog: Arc::new(DepthMeshCatalog::new(
                    upload.into_iter().map(Arc::from).collect(),
                )),
                station_batches,
                drone_parts,
                docked_meshes,
                docked_parts,
            }))
        })
        .clone()
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn cached_drone_parts(class: arkline_protocol::DroneClass) -> Result<Arc<Vec<ModelPart>>, String> {
    Ok(flight_geometry()?.drone_parts[drone_models::class_index(class)].clone())
}

#[cfg(any(not(target_arch = "wasm32"), test))]
fn drone_model_bounds(class: arkline_protocol::DroneClass) -> (Vec3, Vec3) {
    let model = drone_models::model(class);
    (model.min, model.max)
}

fn drone_model_transform(space: Space, drone: &arkline_protocol::DroneView, now: u64) -> Mat4 {
    transform(
        space.scene(drone_present_position(drone, now)),
        Vec3::ONE,
        orientation(drone.heading).unwrap_or(Quat::IDENTITY),
    )
}

struct Renderer {
    runtime_surface_opacity: std::cell::Cell<f32>,
    exterior_emission: std::cell::Cell<exterior_visuals::EmissionPower>,
    exterior_cache: Option<(Option<Arc<arkline_protocol::ExteriorWorldState>>, bool)>,
    material_preview: Option<crate::EmbeddedMaterialPreview>,
    spatial_material: MaterialProgramState,
    spatial_uniforms: Option<SpatialMaterialUniforms>,
    #[cfg(not(target_arch = "wasm32"))]
    debug_wireframe: bool,
    #[cfg(not(target_arch = "wasm32"))]
    debug_metrics: std::cell::Cell<DebugRenderMetrics>,
    #[cfg(not(target_arch = "wasm32"))]
    view_profiler: std::cell::RefCell<ViewProfiler>,
    gl: Arc<glow::Context>,
    program: glow::Program,
    background: glow::Program,
    feed_program: glow::Program,
    empty: glow::VertexArray,
    meshes: Vec<Mesh>,
    depth_meshes: Arc<DepthMeshCatalog>,
    cockpit_depth: CockpitDepthProgram,
    uniforms: Uniforms,
    local_lights: local_lighting::Uniforms,
    station_batches: Vec<(usize, Vec3, f32)>,
    drone_parts: [Arc<Vec<ModelPart>>; 3],
    feed_cache: Vec<CachedFeed>,
    destroyed: bool,
    sun_shadows: Vec<sun_shadows::SunShadows>,
    shadow_fallback: glow::Texture,
    shadow_failed: bool,
    turrets: std::collections::HashMap<(String, u8), TurretState>,
}
impl Renderer {
    /// The registered runtime adapts actual source panes to tinted glazing.
    /// Canonical inspection materials/buffers stay opaque and unchanged. This
    /// pass retains pane triangles, uses no opaque depth writes, and draws after
    /// opaque geometry; it does not turn the whole source batch into an opening.
    unsafe fn draw_registered_glass(&self, part: &ModelPart) {
        let gl = &self.gl;
        let blend = gl.is_enabled(glow::BLEND);
        let src_rgb = gl.get_parameter_i32(glow::BLEND_SRC_RGB) as u32;
        let dst_rgb = gl.get_parameter_i32(glow::BLEND_DST_RGB) as u32;
        let src_alpha = gl.get_parameter_i32(glow::BLEND_SRC_ALPHA) as u32;
        let dst_alpha = gl.get_parameter_i32(glow::BLEND_DST_ALPHA) as u32;
        let depth_write = gl.get_parameter_bool(glow::DEPTH_WRITEMASK);
        let equation_rgb = gl.get_parameter_i32(glow::BLEND_EQUATION_RGB) as u32;
        let equation_alpha = gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA) as u32;
        gl.enable(glow::BLEND);
        gl.blend_equation_separate(glow::FUNC_ADD, glow::FUNC_ADD);
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
        );
        gl.depth_mask(false);
        self.runtime_surface_opacity.set(0.16);
        self.draw(part.0, part.1, part.2, part.3);
        self.runtime_surface_opacity.set(1.);
        gl.depth_mask(depth_write);
        gl.blend_func_separate(src_rgb, dst_rgb, src_alpha, dst_alpha);
        gl.blend_equation_separate(equation_rgb, equation_alpha);
        if !blend {
            gl.disable(glow::BLEND);
        }
    }
    fn visible_part(&self, vp: Mat4, part: &ModelPart, stats: &mut ViewVisibility) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let started = std::time::Instant::now();
        let triangles = self.depth_meshes.vertex_count(part.0) as u64 / 3;
        stats.candidate_draws += 1;
        stats.candidate_triangles += triangles;
        let visible = part_in_view(&self.depth_meshes, vp, part);
        if visible {
            stats.submitted_draws += 1;
            stats.submitted_triangles += triangles;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            stats.cpu_prepare_ns += started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
        }
        visible
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe fn begin_view_profile(&self, key: &str, frame: u64) -> ViewProfileStart {
        self.view_profiler.borrow_mut().begin(&self.gl, key, frame)
    }
    #[cfg(not(target_arch = "wasm32"))]
    unsafe fn finish_view_profile(&self, start: ViewProfileStart, visibility: ViewVisibility) {
        self.view_profiler
            .borrow_mut()
            .finish(&self.gl, start, visibility);
    }
    unsafe fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        if gl.version().is_embedded && gl.version().major < 3 {
            return Err("Arkline 3D requires WebGL2 / OpenGL ES 3 or newer".into());
        }
        let program = compile(
            &gl,
            VERTEX,
            &format!(
                "{}{}{}",
                mesh_fragment_prefix(),
                AUTHORED_MATERIAL_FUNCTION,
                FRAGMENT_BODY
            ),
        )?;
        let local_lights = local_lighting::Uniforms::new(&gl, program);
        let background = compile(&gl, BACK_VERTEX, BACK_FRAGMENT)?;
        let feed_program = compile(&gl, FEED_VERTEX, FEED_FRAGMENT)?;
        let cockpit_depth = CockpitDepthProgram::new(&gl)?;
        let empty = gl.create_vertex_array()?;
        let mut meshes = vec![];
        let uniforms = Uniforms::new(&gl, program, background);
        let geometry = flight_geometry()?;
        let station_batches = geometry.station_batches.clone();
        for vertices in geometry.catalog.uploads() {
            let vao = gl.create_vertex_array()?;
            let buffer = gl.create_buffer()?;
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            let bytes: Vec<u8> = vertices
                .iter()
                .flat_map(|v| [v.p.x, v.p.y, v.p.z, v.n.x, v.n.y, v.n.z])
                .flat_map(f32::to_ne_bytes)
                .collect();
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STATIC_DRAW);
            for index in 0..2 {
                gl.enable_vertex_attrib_array(index);
                gl.vertex_attrib_pointer_f32(index, 3, glow::FLOAT, false, 24, index as i32 * 12);
            }
            meshes.push(Mesh {
                vao,
                buffer,
                count: vertices.len() as i32,
            });
        }
        gl.bind_vertex_array(None);
        let shadow_fallback = sun_shadows::fallback(&gl)?;
        #[cfg(not(target_arch = "wasm32"))]
        let geometry_gl = gl.clone();
        Ok(Self {
            runtime_surface_opacity: std::cell::Cell::new(1.),
            exterior_emission: Default::default(),
            exterior_cache: None,
            gl,
            program,
            background,
            feed_program,
            empty,
            meshes,
            depth_meshes: geometry.catalog.clone(),
            cockpit_depth,
            uniforms,
            local_lights,
            station_batches,
            drone_parts: geometry.drone_parts.clone(),
            feed_cache: Vec::new(),
            destroyed: false,
            material_preview: None,
            spatial_material: Default::default(),
            spatial_uniforms: None,
            #[cfg(not(target_arch = "wasm32"))]
            debug_wireframe: false,
            #[cfg(not(target_arch = "wasm32"))]
            debug_metrics: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            view_profiler: std::cell::RefCell::new(ViewProfiler::new(&geometry_gl)),
            sun_shadows: Vec::new(),
            shadow_fallback,
            shadow_failed: false,
            turrets: std::collections::HashMap::new(),
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    fn set_material_preview(&mut self, preview: Option<crate::EmbeddedMaterialPreview>) {
        let preview = preview.map(crate::EmbeddedMaterialPreview::validated);
        if self.material_preview != preview {
            self.material_preview = preview;
            // Geometry is unchanged: only per-draw uniforms and cached camera pixels change.
            for feed in &mut self.feed_cache {
                feed.painted_ms = None;
            }
        }
    }
    unsafe fn prepare_spatial_material(&mut self) {
        let gl = self.gl.clone();
        let background = self.background;
        let update = self.spatial_material.prepare_validated(
            |revision| {
                let expected = arkline_protocol::material_graph::AssetMaterialBinding::pioneer();
                if *revision.binding() != expected || revision.binding().asset_sha256 != pioneer_asset_sha256() {
                    return Err(MaterialProgramDiagnostic {
                        phase: MaterialProgramErrorPhase::Binding,
                        node_ids: vec![],
                        message: "This renderer supports only the current embedded Pioneer armor material slot 0. The previous valid surface remains visible.".into(),
                    });
                }
                Ok(())
            },
            |revision| {
                let source = MaterialShaderSource::new(&mesh_fragment_prefix(), revision, FRAGMENT_BODY);
                compile_material_program(&gl, VERTEX, &source, revision, |gl, program| {
                    Ok((Uniforms::new(gl, program, background), local_lighting::Uniforms::new(gl, program), SpatialMaterialUniforms::new(gl, program)))
                })
            },
        );
        let Some(update) = update else { return };
        match self.spatial_material.commit(update) {
            Ok(PreparedMaterialUpdate::Program { candidate, .. }) => {
                candidate.install(
                    &gl,
                    &mut self.program,
                    |(uniforms, local_lights, spatial)| {
                        self.uniforms = uniforms;
                        self.local_lights = local_lights;
                        self.spatial_uniforms = Some(spatial);
                    },
                );
            }
            Err(PreparedMaterialUpdate::Program { candidate, .. }) => {
                candidate.destroy(&gl);
                return;
            }
            Err(_) => return,
            Ok(_) => {}
        }
        // Includes parameter/binding/reset changes while the simulation is paused.
        for feed in &mut self.feed_cache {
            feed.painted_ms = None;
        }
    }
    unsafe fn upload_spatial_material(&self) {
        if let (Some(uniforms), Some(revision)) =
            (&self.spatial_uniforms, self.spatial_material.applied())
        {
            uniforms.upload(&self.gl, revision);
        }
    }
    unsafe fn draw(&self, mesh: usize, model: Mat4, color: Vec3, emission: f32) {
        self.draw_material(mesh, model, color, emission, Material::PaintedHull);
    }
    unsafe fn draw_material(
        &self,
        mesh: usize,
        model: Mat4,
        color: Vec3,
        emission: f32,
        material: Material,
    ) {
        self.draw_material_range(mesh, model, color, emission, material, None);
    }
    unsafe fn draw_material_range(
        &self,
        mesh: usize,
        model: Mat4,
        color: Vec3,
        emission: f32,
        material: Material,
        range: Option<(usize, usize)>,
    ) {
        let Some(buffer) = self.meshes.get(mesh_index(mesh)) else {
            return;
        };
        let Some((first, count)) = triangle_draw_range(buffer.count, range) else {
            return;
        };
        let graph_enabled = self.spatial_material.applied().is_some_and(|revision| {
            embedded_primitive(mesh).is_some_and(|p| {
                p.material_index == Some(revision.binding().material_index as usize)
            })
        });
        let (color, emission) = if graph_enabled {
            embedded_primitive(mesh).map_or((color, emission), |p| (p.color, p.emission))
        } else {
            preview_material(mesh, color, emission, self.material_preview)
        };
        let mut params = embedded_primitive(mesh).map_or_else(
            || material.parameters(),
            |p| [p.roughness, p.metallic, 0.0, 0.0],
        );
        if let Some(preview) = self
            .material_preview
            .filter(|_| !graph_enabled && preview_paint(mesh))
        {
            if let Some(roughness) = preview.roughness {
                params[0] = roughness.clamp(0.08, 1.0);
            }
            if let Some(metallic) = preview.metallic {
                params[1] = metallic;
            }
        }
        self.gl.uniform_4_f32(
            self.uniforms.material_params.as_ref(),
            params[0],
            params[1],
            params[2],
            params[3],
        );
        let gl = &self.gl;
        gl.uniform_1_f32(
            self.uniforms.emission_power.as_ref(),
            self.exterior_emission.get().for_mesh(mesh),
        );
        let emitted = self.exterior_emission.get().override_for_mesh(mesh);
        let color_override = emitted.unwrap_or(Vec3::ZERO);
        gl.uniform_4_f32(
            self.uniforms.emission_override.as_ref(),
            color_override.x,
            color_override.y,
            color_override.z,
            if emitted.is_some() { 1. } else { 0. },
        );
        if let Some(uniforms) = &self.spatial_uniforms {
            uniforms.set_enabled(gl, graph_enabled);
        }
        gl.uniform_1_f32(
            self.uniforms.hull_surface.as_ref(),
            if !graph_enabled
                && embedded_primitive(mesh)
                    .is_some_and(|p| p.emission == 0.0 && p.roughness >= 0.25)
            {
                1.0
            } else {
                0.0
            },
        );
        gl.uniform_matrix_4_f32_slice(self.uniforms.model.as_ref(), false, &model.to_cols_array());
        gl.uniform_3_f32(self.uniforms.color.as_ref(), color.x, color.y, color.z);
        gl.uniform_1_f32(
            self.uniforms.surface_opacity.as_ref(),
            self.runtime_surface_opacity.get(),
        );
        gl.uniform_1_f32(self.uniforms.emission.as_ref(), emission);
        let light_color = embedded_primitive(mesh).map_or(color, |p| p.emissive_color);
        gl.uniform_3_f32(
            self.uniforms.emissive_color.as_ref(),
            light_color.x,
            light_color.y,
            light_color.z,
        );
        gl.bind_vertex_array(Some(self.meshes[mesh_index(mesh)].vao));
        gl.draw_arrays(glow::TRIANGLES, first, count);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut metrics = self.debug_metrics.get();
            metrics.draw_calls += 1;
            metrics.triangles += count as u64 / 3;
            if self.debug_wireframe {
                let mut modes = [0; 2];
                gl.get_parameter_i32_slice(glow::POLYGON_MODE, &mut modes);
                let offset_enabled = gl.is_enabled(glow::POLYGON_OFFSET_LINE);
                let factor = gl.get_parameter_f32(glow::POLYGON_OFFSET_FACTOR);
                let units = gl.get_parameter_f32(glow::POLYGON_OFFSET_UNITS);
                gl.enable(glow::POLYGON_OFFSET_LINE);
                gl.polygon_offset(-1., -1.);
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
                gl.uniform_3_f32(self.uniforms.color.as_ref(), 0.02, 0.9, 0.25);
                gl.uniform_3_f32(self.uniforms.emissive_color.as_ref(), 0.02, 0.9, 0.25);
                gl.uniform_1_f32(self.uniforms.emission.as_ref(), 1.5);
                if let Some(uniforms) = &self.spatial_uniforms {
                    uniforms.set_enabled(gl, false);
                }
                gl.draw_arrays(glow::TRIANGLES, first, count);
                if let Some(uniforms) = &self.spatial_uniforms {
                    uniforms.set_enabled(gl, graph_enabled);
                }
                // Core GL uses a shared front/back polygon mode.
                gl.polygon_mode(glow::FRONT_AND_BACK, modes[0] as u32);
                gl.polygon_offset(factor, units);
                if !offset_enabled {
                    gl.disable(glow::POLYGON_OFFSET_LINE);
                }
                gl.uniform_3_f32(self.uniforms.color.as_ref(), color.x, color.y, color.z);
                gl.uniform_3_f32(
                    self.uniforms.emissive_color.as_ref(),
                    light_color.x,
                    light_color.y,
                    light_color.z,
                );
                gl.uniform_1_f32(self.uniforms.emission.as_ref(), emission);
                metrics.wireframe_draw_calls += 1;
            }
            self.debug_metrics.set(metrics);
        }
    }
    #[cfg(test)]
    unsafe fn paint_cached_feed(
        &mut self,
        frame: &Frame,
        key: u64,
        info: egui::PaintCallbackInfo,
        count: usize,
        callback_target: Option<glow::Framebuffer>,
    ) {
        self.paint_cached_feed_on_surface(frame, key, info, count, callback_target, None);
    }

    #[allow(clippy::too_many_arguments)]
    unsafe fn paint_cached_feed_on_surface(
        &mut self,
        frame: &Frame,
        key: u64,
        info: egui::PaintCallbackInfo,
        count: usize,
        callback_target: Option<glow::Framebuffer>,
        geometry: Option<(&CockpitSurface, egui::Rect)>,
    ) {
        if self.destroyed {
            return;
        }
        let viewport = info.viewport_in_pixels();
        if viewport.width_px <= 0 || viewport.height_px <= 0 {
            return;
        }
        // The cache has stable logical dimensions while the physical screen
        // foreshortens. Only the final quad uses the full scene viewport.
        let (surface_geometry, width, height) = if let Some((surface, tile)) = geometry {
            let tile = tile.intersect(surface.canvas_rect());
            let Some(quad) = surface.feed_geometry(tile) else {
                return;
            };
            let (width, height) =
                feed_resolution(tile.width().round() as i32, tile.height().round() as i32);
            (Some(quad), width, height)
        } else {
            let (width, height) = feed_resolution(viewport.width_px, viewport.height_px);
            (None, width, height)
        };
        let gl = self.gl.clone();
        // glow 0.16 cannot query WebGL's null object bindings. The callback
        // supplies that target, and egui restores its object bindings on return.
        #[cfg(target_arch = "wasm32")]
        let (draw, read) = (callback_target, callback_target);
        #[cfg(not(target_arch = "wasm32"))]
        let (draw, read) = (
            gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
            gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
        );
        #[cfg(not(target_arch = "wasm32"))]
        let _ = callback_target;
        let mut old_viewport = [0; 4];
        let mut old_scissor = [0; 4];
        let mut depth_range = [0.0; 2];
        let mut clear_color = [0.0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut old_viewport);
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut old_scissor);
        gl.get_parameter_f32_slice(glow::DEPTH_RANGE, &mut depth_range);
        gl.get_parameter_f32_slice(glow::COLOR_CLEAR_VALUE, &mut clear_color);
        let clear_depth = gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE);
        let capabilities = [
            glow::SCISSOR_TEST,
            glow::DEPTH_TEST,
            glow::BLEND,
            glow::CULL_FACE,
            glow::STENCIL_TEST,
            glow::POLYGON_OFFSET_FILL,
            glow::SAMPLE_ALPHA_TO_COVERAGE,
        ]
        .map(|capability| (capability, gl.is_enabled(capability)));
        let desktop_srgb = !gl.version().is_embedded;
        let srgb = desktop_srgb && gl.is_enabled(glow::FRAMEBUFFER_SRGB);
        let depth_mask = gl.get_parameter_bool(glow::DEPTH_WRITEMASK);
        let depth_func = gl.get_parameter_i32(glow::DEPTH_FUNC) as u32;
        let color_mask = gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK);
        let blend = [
            glow::BLEND_SRC_RGB,
            glow::BLEND_DST_RGB,
            glow::BLEND_SRC_ALPHA,
            glow::BLEND_DST_ALPHA,
        ]
        .map(|parameter| gl.get_parameter_i32(parameter) as u32);
        let equation = [glow::BLEND_EQUATION_RGB, glow::BLEND_EQUATION_ALPHA]
            .map(|parameter| gl.get_parameter_i32(parameter) as u32);
        let active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        gl.active_texture(glow::TEXTURE0);
        #[cfg(not(target_arch = "wasm32"))]
        let objects = (
            gl.get_parameter_program(glow::CURRENT_PROGRAM),
            gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
            gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING),
            gl.get_parameter_renderbuffer(glow::RENDERBUFFER_BINDING),
            gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
            gl.get_parameter_sampler(glow::SAMPLER_BINDING),
        );
        #[cfg(not(target_arch = "wasm32"))]
        let polygon_mode = {
            let mut mode = [0; 2];
            gl.get_parameter_i32_slice(glow::POLYGON_MODE, &mut mode);
            mode[0] as u32
        };

        // Must precede feed_due: paused feeds also need newly installed pixels.
        self.prepare_spatial_material();
        self.prepare_exterior(frame);
        gl.color_mask(true, true, true, true);
        if desktop_srgb {
            gl.disable(glow::FRAMEBUFFER_SRGB);
        }
        if let Some(index) = self
            .feed_cache
            .iter()
            .position(|cache| cache.key == key && (cache.width != width || cache.height != height))
        {
            self.feed_cache.remove(index).destroy(&gl);
        }
        if !self.feed_cache.iter().any(|cache| cache.key == key) {
            if self.feed_cache.len() >= 4 {
                self.feed_cache.remove(0).destroy(&gl);
            }
            if let Ok(cache) = CachedFeed::create(&gl, key, width, height) {
                self.feed_cache.push(cache);
            }
        }
        if let Some(index) = self.feed_cache.iter().position(|cache| cache.key == key) {
            let framebuffer = self.feed_cache[index].framebuffer;
            if feed_due(self.feed_cache[index].painted_ms, frame.now, count) {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
                gl.viewport(0, 0, width, height);
                gl.disable(glow::SCISSOR_TEST);
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear_depth_f32(1.0);
                gl.depth_mask(true);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                self.paint(frame, Some(framebuffer));
                self.feed_cache[index].painted_ms = Some(frame.now);
            }
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
            // A color-only target cannot enforce the physical display's depth.
            if surface_geometry.is_none() || draw_framebuffer_depth_bits(&gl) > 0 {
                gl.viewport(
                    viewport.left_px,
                    viewport.from_bottom_px,
                    viewport.width_px,
                    viewport.height_px,
                );
                let clip = info.clip_rect_in_pixels();
                gl.enable(glow::SCISSOR_TEST);
                gl.scissor(
                    clip.left_px,
                    clip.from_bottom_px,
                    clip.width_px,
                    clip.height_px,
                );
                gl.depth_range_f32(0.0, 1.0);
                gl.depth_mask(false);
                if surface_geometry.is_some() {
                    gl.enable(glow::DEPTH_TEST);
                    gl.depth_func(glow::LEQUAL);
                } else {
                    gl.disable(glow::DEPTH_TEST);
                }
                for capability in [
                    glow::BLEND,
                    glow::CULL_FACE,
                    glow::STENCIL_TEST,
                    glow::POLYGON_OFFSET_FILL,
                    glow::SAMPLE_ALPHA_TO_COVERAGE,
                ] {
                    gl.disable(capability);
                }
                #[cfg(not(target_arch = "wasm32"))]
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
                if desktop_srgb {
                    gl.disable(glow::FRAMEBUFFER_SRGB);
                }
                gl.color_mask(true, true, true, true);
                gl.use_program(Some(self.feed_program));
                gl.bind_vertex_array(Some(self.empty));
                gl.active_texture(glow::TEXTURE0);
                gl.bind_sampler(0, None);
                gl.bind_texture(glow::TEXTURE_2D, Some(self.feed_cache[index].texture));
                gl.uniform_1_i32(
                    gl.get_uniform_location(self.feed_program, "image").as_ref(),
                    0,
                );
                gl.uniform_1_i32(
                    gl.get_uniform_location(self.feed_program, "physical_surface")
                        .as_ref(),
                    i32::from(surface_geometry.is_some()),
                );
                if let Some((matrix, top_left, bottom_right)) = surface_geometry {
                    gl.uniform_matrix_4_f32_slice(
                        gl.get_uniform_location(self.feed_program, "local_to_clip")
                            .as_ref(),
                        false,
                        &matrix.to_cols_array(),
                    );
                    gl.uniform_3_f32(
                        gl.get_uniform_location(self.feed_program, "surface_top_left")
                            .as_ref(),
                        top_left.x,
                        top_left.y,
                        top_left.z,
                    );
                    gl.uniform_3_f32(
                        gl.get_uniform_location(self.feed_program, "surface_bottom_right")
                            .as_ref(),
                        bottom_right.x,
                        bottom_right.y,
                        bottom_right.z,
                    );
                }
                gl.draw_arrays(glow::TRIANGLES, 0, 6);
            }
        }

        gl.depth_mask(depth_mask);
        gl.depth_func(depth_func);
        gl.depth_range_f32(depth_range[0], depth_range[1]);
        gl.clear_color(
            clear_color[0],
            clear_color[1],
            clear_color[2],
            clear_color[3],
        );
        gl.clear_depth_f32(clear_depth);
        gl.color_mask(color_mask[0], color_mask[1], color_mask[2], color_mask[3]);
        gl.blend_func_separate(blend[0], blend[1], blend[2], blend[3]);
        gl.blend_equation_separate(equation[0], equation[1]);
        for (capability, enabled) in capabilities {
            if enabled {
                gl.enable(capability);
            } else {
                gl.disable(capability);
            }
        }
        if desktop_srgb {
            if srgb {
                gl.enable(glow::FRAMEBUFFER_SRGB);
            } else {
                gl.disable(glow::FRAMEBUFFER_SRGB);
            }
        }
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, draw);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
        gl.viewport(
            old_viewport[0],
            old_viewport[1],
            old_viewport[2],
            old_viewport[3],
        );
        gl.scissor(
            old_scissor[0],
            old_scissor[1],
            old_scissor[2],
            old_scissor[3],
        );
        gl.active_texture(glow::TEXTURE0);
        #[cfg(not(target_arch = "wasm32"))]
        {
            gl.use_program(objects.0);
            gl.bind_vertex_array(objects.1);
            gl.bind_buffer(glow::ARRAY_BUFFER, objects.2);
            gl.bind_renderbuffer(glow::RENDERBUFFER, objects.3);
            gl.bind_texture(glow::TEXTURE_2D, objects.4);
            gl.bind_sampler(0, objects.5);
            gl.polygon_mode(glow::FRONT_AND_BACK, polygon_mode);
        }
        #[cfg(target_arch = "wasm32")]
        {
            gl.use_program(None);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_renderbuffer(glow::RENDERBUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_sampler(0, None);
        }
        gl.active_texture(active_texture);
    }
    fn prepare_turrets(&mut self, frame: &Frame) {
        prepare_turrets(&mut self.turrets, frame);
    }
    unsafe fn paint(&mut self, frame: &Frame, callback_target: Option<glow::Framebuffer>) {
        if frame.scene.world_layout.validate().is_err() {
            return;
        }
        self.prepare_spatial_material();
        #[cfg(not(target_arch = "wasm32"))]
        self.debug_metrics.set(Default::default());
        let space = frame.space;
        self.prepare_turrets(frame);
        if self.destroyed {
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let callback_target = self
            .gl
            .get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING)
            .or(callback_target);
        #[cfg(not(target_arch = "wasm32"))]
        let profile = self.begin_view_profile(
            &if frame.drone_camera {
                frame
                    .camera_drone
                    .map_or_else(|| "drone:unbound".to_owned(), |id| format!("drone:{id}"))
            } else {
                "main".to_owned()
            },
            frame.now,
        );
        let mut visibility = ViewVisibility::default();
        let shadow_texture = self.paint_sun_shadow(
            frame,
            callback_target,
            frame.lighting_quality.shadow_size(frame.drone_camera),
        );
        let gl = &self.gl;
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        gl.disable(glow::DEPTH_TEST);
        gl.use_program(Some(self.background));
        gl.bind_vertex_array(Some(self.empty));
        gl.uniform_matrix_4_f32_slice(
            self.uniforms.sky_inverse.as_ref(),
            false,
            &frame.vp.inverse().to_cols_array(),
        );
        gl.uniform_3_f32(
            self.uniforms.sky_eye.as_ref(),
            frame.eye.x,
            frame.eye.y,
            frame.eye.z,
        );
        let camera_world = frame.space.world(frame.eye);
        let (density, up) = atmosphere_at(&frame.scene.world_layout, frame.system_id, camera_world);
        gl.uniform_1_f32(self.uniforms.atmosphere.as_ref(), density);
        gl.uniform_3_f32(self.uniforms.planet_up.as_ref(), up.x, up.y, up.z);
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(true);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        gl.use_program(Some(self.program));
        self.upload_spatial_material();
        self.local_lights.upload(
            gl,
            frame,
            &self.turrets,
            frame.lighting_quality.local_light_limit(frame.drone_camera),
        );
        gl.uniform_matrix_4_f32_slice(self.uniforms.vp.as_ref(), false, &frame.vp.to_cols_array());
        gl.uniform_3_f32(
            self.uniforms.eye.as_ref(),
            frame.eye.x,
            frame.eye.y,
            frame.eye.z,
        );
        gl.uniform_1_f32(self.uniforms.station_lighting.as_ref(), 0.0);
        gl.uniform_1_f32(self.uniforms.environment_atmosphere.as_ref(), density);
        gl.uniform_3_f32(self.uniforms.environment_up.as_ref(), up.x, up.y, up.z);
        self.paint_environment(frame);
        if space.scene(STATION_CENTER_POSITION).distance(frame.eye) < 1100.0 {
            let model = Mat4::from_translation(space.scene(STATION_CENTER_POSITION));
            for &(mesh, color, emission) in &self.station_batches {
                if self.visible_part(frame.vp, &(mesh, model, color, emission), &mut visibility) {
                    self.draw_material(mesh, model, color, emission, Material::BareMetal);
                }
            }
        }
        for asteroid in frame
            .scene
            .asteroids
            .iter()
            .filter(|a| space.scene(a.position).distance(frame.eye) < 1100.0)
        {
            let color = if asteroid.ore == 0 {
                Vec3::splat(0.10)
            } else {
                Vec3::new(0.36, 0.29, 0.18)
            };
            let model = asteroid_model(asteroid, space, frame.now);
            if !self.visible_part(frame.vp, &(3, model, color, 0.), &mut visibility) {
                continue;
            }
            self.draw_material(
                3,
                model,
                color,
                0.0,
                if asteroid.ore > 0 {
                    Material::Ore
                } else {
                    Material::Rock
                },
            );
        }
        for gate in frame
            .scene
            .gates
            .iter()
            .filter(|g| space.scene(g.position).distance(frame.eye) < 1100.0)
        {
            for model in gate_models(gate, space) {
                if !self.visible_part(frame.vp, &(1, model, Vec3::ONE, 0.), &mut visibility) {
                    continue;
                }
                self.draw_material(
                    1,
                    model,
                    Vec3::new(0.16, 0.43, 0.65),
                    0.7,
                    Material::BareMetal,
                );
            }
        }
        for pilot in &frame.scene.pilots {
            if space.position(pilot, frame.now).distance(frame.eye) > 1100.0 {
                continue;
            }
            let own = pilot.name == frame.own;
            let ship = transform(
                space.position(pilot, frame.now),
                Vec3::ONE,
                ship_orientation_at(pilot, frame.now),
            );
            if !frame.cockpit || !own {
                let exterior = exterior_visuals::for_pilot(frame, pilot);
                self.exterior_emission
                    .set(exterior.map_or_else(Default::default, |b| b.emission_power()));
                for &(mesh, model, color, emission) in cached_ship_parts(pilot.ship_class) {
                    if self.visible_part(
                        frame.vp,
                        &(mesh, ship * model, color, emission),
                        &mut visibility,
                    ) {
                        self.draw_material(
                            mesh,
                            ship * model,
                            color,
                            emission,
                            Material::BareMetal,
                        );
                    }
                }
            }
            self.exterior_emission.set(Default::default());
            for module_id in 0..2 {
                if let Some(state) = self.turrets.get(&(pilot.name.clone(), module_id)) {
                    for (mesh, model, color, emission) in
                        turret_parts(pilot.ship_class, state.rotation, module_id)
                    {
                        if self.visible_part(
                            frame.vp,
                            &(mesh, ship * model, color, emission),
                            &mut visibility,
                        ) {
                            self.draw(mesh, ship * model, color, emission);
                        }
                    }
                }
            }
        }
        for pilot in &frame.scene.pilots {
            for drone in fleet(pilot) {
                if frame.camera_drone == Some(drone.id) && pilot.name == frame.own {
                    continue;
                }
                let base = drone_model_transform(space, drone, frame.now);
                for &(mesh, local, color, emission) in
                    self.drone_parts[drone_models::class_index(drone.class)].iter()
                {
                    if self.visible_part(
                        frame.vp,
                        &(mesh, base * local, color, emission),
                        &mut visibility,
                    ) {
                        self.draw(mesh, base * local, color, emission);
                    }
                }
            }
        }
        if frame.connected {
            for pilot in &frame.scene.pilots {
                for drone in fleet(pilot) {
                    if drone.signal_strength <= 0.0
                        || (frame.camera_drone == Some(drone.id) && pilot.name == frame.own)
                    {
                        continue;
                    }
                    let start = space.scene(drone_present_position(drone, frame.now));
                    let beam = match &drone.mode {
                        arkline_protocol::DroneMode::Mining { target_id }
                            if drone.mining_cycle.is_some() =>
                        {
                            frame
                                .scene
                                .asteroids
                                .iter()
                                .find(|a| a.id == *target_id && a.ore > 0)
                                .map(|a| {
                                    let center = space.scene(a.position);
                                    let direction = (start - center).normalize_or_zero();
                                    (
                                        center + direction * a.radius as f32 / 100.0,
                                        Vec3::new(0.15, 0.95, 0.45),
                                    )
                                })
                        }
                        arkline_protocol::DroneMode::Attacking {
                            target_pilot,
                            target_drone_id,
                        } if frame.now % 1000 < 250 => frame
                            .scene
                            .pilots
                            .iter()
                            .find(|p| p.name == *target_pilot)
                            .and_then(|p| fleet(p).into_iter().find(|d| d.id == *target_drone_id))
                            .filter(|d| {
                                space.scene(d.position).distance(start)
                                    <= arkline_protocol::DRONE_ATTACK_RANGE as f32 / 100.0
                            })
                            .map(|d| (space.scene(d.position), Vec3::new(1.0, 0.18, 0.08))),
                        _ => None,
                    };
                    if let Some((end, color)) = beam {
                        if let Some(model) = segment_model(start, end, 0.009) {
                            self.draw(0, model, color, 1.2);
                        }
                    }
                }
            }
        }
        self.paint_living_world(frame);
        self.paint_effects(frame);
        if !frame.drone_camera {
            self.paint_warp_tunnel(frame);
        }
        if frame.cockpit {
            self.paint_cockpit(frame);
        }
        gl.disable(glow::DEPTH_TEST);
        gl.depth_mask(false);
        gl.bind_vertex_array(None);
        gl.use_program(None);
        self.restore_shadow_texture(shadow_texture);
        #[cfg(not(target_arch = "wasm32"))]
        self.finish_view_profile(profile, visibility);
    }
    /// Additive, depth-tested effects do not write depth or obscure UI meshes.
    unsafe fn paint_warp_tunnel(&self, frame: &Frame) {
        let Some(pilot) = frame.scene.pilots.iter().find(|p| p.name == frame.own) else {
            return;
        };
        let Some((phase, progress)) = super::motion::warp_phase(pilot, frame.now) else {
            return;
        };
        let intensity = warp_tunnel_intensity(phase, progress);
        if intensity <= 0.0 {
            return;
        }
        self.gl.enable(glow::BLEND);
        self.gl
            .blend_func_separate(glow::ONE, glow::ONE, glow::ZERO, glow::ONE);
        self.gl.depth_mask(false);
        let base = Mat4::from_rotation_translation(
            ship_orientation_at(pilot, frame.now),
            frame.space.position(pilot, frame.now),
        );
        // A ship-aligned optical transit volume. Bands flow from the forward
        // vanishing point past the observer; none are physical world objects.
        let clock = (frame.now % 6000) as f32 / 6000.0;
        for i in 0..7 {
            let travel = (clock * 3.0 + i as f32 / 7.0).fract();
            let x = 240.0 - travel * 480.0;
            let radius = 28.0 + 3.0 * (travel * std::f32::consts::TAU).sin();
            let alpha = intensity * (1.0 - (x / 260.0).abs()).max(0.0);
            self.draw(
                6,
                base * transform(
                    Vec3::X * x,
                    Vec3::splat(radius),
                    Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                ),
                Vec3::new(0.025, 0.07, 0.11) * alpha,
                1.3,
            );
        }
        for i in 0..96 {
            let seed = i as f32;
            let travel = (clock * 4.0 + seed * 0.61803).fract();
            let angle = seed * 2.39996 + clock * 0.15;
            let radius = 20.0 + (i % 9) as f32 * 2.4;
            let x = 280.0 - travel * 560.0;
            let start =
                base.transform_point3(Vec3::new(x, angle.sin() * radius, angle.cos() * radius));
            let end = base.transform_point3(Vec3::new(
                x + 12.0 + intensity * 26.0,
                angle.sin() * radius,
                angle.cos() * radius,
            ));
            if let Some(model) = segment_model(start, end, 0.045 + intensity * 0.04) {
                self.draw(0, model, Vec3::new(0.10, 0.28, 0.40) * intensity, 1.4);
            }
        }
        self.gl.depth_mask(true);
        self.gl.disable(glow::BLEND);
    }
    unsafe fn paint_cockpit(&self, frame: &Frame) {
        self.gl.depth_mask(true);
        self.gl.clear_depth_f32(1.0);
        self.gl.clear(glow::DEPTH_BUFFER_BIT);
        if let Some(occlusion) = &frame.cockpit_occlusion {
            self.cockpit_depth
                .paint_external(self, frame.cockpit_vp, occlusion);
        }
        self.gl.enable(glow::DEPTH_TEST);
        self.gl.depth_func(glow::LEQUAL);
        self.gl.disable(glow::BLEND);
        self.gl.uniform_matrix_4_f32_slice(
            self.uniforms.vp.as_ref(),
            false,
            &frame.cockpit_vp.to_cols_array(),
        );
        let origin = frame
            .scene
            .pilots
            .iter()
            .find(|pilot| pilot.name == frame.own)
            .map_or_else(
                || {
                    frame.eye
                        - frame.rotation * (cockpit_fit::eye_body_meters(frame.ship_class) / 100.0)
                },
                |pilot| frame.space.position(pilot, frame.now),
            );
        let base = cockpit_fit::mount(origin, frame.rotation, frame.ship_class);
        if frame.ship_class == ShipClass::Pioneer {
            let body = Mat4::from_rotation_translation(frame.rotation, origin);
            for &(mesh, model, color, emission) in cached_ship_parts(frame.ship_class) {
                if cockpit_fit::canopy_ranges(mesh).is_some_and(|ranges| ranges.is_empty()) {
                    continue;
                }
                self.draw_material(mesh, body * model, color, emission, Material::BareMetal);
            }
        }
        for &((mesh, model, color, emission), material) in cached_cockpit_parts(frame.ship_class) {
            self.draw_material(mesh, base * model, color, emission, material);
        }
    }
    unsafe fn paint_effects(&self, frame: &Frame) {
        let space = frame.space;
        let gl = &self.gl;
        gl.enable(glow::BLEND);
        gl.blend_func_separate(glow::ONE, glow::ONE, glow::ZERO, glow::ONE);
        gl.depth_mask(false);
        for pilot in &frame.scene.pilots {
            let ship_position = space.position(pilot, frame.now);
            if ship_position.distance(frame.eye) > 1100.0 {
                continue;
            }
            let spec = pilot.ship_class.spec();
            let ship = transform(
                ship_position,
                Vec3::new(
                    spec.length_m as f32 / 700.0,
                    spec.height_m as f32 / 200.0,
                    spec.width_m as f32 / 500.0,
                ),
                ship_orientation_at(pilot, frame.now),
            );
            let warp = super::motion::warp_phase(pilot, frame.now);
            if let Location::Jumping { arrives_at_ms, .. } = pilot.location {
                let progress = 1.0
                    - arrives_at_ms
                        .saturating_sub(frame.now)
                        .min(JUMP_DURATION_MS) as f32
                        / JUMP_DURATION_MS as f32;
                for offset in [0.0, 0.33, 0.66] {
                    let phase = (progress + offset).fract();
                    self.draw(
                        1,
                        transform(
                            ship_position,
                            Vec3::splat(2.0 + phase * 7.0),
                            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                        ),
                        Vec3::new(0.12, 0.45, 0.8) * (1.0 - phase),
                        1.5,
                    );
                }
            }

            if pilot.location != Location::Docked {
                let (length, intensity) = match warp {
                    Some(("WARP WARMUP", progress)) => (3.8 + progress * 4.5, 1.4 + progress),
                    Some(("WARP LAUNCH", progress)) => (5.0 + progress * 5.0, 2.0 + progress),
                    Some(("WARP ACCELERATING", _)) => (12.0, 3.2),
                    Some(("WARP CRUISE", _)) => (10.0, 2.7),
                    Some(("WARP DECELERATING", progress)) => {
                        (10.0 - progress * 6.0, 2.7 - progress)
                    }
                    Some(("WARP ARRIVAL", progress)) => (4.0 - progress * 3.0, 1.7 - progress),
                    _ => {
                        let forward = if frame.connected {
                            confirmed_body_acceleration(pilot, frame.now).x.max(0.0)
                        } else {
                            0.0
                        };
                        let thrust = (forward / arkline_protocol::SHIP_ACCELERATION as f32)
                            .clamp(0.0, 1.0)
                            .sqrt();
                        (thrust * 4.0, thrust * 1.8)
                    }
                };
                let pulse = 1.0
                    + 0.10
                        * ((frame.now % 260) as f32 / 260.0 * std::f32::consts::TAU
                            + lane(&pilot.name))
                        .sin();
                let effect_scale = Vec3::new(
                    spec.length_m as f32 / 700.0,
                    spec.height_m as f32 / 200.0,
                    spec.width_m as f32 / 500.0,
                );
                for mount in engine_mounts(pilot.ship_class)
                    .into_iter()
                    .filter(|_| intensity > 0.001)
                {
                    let mount = mount / effect_scale;
                    for (width, brightness) in [(0.58, 0.10), (0.31, 0.62), (0.16, 1.0)] {
                        self.draw(
                            2,
                            ship * transform(
                                mount - Vec3::X * (length * 0.5),
                                Vec3::new(length * 0.5 * pulse, width * pulse, width * pulse),
                                Quat::IDENTITY,
                            ),
                            Vec3::new(0.16, 0.48, 0.95) * brightness,
                            intensity,
                        );
                    }
                }
            }
            if frame.connected && warp.is_none() {
                let acceleration = confirmed_body_acceleration(pilot, frame.now);
                let hull = transform(
                    ship_position,
                    Vec3::ONE,
                    ship_orientation_at(pilot, frame.now),
                );
                for axis in 0..3 {
                    let component = acceleration[axis];
                    // Forward thrust already uses the modeled main engines.
                    if component.abs() < 0.01 || (axis == 0 && component > 0.0) {
                        continue;
                    }
                    let exhaust = -component.signum() * [Vec3::X, Vec3::Y, Vec3::Z][axis];
                    let index = axis * 2 + usize::from(exhaust[axis] < 0.0);
                    let Some(mount) = reaction_mounts(pilot.ship_class)[index] else {
                        continue;
                    };
                    let strength = (component.abs() / arkline_protocol::SHIP_ACCELERATION as f32)
                        .clamp(0.0, 1.0)
                        .sqrt();
                    let length = spec.length_m as f32 / 100.0 * (0.08 + strength * 0.16);
                    let radius = spec.width_m as f32 / 100.0 * 0.028;
                    for (width, brightness) in [(1.0, 0.13), (0.40, 0.85)] {
                        self.draw(
                            2,
                            hull * transform(
                                mount + exhaust * (length * 0.5),
                                Vec3::new(length * 0.5, radius * width, radius * width),
                                Quat::from_rotation_arc(Vec3::X, exhaust),
                            ),
                            Vec3::new(0.22, 0.58, 1.0) * brightness,
                            2.2 * strength,
                        );
                    }
                }
            }
            if let Some((stage, progress)) = warp {
                if stage == "WARP WARMUP" {
                    let radius = 3.3 + progress * 1.2;
                    self.draw(
                        6,
                        ship * transform(
                            Vec3::new(-1.0, 0.0, 0.0),
                            Vec3::splat(radius),
                            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                        ),
                        Vec3::new(0.02, 0.08, 0.12) * (0.2 + progress * 0.3),
                        0.4,
                    );
                }
                if stage != "WARP WARMUP" {
                    for i in 0..18 {
                        let phase = ((frame.now % 900) as f32 / 900.0 + i as f32 / 18.0).fract();
                        let alpha = (1.0 - phase).powi(2);
                        for side in [-1.0, 1.0] {
                            self.draw(
                                2,
                                ship * transform(
                                    Vec3::new(
                                        -3.5 - phase * 32.0,
                                        0.2 * (phase * 9.0).sin(),
                                        side * (1.5 + phase * 1.0),
                                    ),
                                    Vec3::new(1.0 + phase * 2.0, 0.22, 0.22),
                                    Quat::IDENTITY,
                                ),
                                Vec3::new(0.09, 0.35, 0.85) * alpha,
                                2.0,
                            );
                        }
                    }
                }
            }
            if frame.connected {
                for module_id in 0..2 {
                    let Some(cycle) = super::effects::module_cycle(pilot, module_id, frame.now)
                    else {
                        continue;
                    };
                    let Some(asteroid) = frame.scene.asteroids.iter().find(|a| {
                        a.id == cycle.target_id && super::effects::can_extract_target(pilot, a)
                    }) else {
                        continue;
                    };
                    let Some(turret) = self.turrets.get(&(pilot.name.clone(), module_id)) else {
                        continue;
                    };
                    let pose = transform(
                        ship_position,
                        Vec3::ONE,
                        ship_orientation_at(pilot, frame.now),
                    );
                    let source = pose.transform_point3(turret_muzzle(
                        pilot.ship_class,
                        turret.rotation,
                        module_id,
                    ));
                    let forward = pose.transform_vector3(turret.rotation * Vec3::X);
                    if (space.scene(asteroid.position) - source)
                        .normalize_or_zero()
                        .dot(forward)
                        < 0.995
                    {
                        continue;
                    }
                    let Some(impact) = mining_impact(source, asteroid, space, frame.now) else {
                        continue;
                    };
                    let pulse = 0.9
                        + 0.1 * ((frame.now % 400) as f32 / 400.0 * std::f32::consts::TAU).sin();
                    let cycle_brightness = 0.8 + 0.2 * cycle.progress;
                    let color = (if module_id == 0 {
                        Vec3::new(0.06, 0.66, 0.40)
                    } else {
                        Vec3::new(0.08, 0.45, 0.76)
                    }) * cycle_brightness;
                    if let Some(beam) = segment_model(source, impact, 0.006 * pulse) {
                        self.draw(0, beam, color, 1.8);
                    }
                    if let Some(beam) = segment_model(source, impact, 0.014 * pulse) {
                        self.draw(0, beam, color * 0.16, 0.6);
                    }
                    self.draw(
                        2,
                        transform(impact, Vec3::splat(0.035 * pulse), Quat::IDENTITY),
                        color,
                        1.6,
                    );
                    for i in 0..6 {
                        let phase =
                            ((frame.now % 1200) as f32 / 1200.0 + i as f32 * 0.61803).fract();
                        let toward = (source - impact).normalize_or_zero();
                        let spark = impact
                            + (toward
                                + Vec3::new((i as f32 * 2.4).cos(), (i as f32 * 1.7).sin(), 0.0))
                                * phase
                                * 0.15;
                        self.draw(
                            2,
                            transform(spark, Vec3::splat(0.005 * (1.0 - phase)), Quat::IDENTITY),
                            color * (1.0 - phase),
                            1.0,
                        );
                    }
                }
            }
            if pilot.name != frame.own {
                continue;
            }
            if let Some((progress, docking)) = frame
                .docking
                .filter(|_| matches!(pilot.location, Location::Docked | Location::Station))
            {
                let radius = if docking {
                    9.0 - progress * 5.0
                } else {
                    4.0 + progress * 7.0
                };
                let color = if docking {
                    Vec3::new(0.08, 0.70, 0.58)
                } else {
                    Vec3::new(0.70, 0.38, 0.10)
                };
                self.draw(
                    1,
                    transform(
                        ship_position - Vec3::Y * 1.2,
                        Vec3::new(radius, 0.55, radius),
                        Quat::IDENTITY,
                    ),
                    color * (1.0 - progress),
                    0.8,
                );
            }
        }
        gl.disable(glow::BLEND);
        gl.depth_mask(true);
    }
    unsafe fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        #[cfg(not(target_arch = "wasm32"))]
        self.view_profiler.borrow_mut().destroy(&self.gl);
        self.gl.delete_texture(self.shadow_fallback);
        for shadow in self.sun_shadows.drain(..) {
            shadow.destroy(&self.gl);
        }
        for cache in self.feed_cache.drain(..) {
            cache.destroy(&self.gl);
        }
        for mesh in &self.meshes {
            self.gl.delete_buffer(mesh.buffer);
            self.gl.delete_vertex_array(mesh.vao);
        }
        self.gl.delete_vertex_array(self.empty);
        self.gl.delete_program(self.program);
        self.gl.delete_program(self.background);
        self.cockpit_depth.destroy(&self.gl);
        self.gl.delete_program(self.feed_program);
    }
}
fn rebased_camera_target(target: Vec3, old: Space, new: Space, warp: bool) -> Vec3 {
    if warp {
        return Vec3::ZERO;
    }
    let target = target + new.scene(old.origin);
    if target.length() > 1200.0 {
        Vec3::ZERO
    } else {
        target
    }
}
fn warp_tunnel_intensity(phase: &str, progress: f32) -> f32 {
    let p = progress.clamp(0.0, 1.0);
    match phase {
        "WARP WARMUP" => 0.18 * p * p,
        "WARP LAUNCH" => 0.18 + 0.22 * p,
        "WARP ACCELERATING" => 0.4 + 0.6 * p,
        "WARP CRUISE" => 1.0,
        "WARP DECELERATING" => 1.0 - 0.85 * p,
        "WARP ARRIVAL" => 0.15 * (1.0 - p),
        _ => 0.0,
    }
}
fn update_head_look(angles: &mut [f32; 2], held: bool, axes: [f32; 2], dt: f32) {
    let dt = dt.clamp(0.0, 0.1);
    for (index, angle) in angles.iter_mut().enumerate() {
        if held {
            let limit = if index == 0 {
                std::f32::consts::PI
            } else {
                1.45
            };
            *angle = (*angle + axes[index] * dt * 1.4).clamp(-limit, limit);
        } else {
            *angle *= (-dt * 12.0).exp();
            if angle.abs() < 0.002 {
                *angle = 0.0;
            }
        }
    }
}
#[cfg(test)]
fn cockpit_camera(ship: Vec3, rotation: Quat) -> (Vec3, Mat4) {
    let eye = ship + rotation * Vec3::new(0.005, 0.015, 0.0);
    (
        eye,
        Mat4::look_at_rh(eye, eye + rotation * Vec3::X, rotation * Vec3::Y),
    )
}
/// Scene units are exactly 100 meters; the plane stays horizontal through the ship.
fn tactical_radii(distance: f32) -> Vec<f32> {
    let step = if distance < 100.0 {
        10.0
    } else if distance < 800.0 {
        50.0
    } else {
        250.0
    };
    let count = ((distance * 0.8 / step).ceil() as usize).clamp(2, 10);
    (1..=count).map(|i| i as f32 * step).collect()
}

fn tactical_ring_point(radius: f32, angle: f32) -> Vec3 {
    Vec3::new(radius * angle.cos(), 0.0, radius * angle.sin())
}

fn paint_tactical_overlay(
    painter: egui::Painter,
    vp: Mat4,
    rect: egui::Rect,
    distance: f32,
    selected: Option<Vec3>,
) {
    let project = |p| project_point(vp, rect, p);
    let line = |a, b, color| {
        if let (Some(a), Some(b)) = (project(a), project(b)) {
            painter.line_segment([a, b], egui::Stroke::new(1.0, color));
        }
    };
    let radii = tactical_radii(distance);
    let grid = super::TEAL.gamma_multiply(0.30);
    for &radius in &radii {
        // Fixed, bounded tessellation keeps cost independent of zoom and world extent.
        for segment in 0..128 {
            let angle = segment as f32 * std::f32::consts::TAU / 128.0;
            let next = (segment + 1) as f32 * std::f32::consts::TAU / 128.0;
            line(
                tactical_ring_point(radius, angle),
                tactical_ring_point(radius, next),
                grid,
            );
        }
        for angle in [
            0.0,
            std::f32::consts::PI,
            std::f32::consts::FRAC_PI_2,
            -std::f32::consts::FRAC_PI_2,
        ] {
            if let Some(at) = project(tactical_ring_point(radius, angle)) {
                painter.text(
                    at,
                    egui::Align2::CENTER_BOTTOM,
                    format!("{:.0} km", radius / 10.0),
                    egui::FontId::monospace(11.0),
                    super::TEAL,
                );
            }
        }
    }
    if let Some(&radius) = radii.last() {
        line(-Vec3::X * radius, Vec3::X * radius, grid);
        line(-Vec3::Z * radius, Vec3::Z * radius, grid);
    }
    if let Some(center) = project(Vec3::ZERO) {
        painter.circle_stroke(center, 5.0, egui::Stroke::new(1.5, super::TEAL));
        painter.text(
            center + egui::vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            "YOUR SHIP · 0 m",
            egui::FontId::monospace(10.0),
            super::TEAL,
        );
    }
    if let Some(target) = selected.filter(|p| p.is_finite()) {
        let foot = Vec3::new(target.x, 0.0, target.z);
        line(Vec3::ZERO, foot, super::AMBER.gamma_multiply(0.65));
        line(foot, target, super::AMBER);
        if let Some(at) = project(target) {
            painter.circle_stroke(at, 7.0, egui::Stroke::new(1.0, super::AMBER));
            painter.text(
                at + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                format!(
                    "{:.2} km center range · height {:+.2} km",
                    target.length() / 10.0,
                    target.y / 10.0
                ),
                egui::FontId::monospace(11.0),
                super::AMBER,
            );
        }
    }
}

fn project_point(vp: Mat4, rect: egui::Rect, point: Vec3) -> Option<egui::Pos2> {
    let clip = vp * point.extend(1.0);
    if !clip.is_finite() || clip.w <= 0.0 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if ndc.z.abs() > 1.0 {
        return None;
    }
    let screen =
        rect.center() + egui::vec2(ndc.x * rect.width() * 0.5, -ndc.y * rect.height() * 0.5);
    rect.contains(screen).then_some(screen)
}

/// A navigation ray intersects the horizontal plane through the real ship.
/// Horizon, behind-camera, outside-viewport and nonfinite targets are rejected.
fn unproject_to_plane(
    vp: Mat4,
    rect: egui::Rect,
    pointer: egui::Pos2,
    height: f32,
) -> Option<Vec3> {
    if !height.is_finite() {
        return None;
    }
    let (origin, direction) = screen_ray(vp, rect, pointer)?;
    if direction.y.abs() < 0.0001 {
        return None;
    }
    let distance = (height - origin.y) / direction.y;
    if distance <= 0.0 || distance > 10000.0 {
        return None;
    }
    let target = origin + direction * distance;
    (target.is_finite() && target.abs().max_element() <= (SPACE_BOUND / 100.0) as f32)
        .then_some(target)
}

fn screen_ray(vp: Mat4, rect: egui::Rect, pointer: egui::Pos2) -> Option<(Vec3, Vec3)> {
    if !rect.contains(pointer) || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return None;
    }
    let x = (pointer.x - rect.center().x) * 2.0 / rect.width();
    let y = (rect.center().y - pointer.y) * 2.0 / rect.height();
    // Invert in double precision to keep distant tactical picking stable.
    let inverse = glam::DMat4::from_cols_array(&vp.to_cols_array().map(f64::from)).inverse();
    let near = inverse * glam::DVec4::new(x as f64, y as f64, -1.0, 1.0);
    let far = inverse * glam::DVec4::new(x as f64, y as f64, 1.0, 1.0);
    if !near.is_finite() || !far.is_finite() || near.w.abs() < 1e-7 || far.w.abs() < 1e-7 {
        return None;
    }
    let origin = near.truncate() / near.w;
    let direction = (far.truncate() / far.w - origin).normalize_or_zero();
    (direction.is_finite() && direction.length_squared() > 0.5)
        .then_some((origin.as_vec3(), direction.as_vec3()))
}
fn transform(p: Vec3, scale: Vec3, rotation: Quat) -> Mat4 {
    Mat4::from_scale_rotation_translation(scale, rotation, p)
}
#[cfg(test)]
fn in_warp(location: &Location, now: u64) -> bool {
    matches!(location, Location::InTransit { arrives_at_ms, .. } if now < *arrives_at_ms)
}
/// Shared bezel geometry and instrument layout: +X forward, +Y up, +Z right.
const COCKPIT_FOV: f32 = 1.22;
fn cockpit_screens(class: ShipClass) -> [(Vec3, Vec3); 6] {
    let mut screens = [
        (Vec3::new(0.72, 0.12, 0.0), Vec3::new(0.020, 0.075, 0.18)),
        (Vec3::new(0.80, -0.23, -0.52), Vec3::new(0.012, 0.13, 0.18)),
        (Vec3::new(0.80, -0.23, 0.52), Vec3::new(0.012, 0.13, 0.18)),
        (Vec3::new(0.65, -0.23, 0.0), Vec3::new(0.020, 0.175, 0.27)),
        (Vec3::new(0.80, 0.02, -0.52), Vec3::new(0.012, 0.085, 0.18)),
        (Vec3::new(0.80, 0.02, 0.52), Vec3::new(0.012, 0.085, 0.18)),
    ];
    let tier = class.spec().mining_tier.unwrap_or(2) as f32;
    // Larger bridge desks spread their side displays; all screen faces still
    // share exact geometry with projection and interaction.
    for index in [1, 2, 4, 5] {
        screens[index].0.z *= 1.0 + (tier - 1.0) * 0.015;
    }
    screens[0].0.y = 0.10 + tier * 0.006;
    screens
}

#[cfg(test)]
fn drone_feed_rect(viewport: egui::Rect, class: ShipClass) -> egui::Rect {
    let (center, size) = cockpit_screens(class)[1];
    let screen = instrument_rect(viewport, center, size);
    screen.shrink(screen.width().min(screen.height()) * 0.05)
}

fn feed_resolution(width: i32, height: i32) -> (i32, i32) {
    let scale = (384.0 / width.max(height).max(1) as f32).min(1.0);
    (
        ((width as f32 * scale).round() as i32).max(1),
        ((height as f32 * scale).round() as i32).max(1),
    )
}
fn feed_due(previous: Option<u64>, now: u64, count: usize) -> bool {
    previous.is_none_or(|before| {
        now < before || now.saturating_sub(before) >= if count > 1 { 33 } else { 16 }
    })
}
fn drone_present_position(drone: &arkline_protocol::DroneView, now: u64) -> [f64; 3] {
    let seconds = now.saturating_sub(drone.updated_at_ms).min(350) as f64 / 1000.0;
    std::array::from_fn(|axis| drone.position[axis] + drone.velocity[axis] * seconds)
}
fn feed_tiles(screen: egui::Rect, count: usize) -> Vec<egui::Rect> {
    let count = count.min(4);
    let columns = if count > 1 { 2 } else { 1 };
    let rows = if count > 2 { 2 } else { 1 };
    (0..count)
        .map(|i| {
            let size = screen.size() / egui::vec2(columns as f32, rows as f32);
            egui::Rect::from_min_size(
                screen.min
                    + egui::vec2((i % columns) as f32 * size.x, (i / columns) as f32 * size.y),
                size,
            )
            .shrink(1.0)
        })
        .collect()
}
fn drone_mode_matrix(
    drone: &arkline_protocol::DroneView,
    space: Space,
    aspect: f32,
    mode: DroneCameraMode,
    now: u64,
) -> (Vec3, Mat4) {
    let mut sampled = drone.clone();
    sampled.position = drone_present_position(drone, now);
    if mode == DroneCameraMode::Pov {
        return drone_camera_matrix(&sampled, space, aspect);
    }
    if mode == DroneCameraMode::Orbit {
        return drone_view_matrix(&sampled, space, aspect, true, now);
    }
    let target = space.scene(sampled.position);
    let forward = Vec3::from_array(sampled.heading.map(|v| v as f32)).normalize_or_zero();
    let (eye, up) = if mode == DroneCameraMode::TopDown {
        (target + Vec3::Y * 1.5, Vec3::X)
    } else {
        (target + Vec3::Y * 0.55 - forward * 0.65, Vec3::Y)
    };
    (
        eye,
        Mat4::perspective_rh_gl(1.05, aspect.max(0.1), 0.002, 1200.0)
            * Mat4::look_at_rh(eye, target, up),
    )
}
fn drone_camera_matrix(
    drone: &arkline_protocol::DroneView,
    space: Space,
    aspect: f32,
) -> (Vec3, Mat4) {
    let eye = space.scene(drone.position);
    let direction = Vec3::from_array(drone.heading.map(|v| v as f32))
        .try_normalize()
        .unwrap_or(Vec3::X);
    let up = if direction.dot(Vec3::Y).abs() > 0.98 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let view = Mat4::look_at_rh(eye, eye + direction, up);
    (
        eye,
        Mat4::perspective_rh_gl(1.05, aspect.max(0.1), 0.002, 1200.0) * view,
    )
}

#[cfg(test)]
fn instrument_rect(viewport: egui::Rect, center: Vec3, size: Vec3) -> egui::Rect {
    // Body-fixed screens and camera share orientation. Project directly in seat
    // coordinates to avoid astronomical-world precision and use the actual face.
    let project = |y: f32, z: f32| {
        let depth = center.x - 0.025 - 0.006;
        let focal = 1.0 / (COCKPIT_FOV * 0.5).tan();
        viewport.center() + egui::vec2(z, -y) * (viewport.height() * 0.5 * focal / depth)
    };
    egui::Rect::from_two_pos(
        project(center.y + size.y - 0.008, center.z - size.z + 0.008),
        project(center.y - size.y + 0.008, center.z + size.z - 0.008),
    )
}

fn instrument_rows(
    own: &PilotView,
    snapshot: &Snapshot,
    target_distance: Option<f64>,
    now: u64,
    connected: bool,
) -> [Vec<String>; 3] {
    let rotation = ship_orientation_at(own, now);
    let velocity = own.flight.as_ref().map_or([0.0; 3], |f| {
        f.warp
            .as_ref()
            .map_or(f.velocity, |w| arkline_protocol::sample_warp(w, now).1)
    });
    let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
    let body = rotation.inverse() * Vec3::from_array(velocity.map(|v| v as f32));
    let forward = rotation * Vec3::X;
    let bearing = (forward.z.atan2(forward.x).to_degrees() + 360.0) % 360.0;
    let pitch = forward.y.clamp(-1.0, 1.0).asin().to_degrees();
    let connected = connected && now.saturating_sub(snapshot.server_time_ms) <= 1500;
    let status = if connected { "LIVE" } else { "STALE DATA" };
    let mode = match own.location {
        Location::Docked => "DOCKED".into(),
        Location::InTransit { arrives_at_ms, .. } => format!(
            "WARP ETA {:.1}s",
            arrives_at_ms.saturating_sub(now) as f64 / 1000.0
        ),
        Location::Jumping { arrives_at_ms, .. } => format!(
            "JUMP ETA {:.1}s",
            arrives_at_ms.saturating_sub(now) as f64 / 1000.0
        ),
        _ => own
            .flight
            .as_ref()
            .map_or("NO FLIGHT DATA", |f| match f.mode {
                NavigationMode::Stop => "STOP / BRAKING",
                NavigationMode::Manual => "MANUAL",
                NavigationMode::Approach { .. } => "APPROACH",
                NavigationMode::Orbit { .. } => "ORBIT",
                NavigationMode::KeepRange { .. } => "KEEP RANGE",
                NavigationMode::FlyTo { .. } => "FLY TO",
            })
            .into(),
    };
    let ships = snapshot
        .pilots
        .iter()
        .filter(|p| {
            p.system_id == own.system_id && p.name != own.name && p.location != Location::Docked
        })
        .count();
    let rocks = snapshot
        .asteroids
        .iter()
        .filter(|a| a.system_id == own.system_id)
        .count();
    let lock = if let Some(lock) = &own.target_lock {
        format!(
            "LOCKING #{} {:.1}s",
            lock.asteroid_id,
            lock.completes_at_ms.saturating_sub(now) as f64 / 1000.0
        )
    } else if let Some(id) = own.locked_asteroid {
        format!("LOCKED #{id}")
    } else {
        "NO TARGET LOCK".into()
    };
    [
        vec![
            format!("{} / {status}", own.ship_class.spec().name.to_uppercase()),
            mode,
            format!("SPEED {}", super::format_speed(speed)),
            format!("FWD {:+.1} m/s", body.x),
            format!("RIGHT {:+.1} m/s", body.z),
            format!("UP {:+.1} m/s", body.y),
            format!("HDG {bearing:05.1}  PITCH {pitch:+.1}"),
            own.surface
                .as_ref()
                .map_or("BODY-AXIS VELOCITY".into(), |s| {
                    format!(
                        "ALT {} / {}",
                        super::format_distance(s.altitude_m),
                        if s.grounded { "GROUNDED" } else { "FLIGHT" }
                    )
                }),
        ],
        vec![
            format!("NAV / {status}"),
            format!("SYSTEM {}", own.system_id),
            format!("SHIPS {ships}  ROCKS {rocks}"),
            "SERVER CONTACT LIST".into(),
            "SELECTED CENTER RANGE".into(),
            target_distance.map_or("NO SELECTION".into(), super::format_distance),
            lock,
            "RANGE / TRUE DISTANCE".into(),
        ],
        vec![
            format!("POWER / MODULES / {status}"),
            own.power
                .as_ref()
                .map_or("POWER DATA UNAVAILABLE".into(), |p| {
                    format!(
                        "CAP {:.1}/{:.1} kJ",
                        p.charge_j as f64 / 1000.0,
                        p.capacity_j as f64 / 1000.0
                    )
                }),
            own.power.as_ref().map_or("REACTOR UNKNOWN".into(), |p| {
                format!("REACTOR +{} W", p.regen_w)
            }),
            format!(
                "WARP COST {:.0} kJ",
                arkline_protocol::power_spec(own.ship_class).warp_cost_j as f64 / 1000.0
            ),
            module_instrument(own, 0, now),
            module_instrument(own, 1, now),
            format!(
                "HOLD {} / {}",
                own.ore,
                arkline_protocol::cargo_capacity(own.ship_class)
            ),
            own.power.as_ref().map_or("CAPACITOR UNKNOWN".into(), |p| {
                if p.charge_j < arkline_protocol::power_spec(own.ship_class).warp_cost_j {
                    "LOW POWER / WARP UNAVAILABLE".into()
                } else {
                    "CAPACITOR / CHARGED".into()
                }
            }),
        ],
    ]
}

fn paint_instruments(
    ui: &egui::Ui,
    surfaces: &[Option<CockpitSurface>; 6],
    own: &PilotView,
    snapshot: &Snapshot,
    target_distance: Option<f64>,
    now: u64,
    connected: bool,
) {
    let rows = instrument_rows(own, snapshot, target_distance, now, connected);
    let connected = connected && now.saturating_sub(snapshot.server_time_ms) <= 1500;
    let color = if connected { super::TEAL } else { super::AMBER };
    for index in 0..3 {
        let Some(surface) = surfaces[index].as_ref() else {
            continue;
        };
        let paint = surface.begin_paint(ui);
        let inner = surface.canvas_rect().shrink(5.);
        let painter = paint.painter().with_clip_rect(inner);
        if index != 0 {
            painter.rect_filled(inner, 0.0, egui::Color32::from_rgb(2, 14, 10));
        }
        let row_height = inner.height() / 10.0;
        let font = (inner.width() / 18.0).min(row_height * 0.75);
        for (row, text) in rows[index].iter().enumerate() {
            painter.text(
                inner.left_top() + egui::vec2(3.0, row_height * row as f32 + 2.0),
                egui::Align2::LEFT_TOP,
                text,
                egui::FontId::monospace(font),
                if row == 0 {
                    color
                } else {
                    egui::Color32::from_rgb(185, 226, 208)
                },
            );
        }
        let baseline = inner.top() + row_height * 8.6;
        painter.line_segment(
            [
                egui::pos2(inner.left(), baseline),
                egui::pos2(inner.right(), baseline),
            ],
            egui::Stroke::new(1.0, color.gamma_multiply(0.35)),
        );
        if index == 2 {
            let width = inner.width()
                * own.power.as_ref().map_or(0.0, |p| {
                    (p.charge_j as f32 / p.capacity_j.max(1) as f32).clamp(0.0, 1.0)
                });
            painter.rect_filled(
                egui::Rect::from_min_size(
                    egui::pos2(inner.left(), baseline + 3.0),
                    egui::vec2(width, row_height * 0.45),
                ),
                0.0,
                color,
            );
        }
    }
}

fn module_instrument(pilot: &PilotView, module_id: u8, now: u64) -> String {
    if let Some(cycle) = super::effects::module_cycle(pilot, module_id, now) {
        format!(
            "T{} #{} {:.1}s",
            module_id + 1,
            cycle.target_id,
            cycle.remaining_ms as f64 / 1000.0
        )
    } else {
        format!("T{} STANDBY", module_id + 1)
    }
}
struct TurretState {
    rotation: Quat,
    last_ms: u64,
}
fn slew_rotation(current: Quat, mut target: Quat, max_radians: f32) -> Quat {
    if current.dot(target) < 0.0 {
        target = -target;
    }
    let angle = 2.0 * current.dot(target).clamp(-1.0, 1.0).acos();
    if angle < 1e-6 {
        target
    } else {
        current
            .slerp(target, (max_radians.max(0.0) / angle).min(1.0))
            .normalize()
    }
}
fn turret_mount(class: ShipClass, module_id: u8) -> (Vec3, f32, f32) {
    let spec = class.spec();
    let radius = (spec.length_m as f32 * 0.012).clamp(0.6, 5.0) / 100.0;
    let barrel = (spec.length_m as f32 * 0.06).clamp(3.0, 24.0) / 100.0;
    (
        Vec3::new(
            spec.length_m as f32 * 0.0015,
            spec.height_m as f32 / 200.0 + radius,
            spec.width_m as f32 * 0.0007 * if module_id == 0 { -1.0 } else { 1.0 },
        ),
        radius,
        barrel,
    )
}
fn turret_muzzle(class: ShipClass, rotation: Quat, module_id: u8) -> Vec3 {
    let (mount, radius, barrel) = turret_mount(class, module_id);
    mount + rotation * Vec3::X * (radius + barrel)
}
/// Measured server acceleration, transformed into body axes; velocity alone
/// must never light a thrust plume while a ship is coasting.
fn confirmed_body_acceleration(pilot: &PilotView, now: u64) -> Vec3 {
    if !matches!(pilot.location, Location::Station | Location::Belt) {
        return Vec3::ZERO;
    }
    let Some(flight) = &pilot.flight else {
        return Vec3::ZERO;
    };
    if now.saturating_sub(flight.updated_at_ms) > 500
        || !flight.acceleration.iter().all(|a| a.is_finite())
    {
        return Vec3::ZERO;
    }
    ship_orientation_at(pilot, now).inverse()
        * Vec3::from_array(flight.acceleration.map(|v| v as f32))
}
/// Reaction nozzles sit on the actual outer hull mesh, cached once per class.
fn reaction_mounts(class: ShipClass) -> &'static [Option<Vec3>; 6] {
    static MOUNTS: std::sync::OnceLock<Vec<[Option<Vec3>; 6]>> = std::sync::OnceLock::new();
    let mounts = MOUNTS.get_or_init(|| {
        arkline_protocol::ALL_SHIP_CLASSES
            .iter()
            .map(|class| {
                let geometry = ship_geometry(*class, Mat4::IDENTITY);
                let distance = class
                    .spec()
                    .length_m
                    .max(class.spec().height_m)
                    .max(class.spec().width_m) as f32
                    / 50.0;
                std::array::from_fn(|index| {
                    let outward = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z][index];
                    let source = outward * distance;
                    geometry
                        .iter()
                        .filter_map(|(mesh, model)| ray_mesh_hit(source, -outward, *model, mesh))
                        .min_by(f32::total_cmp)
                        .map(|hit| source - outward * hit)
                })
            })
            .collect()
    });
    &mounts[arkline_protocol::ALL_SHIP_CLASSES
        .iter()
        .position(|c| *c == class)
        .unwrap()]
}
fn turret_deck(class: ShipClass, module_id: u8) -> f32 {
    static DECKS: std::sync::OnceLock<[f32; 14]> = std::sync::OnceLock::new();
    let decks = DECKS.get_or_init(|| {
        std::array::from_fn(|index| {
            let class = arkline_protocol::ALL_SHIP_CLASSES[index / 2];
            let module_id = (index % 2) as u8;
            let (mount, radius, _) = turret_mount(class, module_id);
            let origin = mount + Vec3::Y * radius;
            let distance = ship_geometry(class, Mat4::IDENTITY)
                .iter()
                .filter_map(|(mesh, model)| ray_mesh_hit(origin, -Vec3::Y, *model, mesh))
                .min_by(f32::total_cmp)
                .expect("mining turret pedestal must intersect its hull");
            origin.y - distance
        })
    });
    decks[(match class {
        ShipClass::Pioneer => 0,
        ShipClass::Prospector => 1,
        ShipClass::Hauler => 2,
        ShipClass::Vanguard => 3,
        ShipClass::Borealis => 4,
        ShipClass::Stratum => 5,
        ShipClass::Monolith => 6,
    }) * 2
        + usize::from(module_id.min(1))]
}
fn turret_parts(class: ShipClass, rotation: Quat, module_id: u8) -> Vec<ModelPart> {
    let (mount, radius, barrel) = turret_mount(class, module_id);
    let base = Mat4::from_rotation_translation(rotation, mount);
    let bottom = turret_deck(class, module_id) - radius * 0.3;
    let top = mount.y;
    let mut parts = vec![
        (
            8,
            base * transform(
                Vec3::ZERO,
                Vec3::new(radius, radius * 0.7, radius * 0.8),
                Quat::IDENTITY,
            ),
            Vec3::new(0.24, 0.29, 0.31),
            0.0,
        ),
        (
            8,
            base * transform(
                Vec3::X * (radius + barrel * 0.5),
                Vec3::new(barrel * 0.5, radius * 0.22, radius * 0.22),
                Quat::IDENTITY,
            ),
            Vec3::new(0.15, 0.20, 0.22),
            0.0,
        ),
        (
            0,
            base * transform(
                Vec3::X * (radius + barrel),
                Vec3::new(0.001, radius * 0.25, radius * 0.25),
                Quat::IDENTITY,
            ),
            Vec3::new(0.05, 0.4, 0.3),
            0.5,
        ),
        (
            0,
            transform(
                Vec3::new(mount.x, (bottom + top) * 0.5, mount.z),
                Vec3::new(radius * 0.7, (top - bottom) * 0.5, radius * 0.7),
                Quat::IDENTITY,
            ),
            Vec3::new(0.20, 0.25, 0.27),
            0.0,
        ),
    ];
    let pedestal = parts.pop().expect("turret pedestal");
    // Armor, paired coolant jackets and rear power pack follow the same aim
    // transform as the emitter; the muzzle and gameplay hardpoint stay unchanged.
    for side in [-1.0, 1.0] {
        parts.push((
            8,
            base * transform(
                Vec3::new(radius * 0.1, 0.0, side * radius * 0.77),
                Vec3::new(radius * 0.7, radius * 0.5, radius * 0.16),
                Quat::IDENTITY,
            ),
            Vec3::new(0.035, 0.24, 0.10),
            0.0,
        ));
        parts.push((
            8,
            base * transform(
                Vec3::new(radius + barrel * 0.45, 0.0, side * radius * 0.31),
                Vec3::new(barrel * 0.35, radius * 0.12, radius * 0.10),
                Quat::IDENTITY,
            ),
            Vec3::new(0.28, 0.32, 0.31),
            0.0,
        ));
    }
    parts.push((
        8,
        base * transform(
            Vec3::new(-radius * 0.8, radius * 0.15, 0.0),
            Vec3::new(radius * 0.4, radius * 0.6, radius * 0.6),
            Quat::IDENTITY,
        ),
        Vec3::new(0.065, 0.075, 0.072),
        0.0,
    ));
    parts.push(pedestal);
    parts
}
fn engine_mounts(class: ShipClass) -> Vec<Vec3> {
    if class == ShipClass::Pioneer {
        // Authored nozzle centers, in scene units. Material-batched lamps do not
        // imply thrust and must never become emitters at the model origin.
        return vec![
            Vec3::new(-0.359, 0.0, -0.125),
            Vec3::new(-0.359, 0.0, 0.125),
        ];
    }
    cached_ship_parts(class)
        .iter()
        .filter(|p| p.3 > 0.0)
        .map(|p| p.1.transform_point3(Vec3::ZERO))
        .collect()
}

fn ship_geometry(class: ShipClass, ship: Mat4) -> Vec<(Vec<Vertex>, Mat4)> {
    ship_parts(class)
        .into_iter()
        .map(|(mesh, model, _, _)| (mesh_vertices(mesh), ship * model))
        .collect()
}
// Mesh slots 0..10 remain stable for procedural world geometry. Embedded
// primitive slots precede station batching, and are shared by every render pass.
fn pioneer_asset() -> Result<&'static Vec<ship_asset::Primitive>, String> {
    static ASSET: std::sync::OnceLock<Result<Vec<ship_asset::Primitive>, String>> =
        std::sync::OnceLock::new();
    ASSET
        .get_or_init(|| {
            let parts =
                ship_asset::decode(include_bytes!("../../../assets/ships/pioneer/pioneer.glb"))?;
            let mut lo = Vec3::splat(f32::INFINITY);
            let mut hi = Vec3::splat(f32::NEG_INFINITY);
            for p in &parts {
                for v in &p.vertices {
                    lo = lo.min(v.p);
                    hi = hi.max(v.p);
                }
            }
            let half = Vec3::new(0.36, 0.09, 0.19);
            if (lo + half).abs().max_element() > 0.0003 || (hi - half).abs().max_element() > 0.0003
            {
                return Err(format!(
                    "Pioneer asset must retain 72×38×18 m hull envelope; got {lo:?}..{hi:?}"
                ));
            }
            Ok(parts)
        })
        .as_ref()
        .map_err(Clone::clone)
}
// Ship-only role tags preserve shared primitive geometry without tinting station parts.
const HULL_ROLE: usize = 1 << 24;
const TRIM_ROLE: usize = 2 << 24;
const EMISSION_ROLE: usize = 3 << 24;
fn mesh_index(mesh: usize) -> usize {
    mesh & 0x00ff_ffff
}
/// Validate expanded triangle-corner draws before measuring or submitting them.
fn triangle_draw_range(vertex_count: i32, range: Option<(usize, usize)>) -> Option<(i32, i32)> {
    let vertex_count = usize::try_from(vertex_count).ok()?;
    let (first, count) = range.unwrap_or((0, vertex_count));
    if count == 0
        || !first.is_multiple_of(3)
        || !count.is_multiple_of(3)
        || first.checked_add(count)? > vertex_count
    {
        return None;
    }
    Some((i32::try_from(first).ok()?, i32::try_from(count).ok()?))
}
fn paint_role(mesh: usize) -> usize {
    match mesh & !0x00ff_ffff {
        HULL_ROLE => HULL_ROLE,
        TRIM_ROLE => TRIM_ROLE,
        _ => match embedded_primitive(mesh).map(|p| p.material_name.as_str()) {
            Some("Graphite ceramic armor") => HULL_ROLE,
            Some("Emerald structural frames") => TRIM_ROLE,
            _ => 0,
        },
    }
}
fn preview_paint(mesh: usize) -> bool {
    paint_role(mesh) != 0
}
fn preview_material(
    mesh: usize,
    color: Vec3,
    emission: f32,
    preview: Option<crate::EmbeddedMaterialPreview>,
) -> (Vec3, f32) {
    let Some(preview) = preview else {
        return (color, emission);
    };
    let color = match paint_role(mesh) {
        HULL_ROLE => Vec3::from_array(preview.hull_rgb),
        TRIM_ROLE => Vec3::from_array(preview.trim_rgb),
        _ => color,
    };
    let emission = if embedded_primitive(mesh).is_some() || mesh & !0x00ff_ffff == EMISSION_ROLE {
        emission * preview.emissive_strength
    } else {
        emission
    };
    (color, emission)
}
fn docked_source_primitive(mesh: usize) -> Option<usize> {
    let mesh = mesh_index(mesh);
    if (10..19).contains(&mesh) {
        return Some(mesh - 10);
    }
    if mesh < 19 {
        return None;
    }
    let geometry = flight_geometry().ok()?;
    geometry
        .docked_meshes?
        .iter()
        .position(|&slot| slot == mesh)
        .map(|i| [0, 1, 6][i])
}
fn embedded_primitive(mesh: usize) -> Option<&'static ship_asset::Primitive> {
    pioneer_asset().ok()?.get(docked_source_primitive(mesh)?)
}
fn docked_pioneer_parts() -> Result<Arc<Vec<ModelPart>>, String> {
    flight_geometry()?.docked_parts.clone()
}
fn pioneer_asset_sha256() -> &'static str {
    use sha2::{Digest, Sha256};
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        hex::encode(Sha256::digest(include_bytes!(
            "../../../assets/ships/pioneer/pioneer.glb"
        )))
    })
}

type ModelPart = (usize, Mat4, Vec3, f32);
// One recipe supplies the live enclosure/console draws and native inspection.
// These coordinates precede the class-specific cockpit_fit mount.
fn cached_cockpit_parts(class: ShipClass) -> &'static [(ModelPart, Material)] {
    static MODELS: [std::sync::OnceLock<Vec<(ModelPart, Material)>>; 7] =
        [const { std::sync::OnceLock::new() }; 7];
    let index = arkline_protocol::ALL_SHIP_CLASSES
        .iter()
        .position(|c| *c == class)
        .unwrap();
    MODELS[index].get_or_init(|| {
        let mut parts = MaterialParts(Vec::new());
        let steel = Vec3::new(0.055, 0.08, 0.09);
        let glow = Vec3::new(0.08, 0.62, 0.43);
        // Forward is local +X; glazing is open between structural pillars.
        for side in [-1.0, 1.0] {
            if class != ShipClass::Pioneer {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(0.88, 0.03, side * cockpit_span(class)),
                        Vec3::new(0.025, 0.62, 0.018),
                        Quat::from_rotation_x(side * 0.22),
                    ),
                    steel,
                    0.0,
                );
            }
            parts.draw(
                0,
                transform(
                    Vec3::new(0.63, -0.38, side * 0.42),
                    Vec3::new(0.35, 0.15, 0.13),
                    Quat::IDENTITY,
                ),
                steel,
                0.0,
            );
            for i in 0..6 {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(0.77, -0.34, side * (0.22 + i as f32 * 0.04)),
                        Vec3::splat(0.013),
                        Quat::IDENTITY,
                    ),
                    glow,
                    0.8,
                );
            }
        }
        if class != ShipClass::Pioneer {
            parts.draw(
                0,
                transform(
                    Vec3::new(0.75, 0.57, 0.0),
                    Vec3::new(0.065, 0.035, 0.57),
                    Quat::IDENTITY,
                ),
                steel,
                0.0,
            );
            parts.draw(
                0,
                transform(
                    Vec3::new(0.82, -0.50, 0.0),
                    Vec3::new(0.22, 0.13, 0.58),
                    Quat::IDENTITY,
                ),
                steel,
                0.0,
            );
        }
        for (index, (center, size)) in cockpit_screens(class).into_iter().enumerate() {
            if index == 0 {
                continue;
            } // Transparent canopy telemetry preserves forward visibility.
            parts.draw(
                0,
                transform(center, size, Quat::IDENTITY),
                glow * 0.23,
                0.25,
            );
            parts.draw_material(
                0,
                transform(
                    center - Vec3::X * 0.025,
                    Vec3::new(0.006, size.y - 0.008, size.z - 0.008),
                    Quat::IDENTITY,
                ),
                Vec3::new(0.004, 0.012, 0.010),
                0.1,
                Material::Glass,
            );
        }
        parts.0
    })
}
struct MaterialParts(Vec<(ModelPart, Material)>);
impl MaterialParts {
    fn draw(&mut self, mesh: usize, model: Mat4, color: Vec3, emission: f32) {
        self.draw_material(mesh, model, color, emission, Material::PaintedHull);
    }
    fn draw_material(
        &mut self,
        mesh: usize,
        model: Mat4,
        color: Vec3,
        emission: f32,
        material: Material,
    ) {
        self.0.push(((mesh, model, color, emission), material));
    }
}

fn ship_parts(class: ShipClass) -> Vec<ModelPart> {
    cached_ship_parts(class).to_vec()
}
fn cached_ship_parts(class: ShipClass) -> &'static [ModelPart] {
    static MODELS: [std::sync::OnceLock<Vec<ModelPart>>; 7] =
        [const { std::sync::OnceLock::new() }; 7];
    MODELS[match class {
        ShipClass::Pioneer => 0,
        ShipClass::Prospector => 1,
        ShipClass::Hauler => 2,
        ShipClass::Vanguard => 3,
        ShipClass::Borealis => 4,
        ShipClass::Stratum => 5,
        ShipClass::Monolith => 6,
    }]
    .get_or_init(|| build_ship_parts(class))
}
fn build_ship_parts(class: ShipClass) -> Vec<ModelPart> {
    if class == ShipClass::Pioneer {
        return match pioneer_asset() {
            Ok(parts) => parts
                .iter()
                .enumerate()
                .map(|(i, p)| (10 + i, Mat4::IDENTITY, p.color, p.emission))
                .collect(),
            Err(_) => pioneer_construction_model(),
        };
    }

    // Original procedural interpretation of the construction-fleet dossiers.
    // Model-space proportions are fitted to measured public bounds below.
    let mut parts = Parts(Vec::new());
    let graphite = Vec3::new(0.075, 0.095, 0.105);
    let plate = Vec3::new(0.15, 0.18, 0.19);
    let edge = Vec3::new(0.24, 0.28, 0.29);
    let green = Vec3::new(0.025, 0.28, 0.15);
    let amber = Vec3::new(0.72, 0.39, 0.09);
    let glass = Vec3::new(0.04, 0.30, 0.27);
    let twin = matches!(class, ShipClass::Prospector | ShipClass::Borealis);
    let sections = match class {
        ShipClass::Pioneer => 3,
        ShipClass::Prospector => 3,
        ShipClass::Hauler => 5,
        ShipClass::Vanguard => 5,
        ShipClass::Borealis => 4,
        ShipClass::Stratum => 6,
        ShipClass::Monolith => 8,
    };
    let industrial = matches!(class, ShipClass::Stratum | ShipClass::Monolith);
    // Box helper takes half-extents, preserving the existing mesh convention.
    let mut block = |p: Vec3, size: Vec3, color: Vec3, emission: f32| {
        let mesh = if emission == 0.0 && size.min_element() > 0.06 {
            8
        } else {
            0
        };
        parts.draw(mesh, transform(p, size, Quat::IDENTITY), color, emission);
    };
    if twin {
        // Two long extraction booms and a rear bridge; an open central channel.
        for side in [-1.0, 1.0] {
            block(
                Vec3::new(0.0, 0.0, side * 0.72),
                Vec3::new(1.85, 0.32, 0.29),
                graphite,
                0.0,
            );
            block(
                Vec3::new(1.72, 0.03, side * 0.72),
                Vec3::new(0.24, 0.24, 0.23),
                green,
                0.0,
            );
        }
        block(
            Vec3::new(-1.05, 0.04, 0.0),
            Vec3::new(0.48, 0.30, 0.80),
            plate,
            0.0,
        );
        // Crossbeam under the unchanged inner turret pedestals.
        block(
            Vec3::new(0.65, -0.02, 0.0),
            Vec3::new(0.38, 0.13, 0.76),
            graphite,
            0.0,
        );
    } else {
        block(
            Vec3::new(0.0, -0.13, 0.0),
            Vec3::new(1.78, 0.16, 0.32),
            graphite,
            0.0,
        );
        block(
            Vec3::new(1.50, 0.02, 0.0),
            Vec3::new(0.34, 0.29, 0.48),
            plate,
            0.0,
        );
        block(
            Vec3::new(1.49, 0.29, 0.0),
            Vec3::new(0.23, 0.10, 0.38),
            green,
            0.0,
        );
    }
    // Broad non-emissive identification bands and structural longitudinal rails.
    for side in [-1.0, 1.0] {
        for y in [-0.34, 0.34] {
            block(
                Vec3::new(-0.08, y, side * 0.94),
                Vec3::new(1.74, 0.035, 0.045),
                edge,
                0.0,
            );
        }
        for x in [-1.58, 1.20] {
            block(
                Vec3::new(x, 0.0, side * 0.89),
                Vec3::new(0.11, 0.36, 0.13),
                green,
                0.0,
            );
        }
        for i in 0..sections {
            let x = -1.35 + i as f32 * 2.55 / sections as f32;
            let half = 0.92 / sections as f32;
            let z = if twin { 0.92 } else { 0.60 };
            block(
                Vec3::new(x, 0.0, side * z),
                Vec3::new(
                    half,
                    if industrial { 0.23 } else { 0.29 },
                    if twin { 0.11 } else { 0.28 },
                ),
                if class == ShipClass::Hauler {
                    plate
                } else {
                    graphite * 1.45
                },
                0.0,
            );
            block(
                Vec3::new(x - half, 0.0, side * (z + 0.12)),
                Vec3::new(0.018, 0.33, 0.035),
                edge,
                0.0,
            );
            // Alternate lamps leave a bounded geometry budget for cargo tops.
            if i % 2 == 0 {
                block(
                    Vec3::new(x, -0.27, side * (z + 0.12)),
                    Vec3::new(0.033, 0.012, 0.018),
                    amber,
                    0.45,
                );
            }
        }
        // Engine housings and recessed luminous exhaust faces; no drill shapes.
        block(
            Vec3::new(-1.94, -0.02, side * 0.59),
            Vec3::new(0.27, 0.26, 0.24),
            graphite,
            0.0,
        );
        block(
            Vec3::new(-2.215, -0.02, side * 0.59),
            Vec3::new(0.012, 0.17, 0.16),
            green,
            0.9,
        );
    }
    if !twin && class != ShipClass::Hauler {
        for i in 0..sections {
            let x = -1.35 + i as f32 * 2.55 / sections as f32;
            block(
                Vec3::new(x, if industrial { 0.15 } else { 0.22 }, 0.0),
                Vec3::new(
                    0.88 / sections as f32,
                    0.12,
                    if industrial { 0.48 } else { 0.40 },
                ),
                if i % 3 == 0 { green } else { plate },
                0.0,
            );
        }
    }
    let tower_x = if industrial {
        -1.18
    } else if twin {
        -0.85
    } else {
        0.86
    };
    let tower_levels = match class {
        ShipClass::Monolith => 3,
        ShipClass::Stratum | ShipClass::Vanguard => 2,
        _ => 1,
    };
    for level in 0..tower_levels {
        let y = 0.42 + level as f32 * 0.13;
        let scale = 1.0 - level as f32 * 0.18;
        block(
            Vec3::new(tower_x, y, 0.0),
            Vec3::new(0.34 * scale, 0.065, 0.33 * scale),
            graphite,
            0.0,
        );
        block(
            Vec3::new(tower_x + 0.345 * scale, y, 0.0),
            Vec3::new(0.008, 0.027, 0.25 * scale),
            glass,
            0.25,
        );
    }
    if class == ShipClass::Hauler {
        // Exposed top cargo cassettes differ from the extraction-boom silhouettes.
        for i in 0..4 {
            block(
                Vec3::new(-1.12 + i as f32 * 0.48, 0.35, 0.0),
                Vec3::new(0.20, 0.12, 0.52),
                plate,
                0.0,
            );
        }
    }
    if class == ShipClass::Vanguard {
        block(
            Vec3::new(-0.35, 0.39, 0.0),
            Vec3::new(1.1, 0.07, 0.31),
            green,
            0.0,
        );
    }
    // End of helper borrow: diagonal structural braces use rotated cube beams.
    if industrial || class == ShipClass::Borealis {
        for side in [-1.0, 1.0] {
            for i in 0..sections {
                let x = -1.40 + i as f32 * 2.60 / sections as f32;
                let a = Vec3::new(x, -0.28, side * if twin { 1.07 } else { 0.96 });
                let b = Vec3::new(
                    x + 2.35 / sections as f32,
                    0.28,
                    side * if twin { 1.07 } else { 0.96 },
                );
                let delta = b - a;
                parts.draw(
                    0,
                    transform(
                        (a + b) * 0.5,
                        Vec3::new(delta.length() * 0.5, 0.018, 0.022),
                        Quat::from_rotation_arc(Vec3::X, delta.normalize()),
                    ),
                    edge,
                    0.0,
                );
            }
        }
    }
    // Blunt armored brows: optical turrets remain the only extraction tools.
    for z in if twin { vec![-0.72, 0.72] } else { vec![0.0] } {
        parts.draw(
            0,
            transform(
                Vec3::new(1.71, 0.26, z),
                Vec3::new(0.23, 0.04, if twin { 0.22 } else { 0.54 }),
                Quat::from_rotation_z(-0.16),
            ),
            green,
            0.0,
        );
    }
    // Fit measured vertices, including all appendages, to the public meter spec.
    let (min, max) = parts_bounds(&parts.0);
    // Keep a solid service core at the fitted center for all six reaction axes.
    // Segmented cargo bays must not leave an axis nozzle firing from empty space.
    // Its lower edge joins the keel/crossbeam; all vertices stay inside the
    // measured envelope, preserving the public dimensions and turret supports.
    let center = (min + max) * 0.5;
    let bottom = -0.20;
    let top = center.y + 0.10;
    parts.draw(
        0,
        transform(
            Vec3::new(center.x, (bottom + top) * 0.5, center.z),
            Vec3::new(0.50, (top - bottom) * 0.5, 0.34),
            Quat::IDENTITY,
        ),
        graphite,
        0.0,
    );
    let spec = class.spec();
    let dimensions = Vec3::new(
        spec.length_m as f32,
        spec.height_m as f32,
        spec.width_m as f32,
    ) / 100.0;
    let fit =
        Mat4::from_scale(dimensions / (max - min)) * Mat4::from_translation(-(min + max) * 0.5);
    for (mesh, model, color, emission) in &mut parts.0 {
        *model = fit * *model;
        // Roles assigned inside the ship builder, never inferred from scene colors.
        if *emission > 0.0 {
            *mesh |= EMISSION_ROLE;
        } else if *emission == 0.0 {
            if *color == graphite || *color == plate {
                *mesh |= HULL_ROLE;
            } else if *color == green {
                *mesh |= TRIM_ROLE;
            }
        }
    }
    parts.0
}
/// Enclosed cutter silhouette derived from Pioneer's concept: sloped bridge,
/// shoulder armor, recessed service cassettes and rear drive housings.
/// Still a procedural exterior; no claim of fitted structural/interior geometry.
fn pioneer_construction_model() -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    let dark = Vec3::new(0.055, 0.070, 0.075);
    let steel = Vec3::new(0.15, 0.18, 0.19);
    let trim = Vec3::new(0.25, 0.28, 0.29);
    let green = Vec3::new(0.018, 0.20, 0.085);
    let mut part = |mesh, pos, size, color, emission| {
        p.draw(mesh, transform(pos, size, Quat::IDENTITY), color, emission)
    };
    part(9, Vec3::ZERO, Vec3::new(1.92, 0.42, 0.87), dark, 0.0);
    // Nose is a continuous faceted pressure-body reservation, not stacked crates.
    part(
        9,
        Vec3::new(1.20, 0.10, 0.0),
        Vec3::new(0.80, 0.37, 0.88),
        dark,
        0.0,
    );
    part(
        8,
        Vec3::new(1.992, 0.10, 0.0),
        Vec3::new(0.014, 0.19, 0.47),
        trim,
        0.0,
    );
    part(
        8,
        Vec3::new(2.010, 0.11, 0.0),
        Vec3::new(0.008, 0.15, 0.42),
        Vec3::new(0.025, 0.16, 0.17),
        0.0,
    );
    for z in [-0.22, 0.0, 0.22] {
        part(
            0,
            Vec3::new(2.020, 0.11, z),
            Vec3::new(0.006, 0.16, 0.008),
            trim,
            0.0,
        );
    }
    for side in [-1.0, 1.0] {
        // Armored green collars and inset side service panels.
        for x in [-1.40, 0.82] {
            part(
                8,
                Vec3::new(x, 0.0, side * 0.78),
                Vec3::new(0.22, 0.44, 0.21),
                green,
                0.0,
            );
            part(
                8,
                Vec3::new(x, 0.03, side * 0.988),
                Vec3::new(0.14, 0.29, 0.018),
                steel,
                0.0,
            );
            for y in [-0.21, 0.21] {
                part(
                    0,
                    Vec3::new(x, y, side * 1.01),
                    Vec3::new(0.09, 0.012, 0.012),
                    trim,
                    0.0,
                );
            }
        }
        for x in [-0.81, -0.18, 0.45] {
            part(
                8,
                Vec3::new(x, -0.015, side * 0.871),
                Vec3::new(0.285, 0.285, 0.055),
                steel,
                0.0,
            );
            part(
                8,
                Vec3::new(x, -0.015, side * 0.928),
                Vec3::new(0.24, 0.235, 0.014),
                dark,
                0.0,
            );
            part(
                0,
                Vec3::new(x, -0.292, side * 0.937),
                Vec3::new(0.17, 0.006, 0.010),
                Vec3::new(0.55, 0.30, 0.06),
                0.4,
            );
        }
        // Short service rails stay attached to the hull instead of framing empty air.
        for y in [-0.32, 0.32] {
            part(
                8,
                Vec3::new(-0.16, y, side * 0.92),
                Vec3::new(0.92, 0.018, 0.025),
                trim,
                0.0,
            );
        }
        part(
            8,
            Vec3::new(-1.85, -0.02, side * 0.55),
            Vec3::new(0.30, 0.28, 0.27),
            dark,
            0.0,
        );
        part(
            8,
            Vec3::new(-2.15, -0.02, side * 0.55),
            Vec3::new(0.016, 0.22, 0.21),
            steel,
            0.0,
        );
        part(
            8,
            Vec3::new(-2.17, -0.02, side * 0.55),
            Vec3::new(0.008, 0.16, 0.15),
            green,
            0.75,
        );
        part(
            8,
            Vec3::new(1.35, 0.36, side * 0.55),
            Vec3::new(0.33, 0.06, 0.19),
            green,
            0.0,
        );
        part(
            0,
            Vec3::new(1.72, -0.18, side * 0.57),
            Vec3::new(0.04, 0.012, 0.020),
            Vec3::new(0.50, 0.65, 0.63),
            0.7,
        );
    }
    for x in [-0.88, -0.25, 0.38] {
        part(
            8,
            Vec3::new(x, 0.423, 0.0),
            Vec3::new(0.285, 0.018, 0.57),
            steel,
            0.0,
        );
        part(
            8,
            Vec3::new(x, 0.444, 0.0),
            Vec3::new(0.235, 0.006, 0.48),
            dark,
            0.0,
        );
    }
    let (min, max) = parts_bounds(&p.0);
    let spec = ShipClass::Pioneer.spec();
    let extent = Vec3::new(
        spec.length_m as f32,
        spec.height_m as f32,
        spec.width_m as f32,
    ) / 100.0;
    let fit = Mat4::from_scale(extent / (max - min)) * Mat4::from_translation(-(min + max) * 0.5);
    for (_, m, _, _) in &mut p.0 {
        *m = fit * *m;
    }
    // Keep the emergency procedural model on the same editor finish path as
    // the authored GLB. The normal model receives these tags while importing
    // its named material roles; this fallback has no named materials.
    for (mesh, _, color, emission) in &mut p.0 {
        if *emission > 0.0 {
            *mesh |= EMISSION_ROLE;
        } else if *color == dark || *color == steel {
            *mesh |= HULL_ROLE;
        } else if *color == trim || *color == green {
            *mesh |= TRIM_ROLE;
        }
    }
    p.0
}
fn parts_bounds(parts: &[(usize, Mat4, Vec3, f32)]) -> (Vec3, Vec3) {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for (mesh, model, _, _) in parts {
        for v in mesh_vertices(*mesh) {
            let point = model.transform_point3(v.p);
            min = min.min(point);
            max = max.max(point);
        }
    }
    (min, max)
}
fn rock_mesh() -> Vec<Vertex> {
    cached_rock().to_vec()
}
fn cached_rock() -> &'static [Vertex] {
    static ROCK: std::sync::OnceLock<Vec<Vertex>> = std::sync::OnceLock::new();
    ROCK.get_or_init(|| {
        let mut mesh = sphere(true);
        let radius = mesh.iter().map(|v| v.p.length()).fold(0.0_f32, f32::max);
        for vertex in &mut mesh {
            vertex.p /= radius;
        }
        mesh
    })
}
/// Visual-only rigid rotation: all callers use the same presentation clock.
/// Server positions, conservative spherical range bounds and ore remain unchanged.
fn asteroid_rotation(id: u64, now: u64) -> Quat {
    let seed = id
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let period_ms = 240_000 + seed % 360_000; // One turn every four to ten minutes.
    let direction = if seed & 1 == 0 { 1.0 } else { -1.0 };
    // Reduce the epoch before float conversion to retain smooth frame-scale steps.
    let phase = (now % period_ms) as f64 / period_ms as f64 * std::f64::consts::TAU;
    let axis = Vec3::new(
        0.37 + (seed % 7) as f32 / 7.0,
        0.65,
        0.23 + (seed % 11) as f32 / 11.0,
    )
    .normalize();
    let f = (id % 10000) as f32;
    Quat::from_euler(glam::EulerRot::XYZ, f * 0.2, f * 0.4, f)
        * Quat::from_axis_angle(axis, phase as f32 * direction)
}
fn asteroid_model(asteroid: &Asteroid, space: Space, now: u64) -> Mat4 {
    transform(
        space.scene(asteroid.position),
        Vec3::splat(asteroid.radius as f32 / 100.0),
        asteroid_rotation(asteroid.id, now),
    )
}
fn gate_models(gate: &Stargate, space: Space) -> [Mat4; 2] {
    [-0.45, 0.45].map(|z| {
        transform(
            space.scene(gate.position) + Vec3::Z * z,
            Vec3::splat(gate.radius as f32 / 112.0),
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        )
    })
}
fn gate_parts(gate: &Stargate, space: Space) -> Vec<(Vec<Vertex>, Mat4)> {
    static TORUS: std::sync::OnceLock<Vec<Vertex>> = std::sync::OnceLock::new();
    let mesh = TORUS.get_or_init(|| torus(1.0, 0.12));
    gate_models(gate, space)
        .into_iter()
        .map(|model| (mesh.clone(), model))
        .collect()
}
fn mining_impact(source: Vec3, asteroid: &Asteroid, space: Space, now: u64) -> Option<Vec3> {
    let direction = (space.scene(asteroid.position) - source).normalize_or_zero();
    ray_mesh_hit(
        source,
        direction,
        asteroid_model(asteroid, space, now),
        cached_rock(),
    )
    .map(|d| source + direction * d)
}
/// World-distance intersection against the actual displayed triangles. Scaling
/// is retained in the local direction, so distances remain comparable across objects.
fn ray_mesh_hit(source: Vec3, direction: Vec3, model: Mat4, mesh: &[Vertex]) -> Option<f32> {
    let inverse_model = model.inverse();
    let origin = inverse_model.transform_point3(source);
    let direction = inverse_model.transform_vector3(direction);
    let mut nearest = f32::INFINITY;
    for triangle in mesh.chunks_exact(3) {
        let a = triangle[0].p;
        let edge1 = triangle[1].p - a;
        let edge2 = triangle[2].p - a;
        let h = direction.cross(edge2);
        let determinant = edge1.dot(h);
        if determinant.abs() < 1e-8 {
            continue;
        }
        let inverse = determinant.recip();
        let s = origin - a;
        let u = inverse * s.dot(h);
        if !(0.0..=1.0).contains(&u) {
            continue;
        }
        let q = s.cross(edge1);
        let v = inverse * direction.dot(q);
        if v < 0.0 || u + v > 1.0 {
            continue;
        }
        let distance = inverse * edge2.dot(q);
        if distance > 0.0 {
            nearest = nearest.min(distance);
        }
    }
    nearest.is_finite().then_some(nearest)
}
struct Parts(Vec<(usize, Mat4, Vec3, f32)>);
impl Parts {
    fn draw(&mut self, mesh: usize, model: Mat4, color: Vec3, emission: f32) {
        self.0.push((mesh, model, color, emission));
    }
}
fn mesh_vertices(mesh: usize) -> Vec<Vertex> {
    if mesh_index(mesh) > hangar::station_floor_mesh_index() {
        if let Ok(geometry) = flight_geometry() {
            if let Some(vertices) = geometry.catalog.uploads().get(mesh_index(mesh)) {
                return vertices.to_vec();
            }
        }
    }
    if mesh_index(mesh) == hangar::station_floor_mesh_index() {
        return hangar::station_floor_vertices();
    }
    if let Some(primitive) = embedded_primitive(mesh) {
        return primitive.vertices.clone();
    }
    match mesh_index(mesh) {
        0 => cube(),
        1 => torus(1.0, 0.12),
        2 => sphere(false),
        8 => armor_block(),
        9 => industrial_shell(),
        _ => hull(),
    }
}
fn station_batches_geometry() -> Vec<(Vec<Vertex>, Vec3, f32)> {
    let parts = station_parts(Space {
        origin: STATION_CENTER_POSITION,
    });
    let mut groups = std::collections::BTreeMap::<[u32; 4], (Vec<Vertex>, Vec3, f32)>::new();
    for (mesh, model, color, emission) in parts {
        let normal = model.inverse().transpose();
        let entry = groups
            .entry([
                color.x.to_bits(),
                color.y.to_bits(),
                color.z.to_bits(),
                emission.to_bits(),
            ])
            .or_insert((Vec::new(), color, emission));
        entry
            .0
            .extend(mesh_vertices(mesh).into_iter().map(|v| Vertex {
                p: model.transform_point3(v.p),
                n: normal.transform_vector3(v.n).normalize(),
            }));
    }
    groups.into_values().collect()
}
fn station_parts(space: Space) -> Vec<(usize, Mat4, Vec3, f32)> {
    let mut parts = Parts(Vec::new());
    let center = Mat4::from_translation(space.scene(STATION_CENTER_POSITION));
    let steel = Vec3::new(0.19, 0.25, 0.29);
    let window = Vec3::new(0.14, 0.43, 0.37);
    let radius = STATION_DIAMETER_M as f32 / 200.0;
    let half_height = STATION_HEIGHT_M as f32 / 200.0;
    parts.draw(
        0,
        center * Mat4::from_scale(Vec3::new(11.0, half_height, 11.0)),
        steel,
        0.0,
    );
    // Habitat rings: the large rings sit above/below the docking plane. Their
    // 1.12 torus extent exactly matches the advertised twenty-kilometer span.
    for (y, r) in [(-15.0, radius / 1.12), (0.0, 68.0), (15.0, radius / 1.12)] {
        parts.draw(
            1,
            center * transform(Vec3::new(0.0, y, 0.0), Vec3::new(r, 5.0, r), Quat::IDENTITY),
            steel,
            0.0,
        );
        for i in 0..32 {
            let angle = i as f32 * std::f32::consts::TAU / 32.0;
            let rotation = Quat::from_rotation_y(angle);
            let p = rotation * Vec3::new(r, 0.0, 0.0) + Vec3::Y * (y + 0.7);
            parts.draw(
                0,
                center * transform(p, Vec3::new(1.5, 0.12, 0.4), rotation),
                window,
                0.55,
            );
        }
        for i in 0..8 {
            let rotation = Quat::from_rotation_y(i as f32 * std::f32::consts::TAU / 8.0);
            parts.draw(
                0,
                center
                    * transform(
                        rotation * Vec3::new(r * 0.52, y, 0.0),
                        Vec3::new(r * 0.44, 0.65, 1.0),
                        rotation,
                    ),
                steel * 0.8,
                0.0,
            );
        }
    }
    for i in 0..8 {
        let y = -24.0 + i as f32 * 6.5;
        parts.draw(
            1,
            center * transform(Vec3::Y * y, Vec3::new(22.0, 3.0, 22.0), Quat::IDENTITY),
            steel * 1.15,
            0.0,
        );
        for side in [-1.0, 1.0] {
            parts.draw(
                0,
                center
                    * transform(
                        Vec3::new(0.0, y, side * 24.0),
                        Vec3::new(5.0, 1.7, 4.0),
                        Quat::IDENTITY,
                    ),
                steel,
                0.0,
            );
        }
    }
    // Recessed +Z-facing port. Nothing occupies the authoritative berth volume
    // x/y +/-450m, z+1000m around STATION_POSITION, or the docked port point.
    let port = Mat4::from_translation(space.scene(STATION_POSITION));
    for y in [-5.5, 5.5] {
        parts.draw(
            0,
            port * transform(
                Vec3::new(-6.0, y, -6.0),
                Vec3::new(6.0, 0.4, 6.0),
                Quat::IDENTITY,
            ),
            steel,
            0.0,
        );
        parts.draw(
            0,
            port * transform(
                Vec3::new(-5.8, y, 0.1),
                Vec3::new(5.5, 0.07, 0.07),
                Quat::IDENTITY,
            ),
            window,
            0.8,
        );
    }
    parts.draw(
        0,
        port * transform(
            Vec3::new(-12.0, 0.0, -6.0),
            Vec3::new(0.4, 5.5, 6.0),
            Quat::IDENTITY,
        ),
        steel,
        0.0,
    );
    for i in 0..8 {
        parts.draw(
            0,
            port * transform(
                Vec3::new(-1.0 - i as f32 * 1.3, -5.0, -2.0),
                Vec3::new(0.1, 0.1, 1.3),
                Quat::IDENTITY,
            ),
            Vec3::new(0.48, 0.32, 0.07),
            0.4,
        );
    }
    parts.0
}
fn segment_model(start: Vec3, end: Vec3, radius: f32) -> Option<Mat4> {
    let delta = end - start;
    let length = delta.length();
    if !length.is_finite() || length < 0.0001 || radius <= 0.0 {
        return None;
    }
    Some(transform(
        (start + end) * 0.5,
        Vec3::new(length * 0.5, radius, radius),
        Quat::from_rotation_arc(Vec3::X, delta / length),
    ))
}
unsafe fn compile(
    gl: &glow::Context,
    vertex: &str,
    fragment: &str,
) -> Result<glow::Program, String> {
    let program = gl.create_program()?;
    for (kind, source) in [
        (glow::VERTEX_SHADER, vertex),
        (glow::FRAGMENT_SHADER, fragment),
    ] {
        let shader = gl.create_shader(kind)?;
        let source = if gl.version().is_embedded {
            source.replacen(
                "#version 150",
                "#version 300 es\nprecision highp float;\nprecision highp int;",
                1,
            )
        } else {
            source.to_owned()
        };
        gl.shader_source(shader, &source);
        gl.compile_shader(shader);
        if !gl.get_shader_compile_status(shader) {
            let error = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            gl.delete_program(program);
            return Err(error);
        }
        gl.attach_shader(program, shader);
        gl.delete_shader(shader);
    }
    gl.bind_attrib_location(program, 0, "position");
    gl.bind_attrib_location(program, 1, "normal");
    gl.link_program(program);
    if !gl.get_program_link_status(program) {
        let error = gl.get_program_info_log(program);
        gl.delete_program(program);
        return Err(error);
    }
    Ok(program)
}
const VERTEX: &str = r#"#version 150
in vec3 position;in vec3 normal;uniform mat4 model;uniform mat4 view_projection;
out vec3 world;out vec3 surface_normal;out vec3 local_position;
void main(){local_position=position;vec4 p=model*vec4(position,1.0);world=p.xyz;surface_normal=transpose(inverse(mat3(model)))*normal;gl_Position=view_projection*p;}
"#;
fn mesh_fragment_prefix() -> String {
    format!(
        "#version 150\n{}{}{}{}",
        sun_shadows::GLSL,
        include_str!("shaders/sky.glsl"),
        include_str!("shaders/materials.glsl"),
        include_str!("shaders/local_lighting.glsl")
    )
}
const AUTHORED_MATERIAL_FUNCTION: &str = "struct ArkSurface{vec3 base_color;float roughness;float metallic;vec3 emission;};ArkSurface ark_graph_surface(vec3 body_meters,ArkSurface authored_surface){return authored_surface;}\n";
const FRAGMENT_BODY: &str = r#"
in vec3 world;in vec3 surface_normal;in vec3 local_position;
uniform float surface_opacity;uniform int ark_graph_enabled;uniform vec4 material_params;uniform float hull_surface;uniform float station_lighting;uniform float environment_atmosphere;uniform vec3 environment_up;
uniform vec3 planet_center;uniform vec3 base_color;uniform vec3 eye;uniform float emission;uniform float emission_power;uniform vec4 emission_override;uniform vec3 emissive_color;uniform int material_kind;out vec4 color;
float ark_terrain(vec3 p){return ark_noise3(p)*0.53+ark_noise3(p*2.07+13.8)*0.27+ark_noise3(p*4.13-7.1)*0.13+ark_noise3(p*8.41+29.4)*0.07;}
void main(){vec3 n=normalize(surface_normal);if(!gl_FrontFacing)n=-n;vec3 l=normalize(vec3(-0.5,0.8,0.65));vec3 v=normalize(eye-world);
float diffuse=max(dot(n,l),0.0);float facing=max(dot(n,v),0.0);float rim=pow(1.0-facing,3.0);
vec3 lit;
if(material_kind==1 || material_kind==2){
    vec3 sphere=material_kind==1 ? normalize(local_position) : normalize(world-planet_center);
    float elevation=ark_terrain(sphere*4.1+vec3(3.2,1.7,9.4));
    float land=smoothstep(0.47,0.51,elevation);
    float aridity=ark_terrain(sphere*9.0+21.5);
    vec3 ocean=mix(vec3(0.018,0.055,0.115),vec3(0.035,0.19,0.22),smoothstep(0.37,0.49,elevation));
    vec3 earth=mix(vec3(0.10,0.18,0.105),vec3(0.36,0.28,0.17),smoothstep(0.42,0.62,aridity));
    float polar=smoothstep(0.79,0.94,abs(sphere.y)+ark_terrain(sphere*14.0)*0.09);
    vec3 surface=mix(mix(ocean,earth,land),vec3(0.73,0.82,0.85),polar);
    vec3 wind=sphere*15.0+vec3(sin(sphere.y*11.0)*0.9,0.0,cos(sphere.y*8.0)*0.7);
    float clouds=smoothstep(0.52,0.68,ark_terrain(wind))*smoothstep(10.0,300.0,length(eye-world));
    float detail=(ark_terrain((world-planet_center)*0.9)-0.5)*0.20;
    if(material_kind==2) surface*=1.0+detail*(1.0-smoothstep(100.0,900.0,length(eye-world)));
    surface=mix(surface,vec3(0.76,0.83,0.88),clouds*0.88);
    lit=surface*(0.10+diffuse*1.22);
    lit+=vec3(0.46,0.64,0.82)*pow(max(dot(n,normalize(l+v)),0.0),95.0)*(1.0-land)*(1.0-clouds)*0.5;
    lit+=vec3(0.11,0.36,0.75)*pow(1.0-facing,4.0)*(0.23+diffuse*0.85);
}else{
    vec3 albedo=max(base_color,vec3(0.0));
    float roughness=material_params.x,metallic=material_params.y;
    vec3 emitted=max(emissive_color,vec3(0.0))*max(emission,0.0);
    if(ark_graph_enabled!=0){
        ArkSurface authored=ArkSurface(albedo,roughness,metallic,emitted);
        ArkSurface surface=ark_graph_surface(local_position*100.0,authored);
        albedo=surface.base_color;roughness=surface.roughness;metallic=surface.metallic;emitted=surface.emission;
    }
    if(material_params.z>0.5){
        float vein=1.0-smoothstep(0.045,0.11,abs(ark_noise3(local_position*5.0)-0.52));
        metallic=mix(0.0,0.82,vein);roughness=mix(0.90,0.34,vein);
        albedo=mix(albedo,vec3(0.48,0.38,0.22),vein*0.72);
    }
    if(hull_surface>0.5){ark_hull_surface(local_position*100.0,albedo,roughness,metallic);}
    // Existing screen panels use a dark dielectric backing; no canopy transmission is invented.
    if(material_params.w>0.5) albedo*=0.70;
    if(station_lighting>0.5){
        lit=ark_station_environment(n,v,albedo,roughness,metallic);
    }else{
        lit=ark_brdf(n,v,l,albedo,roughness,metallic)*vec3(3.8,3.45,2.95)*ark_sun_visibility(world,n,l);
        lit+=ark_environment(n,v,albedo,roughness,metallic,environment_atmosphere,environment_up);
    }
    lit+=ark_local_lighting(world,n,v,albedo,roughness,metallic);
    if(emission_override.a>0.5) emitted=max(emission_override.rgb,vec3(0.0));
    lit+=emitted*emission_power;
}
color=vec4(ark_display(lit),surface_opacity>0.0 ? surface_opacity : 1.0);}
"#;
const BACK_VERTEX: &str = r#"#version 150
out vec2 uv;void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);uv=p;gl_Position=vec4(p*2.0-1.0,0.999,1.0);}
"#;
const FEED_VERTEX: &str = r#"#version 150
uniform bool physical_surface;
uniform mat4 local_to_clip;
uniform vec3 surface_top_left;
uniform vec3 surface_bottom_right;
out vec2 uv;
void main() {
    vec2 corners[6] = vec2[6](
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0));
    vec2 point = corners[gl_VertexID];
    // Cached color attachments use GL bottom-up UVs; display canvases use Y down.
    uv = vec2(point.x, 1.0 - point.y);
    if (physical_surface) {
        vec3 local = vec3(surface_top_left.x,
            mix(surface_top_left.y, surface_bottom_right.y, point.y),
            mix(surface_top_left.z, surface_bottom_right.z, point.x));
        gl_Position = local_to_clip * vec4(local, 1.0);
    } else {
        gl_Position = vec4(point.x * 2.0 - 1.0, 1.0 - point.y * 2.0, 0.0, 1.0);
    }
}
"#;
const FEED_FRAGMENT: &str = r#"#version 150
in vec2 uv;
uniform sampler2D image;
out vec4 color;
void main() { color = texture(image, uv); }
"#;

const BACK_FRAGMENT: &str = concat!(
    "#version 150\n",
    include_str!("shaders/sky.glsl"),
    r#"
in vec2 uv;uniform float atmosphere;uniform vec3 planet_up;uniform mat4 inverse_view_projection;uniform vec3 camera_eye;out vec4 color;
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);}
void main(){vec4 farpoint=inverse_view_projection*vec4(uv*2.0-1.0,1.0,1.0);vec3 direction=normalize(farpoint.xyz/farpoint.w-camera_eye);vec2 p=vec2(atan(direction.z,direction.x)/6.2831853+0.5,asin(clamp(direction.y,-1.0,1.0))/3.1415927+0.5);float mist=noise(p*4.0)+noise(p*9.0)*0.5+noise(p*23.0)*0.18;
float band=exp(-pow((p.y-0.4+sin(p.x*3.4)*0.16)*3.0,2.0));vec3 c=ark_space_radiance(direction)+vec3(0.015,0.025,0.045)*mist*band;
c+=vec3(0.055,0.022,0.055)*pow(mist*0.6,3.0)*band;
for(int i=0;i<2;i++){float scale=180.0+float(i)*155.0;vec2 grid=p*scale;vec2 cell=floor(grid);vec2 offset=vec2(hash(cell),hash(cell+13.7));float d=length(fract(grid)-offset);float seed=hash(cell+71.1);float star=(1.0-smoothstep(0.0,0.09,d))*step(0.972,seed);c+=mix(vec3(0.53,0.7,1.0),vec3(1.0,0.87,0.65),seed)*star*(0.5+seed*0.7);}
float sunFacing=max(dot(direction,normalize(vec3(-0.5,0.8,0.65))),0.0);float flare=pow(sunFacing,64.0)*0.15+pow(sunFacing,800.0)*0.45+smoothstep(0.99982,0.99996,sunFacing)*1.8;c+=vec3(1.0,0.88,0.70)*flare;float horizon=pow(1.0-abs(dot(direction,planet_up)),3.0);
vec3 sky=mix(vec3(0.065,0.17,0.29),vec3(0.44,0.55,0.60),horizon);
c=mix(c,sky,atmosphere);color=vec4(ark_display(c),1.0);}
"#
);

#[cfg(test)]
mod tests {
    fn frame_for_scene_clone_test() -> Frame {
        let vp = Mat4::IDENTITY;
        Frame {
            exterior: None,
            lighting_quality: Default::default(),
            cockpit_vp: vp,
            cockpit_occlusion: None,
            snapshot_time: 1000,
            planet_vp: vp,
            system_id: 3,
            ship_class: ShipClass::Pioneer,
            drone_camera: false,
            camera_drone: None,
            scene: Arc::new(FrameScene {
                world_layout: arkline_protocol::legacy_world_layout(),
                traffic: vec![],
                surface_props: vec![],
                asteroids: vec![],
                gates: vec![],
                pilots: vec![],
            }),
            space: Space { origin: [0.; 3] },
            cockpit: true,
            rotation: Quat::IDENTITY,
            connected: true,
            vp,
            eye: Vec3::ZERO,
            now: 1000,
            own: "fixture".into(),
            docking: None,
        }
    }

    #[test]
    fn camera_frame_clones_share_scene_and_keep_camera_overrides_local() {
        let mut original = frame_for_scene_clone_test();
        Arc::get_mut(&mut original.scene).unwrap().world_layout =
            arkline_protocol::SceneLayoutRevision::OrbitalV1.definition();
        let mut feed = original.clone();
        assert!(Arc::ptr_eq(&original.scene, &feed.scene));
        assert_eq!(
            feed.scene.world_layout,
            arkline_protocol::SceneLayoutRevision::OrbitalV1.definition()
        );

        feed.drone_camera = true;
        feed.camera_drone = Some(42);
        feed.eye = Vec3::X;
        feed.vp = Mat4::from_translation(Vec3::ONE);

        assert!(!original.drone_camera);
        assert_eq!(original.camera_drone, None);
        assert_eq!(original.eye, Vec3::ZERO);
        assert_eq!(original.vp, Mat4::IDENTITY);
        assert_eq!(feed.camera_drone, Some(42));
        assert_eq!(feed.eye, Vec3::X);
        assert_eq!(feed.vp, Mat4::from_translation(Vec3::ONE));
    }

    #[test]
    fn procedural_pioneer_fallback_keeps_editor_finish_roles() {
        let preview = crate::EmbeddedMaterialPreview {
            hull_rgb: [0.8, 0.1, 0.2],
            trim_rgb: [0.2, 0.1, 0.8],
            emissive_strength: 2.0,
            ..Default::default()
        };
        let parts = pioneer_construction_model();
        assert!(parts.iter().any(|p| paint_role(p.0) == HULL_ROLE));
        assert!(parts.iter().any(|p| paint_role(p.0) == TRIM_ROLE));
        assert!(parts.iter().any(|p| p.0 & !0x00ff_ffff == EMISSION_ROLE));
        for &(mesh, _, color, emission) in &parts {
            let (edited_color, edited_emission) =
                preview_material(mesh, color, emission, Some(preview));
            match paint_role(mesh) {
                HULL_ROLE => assert_eq!(edited_color, Vec3::from_array(preview.hull_rgb)),
                TRIM_ROLE => assert_eq!(edited_color, Vec3::from_array(preview.trim_rgb)),
                _ => assert_eq!(edited_color, color),
            }
            if emission > 0.0 {
                assert_eq!(edited_emission, emission * preview.emissive_strength);
            }
        }
    }

    #[test]
    fn fleet_preview_roles_change_finish_not_geometry_or_station() {
        let preview = crate::EmbeddedMaterialPreview {
            hull_rgb: [0.8, 0.1, 0.2],
            trim_rgb: [0.2, 0.1, 0.8],
            ..Default::default()
        };
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let parts = cached_ship_parts(class);
            assert!(
                parts.iter().any(|p| paint_role(p.0) == HULL_ROLE),
                "{class:?}"
            );
            assert!(
                parts.iter().any(|p| paint_role(p.0) == TRIM_ROLE),
                "{class:?}"
            );
            for &(mesh, _, color, emission) in parts {
                let edited = preview_material(mesh, color, emission, Some(preview));
                assert_eq!(
                    preview_material(mesh, color, emission, None),
                    (color, emission)
                );
                if paint_role(mesh) == HULL_ROLE {
                    assert_eq!(edited.0, Vec3::from_array(preview.hull_rgb));
                }
                if mesh & !0x00ff_ffff != 0 {
                    let a = mesh_vertices(mesh);
                    let b = mesh_vertices(mesh_index(mesh));
                    assert_eq!(a.len(), b.len());
                    assert!(a.iter().zip(&b).all(|(a, b)| a.p == b.p && a.n == b.n));
                }
            }
        }
        assert_eq!(
            preview_material(0, Vec3::ONE, 0.0, Some(preview)),
            (Vec3::ONE, 0.0)
        );
    }
    #[test]
    fn armor_surface_is_closed_outward_and_preserves_cube_envelope() {
        use std::collections::BTreeMap;
        let vertices = super::armor_block();
        assert_eq!(vertices.len(), 44 * 3);
        let mut edges = BTreeMap::new();
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        for tri in vertices.chunks_exact(3) {
            let center = (tri[0].p + tri[1].p + tri[2].p) / 3.0;
            assert!(tri[0].n.dot(center) > 0.0);
            for i in 0..3 {
                let v = tri[i];
                assert!(v.p.is_finite() && v.n.is_finite());
                assert!((v.n.length() - 1.0).abs() < 1e-5);
                min = min.min(v.p);
                max = max.max(v.p);
                let mut pair = [
                    v.p.to_array().map(f32::to_bits),
                    tri[(i + 1) % 3].p.to_array().map(f32::to_bits),
                ];
                pair.sort();
                *edges.entry(pair).or_insert(0) += 1;
            }
        }
        assert_eq!(min, glam::Vec3::splat(-1.0));
        assert_eq!(max, glam::Vec3::ONE);
        assert!(edges.values().all(|count| *count == 2));
    }
    #[test]
    fn industrial_shell_is_closed_outward_bounded_and_pickable() {
        use std::collections::BTreeMap;
        let mesh = super::industrial_shell();
        assert_eq!(mesh.len(), 60 * 3);
        let mut edges = BTreeMap::new();
        let mut min = glam::Vec3::splat(f32::INFINITY);
        let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
        for tri in mesh.chunks_exact(3) {
            let geometric = (tri[1].p - tri[0].p).cross(tri[2].p - tri[0].p);
            assert!(geometric.length() > 1e-6);
            assert!(geometric.dot(tri[0].p + tri[1].p + tri[2].p) > 0.0);
            for i in 0..3 {
                let vertex = tri[i];
                assert!(vertex.p.is_finite() && vertex.n.is_finite());
                assert!((vertex.n.length() - 1.0).abs() < 1e-5);
                assert!(geometric.normalize().dot(vertex.n) > 0.9999);
                min = min.min(vertex.p);
                max = max.max(vertex.p);
                let a = vertex.p.to_array().map(f32::to_bits);
                let b = tri[(i + 1) % 3].p.to_array().map(f32::to_bits);
                let (key, direction) = if a < b { ([a, b], 1_i32) } else { ([b, a], -1) };
                let edge = edges.entry(key).or_insert((0, 0));
                edge.0 += 1;
                edge.1 += direction;
            }
        }
        assert_eq!(min, glam::Vec3::splat(-1.0));
        assert_eq!(max, glam::Vec3::ONE);
        assert!(edges.values().all(|edge| *edge == (2, 0)));
        for axis in [glam::Vec3::X, glam::Vec3::Y, glam::Vec3::Z] {
            for sign in [-1.0, 1.0] {
                let outward = axis * sign;
                let distance =
                    super::ray_mesh_hit(outward * 3.0, -outward, glam::Mat4::IDENTITY, &mesh)
                        .unwrap();
                assert!((distance - 2.0).abs() < 1e-5);
            }
        }
        let picked = super::mesh_vertices(9);
        assert_eq!(picked.len(), mesh.len());
        assert!(picked
            .iter()
            .zip(&mesh)
            .all(|(a, b)| a.p == b.p && a.n == b.n));
    }
    use super::*;
    #[test]
    fn drone_runtime_and_cockpit_depth_share_exact_uploaded_geometry() {
        let geometry = flight_geometry().unwrap();
        let source = flight_geometry().unwrap();
        assert!(Arc::ptr_eq(&geometry, &source));
        let first_drone_mesh = geometry.drone_parts[0][0].0;
        assert!(geometry
            .station_batches
            .iter()
            .all(|batch| batch.0 < first_drone_mesh));
        let mut frame = frame_for_scene_clone_test();
        frame.space = Space {
            origin: [1000., 2000., 3000.],
        };
        frame.now = 1200;
        let drones: Vec<arkline_protocol::DroneView> = drone_models::CLASSES
            .into_iter()
            .enumerate()
            .map(|(i, class)| {
                serde_json::from_value(serde_json::json!({
                    "id": i + 1, "class": class, "position": [1025.,2025.,3025.],
                    "velocity": [10.,0.,0.], "heading": [0.,0.,1.],
                    "mode": {"kind":"hold"}, "updated_at_ms":1000
                }))
                .unwrap()
            })
            .collect();
        let pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"fixture", "credits":0, "ore":0, "location":{"kind":"station"},
            "drones":drones
        }))
        .unwrap();
        Arc::get_mut(&mut frame.scene).unwrap().pilots.push(pilot);
        let turrets = std::collections::HashMap::new();
        let collect = |frame: &Frame| {
            cockpit_occlusion_geometry::build(
                frame,
                &geometry.catalog,
                &geometry.station_batches,
                &geometry.drone_parts,
                &turrets,
            )
            .unwrap()
            .0
            .into_iter()
            .filter(|draw| draw.mesh >= first_drone_mesh)
            .collect::<Vec<_>>()
        };
        let draws = collect(&frame);
        let pilot = &frame.scene.pilots[0];
        let expected_count: usize = pilot
            .drones
            .iter()
            .map(|drone| geometry.drone_parts[drone_models::class_index(drone.class)].len())
            .sum();
        assert_eq!(draws.len(), expected_count);
        let mut draw_index = 0;
        for drone in &pilot.drones {
            let cached = cached_drone_parts(drone.class).unwrap();
            assert!(Arc::ptr_eq(
                &cached,
                &geometry.drone_parts[drone_models::class_index(drone.class)]
            ));
            let model = drone_models::model(drone.class);
            let base = drone_model_transform(frame.space, drone, frame.now);
            assert!(base
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(Vec3::new(0.27, 0.25, 0.25), 1e-6));
            assert!(base.transform_vector3(Vec3::X).abs_diff_eq(Vec3::Z, 1e-5));
            for (&(mesh, local, color, emission), batch) in cached.iter().zip(&model.batches) {
                assert_eq!(color, batch.color);
                assert_eq!(emission, batch.emission);
                let vertices = &geometry.catalog.uploads()[mesh];
                assert_eq!(vertices.len(), batch.vertices.len());
                for (actual, authored) in vertices.iter().zip(&batch.vertices) {
                    assert_eq!(actual.p, authored.p);
                    assert_eq!(actual.n, authored.n);
                }
                assert_eq!(draws[draw_index].mesh, mesh);
                assert_eq!(draws[draw_index].vertices, 0..vertices.len());
                assert!(draws[draw_index].model.abs_diff_eq(base * local, 1e-6));
                draw_index += 1;
            }
        }
        // Preserve the actual camera-drone body exclusion, without hiding peers.
        frame.camera_drone = Some(2);
        assert_eq!(
            collect(&frame).len(),
            expected_count - geometry.drone_parts[1].len()
        );
        frame.own = "another pilot".into();
        assert_eq!(collect(&frame).len(), expected_count);
    }

    #[test]
    fn drone_feed_budget_and_tiles_are_bounded() {
        assert_eq!(feed_resolution(1920, 1080), (384, 216));
        assert_eq!(feed_resolution(0, 0), (1, 1));
        assert!(!feed_due(Some(1000), 1010, 1));
        assert!(feed_due(Some(1000), 1016, 1));
        assert!(!feed_due(Some(1000), 1032, 4));
        assert!(feed_due(Some(1000), 1033, 4));
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320., 180.));
        let tiles = feed_tiles(screen, 15);
        assert_eq!(tiles.len(), 4);
        for (index, tile) in tiles.iter().enumerate() {
            assert!(screen.contains_rect(*tile));
            for other in &tiles[index + 1..] {
                assert!(!tile.intersects(*other));
            }
        }
    }
    #[test]
    fn drone_camera_modes_keep_target_visible_and_bound_extrapolation() {
        let drone: arkline_protocol::DroneView=serde_json::from_value(serde_json::json!({"id":1,"position":[0,0,0],"velocity":[100,0,0],"heading":[1,0,0],"mode":{"kind":"hold"},"updated_at_ms":1000})).unwrap();
        assert_eq!(drone_present_position(&drone, 1100), [10., 0., 0.]);
        assert_eq!(drone_present_position(&drone, 2000), [35., 0., 0.]);
        let space = Space { origin: [0.; 3] };
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320., 180.));
        for mode in [
            DroneCameraMode::Raised,
            DroneCameraMode::TopDown,
            DroneCameraMode::Orbit,
        ] {
            let (eye, vp) = drone_mode_matrix(&drone, space, rect.aspect_ratio(), mode, 1100);
            assert!(eye.is_finite() && vp.is_finite());
            let target = space.scene(drone_present_position(&drone, 1100));
            let point = project_point(vp, rect, target).unwrap();
            assert!(point.distance(rect.center()) < 0.01);
            assert!(eye.distance(target) > 0.3);
        }
    }
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Explicit native offscreen OpenGL cache/composite validation; no app window"]
    fn native_camera_cache_composites_and_restores_state() {
        use std::ffi::{c_void, CString};
        #[link(name = "OpenGL", kind = "framework")]
        unsafe extern "C" {
            fn CGLChoosePixelFormat(
                attributes: *const u32,
                format: *mut *mut c_void,
                count: *mut i32,
            ) -> i32;
            fn CGLCreateContext(
                format: *mut c_void,
                share: *mut c_void,
                context: *mut *mut c_void,
            ) -> i32;
            fn CGLSetCurrentContext(context: *mut c_void) -> i32;
            fn CGLDestroyContext(context: *mut c_void) -> i32;
            fn CGLDestroyPixelFormat(format: *mut c_void) -> i32;
        }
        unsafe extern "C" {
            fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
        }
        unsafe {
            let mut format = std::ptr::null_mut();
            let mut count = 0;
            let attributes = [99, 0x3200, 73, 8, 24, 12, 24, 0];
            assert_eq!(
                CGLChoosePixelFormat(attributes.as_ptr(), &mut format, &mut count),
                0
            );
            let mut context = std::ptr::null_mut();
            assert_eq!(
                CGLCreateContext(format, std::ptr::null_mut(), &mut context),
                0
            );
            CGLDestroyPixelFormat(format);
            assert_eq!(CGLSetCurrentContext(context), 0);
            let gl = Arc::new(glow::Context::from_loader_function(|name| {
                dlsym(
                    (-2_isize) as *mut c_void,
                    CString::new(name).unwrap().as_ptr(),
                )
                .cast()
            }));
            let mut renderer = Renderer::new(gl.clone()).unwrap();
            let target = CachedFeed::create(&gl, 999, 128, 128).unwrap();
            sun_shadows::test_sloped_pcf(&gl, &renderer, target.framebuffer);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 128, 128);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            gl.clear_color(1., 0., 1., 1.);
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(40, 40, 48, 48);
            gl.enable(glow::BLEND);
            gl.disable(glow::DEPTH_TEST);
            gl.depth_mask(false);
            let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y);
            let vp = Mat4::perspective_rh_gl(1.05, 1., 0.002, 1200.) * view;
            let mut frame = Frame {
                exterior: None,
                lighting_quality: Default::default(),
                cockpit_vp: vp,
                cockpit_occlusion: None,
                snapshot_time: 1000,
                planet_vp: vp,
                system_id: 3,
                ship_class: ShipClass::Pioneer,
                drone_camera: true,
                camera_drone: None,
                scene: Arc::new(FrameScene {
                    world_layout: arkline_protocol::legacy_world_layout(),
                    traffic: vec![],
                    surface_props: vec![],
                    asteroids: vec![],
                    gates: vec![],
                    pilots: vec![],
                }),
                space: Space { origin: [0.; 3] },
                cockpit: false,
                rotation: Quat::IDENTITY,
                connected: true,
                vp,
                eye: Vec3::ZERO,
                now: 1000,
                own: "fixture".into(),
                docking: None,
            };
            sun_shadows::assert_occluder_depth(&mut renderer, &frame, target.framebuffer);
            let info = || egui::PaintCallbackInfo {
                viewport: egui::Rect::from_min_size(egui::pos2(32., 32.), egui::vec2(64., 64.)),
                clip_rect: egui::Rect::from_min_size(egui::pos2(40., 40.), egui::vec2(48., 48.)),
                pixels_per_point: 1.,
                screen_size_px: [128, 128],
            };
            renderer.paint_cached_feed(&frame, 1, info(), 1, None);
            assert_eq!(
                gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                Some(target.framebuffer)
            );
            assert!(gl.is_enabled(glow::BLEND));
            assert!(!gl.is_enabled(glow::DEPTH_TEST));
            assert!(!gl.get_parameter_bool(glow::DEPTH_WRITEMASK));
            let mut pixels = [0u8; 4];
            gl.read_pixels(
                64,
                64,
                1,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            assert_ne!(
                pixels,
                [255, 0, 255, 255],
                "feed must composite into destination"
            );
            gl.read_pixels(
                16,
                16,
                1,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            assert_eq!(pixels, [255, 0, 255, 255], "outside clip stays untouched");
            frame.now = 1008;
            renderer.paint_cached_feed(&frame, 1, info(), 1, None);
            assert_eq!(renderer.feed_cache[0].painted_ms, Some(1000));
            for key in 2..=5 {
                renderer.paint_cached_feed(&frame, key, info(), 4, None);
            }
            assert_eq!(renderer.feed_cache.len(), 4);
            assert!(!renderer.feed_cache.iter().any(|cache| cache.key == 1));
            let live_textures: Vec<_> = renderer
                .feed_cache
                .iter()
                .map(|cache| cache.texture)
                .collect();
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            // Actual GPU readback: identical geometry/color must retain distinct
            // dielectric/conductor response, independent of base-color brightness.
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 128, 128);
            gl.disable(glow::SCISSOR_TEST);
            gl.disable(glow::BLEND);
            gl.enable(glow::DEPTH_TEST);
            gl.depth_mask(true);
            gl.use_program(Some(renderer.program));
            let material_shadow = renderer.disable_sun_shadow();
            let material_eye = Vec3::new(0.0, 0.0, 4.0);
            let material_vp = Mat4::perspective_rh_gl(0.8, 1.0, 0.1, 100.0)
                * Mat4::look_at_rh(material_eye, Vec3::ZERO, Vec3::Y);
            gl.uniform_matrix_4_f32_slice(
                renderer.uniforms.vp.as_ref(),
                false,
                &material_vp.to_cols_array(),
            );
            gl.uniform_3_f32(
                renderer.uniforms.eye.as_ref(),
                material_eye.x,
                material_eye.y,
                material_eye.z,
            );
            gl.uniform_1_i32(renderer.uniforms.material.as_ref(), 0);
            gl.uniform_1_f32(renderer.uniforms.environment_atmosphere.as_ref(), 0.0);
            gl.uniform_3_f32(renderer.uniforms.environment_up.as_ref(), 0.0, 1.0, 0.0);
            let mut material_pixels = Vec::new();
            for material in [
                Material::Rock,
                Material::Ore,
                Material::PaintedHull,
                Material::BareMetal,
                Material::Glass,
            ] {
                gl.clear_color(0.0, 0.0, 0.0, 1.0);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                renderer.draw_material(
                    0,
                    Mat4::IDENTITY,
                    Vec3::new(0.25, 0.32, 0.38),
                    0.0,
                    material,
                );
                let mut pixel = [0u8; 4];
                gl.read_pixels(
                    64,
                    64,
                    1,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixel)),
                );
                assert_eq!(pixel[3], 255);
                assert!(
                    pixel[..3].iter().any(|c| *c > 8),
                    "material must remain visible: {material:?}"
                );
                material_pixels.push(pixel);
            }
            assert!(
                material_pixels[2][..3]
                    .iter()
                    .zip(&material_pixels[3][..3])
                    .any(|(a, b)| a.abs_diff(*b) > 5),
                "paint and bare metal must differ at equal color: {material_pixels:?}"
            );
            assert_ne!(
                material_pixels[0], material_pixels[4],
                "rock and screen glass cannot share roughness response"
            );
            // Live editor changes use the same draw path in flight and dock scenes.
            // No mesh rebuild; paused feed pixels are invalidated on edits and reset.
            let hull_index = pioneer_asset()
                .unwrap()
                .iter()
                .position(|p| p.material_name == "Graphite ceramic armor")
                .unwrap()
                + 10;
            let mut preview_pixels = Vec::new();
            for preview in [
                None,
                Some(crate::EmbeddedMaterialPreview {
                    hull_rgb: [0.9, 0.02, 0.03],
                    roughness: Some(0.2),
                    metallic: Some(0.7),
                    ..Default::default()
                }),
                None,
            ] {
                renderer.set_material_preview(preview);
                if preview.is_some() {
                    assert!(renderer
                        .feed_cache
                        .iter()
                        .all(|cache| cache.painted_ms.is_none()));
                }
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                let primitive = embedded_primitive(hull_index).unwrap();
                for side in [-1.0, 1.0] {
                    renderer.draw_material(
                        hull_index,
                        Mat4::from_translation(Vec3::new(side * 0.75, 0.0, 0.0))
                            * Mat4::from_scale(Vec3::splat(1.7)),
                        primitive.color,
                        primitive.emission,
                        Material::PaintedHull,
                    );
                }
                let mut pixels = vec![0u8; 128 * 128 * 4];
                gl.read_pixels(
                    0,
                    0,
                    128,
                    128,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                preview_pixels.push(pixels);
            }
            assert_ne!(
                preview_pixels[0], preview_pixels[1],
                "live hull preview must change GPU output"
            );
            for half in [0usize, 64] {
                assert!(
                    (0..128).any(|y| (half..half + 64).any(|x| {
                        let i = (y * 128 + x) * 4;
                        preview_pixels[0][i..i + 3] != preview_pixels[1][i..i + 3]
                    })),
                    "both Pioneer instances must receive live finish"
                );
            }
            assert_eq!(
                preview_pixels[0], preview_pixels[2],
                "reset restores authored GPU output"
            );
            // Independent PBR controls: identical authored hull, base color, camera,
            // lights and exposure. Each pair changes exactly one scalar uniform.
            let mut pbr_pixels = Vec::new();
            for (roughness, metallic) in [(0.15, 0.0), (0.85, 0.0), (0.4, 0.0), (0.4, 1.0)] {
                renderer.set_material_preview(Some(crate::EmbeddedMaterialPreview {
                    hull_rgb: [0.3, 0.3, 0.3],
                    roughness: Some(roughness),
                    metallic: Some(metallic),
                    ..Default::default()
                }));
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                let primitive = embedded_primitive(hull_index).unwrap();
                renderer.draw_material(
                    hull_index,
                    Mat4::from_scale(Vec3::splat(3.0)),
                    primitive.color,
                    primitive.emission,
                    Material::PaintedHull,
                );
                let mut pixels = vec![0u8; 128 * 128 * 4];
                gl.read_pixels(
                    0,
                    0,
                    128,
                    128,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                assert_eq!(gl.get_error(), glow::NO_ERROR);
                assert!(
                    pixels
                        .chunks_exact(4)
                        .filter(|p| p[..3].iter().any(|v| *v > 8))
                        .count()
                        > 64,
                    "PBR fixture must contain visible hull pixels"
                );
                pbr_pixels.push(pixels);
            }
            for (name, a, b) in [("roughness", 0, 1), ("metallic", 2, 3)] {
                let changed = pbr_pixels[a]
                    .chunks_exact(4)
                    .zip(pbr_pixels[b].chunks_exact(4))
                    .filter(|(a, b)| a[..3].iter().zip(&b[..3]).any(|(a, b)| a.abs_diff(*b) >= 2))
                    .count();
                assert!(
                    changed > 16,
                    "{name} alone must visibly change the same RGB hull: {changed} pixels"
                );
                eprintln!("independent {name} GPU response: {changed} pixels differ by >=2/255");
            }
            renderer.set_material_preview(None);
            // Actual cached triangle-edge overlay and native GL-state restoration.
            let mut wire_pixels = Vec::new();
            for enabled in [false, true] {
                renderer.debug_wireframe = enabled;
                renderer.debug_metrics.set(Default::default());
                gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
                gl.disable(glow::POLYGON_OFFSET_LINE);
                gl.polygon_offset(2., 3.);
                gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                renderer.draw(0, Mat4::IDENTITY, Vec3::splat(0.15), 0.);
                let mut pixels = vec![0u8; 128 * 128 * 4];
                gl.read_pixels(
                    0,
                    0,
                    128,
                    128,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                wire_pixels.push(pixels);
                let mut modes = [0; 2];
                gl.get_parameter_i32_slice(glow::POLYGON_MODE, &mut modes);
                assert_eq!(modes, [glow::FILL as i32; 2]);
                assert!(!gl.is_enabled(glow::POLYGON_OFFSET_LINE));
                assert_eq!(gl.get_parameter_f32(glow::POLYGON_OFFSET_FACTOR), 2.);
                assert_eq!(gl.get_parameter_f32(glow::POLYGON_OFFSET_UNITS), 3.);
                assert_eq!(renderer.debug_metrics.get().draw_calls, 1);
                assert_eq!(
                    renderer.debug_metrics.get().triangles,
                    renderer.meshes[0].count as u64 / 3
                );
                assert_eq!(
                    renderer.debug_metrics.get().wireframe_draw_calls,
                    u64::from(enabled)
                );
                assert_eq!(gl.get_error(), glow::NO_ERROR);
            }
            assert!(
                wire_pixels[0]
                    .iter()
                    .zip(&wire_pixels[1])
                    .filter(|(a, b)| a != b)
                    .count()
                    > 40,
                "wireframe must change actual GPU pixels"
            );
            renderer.debug_wireframe = false;
            gl.polygon_offset(0., 0.);
            // Exercise the integrated local-light hook through the actual mesh BRDF,
            // not only the isolated attenuation shader test.
            gl.uniform_1_i32(
                gl.get_uniform_location(renderer.program, "ark_local_count")
                    .as_ref(),
                1,
            );
            gl.uniform_4_f32_slice(
                gl.get_uniform_location(renderer.program, "ark_local_position_range[0]")
                    .as_ref(),
                &[0.0, 0.0, 3.0, 8.0],
            );
            gl.uniform_4_f32_slice(
                gl.get_uniform_location(renderer.program, "ark_local_color_power[0]")
                    .as_ref(),
                &[1.0, 0.8, 0.6, 10.0],
            );
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            renderer.draw_material(
                0,
                Mat4::IDENTITY,
                Vec3::new(0.25, 0.32, 0.38),
                0.0,
                Material::PaintedHull,
            );
            let mut illuminated = [0u8; 4];
            gl.read_pixels(
                64,
                64,
                1,
                1,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut illuminated)),
            );
            assert!(
                illuminated[..3]
                    .iter()
                    .zip(&material_pixels[2][..3])
                    .any(|(lit, unlit)| *lit > unlit.saturating_add(5)),
                "integrated local light must illuminate painted hull: {illuminated:?} vs {:?}",
                material_pixels[2]
            );
            renderer.local_lights.clear(&gl);
            renderer.restore_shadow_texture(material_shadow);
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            // Evaluate the real shared environment shader: smooth conductors
            // reflect sky direction, fully rough ones converge to the same mean.
            let environment_fragment = concat!(
                "#version 150\n",
                include_str!("shaders/sky.glsl"),
                include_str!("shaders/materials.glsl"),
                r#"
                uniform vec3 test_view; uniform float test_roughness; out vec4 color;
                void main(){color=vec4(ark_display(ark_environment(vec3(0,0,1),normalize(test_view),vec3(0.7),test_roughness,1.0,0.0,vec3(0,1,0))),1.0);}
            "#
            );
            let environment_program = compile(&gl, BACK_VERTEX, environment_fragment).unwrap();
            gl.use_program(Some(environment_program));
            gl.bind_vertex_array(Some(renderer.empty));
            gl.disable(glow::DEPTH_TEST);
            let view_uniform = gl.get_uniform_location(environment_program, "test_view");
            let rough_uniform = gl.get_uniform_location(environment_program, "test_roughness");
            let mut sky_pixels = Vec::new();
            for roughness in [0.12, 1.0] {
                for x in [-0.7, 0.7] {
                    gl.uniform_3_f32(view_uniform.as_ref(), x, 0.0, 0.7);
                    gl.uniform_1_f32(rough_uniform.as_ref(), roughness);
                    gl.draw_arrays(glow::TRIANGLES, 0, 3);
                    let mut pixel = [0u8; 4];
                    gl.read_pixels(
                        64,
                        64,
                        1,
                        1,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut pixel)),
                    );
                    sky_pixels.push(pixel);
                }
            }
            assert_ne!(
                sky_pixels[0], sky_pixels[1],
                "smooth metal samples directional sky"
            );
            assert_eq!(
                sky_pixels[2], sky_pixels[3],
                "roughness broadens the reflection toward a hemispheric mean"
            );
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            gl.use_program(None);
            gl.delete_program(environment_program);
            // A dark metal hull must receive the dock ceiling environment, with
            // direction-dependent reflections that broaden with roughness.
            let dock_fragment = concat!(
                "#version 150\n",
                include_str!("shaders/sky.glsl"),
                include_str!("shaders/materials.glsl"),
                r#"uniform float rough; uniform float dock; out vec4 color;
                void main(){vec3 n=vec3(0,1,0),v=normalize(vec3(0,1,-0.55)),a=vec3(0.07);
                vec3 c=dock>0.5?ark_station_environment(n,v,a,rough,0.85):ark_environment(n,v,a,rough,0.85,0.0,n);
                color=vec4(ark_display(c),1.0);}"#
            );
            let dock_program = compile(&gl, BACK_VERTEX, dock_fragment).unwrap();
            gl.use_program(Some(dock_program));
            let dock_uniform = gl.get_uniform_location(dock_program, "dock");
            let rough_uniform = gl.get_uniform_location(dock_program, "rough");
            let mut dock_pixels = Vec::new();
            for (dock, rough) in [(0.0, 0.18), (1.0, 0.18), (1.0, 0.9)] {
                gl.uniform_1_f32(dock_uniform.as_ref(), dock);
                gl.uniform_1_f32(rough_uniform.as_ref(), rough);
                gl.draw_arrays(glow::TRIANGLES, 0, 3);
                let mut pixel = [0u8; 4];
                gl.read_pixels(
                    64,
                    64,
                    1,
                    1,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixel)),
                );
                dock_pixels.push(pixel);
            }
            assert!(
                dock_pixels[1][1] > dock_pixels[0][1].saturating_add(15),
                "dock illumination must reveal dark metal: {dock_pixels:?}"
            );
            assert!(
                dock_pixels[1][1] > dock_pixels[2][1],
                "roughness must reduce the ceiling strip peak: {dock_pixels:?}"
            );
            assert!(
                dock_pixels[1][1] < 245,
                "dock reflection must retain highlight headroom: {dock_pixels:?}"
            );
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            gl.use_program(None);
            gl.delete_program(dock_program);
            if let Ok(directory) = std::env::var("ARKLINE_SHIP_CAPTURE_DIR") {
                std::fs::create_dir_all(&directory).unwrap();
                let capture = CachedFeed::create(&gl, 1100, 1280, 800).unwrap();
                for class in arkline_protocol::ALL_SHIP_CLASSES {
                    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(capture.framebuffer));
                    gl.viewport(0, 0, 1280, 800);
                    gl.disable(glow::SCISSOR_TEST);
                    gl.disable(glow::BLEND);
                    gl.disable(glow::CULL_FACE);
                    gl.enable(glow::DEPTH_TEST);
                    gl.depth_mask(true);
                    gl.color_mask(true, true, true, true);
                    gl.clear_color(0.012, 0.022, 0.024, 1.0);
                    gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
                    assert_eq!(gl.get_error(), glow::NO_ERROR, "capture target");
                    let length = class.spec().length_m as f32 / 100.0;
                    let eye = Vec3::new(0.8, 0.65, 1.0) * length;
                    let vp = Mat4::perspective_rh_gl(0.75, 1.6, length * 0.001, length * 10.0)
                        * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
                    gl.use_program(Some(renderer.program));
                    let old = renderer.disable_sun_shadow();
                    renderer.local_lights.clear(&gl);
                    gl.uniform_1_i32(renderer.uniforms.material.as_ref(), 0);
                    gl.uniform_1_f32(renderer.uniforms.environment_atmosphere.as_ref(), 0.0);
                    gl.uniform_3_f32(renderer.uniforms.environment_up.as_ref(), 0.0, 1.0, 0.0);
                    gl.uniform_3_f32(renderer.uniforms.eye.as_ref(), eye.x, eye.y, eye.z);
                    gl.uniform_matrix_4_f32_slice(
                        renderer.uniforms.vp.as_ref(),
                        false,
                        &vp.to_cols_array(),
                    );
                    assert_eq!(gl.get_error(), glow::NO_ERROR, "capture uniforms");
                    for (mesh, model, color, emission) in ship_parts(class)
                        .into_iter()
                        .chain(turret_parts(class, Quat::IDENTITY, 0))
                        .chain(turret_parts(class, Quat::IDENTITY, 1))
                    {
                        renderer.draw(mesh, model, color, emission);
                    }
                    let mut rgba = vec![0; 1280 * 800 * 4];
                    gl.read_pixels(
                        0,
                        0,
                        1280,
                        800,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut rgba)),
                    );
                    assert_eq!(gl.get_error(), glow::NO_ERROR);
                    let mut ppm = b"P6\n1280 800\n255\n".to_vec();
                    for y in (0..800).rev() {
                        for x in 0..1280 {
                            let i = (y * 1280 + x) * 4;
                            ppm.extend_from_slice(&rgba[i..i + 3]);
                        }
                    }
                    std::fs::write(
                        std::path::Path::new(&directory).join(format!("{class:?}.ppm")),
                        ppm,
                    )
                    .unwrap();
                    renderer.restore_shadow_texture(old);
                }
                capture.destroy(&gl);
            }
            renderer.destroy();
            assert!(renderer.feed_cache.is_empty());
            assert!(live_textures
                .into_iter()
                .all(|texture| !gl.is_texture(texture)));
            target.destroy(&gl);
            CGLSetCurrentContext(std::ptr::null_mut());
            CGLDestroyContext(context);
        }
    }
    #[test]
    fn reaction_thrusters_use_confirmed_acceleration_not_speed_and_mount_on_mesh() {
        let mut pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"ThrustQA","credits":0,"ore":0,"location":Location::Station,
            "flight":{"position":[0.0,0.0,0.0],"velocity":[600.0,0.0,0.0],"heading":[1.0,0.0,0.0],
                "mode":NavigationMode::Stop,"updated_at_ms":1000,"acceleration":[0.0,0.0,0.0]}
        }))
        .unwrap();
        assert_eq!(confirmed_body_acceleration(&pilot, 1000), Vec3::ZERO);
        pilot.flight.as_mut().unwrap().acceleration = [-60.0, 0.0, 0.0];
        assert_eq!(confirmed_body_acceleration(&pilot, 1000), -Vec3::X * 60.0);
        assert_eq!(confirmed_body_acceleration(&pilot, 1501), Vec3::ZERO);
        let rotation = Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let flight = pilot.flight.as_mut().unwrap();
        flight.orientation = rotation.to_array().map(f64::from);
        flight.acceleration = (rotation * (Vec3::X * 150.0)).to_array().map(f64::from);
        assert!(confirmed_body_acceleration(&pilot, 1000).distance(Vec3::X * 150.0) < 0.001);
        pilot.location = Location::Docked;
        assert_eq!(confirmed_body_acceleration(&pilot, 1000), Vec3::ZERO);
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let mounts = reaction_mounts(class);
            assert!(mounts[0].is_some(), "braking nozzle missing for {class:?}");
            for (index, mount) in mounts.iter().enumerate() {
                let mount = mount
                    .as_ref()
                    .unwrap_or_else(|| panic!("reaction nozzle {index} missing for {class:?}"));
                let out = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z][index];
                let nearest = ship_geometry(class, Mat4::IDENTITY)
                    .iter()
                    .filter_map(|(mesh, model)| {
                        ray_mesh_hit(*mount + out * 0.01, -out, *model, mesh)
                    })
                    .min_by(f32::total_cmp)
                    .unwrap();
                assert!(
                    (nearest - 0.01).abs() < 0.001,
                    "reaction nozzle must touch actual hull"
                );
            }
        }
    }
    #[test]
    fn head_look_obeys_viewport_permission_and_preserves_blocked_pose() {
        fn step(
            ctx: &egui::Context,
            cockpit: &mut super::super::Cockpit,
            tick: u32,
            shift: bool,
            events: Vec<egui::Event>,
        ) {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1200., 850.),
                    )),
                    focused: true,
                    time: Some(tick as f64 / 60.),
                    modifiers: egui::Modifiers {
                        shift,
                        ..Default::default()
                    },
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(cockpit, ctx, &mut eframe::Frame::_new_kittest()),
            );
        }
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.max_passes = std::num::NonZeroUsize::MIN);
        let mut cockpit = super::super::tests::view_test_cockpit(&ctx);
        // Paused editor presentation may inspect the view without issuing flight commands.
        cockpit.embedded = true;
        cockpit.embedded_input = false;
        cockpit.embedded_paused = true;
        cockpit.frozen_time = Some(1000);
        for tick in 1..=4 {
            step(&ctx, &mut cockpit, tick, false, vec![]);
        }
        let (viewport, layer) = cockpit.flight_view.input_surface().unwrap();
        let pointer = [
            viewport.center(),
            viewport.center_top() + egui::vec2(0., 24.),
            viewport.left_top() + egui::vec2(24., 24.),
        ]
        .into_iter()
        .find(|point| ctx.layer_id_at(*point) == Some(layer))
        .unwrap();
        let authority = serde_json::to_value(&cockpit.snapshot).unwrap();
        let (tx, requests) = std::sync::mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.flight_view.head_look = [0.2, -0.1];
        cockpit.flight_view.set_head_look_input_allowed(false);
        step(
            &ctx,
            &mut cockpit,
            5,
            true,
            vec![egui::Event::PointerMoved(pointer)],
        );
        let pose = cockpit.flight_view.editor_camera_pose();
        let before = cockpit.flight_view.head_look_angles();
        step(
            &ctx,
            &mut cockpit,
            6,
            true,
            vec![egui::Event::Key {
                key: egui::Key::ArrowLeft,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            }],
        );
        assert_eq!(cockpit.flight_view.head_look_angles(), before);
        assert_eq!(cockpit.flight_view.editor_camera_pose(), pose);
        assert!(cockpit.flight_view.computer_surface().is_some());

        // Positive control: the identical held key rotates only after viewport ownership.
        cockpit.flight_view.set_head_look_input_allowed(true);
        step(&ctx, &mut cockpit, 7, true, vec![]);
        assert!(cockpit.flight_view.head_look_angles()[0] > before[0]);
        assert!(cockpit.flight_view.computer_surface().is_some());

        let before_outside = cockpit.flight_view.head_look_angles();
        let pose = cockpit.flight_view.editor_camera_pose();
        step(
            &ctx,
            &mut cockpit,
            8,
            true,
            vec![egui::Event::PointerMoved(egui::pos2(-20., -20.))],
        );
        assert_eq!(cockpit.flight_view.head_look_angles(), before_outside);
        assert_eq!(cockpit.flight_view.editor_camera_pose(), pose);
        let press = egui::Event::PointerButton {
            pos: pointer,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::SHIFT,
        };
        step(
            &ctx,
            &mut cockpit,
            9,
            true,
            vec![egui::Event::PointerMoved(pointer), press],
        );
        step(&ctx, &mut cockpit, 10, true, vec![]);
        assert_eq!(
            cockpit.flight_view.head_look_angles(),
            before_outside,
            "a held pointer drag must neither capture Shift look nor recenter the view"
        );
        assert!(cockpit.flight_view.computer_surface().is_some());
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), authority);
        assert!(cockpit.controls.is_empty());
        assert!(requests.try_recv().is_err());
    }

    #[test]
    fn head_look_is_bounded_and_release_recenters_without_ship_rotation() {
        let rotation = Quat::from_rotation_x(0.6);
        let mut angles = [0.0; 2];
        for _ in 0..200 {
            update_head_look(&mut angles, true, [-1.0, 1.0], 0.016);
        }
        assert_eq!(angles, [-std::f32::consts::PI, 1.45]);
        let (eye, _) = cockpit_camera(Vec3::ZERO, rotation);
        let looked = Mat4::look_at_rh(
            eye,
            eye + rotation * Quat::from_rotation_y(angles[0]) * Vec3::X,
            rotation * Vec3::Y,
        );
        assert!(looked.is_finite());
        for _ in 0..100 {
            update_head_look(&mut angles, false, [1.0; 2], 0.016);
        }
        assert_eq!(angles, [0.0; 2]);
    }
    #[test]
    fn drone_camera_uses_drone_pose_and_stays_inside_physical_screen() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let class = ShipClass::Pioneer;
        let feed = drone_feed_rect(viewport, class);
        let (center, size) = cockpit_screens(class)[1];
        assert!(instrument_rect(viewport, center, size).contains_rect(feed));
        let space = Space {
            origin: [149_597_870_700.0, 50.0, 100.0],
        };
        let mut drone = arkline_protocol::DroneView {
            id: 1,
            class: arkline_protocol::DroneClass::Recon,
            hit_points: 100,
            signal_strength: 1.0,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [149_597_871_700.0, 50.0, 100.0],
            velocity: [0.0; 3],
            heading: [1.0, 0.0, 0.0],
            mode: arkline_protocol::DroneMode::Hold,
            updated_at_ms: 1000,
        };
        let (eye, vp) = drone_camera_matrix(&drone, space, feed.aspect_ratio());
        assert_eq!(eye, Vec3::new(10.0, 0.0, 0.0));
        let point = eye + Vec3::X * 10.0;
        assert!(
            project_point(vp, feed, point)
                .unwrap()
                .distance(feed.center())
                < 0.01
        );
        drone.position[2] += 500.0;
        let (moved, moved_vp) = drone_camera_matrix(&drone, space, feed.aspect_ratio());
        assert_eq!(moved.z, 5.0);
        assert!(
            project_point(moved_vp, feed, point)
                .unwrap()
                .distance(feed.center())
                > 10.0
        );
        let (orbit_eye, orbit_vp) =
            drone_view_matrix(&drone, space, feed.aspect_ratio(), true, 15_000);
        assert!(orbit_eye.distance(space.scene(drone.position)) > 0.3);
        assert!(
            project_point(orbit_vp, feed, space.scene(drone.position))
                .unwrap()
                .distance(feed.center())
                < 0.01
        );
        let (later_eye, _) = drone_view_matrix(&drone, space, feed.aspect_ratio(), true, 30_000);
        assert!(later_eye.distance(orbit_eye) > 0.1);
        drone.heading = [0.0, 1.0, 0.0];
        assert!(drone_camera_matrix(&drone, space, feed.aspect_ratio())
            .1
            .is_finite());
    }

    #[test]
    fn instrument_faces_match_projection_and_do_not_overlap_after_resize() {
        for size in [
            egui::vec2(900.0, 650.0),
            egui::vec2(1100.0, 768.0),
            egui::vec2(1800.0, 1200.0),
            egui::vec2(2560.0, 1440.0),
        ] {
            let viewport = egui::Rect::from_min_size(egui::pos2(17.0, 29.0), size);
            let vp = Mat4::perspective_rh_gl(COCKPIT_FOV, viewport.aspect_ratio(), 0.0002, 10.0)
                * Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y);
            for class in arkline_protocol::ALL_SHIP_CLASSES {
                let screens = cockpit_screens(class).map(|(center, size)| {
                    let rect = instrument_rect(viewport, center, size);
                    assert!(viewport.contains_rect(rect));
                    let corner = Vec3::new(
                        center.x - 0.031,
                        center.y + size.y - 0.008,
                        center.z - size.z + 0.008,
                    ) * 0.025;
                    assert!(
                        project_point(vp, viewport, corner)
                            .unwrap()
                            .distance(rect.left_top())
                            < 0.001
                    );
                    rect
                });
                for (i, a) in screens.iter().enumerate() {
                    for b in &screens[i + 1..] {
                        assert!(!a.intersects(*b), "physical screens overlap");
                    }
                }
            }
        }
    }

    #[test]
    fn cockpit_telemetry_uses_ship_state_and_marks_disconnection_stale() {
        let mut pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"InstrumentQA", "credits":123, "ore":225, "location":Location::Belt,
            "system_id":2, "locked_asteroid":7
        }))
        .unwrap();
        pilot.flight = Some(arkline_protocol::FlightState {
            acceleration: [0.0; 3],
            position: [0.0; 3],
            velocity: [3.0, 4.0, 0.0],
            heading: [1.0, 0.0, 0.0],
            orientation: [0.0, 0.0, 0.0, 1.0],
            angular_velocity: [0.0; 3],
            mode: NavigationMode::Manual,
            updated_at_ms: 1000,
            warp: None,
        });
        let snapshot = Snapshot {
            docked_traversal_capability: None,
            world_layout: None,
            exterior: None,
            fleet_links: vec![],
            remote_controls: vec![],
            resource_jobs: vec![],
            surface_props: vec![],
            traffic: vec![],
            stored_ore: 0,
            server_time_ms: 1000,
            online_pilots: vec![],
            asteroids: vec![],
            pilots: vec![pilot.clone()],
            orders: vec![],
            belt_ore: 0,
            ledger_sequence: 0,
            ledger_hash: String::new(),
        };
        let rows = instrument_rows(&pilot, &snapshot, Some(5000.0), 1000, true);
        assert!(rows[0].contains(&"FWD +3.0 m/s".to_string()));
        assert!(rows[0].contains(&"UP +4.0 m/s".to_string()));
        let expired = instrument_rows(&pilot, &snapshot, None, 3000, true);
        assert!(expired.iter().all(|rows| rows[0].contains("STALE DATA")));
        assert!(rows[1].contains(&"LOCKED #7".to_string()));
        assert!(rows[1].contains(&super::super::format_distance(5000.0)));
        assert!(rows[2].contains(&"HOLD 225 / 1000".to_string()));
        assert!(rows[2].contains(&"POWER DATA UNAVAILABLE".to_string()));
        pilot.power = Some(arkline_protocol::PowerState {
            charge_j: 25_000,
            capacity_j: 240_000,
            regen_w: 1200,
            updated_at_ms: 1000,
            regen_remainder_mj: 0,
        });
        let powered = instrument_rows(&pilot, &snapshot, None, 1000, true);
        assert!(powered[2].contains(&"CAP 25.0/240.0 kJ".to_string()));
        assert!(powered[2].contains(&"LOW POWER / WARP UNAVAILABLE".to_string()));
        let stale = instrument_rows(&pilot, &snapshot, None, 1000, false);
        assert!(stale.iter().all(|rows| rows[0].contains("STALE DATA")));
        assert!(stale[1].contains(&"NO SELECTION".to_string()));
    }

    #[test]
    fn tactical_rings_use_real_kilometers_and_bounded_geometry() {
        let rings = tactical_radii(350.0);
        assert_eq!(rings[0] * 100.0, 5000.0);
        assert_eq!(rings[1] * 100.0, 10000.0);
        for distance in [15.0, 99.0, 100.0, 350.0, 800.0, 2500.0] {
            let radii = tactical_radii(distance);
            assert!((2..=10).contains(&radii.len()));
            for radius in radii {
                for angle in [0.0, 0.3, 1.2, 3.8, 6.0] {
                    let point = tactical_ring_point(radius, angle);
                    assert_eq!(point.y, 0.0);
                    assert!((point.length() - radius).abs() < 0.001);
                }
            }
        }
    }

    #[test]
    fn tactical_navigation_reaches_plane_at_maximum_zoom() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let eye = Vec3::new(0.0, 1500.0, 2000.0);
        let vp = Mat4::perspective_rh_gl(1.22, rect.aspect_ratio(), 1.0, 10000.0)
            * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let hit = unproject_to_plane(vp, rect, rect.center(), 0.0).unwrap();
        assert!(hit.length() < 0.02, "{hit:?}");
    }

    #[test]
    fn tactical_ship_center_and_meter_scale_survive_astronomical_rebase() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 900.0));
        let view = Mat4::look_at_rh(Vec3::new(200.0, 200.0, 200.0), Vec3::ZERO, Vec3::Y);
        let vp = Mat4::perspective_rh_gl(1.22, 1600.0 / 900.0, 0.03, 10000.0) * view;
        let origin = [149_597_870_700.0, 8000.0, -149_597_870_700.0];
        let space = Space { origin };
        assert!(
            project_point(vp, rect, space.scene(origin))
                .unwrap()
                .distance(rect.center())
                < 0.001
        );
        let target = space.scene([origin[0] + 3000.0, origin[1] + 4000.0, origin[2]]);
        assert_eq!(target.length() * 100.0, 5000.0);
        assert_eq!(target.y * 100.0, 4000.0);
        let near = project_point(vp, rect, Vec3::X * 50.0).unwrap();
        let far = project_point(vp, rect, Vec3::X * 100.0).unwrap();
        assert!(far.distance(rect.center()) > near.distance(rect.center()));
    }

    #[test]
    #[ignore]
    fn render_preparation_profile() {
        let space = Space {
            origin: BELT_POSITION,
        };
        let rock = arkline_protocol::asteroid_catalog(10000).remove(0);
        let source = space.scene(rock.position) + Vec3::Z * 10.0;
        let gates = gates();
        let batches = station_batches_geometry();
        let start = std::time::Instant::now();
        // Reconstruct the former per-frame geometry-generation work. This is
        // CPU-only evidence, not a native GPU frame-rate benchmark.
        for _ in 0..500 {
            std::hint::black_box(station_parts(space));
            for _ in &gates {
                for _ in 0..2 {
                    std::hint::black_box(torus(1.0, 0.12));
                }
            }
            std::hint::black_box(sphere(true));
            std::hint::black_box(mining_impact(source, &rock, space, 0));
        }
        let uncached = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..500 {
            std::hint::black_box(Mat4::from_translation(space.scene(STATION_CENTER_POSITION)));
            for gate in &gates {
                std::hint::black_box(gate_models(gate, space));
            }
            std::hint::black_box(mining_impact(source, &rock, space, 0));
        }
        eprintln!(
            "CPU render preparation500 iterations: reconstructed uncached {:?}; cached {:?}. Station draws {} -> {}",
            uncached,
            start.elapsed(),
            station_parts(space).len(),
            batches.len()
        );
    }
    #[test]
    fn station_batches_preserve_bounds_vertices_and_reduce_draw_calls() {
        let parts = station_parts(Space {
            origin: STATION_CENTER_POSITION,
        });
        let (expected_min, expected_max) = parts_bounds(&parts);
        let batches = station_batches_geometry();
        assert!(batches.len() <= 8);
        assert!(parts.len() > 150);
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut count = 0;
        for (mesh, _, _) in batches {
            for v in mesh {
                min = min.min(v.p);
                max = max.max(v.p);
                count += 1;
                assert!(v.n.is_finite() && (v.n.length() - 1.0).abs() < 1e-4);
            }
        }
        assert!(min.distance(expected_min) < 0.001 && max.distance(expected_max) < 0.001);
        assert_eq!(
            count,
            parts
                .iter()
                .map(|p| mesh_vertices(p.0).len())
                .sum::<usize>()
        );
    }
    #[test]
    fn two_module_mounts_are_distinct_supported_and_forward_of_seat() {
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let left = turret_mount(class, 0).0;
            let right = turret_mount(class, 1).0;
            assert!(left.z < 0.0 && right.z > 0.0);
            assert_eq!(left.x, right.x);
            assert!(left.x > class.spec().length_m as f32 * 0.0005);
            for id in 0..2 {
                assert!(turret_deck(class, id).is_finite());
            }
        }
    }
    #[test]
    fn turret_pedestal_extends_into_measured_hull_deck() {
        let module_id = 0;
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let (mount, _, _) = turret_mount(class, module_id);
            let deck = turret_deck(class, module_id);
            let parts = turret_parts(class, Quat::IDENTITY, module_id);
            let (_, support, _, _) = parts.last().unwrap();
            let bottom = support.transform_point3(-Vec3::Y);
            let top = support.transform_point3(Vec3::Y);
            assert!(bottom.y < deck && top.y > deck);
            assert!((top.y - mount.y).abs() < 1e-6);
            let hit = ship_geometry(class, Mat4::IDENTITY)
                .iter()
                .filter_map(|(mesh, model)| ray_mesh_hit(top, -Vec3::Y, *model, mesh))
                .min_by(f32::total_cmp)
                .unwrap();
            assert!(
                hit < top.y - bottom.y,
                "support must reach actual hull geometry"
            );
        }
    }
    #[test]
    fn mining_turret_slews_at_finite_rate_and_beam_starts_at_barrel_tip() {
        let module_id = 0;
        let desired = Quat::from_rotation_arc(Vec3::X, Vec3::Z);
        let step = slew_rotation(Quat::IDENTITY, desired, 0.1);
        assert!((2.0 * step.w.acos() - 0.1).abs() < 0.0001);
        assert_eq!(slew_rotation(Quat::IDENTITY, desired, 0.0), Quat::IDENTITY);
        let aligned = slew_rotation(Quat::IDENTITY, desired, 4.0);
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let (mount, radius, barrel) = turret_mount(class, module_id);
            let muzzle = turret_muzzle(class, aligned, module_id);
            assert!((muzzle - mount).normalize().distance(Vec3::Z) < 0.0001);
            assert!((muzzle.distance(mount) - radius - barrel).abs() < 0.0001);
            let (_, model, _, _) = &turret_parts(class, aligned, module_id)[2];
            assert!(model.transform_point3(Vec3::ZERO).distance(muzzle) < 0.0001);
            let spec = class.spec();
            assert!(
                muzzle.y > spec.height_m as f32 / 200.0,
                "turret is a roof appendage, excluded from published body dimensions"
            );
        }
    }
    #[test]
    fn pioneer_asset_and_parts_are_cached_with_bounded_draw_calls() {
        let asset = pioneer_asset().unwrap();
        assert!(std::ptr::eq(asset, pioneer_asset().unwrap()));
        let parts = cached_ship_parts(ShipClass::Pioneer);
        assert!(std::ptr::eq(parts, cached_ship_parts(ShipClass::Pioneer)));
        assert_eq!(parts.len(), asset.len());
        assert!(parts.len() <= 16);
        assert!(asset.iter().map(|p| p.vertices.len()).sum::<usize>() <= 600_000);
    }
    #[test]
    #[ignore = "Explicit CPU decode/cache timing, no GPU or UI"]
    fn pioneer_asset_decode_timing() {
        let bytes = include_bytes!("../../../assets/ships/pioneer/pioneer.glb");
        let start = std::time::Instant::now();
        let parsed = ship_asset::decode(bytes).unwrap();
        let decode = start.elapsed();
        let cached = pioneer_asset().unwrap();
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            std::hint::black_box(pioneer_asset().unwrap());
        }
        eprintln!(
            "Pioneer asset: bytes={}, triangles={}, draw_calls={}, decode_once_us={}, cached_100000_accesses_us={}, debug_assertions={}",
            bytes.len(),
            parsed.iter().map(|p| p.vertices.len() / 3).sum::<usize>(),
            cached.len(),
            decode.as_micros(),
            start.elapsed().as_micros(),
            cfg!(debug_assertions)
        );
    }

    #[test]
    fn pioneer_engine_effects_use_two_authored_nozzles_not_material_lamps() {
        let mounts = engine_mounts(ShipClass::Pioneer);
        assert_eq!(mounts.len(), 2);
        assert!(mounts
            .iter()
            .all(|m| (m.x + 0.359).abs() < 1e-6 && m.y == 0.0));
        assert_eq!(mounts[0].z, -mounts[1].z);
    }

    #[test]
    fn construction_fleet_geometry_has_bounded_parts_and_restrained_lamps() {
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let parts = ship_parts(class);
            assert!(
                parts.len() <= 96,
                "{class:?} exceeds procedural detail budget"
            );
            if class == ShipClass::Pioneer {
                // Material batches can span the whole hull: measure luminous
                // triangle area rather than the enclosing batch bounding box.
                let primitives = pioneer_asset().expect("embedded Pioneer asset validates");
                assert!(primitives.len() <= 16, "Pioneer draw-call budget");
                let area = |p: &ship_asset::Primitive| {
                    p.vertices
                        .chunks_exact(3)
                        .map(|v| (v[1].p - v[0].p).cross(v[2].p - v[0].p).length() * 0.5)
                        .sum::<f32>()
                };
                let total: f32 = primitives.iter().map(area).sum();
                let luminous: f32 = primitives
                    .iter()
                    .filter(|p| p.emission > 0.0)
                    .map(area)
                    .sum();
                assert!(luminous < total * 0.12, "Pioneer luminous surface budget");
                assert!(primitives.iter().all(|p| {
                    p.vertices
                        .iter()
                        .all(|v| v.p.is_finite() && v.n.is_finite())
                }));
                continue;
            }
            assert!(
                parts
                    .iter()
                    .filter(|(_, _, c, e)| c.y > c.x * 2.0 && *e == 0.0)
                    .count()
                    >= 4
            );
            assert!(parts
                .iter()
                .all(|(_, m, c, e)| m.is_finite() && c.is_finite() && *e <= 0.9));
            let (lo, hi) = parts_bounds(&parts);
            let bound = hi - lo;
            for (mesh, model, _, emission) in &parts {
                if *emission > 0.0 {
                    let (a, b) = parts_bounds(&[(*mesh, *model, Vec3::ONE, *emission)]);
                    assert!(
                        (b - a).max_element() < bound.max_element() * 0.20,
                        "large luminous hull plate"
                    );
                }
            }
        }
    }
    #[test]
    fn hulls_match_public_meter_dimensions_and_picking_surfaces() {
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let parts = ship_parts(class);
            let (min, max) = parts_bounds(&parts);
            let spec = class.spec();
            let measured = (max - min) * 100.0;
            assert!(
                (measured
                    - Vec3::new(
                        spec.length_m as f32,
                        spec.height_m as f32,
                        spec.width_m as f32
                    ))
                .abs()
                .max_element()
                    < 0.01
            );
            let geometry = ship_geometry(class, Mat4::IDENTITY);
            assert_eq!(geometry.len(), parts.len());
            let hits = geometry
                .iter()
                .filter_map(|(mesh, model)| {
                    ray_mesh_hit(Vec3::X * (max.x + 10.0), -Vec3::X, *model, mesh)
                })
                .collect::<Vec<_>>();
            assert!(
                !hits.is_empty(),
                "every hull can be picked along centerline"
            );
        }
    }
    #[test]
    fn station_is_twenty_km_and_berths_are_clear() {
        let space = Space {
            origin: STATION_CENTER_POSITION,
        };
        let parts = station_parts(space);
        let (min, max) = parts_bounds(&parts);
        assert!(((max.y - min.y) * 100.0 - STATION_HEIGHT_M as f32).abs() < 0.1);
        assert!(((max.z - min.z) * 100.0 - STATION_DIAMETER_M as f32).abs() < 0.1);
        let port = space.scene(STATION_POSITION);
        for x in [-4.5, 0.0, 4.5] {
            for y in [-4.5, 0.0, 4.5] {
                let berth = port + Vec3::new(x, y, 10.0);
                for (mesh, model, _, _) in &parts {
                    let inverse = model.inverse();
                    let local = inverse.transform_point3(berth);
                    let vertices = mesh_vertices(*mesh);
                    let mut lo = Vec3::splat(f32::INFINITY);
                    let mut hi = Vec3::splat(f32::NEG_INFINITY);
                    for v in vertices {
                        lo = lo.min(v.p);
                        hi = hi.max(v.p);
                    }
                    if *mesh == 0 {
                        assert!(
                            !local.cmpge(lo).all() || !local.cmple(hi).all(),
                            "berth intersects a station component"
                        );
                    }
                    if *mesh == 1 {
                        let radial = (local.x * local.x + local.z * local.z).sqrt();
                        assert!((radial - 1.0).powi(2) + local.y.powi(2) > 0.12_f32.powi(2));
                    }
                }
            }
        }
    }
    #[test]
    fn cockpit_roll_rotates_camera_up_without_leveling() {
        let rotation = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
        let (eye, view) = cockpit_camera(Vec3::ZERO, rotation);
        let camera_up = view.inverse().transform_vector3(Vec3::Y);
        assert!(camera_up.distance(Vec3::Z) < 1e-5);
        let ahead = view.transform_point3(eye + rotation * Vec3::X);
        assert!(ahead.distance(-Vec3::Z) < 1e-5);
        let right = view.inverse().transform_vector3(Vec3::X);
        assert!(right.distance(rotation * Vec3::Z) < 1e-5);
    }
    #[test]
    fn warp_camera_tracks_rebased_ship_without_threshold_jumps() {
        let old = Space { origin: [0.0; 3] };
        for meters in [1.0, 100000.0, 1.0e10, arkline_protocol::AU_METERS] {
            let new = Space {
                origin: [meters, 0.0, 0.0],
            };
            assert_eq!(
                rebased_camera_target(Vec3::new(2.0, 3.0, 4.0), old, new, true),
                Vec3::ZERO
            );
        }
        assert_eq!(warp_tunnel_intensity("WARP WARMUP", 0.0), 0.0);
        assert_eq!(warp_tunnel_intensity("WARP CRUISE", 0.5), 1.0);
        assert_eq!(warp_tunnel_intensity("WARP ARRIVAL", 1.0), 0.0);
    }
    #[test]
    fn editor_free_camera_freezes_scene_origin_and_restores_live_player_origin() {
        let initial = Space {
            origin: [1_000_000.0, 20.0, -400.0],
        };
        let mut view = FlightView::new(None);
        view.space = initial;
        view.set_editor_free_camera(true, Some((Vec3::ZERO, 0.0, 0.0)));
        assert_eq!(view.editor_free_camera_space, Some(initial));
        view.set_editor_free_camera(false, None);
        assert!(view.editor_free_camera_space.is_none());
        let (first_frame, frozen) = editor_camera_scene_space(true, None, initial);
        let camera = Vec3::new(7.0, 3.0, -2.0);
        let camera_world = first_frame.world(camera);

        // The ship's display origin moves by ten scene units, but the camera's
        // scene transform remains anchored for the full free-camera session.
        let ship_world = [1_001_000.0, 20.0, -400.0];
        let moved_live = Space { origin: ship_world };
        let (moving_frame, still_frozen) = editor_camera_scene_space(true, frozen, moved_live);
        assert_eq!(moving_frame.origin, initial.origin);
        assert_eq!(still_frozen.unwrap().origin, initial.origin);
        assert_eq!(moving_frame.world(camera), camera_world);

        // On disable, the normal scene origin follows the ship again, while the
        // player target rebases to the same world point in that live frame.
        let (restored, no_freeze) = editor_camera_scene_space(false, still_frozen, moved_live);
        assert_eq!(restored.origin, ship_world);
        assert!(no_freeze.is_none());
        let player_target_during_freecam = moving_frame.scene(ship_world);
        let restored_target =
            rebased_camera_target(player_target_during_freecam, moving_frame, restored, false);
        assert!(restored_target.length() < 1e-5);
    }
    #[test]
    fn cockpit_heading_and_au_relative_precision() {
        let origin = [
            8.0 * arkline_protocol::AU_METERS,
            0.0,
            -4.0 * arkline_protocol::AU_METERS,
        ];
        let space = Space { origin };
        let target = [origin[0] + 1.0, 12.5, origin[2] - 2.0];
        let scene = space.scene(target);
        assert!((scene.x - 0.01).abs() < 1e-7);
        assert_eq!(space.world(scene), target);
        for forward in [Vec3::X, Vec3::Y, Vec3::Z, -Vec3::X] {
            let rotation = Quat::from_rotation_arc(Vec3::X, forward);
            let (eye, view) = cockpit_camera(Vec3::ZERO, rotation);
            let direction = view.transform_point3(eye + forward);
            assert!(direction.x.abs() < 1e-5 && direction.y.abs() < 1e-5 && direction.z < 0.0);
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
            let vp = Mat4::perspective_rh_gl(1.05, rect.aspect_ratio(), 0.03, 1200.0) * view;
            let (_, ray) = screen_ray(vp, rect, rect.center()).unwrap();
            assert!(ray.dot(forward) > 0.9999);
        }
    }
    #[test]
    fn extrapolation_is_bounded_and_warp_uses_server_position() {
        let space = Space::default();
        let mut pilot = PilotView {
            station_avatar: None,
            surface: None,
            power: None,
            mining_modules: vec![],
            target_lock: None,
            target_locks: vec![],
            locked_asteroids: vec![],
            drone: None,
            drones: vec![],
            ship_class: ShipClass::Pioneer,
            system_id: HOME_SYSTEM_ID,
            locked_asteroid: None,
            name: "Moving".into(),
            credits: 1000,
            ore: 0,
            location: Location::Station,
            flight: Some(arkline_protocol::FlightState {
                acceleration: [0.0; 3],
                orientation: [0.0, 0.0, 0.0, 1.0],
                angular_velocity: [0.0; 3],
                warp: None,
                position: [1000.0, 500.0, -200.0],
                velocity: [100.0, 0.0, 0.0],
                heading: [0.0, 0.0, -1.0],
                mode: NavigationMode::Stop,
                updated_at_ms: 1000,
            }),
        };
        assert_eq!(space.position(&pilot, 900), Vec3::new(10.0, 5.0, -2.0));
        assert!((space.position(&pilot, 1200).x - 10.2).abs() < 0.0001);
        assert_eq!(space.position(&pilot, 1350), space.position(&pilot, 90_000));
        pilot.location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: 7000,
        };
        assert_eq!(space.position(&pilot, 1200), Vec3::new(10.0, 5.0, -2.0));
        pilot.location = Location::Docked;
        assert_eq!(space.position(&pilot, 1200), Vec3::new(10.0, 5.0, -2.0));
        pilot.flight.as_mut().unwrap().orientation =
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
                .to_array()
                .map(f64::from);
        let moving_orientation = ship_orientation(&pilot);
        pilot.flight.as_mut().unwrap().velocity = [0.0; 3];
        assert_eq!(
            moving_orientation,
            ship_orientation(&pilot),
            "stopping must preserve authoritative heading"
        );
        assert!((moving_orientation * Vec3::X).distance(-Vec3::Z) < 0.0001);
        assert!(orientation([0.0; 3]).is_none());
        assert!(orientation([f64::NAN, 0.0, 0.0]).is_none());
    }
    #[test]
    fn navigation_projection_roundtrips_horizontal_plane() {
        let rect = egui::Rect::from_min_size(egui::pos2(310.0, 80.0), egui::vec2(800.0, 600.0));
        let eye = Vec3::new(15.0, 22.0, 28.0);
        let vp = Mat4::perspective_rh_gl(0.85, rect.width() / rect.height(), 0.3, 1200.0)
            * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        for world in [
            Vec3::ZERO,
            Vec3::new(3.0, 0.0, -2.0),
            Vec3::new(-2.0, 4.0, 3.0),
        ] {
            let screen = project_point(vp, rect, world).unwrap();
            let recovered = unproject_to_plane(vp, rect, screen, world.y).unwrap();
            assert!(
                world.distance(recovered) < 0.002,
                "{world:?} -> {recovered:?}"
            );
        }
        assert!(unproject_to_plane(vp, rect, rect.max + egui::vec2(2.0, 0.0), 0.0).is_none());
        assert!(unproject_to_plane(Mat4::ZERO, rect, rect.center(), 0.0).is_none());
        let horizon = Mat4::perspective_rh_gl(0.85, 4.0 / 3.0, 0.3, 1200.0)
            * Mat4::look_at_rh(
                Vec3::new(0.0, 10.0, 10.0),
                Vec3::new(0.0, 10.0, 0.0),
                Vec3::Y,
            );
        assert!(unproject_to_plane(horizon, rect, rect.center(), 0.0).is_none());
        assert!(unproject_to_plane(vp, rect, rect.center(), 100.0).is_none());
    }
    #[test]
    fn beam_transform_ends_on_requested_points_and_rejects_empty_segments() {
        let start = Vec3::new(8.0, -3.0, 2.0);
        let end = Vec3::new(-4.0, 1.5, 7.0);
        let model = segment_model(start, end, 0.05).unwrap();
        assert!(model.transform_point3(-Vec3::X).distance(start) < 0.0001);
        assert!(model.transform_point3(Vec3::X).distance(end) < 0.0001);
        assert!(segment_model(start, start, 0.05).is_none());
    }
    #[test]
    fn asteroid_spin_is_slow_epoch_stable_and_shared_by_picking_and_beams() {
        let rock = arkline_protocol::asteroid_catalog(10000).remove(0);
        let space = Space {
            origin: rock.position,
        };
        let now = 1_792_000_000_000;
        let start = asteroid_rotation(rock.id, now);
        let next = asteroid_rotation(rock.id, now + 16);
        assert_eq!(start, asteroid_rotation(rock.id, now));
        assert!(start.angle_between(asteroid_rotation(rock.id, now + 10_000)) > 0.08);
        assert!(start.angle_between(next) < 0.002);
        let seed = rock
            .id
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let period = 240_000 + seed % 360_000;
        assert!(
            asteroid_rotation(rock.id, period - 1)
                .angle_between(asteroid_rotation(rock.id, period + 1))
                < 0.002
        );
        for time in [0, now, now + 10_000, u64::MAX] {
            let model = asteroid_model(&rock, space, time);
            assert_eq!(model.transform_point3(Vec3::ZERO), Vec3::ZERO);
            assert!(cached_rock().iter().all(
                |v| model.transform_point3(v.p).length() <= rock.radius as f32 / 100.0 + 0.001
            ));
            let source = Vec3::Z * (rock.radius as f32 / 100.0 + 20.0);
            let hit = ray_mesh_hit(source, -Vec3::Z, model, cached_rock()).unwrap();
            let beam = mining_impact(source, &rock, space, time).unwrap();
            assert!(beam.distance(source - Vec3::Z * hit) < 0.0001);
        }
    }
    #[test]
    fn mining_targets_exact_authoritative_asteroid() {
        for asteroid in arkline_protocol::asteroid_catalog(10000) {
            let space = Space {
                origin: asteroid.position,
            };
            let center = space.scene(asteroid.position);
            let source = center + Vec3::Z * 10.0;
            let impact = mining_impact(source, &asteroid, space, 0).unwrap();
            assert!(impact.distance(center) <= asteroid.radius as f32 / 100.0 + 0.001);
            assert!(impact.distance(source) > 0.0);
        }
        assert!(!in_warp(&Location::Belt, 3000));
        let transit = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: 3000,
        };
        assert!(in_warp(&transit, 2999));
        assert!(!in_warp(&transit, 3000));
    }
    #[test]
    fn mesh_picking_uses_nearest_surface_and_gate_hole_is_empty() {
        let mesh = rock_mesh();
        let source = Vec3::new(0.0, 0.0, 20.0);
        let near = transform(Vec3::new(0.0, 0.0, 8.0), Vec3::splat(2.0), Quat::IDENTITY);
        let far = transform(Vec3::ZERO, Vec3::splat(4.0), Quat::IDENTITY);
        let a = ray_mesh_hit(source, -Vec3::Z, near, &mesh).unwrap();
        let b = ray_mesh_hit(source, -Vec3::Z, far, &mesh).unwrap();
        assert!(a < b);
        assert!(ray_mesh_hit(source, Vec3::X, near, &mesh).is_none());
        let gate = gates().remove(0);
        let space = Space {
            origin: gate.position,
        };
        let center = space.scene(gate.position);
        for (mesh, model) in gate_parts(&gate, space) {
            assert!(ray_mesh_hit(center + Vec3::Z * 20.0, -Vec3::Z, model, &mesh).is_none());
            assert!(ray_mesh_hit(
                center + Vec3::Z * 20.0 + Vec3::X * (gate.radius as f32 / 112.0),
                -Vec3::Z,
                model,
                &mesh
            )
            .is_some());
        }
        for asteroid in arkline_protocol::asteroid_catalog(10000) {
            let space = Space {
                origin: asteroid.position,
            };
            let model = asteroid_model(&asteroid, space, 0);
            for v in &mesh {
                assert!(
                    model
                        .transform_point3(v.p)
                        .distance(space.scene(asteroid.position))
                        <= asteroid.radius as f32 / 100.0 + 0.001
                );
            }
        }
    }
    #[test]
    fn warp_routes_match_both_directions_and_clamp() {
        let start = 1000;
        let arrival = start + WARP_DURATION_MS;
        let out = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: arrival,
        };
        let back = Location::InTransit {
            destination: Destination::Station,
            arrives_at_ms: arrival,
        };
        for (now, expected) in [
            (0, 0.0),
            (start, 0.0),
            (start + WARP_WARMUP_MS, 0.0),
            (
                start + WARP_WARMUP_MS + (WARP_DURATION_MS - WARP_WARMUP_MS) / 2,
                0.5,
            ),
            (arrival, 1.0),
            (arrival + 5000, 1.0),
        ] {
            assert_eq!(route_progress(&out, now), expected);
            assert_eq!(route_progress(&back, now), 1.0 - expected);
        }
        assert_eq!(heading(&out), 1.0);
        assert_eq!(heading(&back), -1.0);
    }
    #[test]
    fn warp_phases_hold_warmup_then_launch_cruise_and_arrive() {
        let start = 1_800_000_000_000;
        let arrival = start + WARP_DURATION_MS;
        let transit = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: arrival,
        };
        assert_eq!(warp_phase(&transit, start), Some(("WARP WARMUP", 0.0)));
        assert_eq!(
            warp_phase(&transit, start + WARP_WARMUP_MS / 2),
            Some(("WARP WARMUP", 0.5))
        );
        assert_eq!(route_progress(&transit, start + WARP_WARMUP_MS - 1), 0.0);
        assert_eq!(
            warp_phase(&transit, start + WARP_WARMUP_MS),
            Some(("WARP LAUNCH", 0.0))
        );
        assert_eq!(
            warp_phase(&transit, start + WARP_WARMUP_MS + 700),
            Some(("WARP CRUISE", 0.0))
        );
        assert_eq!(
            warp_phase(&transit, arrival - 700),
            Some(("WARP ARRIVAL", 0.0))
        );
        assert_eq!(warp_phase(&transit, arrival), None);
        assert_eq!(warp_phase(&Location::Belt, arrival), None);
        let mut previous = 0.0;
        for elapsed in (0..=WARP_DURATION_MS).step_by(10) {
            let progress = route_progress(&transit, start + elapsed);
            assert!(progress >= previous && progress <= 1.0);
            previous = progress;
            if let Some((_, local)) = warp_phase(&transit, start + elapsed) {
                assert!((0.0..=1.0).contains(&local));
            }
        }
    }
    #[test]
    fn geometry_is_finite_with_valid_normals_and_nondegenerate_faces() {
        for mesh in [
            cube(),
            torus(1.0, 0.12),
            sphere(false),
            rock_mesh(),
            hull(),
            sphere_with_resolution(false, 72, 48),
            torus(1.0, 0.008),
        ] {
            assert!(!mesh.is_empty());
            assert_eq!(mesh.len() % 3, 0);
            for v in &mesh {
                assert!(v.p.is_finite() && v.n.is_finite());
                assert!((v.n.length() - 1.0).abs() < 0.001);
            }
            for t in mesh.chunks_exact(3) {
                let geometric_normal = (t[1].p - t[0].p).cross(t[2].p - t[0].p);
                assert!(geometric_normal.length_squared() > 1e-10);
                assert!(geometric_normal.dot(t[0].n + t[1].n + t[2].n) > 0.0);
            }
        }
    }
    #[test]
    fn stable_pilot_lane_does_not_depend_on_roster_or_view() {
        let space = Space::default();
        let pilot = PilotView {
            station_avatar: None,
            surface: None,
            power: None,
            mining_modules: vec![],
            target_lock: None,
            target_locks: vec![],
            locked_asteroids: vec![],
            drone: None,
            drones: vec![],
            ship_class: ShipClass::Pioneer,
            system_id: HOME_SYSTEM_ID,
            locked_asteroid: None,
            name: "Pilot One".into(),
            credits: 100,
            ore: 0,
            location: Location::Station,
            flight: None,
        };
        let before = space.position(&pilot, 100);
        let _other = lane("Another pilot");
        assert_eq!(before, space.position(&pilot, 100));
        assert!((lane(&pilot.name)).abs() <= 5.0);
    }
}
