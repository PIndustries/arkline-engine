use super::*;
use arkline_protocol::physical_cargo::{
    bay::certified_inspection_binding, catalog::*, control::PHYSICAL_CONTROL_VERSION,
    AuthoredVolumeRate, CargoMassKnowledge,
};
use arkline_protocol::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
const TOKEN: &str = "local-fixture-owner-token-not-a-live-credential";
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "arkline-world-control-{}-{}.sqlite",
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
            material_id: "controlled.test.bulk".into(),
            form: PhysicalCargoForm::BulkVolumeCm3,
            mass: CargoMassKnowledge::Unknown,
            authored_price: None,
        }],
        operating_ratings: vec![AuthoredCargoOperatingRatings {
            rating_id: "controlled.test.rate".into(),
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
                "controlled.test.bulk",
                "controlled.test.rate",
                source.measurements().unwrap(),
            )
            .unwrap(),
        catalog,
        mining_drone_ids: vec![1],
        resource_cm3: VolumeCm3(10000),
    }
}
fn binding() -> Binding {
    let spec = power_spec(ShipClass::Pioneer);
    Binding {
        version: PHYSICAL_CONTROL_VERSION,
        pilot: "captain".into(),
        drone_id: 1,
        target_id: 901,
        system_id: 1,
        initial_power: PowerState {
            charge_j: spec.capacity_j,
            capacity_j: spec.capacity_j,
            regen_w: spec.regen_w,
            updated_at_ms: 0,
            regen_remainder_mj: 0,
        },
        cycle_cost_j: spec.drone_mining_cycle_cost_j,
        frozen_load_w: 0,
    }
}
fn bay_binding() -> PhysicalBayBinding {
    certified_inspection_binding(1, 1000, 123).unwrap()
}
fn connect(name: &str, token: &str) -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        pilot: name.into(),
        resume_token: Some(token.into()),
        world_layout_version: 1,
        supports_room_controls: true,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: true,
    }
}
fn setup() -> (Temp, PhysicalControlledWorld, Option<String>, u64) {
    let p = Temp::new();
    let mut w =
        PhysicalControlledWorld::open(&p.0, genesis(), bay_binding(), binding(), TOKEN, 0).unwrap();
    let mut session = None;
    assert!(w.handle_at(&mut session, connect("captain", TOKEN), 0).ok);
    (p, w, session, 0)
}
fn ok(w: &mut PhysicalControlledWorld, s: &mut Option<String>, r: Request, t: u64) -> Response {
    let out = w.handle_at(s, r, t);
    assert!(out.ok, "{}", out.message);
    out
}
fn snapshot(w: &mut PhysicalControlledWorld, s: &mut Option<String>, t: u64) -> Response {
    ok(w, s, Request::Snapshot, t)
}
fn tick(w: &mut PhysicalControlledWorld, t: &mut u64) {
    *t += 100;
    w.tick_at(*t).unwrap();
}
fn ready(w: &mut PhysicalControlledWorld, s: &mut Option<String>, t: &mut u64) {
    ok(w, s, Request::LockAsteroid { id: 901 }, *t);
    ok(
        w,
        s,
        Request::LaunchDroneClass {
            class: DroneClass::Mining,
        },
        *t,
    );
    for _ in 0..200 {
        let e = w.export().unwrap();
        if e.bay.as_ref().unwrap().deployed
            && e.bay.as_ref().unwrap().route.is_none()
            && e.control.as_ref().unwrap().projection.target_locked
        {
            return;
        }
        tick(w, t)
    }
    panic!("launch/lock did not complete")
}
fn assign(w: &mut PhysicalControlledWorld, s: &mut Option<String>, t: u64) {
    ok(
        w,
        s,
        Request::AssignMiningDrone {
            id: 1,
            target_id: 901,
        },
        t,
    );
}
fn persisted(w: &PhysicalControlledWorld) -> String {
    w.server
        .db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn existing_commands_mine_finite_volume_spend_actual_power_service_and_return() {
    let (_p, mut w, mut s, mut t) = setup();
    ready(&mut w, &mut s, &mut t);
    assign(&mut w, &mut s, t);
    let before = w.export().unwrap();
    let due = before
        .control
        .as_ref()
        .unwrap()
        .projection
        .mining_cycle
        .as_ref()
        .unwrap()
        .completes_at_ms;
    while t < due {
        tick(&mut w, &mut t)
    }
    let after = snapshot(&mut w, &mut s, t);
    let view = after.physical_cargo.as_ref().unwrap();
    assert_eq!(view.drone_payload_cm3, VolumeCm3(1000));
    assert_eq!(
        view.state.projection.resource_remaining_cm3,
        VolumeCm3(9000)
    );
    let spec = power_spec(ShipClass::Pioneer);
    assert_eq!(
        after.snapshot.as_ref().unwrap().pilots[0]
            .power
            .as_ref()
            .unwrap()
            .charge_j,
        spec.capacity_j - spec.drone_mining_cycle_cost_j
    );
    assert_eq!(after.snapshot.as_ref().unwrap().asteroids[0].ore, 0);
    assert_eq!(
        w.export().unwrap().bay.unwrap().energy_units,
        123,
        "old authored budget must not become joules"
    );
    assert!(!view.source_frame.milestones.ore_coupled);
    let mut last = VolumeCm3(0);
    for _ in 0..500 {
        let e = w.export().unwrap();
        if e.account.ship_payload_cm3.0 >= 2000 {
            break;
        }
        assert!(e.account.ship_payload_cm3 >= last);
        last = e.account.ship_payload_cm3;
        tick(&mut w, &mut t)
    }
    assert_eq!(
        w.export().unwrap().account.ship_payload_cm3,
        VolumeCm3(2000)
    );
    ok(&mut w, &mut s, Request::RecallDroneById { id: 1 }, t);
    for _ in 0..400 {
        let b = w.export().unwrap().bay.unwrap();
        if !b.deployed && b.route.is_none() {
            break;
        }
        tick(&mut w, &mut t)
    }
    let e = w.export().unwrap();
    assert!(!e.bay.as_ref().unwrap().deployed && e.bay.as_ref().unwrap().route.is_none());
    assert_eq!(e.account.ship_payload_cm3, VolumeCm3(2000));
    assert_eq!(e.resource_remaining_cm3, VolumeCm3(8000));
    e.verify().unwrap();
    let json = persisted(&w);
    let n = e.entries.len();
    snapshot(&mut w, &mut s, t);
    snapshot(&mut w, &mut s, t);
    w.tick_at(t).unwrap();
    assert_eq!(persisted(&w), json);
    assert_eq!(w.export().unwrap().entries.len(), n);
}
#[test]
fn authenticated_owner_only_and_unsupported_world_controls_never_mutate() {
    let (_p, mut w, mut s, t) = setup();
    let original = persisted(&w);
    let mut foreign = None;
    assert!(!w.handle_at(&mut foreign, connect("intruder", TOKEN), t).ok);
    assert!(foreign.is_none());
    assert!(
        !w.handle_at(&mut foreign, connect("captain", "wrong-owner-token"), t)
            .ok
    );
    assert!(foreign.is_none());
    assert!(
        !w.handle_at(
            &mut foreign,
            Request::LaunchDroneClass {
                class: DroneClass::Mining
            },
            t
        )
        .ok
    );
    for r in [
        Request::LaunchDroneClass {
            class: DroneClass::Attack,
        },
        Request::AssignMiningDrone {
            id: 2,
            target_id: 901,
        },
        Request::LockAsteroid { id: 902 },
        Request::PilotInput {
            translation: [1., 0., 0.],
            rotation: [0.; 3],
        },
        Request::StartMining {
            module_id: 0,
            target_id: 901,
        },
        Request::MineAsteroid {
            id: 901,
            units: 1000,
        },
    ] {
        assert!(!w.handle_at(&mut s, r, t).ok);
        assert_eq!(persisted(&w), original);
    }
    assert!(Server::open(&Temp::new().0).is_ok());
    let r = snapshot(&mut w, &mut s, t);
    assert!(!r.snapshot.unwrap().walk_sprint_v1);
    assert!(r.assets.is_none());
}
#[test]
fn due_time_death_and_lost_signal_cancel_before_real_extraction() {
    for death in [true, false] {
        let (_p, mut w, mut s, mut t) = setup();
        ready(&mut w, &mut s, &mut t);
        assign(&mut w, &mut s, t);
        let due = w
            .export()
            .unwrap()
            .control
            .unwrap()
            .projection
            .mining_cycle
            .unwrap()
            .completes_at_ms;
        while t + 100 < due {
            tick(&mut w, &mut t)
        }
        t = due;
        w.actor_input(
            if death {
                Action::Damage { amount: 100 }
            } else {
                Action::Signal { available: false }
            },
            t,
        )
        .unwrap();
        w.tick_at(t).unwrap();
        let e = w.export().unwrap();
        assert_eq!(e.resource_remaining_cm3, VolumeCm3(10000));
        assert_eq!(e.account.ship_payload_cm3, VolumeCm3(0));
        assert_eq!(e.account.drones[0].payload_cm3, VolumeCm3(0));
        assert!(e.account.drones[0].claim.is_none());
        assert_eq!(
            e.control.unwrap().projection.power.charge_j,
            power_spec(ShipClass::Pioneer).capacity_j
        );
    }
}
#[test]
fn target_unlock_cancels_due_claim_without_award_or_debit() {
    let (_p, mut w, mut s, mut t) = setup();
    ready(&mut w, &mut s, &mut t);
    assign(&mut w, &mut s, t);
    t += 100;
    ok(&mut w, &mut s, Request::UnlockAsteroid { id: 901 }, t);
    for _ in 0..20 {
        tick(&mut w, &mut t)
    }
    let e = w.export().unwrap();
    assert_eq!(e.resource_remaining_cm3, VolumeCm3(10000));
    assert_eq!(
        e.control.unwrap().projection.power.charge_j,
        power_spec(ShipClass::Pioneer).capacity_j
    );
}
#[test]
fn restart_freezes_actual_motion_and_raw_inspection_cannot_bypass_owner() {
    let (p, mut w, mut s, mut t) = setup();
    ready(&mut w, &mut s, &mut t);
    assign(&mut w, &mut s, t);
    for _ in 0..15 {
        tick(&mut w, &mut t)
    }
    let before = snapshot(&mut w, &mut s, t).physical_cargo.unwrap();
    drop(w);
    assert!(PhysicalCargoInspection::open(&p.0, genesis(), t + 10000).is_err());
    assert!(Server::open(&p.0).is_err());
    assert!(PhysicalControlledWorld::open(
        &p.0,
        genesis(),
        bay_binding(),
        binding(),
        "wrong-owner-token",
        t + 10000
    )
    .is_err());
    t += 10000;
    let mut w =
        PhysicalControlledWorld::open(&p.0, genesis(), bay_binding(), binding(), TOKEN, t).unwrap();
    let mut s = None;
    ok(&mut w, &mut s, connect("captain", TOKEN), t);
    let after = snapshot(&mut w, &mut s, t).physical_cargo.unwrap();
    assert_eq!(before.source_frame, after.source_frame);
    assert_eq!(before.drone_payload_cm3, after.drone_payload_cm3);
    assert!(!after.state.projection.mining);
    ok(&mut w, &mut s, Request::RecallDroneById { id: 1 }, t);
    for _ in 0..400 {
        let b = w.export().unwrap().bay.unwrap();
        if !b.deployed && b.route.is_none() {
            break;
        }
        tick(&mut w, &mut t)
    }
    let e = w.export().unwrap();
    assert_eq!(e.account.ship_payload_cm3, VolumeCm3(1000));
    assert!(!e.bay.unwrap().deployed);
}
#[test]
fn sql_failure_rolls_back_actual_power_resource_and_ledger_together() {
    let (_p, mut w, mut s, mut t) = setup();
    ready(&mut w, &mut s, &mut t);
    assign(&mut w, &mut s, t);
    let due = w
        .export()
        .unwrap()
        .control
        .unwrap()
        .projection
        .mining_cycle
        .unwrap()
        .completes_at_ms;
    while t + 100 < due {
        tick(&mut w, &mut t)
    }
    let before = persisted(&w);
    let n = w.export().unwrap().entries.len();
    w.server.db.execute_batch("CREATE TRIGGER reject_control_write BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    assert!(w.tick_at(due).is_err());
    assert_eq!(persisted(&w), before);
    assert_eq!(w.export().unwrap().entries.len(), n);
    w.server
        .db
        .execute_batch("DROP TRIGGER reject_control_write;")
        .unwrap();
    w.tick_at(due).unwrap();
    let e = w.export().unwrap();
    assert_eq!(e.resource_remaining_cm3, VolumeCm3(9000));
    let n = e.entries.len();
    w.tick_at(due).unwrap();
    assert_eq!(w.export().unwrap().entries.len(), n);
}
#[test]
fn persisted_actor_geometry_and_power_tamper_fail_closed() {
    let (_p, mut w, mut s, t) = setup();
    let original = persisted(&w);
    for path in [
        "$.asteroids[0].position[0]",
        "$.pilots.captain.view.flight.position[0]",
        "$.pilots.captain.view.power.capacity_j",
    ] {
        let sql = format!("UPDATE world SET json=json_set(json,'{path}',123456) WHERE id=1");
        w.server.db.execute(&sql, []).unwrap();
        assert!(!w.handle_at(&mut s, Request::Snapshot, t).ok);
        assert!(w.tick_at(t + 100).is_err());
        w.server
            .db
            .execute("UPDATE world SET json=?1 WHERE id=1", [&original])
            .unwrap();
    }
}

#[test]
fn rejected_controlled_open_never_restarts_existing_inspection() {
    let p = Temp::new();
    let mut inspect = PhysicalCargoInspection::open(&p.0, genesis(), 0).unwrap();
    inspect.bind_bay(bay_binding(), 0).unwrap();
    inspect
        .bay_command(
            0,
            arkline_protocol::physical_cargo::bay::PhysicalBayCommand::Launch,
            0,
        )
        .unwrap();
    inspect.bay_tick(100).unwrap();
    let before = serde_json::to_string(&inspect.export().unwrap()).unwrap();
    drop(inspect);
    assert!(
        PhysicalControlledWorld::open(&p.0, genesis(), bay_binding(), binding(), TOKEN, 1000)
            .is_err()
    );
    let db = Connection::open(&p.0).unwrap();
    let tx = db.unchecked_transaction().unwrap();
    let world = read_world(&tx).unwrap();
    let actual = export_state(&world, load_entries(&tx, true).unwrap()).unwrap();
    assert_eq!(serde_json::to_string(&actual).unwrap(), before);
}

#[test]
fn protected_restart_and_prelaunch_hold_can_launch_through_existing_commands() {
    let (p, mut w, mut s, mut t) = setup();
    ok(&mut w, &mut s, Request::DroneHoldById { id: 1 }, t);
    ok(
        &mut w,
        &mut s,
        Request::LaunchDroneClass {
            class: DroneClass::Mining,
        },
        t,
    );
    for _ in 0..60 {
        tick(&mut w, &mut t)
    }
    ok(&mut w, &mut s, Request::RecallDroneById { id: 1 }, t);
    for _ in 0..60 {
        tick(&mut w, &mut t)
    }
    assert!(!w.export().unwrap().bay.unwrap().deployed);
    drop(w);
    t += 10000;
    let mut w =
        PhysicalControlledWorld::open(&p.0, genesis(), bay_binding(), binding(), TOKEN, t).unwrap();
    let mut s = None;
    ok(&mut w, &mut s, connect("captain", TOKEN), t);
    ok(
        &mut w,
        &mut s,
        Request::LaunchDroneClass {
            class: DroneClass::Mining,
        },
        t,
    );
    for _ in 0..60 {
        tick(&mut w, &mut t)
    }
    assert!(w.export().unwrap().bay.unwrap().deployed);
}

#[test]
fn actual_capacitor_exhaustion_blocks_new_cycles_then_recovers_without_free_ore() {
    // Deliberately fast authored test scheduling exercises a reachable power limit;
    // it is not a measured actuator/mining speed or normal gameplay rating.
    let p = Temp::new();
    let mut g = genesis();
    g.resource_cm3 = VolumeCm3(100000);
    let rating = &mut g.catalog.operating_ratings[0];
    rating.drone_extraction.interval_ms = 1;
    rating.vessel_manifold.interval_ms = 1;
    g.design = g
        .catalog
        .bind_manifest(
            "controlled.test.bulk",
            "controlled.test.rate",
            physical_source_configuration()
                .unwrap()
                .measurements()
                .unwrap(),
        )
        .unwrap();
    let mut b = binding();
    b.initial_power.charge_j = 0;
    let bb = certified_inspection_binding(1, 1, 123).unwrap();
    let mut w = PhysicalControlledWorld::open(&p.0, g, bb, b, TOKEN, 0).unwrap();
    let mut s = None;
    let mut t = 0;
    ok(&mut w, &mut s, connect("captain", TOKEN), t);
    ok(&mut w, &mut s, Request::LockAsteroid { id: 901 }, t);
    ok(&mut w, &mut s, Request::DroneHoldById { id: 1 }, t);
    t = w
        .export()
        .unwrap()
        .control
        .unwrap()
        .projection
        .target_lock
        .unwrap()
        .completes_at_ms;
    w.tick_at(t).unwrap();
    ok(
        &mut w,
        &mut s,
        Request::LaunchDroneClass {
            class: DroneClass::Mining,
        },
        t,
    );
    for _ in 0..6 {
        t += 1;
        w.tick_at(t).unwrap();
    }
    assign(&mut w, &mut s, t);
    let mut exhausted = false;
    for _ in 0..800 {
        t += 1;
        w.tick_at(t).unwrap();
        let e = w.export().unwrap();
        let b = e.bay.as_ref().unwrap();
        let c = e.control.as_ref().unwrap();
        if b.deployed
            && b.route.is_none()
            && b.mining_claim_id.is_none()
            && e.account.drones[0].payload_cm3.0 == 0
            && c.projection.power.charge_j < c.binding.cycle_cost_j
        {
            exhausted = true;
            break;
        }
    }
    assert!(
        exhausted,
        "fast real cycles must reach actual capacitor limit"
    );
    let before = w.export().unwrap();
    let json = persisted(&w);
    assert!(
        !w.handle_at(
            &mut s,
            Request::AssignMiningDrone {
                id: 1,
                target_id: 901
            },
            t
        )
        .ok
    );
    assert_eq!(persisted(&w), json);
    for _ in 0..20 {
        t += 1;
        w.tick_at(t).unwrap();
    }
    assert_eq!(
        w.export().unwrap().resource_remaining_cm3,
        before.resource_remaining_cm3
    );
    ok(&mut w, &mut s, Request::DroneHoldById { id: 1 }, t);
    t += 2000;
    w.tick_at(t).unwrap();
    assign(&mut w, &mut s, t);
    t += 1;
    w.tick_at(t).unwrap();
    assert_eq!(
        w.export().unwrap().resource_remaining_cm3.0,
        before.resource_remaining_cm3.0 - 1000
    );
}

#[test]
fn real_actor_loss_at_due_transfer_destroys_payload_once_before_delivery() {
    let (_p, mut w, mut s, mut t) = setup();
    ready(&mut w, &mut s, &mut t);
    assign(&mut w, &mut s, t);
    for _ in 0..200 {
        let e = w.export().unwrap();
        if e.bay.as_ref().unwrap().service_id.is_some() {
            break;
        }
        tick(&mut w, &mut t)
    }
    let e = w.export().unwrap();
    assert_eq!(e.account.drones[0].payload_cm3, VolumeCm3(1000));
    assert!(e.bay.as_ref().unwrap().service_id.is_some());
    t += 500;
    w.actor_input(Action::Damage { amount: 100 }, t).unwrap();
    w.tick_at(t).unwrap();
    let e = w.export().unwrap();
    assert_eq!(e.account.ship_payload_cm3, VolumeCm3(0));
    assert_eq!(e.account.destroyed_cm3, VolumeCm3(1000));
    assert_eq!(e.account.drones[0].payload_cm3, VolumeCm3(0));
    assert_eq!(e.resource_remaining_cm3, VolumeCm3(9000));
    let n = e.entries.len();
    assert!(w.actor_input(Action::Damage { amount: 1 }, t).is_err());
    assert_eq!(w.export().unwrap().entries.len(), n);
    e.verify().unwrap();
}
