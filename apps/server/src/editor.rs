//! In-process editor authority. No transport request can control this clock.
use crate::*;
#[path = "editor_events.rs"]
mod events;

pub const EDITOR_TICK_MS: u64 = 50;
pub const MAX_EDITOR_ADVANCE_MS: u64 = 250;

/// Owns a local-development world and always reopens paused. Play time comes only
/// from explicit bounded deltas, never elapsed wall time. Server is deliberately
/// private: callers cannot accidentally invoke its wall-clock tick/handle APIs.
pub struct EditorWorld {
    server: Server,
    paused: bool,
    remainder_ms: u64,
    graph: events::GraphRuntime,
    graph_session: Option<String>,
}
impl EditorWorld {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let path = local_development::database_path(directory.as_ref())?;
        let server = Server::open_realm(path, true, true)?;
        Ok(Self {
            server,
            paused: true,
            remainder_ms: 0,
            graph: events::GraphRuntime::default(),
            graph_session: None,
        })
    }
    pub fn play(&mut self) {
        self.graph.resume();
        self.paused = false;
    }
    pub fn pause(&mut self) {
        self.paused = true;
        self.remainder_ms = 0;
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    pub fn time(&self) -> u64 {
        self.server.editor_time.expect("editor clock initialized")
    }
    /// At most five fixed ticks per call; excess frame delay is discarded rather
    /// than replaying a backlog that would freeze the editor after a stall.
    pub fn advance(&mut self, delta_ms: u64) -> Result<()> {
        if !self.paused {
            self.run_ticks(delta_ms)?;
        }
        Ok(())
    }
    /// Explicit single-step while paused, with the same fixed-tick/cost bound.
    pub fn step(&mut self, delta_ms: u64) -> Result<()> {
        ensure!(self.paused, "pause the editor before stepping");
        self.run_ticks(delta_ms)
    }
    fn run_ticks(&mut self, delta_ms: u64) -> Result<()> {
        if !self.paused && self.graph_session.is_some() && self.drive_automatic_graph() {
            return Ok(());
        }
        self.remainder_ms += delta_ms.min(MAX_EDITOR_ADVANCE_MS);
        while self.remainder_ms >= EDITOR_TICK_MS {
            let next = self
                .time()
                .checked_add(EDITOR_TICK_MS)
                .context("editor clock exhausted")?;
            let started = std::time::Instant::now();
            if let Err(error) = self.server.tick_at(next) {
                self.graph.record(
                    None,
                    None,
                    self.time(),
                    started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                    "tick_failed",
                    "Authoritative tick failed; transaction not committed",
                    false,
                );
                self.pause();
                return Err(error);
            }
            self.remainder_ms -= EDITOR_TICK_MS;
            let duration = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
            if let Err(error) = self
                .graph
                .tick(next, duration, self.graph_session.is_some())
            {
                self.graph_fault(&error);
                return Ok(());
            }
            if !self.paused && self.graph_session.is_some() && self.drive_automatic_graph() {
                break;
            }
        }
        Ok(())
    }
    pub fn handle(&mut self, session: &mut Option<String>, request: Request) -> Response {
        let connect = matches!(&request, Request::Connect { .. });
        let snapshot = matches!(&request, Request::Snapshot);
        let kind = request_kind(&request);
        let started = std::time::Instant::now();
        let response = self.server.handle_at(session, request, self.time());
        if connect && response.ok {
            if self.graph_session != *session {
                self.graph.clear(self.time());
            }
            self.graph_session = session.clone();
        }
        if !snapshot {
            self.graph.record(
                None,
                None,
                self.time(),
                started.elapsed().as_micros().min(u64::MAX as u128) as u64,
                kind,
                if response.ok {
                    "Command accepted"
                } else {
                    "Command rejected"
                },
                response.ok,
            );
        }
        response
    }
    pub fn disconnect(&mut self, session: &mut Option<String>) {
        if self.graph_session == *session {
            self.graph.clear(self.time());
            self.graph_session = None;
        }
        self.server.disconnect(session);
    }
    pub fn graph_debug(&self) -> EditorGraphDebug {
        self.graph.debug(self.paused)
    }
    pub fn graph_replace(
        &mut self,
        expected_revision: u64,
        graph: EditorEventGraph,
        clear_pending: bool,
    ) -> Result<()> {
        ensure!(self.paused, "pause the editor before replacing its graph");
        self.graph
            .replace(expected_revision, graph, clear_pending, self.time())
    }
    pub fn graph_trigger(&mut self, node_id: u64) -> Result<()> {
        ensure!(
            self.graph_session.is_some(),
            "connect an authenticated local pilot before triggering the graph"
        );
        self.graph.trigger(node_id, self.time())
    }
    pub fn graph_step(&mut self) -> Result<()> {
        ensure!(self.paused, "pause the editor before stepping a graph node");
        self.drive_graph(true).map(|_| ())
    }
    pub fn graph_clear(&mut self) -> Result<()> {
        ensure!(self.paused, "pause the editor before clearing graph work");
        self.graph.clear(self.time());
        Ok(())
    }
    fn graph_fault(&mut self, error: &anyhow::Error) {
        self.pause();
        self.graph.record(
            None,
            None,
            self.time(),
            0,
            "graph_fault",
            &error.to_string(),
            false,
        );
    }
    // Graph authoring faults pause the editor but never terminate its live worker.
    fn drive_automatic_graph(&mut self) -> bool {
        match self.drive_graph(false) {
            Ok(paused) => paused,
            Err(error) => {
                self.graph_fault(&error);
                true
            }
        }
    }
    fn drive_graph(&mut self, single: bool) -> Result<bool> {
        let now = self.time();
        let result = self
            .graph
            .drive(&mut self.server, &mut self.graph_session, now, single);
        match result {
            Ok(true) => {
                self.pause();
                Ok(true)
            }
            Ok(false) => Ok(false),
            Err(error) => {
                self.pause();
                Err(error)
            }
        }
    }
}

