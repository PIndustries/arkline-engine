//! Canonical cockpit UI triangles rendered on the physical display plane.
//! The shared cockpit depth buffer is an input; this pass never clears it.
use super::*;

#[cfg(all(test, target_os = "macos"))]
#[path = "cockpit_displays_gpu_tests.rs"]
mod gpu_tests;

const MAX_DISPLAY_VERTICES: usize = 1_000_000;
const DISPLAY_VERTEX_BYTES: usize = 36;

#[derive(Clone, Copy, Debug)]
pub(super) struct DisplayVertex {
    pub position: Vec3,
    pub uv: egui::Pos2,
    pub color: egui::Color32,
}

pub(super) struct DisplayBatch {
    pub texture: egui::TextureId,
    /// Expanded triangles, after display-local clipping. No projected positions.
    pub vertices: Vec<DisplayVertex>,
}

pub(super) struct CockpitDisplayRenderer {
    gl: Arc<glow::Context>,
    program: glow::Program,
    vao: glow::VertexArray,
    buffer: glow::Buffer,
    matrix: Option<glow::UniformLocation>,
    sampler: Option<glow::UniformLocation>,
    srgb: Option<glow::UniformLocation>,
    srgb_textures: bool,
    destroyed: bool,
}

impl CockpitDisplayRenderer {
    pub(super) unsafe fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        let program = compile(&gl, VERTEX, FRAGMENT)?;
        let vao = match gl.create_vertex_array() {
            Ok(vao) => vao,
            Err(error) => {
                gl.delete_program(program);
                return Err(error);
            }
        };
        let buffer = match gl.create_buffer() {
            Ok(buffer) => buffer,
            Err(error) => {
                gl.delete_vertex_array(vao);
                gl.delete_program(program);
                return Err(error);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        let old_vao = gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING);
        #[cfg(not(target_arch = "wasm32"))]
        let old_buffer = gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING);
        gl.bind_vertex_array(Some(vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
        for (name, count, offset) in [("position", 3, 0), ("uv", 2, 12), ("color", 4, 20)] {
            if let Some(attribute) = gl.get_attrib_location(program, name) {
                gl.enable_vertex_attrib_array(attribute);
                gl.vertex_attrib_pointer_f32(attribute, count, glow::FLOAT, false, 36, offset);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            gl.bind_vertex_array(old_vao);
            gl.bind_buffer(glow::ARRAY_BUFFER, old_buffer);
        }
        #[cfg(target_arch = "wasm32")]
        {
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
        }
        // Match egui_glow's managed-texture upload policy. Its sRGB textures
        // decode to linear on sampling, while egui colors multiply in gamma.
        let srgb_textures = (gl.version().is_embedded && gl.version().major >= 3)
            || gl
                .supported_extensions()
                .iter()
                .any(|name| name.contains("sRGB"));
        Ok(Self {
            matrix: gl.get_uniform_location(program, "local_to_clip"),
            sampler: gl.get_uniform_location(program, "display_texture"),
            srgb: gl.get_uniform_location(program, "texture_is_srgb"),
            gl,
            program,
            vao,
            buffer,
            srgb_textures,
            destroyed: false,
        })
    }

    pub(super) unsafe fn paint(
        &mut self,
        info: egui::PaintCallbackInfo,
        painter: &egui_glow::Painter,
        local_to_clip: Mat4,
        batches: &[DisplayBatch],
    ) {
        if self.destroyed || !local_to_clip.is_finite() || batches.is_empty() {
            return;
        }
        let gl = &self.gl;
        // A color-only callback target cannot enforce physical occlusion.
        if draw_framebuffer_depth_bits(gl) <= 0 {
            return;
        }
        let mut total_vertices = 0usize;
        for batch in batches {
            let Some(total) = total_vertices.checked_add(batch.vertices.len()) else {
                return;
            };
            if total > MAX_DISPLAY_VERTICES
                || batch.vertices.len() % 3 != 0
                || batch
                    .vertices
                    .iter()
                    .any(|vertex| !vertex.position.is_finite() || !vertex.uv.is_finite())
            {
                return;
            }
            total_vertices = total;
        }
        if total_vertices.checked_mul(DISPLAY_VERTEX_BYTES).is_none()
            || i32::try_from(total_vertices).is_err()
        {
            return;
        }
        let viewport = info.viewport_in_pixels();
        if viewport.width_px <= 0 || viewport.height_px <= 0 {
            return;
        }
        let state = DisplayState::capture(gl);
        gl.viewport(
            viewport.left_px,
            viewport.from_bottom_px,
            viewport.width_px,
            viewport.height_px,
        );
        gl.enable(glow::DEPTH_TEST);
        gl.depth_func(glow::LEQUAL);
        gl.depth_mask(false);
        gl.disable(glow::CULL_FACE);
        gl.enable(glow::BLEND);
        gl.blend_equation_separate(glow::FUNC_ADD, glow::FUNC_ADD);
        gl.blend_func_separate(
            glow::ONE,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE_MINUS_DST_ALPHA,
            glow::ONE,
        );
        gl.color_mask(true, true, true, true);
        if !gl.version().is_embedded {
            gl.disable(glow::FRAMEBUFFER_SRGB);
        }
        gl.use_program(Some(self.program));
        gl.bind_vertex_array(Some(self.vao));
        gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.buffer));
        gl.uniform_matrix_4_f32_slice(self.matrix.as_ref(), false, &local_to_clip.to_cols_array());
        gl.uniform_1_i32(self.sampler.as_ref(), 0);
        gl.uniform_1_i32(self.srgb.as_ref(), i32::from(self.srgb_textures));
        gl.active_texture(glow::TEXTURE0);
        gl.bind_sampler(0, None);
        for batch in batches {
            let Some(texture) = painter.texture(batch.texture) else {
                continue;
            };
            let Ok(count) = i32::try_from(batch.vertices.len()) else {
                continue;
            };
            let bytes: Vec<u8> = batch
                .vertices
                .iter()
                .flat_map(|vertex| {
                    let color = vertex
                        .color
                        .to_array()
                        .map(|channel| f32::from(channel) / 255.0);
                    [
                        vertex.position.x,
                        vertex.position.y,
                        vertex.position.z,
                        vertex.uv.x,
                        vertex.uv.y,
                        color[0],
                        color[1],
                        color[2],
                        color[3],
                    ]
                })
                .flat_map(f32::to_ne_bytes)
                .collect();
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, &bytes, glow::STREAM_DRAW);
            gl.draw_arrays(glow::TRIANGLES, 0, count);
        }
        state.restore(gl);
    }

    pub(super) unsafe fn destroy(&mut self) {
        if !self.destroyed {
            self.destroyed = true;
            self.gl.delete_buffer(self.buffer);
            self.gl.delete_vertex_array(self.vao);
            self.gl.delete_program(self.program);
        }
    }
}

