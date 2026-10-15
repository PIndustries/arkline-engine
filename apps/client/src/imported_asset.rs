//! Project asset preview. Own geometry/material bindings; no world state or commands.
use super::*;
#[cfg(not(target_arch = "wasm32"))]
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(target_arch = "wasm32"), derive(Serialize, Deserialize))]
#[cfg_attr(not(target_arch = "wasm32"), serde(deny_unknown_fields))]
pub struct ImportReport {
    pub version: u32,
    pub min_m: [f32; 3],
    pub max_m: [f32; 3],
    pub triangles: usize,
    pub primitives: usize,
    pub materials: Vec<String>,
    pub notes: Vec<String>,
}
pub struct ImportedGeometry {
    parts: Vec<ship_asset::Primitive>,
    pub report: ImportReport,
}
impl ImportedGeometry {
    /// Expanded transformed triangle corners, preserving the imported body-meter frame.
    pub fn triangle_positions_m(&self) -> impl Iterator<Item = [f32; 3]> + '_ {
        self.parts
            .iter()
            .flat_map(|p| p.vertices.iter().map(|v| (v.p * 100.).to_array()))
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("GLB exceeds 16 MiB".into());
        }
        let doc = gltf::Gltf::from_slice(bytes).map_err(|e| e.to_string())?;
        if doc.materials().len() > 512
            || doc
                .materials()
                .any(|m| m.name().is_some_and(|n| n.len() > 1024))
        {
            return Err("Project material/name budget exceeded".into());
        }
        // Bound names before the shared decoder copies them into draw batches.
        // All existing embedded-asset geometry/feature limits remain enforced.
        let parts = ship_asset::decode(bytes)?;
        if doc.cameras().len() != 0 {
            return Err("Cameras/lights are unsupported in project geometry".into());
        }
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut triangles = 0;
        for p in &parts {
            triangles += p.vertices.len() / 3;
            for v in &p.vertices {
                let meters = v.p * 100.;
                if !meters.is_finite() || meters.abs().max_element() > 10_000_000. {
                    return Err("Project geometry exceeds finite meter-coordinate bounds".into());
                }
                min = min.min(meters);
                max = max.max(meters);
            }
        }
        let materials = parts.iter().map(|p| p.material_name.clone()).collect();
        let mut notes = vec![
            "Opaque static indexed triangles; scalar materials only".into(),
            "Node transforms evaluated; node names are not durable source-part IDs".into(),
            "Only the default scene is previewed; other scenes and unreferenced geometry are ignored".into(),
            "No textures, animation, skins, morphs, external resources or physical properties"
                .into(),
        ];
        if doc.meshes().flat_map(|m| m.primitives()).any(|p| {
            p.attributes()
                .any(|(s, _)| !matches!(s, gltf::Semantic::Positions | gltf::Semantic::Normals))
        }) {
            notes.push("Extra vertex attributes (UV/color/tangent/joint/weight data) are ignored; only POSITION/NORMAL are rendered".into());
        }
        if (max - min).max_element() < 0.001 {
            return Err("Project geometry span must be at least 1 mm".into());
        }
        for p in &parts {
            if !p.color.is_finite()
                || !p.emissive_color.is_finite()
                || !p.emission.is_finite()
                || !p.roughness.is_finite()
                || !p.metallic.is_finite()
            {
                return Err("Non-finite imported material".into());
            }
        }
        if doc.nodes().len() > parts.len() {
            notes.push("Default-scene transforms are flattened; inventory nodes are not editable source parts".into());
        }
        Ok(Self {
            report: ImportReport {
                version: 1,
                min_m: min.to_array(),
                max_m: max.to_array(),
                triangles,
                primitives: parts.len(),
                materials,
                notes,
            },
            parts,
        })
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct ImportedAssetPreview {
    renderer: Option<Arc<Mutex<PreviewGpu>>>,
    error: Option<String>,
    yaw: f32,
    pitch: f32,
    zoom: f32,
}
#[cfg(not(target_arch = "wasm32"))]
impl ImportedAssetPreview {
    pub fn new(gl: Option<&Arc<glow::Context>>) -> Self {
        let result = gl.map(|gl| unsafe { PreviewGpu::new(gl.clone()) });
        let (renderer, error) = match result {
            Some(Ok(r)) => (Some(Arc::new(Mutex::new(r))), None),
            Some(Err(e)) => (None, Some(e)),
            None => (None, Some("GPU context unavailable".into())),
        };
        Self {
            renderer,
            error,
            yaw: 1.1,
            pitch: 0.4,
            zoom: 1.,
        }
    }
    pub fn clear(&mut self) {
        if let Some(r) = &self.renderer {
            if let Ok(mut r) = r.lock() {
                unsafe {
                    r.clear();
                }
            }
        }
    }
    pub fn set(&mut self, geometry: &ImportedGeometry) -> Result<(), String> {
        if let Some(r) = &self.renderer {
            unsafe {
                r.lock()
                    .map_err(|_| "Preview renderer unavailable")?
                    .replace(geometry)
            }
        } else {
            Err(self
                .error
                .clone()
                .unwrap_or_else(|| "GPU unavailable".into()))
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, report: Option<&ImportReport>) {
        let Some(report) = report else {
            ui.label("No verified geometry to display.");
            return;
        };
        ui.label("Body meters · +X forward · +Y up · +Z starboard · original origin retained");
        let min = Vec3::from_array(report.min_m);
        let max = Vec3::from_array(report.max_m);
        let span = (max - min).max_element().max(0.001);
        let center = (min + max) * 0.5;
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size().max(egui::vec2(1., 1.)),
            egui::Sense::drag(),
        );
        if response.dragged() {
            let d = ui.input(|i| i.pointer.delta());
            self.yaw -= d.x * 0.008;
            self.pitch = (self.pitch + d.y * 0.006).clamp(-1.3, 1.3);
        }
        if response.hovered() {
            self.zoom =
                (self.zoom * (-ui.input(|i| i.smooth_scroll_delta.y) * 0.002).exp()).clamp(0.6, 8.);
        }
        let radius = span * 1.8 * self.zoom;
        let eye = center
            + Vec3::new(
                self.yaw.cos() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.sin() * self.pitch.cos(),
            ) * radius;
        let vp = Mat4::perspective_rh_gl(
            45f32.to_radians(),
            (rect.width() / rect.height()).max(0.01),
            span * 0.001,
            span * 100.,
        ) * Mat4::look_at_rh(eye, center, Vec3::Y);
        let painter = ui.painter().with_clip_rect(rect);
        if let Some(r) = &self.renderer {
            let r = r.clone();
            painter.add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                    if let Ok(r) = r.lock() {
                        unsafe {
                            r.paint(vp, eye);
                        }
                    }
                })),
            });
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                self.error.as_deref().unwrap_or("GPU unavailable"),
                egui::FontId::proportional(14.),
                egui::Color32::LIGHT_RED,
            );
        }
        // Project a real body-space distance using the exact same camera matrix.
        let project = |p: Vec3| {
            let p = vp.project_point3(p);
            egui::pos2(
                rect.center().x + p.x * rect.width() * 0.5,
                rect.center().y - p.y * rect.height() * 0.5,
            )
        };
        let ruler = 10f32.powf((span / 4.).log10().floor());
        let start = Vec3::new(min.x, min.y - span * 0.1, min.z);
        let a = project(start);
        let b = project(start + Vec3::X * ruler);
        painter.line_segment([a, b], egui::Stroke::new(2., egui::Color32::WHITE));
        painter.text(
            a,
            egui::Align2::LEFT_TOP,
            format!("{ruler} m · X"),
            egui::FontId::monospace(12.),
            egui::Color32::WHITE,
        );
        painter.text(
            rect.left_top() + egui::vec2(10., 10.),
            egui::Align2::LEFT_TOP,
            "Drag: orbit · wheel: zoom · camera framing only",
            egui::FontId::proportional(12.),
            egui::Color32::LIGHT_GRAY,
        );
    }
    pub fn destroy(&mut self) {
        if let Some(r) = self.renderer.take() {
            if let Ok(mut r) = r.lock() {
                unsafe {
                    r.clear();
                    r.gl.delete_program(r.program);
                }
            }
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
struct PreviewMesh {
    vao: glow::VertexArray,
    buffer: glow::Buffer,
    count: i32,
    color: Vec3,
    emission: Vec3,
    roughness: f32,
    metallic: f32,
}
#[cfg(not(target_arch = "wasm32"))]
struct PreviewGpu {
    gl: Arc<glow::Context>,
    program: glow::Program,
    meshes: Vec<PreviewMesh>,
}
#[cfg(not(target_arch = "wasm32"))]
impl PreviewGpu {
    unsafe fn new(gl: Arc<glow::Context>) -> Result<Self, String> {
        let program = compile(
            &gl,
            r#"#version 150
in vec3 position;in vec3 normal;uniform mat4 vp;out vec3 n;out vec3 world;
void main(){world=position;n=normal;gl_Position=vp*vec4(position,1.0);}"#,
            r#"#version 150
in vec3 n;in vec3 world;uniform vec3 base;uniform vec3 emission;uniform vec3 eye;uniform float roughness;uniform float metallic;out vec4 color;
void main(){vec3 normal=normalize(n);if(!gl_FrontFacing)normal=-normal;vec3 l=normalize(vec3(0.4,0.9,0.6));vec3 v=normalize(eye-world);float diffuse=max(dot(normal,l),0.0);float spec=pow(max(dot(normal,normalize(l+v)),0.0),mix(80.0,3.0,roughness));vec3 lit=base*(0.25+0.75*diffuse)*(1.0-0.4*metallic)+mix(vec3(0.04),base,metallic)*spec+emission;color=vec4(pow(lit/(lit+vec3(1.0)),vec3(1.0/2.2)),1.0);}"#,
        )?;
        Ok(Self {
            gl,
            program,
            meshes: vec![],
        })
    }
    unsafe fn clear(&mut self) {
        for m in self.meshes.drain(..) {
            self.gl.delete_buffer(m.buffer);
            self.gl.delete_vertex_array(m.vao);
        }
    }
    unsafe fn replace(&mut self, g: &ImportedGeometry) -> Result<(), String> {
        // Allocate separately. Any failure leaves the previous verified GPU preview intact.
        let gl = &self.gl;
        let mut next = Vec::new();
        for p in &g.parts {
            let vao = match gl.create_vertex_array() {
                Ok(v) => v,
                Err(e) => {
                    for m in next {
                        let m: PreviewMesh = m;
                        gl.delete_buffer(m.buffer);
                        gl.delete_vertex_array(m.vao);
                    }
                    return Err(e);
                }
            };
            let buffer = match gl.create_buffer() {
                Ok(v) => v,
                Err(e) => {
                    gl.delete_vertex_array(vao);
                    for m in next {
                        let m: PreviewMesh = m;
                        gl.delete_buffer(m.buffer);
                        gl.delete_vertex_array(m.vao);
                    }
                    return Err(e);
                }
            };
            let mut data = Vec::<f32>::with_capacity(p.vertices.len() * 6);
            for v in &p.vertices {
                data.extend_from_slice(&(v.p * 100.).to_array());
                data.extend_from_slice(&v.n.to_array());
            }
            gl.bind_vertex_array(Some(vao));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(buffer));
            gl.buffer_data_u8_slice(
                glow::ARRAY_BUFFER,
                std::slice::from_raw_parts(data.as_ptr().cast(), data.len() * 4),
                glow::STATIC_DRAW,
            );
            for (i, offset) in [(0, 0), (1, 12)] {
                gl.enable_vertex_attrib_array(i);
                gl.vertex_attrib_pointer_f32(i, 3, glow::FLOAT, false, 24, offset);
            }
            next.push(PreviewMesh {
                vao,
                buffer,
                count: p.vertices.len() as i32,
                color: p.color,
                emission: p.emissive_color * p.emission,
                roughness: p.roughness,
                metallic: p.metallic,
            });
        }
        gl.bind_vertex_array(None);
        gl.bind_buffer(glow::ARRAY_BUFFER, None);
        if gl.get_error() != glow::NO_ERROR {
            for m in next {
                gl.delete_buffer(m.buffer);
                gl.delete_vertex_array(m.vao);
            }
            return Err("GPU rejected imported geometry upload; previous preview retained".into());
        }
        self.clear();
        self.meshes = next;
        Ok(())
    }
    unsafe fn paint(&self, vp: Mat4, eye: Vec3) {
        let gl = &self.gl;
        gl.enable(glow::DEPTH_TEST);
        gl.depth_mask(true);
        gl.disable(glow::CULL_FACE);
        gl.disable(glow::BLEND);
        gl.clear_color(0.025, 0.04, 0.05, 1.);
        gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        gl.use_program(Some(self.program));
        gl.uniform_matrix_4_f32_slice(
            gl.get_uniform_location(self.program, "vp").as_ref(),
            false,
            &vp.to_cols_array(),
        );
        gl.uniform_3_f32_slice(
            gl.get_uniform_location(self.program, "eye").as_ref(),
            &eye.to_array(),
        );
        for m in &self.meshes {
            gl.bind_vertex_array(Some(m.vao));
            gl.uniform_3_f32_slice(
                gl.get_uniform_location(self.program, "base").as_ref(),
                &m.color.to_array(),
            );
            gl.uniform_3_f32_slice(
                gl.get_uniform_location(self.program, "emission").as_ref(),
                &m.emission.to_array(),
            );
            gl.uniform_1_f32(
                gl.get_uniform_location(self.program, "roughness").as_ref(),
                m.roughness,
            );
            gl.uniform_1_f32(
                gl.get_uniform_location(self.program, "metallic").as_ref(),
                m.metallic,
            );
            gl.draw_arrays(glow::TRIANGLES, 0, m.count);
        }
        gl.bind_vertex_array(None);
        gl.use_program(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const A: &[u8] = include_bytes!("../../editor/tests/fixtures/import/wedge.glb");
    const B: &[u8] = include_bytes!("../../editor/tests/fixtures/import/spire.glb");
    #[test]
    fn imported_materials_belong_to_the_imported_batches() {
        let a = ImportedGeometry::decode(A).unwrap();
        let b = ImportedGeometry::decode(B).unwrap();
        assert!(a.parts[0].color.y > a.parts[0].color.x * 5.);
        assert!(b.parts[0].color.x > b.parts[0].color.y * 5.);
        assert_eq!(a.report.materials, ["wedge coating"]);
        assert_eq!(b.report.materials, ["spire coating"]);
        assert_eq!(a.parts[0].vertices.len(), 12);
    }
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "Explicit actual EGL/llvmpipe GPU acceptance; no app window"]
    fn linux_gpu_import_replacement_materials_and_resource_cleanup() {
        use std::ffi::{c_void, CString};
        #[link(name = "dl")]
        unsafe extern "C" {
            fn dlopen(p: *const i8, flags: i32) -> *mut c_void;
            fn dlsym(h: *mut c_void, n: *const i8) -> *mut c_void;
            fn dlclose(h: *mut c_void) -> i32;
        }
        unsafe {
            let lib = dlopen(c"libEGL.so.1".as_ptr(), 2);
            assert!(!lib.is_null());
            macro_rules! sym {
                ($name:literal,$ty:ty) => {
                    std::mem::transmute::<*mut c_void, $ty>(dlsym(
                        lib,
                        CString::new($name).unwrap().as_ptr(),
                    ))
                };
            }
            let get_proc = sym!(
                "eglGetProcAddress",
                unsafe extern "C" fn(*const i8) -> *mut c_void
            );
            let platform: unsafe extern "C" fn(u32, *mut c_void, *const i32) -> *mut c_void =
                std::mem::transmute(get_proc(c"eglGetPlatformDisplayEXT".as_ptr()));
            let display = platform(0x31dd, std::ptr::null_mut(), std::ptr::null());
            assert!(!display.is_null());
            let initialize = sym!(
                "eglInitialize",
                unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> u32
            );
            assert_ne!(
                initialize(display, std::ptr::null_mut(), std::ptr::null_mut()),
                0
            );
            let bind = sym!("eglBindAPI", unsafe extern "C" fn(u32) -> u32);
            assert_ne!(bind(0x30a2), 0);
            let choose = sym!(
                "eglChooseConfig",
                unsafe extern "C" fn(
                    *mut c_void,
                    *const i32,
                    *mut *mut c_void,
                    i32,
                    *mut i32,
                ) -> u32
            );
            let mut config = std::ptr::null_mut();
            let mut count = 0;
            let attrs = [
                0x3033, 1, 0x3040, 8, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3025, 24, 0x3038,
            ];
            assert_ne!(
                choose(display, attrs.as_ptr(), &mut config, 1, &mut count),
                0
            );
            assert!(count > 0);
            let create_surface = sym!(
                "eglCreatePbufferSurface",
                unsafe extern "C" fn(*mut c_void, *mut c_void, *const i32) -> *mut c_void
            );
            let surface =
                create_surface(display, config, [0x3057, 320, 0x3056, 240, 0x3038].as_ptr());
            assert!(!surface.is_null());
            let create = sym!(
                "eglCreateContext",
                unsafe extern "C" fn(
                    *mut c_void,
                    *mut c_void,
                    *mut c_void,
                    *const i32,
                ) -> *mut c_void
            );
            let context = create(display, config, std::ptr::null_mut(), [0x3038].as_ptr());
            assert!(!context.is_null());
            let current = sym!(
                "eglMakeCurrent",
                unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u32
            );
            assert_ne!(current(display, surface, surface, context), 0);
            let gl = Arc::new(glow::Context::from_loader_function(|name| {
                get_proc(CString::new(name).unwrap().as_ptr())
            }));
            let mut renderer = PreviewGpu::new(gl.clone()).unwrap();
            gl.viewport(0, 0, 320, 240);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            let draw = |r: &PreviewGpu, g: &ImportedGeometry| {
                let min = Vec3::from_array(g.report.min_m);
                let max = Vec3::from_array(g.report.max_m);
                let c = (min + max) * 0.5;
                let span = (max - min).max_element();
                let eye = c + Vec3::new(1., 0.7, 1.5) * span;
                let vp = Mat4::perspective_rh_gl(
                    45f32.to_radians(),
                    320. / 240.,
                    span * 0.001,
                    span * 100.,
                ) * Mat4::look_at_rh(eye, c, Vec3::Y);
                r.paint(vp, eye);
                let mut pixels = vec![0; 320 * 240 * 4];
                gl.read_pixels(
                    0,
                    0,
                    320,
                    240,
                    glow::RGBA,
                    glow::UNSIGNED_BYTE,
                    glow::PixelPackData::Slice(Some(&mut pixels)),
                );
                assert_eq!(gl.get_error(), glow::NO_ERROR);
                pixels
            };
            let a = ImportedGeometry::decode(A).unwrap();
            let b = ImportedGeometry::decode(B).unwrap();
            let pioneer = ImportedGeometry::decode(include_bytes!(
                "../../../assets/ships/pioneer/pioneer.glb"
            ))
            .unwrap();
            renderer.replace(&a).unwrap();
            let first = draw(&renderer, &a);
            let old = renderer.meshes[0].buffer;
            assert!(
                first
                    .chunks_exact(4)
                    .filter(|p| p[1] > p[0].saturating_add(40))
                    .count()
                    > 100
            );
            renderer.replace(&b).unwrap();
            assert!(!gl.is_buffer(old));
            let second = draw(&renderer, &b);
            assert!(
                second
                    .chunks_exact(4)
                    .filter(|p| p[0] > p[1].saturating_add(40))
                    .count()
                    > 100
            );
            assert_ne!(first, second);
            for _ in 0..8 {
                let old = renderer.meshes[0].buffer;
                renderer.replace(&a).unwrap();
                assert!(!gl.is_buffer(old));
                let old = renderer.meshes[0].vao;
                renderer.replace(&b).unwrap();
                let is_vao: unsafe extern "C" fn(u32) -> u8 =
                    std::mem::transmute(get_proc(c"glIsVertexArray".as_ptr()));
                assert_eq!(is_vao(old.0.get()), 0);
                assert_eq!(renderer.meshes.len(), 1);
            }
            renderer.replace(&pioneer).unwrap();
            assert_eq!(renderer.meshes.len(), pioneer.report.primitives);
            let hull = draw(&renderer, &pioneer);
            assert_ne!(hull, first);
            assert_ne!(hull, second);
            if let Ok(dir) = std::env::var("ARKLINE_IMPORT_GPU_EVIDENCE") {
                std::fs::create_dir_all(&dir).unwrap();
                for (name, pixels) in [("wedge", first), ("spire", second), ("pioneer", hull)] {
                    let mut ppm = b"P6\n320 240\n255\n".to_vec();
                    for row in pixels.chunks_exact(320 * 4).rev() {
                        for p in row.chunks_exact(4) {
                            ppm.extend_from_slice(&p[..3]);
                        }
                    }
                    std::fs::write(std::path::Path::new(&dir).join(format!("{name}.ppm")), ppm)
                        .unwrap();
                }
            }
            let handles = renderer.meshes.iter().map(|m| m.buffer).collect::<Vec<_>>();
            renderer.clear();
            for h in handles {
                assert!(!gl.is_buffer(h));
            }
            gl.delete_program(renderer.program);
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            drop(renderer);
            drop(gl);
            current(
                display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            sym!(
                "eglDestroyContext",
                unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32
            )(display, context);
            sym!(
                "eglDestroySurface",
                unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32
            )(display, surface);
            sym!("eglTerminate", unsafe extern "C" fn(*mut c_void) -> u32)(display);
            dlclose(lib);
        }
    }
}
