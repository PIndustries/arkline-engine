//! Disposable stationary source fixture. No flight admission, inventory injection
//! after genesis, cargo yield, or time-rate changes are implied by these tests.
use super::*;
use crate::Server;
struct Fixture {
    server: Server,
    session: Option<String>,
    token: String,
    now: u64,
    path: std::path::PathBuf,
}
fn connect() -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        world_layout_version: 7,
        supports_room_controls: true,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: true,
        pilot: "Pilot".into(),
        resume_token: None,
    }
}
impl Fixture {
    fn new() -> Self {
        Self::with_source(geometry::stationary_fixture_definition().unwrap())
    }
    fn new_1702() -> Self {
        let mut registration = geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
        registration.assembly_revision = 1702;
        registration.assembly_source_sha256 =
            "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113".into();
        Self::with_source(geometry::stationary_fixture_for_registration(&registration).unwrap())
    }
    fn with_source(def: &CarrierInventoryDefinition) -> Self {
        let path = std::env::temp_dir()
            .join(format!(
                "arkline-carrier-{}-{}",
                std::process::id(),
                rand::random::<u64>()
            ))
            .join("world.sqlite");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE identity_realm(id INTEGER PRIMARY KEY,realm TEXT NOT NULL); INSERT INTO identity_realm VALUES(1,'local-development-docking-v1'); CREATE TABLE world(id INTEGER PRIMARY KEY,json TEXT NOT NULL);").unwrap();
            let world = World {
                carrier_inventory_registration: Some(def.topology.registration.clone()),
                carrier_inventory_loadout: Some(vec![DroneClass::Mining; def.topology.capacity()]),
                scene_layout: SceneLayoutRevision::DockingV1,
                ..World::default()
            };
            db.execute(
                "INSERT INTO world VALUES(1,?1)",
                [serde_json::to_string(&world).unwrap()],
            )
            .unwrap();
        }
        let server = Server::open_editor_realm_with_scene(
            &path,
            default_pioneer_room_layout(),
            SceneLayoutRevision::DockingV1,
        )
        .unwrap();
        let now = server.editor_time.unwrap();
        let mut f = Self {
            server,
            session: None,
            token: String::new(),
            now,
            path,
        };
        f.token = f
            .ok(connect())
            .resume_token
            .expect("Disposable local fixture resume credential");
        // Explicit initial stationary source-fixture placement, before all commands.
        let mut w = f.world();
        let p = w.pilots.get_mut("Pilot").unwrap();
        p.view.location = Location::Station;
        p.view.station_avatar = None;
        let flight = p.view.flight.as_mut().unwrap();
        flight.position = [0.; 3];
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
        f.write(&w);
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
    fn ok(&mut self, r: Request) -> Response {
        let response = self.server.handle_at(&mut self.session, r, self.now);
        assert!(response.ok, "{}", response.message);
        response
    }
    fn step(&mut self) {
        self.now += 100;
        if let Err(e) = self.server.tick_at(self.now) {
            let w = self.world();
            let s = state(&w, "Pilot").unwrap();
            panic!(
                "Tick {}: {e:#}; operations {:?}",
                self.now, s.inventory.operations
            )
        }
    }
    fn request(&mut self, r: CarrierInventoryRequest) -> Response {
        self.ok(Request::CarrierInventory { request: r })
    }
    fn coarse_step(&mut self) {
        self.now += 1000;
        self.server.tick_at(self.now).unwrap();
    }
    fn snapshot_valid(&mut self) {
        let r = self.ok(Request::Snapshot);
        if let Ok(directory) = std::env::var("ARKLINE_CARRIER_PUBLIC_SAMPLES") {
            let directory = std::path::PathBuf::from(directory);
            assert!(
                directory.is_dir(),
                "Explicit fresh sample folder must exist"
            );
            let path = directory.join(format!("response-{}.json", self.now));
            if !path.exists() {
                std::fs::write(path, serde_json::to_vec(&r).unwrap()).unwrap();
            }
        }
        let p = &r.snapshot.as_ref().unwrap().pilots[0];
        let public = p
            .carrier_inventory
            .as_ref()
            .expect("Actual carrier projection");
        assert_eq!(
            public.validate_projection(p),
            Ok(CarrierInventoryAdmission::StationaryFixture)
        );
        assert_eq!(
            r.assets.as_ref().unwrap().carrier_inventory.as_ref(),
            Some(&public.inventory)
        );
        assert!(public.validated_next_ids(&public.inventory, p).is_none());
    }
    fn reopen(&mut self) {
        self.server.disconnect(&mut self.session);
        let replacement = Server::open(":memory:").unwrap();
        drop(std::mem::replace(&mut self.server, replacement));
        self.server = Server::open_editor_realm_with_scene(
            &self.path,
            default_pioneer_room_layout(),
            SceneLayoutRevision::DockingV1,
        )
        .unwrap();
        self.now = self.server.editor_time.unwrap();
        let mut request = connect();
        if let Request::Connect { resume_token, .. } = &mut request {
            *resume_token = Some(self.token.clone());
        }
        self.ok(request);
    }
    fn binding(&self) -> CarrierInventoryBinding {
        state(&self.world(), "Pilot")
            .unwrap()
            .inventory
            .binding
            .clone()
    }
}
#[test]
fn actual_source_single_launch_keeps_identity_and_clears_the_next_exit() {
    let mut f = Fixture::new();
    let w = f.world();
    let before = state(&w, "Pilot").unwrap().inventory.roster.clone();
    let drone_id = before[0].drone_id;
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id,
        action: CarrierInventoryAction::Launch,
    });
    let mut complete = false;
    for _ in 0..1500 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.inventory.operation(1).unwrap().phase == CarrierOperationPhase::Completed
            && s.departures.is_empty()
        {
            complete = true;
            break;
        }
    }
    assert!(complete, "launch/departure did not finish");
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.roster, before);
    assert_eq!(s.inventory.entries.len(), 20);
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 1);
    assert!(matches!(
        s.inventory.entry(drone_id).unwrap().location,
        CarrierDroneLocation::Deployed
    ));
    assert_eq!(s.inventory.entry(drone_id).unwrap().hit_points, 100);
    eprintln!(
        "Single source launch and actual outward clearance: {} simulated ms",
        f.now - s.inventory.initialized_at_ms
    );
}
#[test]
fn one_deploy_all_drains_twenty_actual_source_identities_without_wave_commands() {
    let mut f = Fixture::new();
    let w = f.world();
    let original = state(&w, "Pilot").unwrap().inventory.roster.clone();
    let initial = f.now;
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut done = false;
    let mut max_launch = 0;
    let mut saw_loading = false;
    for _ in 0..18000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.roster, original);
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert_eq!(w.pilots["Pilot"].view.ore, 0);
        assert_eq!(s.ore.ore_units(), Some(0));
        max_launch = max_launch.max(
            s.inventory
                .entries
                .iter()
                .filter(|e| matches!(e.location, CarrierDroneLocation::Launching { .. }))
                .count(),
        );
        saw_loading |= s
            .inventory
            .entries
            .iter()
            .any(|e| matches!(e.location, CarrierDroneLocation::Loading { .. }));
        if s.inventory.deployments[0].phase == CarrierDeploymentPhase::Completed {
            done = true;
            break;
        }
        if (f.now - initial) % 10000 == 0 {
            eprintln!(
                "Deploy All {} ms: {} active, {} runs, {} departures",
                f.now - initial,
                w.pilots["Pilot"].view.drones.len(),
                s.runs.len(),
                s.departures.len()
            );
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(done, "Deploy All blocked: {:?}", s.inventory.deployments);
    assert!(saw_loading);
    assert_eq!(max_launch, 10);
    assert!(s.runs.is_empty() && s.departures.is_empty());
    assert!(s
        .inventory
        .entries
        .iter()
        .all(|e| matches!(e.location, CarrierDroneLocation::Deployed) && e.hit_points == 100));
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 20);
    assert_eq!(s.inventory.deployments.len(), 1);
    assert_eq!(s.inventory.operations.len(), 30);
    eprintln!(
        "ONE Deploy All completed all20: {} simulated ms; no manual second command",
        f.now - initial
    );
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
    }
}

