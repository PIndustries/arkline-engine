//! Independent adapter replay. Uses the already independent mechanical reducer,
//! not server commands, capacitor code, world projections or accounting reducers.
use super::*;
use arkline_protocol::{
    lock_duration_ms,
    physical_cargo::bay::{PhysicalBayCommand as Command, PhysicalBayStep as Step},
    physical_cargo::control::*,
    power_spec, PowerState, ShipClass, TargetLock,
};

pub(super) fn bind(
    replay: &PhysicalReplay,
    binding: &PhysicalControlBinding,
    now: u64,
) -> Result<PhysicalControlState> {
    let bay = replay
        .bay
        .as_ref()
        .context("control requires source-bound bay")?;
    let spec = power_spec(ShipClass::Pioneer);
    ensure!(
        replay.head_sequence == 2
            && replay.control.is_none()
            && bay.step_sequence == 0
            && !bay.world_power
            && !bay.deployed
            && !bay.held
            && bay.health == 100
            && replay.drones.len() == 1
            && binding.drone_id == bay.binding.drone_id
            && binding.version == PHYSICAL_CONTROL_VERSION
            && binding.target_id > 0
            && binding.system_id > 0
            && !binding.pilot.is_empty()
            && binding.pilot.len() <= 64
            && binding
                .pilot
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-')),
        "invalid or late physical world binding"
    );
    // The cube with side equal to the target radius fits within the modeled
    // sphere. This bounds authored extractable volume, not density or yield.
    let radius_mm = u128::from(bay.binding.target_radius_mm);
    let target_budget_cm3 = radius_mm.pow(3) / 1000;
    ensure!(
        u128::from(replay.initial_resource_cm3.0) <= target_budget_cm3,
        "physical world resource exceeds conservative target geometry budget"
    );
    let power = &binding.initial_power;
    ensure!(
        power.capacity_j == spec.capacity_j
            && power.regen_w == spec.regen_w
            && power.charge_j <= power.capacity_j
            && power.regen_remainder_mj < 1000
            && power.updated_at_ms == now
            && binding.cycle_cost_j == spec.drone_mining_cycle_cost_j
            && binding.frozen_load_w == 0,
        "physical world power policy differs from explicit Pioneer model"
    );
    Ok(PhysicalControlState {
        binding: binding.clone(),
        projection: PhysicalControlProjection {
            power: power.clone(),
            hit_points: 100,
            signal_available: true,
            target_lock: None,
            target_locked: false,
            mining: false,
            mining_cycle: None,
            recalling: false,
            resource_remaining_cm3: replay.resource_remaining_cm3,
            accepted_at_ms: now,
        },
    })
}

fn regenerate(power: &mut PowerState, now: u64, load: u64) -> Result<()> {
    ensure!(
        load <= power.regen_w
            && power.charge_j <= power.capacity_j
            && power.regen_remainder_mj < 1000,
        "invalid real capacitor state"
    );
    if power.charge_j == power.capacity_j || now <= power.updated_at_ms {
        return Ok(());
    }
    let elapsed = now - power.updated_at_ms;
    let millijoules = u128::from(elapsed) * u128::from(power.regen_w - load)
        + u128::from(power.regen_remainder_mj);
    power.charge_j =
        (u128::from(power.charge_j) + millijoules / 1000).min(u128::from(power.capacity_j)) as u64;
    power.regen_remainder_mj = if power.charge_j == power.capacity_j {
        0
    } else {
        (millijoules % 1000) as u64
    };
    power.updated_at_ms = now;
    Ok(())
}
fn debit(power: &mut PowerState, amount: u64, now: u64) -> Result<()> {
    ensure!(
        power.charge_j >= amount,
        "physical extraction lacks actual capacitor joules"
    );
    if power.charge_j == power.capacity_j {
        power.updated_at_ms = now;
        power.regen_remainder_mj = 0;
    }
    power.charge_j -= amount;
    Ok(())
}

