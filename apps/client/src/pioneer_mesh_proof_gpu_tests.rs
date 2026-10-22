use super::*;
use crate::flight3d::cockpit_displays::CockpitDisplayRenderer;
use crate::flight3d::cockpit_maps::CockpitSurface;
use crate::flight3d::cockpit_occlusion::{CockpitOcclusion, DepthDraw, DepthMeshCatalog};
#[cfg(target_os = "linux")]
use crate::flight3d::exterior_gpu_tests::Egl as PrivateContext;
use sha2::{Digest, Sha256};
#[cfg(target_os = "macos")]
use std::ffi::{c_void, CString};
use std::path::PathBuf;

#[cfg(target_os = "macos")]
#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(a: *const u32, f: *mut *mut c_void, n: *mut i32) -> i32;
    fn CGLCreateContext(f: *mut c_void, s: *mut c_void, c: *mut *mut c_void) -> i32;
    fn CGLGetCurrentContext() -> *mut c_void;
    fn CGLSetCurrentContext(c: *mut c_void) -> i32;
    fn CGLDestroyContext(c: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(f: *mut c_void) -> i32;
    fn dlsym(handle: *mut c_void, name: *const std::ffi::c_char) -> *mut c_void;
}

#[cfg(target_os = "macos")]
struct PrivateContext {
    current: *mut c_void,
    previous: *mut c_void,
}
#[cfg(target_os = "macos")]
impl PrivateContext {
    unsafe fn create() -> (Self, Arc<glow::Context>) {
        let previous = CGLGetCurrentContext();
        let mut format = std::ptr::null_mut();
        let mut count = 0;
        assert_eq!(
            CGLChoosePixelFormat(
                [99, 0x3200, 73, 8, 24, 12, 24, 0].as_ptr(),
                &mut format,
                &mut count
            ),
            0
        );
        assert!(!format.is_null() && count > 0);
        let mut current = std::ptr::null_mut();
        let result = CGLCreateContext(format, std::ptr::null_mut(), &mut current);
        assert_eq!(CGLDestroyPixelFormat(format), 0);
        assert_eq!(result, 0);
        assert!(!current.is_null());
        let guard = Self { current, previous };
        assert_eq!(CGLSetCurrentContext(current), 0);
        let gl = Arc::new(glow::Context::from_loader_function(|name| {
            dlsym(
                (-2_isize) as *mut c_void,
                CString::new(name).unwrap().as_ptr(),
            )
            .cast()
        }));
        (guard, gl)
    }
}
#[cfg(target_os = "macos")]
impl Drop for PrivateContext {
    fn drop(&mut self) {
        unsafe {
            let _ = CGLSetCurrentContext(self.previous);
            let _ = CGLDestroyContext(self.current);
        }
    }
}

const WIDTH: usize = 1500;
const HEIGHT: usize = 900;
const LABEL_SIZE: f32 = 28.;
const VALUE_SIZE: f32 = 50.;
const LABEL_POS: [[f32; 2]; 3] = [[20., 8.], [20., 120.], [245., 120.]];
const VALUE_POS: [[f32; 2]; 3] = [[20., 40.], [20., 154.], [245., 154.]];
fn projected_fonts(surface: &CockpitSurface) -> (f32, f32) {
    let mut labels = f32::INFINITY;
    let mut values = f32::INFINITY;
    for (points, size, result) in [
        (&LABEL_POS, LABEL_SIZE, &mut labels),
        (&VALUE_POS, VALUE_SIZE, &mut values),
    ] {
        for [x, y] in points {
            if let (Some(a), Some(b)) = (
                surface.project(egui::pos2(*x, *y)),
                surface.project(egui::pos2(*x, *y + size)),
            ) {
                *result = (*result).min(a.distance(b));
            } else {
                *result = 0.;
            }
        }
    }
    (labels, values)
}
#[test]
fn proposed_forward_bay_font_budget() {
    // Bind the projection preflight to the checked-in study instead of a stale plane literal.
    // This measures font em size, not visual fidelity or human reading performance.
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../assets/studies/pioneer-forward-bay-proof/manifest.json"
    ))
    .unwrap();
    let instrument = &manifest["instrument"];
    let viewport =
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH as f32, HEIGHT as f32));
    for (name, eye, look) in [
        ("seated", Vec3::new(0., 1.35, 0.), Vec3::new(1.8, 1.10, 0.)),
        (
            "standing",
            Vec3::new(-0.85, 1.65, 0.65),
            Vec3::new(1., 1.0, 0.2),
        ),
    ] {
        let vp =
            Mat4::perspective_rh_gl(60f32.to_radians(), WIDTH as f32 / HEIGHT as f32, 0.02, 100.)
                * Mat4::look_at_rh(eye, look, Vec3::Y);
        let surface = CockpitSurface::registered(
            viewport,
            vp,
            Mat4::IDENTITY,
            vec3(&instrument["center"]),
            vec3(&instrument["half"]),
            3,
            egui::vec2(
                instrument["canvas"][0].as_f64().unwrap() as f32,
                instrument["canvas"][1].as_f64().unwrap() as f32,
            ),
        )
        .unwrap();
        let (labels, values) = projected_fonts(&surface);
        eprintln!("PROPOSED PLANE ONLY {name}: label_em={labels:.2}px value_em={values:.2}px");
        assert!(labels >= 14. && values >= 24.);
    }
}
fn vec3(v: &serde_json::Value) -> Vec3 {
    Vec3::new(
        v[0].as_f64().unwrap() as f32,
        v[1].as_f64().unwrap() as f32,
        v[2].as_f64().unwrap() as f32,
    )
}

