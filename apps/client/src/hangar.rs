//! Private station bay in actual meters, using the same hull meshes as flight.
use super::*;
#[cfg(not(target_arch = "wasm32"))]
#[path = "model_catalog_hangar.rs"]
pub(super) mod inspection_catalog;
use arkline_protocol::{hangar_layout, StationAvatar};
#[path = "avatar_animation.rs"]
mod avatar_animation;
#[path = "avatar_follow.rs"]
mod avatar_follow;
#[path = "pioneer_docked_scene.rs"]
pub(super) mod pioneer_docked_scene;
#[path = "pioneer_interior.rs"]
mod pioneer_interior;
#[cfg(all(test, target_os = "macos"))]
#[path = "pioneer_route_gpu_tests.rs"]
mod pioneer_route_gpu_tests;
#[path = "quarters.rs"]
mod quarters;
pub use quarters::HangarAction;
#[path = "fleet_hangar.rs"]
pub(super) mod fleet_hangar;

// Renderer uploads this after the embedded hull meshes, preserving their indices.
pub(super) fn station_floor_mesh_index() -> usize {
    fleet_hangar::floor_mesh_index()
}
pub(super) fn station_floor_vertices() -> Vec<Vertex> {
    fleet_hangar::floor_vertices()
}
#[cfg(test)]
#[path = "hangar_scale_tests.rs"]
mod hangar_scale_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFootView {
    FirstPerson,
    ThirdPerson,
}

/// The same freshness label for fixed instruments and cassette telemetry.
fn docked_instrument_authority(
    connected: bool,
    paused: bool,
    now: u64,
    snapshot_ms: u64,
) -> &'static str {
    if !connected {
        "DISCONNECTED"
    } else if paused {
        "PAUSED"
    } else if now.saturating_sub(snapshot_ms) > 1500 {
        "STALE"
    } else {
        "LIVE"
    }
}

/// Local cursor ownership only: never changes a ship or avatar snapshot.
#[derive(Default)]
struct FirstPersonMouseLook {
    active: bool,
    captured: bool,
    suspended: bool,
    await_release: bool,
    eligible: bool,
}
impl FirstPersonMouseLook {
    fn release(&mut self, ctx: &egui::Context) {
        if self.captured {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        }
        self.captured = false;
        self.eligible = false;
    }
    fn suspend(&mut self, ctx: &egui::Context) {
        self.release(ctx);
        self.suspended = true;
        self.await_release = true;
    }
    fn begin(&mut self, ctx: &egui::Context, active: bool, allowed: bool) -> egui::Vec2 {
        if !active {
            self.release(ctx);
            self.active = false;
            self.suspended = false;
            self.await_release = false;
            return egui::Vec2::ZERO;
        }
        self.active = true;
        let (focused, escape, any_down, keys_down, delta) = ctx.input(|input| {
            (
                input.focused && !input.viewport().minimized.unwrap_or(false),
                input.key_pressed(crate::control_bindings::MOUSE_LOOK_RELEASE),
                input.pointer.any_down(),
                !input.keys_down.is_empty(),
                input
                    .pointer
                    .motion()
                    .unwrap_or_else(|| input.pointer.delta()),
            )
        });
        if !focused || escape {
            self.suspend(ctx);
            return egui::Vec2::ZERO;
        }
        if !allowed {
            if self.captured || self.suspended {
                self.suspend(ctx);
            } else {
                // Initial entry can wait for clear viewport ownership without
                // turning the first mouse-look into a mandatory click.
                self.release(ctx);
            }
            return egui::Vec2::ZERO;
        }
        if self.await_release {
            if any_down || keys_down {
                return egui::Vec2::ZERO;
            }
            self.await_release = false;
        }
        self.eligible = true;
        if self.captured && delta.is_finite() && !ctx.input(|input| input.pointer.any_pressed()) {
            delta
        } else {
            egui::Vec2::ZERO
        }
    }
    fn finish(&mut self, ctx: &egui::Context, over_clear_world: bool) {
        if !self.active
            || !self.eligible
            || self.captured
            || self.await_release
            || !over_clear_world
        {
            return;
        }
        let (pressed, modifiers, keyboard_owned) = ctx.input(|input| {
            (
                input.pointer.primary_pressed(),
                !input.modifiers.is_none(),
                !input.keys_down.is_empty(),
            )
        });
        if modifiers || keyboard_owned || self.suspended && !pressed {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::Locked));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
        self.captured = true;
        self.suspended = false;
        // The capture frame's old absolute movement is intentionally discarded.
    }
}

pub struct HangarView {
    pub on_foot_view: OnFootView,
    renderer: Option<Arc<Mutex<Renderer>>>,
    error: Option<String>,
    surface: Option<(egui::Rect, egui::LayerId)>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    walking: bool,
    interior: bool,
    fleet_mode: bool,
    interior_deck: u8,
    class: Option<ShipClass>,
    follow: Option<Vec3>,
    follow_heading: Option<f64>,
    gait: avatar_animation::Gait,
    editor_free_camera: Option<(Vec3, f32, f32)>,
    editor_free_camera_active: bool,
    last_camera_pose: Option<(Vec3, f32, f32)>,
    room_layout: Option<arkline_protocol::RoomLayoutState>,
    room_parts: Option<pioneer_interior::LayoutPartsCache>,
    docked_enabled: bool,
    parent_managed_body_hud: bool,
    space_snapshot: Option<Arc<Snapshot>>,
    presentation_paused: bool,
    mechanism_pose: physical_cockpit::PoseSampler,
    reactor_pose: reactor_display::PoseSampler,
    drone_telemetry_time: crate::drone_telemetry::TelemetryTime,
    cargo_unload: Option<arkline_protocol::CargoUnloadOperation>,
    reactor_host_bound: bool,
    mechanism_host_bound: bool,
    mechanism_feed_frame: Option<Frame>,
    #[cfg(test)]
    docked_disable_culling: bool,
    docked_scene: Option<pioneer_docked_scene::SceneCache>,
    docked_initialized: bool,
    head_look_input_allowed: bool,
    head_look: [f32; 2],
    first_person_mouse: FirstPersonMouseLook,
    computer_keyboard_owned: bool,
    docked_surfaces: [Option<CockpitSurface>; COCKPIT_SURFACE_SLOTS],
    passive_console: Option<CockpitSurface>,
    docked_layer: Option<egui::LayerId>,
    docked_display_renderer: Option<Arc<Mutex<CockpitDisplayRenderer>>>,
}
impl HangarView {
    pub(crate) fn set_presentation_paused(&mut self, paused: bool) {
        self.presentation_paused = paused;
    }
    pub(crate) fn audio_listener(
        &self,
        pilot: &str,
        now: u64,
    ) -> Option<super::super::sound_scene::AudioListener> {
        self.mechanism_feed_frame
            .as_ref()
            .filter(|f| f.own == pilot && now.saturating_sub(f.snapshot_time) <= 1500)
            .and_then(Frame::audio_listener)
    }

