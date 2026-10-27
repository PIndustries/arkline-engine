use super::*;
pub(super) fn fixture() -> (CarrierInventoryTopology, CarrierInventoryState) {
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
fn ready_for_second_wave() -> (CarrierInventoryTopology, CarrierInventoryState) {
    let (t, mut s) = fixture();
    // Test setup represents a completed first wave, not a production shortcut.
    for e in &mut s.entries[..10] {
        e.location = CarrierDroneLocation::Deployed;
    }
    for c in &mut s.cassettes[..10] {
        c.drone_id = None;
    }
    assert!(s.validate_shape(&t));
    (t, s)
}
fn request(
    s: &CarrierInventoryState,
    operation_id: u64,
    drone_id: u64,
    berth_id: u8,
) -> CarrierInventoryRequest {
    CarrierInventoryRequest::Begin {
        binding: s.binding.clone(),
        operation_id,
        drone_id,
        action: CarrierInventoryAction::Stage { berth_id },
    }
}
#[test]
fn ten_reserve_identities_stage_without_refill_and_retries_never_repeat_legs() {
    let (t, mut s) = ready_for_second_wave();
    let roster = s.roster.clone();
    for i in 0..10 {
        let id = i + 1;
        let request = request(&s, id, i + 11, i as u8);
        assert!(begin_stage(&mut s, &t, &request, 200).unwrap());
        assert!(reserve_stage(&mut s, &t, id, 200).unwrap());
        for leg in 0..3 {
            let before = s.clone();
            assert!(!begin_stage(&mut s, &t, &request, 999).unwrap());
            assert_eq!(s, before);
            start_stage_leg(&mut s, &t, id).unwrap();
            assert!(s.validate_shape(&t));
            record_stage_endpoint(&mut s, &t, id, 300 + leg).unwrap();
            assert!(s.validate_shape(&t));
            assert!(matches!(
                s.entry(i + 11).unwrap().location,
                CarrierDroneLocation::Loading { .. }
            ));
            assert!(record_stage_endpoint(&mut s, &t, id, 400).is_err());
        }
        record_stage_secured(&mut s, &t, id, 500).unwrap();
        assert!(s.validate_shape(&t));
        let before = s.clone();
        assert!(!begin_stage(&mut s, &t, &request, 1000).unwrap());
        assert_eq!(s, before);
    }
    assert_eq!(s.roster, roster);
    assert_eq!(s.entries.len(), 20);
    assert_eq!(s.cassettes.len(), 20);
    assert!(s.entries.iter().all(|e| e.hit_points == 100));
    assert!(s.reservations.is_empty());
    assert_eq!(
        s.entries
            .iter()
            .filter(|e| matches!(e.location, CarrierDroneLocation::Ready { .. }))
            .count(),
        10
    );
    assert_eq!(
        s.entries
            .iter()
            .filter(|e| e.location == CarrierDroneLocation::Deployed)
            .count(),
        10
    );
}
#[test]
fn same_side_loader_serializes_but_opposite_side_can_own_exchange() {
    let (t, mut s) = ready_for_second_wave();
    for (op, drone, berth) in [(1, 11, 4), (2, 12, 0), (3, 16, 9)] {
        let request = request(&s, op, drone, berth);
        begin_stage(&mut s, &t, &request, 200).unwrap();
    }
    assert!(reserve_stage(&mut s, &t, 1, 200).unwrap());
    let before = s.clone();
    assert!(!reserve_stage(&mut s, &t, 2, 200).unwrap());
    assert_eq!(s, before);
    assert!(reserve_stage(&mut s, &t, 3, 200).unwrap());
    assert_eq!(s.reservations.len(), 12);
    assert!(s.validate_shape(&t));
    let request = request(&s, 4, 13, 5);
    let before = s.clone();
    assert!(begin_stage(&mut s, &t, &request, 200).is_err());
    assert_eq!(s, before);
}
#[test]
fn hold_and_restart_preserve_buffer_and_loaded_ready_not_prematurely_ready() {
    let (t, mut s) = ready_for_second_wave();
    let request = request(&s, 1, 11, 2);
    begin_stage(&mut s, &t, &request, 200).unwrap();
    reserve_stage(&mut s, &t, 1, 200).unwrap();
    for _ in 0..2 {
        start_stage_leg(&mut s, &t, 1).unwrap();
        record_stage_endpoint(&mut s, &t, 1, 300).unwrap();
    }
    hold_stage(&mut s, 1, 400).unwrap();
    let roots = s.cassettes.clone();
    let mut reopened: CarrierInventoryState =
        serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert!(reopened.validate_shape(&t));
    assert_eq!(reopened.cassettes, roots);
    assert!(matches!(
        reopened.entry(11).unwrap().location,
        CarrierDroneLocation::Loading { .. }
    ));
    assert!(start_stage_leg(&mut reopened, &t, 1).is_err());
    assert!(record_stage_secured(&mut reopened, &t, 1, 500).is_err());
    resume_stage(&mut reopened, 1, 100000).unwrap();
    start_stage_leg(&mut reopened, &t, 1).unwrap();
    record_stage_endpoint(&mut reopened, &t, 1, 100100).unwrap();
    record_stage_secured(&mut reopened, &t, 1, 100200).unwrap();
    assert!(reopened.validate_shape(&t));
}
#[test]
fn malformed_exchange_reservations_and_payload_reference_reject() {
    let (t, mut s) = ready_for_second_wave();
    let request = request(&s, 1, 11, 0);
    begin_stage(&mut s, &t, &request, 200).unwrap();
    reserve_stage(&mut s, &t, 1, 200).unwrap();
    let mut bad = s.clone();
    bad.reservations.pop();
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    bad.cassettes[10].drone_id = None;
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    bad.cassettes[1].drone_id = Some(11);
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    bad.operations[0].stage.as_mut().unwrap().buffer_id = 1;
    assert!(!bad.validate_shape(&t));
    let mut bad = s;
    bad.operations[0].stage.as_mut().unwrap().completed_legs = 2;
    assert!(!bad.validate_shape(&t));
}
#[test]
fn death_retains_twenty_cassettes_and_empty_cleanup_on_reopen() {
    let (t, mut s) = ready_for_second_wave();
    let request = request(&s, 1, 11, 0);
    begin_stage(&mut s, &t, &request, 200).unwrap();
    reserve_stage(&mut s, &t, 1, 200).unwrap();
    start_stage_leg(&mut s, &t, 1).unwrap();
    record_stage_endpoint(&mut s, &t, 1, 300).unwrap();
    start_stage_leg(&mut s, &t, 1).unwrap();
    mark_carried_destroyed(&mut s, &t, 1, 400).unwrap();
    let once = s.clone();
    mark_carried_destroyed(&mut s, &t, 1, 500).unwrap();
    assert_eq!(s, once);
    hold_stage(&mut s, 1, 500).unwrap();
    assert!(s.cleanups[0].held);
    assert!(s.validate_shape(&t));
    let mut s: CarrierInventoryState =
        serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert!(record_stage_endpoint(&mut s, &t, 1, 600).is_err());
    resume_stage(&mut s, 1, 700).unwrap();
    record_stage_endpoint(&mut s, &t, 1, 800).unwrap();
    start_stage_leg(&mut s, &t, 1).unwrap();
    record_stage_endpoint(&mut s, &t, 1, 900).unwrap();
    record_cleanup_secured(&mut s, &t, 1).unwrap();
    assert!(s.validate_shape(&t));
    assert_eq!(s.entries.len(), 20);
    assert_eq!(s.cassettes.len(), 20);
    assert_eq!(s.entry(11).unwrap().hit_points, 0);
    assert!(s.reservations.is_empty());
    assert!(s.cleanups.is_empty());
}
#[test]
fn third_cassette_cannot_hide_in_another_owned_loader() {
    let (t, mut s) = ready_for_second_wave();
    let request = request(&s, 1, 11, 0);
    begin_stage(&mut s, &t, &request, 200).unwrap();
    reserve_stage(&mut s, &t, 1, 200).unwrap();
    s.cassettes[1].location = CarrierCassetteLocation::InLoader {
        operation_id: 1,
        from: CarrierCassetteDock::Ready { berth_id: 1 },
        to: CarrierCassetteDock::TransitBuffer { buffer_id: 0 },
    };
    assert!(!s.validate_shape(&t));
}
#[test]
fn retained_economic_attempt_cannot_be_claimed_by_two_physical_operations() {
    let (t, mut s) = ready_for_second_wave();
    for (id, drone, berth) in [(1, 11, 0), (2, 16, 5)] {
        let request = request(&s, id, drone, berth);
        begin_stage(&mut s, &t, &request, 200).unwrap();
    }
    s.operations[0].cargo_service_operation_ids = vec![1];
    s.operations[1].cargo_service_operation_ids = vec![1];
    assert!(!s.validate_shape(&t));
}
#[test]
fn sequential_non_home_exchange_uses_actual_cassette_not_initial_berth_mapping() {
    let (t, mut s) = ready_for_second_wave();
    let first = request(&s, 1, 11, 4);
    begin_stage(&mut s, &t, &first, 200).unwrap();
    reserve_stage(&mut s, &t, 1, 200).unwrap();
    for _ in 0..3 {
        start_stage_leg(&mut s, &t, 1).unwrap();
        record_stage_endpoint(&mut s, &t, 1, 300).unwrap();
    }
    record_stage_secured(&mut s, &t, 1, 400).unwrap();
    assert_eq!(
        s.docked_cassette(CarrierCassetteDock::Ready { berth_id: 4 })
            .unwrap()
            .cassette_id,
        10
    );
    // Ownership fixture for a completed launch, not a physical route claim.
    s.entries
        .iter_mut()
        .find(|e| e.drone_id == 11)
        .unwrap()
        .location = CarrierDroneLocation::Deployed;
    s.cassettes
        .iter_mut()
        .find(|c| c.cassette_id == 10)
        .unwrap()
        .drone_id = None;
    assert!(s.validate_shape(&t));
    let next = request(&s, 2, 13, 4);
    begin_stage(&mut s, &t, &next, 500).unwrap();
    reserve_stage(&mut s, &t, 2, 500).unwrap();
    let exchange = s.operation(2).unwrap().stage.as_ref().unwrap();
    assert_eq!(exchange.loaded_cassette_id, 12);
    assert_eq!(exchange.empty_cassette_id, 10);
    assert_ne!(exchange.empty_cassette_id, 4);
    for _ in 0..3 {
        start_stage_leg(&mut s, &t, 2).unwrap();
        record_stage_endpoint(&mut s, &t, 2, 600).unwrap();
    }
    s.operations[1].phase = CarrierOperationPhase::Securing;
    record_stage_secured(&mut s, &t, 2, 700).unwrap();
    assert!(s.validate_shape(&t));
    assert_eq!(
        s.docked_cassette(CarrierCassetteDock::Protected { slot_id: 2 })
            .unwrap()
            .cassette_id,
        10
    );
    assert_eq!(
        s.docked_cassette(CarrierCassetteDock::Ready { berth_id: 4 })
            .unwrap()
            .drone_id,
        Some(13)
    );
    let before = s.clone();
    assert!(!begin_stage(&mut s, &t, &first, 800).unwrap());
    assert_eq!(s, before);
}

#[test]
fn reviewed_handoff_releases_only_loaded_owners_and_tail_never_rewrites_body() {
    let (mut t, mut s) = ready_for_second_wave();
    t.registration.assembly_revision = 1702;
    s.binding.registration = t.registration.clone();
    let req = request(&s, 1, 11, 2);
    begin_stage(&mut s, &t, &req, 200).unwrap();
    reserve_stage(&mut s, &t, 1, 200).unwrap();
    assert!(record_stage_handoff(&mut s, &t, 1, 200).is_err());
    for _ in 0..2 {
        start_stage_leg(&mut s, &t, 1).unwrap();
        record_stage_endpoint(&mut s, &t, 1, 300).unwrap();
    }
    record_stage_handoff(&mut s, &t, 1, 400).unwrap();
    let handoff = s.clone();
    assert_eq!(
        s.entry(11).unwrap().location,
        CarrierDroneLocation::Ready { berth_id: 2 }
    );
    assert_eq!(s.reservations.len(), 4);
    assert!(s.active_operation(11).is_none());
    record_stage_handoff(&mut s, &t, 1, 500).unwrap();
    assert_eq!(s, handoff);
    for bad in [0, 1, 2, 3] {
        let mut invalid = s.clone();
        match bad {
            0 => {
                invalid.operations[0]
                    .stage
                    .as_mut()
                    .unwrap()
                    .loaded_handoff_at_ms = Some(199)
            }
            1 => {
                invalid.operations[0]
                    .stage
                    .as_mut()
                    .unwrap()
                    .loaded_handoff_at_ms = Some(401)
            }
            2 => invalid.reservations.pop().map(|_| ()).unwrap(),
            _ => invalid.operations[0].stage.as_mut().unwrap().completed_legs = 1,
        }
        assert!(!invalid.validate_shape(&t));
    }
    hold_stage(&mut s, 1, 500).unwrap();
    let mut s: CarrierInventoryState =
        serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert!(s.validate_shape(&t));
    // Structural fixture only: represent a separately completed actual Launch.
    // The world regression below proves the source/tick transition instead.
    s.operations.push(CarrierInventoryOperation {
        recall_batch_id: None,
        operation_id: 2,
        drone_id: 11,
        action: CarrierInventoryAction::Launch,
        phase: CarrierOperationPhase::Completed,
        held_phase: None,
        stage: None,
        berth_id: Some(2),
        accepted_at_ms: 500,
        updated_at_ms: 550,
        message: "Structural completed-launch fixture".into(),
        cargo_service_operation_ids: vec![],
    });
    s.high_watermark = 2;
    s.entries
        .iter_mut()
        .find(|e| e.drone_id == 11)
        .unwrap()
        .location = CarrierDroneLocation::Deployed;
    s.cassettes
        .iter_mut()
        .find(|c| c.drone_id == Some(11))
        .unwrap()
        .drone_id = None;
    assert!(s.validate_shape(&t));
    resume_stage(&mut s, 1, 600).unwrap();
    start_stage_leg(&mut s, &t, 1).unwrap();
    record_stage_endpoint(&mut s, &t, 1, 700).unwrap();
    record_stage_secured(&mut s, &t, 1, 800).unwrap();
    assert_eq!(
        s.entry(11).unwrap().location,
        CarrierDroneLocation::Deployed
    );
    assert!(s.reservations.is_empty());
    assert_eq!(
        s.operations[0].stage.as_ref().unwrap().loaded_handoff_at_ms,
        Some(400)
    );
    assert_eq!(s.operations[0].stage.as_ref().unwrap().completed_legs, 3);
    assert!(s.validate_shape(&t));
}

#[test]
fn reverse_exchange_owns_all_resources_through_each_interrupted_leg() {
    let (mut t, mut s) = fixture();
    t.registration.assembly_revision = 1702;
    s.binding.registration = t.registration.clone();
    // Structural fixture only: five already-deployed protected bodies leave
    // their original empty cassettes. Actual source/world test is separate.
    for e in &mut s.entries[10..] {
        e.location = CarrierDroneLocation::Deployed;
    }
    for c in &mut s.cassettes[10..] {
        c.drone_id = None;
    }
    let req = CarrierInventoryRequest::Begin {
        binding: s.binding.clone(),
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::StoreReady { slot_id: 4 },
    };
    begin_stage(&mut s, &t, &req, 200).unwrap();
    assert!(reserve_stage(&mut s, &t, 1, 200).unwrap());
    let x = s.operation(1).unwrap().stage.clone().unwrap();
    assert_eq!(x.direction, CarrierStageDirection::ToProtected);
    let expected = [
        (
            x.empty_cassette_id,
            CarrierCassetteDock::Protected { slot_id: 4 },
            CarrierCassetteDock::TransitBuffer {
                buffer_id: x.buffer_id,
            },
        ),
        (
            x.loaded_cassette_id,
            CarrierCassetteDock::Ready { berth_id: 0 },
            CarrierCassetteDock::Protected { slot_id: 4 },
        ),
        (
            x.empty_cassette_id,
            CarrierCassetteDock::TransitBuffer {
                buffer_id: x.buffer_id,
            },
            CarrierCassetteDock::Ready { berth_id: 0 },
        ),
    ];
    for (leg, expected) in expected.into_iter().enumerate() {
        assert_eq!(current_leg(&s, 1).unwrap(), expected);
        start_stage_leg(&mut s, &t, 1).unwrap();
        hold_stage(&mut s, 1, 300 + 1000 * leg as u64).unwrap();
        let bytes = serde_json::to_vec(&s).unwrap();
        let mut reopened: CarrierInventoryState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&reopened).unwrap(), bytes);
        assert!(reopened.validate_shape(&t));
        assert_eq!(reopened.reservations.len(), 6);
        assert!(record_stage_handoff(&mut reopened, &t, 1, 400).is_err());
        resume_stage(&mut reopened, 1, 400 + 1000 * leg as u64).unwrap();
        record_stage_endpoint(&mut reopened, &t, 1, 500 + 1000 * leg as u64).unwrap();
        assert!(matches!(
            reopened.entry(1).unwrap().location,
            CarrierDroneLocation::Loading { .. }
        ));
        assert_eq!(reopened.reservations.len(), 6);
        s = reopened;
    }
    record_stage_secured(&mut s, &t, 1, 3600).unwrap();
    assert_eq!(
        s.entry(1).unwrap().location,
        CarrierDroneLocation::Protected { slot_id: 4 }
    );
    assert!(s.reservations.is_empty());
    let before = s.clone();
    assert!(!begin_stage(&mut s, &t, &req, 700).unwrap());
    assert_eq!(s, before);
}

