//! Exact stationary1703 ten-local-loader proof. Source/rates remain immutable.
use super::*;
use sha2::{Digest, Sha256};
pub(super) fn fixture() -> Fixture {
    let mut registration = geometry::stationary_fixture_definition()
        .unwrap()
        .topology
        .registration
        .clone();
    registration.assembly_revision = 1703;
    registration.assembly_source_sha256 =
        "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a".into();
    let def = geometry::stationary_fixture_for_registration(&registration)
        .expect("Exact reviewed1703 source");
    assert!(geometry::lookup(&registration).is_none());
    assert_eq!(def.topology.loaders.len(), 10);
    assert_eq!(def.topology.transit_buffers.len(), 10);
    Fixture::with_source(def)
}
#[test]
fn actual_1703_initial_idle_and_ten_identified_launches() {
    let mut f = fixture();
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    let def = definition(&w).unwrap().unwrap();
    assert_eq!(s.machinery.len(), 480);
    let idle = def
        .idle_roots(
            &s.cassette_poses
                .iter()
                .map(|(id, p)| (*id, *p))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(idle.into_iter().collect::<BTreeMap<_, _>>(), s.machinery);
    assert!(
        def.mechanisms
            .iter()
            .any(|m| s.machinery[&m.semantic_id] != m.reference_pose),
        "Source idle is not immutable authored reference pose"
    );
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    f.step();
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.runs.len(), 10);
    assert!(s.runs.values().all(|r| !r.held));
    assert_eq!(s.inventory.entries.len(), 20);
    assert_eq!(s.inventory.cassettes.len(), 20);
}
/// Bounded restoration smoke: real ticks, no per-frame Snapshot/export overhead.
#[test]
fn actual_1703_twenty_parallel_deployment_completes_and_reopens() {
    let mut f = fixture();
    let original = state(&f.world(), "Pilot").unwrap().inventory.roster.clone();
    let initial = f.now;
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut max_stage = 0;
    let mut completed = false;
    for tick in 0..3000 {
        f.step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert_eq!(s.inventory.binding.registration.assembly_revision, 1703);
        assert_eq!(s.inventory.roster, original);
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert!(s.inventory.entries.iter().all(|e| e.hit_points == 100));
        assert_eq!(s.ore.ore_units(), Some(0));
        assert_eq!(w.pilots["Pilot"].view.ore, 0);
        assert_eq!(w.pilots["Pilot"].view.credits, STARTER_CREDITS);
        max_stage = max_stage.max(
            s.runs
                .values()
                .filter(|r| matches!(r.kind, CarrierRouteKind::Stage { .. }) && !r.held)
                .count(),
        );
        if tick % 100 == 0 {
            eprintln!(
                "1703 smoke {}ms: {} bodies, {} runs, maxStage{}",
                f.now - initial,
                w.pilots["Pilot"].view.drones.len(),
                s.runs.len(),
                max_stage
            );
        }
        if s.inventory.deployments[0].phase == CarrierDeploymentPhase::Completed
            && s.runs.is_empty()
            && s.departures.is_empty()
            && s.inventory.reservations.is_empty()
            && s.inventory.cleanups.is_empty()
        {
            completed = true;
            assert!(s
                .inventory
                .entries
                .iter()
                .all(|e| e.location == CarrierDroneLocation::Deployed));
            assert_eq!(w.pilots["Pilot"].view.drones.len(), 20);
            break;
        }
    }
    assert!(
        completed,
        "1703 did not fully secure within the existing five-minute target"
    );
    assert_eq!(max_stage, 10);
    eprintln!(
        "1703 smoke fully safe at {}ms; maxStage{}",
        f.now - initial,
        max_stage
    );
    f.snapshot_valid();
    f.reopen();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.roster, original);
    assert!(s
        .inventory
        .entries
        .iter()
        .all(|e| e.location == CarrierDroneLocation::Deployed));
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 20);
    f.snapshot_valid();
}
#[test]
#[ignore = "explicit100ms source-bound ten-local-loader timed documentary export"]
fn export_actual_1703_one_command_twenty_parallel_deployment() {
    let output = std::path::PathBuf::from(
        std::env::var("ARKLINE_CARRIER_EXPORT").expect("Explicit fresh output folder"),
    );
    assert!(!output.exists(), "Never overwrite prior evidence");
    std::fs::create_dir_all(&output).unwrap();
    let producer = std::env::var("ARKLINE_PRODUCER_COMMIT").expect("Exact frozen commit");
    assert_eq!(producer.len(), 40);
    let binary_sha = format!(
        "{:x}",
        Sha256::digest(std::fs::read(std::env::current_exe().unwrap()).unwrap())
    );
    let mut f = fixture();
    let initial = f.now;
    let original = state(&f.world(), "Pilot").unwrap().inventory.clone();
    let initial_ready: Vec<_> = original
        .entries
        .iter()
        .filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
        .map(|e| e.drone_id)
        .collect();
    assert_eq!(initial_ready.len(), 10);
    let mut first_ten_source_clear = None;
    let mut first_ten_departure_clear = None;
    let mut all_twenty_deployed = None;
    let mut complete = None;
    let mut max_stage = 0;
    let mut first_stage = None;
    let mut previous_loaded_poses = BTreeMap::new();
    let mut max_displaced_loaded_cassettes = 0;
    let mut ten_loaded_motion_samples = 0;
    let mut frames = Vec::new();
    for index in 0..9000 {
        if index > 0 {
            f.step();
        }
        if index == 10 {
            f.request(CarrierInventoryRequest::DeployAll {
                binding: f.binding(),
                batch_id: 1,
            });
        }
        // Deliberately the exact421 tick -> Snapshot path and100ms cadence.
        let response = f.ok(Request::Snapshot);
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        let public = response.snapshot.as_ref().unwrap().pilots[0]
            .carrier_inventory
            .as_ref()
            .unwrap();
        assert_eq!(public.sample_time_ms, f.now);
        assert_eq!(public.inventory, s.inventory);
        assert_eq!(
            public.validate_projection(&response.snapshot.as_ref().unwrap().pilots[0]),
            Ok(CarrierInventoryAdmission::StationaryFixture)
        );
        assert_eq!(s.inventory.roster, original.roster);
        assert_eq!(s.inventory.entries.len(), 20);
        assert_eq!(s.inventory.cassettes.len(), 20);
        assert!(s.inventory.entries.iter().all(|e| e.hit_points == 100));
        assert_eq!(s.ore.ore_units(), Some(0));
        assert_eq!(w.pilots["Pilot"].view.ore, 0);
        assert_eq!(w.pilots["Pilot"].view.credits, STARTER_CREDITS);
        let stages = s
            .runs
            .values()
            .filter(|r| matches!(r.kind, CarrierRouteKind::Stage { .. }) && !r.held)
            .count();
        max_stage = max_stage.max(stages);
        let current_loaded_poses: BTreeMap<_, _> = s
            .runs
            .values()
            .filter_map(|r| {
                let x = s.inventory.operation(r.operation_id)?.stage.as_ref()?;
                Some((r.operation_id, s.cassette_poses[&x.loaded_cassette_id]))
            })
            .collect();
        let displaced = current_loaded_poses
            .iter()
            .filter(|(id, pose)| {
                previous_loaded_poses
                    .get(*id)
                    .is_some_and(|previous: &BayPose| {
                        (0..3).any(|axis| {
                            (pose.position_m[axis] - previous.position_m[axis]).abs() > 1e-9
                        })
                    })
            })
            .count();
        max_displaced_loaded_cassettes = max_displaced_loaded_cassettes.max(displaced);
        if displaced == 10 {
            ten_loaded_motion_samples += 1;
        }
        previous_loaded_poses = current_loaded_poses;
        if stages > 0 {
            first_stage.get_or_insert(f.now);
        }
        assert!(
            s.runs
                .values()
                .all(|r| r.kind.stage_segment() != Some(CarrierStageSegment::Head)),
            "1703 must retain full loader ownership"
        );
        if initial_ready
            .iter()
            .all(|id| s.inventory.entry(*id).unwrap().location == CarrierDroneLocation::Deployed)
        {
            first_ten_source_clear.get_or_insert(f.now);
            if initial_ready
                .iter()
                .all(|id| !s.departures.contains_key(id))
            {
                first_ten_departure_clear.get_or_insert(f.now);
            }
        }
        if s.inventory
            .entries
            .iter()
            .all(|e| e.location == CarrierDroneLocation::Deployed)
        {
            all_twenty_deployed.get_or_insert(f.now);
        }
        if s.inventory
            .deployments
            .last()
            .is_some_and(|b| b.phase == CarrierDeploymentPhase::Completed)
            && s.runs.is_empty()
            && s.departures.is_empty()
            && s.inventory.reservations.is_empty()
            && s.inventory.cleanups.is_empty()
        {
            complete.get_or_insert(f.now);
        }
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
        let row = serde_json::json!({"format_version":1,"producer_commit":producer,"sample_time_ms":f.now,"pilot":"Pilot","response":response,"parent":parent(&w,"Pilot").unwrap(),"binding":s.inventory.binding,"inventory":s.inventory,"ore":s.ore,"drone_poses":s.drone_poses.iter().map(|(id,p)|(*id,*p)).collect::<Vec<_>>(),"cassette_poses":s.cassette_poses.iter().map(|(id,p)|(*id,*p)).collect::<Vec<_>>(),"machinery":s.machinery.iter().collect::<Vec<_>>(),"motions":motions});
        let bytes = serde_json::to_vec(&row).unwrap();
        let file = format!("response-{index:05}.json");
        let hash = format!("{:x}", Sha256::digest(&bytes));
        std::fs::write(output.join(&file), bytes).unwrap();
        frames.push(serde_json::json!({"file":file,"index":index,"elapsed_ms":f.now-initial,"response_sha256":hash}));
        if index % 100 == 0 {
            eprintln!(
                "1703 {}ms: {} actual live bodies, {} concurrent unheld Stage, {} totalruns, maxStage{}",
                f.now - initial,
                w.pilots["Pilot"].view.drones.len(),
                stages,
                s.runs.len(),
                max_stage
            );
        }
        if complete.is_some_and(|at| f.now >= at + 2000) {
            break;
        }
    }
    let end = complete.expect("Actual20 deployment did not complete within bounded observation");
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    let receipt = serde_json::json!({"format_version":1,"producer_commit":producer,"producer_binary_sha256":binary_sha,"pilot":"Pilot","fixture_conditions":"Exact reviewed stationary1703 source; initial carrier at origin before one command; finite20Mining/20cassettes. Actual100ms authority ticks and Snapshot requests, no interpolation, asteroid relocation, drone pose injection or rate change. Ten independent aligned-column loaders/buffers (buffers are transit-only); no1703reverse/service/normaltravel admission.","command":"oneDeployAll batch1 at1000ms; no additional Stage/Launch requests","registration":s.inventory.binding.registration,"initial_roster":original.roster,"initial_ready_ids":initial_ready,"first10_source_clear_ms":first_ten_source_clear.map(|t|t-initial),"first10_departure_clear_ms":first_ten_departure_clear.map(|t|t-initial),"first_stage_ms":first_stage.map(|t|t-initial),"max_concurrent_unheld_stage":max_stage,"max_simultaneously_displaced_loaded_cassettes":max_displaced_loaded_cassettes,"ten_loaded_motion_samples":ten_loaded_motion_samples,"all20_deployed_ms":all_twenty_deployed.map(|t|t-initial),"fully_safe_ms":end-initial,"from_command_fully_safe_ms":end-initial-1000,"within_5min_target":end-initial-1000<=300000,"duration_ms":f.now-initial,"cadence_ms":100,"frame_count":frames.len(),"final_deployed":20,"final_stored":0,"final_ore":0,"operations":s.inventory.operations,"deployment":s.inventory.deployments,"frames":frames});
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    assert_eq!(
        max_stage, 10,
        "Did not prove ten concurrent local reloader owners"
    );
    assert_eq!(
        max_displaced_loaded_cassettes, 10,
        "Did not measure ten simultaneously moving loaded cassettes"
    );
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 20);
    assert!(
        end - initial - 1000 <= 300000,
        "Physical deployment exceeded five-minute target; retain honest timing receipt"
    );
}