#[test]
#[ignore = "explicit full authoritative 20-drone documentary export"]
fn export_actual_one_deploy_all_twenty_journey() {
    use sha2::{Digest, Sha256};
    let output = std::path::PathBuf::from(
        std::env::var("ARKLINE_CARRIER_EXPORT").expect("explicit fresh output directory"),
    );
    assert!(!output.exists(), "Do not overwrite retained evidence");
    std::fs::create_dir_all(&output).unwrap();
    let producer = std::env::var("ARKLINE_PRODUCER_COMMIT").expect("exact frozen producer commit");
    assert_eq!(producer.len(), 40);
    let binary_sha = format!(
        "{:x}",
        Sha256::digest(std::fs::read(std::env::current_exe().unwrap()).unwrap())
    );
    let mut f = Fixture::new();
    let initial = f.now;
    let roster = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    let mut frames = Vec::new();
    let mut complete_at = None;
    for index in 0..18000 {
        if index > 0 {
            f.step();
        }
        if index == 10 {
            f.request(CarrierInventoryRequest::DeployAll {
                binding: f.binding(),
                batch_id: 1,
            });
        }
        let response = f.ok(Request::Snapshot);
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.roster, roster);
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert_eq!(s.ore.ore_units(), Some(0));
        assert_eq!(w.pilots["Pilot"].view.ore, 0);
        assert_eq!(w.pilots["Pilot"].view.credits, STARTER_CREDITS);
        let mut motions = Vec::new();
        for r in s.runs.values() {
            let drone_pose = r
                .confirmed
                .poses
                .iter()
                .find_map(|(k, p)| (*k == CarrierPoseKey::Drone).then_some(*p));
            let cassettes = r
                .confirmed
                .poses
                .iter()
                .filter_map(|(k, p)| {
                    if let CarrierPoseKey::Cassette { cassette_id } = k {
                        Some((*cassette_id, *p))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let machinery = r
                .confirmed
                .poses
                .iter()
                .filter_map(|(k, p)| {
                    if let CarrierPoseKey::Mechanism { semantic_id } = k {
                        Some((semantic_id.clone(), *p))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            motions.push(serde_json::json!({"operation_id":r.operation_id,"drone_id":r.drone_id,"confirmed_at_ms":r.confirmed_at_ms,"held":r.held,"drone_pose":drone_pose,"cassette_poses":cassettes,"machinery":machinery}));
        }
        for d in s.departures.values() {
            motions.push(serde_json::json!({"operation_id":d.operation_id,"drone_id":d.drone_id,"confirmed_at_ms":d.confirmed_at_ms,"held":d.held,"drone_pose":d.confirmed,"cassette_poses":[],"machinery":[]}));
        }
        let row = serde_json::json!({"format_version":1,"producer_commit":producer,"sample_time_ms":f.now,"pilot":"Pilot","response":response,"parent":parent(&w,"Pilot").unwrap(),"binding":s.inventory.binding,"inventory":s.inventory,"ore":s.ore,"drone_poses":s.drone_poses.iter().map(|(i,p)|(*i,*p)).collect::<Vec<_>>(),"cassette_poses":s.cassette_poses.iter().map(|(i,p)|(*i,*p)).collect::<Vec<_>>(),"machinery":s.machinery.iter().collect::<Vec<_>>(),"motions":motions});
        let bytes = serde_json::to_vec(&row).unwrap();
        let file = format!("response-{index:05}.json");
        let sha = format!("{:x}", Sha256::digest(&bytes));
        std::fs::write(output.join(&file), bytes).unwrap();
        frames.push(serde_json::json!({"file":file,"index":index,"elapsed_ms":f.now-initial,"response_sha256":sha}));
        if s.inventory
            .deployments
            .last()
            .is_some_and(|b| b.phase == CarrierDeploymentPhase::Completed)
            && s.departures.is_empty()
        {
            complete_at.get_or_insert(f.now);
        }
        if index % 100 == 0 {
            eprintln!(
                "EXPORT {} ms / {} frames: {} active, {} mechanical runs",
                f.now - initial,
                index + 1,
                w.pilots["Pilot"].view.drones.len(),
                s.runs.len()
            );
        }
        if complete_at.is_some_and(|at| f.now >= at + 2000) {
            break;
        }
    }
    let complete_at =
        complete_at.expect("Actual deployment did not complete within observation ceiling");
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(s
        .inventory
        .entries
        .iter()
        .all(|e| matches!(e.location, CarrierDroneLocation::Deployed) && e.hit_points == 100));
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 20);
    let receipt = serde_json::json!({"format_version":1,"producer_commit":producer,"producer_binary_sha256":binary_sha,"pilot":"Pilot","fixture_conditions":"Exact compiled stationary source assembly; initial carrier at origin before any operation. Explicit finite all-Mining loadout; no asteroid, drone motion, damage, ore or time-rate injection. Normal travel/source admission remains disabled.","command":"one DeployAll batch_id=1; no later individual Stage/Launch command","registration":s.inventory.binding.registration,"initial_roster":roster,"initial_stored":20,"final_deployed":20,"final_stored":0,"destroyed":0,"final_ore":0,"deploy_all_completed_elapsed_ms":complete_at-initial,"duration_ms":f.now-initial,"frame_count":frames.len(),"cadence_ms":100,"operations":s.inventory.operations,"deployment":s.inventory.deployments,"frames":frames});
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn deployment_hold_cancel_retries_and_sql_rollback_keep_exact_finite_work() {
    let mut f = Fixture::new();
    let initial = f.world();
    let binding = f.binding();
    f.server.db.execute_batch("CREATE TRIGGER test_carrier_rollback BEFORE UPDATE OF json ON world BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    let denied = f.server.handle_at(
        &mut f.session,
        Request::CarrierInventory {
            request: CarrierInventoryRequest::DeployAll {
                binding: binding.clone(),
                batch_id: 1,
            },
        },
        f.now,
    );
    assert!(!denied.ok);
    assert_eq!(
        serde_json::to_value(f.world()).unwrap(),
        serde_json::to_value(&initial).unwrap()
    );
    f.server
        .db
        .execute_batch("DROP TRIGGER test_carrier_rollback;")
        .unwrap();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: binding.clone(),
        batch_id: 1,
    });
    f.request(CarrierInventoryRequest::HoldDeployment {
        binding: binding.clone(),
        batch_id: 1,
    });
    let before = f.world();
    let s = state(&before, "Pilot").unwrap();
    let pending = s.inventory.deployments[0]
        .members
        .iter()
        .find(|m| m.stage_operation_id.is_none() && m.launch_operation_id.is_none())
        .unwrap()
        .drone_id;
    let id = s.inventory.next_operation_id().unwrap();
    let refused = f.server.handle_at(
        &mut f.session,
        Request::CarrierInventory {
            request: CarrierInventoryRequest::Begin {
                binding: binding.clone(),
                operation_id: id,
                drone_id: pending,
                action: CarrierInventoryAction::Stage { berth_id: 0 },
            },
        },
        f.now,
    );
    assert!(!refused.ok);
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, s.inventory);
    f.request(CarrierInventoryRequest::CancelDeploymentRemaining {
        binding: binding.clone(),
        batch_id: 1,
    });
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.inventory.deployments[0].phase,
        CarrierDeploymentPhase::Cancelling
    );
    assert!(s.inventory.deployments[0].held);
    assert!(s.runs.values().all(|r| r.held));
    f.request(CarrierInventoryRequest::ResumeDeployment {
        binding: binding.clone(),
        batch_id: 1,
    });
    for _ in 0..500 {
        f.step();
        if state(&f.world(), "Pilot").unwrap().inventory.deployments[0].phase
            == CarrierDeploymentPhase::Cancelled
        {
            break;
        }
    }
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().inventory.deployments[0].phase,
        CarrierDeploymentPhase::Cancelled
    );
    f.request(CarrierInventoryRequest::DeployAll {
        binding: binding.clone(),
        batch_id: 2,
    });
    // Stabilize the newly accepted route at this same authority timestamp.
    // Old receipts may not take control of that independently advancing work.
    f.ok(Request::Snapshot);
    let before = state(&f.world(), "Pilot").unwrap().clone();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: binding.clone(),
        batch_id: 1,
    });
    f.request(CarrierInventoryRequest::HoldDeployment {
        binding: binding.clone(),
        batch_id: 1,
    });
    f.request(CarrierInventoryRequest::CancelDeploymentRemaining {
        binding,
        batch_id: 1,
    });
    let after = f.world();
    let after = state(&after, "Pilot").unwrap();
    assert_eq!(after.inventory, before.inventory);
    assert_eq!(after.runs, before.runs);
}

