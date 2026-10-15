//! In-process editor authority. No transport request can control this clock.
use crate::*;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[path = "editor_events.rs"]
mod events;

pub const EDITOR_TICK_MS: u64 = 50;
pub const MAX_EDITOR_ADVANCE_MS: u64 = 250;
const MAX_ROOM_LAYOUT_FILE_BYTES: usize = 64 * 1024;

fn load_room_layout(path: &Path) -> Result<RoomLayoutState> {
    let path_metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RoomLayoutState {
                revision: 0,
                document: default_pioneer_room_layout(),
            });
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(
        path_metadata.is_file() && !path_metadata.file_type().is_symlink(),
        "room layout state must be a regular file"
    );

    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file(),
        "room layout state must be a regular file"
    );
    ensure!(
        metadata.len() <= MAX_ROOM_LAYOUT_FILE_BYTES as u64,
        "room layout state exceeds the file size limit"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.nlink() == 1,
            "room layout state must not be hardlinked"
        );
    }
    let mut bounded = file.take(MAX_ROOM_LAYOUT_FILE_BYTES as u64 + 1);
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    bounded.read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_ROOM_LAYOUT_FILE_BYTES,
        "room layout state exceeds the file size limit"
    );
    let state = serde_json::from_slice::<RoomLayoutState>(&bytes)?;
    state.document.validate_live().map_err(anyhow::Error::msg)?;
    Ok(state)
}

fn persist_room_layout(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("room layout path has no parent")?;
    let temp_path = parent.join(format!(
        ".room_layout-{}-{}.tmp",
        std::process::id(),
        rand::random::<u64>()
    ));
    let result = (|| -> Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path).context("atomically replace room layout state")?;
        // The rename has committed. Directory sync is best effort so a sync
        // error cannot leave disk advanced while memory/revision stays old.
        if let Ok(directory) = File::open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}

