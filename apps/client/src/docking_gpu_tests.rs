//! Opt-in evidence: actual authority snapshots rendered by the production GPU path.
use super::*;
use arkline_protocol::{DockingPhase, Request, SceneLayoutRevision};
use arkline_server::editor::EditorWorld;

fn request(w: &mut EditorWorld, session: &mut Option<String>, r: Request) -> Snapshot {
    let response = w.handle(session, r);
    assert!(response.ok, "{}", response.message);
    response.snapshot.unwrap()
}
fn pilot(snapshot: &Snapshot) -> &PilotView {
    snapshot
        .pilots
        .iter()
        .find(|p| p.name == "DockingCapture")
        .unwrap()
}
fn frame(snapshot: &Snapshot, origin: [f64; 3], cockpit: bool) -> Frame {
    frame_for_pilot(snapshot, origin, cockpit, "DockingCapture")
}
fn frame_for_pilot(snapshot: &Snapshot, origin: [f64; 3], cockpit: bool, name: &str) -> Frame {
    let p = snapshot.pilots.iter().find(|p| p.name == name).unwrap();
    let now = snapshot.server_time_ms;
    let rotation = ship_orientation_at(p, now);
    let space = Space { origin };
    let (eye, look) = if cockpit {
        let eye = space.position(p, now)
            + rotation * cockpit_fit::eye_body_meters(ShipClass::Pioneer) / 100.;
        (eye, eye + rotation * Vec3::X)
    } else {
        (Vec3::new(0.8, 1.2, 3.0), Vec3::ZERO)
    };
    let view = Mat4::look_at_rh(eye, look, rotation * Vec3::Y);
    let projection = |near, far| {
        Mat4::perspective_rh_gl(
            if cockpit {
                cockpit_fov(ShipClass::Pioneer)
            } else {
                58_f32.to_radians()
            },
            1.5,
            near,
            far,
        ) * view
    };
    Frame {
        pioneer_parts: hangar::pioneer_docked_scene::default_body_parts().ok(),
        exterior: snapshot.exterior.clone().map(Arc::new),
        lighting_quality: super::super::lighting_settings::LightingQuality::High,
        cockpit_vp: projection(0.0002, 10.),
        cockpit_occlusion: None,
        snapshot_time: now,
        paused: false,
        planet_vp: projection(1., 300_000.),
        system_id: p.system_id,
        ship_class: p.ship_class,
        drone_camera: false,
        camera_drone: None,
        scene: Arc::new(FrameScene {
            physical: None,
            carrier_inventory_registration: snapshot.carrier_inventory_registration.clone(),
            world_layout: super::super::snapshot_world_layout(snapshot)
                .unwrap()
                .clone(),
            traffic: snapshot
                .traffic
                .iter()
                .filter(|x| x.system_id == p.system_id)
                .cloned()
                .collect(),
            surface_props: snapshot
                .surface_props
                .iter()
                .filter(|x| x.system_id == p.system_id)
                .cloned()
                .collect(),
            asteroids: snapshot
                .asteroids
                .iter()
                .filter(|x| x.system_id == p.system_id)
                .cloned()
                .collect(),
            gates: gates()
                .into_iter()
                .filter(|x| x.system_id == p.system_id)
                .collect(),
            pilots: snapshot.pilots.clone(),
        }),
        space,
        cockpit,
        rotation,
        connected: true,
        vp: projection(0.03, 1200.),
        eye,
        now,
        own: p.name.clone(),
        docking: None,
    }
}
unsafe fn capture(
    gl: &glow::Context,
    renderer: &mut Renderer,
    target: &CachedFeed,
    frame: &Frame,
    path: &std::path::Path,
) {
    gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
    gl.viewport(0, 0, 960, 640);
    gl.disable(glow::SCISSOR_TEST);
    gl.color_mask(true, true, true, true);
    renderer.paint(frame, Some(target.framebuffer));
    let mut pixels = vec![0u8; 960 * 640 * 4];
    gl.read_pixels(
        0,
        0,
        960,
        640,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut pixels)),
    );
    assert_eq!(gl.get_error(), glow::NO_ERROR);
    let flipped: Vec<_> = pixels
        .chunks_exact(960 * 4)
        .rev()
        .flat_map(|r| r.iter().copied())
        .collect();
    image::save_buffer(path, &flipped, 960, 640, image::ColorType::Rgba8).unwrap();
}
struct CockpitCapture {
    cockpit: crate::Cockpit,
    ctx: egui::Context,
    painter: egui_glow::Painter,
    updates: std::sync::mpsc::Sender<crate::Update>,
    _requests: std::sync::mpsc::Receiver<Request>,
    index: u64,
    freeze_presentation: bool,
}
impl CockpitCapture {
    unsafe fn new(gl: Arc<glow::Context>, response: arkline_protocol::Response) -> Self {
        Self::new_for_pilot(gl, response, "DockingCapture")
    }
    unsafe fn new_for_pilot(
        gl: Arc<glow::Context>,
        response: arkline_protocol::Response,
        name: &str,
    ) -> Self {
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.animation_time = 0.);
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.flight_view = FlightView::new(Some(&gl));
        cockpit.hangar_view = hangar::HangarView::new(Some(&gl));
        cockpit.pilot = name.into();
        cockpit.snapshot = None;
        cockpit.assets = None;
        cockpit.embedded = true;
        cockpit.embedded_input = true;
        cockpit.embedded_paused = false;
        cockpit.focus_on_first_frame = false;
        cockpit.identity_collapsed = true;
        cockpit.view_mode = crate::ViewMode::Cockpit;
        cockpit.services_open = false;
        cockpit.overview_open = false;
        cockpit.drone_camera = false;
        cockpit.selected_object = crate::SelectedObject::Station;
        cockpit.sound_settings.muted = true;
        let (commands, requests) = std::sync::mpsc::channel();
        cockpit.commands = Some(commands);
        let (updates, receiver) = std::sync::mpsc::channel();
        cockpit.updates = Some(receiver);
        updates
            .send(crate::Update::Response(Box::new(response), true))
            .unwrap();
        cockpit.poll();
        Self {
            cockpit,
            ctx,
            painter: egui_glow::Painter::new(gl, "", None, false).unwrap(),
            updates,
            _requests: requests,
            index: 0,
            freeze_presentation: true,
        }
    }
    fn camera_receipt(&self) -> serde_json::Value {
        let station = self.cockpit.in_station_hangar();
        let camera = if station {
            self.cockpit.hangar_view.editor_camera_pose()
        } else {
            self.cockpit.flight_view.editor_camera_pose()
        };
        serde_json::json!({"route":if station {"registered station"} else {"world-space flight"},"services_view_open":self.cockpit.services_open,
            "camera_pose":camera.map(|(eye,yaw,pitch)|serde_json::json!({"eye":eye.to_array(),"yaw_rad":yaw,"pitch_rad":pitch})),
            "camera_units":if station {"registered station metres"} else {"floating origin scene units, 100 metres per unit"}})
    }
    unsafe fn capture(
        &mut self,
        gl: &glow::Context,
        target: &CachedFeed,
        response: arkline_protocol::Response,
        path: &std::path::Path,
    ) {
        self.capture_input(gl, target, response, path, None);
    }
    /// Explicit UI proof uses real input; the original motion wrapper retains its
    /// stricter no-workspace replacement invariant.
    unsafe fn capture_input(
        &mut self,
        gl: &glow::Context,
        target: &CachedFeed,
        response: arkline_protocol::Response,
        path: &std::path::Path,
        input: Option<Vec<egui::Event>>,
    ) -> Vec<(String, egui::Pos2)> {
        let explicit_ui = input.is_some();
        let mut input = input.unwrap_or_default();
        let mut labels = Vec::new();
        let width = target.width;
        let height = target.height;
        assert!(response.ok);
        let now = response.snapshot.as_ref().unwrap().server_time_ms;
        self.updates
            .send(crate::Update::Response(Box::new(response), false))
            .unwrap();
        self.cockpit.poll();
        self.cockpit.frozen_time = self.freeze_presentation.then_some(now);
        self.cockpit.timeline_origin = Some(now);
        self.cockpit.observed = crate::Instant::now();
        let authority = self.cockpit.snapshot.clone();
        for _ in 0..if self.index == 0 && !explicit_ui {
            3
        } else {
            1
        } {
            self.index += 1;
            let output = self.ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width as f32, height as f32),
                    )),
                    time: Some(if self.freeze_presentation {
                        self.index as f64 / 10.
                    } else {
                        self.index as f64 / 60.
                    }),
                    focused: true,
                    events: std::mem::take(&mut input),
                    ..Default::default()
                },
                |ctx| {
                    eframe::App::update(&mut self.cockpit, ctx, &mut eframe::Frame::_new_kittest())
                },
            );
            assert_eq!(
                serde_json::to_value(&self.cockpit.snapshot).unwrap(),
                serde_json::to_value(&authority).unwrap(),
                "painting cannot edit authority"
            );
            if !explicit_ui
                && authority
                    .as_ref()
                    .unwrap()
                    .world_layout
                    .as_ref()
                    .unwrap()
                    .revision
                    == SceneLayoutRevision::DockingV1
            {
                let walking = pilot(authority.as_ref().unwrap())
                    .station_avatar
                    .as_ref()
                    .is_some_and(|a| a.docked_traversal.is_some());
                assert_eq!(
                    self.cockpit.in_station_hangar(), walking,
                    "seated docking keeps flight route; authoritative v2 walking uses registered route"
                );
                assert!(
                    !self.cockpit.services_open,
                    "capture must not open a replacement services view"
                );
            }
            labels = output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        let point = t.pos + t.galley.rect.center().to_vec2();
                        s.clip_rect
                            .contains_rect(t.visual_bounding_rect())
                            .then(|| (t.galley.job.text.clone(), point))
                    } else {
                        None
                    }
                })
                .collect();
            let primitives = self.ctx.tessellate(output.shapes, output.pixels_per_point);
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, width, height);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            gl.depth_mask(true);
            gl.clear_depth_f32(1.);
            gl.clear_color(0., 0., 0., 1.);
            gl.clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
            self.painter.paint_and_update_textures(
                [width as u32, height as u32],
                output.pixels_per_point,
                &primitives,
                &output.textures_delta,
            );
        }
        let mut pixels = vec![0u8; (width * height * 4) as usize];
        gl.read_pixels(
            0,
            0,
            width,
            height,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut pixels)),
        );
        assert_eq!(gl.get_error(), glow::NO_ERROR);
        let flipped: Vec<_> = pixels
            .chunks_exact(width as usize * 4)
            .rev()
            .flat_map(|r| r.iter().copied())
            .collect();
        image::save_buffer(
            path,
            &flipped,
            width as u32,
            height as u32,
            image::ColorType::Rgba8,
        )
        .unwrap();
        labels
    }
}
#[test]
#[ignore = "Explicit production Renderer EGL docking sequence; requires capture directory"]
fn native_authoritative_docking_motion_capture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_DOCKING_CAPTURE_DIR").expect("capture destination"),
    );
    std::fs::create_dir_all(&root).unwrap();
    // Optional smoke capture is not accepted as a completed motion sequence.
    let limit = std::env::var("ARKLINE_DOCKING_CAPTURE_LIMIT")
        .ok()
        .map(|v| v.parse::<usize>().unwrap());
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, 1977, 960, 640).unwrap();
        let mut receipt = vec![];
        for (label, revision, count) in [
            ("after", SceneLayoutRevision::DockingV1, 461),
            ("before-retained-legacy", SceneLayoutRevision::OrbitalV1, 31),
        ] {
            let dir = root.join(label);
            std::fs::create_dir_all(&dir).unwrap();
            let mut world =
                EditorWorld::open_with_creation_scene(dir.join("world"), revision).unwrap();
            let mut session = None;
            let response = world.handle(
                &mut session,
                Request::Connect {
                    protocol_version: arkline_protocol::PROTOCOL_VERSION,
                    world_layout_version: arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
                    supports_room_controls: true,
                    supports_fleet_hangar: true,
                    supports_pioneer_interior: true,
                    supports_docked_traversal_v2: true,
                    pilot: "DockingCapture".into(),
                    resume_token: None,
                },
            );
            assert!(response.ok, "{}", response.message);
            let first = response.snapshot.clone().unwrap();
            let mut cockpit_capture = CockpitCapture::new(gl.clone(), response);
            let mut origin = pilot(&first).flight.as_ref().unwrap().position;
            if label == "after" {
                origin[0] += 125.;
            }
            // The legacy framing is deliberately wide: its command changes the actual
            // world pose by over a kilometre in one tick, not by an animated camera.
            let legacy_origin = [arkline_protocol::STATION_POSITION[0] + 500., 0., 500.];
            for index in 0..limit.unwrap_or(count).min(count) {
                let command = match (label, index) {
                    (_, 10) => Some(Request::Undock),
                    ("after", 180) => Some(Request::PilotInput {
                        translation: [0.; 3],
                        rotation: [0., 1., 0.],
                    }),
                    ("after", 185) => Some(Request::PilotInput {
                        translation: [0.; 3],
                        rotation: [0.; 3],
                    }),
                    ("after", 190) => Some(Request::Dock),
                    ("before-retained-legacy", 20) => Some(Request::Dock),
                    _ => None,
                };
                let snapshot = if let Some(command) = command {
                    request(&mut world, &mut session, command)
                } else {
                    request(&mut world, &mut session, Request::Snapshot)
                };
                let p = pilot(&snapshot);
                if label == "after" && index == 460 {
                    assert_eq!(
                        p.flight.as_ref().unwrap().docking.as_ref().unwrap().phase,
                        DockingPhase::Secured
                    );
                }
                for (camera, cockpit) in [("exterior", false), ("cockpit", true)] {
                    let mut f = frame(
                        &snapshot,
                        if cockpit {
                            p.flight.as_ref().unwrap().position
                        } else if label == "after" {
                            origin
                        } else {
                            legacy_origin
                        },
                        cockpit,
                    );
                    if label != "after" && !cockpit {
                        f.eye = Vec3::new(1., 7., 18.);
                        f.vp = Mat4::perspective_rh_gl(58_f32.to_radians(), 1.5, 0.03, 1200.)
                            * Mat4::look_at_rh(f.eye, Vec3::ZERO, Vec3::Y);
                        f.planet_vp = f.vp;
                        f.cockpit_vp = f.vp;
                    }
                    let file = format!("{camera}-{index:04}.png");
                    if cockpit {
                        let response = world.handle(&mut session, Request::Snapshot);
                        cockpit_capture.capture(&gl, &target, response, &dir.join(&file));
                    } else {
                        capture(&gl, &mut renderer, &target, &f, &dir.join(&file));
                    }
                    receipt.push(serde_json::json!({"realm":label,"file":file,"camera":camera,
                        "index":index,"simulation_time_ms":snapshot.server_time_ms,"location":p.location,
                        "flight":p.flight,"camera_receipt":if cockpit {cockpit_capture.camera_receipt()} else {serde_json::json!({"view_projection":f.vp.to_cols_array(),"eye_scene":f.eye.to_array(),"origin_world_m":f.space.origin})}}));
                }
                world.step(100).unwrap();
            }
        }
        std::fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "cadence_fps":10,"size":[960,640],"actual_engine":"Production Cockpit update + egui_glow callbacks for seated view; Renderer::paint for exterior; Mesa EGL; EditorWorld fixed ticks",
            "baseline":"Retained unchanged OrbitalV1 immediate Dock/Undock branch, not an exact baseline binary",
            "source":std::env::var("ARKLINE_DOCKING_CAPTURE_SOURCE").unwrap_or_else(|_|"unrecorded".into()),
            "complete":limit.is_none(),"frames":receipt
        })).unwrap()).unwrap();
        target.destroy(&gl);
        renderer.destroy();
    }
}

