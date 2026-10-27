//! One actual source-mounted crane feed, using the existing workspace cache.
use super::*;
use crate::carrier_camera_geometry::{CameraHealth, Confirmed, Pose};
use crate::carrier_mounted_feed::{Installed, ResolvedFeed, Resolver};
use std::hash::{Hash, Hasher};

const DEFINITION_SHA: &str = "425597be16b9c252d0071d5ab1e4d2afb03d5905dc0b2fbe99fe955c94824f0d";
#[derive(Default)]
struct Pending {
    retire: Vec<u64>,
    invalidate: Vec<u64>,
}
#[derive(Default)]
pub(super) struct State {
    resolver: Resolver,
    pending: Arc<Mutex<Pending>>,
    active_key: Option<u64>,
    fingerprint: Option<(u64, u64)>,
}
impl State {
    pub(super) fn reset(&mut self) {
        if let Some(key) = self.resolver.reset_session().or(self.active_key.take()) {
            self.retire(key);
        }
        self.active_key = None;
        self.fingerprint = None;
    }
    fn retire(&self, key: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            if !pending.retire.contains(&key) {
                pending.retire.push(key);
            }
        }
    }
    pub(super) fn suppress(&mut self) {
        if let Some(key) = self.active_key.take() {
            self.retire(key);
        }
    }
    fn invalidate(&self, key: u64) {
        if let Ok(mut pending) = self.pending.lock() {
            if !pending.invalidate.contains(&key) {
                pending.invalidate.push(key);
            }
        }
    }
    fn changed_equal_time(&mut self, snapshot: &Snapshot) -> Result<bool, String> {
        // The entire accepted response scene participates, not just camera pose:
        // same-time FIFO updates can change stock, doors, lights or other vessels.
        let bytes = serde_json::to_vec(snapshot).map_err(|e| e.to_string())?;
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hash);
        let next = (snapshot.server_time_ms, hash.finish());
        let changed = self
            .fingerprint
            .is_some_and(|old| old.0 == next.0 && old.1 != next.1);
        self.fingerprint = Some(next);
        Ok(changed)
    }
}
unsafe fn flush_pending(renderer: &mut Renderer, queue: &Mutex<Pending>) {
    if let Ok(mut pending) = queue.lock() {
        for key in pending.retire.drain(..) {
            if let Some(index) = renderer.feed_cache.iter().position(|c| c.key == key) {
                renderer.feed_cache.remove(index).destroy(&renderer.gl);
            }
            if renderer.workspace_feed_key == Some(key) {
                renderer.workspace_feed_key = None;
            }
        }
        for key in pending.invalidate.drain(..) {
            if let Some(cache) = renderer.feed_cache.iter_mut().find(|c| c.key == key) {
                cache.painted_ms = None;
                cache.captured_ui_ms = None;
            }
        }
    }
}
const DEVICE_CAPTION: &str = "Mounted crane camera";
const SUPPLY_CAPTION: &str = "Unmetered · health not simulated";
fn mounted_status(paused: bool, image_age: Option<u64>) -> String {
    match (paused, image_age) {
        (true, Some(age)) => format!("PAUSED ({age} ms)"),
        (true, None) => "PAUSED · no image".into(),
        (false, None) => "NO IMAGE".into(),
        (false, Some(age)) if age <= 250 => format!("LIVE ({age} ms)"),
        (false, Some(age)) => format!("CACHED ({age} ms)"),
    }
}
fn caption_height(ui: &egui::Ui, width: f32, paused: bool, image_age: Option<u64>) -> f32 {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let measure = |text: String| {
        ui.painter()
            .layout(text, font.clone(), egui::Color32::WHITE, width)
            .size()
            .y
    };
    // A resized/reselected/invalidated image may be cold even if the prior key
    // had pixels. Reserve both actual status and cold status at the same font.
    let status = [image_age, None]
        .into_iter()
        .map(|age| {
            measure(format!(
                "{DEVICE_CAPTION} · {}",
                mounted_status(paused, age)
            ))
        })
        .fold(0., f32::max);
    status + ui.spacing().item_spacing.y
}
fn image_height(width: f32, visible_height: f32, captions: f32) -> Option<f32> {
    if ![width, visible_height, captions]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    let preferred = (width * 9. / 16.).min(360.);
    let room = visible_height - captions;
    (preferred >= 96. && room >= 96.).then_some(preferred.min(room))
}
fn letterbox(rect: egui::Rect) -> egui::Rect {
    let width = rect.width().min(rect.height() * 16. / 9.).max(0.);
    egui::Rect::from_center_size(rect.center(), egui::vec2(width, width * 9. / 16.))
}
fn build_frame(
    snapshot: &Snapshot,
    pilot: &PilotView,
    parent: arkline_protocol::drone_bay_geometry::BayPose,
    resolved: ResolvedFeed,
    paused: bool,
    lighting_quality: crate::lighting_settings::LightingQuality,
) -> Result<Frame, String> {
    let (space, rotation, sample_time) = confirmed_carrier_parent(parent, resolved.scene_time_ms)?;
    if sample_time != snapshot.server_time_ms {
        return Err("Camera and scene clocks differ".into());
    }
    let eye = space.scene(resolved.camera.eye_world_m);
    let target = space.scene(resolved.camera.target_world_m);
    let up = Vec3::from_array(resolved.camera.up_world.map(|v| v as f32));
    let view = Mat4::look_at_rh(eye, target, up);
    let fov = resolved.camera.vertical_fov_radians as f32;
    let aspect = 16. / 9.;
    let vp = Mat4::perspective_rh_gl(
        fov,
        aspect,
        resolved.camera.near_m as f32 / 100.,
        resolved.camera.far_m as f32 / 100.,
    ) * view;
    if !vp.is_finite() {
        return Err("Mounted camera projection is invalid".into());
    }
    let system = pilot.system_id;
    Ok(Frame {
        pioneer_parts: None,
        exterior: snapshot.exterior.clone().map(Arc::new),
        lighting_quality,
        cockpit_vp: vp,
        cockpit_occlusion: None,
        snapshot_time: snapshot.server_time_ms,
        paused,
        planet_vp: Mat4::perspective_rh_gl(fov, aspect, 1., 300_000.) * view,
        system_id: system,
        ship_class: pilot.ship_class,
        drone_camera: true,
        camera_drone: None,
        scene: Arc::new(FrameScene {
            physical: None,
            carrier_inventory_registration: snapshot.carrier_inventory_registration.clone(),
            world_layout: crate::snapshot_world_layout(snapshot)?.clone(),
            traffic: snapshot
                .traffic
                .iter()
                .filter(|a| a.system_id == system)
                .cloned()
                .collect(),
            surface_props: snapshot
                .surface_props
                .iter()
                .filter(|a| a.system_id == system)
                .cloned()
                .collect(),
            asteroids: snapshot
                .asteroids
                .iter()
                .filter(|a| a.system_id == system)
                .cloned()
                .collect(),
            gates: gates()
                .into_iter()
                .filter(|a| a.system_id == system)
                .collect(),
            pilots: snapshot
                .pilots
                .iter()
                .filter(|a| a.system_id == system)
                .cloned()
                .collect(),
        }),
        space,
        cockpit: false,
        rotation,
        connected: true,
        vp,
        eye,
        now: sample_time,
        own: pilot.name.clone(),
        docking: None,
    })
}
/// Native capture and UI share this exact confirmed-device path. This is not
/// a documentary/source-preview pose constructor.
#[allow(clippy::too_many_arguments)]
pub(super) fn resolve_frame(
    monitor: &mut State,
    snapshot: &Snapshot,
    viewed_name: &str,
    now: u64,
    connected: bool,
    paused: bool,
    selected_device: &str,
    visible: bool,
    dimensions: [u32; 2],
    lighting_quality: crate::lighting_settings::LightingQuality,
) -> Result<Option<(Frame, u64)>, String> {
    let pilot = snapshot
        .pilots
        .iter()
        .find(|p| p.name == viewed_name)
        .ok_or("Viewed vessel unavailable")?;
    let public = pilot
        .carrier_inventory
        .as_ref()
        .ok_or("Carrier projection unavailable")?;
    public.validate_projection(pilot)?;
    if public.sample_time_ms != snapshot.server_time_ms
        || snapshot.carrier_inventory_registration.as_ref() != Some(&public.binding().registration)
    {
        return Err("Carrier registration or sample clock differs".into());
    }
    let geometry = flight_geometry()?;
    if !geometry
        .carriers
        .iter()
        .filter_map(|c| c.as_ref().ok())
        .any(|c| c.registration == public.binding().registration)
    {
        return Err("Exact installed carrier source unavailable".into());
    }
    let cameras = public.cameras.as_ref();
    let device = cameras.and_then(|c| c.devices.iter().find(|d| d.device_id == selected_device));
    let state = cameras.zip(device).and_then(|(c, d)| {
        d.parent_world_pose.map(|parent| Confirmed {
            vessel_id: &c.vessel_id,
            device_id: &d.device_id,
            source_sha256: &c.source_sha256,
            device_revision: c.device_revision,
            parent_id: &d.parent_id,
            parent: Pose {
                position_m: parent.position_m,
                orientation_xyzw: parent.orientation_xyzw,
            },
            sampled_at_ms: c.sampled_at_ms,
            installed: true,
            powered: d.availability
                == arkline_protocol::carrier_camera_devices::CameraAvailability::Available,
            health: CameraHealth::NotSimulated,
        })
    });
    let binding = Installed {
        vessel_id: &public.binding().vessel_id,
        assembly_revision: public.binding().registration.assembly_revision,
        source_sha256: &public.binding().registration.assembly_source_sha256,
        definition_sha256: cameras.map_or(DEFINITION_SHA, |c| c.definition_sha256.as_str()),
    };
    let resolution = monitor.resolver.resolve(
        &binding,
        selected_device,
        state.as_ref(),
        snapshot.server_time_ms,
        now,
        connected,
        visible,
        dimensions,
    );
    if let Some(key) = resolution.retire_key {
        monitor.retire(key);
    }
    let Some(resolved) = resolution.feed else {
        monitor.suppress();
        if !visible {
            return Ok(None);
        }
        return Err(format!(
            "Mounted camera unavailable: {:?}",
            resolution.unavailable
        ));
    };
    let changed_scene = monitor.changed_equal_time(snapshot)?;
    if resolution.invalidate_pixels || changed_scene {
        monitor.invalidate(resolved.capture_key);
    }
    monitor.active_key = Some(resolved.capture_key);
    Ok(Some((
        build_frame(
            snapshot,
            pilot,
            public.parent,
            resolved,
            paused,
            lighting_quality,
        )?,
        resolved.capture_key,
    )))
}
impl FlightView {
    /// Existing one-workspace-entry budget. No order, audio listener or main
    /// camera selection is changed; only the selected real device is rendered.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn carrier_operations_camera_ui(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        viewed_name: &str,
        now: u64,
        connected: bool,
        paused: bool,
        selected_device: &str,
        hidden: bool,
    ) {
        let width = ui.available_width().max(1.);
        let ui_ms = ui.input(|i| (i.time.max(0.) * 1000.) as u64);
        let prior_age = self
            .carrier_monitor
            .active_key
            .and_then(|key| {
                self.renderer
                    .as_ref()
                    .ok()
                    .and_then(|r| r.lock().ok())
                    .and_then(|r| {
                        r.feed_cache
                            .iter()
                            .find(|c| c.key == key)
                            .and_then(|c| c.captured_ui_ms)
                    })
            })
            .filter(|t| *t <= ui_ms)
            .map(|t| ui_ms - t);
        let available_height = (ui
            .clip_rect()
            .bottom()
            .min(ui.available_rect_before_wrap().bottom())
            - ui.cursor().top())
        .max(0.);
        let height = image_height(
            width,
            available_height,
            caption_height(ui, width, paused, prior_age),
        );
        #[cfg(test)]
        if std::env::var_os("ARKLINE_CARRIER_LAYOUT_TRACE").is_some() {
            let font = egui::TextStyle::Small.resolve(ui.style());
            let measure = |text: String| {
                ui.painter()
                    .layout(text, font.clone(), egui::Color32::WHITE, width)
                    .size()
                    .y
            };
            let old_cold = measure(format!(
                "{} · {DEVICE_CAPTION}",
                bay_monitor_image_status(true, paused, true, None)
            )) + measure(SUPPLY_CAPTION.into())
                + 2. * ui.spacing().item_spacing.y;
            eprintln!(
                "CARRIER_LAYOUT {}",
                serde_json::json!({"width":width,"clip_bottom":ui.clip_rect().bottom(),"available_bottom":ui.available_rect_before_wrap().bottom(),"cursor_top":ui.cursor().top(),"visible_height":available_height,"font_size":font.size,"prior_age":prior_age,"cold_status":mounted_status(paused,None),"current_status":mounted_status(paused,prior_age),"old_verbose_cold_caption_height":old_cold,"complete_caption_height":caption_height(ui,width,paused,prior_age),"image_height":height,"minimum_image_height":96.})
            );
        }
        let insufficient_room = !hidden && height.is_none();
        let hidden = hidden || insufficient_room;
        if insufficient_room {
            ui.small("More visible space is needed for the crane camera and status");
        }
        let rect = if hidden {
            ui.clip_rect()
        } else {
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(width, height.unwrap_or(0.)),
                egui::Sense::hover(),
            );
            ui.painter().rect_filled(rect, 3., egui::Color32::BLACK);
            rect
        };
        let image_rect = letterbox(rect);
        let visible = !hidden && ui.is_rect_visible(image_rect);
        let physical = ui.ctx().pixels_per_point();
        let (w, h) = feed_resolution(
            (image_rect.width() * physical).round() as i32,
            (image_rect.height() * physical).round() as i32,
        );
        let result = resolve_frame(
            &mut self.carrier_monitor,
            snapshot,
            viewed_name,
            now,
            connected,
            paused,
            selected_device,
            visible,
            [w as u32, h as u32],
            self.lighting_quality,
        );
        let feed = match result {
            Ok(feed) => feed,
            Err(reason) => {
                self.carrier_monitor.suppress();
                if !hidden {
                    ui.colored_label(crate::AMBER, reason);
                }
                None
            }
        };
        let Ok(renderer) = &self.renderer else {
            if !hidden {
                ui.colored_label(
                    crate::AMBER,
                    "Graphics context unavailable · no camera image",
                );
            }
            return;
        };
        let pass = ui.ctx().cumulative_pass_nr();
        if let Some((frame, key)) = &feed {
            if let Ok(mut visible) = self.visible_feed_sources.lock() {
                visible.register_clocks(pass, [(*key, frame.now)]);
            }
            let pending_refresh = self
                .carrier_monitor
                .pending
                .lock()
                .map(|pending| pending.invalidate.contains(key) || pending.retire.contains(key))
                .unwrap_or(true);
            let age = renderer
                .lock()
                .ok()
                .and_then(|r| {
                    r.feed_cache
                        .iter()
                        .find(|c| c.key == *key)
                        .and_then(|c| c.captured_ui_ms)
                })
                .filter(|t| !pending_refresh && *t <= ui_ms)
                .map(|t| ui_ms - t);
            ui.small(format!(
                "{DEVICE_CAPTION} · {}",
                mounted_status(paused, age)
            ))
            .on_hover_text(SUPPLY_CAPTION);
        }
        // Cleanup is a GL callback even when unavailable/hidden. If clipping
        // prevents invocation, the shared queue survives until a later callback.
        let pending = self.carrier_monitor.pending.clone();
        let visible_sources = self.visible_feed_sources.clone();
        let renderer = renderer.clone();
        #[cfg(target_arch = "wasm32")]
        let renderer = send_wrapper::SendWrapper::new(renderer);
        ui.painter().add(egui::PaintCallback {
            rect: image_rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                if let Ok(mut renderer) = renderer.lock() {
                    unsafe {
                        flush_pending(&mut renderer, &pending);
                    }
                    if let Some((frame, key)) = &feed {
                        let keys = visible_sources
                            .lock()
                            .map(|v| v.for_pass(pass))
                            .unwrap_or_default();
                        renderer.feed_source_clocks = visible_sources
                            .lock()
                            .map(|v| v.clocks_for_pass(pass))
                            .unwrap_or_default();
                        unsafe {
                            renderer.prepare_workspace_feed(*key);
                            renderer.paint_cached_feed_on_surface(
                                frame,
                                *key,
                                info,
                                keys.len(),
                                painter.intermediate_fbo(),
                                None,
                                Some((pass, ui_ms, &keys)),
                            );
                        }
                    }
                }
            })),
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_status_keeps_image_age_and_truthful_cache_pause_states() {
        assert_eq!(mounted_status(false, None), "NO IMAGE");
        assert_eq!(mounted_status(false, Some(0)), "LIVE (0 ms)");
        assert_eq!(mounted_status(false, Some(251)), "CACHED (251 ms)");
        assert_eq!(mounted_status(true, Some(25)), "PAUSED (25 ms)");
        assert_eq!(mounted_status(true, None), "PAUSED · no image");
    }
    #[test]
    fn image_and_wrapped_captions_fit_the_actual_visible_remainder() {
        assert_eq!(image_height(1000., 200., 48.), Some(152.));
        assert_eq!(image_height(640., 600., 48.), Some(360.));
        assert_eq!(image_height(640., 100., 48.), None);
        assert_eq!(image_height(100., 600., 48.), None);
        assert_eq!(image_height(f32::NAN, 600., 48.), None);
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for width in [240., 400., 640.] {
                    let captions = caption_height(ui, width, false, Some(0));
                    assert!(captions > 0.);
                    let height = image_height(width, 220., captions).unwrap();
                    assert!(height + captions <= 220. + 1e-5);
                }
            });
        });
    }
    #[test]
    fn viewport_is_letterboxed_not_fov_stretched() {
        for size in [
            egui::vec2(1000., 200.),
            egui::vec2(200., 1000.),
            egui::vec2(640., 360.),
        ] {
            let outer = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            let inner = letterbox(outer);
            assert!((inner.aspect_ratio() - 16. / 9.).abs() < 1e-5);
            assert!(outer.contains_rect(inner));
        }
    }
    #[test]
    fn full_scene_same_time_fingerprint_detects_non_camera_change() {
        let ctx = egui::Context::default();
        let cockpit = crate::tests::view_test_cockpit(&ctx);
        let mut snapshot = cockpit.snapshot.unwrap();
        let mut state = State::default();
        assert!(!state.changed_equal_time(&snapshot).unwrap());
        assert!(!state.changed_equal_time(&snapshot).unwrap());
        snapshot.pilots[0].name.push_str("-changed");
        assert!(state.changed_equal_time(&snapshot).unwrap());
        snapshot.server_time_ms += 1;
        assert!(!state.changed_equal_time(&snapshot).unwrap());
    }
    #[test]
    fn frame_preserves_au_origin_and_confirmed_clock_without_hiding_ship_or_payload() {
        let ctx = egui::Context::default();
        let cockpit = crate::tests::view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.unwrap();
        let pilot = &snapshot.pilots[0];
        let origin = [149_597_870_700., 0., 0.];
        let parent = arkline_protocol::drone_bay_geometry::BayPose {
            position_m: origin,
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let resolved = ResolvedFeed {
            camera: crate::carrier_camera_geometry::CameraFrame {
                eye_world_m: [origin[0] + 1., 7.7, 0.],
                target_world_m: [origin[0], 3.5, 0.],
                up_world: [-0.972806, 0.231621, 0.],
                vertical_fov_radians: 110_f64.to_radians(),
                near_m: 0.03,
                far_m: 100.,
            },
            capture_key: 1,
            scene_time_ms: snapshot.server_time_ms,
            width: 640,
            height: 360,
        };
        let frame = build_frame(
            &snapshot,
            pilot,
            parent,
            resolved,
            false,
            Default::default(),
        )
        .unwrap();
        assert!((frame.eye.x - 0.01).abs() < 1e-7);
        assert_eq!(frame.space.origin, origin);
        assert_eq!(frame.now, snapshot.server_time_ms);
        assert!(!frame.cockpit && frame.drone_camera && frame.camera_drone.is_none());
        assert_eq!(
            frame.scene.pilots.len(),
            snapshot
                .pilots
                .iter()
                .filter(|p| p.system_id == pilot.system_id)
                .count()
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "carrier_monitor_native_tests.rs"]
mod native_tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "carrier_warning_native_tests.rs"]
mod warning_native_tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "parallel_turret_native_tests.rs"]
mod turret_native_tests;
