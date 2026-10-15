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
        exterior: Some(Arc::new(exterior())),
        lighting_quality: super::super::lighting_settings::LightingQuality::High,
        cockpit_vp: vp,
        snapshot_time: 1000,
        planet_vp: vp,
        system_id: 3,
        ship_class: ShipClass::Pioneer,
        drone_camera: false,
        camera_drone: None,
        scene: Arc::new(FrameScene {
            traffic: vec![],
            surface_props: vec![arkline_protocol::SurfaceProp {
                id: 1,
                owner: "ExteriorQA".into(),
                system_id: 3,
                kind: arkline_protocol::SurfacePropKind::Crate,
                position: [-42., -3.5, -12.5],
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
        for fixture in &mut state(&mut frame).definition.document.fixtures {
            fixture.light_intensity = 0.;
        }
        let unlit = render(&mut renderer, &frame);
        save(&directory, "pioneer-exterior-emission-only.png", &unlit);
        let illumination_pixels = changed(&lit, &unlit);
        assert!(
            illumination_pixels > 32,
            "powered fixtures must illuminate non-emissive receiving surfaces"
        );
        // A paused cached feed must accept new state without requiring clock advancement.
        let info = egui::PaintCallbackInfo {
            viewport: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960., 640.)),
            clip_rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(960., 640.)),
            pixels_per_point: 1.,
            screen_size_px: [960, 640],
        };
        renderer.paint_cached_feed(&frame, 813, info, 1, Some(target.framebuffer));
        let feed_before = pixels(&gl);
        {
            let vessel = &mut state(&mut frame).vessels[0];
            vessel.enabled = false;
            vessel.powered = false;
            vessel.drive = ExteriorDriveState::Off;
        }
        renderer.paint_cached_feed(&frame, 813, info, 1, Some(target.framebuffer));
        let feed_after = pixels(&gl);
        assert!(
            changed(&feed_before, &feed_after) > 16,
            "paused state changes must invalidate cached feeds"
        );
        // A second renderer consumes the same observer snapshot and matches exactly.
        let mut observer = Renderer::new(gl.clone()).unwrap();
        let observer_pixels = render(&mut observer, &frame);
        assert_eq!(render(&mut renderer, &frame), observer_pixels);
        if let Some(directory) = directory {
            std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
                "build":include_str!("../../../BUILD_VERSION").trim(),"asset_sha256":pioneer_asset_sha256(),
                "vendor":gl.get_parameter_string(glow::VENDOR),"renderer":gl.get_parameter_string(glow::RENDERER),
                "gl_version":gl.get_parameter_string(glow::VERSION),"viewport":[960,640],
                "off_idle_changed_pixels":changed(&off,&idle),"idle_active_changed_pixels":changed(&idle,&active),
                "illumination_changed_pixels":illumination_pixels,"dynamic_turret_count":renderer.turrets.len(),
                "limitations":["actual engine offscreen EGL capture; no OS interaction or TCP two-client journey", "diagnostic receiver crate is an authoritative scene fixture", "no model adoption or release publication"]
            })).unwrap()).unwrap();
        }
        observer.destroy();
        target.destroy(&gl);
        renderer.destroy();
    }
}