#[test]
#[ignore = "Explicit integrated authority walk/seat/docking EGL proof; requires fresh destination"]
fn native_integrated_docking_walk_capture() {
    use arkline_protocol::{
        DockedTraversalBinding, DockedTraversalRequest, DOCKED_SEAT_TRANSITION_MS,
    };
    use sha2::{Digest, Sha256};
    let root = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_DOCKING_WALK_CAPTURE_DIR").expect("fresh capture destination"),
    );
    assert!(
        !root.join("world").exists(),
        "capture requires a fresh disposable authority"
    );
    std::fs::create_dir_all(&root).unwrap();
    let mut world =
        EditorWorld::open_with_creation_scene(root.join("world"), SceneLayoutRevision::DockingV1)
            .unwrap();
    let mut session = None;
    let response = world.handle(
        &mut session,
        Request::Connect {
            protocol_version: arkline_protocol::PROTOCOL_VERSION,
            world_layout_version: arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
            supports_room_controls: true,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
            supports_docked_traversal_v2: true,
            pilot: "DockingCapture".into(),
            resume_token: None,
        },
    );
    assert!(response.ok, "{}", response.message);
    let first = response.snapshot.as_ref().unwrap();
    let host = pilot(first)
        .flight
        .as_ref()
        .unwrap()
        .docking
        .as_ref()
        .unwrap()
        .host_vessel_id
        .clone();
    let binding = arkline_protocol::docked_traversal_binding(
        host,
        &arkline_protocol::default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1.definition().fleet_layout,
        false,
    );
    let leave = |binding: DockedTraversalBinding| Request::DockedTraversal {
        request: DockedTraversalRequest::LeaveSeat { binding },
    };
    let take = |binding: DockedTraversalBinding| Request::DockedTraversal {
        request: DockedTraversalRequest::TakeSeat { binding },
    };
    let walk = |forward| Request::DockedTraversal {
        request: DockedTraversalRequest::Walk {
            binding: binding.clone(),
            forward,
            strafe: 0.,
            turn: 0.,
            sprint: false,
        },
    };
    let step = |world: &mut EditorWorld, ms: u64| {
        assert_eq!(ms % 50, 0);
        for _ in 0..ms / 50 {
            world.step(50).unwrap();
        }
    };
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let target = CachedFeed::create(&gl, 1978, 960, 640).unwrap();
        let mut capture = CockpitCapture::new(gl.clone(), response);
        capture.cockpit.hangar_view.on_foot_view = hangar::OnFootView::FirstPerson;
        let mut receipts = Vec::new();
        let mut record = |label: &str, world: &mut EditorWorld, session: &mut Option<String>| {
            let response = world.handle(session, Request::Snapshot);
            assert!(response.ok, "{}", response.message);
            let snapshot = response.snapshot.as_ref().unwrap().clone();
            let p = pilot(&snapshot);
            let file = format!("{label}.png");
            let snapshot_file = format!("{label}-snapshot.json");
            let bytes = serde_json::to_vec_pretty(&snapshot).unwrap();
            std::fs::write(root.join(&snapshot_file), &bytes).unwrap();
            capture.capture(&gl, &target, response, &root.join(&file));
            let rgba = image::open(root.join(&file)).unwrap().to_rgba8();
            receipts.push(serde_json::json!({
                "file":file, "snapshot_file":snapshot_file,
                "snapshot_file_sha256":hex::encode(Sha256::digest(&bytes)),
                "rgba_sha256":hex::encode(Sha256::digest(rgba.as_raw())),
                "simulation_time_ms":snapshot.server_time_ms,
                "flight":p.flight,"location":p.location,"station_avatar":p.station_avatar,
                "camera_receipt":capture.camera_receipt(),
            }));
        };
        record("01-secured-seated", &mut world, &mut session);
        request(&mut world, &mut session, leave(binding.clone()));
        assert!(
            !world.handle(&mut session, take(binding.clone())).ok,
            "cannot reverse unfinished seat exit"
        );
        step(&mut world, DOCKED_SEAT_TRANSITION_MS / 2);
        record("02-leaving-seat", &mut world, &mut session);
        step(&mut world, DOCKED_SEAT_TRANSITION_MS / 2);
        record("03-standing", &mut world, &mut session);
        let standing = request(&mut world, &mut session, Request::Snapshot);
        let start = pilot(&standing)
            .station_avatar
            .as_ref()
            .unwrap()
            .docked_traversal
            .as_ref()
            .unwrap()
            .feet_m;
        request(&mut world, &mut session, walk(-1.));
        step(&mut world, 100);
        let walked = request(&mut world, &mut session, walk(0.));
        let feet = pilot(&walked)
            .station_avatar
            .as_ref()
            .unwrap()
            .docked_traversal
            .as_ref()
            .unwrap()
            .feet_m;
        let distance = feet
            .iter()
            .zip(start)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        assert!(
            distance > 0.01 && distance < 0.5,
            "actual accepted walk {distance}m"
        );
        record("04-walked", &mut world, &mut session);
        request(&mut world, &mut session, take(binding.clone()));
        step(&mut world, DOCKED_SEAT_TRANSITION_MS / 2);
        record("05-reseating", &mut world, &mut session);
        step(&mut world, DOCKED_SEAT_TRANSITION_MS / 2);
        record("06-reseated", &mut world, &mut session);
        request(&mut world, &mut session, Request::Undock);
        step(&mut world, 4000);
        assert!(
            !world.handle(&mut session, leave(binding.clone())).ok,
            "manual docking motion does not grant routine-only walking"
        );
        record("07-departing", &mut world, &mut session);
        step(&mut world, 13000);
        request(&mut world, &mut session, Request::Dock);
        step(&mut world, 26000);
        let returned = request(&mut world, &mut session, Request::Snapshot);
        assert!(pilot(&returned)
            .flight
            .as_ref()
            .unwrap()
            .docking
            .as_ref()
            .unwrap()
            .secured());
        request(&mut world, &mut session, leave(binding.clone()));
        step(&mut world, DOCKED_SEAT_TRANSITION_MS);
        record("08-recaptured-standing", &mut world, &mut session);
        assert_eq!(receipts.len(), 8);
        std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "source":std::env::var("ARKLINE_DOCKING_CAPTURE_SOURCE").unwrap_or_else(|_|"unrecorded".into()),
            "size":[960,640], "actual_engine":"EditorWorld accepted commands/fixed ticks, production Cockpit update and egui_glow callbacks, Mesa EGL",
            "scope":"Eight state views; not a motion sequence, performance proof or full station traversal",
            "walk_distance_m":distance, "frames":receipts,
        })).unwrap()).unwrap();
        target.destroy(&gl);
    }
}

