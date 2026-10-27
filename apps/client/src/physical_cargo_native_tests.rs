//! Opt-in actual owner-command trace and native rendering of physical-source 1704.
//! No network listener, synthetic pose, automatic gameplay admission or FPS claim.
use super::*;
use arkline_protocol::{
    physical_cargo::{
        bay::{certified_inspection_binding, PhysicalBayBinding, PhysicalBayRoute},
        catalog::{
            AuthoredCargoOperatingRatings, PhysicalCargoCatalog, PhysicalCargoForm,
            PhysicalCargoMaterial,
        },
        control::{PhysicalControlBinding, PHYSICAL_CONTROL_VERSION},
        source::physical_source_configuration,
        AuthoredVolumeRate, CargoMassKnowledge, VolumeCm3,
    },
    power_spec, PowerState, Request, Response, PROTOCOL_VERSION,
};
use arkline_server::physical_cargo::{control::PhysicalControlledWorld, PhysicalCargoGenesis};
use sha2::{Digest, Sha256};
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

const OWNER: &str = "captain";
const TOKEN: &str = "documentary-local-fixture-owner-token";

pub(crate) fn genesis() -> PhysicalCargoGenesis {
    let source = physical_source_configuration().unwrap();
    let rate = |volume_cm3, interval_ms| AuthoredVolumeRate {
        volume_cm3: VolumeCm3(volume_cm3),
        interval_ms,
    };
    let catalog = PhysicalCargoCatalog {
        schema_version: 1,
        materials: vec![PhysicalCargoMaterial {
            material_id: "controlled.capture.bulk".into(),
            form: PhysicalCargoForm::BulkVolumeCm3,
            mass: CargoMassKnowledge::Unknown,
            authored_price: None,
        }],
        operating_ratings: vec![AuthoredCargoOperatingRatings {
            rating_id: "controlled.capture.rate".into(),
            revision: 1,
            drone_extraction: rate(1000, 1000),
            ship_extraction: rate(1, 1000),
            vessel_manifold: rate(500, 500),
            drone_batch_energy_units: 2,
            ship_batch_energy_units: 1,
        }],
    };
    PhysicalCargoGenesis {
        source_configuration_sha256: source.identity_sha256().unwrap(),
        design: catalog
            .bind_manifest(
                "controlled.capture.bulk",
                "controlled.capture.rate",
                source.measurements().unwrap(),
            )
            .unwrap(),
        catalog,
        mining_drone_ids: vec![1],
        resource_cm3: VolumeCm3(10000),
    }
}
pub(crate) fn binding() -> PhysicalControlBinding {
    let spec = power_spec(ShipClass::Pioneer);
    PhysicalControlBinding {
        version: PHYSICAL_CONTROL_VERSION,
        pilot: OWNER.into(),
        drone_id: 1,
        target_id: 901,
        system_id: 1,
        initial_power: PowerState {
            charge_j: spec.capacity_j,
            capacity_j: spec.capacity_j,
            regen_w: spec.regen_w,
            updated_at_ms: 0,
            regen_remainder_mj: 0,
        },
        cycle_cost_j: spec.drone_mining_cycle_cost_j,
        frozen_load_w: 0,
    }
}
pub(crate) fn bay_binding() -> PhysicalBayBinding {
    certified_inspection_binding(1, 1000, 123).unwrap()
}
pub(crate) fn connect() -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        pilot: OWNER.into(),
        resume_token: Some(TOKEN.into()),
        world_layout_version: 1,
        supports_room_controls: true,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: true,
    }
}
pub(crate) fn initial_response_for_tests() -> Response {
    let mut world =
        PhysicalControlledWorld::open(":memory:", genesis(), bay_binding(), binding(), TOKEN, 0)
            .unwrap();
    let mut session = None;
    let response = world.handle_at(&mut session, connect(), 0);
    assert!(response.ok);
    response
}

