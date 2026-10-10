//! Native GPU flight view. Presentation only: never issues simulation commands.
use arkline_protocol::{
    gates, systems, Asteroid, Destination, Location, NavigationMode, NavigationTarget, PilotView,
    ShipClass, Snapshot, Stargate, BELT_POSITION, HOME_SYSTEM_ID, JUMP_DURATION_MS, MAX_SHIP_SPEED,
    SPACE_BOUND, STATION_CENTER_POSITION, STATION_DIAMETER_M, STATION_HEIGHT_M, STATION_POSITION,
};
#[cfg(test)]
use arkline_protocol::{WARP_DURATION_MS, WARP_WARMUP_MS};
use eframe::{egui, egui_glow, glow};
use glam::{Mat4, Quat, Vec3};
use glow::HasContext;
use std::sync::{Arc, Mutex};

#[path = "cockpit_maps.rs"]
mod cockpit_maps;
use cockpit_maps::*;

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
#[derive(Clone, Copy, Default)]
struct Space {
    origin: [f64; 3],
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
    input_surface: Option<(egui::Rect, egui::LayerId)>,
    camera_ship_class: Option<ShipClass>,
    tactical: bool,
    other_camera: (f32, f32, f32),
    tactical_overlay: bool,
    drone_camera: bool,
    selected_drone: Option<u64>,
    drone_orbit: bool,
    console_rect: Option<egui::Rect>,
    map_range: f64,
    space: Space,
}
impl FlightView {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let renderer = gl
            .ok_or_else(|| {
                "OpenGL is unavailable. A graphics context is required for the cockpit.".into()
            })
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
            input_surface: None,
            camera_ship_class: None,
            tactical: false,
            other_camera: (0.72, 0.65, 350.0),
            tactical_overlay: true,
            drone_camera: true,
            selected_drone: None,
            drone_orbit: false,
            console_rect: None,
            map_range: 1_000_000.0,
            space: Space::default(),
        }
    }
    pub fn set_drone_camera(&mut self, enabled: bool) {
        self.drone_camera = enabled;
    }
    pub fn set_drone_view(&mut self, selected: Option<u64>, orbit: bool) {
        self.selected_drone = selected;
        self.drone_orbit = orbit;
    }
    pub fn cockpit_console_rect(&self) -> Option<egui::Rect> {
        self.console_rect
    }
    pub fn input_surface(&self) -> Option<(egui::Rect, egui::LayerId)> {
        self.input_surface
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
        mode: super::ViewMode,
        layout: &mut super::layout::Layout,
    ) -> Option<ViewAction> {
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
        let space = Space {
            origin: own_pilot.map_or(STATION_POSITION, |p| display_position(p, now)),
        };
        let warp_tracking =
            own_pilot.is_some_and(|p| matches!(p.location, Location::InTransit { .. }));
        self.target = rebased_camera_target(self.target, self.space, space, warp_tracking);
        self.space = space;
        let cockpit = matches!(mode, super::ViewMode::Cockpit);
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

        let bounds = ui.ctx().screen_rect();
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
            .constrain_to(ui.ctx().screen_rect().shrink(8.0))
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
                if cockpit {
                    ui.label("Pilot seat · full six-axis flight");
                    ui.small("W/S: thrust · A/D: strafe · Space/C: vertical");
                    ui.small("Q/E: roll · Arrows: aim · RMB + mouse: aim");
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
        self.console_rect = cockpit.then(|| {
            let (center, size) = cockpit_screens()[3];
            instrument_rect(rect, center, size).shrink(5.0)
        });
        let pointer_on_instrument = cockpit
            && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| {
                cockpit_screens()
                    .iter()
                    .any(|(center, size)| instrument_rect(rect, *center, *size).contains(p))
            });
        self.input_surface = (!pointer_on_instrument).then_some((rect, ui.layer_id()));
        // Foreground panels own their pointer input, including transparent
        // panel margins. Checking the layer also blocks a drag crossing a HUD.
        let world_input = ui.input(|i| i.pointer.hover_pos()).is_some_and(|pointer| {
            rect.contains(pointer)
                && ui.ctx().layer_id_at(pointer) == Some(ui.layer_id())
                && !(cockpit
                    && cockpit_screens()
                        .iter()
                        .any(|(c, s)| instrument_rect(rect, *c, *s).contains(pointer)))
        });
        if !cockpit && world_input && response.dragged_by(egui::PointerButton::Primary) {
            let delta = ui.input(|i| i.pointer.delta());
            self.yaw -= delta.x * 0.007;
            self.pitch = (self.pitch + delta.y * 0.006).clamp(-1.25, 1.25);
        }
        if !cockpit && world_input && response.hovered() {
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
        let (eye, view) = if cockpit {
            cockpit_camera(
                own_pilot.map_or(Vec3::ZERO, |p| {
                    let spec = p.ship_class.spec();
                    space.position(p, now)
                        + rotation
                            * Vec3::new(
                                spec.length_m as f32 * 0.0005,
                                spec.height_m as f32 * 0.005,
                                0.0,
                            )
                }),
                rotation,
            )
        } else {
            (
                external_eye,
                Mat4::look_at_rh(external_eye, self.target, Vec3::Y),
            )
        };
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
        match &self.renderer {
            Ok(renderer) => {
                let renderer = renderer.clone();
                let frame = Frame {
                    cockpit_vp,
                    drone_camera: false,
                    camera_drone: None,
                    space,
                    cockpit,
                    rotation,
                    asteroids: asteroids.clone(),
                    gates: local_gates.clone(),
                    connected,
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
                let drone_screen = drone_feed_rect(rect);
                let camera_drone = own_pilot.and_then(|p| {
                    fleet(p)
                        .into_iter()
                        .find(|d| Some(d.id) == self.selected_drone)
                        .or_else(|| fleet(p).first().copied())
                });
                let camera_frame = if cockpit
                    && self.drone_camera
                    && connected
                    && now.saturating_sub(snapshot.server_time_ms) <= 1500
                {
                    camera_drone
                        .filter(|d| d.signal_strength > 0.0)
                        .map(|drone| {
                            let mut feed = frame.clone();
                            let (eye, vp) = drone_view_matrix(
                                drone,
                                space,
                                drone_screen.aspect_ratio(),
                                self.drone_orbit,
                                now,
                            );
                            feed.eye = eye;
                            feed.vp = vp;
                            feed.cockpit = false;
                            feed.drone_camera = true;
                            feed.camera_drone = (!self.drone_orbit).then_some(drone.id);
                            feed.docking = None;
                            feed
                        })
                } else {
                    None
                };
                let drone_renderer = renderer.clone();
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
                                asteroid_model(asteroid, space),
                            ));
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
                if !cockpit && (world_input || response.context_menu_opened()) {
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
                            &painter,
                            rect,
                            own,
                            snapshot,
                            selected_distance,
                            now,
                            connected,
                        );
                        if let Some(feed) = camera_frame {
                            painter.with_clip_rect(drone_screen.intersect(rect)).add(
                                egui::PaintCallback {
                                    rect: drone_screen,
                                    callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                                        if let Ok(mut renderer) = drone_renderer.lock() {
                                            unsafe {
                                                renderer.paint(&feed);
                                            }
                                        }
                                    })),
                                },
                            );
                            let overlay = painter.with_clip_rect(drone_screen.intersect(rect));
                            overlay.text(
                                drone_screen.left_top() + egui::vec2(4.0, 3.0),
                                egui::Align2::LEFT_TOP,
                                format!(
                                    "{:?} #{} / {}",
                                    camera_drone.unwrap().class,
                                    camera_drone.unwrap().id,
                                    if self.drone_orbit { "ORBIT" } else { "ONBOARD" }
                                ),
                                egui::FontId::monospace((drone_screen.width() / 18.0).min(12.0)),
                                super::TEAL,
                            );
                            overlay.circle_stroke(
                                drone_screen.center(),
                                4.0,
                                egui::Stroke::new(1.0, super::TEAL),
                            );
                            if let Some(drone) = camera_drone {
                                let meters = space.scene(drone.position).length() * 100.0;
                                overlay.text(
                                    drone_screen.left_bottom() + egui::vec2(4.0, -3.0),
                                    egui::Align2::LEFT_BOTTOM,
                                    format!(
                                        "LINK {:.0}% / {}",
                                        drone.signal_strength * 100.0,
                                        super::format_distance(meters as f64)
                                    ),
                                    egui::FontId::monospace(
                                        (drone_screen.width() / 18.0).min(12.0),
                                    ),
                                    super::TEAL,
                                );
                            }
                        }
                        if self.drone_camera {
                            paint_camera_signal(
                                &painter,
                                drone_screen,
                                camera_drone,
                                now,
                                connected && now.saturating_sub(snapshot.server_time_ms) <= 1500,
                            );
                        }
                        if let Some(action) = paint_cockpit_maps(
                            ui,
                            rect,
                            own,
                            snapshot,
                            now,
                            connected,
                            selected,
                            &mut self.map_range,
                        ) {
                            picked = Some(action);
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
    cockpit_vp: Mat4,
    drone_camera: bool,
    camera_drone: Option<u64>,
    space: Space,
    cockpit: bool,
    rotation: Quat,
    asteroids: Vec<Asteroid>,
    gates: Vec<Stargate>,
    connected: bool,
    vp: Mat4,
    eye: Vec3,
    now: u64,
    pilots: Vec<PilotView>,
    own: String,

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
struct Uniforms {
    model: Option<glow::UniformLocation>,
    color: Option<glow::UniformLocation>,
    emission: Option<glow::UniformLocation>,
    vp: Option<glow::UniformLocation>,
    eye: Option<glow::UniformLocation>,
    material: Option<glow::UniformLocation>,
    sky_inverse: Option<glow::UniformLocation>,
    sky_eye: Option<glow::UniformLocation>,
}
struct Renderer {
    gl: Arc<glow::Context>,
    program: glow::Program,
    background: glow::Program,
    empty: glow::VertexArray,
    meshes: Vec<Mesh>,
    uniforms: Uniforms,
    station_batches: Vec<(usize, Vec3, f32)>,
    destroyed: bool,
    turrets: std::collections::HashMap<(String, u8), TurretState>,
}
impl Renderer {
    unsafe fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        let program = compile(&gl, VERTEX, FRAGMENT)?;
        let background = compile(&gl, BACK_VERTEX, BACK_FRAGMENT)?;
        let empty = gl.create_vertex_array()?;
        let mut meshes = vec![];
        let uniforms = Uniforms {
            model: gl.get_uniform_location(program, "model"),
            color: gl.get_uniform_location(program, "base_color"),
            emission: gl.get_uniform_location(program, "emission"),
            vp: gl.get_uniform_location(program, "view_projection"),
            eye: gl.get_uniform_location(program, "eye"),
            material: gl.get_uniform_location(program, "material_kind"),
            sky_inverse: gl.get_uniform_location(background, "inverse_view_projection"),
            sky_eye: gl.get_uniform_location(background, "camera_eye"),
        };
        let mut upload = vec![
            cube(),
            torus(1.0, 0.12),
            sphere(false),
            rock_mesh(),
            hull(),
            sphere_with_resolution(false, 72, 48),
            torus(1.0, 0.008),
        ];
        let mut station_batches = Vec::new();
        for (vertices, color, emission) in station_batches_geometry() {
            station_batches.push((upload.len(), color, emission));
            upload.push(vertices);
        }
        for vertices in upload {
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
            uniforms,
            station_batches,
            destroyed: false,
            turrets: std::collections::HashMap::new(),
        })
    }
    unsafe fn draw(&self, mesh: usize, model: Mat4, color: Vec3, emission: f32) {
        let gl = &self.gl;
        gl.uniform_matrix_4_f32_slice(self.uniforms.model.as_ref(), false, &model.to_cols_array());
        gl.uniform_3_f32(self.uniforms.color.as_ref(), color.x, color.y, color.z);
        gl.uniform_1_f32(self.uniforms.emission.as_ref(), emission);
        gl.bind_vertex_array(Some(self.meshes[mesh].vao));
        gl.draw_arrays(glow::TRIANGLES, 0, self.meshes[mesh].count);
    }
    unsafe fn paint(&mut self, frame: &Frame) {
        let space = frame.space;
        self.turrets
            .retain(|(name, _), _| frame.pilots.iter().any(|p| &p.name == name));
        for pilot in &frame.pilots {
            for module_id in 0..2 {
                let state = self
                    .turrets
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
        if self.destroyed {
            return;
        }
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
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(true);
        gl.clear_depth_f32(1.0);
        gl.clear(glow::DEPTH_BUFFER_BIT);
        gl.use_program(Some(self.program));
        gl.uniform_matrix_4_f32_slice(self.uniforms.vp.as_ref(), false, &frame.vp.to_cols_array());
        gl.uniform_3_f32(
            self.uniforms.eye.as_ref(),
            frame.eye.x,
            frame.eye.y,
            frame.eye.z,
        );
        // Procedural planet material stays isolated from ship/station surfaces.
        gl.uniform_1_i32(self.uniforms.material.as_ref(), 1);
        // Fixed world anchor near the station; changing render origin must not
        // carry the planet along with a ship traveling astronomical distances.
        let planet = space.scene([30000.0, -18000.0, -60000.0]);
        if planet.distance(frame.eye) < 1100.0 {
            self.draw(
                5,
                transform(planet, Vec3::splat(90.0), Quat::IDENTITY),
                Vec3::new(0.13, 0.22, 0.36),
                0.0,
            );
        }
        gl.uniform_1_i32(self.uniforms.material.as_ref(), 0);
        if space.scene(STATION_CENTER_POSITION).distance(frame.eye) < 1100.0 {
            let model = Mat4::from_translation(space.scene(STATION_CENTER_POSITION));
            for &(mesh, color, emission) in &self.station_batches {
                self.draw(mesh, model, color, emission);
            }
        }
        for asteroid in frame
            .asteroids
            .iter()
            .filter(|a| space.scene(a.position).distance(frame.eye) < 1100.0)
        {
            let color = if asteroid.ore == 0 {
                Vec3::splat(0.10)
            } else {
                Vec3::new(0.36, 0.29, 0.18)
            };
            self.draw(3, asteroid_model(asteroid, space), color, 0.0);
        }
        for gate in frame
            .gates
            .iter()
            .filter(|g| space.scene(g.position).distance(frame.eye) < 1100.0)
        {
            for model in gate_models(gate, space) {
                self.draw(1, model, Vec3::new(0.16, 0.43, 0.65), 0.7);
            }
        }
        for pilot in &frame.pilots {
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
                for (mesh, model, color, emission) in ship_parts(pilot.ship_class) {
                    self.draw(mesh, ship * model, color, emission);
                }
            }
            for module_id in 0..2 {
                if let Some(state) = self.turrets.get(&(pilot.name.clone(), module_id)) {
                    for (mesh, model, color, emission) in
                        turret_parts(pilot.ship_class, state.rotation, module_id)
                    {
                        self.draw(mesh, ship * model, color, emission);
                    }
                }
            }
        }
        for pilot in &frame.pilots {
            for drone in fleet(pilot) {
                if frame.camera_drone == Some(drone.id) && pilot.name == frame.own {
                    continue;
                }
                let base = transform(
                    space.scene(drone.position),
                    Vec3::ONE,
                    orientation(drone.heading).unwrap_or(Quat::IDENTITY),
                );
                self.draw(
                    0,
                    base * transform(Vec3::ZERO, Vec3::new(0.025, 0.009, 0.017), Quat::IDENTITY),
                    match drone.class {
                        arkline_protocol::DroneClass::Recon => Vec3::new(0.12, 0.25, 0.20),
                        arkline_protocol::DroneClass::Mining => Vec3::new(0.50, 0.34, 0.09),
                        arkline_protocol::DroneClass::Attack => Vec3::new(0.48, 0.12, 0.09),
                    },
                    0.1,
                );
                for side in [-1.0, 1.0] {
                    self.draw(
                        0,
                        base * transform(
                            Vec3::new(0.0, 0.0, side * 0.023),
                            Vec3::new(0.012, 0.004, 0.007),
                            Quat::IDENTITY,
                        ),
                        Vec3::new(0.05, 0.7, 0.4),
                        0.8,
                    );
                }
            }
        }
        if frame.connected {
            for pilot in &frame.pilots {
                for drone in fleet(pilot) {
                    if drone.signal_strength <= 0.0 {
                        continue;
                    }
                    let start = space.scene(drone.position);
                    let beam = match &drone.mode {
                        arkline_protocol::DroneMode::Mining { target_id }
                            if drone.mining_cycle.is_some() =>
                        {
                            frame
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
    }
    /// Additive, depth-tested effects do not write depth or obscure UI meshes.
    unsafe fn paint_warp_tunnel(&self, frame: &Frame) {
        let Some(pilot) = frame.pilots.iter().find(|p| p.name == frame.own) else {
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
        // Interior geometry occupies a separate depth layer inside the ship,
        // while retaining the same view/projection as the outside universe.
        self.gl.depth_mask(true);
        self.gl.clear(glow::DEPTH_BUFFER_BIT);
        self.gl.enable(glow::DEPTH_TEST);
        self.gl.disable(glow::BLEND);
        self.gl.uniform_matrix_4_f32_slice(
            self.uniforms.vp.as_ref(),
            false,
            &frame.cockpit_vp.to_cols_array(),
        );
        let base =
            Mat4::from_scale_rotation_translation(Vec3::splat(0.025), frame.rotation, frame.eye);
        let steel = Vec3::new(0.055, 0.08, 0.09);
        let glow = Vec3::new(0.08, 0.62, 0.43);
        // Forward is local +X; glazing is open between structural pillars.
        for side in [-1.0, 1.0] {
            self.draw(
                0,
                base * transform(
                    Vec3::new(0.78, 0.03, side * 0.49),
                    Vec3::new(0.055, 0.62, 0.045),
                    Quat::from_rotation_x(side * 0.22),
                ),
                steel,
                0.0,
            );
            self.draw(
                0,
                base * transform(
                    Vec3::new(0.63, -0.38, side * 0.42),
                    Vec3::new(0.35, 0.15, 0.13),
                    Quat::IDENTITY,
                ),
                steel,
                0.0,
            );
            for i in 0..6 {
                self.draw(
                    0,
                    base * transform(
                        Vec3::new(0.77, -0.34, side * (0.22 + i as f32 * 0.04)),
                        Vec3::splat(0.013),
                        Quat::IDENTITY,
                    ),
                    glow,
                    0.8,
                );
            }
        }
        self.draw(
            0,
            base * transform(
                Vec3::new(0.75, 0.57, 0.0),
                Vec3::new(0.065, 0.035, 0.57),
                Quat::IDENTITY,
            ),
            steel,
            0.0,
        );
        self.draw(
            0,
            base * transform(
                Vec3::new(0.82, -0.50, 0.0),
                Vec3::new(0.22, 0.13, 0.58),
                Quat::IDENTITY,
            ),
            steel,
            0.0,
        );
        for (center, size) in cockpit_screens() {
            self.draw(
                0,
                base * transform(center, size, Quat::IDENTITY),
                glow * 0.23,
                0.25,
            );
            self.draw(
                0,
                base * transform(
                    center - Vec3::X * 0.025,
                    Vec3::new(0.006, size.y - 0.008, size.z - 0.008),
                    Quat::IDENTITY,
                ),
                Vec3::new(0.004, 0.012, 0.010),
                0.1,
            );
        }
    }
    unsafe fn paint_effects(&self, frame: &Frame) {
        let space = frame.space;
        let gl = &self.gl;
        gl.enable(glow::BLEND);
        gl.blend_func_separate(glow::ONE, glow::ONE, glow::ZERO, glow::ONE);
        gl.depth_mask(false);
        for pilot in &frame.pilots {
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
                let effect_scale = Vec3::new(
                    spec.length_m as f32 / 700.0,
                    spec.height_m as f32 / 200.0,
                    spec.width_m as f32 / 500.0,
                );
                for (_, model, _, _) in ship_parts(pilot.ship_class)
                    .into_iter()
                    .filter(|p| p.3 > 0.0)
                {
                    let mount = model.transform_point3(Vec3::ZERO) / effect_scale;
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
                    let Some(asteroid) = frame.asteroids.iter().find(|a| {
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
                    let Some(impact) = mining_impact(source, asteroid, space) else {
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
        for mesh in &self.meshes {
            self.gl.delete_buffer(mesh.buffer);
            self.gl.delete_vertex_array(mesh.vao);
        }
        self.gl.delete_vertex_array(self.empty);
        self.gl.delete_program(self.program);
        self.gl.delete_program(self.background);
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
fn cockpit_screens() -> [(Vec3, Vec3); 6] {
    [
        (Vec3::new(0.72, -0.065, 0.0), Vec3::new(0.020, 0.09, 0.18)),
        (Vec3::new(0.80, -0.23, -0.49), Vec3::new(0.012, 0.13, 0.19)),
        (Vec3::new(0.80, -0.23, 0.49), Vec3::new(0.012, 0.13, 0.19)),
        (Vec3::new(0.65, -0.30, 0.0), Vec3::new(0.020, 0.105, 0.24)),
        (Vec3::new(0.80, 0.02, -0.49), Vec3::new(0.012, 0.085, 0.19)),
        (Vec3::new(0.80, 0.02, 0.49), Vec3::new(0.012, 0.085, 0.19)),
    ]
}

fn drone_feed_rect(viewport: egui::Rect) -> egui::Rect {
    let (center, size) = cockpit_screens()[1];
    let screen = instrument_rect(viewport, center, size);
    screen.shrink(screen.width().min(screen.height()) * 0.05)
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
            format!("FLIGHT / {status}"),
            mode,
            format!("SPEED {}", super::format_speed(speed)),
            format!("FWD {:+.1} m/s", body.x),
            format!("RIGHT {:+.1} m/s", body.z),
            format!("UP {:+.1} m/s", body.y),
            format!("HDG {bearing:05.1}  PITCH {pitch:+.1}"),
            "BODY-AXIS VELOCITY".into(),
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
            format!("MODULES / {status}"),
            module_instrument(own, 0, now),
            module_instrument(own, 1, now),
            format!("HOLD {} / {}", own.ore, arkline_protocol::CARGO_CAPACITY),
            format!("CREDITS {}", own.credits),
            format!(
                "YIELD {} / CYCLE",
                arkline_protocol::mining_spec(own.ship_class).yield_units
            ),
            "SERVER-TIMED MODULES".into(),
            "CARGO CAPACITY".into(),
        ],
    ]
}

fn paint_instruments(
    painter: &egui::Painter,
    viewport: egui::Rect,
    own: &PilotView,
    snapshot: &Snapshot,
    target_distance: Option<f64>,
    now: u64,
    connected: bool,
) {
    let rows = instrument_rows(own, snapshot, target_distance, now, connected);
    let connected = connected && now.saturating_sub(snapshot.server_time_ms) <= 1500;
    let color = if connected { super::TEAL } else { super::AMBER };
    for (index, (center, size)) in cockpit_screens().into_iter().take(3).enumerate() {
        let screen = instrument_rect(viewport, center, size);
        let inner = screen.shrink(screen.width().min(screen.height()) * 0.045);
        let painter = painter.with_clip_rect(inner.intersect(viewport));
        painter.rect_filled(inner, 0.0, egui::Color32::from_rgb(2, 14, 10));
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
                * (own.ore as f32 / arkline_protocol::CARGO_CAPACITY as f32).clamp(0.0, 1.0);
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
fn turret_deck(class: ShipClass, module_id: u8) -> f32 {
    static DECKS: std::sync::OnceLock<[f32; 8]> = std::sync::OnceLock::new();
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
    }) * 2
        + usize::from(module_id.min(1))]
}
fn turret_parts(class: ShipClass, rotation: Quat, module_id: u8) -> Vec<ModelPart> {
    let (mount, radius, barrel) = turret_mount(class, module_id);
    let base = Mat4::from_rotation_translation(rotation, mount);
    let bottom = turret_deck(class, module_id) - radius * 0.3;
    let top = mount.y;
    vec![
        (
            2,
            transform(mount, Vec3::splat(radius), Quat::IDENTITY),
            Vec3::new(0.24, 0.29, 0.31),
            0.0,
        ),
        (
            0,
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
    ]
}
fn ship_geometry(class: ShipClass, ship: Mat4) -> Vec<(Vec<Vertex>, Mat4)> {
    ship_parts(class)
        .into_iter()
        .map(|(mesh, model, _, _)| (mesh_vertices(mesh), ship * model))
        .collect()
}
type ModelPart = (usize, Mat4, Vec3, f32);
fn ship_parts(class: ShipClass) -> Vec<ModelPart> {
    static MODELS: std::sync::OnceLock<[Vec<ModelPart>; 4]> = std::sync::OnceLock::new();
    let models = MODELS.get_or_init(|| arkline_protocol::ALL_SHIP_CLASSES.map(build_ship_parts));
    models[match class {
        ShipClass::Pioneer => 0,
        ShipClass::Prospector => 1,
        ShipClass::Hauler => 2,
        ShipClass::Vanguard => 3,
    }]
    .clone()
}
fn build_ship_parts(class: ShipClass) -> Vec<ModelPart> {
    let mut parts = Parts(Vec::new());
    let hull_color = Vec3::new(0.25, 0.32, 0.38);
    let cargo = Vec3::new(0.36, 0.28, 0.15);
    let accent = Vec3::new(0.07, 0.48, 0.35);
    match class {
        ShipClass::Pioneer => {
            parts.draw(
                4,
                Mat4::from_scale(Vec3::new(0.6, 0.5, 0.6)),
                hull_color,
                0.0,
            );
            for side in [-1.0, 1.0] {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(-0.7, 0.0, side * 0.8),
                        Vec3::new(0.8, 0.12, 0.45),
                        Quat::from_rotation_y(side * 0.25),
                    ),
                    hull_color,
                    0.0,
                );
            }
        }
        ShipClass::Prospector => {
            parts.draw(
                0,
                Mat4::from_scale(Vec3::new(1.6, 0.4, 0.5)),
                hull_color,
                0.0,
            );
            parts.draw(
                4,
                transform(
                    Vec3::new(0.6, 0.0, 0.0),
                    Vec3::new(0.32, 0.5, 0.4),
                    Quat::IDENTITY,
                ),
                hull_color,
                0.0,
            );
            for side in [-1.0, 1.0] {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(-0.2, -0.1, side * 0.9),
                        Vec3::new(0.9, 0.5, 0.3),
                        Quat::IDENTITY,
                    ),
                    cargo,
                    0.0,
                );
                parts.draw(
                    0,
                    transform(
                        Vec3::new(1.0, -0.4, side * 0.8),
                        Vec3::new(1.0, 0.08, 0.09),
                        Quat::IDENTITY,
                    ),
                    hull_color,
                    0.0,
                );
            }
        }
        ShipClass::Hauler => {
            parts.draw(
                0,
                Mat4::from_scale(Vec3::new(2.2, 0.18, 0.4)),
                hull_color,
                0.0,
            );
            for x in [-1.3, -0.4, 0.5] {
                for side in [-1.0, 1.0] {
                    parts.draw(
                        0,
                        transform(
                            Vec3::new(x, 0.0, side * 0.65),
                            Vec3::new(0.4, 0.48, 0.35),
                            Quat::IDENTITY,
                        ),
                        cargo,
                        0.0,
                    );
                }
            }
            parts.draw(
                0,
                transform(
                    Vec3::new(1.7, 0.45, 0.0),
                    Vec3::new(0.42, 0.3, 0.65),
                    Quat::IDENTITY,
                ),
                hull_color,
                0.0,
            );
        }
        ShipClass::Vanguard => {
            parts.draw(
                4,
                Mat4::from_scale(Vec3::new(0.9, 0.55, 0.65)),
                hull_color,
                0.0,
            );
            for side in [-1.0, 1.0] {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(-0.8, 0.0, side * 0.8),
                        Vec3::new(1.4, 0.25, 0.25),
                        Quat::IDENTITY,
                    ),
                    hull_color,
                    0.0,
                );
            }
            for i in 0..4 {
                parts.draw(
                    0,
                    transform(
                        Vec3::new(-1.4 + i as f32 * 0.45, 0.55, 0.0),
                        Vec3::new(0.3, 0.22, 0.25),
                        Quat::IDENTITY,
                    ),
                    hull_color * 1.2,
                    0.0,
                );
            }
        }
    }
    for side in [-1.0, 1.0] {
        parts.draw(
            0,
            transform(
                Vec3::new(-1.8, 0.0, side * 0.5),
                Vec3::new(0.3, 0.22, 0.22),
                Quat::IDENTITY,
            ),
            hull_color * 0.6,
            0.0,
        );
        parts.draw(
            0,
            transform(
                Vec3::new(-2.11, 0.0, side * 0.5),
                Vec3::new(0.01, 0.16, 0.16),
                Quat::IDENTITY,
            ),
            accent,
            1.0,
        );
    }
    // Fit measured vertices, including all appendages, to the public meter spec.
    let (min, max) = parts_bounds(&parts.0);
    let spec = class.spec();
    let dimensions = Vec3::new(
        spec.length_m as f32,
        spec.height_m as f32,
        spec.width_m as f32,
    ) / 100.0;
    let fit =
        Mat4::from_scale(dimensions / (max - min)) * Mat4::from_translation(-(min + max) * 0.5);
    for (_, model, _, _) in &mut parts.0 {
        *model = fit * *model;
    }
    parts.0
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
fn asteroid_model(asteroid: &Asteroid, space: Space) -> Mat4 {
    let f = (asteroid.id % 10000) as f32;
    transform(
        space.scene(asteroid.position),
        Vec3::splat(asteroid.radius as f32 / 100.0),
        Quat::from_euler(glam::EulerRot::XYZ, f * 0.2, f * 0.4, f),
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
fn mining_impact(source: Vec3, asteroid: &Asteroid, space: Space) -> Option<Vec3> {
    let direction = (space.scene(asteroid.position) - source).normalize_or_zero();
    ray_mesh_hit(
        source,
        direction,
        asteroid_model(asteroid, space),
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
    match mesh {
        0 => cube(),
        1 => torus(1.0, 0.12),
        2 => sphere(false),
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
in vec2 uv;uniform mat4 inverse_view_projection;uniform vec3 camera_eye;out vec4 color;
float hash(vec2 p){return fract(sin(dot(p,vec2(127.1,311.7)))*43758.5453);}
float noise(vec2 p){vec2 i=floor(p),f=fract(p);f=f*f*(3.0-2.0*f);return mix(mix(hash(i),hash(i+vec2(1,0)),f.x),mix(hash(i+vec2(0,1)),hash(i+vec2(1,1)),f.x),f.y);}
void main(){vec4 farpoint=inverse_view_projection*vec4(uv*2.0-1.0,1.0,1.0);vec3 direction=normalize(farpoint.xyz/farpoint.w-camera_eye);vec2 p=vec2(atan(direction.z,direction.x)/6.2831853+0.5,asin(clamp(direction.y,-1.0,1.0))/3.1415927+0.5);float mist=noise(p*4.0)+noise(p*9.0)*0.5+noise(p*23.0)*0.18;
float band=exp(-pow((p.y-0.4+sin(p.x*3.4)*0.16)*3.0,2.0));vec3 c=vec3(0.008,0.014,0.032)+vec3(0.032,0.07,0.125)*mist*band;
c+=vec3(0.055,0.022,0.055)*pow(mist*0.6,3.0)*band;
for(int i=0;i<2;i++){float scale=180.0+float(i)*155.0;vec2 grid=p*scale;vec2 cell=floor(grid);vec2 offset=vec2(hash(cell),hash(cell+13.7));float d=length(fract(grid)-offset);float seed=hash(cell+71.1);float star=(1.0-smoothstep(0.0,0.09,d))*step(0.972,seed);c+=mix(vec3(0.53,0.7,1.0),vec3(1.0,0.87,0.65),seed)*star*(0.5+seed*0.7);}
vec2 sun=p-vec2(0.84,0.82);float flare=0.0018/(dot(sun,sun)+0.006);c+=vec3(0.17,0.28,0.40)*flare;color=vec4(c,1.0);}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drone_camera_uses_drone_pose_and_stays_inside_physical_screen() {
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
        let feed = drone_feed_rect(viewport);
        let (center, size) = cockpit_screens()[1];
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
            egui::vec2(1100.0, 768.0),
            egui::vec2(1800.0, 1200.0),
            egui::vec2(2560.0, 1440.0),
        ] {
            let viewport = egui::Rect::from_min_size(egui::pos2(17.0, 29.0), size);
            let vp = Mat4::perspective_rh_gl(COCKPIT_FOV, viewport.aspect_ratio(), 0.0002, 10.0)
                * Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y);
            let screens = cockpit_screens().map(|(center, size)| {
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

    #[test]
    fn cockpit_telemetry_uses_ship_state_and_marks_disconnection_stale() {
        let mut pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"InstrumentQA", "credits":123, "ore":225, "location":Location::Belt,
            "system_id":2, "locked_asteroid":7
        }))
        .unwrap();
        pilot.flight = Some(arkline_protocol::FlightState {
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
        assert!(rows[2].contains(&"CREDITS 123".to_string()));
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
            std::hint::black_box(mining_impact(source, &rock, space));
        }
        let uncached = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..500 {
            std::hint::black_box(Mat4::from_translation(space.scene(STATION_CENTER_POSITION)));
            for gate in &gates {
                std::hint::black_box(gate_models(gate, space));
            }
            std::hint::black_box(mining_impact(source, &rock, space));
        }
        eprintln!("CPU render preparation500 iterations: reconstructed uncached {:?}; cached {:?}. Station draws {} -> {}",uncached,start.elapsed(),station_parts(space).len(),batches.len());
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
    fn mining_targets_exact_authoritative_asteroid() {
        for asteroid in arkline_protocol::asteroid_catalog(10000) {
            let space = Space {
                origin: asteroid.position,
            };
            let center = space.scene(asteroid.position);
            let source = center + Vec3::Z * 10.0;
            let impact = mining_impact(source, &asteroid, space).unwrap();
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
            let model = asteroid_model(&asteroid, space);
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
