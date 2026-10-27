use super::*;
use crate::drone_bay::runtime::{self as shared, CarrierOrigins, ExternalRouteCheck};
use arkline_protocol::{
    drone_bay_geometry::{BayMotionSegment, BayPose},
    drone_bay_layout as bay, drone_dropoff_geometry as geometry,
};
const MAX_GAP_MS: u64 = 2000;
const HANDLING_SPEED_M_S: f64 = 2.;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Frame {
    drone: BayPose,
    groups: [BayPose; 5],
}
impl Frame {
    fn sample(&self, to: &Self, start: u64, duration: u64, now: u64) -> Result<Self> {
        let sample = |source, target| {
            BayMotionSegment {
                source,
                target,
                started_at_ms: start,
                duration_ms: duration,
            }
            .sample(now)
            .map(|v| v.0)
            .context("receiver segment invalid")
        };
        let mut groups = self.groups;
        for (i, g) in groups.iter_mut().enumerate() {
            *g = sample(self.groups[i], to.groups[i])?;
        }
        Ok(Self {
            drone: sample(self.drone, to.drone)?,
            groups,
        })
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Runtime {
    operation_id: u64,
    drone_id: u64,
    parent: BayPose,
    system_id: u32,
    points: Vec<Frame>,
    phases: Vec<DroneDropoffPhase>,
    index: usize,
    entry: Frame,
    route_origin: Frame,
    source: Frame,
    confirmed: Frame,
    started_at_ms: u64,
    duration_ms: u64,
    updated_at_ms: u64,
    confirmed_at_ms: u64,
    held: bool,
    cleanup: bool,
    cancelling: bool,
    ready: bool,
}
impl Runtime {
    pub(crate) fn constrains(&self, id: u64) -> bool {
        self.drone_id == id && !self.cleanup
    }
}
fn runtime<'a>(w: &'a World, name: &str) -> Option<&'a Runtime> {
    let id = &state(w, name)?.binding.vessel_id;
    w.accounts.get(name)?.drone_receivers.get(id)
}
fn runtime_mut<'a>(w: &'a mut World, name: &str) -> Option<&'a mut Runtime> {
    let id = state(w, name)?.binding.vessel_id.clone();
    w.accounts.get_mut(name)?.drone_receivers.get_mut(&id)
}
pub(crate) fn busy(w: &World, name: &str) -> bool {
    runtime(w, name).is_some()
}
pub(crate) fn constrained(w: &World, name: &str, id: u64) -> bool {
    state(w, name).is_some_and(|s| s.operations.iter().any(|o| o.drone_id == id && o.active()))
        || runtime(w, name).is_some_and(|r| r.constrains(id))
}
fn op_mut<'a>(w: &'a mut World, name: &str, id: u64) -> Result<&'a mut DroneDropoffOperation> {
    state_mut(w, name)?
        .operations
        .iter_mut()
        .find(|o| o.operation_id == id)
        .context("receiver receipt missing")
}
fn set_phase(
    w: &mut World,
    name: &str,
    id: u64,
    phase: DroneDropoffPhase,
    now: u64,
    message: &str,
) -> Result<()> {
    let op = op_mut(w, name, id)?;
    op.phase = phase;
    op.held_phase = None;
    op.updated_at_ms = now;
    op.message = message.into();
    if phase != DroneDropoffPhase::Transferring {
        op.next_batch_at_ms = None;
    }
    Ok(())
}
fn groups(config: geometry::DropoffConfiguration) -> [BayPose; 5] {
    geometry::mechanism_poses(config).expect("fixed receiver configuration")
}
fn frame(drone: BayPose, config: geometry::DropoffConfiguration) -> Frame {
    Frame {
        drone,
        groups: groups(config),
    }
}
fn duration(a: &Frame, b: &Frame, phase: DroneDropoffPhase) -> Result<u64> {
    let distance = std::iter::once(flight::distance(a.drone.position_m, b.drone.position_m))
        .chain(
            a.groups
                .iter()
                .zip(b.groups)
                .map(|(a, b)| flight::distance(a.position_m, b.position_m)),
        )
        .fold(0f64, f64::max);
    let speed = if phase == DroneDropoffPhase::Approaching {
        drone_speed(DroneClass::Mining).min(40.)
    } else {
        HANDLING_SPEED_M_S
    };
    let ms = (distance * 1500. / speed).ceil().max(750.);
    ensure!(
        ms.is_finite() && ms <= 86_400_000.,
        "receiver route duration out of bounds"
    );
    Ok(ms as u64)
}
fn intersects(a: [f64; 3], b: [f64; 3], min: [f64; 3], max: [f64; 3]) -> bool {
    let (mut lo, mut hi) = (0f64, 1f64);
    for i in 0..3 {
        let d = b[i] - a[i];
        if d.abs() < 1e-12 {
            if a[i] < min[i] || a[i] > max[i] {
                return false;
            }
        } else {
            let x = (min[i] - a[i]) / d;
            let y = (max[i] - a[i]) / d;
            lo = lo.max(x.min(y));
            hi = hi.min(x.max(y));
            if lo > hi {
                return false;
            }
        }
    }
    true
}
/// Visibility graph is wholly outside the full hull plus stowed receiver and a
/// circumscribed drone body. Only the registered gate→contact segment enters its
/// scoped aft socket corridor, at the exact authored full attitude.
fn approach(
    w: &World,
    name: &str,
    parent: BayPose,
    start: BayPose,
    requesting_drone: u64,
) -> Result<Vec<BayPose>> {
    let definition = shared::definition(w, name)?;
    let radius = definition
        .drone_half_extents_m
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
        + 0.1;
    let bounds = geometry::DROPOFF_MODULE_STOWED_BOUNDS_M;
    let min =
        std::array::from_fn(|i| definition.carrier_envelope.min_m[i].min(bounds[0][i]) - radius);
    let max =
        std::array::from_fn(|i| definition.carrier_envelope.max_m[i].max(bounds[1][i]) + radius);
    ensure!(
        !intersects(start.position_m, start.position_m, min, max),
        "move drone clear of carrier/receiver before drop-off"
    );
    let end = geometry::gate_pose();
    let mut nodes = vec![start, end];
    for corner in 0..8 {
        nodes.push(BayPose {
            position_m: std::array::from_fn(|i| {
                if corner & (1 << i) == 0 {
                    min[i] - 1.
                } else {
                    max[i] + 1.
                }
            }),
            ..end
        });
    }
    let inverse = shared::inverse(parent)?;
    let end = geometry::gate_pose();
    let system = w.pilots[name].view.system_id;
    // Static/catalog hazards and sampled peer bodies inform route planning;
    // every accepted segment is still checked against conservative same-tick
    // dynamic reach. The graph is not a global free-flight physics guarantee.
    let mut hazards: Vec<([f64; 3], f64)> = w
        .asteroids
        .iter()
        .filter(|a| a.system_id == system)
        .map(|a| (a.position, a.radius))
        .collect();
    for (other, p) in &w.pilots {
        if p.view.system_id != system {
            continue;
        }
        if other != name
            && !matches!(
                p.view.location,
                Location::InTransit { .. } | Location::Jumping { .. }
            )
        {
            if let Some(f) = &p.view.flight {
                hazards.push((f.position, hull_radius_m(p.view.ship_class)));
            }
        }
        // Exclude only the same owned identity, not a rounded coordinate match.
        for d in &p.view.drones {
            if !(other == name && d.id == requesting_drone) {
                hazards.push((d.position, radius - 0.1));
            }
        }
    }
    hazards.push((STATION_CENTER_POSITION, STATION_DIAMETER_M / 2.));
    let planet = w.planet();
    if planet.system_id == system {
        hazards.push((planet.center, planet.radius_m));
    }
    let obstacles: Vec<_> = hazards
        .into_iter()
        .map(|(position, r)| {
            let p = inverse
                .compose(BayPose {
                    position_m: position,
                    orientation_xyzw: [0., 0., 0., 1.],
                })
                .unwrap()
                .position_m;
            (p, r + radius)
        })
        .collect();
    let sphere_hits = |a: [f64; 3], b: [f64; 3], center: [f64; 3], radius: f64| {
        let delta: [f64; 3] = std::array::from_fn(|i| b[i] - a[i]);
        let length = delta.iter().map(|v| v * v).sum::<f64>();
        let t = if length > 1e-12 {
            (0..3).map(|i| (center[i] - a[i]) * delta[i]).sum::<f64>() / length
        } else {
            0.
        }
        .clamp(0., 1.);
        (0..3)
            .map(|i| (center[i] - a[i] - t * delta[i]).powi(2))
            .sum::<f64>()
            <= radius * radius
    };
    for (center, r) in &obstacles {
        if *r > 5000.
            || (flight::distance(*center, start.position_m) > 5000. + r
                && flight::distance(*center, end.position_m) > 5000. + r)
        {
            continue;
        }
        for corner in 0..8 {
            nodes.push(BayPose {
                position_m: std::array::from_fn(|i| {
                    center[i]
                        + if corner & (1 << i) == 0 {
                            -r - 1.
                        } else {
                            r + 1.
                        }
                }),
                ..end
            });
        }
    }
    let edge_clear = |a: [f64; 3], b: [f64; 3]| {
        !intersects(a, b, min, max)
            && !obstacles
                .iter()
                .any(|(center, r)| sphere_hits(a, b, *center, *r))
    };
    let n = nodes.len();
    let mut costs = vec![f64::INFINITY; n];
    let mut prev = vec![None; n];
    let mut done = vec![false; n];
    costs[0] = 0.;
    for _ in 0..n {
        let Some(u) = (0..n)
            .filter(|i| !done[*i])
            .min_by(|a, b| costs[*a].total_cmp(&costs[*b]))
        else {
            break;
        };
        if !costs[u].is_finite() {
            break;
        }
        done[u] = true;
        for v in 0..n {
            if !done[v] && edge_clear(nodes[u].position_m, nodes[v].position_m) {
                let c = costs[u] + flight::distance(nodes[u].position_m, nodes[v].position_m);
                if c < costs[v] {
                    costs[v] = c;
                    prev[v] = Some(u);
                }
            }
        }
    }
    ensure!(costs[1].is_finite(), "no clear receiver approach");
    let mut path = vec![1];
    while *path.last().unwrap() != 0 {
        path.push(prev[*path.last().unwrap()].context("receiver approach graph broken")?);
    }
    path.reverse();
    Ok(path.into_iter().map(|i| nodes[i]).collect())
}
fn clear_pose(home: u8) -> BayPose {
    BayPose {
        position_m: [
            -60. - 12. * f64::from(home % 3),
            -1. + 12. * f64::from(home / 3),
            17.,
        ],
        ..geometry::gate_pose()
    }
}
fn ready(w: &World, name: &str, manual: bool) -> Result<BayPose> {
    ensure!(
        w.drone_cargo_registration.as_ref() == Some(&registration()?),
        "receiver source unsupported"
    );
    let parent = shared::parent(w, name)?;
    ensure!(
        !drone_bay::busy(w, name),
        "storage handling owns the nearby route; wait for secure machinery"
    );
    if manual {
        ensure!(
            w.resource_jobs
                .get(name)
                .is_none_or(|j| j.view.phase.is_terminal()),
            "cancel the active resource job before manual ore service"
        );
    }
    Ok(parent)
}
fn check(
    w: &World,
    name: &str,
    run: &Runtime,
    now: u64,
    origins: Option<&CarrierOrigins>,
) -> Result<()> {
    ensure!(
        shared::parent(w, name)? == run.parent && w.pilots[name].view.system_id == run.system_id,
        "carrier pose/system changed; receiver held"
    );
    ensure!(
        !drone_bay::busy(w, name),
        "storage handling conflicts with receiver route"
    );
    let target = run.points.get(run.index).unwrap_or(&run.confirmed);
    let bounds = geometry::DROPOFF_MODULE_WORKING_BOUNDS_M;
    shared::check_external_route(
        w,
        name,
        ExternalRouteCheck {
            drone: run.drone_id,
            parent: run.parent,
            source: run.confirmed.drone,
            target: target.drone,
            working: bay::BayEnvelope {
                min_m: bounds[0],
                max_m: bounds[1],
            },
            moving_hardware: run.confirmed.groups != target.groups,
            now,
            lookback_ms: now.saturating_sub(run.updated_at_ms),
            carriers: origins,
            has_drone: !run.cleanup,
        },
    )
}
fn begin(
    w: &mut World,
    name: &str,
    id: u64,
    drone: u64,
    followup: DroneDropoffFollowup,
    now: u64,
    manual: bool,
) -> Result<()> {
    let resume = matches!(followup, DroneDropoffFollowup::ResumeMining { .. });
    let s = state(w, name).context("cargo unavailable in this world")?;
    if let Some(old) = s.operation(id) {
        ensure!(
            old.drone_id == drone && old.requested_resume_mining == resume,
            "drop-off ID reused with different intent"
        );
        return Ok(());
    }
    ensure!(
        s.validated_next_id() == Some(id),
        "new drop-off ID must be confirmed high watermark plus one"
    );
    ensure!(
        !s.operations
            .iter()
            .any(|o| o.drone_id == drone && o.active()),
        "drone already has a receiver operation"
    );
    if manual {
        ensure!(
            !drone_bay::batch_active(w, name),
            "Recall All owns this drone; use its batch controls"
        );
    }
    if manual {
        ready(w, name, true)?;
    }
    let h = s
        .holds
        .iter()
        .find(|h| h.drone_id == drone)
        .context("only the owned Mining roster has ore holds")?;
    ensure!(
        h.ore_units > 0 && h.destroyed_at_ms.is_none(),
        "drone has no transferable ore"
    );
    ensure!(
        w.pilots[name]
            .view
            .drones
            .iter()
            .any(|d| d.id == drone && d.hit_points > 0),
        "loaded drone must be active"
    );
    ensure!(
        !drone_bay::constrained(w, name, drone),
        "storage handling owns this drone"
    );
    let units = h.ore_units;
    let binding = s.binding.clone();
    let s = state_mut(w, name)?;
    s.high_watermark = id;
    s.holds
        .iter_mut()
        .find(|h| h.drone_id == drone)
        .unwrap()
        .cycle = None;
    s.operations.push(DroneDropoffOperation {
        operation_id: id,
        binding,
        drone_id: drone,
        requested_resume_mining: resume,
        cancel_requested: false,
        followup,
        requested_units: units,
        committed_units: 0,
        remaining_units: units,
        phase: DroneDropoffPhase::Queued,
        held_phase: None,
        accepted_at_ms: now,
        updated_at_ms: now,
        next_batch_at_ms: None,
        message: "Queued for the single ore receiver".into(),
        receipts: vec![],
    });
    let p = w.pilots.get_mut(name).unwrap();
    let d = p.view.drones.iter_mut().find(|d| d.id == drone).unwrap();
    d.mode = DroneMode::Hold;
    d.velocity = [0.; 3];
    d.mining_cycle = None;
    d.attack_ready_at_ms = None;
    d.updated_at_ms = now;
    drones::sync(p);
    admit_next(w, name, now)
}
pub(crate) fn begin_automatic(
    w: &mut World,
    name: &str,
    drone: u64,
    batch: Option<u64>,
    now: u64,
) -> Result<u64> {
    let s = state(w, name).context("cargo state missing")?;
    if let Some(old) = s
        .operations
        .iter()
        .find(|o| o.drone_id == drone && o.active())
    {
        ensure!(
            batch.is_none()
                || old.followup
                    == DroneDropoffFollowup::RecallAll {
                        batch_id: batch.unwrap()
                    },
            "drone already has another service intent"
        );
        return Ok(old.operation_id);
    }
    let followup = if let Some(batch_id) = batch {
        DroneDropoffFollowup::RecallAll { batch_id }
    } else if let Some(intent) = s
        .holds
        .iter()
        .find(|h| h.drone_id == drone)
        .and_then(|h| h.mining_intent.clone())
    {
        DroneDropoffFollowup::ResumeMining {
            target_id: intent.target_id,
            job_id: intent.job_id,
            intent_generation: intent.generation,
        }
    } else {
        DroneDropoffFollowup::Hold
    };
    let id = s.validated_next_id().context("drop-off ID unavailable")?;
    begin(w, name, id, drone, followup, now, false)?;
    Ok(id)
}
fn admit_next(w: &mut World, name: &str, now: u64) -> Result<()> {
    if busy(w, name) || drone_bay::busy(w, name) {
        // An accepted storage stroke is ordinary queue contention, not a
        // failed approach. Do not trap an automatic payload in Held.
        return Ok(());
    }
    let Some(op) = state(w, name)
        .and_then(|s| s.operations.iter().find(|o| o.active()))
        .cloned()
    else {
        return Ok(());
    };
    if op.phase != DroneDropoffPhase::Queued {
        return Ok(());
    }
    let prepared = (|| -> Result<Runtime> {
        let parent = ready(w, name, false)?;
        let p = &w.pilots[name];
        let d = p
            .view
            .drones
            .iter()
            .find(|d| d.id == op.drone_id && d.hit_points > 0)
            .context("receiver drone missing")?;
        let pose = BayPose {
            position_m: d.position,
            orientation_xyzw: d.orientation_xyzw.context("full drone attitude required")?,
        };
        let start = shared::inverse(parent)?
            .compose(pose)
            .context("invalid receiver local pose")?;
        let path = approach(w, name, parent, start, op.drone_id)?;
        let mut points: Vec<_> = path
            .into_iter()
            .map(|p| frame(p, geometry::DropoffConfiguration::STOWED))
            .collect();
        let mut phases = vec![DroneDropoffPhase::Approaching; points.len()];
        let home = drone_bay::state(w, name)
            .unwrap()
            .stock
            .iter()
            .find(|e| e.drone_id == op.drone_id)
            .unwrap()
            .home_cradle;
        let (tail, tail_phases) = service_tail();
        points.extend(tail);
        phases.extend(tail_phases);
        // Gate-to-parking fan-out must avoid miners already parked by earlier
        // services. Reverse the same conservative exterior visibility route;
        // all later samples still recheck dynamic obstacles before movement.
        let egress = approach(w, name, parent, clear_pose(home), op.drone_id)?;
        for pose in egress.into_iter().rev().skip(1) {
            points.push(frame(pose, geometry::DropoffConfiguration::STOWED));
            phases.push(DroneDropoffPhase::Egress);
        }
        let source = points.remove(0);
        phases.remove(0);
        let duration_ms = duration(&source, &points[0], phases[0])?;
        let run = Runtime {
            operation_id: op.operation_id,
            drone_id: op.drone_id,
            parent,
            system_id: p.view.system_id,
            points,
            phases,
            index: 0,
            entry: source.clone(),
            route_origin: source.clone(),
            source: source.clone(),
            confirmed: source,
            started_at_ms: now,
            duration_ms,
            updated_at_ms: now,
            confirmed_at_ms: now,
            held: false,
            cleanup: false,
            cancelling: false,
            ready: false,
        };
        check(w, name, &run, now, None)?;
        Ok(run)
    })();
    match prepared {
        Ok(run) => {
            let phase = run.phases[0];
            let id = state(w, name).unwrap().binding.vessel_id.clone();
            w.accounts
                .get_mut(name)
                .unwrap()
                .drone_receivers
                .insert(id, run);
            state_mut(w, name)?.receiver_operation = Some(op.operation_id);
            set_phase(w, name, op.operation_id, phase, now, phase.label())?;
        }
        Err(error) => {
            let op = op_mut(w, name, op.operation_id)?;
            op.phase = DroneDropoffPhase::Held;
            op.held_phase = Some(DroneDropoffPhase::Queued);
            op.updated_at_ms = now;
            op.message = format!("Ore service held before approach: {error}");
        }
    }
    Ok(())
}
pub(crate) fn execute(
    w: &mut World,
    name: &str,
    request: &DroneCargoRequest,
    now: u64,
    automated: bool,
) -> Result<()> {
    execute_control(w, name, request, now, automated, false)
}
/// Only the owning Recall All queue can control its accepted service member.
pub(crate) fn execute_queued(
    w: &mut World,
    name: &str,
    request: &DroneCargoRequest,
    now: u64,
) -> Result<()> {
    execute_control(w, name, request, now, false, true)
}
fn execute_control(
    w: &mut World,
    name: &str,
    request: &DroneCargoRequest,
    now: u64,
    automated: bool,
    batch_control: bool,
) -> Result<()> {
    let s = state(w, name).context("ore drop-off requires the fresh receiver preview")?;
    ensure!(
        &s.binding == request.binding() && s.binding.registration == registration()?,
        "receiver command names a different owned vessel or source"
    );
    if !batch_control && !matches!(request, DroneCargoRequest::BeginDropoff { .. }) {
        let id = match request {
            DroneCargoRequest::Hold { operation_id, .. }
            | DroneCargoRequest::Resume { operation_id, .. }
            | DroneCargoRequest::Cancel { operation_id, .. } => *operation_id,
            _ => unreachable!(),
        };
        if let Some(op) = s.operation(id) {
            let cleanup = s.cleanup.as_ref().is_some_and(|c| c.operation_id == id);
            if op.active() || cleanup {
                ensure!(!matches!(op.followup, DroneDropoffFollowup::RecallAll { .. }), "Recall All owns this service; use its Hold, Resume or Cancel Remaining controls");
            }
        }
    }
    match request {
        DroneCargoRequest::BeginDropoff {
            operation_id,
            drone_id,
            resume_mining,
            ..
        } => {
            if let Some(old) = s.operation(*operation_id) {
                ensure!(
                    old.drone_id == *drone_id && old.requested_resume_mining == *resume_mining,
                    "drop-off ID reused with different intent"
                );
                return Ok(());
            }
            let followup = if *resume_mining {
                let intent = s
                    .holds
                    .iter()
                    .find(|h| h.drone_id == *drone_id)
                    .and_then(|h| h.mining_intent.as_ref())
                    .context("no retained mining intent to resume")?;
                DroneDropoffFollowup::ResumeMining {
                    target_id: intent.target_id,
                    job_id: intent.job_id,
                    intent_generation: intent.generation,
                }
            } else {
                DroneDropoffFollowup::Hold
            };
            begin(w, name, *operation_id, *drone_id, followup, now, !automated)
        }
        DroneCargoRequest::Hold { operation_id, .. } => {
            control_hold(w, name, *operation_id, now, "Held by pilot")
        }
        DroneCargoRequest::Resume { operation_id, .. } => resume(w, name, *operation_id, now),
        DroneCargoRequest::Cancel { operation_id, .. } => cancel(w, name, *operation_id, now),
    }
}
fn control_hold(w: &mut World, name: &str, id: u64, now: u64, reason: &str) -> Result<()> {
    let op = state(w, name)
        .unwrap()
        .operation(id)
        .context("drop-off operation missing")?
        .clone();
    let cleanup = state(w, name)
        .unwrap()
        .cleanup
        .as_ref()
        .is_some_and(|c| c.operation_id == id);
    if !op.active() && !cleanup {
        return Ok(());
    }
    drone_bay::hold_service_batch(w, name, id, now, reason);
    if let Some(run) = runtime_mut(w, name).filter(|r| r.operation_id == id) {
        run.held = true;
        run.updated_at_ms = now;
    }
    if !cleanup {
        if let Some(p) = w.pilots.get_mut(name) {
            if let Some(d) = p.view.drones.iter_mut().find(|d| d.id == op.drone_id) {
                d.velocity = [0.; 3];
                d.mining_cycle = None;
                d.attack_ready_at_ms = None;
                d.updated_at_ms = now;
            }
            drones::sync(p);
        }
    }
    if cleanup {
        let c = state_mut(w, name)?.cleanup.as_mut().unwrap();
        c.held = true;
        c.reason = reason.into();
        return Ok(());
    }
    let op = op_mut(w, name, id)?;
    if op.phase != DroneDropoffPhase::Held {
        op.held_phase = Some(op.phase);
        op.phase = DroneDropoffPhase::Held;
    }
    op.updated_at_ms = now;
    op.next_batch_at_ms = None;
    op.message = reason.into();
    Ok(())
}
pub(crate) fn hold(w: &mut World, name: &str, now: u64, reason: &str) -> Result<()> {
    let Some(s) = state(w, name) else {
        return Ok(());
    };
    let ids: Vec<_> = s
        .operations
        .iter()
        .filter(|o| o.active())
        .map(|o| o.operation_id)
        .chain(s.cleanup.iter().map(|c| c.operation_id))
        .collect();
    for id in ids {
        control_hold(w, name, id, now, reason)?;
    }
    Ok(())
}
fn resume(w: &mut World, name: &str, id: u64, now: u64) -> Result<()> {
    let old = state(w, name)
        .unwrap()
        .operation(id)
        .context("drop-off operation missing")?
        .clone();
    let cleanup = state(w, name)
        .unwrap()
        .cleanup
        .as_ref()
        .is_some_and(|c| c.operation_id == id);
    if !old.active() && !cleanup {
        return Ok(());
    }
    if let Some(run) = runtime(w, name).filter(|r| r.operation_id == id).cloned() {
        if !run.held {
            return Ok(());
        }
        check(w, name, &run, now, None)?;
        let r = runtime_mut(w, name).unwrap();
        r.source = r.confirmed.clone();
        r.started_at_ms = now;
        r.updated_at_ms = now;
        r.confirmed_at_ms = now;
        r.held = false;
        r.duration_ms = duration(&r.source, &r.points[r.index], r.phases[r.index])?;
        if !cleanup {
            if let Some(d) = w
                .pilots
                .get_mut(name)
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == old.drone_id)
            {
                d.updated_at_ms = now;
            }
            drones::sync(w.pilots.get_mut(name).unwrap());
        }
        if cleanup {
            let c = state_mut(w, name)?.cleanup.as_mut().unwrap();
            c.held = false;
            c.reason = "Returning empty receiver hardware".into();
        } else {
            let phase = old.held_phase.context("held phase missing")?;
            set_phase(w, name, id, phase, now, phase.label())?;
            if phase == DroneDropoffPhase::Transferring {
                op_mut(w, name, id)?.next_batch_at_ms = Some(
                    now.checked_add(DRONE_DROPOFF_BATCH_MS)
                        .context("transfer clock overflow")?,
                );
            }
        }
    } else if old.phase == DroneDropoffPhase::Held
        && old.held_phase == Some(DroneDropoffPhase::Queued)
    {
        ready(w, name, false)?;
        set_phase(
            w,
            name,
            id,
            DroneDropoffPhase::Queued,
            now,
            "Queued after explicit Resume",
        )?;
        admit_next(w, name, now)?;
    }
    Ok(())
}
fn release_points(run: &Runtime) -> (Vec<Frame>, Vec<DroneDropoffPhase>) {
    let mut points = Vec::new();
    let mut phases = Vec::new();
    let mut unlocked = run.confirmed.clone();
    // Keep extension fixed while opening both locks, then retract. This also
    // safely reverses a partly completed coupling stroke without snapping.
    unlocked.groups[3].position_m[2] = geometry::reference_pose().position_m[2];
    unlocked.groups[4].position_m[2] = geometry::reference_pose().position_m[2];
    if unlocked.groups != run.confirmed.groups {
        points.push(unlocked.clone());
        phases.push(DroneDropoffPhase::Decoupling);
    }
    let stowed = Frame {
        groups: geometry::reference_poses(),
        ..unlocked
    };
    if points.last().unwrap_or(&run.confirmed).groups != stowed.groups {
        points.push(stowed.clone());
        phases.push(DroneDropoffPhase::Decoupling);
    }
    if !run.cleanup {
        if run.phases[run.index] == DroneDropoffPhase::Approaching {
            for frame in run.points[..run.index].iter().rev() {
                points.push(frame.clone());
                phases.push(DroneDropoffPhase::Egress);
            }
            points.push(run.entry.clone());
            phases.push(DroneDropoffPhase::Egress);
        } else {
            let start = if run.phases[run.index] == DroneDropoffPhase::Egress {
                run.index
            } else {
                run.phases
                    .iter()
                    .position(|p| *p == DroneDropoffPhase::Egress)
                    .expect("accepted receiver egress")
            };
            for target in &run.points[start..] {
                points.push(target.clone());
                phases.push(DroneDropoffPhase::Egress);
            }
        }
    }
    if points.is_empty() {
        points.push(stowed);
        phases.push(DroneDropoffPhase::Decoupling);
    }
    (points, phases)
}
fn cancel(w: &mut World, name: &str, id: u64, now: u64) -> Result<()> {
    let op = state(w, name)
        .unwrap()
        .operation(id)
        .context("drop-off operation missing")?
        .clone();
    if !op.active() || op.cancel_requested {
        return Ok(());
    }
    if let Some(h) = state_mut(w, name)?
        .holds
        .iter_mut()
        .find(|h| h.drone_id == op.drone_id)
    {
        invalidate_intent(h)?;
    }
    op_mut(w, name, id)?.cancel_requested = true;
    let Some(mut r) = runtime(w, name).filter(|r| r.operation_id == id).cloned() else {
        return set_phase(
            w,
            name,
            id,
            DroneDropoffPhase::Cancelled,
            now,
            "Queued service cancelled; ore remains on drone",
        );
    };
    let (points, phases) = release_points(&r);
    r.points = points;
    r.phases = phases;
    r.index = 0;
    r.source = r.confirmed.clone();
    r.route_origin = r.confirmed.clone();
    r.confirmed_at_ms = now;
    r.started_at_ms = now;
    r.updated_at_ms = now;
    r.cancelling = true;
    r.ready = false;
    r.duration_ms = duration(&r.source, &r.points[0], r.phases[0])?;
    let phase = r.phases[0];
    let held = r.held;
    *runtime_mut(w, name).unwrap() = r;
    set_phase(
        w,
        name,
        id,
        phase,
        now,
        "Cancelling safely; committed ore is retained",
    )?;
    if held {
        control_hold(
            w,
            name,
            id,
            now,
            "Cancellation route held; Resume to clear receiver safely",
        )?;
    }
    Ok(())
}
pub(crate) fn destroyed(w: &mut World, name: &str, drone: u64, now: u64) -> Result<()> {
    let ids: Vec<_> = state(w, name)
        .unwrap()
        .operations
        .iter()
        .filter(|o| o.drone_id == drone && o.active())
        .map(|o| o.operation_id)
        .collect();
    for id in ids {
        set_phase(
            w,
            name,
            id,
            DroneDropoffPhase::Destroyed,
            now,
            "Drone destroyed; payload recorded as lost",
        )?;
    }
    let Some(mut r) = runtime(w, name)
        .filter(|r| r.drone_id == drone && !r.cleanup)
        .cloned()
    else {
        return Ok(());
    };
    r.cleanup = true;
    r.cancelling = true;
    r.ready = false;
    let (points, phases) = release_points(&r);
    r.points = points;
    r.phases = phases;
    r.index = 0;
    r.source = r.confirmed.clone();
    r.route_origin = r.confirmed.clone();
    r.confirmed_at_ms = now;
    r.started_at_ms = now;
    r.updated_at_ms = now;
    r.duration_ms = duration(&r.source, &r.points[0], r.phases[0])?;
    let s = state_mut(w, name)?;
    s.receiver_operation = None;
    s.cleanup = Some(DroneBayCleanupView {
        operation_id: r.operation_id,
        held: r.held,
        reason: "Returning empty receiver hardware after destruction".into(),
    });
    *runtime_mut(w, name).unwrap() = r;
    Ok(())
}

