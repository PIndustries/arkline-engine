use super::*;
use crate::physical_cargo::tests::{append, init, rehash};

fn fixture() -> Vec<LedgerEntry> {
    let mut genesis = init();
    if let PhysicalCargoEvent::Initialized {
        mining_drone_ids, ..
    } = &mut genesis
    {
        *mining_drone_ids = vec![1];
    }
    let mut entries = vec![];
    append(&mut entries, 100, genesis);
    let binding =
        certified_inspection_binding(1, 1_000, 10).expect("final selected-bay source certificate");
    append(
        &mut entries,
        100,
        PhysicalCargoEvent::BayBound {
            binding: Box::new(binding),
        },
    );
    entries
}
fn push_step(entries: &mut Vec<LedgerEntry>, now: u64, step: PhysicalBayStep) {
    append(entries, now, PhysicalCargoEvent::BayStep { step });
}
fn command(entries: &mut Vec<LedgerEntry>, now: u64, command: PhysicalBayCommand) {
    let sequence = entries
        .iter()
        .filter(|e| {
            matches!(
                e.event,
                EconomicEvent::PhysicalCargo {
                    record: PhysicalCargoEvent::BayStep { .. }
                }
            )
        })
        .count() as u64;
    push_step(
        entries,
        now,
        PhysicalBayStep::Command {
            expected_step_sequence: sequence,
            command,
        },
    );
}
fn advance(entries: &mut Vec<LedgerEntry>, now: &mut u64, delta: u64) {
    *now += delta;
    push_step(
        entries,
        *now,
        PhysicalBayStep::Advance { elapsed_ms: delta },
    );
}
fn launched() -> (Vec<LedgerEntry>, u64) {
    let mut entries = fixture();
    let mut now = 100;
    command(&mut entries, now, PhysicalBayCommand::Launch);
    for _ in 0..6 {
        advance(&mut entries, &mut now, 1_000);
    }
    (entries, now)
}
fn claim(entries: &mut Vec<LedgerEntry>, now: u64) {
    command(
        entries,
        now,
        PhysicalBayCommand::Mine {
            volume_cm3: VolumeCm3(100_000),
        },
    );
    append(
        entries,
        now,
        PhysicalCargoEvent::Claim {
            drone_id: 1,
            claim_id: 1,
            volume_cm3: VolumeCm3(100_000),
            due_at_ms: now + 10,
        },
    );
}
fn complete_trip() -> Vec<LedgerEntry> {
    let (mut entries, mut now) = launched();
    claim(&mut entries, now);
    advance(&mut entries, &mut now, 10);
    append(
        &mut entries,
        now,
        PhysicalCargoEvent::Extract {
            drone_id: 1,
            claim_id: 1,
        },
    );
    command(&mut entries, now, PhysicalBayCommand::Service);
    for _ in 0..7 {
        advance(&mut entries, &mut now, 1_000);
    }
    append(
        &mut entries,
        now,
        PhysicalCargoEvent::ServiceOpened {
            drone_id: 1,
            service_id: 1,
            ready_at_ms: now + 5,
        },
    );
    advance(&mut entries, &mut now, 5);
    append(
        &mut entries,
        now,
        PhysicalCargoEvent::Transfer {
            drone_id: 1,
            service_id: 1,
            sequence: 1,
            request_id: 1,
            volume_cm3: VolumeCm3(60_000),
        },
    );
    advance(&mut entries, &mut now, 5);
    append(
        &mut entries,
        now,
        PhysicalCargoEvent::Transfer {
            drone_id: 1,
            service_id: 1,
            sequence: 2,
            request_id: 2,
            volume_cm3: VolumeCm3(40_000),
        },
    );
    append(
        &mut entries,
        now,
        PhysicalCargoEvent::ServiceClosed { service_id: 1 },
    );
    for _ in 0..7 {
        advance(&mut entries, &mut now, 1_000);
    }
    command(&mut entries, now, PhysicalBayCommand::Return);
    for _ in 0..6 {
        advance(&mut entries, &mut now, 1_000);
    }
    entries
}
#[test]
fn certified_bay_trip_replays_motion_energy_and_conserved_volume() {
    let entries = complete_trip();
    let state = replay(&entries).unwrap();
    assert_eq!(state.ship_payload_cm3, VolumeCm3(100_000));
    assert_eq!(state.resource_remaining_cm3, VolumeCm3(900_000));
    assert_eq!(state.drones[&1].payload_cm3, VolumeCm3(0));
    let bay = state.bay.unwrap();
    assert!(!bay.deployed && bay.route.is_none() && !bay.held);
    assert_eq!(bay.energy_units, 9);
    assert_eq!(bay.frame_index, 0);
    assert!(bay.service_id.is_none() && bay.mining_claim_id.is_none());
    let copy: Vec<LedgerEntry> =
        serde_json::from_slice(&serde_json::to_vec(&entries).unwrap()).unwrap();
    assert_eq!(replay(&copy).unwrap(), replay(&entries).unwrap());
}
#[test]
fn rehashed_bay_history_rejects_source_time_order_and_accounting_forgery() {
    for case in 0..9 {
        let mut entries = complete_trip();
        match case {
            0 => {
                if let EconomicEvent::PhysicalCargo {
                    record: PhysicalCargoEvent::BayBound { binding },
                } = &mut entries[1].event
                {
                    binding.selected_bay_certificate_sha256 = "0".repeat(64);
                }
            }
            1 => {
                if let EconomicEvent::PhysicalCargo {
                    record: PhysicalCargoEvent::BayBound { binding },
                } = &mut entries[1].event
                {
                    binding.target_center_mm[2] += 1;
                }
            }
            2 => {
                if let EconomicEvent::PhysicalCargo {
                    record:
                        PhysicalCargoEvent::BayStep {
                            step: PhysicalBayStep::Advance { elapsed_ms },
                        },
                } = &mut entries[3].event
                {
                    *elapsed_ms = 999;
                }
            }
            3 => {
                if let EconomicEvent::PhysicalCargo {
                    record:
                        PhysicalCargoEvent::BayStep {
                            step:
                                PhysicalBayStep::Command {
                                    expected_step_sequence,
                                    ..
                                },
                        },
                } = &mut entries[2].event
                {
                    *expected_step_sequence = 1;
                }
            }
            4 => {
                let at = entries
                    .iter()
                    .position(|e| {
                        matches!(
                            e.event,
                            EconomicEvent::PhysicalCargo {
                                record: PhysicalCargoEvent::Extract { .. }
                            }
                        )
                    })
                    .unwrap();
                entries.remove(at);
                for (i, e) in entries.iter_mut().enumerate() {
                    e.sequence = i as u64 + 1;
                }
            }
            5 => {
                let at = entries
                    .iter()
                    .position(|e| {
                        matches!(
                            e.event,
                            EconomicEvent::PhysicalCargo {
                                record: PhysicalCargoEvent::ServiceOpened { .. }
                            }
                        )
                    })
                    .unwrap();
                entries[at].timestamp_ms += 1;
            }
            6 => {
                let at = entries
                    .iter()
                    .position(|e| {
                        matches!(
                            e.event,
                            EconomicEvent::PhysicalCargo {
                                record: PhysicalCargoEvent::Transfer { .. }
                            }
                        )
                    })
                    .unwrap();
                if let EconomicEvent::PhysicalCargo {
                    record: PhysicalCargoEvent::Transfer { volume_cm3, .. },
                } = &mut entries[at].event
                {
                    volume_cm3.0 -= 1;
                }
            }
            7 => {
                let now = entries.last().unwrap().timestamp_ms;
                append(
                    &mut entries,
                    now,
                    PhysicalCargoEvent::Claim {
                        drone_id: 1,
                        claim_id: 2,
                        volume_cm3: VolumeCm3(1),
                        due_at_ms: now + 10,
                    },
                );
            }
            _ => {
                let bound = entries[1].event.clone();
                entries[2].event = bound;
            }
        }
        rehash(&mut entries);
        assert!(replay(&entries).is_err(), "forgery case {case} accepted");
    }
}
#[test]
fn due_tick_loss_and_restart_never_extract_or_recharge() {
    let (mut entries, now) = launched();
    claim(&mut entries, now);
    command(
        &mut entries,
        now + 10,
        PhysicalBayCommand::Damage { amount: 100 },
    );
    append(
        &mut entries,
        now + 10,
        PhysicalCargoEvent::DroneLost { drone_id: 1 },
    );
    let state = replay(&entries).unwrap();
    assert_eq!(state.resource_remaining_cm3, VolumeCm3(1_000_000));
    assert_eq!(state.bay.as_ref().unwrap().energy_units, 10);
    assert_eq!(state.bay.as_ref().unwrap().health, 0);
    push_step(
        &mut entries,
        now + 11,
        PhysicalBayStep::Advance { elapsed_ms: 1 },
    );
    assert!(replay(&entries).is_err());

    let (mut entries, now) = launched();
    claim(&mut entries, now);
    push_step(&mut entries, now + 50_000, PhysicalBayStep::Restart);
    append(
        &mut entries,
        now + 50_000,
        PhysicalCargoEvent::CancelClaim { drone_id: 1 },
    );
    let state = replay(&entries).unwrap();
    assert!(state.bay.as_ref().unwrap().held);
    assert!(state.drones[&1].claim.is_none());
    assert_eq!(state.bay.as_ref().unwrap().energy_units, 10);
    command(&mut entries, now + 50_000, PhysicalBayCommand::Resume);
    assert!(replay(&entries).is_ok());
}
#[test]
fn partial_atomic_suffix_and_mining_in_storage_are_rejected() {
    let (mut entries, now) = launched();
    command(
        &mut entries,
        now,
        PhysicalBayCommand::Mine {
            volume_cm3: VolumeCm3(1),
        },
    );
    assert!(replay(&entries).is_err());
    let mut entries = fixture();
    command(
        &mut entries,
        100,
        PhysicalBayCommand::Mine {
            volume_cm3: VolumeCm3(1),
        },
    );
    append(
        &mut entries,
        100,
        PhysicalCargoEvent::Claim {
            drone_id: 1,
            claim_id: 1,
            volume_cm3: VolumeCm3(1),
            due_at_ms: 110,
        },
    );
    assert!(replay(&entries).is_err());
}

#[test]
fn explicitly_powerless_binding_never_mines_or_recharges() {
    let (mut entries, now) = launched();
    if let EconomicEvent::PhysicalCargo {
        record: PhysicalCargoEvent::BayBound { binding },
    } = &mut entries[1].event
    {
        binding.initial_energy_units = 0;
    }
    rehash(&mut entries);
    let state = replay(&entries).unwrap();
    assert_eq!(state.bay.unwrap().energy_units, 0);
    claim(&mut entries, now);
    assert!(replay(&entries).is_err());
}
