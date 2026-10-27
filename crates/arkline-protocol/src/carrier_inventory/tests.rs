use super::*;

pub(crate) fn fixture() -> (CarrierInventoryTopology, CarrierInventoryState) {
    let registration = CarrierInventoryRegistration {
        schema_version: 1,
        host: DroneBayRegistration {
            geometry_revision: 12,
            source_sha256: "a".repeat(64),
        },
        assembly_revision: 1,
        assembly_source_sha256: "b".repeat(64),
    };
    let side = |i: u8| {
        if i < 5 {
            CarrierSide::Port
        } else {
            CarrierSide::Starboard
        }
    };
    let classes = vec![DroneClass::Mining, DroneClass::Attack, DroneClass::Recon];
    let topology = CarrierInventoryTopology {
        registration: registration.clone(),
        ready_berths: (0..10)
            .map(|i| CarrierReadyBerthSpec {
                berth_id: i,
                side: side(i),
                compatible_classes: classes.clone(),
            })
            .collect(),
        magazine_slots: (0..10)
            .map(|i| CarrierMagazineSlotSpec {
                slot_id: u16::from(i),
                side: side(i),
                compatible_classes: classes.clone(),
            })
            .collect(),
        loaders: (0..2)
            .map(|i| CarrierLoaderSpec {
                loader_id: i,
                side: side(i * 5),
                magazine_slots: (u16::from(i) * 5..u16::from(i) * 5 + 5).collect(),
                ready_berths: (i * 5..i * 5 + 5).collect(),
                transit_buffer_id: i,
            })
            .collect(),
        transit_buffers: (0..2)
            .map(|i| CarrierTransitBufferSpec {
                buffer_id: i,
                side: side(i * 5),
            })
            .collect(),
        cassette_count: 20,
    };
    let entries: Vec<_> = (1..=20)
        .map(|id| CarrierDroneEntry {
            drone_id: id,
            class: DroneClass::Mining,
            hit_points: 100,
            location: if id <= 10 {
                CarrierDroneLocation::Ready {
                    berth_id: (id - 1) as u8,
                }
            } else {
                CarrierDroneLocation::Protected {
                    slot_id: (id - 11) as u16,
                }
            },
        })
        .collect();
    let state = CarrierInventoryState {
        schema_version: 1,
        binding: registration.bind("fixture-vessel".into()),
        initialized_at_ms: 100,
        roster: entries
            .iter()
            .map(|e| CarrierRosterIdentity {
                drone_id: e.drone_id,
                class: e.class,
                preferred_magazine_slot: if e.drone_id > 10 {
                    Some((e.drone_id - 11) as u16)
                } else {
                    None
                },
            })
            .collect(),
        cassettes: entries
            .iter()
            .map(|e| CarrierCassette {
                cassette_id: e.drone_id - 1,
                drone_id: Some(e.drone_id),
                location: CarrierCassetteLocation::Docked {
                    dock: e.location.dock().unwrap(),
                },
            })
            .collect(),
        entries,
        recall_high_watermark: 0,
        recalls: vec![],
        deployment_high_watermark: 0,
        deployments: vec![],
        high_watermark: 0,
        operations: vec![],
        reservations: vec![],
        cleanups: vec![],
    };
    (topology, state)
}
#[test]
fn all_twenty_mining_is_finite_without_class_quotas_or_buffer_capacity() {
    let (t, s) = fixture();
    assert!(t.valid_shape());
    assert!(s.validate_shape(&t));
    assert_eq!(t.capacity(), 20);
    assert_eq!(
        s.entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
            .count(),
        10
    );
    let mut bad = s.clone();
    bad.entries.push(bad.entries[0].clone());
    assert!(!bad.validate_shape(&t));
    let mut bad = t.clone();
    bad.cassette_count = 22;
    assert!(!bad.valid_shape());
    assert_eq!(t.stage_loader(4, 0, DroneClass::Mining), Some(0));
    assert_eq!(t.stage_loader(4, 5, DroneClass::Mining), None);
}
#[test]
fn both_banks_accept_actual_class_without_rewriting_identity() {
    let (t, mut s) = fixture();
    for index in 5..10 {
        s.entries[index].class = DroneClass::Attack;
        s.roster[index].class = DroneClass::Attack;
    }
    assert!(s.validate_shape(&t));
    s.entries[5].class = DroneClass::Mining;
    assert!(!s.validate_shape(&t));
}
#[test]
fn missing_duplicate_and_buffered_loaded_cassette_reject() {
    let (t, s) = fixture();
    let mut bad = s.clone();
    bad.cassettes[0].drone_id = None;
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    bad.cassettes[1].drone_id = Some(1);
    assert!(!bad.validate_shape(&t));
    let mut bad = s;
    bad.cassettes[0].location = CarrierCassetteLocation::Docked {
        dock: CarrierCassetteDock::TransitBuffer { buffer_id: 0 },
    };
    assert!(!bad.validate_shape(&t));
}
#[test]
fn queued_live_eligibility_and_terminal_replay_are_distinct() {
    let (t, mut s) = fixture();
    s.operations.push(CarrierInventoryOperation {
        recall_batch_id: None,
        operation_id: 1,
        drone_id: 11,
        action: CarrierInventoryAction::Stage { berth_id: 3 },
        phase: CarrierOperationPhase::Queued,
        held_phase: None,
        stage: None,
        berth_id: None,
        accepted_at_ms: 100,
        updated_at_ms: 100,
        message: "Queued".into(),
        cargo_service_operation_ids: vec![],
    });
    s.high_watermark = 1;
    assert!(s.validate_shape(&t));
    s.operations[0].action = CarrierInventoryAction::Launch;
    assert!(!s.validate_shape(&t));
    s.operations[0].phase = CarrierOperationPhase::Cancelled;
    assert!(s.validate_shape(&t));
}