/// Restore all mutable state used by this pass. FBO, scissor and depth contents
/// are never changed. egui_glow also rebinds its own draw state after callbacks.
struct DisplayState {
    viewport: [i32; 4],
    depth: bool,
    depth_function: u32,
    depth_mask: bool,
    blend: bool,
    blend_function: [u32; 4],
    blend_equation: [u32; 2],
    cull: bool,
    framebuffer_srgb: bool,
    color_mask: [bool; 4],
    active_texture: u32,
    #[cfg(not(target_arch = "wasm32"))]
    program: Option<glow::Program>,
    #[cfg(not(target_arch = "wasm32"))]
    vao: Option<glow::VertexArray>,
    #[cfg(not(target_arch = "wasm32"))]
    buffer: Option<glow::Buffer>,
    #[cfg(not(target_arch = "wasm32"))]
    texture: Option<glow::Texture>,
    #[cfg(not(target_arch = "wasm32"))]
    sampler: Option<glow::Sampler>,
}

impl DisplayState {
    unsafe fn capture(gl: &glow::Context) -> Self {
        let mut viewport = [0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        let active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        gl.active_texture(glow::TEXTURE0);
        Self {
            viewport,
            depth: gl.is_enabled(glow::DEPTH_TEST),
            depth_function: gl.get_parameter_i32(glow::DEPTH_FUNC) as u32,
            depth_mask: gl.get_parameter_bool(glow::DEPTH_WRITEMASK),
            blend: gl.is_enabled(glow::BLEND),
            blend_function: [
                glow::BLEND_SRC_RGB,
                glow::BLEND_DST_RGB,
                glow::BLEND_SRC_ALPHA,
                glow::BLEND_DST_ALPHA,
            ]
            .map(|parameter| gl.get_parameter_i32(parameter) as u32),
            blend_equation: [glow::BLEND_EQUATION_RGB, glow::BLEND_EQUATION_ALPHA]
                .map(|parameter| gl.get_parameter_i32(parameter) as u32),
            cull: gl.is_enabled(glow::CULL_FACE),
            framebuffer_srgb: !gl.version().is_embedded && gl.is_enabled(glow::FRAMEBUFFER_SRGB),
            color_mask: gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK),
            active_texture,
            #[cfg(not(target_arch = "wasm32"))]
            program: gl.get_parameter_program(glow::CURRENT_PROGRAM),
            #[cfg(not(target_arch = "wasm32"))]
            vao: gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
            #[cfg(not(target_arch = "wasm32"))]
            buffer: gl.get_parameter_buffer(glow::ARRAY_BUFFER_BINDING),
            #[cfg(not(target_arch = "wasm32"))]
            texture: gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
            #[cfg(not(target_arch = "wasm32"))]
            sampler: gl.get_parameter_sampler(glow::SAMPLER_BINDING),
        }
    }

