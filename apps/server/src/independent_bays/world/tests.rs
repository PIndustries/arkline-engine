//! Fixed-source stationary disposable-world integration. Initial carrier pose
//! and nearby catalog asteroid are explicit fixture conditions, not travel proof.
use super::*;
use crate::{token_hash, Server};
struct Fixture {
    server: Server,
    session: Option<String>,
    now: u64,
    path: std::path::PathBuf,
    token: String,
}
fn connect(token: Option<String>) -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        world_layout_version: 6,
        supports_room_controls: true,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: true,
        pilot: "Pilot".into(),
        resume_token: token,
    }
}
impl Fixture {
    fn attack_identity(&mut self) -> u64 {
        let mut session = None;
        let mut request = connect(None);
        if let Request::Connect { pilot, .. } = &mut request {
            *pilot = "Attacker".into();
        }
        let response = self.server.handle_at(&mut session, request, self.now);
        assert!(response.ok, "{}", response.message);
        let mut w = self.world();
        let p = w.pilots.get_mut("Attacker").unwrap();
        p.view.location = Location::Station;
        p.view.station_avatar = None;
        let flight = p.view.flight.as_mut().unwrap();
        flight.position = [2000., 0., 0.];
        flight.orientation = [0., 0., 0., 1.];
        flight.heading = [1., 0., 0.];
        flight.velocity = [0.; 3];
        flight.acceleration = [0.; 3];
        flight.angular_velocity = [0.; 3];
        flight.mode = NavigationMode::Stop;
        let dock = flight.docking.as_mut().unwrap();
        dock.phase = DockingPhase::Free;
        dock.clamps = 0.;
        dock.segment = None;
        let drone = drone_bay::state(&w, "Attacker")
            .unwrap()
            .stock
            .iter()
            .find(|d| d.class == DroneClass::Attack)
            .unwrap()
            .drone_id;
        let binding = state(&w, "Attacker").unwrap().state.binding.clone();
        self.write(&w);
        let response = self.server.handle_at(
            &mut session,
            Request::IndependentBays {
                request: IndependentBayRequest::Begin {
                    binding,
                    operation_id: 1,
                    drone_id: drone,
                    action: IndependentBayAction::Launch,
                },
            },
            self.now,
        );
        assert!(response.ok, "{}", response.message);
        for _ in 0..500 {
            if state(&self.world(), "Attacker")
                .unwrap()
                .state
                .operation(1)
                .unwrap()
                .phase
                .terminal()
            {
                return drone;
            }
            self.step()
        }
        panic!("Actual attacker launch did not finish")
    }
    fn ready_lethal_attack(&mut self, victim: u64, attacker: u64) {
        // Explicit combat setup: already launched finite attacker, live damaged
        // victim, ordinary ready attack. No zero-HP, hit or ore injection.
        let mut w = self.world();
        let d = w
            .pilots
            .get_mut("Pilot")
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == victim)
            .unwrap();
        d.hit_points = DRONE_ATTACK_DAMAGE;
        let position = d.position;
        let d = w
            .pilots
            .get_mut("Attacker")
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == attacker)
            .unwrap();
        d.position = position;
        d.position[1] += DRONE_ATTACK_RANGE * 0.5;
        d.velocity = [0.; 3];
        d.mode = DroneMode::Attacking {
            target_pilot: "Pilot".into(),
            target_drone_id: victim,
        };
        d.attack_ready_at_ms = Some(self.now);
        d.updated_at_ms = self.now;
        drones::sync(w.pilots.get_mut("Pilot").unwrap());
        drones::sync(w.pilots.get_mut("Attacker").unwrap());
        self.write(&w);
    }
    fn mine_units(&mut self, units: u64) {
        // Relocate one actual catalog asteroid, preserving its ID, radius and
        // finite reserve. No drone pose, ore balance, hit or cycle is injected.
        let mut w = self.world();
        let a = w
            .asteroids
            .iter_mut()
            .find(|a| a.system_id == HOME_SYSTEM_ID)
            .unwrap();
        a.position = [-1200., 100., -800.];
        let target = a.id;
        self.write(&w);
        self.ok(Request::LockAsteroid { id: target });
        for _ in 0..300 {
            if self.world().pilots["Pilot"]
                .view
                .locked_asteroids
                .contains(&target)
            {
                break;
            }
            self.step()
        }
        assert!(self.world().pilots["Pilot"]
            .view
            .locked_asteroids
            .contains(&target));
        self.ok(Request::AssignMiningDrone {
            id: 6,
            target_id: target,
        });
        for _ in 0..2000 {
            if drone_cargo::payload(&self.world(), "Pilot", 6) >= units {
                return;
            }
            self.step()
        }
        panic!("Real mining cycles did not finish")
    }
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "arkline-independent-fixed-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("world.sqlite");
        let def = geometry::stationary_fixture_definition().unwrap();
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE identity_realm(id INTEGER PRIMARY KEY,realm TEXT NOT NULL); INSERT INTO identity_realm VALUES(1,'local-development-docking-v1'); CREATE TABLE world(id INTEGER PRIMARY KEY,json TEXT NOT NULL);").unwrap();
            let world = World {
                independent_bay_registration: Some(def.registration.clone()),
                drone_bay_registration: Some(def.registration.host.clone()),
                drone_bay_batch_version: Some(DRONE_BAY_BATCH_VERSION),
                drone_cargo_registration: Some(cargo_registration(def)),
                scene_layout: SceneLayoutRevision::DockingV1,
                ..World::default()
            };
            db.execute(
                "INSERT INTO world VALUES(1,?1)",
                [serde_json::to_string(&world).unwrap()],
            )
            .unwrap();
        }
        let mut server = Server::open_editor_realm_with_scene(
            &path,
            default_pioneer_room_layout(),
            SceneLayoutRevision::DockingV1,
        )
        .unwrap();
        let now = server.editor_time.unwrap();
        let mut session = None;
        let response = server.handle_at(&mut session, connect(None), now);
        assert!(response.ok, "{}", response.message);
        let token = response.resume_token.unwrap();
        let mut f = Self {
            server,
            session,
            now,
            path,
            token,
        };
        // One initial stationary fixture setup before any bay command or mining.
        // No runtime constructor authorizes this pose or variant flight yet.
        let mut world = f.world();
        let p = world.pilots.get_mut("Pilot").unwrap();
        p.view.location = Location::Station;
        p.view.station_avatar = None;
        let flight = p.view.flight.as_mut().unwrap();
        flight.position = [0., 0., 0.];
        flight.orientation = [0., 0., 0., 1.];
        flight.heading = [1., 0., 0.];
        flight.velocity = [0.; 3];
        flight.acceleration = [0.; 3];
        flight.angular_velocity = [0.; 3];
        flight.mode = NavigationMode::Stop;
        let dock = flight.docking.as_mut().unwrap();
        dock.phase = DockingPhase::Free;
        dock.clamps = 0.;
        dock.segment = None;
        f.write(&world);
        f.ok(Request::Snapshot);
        f
    }
    fn world(&self) -> World {
        serde_json::from_str(
            &self
                .server
                .db
                .query_row("SELECT json FROM world WHERE id=1", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap(),
        )
        .unwrap()
    }
    fn write(&mut self, w: &World) {
        self.server
            .db
            .execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(w).unwrap()],
            )
            .unwrap();
    }
    fn ok(&mut self, request: Request) -> Response {
        let response = self.server.handle_at(&mut self.session, request, self.now);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn step(&mut self) {
        self.now += 100;
        if let Err(error) = self.server.tick_at(self.now) {
            panic!(
                "Tick {} failed: {error}; retained operations={:?}",
                self.now,
                state(&self.world(), "Pilot").map(|s| &s.state.operations)
            );
        }
    }
    fn begin(&mut self, action: IndependentBayAction) -> u64 {
        self.begin_drone(6, action)
    }
    fn begin_drone(&mut self, drone_id: u64, action: IndependentBayAction) -> u64 {
        let w = self.world();
        let s = state(&w, "Pilot").unwrap();
        let id = s.state.high_watermark + 1;
        self.ok(Request::IndependentBays {
            request: IndependentBayRequest::Begin {
                binding: s.state.binding.clone(),
                operation_id: id,
                drone_id,
                action,
            },
        });
        id
    }
    fn until(&mut self, id: u64) {
        for _ in 0..3000 {
            let w = self.world();
            let op = state(&w, "Pilot").unwrap().state.operation(id).unwrap();
            if op.phase.terminal() {
                return;
            }
            assert_ne!(
                op.phase,
                IndependentBayPhase::Held,
                "held op {id}: {}; movement={:?}",
                op.message,
                state(&w, "Pilot").unwrap().runs.get(&id)
            );
            self.step()
        }
        panic!(
            "Independent operation did not finish: {:?}",
            state(&self.world(), "Pilot").unwrap().state.operation(id)
        );
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
    }
}
#[test]
fn exact_stationary_source_real_request_launch_and_full_capture_preserve_identity() {
    let mut f = Fixture::new();
    let initial = f.world();
    let stock = drone_bay::state(&initial, "Pilot").unwrap().stock.clone();
    let launch = f.begin(IndependentBayAction::Launch);
    f.until(launch);
    let w = f.world();
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 1);
    assert_eq!(w.pilots["Pilot"].view.drones[0].id, 6);
    assert_eq!(w.pilots["Pilot"].view.drones[0].hit_points, 100);
    let recover = f.begin(IndependentBayAction::ReturnToStorage);
    f.until(recover);
    let w = f.world();
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(drone_bay::state(&w, "Pilot").unwrap().stock, stock);
    assert_eq!(
        state(&w, "Pilot")
            .unwrap()
            .state
            .operation(recover)
            .unwrap()
            .phase,
        IndependentBayPhase::Stored
    );
    assert_eq!(w.pilots["Pilot"].token_hash, token_hash(&f.token));
}
#[test]
fn actual_mining_payload_uses_confirmed_shared_manifold_then_full_storage() {
    let mut f = Fixture::new();
    let launch = f.begin(IndependentBayAction::Launch);
    f.until(launch);
    f.mine_units(40);
    let before = f.world();
    assert_eq!(drone_cargo::payload(&before, "Pilot", 6), 40);
    assert_eq!(before.pilots["Pilot"].view.ore, 0);
    let storage = f.begin(IndependentBayAction::ReturnToStorage);
    f.until(storage);
    let w = f.world();
    assert_eq!(w.pilots["Pilot"].view.ore, 40);
    assert_eq!(drone_cargo::payload(&w, "Pilot", 6), 0);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    let cargo = drone_cargo::state(&w, "Pilot").unwrap();
    assert_eq!(cargo.operations.len(), 1);
    assert_eq!(
        cargo.operations[0]
            .receipts
            .iter()
            .map(|r| r.units)
            .collect::<Vec<_>>(),
        [25, 15]
    );
    assert_eq!(
        cargo.operations[0].receipts[1].committed_at_ms
            - cargo.operations[0].receipts[0].committed_at_ms,
        500
    );
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.state
            .operation(storage)
            .unwrap()
            .cargo_service_operation_ids,
        [1]
    );
    assert_eq!(
        s.state.operation(storage).unwrap().phase,
        IndependentBayPhase::Stored
    );
    validate_history(&w, &f.server.db).unwrap();
    assert_eq!(w.belt_ore + 40, INITIAL_BELT_ORE);
}

