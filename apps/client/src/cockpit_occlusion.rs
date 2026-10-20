//! Exact triangle instances shared by cockpit depth and pointer rays.
//! No depth from the world projection is ever reused by this module.
use super::*;
use std::ops::Range;

/// Retain the exact vectors used by Renderer::new's upload loop, in upload order.
/// In particular mesh_vertices(3/5/6/7) is NOT an exact GPU mesh resolver.
pub(super) struct DepthMeshCatalog {
    meshes: Vec<Arc<[Vertex]>>,
    bounds: Vec<(Vec3, Vec3)>,
    finite: Vec<bool>,
}

impl DepthMeshCatalog {
    pub(super) fn new(meshes: Vec<Arc<[Vertex]>>) -> Self {
        let bounds = meshes
            .iter()
            .map(|mesh| {
                mesh.iter().fold(
                    (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
                    |(min, max), vertex| (min.min(vertex.p), max.max(vertex.p)),
                )
            })
            .collect();
        let finite = meshes
            .iter()
            .map(|mesh| mesh.iter().all(|vertex| vertex.p.is_finite()))
            .collect();
        Self {
            meshes,
            bounds,
            finite,
        }
    }
    pub(super) fn uploads(&self) -> &[Arc<[Vertex]>] {
        &self.meshes
    }
}

fn valid_affine(matrix: Mat4) -> bool {
    matrix.is_finite()
        && matrix.x_axis.w == 0.0
        && matrix.y_axis.w == 0.0
        && matrix.z_axis.w == 0.0
        && matrix.w_axis.w == 1.0
        && matrix.determinant().is_finite()
        && matrix.determinant().abs() > 1e-20
        && matrix.inverse().is_finite()
}

#[derive(Clone)]
pub(super) struct DepthDraw {
    /// Real GPU upload slot; strip paint-role tags with mesh_index first.
    pub(super) mesh: usize,
    pub(super) model: Mat4,
    inverse_model: Mat4,
    /// Vertex range, not triangle indices. Must have triangle-aligned endpoints.
    pub(super) vertices: Range<usize>,
}

impl DepthDraw {
    pub(super) fn whole(catalog: &DepthMeshCatalog, mesh: usize, model: Mat4) -> Option<Self> {
        let mesh = mesh_index(mesh);
        let vertices = 0..catalog.meshes.get(mesh)?.len();
        let result = Self {
            mesh,
            model,
            inverse_model: model.inverse(),
            vertices,
        };
        result.valid(catalog).then_some(result)
    }

    fn valid(&self, catalog: &DepthMeshCatalog) -> bool {
        valid_affine(self.model)
            && self.vertices.start <= self.vertices.end
            && self.vertices.start.is_multiple_of(3)
            && self.vertices.end.is_multiple_of(3)
            && self.vertices.end <= i32::MAX as usize
            && catalog.meshes.get(self.mesh).is_some_and(|mesh| {
                mesh.get(self.vertices.clone()).is_some_and(|vertices| {
                    vertices.is_empty() || {
                        let (min, max) = catalog.bounds[self.mesh];
                        min.is_finite() && max.is_finite() && catalog.finite[self.mesh]
                    }
                })
            })
    }
}

/// Build this before processing this frame's pointer events and capture the same
/// Arc in Frame. A list collected during the subsequent GL callback is too late
/// for current-frame interaction. Supplied draws define the coverage of the pass.
pub(super) struct CockpitOcclusion {
    class: ShipClass,
    pub(super) catalog: Arc<DepthMeshCatalog>,
    pub(super) external: Vec<DepthDraw>,
    pub(super) own: Vec<DepthDraw>,
    valid: bool,
}

impl CockpitOcclusion {
    pub(super) fn new(
        class: ShipClass,
        catalog: Arc<DepthMeshCatalog>,
        external: Vec<DepthDraw>,
        own: Vec<DepthDraw>,
    ) -> Option<Self> {
        let valid = external.iter().chain(&own).all(|draw| draw.valid(&catalog));
        valid.then_some(Self {
            class,
            catalog,
            external,
            own,
            valid,
        })
    }

