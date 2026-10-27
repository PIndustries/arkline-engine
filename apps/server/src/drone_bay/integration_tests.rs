//! Disposable real Server command/transaction acceptance; no production saves.
use super::*;
use std::path::PathBuf;
pub(super) struct Fixture {
    server: Server,
    session: Option<String>,
    now: u64,
    path: PathBuf,
    token: String,
}
fn connect(token: Option<String>) -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
        supports_room_controls: true,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: true,
        pilot: "Pilot".into(),
        resume_token: token,
    }
}
impl Fixture {
    pub(super) fn new() -> Self {
        Self::with_prior_world(None)
    }
    /// Disposable pre-feature file, established before the current server opens it.
    /// This is a compatibility fixture, not a migration of an existing world.
    fn with_prior_world(physical: Option<bool>) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "arkline-physical-bay-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("world.sqlite");
        if let Some(physical) = physical {
            let db = rusqlite::Connection::open(&path).unwrap();
            db.execute_batch("CREATE TABLE identity_realm(id INTEGER PRIMARY KEY,realm TEXT NOT NULL); INSERT INTO identity_realm VALUES(1,'local-development-docking-v1'); CREATE TABLE world(id INTEGER PRIMARY KEY,json TEXT NOT NULL);").unwrap();
            let world = World {
                scene_layout: SceneLayoutRevision::DockingV1,
                drone_bay_registration: physical.then(runtime::registration).flatten(),
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
        let mut session = None;
        let now = server.editor_time.unwrap();
        let response = server.handle_at(&mut session, connect(None), now);
        assert!(response.ok, "{}", response.message);
        Self {
            server,
            session,
            now,
            path,
            token: response.resume_token.unwrap(),
        }
    }
    pub(super) fn ok(&mut self, request: Request) -> Response {
        let r = self.server.handle_at(&mut self.session, request, self.now);
        assert!(r.ok, "{}", r.message);
        r
    }
    fn snap(&mut self) -> Response {
        self.ok(Request::Snapshot)
    }
    pub(super) fn world(&self) -> World {
        serde_json::from_str(&self.raw()).unwrap()
    }
    fn raw(&self) -> String {
        self.server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap()
    }
    fn step(&mut self) {
        self.now += 100;
        self.server.tick_at(self.now).unwrap();
    }
    pub(super) fn space(&mut self) {
        self.ok(Request::Undock);
        for _ in 0..180 {
            self.step();
        }
        assert_eq!(
            self.world().pilots["Pilot"]
                .view
                .flight
                .as_ref()
                .unwrap()
                .docking
                .as_ref()
                .unwrap()
                .phase,
            DockingPhase::Free
        );
    }
    pub(super) fn request(&self, kind: DroneBayOperationKind, drone: u64) -> Request {
        let w = self.world();
        let s = state(&w, "Pilot").unwrap();
        let binding = s.binding.clone();
        let operation_id = s.high_watermark + 1;
        Request::DroneBay {
            request: match kind {
                DroneBayOperationKind::Launch => DroneBayRequest::Launch {
                    binding,
                    drone_id: drone,
                    operation_id,
                },
                DroneBayOperationKind::Recover => DroneBayRequest::Recover {
                    binding,
                    drone_id: drone,
                    operation_id,
                },
            },
        }
    }
    fn control(&self, which: &str) -> Request {
        let w = self.world();
        let s = state(&w, "Pilot").unwrap();
        let binding = s.binding.clone();
        let operation_id = s.high_watermark;
        Request::DroneBay {
            request: match which {
                "hold" => DroneBayRequest::Hold {
                    binding,
                    operation_id,
                },
                "resume" => DroneBayRequest::Resume {
                    binding,
                    operation_id,
                },
                _ => DroneBayRequest::Cancel {
                    binding,
                    operation_id,
                },
            },
        }
    }
    fn finish(&mut self) {
        for _ in 0..300 {
            if !busy(&self.world(), "Pilot") {
                return;
            }
            // Conservation/interruption gates do not need video-rate SQLite
            // writes. Motion evidence uses its separate exact100ms export.
            self.now += 1_000;
            self.server.tick_at(self.now).unwrap();
        }
        panic!(
            "bay did not complete: {}",
            serde_json::to_string(&state(&self.world(), "Pilot")).unwrap()
        );
    }
}
#[test]
fn real_fresh_connect_seeds_once_and_legacy_world_stays_unseeded() {
    let mut f = Fixture::new();
    let r = f.snap();
    let stock = r.assets.unwrap().drone_bay.unwrap();
    assert_eq!(stock.counts(None).stored, 15);
    assert!(r.snapshot.unwrap().pilots[0].drones.is_empty());
    let before = f.world().next_drone_id;
    f.ok(connect(Some(f.token.clone())));
    assert_eq!(f.world().next_drone_id, before);
    let mut legacy = Fixture::with_prior_world(Some(false));
    assert!(state(&legacy.world(), "Pilot").is_none());
    let before = legacy.world().next_drone_id;
    legacy.ok(connect(Some(legacy.token.clone())));
    assert!(state(&legacy.world(), "Pilot").is_none());
    assert_eq!(legacy.world().next_drone_id, before);
}
#[test]
fn actual_launch_return_keeps_identity_health_and_continuous_pose() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[5].drone_id;
    let mut w = f.world();
    w.accounts
        .get_mut("Pilot")
        .unwrap()
        .drone_bays
        .values_mut()
        .next()
        .unwrap()
        .stock[5]
        .status = DroneStockStatus::Stored { hit_points: 37 };
    f.server
        .db
        .execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
    let command = f.request(DroneBayOperationKind::Launch, id);
    f.ok(command.clone());
    let original = f.world().pilots["Pilot"].view.drones[0].clone();
    let mut previous = original.position;
    for _ in 0..1500 {
        let w = f.world();
        if !busy(&w, "Pilot") {
            break;
        }
        f.step();
        let w = f.world();
        let d = &w.pilots["Pilot"].view.drones[0];
        assert_eq!(d.id, id);
        assert_eq!(d.hit_points, 37);
        assert!(
            flight::distance(previous, d.position) <= 4.001,
            "motion discontinuity"
        );
        assert!(d.orientation_xyzw.is_some());
        previous = d.position;
    }
    assert!(!busy(&f.world(), "Pilot"));
    let before = f.raw();
    f.ok(command);
    assert_eq!(f.raw(), before, "exact retry mutated the accepted state");
    f.ok(f.request(DroneBayOperationKind::Recover, id));
    f.finish();
    let w = f.world();
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(
        state(&w, "Pilot").unwrap().stock[5].status,
        DroneStockStatus::Stored { hit_points: 37 }
    );
    assert_eq!(state(&w, "Pilot").unwrap().counts(None).total(), 15);
    assert_eq!(w.pilots["Pilot"].view.ore, 0);
}
#[test]
fn held_resume_repeated_resume_and_cancel_do_not_snap_or_restart_deadlines() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[0].drone_id;
    f.ok(f.request(DroneBayOperationKind::Launch, id));
    for _ in 0..35 {
        f.step();
    }
    f.ok(f.control("hold"));
    let position = f.world().pilots["Pilot"].view.drones[0].position;
    for _ in 0..30 {
        f.step();
    }
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].position, position);
    let held = f
        .snap()
        .snapshot
        .unwrap()
        .pilots
        .remove(0)
        .drone_bay
        .unwrap();
    assert!(held.machinery.groups.iter().all(|g| g.segment.is_none()));
    f.ok(f.control("resume"));
    let raw = f.raw();
    f.ok(f.control("resume"));
    assert_eq!(raw, f.raw());
    f.ok(f.control("cancel"));
    assert_eq!(position, f.world().pilots["Pilot"].view.drones[0].position);
    f.finish();
    let w = f.world();
    assert_eq!(
        state(&w, "Pilot").unwrap().stock[0].status,
        DroneStockStatus::Stored { hit_points: 100 }
    );
    assert_eq!(
        state(&w, "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Cancelled
    );
    let before = f.raw();
    f.ok(f.control("resume"));
    assert_eq!(before, f.raw());
}
#[test]
fn actual_sql_write_fault_rolls_back_stock_operation_and_drone_together() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[0].drone_id;
    let command = f.request(DroneBayOperationKind::Launch, id);
    let before = f.raw();
    f.server.db.execute_batch("CREATE TRIGGER fail_bay_write BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'bay commit fault'); END;").unwrap();
    let r = f.server.handle_at(&mut f.session, command.clone(), f.now);
    assert!(!r.ok);
    assert_eq!(f.raw(), before);
    f.server
        .db
        .execute_batch("DROP TRIGGER fail_bay_write;")
        .unwrap();
    f.ok(command);
    let before = f.raw();
    f.server.db.execute_batch("CREATE TRIGGER fail_bay_tick BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'bay tick fault'); END;").unwrap();
    assert!(f.server.tick_at(f.now + 100).is_err());
    assert_eq!(f.raw(), before);
    f.server
        .db
        .execute_batch("DROP TRIGGER fail_bay_tick;")
        .unwrap();
}
#[test]
fn restart_holds_same_body_and_no_offline_completion_or_refill() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[1].drone_id;
    let request = f.request(DroneBayOperationKind::Launch, id);
    f.ok(request.clone());
    for _ in 0..25 {
        f.step();
    }
    let position = f.world().pilots["Pilot"].view.drones[0].position;
    let next = f.world().next_drone_id;
    let path = f.path.clone();
    drop(f.server);
    f.server = Server::open_editor_realm_with_scene(
        &path,
        default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1,
    )
    .unwrap();
    f.session = None;
    f.ok(connect(Some(f.token.clone())));
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].position, position);
    assert_eq!(f.world().next_drone_id, next);
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
    f.now += 20_000;
    f.server.tick_at(f.now).unwrap();
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].position, position);
    f.ok(request);
    f.ok(f.control("resume"));
    f.finish();
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Free
    );
}
#[test]
#[ignore = "exports actual initial authority Response for native proof"]
fn fresh_stock_initial_response_export() {
    let directory =
        std::env::var_os("ARKLINE_BAY_INITIAL_EXPORT").expect("explicit private output directory");
    let mut f = Fixture::new();
    let mut response = f.snap();
    response.resume_token = None;
    let directory = PathBuf::from(directory);
    std::fs::create_dir_all(&directory).unwrap();
    let bytes = serde_json::to_vec_pretty(&response).unwrap();
    std::fs::write(directory.join("initial-response.json"), &bytes).unwrap();
    std::fs::write(directory.join("initial-response-receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"kind":"actual fresh Server Connect/Snapshot", "schema":1,"time_ms":f.now,"source_sha256":arkline_protocol::drone_bay_layout::DRONE_BAY_SOURCE_SHA256,"response_sha256":hex::encode(Sha256::digest(&bytes)),"stored":15,"deployed":0,"credentials_removed":true})).unwrap()).unwrap();
}

#[test]
fn physical_fleet_fifteen_launches_and_returns_without_free_replacements() {
    let mut f = Fixture::new();
    f.space();
    let ids: Vec<_> = state(&f.world(), "Pilot")
        .unwrap()
        .stock
        .iter()
        .map(|e| e.drone_id)
        .collect();
    for id in &ids {
        f.ok(f.request(DroneBayOperationKind::Launch, *id));
        f.finish();
    }
    let w = f.world();
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 15);
    assert_eq!(w.next_drone_id, 16);
    for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
        assert_eq!(state(&w, "Pilot").unwrap().counts(Some(class)).active, 5);
    }
    for id in ids {
        f.ok(f.request(DroneBayOperationKind::Recover, id));
        f.finish();
    }
    let w = f.world();
    assert_eq!(state(&w, "Pilot").unwrap().counts(None).stored, 15);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!(w.next_drone_id, 16);
}
#[test]
fn duplicate_payload_foreign_vessel_and_departure_guards_preserve_state() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[0].drone_id;
    let request = f.request(DroneBayOperationKind::Launch, id);
    f.ok(request.clone());
    let before = f.raw();
    let mut foreign = request.clone();
    if let Request::DroneBay {
        request: DroneBayRequest::Launch { binding, .. },
    } = &mut foreign
    {
        binding.vessel_id = "another-real-vessel".into();
    }
    assert!(!f.server.handle_at(&mut f.session, foreign, f.now).ok);
    assert_eq!(f.raw(), before);
    let mut conflict = request.clone();
    if let Request::DroneBay {
        request: DroneBayRequest::Launch { drone_id, .. },
    } = &mut conflict
    {
        *drone_id += 1;
    }
    assert!(!f.server.handle_at(&mut f.session, conflict, f.now).ok);
    assert_eq!(f.raw(), before);
    for request in [
        Request::LaunchDroneClass {
            class: DroneClass::Mining,
        },
        Request::Warp {
            destination: Destination::Belt,
        },
        Request::Dock,
        Request::Navigate {
            mode: NavigationMode::Manual,
        },
        Request::PilotInput {
            translation: [1., 0., 0.],
            rotation: [0.; 3],
        },
        Request::AssignMiningDrone { id, target_id: 1 },
    ] {
        assert!(!f.server.handle_at(&mut f.session, request, f.now).ok);
        assert_eq!(f.raw(), before);
    }
    f.ok(Request::Navigate {
        mode: NavigationMode::Stop,
    });
    f.ok(Request::PilotInput {
        translation: [0.; 3],
        rotation: [0.; 3],
    });
    f.step();
    assert_ne!(
        state(&f.world(), "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
}
#[test]
fn last_session_disconnect_holds_and_late_join_sees_same_identity() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[2].drone_id;
    f.ok(f.request(DroneBayOperationKind::Launch, id));
    for _ in 0..20 {
        f.step();
    }
    let mut second = None;
    let r = f
        .server
        .handle_at(&mut second, connect(Some(f.token.clone())), f.now);
    assert!(r.ok, "{}", r.message);
    assert_eq!(r.snapshot.unwrap().pilots[0].drones[0].id, id);
    f.server.disconnect(&mut second);
    assert_ne!(
        state(&f.world(), "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
    f.server.disconnect(&mut f.session);
    let w = f.world();
    let position = w.pilots["Pilot"].view.drones[0].position;
    assert_eq!(
        state(&w, "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
    for _ in 0..100 {
        f.step();
    }
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].position, position);
    f.ok(connect(Some(f.token.clone())));
    f.ok(f.control("resume"));
    f.finish();
    assert_eq!(f.world().pilots["Pilot"].view.drones[0].id, id);
}
#[test]
fn actual_job_launch_counter_and_operation_rollback_together() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[5].drone_id;
    let mut w = f.world();
    resource_jobs::start(&mut w, "Pilot", "Pilot", 100, f.now).unwrap();
    w.resource_jobs.get_mut("Pilot").unwrap().view.phase = ResourceJobPhase::Mining;
    f.server
        .db
        .execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
    let request = f.request(DroneBayOperationKind::Launch, id);
    let before = f.raw();
    f.server.db.execute_batch("CREATE TRIGGER fail_job_bay BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'job inventory fault'); END;").unwrap();
    assert!(f
        .server
        .execute(&mut f.session, request.clone(), f.now, true, None)
        .is_err());
    assert_eq!(f.raw(), before);
    f.server
        .db
        .execute_batch("DROP TRIGGER fail_job_bay;")
        .unwrap();
    f.server
        .execute(&mut f.session, request.clone(), f.now, true, None)
        .unwrap();
    let w = f.world();
    assert_eq!(
        w.resource_jobs["Pilot"].view.phase,
        ResourceJobPhase::Mining
    );
    let value = serde_json::to_value(&w.resource_jobs["Pilot"]).unwrap();
    assert_eq!(value["drones_launched"], 1);
    assert_eq!(value["bay_launches"].as_array().unwrap().len(), 1);
    f.server
        .execute(&mut f.session, request, f.now, true, None)
        .unwrap();
    assert_eq!(
        serde_json::to_value(&f.world().resource_jobs["Pilot"]).unwrap()["drones_launched"],
        1
    );
    f.ok(Request::Navigate {
        mode: NavigationMode::Stop,
    });
    let w = f.world();
    assert_eq!(
        w.resource_jobs["Pilot"].view.phase,
        ResourceJobPhase::Cancelled
    );
    assert_eq!(
        state(&w, "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
}
#[test]
fn unknown_saved_geometry_fails_closed_without_reseeding_or_rewriting() {
    let f = Fixture::new();
    let mut w = f.world();
    w.drone_bay_registration.as_mut().unwrap().geometry_revision += 1;
    let raw = serde_json::to_string(&w).unwrap();
    f.server
        .db
        .execute("UPDATE world SET json=?1", [&raw])
        .unwrap();
    let path = f.path.clone();
    drop(f.server);
    assert!(Server::open_editor_realm_with_scene(
        &path,
        default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1
    )
    .is_err());
    let db = Connection::open(&path).unwrap();
    let after: String = db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, raw);
}
#[test]
#[ignore = "exports real authority Responses for native motion proof"]
fn export_actual_launch_recovery_journey() {
    let dir = PathBuf::from(
        std::env::var_os("ARKLINE_BAY_JOURNEY_EXPORT").expect("explicit private output directory"),
    );
    std::fs::create_dir_all(&dir).unwrap();
    let mut f = Fixture::new();
    f.space();
    let start = f.now;
    let id = state(&f.world(), "Pilot").unwrap().stock[7].drone_id;
    let mut frames = Vec::new();
    let record = |f: &mut Fixture, frames: &mut Vec<serde_json::Value>| {
        let mut r = f.snap();
        r.resume_token = None;
        let file = format!("response-{:04}.json", frames.len());
        let bytes = serde_json::to_vec(&r).unwrap();
        std::fs::write(dir.join(&file), &bytes).unwrap();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        frames.push(serde_json::json!({"index":frames.len(),"file":file,"time_ms":f.now,"elapsed_ms":f.now-start,"response_sha256":hex::encode(Sha256::digest(&bytes)),"stored":s.counts(None).stored,"active":s.counts(None).active,"destroyed":s.counts(None).destroyed,"phase":s.operations.last().map(|o|o.phase)}));
    };
    for _ in 0..10 {
        record(&mut f, &mut frames);
        f.step();
    }
    let launch_at = frames.len();
    f.ok(f.request(DroneBayOperationKind::Launch, id));
    record(&mut f, &mut frames);
    for _ in 0..1500 {
        if !busy(&f.world(), "Pilot") {
            break;
        }
        f.step();
        record(&mut f, &mut frames);
    }
    assert!(!busy(&f.world(), "Pilot"));
    for _ in 0..20 {
        f.step();
        record(&mut f, &mut frames);
    }
    let recover_at = frames.len();
    f.ok(f.request(DroneBayOperationKind::Recover, id));
    for _ in 0..1500 {
        f.step();
        record(&mut f, &mut frames);
        if !busy(&f.world(), "Pilot") {
            break;
        }
    }
    assert!(!busy(&f.world(), "Pilot"));
    for _ in 0..10 {
        f.step();
        record(&mut f, &mut frames);
    }
    let final_stock = state(&f.world(), "Pilot").unwrap().clone();
    assert_eq!(final_stock.counts(None).stored, 15);
    assert_eq!(final_stock.stock[7].drone_id, id);
    let receipt = serde_json::json!({"schema":"arkline.drone-bay-authority-sequence/v1","pilot":"Pilot","cadence_ms":100,"drone_id":id,"home_cradle":7,"registration":12,"source_sha256":arkline_protocol::drone_bay_layout::DRONE_BAY_SOURCE_SHA256,"launch_frame":launch_at,"recovery_frame":recover_at,"initial_time_ms":start,"final_time_ms":f.now,"frames":frames,"credentials_removed":true,"fixture":"fresh DockingV1 Server, actual Connect, Undock, identified Launch/Recover and ticks; no snapshot pose edits"});
    std::fs::write(
        dir.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn old_client_mixed_session_refusal_is_non_mutating_and_unstamped_world_accepts_two() {
    let mut f = Fixture::new();
    let before = f.raw();
    let online = f.server.online_sessions.clone();
    for existing in [false, true] {
        for version in [0, 1, 2, 3, WORLD_LAYOUT_SCHEMA_VERSION + 1] {
            let mut request = connect(existing.then(|| f.token.clone()));
            if let Request::Connect {
                world_layout_version,
                pilot,
                ..
            } = &mut request
            {
                *world_layout_version = version;
                if !existing {
                    *pilot = "OldClient".into();
                }
            }
            let mut session = None;
            let r = f.server.handle_at(&mut session, request, f.now);
            assert!(!r.ok);
            assert!(session.is_none());
            assert_eq!(f.raw(), before);
            assert_eq!(f.server.online_sessions, online);
        }
    }
    let mut current = None;
    let r = f
        .server
        .handle_at(&mut current, connect(Some(f.token.clone())), f.now);
    assert!(r.ok, "{}", r.message);
    assert_eq!(current.as_deref(), Some("Pilot"));
    let layout = r.snapshot.unwrap().world_layout.unwrap();
    assert_eq!(layout.schema_version, 2);
    layout.validate().unwrap();
    let mut legacy = Fixture::with_prior_world(Some(false));
    let mut request = connect(Some(legacy.token.clone()));
    if let Request::Connect {
        world_layout_version,
        ..
    } = &mut request
    {
        *world_layout_version = 2;
    }
    assert!(
        legacy
            .server
            .handle_at(&mut legacy.session, request, legacy.now)
            .ok
    );
    assert!(state(&legacy.world(), "Pilot").is_none());
}

#[test]
fn incoming_combat_wins_final_capture_and_terminal_cleanup_never_revives_drone() {
    let mut f = Fixture::new();
    f.space();
    let mut attacker = None;
    let mut request = connect(None);
    if let Request::Connect { pilot, .. } = &mut request {
        *pilot = "Attacker".into();
    }
    assert!(f.server.handle_at(&mut attacker, request, f.now).ok);
    assert!(f.server.handle_at(&mut attacker, Request::Undock, f.now).ok);
    for _ in 0..180 {
        f.step();
    }
    let w = f.world();
    let a = state(&w, "Attacker").unwrap();
    let attack_id = a.stock[10].drone_id;
    let request = Request::DroneBay {
        request: DroneBayRequest::Launch {
            binding: a.binding.clone(),
            drone_id: attack_id,
            operation_id: 1,
        },
    };
    let r = f.server.handle_at(&mut attacker, request, f.now);
    assert!(r.ok, "{}", r.message);
    for _ in 0..300 {
        if !busy(&f.world(), "Attacker") {
            break;
        }
        f.now += 1000;
        f.server.tick_at(f.now).unwrap();
    }
    assert!(!busy(&f.world(), "Attacker"));
    let victim = state(&f.world(), "Pilot").unwrap().stock[0].drone_id;
    f.ok(f.request(DroneBayOperationKind::Launch, victim));
    f.finish();
    let recovery = f.request(DroneBayOperationKind::Recover, victim);
    f.ok(recovery.clone());
    let mut at_final = false;
    for _ in 0..300 {
        let w = f.world();
        let runtime = serde_json::to_value(
            w.accounts["Pilot"]
                .drone_bay_runtime
                .values()
                .next()
                .unwrap(),
        )
        .unwrap();
        if runtime["index"].as_u64().unwrap() + 2
            == runtime["points"].as_array().unwrap().len() as u64
        {
            at_final = true;
            break;
        }
        f.now += 1000;
        f.server.tick_at(f.now).unwrap();
    }
    assert!(at_final, "did not reach final actual capture interval");
    // Controlled combat initial condition: same real launched attack entity,
    // healthy damaged victim, ready ordinary attack cycle. No HP-zero injection.
    let mut w = f.world();
    let target = w
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == victim)
        .unwrap();
    target.hit_points = DRONE_ATTACK_DAMAGE;
    let target_position = target.position;
    let source = w
        .pilots
        .get_mut("Attacker")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == attack_id)
        .unwrap();
    source.position = target_position;
    source.position[1] += DRONE_ATTACK_RANGE * 0.5;
    source.velocity = [0.; 3];
    source.mode = DroneMode::Attacking {
        target_pilot: "Pilot".into(),
        target_drone_id: victim,
    };
    source.attack_ready_at_ms = Some(f.now);
    source.updated_at_ms = f.now;
    let ore_before = (w.pilots["Pilot"].view.ore, w.belt_ore);
    f.server
        .db
        .execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
    f.now += 1000;
    f.server.tick_at(f.now).unwrap();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert!(matches!(
        s.stock[0].status,
        DroneStockStatus::Destroyed { .. }
    ));
    assert_eq!(s.operations[1].phase, DroneBayPhase::Destroyed);
    assert!(s.handling_operation.is_none());
    assert!(s.cleanup.is_some());
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    assert_eq!((w.pilots["Pilot"].view.ore, w.belt_ore), ore_before);
    let projection = w.pilots["Pilot"].view.drone_bay.as_ref().unwrap();
    assert!(projection.motions.is_empty());
    assert!(projection.machinery.cleanup.is_some());
    f.server.disconnect(&mut f.session);
    assert!(
        state(&f.world(), "Pilot")
            .unwrap()
            .cleanup
            .as_ref()
            .unwrap()
            .held
    );
    f.ok(connect(Some(f.token.clone())));
    f.ok(f.control("resume"));
    f.finish();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.counts(None).destroyed, 1);
    assert_eq!(s.counts(None).stored, 14);
    assert!(s.cleanup.is_none());
    assert_eq!(s.operations[1].phase, DroneBayPhase::Destroyed);
    assert_eq!(w.next_drone_id, 31);
    f.ok(recovery);
    assert!(f.world().pilots["Pilot"].view.drones.is_empty());
    let retry = f.request(DroneBayOperationKind::Launch, victim);
    assert!(!f.server.handle_at(&mut f.session, retry, f.now).ok);
}
#[test]
fn real_resource_job_can_mine_recover_and_unload_with_atomic_finite_stock() {
    let mut f = Fixture::new();
    f.ok(Request::StartResourceJob {
        target_units: 100,
        pilot: None,
    });
    let mut done = false;
    for _ in 0..2400 {
        f.now += 500;
        f.server.tick_at(f.now).unwrap();
        let w = f.world();
        let j = &w.resource_jobs["Pilot"].view;
        if j.phase == ResourceJobPhase::Completed {
            done = true;
            break;
        }
        assert_ne!(j.phase, ResourceJobPhase::Blocked, "{}", j.message);
    }
    let w = f.world();
    assert!(done, "{}", w.resource_jobs["Pilot"].view.message);
    assert!(w.resource_jobs["Pilot"].view.stored_units >= 100);
    assert_eq!(state(&w, "Pilot").unwrap().counts(None).stored, 15);
    assert_eq!(w.next_drone_id, 16);
    assert!(w.pilots["Pilot"].view.drones.is_empty());
    let report = arkline_verifier::verify(&f.ok(Request::ExportLedger).ledger.unwrap(), None);
    assert!(report.is_ok(), "{report:?}");
}

#[test]
fn dynamic_peer_obstruction_holds_confirmed_pose_and_resume_uses_same_route() {
    let mut f = Fixture::new();
    f.space();
    let ids: Vec<_> = state(&f.world(), "Pilot")
        .unwrap()
        .stock
        .iter()
        .take(2)
        .map(|e| e.drone_id)
        .collect();
    for id in &ids {
        f.ok(f.request(DroneBayOperationKind::Launch, *id));
        f.finish();
    }
    f.ok(f.request(DroneBayOperationKind::Recover, ids[0]));
    let mut w = f.world();
    let route = serde_json::to_value(
        w.accounts["Pilot"]
            .drone_bay_runtime
            .values()
            .next()
            .unwrap(),
    )
    .unwrap();
    let source: BayPose = serde_json::from_value(route["source"]["drone"].clone()).unwrap();
    let target: BayPose = serde_json::from_value(route["points"][1]["drone"].clone()).unwrap();
    let parent: BayPose = serde_json::from_value(route["parent"].clone()).unwrap();
    let midpoint = BayPose {
        position_m: std::array::from_fn(|i| (source.position_m[i] + target.position_m[i]) * 0.5),
        ..source
    };
    let obstacle = parent.compose(midpoint).unwrap().position_m;
    let d = w
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == ids[1])
        .unwrap();
    let original = d.position;
    d.position = obstacle;
    d.updated_at_ms = f.now;
    f.server
        .db
        .execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
    let position = f.world().pilots["Pilot"]
        .view
        .drones
        .iter()
        .find(|d| d.id == ids[0])
        .unwrap()
        .position;
    f.step();
    let w = f.world();
    assert_eq!(
        state(&w, "Pilot").unwrap().operations.last().unwrap().phase,
        DroneBayPhase::Held
    );
    assert_eq!(
        w.pilots["Pilot"]
            .view
            .drones
            .iter()
            .find(|d| d.id == ids[0])
            .unwrap()
            .position,
        position
    );
    let resume = f.control("resume");
    assert!(!f.server.handle_at(&mut f.session, resume, f.now).ok);
    let mut w = f.world();
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == ids[1])
        .unwrap()
        .position = original;
    f.server
        .db
        .execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
    f.ok(f.control("resume"));
    f.finish();
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().stock[0].status,
        DroneStockStatus::Stored { hit_points: 100 }
    );
}
#[test]
fn active_long_gap_holds_without_offline_completion() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[3].drone_id;
    f.ok(f.request(DroneBayOperationKind::Launch, id));
    for _ in 0..15 {
        f.step();
    }
    let position = f.world().pilots["Pilot"].view.drones[0].position;
    f.now += 20_000;
    f.server.tick_at(f.now).unwrap();
    let w = f.world();
    assert_eq!(
        state(&w, "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
    assert_eq!(w.pilots["Pilot"].view.drones[0].position, position);
    f.ok(f.control("resume"));
    f.finish();
}
#[test]
fn real_remote_lease_uses_worker_stock_but_keeps_controller_private_and_holds_on_loss() {
    let mut f = Fixture::new();
    f.space();
    let mut controller = None;
    let mut connect_controller = connect(None);
    if let Request::Connect { pilot, .. } = &mut connect_controller {
        *pilot = "Controller".into();
    }
    let r = f
        .server
        .handle_at(&mut controller, connect_controller, f.now);
    assert!(r.ok, "{}", r.message);
    let controller_token = r.resume_token.unwrap();
    f.ok(Request::SetFleetController {
        controller: Some("Controller".into()),
    });
    let r = f.server.handle_at(
        &mut controller,
        Request::BeginRemoteControl {
            pilot: "Pilot".into(),
        },
        f.now,
    );
    assert!(r.ok, "{}", r.message);
    let id = state(&f.world(), "Pilot").unwrap().stock[0].drone_id;
    let launch = f.request(DroneBayOperationKind::Launch, id);
    let r = f.server.handle_at(
        &mut controller,
        Request::RemoteControl {
            pilot: "Pilot".into(),
            request: Box::new(launch),
        },
        f.now,
    );
    assert!(r.ok, "{}", r.message);
    assert_eq!(
        r.assets.unwrap().drone_bay.unwrap().binding.vessel_id,
        "v1:Controller:Pioneer"
    );
    assert_eq!(state(&f.world(), "Pilot").unwrap().counts(None).active, 1);
    f.server.disconnect(&mut controller);
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
    f.ok(f.control("resume"));
    let mut reconnect = connect(Some(controller_token));
    if let Request::Connect { pilot, .. } = &mut reconnect {
        *pilot = "Controller".into();
    }
    assert!(f.server.handle_at(&mut controller, reconnect, f.now).ok);
    assert!(
        f.server
            .handle_at(
                &mut controller,
                Request::BeginRemoteControl {
                    pilot: "Pilot".into()
                },
                f.now
            )
            .ok
    );
    f.server.disconnect(&mut f.session);
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().operations[0].phase,
        DroneBayPhase::Held
    );
    assert_eq!(
        state(&f.world(), "Controller").unwrap().counts(None).stored,
        15
    );
}

