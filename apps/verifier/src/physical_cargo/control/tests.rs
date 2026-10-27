use super::*;
use crate::physical_cargo::tests::{append, init, rehash};
use arkline_protocol::physical_cargo::bay::certified_inspection_binding;

fn fixture(charge: u64) -> Vec<LedgerEntry> {
    let mut event = init();
    if let PhysicalCargoEvent::Initialized {
        mining_drone_ids, ..
    } = &mut event
    {
        *mining_drone_ids = vec![1];
    }
    let mut entries = vec![];
    append(&mut entries, 100, event);
    append(
        &mut entries,
        100,
        PhysicalCargoEvent::BayBound {
            binding: Box::new(certified_inspection_binding(1, 1000, 0).unwrap()),
        },
    );
    let spec = power_spec(ShipClass::Pioneer);
    append(
        &mut entries,
        100,
        PhysicalCargoEvent::WorldBound {
            binding: Box::new(PhysicalControlBinding {
                version: PHYSICAL_CONTROL_VERSION,
                pilot: "captain".into(),
                drone_id: 1,
                target_id: 77,
                system_id: 1,
                initial_power: PowerState {
                    charge_j: charge,
                    capacity_j: spec.capacity_j,
                    regen_w: spec.regen_w,
                    updated_at_ms: 100,
                    regen_remainder_mj: 0,
                },
                cycle_cost_j: spec.drone_mining_cycle_cost_j,
                frozen_load_w: 0,
            }),
        },
    );
    entries
}
fn action(entries: &mut Vec<LedgerEntry>, action: PhysicalControlAction, now: u64) {
    let prior = replay(entries).unwrap();
    let before = prior.control.as_ref().unwrap().projection.clone();
    let (state, _, steps, events) = derive(&prior, &action, now).unwrap();
    append(
        entries,
        now,
        PhysicalCargoEvent::WorldStep {
            step: Box::new(PhysicalControlStep {
                action,
                before,
                after: state.projection,
                bay_steps: steps,
            }),
        },
    );
    for event in events {
        append(entries, now, event);
    }
    replay(entries).unwrap();
}
fn mining() -> (Vec<LedgerEntry>, u64) {
    let mut entries = fixture(power_spec(ShipClass::Pioneer).capacity_j);
    action(&mut entries, PhysicalControlAction::Lock, 100);
    action(&mut entries, PhysicalControlAction::Launch, 100);
    for n in 1..=8 {
        action(&mut entries, PhysicalControlAction::Tick, 100 + n * 1000);
    }
    action(&mut entries, PhysicalControlAction::Assign, 8100);
    (entries, 8100)
}
#[test]
fn real_power_and_volume_are_replayed_without_relabeling_inspection_units() {
    let (mut entries, now) = mining();
    action(&mut entries, PhysicalControlAction::Tick, now + 10);
    let state = replay(&entries).unwrap();
    let control = state.control.as_ref().unwrap();
    let spec = power_spec(ShipClass::Pioneer);
    assert_eq!(
        control.projection.power.charge_j,
        spec.capacity_j - spec.drone_mining_cycle_cost_j
    );
    assert_eq!(state.resource_remaining_cm3, VolumeCm3(900_000));
    assert_eq!(state.drones[&1].payload_cm3, VolumeCm3(100_000));
    assert_eq!(state.bay.as_ref().unwrap().energy_units, 0);
    assert!(state.bay.as_ref().unwrap().world_power);
    assert_eq!(
        state.bay.unwrap().route,
        Some(arkline_protocol::physical_cargo::bay::PhysicalBayRoute::Service)
    );
}
#[test]
fn power_observation_and_mechanical_suffix_tampering_rejects_with_valid_hashes() {
    let (mut original, now) = mining();
    action(&mut original, PhysicalControlAction::Tick, now + 10);
    for mutate in 0..5 {
        let mut entries = original.clone();
        let record = entries
            .iter_mut()
            .rev()
            .find_map(|e| match &mut e.event {
                EconomicEvent::PhysicalCargo {
                    record: PhysicalCargoEvent::WorldStep { step },
                } => Some(step),
                _ => None,
            })
            .unwrap();
        match mutate {
            0 => record.after.power.charge_j += 1,
            1 => record.before.power.charge_j -= 1,
            2 => record.after.resource_remaining_cm3.0 += 1,
            3 => record.bay_steps.clear(),
            _ => {
                record.after.mining_cycle = Some(arkline_protocol::DroneMiningCycle {
                    started_at_ms: 0,
                    completes_at_ms: 1,
                })
            }
        }
        rehash(&mut entries);
        assert!(replay(&entries).is_err(), "mutation {mutate}");
    }
    let mut truncated = original.clone();
    truncated.pop();
    assert!(replay(&truncated).is_err());
}
#[test]
fn world_binding_refuses_unit_policy_changes_and_raw_bay_bypass() {
    for field in 0..4 {
        let mut entries = fixture(1000);
        let EconomicEvent::PhysicalCargo {
            record: PhysicalCargoEvent::WorldBound { binding },
        } = &mut entries[2].event
        else {
            unreachable!()
        };
        match field {
            0 => binding.cycle_cost_j += 1,
            1 => binding.initial_power.capacity_j += 1,
            2 => binding.initial_power.regen_w += 1,
            _ => binding.frozen_load_w = 1,
        }
        rehash(&mut entries);
        assert!(replay(&entries).is_err());
    }
    let mut entries = fixture(1000);
    append(
        &mut entries,
        100,
        PhysicalCargoEvent::BayStep {
            step: Step::Command {
                expected_step_sequence: 0,
                command: Command::Launch,
            },
        },
    );
    assert!(replay(&entries).is_err());
}
#[test]
fn signal_loss_or_death_precedes_due_extraction_and_restart_does_not_catch_up() {
    for action_kind in [
        PhysicalControlAction::Signal { available: false },
        PhysicalControlAction::Damage { amount: 100 },
    ] {
        let (mut entries, now) = mining();
        action(&mut entries, action_kind, now + 10);
        action(&mut entries, PhysicalControlAction::Tick, now + 20);
        let state = replay(&entries).unwrap();
        assert_eq!(state.resource_remaining_cm3, VolumeCm3(1_000_000));
        assert_eq!(
            state.control.as_ref().unwrap().projection.power.charge_j,
            power_spec(ShipClass::Pioneer).capacity_j
        );
        assert!(state.drones[&1].claim.is_none());
    }
    let (mut entries, now) = mining();
    let before = replay(&entries)
        .unwrap()
        .control
        .unwrap()
        .projection
        .power
        .charge_j;
    action(&mut entries, PhysicalControlAction::Restart, now + 100_000);
    let state = replay(&entries).unwrap();
    assert_eq!(state.control.unwrap().projection.power.charge_j, before);
    assert!(state.bay.unwrap().held);
    assert!(state.drones[&1].claim.is_none());
}

