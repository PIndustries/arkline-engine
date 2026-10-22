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
    generated_normals: bool,
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
        Self::decode_with_mode(bytes, false)
    }

    /// Preview-only repair for absent NORMAL attributes. Original bytes remain
    /// unchanged; authoring import, acceptance and saving must use strict data.
    pub fn decode_preview(bytes: &[u8]) -> Result<Self, String> {
        Self::decode_with_mode(bytes, true)
    }

    pub fn uses_generated_normals(&self) -> bool {
        self.generated_normals
    }

    fn decode_with_mode(bytes: &[u8], preview: bool) -> Result<Self, String> {
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
        let (parts, generated_normals) = if preview {
            ship_asset::decode_preview(bytes)?
        } else {
            (ship_asset::decode(bytes)?, false)
        };
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
        if generated_normals {
            notes.push("Preview only: missing NORMAL attributes were replaced with generated flat face normals. Original asset bytes are unchanged; this geometry cannot be staged, accepted or saved.".into());
        }
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
            generated_normals,
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
    /// Reuse the imported meter-space GPU data with a shared display-space camera.
    /// `display` is a positive uniform scale plus translation; neither geometry nor
    /// normals change, so lighting still uses the original scalar materials.
    pub(super) fn shared_callback(
        &self,
        rect: egui::Rect,
        vp: Mat4,
        eye: Vec3,
        display: Mat4,
    ) -> Result<egui::PaintCallback, String> {
        let (vp, eye) = Self::shared_camera(vp, eye, display)?;
        let renderer = self
            .renderer
            .as_ref()
            .ok_or_else(|| {
                self.error
                    .clone()
                    .unwrap_or_else(|| "GPU unavailable".into())
            })?
            .clone();
        drop(
            renderer
                .lock()
                .map_err(|_| "Preview renderer unavailable".to_owned())?,
        );
        Ok(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                if let Ok(renderer) = renderer.lock() {
                    unsafe {
                        renderer.paint(vp, eye);
                        #[cfg(test)]
                        assert_eq!(
                            renderer.gl.get_error(),
                            glow::NO_ERROR,
                            "Imported preview shared paint callback"
                        );
                    }
                }
            })),
        })
    }
    fn shared_camera(vp: Mat4, eye: Vec3, display: Mat4) -> Result<(Mat4, Vec3), String> {
        if !vp.is_finite() || !eye.is_finite() || !display.is_finite() {
            return Err("Imported preview camera must be finite".into());
        }
        let determinant = display.determinant();
        if !determinant.is_finite() || determinant <= 0.0 {
            return Err(
                "Imported preview display transform must be invertible and positive".into(),
            );
        }
        let inverse = display.inverse();
        if !inverse.is_finite() {
            return Err("Imported preview display inverse must be finite".into());
        }
        let vp = vp * display;
        let eye = inverse.transform_point3(eye);
        if !vp.is_finite() || !eye.is_finite() {
            return Err("Imported preview meter-space camera must be finite".into());
        }
        Ok((vp, eye))
    }
    #[cfg(all(test, target_os = "macos"))]
    pub(super) fn debug_mesh_bindings(&self) -> Vec<(glow::VertexArray, glow::Buffer, i32)> {
        self.renderer
            .as_ref()
            .and_then(|renderer| {
                renderer.lock().ok().map(|renderer| {
                    renderer
                        .meshes
                        .iter()
                        .map(|mesh| (mesh.vao, mesh.buffer, mesh.count))
                        .collect()
                })
            })
            .unwrap_or_default()
    }
    #[cfg(all(test, target_os = "macos"))]
    pub(super) fn debug_materials(&self) -> Vec<(Vec3, Vec3, f32, f32)> {
        self.renderer
            .as_ref()
            .and_then(|renderer| {
                renderer.lock().ok().map(|renderer| {
                    renderer
                        .meshes
                        .iter()
                        .map(|mesh| (mesh.color, mesh.emission, mesh.roughness, mesh.metallic))
                        .collect()
                })
            })
            .unwrap_or_default()
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
                            #[cfg(test)]
                            assert_eq!(
                                r.gl.get_error(),
                                glow::NO_ERROR,
                                "Imported preview paint callback"
                            );
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
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn imported_shared_camera_preserves_meter_space_lighting_and_projection() {
        let center = Vec3::new(12., -7., 3.);
        let display = Mat4::from_scale(Vec3::splat(0.25)) * Mat4::from_translation(-center);
        let eye = Vec3::new(3., 2., 5.);
        let vp = Mat4::perspective_rh_gl(45f32.to_radians(), 1.5, 0.01, 100.)
            * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let (meter_vp, meter_eye) = ImportedAssetPreview::shared_camera(vp, eye, display).unwrap();
        let point = center + Vec3::new(1., 2., -1.);
        assert!(
            (meter_vp.project_point3(point) - vp.project_point3(display.transform_point3(point)))
                .length()
                < 1e-5
        );
        assert!((display.transform_point3(meter_eye) - eye).length() < 1e-5);
        let meter_view = (meter_eye - point).normalize();
        let display_view = (eye - display.transform_point3(point)).normalize();
        assert!((meter_view - display_view).length() < 1e-5);
    }
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn imported_shared_camera_rejects_nonfinite_or_singular_transforms() {
        let invalid = Mat4::from_cols_array(&[f32::NAN; 16]);
        for (vp, eye, display) in [
            (invalid, Vec3::ZERO, Mat4::IDENTITY),
            (Mat4::IDENTITY, Vec3::splat(f32::INFINITY), Mat4::IDENTITY),
            (Mat4::IDENTITY, Vec3::ZERO, invalid),
            (Mat4::IDENTITY, Vec3::ZERO, Mat4::ZERO),
            (
                Mat4::IDENTITY,
                Vec3::ZERO,
                Mat4::from_scale(Vec3::splat(-1.)),
            ),
            (
                Mat4::from_scale(Vec3::splat(f32::MAX)),
                Vec3::ZERO,
                Mat4::from_scale(Vec3::splat(2.)),
            ),
            (
                Mat4::IDENTITY,
                -Vec3::splat(f32::MAX),
                Mat4::from_translation(Vec3::splat(f32::MAX * 0.5)),
            ),
            (
                Mat4::IDENTITY,
                Vec3::ZERO,
                Mat4::from_scale(Vec3::new(1e-39, 1., 1.)),
            ),
        ] {
            assert!(ImportedAssetPreview::shared_camera(vp, eye, display).is_err());
        }
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

    #[test]
    fn preview_missing_normals_reports_measured_calibration_and_preserves_original_asset() {
        use sha2::{Digest, Sha256};
        let bytes = include_bytes!("../../../assets/examples/calibration-cube.glb").to_vec();
        let original = bytes.clone();
        let hash = Sha256::digest(&bytes);
        assert_eq!(
            ImportedGeometry::decode(&bytes).err().unwrap(),
            "Missing ship normals"
        );
        let geometry = ImportedGeometry::decode_preview(&bytes).unwrap();
        assert!(geometry.uses_generated_normals());
        assert_eq!(geometry.report.version, 1);
        assert_eq!(geometry.report.triangles, 12);
        assert_eq!(geometry.report.primitives, 1);
        assert_eq!(geometry.report.min_m, [0.0; 3]);
        assert_eq!(geometry.report.max_m, [1.0; 3]);
        assert!(geometry
            .report
            .notes
            .iter()
            .any(|note| note.contains("Preview only")
                && note.contains("generated flat face normals")));
        assert!(geometry
            .parts
            .iter()
            .flat_map(|part| &part.vertices)
            .all(|vertex| vertex.n.is_finite() && (vertex.n.length() - 1.0).abs() < 1e-5));
        assert_eq!(bytes, original);
        assert_eq!(Sha256::digest(&bytes), hash);
    }

    #[test]
    fn preview_with_existing_normals_retains_strict_geometry_and_has_no_repair_flag() {
        let strict = ImportedGeometry::decode(A).unwrap();
        let preview = ImportedGeometry::decode_preview(A).unwrap();
        assert!(!strict.uses_generated_normals() && !preview.uses_generated_normals());
        assert_eq!(strict.report, preview.report);
        for (expected, actual) in strict.parts.iter().zip(&preview.parts) {
            assert_eq!(expected.material_index, actual.material_index);
            assert_eq!(expected.color, actual.color);
            assert_eq!(expected.emission, actual.emission);
            for (expected, actual) in expected.vertices.iter().zip(&actual.vertices) {
                assert_eq!(expected.p, actual.p);
                assert_eq!(expected.n, actual.n);
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "pioneer_mesh_proof_gpu_tests.rs"]
mod mesh_proof_tests;
