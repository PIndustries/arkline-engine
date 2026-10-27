//! Registered physical route authority. Samples are committed with the world;
//! client clocks never move stock between deployed and stored ownership.
use super::*;
use arkline_protocol::drone_bay_definition::{
    official_bay_definition, official_bay_for_binding, OfficialBayDefinition, PIONEER_BAY_REG12,
};
use arkline_protocol::drone_bay_geometry::BayMotionSegment;
use arkline_protocol::drone_bay_layout as layout;

const MAX_ADVANCE_GAP_MS: u64 = 2_000;
const HANDLING_SPEED_M_S: f64 = 2.0;

#[derive(Clone, Serialize, Deserialize)]
struct Frame {
    drone: BayPose,
    groups: Vec<BayPose>,
}
impl Frame {
    fn from_route(frame: layout::BayRouteState) -> Self {
        Self {
            drone: frame.drone_body_pose,
            groups: frame.mechanism_body_roots.to_vec(),
        }
    }
    fn sample(&self, target: &Self, start: u64, duration: u64, now: u64) -> Result<Self> {
        let sample = |source, target| {
            BayMotionSegment {
                source,
                target,
                started_at_ms: start,
                duration_ms: duration,
            }
            .sample(now)
            .map(|v| v.0)
            .context("invalid registered bay segment")
        };
        Ok(Self {
            drone: sample(self.drone, target.drone)?,
            groups: self
                .groups
                .iter()
                .zip(&target.groups)
                .map(|(a, b)| sample(*a, *b))
                .collect::<Result<_>>()?,
        })
    }
    fn valid(&self) -> bool {
        self.drone.valid()
            && self.groups.len() == layout::DRONE_BAY_MECHANISM_COUNT
            && self.groups.iter().all(|p| p.valid())
    }
    fn door(&self, definition: &OfficialBayDefinition) -> f64 {
        let id = layout::BayMechanismId::DoorUpper;
        ((self.groups[id.index()].position_m[1]
            - definition.mechanism(id).reference_pose.position_m[1])
            / definition.rig.door_travel_m)
            .clamp(0., 1.)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Runtime {
    operation_id: u64,
    drone_id: u64,
    parent: BayPose,
    system_id: u32,
    /// Accepted source-bound route plus collision-checked outside approach.
    points: Vec<Frame>,
    phases: Vec<DroneBayPhase>,
    index: usize,
    source: Frame,
    confirmed: Frame,
    started_at_ms: u64,
    duration_ms: u64,
    updated_at_ms: u64,
    held: bool,
    cancelling: bool,
    cleanup: bool,
    ready_to_finalize: bool,
    /// Cancellation of launch returns home; recovery cancellation stays active.
    capture_at_end: bool,
}

pub(crate) fn registration() -> Option<DroneBayRegistration> {
    (pioneer_docked::PIONEER_DOCKED_REGISTRATION_REVISION
        == PIONEER_BAY_REG12.compatible_host.geometry_revision
        && pioneer_docked::PIONEER_DOCKED_SOURCE_SHA256
            == PIONEER_BAY_REG12.compatible_host.source_sha256
        && PIONEER_MATERIAL_SHA256 == PIONEER_BAY_REG12.compatible_host.source_sha256)
        .then(|| PIONEER_BAY_REG12.registration())
}
/// Resolve only the persisted source-bound official definition. Never substitute
/// the current default for a different or unknown owned-vessel registration.
pub(crate) fn definition(w: &World, name: &str) -> Result<&'static OfficialBayDefinition> {
    let stock = state(w, name).context("physical drone stock unavailable")?;
    let definition = official_bay_for_binding(&stock.binding)
        .context("unsupported physical bay source binding")?;
    ensure!(
        w.drone_bay_registration.as_ref() == Some(&definition.registration()),
        "vessel bay definition differs from immutable world registration"
    );
    Ok(definition)
}
pub(crate) fn creation_registration(scene: SceneLayoutRevision) -> Option<DroneBayRegistration> {
    (scene == SceneLayoutRevision::DockingV1)
        .then(registration)
        .flatten()
}
pub(crate) fn state<'a>(w: &'a World, name: &str) -> Option<&'a DroneBayState> {
    let p = w.pilots.get(name)?;
    let a = w.accounts.get(name)?;
    a.drone_bays.get(a.vessel_id(p.view.ship_class)?)
}
fn vessel(w: &World, name: &str) -> Option<String> {
    state(w, name).map(|s| s.binding.vessel_id.clone())
}
pub(crate) fn busy(w: &World, name: &str) -> bool {
    vessel(w, name).is_some_and(|id| w.accounts[name].drone_bay_runtime.contains_key(&id))
}
pub(crate) fn constrained(w: &World, name: &str, id: u64) -> bool {
    vessel(w, name)
        .and_then(|v| w.accounts[name].drone_bay_runtime.get(&v))
        .is_some_and(|r| r.drone_id == id && !r.cleanup)
}
pub(crate) fn create_pilot_stock(w: &mut World, name: &str, now: u64) -> Result<()> {
    let Some(reg) = w.drone_bay_registration.clone() else {
        return Ok(());
    };
    ensure!(
        Some(reg.clone()) == registration(),
        "unsupported drone bay creation registration"
    );
    let a = w.accounts.get_mut(name).context("owned account missing")?;
    let id = a
        .vessel_id(ShipClass::Pioneer)
        .context("owned Pioneer missing")?
        .to_owned();
    let initialized = initialize_once(
        &mut a.drone_bays,
        reg.bind(id.clone()),
        &mut w.next_drone_id,
        now,
    )?;
    if initialized && w.drone_bay_batch_version == Some(DRONE_BAY_BATCH_VERSION) {
        let s = a.drone_bays.get_mut(&id).unwrap();
        s.schema_version = DRONE_BAY_SCHEMA_VERSION;
        s.batches = Some(DroneBayBatchJournal::default());
    }
    Ok(())
}
pub(crate) fn parent(w: &World, name: &str) -> Result<BayPose> {
    let p = &w.pilots[name];
    ensure!(
        universe::normal(p),
        "physical bay requires undocked normal space"
    );
    let f = p.view.flight.as_ref().context("carrier pose missing")?;
    ensure!(
        f.velocity
            .iter()
            .chain(&f.angular_velocity)
            .all(|v| v.abs() <= 1e-6)
            && matches!(f.mode, NavigationMode::Stop),
        "stop the carrier before using the physical drone bay"
    );
    let pose = BayPose {
        position_m: f.position,
        orientation_xyzw: f.orientation,
    };
    ensure!(pose.valid(), "invalid carrier attitude");
    Ok(pose)
}
pub(crate) fn inverse(p: BayPose) -> Result<BayPose> {
    ensure!(p.valid(), "invalid inverse pose");
    let [x, y, z, w] = p.orientation_xyzw;
    let mut inv = BayPose {
        position_m: [0.; 3],
        orientation_xyzw: [-x, -y, -z, w],
    };
    inv.position_m = inv
        .transform_vector(p.position_m.map(|x| -x))
        .context("invalid inverse translation")?;
    Ok(inv)
}
fn duration(
    definition: &OfficialBayDefinition,
    a: &Frame,
    b: &Frame,
    approach: bool,
    class: DroneClass,
) -> Result<u64> {
    let distance = std::iter::once(flight::distance(a.drone.position_m, b.drone.position_m))
        .chain(
            a.groups
                .iter()
                .zip(&b.groups)
                .map(|(a, b)| flight::distance(a.position_m, b.position_m)),
        )
        .fold(0f64, f64::max);
    let exterior = a.drone.position_m[0] <= definition.external_hold.position_m[0]
        && b.drone.position_m[0] <= definition.external_hold.position_m[0]
        && a.groups == b.groups;
    let speed = if approach || exterior {
        drone_speed(class).min(40.)
    } else {
        HANDLING_SPEED_M_S
    };
    // Smoothstep peak speed is 1.5 times average. A minimum turn/actuator time
    // avoids zero-duration keyframes and deliberately does not claim ratings.
    let ms = (distance * 1500. / speed).ceil().max(750.);
    ensure!(
        ms.is_finite() && ms <= 86_400_000.,
        "bay route duration out of bounds"
    );
    Ok(ms as u64)
}
fn phase(kind: DroneBayOperationKind, index: usize) -> DroneBayPhase {
    match kind {
        DroneBayOperationKind::Launch => match index {
            0..=2 => DroneBayPhase::ReleasingCradle,
            3..=5 => DroneBayPhase::TransferringOut,
            6 => DroneBayPhase::OpeningDoor,
            _ => DroneBayPhase::Departing,
        },
        DroneBayOperationKind::Recover => match index {
            0..=7 => DroneBayPhase::Aligning,
            8..=9 => DroneBayPhase::Entering,
            10..=13 => DroneBayPhase::TransferringIn,
            _ => DroneBayPhase::SecuringCradle,
        },
    }
}
/// Inclusive slab intersection used for conservative collision admission.
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
/// A small visibility graph around the inflated carrier. All approach edges
/// are outside its complete envelope; the final aligned segment uses the
/// separately authored rear throat route. No hull-centre recall shortcut.
fn approach_points(
    definition: &OfficialBayDefinition,
    start: BayPose,
    receiver: bool,
) -> Result<Vec<BayPose>> {
    let mut bounds = definition.carrier_envelope;
    if receiver {
        let added = arkline_protocol::drone_dropoff_geometry::DROPOFF_MODULE_STOWED_BOUNDS_M;
        for axis in 0..3 {
            bounds.min_m[axis] = bounds.min_m[axis].min(added[0][axis]);
            bounds.max_m[axis] = bounds.max_m[axis].max(added[1][axis]);
        }
    }
    let min = bounds.min_m.map(|v| v - drone_radius() - 0.1);
    let max = bounds.max_m.map(|v| v + drone_radius() + 0.1);
    let end = definition.external_hold;
    let staging = BayPose {
        position_m: [min[0] - 1., end.position_m[1], end.position_m[2]],
        ..end
    };
    ensure!(
        !intersects(start.position_m, start.position_m, min, max),
        "recovery starts inside carrier clearance; move the drone clear first"
    );
    let mut nodes = vec![start, staging];
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
            if !done[v] && !intersects(nodes[u].position_m, nodes[v].position_m, min, max) {
                let c = costs[u] + flight::distance(nodes[u].position_m, nodes[v].position_m);
                if c < costs[v] {
                    costs[v] = c;
                    prev[v] = Some(u);
                }
            }
        }
    }
    ensure!(costs[1].is_finite(), "no clear carrier recovery approach");
    let mut path = vec![1];
    while *path.last().unwrap() != 0 {
        path.push(prev[*path.last().unwrap()].context("broken approach path")?);
    }
    path.reverse();
    let mut result: Vec<_> = path.into_iter().map(|i| nodes[i]).collect();
    // At staging the full rear-facing attitude is reached before entering the
    // authored narrow hold, whose real 2.5m longitudinal half extent is clear.
    result.push(end);
    Ok(result)
}
fn drone_radius() -> f64 {
    layout::DRONE_BAY_DRONE_HALF_EXTENTS_M
        .iter()
        .map(|v| v * v)
        .sum::<f64>()
        .sqrt()
}
/// Transaction-local evidence; never persisted or supplied by a client.
#[derive(Clone, Copy)]
pub(crate) struct CarrierOrigin {
    system_id: u32,
    position: [f64; 3],
    travel_bound_m: f64,
}
pub(crate) type CarrierOrigins = BTreeMap<String, CarrierOrigin>;
pub(crate) fn carrier_origins(w: &World, now: u64) -> CarrierOrigins {
    w.pilots
        .iter()
        .filter_map(|(name, p)| {
            p.view.flight.as_ref().map(|f| {
                // Normal/manual flight integrates <=10s in many curved substeps. Keep
                // pre-step motion evidence: post-step Stop or equal endpoints do not
                // prove that a moving carrier stayed still throughout this tick.
                let moving = universe::normal(p)
                    && p.view.location != Location::Docked
                    && (!matches!(f.mode, NavigationMode::Stop)
                        || f.velocity.iter().any(|v| v.abs() >= 0.001));
                let elapsed = now.saturating_sub(f.updated_at_ms) as f64 / 1000.;
                let mut travel_bound_m = if moving {
                    MAX_SHIP_SPEED * elapsed.min(10.)
                } else {
                    0.
                };
                if let Some((docking, segment)) = f
                    .docking
                    .as_ref()
                    .filter(|d| {
                        !matches!(
                            d.phase,
                            DockingPhase::Free | DockingPhase::Secured | DockingPhase::Held
                        )
                    })
                    .and_then(|d| d.segment.as_ref().map(|s| (d, s)))
                {
                    // Hermite derivative <=1.5*distance/duration + |initial velocity|.
                    let speed = 1.5 * flight::distance(segment.source_m, segment.target_m)
                        / (segment.duration_ms.max(1) as f64 / 1000.)
                        + segment
                            .initial_velocity_mps
                            .iter()
                            .map(|v| v * v)
                            .sum::<f64>()
                            .sqrt();
                    // docking::advance can cross several phase boundaries.
                    // Beyond this segment its bounded phase graph contains at
                    // most one further translating gate<->berth leg; all other
                    // phases are stationary, or braking ends Held. Add its full
                    // length so a zero-speed Releasing segment cannot hide the
                    // later Departing sweep in a coarse tick.
                    let later_leg = flight::distance(
                        docking_gate_position(docking.berth_id),
                        docking_berth_position(docking.berth_id),
                    );
                    travel_bound_m = travel_bound_m.max(speed * elapsed + later_leg);
                }
                (
                    name.clone(),
                    CarrierOrigin {
                        system_id: p.view.system_id,
                        position: f.position,
                        travel_bound_m,
                    },
                )
            })
        })
        .collect()
}
#[derive(Clone, Copy)]
struct ObstacleWindow<'a> {
    now: u64,
    lookback_ms: u64,
    carriers: Option<&'a CarrierOrigins>,
}
impl ObstacleWindow<'_> {
    fn at(now: u64) -> Self {
        Self {
            now,
            lookback_ms: 0,
            carriers: None,
        }
    }
}
fn obstruction(
    w: &World,
    name: &str,
    drone: u64,
    parent: BayPose,
    a: &Frame,
    b: &Frame,
    window: ObstacleWindow<'_>,
) -> Result<()> {
    check_external_route(
        w,
        name,
        ExternalRouteCheck {
            drone,
            parent,
            source: a.drone,
            target: b.drone,
            working: definition(w, name)?.working_envelope,
            moving_hardware: a.groups != b.groups,
            now: window.now,
            lookback_ms: window.lookback_ms,
            carriers: window.carriers,
            has_drone: true,
        },
    )
}
/// Shared conservative dynamic-obstacle admission. Receiver callers supply the
/// independently authored working envelope, not the storage bay's footprint.
pub(crate) struct ExternalRouteCheck<'a> {
    pub drone: u64,
    pub parent: BayPose,
    pub source: BayPose,
    pub target: BayPose,
    pub working: layout::BayEnvelope,
    pub moving_hardware: bool,
    pub now: u64,
    pub lookback_ms: u64,
    pub carriers: Option<&'a CarrierOrigins>,
    pub has_drone: bool,
}
pub(crate) fn check_external_route(
    w: &World,
    name: &str,
    route: ExternalRouteCheck<'_>,
) -> Result<()> {
    let ExternalRouteCheck {
        drone,
        parent,
        source,
        target,
        working,
        moving_hardware,
        now,
        lookback_ms: carrier_lookback_ms,
        carriers,
        has_drone,
    } = route;
    let p = &w.pilots[name];
    let pa = parent
        .compose(source)
        .context("invalid world route source")?
        .position_m;
    let pb = parent
        .compose(target)
        .context("invalid world route target")?
        .position_m;
    // Full body conservative sphere for other craft/asteroids, never only its centre.
    let radius = if has_drone { drone_radius() } else { 0. };
    let delta: [f64; 3] = std::array::from_fn(|i| pb[i] - pa[i]);
    let length2 = delta.iter().map(|v| v * v).sum::<f64>();
    let working = working
        .transformed(parent)
        .context("invalid machinery sweep")?;
    let sphere = |point: [f64; 3], r: f64| {
        let t = if length2 > 1e-12 {
            (0..3).map(|i| (point[i] - pa[i]) * delta[i]).sum::<f64>() / length2
        } else {
            0.
        }
        .clamp(0., 1.);
        let route_distance2 = (0..3)
            .map(|i| (point[i] - pa[i] - t * delta[i]).powi(2))
            .sum::<f64>();
        let hardware_distance2 = (0..3)
            .map(|i| {
                if point[i] < working.min_m[i] {
                    (working.min_m[i] - point[i]).powi(2)
                } else if point[i] > working.max_m[i] {
                    (point[i] - working.max_m[i]).powi(2)
                } else {
                    0.
                }
            })
            .sum::<f64>();
        (has_drone && route_distance2 <= (r + radius).powi(2))
            || (moving_hardware && hardware_distance2 <= r * r)
    };
    for (other, q) in &w.pilots {
        if q.view.system_id != p.view.system_id {
            continue;
        }
        if other != name
            && !matches!(
                q.view.location,
                Location::InTransit { .. } | Location::Jumping { .. }
            )
            && q.view.flight.as_ref().is_some_and(|f| {
                // A carrier may have moved and reached Stop earlier in this
                // tick. Its post-step velocity is not movement evidence.
                let lookback = carriers
                    .and_then(|origins| origins.get(other))
                    .filter(|origin| origin.system_id == q.view.system_id)
                    .map_or(0., |origin| {
                        origin
                            .travel_bound_m
                            .max(flight::distance(origin.position, f.position))
                    });
                sphere(f.position, hull_radius_m(q.view.ship_class) + lookback)
            })
        {
            bail!("bay route obstructed by another carrier");
        }
        if q.view.drones.iter().any(|d| {
            // Free drones integrate after this controller. Bound their whole
            // possible path this tick, not just its still-old endpoint.
            let moving = !(matches!(d.mode, DroneMode::Hold)
                || matches!(d.mode, DroneMode::Returning) && q.view.drone_bay.is_some());
            let constrained = w.accounts.get(other).is_some_and(|a| {
                a.drone_bay_runtime
                    .values()
                    .any(|r| r.drone_id == d.id && !r.cleanup)
                    || a.drone_receivers.values().any(|r| r.constrains(d.id))
            });
            // Constrained drones deliberately use Hold mode. Bound their
            // accepted peak speed independently of name iteration order,
            // including a peer already sampled to `now` in this same tick.
            let reach = if constrained {
                40. * (carrier_lookback_ms.max(now.saturating_sub(d.updated_at_ms)) as f64 / 1000.)
                    .min(2.)
            } else if moving {
                drone_speed(d.class) * (now.saturating_sub(d.updated_at_ms) as f64 / 1000.).min(10.)
            } else {
                0.
            };
            !(other == name && d.id == drone) && sphere(d.position, drone_radius() + reach)
        }) {
            bail!("bay route obstructed by another active drone");
        }
    }
    ensure!(
        !w.asteroids
            .iter()
            .any(|a| a.system_id == p.view.system_id && sphere(a.position, a.radius)),
        "bay route obstructed by an asteroid"
    );
    let planet = w.planet();
    ensure!(
        planet.system_id != p.view.system_id || !sphere(planet.center, planet.radius_m),
        "bay route obstructed by planetary surface"
    );
    ensure!(
        !sphere(STATION_CENTER_POSITION, STATION_DIAMETER_M / 2.),
        "bay route obstructed by station clearance"
    );

    Ok(())
}