// Exhaustive fixed names only: never serialize request payloads or credentials into traces.
fn request_kind(request: &Request) -> &'static str {
    match request {
        Request::StationSit { .. } => "station_sit",
        Request::DisembarkShip => "disembark_ship",
        Request::EnterShip => "enter_ship",
        Request::EnterInterior => "enter_interior",
        Request::ExitInterior => "exit_interior",
        Request::InteriorLift { .. } => "interior_lift",
        Request::StationJump => "station_jump",
        Request::StationWalk { .. } => "station_walk",
        Request::OrbitDrone { .. } => "orbit_drone",
        Request::BeginRemoteControl { .. } => "begin_remote_control",
        Request::EndRemoteControl => "end_remote_control",
        Request::RemoteControl { .. } => "remote_control",
        Request::StartResourceJob { .. } => "start_resource_job",
        Request::CancelResourceJob { .. } => "cancel_resource_job",
        Request::SetFleetController { .. } => "set_fleet_controller",
        Request::SpawnSurfaceProp { .. } => "spawn_surface_prop",
        Request::MoveSurfaceProp { .. } => "move_surface_prop",
        Request::RemoveSurfaceProp { .. } => "remove_surface_prop",
        Request::LaunchDroneClass { .. } => "launch_drone_class",
        Request::RecallDroneById { .. } => "recall_drone_by_id",
        Request::DroneHoldById { .. } => "drone_hold_by_id",
        Request::DroneNavigateById { .. } => "drone_navigate_by_id",
        Request::AssignMiningDrone { .. } => "assign_mining_drone",
        Request::AssignAttackDrone { .. } => "assign_attack_drone",
        Request::UnlockAsteroid { .. } => "unlock_asteroid",
        Request::LaunchDrone => "launch_drone",
        Request::RecallDrone => "recall_drone",
        Request::DroneHold => "drone_hold",
        Request::DroneNavigate { .. } => "drone_navigate",
        Request::StartMining { .. } => "start_mining",
        Request::StopMining { .. } => "stop_mining",
        Request::DepositOre { .. } => "deposit_ore",
        Request::WithdrawOre { .. } => "withdraw_ore",
        Request::CreateCorporation { .. } => "create_corporation",
        Request::InviteCorporation { .. } => "invite_corporation",
        Request::AcceptCorporation { .. } => "accept_corporation",
        Request::DepositCorporationOre { .. } => "deposit_corporation_ore",
        Request::WithdrawCorporationOre { .. } => "withdraw_corporation_ore",
        Request::SetPioneerRoom { .. } => "set_pioneer_room",
        Request::BoardShip { .. } => "board_ship",
        Request::AuthCapabilities => "auth_capabilities",
        Request::AccountConnect { .. } => "account_connect",
        Request::Connect { .. } => "connect",
        Request::Snapshot => "snapshot",
        Request::UnlockTarget => "unlock_target",
        Request::LockAsteroid { .. } => "lock_asteroid",
        Request::MineAsteroid { .. } => "mine_asteroid",
        Request::Jump { .. } => "jump",
        Request::Navigate { .. } => "navigate",
        Request::PilotInput { .. } => "pilot_input",
        Request::SelectShip { .. } => "select_ship",
        Request::Undock => "undock",
        Request::Warp { .. } => "warp",
        Request::Dock => "dock",
        Request::Mine { .. } => "mine",
        Request::PlaceSell { .. } => "place_sell",
        Request::Buy { .. } => "buy",
        Request::CancelSell { .. } => "cancel_sell",
        Request::ExportLedger => "export_ledger",
    }
}

