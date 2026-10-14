pub mod access_tokens;
mod assets;
pub mod collaborative_editor;
pub mod collaborative_transport;
mod drones;
pub mod editor;
mod flight;
mod identity;
pub mod local_development;
mod mining;
mod power;
mod remote_control;
mod resource_jobs;
mod room_controls;
mod station;
mod surface;
mod traffic;
mod universe;
use anyhow::{bail, ensure, Context, Result};
use arkline_protocol::*;
use fs2::FileExt;
use rand::{rngs::OsRng, RngCore};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
struct Pilot {
    view: PilotView,
    token_hash: String,
    #[serde(default)]
    identity: Option<identity::Binding>,
    last_mine_ms: Option<u64>,
    #[serde(default)]
    warp: Option<flight::WarpPath>,
    #[serde(default)]
    controls: Option<flight::ControlInput>,
    #[serde(default)]
    walk_input: Option<station::WalkInput>,
}
#[derive(Clone, Serialize, Deserialize)]
struct World {
    #[serde(default)]
    remote_controls: BTreeMap<String, String>,
    #[serde(default)]
    fleet_links: BTreeMap<String, String>,
    #[serde(default)]
    resource_jobs: BTreeMap<String, resource_jobs::Job>,
    #[serde(default)]
    next_job_id: u64,
    #[serde(default)]
    surface_props: Vec<SurfaceProp>,
    #[serde(default)]
    next_prop_id: u64,
    #[serde(default = "default_next_drone_id")]
    next_drone_id: u64,
    #[serde(default)]
    accounts: BTreeMap<String, assets::Account>,
    #[serde(default)]
    corporations: BTreeMap<String, assets::Corporation>,
    pilots: BTreeMap<String, Pilot>,
    orders: BTreeMap<u64, SellOrder>,
    belt_ore: u64,
    next_order: u64,
    #[serde(default)]
    asteroids: Vec<Asteroid>,
    #[serde(default)]
    spatial_version: u32,
}
fn default_next_drone_id() -> u64 {
    1
}
impl Default for World {
    fn default() -> Self {
        Self {
            remote_controls: BTreeMap::new(),
            fleet_links: BTreeMap::new(),
            resource_jobs: BTreeMap::new(),
            next_job_id: 1,
            surface_props: Vec::new(),
            next_prop_id: 1,
            next_drone_id: 1,
            accounts: BTreeMap::new(),
            corporations: BTreeMap::new(),
            pilots: BTreeMap::new(),
            orders: BTreeMap::new(),
            belt_ore: INITIAL_BELT_ORE,
            next_order: 1,
            asteroids: asteroid_catalog(INITIAL_BELT_ORE),
            spatial_version: 2,
        }
    }
}
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Release flock explicitly: a concurrently forked child may still hold a copy
/// of the open file description until exec, even though this File is CLOEXEC.
/// Stored after SQLite in Server so authority is released only after DB close.
struct OwnershipLock(std::fs::File);
impl Drop for OwnershipLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