    pub(super) fn valid(&self) -> bool {
        self.valid
    }

    fn nearest(&self, ray: SceneRay, vp: Mat4, max_distance: f32) -> Option<f32> {
        self.external
            .iter()
            .chain(&self.own)
            .filter_map(|draw| {
                let local_origin = draw.inverse_model.transform_point3(ray.origin);
                let local_direction = draw.inverse_model.transform_vector3(ray.direction);
                let (min, max) = self.catalog.bounds[draw.mesh];
                if !ray_bounds(local_origin, local_direction, min, max, max_distance) {
                    return None;
                }
                let vertices = &self.catalog.meshes[draw.mesh][draw.vertices.clone()];
                let t = ray_mesh_hit(ray.origin, ray.direction, draw.model, vertices)?;
                if t >= max_distance {
                    return None;
                }
                let clip = vp * (ray.origin + ray.direction * t).extend(1.0);
                // Match the rasterizer's near/far clipping. Ray origin is already
                // on the near plane, so fragments before it cannot reject input.
                (clip.is_finite() && clip.w > 0.0 && clip.z >= -clip.w && clip.z <= clip.w)
                    .then_some(t)
            })
            .min_by(f32::total_cmp)
    }

    /// Select the nearest physical display face that is not hidden by an actual
    /// supplied triangle. Use this same gate for hover, press, release and touch.
    /// Invalid/stale frame state must not fall back to surface.unproject alone.
    pub(super) fn pick(
        &self,
        vp: Mat4,
        viewport: egui::Rect,
        pointer: egui::Pos2,
        faces: &[DisplayFace],
    ) -> Option<DisplayHit> {
        if !self.valid() {
            return None;
        }
        let ray = SceneRay::screen(vp, viewport, pointer)?;
        let hit = faces
            .iter()
            .filter_map(|face| face.hit(ray, vp))
            .min_by(|a, b| a.scene_distance.total_cmp(&b.scene_distance))?;
        self.unblocked(ray, vp, hit)
    }

    fn unblocked(&self, ray: SceneRay, vp: Mat4, hit: DisplayHit) -> Option<DisplayHit> {
        self.nearest(ray, vp, hit.scene_distance)
            .is_none()
            .then_some(hit)
    }

    pub(super) fn hit_surface(
        &self,
        vp: Mat4,
        viewport: egui::Rect,
        pointer: egui::Pos2,
        index: usize,
        mount: Mat4,
        canvas: egui::Rect,
    ) -> Option<egui::Pos2> {
        if !self.valid() {
            return None;
        }
        let ray = SceneRay::screen(vp, viewport, pointer)?;
        let requested = DisplayFace::new(self.class, index, mount, canvas)?;
        let mut hit = requested.hit(ray, vp)?;
        // Reject other physical displays before testing blocker triangles. This
        // keeps the common off-display/nonmatching hover path constant work.
        if index != 0 {
            for candidate in 1..6 {
                if candidate == index {
                    continue;
                }
                let Some(face) = DisplayFace::new(
                    self.class,
                    candidate,
                    mount,
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0)),
                ) else {
                    continue;
                };
                if let Some(other) = face.hit(ray, vp) {
                    if other.scene_distance < hit.scene_distance {
                        hit = other;
                    }
                }
            }
        }
        if hit.index != index {
            return None;
        }
        self.unblocked(ray, vp, hit).map(|hit| hit.canvas)
    }
}

fn ray_bounds(origin: Vec3, direction: Vec3, min: Vec3, max: Vec3, max_distance: f32) -> bool {
    let mut enter = 0.0f32;
    let mut leave = max_distance;
    for axis in 0..3 {
        if direction[axis].abs() < 1e-12 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return false;
            }
        } else {
            let a = (min[axis] - origin[axis]) / direction[axis];
            let b = (max[axis] - origin[axis]) / direction[axis];
            enter = enter.max(a.min(b));
            leave = leave.min(a.max(b));
            if leave < enter {
                return false;
            }
        }
    }
    true
}

