//! Collision-checked exterior dispersal after the source launch route is secure.
//! Retained per-berth launch order puts earlier departures farther out on each fixed
//! source axis, so later departures do not target an occupied clear point.
use super::*;
use crate::independent_bays::IndependentBaySweptBody;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Departure {
    pub operation_id: u64,
    pub cassette_id: u64,
    pub drone_id: u64,
    pub parent: BayPose,
    pub initial: BayPose,
    pub source: BayPose,
    pub target: BayPose,
    pub confirmed: BayPose,
    pub started_at_ms: u64,
    pub duration_ms: u64,
    pub confirmed_at_ms: u64,
    pub held: bool,
    pub hold_reason: String,
}
fn target(s: &OwnedCarrier, id: u64, operation_id: u64, initial: BayPose) -> Result<BayPose> {
    let op = s
        .inventory
        .operation(operation_id)
        .context("Departure operation missing")?;
    ensure!(
        op.drone_id == id && op.berth_id.is_some(),
        "Departure owner missing"
    );
    let ordinal = s
        .inventory
        .operations
        .iter()
        .filter(|o| {
            o.operation_id <= operation_id
                && o.action == CarrierInventoryAction::Launch
                && o.berth_id == op.berth_id
        })
        .count();
    ensure!(
        ordinal > 0 && ordinal <= s.inventory.roster.len(),
        "No free source-qualified exterior formation slot; hold for allocation"
    );
    let distance = 40. + 12. * (s.inventory.roster.len() + 1 - ordinal) as f64;
    let outward = initial
        .transform_vector([distance, 0., 0.])
        .context("Departure attitude")?;
    Ok(BayPose {
        position_m: std::array::from_fn(|i| initial.position_m[i] + outward[i]),
        ..initial
    })
}
fn duration(source: BayPose, target: BayPose) -> u64 {
    (1500.
        * source
            .position_m
            .iter()
            .zip(target.position_m)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt()
        / 40.)
        .ceil()
        .max(750.) as u64
}
impl Departure {
    pub(super) fn begin(s: &OwnedCarrier, operation_id: u64, now: u64) -> Result<Self> {
        let r = &s.runs[&operation_id];
        let initial = s.drone_poses[&r.drone_id];
        let target = target(s, r.drone_id, operation_id, initial)?;
        let CarrierRouteKind::Bay { cassette_id, .. } = r.kind else {
            bail!("Only launched bodies receive departure paths")
        };
        ensure!(
            now.checked_add(duration(initial, target)).is_some(),
            "Departure clock overflow"
        );
        Ok(Self {
            operation_id,
            cassette_id,
            drone_id: r.drone_id,
            parent: r.parent,
            initial,
            source: initial,
            target,
            confirmed: initial,
            started_at_ms: now,
            duration_ms: duration(initial, target),
            confirmed_at_ms: now,
            held: false,
            hold_reason: String::new(),
        })
    }
    fn segment(&self) -> BayMotionSegment {
        BayMotionSegment {
            source: self.source,
            target: self.target,
            started_at_ms: self.started_at_ms,
            duration_ms: self.duration_ms,
        }
    }
    pub(super) fn body(&self) -> IndependentBaySweptBody {
        IndependentBaySweptBody {
            segment: if self.held {
                BayMotionSegment {
                    source: self.confirmed,
                    target: self.confirmed,
                    started_at_ms: self.confirmed_at_ms,
                    duration_ms: 1,
                }
            } else {
                self.segment()
            },
            local_bounds_m: [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]],
        }
    }
    pub(super) fn resume(&mut self, now: u64) -> Result<()> {
        ensure!(now >= self.confirmed_at_ms, "Departure clock reversed");
        if self.held {
            self.source = self.confirmed;
            self.started_at_ms = now;
            self.confirmed_at_ms = now;
            self.duration_ms = duration(self.source, self.target);
            ensure!(
                now.checked_add(self.duration_ms).is_some(),
                "Departure resume clock overflow"
            );
            self.held = false;
            self.hold_reason.clear();
        }
        Ok(())
    }
    pub(super) fn validate(
        &self,
        s: &OwnedCarrier,
        def: &CarrierInventoryDefinition,
        host: BayPose,
    ) -> Result<()> {
        let op = s
            .inventory
            .operation(self.operation_id)
            .context("Departure operation missing")?;
        let r = def
            .bay_route(
                op.berth_id.context("Departure berth missing")?,
                s.inventory
                    .entry(self.drone_id)
                    .context("Departure finite identity missing")?
                    .class,
                self.cassette_id,
                CarrierInventoryAction::Launch,
            )
            .context("Departure source missing")?;
        ensure!(
            op.drone_id == self.drone_id
                && op.action == CarrierInventoryAction::Launch
                && op.phase == CarrierOperationPhase::Completed
                && matches!(
                    s.inventory
                        .entry(self.drone_id)
                        .context("Departure finite identity missing")?
                        .location,
                    CarrierDroneLocation::Deployed
                )
                && r.last().and_then(|f| f.drone_pose) == Some(self.initial)
                && self.target == target(s, self.drone_id, self.operation_id, self.initial)?
                && self.parent == host,
            "Departure identity/source/stand-off mismatch"
        );
        let axis = (0..3)
            .max_by(|a, b| {
                (self.target.position_m[*a] - self.initial.position_m[*a])
                    .abs()
                    .total_cmp(&(self.target.position_m[*b] - self.initial.position_m[*b]).abs())
            })
            .unwrap();
        let fraction = (self.source.position_m[axis] - self.initial.position_m[axis])
            / (self.target.position_m[axis] - self.initial.position_m[axis]);
        ensure!(
            self.source.orientation_xyzw == self.initial.orientation_xyzw
                && (0.0..=1.0).contains(&fraction)
                && (0..3).all(|i| (self.source.position_m[i]
                    - (self.initial.position_m[i]
                        + fraction * (self.target.position_m[i] - self.initial.position_m[i])))
                    .abs()
                    < 1e-7),
            "Departure resumed off its accepted axis"
        );
        let finish = self
            .started_at_ms
            .checked_add(self.duration_ms)
            .context("Departure clock overflow")?;
        ensure!(
            self.held != self.hold_reason.is_empty() && self.hold_reason.len() <= 512,
            "Departure hold reason mismatch"
        );
        ensure!(
            self.duration_ms == duration(self.source, self.target)
                && self.started_at_ms <= self.confirmed_at_ms
                && self.confirmed_at_ms <= finish
                && self
                    .segment()
                    .sample(self.confirmed_at_ms)
                    .is_some_and(|(p, _)| p == self.confirmed),
            "Departure clock/pose mismatch"
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
    let ids = s.departures.keys().copied().collect::<Vec<_>>();
    for id in ids {
        let d = s.departures.get_mut(&id).unwrap();
        ensure!(now >= d.confirmed_at_ms, "Departure clock reversed");
        if d.held {
            continue;
        }
        if now - d.confirmed_at_ms > 2000
            || !simulation::clear_bodies_with_peers(
                snapshot,
                name,
                context,
                id,
                None,
                d.parent,
                &[d.body()],
                d.confirmed_at_ms,
                now,
            )?
        {
            d.held = true;
            d.hold_reason = if now - d.confirmed_at_ms > 2000 {
                "Simulation gap; departure held"
            } else {
                "Actual exterior obstacle; departure held"
            }
            .into();
            if let Some(body) = w
                .pilots
                .get_mut(name)
                .unwrap()
                .view
                .drones
                .iter_mut()
                .find(|v| v.id == id)
            {
                body.velocity = [0.; 3];
            }
            continue;
        }
        let (pose, velocity) = d
            .segment()
            .sample(now)
            .context("Departure sample invalid")?;
        d.confirmed = pose;
        d.confirmed_at_ms = now.min(d.started_at_ms + d.duration_ms);
        s.drone_poses.insert(id, pose);
        let world_pose = d.parent.compose(pose).context("Departure world pose")?;
        let drone = w
            .pilots
            .get_mut(name)
            .unwrap()
            .view
            .drones
            .iter_mut()
            .find(|v| v.id == id)
            .context("Departure live body missing")?;
        drone.position = world_pose.position_m;
        drone.orientation_xyzw = Some(world_pose.orientation_xyzw);
        drone.heading = world_pose.transform_vector([1., 0., 0.]).unwrap();
        drone.velocity = d.parent.transform_vector(velocity).unwrap();
        drone.mode = DroneMode::Hold;
        drone.updated_at_ms = now;
        if now >= d.started_at_ms + d.duration_ms {
            drone.velocity = [0.; 3];
            s.departures.remove(&id);
        }
    }
    Ok(())
}
