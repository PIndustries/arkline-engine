//! Camera-relative, bounded raster sun shadows. No ray tracing or distant-world promise.
use super::*;
const MAX_CASTERS: usize = 256;
const RADIUS: f32 = 24.0; // 2.4 km, scene units are 100 metres.
const DEPTH: f32 = 320.0;
const UNIT: u32 = 3;
pub(super) const GLSL: &str = include_str!("shaders/shadows.glsl");
const VERT: &str = "#version 150\nin vec3 position;uniform mat4 model;uniform mat4 light_matrix;void main(){gl_Position=light_matrix*model*vec4(position,1.0);}";
const FRAG: &str = "#version 150\nvoid main(){}";

pub(super) struct SunShadows {
    framebuffer: glow::Framebuffer,
    texture: glow::Texture,
    program: glow::Program,
    model: Option<glow::UniformLocation>,
    matrix: Option<glow::UniformLocation>,
    size: i32,
}
fn light_matrix(eye: Vec3, size: i32) -> Mat4 {
    let l = Vec3::new(-0.5, 0.8, 0.65).normalize();
    let rotation = Mat4::look_at_rh(Vec3::ZERO, -l, Vec3::Y);
    let mut center = rotation.transform_point3(eye);
    let texel = 2.0 * RADIUS / size.max(1) as f32;
    center.x = (center.x / texel).floor() * texel;
    center.y = (center.y / texel).floor() * texel;
    let center = rotation.inverse().transform_point3(center);
    Mat4::orthographic_rh_gl(-RADIUS, RADIUS, -RADIUS, RADIUS, 0.1, DEPTH * 2.0)
        * Mat4::look_at_rh(center + l * DEPTH, center, Vec3::Y)
}
fn push_caster(casters: &mut Vec<(usize, Mat4)>, caster: (usize, Mat4), eye: Vec3) {
    if casters.len() < MAX_CASTERS {
        casters.push(caster);
        return;
    }
    let distance = |m: Mat4| m.w_axis.truncate().distance_squared(eye);
    let farthest = casters
        .iter()
        .enumerate()
        .max_by(|a, b| distance(a.1 .1).total_cmp(&distance(b.1 .1)))
        .map(|x| x.0)
        .unwrap();
    if distance(caster.1) < distance(casters[farthest].1) {
        casters[farthest] = caster;
    }
}
impl SunShadows {
    unsafe fn create(gl: &glow::Context, size: i32) -> Result<Self, String> {
        let program = compile(gl, VERT, FRAG)?;
        let framebuffer = match gl.create_framebuffer() {
            Ok(v) => v,
            Err(e) => {
                gl.delete_program(program);
                return Err(e);
            }
        };
        let texture = match gl.create_texture() {
            Ok(v) => v,
            Err(e) => {
                gl.delete_framebuffer(framebuffer);
                gl.delete_program(program);
                return Err(e);
            }
        };
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::DEPTH_COMPONENT24 as i32,
            size,
            size,
            0,
            glow::DEPTH_COMPONENT,
            glow::UNSIGNED_INT,
            glow::PixelUnpackData::Slice(None),
        );
        for (p, v) in [
            (glow::TEXTURE_MIN_FILTER, glow::NEAREST),
            (glow::TEXTURE_MAG_FILTER, glow::NEAREST),
            (glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE),
            (glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameter_i32(glow::TEXTURE_2D, p, v as i32);
        }
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(framebuffer));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::DEPTH_ATTACHMENT,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        gl.draw_buffers(&[glow::NONE]);
        gl.read_buffer(glow::NONE);
        let result = Self {
            framebuffer,
            texture,
            program,
            model: gl.get_uniform_location(program, "model"),
            matrix: gl.get_uniform_location(program, "light_matrix"),
            size,
        };
        if gl.check_framebuffer_status(glow::FRAMEBUFFER) != glow::FRAMEBUFFER_COMPLETE {
            result.destroy(gl);
            return Err("Sun shadow depth framebuffer unavailable".into());
        }
        Ok(result)
    }
    pub(super) unsafe fn destroy(self, gl: &glow::Context) {
        gl.delete_texture(self.texture);
        gl.delete_framebuffer(self.framebuffer);
        gl.delete_program(self.program);
    }
}
impl Renderer {
    /// Returns the prior unit-3 texture and active unit for restoration after the
    /// color pass; never queries a nullable WebGL framebuffer (caller supplies it).
    pub(super) unsafe fn paint_sun_shadow(
        &mut self,
        frame: &Frame,
        target: Option<glow::Framebuffer>,
        size: i32,
    ) -> (Option<glow::Texture>, u32, Option<glow::Sampler>) {
        let gl = &self.gl;
        let active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        gl.active_texture(glow::TEXTURE0 + UNIT);
        #[cfg(not(target_arch = "wasm32"))]
        let old_texture = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
        // Unit 3 is reserved for this renderer and left unbound between passes.
        // glow 0.16 panics when querying a null WebGL object binding.
        #[cfg(target_arch = "wasm32")]
        let old_texture = None;
        #[cfg(not(target_arch = "wasm32"))]
        let old_sampler = gl.get_parameter_sampler(glow::SAMPLER_BINDING);
        #[cfg(target_arch = "wasm32")]
        let old_sampler = None;
        gl.bind_sampler(UNIT, None);
        let mut viewport = [0; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        let scissor = gl.is_enabled(glow::SCISSOR_TEST);
        let depth = gl.is_enabled(glow::DEPTH_TEST);
        let cull = gl.is_enabled(glow::CULL_FACE);
        let depth_mask = gl.get_parameter_bool(glow::DEPTH_WRITEMASK);
        let depth_func = gl.get_parameter_i32(glow::DEPTH_FUNC) as u32;
        let depth_clear = gl.get_parameter_f32(glow::DEPTH_CLEAR_VALUE);
        let color_mask = gl.get_parameter_bool_array::<4>(glow::COLOR_WRITEMASK);
        #[cfg(not(target_arch = "wasm32"))]
        let read = gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING);
        #[cfg(target_arch = "wasm32")]
        let read = target;
        // At most three cached resolutions; switching drone/main cameras does not
        // allocate every frame. One allocation per size, not per frame. An unsupported FBO disables the
        // feature until renderer recreation rather than retrying allocation forever.
        let size = match size {
            512 | 1024 | 2048 => size,
            _ => 0,
        };
        if !self.shadow_failed && size > 0 && !self.sun_shadows.iter().any(|s| s.size == size) {
            match SunShadows::create(gl, size) {
                Ok(s) => self.sun_shadows.push(s),
                Err(e) => {
                    eprintln!("Sun shadows disabled: {e}");
                    self.shadow_failed = true;
                }
            }
        }
        let matrix = light_matrix(frame.eye, size);
        if size > 0 {
            if let Some(shadow) = self.sun_shadows.iter().find(|s| s.size == size) {
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(shadow.framebuffer));
                gl.viewport(0, 0, shadow.size, shadow.size);
                gl.disable(glow::SCISSOR_TEST);
                gl.disable(glow::CULL_FACE);
                gl.enable(glow::DEPTH_TEST);
                gl.depth_func(glow::LESS);
                gl.depth_mask(true);
                gl.color_mask(false, false, false, false);
                gl.clear_depth_f32(1.0);
                gl.clear(glow::DEPTH_BUFFER_BIT);
                gl.use_program(Some(shadow.program));
                gl.uniform_matrix_4_f32_slice(
                    shadow.matrix.as_ref(),
                    false,
                    &matrix.to_cols_array(),
                );
                let mut casters = Vec::new();
                // The same ship and asteroid meshes/transforms as the visible pass,
                // including the own ship in cockpit view: hulls really block sunlight.
                for p in &frame.scene.pilots {
                    if frame.space.position(p, frame.now).distance(frame.eye) > DEPTH + RADIUS {
                        continue;
                    }
                    let ship = transform(
                        frame.space.position(p, frame.now),
                        Vec3::ONE,
                        ship_orientation_at(p, frame.now),
                    );
                    if p.name == frame.own && p.ship_class == ShipClass::Pioneer {
                        if let Some(parts) = frame.canonical_pioneer_parts() {
                            let mount = frame.pioneer_body_mount();
                            for &(mesh, model, _, _) in parts.iter() {
                                if docked_source_primitive(mesh) != Some(6) {
                                    push_caster(&mut casters, (mesh, mount * model), frame.eye);
                                }
                            }
                        }
                    } else {
                        for &(mesh, model, _, _) in cached_ship_parts(p.ship_class) {
                            push_caster(&mut casters, (mesh, ship * model), frame.eye);
                        }
                    }
                    for id in 0..2 {
                        if let Some(t) = self.turrets.get(&(p.name.clone(), id)) {
                            for (mesh, model, _, _) in turret_parts(p.ship_class, t.rotation, id) {
                                push_caster(&mut casters, (mesh, ship * model), frame.eye);
                            }
                        }
                    }
                }
                for a in &frame.scene.asteroids {
                    if frame.space.scene(a.position).distance(frame.eye) > DEPTH + RADIUS {
                        continue;
                    }
                    push_caster(
                        &mut casters,
                        (3, asteroid_model(a, frame.space, frame.now)),
                        frame.eye,
                    );
                }
                // Station batches already combine parts into a small fixed draw list.
                if frame
                    .space
                    .scene(STATION_CENTER_POSITION)
                    .distance(frame.eye)
                    < 320.0
                {
                    for &(mesh, _, _) in &self.station_batches {
                        push_caster(
                            &mut casters,
                            (
                                mesh,
                                Mat4::from_translation(frame.space.scene(STATION_CENTER_POSITION)),
                            ),
                            frame.eye,
                        );
                    }
                }
                casters.retain(|(_, m)| m.w_axis.truncate().distance(frame.eye) < DEPTH + RADIUS);
                casters.sort_by(|a, b| {
                    a.1.w_axis
                        .truncate()
                        .distance_squared(frame.eye)
                        .total_cmp(&b.1.w_axis.truncate().distance_squared(frame.eye))
                });
                for (mesh, model) in casters.into_iter().take(MAX_CASTERS) {
                    gl.uniform_matrix_4_f32_slice(
                        shadow.model.as_ref(),
                        false,
                        &model.to_cols_array(),
                    );
                    gl.bind_vertex_array(Some(self.meshes[mesh_index(mesh)].vao));
                    gl.draw_arrays(glow::TRIANGLES, 0, self.meshes[mesh_index(mesh)].count);
                }
            }
        }
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, target);
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, read);
        gl.viewport(viewport[0], viewport[1], viewport[2], viewport[3]);
        for (cap, enabled) in [
            (glow::SCISSOR_TEST, scissor),
            (glow::DEPTH_TEST, depth),
            (glow::CULL_FACE, cull),
        ] {
            if enabled {
                gl.enable(cap)
            } else {
                gl.disable(cap)
            }
        }
        gl.depth_mask(depth_mask);
        gl.depth_func(depth_func);
        gl.clear_depth_f32(depth_clear);
        gl.color_mask(color_mask[0], color_mask[1], color_mask[2], color_mask[3]);
        gl.use_program(Some(self.program));
        let shadow = self.sun_shadows.iter().find(|s| s.size == size);
        let enabled = shadow.is_some();
        gl.uniform_1_f32(
            gl.get_uniform_location(self.program, "ark_shadow_enabled")
                .as_ref(),
            if enabled { 1.0 } else { 0.0 },
        );
        gl.uniform_matrix_4_f32_slice(
            gl.get_uniform_location(self.program, "ark_shadow_matrix")
                .as_ref(),
            false,
            &matrix.to_cols_array(),
        );
        gl.uniform_1_f32(
            gl.get_uniform_location(self.program, "ark_shadow_texel")
                .as_ref(),
            1.0 / size.max(1) as f32,
        );
        gl.uniform_1_i32(
            gl.get_uniform_location(self.program, "ark_shadow_map")
                .as_ref(),
            UNIT as i32,
        );
        // Never sample the depth attachment while it is the current draw target.
        gl.bind_texture(
            glow::TEXTURE_2D,
            Some(shadow.map_or(self.shadow_fallback, |s| s.texture)),
        );
        gl.active_texture(active);
        (old_texture, active, old_sampler)
    }
    /// Hangar uses a separate coordinate space and must not reuse a flight shadow map.
    pub(super) unsafe fn disable_sun_shadow(
        &self,
    ) -> (Option<glow::Texture>, u32, Option<glow::Sampler>) {
        let gl = &self.gl;
        let active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
        gl.active_texture(glow::TEXTURE0 + UNIT);
        #[cfg(not(target_arch = "wasm32"))]
        let old = (
            gl.get_parameter_texture(glow::TEXTURE_BINDING_2D),
            active,
            gl.get_parameter_sampler(glow::SAMPLER_BINDING),
        );
        #[cfg(target_arch = "wasm32")]
        let old = (None, active, None);
        gl.bind_sampler(UNIT, None);
        gl.bind_texture(glow::TEXTURE_2D, Some(self.shadow_fallback));
        gl.uniform_1_f32(
            gl.get_uniform_location(self.program, "ark_shadow_enabled")
                .as_ref(),
            0.0,
        );
        gl.uniform_1_i32(
            gl.get_uniform_location(self.program, "ark_shadow_map")
                .as_ref(),
            UNIT as i32,
        );
        gl.active_texture(active);
        old
    }
    pub(super) unsafe fn restore_shadow_texture(
        &self,
        old: (Option<glow::Texture>, u32, Option<glow::Sampler>),
    ) {
        self.gl.active_texture(glow::TEXTURE0 + UNIT);
        self.gl.bind_texture(glow::TEXTURE_2D, old.0);
        self.gl.bind_sampler(UNIT, old.2);
        self.gl.active_texture(old.1);
    }
}