#[test]
fn actual_1703_all_ten_loaded_cassettes_hold_reopen_and_resume_without_early_release() {
    let mut f = fixture();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut loaded = false;
    for _ in 0..150 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert!(s.runs.values().all(|r| !r.held));
        if s.runs.len() == 10
            && s.runs.values().all(|r| {
                let Some(x) = s
                    .inventory
                    .operation(r.operation_id)
                    .and_then(|o| o.stage.as_ref())
                else {
                    return false;
                };
                r.completed_stage_legs == 1
                    && r.route[r.target_index].carried_cassette_id == Some(x.loaded_cassette_id)
                    && matches!(
                        s.inventory
                            .cassettes
                            .iter()
                            .find(|c| c.cassette_id == x.loaded_cassette_id)
                            .unwrap()
                            .location,
                        CarrierCassetteLocation::InLoader { .. }
                    )
            })
        {
            loaded = true;
            break;
        }
    }
    assert!(
        loaded,
        "No actual concurrent loaded-transfer interval reached"
    );
    f.request(CarrierInventoryRequest::HoldDeployment {
        binding: f.binding(),
        batch_id: 1,
    });
    let before = state(&f.world(), "Pilot").unwrap().clone();
    assert_eq!(before.inventory.reservations.len(), 60);
    assert!(before.runs.values().all(|r| r.held));
    assert!(before
        .inventory
        .operations
        .iter()
        .filter_map(|o| o.stage.as_ref())
        .all(|x| x.loaded_handoff_at_ms.is_none()));
    f.reopen();
    for _ in 0..3 {
        f.coarse_step();
    }
    let after = state(&f.world(), "Pilot").unwrap().clone();
    assert_eq!(before.inventory.roster, after.inventory.roster);
    assert_eq!(before.inventory.cassettes, after.inventory.cassettes);
    assert_eq!(before.inventory.reservations, after.inventory.reservations);
    assert_eq!(before.drone_poses, after.drone_poses);
    assert_eq!(before.cassette_poses, after.cassette_poses);
    assert_eq!(before.machinery, after.machinery);
    for (id, r) in &before.runs {
        assert_eq!(r.confirmed, after.runs[id].confirmed);
        assert!(after.runs[id].held);
    }
    f.snapshot_valid();
    f.request(CarrierInventoryRequest::ResumeDeployment {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut complete = false;
    for _ in 0..450 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        if s.inventory.deployments[0].phase == CarrierDeploymentPhase::Completed
            && s.runs.is_empty()
            && s.departures.is_empty()
        {
            complete = true;
            break;
        }
    }
    assert!(complete);
    f.snapshot_valid();
    let w = f.world();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.roster, before.inventory.roster);
    assert_eq!(s.inventory.cassettes.len(), 20);
    assert_eq!(w.pilots["Pilot"].view.drones.len(), 20);
    assert!(s
        .inventory
        .entries
        .iter()
        .all(|e| e.location == CarrierDroneLocation::Deployed && e.hit_points == 100));
    assert!(s.inventory.reservations.is_empty() && s.inventory.cleanups.is_empty());
    assert_eq!(s.ore.ore_units(), Some(0));
}