pub(crate) fn advance(
    w: &mut World,
    now: u64,
    origins: &CarrierOrigins,
    defer: Option<&str>,
) -> Result<()> {
    if w.drone_cargo_registration.is_none() {
        return Ok(());
    }
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        if let Some(mut r) = runtime(w, &name).cloned() {
            if r.held || r.ready {
                continue;
            }
            if now.saturating_sub(r.updated_at_ms) > MAX_GAP_MS {
                control_hold(
                    w,
                    &name,
                    r.operation_id,
                    now,
                    "Tick gap; receiver held without offline catch-up",
                )?;
                continue;
            }
            if let Err(error) = check(w, &name, &r, now, Some(origins)) {
                control_hold(
                    w,
                    &name,
                    r.operation_id,
                    now,
                    &format!("Receiver obstructed: {error}"),
                )?;
                continue;
            }
            let phase = r.phases[r.index];
            if phase != DroneDropoffPhase::Transferring {
                r.confirmed =
                    r.source
                        .sample(&r.points[r.index], r.started_at_ms, r.duration_ms, now)?;
                if now >= r.started_at_ms.saturating_add(r.duration_ms) {
                    r.confirmed = r.points[r.index].clone();
                    r.index += 1;
                    if r.index == r.points.len() {
                        r.ready = true;
                    } else {
                        r.source = r.confirmed.clone();
                        r.started_at_ms = now;
                        r.duration_ms = duration(&r.source, &r.points[r.index], r.phases[r.index])?;
                        if !r.cleanup {
                            let phase = r.phases[r.index];
                            set_phase(w, &name, r.operation_id, phase, now, phase.label())?;
                            if phase == DroneDropoffPhase::Transferring {
                                op_mut(w, &name, r.operation_id)?.next_batch_at_ms = Some(
                                    now.checked_add(DRONE_DROPOFF_BATCH_MS)
                                        .context("transfer clock overflow")?,
                                );
                            }
                        }
                    }
                }
            }
            r.updated_at_ms = now;
            r.confirmed_at_ms = now;
            if !r.cleanup {
                op_mut(w, &name, r.operation_id)?.updated_at_ms = now;
                let pose = r
                    .parent
                    .compose(r.confirmed.drone)
                    .context("invalid receiver world pose")?;
                if let Some(d) = w
                    .pilots
                    .get_mut(&name)
                    .unwrap()
                    .view
                    .drones
                    .iter_mut()
                    .find(|d| d.id == r.drone_id)
                {
                    d.position = pose.position_m;
                    d.orientation_xyzw = Some(pose.orientation_xyzw);
                    d.heading = pose.transform_vector([1., 0., 0.]).unwrap();
                    d.velocity = if r.ready || phase == DroneDropoffPhase::Transferring {
                        [0.; 3]
                    } else {
                        r.parent
                            .transform_vector(
                                BayMotionSegment {
                                    source: r.source.drone,
                                    target: r.points[r.index].drone,
                                    started_at_ms: r.started_at_ms,
                                    duration_ms: r.duration_ms,
                                }
                                .sample(now)
                                .map_or([0.; 3], |v| v.1),
                            )
                            .unwrap_or([0.; 3])
                    };
                    d.mode = DroneMode::Hold;
                    d.mining_cycle = None;
                    d.attack_ready_at_ms = None;
                    d.updated_at_ms = now;
                }
                drones::sync(w.pilots.get_mut(&name).unwrap());
            }
            *runtime_mut(w, &name).unwrap() = r;
        } else if defer != Some(name.as_str()) {
            admit_next(w, &name, now)?;
        }
    }
    Ok(())
}
/// Finalizes only after incoming damage and cargo-loss processing.
pub(crate) fn finish(w: &mut World, now: u64) -> Result<()> {
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let Some(r) = runtime(w, &name).filter(|r| r.ready && !r.held).cloned() else {
            continue;
        };
        let id = state(w, &name).unwrap().binding.vessel_id.clone();
        if !r.cleanup {
            let old = state(w, &name)
                .unwrap()
                .operation(r.operation_id)
                .unwrap()
                .clone();
            let phase = if r.cancelling {
                DroneDropoffPhase::Cancelled
            } else {
                DroneDropoffPhase::Completed
            };
            set_phase(w, &name, r.operation_id, phase, now, phase.label())?;
            if !r.cancelling {
                if let DroneDropoffFollowup::ResumeMining {
                    target_id,
                    job_id,
                    intent_generation,
                } = old.followup
                {
                    let valid = state(w, &name)
                        .unwrap()
                        .holds
                        .iter()
                        .find(|h| h.drone_id == r.drone_id)
                        .is_some_and(|h| {
                            h.mining_intent.as_ref().is_some_and(|i| {
                                i.generation == intent_generation
                                    && i.target_id == target_id
                                    && i.job_id == job_id
                            })
                        })
                        && job_id.is_none_or(|id| {
                            w.resource_jobs.get(&name).is_some_and(|j| {
                                j.view.id == id
                                    && matches!(
                                        j.view.phase,
                                        ResourceJobPhase::Mining
                                            | ResourceJobPhase::AcquiringTarget
                                    )
                            })
                        })
                        && w.pilots[&name].view.locked_asteroids.contains(&target_id)
                        && w.asteroids
                            .iter()
                            .any(|a| a.id == target_id && a.ore > 0 && a.system_id == r.system_id)
                        && hold_capacity(w, &name) > w.pilots[&name].view.ore;
                    if valid {
                        if let Some(d) = w
                            .pilots
                            .get_mut(&name)
                            .unwrap()
                            .view
                            .drones
                            .iter_mut()
                            .find(|d| d.id == r.drone_id)
                        {
                            d.mode = DroneMode::Mining { target_id };
                            d.updated_at_ms = now;
                        }
                    }
                }
            }
        }
        w.accounts
            .get_mut(&name)
            .unwrap()
            .drone_receivers
            .remove(&id);
        let s = state_mut(w, &name)?;
        s.receiver_operation = None;
        s.cleanup = None;
        drones::sync(w.pilots.get_mut(&name).unwrap());
    }
    Ok(())
}
pub(crate) fn commit_transfers(
    w: &mut World,
    tx: &rusqlite::Transaction<'_>,
    entries: &mut Vec<LedgerEntry>,
    now: u64,
) -> Result<()> {
    if w.drone_cargo_registration.is_none() || w.independent_bay_registration.is_some() {
        return Ok(());
    }
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        let Some(old) = state(w, &name)
            .and_then(|s| s.receiver_operation.and_then(|id| s.operation(id)))
            .filter(|o| {
                o.phase == DroneDropoffPhase::Transferring
                    && o.next_batch_at_ms.is_some_and(|t| now >= t)
            })
            .cloned()
        else {
            continue;
        };
        let r = runtime(w, &name)
            .context("transfer runtime missing")?
            .clone();
        check(w, &name, &r, now, None)?;
        ensure!(
            r.confirmed.drone == geometry::coupled_pose()
                && r.confirmed.groups == groups(geometry::DropoffConfiguration::COUPLED)
                && !r.held
                && !r.cleanup
                && w.pilots[&name]
                    .view
                    .drones
                    .iter()
                    .any(|d| d.id == old.drone_id && d.hit_points > 0),
            "transfer requires live confirmed coupling"
        );
        let last = state(w, &name)
            .unwrap()
            .operations
            .iter()
            .flat_map(|o| &o.receipts)
            .map(|r| r.committed_at_ms)
            .max();
        if last.is_some_and(|t| t.saturating_add(DRONE_DROPOFF_BATCH_MS) > now) {
            continue;
        }
        let units = old.remaining_units.min(DRONE_DROPOFF_BATCH_UNITS);
        ensure!(
            units > 0 && payload(w, &name, old.drone_id) == old.remaining_units,
            "receiver payload reservation changed"
        );
        let next = w.pilots[&name]
            .view
            .ore
            .checked_add(units)
            .context("ship cargo overflow")?;
        ensure!(
            next <= cargo_capacity(w.pilots[&name].view.ship_class),
            "reserved ship capacity disappeared"
        );
        state_mut(w, &name)?
            .holds
            .iter_mut()
            .find(|h| h.drone_id == old.drone_id)
            .unwrap()
            .ore_units -= units;
        w.pilots.get_mut(&name).unwrap().view.ore = next;
        let sequence = old.receipts.len() as u64 + 1;
        append_event(
            tx,
            entries,
            EconomicEvent::DroneCargo {
                record: DroneCargoLedgerEvent::Transferred {
                    pilot: name.clone(),
                    vessel_id: old.binding.vessel_id.clone(),
                    drone_id: old.drone_id,
                    operation_id: old.operation_id,
                    operation_started_at_ms: old.accepted_at_ms,
                    requested_units: old.requested_units,
                    sequence,
                    units,
                },
            },
            now,
            ruleset(w),
        )?;
        let entry = entries.last().unwrap();
        let op = op_mut(w, &name, old.operation_id)?;
        op.committed_units += units;
        op.remaining_units -= units;
        op.updated_at_ms = now;
        op.receipts.push(DroneOreTransferReceipt {
            sequence,
            units,
            committed_at_ms: now,
            ledger_sequence: entry.sequence,
            ledger_hash: entry.hash.clone(),
        });
        op.next_batch_at_ms = Some(
            now.checked_add(DRONE_DROPOFF_BATCH_MS)
                .context("transfer clock overflow")?,
        );
        op.message = format!(
            "Transferring confirmed ore: {} / {} units",
            op.committed_units, op.requested_units
        );
        if op.remaining_units == 0 {
            let run = runtime_mut(w, &name).unwrap();
            run.index += 1;
            run.source = run.confirmed.clone();
            run.started_at_ms = now;
            run.updated_at_ms = now;
            run.confirmed_at_ms = now;
            run.duration_ms = duration(&run.source, &run.points[run.index], run.phases[run.index])?;
            set_phase(
                w,
                &name,
                old.operation_id,
                DroneDropoffPhase::Decoupling,
                now,
                "Payload delivered; disconnecting safely",
            )?;
        }
    }
    Ok(())
}
pub(crate) fn projection(w: &World, name: &str) -> Result<Option<DroneCargoPublicState>> {
    let Some(s) = state(w, name) else {
        return Ok(None);
    };
    let run = runtime(w, name);
    if let Some(r) = run {
        ensure!(
            !r.points.is_empty()
                && r.points.len() == r.phases.len()
                && r.index < r.points.len()
                && !r.ready,
            "invalid persisted receiver projection route"
        );
    }
    let latest = s
        .operations
        .iter()
        .rev()
        .find(|o| !o.active())
        .map(|o| o.operation_id);
    let operations = s
        .operations
        .iter()
        .filter(|o| {
            o.active()
                || Some(o.operation_id) == latest
                || s.cleanup
                    .as_ref()
                    .is_some_and(|c| c.operation_id == o.operation_id)
        })
        .cloned()
        .collect();
    let segment = |source, target, r: &Runtime| {
        (!r.held && !r.ready && r.phases[r.index] != DroneDropoffPhase::Transferring).then_some(
            BayMotionSegment {
                source,
                target,
                started_at_ms: r.started_at_ms,
                duration_ms: r.duration_ms,
            },
        )
    };
    let motion = run.filter(|r| !r.cleanup).map(|r| DroneDropoffMotionView {
        operation_id: r.operation_id,
        drone_id: r.drone_id,
        frame: DroneBayMotionFrame::Body,
        confirmed_pose: r.confirmed.drone,
        segment: if r.ready {
            None
        } else {
            segment(r.source.drone, r.points[r.index].drone, r)
        },
    });
    let roots = run.map_or(geometry::reference_poses(), |r| r.confirmed.groups);
    let groups = geometry::mechanism_ids()
        .into_iter()
        .enumerate()
        .map(|(i, id)| DroneBayMechanismView {
            semantic_id: id.semantic_id().into(),
            confirmed_pose: roots[i],
            segment: run
                .filter(|r| !r.ready)
                .and_then(|r| segment(r.source.groups[i], r.points[r.index].groups[i], r)),
        })
        .collect();
    Ok(Some(DroneCargoPublicState {
        schema_version: 1,
        binding: s.binding.clone(),
        holds: s
            .holds
            .iter()
            .map(|h| DroneOreHoldView {
                drone_id: h.drone_id,
                ore_units: h.ore_units,
                capacity_units: MINING_DRONE_CARGO_CAPACITY,
                cycle_reserved_units: h.cycle.as_ref().map_or(0, |c| c.reserved_units),
            })
            .collect(),
        incoming_reserved_units: s.reserved_incoming().context("reservation overflow")?,
        operations,
        receiver_operation: s.receiver_operation,
        cleanup: s.cleanup.clone(),
        motion,
        groups,
    }))
}
pub(crate) fn validate(w: &World, name: &str) -> Result<()> {
    let s = state(w, name).unwrap();
    let run = runtime(w, name);
    ensure!(
        run.map(|r| r.operation_id)
            == s.receiver_operation
                .or_else(|| s.cleanup.as_ref().map(|c| c.operation_id)),
        "receiver runtime/receipt ownership mismatch"
    );
    if let Some(r) = run {
        ensure!(
            !r.points.is_empty()
                && r.points.len() == r.phases.len()
                && r.index < r.points.len()
                && !r.ready
                && r.parent.valid()
                && r.source.drone.valid()
                && r.confirmed.drone.valid()
                && r.points
                    .iter()
                    .all(|f| f.drone.valid() && f.groups.iter().all(|p| p.valid()))
                && r.duration_ms > 0
                && r.started_at_ms.checked_add(r.duration_ms).is_some(),
            "invalid persisted receiver route"
        );
    }
    if let Some(r) = run {
        ensure!(
            shared::parent(w, name)? == r.parent,
            "receiver carrier pose differs from accepted stationary frame"
        );
        let op = s
            .operation(r.operation_id)
            .context("receiver runtime receipt missing")?;
        ensure!(
            op.drone_id == r.drone_id && r.system_id == w.pilots[name].view.system_id,
            "receiver runtime identity mismatch"
        );
        ensure!(
            r.cleanup == s.cleanup.is_some()
                && (!r.cleanup || op.phase == DroneDropoffPhase::Destroyed),
            "receiver cleanup identity mismatch"
        );
        if r.cleanup {
            ensure!(
                s.cleanup.as_ref().unwrap().held == r.held,
                "receiver cleanup hold mismatch"
            );
        } else {
            ensure!(
                r.held == (op.phase == DroneDropoffPhase::Held)
                    && op.held_phase.unwrap_or(op.phase) == r.phases[r.index],
                "receiver runtime phase mismatch"
            );
            ensure!(
                r.cancelling == op.cancel_requested,
                "receiver cancellation mismatch"
            );
        }
    }
    if let Some(r) = run {
        let home = drone_bay::state(w, name)
            .unwrap()
            .stock
            .iter()
            .find(|e| e.drone_id == r.drone_id)
            .context("receiver home identity missing")?
            .home_cradle;
        validate_route(shared::definition(w, name)?, r, home)?;
        if !r.cleanup {
            let d = w.pilots[name]
                .view
                .drones
                .iter()
                .find(|d| d.id == r.drone_id)
                .context("receiver body missing")?;
            let world = r
                .parent
                .compose(r.confirmed.drone)
                .context("receiver world transform invalid")?;
            ensure!(
                pose_close(
                    world,
                    BayPose {
                        position_m: d.position,
                        orientation_xyzw: d
                            .orientation_xyzw
                            .context("receiver body attitude missing")?
                    }
                ),
                "receiver confirmed body disagrees with actual identity"
            );
        }
    }
    let public = projection(w, name)?.unwrap();
    public
        .validate_projection(&w.pilots[name].view)
        .map_err(anyhow::Error::msg)
}
pub(crate) fn authorize(w: &World, name: &str, request: &Request) -> Result<()> {
    let Some(s) = state(w, name) else {
        return Ok(());
    };
    let occupied = busy(w, name);
    if matches!(request, Request::StartResourceJob { .. }) {
        ensure!(
            !s.operations.iter().any(|o| o.active()) && s.cleanup.is_none(),
            "finish or cancel ore service before starting a resource job"
        );
    }
    match request {
        Request::Navigate { mode } if occupied && !matches!(mode, NavigationMode::Stop) => {
            bail!("Ore receiver handling requires a stationary carrier; STOP remains available")
        }
        Request::PilotInput {
            translation,
            rotation,
        } if occupied && translation.iter().chain(rotation).any(|v| *v != 0.) => {
            bail!("Ore receiver handling requires a stationary carrier")
        }
        Request::Warp { .. }
        | Request::Jump { .. }
        | Request::Dock
        | Request::Undock
        | Request::SelectShip { .. }
        | Request::BoardShip { .. }
            if occupied =>
        {
            bail!("Clear or resume the ore receiver before departure")
        }
        Request::DroneBay {
            request: DroneBayRequest::Launch { .. } | DroneBayRequest::Recover { .. },
        } if occupied => bail!("Ore receiver owns the nearby physical route"),
        Request::DroneBay {
            request: DroneBayRequest::Recover { drone_id, .. },
        } if payload(w, name, *drone_id) > 0 => {
            bail!("Drop off ore first, or use Recall All to unload then stow")
        }
        Request::DroneNavigate { .. } | Request::DroneHold
            if w.pilots[name]
                .view
                .drones
                .first()
                .is_some_and(|d| constrained(w, name, d.id)) =>
        {
            bail!("Ore service owns this drone; use its Hold, Resume or Cancel controls")
        }
        Request::AssignMiningDrone { id, .. }
        | Request::AssignAttackDrone { id, .. }
        | Request::DroneNavigateById { id, .. }
        | Request::DroneHoldById { id }
        | Request::OrbitDrone { id, .. }
            if constrained(w, name, *id) =>
        {
            bail!("Ore service owns this drone; use its Hold, Resume or Cancel controls")
        }
        _ => {}
    }
    Ok(())
}

