//! Native GPU flight view. Presentation only: never issues simulation commands.
use arkline_protocol::{
    gates, systems, Asteroid, Destination, Location, NavigationMode, NavigationTarget, PilotView,
    Snapshot, Stargate, BELT_POSITION, HOME_SYSTEM_ID, JUMP_DURATION_MS, MAX_SHIP_SPEED,
    SPACE_BOUND, STATION_POSITION, WARP_DURATION_MS, WARP_WARMUP_MS,
};
use eframe::{egui, egui_glow, glow};
use glam::{Mat4, Quat, Vec3};
use glow::HasContext;
use std::sync::{Arc, Mutex};

pub enum ViewAction {
    Select(super::SelectedObject),
    FlyTo([f64; 3]),
    Command(arkline_protocol::Request),
}

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
    if let Some(flight) = &pilot.flight {
        let seconds = if matches!(
            pilot.location,
            Location::Docked | Location::Jumping { .. } | Location::InTransit { .. }
        ) {
            0.0
        } else {
            now.saturating_sub(flight.updated_at_ms).min(350) as f64 / 1000.0
        };
        return std::array::from_fn(|axis| flight.position[axis] + flight.velocity[axis] * seconds);
    }
    // Older snapshots lack a flight record. Their coarse locations retain the
    // legacy display route; current servers always supply authoritative flight.
    Vec3::new(
        -45.0 + route_progress(&pilot.location, now) * 105.0,
        lane(&pilot.name) * 1.8,
        20.0 + lane(&pilot.name) * 2.0,
    )
    .to_array()
    .map(|value| value as f64 * 100.0)
}

fn scene_position(meters: [f64; 3]) -> Vec3 {
    Vec3::from_array(meters.map(|value| (value / 100.0) as f32))
}
fn position(pilot: &PilotView, now: u64) -> Vec3 {
    scene_position(display_position(pilot, now))
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
        .and_then(|flight| orientation(flight.heading).or_else(|| orientation(flight.velocity)))
        .unwrap_or_else(|| {
            Quat::from_rotation_y(if heading(&pilot.location) < 0.0 {
                std::f32::consts::PI
            } else {
                0.0
            })
        })
}

fn navigation_destination(
    pilot: &PilotView,
    snapshot: &Snapshot,
    now: u64,
) -> Option<(&'static str, Vec3)> {
    if matches!(
        pilot.location,
        Location::Docked | Location::Jumping { .. } | Location::InTransit { .. }
    ) {
        return None;
    }
    let (label, target) = match &pilot.flight.as_ref()?.mode {
        NavigationMode::Stop => return None,
        NavigationMode::FlyTo { position } => return Some(("FLY TO", scene_position(*position))),
        NavigationMode::Approach { target } => ("APPROACH", target),
        NavigationMode::Orbit { target, .. } => ("ORBIT", target),
        NavigationMode::KeepRange { target, .. } => ("KEEP RANGE", target),
    };
    let target = match target {
        NavigationTarget::Station => scene_position(STATION_POSITION),
        NavigationTarget::Belt => scene_position(BELT_POSITION),
        NavigationTarget::Asteroid { id } => scene_position(
            snapshot
                .asteroids
                .iter()
                .find(|a| a.id == *id && a.system_id == pilot.system_id)?
                .position,
        ),
        NavigationTarget::Gate { id } => scene_position(
            gates()
                .iter()
                .find(|g| g.id == *id && g.system_id == pilot.system_id)?
                .position,
        ),
        NavigationTarget::Pilot { pilot: name } => position(
            snapshot
                .pilots
                .iter()
                .find(|p| &p.name == name && p.system_id == pilot.system_id)?,
            now,
        ),
    };
    Some((label, target))
}