#[test]
fn all_fifteen_real_lanes_launch_and_capture_without_global_handling_lock() {
    let mut f = Fixture::new();
    let initial = f.world();
    let original = drone_bay::state(&initial, "Pilot").unwrap().stock.clone();
    let ids: Vec<_> = (1..=15)
        .map(|drone| f.begin_drone(drone, IndependentBayAction::Launch))
        .collect();
    let started = f.world();
    assert_eq!(started.pilots["Pilot"].view.drones.len(), 15);
    assert_eq!(state(&started, "Pilot").unwrap().runs.len(), 15);
    for id in ids {
        f.until(id)
    }
    let free = f.world();
    assert_eq!(free.pilots["Pilot"].view.drones.len(), 15);
    assert!(state(&free, "Pilot").unwrap().runs.is_empty());
    let ids: Vec<_> = (1..=15)
        .rev()
        .map(|drone| f.begin_drone(drone, IndependentBayAction::ReturnToStorage))
        .collect();
    assert_eq!(state(&f.world(), "Pilot").unwrap().runs.len(), 15);
    for id in ids {
        f.until(id)
    }
    let w = f.world();
    assert_eq!(drone_bay::state(&w, "Pilot").unwrap().stock, original);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(
        w.pilots["Pilot"].view.flight.as_ref().unwrap().position,
        [0.; 3]
    );
    assert_eq!(w.belt_ore, INITIAL_BELT_ORE);
}

