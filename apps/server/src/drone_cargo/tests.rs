use super::*;
struct Fixture {
    server: Server,
    session: Option<String>,
    now: u64,
    path: std::path::PathBuf,
    token: String,
}
fn connect(version: u32, token: Option<String>) -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        world_layout_version: version,
        supports_room_controls: true,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: true,
        pilot: "Pilot".into(),
        resume_token: token,
    }
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "arkline-ore-receiver-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("world.sqlite");
        let mut server =
            Server::open_editor_ore_receiver_realm(&path, default_pioneer_room_layout()).unwrap();
        let now = server.editor_time.unwrap();
        let mut session = None;
        let r = server.handle_at(&mut session, connect(5, None), now);
        assert!(r.ok, "{}", r.message);
        Self {
            server,
            session,
            now,
            path,
            token: r.resume_token.unwrap(),
        }
    }
    fn ok(&mut self, r: Request) -> Response {
        let r = self.server.handle_at(&mut self.session, r, self.now);
        assert!(r.ok, "{}", r.message);
        r
    }
    fn raw(&self) -> String {
        self.server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap()
    }
    fn world(&self) -> World {
        serde_json::from_str(&self.raw()).unwrap()
    }
    fn step(&mut self) {
        self.now += 100;
        self.server.tick_at(self.now).unwrap();
    }
    fn until(&mut self, n: usize, condition: impl Fn(&World) -> bool) {
        for _ in 0..n {
            if condition(&self.world()) {
                return;
            }
            self.step();
        }
        panic!("condition not met: {}", self.raw());
    }
    fn space(&mut self) {
        self.ok(Request::Undock);
        self.until(200, |w| {
            w.pilots["Pilot"]
                .view
                .flight
                .as_ref()
                .unwrap()
                .docking
                .as_ref()
                .unwrap()
                .phase
                == DockingPhase::Free
        });
    }
    fn launch(&mut self, id: u64) {
        let w = self.world();
        let s = drone_bay::state(&w, "Pilot").unwrap();
        self.ok(Request::DroneBay {
            request: DroneBayRequest::Launch {
                binding: s.binding.clone(),
                operation_id: s.high_watermark + 1,
                drone_id: id,
            },
        });
        self.until(1000, |w| !drone_bay::busy(w, "Pilot"));
    }
    fn prepared_miner(&mut self) -> u64 {
        self.space();
        let w = self.world();
        let id = state(&w, "Pilot").unwrap().holds[0].drone_id;
        self.launch(id);
        // Explicit disposable fixture: move one actual finite catalog asteroid,
        // not its ID, ore reserve, balances, cycles, cargo or drone pose.
        let mut w = self.world();
        let f = w.pilots["Pilot"].view.flight.as_ref().unwrap();
        let a = w
            .asteroids
            .iter_mut()
            .find(|a| a.system_id == HOME_SYSTEM_ID)
            .unwrap();
        a.position = arkline_protocol::drone_bay_geometry::BayPose {
            position_m: f.position,
            orientation_xyzw: f.orientation,
        }
        .compose(arkline_protocol::drone_bay_geometry::BayPose {
            position_m: [-1200., 100., 800.],
            orientation_xyzw: [0., 0., 0., 1.],
        })
        .unwrap()
        .position_m;
        assert!(
            flight::distance(a.position, STATION_CENTER_POSITION)
                > STATION_DIAMETER_M / 2. + a.radius + 5.
        );
        let target = a.id;
        self.server
            .db
            .execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&w).unwrap()],
            )
            .unwrap();
        self.ok(Request::LockAsteroid { id: target });
        self.until(300, |w| {
            w.pilots["Pilot"].view.locked_asteroids.contains(&target)
        });
        self.ok(Request::AssignMiningDrone {
            id,
            target_id: target,
        });
        id
    }
    fn request(&self, id: u64, resume: bool) -> Request {
        let w = self.world();
        let s = state(&w, "Pilot").unwrap();
        Request::DroneCargo {
            request: DroneCargoRequest::BeginDropoff {
                binding: s.binding.clone(),
                operation_id: s.high_watermark + 1,
                drone_id: id,
                resume_mining: resume,
            },
        }
    }
    fn verify(&mut self) {
        let ledger = self.ok(Request::ExportLedger).ledger.unwrap();
        arkline_verifier::verify(&ledger, None).unwrap();
    }
}
#[test]
fn fresh_cargo_initialization_is_finite_and_old_capability_refusal_is_nonmutating() {
    let mut f = Fixture::new();
    let w = f.world();
    assert_eq!(state(&w, "Pilot").unwrap().holds.len(), 5);
    assert_eq!(totals(&w).unwrap().unwrap().drone_ore_units, 0);
    let raw = f.raw();
    let mut old = None;
    let r = f
        .server
        .handle_at(&mut old, connect(4, Some(f.token.clone())), f.now);
    assert!(!r.ok);
    assert!(old.is_none());
    assert_eq!(raw, f.raw());
    let sql = "UPDATE world SET json=json_remove(json,'$.drone_cargo_registration') WHERE id=1";
    assert!(f.server.db.execute(sql, []).is_err());
    assert_eq!(raw, f.raw());
    f.verify();
    let p = w.pilots["Pilot"].view.clone();
    p.drone_cargo
        .as_ref()
        .unwrap()
        .validate_projection(&p)
        .unwrap();
}
#[test]
fn actual_extraction_owns_drone_payload_then_confirmed_receiver_batches_credit_ship() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(500, |w| payload(w, "Pilot", id) >= 20);
    let before = f.world();
    assert_eq!(before.pilots["Pilot"].view.ore, 0);
    let units = payload(&before, "Pilot", id);
    assert!(units >= 20);
    let request = f.request(id, true);
    f.ok(request.clone());
    f.until(1500, |w| {
        state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Completed
    });
    let w = f.world();
    let op = state(&w, "Pilot").unwrap().operation(1).unwrap();
    assert_eq!(op.committed_units, units);
    assert_eq!(w.pilots["Pilot"].view.ore, units);
    assert_eq!(payload(&w, "Pilot", id), 0);
    assert_eq!(
        w.pilots["Pilot"]
            .view
            .drones
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .hit_points,
        100
    );
    assert!(matches!(
        w.pilots["Pilot"]
            .view
            .drones
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .mode,
        DroneMode::Mining { .. }
    ));
    let receipts = op.receipts.clone();
    f.ok(request);
    assert_eq!(
        state(&f.world(), "Pilot")
            .unwrap()
            .operation(1)
            .unwrap()
            .receipts,
        receipts
    );
    f.verify();
}

