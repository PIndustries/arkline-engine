//! Opt-in Linux captures of the actual flight renderer, using Mesa EGL.
use super::*;
use arkline_protocol::{ExteriorDriveState, ExteriorVesselState, ExteriorWorldState};
use std::ffi::{c_void, CString};

struct Egl {
    library: *mut c_void,
    display: *mut c_void,
    surface: *mut c_void,
    context: *mut c_void,
}
#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(path: *const i8, flags: i32) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const i8) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> i32;
}
impl Egl {
    unsafe fn symbol<T: Copy>(&self, name: &str) -> T {
        let pointer = dlsym(self.library, CString::new(name).unwrap().as_ptr());
        assert!(!pointer.is_null(), "missing EGL symbol {name}");
        std::mem::transmute_copy(&pointer)
    }
    unsafe fn create() -> (Self, Arc<glow::Context>) {
        let mut egl = Self {
            library: dlopen(c"libEGL.so.1".as_ptr(), 2),
            display: std::ptr::null_mut(),
            surface: std::ptr::null_mut(),
            context: std::ptr::null_mut(),
        };
        assert!(
            !egl.library.is_null(),
            "libEGL.so.1 is required for explicit native GPU acceptance"
        );
        let get_proc: unsafe extern "C" fn(*const i8) -> *mut c_void =
            egl.symbol("eglGetProcAddress");
        let platform: unsafe extern "C" fn(u32, *mut c_void, *const i32) -> *mut c_void =
            std::mem::transmute(get_proc(c"eglGetPlatformDisplayEXT".as_ptr()));
        egl.display = platform(0x31dd, std::ptr::null_mut(), std::ptr::null());
        let initialize: unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32) -> u32 =
            egl.symbol("eglInitialize");
        assert_ne!(
            initialize(egl.display, std::ptr::null_mut(), std::ptr::null_mut()),
            0
        );
        let bind: unsafe extern "C" fn(u32) -> u32 = egl.symbol("eglBindAPI");
        assert_ne!(bind(0x30a2), 0);
        let choose: unsafe extern "C" fn(
            *mut c_void,
            *const i32,
            *mut *mut c_void,
            i32,
            *mut i32,
        ) -> u32 = egl.symbol("eglChooseConfig");
        let attributes = [
            0x3033, 1, 0x3040, 8, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3025, 24, 0x3038,
        ];
        let mut config = std::ptr::null_mut();
        let mut count = 0;
        assert_ne!(
            choose(egl.display, attributes.as_ptr(), &mut config, 1, &mut count),
            0
        );
        assert!(count > 0);
        let surface: unsafe extern "C" fn(*mut c_void, *mut c_void, *const i32) -> *mut c_void =
            egl.symbol("eglCreatePbufferSurface");
        egl.surface = surface(
            egl.display,
            config,
            [0x3057, 960, 0x3056, 640, 0x3038].as_ptr(),
        );
        assert!(!egl.surface.is_null());
        let context: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
            *const i32,
        ) -> *mut c_void = egl.symbol("eglCreateContext");
        egl.context = context(egl.display, config, std::ptr::null_mut(), [0x3038].as_ptr());
        assert!(!egl.context.is_null());
        let current: unsafe extern "C" fn(
            *mut c_void,
            *mut c_void,
            *mut c_void,
            *mut c_void,
        ) -> u32 = egl.symbol("eglMakeCurrent");
        assert_ne!(
            current(egl.display, egl.surface, egl.surface, egl.context),
            0
        );
        let gl = Arc::new(glow::Context::from_loader_function(|name| {
            get_proc(CString::new(name).unwrap().as_ptr())
        }));
        (egl, gl)
    }
}
impl Drop for Egl {
    fn drop(&mut self) {
        unsafe {
            let current: unsafe extern "C" fn(
                *mut c_void,
                *mut c_void,
                *mut c_void,
                *mut c_void,
            ) -> u32 = self.symbol("eglMakeCurrent");
            current(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            let destroy_context: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32 =
                self.symbol("eglDestroyContext");
            destroy_context(self.display, self.context);
            let destroy_surface: unsafe extern "C" fn(*mut c_void, *mut c_void) -> u32 =
                self.symbol("eglDestroySurface");
            destroy_surface(self.display, self.surface);
            let terminate: unsafe extern "C" fn(*mut c_void) -> u32 = self.symbol("eglTerminate");
            terminate(self.display);
            dlclose(self.library);
        }
    }
}
unsafe fn pixels(gl: &glow::Context) -> Vec<u8> {
    let mut result = vec![0; 960 * 640 * 4];
    gl.read_pixels(
        0,
        0,
        960,
        640,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut result)),
    );
    assert_eq!(gl.get_error(), glow::NO_ERROR);
    result
}
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count()
}
unsafe fn depths(gl: &glow::Context) -> Vec<f32> {
    let mut bytes = vec![0; 960 * 640 * 4];
    gl.read_pixels(
        0,
        0,
        960,
        640,
        glow::DEPTH_COMPONENT,
        glow::FLOAT,
        glow::PixelPackData::Slice(Some(&mut bytes)),
    );
    assert_eq!(gl.get_error(), glow::NO_ERROR);
    bytes
        .chunks_exact(4)
        .map(|p| f32::from_ne_bytes(p.try_into().unwrap()))
        .collect()
}
fn receiver_changed(frame: &Frame, depth: &[f32], a: &[u8], b: &[u8]) -> usize {
    // Reconstruct the actual visible surface from the engine's depth buffer.
    // The crate lies beyond X=-38m, outside the hull's -36m aft extent.
    let inverse = frame.vp.inverse();
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .enumerate()
        .filter(|(i, (a, b))| {
            if a == b {
                return false;
            }
            let point = inverse
                * glam::Vec4::new(
                    ((*i % 960) as f32 + 0.5) / 480. - 1.,
                    ((*i / 960) as f32 + 0.5) / 320. - 1.,
                    depth[*i] * 2. - 1.,
                    1.,
                );
            let point = point.truncate() / point.w;
            point.cmpge(Vec3::new(-0.4601, -0.1001, -0.1651)).all()
                && point.cmple(Vec3::new(-0.3799, -0.0199, -0.0849)).all()
        })
        .count()
}

