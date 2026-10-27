use super::*;
use arkline_protocol::physical_cargo::{
    catalog::{AuthoredCargoOperatingRatings, PhysicalCargoForm, PhysicalCargoMaterial},
    AuthoredVolumeRate, CargoMassKnowledge,
};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "arkline-physical-{}-{}.sqlite",
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
    let rate = |amount, interval| AuthoredVolumeRate {
        volume_cm3: VolumeCm3(amount),
        interval_ms: interval,
    };
    // Explicit inspection-only rates. Neither measured flow ratings nor proposed economy.
    let catalog = PhysicalCargoCatalog {
        schema_version: 1,
        materials: vec![PhysicalCargoMaterial {
            material_id: "inspection.bulk".into(),
            form: PhysicalCargoForm::BulkVolumeCm3,
            mass: CargoMassKnowledge::Unknown,
            authored_price: None,
        }],
        operating_ratings: vec![AuthoredCargoOperatingRatings {
            rating_id: "inspection.authored".into(),
            revision: 1,
            drone_extraction: rate(893833, 1000),
            ship_extraction: rate(1, 1000),
            vessel_manifold: rate(893833, 500),
            drone_batch_energy_units: 1,
            ship_batch_energy_units: 1,
        }],
    };
    PhysicalCargoGenesis {
        source_configuration_sha256: source.identity_sha256().unwrap(),
        design: catalog
            .bind_manifest(
                "inspection.bulk",
                "inspection.authored",
                source.measurements().unwrap(),
            )
            .unwrap(),
        catalog,
        mining_drone_ids: (1..=20).collect(),
        resource_cm3: VolumeCm3(300_000_000),
    }
}
fn json(x: &PhysicalCargoExport) -> String {
    serde_json::to_string(x).unwrap()
}
fn mine(s: &mut PhysicalCargoInspection, id: u64, claim: u64, volume: u64, now: u64) {
    s.apply(
        PhysicalCargoEvent::Claim {
            drone_id: id,
            claim_id: claim,
            volume_cm3: VolumeCm3(volume),
            due_at_ms: now + 1000,
        },
        now,
    )
    .unwrap();
    s.apply(
        PhysicalCargoEvent::Extract {
            drone_id: id,
            claim_id: claim,
        },
        now + 1000,
    )
    .unwrap();
}
fn transfer(
    drone: u64,
    service: u64,
    sequence: u64,
    request: u64,
    volume: u64,
) -> PhysicalCargoEvent {
    PhysicalCargoEvent::Transfer {
        drone_id: drone,
        service_id: service,
        sequence,
        request_id: request,
        volume_cm3: VolumeCm3(volume),
    }
}
#[test]
fn actual_source_volume_transfer_hold_reopen_retry_and_loss_conserve() {
    let p = Temp::new();
    let g = genesis();
    let mut s = PhysicalCargoInspection::open(&p.0, g.clone(), 100).unwrap();
    mine(&mut s, 1, 1, 800_000, 100);
    let before = s.export().unwrap();
    s.apply(
        PhysicalCargoEvent::ServiceOpened {
            drone_id: 1,
            service_id: 1,
            ready_at_ms: 1600,
        },
        1100,
    )
    .unwrap();
    let opened = json(&s.export().unwrap());
    assert!(s.apply(transfer(1, 1, 1, 1, 300_000), 1599).is_err());
    assert_eq!(json(&s.export().unwrap()), opened);
    s.apply(transfer(1, 1, 1, 1, 300_000), 1600).unwrap();
    let once = json(&s.export().unwrap());
    s.apply(transfer(1, 1, 1, 1, 300_000), 1600).unwrap();
    assert_eq!(json(&s.export().unwrap()), once);
    assert!(s.apply(transfer(1, 1, 1, 1, 300_001), 1600).is_err());
    s.apply(
        PhysicalCargoEvent::Claim {
            drone_id: 2,
            claim_id: 1,
            volume_cm3: VolumeCm3(10),
            due_at_ms: 2600,
        },
        1600,
    )
    .unwrap();
    drop(s);
    let mut s = PhysicalCargoInspection::open(&p.0, g, 10_000).unwrap();
    let held = s.export().unwrap();
    assert!(held
        .account
        .drones
        .iter()
        .find(|h| h.drone_id == 2)
        .unwrap()
        .claim
        .is_none());
    assert!(s.apply(transfer(1, 1, 2, 2, 500_000), 10_000).is_err());
    s.apply(
        PhysicalCargoEvent::ServiceResumed {
            service_id: 1,
            ready_at_ms: 10_500,
        },
        10_000,
    )
    .unwrap();
    assert!(s.apply(transfer(1, 1, 2, 2, 500_000), 10_499).is_err());
    let done = s.apply(transfer(1, 1, 2, 2, 500_000), 10_500).unwrap();
    assert_eq!(done.account.ship_payload_cm3, VolumeCm3(800_000));
    assert_eq!(done.resource_remaining_cm3, before.resource_remaining_cm3);
    s.apply(PhysicalCargoEvent::ServiceClosed { service_id: 1 }, 10_500)
        .unwrap();
    mine(&mut s, 2, 2, 90, 10_500);
    let lost = s
        .apply(PhysicalCargoEvent::DroneLost { drone_id: 2 }, 11_500)
        .unwrap();
    assert_eq!(lost.account.destroyed_cm3, VolumeCm3(90));
    assert!(s
        .apply(PhysicalCargoEvent::DroneLost { drone_id: 2 }, 11_500)
        .is_err());
    lost.verify().unwrap();
    // Original transfer remains idempotent even after close/other later operations.
    let prior = json(&s.export().unwrap());
    s.apply(transfer(1, 1, 1, 1, 300_000), 11_500).unwrap();
    assert_eq!(json(&s.export().unwrap()), prior);
}
#[test]
fn actual_source_capacity_resource_reservation_and_sql_rollback_fail_closed() {
    let p = Temp::new();
    let mut g = genesis();
    g.resource_cm3 = VolumeCm3(893_834);
    let mut s = PhysicalCargoInspection::open(&p.0, g, 100).unwrap();
    s.apply(
        PhysicalCargoEvent::Claim {
            drone_id: 1,
            claim_id: 1,
            volume_cm3: VolumeCm3(893_833),
            due_at_ms: 1100,
        },
        100,
    )
    .unwrap();
    assert!(s
        .apply(
            PhysicalCargoEvent::Claim {
                drone_id: 2,
                claim_id: 1,
                volume_cm3: VolumeCm3(2),
                due_at_ms: 1100
            },
            100
        )
        .is_err());
    assert!(s
        .apply(
            PhysicalCargoEvent::Extract {
                drone_id: 1,
                claim_id: 1
            },
            1099
        )
        .is_err());
    s.apply(
        PhysicalCargoEvent::Extract {
            drone_id: 1,
            claim_id: 1,
        },
        1100,
    )
    .unwrap();
    assert!(s
        .apply(
            PhysicalCargoEvent::Claim {
                drone_id: 1,
                claim_id: 2,
                volume_cm3: VolumeCm3(1),
                due_at_ms: 2100
            },
            1100
        )
        .is_err());
    s.apply(
        PhysicalCargoEvent::ServiceOpened {
            drone_id: 1,
            service_id: 1,
            ready_at_ms: 1600,
        },
        1100,
    )
    .unwrap();
    let before = json(&s.export().unwrap());
    s.server.db.execute_batch("CREATE TRIGGER fail_test_world BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'test rollback'); END;").unwrap();
    assert!(s.apply(transfer(1, 1, 1, 1, 100), 1600).is_err());
    assert_eq!(json(&s.export().unwrap()), before);
    s.server
        .db
        .execute_batch("DROP TRIGGER fail_test_world;")
        .unwrap();
    s.apply(transfer(1, 1, 1, 1, 100), 1600).unwrap();
}
#[test]
fn physical_factory_rejects_legacy_rebinding_tamper_and_normal_open() {
    let legacy = Temp::new();
    let g = genesis();
    drop(Server::open(&legacy.0).unwrap());
    let old = Connection::open(&legacy.0)
        .unwrap()
        .query_row::<String, _, _>("SELECT json FROM world", [], |r| r.get(0))
        .unwrap();
    assert!(PhysicalCargoInspection::open(&legacy.0, g.clone(), 0).is_err());
    assert_eq!(
        Connection::open(&legacy.0)
            .unwrap()
            .query_row::<String, _, _>("SELECT json FROM world", [], |r| r.get(0))
            .unwrap(),
        old
    );
    let p = Temp::new();
    let mut s = PhysicalCargoInspection::open(&p.0, g.clone(), 0).unwrap();
    let good = json(&s.export().unwrap());
    drop(s);
    assert!(Server::open(&p.0).is_err());
    let mut wrong = g.clone();
    wrong.design.manifest.ship_hold.usable_cells[0].max[0] += 1;
    assert!(PhysicalCargoInspection::open(&p.0, wrong, 0).is_err());
    let mut s = PhysicalCargoInspection::open(&p.0, g.clone(), 0).unwrap();
    assert_eq!(json(&s.export().unwrap()), good);
    assert!(s
        .server
        .db
        .execute(
            "UPDATE world SET json=json_remove(json,'$.physical_cargo')",
            []
        )
        .is_err());
    s.server
        .db
        .execute(
            "UPDATE world SET json=json_set(json,'$.physical_cargo.account.ship_payload_cm3',1)",
            [],
        )
        .unwrap();
    drop(s);
    assert!(PhysicalCargoInspection::open(&p.0, g, 0).is_err());
}
#[test]
fn entire_ship_measured_capacity_is_reserved_not_guessed() {
    let p = Temp::new();
    let g = genesis();
    let cap = g.design.manifest.ship_hold.capacity_cm3().unwrap().0;
    let batch = g
        .design
        .manifest
        .mining_drone_hold
        .capacity_cm3()
        .unwrap()
        .0;
    let mut s = PhysicalCargoInspection::open(&p.0, g, 0).unwrap();
    let mut now = 0;
    let mut remaining = cap;
    let mut claim = 0;
    while remaining > 0 {
        claim += 1;
        let volume = remaining.min(batch);
        mine(&mut s, 1, claim, volume, now);
        now += 1000;
        s.apply(
            PhysicalCargoEvent::ServiceOpened {
                drone_id: 1,
                service_id: claim,
                ready_at_ms: now + 500,
            },
            now,
        )
        .unwrap();
        now += 500;
        s.apply(transfer(1, claim, 1, claim, volume), now).unwrap();
        s.apply(PhysicalCargoEvent::ServiceClosed { service_id: claim }, now)
            .unwrap();
        remaining -= volume;
    }
    let full = s.export().unwrap();
    assert_eq!(full.account.ship_payload_cm3.0, cap);
    assert!(s
        .apply(
            PhysicalCargoEvent::Claim {
                drone_id: 2,
                claim_id: 1,
                volume_cm3: VolumeCm3(1),
                due_at_ms: now + 1000
            },
            now
        )
        .is_err());
    full.verify().unwrap();
}

