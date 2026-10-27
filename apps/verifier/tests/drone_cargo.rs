//! Independent rehashed adversarial ledger fixtures, not authority-produced balances.
use arkline_protocol::*;
use arkline_verifier::verify;

fn append(e: &mut LedgerExport, at: u64, event: EconomicEvent) {
    let mut entry = LedgerEntry {
        sequence: e.entries.len() as u64 + 1,
        ruleset_version: DRONE_CARGO_RULESET_VERSION,
        timestamp_ms: at,
        previous_hash: e.snapshot.ledger_hash.clone(),
        event,
        hash: String::new(),
    };
    entry.hash = entry.calculate_hash();
    e.snapshot.ledger_sequence = entry.sequence;
    e.snapshot.ledger_hash = entry.hash.clone();
    e.snapshot.server_time_ms = at;
    e.entries.push(entry);
}
fn event(record: DroneCargoLedgerEvent) -> EconomicEvent {
    EconomicEvent::DroneCargo { record }
}
fn record_mut(e: &mut LedgerExport, index: usize) -> &mut DroneCargoLedgerEvent {
    let EconomicEvent::DroneCargo { record } = &mut e.entries[index].event else {
        panic!("cargo fixture")
    };
    record
}
fn rehash(e: &mut LedgerExport) {
    let mut previous = GENESIS_HASH.to_owned();
    for (i, entry) in e.entries.iter_mut().enumerate() {
        entry.sequence = i as u64 + 1;
        entry.previous_hash = previous;
        entry.hash = entry.calculate_hash();
        previous = entry.hash.clone();
    }
    e.snapshot.ledger_sequence = e.entries.len() as u64;
    e.snapshot.ledger_hash = previous;
}
fn fixture() -> LedgerExport {
    let registration = supported_drone_cargo_registration();
    let binding = registration.bind("vessel-A".into());
    let mut pilot: PilotView = serde_json::from_value(serde_json::json!({
        "name":"Alice","credits":STARTER_CREDITS,"ore":0,"location":{"kind":"station"},
        "flight":{"position":[0,0,0],"velocity":[0,0,0],"heading":[1,0,0],
          "mode":{"mode":"stop"},"updated_at_ms":1000,
          "docking":{"version":1,"host_vessel_id":"vessel-A","berth_id":0,
          "operation_id":0,"phase":"free","segment":null,"clamps":0.,
          "updated_at_ms":1000,"hold_reason":null,"departing":false}}
    }))
    .unwrap();
    pilot.drone_bay = Some(DroneBayPublicState {
        schema_version: DRONE_BAY_SCHEMA_VERSION,
        batch_capability: Some(DRONE_BAY_BATCH_VERSION),
        batch: None,
        binding: registration.base.bind("vessel-A".into()),
        cradles: drone_bay_layout::home_poses()
            .into_iter()
            .enumerate()
            .map(|(i, body_pose)| DroneCradleView {
                drone_id: i as u64 + 1,
                class: [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack][i / 5],
                hit_points: 100,
                home_cradle: i as u8,
                presence: DroneBayPresence::Active,
                body_pose,
            })
            .collect(),
        motions: vec![],
        machinery: DroneBayMachineryView {
            cleanup: None,
            handling_operation: None,
            door_fraction: 0.,
            door_segment: None,
            groups: vec![],
        },
    });
    pilot.drone_cargo = Some(DroneCargoPublicState {
        schema_version: DRONE_CARGO_SCHEMA_VERSION,
        binding,
        holds: (6..=10)
            .map(|drone_id| DroneOreHoldView {
                drone_id,
                ore_units: 0,
                capacity_units: MINING_DRONE_CARGO_CAPACITY,
                cycle_reserved_units: 0,
            })
            .collect(),
        incoming_reserved_units: 0,
        operations: vec![],
        receiver_operation: None,
        cleanup: None,
        motion: None,
        groups: vec![],
    });
    let mut e = LedgerExport {
        protocol_version: PROTOCOL_VERSION,
        entries: vec![],
        snapshot: serde_json::from_value(serde_json::json!({
            "server_time_ms":0,"pilots":[],"orders":[],"belt_ore":INITIAL_BELT_ORE,
            "ledger_sequence":0,"ledger_hash":GENESIS_HASH
        }))
        .unwrap(),
    };
    e.snapshot.pilots.push(pilot);
    e.snapshot.drone_cargo_registration = Some(registration.clone());
    e.snapshot.drone_cargo_totals = Some(DroneOreTotals {
        schema_version: DRONE_CARGO_SCHEMA_VERSION,
        drone_ore_units: 0,
        lost_ore_units: 0,
    });
    append(
        &mut e,
        0,
        EconomicEvent::PilotCreated {
            pilot: "Alice".into(),
        },
    );
    append(
        &mut e,
        1000,
        event(DroneCargoLedgerEvent::Initialized {
            pilot: "Alice".into(),
            vessel_id: "vessel-A".into(),
            registration,
            drone_ids: (6..=10).collect(),
            capacity_units: 100,
        }),
    );
    e
}
fn extract(e: &mut LedgerExport, cycle: u64, at: u64, units: u64) {
    append(
        e,
        at,
        event(DroneCargoLedgerEvent::Extracted {
            pilot: "Alice".into(),
            vessel_id: "vessel-A".into(),
            drone_id: 6,
            cycle_id: cycle,
            target_id: asteroid_catalog(INITIAL_BELT_ORE)[0].id,
            cycle_started_at_ms: at - 8000,
            units,
            job_id: None,
        }),
    );
    e.snapshot.belt_ore -= units;
    let cargo = e.snapshot.pilots[0].drone_cargo.as_mut().unwrap();
    cargo.holds[0].ore_units += units;
    cargo.incoming_reserved_units += units;
    e.snapshot
        .drone_cargo_totals
        .as_mut()
        .unwrap()
        .drone_ore_units += units;
}
fn transfer(e: &mut LedgerExport, sequence: u64, at: u64, units: u64, requested: u64) {
    append(
        e,
        at,
        event(DroneCargoLedgerEvent::Transferred {
            pilot: "Alice".into(),
            vessel_id: "vessel-A".into(),
            drone_id: 6,
            operation_id: 1,
            operation_started_at_ms: 10000,
            requested_units: requested,
            sequence,
            units,
        }),
    );
    e.snapshot.pilots[0].ore += units;
    let cargo = e.snapshot.pilots[0].drone_cargo.as_mut().unwrap();
    cargo.holds[0].ore_units -= units;
    cargo.incoming_reserved_units -= units;
    e.snapshot
        .drone_cargo_totals
        .as_mut()
        .unwrap()
        .drone_ore_units -= units;
}
#[test]
fn extraction_transfer_loss_conserve_without_double_credit() {
    let mut e = fixture();
    extract(&mut e, 1, 10000, 10);
    assert_eq!(verify(&e, None).unwrap().belt_ore, INITIAL_BELT_ORE - 10);
    assert_eq!(e.snapshot.pilots[0].ore, 0);
    transfer(&mut e, 1, 11000, 6, 10);
    append(
        &mut e,
        12000,
        event(DroneCargoLedgerEvent::Destroyed {
            pilot: "Alice".into(),
            vessel_id: "vessel-A".into(),
            drone_id: 6,
            destroyed_at_ms: 12000,
            lost_units: 4,
        }),
    );
    let p = &mut e.snapshot.pilots[0];
    p.drone_cargo.as_mut().unwrap().holds[0].ore_units = 0;
    p.drone_cargo.as_mut().unwrap().incoming_reserved_units = 0;
    p.drone_bay.as_mut().unwrap().cradles[5].presence = DroneBayPresence::Destroyed;
    p.drone_bay.as_mut().unwrap().cradles[5].hit_points = 0;
    e.snapshot
        .drone_cargo_totals
        .as_mut()
        .unwrap()
        .drone_ore_units = 0;
    e.snapshot
        .drone_cargo_totals
        .as_mut()
        .unwrap()
        .lost_ore_units = 4;
    verify(&e, None).unwrap();
    assert_eq!(e.snapshot.pilots[0].ore, 6);
    for case in ["lost", "mint", "resurrect", "duplicate-loss"] {
        let mut bad = e.clone();
        match case {
            "lost" => {
                bad.snapshot
                    .drone_cargo_totals
                    .as_mut()
                    .unwrap()
                    .lost_ore_units = 3
            }
            "mint" => bad.snapshot.pilots[0].ore += 4,
            "resurrect" => extract(&mut bad, 2, 21000, 1),
            "duplicate-loss" => {
                let last = bad.entries.last().unwrap().event.clone();
                append(&mut bad, 12000, last);
            }
            _ => unreachable!(),
        }
        assert!(verify(&bad, None).is_err(), "{case}");
    }
}
#[test]
fn world_rulesets_and_initialization_fail_closed() {
    let good = fixture();
    verify(&good, None).unwrap();
    for case in [
        "mixed",
        "legacy",
        "unknown",
        "missing-init",
        "double-init",
        "duplicate-id",
        "zero-id",
        "capacity",
        "source",
        "missing-snapshot",
        "foreign",
    ] {
        let mut e = good.clone();
        match case {
            "mixed" => e.entries[0].ruleset_version = 1,
            "legacy" => e.snapshot.drone_cargo_registration = None,
            "unknown" => e.entries[0].ruleset_version = 99,
            "missing-init" => {
                e.entries.pop();
            }
            "double-init" => {
                let init = e.entries[1].event.clone();
                append(&mut e, 2000, init);
            }
            "duplicate-id" | "zero-id" | "capacity" | "source" => {
                if let DroneCargoLedgerEvent::Initialized {
                    drone_ids,
                    capacity_units,
                    registration,
                    ..
                } = record_mut(&mut e, 1)
                {
                    match case {
                        "duplicate-id" => drone_ids[1] = 6,
                        "zero-id" => drone_ids[0] = 0,
                        "capacity" => *capacity_units = 101,
                        "source" => registration.receiver_revision += 1,
                        _ => unreachable!(),
                    }
                }
            }
            "missing-snapshot" => e.snapshot.pilots[0].drone_cargo = None,
            "foreign" => {
                e.snapshot.pilots[0]
                    .drone_cargo
                    .as_mut()
                    .unwrap()
                    .binding
                    .vessel_id = "foreign".into()
            }
            _ => unreachable!(),
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err(), "{case}");
    }
}
#[test]
fn extraction_identity_quantity_maturity_and_uniqueness() {
    let mut good = fixture();
    extract(&mut good, 1, 10000, 10);
    verify(&good, None).unwrap();
    for case in [
        "pilot", "vessel", "drone", "cycle", "target", "zero", "yield", "early", "job",
    ] {
        let mut e = good.clone();
        if let DroneCargoLedgerEvent::Extracted {
            pilot,
            vessel_id,
            drone_id,
            cycle_id,
            target_id,
            cycle_started_at_ms,
            units,
            job_id,
        } = record_mut(&mut e, 2)
        {
            match case {
                "pilot" => *pilot = "Bob".into(),
                "vessel" => *vessel_id = "B".into(),
                "drone" => *drone_id = 1,
                "cycle" => *cycle_id = 0,
                "target" => *target_id = u64::MAX,
                "zero" => *units = 0,
                "yield" => *units = 11,
                "early" => *cycle_started_at_ms = 2001,
                "job" => *job_id = Some(0),
                _ => unreachable!(),
            }
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err(), "{case}");
    }
    extract(&mut good, 3, 20000, 10);
    verify(&good, None).unwrap(); // canceled-cycle gap is legal
    extract(&mut good, 3, 30000, 10);
    assert!(verify(&good, None).is_err());
}
#[test]
fn transfer_batches_cannot_reuse_accelerate_or_change_intent() {
    let mut good = fixture();
    extract(&mut good, 1, 10000, 10);
    transfer(&mut good, 1, 11000, 5, 10);
    transfer(&mut good, 2, 11500, 5, 10);
    verify(&good, None).unwrap();
    for case in [
        "sequence",
        "operation",
        "drone",
        "vessel",
        "request",
        "start",
        "fast",
        "overdraw",
        "oversize",
    ] {
        let mut e = good.clone();
        if let DroneCargoLedgerEvent::Transferred {
            sequence,
            operation_id,
            drone_id,
            vessel_id,
            requested_units,
            operation_started_at_ms,
            units,
            ..
        } = record_mut(&mut e, 4)
        {
            match case {
                "sequence" => *sequence = 1,
                "operation" => *operation_id = 2,
                "drone" => *drone_id = 7,
                "vessel" => *vessel_id = "B".into(),
                "request" => *requested_units = 9,
                "start" => *operation_started_at_ms = 10001,
                "overdraw" => *units = 6,
                "oversize" => *units = 26,
                "fast" => {}
                _ => unreachable!(),
            }
        }
        if case == "fast" {
            e.entries[4].timestamp_ms = 11499;
        }
        rehash(&mut e);
        assert!(verify(&e, None).is_err(), "{case}");
    }
}
#[test]
fn claims_are_capacity_only_not_ore_and_stored_loaded_is_invalid() {
    let mut e = fixture();
    extract(&mut e, 1, 10000, 10);
    let c = e.snapshot.pilots[0].drone_cargo.as_mut().unwrap();
    c.holds[0].cycle_reserved_units = 10;
    c.incoming_reserved_units = 20;
    verify(&e, None).unwrap();
    for case in [
        "claim",
        "sum",
        "stored",
        "class",
        "duplicate",
        "balance",
        "totals",
    ] {
        let mut bad = e.clone();
        let p = &mut bad.snapshot.pilots[0];
        let c = p.drone_cargo.as_mut().unwrap();
        match case {
            "claim" => c.holds[0].cycle_reserved_units = 11,
            "sum" => c.incoming_reserved_units = 10,
            "stored" => {
                p.drone_bay.as_mut().unwrap().cradles[5].presence = DroneBayPresence::Stored
            }
            "class" => p.drone_bay.as_mut().unwrap().cradles[5].class = DroneClass::Recon,
            "duplicate" => c.holds[1].drone_id = 6,
            "balance" => c.holds[0].ore_units = 9,
            "totals" => {
                bad.snapshot
                    .drone_cargo_totals
                    .as_mut()
                    .unwrap()
                    .drone_ore_units = 20
            }
            _ => unreachable!(),
        }
        assert!(verify(&bad, None).is_err(), "{case}");
    }
}
#[test]
fn per_drone_and_combined_capacity_apply_at_each_event() {
    let mut e = fixture();
    for cycle in 1..=10 {
        extract(&mut e, cycle, cycle * 10000, 10);
    }
    verify(&e, None).unwrap();
    extract(&mut e, 11, 110000, 1);
    assert!(verify(&e, None).is_err());
    let mut e = fixture();
    extract(&mut e, 1, 10000, 10);
    let mut time = 11000;
    while e.snapshot.pilots[0].ore < 990 {
        let units = (990 - e.snapshot.pilots[0].ore).min(MAX_MINE_UNITS);
        append(
            &mut e,
            time,
            EconomicEvent::OreMined {
                pilot: "Alice".into(),
                units,
            },
        );
        e.snapshot.pilots[0].ore += units;
        e.snapshot.belt_ore -= units;
        time += 1000;
    }
    verify(&e, None).unwrap();
    let mut claimed = e.clone();
    let c = claimed.snapshot.pilots[0].drone_cargo.as_mut().unwrap();
    c.holds[0].cycle_reserved_units = 1;
    c.incoming_reserved_units += 1;
    assert!(verify(&claimed, None).is_err());
    append(
        &mut e,
        time,
        EconomicEvent::OreMined {
            pilot: "Alice".into(),
            units: 1,
        },
    );
    e.snapshot.pilots[0].ore += 1;
    e.snapshot.belt_ore -= 1;
    assert!(verify(&e, None).is_err());
}