    pub(crate) fn set_cargo_unload(&mut self, op: Option<arkline_protocol::CargoUnloadOperation>) {
        self.cargo_unload = op;
    }
    pub(crate) fn set_drone_telemetry_time(&mut self, time: crate::drone_telemetry::TelemetryTime) {
        self.drone_telemetry_time = time;
    }
    // egui callbacks share ownership on one browser thread; SendWrapper guards access.
    #[cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let result = gl.map(|gl| unsafe { Renderer::new(gl.clone()) });
        let (renderer, error) = match result {
            Some(Ok(renderer)) => (Some(Arc::new(Mutex::new(renderer))), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, Some("Hangar needs a GPU rendering context".into())),
        };
        let docked_display_renderer = gl
            .and_then(|gl| unsafe { CockpitDisplayRenderer::new(gl.clone()).ok() })
            .map(|renderer| Arc::new(Mutex::new(renderer)));
        Self {
            on_foot_view: OnFootView::ThirdPerson,
            renderer,
            error,
            surface: None,
            yaw: -0.65,
            pitch: 0.32,
            zoom: 1.0,
            walking: false,
            interior: false,
            fleet_mode: false,
            interior_deck: 1,
            class: None,
            follow: None,
            follow_heading: None,
            gait: Default::default(),
            editor_free_camera: None,
            editor_free_camera_active: false,
            last_camera_pose: None,
            room_layout: None,
            room_parts: None,
            docked_enabled: false,
            parent_managed_body_hud: false,
            space_snapshot: None,
            presentation_paused: false,
            mechanism_pose: Default::default(),
            reactor_pose: Default::default(),
            cargo_unload: None,
            drone_telemetry_time: Default::default(),
            reactor_host_bound: false,
            mechanism_host_bound: false,
            mechanism_feed_frame: None,
            #[cfg(test)]
            docked_disable_culling: false,
            docked_scene: None,
            docked_initialized: false,
            head_look_input_allowed: true,
            head_look: [0.0; 2],
            first_person_mouse: FirstPersonMouseLook::default(),
            computer_keyboard_owned: false,
            docked_surfaces: std::array::from_fn(|_| None),
            passive_console: None,
            docked_layer: None,
            docked_display_renderer,
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_material_preview(&mut self, preview: Option<crate::EmbeddedMaterialPreview>) {
        if let Some(renderer) = &self.renderer {
            if let Ok(mut renderer) = renderer.lock() {
                renderer.set_material_preview(preview);
            }
        }
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // Native editor authoring entry.
    pub fn set_spatial_material(&mut self, material: Option<Arc<MaterialRevision>>) {
        if let Some(renderer) = &self.renderer {
            if let Ok(mut renderer) = renderer.lock() {
                let _ = renderer.spatial_material.queue(material);
            }
        }
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn material_program_status(&self) -> MaterialProgramStatus {
        self.renderer
            .as_ref()
            .and_then(|r| r.lock().ok().map(|r| r.spatial_material.status().clone()))
            .unwrap_or_default()
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_debug_wireframe(&mut self, enabled: bool) {
        if let Some(renderer) = &self.renderer {
            if let Ok(mut renderer) = renderer.lock() {
                renderer.debug_wireframe = enabled;
            }
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn debug_render_metrics(&self) -> DebugRenderMetrics {
        self.renderer
            .as_ref()
            .and_then(|r| r.lock().ok().map(|r| r.debug_metrics.get()))
            .unwrap_or_default()
    }
    /// The full Cockpit owns registered-body instructions; standalone views
    /// retain their fallback hint unless their parent explicitly opts in.
    pub(crate) fn set_parent_managed_body_hud(&mut self, managed: bool) {
        self.parent_managed_body_hud = managed;
    }

    /// The caller enables this only after matching the negotiated source registration.
    pub fn set_docked_traversal_enabled(&mut self, enabled: bool) {
        self.docked_enabled = enabled;
    }
    pub fn set_head_look_input_allowed(&mut self, allowed: bool) {
        self.head_look_input_allowed = allowed;
    }
    /// Release on a hidden viewport, editor/menu ownership, or session change.
    /// Resuming mouse look then requires a deliberate clear-viewport click.
    pub(crate) fn release_first_person_mouse(&mut self, ctx: &egui::Context) {
        if self.first_person_mouse.active {
            self.first_person_mouse.suspend(ctx);
        } else {
            self.first_person_mouse.release(ctx);
            self.first_person_mouse.suspended = false;
            self.first_person_mouse.await_release = false;
        }
    }
    pub(crate) fn first_person_mouse_captured(&self) -> bool {
        self.first_person_mouse.captured
    }
    fn begin_first_person_mouse(
        &mut self,
        ui: &egui::Ui,
        rect: egui::Rect,
        active: bool,
    ) -> egui::Vec2 {
        let clear_layer = ui.ctx().pointer_latest_pos().is_some_and(|point| {
            let layer = ui.ctx().layer_id_at(point);
            let passive_body_hud =
                egui::LayerId::new(egui::Order::Foreground, egui::Id::new("body-context-hud"));
            rect.contains(point)
                && (layer == Some(ui.layer_id())
                    || self.first_person_mouse.captured && layer == Some(passive_body_hud))
        });
        let allowed = self.head_look_input_allowed
            && !self.editor_free_camera_active
            && !self.computer_keyboard_owned
            && !ui.ctx().wants_keyboard_input()
            && clear_layer;
        self.first_person_mouse.begin(ui.ctx(), active, allowed)
    }
    fn finish_first_person_mouse(&mut self, ui: &egui::Ui, rect: egui::Rect) {
        // Use THIS frame's exact projected/depth-gated faces. A screen click
        // must reach its interface rather than recapturing the camera.
        let over_clear_world = ui.ctx().pointer_latest_pos().is_some_and(|point| {
            rect.contains(point)
                && ui.ctx().layer_id_at(point) == Some(ui.layer_id())
                && !self
                    .docked_surfaces
                    .iter()
                    .flatten()
                    .any(|surface| surface.contains(point))
                && !self
                    .passive_console
                    .as_ref()
                    .is_some_and(|surface| surface.contains(point))
        });
        self.first_person_mouse.finish(ui.ctx(), over_clear_world);
    }
    #[cfg(test)]
    pub fn head_look_angles(&self) -> [f32; 2] {
        self.head_look
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn debug_view_render_metrics(&self) -> Vec<ViewRenderMetrics> {
        self.renderer
            .as_ref()
            .and_then(|renderer| {
                renderer
                    .lock()
                    .ok()
                    .map(|renderer| renderer.view_profiler.borrow().samples())
            })
            .unwrap_or_default()
    }
    pub(crate) fn computer_surface(&self) -> Option<CockpitSurface> {
        self.docked_surfaces[3].clone()
    }
    pub(crate) fn set_computer_keyboard_owned(&mut self, owned: bool) {
        self.computer_keyboard_owned = owned;
    }
    pub(crate) fn clear_cockpit_presentation(&mut self) {
        self.docked_surfaces = std::array::from_fn(|_| None);
        self.passive_console = None;
        self.docked_layer = None;
        self.mechanism_host_bound = false;
        self.reactor_host_bound = false;
        self.mechanism_feed_frame = None;
    }
    pub(crate) fn computer_hit_tester(
        &self,
        ctx: &egui::Context,
    ) -> impl Fn(egui::Pos2) -> Option<egui::Pos2> + 'static {
        let surface = self.computer_surface();
        let layer = self.docked_layer;
        let ctx = ctx.clone();
        move |point| {
            if layer.is_none() || ctx.layer_id_at(point) != layer {
                return None;
            }
            surface.as_ref()?.unproject(point)
        }
    }
    pub(crate) fn submit_computer(
        &self,
        ctx: &egui::Context,
        primitives: Vec<egui::ClippedPrimitive>,
    ) {
        if let (Some(surface), Some(layer)) = (self.computer_surface(), self.docked_layer) {
            surface.submit(ctx, layer, primitives);
        }
    }
    // Match the existing instrument painters: authority inputs remain explicit.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_docked_instruments(
        &self,
        ui: &mut egui::Ui,
        pilot: &PilotView,
        snapshot: &Snapshot,
        selected_center_distance_m: Option<f64>,
        now: u64,
        connected: bool,
        paused: bool,
    ) {
        paint_instruments(
            ui,
            &self.docked_surfaces,
            pilot,
            snapshot,
            selected_center_distance_m,
            now,
            connected && !paused,
            Some(docked_instrument_authority(
                connected,
                paused,
                now,
                snapshot.server_time_ms,
            )),
        );
        if let Some(surface) = &self.docked_surfaces[7] {
            cargo_instrument::paint(
                ui,
                surface,
                pilot,
                snapshot,
                self.drone_telemetry_time,
                self.cargo_unload.as_ref(),
            );
        }
        let mut range = 1_000_000.0;
        if let Some(surface) = &self.docked_surfaces[4] {
            // Same contact/map painter as the seated flight view. Disabled UI
            // suppresses map input; no returned action reaches gameplay.
            ui.add_enabled_ui(false, |ui| {
                let _ = paint_cockpit_maps(
                    ui,
                    surface,
                    pilot,
                    snapshot,
                    now,
                    connected,
                    paused,
                    &super::super::SelectedObject::Station,
                    &mut range,
                );
            });
        }
        if let Some(surface) = &self.docked_surfaces[5] {
            cockpit_radar::paint(ui, surface, pilot, snapshot, now, connected, paused, range);
        }
        if let Some(surface) = &self.passive_console {
            paint_passive_flight_console(ui, surface, pilot, snapshot, now, connected, paused);
        }
        if self.mechanism_host_bound {
            drone_cassette_metrics::paint(
                ui,
                &self.docked_surfaces,
                self.mechanism_pose.state(),
                pilot,
                self.drone_telemetry_time,
            );
            physical_cockpit::backgrounds(
                ui,
                &self.docked_surfaces,
                &self.mechanism_pose,
                pilot,
                now,
                docked_instrument_authority(connected, paused, now, snapshot.server_time_ms),
            );
            if let (Some((viewport, _)), Some(renderer), Some(frame)) = (
                self.surface,
                self.renderer.as_ref(),
                self.mechanism_feed_frame.as_ref(),
            ) {
                physical_cockpit_onfoot::paint(
                    ui,
                    viewport,
                    &self.docked_surfaces,
                    renderer,
                    frame,
                    pilot,
                    &self.mechanism_pose,
                );
            }
        }
        if self.reactor_host_bound {
            if let Some(surface) = &self.docked_surfaces[REACTOR_SURFACE_SLOT] {
                let authority = if !connected {
                    "DISCONNECTED"
                } else if paused {
                    "PAUSED"
                } else if now.saturating_sub(snapshot.server_time_ms) > 1500 {
                    "STALE"
                } else {
                    "LIVE"
                };
                reactor_display::paint(ui, surface, &self.reactor_pose, pilot, authority);
            }
        }
        for (index, surface) in self.docked_surfaces.iter().enumerate() {
            // These cards own their authority header; a second top-right badge
            // would obscure real data on the small physical display canvas.
            if matches!(index, 1 | 2 | 4 | 5 | 6 | 7) || (8..COCKPIT_SURFACE_SLOTS).contains(&index)
            {
                continue;
            }
            if let Some(surface) = surface {
                paint_passive_authority(ui, surface, snapshot, now, connected, paused);
            }
        }
        // The passive navigation card owns its full bounded canvas. Dedicated
        // physical camera faces remain read-only while walking.
    }
    pub(crate) fn set_space_snapshot(&mut self, snapshot: Option<Arc<Snapshot>>) {
        self.space_snapshot = snapshot;
    }
    pub(crate) fn observe_mechanisms(
        &mut self,
        snapshot: &Snapshot,
        owner: &str,
        connected: bool,
        now: u64,
    ) {
        self.mechanism_pose.observe(snapshot, owner, connected, now);
        self.reactor_pose.observe(snapshot, owner, connected, now);
    }
    pub(crate) fn disconnect_mechanisms(&mut self) {
        self.mechanism_pose.disconnect();
        self.reactor_pose.disconnect();
    }
    pub(crate) fn reset_mechanisms(&mut self) {
        self.mechanism_pose.reset_session();
        self.reactor_pose.reset_session();
    }
    pub fn input_surface(&self) -> Option<(egui::Rect, egui::LayerId)> {
        self.surface
    }
    pub fn camera_yaw(&self) -> f64 {
        self.yaw as f64
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn set_editor_free_camera(&mut self, active: bool, pose: Option<(Vec3, f32, f32)>) {
        self.editor_free_camera_active = active;
        self.editor_free_camera = pose;
    }
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn editor_camera_pose(&self) -> Option<(Vec3, f32, f32)> {
        self.last_camera_pose
    }
    pub(crate) fn set_room_layout(&mut self, layout: Option<arkline_protocol::RoomLayoutState>) {
        if let Some(next) = layout.as_ref() {
            if self
                .room_parts
                .as_ref()
                .is_some_and(|cached| cached.matches(next))
            {
                self.room_layout = layout;
                return;
            }
        }
        let layout = layout.filter(|state| state.document.validate_live().is_ok());
        self.room_parts = layout
            .as_ref()
            .and_then(pioneer_interior::LayoutPartsCache::new);
        self.room_layout = layout;
    }
    pub fn walk_turn(&self, heading: f64) -> f64 {
        if !self.walking {
            return 0.0;
        }
        let angle = (self.camera_yaw() - heading + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        if angle.abs() < 0.005 {
            0.0
        } else {
            (angle * 3.0).clamp(-1.0, 1.0)
        }
    }
    pub fn follow_walk_turn(&mut self, turn: f64, dt: f32) {
        // Keep the RMB look target aligned when keyboard input turns the body.
        // Otherwise the old camera yaw would immediately steer the avatar back.
        self.yaw += (turn * 2.0 * dt as f64) as f32;
    }
    pub fn destroy(&mut self) {
        self.clear_cockpit_presentation();
        if let Some(renderer) = self.docked_display_renderer.take() {
            if let Ok(mut renderer) = renderer.lock() {
                unsafe {
                    renderer.destroy();
                }
            }
        }
        if let Some(renderer) = self.renderer.take() {
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
        pilot: &PilotView,
        now: u64,
        live: bool,
        fleet: Option<&arkline_protocol::HangarView>,
        room: Option<&arkline_protocol::PioneerRoomState>,
        exterior: Option<&arkline_protocol::ExteriorWorldState>,
        server_time_ms: u64,
        carrier_registration: Option<&arkline_protocol::CarrierInventoryRegistration>,
    ) -> Option<HangarAction> {
        if carrier_registration.is_some()
            || pilot.carrier_inventory.is_some()
            || (pilot.ship_class == ShipClass::Pioneer
                && (self.docked_enabled
                    || pilot
                        .station_avatar
                        .as_ref()
                        .is_some_and(|avatar| avatar.docked_traversal.is_some())))
        {
            return pioneer_docked_scene::show(
                self,
                ui,
                pilot,
                now,
                live,
                fleet,
                room,
                server_time_ms,
                carrier_registration,
            );
        }
        self.docked_initialized = false;
        self.clear_cockpit_presentation();
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.surface = Some((rect, ui.layer_id()));
        let walking = pilot.station_avatar.is_some();
        let interior = pilot
            .station_avatar
            .as_ref()
            .is_some_and(|a| a.onboard_interior);
        let interior_deck = pilot.station_avatar.as_ref().map_or(1, |a| a.interior_deck);
        let reset_heading = self.fleet_mode != fleet.is_some()
            || self.walking != walking
            || self.interior != interior
            || self.class != Some(pilot.ship_class);
        if self.interior_deck != interior_deck || reset_heading {
            self.fleet_mode = fleet.is_some();
            self.interior = interior;
            self.interior_deck = interior_deck;
            self.zoom = 1.0;
            self.follow = None;
            self.follow_heading = None;
            self.gait = Default::default();
            self.pitch = if walking { 0.23 } else { 0.32 };
            // A lift changes the floor, not the direction the user is looking.
            if reset_heading {
                if let Some(avatar) = &pilot.station_avatar {
                    self.yaw = avatar.heading as f32;
                }
            }
            self.walking = walking;
            self.class = Some(pilot.ship_class);
        }
        let unobstructed = !self.editor_free_camera_active
            && ui.ctx().pointer_hover_pos().is_some_and(|p| {
                rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id())
            });
        // Response helpers also access Context: never invoke them inside an input lock.
        let dragging = !self.editor_free_camera_active
            && (response.dragged_by(egui::PointerButton::Secondary)
                || (!walking && response.dragged_by(egui::PointerButton::Primary)));
        let first_person = walking && self.on_foot_view == OnFootView::FirstPerson;
        let mouse_delta = self.begin_first_person_mouse(ui, rect, first_person);
        if first_person {
            self.yaw += mouse_delta.x * 0.007;
            self.pitch = (self.pitch + mouse_delta.y * 0.005).clamp(-0.8, 1.25);
        }
        if unobstructed {
            ui.input(|i| {
                if dragging && !first_person {
                    // Dragging right should look/turn right with the pointer.
                    self.yaw += i.pointer.delta().x * 0.007;
                    self.pitch = (self.pitch + i.pointer.delta().y * 0.005)
                        .clamp(if walking { -0.8 } else { 0.06 }, 1.25);
                }
                if !self.editor_free_camera_active {
                    self.zoom =
                        (self.zoom * (-i.smooth_scroll_delta.y * 0.002).exp()).clamp(0.45, 2.5);
                }
            });
        }
        let room_document = self.room_layout.as_ref().map(|layout| &layout.document);
        let layout = hangar_layout(pilot.ship_class);
        let target = if let Some(avatar) = &pilot.station_avatar {
            let next = Vec3::from_array(avatar.position.map(|v| v as f32));
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            self.follow_heading = Some(avatar_follow::heading(
                self.follow_heading,
                avatar.heading,
                dt,
            ));
            let smooth = self
                .follow
                .unwrap_or(next)
                .lerp(next, 1.0 - (-dt * 18.0).exp());
            self.follow = Some(smooth);
            smooth + Vec3::Y * 1.25
        } else {
            Vec3::from_array(
                fleet
                    .and_then(|f| f.berths.iter().find(|b| b.active))
                    .map_or(layout.ship_center, |b| b.position)
                    .map(|v| v as f32),
            )
        };
        let distance = if walking {
            5.5
        } else {
            pilot.ship_class.spec().length_m as f32 * 0.88
        } * self.zoom;
        // Follow the authoritative body heading, including stationary turns.
        let view_yaw = self
            .follow_heading
            .map_or(self.yaw, |heading| heading as f32);
        let forward = Vec3::new(view_yaw.cos(), 0.0, view_yaw.sin());
        let mut eye = third_person_eye(target, view_yaw, self.pitch, distance);
        eye.y = eye.y.max(0.6);
        if walking {
            eye = if interior {
                pioneer_interior::camera_with_layout(
                    target,
                    eye,
                    interior_deck,
                    room_document,
                    room,
                )
            } else if let Some(fleet) = fleet {
                fleet_hangar::camera(fleet, target, eye)
            } else {
                quarters::camera_eye(pilot.ship_class, target, eye)
            };
        } else if let Some(fleet) = fleet {
            eye = fleet_hangar::orbit_camera(fleet, target, eye);
        }
        let player_first_person = walking && self.on_foot_view == OnFootView::FirstPerson;
        let first_person = player_first_person && !self.editor_free_camera_active;
        let look_target = if player_first_person {
            eye = self.follow.unwrap() + Vec3::Y * 1.65;
            eye + forward * self.pitch.cos() - Vec3::Y * self.pitch.sin()
        } else {
            target
        };
        let player_direction = (look_target - eye).normalize_or_zero();
        self.last_camera_pose = Some((
            eye,
            player_direction.z.atan2(player_direction.x),
            player_direction.y.clamp(-1.0, 1.0).asin(),
        ));
        let mut look_target = look_target;
        if let Some((position, yaw, pitch)) = self.editor_free_camera {
            let direction = editor_camera_direction(yaw, pitch);
            eye = position;
            look_target = eye + direction;
        }
        let vp = view_projection(eye, look_target, rect.aspect_ratio());
        let interior_base = if interior {
            Some(
                self.room_parts
                    .as_ref()
                    .map(|cache| cache.parts(interior_deck))
                    .unwrap_or_else(|| pioneer_interior::cached_parts(interior_deck)),
            )
        } else {
            None
        };
        let mut exterior_instances = Vec::new();
        let mut parts = if interior {
            pioneer_interior::state_parts(interior_deck, room)
        } else if let Some(fleet) = fleet {
            let bindings: Vec<_> = fleet_hangar::visible(fleet, eye)
                .into_iter()
                .filter_map(|berth| {
                    exterior_visuals::for_vessel(exterior, &berth.id, server_time_ms, live)
                        .map(|binding| (berth, binding))
                })
                .collect();
            let hidden: Vec<_> = bindings
                .iter()
                .map(|(berth, _)| berth.id.as_str())
                .collect();
            let mut p = fleet_hangar::parts_except_hulls(fleet, eye, &hidden);
            for (berth, binding) in bindings {
                exterior_instances.push(binding.instance(
                    fleet_hangar::hull_transform(berth),
                    fleet_hangar::hull_parts(berth).collect(),
                ));
            }
            let t = Mat4::from_translation(fleet_hangar::quarters_offset());
            p.extend(
                quarters::room_parts(ShipClass::Pioneer, now)
                    .into_iter()
                    .map(|(mesh, m, c, e)| (mesh, t * m, c, e)),
            );
            p
        } else {
            let mut p = bay_parts(pilot.ship_class);
            if let Some(binding) =
                exterior_visuals::for_pilot_state(exterior, pilot, server_time_ms, live)
            {
                p.retain(|(mesh, _, _, _)| embedded_primitive(*mesh).is_none());
                let pose = Mat4::from_scale_rotation_translation(
                    Vec3::splat(100.),
                    Quat::IDENTITY,
                    Vec3::from_array(layout.ship_center.map(|v| v as f32)),
                );
                exterior_instances.push(binding.instance(pose, bay_hull_parts(pilot.ship_class)));
            }
            p.extend(quarters::room_parts(pilot.ship_class, now));
            p
        };
        if let Some(avatar) = &pilot.station_avatar {
            let position = self
                .follow
                .unwrap_or_else(|| Vec3::from_array(avatar.position.map(|v| v as f32)));
            // Body and camera use exactly the same interpolated confirmed pose.
            let mut visual_avatar = avatar.clone();
            visual_avatar.heading = self.follow_heading.unwrap_or(avatar.heading);
            let pose = self.gait.update(
                now,
                position,
                visual_avatar.heading,
                avatar.moving && live,
                avatar.seated,
            );
            if should_render_avatar(first_person, self.editor_free_camera_active, eye, position) {
                parts.extend(avatar_parts(&visual_avatar, pose, self.follow));
            }
        }
        let station_fixtures = if interior {
            None
        } else {
            fleet.and_then(fleet_hangar::fixture_lights)
        };
        if let Some(renderer) = &self.renderer {
            let renderer = renderer.clone();
            #[cfg(target_arch = "wasm32")]
            let renderer = send_wrapper::SendWrapper::new(renderer);
            ui.painter().add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                    if let Ok(mut renderer) = renderer.lock() {
                        unsafe {
                            paint_with_fixture_lighting(
                                &mut renderer,
                                vp,
                                eye,
                                interior_base.as_ref().map(|parts| parts.as_slice()),
                                &parts,
                                true,
                                &exterior_instances,
                                station_fixtures.as_deref(),
                            );
                        }
                    }
                })),
            });
        } else {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.error.as_deref().unwrap_or("Hangar unavailable"),
                egui::FontId::proportional(16.0),
                egui::Color32::LIGHT_RED,
            );
        }
        ui.painter().text(
            rect.left_top() + egui::vec2(20.0, 85.0),
            egui::Align2::LEFT_TOP,
            format!(
                "{}  /  {}\n{}",
                if interior {
                    if interior_deck == 2 {
                        "ONBOARD · DECK TWO"
                    } else {
                        "ONBOARD · DECK ONE"
                    }
                } else if fleet.is_some() {
                    "PERSONAL FLEET DOCK"
                } else {
                    "PRIVATE BAY"
                },
                pilot.ship_class.spec().name,
                if self.editor_free_camera_active {
                    "EDITOR FREE CAMERA · WASD move · E/Q or Space/C vertical · Shift fast · RMB look"
                } else if walking && self.on_foot_view == OnFootView::FirstPerson {
                    if self.first_person_mouse_captured() {
                        "ON FOOT · Mouse look · Esc cursor · W/S move · A/D turn · Q/E strafe · Space jump"
                    } else {
                        "ON FOOT · Click clear viewport for mouse look · Esc cursor · W/S move · Q/E strafe · Space jump"
                    }
                } else if walking {
                    "ON FOOT · W/S or Up/Down move · A/D or Left/Right turn · Q/E strafe · RMB look · Space jump"
                } else {
                    "SHIP INSPECTION · Drag orbit · Wheel zoom · Disembark to explore"
                }
            ),
            egui::FontId::proportional(14.0),
            egui::Color32::from_rgb(150, 220, 185),
        );
        let action = if interior {
            pioneer_interior::labels(ui, rect, vp, pilot, live, room_document);
            pioneer_interior::device_panel(
                ui,
                rect,
                vp,
                eye,
                pilot,
                room,
                room_document,
                live,
                unobstructed,
            )
        } else if let Some(fleet) = fleet {
            let terminal = quarters::fleet_terminals(ui, rect, vp, eye, pilot, unobstructed, live);
            terminal.or_else(|| {
                fleet_hangar::labels(ui, rect, vp, eye, pilot, fleet, unobstructed, live)
            })
        } else {
            quarters::terminals(ui, rect, vp, eye, pilot, unobstructed, live)
        };
        // All borrowed room-document uses finish before changing local cursor ownership.
        self.finish_first_person_mouse(ui, rect);
        action
    }
}

fn third_person_eye(target: Vec3, heading: f32, pitch: f32, distance: f32) -> Vec3 {
    let forward = Vec3::new(heading.cos(), 0.0, heading.sin());
    target - forward * distance * pitch.cos() + Vec3::Y * distance * pitch.sin()
}

pub(super) fn editor_camera_direction(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(
        yaw.cos() * pitch.cos(),
        pitch.sin(),
        yaw.sin() * pitch.cos(),
    )
}

fn should_render_avatar(
    first_person: bool,
    free_camera_active: bool,
    camera: Vec3,
    avatar_position: Vec3,
) -> bool {
    if first_person {
        return false;
    }
    if !free_camera_active {
        return true;
    }

    // The torso and head occupy a narrow vertical column above the avatar
    // root. Hide the model while the editor camera intersects that envelope.
    let offset = camera - avatar_position;
    let horizontal_radius = 0.38;
    let inside_body_height = (-0.15..=1.95).contains(&offset.y);
    let inside_body_radius =
        offset.x * offset.x + offset.z * offset.z <= horizontal_radius * horizontal_radius;
    !(inside_body_height && inside_body_radius)
}

fn view_projection(eye: Vec3, target: Vec3, aspect: f32) -> Mat4 {
    Mat4::perspective_rh_gl(
        pioneer_docked_scene::FOV_RADIANS,
        aspect.max(0.1),
        0.08,
        30000.0,
    ) * Mat4::look_at_rh(eye, target, Vec3::Y)
}

fn bay_parts(class: ShipClass) -> Vec<ModelPart> {
    cached_bay_parts(class).to_vec()
}
fn cached_bay_parts(class: ShipClass) -> &'static [ModelPart] {
    static BAYS: [std::sync::OnceLock<Vec<ModelPart>>; 7] =
        [const { std::sync::OnceLock::new() }; 7];
    let index = arkline_protocol::ALL_SHIP_CLASSES
        .iter()
        .position(|c| *c == class)
        .unwrap();
    BAYS[index].get_or_init(|| build_bay_parts(class))
}
// Keep hull instance composition shared with calibration tests. These matrices
// are the ones submitted to the meter-space hangar renderer.
fn bay_hull_parts(class: ShipClass) -> Vec<ModelPart> {
    let bay = hangar_layout(class);
    let parent = Mat4::from_scale_rotation_translation(
        Vec3::splat(100.0),
        Quat::IDENTITY,
        Vec3::from_array(bay.ship_center.map(|v| v as f32)),
    );
    ship_parts(class)
        .into_iter()
        .map(|(mesh, m, c, e)| (mesh, parent * m, c, e * 0.25))
        .collect()
}
fn build_bay_parts(class: ShipClass) -> Vec<ModelPart> {
    let bay = hangar_layout(class);
    let l = bay.half_length as f32;
    let w = bay.half_width as f32;
    let h = bay.height as f32;
    let steel = Vec3::new(0.10, 0.15, 0.17);
    let dark = Vec3::new(0.055, 0.08, 0.09);
    let green = Vec3::new(0.06, 0.65, 0.36);
    let mut p = Parts(Vec::new());
    p.draw(
        0,
        transform(
            Vec3::new(0.0, -0.4, 0.0),
            Vec3::new(l, 0.4, w),
            Quat::IDENTITY,
        ),
        steel,
        0.0,
    );
    // Cutaway gantries leave the inspection camera unobstructed while retaining bay scale.
    for side in [-1.0, 1.0] {
        p.draw(
            0,
            transform(
                Vec3::new(0.0, 0.025, side * (w - 3.0)),
                Vec3::new(l - 2.0, 0.025, 0.14),
                Quat::IDENTITY,
            ),
            green,
            1.0,
        );
        p.draw(
            0,
            transform(
                Vec3::new(side * (l - 2.0), 0.025, 0.0),
                Vec3::new(0.14, 0.025, w - 3.0),
                Quat::IDENTITY,
            ),
            green,
            1.0,
        );
        for i in 0..9 {
            let x = -l + 3.0 + (2.0 * l - 6.0) * i as f32 / 8.0;
            p.draw(
                0,
                transform(
                    Vec3::new(x, h * 0.5, side * (w - 1.0)),
                    Vec3::new(0.65, h * 0.5, 0.65),
                    Quat::IDENTITY,
                ),
                dark,
                0.0,
            );
            p.draw(
                0,
                transform(
                    Vec3::new(x, h - 1.0, side * (w - 2.0)),
                    Vec3::new(1.5, 0.15, 1.0),
                    Quat::IDENTITY,
                ),
                Vec3::new(0.55, 0.8, 0.68),
                1.0,
            );
            p.draw(
                0,
                transform(
                    Vec3::new(x, 0.03, side * (w - 7.0)),
                    Vec3::new(0.5, 0.03, 2.0),
                    Quat::IDENTITY,
                ),
                Vec3::new(0.65, 0.45, 0.13),
                0.3,
            );
        }
        p.draw(
            0,
            transform(
                Vec3::new(0.0, h - 0.5, side * (w - 1.0)),
                Vec3::new(l, 0.5, 0.7),
                Quat::IDENTITY,
            ),
            steel,
            0.0,
        );
    }
    // Real one-meter floor divisions around the boarding area make human scale legible.
    let boarding = Vec3::from_array(bay.boarding_point.map(|v| v as f32));
    for i in -10..=10 {
        p.draw(
            0,
            transform(
                boarding + Vec3::new(i as f32, 0.012, 0.0),
                Vec3::new(0.012, 0.012, 5.0),
                Quat::IDENTITY,
            ),
            Vec3::splat(0.23),
            0.1,
        );
    }
    let room = arkline_protocol::quarters_layout(class);
    p.0.retain(|(_, m, _, _)| {
        let c = m.w_axis;
        c.y < 0.0
            || c.x < room.min[0] as f32 - 1.5
            || c.x > room.max[0] as f32 + 1.5
            || c.z < room.min[2] as f32
            || c.z > room.max[2] as f32 + 1.5
    });
    p.0.extend(bay_hull_parts(class));
    p.0
}
fn avatar_parts(
    avatar: &StationAvatar,
    pose: avatar_animation::Pose,
    smooth: Option<Vec3>,
) -> Vec<ModelPart> {
    let root = Mat4::from_rotation_translation(
        Quat::from_rotation_y(-avatar.heading as f32),
        smooth.unwrap_or_else(|| Vec3::from_array(avatar.position.map(|v| v as f32))),
    );
    let mut p = Parts(Vec::new());
    let suit = Vec3::new(0.025, 0.038, 0.042);
    p.draw(
        0,
        root * transform(
            Vec3::new(0.0, 1.20, 0.0),
            Vec3::new(0.16, 0.25, 0.23),
            Quat::IDENTITY,
        ),
        suit,
        0.0,
    );
    p.draw(
        2,
        root * transform(Vec3::new(0.0, 1.64, 0.0), Vec3::splat(0.16), Quat::IDENTITY),
        Vec3::new(0.035, 0.052, 0.055),
        0.0,
    );
    p.draw(
        0,
        root * transform(
            Vec3::new(0.15, 1.66, 0.0),
            Vec3::new(0.02, 0.06, 0.12),
            Quat::IDENTITY,
        ),
        Vec3::new(0.015, 0.32, 0.12),
        0.8,
    );
    // Layered charcoal mining suit. Light accents are deliberately restrained in
    // linear space because the shared filmic shader lifts midtone albedo strongly.
    let armor = Vec3::new(0.045, 0.065, 0.065);
    let green = Vec3::new(0.012, 0.30, 0.095);
    for (position, scale, color, emission) in [
        // Collar, breastplate, belt and compact life-support/tool pack.
        (
            Vec3::new(0.0, 1.46, 0.0),
            Vec3::new(0.15, 0.035, 0.19),
            armor,
            0.0,
        ),
        (
            Vec3::new(0.17, 1.28, 0.0),
            Vec3::new(0.045, 0.16, 0.18),
            armor,
            0.0,
        ),
        (
            Vec3::new(0.01, 0.99, 0.0),
            Vec3::new(0.18, 0.055, 0.24),
            armor,
            0.0,
        ),
        (
            Vec3::new(-0.22, 1.23, 0.0),
            Vec3::new(0.11, 0.22, 0.17),
            armor,
            0.0,
        ),
        (
            Vec3::new(-0.345, 1.31, 0.0),
            Vec3::new(0.016, 0.035, 0.125),
            green,
            0.65,
        ),
        (
            Vec3::new(-0.345, 1.19, 0.0),
            Vec3::new(0.016, 0.035, 0.125),
            green,
            0.65,
        ),
        (
            Vec3::new(0.22, 1.36, 0.0),
            Vec3::new(0.012, 0.018, 0.14),
            green,
            0.7,
        ),
        // Chin guard breaks up the spherical helmet silhouette.
        (
            Vec3::new(0.115, 1.54, 0.0),
            Vec3::new(0.075, 0.045, 0.13),
            armor,
            0.0,
        ),
    ] {
        p.draw(
            0,
            root * transform(position, scale, Quat::IDENTITY),
            color,
            emission,
        );
    }
    for side in [-1.0, 1.0] {
        let foot = pose.foot(side);
        let knee = pose.knee(side);
        let hand = pose.hand(side);
        for (position, scale, color, emission) in [
            (
                Vec3::new(0.0, 1.40, side * 0.28),
                Vec3::new(0.105, 0.095, 0.075),
                armor,
                0.0,
            ),
            (
                Vec3::new(0.0, 1.435, side * 0.35),
                Vec3::new(0.08, 0.018, 0.012),
                green,
                0.7,
            ),
            (
                foot + Vec3::X * 0.035,
                Vec3::new(0.15, 0.075, 0.105),
                armor,
                0.0,
            ),
            (
                knee + Vec3::X * 0.075,
                Vec3::new(0.045, 0.08, 0.09),
                armor,
                0.0,
            ),
            (hand, Vec3::new(0.07, 0.075, 0.07), armor, 0.0),
            (
                Vec3::new(0.03, 1.01, side * 0.255),
                Vec3::new(0.07, 0.075, 0.035),
                Vec3::new(0.13, 0.095, 0.03),
                0.0,
            ),
        ] {
            p.draw(
                0,
                root * transform(position, scale, Quat::IDENTITY),
                color,
                emission,
            );
        }
        for (index, (mut a, mut b, r)) in [
            (Vec3::new(0.0, 0.94, side * 0.13), knee, 0.09),
            (knee, foot, 0.075),
            (
                Vec3::new(0.0, 1.40, side * 0.28),
                Vec3::new(0.0, 1.4, side * 0.28).lerp(hand, 0.55),
                0.07,
            ),
            (
                Vec3::new(0.0, 1.4, side * 0.28).lerp(hand, 0.55),
                hand,
                0.06,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if avatar.seated && index < 2 {
                let hip = Vec3::new(-0.32, 0.63, side * 0.13);
                let knee = Vec3::new(0.08, 0.55, side * 0.13);
                let foot = Vec3::new(0.08, 0.09, side * 0.13);
                (a, b) = if index == 0 {
                    (hip, knee)
                } else {
                    (knee, foot)
                };
            }
            if let Some(m) = segment_model(a, b, r) {
                p.draw(0, root * m, suit, 0.0);
            }
        }
    }
    if avatar.seated {
        let inverse = root.inverse();
        for (_, model, _, _) in &mut p.0 {
            let local = inverse * *model;
            let y = local.w_axis.y;
            if y > 0.92 {
                *model = root * Mat4::from_translation(Vec3::new(-0.32, -0.32, 0.0)) * local;
            }
        }
    }
    p.0
}
#[cfg(test)]
fn apply_material_preview(parts: &mut [ModelPart], preview: crate::EmbeddedMaterialPreview) {
    for (mesh, _, color, emission) in parts {
        (*color, *emission) = preview_material(*mesh, *color, *emission, Some(preview.validated()));
    }
}
#[cfg(all(test, target_os = "macos"))]
pub(super) unsafe fn paint(renderer: &mut Renderer, vp: Mat4, eye: Vec3, parts: &[ModelPart]) {
    paint_with_base(renderer, vp, eye, None, parts);
}
#[cfg(not(target_arch = "wasm32"))]
pub(super) unsafe fn paint_preview(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    parts: &[ModelPart],
    station_lighting: bool,
) {
    paint_with_lighting(renderer, vp, eye, None, parts, station_lighting, &[]);
}

#[cfg(all(test, target_os = "macos"))]
unsafe fn paint_with_base(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    base: Option<&[ModelPart]>,
    dynamic: &[ModelPart],
) {
    paint_with_lighting(renderer, vp, eye, base, dynamic, true, &[]);
}
#[cfg(not(target_arch = "wasm32"))]
unsafe fn paint_with_lighting(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    base: Option<&[ModelPart]>,
    dynamic: &[ModelPart],
    station_lighting: bool,
    exterior: &[exterior_visuals::Instance],
) {
    paint_with_fixture_lighting(
        renderer,
        vp,
        eye,
        base,
        dynamic,
        station_lighting,
        exterior,
        None,
    );
}

#[allow(clippy::too_many_arguments)]
unsafe fn paint_with_fixture_lighting(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    base: Option<&[ModelPart]>,
    dynamic: &[ModelPart],
    station_lighting: bool,
    exterior: &[exterior_visuals::Instance],
    station_fixtures: Option<&[local_lighting::Light]>,
) {
    let Some(shadow_texture) = begin_model_pass(renderer, vp, eye, station_lighting) else {
        return;
    };
    let gl = &renderer.gl;
    let mut lights: Vec<_> = base
        .into_iter()
        .flatten()
        .chain(dynamic.iter())
        .filter(|(mesh, _, color, emission)| {
            mesh_index(*mesh) == 0 && *emission >= 0.7 && color.min_element() > 0.25
        })
        .map(|(_, m, c, _)| (m.w_axis.truncate(), *c))
        .collect();
    lights.sort_by(|a, b| {
        a.0.distance_squared(eye)
            .total_cmp(&b.0.distance_squared(eye))
    });
    lights.truncate(8);
    if exterior.is_empty() && station_fixtures.is_none() {
        renderer.local_lights.upload_station(gl, &lights);
    } else {
        let source: Vec<_> = if let Some(fixtures) = station_fixtures {
            fixtures.to_vec()
        } else {
            lights
                .into_iter()
                .map(|(position, color)| local_lighting::Light {
                    position,
                    range: 30.,
                    color,
                    power: 2.5,
                })
                .collect()
        };
        let mut combined: Vec<_> = source
            .into_iter()
            .chain(
                exterior
                    .iter()
                    .flat_map(|instance| instance.lights.iter().copied()),
            )
            .collect();
        combined.sort_by(|a, b| {
            (a.position.distance(eye) - a.range)
                .max(0.)
                .total_cmp(&(b.position.distance(eye) - b.range).max(0.))
        });
        renderer.local_lights.upload_lights(gl, combined);
    }
    for &(mesh, model, color, emission) in base.into_iter().flatten().chain(dynamic.iter()) {
        renderer.draw(mesh, model, color, emission);
    }
    for instance in exterior {
        renderer.exterior_emission.set(instance.emission);
        for &(mesh, model, color, emission) in &instance.hull {
            renderer.draw(mesh, model, color, emission);
        }
        renderer.exterior_emission.set(Default::default());
        for &(mesh, model, color, emission) in &instance.fixtures {
            renderer.draw(mesh, model, color, emission);
        }
    }
    renderer.restore_shadow_texture(shadow_texture);
}

// All preview adapters share the existing material/shadow/state preparation.
// Callers keep their own geometry and lighting recipes; this only sets the pass.
type ShadowBinding = (Option<glow::Texture>, u32, Option<glow::Sampler>);
unsafe fn begin_model_pass(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    station_lighting: bool,
) -> Option<ShadowBinding> {
    renderer.prepare_spatial_material();
    #[cfg(not(target_arch = "wasm32"))]
    renderer.debug_metrics.set(Default::default());
    if renderer.destroyed {
        return None;
    }
    let gl = &renderer.gl;
    gl.disable(glow::CULL_FACE);
    gl.disable(glow::BLEND);
    gl.enable(glow::DEPTH_TEST);
    gl.depth_func(glow::LEQUAL);
    gl.depth_mask(true);
    gl.clear_depth_f32(1.0);
    gl.clear_color(0.008, 0.014, 0.019, 1.0);
    gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
    gl.use_program(Some(renderer.program));
    renderer.upload_spatial_material();
    // Hangar/inspection recipes already use metres; flight passes use 100 m units.
    gl.uniform_1_f32(renderer.uniforms.scene_meter_scale.as_ref(), 1.0);
    gl.uniform_1_f32(
        gl.get_uniform_location(renderer.program, "station_lighting")
            .as_ref(),
        if station_lighting { 1.0 } else { 0.0 },
    );
    let shadow_texture = renderer.disable_sun_shadow();
    gl.uniform_1_f32(renderer.uniforms.environment_atmosphere.as_ref(), 0.0);
    gl.uniform_3_f32(renderer.uniforms.environment_up.as_ref(), 0.0, 1.0, 0.0);
    gl.uniform_matrix_4_f32_slice(renderer.uniforms.vp.as_ref(), false, &vp.to_cols_array());
    gl.uniform_3_f32(renderer.uniforms.eye.as_ref(), eye.x, eye.y, eye.z);
    gl.uniform_1_i32(renderer.uniforms.material.as_ref(), 0);
    Some(shadow_texture)
}

/// Inspect exact shared catalog draws, retaining each runtime surface response.
#[cfg(not(target_arch = "wasm32"))]
pub(super) unsafe fn paint_inspection(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    model: &model_catalog::RuntimeInspectionModel,
    display: Mat4,
) {
    let Some(shadow_texture) = begin_model_pass(renderer, vp, eye, true) else {
        return;
    };
    renderer.exterior_emission.set(Default::default());
    let lights: Vec<_> = model
        .parts
        .iter()
        .filter_map(|draw| {
            let (mesh, transform, color, emission) = draw.part;
            (mesh_index(mesh) == 0 && emission >= 0.7 && color.min_element() > 0.25)
                .then_some(((display * transform).w_axis.truncate(), color))
        })
        .take(8)
        .collect();
    renderer.local_lights.upload_station(&renderer.gl, &lights);
    for draw in &model.parts {
        let (mesh, transform, color, emission) = draw.part;
        renderer.draw_material_range(
            mesh,
            display * transform,
            color,
            emission,
            draw.material,
            draw.range,
        );
    }
    renderer.restore_shadow_texture(shadow_texture);
}

/// A wireframe ship gives a piece's location without concealing it behind the hull.
/// Both passes use the original shared buffers, source transforms and draw ranges.
#[cfg(not(target_arch = "wasm32"))]
pub(super) unsafe fn paint_inspection_context(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    selected: &model_catalog::RuntimeInspectionModel,
    context: &model_catalog::RuntimeInspectionModel,
    display: Mat4,
) {
    let Some(shadow_texture) = begin_model_pass(renderer, vp, eye, true) else {
        return;
    };
    renderer.exterior_emission.set(Default::default());
    renderer.local_lights.upload_station(&renderer.gl, &[]);
    let wireframe = renderer.debug_wireframe;
    renderer.debug_wireframe = false;
    let gl = &renderer.gl;
    let mut polygon_modes = [0; 2];
    gl.get_parameter_i32_slice(glow::POLYGON_MODE, &mut polygon_modes);
    gl.polygon_mode(glow::FRONT_AND_BACK, glow::LINE);
    // Context lines do not obscure the selected piece, including an internal part.
    gl.depth_mask(false);
    for draw in &context.parts {
        let (mesh, transform, _, _) = draw.part;
        renderer.draw_material_range(
            mesh,
            display * transform,
            Vec3::new(0.06, 0.12, 0.14),
            0.0,
            draw.material,
            draw.range,
        );
    }
    gl.depth_mask(true);
    gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
    // Reuse the existing inspection wireframe overlay rather than modifying source
    // colors, materials or geometry to mark selection.
    renderer.debug_wireframe = true;
    for draw in &selected.parts {
        let (mesh, transform, color, emission) = draw.part;
        renderer.draw_material_range(
            mesh,
            display * transform,
            color,
            emission,
            draw.material,
            draw.range,
        );
    }
    renderer.debug_wireframe = wireframe;
    renderer
        .gl
        .polygon_mode(glow::FRONT_AND_BACK, polygon_modes[0] as u32);
    renderer.restore_shadow_texture(shadow_texture);
}

#[cfg(test)]
mod tests {
    #[test]
    fn onfoot_cassette_and_instrument_authority_distinguishes_live_paused_stale_and_offline() {
        assert_eq!(
            super::docked_instrument_authority(true, false, 1000, 1000),
            "LIVE"
        );
        assert_eq!(
            super::docked_instrument_authority(true, false, 2500, 1000),
            "LIVE"
        );
        assert_eq!(
            super::docked_instrument_authority(true, false, 2501, 1000),
            "STALE"
        );
        assert_eq!(
            super::docked_instrument_authority(true, true, 3000, 1000),
            "PAUSED"
        );
        assert_eq!(
            super::docked_instrument_authority(false, true, 3000, 1000),
            "DISCONNECTED"
        );
    }
    #[test]
    fn live_material_preview_changes_only_authored_pioneer_materials() {
        let original = bay_parts(ShipClass::Pioneer);
        let mut preview_parts = original.clone();
        let preview = crate::EmbeddedMaterialPreview {
            hull_rgb: [0.7, 0.03, 0.1],
            trim_rgb: [0.1, 0.2, 0.9],
            emissive_strength: 0.,
            ..Default::default()
        };
        apply_material_preview(&mut preview_parts, preview);
        let mut hull_changed = false;
        let mut trim_changed = false;
        let mut emission_changed = false;
        for (before, after) in original.iter().zip(&preview_parts) {
            assert_eq!(before.0, after.0);
            assert_eq!(before.1, after.1);
            match embedded_primitive(before.0) {
                None => assert_eq!(
                    before, after,
                    "station and dynamic geometry must not change"
                ),
                Some(p) => {
                    if p.material_name == "Graphite ceramic armor" {
                        assert_eq!(after.2, Vec3::from_array(preview.hull_rgb));
                        hull_changed |= before.2 != after.2;
                    } else if p.material_name == "Emerald structural frames" {
                        assert_eq!(after.2, Vec3::from_array(preview.trim_rgb));
                        trim_changed |= before.2 != after.2;
                    } else {
                        assert_eq!(before.2, after.2);
                    }
                    assert_eq!(after.3, 0.);
                    emission_changed |= before.3 > 0.;
                }
            }
        }
        assert!(hull_changed && trim_changed && emission_changed);
        assert_eq!(
            original,
            bay_parts(ShipClass::Pioneer),
            "preview must not mutate cached authored scene"
        );
    }
    #[test]
    fn cached_bays_preserve_every_transform_and_material() {
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            assert_eq!(super::bay_parts(class), super::build_bay_parts(class));
            assert!(std::ptr::eq(
                super::cached_bay_parts(class),
                super::cached_bay_parts(class)
            ));
        }
    }
    #[test]
    #[ignore = "Explicit CPU preparation measurement, not a GPU frame-rate benchmark"]
    fn static_bay_preparation_profile() {
        use std::hint::black_box;
        let class = arkline_protocol::ShipClass::Pioneer;
        black_box(super::bay_parts(class));
        let start = std::time::Instant::now();
        for _ in 0..2000 {
            black_box(super::build_bay_parts(class));
        }
        let rebuilt = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..2000 {
            black_box(super::bay_parts(class));
        }
        eprintln!(
            "2000 actual bay preparation calls: rebuild={rebuilt:?}, cached_copy={:?}, parts={}",
            start.elapsed(),
            super::bay_parts(class).len()
        );
    }
    use super::*;
    #[test]
    fn editor_camera_pose_uses_fly_angles_and_preserves_player_camera_pose() {
        let forward = editor_camera_direction(0.0, 0.0);
        assert!(forward.distance(Vec3::X) < 1e-6);
        let up = editor_camera_direction(0.0, std::f32::consts::FRAC_PI_2);
        assert!(up.distance(Vec3::Y) < 1e-6);

        let ctx = egui::Context::default();
        let pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"FreecamQA","credits":0,"ore":0,"location":Location::Docked
        }))
        .unwrap();
        let pose = (Vec3::new(12.0, 8.0, -4.0), 1.2, -0.25);
        let mut view = HangarView::new(None);
        view.set_editor_free_camera(true, Some(pose));
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                view.ui(ui, &pilot, 1000, true, None, None, None, 1000, None);
            });
        });
        let camera = view.editor_camera_pose().unwrap();
        assert!(camera.0.is_finite());
        assert!(camera.0.distance(pose.0) > 1.0);
        assert_eq!(view.editor_free_camera, Some(pose));
    }

    #[test]
    fn free_camera_hides_avatar_only_while_camera_overlaps_body_or_head() {
        let avatar = Vec3::new(4.0, 0.0, -2.0);
        let seeded_first_person_eye = avatar + Vec3::Y * 1.65;

        // Regular first person continues hiding the body and regular third
        // person continues showing it, independent of the proximity helper.
        assert!(!should_render_avatar(
            true,
            false,
            seeded_first_person_eye,
            avatar
        ));
        assert!(should_render_avatar(
            false,
            false,
            seeded_first_person_eye,
            avatar
        ));

        // The editor camera begins at the player's eye, then restores the
        // avatar as soon as it clears the body/head envelope.
        assert!(!should_render_avatar(
            false,
            true,
            seeded_first_person_eye,
            avatar
        ));
        assert!(should_render_avatar(
            false,
            true,
            seeded_first_person_eye + Vec3::Z * 0.5,
            avatar
        ));
        assert!(should_render_avatar(
            false,
            true,
            avatar + Vec3::Y * 2.1,
            avatar
        ));
    }

    #[test]
    fn third_person_remains_behind_avatar_at_every_heading() {
        for heading in [0.0, 1.2, -2.7, std::f32::consts::PI] {
            let target = Vec3::new(8.0, 1.25, 7.0);
            let forward = Vec3::new(heading.cos(), 0.0, heading.sin());
            for pitch in [-0.8, 0.23, 1.25] {
                let eye = third_person_eye(target, heading, pitch, 5.5);
                assert!((eye - target).dot(forward) < 0.0);
                assert!((eye - target).cross(Vec3::Y).dot(forward).abs() < 0.00001);
                let clipped = pioneer_interior::camera(target, eye);
                assert!((clipped - target).dot(forward) <= 0.0);
            }
        }
    }
    #[test]
    fn hovered_and_dragged_egui_frames_finish_without_context_reentry() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ctx = egui::Context::default();
            let mut hangar = HangarView::new(None);
            let pilot: PilotView = serde_json::from_value(serde_json::json!({
                "name":"HangarQA","credits":0,"ore":0,"location":Location::Docked
            }))
            .unwrap();
            let initial = hangar.camera_yaw();
            for (frame, events) in [
                vec![],
                vec![egui::Event::PointerMoved(egui::pos2(300.0, 300.0))],
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(300.0, 300.0),
                    button: egui::PointerButton::Secondary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                }],
                vec![egui::Event::PointerMoved(egui::pos2(350.0, 320.0))],
                vec![egui::Event::PointerButton {
                    pos: egui::pos2(350.0, 320.0),
                    button: egui::PointerButton::Secondary,
                    pressed: false,
                    modifiers: egui::Modifiers::default(),
                }],
            ]
            .into_iter()
            .enumerate()
            {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(900.0, 650.0),
                        )),
                        time: Some(frame as f64 / 60.0),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            hangar.ui(ui, &pilot, 1000, true, None, None, None, 1000, None)
                        });
                    },
                );
            }
            tx.send(hangar.camera_yaw() > initial).unwrap();
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("Hangar pointer frame deadlocked"));
    }
    #[test]
    fn fleet_coordinate_switch_discards_camera_history() {
        let ctx = egui::Context::default();
        let pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"SwitchQA","credits":0,"ore":0,"location":Location::Docked
        }))
        .unwrap();
        let fleet = arkline_protocol::HangarView {
            layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
            layout: arkline_protocol::fleet_hangar_layout(),
            berths: vec![],
        };
        let mut view = HangarView::new(None);
        view.class = Some(pilot.ship_class);
        for fleet_mode in [true, false] {
            view.follow = Some(Vec3::splat(10000.));
            view.follow_heading = Some(2.);
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.ui(
                        ui,
                        &pilot,
                        1000,
                        true,
                        fleet_mode.then_some(&fleet),
                        None,
                        None,
                        1000,
                        None,
                    );
                });
            });
            assert_eq!(view.fleet_mode, fleet_mode);
            assert!(view.follow.is_none());
            assert!(view.follow_heading.is_none());
        }
    }
    #[test]
    fn avatar_has_human_scale_and_walk_articulation() {
        let mut avatar = StationAvatar {
            sprinting: false,
            docked_traversal: None,
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: [0.0; 3],
            heading: 0.0,
            moving: false,
            updated_at_ms: 0,
        };
        let still = avatar_parts(&avatar, Default::default(), None);
        let (_, max) = parts_bounds(&still);
        assert!((max.y - 1.8).abs() < 0.01);
        avatar.moving = true;
        let walking = avatar_parts(
            &avatar,
            avatar_animation::Pose {
                phase: 0.1,
                weight: 1.0,
                direction: Vec3::X,
            },
            None,
        );
        assert!(still.iter().zip(&walking).any(|(a, b)| a.1 != b.1));
        assert!(walking.iter().all(|p| p.1.is_finite()));
    }
    #[test]
    fn every_bay_preserves_real_hull_geometry() {
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            let parts = bay_parts(class);
            assert!(parts.iter().all(|p| p.1.is_finite()));
            let bay = hangar_layout(class);
            assert!(bay.boarding_point[2] > class.spec().width_m / 2.0);
            let parent = Mat4::from_scale(Vec3::splat(100.0));
            let hull: Vec<_> = ship_parts(class)
                .into_iter()
                .map(|(a, b, c, d)| (a, parent * b, c, d))
                .collect();
            let (min, max) = parts_bounds(&hull);
            assert!(((max.x - min.x) as f64 - class.spec().length_m).abs() < 1.0);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "Explicit offscreen native hangar render; no app window"]
    fn native_hangar_renders_real_geometry() {
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
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 128, 128);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);

            let eye = Vec3::new(60.0, 40.0, 65.0);
            let vp = Mat4::perspective_rh_gl(1.0, 1.0, 0.08, 30000.0)
                * Mat4::look_at_rh(eye, Vec3::new(0.0, 5.0, 0.0), Vec3::Y);
            paint(&mut renderer, vp, eye, &bay_parts(ShipClass::Pioneer));
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
            let lit = pixels.chunks_exact(4).filter(|p| p[1] > 25).count();
            assert!(lit > 500, "Hangar must cover meaningful image area: {lit}");
            if let Ok(directory) = std::env::var("ARKLINE_ROOM_DEVICE_CAPTURE") {
                std::fs::create_dir_all(&directory).unwrap();
                let capture = CachedFeed::create(&gl, 1006, 1280, 720).unwrap();
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(capture.framebuffer));
                gl.viewport(0, 0, 1280, 720);
                let eye = Vec3::new(11.8, 1.65, 4.8);
                let vp = Mat4::perspective_rh_gl(1.3, 1280. / 720., 0.08, 100.)
                    * Mat4::look_at_rh(eye, Vec3::new(6.1, 1.8, 2.7), Vec3::Y);
                let mut images = Vec::new();
                for mode in ["normal", "off", "emergency"] {
                    let mut state =
                        arkline_protocol::PioneerRoomState::uncommissioned("capture".into());
                    state.commissioned = true;
                    state.breaker_closed = mode == "normal";
                    state.light_on = mode == "normal";
                    state.emergency_light_on = mode == "emergency";
                    state.door_open = false;
                    paint(
                        &mut renderer,
                        vp,
                        eye,
                        &pioneer_interior::parts_with_room(1, Some(&state)),
                    );
                    let mut rgba = vec![0u8; 1280 * 720 * 4];
                    gl.read_pixels(
                        0,
                        0,
                        1280,
                        720,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut rgba)),
                    );
                    assert_eq!(gl.get_error(), glow::NO_ERROR);
                    let mut ppm = b"P6\n1280 720\n255\n".to_vec();
                    for row in rgba.chunks_exact(1280 * 4).rev() {
                        for pixel in row.chunks_exact(4) {
                            ppm.extend_from_slice(&pixel[..3]);
                        }
                    }
                    std::fs::write(
                        std::path::Path::new(&directory).join(format!("{mode}.ppm")),
                        ppm,
                    )
                    .unwrap();
                    images.push(rgba);
                }
                assert_ne!(
                    images[0], images[1],
                    "Confirmed main light must affect rendering"
                );
                assert_ne!(
                    images[1], images[2],
                    "Emergency output must affect rendering"
                );
                capture.destroy(&gl);
            }
            if let Ok(path) = std::env::var("ARKLINE_PIONEER_INTERIOR_CAPTURE") {
                let capture = CachedFeed::create(&gl, 1002, 1280, 720).unwrap();
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(capture.framebuffer));
                gl.viewport(0, 0, 1280, 720);
                let eye = Vec3::new(5.0, 1.7, 7.0);
                let vp = Mat4::perspective_rh_gl(1.2, 1280.0 / 720.0, 0.08, 100.0)
                    * Mat4::look_at_rh(eye, Vec3::new(20.0, 1.4, 7.0), Vec3::Y);
                for deck in [1, 2] {
                    let (eye, vp) = if deck == 2 {
                        let eye = Vec3::new(10.6, 2.2, 12.8);
                        (
                            eye,
                            Mat4::perspective_rh_gl(1.2, 1280.0 / 720.0, 0.08, 100.0)
                                * Mat4::look_at_rh(eye, Vec3::new(6.2, 1.0, 9.6), Vec3::Y),
                        )
                    } else {
                        (eye, vp)
                    };
                    let mut parts = pioneer_interior::parts(deck);
                    if deck == 2 {
                        let avatar = StationAvatar {
                            sprinting: false,
                            docked_traversal: None,
                            onboard_interior: true,
                            interior_deck: 2,
                            jump_started_ms: None,
                            seated: false,
                            position: [8.5, 0.0, 11.0],
                            heading: -std::f64::consts::FRAC_PI_2,
                            moving: false,
                            updated_at_ms: 0,
                        };
                        parts.extend(avatar_parts(&avatar, Default::default(), None));
                    }
                    paint(&mut renderer, vp, eye, &parts);
                    let mut rgba = vec![0u8; 1280 * 720 * 4];
                    gl.read_pixels(
                        0,
                        0,
                        1280,
                        720,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut rgba)),
                    );
                    assert_eq!(gl.get_error(), glow::NO_ERROR);
                    let mut ppm = b"P6\n1280 720\n255\n".to_vec();
                    for row in rgba.chunks_exact(1280 * 4).rev() {
                        for pixel in row.chunks_exact(4) {
                            ppm.extend_from_slice(&pixel[..3]);
                        }
                    }
                    std::fs::write(
                        if deck == 1 {
                            path.clone()
                        } else {
                            format!("{path}.deck-two.ppm")
                        },
                        ppm,
                    )
                    .unwrap();
                }
                capture.destroy(&gl);
            }
            if let Ok(directory) = std::env::var("ARKLINE_FLEET_HANGAR_CAPTURE") {
                std::fs::create_dir_all(&directory).unwrap();
                let capture = CachedFeed::create(&gl, 1005, 1280, 720).unwrap();
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(capture.framebuffer));
                gl.viewport(0, 0, 1280, 720);
                let fleet = arkline_protocol::HangarView {
                    layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
                    layout: arkline_protocol::fleet_hangar_layout(),
                    berths: arkline_protocol::ALL_SHIP_CLASSES
                        .iter()
                        .enumerate()
                        .map(|(i, c)| {
                            let (position, boarding_point) = arkline_protocol::fleet_berth(*c);
                            arkline_protocol::HangarBerth {
                                id: format!("fixture-{i}"),
                                ship_class: *c,
                                position,
                                yaw: 0.,
                                boarding_point,
                                active: i == 0,
                                can_board: true,
                            }
                        })
                        .collect(),
                };
                for (name, eye, aim) in [
                    (
                        "pioneer-access",
                        Vec3::new(0., 1.65, -7.),
                        Vec3::new(0., 8., 25.),
                    ),
                    (
                        "concourse",
                        Vec3::new(-45., 4., -10.),
                        Vec3::new(260., 6., 10.),
                    ),
                    (
                        "overview",
                        Vec3::new(450., 500., -700.),
                        Vec3::new(700., 80., 130.),
                    ),
                ] {
                    let vp = Mat4::perspective_rh_gl(1.1, 1280. / 720., 0.08, 30000.)
                        * Mat4::look_at_rh(eye, aim, Vec3::Y);
                    let mut parts = fleet_hangar::parts(&fleet, eye);
                    let t = Mat4::from_translation(fleet_hangar::quarters_offset());
                    parts.extend(
                        quarters::room_parts(ShipClass::Pioneer, 1000)
                            .into_iter()
                            .map(|(mesh, m, c, e)| (mesh, t * m, c, e)),
                    );
                    paint(&mut renderer, vp, eye, &parts);
                    let mut rgba = vec![0; 1280 * 720 * 4];
                    gl.read_pixels(
                        0,
                        0,
                        1280,
                        720,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut rgba)),
                    );
                    assert_eq!(gl.get_error(), glow::NO_ERROR);
                    let mut ppm = b"P6\n1280 720\n255\n".to_vec();
                    for row in rgba.chunks_exact(1280 * 4).rev() {
                        for pixel in row.chunks_exact(4) {
                            ppm.extend_from_slice(&pixel[..3]);
                        }
                    }
                    std::fs::write(
                        std::path::Path::new(&directory).join(format!("{name}.ppm")),
                        ppm,
                    )
                    .unwrap();
                }
                capture.destroy(&gl);
            }
            if let Ok(path) = std::env::var("ARKLINE_HANGAR_CAPTURE") {
                let capture = CachedFeed::create(&gl, 1000, 1280, 720).unwrap();
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(capture.framebuffer));
                gl.viewport(0, 0, 1280, 720);
                let q = arkline_protocol::quarters_layout(ShipClass::Pioneer);
                let eye = Vec3::new(-1.5, 2.4, q.min[2] as f32 + 0.9);
                let aim = Vec3::new(1.0, 1.1, q.max[2] as f32 - 2.0);
                let vp = Mat4::perspective_rh_gl(1.3, 1280.0 / 720.0, 0.08, 30000.0)
                    * Mat4::look_at_rh(eye, aim, Vec3::Y);
                let mut parts = bay_parts(ShipClass::Pioneer);
                parts.extend(quarters::room_parts(ShipClass::Pioneer, 1000));
                paint(&mut renderer, vp, eye, &parts);
                let mut rgba = vec![0u8; 1280 * 720 * 4];
                gl.read_pixels(
                    0,
                    0,
                    1280,
                    720,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut rgba)),
                );
                let mut ppm = b"P6\n1280 720\n255\n".to_vec();
                for row in rgba.chunks_exact(1280 * 4).rev() {
                    for pixel in row.chunks_exact(4) {
                        ppm.extend_from_slice(&pixel[..3]);
                    }
                }
                std::fs::write(&path, ppm).unwrap();
                let eye = Vec3::new(0.0, 2.1, q.max[2] as f32 - 2.0);
                let aim = Vec3::new(0.0, 1.7, q.min[2] as f32 - 8.0);
                let vp = Mat4::perspective_rh_gl(1.3, 1280.0 / 720.0, 0.08, 30000.0)
                    * Mat4::look_at_rh(eye, aim, Vec3::Y);
                paint(&mut renderer, vp, eye, &parts);
                gl.read_pixels(
                    0,
                    0,
                    1280,
                    720,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut rgba)),
                );
                let mut ppm = b"P6\n1280 720\n255\n".to_vec();
                for row in rgba.chunks_exact(1280 * 4).rev() {
                    for pixel in row.chunks_exact(4) {
                        ppm.extend_from_slice(&pixel[..3]);
                    }
                }
                std::fs::write(format!("{path}.overlook.ppm"), ppm).unwrap();
                capture.destroy(&gl);
            }
            if let Ok(directory) = std::env::var("ARKLINE_GAIT_CAPTURE_DIR") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let tile = 384usize;
                let capture = CachedFeed::create(&gl, 1001, tile as i32, tile as i32).unwrap();
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(capture.framebuffer));
                gl.viewport(0, 0, tile as i32, tile as i32);
                let avatar = StationAvatar {
                    sprinting: false,
                    docked_traversal: None,
                    onboard_interior: false,
                    interior_deck: 1,
                    jump_started_ms: None,
                    position: [0.0; 3],
                    heading: 0.0,
                    moving: true,
                    seated: false,
                    updated_at_ms: 0,
                };
                let eye = Vec3::new(2.8, 1.8, 3.0);
                let vp = Mat4::perspective_rh_gl(0.7, 1.0, 0.08, 100.0)
                    * Mat4::look_at_rh(eye, Vec3::new(0.0, 0.9, 0.0), Vec3::Y);
                let mut sheet = vec![0u8; tile * 4 * tile * 2 * 3];
                for index in 0..8 {
                    let mut parts = avatar_parts(
                        &avatar,
                        avatar_animation::Pose {
                            phase: index as f32 / 8.0,
                            weight: 1.0,
                            direction: Vec3::X,
                        },
                        None,
                    );
                    parts.push((
                        0,
                        transform(
                            Vec3::new(0.0, -0.04, 0.0),
                            Vec3::new(2.0, 0.04, 2.0),
                            Quat::IDENTITY,
                        ),
                        Vec3::new(0.045, 0.07, 0.07),
                        0.0,
                    ));
                    paint(&mut renderer, vp, eye, &parts);
                    let mut rgba = vec![0u8; tile * tile * 4];
                    gl.read_pixels(
                        0,
                        0,
                        tile as i32,
                        tile as i32,
                        glow::RGBA,
                        glow::UNSIGNED_BYTE,
                        glow::PixelPackData::Slice(Some(&mut rgba)),
                    );
                    assert_eq!(gl.get_error(), glow::NO_ERROR);
                    let mut ppm = format!("P6\n{tile} {tile}\n255\n").into_bytes();
                    for (y, row) in rgba.chunks_exact(tile * 4).rev().enumerate() {
                        for (x, pixel) in row.chunks_exact(4).enumerate() {
                            ppm.extend_from_slice(&pixel[..3]);
                            let offset =
                                ((index / 4 * tile + y) * tile * 4 + index % 4 * tile + x) * 3;
                            sheet[offset..offset + 3].copy_from_slice(&pixel[..3]);
                        }
                    }
                    std::fs::write(directory.join(format!("gait-{index:02}.ppm")), ppm).unwrap();
                }
                let mut ppm = format!("P6\n{} {}\n255\n", tile * 4, tile * 2).into_bytes();
                ppm.extend(sheet);
                std::fs::write(directory.join("gait-contact-sheet.ppm"), ppm).unwrap();
                capture.destroy(&gl);
            }
            renderer.destroy();
            target.destroy(&gl);
            CGLSetCurrentContext(std::ptr::null_mut());
            CGLDestroyContext(context);
        }
    }
}

