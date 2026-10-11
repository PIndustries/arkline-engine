//! Private station bay in actual meters, using the same hull meshes as flight.
use super::*;
use arkline_protocol::{hangar_layout, StationAvatar};
#[path = "avatar_animation.rs"]
mod avatar_animation;
#[path = "pioneer_interior.rs"]
mod pioneer_interior;
#[path = "quarters.rs"]
mod quarters;
pub use quarters::HangarAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFootView {
    FirstPerson,
    ThirdPerson,
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
    interior_deck: u8,
    class: Option<ShipClass>,
    follow: Option<Vec3>,
    gait: avatar_animation::Gait,
}
impl HangarView {
    // egui callbacks share ownership on one browser thread; SendWrapper guards access.
    #[cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let result = gl.map(|gl| unsafe { Renderer::new(gl.clone()) });
        let (renderer, error) = match result {
            Some(Ok(renderer)) => (Some(Arc::new(Mutex::new(renderer))), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, Some("Hangar needs a GPU rendering context".into())),
        };
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
            interior_deck: 1,
            class: None,
            follow: None,
            gait: Default::default(),
        }
    }
    pub fn input_surface(&self) -> Option<(egui::Rect, egui::LayerId)> {
        self.surface
    }
    pub fn camera_yaw(&self) -> f64 {
        self.yaw as f64
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
        let interior = pilot
            .station_avatar
            .as_ref()
            .is_some_and(|a| a.onboard_interior);
        let interior_deck = pilot.station_avatar.as_ref().map_or(1, |a| a.interior_deck);
        let reset_heading = self.walking != walking
            || self.interior != interior
            || self.class != Some(pilot.ship_class);
        if self.interior_deck != interior_deck || reset_heading {
            self.interior = interior;
            self.interior_deck = interior_deck;
            self.zoom = 1.0;
            self.follow = None;
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
                    self.pitch = (self.pitch + i.pointer.delta().y * 0.005)
                        .clamp(if walking { -0.8 } else { 0.06 }, 1.25);
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
        // Follow the authoritative body heading, including stationary turns.
        let view_yaw = pilot
            .station_avatar
            .as_ref()
            .map_or(self.yaw, |a| a.heading as f32);
        let forward = Vec3::new(view_yaw.cos(), 0.0, view_yaw.sin());
        let mut eye = third_person_eye(target, view_yaw, self.pitch, distance);
        eye.y = eye.y.max(0.6);
        if walking {
            eye = if interior {
                pioneer_interior::camera(target, eye)
            } else {
                quarters::camera_eye(pilot.ship_class, target, eye)
            };
        }
        let first_person = walking && self.on_foot_view == OnFootView::FirstPerson;
        let look_target = if first_person {
            eye = self.follow.unwrap() + Vec3::Y * 1.65;
            eye + forward * self.pitch.cos() - Vec3::Y * self.pitch.sin()
        } else {
            target
        };
        let vp = Mat4::perspective_rh_gl(
            55f32.to_radians(),
            rect.aspect_ratio().max(0.1),
            0.08,
            30000.0,
        ) * Mat4::look_at_rh(eye, look_target, Vec3::Y);
        let mut parts = if interior {
            pioneer_interior::parts(interior_deck)
        } else {
            let mut p = bay_parts(pilot.ship_class);
            p.extend(quarters::room_parts(pilot.ship_class, now));
            p
        };
        if let Some(avatar) = &pilot.station_avatar {
            let position = self
                .follow
                .unwrap_or_else(|| Vec3::from_array(avatar.position.map(|v| v as f32)));
            let pose = self.gait.update(
                now,
                position,
                avatar.heading,
                avatar.moving && live,
                avatar.seated,
            );
            if !first_person {
                parts.extend(avatar_parts(avatar, pose, self.follow));
            }
        }
        if let Some(renderer) = &self.renderer {
            let renderer = renderer.clone();
            #[cfg(target_arch = "wasm32")]
            let renderer = send_wrapper::SendWrapper::new(renderer);
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
                "{}  /  {}\n{}",
                if interior {
                    if interior_deck == 2 {
                        "ONBOARD · DECK TWO"
                    } else {
                        "ONBOARD · DECK ONE"
                    }
                } else {
                    "PRIVATE BAY"
                },
                pilot.ship_class.spec().name,
                if walking {
                    "ON FOOT · WASD / arrows walk · Right-drag turn / look · Space jump"
                } else {
                    "SHIP INSPECTION · Drag orbit · Wheel zoom · Disembark to explore"
                }
            ),
            egui::FontId::proportional(14.0),
            egui::Color32::from_rgb(150, 220, 185),
        );
        if interior {
            pioneer_interior::labels(ui, rect, vp, pilot, live);
            None
        } else {
            quarters::terminals(ui, rect, vp, eye, pilot, unobstructed, live)
        }
    }
}

fn third_person_eye(target: Vec3, heading: f32, pitch: f32, distance: f32) -> Vec3 {
    let forward = Vec3::new(heading.cos(), 0.0, heading.sin());
    target - forward * distance * pitch.cos() + Vec3::Y * distance * pitch.sin()
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
    renderer.local_lights.clear(gl);
    let shadow_texture = renderer.disable_sun_shadow();
    gl.uniform_1_f32(renderer.uniforms.environment_atmosphere.as_ref(), 0.0);
    gl.uniform_3_f32(renderer.uniforms.environment_up.as_ref(), 0.0, 1.0, 0.0);
    gl.uniform_matrix_4_f32_slice(renderer.uniforms.vp.as_ref(), false, &vp.to_cols_array());
    gl.uniform_3_f32(renderer.uniforms.eye.as_ref(), eye.x, eye.y, eye.z);
    gl.uniform_1_i32(renderer.uniforms.material.as_ref(), 0);
    for &(mesh, model, color, emission) in parts {
        renderer.draw(mesh, model, color, emission);
    }
    renderer.restore_shadow_texture(shadow_texture);
}

#[cfg(test)]
mod tests {
    use super::*;
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
            if let Ok(directory) = std::env::var("ARKLINE_GAIT_CAPTURE_DIR") {
                let directory = std::path::PathBuf::from(directory);
                std::fs::create_dir_all(&directory).unwrap();
                let tile = 384usize;
                let capture = CachedFeed::create(&gl, 1001, tile as i32, tile as i32).unwrap();
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(capture.framebuffer));
                gl.viewport(0, 0, tile as i32, tile as i32);
                let avatar = StationAvatar {
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
                    paint(&renderer, vp, eye, &parts);
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