/// Owns a local-development world and always reopens paused. Play time comes only
/// from explicit bounded deltas, never elapsed wall time. Server is deliberately
/// private: callers cannot accidentally invoke its wall-clock tick/handle APIs.
pub struct EditorWorld {
    server: Server,
    paused: bool,
    remainder_ms: u64,
    graph: events::GraphRuntime,
    graph_session: Option<String>,
    room_layout_path: PathBuf,
    room_layout: RoomLayoutState,
}
impl EditorWorld {
    pub fn open(directory: impl AsRef<Path>) -> Result<Self> {
        let path = local_development::database_path(directory.as_ref())?;
        let room_layout_path = path
            .parent()
            .context("editor authority database has no directory")?
            .join("room_layout.json");
        let room_layout = load_room_layout(&room_layout_path)?;
        let server = Server::open_editor_realm(&path, room_layout.document.clone())?;
        server.validate_editor_room_layout(&room_layout.document)?;
        Ok(Self {
            server,
            paused: true,
            remainder_ms: 0,
            graph: events::GraphRuntime::default(),
            graph_session: None,
            room_layout_path,
            room_layout,
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
        let mut response = self.server.handle_at(session, request, self.time());
        if let Some(assets) = response.assets.as_mut() {
            assets.pioneer_room_layout = Some(self.room_layout.clone());
        }
        if connect && response.ok {
            // A second pilot joins the same editor authority. Keep the existing
            // graph executor and its trace; disconnect clears work bound to that
            // executor rather than silently transferring it to another pilot.
            if self.graph_session.is_none() {
                self.graph_session = session.clone();
            }
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
    /// Explicit sandbox placement of the caller's own avatar. This bypasses
    /// walking routes only in the editor; it never moves or docks a ship and
    /// never advances simulation systems, including the paused clock.
    pub fn set_scene_state(
        &mut self,
        session: &Option<String>,
        state: EditorSceneState,
    ) -> Result<()> {
        let name = session
            .as_ref()
            .context("connect an editor pilot before changing scene state")?;
        ensure!(
            self.server.online_sessions.contains_key(name),
            "editor pilot is not connected"
        );
        if matches!(
            state,
            EditorSceneState::InteriorDeck1 | EditorSceneState::InteriorDeck2
        ) {
            ensure!(
                !self.server.interior_incompatible.contains(name),
                "close incompatible clients before entering the interior"
            );
        }
        let now = self.time();
        let tx = self.server.db.transaction()?;
        let json: String =
            tx.query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))?;
        let mut world: World = serde_json::from_str(&json)?;
        ensure!(
            !world.remote_controls.contains_key(name)
                && !world.remote_controls.values().any(|pilot| pilot == name),
            "end remote control before changing editor scene state"
        );
        let fleet = world
            .accounts
            .get(name)
            .is_some_and(|account| account.fleet_hangar);
        let pilot = world.pilots.get_mut(name).context("editor pilot missing")?;
        ensure!(
            pilot.view.location == Location::Docked,
            "dock your ship before changing editor scene state"
        );
        let avatar = match state {
            EditorSceneState::Cockpit => None,
            EditorSceneState::StationHangar => {
                let position = if fleet {
                    fleet_berth(pilot.view.ship_class).1
                } else {
                    hangar_layout(pilot.view.ship_class).boarding_point
                };
                Some(StationAvatar {
                    onboard_interior: false,
                    interior_deck: 1,
                    position,
                    heading: 0.0,
                    seated: false,
                    moving: false,
                    jump_started_ms: None,
                    updated_at_ms: now,
                })
            }
            EditorSceneState::InteriorDeck1 | EditorSceneState::InteriorDeck2 => {
                ensure!(
                    pilot.view.ship_class == ShipClass::Pioneer,
                    "only Pioneer interiors are walkable"
                );
                let deck = if state == EditorSceneState::InteriorDeck1 {
                    1
                } else {
                    2
                };
                Some(StationAvatar {
                    onboard_interior: true,
                    interior_deck: deck,
                    position: if deck == 1 {
                        PIONEER_INTERIOR_ENTRY
                    } else {
                        PIONEER_LIFT_POINT
                    },
                    heading: std::f64::consts::FRAC_PI_2,
                    seated: false,
                    moving: false,
                    jump_started_ms: None,
                    updated_at_ms: now,
                })
            }
        };
        pilot.view.station_avatar = avatar;
        pilot.walk_input = None;
        pilot.controls = None;
        validate(&world, self.server.editor_room_layout.as_ref())?;
        tx.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&world)?],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn room_layout_state(&self) -> RoomLayoutState {
        self.room_layout.clone()
    }
    pub fn exterior_state(&self) -> Result<ExteriorDefinitionState> {
        let json: String =
            self.server
                .db
                .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let world: World = serde_json::from_str(&json)?;
        Ok(world.exterior_definition)
    }
    /// Adopt one project definition for every Pioneer replica. The accepted
    /// revision, circuit load anchors and room state share one SQLite commit.
    pub fn exterior_replace(
        &mut self,
        expected_revision: u64,
        document: ExteriorDefinition,
    ) -> Result<ExteriorDefinitionState> {
        document.validate().map_err(anyhow::Error::msg)?;
        let now = self.time();
        let tx = self.server.db.transaction()?;
        let json: String = tx.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json)?;
        ensure!(
            world.exterior_definition.revision == expected_revision,
            "exterior definition revision conflict: expected {}, current {}",
            expected_revision,
            world.exterior_definition.revision
        );
        assets::ensure_accounts(&mut world);
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        let next = ExteriorDefinitionState {
            revision: expected_revision
                .checked_add(1)
                .context("exterior definition revision exhausted")?,
            document,
        };
        world.exterior_definition = next.clone();
        exterior_effects::anchor(&mut world, now);
        validate(&world, self.server.editor_room_layout.as_ref())?;
        tx.execute(
            "UPDATE world SET json=?1 WHERE id=1",
            [serde_json::to_string(&world)?],
        )?;
        tx.commit()?;
        Ok(next)
    }
    /// Paused authoring operates the same owned circuit command at frozen time.
    pub fn exterior_set_enabled(
        &mut self,
        session: &mut Option<String>,
        enabled: bool,
        expected_revision: u64,
    ) -> Result<()> {
        let response = self.handle(
            session,
            Request::SetPioneerExterior {
                enabled,
                expected_revision,
            },
        );
        ensure!(response.ok, "{}", response.message);
        Ok(())
    }
    pub fn room_layout_replace(
        &mut self,
        expected_revision: u64,
        document: RoomLayoutDocument,
    ) -> Result<RoomLayoutState> {
        ensure!(
            expected_revision == self.room_layout.revision,
            "room layout revision conflict: expected {}, current {}",
            expected_revision,
            self.room_layout.revision
        );
        document.validate_live().map_err(anyhow::Error::msg)?;
        let next = RoomLayoutState {
            revision: self
                .room_layout
                .revision
                .checked_add(1)
                .context("room layout revision exhausted")?,
            document,
        };
        let bytes = serde_json::to_vec(&next)?;
        ensure!(
            bytes.len() <= MAX_ROOM_LAYOUT_FILE_BYTES,
            "room layout state exceeds the file size limit"
        );
        // Validate all persisted avatars before writing. No pilot is relocated
        // to accommodate an authored geometry change.
        self.server.validate_editor_room_layout(&next.document)?;
        persist_room_layout(&self.room_layout_path, &bytes)?;
        self.server.set_editor_room_layout(next.document.clone());
        self.room_layout = next.clone();
        Ok(next)
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
        Request::SetPioneerExterior { .. } => "set_pioneer_exterior",
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
    #[test]
    fn exterior_apply_is_atomic_preserves_room_state_and_reopens_paused_without_replay() {
        let path = dir();
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        let token = call(&mut w, &mut session, connect_interior()).resume_token;
        call(
            &mut w,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Breaker { closed: true },
            },
        );
        let room_before = call(&mut w, &mut session, Request::Snapshot)
            .assets
            .unwrap()
            .pioneer_room;
        let mut document = default_pioneer_exterior();
        document.fixtures[3].pulse.as_mut().unwrap().on_ms = 50;
        let accepted = w.exterior_replace(0, document.clone()).unwrap();
        assert_eq!(accepted.revision, 1);
        assert_eq!(w.time(), 0);
        assert_eq!(
            call(&mut w, &mut session, Request::Snapshot)
                .assets
                .unwrap()
                .pioneer_room,
            room_before
        );
        assert!(w.exterior_replace(0, document.clone()).is_err());
        let mut wrong_hull = document.clone();
        wrong_hull.hull_asset_sha256 = "0".repeat(64);
        assert!(w.exterior_replace(1, wrong_hull).is_err());
        let before = persisted_world(&w);
        w.server.db.execute_batch("CREATE TRIGGER reject_exterior BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT, 'test exterior write failure'); END;").unwrap();
        document.fixtures[0].load_w += 1;
        assert!(w.exterior_replace(1, document).is_err());
        assert_eq!(w.exterior_state().unwrap(), accepted);
        assert_eq!(persisted_world(&w), before);
        w.server
            .db
            .execute_batch("DROP TRIGGER reject_exterior")
            .unwrap();
        w.exterior_set_enabled(&mut session, false, 1).unwrap();
        let off = snapshot(&mut w).exterior.unwrap();
        assert!(!off.vessels[0].powered);
        w.advance(250).unwrap();
        assert_eq!(snapshot(&mut w).exterior.unwrap(), off);
        w.exterior_set_enabled(&mut session, true, 1).unwrap();
        let on = snapshot(&mut w).exterior.unwrap();
        assert_eq!(
            on.definition.document.fixtures[3].output(&on.vessels[0], w.time()),
            1.0
        );
        w.step(50).unwrap();
        assert_eq!(w.time(), 50);
        let stepped = snapshot(&mut w).exterior.unwrap();
        assert_eq!(
            stepped.definition.document.fixtures[3].output(&stepped.vessels[0], w.time()),
            0.0
        );
        w.exterior_set_enabled(&mut session, false, 1).unwrap();
        let saved = snapshot(&mut w).exterior.unwrap();
        drop(w);
        let mut w = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut w, &mut session, connect(token));
        assert!(w.is_paused());
        assert_eq!(w.time(), 50);
        assert_eq!(snapshot(&mut w).exterior.unwrap(), saved);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
    fn dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("arkline-editor-clock-{}", rand::random::<u64>()))
    }
    fn connect(token: Option<String>) -> Request {
        serde_json::from_value(serde_json::json!({"command":"connect","protocol_version":PROTOCOL_VERSION,"pilot":"EditorPilot","resume_token":token})).unwrap()
    }
    fn connect_interior() -> Request {
        Request::Connect {
            protocol_version: PROTOCOL_VERSION,
            pilot: "EditorPilot".into(),
            resume_token: None,
            supports_room_controls: true,
            supports_fleet_hangar: true,
            supports_pioneer_interior: true,
        }
    }
    fn call(w: &mut EditorWorld, s: &mut Option<String>, r: Request) -> Response {
        let response = w.handle(s, r);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn snapshot(w: &mut EditorWorld) -> Snapshot {
        call(w, &mut None, Request::Snapshot).snapshot.unwrap()
    }
    fn walk_for(
        world: &mut EditorWorld,
        session: &mut Option<String>,
        forward: f64,
        strafe: f64,
        duration_ms: u64,
    ) {
        call(
            world,
            session,
            Request::StationWalk {
                forward,
                strafe,
                turn: 0.0,
            },
        );
        let mut remaining = duration_ms;
        while remaining > 0 {
            let step = remaining.min(50);
            world.step(step).unwrap();
            remaining -= step;
        }
    }
    fn assert_airlock_exit(path: &std::path::Path, layout: Option<RoomLayoutDocument>) {
        let mut world = EditorWorld::open(path).unwrap();
        if let Some(document) = layout {
            world.room_layout_replace(0, document).unwrap();
        }
        let mut session = None;
        call(&mut world, &mut session, connect_interior());
        call(&mut world, &mut session, Request::EnterInterior);

        call(
            &mut world,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: false,
                    manual: true,
                },
            },
        );
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        let held_at_closed_door = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(held_at_closed_door[2] <= 5.3, "{held_at_closed_door:?}");
        assert!(!world.handle(&mut session, Request::ExitInterior).ok);

        call(
            &mut world,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: true,
                    manual: true,
                },
            },
        );
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        let corridor = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(corridor[2] > 6.5, "{corridor:?}");
        assert!(!world.handle(&mut session, Request::ExitInterior).ok);

        walk_for(&mut world, &mut session, -1.0, 0.0, 500);
        walk_for(&mut world, &mut session, -1.0, 0.0, 500);
        let at_airlock = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(
            (at_airlock[2] - PIONEER_INTERIOR_ENTRY[2]).abs() <= 0.3 + 1e-9,
            "{at_airlock:?}"
        );
        call(&mut world, &mut session, Request::ExitInterior);
        let exited_snapshot = snapshot(&mut world);
        let exited = exited_snapshot.pilots[0].station_avatar.as_ref().unwrap();
        assert!(!exited.onboard_interior);
        drop(world);
        std::fs::remove_dir_all(path).unwrap();
    }
    fn short_p2_layout() -> RoomLayoutDocument {
        let mut document = default_pioneer_room_layout();
        document
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
            .unwrap()
            .clear_lwh_m[0] = 8.0;
        document.validate_live().unwrap();
        document
    }
    #[test]
    fn room_layout_loader_rejects_oversized_and_symlink_files() {
        let path = dir();
        std::fs::create_dir_all(&path).unwrap();
        let layout_path = path.join("room_layout.json");
        std::fs::write(&layout_path, vec![b' '; MAX_ROOM_LAYOUT_FILE_BYTES + 1]).unwrap();
        let oversized = load_room_layout(&layout_path).unwrap_err();
        assert!(oversized.to_string().contains("file size limit"));

        #[cfg(unix)]
        {
            let target = path.join("layout-target.json");
            let valid = RoomLayoutState {
                revision: 0,
                document: default_pioneer_room_layout(),
            };
            std::fs::write(&target, serde_json::to_vec(&valid).unwrap()).unwrap();
            std::fs::remove_file(&layout_path).unwrap();
            std::os::unix::fs::symlink(&target, &layout_path).unwrap();
            assert!(load_room_layout(&layout_path)
                .unwrap_err()
                .to_string()
                .contains("regular file"));
        }
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn externally_replaced_layout_cannot_relocate_persisted_avatar_on_reopen() {
        let path = dir();
        let mut world = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut world, &mut session, connect_interior());
        call(&mut world, &mut session, Request::EnterInterior);
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        for _ in 0..4 {
            walk_for(&mut world, &mut session, 0.0, -1.0, 500);
        }
        walk_for(&mut world, &mut session, 0.0, -1.0, 400);
        walk_for(&mut world, &mut session, -1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 0.0, -1.0, 500);
        walk_for(&mut world, &mut session, 0.0, -1.0, 250);
        let avatar = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(avatar[0] > 21.0, "{avatar:?}");
        drop(world);

        let db_path = path.join("world.sqlite");
        let db = rusqlite::Connection::open(&db_path).unwrap();
        let before: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
            .unwrap();
        drop(db);
        let replacement = RoomLayoutState {
            revision: 1,
            document: short_p2_layout(),
        };
        std::fs::write(
            path.join("room_layout.json"),
            serde_json::to_vec(&replacement).unwrap(),
        )
        .unwrap();
        assert!(EditorWorld::open(&path).is_err());

        let db = rusqlite::Connection::open(&db_path).unwrap();
        let after: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, before);
        drop(db);
        std::fs::remove_dir_all(path).unwrap();
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

    #[test]
    fn live_layout_changes_drive_real_walk_collision_and_reject_trapping_or_stale_edits() {
        let path = dir();
        let mut world = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut world, &mut session, connect_interior());
        call(&mut world, &mut session, Request::EnterInterior);

        // Walk from the airlock, through the corridor and D1-P2 doorway, then
        // toward its east wall. The original wall at x=22.466 m permits this.
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        for _ in 0..4 {
            walk_for(&mut world, &mut session, 0.0, -1.0, 500);
        }
        walk_for(&mut world, &mut session, 0.0, -1.0, 400);
        walk_for(&mut world, &mut session, -1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 0.0, -1.0, 500);
        walk_for(&mut world, &mut session, 0.0, -1.0, 250);
        let baseline = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(baseline[0] > 21.0 && baseline[2] < 5.8, "{baseline:?}");

        // Return through the same doorway so the edit is safe to apply.
        walk_for(&mut world, &mut session, 0.0, 1.0, 500);
        walk_for(&mut world, &mut session, 0.0, 1.0, 250);
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        for _ in 0..5 {
            walk_for(&mut world, &mut session, 0.0, 1.0, 500);
        }
        walk_for(&mut world, &mut session, 0.0, -1.0, 100);
        walk_for(&mut world, &mut session, -1.0, 0.0, 500);
        walk_for(&mut world, &mut session, -1.0, 0.0, 500);
        let entry = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(
            (entry[0] - PIONEER_INTERIOR_ENTRY[0]).abs() < 0.01,
            "{entry:?}"
        );
        assert!(
            (entry[2] - PIONEER_INTERIOR_ENTRY[2]).abs() < 0.01,
            "{entry:?}"
        );

        let state = world.room_layout_replace(0, short_p2_layout()).unwrap();
        assert_eq!(state.revision, 1);
        assert!(world
            .room_layout_replace(0, state.document.clone())
            .is_err());
        let persisted_path = path.join("room_layout.json");
        assert!(persisted_path.is_file());

        // The shortened authored wall now blocks authoritative StationWalk at
        // x≈21.18 m; the request never moves the pilot through it.
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        for _ in 0..4 {
            walk_for(&mut world, &mut session, 0.0, -1.0, 500);
        }
        walk_for(&mut world, &mut session, 0.0, -1.0, 400);
        walk_for(&mut world, &mut session, -1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 0.0, -1.0, 500);
        walk_for(&mut world, &mut session, 0.0, -1.0, 500);
        let stopped = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(stopped[0] > 20.9 && stopped[0] <= 21.2, "{stopped:?}");

        // Moving the same wall farther west would overlap the active avatar.
        // Reject before disk/revision/world state changes; never teleport it.
        let mut trapping = state.document.clone();
        trapping
            .rooms
            .iter_mut()
            .find(|room| room.source_room_id.as_deref() == Some("D1-P2"))
            .unwrap()
            .clear_lwh_m[0] = 7.5;
        trapping.validate_live().unwrap();
        assert!(world.room_layout_replace(1, trapping).is_err());
        assert_eq!(world.room_layout_state(), state);
        let still_stopped = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert_eq!(still_stopped, stopped);
        let disk: RoomLayoutState =
            serde_json::from_slice(&std::fs::read(&persisted_path).unwrap()).unwrap();
        assert_eq!(disk, state);

        // A disk failure must leave both the live layout and its revision at
        // the last durable value.
        let backup_path = path.join("room_layout.backup");
        std::fs::rename(&persisted_path, &backup_path).unwrap();
        std::fs::create_dir(&persisted_path).unwrap();
        let mut safe_again = state.document.clone();
        safe_again.rooms[0].name.push_str(" save failure");
        assert!(world.room_layout_replace(1, safe_again).is_err());
        assert_eq!(world.room_layout_state(), state);
        std::fs::remove_dir(&persisted_path).unwrap();
        std::fs::rename(&backup_path, &persisted_path).unwrap();
        drop(world);
        let reopened = EditorWorld::open(&path).unwrap();
        assert_eq!(reopened.room_layout_state(), state);
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn safe_layout_replace_is_allowed_while_playing_and_is_in_snapshot_assets() {
        let path = dir();
        let mut world = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut world, &mut session, connect(None));
        world.play();
        world.advance(50).unwrap();
        let before = world.room_layout_state();
        let mut document = before.document.clone();
        document.rooms[0].name.push_str(" live");
        let next = world
            .room_layout_replace(before.revision, document)
            .unwrap();
        assert_eq!(next.revision, before.revision + 1);
        let response = call(&mut world, &mut session, Request::Snapshot);
        assert_eq!(
            response.assets.unwrap().pioneer_room_layout,
            Some(next.clone())
        );
        drop(world);
        std::fs::remove_dir_all(path).unwrap();
    }

    fn persisted_world(world: &EditorWorld) -> String {
        world
            .server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))
            .unwrap()
    }

    #[test]
    fn scene_state_jumps_preserve_clock_systems_and_use_valid_landings() {
        let path = dir();
        let mut world = EditorWorld::open(&path).unwrap();
        let mut session = None;
        call(&mut world, &mut session, connect_interior());
        world
            .set_scene_state(&session, EditorSceneState::InteriorDeck1)
            .unwrap();
        call(
            &mut world,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: false,
                    manual: true,
                },
            },
        );
        call(
            &mut world,
            &mut session,
            Request::StationWalk {
                forward: 1.0,
                strafe: 0.0,
                turn: 0.0,
            },
        );
        call(&mut world, &mut session, Request::StationJump);
        let before: World = serde_json::from_str(&persisted_world(&world)).unwrap();
        let time = world.time();
        for (state, deck, position) in [
            (EditorSceneState::InteriorDeck2, 2, PIONEER_LIFT_POINT),
            (EditorSceneState::InteriorDeck1, 1, PIONEER_INTERIOR_ENTRY),
        ] {
            world.set_scene_state(&session, state).unwrap();
            let after: World = serde_json::from_str(&persisted_world(&world)).unwrap();
            let avatar = after.pilots["EditorPilot"]
                .view
                .station_avatar
                .as_ref()
                .unwrap();
            assert!(avatar.onboard_interior);
            assert_eq!(avatar.interior_deck, deck);
            assert_eq!(avatar.position, position);
            assert!(!avatar.moving);
            assert!(avatar.jump_started_ms.is_none());
            assert!(after.pilots["EditorPilot"].walk_input.is_none());
            assert_eq!(
                serde_json::to_value(&before.accounts).unwrap(),
                serde_json::to_value(&after.accounts).unwrap()
            );
            assert_eq!(world.time(), time);
            assert!(world.is_paused());
            // The same validator and authored geometry drive real collision.
            validate(&after, Some(&world.room_layout.document)).unwrap();
        }
        world.play();
        world
            .set_scene_state(&session, EditorSceneState::StationHangar)
            .unwrap();
        assert!(!world.is_paused());
        assert_eq!(world.time(), time);
        let after: World = serde_json::from_str(&persisted_world(&world)).unwrap();
        let avatar = after.pilots["EditorPilot"]
            .view
            .station_avatar
            .as_ref()
            .unwrap();
        assert!(!avatar.onboard_interior);
        assert_eq!(avatar.position, fleet_berth(ShipClass::Pioneer).1);
        world
            .set_scene_state(&session, EditorSceneState::Cockpit)
            .unwrap();
        assert!(snapshot(&mut world).pilots[0].station_avatar.is_none());
        world.pause();
        world
            .set_scene_state(&session, EditorSceneState::InteriorDeck2)
            .unwrap();
        drop(world);
        let reopened = EditorWorld::open(&path).unwrap();
        assert_eq!(reopened.time(), time);
        let after: World = serde_json::from_str(&persisted_world(&reopened)).unwrap();
        assert_eq!(
            after.pilots["EditorPilot"]
                .view
                .station_avatar
                .as_ref()
                .unwrap()
                .interior_deck,
            2
        );
        drop(reopened);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn scene_state_jump_keeps_authored_collision_and_airlock_door_enforced() {
        let path = dir();
        let mut world = EditorWorld::open(&path).unwrap();
        world.room_layout_replace(0, short_p2_layout()).unwrap();
        let mut session = None;
        call(&mut world, &mut session, connect_interior());
        world
            .set_scene_state(&session, EditorSceneState::InteriorDeck1)
            .unwrap();
        call(
            &mut world,
            &mut session,
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: false,
                    manual: true,
                },
            },
        );
        // Development placement does not grant subsequent gameplay actions a
        // doorway bypass; the persisted room controller still blocks movement.
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        walk_for(&mut world, &mut session, 1.0, 0.0, 500);
        let position = snapshot(&mut world).pilots[0]
            .station_avatar
            .as_ref()
            .unwrap()
            .position;
        assert!(position[2] <= 5.3, "closed door was crossed: {position:?}");
        assert!(!world.handle(&mut session, Request::ExitInterior).ok);
        world
            .set_scene_state(&session, EditorSceneState::InteriorDeck2)
            .unwrap();
        let persisted: World = serde_json::from_str(&persisted_world(&world)).unwrap();
        station::validate(&persisted, Some(&world.room_layout.document)).unwrap();
        drop(world);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn rejected_scene_state_jumps_do_not_mutate_or_dock_the_ship() {
        let path = dir();
        let mut world = EditorWorld::open(&path).unwrap();
        let mut session = None;
        let mut request = connect_interior();
        if let Request::Connect {
            supports_fleet_hangar,
            ..
        } = &mut request
        {
            *supports_fleet_hangar = false;
        }
        call(&mut world, &mut session, request);
        let before = persisted_world(&world);
        assert!(world
            .set_scene_state(&None, EditorSceneState::InteriorDeck1)
            .is_err());
        assert_eq!(persisted_world(&world), before);
        call(
            &mut world,
            &mut session,
            Request::SelectShip {
                ship_class: ShipClass::Prospector,
            },
        );
        let before = persisted_world(&world);
        assert!(world
            .set_scene_state(&session, EditorSceneState::InteriorDeck1)
            .is_err());
        assert_eq!(persisted_world(&world), before);
        world
            .set_scene_state(&session, EditorSceneState::Cockpit)
            .unwrap();
        call(&mut world, &mut session, Request::Undock);
        let before = persisted_world(&world);
        let time = world.time();
        for state in [
            EditorSceneState::Cockpit,
            EditorSceneState::StationHangar,
            EditorSceneState::InteriorDeck2,
        ] {
            assert!(world.set_scene_state(&session, state).is_err());
            assert_eq!(persisted_world(&world), before);
            assert_eq!(world.time(), time);
        }
        // Persistence failure also leaves the previous avatar untouched.
        call(&mut world, &mut session, Request::Dock);
        let before = persisted_world(&world);
        world.server.db.execute_batch("CREATE TRIGGER reject_scene BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT, 'test scene write failure'); END;").unwrap();
        assert!(world
            .set_scene_state(&session, EditorSceneState::StationHangar)
            .is_err());
        assert_eq!(persisted_world(&world), before);
        drop(world);
        std::fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn pioneer_airlock_exit_works_with_default_and_edited_layouts_and_respects_door() {
        assert_airlock_exit(&dir(), None);
        assert_airlock_exit(&dir(), Some(short_p2_layout()));
    }
}

