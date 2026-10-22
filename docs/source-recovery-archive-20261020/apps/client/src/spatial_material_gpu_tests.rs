//! Opt-in tests of the real Pioneer mesh, shared mesh shader and cached feeds.
use super::*;
use arkline_protocol::material_graph::{
    body_box_mask, MaterialGraphDocument, MaterialNodeOp, MaterialParameterValue,
};
use std::ffi::{c_void, CString};

#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
    fn CGLChoosePixelFormat(
        attributes: *const u32,
        format: *mut *mut c_void,
        count: *mut i32,
    ) -> i32;
    fn CGLCreateContext(format: *mut c_void, share: *mut c_void, context: *mut *mut c_void) -> i32;
    fn CGLSetCurrentContext(context: *mut c_void) -> i32;
    fn CGLDestroyContext(context: *mut c_void) -> i32;
    fn CGLDestroyPixelFormat(format: *mut c_void) -> i32;
}
unsafe extern "C" {
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
}
fn revision(number: u64, doc: &MaterialGraphDocument) -> Arc<MaterialRevision> {
    Arc::new(MaterialRevision::new(1, number, doc).unwrap())
}
unsafe fn pixels(gl: &glow::Context, width: usize, height: usize) -> Vec<u8> {
    let mut bytes = vec![0u8; width * height * 4];
    gl.read_pixels(
        0,
        0,
        width as i32,
        height as i32,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut bytes)),
    );
    bytes
}
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}
fn capture(
    directory: &Option<std::path::PathBuf>,
    name: &str,
    width: usize,
    height: usize,
    bytes: &[u8],
) {
    if let Some(directory) = directory {
        let mut ppm = format!("P6\n{width} {height}\n255\n").into_bytes();
        for row in bytes.chunks_exact(width * 4).rev() {
            for pixel in row.chunks_exact(4) {
                ppm.extend_from_slice(&pixel[..3]);
            }
        }
        std::fs::write(directory.join(name), ppm).unwrap();
    }
}