unsafe fn assert_post_graph_power_gate(gl: &glow::Context, renderer: &Renderer, frame: &Frame) {
    // Exercise the integrated mesh shader with a graph that explicitly emits.
    // Current editor bindings remain restricted to armor material slot zero.
    let program = compile(gl, VERTEX, &format!("{}{}{}", mesh_fragment_prefix(),
        "struct ArkSurface{vec3 base_color;float roughness;float metallic;vec3 emission;};ArkSurface ark_graph_surface(vec3 p,ArkSurface s){s.emission=vec3(4.0);return s;}", FRAGMENT_BODY)).unwrap();
    gl.use_program(Some(program));
    gl.uniform_matrix_4_f32_slice(
        gl.get_uniform_location(program, "view_projection").as_ref(),
        false,
        &frame.vp.to_cols_array(),
    );
    gl.uniform_matrix_4_f32_slice(
        gl.get_uniform_location(program, "model").as_ref(),
        false,
        &Mat4::IDENTITY.to_cols_array(),
    );
    gl.uniform_3_f32(
        gl.get_uniform_location(program, "eye").as_ref(),
        frame.eye.x,
        frame.eye.y,
        frame.eye.z,
    );
    gl.uniform_4_f32(
        gl.get_uniform_location(program, "material_params").as_ref(),
        0.5,
        0.,
        0.,
        0.,
    );
    gl.bind_vertex_array(Some(renderer.meshes[14].vao));
    gl.disable(glow::DEPTH_TEST);
    let render = |power, graph| {
        gl.clear_color(0., 0., 0., 1.);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.uniform_1_f32(
            gl.get_uniform_location(program, "emission_power").as_ref(),
            power,
        );
        gl.uniform_1_i32(
            gl.get_uniform_location(program, "ark_graph_enabled")
                .as_ref(),
            graph,
        );
        gl.draw_arrays(glow::TRIANGLES, 0, renderer.meshes[14].count);
        pixels(gl)
    };
    let powered = render(1., 1);
    let off = render(0., 1);
    let authored_dark = render(1., 0);
    assert!(
        changed(&powered, &off) > 16,
        "power gates graph-authored emission"
    );
    assert_eq!(
        off, authored_dark,
        "OFF must suppress emission after graph evaluation"
    );
    gl.bind_vertex_array(None);
    gl.use_program(None);
    gl.delete_program(program);
}
fn save(directory: &Option<std::path::PathBuf>, name: &str, pixels: &[u8]) {
    if let Some(directory) = directory {
        let flipped: Vec<_> = pixels
            .chunks_exact(960 * 4)
            .rev()
            .flat_map(|row| row.iter().copied())
            .collect();
        image::save_buffer(
            directory.join(name),
            &flipped,
            960,
            640,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}
fn exterior() -> ExteriorWorldState {
    ExteriorWorldState {
        definition: arkline_protocol::ExteriorDefinitionState {
            revision: 1,
            document: arkline_protocol::default_pioneer_exterior(),
        },
        vessels: vec![ExteriorVesselState {
            pilot: "ExteriorQA".into(),
            ship_class: ShipClass::Pioneer,
            vessel_id: "v1:ExteriorQA:Pioneer".into(),
            enabled: false,
            powered: false,
            drive: ExteriorDriveState::Off,
            load_w: 0,
        }],
    }
}
fn frame() -> Frame {
    let eye = Vec3::new(-0.85, 0.50, -0.95);
    let vp = Mat4::orthographic_rh_gl(-0.54, 0.54, -0.36, 0.36, 0.001, 4.)
        * Mat4::look_at_rh(eye, Vec3::new(-0.04, 0., 0.), Vec3::Y);
    let pilot: PilotView =
        serde_json::from_value(serde_json::json!({"name":"ExteriorQA","credits":0,"ore":0,
        "ship_class":"pioneer","location":Location::Belt,"system_id":3,
        "flight":{"position":[0.,0.,0.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],
        "mode":NavigationMode::Stop,"updated_at_ms":1000,"acceleration":[0.,0.,0.]}}))
        .unwrap();
    Frame {
        pioneer_parts: None,
        exterior: Some(Arc::new(exterior())),
        lighting_quality: super::super::lighting_settings::LightingQuality::High,
        cockpit_vp: vp,
        cockpit_occlusion: None,
        snapshot_time: 1000,
        planet_vp: vp,
        system_id: 3,
        ship_class: ShipClass::Pioneer,
        drone_camera: false,
        camera_drone: None,
        scene: Arc::new(FrameScene {
            world_layout: arkline_protocol::legacy_world_layout(),
            traffic: vec![],
            surface_props: vec![arkline_protocol::SurfaceProp {
                id: 1,
                owner: "ExteriorQA".into(),
                system_id: 3,
                kind: arkline_protocol::SurfacePropKind::Crate,
                position: [-42., -6., -12.5],
            }],
            asteroids: vec![],
            gates: vec![],
            pilots: vec![pilot],
        }),
        space: Space { origin: [0.; 3] },
        cockpit: false,
        rotation: Quat::IDENTITY,
        connected: true,
        vp,
        eye,
        now: 1000,
        own: "ExteriorQA".into(),
        docking: None,
    }
}
fn state(frame: &mut Frame) -> &mut ExteriorWorldState {
    Arc::make_mut(frame.exterior.as_mut().unwrap())
}

#[test]
#[ignore = "Explicit actual flight Renderer::paint EGL powered-exterior capture; no OS window"]
fn native_powered_exterior_frame_capture() {
    unsafe {
        let (_egl, gl) = Egl::create();
        let directory =
            std::env::var_os("ARKLINE_EXTERIOR_CAPTURE_DIR").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir_all(directory).unwrap();
        }
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, 812, 960, 640).unwrap();
        let render = |renderer: &mut Renderer, frame: &Frame| {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 960, 640);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            renderer.paint(frame, Some(target.framebuffer));
            pixels(&gl)
        };
        let mut frame = frame();
        let off = render(&mut renderer, &frame);
        save(&directory, "pioneer-exterior-off.png", &off);
        {
            let world = state(&mut frame);
            world.vessels[0].enabled = true;
            world.vessels[0].powered = true;
            world.vessels[0].drive = ExteriorDriveState::Idle;
        }
        let idle = render(&mut renderer, &frame);
        save(&directory, "pioneer-exterior-idle.png", &idle);
        assert!(
            changed(&off, &idle) > 32,
            "OFF and IDLE must visibly differ"
        );
        state(&mut frame).vessels[0].drive = ExteriorDriveState::Active;
        let active = render(&mut renderer, &frame);
        save(&directory, "pioneer-exterior-active.png", &active);
        assert!(
            changed(&idle, &active) > 16,
            "ACTIVE must change confirmed drive glow"
        );
        assert_eq!(
            renderer.turrets.len(),
            2,
            "existing articulated turrets remain present"
        );
        let repeated = render(&mut renderer, &frame);
        assert_eq!(active, repeated, "rendering cannot advance simulation time");
        frame.snapshot_time += 50;
        frame.now += 50;
        let step50 = render(&mut renderer, &frame);
        save(&directory, "pioneer-exterior-step-50ms.png", &step50);
        assert_eq!(
            active, step50,
            "first fixed step stays inside the 100ms strobe pulse"
        );
        frame.snapshot_time += 50;
        frame.now += 50;
        let step100 = render(&mut renderer, &frame);
        save(&directory, "pioneer-exterior-step-100ms.png", &step100);
        assert!(
            changed(&step50, &step100) > 0,
            "second fixed step reaches the strobe dark interval"
        );
        // Same source pixels and geometry; only local illumination is removed.
        let lit = step100;
        let lit_depth = depths(&gl);
        for fixture in &mut state(&mut frame).definition.document.fixtures {
            fixture.light_intensity = 0.;
        }
        let unlit = render(&mut renderer, &frame);
        save(&directory, "pioneer-exterior-emission-only.png", &unlit);
        let illumination_pixels = changed(&lit, &unlit);
        let receiver_pixels = receiver_changed(&frame, &lit_depth, &lit, &unlit);
        assert!(
            illumination_pixels > 32,
            "powered fixtures must illuminate non-emissive receiving surfaces"
        );
        assert!(
            receiver_pixels > 8,
            "the separate non-emissive receiver must be illuminated: {receiver_pixels} receiver pixels, {illumination_pixels} total"
        );
        // Authoring fields must reach the material-batched source emitters,
        // including the source drive material and its bounded nozzle glow surface.
        let mut edited_material_pixels = serde_json::Map::new();
        for (kind, name, eye) in [
            (
                arkline_protocol::ExteriorFixtureKind::EngineGlow,
                "drive",
                frame.eye,
            ),
            (
                arkline_protocol::ExteriorFixtureKind::Window,
                "window",
                Vec3::new(0.85, 0.5, -0.95),
            ),
            (
                arkline_protocol::ExteriorFixtureKind::Panel,
                "panel",
                Vec3::new(0.85, 0.5, -0.95),
            ),
        ] {
            let mut edited = frame.clone();
            edited.eye = eye;
            edited.vp = Mat4::orthographic_rh_gl(-0.54, 0.54, -0.36, 0.36, 0.001, 4.)
                * Mat4::look_at_rh(eye, Vec3::new(-0.04, 0., 0.), Vec3::Y);
            edited.cockpit_vp = edited.vp;
            edited.planet_vp = edited.vp;
            let original = render(&mut renderer, &edited);
            save(
                &directory,
                &format!("pioneer-exterior-{name}-before-edit.png"),
                &original,
            );
            for fixture in &mut state(&mut edited).definition.document.fixtures {
                if fixture.kind == kind {
                    fixture.emission = 0.;
                }
            }
            let zero = render(&mut renderer, &edited);
            let zero_changed = changed(&original, &zero);
            assert!(
                zero_changed > 8,
                "{name} emission=0 must change actual emitter pixels"
            );
            save(
                &directory,
                &format!("pioneer-exterior-{name}-emission-zero.png"),
                &zero,
            );
            edited.exterior = frame.exterior.clone();
            for fixture in &mut state(&mut edited).definition.document.fixtures {
                if fixture.kind == kind {
                    fixture.color = [1., 0.02, 0.01];
                }
            }
            let red = render(&mut renderer, &edited);
            let color_changed = changed(&original, &red);
            assert!(
                color_changed > 8,
                "{name} color must change actual emitter pixels"
            );
            save(
                &directory,
                &format!("pioneer-exterior-{name}-color-red.png"),
                &red,
            );
            edited_material_pixels.insert(
                name.into(),
                serde_json::json!({"emission_zero":zero_changed,"color":color_changed}),
            );
        }
        // A paused cached feed must accept new state without requiring clock advancement.
        let info = || egui::PaintCallbackInfo {
            viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960., 640.)),
            clip_rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960., 640.)),
            pixels_per_point: 1.,
            screen_size_px: [960, 640],
        };
        renderer.paint_cached_feed(&frame, 813, info(), 1, Some(target.framebuffer));
        let feed_before = pixels(&gl);
        {
            let vessel = &mut state(&mut frame).vessels[0];
            vessel.enabled = false;
            vessel.powered = false;
            vessel.drive = ExteriorDriveState::Off;
        }
        renderer.paint_cached_feed(&frame, 813, info(), 1, Some(target.framebuffer));
        let feed_after = pixels(&gl);
        assert!(
            changed(&feed_before, &feed_after) > 16,
            "paused state changes must invalidate cached feeds"
        );
        // A second renderer consumes the same observer snapshot and matches exactly.
        let mut observer = Renderer::new(gl.clone()).unwrap();
        let observer_pixels = render(&mut observer, &frame);
        assert_eq!(render(&mut renderer, &frame), observer_pixels);
        assert_post_graph_power_gate(&gl, &renderer, &frame);
        if let Some(directory) = directory {
            std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
                "build":include_str!("../../../BUILD_VERSION").trim(),"asset_sha256":pioneer_asset_sha256(),
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),
                "gl_version":gl.get_parameter_string(glow::VERSION),"viewport":[960,640],
                "off_idle_changed_pixels":changed(&off,&idle),"idle_active_changed_pixels":changed(&idle,&active),
                "illumination_changed_pixels":illumination_pixels,"dynamic_turret_count":renderer.turrets.len(),
                "receiver_changed_pixels":receiver_pixels,"post_graph_off_gate":true,
                "paused_frame_equal":true,"step_50_ms_same_pulse":true,"step_100_ms_changed":true,
                "paused_feed_invalidated":true,"observer_same_snapshot_equal":true,
                "edited_material_pixels":edited_material_pixels,
                "limitations":["actual engine offscreen EGL capture; no OS interaction or TCP two-client journey", "diagnostic receiver crate uses the existing SurfaceProp path in this capture fixture", "no model adoption or release publication"]
            })).unwrap()).unwrap();
        }
        observer.destroy();
        target.destroy(&gl);
        renderer.destroy();
    }
}

