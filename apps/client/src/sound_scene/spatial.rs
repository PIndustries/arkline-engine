//! Listener-space attenuation, physical emitters and confirmed spatial event edges.

use super::*;

impl AudioListener {
    pub(super) fn spatial(self, emitter: [f64; 3], level: f32) -> Option<(f32, f32)> {
        if !level.is_finite()
            || !self
                .position_m
                .iter()
                .chain(self.right.iter())
                .chain(emitter.iter())
                .all(|v| v.is_finite())
            || (self.right.iter().map(|v| v * v).sum::<f64>() - 1.).abs() > 1e-5
        {
            return None;
        }
        let delta = std::array::from_fn::<_, 3, _>(|i| emitter[i] - self.position_m[i]);
        let distance = delta[0].hypot(delta[1]).hypot(delta[2]);
        if !distance.is_finite() {
            return None;
        }
        let pan = if distance < 1e-6 {
            0.
        } else {
            delta
                .iter()
                .zip(self.right)
                .map(|(a, b)| a * b)
                .sum::<f64>()
                / distance
        };
        // Bounded presentation attenuation, not a pressure/acoustic simulation.
        let attenuation = 1. / (1. + (distance / 30.).powi(2));
        Some((
            (level.clamp(0., 1.) as f64 * attenuation) as f32,
            pan.clamp(-1., 1.) as f32,
        ))
    }
}

impl Scene {
    /// Same scene used by live CPAL and deterministic offline capture. Pausing
    /// clears transition history, so resuming/late joining never replays a latch.
    pub fn update_spatial(
        &mut self,
        snapshot: Option<&Snapshot>,
        pilot: &str,
        connected: bool,
        now_ms: u64,
        listener: Option<AudioListener>,
        paused: bool,
    ) -> (Mix, Vec<Cue>) {
        let (mut mix, mut cues) = self.update(snapshot, pilot, connected, now_ms);
        if paused {
            self.previous = None;
            self.spatial_previous = None;
            mix.active = false;
            mix.engine = 0.;
            mix.mining = 0.;
            mix.warp = 0.;
            cues.clear();
            return (mix, cues);
        }
        let current = snapshot
            .filter(|s| {
                connected && now_ms >= s.server_time_ms && now_ms - s.server_time_ms <= STALE_MS
            })
            .and_then(|s| s.pilots.iter().find(|p| p.name == pilot).map(|p| (s, p)));
        let Some(((snapshot, current), listener)) =
            current.zip(listener.filter(|l| l.spatial(l.position_m, 1.).is_some()))
        else {
            self.spatial_previous = None;
            return (mix, cues);
        };
        if self
            .spatial_previous
            .as_ref()
            .is_some_and(|old| snapshot.server_time_ms < old.time)
        {
            return (mix, cues);
        }
        let compatible = self.spatial_previous.as_ref().is_some_and(|old| {
            old.pilot.name == current.name
                && old.pilot.system_id == current.system_id
                && old.pilot.ship_class == current.ship_class
                && old.pilot.drone_bay.as_ref().map(|b| &b.binding)
                    == current.drone_bay.as_ref().map(|b| &b.binding)
                && snapshot.server_time_ms >= old.time
                && snapshot.server_time_ms - old.time <= STALE_MS
        });
        if !compatible {
            self.spatial_previous = None;
        }
        spatial_loops(snapshot, current, now_ms, listener, &mut mix);
        if let Some(old) = &self.spatial_previous {
            // Accepted commands can change state without advancing the simulation
            // clock. Compare actual ordered state edges even at the same time;
            // identical repeated snapshots have no edge to replay.
            spatial_transitions(old, snapshot, current, listener, &mut cues);
        }
        self.spatial_previous = Some(SpatialSample {
            time: snapshot.server_time_ms,
            pilot: current.clone(),
            asteroid_ore: snapshot
                .asteroids
                .iter()
                .filter(|a| a.system_id == current.system_id)
                .map(|a| (a.id, a.ore))
                .collect(),
        });
        (mix, cues)
    }
}