/// Real server + real foreground pointer ownership, drawn by the same native
/// painter as docking. This is a legacy-layout regression, not a DockingV1 walk.
struct NativeUiJourney {
    capture: CockpitCapture,
    server: arkline_server::Server,
    session: Option<String>,
    now: u64,
    gl: Arc<glow::Context>,
    target: CachedFeed,
    root: std::path::PathBuf,
}
impl NativeUiJourney {
    unsafe fn step(&mut self, events: Vec<egui::Event>, name: &str) -> Vec<(String, egui::Pos2)> {
        self.now += 50;
        self.server.tick_at(self.now).unwrap();
        let response = self
            .server
            .handle_at(&mut self.session, Request::Snapshot, self.now);
        let labels = self.capture.capture_input(
            &self.gl,
            &self.target,
            response,
            &self.root.join(format!("{name}.png")),
            Some(events),
        );
        let mut requests: Vec<_> = self.capture._requests.try_iter().collect();
        if let Some(control) = self.capture.cockpit.controls.take() {
            requests.push(control);
        }
        for request in requests {
            let response = self
                .server
                .handle_at(&mut self.session, request.clone(), self.now);
            assert!(
                response.ok,
                "UI emitted rejected {request:?}: {}",
                response.message
            );
            self.capture
                .updates
                .send(crate::Update::Response(Box::new(response), true))
                .unwrap();
        }
        self.capture.cockpit.poll();
        labels
    }
    unsafe fn click_station(&mut self, text: &str) {
        self.step(vec![], "working");
        let labels = self.step(vec![], "working");
        let id = egui::Id::new("station_embodiment_controls");
        let rect = self.capture.ctx.memory(|m| m.area_rect(id)).unwrap();
        let point = labels
            .iter()
            .rev()
            .find(|(label, pos)| label == text && rect.contains(*pos))
            .unwrap_or_else(|| panic!("Missing foreground action {text}: {labels:?}"))
            .1;
        assert_eq!(
            self.capture.ctx.layer_id_at(point),
            Some(egui::LayerId::new(egui::Order::Middle, id))
        );
        for pressed in [true, false] {
            self.step(
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
                "working",
            );
        }
    }
    unsafe fn key(&mut self, key: egui::Key) {
        for pressed in [true, false] {
            self.step(
                vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                "working",
            );
        }
    }
}
#[test]
#[ignore = "Actual EGL foreground legacy camera controls; requires explicit output directory"]
fn native_baseline_foreground_camera_controls() {
    use sha2::{Digest, Sha256};
    let root = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_BASELINE_UI_CAPTURE_DIR").expect("capture destination"),
    );
    std::fs::create_dir_all(&root).unwrap();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let mut server = arkline_server::Server::open(":memory:").unwrap();
        let mut session = None;
        let response = server.handle_at(
            &mut session,
            Request::Connect {
                supports_pioneer_interior: true,
                supports_docked_traversal_v2: false,
                world_layout_version: 1,
                supports_fleet_hangar: true,
                supports_room_controls: true,
                protocol_version: arkline_protocol::PROTOCOL_VERSION,
                pilot: "DockingCapture".into(),
                resume_token: None,
            },
            1000,
        );
        assert!(response.ok);
        let mut capture = CockpitCapture::new(gl.clone(), response.clone());
        capture.cockpit.embedded = false;
        // Re-deliver the genuine connection through the desktop transport path,
        // so its normal initial Station workspace is not suppressed by the
        // embedded-motion defaults of the shared capture constructor.
        capture.updates.send(crate::Update::Ready).unwrap();
        capture
            .updates
            .send(crate::Update::Response(Box::new(response), true))
            .unwrap();
        capture.cockpit.poll();
        let target = CachedFeed::create(&gl, 902, 1280, 800).unwrap();
        let mut j = NativeUiJourney {
            capture,
            server,
            session,
            now: 1000,
            gl,
            target,
            root: root.clone(),
        };
        j.step(vec![], "01-station-before");
        j.click_station("Explore Pioneer interior");
        let start = j.capture.cockpit.own().unwrap().station_avatar.unwrap();
        j.step(vec![], "02-interior-third-person");
        j.click_station("First person");
        assert_eq!(
            j.capture.cockpit.hangar_view.on_foot_view,
            hangar::OnFootView::FirstPerson
        );
        assert_eq!(
            j.capture
                .cockpit
                .own()
                .unwrap()
                .station_avatar
                .unwrap()
                .position,
            start.position
        );
        assert!(j.capture.cockpit.services_open);
        j.key(egui::Key::W);
        assert_eq!(
            j.capture
                .cockpit
                .own()
                .unwrap()
                .station_avatar
                .unwrap()
                .position,
            start.position
        );
        j.step(vec![], "03-first-person-workspace-gate");
        j.key(egui::Key::Escape);
        assert!(!j.capture.cockpit.services_open);
        j.step(vec![], "04-first-person-workspace-closed");
        j.click_station("Third person");
        assert_eq!(
            j.capture.cockpit.hangar_view.on_foot_view,
            hangar::OnFootView::ThirdPerson
        );
        j.step(vec![], "05-third-person-return");
        let mut images = Vec::new();
        for name in [
            "01-station-before",
            "02-interior-third-person",
            "03-first-person-workspace-gate",
            "04-first-person-workspace-closed",
            "05-third-person-return",
        ] {
            let bytes = std::fs::read(root.join(format!("{name}.png"))).unwrap();
            images.push(serde_json::json!({"file":format!("{name}.png"), "sha256":hex::encode(Sha256::digest(bytes))}));
        }
        std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "fixture":"real in-memory legacy LinearV1 server; protocol Connect layout version 1",
            "viewport":[1280,800], "input":"actual foreground-layer pointer clicks and keyboard",
            "assertions":["First person changes camera not avatar", "workspace blocks movement", "Escape closes workspace", "Third person returns"],
            "images":images,
        })).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "Native replay of actual timed-unload authority Responses; explicit input/output paths"]
fn native_authoritative_timed_unload_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_UNLOAD_CAPTURE_DIR").expect("authority input"),
    );
    let root = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_UNLOAD_GPU_DIR").expect("native output"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let frames = receipt["frames"].as_array().unwrap();
    assert_eq!(receipt["cadence_ms"], 100);
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let first: arkline_protocol::Response = serde_json::from_slice(
            &std::fs::read(source.join(frames[0]["file"].as_str().unwrap())).unwrap(),
        )
        .unwrap();
        let owner = std::env::var("ARKLINE_UNLOAD_PILOT").unwrap_or_else(|_| "Pilot".into());
        let mut capture = CockpitCapture::new_for_pilot(gl.clone(), first, &owner);
        let target = CachedFeed::create(&gl, 903, 1280, 800).unwrap();
        capture.cockpit.service_tab = crate::ServiceTab::Cargo;
        capture.cockpit.services_open = true;
        let mut connected = true;
        let mut proof = Vec::new();
        for (index, row) in frames.iter().enumerate() {
            let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
            let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
            let next_connected = row["connected"].as_bool().unwrap();
            if connected != next_connected {
                capture
                    .updates
                    .send(if next_connected {
                        crate::Update::Ready
                    } else {
                        crate::Update::Reconnecting(
                            "Authority fixture disconnected; retained data is cached".into(),
                        )
                    })
                    .unwrap();
                connected = next_connected;
            }
            let name = format!("unload-{index:04}.png");
            let labels = capture.capture_input(
                &gl,
                &target,
                response.clone(),
                &root.join(&name),
                Some(vec![]),
            );
            assert_eq!(capture.cockpit.connected, connected);
            assert!(capture.cockpit.services_open);
            if let Some(op) = response
                .assets
                .as_ref()
                .and_then(|a| a.cargo_unloading.as_ref())
                .and_then(|s| s.latest())
            {
                let title = format!("{} #{}", op.phase.label(), op.operation_id);
                assert!(
                    labels.iter().any(|(text, _)| text == &title),
                    "operation progress must be visible without inventory scroll: {title}"
                );
                if op.phase.active() {
                    assert!(labels
                        .iter()
                        .any(|(text, _)| text == "Cancel remaining unload"));
                }
            }
            assert!(
                capture
                    ._requests
                    .try_iter()
                    .all(|r| matches!(r, Request::Snapshot)),
                "passive replay cannot initiate a transfer"
            );
            let image = std::fs::read(root.join(&name)).unwrap();
            proof.push(
                serde_json::json!({"file":name,"sha256":hex::encode(Sha256::digest(image)),
                "authority_file":row["file"],"authority_sha256":hex::encode(Sha256::digest(bytes)),
                "server_ms":row["server_ms"],"connected":connected,"action":row["action"],
                "visible_text":labels.iter().map(|(text,_)|text).collect::<Vec<_>>(),
                "assets_present":response.assets.is_some()}),
            );
        }
        std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "authority_receipt":receipt,"frames":proof,"cadence_ms":100,"viewport":[1280,800],
            "description":"Production Cargo workspace; actual committed generic ore batches. Offline view is retained cached data with disconnected authority, not new private responses. No physical crane animation."})).unwrap()).unwrap();
    }
}