#[test]
fn recovery_cancel_crossing_door_is_continuous_and_keeps_actionable_operation_until_safe() {
    let mut f = Fixture::new();
    f.space();
    let id = state(&f.world(), "Pilot").unwrap().stock[7].drone_id;
    f.ok(f.request(DroneBayOperationKind::Launch, id));
    f.finish();
    let original = f.world().pilots["Pilot"].view.drones[0].position;
    f.ok(f.request(DroneBayOperationKind::Recover, id));
    for _ in 0..500 {
        if state(&f.world(), "Pilot")
            .unwrap()
            .operations
            .last()
            .unwrap()
            .phase
            == DroneBayPhase::Entering
        {
            break;
        }
        f.step();
    }
    assert_eq!(
        state(&f.world(), "Pilot")
            .unwrap()
            .operations
            .last()
            .unwrap()
            .phase,
        DroneBayPhase::Entering
    );
    for _ in 0..12 {
        f.step();
    }
    let position = f.world().pilots["Pilot"].view.drones[0].position;
    f.ok(f.control("cancel"));
    assert_eq!(position, f.world().pilots["Pilot"].view.drones[0].position);
    let before = f.raw();
    f.ok(f.control("cancel"));
    assert_eq!(before, f.raw());
    assert!(!state(&f.world(), "Pilot")
        .unwrap()
        .operations
        .last()
        .unwrap()
        .phase
        .terminal());
    f.ok(f.control("hold"));
    f.ok(f.control("resume"));
    let before = f.raw();
    f.ok(f.control("resume"));
    assert_eq!(before, f.raw());
    f.finish();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.operations.last().unwrap().phase, DroneBayPhase::Cancelled);
    assert!(s.handling_operation.is_none());
    assert_eq!(s.stock[7].status, DroneStockStatus::Active);
    assert_eq!(w.pilots["Pilot"].view.drones[0].id, id);
    assert!(flight::distance(w.pilots["Pilot"].view.drones[0].position, original) < 1e-8);
    f.ok(f.request(DroneBayOperationKind::Recover, id));
    f.finish();
    assert_eq!(
        state(&f.world(), "Pilot").unwrap().stock[7].status,
        DroneStockStatus::Stored { hit_points: 100 }
    );
}
#[test]
fn destruction_during_cancellation_continues_toward_idle_not_back_to_midroute() {
    for held in [false, true] {
        let mut f = Fixture::new();
        f.space();
        let id = state(&f.world(), "Pilot").unwrap().stock[0].drone_id;
        f.ok(f.request(DroneBayOperationKind::Launch, id));
        for _ in 0..100 {
            f.step();
        }
        f.ok(f.control("cancel"));
        for _ in 0..5 {
            f.step();
        }
        if held {
            f.ok(f.control("hold"));
        }
        // Exercise the post-combat transition directly; real weapon/capture
        // ordering is covered by the separate incoming-combat Server test.
        let mut w = f.world();
        w.pilots.get_mut("Pilot").unwrap().view.drones[0].hit_points = 0;
        finish(&mut w, f.now).unwrap();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.operations[0].phase, DroneBayPhase::Destroyed);
        assert_eq!(s.cleanup.as_ref().unwrap().held, held);
        let r = serde_json::to_value(
            w.accounts["Pilot"]
                .drone_bay_runtime
                .values()
                .next()
                .unwrap(),
        )
        .unwrap();
        let groups: Vec<BayPose> = serde_json::from_value(
            r["points"].as_array().unwrap().last().unwrap()["groups"].clone(),
        )
        .unwrap();
        assert_eq!(
            groups,
            arkline_protocol::drone_bay_layout::reference_poses()
        );
        f.server
            .db
            .execute(
                "UPDATE world SET json=?1",
                [serde_json::to_string(&w).unwrap()],
            )
            .unwrap();
        if held {
            f.ok(f.control("resume"));
        }
        f.finish();
        let w = f.world();
        assert_eq!(state(&w, "Pilot").unwrap().counts(None).destroyed, 1);
        assert!(state(&w, "Pilot").unwrap().cleanup.is_none());
        assert!(w.pilots["Pilot"].view.drones.is_empty());
    }
}
#[test]
fn missing_initialization_receipt_cannot_turn_new_world_into_free_spawn() {
    let mut f = Fixture::new();
    let mut w = f.world();
    w.accounts.get_mut("Pilot").unwrap().drone_bays.clear();
    let raw = serde_json::to_string(&w).unwrap();
    f.server
        .db
        .execute("UPDATE world SET json=?1", [&raw])
        .unwrap();
    let response = f.server.handle_at(&mut f.session, Request::Snapshot, f.now);
    assert!(!response.ok);
    assert!(response.message.contains("initialization receipt"));
    assert_eq!(raw, f.raw());
}

