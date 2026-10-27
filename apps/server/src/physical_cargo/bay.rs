//! Selected-bay inspection executor. The only allowed movement is the certified
//! stationary source's launch/service/capture path. Other host machinery and all
//! other bays remain frozen; no normal transport admits this authority.
use super::*;
use arkline_protocol::{
    carrier_inventory_geometry::{CarrierInventoryKeyframe, PhysicalServiceFrame},
    drone_bay_geometry::BayPose,
    physical_cargo::bay::{
        binding_source_valid, physical_definition, PhysicalBayBinding,
        PhysicalBayCommand as Command, PhysicalBayRoute as Route, PhysicalBayState as State,
        PhysicalBayStep as Step,
    },
    CarrierInventoryAction, DroneClass,
};
use arkline_verifier::physical_cargo::PhysicalReplay;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhysicalBayView {
    pub state: State,
    pub source_frame: CarrierInventoryKeyframe,
    pub drone_inlet_travel_mm: f64,
}

impl PhysicalCargoInspection {
    /// Fresh binding only. A caller cannot attach a physical source to previously
    /// changed accounting balances or a differently sourced world.
    pub fn bind_bay(&mut self, binding: PhysicalBayBinding, now: u64) -> Result<PhysicalBayView> {
        ensure!(
            binding_source_valid(&binding),
            "Uncertified selected-bay source"
        );
        let export = self.export()?;
        ensure!(
            export.control.is_none(),
            "Control-bound world rejects inspection bypass"
        );
        ensure!(
            export.entries.len() == 1 && export.genesis.mining_drone_ids == [binding.drone_id],
            "Bay binding requires untouched singleton inspection genesis"
        );
        ensure!(
            binding.source_configuration_sha256 == export.genesis.source_configuration_sha256,
            "Bay/accounting source mismatch"
        );
        let state = initial(binding.clone(), now);
        apply_authorized(
            &mut self.server.db,
            &[PhysicalCargoEvent::BayBound {
                binding: Box::new(binding),
            }],
            now,
            Some(state),
        )?;
        self.bay_view()
    }
    pub fn bay_command(
        &mut self,
        expected_step_sequence: u64,
        command: Command,
        now: u64,
    ) -> Result<PhysicalBayView> {
        step_db(
            &mut self.server.db,
            Step::Command {
                expected_step_sequence,
                command,
            },
            now,
        )?;
        self.bay_view()
    }
    pub fn bay_tick(&mut self, now: u64) -> Result<PhysicalBayView> {
        let world = read_world(&self.server.db)?;
        let state = world
            .physical_cargo
            .as_ref()
            .and_then(|s| s.bay.as_ref())
            .context("No bound bay")?;
        let elapsed_ms = now
            .checked_sub(state.last_step_ms)
            .context("Bay clock moved backwards")?;
        if elapsed_ms != 0 {
            step_db(&mut self.server.db, Step::Advance { elapsed_ms }, now)?;
        }
        self.bay_view()
    }
    pub fn bay_view(&mut self) -> Result<PhysicalBayView> {
        let export = self.export()?;
        let state = export.bay.context("No bound bay")?;
        view(state)
    }
}
pub(super) fn initial(binding: PhysicalBayBinding, now: u64) -> State {
    State {
        world_power: false,
        energy_units: binding.initial_energy_units,
        binding,
        step_sequence: 0,
        route: None,
        frame_index: 0,
        segment_elapsed_ms: 0,
        deployed: false,
        held: false,
        available: true,
        health: 100,
        mining_claim_id: None,
        claim_high_watermark: 0,
        service_id: None,
        service_high_watermark: 0,
        transfer_sequence: 0,
        request_high_watermark: 0,
        last_step_ms: now,
    }
}
pub(super) fn restart_db(db: &mut Connection, now: u64) -> Result<()> {
    step_db(db, Step::Restart, now)
}
fn step_db(db: &mut Connection, step: Step, now: u64) -> Result<()> {
    let read = db.transaction()?;
    let world = read_world(&read)?;
    let physical = world.physical_cargo.as_ref().context("No physical state")?;
    ensure!(
        physical.control.is_none(),
        "Control-bound world rejects inspection bypass"
    );
    let mut state = physical.bay.clone().context("No bound bay")?;
    let entries = load_entries(&read, true)?;
    let prior = replay(&entries)?;
    export_state(&world, entries)?.verify()?;
    read.commit()?;
    let mut events = vec![PhysicalCargoEvent::BayStep { step: step.clone() }];
    events.extend(advance(&mut state, &step, &prior, now)?);
    apply_authorized(db, &events, now, Some(state))
}
fn checked_inc(n: &mut u64) -> Result<u64> {
    *n = n.checked_add(1).context("Bay counter overflow")?;
    Ok(*n)
}
fn pause(state: &mut State, prior: &PhysicalReplay, events: &mut Vec<PhysicalCargoEvent>) {
    let drone_id = state.binding.drone_id;
    if state.mining_claim_id.take().is_some() {
        events.push(PhysicalCargoEvent::CancelClaim { drone_id });
    }
    if let Some(service) = prior.active_service.as_ref() {
        if !service.held {
            events.push(PhysicalCargoEvent::ServiceHeld {
                service_id: service.service_id,
            });
        }
    }
    state.held = true;
}
fn runnable(state: &State) -> Result<()> {
    ensure!(
        state.health > 0 && state.available && !state.held,
        "Bay is unavailable, held or destroyed"
    );
    Ok(())
}
fn start_route(state: &mut State, route: Route) {
    state.route = Some(route);
    state.frame_index = 0;
    state.segment_elapsed_ms = 0;
}
fn energy_cost(prior: &PhysicalReplay) -> Result<u64> {
    Ok(prior
        .catalog
        .operating_ratings
        .iter()
        .find(|r| r.rating_id == prior.design.rating_id)
        .context("Missing authored energy rating")?
        .drone_batch_energy_units)
}
pub(super) fn advance(
    state: &mut State,
    step: &Step,
    prior: &PhysicalReplay,
    now: u64,
) -> Result<Vec<PhysicalCargoEvent>> {
    ensure!(
        binding_source_valid(&state.binding),
        "Bay source certificate changed"
    );
    ensure!(now >= state.last_step_ms, "Bay clock moved backwards");
    let id = state.binding.drone_id;
    let drone = prior.drones.get(&id).context("Bound drone missing")?;
    let mut events = Vec::new();
    match step {
        Step::Restart => pause(state, prior, &mut events),
        Step::Command {
            expected_step_sequence,
            command,
        } => {
            ensure!(
                *expected_step_sequence == state.step_sequence,
                "Stale bay command"
            );
            match command {
                Command::Hold | Command::Cancel => {
                    ensure!(!state.held && state.health > 0, "Bay already held or lost");
                    pause(state, prior, &mut events);
                }
                Command::Resume => {
                    ensure!(
                        state.held && state.available && state.health > 0,
                        "Bay cannot resume"
                    );
                    state.held = false;
                    if let Some(service) = prior.active_service.as_ref() {
                        ensure!(service.held, "Service hold mismatch");
                        events.push(PhysicalCargoEvent::ServiceResumed {
                            service_id: service.service_id,
                            ready_at_ms: now
                                .checked_add(prior.design.manifest.vessel_manifold.interval_ms)
                                .context("Time overflow")?,
                        });
                    }
                }
                Command::SetAvailable { available } => {
                    ensure!(
                        state.health > 0 && *available != state.available,
                        "Availability unchanged or drone lost"
                    );
                    state.available = *available;
                    if !available {
                        pause(state, prior, &mut events);
                    }
                }
                Command::Damage { amount } => {
                    ensure!(
                        *amount > 0 && state.health > 0,
                        "Invalid or repeated damage"
                    );
                    state.health = state.health.saturating_sub(*amount);
                    if state.health == 0 {
                        state.held = true;
                        state.mining_claim_id = None;
                        state.service_id = None;
                        events.push(PhysicalCargoEvent::DroneLost { drone_id: id });
                    }
                }
                Command::Launch => {
                    runnable(state)?;
                    ensure!(
                        !state.deployed && state.route.is_none() && state.mining_claim_id.is_none(),
                        "Cannot launch this state"
                    );
                    start_route(state, Route::Launch);
                }
                Command::Return | Command::Service => {
                    runnable(state)?;
                    ensure!(
                        state.deployed && state.route.is_none() && state.mining_claim_id.is_none(),
                        "Drone not ready at exterior endpoint"
                    );
                    if matches!(command, Command::Service) {
                        ensure!(drone.payload_cm3.0 > 0, "No payload to service");
                        start_route(state, Route::Service);
                    } else {
                        ensure!(drone.payload_cm3.0 == 0, "Unload before protected return");
                        start_route(state, Route::Return);
                    }
                }
                Command::Mine { volume_cm3 } => {
                    runnable(state)?;
                    ensure!(
                        state.deployed && state.route.is_none() && state.mining_claim_id.is_none(),
                        "Mining requires reached exterior endpoint"
                    );
                    ensure!(
                        state.world_power || state.energy_units >= energy_cost(prior)?,
                        "Insufficient authored energy"
                    );
                    // The immutable certificate checks the exact target, body,
                    // source muzzle, range and unobstructed stationary beam.
                    let claim_id = checked_inc(&mut state.claim_high_watermark)?;
                    state.mining_claim_id = Some(claim_id);
                    events.push(PhysicalCargoEvent::Claim {
                        drone_id: id,
                        claim_id,
                        volume_cm3: *volume_cm3,
                        due_at_ms: now
                            .checked_add(prior.design.manifest.drone_extraction.interval_ms)
                            .context("Time overflow")?,
                    });
                }
            }
        }
        Step::Advance { elapsed_ms } => {
            ensure!(
                *elapsed_ms > 0
                    && *elapsed_ms == now - state.last_step_ms
                    && *elapsed_ms <= state.binding.segment_duration_ms,
                "Bay tick delta invalid; no catch-up"
            );
            runnable(state)?;
            {
                if let Some(claim_id) = state.mining_claim_id {
                    let claim = drone.claim.as_ref().context("Mining claim missing")?;
                    ensure!(claim.claim_id == claim_id, "Claim identity mismatch");
                    if now >= claim.due_at_ms {
                        if !state.world_power {
                            state.energy_units = state
                                .energy_units
                                .checked_sub(energy_cost(prior)?)
                                .context("Insufficient extraction energy")?;
                        }
                        events.push(PhysicalCargoEvent::Extract {
                            drone_id: id,
                            claim_id,
                        });
                        state.mining_claim_id = None;
                    }
                } else if let Some(service_id) = state.service_id {
                    ensure!(
                        state.route == Some(Route::Service)
                            && state.frame_index == 7
                            && state.segment_elapsed_ms == 0,
                        "Transfer outside reached dual-valve barrier"
                    );
                    let service = prior
                        .active_service
                        .as_ref()
                        .context("Missing active service")?;
                    ensure!(
                        service.service_id == service_id && !service.held,
                        "Service identity/hold mismatch"
                    );
                    if now >= service.ready_at_ms {
                        let amount = drone
                            .payload_cm3
                            .0
                            .min(prior.design.manifest.vessel_manifold.volume_cm3.0);
                        ensure!(amount > 0, "Empty transfer");
                        let request_id = checked_inc(&mut state.request_high_watermark)?;
                        events.push(PhysicalCargoEvent::Transfer {
                            drone_id: id,
                            service_id,
                            sequence: state.transfer_sequence,
                            request_id,
                            volume_cm3: VolumeCm3(amount),
                        });
                        checked_inc(&mut state.transfer_sequence)?;
                        if amount == drone.payload_cm3.0 {
                            events.push(PhysicalCargoEvent::ServiceClosed { service_id });
                            state.service_id = None;
                        }
                    }
                } else if let Some(route) = state.route {
                    let frames = route_frames(route)?;
                    let next = state
                        .segment_elapsed_ms
                        .checked_add(*elapsed_ms)
                        .context("Motion overflow")?;
                    state.segment_elapsed_ms = next;
                    if next >= state.binding.segment_duration_ms {
                        state.frame_index += 1;
                        state.segment_elapsed_ms = 0;
                        ensure!(
                            usize::from(state.frame_index) < frames.len(),
                            "Route exceeded source"
                        );
                        if route == Route::Service && state.frame_index == 7 {
                            let reached = &frames[7];
                            ensure!(
                                reached.frame.milestones.ore_coupled
                                    && reached.drone_inlet_travel_mm == 250,
                                "Missing certified dual-open contact"
                            );
                            ensure!(drone.payload_cm3.0 > 0, "No payload at service contact");
                            let service_id = checked_inc(&mut state.service_high_watermark)?;
                            state.service_id = Some(service_id);
                            state.transfer_sequence = 1;
                            events.push(PhysicalCargoEvent::ServiceOpened {
                                drone_id: id,
                                service_id,
                                ready_at_ms: now
                                    .checked_add(prior.design.manifest.vessel_manifold.interval_ms)
                                    .context("Time overflow")?,
                            });
                        } else if usize::from(state.frame_index) == frames.len() - 1 {
                            ensure!(
                                frames.last().unwrap().frame.milestones.hardware_safe,
                                "Terminal hardware not secured"
                            );
                            state.route = None;
                            state.frame_index = 0;
                            state.deployed = route != Route::Return;
                        }
                    }
                }
            }
        }
    }
    checked_inc(&mut state.step_sequence)?;
    state.last_step_ms = now;
    Ok(events)
}
fn route_frames(route: Route) -> Result<Vec<PhysicalServiceFrame>> {
    let definition = physical_definition().context("Physical definition unavailable")?;
    match route {
        Route::Service => definition
            .physical_inspection_service_route(0, DroneClass::Mining, 0)
            .context("Service route absent"),
        Route::Launch | Route::Return => Ok(definition
            .bay_route(
                0,
                DroneClass::Mining,
                0,
                if route == Route::Launch {
                    CarrierInventoryAction::Launch
                } else {
                    CarrierInventoryAction::RecallToStorage
                },
            )
            .context("Physical route absent")?
            .into_iter()
            .map(|frame| PhysicalServiceFrame {
                frame,
                drone_inlet_travel_mm: 0,
            })
            .collect()),
    }
}
fn pose_mix(a: BayPose, b: BayPose, t: f64) -> Result<BayPose> {
    ensure!(
        a.orientation_xyzw == b.orientation_xyzw,
        "Uncertified rotating segment"
    );
    Ok(BayPose {
        position_m: std::array::from_fn(|i| {
            a.position_m[i] + (b.position_m[i] - a.position_m[i]) * t
        }),
        orientation_xyzw: a.orientation_xyzw,
    })
}
pub(super) fn view(state: State) -> Result<PhysicalBayView> {
    ensure!(binding_source_valid(&state.binding), "Uncertified bay view");
    let (route, index) = match state.route {
        Some(route) => (route, usize::from(state.frame_index)),
        None => (
            if state.deployed {
                Route::Launch
            } else {
                Route::Return
            },
            6,
        ),
    };
    let frames = route_frames(route)?;
    let mut frame = frames.get(index).context("Invalid source frame")?.clone();
    if state.segment_elapsed_ms > 0 {
        let next = frames.get(index + 1).context("Invalid source segment")?;
        let u = state.segment_elapsed_ms as f64 / state.binding.segment_duration_ms as f64;
        let t = u * u * (3.0 - 2.0 * u);
        frame.frame.drone_pose = Some(pose_mix(
            frame.frame.drone_pose.context("No drone pose")?,
            next.frame.drone_pose.context("No next drone pose")?,
            t,
        )?);
        for (name, pose) in &mut frame.frame.machinery {
            let other = next
                .frame
                .machinery
                .iter()
                .find(|(n, _)| n == name)
                .context("Missing machinery endpoint")?;
            *pose = pose_mix(*pose, other.1, t)?;
        }
        // No reached-contact or safe-state assertion survives an in-flight segment.
        frame.frame.milestones = Default::default();
        frame.drone_inlet_travel_mm = frames[index].drone_inlet_travel_mm;
        let inlet = f64::from(frames[index].drone_inlet_travel_mm)
            + (f64::from(next.drone_inlet_travel_mm)
                - f64::from(frames[index].drone_inlet_travel_mm))
                * t;
        return Ok(PhysicalBayView {
            state,
            source_frame: frame.frame,
            drone_inlet_travel_mm: inlet,
        });
    }
    Ok(PhysicalBayView {
        state,
        source_frame: frame.frame,
        drone_inlet_travel_mm: f64::from(frame.drone_inlet_travel_mm),
    })
}

#[cfg(test)]
mod tests;