/// Complete white texture keeps the shader valid when shadows are disabled or unsupported.
pub(super) unsafe fn fallback(gl: &glow::Context) -> Result<glow::Texture, String> {
    let active = gl.get_parameter_i32(glow::ACTIVE_TEXTURE) as u32;
    gl.active_texture(glow::TEXTURE0 + UNIT);
    #[cfg(not(target_arch = "wasm32"))]
    let old = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
    #[cfg(target_arch = "wasm32")]
    let old = None;
    let t = gl.create_texture()?;
    gl.bind_texture(glow::TEXTURE_2D, Some(t));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::RGBA8 as i32,
        1,
        1,
        0,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelUnpackData::Slice(Some(&[255, 255, 255, 255])),
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::NEAREST as i32,
    );
    gl.bind_texture(glow::TEXTURE_2D, old);
    gl.active_texture(active);
    Ok(t)
}

#[cfg(all(test, target_os = "macos"))]
pub(super) unsafe fn assert_occluder_depth(
    renderer: &mut Renderer,
    frame: &Frame,
    target: glow::Framebuffer,
) {
    let mut sample = frame.clone();
    std::sync::Arc::make_mut(&mut sample.scene).asteroids = vec![Asteroid {
        id: 998,
        system_id: frame.system_id,
        name: "shadow fixture".into(),
        position: sample.space.world(sample.eye),
        radius: 100.0,
        ore: 1,
    }];
    let old = renderer.paint_sun_shadow(&sample, Some(target), 512);
    let gl = &renderer.gl;
    let shadow = renderer.sun_shadows.iter().find(|s| s.size == 512).unwrap();
    gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(shadow.framebuffer));
    let mut bytes = [0u8; 4];
    gl.read_pixels(
        256,
        256,
        1,
        1,
        glow::DEPTH_COMPONENT,
        glow::FLOAT,
        glow::PixelPackData::Slice(Some(&mut bytes)),
    );
    let depth = f32::from_ne_bytes(bytes);
    // The actual asteroid mesh writes a depth in front of the receiver at zero.
    let receiver = (light_matrix(sample.eye, 512) * sample.eye.extend(1.0)).z * 0.5 + 0.5;
    assert!(
        depth > 0.0 && depth < receiver,
        "caster depth {depth} did not occlude receiver {receiver}"
    );
    gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(target));
    renderer.restore_shadow_texture(old);
}