#[test]
fn identified_hold_and_cleanup_hold_preserve_unrelated_free_drone_work() {
    let mut f = Fixture::new();
    f.space();
    f.ok(f.request(DroneBayOperationKind::Launch, 1));
    f.finish();
    f.ok(f.request(DroneBayOperationKind::Launch, 2));
    let mut w = f.world();
    let free = w
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == 1)
        .unwrap();
    free.mode = DroneMode::FlyTo {
        position: [1e6, 2e6, 3e6],
    };
    free.velocity = [3., 2., 1.];
    free.attack_ready_at_ms = Some(f.now + 500);
    let before = serde_json::to_value(&*free).unwrap();
    hold(&mut w, "Pilot", f.now, "identified hold");
    assert_eq!(
        serde_json::to_value(&w.pilots["Pilot"].view.drones[0]).unwrap(),
        before
    );
    // Destroyed cleanup has no live constrained drone to stop, and must not
    // silently stop an unrelated miner or attacker either.
    w.pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == 2)
        .unwrap()
        .hit_points = 0;
    finish(&mut w, f.now).unwrap();
    assert!(state(&w, "Pilot").unwrap().cleanup.is_some());
    hold(&mut w, "Pilot", f.now + 100, "cleanup hold");
    assert_eq!(
        serde_json::to_value(&w.pilots["Pilot"].view.drones[0]).unwrap(),
        before
    );
    stop_all(&mut w, "Pilot", f.now + 200, "source disconnected");
    let free = &w.pilots["Pilot"].view.drones[0];
    assert_eq!(free.mode, DroneMode::Hold);
    assert_eq!(free.velocity, [0.; 3]);
    assert!(free.attack_ready_at_ms.is_none());
}