fn service_tail() -> (Vec<Frame>, Vec<DroneDropoffPhase>) {
    use geometry::DropoffConfiguration as C;
    use DroneDropoffPhase::*;
    let steps = [
        (C::STOWED, Aligning),
        (C::EXTENDED, Coupling),
        (C::COUPLED, Coupling),
        (C::COUPLED, Transferring),
        (C::EXTENDED, Decoupling),
        (C::STOWED, Decoupling),
    ];
    let mut points: Vec<_> = steps
        .iter()
        .map(|(c, _)| frame(geometry::coupled_pose(), *c))
        .collect();
    let mut phases: Vec<_> = steps.iter().map(|(_, p)| *p).collect();
    points.push(frame(geometry::gate_pose(), C::STOWED));
    phases.push(Egress);
    (points, phases)
}
fn pose_close(a: BayPose, b: BayPose) -> bool {
    a.position_m
        .iter()
        .chain(&a.orientation_xyzw)
        .zip(b.position_m.iter().chain(&b.orientation_xyzw))
        .all(|(a, b)| (a - b).abs() < 1e-8)
}
fn frame_close(a: &Frame, b: &Frame) -> bool {
    pose_close(a.drone, b.drone)
        && a.groups
            .iter()
            .zip(b.groups)
            .all(|(a, b)| pose_close(*a, b))
}
fn outside_carrier(
    definition: &arkline_protocol::drone_bay_definition::OfficialBayDefinition,
    a: BayPose,
    b: BayPose,
) -> bool {
    let radius = definition
        .drone_half_extents_m
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
        + 0.1;
    let bounds = geometry::DROPOFF_MODULE_STOWED_BOUNDS_M;
    let min =
        std::array::from_fn(|i| definition.carrier_envelope.min_m[i].min(bounds[0][i]) - radius);
    let max =
        std::array::from_fn(|i| definition.carrier_envelope.max_m[i].max(bounds[1][i]) + radius);
    !intersects(a.position_m, b.position_m, min, max)
}
fn corridor(p: BayPose) -> bool {
    let mut expected = geometry::coupled_pose();
    expected.position_m[0] = p.position_m[0];
    (-50.000000001..=-39.999999999).contains(&p.position_m[0]) && pose_close(p, expected)
}
fn valid_frame(f: &Frame) -> bool {
    f.drone.valid()
        && f.drone.position_m.iter().all(|v| v.abs() <= SPACE_BOUND)
        && valid_dropoff_mechanism_roots(&f.groups)
}
fn stroke(f: &Frame) -> (f64, f64) {
    (
        geometry::reference_pose().position_m[0] - f.groups[2].position_m[0],
        (geometry::reference_pose().position_m[2] - f.groups[3].position_m[2]) / 0.03,
    )
}
fn validate_route_segment(
    definition: &arkline_protocol::drone_bay_definition::OfficialBayDefinition,
    a: &Frame,
    b: &Frame,
    phase: DroneDropoffPhase,
    cleanup: bool,
) -> Result<()> {
    use DroneDropoffPhase::*;
    ensure!(
        valid_frame(a) && valid_frame(b),
        "receiver route frame violates source kinematics"
    );
    let stowed = |f: &Frame| {
        f.groups
            .iter()
            .zip(geometry::reference_poses())
            .all(|(a, b)| pose_close(*a, b))
    };
    let coupled = |f: &Frame| pose_close(f.drone, geometry::coupled_pose());
    let (ae, al) = stroke(a);
    let (be, bl) = stroke(b);
    let valid = if cleanup {
        phase == Decoupling && pose_close(a.drone, b.drone) && be <= ae + 1e-9 && bl <= al + 1e-9
    } else {
        match phase {
            Approaching => stowed(a) && stowed(b) && outside_carrier(definition, a.drone, b.drone),
            Aligning => stowed(a) && stowed(b) && corridor(a.drone) && coupled(b),
            Coupling => coupled(a) && coupled(b) && be + 1e-9 >= ae && bl + 1e-9 >= al,
            Transferring => {
                a == &frame(
                    geometry::coupled_pose(),
                    geometry::DropoffConfiguration::COUPLED,
                ) && a == b
            }
            Decoupling => coupled(a) && coupled(b) && be <= ae + 1e-9 && bl <= al + 1e-9,
            Egress => {
                stowed(a)
                    && stowed(b)
                    && (outside_carrier(definition, a.drone, b.drone)
                        || (corridor(a.drone) && corridor(b.drone)))
            }
            _ => false,
        }
    };
    ensure!(
        valid,
        "receiver route phase violates accepted physical corridor"
    );
    let sampled = a.sample(b, 0, 1000, 500)?;
    ensure!(
        valid_frame(&sampled),
        "receiver route crosses an invalid hardware configuration"
    );
    Ok(())
}
fn validate_route(
    definition: &arkline_protocol::drone_bay_definition::OfficialBayDefinition,
    r: &Runtime,
    home: u8,
) -> Result<()> {
    ensure!(
        valid_frame(&r.entry)
            && valid_frame(&r.route_origin)
            && valid_frame(&r.source)
            && valid_frame(&r.confirmed),
        "invalid receiver entry/source/confirmed frame"
    );
    if !r.cancelling {
        let (tail, phases) = service_tail();
        let n = r
            .phases
            .iter()
            .position(|p| *p == DroneDropoffPhase::Aligning)
            .context("receiver alignment phase missing")?;
        ensure!(
            n > 0 && r.points.len() > n + tail.len(),
            "receiver route lacks accepted exterior paths"
        );
        ensure!(
            r.points[n..n + tail.len()] == tail
                && r.phases[n..n + tail.len()] == phases
                && r.phases[..n]
                    .iter()
                    .all(|p| *p == DroneDropoffPhase::Approaching)
                && r.points[n - 1]
                    == frame(
                        geometry::gate_pose(),
                        geometry::DropoffConfiguration::STOWED
                    )
                && r.phases[n + tail.len()..]
                    .iter()
                    .all(|p| *p == DroneDropoffPhase::Egress)
                && r.points.last().unwrap()
                    == &frame(clear_pose(home), geometry::DropoffConfiguration::STOWED)
                && r.route_origin == r.entry,
            "receiver route differs from registered service sequence"
        );
    } else {
        ensure!(
            r.phases
                .iter()
                .all(|p| matches!(p, DroneDropoffPhase::Decoupling | DroneDropoffPhase::Egress)),
            "cancelled receiver route cannot re-enter service"
        );
        let end = r.points.last().unwrap();
        ensure!(
            end.groups == geometry::reference_poses(),
            "receiver cleanup must end safely stowed"
        );
        ensure!(
            r.cleanup
                || pose_close(end.drone, clear_pose(home))
                || pose_close(end.drone, r.entry.drone),
            "receiver cancellation endpoint is not accepted egress"
        );
    }
    let mut source = &r.route_origin;
    for (target, phase) in r.points.iter().zip(&r.phases) {
        validate_route_segment(definition, source, target, *phase, r.cleanup)?;
        source = target;
    }
    validate_route_segment(
        definition,
        &r.source,
        &r.points[r.index],
        r.phases[r.index],
        r.cleanup,
    )?;
    ensure!(
        r.confirmed_at_ms >= r.started_at_ms
            && r.confirmed_at_ms <= r.updated_at_ms
            && r.duration_ms == duration(&r.source, &r.points[r.index], r.phases[r.index])?,
        "receiver sampled clock or speed contract changed"
    );
    let sampled = r.source.sample(
        &r.points[r.index],
        r.started_at_ms,
        r.duration_ms,
        r.confirmed_at_ms,
    )?;
    ensure!(
        frame_close(&sampled, &r.confirmed),
        "receiver confirmed pose differs from accepted shared clock"
    );
    Ok(())
}