#[test]
fn quick_service_resumes_same_real_mining_intent_before_later_storage() {
    let mut f = Fixture::new();
    let launch = f.begin(IndependentBayAction::Launch);
    f.until(launch);
    f.mine_units(20);
    let old = drone_cargo::state(&f.world(), "Pilot").unwrap().holds[0]
        .mining_intent
        .clone();
    let service = f.begin(IndependentBayAction::ReturnToUnload {
        resume_mining: true,
    });
    f.until(service);
    let w = f.world();
    assert_eq!(
        state(&w, "Pilot")
            .unwrap()
            .state
            .operation(service)
            .unwrap()
            .phase,
        IndependentBayPhase::Free
    );
    assert_eq!(w.pilots["Pilot"].view.ore, 20);
    assert!(matches!(
        w.pilots["Pilot"].view.drones[0].mode,
        DroneMode::Mining { .. }
    ));
    assert_eq!(
        drone_cargo::state(&w, "Pilot").unwrap().holds[0].mining_intent,
        old
    );
    for _ in 0..1500 {
        if drone_cargo::payload(&f.world(), "Pilot", 6) >= 10 {
            break;
        }
        f.step()
    }
    assert_eq!(drone_cargo::payload(&f.world(), "Pilot", 6), 10);
    let storage = f.begin(IndependentBayAction::ReturnToStorage);
    f.until(storage);
    let w = f.world();
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(w.pilots["Pilot"].view.ore, 30);
    assert_eq!(w.belt_ore + 30, INITIAL_BELT_ORE);
}