#[test]
fn fresh_factory_does_not_change_partial_or_unrelated_database() {
    for schema in [
        "CREATE TABLE ledger(sequence INTEGER PRIMARY KEY,json TEXT NOT NULL);",
        "CREATE TABLE unrelated(value TEXT);",
        "CREATE TABLE sqliteXcustom(value TEXT);",
    ] {
        let p = Temp::new();
        let db = Connection::open(&p.0).unwrap();
        db.execute_batch(schema).unwrap();
        let before: String = db
            .query_row(
                "SELECT group_concat(sql,';') FROM sqlite_master ORDER BY name",
                [],
                |r| r.get(0),
            )
            .unwrap();
        drop(db);
        assert!(PhysicalCargoInspection::open(&p.0, genesis(), 0).is_err());
        let db = Connection::open(&p.0).unwrap();
        let after: String = db
            .query_row(
                "SELECT group_concat(sql,';') FROM sqlite_master ORDER BY name",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
    }
}
#[test]
fn transfer_retry_performs_no_world_or_ledger_write() {
    let p = Temp::new();
    let mut s = PhysicalCargoInspection::open(&p.0, genesis(), 0).unwrap();
    mine(&mut s, 1, 1, 100, 0);
    s.apply(
        PhysicalCargoEvent::ServiceOpened {
            drone_id: 1,
            service_id: 1,
            ready_at_ms: 1500,
        },
        1000,
    )
    .unwrap();
    let event = transfer(1, 1, 1, 1, 100);
    s.apply(event.clone(), 1500).unwrap();
    let before = json(&s.export().unwrap());
    s.server.db.execute_batch("CREATE TRIGGER no_world_write BEFORE UPDATE ON world BEGIN SELECT RAISE(ABORT,'unexpected retry write'); END; CREATE TRIGGER no_ledger_write BEFORE INSERT ON ledger BEGIN SELECT RAISE(ABORT,'unexpected retry ledger'); END;").unwrap();
    s.apply(event, 1500).unwrap();
    assert_eq!(before, json(&s.export().unwrap()));
}
#[test]
fn physical_reopen_rejects_missing_or_modified_downgrade_guard() {
    for altered in [false, true] {
        let p = Temp::new();
        let s = PhysicalCargoInspection::open(&p.0, genesis(), 0).unwrap();
        s.server
            .db
            .execute_batch("DROP TRIGGER preserve_physical_cargo_v1;")
            .unwrap();
        if altered {
            s.server.db.execute_batch("CREATE TRIGGER preserve_physical_cargo_v1 BEFORE UPDATE ON world BEGIN SELECT 1; END;").unwrap();
        }
        drop(s);
        assert!(PhysicalCargoInspection::open(&p.0, genesis(), 0).is_err());
    }
}
