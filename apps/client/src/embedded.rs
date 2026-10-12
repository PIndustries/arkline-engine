//! The editor owns the window; a dedicated worker owns the local authority and SQLite.
use super::*;
use arkline_server::editor::EditorWorld;
use eframe::App;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmbeddedStatus {
    Loading,
    Paused,
    Running,
    Stopped,
    Error(String),
}
enum ClockCommand {
    Play,
    Pause,
    Step,
    Stop,
    Shutdown,
}

pub struct EmbeddedClient {
    cockpit: Cockpit,
    clock: Sender<ClockCommand>,
    states: Receiver<EmbeddedStatus>,
    state: EmbeddedStatus,
    destroyed: bool,
}
impl EmbeddedClient {
    pub fn new(cc: &eframe::CreationContext<'_>, data_dir: PathBuf, pilot: String) -> Self {
        let mut cockpit = Cockpit::new(
            cc,
            "Embedded local world".into(),
            data_dir.join("client"),
            Some("embedded-editor".into()),
        );
        cockpit.embedded = true;
        cockpit.embedded_paused = true;
        cockpit.focus_on_first_frame = false;
        cockpit.boot.visible = false;
        cockpit.pilot = pilot.trim().to_owned();
        cockpit.connecting = true;
        cockpit.notice = "Opening local world…".into();
        cockpit.pilot_input.disable_capture();
        let (clock, clock_rx) = mpsc::channel();
        let (status_tx, states) = mpsc::channel();
        let (commands, requests) = mpsc::channel();
        let (updates, receiver) = mpsc::sync_channel(64);
        cockpit.commands = Some(commands);
        cockpit.updates = Some(receiver);
        let pilot = cockpit.pilot.clone();
        let controls = cockpit.controls.clone();
        std::thread::spawn(move || {
            if let Err(error) = run(
                data_dir,
                pilot,
                clock_rx,
                requests,
                updates.clone(),
                controls,
                status_tx.clone(),
            ) {
                let _ = updates.send(Update::Failure(error.clone()));
                let _ = status_tx.send(EmbeddedStatus::Error(error));
            }
        });
        Self {
            cockpit,
            clock,
            states,
            state: EmbeddedStatus::Loading,
            destroyed: false,
        }
    }
    pub fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        for state in self.states.try_iter() {
            self.state = state;
        }
        self.cockpit.embedded_paused = self.state != EmbeddedStatus::Running;
        self.cockpit.embedded_input = ctx
            .pointer_latest_pos()
            .is_some_and(|p| ctx.available_rect().contains(p));
        self.cockpit.update(ctx, frame);
        // Paused editor time never advances because of UI rendering.
        if self.cockpit.embedded_paused {
            self.cockpit.frozen_time = self.cockpit.snapshot.as_ref().map(|s| s.server_time_ms);
        }
    }
    pub fn play(&mut self) {
        let _ = self.clock.send(ClockCommand::Play);
    }
    pub fn pause(&mut self) {
        let _ = self.clock.send(ClockCommand::Pause);
    }
    pub fn step(&mut self) {
        let _ = self.clock.send(ClockCommand::Step);
    }
    pub fn stop(&mut self) {
        let _ = self.clock.send(ClockCommand::Stop);
        self.state = EmbeddedStatus::Stopped;
        self.cockpit.notice = "Local world stopped".into();
    }
    pub fn on_exit(&mut self) {
        if self.destroyed {
            return;
        }
        self.destroyed = true;
        let _ = self.clock.send(ClockCommand::Shutdown);
        self.cockpit.flight_view.destroy();
        self.cockpit.hangar_view.destroy();
    }
    pub fn status(&self) -> &EmbeddedStatus {
        &self.state
    }
    pub fn ready(&self) -> bool {
        matches!(self.state, EmbeddedStatus::Paused | EmbeddedStatus::Running)
    }
    pub fn paused(&self) -> bool {
        self.state == EmbeddedStatus::Paused
    }
    pub fn snapshot(&self) -> Option<Snapshot> {
        self.cockpit.snapshot.clone()
    }
    pub fn assets(&self) -> Option<AssetsView> {
        self.cockpit.assets.clone()
    }
    pub fn pilot(&self) -> &str {
        &self.cockpit.pilot
    }
    pub fn set_material_preview(&mut self, preview: Option<EmbeddedMaterialPreview>) {
        self.cockpit
            .hangar_view
            .set_material_preview(preview.map(EmbeddedMaterialPreview::validated));
    }
    pub fn send_command(&mut self, request: Request) {
        self.cockpit.send(request);
    }
}
impl Drop for EmbeddedClient {
    fn drop(&mut self) {
        let _ = self.clock.send(ClockCommand::Shutdown);
        // Never wait for database writes on the render thread.
        self.on_exit();
    }
}
fn permitted_while_paused(request: &Request) -> bool {
    matches!(request, Request::Snapshot | Request::ExportLedger)
}
fn run(
    dir: PathBuf,
    pilot: String,
    clock: Receiver<ClockCommand>,
    requests: Receiver<Request>,
    updates: mpsc::SyncSender<Update>,
    controls: pilot_input::Mailbox,
    states: Sender<EmbeddedStatus>,
) -> Result<(), String> {
    let mut world = EditorWorld::open(&dir).map_err(|e| e.to_string())?;
    let credentials = dir.join("client");
    fs::create_dir_all(&credentials).map_err(|e| e.to_string())?;
    let path = credentials_path(&credentials, "embedded-editor", &pilot);
    let token = match fs::read_to_string(&path) {
        Ok(s) => Some(s),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.to_string()),
    };
    let mut session = None;
    let mut response = world.handle(
        &mut session,
        WorkerIdentity::default().request("embedded-editor", &pilot, token)?,
    );
    if !response.ok {
        return Err(response.message);
    }
    if let Some(token) = response.resume_token.take() {
        save_token(&path, &token)?;
    }
    updates.send(Update::Ready).map_err(|e| e.to_string())?;
    updates
        .send(Update::Response(Box::new(response), false))
        .map_err(|e| e.to_string())?;
    let _ = states.send(EmbeddedStatus::Paused);
    let mut previous = Instant::now();
    loop {
        match clock.recv_timeout(Duration::from_millis(50)) {
            Ok(ClockCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(ClockCommand::Stop) => {
                world.pause();
                controls.clear();
                world.disconnect(&mut session);
                let _ = states.send(EmbeddedStatus::Stopped);
            }
            Ok(ClockCommand::Play) => {
                if session.is_none() {
                    let token = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    let response = world.handle(
                        &mut session,
                        WorkerIdentity::default().request(
                            "embedded-editor",
                            &pilot,
                            Some(token),
                        )?,
                    );
                    if !response.ok {
                        return Err(response.message);
                    }
                }
                world.play();
                previous = Instant::now();
                let _ = states.send(EmbeddedStatus::Running);
            }
            Ok(ClockCommand::Pause) => {
                world.pause();
                controls.clear();
                let _ = states.send(EmbeddedStatus::Paused);
            }
            Ok(ClockCommand::Step) => {
                world.step(50).map_err(|e| e.to_string())?;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        world
            .advance(now.duration_since(previous).as_millis().min(250) as u64)
            .map_err(|e| e.to_string())?;
        previous = now;
        for request in requests.try_iter().take(32) {
            let response = if world.is_paused() && !permitted_while_paused(&request) {
                Response::error("Simulation paused. Press Play to operate the ship.")
            } else {
                world.handle(&mut session, request)
            };
            if updates
                .send(Update::Response(Box::new(response), true))
                .is_err()
            {
                world.disconnect(&mut session);
                return Ok(());
            }
        }
        if let Some(request) = controls.take() {
            if !world.is_paused() {
                let _ = world.handle(&mut session, request);
            }
        }
        if session.is_none() {
            continue;
        }
        let response = world.handle(&mut session, Request::Snapshot);
        match updates.try_send(Update::Response(Box::new(response), false)) {
            Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
            Err(mpsc::TrySendError::Disconnected(_)) => break,
        }
    }
    world.disconnect(&mut session);
    let _ = states.send(EmbeddedStatus::Stopped);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_frame_leaves_editor_panels_and_window_ownership_intact() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        cockpit.embedded_input = false;
        cockpit.focus_on_first_frame = false;
        cockpit.view_mode = ViewMode::Tactical;
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(1400., 900.),
                )),
                events: vec![egui::Event::Key {
                    key: egui::Key::F11,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                ..Default::default()
            },
            |ctx| {
                egui::TopBottomPanel::top("editor_toolbar")
                    .exact_height(60.)
                    .show(ctx, |ui| {
                        ui.label("Editor");
                    });
                egui::SidePanel::left("editor_inspector")
                    .exact_width(260.)
                    .show(ctx, |ui| {
                        ui.label("Inspector");
                    });
                let available = ctx.available_rect();
                assert!(available.left() >= 260. && available.top() >= 60.);
                cockpit.update(ctx, &mut eframe::Frame::_new_kittest());
            },
        );
        assert!(output
            .viewport_output
            .values()
            .all(|v| v.commands.is_empty()));
        assert!(rx.try_recv().is_err());
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text().contains("Fullscreen") || t.galley.text() == "Open second cockpit")));
    }
    #[test]
    fn hangar_access_anchor_and_paint_stay_above_editor_console() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.station_avatar = None;
        for frame in 0..3 {
            let mut viewport = egui::Rect::NOTHING;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        egui::vec2(1224., 768.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("toolbar")
                        .exact_height(54.)
                        .show(ctx, |_| {});
                    egui::TopBottomPanel::bottom("console")
                        .exact_height(67.)
                        .show(ctx, |_| {});
                    egui::SidePanel::left("outliner")
                        .exact_width(205.)
                        .show(ctx, |_| {});
                    egui::SidePanel::right("inspector")
                        .exact_width(290.)
                        .show(ctx, |_| {});
                    viewport = ctx.available_rect().shrink(8.);
                    cockpit.station_overlay(ctx);
                },
            );
            let mut found = false;
            for clipped in output.shapes {
                if let egui::Shape::Text(t) = clipped.shape {
                    if t.galley.text() == "Leave ship" {
                        found = true;
                        assert!(
                            viewport.contains_rect(clipped.clip_rect),
                            "{:?} outside {:?}",
                            clipped.clip_rect,
                            viewport
                        );
                        assert!(
                            viewport.contains(t.pos),
                            "button outside viewport: {:?}",
                            t.pos
                        );
                    }
                }
            }
            if frame > 0 {
                assert!(found);
            }
        }
    }
    #[test]
    fn worker_pauses_steps_restarts_and_preserves_authenticated_pilot() {
        let dir = std::env::temp_dir().join(format!(
            "arkline-embedded-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let (clock, rx) = mpsc::channel();
        let (requests, req_rx) = mpsc::channel();
        let (updates, out) = mpsc::sync_channel(64);
        let (states, status) = mpsc::channel();
        let worker_dir = dir.clone();
        let worker = std::thread::spawn(move || {
            run(
                worker_dir,
                "EditorPilot".into(),
                rx,
                req_rx,
                updates,
                Default::default(),
                states,
            )
        });
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Paused
        );
        let snapshot = |out: &Receiver<Update>| loop {
            if let Update::Response(response, _) = out.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                if let Some(snapshot) = response.snapshot {
                    break snapshot;
                }
            }
        };
        let first = snapshot(&out);
        requests.send(Request::Undock).unwrap();
        loop {
            if let Update::Response(response, true) =
                out.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(!response.ok);
                assert!(response.message.contains("paused"));
                break;
            }
        }
        assert_eq!(snapshot(&out).server_time_ms, first.server_time_ms);
        clock.send(ClockCommand::Step).unwrap();
        loop {
            let current = snapshot(&out);
            if current.server_time_ms > first.server_time_ms {
                assert_eq!(current.server_time_ms, first.server_time_ms + 50);
                break;
            }
        }
        clock.send(ClockCommand::Play).unwrap();
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Running
        );
        requests.send(Request::Undock).unwrap();
        loop {
            if let Update::Response(response, true) =
                out.recv_timeout(Duration::from_secs(5)).unwrap()
            {
                assert!(response.ok, "{}", response.message);
                break;
            }
        }
        clock.send(ClockCommand::Stop).unwrap();
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Stopped
        );
        clock.send(ClockCommand::Play).unwrap();
        assert_eq!(
            status.recv_timeout(Duration::from_secs(5)).unwrap(),
            EmbeddedStatus::Running
        );
        assert_eq!(snapshot(&out).pilots.len(), 1);
        clock.send(ClockCommand::Shutdown).unwrap();
        worker.join().unwrap().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