/// GPU regression: a sloped receiver must not shadow itself under the nine PCF
/// samples; a genuinely nearer blocker must still darken that same receiver.
#[cfg(all(test, target_os = "macos"))]
pub(super) unsafe fn test_sloped_pcf(
    gl: &glow::Context,
    renderer: &Renderer,
    target: glow::Framebuffer,
) {
    let size = 512usize;
    let mut depth: Vec<u8> = (0..size * size)
        .flat_map(|i| {
            let u = (i % size) as f32 / size as f32 + 0.5 / size as f32;
            let z = 0.5 + 0.3 * (u - 0.5);
            ((z as f64 * u32::MAX as f64) as u32).to_ne_bytes()
        })
        .collect();
    let texture = gl.create_texture().unwrap();
    gl.active_texture(glow::TEXTURE0 + UNIT);
    gl.bind_texture(glow::TEXTURE_2D, Some(texture));
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::DEPTH_COMPONENT24 as i32,
        size as i32,
        size as i32,
        0,
        glow::DEPTH_COMPONENT,
        glow::UNSIGNED_INT,
        glow::PixelUnpackData::Slice(Some(&depth)),
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MIN_FILTER,
        glow::NEAREST as i32,
    );
    gl.tex_parameter_i32(
        glow::TEXTURE_2D,
        glow::TEXTURE_MAG_FILTER,
        glow::NEAREST as i32,
    );
    let fragment=format!("#version 150\n{}\nin vec2 uv;out vec4 color;void main(){{vec2 q=mix(vec2(0.2),vec2(0.8),uv);float z=0.5+0.3*(q.x-0.5);float v=ark_sun_visibility(vec3(q*2.0-1.0,z*2.0-1.0),normalize(vec3(-0.3,0,1)),vec3(0,0,1));color=vec4(v,v,v,1);}}",GLSL);
    let program = compile(gl, BACK_VERTEX, &fragment).unwrap();
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target));
    gl.viewport(0, 0, 128, 128);
    gl.disable(glow::DEPTH_TEST);
    gl.disable(glow::SCISSOR_TEST);
    gl.disable(glow::BLEND);
    gl.color_mask(true, true, true, true);
    gl.use_program(Some(program));
    gl.bind_vertex_array(Some(renderer.empty));
    gl.uniform_1_f32(
        gl.get_uniform_location(program, "ark_shadow_enabled")
            .as_ref(),
        1.0,
    );
    gl.uniform_1_f32(
        gl.get_uniform_location(program, "ark_shadow_texel")
            .as_ref(),
        1.0 / size as f32,
    );
    gl.uniform_1_i32(
        gl.get_uniform_location(program, "ark_shadow_map").as_ref(),
        UNIT as i32,
    );
    gl.uniform_matrix_4_f32_slice(
        gl.get_uniform_location(program, "ark_shadow_matrix")
            .as_ref(),
        false,
        &Mat4::IDENTITY.to_cols_array(),
    );
    gl.draw_arrays(glow::TRIANGLES, 0, 3);
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
    assert!(
        pixels.chunks_exact(4).all(|p| p[0] > 250),
        "sloped receiver produced shadow acne"
    );
    for y in 200..312 {
        for x in 200..312 {
            let i = (y * size + x) * 4;
            let receiver = u32::from_ne_bytes(depth[i..i + 4].try_into().unwrap());
            // A nearby blocker only two shadow texel footprints ahead must
            // survive the bias; this is not a distant black-box occluder.
            let separation = (0.075 * 2.0 / size as f64 * u32::MAX as f64) as u32;
            depth[i..i + 4].copy_from_slice(&(receiver - separation).to_ne_bytes());
        }
    }
    gl.tex_image_2d(
        glow::TEXTURE_2D,
        0,
        glow::DEPTH_COMPONENT24 as i32,
        size as i32,
        size as i32,
        0,
        glow::DEPTH_COMPONENT,
        glow::UNSIGNED_INT,
        glow::PixelUnpackData::Slice(Some(&depth)),
    );
    gl.draw_arrays(glow::TRIANGLES, 0, 3);
    let mut center = [0u8; 4];
    gl.read_pixels(
        64,
        64,
        1,
        1,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut center)),
    );
    assert!(center[0] < 5, "real blocker failed to cast shadow");
    gl.delete_program(program);
    gl.delete_texture(texture);
    gl.bind_texture(glow::TEXTURE_2D, None);
    gl.active_texture(glow::TEXTURE0);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn caster_budget_retains_nearest_geometry() {
        let mut casters = Vec::new();
        for x in (0..1024).rev() {
            push_caster(
                &mut casters,
                (0, Mat4::from_translation(Vec3::X * x as f32)),
                Vec3::ZERO,
            );
        }
        assert_eq!(casters.len(), MAX_CASTERS);
        assert!(casters.iter().all(|(_, m)| m.w_axis.x < MAX_CASTERS as f32));
    }
    #[test]
    fn receiver_and_sun_caster_are_in_depth_volume() {
        let eye = Vec3::new(14.0, -3.0, 91.0);
        let matrix = light_matrix(eye, 1024);
        for p in [eye, eye + Vec3::new(-0.5, 0.8, 0.65).normalize() * 120.0] {
            let q = matrix.project_point3(p);
            assert!(q.abs().max_element() < 1.0);
        }
    }
    #[test]
    fn camera_relative_au_origin_keeps_local_precision() {
        let space = Space {
            origin: [1.5e12, -3.0e12, 8.0e11],
        };
        let p = space.scene([1.5e12 + 72.0, -3.0e12, 8.0e11]);
        assert!((p.x - 0.72).abs() < 1e-5);
        assert!(light_matrix(p, 2048).is_finite());
    }
    #[test]
    fn projection_is_finite_for_disabled_size() {
        assert!(light_matrix(Vec3::ZERO, 0).is_finite());
    }
}