pub(crate) fn execute(
    w: &mut World,
    name: &str,
    request: &DroneBayRequest,
    now: u64,
    automated: bool,
) -> Result<()> {
    if batches::handles(request) {
        ensure!(!automated, "Resource jobs do not create manual bay batches");
        return batches::execute(w, name, request, now);
    }
    execute_inner(w, name, request, now, automated, false)?;
    batches::individual_control(w, name, request, now);
    Ok(())
}
pub(super) fn execute_queued(
    w: &mut World,
    name: &str,
    request: &DroneBayRequest,
    now: u64,
) -> Result<()> {
    execute_inner(w, name, request, now, false, true)
}
fn execute_inner(
    w: &mut World,
    name: &str,
    request: &DroneBayRequest,
    now: u64,
    automated: bool,
    queued: bool,
) -> Result<()> {
    let id = vessel(w, name)
        .context("physical drone stock unavailable in this world; no legacy migration")?;
    let binding = state(w, name).unwrap().binding.clone();
    let definition = definition(w, name)?;
    ensure!(
        request.binding() == &binding,
        "bay command names a different owned vessel or source"
    );
    match request {
        DroneBayRequest::BeginBatch { .. }
        | DroneBayRequest::HoldBatch { .. }
        | DroneBayRequest::ResumeBatch { .. }
        | DroneBayRequest::CancelBatch { .. } => bail!("batch request requires queue dispatch"),
        DroneBayRequest::Launch {
            drone_id,
            operation_id,
            ..
        }
        | DroneBayRequest::Recover {
            drone_id,
            operation_id,
            ..
        } => {
            let kind = if matches!(request, DroneBayRequest::Launch { .. }) {
                DroneBayOperationKind::Launch
            } else {
                DroneBayOperationKind::Recover
            };
            if retry(
                state(w, name).unwrap(),
                &binding,
                *operation_id,
                *drone_id,
                kind,
            )? {
                return Ok(());
            }
            ensure!(
                !drone_cargo::busy(w, name),
                "ore receiver owns the nearby handling route"
            );
            ensure!(
                kind != DroneBayOperationKind::Recover
                    || drone_cargo::payload(w, name, *drone_id) == 0,
                "Drop off ore first, or use Recall All to unload then stow"
            );
            ensure!(
                queued || batches::active(w, name).is_none(),
                "A bay batch owns this vessel; use its Hold/Resume/Cancel Remaining controls"
            );
            ensure!(
                !busy(w, name),
                "bay handling channel is occupied or held; resume/cancel its operation first"
            );
            ensure!(
                automated
                    || w.resource_jobs
                        .get(name)
                        .is_none_or(|j| j.view.phase.is_terminal()),
                "cancel the active resource job before manual bay handling"
            );
            let parent = parent(w, name)?;
            let entry = state(w, name)
                .unwrap()
                .stock
                .iter()
                .find(|e| e.drone_id == *drone_id)
                .context("drone not owned by this vessel")?
                .clone();
            let route = match kind {
                DroneBayOperationKind::Launch => definition.launch_route(entry.home_cradle),
                DroneBayOperationKind::Recover => definition.recovery_route(entry.home_cradle),
            }
            .context("unsupported cradle route")?;
            let mut points = Vec::new();
            let mut phases = Vec::new();
            if kind == DroneBayOperationKind::Recover {
                let d = w.pilots[name]
                    .view
                    .drones
                    .iter()
                    .find(|d| d.id == *drone_id)
                    .context("owned active drone missing")?;
                let pose = BayPose {
                    position_m: d.position,
                    orientation_xyzw: d
                        .orientation_xyzw
                        .context("physical drone attitude missing")?,
                };
                let body = inverse(parent)?
                    .compose(pose)
                    .context("invalid recovery body pose")?;
                for pose in approach_points(definition, body, w.drone_cargo_registration.is_some())?
                {
                    let mut frame = Frame::from_route(route.keyframes[0]);
                    frame.drone = pose;
                    points.push(frame);
                }
                phases.resize(points.len() - 1, DroneBayPhase::Returning);
            } else {
                points.push(Frame::from_route(route.keyframes[0]));
            }
            for (i, frame) in route.keyframes.iter().enumerate().skip(1) {
                points.push(Frame::from_route(*frame));
                phases.push(phase(kind, i - 1));
            }
            if kind == DroneBayOperationKind::Launch {
                let mut egress = points.last().unwrap().clone();
                egress.drone.position_m = definition
                    .slot(entry.home_cradle)
                    .context("unregistered egress cradle")?
                    .external_clear_pose
                    .position_m;
                let direction = std::array::from_fn(|i| {
                    egress.drone.position_m[i] - points.last().unwrap().drone.position_m[i]
                });
                egress.drone = egress
                    .drone
                    .with_forward(direction)
                    .context("invalid aft egress attitude")?;
                points.push(egress);
                phases.push(DroneBayPhase::Departing);
            }
            obstruction(
                w,
                name,
                *drone_id,
                parent,
                &points[0],
                &points[1],
                ObstacleWindow::at(now),
            )?;
            let duration_ms = duration(
                definition,
                &points[0],
                &points[1],
                phases[0] == DroneBayPhase::Returning,
                entry.class,
            )?;
            let job_id = automated
                .then(|| w.resource_jobs.get(name).map(|j| j.view.id))
                .flatten();
            let a = w.accounts.get_mut(name).unwrap();
            let s = a.drone_bays.get_mut(&id).unwrap();
            let p = w.pilots.get_mut(name).unwrap();
            match kind {
                DroneBayOperationKind::Launch => {
                    launch(
                        s,
                        &mut p.view.drones,
                        request,
                        parent
                            .compose(points[0].drone)
                            .context("invalid launch world pose")?,
                        parent.position_m,
                        job_id,
                        now,
                    )?;
                }
                DroneBayOperationKind::Recover => {
                    recover(s, &mut p.view.drones, request, job_id, now)?;
                    s.handling_operation = Some(*operation_id);
                }
            }
            a.drone_bay_runtime.insert(
                id,
                Runtime {
                    operation_id: *operation_id,
                    drone_id: *drone_id,
                    parent,
                    system_id: p.view.system_id,
                    source: points[0].clone(),
                    confirmed: points[0].clone(),
                    points,
                    phases,
                    index: 0,
                    started_at_ms: now,
                    duration_ms,
                    updated_at_ms: now,
                    held: false,
                    cancelling: false,
                    cleanup: false,
                    ready_to_finalize: false,
                    capture_at_end: kind == DroneBayOperationKind::Recover,
                },
            );
            if automated && kind == DroneBayOperationKind::Launch {
                resource_jobs::record_bay_launch(w, name, *drone_id, *operation_id)?;
            }
            drones::sync(w.pilots.get_mut(name).unwrap());
        }
        DroneBayRequest::Hold { operation_id, .. } => {
            ensure!(
                state(w, name).unwrap().operation(*operation_id).is_some(),
                "bay operation missing"
            );
            if w.accounts[name]
                .drone_bay_runtime
                .get(&id)
                .is_some_and(|r| r.operation_id == *operation_id)
            {
                hold(w, name, now, "Bay handling held by pilot");
            }
        }
        DroneBayRequest::Resume { operation_id, .. } => {
            ensure!(
                state(w, name).unwrap().operation(*operation_id).is_some(),
                "bay operation missing"
            );
            let Some(run) = w.accounts[name].drone_bay_runtime.get(&id) else {
                return Ok(());
            };
            ensure!(
                run.operation_id == *operation_id,
                "another bay operation owns handling"
            );
            if !run.held {
                return Ok(());
            }
            let pose = parent(w, name)?;
            ensure!(pose == run.parent, "carrier moved while bay held");
            obstruction(
                w,
                name,
                run.drone_id,
                pose,
                &run.confirmed,
                &run.points[run.index + 1],
                ObstacleWindow::at(now),
            )?;
            let class = state(w, name)
                .unwrap()
                .stock
                .iter()
                .find(|e| e.drone_id == run.drone_id)
                .unwrap()
                .class;
            let a = w.accounts.get_mut(name).unwrap();
            let run = a.drone_bay_runtime.get_mut(&id).unwrap();
            run.source = run.confirmed.clone();
            run.duration_ms = duration(
                definition,
                &run.source,
                &run.points[run.index + 1],
                run.phases[run.index] == DroneBayPhase::Returning,
                class,
            )?;
            run.started_at_ms = now;
            run.updated_at_ms = now;
            run.held = false;
            let s = a.drone_bays.get_mut(&id).unwrap();
            if let Some(c) = s.cleanup.as_mut() {
                c.held = false;
                c.reason = "Returning empty machinery to safe idle".into();
            } else {
                let op = s
                    .operations
                    .iter_mut()
                    .find(|o| o.operation_id == *operation_id)
                    .unwrap();
                op.phase = op
                    .held_phase
                    .take()
                    .context("held operation phase missing")?;
                op.updated_at_ms = now;
                op.message = if run.cancelling {
                    "Cancelling: reversing accepted bay movement safely".into()
                } else {
                    op.phase.label().into()
                };
            }
            if let Some(d) = w
                .pilots
                .get_mut(name)
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == run.drone_id)
            {
                d.updated_at_ms = now;
                d.velocity = [0.; 3];
            }
            drones::sync(w.pilots.get_mut(name).unwrap());
        }
        DroneBayRequest::Cancel { operation_id, .. } => {
            ensure!(
                state(w, name).unwrap().operation(*operation_id).is_some(),
                "bay operation missing"
            );
            let Some(run) = w.accounts[name].drone_bay_runtime.get(&id) else {
                return Ok(());
            };
            ensure!(
                run.operation_id == *operation_id,
                "another bay operation owns handling"
            );
            if run.cancelling || run.cleanup {
                return Ok(());
            }
            ensure!(
                parent(w, name)? == run.parent,
                "carrier moved while bay held"
            );
            obstruction(
                w,
                name,
                run.drone_id,
                run.parent,
                &run.confirmed,
                &run.points[run.index],
                ObstacleWindow::at(now),
            )?;
            let a = w.accounts.get_mut(name).unwrap();
            let run = a.drone_bay_runtime.get_mut(&id).unwrap();
            reverse(definition, run, now)?;
            run.cancelling = true;
            run.capture_at_end = !run.capture_at_end;
            let s = a.drone_bays.get_mut(&id).unwrap();
            let op = s
                .operations
                .iter_mut()
                .find(|o| o.operation_id == *operation_id)
                .unwrap();
            op.phase = run.phases[0];
            op.held_phase = None;
            op.updated_at_ms = now;
            op.message = "Cancelling: reversing accepted bay movement safely".into();
            if let Some(d) = w
                .pilots
                .get_mut(name)
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == run.drone_id)
            {
                d.velocity = [0.; 3];
                d.updated_at_ms = now;
            }
            drones::sync(w.pilots.get_mut(name).unwrap());
        }
    }
    project(w)?;
    Ok(())
}
fn reverse(definition: &OfficialBayDefinition, run: &mut Runtime, now: u64) -> Result<()> {
    let mut points = vec![run.confirmed.clone()];
    points.extend(run.points[..=run.index].iter().rev().cloned());
    run.points = points;
    run.index = 0;
    run.source = run.confirmed.clone();
    run.phases = vec![DroneBayPhase::TransferringIn; run.points.len() - 1];
    run.started_at_ms = now;
    run.updated_at_ms = now;
    run.duration_ms = duration(
        definition,
        &run.source,
        &run.points[1],
        false,
        DroneClass::Recon,
    )?;
    run.held = false;
    run.ready_to_finalize = false;
    Ok(())
}