pub(crate) fn options_for_tests(path: PathBuf) -> crate::PhysicalBayClientOptions {
    crate::PhysicalBayClientOptions {
        client_data_dir: path.with_extension("client"),
        world_path: path,
        genesis: genesis(),
        bay_binding: bay_binding(),
        control_binding: binding(),
        owner_token: TOKEN.into(),
        start_at_ms: 0,
    }
}

#[test]
fn actual_held_tick_keeps_source_pose_with_advancing_observation_clock() {
    let directory = std::env::temp_dir().join(format!(
        "arkline-physical-held-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let database = directory.join("world.sqlite");
    let mut world =
        PhysicalControlledWorld::open(&database, genesis(), bay_binding(), binding(), TOKEN, 0)
            .unwrap();
    let mut session = None;
    assert!(world.handle_at(&mut session, connect(), 0).ok);
    assert!(
        world
            .handle_at(
                &mut session,
                Request::LaunchDroneClass {
                    class: arkline_protocol::DroneClass::Mining,
                },
                0
            )
            .ok
    );
    world.tick_at(100).unwrap();
    let held = world.handle_at(&mut session, Request::DroneHoldById { id: 1 }, 100);
    assert!(held.ok);
    world.tick_at(200).unwrap();
    let next = world.handle_at(&mut session, Request::Snapshot, 200);
    assert!(next.ok);
    let before = held.physical_cargo.as_ref().unwrap();
    let view = next.physical_cargo.as_ref().unwrap();
    assert_eq!(before.source_frame, view.source_frame);
    assert_eq!(view.bay_state.as_ref().unwrap().last_step_ms, 100);
    assert_eq!(view.state.projection.accepted_at_ms, 200);
    physical_source_scene::Scene::from_view(next.snapshot.as_ref().unwrap(), view, OWNER).unwrap();
    let mut active_stale = view.clone();
    active_stale.bay_state.as_mut().unwrap().held = false;
    assert!(
        physical_source_scene::Scene::from_view(
            next.snapshot.as_ref().unwrap(),
            &active_stale,
            OWNER,
        )
        .is_err(),
        "Active motion cannot use a stale machinery clock"
    );
    let mut future = view.clone();
    future.bay_state.as_mut().unwrap().last_step_ms = 201;
    assert!(
        physical_source_scene::Scene::from_view(next.snapshot.as_ref().unwrap(), &future, OWNER,)
            .is_err(),
        "Future machinery clock must fail closed"
    );
    let export_before = serde_json::to_value(world.export().unwrap()).unwrap();
    let repeat = world.handle_at(&mut session, Request::Snapshot, 200);
    assert_eq!(
        serde_json::to_value(&repeat).unwrap(),
        serde_json::to_value(&next).unwrap()
    );
    assert_eq!(
        serde_json::to_value(world.export().unwrap()).unwrap(),
        export_before
    );
    drop(world);
    let mut world =
        PhysicalControlledWorld::open(&database, genesis(), bay_binding(), binding(), TOKEN, 1200)
            .unwrap();
    session = None;
    let reopened = world.handle_at(&mut session, connect(), 1200);
    assert!(reopened.ok);
    let reopened_view = reopened.physical_cargo.as_ref().unwrap();
    assert_eq!(reopened_view.source_frame, view.source_frame);
    physical_source_scene::Scene::from_view(
        reopened.snapshot.as_ref().unwrap(),
        reopened_view,
        OWNER,
    )
    .unwrap();
    let recalled = world.handle_at(&mut session, Request::RecallDroneById { id: 1 }, 1200);
    assert!(recalled.ok);
    assert!(
        !recalled
            .physical_cargo
            .as_ref()
            .unwrap()
            .bay_state
            .as_ref()
            .unwrap()
            .held
    );
    world.tick_at(1300).unwrap();
    let resumed = world.handle_at(&mut session, Request::Snapshot, 1300);
    assert!(resumed.ok);
    physical_source_scene::Scene::from_view(
        resumed.snapshot.as_ref().unwrap(),
        resumed.physical_cargo.as_ref().unwrap(),
        OWNER,
    )
    .unwrap();
    world.export().unwrap().verify().unwrap();
    drop(world);
    std::fs::remove_dir_all(directory).unwrap();
}

struct Trace {
    directory: PathBuf,
    rows: Vec<serde_json::Value>,
}
impl Trace {
    fn save(&mut self, response: Response, label: &str) -> Response {
        assert!(response.ok, "{}", response.message);
        let snapshot = response.snapshot.as_ref().unwrap();
        let view = response.physical_cargo.as_ref().unwrap();
        assert_eq!(
            snapshot.server_time_ms,
            view.state.projection.accepted_at_ms
        );
        let scene = physical_source_scene::Scene::from_view(snapshot, view, OWNER).unwrap_or_else(
            |error| {
                std::fs::write(
                    self.directory.join("rejected-response.json"),
                    serde_json::to_vec_pretty(&response).unwrap(),
                )
                .unwrap();
                panic!(
                    "Rejected actual response at {}ms ({label}): {error}",
                    snapshot.server_time_ms
                );
            },
        );
        let bay = view.bay_state.as_ref().unwrap();
        let expected_beam = view.state.projection.mining
            && view.state.projection.mining_cycle.is_some()
            && view.state.projection.target_locked
            && view.state.projection.signal_available
            && view.state.projection.resource_remaining_cm3.0 > 0
            && view.state.projection.hit_points > 0
            && bay.deployed
            && bay.route.is_none()
            && !bay.held;
        assert_eq!(
            scene.mining_beam().is_some(),
            expected_beam,
            "Only accepted current mining cycles may show a certified beam"
        );
        let file = format!("response-{:05}.json", self.rows.len());
        let bytes = serde_json::to_vec_pretty(&response).unwrap();
        std::fs::write(self.directory.join(&file), &bytes).unwrap();
        self.rows.push(serde_json::json!({
            "index":self.rows.len(), "file":file, "sha256":hex::encode(Sha256::digest(&bytes)),
            "simulation_ms":snapshot.server_time_ms, "label":label,
            "ship_payload_cm3":view.ship_payload_cm3.0,
            "drone_payload_cm3":view.drone_payload_cm3.0,
            "drone_inlet_travel_mm":view.drone_inlet_travel_mm,
            "ore_coupled":view.source_frame.milestones.ore_coupled,
            "certified_mining_beam":expected_beam,
            "resource_remaining_cm3":view.state.projection.resource_remaining_cm3.0,
            "charge_j":view.state.projection.power.charge_j,
            "ledger_sequence":snapshot.ledger_sequence,
            "ledger_hash":snapshot.ledger_hash,
        }));
        response
    }
    fn request(
        &mut self,
        world: &mut PhysicalControlledWorld,
        session: &mut Option<String>,
        request: Request,
        now: u64,
        label: &str,
    ) -> Response {
        self.save(world.handle_at(session, request, now), label)
    }
    fn step(
        &mut self,
        world: &mut PhysicalControlledWorld,
        session: &mut Option<String>,
        now: &mut u64,
        label: &str,
    ) -> Response {
        *now += 100;
        world.tick_at(*now).unwrap();
        self.request(world, session, Request::Snapshot, *now, label)
    }
}

#[test]
#[ignore = "Explicit authenticated source-bound physical1704 command trace; no renderer or FPS claim"]
fn physical1704_actual_authority_trace() {
    let directory = PathBuf::from(std::env::var_os("ARKLINE_PHYSICAL_TRACE").unwrap());
    std::fs::create_dir(&directory).unwrap();
    let producer = std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap();
    assert_eq!(producer.len(), 40);
    assert!(producer.bytes().all(|b| b.is_ascii_hexdigit()));
    let database = directory.join("controlled.sqlite");
    let mut world =
        PhysicalControlledWorld::open(&database, genesis(), bay_binding(), binding(), TOKEN, 0)
            .unwrap();
    let mut trace = Trace {
        directory: directory.clone(),
        rows: vec![],
    };
    let mut session = None;
    let mut now = 0;
    trace.request(&mut world, &mut session, connect(), now, "connected-stored");
    trace.request(
        &mut world,
        &mut session,
        Request::LockAsteroid { id: 901 },
        now,
        "target-lock",
    );
    trace.request(
        &mut world,
        &mut session,
        Request::LaunchDroneClass {
            class: arkline_protocol::DroneClass::Mining,
        },
        now,
        "launch-command",
    );
    for _ in 0..200 {
        let export = world.export().unwrap();
        let bay = export.bay.as_ref().unwrap();
        if bay.deployed
            && bay.route.is_none()
            && export.control.as_ref().unwrap().projection.target_locked
        {
            break;
        }
        trace.step(&mut world, &mut session, &mut now, "launch");
    }
    let export = world.export().unwrap();
    assert!(export.bay.as_ref().unwrap().deployed);
    assert!(export.bay.as_ref().unwrap().route.is_none());
    trace.request(
        &mut world,
        &mut session,
        Request::AssignMiningDrone {
            id: 1,
            target_id: 901,
        },
        now,
        "assign-mining",
    );
    for _ in 0..700 {
        if world.export().unwrap().account.ship_payload_cm3.0 >= 2000 {
            break;
        }
        trace.step(
            &mut world,
            &mut session,
            &mut now,
            "mining-service-redeploy",
        );
    }
    assert_eq!(
        world.export().unwrap().account.ship_payload_cm3,
        VolumeCm3(2000)
    );
    trace.request(
        &mut world,
        &mut session,
        Request::RecallDroneById { id: 1 },
        now,
        "recall-command",
    );
    for _ in 0..400 {
        let export = world.export().unwrap();
        let bay = export.bay.as_ref().unwrap();
        if !bay.deployed && bay.route.is_none() {
            break;
        }
        trace.step(&mut world, &mut session, &mut now, "protected-return");
    }
    let export = world.export().unwrap();
    let bay = export.bay.as_ref().unwrap();
    assert!(!bay.deployed && bay.route.is_none());
    assert_eq!(export.account.ship_payload_cm3, VolumeCm3(2000));
    assert_eq!(export.resource_remaining_cm3, VolumeCm3(8000));
    export.verify().unwrap();
    let movie_end_ms = now;

    // A separate real command/restart section follows the complete first trip.
    trace.request(
        &mut world,
        &mut session,
        Request::LaunchDroneClass {
            class: arkline_protocol::DroneClass::Mining,
        },
        now,
        "second-launch",
    );
    for _ in 0..15 {
        trace.step(&mut world, &mut session, &mut now, "second-launch");
    }
    trace.request(
        &mut world,
        &mut session,
        Request::DroneHoldById { id: 1 },
        now,
        "hold",
    );
    let before = world.export().unwrap();
    let held = trace.request(
        &mut world,
        &mut session,
        Request::Snapshot,
        now,
        "held-state",
    );
    for _ in 0..10 {
        trace.step(&mut world, &mut session, &mut now, "held");
    }
    let still = trace.request(
        &mut world,
        &mut session,
        Request::Snapshot,
        now,
        "held-state",
    );
    assert_eq!(
        held.physical_cargo.as_ref().unwrap().source_frame,
        still.physical_cargo.as_ref().unwrap().source_frame
    );
    drop(world);
    now += 10000;
    let mut world =
        PhysicalControlledWorld::open(&database, genesis(), bay_binding(), binding(), TOKEN, now)
            .unwrap();
    session = None;
    let reopened = trace.request(&mut world, &mut session, connect(), now, "reopened-frozen");
    assert_eq!(
        held.physical_cargo.as_ref().unwrap().source_frame,
        reopened.physical_cargo.as_ref().unwrap().source_frame
    );
    assert_eq!(world.export().unwrap().account, before.account);
    trace.request(
        &mut world,
        &mut session,
        Request::RecallDroneById { id: 1 },
        now,
        "post-restart-recall",
    );
    for _ in 0..400 {
        let export = world.export().unwrap();
        let bay = export.bay.as_ref().unwrap();
        if !bay.deployed && bay.route.is_none() {
            break;
        }
        trace.step(
            &mut world,
            &mut session,
            &mut now,
            "post-restart-protected-return",
        );
    }
    let final_export = world.export().unwrap();
    let final_bay = final_export.bay.as_ref().unwrap();
    assert!(!final_bay.deployed && final_bay.route.is_none());
    assert!(final_bay.route != Some(PhysicalBayRoute::Service));
    final_export.verify().unwrap();
    let before = serde_json::to_vec(&final_export).unwrap();
    let response = trace.request(
        &mut world,
        &mut session,
        Request::Snapshot,
        now,
        "final-stored",
    );
    trace.request(
        &mut world,
        &mut session,
        Request::Snapshot,
        now,
        "repeat-observation",
    );
    assert_eq!(
        serde_json::to_vec(&world.export().unwrap()).unwrap(),
        before
    );
    let export_bytes = serde_json::to_vec_pretty(&final_export).unwrap();
    std::fs::write(directory.join("verified-export.json"), &export_bytes).unwrap();
    let source = physical_source_configuration().unwrap();
    let receipt = serde_json::json!({
        "format_version":1, "result":"passed", "producer_commit":producer,
        "pilot":OWNER, "source_configuration_sha256":source.identity_sha256().unwrap(),
        "source_assets":source.assets,
        "bay_binding":bay_binding(), "control_binding":binding(),
        "scope":"Authenticated fixed-world bay0; actual Server commands/ticks; authored target/rates; no public listener or fleet/warp acceptance",
        "frame_step_ms":100, "movie_end_ms":movie_end_ms, "final_ms":now,
        "frames":trace.rows, "export_sha256":hex::encode(Sha256::digest(&export_bytes)),
        "final_response":response,
        "no_offline_catchup":true, "repeated_snapshot_no_mutation":true,
        "mass_rating":"unknown", "normal_gameplay_admission":false
    });
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[cfg(target_os = "linux")]
fn recorded_response(directory: &Path, row: &serde_json::Value) -> Response {
    let bytes = std::fs::read(directory.join(row["file"].as_str().unwrap())).unwrap();
    assert_eq!(hex::encode(Sha256::digest(&bytes)), row["sha256"]);
    let response: Response = serde_json::from_slice(&bytes).unwrap();
    assert!(response.ok);
    assert_eq!(
        response.snapshot.as_ref().unwrap().server_time_ms,
        row["simulation_ms"]
    );
    response
}
#[cfg(target_os = "linux")]
pub(super) fn physical_frame(response: &Response) -> Frame {
    let snapshot = response.snapshot.as_ref().unwrap();
    let view = response.physical_cargo.as_ref().unwrap();
    let mut frame = super::tests::frame_for_scene_clone_test();
    let eye = Vec3::new(-40., 12., -61.) / 100.;
    let target = Vec3::new(-26., 0., -29.) / 100.;
    let vp = Mat4::orthographic_rh_gl(-0.21, 0.21, -0.14, 0.14, 0.001, 8.)
        * Mat4::look_at_rh(eye, target, Vec3::Y);
    frame.pioneer_parts = None;
    frame.exterior = None;
    frame.snapshot_time = snapshot.server_time_ms;
    frame.now = snapshot.server_time_ms;
    frame.own = OWNER.into();
    frame.system_id = view.state.binding.system_id;
    frame.ship_class = ShipClass::Pioneer;
    frame.cockpit = false;
    frame.connected = true;
    frame.paused = false;
    frame.space = Space { origin: [0.; 3] };
    frame.eye = eye;
    frame.vp = vp;
    frame.cockpit_vp = vp;
    frame.planet_vp = vp;
    frame.lighting_quality = crate::lighting_settings::LightingQuality::High;
    frame.scene = Arc::new(FrameScene {
        physical: Some(Arc::new(
            physical_source_scene::Scene::from_view(snapshot, view, OWNER).unwrap(),
        )),
        carrier_inventory_registration: None,
        world_layout: arkline_protocol::legacy_world_layout(),
        traffic: snapshot.traffic.clone(),
        surface_props: snapshot.surface_props.clone(),
        asteroids: snapshot.asteroids.clone(),
        gates: vec![],
        pilots: snapshot.pilots.clone(),
    });
    frame
}

#[cfg(target_os = "linux")]
struct PanelCapture {
    app: crate::Cockpit,
    ctx: egui::Context,
    painter: egui_glow::Painter,
    updates: std::sync::mpsc::Sender<crate::Update>,
    requests: std::sync::mpsc::Receiver<Request>,
}
#[cfg(target_os = "linux")]
impl PanelCapture {
    unsafe fn new(gl: Arc<glow::Context>, profile: PathBuf) -> Self {
        let ctx = egui::Context::default();
        ctx.style_mut(|s| s.animation_time = 0.);
        let mut app = crate::tests::view_test_cockpit_with_profile(&ctx, profile);
        app.pilot = OWNER.into();
        app.physical_session = true;
        app.snapshot = None;
        app.assets = None;
        app.flight_view = FlightView::new(Some(&gl));
        app.connected = true;
        app.connecting = false;
        app.request_pending = false;
        let (commands, requests) = std::sync::mpsc::channel();
        app.commands = Some(commands);
        let (updates, receiver) = std::sync::mpsc::channel();
        app.updates = Some(receiver);
        Self {
            app,
            ctx,
            painter: egui_glow::Painter::new(gl, "", None, false).unwrap(),
            updates,
            requests,
        }
    }
    unsafe fn paint(
        &mut self,
        gl: &glow::Context,
        target: &CachedFeed,
        response: &Response,
        events: Vec<egui::Event>,
        time: f64,
    ) -> Vec<(String, egui::Pos2)> {
        self.updates
            .send(crate::Update::Response(Box::new(response.clone()), false))
            .unwrap();
        self.app.poll();
        assert!(
            self.app.physical_cargo.is_some(),
            "Accepted physical response must bind client state"
        );
        self.app.frozen_time = Some(response.snapshot.as_ref().unwrap().server_time_ms);
        let original =
            serde_json::to_value((&self.app.snapshot, &self.app.physical_cargo)).unwrap();
        let pilot = self.app.own().unwrap();
        let output = self.ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(target.width as f32, target.height as f32),
                )),
                time: Some(time),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::Window::new("PIONEER · PHYSICAL BAY 01")
                    .fade_in(false)
                    .fade_out(false)
                    .fixed_pos(egui::pos2(610., 12.))
                    .fixed_size(egui::vec2(335., 510.))
                    .resizable(false)
                    .collapsible(false)
                    .show(ctx, |ui| {
                        self.app.physical_cargo_controls(ui, &pilot);
                        ui.separator();
                        ui.small("Native inspection capture · source 1704");
                        ui.small("One bay · authored fixed target/rates · not general gameplay");
                    });
            },
        );
        assert_eq!(
            serde_json::to_value((&self.app.snapshot, &self.app.physical_cargo)).unwrap(),
            original
        );
        let labels = output
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
        self.painter.paint_and_update_textures(
            [target.width as u32, target.height as u32],
            output.pixels_per_point,
            &primitives,
            &output.textures_delta,
        );
        labels
    }
}
#[cfg(target_os = "linux")]
unsafe fn read_pixels(gl: &glow::Context, width: i32, height: i32) -> Vec<u8> {
    let mut rgba = vec![0; (width * height * 4) as usize];
    gl.read_pixels(
        0,
        0,
        width,
        height,
        glow::RGBA,
        glow::UNSIGNED_BYTE,
        glow::PixelPackData::Slice(Some(&mut rgba)),
    );
    assert_eq!(gl.get_error(), glow::NO_ERROR);
    rgba.chunks_exact(width as usize * 4)
        .rev()
        .flatten()
        .copied()
        .collect()
}