impl Fixture {
    fn reopen(&mut self) {
        let placeholder = Server::open(":memory:").unwrap();
        drop(std::mem::replace(&mut self.server, placeholder));
        self.server =
            Server::open_editor_ore_receiver_realm(&self.path, default_pioneer_room_layout())
                .unwrap();
        self.session = None;
        self.ok(connect(5, Some(self.token.clone())));
    }
    fn control(&self, id: u64, kind: &str) -> Request {
        let binding = state(&self.world(), "Pilot").unwrap().binding.clone();
        Request::DroneCargo {
            request: match kind {
                "hold" => DroneCargoRequest::Hold {
                    binding,
                    operation_id: id,
                },
                "resume" => DroneCargoRequest::Resume {
                    binding,
                    operation_id: id,
                },
                _ => DroneCargoRequest::Cancel {
                    binding,
                    operation_id: id,
                },
            },
        }
    }
    fn phase(&self, id: u64) -> DroneDropoffPhase {
        state(&self.world(), "Pilot")
            .unwrap()
            .operation(id)
            .unwrap()
            .phase
    }
}
#[test]
fn restart_holds_real_receiver_pose_and_resuming_does_not_revive_cancelled_mining_intent() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(500, |w| payload(w, "Pilot", id) >= 20);
    let req = f.request(id, true);
    f.ok(req);
    for _ in 0..20 {
        f.step();
    }
    let w = f.world();
    let before = w.pilots["Pilot"]
        .view
        .drones
        .iter()
        .find(|d| d.id == id)
        .unwrap()
        .position;
    let ore = payload(&w, "Pilot", id);
    f.reopen();
    assert_eq!(f.phase(1), DroneDropoffPhase::Held);
    for _ in 0..20 {
        f.step();
    }
    let w = f.world();
    assert_eq!(
        w.pilots["Pilot"]
            .view
            .drones
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .position,
        before
    );
    assert_eq!(payload(&w, "Pilot", id), ore);
    f.ok(f.control(1, "resume"));
    let first = f.raw();
    f.ok(f.control(1, "resume"));
    assert_eq!(f.raw(), first, "repeated Resume cannot reset time");
    f.until(1800, |w| {
        state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Completed
    });
    assert_eq!(
        f.world().pilots["Pilot"]
            .view
            .drones
            .iter()
            .find(|d| d.id == id)
            .unwrap()
            .mode,
        DroneMode::Hold
    );
    f.verify();
}
#[test]
fn transfer_append_and_inventory_receipt_rollback_as_one_transaction() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(500, |w| payload(w, "Pilot", id) >= 20);
    let req = f.request(id, false);
    f.ok(req.clone());
    f.until(1800, |w| {
        state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Transferring
    });
    let w = f.world();
    let due = state(&w, "Pilot")
        .unwrap()
        .operation(1)
        .unwrap()
        .next_batch_at_ms
        .unwrap();
    let before = f.raw();
    let count: i64 = f
        .server
        .db
        .query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get(0))
        .unwrap();
    f.server.db.execute_batch("CREATE TRIGGER cargo_after_append_fault BEFORE UPDATE OF json ON world BEGIN SELECT RAISE(ABORT,'test fault after ledger append'); END;").unwrap();
    assert!(f.server.tick_at(due).is_err());
    assert_eq!(f.raw(), before);
    assert_eq!(
        f.server
            .db
            .query_row("SELECT COUNT(*) FROM ledger", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        count
    );
    f.server
        .db
        .execute_batch("DROP TRIGGER cargo_after_append_fault;")
        .unwrap();
    f.now = due;
    f.server.tick_at(due).unwrap();
    assert_eq!(
        state(&f.world(), "Pilot")
            .unwrap()
            .operation(1)
            .unwrap()
            .receipts
            .len(),
        1
    );
    f.reopen();
    let credited = f.world().pilots["Pilot"].view.ore;
    f.ok(req);
    assert_eq!(f.world().pilots["Pilot"].view.ore, credited);
    f.ok(f.control(1, "resume"));
    f.until(500, |w| {
        state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Completed
    });
    f.verify();
}
#[test]
fn loaded_recall_all_retains_service_then_storage_receipts_and_only_captures_empty() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(500, |w| payload(w, "Pilot", id) >= 20);
    let w = f.world();
    let units = payload(&w, "Pilot", id);
    let binding = drone_bay::state(&w, "Pilot").unwrap().binding.clone();
    f.ok(Request::DroneBay {
        request: DroneBayRequest::BeginBatch {
            binding,
            batch_id: 1,
            batch_kind: DroneBayBatchKind::RecallAll,
        },
    });
    let w = f.world();
    let b = drone_bay::state(&w, "Pilot")
        .unwrap()
        .batches
        .as_ref()
        .unwrap()
        .operation(1)
        .unwrap();
    assert_eq!(b.members[0].status, DroneBayBatchMemberStatus::Servicing);
    assert!(b.members[0].operation_id.is_none());
    assert_eq!(b.members[0].service_operation_ids, vec![1]);
    f.until(2500, |w| !drone_bay::batch_active(w, "Pilot"));
    let w = f.world();
    let b = drone_bay::state(&w, "Pilot")
        .unwrap()
        .batches
        .as_ref()
        .unwrap()
        .operation(1)
        .unwrap();
    assert_eq!(b.members[0].status, DroneBayBatchMemberStatus::Stored);
    assert!(b.members[0].operation_id.is_some());
    assert_eq!(b.members[0].service_operation_ids, vec![1]);
    assert_eq!(w.pilots["Pilot"].view.ore, units);
    assert_eq!(payload(&w, "Pilot", id), 0);
    assert_eq!(
        drone_bay::state(&w, "Pilot").unwrap().counts(None).stored,
        15
    );
    f.verify();
}
#[test]
#[ignore = "exports an actual fresh server response for native consumer acceptance"]
fn export_initial_ore_receiver_response() {
    let mut f = Fixture::new();
    let mut response = f.ok(Request::Snapshot);
    response.resume_token = None;
    let directory = std::path::PathBuf::from(
        std::env::var("ARKLINE_ORE_RESPONSE_OUT").expect("explicit output directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let bytes = serde_json::to_vec_pretty(&response).unwrap();
    std::fs::write(directory.join("initial.json"), &bytes).unwrap();
    std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"producer_commit":std::env::var("ARKLINE_PRODUCER_COMMIT").unwrap(),"pilot":"Pilot","response_sha256":hex::encode(Sha256::digest(&bytes)),"status":"preliminary real fresh cargo initialization; continuous journey not claimed"})).unwrap()).unwrap();
    f.verify();
}