pub(crate) fn hold(w: &mut World, name: &str, now: u64, reason: &str) {
    batches::hold(w, name, now, reason);
    let Some(id) = vessel(w, name) else {
        return;
    };
    let a = w.accounts.get_mut(name).unwrap();
    let mut constrained_drone = None;
    if let Some(run) = a.drone_bay_runtime.get_mut(&id) {
        if !run.cleanup {
            constrained_drone = Some(run.drone_id);
        }
        run.held = true;
        run.updated_at_ms = now;
        let s = a.drone_bays.get_mut(&id).unwrap();
        if let Some(c) = s.cleanup.as_mut() {
            c.held = true;
            c.reason = reason.into();
        } else if let Some(op) = s
            .operations
            .iter_mut()
            .find(|o| o.operation_id == run.operation_id)
        {
            if op.phase != DroneBayPhase::Held {
                op.held_phase = Some(op.phase);
                op.phase = DroneBayPhase::Held;
            }
            op.updated_at_ms = now;
            op.message = reason.into();
        }
    }
    if let Some(p) = w.pilots.get_mut(name) {
        for d in p
            .view
            .drones
            .iter_mut()
            .filter(|d| Some(d.id) == constrained_drone)
        {
            d.velocity = [0.; 3];
            d.mode = DroneMode::Hold;
            d.mining_cycle = None;
            d.attack_ready_at_ms = None;
            d.updated_at_ms = now;
        }
        drones::sync(p);
    }
}