#[derive(Clone, Copy)]
struct SceneRay {
    origin: Vec3,
    direction: Vec3,
}

impl SceneRay {
    fn screen(vp: Mat4, viewport: egui::Rect, pointer: egui::Pos2) -> Option<Self> {
        if !vp.is_finite() || !viewport.is_finite() || !pointer.is_finite() {
            return None;
        }
        let (origin, direction) = screen_ray(vp, viewport, pointer)?;
        (origin.is_finite() && direction.is_finite() && direction.length_squared() > 0.5).then_some(
            Self {
                origin,
                direction: direction.normalize(),
            },
        )
    }
}

/// Move only the content/hit plane toward the seated observer. Source geometry
/// and hull dimensions stay unchanged. Pioneer: 0.1 mm; other mounts: 0.25 mm.
pub(super) const DISPLAY_FRONT_BIAS_LAYOUT: f32 = 0.000_1;

#[derive(Clone)]
pub(super) struct DisplayFace {
    pub(super) index: usize,
    pub(super) mount: Mat4,
    pub(super) canvas: egui::Rect,
    center: Vec3,
    half: Vec3,
    plane_x: f32,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DisplayHit {
    pub(super) index: usize,
    pub(super) canvas: egui::Pos2,
    pub(super) scene_distance: f32,
}

impl DisplayFace {
    pub(super) fn new(
        class: ShipClass,
        index: usize,
        mount: Mat4,
        canvas: egui::Rect,
    ) -> Option<Self> {
        let (center, half) = *cockpit_screens(class).get(index)?;
        if !valid_affine(mount) || !canvas.is_finite() || !canvas.is_positive() {
            return None;
        }
        Some(Self {
            index,
            mount,
            canvas,
            center,
            half: Vec3::new(0.0, half.y - 0.008, half.z - 0.008),
            plane_x: center.x - 0.025 - 0.006 - DISPLAY_FRONT_BIAS_LAYOUT,
        })
    }

    /// Map clipped canonical UI vertices through this method for GPU painting.
    /// This is the exact same plane used for input; do not separately bias input.
    pub(super) fn local_point(&self, point: egui::Pos2) -> Option<Vec3> {
        if !point.is_finite() {
            return None;
        }
        let u = (point.x - self.canvas.left()) / self.canvas.width();
        let v = (point.y - self.canvas.top()) / self.canvas.height();
        Some(Vec3::new(
            self.plane_x,
            self.center.y + (0.5 - v) * self.half.y * 2.0,
            self.center.z + (u - 0.5) * self.half.z * 2.0,
        ))
    }

    fn hit(&self, ray: SceneRay, vp: Mat4) -> Option<DisplayHit> {
        let inverse = self.mount.inverse();
        let origin = inverse.transform_point3(ray.origin);
        // DO NOT normalize this vector: t must remain scene distance even when
        // the mount has scale .01 or .025 and the hull has nonuniform scaling.
        let direction = inverse.transform_vector3(ray.direction);
        if !origin.is_finite() || !direction.is_finite() || direction.x <= 1e-7 {
            return None;
        }
        let local_to_clip = vp * self.mount;
        let camera = local_to_clip.inverse() * glam::Vec4::new(0.0, 0.0, 1.0, 0.0);
        if !camera.is_finite() || camera.w.abs() < 1e-8 || camera.x / camera.w >= self.plane_x {
            return None;
        }
        let t = (self.plane_x - origin.x) / direction.x;
        if !t.is_finite() || t < 0.0 {
            return None;
        }
        let local = origin + direction * t;
        let clip = local_to_clip * local.extend(1.0);
        if !clip.is_finite() || clip.w <= 0.0 || clip.z < -clip.w || clip.z > clip.w {
            return None;
        }
        let u = (local.z - self.center.z) / (2.0 * self.half.z) + 0.5;
        let v = 0.5 - (local.y - self.center.y) / (2.0 * self.half.y);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return None;
        }
        Some(DisplayHit {
            index: self.index,
            scene_distance: t,
            canvas: self.canvas.min + egui::vec2(u * self.canvas.width(), v * self.canvas.height()),
        })
    }
}