#[test]
fn actual_1703_cross_column_coordinated_cassette_swap_refuses_reopen_without_write() {
    let mut f = fixture();
    f.reopen();
    f.snapshot_valid();
    let original = f.world();
    validate(&original).unwrap();
    let def = definition(&original).unwrap().unwrap();
    let mut bad = original.clone();
    let s = bad
        .accounts
        .get_mut("Pilot")
        .unwrap()
        .carrier_inventory
        .as_mut()
        .unwrap();
    for (cid, berth) in [(0u64, 1u8), (1, 0)] {
        let dock = CarrierCassetteDock::Ready { berth_id: berth };
        let pose = def
            .docks
            .iter()
            .find(|d| d.dock == dock)
            .unwrap()
            .cassette_pose;
        let cassette = s
            .inventory
            .cassettes
            .iter_mut()
            .find(|c| c.cassette_id == cid)
            .unwrap();
        cassette.location = CarrierCassetteLocation::Docked { dock };
        let drone = cassette.drone_id.unwrap();
        s.inventory
            .entries
            .iter_mut()
            .find(|e| e.drone_id == drone)
            .unwrap()
            .location = CarrierDroneLocation::Ready { berth_id: berth };
        s.drone_poses.insert(drone, pose);
        s.cassette_poses.insert(cid, pose);
    }
    s.machinery = def
        .idle_roots(
            &s.cassette_poses
                .iter()
                .map(|(id, p)| (*id, *p))
                .collect::<Vec<_>>(),
        )
        .unwrap()
        .into_iter()
        .collect();
    assert!(
        s.inventory.validate_shape(&def.topology),
        "Generic stock shape alone is insufficient"
    );
    let error = validate(&bad).unwrap_err().to_string();
    assert!(error.contains("component"), "{error}");
    assert!(public_view(&bad, "Pilot", f.now)
        .unwrap_err()
        .to_string()
        .contains("component"));
    let path = f.path.clone();
    drop(std::mem::replace(
        &mut f.server,
        Server::open(":memory:").unwrap(),
    ));
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute(
        "UPDATE world SET json=?1 WHERE id=1",
        [serde_json::to_string(&bad).unwrap()],
    )
    .unwrap();
    drop(db);
    let before = std::fs::read(&path).unwrap();
    let opened = Server::open_editor_realm_with_scene(
        &path,
        default_pioneer_room_layout(),
        SceneLayoutRevision::DockingV1,
    );
    assert!(opened.is_err());
    assert_eq!(
        std::fs::read(&path).unwrap(),
        before,
        "Preflight must reject before any SQLite mutation"
    );
}

