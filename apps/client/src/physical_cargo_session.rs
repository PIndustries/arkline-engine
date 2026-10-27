//! Explicit local-only bridge to the existing physical authority and cockpit.
//! No listener, generated credential, prototype migration or normal-world flag.
use super::*;
use arkline_protocol::physical_cargo::{bay::PhysicalBayBinding, control::PhysicalControlBinding};
use arkline_server::physical_cargo::{control::PhysicalControlledWorld, PhysicalCargoGenesis};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Caller-selected fresh configuration or the exact original saved-world policy.
/// Credentials are provided by the caller and are never logged or saved by this bridge.
pub struct PhysicalBayClientOptions {
    pub world_path: PathBuf,
    pub client_data_dir: PathBuf,
    pub genesis: PhysicalCargoGenesis,
    pub bay_binding: PhysicalBayBinding,
    pub control_binding: PhysicalControlBinding,
    pub owner_token: String,
    pub start_at_ms: u64,
}
struct Worker {
    stop: Arc<AtomicBool>,
    join: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}
struct Bridge {
    worker: Worker,
    commands: Sender<Request>,
    updates: Receiver<Update>,
}
fn start(options: PhysicalBayClientOptions) -> Bridge {
    let (commands, requests) = std::sync::mpsc::channel();
    let (updates_tx, updates) = std::sync::mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let join = std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut now = options.start_at_ms;
            let pilot = options.control_binding.pilot.clone();
            let mut world = PhysicalControlledWorld::open(
                &options.world_path,
                options.genesis,
                options.bay_binding,
                options.control_binding,
                &options.owner_token,
                now,
            )
            .map_err(|e| e.to_string())?;
            let mut owner = None;
            let response = world.handle_at(
                &mut owner,
                Request::Connect {
                    protocol_version: PROTOCOL_VERSION,
                    pilot,
                    resume_token: Some(options.owner_token),
                    world_layout_version: 1,
                    supports_room_controls: true,
                    supports_fleet_hangar: true,
                    supports_pioneer_interior: true,
                    supports_docked_traversal_v2: true,
                },
                now,
            );
            if !response.ok {
                return Err(response.message);
            }
            updates_tx.send(Update::Ready).map_err(|e| e.to_string())?;
            updates_tx
                .send(Update::PhysicalResponse(
                    Box::new(response),
                    true,
                    Instant::now(),
                    None,
                ))
                .map_err(|e| e.to_string())?;
            let observation_pending = Arc::new(AtomicBool::new(false));
            let interval = std::time::Duration::from_millis(100);
            let mut next = std::time::Instant::now() + interval;
            while !stopping.load(Ordering::Acquire) {
                let wait = next.saturating_duration_since(std::time::Instant::now());
                match requests.recv_timeout(wait) {
                    Ok(request) => {
                        if stopping.load(Ordering::Acquire) {
                            break;
                        }
                        let response = world.handle_at(&mut owner, request, now);
                        updates_tx
                            .send(Update::PhysicalResponse(
                                Box::new(response),
                                true,
                                Instant::now(),
                                None,
                            ))
                            .map_err(|e| e.to_string())?;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                if std::time::Instant::now() >= next {
                    // Exactly one accepted segment, never burst/catch-up after a slow frame.
                    now = now
                        .checked_add(100)
                        .ok_or("Physical preview clock exhausted")?;
                    world.tick_at(now).map_err(|e| e.to_string())?;
                    // Backpressure observations only: keep ticking/committing authority
                    // while the UI is busy, without building an unbounded stale queue.
                    // No command response, error or ledger transition is discarded.
                    if !observation_pending.load(Ordering::Acquire) {
                        let response = world.handle_at(&mut owner, Request::Snapshot, now);
                        if !response.ok {
                            return Err(response.message);
                        }
                        observation_pending.store(true, Ordering::Release);
                        updates_tx
                            .send(Update::PhysicalResponse(
                                Box::new(response),
                                false,
                                Instant::now(),
                                Some(observation_pending.clone()),
                            ))
                            .map_err(|e| e.to_string())?;
                    }
                    next = std::time::Instant::now() + interval;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            let _ = updates_tx.send(Update::Failure(error));
        }
    });
    Bridge {
        worker: Worker {
            stop,
            join: Some(join),
        },
        commands,
        updates,
    }
}

#[cfg(test)]
pub(crate) fn live_bridge_for_tests(
    options: PhysicalBayClientOptions,
) -> (
    Sender<Request>,
    Receiver<Update>,
    Arc<AtomicBool>,
    impl Drop,
) {
    let bridge = start(options);
    let stop = bridge.worker.stop.clone();
    (bridge.commands, bridge.updates, stop, bridge.worker)
}

/// Existing native cockpit, backed by an explicit same-process local authority.
/// This can be embedded by tooling without opening any network transport.
pub struct PhysicalBayClient {
    cockpit: Cockpit,
    _worker: Worker,
}
impl PhysicalBayClient {
    pub fn new(cc: &eframe::CreationContext<'_>, options: PhysicalBayClientOptions) -> Self {
        let mut cockpit = Cockpit::new(
            cc,
            "local physical bay / no listener".into(),
            options.client_data_dir.clone(),
            None,
        );
        cockpit.physical_session = true;
        cockpit.pilot = options.control_binding.pilot.clone();
        cockpit.connecting = true;
        cockpit.boot.connect();
        let bridge = start(options);
        cockpit.physical_stop = Some(bridge.worker.stop.clone());
        cockpit.commands = Some(bridge.commands);
        cockpit.updates = Some(bridge.updates);
        Self {
            cockpit,
            _worker: bridge.worker,
        }
    }
    pub fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        eframe::App::update(&mut self.cockpit, ctx, frame);
    }
}
impl eframe::App for PhysicalBayClient {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.update(ctx, frame);
    }
}
/// Explicit native inspection launcher. Ordinary run_native/browser modes do not select it.
pub fn run_physical_bay_preview(options: PhysicalBayClientOptions) -> eframe::Result<()> {
    eframe::run_native(
        "Arkline · physical bay 01 inspection",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            depth_buffer: 24,
            viewport: egui::ViewportBuilder::default().with_inner_size([1280., 900.]),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(PhysicalBayClient::new(cc, options)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flight3d::physical_cargo_native_tests::options_for_tests;
    fn visible(bridge: &Bridge) -> Response {
        loop {
            match bridge
                .updates
                .recv_timeout(std::time::Duration::from_secs(15))
                .expect("local bridge response")
            {
                Update::PhysicalResponse(response, true, _, _) => return *response,
                Update::Failure(error) => panic!("{error}"),
                _ => {}
            }
        }
    }
    #[test]
    fn local_bridge_authenticates_and_routes_existing_commands_without_listener() {
        let bridge = start(options_for_tests(PathBuf::from(":memory:")));
        let initial = visible(&bridge);
        let owner = initial
            .physical_cargo
            .as_ref()
            .unwrap()
            .state
            .binding
            .pilot
            .clone();
        crate::physical_cargo_ui::validate(
            initial.physical_cargo.as_ref().unwrap(),
            initial.snapshot.as_ref().unwrap(),
            &owner,
        )
        .unwrap();
        bridge
            .commands
            .send(Request::LaunchDroneClass {
                class: DroneClass::Mining,
            })
            .unwrap();
        let launch = visible(&bridge);
        assert!(launch.ok, "{}", launch.message);
        assert_eq!(
            launch.physical_cargo.unwrap().bay_state.unwrap().route,
            Some(arkline_protocol::physical_cargo::bay::PhysicalBayRoute::Launch)
        );
        bridge.commands.send(Request::Undock).unwrap();
        assert!(!visible(&bridge).ok);
        drop(bridge);
    }
    #[test]
    fn failed_local_policy_reports_failure_not_ready_or_transport_fallback() {
        let mut options = options_for_tests(PathBuf::from(":memory:"));
        options.owner_token = "short".into();
        let bridge = start(options);
        assert!(matches!(
            bridge
                .updates
                .recv_timeout(std::time::Duration::from_secs(15))
                .unwrap(),
            Update::Failure(_)
        ));
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::flight3d::physical_cargo_native_tests::{
        initial_response_for_tests, options_for_tests,
    };

    fn fixture(
        ctx: &egui::Context,
    ) -> (Cockpit, Arc<AtomicBool>, Receiver<Request>, Sender<Update>) {
        let mut cockpit = crate::tests::view_test_cockpit(ctx);
        let response = initial_response_for_tests();
        cockpit.pilot = response
            .physical_cargo
            .as_ref()
            .unwrap()
            .state
            .binding
            .pilot
            .clone();
        cockpit.physical_session = true;
        cockpit.snapshot = None;
        cockpit
            .accept_physical_response(response.physical_cargo, response.snapshot.as_ref().unwrap())
            .unwrap();
        cockpit.snapshot = response.snapshot;
        cockpit.performance_snapshot_received = Some(Instant::now());
        let stop = Arc::new(AtomicBool::new(false));
        cockpit.physical_stop = Some(stop.clone());
        let (commands, requests) = std::sync::mpsc::channel();
        let (updates, receiver) = std::sync::mpsc::channel();
        cockpit.commands = Some(commands);
        cockpit.updates = Some(receiver);
        (cockpit, stop, requests, updates)
    }
    fn frame(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        events: Vec<egui::Event>,
        full: bool,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1280., 900.),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                if full {
                    eframe::App::update(cockpit, ctx, &mut eframe::Frame::_new_kittest());
                } else {
                    egui::CentralPanel::default().show(ctx, |ui| cockpit.session_menu(ui));
                }
            },
        )
    }
    fn point(output: &egui::FullOutput, label: &str) -> Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing pointer target {label}"))
    }
    fn click(ctx: &egui::Context, cockpit: &mut Cockpit, pos: Pos2, full: bool) {
        frame(ctx, cockpit, vec![egui::Event::PointerMoved(pos)], full);
        for pressed in [true, false] {
            frame(
                ctx,
                cockpit,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                full,
            );
        }
    }
    fn revoked(cockpit: &Cockpit, stop: &AtomicBool, requests: &Receiver<Request>) {
        assert!(stop.load(Ordering::Acquire));
        assert!(cockpit.snapshot.is_none() && cockpit.physical_cargo.is_none());
        assert!(cockpit.commands.is_none() && cockpit.updates.is_none());
        assert!(!cockpit.connected && !cockpit.connecting && !cockpit.request_pending);
        assert!(cockpit.performance_snapshot_received.is_none());
        assert!(!cockpit.can_operate());
        assert!(
            requests.try_recv().is_err(),
            "teardown must not enqueue gameplay"
        );
    }
    #[test]
    fn pointer_boot_back_revokes_physical_session_without_replaying_updates() {
        let ctx = egui::Context::default();
        let (mut cockpit, stop, requests, updates) = fixture(&ctx);
        cockpit.connected = false;
        cockpit.connecting = true;
        cockpit.request_pending = true;
        cockpit.boot.connect();
        cockpit.boot.fail();
        frame(&ctx, &mut cockpit, vec![], true);
        let output = frame(&ctx, &mut cockpit, vec![], true);
        let back = point(&output, "Pilot / server settings");
        click(&ctx, &mut cockpit, back, true);
        revoked(&cockpit, &stop, &requests);
        assert!(
            updates.send(Update::Ready).is_err(),
            "old session must be disconnected"
        );
        cockpit.poll();
        // A repeated back gesture cannot restore an authority or send a command.
        cockpit.boot.connect();
        cockpit.boot.fail();
        frame(&ctx, &mut cockpit, vec![], true);
        let output = frame(&ctx, &mut cockpit, vec![], true);
        click(
            &ctx,
            &mut cockpit,
            point(&output, "Pilot / server settings"),
            true,
        );
        revoked(&cockpit, &stop, &requests);
    }
    #[test]
    fn pointer_session_disconnect_change_and_disabled_reconnect_clear_physical_state() {
        let ctx = egui::Context::default();
        let (mut cockpit, stop, requests, updates) = fixture(&ctx);
        for label in [
            "Disconnect",
            "Reconnect",
            "Change pilot / server",
            "Change pilot / server",
        ] {
            frame(&ctx, &mut cockpit, vec![], false);
            let output = frame(&ctx, &mut cockpit, vec![], false);
            click(&ctx, &mut cockpit, point(&output, "Session"), false);
            frame(&ctx, &mut cockpit, vec![], false);
            let output = frame(&ctx, &mut cockpit, vec![], false);
            click(&ctx, &mut cockpit, point(&output, label), false);
            // Disabled Reconnect leaves its menu open; close only presentation.
            if label == "Reconnect" {
                click(&ctx, &mut cockpit, Pos2::new(900., 750.), false);
            }
            revoked(&cockpit, &stop, &requests);
        }
        assert!(updates.send(Update::Ready).is_err());
        cockpit.poll();
        revoked(&cockpit, &stop, &requests);
    }
    #[test]
    fn rejected_physical_batch_cannot_restore_scene_from_later_valid_response() {
        for failure in [false, true] {
            let ctx = egui::Context::default();
            let (mut cockpit, stop, requests, updates) = fixture(&ctx);
            let valid = initial_response_for_tests();
            if failure {
                updates
                    .send(Update::Failure("closed authority".into()))
                    .unwrap();
            } else {
                let mut invalid = valid.clone();
                invalid
                    .physical_cargo
                    .as_mut()
                    .unwrap()
                    .source_configuration_sha256 = "00".repeat(32);
                updates
                    .send(Update::Response(Box::new(invalid), false))
                    .unwrap();
            }
            updates
                .send(Update::Response(Box::new(valid), true))
                .unwrap();
            cockpit.poll();
            revoked(&cockpit, &stop, &requests);
            assert!(
                updates.send(Update::Ready).is_err(),
                "revoked worker must not reattach"
            );
            cockpit.poll();
            revoked(&cockpit, &stop, &requests);
        }
    }
    #[test]
    fn physical_delivery_freshness_uses_production_time_and_releases_observation_slot() {
        for visible in [false, true] {
            let ctx = egui::Context::default();
            let (mut cockpit, _stop, _requests, updates) = fixture(&ctx);
            let pending = Arc::new(AtomicBool::new(!visible));
            let emitted = Instant::now() - Duration::from_secs(2);
            updates
                .send(Update::PhysicalResponse(
                    Box::new(initial_response_for_tests()),
                    visible,
                    emitted,
                    (!visible).then_some(pending.clone()),
                ))
                .unwrap();
            cockpit.poll();
            assert_eq!(cockpit.performance_snapshot_received, Some(emitted));
            assert!(!cockpit.physical_request_allowed(&Request::DroneHold));
            assert!(!pending.load(Ordering::Acquire));
            let fresh = Instant::now();
            updates
                .send(Update::PhysicalResponse(
                    Box::new(initial_response_for_tests()),
                    true,
                    fresh,
                    None,
                ))
                .unwrap();
            cockpit.poll();
            assert_eq!(cockpit.performance_snapshot_received, Some(fresh));
            assert!(cockpit.physical_request_allowed(&Request::DroneHold));
        }
    }
    #[test]
    fn periodic_backpressure_keeps_command_fifo_and_authority_clock_advancing() {
        let bridge = start(options_for_tests(PathBuf::from(":memory:")));
        let (periodic, pending) = loop {
            match bridge
                .updates
                .recv_timeout(Duration::from_secs(15))
                .unwrap()
            {
                Update::PhysicalResponse(response, false, _, Some(pending)) => {
                    break (response, pending)
                }
                Update::Failure(error) => panic!("{error}"),
                _ => {}
            }
        };
        assert!(pending.load(Ordering::Acquire));
        bridge
            .commands
            .send(Request::LaunchDroneClass {
                class: DroneClass::Mining,
            })
            .unwrap();
        bridge
            .commands
            .send(Request::DroneHoldById { id: 1 })
            .unwrap();
        let mut acknowledged_at = periodic.snapshot.as_ref().unwrap().server_time_ms;
        for held in [false, true] {
            let Update::PhysicalResponse(response, true, _, None) = bridge
                .updates
                .recv_timeout(Duration::from_secs(15))
                .unwrap()
            else {
                panic!("pending periodic must not duplicate or reorder acknowledgements");
            };
            assert!(response.ok, "{}", response.message);
            let bay = response
                .physical_cargo
                .as_ref()
                .unwrap()
                .bay_state
                .as_ref()
                .unwrap();
            assert_eq!(
                bay.held, held,
                "launch and hold acknowledgements must retain FIFO"
            );
            assert!(bay.route.is_some());
            acknowledged_at = response.snapshot.as_ref().unwrap().server_time_ms;
        }
        assert!(
            matches!(
                bridge.updates.recv_timeout(Duration::from_millis(350)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ),
            "a consumed-but-unreleased periodic slot must stay bounded"
        );
        bridge.commands.send(Request::Snapshot).unwrap();
        let Update::PhysicalResponse(response, true, _, None) = bridge
            .updates
            .recv_timeout(Duration::from_secs(15))
            .unwrap()
        else {
            panic!("explicit observation acknowledgement missing");
        };
        let accepted_at = response.snapshot.as_ref().unwrap().server_time_ms;
        assert!(
            accepted_at > acknowledged_at,
            "authority must advance while periodic presentation is blocked"
        );
        pending.store(false, Ordering::Release);
        let Update::PhysicalResponse(response, false, _, Some(next_pending)) = bridge
            .updates
            .recv_timeout(Duration::from_secs(15))
            .unwrap()
        else {
            panic!("next periodic observation should resume after release");
        };
        assert!(response.snapshot.as_ref().unwrap().server_time_ms > accepted_at);
        assert!(next_pending.load(Ordering::Acquire));
        drop(bridge);
    }
    #[test]
    fn stopped_worker_does_not_execute_next_queued_request_and_releases_world() {
        let directory = std::env::temp_dir().join(format!(
            "arkline-physical-stop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("world.sqlite");
        let bridge = start(options_for_tests(path.clone()));
        let _initial = loop {
            match bridge
                .updates
                .recv_timeout(Duration::from_secs(15))
                .unwrap()
            {
                Update::PhysicalResponse(response, true, _, _) => break response,
                Update::Failure(error) => panic!("{error}"),
                _ => {}
            }
        };
        bridge.worker.stop.store(true, Ordering::Release);
        let _ = bridge.commands.send(Request::LaunchDroneClass {
            class: DroneClass::Mining,
        });
        // Joining is a test/App-drop operation, never done by the pointer handler.
        drop(bridge);
        let options = options_for_tests(path);
        let mut world = PhysicalControlledWorld::open(
            &options.world_path,
            options.genesis,
            options.bay_binding,
            options.control_binding,
            &options.owner_token,
            1_000,
        )
        .unwrap();
        let mut owner = None;
        let response = world.handle_at(
            &mut owner,
            crate::flight3d::physical_cargo_native_tests::connect(),
            1_000,
        );
        assert!(response.ok, "{}", response.message);
        let bay = response.physical_cargo.unwrap().bay_state.unwrap();
        assert!(
            bay.route.is_none() && !bay.deployed,
            "stopped queue must not launch drone"
        );
        drop(world);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