#[test]
fn initialization_cannot_be_stripped_from_a_stamped_world() {
    let mut e = fixture();
    e.entries.pop();
    e.snapshot.pilots[0].drone_cargo = None;
    rehash(&mut e);
    assert!(verify(&e, None).is_err());
    e.entries.clear();
    e.snapshot.pilots.clear();
    rehash(&mut e);
    verify(&e, None).unwrap(); // Explicit empty fresh world is valid.
}
#[test]
fn distinct_cycles_cannot_reuse_the_same_mining_interval() {
    let mut e = fixture();
    extract(&mut e, 1, 10000, 10);
    extract(&mut e, 2, 10000, 10);
    assert!(verify(&e, None).is_err());
}
#[test]
fn retained_receipts_are_exact_ledger_evidence() {
    let mut e = fixture();
    extract(&mut e, 1, 10000, 10);
    transfer(&mut e, 1, 11000, 10, 10);
    let entry = e.entries.last().unwrap().clone();
    let cargo = e.snapshot.pilots[0].drone_cargo.as_mut().unwrap();
    cargo.operations.push(DroneDropoffOperation {
        operation_id: 1,
        binding: cargo.binding.clone(),
        drone_id: 6,
        requested_resume_mining: false,
        cancel_requested: false,
        followup: DroneDropoffFollowup::Hold,
        requested_units: 10,
        committed_units: 10,
        remaining_units: 0,
        phase: DroneDropoffPhase::Completed,
        held_phase: None,
        accepted_at_ms: 10000,
        updated_at_ms: 11000,
        next_batch_at_ms: None,
        message: "Complete".into(),
        receipts: vec![DroneOreTransferReceipt {
            sequence: 1,
            units: 10,
            committed_at_ms: 11000,
            ledger_sequence: entry.sequence,
            ledger_hash: entry.hash,
        }],
    });
    verify(&e, None).unwrap();
    for case in [
        "hash",
        "sequence",
        "units",
        "time",
        "intent",
        "owner",
        "drone",
        "committed",
        "remaining",
        "duplicate",
    ] {
        let mut bad = e.clone();
        let c = bad.snapshot.pilots[0].drone_cargo.as_mut().unwrap();
        let op = &mut c.operations[0];
        match case {
            "hash" => op.receipts[0].ledger_hash = "a".repeat(64),
            "sequence" => op.receipts[0].ledger_sequence -= 1,
            "units" => op.receipts[0].units = 9,
            "time" => op.receipts[0].committed_at_ms -= 1,
            "intent" => op.accepted_at_ms -= 1,
            "owner" => op.binding.vessel_id = "B".into(),
            "drone" => op.drone_id = 7,
            "committed" => op.committed_units = 9,
            "remaining" => op.remaining_units = 1,
            "duplicate" => {
                let clone = op.clone();
                c.operations.push(clone);
            }
            _ => unreachable!(),
        }
        assert!(verify(&bad, None).is_err(), "{case}");
    }
}