pub(super) fn supported_bay(p: &PilotView) -> Option<&DroneBayPublicState> {
    super::flight3d::bay_view_available(p)
        .then_some(p.drone_bay.as_ref())
        .flatten()
}

fn parent_pose(p: &PilotView) -> Option<BayPose> {
    let f = p.flight.as_ref()?;
    let pose = BayPose {
        position_m: f.position,
        orientation_xyzw: f.orientation,
    };
    pose.valid().then_some(pose)
}

fn world_point(p: &PilotView, point: [f64; 3]) -> Option<[f64; 3]> {
    parent_pose(p)?
        .compose(BayPose {
            position_m: point,
            orientation_xyzw: [0., 0., 0., 1.],
        })
        .map(|p| p.position_m)
}

pub(super) fn spatial_loops(
    s: &Snapshot,
    p: &PilotView,
    now: u64,
    listener: AudioListener,
    mix: &mut Mix,
) {
    let mut emitters = Vec::new();
    let mut add = |sound, position, level| {
        if let Some((gain, pan)) = listener.spatial(position, level) {
            if gain > 0.0001 {
                emitters.push(SpatialLoop { sound, gain, pan });
            }
        }
    };
    let bay = supported_bay(p);
    if let Some(bay) = bay {
        let held = bay.machinery.cleanup.as_ref().is_some_and(|c| c.held)
            || bay.motions.iter().any(|m| m.phase == DroneBayPhase::Held);
        if !held {
            for group in &bay.machinery.groups {
                if let Some((pose, velocity)) = group.segment.and_then(|seg| seg.sample(now)) {
                    let speed = magnitude(velocity);
                    if speed > 1e-6 {
                        if let Some(position) = world_point(p, pose.position_m) {
                            add(
                                if group.semantic_id.starts_with("door/") {
                                    SpatialSound::Door
                                } else {
                                    SpatialSound::Handling
                                },
                                position,
                                (0.22 + speed.min(1.) * 0.28) as f32,
                            );
                        }
                    }
                }
            }
        }
    }
    for drone in p.drones.iter().filter(|_| normal(p)) {
        if drone.hit_points == 0 || now.saturating_sub(drone.updated_at_ms) > STALE_MS {
            continue;
        }
        if p.drone_bay.is_some() && bay.is_none() {
            continue;
        }
        if let Some(motion) = p.drone_bay.as_ref().and_then(|b| b.constrained(drone.id)) {
            // Velocity-based drive-motion sonification, not measured thrust:
            // DroneView has no acceleration/force channel. Internal carriage
            // transport deliberately does not produce this drive sound.
            if motion.frame == DroneBayMotionFrame::World && motion.phase != DroneBayPhase::Held {
                if let Some((pose, velocity)) = motion.sample(now) {
                    let speed = magnitude(velocity);
                    if speed > 1e-6 {
                        add(
                            SpatialSound::DriveMotion,
                            pose.position_m,
                            (speed / DRONE_SPEED).sqrt().min(1.) as f32 * 0.6,
                        );
                    }
                }
            }
            continue;
        }
        let speed = magnitude(drone.velocity);
        if speed.is_finite() && speed > 1e-6 && !matches!(drone.mode, DroneMode::Hold) {
            add(
                SpatialSound::DriveMotion,
                drone.position,
                (speed / DRONE_SPEED).sqrt().min(1.) as f32 * 0.6,
            );
        }
        if let Some(target_id) = super::flight3d::drone_mining_target(p, drone, now) {
            if let Some(asteroid) = s
                .asteroids
                .iter()
                .find(|a| a.id == target_id && a.system_id == p.system_id && a.ore > 0)
            {
                if let Some(position) =
                    super::flight3d::drone_work_head_world(p, drone, now, Some(asteroid.position))
                {
                    add(SpatialSound::Mining, position, 0.35);
                }
            }
        }
    }
    // Highest audible emitters win the fixed callback budget, deterministically.
    emitters.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    for (slot, emitter) in mix.spatial.iter_mut().zip(emitters) {
        *slot = emitter;
    }
}