#[test]
fn malformed_source_runtime_and_departure_refuse_before_any_database_write() {
    let mut f = Fixture::new();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::Launch,
    });
    let live = f.world();
    let mut cases = Vec::new();
    let mut bad = live.clone();
    bad.accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .runs
        .clear();
    cases.push(bad);
    let mut bad = live.clone();
    bad.accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .inventory
        .reservations
        .retain(|r| !matches!(r.resource, CarrierResource::Cassette { .. }));
    cases.push(bad);
    let mut bad = live.clone();
    bad.carrier_inventory_registration = None;
    cases.push(bad);
    let mut bad = live.clone();
    bad.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .flight
        .as_mut()
        .unwrap()
        .position[0] += 1.;
    cases.push(bad);
    for _ in 0..200 {
        f.step();
        if !state(&f.world(), "Pilot").unwrap().departures.is_empty() {
            break;
        }
    }
    let departure = f.world();
    assert!(!state(&departure, "Pilot").unwrap().departures.is_empty());
    let mut bad = departure.clone();
    let s = bad
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap();
    let mut d = s.departures.remove(&1).unwrap();
    d.drone_id = 999999;
    s.departures.insert(d.drone_id, d);
    cases.push(bad);
    let mut bad = departure.clone();
    let d = bad
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap()
        .departures
        .get_mut(&1)
        .unwrap();
    d.started_at_ms = u64::MAX;
    d.confirmed_at_ms = u64::MAX;
    cases.push(bad);
    let ledger = f
        .server
        .db
        .prepare("SELECT sequence,json FROM ledger ORDER BY sequence")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for bad in cases {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE world(id INTEGER PRIMARY KEY,json TEXT NOT NULL);")
            .unwrap();
        db.execute_batch("CREATE TABLE ledger(sequence INTEGER PRIMARY KEY,json TEXT NOT NULL);")
            .unwrap();
        for (sequence, json) in &ledger {
            db.execute(
                "INSERT INTO ledger VALUES(?1,?2)",
                rusqlite::params![sequence, json],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO world VALUES(1,?1)",
            [serde_json::to_string(&live).unwrap()],
        )
        .unwrap();
        preflight_existing(&db).expect("Unchanged source-bound positive control must open");
        db.execute("DELETE FROM world", []).unwrap();
        let bytes = serde_json::to_string(&bad).unwrap();
        db.execute("INSERT INTO world VALUES(1,?1)", [&bytes])
            .unwrap();
        let changes = db.total_changes();
        assert!(preflight_existing(&db).is_err());
        assert_eq!(changes, db.total_changes());
        assert_eq!(
            db.query_row("SELECT json FROM world", [], |r| r.get::<_, String>(0))
                .unwrap(),
            bytes
        );
    }
}

#[test]
fn one_blocked_exit_does_not_stop_the_other_bank_and_resume_is_explicit() {
    let mut f = Fixture::new();
    let def = geometry::stationary_fixture_definition().unwrap();
    let starboard = state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .entries
        .iter()
        .filter_map(|e| {
            if let CarrierDroneLocation::Ready { berth_id } = e.location {
                (def.topology.berth(berth_id).unwrap().side == CarrierSide::Starboard)
                    .then_some(e.drone_id)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(starboard.len(), 5);
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut w = f.world();
    let obstacle = w
        .asteroids
        .iter_mut()
        .find(|a| a.system_id == HOME_SYSTEM_ID)
        .unwrap();
    let old = obstacle.position;
    let asteroid_id = obstacle.id;
    // Explicit obstacle fixture: an existing finite catalog asteroid intrudes
    // into the port exit region after acceptance. Its near face is at the
    // port clear pose, leaving the opposite bank untouched. No drone pose,
    // ore yield, radius, damage or rate is injected.
    let clear = def
        .berths
        .iter()
        .find(|b| b.berth_id == 0)
        .unwrap()
        .clear_pose;
    let outward = clear.transform_vector([obstacle.radius, 0., 0.]).unwrap();
    obstacle.position = std::array::from_fn(|i| clear.position_m[i] + outward[i]);
    f.write(&w);
    for _ in 0..300 {
        f.step();
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(s
        .inventory
        .operations
        .iter()
        .any(|o| o.drone_id == 1 && o.phase == CarrierOperationPhase::Held));
    assert!(s
        .inventory
        .entries
        .iter()
        .filter(|e| starboard.contains(&e.drone_id))
        .all(|e| matches!(e.location, CarrierDroneLocation::Deployed)));
    let before = s.drone_poses[&1];
    for _ in 0..10 {
        f.step();
    }
    assert_eq!(state(&f.world(), "Pilot").unwrap().drone_poses[&1], before);
    let mut w = f.world();
    w.asteroids
        .iter_mut()
        .find(|a| a.id == asteroid_id)
        .unwrap()
        .position = old;
    f.write(&w);
    f.request(CarrierInventoryRequest::ResumeDeployment {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..500 {
        f.step();
        if matches!(
            state(&f.world(), "Pilot")
                .unwrap()
                .inventory
                .entry(1)
                .unwrap()
                .location,
            CarrierDroneLocation::Deployed
        ) {
            break;
        }
    }
    assert!(matches!(
        state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .entry(1)
            .unwrap()
            .location,
        CarrierDroneLocation::Deployed
    ));
}

#[test]
fn actual_departure_restart_and_batch_owned_controls_preserve_physical_ownership() {
    let mut f = Fixture::new();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..200 {
        f.step();
        f.snapshot_valid();
        if !state(&f.world(), "Pilot").unwrap().departures.is_empty() {
            break;
        }
    }
    let before = f.world();
    let s = state(&before, "Pilot").unwrap();
    let pose = s.drone_poses[&1];
    let target = s.departures[&1].target;
    assert_eq!(
        s.inventory.operation(1).unwrap().phase,
        CarrierOperationPhase::Completed
    );
    f.reopen();
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(s.departures[&1].held);
    assert_eq!(s.drone_poses[&1], pose);
    assert_eq!(s.departures[&1].target, target);
    for _ in 0..3 {
        f.coarse_step();
        f.snapshot_valid();
    }
    assert_eq!(state(&f.world(), "Pilot").unwrap().drone_poses[&1], pose);
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: 1,
    });
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: 1,
    });
    for _ in 0..200 {
        f.step();
        f.snapshot_valid();
        if state(&f.world(), "Pilot").unwrap().departures.is_empty() {
            break;
        }
    }
    assert!(state(&f.world(), "Pilot").unwrap().departures.is_empty());
    assert_eq!(state(&f.world(), "Pilot").unwrap().drone_poses[&1], target);
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..200 {
        f.step();
        if !state(&f.world(), "Pilot").unwrap().departures.is_empty() {
            break;
        }
    }
    f.request(CarrierInventoryRequest::HoldDeployment {
        binding: f.binding(),
        batch_id: 1,
    });
    let before = f.world();
    let s = state(&before, "Pilot").unwrap();
    let child = s.departures.values().next().unwrap().operation_id;
    for request in [
        CarrierInventoryRequest::Resume {
            binding: f.binding(),
            operation_id: child,
        },
        CarrierInventoryRequest::Hold {
            binding: f.binding(),
            operation_id: child,
        },
    ] {
        let r = f
            .server
            .handle_at(&mut f.session, Request::CarrierInventory { request }, f.now);
        assert!(!r.ok);
    }
    let w = f.world();
    let s2 = state(&w, "Pilot").unwrap();
    assert_eq!(s2.inventory, s.inventory);
    assert_eq!(
        serde_json::to_value(&s2.departures).unwrap(),
        serde_json::to_value(&s.departures).unwrap()
    );
    f.snapshot_valid();
}

#[test]
fn actual_loaded_cassette_close_reopen_and_resume_preserve_every_owned_root() {
    let mut f = Fixture::new();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..400 {
        f.step();
        if state(&f.world(), "Pilot").unwrap().runs.is_empty()
            && state(&f.world(), "Pilot").unwrap().departures.is_empty()
        {
            break;
        }
    }
    let w = f.world();
    let original = state(&w, "Pilot").unwrap().inventory.roster.clone();
    let loaded = state(&w, "Pilot")
        .unwrap()
        .inventory
        .entries
        .iter()
        .find(|e| e.location == CarrierDroneLocation::Protected { slot_id: 0 })
        .unwrap()
        .drone_id;
    let cassette = state(&w, "Pilot")
        .unwrap()
        .inventory
        .cassettes
        .iter()
        .find(|c| c.drone_id == Some(loaded))
        .unwrap()
        .cassette_id;
    let original_pose = state(&w, "Pilot").unwrap().cassette_poses[&cassette];
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 2,
        drone_id: loaded,
        action: CarrierInventoryAction::Stage { berth_id: 0 },
    });
    // Explicit normal 1-second authority cadence, below the 2-second gap guard;
    // no catch-up or duration changes. Full documentary baseline remains100ms.
    for _ in 0..500 {
        f.coarse_step();
        f.snapshot_valid();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.cassette_poses[&cassette] != original_pose {
            break;
        }
    }
    let before = f.world();
    let b = state(&before, "Pilot").unwrap();
    assert_ne!(b.cassette_poses[&cassette], original_pose);
    assert!(matches!(
        b.inventory.entry(loaded).unwrap().location,
        CarrierDroneLocation::Loading { .. }
    ));
    f.reopen();
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.roster, original);
    assert_eq!(s.cassette_poses, b.cassette_poses);
    assert_eq!(s.drone_poses, b.drone_poses);
    assert_eq!(s.machinery, b.machinery);
    assert!(s.runs[&2].held);
    for _ in 0..3 {
        f.coarse_step();
        f.snapshot_valid();
    }
    assert_eq!(state(&f.world(), "Pilot").unwrap().machinery, b.machinery);
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: 2,
    });
    for _ in 0..700 {
        f.coarse_step();
        f.snapshot_valid();
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .operation(2)
            .unwrap()
            .phase
            == CarrierOperationPhase::Completed
        {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.roster, original);
    assert_eq!(
        s.inventory.operation(2).unwrap().phase,
        CarrierOperationPhase::Completed
    );
    assert_eq!(
        s.inventory.entry(loaded).unwrap().location,
        CarrierDroneLocation::Ready { berth_id: 0 }
    );
    assert!(s.runs.is_empty() && s.inventory.reservations.is_empty());
    assert_eq!(s.drone_poses[&loaded], s.cassette_poses[&cassette]);
}