struct Adapter {
    replay: PhysicalReplay,
    state: PhysicalControlState,
    steps: Vec<Step>,
    events: Vec<PhysicalCargoEvent>,
    now: u64,
}
impl Adapter {
    fn bay(&self) -> &PhysicalBayState {
        self.replay.bay.as_ref().expect("checked bay")
    }
    fn command(&mut self, command: Command) -> Result<()> {
        self.mechanical(Step::Command {
            expected_step_sequence: self.bay().step_sequence,
            command,
        })
    }
    fn eligible(&self) -> bool {
        let p = &self.state.projection;
        let b = self.bay();
        p.hit_points > 0
            && p.signal_available
            && p.target_locked
            && p.power.charge_j >= self.state.binding.cycle_cost_j
            && b.deployed
            && b.route.is_none()
            && !b.held
            && b.available
    }
    fn mechanical(&mut self, step: Step) -> Result<()> {
        let (next, suffix) = bay::step(&self.replay, self.bay(), &step, self.now)?;
        for event in &suffix {
            if matches!(event, PhysicalCargoEvent::Extract { .. }) {
                ensure!(self.eligible(), "unguarded real-world extraction");
                debit(
                    &mut self.state.projection.power,
                    self.state.binding.cycle_cost_j,
                    self.now,
                )?;
            }
        }
        self.replay.bay = Some(next);
        self.steps.push(step.clone());
        self.events.push(PhysicalCargoEvent::BayStep { step });
        for event in suffix {
            self.replay.apply(&event, self.now)?;
            self.events.push(event);
        }
        self.state.projection.resource_remaining_cm3 = self.replay.resource_remaining_cm3;
        Ok(())
    }
    fn cancel_claim(&mut self) -> Result<()> {
        if self.bay().mining_claim_id.is_some() {
            if !self.bay().held {
                self.command(Command::Hold)?;
            }
            if self.bay().available && self.bay().health > 0 {
                self.command(Command::Resume)?;
            }
        }
        Ok(())
    }
    fn mining_amount(&self) -> Result<u64> {
        let m = &self.replay.design.manifest;
        let id = self.state.binding.drone_id;
        let drone = &self.replay.drones[&id];
        let capacity = m
            .mining_drone_hold
            .capacity_cm3()
            .context("drone capacity")?
            .0;
        let ship_capacity = m.ship_hold.capacity_cm3().context("ship capacity")?.0;
        let room = sub(
            sub(ship_capacity, self.replay.ship_payload_cm3.0)?,
            self.replay.incoming()?,
        )?;
        Ok(m.drone_extraction
            .volume_cm3
            .0
            .min(self.replay.resource_remaining_cm3.0)
            .min(sub(capacity, drone.payload_cm3.0)?)
            .min(room))
    }
    fn begin_mining(&mut self, required: bool) -> Result<()> {
        if !self.eligible() || self.bay().mining_claim_id.is_some() {
            ensure!(
                !required,
                "mining target, health, signal, source pose or power invalid"
            );
            return Ok(());
        }
        let amount = self.mining_amount()?;
        ensure!(
            !required || amount > 0,
            "no physical extraction capacity/resource"
        );
        if amount > 0 {
            self.command(Command::Mine {
                volume_cm3: VolumeCm3(amount),
            })?;
        }
        Ok(())
    }
    fn schedule(&mut self) -> Result<()> {
        let b = self.bay();
        if !b.deployed
            || b.route.is_some()
            || b.mining_claim_id.is_some()
            || b.held
            || !b.available
            || b.health == 0
        {
            return Ok(());
        }
        let payload = self.replay.drones[&self.state.binding.drone_id]
            .payload_cm3
            .0;
        if payload > 0 {
            self.command(Command::Service)?;
        } else if self.state.projection.recalling {
            self.command(Command::Return)?;
        } else if self.state.projection.mining {
            self.begin_mining(false)?;
        }
        Ok(())
    }
}