    unsafe fn restore(self, gl: &glow::Context) {
        gl.viewport(
            self.viewport[0],
            self.viewport[1],
            self.viewport[2],
            self.viewport[3],
        );
        gl.depth_func(self.depth_function);
        gl.depth_mask(self.depth_mask);
        gl.color_mask(
            self.color_mask[0],
            self.color_mask[1],
            self.color_mask[2],
            self.color_mask[3],
        );
        gl.blend_func_separate(
            self.blend_function[0],
            self.blend_function[1],
            self.blend_function[2],
            self.blend_function[3],
        );
        gl.blend_equation_separate(self.blend_equation[0], self.blend_equation[1]);
        for (capability, enabled) in [
            (glow::DEPTH_TEST, self.depth),
            (glow::BLEND, self.blend),
            (glow::CULL_FACE, self.cull),
        ] {
            if enabled {
                gl.enable(capability);
            } else {
                gl.disable(capability);
            }
        }
        if !gl.version().is_embedded {
            if self.framebuffer_srgb {
                gl.enable(glow::FRAMEBUFFER_SRGB);
            } else {
                gl.disable(glow::FRAMEBUFFER_SRGB);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vao);
            gl.bind_buffer(glow::ARRAY_BUFFER, self.buffer);
            gl.bind_texture(glow::TEXTURE_2D, self.texture);
            gl.bind_sampler(0, self.sampler);
        }
        // glow 0.16 panics querying WebGL null object bindings. The documented
        // egui callback protocol restores its bindings immediately after return.
        #[cfg(target_arch = "wasm32")]
        {
            gl.use_program(None);
            gl.bind_vertex_array(None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_sampler(0, None);
        }
        gl.active_texture(self.active_texture);
    }
}

const VERTEX: &str = r#"#version 150
in vec3 position;
in vec2 uv;
in vec4 color;
uniform mat4 local_to_clip;
out vec2 texture_uv;
out vec4 vertex_color;
void main() {
    gl_Position = local_to_clip * vec4(position, 1.0);
    texture_uv = uv;
    vertex_color = color;
}
"#;

const FRAGMENT: &str = r#"#version 150
uniform sampler2D display_texture;
uniform bool texture_is_srgb;
in vec2 texture_uv;
in vec4 vertex_color;
out vec4 fragment_color;
vec3 gamma_from_linear(vec3 rgb) {
    bvec3 cutoff = lessThan(rgb, vec3(0.0031308));
    vec3 lower = rgb * 12.92;
    vec3 higher = 1.055 * pow(rgb, vec3(1.0 / 2.4)) - 0.055;
    return mix(higher, lower, vec3(cutoff));
}
void main() {
    vec4 sampled = texture(display_texture, texture_uv);
    if (texture_is_srgb) sampled.rgb = gamma_from_linear(sampled.rgb);
    fragment_color = vertex_color * sampled;
}
"#;