#[test]
fn free_peer_pending_same_tick_reach_holds_bay_before_sampling() {
    let mut f = Fixture::new();
    f.space();
    for id in [1, 2] {
        f.ok(f.request(DroneBayOperationKind::Launch, id));
        f.finish();
    }
    f.ok(f.request(DroneBayOperationKind::Recover, 1));
    let mut w = f.world();
    let r = serde_json::to_value(
        w.accounts["Pilot"]
            .drone_bay_runtime
            .values()
            .next()
            .unwrap(),
    )
    .unwrap();
    let parent: BayPose = serde_json::from_value(r["parent"].clone()).unwrap();
    let a: BayPose = serde_json::from_value(r["source"]["drone"].clone()).unwrap();
    let b: BayPose = serde_json::from_value(r["points"][1]["drone"].clone()).unwrap();
    let offset = BayPose {
        position_m: [
            (a.position_m[0] + b.position_m[0]) * 0.5,
            (a.position_m[1] + b.position_m[1]) * 0.5 + 20.,
            (a.position_m[2] + b.position_m[2]) * 0.5,
        ],
        ..a
    };
    let obstacle = parent.compose(offset).unwrap().position_m;
    let target = parent
        .compose(BayPose {
            position_m: [
                offset.position_m[0],
                offset.position_m[1] - 40.,
                offset.position_m[2],
            ],
            ..a
        })
        .unwrap()
        .position_m;
    let d = w
        .pilots
        .get_mut("Pilot")
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == 2)
        .unwrap();
    d.position = obstacle;
    d.mode = DroneMode::FlyTo { position: target };
    d.updated_at_ms = f.now - 1000;
    // Its current point is outside both body spheres, but the pending free
    // integration can cross the route in this same authority tick.
    f.server
        .db
        .execute(
            "UPDATE world SET json=?1",
            [serde_json::to_string(&w).unwrap()],
        )
        .unwrap();
    let before = w.pilots["Pilot"]
        .view
        .drones
        .iter()
        .find(|d| d.id == 1)
        .unwrap()
        .position;
    f.step();
    let after = f.world();
    assert_eq!(
        state(&after, "Pilot")
            .unwrap()
            .operations
            .last()
            .unwrap()
            .phase,
        DroneBayPhase::Held
    );
    assert_eq!(
        after.pilots["Pilot"]
            .view
            .drones
            .iter()
            .find(|d| d.id == 1)
            .unwrap()
            .position,
        before
    );
}

#[path = "batch_tests.rs"]
mod batch_tests;