/// Only call inside Renderer::paint_cockpit, immediately after its depth clear
/// and before own cockpit geometry, on the existing full-flight viewport/target.
/// This method neither binds a framebuffer nor clears any buffer.
pub(super) struct CockpitDepthProgram {
    program: glow::Program,
    vp: glow::UniformLocation,
    model: glow::UniformLocation,
}

impl CockpitDepthProgram {
    pub(super) unsafe fn new(gl: &glow::Context) -> Result<Self, String> {
        let program = compile(gl, DEPTH_VERTEX, DEPTH_FRAGMENT)?;
        let uniforms = gl
            .get_uniform_location(program, "depth_vp")
            .zip(gl.get_uniform_location(program, "depth_model"));
        let Some((vp, model)) = uniforms else {
            gl.delete_program(program);
            return Err("Missing cockpit depth uniforms".into());
        };
        Ok(Self { program, vp, model })
    }

    /// Return false if no safe matching depth was established. The caller must
    /// then suppress physical display painting/input for that frame.
    pub(super) unsafe fn paint_external(
        &self,
        renderer: &Renderer,
        vp: Mat4,
        geometry: &CockpitOcclusion,
    ) -> bool {
        let gl = &renderer.gl;
        if !vp.is_finite()
            || !geometry.valid()
            || gl.get_parameter_i32(glow::DEPTH_BITS) <= 0
            || geometry.external.iter().any(|draw| {
                renderer
                    .meshes
                    .get(draw.mesh)
                    .is_none_or(|mesh| draw.vertices.end > mesh.count as usize)
            })
        {
            return false;
        }
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(true);
        gl.depth_range_f32(0.0, 1.0);
        gl.color_mask(false, false, false, false);
        gl.disable(glow::BLEND);
        gl.disable(glow::CULL_FACE); // ray_mesh_hit is also two sided.
        gl.disable(glow::STENCIL_TEST);
        gl.disable(glow::POLYGON_OFFSET_FILL);
        gl.disable(glow::SAMPLE_ALPHA_TO_COVERAGE);
        #[cfg(not(target_arch = "wasm32"))]
        let polygon_mode = {
            let mut mode = [0; 2];
            gl.get_parameter_i32_slice(glow::POLYGON_MODE, &mut mode);
            gl.polygon_mode(glow::FRONT_AND_BACK, glow::FILL);
            mode[0] as u32
        };
        gl.use_program(Some(self.program));
        gl.uniform_matrix_4_f32_slice(Some(&self.vp), false, &vp.to_cols_array());
        for draw in &geometry.external {
            gl.uniform_matrix_4_f32_slice(Some(&self.model), false, &draw.model.to_cols_array());
            gl.bind_vertex_array(Some(renderer.meshes[draw.mesh].vao));
            gl.draw_arrays(
                glow::TRIANGLES,
                draw.vertices.start as i32,
                (draw.vertices.end - draw.vertices.start) as i32,
            );
        }
        #[cfg(not(target_arch = "wasm32"))]
        gl.polygon_mode(glow::FRONT_AND_BACK, polygon_mode);
        // Defined continuation state for the immediately following own cockpit
        // draw_material calls. This is not a general arbitrary-callback helper.
        gl.color_mask(true, true, true, true);
        gl.bind_vertex_array(None);
        gl.use_program(Some(renderer.program));
        gl.uniform_matrix_4_f32_slice(renderer.uniforms.vp.as_ref(), false, &vp.to_cols_array());
        true
    }

    pub(super) unsafe fn destroy(&self, gl: &glow::Context) {
        gl.delete_program(self.program);
    }
}

const DEPTH_VERTEX: &str = r#"#version 150
in vec3 position;
uniform mat4 depth_model;
uniform mat4 depth_vp;
void main() { gl_Position = depth_vp * depth_model * vec4(position, 1.0); }
"#;
const DEPTH_FRAGMENT: &str = r#"#version 150
void main() {}
"#;

#[cfg(test)]
#[path = "cockpit_occlusion_tests.rs"]
mod tests;