#[test]
fn historical_forward_direction_is_omitted_and_roundtrips_byte_exact() {
    let (t, mut s) = ready_for_second_wave();
    let req = request(&s, 1, 11, 4);
    begin_stage(&mut s, &t, &req, 200).unwrap();
    reserve_stage(&mut s, &t, 1, 200).unwrap();
    let bytes = serde_json::to_vec(&s).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("direction"));
    let reopened: CarrierInventoryState = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(s, reopened);
    assert_eq!(bytes, serde_json::to_vec(&reopened).unwrap());
    let mut corrupt = reopened;
    corrupt.operations[0].stage.as_mut().unwrap().direction = CarrierStageDirection::ToProtected;
    assert!(!corrupt.validate_shape(&t));
}

#[test]
fn recall_child_links_are_bijective_and_old_empty_journal_bytes_stay_omitted() {
    let (mut t, mut s) = fixture();
    t.registration.assembly_revision = 1702;
    s.binding.registration = t.registration.clone();
    let old = serde_json::to_vec(&s).unwrap();
    assert!(!String::from_utf8_lossy(&old).contains("recall"));
    for id in [1, 2] {
        s.entries
            .iter_mut()
            .find(|e| e.drone_id == id)
            .unwrap()
            .location = CarrierDroneLocation::Deployed;
        s.cassettes
            .iter_mut()
            .find(|c| c.drone_id == Some(id))
            .unwrap()
            .drone_id = None;
    }
    s.recalls.push(CarrierRecallBatch {
        batch_id: 1,
        members: [1, 2]
            .into_iter()
            .map(|id| CarrierRecallMember {
                drone_id: id,
                accepted_location: CarrierDroneLocation::Deployed,
                service_operation_id: None,
                capture_operation_id: Some(id),
                store_operation_id: None,
                cancelled: false,
                blocked_reason: String::new(),
                completed_location: None,
            })
            .collect(),
        phase: CarrierRecallPhase::Running,
        held: false,
        accepted_at_ms: 200,
        updated_at_ms: 200,
    });
    s.recall_high_watermark = 1;
    for id in [1, 2] {
        s.operations.push(CarrierInventoryOperation {
            recall_batch_id: None,
            operation_id: id,
            drone_id: id,
            action: CarrierInventoryAction::RecallToStorage,
            phase: CarrierOperationPhase::Queued,
            held_phase: None,
            stage: None,
            berth_id: None,
            accepted_at_ms: 200,
            updated_at_ms: 200,
            message: String::new(),
            cargo_service_operation_ids: vec![],
        });
    }
    s.high_watermark = 2;
    assert!(s.validate_shape(&t));
    let historical = serde_json::to_vec(&s).unwrap();
    assert!(!String::from_utf8_lossy(&historical).contains("\"service_operation_id\":"));
    assert_eq!(
        serde_json::to_vec(&serde_json::from_slice::<CarrierInventoryState>(&historical).unwrap())
            .unwrap(),
        historical
    );
    let mut bad = s.clone();
    bad.recalls[0].members[0].capture_operation_id = None;
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    bad.recalls[0].members[0].capture_operation_id = Some(2);
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    bad.recalls[0].members[0].accepted_location = CarrierDroneLocation::Ready { berth_id: 0 };
    assert!(!bad.validate_shape(&t));
    let mut bad = s.clone();
    let duplicate = bad.recalls[0].members[0].clone();
    bad.recalls[0].members.push(duplicate);
    assert!(!bad.validate_shape(&t));
    // A retained source-incompatible reverse receipt is refused even without a run.
    let (mut t, mut legacy) = fixture();
    t.registration.assembly_revision = 1701;
    legacy.binding.registration = t.registration.clone();
    legacy.operations.push(CarrierInventoryOperation {
        recall_batch_id: None,
        operation_id: 1,
        drone_id: 1,
        action: CarrierInventoryAction::StoreReady { slot_id: 0 },
        phase: CarrierOperationPhase::Cancelled,
        held_phase: None,
        stage: None,
        berth_id: None,
        accepted_at_ms: 200,
        updated_at_ms: 200,
        message: String::new(),
        cargo_service_operation_ids: vec![],
    });
    legacy.high_watermark = 1;
    assert!(!legacy.validate_shape(&t));
}

