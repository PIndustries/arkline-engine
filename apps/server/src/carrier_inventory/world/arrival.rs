//! Retained exterior approach to a reserved, source-confirmed empty Ready cassette.
//! It owns the drone only; all hardware stays at its checked entry until the
//! post-combat atomic handoff to the reversed source route.
use super::*;
use crate::independent_bays::{
    exterior_entry_poses, independent_entry_swept_body, independent_world_to_body,
    IndependentBaySweptBody,
};
const BODY: [[f64; 3]; 2] = [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]];
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Arrival {
    pub operation_id: u64,
    pub drone_id: u64,
    pub cassette_id: u64,
    pub berth_id: u8,
    pub parent: BayPose,
    pub system_id: u32,
    pub class: DroneClass,
    pub original_body_pose: BayPose,
    pub admitted_at_ms: u64,
    pub path: Vec<BayPose>,
    pub asteroid_obstacles: Vec<(u64, [[f64; 3]; 2])>,
    /// Historical admission hints only; dynamic peers are rechecked every tick.
    pub peer_obstacles: Vec<(u64, [[f64; 3]; 2])>,
    pub target_index: usize,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub confirmed: BayPose,
    pub confirmed_at_ms: u64,
    pub held: bool,
    pub hold_reason: String,
    pub ready_to_handoff: bool,
}
fn duration(a: BayPose, b: BayPose, class: DroneClass) -> Result<u64> {
    let n = (1500.
        * (0..3)
            .map(|i| (a.position_m[i] - b.position_m[i]).powi(2))
            .sum::<f64>()
            .sqrt()
        / drone_speed(class).min(40.))
    .ceil();
    ensure!(
        n.is_finite() && n >= 0. && n < u64::MAX as f64,
        "Approach duration overflow"
    );
    Ok((n as u64).max(750))
}
fn world_box(host: BayPose, position: [f64; 3], radius: f64) -> Result<[[f64; 3]; 2]> {
    let p = independent_world_to_body(
        host,
        BayPose {
            position_m: position,
            orientation_xyzw: [0., 0., 0., 1.],
        },
    )
    .context("Obstacle transform")?;
    let radius = radius + simulation::rounding_margin(host.position_m, position);
    Ok([
        p.position_m.map(|x| x - radius),
        p.position_m.map(|x| x + radius),
    ])
}
fn asteroid_obstacles(w: &World, name: &str, host: BayPose) -> Result<Vec<(u64, [[f64; 3]; 2])>> {
    w.asteroids
        .iter()
        .filter(|a| a.system_id == w.pilots[name].view.system_id)
        .map(|a| Ok((a.id, world_box(host, a.position, a.radius)?)))
        .collect()
}
fn peer_obstacles(
    w: &World,
    name: &str,
    id: u64,
    host: BayPose,
) -> Result<Vec<(u64, [[f64; 3]; 2])>> {
    let mut out = Vec::new();
    for (n, p) in &w.pilots {
        if p.view.system_id != w.pilots[name].view.system_id {
            continue;
        }
        for d in &p.view.drones {
            if n != name || d.id != id {
                let bounds = if let Some(orientation_xyzw) = d.orientation_xyzw {
                    // Complete oriented peer body, not its enclosing sphere: the
                    // latter needlessly blocks the authored adjacent 8m lanes.
                    // The planner still inflates by our entire corner radius;
                    // dynamic full-body sweeps are rechecked on every tick.
                    let pose = independent_world_to_body(
                        host,
                        BayPose {
                            position_m: d.position,
                            orientation_xyzw,
                        },
                    )
                    .context("Peer body transform")?;
                    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
                    for x in [BODY[0][0], BODY[1][0]] {
                        for y in [BODY[0][1], BODY[1][1]] {
                            for z in [BODY[0][2], BODY[1][2]] {
                                let corner = pose
                                    .compose(BayPose {
                                        position_m: [x, y, z],
                                        orientation_xyzw: [0., 0., 0., 1.],
                                    })
                                    .context("Peer corner transform")?
                                    .position_m;
                                for (axis, value) in corner.into_iter().enumerate() {
                                    bounds[0][axis] = bounds[0][axis].min(value);
                                    bounds[1][axis] = bounds[1][axis].max(value);
                                }
                            }
                        }
                    }
                    let margin = simulation::rounding_margin(host.position_m, d.position);
                    for axis in 0..3 {
                        bounds[0][axis] -= margin;
                        bounds[1][axis] += margin;
                    }
                    bounds
                } else {
                    world_box(
                        host,
                        d.position,
                        crate::independent_bays::independent_drone_radius(BODY).unwrap(),
                    )?
                };
                out.push((d.id, bounds));
            }
        }
    }
    Ok(out)
}
/// Conservative geometric reservation of the entire remaining exterior path.
/// This is deliberately independent of future scheduling: intersecting routes
/// queue before acquiring a body writer instead of creating mutual held craft.
fn corridor_bounds(a: BayPose, b: BayPose, margin: f64) -> Result<[[f64; 3]; 2]> {
    let fixed = a.orientation_xyzw == b.orientation_xyzw
        || a.orientation_xyzw == b.orientation_xyzw.map(|v| -v);
    if !fixed {
        ensure!(
            a.position_m == b.position_m,
            "Exterior corridor cannot combine rotation and translation"
        );
        let radius = crate::independent_bays::independent_drone_radius(BODY)
            .context("Body bounds")?
            + margin;
        return Ok([
            a.position_m.map(|v| v - radius),
            a.position_m.map(|v| v + radius),
        ]);
    }
    let mut bounds = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for pose in [a, b] {
        for x in [BODY[0][0], BODY[1][0]] {
            for y in [BODY[0][1], BODY[1][1]] {
                for z in [BODY[0][2], BODY[1][2]] {
                    let corner = pose
                        .compose(BayPose {
                            position_m: [x, y, z],
                            orientation_xyzw: [0., 0., 0., 1.],
                        })
                        .context("Corridor corner")?
                        .position_m;
                    for (axis, value) in corner.into_iter().enumerate() {
                        bounds[0][axis] = bounds[0][axis].min(value - margin);
                        bounds[1][axis] = bounds[1][axis].max(value + margin);
                    }
                }
            }
        }
    }
    Ok(bounds)
}
fn corridor_margin(parent: BayPose, a: BayPose, b: BayPose) -> Result<f64> {
    let aw = parent
        .compose(a)
        .context("Corridor source world transform")?;
    let bw = parent
        .compose(b)
        .context("Corridor target world transform")?;
    Ok(0.001
        + simulation::rounding_margin(parent.position_m, aw.position_m)
            .max(simulation::rounding_margin(
                parent.position_m,
                bw.position_m,
            ))
            .max(simulation::rounding_margin(a.position_m, b.position_m)))
}
impl Arrival {
    pub(super) fn corridor_clear(&self, peers: &BTreeMap<u64, Arrival>) -> Result<bool> {
        let domains = |a: &Arrival| -> Result<Vec<[[f64; 3]; 2]>> {
            let poses: Vec<_> = std::iter::once(a.confirmed)
                .chain(a.path[a.target_index..].iter().copied())
                .collect();
            poses
                .windows(2)
                .map(|p| {
                    let margin = corridor_margin(a.parent, p[0], p[1])?;
                    corridor_bounds(p[0], p[1], margin)
                })
                .collect()
        };
        let ours = domains(self)?;
        for (id, peer) in peers {
            if *id == self.operation_id {
                continue;
            }
            ensure!(
                peer.parent == self.parent,
                "Exterior reservation parent differs"
            );
            let theirs = domains(peer)?;
            if ours.iter().any(|a| {
                theirs
                    .iter()
                    .any(|b| !(0..3).any(|axis| a[1][axis] < b[0][axis] || b[1][axis] < a[0][axis]))
            }) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    pub(super) fn begin(
        w: &World,
        name: &str,
        s: &OwnedCarrier,
        id: u64,
        cassette_id: u64,
        berth_id: u8,
        now: u64,
    ) -> Result<Self> {
        let def = definition(w)?.context("Missing carrier source")?;
        let op = s
            .inventory
            .operation(id)
            .context("Missing capture operation")?;
        let e = s
            .inventory
            .entry(op.drone_id)
            .context("Missing capture drone")?;
        let host = parent(w, name)?;
        let live = w.pilots[name]
            .view
            .drones
            .iter()
            .find(|d| d.id == e.drone_id)
            .context("Actual returning body missing")?;
        ensure!(
            live.class == e.class && live.hit_points == e.hit_points && live.hit_points > 0,
            "Actual returning identity/HP mismatch"
        );
        let source = independent_world_to_body(
            host,
            BayPose {
                position_m: live.position,
                orientation_xyzw: live
                    .orientation_xyzw
                    .context("Actual return attitude missing")?,
            },
        )
        .context("Actual return parent transform")?;
        let canonical = def
            .bay_route(berth_id, e.class, cassette_id, op.action)
            .context("Capture route unavailable")?;
        let entry = CarrierMotionFrame::from(&canonical[0]);
        let mut actual = actual(s, &entry, e.drone_id)?;
        // Only the exterior drone differs; every reserved hardware root must already agree.
        actual
            .poses
            .iter_mut()
            .filter(|(k, _)| *k == CarrierPoseKey::Drone)
            .for_each(|(_, p)| *p = canonical[0].drone_pose.unwrap());
        ensure!(
            actual == entry,
            "Capture hardware is not at exact registered entry"
        );
        let asteroid_obstacles = asteroid_obstacles(w, name, host)?;
        let peer_obstacles = peer_obstacles(w, name, e.drone_id, host)?;
        let obstacles: Vec<_> = asteroid_obstacles
            .iter()
            .chain(&peer_obstacles)
            .map(|(_, b)| *b)
            .collect();
        let path = exterior_entry_poses(
            source,
            canonical[0].drone_pose.context("Capture entry missing")?,
            BODY,
            def.deployed_hardware_bounds_m,
            &obstacles,
        )?;
        ensure!(path.len() >= 2, "Approach requires retained entry segment");
        let duration_ms = duration(path[0], path[1], e.class)?;
        ensure!(
            now.checked_add(duration_ms).is_some(),
            "Approach deadline overflow"
        );
        Ok(Self {
            operation_id: id,
            drone_id: e.drone_id,
            cassette_id,
            berth_id,
            parent: host,
            system_id: w.pilots[name].view.system_id,
            class: e.class,
            confirmed: path[0],
            original_body_pose: source,
            admitted_at_ms: now,
            path,
            asteroid_obstacles,
            peer_obstacles,
            target_index: 1,
            started_at_ms: now,
            duration_ms,
            confirmed_at_ms: now,
            held: false,
            hold_reason: String::new(),
            ready_to_handoff: false,
        })
    }
    pub(super) fn segment(&self) -> Result<BayMotionSegment> {
        ensure!(
            (1..self.path.len()).contains(&self.target_index),
            "Invalid approach cursor"
        );
        Ok(BayMotionSegment {
            source: self.path[self.target_index - 1],
            target: self.path[self.target_index],
            started_at_ms: self.started_at_ms,
            duration_ms: self.duration_ms,
        })
    }
    pub(super) fn body(&self) -> Result<IndependentBaySweptBody> {
        let segment = if self.held || self.ready_to_handoff {
            BayMotionSegment {
                source: self.confirmed,
                target: self.confirmed,
                started_at_ms: self.confirmed_at_ms,
                duration_ms: 1,
            }
        } else {
            self.segment()?
        };
        independent_entry_swept_body(segment, BODY).context("Unsupported approach motion")
    }
    pub(super) fn hold(&mut self, reason: &str) {
        self.held = true;
        self.hold_reason = reason.into();
    }
    pub(super) fn resume(&mut self, now: u64) -> Result<()> {
        ensure!(
            now >= self.confirmed_at_ms && !self.ready_to_handoff,
            "Approach clock/finalization mismatch"
        );
        if self.held {
            self.started_at_ms = self
                .started_at_ms
                .checked_add(now - self.confirmed_at_ms)
                .context("Approach resume overflow")?;
            ensure!(
                self.started_at_ms.checked_add(self.duration_ms).is_some(),
                "Approach deadline overflow"
            );
            self.confirmed_at_ms = now;
            self.held = false;
            self.hold_reason.clear();
        }
        Ok(())
    }
    pub(super) fn validate(&self, w: &World, name: &str, s: &OwnedCarrier) -> Result<()> {
        let def = definition(w)?.context("Approach source missing")?;
        let op = s
            .inventory
            .operation(self.operation_id)
            .context("Approach receipt missing")?;
        let e = s
            .inventory
            .entry(self.drone_id)
            .context("Approach identity missing")?;
        let canonical = def
            .bay_route(self.berth_id, e.class, self.cassette_id, op.action)
            .context("Approach source unavailable")?;
        ensure!(
            !self.ready_to_handoff
                && self.parent == parent(w, name)?
                && self.system_id == w.pilots[name].view.system_id
                && self.class == e.class
                && op.drone_id == self.drone_id
                && op.berth_id == Some(self.berth_id)
                && matches!(
                    op.action,
                    CarrierInventoryAction::RecallToStorage
                        | CarrierInventoryAction::ReturnToUnload { .. }
                )
                && op.effective_phase() == CarrierOperationPhase::Returning
                && (op.phase == CarrierOperationPhase::Held) == self.held
                && self.held != self.hold_reason.is_empty()
                && self.hold_reason.len() <= 512,
            "Approach ownership/hold mismatch"
        );
        ensure!(
            self.admitted_at_ms >= op.accepted_at_ms
                && self.admitted_at_ms <= op.updated_at_ms
                && self.admitted_at_ms <= self.started_at_ms
                && self.confirmed_at_ms >= self.admitted_at_ms
                && self.path.first() == Some(&self.original_body_pose)
                && self.asteroid_obstacles == asteroid_obstacles(w, name, self.parent)?
                && s.inventory
                    .docked_cassette(CarrierCassetteDock::Ready {
                        berth_id: self.berth_id
                    })
                    .is_some_and(|c| c.cassette_id == self.cassette_id && c.drone_id.is_none()),
            "Approach acceptance clock/catalog/cassette mismatch"
        );
        let mut peer_ids = BTreeSet::new();
        ensure!(
            self.peer_obstacles
                .iter()
                .all(|(id, _)| *id > 0 && *id != self.drone_id && peer_ids.insert(*id)),
            "Invalid historical approach peer set"
        );
        let obstacles: Vec<_> = self
            .asteroid_obstacles
            .iter()
            .chain(&self.peer_obstacles)
            .map(|(_, b)| *b)
            .collect();
        let initial = self.original_body_pose;
        ensure!(
            self.path
                == exterior_entry_poses(
                    initial,
                    canonical[0].drone_pose.unwrap(),
                    BODY,
                    def.deployed_hardware_bounds_m,
                    &obstacles
                )?,
            "Approach differs from accepted source-clear plan"
        );
        let segment = self.segment()?;
        ensure!(
            self.duration_ms == duration(segment.source, segment.target, self.class)?
                && self.started_at_ms <= self.confirmed_at_ms
                && self
                    .started_at_ms
                    .checked_add(self.duration_ms)
                    .is_some_and(|end| self.confirmed_at_ms <= end)
                && segment
                    .sample(self.confirmed_at_ms)
                    .is_some_and(|(p, _)| p == self.confirmed)
                && s.drone_poses.get(&self.drone_id) == Some(&self.confirmed),
            "Approach clock/body mismatch"
        );
        let mut current = actual(s, &CarrierMotionFrame::from(&canonical[0]), self.drone_id)?;
        current
            .poses
            .iter_mut()
            .filter(|(k, _)| *k == CarrierPoseKey::Drone)
            .for_each(|(_, p)| *p = canonical[0].drone_pose.unwrap());
        ensure!(
            current == CarrierMotionFrame::from(&canonical[0]),
            "Reserved approach hardware changed"
        );
        Ok(())
    }
}
pub(super) fn advance(
    snapshot: &World,
    w: &mut World,
    name: &str,
    context: &simulation::PeerSweeps<'_>,
    s: &mut OwnedCarrier,
    now: u64,
) -> Result<()> {
    for id in s.arrivals.keys().copied().collect::<Vec<_>>() {
        let a = s.arrivals.get_mut(&id).unwrap();
        ensure!(now >= a.confirmed_at_ms, "Approach clock reversed");
        if a.held {
            continue;
        }
        if now - a.confirmed_at_ms > 2000
            || !simulation::clear_bodies_with_peers(
                snapshot,
                name,
                context,
                a.drone_id,
                Some(id),
                a.parent,
                &[a.body()?],
                a.confirmed_at_ms,
                now,
            )?
        {
            a.hold("Approach blocked by an actual obstacle or simulation gap");
            hold_stage(&mut s.inventory, id, now)?;
            continue;
        }
        let segment = a.segment()?;
        a.confirmed = segment.sample(now).context("Invalid approach sample")?.0;
        a.confirmed_at_ms = now;
        if now
            >= segment
                .started_at_ms
                .checked_add(segment.duration_ms)
                .context("Approach deadline overflow")?
        {
            if a.target_index + 1 == a.path.len() {
                a.ready_to_handoff = true;
            } else {
                a.target_index += 1;
                a.started_at_ms = now;
                a.duration_ms =
                    duration(a.path[a.target_index - 1], a.path[a.target_index], a.class)?;
                ensure!(
                    now.checked_add(a.duration_ms).is_some(),
                    "Approach deadline overflow"
                );
            }
        }
        s.drone_poses.insert(a.drone_id, a.confirmed);
        let pose = a
            .parent
            .compose(a.confirmed)
            .context("Approach world pose")?;
        let drone = w
            .pilots
            .get_mut(name)
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|d| d.id == a.drone_id)
            .context("Approach live body missing")?;
        drone.position = pose.position_m;
        drone.orientation_xyzw = Some(pose.orientation_xyzw);
        drone.heading = pose.transform_vector([1., 0., 0.]).unwrap();
        drone.velocity = [0.; 3];
        drone.mode = DroneMode::Hold;
        drone.updated_at_ms = now;
    }
    Ok(())
}

#[cfg(test)]
mod corridor_tests {
    use super::*;
    #[test]
    fn complete_corridor_domains_cover_body_rotation_and_parallel_lanes() {
        let pose = |x, z| BayPose {
            position_m: [x, 0., z],
            orientation_xyzw: [
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
        };
        let a = corridor_bounds(pose(0., 100.), pose(0., 40.), 0.001).unwrap();
        let b = corridor_bounds(pose(8., 100.), pose(8., 40.), 0.001).unwrap();
        assert!(a[1][0] < b[0][0]);
        let cross = corridor_bounds(pose(-8., 60.), pose(8., 60.), 0.001).unwrap();
        assert!(!(0..3).any(|i| a[1][i] < cross[0][i] || cross[1][i] < a[0][i]));
        let mut rotated = pose(0., 40.);
        rotated.orientation_xyzw = [0., 0., 0., 1.];
        let spin = corridor_bounds(pose(0., 40.), rotated, 0.001).unwrap();
        assert!(spin[1][0] > 4. && spin[1][2] > 44.);
        assert!(corridor_bounds(pose(0., 100.), rotated, 0.001).is_err());
    }
    #[test]
    fn corridor_budget_includes_distant_local_endpoints_and_world_origin() {
        let origin = BayPose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let far = BayPose {
            position_m: [1e12, 0., 0.],
            ..origin
        };
        let margin = corridor_margin(origin, far, far).unwrap();
        assert!(margin > 0.02);
        let a = corridor_bounds(far, far, margin).unwrap();
        let adjacent = BayPose {
            position_m: [1e12 + 5.01, 0., 0.],
            ..origin
        };
        let b = corridor_bounds(
            adjacent,
            adjacent,
            corridor_margin(origin, adjacent, adjacent).unwrap(),
        )
        .unwrap();
        assert!(
            a[1][0] >= b[0][0],
            "Sub-budget apparent separation must not admit a near-tangent corridor"
        );
        assert!(corridor_margin(far, origin, origin).unwrap() >= margin);
    }
}