#[test]
fn receiver_owns_legacy_first_drone_controls_and_batch_controls() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(500, |w| payload(w, "Pilot", id) >= 20);
    let binding = drone_bay::state(&f.world(), "Pilot")
        .unwrap()
        .binding
        .clone();
    f.ok(Request::DroneBay {
        request: DroneBayRequest::BeginBatch {
            binding: binding.clone(),
            batch_id: 1,
            batch_kind: DroneBayBatchKind::RecallAll,
        },
    });
    for held in [false, true] {
        if held {
            f.ok(Request::DroneBay {
                request: DroneBayRequest::HoldBatch {
                    binding: binding.clone(),
                    batch_id: 1,
                },
            });
        }
        for request in [
            Request::DroneHold,
            Request::DroneNavigate {
                position: [1., 2., 3.],
            },
            f.control(1, "resume"),
            f.control(1, "cancel"),
            f.control(1, "hold"),
        ] {
            let before = f.raw();
            let r = f.server.handle_at(&mut f.session, request, f.now);
            assert!(!r.ok, "service ownership must reject command");
            assert_eq!(f.raw(), before, "refused command must not mutate authority");
        }
    }
    assert_eq!(f.phase(1), DroneDropoffPhase::Held);
    f.ok(Request::DroneBay {
        request: DroneBayRequest::ResumeBatch {
            binding,
            batch_id: 1,
        },
    });
    assert_ne!(f.phase(1), DroneDropoffPhase::Held);
    f.verify();
}

