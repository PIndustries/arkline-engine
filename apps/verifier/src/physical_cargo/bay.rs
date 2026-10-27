//! Independent single-bay inspection replay. This module reconstructs the
//! accepted route cursor and economic barriers; it never calls server reducers.
use super::*;
use arkline_protocol::{physical_cargo::bay::*, CarrierInventoryAction, DroneClass};

pub(super) fn bind(
    replay: &PhysicalReplay,
    binding: &PhysicalBayBinding,
    now: u64,
) -> Result<PhysicalBayState> {
    ensure!(
        binding.version == PHYSICAL_BAY_VERSION
            && binding.berth_id == 0
            && binding.cassette_id == 0,
        "unsupported physical bay binding"
    );
    ensure!(
        binding_source_valid(binding),
        "uncertified physical bay source/target"
    );
    ensure!(
        binding.source_configuration_sha256 == replay.source_configuration_sha256,
        "bay source differs from cargo source"
    );
    ensure!(
        replay.head_sequence == 1
            && replay.bay.is_none()
            && replay.drones.len() == 1
            && replay.drones.contains_key(&binding.drone_id),
        "bay binding requires untouched singleton genesis"
    );
    ensure!(
        replay.ship_payload_cm3.0 == 0
            && replay.transfers.is_empty()
            && replay.used_claim_ids.is_empty()
            && replay.active_service.is_none()
            && replay.resource_remaining_cm3 == replay.initial_resource_cm3,
        "late physical bay binding"
    );
    ensure!(
        binding.segment_duration_ms > 0,
        "invalid authored bay timing"
    );
    Ok(PhysicalBayState {
        binding: binding.clone(),
        route: None,
        frame_index: 0,
        segment_elapsed_ms: 0,
        deployed: false,
        held: false,
        available: true,
        health: 100,
        energy_units: binding.initial_energy_units,
        world_power: false,
        mining_claim_id: None,
        claim_high_watermark: 0,
        service_id: None,
        service_high_watermark: 0,
        transfer_sequence: 0,
        request_high_watermark: 0,
        last_step_ms: now,
        step_sequence: 0,
    })
}

fn route_shape(route: PhysicalBayRoute) -> Result<(usize, Option<usize>)> {
    let definition = physical_definition().context("physical bay definition unavailable")?;
    match route {
        PhysicalBayRoute::Launch | PhysicalBayRoute::Return => {
            let action = if route == PhysicalBayRoute::Launch {
                CarrierInventoryAction::Launch
            } else {
                CarrierInventoryAction::RecallToStorage
            };
            let frames = definition
                .bay_route(0, DroneClass::Mining, 0, action)
                .context("physical source route absent")?;
            ensure!(frames.len() >= 2, "empty physical route");
            Ok((frames.len(), None))
        }
        PhysicalBayRoute::Service => {
            let frames = definition
                .physical_inspection_service_route(0, DroneClass::Mining, 0)
                .context("physical service route absent")?;
            let barriers: Vec<_> = frames
                .iter()
                .enumerate()
                .filter(|(_, f)| f.frame.milestones.ore_coupled)
                .map(|(i, _)| i)
                .collect();
            ensure!(barriers.len() == 1, "ambiguous physical coupled barrier");
            let barrier = barriers[0];
            ensure!(
                barrier > 0
                    && barrier + 1 < frames.len()
                    && frames[barrier].drone_inlet_travel_mm == 250,
                "invalid dual-open source barrier"
            );
            Ok((frames.len(), Some(barrier)))
        }
    }
}

fn cancel_claim(state: &mut PhysicalBayState, expected: &mut Vec<PhysicalCargoEvent>) {
    if state.mining_claim_id.take().is_some() {
        expected.push(PhysicalCargoEvent::CancelClaim {
            drone_id: state.binding.drone_id,
        });
    }
}
fn hold(
    state: &mut PhysicalBayState,
    replay: &PhysicalReplay,
    expected: &mut Vec<PhysicalCargoEvent>,
) {
    state.held = true;
    cancel_claim(state, expected);
    if let Some(service) = &replay.active_service {
        if !service.held {
            expected.push(PhysicalCargoEvent::ServiceHeld {
                service_id: service.service_id,
            });
        }
    }
}
fn healthy(state: &PhysicalBayState) -> Result<()> {
    ensure!(
        state.health > 0 && state.available && !state.held,
        "physical bay unhealthy, unavailable or held"
    );
    Ok(())
}