#[test]
#[ignore = "Explicit private CGL Pioneer section/paused camera GPU acceptance; no app window"]
fn native_spatial_material_pioneer_section_and_paused_feeds() {
    unsafe {
        let directory =
            std::env::var_os("ARKLINE_SPATIAL_MATERIAL_EVIDENCE").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir(directory).unwrap();
        }
        let mut format = std::ptr::null_mut();
        let mut count = 0;
        let attributes = [99u32, 0x3200, 73, 8, 24, 12, 24, 0];
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
        let target = CachedFeed::create(&gl, 999, 384, 192).unwrap();
        let asset = pioneer_asset().unwrap();
        assert_eq!(
            asset.iter().filter(|p| p.material_index == Some(0)).count(),
            1
        );
        let mesh_count = renderer.meshes.len();
        let root = Mat4::from_scale(Vec3::splat(100.));
        let parts: Vec<_> = ship_parts(ShipClass::Pioneer)
            .into_iter()
            .map(|(mesh, model, color, emission)| (mesh, root * model, color, emission))
            .collect();
        let eye = Vec3::new(-10., 5., -65.);
        let vp = Mat4::orthographic_rh_gl(-42., 42., -21., 21., 0.1, 200.)
            * Mat4::look_at_rh(eye, Vec3::new(-10., 0., 0.), Vec3::Y);
        let render = |renderer: &mut Renderer, station| {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 384, 192);
            gl.disable(glow::SCISSOR_TEST);
            hangar::paint_preview(renderer, vp, eye, &parts, station);
            pixels(&gl, 384, 192)
        };
        let mut identity = MaterialGraphDocument::pioneer_default();
        identity
            .nodes
            .iter_mut()
            .find(|n| matches!(n.op, MaterialNodeOp::SurfaceOutput { .. }))
            .unwrap()
            .op = MaterialNodeOp::SurfaceOutput { surface: 24 };
        renderer
            .spatial_material
            .queue(Some(revision(1, &identity)))
            .unwrap();
        let baseline = render(&mut renderer, true);
        let baseline_space = render(&mut renderer, false);
        assert_eq!(renderer.spatial_material.status().compilation_count, 1);
        let mut section = MaterialGraphDocument::pioneer_default();
        for (id, value) in [
            (6, MaterialParameterValue::LinearColor([0.055, 0.13, 0.18])),
            (7, MaterialParameterValue::Scalar(0.55)),
            (8, MaterialParameterValue::Scalar(0.0)),
            (10, MaterialParameterValue::LinearColor([0.42, 0.45, 0.44])),
            (11, MaterialParameterValue::Scalar(0.24)),
            (12, MaterialParameterValue::Scalar(1.0)),
        ] {
            section
                .parameters
                .iter_mut()
                .find(|p| p.id == id)
                .unwrap()
                .value = value;
        }
        let lowered = revision(2, &section);
        renderer
            .spatial_material
            .queue(Some(lowered.clone()))
            .unwrap();
        let hangar = render(&mut renderer, true);
        let space = render(&mut renderer, false);
        let section_changed = changed(&baseline, &hangar);
        assert!(
            section_changed > 64 && section_changed < 384 * 192 / 3,
            "section must affect a bounded visible part: {section_changed}"
        );
        assert!(changed(&baseline_space, &space) > 64);
        assert_ne!(
            hangar, space,
            "matched geometry uses genuinely different existing lighting paths"
        );
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(384., 192.));
        let mut outside = 0;
        let mut inside_changed = 0;
        for y in (8..192).step_by(12) {
            for x in (8..384).step_by(12) {
                let (origin, direction) =
                    screen_ray(vp, rect, egui::pos2(x as f32 + 0.5, 192. - y as f32 - 0.5))
                        .unwrap();
                let hit = asset
                    .iter()
                    .filter_map(|p| {
                        ray_mesh_hit(origin, direction, root, &p.vertices)
                            .map(|distance| (distance, p.material_index))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0));
                let Some((distance, slot)) = hit else {
                    continue;
                };
                let position = origin + direction * distance;
                let mask =
                    body_box_mask(position.to_array(), [-22., -8., -20.], [2., 7., -14.], 0.5)
                        .unwrap();
                let i = (y * 384 + x) * 4;
                if slot != Some(0) || mask == 0. {
                    assert_eq!(
                        &baseline[i..i + 4],
                        &hangar[i..i + 4],
                        "unbound/outside surface changed at {position:?}, material {slot:?}"
                    );
                    outside += 1;
                } else if baseline[i..i + 4] != hangar[i..i + 4] {
                    inside_changed += 1;
                }
            }
        }
        assert!(
            outside > 32 && inside_changed > 4,
            "actual mesh samples: {outside} unchanged outside, {inside_changed} changed inside"
        );
        renderer.debug_wireframe = true;
        let wire = render(&mut renderer, true);
        renderer.debug_wireframe = false;
        let green_inside = baseline
            .chunks_exact(4)
            .zip(hangar.chunks_exact(4))
            .zip(wire.chunks_exact(4))
            .filter(|((base, solid), wire)| {
                base != solid
                    && wire[1] > 120
                    && u16::from(wire[1]) > u16::from(wire[0]) + 25
                    && u16::from(wire[1]) > u16::from(wire[2]) + 15
            })
            .count();
        capture(&directory, "pioneer-section-wire.ppm", 384, 192, &wire);
        assert!(
            green_inside > 16,
            "wireframe stays green inside the graph region: {green_inside}"
        );
        let compile_count = renderer.spatial_material.status().compilation_count;
        let mut expanded = section.clone();
        expanded
            .parameters
            .iter_mut()
            .find(|p| p.id == 15)
            .unwrap()
            .value = MaterialParameterValue::Vector3([16., 7., -14.]);
        renderer
            .spatial_material
            .queue(Some(revision(3, &expanded)))
            .unwrap();
        let expanded_pixels = render(&mut renderer, true);
        assert!(changed(&baseline, &expanded_pixels) > section_changed);
        assert_eq!(
            renderer.spatial_material.status().compilation_count,
            compile_count,
            "box bounds are uniform edits"
        );
        capture(
            &directory,
            "pioneer-authored-hangar.ppm",
            384,
            192,
            &baseline,
        );
        capture(&directory, "pioneer-section-hangar.ppm", 384, 192, &hangar);
        capture(
            &directory,
            "pioneer-authored-space.ppm",
            384,
            192,
            &baseline_space,
        );
        capture(&directory, "pioneer-section-space.ppm", 384, 192, &space);
        capture(
            &directory,
            "pioneer-section-expanded.ppm",
            384,
            192,
            &expanded_pixels,
        );

        let pilot: PilotView = serde_json::from_value(serde_json::json!({"name":"MaterialQA","credits":0,"ore":0,"location":Location::Belt,"system_id":3,"flight":{"position":[0.,0.,0.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],"mode":NavigationMode::Stop,"updated_at_ms":1000}})).unwrap();
        let flight_eye = eye / 100.;
        let flight_vp = Mat4::orthographic_rh_gl(-0.42, 0.42, -0.21, 0.21, 0.001, 2.)
            * Mat4::look_at_rh(flight_eye, Vec3::new(-0.1, 0., 0.), Vec3::Y);
        let frame = Frame {
            exterior: None,
            lighting_quality: Default::default(),
            cockpit_vp: flight_vp,
            cockpit_occlusion: None,
            snapshot_time: 1000,
            planet_vp: flight_vp,
            system_id: 3,
            ship_class: ShipClass::Pioneer,
            drone_camera: true,
            camera_drone: None,
            scene: Arc::new(FrameScene {
                world_layout: arkline_protocol::legacy_world_layout(),
                traffic: vec![],
                surface_props: vec![],
                asteroids: vec![],
                gates: vec![],
                pilots: vec![pilot],
            }),
            space: Space { origin: [0.; 3] },
            cockpit: false,
            rotation: Quat::IDENTITY,
            connected: true,
            vp: flight_vp,
            eye: flight_eye,
            now: 1000,
            own: "MaterialQA".into(),
            docking: None,
        };
        let info = || egui::PaintCallbackInfo {
            viewport: rect,
            clip_rect: rect,
            pixels_per_point: 1.,
            screen_size_px: [384, 192],
        };
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
        renderer.paint_cached_feed(&frame, 31, info(), 1, Some(target.framebuffer));
        let feed = renderer.feed_cache.iter().find(|c| c.key == 31).unwrap();
        assert_eq!(feed.painted_ms, Some(1000));
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(feed.framebuffer));
        let feed_before = pixels(&gl, feed.width as usize, feed.height as usize);
        let mut altered = expanded.clone();
        altered
            .parameters
            .iter_mut()
            .find(|p| p.id == 6)
            .unwrap()
            .value = MaterialParameterValue::LinearColor([0.85, 0.02, 0.05]);
        renderer
            .spatial_material
            .queue(Some(revision(4, &altered)))
            .unwrap();
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
        renderer.paint_cached_feed(&frame, 31, info(), 1, Some(target.framebuffer));
        let feed = renderer.feed_cache.iter().find(|c| c.key == 31).unwrap();
        assert_eq!(
            feed.painted_ms,
            Some(1000),
            "simulation time remains paused"
        );
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(feed.framebuffer));
        let feed_after = pixels(&gl, feed.width as usize, feed.height as usize);
        assert!(
            changed(&feed_before, &feed_after) > 16,
            "same-time cached feed must refresh after parameter edit"
        );
        assert_eq!(
            renderer.spatial_material.status().compilation_count,
            compile_count
        );
        let program = renderer.program;
        let mut unsupported = altered;
        unsupported.binding.material_index = 1;
        renderer
            .spatial_material
            .queue(Some(revision(5, &unsupported)))
            .unwrap();
        renderer.prepare_spatial_material();
        assert_eq!(renderer.program, program);
        assert_eq!(
            renderer.spatial_material.status().phase,
            crate::MaterialProgramPhase::Rejected
        );
        assert_eq!(
            renderer.spatial_material.status().applied.unwrap().revision,
            4
        );
        assert_eq!(
            renderer.spatial_material.status().compilation_count,
            compile_count,
            "unsupported same-topology binding cannot bypass validation"
        );
        assert_eq!(renderer.meshes.len(), mesh_count);
        assert_eq!(gl.get_error(), glow::NO_ERROR);
        eprintln!(
            "Pioneer section: {section_changed}/73728 changed pixels; {outside} outside samples unchanged; {inside_changed} inside samples changed; paused feed {} pixels changed",
            changed(&feed_before, &feed_after)
        );
        if let Some(directory) = &directory {
            std::fs::write(directory.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"build":include_str!("../../../BUILD_VERSION").trim(),"asset_sha256":pioneer_asset_sha256(),"vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),"gl_version":gl.get_parameter_string(glow::VERSION),"section_changed_pixels":section_changed,"outside_unchanged_samples":outside,"inside_changed_samples":inside_changed,"paused_feed_changed_pixels":changed(&feed_before,&feed_after),"compilation_count":compile_count,"limitations":["offscreen native GPU acceptance; no operating-system UI journey","matched mesh preview uses existing lighting; no released build claim","WebGL execution requires separate browser evidence"]})).unwrap()).unwrap();
        }
        renderer.destroy();
        target.destroy(&gl);
        CGLSetCurrentContext(std::ptr::null_mut());
        CGLDestroyContext(context);
    }
}