#[test]
fn malformed_saved_receiver_ready_or_phase_rejects_without_rewriting() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(500, |w| payload(w, "Pilot", id) >= 20);
    f.ok(f.request(id, false));
    let valid = f.raw();
    let vessel = state(&f.world(), "Pilot")
        .unwrap()
        .binding
        .vessel_id
        .clone();
    for case in [
        "ready",
        "phase",
        "identity",
        "future_phase",
        "future_body",
        "future_hardware",
        "source",
        "clock",
        "index",
        "carrier_pose",
        "carrier_motion",
        "orphan",
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&valid).unwrap();
        let run = &mut value["accounts"]["Pilot"]["drone_receivers"][&vessel];
        match case {
            "ready" => run["ready"] = true.into(),
            "phase" => {
                run["phases"][0] = serde_json::to_value(DroneDropoffPhase::Coupling).unwrap()
            }
            "identity" => run["drone_id"] = 999_u64.into(),
            "future_phase" => {
                let n = run["phases"].as_array().unwrap().len();
                run["phases"][n - 1] =
                    serde_json::to_value(DroneDropoffPhase::Transferring).unwrap();
            }
            "future_body" => {
                let n = run["points"].as_array().unwrap().len();
                run["points"][n - 5]["drone"]["position_m"][1] = 3.0.into();
            }
            "future_hardware" => {
                let n = run["points"].as_array().unwrap().len();
                run["points"][n - 4]["groups"][1]["position_m"][1] = 3.0.into();
            }
            "source" => run["source"]["groups"][0]["position_m"][1] = 3.0.into(),
            "clock" => run["confirmed_at_ms"] = (run["updated_at_ms"].as_u64().unwrap() + 1).into(),
            "index" => run["index"] = 999_u64.into(),
            "carrier_pose" => {
                let position = &mut value["pilots"]["Pilot"]["view"]["flight"]["position"][0];
                *position = (position.as_f64().unwrap() + 1.).into();
            }
            "carrier_motion" => {
                value["pilots"]["Pilot"]["view"]["flight"]["velocity"][0] = 1.0.into()
            }
            _ => {
                let extra = run.clone();
                value["accounts"]["Pilot"]["drone_receivers"]["other-vessel"] = extra;
            }
        }
        let malformed = serde_json::to_string(&value).unwrap();
        f.server
            .db
            .execute("UPDATE world SET json=?1 WHERE id=1", [&malformed])
            .unwrap();
        let result = Server::open_editor_ore_receiver_realm(&f.path, default_pioneer_room_layout());
        assert!(result.is_err(), "must reject {case}");
        assert_eq!(f.raw(), malformed, "failed open must preserve {case} bytes");
    }
    f.server
        .db
        .execute("UPDATE world SET json=?1 WHERE id=1", [&valid])
        .unwrap();
}

#[test]
#[ignore = "exports sampled real authority milestones, not a continuous movie"]
fn export_actual_receiver_landmarks() {
    let directory = std::path::PathBuf::from(
        std::env::var("ARKLINE_ORE_RESPONSE_OUT").expect("explicit output directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let mut f = Fixture::new();
    let start = f.now;
    let mut rows = Vec::new();
    let mut save = |f: &mut Fixture, label: &str| {
        let mut response = f.ok(Request::Snapshot);
        response.resume_token = None;
        let index = rows.len();
        let file = format!("{index:04}-{label}.json");
        let bytes = serde_json::to_vec_pretty(&response).unwrap();
        std::fs::write(directory.join(&file), &bytes).unwrap();
        rows.push(serde_json::json!({"file":file,"response_sha256":hex::encode(Sha256::digest(&bytes)),"index":index,"elapsed_ms":f.now-start,"label":label}));
    };
    save(&mut f, "initial");
    let id = f.prepared_miner();
    f.until(700, |w| payload(w, "Pilot", id) >= 40);
    save(&mut f, "mined");
    f.ok(f.request(id, true));
    for _ in 0..10 {
        f.step();
    }
    save(&mut f, "approach");
    f.until(1800, |w| {
        state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Transferring
    });
    save(&mut f, "coupled");
    f.until(10, |w| {
        !state(w, "Pilot")
            .unwrap()
            .operation(1)
            .unwrap()
            .receipts
            .is_empty()
    });
    save(&mut f, "one-transfer");
    f.until(800, |w| {
        state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Completed
    });
    save(&mut f, "cleared");
    let ledger = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&ledger, None).unwrap();
    std::fs::write(
        directory.join("ledger.json"),
        serde_json::to_vec_pretty(&ledger).unwrap(),
    )
    .unwrap();
    let op = state(&f.world(), "Pilot")
        .unwrap()
        .operation(1)
        .unwrap()
        .clone();
    assert_eq!(op.committed_units, 40);
    std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "producer_commit":std::env::var("ARKLINE_PRODUCER_COMMIT").unwrap(),"pilot":"Pilot","frames":rows,"drone_id":id,"operation":op,
        "fixture":"One actual finite catalog asteroid repositioned in disposable world at carrier-body [-1200,100,800]m; ore, IDs, poses, rates and balances unchanged. Actual undock, launch, lock, mining, drop-off requests and 100ms ticks.",
        "status":"preliminary actual sampled authority landmarks; not continuous cadence or final acceptance","elapsed_ms":f.now-start
    })).unwrap()).unwrap();
}