fn derive(
    replay: &PhysicalReplay,
    action: &PhysicalControlAction,
    now: u64,
) -> Result<(
    PhysicalControlState,
    PhysicalBayState,
    Vec<Step>,
    Vec<PhysicalCargoEvent>,
)> {
    let old = replay
        .control
        .as_ref()
        .context("world step before control binding")?;
    let b = replay.bay.as_ref().context("world step without bay")?;
    ensure!(
        b.world_power
            && b.health == old.projection.hit_points
            && old.projection.resource_remaining_cm3 == replay.resource_remaining_cm3,
        "world/bay/resource state mismatch"
    );
    ensure!(
        now >= old.projection.accepted_at_ms,
        "physical world clock moved backwards"
    );
    let mut a = Adapter {
        replay: replay.clone(),
        state: old.clone(),
        steps: vec![],
        events: vec![],
        now,
    };
    if *action == PhysicalControlAction::Restart {
        a.state.projection.power.updated_at_ms = now;
    } else {
        regenerate(
            &mut a.state.projection.power,
            now,
            a.state.binding.frozen_load_w,
        )?;
        if a.state
            .projection
            .target_lock
            .as_ref()
            .is_some_and(|l| now >= l.completes_at_ms)
        {
            a.state.projection.target_lock = None;
            a.state.projection.target_locked = a.replay.resource_remaining_cm3.0 > 0;
        }
    }
    use PhysicalControlAction as Action;
    match *action {
        Action::Launch => {
            let b = a.bay();
            if b.held && !b.deployed && b.route.is_none() && b.available && b.health > 0 {
                a.command(Command::Resume)?;
            }
            a.command(Command::Launch)?;
        }
        Action::Lock => {
            ensure!(
                a.replay.resource_remaining_cm3.0 > 0,
                "empty physical target"
            );
            ensure!(
                !a.state.projection.target_locked && a.state.projection.target_lock.is_none(),
                "target already locked/acquiring"
            );
            let radius = f64::from(a.bay().binding.target_radius_mm) / 1000.;
            a.state.projection.target_lock = Some(TargetLock {
                asteroid_id: a.state.binding.target_id,
                started_at_ms: now,
                completes_at_ms: add(now, lock_duration_ms(ShipClass::Pioneer, radius))?,
            });
        }
        Action::Unlock => {
            a.state.projection.target_lock = None;
            a.state.projection.target_locked = false;
            a.state.projection.mining = false;
            a.cancel_claim()?;
        }
        Action::Assign => {
            ensure!(
                a.state.projection.target_locked
                    && a.bay().deployed
                    && a.bay().route.is_none()
                    && a.bay().mining_claim_id.is_none(),
                "assign requires locked idle deployed actor"
            );
            if a.bay().held {
                a.command(Command::Resume)?;
            }
            a.state.projection.mining = true;
            a.state.projection.recalling = false;
            a.begin_mining(true)?;
        }
        Action::Hold => {
            a.state.projection.mining = false;
            a.state.projection.recalling = false;
            a.command(Command::Hold)?;
        }
        Action::Resume => {
            a.command(Command::Resume)?;
        }
        Action::Recall => {
            ensure!(
                a.bay().deployed || a.bay().route.is_some(),
                "drone already protected"
            );
            a.state.projection.mining = false;
            a.state.projection.recalling = true;
            a.cancel_claim()?;
            if a.bay().held {
                a.command(Command::Resume)?;
            }
            a.schedule()?;
        }
        Action::Tick => {
            ensure!(
                now > old.projection.accepted_at_ms,
                "duplicate or zero elapsed world tick"
            );
            if a.bay().health > 0 && a.bay().available && !a.bay().held {
                let cancel_due = a.bay().mining_claim_id.is_some() && !a.eligible();
                if cancel_due {
                    a.cancel_claim()?;
                } else {
                    let elapsed_ms = sub(now, a.bay().last_step_ms)?;
                    if elapsed_ms > 0 {
                        a.mechanical(Step::Advance { elapsed_ms })?;
                    }
                }
                a.schedule()?;
            }
        }
        Action::Restart => {
            a.state.projection.mining = false;
            a.state.projection.recalling = false;
            a.state.projection.target_lock = None;
            a.state.projection.target_locked = false;
            a.mechanical(Step::Restart)?;
        }
        Action::Damage { amount } => {
            a.command(Command::Damage { amount })?;
            a.state.projection.hit_points = a.bay().health;
            if a.bay().health == 0 {
                a.state.projection.mining = false;
                a.state.projection.recalling = false;
            }
        }
        Action::Signal { available } => {
            ensure!(
                available != a.state.projection.signal_available,
                "signal unchanged"
            );
            a.command(Command::SetAvailable { available })?;
            a.state.projection.signal_available = available;
            if !available {
                a.state.projection.mining = false;
            }
        }
    }
    if !a.bay().deployed && a.bay().route.is_none() {
        a.state.projection.recalling = false;
    }
    a.state.projection.mining_cycle = a.replay.drones[&a.state.binding.drone_id]
        .claim
        .as_ref()
        .map(|claim| arkline_protocol::DroneMiningCycle {
            started_at_ms: claim.due_at_ms - a.replay.design.manifest.drone_extraction.interval_ms,
            completes_at_ms: claim.due_at_ms,
        });
    a.state.projection.accepted_at_ms = now;
    Ok((
        a.state,
        a.replay.bay.expect("checked bay"),
        a.steps,
        a.events,
    ))
}

pub(super) fn step(
    replay: &PhysicalReplay,
    supplied: &PhysicalControlStep,
    now: u64,
) -> Result<(
    PhysicalControlState,
    PhysicalBayState,
    Vec<PhysicalCargoEvent>,
)> {
    ensure!(
        replay
            .control
            .as_ref()
            .is_some_and(|c| c.projection == supplied.before),
        "stale or forged physical world observation"
    );
    ensure!(
        supplied.bay_steps.len() <= 8,
        "unbounded world mechanical batch"
    );
    let (state, bay, steps, events) = derive(replay, &supplied.action, now)?;
    ensure!(
        steps == supplied.bay_steps,
        "world action mechanical step list altered"
    );
    ensure!(
        state.projection == supplied.after,
        "world power/target/actor result altered"
    );
    Ok((state, bay, events))
}

#[cfg(test)]
mod tests;