fn staged_1702_handoff() -> Fixture {
    let mut f = Fixture::new_1702();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..60 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.runs.is_empty() && s.departures.is_empty() {
            break;
        }
    }
    assert!(state(&f.world(), "Pilot").unwrap().departures.is_empty());
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 2,
        drone_id: 2,
        action: CarrierInventoryAction::Stage { berth_id: 0 },
    });
    for _ in 0..300 {
        f.coarse_step();
        f.snapshot_valid();
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .operation(2)
            .unwrap()
            .released_loaded_owner()
        {
            return f;
        }
    }
    panic!("Actual Stage head never reached source-confirmed handoff");
}

#[test]
fn actual_1702_held_empty_tail_and_launch_reopen_without_overwriting_released_drone() {
    let mut f = staged_1702_handoff();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    let handoff = s
        .inventory
        .operation(2)
        .unwrap()
        .stage
        .as_ref()
        .unwrap()
        .loaded_handoff_at_ms;
    assert_eq!(
        s.inventory.entry(2).unwrap().location,
        CarrierDroneLocation::Ready { berth_id: 0 }
    );
    assert_eq!(
        s.inventory
            .operation(2)
            .unwrap()
            .stage
            .as_ref()
            .unwrap()
            .completed_legs,
        2
    );
    assert_eq!(s.inventory.reservations.len(), 4);
    assert!(!s.runs[&2].kind.writes_drone());
    let tail_keys = s.runs[&2].confirmed.poses.clone();
    assert_handoff_corruption_refuses(&f, &w);
    f.request(CarrierInventoryRequest::Hold {
        binding: f.binding(),
        operation_id: 2,
    });
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 3,
        drone_id: 2,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..4 {
        f.coarse_step();
        f.snapshot_valid();
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(
        s.runs.contains_key(&3),
        "Released Ready body did not enter actual Launch"
    );
    assert_eq!(s.runs[&2].confirmed.poses, tail_keys);
    assert_ne!(s.drone_poses[&2], s.cassette_poses[&10]);
    let before = s.clone();
    f.reopen();
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.drone_poses, before.drone_poses);
    assert_eq!(s.cassette_poses, before.cassette_poses);
    assert_eq!(s.machinery, before.machinery);
    assert!(s.runs[&2].held && s.runs[&3].held);
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: 3,
    });
    for _ in 0..60 {
        f.coarse_step();
        f.snapshot_valid();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.runs[&2].confirmed.poses, tail_keys);
        if !s.runs.contains_key(&3) && s.departures.is_empty() {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.inventory.entry(2).unwrap().location,
        CarrierDroneLocation::Deployed
    );
    assert!(!s.runs.contains_key(&3) && s.departures.is_empty());
    let deployed = s.drone_poses[&2];
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: 2,
    });
    for _ in 0..200 {
        f.coarse_step();
        f.snapshot_valid();
        if state(&f.world(), "Pilot").unwrap().runs.is_empty() {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(s.runs.is_empty() && s.inventory.reservations.is_empty());
    assert_eq!(s.drone_poses[&2], deployed);
    assert_eq!(
        s.inventory.entry(2).unwrap().location,
        CarrierDroneLocation::Deployed
    );
    assert_eq!(
        s.inventory
            .operation(2)
            .unwrap()
            .stage
            .as_ref()
            .unwrap()
            .loaded_handoff_at_ms,
        handoff
    );
    assert_eq!(
        s.inventory
            .operation(2)
            .unwrap()
            .stage
            .as_ref()
            .unwrap()
            .completed_legs,
        3
    );
    assert_eq!(s.inventory.roster.len(), 20);
    assert_eq!(s.inventory.cassettes.len(), 20);
}

fn assert_handoff_corruption_refuses(f: &Fixture, valid: &World) {
    let ledger = f
        .server
        .db
        .prepare("SELECT sequence,json FROM ledger ORDER BY sequence")
        .unwrap()
        .query_map([], |r| Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    for mutation in 0..6 {
        let mut bad = valid.clone();
        let s = bad
            .accounts
            .get_mut("Pilot")
            .unwrap()
            .carrier_inventory
            .as_mut()
            .unwrap();
        match mutation {
            0 => {
                s.inventory.operations[1]
                    .stage
                    .as_mut()
                    .unwrap()
                    .loaded_handoff_at_ms = None
            }
            1 => {
                s.runs.remove(&2);
            }
            2 => {
                s.runs
                    .get_mut(&2)
                    .unwrap()
                    .confirmed
                    .poses
                    .push((CarrierPoseKey::Drone, s.drone_poses[&2]));
            }
            3 => s.inventory.reservations.push(CarrierResourceReservation {
                resource: CarrierResource::Cassette { cassette_id: 10 },
                operation_id: 2,
                drone_id: 2,
            }),
            4 => s.inventory.operations[1].phase = CarrierOperationPhase::Loading,
            _ => {
                s.inventory.operations[1]
                    .stage
                    .as_mut()
                    .unwrap()
                    .loaded_handoff_at_ms = Some(u64::MAX)
            }
        }
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE world(id INTEGER PRIMARY KEY,json TEXT NOT NULL); CREATE TABLE ledger(sequence INTEGER PRIMARY KEY,json TEXT NOT NULL);").unwrap();
        for (sequence, json) in &ledger {
            db.execute(
                "INSERT INTO ledger VALUES(?1,?2)",
                rusqlite::params![sequence, json],
            )
            .unwrap();
        }
        db.execute(
            "INSERT INTO world VALUES(1,?1)",
            [serde_json::to_string(valid).unwrap()],
        )
        .unwrap();
        preflight_existing(&db).unwrap();
        let json = serde_json::to_string(&bad).unwrap();
        db.execute("UPDATE world SET json=?1", [&json]).unwrap();
        let changes = db.total_changes();
        assert!(
            preflight_existing(&db).is_err(),
            "handoff mutation {mutation} accepted"
        );
        assert_eq!(changes, db.total_changes());
        assert_eq!(
            db.query_row("SELECT json FROM world", [], |r| r.get::<_, String>(0))
                .unwrap(),
            json
        );
    }
}

#[test]
fn actual_1702_injected_launch_death_retains_only_its_cleanup_and_empty_tail() {
    let mut f = staged_1702_handoff();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 3,
        drone_id: 2,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..3 {
        f.coarse_step();
    }
    // Explicit bounded post-combat fault injection. This tests authority death
    // ordering/conservation, not a newly reachable protected-bay damage feature.
    let mut w = f.world();
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == 2)
        .unwrap()
        .hit_points = 0;
    let events = finish(&mut w, f.now).unwrap();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        EconomicEvent::CarrierInventory {
            record: CarrierInventoryLedgerEvent::Destroyed {
                drone_id: 2,
                lost_units: 0,
                ..
            }
        }
    ));
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .retain(|d| d.hit_points > 0);
    drones::sync(w.pilots.get_mut("Pilot").unwrap());
    validate(&w).unwrap();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.inventory.operation(2).unwrap().phase,
        CarrierOperationPhase::Securing
    );
    assert_eq!(
        s.inventory.operation(3).unwrap().phase,
        CarrierOperationPhase::Destroyed
    );
    assert_eq!(
        s.inventory
            .cleanups
            .iter()
            .map(|c| c.operation_id)
            .collect::<Vec<_>>(),
        vec![3]
    );
    assert!(
        finish(&mut w, f.now).unwrap().is_empty(),
        "Death must not emit twice"
    );
    {
        let tx = f.server.db.transaction().unwrap();
        let mut entries = crate::load_entries(&tx, false).unwrap();
        for event in events {
            crate::append_event(
                &tx,
                &mut entries,
                event,
                f.now,
                CARRIER_INVENTORY_RULESET_VERSION,
            )
            .unwrap();
        }
        tx.execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    f.reopen();
    f.snapshot_valid();
    for id in [2, 3] {
        f.request(CarrierInventoryRequest::Resume {
            binding: f.binding(),
            operation_id: id,
        });
    }
    for _ in 0..200 {
        f.coarse_step();
        f.snapshot_valid();
        if state(&f.world(), "Pilot").unwrap().runs.is_empty() {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(
        s.runs.is_empty() && s.inventory.cleanups.is_empty() && s.inventory.reservations.is_empty()
    );
    assert!(matches!(
        s.inventory.entry(2).unwrap().location,
        CarrierDroneLocation::Destroyed { .. }
    ));
    assert_eq!(
        s.inventory.operation(2).unwrap().phase,
        CarrierOperationPhase::Completed
    );
    assert_eq!(
        s.inventory.operation(3).unwrap().phase,
        CarrierOperationPhase::Destroyed
    );
    assert_eq!(s.inventory.entries.len(), 20);
    assert_eq!(s.inventory.cassettes.len(), 20);
}

#[test]
fn actual_1702_cancel_remaining_after_handoff_keeps_tail_until_explicit_resume() {
    let mut f = Fixture::new_1702();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..350 {
        f.coarse_step();
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .operations
            .iter()
            .any(|o| o.released_loaded_owner())
        {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(s
        .inventory
        .operations
        .iter()
        .any(|o| o.released_loaded_owner()));
    let launched = s
        .inventory
        .operations
        .iter()
        .filter(|o| o.action == CarrierInventoryAction::Launch)
        .count();
    // Tick completion dispatches newly eligible work atomically. The two
    // loaded handoffs therefore already have Launch children when this tick is
    // observed; cancellation must finish admitted children, not erase them.
    assert_eq!(launched, 12);
    let handed_off: BTreeSet<_> = s
        .inventory
        .operations
        .iter()
        .filter(|o| o.released_loaded_owner())
        .map(|o| o.drone_id)
        .collect();
    assert_eq!(handed_off.len(), 2);
    let already_deployed: BTreeSet<_> = s
        .inventory
        .entries
        .iter()
        .filter(|e| e.location == CarrierDroneLocation::Deployed)
        .map(|e| e.drone_id)
        .collect();
    assert_eq!(already_deployed.len(), 10);
    let admitted_launches: BTreeSet<_> = s
        .inventory
        .entries
        .iter()
        .filter(|e| matches!(e.location, CarrierDroneLocation::Launching { .. }))
        .map(|e| e.drone_id)
        .collect();
    assert_eq!(admitted_launches, handed_off);
    let expected_deployed: BTreeSet<_> = already_deployed
        .union(&admitted_launches)
        .copied()
        .collect();
    f.request(CarrierInventoryRequest::HoldDeployment {
        binding: f.binding(),
        batch_id: 1,
    });
    f.request(CarrierInventoryRequest::CancelDeploymentRemaining {
        binding: f.binding(),
        batch_id: 1,
    });
    let held = state(&f.world(), "Pilot").unwrap().clone();
    assert!(held.inventory.deployments[0].held);
    assert_eq!(
        held.inventory.deployments[0].phase,
        CarrierDeploymentPhase::Cancelling
    );
    assert!(!held.runs.is_empty());
    f.reopen();
    f.snapshot_valid();
    for _ in 0..3 {
        f.coarse_step();
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.machinery, held.machinery);
    assert_eq!(s.cassette_poses, held.cassette_poses);
    f.request(CarrierInventoryRequest::ResumeDeployment {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..250 {
        f.coarse_step();
        if !state(&f.world(), "Pilot").unwrap().inventory.deployments[0]
            .phase
            .active()
        {
            break;
        }
    }
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.inventory.deployments[0].phase,
        CarrierDeploymentPhase::Cancelled
    );
    assert!(s.runs.is_empty() && s.inventory.reservations.is_empty());
    assert_eq!(
        s.inventory
            .operations
            .iter()
            .filter(|o| o.action == CarrierInventoryAction::Launch)
            .count(),
        launched
    );
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| e.location == CarrierDroneLocation::Deployed)
            .map(|e| e.drone_id)
            .collect::<BTreeSet<_>>(),
        expected_deployed
    );
    assert_eq!(
        s.inventory.deployments[0]
            .members
            .iter()
            .filter(|m| m.cancelled)
            .count(),
        8,
        "Only the untouched reserve remainder is cancelled"
    );
    assert_eq!(s.inventory.entries.len(), 20);
    let before = s.inventory.clone();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, before);
}

#[test]
fn actual_1702_one_command_pipelines_all_twenty_with_released_tail_ownership() {
    let mut f = Fixture::new_1702();
    let w = f.world();
    let original = state(&w, "Pilot").unwrap().inventory.roster.clone();
    let initial = f.now;
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    // The finite automatic queue must also finish while its owner remains in
    // the source-preserved enclosed crew core, without forcing a pilot reseat.
    let w = f.world();
    let account = &w.accounts["Pilot"];
    let crew_binding = docked_traversal_binding(
        account.vessel_id(ShipClass::Pioneer).unwrap().into(),
        &default_pioneer_room_layout(),
        w.scene_layout.definition().fleet_layout,
        account.fleet_hangar,
    );
    f.ok(Request::DockedTraversal {
        request: DockedTraversalRequest::LeaveSeat {
            binding: crew_binding,
        },
    });
    let mut done = false;
    let mut overlap = false;
    let mut max_launch = 0;
    let mut saw_loading = false;
    for _ in 0..1800 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        overlap |= s.runs.values().any(|tail| {
            !tail.kind.writes_drone()
                && s.runs
                    .values()
                    .any(|launch| launch.drone_id == tail.drone_id && launch.kind.writes_drone())
        });
        assert_eq!(s.inventory.roster, original);
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert_eq!(w.pilots["Pilot"].view.ore, 0);
        assert_eq!(s.ore.ore_units(), Some(0));
        max_launch = max_launch.max(
            s.inventory
                .entries
                .iter()
                .filter(|e| matches!(e.location, CarrierDroneLocation::Launching { .. }))
                .count(),
        );
        saw_loading |= s
            .inventory
            .entries
            .iter()
            .any(|e| matches!(e.location, CarrierDroneLocation::Loading { .. }));
        if s.inventory.deployments[0].phase == CarrierDeploymentPhase::Completed {
            done = true;
            break;
        }
        if (f.now - initial) % 10000 == 0 {
            eprintln!(
                "Deploy All {} ms: {} active, {} runs, {} departures",
                f.now - initial,
                w.pilots["Pilot"].view.drones.len(),
                s.runs.len(),
                s.departures.len()
            );
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(done, "Deploy All blocked: {:?}", s.inventory.deployments);
    assert!(
        saw_loading && overlap,
        "No actual same-ID Launch overlapped the retained empty tail"
    );
    assert_eq!(max_launch, 10);
    assert!(s.runs.is_empty() && s.departures.is_empty());
    assert!(s
        .inventory
        .entries
        .iter()
        .all(|e| matches!(e.location, CarrierDroneLocation::Deployed) && e.hit_points == 100));
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 20);
    assert_eq!(s.inventory.deployments.len(), 1);
    assert_eq!(s.inventory.operations.len(), 30);
    eprintln!(
        "ONE Deploy All completed all20: {} simulated ms; no manual second command",
        f.now - initial
    );
    assert!(f.world().pilots["Pilot"]
        .view
        .station_avatar
        .as_ref()
        .is_some_and(|a| a
            .docked_traversal
            .as_ref()
            .is_some_and(|t| t.seat_transition.is_none())));
}

#[test]
fn actual_1702_zero_payload_return_approaches_reopens_and_captures_same_identity() {
    let mut f = Fixture::new_1702();
    let id = state(&f.world(), "Pilot").unwrap().inventory.roster[0].drone_id;
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: id,
        action: CarrierInventoryAction::Launch,
    });
    // Deployed does not mean the retained exterior departure has completed.
    for _ in 0..100 {
        f.coarse_step();
        if state(&f.world(), "Pilot")
            .unwrap()
            .departures
            .contains_key(&id)
        {
            break;
        }
    }
    let binding = f.binding();
    let refusal = f.server.handle_at(
        &mut f.session,
        Request::CarrierInventory {
            request: CarrierInventoryRequest::Begin {
                binding,
                operation_id: 2,
                drone_id: id,
                action: CarrierInventoryAction::RecallToStorage,
            },
        },
        f.now,
    );
    assert!(!refusal.ok);
    for _ in 0..100 {
        f.coarse_step();
        if state(&f.world(), "Pilot").unwrap().departures.is_empty() {
            break;
        }
    }
    // Explicit exterior fixture displacement/attitude, not a claim that free
    // mining navigation is enabled. Keep the old pose map stale to prove request
    // admission reads the real same-ID body rather than teleporting to history.
    let mut moved = f.world();
    let body = moved
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == id)
        .unwrap();
    body.position = [60., 25., 100.];
    body.orientation_xyzw = Some([0., 0., 0., 1.]);
    body.heading = [1., 0., 0.];
    f.write(&moved);
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 2,
        drone_id: id,
        action: CarrierInventoryAction::RecallToStorage,
    });
    assert!(state(&f.world(), "Pilot")
        .unwrap()
        .arrivals
        .contains_key(&2));
    let accepted = f.world();
    let owned = state(&accepted, "Pilot").unwrap();
    assert_eq!(owned.arrivals[&2].path[0].position_m, [60., 25., 100.]);
    assert_eq!(
        owned.arrivals[&2].path[0].orientation_xyzw,
        [0., 0., 0., 1.]
    );
    assert!(owned.arrivals[&2].path.len() > 2);
    assert!(!owned.arrivals[&2].asteroid_obstacles.is_empty());
    validate(&accepted).unwrap();
    for mutation in 0..9 {
        let mut bad = accepted.clone();
        let s = bad
            .accounts
            .get_mut("Pilot")
            .unwrap()
            .carrier_inventory
            .as_mut()
            .unwrap();
        match mutation {
            0 => {
                s.arrivals.remove(&2);
            }
            1 => s.arrivals.get_mut(&2).unwrap().drone_id = u64::MAX,
            2 => s.arrivals.get_mut(&2).unwrap().started_at_ms = u64::MAX,
            3 => s.arrivals.get_mut(&2).unwrap().path[1].position_m = [0.; 3],
            4 => {
                s.arrivals.get_mut(&2).unwrap().asteroid_obstacles.pop();
            }
            5 => {
                s.arrivals
                    .get_mut(&2)
                    .unwrap()
                    .original_body_pose
                    .position_m[0] += 1.
            }
            6 => s.arrivals.get_mut(&2).unwrap().cassette_id = 1,
            7 => s.arrivals.get_mut(&2).unwrap().admitted_at_ms = 0,
            _ => {
                let key = s
                    .machinery
                    .keys()
                    .find(|k| k.starts_with("b00/"))
                    .cloned()
                    .unwrap();
                s.machinery.get_mut(&key).unwrap().position_m[0] += 1.;
            }
        }
        assert!(validate(&bad).is_err(), "malformed capture {mutation}");
    }
    f.coarse_step();
    f.snapshot_valid();
    let before = state(&f.world(), "Pilot").unwrap().drone_poses[&id];
    f.reopen();
    f.snapshot_valid();
    for _ in 0..3 {
        f.coarse_step();
        assert_eq!(state(&f.world(), "Pilot").unwrap().drone_poses[&id], before);
    }
    f.request(CarrierInventoryRequest::Resume {
        binding: f.binding(),
        operation_id: 2,
    });
    let mut saw_source = false;
    for _ in 0..160 {
        f.coarse_step();
        f.snapshot_valid();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        saw_source |= s.runs.contains_key(&2);
        if s.inventory.operation(2).unwrap().phase == CarrierOperationPhase::Completed {
            break;
        }
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(saw_source);
    assert_eq!(
        s.inventory.operation(2).unwrap().phase,
        CarrierOperationPhase::Completed
    );
    assert_eq!(
        s.inventory.entry(id).unwrap().location,
        CarrierDroneLocation::Ready { berth_id: 0 }
    );
    assert_eq!(s.inventory.entry(id).unwrap().hit_points, DRONE_HIT_POINTS);
    assert_eq!(s.inventory.entries.len(), 20);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert!(
        s.runs.is_empty()
            && s.arrivals.is_empty()
            && s.departures.is_empty()
            && s.inventory.reservations.is_empty()
    );
    assert_eq!(
        s.inventory
            .docked_cassette(CarrierCassetteDock::Ready { berth_id: 0 })
            .unwrap()
            .drone_id,
        Some(id)
    );
}