#[cfg(test)]
mod graph_tests {
    use super::*;
    fn setup() -> (std::path::PathBuf, EditorWorld, Option<String>) {
        setup_with_room_support(true)
    }
    fn setup_with_room_support(
        supports_room_controls: bool,
    ) -> (std::path::PathBuf, EditorWorld, Option<String>) {
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
                supports_room_controls,
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

    #[test]
    fn rejected_graph_room_command_is_traced_and_does_not_follow_edge() {
        let (path, mut w, mut session) = setup_with_room_support(false);
        let before = w
            .handle(&mut session, Request::Snapshot)
            .assets
            .unwrap()
            .pioneer_room;
        w.graph_replace(
            0,
            chain(vec![
                EditorNodeOp::Manual,
                EditorNodeOp::RoomLight { on: true },
                EditorNodeOp::Trace {
                    message: "must not run after rejected command".into(),
                },
            ]),
            false,
        )
        .unwrap();
        w.graph_trigger(1).unwrap();
        w.graph_step().unwrap();
        assert!(w.is_paused());
        w.graph_step().unwrap();

        let debug = w.graph_debug();
        assert!(debug.paused);
        assert_eq!(debug.pending, 0);
        let rejected = debug
            .trace
            .iter()
            .rev()
            .find(|t| t.node_id == Some(2) && t.kind == "node")
            .unwrap();
        assert!(!rejected.ok);
        assert!(rejected.message.contains("room state unavailable"));
        assert!(!debug
            .trace
            .iter()
            .any(|t| t.message == "must not run after rejected command"));
        let after = w
            .handle(&mut session, Request::Snapshot)
            .assets
            .unwrap()
            .pioneer_room;
        assert_eq!(after, before);
        drop(w);
        std::fs::remove_dir_all(path).unwrap();
    }
}