/// Produce the EXACT required economic suffix for one accepted step. The outer
/// replay requires every suffix event at this same timestamp before another step
/// and rejects truncated batches or any unsolicited accounting event.
pub(super) fn step(
    replay: &PhysicalReplay,
    old: &PhysicalBayState,
    step: &PhysicalBayStep,
    now: u64,
) -> Result<(PhysicalBayState, Vec<PhysicalCargoEvent>)> {
    let mut state = old.clone();
    ensure!(
        binding_source_valid(&state.binding),
        "bay binding no longer certified"
    );
    ensure!(now >= state.last_step_ms, "bay clock moved backwards");
    let id = state.binding.drone_id;
    let drone = replay.drones.get(&id).context("bound drone missing")?;
    ensure!(
        (state.health == 0) == drone.destroyed,
        "bay health/account loss differs"
    );
    ensure!(
        state.mining_claim_id == drone.claim.as_ref().map(|c| c.claim_id),
        "bay claim/account differs"
    );
    ensure!(
        state.service_id == replay.active_service.as_ref().map(|s| s.service_id),
        "bay service/account differs"
    );
    let rate = replay
        .catalog
        .operating_ratings
        .iter()
        .find(|r| r.rating_id == replay.design.rating_id)
        .context("bay energy rating absent")?;
    let mut expected = vec![];
    match step {
        PhysicalBayStep::Command {
            expected_step_sequence,
            command,
        } => {
            ensure!(
                *expected_step_sequence == state.step_sequence,
                "stale bay command"
            );
            match command {
                PhysicalBayCommand::Launch => {
                    healthy(&state)?;
                    ensure!(
                        !state.deployed
                            && state.route.is_none()
                            && state.mining_claim_id.is_none()
                            && state.service_id.is_none(),
                        "cannot launch physical bay"
                    );
                    route_shape(PhysicalBayRoute::Launch)?;
                    state.route = Some(PhysicalBayRoute::Launch);
                    state.frame_index = 0;
                    state.segment_elapsed_ms = 0;
                }
                PhysicalBayCommand::Mine { volume_cm3 } => {
                    healthy(&state)?;
                    ensure!(
                        state.deployed
                            && state.route.is_none()
                            && state.mining_claim_id.is_none()
                            && state.service_id.is_none(),
                        "mining without exterior-clear source pose"
                    );
                    ensure!(
                        state.world_power || state.energy_units >= rate.drone_batch_energy_units,
                        "insufficient physical mining energy"
                    );
                    state.claim_high_watermark = add(state.claim_high_watermark, 1)?;
                    state.mining_claim_id = Some(state.claim_high_watermark);
                    expected.push(PhysicalCargoEvent::Claim {
                        drone_id: id,
                        claim_id: state.claim_high_watermark,
                        volume_cm3: *volume_cm3,
                        due_at_ms: add(now, replay.design.manifest.drone_extraction.interval_ms)?,
                    });
                }
                PhysicalBayCommand::Service | PhysicalBayCommand::Return => {
                    healthy(&state)?;
                    ensure!(
                        state.deployed
                            && state.route.is_none()
                            && state.mining_claim_id.is_none()
                            && state.service_id.is_none(),
                        "cannot begin physical approach"
                    );
                    let route = if matches!(command, PhysicalBayCommand::Service) {
                        ensure!(drone.payload_cm3.0 > 0, "service without payload");
                        PhysicalBayRoute::Service
                    } else {
                        ensure!(
                            drone.payload_cm3.0 == 0,
                            "loaded drone must unload before protected storage"
                        );
                        PhysicalBayRoute::Return
                    };
                    route_shape(route)?;
                    state.route = Some(route);
                    state.frame_index = 0;
                    state.segment_elapsed_ms = 0;
                }
                PhysicalBayCommand::Hold | PhysicalBayCommand::Cancel => {
                    ensure!(
                        state.health > 0 && !state.held,
                        "physical bay already held or destroyed"
                    );
                    hold(&mut state, replay, &mut expected);
                }
                PhysicalBayCommand::Resume => {
                    ensure!(
                        state.health > 0 && state.held && state.available,
                        "physical bay cannot resume"
                    );
                    state.held = false;
                    if let Some(service) = &replay.active_service {
                        ensure!(service.held, "physical service not held");
                        expected.push(PhysicalCargoEvent::ServiceResumed {
                            service_id: service.service_id,
                            ready_at_ms: add(
                                now,
                                replay.design.manifest.vessel_manifold.interval_ms,
                            )?,
                        });
                    }
                }
                PhysicalBayCommand::SetAvailable { available } => {
                    ensure!(
                        state.health > 0 && state.available != *available,
                        "availability unchanged or destroyed"
                    );
                    state.available = *available;
                    if !available {
                        hold(&mut state, replay, &mut expected);
                    }
                }
                PhysicalBayCommand::Damage { amount } => {
                    ensure!(
                        *amount > 0 && state.health > 0,
                        "invalid or repeated drone damage"
                    );
                    state.health = state.health.saturating_sub(*amount);
                    if state.health == 0 {
                        // Loss itself cancels claims and closes the service exactly once.
                        state.held = true;
                        state.mining_claim_id = None;
                        state.service_id = None;
                        expected.push(PhysicalCargoEvent::DroneLost { drone_id: id });
                    }
                }
            }
        }
        PhysicalBayStep::Restart => {
            if state.health > 0 {
                hold(&mut state, replay, &mut expected);
            } else {
                state.held = true;
            }
        }
        PhysicalBayStep::Advance { elapsed_ms } => {
            ensure!(
                *elapsed_ms > 0
                    && *elapsed_ms == sub(now, state.last_step_ms)?
                    && *elapsed_ms <= state.binding.segment_duration_ms,
                "invalid physical elapsed time or offline catchup"
            );
            healthy(&state)?;
            if let Some(route) = state.route {
                let (count, barrier) = route_shape(route)?;
                ensure!(
                    usize::from(state.frame_index) < count,
                    "physical frame outside source route"
                );
                if barrier == Some(usize::from(state.frame_index)) && state.service_id.is_some() {
                    let service = replay
                        .active_service
                        .as_ref()
                        .context("coupled service missing")?;
                    ensure!(!service.held, "held physical service cannot advance");
                    if now >= service.ready_at_ms {
                        let amount = drone
                            .payload_cm3
                            .0
                            .min(replay.design.manifest.vessel_manifold.volume_cm3.0);
                        ensure!(amount > 0, "empty physical service remains open");
                        state.request_high_watermark = add(state.request_high_watermark, 1)?;
                        expected.push(PhysicalCargoEvent::Transfer {
                            drone_id: id,
                            service_id: service.service_id,
                            sequence: service.next_sequence,
                            request_id: state.request_high_watermark,
                            volume_cm3: VolumeCm3(amount),
                        });
                        state.transfer_sequence = add(service.next_sequence, 1)?;
                        if amount == drone.payload_cm3.0 {
                            expected.push(PhysicalCargoEvent::ServiceClosed {
                                service_id: service.service_id,
                            });
                            state.service_id = None;
                        }
                    }
                } else {
                    state.segment_elapsed_ms = add(state.segment_elapsed_ms, *elapsed_ms)?;
                    if state.segment_elapsed_ms >= state.binding.segment_duration_ms {
                        state.frame_index = state
                            .frame_index
                            .checked_add(1)
                            .context("physical frame overflow")?;
                        state.segment_elapsed_ms = 0; // no residual catch-up into next segment
                        ensure!(
                            usize::from(state.frame_index) < count,
                            "physical route overrun"
                        );
                        if barrier == Some(usize::from(state.frame_index)) {
                            state.service_high_watermark = add(state.service_high_watermark, 1)?;
                            state.service_id = Some(state.service_high_watermark);
                            state.transfer_sequence = 1;
                            expected.push(PhysicalCargoEvent::ServiceOpened {
                                drone_id: id,
                                service_id: state.service_high_watermark,
                                ready_at_ms: add(
                                    now,
                                    replay.design.manifest.vessel_manifold.interval_ms,
                                )?,
                            });
                        }
                        if usize::from(state.frame_index) + 1 == count {
                            state.deployed = route != PhysicalBayRoute::Return;
                            state.route = None;
                            state.frame_index = 0;
                        }
                    }
                }
            } else if let Some(claim) = &drone.claim {
                ensure!(state.deployed, "stored drone has mining claim");
                if now >= claim.due_at_ms {
                    if !state.world_power {
                        state.energy_units =
                            sub(state.energy_units, rate.drone_batch_energy_units)?;
                    }
                    state.mining_claim_id = None;
                    expected.push(PhysicalCargoEvent::Extract {
                        drone_id: id,
                        claim_id: claim.claim_id,
                    });
                }
            }
        }
    }
    state.last_step_ms = now;
    state.step_sequence = add(state.step_sequence, 1)?;
    Ok((state, expected))
}

#[cfg(test)]
mod tests;
