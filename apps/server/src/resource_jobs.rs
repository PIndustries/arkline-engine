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
    /// Identified bay launches committed atomically with the actual drone.
    #[serde(default)]
    bay_launches: Vec<(u64, u64)>,
    #[serde(default)]
    initial_cargo: u64,
    #[serde(default, skip_serializing_if = "zero")]
    initial_lost_ore: u64,
}
fn zero(v: &u64) -> bool {
    *v == 0
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
    ensure!(
        cargo_unloading::active(w, name).is_none(),
        "worker cargo is reserved by unloading"
    );
    ensure!(
        !drone_bay::busy(w, name) && !drone_bay::batch_active(w, name),
        "finish, cancel or resume the occupied drone bay/batch before starting a resource job"
    );
    ensure!(
        drone_cargo::state(w, name)
            .is_none_or(|s| !s.operations.iter().any(|o| o.active()) && s.cleanup.is_none()),
        "finish ore service before starting a resource job"
    );
    let p = w.pilots[name].view.clone();
    let cargo_state = drone_cargo::state(w, name);
    let cargo_enabled = cargo_state.is_some();
    let initial_lost_ore = cargo_state.map_or(0, |s| s.lost_ore_units);
    let available = p.ore.saturating_add(
        cargo_state
            .and_then(DroneCargoState::ore_units)
            .unwrap_or(0),
    );
    ensure!(
        p.station_avatar.is_none(),
        "board your ship before starting a resource job"
    );
    ensure!(
        !matches!(p.location, Location::Jumping { .. }),
        "wait for gate travel to finish before assigning a resource job"
    );
    ensure!(
        (1..=MAX_RESOURCE_JOB_UNITS).contains(&units),
        "total delivery target must be 1 through 1,000,000 ore"
    );
    ensure!(
        w.resource_jobs
            .get(name)
            .is_none_or(|j| j.view.phase.is_terminal()),
        "pilot already has an active resource job"
    );
    if available < units && available < cargo_capacity(p.ship_class) {
        if let Some(stock) = drone_bay::state(w, name) {
            let counts = stock.counts(Some(DroneClass::Mining));
            ensure!(counts.stored+counts.active==DRONE_CLASS_CAP,"insufficient surviving mining stock: automated jobs require five miners; surviving drones remain manually usable");
        }
    }
    let initial_phase = if available >= units || available >= cargo_capacity(p.ship_class) {
        if p.location == Location::Docked {
            ResourceJobPhase::Depositing
        } else {
            ResourceJobPhase::ReturningToStation
        }
    } else if p.location == Location::Docked {
        ResourceJobPhase::Undocking
    } else {
        ResourceJobPhase::TravelToBelt
    };
    let id = w.next_job_id.max(1);
    w.next_job_id = id.checked_add(1).context("job ID exhausted")?;
    w.resource_jobs.insert(
        name.into(),
        Job {
            home_system_id: p.system_id,
            stage_started_ms: now,
            drones_launched: 0,
            bay_launches: Vec::new(),
            initial_cargo: available,
            initial_lost_ore,
            view: ResourceJob {
                drone_cargo: cargo_enabled.then_some(DroneCargoJobProgress {
                    historical_extracted_units: 0,
                    available_units: available,
                    confirmed_delivered_units: 0,
                    lost_units: 0,
                }),
                id,
                pilot: name.into(),
                controller: controller.into(),
                target_units: units,
                mined_units: 0,
                stored_units: 0,
                target_id: None,
                phase: initial_phase,
                message: "Preparing haul; existing cargo counts toward storage target".into(),
                started_at_ms: now,
                updated_at_ms: now,
            },
        },
    );
    drone_cargo::clear_intents(w, name);
    stop(
        w.pilots.get_mut(name).unwrap(),
        "Resource job taking control",
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
        d.mode = if p
            .view
            .drone_bay
            .as_ref()
            .is_some_and(|b| b.constrained(d.id).is_some())
            || p.view.drone_cargo.as_ref().is_some_and(|c| {
                c.operations
                    .iter()
                    .any(|o| o.drone_id == d.id && o.active())
            }) {
            DroneMode::Hold
        } else {
            DroneMode::Returning
        };
        d.mining_cycle = None;
    }
    p.view.drone = p.view.drones.first().cloned();
}
pub(super) fn cancel(w: &mut World, name: &str, now: u64, reason: &str) {
    if let Some(id) = w
        .resource_jobs
        .get(name)
        .filter(|j| !j.view.phase.is_terminal())
        .map(|j| j.view.id)
    {
        drone_cargo::cancel_job(w, name, id);
        let _ = drone_cargo::hold(w, name, now, reason);
    }
    if let Some(j) = w
        .resource_jobs
        .get_mut(name)
        .filter(|j| !j.view.phase.is_terminal())
    {
        j.view.phase = ResourceJobPhase::Cancelled;
        j.view.message = reason.into();
        j.view.updated_at_ms = now;
        if let Some(p) = w.pilots.get_mut(name) {
            j.view.mined_units = j.view.mined_units.max(
                j.view
                    .stored_units
                    .saturating_add(p.view.ore)
                    .saturating_sub(j.initial_cargo),
            );
            stop(p, reason);
        }
    }
    if drone_bay::state(w, name).is_some_and(|s| {
        s.operations
            .iter()
            .any(|op| !op.phase.terminal() && op.job_id.is_some())
    }) {
        drone_bay::hold(w, name, now, reason);
    }
    if cargo_unloading::active(w, name).is_some_and(|op| op.job_id.is_some()) {
        cargo_unloading::hold(w, name, reason);
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
    let cargo = drone_cargo::state(w, &j.view.pilot);
    let available = p
        .ore
        .saturating_add(cargo.and_then(DroneCargoState::ore_units).unwrap_or(0));
    if let Some(cargo) = cargo {
        j.view.drone_cargo = Some(DroneCargoJobProgress {
            historical_extracted_units: j.view.mined_units,
            available_units: available,
            confirmed_delivered_units: j.view.stored_units,
            lost_units: cargo.lost_ore_units.saturating_sub(j.initial_lost_ore),
        });
    }
    let newly_mined = j
        .view
        .stored_units
        .saturating_add(p.ore)
        .saturating_sub(j.initial_cargo);
    if cargo.is_none() && newly_mined > j.view.mined_units {
        j.view.mined_units = newly_mined;
        j.stage_started_ms = now;
    }
    j.view.updated_at_ms = now;
    if cargo.is_some_and(|c| {
        c.operations.iter().any(|o| {
            o.active()
                && o.phase != DroneDropoffPhase::Held
                && o.phase != DroneDropoffPhase::Queued
                && o.updated_at_ms == now
        })
    }) {
        j.stage_started_ms = now;
    }
    if p.system_id != j.home_system_id {
        block(j, "Worker left the assigned system", now);
        return None;
    }
    // Serialized physical handling is genuine job progress. Five real launches
    // may take longer than the former instantaneous deployment timeout, but a
    // held/blocked/offline route never refreshes this deadline.
    if drone_bay::state(w, &j.view.pilot).is_some_and(|s| {
        s.operations.iter().any(|op| {
            op.job_id == Some(j.view.id)
                && !op.phase.terminal()
                && op.phase != DroneBayPhase::Held
                && op.updated_at_ms == now
        })
    }) {
        j.stage_started_ms = now;
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
    if drone_bay::busy(w, &j.view.pilot) {
        let held = drone_bay::state(w, &j.view.pilot).is_some_and(|s| {
            s.cleanup.as_ref().is_some_and(|c| c.held)
                || s.operations
                    .iter()
                    .any(|op| op.phase == DroneBayPhase::Held)
        });
        j.view.message = if held {
            "Drone bay held; explicit pilot resume or cancel required"
        } else {
            "Waiting for physical drone handling and safe machinery"
        }
        .into();
        return None;
    }
    if p.cockpit_mechanisms
        .as_ref()
        .is_some_and(|m| m.turrets.source_admitted && m.turrets.pending_warp.is_some())
    {
        j.view.message = "Securing physical turrets before warp; waiting for actual transit".into();
        return None;
    }
    match j.view.phase {
        Undocking => {
            if p.flight.as_ref().and_then(|f| f.docking.as_ref()).is_some() {
                if docking::busy(&w.pilots[&j.view.pilot]) {
                    j.view.message = "Waiting for controlled departure".into();
                    return None;
                }
                if p.location == Location::Docked {
                    return Some(Request::Undock);
                }
                phase(j, TravelToBelt, "Departed station", now);
                None
            } else {
                phase(j, TravelToBelt, "Departing station", now);
                Some(Request::Undock)
            }
        }
        TravelToBelt => {
            if !p.drones.is_empty() {
                if let Some(cargo) = cargo {
                    // Fresh ore worlds may start with manually deployed loaded
                    // identities. Preparation uses the normal accepted service
                    // and storage routes; legacy worlds keep their old policy.
                    if f.velocity.iter().any(|v| v.abs() > 0.01) || f.mode != NavigationMode::Stop {
                        j.view.message =
                            "Braking before preparing the deployed mining fleet".into();
                        return None;
                    }
                    if cargo.operations.iter().any(|o| o.active()) || cargo.cleanup.is_some() {
                        j.view.message =
                            "Waiting for owned ore service; resume it explicitly if held".into();
                        return None;
                    }
                    if let Some(hold) = cargo.holds.iter().find(|h| h.ore_units > 0) {
                        j.view.message = "Unloading the existing manual fleet before travel".into();
                        return Some(Request::DroneCargo {
                            request: DroneCargoRequest::BeginDropoff {
                                binding: cargo.binding.clone(),
                                operation_id: cargo.high_watermark.checked_add(1)?,
                                drone_id: hold.drone_id,
                                resume_mining: false,
                            },
                        });
                    }
                    if let Some(stock) = drone_bay::state(w, &j.view.pilot) {
                        j.view.message = "Stowing the existing manual fleet before travel".into();
                        return Some(Request::DroneBay {
                            request: DroneBayRequest::Recover {
                                binding: stock.binding.clone(),
                                operation_id: stock.high_watermark.checked_add(1)?,
                                drone_id: p.drones[0].id,
                            },
                        });
                    }
                }
                j.view.message = "Waiting for deployed drones to return before gathering".into();
                return None;
            }
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
            if available >= j.view.target_units.saturating_sub(j.view.stored_units)
                || available >= cargo_capacity(p.ship_class)
            {
                phase(
                    j,
                    ReturningToStation,
                    "Haul ready; recovering mining drones",
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
            // Let the approach controller finish its mass-aware braking. Issuing
            // Stop at the range edge would overshoot with a massive loaded hull.
            if matches!(f.mode, NavigationMode::Approach { .. })
                && f.velocity.iter().map(|v| v * v).sum::<f64>().sqrt() > 10.0
            {
                j.view.message = "Braking onto the mining approach".into();
                return None;
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
            if let Some(mechanisms) = &p.cockpit_mechanisms {
                if mechanisms.turrets.source_admitted && !mechanisms.turrets.operational_at(now) {
                    if mechanisms
                        .turrets
                        .actuators
                        .iter()
                        .any(|a| a.blocked.is_some())
                    {
                        j.view.message = "Physical turret travel held; clear the obstruction and resume manually".into();
                        return None;
                    }
                    j.view.message = "Deploying physical mining turrets".into();
                    if mechanisms.turrets.pending_warp.is_none()
                        && mechanisms
                            .turrets
                            .actuators
                            .iter()
                            .any(|a| !a.target_deployed || a.blocked.is_some())
                    {
                        return Some(Request::SetTurretDeployment {
                            vessel_id: mechanisms.vessel_id.clone(),
                            deployed: true,
                        });
                    }
                    return None;
                }
            }
            if p.drones.len() < usize::from(j.drones_launched) {
                block(
                    j,
                    "Mining fleet lost a drone; inspect the fleet and restart the job",
                    now,
                );
                return None;
            }
            if available >= j.view.target_units.saturating_sub(j.view.stored_units)
                || available >= cargo_capacity(p.ship_class)
            {
                phase(
                    j,
                    ReturningToStation,
                    "Haul ready; recovering mining drones",
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
                if let Some(stock) = drone_bay::state(w, &j.view.pilot) {
                    let Some(entry) = stock.stock.iter().find(|e| {
                        e.class == DroneClass::Mining
                            && matches!(e.status, DroneStockStatus::Stored { .. })
                    }) else {
                        block(
                            j,
                            "Insufficient stored mining drones; no free replacement is created",
                            now,
                        );
                        return None;
                    };
                    let Some(operation_id) = stock.high_watermark.checked_add(1) else {
                        block(j, "Bay operation identity exhausted", now);
                        return None;
                    };
                    return Some(Request::DroneBay {
                        request: DroneBayRequest::Launch {
                            binding: stock.binding.clone(),
                            drone_id: entry.drone_id,
                            operation_id,
                        },
                    });
                }
                return Some(Request::LaunchDroneClass {
                    class: DroneClass::Mining,
                });
            }
            for d in &p.drones {
                if drone_cargo::constrained(w, &j.view.pilot, d.id) {
                    continue;
                }
                if !matches!(d.mode,DroneMode::Mining{target_id} if target_id==id) {
                    return Some(Request::AssignMiningDrone {
                        id: d.id,
                        target_id: id,
                    });
                }
            }
            if drone_cargo::hold_capacity(w, &j.view.pilot) <= p.ore {
                j.view.message =
                    "Waiting for accepted drone cycles within reserved ship headroom".into();
                return None;
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
            // Stop every free miner before serializing the long physical
            // recovery channel; do not let waiting drones keep gathering ore.
            if drone_bay::state(w, &j.view.pilot).is_some() {
                if let Some(d) = p
                    .drones
                    .iter()
                    .find(|d| !matches!(d.mode, DroneMode::Hold | DroneMode::Returning))
                {
                    return Some(Request::DroneHoldById { id: d.id });
                }
            }
            if let Some(cargo) = drone_cargo::state(w, &j.view.pilot) {
                if cargo.operations.iter().any(|o| o.active()) || cargo.cleanup.is_some() {
                    j.view.message = "Waiting for physical ore service and safe disconnect".into();
                    return None;
                }
                if let Some(hold) = cargo.holds.iter().find(|h| h.ore_units > 0) {
                    return Some(Request::DroneCargo {
                        request: DroneCargoRequest::BeginDropoff {
                            binding: cargo.binding.clone(),
                            operation_id: cargo.high_watermark.checked_add(1)?,
                            drone_id: hold.drone_id,
                            resume_mining: false,
                        },
                    });
                }
            }
            for d in &p.drones {
                if let Some(stock) = drone_bay::state(w, &j.view.pilot) {
                    let Some(operation_id) = stock.high_watermark.checked_add(1) else {
                        block(j, "Bay operation identity exhausted", now);
                        return None;
                    };
                    return Some(Request::DroneBay {
                        request: DroneBayRequest::Recover {
                            binding: stock.binding.clone(),
                            drone_id: d.id,
                            operation_id,
                        },
                    });
                }
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
            if f.docking.as_ref().is_some_and(|d| {
                flight::distance(f.position, docking_gate_position(d.berth_id)) <= 6000.
            }) || hull_surface_distance(p.ship_class, f.position, STATION_POSITION) <= DOCK_RANGE
            {
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
            } else if docking::busy(&w.pilots[&j.view.pilot]) {
                j.view.message = "Waiting for confirmed docking capture".into();
                None
            } else {
                block(j, "Docking did not complete", now);
                None
            }
        }
        Depositing => {
            if cargo_unloading::active(w, &j.view.pilot).is_some() {
                j.view.message = "Waiting for confirmed unloading batches".into();
                None
            } else if p.ore == 0 {
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
fn save(s: &Server, w: &mut World) -> Result<()> {
    validate(w)?;
    drone_bay::project(w)?;
    drone_cargo::project(w)?;
    drone_cargo::validate(w)?;
    s.db.execute(
        "UPDATE world SET json=?1 WHERE id=1",
        [serde_json::to_string(w)?],
    )?;
    Ok(())
}
/// Called inside the same transaction as the deposit and its economic event.
/// A failed world write must roll back both inventory and delivery progress.
pub(super) fn record_delivery(w: &mut World, name: &str, units: u64, now: u64) -> Result<()> {
    record_delivery_batch(w, name, units, now, true)
}
pub(super) fn record_delivery_batch(
    w: &mut World,
    name: &str,
    units: u64,
    now: u64,
    final_batch: bool,
) -> Result<()> {
    let j = w
        .resource_jobs
        .get_mut(name)
        .context("delivery job missing")?;
    ensure!(
        j.view.phase == ResourceJobPhase::Depositing,
        "job is not depositing"
    );
    j.view.stored_units = j
        .view
        .stored_units
        .checked_add(units)
        .context("job delivery total overflow")?;
    if !final_batch {
        j.view.updated_at_ms = now;
        return Ok(());
    }
    if j.view.stored_units >= j.view.target_units {
        phase(
            j,
            ResourceJobPhase::Completed,
            "Total delivery goal stored in personal station inventory",
            now,
        );
    } else if !w
        .asteroids
        .iter()
        .any(|a| a.system_id == j.home_system_id && a.ore > 0)
    {
        phase(
            j,
            ResourceJobPhase::Completed,
            "Available partial haul stored; local resources exhausted",
            now,
        );
    } else {
        j.drones_launched = 0;
        j.bay_launches.clear();
        j.view.target_id = None;
        phase(
            j,
            ResourceJobPhase::Undocking,
            "Haul stored; departing for remaining delivery goal",
            now,
        );
    }
    Ok(())
}

/// The caller owns the server exclusively and has just durably committed this
/// validated world. Reuse it only to enumerate work; every active job still
/// reloads current authority before planning and again after issuing a command.
pub(super) fn advance(s: &mut Server, now: u64, committed: &World) -> Result<()> {
    let names = committed
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
            drone_cargo::cancel_job(&mut w, &name, job.view.id);
            drone_cargo::hold(&mut w, &name, now, &job.view.message)?;
            stop(w.pilots.get_mut(&name).unwrap(), &job.view.message);
            drone_bay::hold(&mut w, &name, now, &job.view.message);
            drone_bay::project(&mut w)?;
        }
        w.resource_jobs.insert(name.clone(), job);
        save(s, &mut w)?;
        if let Some(command) = command {
            let launch = matches!(command, Request::LaunchDroneClass { .. });
            // This is the ordinary validated command handler, with a server-established
            // worker identity. Internal commands alone bypass manual takeover cancellation.
            let result = s.execute(&mut Some(name.clone()), command, now, true, None);
            let mut w = load(s)?;
            let j = w.resource_jobs.get_mut(&name).unwrap();
            match result {
                Ok(_) => {
                    if launch {
                        j.drones_launched += 1;
                    }
                }
                Err(error) => {
                    block(j, &format!("Command rejected: {error}"), now);
                    let id = j.view.id;
                    drone_cargo::cancel_job(&mut w, &name, id);
                    drone_cargo::hold(&mut w, &name, now, "Resource job blocked")?;
                    stop(w.pilots.get_mut(&name).unwrap(), "Resource job blocked");
                }
            }
            save(s, &mut w)?;
        }
    }
    Ok(())
}
/// Runs inside Server::execute's same IMMEDIATE inventory/world transaction.
pub(super) fn record_bay_launch(
    w: &mut World,
    name: &str,
    drone: u64,
    operation: u64,
) -> Result<()> {
    let j = w
        .resource_jobs
        .get_mut(name)
        .context("automated launch job missing")?;
    ensure!(
        j.view.phase == ResourceJobPhase::Mining,
        "job is not deploying miners"
    );
    if j.bay_launches.contains(&(drone, operation)) {
        return Ok(());
    }
    ensure!(
        !j.bay_launches
            .iter()
            .any(|(id, op)| *id == drone || *op == operation)
            && j.drones_launched < 5,
        "duplicate or excessive job drone launch"
    );
    j.bay_launches.push((drone, operation));
    j.drones_launched += 1;
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
                j.view.target_units <= MAX_RESOURCE_JOB_UNITS,
                "active job target exceeds delivery limit"
            );
        }
        ensure!(
            u128::from(j.view.stored_units)
                <= u128::from(j.view.mined_units) + u128::from(j.initial_cargo),
            "job stored more than gathered"
        );
    }
    Ok(())
}

pub(super) fn record_drone_extraction(
    w: &mut World,
    name: &str,
    id: u64,
    units: u64,
    now: u64,
) -> Result<()> {
    if let Some(j) = w
        .resource_jobs
        .get_mut(name)
        .filter(|j| j.view.id == id && !j.view.phase.is_terminal())
    {
        j.view.mined_units = j
            .view
            .mined_units
            .checked_add(units)
            .context("job extraction counter overflow")?;
        j.view.updated_at_ms = now;
        j.stage_started_ms = now;
    }
    Ok(())
}
pub(super) fn record_turret_extractions(
    w: &mut World,
    events: &[EconomicEvent],
    now: u64,
) -> Result<()> {
    if w.drone_cargo_registration.is_none() {
        return Ok(());
    }
    for event in events {
        if let EconomicEvent::OreMined { pilot, units } = event {
            if let Some(id) = w
                .resource_jobs
                .get(pilot)
                .filter(|j| !j.view.phase.is_terminal())
                .map(|j| j.view.id)
            {
                record_drone_extraction(w, pilot, id, *units, now)?;
            }
        }
    }
    Ok(())
}

/// Derived delivery telemetry is saved in the same transaction as cargo changes,
/// including a terminal job's final committed station batch.
pub(super) fn project_drone_cargo(w: &mut World) -> Result<()> {
    let values: Vec<_> = w
        .resource_jobs
        .keys()
        .filter_map(|name| {
            drone_cargo::state(w, name).map(|s| (name.clone(), s.ore_units(), s.lost_ore_units))
        })
        .collect();
    for (name, drone_ore, lost) in values {
        let available = w.pilots[&name]
            .view
            .ore
            .checked_add(drone_ore.context("job drone ore overflow")?)
            .context("job available ore overflow")?;
        let j = w.resource_jobs.get_mut(&name).unwrap();
        j.view.drone_cargo = Some(DroneCargoJobProgress {
            historical_extracted_units: j.view.mined_units,
            available_units: available,
            confirmed_delivered_units: j.view.stored_units,
            lost_units: lost.saturating_sub(j.initial_lost_ore),
        });
    }
    Ok(())
}