#[test]
fn cancellation_at_each_receiver_phase_preserves_ore_and_returns_hardware_without_snap() {
    for phase in [
        DroneDropoffPhase::Approaching,
        DroneDropoffPhase::Aligning,
        DroneDropoffPhase::Coupling,
        DroneDropoffPhase::Transferring,
        DroneDropoffPhase::Decoupling,
        DroneDropoffPhase::Egress,
    ] {
        let mut f = Fixture::new();
        let id = f.prepared_miner();
        f.until(500, |w| payload(w, "Pilot", id) >= 20);
        f.ok(f.request(id, true));
        f.until(1800, |w| {
            state(w, "Pilot").unwrap().operation(1).unwrap().phase == phase
        });
        // Advance into a stroke, except transfer which has a deliberately short payload.
        if phase != DroneDropoffPhase::Transferring {
            f.step();
        }
        let before = f.world();
        let position = before.pilots["Pilot"].view.drones[0].position;
        let total = before.pilots["Pilot"].view.ore + payload(&before, "Pilot", id);
        f.ok(f.control(1, "hold"));
        f.ok(f.control(1, "cancel"));
        assert_eq!(
            f.world().pilots["Pilot"].view.drones[0].position,
            position,
            "cancel must not snap at {phase:?}"
        );
        let once = f.raw();
        f.ok(f.control(1, "cancel"));
        assert_eq!(f.raw(), once);
        assert_eq!(f.phase(1), DroneDropoffPhase::Held);
        f.reopen();
        f.ok(f.control(1, "resume"));
        f.until(2000, |w| {
            state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Cancelled
        });
        let w = f.world();
        assert_eq!(w.pilots["Pilot"].view.ore + payload(&w, "Pilot", id), total);
        assert!(!busy(&w, "Pilot"));
        assert_eq!(w.pilots["Pilot"].view.drones[0].mode, DroneMode::Hold);
        f.ok(f.control(1, "resume"));
        assert_eq!(f.phase(1), DroneDropoffPhase::Cancelled);
        f.verify();
    }
}

#[test]
#[ignore = "exports continuous natural seeded-belt launch, ore service, resume and full recall"]
fn export_actual_seeded_belt_ore_service_journey() {
    let directory = std::path::PathBuf::from(
        std::env::var("ARKLINE_ORE_RESPONSE_OUT").expect("explicit output directory"),
    );
    std::fs::create_dir_all(&directory).unwrap();
    let mut f = Fixture::new();
    f.space();
    f.ok(Request::Warp {
        destination: Destination::Belt,
    });
    f.until(1000, |w| w.pilots["Pilot"].view.location == Location::Belt);
    let before = f.world();
    let ship = before.pilots["Pilot"].view.flight.as_ref().unwrap();
    let target = before
        .asteroids
        .iter()
        .filter(|a| a.system_id == HOME_SYSTEM_ID && a.ore > 0)
        .min_by(|a, b| {
            flight::distance(a.position, ship.position)
                .total_cmp(&flight::distance(b.position, ship.position))
        })
        .unwrap()
        .id;
    f.ok(Request::LockAsteroid { id: target });
    f.until(300, |w| {
        w.pilots["Pilot"].view.locked_asteroids.contains(&target)
    });
    let start = f.now;
    let mut rows = Vec::new();
    let mut landmarks = Vec::new();
    let mut save = |f: &mut Fixture| {
        let mut response = f.ok(Request::Snapshot);
        response.resume_token = None;
        let index = rows.len();
        let file = format!("response-{index:05}.json");
        let bytes = serde_json::to_vec(&response).unwrap();
        std::fs::write(directory.join(&file), &bytes).unwrap();
        rows.push(serde_json::json!({"file":file,"index":index,"elapsed_ms":f.now-start,"response_sha256":hex::encode(Sha256::digest(&bytes))}));
    };
    save(&mut f);
    let w = f.world();
    let bay = drone_bay::state(&w, "Pilot").unwrap();
    let id = state(&w, "Pilot").unwrap().holds[0].drone_id;
    let original = bay.stock.clone();
    f.ok(Request::DroneBay {
        request: DroneBayRequest::Launch {
            binding: bay.binding.clone(),
            operation_id: bay.high_watermark + 1,
            drone_id: id,
        },
    });
    let mut stage = 0;
    let mut finish_at = None;
    for index in 1..=7000 {
        f.step();
        save(&mut f);
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        let bay = drone_bay::state(&w, "Pilot").unwrap();
        assert_eq!(bay.stock.len(), 15);
        assert!(bay
            .stock
            .iter()
            .zip(&original)
            .all(|(a, b)| a.drone_id == b.drone_id
                && a.class == b.class
                && a.home_cradle == b.home_cradle));
        assert!(
            !s.operations
                .iter()
                .any(|o| o.phase == DroneDropoffPhase::Held),
            "natural route held: {:?}",
            s.operations.last()
        );
        assert!(
            !bay.operations
                .iter()
                .any(|o| o.phase == DroneBayPhase::Held),
            "natural storage route held"
        );
        if let Some(d) = w.pilots["Pilot"].view.drones.iter().find(|d| d.id == id) {
            assert_eq!(d.hit_points, 100);
        }
        match stage {
            0 if !drone_bay::busy(&w, "Pilot") => {
                landmarks.push(serde_json::json!({"label":"launch_free","index":index,"elapsed_ms":f.now-start}));
                f.ok(Request::AssignMiningDrone {
                    id,
                    target_id: target,
                });
                stage = 1;
            }
            1 if payload(&w, "Pilot", id) >= 40 => {
                landmarks.push(serde_json::json!({"label":"forty_mined","index":index,"elapsed_ms":f.now-start}));
                f.ok(f.request(id, true));
                stage = 2;
            }
            2 if s
                .operation(1)
                .is_some_and(|o| o.phase == DroneDropoffPhase::Completed) =>
            {
                landmarks.push(serde_json::json!({"label":"first_dropoff_cleared","index":index,"elapsed_ms":f.now-start}));
                assert_eq!(w.pilots["Pilot"].view.ore, 40);
                stage = 3;
            }
            3 if payload(&w, "Pilot", id) >= 10 => {
                landmarks.push(serde_json::json!({"label":"resumed_mining_confirmed","index":index,"elapsed_ms":f.now-start}));
                f.ok(Request::DroneBay {
                    request: DroneBayRequest::BeginBatch {
                        binding: bay.binding.clone(),
                        batch_id: 1,
                        batch_kind: DroneBayBatchKind::RecallAll,
                    },
                });
                stage = 4;
            }
            4 if !drone_bay::batch_active(&w, "Pilot") => {
                assert_eq!(bay.counts(None).stored, 15);
                assert_eq!(payload(&w, "Pilot", id), 0);
                landmarks.push(serde_json::json!({"label":"recall_unloaded_and_stored","index":index,"elapsed_ms":f.now-start}));
                finish_at = Some(f.now + 2000);
                stage = 5;
            }
            _ => {}
        }
        if finish_at.is_some_and(|t| f.now >= t) {
            break;
        }
    }
    assert_eq!(stage, 5, "natural journey did not complete");
    let ledger = f.ok(Request::ExportLedger).ledger.unwrap();
    arkline_verifier::verify(&ledger, None).unwrap();
    let after = f.world();
    assert_eq!(after.pilots["Pilot"].view.ore, 50);
    assert_eq!(after.belt_ore + 50, before.belt_ore);
    std::fs::write(
        directory.join("ledger.json"),
        serde_json::to_vec_pretty(&ledger).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("receipt.json"),serde_json::to_vec_pretty(&serde_json::json!({"producer_commit":std::env::var("ARKLINE_PRODUCER_COMMIT").unwrap(),"pilot":"Pilot","drone_id":id,"target_id":target,"frames":rows,"landmarks":landmarks,"cadence_ms":100,"elapsed_ms":f.now-start,"setup":"Actual fresh preview Connect, Undock, Warp Belt and target lock. No world, asteroid, drone pose, cargo, health or rate edits.","end_stored":15,"end_active":0,"ship_ore":50,"lost_ore":0})).unwrap()).unwrap();
}

