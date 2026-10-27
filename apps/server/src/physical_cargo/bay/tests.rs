use super::*;
use arkline_protocol::physical_cargo::{
    bay::certified_inspection_binding,
    catalog::{
        AuthoredCargoOperatingRatings, PhysicalCargoCatalog, PhysicalCargoForm,
        PhysicalCargoMaterial,
    },
    AuthoredVolumeRate, CargoMassKnowledge,
};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "arkline-connected-bay-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", ".lock"] {
            let _ = std::fs::remove_file(format!("{}{}", self.0.display(), suffix));
        }
    }
}
fn genesis() -> PhysicalCargoGenesis {
    let source = physical_source_configuration().unwrap();
    let rate = |n, ms| AuthoredVolumeRate {
        volume_cm3: VolumeCm3(n),
        interval_ms: ms,
    };
    let catalog = PhysicalCargoCatalog {
        schema_version: 1,
        materials: vec![PhysicalCargoMaterial {
            material_id: "bay.test.bulk".into(),
            form: PhysicalCargoForm::BulkVolumeCm3,
            mass: CargoMassKnowledge::Unknown,
            authored_price: None,
        }],
        operating_ratings: vec![AuthoredCargoOperatingRatings {
            rating_id: "bay.test.authoring".into(),
            revision: 1,
            drone_extraction: rate(1000, 1000),
            ship_extraction: rate(1, 1000),
            vessel_manifold: rate(500, 500),
            drone_batch_energy_units: 2,
            ship_batch_energy_units: 1,
        }],
    };
    PhysicalCargoGenesis {
        source_configuration_sha256: source.identity_sha256().unwrap(),
        design: catalog
            .bind_manifest(
                "bay.test.bulk",
                "bay.test.authoring",
                source.measurements().unwrap(),
            )
            .unwrap(),
        catalog,
        mining_drone_ids: vec![1],
        resource_cm3: VolumeCm3(10000),
    }
}
fn setup() -> (Temp, PhysicalCargoInspection, u64) {
    let p = Temp::new();
    let mut s = PhysicalCargoInspection::open(&p.0, genesis(), 0).unwrap();
    s.bind_bay(
        certified_inspection_binding(1, 1000, 20).expect("accepted selected-bay certificate"),
        0,
    )
    .unwrap();
    (p, s, 0)
}
fn command(s: &mut PhysicalCargoInspection, c: Command, now: u64) -> PhysicalBayView {
    let seq = s.bay_view().unwrap().state.step_sequence;
    s.bay_command(seq, c, now).unwrap()
}
fn ticks(s: &mut PhysicalCargoInspection, now: &mut u64, n: usize) {
    for _ in 0..n {
        *now += 100;
        s.bay_tick(*now).unwrap();
    }
}
fn launch(s: &mut PhysicalCargoInspection, now: &mut u64) {
    command(s, Command::Launch, *now);
    ticks(s, now, 60);
    assert!(s.bay_view().unwrap().state.deployed);
}
fn mine(s: &mut PhysicalCargoInspection, now: &mut u64) {
    command(
        s,
        Command::Mine {
            volume_cm3: VolumeCm3(1000),
        },
        *now,
    );
    ticks(s, now, 10);
}
fn service(s: &mut PhysicalCargoInspection, now: &mut u64) {
    command(s, Command::Service, *now);
    for _ in 0..200 {
        if s.bay_view().unwrap().state.route.is_none() {
            return;
        }
        ticks(s, now, 1);
    }
    panic!("service failed to finish");
}
fn json(s: &mut PhysicalCargoInspection) -> String {
    serde_json::to_string(&s.export().unwrap()).unwrap()
}

