use super::*;
use crate::independent_bays::{
    independent_drone_radius, independent_world_to_body, prepare_independent_bay_body,
    prepared_independent_bay_sweeps_clear, IndependentBaySweptBody, PreparedIndependentBayBody,
};

pub(super) fn admit(w: &mut World, name: &str, now: u64) -> Result<()> {
    let Some(def) = definition(w)? else {
        return Ok(());
    };
    let current = state(w, name).context("Carrier ownership missing")?;
    let queued: Vec<_> = current
        .inventory
        .operations
        .iter()
        .filter(|o| o.phase == CarrierOperationPhase::Queued)
        .map(|o| o.operation_id)
        .collect();
    if queued.is_empty() {
        // Preserve the existing projection even when no owned state needs copying.
        drones::sync(w.pilots.get_mut(name).unwrap());
        return Ok(());
    }
    let mut s = current.clone();
    for id in queued {
        let mut candidate = s.clone();
        let op = candidate.inventory.operation(id).unwrap().clone();
        let e = candidate.inventory.entry(op.drone_id).unwrap().clone();
        match op.action {
            CarrierInventoryAction::Stage { .. } | CarrierInventoryAction::StoreReady { .. } => {
                if !reserve_stage(&mut candidate.inventory, &def.topology, id, now)? {
                    if matches!(op.action, CarrierInventoryAction::StoreReady { .. }) {
                        s.inventory
                            .operations
                            .iter_mut()
                            .find(|o| o.operation_id == id)
                            .unwrap()
                            .message =
                            "Queued: protected dock, cassette, loader or buffer is busy".into();
                    }
                    continue;
                }
            }
            CarrierInventoryAction::Launch => {
                let CarrierDroneLocation::Ready { berth_id } = e.location else {
                    bail!("Queued launch identity no longer Ready")
                };
                let cassette_id = candidate
                    .inventory
                    .docked_cassette(CarrierCassetteDock::Ready { berth_id })
                    .context("Ready cassette absent")?
                    .cassette_id;
                let resources = [
                    CarrierResource::ReadyBerth { berth_id },
                    CarrierResource::Cassette { cassette_id },
                ];
                if resources.iter().any(|r| candidate.inventory.reserved(*r)) {
                    continue;
                }
                for resource in resources {
                    candidate
                        .inventory
                        .reservations
                        .push(CarrierResourceReservation {
                            resource,
                            operation_id: id,
                            drone_id: op.drone_id,
                        });
                }
                let receipt = candidate
                    .inventory
                    .operations
                    .iter_mut()
                    .find(|o| o.operation_id == id)
                    .unwrap();
                receipt.berth_id = Some(berth_id);
                receipt.phase = CarrierOperationPhase::Launching;
                receipt.updated_at_ms = now;
                candidate
                    .inventory
                    .entries
                    .iter_mut()
                    .find(|e| e.drone_id == op.drone_id)
                    .unwrap()
                    .location = CarrierDroneLocation::Launching {
                    operation_id: id,
                    berth_id,
                };
            }
            CarrierInventoryAction::RecallToStorage
            | CarrierInventoryAction::ReturnToUnload { .. } => {
                ensure!(
                    matches!(e.location, CarrierDroneLocation::Deployed)
                        && !candidate.departures.contains_key(&e.drone_id),
                    "Capture cannot take an existing departure body"
                );
                let mut admitted = None;
                let host = parent(w, name)?;
                let live = w.pilots[name]
                    .view
                    .drones
                    .iter()
                    .find(|d| d.id == e.drone_id)
                    .context("Returning identity missing")?;
                let body = independent_world_to_body(
                    host,
                    BayPose {
                        position_m: live.position,
                        orientation_xyzw: live
                            .orientation_xyzw
                            .context("Returning attitude missing")?,
                    },
                )
                .context("Returning body transform")?;
                let mut berths: Vec<_> = def.topology.ready_berths.iter().collect();
                let distance = |id| {
                    def.docks
                        .iter()
                        .find(|d| d.dock == CarrierCassetteDock::Ready { berth_id: id })
                        .map(|d| {
                            (0..3)
                                .map(|axis| {
                                    (body.position_m[axis] - d.cassette_pose.position_m[axis])
                                        .powi(2)
                                })
                                .sum::<f64>()
                        })
                        .unwrap_or(f64::INFINITY)
                };
                berths.sort_by(|a, b| {
                    distance(a.berth_id)
                        .total_cmp(&distance(b.berth_id))
                        .then(a.berth_id.cmp(&b.berth_id))
                });
                for berth in berths {
                    if !berth.compatible_classes.contains(&e.class) {
                        continue;
                    }
                    let Some(c) = candidate
                        .inventory
                        .docked_cassette(CarrierCassetteDock::Ready {
                            berth_id: berth.berth_id,
                        })
                    else {
                        continue;
                    };
                    let cid = c.cassette_id;
                    let resources = [
                        CarrierResource::ReadyBerth {
                            berth_id: berth.berth_id,
                        },
                        CarrierResource::Cassette { cassette_id: cid },
                    ];
                    if c.drone_id.is_some()
                        || resources.iter().any(|r| candidate.inventory.reserved(*r))
                    {
                        continue;
                    }
                    let Ok(a) =
                        arrival::Arrival::begin(w, name, &candidate, id, cid, berth.berth_id, now)
                    else {
                        continue;
                    };
                    if !a.corridor_clear(&candidate.arrivals)? {
                        continue;
                    }
                    if !clear_bodies(
                        w,
                        name,
                        &candidate,
                        e.drone_id,
                        Some(id),
                        a.parent,
                        &[a.body()?],
                        now,
                        now,
                    )? {
                        continue;
                    }
                    admitted = Some((a, resources));
                    break;
                }
                let Some((a, resources)) = admitted else {
                    s.inventory
                        .operations
                        .iter_mut()
                        .find(|o| o.operation_id == id)
                        .unwrap()
                        .message = "Queued: no compatible empty Ready cassette with clear unreserved exterior corridor".into();
                    continue;
                };
                for resource in resources {
                    candidate
                        .inventory
                        .reservations
                        .push(CarrierResourceReservation {
                            resource,
                            operation_id: id,
                            drone_id: e.drone_id,
                        });
                }
                let op = candidate
                    .inventory
                    .operations
                    .iter_mut()
                    .find(|o| o.operation_id == id)
                    .unwrap();
                op.berth_id = Some(a.berth_id);
                op.phase = CarrierOperationPhase::Returning;
                op.updated_at_ms = now;
                op.message = "Returning through checked exterior approach".into();
                candidate
                    .inventory
                    .entries
                    .iter_mut()
                    .find(|x| x.drone_id == e.drone_id)
                    .unwrap()
                    .location = CarrierDroneLocation::Returning {
                    operation_id: id,
                    berth_id: a.berth_id,
                };
                candidate.drone_poses.insert(a.drone_id, a.confirmed);
                candidate.arrivals.insert(id, a);
                s = candidate;
                continue;
            }
        }
        let kind = run_kind(&candidate.inventory, id)?;
        let path = route(def, &kind, e.class)?;
        let entry = CarrierMotionFrame::from(&path[0]);
        let start = CarrierMotionStart {
            binding: candidate.inventory.binding.clone(),
            operation_id: id,
            drone_id: op.drone_id,
            class: e.class,
            kind,
            parent: parent(w, name)?,
            system_id: w.pilots[name].view.system_id,
            actual: actual(&candidate, &entry, op.drone_id)?,
            now,
        };
        let r = if geometry::lookup(&start.binding.registration).is_some() {
            CarrierMotionRuntime::begin(start)?
        } else {
            CarrierMotionRuntime::from_route(start, def, path)?
        };
        let initial_phase = r.route[r.target_index].phase;
        phase(
            &mut candidate,
            id,
            initial_phase,
            now,
            "Source route accepted",
        );
        candidate.runs.insert(id, r);
        if !clear(w, name, &candidate, &candidate.runs[&id], now)? {
            s.inventory
                .operations
                .iter_mut()
                .find(|o| o.operation_id == id)
                .unwrap()
                .message = "Queued: physical route blocked by an actual obstacle".into();
            continue;
        }
        s = candidate;
        if op.action == CarrierInventoryAction::Launch {
            let r = &s.runs[&id];
            let pose = r
                .parent
                .compose(s.drone_poses[&op.drone_id])
                .context("Launch transform")?;
            ensure!(
                !w.pilots[name]
                    .view
                    .drones
                    .iter()
                    .any(|d| d.id == op.drone_id),
                "Duplicate live drone identity"
            );
            w.pilots.get_mut(name).unwrap().view.drones.push(DroneView {
                id: op.drone_id,
                class: e.class,
                hit_points: e.hit_points,
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
        }
    }
    drones::sync(w.pilots.get_mut(name).unwrap());
    w.accounts.get_mut(name).unwrap().carrier_inventory = Some(s);
    Ok(())
}
fn body_bounds(def: &CarrierInventoryDefinition, key: &CarrierPoseKey) -> Result<[[f64; 3]; 2]> {
    Ok(match key {
        CarrierPoseKey::Drone => [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]],
        CarrierPoseKey::Cassette { cassette_id } => {
            def.cassettes
                .iter()
                .find(|c| c.cassette_id == *cassette_id)
                .context("Cassette envelope missing")?
                .bounds_local_m
        }
        CarrierPoseKey::Mechanism { semantic_id } => {
            def.mechanisms
                .iter()
                .find(|m| m.semantic_id == *semantic_id)
                .context("Mechanism envelope missing")?
                .local_bounds_m
        }
    })
}
pub(super) fn rounding_margin(a: [f64; 3], b: [f64; 3]) -> f64 {
    0.001 + 128. * f64::EPSILON * a.into_iter().chain(b).map(f64::abs).fold(1., f64::max)
}
fn stationary(p: BayPose, now: u64) -> BayMotionSegment {
    BayMotionSegment {
        source: p,
        target: p,
        started_at_ms: now,
        duration_ms: 1,
    }
}
/// A source-owned rigid root keeps its conservative aggregate broadphase. Only
/// exact certified part coverage may refine an overlap into continuous leaf tests.
#[derive(Clone)]
struct CachedSweep {
    body: IndependentBaySweptBody,
    prepared: std::cell::OnceCell<Option<PreparedIndependentBayBody>>,
}
impl CachedSweep {
    fn new(body: IndependentBaySweptBody) -> Self {
        Self {
            body,
            prepared: std::cell::OnceCell::new(),
        }
    }
    fn shape(&self) -> Option<&PreparedIndependentBayBody> {
        self.prepared
            .get_or_init(|| {
                #[cfg(test)]
                crate::tick_profile::count("shape_preparations");
                prepare_independent_bay_body(self.body)
            })
            .as_ref()
    }
}
#[derive(Clone)]
struct CarrierSweptBody {
    union: CachedSweep,
    leaves: Option<Vec<CachedSweep>>,
}
impl CarrierSweptBody {
    fn new(union: IndependentBaySweptBody, leaves: Option<Vec<IndependentBaySweptBody>>) -> Self {
        Self {
            union: CachedSweep::new(union),
            leaves: leaves.map(|p| p.into_iter().map(CachedSweep::new).collect()),
        }
    }
    fn single(union: IndependentBaySweptBody) -> Self {
        Self::new(union, None)
    }
    fn parts(&self) -> &[CachedSweep] {
        self.leaves
            .as_deref()
            .unwrap_or(std::slice::from_ref(&self.union))
    }
}
fn cached_sweeps_clear(
    a: &CachedSweep,
    b: &CachedSweep,
    start: u64,
    end: u64,
    margin: f64,
) -> bool {
    if end < start || !margin.is_finite() || margin < 0. {
        return false;
    }
    let (Some(a), Some(b)) = (a.shape(), b.shape()) else {
        return false;
    };
    prepared_independent_bay_sweeps_clear(a, b, start, end, margin)
}
fn compounds_clear(
    a: &CarrierSweptBody,
    b: &CarrierSweptBody,
    start: u64,
    end: u64,
    margin: f64,
) -> bool {
    if cached_sweeps_clear(&a.union, &b.union, start, end, margin) {
        return true;
    }
    a.parts().iter().all(|a| {
        b.parts()
            .iter()
            .all(|b| cached_sweeps_clear(a, b, start, end, margin))
    })
}

fn bodies(
    def: &CarrierInventoryDefinition,
    r: &CarrierMotionRuntime,
) -> Result<Vec<CarrierSweptBody>> {
    #[cfg(test)]
    crate::tick_profile::count("swept_bodies_build");
    let target = CarrierMotionFrame::from(&r.route[r.target_index.min(r.route.len() - 1)]);
    let compound = def.has_complete_compound_collision();
    ensure!(
        def.topology.registration.assembly_revision != 1703 || compound,
        "Parallel source requires complete certified collision parts"
    );
    let mut result = Vec::new();
    for (key, pose) in &r.confirmed.poses {
        if let CarrierPoseKey::Cassette { cassette_id } = key {
            if compound {
                let cassette = def
                    .cassettes
                    .iter()
                    .find(|c| c.cassette_id == *cassette_id)
                    .context("Cassette collision source missing")?;
                ensure!(
                    !cassette.mechanism_ids.is_empty(),
                    "Cassette physical coverage empty"
                );
                for id in &cassette.mechanism_ids {
                    let k = CarrierPoseKey::Mechanism {
                        semantic_id: id.clone(),
                    };
                    ensure!(
                        [&r.source, &r.confirmed, &target]
                            .iter()
                            .all(|frame| frame.poses.iter().any(|(key, _)| key == &k)),
                        "Cassette physical collision root missing: {id}"
                    );
                    ensure!(
                        def.mechanism_collision_parts(id)
                            .is_some_and(|parts| !parts.is_empty()),
                        "Cassette collision coverage missing: {id}"
                    );
                }
                // This key locates an identity, not an extra solid block inside
                // a hollow cassette. All actual solid roots above remain owned.
                continue;
            }
        }
        let segment = if r.held || r.waiting_for_transfer || r.ready_to_finalize {
            stationary(*pose, r.confirmed_at_ms)
        } else {
            BayMotionSegment {
                source: r
                    .source
                    .poses
                    .iter()
                    .find(|(k, _)| k == key)
                    .context("Source root missing")?
                    .1,
                target: target
                    .poses
                    .iter()
                    .find(|(k, _)| k == key)
                    .context("Target root missing")?
                    .1,
                started_at_ms: r.started_at_ms,
                duration_ms: r.duration_ms,
            }
        };
        let union = IndependentBaySweptBody {
            segment,
            local_bounds_m: body_bounds(def, key)?,
        };
        let leaves = if let CarrierPoseKey::Mechanism { semantic_id } = key {
            if compound {
                let parts = def
                    .mechanism_collision_parts(semantic_id)
                    .context("Certified mechanism parts missing")?;
                ensure!(!parts.is_empty(), "Certified mechanism parts empty");
                Some(
                    parts
                        .iter()
                        .map(|part| IndependentBaySweptBody {
                            segment,
                            local_bounds_m: part.local_bounds_m,
                        })
                        .collect(),
                )
            } else {
                None
            }
        } else {
            None
        };
        result.push(CarrierSweptBody::new(union, leaves));
    }
    Ok(result)
}
/// Scratch data borrows one immutable owner snapshot. Slots are filled in the
/// original query traversal order, so an unrelated malformed peer is not examined
/// early. No results, time intervals, world hazards or mutable owner state are cached.
pub(super) struct PeerSweeps<'a> {
    def: &'a CarrierInventoryDefinition,
    peers: &'a OwnedCarrier,
    runs: BTreeMap<u64, std::cell::OnceCell<Vec<CarrierSweptBody>>>,
}
impl<'a> PeerSweeps<'a> {
    fn new(def: &'a CarrierInventoryDefinition, peers: &'a OwnedCarrier) -> Self {
        Self {
            def,
            peers,
            runs: peers
                .runs
                .keys()
                .map(|id| (*id, std::cell::OnceCell::new()))
                .collect(),
        }
    }
    fn run(&self, id: u64) -> Result<&[CarrierSweptBody]> {
        let slot = &self.runs[&id];
        if slot.get().is_none() {
            let prepared = bodies(self.def, &self.peers.runs[&id])?;
            // This context is single-threaded and the slot cannot be filled by
            // body construction. Errors are not cached or reordered.
            assert!(slot.set(prepared).is_ok());
        }
        Ok(slot.get().unwrap())
    }
}
pub(super) fn clear(
    w: &World,
    name: &str,
    peers: &OwnedCarrier,
    r: &CarrierMotionRuntime,
    now: u64,
) -> Result<bool> {
    let context = PeerSweeps::new(definition(w)?.unwrap(), peers);
    clear_with_peers(w, name, &context, r, now)
}
fn clear_with_peers(
    w: &World,
    name: &str,
    context: &PeerSweeps<'_>,
    r: &CarrierMotionRuntime,
    now: u64,
) -> Result<bool> {
    #[cfg(test)]
    crate::tick_profile::count("carrier_clear");
    let ours = bodies(context.def, r)?;
    clear_compounds(
        w,
        name,
        context,
        if r.kind.writes_drone() { r.drone_id } else { 0 },
        Some(r.operation_id),
        r.parent,
        &ours,
        r.confirmed_at_ms,
        now.max(r.confirmed_at_ms),
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn clear_bodies(
    w: &World,
    name: &str,
    peers: &OwnedCarrier,
    drone_id: u64,
    operation_id: Option<u64>,
    host: BayPose,
    ours: &[IndependentBaySweptBody],
    start: u64,
    end: u64,
) -> Result<bool> {
    let context = PeerSweeps::new(definition(w)?.unwrap(), peers);
    clear_bodies_with_peers(
        w,
        name,
        &context,
        drone_id,
        operation_id,
        host,
        ours,
        start,
        end,
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn clear_bodies_with_peers(
    w: &World,
    name: &str,
    context: &PeerSweeps<'_>,
    drone_id: u64,
    operation_id: Option<u64>,
    host: BayPose,
    ours: &[IndependentBaySweptBody],
    start: u64,
    end: u64,
) -> Result<bool> {
    let ours: Vec<_> = ours.iter().copied().map(CarrierSweptBody::single).collect();
    clear_compounds(
        w,
        name,
        context,
        drone_id,
        operation_id,
        host,
        &ours,
        start,
        end,
    )
}
#[allow(clippy::too_many_arguments)]
fn clear_compounds(
    w: &World,
    name: &str,
    context: &PeerSweeps<'_>,
    drone_id: u64,
    operation_id: Option<u64>,
    host: BayPose,
    ours: &[CarrierSweptBody],
    start: u64,
    end: u64,
) -> Result<bool> {
    let def = context.def;
    let peers = context.peers;
    for other_id in peers.runs.keys() {
        if Some(*other_id) == operation_id {
            continue;
        }
        let other = context.run(*other_id)?;
        if ours.iter().any(|a| {
            other
                .iter()
                .any(|b| !compounds_clear(a, b, start, end, 0.001))
        }) {
            return Ok(false);
        }
    }
    for (id, arrival) in &peers.arrivals {
        if Some(*id) != operation_id {
            let other = CarrierSweptBody::single(arrival.body()?);
            if ours
                .iter()
                .any(|a| !compounds_clear(a, &other, start, end, 0.001))
            {
                return Ok(false);
            }
        }
    }
    for (id, departure) in &peers.departures {
        if *id != drone_id {
            let b = CarrierSweptBody::single(departure.body());
            if ours
                .iter()
                .any(|a| !compounds_clear(a, &b, start, end, 0.001))
            {
                return Ok(false);
            }
        }
    }
    for (peer_name, p) in &w.pilots {
        if p.view.system_id != w.pilots[name].view.system_id {
            continue;
        }
        for d in &p.view.drones {
            if peer_name == name
                && (d.id == drone_id
                    || peers
                        .runs
                        .values()
                        .any(|other| other.drone_id == d.id && other.kind.writes_drone())
                    || peers.departures.contains_key(&d.id)
                    || peers.arrivals.values().any(|a| a.drone_id == d.id))
            {
                continue;
            }
            let pose = independent_world_to_body(
                host,
                BayPose {
                    position_m: d.position,
                    orientation_xyzw: d
                        .orientation_xyzw
                        .context("Physical peer attitude missing")?,
                },
            )
            .context("Peer transform")?;
            let peer = if d.mode == DroneMode::Hold && d.velocity == [0.; 3] {
                IndependentBaySweptBody {
                    segment: stationary(pose, start),
                    local_bounds_m: [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]],
                }
            } else {
                let radius = independent_drone_radius([[-2.5, -0.9, -3.], [2.5, 0.9, 3.]]).unwrap()
                    + drone_speed(d.class)
                        * (end.saturating_sub(d.updated_at_ms).min(10_000) as f64 / 1000.);
                IndependentBaySweptBody {
                    segment: stationary(
                        BayPose {
                            orientation_xyzw: [0., 0., 0., 1.],
                            ..pose
                        },
                        start,
                    ),
                    local_bounds_m: [[-radius; 3], [radius; 3]],
                }
            };
            let peer = CarrierSweptBody::single(peer);
            if ours.iter().any(|a| {
                !compounds_clear(
                    a,
                    &peer,
                    start,
                    end,
                    rounding_margin(host.position_m, d.position),
                )
            }) {
                return Ok(false);
            }
        }
        if peer_name != name {
            if let Some(f) = &p.view.flight {
                let pose = independent_world_to_body(
                    host,
                    BayPose {
                        position_m: f.position,
                        orientation_xyzw: [0., 0., 0., 1.],
                    },
                )
                .context("Carrier peer transform")?;
                let radius = if let Some(envelope) = crate::docking::owner_envelope(w, peer_name)? {
                    envelope
                        .origin_radius_m()
                        .context("Exact carrier peer envelope")?
                } else if state(w, peer_name).is_some() {
                    // Historical registered fixtures retain their original consumer path.
                    independent_drone_radius(def.deployed_hardware_bounds_m)
                        .context("Registered carrier bounds")?
                } else {
                    hull_radius_m(p.view.ship_class)
                } + f.velocity.iter().map(|v| v * v).sum::<f64>().sqrt() * 2.;
                let peer = IndependentBaySweptBody {
                    segment: stationary(pose, start),
                    local_bounds_m: [[-radius; 3], [radius; 3]],
                };
                let peer = CarrierSweptBody::single(peer);
                if ours.iter().any(|a| {
                    !compounds_clear(
                        a,
                        &peer,
                        start,
                        end,
                        rounding_margin(host.position_m, f.position),
                    )
                }) {
                    return Ok(false);
                }
            }
        }
    }
    for a in w
        .asteroids
        .iter()
        .filter(|a| a.system_id == w.pilots[name].view.system_id)
    {
        let p = independent_world_to_body(
            host,
            BayPose {
                position_m: a.position,
                orientation_xyzw: [0., 0., 0., 1.],
            },
        )
        .context("Asteroid transform")?;
        let peer = IndependentBaySweptBody {
            segment: stationary(p, start),
            local_bounds_m: [[-a.radius; 3], [a.radius; 3]],
        };
        let margin = rounding_margin(host.position_m, a.position);
        let peer = CarrierSweptBody::single(peer);
        if ours
            .iter()
            .any(|a| !compounds_clear(a, &peer, start, end, margin))
        {
            return Ok(false);
        }
    }
    Ok(true)
}
fn project_drone(w: &mut World, name: &str, r: &CarrierMotionRuntime, now: u64) -> Result<()> {
    if !r.kind.writes_drone() {
        return Ok(());
    }
    let Some(d) = w
        .pilots
        .get_mut(name)
        .unwrap()
        .view
        .drones
        .iter_mut()
        .find(|d| d.id == r.drone_id)
    else {
        return Ok(());
    };
    let local = r
        .confirmed
        .poses
        .iter()
        .find(|(k, _)| *k == CarrierPoseKey::Drone)
        .context("Confirmed drone pose missing")?
        .1;
    let pose = r
        .parent
        .compose(local)
        .context("Confirmed world drone transform")?;
    d.position = pose.position_m;
    d.orientation_xyzw = Some(pose.orientation_xyzw);
    d.heading = pose.transform_vector([1., 0., 0.]).unwrap();
    d.velocity = [0.; 3];
    d.mode = DroneMode::Hold;
    d.mining_cycle = None;
    d.attack_ready_at_ms = None;
    d.updated_at_ms = now;
    Ok(())
}
/// Dispatch newly eligible finite work without advancing physical routes. Run
/// after lifecycle completion as well as before motion, so observing the same
/// accepted clock does not admit work that the authoritative tick left behind.
/// A pending captain carrier command keeps its original precedence.
pub(crate) fn dispatch(w: &mut World, now: u64, defer: Option<&str>) -> Result<()> {
    if definition(w)?.is_none() {
        return Ok(());
    }
    for name in w.pilots.keys().cloned().collect::<Vec<_>>() {
        if defer != Some(name.as_str()) {
            deployment::drive(w, &name, now)?;
            recall::drive(w, &name, now)?;
            admit(w, &name, now)?;
        }
    }
    Ok(())
}
pub(crate) fn advance(w: &mut World, now: u64, defer: Option<&str>) -> Result<()> {
    if definition(w)?.is_none() {
        return Ok(());
    }
    dispatch(w, now, defer)?;
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    let snapshot = w.clone();
    for name in names {
        let mut s = state(w, &name).context("Missing carrier state")?.clone();
        let immutable = s.clone();
        let context = PeerSweeps::new(definition(&snapshot)?.unwrap(), &immutable);
        let ids: Vec<_> = s.runs.keys().copied().collect();
        for id in ids {
            let allowed = clear_with_peers(&snapshot, &name, &context, &immutable.runs[&id], now)?;
            let r = s.runs.get_mut(&id).unwrap();
            let before = r.completed_stage_legs;
            let result = r.advance(now, allowed)?;
            let frame = r.confirmed.clone();
            let drone_id = r.drone_id;
            let completed = r.completed_stage_legs;
            let target = &r.route[r.target_index.min(r.route.len() - 1)];
            let carried = target.carried_cassette_id;
            let next_phase = target.phase;
            apply_frame(&mut s, drone_id, &frame);
            if result != CarrierMotionProgress::Held
                && s.inventory.operation(id).unwrap().stage.is_some()
            {
                // Ownership moves to InLoader as the exact accepted carry leg
                // begins, not at request time or from visual proximity.
                if before < 3 {
                    let (cid, _, _) = super::super::current_leg(&s.inventory, id)?;
                    if carried == Some(cid)
                        && matches!(
                            s.inventory
                                .cassettes
                                .iter()
                                .find(|c| c.cassette_id == cid)
                                .unwrap()
                                .location,
                            CarrierCassetteLocation::Docked { .. }
                        )
                    {
                        super::super::start_stage_leg(
                            &mut s.inventory,
                            &definition(w)?.unwrap().topology,
                            id,
                        )?;
                    }
                }
                if completed > before {
                    super::super::record_stage_endpoint(
                        &mut s.inventory,
                        &definition(w)?.unwrap().topology,
                        id,
                        now,
                    )?;
                }
            }
            if result == CarrierMotionProgress::Held {
                hold_stage(&mut s.inventory, id, now)?;
            } else if !s.inventory.operation(id).unwrap().phase.terminal() {
                phase(
                    &mut s,
                    id,
                    next_phase,
                    now,
                    "Advancing confirmed source route",
                );
            }
            if matches!(
                s.inventory.operation(id).unwrap().action,
                CarrierInventoryAction::ReturnToUnload { .. }
            ) && next_phase == CarrierOperationPhase::Servicing
                && !s.inventory.operation(id).unwrap().phase.terminal()
            {
                let berth_id = s.inventory.operation(id).unwrap().berth_id.unwrap();
                s.inventory
                    .entries
                    .iter_mut()
                    .find(|e| e.drone_id == drone_id)
                    .unwrap()
                    .location = CarrierDroneLocation::Servicing {
                    operation_id: id,
                    berth_id,
                };
            }
            project_drone(w, &name, &s.runs[&id], now)?;
        }
        service::reconcile(&mut s, now)?;
        arrival::advance(&snapshot, w, &name, &context, &mut s, now)?;
        departure::advance(&snapshot, w, &name, &context, &mut s, now)?;
        w.accounts.get_mut(&name).unwrap().carrier_inventory = Some(s);
        drones::sync(w.pilots.get_mut(&name).unwrap());
    }
    Ok(())
}
pub(crate) fn finish(w: &mut World, now: u64) -> Result<Vec<EconomicEvent>> {
    let Some(def) = definition(w)? else {
        return Ok(vec![]);
    };
    let mut events = Vec::new();
    let names: Vec<_> = w.pilots.keys().cloned().collect();
    for name in names {
        let mut s = state(w, &name).context("Carrier state missing")?.clone();
        for d in &w.pilots[&name].view.drones {
            if d.hit_points > 0 {
                s.inventory
                    .entries
                    .iter_mut()
                    .find(|e| e.drone_id == d.id)
                    .context("Foreign live drone")?
                    .hit_points = d.hit_points;
            }
        }
        let dead: Vec<_> = w.pilots[&name]
            .view
            .drones
            .iter()
            .filter(|d| d.hit_points == 0)
            .map(|d| d.id)
            .collect();
        for drone_id in dead {
            let entry = s
                .inventory
                .entries
                .iter_mut()
                .find(|e| e.drone_id == drone_id)
                .context("Foreign carrier drone")?;
            if matches!(entry.location, CarrierDroneLocation::Destroyed { .. }) {
                continue;
            }
            s.departures.remove(&drone_id);
            s.mining_blocks.remove(&drone_id);
            let arrival_ids: Vec<_> = s
                .arrivals
                .iter()
                .filter(|(_, a)| a.drone_id == drone_id)
                .map(|(id, _)| *id)
                .collect();
            for id in arrival_ids {
                s.arrivals.remove(&id);
                s.inventory.reservations.retain(|r| r.operation_id != id);
            }
            entry.hit_points = 0;
            entry.location = CarrierDroneLocation::Destroyed { at_ms: now };
            for c in &mut s.inventory.cassettes {
                if c.drone_id == Some(drone_id) {
                    c.drone_id = None;
                }
            }
            if let Some(id) = s
                .inventory
                .operations
                .iter()
                .find(|o| {
                    o.drone_id == drone_id && !o.phase.terminal() && !o.released_loaded_owner()
                })
                .map(|o| o.operation_id)
            {
                service::close_attempt(&mut s, id, now)?;
                phase(
                    &mut s,
                    id,
                    CarrierOperationPhase::Destroyed,
                    now,
                    "Destroyed; retained empty hardware cleanup",
                );
                if let Some(r) = s.runs.get_mut(&id) {
                    r.retain_full_destroyed_cleanup(def, now)?;
                    s.inventory.cleanups.push(CarrierHardwareCleanup {
                        operation_id: id,
                        held: r.held,
                        reason: "Destroyed hardware cleanup".into(),
                    });
                }
            }
            let mut lost = 0;
            if let Some(h) = s.ore.holds.iter_mut().find(|h| h.drone_id == drone_id) {
                lost = h.ore_units;
                h.ore_units = 0;
                h.cycle = None;
                h.mining_intent = None;
                h.destroyed_at_ms = Some(now);
                s.ore.lost_ore_units = s
                    .ore
                    .lost_ore_units
                    .checked_add(lost)
                    .context("Ore sink overflow")?;
            }
            events.push(EconomicEvent::CarrierInventory {
                record: CarrierInventoryLedgerEvent::Destroyed {
                    pilot: name.clone(),
                    vessel_id: s.inventory.binding.vessel_id.clone(),
                    drone_id,
                    destroyed_at_ms: now,
                    lost_units: lost,
                },
            });
        }
        // Entry completion is not capture. Swap to exact registered body AND
        // hardware only after combat, with current peers rechecked at the handoff.
        for id in s
            .arrivals
            .iter()
            .filter(|(_, a)| a.ready_to_handoff)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>()
        {
            let a = s.arrivals[&id].clone();
            let kind = run_kind(&s.inventory, id)?;
            let path = route(def, &kind, a.class)?;
            let actual = actual(&s, &CarrierMotionFrame::from(&path[0]), a.drone_id)?;
            let r = CarrierMotionRuntime::from_route(
                CarrierMotionStart {
                    binding: s.inventory.binding.clone(),
                    operation_id: id,
                    drone_id: a.drone_id,
                    class: a.class,
                    kind,
                    parent: a.parent,
                    system_id: a.system_id,
                    actual,
                    now,
                },
                def,
                path,
            )?;
            if !clear(w, &name, &s, &r, now)? {
                let a = s.arrivals.get_mut(&id).unwrap();
                a.ready_to_handoff = false;
                // Retain the completed exterior segment at its exact endpoint.
                a.confirmed_at_ms = a.started_at_ms + a.duration_ms;
                a.hold("Capture entry obstructed; exterior approach held");
                hold_stage(&mut s.inventory, id, now)?;
                continue;
            }
            s.arrivals.remove(&id);
            phase(
                &mut s,
                id,
                r.route[r.target_index].phase,
                now,
                "Aligned; physical capture source route accepted",
            );
            if matches!(
                s.inventory.operation(id).unwrap().action,
                CarrierInventoryAction::ReturnToUnload { .. }
            ) {
                s.inventory
                    .entries
                    .iter_mut()
                    .find(|e| e.drone_id == a.drone_id)
                    .unwrap()
                    .location = CarrierDroneLocation::Servicing {
                    operation_id: id,
                    berth_id: a.berth_id,
                };
            }
            s.runs.insert(id, r);
        }
        let ready: Vec<_> = s
            .runs
            .iter()
            .filter(|(_, r)| r.ready_to_finalize)
            .map(|(id, _)| *id)
            .collect();
        for id in ready {
            let r = s.runs[&id].clone();
            let handoff = r.kind.stage_segment() == Some(CarrierStageSegment::Head);
            ensure!(
                (r.reached.hardware_safe || handoff) && r.reached.loader_clear,
                "Cannot finalize unsafe source hardware"
            );
            let op = s.inventory.operation(id).unwrap().clone();
            if handoff {
                // Combat has already resolved. A dead carried drone cannot become Ready.
                ensure!(
                    op.phase != CarrierOperationPhase::Destroyed,
                    "Destroyed Stage head needs empty completion before release"
                );
                ensure!(
                    r.completed_stage_legs == 2
                        && r.reached.cassette_supported
                        && r.reached.cassette_locked
                        && r.reached.drone_clamped,
                    "Unconfirmed loaded handoff"
                );
                record_stage_handoff(&mut s.inventory, &def.topology, id, now)?;
                let kind = run_kind(&s.inventory, id)?;
                let path = route(def, &kind, r.class)?;
                let actual = actual(&s, &CarrierMotionFrame::from(&path[0]), r.drone_id)?;
                let tail = CarrierMotionRuntime::from_route(
                    CarrierMotionStart {
                        binding: r.binding,
                        operation_id: id,
                        drone_id: r.drone_id,
                        class: r.class,
                        kind,
                        parent: r.parent,
                        system_id: r.system_id,
                        actual,
                        now,
                    },
                    def,
                    path,
                )?;
                s.runs.insert(id, tail);
                continue;
            }
            if op.phase == CarrierOperationPhase::Destroyed {
                if op.stage.is_some() {
                    record_cleanup_secured(&mut s.inventory, &def.topology, id)?;
                } else {
                    s.inventory.cleanups.retain(|c| c.operation_id != id);
                    s.inventory.reservations.retain(|r| r.operation_id != id);
                }
            } else if op.stage.is_some() {
                record_stage_secured(&mut s.inventory, &def.topology, id, now)?;
            } else if op.action == CarrierInventoryAction::RecallToStorage {
                ensure!(
                    r.reached.drone_clamped
                        && r.reached.cassette_supported
                        && r.reached.cassette_locked
                        && r.reached.hardware_safe,
                    "Capture lacks actual supported locked safe endpoint"
                );
                let berth_id = op.berth_id.context("Capture berth missing")?;
                let cassette = s
                    .inventory
                    .cassettes
                    .iter_mut()
                    .find(|c| {
                        c.location
                            == (CarrierCassetteLocation::Docked {
                                dock: CarrierCassetteDock::Ready { berth_id },
                            })
                    })
                    .context("Capture cassette absent")?;
                ensure!(
                    cassette.drone_id.is_none()
                        && s.drone_poses[&op.drone_id] == s.cassette_poses[&cassette.cassette_id],
                    "Capture endpoint identity/pose changed"
                );
                cassette.drone_id = Some(op.drone_id);
                s.inventory
                    .entries
                    .iter_mut()
                    .find(|e| e.drone_id == op.drone_id)
                    .unwrap()
                    .location = CarrierDroneLocation::Ready { berth_id };
                w.pilots
                    .get_mut(&name)
                    .unwrap()
                    .view
                    .drones
                    .retain(|d| d.id != op.drone_id);
                phase(
                    &mut s,
                    id,
                    CarrierOperationPhase::Completed,
                    now,
                    "Captured same identity on secured Ready cassette",
                );
                s.inventory.reservations.retain(|x| x.operation_id != id);
            } else if let CarrierInventoryAction::ReturnToUnload { resume_mining } = op.action {
                ensure!(
                    r.reached.exterior_clear
                        && !r.waiting_for_transfer
                        && s.ore
                            .holds
                            .iter()
                            .find(|h| h.drone_id == op.drone_id)
                            .is_some_and(|h| h.ore_units == 0 && h.cycle.is_none()),
                    "Service cannot complete with retained payload/contact"
                );
                service::close_attempt(&mut s, id, now)?;
                s.inventory
                    .entries
                    .iter_mut()
                    .find(|e| e.drone_id == op.drone_id)
                    .unwrap()
                    .location = CarrierDroneLocation::Deployed;
                phase(
                    &mut s,
                    id,
                    CarrierOperationPhase::Completed,
                    now,
                    "Unloaded; exterior clear and service hardware secured",
                );
                s.inventory.reservations.retain(|r| r.operation_id != id);
                if resume_mining {
                    if let Some(intent) = s
                        .ore
                        .holds
                        .iter()
                        .find(|h| h.drone_id == op.drone_id)
                        .and_then(|h| h.mining_intent.as_ref())
                    {
                        let p = w.pilots.get_mut(&name).unwrap();
                        if p.view.locked_asteroids.contains(&intent.target_id)
                            && w.asteroids.iter().any(|a| {
                                a.id == intent.target_id
                                    && a.ore > 0
                                    && a.system_id == p.view.system_id
                            })
                        {
                            p.view
                                .drones
                                .iter_mut()
                                .find(|d| d.id == op.drone_id)
                                .unwrap()
                                .mode = DroneMode::Mining {
                                target_id: intent.target_id,
                            };
                        }
                    }
                }
            } else {
                ensure!(
                    op.action == CarrierInventoryAction::Launch && r.reached.exterior_clear,
                    "Unproved launch completion"
                );
                s.inventory
                    .entries
                    .iter_mut()
                    .find(|e| e.drone_id == op.drone_id)
                    .unwrap()
                    .location = CarrierDroneLocation::Deployed;
                for c in &mut s.inventory.cassettes {
                    if c.drone_id == Some(op.drone_id) {
                        c.drone_id = None;
                    }
                }
                let departure = departure::Departure::begin(&s, id, now)?;
                s.departures.insert(op.drone_id, departure);
                phase(
                    &mut s,
                    id,
                    CarrierOperationPhase::Completed,
                    now,
                    "Outside; launch hardware secured",
                );
                s.inventory.reservations.retain(|r| r.operation_id != id);
            }
            s.runs.remove(&id);
        }
        // Free flight is authoritative in DroneView, not the last handling pose.
        for d in &w.pilots[&name].view.drones {
            if d.hit_points > 0
                && s.inventory
                    .entry(d.id)
                    .is_some_and(|e| e.location == CarrierDroneLocation::Deployed)
                && !s.departures.contains_key(&d.id)
            {
                let pose = independent_world_to_body(
                    parent(w, &name)?,
                    BayPose {
                        position_m: d.position,
                        orientation_xyzw: d
                            .orientation_xyzw
                            .context("Free carrier attitude missing")?,
                    },
                )
                .context("Free carrier pose invalid")?;
                s.drone_poses.insert(d.id, pose);
            }
        }
        service::reconcile(&mut s, now)?;
        w.accounts.get_mut(&name).unwrap().carrier_inventory = Some(s);
        drones::sync(w.pilots.get_mut(&name).unwrap());
    }
    Ok(events)
}

#[cfg(test)]
#[path = "compound_tests.rs"]
mod compound_tests;

#[cfg(test)]
mod prepared_compound_tests {
    use super::*;
    use crate::independent_bays::independent_bay_sweeps_clear;
    fn body(x: f64) -> IndependentBaySweptBody {
        IndependentBaySweptBody {
            segment: stationary(
                BayPose {
                    position_m: [x, 0., 0.],
                    orientation_xyzw: [0., 0., 0., 1.],
                },
                0,
            ),
            local_bounds_m: [[-1.; 3], [1.; 3]],
        }
    }
    fn reference(a: &CarrierSweptBody, b: &CarrierSweptBody) -> bool {
        if independent_bay_sweeps_clear(a.union.body, b.union.body, 0, 100, 0.) {
            return true;
        }
        a.parts().iter().all(|a| {
            b.parts()
                .iter()
                .all(|b| independent_bay_sweeps_clear(a.body, b.body, 0, 100, 0.))
        })
    }
    #[test]
    fn invalid_union_retains_certified_leaf_fallback_and_invalid_leaf_blocks() {
        let mut invalid = body(0.);
        invalid.local_bounds_m[1][0] = f64::NAN;
        let b = CarrierSweptBody::single(body(0.));
        let a = CarrierSweptBody::new(invalid, Some(vec![body(8.)]));
        assert!(reference(&a, &b));
        assert!(compounds_clear(&a, &b, 0, 100, 0.));
        assert!(compounds_clear(&a, &b, 0, 100, 0.));
        let a = CarrierSweptBody::new(body(0.), Some(vec![invalid]));
        assert!(!reference(&a, &b));
        assert!(!compounds_clear(&a, &b, 0, 100, 0.));
    }
    #[test]
    fn new_body_context_never_reuses_old_bounds_pose_or_segment() {
        let a = CarrierSweptBody::single(body(0.));
        let clear = CarrierSweptBody::single(body(8.));
        assert!(compounds_clear(&a, &clear, 0, 100, 0.));
        let changed = CarrierSweptBody::single(body(0.));
        assert!(!compounds_clear(&a, &changed, 0, 100, 0.));
        let mut wide = body(8.);
        wide.local_bounds_m = [[-20.; 3], [20.; 3]];
        let changed = CarrierSweptBody::single(wide);
        assert!(!compounds_clear(&a, &changed, 0, 100, 0.));
        let mut crossing = body(8.);
        crossing.segment.target.position_m = [0.; 3];
        let changed = CarrierSweptBody::single(crossing);
        assert!(!compounds_clear(&a, &changed, 0, 100, 0.));
        for b in [&clear, &changed] {
            assert_eq!(compounds_clear(&a, b, 0, 100, 0.), reference(&a, b));
        }
    }
}