fn spatial_transitions(
    old: &SpatialSample,
    s: &Snapshot,
    p: &PilotView,
    listener: AudioListener,
    cues: &mut Vec<Cue>,
) {
    let mut emit = |event, position| {
        if let Some((gain, pan)) = listener.spatial(position, 0.8) {
            if gain > 0.0001 {
                cues.push(Cue::Spatial { event, gain, pan });
            }
        }
    };
    if let (Some(before), Some(after)) = (supported_bay(&old.pilot), supported_bay(p)) {
        for next in &after.cradles {
            let Some(previous) = before
                .cradles
                .iter()
                .find(|c| c.drone_id == next.drone_id && c.home_cradle == next.home_cradle)
            else {
                continue;
            };
            let event = match (previous.presence, next.presence) {
                (DroneBayPresence::Stored, DroneBayPresence::Active) => Some(SpatialEvent::Release),
                (DroneBayPresence::Active, DroneBayPresence::Stored) => Some(SpatialEvent::Secure),
                _ => None,
            };
            if let (Some(event), Some(position)) =
                (event, world_point(p, next.body_pose.position_m))
            {
                emit(event, position);
            }
        }
        if before.machinery.door_fraction > 0.
            && after.machinery.door_fraction == 0.
            && after.machinery.door_segment.is_none()
        {
            if let Some(position) = world_point(
                p,
                drone_bay_layout::BayMechanismId::DoorLower
                    .reference_pose()
                    .position_m,
            ) {
                emit(SpatialEvent::DoorLatch, position);
            }
        }
    }
    if !normal(p) || !normal(&old.pilot) {
        return;
    }
    for previous in &old.pilot.drones {
        if !super::flight3d::drone_mining_yield_confirmed(&old.pilot, p, previous.id)
            || previous.class != DroneClass::Mining
            || previous.hit_points == 0
            || constrained(&old.pilot, previous.id)
        {
            continue;
        }
        let Some(target_id) = super::flight3d::drone_mining_target(&old.pilot, previous, old.time)
        else {
            continue;
        };
        let Some(cycle) = &previous.mining_cycle else {
            continue;
        };
        if cycle.completes_at_ms <= old.time || cycle.completes_at_ms > s.server_time_ms {
            continue;
        }
        let Some(current) = p.drones.iter().find(|d| {
            d.id == previous.id
                && d.hit_points > 0
                && d.class == DroneClass::Mining
                && d.mode == previous.mode
                && !constrained(p, d.id)
        }) else {
            continue;
        };
        if super::flight3d::drone_mining_target(p, current, s.server_time_ms) != Some(target_id) {
            continue;
        }
        // Shared cargo/asteroid deltas alone cannot identify which miner worked.
        // Require the same entity's cycle to advance. Ambiguous terminal depletion
        // has no per-drone receipt and therefore deliberately makes no hit cue.
        if !current.mining_cycle.as_ref().is_some_and(|next| {
            next.started_at_ms >= cycle.completes_at_ms
                && next.completes_at_ms > cycle.completes_at_ms
        }) {
            continue;
        }
        if let Some(asteroid) = s.asteroids.iter().find(|a| {
            a.id == target_id
                && a.system_id == p.system_id
                && old.asteroid_ore.get(&a.id).is_some_and(|ore| a.ore < *ore)
        }) {
            // The exact full-attitude work-head mount also drives the beam.
            if let Some(position) = super::flight3d::drone_work_head_world(
                p,
                current,
                s.server_time_ms,
                Some(asteroid.position),
            ) {
                emit(SpatialEvent::MiningImpact, position);
            }
        }
    }
}