/// Explicit lifecycle stop for restart or ordinary last-source-session loss.
/// An identified bay Hold never mutates unrelated free entities.
pub(crate) fn stop_all(w: &mut World, name: &str, now: u64, reason: &str) {
    if state(w, name).is_none() {
        return;
    }
    hold(w, name, now, reason);
    if let Some(p) = w.pilots.get_mut(name) {
        for d in &mut p.view.drones {
            d.velocity = [0.; 3];
            d.mode = DroneMode::Hold;
            d.mining_cycle = None;
            d.attack_ready_at_ms = None;
            d.updated_at_ms = now;
        }
        drones::sync(p);
    }
}

pub(crate) fn advance(w: &mut World, now: u64, origins: &CarrierOrigins) -> Result<()> {
    advance_with_deferred_batch(w, now, origins, None)
}
/// Identified controls must inspect the committed queue before a pending member
/// is started. Accepted physical movement still advances; only new admission is
/// deferred for this requesting vessel, never for unrelated pilots.
pub(crate) fn advance_with_deferred_batch(
    w: &mut World,
    now: u64,
    origins: &CarrierOrigins,
    defer: Option<&str>,
) -> Result<()> {
    batches::advance(w, now, defer)?;
    let returning: Vec<_> = w
        .pilots
        .iter()
        .filter(|(name, _)| {
            state(w, name).is_some() && !busy(w, name) && batches::active(w, name).is_none()
        })
        .filter_map(|(name, p)| {
            p.view
                .drones
                .iter()
                .find(|d| d.mode == DroneMode::Returning)
                .map(|d| (name.clone(), d.id))
        })
        .collect();
    for (name, drone_id) in returning {
        let s = state(w, &name).unwrap();
        if let Some(operation_id) = s.high_watermark.checked_add(1) {
            let request = DroneBayRequest::Recover {
                binding: s.binding.clone(),
                drone_id,
                operation_id,
            };
            // An obstructed autonomous attempt must not partially mutate the
            // accepted world. Retry after carrier/obstacle state changes.
            let mut candidate = w.clone();
            if execute(&mut candidate, &name, &request, now, true).is_ok() {
                *w = candidate;
            }
        }
    }
    let names: Vec<_> = w.pilots.keys().filter(|n| busy(w, n)).cloned().collect();
    for name in names {
        let definition = definition(w, &name)?;
        let id = vessel(w, &name).unwrap();
        let run = w.accounts[&name].drone_bay_runtime[&id].clone();
        if run.held || run.ready_to_finalize {
            continue;
        }
        if now < run.updated_at_ms || now - run.updated_at_ms > MAX_ADVANCE_GAP_MS {
            hold(
                w,
                &name,
                now,
                "Bay held after an interrupted authority clock; resume explicitly",
            );
            continue;
        }
        if parent(w, &name).is_err()
            || w.pilots[&name].view.system_id != run.system_id
            || parent(w, &name).ok() != Some(run.parent)
        {
            hold(w, &name, now, "Carrier motion changed; bay safely held");
            continue;
        }
        let target = &run.points[run.index + 1];
        if let Err(reason) = obstruction(
            w,
            &name,
            run.drone_id,
            run.parent,
            &run.confirmed,
            target,
            ObstacleWindow {
                now,
                lookback_ms: now.saturating_sub(run.updated_at_ms),
                carriers: Some(origins),
            },
        ) {
            hold(w, &name, now, &reason.to_string());
            continue;
        }
        let next = run
            .source
            .sample(target, run.started_at_ms, run.duration_ms, now)?;
        let end = run
            .started_at_ms
            .checked_add(run.duration_ms)
            .context("bay timing overflow")?;
        let class = state(w, &name)
            .unwrap()
            .stock
            .iter()
            .find(|e| e.drone_id == run.drone_id)
            .unwrap()
            .class;
        let a = w.accounts.get_mut(&name).unwrap();
        let current = a.drone_bay_runtime.get_mut(&id).unwrap();
        current.confirmed = next;
        current.updated_at_ms = now;
        if now >= end {
            current.index += 1;
            if current.index + 1 == current.points.len() {
                current.ready_to_finalize = true;
            } else {
                current.source = current.confirmed.clone();
                current.started_at_ms = now;
                current.duration_ms = duration(
                    definition,
                    &current.source,
                    &current.points[current.index + 1],
                    current.phases[current.index] == DroneBayPhase::Returning,
                    class,
                )?;
            }
        }
        let s = a.drone_bays.get_mut(&id).unwrap();
        if !current.cleanup {
            let op = s
                .operations
                .iter_mut()
                .find(|o| o.operation_id == current.operation_id)
                .unwrap();
            op.updated_at_ms = now;
            if !current.ready_to_finalize {
                op.phase = current.phases[current.index];
            }
            op.message = if current.cancelling {
                "Cancelling: reversing accepted bay movement safely".into()
            } else if op.kind == DroneBayOperationKind::Launch
                && current.index >= 9
                && current.index < 17
            {
                "Securing launch machinery".into()
            } else {
                op.phase.label().into()
            };
            let pose = current
                .parent
                .compose(current.confirmed.drone)
                .context("invalid sampled world pose")?;
            let d = w
                .pilots
                .get_mut(&name)
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|d| d.id == current.drone_id)
                .context("constrained drone missing")?;
            d.position = pose.position_m;
            d.orientation_xyzw = Some(pose.orientation_xyzw);
            d.heading = pose
                .transform_vector([1., 0., 0.])
                .context("invalid drone forward")?;
            // Constrained velocity is supplied by the accepted sampler, not
            // free-flight extrapolation. Exact end points remain stationary.
            let (_, body_velocity) = BayMotionSegment {
                source: run.source.drone,
                target: target.drone,
                started_at_ms: run.started_at_ms,
                duration_ms: run.duration_ms,
            }
            .sample(now)
            .context("invalid route velocity")?;
            d.velocity = current
                .parent
                .transform_vector(body_velocity)
                .context("invalid route world velocity")?;
            d.mode = DroneMode::Hold;
            d.mining_cycle = None;
            d.attack_ready_at_ms = None;
            d.updated_at_ms = now;
            drones::sync(w.pilots.get_mut(&name).unwrap());
        }
    }
    project(w)
}
/// Called by the drone combat step after simultaneous incoming hits resolve,
/// before any HP-zero object is removed or any recovering entity is captured.
pub(crate) fn finish(w: &mut World, now: u64) -> Result<()> {
    let names: Vec<_> = w
        .pilots
        .keys()
        .filter(|n| state(w, n).is_some())
        .cloned()
        .collect();
    for name in names {
        let definition = definition(w, &name)?;
        let id = vessel(w, &name).unwrap();
        let dead: Vec<_> = w.pilots[&name]
            .view
            .drones
            .iter()
            .filter(|d| d.hit_points == 0)
            .map(|d| d.id)
            .collect();
        for drone in dead {
            let a = w.accounts.get_mut(&name).unwrap();
            let s = a.drone_bays.get_mut(&id).unwrap();
            destroy(
                s,
                &mut w.pilots.get_mut(&name).unwrap().view.drones,
                drone,
                now,
            )?;
            if let Some(run) = a
                .drone_bay_runtime
                .get_mut(&id)
                .filter(|r| r.drone_id == drone && !r.cleanup)
            {
                // Cancellation already follows a checked path TO idle. Reversing
                // it again would end at an unsafe mid-route cancellation origin.
                // Otherwise retrace the accepted outward/inward route to idle.
                let was_held = run.held;
                if !run.cancelling {
                    reverse(definition, run, now)?;
                }
                run.cleanup = true;
                run.capture_at_end = false;
                run.held = was_held;
                s.cleanup = Some(DroneBayCleanupView {
                    operation_id: run.operation_id,
                    held: was_held,
                    reason: if was_held {
                        "Drone destroyed; empty machinery held—resume to secure bay"
                    } else {
                        "Drone destroyed; returning empty handling machinery safely"
                    }
                    .into(),
                });
            }
        }
        let Some(run) = w.accounts[&name].drone_bay_runtime.get(&id).cloned() else {
            drones::sync(w.pilots.get_mut(&name).unwrap());
            continue;
        };
        if !run.ready_to_finalize || run.held {
            drones::sync(w.pilots.get_mut(&name).unwrap());
            continue;
        }
        let a = w.accounts.get_mut(&name).unwrap();
        let s = a.drone_bays.get_mut(&id).unwrap();
        if run.cleanup {
            s.cleanup = None;
        } else if run.capture_at_end {
            let op = s
                .operations
                .iter_mut()
                .find(|o| o.operation_id == run.operation_id)
                .unwrap();
            op.phase = DroneBayPhase::SecuringCradle;
            capture(
                s,
                &mut w.pilots.get_mut(&name).unwrap().view.drones,
                run.operation_id,
                run.cancelling,
                now,
            )?;
        } else if run.cancelling {
            let op = s
                .operations
                .iter_mut()
                .find(|o| o.operation_id == run.operation_id)
                .unwrap();
            op.phase = DroneBayPhase::Cancelled;
            op.held_phase = None;
            op.updated_at_ms = now;
            op.message =
                "Recovery cancelled; same drone remains active at its accepted outside position"
                    .into();
            s.handling_operation = None;
        } else {
            s.operations
                .iter_mut()
                .find(|o| o.operation_id == run.operation_id)
                .unwrap()
                .phase = DroneBayPhase::Departing;
            release(s, run.operation_id, now)?;
        }
        a.drone_bay_runtime.remove(&id);
        if let Some(d) = w
            .pilots
            .get_mut(&name)
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == run.drone_id)
        {
            d.velocity = [0.; 3];
            d.mode = DroneMode::Hold;
            d.updated_at_ms = now;
        }
        drones::sync(w.pilots.get_mut(&name).unwrap());
    }
    batches::reconcile(w, now)?;
    project(w)
}