pub struct FlightView {
    renderer: Result<Arc<Mutex<Renderer>>, String>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    target: Vec3,
    follow: bool,
    focus_selection: bool,
    context_target: Option<[f64; 3]>,
    context_object: Option<super::SelectedObject>,
}
impl FlightView {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let renderer = gl
            .ok_or_else(|| "OpenGL is unavailable. Switch to 2D Tactical.".into())
            .and_then(|gl| unsafe { Renderer::new(gl.clone()) })
            .map(|r| Arc::new(Mutex::new(r)));
        Self {
            renderer,
            yaw: 0.72,
            pitch: 0.35,
            distance: 27.0,
            target: Vec3::new(-45.0, 0.0, 20.0),
            follow: true,
            focus_selection: false,
            context_target: None,
            context_object: None,
        }
    }
    pub fn destroy(&mut self) {
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
    ) -> Option<ViewAction> {
        let mut picked = None;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("FLIGHT CAMERA")
                    .color(super::TEAL)
                    .strong(),
            );
            if ui.button("Focus my ship").clicked() {
                self.follow = true;
                self.focus_selection = false;
                self.distance = 27.0;
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
                self.distance = 27.0;
                self.yaw = 0.72;
                self.pitch = 0.35;
            }
        });
        ui.small("Drag: orbit camera · Scroll: zoom · Double-click empty space: fly there · Right-click: navigation");
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(100.0, 150.0)),
            egui::Sense::click_and_drag(),
        );
        if response.dragged_by(egui::PointerButton::Primary) {
            let delta = ui.input(|i| i.pointer.delta());
            self.yaw -= delta.x * 0.007;
            self.pitch = (self.pitch + delta.y * 0.006).clamp(-1.25, 1.25);
        }
        if response.hovered() {
            self.distance = (self.distance
                * (-ui.input(|i| i.smooth_scroll_delta.y) * 0.002).exp())
            .clamp(9.0, 260.0);
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
        let selected_position = match selected {
            super::SelectedObject::Asteroid(id) => asteroids
                .iter()
                .find(|a| a.id == *id)
                .map(|a| scene_position(a.position))
                .unwrap_or(self.target),
            super::SelectedObject::Gate(id) => local_gates
                .iter()
                .find(|g| g.id == *id)
                .map(|g| scene_position(g.position))
                .unwrap_or(self.target),
            super::SelectedObject::Station => scene_position(STATION_POSITION),
            super::SelectedObject::Belt => scene_position(BELT_POSITION),
            super::SelectedObject::Pilot(name) => snapshot
                .pilots
                .iter()
                .find(|p| &p.name == name && p.system_id == system_id)
                .map(|p| position(p, now))
                .unwrap_or(self.target),
        };
        let desired = if self.focus_selection {
            selected_position
        } else if self.follow {
            snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot_name)
                .map(|p| position(p, now))
                .unwrap_or(self.target)
        } else {
            Vec3::new(7.5, 0.0, 5.0)
        };
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        self.target = self.target.lerp(desired, 1.0 - (-dt * 8.0).exp());
        let eye = self.target
            + Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            ) * self.distance;
        let view = Mat4::look_at_rh(eye, self.target, Vec3::Y);
        let projection =
            Mat4::perspective_rh_gl(0.85, rect.width() / rect.height().max(1.0), 0.3, 1200.0);
        let vp = projection * view;
        match &self.renderer {
            Ok(renderer) => {
                let renderer = renderer.clone();
                let frame = Frame {
                    asteroids: asteroids.clone(),
                    gates: local_gates.clone(),
                    mining_target: effects.mining_target(now),
                    vp,
                    eye,
                    now,
                    pilots: snapshot
                        .pilots
                        .iter()
                        .filter(|p| p.system_id == system_id && p.location != Location::Docked)
                        .cloned()
                        .collect(),
                    own: pilot_name.to_string(),
                    yaw: self.yaw,
                    pitch: self.pitch,
                    mining: connected.then(|| effects.mining(now)).flatten(),
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
                ui.painter().add(egui::PaintCallback {
                    rect,
                    callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                        if let Ok(mut renderer) = renderer.lock() {
                            unsafe {
                                renderer.paint(&frame);
                            }
                        }
                    })),
                });
                for pilot in snapshot
                    .pilots
                    .iter()
                    .filter(|p| p.system_id == system_id && p.location != Location::Docked)
                {
                    let clip = vp * (position(pilot, now) + Vec3::Y * 4.2).extend(1.0);
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
                let project = |point: Vec3| project_point(vp, rect, point);
                let own_pilot = snapshot
                    .pilots
                    .iter()
                    .find(|pilot| pilot.name == pilot_name);
                let can_navigate = connected
                    && own_pilot.is_some_and(|pilot| {
                        pilot.flight.is_some()
                            && !matches!(
                                pilot.location,
                                Location::Docked
                                    | Location::Jumping { .. }
                                    | Location::InTransit { .. }
                            )
                    });
                if response.clicked() || response.secondary_clicked() {
                    if let Some(pointer) = response.interact_pointer_pos() {
                        let mut targets = Vec::new();
                        for asteroid in &asteroids {
                            targets.push((
                                super::SelectedObject::Asteroid(asteroid.id),
                                rock_mesh(),
                                asteroid_model(asteroid),
                            ));
                        }
                        for gate in &local_gates {
                            for (mesh, model) in gate_parts(gate) {
                                targets.push((super::SelectedObject::Gate(gate.id), mesh, model));
                            }
                        }
                        for (mesh, model, _, _) in station_parts() {
                            targets.push((
                                super::SelectedObject::Station,
                                mesh_vertices(mesh),
                                model,
                            ));
                        }
                        for pilot in snapshot
                            .pilots
                            .iter()
                            .filter(|p| p.system_id == system_id && p.location != Location::Docked)
                        {
                            let model =
                                transform(position(pilot, now), Vec3::ONE, ship_orientation(pilot));
                            for (mesh, part) in ship_geometry(model) {
                                targets.push((
                                    super::SelectedObject::Pilot(pilot.name.clone()),
                                    mesh,
                                    part,
                                ));
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
                                    unproject_to_plane(vp, rect, pointer, position(pilot, now).y)
                                })
                                .map(|point| point.to_array().map(|value| value as f64 * 100.0))
                        } else {
                            None
                        };
                        if response.secondary_clicked() {
                            self.context_target = destination;
                            self.context_object = object.clone();
                        }
                        if let Some(object) = object {
                            picked = Some(ViewAction::Select(object));
                        } else if response.double_clicked() {
                            picked = destination.map(ViewAction::FlyTo);
                        }
                    }
                }
                response.context_menu(|ui| {
                    ui.label("Navigation");
                    match &self.context_object {
                        Some(super::SelectedObject::Asteroid(id)) => {
                            if let Some(asteroid) = asteroids.iter().find(|a| a.id == *id) {
                                ui.label(format!("{} · {} ore", asteroid.name, asteroid.ore));
                                let lockable = can_navigate
                                    && own_pilot.is_some_and(|p| {
                                        position(p, now).distance(scene_position(asteroid.position))
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
                                    .add_enabled(can_navigate, egui::Button::new("Warp to gate"))
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
                                        position(p, now).distance(scene_position(gate.position))
                                            * 100.0
                                            <= arkline_protocol::GATE_RANGE as f32
                                    });
                                if ui
                                    .add_enabled(in_range, egui::Button::new("Jump through gate"))
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
                let painter = ui.painter_at(rect);
                for gate in &local_gates {
                    if let Some(point) = project(
                        scene_position(gate.position)
                            + Vec3::Y * (gate.radius as f32 / 100.0 + 1.0),
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
                        if let Some(point) = project(scene_position(a.position)) {
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
                    if let Some((label, destination)) = navigation_destination(own, snapshot, now) {
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
                            if let Some(ship) = project(position(own, now)) {
                                painter.line_segment(
                                    [ship, point],
                                    egui::Stroke::new(1.0, super::AMBER.gamma_multiply(0.35)),
                                );
                            }
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
                            let meters = position(own, now).distance(selected_position) * 100.0;
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
                        if let Some((phase, progress)) = warp_phase(&own.location, now) {
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
struct Frame {
    asteroids: Vec<Asteroid>,
    gates: Vec<Stargate>,
    mining_target: Option<u64>,
    vp: Mat4,
    eye: Vec3,
    now: u64,
    pilots: Vec<PilotView>,
    own: String,
    yaw: f32,
    pitch: f32,
    mining: Option<(f32, u64)>,
    docking: Option<(f32, bool)>,
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
struct Mesh {
    vao: glow::VertexArray,
    buffer: glow::Buffer,
    count: i32,
}
struct Renderer {
    gl: Arc<glow::Context>,
    program: glow::Program,
    background: glow::Program,
    empty: glow::VertexArray,
    meshes: Vec<Mesh>,
    destroyed: bool,
}
impl Renderer {
    unsafe fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        let program = compile(&gl, VERTEX, FRAGMENT)?;
        let background = compile(&gl, BACK_VERTEX, BACK_FRAGMENT)?;
        let empty = gl.create_vertex_array()?;
        let mut meshes = vec![];
        for vertices in [
            cube(),
            torus(1.0, 0.12),
            sphere(false),
            rock_mesh(),
            hull(),
            sphere_with_resolution(false, 72, 48),
        ] {
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
        Ok(Self {
            gl,
            program,
            background,
            empty,
            meshes,
            destroyed: false,
        })
    }
    unsafe fn draw(&self, mesh: usize, model: Mat4, color: Vec3, emission: f32) {
        let gl = &self.gl;
        gl.uniform_matrix_4_f32_slice(
            gl.get_uniform_location(self.program, "model").as_ref(),
            false,
            &model.to_cols_array(),
        );
        gl.uniform_3_f32(
            gl.get_uniform_location(self.program, "base_color").as_ref(),
            color.x,
            color.y,
            color.z,
        );
        gl.uniform_1_f32(
            gl.get_uniform_location(self.program, "emission").as_ref(),
            emission,
        );
        gl.bind_vertex_array(Some(self.meshes[mesh].vao));
        gl.draw_arrays(glow::TRIANGLES, 0, self.meshes[mesh].count);
    }
    unsafe fn paint(&mut self, frame: &Frame) {
        if self.destroyed {
            return;
        }
        let gl = &self.gl;
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        gl.disable(glow::DEPTH_TEST);
        gl.use_program(Some(self.background));
        gl.bind_vertex_array(Some(self.empty));
        gl.uniform_2_f32(
            gl.get_uniform_location(self.background, "camera_angles")
                .as_ref(),
            frame.yaw,
            frame.pitch,
        );
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(true);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        gl.use_program(Some(self.program));
        gl.uniform_matrix_4_f32_slice(
            gl.get_uniform_location(self.program, "view_projection")
                .as_ref(),
            false,
            &frame.vp.to_cols_array(),
        );
        gl.uniform_3_f32(
            gl.get_uniform_location(self.program, "eye").as_ref(),
            frame.eye.x,
            frame.eye.y,
            frame.eye.z,
        );
        let steel = Vec3::new(0.20, 0.29, 0.37);
        let teal = Vec3::new(0.12, 0.86, 0.83);
        let amber = Vec3::new(1.0, 0.43, 0.12);
        // Procedural planet material stays isolated from ship/station surfaces.
        gl.uniform_1_i32(
            gl.get_uniform_location(self.program, "material_kind")
                .as_ref(),
            1,
        );
        self.draw(
            5,
            transform(
                Vec3::new(70.0, -95.0, -230.0),
                Vec3::splat(90.0),
                Quat::IDENTITY,
            ),
            Vec3::new(0.13, 0.22, 0.36),
            0.0,
        );
        gl.uniform_1_i32(
            gl.get_uniform_location(self.program, "material_kind")
                .as_ref(),
            0,
        );
        for (mesh, model, color, emission) in station_parts() {
            self.draw(mesh, model, color, emission);
        }
        for asteroid in &frame.asteroids {
            let color = if asteroid.ore == 0 {
                Vec3::splat(0.10)
            } else {
                Vec3::new(0.36, 0.29, 0.18)
            };
            self.draw(3, asteroid_model(asteroid), color, 0.0);
        }
        for gate in &frame.gates {
            for (_, model) in gate_parts(gate) {
                self.draw(1, model, Vec3::new(0.16, 0.43, 0.65), 0.7);
            }
        }
        for pilot in &frame.pilots {
            let own = pilot.name == frame.own;
            let accent = if own { teal } else { amber };
            let ship = transform(
                position(pilot, frame.now),
                Vec3::ONE,
                ship_orientation(pilot),
            );
            self.draw(
                4,
                ship,
                if own {
                    Vec3::new(0.37, 0.48, 0.57)
                } else {
                    Vec3::new(0.46, 0.39, 0.30)
                },
                0.0,
            );
            self.draw(
                0,
                ship * transform(
                    Vec3::new(-0.7, 0.72, 0.0),
                    Vec3::new(1.25, 0.12, 0.43),
                    Quat::from_rotation_z(-0.12),
                ),
                accent,
                0.8,
            );
            for side in [-1.0, 1.0] {
                self.draw(
                    0,
                    ship * transform(
                        Vec3::new(-1.6, -0.05, side * 1.7),
                        Vec3::new(1.8, 0.22, 0.55),
                        Quat::from_rotation_y(side * 0.18),
                    ),
                    steel,
                    0.0,
                );
                self.draw(
                    0,
                    ship * transform(
                        Vec3::new(-2.5, 0.0, side * 1.5),
                        Vec3::new(0.45, 0.48, 0.48),
                        Quat::IDENTITY,
                    ),
                    steel * 0.6,
                    0.0,
                );
                self.draw(
                    2,
                    ship * transform(
                        Vec3::new(-3.0, 0.0, side * 1.5),
                        Vec3::new(0.3, 0.31, 0.31),
                        Quat::IDENTITY,
                    ),
                    accent,
                    3.0,
                );
            }
        }
        self.paint_effects(frame);
        gl.disable(glow::DEPTH_TEST);
        gl.depth_mask(false);
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }
    /// Additive, depth-tested effects do not write depth or obscure UI meshes.
    unsafe fn paint_effects(&self, frame: &Frame) {
        let gl = &self.gl;
        gl.enable(glow::BLEND);
        gl.blend_func_separate(glow::ONE, glow::ONE, glow::ZERO, glow::ONE);
        gl.depth_mask(false);
        for pilot in &frame.pilots {
            let ship_position = position(pilot, frame.now);
            let ship = transform(ship_position, Vec3::ONE, ship_orientation(pilot));
            let warp = warp_phase(&pilot.location, frame.now);
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
                    Some(("WARP LAUNCH", _)) => (12.0, 3.2),
                    Some(("WARP CRUISE", _)) => (10.0, 2.7),
                    Some(("WARP ARRIVAL", progress)) => (10.0 - progress * 6.0, 2.7 - progress),
                    _ => {
                        let speed = pilot.flight.as_ref().map_or(0.0, |flight| {
                            flight
                                .velocity
                                .iter()
                                .map(|value| value * value)
                                .sum::<f64>()
                                .sqrt()
                        });
                        let thrust = (speed / MAX_SHIP_SPEED).clamp(0.0, 1.0) as f32;
                        (0.3 + thrust * 3.5, 0.25 + thrust * 1.35)
                    }
                };
                let pulse = 1.0
                    + 0.10
                        * ((frame.now % 260) as f32 / 260.0 * std::f32::consts::TAU
                            + lane(&pilot.name))
                        .sin();
                for side in [-1.0, 1.0] {
                    for (width, brightness) in [(0.58, 0.10), (0.31, 0.62), (0.16, 1.0)] {
                        self.draw(
                            2,
                            ship * transform(
                                Vec3::new(-3.0 - length * 0.5, 0.0, side * 1.5),
                                Vec3::new(length * 0.5 * pulse, width * pulse, width * pulse),
                                Quat::IDENTITY,
                            ),
                            Vec3::new(0.16, 0.48, 0.95) * brightness,
                            intensity,
                        );
                    }
                }
            }
            if let Some((stage, progress)) = warp {
                if stage == "WARP WARMUP" || stage == "WARP LAUNCH" || stage == "WARP ARRIVAL" {
                    let (radius, alpha) = match stage {
                        "WARP WARMUP" => (3.3 + progress * 2.2, 0.30 + progress * 0.55),
                        "WARP LAUNCH" => (5.5 + progress * 12.0, 1.0 - progress * 0.6),
                        _ => (
                            13.0 - progress * 9.0,
                            0.75 * (std::f32::consts::PI * progress).sin().max(0.15),
                        ),
                    };
                    self.draw(
                        1,
                        ship * transform(
                            Vec3::new(-1.0, 0.0, 0.0),
                            Vec3::new(radius, 0.50, radius),
                            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                        ),
                        Vec3::new(0.12, 0.55, 0.95) * alpha,
                        1.5,
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
            if pilot.name != frame.own {
                continue;
            }
            if let Some((progress, units)) = frame.mining.filter(|_| {
                frame.asteroids.iter().any(|a| {
                    Some(a.id) == frame.mining_target
                        && super::effects::can_extract_target(pilot, a)
                })
            }) {
                let source = ship.transform_point3(Vec3::new(0.4, 1.05, 0.0));
                if let Some(impact) = frame
                    .asteroids
                    .iter()
                    .find(|a| Some(a.id) == frame.mining_target)
                    .and_then(|a| mining_impact(source, a))
                {
                    let fade = (progress * 30.0).min(1.0) * ((1.0 - progress) * 5.0).min(1.0);
                    let pulse = 0.9
                        + 0.1 * ((frame.now % 134) as f32 / 134.0 * std::f32::consts::TAU).sin();
                    let color = Vec3::new(0.09, 0.74, 0.68) * fade;
                    if let Some(beam) = segment_model(source, impact, 0.18 * pulse) {
                        self.draw(0, beam, color, 1.8);
                    }
                    if let Some(halo) = segment_model(source, impact, 0.46 * pulse) {
                        self.draw(0, halo, color * 0.18, 0.8);
                    }
                    self.draw(
                        2,
                        transform(impact, Vec3::splat(0.70 * pulse), Quat::IDENTITY),
                        color,
                        1.8,
                    );
                    let toward_ship = (source - impact).normalize_or_zero();
                    for i in 0..16 {
                        let seed = i as f32;
                        let age = (progress * 3.0 + seed * 0.38197).fract();
                        let direction = (toward_ship
                            + Vec3::new(
                                (seed * 2.4).cos(),
                                (seed * 1.7).sin(),
                                (seed * 3.1).sin(),
                            ) * 0.8)
                            .normalize_or_zero();
                        let spark = impact + direction * (0.15 + age * 2.8);
                        self.draw(
                            2,
                            transform(
                                spark,
                                Vec3::splat(0.11 + (1.0 - age) * 0.09),
                                Quat::IDENTITY,
                            ),
                            Vec3::new(0.8, 0.45, 0.10) * fade * (1.0 - age),
                            1.0,
                        );
                    }
                    for i in 0..units.clamp(8, 24) {
                        let seed = i as f32;
                        let travel = (progress * 1.65 + seed * 0.61803).fract();
                        let arc = Vec3::Y
                            * (std::f32::consts::PI * travel).sin()
                            * (0.5 + (i % 4) as f32 * 0.25);
                        let ore = impact.lerp(source, travel) + arc;
                        let scale = 0.17 + (i % 3) as f32 * 0.045;
                        self.draw(
                            3,
                            transform(
                                ore,
                                Vec3::splat(scale),
                                Quat::from_rotation_y(progress * 8.0 + seed),
                            ),
                            Vec3::new(0.42, 0.66, 0.37) * fade,
                            0.7,
                        );
                    }
                }
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
        for mesh in &self.meshes {
            self.gl.delete_buffer(mesh.buffer);
            self.gl.delete_vertex_array(mesh.vao);
        }
        self.gl.delete_vertex_array(self.empty);
        self.gl.delete_program(self.program);
        self.gl.delete_program(self.background);
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
    if distance <= 0.0 || distance > 1200.0 {
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
    let inverse = vp.inverse();
    let near = inverse * glam::Vec4::new(x, y, -1.0, 1.0);
    let far = inverse * glam::Vec4::new(x, y, 1.0, 1.0);
    if !near.is_finite() || !far.is_finite() || near.w.abs() < 1e-7 || far.w.abs() < 1e-7 {
        return None;
    }
    let origin = near.truncate() / near.w;
    let direction = (far.truncate() / far.w - origin).normalize_or_zero();
    (direction.is_finite() && direction.length_squared() > 0.5).then_some((origin, direction))
}
fn transform(p: Vec3, scale: Vec3, rotation: Quat) -> Mat4 {
    Mat4::from_scale_rotation_translation(scale, rotation, p)
}
fn in_warp(location: &Location, now: u64) -> bool {
    matches!(location, Location::InTransit { arrives_at_ms, .. } if now < *arrives_at_ms)
}
fn ship_geometry(ship: Mat4) -> Vec<(Vec<Vertex>, Mat4)> {
    let mut parts = vec![
        (hull(), ship),
        (
            cube(),
            ship * transform(
                Vec3::new(-0.7, 0.72, 0.0),
                Vec3::new(1.25, 0.12, 0.43),
                Quat::from_rotation_z(-0.12),
            ),
        ),
    ];
    for side in [-1.0, 1.0] {
        parts.push((
            cube(),
            ship * transform(
                Vec3::new(-1.6, -0.05, side * 1.7),
                Vec3::new(1.8, 0.22, 0.55),
                Quat::from_rotation_y(side * 0.18),
            ),
        ));
        parts.push((
            cube(),
            ship * transform(
                Vec3::new(-2.5, 0.0, side * 1.5),
                Vec3::new(0.45, 0.48, 0.48),
                Quat::IDENTITY,
            ),
        ));
        parts.push((
            sphere(false),
            ship * transform(
                Vec3::new(-3.0, 0.0, side * 1.5),
                Vec3::new(0.3, 0.31, 0.31),
                Quat::IDENTITY,
            ),
        ));
    }
    parts
}
fn rock_mesh() -> Vec<Vertex> {
    let mut mesh = sphere(true);
    let radius = mesh.iter().map(|v| v.p.length()).fold(0.0_f32, f32::max);
    for vertex in &mut mesh {
        vertex.p /= radius;
    }
    mesh
}
fn asteroid_model(asteroid: &Asteroid) -> Mat4 {
    let f = (asteroid.id % 10000) as f32;
    transform(
        scene_position(asteroid.position),
        Vec3::splat(asteroid.radius as f32 / 100.0),
        Quat::from_euler(glam::EulerRot::XYZ, f * 0.2, f * 0.4, f),
    )
}
fn gate_parts(gate: &Stargate) -> Vec<(Vec<Vertex>, Mat4)> {
    [-0.45, 0.45]
        .into_iter()
        .map(|z| {
            (
                torus(1.0, 0.12),
                transform(
                    scene_position(gate.position) + Vec3::Z * z,
                    Vec3::splat(gate.radius as f32 / 112.0),
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
            )
        })
        .collect()
}
fn mining_impact(source: Vec3, asteroid: &Asteroid) -> Option<Vec3> {
    let direction = (scene_position(asteroid.position) - source).normalize_or_zero();
    ray_mesh_hit(source, direction, asteroid_model(asteroid), &rock_mesh())
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
    match mesh {
        0 => cube(),
        1 => torus(1.0, 0.12),
        2 => sphere(false),
        _ => hull(),
    }
}
fn station_parts() -> Vec<(usize, Mat4, Vec3, f32)> {
    let mut parts = Parts(Vec::new());
    let steel = Vec3::new(0.28, 0.34, 0.42);
    let teal = Vec3::new(0.08, 0.78, 0.71);
    let amber = Vec3::new(0.95, 0.49, 0.12);
    let station = Mat4::from_translation(scene_position(STATION_POSITION));
    parts.draw(1, station * Mat4::from_scale(Vec3::splat(13.0)), steel, 0.0);
    parts.draw(
        1,
        station
            * transform(
                Vec3::new(0.0, 0.9, 0.0),
                Vec3::new(13.0, 1.8, 13.0),
                Quat::IDENTITY,
            ),
        teal,
        1.6,
    );
    parts.draw(
        1,
        station * transform(Vec3::new(0.0, -2.0, 0.0), Vec3::splat(8.5), Quat::IDENTITY),
        steel * 0.7,
        0.0,
    );
    parts.draw(
        0,
        station * Mat4::from_scale(Vec3::new(2.8, 5.0, 2.8)),
        steel,
        0.0,
    );
    parts.draw(
        2,
        station
            * transform(
                Vec3::new(0.0, 6.0, 0.0),
                Vec3::new(1.7, 2.0, 1.7),
                Quat::IDENTITY,
            ),
        teal,
        1.0,
    );
    for i in 0..8 {
        let angle = i as f32 * std::f32::consts::TAU / 8.0;
        let rot = Quat::from_rotation_y(angle);
        parts.draw(
            0,
            station
                * transform(
                    rot * Vec3::new(7.0, 0.0, 0.0),
                    Vec3::new(5.0, 0.7, 0.7),
                    rot,
                ),
            steel,
            0.0,
        );
        parts.draw(
            0,
            station
                * transform(
                    rot * Vec3::new(13.0, 1.5, 0.0),
                    Vec3::new(1.5, 2.0, 1.5),
                    rot,
                ),
            steel * 1.2,
            0.0,
        );
        parts.draw(
            0,
            station
                * transform(
                    rot * Vec3::new(14.5, 2.2, 0.0),
                    Vec3::new(0.2, 0.2, 0.8),
                    rot,
                ),
            amber,
            1.5,
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
        gl.shader_source(shader, source);
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
const FRAGMENT: &str = r#"#version 150
in vec3 world;in vec3 surface_normal;in vec3 local_position;
uniform vec3 base_color;uniform vec3 eye;uniform float emission;uniform int material_kind;out vec4 color;
float ark_hash3(vec3 p){return fract(sin(dot(p,vec3(127.1,311.7,74.7)))*43758.5453);}
float ark_noise3(vec3 p){vec3 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);
return mix(mix(mix(ark_hash3(i),ark_hash3(i+vec3(1,0,0)),f.x),mix(ark_hash3(i+vec3(0,1,0)),ark_hash3(i+vec3(1,1,0)),f.x),f.y),
mix(mix(ark_hash3(i+vec3(0,0,1)),ark_hash3(i+vec3(1,0,1)),f.x),mix(ark_hash3(i+vec3(0,1,1)),ark_hash3(i+vec3(1,1,1)),f.x),f.y),f.z);}
float ark_terrain(vec3 p){return ark_noise3(p)*0.53+ark_noise3(p*2.07+13.8)*0.27+ark_noise3(p*4.13-7.1)*0.13+ark_noise3(p*8.41+29.4)*0.07;}
void main(){vec3 n=normalize(surface_normal);if(!gl_FrontFacing)n=-n;vec3 l=normalize(vec3(-0.5,0.8,0.65));vec3 v=normalize(eye-world);
float diffuse=max(dot(n,l),0.0);float facing=max(dot(n,v),0.0);float rim=pow(1.0-facing,3.0);float spec=pow(max(dot(n,normalize(l+v)),0.0),38.0);
vec3 lit;
if(material_kind==1){
    vec3 sphere=normalize(local_position);
    float elevation=ark_terrain(sphere*4.1+vec3(3.2,1.7,9.4));
    float land=smoothstep(0.47,0.51,elevation);
    float aridity=ark_terrain(sphere*9.0+21.5);
    vec3 ocean=mix(vec3(0.018,0.055,0.115),vec3(0.035,0.19,0.22),smoothstep(0.37,0.49,elevation));
    vec3 earth=mix(vec3(0.10,0.18,0.105),vec3(0.36,0.28,0.17),smoothstep(0.42,0.62,aridity));
    float polar=smoothstep(0.79,0.94,abs(sphere.y)+ark_terrain(sphere*14.0)*0.09);
    vec3 surface=mix(mix(ocean,earth,land),vec3(0.73,0.82,0.85),polar);
    vec3 wind=sphere*15.0+vec3(sin(sphere.y*11.0)*0.9,0.0,cos(sphere.y*8.0)*0.7);
    float clouds=smoothstep(0.52,0.68,ark_terrain(wind));
    surface=mix(surface,vec3(0.76,0.83,0.88),clouds*0.88);
    lit=surface*(0.10+diffuse*1.22);
    lit+=vec3(0.46,0.64,0.82)*pow(max(dot(n,normalize(l+v)),0.0),95.0)*(1.0-land)*(1.0-clouds)*0.5;
    lit+=vec3(0.11,0.36,0.75)*pow(1.0-facing,4.0)*(0.23+diffuse*0.85);
}else{
    lit=base_color*(0.16+diffuse*0.95)+vec3(0.1,0.21,0.3)*rim*0.7+vec3(0.8,0.88,1.0)*spec*0.42+base_color*emission;
}
color=vec4(pow(lit/(lit+vec3(0.7)),vec3(0.78)),1.0);}
"#;
const BACK_VERTEX: &str = r#"#version 150
out vec2 uv;void main(){vec2 p=vec2((gl_VertexID<<1)&2,gl_VertexID&2);uv=p;gl_Position=vec4(p*2.0-1.0,0.999,1.0);}
"#;
const BACK_FRAGMENT: &str = r#"#version 150
in vec2 uv;uniform vec2 camera_angles;out vec4 color;
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);}
void main(){vec2 p=uv+camera_angles*vec2(0.12,0.18);float mist=noise(p*4.0)+noise(p*9.0)*0.5+noise(p*23.0)*0.18;
float band=exp(-pow((p.y-0.4+sin(p.x*3.4)*0.16)*3.0,2.0));vec3 c=vec3(0.008,0.014,0.032)+vec3(0.032,0.07,0.125)*mist*band;
c+=vec3(0.055,0.022,0.055)*pow(mist*0.6,3.0)*band;
for(int i=0;i<2;i++){float scale=180.0+float(i)*155.0;vec2 grid=p*scale;vec2 cell=floor(grid);vec2 offset=vec2(hash(cell),hash(cell+13.7));float d=length(fract(grid)-offset);float seed=hash(cell+71.1);float star=(1.0-smoothstep(0.0,0.09,d))*step(0.972,seed);c+=mix(vec3(0.53,0.7,1.0),vec3(1.0,0.87,0.65),seed)*star*(0.5+seed*0.7);}
vec2 sun=p-vec2(0.84,0.82);float flare=0.0018/(dot(sun,sun)+0.006);c+=vec3(0.17,0.28,0.40)*flare;color=vec4(c,1.0);}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extrapolation_is_bounded_and_warp_uses_server_position() {
        let mut pilot = PilotView {
            system_id: HOME_SYSTEM_ID,
            locked_asteroid: None,
            name: "Moving".into(),
            credits: 1000,
            ore: 0,
            location: Location::Station,
            flight: Some(arkline_protocol::FlightState {
                position: [1000.0, 500.0, -200.0],
                velocity: [100.0, 0.0, 0.0],
                heading: [0.0, 0.0, -1.0],
                mode: NavigationMode::Stop,
                updated_at_ms: 1000,
            }),
        };
        assert_eq!(position(&pilot, 900), Vec3::new(10.0, 5.0, -2.0));
        assert!((position(&pilot, 1200).x - 10.2).abs() < 0.0001);
        assert_eq!(position(&pilot, 1350), position(&pilot, 90_000));
        pilot.location = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: 7000,
        };
        assert_eq!(position(&pilot, 1200), Vec3::new(10.0, 5.0, -2.0));
        pilot.location = Location::Docked;
        assert_eq!(position(&pilot, 1200), Vec3::new(10.0, 5.0, -2.0));
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
    fn mining_targets_exact_authoritative_asteroid() {
        for asteroid in arkline_protocol::asteroid_catalog(10000) {
            let center = scene_position(asteroid.position);
            let source = center + Vec3::Z * 10.0;
            let impact = mining_impact(source, &asteroid).unwrap();
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
        let center = scene_position(gate.position);
        for (mesh, model) in gate_parts(&gate) {
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
            let model = asteroid_model(&asteroid);
            for v in &mesh {
                assert!(
                    model
                        .transform_point3(v.p)
                        .distance(scene_position(asteroid.position))
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
        let pilot = PilotView {
            system_id: HOME_SYSTEM_ID,
            locked_asteroid: None,
            name: "Pilot One".into(),
            credits: 100,
            ore: 0,
            location: Location::Station,
            flight: None,
        };
        let before = position(&pilot, 100);
        let _other = lane("Another pilot");
        assert_eq!(before, position(&pilot, 100));
        assert!((lane(&pilot.name)).abs() <= 5.0);
    }
}