fn observation_state(f: &Fixture) -> serde_json::Value {
    let w = f.world();
    let ledger: (u64, u64) = f
        .server
        .db
        .query_row(
            "SELECT COUNT(*), COALESCE(MAX(sequence), 0) FROM ledger",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    let mut value = serde_json::json!({"carrier":state(&w,"Pilot").unwrap(),
        "drones":w.pilots["Pilot"].view.drones,"ore":w.pilots["Pilot"].view.ore,
        "credits":w.pilots["Pilot"].view.credits,"ledger_count_sequence":ledger});
    // Only the observed non-authoritative operation message is excluded.
    // Identity, status, clocks, poses, resources, ore and ledger remain compared.
    for operation in value["carrier"]["inventory"]["operations"]
        .as_array_mut()
        .unwrap()
    {
        operation.as_object_mut().unwrap().remove("message");
    }
    value
}
fn observation_differences(
    a: &serde_json::Value,
    b: &serde_json::Value,
    path: &str,
    out: &mut Vec<serde_json::Value>,
) {
    if a == b || out.len() >= 12 {
        return;
    }
    match (a, b) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            let keys: BTreeSet<_> = a.keys().chain(b.keys()).collect();
            for key in keys {
                observation_differences(
                    a.get(key).unwrap_or(&serde_json::Value::Null),
                    b.get(key).unwrap_or(&serde_json::Value::Null),
                    &format!("{path}/{key}"),
                    out,
                );
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) if a.len() == b.len() => {
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                observation_differences(a, b, &format!("{path}/{i}"), out);
            }
        }
        _ => out.push(serde_json::json!({"path":path,"before":a,"after":b})),
    }
}
fn diagnostic_output(value: &serde_json::Value, variable: &str) {
    use std::io::Write;
    let path = std::env::var(variable).expect("Explicit new diagnostic output required");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
}