#[test]
fn receiver_spacing_and_payload_binding_apply_across_operations() {
    let mut good = fixture();
    extract(&mut good, 1, 10000, 10);
    transfer(&mut good, 1, 11000, 5, 10);
    // A cancelled partially transferred operation may be replaced, retaining
    // its committed ore. The new request owns exactly the remaining five.
    transfer(&mut good, 1, 11500, 5, 5);
    if let DroneCargoLedgerEvent::Transferred { operation_id, .. } = record_mut(&mut good, 4) {
        *operation_id = 2;
    }
    rehash(&mut good);
    verify(&good, None).unwrap();
    for case in ["fast-new-operation", "fabricated-request", "old-operation"] {
        let mut bad = good.clone();
        if case == "fast-new-operation" {
            bad.entries[4].timestamp_ms = 11499;
        }
        if let DroneCargoLedgerEvent::Transferred {
            operation_id,
            requested_units,
            ..
        } = record_mut(&mut bad, 4)
        {
            match case {
                "fabricated-request" => *requested_units = 10,
                "old-operation" => *operation_id = 0,
                _ => {}
            }
        }
        rehash(&mut bad);
        assert!(verify(&bad, None).is_err(), "{case}");
    }
}
#[test]
fn empty_destruction_is_terminal_and_hull_change_is_rejected() {
    let mut e = fixture();
    append(
        &mut e,
        2000,
        event(DroneCargoLedgerEvent::Destroyed {
            pilot: "Alice".into(),
            vessel_id: "vessel-A".into(),
            drone_id: 6,
            destroyed_at_ms: 2000,
            lost_units: 0,
        }),
    );
    let cradle = &mut e.snapshot.pilots[0].drone_bay.as_mut().unwrap().cradles[5];
    cradle.presence = DroneBayPresence::Destroyed;
    cradle.hit_points = 0;
    verify(&e, None).unwrap();
    let mut bad = e.clone();
    extract(&mut bad, 1, 10000, 1);
    assert!(verify(&bad, None).is_err());
    let mut bad = fixture();
    append(
        &mut bad,
        2000,
        EconomicEvent::ShipBoarded {
            pilot: "Alice".into(),
            from_ship_class: ShipClass::Pioneer,
            ship_class: ShipClass::Prospector,
            system_id: HOME_SYSTEM_ID,
            stored_units: 0,
            loaded_units: 0,
        },
    );
    assert!(verify(&bad, None).is_err());
}
