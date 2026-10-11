//! Private station bay in actual meters, using the same hull meshes as flight.
use super::*;
use arkline_protocol::{hangar_layout, StationAvatar};
#[path = "quarters.rs"]
mod quarters;
pub use quarters::HangarAction;

pub struct HangarView {
    renderer: Option<Arc<Mutex<Renderer>>>,
    error: Option<String>,
    surface: Option<(egui::Rect, egui::LayerId)>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
    walking: bool,
    class: Option<ShipClass>,
    follow: Option<Vec3>,
}
impl HangarView {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let result = gl.map(|gl| unsafe { Renderer::new(gl.clone()) });
        let (renderer, error) = match result {
            Some(Ok(renderer)) => (Some(Arc::new(Mutex::new(renderer))), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, Some("Hangar needs a GPU rendering context".into())),
        };
        Self {
            renderer,
            error,
            surface: None,
            yaw: -0.65,
            pitch: 0.32,
            zoom: 1.0,
            walking: false,
            class: None,
            follow: None,
        }
    }
    pub fn input_surface(&self) -> Option<(egui::Rect, egui::LayerId)> {
        self.surface
    }
    pub fn camera_yaw(&self) -> f64 {
        self.yaw as f64
    }
    pub fn destroy(&mut self) {
        if let Some(renderer) = self.renderer.take() {
            if let Ok(mut renderer) = renderer.lock() {
                unsafe {
                    renderer.destroy();
                }
            }
        }
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        pilot: &PilotView,
        now: u64,
        live: bool,
    ) -> Option<HangarAction> {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        self.surface = Some((rect, ui.layer_id()));
        let walking = pilot.station_avatar.is_some();
        if self.walking != walking || self.class != Some(pilot.ship_class) {
            self.zoom = 1.0;
            self.follow = None;
            self.pitch = if walking { 0.23 } else { 0.32 };
            self.walking = walking;
            self.class = Some(pilot.ship_class);
        }
        let unobstructed = ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|p| rect.contains(p) && ui.ctx().layer_id_at(p) == Some(ui.layer_id()));
        // Response helpers also access Context: never invoke them inside an input lock.
        let dragging = response.dragged_by(egui::PointerButton::Secondary)
            || (!walking && response.dragged_by(egui::PointerButton::Primary));
        if unobstructed {
            ui.input(|i| {
                if dragging {
                    self.yaw -= i.pointer.delta().x * 0.007;
                    self.pitch = (self.pitch + i.pointer.delta().y * 0.005).clamp(0.06, 1.25);
                }
                self.zoom = (self.zoom * (-i.smooth_scroll_delta.y * 0.002).exp()).clamp(0.45, 2.5);
            });
        }
        let layout = hangar_layout(pilot.ship_class);
        let target = if let Some(avatar) = &pilot.station_avatar {
            let next = Vec3::from_array(avatar.position.map(|v| v as f32));
            let dt = ui.input(|i| i.stable_dt).min(0.1);
            let smooth = self
                .follow
                .unwrap_or(next)
                .lerp(next, 1.0 - (-dt * 18.0).exp());
            self.follow = Some(smooth);
            smooth + Vec3::Y * 1.25
        } else {
            Vec3::from_array(layout.ship_center.map(|v| v as f32))
        };
        let distance = if walking {
            5.5
        } else {
            pilot.ship_class.spec().length_m as f32 * 0.88
        } * self.zoom;
        let forward = Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let mut eye =
            target - forward * distance * self.pitch.cos() + Vec3::Y * distance * self.pitch.sin();
        eye.y = eye.y.max(0.6);
        if walking {
            eye = quarters::camera_eye(pilot.ship_class, target, eye);
        }
        let vp = Mat4::perspective_rh_gl(
            55f32.to_radians(),
            rect.aspect_ratio().max(0.1),
            0.08,
            30000.0,
        ) * Mat4::look_at_rh(eye, target, Vec3::Y);
        let mut parts = bay_parts(pilot.ship_class);
        parts.extend(quarters::room_parts(pilot.ship_class, now));
        if let Some(avatar) = &pilot.station_avatar {
            parts.extend(avatar_parts(avatar, now, self.follow));
        }
        if let Some(renderer) = &self.renderer {
            let renderer = renderer.clone();
            ui.painter().add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                    if let Ok(renderer) = renderer.lock() {
                        unsafe {
                            paint(&renderer, vp, eye, &parts);
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
                "PRIVATE BAY  /  {}\n{}",
                pilot.ship_class.spec().name,
                if walking {
                    "ON FOOT · WASD walk · Arrows turn · Right-drag look · Wheel zoom"
                } else {
                    "SHIP INSPECTION · Drag orbit · Wheel zoom · Disembark to explore"
                }
            ),
            egui::FontId::proportional(14.0),
            egui::Color32::from_rgb(150, 220, 185),
        );
        quarters::terminals(ui, rect, vp, eye, pilot, unobstructed, live)
    }
}