impl Fixture {
    fn attack_identity(&mut self) -> u64 {
        let mut session = None;
        let mut request = connect(5, None);
        if let Request::Connect { pilot, .. } = &mut request {
            *pilot = "Attacker".into();
        }
        let r = self.server.handle_at(&mut session, request, self.now);
        assert!(r.ok, "{}", r.message);
        let r = self
            .server
            .handle_at(&mut session, Request::Undock, self.now);
        assert!(r.ok, "{}", r.message);
        self.until(300, |w| {
            w.pilots["Attacker"]
                .view
                .flight
                .as_ref()
                .unwrap()
                .docking
                .as_ref()
                .unwrap()
                .phase
                == DockingPhase::Free
        });
        let w = self.world();
        let s = drone_bay::state(&w, "Attacker").unwrap();
        let id = s
            .stock
            .iter()
            .find(|d| d.class == DroneClass::Attack)
            .unwrap()
            .drone_id;
        let r = self.server.handle_at(
            &mut session,
            Request::DroneBay {
                request: DroneBayRequest::Launch {
                    binding: s.binding.clone(),
                    operation_id: 1,
                    drone_id: id,
                },
            },
            self.now,
        );
        assert!(r.ok, "{}", r.message);
        self.until(1000, |w| !drone_bay::busy(w, "Attacker"));
        id
    }
    fn ready_lethal_attack(&mut self, victim: u64, attacker: u64) {
        // Explicit combat fixture: an already launched finite Attack identity,
        // a damaged but LIVE victim, and one ready ordinary attack. No fake hit,
        // zero-HP injection, cargo edit or synthetic economic event.
        let mut w = self.world();
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
        let position = target.position;
        let source = w
            .pilots
            .get_mut("Attacker")
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == attacker)
            .unwrap();
        source.position = position;
        source.position[1] += DRONE_ATTACK_RANGE * 0.5;
        source.velocity = [0.; 3];
        source.mode = DroneMode::Attacking {
            target_pilot: "Pilot".into(),
            target_drone_id: victim,
        };
        source.attack_ready_at_ms = Some(self.now);
        source.updated_at_ms = self.now;
        drones::sync(w.pilots.get_mut("Pilot").unwrap());
        drones::sync(w.pilots.get_mut("Attacker").unwrap());
        self.server
            .db
            .execute(
                "UPDATE world SET json=?1 WHERE id=1",
                [serde_json::to_string(&w).unwrap()],
            )
            .unwrap();
    }
}
#[test]
fn lethal_damage_precedes_due_extraction_and_receiver_transfer_with_exact_sink_and_cleanup() {
    for mode in 0..3 {
        let at_receiver = mode > 0;
        let mut f = Fixture::new();
        let id = f.prepared_miner();
        f.until(700, |w| {
            payload(w, "Pilot", id) >= if mode == 2 { 40 } else { 20 }
        });
        let target = match f.world().pilots["Pilot"].view.drones[0].mode {
            DroneMode::Mining { target_id } => target_id,
            _ => panic!("expected miner"),
        };
        f.ok(Request::DroneHoldById { id });
        let attack = f.attack_identity();
        if at_receiver {
            f.ok(f.request(id, false));
            f.until(1800, |w| {
                state(w, "Pilot").unwrap().operation(1).unwrap().phase
                    == DroneDropoffPhase::Transferring
            });
            if mode == 2 {
                f.until(10, |w| {
                    state(w, "Pilot")
                        .unwrap()
                        .operation(1)
                        .unwrap()
                        .receipts
                        .len()
                        == 1
                });
            }
            let due = state(&f.world(), "Pilot")
                .unwrap()
                .operation(1)
                .unwrap()
                .next_batch_at_ms
                .unwrap();
            while f.now + 100 < due {
                f.step();
            }
        } else {
            f.ok(Request::AssignMiningDrone {
                id,
                target_id: target,
            });
            f.until(300, |w| {
                state(w, "Pilot")
                    .unwrap()
                    .holds
                    .iter()
                    .find(|h| h.drone_id == id)
                    .unwrap()
                    .cycle
                    .is_some()
            });
            let due = state(&f.world(), "Pilot")
                .unwrap()
                .holds
                .iter()
                .find(|h| h.drone_id == id)
                .unwrap()
                .cycle
                .as_ref()
                .unwrap()
                .completes_at_ms;
            while f.now + 100 < due {
                f.step();
            }
        }
        f.ready_lethal_attack(id, attack);
        let before = f.world();
        let ore = payload(&before, "Pilot", id);
        let ship = before.pilots["Pilot"].view.ore;
        let belt = before.belt_ore;
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(payload(&w, "Pilot", id), 0);
        assert_eq!(s.lost_ore_units, ore);
        assert_eq!(w.pilots["Pilot"].view.ore, ship);
        assert_eq!(w.belt_ore, belt);
        assert!(s
            .holds
            .iter()
            .find(|h| h.drone_id == id)
            .unwrap()
            .cycle
            .is_none());
        assert_eq!(
            drone_bay::state(&w, "Pilot")
                .unwrap()
                .counts(None)
                .destroyed,
            1
        );
        if at_receiver {
            assert_eq!(s.operation(1).unwrap().phase, DroneDropoffPhase::Destroyed);
            assert_eq!(
                s.operation(1).unwrap().receipts.len(),
                usize::from(mode == 2)
            );
            if mode == 2 {
                assert_eq!(ship, 25);
                assert_eq!(ore, 15);
            }
            assert!(s.cleanup.is_some());
            f.ok(f.control(1, "hold"));
            f.reopen();
            assert!(
                state(&f.world(), "Pilot")
                    .unwrap()
                    .cleanup
                    .as_ref()
                    .unwrap()
                    .held
            );
            f.ok(f.control(1, "resume"));
            f.until(100, |w| !busy(w, "Pilot"));
        }
        for _ in 0..10 {
            f.step();
        }
        assert_eq!(state(&f.world(), "Pilot").unwrap().lost_ore_units, ore);
        f.verify();
        let ledger = f.ok(Request::ExportLedger).ledger.unwrap();
        assert_eq!(ledger.entries.iter().filter(|e|matches!(&e.event,EconomicEvent::DroneCargo{record:DroneCargoLedgerEvent::Destroyed{pilot,drone_id,..}}if pilot=="Pilot"&&*drone_id==id)).count(),1);
    }
}