#[test]
fn actual_1702_deploy_all_continues_while_owner_leaves_seat_and_walks_safe_core() {
    let mut f = Fixture::new_1702();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let w = f.world();
    let account = &w.accounts["Pilot"];
    let binding = docked_traversal_binding(
        account.vessel_id(ShipClass::Pioneer).unwrap().into(),
        &default_pioneer_room_layout(),
        w.scene_layout.definition().fleet_layout,
        account.fleet_hangar,
    );
    let before = state(&w, "Pilot").unwrap().machinery.clone();
    f.ok(Request::DockedTraversal {
        request: DockedTraversalRequest::LeaveSeat {
            binding: binding.clone(),
        },
    });
    for _ in 0..3 {
        f.coarse_step();
    }
    let standing = f.world().pilots["Pilot"]
        .view
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap()
        .clone();
    assert!(standing.seat_transition.is_none());
    assert_eq!(standing.support, DockedSupport::Cockpit);
    for _ in 0..8 {
        f.ok(Request::DockedTraversal {
            request: DockedTraversalRequest::Walk {
                binding: binding.clone(),
                forward: 1.,
                strafe: 0.,
                turn: 0.,
                sprint: false,
            },
        });
        f.step();
    }
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    let walked = w.pilots["Pilot"]
        .view
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap();
    assert_ne!(walked.feet_m, standing.feet_m);
    assert!(matches!(
        walked.support,
        DockedSupport::Cockpit
            | DockedSupport::Connector
            | DockedSupport::CoreStairs
            | DockedSupport::Deck1
            | DockedSupport::Deck2
    ));
    assert!(s.inventory.deployments[0].phase.active() && !s.inventory.deployments[0].held);
    assert!(!s.runs.is_empty() && s.runs.values().all(|r| !r.held));
    assert_ne!(s.machinery, before);
    let response = f.server.handle_at(
        &mut f.session,
        Request::DockedTraversal {
            request: DockedTraversalRequest::SetHatch {
                binding,
                hatch: DockedHatch::Outer,
                open: true,
            },
        },
        f.now,
    );
    assert!(!response.ok && response.message.contains("locked"));
    let denied = f.server.handle_at(
        &mut f.session,
        Request::DockedTraversal {
            request: DockedTraversalRequest::CallLift {
                binding: walked.binding.clone(),
                stop: DockedLiftStop::Station,
            },
        },
        f.now,
    );
    assert!(!denied.ok && denied.message.contains("locked"));
    for _ in 0..8 {
        f.ok(Request::DockedTraversal {
            request: DockedTraversalRequest::Walk {
                binding: walked.binding.clone(),
                forward: -1.,
                strafe: 0.,
                turn: 0.,
                sprint: false,
            },
        });
        f.step();
    }
    f.ok(Request::DockedTraversal {
        request: DockedTraversalRequest::TakeSeat {
            binding: walked.binding.clone(),
        },
    });
    for _ in 0..3 {
        f.coarse_step();
    }
    let w = f.world();
    assert!(w.pilots["Pilot"].view.station_avatar.is_none());
    assert!(!state(&w, "Pilot").unwrap().inventory.deployments[0].held);
    f.snapshot_valid();
}