#[test]
#[cfg(target_os = "linux")]
#[ignore = "Actual production Renderer and control panel over authenticated1704 response trace; Mesa EGL required"]
fn native_physical1704_recorded_capture() {
    let input = PathBuf::from(std::env::var_os("ARKLINE_PHYSICAL_TRACE").unwrap());
    let output = PathBuf::from(std::env::var_os("ARKLINE_PHYSICAL_CAPTURE").unwrap());
    std::fs::create_dir(&output).unwrap();
    let receipt_bytes = std::fs::read(input.join("receipt.json")).unwrap();
    let receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(receipt["result"], "passed");
    let from = std::env::var("ARKLINE_PHYSICAL_CAPTURE_FROM_MS")
        .unwrap_or_else(|_| "0".into())
        .parse::<u64>()
        .unwrap();
    let to = std::env::var("ARKLINE_PHYSICAL_CAPTURE_TO_MS")
        .ok()
        .map(|x| x.parse::<u64>().unwrap())
        .unwrap_or_else(|| receipt["movie_end_ms"].as_u64().unwrap());
    let stride = std::env::var("ARKLINE_PHYSICAL_CAPTURE_STRIDE")
        .unwrap_or_else(|_| "1".into())
        .parse::<usize>()
        .unwrap();
    assert!(stride > 0 && from <= to);
    // Keep the last accepted command state at each actual clock. No frames are interpolated.
    let mut clocks = std::collections::BTreeMap::new();
    for row in receipt["frames"].as_array().unwrap() {
        let ms = row["simulation_ms"].as_u64().unwrap();
        if (from..=to).contains(&ms) {
            clocks.insert(ms, row);
        }
    }
    let selected: Vec<_> = clocks.into_iter().step_by(stride).collect();
    assert!(!selected.is_empty());
    let renderer_source = std::env::var("ARKLINE_CAPTURE_SOURCE").unwrap();
    let mut frames = vec![];
    unsafe {
        let (_egl, gl) = super::exterior_gpu_tests::Egl::create();
        let mut renderer = Renderer::new(gl.clone()).unwrap();
        let base_meshes = flight_geometry().unwrap().catalog.uploads().len();
        assert_eq!(renderer.meshes.len(), base_meshes);
        assert_eq!(renderer.depth_meshes.uploads().len(), base_meshes);

        let target = CachedFeed::create(&gl, 170400, 960, 640).unwrap();
        let mut panel = PanelCapture::new(gl.clone(), output.join("isolated-client-profile"));
        for (index, (ms, row)) in selected.iter().enumerate() {
            let response = recorded_response(&input, row);
            let before = serde_json::to_vec(&response).unwrap();
            let frame = physical_frame(&response);
            if index == 0 {
                // egui first lays out a new Window invisibly. Warm up only this
                // same accepted observation, never advance authority or time.
                panel.paint(&gl, &target, &response, vec![], 0.);
                assert!(panel.requests.try_recv().is_err());
            }

            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(target.framebuffer));
            gl.viewport(0, 0, 960, 640);
            gl.disable(glow::SCISSOR_TEST);
            renderer.paint(&frame, Some(target.framebuffer));
            let physical_meshes = physical_geometry().unwrap().catalog.uploads().len();
            assert!(physical_meshes > base_meshes);
            assert_eq!(renderer.meshes.len(), physical_meshes);
            assert_eq!(renderer.depth_meshes.uploads().len(), physical_meshes);
            // A second paint must not append or replace source geometry again.
            renderer.paint(&frame, Some(target.framebuffer));
            assert_eq!(renderer.meshes.len(), physical_meshes);
            assert_eq!(renderer.depth_meshes.uploads().len(), physical_meshes);

            let scene_rgba = read_pixels(&gl, 960, 640);
            let labels = panel.paint(&gl, &target, &response, vec![], index as f64 / 10.);
            assert!(
                labels
                    .iter()
                    .any(|(text, _)| text.starts_with("Ship hold:")),
                "Captured frame must contain visible authoritative payload text"
            );

            gl.finish();
            let rgba = read_pixels(&gl, 960, 640);
            let changed_panel_pixels = (12usize..522)
                .flat_map(|y| (610usize..945).map(move |x| (y * 960 + x) * 4))
                .filter(|&i| scene_rgba[i..i + 4] != rgba[i..i + 4])
                .count();
            assert!(
                changed_panel_pixels > 1000,
                "Actual egui pixels must reach the captured framebuffer, not just text metadata"
            );

            assert_eq!(serde_json::to_vec(&response).unwrap(), before);
            assert!(
                panel.requests.try_recv().is_err(),
                "Observation cannot issue a command"
            );
            let file = format!("frame-{index:05}.png");
            image::save_buffer(output.join(&file), &rgba, 960, 640, image::ColorType::Rgba8)
                .unwrap();
            frames.push(serde_json::json!({
                "index":index,"simulation_ms":ms,"file":file,
                "response_file":row["file"],"response_sha256":row["sha256"],
                "pixel_sha256":hex::encode(Sha256::digest(&rgba)),
                "labels":labels.iter().map(|(s,_)|s).collect::<Vec<_>>(),
                "view_projection":frame.vp.to_cols_array(),
                "eye_scene":frame.eye.to_array(),
                "authority_unchanged":true,"commands_sent":0,"changed_panel_pixels":changed_panel_pixels
            }));
        }
        // Explicit same-state monitor visibility only. It must not hold equipment.
        let response = recorded_response(&input, selected.last().unwrap().1);
        let mut labels = panel.paint(&gl, &target, &response, vec![], 1000.);
        for (step, label) in [
            "Hide monitor",
            "Show monitor",
            "Hide monitor",
            "Show monitor",
        ]
        .iter()
        .enumerate()
        {
            let pos = labels
                .iter()
                .find(|(text, _)| text == label)
                .unwrap_or_else(|| panic!("{label} absent from {labels:?}"))
                .1;
            for pressed in [true, false] {
                panel.paint(
                    &gl,
                    &target,
                    &response,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    1001. + step as f64 + if pressed { 0. } else { 0.1 },
                );
            }
            assert!(
                panel.requests.try_recv().is_err(),
                "Monitor visibility must not send a command"
            );
            labels = panel.paint(&gl, &target, &response, vec![], 1001.5 + step as f64);
        }
        let pos = labels
            .iter()
            .find(|(text, _)| text == "Hold")
            .expect("Hold control visible")
            .1;
        for pressed in [true, false] {
            panel.paint(
                &gl,
                &target,
                &response,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                if pressed { 1007. } else { 1007.1 },
            );
        }
        assert!(matches!(
            panel.requests.try_recv().unwrap(),
            Request::DroneHoldById { id: 1 }
        ));
        assert!(
            panel.requests.try_recv().is_err(),
            "One click must enqueue exactly one existing request"
        );
        renderer.destroy();
        target.destroy(&gl);
    }
    let report = serde_json::json!({
        "result":"passed", "producer_commit":receipt["producer_commit"],"renderer_commit":renderer_source,
        "input_receipt_sha256":hex::encode(Sha256::digest(&receipt_bytes)),
        "source_configuration_sha256":receipt["source_configuration_sha256"],
        "scope":"Actual production Renderer::paint and existing physical cargo panel, fixed external inspection camera; not OS window, default cockpit camera, public gameplay or FPS acceptance",
        "geometry_or_authority_mutated":false,"interpolated_frames":0,"sample_stride":stride,
        "dimensions":[960,640],"monitor_hide_show_no_commands":true,"hold_click_existing_request_only":true,"frames":frames
    });
    std::fs::write(
        output.join("capture-receipt.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
}

#[cfg(target_os = "linux")]
mod live {
    use super::*;
    include!("physical_live_tests.rs");
}
