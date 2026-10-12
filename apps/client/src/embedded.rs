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
/// Local renderer modes only; selecting one never changes server embodiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddedView {
    Cockpit,
    External,
    Tactical,
    Tactical3d,
    Hangar,
    FirstPerson,
    ThirdPerson,
}
impl EmbeddedView {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cockpit => "Cockpit",
            Self::External => "3D External",
            Self::Tactical => "2D Tactical",
            Self::Tactical3d => "3D Tactical",
            Self::Hangar => "Hangar inspection",
            Self::FirstPerson => "On foot · first person",
            Self::ThirdPerson => "On foot · third person",
        }
    }
}
#[derive(Clone, Debug)]
pub struct ViewTransition {
    pub sequence: u64,
    pub from: Option<EmbeddedView>,
    pub to: Option<EmbeddedView>,
    pub reason: String,
    pub simulation_time_ms: Option<u64>,
    /// Monotonic host elapsed time since this embedded client was created.
    pub observed_at_ms: u64,
}
#[derive(Clone, Debug)]
pub struct EmbeddedViewState {
    pub current: Option<EmbeddedView>,
    pub available: Vec<EmbeddedView>,
    pub wireframe: bool,
    pub viewed_pilot: String,
    pub scene: String,
    pub transitions: Vec<ViewTransition>,
}
#[derive(Default)]
struct ViewDiagnostics {
    previous: Option<EmbeddedView>,
    sequence: u64,
    started: Option<Instant>,
    events: VecDeque<ViewTransition>,
    wireframe: bool,
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
    views: ViewDiagnostics,
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
            views: ViewDiagnostics {
                started: Some(Instant::now()),
                ..Default::default()
            },
        }
    }
    /// Drain authority updates while another editor workspace owns the viewport.
    /// This never renders, consumes egui input, or starts a new connection.
    pub fn poll(&mut self) {
        for state in self.states.try_iter() {
            self.state = state;
        }
        self.cockpit.embedded_paused = self.state != EmbeddedStatus::Running;
        self.cockpit.embedded_input = false;
        self.cockpit.poll();
        self.observe_view("Authoritative state or observed client view changed");
        if self.cockpit.embedded_paused {
            self.cockpit.frozen_time = self.cockpit.snapshot.as_ref().map(|s| s.server_time_ms);
        }
    }
    /// Poll a hidden viewport without treating workspace absence as a rendered frame.
    pub fn poll_hidden(&mut self) {
        self.poll();
        self.cockpit.frame_intervals.clear();
        self.cockpit.frame_observed = Instant::now();
    }
    pub fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll();
        self.cockpit.embedded_input = ctx
            .pointer_latest_pos()
            .is_some_and(|p| ctx.available_rect().contains(p));
        self.cockpit.update(ctx, frame);
        self.observe_view("In-game view control changed");
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
    fn current_view(&self) -> Option<EmbeddedView> {
        let pilot = self.cockpit.own()?;
        if self.cockpit.in_station_hangar() {
            return Some(if pilot.station_avatar.is_some() {
                match self.cockpit.hangar_view.on_foot_view {
                    flight3d::hangar::OnFootView::FirstPerson => EmbeddedView::FirstPerson,
                    flight3d::hangar::OnFootView::ThirdPerson => EmbeddedView::ThirdPerson,
                }
            } else {
                EmbeddedView::Hangar
            });
        }
        Some(match self.cockpit.view_mode {
            ViewMode::Cockpit => EmbeddedView::Cockpit,
            ViewMode::External => EmbeddedView::External,
            ViewMode::Tactical => EmbeddedView::Tactical,
            ViewMode::Tactical3d => EmbeddedView::Tactical3d,
        })
    }
    fn available_views(&self) -> Vec<EmbeddedView> {
        if !self.ready() || !self.cockpit.connected {
            return vec![];
        }
        match self.current_view() {
            None => vec![],
            Some(EmbeddedView::Hangar) => vec![EmbeddedView::Hangar],
            Some(EmbeddedView::FirstPerson | EmbeddedView::ThirdPerson) => {
                vec![EmbeddedView::FirstPerson, EmbeddedView::ThirdPerson]
            }
            _ => vec![
                EmbeddedView::Cockpit,
                EmbeddedView::External,
                EmbeddedView::Tactical,
                EmbeddedView::Tactical3d,
            ],
        }
    }
    fn observe_view(&mut self, reason: &str) {
        let current = self.current_view();
        if current == self.views.previous {
            return;
        }
        self.views.sequence = self.views.sequence.saturating_add(1);
        self.views.events.push_back(ViewTransition {
            sequence: self.views.sequence,
            from: self.views.previous,
            to: current,
            reason: reason.into(),
            simulation_time_ms: self.cockpit.snapshot.as_ref().map(|s| s.server_time_ms),
            observed_at_ms: self
                .views
                .started
                .get_or_insert_with(Instant::now)
                .elapsed()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        });
        self.views.previous = current;
        while self.views.events.len() > 32 {
            self.views.events.pop_front();
        }
    }
    pub fn debug_view_state(&self) -> EmbeddedViewState {
        let scene = match self.cockpit.own() {
            None => "No authoritative pilot snapshot".into(),
            Some(p) if self.cockpit.in_station_hangar() => match p.station_avatar {
                Some(a) if a.onboard_interior => {
                    format!("Pioneer interior · deck {}", a.interior_deck)
                }
                Some(_) => "Station · on foot".into(),
                None => "Station · aboard docked ship".into(),
            },
            Some(p) => format!("System {} · {:?}", p.system_id, p.location),
        };
        EmbeddedViewState {
            current: self.current_view(),
            available: self.available_views(),
            wireframe: self.views.wireframe,
            viewed_pilot: self.cockpit.controlled_name().into(),
            scene,
            transitions: self.views.events.iter().cloned().collect(),
        }
    }
    pub fn set_debug_view(&mut self, view: EmbeddedView) -> Result<(), String> {
        if !self.available_views().contains(&view) {
            return Err("View unavailable for the current confirmed pilot state. Camera controls cannot dock, board, or disembark.".into());
        }
        self.observe_view("Initial confirmed view");
        match view {
            EmbeddedView::Cockpit => self.cockpit.view_mode = ViewMode::Cockpit,
            EmbeddedView::External => self.cockpit.view_mode = ViewMode::External,
            EmbeddedView::Tactical => self.cockpit.view_mode = ViewMode::Tactical,
            EmbeddedView::Tactical3d => self.cockpit.view_mode = ViewMode::Tactical3d,
            EmbeddedView::FirstPerson => {
                self.cockpit.hangar_view.on_foot_view = flight3d::hangar::OnFootView::FirstPerson
            }
            EmbeddedView::ThirdPerson => {
                self.cockpit.hangar_view.on_foot_view = flight3d::hangar::OnFootView::ThirdPerson
            }
            EmbeddedView::Hangar => {}
        }
        self.observe_view("Editor requested local view change");
        Ok(())
    }
    pub fn set_debug_wireframe(&mut self, enabled: bool) {
        self.views.wireframe = enabled;
        self.cockpit.hangar_view.set_debug_wireframe(enabled);
        self.cockpit.flight_view.set_debug_wireframe(enabled);
    }
    pub fn debug_render_metrics(&self) -> crate::DebugRenderMetrics {
        if self.cockpit.in_station_hangar() {
            self.cockpit.hangar_view.debug_render_metrics()
        } else if self.cockpit.view_mode == ViewMode::Tactical {
            Default::default()
        } else {
            self.cockpit.flight_view.debug_render_metrics()
        }
    }
    pub fn set_material_preview(&mut self, preview: Option<EmbeddedMaterialPreview>) {
        self.cockpit.flight_view.set_material_preview(preview);
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
    fn debug_fixture() -> (EmbeddedClient, Receiver<Request>) {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        cockpit.connected = true;
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Station;
        cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = None;
        let (commands, requests) = mpsc::channel();
        cockpit.commands = Some(commands);
        let (clock, _) = mpsc::channel();
        let (_, states) = mpsc::channel();
        (
            EmbeddedClient {
                cockpit,
                clock,
                states,
                state: EmbeddedStatus::Paused,
                destroyed: false,
                views: ViewDiagnostics {
                    started: Some(Instant::now()),
                    ..Default::default()
                },
            },
            requests,
        )
    }
    #[test]
    fn debug_view_changes_only_camera_and_logs_bounded_ordered_transitions() {
        let (mut client, requests) = debug_fixture();
        let authority = serde_json::to_value(client.snapshot()).unwrap();
        for i in 0..40 {
            client
                .set_debug_view(if i % 2 == 0 {
                    EmbeddedView::External
                } else {
                    EmbeddedView::Tactical3d
                })
                .unwrap();
        }
        assert_eq!(serde_json::to_value(client.snapshot()).unwrap(), authority);
        assert!(requests.try_recv().is_err());
        let state = client.debug_view_state();
        assert_eq!(state.current, Some(EmbeddedView::Tactical3d));
        assert_eq!(state.transitions.len(), 32);
        assert!(state
            .transitions
            .windows(2)
            .all(|w| w[0].sequence < w[1].sequence && w[0].observed_at_ms <= w[1].observed_at_ms));
        assert!(state
            .transitions
            .iter()
            .all(|e| e.simulation_time_ms == Some(1000)));
        assert_eq!(state.viewed_pilot, "pilot");
        client.cockpit.view_mode = ViewMode::Cockpit;
        client.observe_view("In-game view control changed");
        assert_eq!(
            client.debug_view_state().transitions.last().unwrap().to,
            Some(EmbeddedView::Cockpit)
        );
    }
    #[test]
    fn debug_view_rejects_embodiment_changes_and_disconnected_controls() {
        let (mut client, requests) = debug_fixture();
        client.cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        assert_eq!(
            client.debug_view_state().available,
            vec![EmbeddedView::Hangar]
        );
        assert!(client.set_debug_view(EmbeddedView::Cockpit).is_err());
        client.cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar=Some(serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000,"onboard_interior":true,"interior_deck":2})).unwrap());
        client.set_debug_view(EmbeddedView::FirstPerson).unwrap();
        assert_eq!(
            client.cockpit.hangar_view.on_foot_view,
            flight3d::hangar::OnFootView::FirstPerson
        );
        assert!(client.debug_view_state().scene.contains("deck 2"));
        assert!(client.set_debug_view(EmbeddedView::External).is_err());
        client.set_debug_view(EmbeddedView::ThirdPerson).unwrap();
        client.cockpit.connected = false;
        assert!(client.debug_view_state().available.is_empty());
        assert!(client.set_debug_view(EmbeddedView::FirstPerson).is_err());
        assert!(requests.try_recv().is_err());
    }
    #[test]
    fn hidden_poll_drains_state_without_paint_or_input() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.take().unwrap();
        let expected_time = snapshot.server_time_ms;
        cockpit.embedded = true;
        let (updates, receiver) = mpsc::channel();
        cockpit.updates = Some(receiver);
        let mut response = Response::error("");
        response.ok = true;
        response.snapshot = Some(snapshot);
        updates
            .send(Update::Response(Box::new(response), false))
            .unwrap();
        let (clock, _commands) = mpsc::channel();
        let (states, receiver) = mpsc::channel();
        states.send(EmbeddedStatus::Paused).unwrap();
        let mut client = EmbeddedClient {
            cockpit,
            clock,
            states: receiver,
            state: EmbeddedStatus::Loading,
            destroyed: false,
            views: ViewDiagnostics {
                started: Some(Instant::now()),
                ..Default::default()
            },
        };
        client.cockpit.frame_intervals.push_back(1.245);
        client.cockpit.frame_observed = Instant::now() - Duration::from_secs(30);
        let output = ctx.run(Default::default(), |_| client.poll_hidden());
        assert!(client.cockpit.frame_intervals.is_empty());
        assert!(client.cockpit.frame_observed.elapsed() < Duration::from_secs(1));
        assert_eq!(client.status(), &EmbeddedStatus::Paused);
        assert_eq!(client.snapshot().unwrap().server_time_ms, expected_time);
        assert_eq!(client.cockpit.frozen_time, Some(expected_time));
        assert!(!client.cockpit.embedded_input);
        assert!(output.shapes.is_empty());
        assert!(output
            .viewport_output
            .values()
            .all(|v| v.commands.is_empty()));
    }
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