#[test]
fn actual_1702_capture_death_releases_entry_or_retains_only_empty_hardware_cleanup() {
    for during_hardware in [false, true] {
        let mut f = Fixture::new_1702();
        f.request(CarrierInventoryRequest::Begin {
            binding: f.binding(),
            operation_id: 1,
            drone_id: 1,
            action: CarrierInventoryAction::Launch,
        });
        for _ in 0..100 {
            f.coarse_step();
            let w = f.world();
            let s = state(&w, "Pilot").unwrap();
            if s.runs.is_empty() && s.departures.is_empty() {
                break;
            }
        }
        f.request(CarrierInventoryRequest::Begin {
            binding: f.binding(),
            operation_id: 2,
            drone_id: 1,
            action: CarrierInventoryAction::RecallToStorage,
        });
        if during_hardware {
            for _ in 0..100 {
                f.coarse_step();
                if state(&f.world(), "Pilot").unwrap().runs.contains_key(&2) {
                    break;
                }
            }
        }
        let mut w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.runs.contains_key(&2), during_hardware);
        assert_eq!(s.arrivals.contains_key(&2), !during_hardware);
        // Explicit post-combat death injection, not a newly enabled damage source.
        w.pilots
            .get_mut("Pilot")
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == 1)
            .unwrap()
            .hit_points = 0;
        let events = finish(&mut w, f.now).unwrap();
        assert_eq!(events.len(), 1);
        w.pilots
            .get_mut("Pilot")
            .unwrap()
            .view
            .drones
            .retain(|d| d.hit_points > 0);
        drones::sync(w.pilots.get_mut("Pilot").unwrap());
        validate(&w).unwrap();
        assert!(finish(&mut w, f.now).unwrap().is_empty());
        assert!(state(&w, "Pilot").unwrap().arrivals.is_empty());
        assert_eq!(
            state(&w, "Pilot").unwrap().runs.contains_key(&2),
            during_hardware
        );
        {
            let tx = f.server.db.transaction().unwrap();
            let mut entries = crate::load_entries(&tx, false).unwrap();
            for event in events {
                crate::append_event(
                    &tx,
                    &mut entries,
                    event,
                    f.now,
                    CARRIER_INVENTORY_RULESET_VERSION,
                )
                .unwrap();
            }
            tx.execute(
                "UPDATE world SET json=?1",
                [serde_json::to_string(&w).unwrap()],
            )
            .unwrap();
            tx.commit().unwrap();
        }
        f.reopen();
        f.snapshot_valid();
        if during_hardware {
            let held = state(&f.world(), "Pilot").unwrap().machinery.clone();
            f.coarse_step();
            assert_eq!(state(&f.world(), "Pilot").unwrap().machinery, held);
            f.request(CarrierInventoryRequest::Resume {
                binding: f.binding(),
                operation_id: 2,
            });
            for _ in 0..100 {
                f.coarse_step();
                f.snapshot_valid();
                if state(&f.world(), "Pilot").unwrap().runs.is_empty() {
                    break;
                }
            }
        }
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert!(matches!(
            s.inventory.entry(1).unwrap().location,
            CarrierDroneLocation::Destroyed { .. }
        ));
        assert_eq!(
            s.inventory.operation(2).unwrap().phase,
            CarrierOperationPhase::Destroyed
        );
        assert!(
            s.runs.is_empty()
                && s.inventory.cleanups.is_empty()
                && s.inventory.reservations.is_empty()
        );
        assert!(s.inventory.cassettes.iter().all(|c| c.drone_id != Some(1)));
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert_eq!(s.ore.lost_ore_units, 0);
    }
}