fn bay_parts(class: ShipClass) -> Vec<ModelPart> {
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
    let parent = Mat4::from_scale_rotation_translation(
        Vec3::splat(100.0),
        Quat::IDENTITY,
        Vec3::from_array(bay.ship_center.map(|v| v as f32)),
    );
    p.0.extend(
        ship_parts(class)
            .into_iter()
            .map(|(mesh, m, c, e)| (mesh, parent * m, c, e * 0.25)),
    );
    p.0
}
fn avatar_parts(avatar: &StationAvatar, now: u64, smooth: Option<Vec3>) -> Vec<ModelPart> {
    let root = Mat4::from_rotation_translation(
        Quat::from_rotation_y(-avatar.heading as f32),
        smooth.unwrap_or_else(|| Vec3::from_array(avatar.position.map(|v| v as f32))),
    );
    let swing = if avatar.moving {
        (now as f32 * 0.009).sin() * 0.28
    } else {
        0.0
    };
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
        let step = swing * side;
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
                Vec3::new(step + 0.035, 0.075, side * 0.13),
                Vec3::new(0.15, 0.075, 0.105),
                armor,
                0.0,
            ),
            (
                Vec3::new(step * 0.5 + 0.075, 0.52, side * 0.13),
                Vec3::new(0.045, 0.08, 0.09),
                armor,
                0.0,
            ),
            (
                Vec3::new(-step, 0.87, side * 0.31),
                Vec3::new(0.07, 0.075, 0.07),
                armor,
                0.0,
            ),
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
            (
                Vec3::new(0.0, 0.94, side * 0.13),
                Vec3::new(step * 0.5, 0.52, side * 0.13),
                0.09,
            ),
            (
                Vec3::new(step * 0.5, 0.52, side * 0.13),
                Vec3::new(step, 0.09, side * 0.13),
                0.075,
            ),
            (
                Vec3::new(0.0, 1.40, side * 0.28),
                Vec3::new(-step * 0.5, 1.08, side * 0.31),
                0.07,
            ),
            (
                Vec3::new(-step * 0.5, 1.08, side * 0.31),
                Vec3::new(-step, 0.87, side * 0.31),
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
unsafe fn paint(renderer: &Renderer, vp: Mat4, eye: Vec3, parts: &[ModelPart]) {
    if renderer.destroyed {
        return;
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
    gl.uniform_matrix_4_f32_slice(renderer.uniforms.vp.as_ref(), false, &vp.to_cols_array());
    gl.uniform_3_f32(renderer.uniforms.eye.as_ref(), eye.x, eye.y, eye.z);
    gl.uniform_1_i32(renderer.uniforms.material.as_ref(), 0);
    for &(mesh, model, color, emission) in parts {
        renderer.draw(mesh, model, color, emission);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                        egui::CentralPanel::default()
                            .show(ctx, |ui| hangar.ui(ui, &pilot, 1000, true));
                    },
                );
            }
            tx.send(hangar.camera_yaw() != initial).unwrap();
        });
        assert!(rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .expect("Hangar pointer frame deadlocked"));
    }
    #[test]
    fn avatar_has_human_scale_and_walk_articulation() {
        let mut avatar = StationAvatar {
            seated: false,
            position: [0.0; 3],
            heading: 0.0,
            moving: false,
            updated_at_ms: 0,
        };
        let still = avatar_parts(&avatar, 0, None);
        let (_, max) = parts_bounds(&still);
        assert!((max.y - 1.8).abs() < 0.01);
        avatar.moving = true;
        let walking = avatar_parts(&avatar, 175, None);
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
            paint(&renderer, vp, eye, &bay_parts(ShipClass::Pioneer));
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
                paint(&renderer, vp, eye, &parts);
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
                paint(&renderer, vp, eye, &parts);
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
            renderer.destroy();
            target.destroy(&gl);
            CGLSetCurrentContext(std::ptr::null_mut());
            CGLDestroyContext(context);
        }
    }
}
