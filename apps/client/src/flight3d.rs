//! Native GPU flight view. Presentation only: never issues simulation commands.
use arkline_protocol::{Destination, Location, PilotView, Snapshot, WARP_DURATION_MS};
use eframe::{egui, egui_glow, glow};
use glam::{Mat4, Quat, Vec3};
use glow::HasContext;
use std::sync::{Arc, Mutex};

pub fn route_progress(location: &Location, now: u64) -> f32 {
    match *location {
        Location::Docked | Location::Station => 0.0,
        Location::Belt => 1.0,
        Location::InTransit {
            destination,
            arrives_at_ms,
        } => {
            let fraction = 1.0
                - arrives_at_ms.saturating_sub(now).min(WARP_DURATION_MS) as f32
                    / WARP_DURATION_MS as f32;
            if destination == Destination::Belt {
                fraction
            } else {
                1.0 - fraction
            }
        }
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
fn position(pilot: &PilotView, now: u64) -> Vec3 {
    Vec3::new(
        -45.0 + route_progress(&pilot.location, now) * 105.0,
        lane(&pilot.name) * 1.8,
        20.0 + lane(&pilot.name) * 2.0,
    )
}

pub struct FlightView {
    renderer: Result<Arc<Mutex<Renderer>>, String>,
    yaw: f32,
    pitch: f32,
    distance: f32,
    target: Vec3,
    follow: bool,
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
            distance: 42.0,
            target: Vec3::new(-45.0, 0.0, 20.0),
            follow: true,
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
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &Snapshot,
        pilot_name: &str,
        now: u64,
        connected: bool,
    ) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("FLIGHT CAMERA")
                    .color(super::TEAL)
                    .strong(),
            );
            if ui.button("Focus my ship").clicked() {
                self.follow = true;
                self.distance = 27.0;
            }
            if ui.button("Sector overview").clicked() {
                self.follow = false;
                self.distance = 150.0;
                self.yaw = 0.25;
                self.pitch = 0.65;
            }
            if ui.button("Reset camera").clicked() {
                self.follow = true;
                self.distance = 42.0;
                self.yaw = 0.72;
                self.pitch = 0.35;
            }
        });
        ui.small("Drag to orbit · Scroll to zoom · Flight controls remain server authoritative");
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(100.0, 150.0)),
            egui::Sense::drag(),
        );
        if response.dragged() {
            let delta = ui.input(|i| i.pointer.delta());
            self.yaw -= delta.x * 0.007;
            self.pitch = (self.pitch + delta.y * 0.006).clamp(-1.25, 1.25);
        }
        if response.hovered() {
            self.distance = (self.distance
                * (-ui.input(|i| i.smooth_scroll_delta.y) * 0.002).exp())
            .clamp(9.0, 260.0);
        }
        let desired = if self.follow {
            snapshot
                .pilots
                .iter()
                .find(|p| p.name == pilot_name)
                .map(|p| position(p, now))
                .unwrap_or(Vec3::ZERO)
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
                    vp,
                    eye,
                    now,
                    pilots: snapshot.pilots.clone(),
                    own: pilot_name.to_string(),
                    yaw: self.yaw,
                    pitch: self.pitch,
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
                for pilot in &snapshot.pilots {
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
                let painter = ui.painter_at(rect);
                painter.text(
                    rect.left_top() + egui::vec2(18.0, 16.0),
                    egui::Align2::LEFT_TOP,
                    "SECTOR 01 / ARKLINE ORBITAL",
                    egui::FontId::monospace(12.0),
                    super::TEAL,
                );
                painter.text(
                    rect.left_bottom() + egui::vec2(18.0, -16.0),
                    egui::Align2::LEFT_BOTTOM,
                    if self.follow {
                        "CAMERA: SHIP TRACKING"
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
    }
}
struct Frame {
    vp: Mat4,
    eye: Vec3,
    now: u64,
    pilots: Vec<PilotView>,
    own: String,
    yaw: f32,
    pitch: f32,
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
            sphere(true),
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
        let station = Mat4::from_translation(Vec3::new(-45.0, 0.0, 0.0));
        self.draw(1, station * Mat4::from_scale(Vec3::splat(13.0)), steel, 0.0);
        self.draw(
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
        self.draw(
            1,
            station * transform(Vec3::new(0.0, -2.0, 0.0), Vec3::splat(8.5), Quat::IDENTITY),
            steel * 0.7,
            0.0,
        );
        self.draw(
            0,
            station * Mat4::from_scale(Vec3::new(2.8, 5.0, 2.8)),
            steel,
            0.0,
        );
        self.draw(
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
            self.draw(
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
            self.draw(
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
            self.draw(
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
        for i in 0..115 {
            let f = i as f32;
            let angle = f * 2.39996;
            let radius = 6.0 + (i % 13) as f32 * 2.9;
            let p = Vec3::new(
                62.0 + angle.cos() * radius,
                (f * 1.71).sin() * 7.0,
                angle.sin() * radius,
            );
            let scale = 0.6 + (i % 7) as f32 * 0.28;
            self.draw(
                3,
                transform(
                    p,
                    Vec3::new(scale, scale * 0.76, scale * 1.3),
                    Quat::from_euler(glam::EulerRot::XYZ, f * 0.2, f * 0.4, f),
                ),
                Vec3::new(0.31, 0.27, 0.24),
                0.0,
            );
        }
        for pilot in &frame.pilots {
            let own = pilot.name == frame.own;
            let accent = if own { teal } else { amber };
            let dir = heading(&pilot.location);
            let yaw = if dir < 0.0 { std::f32::consts::PI } else { 0.0 };
            let ship = transform(
                position(pilot, frame.now),
                Vec3::ONE,
                Quat::from_rotation_y(yaw),
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
                let warp = matches!(pilot.location, Location::InTransit { .. });
                self.draw(
                    2,
                    ship * transform(
                        Vec3::new(if warp { -5.0 } else { -3.5 }, 0.0, side * 1.5),
                        Vec3::new(if warp { 2.1 } else { 0.7 }, 0.16, 0.16),
                        Quat::IDENTITY,
                    ),
                    Vec3::new(0.3, 0.65, 1.0),
                    2.0,
                );
            }
        }
        gl.disable(glow::DEPTH_TEST);
        gl.depth_mask(false);
        gl.bind_vertex_array(None);
        gl.use_program(None);
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
fn transform(p: Vec3, scale: Vec3, rotation: Quat) -> Mat4 {
    Mat4::from_scale_rotation_translation(scale, rotation, p)
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
    fn warp_routes_match_both_directions_and_clamp() {
        let out = Location::InTransit {
            destination: Destination::Belt,
            arrives_at_ms: 5000,
        };
        let back = Location::InTransit {
            destination: Destination::Station,
            arrives_at_ms: 5000,
        };
        for (now, expected) in [(0, 0.0), (2000, 0.0), (3500, 0.5), (5000, 1.0), (9000, 1.0)] {
            assert_eq!(route_progress(&out, now), expected);
            assert_eq!(route_progress(&back, now), 1.0 - expected);
        }
        assert_eq!(heading(&out), 1.0);
        assert_eq!(heading(&back), -1.0);
    }
    #[test]
    fn geometry_is_finite_with_valid_normals_and_nondegenerate_faces() {
        for mesh in [
            cube(),
            torus(1.0, 0.12),
            sphere(false),
            sphere(true),
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
            name: "Pilot One".into(),
            credits: 100,
            ore: 0,
            location: Location::Station,
        };
        let before = position(&pilot, 100);
        let _other = lane("Another pilot");
        assert_eq!(before, position(&pilot, 100));
        assert!((lane(&pilot.name)).abs() <= 5.0);
    }
}