#[test]
fn actual_1702_reverse_cassette_exchange_reopens_every_loaded_leg_without_early_release() {
    let mut f = staged_1702_handoff();
    for _ in 0..180 {
        f.coarse_step();
        if state(&f.world(), "Pilot").unwrap().runs.is_empty() {
            break;
        }
    }
    assert!(state(&f.world(), "Pilot").unwrap().runs.is_empty());
    let original = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    let request = CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 3,
        drone_id: 2,
        action: CarrierInventoryAction::StoreReady { slot_id: 0 },
    };
    f.request(request.clone());
    let mut interrupted = std::collections::BTreeSet::new();
    for tick in 0..450 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.roster, original);
        let op = s.inventory.operation(3).unwrap();
        let x = op.stage.as_ref().unwrap();
        assert!(x.loaded_handoff_at_ms.is_none());
        if op.phase == CarrierOperationPhase::Completed {
            break;
        }
        assert_eq!(
            s.inventory
                .reservations
                .iter()
                .filter(|r| r.operation_id == 3)
                .count(),
            6
        );
        assert_eq!(
            s.inventory.entry(2).unwrap().location,
            CarrierDroneLocation::Loading { operation_id: 3 }
        );
        assert!(s.runs[&3].kind.writes_drone());
        if tick % 10 == 0 {
            f.snapshot_valid();
        }
        if x.completed_legs < 3
            && s.inventory.cassettes.iter().any(|c| {
                matches!(
                    c.location,
                    CarrierCassetteLocation::InLoader {
                        operation_id: 3,
                        ..
                    }
                )
            })
            && interrupted.insert(x.completed_legs)
        {
            f.request(CarrierInventoryRequest::Hold {
                binding: f.binding(),
                operation_id: 3,
            });
            let held = state(&f.world(), "Pilot").unwrap().clone();
            f.reopen();
            for _ in 0..2 {
                f.coarse_step();
            }
            let current = state(&f.world(), "Pilot").unwrap().clone();
            assert_eq!(held.machinery, current.machinery);
            assert_eq!(held.cassette_poses, current.cassette_poses);
            assert_eq!(held.drone_poses, current.drone_poses);
            f.snapshot_valid();
            f.request(CarrierInventoryRequest::Resume {
                binding: f.binding(),
                operation_id: 3,
            });
        }
    }
    f.snapshot_valid();
    assert_eq!(interrupted.len(), 3);
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(
        s.inventory.operation(3).unwrap().phase,
        CarrierOperationPhase::Completed
    );
    assert_eq!(
        s.inventory.entry(2).unwrap().location,
        CarrierDroneLocation::Protected { slot_id: 0 }
    );
    assert_eq!(
        s.inventory
            .docked_cassette(CarrierCassetteDock::Protected { slot_id: 0 })
            .unwrap()
            .drone_id,
        Some(2)
    );
    assert!(s
        .inventory
        .docked_cassette(CarrierCassetteDock::Ready { berth_id: 0 })
        .unwrap()
        .drone_id
        .is_none());
    assert!(s.inventory.reservations.is_empty() && s.runs.is_empty());
    assert_eq!(s.ore.ore_units(), Some(0));
    assert!(s.inventory.entries.iter().all(|e| e.hit_points == 100));
    let before = s.inventory.clone();
    f.request(request);
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, before);
}