#[test]
#[ignore = "Native departure-readiness proof from exact timed-unload authority responses"]
fn native_cargo_departure_readiness_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_UNLOAD_CAPTURE_DIR").expect("authority input"),
    );
    let root = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_DEPARTURE_GPU_DIR").expect("native output"),
    );
    std::fs::create_dir_all(&root).unwrap();
    let authority_receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let mut proof = Vec::new();
    let layout_review = std::env::var_os("ARKLINE_WORKSPACE_LAYOUT_CAPTURE").is_some();
    let viewports: &[(i32, i32)] = if layout_review {
        &[(800, 600), (900, 650), (1280, 800)]
    } else {
        &[(900, 650), (1280, 800)]
    };
    let scenarios: &[(usize, &str, bool)] = if layout_review {
        &[(34, "held", true)]
    } else {
        &[
            (15, "unloading", true),
            (34, "held", true),
            (45, "resumed", true),
            (50, "completed", false),
            (86, "cancelled", false),
        ]
    };
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        for &(width, height) in viewports {
            let target = CachedFeed::create(&gl, 904, width, height).unwrap();
            for &(index, phase, reserved) in scenarios {
                let row = &authority_receipt["frames"][index];
                assert_eq!(row["connected"], true);
                let authority_file = row["file"].as_str().unwrap();
                let bytes = std::fs::read(source.join(authority_file)).unwrap();
                let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
                let mut capture =
                    CockpitCapture::new_for_pilot(gl.clone(), response.clone(), "Pilot");
                capture.cockpit.identity_collapsed = !layout_review;
                for workspace in [false, true] {
                    capture.cockpit.services_open = workspace;
                    capture.cockpit.service_tab = crate::ServiceTab::Navigation;
                    let name = format!(
                        "{width}-{phase}-{}.png",
                        if workspace { "navigation" } else { "cockpit" }
                    );
                    // Settle real egui window placement before recording its visible text.
                    let mut labels = Vec::new();
                    for _ in 0..3 {
                        labels = capture.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&name),
                            Some(vec![]),
                        );
                    }
                    assert!(capture.cockpit.navigation_order_reason().is_none());
                    assert_eq!(capture.cockpit.departure_order_reason().is_some(), reserved);
                    if workspace && reserved {
                        assert!(
                            labels
                                .iter()
                                .any(|(text, _)| text.contains("Cargo reserved")),
                            "cargo departure explanation must be visible at {width}x{height}"
                        );
                    }
                    assert!(
                        capture
                            ._requests
                            .try_iter()
                            .all(|r| matches!(r, Request::Snapshot)),
                        "passive proof must not request departure or mutate cargo"
                    );
                    proof.push(serde_json::json!({
                        "file":name,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&name)).unwrap())),
                        "authority_file":authority_file,"authority_sha256":hex::encode(Sha256::digest(&bytes)),
                        "phase":phase,"reserved":reserved,"viewport":[width,height],"workspace":workspace,
                        "authority_expanded":layout_review,
                        "departure_reason":capture.cockpit.departure_order_reason(),
                        "visible_text":labels.iter().map(|(text,_)|text).collect::<Vec<_>>(),
                        "camera":capture.camera_receipt(),
                    }));
                    // Opt-in diagnostic below the supported900x650 minimum. Its known
                    // post-click scroll shift is retained, not a passed acceptance gate.
                    if layout_review
                        && workspace
                        && width == 800
                        && std::env::var_os("ARKLINE_WORKSPACE_SCROLL_DIAGNOSTIC").is_some()
                    {
                        let scrolled = format!("{width}-{phase}-navigation-scrolled.png");
                        for events in [
                            vec![egui::Event::PointerMoved(egui::pos2(500., 505.))],
                            vec![egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(0., -60.),
                                modifiers: egui::Modifiers::NONE,
                            }],
                            vec![],
                        ] {
                            labels = capture.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &root.join(&scrolled),
                                Some(events),
                            );
                        }
                        // Move onto the control before settling hover/layout, just as a
                        // real pointer does before its button-down frame.
                        let hover = labels
                            .iter()
                            .find(|(text, _)| text == "Undock")
                            .expect("scroll reveals departure control")
                            .1;
                        labels = capture.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&scrolled),
                            Some(vec![egui::Event::PointerMoved(hover)]),
                        );
                        let mut previous: Option<egui::Pos2> = None;
                        let mut stable_frames = 0;
                        for _ in 0..40 {
                            labels = capture.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &root.join(&scrolled),
                                Some(vec![]),
                            );
                            if let Some((_, current)) =
                                labels.iter().find(|(text, _)| text == "Undock")
                            {
                                stable_frames =
                                    if previous.is_some_and(|old| old.distance(*current) < 0.1) {
                                        stable_frames + 1
                                    } else {
                                        0
                                    };
                                previous = Some(*current);
                                if stable_frames >= 3 {
                                    break;
                                }
                            }
                        }
                        assert!(
                            stable_frames >= 3,
                            "actual scroll must reach a stable visible control"
                        );
                        let point = labels
                            .iter()
                            .find(|(text, _)| text == "Undock")
                            .expect("real scroll must reveal departure control")
                            .1;
                        let notice = capture.cockpit.notice.clone();
                        for pressed in [true, false] {
                            labels = capture.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &root.join(&scrolled),
                                Some(vec![
                                    egui::Event::PointerMoved(point),
                                    egui::Event::PointerButton {
                                        pos: point,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ]),
                            );
                        }
                        let final_point = labels
                            .iter()
                            .find(|(text, _)| text == "Undock")
                            .expect("settled departure control must remain visible")
                            .1;
                        assert!(
                            point.distance(final_point) < 1.0,
                            "scroll must settle before pointer interaction: before={point:?} after={final_point:?}"
                        );
                        assert_eq!(
                            capture.cockpit.notice, notice,
                            "disabled control must not invoke the send guard"
                        );
                        assert!(!capture.cockpit.request_pending);
                        assert!(capture
                            ._requests
                            .try_iter()
                            .all(|r| matches!(r, Request::Snapshot)));
                        proof.push(serde_json::json!({"file":scrolled,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&scrolled)).unwrap())),"authority_file":authority_file,"authority_sha256":hex::encode(Sha256::digest(&bytes)),"viewport":[width,height],"authority_expanded":true,"interaction":"actual wheel scroll and disabled Undock press/release","visible_text":labels.iter().map(|(text,_)|text).collect::<Vec<_>>(),"camera":capture.camera_receipt()}));
                    }
                }
            }
        }
    }
    std::fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "authority_producer":"e443229c4793b10581b97c7e1b7959d7fbb98cc8",
        "description":"Actual committed authority responses, production cockpit and Navigation workspace; controlled-clock software-renderer stills, not live performance or a new cargo simulation.",
        "frames":proof,
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "native static geometry design study; not a simulated cargo operation"]
fn native_ore_receiver_geometry_design_capture() {
    use arkline_protocol::drone_dropoff_geometry as geometry;
    use sha2::{Digest, Sha256};
    let root = std::path::PathBuf::from(std::env::var_os("ARKLINE_RECEIVER_DESIGN_DIR").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let mut proof = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, 998, 960, 640).unwrap();
        let hull: Vec<_> = docked_pioneer_parts()
            .unwrap()
            .iter()
            .map(|&(mesh, matrix, color, emission)| {
                (
                    mesh,
                    Mat4::from_scale(Vec3::splat(100.0)) * matrix,
                    color,
                    emission,
                )
            })
            .collect();
        for (label, config, show_drone, eye, look) in [
            (
                "receiver-stowed-context",
                geometry::DropoffConfiguration::STOWED,
                false,
                Vec3::new(-46., 2., 11.),
                Vec3::new(-35., -1., 4.),
            ),
            (
                "receiver-coupled-contact",
                geometry::DropoffConfiguration::COUPLED,
                true,
                Vec3::new(-37.2, 0.4, 7.2),
                Vec3::new(-37.5, -1., 5.),
            ),
            (
                "receiver-coupled-wide",
                geometry::DropoffConfiguration::COUPLED,
                true,
                Vec3::new(-48., 2.5, 12.),
                Vec3::new(-38., -1., 5.),
            ),
        ] {
            let roots = geometry::mechanism_ids()
                .into_iter()
                .zip(geometry::mechanism_poses(config).unwrap())
                .map(|(id, pose)| (id.semantic_id().to_owned(), pose))
                .collect();
            let mut parts = hull.clone();
            parts.extend(receiver_body_parts(&roots).unwrap());
            if show_drone {
                let pose = geometry::coupled_pose();
                let mount = Mat4::from_scale_rotation_translation(
                    Vec3::splat(100.),
                    Quat::from_array(pose.orientation_xyzw.map(|v| v as f32)),
                    Vec3::from_array(pose.position_m.map(|v| v as f32)),
                );
                parts.extend(
                    cached_drone_parts(arkline_protocol::DroneClass::Mining)
                        .unwrap()
                        .iter()
                        .map(|&(mesh, matrix, color, emission)| {
                            (mesh, mount * matrix, color, emission)
                        }),
                );
            }
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 960, 640);
            gl.disable(glow::SCISSOR_TEST);
            gl.color_mask(true, true, true, true);
            let vp = Mat4::perspective_rh_gl(50_f32.to_radians(), 1.5, 0.02, 300.)
                * Mat4::look_at_rh(eye, look, Vec3::Y);
            hangar::paint_preview(&mut renderer, vp, eye, &parts, true);
            let mut pixels = vec![0u8; 960 * 640 * 4];
            gl.read_pixels(
                0,
                0,
                960,
                640,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelPackData::Slice(Some(&mut pixels)),
            );
            assert_eq!(gl.get_error(), glow::NO_ERROR);
            let flipped: Vec<_> = pixels
                .chunks_exact(960 * 4)
                .rev()
                .flat_map(|row| row.iter().copied())
                .collect();
            let file = format!("{label}.png");
            image::save_buffer(
                root.join(&file),
                &flipped,
                960,
                640,
                image::ColorType::Rgba8,
            )
            .unwrap();
            proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),"eye_body_m":eye.to_array(),"look_body_m":look.to_array(),"receiver_extension":config.extension,"latch_closed":config.latch_closed,"design_fixture_drone":show_drone}));
        }
    }
    std::fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"kind":"STATIC SOURCE GEOMETRY DESIGN STUDY: registered keyframes and production meshes/materials, not authority Responses, no cargo transfer or fleet claim","base_source_sha256":geometry::DROPOFF_BASE_SOURCE_SHA256,"receiver_source_sha256":geometry::DROPOFF_RECEIVER_SOURCE_SHA256,"frames":proof})).unwrap()).unwrap();
}