#[test]
fn fresh_cargo_roster_is_explicit_and_aggregate_headroom_still_bounded() {
    let (_, inventory) = fixture();
    let cargo = CarrierOreState {
        binding: inventory.binding.clone(),
        holds: inventory
            .roster
            .iter()
            .map(|i| crate::DroneOreHold {
                drone_id: i.drone_id,
                ore_units: 0,
                destroyed_at_ms: None,
                intent_generation: 0,
                mining_intent: None,
                cycle_high_watermark: 0,
                cycle: None,
            })
            .collect(),
        lost_ore_units: 0,
        service_high_watermark: 0,
        services: vec![],
        waiting: vec![],
        granted_service_id: None,
        last_committed_at_ms: None,
    };
    assert!(cargo.validate_shape(&inventory));
    assert_eq!(cargo.holds.len(), 20);
    let mut full = cargo.clone();
    for h in &mut full.holds {
        h.ore_units = 100;
    }
    assert_eq!(full.available_ship_headroom(0, 1000), None);
    let mut bad = cargo.clone();
    bad.holds.pop();
    assert!(!bad.validate_shape(&inventory));
    let mut mixed = inventory.clone();
    mixed.entries[0].class = DroneClass::Attack;
    mixed.roster[0].class = DroneClass::Attack;
    assert!(!cargo.validate_shape(&mixed));
    let mut valid = cargo;
    valid.holds.remove(0);
    assert!(valid.validate_shape(&mixed));
    assert_eq!(crate::MINING_DRONE_CARGO_CAPACITY, 100);
    assert_eq!(crate::DRONE_MINING_CYCLE_MS, 8000);
    assert_eq!(crate::DRONE_MINING_YIELD, 10);
    assert_eq!(crate::DRONE_DROPOFF_BATCH_UNITS, 25);
    assert_eq!(crate::DRONE_DROPOFF_BATCH_MS, 500);
}
#[test]
fn active_manifold_grant_binds_payload_location_and_historical_berth() {
    let (t, mut inventory) = fixture();
    inventory.cassettes[0].drone_id = None;
    inventory.entries[0].location = CarrierDroneLocation::Servicing {
        operation_id: 1,
        berth_id: 0,
    };
    inventory.operations.push(CarrierInventoryOperation {
        recall_batch_id: None,
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::ReturnToUnload {
            resume_mining: true,
        },
        phase: CarrierOperationPhase::Servicing,
        held_phase: None,
        stage: None,
        berth_id: Some(0),
        accepted_at_ms: 100,
        updated_at_ms: 200,
        message: "Coupled".into(),
        cargo_service_operation_ids: vec![1],
    });
    inventory.high_watermark = 1;
    inventory.reservations.push(CarrierResourceReservation {
        resource: CarrierResource::ReadyBerth { berth_id: 0 },
        operation_id: 1,
        drone_id: 1,
    });
    assert!(inventory.validate_shape(&t));
    let mut cargo = CarrierOreState {
        binding: inventory.binding.clone(),
        holds: inventory
            .roster
            .iter()
            .map(|i| crate::DroneOreHold {
                drone_id: i.drone_id,
                ore_units: if i.drone_id == 1 { 40 } else { 0 },
                destroyed_at_ms: None,
                intent_generation: 0,
                mining_intent: None,
                cycle_high_watermark: 0,
                cycle: None,
            })
            .collect(),
        lost_ore_units: 0,
        service_high_watermark: 1,
        services: vec![CarrierOreService {
            service_id: 1,
            physical_operation_id: 1,
            drone_id: 1,
            berth_id: 0,
            accepted_at_ms: 200,
            closed_at_ms: None,
            requested_units: 40,
            receipts: vec![],
        }],
        waiting: vec![],
        granted_service_id: Some(1),
        last_committed_at_ms: None,
    };
    assert!(cargo.validate_shape(&inventory));
    cargo.holds[0].ore_units = 41;
    assert!(!cargo.validate_shape(&inventory));
    cargo.holds[0].ore_units = 40;
    cargo.services[0].berth_id = 1;
    assert!(!cargo.validate_shape(&inventory));
    cargo.services[0].berth_id = 0;
    inventory.entries[0].location = CarrierDroneLocation::Deployed;
    assert!(!cargo.validate_shape(&inventory));
    inventory.entries[0].location = CarrierDroneLocation::Ready { berth_id: 0 };
    assert!(!cargo.validate_shape(&inventory));
}
#[test]
fn identified_request_roundtrip_separates_envelope_from_intent() {
    let (_, s) = fixture();
    for action in [
        CarrierInventoryAction::Stage { berth_id: 0 },
        CarrierInventoryAction::Launch,
        CarrierInventoryAction::ReturnToUnload {
            resume_mining: true,
        },
        CarrierInventoryAction::RecallToStorage,
    ] {
        let request = CarrierInventoryRequest::Begin {
            binding: s.binding.clone(),
            operation_id: 1,
            drone_id: 11,
            action,
        };
        let raw = serde_json::to_string(&request).unwrap();
        assert_eq!(
            serde_json::from_str::<CarrierInventoryRequest>(&raw).unwrap(),
            request
        );
        assert!(raw.contains("\"kind\":\"begin\""));
    }
}