#[test]
fn actual_1702_one_recall_all_recovers_twenty_to_ten_protected_and_ten_ready() {
    let mut f = Fixture::new_1702();
    let original = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    for tick in 0..1800 {
        f.coarse_step();
        if tick % 100 == 0 {
            eprintln!("full recall preparation deployment {}s", tick);
        }
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .active_deployment()
            .is_none()
        {
            break;
        }
    }
    assert!(state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .active_deployment()
        .is_none());
    assert_eq!(f.world().pilots["Pilot"].view.drones.len(), 20);
    f.request(CarrierInventoryRequest::RecallAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut restarted = false;
    let mut saw_store = false;
    let mut checked_links = false;
    let mut checked_cancel = false;
    let mut done = false;
    for tick in 0..4000 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.roster, original);
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert!(s.inventory.entries.iter().all(|e| e.hit_points == 100));
        assert_eq!(s.ore.ore_units(), Some(0));
        saw_store |= s.inventory.operations.iter().any(|o| {
            matches!(o.action, CarrierInventoryAction::StoreReady { .. }) && !o.phase.terminal()
        });
        if !checked_links
            && s.inventory.recalls[0]
                .members
                .iter()
                .any(|m| m.store_operation_id.is_some())
        {
            assert!(validate(&w).is_ok());
            for store in [false, true] {
                let mut bad = w.clone();
                let s = bad
                    .accounts
                    .get_mut("Pilot")
                    .unwrap()
                    .carrier_inventory
                    .as_mut()
                    .unwrap();
                let m = s.inventory.recalls[0]
                    .members
                    .iter_mut()
                    .find(|m| {
                        if store {
                            m.store_operation_id.is_some()
                        } else {
                            m.capture_operation_id.is_some()
                        }
                    })
                    .unwrap();
                if store {
                    m.store_operation_id = None;
                } else {
                    m.capture_operation_id = None;
                }
                assert!(
                    validate(&bad).is_err(),
                    "Stripped physical child link was accepted"
                );
            }
            checked_links = true;
        }
        if !checked_cancel
            && s.inventory
                .entries
                .iter()
                .any(|e| matches!(e.location, CarrierDroneLocation::Protected { .. }))
            && s.inventory
                .entries
                .iter()
                .any(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
            && !s.runs.is_empty()
            && s.inventory
                .operations
                .iter()
                .any(|o| o.phase == CarrierOperationPhase::Queued)
        {
            // Controller clone validation only; the real fixture continues its
            // uninterrupted finite queue and later actual close/reopen test.
            let secured: Vec<_> = s
                .inventory
                .entries
                .iter()
                .filter(|e| e.location.dock().is_some())
                .map(|e| e.drone_id)
                .collect();
            let binding = s.inventory.binding.clone();
            let mut cancelled = w.clone();
            recall::execute(
                &mut cancelled,
                "Pilot",
                &CarrierInventoryRequest::HoldRecall {
                    binding: binding.clone(),
                    batch_id: 1,
                },
                f.now,
            )
            .unwrap();
            recall::execute(
                &mut cancelled,
                "Pilot",
                &CarrierInventoryRequest::CancelRecallRemaining {
                    binding,
                    batch_id: 1,
                },
                f.now,
            )
            .unwrap();
            validate(&cancelled).unwrap();
            assert!(state(&cancelled, "Pilot").unwrap().inventory.recalls[0]
                .members
                .iter()
                .filter(|m| secured.contains(&m.drone_id))
                .all(|m| !m.cancelled));
            checked_cancel = true;
        }
        if tick % 100 == 0 {
            eprintln!(
                "recall {}s: active={} runs={} arrivals={} held={} remaining={:?}",
                tick,
                w.pilots["Pilot"].view.drones.len(),
                s.runs.len(),
                s.arrivals.len(),
                s.inventory
                    .operations
                    .iter()
                    .filter(|o| o.phase == CarrierOperationPhase::Held)
                    .count(),
                s.inventory.recalls[0]
                    .members
                    .iter()
                    .filter(|m| !m.blocked_reason.is_empty())
                    .map(|m| (m.drone_id, &m.blocked_reason))
                    .collect::<Vec<_>>()
            );
            f.snapshot_valid();
        }
        if !restarted
            && saw_store
            && s.inventory
                .cassettes
                .iter()
                .any(|c| matches!(c.location, CarrierCassetteLocation::InLoader { .. }))
        {
            f.request(CarrierInventoryRequest::HoldRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            let before = state(&f.world(), "Pilot").unwrap().clone();
            let child = before.inventory.recalls[0]
                .members
                .iter()
                .flat_map(|m| [m.capture_operation_id, m.store_operation_id])
                .flatten()
                .find(|id| {
                    before
                        .inventory
                        .operation(*id)
                        .is_some_and(|o| !o.phase.terminal())
                })
                .unwrap();
            let denied = f.server.handle_at(
                &mut f.session,
                Request::CarrierInventory {
                    request: CarrierInventoryRequest::Resume {
                        binding: before.inventory.binding.clone(),
                        operation_id: child,
                    },
                },
                f.now,
            );
            assert!(!denied.ok);
            f.reopen();
            for _ in 0..3 {
                f.coarse_step();
            }
            let held = state(&f.world(), "Pilot").unwrap().clone();
            assert_eq!(before.machinery, held.machinery);
            assert_eq!(before.cassette_poses, held.cassette_poses);
            assert_eq!(before.drone_poses, held.drone_poses);
            f.request(CarrierInventoryRequest::ResumeRecall {
                binding: f.binding(),
                batch_id: 1,
            });
            restarted = true;
        }
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .active_recall()
            .is_none()
        {
            done = true;
            break;
        }
    }
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(
        done && restarted && saw_store && checked_links && checked_cancel,
        "Recall did not finish: {:?}",
        s.inventory.recalls
    );
    assert_eq!(s.inventory.recalls[0].phase, CarrierRecallPhase::Completed);
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Protected { .. }))
            .count(),
        10
    );
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
            .count(),
        10
    );
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert!(
        s.runs.is_empty()
            && s.arrivals.is_empty()
            && s.departures.is_empty()
            && s.inventory.reservations.is_empty()
            && s.inventory.cleanups.is_empty()
    );
    let before = s.inventory.clone();
    f.request(CarrierInventoryRequest::RecallAll {
        binding: f.binding(),
        batch_id: 1,
    });
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, before);
}

#[test]
fn actual_1702_recall_cancel_keeps_stored_outcomes_and_dead_cleanup_owned_on_reopen() {
    let mut f = Fixture::new_1702();
    f.request(CarrierInventoryRequest::Begin {
        binding: f.binding(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::Launch,
    });
    for _ in 0..60 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.runs.is_empty() && s.departures.is_empty() {
            break;
        }
    }
    f.request(CarrierInventoryRequest::RecallAll {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..120 {
        f.coarse_step();
        if state(&f.world(), "Pilot").unwrap().runs.contains_key(&2) {
            break;
        }
    }
    assert!(state(&f.world(), "Pilot").unwrap().runs.contains_key(&2));
    // Deliberate post-combat test injection, not a weapon/health simulation claim.
    let mut w = f.world();
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == 1)
        .unwrap()
        .hit_points = 0;
    let events = finish(&mut w, f.now).unwrap();
    assert_eq!(events.len(), 1);
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .retain(|d| d.hit_points > 0);
    drones::sync(w.pilots.get_mut("Pilot").unwrap());
    validate(&w).unwrap();
    assert!(finish(&mut w, f.now).unwrap().is_empty());
    {
        let tx = f.server.db.transaction().unwrap();
        let mut ledger = crate::load_entries(&tx, false).unwrap();
        for event in events {
            crate::append_event(
                &tx,
                &mut ledger,
                event,
                f.now,
                CARRIER_INVENTORY_RULESET_VERSION,
            )
            .unwrap();
        }
        tx.execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
        tx.commit().unwrap();
    }
    assert!(state(&f.world(), "Pilot")
        .unwrap()
        .inventory
        .cleanups
        .iter()
        .any(|c| c.operation_id == 2));
    f.request(CarrierInventoryRequest::HoldRecall {
        binding: f.binding(),
        batch_id: 1,
    });
    f.request(CarrierInventoryRequest::CancelRecallRemaining {
        binding: f.binding(),
        batch_id: 1,
    });
    let held = state(&f.world(), "Pilot").unwrap().clone();
    assert!(held.inventory.recalls[0]
        .members
        .iter()
        .all(|m| !m.cancelled));
    f.reopen();
    for _ in 0..3 {
        f.coarse_step();
    }
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().machinery,
        held.machinery
    );
    f.request(CarrierInventoryRequest::ResumeRecall {
        binding: f.binding(),
        batch_id: 1,
    });
    for _ in 0..60 {
        f.coarse_step();
        if state(&f.world(), "Pilot")
            .unwrap()
            .inventory
            .active_recall()
            .is_none()
        {
            break;
        }
    }
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.recalls[0].phase, CarrierRecallPhase::Cancelled);
    assert!(s.inventory.recalls[0]
        .members
        .iter()
        .all(|m| m.completed_location.is_some() && !m.cancelled));
    assert!(s.inventory.recalls[0]
        .members
        .iter()
        .find(|m| m.drone_id == 1)
        .unwrap()
        .completed_location
        .as_ref()
        .is_some_and(|p| matches!(p, CarrierDroneLocation::Destroyed { .. })));
    assert_eq!(
        s.inventory
            .entries
            .iter()
            .filter(|e| e.location.dock().is_some())
            .count(),
        19
    );
    assert!(
        s.runs.is_empty() && s.inventory.cleanups.is_empty() && s.inventory.reservations.is_empty()
    );
    assert_eq!(s.ore.lost_ore_units, 0);
    let before = s.inventory.clone();
    f.request(CarrierInventoryRequest::RecallAll {
        binding: f.binding(),
        batch_id: 1,
    });
    assert_eq!(state(&f.world(), "Pilot").unwrap().inventory, before);
}

#[path = "parallel_tests.rs"]
mod parallel_tests;

#[path = "camera_tests.rs"]
mod camera_tests;

#[path = "parallel_return_tests.rs"]
mod parallel_return_tests;
#[path = "service_tests.rs"]
mod service_tests;

#[path = "parallel_turret_tests.rs"]
mod parallel_turret_tests;

#[path = "parallel_turret_samples.rs"]
mod parallel_turret_samples;

#[path = "corridor_tests.rs"]
mod corridor_tests;

#[path = "parallel_turret_aim_tests.rs"]
mod parallel_turret_aim_tests;

#[path = "parallel_turret_aim_capture_tests.rs"]
mod parallel_turret_aim_capture_tests;