#[test]
#[ignore = "actual authority Response and native software renderer required"]
fn native_authoritative_stored_drone_bay_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_BAY_INITIAL_RESPONSE").expect("actual authority Response"),
    );
    let root = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_BAY_GPU_DIR").expect("output directory"),
    );
    let name = std::env::var("ARKLINE_BAY_PILOT").expect("actual pilot name");
    let bytes = std::fs::read(&source).unwrap();
    let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
    assert!(response.ok);
    let snapshot = response.snapshot.as_ref().unwrap();
    let pilot = snapshot.pilots.iter().find(|p| p.name == name).unwrap();
    let bay = drone_bay_scene::validated_bay(pilot).expect("actual supported bay");
    assert_eq!(
        bay.cradles
            .iter()
            .filter(|c| c.presence == arkline_protocol::DroneBayPresence::Stored)
            .count(),
        15
    );
    assert!(pilot.drones.is_empty());
    std::fs::create_dir_all(&root).unwrap();
    let mut proof = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, 905, 960, 640).unwrap();
        let base = frame_for_pilot(
            snapshot,
            display_position(pilot, snapshot.server_time_ms),
            true,
            &name,
        );
        for (label, source) in [
            ("overview", crate::camera_sources::CameraSource::BayOverview),
            ("recon", crate::camera_sources::CameraSource::BayRack(0)),
            ("mining", crate::camera_sources::CameraSource::BayRack(1)),
            ("combat", crate::camera_sources::CameraSource::BayRack(2)),
        ] {
            let feed = drone_bay_scene::camera_frame(&base, pilot, source, 1.5).unwrap();
            let file = format!("stored-bay-{label}.png");
            capture(&gl, &mut renderer, &target, &feed, &root.join(&file));
            proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),"camera":label,"viewport":[960,640],"stored_ids":bay.cradles.iter().map(|c|c.drone_id).collect::<Vec<_>>()}));
        }
        let full_target = CachedFeed::create(&gl, 906, 1280, 800).unwrap();
        let mut app = CockpitCapture::new_for_pilot(gl.clone(), response.clone(), &name);
        app.cockpit.drone_camera = true;
        app.cockpit.camera_source = crate::camera_sources::CameraSource::BayOverview;
        let file = "stored-bay-cockpit.png";
        for _ in 0..3 {
            app.capture_input(
                &gl,
                &full_target,
                response.clone(),
                &root.join(file),
                Some(vec![]),
            );
        }
        assert!(app
            ._requests
            .try_iter()
            .all(|r| matches!(r, Request::Snapshot)));
        proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(file)).unwrap())),"camera":"normal cockpit with bay monitor and authority HUD","viewport":[1280,800],"camera_receipt":app.camera_receipt()}));
        app.cockpit.services_open = true;
        app.cockpit.service_tab = crate::ServiceTab::Drones;
        app.cockpit.selected_drone = Some(bay.cradles[0].drone_id);
        let file = "stored-bay-operations.png";
        let mut labels = Vec::new();
        for _ in 0..3 {
            labels = app.capture_input(
                &gl,
                &full_target,
                response.clone(),
                &root.join(file),
                Some(vec![]),
            );
        }
        assert!(
            labels.iter().any(|(text, _)| text.contains("FINITE BAY")),
            "actual stored roster must be visible"
        );
        assert!(app
            ._requests
            .try_iter()
            .all(|r| matches!(r, Request::Snapshot)));
        proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(file)).unwrap())),"camera":"normal cockpit with actual finite-bay operations workspace","viewport":[1280,800],"selected_drone":app.cockpit.selected_drone,"visible_text":labels.iter().map(|(text,_)|text).collect::<Vec<_>>()}));
        let narrow_target = CachedFeed::create(&gl, 908, 900, 650).unwrap();
        let file = "stored-bay-operations-900.png";
        for _ in 0..3 {
            labels = app.capture_input(
                &gl,
                &narrow_target,
                response.clone(),
                &root.join(file),
                Some(vec![]),
            );
        }
        assert!(labels.iter().any(|(text, _)| text.contains("FINITE BAY")));
        assert!(app
            ._requests
            .try_iter()
            .all(|r| matches!(r, Request::Snapshot)));
        proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(file)).unwrap())),"camera":"supported narrow viewport finite-bay workspace","viewport":[900,650],"selected_drone":app.cockpit.selected_drone,"visible_text":labels.iter().map(|(text,_)|text).collect::<Vec<_>>()}));
    }
    std::fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"authority_sha256":hex::encode(Sha256::digest(bytes)),"pilot":name,"registration":bay.binding.geometry_revision,"source_sha256":bay.binding.source_sha256,"kind":"actual server stored inventory, production renderer fixed bay cameras; not motion or performance proof","frames":proof})).unwrap()).unwrap();
}

/// Continuous documentary camera following the same actual entity, including
/// its home-cradle pose while Stored. No body/hull geometry is hidden.
fn mining_inspection_frame(base: &Frame, pilot: &PilotView, id: u64) -> Frame {
    let model = if let Some(drone) = pilot.drones.iter().find(|d| d.id == id) {
        drone_bay_scene::active_transform(pilot, drone, base.space, base.now).unwrap()
    } else {
        let bay = drone_bay_scene::validated_bay(pilot).unwrap();
        drone_bay_scene::stored_instances(pilot, bay, base.space, base.now)
            .into_iter()
            .find(|(i, _, _)| *i == id)
            .unwrap()
            .2
    };
    let center = model.transform_point3(Vec3::ZERO);
    let desired = model.transform_point3(Vec3::new(0.065, 0.04, 0.07));
    let carrier = ship_orientation_at(pilot, base.now);
    let origin = base.space.position(pilot, base.now);
    let body = carrier.inverse() * (center - origin) * 100.;
    // The close inspection camera stays in the existing open handling aisle
    // while the body is inside. It continuously blends to the body-follow rig
    // only after clearing the authored bay bounds. No hull is made transparent.
    let inside = Vec3::new(
        (body.x - 6.5).clamp(-34., -17.),
        (body.y + 0.7).clamp(-5.8, 4.2),
        -6.8,
    );
    let outside = Vec3::new(
        (-36. - body.x).max(body.x + 15.).max(0.),
        (-6.4 - body.y).max(body.y - 4.9).max(0.),
        (-7.7 - body.z).max(body.z - 7.7).max(0.),
    )
    .max_element();
    let t = ((outside - 4.) / 10.).clamp(0., 1.);
    let blend = t * t * (3. - 2. * t);
    // Move the inspection eye through the open throat while extraction is
    // actually occurring, before the carrier secures its door at external hold.
    // Keep lateral/vertical eye coordinates inside the real aperture until clear.
    let throat_t = ((-body.x - 31.) / 7.5).clamp(0., 1.);
    let throat_blend = throat_t * throat_t * (3. - 2. * throat_t);
    let through = inside.lerp(Vec3::new(body.x - 6.5, body.y + 0.7, -6.8), throat_blend);
    let eye = (origin + carrier * through * 0.01).lerp(desired, blend);
    let up = (carrier * Vec3::Y)
        .lerp(model.transform_vector3(Vec3::Y).normalize(), blend)
        .normalize();
    let view = Mat4::look_at_rh(eye, center, up);
    let mut f = base.clone();
    f.eye = eye;
    f.drone_camera = true;
    f.camera_drone = None;
    f.cockpit = false;
    f.vp = Mat4::perspective_rh_gl(58_f32.to_radians(), 1.5, 0.0005, 1200.) * view;
    f.cockpit_vp = f.vp;
    f.planet_vp = Mat4::perspective_rh_gl(58_f32.to_radians(), 1.5, 1., 300_000.) * view;
    f
}

fn mining_impact_inspection_frame(base: &Frame, pilot: &PilotView, id: u64) -> Option<Frame> {
    let drone = pilot.drones.iter().find(|d| d.id == id)?;
    let target = drone_work::mining_target(pilot, drone, base.now)?;
    let asteroid = base.scene.asteroids.iter().find(|a| a.id == target)?;
    let start = base.space.scene(drone_work::work_head_world(
        pilot,
        drone,
        base.now,
        Some(asteroid.position),
    )?);
    let impact = mining_impact(start, asteroid, base.space, base.now)?;
    let outward = (start - impact).normalize();
    let side = outward.cross(Vec3::Y).normalize_or_zero();
    let eye = impact + outward * 0.18 + side * 0.08;
    let up = if outward.dot(Vec3::Y).abs() > 0.95 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    let view = Mat4::look_at_rh(eye, impact, up);
    let mut f = base.clone();
    f.eye = eye;
    f.cockpit = false;
    f.drone_camera = true;
    f.camera_drone = None;
    f.vp = Mat4::perspective_rh_gl(58_f32.to_radians(), 1.5, 0.0005, 1200.) * view;
    f.cockpit_vp = f.vp;
    f.planet_vp = Mat4::perspective_rh_gl(58_f32.to_radians(), 1.5, 1., 300_000.) * view;
    Some(f)
}

