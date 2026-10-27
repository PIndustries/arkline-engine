//! Transaction-owned adapter. Normal source admission remains fail-closed;
//! only the explicit fixed-source test world can currently exercise these paths.
use super::*;
use crate::{drone_bay, drone_cargo, drones, World};
use anyhow::{bail, ensure, Context, Result};
use arkline_protocol::drone_bay_geometry::{BayMotionSegment, BayPose};
use arkline_protocol::independent_bay_geometry::{self as geometry, IndependentBayDefinition};
use arkline_protocol::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod transfers;
pub(crate) use transfers::commit_transfers;
#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) enum Movement {
    Entry(IndependentBayEntryRuntime),
    Handling(IndependentBayRuntime),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Run {
    pub movement: Movement,
    pub cancel_requested: bool,
    pub cleanup_only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct OwnedBays {
    pub state: IndependentBayState,
    pub runs: BTreeMap<u64, Run>,
    /// Actual retained idle/held group roots, never reset from a visual timer.
    pub mechanisms: BTreeMap<u8, Vec<(String, BayPose)>>,
}
impl Run {
    fn view(&self, phase: IndependentBayPhase) -> Result<IndependentBayMotionView> {
        match &self.movement {
            Movement::Entry(r) => {
                let mut view = r.view()?;
                view.cleanup_only = self.cleanup_only;
                Ok(view)
            }
            Movement::Handling(r) => r.view(
                if self.cleanup_only {
                    IndependentBayPhase::Cancelling
                } else {
                    phase
                },
                self.cleanup_only,
            ),
        }
    }
    fn hold(&mut self, reason: &str) -> Result<()> {
        match &mut self.movement {
            Movement::Entry(r) => r.hold(reason),
            Movement::Handling(r) => r.hold(reason),
        }
    }
    fn ready(&self) -> bool {
        match &self.movement {
            Movement::Entry(r) => r.ready_to_handoff,
            Movement::Handling(r) => r.ready_to_finalize,
        }
    }
}
pub(crate) fn definition(w: &World) -> Result<Option<&'static IndependentBayDefinition>> {
    let Some(reg) = w.independent_bay_registration.as_ref() else {
        return Ok(None);
    };
    if let Some(definition) = geometry::lookup(reg) {
        return Ok(Some(definition));
    }
    // Deliberately unavailable in ordinary server binaries. The fixture is a
    // compiled authored source, not arbitrary caller supplied geometry.
    #[cfg(test)]
    if let Some(definition) =
        geometry::stationary_fixture_definition().filter(|d| &d.registration == reg)
    {
        return Ok(Some(definition));
    }
    bail!("Independent bay assembly is not admitted for normal world/travel use")
}
pub(crate) fn cargo_registration(def: &IndependentBayDefinition) -> DroneCargoRegistration {
    DroneCargoRegistration {
        base: def.registration.host.clone(),
        receiver_revision: def.registration.assembly_revision,
        receiver_source_sha256: def.registration.assembly_source_sha256.clone(),
    }
}
pub(crate) fn state<'a>(w: &'a World, name: &str) -> Option<&'a OwnedBays> {
    w.independent_bay_registration.as_ref()?;
    w.accounts.get(name)?.independent_bays.as_ref()
}
pub(crate) fn constrained(w: &World, name: &str, drone: u64) -> bool {
    state(w, name).is_some_and(|s| s.state.active_operation(drone).is_some())
}
pub(crate) fn initialize(w: &mut World, name: &str) -> Result<()> {
    let Some(def) = definition(w)? else {
        return Ok(());
    };
    let stock =
        drone_bay::state(w, name).context("Independent initialization needs finite owned stock")?;
    ensure!(
        w.accounts[name].independent_bays.is_none(),
        "Independent bay initialization must never be repeated"
    );
    let binding = IndependentBayBinding {
        vessel_id: stock.binding.vessel_id.clone(),
        registration: def.registration.clone(),
    };
    let mut lanes = Vec::new();
    let mut mechanisms = BTreeMap::new();
    for bay in &def.bays {
        let entry = stock
            .stock
            .iter()
            .find(|e| {
                e.home_cradle == bay.preferred_home && bay.compatible_classes.contains(&e.class)
            })
            .context("Source home/class differs from finite roster")?;
        ensure!(
            matches!(entry.status, DroneStockStatus::Stored { .. }),
            "Fresh initialization requires original stored identities"
        );
        let available = IndependentBayAvailability::Available;
        lanes.push(IndependentBayLane {
            bay_id: bay.bay_id,
            components: IndependentBayComponents {
                door: available.clone(),
                support: available.clone(),
                clamp: available,
                ore_mount: if bay.ore_transfer_capable {
                    IndependentBayAvailability::Available
                } else {
                    IndependentBayAvailability::Unavailable {
                        reason: "No ore hardware installed in this class bay".into(),
                    }
                },
            },
            stored_drone_id: Some(entry.drone_id),
            operation_id: None,
            cleanup: None,
        });
        mechanisms.insert(
            bay.bay_id,
            def.route(bay.bay_id, IndependentBayAction::Launch)
                .context("Source launch missing")?[0]
                .mechanism_poses
                .clone(),
        );
    }
    lanes.sort_by_key(|b| b.bay_id);
    let owned = OwnedBays {
        state: IndependentBayState {
            schema_version: INDEPENDENT_BAY_SCHEMA_VERSION,
            binding,
            lanes,
            high_watermark: 0,
            operations: vec![],
            manifold: IndependentBayManifold::default(),
        },
        runs: BTreeMap::new(),
        mechanisms,
    };
    ensure!(
        owned.state.validate_shape(&stock.stock),
        "Invalid fresh independent inventory"
    );
    w.accounts.get_mut(name).unwrap().independent_bays = Some(owned);
    Ok(())
}
fn parent(w: &World, name: &str) -> Result<BayPose> {
    drone_bay::runtime::parent(w, name)
}
fn phase(s: &mut OwnedBays, id: u64, value: IndependentBayPhase, now: u64, message: &str) {
    let op = s
        .state
        .operations
        .iter_mut()
        .find(|o| o.operation_id == id)
        .unwrap();
    op.phase = value;
    op.held_phase = None;
    op.updated_at_ms = now;
    op.message = message.into();
}
fn actual(w: &World, name: &str, id: u64, host: BayPose) -> Result<BayPose> {
    let d = w.pilots[name]
        .view
        .drones
        .iter()
        .find(|d| d.id == id)
        .context("Active independent drone missing")?;
    independent_world_to_body(
        host,
        BayPose {
            position_m: d.position,
            orientation_xyzw: d
                .orientation_xyzw
                .context("Independent drone full attitude missing")?,
        },
    )
    .context("Invalid body transform")
}
fn static_obstacles(w: &World, name: &str, host: BayPose) -> Result<Vec<[[f64; 3]; 2]>> {
    let mut asteroids: Vec<_> = w
        .asteroids
        .iter()
        .filter(|a| a.system_id == w.pilots[name].view.system_id)
        .collect();
    asteroids.sort_by_key(|a| a.id);
    asteroids
        .into_iter()
        .map(|a| {
            let p = independent_world_to_body(
                host,
                BayPose {
                    position_m: a.position,
                    orientation_xyzw: [0., 0., 0., 1.],
                },
            )
            .context("Invalid static obstacle frame")?
            .position_m;
            Ok([p.map(|v| v - a.radius), p.map(|v| v + a.radius)])
        })
        .collect()
}
fn start_run(
    w: &World,
    name: &str,
    s: &OwnedBays,
    id: u64,
    bay: u8,
    action: IndependentBayAction,
    now: u64,
) -> Result<Run> {
    let def = definition(w)?.context("Independent source unavailable")?;
    let op = s
        .state
        .operation(id)
        .context("Independent operation missing")?;
    let entry = drone_bay::state(w, name)
        .unwrap()
        .stock
        .iter()
        .find(|e| e.drone_id == op.drone_id)
        .unwrap();
    let route = def
        .route(bay, action)
        .context("Compatible source route missing")?;
    let host = parent(w, name)?;
    let body = if action == IndependentBayAction::Launch {
        def.bays
            .iter()
            .find(|b| b.bay_id == bay)
            .unwrap()
            .stored_pose
    } else {
        actual(w, name, op.drone_id, host)?
    };
    let start = IndependentBayMotionStart {
        binding: s.state.binding.clone(),
        operation_id: id,
        drone_id: op.drone_id,
        bay_id: bay,
        action,
        parent: host,
        system_id: w.pilots[name].view.system_id,
        class: entry.class,
        actual_body_pose: body,
        actual_mechanism_poses: s.mechanisms[&bay].clone(),
        now,
    };
    // World-to-body roundoff can be ~10um at AU positions. Exact registered
    // entry is established only by the retained entry executor, never by snap.
    let movement = if body == route[0].drone_pose {
        Movement::Handling(IndependentBayRuntime::from_route(start, def, route)?)
    } else {
        Movement::Entry(IndependentBayEntryRuntime::from_definition_with_obstacles(
            start,
            def,
            &route,
            static_obstacles(w, name, host)?,
        )?)
    };
    Ok(Run {
        movement,
        cancel_requested: false,
        cleanup_only: false,
    })
}
fn place(w: &mut World, name: &str, view: &IndependentBayMotionView, now: u64) -> Result<()> {
    if view.cleanup_only {
        return Ok(());
    }
    let pose = view
        .parent
        .compose(view.drone.confirmed)
        .context("Independent world pose overflow")?;
    let d = w
        .pilots
        .get_mut(name)
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == view.drone_id)
        .context("Independent body missing")?;
    d.position = pose.position_m;
    d.orientation_xyzw = Some(pose.orientation_xyzw);
    d.heading = pose
        .transform_vector([1., 0., 0.])
        .context("Invalid drone heading")?;
    d.velocity = if let Some(segment) = view.drone.segment {
        view.parent
            .transform_vector(segment.sample(now).context("Invalid accepted velocity")?.1)
            .context("Invalid world velocity")?
    } else {
        [0.; 3]
    };
    d.mode = DroneMode::Hold;
    d.updated_at_ms = now;
    d.mining_cycle = None;
    d.attack_ready_at_ms = None;
    d.signal_strength = drone_signal_strength(d.position, view.parent.position_m);
    drones::sync(w.pilots.get_mut(name).unwrap());
    Ok(())
}
fn launch_identity(w: &mut World, name: &str, s: &mut OwnedBays, id: u64, now: u64) -> Result<()> {
    let op = s.state.operation(id).unwrap();
    let bay = op.assigned_bay.unwrap();
    let drone_id = op.drone_id;
    let view = s.runs[&id].view(op.phase)?;
    let pose = view
        .parent
        .compose(view.drone.confirmed)
        .context("Invalid launch pose")?;
    let stock = w
        .accounts
        .get_mut(name)
        .unwrap()
        .drone_bays
        .get_mut(&s.state.binding.vessel_id)
        .unwrap();
    let entry = stock
        .stock
        .iter_mut()
        .find(|e| e.drone_id == drone_id)
        .unwrap();
    let DroneStockStatus::Stored { hit_points } = entry.status else {
        bail!("Launch must move same stored identity")
    };
    let fleet = &mut w.pilots.get_mut(name).unwrap().view.drones;
    ensure!(
        fleet.len() < MAX_DRONES
            && fleet.iter().filter(|d| d.class == entry.class).count() < DRONE_CLASS_CAP
            && !fleet.iter().any(|d| d.id == drone_id),
        "Finite fleet cap or identity conflict"
    );
    fleet.push(DroneView {
        id: drone_id,
        class: entry.class,
        hit_points,
        orientation_xyzw: Some(pose.orientation_xyzw),
        position: pose.position_m,
        heading: pose.transform_vector([1., 0., 0.]).unwrap(),
        velocity: [0.; 3],
        mode: DroneMode::Hold,
        signal_strength: 1.,
        mining_cycle: None,
        attack_ready_at_ms: None,
        updated_at_ms: now,
    });
    entry.status = DroneStockStatus::Active;
    s.state.lanes[usize::from(bay)].stored_drone_id = None;
    Ok(())
}
pub(crate) fn execute(
    w: &mut World,
    name: &str,
    request: &IndependentBayRequest,
    now: u64,
) -> Result<()> {
    definition(w)?.context("Independent bays unavailable; old worlds are not migrated")?;
    let mut owned = state(w, name)
        .context("Owned independent initialization missing")?
        .clone();
    match request {
        IndependentBayRequest::Begin { .. } => {
            if begin(
                &mut owned.state,
                &drone_bay::state(w, name).unwrap().stock,
                request,
                now,
                None,
            )? == BeginDisposition::Retained
            {
                return Ok(());
            }
            ensure!(
                w.resource_jobs
                    .get(name)
                    .is_none_or(|j| j.view.phase.is_terminal()),
                "Cancel active resource routine before manual bay operation"
            );
            parent(w, name)?;
            // A queued return owns this drone immediately, even if no compatible
            // lane can yet be reserved. Stop outgoing work without replacing the
            // retained mining intent used by an explicit resume-after-unload.
            if let IndependentBayRequest::Begin { drone_id, .. } = request {
                if let Some(drone) = w
                    .pilots
                    .get_mut(name)
                    .unwrap()
                    .view
                    .drones
                    .iter_mut()
                    .find(|d| d.id == *drone_id)
                {
                    drone.mode = DroneMode::Hold;
                    drone.velocity = [0.; 3];
                    drone.mining_cycle = None;
                    drone.attack_ready_at_ms = None;
                    drone.updated_at_ms = now;
                }
            }
            w.accounts.get_mut(name).unwrap().independent_bays = Some(owned);
            admit(w, name, now)?;
            return Ok(());
        }
        IndependentBayRequest::Hold {
            binding,
            operation_id,
        }
        | IndependentBayRequest::Resume {
            binding,
            operation_id,
        }
        | IndependentBayRequest::Cancel {
            binding,
            operation_id,
        } => {
            ensure!(
                binding == &owned.state.binding,
                "Foreign independent bay binding"
            );
            let op = owned
                .state
                .operation(*operation_id)
                .context("Unknown retained operation")?
                .clone();
            if op.phase.terminal() && !owned.runs.contains_key(operation_id) {
                return Ok(());
            }
            match request {
                IndependentBayRequest::Hold { .. } => hold_owned(
                    w,
                    name,
                    &mut owned,
                    *operation_id,
                    now,
                    "User held independent bay",
                )?,
                IndependentBayRequest::Cancel { .. } => {
                    if !cancel_unstarted(&mut owned.state, *operation_id, now)? {
                        let run = owned
                            .runs
                            .get_mut(operation_id)
                            .context("Accepted handling missing")?;
                        run.cancel_requested = true;
                        // No unsafe reversal: retain current accepted physical route
                        // to its safe outside/stored endpoint; stop future pumping.
                        if owned
                            .state
                            .manifold
                            .grant
                            .as_ref()
                            .is_some_and(|g| g.physical_operation_id == *operation_id)
                        {
                            transfers::close_attempt(
                                w,
                                name,
                                &mut owned,
                                *operation_id,
                                now,
                                false,
                            )?;
                        }
                    }
                }
                IndependentBayRequest::Resume { .. } => {
                    let host = parent(w, name)?;
                    if let Some(run) = owned.runs.get_mut(operation_id) {
                        let def = definition(w)?.unwrap();
                        match &mut run.movement {
                            Movement::Entry(r) => {
                                let route = def.route(r.bay_id, r.action).unwrap();
                                r.validate_definition(
                                    def,
                                    &route,
                                    host,
                                    w.pilots[name].view.system_id,
                                )?;
                                r.resume_validated(now)?;
                            }
                            Movement::Handling(r) => {
                                r.validate_expected(
                                    &def.route(r.bay_id, r.route_action).unwrap(),
                                    host,
                                    w.pilots[name].view.system_id,
                                )?;
                                r.resume_validated(now)?;
                            }
                        }
                    }
                    if owned.runs.get(operation_id).is_some_and(|r| r.cleanup_only) {
                        owned.state.lanes[usize::from(op.assigned_bay.unwrap())]
                            .cleanup
                            .as_mut()
                            .context("Cleanup ownership missing")?
                            .held = false;
                    }
                    let value = op.held_phase.unwrap_or(op.phase);
                    phase(
                        &mut owned,
                        *operation_id,
                        if matches!(
                            value,
                            IndependentBayPhase::Transferring
                                | IndependentBayPhase::WaitingForManifold
                        ) {
                            IndependentBayPhase::Coupling
                        } else {
                            value
                        },
                        now,
                        "Resumed accepted route",
                    );
                }
                _ => unreachable!(),
            }
        }
    }
    w.accounts.get_mut(name).unwrap().independent_bays = Some(owned);
    Ok(())
}
fn admit(w: &mut World, name: &str, now: u64) -> Result<()> {
    let def = definition(w)?.context("Independent source missing")?;
    let mut s = state(w, name).unwrap().clone();
    let ids: Vec<_> = s
        .state
        .operations
        .iter()
        .filter(|o| o.phase == IndependentBayPhase::Queued)
        .map(|o| o.operation_id)
        .collect();
    for id in ids {
        let op = s.state.operation(id).unwrap().clone();
        let stock = &drone_bay::state(w, name).unwrap().stock;
        let entry = stock.iter().find(|e| e.drone_id == op.drone_id).unwrap();
        let route_action = if op.action == IndependentBayAction::ReturnToStorage
            && drone_cargo::payload(w, name, op.drone_id) > 0
        {
            IndependentBayAction::ReturnToUnload {
                resume_mining: false,
            }
        } else {
            op.action
        };
        let ore = matches!(route_action, IndependentBayAction::ReturnToUnload { .. });
        let mut bays: Vec<_> = def
            .bays
            .iter()
            .filter(|b| {
                b.compatible_classes.contains(&entry.class) && (!ore || b.ore_transfer_capable)
            })
            .collect();
        bays.sort_by_key(|b| (b.preferred_home != entry.home_cradle, b.bay_id));
        for bay in bays {
            let mut candidate = s.clone();
            if reserve_lane(&mut candidate.state, stock, id, &[bay.bay_id], ore, now)?.is_none() {
                continue;
            }
            let run = match start_run(w, name, &candidate, id, bay.bay_id, route_action, now) {
                Ok(run) => run,
                Err(_) => continue,
            };
            candidate.runs.insert(id, run);
            if !route_clear(w, name, &candidate, id, now, now.saturating_add(100))? {
                continue;
            }
            s = candidate;
            if op.action == IndependentBayAction::Launch {
                launch_identity(w, name, &mut s, id, now)?;
            }
            let view = s.runs[&id].view(s.state.operation(id).unwrap().phase)?;
            place(w, name, &view, now)?;
            break;
        }
    }
    w.accounts.get_mut(name).unwrap().independent_bays = Some(s);
    Ok(())
}
fn hold_owned(
    w: &mut World,
    name: &str,
    s: &mut OwnedBays,
    id: u64,
    now: u64,
    reason: &str,
) -> Result<()> {
    if s.state
        .manifold
        .grant
        .as_ref()
        .is_some_and(|g| g.physical_operation_id == id)
    {
        transfers::close_attempt(w, name, s, id, now, false)?;
    }
    if let Some(run) = s.runs.get_mut(&id) {
        run.hold(reason)?;
    }
    if s.runs.get(&id).is_some_and(|r| r.cleanup_only) {
        let bay = s.state.operation(id).unwrap().assigned_bay.unwrap();
        let cleanup = s.state.lanes[usize::from(bay)]
            .cleanup
            .as_mut()
            .context("Missing cleanup ownership")?;
        cleanup.held = true;
        cleanup.reason = reason.into();
        return Ok(());
    }
    hold_operation(&mut s.state, id, now, reason)
}
pub(crate) fn stop_all(w: &mut World, name: &str, now: u64, reason: &str) -> Result<()> {
    let Some(mut s) = state(w, name).cloned() else {
        return Ok(());
    };
    let mut ids: Vec<_> = s
        .state
        .operations
        .iter()
        .filter(|o| !o.phase.terminal())
        .map(|o| o.operation_id)
        .collect();
    ids.extend(s.runs.keys().copied());
    ids.sort_unstable();
    ids.dedup();
    for id in ids {
        hold_owned(w, name, &mut s, id, now, reason)?;
    }
    w.accounts.get_mut(name).unwrap().independent_bays = Some(s);
    Ok(())
}
fn stationary(pose: BayPose, now: u64) -> BayMotionSegment {
    BayMotionSegment {
        source: pose,
        target: pose,
        started_at_ms: now,
        duration_ms: 1,
    }
}
fn bodies(
    def: &IndependentBayDefinition,
    view: &IndependentBayMotionView,
    now: u64,
) -> Result<Vec<IndependentBaySweptBody>> {
    let bay = def
        .bays
        .iter()
        .find(|b| b.bay_id == view.bay_id)
        .context("Unknown lane source")?;
    let mut result = Vec::new();
    if !view.cleanup_only {
        result.push(
            independent_entry_swept_body(
                view.drone
                    .segment
                    .unwrap_or_else(|| stationary(view.drone.confirmed, now)),
                bay.drone_bounds_local_m,
            )
            .context("Unsupported drone sweep")?,
        );
    }
    for group in &view.mechanisms {
        let source = bay
            .mechanisms
            .iter()
            .find(|g| g.semantic_id == group.semantic_id)
            .context("Unknown moving group")?;
        result.push(IndependentBaySweptBody {
            segment: group
                .pose
                .segment
                .unwrap_or_else(|| stationary(group.pose.confirmed, now)),
            local_bounds_m: source.local_bounds_m,
        });
    }
    Ok(result)
}
fn route_clear(
    w: &World,
    name: &str,
    s: &OwnedBays,
    id: u64,
    start: u64,
    end: u64,
) -> Result<bool> {
    let def = definition(w)?.unwrap();
    let op = s.state.operation(id).unwrap();
    let view = s.runs[&id].view(op.phase)?;
    let route_action = match &s.runs[&id].movement {
        Movement::Entry(r) => r.action,
        Movement::Handling(r) => r.route_action,
    };
    let components = &s.state.lanes[usize::from(view.bay_id)].components;
    if !components.handling_available()
        || (matches!(route_action, IndependentBayAction::ReturnToUnload { .. })
            && !components.ore_mount.available())
    {
        return Ok(false);
    }
    let ours = bodies(def, &view, start)?;
    // Trusted constrained peers use registered full oriented boxes at the same
    // time. No sphere overlap shortcut, global gantry exclusion or ignored IDs.
    for bay in &def.bays {
        if bay.bay_id == view.bay_id {
            continue;
        }
        let lane = &s.state.lanes[usize::from(bay.bay_id)];
        let peer_id = lane
            .operation_id
            .or_else(|| lane.cleanup.as_ref().map(|c| c.operation_id));
        let peers = if let Some(peer) = peer_id.and_then(|i| s.runs.get(&i).map(|r| (i, r))) {
            bodies(
                def,
                &peer.1.view(s.state.operation(peer.0).unwrap().phase)?,
                start,
            )?
        } else {
            let mut list = Vec::new();
            if lane.stored_drone_id.is_some() {
                list.push(IndependentBaySweptBody {
                    segment: stationary(bay.stored_pose, start),
                    local_bounds_m: bay.drone_bounds_local_m,
                });
            }
            for (semantic, pose) in &s.mechanisms[&bay.bay_id] {
                list.push(IndependentBaySweptBody {
                    segment: stationary(*pose, start),
                    local_bounds_m: bay
                        .mechanisms
                        .iter()
                        .find(|m| m.semantic_id == *semantic)
                        .unwrap()
                        .local_bounds_m,
                });
            }
            list
        };
        if ours.iter().any(|a| {
            peers
                .iter()
                .any(|b| !independent_bay_sweeps_clear(*a, *b, start, end, 0.001))
        }) {
            return Ok(false);
        }
    }
    let host = view.parent;
    for (peer_name, p) in &w.pilots {
        if p.view.system_id != w.pilots[name].view.system_id {
            continue;
        }
        for d in &p.view.drones {
            if peer_name == name
                && (d.id == op.drone_id
                    || s.runs
                        .keys()
                        .any(|id| s.state.operation(*id).is_some_and(|o| o.drone_id == d.id)))
            {
                continue;
            }
            let body = independent_world_to_body(
                host,
                BayPose {
                    position_m: d.position,
                    orientation_xyzw: d.orientation_xyzw.unwrap_or([0., 0., 0., 1.]),
                },
            )
            .context("Peer body transform invalid")?;
            let moving = !matches!(d.mode, DroneMode::Hold);
            let reach = if moving {
                drone_speed(d.class)
                    * ((end.saturating_sub(d.updated_at_ms).min(10_000)) as f64 / 1000.)
            } else {
                0.
            };
            let radius =
                independent_drone_radius([[-2.5, -0.9, -3.], [2.5, 0.9, 3.]]).unwrap() + reach;
            let peer = IndependentBaySweptBody {
                segment: stationary(
                    BayPose {
                        orientation_xyzw: [0., 0., 0., 1.],
                        ..body
                    },
                    start,
                ),
                local_bounds_m: [[-radius; 3], [radius; 3]],
            };
            if ours
                .iter()
                .any(|a| !independent_bay_sweeps_clear(*a, peer, start, end, 0.001))
            {
                return Ok(false);
            }
        }
        if peer_name != name && crate::universe::normal(p) {
            if let Some(f) = &p.view.flight {
                let body = independent_world_to_body(
                    host,
                    BayPose {
                        position_m: f.position,
                        orientation_xyzw: f.orientation,
                    },
                )
                .context("Carrier obstacle transform")?;
                let radius = hull_radius_m(p.view.ship_class)
                    + f.velocity.iter().map(|v| v * v).sum::<f64>().sqrt() * 2.;
                let peer = IndependentBaySweptBody {
                    segment: stationary(
                        BayPose {
                            orientation_xyzw: [0., 0., 0., 1.],
                            ..body
                        },
                        start,
                    ),
                    local_bounds_m: [[-radius; 3], [radius; 3]],
                };
                if ours
                    .iter()
                    .any(|a| !independent_bay_sweeps_clear(*a, peer, start, end, 0.001))
                {
                    return Ok(false);
                }
            }
        }
    }
    for asteroid in w
        .asteroids
        .iter()
        .filter(|a| a.system_id == w.pilots[name].view.system_id)
    {
        let body = independent_world_to_body(
            host,
            BayPose {
                position_m: asteroid.position,
                orientation_xyzw: [0., 0., 0., 1.],
            },
        )
        .context("Asteroid transform")?;
        let peer = IndependentBaySweptBody {
            segment: stationary(
                BayPose {
                    orientation_xyzw: [0., 0., 0., 1.],
                    ..body
                },
                start,
            ),
            local_bounds_m: [[-asteroid.radius; 3], [asteroid.radius; 3]],
        };
        if ours
            .iter()
            .any(|a| !independent_bay_sweeps_clear(*a, peer, start, end, 0.001))
        {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn advance(w: &mut World, now: u64, defer: Option<&str>) -> Result<()> {
    if definition(w)?.is_none() {
        return Ok(());
    }
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    // All pair checks read the same pre-advance snapshot; iteration order cannot
    // make a later peer appear stationary merely because its timestamp is newer.
    let snapshot = w.clone();
    for name in names {
        let Some(mut s) = state(w, &name).cloned() else {
            continue;
        };
        let host = parent(w, &name)?;
        let system = w.pilots[&name].view.system_id;
        let clearance_state = s.clone();
        for id in s.runs.keys().copied().collect::<Vec<_>>() {
            let phase_before = s.state.operation(id).unwrap().phase;
            let old = s.runs[&id].view(phase_before)?;
            let clear = route_clear(
                &snapshot,
                &name,
                &clearance_state,
                id,
                old.confirmed_at_ms,
                now,
            )?;
            let run = s.runs.get_mut(&id).unwrap();
            let progress = match &mut run.movement {
                Movement::Entry(r) => r.advance(now, clear, host, system)?,
                Movement::Handling(r) => r.advance(now, clear, host, system)?,
            };
            let mut view = run.view(phase_before)?;
            if progress == IndependentBayMotionProgress::Held {
                hold_owned(
                    w,
                    &name,
                    &mut s,
                    id,
                    now,
                    "Physical lane held: obstruction, stale clock or explicit pause",
                )?;
                view = s.runs[&id].view(IndependentBayPhase::Held)?;
            } else if progress == IndependentBayMotionProgress::TransferBarrier {
                if s.runs[&id].cancel_requested
                    || drone_cargo::payload(w, &name, view.drone_id) == 0
                {
                    if s.state
                        .manifold
                        .grant
                        .as_ref()
                        .is_some_and(|g| g.physical_operation_id == id)
                    {
                        transfers::close_attempt(w, &name, &mut s, id, now, false)?;
                    }
                    s.state.manifold.waiting.retain(|v| *v != id);
                    if let Movement::Handling(r) = &mut s.runs.get_mut(&id).unwrap().movement {
                        r.release_transfer_barrier(now)?;
                    }
                    if !s.runs[&id].cleanup_only {
                        phase(
                            &mut s,
                            id,
                            IndependentBayPhase::Uncoupling,
                            now,
                            "Disconnecting source-confirmed contact",
                        );
                    }
                } else if !matches!(
                    phase_before,
                    IndependentBayPhase::WaitingForManifold | IndependentBayPhase::Transferring
                ) {
                    phase(
                        &mut s,
                        id,
                        IndependentBayPhase::Coupling,
                        now,
                        "Contact confirmed",
                    );
                    wait_for_manifold(&mut s.state, id, now)?;
                }
            } else if !s.runs[&id].cleanup_only {
                let next = match &s.runs[&id].movement {
                    Movement::Entry(r) => r.frames[r.target_index].phase,
                    Movement::Handling(r) => r.route[r.target_index].phase,
                };
                phase(
                    &mut s,
                    id,
                    if next == IndependentBayPhase::WaitingForManifold {
                        IndependentBayPhase::Coupling
                    } else {
                        next
                    },
                    now,
                    "Advancing accepted physical route",
                );
            }
            s.mechanisms.insert(
                view.bay_id,
                view.mechanisms
                    .iter()
                    .map(|m| (m.semantic_id.clone(), m.pose.confirmed))
                    .collect(),
            );
            place(w, &name, &view, now)?;
        }
        w.accounts.get_mut(&name).unwrap().independent_bays = Some(s);
        if defer != Some(name.as_str()) {
            admit(w, &name, now)?;
        }
    }
    Ok(())
}
/// Called AFTER simultaneous incoming damage and cargo destruction sinks, but
/// before the legacy stock finalizer removes dead objects. No visual timer owns
/// storage or ore. Every ownership transition remains in the enclosing SQLite tx.
pub(crate) fn finish(w: &mut World, now: u64) -> Result<()> {
    let Some(def) = definition(w)? else {
        return Ok(());
    };
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let Some(mut s) = state(w, &name).cloned() else {
            continue;
        };
        for id in s.runs.keys().copied().collect::<Vec<_>>() {
            let op = s.state.operation(id).unwrap().clone();
            let dead = w.pilots[&name]
                .view
                .drones
                .iter()
                .any(|d| d.id == op.drone_id && d.hit_points == 0);
            if dead && !s.runs[&id].cleanup_only {
                if s.state
                    .manifold
                    .grant
                    .as_ref()
                    .is_some_and(|g| g.physical_operation_id == id)
                {
                    transfers::close_attempt(w, &name, &mut s, id, now, false)?;
                }
                s.state.manifold.waiting.retain(|v| *v != id);
                let run = s.runs.get_mut(&id).unwrap();
                let held = run.view(op.phase)?.phase == IndependentBayPhase::Held;
                run.cleanup_only = true;
                run.cancel_requested = true;
                // Continue only the already accepted safe hardware route, with
                // no ghost body, no pumping and no subsequent capture.
                phase(
                    &mut s,
                    id,
                    IndependentBayPhase::Destroyed,
                    now,
                    "Destroyed; securing empty hardware",
                );
                let lane = &mut s.state.lanes[usize::from(op.assigned_bay.unwrap())];
                lane.operation_id = None;
                lane.cleanup = Some(IndependentBayCleanup {
                    operation_id: id,
                    drone_id: op.drone_id,
                    held,
                    reason: "Empty hardware must finish securing".into(),
                });
            }
            if !s.runs[&id].ready() {
                continue;
            }
            let run = s.runs[&id].clone();
            let view = run.view(op.phase)?;
            if let Movement::Entry(entry) = run.movement {
                if run.cleanup_only {
                    s.runs.remove(&id);
                    s.state.lanes[usize::from(view.bay_id)].cleanup = None;
                    continue;
                }
                let start = IndependentBayMotionStart {
                    binding: s.state.binding.clone(),
                    operation_id: id,
                    drone_id: op.drone_id,
                    bay_id: view.bay_id,
                    action: entry.action,
                    parent: entry.parent,
                    system_id: entry.system_id,
                    class: entry.class,
                    actual_body_pose: entry.confirmed,
                    actual_mechanism_poses: s.mechanisms[&view.bay_id].clone(),
                    now,
                };
                let handling = IndependentBayRuntime::from_route(
                    start,
                    def,
                    def.route(view.bay_id, entry.action).unwrap(),
                )?;
                let next = handling.route[handling.target_index].phase;
                s.runs.get_mut(&id).unwrap().movement = Movement::Handling(handling);
                phase(
                    &mut s,
                    id,
                    if next == IndependentBayPhase::WaitingForManifold {
                        IndependentBayPhase::Coupling
                    } else {
                        next
                    },
                    now,
                    "Exterior alignment complete; registered handling begins",
                );
                continue;
            }
            let Movement::Handling(handling) = run.movement else {
                unreachable!()
            };
            ensure!(
                handling.reached.hardware_safe,
                "Physical finalization requires confirmed safe hardware"
            );
            if !run.cleanup_only && handling.route_action == IndependentBayAction::ReturnToStorage {
                ensure!(
                    handling.reached.supported
                        && handling.reached.clamped
                        && drone_cargo::payload(w, &name, op.drone_id) == 0,
                    "Storage requires supported clamped empty same entity"
                );
                let fleet = &mut w.pilots.get_mut(&name).unwrap().view.drones;
                let index = fleet
                    .iter()
                    .position(|d| d.id == op.drone_id && d.hit_points > 0)
                    .context("Capture requires living same identity")?;
                let drone = fleet.remove(index);
                let stock = w
                    .accounts
                    .get_mut(&name)
                    .unwrap()
                    .drone_bays
                    .get_mut(&s.state.binding.vessel_id)
                    .unwrap();
                stock
                    .stock
                    .iter_mut()
                    .find(|e| e.drone_id == op.drone_id)
                    .unwrap()
                    .status = DroneStockStatus::Stored {
                    hit_points: drone.hit_points,
                };
                s.state.lanes[usize::from(view.bay_id)].stored_drone_id = Some(op.drone_id);
                phase(
                    &mut s,
                    id,
                    IndependentBayPhase::Stored,
                    now,
                    "Same drone captured, clamped and secured",
                );
            } else if !run.cleanup_only
                && op.action == IndependentBayAction::ReturnToStorage
                && !run.cancel_requested
            {
                ensure!(
                    drone_cargo::payload(w, &name, op.drone_id) == 0,
                    "Unload prefix did not clear payload"
                );
                let next = start_run(
                    w,
                    &name,
                    &s,
                    id,
                    view.bay_id,
                    IndependentBayAction::ReturnToStorage,
                    now,
                )?;
                s.runs.insert(id, next);
                phase(
                    &mut s,
                    id,
                    IndependentBayPhase::Approaching,
                    now,
                    "Payload delivered; entering full storage route",
                );
                continue;
            } else if !run.cleanup_only {
                ensure!(
                    handling.reached.exterior_clear,
                    "Release requires full body exterior clearance"
                );
                phase(
                    &mut s,
                    id,
                    if run.cancel_requested {
                        IndependentBayPhase::Cancelled
                    } else {
                        IndependentBayPhase::Free
                    },
                    now,
                    "Same drone outside; hardware secured",
                );
            }
            if !run.cleanup_only
                && !run.cancel_requested
                && matches!(
                    op.action,
                    IndependentBayAction::ReturnToUnload {
                        resume_mining: true
                    }
                )
            {
                let intent = drone_cargo::state(w, &name)
                    .and_then(|c| c.holds.iter().find(|h| h.drone_id == op.drone_id))
                    .and_then(|h| {
                        h.mining_intent
                            .as_ref()
                            .filter(|i| i.generation == h.intent_generation)
                    })
                    .cloned();
                if let Some(intent) = intent.filter(|i| {
                    i.job_id.is_none_or(|job| {
                        w.resource_jobs
                            .get(&name)
                            .is_some_and(|j| j.view.id == job && !j.view.phase.is_terminal())
                    })
                }) {
                    if w.pilots[&name]
                        .view
                        .locked_asteroids
                        .contains(&intent.target_id)
                        && w.asteroids.iter().any(|a| {
                            a.id == intent.target_id
                                && a.ore > 0
                                && a.system_id == w.pilots[&name].view.system_id
                        })
                    {
                        if let Some(d) = w
                            .pilots
                            .get_mut(&name)
                            .unwrap()
                            .view
                            .drones
                            .iter_mut()
                            .find(|d| d.id == op.drone_id && d.hit_points > 0)
                        {
                            d.mode = DroneMode::Mining {
                                target_id: intent.target_id,
                            };
                            d.updated_at_ms = now;
                        }
                    }
                }
            }
            s.runs.remove(&id);
            let lane = &mut s.state.lanes[usize::from(view.bay_id)];
            lane.operation_id = None;
            lane.cleanup = None;
        }
        w.accounts.get_mut(&name).unwrap().independent_bays = Some(s);
        drones::sync(w.pilots.get_mut(&name).unwrap());
    }
    Ok(())
}
pub(crate) fn authorize(w: &World, name: &str, request: &Request) -> Result<()> {
    let Some(_s) = state(w, name) else {
        return Ok(());
    };
    match request {
        Request::Snapshot
        | Request::ExportLedger
        | Request::Connect { .. }
        | Request::IndependentBays { .. } => (),
        Request::Navigate {
            mode: NavigationMode::Stop,
        } => (),
        Request::DroneBay { .. }
        | Request::DroneCargo { .. }
        | Request::LaunchDrone
        | Request::LaunchDroneClass { .. }
        | Request::RecallDrone
        | Request::RecallDroneById { .. } => {
            bail!("Use identified independent bay actions in this fresh fixture")
        }
        Request::Warp { .. }
        | Request::Jump { .. }
        | Request::Dock
        | Request::Undock
        | Request::SelectShip { .. }
        | Request::BoardShip { .. }
        | Request::StartResourceJob { .. } => {
            bail!("Stationary independent source fixture: travel and routines not admitted")
        }
        Request::Navigate { .. } | Request::PilotInput { .. } => {
            bail!("Independent bay operation owns stationary carrier")
        }
        Request::DroneNavigateById { id, .. }
        | Request::DroneHoldById { id }
        | Request::AssignMiningDrone { id, .. }
        | Request::AssignAttackDrone { id, .. }
        | Request::OrbitDrone { id, .. }
            if constrained(w, name, *id) =>
        {
            bail!("Identified independent operation owns this drone")
        }
        Request::DroneNavigate { .. } | Request::DroneHold
            if w.pilots[name]
                .view
                .drones
                .first()
                .is_some_and(|d| constrained(w, name, d.id)) =>
        {
            bail!("Independent operation owns the legacy selected drone")
        }
        _ => (),
    }
    Ok(())
}
pub(crate) fn validate(w: &World) -> Result<()> {
    let Some(def) = definition(w)? else {
        ensure!(
            w.accounts.values().all(|a| a.independent_bays.is_none()),
            "Unstamped world cannot contain independent state"
        );
        return Ok(());
    };
    ensure!(
        w.drone_cargo_registration.as_ref() == Some(&cargo_registration(def)),
        "Independent cargo must bind actual assembly source, not old receiver"
    );
    for (name, a) in &w.accounts {
        let s = state(w, name).context("Missing finite independent initialization")?;
        let flight = w
            .pilots
            .get(name)
            .and_then(|p| p.view.flight.as_ref())
            .context("Stationary fixture carrier missing")?;
        ensure!(
            flight.mode == NavigationMode::Stop
                && flight.velocity == [0.; 3]
                && flight.acceleration == [0.; 3]
                && flight.angular_velocity == [0.; 3],
            "Stationary fixture carrier cannot move"
        );
        let stock = drone_bay::state(w, name).context("Missing canonical stock")?;
        ensure!(
            s.state.binding.registration == def.registration
                && s.state.binding.vessel_id == stock.binding.vessel_id
                && s.state.validate_shape(&stock.stock),
            "Independent ownership/source invalid"
        );
        ensure!(
            a.drone_bay_runtime.is_empty()
                && a.drone_receivers.is_empty()
                && stock.handling_operation.is_none()
                && stock.cleanup.is_none(),
            "Independent authority cannot share legacy physical runtimes"
        );
        ensure!(
            s.mechanisms.len() == def.bays.len(),
            "Missing lane confirmed roots"
        );
        for bay in &def.bays {
            let roots = s
                .mechanisms
                .get(&bay.bay_id)
                .context("Missing confirmed lane")?;
            let lane = &s.state.lanes[usize::from(bay.bay_id)];
            if lane.operation_id.is_none() && lane.cleanup.is_none() {
                ensure!(
                    *roots
                        == def.route(bay.bay_id, IndependentBayAction::Launch).unwrap()[0]
                            .mechanism_poses,
                    "Idle hardware differs from source safe rest"
                );
            }
            ensure!(
                roots.len() == bay.mechanisms.len()
                    && roots
                        .iter()
                        .zip(&bay.mechanisms)
                        .all(|((id, p), m)| *id == m.semantic_id && p.valid()),
                "Lane group identity invalid"
            );
        }
        for (id, run) in &s.runs {
            let op = s
                .state
                .operation(*id)
                .context("Orphan independent runtime")?;
            let host = parent(w, name)?;
            let entry = stock
                .stock
                .iter()
                .find(|e| e.drone_id == op.drone_id)
                .unwrap();
            let (binding, class, action) = match &run.movement {
                Movement::Entry(r) => (&r.binding, r.class, r.action),
                Movement::Handling(r) => (&r.binding, r.class, r.route_action),
            };
            ensure!(
                *binding == s.state.binding
                    && class == entry.class
                    && (action == op.action
                        || (op.action == IndependentBayAction::ReturnToStorage
                            && action
                                == IndependentBayAction::ReturnToUnload {
                                    resume_mining: false
                                })),
                "Runtime action/class/binding mismatch"
            );
            if let Movement::Entry(r) = &run.movement {
                ensure!(
                    r.static_obstacles == static_obstacles(w, name, host)?,
                    "Entry static obstacles differ from actual catalog"
                );
            }
            match &run.movement {
                Movement::Entry(r) => r.validate_definition(
                    def,
                    &def.route(r.bay_id, r.action)
                        .context("Source entry unavailable")?,
                    host,
                    w.pilots[name].view.system_id,
                )?,
                Movement::Handling(r) => r.validate_expected(
                    &def.route(r.bay_id, r.route_action)
                        .context("Source handling unavailable")?,
                    host,
                    w.pilots[name].view.system_id,
                )?,
            }
            let v = run.view(op.phase)?;
            let lane = &s.state.lanes[usize::from(v.bay_id)];
            ensure!(
                run.cleanup_only
                    == lane
                        .cleanup
                        .as_ref()
                        .is_some_and(|c| c.operation_id == *id && c.drone_id == op.drone_id),
                "Cleanup runtime/owner mismatch"
            );
            if run.cleanup_only {
                ensure!(
                    op.phase == IndependentBayPhase::Destroyed
                        && lane.operation_id.is_none()
                        && !w.pilots[name]
                            .view
                            .drones
                            .iter()
                            .any(|d| d.id == op.drone_id)
                        && lane.cleanup.as_ref().unwrap().held
                            == (v.phase == IndependentBayPhase::Held),
                    "Destroyed cleanup differs from physical ownership"
                );
            } else {
                ensure!(
                    !op.phase.terminal()
                        && lane.operation_id == Some(*id)
                        && (op.phase == IndependentBayPhase::Held)
                            == (v.phase == IndependentBayPhase::Held),
                    "Live lane receipt/hold mismatch"
                );
            }
            ensure!(
                v.operation_id == *id
                    && v.drone_id == op.drone_id
                    && Some(v.bay_id) == op.assigned_bay
                    && s.mechanisms[&v.bay_id]
                        == v.mechanisms
                            .iter()
                            .map(|m| (m.semantic_id.clone(), m.pose.confirmed))
                            .collect::<Vec<_>>(),
                "Runtime/public group identity mismatch"
            );
            if !run.cleanup_only {
                let d = w.pilots[name]
                    .view
                    .drones
                    .iter()
                    .find(|d| d.id == op.drone_id)
                    .context("Runtime drone missing")?;
                let pose = host
                    .compose(v.drone.confirmed)
                    .context("Invalid actual drone pose")?;
                ensure!(
                    d.position == pose.position_m
                        && d.orientation_xyzw == Some(pose.orientation_xyzw)
                        && d.mode == DroneMode::Hold,
                    "Actual drone differs from accepted constrained pose"
                );
            }
        }
        ensure!(
            s.state.lanes.iter().all(|l| l
                .operation_id
                .or_else(|| l.cleanup.as_ref().map(|c| c.operation_id))
                .is_none_or(|id| s.runs.contains_key(&id))),
            "Lane lost its actionable route"
        );
    }
    Ok(())
}

pub(crate) fn begin_automatic(w: &mut World, name: &str, drone: u64, now: u64) -> Result<()> {
    let s = state(w, name).context("Independent auto service state missing")?;
    if s.state.active_operation(drone).is_some() {
        return Ok(());
    }
    let request = IndependentBayRequest::Begin {
        binding: s.state.binding.clone(),
        operation_id: s
            .state
            .next_operation_id()
            .context("Independent operation IDs exhausted")?,
        drone_id: drone,
        action: IndependentBayAction::ReturnToUnload {
            resume_mining: true,
        },
    };
    execute(w, name, &request, now)
}
/// Refuse unknown or malformed stamped saves before any realm/schema/clock write.
/// Legacy unstamped worlds retain their existing open path.
pub(crate) fn preflight_existing(db: &rusqlite::Connection) -> Result<()> {
    use rusqlite::OptionalExtension;
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='world')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let json: Option<String> = db
        .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
        .optional()?;
    let Some(json) = json else {
        return Ok(());
    };
    let value: serde_json::Value = serde_json::from_str(&json)?;
    if value
        .get("independent_bay_registration")
        .is_none_or(serde_json::Value::is_null)
        && !value
            .get("accounts")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|accounts| {
                accounts.values().any(|account| {
                    account
                        .get("independent_bays")
                        .is_some_and(|state| !state.is_null())
                })
            })
    {
        return Ok(());
    }
    let world: World = serde_json::from_value(value)?;
    definition(&world)?;
    drone_bay::validate(&world)?;
    drone_cargo::validate(&world)?;
    validate(&world)?;
    if !world.pilots.is_empty() {
        validate_history(&world, db)?;
    }
    Ok(())
}
pub(crate) fn validate_history(w: &World, db: &rusqlite::Connection) -> Result<()> {
    if w.independent_bay_registration.is_none() {
        return Ok(());
    }
    let mut query = db.prepare("SELECT json FROM ledger ORDER BY sequence")?;
    let entries = query
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|r| Ok(serde_json::from_str::<LedgerEntry>(&r?)?))
        .collect::<Result<Vec<_>>>()?;
    for name in w.pilots.keys() {
        let s = state(w, name).context("Missing independent history")?;
        validate_manifold_history(
            name,
            &s.state,
            drone_cargo::state(w, name).context("Missing cargo history")?,
            &entries,
        )?;
    }
    Ok(())
}
