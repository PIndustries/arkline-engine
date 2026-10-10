//! A bounded controller issuing the same commands as an authenticated cockpit.
use super::*;
const STAGE_TIMEOUT_MS: u64 = 300_000;
const JOB_TIMEOUT_MS: u64 = 7_200_000;
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Job {
    pub(super) view: ResourceJob,
    home_system_id: u32,
    stage_started_ms: u64,
    drones_launched: u8,
}
pub(super) fn authorize(w: &World, actor: &str, worker: &str) -> Result<()> {
    ensure!(w.pilots.contains_key(worker), "worker pilot not found");
    ensure!(
        actor == worker || w.fleet_links.get(worker).is_some_and(|c| c == actor),
        "worker has not granted fleet control"
    );
    Ok(())
}
pub(super) fn start(
    w: &mut World,
    name: &str,
    controller: &str,
    units: u64,
    now: u64,
) -> Result<()> {
    let p = &w.pilots[name].view;
    ensure!(
        p.location == Location::Docked,
        "resource job must start docked"
    );
    ensure!(p.ore == 0, "resource job requires empty active cargo");
    ensure!(
        p.drones.is_empty(),
        "recall drones before starting a resource job"
    );
    ensure!(
        (1..=cargo_capacity(p.ship_class)).contains(&units),
        "target must be 1 through active hull cargo capacity"
    );
    ensure!(
        w.resource_jobs
            .get(name)
            .is_none_or(|j| j.view.phase.is_terminal()),
        "pilot already has an active resource job"
    );
    let id = w.next_job_id.max(1);
    w.next_job_id = id.checked_add(1).context("job ID exhausted")?;
    w.resource_jobs.insert(
        name.into(),
        Job {
            home_system_id: p.system_id,
            stage_started_ms: now,
            drones_launched: 0,
            view: ResourceJob {
                id,
                pilot: name.into(),
                controller: controller.into(),
                target_units: units,
                mined_units: 0,
                stored_units: 0,
                target_id: None,
                phase: ResourceJobPhase::Undocking,
                message: "Preparing resource haul".into(),
                started_at_ms: now,
                updated_at_ms: now,
            },
        },
    );
    Ok(())
}
fn stop(p: &mut Pilot, reason: &str) {
    mining::stop_all(p, reason);
    p.controls = None;
    if universe::normal(p) {
        if let Some(f) = &mut p.view.flight {
            f.mode = NavigationMode::Stop;
            f.angular_velocity = [0.0; 3];
        }
    }
    // Cancellation does not destroy deployed assets: give drones their existing return behavior.
    for d in &mut p.view.drones {
        d.mode = DroneMode::Returning;
        d.mining_cycle = None;
    }
    p.view.drone = p.view.drones.first().cloned();
}
pub(super) fn cancel(w: &mut World, name: &str, now: u64, reason: &str) {
    if let Some(j) = w
        .resource_jobs
        .get_mut(name)
        .filter(|j| !j.view.phase.is_terminal())
    {
        j.view.phase = ResourceJobPhase::Cancelled;
        j.view.message = reason.into();
        j.view.updated_at_ms = now;
        if let Some(p) = w.pilots.get_mut(name) {
            j.view.mined_units = j.view.mined_units.max(p.view.ore);
            stop(p, reason);
        }
    }
}
pub(super) fn cancel_all(w: &mut World, now: u64, reason: &str) {
    let names = w.resource_jobs.keys().cloned().collect::<Vec<_>>();
    for name in names {
        cancel(w, &name, now, reason);
    }
}
fn phase(j: &mut Job, p: ResourceJobPhase, message: &str, now: u64) {
    if j.view.phase != p {
        j.stage_started_ms = now;
    }
    j.view.phase = p;
    j.view.message = message.into();
    j.view.updated_at_ms = now;
}
fn block(j: &mut Job, message: &str, now: u64) {
    phase(j, ResourceJobPhase::Blocked, message, now);
}
fn ready_power(j: &mut Job, p: &PilotView, cost: u64) -> bool {
    if p.power.as_ref().is_some_and(|power| power.charge_j >= cost) {
        true
    } else {
        j.view.message = "Waiting for capacitor recharge".into();
        false
    }
}
fn plan(w: &World, j: &mut Job, now: u64) -> Option<Request> {
    use ResourceJobPhase::*;
    let p = &w.pilots[&j.view.pilot].view;
    if p.ore > j.view.mined_units {
        j.view.mined_units = p.ore;
        j.stage_started_ms = now;
    }
    j.view.updated_at_ms = now;
    if p.system_id != j.home_system_id {
        block(j, "Worker left the assigned system", now);
        return None;
    }
    if now.saturating_sub(j.view.started_at_ms) > JOB_TIMEOUT_MS
        || now.saturating_sub(j.stage_started_ms) > STAGE_TIMEOUT_MS
    {
        block(
            j,
            "Job timed out without progress; manual review required",
            now,
        );
        return None;
    }
    let f = p.flight.as_ref()?;
    let in_transit = matches!(
        p.location,
        Location::InTransit { .. } | Location::Jumping { .. }
    );
    match j.view.phase {
        Undocking => {
            phase(j, TravelToBelt, "Departing station", now);
            Some(Request::Undock)
        }
        TravelToBelt => {
            if in_transit {
                j.view.message = "Warping to local asteroid belt".into();
                return None;
            }
            if flight::distance(f.position, BELT_POSITION) < 10000.0 {
                phase(j, AcquiringTarget, "Selecting an asteroid", now);
                None
            } else if ready_power(j, p, power_spec(p.ship_class).warp_cost_j) {
                Some(Request::Warp {
                    destination: Destination::Belt,
                })
            } else {
                None
            }
        }
        AcquiringTarget => {
            if p.ore >= j.view.target_units || p.ore >= cargo_capacity(p.ship_class) {
                phase(
                    j,
                    ReturningToStation,
                    "Target gathered; recovering mining drones",
                    now,
                );
                return None;
            }
            let asteroid = j.view.target_id.and_then(|id| {
                w.asteroids
                    .iter()
                    .find(|a| a.id == id && a.ore > 0 && a.system_id == p.system_id)
            });
            let asteroid = asteroid.or_else(|| {
                w.asteroids
                    .iter()
                    .filter(|a| a.ore > 0 && a.system_id == p.system_id)
                    .min_by(|a, b| {
                        flight::distance(f.position, a.position)
                            .total_cmp(&flight::distance(f.position, b.position))
                    })
            });
            let Some(a) = asteroid else {
                phase(
                    j,
                    ReturningToStation,
                    "No local ore remains; returning available haul",
                    now,
                );
                return None;
            };
            if j.view.target_id != Some(a.id) {
                j.view.target_id = Some(a.id);
                j.stage_started_ms = now;
            }
            if flight::distance(f.position, a.position) - a.radius > MINING_RANGE - 100.0 {
                j.view.message = "Approaching selected asteroid".into();
                let mode = NavigationMode::Approach {
                    target: NavigationTarget::Asteroid { id: a.id },
                };
                return (f.mode != mode).then_some(Request::Navigate { mode });
            }
            if !matches!(f.mode, NavigationMode::Stop) || f.velocity.iter().any(|v| v.abs() > 0.01)
            {
                return Some(Request::Navigate {
                    mode: NavigationMode::Stop,
                });
            }
            if p.locked_asteroids.contains(&a.id) {
                phase(j, Mining, "Deploying mining fleet and cycling turrets", now);
                return None;
            }
            if p.target_locks.iter().any(|l| l.asteroid_id == a.id) {
                j.view.message = "Acquiring confirmed asteroid lock".into();
                return None;
            }
            if p.locked_asteroids.len() + p.target_locks.len() >= MAX_TARGET_LOCKS {
                return Some(Request::UnlockTarget);
            }
            Some(Request::LockAsteroid { id: a.id })
        }
        Mining => {
            if p.ore >= j.view.target_units || p.ore >= cargo_capacity(p.ship_class) {
                phase(
                    j,
                    ReturningToStation,
                    "Target gathered; recovering mining drones",
                    now,
                );
                return None;
            }
            let Some(id) = j.view.target_id else {
                phase(j, AcquiringTarget, "Selecting replacement asteroid", now);
                return None;
            };
            if !p.locked_asteroids.contains(&id)
                || !w.asteroids.iter().any(|a| a.id == id && a.ore > 0)
            {
                phase(
                    j,
                    AcquiringTarget,
                    "Target depleted or unavailable; selecting another",
                    now,
                );
                return None;
            }
            if j.drones_launched < 5 {
                return Some(Request::LaunchDroneClass {
                    class: DroneClass::Mining,
                });
            }
            for d in &p.drones {
                if !matches!(d.mode,DroneMode::Mining{target_id} if target_id==id) {
                    return Some(Request::AssignMiningDrone {
                        id: d.id,
                        target_id: id,
                    });
                }
            }
            for m in &p.mining_modules {
                if m.target_id != Some(id) {
                    if !ready_power(j, p, power_spec(p.ship_class).mining_cycle_cost_j) {
                        return None;
                    }
                    return Some(Request::StartMining {
                        module_id: m.module_id,
                        target_id: id,
                    });
                }
            }
            j.view.message = "Mining with fixed-yield turrets and assigned drones".into();
            None
        }
        ReturningToStation => {
            for m in &p.mining_modules {
                if m.target_id.is_some() {
                    return Some(Request::StopMining {
                        module_id: m.module_id,
                    });
                }
            }
            for d in &p.drones {
                if !matches!(d.mode, DroneMode::Returning) {
                    return Some(Request::RecallDroneById { id: d.id });
                }
            }
            if !p.drones.is_empty() {
                j.view.message = "Waiting for all mining drones to return".into();
                return None;
            }
            if in_transit {
                j.view.message = "Warping back to starting station".into();
                return None;
            }
            if hull_surface_distance(p.ship_class, f.position, STATION_POSITION) <= DOCK_RANGE {
                phase(j, Docking, "Docking at starting station", now);
                Some(Request::Dock)
            } else if ready_power(j, p, power_spec(p.ship_class).warp_cost_j) {
                Some(Request::Warp {
                    destination: Destination::Station,
                })
            } else {
                None
            }
        }
        Docking => {
            if p.location == Location::Docked {
                phase(
                    j,
                    Depositing,
                    "Moving gathered ore into personal station storage",
                    now,
                );
                None
            } else {
                block(j, "Docking did not complete", now);
                None
            }
        }
        Depositing => {
            if p.ore == 0 {
                block(j, "No ore was available to store", now);
                None
            } else {
                Some(Request::DepositOre { units: p.ore })
            }
        }
        Completed | Blocked | Cancelled => None,
    }
}
fn load(s: &Server) -> Result<World> {
    let raw: String =
        s.db.query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))?;
    Ok(serde_json::from_str(&raw)?)
}
fn save(s: &Server, w: &World) -> Result<()> {
    validate(w)?;
    s.db.execute(
        "UPDATE world SET json=?1 WHERE id=1",
        [serde_json::to_string(w)?],
    )?;
    Ok(())
}
pub(super) fn advance(s: &mut Server, now: u64) -> Result<()> {
    let names = load(s)?
        .resource_jobs
        .iter()
        .filter(|(_, j)| !j.view.phase.is_terminal())
        .map(|(n, _)| n.clone())
        .collect::<Vec<_>>();
    for name in names {
        let mut w = load(s)?;
        let mut job = w.resource_jobs.remove(&name).context("job disappeared")?;
        if !s.online_sessions.contains_key(&job.view.controller) {
            block(&mut job, "Controller is offline", now);
        }
        let command = if job.view.phase.is_terminal() {
            None
        } else {
            plan(&w, &mut job, now)
        };
        if job.view.phase.is_terminal() {
            stop(w.pilots.get_mut(&name).unwrap(), &job.view.message);
        }
        w.resource_jobs.insert(name.clone(), job);
        save(s, &w)?;
        if let Some(command) = command {
            let deposit = if let Request::DepositOre { units } = &command {
                Some(*units)
            } else {
                None
            };
            let launch = matches!(command, Request::LaunchDroneClass { .. });
            // This is the ordinary validated command handler, with a server-established
            // worker identity. Internal commands alone bypass manual takeover cancellation.
            let result = s.execute(&mut Some(name.clone()), command, now, true);
            let mut w = load(s)?;
            let j = w.resource_jobs.get_mut(&name).unwrap();
            match result {
                Ok(_) => {
                    if launch {
                        j.drones_launched += 1;
                    }
                    if let Some(units) = deposit {
                        j.view.stored_units = units;
                        let message = if units >= j.view.target_units {
                            "Haul stored in personal station inventory"
                        } else {
                            "Available partial haul stored; local resources exhausted"
                        };
                        phase(j, ResourceJobPhase::Completed, message, now);
                    }
                }
                Err(error) => {
                    block(j, &format!("Command rejected: {error}"), now);
                    stop(w.pilots.get_mut(&name).unwrap(), "Resource job blocked");
                }
            }
            save(s, &w)?;
        }
    }
    Ok(())
}
pub(super) fn validate(w: &World) -> Result<()> {
    for (pilot, controller) in &w.fleet_links {
        ensure!(
            pilot != controller
                && w.pilots.contains_key(pilot)
                && w.pilots.contains_key(controller),
            "invalid fleet permission"
        );
    }
    for (name, j) in &w.resource_jobs {
        ensure!(
            name == &j.view.pilot
                && w.pilots.contains_key(name)
                && w.pilots.contains_key(&j.view.controller)
                && j.view.id > 0
                && j.view.id < w.next_job_id
                && j.view.target_units > 0
                && j.drones_launched <= 5,
            "invalid resource job identity"
        );
        if !j.view.phase.is_terminal() {
            authorize(w, &j.view.controller, name)?;
            ensure!(
                j.view.target_units <= cargo_capacity(w.pilots[name].view.ship_class),
                "active job target exceeds hull capacity"
            );
        }
        ensure!(
            j.view.stored_units <= j.view.mined_units,
            "job stored more than gathered"
        );
    }
    Ok(())
}