#[test]
#[ignore = "actual continuous authority journey and native software renderer required"]
fn native_authoritative_drone_bay_journey_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_BAY_JOURNEY_DIR").expect("authority journey"),
    );
    let root =
        std::path::PathBuf::from(std::env::var_os("ARKLINE_BAY_GPU_DIR").expect("native output"));
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["cadence_ms"], 100);
    let name = std::env::var("ARKLINE_BAY_PILOT").expect("actual pilot");
    let smoke = std::env::var_os("ARKLINE_BAY_MOTION_SMOKE").is_some();
    let smoke_indices: Option<std::collections::BTreeSet<usize>> =
        std::env::var("ARKLINE_BAY_SMOKE_INDICES").ok().map(|raw| {
            raw.split(',')
                .map(|v| v.parse().expect("explicit smoke frame index"))
                .collect()
        });
    if let Some(indices) = &smoke_indices {
        assert!(
            !indices.is_empty()
                && indices
                    .iter()
                    .all(|i| *i < receipt["frames"].as_array().unwrap().len())
        );
    }
    let stride: usize = std::env::var("ARKLINE_BAY_CAPTURE_STRIDE")
        .ok()
        .map(|v| v.parse().expect("integer capture stride"))
        .unwrap_or(1);
    assert!(
        [1, 2].contains(&stride),
        "only actual10fps or5fps sampling supported"
    );
    let camera_set = std::env::var("ARKLINE_BAY_CAMERA_SET").unwrap_or_else(|_| "bay,aft".into());
    let selected_cameras = camera_set.split(',').collect::<Vec<_>>();
    assert!(
        !selected_cameras.is_empty()
            && selected_cameras.iter().all(|v| [
                "bay",
                "aft",
                "far-rack",
                "exit-rack",
                "bay-inspection",
                "mining-inspection",
                "mining-impact",
                "ore-receiver"
            ]
            .contains(v))
    );
    std::fs::create_dir_all(&root).unwrap();
    let mut proof = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, 907, 960, 640).unwrap();
        for (index, row) in receipt["frames"].as_array().unwrap().iter().enumerate() {
            if smoke
                && !smoke_indices.as_ref().map_or_else(
                    || [0, 10, 150, 300, 500, 592, 800, 1170].contains(&index),
                    |indices| indices.contains(&index),
                )
            {
                continue;
            }
            if !smoke && index % stride != 0 {
                continue;
            }
            let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
            assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
            let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
            assert!(response.ok);
            let snapshot = response.snapshot.as_ref().unwrap();
            let pilot = snapshot.pilots.iter().find(|p| p.name == name).unwrap();
            let bay = drone_bay_scene::validated_bay(pilot).expect("supported actual bay");
            let origin = display_position(pilot, snapshot.server_time_ms);
            let base = frame_for_pilot(snapshot, origin, false, &name);
            let inside = drone_bay_scene::camera_frame(
                &base,
                pilot,
                crate::camera_sources::CameraSource::BayOverview,
                1.5,
            )
            .unwrap();
            let mut outside = base.clone();
            let rotation = ship_orientation_at(pilot, base.now);
            let eye = base.space.scene(origin) + rotation * Vec3::new(-0.49, 0.04, -0.17);
            let look = base.space.scene(origin) + rotation * Vec3::new(-0.30, -0.01, -0.035);
            let view = Mat4::look_at_rh(eye, look, rotation * Vec3::Y);
            outside.eye = eye;
            outside.vp = Mat4::perspective_rh_gl(58_f32.to_radians(), 1.5, 0.002, 1200.) * view;
            outside.planet_vp =
                Mat4::perspective_rh_gl(58_f32.to_radians(), 1.5, 1., 300_000.) * view;
            outside.drone_camera = true;
            // Fixed documentary inspection viewpoint inside the front/side bay
            // envelope: all source hull, racks and hardware remain rendered.
            // It looks along the aisle, avoiding the close wall camera's pass-by.
            let mut inspection = inside.clone();
            let inspect_eye =
                base.space.scene(origin) + rotation * Vec3::new(-0.185, -0.01, -0.0695);
            let inspect_look =
                base.space.scene(origin) + rotation * Vec3::new(-0.26, -0.01, -0.005);
            let inspect_view = Mat4::look_at_rh(inspect_eye, inspect_look, rotation * Vec3::Y);
            inspection.eye = inspect_eye;
            inspection.vp =
                Mat4::perspective_rh_gl(90_f32.to_radians(), 1.5, 0.0005, 1200.) * inspect_view;
            inspection.cockpit_vp = inspection.vp;
            inspection.planet_vp =
                Mat4::perspective_rh_gl(90_f32.to_radians(), 1.5, 1., 300_000.) * inspect_view;
            // These are existing physical bay observation cameras, not a
            // transparent cutaway or a modified drone route.
            let far_rack = drone_bay_scene::camera_frame(
                &base,
                pilot,
                crate::camera_sources::CameraSource::BayRack(2),
                1.5,
            )
            .unwrap();
            let exit_rack = drone_bay_scene::camera_frame(
                &base,
                pilot,
                crate::camera_sources::CameraSource::BayRack(0),
                1.5,
            )
            .unwrap();
            let mining = receipt["drone_id"]
                .as_u64()
                .map(|id| mining_inspection_frame(&base, pilot, id));
            let mut cameras = vec![
                ("bay", inside),
                ("aft", outside),
                ("far-rack", far_rack),
                ("exit-rack", exit_rack),
                ("bay-inspection", inspection),
            ];
            if let Some(mining) = mining {
                cameras.push(("mining-inspection", mining));
            }
            if let Some(impact) = receipt["drone_id"]
                .as_u64()
                .and_then(|id| mining_impact_inspection_frame(&base, pilot, id))
            {
                cameras.push(("mining-impact", impact));
            }
            if selected_cameras.contains(&"ore-receiver") {
                cameras.push((
                    "ore-receiver",
                    drone_bay_scene::operations_camera_frame(
                        &base,
                        pilot,
                        BayMonitorCamera::OreReceiver,
                        1.5,
                    )
                    .expect("source-validated actual ore receiver"),
                ));
            }
            for (camera, frame) in cameras {
                if !selected_cameras.contains(&camera) {
                    continue;
                }
                let file = format!("{camera}-{index:04}.png");
                capture(&gl, &mut renderer, &target, &frame, &root.join(&file));
                let mining_evidence=receipt["drone_id"].as_u64().and_then(|id|pilot.drones.iter().find(|d|d.id==id)).and_then(|drone|{
                    let target=drone_work::mining_target(pilot,drone,frame.now)?;let asteroid=frame.scene.asteroids.iter().find(|a|a.id==target)?;
                    let mount=drone_work::work_head_world(pilot,drone,frame.now,Some(asteroid.position))?;
                    let impact=mining_impact(frame.space.scene(mount),asteroid,frame.space,frame.now)?;
                    Some(serde_json::json!({"drone_id":drone.id,"target_id":target,"work_head_world_m":mount,"surface_world_m":frame.space.world(impact),"confirmed_impact_strength":renderer.drone_impacts.strength(&pilot.name,drone.id,target,frame.now)}))
                });
                if receipt["confirmed_cycles"]
                    .as_array()
                    .is_some_and(|cycles| {
                        cycles
                            .iter()
                            .any(|cycle| cycle["frame"].as_u64() == Some(index as u64))
                    })
                {
                    assert!(
                        mining_evidence.as_ref().is_some_and(|e| {
                            e["confirmed_impact_strength"]
                                .as_f64()
                                .is_some_and(|s| s > 0.0)
                        }),
                        "accepted cycle must reach the actual world painter at frame {index}"
                    );
                }
                proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),"camera":camera,"index":index,"time_ms":snapshot.server_time_ms,"authority_file":row["file"],"authority_sha256":row["response_sha256"],"phase":row["phase"],"door_fraction":bay.machinery.door_fraction,"stored":bay.cradles.iter().filter(|c| c.presence==arkline_protocol::DroneBayPresence::Stored).count(),"active":pilot.drones.len(),"drone_id":receipt["drone_id"],"drone_ids":receipt["drone_ids"],"camera_eye":frame.eye.to_array(),"camera_vp":frame.vp.to_cols_array(),"mining_evidence":mining_evidence,"drone_cargo":pilot.drone_cargo,"ship_ore":pilot.ore,"cargo_totals":snapshot.drone_cargo_totals}));
            }
            if index % 100 == 0 {
                eprintln!("actual bay journey frame {index}");
            }
        }
    }
    std::fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"authority_receipt":receipt,"sampled_smoke_only":smoke,"camera_set":camera_set,"cadence_ms":100*stride,"capture_stride":stride,"kind":"actual authority sequence, production renderer; fixed full-geometry bay and aft inspection views; software capture not live frame-rate proof","frames":proof})).unwrap()).unwrap();
}

#[test]
#[ignore = "actual authority journey and native software renderer required"]
fn native_same_drone_cockpit_camera_continuity_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(
        std::env::var_os("ARKLINE_BAY_JOURNEY_DIR").expect("authority journey"),
    );
    let root =
        std::path::PathBuf::from(std::env::var_os("ARKLINE_BAY_GPU_DIR").expect("native output"));
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let name = std::env::var("ARKLINE_BAY_PILOT").expect("actual pilot");
    let selected = receipt["drone_id"].as_u64().unwrap();
    let rows = receipt["frames"].as_array().unwrap();
    let first: arkline_protocol::Response = serde_json::from_slice(
        &std::fs::read(source.join(rows[0]["file"].as_str().unwrap())).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let mut proof = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let target = CachedFeed::create(&gl, 909, 1280, 800).unwrap();
        let mut app = CockpitCapture::new_for_pilot(gl.clone(), first, &name);
        app.cockpit.drone_camera = true;
        app.cockpit.camera_source = crate::camera_sources::CameraSource::Drones;
        app.cockpit.drone_camera_mode = DroneCameraMode::Pov;
        app.cockpit.drone_split = false;
        app.cockpit.selected_drone = Some(selected);
        for index in [0, 150, 571, 935, 1170] {
            let row = &rows[index];
            let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
            assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
            let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
            let pilot = response
                .snapshot
                .as_ref()
                .unwrap()
                .pilots
                .iter()
                .find(|p| p.name == name)
                .unwrap();
            let bay = drone_bay_scene::validated_bay(pilot).unwrap();
            let entry = bay
                .cradles
                .iter()
                .find(|entry| entry.drone_id == selected)
                .unwrap();
            let file = format!("same-drone-{index:04}.png");
            for _ in 0..3 {
                app.capture_input(
                    &gl,
                    &target,
                    response.clone(),
                    &root.join(&file),
                    Some(vec![]),
                );
            }
            assert_eq!(app.cockpit.selected_drone, Some(selected));
            assert!(app
                ._requests
                .try_iter()
                .all(|request| matches!(request, Request::Snapshot)));
            proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),"authority_sha256":row["response_sha256"],"index":index,"selected_drone":selected,"presence":entry.presence,"home_cradle":entry.home_cradle,"hit_points":entry.hit_points,"kind":"unchanged selected identity; active POV or explicit stored home-rack camera"}));
        }
    }
    std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"frames":proof,"sampled_status_views":true,"not_continuous_motion_footage":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "actual authority journey and native software renderer required"]
fn native_bay_carrier_interlock_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(std::env::var_os("ARKLINE_BAY_JOURNEY_DIR").unwrap());
    let root = std::path::PathBuf::from(std::env::var_os("ARKLINE_BAY_GPU_DIR").unwrap());
    let name = std::env::var("ARKLINE_BAY_PILOT").unwrap();
    let authority: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let rows = authority["frames"].as_array().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let mut proof = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let target = CachedFeed::create(&gl, 910, 900, 650).unwrap();
        for index in [0, 150, 571, 1170] {
            let row = &rows[index];
            let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
            assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
            let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
            let mut app = CockpitCapture::new_for_pilot(gl.clone(), response.clone(), &name);
            for navigation in [false, true] {
                app.cockpit.services_open = navigation;
                app.cockpit.service_tab = crate::ServiceTab::Navigation;
                let file = format!(
                    "interlock-{index:04}-{}.png",
                    if navigation { "navigation" } else { "physical" }
                );
                let mut labels = Vec::new();
                for _ in 0..3 {
                    labels = app.capture_input(
                        &gl,
                        &target,
                        response.clone(),
                        &root.join(&file),
                        Some(vec![]),
                    );
                }
                assert!(app.cockpit.can_fly(), "STOP baseline remains available");
                assert_eq!(app.cockpit.bay_maneuver_reason().is_some(), index == 150);
                assert_eq!(
                    app.cockpit.bay_departure_reason().is_some(),
                    index == 150 || index == 571
                );
                assert!(app
                    ._requests
                    .try_iter()
                    .all(|r| matches!(r, Request::Snapshot)));
                if navigation && (index == 150 || index == 571) {
                    assert!(labels
                        .iter()
                        .any(|(text, _)| text.contains(if index == 150 {
                            "Drone bay handling"
                        } else {
                            "Drones deployed"
                        })));
                }
                proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),"authority_sha256":row["response_sha256"],"index":index,"viewport":[900,650],"maneuver_reason":app.cockpit.bay_maneuver_reason(),"departure_reason":app.cockpit.bay_departure_reason(),"stop_baseline":app.cockpit.can_fly(),"visible_text":labels.iter().map(|(text,_)|text).collect::<Vec<_>>()}));
            }
        }
    }
    std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"frames":proof,"authority_producer":authority["producer_commit"],"sampled_status_views":true,"not_live_performance_or_motion_proof":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "actual electrical authority responses and native software renderer required"]