#[test]
fn destroyed_cleanup_is_explicitly_held_on_short_disconnect_and_restart() {
    let mut f = Fixture::new();
    let attacker = f.attack_identity();
    let launch = f.begin(IndependentBayAction::Launch);
    for _ in 0..15 {
        f.step()
    }
    f.ready_lethal_attack(6, attacker);
    f.step();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.state.operation(launch).unwrap().phase,
        IndependentBayPhase::Destroyed
    );
    assert!(s.runs[&launch].cleanup_only);
    f.server.stop_disconnected("Pilot").unwrap();
    let before = f.world();
    let b = state(&before, "Pilot").unwrap();
    assert!(b.state.lanes[5].cleanup.as_ref().unwrap().held);
    let poses = b.mechanisms.clone();
    f.step();
    assert_eq!(state(&f.world(), "Pilot").unwrap().mechanisms, poses);
    let replacement = Server::open(":memory:").unwrap();
    drop(std::mem::replace(&mut f.server, replacement));
    f.server = Server::open_editor_realm_with_scene(
        &f.path,
        default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1,
    )
    .unwrap();
    f.session = None;
    f.ok(connect(Some(f.token.clone())));
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.mechanisms, poses);
    assert!(s.state.lanes[5].cleanup.as_ref().unwrap().held);
    assert_eq!(
        s.state.operation(launch).unwrap().phase,
        IndependentBayPhase::Destroyed
    );
}

#[test]
fn unsupported_assembly_without_sql_guard_is_refused_before_any_database_write() {
    for mutation in [
        "UPDATE world SET json=json_set(json,'$.independent_bay_registration.assembly_revision',99999)",
        "UPDATE world SET json=json_remove(json,'$.independent_bay_registration')",
        "UPDATE world SET json=json_set(json,'$.pilots.Pilot.view.flight.velocity[0]',1.0)",
    ] {
        let mut f=Fixture::new();
        let replacement=Server::open(":memory:").unwrap();drop(std::mem::replace(&mut f.server,replacement));
        {let db=rusqlite::Connection::open(&f.path).unwrap();db.execute_batch(&format!("DROP TRIGGER preserve_independent_bays_v1; {mutation}; PRAGMA wal_checkpoint(TRUNCATE);")).unwrap();}
        let before=std::fs::read(&f.path).unwrap();
        let result=Server::open_editor_realm_with_scene(&f.path,default_pioneer_room_layout(),SceneLayoutRevision::DockingV1);
        assert!(result.is_err(),"accepted mutation: {mutation}");assert_eq!(std::fs::read(&f.path).unwrap(),before,"mutated rejected save: {mutation}");
        let db=rusqlite::Connection::open(&f.path).unwrap();let guard:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='trigger' AND name='preserve_independent_bays_v1')",[],|r|r.get(0)).unwrap();assert!(!guard);
    }
}