/// Read before startup advancement: paused reopen must never recharge reactors
/// or progress room systems by the time elapsed on the host clock.
pub(super) fn open_clock(db: &mut Connection, editor: bool) -> Result<Option<u64>> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='editor_clock')",
        [],
        |r| r.get(0),
    )?;
    ensure!(
        editor || !exists,
        "editor-clock world requires the embedded editor"
    );
    if !editor {
        return Ok(None);
    }
    if exists {
        let time: String =
            db.query_row("SELECT time_ms FROM editor_clock WHERE id=1", [], |r| {
                r.get(0)
            })?;
        return Ok(Some(
            time.parse().context("invalid persisted editor clock")?,
        ));
    }
    // Adoption of an existing local world happens once; subsequent opens use only
    // this value. The existing world remains subject to normal restart cleanup.
    let time = now_ms();
    let tx = db.transaction()?;
    tx.execute_batch(
        "CREATE TABLE editor_clock(id INTEGER PRIMARY KEY CHECK(id=1),time_ms TEXT NOT NULL)",
    )?;
    tx.execute(
        "INSERT INTO editor_clock(id,time_ms) VALUES(1,?1)",
        [time.to_string()],
    )?;
    tx.commit()?;
    Ok(Some(time))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("arkline-editor-clock-{}", rand::random::<u64>()))
    }
    fn connect(token: Option<String>) -> Request {
        serde_json::from_value(serde_json::json!({"command":"connect","protocol_version":PROTOCOL_VERSION,"pilot":"EditorPilot","resume_token":token})).unwrap()
    }
    fn call(w: &mut EditorWorld, s: &mut Option<String>, r: Request) -> Response {
        let response = w.handle(s, r);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn snapshot(w: &mut EditorWorld) -> Snapshot {
        call(w, &mut None, Request::Snapshot).snapshot.unwrap()
    }
    #[test]
    fn paused_movement_and_mining_freeze_and_resume_only_explicit_ticks() {
        let path = dir();
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut w, &mut session, connect(None));
        call(&mut w, &mut session, Request::Undock);
        call(
            &mut w,
            &mut session,
            Request::Navigate {
                mode: NavigationMode::FlyTo {
                    position: [5000.0, 0.0, 1000.0],
                },
            },
        );
        let start = snapshot(&mut w);
        w.advance(u64::MAX).unwrap();
        assert_eq!(
            serde_json::to_value(&start).unwrap(),
            serde_json::to_value(snapshot(&mut w)).unwrap()
        );
        let time = w.time();
        w.step(250).unwrap();
        assert_eq!(w.time(), time + 250);
        assert_ne!(
            snapshot(&mut w).pilots[0].flight.as_ref().unwrap().position,
            start.pilots[0].flight.as_ref().unwrap().position
        );
        // Let the ordinary autonomous controller travel, target and start miners.
        call(
            &mut w,
            &mut session,
            Request::StartResourceJob {
                pilot: None,
                target_units: 100,
            },
        );
        w.play();
        for _ in 0..2400 {
            w.advance(250).unwrap();
            if snapshot(&mut w).pilots[0]
                .mining_modules
                .iter()
                .any(|m| m.target_id.is_some())
            {
                break;
            }
        }
        let before = snapshot(&mut w);
        assert!(before.pilots[0]
            .mining_modules
            .iter()
            .any(|m| m.target_id.is_some()));
        w.pause();
        let time = w.time();
        std::thread::sleep(std::time::Duration::from_millis(60));
        w.advance(u64::MAX).unwrap();
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(snapshot(&mut w)).unwrap()
        );
        assert_eq!(time, w.time());
        w.play();
        w.advance(u64::MAX).unwrap();
        assert_eq!(w.time(), time + 250);
        for _ in 0..28 {
            w.advance(250).unwrap();
        }
        assert!(snapshot(&mut w).pilots[0].ore > before.pilots[0].ore);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn restart_preserves_clock_and_charge_and_cannot_open_as_network_world() {
        let path = dir();
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        let token = call(&mut w, &mut session, connect(None))
            .resume_token
            .unwrap();
        call(&mut w, &mut session, Request::Undock);
        call(
            &mut w,
            &mut session,
            Request::Warp {
                destination: Destination::Belt,
            },
        );
        w.step(250).unwrap();
        let before = snapshot(&mut w);
        let time = w.time();
        drop(w);
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(Server::open_local_development(&path).is_err());
        let mut w = EditorWorld::open(&path).unwrap();
        assert!(w.is_paused());
        assert_eq!(w.time(), time);
        let after = snapshot(&mut w);
        assert_eq!(before.pilots[0].power, after.pilots[0].power);
        assert_eq!(before.pilots[0].flight, after.pilots[0].flight);
        assert!(!w.handle(&mut None, connect(None)).ok);
        call(&mut w, &mut session, connect(Some(token)));
        w.play();
        assert!(w.step(50).is_err());
        w.advance(49).unwrap();
        assert_eq!(w.time(), time);
        w.advance(1).unwrap();
        assert_eq!(w.time(), time + 50);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn failed_clock_commit_rolls_back_world_and_pauses_without_backlog() {
        let path = dir();
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut w, &mut session, connect(None));
        call(&mut w, &mut session, Request::Undock);
        call(
            &mut w,
            &mut session,
            Request::Navigate {
                mode: NavigationMode::FlyTo {
                    position: [5000.0, 0.0, 1000.0],
                },
            },
        );
        let before = snapshot(&mut w);
        let time = w.time();
        w.server.db.execute_batch("CREATE TRIGGER reject_clock BEFORE UPDATE ON editor_clock BEGIN SELECT RAISE(ABORT, 'test clock write failure'); END;").unwrap();
        w.play();
        assert!(w.advance(250).is_err());
        assert!(w.is_paused());
        assert_eq!(w.time(), time);
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(snapshot(&mut w)).unwrap()
        );
        w.server
            .db
            .execute_batch("DROP TRIGGER reject_clock")
            .unwrap();
        w.play();
        w.advance(50).unwrap();
        assert_eq!(w.time(), time + 50);
        drop(w);
        let w = EditorWorld::open(&path).unwrap();
        assert_eq!(w.time(), time + 50);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    fn setup() -> (std::path::PathBuf, EditorWorld, Option<String>) {
        let path =
            std::env::temp_dir().join(format!("arkline-editor-graph-{}", rand::random::<u64>()));
        let mut world = EditorWorld::open(&path).unwrap();
        let mut session = None;
        let r = world.handle(
            &mut session,
            Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                pilot: "GraphPilot".into(),
                resume_token: None,
                supports_room_controls: true,
                supports_fleet_hangar: true,
                supports_pioneer_interior: true,
            },
        );
        assert!(r.ok);
        assert!(!serde_json::to_string(&world.graph_debug())
            .unwrap()
            .contains(&r.resume_token.unwrap()));
        (path, world, session)
    }
    fn node(id: u64, operation: EditorNodeOp, breakpoint: bool) -> EditorGraphNode {
        EditorGraphNode {
            id,
            label: format!("Node {id}"),
            position: [0.; 2],
            operation,
            breakpoint,
        }
    }
    fn chain(ops: Vec<EditorNodeOp>) -> EditorEventGraph {
        let nodes: Vec<_> = ops
            .into_iter()
            .enumerate()
            .map(|(i, op)| node(i as u64 + 1, op, false))
            .collect();
        let edges = (1..nodes.len() as u64)
            .map(|id| EditorGraphEdge {
                from: id,
                to: id + 1,
                port: EditorGraphPort::Next,
            })
            .collect();
        EditorEventGraph {
            nodes,
            edges,
            ..Default::default()
        }
    }
    #[test]
    fn breakpoint_and_node_step_are_not_partial_simulation_ticks() {
        let (path, mut w, _) = setup();
        let mut graph = chain(vec![
            EditorNodeOp::Manual,
            EditorNodeOp::Trace {
                message: "observed".into(),
            },
        ]);
        graph.nodes[1].breakpoint = true;
        w.graph_replace(0, graph, false).unwrap();
        w.graph_trigger(1).unwrap();
        let time = w.time();
        w.play();
        w.advance(250).unwrap();
        assert!(w.is_paused());
        assert_eq!(w.time(), time);
        assert_eq!(w.graph_debug().breakpoint_node, Some(2));
        w.graph_step().unwrap();
        let debug = w.graph_debug();
        assert_eq!(debug.pending, 0);
        assert_eq!(w.time(), time);
        let node = debug
            .trace
            .iter()
            .find(|t| t.node_id == Some(2) && t.kind == "node")
            .unwrap();
        assert!(node.cause.is_some());
        assert_eq!(node.message, "observed");
        w.step(50).unwrap();
        assert_eq!(w.time(), time + 50);
        assert!(w
            .graph_debug()
            .trace
            .iter()
            .any(|t| t.kind == "tick" && t.simulation_time_ms == time + 50));
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn delay_uses_only_simulation_time_and_restart_discards_execution_not_world_clock() {
        let (path, mut w, _) = setup();
        w.graph_replace(
            0,
            chain(vec![
                EditorNodeOp::Manual,
                EditorNodeOp::Delay { ms: 50 },
                EditorNodeOp::Trace {
                    message: "later".into(),
                },
            ]),
            false,
        )
        .unwrap();
        w.graph_trigger(1).unwrap();
        w.graph_step().unwrap();
        w.graph_step().unwrap();
        let time = w.time();
        assert!(w.graph_step().is_err());
        assert_eq!(w.time(), time);
        w.step(50).unwrap();
        w.graph_step().unwrap();
        assert_eq!(w.graph_debug().pending, 0);
        drop(w);
        let w = EditorWorld::open(&path).unwrap();
        assert!(w.is_paused());
        assert_eq!(w.time(), time + 50);
        assert_eq!(w.graph_debug().revision, 0);
        assert!(w.graph_debug().graph.nodes.is_empty());
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn rejected_revision_cycle_or_busy_replace_preserves_graph_and_queue() {
        let (path, mut w, _) = setup();
        let graph = chain(vec![
            EditorNodeOp::Manual,
            EditorNodeOp::Trace {
                message: "end".into(),
            },
        ]);
        w.graph_replace(0, graph.clone(), false).unwrap();
        w.graph_trigger(1).unwrap();
        let before = w.graph_debug();
        assert!(w.graph_replace(0, graph.clone(), true).is_err());
        assert_eq!(w.graph_debug(), before);
        assert!(w.graph_replace(1, graph.clone(), false).is_err());
        assert_eq!(w.graph_debug(), before);
        let mut cycle = graph.clone();
        cycle.nodes[0].operation = EditorNodeOp::Delay { ms: 1 };
        cycle.edges.push(EditorGraphEdge {
            from: 2,
            to: 1,
            port: EditorGraphPort::Next,
        });
        assert!(w.graph_replace(1, cycle, true).is_err());
        assert_eq!(w.graph_debug(), before);
        w.graph_replace(1, graph, true).unwrap();
        assert_eq!(w.graph_debug().revision, 2);
        assert_eq!(w.graph_debug().pending, 0);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn tick_roots_have_stable_id_order_and_trace_is_bounded() {
        let (path, mut w, _) = setup();
        let graph = EditorEventGraph {
            nodes: vec![
                node(9, EditorNodeOp::Tick, false),
                node(2, EditorNodeOp::Tick, false),
            ],
            ..Default::default()
        };
        w.graph_replace(0, graph, false).unwrap();
        w.play();
        w.advance(50).unwrap();
        let nodes: Vec<_> = w
            .graph_debug()
            .trace
            .iter()
            .filter(|t| t.kind == "node")
            .map(|t| t.node_id.unwrap())
            .collect();
        assert_eq!(nodes, vec![2, 9]);
        for _ in 0..100 {
            w.advance(50).unwrap();
        }
        assert_eq!(w.graph_debug().trace.len(), 256);
        w.pause();
        w.graph_clear().unwrap();
        for _ in 0..128 {
            w.step(50).unwrap();
        }
        assert_eq!(w.graph_debug().pending, 256);
        assert!(w.step(50).is_ok());
        assert!(w.is_paused());
        assert_eq!(w.graph_debug().pending, 256);
        assert!(w
            .graph_debug()
            .trace
            .iter()
            .any(|t| t.kind == "graph_fault" && !t.ok));
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn automatic_delay_queue_saturation_pauses_without_losing_connected_world() {
        let (path, mut w, mut session) = setup();
        w.graph_replace(
            0,
            chain(vec![
                EditorNodeOp::Tick,
                EditorNodeOp::Delay { ms: 60_000 },
                EditorNodeOp::Trace {
                    message: "delayed".into(),
                },
            ]),
            false,
        )
        .unwrap();
        w.play();
        for _ in 0..300 {
            w.advance(50)
                .expect("graph scheduling faults must not disconnect the worker");
            if w.is_paused() {
                break;
            }
        }
        assert!(w.is_paused());
        assert_eq!(w.graph_debug().pending, 256);
        assert!(w
            .graph_debug()
            .trace
            .iter()
            .any(|t| t.kind == "graph_fault" && !t.ok));
        assert!(w.handle(&mut session, Request::Snapshot).ok);
        assert!(session.is_some());
        w.graph_clear().unwrap();
        w.graph_replace(
            1,
            chain(vec![
                EditorNodeOp::Manual,
                EditorNodeOp::Trace {
                    message: "recovered".into(),
                },
            ]),
            false,
        )
        .unwrap();
        w.graph_trigger(1).unwrap();
        let time = w.time();
        w.play();
        w.advance(50).unwrap();
        assert!(!w.is_paused());
        assert_eq!(w.time(), time + 50);
        assert!(w
            .graph_debug()
            .trace
            .iter()
            .any(|t| t.message == "recovered" && t.ok));
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn graph_room_commands_preserve_power_and_pressure_interlocks() {
        let (path, mut w, mut session) = setup();
        let commands = [
            PioneerRoomCommand::Breaker { closed: true },
            PioneerRoomCommand::Door {
                open: false,
                manual: false,
            },
            PioneerRoomCommand::Vent {
                mode: VentMode::Exhaust,
            },
        ];
        for action in commands {
            assert!(
                w.handle(&mut session, Request::SetPioneerRoom { action })
                    .ok
            );
        }
        for _ in 0..40 {
            w.step(250).unwrap();
        }
        w.graph_replace(
            0,
            chain(vec![
                EditorNodeOp::Manual,
                EditorNodeOp::RoomDoor { open: true },
                EditorNodeOp::RoomBreaker { closed: false },
            ]),
            false,
        )
        .unwrap();
        w.graph_trigger(1).unwrap();
        w.graph_step().unwrap();
        w.graph_step().unwrap();
        let room = w
            .handle(&mut session, Request::Snapshot)
            .assets
            .unwrap()
            .pioneer_room
            .unwrap();
        assert!(!room.door_open);
        assert!(room.door_commanded_open);
        assert!(room.interlock.unwrap().contains("Pressure"));
        let debug = w.graph_debug();
        let door_trace = debug
            .trace
            .iter()
            .rev()
            .find(|t| t.node_id == Some(2) && t.kind == "node")
            .unwrap();
        assert!(
            door_trace.ok,
            "accepted does not mean the interlocked door opened"
        );
        assert!(door_trace
            .message
            .contains("door(cmd=true,effective=false)"));
        assert!(door_trace.message.contains("interlock=Pressure"));
        assert!(door_trace.message.contains("pressure="));
        assert!(door_trace.message.chars().count() <= 256);
        w.graph_step().unwrap();
        let room = w
            .handle(&mut session, Request::Snapshot)
            .assets
            .unwrap()
            .pioneer_room
            .unwrap();
        assert!(!room.breaker_closed && !room.light_on);
        w.disconnect(&mut session);
        assert!(w.graph_trigger(1).is_err());
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
}