const AUTHORITY_CAPTURE_PILOT: &str = "PioneerAdoptionQA";
const PRE_ADOPTION_HULL_SHA256: &str =
    "f8f8cc65772c678a46606c27abe205d62c4e6ab37c350f79a0f3a4cdc0c6f402";

struct AuthorityCaptureDirectory(std::path::PathBuf);
impl AuthorityCaptureDirectory {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        Self(std::env::temp_dir().join(format!(
            "arkline-pioneer-adoption-{}-{nonce}",
            std::process::id()
        )))
    }
}
impl Drop for AuthorityCaptureDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn authority_request(
    world: &mut arkline_server::editor::EditorWorld,
    session: &mut Option<String>,
    request: arkline_protocol::Request,
) -> Snapshot {
    let response = world.handle(session, request);
    assert!(
        response.ok,
        "authority rejected capture command: {}",
        response.message
    );
    response
        .snapshot
        .expect("accepted capture command supplies a snapshot")
}
fn authority_pilot(snapshot: &Snapshot) -> &PilotView {
    snapshot
        .pilots
        .iter()
        .find(|p| p.name == AUTHORITY_CAPTURE_PILOT)
        .unwrap()
}
fn authority_vessel(snapshot: &Snapshot) -> &ExteriorVesselState {
    snapshot
        .exterior
        .as_ref()
        .unwrap()
        .vessels
        .iter()
        .find(|v| v.pilot == AUTHORITY_CAPTURE_PILOT && v.ship_class == ShipClass::Pioneer)
        .unwrap()
}