#[test]
fn unavailable_home_uses_only_empty_compatible_berth_and_competing_return_waits() {
    let mut f = Fixture::new();
    let a = f.begin_drone(6, IndependentBayAction::Launch);
    let b = f.begin_drone(7, IndependentBayAction::Launch);
    f.until(a);
    f.until(b);
    // Real free-flight reposition clears the prospective spare's gate; no snap.
    let away = [-150., 60., -200.];
    f.ok(Request::DroneNavigateById {
        id: 7,
        position: away,
    });
    for _ in 0..300 {
        let w = f.world();
        let d = w.pilots["Pilot"]
            .view
            .drones
            .iter()
            .find(|d| d.id == 7)
            .unwrap();
        if crate::flight::distance(d.position, away) < 1. {
            break;
        }
        f.step()
    }
    f.ok(Request::DroneHoldById { id: 7 });
    let mut w = f.world();
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .independent_bays
        .as_mut()
        .unwrap()
        .state
        .lanes[5]
        .components
        .door = IndependentBayAvailability::Unavailable {
        reason: "Explicit test-only door availability fault".into(),
    };
    f.write(&w);
    let first = f.begin_drone(6, IndependentBayAction::ReturnToStorage);
    let second = f.begin_drone(7, IndependentBayAction::ReturnToStorage);
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.state.operation(first).unwrap().assigned_bay, Some(6));
    assert_eq!(
        s.state.operation(second).unwrap().phase,
        IndependentBayPhase::Queued
    );
    assert_eq!(s.state.operation(second).unwrap().assigned_bay, None);
    assert_eq!(s.state.lanes[6].operation_id, Some(first));
    f.until(first);
    let w = f.world();
    assert_eq!(
        state(&w, "Pilot").unwrap().state.lanes[6].stored_drone_id,
        Some(6)
    );
    assert_eq!(
        state(&w, "Pilot")
            .unwrap()
            .state
            .operation(second)
            .unwrap()
            .phase,
        IndependentBayPhase::Queued
    );
    let mut w = w;
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .independent_bays
        .as_mut()
        .unwrap()
        .state
        .lanes[5]
        .components
        .door = IndependentBayAvailability::Available;
    f.write(&w);
    f.until(second);
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.state.lanes[5].stored_drone_id, Some(7));
    assert_eq!(s.state.lanes[6].stored_drone_id, Some(6));
    let stock = drone_bay::state(&w, "Pilot").unwrap();
    assert_eq!(
        stock
            .stock
            .iter()
            .find(|d| d.drone_id == 6)
            .unwrap()
            .home_cradle,
        5
    );
    assert_eq!(
        stock
            .stock
            .iter()
            .find(|d| d.drone_id == 7)
            .unwrap()
            .home_cradle,
        6
    );
}

#[test]
fn injected_midstroke_unavailable_component_holds_exact_pose_until_repaired_and_resumed() {
    let mut f = Fixture::new();
    let id = f.begin(IndependentBayAction::Launch);
    for _ in 0..20 {
        f.step()
    }
    let mut w = f.world();
    let before = state(&w, "Pilot").unwrap().mechanisms.clone();
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .independent_bays
        .as_mut()
        .unwrap()
        .state
        .lanes[5]
        .components
        .support = IndependentBayAvailability::Unavailable {
        reason: "Explicit test-only support availability fault".into(),
    };
    f.write(&w);
    f.step();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.state.operation(id).unwrap().phase,
        IndependentBayPhase::Held
    );
    assert_eq!(s.mechanisms, before);
    let binding = s.state.binding.clone();
    f.ok(Request::IndependentBays {
        request: IndependentBayRequest::Resume {
            binding: binding.clone(),
            operation_id: id,
        },
    });
    f.step();
    assert_eq!(state(&f.world(), "Pilot").unwrap().mechanisms, before);
    let mut w = f.world();
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .independent_bays
        .as_mut()
        .unwrap()
        .state
        .lanes[5]
        .components
        .support = IndependentBayAvailability::Available;
    f.write(&w);
    f.ok(Request::IndependentBays {
        request: IndependentBayRequest::Resume {
            binding,
            operation_id: id,
        },
    });
    f.until(id);
    assert_eq!(
        state(&f.world(), "Pilot")
            .unwrap()
            .state
            .operation(id)
            .unwrap()
            .phase,
        IndependentBayPhase::Free
    );
}