#[test]
fn full_natural_resource_job_conserves_headroom_and_delivers_ledger_confirmed_cargo() {
    let mut f = Fixture::new();
    f.ok(Request::StartResourceJob {
        target_units: 1000,
        pilot: None,
    });
    let mut complete = false;
    let mut max_reserved = 0;
    let start = f.now;
    for _ in 0..2200 {
        f.now += 1000;
        f.server.tick_at(f.now).unwrap();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        let incoming = s.reserved_incoming().unwrap();
        max_reserved = max_reserved.max(incoming);
        assert!(w.pilots["Pilot"].view.ore + incoming <= 1000);
        let job = &w.resource_jobs["Pilot"].view;
        let progress = job.drone_cargo.as_ref().unwrap();
        assert_eq!(
            progress.available_units,
            w.pilots["Pilot"].view.ore + s.ore_units().unwrap()
        );
        assert_eq!(progress.historical_extracted_units, job.mined_units);
        assert_eq!(progress.confirmed_delivered_units, job.stored_units);
        assert_ne!(job.phase, ResourceJobPhase::Blocked, "{}", job.message);
        if job.phase == ResourceJobPhase::Completed {
            complete = true;
            break;
        }
    }
    assert!(
        complete,
        "full cargo routine did not finish: {:?}",
        f.world().resource_jobs["Pilot"].view
    );
    let w = f.world();
    assert_eq!(w.resource_jobs["Pilot"].view.stored_units, 1000);
    assert_eq!(w.pilots["Pilot"].view.ore, 0);
    assert_eq!(state(&w, "Pilot").unwrap().ore_units(), Some(0));
    assert_eq!(
        drone_bay::state(&w, "Pilot").unwrap().counts(None).stored,
        15
    );
    assert!(max_reserved > 0);
    println!(
        "cargo routine simulated elapsed_ms={} max incoming reservation={}",
        f.now - start,
        max_reserved
    );
    f.verify();
}