#[test]
fn loaded_recall_finishes_service_before_source_return_and_never_duplicates_volume() {
    let (mut entries, mut now) = mining();
    now += 10;
    action(&mut entries, PhysicalControlAction::Tick, now);
    action(&mut entries, PhysicalControlAction::Recall, now);
    for _ in 0..300 {
        let state = replay(&entries).unwrap();
        let bay = state.bay.as_ref().unwrap();
        if !bay.deployed && bay.route.is_none() {
            break;
        }
        now += 100;
        action(&mut entries, PhysicalControlAction::Tick, now);
    }
    let state = replay(&entries).unwrap();
    let bay = state.bay.as_ref().unwrap();
    assert!(!bay.deployed && bay.route.is_none());
    assert_eq!(state.ship_payload_cm3, VolumeCm3(100_000));
    assert_eq!(state.resource_remaining_cm3, VolumeCm3(900_000));
    assert_eq!(state.drones[&1].payload_cm3, VolumeCm3(0));
    assert!(!state.control.unwrap().projection.recalling);
    let mut duplicate = entries.clone();
    duplicate.push(entries.last().unwrap().clone());
    rehash(&mut duplicate);
    assert!(replay(&duplicate).is_err());
}

#[test]
fn zero_power_predicate_rejects_assignment_without_manufacturing_a_valid_outage_history() {
    // Predicate-only state injection: this is not a valid saved-world fixture or
    // evidence that this frozen-host loop naturally exhausts its capacitor.
    let (entries, now) = mining();
    let mut state = replay(&entries[..entries.len() - 3]).unwrap();
    state.control.as_mut().unwrap().projection.power.charge_j = 0;
    state
        .control
        .as_mut()
        .unwrap()
        .projection
        .power
        .updated_at_ms = now;
    assert!(derive(&state, &PhysicalControlAction::Assign, now).is_err());
}

#[test]
fn stored_restart_launch_resumes_only_safe_protected_actor() {
    let mut entries = fixture(1000);
    action(&mut entries, PhysicalControlAction::Restart, 200);
    assert!(replay(&entries).unwrap().bay.unwrap().held);
    action(&mut entries, PhysicalControlAction::Launch, 200);
    let state = replay(&entries).unwrap();
    let bay = state.bay.unwrap();
    assert!(!bay.held);
    assert_eq!(
        bay.route,
        Some(arkline_protocol::physical_cargo::bay::PhysicalBayRoute::Launch)
    );
    let steps = entries
        .iter()
        .find_map(|entry| match &entry.event {
            EconomicEvent::PhysicalCargo {
                record: PhysicalCargoEvent::WorldStep { step },
            } if step.action == PhysicalControlAction::Launch => Some(&step.bay_steps),
            _ => None,
        })
        .unwrap();
    assert!(matches!(
        steps.as_slice(),
        [
            Step::Command {
                command: Command::Resume,
                ..
            },
            Step::Command {
                command: Command::Launch,
                ..
            }
        ]
    ));
    for fault in [
        PhysicalControlAction::Signal { available: false },
        PhysicalControlAction::Damage { amount: 100 },
    ] {
        let mut entries = fixture(1000);
        action(&mut entries, PhysicalControlAction::Restart, 200);
        action(&mut entries, fault, 200);
        assert!(derive(
            &replay(&entries).unwrap(),
            &PhysicalControlAction::Launch,
            200
        )
        .is_err());
    }
}

#[test]
fn world_target_budget_rejects_one_cm3_over_without_restricting_generic_ledger() {
    let mut entries = fixture(1000);
    assert!(replay(&entries).is_ok()); // Exactly 1,000,000 cm³ fits.
    let EconomicEvent::PhysicalCargo {
        record: PhysicalCargoEvent::Initialized { resource_cm3, .. },
    } = &mut entries[0].event
    else {
        unreachable!()
    };
    *resource_cm3 = VolumeCm3(1_000_001);
    rehash(&mut entries);
    assert!(replay(&entries[..2]).is_ok()); // Old inspection/accounting stays unchanged.
    let error = replay(&entries).unwrap_err();
    assert!(format!("{error:#}").contains("target geometry budget"));
}