pub(crate) fn project(w: &mut World) -> Result<()> {
    if w.independent_bay_registration.is_some() {
        for p in w.pilots.values_mut() {
            p.view.drone_bay = None;
        }
        return Ok(());
    }
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    for name in names {
        let Some(s) = state(w, &name) else {
            w.pilots.get_mut(&name).unwrap().view.drone_bay = None;
            continue;
        };
        let definition = definition(w, &name)?;
        let active = &w.pilots[&name].view.drones;
        let run = w.accounts[&name]
            .drone_bay_runtime
            .get(&s.binding.vessel_id);
        let mut machinery = DroneBayMachineryView {
            cleanup: s.cleanup.clone(),
            handling_operation: s.handling_operation,
            door_fraction: 0.,
            door_segment: None,
            groups: definition
                .mechanisms
                .iter()
                .map(|group| DroneBayMechanismView {
                    semantic_id: group.semantic_id.into(),
                    confirmed_pose: group.reference_pose,
                    segment: None,
                })
                .collect(),
        };
        let mut motions = vec![];
        if let Some(run) = run {
            machinery.door_fraction = run.confirmed.door(definition);
            let segment = |a, b| {
                (!run.held && !run.ready_to_finalize).then_some(BayMotionSegment {
                    source: a,
                    target: b,
                    started_at_ms: run.started_at_ms,
                    duration_ms: run.duration_ms,
                })
            };
            let target = &run.points[(run.index + 1).min(run.points.len() - 1)];
            for (i, group) in machinery.groups.iter_mut().enumerate() {
                group.confirmed_pose = run.confirmed.groups[i];
                group.segment = segment(run.source.groups[i], target.groups[i]);
            }
            machinery.door_segment =
                (!run.held && !run.ready_to_finalize).then_some(DroneBayScalarSegment {
                    from: run.source.door(definition),
                    to: target.door(definition),
                    started_at_ms: run.started_at_ms,
                    duration_ms: run.duration_ms,
                });
            if !run.cleanup {
                let op = s
                    .operation(run.operation_id)
                    .context("live bay operation missing")?;
                motions.push(DroneBayMotionView {
                    drone_id: run.drone_id,
                    operation_id: run.operation_id,
                    phase: op.phase,
                    frame: DroneBayMotionFrame::Body,
                    confirmed_pose: run.confirmed.drone,
                    segment: segment(run.source.drone, target.drone),
                    updated_at_ms: run.updated_at_ms,
                });
            }
        }
        let projection = DroneBayPublicState {
            batch_capability: s.batches.as_ref().map(|j| j.version),
            batch: s
                .batches
                .as_ref()
                .and_then(|j| j.operations.last())
                .cloned(),
            schema_version: s.schema_version,
            binding: s.binding.clone(),
            machinery,
            motions,
            cradles: s
                .stock
                .iter()
                .map(|e| {
                    let (presence, hit_points) = match e.status {
                        DroneStockStatus::Stored { hit_points } => {
                            (DroneBayPresence::Stored, hit_points)
                        }
                        DroneStockStatus::Destroyed { .. } => (DroneBayPresence::Destroyed, 0),
                        DroneStockStatus::Active => (
                            DroneBayPresence::Active,
                            active
                                .iter()
                                .find(|d| d.id == e.drone_id)
                                .map_or(0, |d| d.hit_points),
                        ),
                    };
                    DroneCradleView {
                        drone_id: e.drone_id,
                        class: e.class,
                        hit_points,
                        home_cradle: e.home_cradle,
                        presence,
                        body_pose: definition
                            .slot(e.home_cradle)
                            .expect("validated cradle")
                            .home_pose,
                    }
                })
                .collect(),
        };
        projection
            .validate_projection(
                &s.binding,
                &definition.home_poses(),
                &definition.mechanism_ids(),
                active,
            )
            .map_err(anyhow::Error::msg)?;
        w.pilots.get_mut(&name).unwrap().view.drone_bay = Some(projection);
    }
    Ok(())
}