#[test]
fn deploy_all_retains_finite_ids_not_waves_or_future_inventory() {
    let (t, mut s) = fixture();
    let ids = s.entries.iter().map(|e| e.drone_id).collect::<Vec<_>>();
    s.deployment_high_watermark = 1;
    s.deployments.push(CarrierDeploymentBatch {
        batch_id: 1,
        members: ids
            .iter()
            .map(|id| CarrierDeploymentMember {
                drone_id: *id,
                stage_operation_id: None,
                launch_operation_id: None,
                cancelled: false,
                blocked_reason: String::new(),
            })
            .collect(),
        phase: CarrierDeploymentPhase::Running,
        held: false,
        accepted_at_ms: s.initialized_at_ms,
        updated_at_ms: s.initialized_at_ms,
    });
    assert!(s.validate_shape(&t));
    let mut bad = s.clone();
    let duplicate = bad.deployments[0].members[0].clone();
    bad.deployments[0].members.push(duplicate);
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    bad.deployments[0].phase = CarrierDeploymentPhase::Completed;
    assert!(!bad.validate_shape(&t));
    s.deployments[0].phase = CarrierDeploymentPhase::Cancelled;
    for m in &mut s.deployments[0].members {
        m.cancelled = true;
    }
    assert!(s.validate_shape(&t));
    assert_eq!(s.next_deployment_id(), Some(2));
    assert_eq!(
        s.deployments[0]
            .members
            .iter()
            .map(|m| m.drone_id)
            .collect::<Vec<_>>(),
        ids
    );
}

#[test]
fn loader_components_preserve_shared_banks_but_isolate_local_columns() {
    let (shared, _) = fixture();
    assert!(shared.same_dock_component(
        CarrierCassetteDock::Ready { berth_id: 0 },
        CarrierCassetteDock::Protected { slot_id: 4 }
    ));
    assert!(!shared.same_dock_component(
        CarrierCassetteDock::Ready { berth_id: 0 },
        CarrierCassetteDock::Protected { slot_id: 5 }
    ));
    let mut local = shared.clone();
    local.transit_buffers = (0..10)
        .map(|id| CarrierTransitBufferSpec {
            buffer_id: id,
            side: shared.berth(id).unwrap().side,
        })
        .collect();
    local.loaders = (0..10)
        .map(|id| CarrierLoaderSpec {
            loader_id: id,
            side: shared.berth(id).unwrap().side,
            magazine_slots: vec![u16::from(id)],
            ready_berths: vec![id],
            transit_buffer_id: id,
        })
        .collect();
    assert!(local.valid_shape());
    for id in 0..10 {
        let home = CarrierCassetteDock::Ready { berth_id: id };
        for other in 0..10 {
            assert_eq!(
                local.same_dock_component(
                    home,
                    CarrierCassetteDock::Protected {
                        slot_id: u16::from(other)
                    }
                ),
                id == other
            );
            assert_eq!(
                local.same_dock_component(
                    home,
                    CarrierCassetteDock::TransitBuffer { buffer_id: other }
                ),
                id == other
            );
        }
    }
    assert!(!local.same_dock_component(
        CarrierCassetteDock::Ready { berth_id: 99 },
        CarrierCassetteDock::Ready { berth_id: 99 }
    ));
}