fn native_electrical_accounting_capture() {
    use sha2::{Digest, Sha256};
    let source =
        std::path::PathBuf::from(std::env::var_os("ARKLINE_ELECTRICAL_RESPONSES").unwrap());
    let root = std::path::PathBuf::from(std::env::var_os("ARKLINE_ELECTRICAL_GPU_DIR").unwrap());
    let authority: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let pilot = authority["pilot"].as_str().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let mut proof = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        for (width, height) in [(900, 650), (1280, 800)] {
            let target = CachedFeed::create(&gl, 913, width, height).unwrap();
            for index in [0, 1, 4, 5, 7, 9, 11, 12] {
                let row = &authority["frames"][index];
                let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
                assert_eq!(hex::encode(Sha256::digest(&bytes)), row["sha256"]);
                let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
                let mut app = CockpitCapture::new_for_pilot(gl.clone(), response.clone(), pilot);
                app.cockpit.services_open = true;
                app.cockpit.service_tab = crate::ServiceTab::Hangar;
                app.cockpit.embedded_paused = row["paused"].as_bool().unwrap();
                let file = format!("{width}-accounting-{index:02}.png");
                let path = root.join(&file);
                let mut labels = Vec::new();
                for _ in 0..3 {
                    labels = app.capture_input(&gl, &target, response.clone(), &path, Some(vec![]));
                }
                let pos = labels
                    .iter()
                    .find(|(text, _)| {
                        text.contains("Electrical accounting · existing modeled circuits")
                    })
                    .expect("accounting expansion is visible in the existing Fleet workspace")
                    .1;
                for pressed in [true, false] {
                    app.capture_input(
                        &gl,
                        &target,
                        response.clone(),
                        &path,
                        Some(vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]),
                    );
                }
                for _ in 0..4 {
                    labels = app.capture_input(&gl, &target, response.clone(), &path, Some(vec![]));
                }
                // Fleet is a real scrolling workspace. Reveal the expanded budget
                // with a wheel input rather than moving HUD or shrinking its text.
                app.capture_input(
                    &gl,
                    &target,
                    response.clone(),
                    &path,
                    Some(vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0., -180.),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]),
                );
                for _ in 0..8 {
                    labels = app.capture_input(&gl, &target, response.clone(), &path, Some(vec![]));
                }
                for required in [
                    "Reactor rating:",
                    "Main-bus reservations:",
                    "Net available charging:",
                    "Model capacitor storage:",
                ] {
                    assert!(
                        labels.iter().any(|(text, _)| text.contains(required)),
                        "{width} frame {index}: missing {required}"
                    );
                }
                assert!(app
                    ._requests
                    .try_iter()
                    .all(|request| matches!(request, Request::Snapshot)));
                proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(&path).unwrap())),"authority_file":row["file"],"authority_sha256":row["sha256"],"viewport":[width,height],"paused":row["paused"],"visible_text":labels.iter().map(|(text,_)|text).collect::<Vec<_>>(),"camera":app.camera_receipt()}));
            }
        }
    }
    std::fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"frames":proof,"authority":authority,"sampled_command_states":true,"not_continuous_motion_or_measured_power":true})).unwrap()).unwrap();
}

#[test]
#[ignore = "actual authority lifecycle required; captures the production procedural synthesizer"]
fn actual_mining_lifecycle_engine_pcm_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(std::env::var_os("ARKLINE_BAY_JOURNEY_DIR").unwrap());
    let root = std::path::PathBuf::from(std::env::var_os("ARKLINE_BAY_AUDIO_DIR").unwrap());
    std::fs::create_dir_all(&root).unwrap();
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let name = std::env::var("ARKLINE_BAY_PILOT").unwrap();
    let id = receipt["drone_id"].as_u64().unwrap();
    let frames = receipt["frames"].as_array().unwrap();
    let mut scene = crate::sound_scene::Scene::default();
    let mut synth = crate::sound_engine::OfflineRenderer::new(48000);
    let mut samples = Vec::<i16>::new();
    let mut events = Vec::new();
    for (index, row) in frames.iter().enumerate() {
        let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
        assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
        let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
        let snapshot = response.snapshot.as_ref().unwrap();
        let pilot = snapshot.pilots.iter().find(|p| p.name == name).unwrap();
        let base = frame_for_pilot(
            snapshot,
            display_position(pilot, snapshot.server_time_ms),
            false,
            &name,
        );
        let view = mining_inspection_frame(&base, pilot, id);
        let listener = view.audio_listener().unwrap();
        let (mix, cues) = scene.update_spatial(
            Some(snapshot),
            &name,
            true,
            snapshot.server_time_ms,
            Some(listener),
            false,
        );
        events.push(serde_json::json!({"index":index,"authority_sha256":row["response_sha256"],"time_ms":snapshot.server_time_ms,"listener_position_m":listener.position_m,"listener_right":listener.right,"cues":format!("{cues:?}"),"mix":format!("{mix:?}")}));
        synth.update(mix, &cues);
        let count = frames.get(index + 1).map_or(0, |next| {
            ((next["time_ms"].as_u64().unwrap() - snapshot.server_time_ms) * 48) as usize
        });
        for frame in synth.render_frames(count) {
            for sample in frame {
                assert!(sample.is_finite() && sample.abs() <= 1.);
                samples.push((sample * 32767.).round() as i16);
            }
        }
    }
    let bytes = (samples.len() * 2) as u32;
    let mut wav = Vec::new();
    wav.extend(b"RIFF");
    wav.extend((36 + bytes).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend(1u16.to_le_bytes());
    wav.extend(2u16.to_le_bytes());
    wav.extend(48000u32.to_le_bytes());
    wav.extend(192000u32.to_le_bytes());
    wav.extend(4u16.to_le_bytes());
    wav.extend(16u16.to_le_bytes());
    wav.extend(b"data");
    wav.extend(bytes.to_le_bytes());
    for sample in &samples {
        wav.extend(sample.to_le_bytes());
    }
    std::fs::write(root.join("engine-listener.wav"), &wav).unwrap();
    std::fs::write(root.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"authority_receipt":receipt,"kind":"same production sound_scene and stereo Synth as CPAL; deterministic offline PCM, hardware playback not validated","listener":"continuous same-drone documentary inspection camera; no acoustic occlusion model","sample_rate":48000,"channels":2,"sample_frames":samples.len()/2,"duration_seconds":samples.len() as f64/96000.,"wav_sha256":hex::encode(Sha256::digest(&wav)),"events":events})).unwrap()).unwrap();
}

/// Actual production App/egui/GL bay monitor; authority responses are immutable.
/// Warmup passes exercise the normal shared feed budget, not a fabricated texture.
#[test]
#[ignore = "actual sampled cargo authority Responses and software EGL required"]
fn native_actual_ore_receiver_landmarks() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(std::env::var_os("ARKLINE_CARGO_LANDMARK_DIR").unwrap());
    let root = std::path::PathBuf::from(std::env::var_os("ARKLINE_CARGO_GPU_DIR").unwrap());
    let authority: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let name = authority["pilot"].as_str().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let mut proof = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let target = CachedFeed::create(&gl, 987, 960, 640).unwrap();
        for row in authority["frames"].as_array().unwrap() {
            let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
            assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
            let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
            assert!(response.ok);
            let snapshot = response.snapshot.as_ref().unwrap();
            let pilot = snapshot.pilots.iter().find(|p| p.name == name).unwrap();
            let cargo = drone_bay_scene::validated_cargo(pilot).expect("actual source-bound cargo");
            let base = frame_for_pilot(
                snapshot,
                display_position(pilot, snapshot.server_time_ms),
                false,
                name,
            );
            let frame = drone_bay_scene::operations_camera_frame(
                &base,
                pilot,
                BayMonitorCamera::OreReceiver,
                1.5,
            )
            .unwrap();
            let file = format!(
                "receiver-{}-{}.png",
                row["index"],
                row["label"].as_str().unwrap()
            );
            capture(&gl, &mut renderer, &target, &frame, &root.join(&file));
            proof.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),
                "authority_file":row["file"],"authority_sha256":row["response_sha256"],"label":row["label"],
                "server_time_ms":snapshot.server_time_ms,"cargo":cargo,"ship_ore":pilot.ore,
                "camera_eye":frame.eye.to_array(),"camera_vp":frame.vp.to_cols_array()}));
        }
    }
    std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "kind":"Actual sampled authority landmarks, production geometry and Ore receiver inspection camera; NOT a continuous movie or live FPS proof",
        "authority_receipt":authority,"frames":proof})).unwrap()).unwrap();
}