#[test]
fn starting_job_with_loaded_manual_miner_unloads_then_stows_before_warp() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(500, |w| payload(w, "Pilot", id) >= 20);
    let before = f.world();
    let units = payload(&before, "Pilot", id);
    f.ok(Request::StartResourceJob {
        target_units: 100,
        pilot: None,
    });
    let mut prepared = false;
    for _ in 0..2000 {
        f.step();
        let w = f.world();
        let j = &w.resource_jobs["Pilot"].view;
        assert_ne!(j.phase, ResourceJobPhase::Blocked, "{}", j.message);
        if drone_bay::state(&w, "Pilot").unwrap().counts(None).stored == 15 {
            assert_eq!(w.pilots["Pilot"].view.ore, units);
            assert_eq!(payload(&w, "Pilot", id), 0);
            assert!(state(&w, "Pilot")
                .unwrap()
                .operations
                .iter()
                .any(|o| o.drone_id == id && o.phase == DroneDropoffPhase::Completed));
            prepared = true;
            break;
        }
        assert!(
            !matches!(w.pilots["Pilot"].view.location, Location::InTransit { .. }),
            "cannot warp with manual fleet outside"
        );
    }
    assert!(prepared, "manual fleet never prepared");
    f.verify();
}

#[test]
fn recall_cancel_remaining_keeps_held_service_resumable_and_never_marks_drone_stored() {
    let mut f = Fixture::new();
    let id = f.prepared_miner();
    f.until(700, |w| payload(w, "Pilot", id) >= 40);
    let binding = drone_bay::state(&f.world(), "Pilot")
        .unwrap()
        .binding
        .clone();
    f.ok(Request::DroneBay {
        request: DroneBayRequest::BeginBatch {
            binding: binding.clone(),
            batch_id: 1,
            batch_kind: DroneBayBatchKind::RecallAll,
        },
    });
    f.until(1800, |w| {
        state(w, "Pilot").unwrap().operation(1).unwrap().phase == DroneDropoffPhase::Coupling
    });
    f.step();
    f.ok(Request::DroneBay {
        request: DroneBayRequest::HoldBatch {
            binding: binding.clone(),
            batch_id: 1,
        },
    });
    let position = f.world().pilots["Pilot"].view.drones[0].position;
    let cancel = Request::DroneBay {
        request: DroneBayRequest::CancelBatch {
            binding: binding.clone(),
            batch_id: 1,
        },
    };
    f.ok(cancel.clone());
    f.ok(cancel);
    let w = f.world();
    let b = drone_bay::state(&w, "Pilot")
        .unwrap()
        .batches
        .as_ref()
        .unwrap()
        .operation(1)
        .unwrap();
    assert!(b.active() && b.held);
    assert_eq!(b.phase, DroneBayBatchPhase::Cancelling);
    assert_eq!(b.current_service_operation_id, Some(1));
    assert_eq!(w.pilots["Pilot"].view.drones[0].position, position);
    f.reopen();
    f.ok(Request::DroneBay {
        request: DroneBayRequest::ResumeBatch {
            binding,
            batch_id: 1,
        },
    });
    f.until(1000, |w| !drone_bay::batch_active(w, "Pilot"));
    let w = f.world();
    let s = drone_bay::state(&w, "Pilot").unwrap();
    let b = s.batches.as_ref().unwrap().operation(1).unwrap();
    assert_eq!(b.phase, DroneBayBatchPhase::Cancelled);
    assert_eq!(b.members[0].status, DroneBayBatchMemberStatus::Cancelled);
    assert!(b.members[0].operation_id.is_none());
    assert_eq!(b.members[0].service_operation_ids, vec![1]);
    assert_eq!(s.counts(None).stored, 14);
    assert_eq!(s.counts(None).active, 1);
    assert_eq!(w.pilots["Pilot"].view.drones[0].id, id);
    assert_eq!(w.pilots["Pilot"].view.ore, 40);
    assert_eq!(payload(&w, "Pilot", id), 0);
    f.verify();
}