pub(crate) fn authorize(w: &World, name: &str, request: &Request) -> Result<()> {
    let Some(s) = state(w, name) else {
        return Ok(());
    };
    let reserved = busy(w, name) || batches::active(w, name).is_some();
    let deployed = s.counts(None).active > 0;
    if let Some(batch) = batches::active(w, name) {
        ensure!(
            !matches!(request, Request::StartResourceJob { .. }),
            "Cancel the bay batch before starting a resource job"
        );
        let member_id = match request {
            Request::DroneNavigateById { id, .. }
            | Request::AssignMiningDrone { id, .. }
            | Request::AssignAttackDrone { id, .. }
            | Request::OrbitDrone { id, .. } => Some(*id),
            Request::DroneNavigate { .. } => w.pilots[name].view.drones.first().map(|d| d.id),
            _ => None,
        };
        ensure!(member_id.is_none_or(|id| !batch.members.iter().any(|m| m.drone_id == id && matches!(m.status, DroneBayBatchMemberStatus::Pending | DroneBayBatchMemberStatus::Active | DroneBayBatchMemberStatus::Servicing))),
            "This drone is reserved by the bay batch; pause/cancel the queue before issuing other work");
    }

    match request {
        Request::LaunchDrone | Request::LaunchDroneClass { .. } => bail!(
            "Physical bay uses identified stored drones; choose an owned drone and operation ID"
        ),
        Request::RecallDrone | Request::RecallDroneById { .. } => {
            bail!("Physical recovery requires an identified bay operation")
        }
        Request::Warp { .. }
        | Request::Jump { .. }
        | Request::Dock
        | Request::SelectShip { .. }
        | Request::BoardShip { .. }
            if deployed || reserved =>
        {
            bail!("Recover all owned drones and secure bay machinery before departure or docking")
        }
        Request::Undock if reserved => {
            bail!("Bay machinery is occupied or held; secure it before departure")
        }
        Request::Navigate { mode } if reserved && !matches!(mode, NavigationMode::Stop) => {
            bail!("Bay handling requires a stationary carrier; STOP remains available")
        }
        Request::PilotInput {
            translation,
            rotation,
        } if reserved && (translation.iter().chain(rotation).any(|v| *v != 0.)) => {
            bail!("Bay handling requires a stationary carrier; neutral input remains available")
        }
        Request::OrbitDrone { id, .. }
        | Request::DroneHoldById { id }
        | Request::DroneNavigateById { id, .. }
        | Request::AssignMiningDrone { id, .. }
        | Request::AssignAttackDrone { id, .. }
            if constrained(w, name, *id) =>
        {
            bail!("Drone is constrained by bay handling; use bay Hold, Resume or Cancel")
        }
        Request::DroneHold | Request::DroneNavigate { .. }
            if w.pilots[name]
                .view
                .drones
                .first()
                .is_some_and(|d| constrained(w, name, d.id)) =>
        {
            bail!("Drone is constrained by bay handling; use bay Hold, Resume or Cancel")
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn validate(w: &World) -> Result<()> {
    if w.drone_cargo_registration.is_none() {
        ensure!(
            w.accounts
                .values()
                .flat_map(|a| a.drone_bays.values())
                .flat_map(|s| s.batches.iter())
                .flat_map(|j| &j.operations)
                .all(|b| b.current_service_operation_id.is_none()
                    && b.members.iter().all(|m| m.service_operation_ids.is_empty()
                        && m.status != DroneBayBatchMemberStatus::Servicing)),
            "old world cannot contain ore service batch state"
        );
    }

    ensure!(
        w.drone_bay_batch_version.is_none()
            || (w.drone_bay_batch_version == Some(DRONE_BAY_BATCH_VERSION)
                && w.drone_bay_registration.is_some()),
        "unsupported durable bay batch feature stamp"
    );
    if let Some(reg) = &w.drone_bay_registration {
        ensure!(
            official_bay_definition(reg).is_some()
                && w.scene_layout == SceneLayoutRevision::DockingV1,
            "unsupported durable drone bay registration"
        );
    }
    let mut ids = std::collections::BTreeSet::new();
    for (name, a) in &w.accounts {
        if w.drone_bay_registration.is_some() {
            ensure!(a.vessel_id(ShipClass::Pioneer).is_some_and(|id|a.drone_bays.contains_key(id)),
                "fresh physical bay vessel lost its initialization receipt; never refill or fall back to free spawn");
        }
        ensure!(
            a.drone_bays.is_empty() || w.drone_bay_registration.is_some(),
            "drone stock lacks explicit fresh-world initialization stamp"
        );
        for (id, s) in &a.drone_bays {
            let definition = official_bay_for_binding(&s.binding)
                .context("unsupported stored bay definition")?;
            ensure!(
                id == &s.binding.vessel_id
                    && a.vessel_id(ShipClass::Pioneer) == Some(id.as_str())
                    && w.drone_bay_registration.as_ref() == Some(&definition.registration()),
                "drone stock has foreign vessel/source binding"
            );
            let pilot = w
                .pilots
                .get(name)
                .context("owned drone account has no pilot")?;
            let fleet = if pilot.view.ship_class == ShipClass::Pioneer {
                pilot.view.drones.as_slice()
            } else {
                &[]
            };
            validate_inventory(s, fleet, w.next_drone_id)?;
            ensure!(
                s.stock.iter().all(|entry| definition
                    .slot(entry.home_cradle)
                    .is_some_and(|slot| slot.expected_class == entry.class)),
                "owned stock differs from registered cradle class policy"
            );
            ensure!(
                (w.drone_bay_batch_version.is_some()
                    && s.schema_version == DRONE_BAY_SCHEMA_VERSION
                    && s.batches.is_some())
                    || (w.drone_bay_batch_version.is_none()
                        && s.schema_version == DRONE_BAY_LEGACY_SCHEMA_VERSION
                        && s.batches.is_none()),
                "bay batch initialization/schema does not match this immutable world"
            );
            ensure!(
                s.stock.iter().all(|e| ids.insert(e.drone_id)),
                "duplicate owned drone identity across vessels"
            );
            let run = a.drone_bay_runtime.get(id);
            ensure!(
                (s.handling_operation.is_some() || s.cleanup.is_some()) == run.is_some(),
                "bay reservation and runtime disagree"
            );
            if let Some(r) = run {
                ensure!(
                    r.parent.valid()
                        && r.points.len() >= 2
                        && r.points.len() <= 64
                        && r.phases.len() + 1 == r.points.len()
                        && r.index < r.points.len() - 1
                        && !r.ready_to_finalize
                        && r.points.iter().all(Frame::valid)
                        && r.source.valid()
                        && r.confirmed.valid()
                        && r.duration_ms > 0
                        && r.started_at_ms.checked_add(r.duration_ms).is_some(),
                    "invalid accepted bay runtime"
                );
                ensure!(!r.ready_to_finalize, "bay endpoint flag is inconsistent");
                let op = s
                    .operation(r.operation_id)
                    .context("runtime operation missing")?;
                ensure!(op.drone_id == r.drone_id, "runtime changed drone identity");
                if r.cleanup {
                    ensure!(
                        op.phase == DroneBayPhase::Destroyed
                            && s.handling_operation.is_none()
                            && s.cleanup.as_ref().is_some_and(
                                |c| c.operation_id == r.operation_id && c.held == r.held
                            ),
                        "invalid destroyed hardware cleanup"
                    );
                } else {
                    ensure!(
                        s.handling_operation == Some(r.operation_id)
                            && !op.phase.terminal()
                            && (op.phase == DroneBayPhase::Held) == r.held,
                        "invalid live bay reservation"
                    );
                }
            }
        }
        ensure!(
            a.drone_bay_runtime
                .keys()
                .all(|id| a.drone_bays.contains_key(id)),
            "orphaned bay runtime"
        );
    }
    for (name, p) in &w.pilots {
        if state(w, name).is_none() {
            ensure!(
                p.view.drones.iter().all(|d| ids.insert(d.id)),
                "legacy drone conflicts with finite owned stock"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod obstacle_tests {
    use super::*;
    #[test]
    fn conservative_sphere_contains_registered_drone_corners() {
        let radius = drone_radius();
        assert!(radius > 4.007);
        assert!((radius * radius - 16.06).abs() < 1e-12);
        for x in [-1., 1.] {
            for y in [-1., 1.] {
                for z in [-1., 1.] {
                    let h = layout::DRONE_BAY_DRONE_HALF_EXTENTS_M;
                    assert!(
                        (x * h[0]).powi(2) + (y * h[1]).powi(2) + (z * h[2]).powi(2)
                            <= radius * radius + 1e-12
                    );
                }
            }
        }
    }
    #[test]
    fn stopped_carrier_origin_and_constrained_hold_peer_bound_same_tick_sweeps() {
        use crate::drone_bay::integration_tests::Fixture;
        let mut f = Fixture::new();
        f.space();
        f.ok(f.request(DroneBayOperationKind::Launch, 1));
        let mut w = f.world();
        let source = w.accounts["Pilot"]
            .drone_bay_runtime
            .values()
            .next()
            .unwrap()
            .clone();
        let now = source.updated_at_ms;
        let parent = BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let mut a = source.confirmed.clone();
        a.drone.position_m = [1e6, 1e6, 1e6];
        let mut b = a.clone();
        b.drone.position_m[0] += 10.;
        let mut peer = w.pilots["Pilot"].clone();
        peer.view.name = "Peer".into();
        peer.view.drones.clear();
        let flight = peer.view.flight.as_mut().unwrap();
        flight.position = [1e6 + 200., 1e6, 1e6];
        flight.velocity = [0.; 3];
        flight.acceleration = [0.; 3];
        w.pilots.insert("Peer".into(), peer);
        assert!(obstruction(&w, "Pilot", 1, parent, &a, &b, ObstacleWindow::at(now)).is_ok());
        let origins = CarrierOrigins::from([(
            "Peer".into(),
            CarrierOrigin {
                system_id: w.pilots["Peer"].view.system_id,
                position: [1e6 - 200., 1e6, 1e6],
                travel_bound_m: 400.,
            },
        )]);
        assert!(obstruction(
            &w,
            "Pilot",
            1,
            parent,
            &a,
            &b,
            ObstacleWindow {
                now,
                lookback_ms: 1000,
                carriers: Some(&origins)
            }
        )
        .is_err());
        // Equal endpoints do not erase an Orbit/manual curved substep path.
        w.pilots
            .get_mut("Peer")
            .unwrap()
            .view
            .flight
            .as_mut()
            .unwrap()
            .mode = NavigationMode::Manual;
        w.pilots
            .get_mut("Peer")
            .unwrap()
            .view
            .flight
            .as_mut()
            .unwrap()
            .updated_at_ms = now - 1000;
        let curved_origins = carrier_origins(&w, now);
        w.pilots
            .get_mut("Peer")
            .unwrap()
            .view
            .flight
            .as_mut()
            .unwrap()
            .mode = NavigationMode::Stop;
        assert!(obstruction(
            &w,
            "Pilot",
            1,
            parent,
            &a,
            &b,
            ObstacleWindow {
                now,
                lookback_ms: 1000,
                carriers: Some(&curved_origins)
            }
        )
        .is_err());
        let flight = w
            .pilots
            .get_mut("Peer")
            .unwrap()
            .view
            .flight
            .as_mut()
            .unwrap();
        let docking = flight.docking.as_mut().unwrap();
        docking.phase = DockingPhase::Releasing;
        docking.segment = Some(DockingSegment {
            source_m: flight.position,
            target_m: flight.position,
            initial_velocity_mps: [0.; 3],
            source_orientation: flight.orientation,
            target_orientation: flight.orientation,
            started_at_ms: now - 1000,
            duration_ms: 1000,
        });
        let next_leg = flight::distance(
            docking_gate_position(docking.berth_id),
            docking_berth_position(docking.berth_id),
        );
        assert!(carrier_origins(&w, now)["Peer"].travel_bound_m >= next_leg);
        // Physically present docking traffic is not normal gameplay flight,
        // but must remain an obstacle while its docking controller owns it.
        let peer = w.pilots.get_mut("Peer").unwrap();
        peer.view.flight.as_mut().unwrap().position = a.drone.position_m;
        peer.view
            .flight
            .as_mut()
            .unwrap()
            .docking
            .as_mut()
            .unwrap()
            .phase = DockingPhase::Departing;
        assert!(!universe::normal(peer));
        assert!(obstruction(&w, "Pilot", 1, parent, &a, &b, ObstacleWindow::at(now)).is_err());
        w.pilots.get_mut("Peer").unwrap().view.location = Location::Docked;
        assert!(obstruction(&w, "Pilot", 1, parent, &a, &b, ObstacleWindow::at(now)).is_err());
        // A peer already advanced earlier in name order still has a possible
        // previous segment sweep, despite mode Hold and updated_at==now.
        let mut peer_drone = w.pilots["Pilot"].view.drones[0].clone();
        peer_drone.id = 999;
        peer_drone.position = [1e6 + 5., 1e6 + 20., 1e6];
        peer_drone.mode = DroneMode::Hold;
        peer_drone.updated_at_ms = now;
        w.pilots.get_mut("Peer").unwrap().view.flight = None;
        w.pilots.get_mut("Peer").unwrap().view.drones = vec![peer_drone];
        let mut account = w.accounts["Pilot"].clone();
        account
            .drone_bay_runtime
            .values_mut()
            .next()
            .unwrap()
            .drone_id = 999;
        w.accounts.insert("Peer".into(), account);
        assert!(obstruction(&w, "Pilot", 1, parent, &a, &b, ObstacleWindow::at(now)).is_ok());
        assert!(obstruction(
            &w,
            "Pilot",
            1,
            parent,
            &a,
            &b,
            ObstacleWindow {
                now,
                lookback_ms: 1000,
                carriers: None
            }
        )
        .is_err());
    }
}

#[cfg(test)]
mod definition_tests {
    use super::*;
    #[test]
    fn all_registered_handling_segment_durations_match_pre_refactor_baseline() {
        let baseline: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/arkline-protocol/src/drone_bay_reg12_route_baseline.json"
        )))
        .unwrap();
        for row in baseline["routes"].as_array().unwrap() {
            let slot = row["slot"].as_u64().unwrap() as u8;
            let route = match row["direction"].as_str().unwrap() {
                "Launch" => PIONEER_BAY_REG12.launch_route(slot).unwrap(),
                "Recovery" => PIONEER_BAY_REG12.recovery_route(slot).unwrap(),
                _ => unreachable!(),
            };
            for (index, pair) in route.keyframes.windows(2).enumerate() {
                for class in [DroneClass::Recon, DroneClass::Mining, DroneClass::Attack] {
                    let actual = duration(
                        &PIONEER_BAY_REG12,
                        &Frame::from_route(pair[0]),
                        &Frame::from_route(pair[1]),
                        false,
                        class,
                    )
                    .unwrap();
                    assert_eq!(
                        actual,
                        row["handling_durations_ms"][index].as_u64().unwrap(),
                        "slot{slot} {:?} segment{index}",
                        route.direction
                    );
                }
            }
        }
    }
    #[test]
    #[ignore = "requires actual e9ef pre-refactor disposable SQLite produced by the separate baseline executable"]
    fn reopens_pre_refactor_midstroke_world_without_rebinding_stock_or_pose() {
        let input = std::path::PathBuf::from(std::env::var("ARKLINE_BAY_BASELINE_WORLD").unwrap());
        let dir = std::env::temp_dir().join(format!(
            "arkline-bay-definition-reopen-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("world.sqlite");
        std::fs::copy(input, &path).unwrap();
        let db = rusqlite::Connection::open(&path).unwrap();
        let raw: String = db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let before: World = serde_json::from_str(&raw).unwrap();
        drop(db);
        let initial = state(&before, "Pilot").unwrap();
        let id = initial.binding.vessel_id.clone();
        let previous = &before.accounts["Pilot"].drone_bay_runtime[&id];
        assert!(!previous.held);
        assert!(previous.index + 1 < previous.points.len());
        let mut server = Server::open_editor_realm_with_scene(
            &path,
            default_pioneer_room_layout(),
            SceneLayoutRevision::DockingV1,
        )
        .unwrap();
        let raw: String = server
            .db
            .query_row("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
            .unwrap();
        let after: World = serde_json::from_str(&raw).unwrap();
        let current = state(&after, "Pilot").unwrap();
        assert_eq!(initial.binding, current.binding);
        assert_eq!(initial.stock, current.stock);
        assert_eq!(initial.initialization, current.initialization);
        assert_eq!(initial.high_watermark, current.high_watermark);
        assert_eq!(before.next_drone_id, after.next_drone_id);
        assert_eq!(before.drone_bay_registration, after.drone_bay_registration);
        assert_eq!(
            before.pilots["Pilot"].view.drones,
            after.pilots["Pilot"].view.drones
        );
        let held = &after.accounts["Pilot"].drone_bay_runtime[&id];
        assert!(held.held);
        assert_eq!(
            serde_json::to_value(&previous.confirmed).unwrap(),
            serde_json::to_value(&held.confirmed).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&previous.points).unwrap(),
            serde_json::to_value(&held.points).unwrap()
        );
        assert_eq!(previous.index, held.index);
        let now = server.editor_time.unwrap();
        server.tick_at(now + 1000).unwrap();
        let later: World = serde_json::from_str(
            &server
                .db
                .query_row::<String, _, _>("SELECT json FROM world WHERE id=1", [], |r| r.get(0))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(&held.confirmed).unwrap(),
            serde_json::to_value(&later.accounts["Pilot"].drone_bay_runtime[&id].confirmed)
                .unwrap()
        );
        assert_eq!(current.stock, state(&later, "Pilot").unwrap().stock);
        drop(server);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