/// Diagnostic only: records the first same-clock difference, not a passing
/// assertion that observation is inert. Does not change authority behavior.
#[test]
#[ignore = "explicit bounded same-clock observation diagnostic"]
fn inspect_actual_1703_same_clock_snapshot_effects() {
    let mut f = fixture();
    let initial = f.now;
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut finding = serde_json::Value::Null;
    let mut sampled = 0;
    for tick in 0..250 {
        if tick > 0 {
            f.step();
        }
        let before = observation_state(&f);
        f.ok(Request::Snapshot);
        let first = observation_state(&f);
        f.ok(Request::Snapshot);
        let repeated = observation_state(&f);
        sampled += 1;
        if before != first || first != repeated {
            let mut first_diffs = Vec::new();
            let mut repeat_diffs = Vec::new();
            observation_differences(&before, &first, "", &mut first_diffs);
            observation_differences(&first, &repeated, "", &mut repeat_diffs);
            finding = serde_json::json!({"elapsed_ms":f.now-initial,
                "first_snapshot_changes":first_diffs,"repeated_snapshot_changes":repeat_diffs,
                "repeated_snapshot_identical":first==repeated});
            break;
        }
    }
    let output = serde_json::json!({"format_version":1,"kind":"diagnostic_not_acceptance",
        "producer_commit":std::env::var("ARKLINE_PRODUCER_COMMIT").unwrap(),
        "registration":f.binding().registration,"command_offset_ms":0,"tick_delta_ms":100,
        "sampled_accepted_times":sampled,"first_difference":finding,
        "limit":"Stops at first difference or250accepted clocks; no full-journey equivalence claim"});
    eprintln!("{}", serde_json::to_string_pretty(&output).unwrap());
    diagnostic_output(&output, "ARKLINE_OBSERVATION_OUTPUT");
}