#[test]
#[ignore = "writes an explicitly requested actual stationary-source native evidence journey"]
fn export_actual_stationary_lane_journey() {
    use sha2::{Digest, Sha256};
    let directory = std::path::PathBuf::from(
        std::env::var("ARKLINE_INDEPENDENT_EXPORT").expect("explicit output directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let producer = std::env::var("ARKLINE_PRODUCER_COMMIT").expect("pinned producer commit");
    let launch_only = std::env::var_os("ARKLINE_INDEPENDENT_NO_MINING").is_some();
    let mut f = Fixture::new();
    let mut setup = f.world();
    let asteroid = setup
        .asteroids
        .iter_mut()
        .find(|a| a.system_id == HOME_SYSTEM_ID)
        .unwrap();
    asteroid.position = [-1200., 100., -800.];
    let target = asteroid.id;
    let target_position = asteroid.position;
    f.write(&setup);
    let start = f.now;
    let mut rows = Vec::new();
    let mut landmarks = serde_json::Map::new();
    let record = |f: &mut Fixture, rows: &mut Vec<serde_json::Value>| {
        let response = f.ok(Request::Snapshot);
        assert!(response.resume_token.is_none());
        let w = f.world();
        let owned = state(&w, "Pilot").unwrap();
        let stock = drone_bay::state(&w, "Pilot").unwrap();
        let cargo = drone_cargo::state(&w, "Pilot").unwrap();
        assert_eq!(stock.stock.len(), 15);
        assert_eq!(
            w.belt_ore
                + w.pilots["Pilot"].view.ore
                + cargo.ore_units().unwrap()
                + cargo.lost_ore_units,
            INITIAL_BELT_ORE
        );
        let motions = owned
            .runs
            .iter()
            .map(|(id, run)| run.view(owned.state.operation(*id).unwrap().phase).unwrap())
            .collect::<Vec<_>>();
        let lane_mechanisms=owned.mechanisms.iter().map(|(bay,roots)|serde_json::json!({"bay_id":bay,"groups":roots.iter().map(|(id,pose)|IndependentBayMechanismView{semantic_id:id.clone(),pose:IndependentBayPoseView{confirmed:*pose,segment:None}}).collect::<Vec<_>>()})).collect::<Vec<_>>();
        let value = serde_json::json!({"response":response,"sample_time_ms":f.now,"binding":owned.state.binding,"stock":stock.stock,"state":owned.state,"cargo":cargo,"lane_mechanisms":lane_mechanisms,"motions":motions});
        let file = format!("frame-{:05}.json", rows.len());
        let bytes = serde_json::to_vec(&value).unwrap();
        let hash = hex::encode(Sha256::digest(&bytes));
        std::fs::write(directory.join(&file), bytes).unwrap();
        rows.push(serde_json::json!({"file":file,"response_sha256":hash,"index":rows.len(),"elapsed_ms":f.now-start}));
    };
    record(&mut f, &mut rows);
    for _ in 0..10 {
        f.step();
        record(&mut f, &mut rows)
    }
    let launch = f.begin(IndependentBayAction::Launch);
    landmarks.insert("launch_requested".into(), serde_json::json!(rows.len()));
    for _ in 0..2000 {
        f.step();
        record(&mut f, &mut rows);
        if state(&f.world(), "Pilot")
            .unwrap()
            .state
            .operation(launch)
            .unwrap()
            .phase
            .terminal()
        {
            break;
        }
    }
    assert_eq!(
        state(&f.world(), "Pilot")
            .unwrap()
            .state
            .operation(launch)
            .unwrap()
            .phase,
        IndependentBayPhase::Free
    );
    landmarks.insert(
        "outside_and_secured".into(),
        serde_json::json!(rows.len() - 1),
    );
    if !launch_only {
        f.ok(Request::LockAsteroid { id: target });
        for _ in 0..300 {
            f.step();
            record(&mut f, &mut rows);
            if f.world().pilots["Pilot"]
                .view
                .locked_asteroids
                .contains(&target)
            {
                break;
            }
        }
        f.ok(Request::AssignMiningDrone {
            id: 6,
            target_id: target,
        });
        landmarks.insert("mining_requested".into(), serde_json::json!(rows.len()));
        for _ in 0..2000 {
            f.step();
            record(&mut f, &mut rows);
            if drone_cargo::payload(&f.world(), "Pilot", 6) >= 40 {
                break;
            }
        }
        assert_eq!(drone_cargo::payload(&f.world(), "Pilot", 6), 40);
        landmarks.insert(
            "payload40_confirmed".into(),
            serde_json::json!(rows.len() - 1),
        );
        let service = f.begin(IndependentBayAction::ReturnToUnload {
            resume_mining: false,
        });
        for _ in 0..3000 {
            f.step();
            record(&mut f, &mut rows);
            let w = f.world();
            let op = state(&w, "Pilot")
                .unwrap()
                .state
                .operation(service)
                .unwrap();
            assert_ne!(op.phase, IndependentBayPhase::Held, "{}", op.message);
            if op.phase.terminal() {
                break;
            }
        }
        assert_eq!(f.world().pilots["Pilot"].view.ore, 40);
        landmarks.insert("service_clear".into(), serde_json::json!(rows.len() - 1));
    }
    let storage = f.begin(IndependentBayAction::ReturnToStorage);
    for _ in 0..2000 {
        f.step();
        record(&mut f, &mut rows);
        let w = f.world();
        let op = state(&w, "Pilot")
            .unwrap()
            .state
            .operation(storage)
            .unwrap();
        assert_ne!(op.phase, IndependentBayPhase::Held, "{}", op.message);
        if op.phase.terminal() {
            break;
        }
    }
    assert_eq!(
        state(&f.world(), "Pilot")
            .unwrap()
            .state
            .operation(storage)
            .unwrap()
            .phase,
        IndependentBayPhase::Stored
    );
    landmarks.insert("stored_secured".into(), serde_json::json!(rows.len() - 1));
    for _ in 0..20 {
        f.step();
        record(&mut f, &mut rows)
    }
    let ledger = f.ok(Request::ExportLedger).ledger.unwrap();
    std::fs::write(
        directory.join("ledger.json"),
        serde_json::to_vec(&ledger).unwrap(),
    )
    .unwrap();
    let receipt = serde_json::json!({"format":1,"producer_commit":producer,"pilot":"Pilot","cadence_ms":100,"duration_ms":f.now-start,"binding":state(&f.world(),"Pilot").unwrap().state.binding,"drone_id":6,"home_index":5,"display_home":6,"qualification":"Actual Request/tick/SQLite stationary disposable source fixture. Carrier seeded at fixed identity pose; one real finite catalog asteroid relocated before capture. Not natural travel or normal-world source admission. No ore, drone HP, hit or cycle injection.","asteroid_fixture":{"id":target,"position_m":target_position},"launch_only_no_mining":launch_only,"landmarks":landmarks,"frames":rows});
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "writes the explicitly requested actual fifteen-lane concurrent journey"]
fn export_actual_fifteen_lane_journey() {
    use sha2::{Digest, Sha256};
    let directory = std::path::PathBuf::from(
        std::env::var("ARKLINE_INDEPENDENT_EXPORT").expect("explicit output directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let producer = std::env::var("ARKLINE_PRODUCER_COMMIT").expect("pinned producer commit");
    let mut f = Fixture::new();
    let start = f.now;
    let initial = f.world();
    let stock = drone_bay::state(&initial, "Pilot").unwrap().stock.clone();
    let mut rows = Vec::new();
    let mut landmarks = serde_json::Map::new();
    let record = |f: &mut Fixture, rows: &mut Vec<serde_json::Value>| {
        let response = f.ok(Request::Snapshot);
        assert!(response.resume_token.is_none());
        let w = f.world();
        let owned = state(&w, "Pilot").unwrap();
        let stock = drone_bay::state(&w, "Pilot").unwrap();
        let cargo = drone_cargo::state(&w, "Pilot").unwrap();
        assert_eq!(stock.stock.len(), 15);
        assert_eq!(
            w.belt_ore
                + w.pilots["Pilot"].view.ore
                + cargo.ore_units().unwrap()
                + cargo.lost_ore_units,
            INITIAL_BELT_ORE
        );
        let motions = owned
            .runs
            .iter()
            .map(|(id, run)| run.view(owned.state.operation(*id).unwrap().phase).unwrap())
            .collect::<Vec<_>>();
        let lane_mechanisms=owned.mechanisms.iter().map(|(bay,roots)|serde_json::json!({"bay_id":bay,"groups":roots.iter().map(|(id,pose)|IndependentBayMechanismView{semantic_id:id.clone(),pose:IndependentBayPoseView{confirmed:*pose,segment:None}}).collect::<Vec<_>>()})).collect::<Vec<_>>();
        let value = serde_json::json!({"response":response,"sample_time_ms":f.now,"binding":owned.state.binding,"stock":stock.stock,"state":owned.state,"cargo":cargo,"lane_mechanisms":lane_mechanisms,"motions":motions});
        let file = format!("frame-{:05}.json", rows.len());
        let bytes = serde_json::to_vec(&value).unwrap();
        let hash = hex::encode(Sha256::digest(&bytes));
        std::fs::write(directory.join(&file), bytes).unwrap();
        rows.push(serde_json::json!({"file":file,"response_sha256":hash,"index":rows.len(),"elapsed_ms":f.now-start}));
    };

    record(&mut f, &mut rows);
    for _ in 0..10 {
        f.step();
        record(&mut f, &mut rows)
    }
    let launches: Vec<_> = stock
        .iter()
        .map(|d| f.begin_drone(d.drone_id, IndependentBayAction::Launch))
        .collect();
    landmarks.insert("all_launch_requested".into(), serde_json::json!(rows.len()));
    let launch_started = f.now;
    for _ in 0..1000 {
        f.step();
        record(&mut f, &mut rows);
        let w = f.world();
        let owned = state(&w, "Pilot").unwrap();
        for id in &launches {
            let op = owned.state.operation(*id).unwrap();
            assert_ne!(op.phase, IndependentBayPhase::Held, "{}", op.message);
            if op.phase == IndependentBayPhase::Free {
                landmarks
                    .entry(format!("drone_{}_outside_secured", op.drone_id))
                    .or_insert_with(|| serde_json::json!(rows.len() - 1));
            }
        }
        if launches
            .iter()
            .all(|id| owned.state.operation(*id).unwrap().phase == IndependentBayPhase::Free)
        {
            break;
        }
    }
    let free = f.world();
    assert_eq!(free.pilots["Pilot"].view.drones.len(), 15);
    assert!(state(&free, "Pilot").unwrap().runs.is_empty());
    assert!(launches.iter().all(|id| state(&free, "Pilot")
        .unwrap()
        .state
        .operation(*id)
        .unwrap()
        .phase
        == IndependentBayPhase::Free));
    let launch_duration = f.now - launch_started;
    landmarks.insert(
        "all_outside_and_secured".into(),
        serde_json::json!(rows.len() - 1),
    );
    for _ in 0..20 {
        f.step();
        record(&mut f, &mut rows)
    }
    let recoveries: Vec<_> = stock
        .iter()
        .rev()
        .map(|d| f.begin_drone(d.drone_id, IndependentBayAction::ReturnToStorage))
        .collect();
    assert_eq!(state(&f.world(), "Pilot").unwrap().runs.len(), 15);
    landmarks.insert(
        "all_storage_requested".into(),
        serde_json::json!(rows.len()),
    );
    let recovery_started = f.now;
    for _ in 0..1000 {
        f.step();
        record(&mut f, &mut rows);
        let w = f.world();
        let owned = state(&w, "Pilot").unwrap();
        for id in &recoveries {
            let op = owned.state.operation(*id).unwrap();
            assert_ne!(op.phase, IndependentBayPhase::Held, "{}", op.message);
            if op.phase == IndependentBayPhase::Stored {
                landmarks
                    .entry(format!("drone_{}_stored_secured", op.drone_id))
                    .or_insert_with(|| serde_json::json!(rows.len() - 1));
            }
        }
        if recoveries
            .iter()
            .all(|id| owned.state.operation(*id).unwrap().phase == IndependentBayPhase::Stored)
        {
            break;
        }
    }
    let final_world = f.world();
    assert!(final_world.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(
        drone_bay::state(&final_world, "Pilot").unwrap().stock,
        stock
    );
    assert!(state(&final_world, "Pilot").unwrap().runs.is_empty());
    assert_eq!(final_world.belt_ore, INITIAL_BELT_ORE);
    assert_eq!(final_world.pilots["Pilot"].view.ore, 0);
    let recovery_duration = f.now - recovery_started;
    landmarks.insert(
        "all_stored_secured".into(),
        serde_json::json!(rows.len() - 1),
    );
    for _ in 0..20 {
        f.step();
        record(&mut f, &mut rows)
    }
    let ledger = f.ok(Request::ExportLedger).ledger.unwrap();
    std::fs::write(
        directory.join("ledger.json"),
        serde_json::to_vec(&ledger).unwrap(),
    )
    .unwrap();
    let receipt = serde_json::json!({"format":1,"producer_commit":producer,"pilot":"Pilot","cadence_ms":100,"duration_ms":f.now-start,"launch_duration_ms":launch_duration,"recovery_duration_ms":recovery_duration,"binding":state(&f.world(),"Pilot").unwrap().state.binding,"drone_ids":stock.iter().map(|d|d.drone_id).collect::<Vec<_>>(),"initial_stock":stock,"launch_operation_ids":launches,"recovery_operation_ids":recoveries,"qualification":"Actual Request/tick/SQLite stationary disposable source fixture; all15 original drones launch in concurrent source-bound independent lanes and return through complete physical capture. Carrier fixed identity pose before capture. No asteroid relocation, mining, ore, HP, hit or cycle injection. No normal-world/travel admission.","landmarks":landmarks,"frames":rows});
    std::fs::write(
        directory.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}
