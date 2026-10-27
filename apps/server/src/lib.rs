pub mod access_tokens;
mod assets;
mod cargo_unloading;
mod cockpit_mechanisms;
pub mod collaborative_editor;
pub mod collaborative_transport;
mod docked_traversal;
#[cfg(test)]
mod docked_traversal_tests;
mod docking;
mod drone_bay;
mod drone_cargo;
mod drones;
pub mod editor;
mod exterior_effects;
mod flight;
mod identity;
mod turret_aim;
mod turret_aim_collision;
mod turret_stow;
// Source-bound finite carrier inventory; normal geometry admission remains gated.
mod carrier_inventory;
pub mod independent_bays;
pub mod local_development;
mod mining;
pub mod physical_cargo;
mod power;
mod reactor_configuration;
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
    /// Fresh, explicit accounting inspection only; absent legacy worlds stay absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    physical_cargo: Option<physical_cargo::PhysicalCargoState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carrier_inventory_registration: Option<CarrierInventoryRegistration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    carrier_inventory_loadout: Option<Vec<DroneClass>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    independent_bay_registration: Option<IndependentBayRegistration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drone_cargo_registration: Option<DroneCargoRegistration>,
    /// Only INSERT of a fresh opted-in realm sets this; absent old saves stay absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drone_bay_registration: Option<DroneBayRegistration>,
    /// Fresh-only durable batch capability; absent worlds are never upgraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drone_bay_batch_version: Option<u32>,
    // Old JSON deliberately retains the exact original anchors. This is a
    // creation choice for new realms, never an automatic coordinate migration.
    #[serde(default, skip_serializing_if = "SceneLayoutRevision::is_legacy")]
    scene_layout: SceneLayoutRevision,
    #[serde(default)]
    exterior_definition: ExteriorDefinitionState,
    #[serde(default)]
    remote_controls: BTreeMap<String, String>,
    #[serde(default)]
    fleet_links: BTreeMap<String, String>,
    #[serde(default)]
    resource_jobs: BTreeMap<String, resource_jobs::Job>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    cargo_unloading: BTreeMap<String, CargoUnloadingState>,
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
            physical_cargo: None,
            carrier_inventory_registration: None,
            carrier_inventory_loadout: None,
            independent_bay_registration: None,
            drone_cargo_registration: None,
            drone_bay_registration: None,
            drone_bay_batch_version: None,
            scene_layout: SceneLayoutRevision::LegacyV1,
            exterior_definition: ExteriorDefinitionState::default(),
            remote_controls: BTreeMap::new(),
            fleet_links: BTreeMap::new(),
            resource_jobs: BTreeMap::new(),
            cargo_unloading: BTreeMap::new(),
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
impl World {
    fn planet(&self) -> PlanetDefinition {
        self.scene_layout.definition().planets.remove(0)
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
    physical_controlled: bool,
    local_development: bool,
    editor_room_layout: Option<RoomLayoutDocument>,
    editor_time: Option<u64>,
    db: Connection,
    auth: Option<identity::Authenticator>,
    online_sessions: BTreeMap<String, usize>,
    // Conservative latch: clear only after ALL sessions for this pilot leave.
    interior_incompatible: std::collections::BTreeSet<String>,
    docked_incompatible: std::collections::BTreeSet<String>,
    fleet_incompatible: std::collections::BTreeSet<String>,
    room_incompatible: std::collections::BTreeSet<String>,
    // Kept after db so SQLite closes before authority is released. Never unlink
    // the sidecar: replacing its inode would allow competing locks.
    _ownership_lock: Option<OwnershipLock>,
}
impl Server {
    pub(crate) fn set_editor_room_layout(&mut self, document: RoomLayoutDocument) {
        self.editor_room_layout = Some(document);
    }