#[derive(Clone, Copy)]
struct AuthorityCaptureCamera {
    name: &'static str,
    eye_m: [f32; 3],
    target_m: [f32; 3],
    up: [f32; 3],
    half_width_m: f32,
}
impl AuthorityCaptureCamera {
    fn frame(self, snapshot: &Snapshot) -> Frame {
        let pilot = authority_pilot(snapshot);
        let now = snapshot.server_time_ms;
        let space = Space {
            origin: display_position(pilot, now),
        };
        let rotation = ship_orientation_at(pilot, now);
        let eye = rotation * Vec3::from_array(self.eye_m) / 100.;
        let target = rotation * Vec3::from_array(self.target_m) / 100.;
        let half_width = self.half_width_m / 100.;
        let half_height = half_width * 2. / 3.;
        let vp = Mat4::orthographic_rh_gl(
            -half_width,
            half_width,
            -half_height,
            half_height,
            0.001,
            8.,
        ) * Mat4::look_at_rh(eye, target, rotation * Vec3::from_array(self.up));
        Frame {
            pioneer_parts: None,
            exterior: snapshot.exterior.clone().map(Arc::new),
            lighting_quality: super::super::lighting_settings::LightingQuality::High,
            cockpit_vp: vp,
            cockpit_occlusion: None,
            snapshot_time: now,
            planet_vp: vp,
            system_id: pilot.system_id,
            ship_class: pilot.ship_class,
            drone_camera: false,
            camera_drone: None,
            // Isolate the actual accepted pilot for model review. No diagnostic
            // props, invented equipment, traffic, rocks or extra pilots are added.
            scene: Arc::new(FrameScene {
                world_layout: super::super::snapshot_world_layout(snapshot)
                    .expect("authority snapshot has a supported world layout")
                    .clone(),
                traffic: vec![],
                surface_props: vec![],
                asteroids: vec![],
                gates: vec![],
                pilots: vec![pilot.clone()],
            }),
            space,
            cockpit: false,
            rotation,
            connected: true,
            vp,
            eye,
            now,
            own: AUTHORITY_CAPTURE_PILOT.into(),
            docking: None,
        }
    }
    fn receipt(self, frame: &Frame) -> serde_json::Value {
        serde_json::json!({"name":self.name,"projection":"orthographic",
            "eye_body_m":self.eye_m,"target_body_m":self.target_m,"up_body":self.up,
            "half_width_m":self.half_width_m,"half_height_m":self.half_width_m*2./3.,
            "eye_scene":frame.eye.to_array(),"origin_world_m":frame.space.origin,
            "view_projection":frame.vp.to_cols_array()})
    }
}
fn authority_public_receipt(snapshot: &Snapshot) -> serde_json::Value {
    let pilot = authority_pilot(snapshot);
    let exterior = snapshot.exterior.as_ref().unwrap();
    let flight = pilot.flight.as_ref().unwrap();
    // Select only public renderer inputs; never serialize Response or session.
    serde_json::json!({"simulation_time_ms":snapshot.server_time_ms,
        "definition_revision":exterior.definition.revision,"location":pilot.location,"system_id":pilot.system_id,
        "hull_asset_sha256":exterior.definition.document.hull_asset_sha256,
        "vessel":authority_vessel(snapshot),"power":pilot.power,
        "flight":{"position_m":flight.position,"velocity_m_s":flight.velocity,
            "acceleration_m_s2":flight.acceleration,"orientation":flight.orientation,
            "updated_at_ms":flight.updated_at_ms},
        "strobe_outputs":exterior.definition.document.fixtures.iter()
            .filter(|f|f.kind==arkline_protocol::ExteriorFixtureKind::Strobe)
            .map(|f|serde_json::json!({"id":f.id,"pulse":f.pulse,
                "output":f.output(authority_vessel(snapshot),snapshot.server_time_ms)})).collect::<Vec<_>>()})
}