#[cfg(test)]
mod first_person_mouse_tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        state: &mut FirstPersonMouseLook,
        events: Vec<egui::Event>,
        focused: bool,
        allowed: bool,
        clear_world: bool,
    ) -> (egui::Vec2, egui::FullOutput) {
        let mut delta = egui::Vec2::ZERO;
        let output = ctx.run(
            egui::RawInput {
                focused,
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| {
                delta = state.begin(ctx, true, allowed);
                state.finish(ctx, clear_world);
            },
        );
        (delta, output)
    }
    fn key(key: egui::Key, pressed: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        }
    }
    fn press() -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(egui::pos2(200.0, 200.0)),
            egui::Event::PointerButton {
                pos: egui::pos2(200.0, 200.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]
    }
    #[test]
    fn first_person_defaults_to_mouse_look_and_escape_requires_deliberate_recapture() {
        let ctx = egui::Context::default();
        let mut state = FirstPersonMouseLook::default();
        let (delta, output) = frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerMoved(egui::pos2(200.0, 200.0))],
            true,
            true,
            true,
        );
        assert_eq!(delta, egui::Vec2::ZERO);
        assert!(state.captured);
        assert!(output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(
                command,
                egui::ViewportCommand::CursorGrab(egui::CursorGrab::Locked)
            )));
        let (delta, _) = frame(
            &ctx,
            &mut state,
            vec![egui::Event::MouseMoved(egui::vec2(12.0, -4.0))],
            true,
            true,
            true,
        );
        assert_eq!(delta, egui::vec2(12.0, -4.0));
        let (delta, output) = frame(
            &ctx,
            &mut state,
            vec![key(egui::Key::Escape, true)],
            true,
            true,
            true,
        );
        assert_eq!(delta, egui::Vec2::ZERO);
        assert!(!state.captured);
        assert!(output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(
                command,
                egui::ViewportCommand::CursorGrab(egui::CursorGrab::None)
            )));
        frame(
            &ctx,
            &mut state,
            vec![key(egui::Key::Escape, false)],
            true,
            true,
            true,
        );
        assert!(!state.captured, "Escape must not immediately recapture");
        frame(&ctx, &mut state, press(), true, true, true);
        assert!(state.captured);
    }
    #[test]
    fn first_person_focus_or_ui_ownership_releases_and_screen_click_does_not_recapture() {
        let ctx = egui::Context::default();
        let mut state = FirstPersonMouseLook::default();
        frame(&ctx, &mut state, vec![], true, true, true);
        assert!(state.captured);
        let (delta, _) = frame(
            &ctx,
            &mut state,
            vec![egui::Event::MouseMoved(egui::vec2(8.0, 9.0))],
            false,
            true,
            true,
        );
        assert_eq!(delta, egui::Vec2::ZERO);
        assert!(!state.captured);
        frame(&ctx, &mut state, vec![], true, true, true);
        assert!(!state.captured, "Focus recovery requires an explicit click");
        frame(&ctx, &mut state, press(), true, true, false);
        assert!(!state.captured, "An actual screen hit keeps its cursor");
        frame(
            &ctx,
            &mut state,
            vec![egui::Event::PointerButton {
                pos: egui::pos2(200.0, 200.0),
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
            true,
            true,
            true,
        );
        frame(&ctx, &mut state, press(), true, true, true);
        assert!(state.captured);
        let (delta, _) = frame(
            &ctx,
            &mut state,
            vec![egui::Event::MouseMoved(egui::vec2(8.0, 9.0))],
            true,
            false,
            true,
        );
        assert_eq!(delta, egui::Vec2::ZERO);
        assert!(
            !state.captured,
            "Menu/text/editor ownership releases mouse look"
        );
    }
}