fn timed_tick(f: &mut Fixture, initial: u64, index: usize) -> serde_json::Value {
    use std::time::Instant;
    let begin = Instant::now();
    f.step();
    let tick_us = begin.elapsed().as_micros() as u64;
    let begin = Instant::now();
    let w = f.world();
    let observer_read_us = begin.elapsed().as_micros() as u64;
    let begin = Instant::now();
    let s = state(&w, "Pilot").unwrap();
    assert_eq!(s.inventory.binding.registration.assembly_revision, 1703);
    assert_eq!(s.inventory.entries.len(), 20);
    assert_eq!(s.inventory.cassettes.len(), 20);
    assert_eq!(s.ore.ore_units(), Some(0));
    assert_eq!(w.pilots["Pilot"].view.ore, 0);
    let stages = s
        .runs
        .values()
        .filter(|r| matches!(r.kind, CarrierRouteKind::Stage { .. }) && !r.held)
        .count();
    let phase = if stages == 10 {
        "ten_stage"
    } else if s.runs.is_empty() {
        "idle"
    } else {
        "launch_or_transition"
    };
    let observer_assert_us = begin.elapsed().as_micros() as u64;
    serde_json::json!({"index":index,"elapsed_ms":f.now-initial,"phase":phase,
        "tick_including_db_us":tick_us,"observer_read_us":observer_read_us,
        "observer_assert_us":observer_assert_us,"stages":stages,"runs":s.runs.len(),
        "bodies":w.pilots["Pilot"].view.drones.len()})
}
fn timing_database(f: &Fixture) -> serde_json::Value {
    let journal: String = f
        .server
        .db
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    let synchronous: u64 = f
        .server
        .db
        .query_row("PRAGMA synchronous", [], |r| r.get(0))
        .unwrap();
    let page_size: u64 = f
        .server
        .db
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .unwrap();
    serde_json::json!({"mode":"fresh_file_backed_sqlite","journal_mode":journal,
        "synchronous":synchronous,"page_size":page_size})
}
/// Small optimized probe of idle/initial launch/ten-loader work, not a whole
/// deployment performance claim and not native/FPS acceptance.
#[test]
#[ignore = "explicit optimized quiet-window timing only"]
fn measure_actual_1703_idle_launch_and_ten_stage_ticks() {
    assert!(
        !cfg!(debug_assertions),
        "Use the release profile for this diagnostic"
    );
    let profiling = std::env::var_os("ARKLINE_PHASE_PROFILE_OUTPUT").is_some();
    if profiling {
        crate::tick_profile::enable();
    }
    let mut idle = fixture();
    let initial = idle.now;
    let database = timing_database(&idle);
    let idle_rows: Vec<_> = (0..16).map(|i| timed_tick(&mut idle, initial, i)).collect();
    let mut active = fixture();
    let initial = active.now;
    let command_started = std::time::Instant::now();
    active.request(CarrierInventoryRequest::DeployAll {
        binding: active.binding(),
        batch_id: 1,
    });
    let command_us = command_started.elapsed().as_micros() as u64;
    let mut rows = Vec::new();
    let mut stage_samples = 0;
    for index in 0..400 {
        let row = timed_tick(&mut active, initial, index);
        if row["phase"] == "ten_stage" {
            stage_samples += 1;
        }
        rows.push(row);
        if stage_samples == 32 {
            break;
        }
    }
    assert_eq!(
        stage_samples, 32,
        "Bounded probe never reached ten active stages"
    );
    let output = serde_json::json!({"format_version":1,"kind":"bounded_timing_not_native_acceptance",
        "producer_commit":std::env::var("ARKLINE_PRODUCER_COMMIT").unwrap(),
        "producer_binary_sha256":format!("{:x}",Sha256::digest(std::fs::read(std::env::current_exe().unwrap()).unwrap())),
        "registration":active.binding().registration,"tick_delta_ms":100,"configured_sleep_ms":100,
        "scheduler":"production loop sleeps100ms then locks and ticks; not fixed deadline",
        "profile":"release; debug_assertions=false; effective compile flags recorded externally",
        "database":database,"deploy_command_us":command_us,"idle_samples":idle_rows,"active_samples":rows,
        "limits":"Shared cloud; fixture reload/assert separately measured but warms caches; no Snapshot/export in timed ticks; no20body-cleanup, full-journey, client-FPS or normal-flight claim"});
    diagnostic_output(&output, "ARKLINE_TIMING_OUTPUT");
    if profiling {
        let phases = serde_json::json!({"kind":"instrumented_phase_diagnostic", "ticks":crate::tick_profile::take()});
        diagnostic_output(&phases, "ARKLINE_PHASE_PROFILE_OUTPUT");
    }
}