#[test]
fn source_driven_launch_mine_transfer_redeploy_return_is_one_conserved_loop() {
    let (_p, mut s, mut now) = setup();
    let stored = s.bay_view().unwrap();
    command(&mut s, Command::Launch, now);
    ticks(&mut s, &mut now, 15);
    let partial = s.bay_view().unwrap();
    assert_eq!(partial.state.frame_index, 1);
    assert_eq!(partial.state.segment_elapsed_ms, 500);
    assert!(!partial.source_frame.milestones.exterior_clear);
    assert_ne!(
        stored.source_frame.drone_pose,
        partial.source_frame.drone_pose
    );
    ticks(&mut s, &mut now, 45);
    let exterior = s.bay_view().unwrap();
    assert!(exterior.state.deployed && exterior.state.route.is_none());
    mine(&mut s, &mut now);
    assert_eq!(
        s.export().unwrap().account.drones[0].payload_cm3,
        VolumeCm3(1000)
    );
    let seq = s.bay_view().unwrap().state.step_sequence;
    assert!(s.bay_command(seq, Command::Return, now).is_err());
    service(&mut s, &mut now);
    assert_eq!(
        s.bay_view().unwrap().source_frame.drone_pose,
        exterior.source_frame.drone_pose
    );
    let out = s.export().unwrap();
    assert_eq!(out.account.ship_payload_cm3, VolumeCm3(1000));
    assert_eq!(out.resource_remaining_cm3, VolumeCm3(9000));
    assert_eq!(out.bay.unwrap().energy_units, 18);
    mine(&mut s, &mut now);
    service(&mut s, &mut now);
    command(&mut s, Command::Return, now);
    ticks(&mut s, &mut now, 60);
    let final_view = s.bay_view().unwrap();
    assert!(!final_view.state.deployed);
    assert!(final_view.state.route.is_none());
    assert_eq!(
        final_view.source_frame.drone_pose,
        stored.source_frame.drone_pose
    );
    let out = s.export().unwrap();
    out.verify().unwrap();
    assert_eq!(out.account.ship_payload_cm3, VolumeCm3(2000));
}
#[test]
fn raw_events_stale_commands_and_same_clock_snapshots_cannot_change_bound_bay() {
    let (_p, mut s, mut now) = setup();
    let before = json(&mut s);
    assert!(s
        .apply(
            PhysicalCargoEvent::Claim {
                drone_id: 1,
                claim_id: 1,
                volume_cm3: VolumeCm3(1),
                due_at_ms: 1000
            },
            0
        )
        .is_err());
    assert_eq!(before, json(&mut s));
    command(&mut s, Command::Launch, 0);
    let stable = json(&mut s);
    assert!(s.bay_command(0, Command::Hold, 0).is_err());
    assert_eq!(stable, json(&mut s));
    s.bay_tick(0).unwrap();
    s.bay_view().unwrap();
    assert_eq!(stable, json(&mut s));
    ticks(&mut s, &mut now, 5);
    let prior = json(&mut s);
    assert!(s.bay_tick(now + 1001).is_err());
    assert_eq!(prior, json(&mut s));
}
#[test]
fn reached_valves_required_and_fault_hold_reopen_does_not_catch_up() {
    let (p, mut s, mut now) = setup();
    launch(&mut s, &mut now);
    mine(&mut s, &mut now);
    command(&mut s, Command::Service, now);
    ticks(&mut s, &mut now, 65);
    let pre = s.export().unwrap();
    assert_eq!(pre.account.ship_payload_cm3, VolumeCm3(0));
    assert!(pre.bay.as_ref().unwrap().service_id.is_none());
    ticks(&mut s, &mut now, 5);
    let at = s.bay_view().unwrap();
    assert_eq!(at.state.frame_index, 7);
    assert!(at.source_frame.milestones.ore_coupled);
    assert_eq!(at.drone_inlet_travel_mm, 250.0);
    command(&mut s, Command::SetAvailable { available: false }, now);
    let pose = s.bay_view().unwrap().source_frame;
    drop(s);
    now += 50000;
    let mut s = PhysicalCargoInspection::open(&p.0, genesis(), now).unwrap();
    assert_eq!(s.bay_view().unwrap().source_frame, pose);
    assert_eq!(s.export().unwrap().account.ship_payload_cm3, VolumeCm3(0));
    assert!(s.bay_tick(now + 100).is_err());
    command(&mut s, Command::SetAvailable { available: true }, now);
    command(&mut s, Command::Resume, now);
    ticks(&mut s, &mut now, 4);
    assert_eq!(s.export().unwrap().account.ship_payload_cm3, VolumeCm3(0));
    ticks(&mut s, &mut now, 1);
    assert_eq!(s.export().unwrap().account.ship_payload_cm3, VolumeCm3(500));
}
#[test]
fn due_tick_loss_precedes_extraction_and_transfer_without_double_debit() {
    let (_p, mut s, mut now) = setup();
    launch(&mut s, &mut now);
    command(
        &mut s,
        Command::Mine {
            volume_cm3: VolumeCm3(1000),
        },
        now,
    );
    now += 1000;
    command(&mut s, Command::Damage { amount: 100 }, now);
    let out = s.export().unwrap();
    assert_eq!(out.resource_remaining_cm3, VolumeCm3(10000));
    assert_eq!(out.bay.unwrap().energy_units, 20);
    assert!(s.bay_tick(now + 100).is_err());
    let (_p2, mut t, mut at) = setup();
    launch(&mut t, &mut at);
    mine(&mut t, &mut at);
    command(&mut t, Command::Service, at);
    ticks(&mut t, &mut at, 70);
    at += 500;
    command(&mut t, Command::Damage { amount: 100 }, at);
    let out = t.export().unwrap();
    assert_eq!(out.account.ship_payload_cm3, VolumeCm3(0));
    assert_eq!(out.account.destroyed_cm3, VolumeCm3(1000));
    let seq = t.bay_view().unwrap().state.step_sequence;
    assert!(t
        .bay_command(seq, Command::Damage { amount: 1 }, at)
        .is_err());
}
#[test]
fn cancel_preserves_partial_pose_and_insufficient_energy_or_resource_is_atomic() {
    let (_p, mut s, mut now) = setup();
    command(&mut s, Command::Launch, now);
    ticks(&mut s, &mut now, 13);
    let pose = s.bay_view().unwrap().source_frame;
    command(&mut s, Command::Cancel, now);
    assert_eq!(pose, s.bay_view().unwrap().source_frame);
    now += 5000;
    command(&mut s, Command::Resume, now);
    ticks(&mut s, &mut now, 47);
    assert!(s.bay_view().unwrap().state.deployed);
    let before = json(&mut s);
    let seq = s.bay_view().unwrap().state.step_sequence;
    assert!(s
        .bay_command(
            seq,
            Command::Mine {
                volume_cm3: VolumeCm3(1001)
            },
            now
        )
        .is_err());
    assert_eq!(before, json(&mut s));
    let p = Temp::new();
    let mut g = genesis();
    g.resource_cm3 = VolumeCm3(999);
    let mut s = PhysicalCargoInspection::open(&p.0, g, 0).unwrap();
    s.bind_bay(certified_inspection_binding(1, 1000, 1).unwrap(), 0)
        .unwrap();
    let mut now = 0;
    launch(&mut s, &mut now);
    let before = json(&mut s);
    let seq = s.bay_view().unwrap().state.step_sequence;
    assert!(s
        .bay_command(
            seq,
            Command::Mine {
                volume_cm3: VolumeCm3(1000)
            },
            now
        )
        .is_err());
    assert_eq!(before, json(&mut s));
}
#[test]
fn durable_pose_tamper_and_transaction_failure_are_rejected_without_partial_events() {
    let (p, mut s, mut now) = setup();
    launch(&mut s, &mut now);
    let before = json(&mut s);
    s.server.db.execute_batch("CREATE TRIGGER fail_bay BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    let seq = s.bay_view().unwrap().state.step_sequence;
    assert!(s
        .bay_command(
            seq,
            Command::Mine {
                volume_cm3: VolumeCm3(1000)
            },
            now
        )
        .is_err());
    assert_eq!(before, json(&mut s));
    s.server.db.execute_batch("DROP TRIGGER fail_bay").unwrap();
    drop(s);
    let db = Connection::open(&p.0).unwrap();
    db.execute("UPDATE world SET json=json_set(json,'$.physical_cargo.bay.energy_units',999999) WHERE id=1",[]).unwrap();
    drop(db);
    assert!(PhysicalCargoInspection::open(&p.0, genesis(), now).is_err());
}
#[test]
fn source_mismatch_and_late_binding_do_not_relabel_an_existing_world() {
    let p = Temp::new();
    let mut s = PhysicalCargoInspection::open(&p.0, genesis(), 0).unwrap();
    let before = json(&mut s);
    let mut wrong = certified_inspection_binding(1, 1000, 20).unwrap();
    wrong.fixture_sha256 = "0".repeat(64);
    assert!(s.bind_bay(wrong, 0).is_err());
    assert_eq!(before, json(&mut s));
    s.apply(
        PhysicalCargoEvent::Claim {
            drone_id: 1,
            claim_id: 1,
            volume_cm3: VolumeCm3(1),
            due_at_ms: 1000,
        },
        0,
    )
    .unwrap();
    assert!(s
        .bind_bay(certified_inspection_binding(1, 1000, 20).unwrap(), 0)
        .is_err());
}
