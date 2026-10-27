//! Adapter decisions over current world actors. Mechanical steps remain in the
//! existing bay executor, economic mutations remain in the existing transaction.
use super::*;
use arkline_protocol::physical_cargo::bay::{PhysicalBayCommand as C, PhysicalBayStep as S};
use arkline_protocol::{lock_duration_ms, ShipClass, TargetLock};
use arkline_verifier::physical_cargo::{PhysicalClaim, PhysicalReplay, PhysicalService};

struct Work {
    bay: arkline_protocol::physical_cargo::bay::PhysicalBayState,
    prior: PhysicalReplay,
    steps: Vec<S>,
    events: Vec<PhysicalCargoEvent>,
    power: Option<arkline_protocol::PowerState>,
    cost: u64,
}
impl Work {
    fn step(&mut self, step: S, now: u64) -> Result<()> {
        let events = bay::advance(&mut self.bay, &step, &self.prior, now)?;
        self.steps.push(step.clone());
        self.events.push(PhysicalCargoEvent::BayStep { step });
        for e in events {
            // This provisional planning view is not persisted. The transaction
            // applies account primitives and independent replay verifies all suffixes.
            let id = self.bay.binding.drone_id;
            match &e {
                PhysicalCargoEvent::Claim {
                    claim_id,
                    volume_cm3,
                    due_at_ms,
                    ..
                } => {
                    self.prior.drones.get_mut(&id).unwrap().claim = Some(PhysicalClaim {
                        claim_id: *claim_id,
                        volume_cm3: *volume_cm3,
                        due_at_ms: *due_at_ms,
                    })
                }
                PhysicalCargoEvent::CancelClaim { .. } => {
                    self.prior.drones.get_mut(&id).unwrap().claim = None
                }
                PhysicalCargoEvent::Extract { .. } => {
                    ensure!(
                        crate::power::spend(&mut self.power, self.cost, now),
                        "Actual Pioneer capacitor cannot fund extraction"
                    );
                    let d = self.prior.drones.get_mut(&id).unwrap();
                    let amount = d
                        .claim
                        .take()
                        .context("Missing real extraction claim")?
                        .volume_cm3;
                    d.payload_cm3 = d
                        .payload_cm3
                        .checked_add(amount)
                        .context("Volume overflow")?;
                    self.prior.resource_remaining_cm3 = self
                        .prior
                        .resource_remaining_cm3
                        .checked_sub(amount)
                        .context("Target exhausted")?;
                }
                PhysicalCargoEvent::ServiceOpened {
                    service_id,
                    ready_at_ms,
                    ..
                } => {
                    self.prior.active_service = Some(PhysicalService {
                        service_id: *service_id,
                        drone_id: id,
                        ready_at_ms: *ready_at_ms,
                        held: false,
                        next_sequence: 1,
                    })
                }
                PhysicalCargoEvent::ServiceHeld { .. } => {
                    self.prior
                        .active_service
                        .as_mut()
                        .context("Missing service")?
                        .held = true
                }
                PhysicalCargoEvent::ServiceResumed { ready_at_ms, .. } => {
                    let s = self
                        .prior
                        .active_service
                        .as_mut()
                        .context("Missing service")?;
                    s.held = false;
                    s.ready_at_ms = *ready_at_ms;
                }
                PhysicalCargoEvent::Transfer { volume_cm3, .. } => {
                    let d = self.prior.drones.get_mut(&id).unwrap();
                    d.payload_cm3 = d
                        .payload_cm3
                        .checked_sub(*volume_cm3)
                        .context("Payload insufficient")?;
                    self.prior.ship_payload_cm3 = self
                        .prior
                        .ship_payload_cm3
                        .checked_add(*volume_cm3)
                        .context("Hold overflow")?;
                    let s = self.prior.active_service.as_mut().context("No contact")?;
                    s.next_sequence += 1;
                    s.ready_at_ms = now
                        .checked_add(self.prior.design.manifest.vessel_manifold.interval_ms)
                        .context("Clock overflow")?;
                }
                PhysicalCargoEvent::ServiceClosed { .. } => self.prior.active_service = None,
                PhysicalCargoEvent::DroneLost { .. } => {
                    let d = self.prior.drones.get_mut(&id).unwrap();
                    d.payload_cm3 = VolumeCm3(0);
                    d.claim = None;
                    d.destroyed = true;
                    self.prior.active_service = None;
                }
                _ => bail!("Unexpected mechanical economic event"),
            }
            self.events.push(e);
        }
        self.prior.bay = Some(self.bay.clone());
        Ok(())
    }
    fn command(&mut self, c: C, now: u64) -> Result<()> {
        self.step(
            S::Command {
                expected_step_sequence: self.bay.step_sequence,
                command: c,
            },
            now,
        )
    }
    fn cancel_claim(&mut self, now: u64) -> Result<()> {
        if self.bay.mining_claim_id.is_some() {
            self.command(C::Hold, now)?;
            self.command(C::Resume, now)?;
        }
        Ok(())
    }
    fn amount(&self) -> u64 {
        let m = &self.prior.design.manifest;
        let incoming = self.prior.drones.values().fold(0u64, |n, d| {
            n.saturating_add(d.payload_cm3.0)
                .saturating_add(d.claim.as_ref().map_or(0, |c| c.volume_cm3.0))
        });
        let d = &self.prior.drones[&self.bay.binding.drone_id];
        m.drone_extraction
            .volume_cm3
            .0
            .min(self.prior.resource_remaining_cm3.0)
            .min(
                m.mining_drone_hold
                    .capacity_cm3()
                    .map_or(0, |c| c.0)
                    .saturating_sub(d.payload_cm3.0),
            )
            .min(
                m.ship_hold
                    .capacity_cm3()
                    .map_or(0, |c| c.0)
                    .saturating_sub(self.prior.ship_payload_cm3.0)
                    .saturating_sub(incoming),
            )
    }
    fn eligible(&self, p: &Projection) -> bool {
        p.hit_points > 0
            && p.signal_available
            && p.target_locked
            && p.resource_remaining_cm3.0 > 0
            && self.power.as_ref().is_some_and(|s| s.charge_j >= self.cost)
    }
    fn idle(&self) -> bool {
        self.bay.deployed
            && self.bay.route.is_none()
            && !self.bay.held
            && self.bay.available
            && self.bay.health > 0
            && self.bay.mining_claim_id.is_none()
    }
    fn automatic(&mut self, p: &Projection, now: u64) -> Result<()> {
        if !self.idle() {
            return Ok(());
        }
        if self.prior.drones[&self.bay.binding.drone_id].payload_cm3.0 > 0 {
            self.command(C::Service, now)?;
        } else if p.recalling {
            self.command(C::Return, now)?;
        } else if p.mining && self.eligible(p) && self.amount() > 0 {
            self.command(
                C::Mine {
                    volume_cm3: VolumeCm3(self.amount()),
                },
                now,
            )?;
        }
        Ok(())
    }
}
pub(super) fn reduce(
    world: &World,
    prior: &PhysicalReplay,
    action: Action,
    now: u64,
) -> Result<(
    State,
    arkline_protocol::physical_cargo::bay::PhysicalBayState,
    Vec<PhysicalCargoEvent>,
)> {
    validate_world(world)?;
    let old = world
        .physical_cargo
        .as_ref()
        .unwrap()
        .control
        .clone()
        .context("No control state")?;
    let mut p = old.projection.clone();
    ensure!(now >= p.accepted_at_ms, "Control clock moved backwards");
    let mut actual_power = world.pilots[&old.binding.pilot].view.power.clone();
    if action == Action::Restart {
        let power = actual_power.as_mut().context("Missing actual power")?;
        power.updated_at_ms = now;
    } else {
        crate::power::advance_state_with_load(
            &mut actual_power,
            ShipClass::Pioneer,
            now,
            old.binding.frozen_load_w,
        )?;
    }
    if action != Action::Restart
        && p.target_lock
            .as_ref()
            .is_some_and(|l| now >= l.completes_at_ms)
    {
        p.target_lock = None;
        p.target_locked = p.resource_remaining_cm3.0 > 0;
    }
    let mut w = Work {
        bay: prior.bay.clone().context("No bay")?,
        prior: prior.clone(),
        steps: vec![],
        events: vec![],
        power: actual_power,
        cost: old.binding.cycle_cost_j,
    };
    match &action {
        Action::Launch => {
            if w.bay.held
                && !w.bay.deployed
                && w.bay.route.is_none()
                && w.bay.available
                && w.bay.health > 0
            {
                w.command(C::Resume, now)?;
            }
            w.command(C::Launch, now)?;
        }
        Action::Lock => {
            ensure!(
                p.resource_remaining_cm3.0 > 0 && !p.target_locked && p.target_lock.is_none(),
                "Target depleted or already locked/acquiring"
            );
            p.target_lock = Some(TargetLock {
                asteroid_id: old.binding.target_id,
                started_at_ms: now,
                completes_at_ms: now
                    .checked_add(lock_duration_ms(
                        ShipClass::Pioneer,
                        f64::from(w.bay.binding.target_radius_mm) / 1000.,
                    ))
                    .context("Clock overflow")?,
            });
        }
        Action::Unlock => {
            p.target_lock = None;
            p.target_locked = false;
            p.mining = false;
            w.cancel_claim(now)?;
        }
        Action::Assign => {
            ensure!(
                w.bay.deployed
                    && w.bay.route.is_none()
                    && w.bay.mining_claim_id.is_none()
                    && w.eligible(&p)
                    && w.amount() > 0,
                "Target/drone/power/volume not eligible"
            );
            if w.bay.held {
                w.command(C::Resume, now)?;
            }
            p.mining = true;
            p.recalling = false;
            w.command(
                C::Mine {
                    volume_cm3: VolumeCm3(w.amount()),
                },
                now,
            )?;
        }
        Action::Hold => {
            p.mining = false;
            p.recalling = false;
            w.command(C::Hold, now)?;
        }
        Action::Resume => w.command(C::Resume, now)?,
        Action::Recall => {
            ensure!(
                w.bay.deployed || w.bay.route.is_some(),
                "Drone already protected"
            );
            p.mining = false;
            p.recalling = true;
            w.cancel_claim(now)?;
            if w.bay.held {
                w.command(C::Resume, now)?;
            }
            w.automatic(&p, now)?;
        }
        Action::Tick => {
            if !w.bay.held && w.bay.health > 0 && w.bay.available {
                if w.bay.mining_claim_id.is_some() && !w.eligible(&p) {
                    w.cancel_claim(now)?;
                } else if now > w.bay.last_step_ms {
                    w.step(
                        S::Advance {
                            elapsed_ms: now - w.bay.last_step_ms,
                        },
                        now,
                    )?;
                }
                p.resource_remaining_cm3 = w.prior.resource_remaining_cm3;
                w.automatic(&p, now)?;
            }
        }
        Action::Restart => {
            p.mining = false;
            p.recalling = false;
            p.target_lock = None;
            p.target_locked = false;
            w.step(S::Restart, now)?;
        }
        Action::Damage { amount } => {
            w.command(C::Damage { amount: *amount }, now)?;
            p.hit_points = w.bay.health;
            if p.hit_points == 0 {
                p.mining = false;
                p.recalling = false;
            }
        }
        Action::Signal { available } => {
            ensure!(*available != p.signal_available, "Signal unchanged");
            p.signal_available = *available;
            if !available {
                p.mining = false;
            }
            w.command(
                C::SetAvailable {
                    available: *available,
                },
                now,
            )?;
        }
    }
    if !w.bay.deployed && w.bay.route.is_none() {
        p.recalling = false;
    }
    p.power = w.power.context("Power vanished")?;
    p.resource_remaining_cm3 = w.prior.resource_remaining_cm3;
    p.accepted_at_ms = now;
    p.mining_cycle = w.prior.drones[&old.binding.drone_id]
        .claim
        .as_ref()
        .map(|c| arkline_protocol::DroneMiningCycle {
            started_at_ms: c.due_at_ms - w.prior.design.manifest.drone_extraction.interval_ms,
            completes_at_ms: c.due_at_ms,
        });
    let step = PhysicalControlStep {
        action,
        before: old.projection,
        after: p.clone(),
        bay_steps: w.steps,
    };
    let mut events = vec![PhysicalCargoEvent::WorldStep {
        step: Box::new(step),
    }];
    events.extend(w.events);
    Ok((
        State {
            binding: old.binding,
            projection: p,
        },
        w.bay,
        events,
    ))
}