#[test]
fn same_clock_snapshots_do_not_admit_post_launch_stage_work() {
    let mut f = fixture();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let mut observed_stage = false;
    for _ in 0..250 {
        f.step();
        let before = observation_state(&f);
        observed_stage |= state(&f.world(), "Pilot")
            .unwrap()
            .runs
            .values()
            .any(|r| matches!(r.kind, CarrierRouteKind::Stage { .. }));
        for _ in 0..2 {
            f.ok(Request::Snapshot);
            let after = observation_state(&f);
            if before != after {
                let mut differences = vec![];
                observation_differences(&before, &after, "", &mut differences);
                panic!(
                    "Same-clock observation changed authority at {}: {}",
                    f.now,
                    serde_json::to_string_pretty(&differences).unwrap()
                );
            }
        }
    }
    assert!(observed_stage, "Must cover post-launch reserve admission");
    f.reopen();
    let before = observation_state(&f);
    f.ok(Request::Snapshot);
    assert_eq!(before, observation_state(&f));
}

#[test]
fn same_clock_snapshot_preserves_final_loaded_service_transfer() {
    let mut f = fixture();
    service_tests::launch(&mut f, 1);
    service_tests::mine(&mut f, 1, 40, -1.);
    let op = service_tests::service(&mut f, 1, false);
    let mut coupled = false;
    let mut checked_final = false;
    for _ in 0..1000 {
        if coupled {
            f.step();
        } else {
            f.coarse_step();
        }
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        coupled |= s.runs.get(&op).is_some_and(|r| r.waiting_for_transfer);
        if w.pilots["Pilot"].view.ore == 40 {
            let before = observation_state(&f);
            for _ in 0..2 {
                f.ok(Request::Snapshot);
                let after = observation_state(&f);
                if before != after {
                    let mut differences = vec![];
                    observation_differences(&before, &after, "", &mut differences);
                    panic!(
                        "Final transfer same-clock mutation: {}",
                        serde_json::to_string_pretty(&differences).unwrap()
                    );
                }
            }
            checked_final = true;
            break;
        }
    }
    assert!(coupled && checked_final);
}