/// One connection, protected by the transport's mutex, serializes all writes.
/// The durable state and its economic event commit in one SQLite transaction.
pub struct Server {
    local_development: bool,
    editor_time: Option<u64>,
    db: Connection,
    auth: Option<identity::Authenticator>,
    online_sessions: BTreeMap<String, usize>,
    // Conservative latch: clear only after ALL sessions for this pilot leave.
    interior_incompatible: std::collections::BTreeSet<String>,
    fleet_incompatible: std::collections::BTreeSet<String>,
    room_incompatible: std::collections::BTreeSet<String>,
    // Kept after db so SQLite closes before authority is released. Never unlink
    // the sidecar: replacing its inode would allow competing locks.
    _ownership_lock: Option<OwnershipLock>,
}
impl Server {
    pub fn is_local_development(&self) -> bool {
        self.local_development
    }
    pub fn open_local_development(directory: impl AsRef<Path>) -> Result<Self> {
        Self::open_realm(
            local_development::database_path(directory.as_ref())?,
            true,
            false,
        )
    }
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_realm(path, false, false)
    }
    fn open_realm(
        path: impl AsRef<Path>,
        local_development: bool,
        editor_mode: bool,
    ) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let ownership_lock = if path == Path::new(":memory:") {
            None
        } else {
            // Resolve directory and existing-file symlinks to share a lock for
            // ordinary alternate spellings of the same database path.
            let resolved = if path.exists() {
                std::fs::canonicalize(path)?
            } else {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or(Path::new("."));
                std::fs::canonicalize(parent)?.join(
                    path.file_name()
                        .context("database path requires a filename")?,
                )
            };
            let mut sidecar = resolved.as_os_str().to_os_string();
            sidecar.push(".lock");
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(&sidecar)
                .context("cannot open database ownership lock")?;
            file.try_lock_exclusive().context("world database is already owned by another server (or ownership lock is unavailable)")?;
            Some(OwnershipLock(file))
        };
        let existed = path.exists();
        let mut db = Connection::open(path)?;
        local_development::establish_realm(&mut db, local_development, existed)?;
        #[cfg(unix)]
        if local_development {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                path.parent().unwrap(),
                std::fs::Permissions::from_mode(0o700),
            )?;
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            CREATE TABLE IF NOT EXISTS world (id INTEGER PRIMARY KEY CHECK(id=1), json TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS ledger (sequence INTEGER PRIMARY KEY, json TEXT NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS identity_audit (pilot TEXT PRIMARY KEY, issuer TEXT NOT NULL, subject TEXT NOT NULL, linked_at_ms INTEGER NOT NULL);")?;
        db.execute(
            "INSERT OR IGNORE INTO world(id,json) VALUES(1,?1)",
            [serde_json::to_string(&World::default())?],
        )?;
        let json: String = db.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json)?;
        let editor_time = editor::open_clock(&mut db, editor_mode)?;
        let now = editor_time.unwrap_or_else(now_ms);
        flight::migrate_spatial(&mut world);
        universe::ensure_asteroids(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        world.remote_controls.clear();
        resource_jobs::cancel_all(&mut world, now, "Server restarted");
        station::migrate(&mut world);
        for pilot in world.pilots.values_mut() {
            mining::stop_all(pilot, "Server restarted");
            pilot.controls = None;
            station::stop(pilot);
            pilot.view.target_locks.clear();
            universe::sync_lock_mirrors(pilot);
            drones::clear(pilot);
            if !matches!(
                pilot.view.location,
                Location::InTransit { .. } | Location::Jumping { .. }
            ) {
                if let Some(flight) = &mut pilot.view.flight {
                    flight.mode = NavigationMode::Stop;
                    flight.velocity = [0.0; 3];
                    flight.acceleration = [0.0; 3];
                    flight.angular_velocity = [0.0; 3];
                }
            }
        }
        validate(&world)?;
        let stopped = serde_json::to_string(&world)?;
        if stopped != json {
            db.execute("UPDATE world SET json=?1 WHERE id=1", [stopped])?;
        }
        Ok(Self {
            local_development,
            editor_time,
            db,
            auth: None,
            online_sessions: BTreeMap::new(),
            interior_incompatible: Default::default(),
            fleet_incompatible: Default::default(),
            room_incompatible: Default::default(),
            _ownership_lock: ownership_lock,
        })
    }
    /// Additional public-WebSocket admission bounds; local operator transport remains available.
    /// Returns true only for an attempt to register a new pilot.
    pub fn check_public_request(
        &self,
        pilot: Option<&str>,
        request: &Request,
        max_pilots: usize,
    ) -> Result<bool> {
        if matches!(request, Request::ExportLedger) {
            let entries: u64 = self
                .db
                .query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get(0))?;
            ensure!(
                entries <= 10_000,
                "ledger exceeds public export limit; request an operator export"
            );
        }
        if !matches!(
            request,
            Request::Connect {
                resume_token: None,
                ..
            } | Request::AccountConnect { .. }
                | Request::PlaceSell { .. }
        ) {
            return Ok(false);
        }
        let raw: String = self
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let w: World = serde_json::from_str(&raw)?;
        match request {
            Request::Connect { pilot, .. } | Request::AccountConnect { pilot, .. }
                if !w.pilots.contains_key(pilot) =>
            {
                ensure!(
                    w.pilots.len() < max_pilots,
                    "public world registration capacity reached"
                );
                Ok(true)
            }
            Request::PlaceSell { .. } => {
                ensure!(w.orders.len() < 1024, "public open-order capacity reached");
                ensure!(
                    w.orders
                        .values()
                        .filter(|o| Some(o.seller.as_str()) == pilot)
                        .count()
                        < 64,
                    "pilot open-order capacity reached"
                );
                Ok(false)
            }
            _ => Ok(false),
        }
    }
    pub fn handle(&mut self, session: &mut Option<String>, request: Request) -> Response {
        self.handle_at(session, request, now_ms())
    }
    pub fn handle_at(
        &mut self,
        session: &mut Option<String>,
        request: Request,
        now: u64,
    ) -> Response {
        self.handle_identity_at(session, request, now, None)
    }
    fn handle_identity_at(
        &mut self,
        session: &mut Option<String>,
        mut request: Request,
        now: u64,
        account: Option<identity::Admission>,
    ) -> Response {
        if matches!(request, Request::Connect { .. })
            && account.is_none()
            && self.auth.as_ref().is_some_and(|a| a.required)
        {
            return Response::error("account authentication required");
        }
        let previous = session.clone();
        let legacy_room = matches!(
            &request,
            Request::Connect {
                supports_room_controls: false,
                ..
            }
        );
        if let Request::Connect {
            pilot,
            supports_room_controls,
            ..
        } = &mut request
        {
            if self.room_incompatible.contains(pilot) {
                *supports_room_controls = false;
            }
        }

        let legacy_fleet = matches!(
            &request,
            Request::Connect {
                supports_fleet_hangar: false,
                ..
            }
        );
        if let Request::Connect {
            pilot,
            supports_fleet_hangar,
            ..
        } = &mut request
        {
            if self.fleet_incompatible.contains(pilot) {
                *supports_fleet_hangar = false;
            }
        }

        let legacy_connect = matches!(
            &request,
            Request::Connect {
                supports_pioneer_interior: false,
                ..
            }
        );
        if matches!(&request, Request::EnterInterior)
            && session
                .as_ref()
                .is_some_and(|name| self.interior_incompatible.contains(name))
        {
            return Response::error("Close older or terminal clients, then disconnect and reconnect all sessions for this pilot before entering the interior");
        }
        match self.execute(session, request, now, false, account) {
            Ok(mut response) => {
                // execute changes the session only after a successful durable
                // commit. Repeated Connect on one socket must not double-count.
                if previous != *session {
                    if let Some(previous) = previous {
                        self.remove_presence(&previous);
                    }
                    if let Some(current) = session.as_ref() {
                        *self.online_sessions.entry(current.clone()).or_default() += 1;
                    }
                }
                if legacy_room {
                    if let Some(name) = session.as_ref() {
                        self.room_incompatible.insert(name.clone());
                    }
                }
                if legacy_fleet {
                    if let Some(name) = session.as_ref() {
                        self.fleet_incompatible.insert(name.clone());
                    }
                }
                if legacy_connect {
                    if let Some(name) = session.as_ref() {
                        self.interior_incompatible.insert(name.clone());
                    }
                }
                let online = self.online_sessions.keys().cloned().collect::<Vec<_>>();
                if let Some(snapshot) = &mut response.snapshot {
                    snapshot.online_pilots = online.clone();
                }
                if let Some(export) = &mut response.ledger {
                    export.snapshot.online_pilots = online;
                }
                response
            }
            Err(error) => Response::error(error.to_string()),
        }
    }
    /// Called exactly once by the transport session guard, on all exit paths.
    /// Presence is in-memory only; persistent ships and economy are untouched.
    pub fn disconnect(&mut self, session: &mut Option<String>) {
        if let Some(pilot) = session.take() {
            self.remove_presence(&pilot);
        }
    }
    fn remove_presence(&mut self, pilot: &str) {
        if let Some(count) = self.online_sessions.get_mut(pilot) {
            *count -= 1;
            if *count == 0 {
                self.online_sessions.remove(pilot);
                self.interior_incompatible.remove(pilot);
                self.fleet_incompatible.remove(pilot);
                self.room_incompatible.remove(pilot);
                if let Err(error) = self.stop_disconnected(pilot) {
                    eprintln!("Could not persist disconnected flight stop: {error}");
                }
            }
        }
    }
    pub fn tick(&mut self) -> Result<()> {
        self.tick_at(now_ms())
    }
    pub fn tick_at(&mut self, now: u64) -> Result<()> {
        if let Some(previous) = self.editor_time {
            ensure!(now >= previous, "editor time cannot move backwards");
        }
        let tx = self.db.transaction()?;
        let json: String = tx.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json)?;
        flight::migrate_spatial(&mut world);
        universe::ensure_asteroids(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        flight::advance(&mut world, now);
        station::advance(&mut world, now);
        surface::update(&mut world);
        universe::advance_locks(&mut world, now);
        let mut events = mining::advance(&mut world, now);
        events.extend(drones::advance(&mut world, now));
        universe::advance_locks(&mut world, now);
        mining::invalidate(&mut world);
        let mut entries = load_entries(&tx, false)?;
        for event in events {
            append_event(&tx, &mut entries, event, now)?;
        }
        validate(&world)?;
        let next = serde_json::to_string(&world)?;
        if next != json {
            tx.execute("UPDATE world SET json=?1 WHERE id=1", [next])?;
        }
        if self.editor_time.is_some() {
            tx.execute(
                "UPDATE editor_clock SET time_ms=?1 WHERE id=1",
                [now.to_string()],
            )?;
        }
        tx.commit()?;
        if self.editor_time.is_some() {
            self.editor_time = Some(now);
        }
        resource_jobs::advance(self, now)?;
        Ok(())
    }
    fn stop_disconnected(&mut self, name: &str) -> Result<()> {
        let tx = self.db.transaction()?;
        let json: String = tx.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json)?;
        flight::migrate_spatial(&mut world);
        universe::ensure_asteroids(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;

        let now = self.editor_time.unwrap_or_else(now_ms);
        remote_control::release_controller(&mut world, name);
        let controlled: Vec<String> = world
            .resource_jobs
            .iter()
            .filter(|(_, j)| j.view.controller == name && !j.view.phase.is_terminal())
            .map(|(n, _)| n.clone())
            .collect();
        for worker in controlled {
            resource_jobs::cancel(&mut world, &worker, now, "Controller disconnected");
        }
        let delegated = world.resource_jobs.get(name).is_some_and(|j| {
            !j.view.phase.is_terminal()
                && j.view.controller != name
                && self.online_sessions.contains_key(&j.view.controller)
        });
        let delegated = delegated
            || world.remote_controls.iter().any(|(controller, worker)| {
                worker == name && self.online_sessions.contains_key(controller)
            });
        if delegated {
            validate(&world)?;
            tx.execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&world)?],
            )?;
            tx.commit()?;
            return Ok(());
        }
        resource_jobs::cancel(&mut world, name, now, "Pilot disconnected");
        if let Some(p) = world.pilots.get_mut(name) {
            mining::stop_all(p, "Pilot disconnected");
            p.controls = None;
            station::stop(p);
            p.view.target_locks.clear();
            universe::sync_lock_mirrors(p);
            drones::clear(p);
            if !matches!(
                p.view.location,
                Location::InTransit { .. } | Location::Jumping { .. }
            ) {
                if let Some(f) = &mut p.view.flight {
                    f.mode = NavigationMode::Stop;
                    f.velocity = [0.0; 3];
                    f.acceleration = [0.0; 3];
                    f.angular_velocity = [0.0; 3];
                }
            }
        }
        validate(&world)?;
        let next = serde_json::to_string(&world)?;
        if next != json {
            tx.execute("UPDATE world SET json=?1 WHERE id=1", [next])?;
        }
        tx.commit()?;
        Ok(())
    }
    fn execute(
        &mut self,
        session: &mut Option<String>,
        request: Request,
        now: u64,
        automated: bool,
        account: Option<identity::Admission>,
    ) -> Result<Response> {
        let transport_session = session;
        let mut effective_session = transport_session.clone();
        let session = &mut effective_session;
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let json: String = tx.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json).context("invalid durable world")?;
        let remote = matches!(&request, Request::RemoteControl { .. });
        let request = if let Request::RemoteControl { pilot, request } = request {
            let controller = session
                .as_ref()
                .context("connect before issuing commands")?;
            ensure!(!automated, "job controller cannot issue remote wrappers");
            ensure!(
                world.pilots[controller].view.station_avatar.is_none(),
                "board your ship before remote control"
            );
            remote_control::validate_request(&request)?;
            ensure!(
                world.remote_controls.get(controller) == Some(&pilot),
                "remote control lease does not match requested pilot"
            );
            resource_jobs::authorize(&world, controller, &pilot)?;
            *session = Some(pilot);
            *request
        } else {
            request
        };

        flight::migrate_spatial(&mut world);
        universe::ensure_asteroids(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        flight::advance(&mut world, now);
        station::advance(&mut world, now);
        surface::update(&mut world);
        universe::advance_locks(&mut world, now);
        let mut token = None;
        let mut cycle_events = mining::advance(&mut world, now);
        cycle_events.extend(drones::advance(&mut world, now));
        universe::advance_locks(&mut world, now);
        mining::invalidate(&mut world);
        let mut event = None;
        let mut connected = None;
        let export = matches!(request, Request::ExportLedger);
        let message;
        if !automated
            && !matches!(
                request,
                Request::Snapshot
                    | Request::ExportLedger
                    | Request::Connect { .. }
                    | Request::StartResourceJob { .. }
                    | Request::CancelResourceJob { .. }
                    | Request::BeginRemoteControl { .. }
                    | Request::EndRemoteControl
            )
        {
            if let Some(name) = session.as_ref() {
                if !remote {
                    remote_control::release_worker(&mut world, name);
                }
                resource_jobs::cancel(&mut world, name, now, "Manual command took control");
            }
        }
        if let Some(name) = session.as_ref() {
            if world
                .pilots
                .get(name)
                .is_some_and(|p| p.view.station_avatar.is_some())
            {
                ensure!(
                    !request.is_remote_gameplay()
                        && !matches!(
                            request,
                            Request::SelectShip { .. } | Request::BeginRemoteControl { .. }
                        ),
                    "board your ship before issuing ship commands"
                );
            }
        }
        match request {
            request @ (Request::StationSit { .. }
            | Request::DisembarkShip
            | Request::EnterShip
            | Request::EnterInterior
            | Request::ExitInterior
            | Request::InteriorLift { .. }
            | Request::StationJump
            | Request::StationWalk { .. }) => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                ensure!(!remote, "station walking cannot be remotely delegated");
                if matches!(request, Request::DisembarkShip) {
                    remote_control::release_controller(&mut world, name);
                }
                station::execute(&mut world, name, request, now)?;
                message = "Station action accepted";
            }
            Request::BeginRemoteControl { pilot } => {
                let controller = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                ensure!(
                    world
                        .pilots
                        .get(&pilot)
                        .is_none_or(|p| p.view.station_avatar.is_none()),
                    "worker must board their ship before remote control"
                );
                remote_control::begin(&mut world, controller, &pilot, now)?;
                message = "Remote control granted";
            }
            Request::EndRemoteControl => {
                let controller = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                remote_control::release_controller(&mut world, controller);
                message = "Remote control released; own ship remains selected";
            }
            Request::StartResourceJob {
                target_units,
                pilot,
            } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                let worker = pilot.as_deref().unwrap_or(name);
                resource_jobs::authorize(&world, name, worker)?;
                ensure!(
                    !world.remote_controls.contains_key(name)
                        && !world.remote_controls.values().any(|p| p == worker),
                    "end remote control before starting a resource job"
                );
                resource_jobs::start(&mut world, worker, name, target_units, now)?;
                message = "Resource gathering job started";
            }
            Request::CancelResourceJob { pilot } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                let worker = pilot.as_deref().unwrap_or(name);
                resource_jobs::authorize(&world, name, worker)?;
                ensure!(
                    world
                        .resource_jobs
                        .get(worker)
                        .is_some_and(|j| !j.view.phase.is_terminal()),
                    "no active resource job"
                );
                resource_jobs::cancel(&mut world, worker, now, "Cancelled by controller or pilot");
                message = "Resource gathering job cancelled";
            }
            Request::SetFleetController { controller } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                if let Some(controller) = controller {
                    ensure!(
                        controller != *name && world.pilots.contains_key(&controller),
                        "controller must be another existing pilot"
                    );
                    world.fleet_links.insert(name.clone(), controller);
                } else {
                    world.fleet_links.remove(name);
                }
                message = "Fleet controller permission updated";
            }
            request if surface::handles(&request) => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                surface::execute(&mut world, name, request)?;
                message = "Surface prop updated";
            }
            request if drones::handles(&request) => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                drones::execute(&mut world, name, request, now)?;
                message = "Drone command accepted";
            }
            Request::StartMining {
                module_id,
                target_id,
            } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                mining::start(&mut world, name, module_id, target_id, now)?;
                message = "Mining cycle started";
            }
            Request::StopMining { module_id } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                mining::stop(&mut world, name, module_id)?;
                message = "Mining module stopped";
            }
            Request::SetPioneerRoom { action } => {
                let name = session
                    .as_ref()
                    .context("connect before engineering commands")?;
                room_controls::execute(&mut world, name, action, now)?;
                message = "Pioneer room command accepted";
            }
            Request::BoardShip { id } => {
                let name = session.as_ref().context("connect before boarding")?;
                event = assets::board(&mut world, name, &id, now)?;
                message = "Vessel boarded";
            }
            request if assets::handles(&request) => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                ensure!(
                    !matches!(request, Request::SelectShip { .. })
                        || !world.accounts[name].fleet_hangar,
                    "walk to the vessel boarding marker and use BoardShip"
                );
                let outcome = assets::execute(&mut world, name, request, now)?;
                event = outcome;
                message = "Station assets updated";
            }
            Request::AccountConnect { .. } | Request::AuthCapabilities => {
                bail!("use authenticated transport admission")
            }
            Request::Connect {
                supports_room_controls,
                supports_fleet_hangar,
                supports_pioneer_interior,
                protocol_version,
                pilot,
                resume_token,
            } => {
                ensure!(
                    protocol_version == PROTOCOL_VERSION,
                    "unsupported protocol version"
                );
                ensure!(
                    !pilot.is_empty()
                        && pilot.len() <= 24
                        && pilot
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                    "pilot name must be 1-24 ASCII letters, digits, underscore or hyphen"
                );
                if let Some(a) = &account {
                    identity::authorize(&world, &pilot, a, resume_token.as_deref())?;
                }
                if let Some(existing) = world.pilots.get(&pilot) {
                    if account.is_none() {
                        let supplied =
                            resume_token.context("resume token required for existing pilot")?;
                        ensure!(
                            token_hash(&supplied) == existing.token_hash,
                            "invalid resume token"
                        );
                    }
                    if !supports_pioneer_interior {
                        let existing = world.pilots.get_mut(&pilot).unwrap();
                        if existing
                            .view
                            .station_avatar
                            .as_ref()
                            .is_some_and(|a| a.onboard_interior)
                        {
                            let point = if supports_fleet_hangar {
                                fleet_berth(existing.view.ship_class).1
                            } else {
                                hangar_layout(existing.view.ship_class).boarding_point
                            };
                            let avatar = existing.view.station_avatar.as_mut().unwrap();
                            avatar.onboard_interior = false;
                            avatar.interior_deck = 1;
                            avatar.position = point;
                            avatar.heading = 0.0;
                            avatar.seated = false;
                            avatar.updated_at_ms = now;
                            station::stop(existing);
                        }
                    }
                } else {
                    ensure!(
                        resume_token.is_none(),
                        "unknown pilot; omit resume token to create"
                    );
                    let mut bytes = [0u8; 32];
                    OsRng
                        .try_fill_bytes(&mut bytes)
                        .context("secure random generator unavailable")?;
                    let issued = hex::encode(bytes);
                    world.pilots.insert(
                        pilot.clone(),
                        Pilot {
                            view: PilotView {
                                station_avatar: None,
                                surface: None,
                                power: None,
                                drones: Vec::new(),
                                target_locks: Vec::new(),
                                locked_asteroids: Vec::new(),
                                drone: None,
                                mining_modules: Vec::new(),
                                target_lock: None,
                                ship_class: ShipClass::default(),
                                name: pilot.clone(),
                                credits: STARTER_CREDITS,
                                ore: 0,
                                location: Location::Docked,
                                flight: None,
                                system_id: HOME_SYSTEM_ID,
                                locked_asteroid: None,
                            },
                            token_hash: token_hash(&issued),
                            identity: None,
                            last_mine_ms: None,
                            warp: None,
                            controls: None,
                            walk_input: None,
                        },
                    );
                    if account.is_none() {
                        token = Some(issued);
                    }
                    event = Some(EconomicEvent::PilotCreated {
                        pilot: pilot.clone(),
                    });
                }
                if let Some(a) = &account {
                    let p = world.pilots.get_mut(&pilot).unwrap();
                    if p.identity.is_none() {
                        p.identity = Some(a.binding.clone());
                        tx.execute("INSERT INTO identity_audit(pilot, issuer, subject, linked_at_ms) VALUES(?1,?2,?3,?4)", params![pilot, a.binding.issuer, a.binding.subject, now])?;
                    }
                }
                assets::ensure_accounts(&mut world);
                world.accounts.get_mut(&pilot).unwrap().room_compatible = supports_room_controls;
                if !supports_room_controls {
                    room_controls::legacy_recovery(&mut world, &pilot, now);
                }
                let old_fleet = world.accounts[&pilot].fleet_hangar;
                world.accounts.get_mut(&pilot).unwrap().fleet_hangar = supports_fleet_hangar;
                if old_fleet != supports_fleet_hangar {
                    let p = world.pilots.get_mut(&pilot).unwrap();
                    if let Some(a) = &mut p.view.station_avatar {
                        if !a.onboard_interior {
                            a.position = if supports_fleet_hangar {
                                fleet_berth(p.view.ship_class).1
                            } else {
                                hangar_layout(p.view.ship_class).boarding_point
                            };
                            a.seated = false;
                            a.jump_started_ms = None;
                        }
                    }
                }
                remote_control::release_worker(&mut world, &pilot);
                station::stop(world.pilots.get_mut(&pilot).unwrap());
                resource_jobs::cancel(
                    &mut world,
                    &pilot,
                    now,
                    "Pilot reconnected and took control",
                );
                connected = Some(pilot);
                message = "Connected";
            }
            Request::Snapshot | Request::ExportLedger => {
                message = "Snapshot";
            }
            request => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?
                    .clone();
                if let Request::Navigate { mode } = &request {
                    flight::validate_mode(mode, &name, &world)?;
                }
                if let Request::Warp {
                    destination: Destination::Gate { id },
                } = &request
                {
                    let gate = universe::gate(*id)?;
                    ensure!(
                        gate.system_id == world.pilots[&name].view.system_id,
                        "gate is in another system"
                    );
                }
                let pilot = world.pilots.get_mut(&name).context("pilot missing")?;
                flight::ensure_flight(pilot, now);
                match request {
                    Request::PilotInput {
                        translation,
                        rotation,
                    } => {
                        ensure!(
                            universe::normal(pilot),
                            "manual piloting requires undocked normal space"
                        );
                        flight::validate_input(translation, rotation)?;
                        pilot.controls = Some(flight::controls(translation, rotation, now));
                        let f = pilot.view.flight.as_mut().unwrap();
                        f.mode = NavigationMode::Manual;
                        f.updated_at_ms = now;
                        f.angular_velocity = pilot
                            .controls
                            .as_ref()
                            .unwrap()
                            .rotation
                            .map(|v| v * MAX_ANGULAR_SPEED);
                        message = "Piloting input accepted";
                    }
                    Request::UnlockTarget => {
                        universe::clear_locks(pilot);
                        message = "All targets unlocked";
                    }
                    Request::UnlockAsteroid { id } => {
                        pilot
                            .view
                            .target_locks
                            .retain(|lock| lock.asteroid_id != id);
                        pilot.view.locked_asteroids.retain(|locked| *locked != id);
                        universe::sync_lock_mirrors(pilot);
                        message = "Target unlocked";
                    }
                    Request::LockAsteroid { id } => {
                        ensure!(universe::normal(pilot), "target lock requires normal space");
                        let asteroid = world
                            .asteroids
                            .iter()
                            .find(|a| a.id == id)
                            .context("asteroid not found")?;
                        ensure!(
                            asteroid.system_id == pilot.view.system_id,
                            "asteroid is in another system"
                        );
                        ensure!(asteroid.ore > 0, "asteroid depleted");
                        ensure!(
                            universe::surface(pilot, asteroid) <= LOCK_RANGE,
                            "asteroid outside lock range"
                        );
                        if !pilot.view.locked_asteroids.contains(&id)
                            && !pilot
                                .view
                                .target_locks
                                .iter()
                                .any(|lock| lock.asteroid_id == id)
                        {
                            ensure!(
                                pilot.view.locked_asteroids.len() + pilot.view.target_locks.len()
                                    < MAX_TARGET_LOCKS,
                                "maximum five target locks"
                            );
                            pilot.view.target_locks.push(TargetLock {
                                asteroid_id: id,
                                started_at_ms: now,
                                completes_at_ms: now
                                    .checked_add(lock_duration_ms(
                                        pilot.view.ship_class,
                                        asteroid.radius,
                                    ))
                                    .context("clock overflow")?,
                            });
                            universe::sync_lock_mirrors(pilot);
                        }
                        message = "Target acquisition accepted";
                    }
                    Request::Jump { gate_id } => {
                        ensure!(universe::normal(pilot), "jump requires normal space");
                        let gate = universe::gate(gate_id)?;
                        ensure!(
                            gate.system_id == pilot.view.system_id,
                            "gate is in another system"
                        );
                        let f = pilot.view.flight.as_mut().unwrap();
                        ensure!(
                            flight::distance(f.position, gate.position) <= GATE_RANGE,
                            "outside gate activation range"
                        );
                        f.mode = NavigationMode::Stop;
                        f.velocity = [0.0; 3];
                        f.acceleration = [0.0; 3];
                        f.updated_at_ms = now;
                        pilot.controls = None;
                        f.angular_velocity = [0.0; 3];
                        universe::clear_locks(pilot);
                        pilot.view.location = Location::Jumping {
                            gate_id,
                            arrives_at_ms: now
                                .checked_add(JUMP_DURATION_MS)
                                .context("clock overflow")?,
                        };
                        message = "Gate jump charging";
                    }
                    Request::Navigate { mode } => {
                        ensure!(
                            !matches!(
                                pilot.view.location,
                                Location::Docked
                                    | Location::InTransit { .. }
                                    | Location::Jumping { .. }
                            ),
                            "navigation requires undocked normal space"
                        );
                        let f = pilot.view.flight.as_mut().unwrap();
                        pilot.controls = None;
                        f.angular_velocity = [0.0; 3];
                        f.mode = mode;
                        f.updated_at_ms = now;
                        message = "Navigation accepted";
                    }
                    Request::Undock => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        pilot.view.location = Location::Station;
                        let f = pilot.view.flight.as_mut().unwrap();
                        f.position = flight::berth(
                            &pilot.view.name,
                            Destination::Station,
                            pilot.view.ship_class,
                        );
                        f.velocity = [0.0; 3];
                        f.acceleration = [0.0; 3];
                        f.mode = NavigationMode::Stop;
                        f.updated_at_ms = now;
                        message = "Undocked";
                    }
                    Request::Warp { destination } => {
                        ensure!(
                            matches!(pilot.view.location, Location::Station | Location::Belt),
                            "must be in space and not in transit"
                        );
                        ensure!(
                            flight::distance(
                                pilot.view.flight.as_ref().unwrap().position,
                                flight::berth(&pilot.view.name, destination, pilot.view.ship_class)
                            ) > 100.0,
                            "already at destination"
                        );
                        ensure!(
                            power::spend(
                                &mut pilot.view.power,
                                power_spec(pilot.view.ship_class).warp_cost_j,
                                now
                            ),
                            "insufficient capacitor energy for warp reservation"
                        );
                        universe::clear_locks(pilot);
                        flight::begin_warp(pilot, destination, now);
                        pilot.view.location = Location::InTransit {
                            destination,
                            arrives_at_ms: pilot
                                .view
                                .flight
                                .as_ref()
                                .unwrap()
                                .warp
                                .as_ref()
                                .unwrap()
                                .arrives_at_ms,
                        };
                        message = "Warp initiated";
                    }
                    Request::Dock => {
                        ensure!(
                            !matches!(
                                pilot.view.location,
                                Location::Docked
                                    | Location::InTransit { .. }
                                    | Location::Jumping { .. }
                            ) && hull_surface_distance(
                                pilot.view.ship_class,
                                pilot.view.flight.as_ref().unwrap().position,
                                STATION_POSITION
                            ) <= DOCK_RANGE,
                            "must be within docking range in normal space"
                        );
                        universe::clear_locks(pilot);
                        pilot.controls = None;
                        pilot.view.location = Location::Docked;
                        let f = pilot.view.flight.as_mut().unwrap();
                        f.position = STATION_POSITION;
                        f.velocity = [0.0; 3];
                        f.acceleration = [0.0; 3];
                        f.mode = NavigationMode::Stop;
                        f.updated_at_ms = now;
                        f.angular_velocity = [0.0; 3];
                        message = "Docked";
                    }
                    Request::Mine { .. } | Request::MineAsteroid { .. } => {
                        bail!("manual mining quantities are disabled; use StartMining with a confirmed target")
                    }
                    Request::PlaceSell { units, unit_price } => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        ensure!(
                            units > 0 && unit_price > 0,
                            "quantity and price must be positive"
                        );
                        units.checked_mul(unit_price).context("price overflow")?;
                        ensure!(pilot.view.ore >= units, "insufficient ore");
                        pilot.view.ore -= units;
                        let id = world.next_order;
                        world.next_order = id.checked_add(1).context("order id overflow")?;
                        world.orders.insert(
                            id,
                            SellOrder {
                                id,
                                seller: name.clone(),
                                units,
                                unit_price,
                            },
                        );
                        event = Some(EconomicEvent::SellPlaced {
                            order_id: id,
                            seller: name,
                            units,
                            unit_price,
                        });
                        message = "Sell order placed";
                    }
                    Request::Buy { order_id, units } => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        let order = world
                            .orders
                            .get(&order_id)
                            .context("order not found")?
                            .clone();
                        ensure!(order.seller != name, "cannot buy own order");
                        ensure!(units > 0 && units <= order.units, "invalid buy quantity");
                        let price = units
                            .checked_mul(order.unit_price)
                            .context("price overflow")?;
                        ensure!(pilot.view.credits >= price, "insufficient credits");
                        pilot.view.ore = add_cargo(
                            pilot.view.ore,
                            units,
                            cargo_capacity(pilot.view.ship_class),
                        )?;
                        pilot.view.credits -= price;
                        let seller = world
                            .pilots
                            .get_mut(&order.seller)
                            .context("seller missing")?;
                        seller.view.credits = seller
                            .view
                            .credits
                            .checked_add(price)
                            .context("balance overflow")?;
                        if order.units == units {
                            world.orders.remove(&order_id);
                        } else {
                            world.orders.get_mut(&order_id).unwrap().units -= units;
                        }
                        event = Some(EconomicEvent::TradeExecuted {
                            order_id,
                            buyer: name,
                            units,
                        });
                        message = "Trade committed";
                    }
                    Request::CancelSell { order_id } => {
                        ensure!(pilot.view.location == Location::Docked, "must be docked");
                        let order = world.orders.get(&order_id).context("order not found")?;
                        ensure!(order.seller == name, "only seller can cancel");
                        pilot.view.ore = add_cargo(
                            pilot.view.ore,
                            order.units,
                            cargo_capacity(pilot.view.ship_class),
                        )?;
                        world.orders.remove(&order_id);
                        event = Some(EconomicEvent::SellCancelled {
                            order_id,
                            seller: name,
                        });
                        message = "Sell order cancelled";
                    }
                    _ => unreachable!(),
                }
            }
        }
        if automated {
            if let Some(EconomicEvent::PersonalOreDeposited { pilot, units, .. }) = &event {
                resource_jobs::record_delivery(&mut world, pilot, *units, now)?;
            }
        }
        for pilot in world.pilots.values_mut() {
            flight::ensure_flight(pilot, now);
        }
        surface::update(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        validate(&world)?;
        let mut entries = load_entries(&tx, export)?;
        for event in cycle_events.into_iter().chain(event) {
            append_event(&tx, &mut entries, event, now)?;
        }
        // Snapshot polling must not rewrite and fsync the entire world unless
        // it resolves a due arrival. Economic updates still commit with their
        // ledger entry in this same transaction.
        universe::advance_locks(&mut world, now);
        mining::invalidate(&mut world);
        drones::invalidate(&mut world);
        let updated_json = serde_json::to_string(&world)?;
        if updated_json != json {
            tx.execute("UPDATE world SET json=?1 WHERE id=1", [updated_json])?;
        }
        let private_assets = connected
            .as_ref()
            .or(transport_session.as_ref())
            .and_then(|name| assets::view(&world, name));
        let snapshot = Snapshot {
            remote_controls: world
                .remote_controls
                .iter()
                .map(|(controller, pilot)| RemoteControlLease {
                    controller: controller.clone(),
                    pilot: pilot.clone(),
                })
                .collect(),
            fleet_links: world
                .fleet_links
                .iter()
                .map(|(pilot, controller)| FleetLink {
                    pilot: pilot.clone(),
                    controller: controller.clone(),
                })
                .collect(),
            resource_jobs: world
                .resource_jobs
                .values()
                .map(|j| j.view.clone())
                .collect(),
            surface_props: world.surface_props.clone(),
            traffic: traffic::actors(now),
            stored_ore: assets::stored_ore(&world)
                .try_into()
                .context("stored ore overflow")?,
            server_time_ms: now,
            online_pilots: self.online_sessions.keys().cloned().collect(),
            asteroids: world.asteroids.clone(),
            pilots: world.pilots.values().map(|p| p.view.clone()).collect(),
            orders: world.orders.values().cloned().collect(),
            belt_ore: world.belt_ore,
            ledger_sequence: entries.last().map_or(0, |e| e.sequence),
            ledger_hash: entries
                .last()
                .map_or(GENESIS_HASH, |e| e.hash.as_str())
                .into(),
        };
        tx.commit()?;
        if let Some(pilot) = connected {
            *transport_session = Some(pilot);
        }
        Ok(Response {
            auth: None,
            assets: private_assets,
            ok: true,
            message: message.into(),
            resume_token: token,
            snapshot: Some(snapshot.clone()),
            ledger: if export {
                Some(LedgerExport {
                    protocol_version: PROTOCOL_VERSION,
                    entries,
                    snapshot,
                })
            } else {
                None
            },
        })
    }
}
fn load_entries(tx: &rusqlite::Transaction<'_>, export: bool) -> Result<Vec<LedgerEntry>> {
    let query = if export {
        "SELECT json FROM ledger ORDER BY sequence"
    } else {
        "SELECT json FROM ledger ORDER BY sequence DESC LIMIT 1"
    };
    let mut statement = tx.prepare(query)?;
    let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str::<LedgerEntry>(&row?)?))
        .collect()
}
fn append_event(
    tx: &rusqlite::Transaction<'_>,
    entries: &mut Vec<LedgerEntry>,
    event: EconomicEvent,
    now: u64,
) -> Result<()> {
    let last = entries.last();
    let mut entry = LedgerEntry {
        sequence: last
            .map_or(0, |e| e.sequence)
            .checked_add(1)
            .context("ledger sequence overflow")?,
        ruleset_version: RULESET_VERSION,
        timestamp_ms: now.max(last.map_or(0, |e| e.timestamp_ms)),
        previous_hash: last.map_or(GENESIS_HASH, |e| e.hash.as_str()).into(),
        event,
        hash: String::new(),
    };
    entry.hash = entry.calculate_hash();
    tx.execute(
        "INSERT INTO ledger(sequence,json) VALUES(?1,?2)",
        params![
            i64::try_from(entry.sequence)?,
            serde_json::to_string(&entry)?
        ],
    )?;
    entries.push(entry);
    Ok(())
}
fn add_cargo(current: u64, units: u64, capacity: u64) -> Result<u64> {
    let next = current.checked_add(units).context("cargo overflow")?;
    ensure!(next <= capacity, "cargo capacity exceeded");
    Ok(next)
}
fn validate(world: &World) -> Result<()> {
    station::validate(world)?;
    remote_control::validate(world)?;
    resource_jobs::validate(world)?;
    surface::validate(world)?;
    ensure!(
        world
            .pilots
            .values()
            .all(|p| p.view.ore <= cargo_capacity(p.view.ship_class)),
        "active cargo exceeds hull capacity"
    );
    assets::validate(world)?;
    room_controls::validate(world)?;
    mining::validate(world)?;
    universe::validate_locks(world)?;
    drones::validate(world)?;
    ensure!(
        world.asteroids.iter().map(|a| a.ore as u128).sum::<u128>() == world.belt_ore as u128,
        "asteroid reserve mismatch"
    );
    let credits: u128 = world.pilots.values().map(|p| p.view.credits as u128).sum();
    let ore = world.belt_ore as u128
        + world
            .pilots
            .values()
            .map(|p| p.view.ore as u128)
            .sum::<u128>()
        + world.orders.values().map(|o| o.units as u128).sum::<u128>()
        + assets::stored_ore(world);
    if credits != world.pilots.len() as u128 * STARTER_CREDITS as u128
        || ore != INITIAL_BELT_ORE as u128
    {
        bail!("economic conservation invariant failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polling_writes_only_when_a_persisted_arrival_changes_state() {
        let mut server = Server::open(":memory:").unwrap();
        let mut session = None;
        let connected = server.handle_at(
            &mut session,
            Request::Connect {
                supports_room_controls: false,
                supports_fleet_hangar: false,
                supports_pioneer_interior: true,
                protocol_version: PROTOCOL_VERSION,
                pilot: "Traveler".into(),
                resume_token: None,
            },
            100,
        );
        assert!(connected.ok);
        assert!(server.handle_at(&mut session, Request::Undock, 101).ok);
        let before = server.db.total_changes();
        for time in [101, 101, 101] {
            assert!(server.handle_at(&mut None, Request::Snapshot, time).ok);
            assert!(server.handle_at(&mut None, Request::ExportLedger, time).ok);
        }
        assert_eq!(
            server.db.total_changes(),
            before,
            "polling unchanged state must not write"
        );
        assert!(
            server
                .handle_at(
                    &mut session,
                    Request::Warp {
                        destination: Destination::Belt
                    },
                    102
                )
                .ok
        );
        let arrived = server.handle_at(
            &mut None,
            Request::Snapshot,
            plan_warp(STATION_POSITION, BELT_POSITION, 0).arrives_at_ms + 102,
        );
        assert_eq!(arrived.snapshot.unwrap().pilots[0].location, Location::Belt);
        assert_eq!(
            server.db.total_changes(),
            before + 2,
            "warp command and arrival each persist once"
        );
        assert!(
            server
                .handle_at(
                    &mut None,
                    Request::Snapshot,
                    plan_warp(STATION_POSITION, BELT_POSITION, 0).arrives_at_ms + 102
                )
                .ok
        );
        assert_eq!(server.db.total_changes(), before + 2);
    }
}

#[cfg(all(test, unix))]
mod ownership_release_tests {
    use super::*;

    #[test]
    fn server_drop_releases_authority_with_inherited_descriptor_still_open() {
        let root = std::env::temp_dir().join(format!(
            "arkline-inherited-lock-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("world.sqlite");
        let server = Server::open(&path).unwrap();
        // dup has the same open-file-description lifetime as a fork-inherited FD.
        // A concurrently spawning process can retain it briefly until exec.
        let inherited = server
            ._ownership_lock
            .as_ref()
            .unwrap()
            .0
            .try_clone()
            .unwrap();
        assert!(
            Server::open(&path).is_err(),
            "live writer must remain exclusive"
        );
        drop(server);
        let reopened = Server::open(&path)
            .expect("dropped server must release authority before an inherited FD closes");
        drop(inherited);
        assert!(
            Server::open(&path).is_err(),
            "closing the old descriptor must not release the new writer's lock"
        );
        drop(reopened);
        assert!(Server::open(&path).is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }
}