#[test]
#[ignore = "Explicit installed-hull capture from real disposable EditorWorld commands and clock; no OS window"]
fn native_authority_pioneer_adoption_capture() {
    use arkline_protocol::{ExteriorFixtureKind, Request};
    let expected_hash = std::env::var("ARKLINE_ADOPTION_EXPECTED_SHA256")
        .expect("set ARKLINE_ADOPTION_EXPECTED_SHA256 to the reviewed candidate export hash");
    assert_eq!(
        pioneer_asset_sha256(),
        expected_hash,
        "compiled hull must match the explicitly reviewed candidate export"
    );
    let previous_hash = std::env::var("ARKLINE_ADOPTION_PREVIOUS_SHA256")
        .unwrap_or_else(|_| PRE_ADOPTION_HULL_SHA256.into());
    assert!(
        previous_hash.len() == 64
            && previous_hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "previous hull binding must be an exact lowercase SHA-256"
    );
    assert_ne!(
        pioneer_asset_sha256(),
        previous_hash,
        "the historical graph binding must differ from the installed reviewed hull"
    );
    unsafe {
        let (_egl, gl) = Egl::create();
        let directory =
            std::env::var_os("ARKLINE_ADOPTION_CAPTURE_DIR").map(std::path::PathBuf::from);
        if let Some(directory) = &directory {
            std::fs::create_dir_all(directory).unwrap();
        }
        let sandbox = AuthorityCaptureDirectory::new();
        let mut world = arkline_server::editor::EditorWorld::open(&sandbox.0).unwrap();
        let mut session = None;
        authority_request(
            &mut world,
            &mut session,
            Request::Connect {
                supports_room_controls: true,
                world_layout_version: 1,
                supports_fleet_hangar: true,
                supports_pioneer_interior: true,
                supports_docked_traversal_v2: false,
                protocol_version: arkline_protocol::PROTOCOL_VERSION,
                pilot: AUTHORITY_CAPTURE_PILOT.into(),
                resume_token: None,
            },
        );
        authority_request(&mut world, &mut session, Request::Undock);
        // Leave the real station's shadow through an accepted warp and fixed
        // authority ticks. The renderer retains its ordinary sun and shadows.
        let warping = authority_request(
            &mut world,
            &mut session,
            Request::Warp {
                destination: arkline_protocol::Destination::Belt,
            },
        );
        let arrival = authority_pilot(&warping)
            .flight
            .as_ref()
            .unwrap()
            .warp
            .as_ref()
            .unwrap()
            .arrives_at_ms;
        let warp_started = world.time();
        assert!(arrival - warp_started < 120_000);
        while world.time() < arrival {
            world.step(50).unwrap();
        }
        let arrived = authority_request(&mut world, &mut session, Request::Snapshot);
        assert_eq!(authority_pilot(&arrived).location, Location::Belt);
        // Align the concept-angle camera with the existing horizontal sunlight
        // through real yaw controls. Never write an orientation or a power state.
        let heading = authority_pilot(&arrived).flight.as_ref().unwrap().heading;
        let current_yaw = (-heading[2]).atan2(heading[0]);
        let target_yaw = (-0.65f64).atan2(-0.5) - std::f64::consts::FRAC_PI_4;
        let mut remaining = (target_yaw - current_yaw + std::f64::consts::PI)
            .rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        let mut yaw_steps = 0;
        while remaining.abs() > 1e-10 {
            let input = (remaining / (arkline_protocol::MAX_ANGULAR_SPEED * 0.05)).clamp(-1., 1.);
            authority_request(
                &mut world,
                &mut session,
                Request::PilotInput {
                    translation: [0.; 3],
                    rotation: [0., input, 0.],
                },
            );
            world.step(50).unwrap();
            remaining -= input * arkline_protocol::MAX_ANGULAR_SPEED * 0.05;
            yaw_steps += 1;
            assert!(yaw_steps <= 41);
        }
        authority_request(
            &mut world,
            &mut session,
            Request::PilotInput {
                translation: [0.; 3],
                rotation: [0.; 3],
            },
        );
        world.step(50).unwrap();
        let accepted_definition = world.exterior_state().unwrap();
        assert_eq!(
            accepted_definition.document,
            arkline_protocol::default_pioneer_exterior(),
            "capture must use the unchanged installed default fixture definition"
        );
        assert_eq!(
            accepted_definition.document.hull_asset_sha256,
            pioneer_asset_sha256()
        );
        world
            .exterior_set_enabled(&mut session, false, accepted_definition.revision)
            .unwrap();
        let off = authority_request(&mut world, &mut session, Request::Snapshot);
        assert!(!authority_vessel(&off).powered);
        world
            .exterior_set_enabled(&mut session, true, accepted_definition.revision)
            .unwrap();
        let idle = authority_request(&mut world, &mut session, Request::Snapshot);
        assert!(authority_vessel(&idle).powered);
        assert_eq!(authority_vessel(&idle).drive, ExteriorDriveState::Idle);
        assert_eq!(off.server_time_ms, idle.server_time_ms);

        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, 1814, 960, 640).unwrap();
        let render = |renderer: &mut Renderer, frame: &Frame| {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 960, 640);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            renderer.paint(frame, Some(target.framebuffer));
            pixels(&gl)
        };
        let front = AuthorityCaptureCamera {
            name: "front-quarter",
            eye_m: [95., 50., -95.],
            target_m: [0.; 3],
            up: [0., 1., 0.],
            half_width_m: 54.,
        };
        let rear = AuthorityCaptureCamera {
            name: "rear",
            eye_m: [-120., 15., 0.],
            target_m: [0.; 3],
            up: [0., 1., 0.],
            half_width_m: 30.,
        };
        let mut captures = Vec::new();
        let mut capture = |name: &str, camera: AuthorityCaptureCamera, snapshot: &Snapshot| {
            let frame = camera.frame(snapshot);
            let image = render(&mut renderer, &frame);
            save(&directory, &format!("{name}.png"), &image);
            captures.push(serde_json::json!({"file":format!("{name}.png"),
                "camera":camera.receipt(&frame),"authority":authority_public_receipt(snapshot)}));
            image
        };
        let off_pixels = capture("pioneer-authority-exterior-off", front, &off);
        let idle_pixels = capture("pioneer-authority-idle-front-quarter", front, &idle);
        assert!(
            changed(&off_pixels, &idle_pixels) > 8,
            "accepted exterior circuit must change visible pixels"
        );
        let bridge_close = AuthorityCaptureCamera {
            name: "bridge-quarter",
            eye_m: [65., 12., -38.],
            target_m: [33.5, 0.8, 0.],
            up: [0., 1., 0.],
            half_width_m: 18.,
        };
        // Frozen accepted OFF/IDLE snapshots share clock, pose and camera.
        // These views expose opaque glazing depth without changing its output.
        let bridge_off_pixels = capture("pioneer-authority-off-bridge-quarter", bridge_close, &off);
        let bridge_idle_pixels =
            capture("pioneer-authority-idle-bridge-quarter", bridge_close, &idle);
        for camera in [
            AuthorityCaptureCamera {
                name: "bridge-port-pane",
                eye_m: [60., 5., -18.],
                target_m: [35.8, 1.275, -3.555],
                up: [0., 1., 0.],
                half_width_m: 7.,
            },
            AuthorityCaptureCamera {
                name: "aft-roof",
                eye_m: [-62., 35., -48.],
                target_m: [-20., 4., 0.],
                up: [0., 1., 0.],
                half_width_m: 23.,
            },
            AuthorityCaptureCamera {
                name: "port-nozzle",
                eye_m: [-68., 5., -12.5],
                target_m: [-35.9, 0., -12.5],
                up: [0., 1., 0.],
                half_width_m: 9.,
            },
            AuthorityCaptureCamera {
                name: "side",
                eye_m: [0., 20., -120.],
                target_m: [0.; 3],
                up: [0., 1., 0.],
                half_width_m: 45.,
            },
            AuthorityCaptureCamera {
                name: "top",
                eye_m: [0., 120., 0.],
                target_m: [0.; 3],
                up: [0., 0., 1.],
                half_width_m: 45.,
            },
            AuthorityCaptureCamera {
                name: "front",
                eye_m: [120., 0., 0.],
                target_m: [0.; 3],
                up: [0., 1., 0.],
                half_width_m: 30.,
            },
            AuthorityCaptureCamera {
                name: "bottom",
                eye_m: [0., -120., 0.],
                target_m: [0.; 3],
                up: [0., 0., -1.],
                half_width_m: 45.,
            },
            AuthorityCaptureCamera {
                name: "rear",
                eye_m: [-120., 15., 0.],
                target_m: [0.; 3],
                up: [0., 1., 0.],
                half_width_m: 30.,
            },
        ] {
            capture(
                &format!("pioneer-authority-idle-{}", camera.name),
                camera,
                &idle,
            );
        }

        // Find an ON interval using only genuine fixed ticks. Neither phase nor
        // period is edited to arrange the evidence.
        let strobe = accepted_definition
            .document
            .fixtures
            .iter()
            .filter(|f| f.kind == ExteriorFixtureKind::Strobe)
            .max_by(|a, b| a.position_m[1].total_cmp(&b.position_m[1]))
            .unwrap();
        let pulse = strobe.pulse.unwrap();
        assert_eq!(pulse.period_ms, 1000);
        assert_eq!(pulse.on_ms, 100);
        let mut strobe_on = idle.clone();
        for _ in 0..20 {
            if strobe.output(authority_vessel(&strobe_on), strobe_on.server_time_ms) > 0. {
                break;
            }
            world.step(50).unwrap();
            strobe_on = authority_request(&mut world, &mut session, Request::Snapshot);
        }
        assert_eq!(
            strobe.output(authority_vessel(&strobe_on), strobe_on.server_time_ms),
            1.
        );
        let strobe_target = strobe.position_m.map(|v| v as f32);
        let strobe_camera = AuthorityCaptureCamera {
            name: "dorsal-strobe-closeup",
            eye_m: [
                strobe_target[0] + 8.,
                strobe_target[1] + 12.,
                strobe_target[2] - 10.,
            ],
            target_m: strobe_target,
            up: [0., 1., 0.],
            half_width_m: 3.,
        };
        let on_pixels = capture("pioneer-authority-strobe-on", strobe_camera, &strobe_on);
        world.play();
        world.pause();
        assert!(world.is_paused());
        world.advance(250).unwrap();
        let paused = authority_request(&mut world, &mut session, Request::Snapshot);
        assert_eq!(paused.server_time_ms, strobe_on.server_time_ms);
        assert_eq!(paused.exterior, strobe_on.exterior);
        let paused_pixels = capture("pioneer-authority-strobe-paused", strobe_camera, &paused);
        assert_eq!(
            on_pixels, paused_pixels,
            "actual paused authority must freeze rendered phase"
        );
        let mut steps = Vec::new();
        let mut first_off = None;
        let mut cycle_pixels = None;
        for index in 1..=20 {
            let previous = world.time();
            world.step(50).unwrap();
            assert_eq!(world.time(), previous + 50);
            let stepped = authority_request(&mut world, &mut session, Request::Snapshot);
            let output = strobe.output(authority_vessel(&stepped), stepped.server_time_ms);
            steps.push(serde_json::json!({"step":index,"delta_ms":50,
                "simulation_time_ms":stepped.server_time_ms,"strobe_output":output}));
            if index == 1 {
                capture(
                    "pioneer-authority-strobe-step-50ms",
                    strobe_camera,
                    &stepped,
                );
            }
            if output == 0. && first_off.is_none() {
                let image = capture("pioneer-authority-strobe-off", strobe_camera, &stepped);
                first_off = Some((stepped.server_time_ms, image));
            }
            if index == 20 {
                assert_eq!(stepped.server_time_ms, strobe_on.server_time_ms + 1000);
                assert_eq!(output, 1.);
                cycle_pixels = Some(capture(
                    "pioneer-authority-strobe-next-cycle",
                    strobe_camera,
                    &stepped,
                ));
            }
        }
        let (strobe_off_time, off_strobe_pixels) = first_off.unwrap();
        let strobe_changed = changed(&on_pixels, &off_strobe_pixels);
        assert!(
            strobe_changed > 8,
            "unchanged default strobe must visibly switch on real tick boundaries"
        );
        assert_eq!(
            on_pixels,
            cycle_pixels.unwrap(),
            "default phase repeats after twenty real 50ms steps"
        );

        let pre_thrust_idle = authority_request(
            &mut world,
            &mut session,
            Request::PilotInput {
                translation: [0.; 3],
                rotation: [0.; 3],
            },
        );
        assert_eq!(
            authority_vessel(&pre_thrust_idle).drive,
            ExteriorDriveState::Idle
        );
        let pre_thrust_pixels =
            capture("pioneer-authority-pre-thrust-idle", rear, &pre_thrust_idle);
        let commanded = authority_request(
            &mut world,
            &mut session,
            Request::PilotInput {
                translation: [1., 0., 0.],
                rotation: [0.; 3],
            },
        );
        assert_eq!(pre_thrust_idle.server_time_ms, commanded.server_time_ms);
        assert_eq!(
            authority_pilot(&pre_thrust_idle)
                .flight
                .as_ref()
                .unwrap()
                .position,
            authority_pilot(&commanded)
                .flight
                .as_ref()
                .unwrap()
                .position
        );
        assert_eq!(
            authority_vessel(&commanded).drive,
            ExteriorDriveState::Active
        );
        let commanded_pixels = capture(
            "pioneer-authority-thrust-command-accepted",
            rear,
            &commanded,
        );
        let same_time_drive_pixels = changed(&pre_thrust_pixels, &commanded_pixels);
        assert!(same_time_drive_pixels > 8,
            "same-time accepted drive telemetry must change visible pixels without a strobe phase change");
        let before_thrust = world.time();
        world.step(50).unwrap();
        let active = authority_request(&mut world, &mut session, Request::Snapshot);
        assert_eq!(active.server_time_ms, before_thrust + 50);
        assert_eq!(authority_vessel(&active).drive, ExteriorDriveState::Active);
        let active_flight = authority_pilot(&active).flight.as_ref().unwrap();
        assert!(
            active_flight
                .acceleration
                .iter()
                .zip(active_flight.heading)
                .map(|(a, h)| a * h)
                .sum::<f64>()
                > 0.,
            "accepted body-forward input must integrate real forward acceleration"
        );
        let active_pixels = capture("pioneer-authority-active-front-quarter", front, &active);
        assert!(
            changed(&idle_pixels, &active_pixels) > 8,
            "accepted thrust must change actual engine presentation"
        );
        capture(
            "pioneer-authority-active-rear",
            AuthorityCaptureCamera {
                name: "rear",
                eye_m: [-120., 15., 0.],
                target_m: [0.; 3],
                up: [0., 1., 0.],
                half_width_m: 30.,
            },
            &active,
        );
        // All camera captures are complete; subsequent checks use the actual
        // renderer's gameplay turret cache and material admission path.
        assert_eq!(
            renderer.turrets.len(),
            2,
            "existing gameplay turrets must be present"
        );

        let mut graph = arkline_protocol::material_graph::MaterialGraphDocument::pioneer_default();
        graph.binding.asset_sha256 = previous_hash.clone();
        renderer
            .spatial_material
            .queue(Some(Arc::new(MaterialRevision::new(1, 1, &graph).unwrap())))
            .unwrap();
        renderer.prepare_spatial_material();
        let rejected = renderer.spatial_material.status().clone();
        assert_eq!(
            rejected.phase,
            crate::spatial_material::MaterialProgramPhase::Rejected
        );
        assert!(rejected
            .diagnostics
            .iter()
            .any(|d| d.phase == MaterialProgramErrorPhase::Binding));
        assert!(renderer.spatial_material.applied().is_none());
        let rejected_pixels = render(&mut renderer, &front.frame(&active));
        assert_eq!(
            active_pixels, rejected_pixels,
            "rejected historical binding must leave authored hull pixels unchanged"
        );
        save(
            &directory,
            "pioneer-authority-old-hash-rejected.png",
            &rejected_pixels,
        );
        graph.binding = arkline_protocol::material_graph::AssetMaterialBinding::pioneer();
        let authored_surface = graph
            .nodes
            .iter()
            .find(|node| {
                matches!(
                    node.op,
                    arkline_protocol::material_graph::MaterialNodeOp::AuthoredSurface
                )
            })
            .unwrap()
            .id;
        for node in &mut graph.nodes {
            if matches!(
                node.op,
                arkline_protocol::material_graph::MaterialNodeOp::SurfaceOutput { .. }
            ) {
                node.op = arkline_protocol::material_graph::MaterialNodeOp::SurfaceOutput {
                    surface: authored_surface,
                };
            }
        }
        renderer
            .spatial_material
            .queue(Some(Arc::new(MaterialRevision::new(1, 2, &graph).unwrap())))
            .unwrap();
        let recovery_frame = front.frame(&active);
        let recovered_pixels = render(&mut renderer, &recovery_frame);
        assert_eq!(
            renderer.spatial_material.status().phase,
            crate::spatial_material::MaterialProgramPhase::Applied
        );
        assert_eq!(
            renderer
                .spatial_material
                .applied()
                .unwrap()
                .binding()
                .asset_sha256,
            pioneer_asset_sha256()
        );
        save(
            &directory,
            "pioneer-authority-current-hash-graph-recovery.png",
            &recovered_pixels,
        );
        if let Some(directory) = directory {
            std::fs::write(directory.join("authority-receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
                "build":include_str!("../../../BUILD_VERSION").trim(),"asset_sha256":pioneer_asset_sha256(),
                "reviewed_expected_asset_sha256":expected_hash,
                "source_commit":std::env::var("ARKLINE_CAPTURE_SOURCE_COMMIT").ok(),
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),
                "gl_version":gl.get_parameter_string(glow::VERSION),"viewport":[960,640],
                "authority":"disposable arkline_server::editor::EditorWorld", "fixture_definition_modified":false,
                "review_attitude":{"method":"accepted Belt warp and yaw PilotInput with real fixed steps; existing sunlight and shadows unchanged","warp_started_ms":warp_started,"warp_arrival_ms":arrival,"yaw_steps":yaw_steps,"step_ms":50,"zero_input_settle_ms":50},
                "omitted_snapshot_scene":["traffic","asteroids","gates","surface_props","other_pilots"],
                "captures":captures,"off_idle_changed_pixels":changed(&off_pixels,&idle_pixels),
                "close_bridge_off_idle_changed_pixels":changed(&bridge_off_pixels,&bridge_idle_pixels),
                "idle_active_changed_pixels":same_time_drive_pixels,
                "integrated_active_vs_initial_idle_changed_pixels":changed(&idle_pixels,&active_pixels),
                "drive_comparison_same_simulation_time_ms":commanded.server_time_ms,"drive_comparison_camera":"rear",
                "dynamic_turret_count":renderer.turrets.len(),
                "strobe":{"id":strobe.id,"pulse":pulse,"on_time_ms":strobe_on.server_time_ms,
                    "off_time_ms":strobe_off_time,"changed_pixels":strobe_changed,"paused_pixels_equal":true,
                    "full_cycle_pixels_equal":true,"steps":steps},
                "graph_binding":{"old_asset_sha256":previous_hash,"old_rejected":true,
                    "old_error_phase":"binding","rejection_pixels_unchanged":true,"recovery_asset_sha256":pioneer_asset_sha256(),"recovery_applied":true,"recovery_graph":"authored_surface_identity"},
                "limitations":["actual authority plus native engine EGL capture; no OS window or TCP journey",
                    "camera isolates the accepted pilot; unrelated snapshot objects are omitted",
                    "no fixture size, intensity, period or phase changes; no automatic source-part retargeting"]
            })).unwrap()).unwrap();
        }
        target.destroy(&gl);
        renderer.destroy();
    }
}