    pub(crate) fn validate_editor_room_layout(&self, document: &RoomLayoutDocument) -> Result<()> {
        document.validate_live().map_err(anyhow::Error::msg)?;
        let json: String = self
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |row| row.get(0))?;
        let world: World = serde_json::from_str(&json)?;
        station::validate_room_layout(&world, document)
    }

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
        Self::open_realm_with_layout(
            path,
            local_development,
            editor_mode,
            None,
            SceneLayoutRevision::LegacyV1,
            false,
        )
    }

    pub(crate) fn open_editor_realm_with_scene(
        path: impl AsRef<Path>,
        document: RoomLayoutDocument,
        creation_scene: SceneLayoutRevision,
    ) -> Result<Self> {
        document.validate_live().map_err(anyhow::Error::msg)?;
        Self::open_realm_with_layout(path, true, true, Some(document), creation_scene, false)
    }

    pub(crate) fn open_editor_ore_receiver_realm(
        path: impl AsRef<Path>,
        document: RoomLayoutDocument,
    ) -> Result<Self> {
        document.validate_live().map_err(anyhow::Error::msg)?;
        Self::open_realm_with_layout(
            path,
            true,
            true,
            Some(document),
            SceneLayoutRevision::DockingV1,
            true,
        )
    }
    fn open_realm_with_layout(
        path: impl AsRef<Path>,
        local_development: bool,
        editor_mode: bool,
        editor_room_layout: Option<RoomLayoutDocument>,
        creation_scene: SceneLayoutRevision,
        ore_receiver: bool,
    ) -> Result<Self> {
        Self::open_realm_with_configuration(
            path,
            local_development,
            editor_mode,
            editor_room_layout,
            creation_scene,
            ore_receiver,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn open_realm_with_configuration(
        path: impl AsRef<Path>,
        local_development: bool,
        editor_mode: bool,
        editor_room_layout: Option<RoomLayoutDocument>,
        creation_scene: SceneLayoutRevision,
        ore_receiver: bool,
        physical: Option<(physical_cargo::PhysicalCargoGenesis, u64, bool)>,
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
        if let Some((genesis, now, controlled)) = physical {
            return physical_cargo::open_server(db, ownership_lock, genesis, now, controlled);
        }
        physical_cargo::refuse_existing(&db)?;
        independent_bays::world::preflight_existing(&db)?;
        carrier_inventory::world::preflight_existing(&db)?;
        let realm_scene = local_development::establish_realm(
            &mut db,
            local_development,
            existed,
            creation_scene,
        )?;
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
        {
            // Create the feature stamp and its downgrade guard atomically. This
            // installs no guard and rewrites no JSON in existing unstamped worlds.
            let creation =
                db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            let registration = drone_bay::creation_registration(realm_scene);
            creation.execute(
                "INSERT OR IGNORE INTO world(id,json) VALUES(1,?1)",
                [serde_json::to_string(&World {
                    drone_cargo_registration: if ore_receiver {
                        Some(drone_cargo::registration()?)
                    } else {
                        None
                    },
                    drone_bay_batch_version: registration.as_ref().map(|_| DRONE_BAY_BATCH_VERSION),
                    drone_bay_registration: registration,
                    scene_layout: realm_scene,
                    ..World::default()
                })?],
            )?;
            let batch_version: Option<u32> = creation.query_row(
                "SELECT json_extract(json,'$.drone_bay_batch_version') FROM world WHERE id=1",
                [],
                |row| row.get(0),
            )?;
            if batch_version == Some(DRONE_BAY_BATCH_VERSION) {
                creation.execute_batch("CREATE TRIGGER IF NOT EXISTS preserve_drone_bay_batch_v1
                    BEFORE UPDATE OF json ON world
                    WHEN json_extract(OLD.json,'$.drone_bay_batch_version')=1
                    AND json_extract(NEW.json,'$.drone_bay_batch_version') IS NOT 1
                    BEGIN SELECT RAISE(ABORT,'physical bay batch version cannot be discarded by an older world writer'); END;")?;
            }
            let cargo_stamp: Option<String> = creation.query_row(
                "SELECT json_extract(json,'$.drone_cargo_registration') FROM world WHERE id=1",
                [],
                |row| row.get(0),
            )?;
            ensure!(!ore_receiver || cargo_stamp.is_some(),
                "Existing world has no ore receiver stamp; use a fresh isolated ore receiver preview");
            if cargo_stamp.is_some() {
                creation.execute_batch("CREATE TRIGGER IF NOT EXISTS preserve_drone_cargo_v1
                    BEFORE UPDATE OF json ON world
                    WHEN json_extract(OLD.json,'$.drone_cargo_registration') IS NOT NULL
                    AND json_extract(NEW.json,'$.drone_cargo_registration') IS NOT json_extract(OLD.json,'$.drone_cargo_registration')
                    BEGIN SELECT RAISE(ABORT,'drone cargo registration cannot be discarded or rebound by an older writer'); END;")?;
            }
            let independent_stamp: Option<String> = creation.query_row(
                "SELECT json_extract(json,'$.independent_bay_registration') FROM world WHERE id=1",
                [],
                |row| row.get(0),
            )?;
            if independent_stamp.is_some() {
                creation.execute_batch("CREATE TRIGGER IF NOT EXISTS preserve_independent_bays_v1 BEFORE UPDATE OF json ON world WHEN json_extract(OLD.json,'$.independent_bay_registration') IS NOT NULL AND json_extract(NEW.json,'$.independent_bay_registration') IS NOT json_extract(OLD.json,'$.independent_bay_registration') BEGIN SELECT RAISE(ABORT,'independent bay source cannot be discarded or rebound'); END;")?;
            }
            let carrier_stamp: Option<String> = creation.query_row("SELECT json_extract(json,'$.carrier_inventory_registration') FROM world WHERE id=1", [], |r|r.get(0))?;
            if carrier_stamp.is_some() {
                creation.execute_batch("CREATE TRIGGER IF NOT EXISTS preserve_carrier_inventory_v1 BEFORE UPDATE OF json ON world WHEN json_extract(OLD.json,'$.carrier_inventory_registration') IS NOT NULL AND (json_extract(NEW.json,'$.carrier_inventory_registration') IS NOT json_extract(OLD.json,'$.carrier_inventory_registration') OR json_extract(NEW.json,'$.carrier_inventory_loadout') IS NOT json_extract(OLD.json,'$.carrier_inventory_loadout')) BEGIN SELECT RAISE(ABORT,'finite carrier source/loadout cannot be discarded or rebound'); END;")?;
            }
            creation.commit()?;
        }
        let json: String = db.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json)?;
        local_development::validate_realm_scene(&db, local_development, world.scene_layout)?;
        independent_bays::world::validate_history(&world, &db)?;
        carrier_inventory::world::validate_history(&world, &db)?;
        let editor_time = editor::open_clock(&mut db, editor_mode)?;
        let now = editor_time.unwrap_or_else(now_ms);
        flight::migrate_spatial(&mut world);
        universe::ensure_asteroids(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;
        drone_bay::validate(&world)?;
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        drone_cargo::validate(&world)?;
        independent_bays::world::validate(&world)?;
        carrier_inventory::world::validate(&world)?;
        turret_stow::validate(&world, now)?;
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        turret_stow::stop_on_restart(&mut world, now);
        world.remote_controls.clear();
        resource_jobs::cancel_all(&mut world, now, "Server restarted");
        for name in world.pilots.keys().cloned().collect::<Vec<_>>() {
            independent_bays::world::stop_all(
                &mut world,
                &name,
                now,
                "Server restarted; independent lanes held",
            )?;
            carrier_inventory::world::stop_all(
                &mut world,
                &name,
                now,
                "Server restarted; carrier inventory held",
            )?;
            cargo_unloading::hold(&mut world, &name, "Server restarted; unloading held");
            drone_bay::stop_all(&mut world, &name, now, "Server restarted; drone bay held");
            drone_cargo::stop_all(
                &mut world,
                &name,
                now,
                "Server restarted; ore receiver held",
            )?;
        }
        if let Some(room_layout) = editor_room_layout.as_ref() {
            // An editor layout must be compatible with persisted pilot
            // positions before migration. Do not silently relocate avatars
            // when an externally changed layout file would trap them.
            station::validate_room_layout(&world, room_layout)?;
        }
        docked_traversal::migrate_stowed_cockpit_v3(&mut world)?;
        for pilot in world.pilots.values_mut() {
            docking::hold(pilot, now, "Server restarted; docking held");
        }
        docked_traversal::stop_lifts(&mut world, now);
        cockpit_mechanisms::stop_on_restart(&mut world, now);
        reactor_configuration::stop_on_restart(&mut world, now, editor_room_layout.as_ref());
        station::migrate(&mut world, editor_room_layout.as_ref());
        for pilot in world.pilots.values_mut() {
            mining::stop_all(pilot, "Server restarted");
            pilot.controls = None;
            station::stop(pilot);
            pilot.view.target_locks.clear();
            universe::sync_lock_mirrors(pilot);
            if pilot.view.drone_bay.is_none()
                && world.independent_bay_registration.is_none()
                && world.carrier_inventory_registration.is_none()
            {
                drones::clear(pilot);
            }
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
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        validate(&world, editor_room_layout.as_ref())?;
        let stopped = serde_json::to_string(&world)?;
        if stopped != json {
            db.execute("UPDATE world SET json=?1 WHERE id=1", [stopped])?;
        }
        Ok(Self {
            physical_controlled: false,
            local_development,
            editor_room_layout,
            editor_time,
            db,
            auth: None,
            online_sessions: BTreeMap::new(),
            interior_incompatible: Default::default(),
            docked_incompatible: Default::default(),
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

        let legacy_docked = matches!(
            &request,
            Request::Connect {
                supports_docked_traversal_v2: false,
                ..
            }
        );
        if let Request::Connect {
            pilot,
            supports_docked_traversal_v2,
            ..
        } = &mut request
        {
            if self.docked_incompatible.contains(pilot) {
                *supports_docked_traversal_v2 = false;
            }
        }
        if matches!(&request, Request::DockedTraversal { .. })
            && session
                .as_ref()
                .is_some_and(|name| self.docked_incompatible.contains(name))
        {
            return Response::error("Close incompatible clients, then disconnect and reconnect all sessions before docked traversal");
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
                if legacy_docked {
                    if let Some(name) = session.as_ref() {
                        self.docked_incompatible.insert(name.clone());
                    }
                }
                if legacy_connect {
                    if let Some(name) = session.as_ref() {
                        self.interior_incompatible.insert(name.clone());
                    }
                }
                if session
                    .as_ref()
                    .is_some_and(|name| self.docked_incompatible.contains(name))
                {
                    docked_traversal::project_legacy_response(&mut response);
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
                self.docked_incompatible.remove(pilot);
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
        if self.physical_controlled {
            return physical_cargo::control::tick(self, now);
        }
        #[cfg(test)]
        tick_profile::start_tick();
        if let Some(previous) = self.editor_time {
            ensure!(now >= previous, "editor time cannot move backwards");
        }
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let json: String = tx.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        #[cfg(test)]
        tick_profile::mark("db_begin_read");
        let mut world: World = serde_json::from_str(&json)?;
        #[cfg(test)]
        tick_profile::mark("json_decode");
        flight::migrate_spatial(&mut world);
        universe::ensure_asteroids(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;
        drone_bay::validate(&world)?;
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        drone_cargo::validate(&world)?;
        independent_bays::world::validate(&world)?;
        #[cfg(test)]
        tick_profile::mark("migration_legacy_preflight");
        carrier_inventory::world::validate(&world)?;
        #[cfg(test)]
        tick_profile::mark("carrier_prevalidate");
        turret_stow::validate(&world, now)?;
        #[cfg(test)]
        tick_profile::mark("turret_prevalidate");
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        let bay_obstacle_origins = drone_bay::carrier_origins(&world, now);
        flight::advance(&mut world, now);
        cockpit_mechanisms::advance(&mut world, now)?;
        turret_stow::advance(&mut world, now);
        reactor_configuration::advance(&mut world, now, self.editor_room_layout.as_ref())?;
        docked_traversal::advance(&mut world, now, self.editor_room_layout.as_ref())?;
        station::advance(&mut world, now, self.editor_room_layout.as_ref());
        surface::update(&mut world);
        universe::advance_locks(&mut world, now);
        #[cfg(test)]
        tick_profile::mark("flight_room_surface_locks");
        let mut events = mining::advance(&mut world, now);
        resource_jobs::record_turret_extractions(&mut world, &events, now)?;
        drone_bay::advance(&mut world, now, &bay_obstacle_origins)?;
        independent_bays::world::advance(&mut world, now, None)?;
        #[cfg(test)]
        tick_profile::mark("mining_legacy_bays");
        carrier_inventory::world::advance(&mut world, now, None)?;
        #[cfg(test)]
        tick_profile::mark("carrier_advance");
        drone_cargo::advance(&mut world, now, &bay_obstacle_origins, None)?;
        events.extend(drones::advance(&mut world, now)?);
        carrier_inventory::world::dispatch(&mut world, now, None)?;
        #[cfg(test)]
        tick_profile::mark("drone_cargo_and_drones");
        universe::advance_locks(&mut world, now);
        turret_aim::advance(&mut world, now);
        let source_events = mining::advance_source(&mut world, now);
        resource_jobs::record_turret_extractions(&mut world, &source_events, now)?;
        events.extend(source_events);
        mining::invalidate(&mut world);
        cockpit_mechanisms::advance(&mut world, now)?;
        turret_stow::advance(&mut world, now);
        reactor_configuration::advance(&mut world, now, self.editor_room_layout.as_ref())?;
        #[cfg(test)]
        tick_profile::mark("source_mining_post_domains");
        let mut entries = load_entries(&tx, world.independent_bay_registration.is_some())?;
        for event in events {
            append_event(&tx, &mut entries, event, now, drone_cargo::ruleset(&world))?;
        }
        drone_cargo::commit_transfers(&mut world, &tx, &mut entries, now)?;
        independent_bays::world::commit_transfers(&mut world, &tx, &mut entries, now)?;
        carrier_inventory::world::service::commit_transfers(&mut world, &tx, &mut entries, now)?;
        cargo_unloading::advance(&mut world, &tx, &mut entries, now)?;
        #[cfg(test)]
        tick_profile::mark("ledger_transfers");
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        turret_stow::dispatch(&mut world, now);
        #[cfg(test)]
        tick_profile::mark("projections_dispatch");
        validate(&world, self.editor_room_layout.as_ref())?;
        #[cfg(test)]
        tick_profile::mark("full_postvalidate");
        let next = serde_json::to_string(&world)?;
        #[cfg(test)]
        tick_profile::mark("json_encode");
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
        #[cfg(test)]
        tick_profile::mark("db_write_commit");
        resource_jobs::advance(self, now, &world)?;
        #[cfg(test)]
        {
            tick_profile::mark("resource_jobs");
            tick_profile::finish_tick();
        }
        Ok(())
    }
    fn stop_disconnected(&mut self, name: &str) -> Result<()> {
        let tx = self
            .db
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let json: String = tx.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
        let mut world: World = serde_json::from_str(&json)?;
        flight::migrate_spatial(&mut world);
        universe::ensure_asteroids(&mut world);
        assets::ensure_accounts(&mut world);
        mining::ensure_modules(&mut world);
        drones::ensure_fleets(&mut world)?;
        drone_bay::validate(&world)?;
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        drone_cargo::validate(&world)?;
        independent_bays::world::validate(&world)?;
        carrier_inventory::world::validate(&world)?;

        let now = self.editor_time.unwrap_or_else(now_ms);
        turret_stow::validate(&world, now)?;
        independent_bays::world::stop_all(
            &mut world,
            name,
            now,
            "Source disconnected; independent bays held",
        )?;
        carrier_inventory::world::stop_all(
            &mut world,
            name,
            now,
            "Source disconnected; carrier inventory held",
        )?;
        drone_cargo::stop_all(
            &mut world,
            name,
            now,
            "Source pilot disconnected; ore service held",
        )?;
        if let Some(worker) = world.remote_controls.get(name).cloned() {
            drone_cargo::stop_all(
                &mut world,
                &worker,
                now,
                "Remote controller disconnected; ore service held",
            )?;
            drone_bay::hold(
                &mut world,
                &worker,
                now,
                "Remote controller disconnected; drone bay held",
            );
        }
        turret_stow::hold(
            &mut world,
            name,
            now,
            "Source disconnected; turrets held and warp canceled",
        );
        remote_control::release_controller(&mut world, name);
        let controlled: Vec<String> = world
            .resource_jobs
            .iter()
            .filter(|(_, j)| j.view.controller == name && !j.view.phase.is_terminal())
            .map(|(n, _)| n.clone())
            .collect();
        for worker in controlled {
            resource_jobs::cancel(&mut world, &worker, now, "Controller disconnected");
            cargo_unloading::hold(
                &mut world,
                &worker,
                "Controller disconnected; unloading held",
            );
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
        // Cargo authority always holds when its source pilot loses the last
        // session, even if an existing flight/job delegation remains online.
        independent_bays::world::stop_all(
            &mut world,
            name,
            now,
            "Source disconnected; independent lanes held",
        )?;
        carrier_inventory::world::stop_all(
            &mut world,
            name,
            now,
            "Source disconnected; carrier inventory held",
        )?;
        cargo_unloading::hold(&mut world, name, "Pilot disconnected; unloading held");
        drone_bay::hold(&mut world, name, now, "Pilot disconnected; drone bay held");
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        if delegated {
            validate(&world, self.editor_room_layout.as_ref())?;
            tx.execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&world)?],
            )?;
            tx.commit()?;
            return Ok(());
        }
        resource_jobs::cancel(&mut world, name, now, "Pilot disconnected");
        drone_bay::stop_all(&mut world, name, now, "Pilot disconnected; drone bay held");
        if let Some(p) = world.pilots.get_mut(name) {
            docking::hold(p, now, "Pilot disconnected; docking held");
            mining::stop_all(p, "Pilot disconnected");
            p.controls = None;
            station::stop(p);
            p.view.target_locks.clear();
            universe::sync_lock_mirrors(p);
            if p.view.drone_bay.is_none()
                && world.independent_bay_registration.is_none()
                && world.carrier_inventory_registration.is_none()
            {
                drones::clear(p);
            }
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
        validate(&world, self.editor_room_layout.as_ref())?;
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
        if self.physical_controlled {
            ensure!(
                !automated && account.is_none(),
                "Physical controls reject automated/account aliases"
            );
            return physical_cargo::control::execute(self, session, request, now);
        }
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
        drone_bay::validate(&world)?;
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        drone_cargo::validate(&world)?;
        independent_bays::world::validate(&world)?;
        carrier_inventory::world::validate(&world)?;
        turret_stow::validate(&world, now)?;
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        let bay_obstacle_origins = drone_bay::carrier_origins(&world, now);
        flight::advance(&mut world, now);
        cockpit_mechanisms::advance(&mut world, now)?;
        turret_stow::advance(&mut world, now);
        reactor_configuration::advance(&mut world, now, self.editor_room_layout.as_ref())?;
        docked_traversal::advance(&mut world, now, self.editor_room_layout.as_ref())?;
        station::advance(&mut world, now, self.editor_room_layout.as_ref());
        surface::update(&mut world);
        universe::advance_locks(&mut world, now);
        let mut token = None;
        let mut cycle_events = mining::advance(&mut world, now);
        resource_jobs::record_turret_extractions(&mut world, &cycle_events, now)?;
        let defer_batch = matches!(request, Request::DroneBay { .. })
            .then_some(session.as_deref())
            .flatten();
        drone_bay::advance_with_deferred_batch(
            &mut world,
            now,
            &bay_obstacle_origins,
            defer_batch,
        )?;
        drone_cargo::advance(
            &mut world,
            now,
            &bay_obstacle_origins,
            matches!(
                request,
                Request::DroneCargo { .. } | Request::DroneBay { .. }
            )
            .then_some(session.as_deref())
            .flatten(),
        )?;
        independent_bays::world::advance(
            &mut world,
            now,
            matches!(request, Request::IndependentBays { .. })
                .then_some(session.as_deref())
                .flatten(),
        )?;
        let defer_carrier = matches!(request, Request::CarrierInventory { .. })
            .then_some(session.as_deref())
            .flatten();
        carrier_inventory::world::advance(&mut world, now, defer_carrier)?;
        cycle_events.extend(drones::advance(&mut world, now)?);
        carrier_inventory::world::dispatch(&mut world, now, defer_carrier)?;
        universe::advance_locks(&mut world, now);
        turret_aim::advance(&mut world, now);
        let source_events = mining::advance_source(&mut world, now);
        resource_jobs::record_turret_extractions(&mut world, &source_events, now)?;
        cycle_events.extend(source_events);
        mining::invalidate(&mut world);
        let mut event = None;
        let mut connected = None;
        let export = matches!(request, Request::ExportLedger);
        let message;
        let cargo_intent_request = request.clone();
        let mut cargo_initialization = None;
        if let Some(name) = session.as_ref() {
            turret_stow::before_request(&mut world, name, &request, now);
            turret_stow::authorize(&world, name, &request, now)?;
            cargo_unloading::authorize(&world, name, &request)?;
            drone_bay::authorize(&world, name, &request)?;
            drone_cargo::authorize(&world, name, &request)?;
            independent_bays::world::authorize(&world, name, &request)?;
            carrier_inventory::world::authorize(&world, name, &request)?;
        }
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
                    | Request::DockedTraversal { .. }
                    | Request::SetCockpitScreenMode { .. }
                    | Request::StartCargoUnload { .. }
                    | Request::ResumeCargoUnload { .. }
                    | Request::CancelCargoUnload { .. }
                    | Request::DroneBay { .. }
                    | Request::DroneCargo { .. }
                    | Request::IndependentBays { .. }
                    | Request::CarrierInventory { .. }
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
            if !automated
                && world
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
        if docking::enabled(&world) {
            if let Some(name) = session.as_ref() {
                ensure!(
                    !matches!(
                        request,
                        Request::SelectShip { .. }
                            | Request::BoardShip { .. }
                            | Request::Jump { .. }
                            | Request::DisembarkShip
                            | Request::EnterShip
                            | Request::EnterInterior
                            | Request::ExitInterior
                            | Request::StationSit { .. }
                            | Request::StationWalk { .. }
                            | Request::StationJump
                            | Request::InteriorLift { .. }
                    ),
                    "docking preview currently supports the assigned home-system Pioneer only"
                );
                if docking::busy(&world.pilots[name]) {
                    ensure!(!request.is_remote_gameplay() || matches!(request, Request::Dock | Request::Undock),
                        "docking authority owns vessel motion; cancel or resume the current operation");
                }
            }
        }
        match request {
            Request::CarrierInventory { request } => {
                let name = session.as_deref().context("Connect first")?;
                carrier_inventory::world::execute(&mut world, name, &request, now)?;
                message = "Carrier inventory command confirmed";
            }
            Request::IndependentBays { request } => {
                let name = session
                    .as_ref()
                    .context("Connect before independent bay command")?;
                independent_bays::world::execute(&mut world, name, &request, now)?;
                message = "Independent bay command confirmed";
            }
            Request::DroneCargo { request } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing ore receiver commands")?;
                drone_cargo::execute(&mut world, name, &request, now, automated)?;
                message = "Ore receiver command confirmed";
            }
            Request::DroneBay { request } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing drone bay commands")?;
                drone_bay::execute(&mut world, name, &request, now, automated)?;
                message = "Physical drone bay command confirmed";
            }
            Request::StartCargoUnload {
                operation_id,
                units,
                destination,
            } => {
                ensure!(
                    !remote && !automated,
                    "unload identity must belong to the authenticated pilot"
                );
                let name = session.as_ref().context("connect before unloading")?;
                cargo_unloading::begin(
                    &mut world,
                    name,
                    operation_id,
                    units,
                    destination,
                    now,
                    false,
                )?;
                message = "Unload operation accepted; consult confirmed batch receipts";
            }
            Request::CancelCargoUnload { operation_id } => {
                ensure!(
                    !remote && !automated,
                    "unload cancellation must belong to the authenticated pilot"
                );
                let name = session.as_ref().context("connect before unloading")?;
                let linked_job = world
                    .cargo_unloading
                    .get(name)
                    .and_then(|s| {
                        s.operations
                            .iter()
                            .find(|op| op.operation_id == operation_id && op.phase.active())
                    })
                    .and_then(|op| op.job_id);
                cargo_unloading::cancel(&mut world, name, operation_id)?;
                if linked_job.is_some_and(|id| {
                    world
                        .resource_jobs
                        .get(name)
                        .is_some_and(|j| j.view.id == id && !j.view.phase.is_terminal())
                }) {
                    resource_jobs::cancel(&mut world, name, now, "Unload cancelled by pilot");
                }
                message = "Unload cancellation accepted; committed batches retained";
            }
            Request::ResumeCargoUnload { operation_id } => {
                ensure!(
                    !remote && !automated,
                    "unload resume must belong to the authenticated pilot"
                );
                let name = session.as_ref().context("connect before unloading")?;
                cargo_unloading::resume(&mut world, name, operation_id, now)?;
                message = "Unload resume accepted";
            }
            Request::DepositOre { units } if docking::enabled(&world) && automated => {
                let name = session.as_ref().context("delivery pilot missing")?;
                let id = world
                    .cargo_unloading
                    .get(name)
                    .map_or(0, |s| s.high_watermark)
                    .checked_add(1)
                    .context("unload operation IDs exhausted")?;
                cargo_unloading::begin(
                    &mut world,
                    name,
                    id,
                    units,
                    CargoUnloadDestination::Personal,
                    now,
                    true,
                )?;
                message = "Timed job unloading accepted";
            }
            Request::DepositOre { .. } | Request::DepositCorporationOre { .. }
                if docking::enabled(&world) =>
            {
                bail!(
                    "DockingV1 unloading requires StartCargoUnload with a confirmed operation ID"
                );
            }
            Request::Dock | Request::Undock if docking::enabled(&world) => {
                let name = session.as_ref().context("connect before docking")?;
                docking::begin(&mut world, name, matches!(request, Request::Undock), now)?;
                message = "Continuous docking operation accepted";
            }
            Request::CancelDocking { operation_id } => {
                let name = session.as_ref().context("connect before docking")?;
                docking::cancel(&mut world, name, operation_id, now)?;
                message = "Docking stop accepted";
            }
            Request::ResumeDocking { operation_id } => {
                let name = session.as_ref().context("connect before docking")?;
                docking::resume(&mut world, name, operation_id, now)?;
                message = "Docking resume accepted";
            }
            Request::DockedTraversal { request } => {
                let name = session
                    .as_ref()
                    .context("connect before docked traversal")?;
                ensure!(
                    !remote && !automated,
                    "docked traversal cannot be delegated"
                );
                docked_traversal::execute(
                    &mut world,
                    name,
                    request,
                    now,
                    self.editor_room_layout.as_ref(),
                )?;
                message = "Docked traversal action accepted";
            }
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
                if docking::busy(&world.pilots[worker]) {
                    let id = docking::state(&world.pilots[worker]).unwrap().operation_id;
                    docking::cancel(&mut world, worker, id, now)?;
                }
                resource_jobs::cancel(&mut world, worker, now, "Cancelled by controller or pilot");
                cargo_unloading::cancel_job(&mut world, worker);
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
                message = if turret_stow::enabled(&world, name) {
                    "Turret aim accepted; waiting for confirmed alignment"
                } else {
                    "Mining cycle started"
                };
            }
            Request::StopMining { module_id } => {
                let name = session
                    .as_ref()
                    .context("connect before issuing commands")?;
                mining::stop(&mut world, name, module_id)?;
                message = "Mining module stopped";
            }
            Request::SetTurretDeployment {
                vessel_id,
                deployed,
            } => {
                let name = session.as_ref().context("connect before turret controls")?;
                turret_stow::set_deployment(&mut world, name, &vessel_id, deployed, now)?;
                message = "Physical turret command accepted";
            }
            Request::Warp { destination }
                if session
                    .as_ref()
                    .is_some_and(|name| turret_stow::enabled(&world, name)) =>
            {
                message = turret_stow::request_warp(
                    &mut world,
                    session.as_ref().unwrap(),
                    destination,
                    now,
                )?;
            }
            Request::SetCockpitScreenMode {
                vessel_id,
                target,
                mode,
            } => {
                let name = session
                    .as_ref()
                    .context("connect before captain monitor controls")?;
                ensure!(
                    !remote && !automated,
                    "monitor controls cannot be delegated or automated"
                );
                cockpit_mechanisms::set_mode(&mut world, name, &vessel_id, target, mode, now)?;
                message = "Captain monitor preference accepted";
            }
            Request::SetPioneerRoom { action } => {
                let name = session
                    .as_ref()
                    .context("connect before engineering commands")?;
                room_controls::execute(&mut world, name, action, now)?;
                message = "Pioneer room command accepted";
            }
            Request::SetPioneerExterior {
                enabled,
                expected_revision,
            } => {
                let name = session
                    .as_ref()
                    .context("connect before engineering commands")?;
                ensure!(!remote, "exterior controls cannot be remotely delegated");
                exterior_effects::execute(&mut world, name, enabled, expected_revision, now)?;
                message = "Pioneer exterior command accepted";
            }
            Request::BoardShip { id } => {
                let name = session.as_ref().context("connect before boarding")?;
                ensure!(
                    world.pilots[name]
                        .view
                        .station_avatar
                        .as_ref()
                        .is_none_or(|avatar| avatar.docked_traversal.is_none()),
                    "return to your pilot seat before boarding another vessel"
                );
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
                world_layout_version,
                supports_room_controls,
                supports_fleet_hangar,
                supports_pioneer_interior,
                supports_docked_traversal_v2,
                protocol_version,
                pilot,
                resume_token,
            } => {
                ensure!(
                    protocol_version == PROTOCOL_VERSION,
                    "unsupported protocol version"
                );
                ensure!(
                    world_layout_version <= WORLD_LAYOUT_SCHEMA_VERSION,
                    "unsupported world layout capability version"
                );
                ensure!(
                    world_layout_version >= world.scene_layout.required_capability_version(),
                    "this realm requires world layout support; update the client before connecting"
                );
                ensure!(world.drone_bay_registration.is_none() || world_layout_version >= DRONE_BAY_CLIENT_CAPABILITY_VERSION,
                    "this physical drone bay realm requires world feature capability3; update the client before connecting");
                ensure!(world.drone_bay_batch_version.is_none() || world_layout_version >= DRONE_BAY_BATCH_CLIENT_CAPABILITY_VERSION,
                    "this fresh bay batch realm requires world feature capability4; update the client before connecting");
                ensure!(world.drone_cargo_registration.is_none() || world_layout_version >= DRONE_CARGO_CLIENT_CAPABILITY_VERSION,
                    "this ore receiver realm requires world feature capability5; update the client before connecting");
                ensure!(
                    world.independent_bay_registration.is_none()
                        || world_layout_version >= INDEPENDENT_BAY_CLIENT_CAPABILITY_VERSION,
                    "Independent bays require capability6"
                );
                ensure!(
                    world.carrier_inventory_registration.is_none()
                        || world_layout_version >= CARRIER_INVENTORY_CLIENT_CAPABILITY,
                    "Carrier inventory requires capability7"
                );
                let new_pilot = !world.pilots.contains_key(&pilot);
                let continuous_docking = docking::enabled(&world);
                ensure!(
                    !continuous_docking
                        || supports_docked_traversal_v2 && supports_pioneer_interior,
                    "docking preview requires continuous Pioneer traversal support"
                );
                let supports_fleet_hangar = supports_fleet_hangar && !continuous_docking;
                let fleet_revision = world.scene_layout.definition().fleet_layout;
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
                                fleet_berth_pose(fleet_revision, existing.view.ship_class)
                                    .boarding_point
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
                                carrier_inventory: None,
                                drone_cargo: None,
                                power_accounting: None,
                                drone_bay: None,
                                cockpit_mechanisms: None,
                                reactor_configuration: None,
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
                docking::assign(&mut world, &pilot, now)?;
                if new_pilot {
                    drone_bay::create_pilot_stock(&mut world, &pilot, now)?;
                    cargo_initialization = drone_cargo::initialize(&mut world, &pilot, now)?;
                    independent_bays::world::initialize(&mut world, &pilot)?;
                    if world.carrier_inventory_registration.is_some() {
                        cargo_initialization =
                            carrier_inventory::world::initialize(&mut world, &pilot, now)?;
                    }
                }
                let old_fleet_mode = world.accounts[&pilot].fleet_hangar;
                world
                    .accounts
                    .get_mut(&pilot)
                    .unwrap()
                    .docked_traversal_compatible = supports_docked_traversal_v2;
                if !supports_docked_traversal_v2 || old_fleet_mode != supports_fleet_hangar {
                    docked_traversal::legacy_recovery(&mut world, &pilot, now);
                }
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
                                fleet_berth_pose(fleet_revision, p.view.ship_class).boarding_point
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
                carrier_inventory::world::stop_all(
                    &mut world,
                    &pilot,
                    now,
                    "Pilot connected; carrier inventory held",
                )?;
                docking::hold(
                    world.pilots.get_mut(&pilot).unwrap(),
                    now,
                    "Pilot reconnected; docking held",
                );
                cargo_unloading::hold(&mut world, &pilot, "Pilot reconnected; unloading held");
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
                if matches!(&request, Request::Undock) {
                    docked_traversal::can_undock(&world, &name)?;
                }
                let bay_reserved = drone_bay::busy(&world, &name);
                let available_hold_capacity = drone_cargo::hold_capacity(&world, &name);
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
                        pilot.controls =
                            (!bay_reserved).then(|| flight::controls(translation, rotation, now));
                        let f = pilot.view.flight.as_mut().unwrap();
                        // A neutral release packet must not turn a stationary
                        // bay carrier into Manual mode and spuriously hold it.
                        f.mode = if bay_reserved {
                            NavigationMode::Stop
                        } else {
                            NavigationMode::Manual
                        };
                        f.updated_at_ms = now;
                        f.angular_velocity = pilot.controls.as_ref().map_or([0.; 3], |input| {
                            input.rotation.map(|v| v * MAX_ANGULAR_SPEED)
                        });
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
                        pilot.view.ore = add_cargo(pilot.view.ore, units, available_hold_capacity)?;
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
                        pilot.view.ore =
                            add_cargo(pilot.view.ore, order.units, available_hold_capacity)?;
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
        if let Some(name) = session.as_ref() {
            drone_cargo::after_request(&mut world, name, &cargo_intent_request, now, automated)?;
        }
        drone_cargo::prune_claims(&mut world);
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
        drone_bay::validate(&world)?;
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        drone_cargo::validate(&world)?;
        independent_bays::world::validate(&world)?;
        carrier_inventory::world::validate(&world)?;
        turret_stow::validate(&world, now)?;
        power::advance(&mut world, now)?;
        room_controls::advance(&mut world, now)?;
        validate(&world, self.editor_room_layout.as_ref())?;
        let mut entries =
            load_entries(&tx, export || world.independent_bay_registration.is_some())?;
        for event in cycle_events
            .into_iter()
            .chain(event)
            .chain(cargo_initialization)
        {
            append_event(&tx, &mut entries, event, now, drone_cargo::ruleset(&world))?;
        }
        drone_cargo::commit_transfers(&mut world, &tx, &mut entries, now)?;
        independent_bays::world::commit_transfers(&mut world, &tx, &mut entries, now)?;
        carrier_inventory::world::service::commit_transfers(&mut world, &tx, &mut entries, now)?;
        cargo_unloading::advance(&mut world, &tx, &mut entries, now)?;
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        validate(&world, self.editor_room_layout.as_ref())?;
        // Snapshot polling must not rewrite and fsync the entire world unless
        // it resolves a due arrival. Economic updates still commit with their
        // ledger entry in this same transaction.
        universe::advance_locks(&mut world, now);
        mining::invalidate(&mut world);
        drones::invalidate(&mut world);
        carrier_inventory::world::cargo::settle_free_modes(&mut world, now)?;
        drone_cargo::prune_claims(&mut world);
        cockpit_mechanisms::advance(&mut world, now)?;
        turret_stow::advance(&mut world, now);
        turret_aim::advance(&mut world, now);
        reactor_configuration::advance(&mut world, now, self.editor_room_layout.as_ref())?;
        cockpit_mechanisms::validate(&world)?;
        reactor_configuration::validate(&world)?;
        drone_bay::project(&mut world)?;
        drone_cargo::project(&mut world)?;
        let updated_json = serde_json::to_string(&world)?;
        if updated_json != json {
            tx.execute("UPDATE world SET json=?1 WHERE id=1", [updated_json])?;
        }
        let private_assets = connected
            .as_ref()
            .or(transport_session.as_ref())
            .and_then(|name| assets::view(&world, name, now));
        let snapshot = project_snapshot(
            &world,
            &entries,
            now,
            self.online_sessions.keys().cloned().collect(),
        )?;
        tx.commit()?;
        if let Some(pilot) = connected {
            *transport_session = Some(pilot);
        }
        Ok(Response {
            physical_cargo: None,
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
/// Pure accepted-state projection, shared by live responses and carrier-only
/// preflight replay. Does not advance clocks or mutate authority state.
fn project_snapshot(
    world: &World,
    entries: &[LedgerEntry],
    now: u64,
    online_pilots: Vec<String>,
) -> Result<Snapshot> {
    Ok(Snapshot {
        carrier_inventory_registration: world.carrier_inventory_registration.clone(),
        drone_cargo_registration: world.drone_cargo_registration.clone(),
        drone_cargo_totals: drone_cargo::totals(world)?,
        walk_sprint_v1: true,
        docked_traversal_capability: Some(docked_traversal_capability()),
        world_layout: Some(world.scene_layout.definition()),
        exterior: Some(exterior_effects::view(world, now)),
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
        stored_ore: assets::stored_ore(world)
            .try_into()
            .context("stored ore overflow")?,
        server_time_ms: now,
        online_pilots,
        asteroids: world.asteroids.clone(),
        pilots: world
            .pilots
            .iter()
            .map(|(name, p)| {
                let mut view = p.view.clone();
                view.carrier_inventory = carrier_inventory::world::public_view(world, name, now)?;
                view.power_accounting =
                    power::accounting(world, name, view.ship_class, view.power.as_ref(), now);
                Ok(view)
            })
            .collect::<Result<Vec<_>>>()?,
        orders: world.orders.values().cloned().collect(),
        belt_ore: world.belt_ore,
        ledger_sequence: entries.last().map_or(0, |e| e.sequence),
        ledger_hash: entries
            .last()
            .map_or(GENESIS_HASH, |e| e.hash.as_str())
            .into(),
    })
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
    ruleset_version: u32,
) -> Result<()> {
    let last = entries.last();
    let mut entry = LedgerEntry {
        sequence: last
            .map_or(0, |e| e.sequence)
            .checked_add(1)
            .context("ledger sequence overflow")?,
        ruleset_version,
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
fn validate(world: &World, room_layout: Option<&RoomLayoutDocument>) -> Result<()> {
    world
        .scene_layout
        .definition()
        .validate()
        .map_err(anyhow::Error::msg)?;
    docking::validate(world)?;
    cargo_unloading::validate(world)?;
    station::validate(world, room_layout)?;
    docked_traversal::validate(world, room_layout)?;
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
    exterior_effects::validate(world)?;
    room_controls::validate(world)?;
    cockpit_mechanisms::validate(world)?;
    reactor_configuration::validate(world)?;
    mining::validate(world)?;
    universe::validate_locks(world)?;
    drones::validate(world)?;
    drone_bay::validate(world)?;
    drone_cargo::validate(world)?;
    independent_bays::world::validate(world)?;
    carrier_inventory::world::validate(world)?;
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
        + assets::stored_ore(world)
        + world
            .accounts
            .values()
            .filter_map(|a| a.carrier_inventory.as_ref())
            .map(|s| u128::from(s.ore.ore_units().unwrap()) + u128::from(s.ore.lost_ore_units))
            .sum::<u128>()
        + drone_cargo::totals(world)?.map_or(0, |t| {
            u128::from(t.drone_ore_units) + u128::from(t.lost_ore_units)
        });
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
                supports_docked_traversal_v2: false,
                protocol_version: PROTOCOL_VERSION,
                world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
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

// Optional diagnostic accounting only. No production build contains these hooks.
#[cfg(test)]
mod tick_profile {
    use std::{cell::RefCell, collections::BTreeMap, time::Instant};
    #[derive(Default)]
    struct Profile {
        last: Option<Instant>,
        phases: BTreeMap<&'static str, u64>,
        counts: BTreeMap<&'static str, u64>,
        ticks: Vec<serde_json::Value>,
    }
    thread_local! { static ACTIVE: RefCell<Option<Profile>> = const { RefCell::new(None) }; }
    pub(crate) fn enable() {
        ACTIVE.with(|p| *p.borrow_mut() = Some(Profile::default()));
    }
    pub(crate) fn start_tick() {
        ACTIVE.with(|p| {
            if let Some(p) = p.borrow_mut().as_mut() {
                p.phases.clear();
                p.counts.clear();
                p.last = Some(Instant::now());
            }
        });
    }
    pub(crate) fn mark(label: &'static str) {
        ACTIVE.with(|p| {
            if let Some(p) = p.borrow_mut().as_mut() {
                let now = Instant::now();
                if let Some(last) = p.last {
                    *p.phases.entry(label).or_default() +=
                        now.duration_since(last).as_micros() as u64;
                }
                p.last = Some(now);
            }
        });
    }
    pub(crate) fn count(label: &'static str) {
        ACTIVE.with(|p| {
            if let Some(p) = p.borrow_mut().as_mut() {
                *p.counts.entry(label).or_default() += 1;
            }
        });
    }
    pub(crate) fn finish_tick() {
        ACTIVE.with(|p| {
            if let Some(p) = p.borrow_mut().as_mut() {
                p.ticks
                    .push(serde_json::json!({"phase_us":p.phases,"calls":p.counts}));
                p.last = None;
            }
        });
    }
    pub(crate) fn take() -> Vec<serde_json::Value> {
        ACTIVE.with(|p| p.borrow_mut().take().unwrap_or_default().ticks)
    }
}