#[test]
fn cancelled_recall_does_not_seize_same_clock_instant_destroyed_manual_service() {
    // Structural lifetime boundary, not a simulated combat/economic event.
    let (mut t, mut s) = fixture();
    t.registration.assembly_revision = 1703;
    s.binding.registration = t.registration.clone();
    s.entries[0].hit_points = 0;
    s.entries[0].location = CarrierDroneLocation::Destroyed { at_ms: 200 };
    s.cassettes[0].drone_id = None;
    s.operations = [
        CarrierOperationPhase::Cancelled,
        CarrierOperationPhase::Destroyed,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, phase)| CarrierInventoryOperation {
        recall_batch_id: None,
        operation_id: i as u64 + 1,
        drone_id: 1,
        action: CarrierInventoryAction::ReturnToUnload {
            resume_mining: false,
        },
        phase,
        held_phase: None,
        stage: None,
        berth_id: None,
        accepted_at_ms: if i == 0 { 150 } else { 200 },
        updated_at_ms: 200,
        message: String::new(),
        cargo_service_operation_ids: vec![],
    })
    .collect();
    s.high_watermark = 2;
    s.recalls = vec![CarrierRecallBatch {
        batch_id: 1,
        phase: CarrierRecallPhase::Cancelled,
        held: false,
        accepted_at_ms: 150,
        updated_at_ms: 200,
        members: vec![CarrierRecallMember {
            drone_id: 1,
            accepted_location: CarrierDroneLocation::Deployed,
            service_operation_id: Some(1),
            capture_operation_id: None,
            store_operation_id: None,
            cancelled: true,
            blocked_reason: String::new(),
            completed_location: None,
        }],
    }];
    s.recall_high_watermark = 1;
    assert!(
        s.validate_shape(&t),
        "Later instant loss is not old recall provenance"
    );
    let bytes = serde_json::to_vec(&s).unwrap();
    assert!(serde_json::from_slice::<CarrierInventoryState>(&bytes)
        .unwrap()
        .validate_shape(&t));
    // Genuine batch child accepted and destroyed at this exact clock retains
    // typed provenance even though duration cannot disambiguate ownership.
    let mut genuine = s.clone();
    genuine.operations[0].phase = CarrierOperationPhase::Destroyed;
    genuine.operations[0].accepted_at_ms = 200;
    genuine.operations[0].recall_batch_id = Some(1);
    assert!(genuine.validate_shape(&t));
    genuine.recalls[0].members[0].service_operation_id = None;
    assert!(!genuine.validate_shape(&t));
    // A completed positive-duration prefix inside the retained lifetime cannot
    // silently lose its member link, even after the batch becomes terminal.
    s.operations[1].phase = CarrierOperationPhase::Completed;
    s.operations[1].accepted_at_ms = 175;
    assert!(!s.validate_shape(&t));
}