/// Explicit asset preview / isolated triangle-picking fixture. No EditorWorld,
/// production registration, desktop window, command transport or live authority.
#[test]
#[ignore = "Explicit original forward-bay asset proof; retained isolated native captures"]
fn native_authored_forward_bay_proof() {
    unsafe {
        let asset =
            PathBuf::from(std::env::var_os("ARKLINE_FORWARD_BAY_GLB").expect("Explicit proof GLB"));
        let manifest_path = PathBuf::from(
            std::env::var_os("ARKLINE_FORWARD_BAY_MANIFEST")
                .expect("Bound plane/component manifest"),
        );
        let telemetry_path = PathBuf::from(
            std::env::var_os("ARKLINE_FORWARD_BAY_TELEMETRY")
                .expect("Recorded confirmed telemetry, never fabricated"),
        );
        let out = PathBuf::from(
            std::env::var_os("ARKLINE_FORWARD_BAY_EVIDENCE")
                .expect("New retained evidence directory"),
        );
        std::fs::create_dir(&out).unwrap();
        let bytes = std::fs::read(&asset).unwrap();
        let manifest_bytes = std::fs::read(&manifest_path).unwrap();
        let telemetry_bytes = std::fs::read(&telemetry_path).unwrap();
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
        let telemetry: serde_json::Value = serde_json::from_slice(&telemetry_bytes).unwrap();
        let geometry = ImportedGeometry::decode(&bytes).unwrap();
        assert!(
            !geometry.uses_generated_normals(),
            "Strict authored normals required"
        );
        assert_eq!(manifest["study_only"].as_bool(), Some(true));
        assert_eq!(
            manifest["sha256"].as_str().unwrap(),
            hex::encode(Sha256::digest(&bytes))
        );
        assert_eq!(
            manifest["byte_length"].as_u64().unwrap(),
            bytes.len() as u64
        );
        let document = gltf::Gltf::from_slice(&bytes).unwrap();
        let components = manifest["components"].as_array().unwrap();
        let nodes = document
            .default_scene()
            .unwrap()
            .nodes()
            .collect::<Vec<_>>();
        assert_eq!(nodes.len(), components.len());
        assert_eq!(geometry.parts.len(), components.len());
        assert_eq!(
            manifest["mesh_count"].as_u64().unwrap(),
            components.len() as u64
        );
        assert_eq!(
            manifest["material_count"].as_u64().unwrap(),
            document.materials().count() as u64
        );
        let mut ids = std::collections::HashSet::new();
        let mut triangle_count = 0;
        let mut collision_boxes = Vec::new();
        for ((node, component), part) in nodes.iter().zip(components).zip(&geometry.parts) {
            let id = component["id"].as_str().unwrap();
            assert!(ids.insert(id));
            assert_eq!(node.index(), component["node"].as_u64().unwrap() as usize);
            assert_eq!(node.name(), Some(id));
            let mesh = node.mesh().unwrap();
            assert_eq!(mesh.index(), component["mesh"].as_u64().unwrap() as usize);
            assert_eq!(mesh.name(), Some(id));
            assert_eq!(mesh.primitives().count(), 1);
            assert_eq!(node.transform().matrix(), Mat4::IDENTITY.to_cols_array_2d());
            let mut min = Vec3::splat(f32::INFINITY);
            let mut max = Vec3::splat(f32::NEG_INFINITY);
            for v in part.vertices.iter() {
                min = min.min(v.p * 100.);
                max = max.max(v.p * 100.);
            }
            assert!((min - vec3(&component["bounds_min"])).abs().max_element() < 1e-5);
            assert!((max - vec3(&component["bounds_max"])).abs().max_element() < 1e-5);
            triangle_count += part.vertices.len() / 3;
            // Conservative per-component clearance proxy, not exact mesh collision.
            collision_boxes.push(arkline_protocol::PioneerDockedBox {
                center_m: ((min + max) * 0.5).to_array().map(f64::from),
                half_m: ((max - min) * 0.5).to_array().map(f64::from),
                axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            });
        }
        assert_eq!(
            manifest["triangle_count"].as_u64().unwrap(),
            triangle_count as u64
        );
        let capsule_blocked = |feet| {
            collision_boxes.iter().any(|bounds| {
                arkline_protocol::box_intersects_capsule(
                    *bounds,
                    feet,
                    arkline_protocol::AVATAR_HEIGHT_M,
                    arkline_protocol::STATION_AVATAR_RADIUS_M,
                )
            })
        };
        for i in 0..=16 {
            let t = i as f64 / 16.;
            assert!(
                !capsule_blocked([-0.85 * t, 0., 0.65 * t]),
                "Study pose/path component-envelope obstruction"
            );
        }
        let component_box = |wanted: &str| {
            collision_boxes[components.iter().position(|c| c["id"] == wanted).unwrap()]
        };
        let deck = component_box("enclosure.deck");
        assert!(
            (deck.center_m[1] + deck.half_m[1]).abs() < 1e-6,
            "Study deck must meet the unchanged zero-height feet plane"
        );
        assert!(
            arkline_protocol::box_intersects_capsule(
                deck,
                [0., -0.05, 0.],
                arkline_protocol::AVATAR_HEIGHT_M,
                arkline_protocol::STATION_AVATAR_RADIUS_M
            ),
            "Deck penetration control must be rejected while tangent standing contact stays clear"
        );
        let blocked_control = component_box("console.continuous-dashboard-shell").center_m;
        assert!(
            capsule_blocked([blocked_control[0], 0., blocked_control[2]]),
            "Collision proxy must reject a console-intersecting pose"
        );
        let instrument = &manifest["instrument"];
        let center = vec3(&instrument["center"]);
        let half = vec3(&instrument["half"]);
        let axes = &instrument["axes"];
        let rotation = Mat4::from_cols(
            vec3(&axes[0]).extend(0.),
            vec3(&axes[1]).extend(0.),
            vec3(&axes[2]).extend(0.),
            glam::Vec4::W,
        );
        let mount = Mat4::from_translation(center) * rotation * Mat4::from_translation(-center);
        assert!((instrument["plane_x"].as_f64().unwrap() as f32 - (center.x - 0.032)).abs() < 1e-6);
        let canvas = egui::vec2(
            instrument["canvas"][0].as_f64().unwrap() as f32,
            instrument["canvas"][1].as_f64().unwrap() as f32,
        );
        let exact_meshes = geometry
            .parts
            .iter()
            .map(|part| {
                Arc::from(
                    part.vertices
                        .iter()
                        .map(|v| Vertex {
                            p: v.p * 100.,
                            n: v.n,
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<Arc<[Vertex]>>>();
        let catalog = Arc::new(DepthMeshCatalog::new(exact_meshes));
        let draws = (0..geometry.parts.len())
            .map(|i| DepthDraw::whole(&catalog, i, Mat4::IDENTITY).unwrap())
            .collect();
        let scene = Arc::new(
            CockpitOcclusion::new(arkline_protocol::ShipClass::Pioneer, catalog, draws, vec![])
                .unwrap(),
        );
        let (_context, gl) = PrivateContext::create();
        let mut preview = ImportedAssetPreview::new(Some(&gl));
        preview.set(&geometry).unwrap();
        let display = Arc::new(Mutex::new(CockpitDisplayRenderer::new(gl.clone()).unwrap()));
        let mut painter = egui_glow::Painter::new(gl.clone(), "", None, false).unwrap();
        let target = CachedFeed::create(&gl, 83001, WIDTH as i32, HEIGHT as i32).unwrap();
        let ctx = egui::Context::default();
        let viewport =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(WIDTH as f32, HEIGHT as f32));
        let charge = telemetry["charge_j"].as_u64().unwrap() as f64 / 1000.;
        let ore = telemetry["ore"].as_u64().unwrap();
        let mode = telemetry["location_kind"].as_str().unwrap().to_uppercase();
        let rows = [
            ("STATE".to_owned(), mode),
            ("CAPACITOR".to_owned(), format!("{charge:0.0} kJ")),
            ("ORE [u]".to_owned(), ore.to_string()),
        ];
        let mut captures = Vec::new();
        for (name, eye, look) in [
            ("seated", Vec3::new(0., 1.35, 0.), Vec3::new(1.8, 1.10, 0.)),
            (
                "standing",
                Vec3::new(-0.85, 1.65, 0.65),
                Vec3::new(1., 1.0, 0.2),
            ),
        ] {
            let vp = Mat4::perspective_rh_gl(
                60f32.to_radians(),
                WIDTH as f32 / HEIGHT as f32,
                0.02,
                100.,
            ) * Mat4::look_at_rh(eye, look, Vec3::Y);
            let surface = CockpitSurface::registered(viewport, vp, mount, center, half, 3, canvas)
                .unwrap()
                .with_registered_occlusion(scene.clone(), vp, mount)
                .with_renderer(display.clone());
            let mut corners = Vec::new();
            let mut clear = 0;
            for point in [
                egui::pos2(0., 0.),
                egui::pos2(canvas.x, 0.),
                egui::pos2(0., canvas.y),
                egui::pos2(canvas.x, canvas.y),
                egui::pos2(canvas.x * 0.5, canvas.y * 0.5),
            ] {
                let screen = surface.project(point);
                let hit = screen.and_then(|p| surface.unproject(p));
                let good = hit.is_some_and(|p| p.distance(point) < 0.1);
                clear += usize::from(good);
                corners.push(serde_json::json!({"canvas":[point.x,point.y],"screen":screen.map(|p|[p.x,p.y]),"clear":good}));
            }
            let (label_pixels, value_pixels) = projected_fonts(&surface);
            for frame in 0..3 {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        focused: false,
                        time: Some((frame + 1) as f64 * 0.016),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                ui.painter().add(
                                    preview
                                        .shared_callback(viewport, vp, eye, Mat4::IDENTITY)
                                        .unwrap(),
                                );
                                surface.paint(ui, |p| {
                                    p.rect_filled(
                                        surface.canvas_rect(),
                                        0.,
                                        egui::Color32::from_rgb(5, 15, 18),
                                    );
                                    let mut text_bounds = Vec::new();
                                    for (i, (label, value)) in rows.iter().enumerate() {
                                        text_bounds.push(p.text(
                                            egui::pos2(LABEL_POS[i][0], LABEL_POS[i][1]),
                                            egui::Align2::LEFT_TOP,
                                            label,
                                            egui::FontId::proportional(LABEL_SIZE),
                                            egui::Color32::from_rgb(160, 193, 190),
                                        ));
                                        text_bounds.push(p.text(
                                            egui::pos2(VALUE_POS[i][0], VALUE_POS[i][1]),
                                            egui::Align2::LEFT_TOP,
                                            value,
                                            egui::FontId::monospace(VALUE_SIZE),
                                            egui::Color32::from_rgb(217, 239, 226),
                                        ));
                                    }
                                    text_bounds.push(p.text(
                                        egui::pos2(canvas.x - 12., canvas.y - 8.),
                                        egui::Align2::RIGHT_BOTTOM,
                                        "RECORDED · NOT LIVE",
                                        egui::FontId::proportional(14.),
                                        egui::Color32::from_rgb(210, 167, 98),
                                    ));
                                    for (i, bounds) in text_bounds.iter().enumerate() {
                                        assert!(surface.canvas_rect().contains_rect(*bounds),
                                            "{name}: recorded text {i} clips the instrument: {bounds:?}");
                                        for other in &text_bounds[i + 1..] {
                                            assert!(!bounds.shrink(0.25).intersects(other.shrink(0.25)),
                                                "{name}: recorded text overlaps: {bounds:?} / {other:?}");
                                        }
                                    }
                                });
                                ui.painter().text(
                                    egui::pos2(20., 20.),
                                    egui::Align2::LEFT_TOP,
                                    "ISOLATED AUTHORED-MESH PROOF · IMPORT PREVIEW LIGHTING",
                                    egui::FontId::proportional(18.),
                                    egui::Color32::WHITE,
                                );
                            });
                    },
                );
                let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
                gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
                gl.viewport(0, 0, WIDTH as i32, HEIGHT as i32);
                gl.disable(glow::SCISSOR_TEST);
                painter.paint_and_update_textures(
                    [WIDTH as u32, HEIGHT as u32],
                    output.pixels_per_point,
                    &primitives,
                    &output.textures_delta,
                );
                assert_eq!(gl.get_error(), glow::NO_ERROR);
            }
            let mut raw = vec![0u8; WIDTH * HEIGHT * 4];
            gl.read_pixels(
                0,
                0,
                WIDTH as i32,
                HEIGHT as i32,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut raw)),
            );
            let top_down = raw
                .chunks_exact(WIDTH * 4)
                .rev()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            image::save_buffer_with_format(
                out.join(format!("{name}.png")),
                &top_down,
                WIDTH as u32,
                HEIGHT as u32,
                image::ColorType::Rgba8,
                image::ImageFormat::Png,
            )
            .unwrap();
            captures.push(serde_json::json!({"view":name,"eye_m":eye.to_array(),"look_target_m":look.to_array(),"width":WIDTH,"height":HEIGHT,"all_5_display_points_clear":clear==5,"display_points":corners,"minimum_label_projected_px":label_pixels,"minimum_value_projected_px":value_pixels,"seated_label_target_px":14,"seated_value_target_px":24,"readability_target_pass":label_pixels>=14. && value_pixels>=24.,"rgba_sha256":hex::encode(Sha256::digest(&top_down))}));
        }
        let mut receipt = serde_json::json!({"scope":"Isolated original GLB asset preview with actual engine imported-mesh and cockpit-display code; no production registration or command dispatch","component_identity_verified":true,"clearance_proxy":"17 sampled standing capsules clear of conservative per-component AABBs including tangent deck contact; console and deck-penetration controls blocked; no registered support, seat or exact triangle collision", "geometry_sha256":hex::encode(Sha256::digest(&bytes)),"manifest_sha256":hex::encode(Sha256::digest(&manifest_bytes)),"recorded_telemetry_sha256":hex::encode(Sha256::digest(&telemetry_bytes)),"telemetry":telemetry,"import_report":geometry.report,"captures":captures,"limits":["Preview lighting is not live cockpit lighting","Recorded confirmed values, not live simulation or fabricated progress","Projected font em size is not human reading certification","Full canonical collision/server admission not established by importpreview"]});
        receipt["graphics"] = serde_json::json!({
            "os": std::env::consts::OS,
            "renderer": gl.get_parameter_string(glow::RENDERER),
            "version": gl.get_parameter_string(glow::VERSION),
            "text_bounds": "Measured egui glyph layouts fit the canvas without pairwise overlap",
        });
        std::fs::write(
            out.join("receipt.json"),
            serde_json::to_vec_pretty(&receipt).unwrap(),
        )
        .unwrap();
        painter.destroy();
        preview.destroy();
        display.lock().unwrap().destroy();
        target.destroy(&gl);
        // Retain the receipt before failing, so a failed regression remains inspectable.
        for capture in receipt["captures"].as_array().unwrap() {
            assert_eq!(
                capture["all_5_display_points_clear"], true,
                "display visibility/picking regression: {capture}"
            );
            assert_eq!(
                capture["readability_target_pass"], true,
                "projected font budget regression: {capture}"
            );
        }
    }
}