#[test]
#[ignore = "requires accepted batch Responses and actual software EGL"]
fn native_drone_operations_workspace_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(std::env::var_os("ARKLINE_BATCH_RESPONSES").unwrap());
    let root = std::path::PathBuf::from(std::env::var_os("ARKLINE_BATCH_UI_CAPTURE").unwrap());
    let authority: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let name = authority["pilot"].as_str().unwrap();
    let rows = authority
        .get("frames")
        .or_else(|| authority.get("responses"))
        .unwrap()
        .as_array()
        .unwrap();
    let indices: Vec<usize> = std::env::var("ARKLINE_BATCH_INDICES")
        .map(|s| s.split(',').map(|i| i.parse().unwrap()).collect())
        .unwrap_or_else(|_| (0..rows.len()).collect());
    std::fs::create_dir_all(&root).unwrap();
    let mut frames = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let widths: Vec<i32> = std::env::var("ARKLINE_BATCH_WIDTHS")
            .map(|s| s.split(',').map(|v| v.parse().unwrap()).collect())
            .unwrap_or_else(|_| vec![900, 1280]);
        for width in widths {
            let height = if width == 900 { 650 } else { 800 };
            let target = CachedFeed::create(&gl, 950 + width as u64, width, height).unwrap();
            for index in &indices {
                let row = &rows[*index];
                let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
                assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
                let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
                let mut app = CockpitCapture::new_for_pilot(gl.clone(), response.clone(), name);
                app.freeze_presentation = false;
                app.cockpit.services_open = true;
                app.cockpit.service_tab = crate::ServiceTab::Drones;
                if std::env::var_os("ARKLINE_ORE_UI_CAPTURE").is_some() {
                    app.cockpit.selected_drone = authority["drone_id"].as_u64();
                }
                let main_source = app.cockpit.camera_source;
                let views: &[&str] = if std::env::var_os("ARKLINE_ORE_UI_CAPTURE").is_some() {
                    &["wide", "doorway", "ore-receiver"]
                } else {
                    &["wide", "doorway"]
                };
                for view in views {
                    let doorway = *view == "doorway";
                    app.cockpit.bay_monitor_doorway = doorway;
                    let file = format!("operations-{width}-{index:04}-{view}.png");
                    let mut labels = Vec::new();
                    if *view == "ore-receiver" {
                        labels = app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&file),
                            Some(vec![]),
                        );
                        let point = labels
                            .iter()
                            .find(|(label, _)| label == "Ore receiver")
                            .expect("actual visible receiver selector")
                            .1;
                        for pressed in [true, false] {
                            app.capture_input(
                                &gl,
                                &target,
                                response.clone(),
                                &root.join(&file),
                                Some(vec![
                                    egui::Event::PointerMoved(point),
                                    egui::Event::PointerButton {
                                        pos: point,
                                        button: egui::PointerButton::Primary,
                                        pressed,
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ]),
                            );
                        }
                    }
                    for _ in 0..18 {
                        labels = app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&file),
                            Some(vec![]),
                        );
                    }
                    assert_eq!(
                        app.cockpit.camera_source, main_source,
                        "monitor cannot redirect captain camera"
                    );
                    assert!(
                        app._requests
                            .try_iter()
                            .all(|r| matches!(r, Request::Snapshot)),
                        "view-only monitor emitted gameplay"
                    );
                    assert!(
                        labels.iter().any(|(s, _)| s == "VIEW ONLY"),
                        "view-only monitor label must be visible"
                    );
                    assert!(
                        labels
                            .iter()
                            .any(|(s, _)| s.starts_with(if *view == "ore-receiver" {
                                "ORE RECEIVER"
                            } else if doorway {
                                "BAY DOORWAY"
                            } else {
                                "INTERIOR BAY"
                            })),
                        "entire camera caption must be visible below image"
                    );
                    assert!(
                        labels.iter().any(|(s, _)| s.starts_with("LIVE · image")
                            || s.starts_with("CACHED · image")),
                        "actual captured-image freshness must be visible"
                    );
                    if std::env::var_os("ARKLINE_ORE_UI_CAPTURE").is_some() {
                        let pilot = response
                            .snapshot
                            .as_ref()
                            .unwrap()
                            .pilots
                            .iter()
                            .find(|p| p.name == name)
                            .unwrap();
                        for home in pilot
                            .drone_bay
                            .as_ref()
                            .unwrap()
                            .cradles
                            .iter()
                            .filter(|c| c.class == arkline_protocol::DroneClass::Mining)
                        {
                            let expected =
                                format!("#{} · home {}", home.drone_id, home.home_cradle + 1);
                            assert!(
                                labels.iter().any(|(s, _)| s == &expected),
                                "all five roster rows must be visible: {expected}"
                            );
                        }
                    }
                    frames.push(serde_json::json!({"file":file,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),"authority_sha256":row["response_sha256"],"authority_index":index,"viewport":[width,height],"camera":view,"warmup_passes":18,"ui_clock":"60 Hz test passes; authority sample unchanged during warmup","presentation_frozen":false,"visible_text":labels.iter().map(|(s,_)|s).collect::<Vec<_>>()}));
                }
                if width == 900 && Some(index) == indices.first() {
                    let hidden_file = format!("operations-{width}-{index:04}-hidden.png");
                    let reopened_file = format!("operations-{width}-{index:04}-reopened.png");
                    let labels = app.capture_input(
                        &gl,
                        &target,
                        response.clone(),
                        &root.join(&hidden_file),
                        Some(vec![]),
                    );
                    let back = labels
                        .iter()
                        .find(|(text, _)| text == "Back to Flight")
                        .expect("visible Back control")
                        .1;
                    for pressed in [true, false] {
                        app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&hidden_file),
                            Some(vec![
                                egui::Event::PointerMoved(back),
                                egui::Event::PointerButton {
                                    pos: back,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ]),
                        );
                    }
                    assert!(
                        !app.cockpit.services_open,
                        "actual Back must hide workspace"
                    );
                    app.capture_input(
                        &gl,
                        &target,
                        response.clone(),
                        &root.join(&hidden_file),
                        Some(vec![]),
                    );
                    let drones = app
                        .cockpit
                        .flight_view
                        .computer_surface
                        .as_ref()
                        .expect("actual captain console")
                        .project(egui::pos2(280., 352.))
                        .expect("visible physical DRONES key");
                    for pressed in [true, false] {
                        app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&reopened_file),
                            Some(vec![
                                egui::Event::PointerMoved(drones),
                                egui::Event::PointerButton {
                                    pos: drones,
                                    button: egui::PointerButton::Primary,
                                    pressed,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ]),
                        );
                    }
                    assert!(
                        app.cockpit.services_open
                            && app.cockpit.service_tab == crate::ServiceTab::Drones,
                        "physical DRONES key must reopen workspace"
                    );
                    let mut labels = Vec::new();
                    for _ in 0..18 {
                        labels = app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&reopened_file),
                            Some(vec![]),
                        );
                    }
                    assert!(
                        labels.iter().any(|(s, _)| s.starts_with("LIVE · image")
                            || s.starts_with("CACHED · image")),
                        "reopened monitor must retain honest captured-image status"
                    );
                    assert_eq!(app.cockpit.camera_source, main_source);
                    assert!(
                        app._requests
                            .try_iter()
                            .all(|r| matches!(r, Request::Snapshot)),
                        "hide/reopen cannot issue gameplay"
                    );
                    for (file, kind) in [
                        (hidden_file, "actual Back pointer hides workspace"),
                        (
                            reopened_file,
                            "actual projected physical DRONES key reopens live monitor",
                        ),
                    ] {
                        frames.push(serde_json::json!({"file":file,"kind":kind,"sha256":hex::encode(Sha256::digest(std::fs::read(root.join(&file)).unwrap())),"authority_sha256":row["response_sha256"],"authority_index":index,"viewport":[width,height],"captain_camera_unchanged":true,"no_gameplay_requests":true}));
                    }
                }
            }
        }
    }
    std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({"kind":"production cockpit Drone operations UI with live-rendered bay monitor; sampled authority states, not continuous motion or real-time performance proof","authority_producer":authority["producer_commit"],"renderer":std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap_or_else(|_|"uncommitted".into()),"frames":frames})).unwrap()).unwrap();
}

#[test]
#[ignore = "explicit matched native panel organization proof from accepted Responses"]
fn native_cockpit_panel_organization_capture() {
    use sha2::{Digest, Sha256};
    let source = std::path::PathBuf::from(std::env::var_os("ARKLINE_PANEL_RESPONSES").unwrap());
    let root = std::path::PathBuf::from(std::env::var_os("ARKLINE_PANEL_CAPTURE").unwrap());
    let authority: serde_json::Value =
        serde_json::from_slice(&std::fs::read(source.join("receipt.json")).unwrap()).unwrap();
    let name = authority["pilot"].as_str().unwrap();
    let rows = authority["frames"].as_array().unwrap();
    std::fs::create_dir_all(&root).unwrap();
    let mut captures = Vec::new();
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        for width in [900, 1280] {
            let height = if width == 900 { 650 } else { 800 };
            let target = CachedFeed::create(&gl, 9010 + width as u64, width, height).unwrap();
            for index in [992usize, 2925] {
                let row = &rows[index];
                let bytes = std::fs::read(source.join(row["file"].as_str().unwrap())).unwrap();
                assert_eq!(hex::encode(Sha256::digest(&bytes)), row["response_sha256"]);
                let response: arkline_protocol::Response = serde_json::from_slice(&bytes).unwrap();
                for (label, tab) in [
                    ("navigation", crate::ServiceTab::Navigation),
                    ("mining", crate::ServiceTab::Modules),
                    ("exchange", crate::ServiceTab::Exchange),
                    ("ledger", crate::ServiceTab::Ledger),
                    ("settings", crate::ServiceTab::Settings),
                ] {
                    let mut app = CockpitCapture::new_for_pilot(gl.clone(), response.clone(), name);
                    app.freeze_presentation = true;
                    app.cockpit.services_open = true;
                    app.cockpit.service_tab = tab;
                    if let Some(id) = app
                        .cockpit
                        .own()
                        .and_then(|p| p.locked_asteroids.first().copied().or(p.locked_asteroid))
                    {
                        app.cockpit.selected_object = crate::SelectedObject::Asteroid(id);
                    }
                    let _ = app._requests.try_iter().collect::<Vec<_>>();
                    let file = format!("panels-{width}-{index:04}-{label}.png");
                    let mut labels = Vec::new();
                    for _ in 0..12 {
                        labels = app.capture_input(
                            &gl,
                            &target,
                            response.clone(),
                            &root.join(&file),
                            Some(vec![]),
                        );
                    }
                    assert!(
                        app._requests.try_recv().is_err(),
                        "passive panel capture must send no request"
                    );
                    assert!(
                        app.cockpit.audit_updates.is_none(),
                        "ledger capture must not start an audit"
                    );
                    assert!(!labels.is_empty());
                    let png = std::fs::read(root.join(&file)).unwrap();
                    captures.push(serde_json::json!({
                        "file":file, "sha256":hex::encode(Sha256::digest(&png)),
                        "width":width, "height":height, "panel":label,
                        "authority_index":index, "response_sha256":row["response_sha256"],
                        "snapshot_time_ms":response.snapshot.as_ref().unwrap().server_time_ms,
                        "camera":app.camera_receipt(), "visible_labels":labels.iter().map(|x|x.0.clone()).collect::<Vec<_>>()
                    }));
                    app.cockpit.flight_view.destroy();
                    app.cockpit.hangar_view.destroy();
                    app.painter.destroy();
                }
            }
            target.destroy(&gl);
        }
    }
    std::fs::write(root.join("receipt.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "kind":"Matched native full-App cockpit panel organization regression; no UI redesign or gameplay changes",
        "producer":std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap_or_else(|_|"unqualified".into()),
        "authority":authority["producer_commit"], "captures":captures,
        "scope":"Identical accepted Responses, frozen presentation time, viewport, selected object, panel and camera. Passive input only, no audit/network command."
    })).unwrap()).unwrap();
}

#[path = "carrier_inventory_native_tests.rs"]
mod carrier_inventory_native_tests;
