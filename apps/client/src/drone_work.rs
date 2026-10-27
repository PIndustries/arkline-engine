//! Presentation of confirmed drone work. No deadline predicts a successful yield.
use super::*;
use arkline_protocol::{DroneClass, DroneMode, DroneView};
use std::collections::BTreeMap;

pub(super) fn head_transform(
    pilot: &PilotView,
    drone: &DroneView,
    now: u64,
    target: Option<[f64; 3]>,
) -> Option<Mat4> {
    let sampled = drone_bay_scene::camera_sample(pilot, drone, now)?;
    let rotation = drone_attitude(&sampled)?;
    let pivot = Vec3::from_array(drone_models::MINING_WORK_HEAD_PIVOT_M.map(|v| v as f32));
    let center = drone_present_position(&sampled, now);
    let aim = if let Some(target) = target {
        let delta = Vec3::from_array(std::array::from_fn(|i| (target[i] - center[i]) as f32));
        let direction = (rotation.inverse() * delta - pivot).normalize_or_zero();
        if !direction.is_finite() || direction.length_squared() < 0.99 {
            return None;
        }
        Quat::from_rotation_arc(Vec3::X, direction)
    } else {
        Quat::IDENTITY
    };
    let pivot = pivot * 0.01;
    Some(Mat4::from_translation(pivot) * Mat4::from_quat(aim) * Mat4::from_translation(-pivot))
}
pub(super) fn work_head_world(
    pilot: &PilotView,
    drone: &DroneView,
    now: u64,
    target: Option<[f64; 3]>,
) -> Option<[f64; 3]> {
    let sampled = drone_bay_scene::camera_sample(pilot, drone, now)?;
    let rotation = drone_attitude(&sampled)?;
    let transform = head_transform(pilot, drone, now, target)?;
    let local = transform.transform_point3(Vec3::from_array(
        drone_models::MINING_WORK_HEAD_M.map(|x| (x * 0.01) as f32),
    )) * 100.;
    let center = drone_present_position(&sampled, now);
    if let Some(target) = target {
        let delta = Vec3::from_array(std::array::from_fn(|i| (target[i] - center[i]) as f32));
        let end = rotation.inverse() * delta * 0.01;
        let start = local * 0.01;
        let direction = (end - start).normalize_or_zero();
        let distance = end.distance(start);
        let model = drone_models::model(drone.class);
        let body_count = model.work_head_batch_start.unwrap_or(model.batches.len());
        if model.batches[..body_count].iter().any(|batch| {
            ray_mesh_hit(
                start + direction * 0.00001,
                direction,
                Mat4::IDENTITY,
                &batch.vertices,
            )
            .is_some_and(|hit| hit < distance)
        }) {
            return None;
        }
    }
    let offset = rotation * local;
    let result = std::array::from_fn(|i| center[i] + f64::from(offset[i]));
    result.iter().all(|v| v.is_finite()).then_some(result)
}
pub(super) fn target_position(
    frame: &Frame,
    pilot: &PilotView,
    drone: &DroneView,
) -> Option<[f64; 3]> {
    if !frame.presentation_live() {
        return None;
    }
    let target = mining_target(pilot, drone, frame.now)?;
    frame
        .scene
        .asteroids
        .iter()
        .find(|a| a.id == target && a.system_id == pilot.system_id && a.ore > 0)
        .map(|a| a.position)
}
pub(super) fn part_transform(
    frame: &Frame,
    pilot: &PilotView,
    drone: &DroneView,
    index: usize,
) -> Mat4 {
    let model = drone_models::model(drone.class);
    if model
        .work_head_batch_start
        .is_some_and(|start| index >= start)
    {
        // A paused or stale view retains the last confirmed joint aim; only
        // work light/beam/audio is disabled. It must not snap back to neutral.
        let target = mining_target(pilot, drone, frame.snapshot_time).and_then(|id| {
            frame
                .scene
                .asteroids
                .iter()
                .find(|a| a.id == id && a.system_id == pilot.system_id && a.ore > 0)
                .map(|a| a.position)
        });
        head_transform(pilot, drone, frame.authoritative_motion_time(), target)
            .unwrap_or(Mat4::IDENTITY)
    } else {
        Mat4::IDENTITY
    }
}

/// Evidence only: callers still require a specific completed cycle and target
/// depletion. A drone-to-ship transfer is never extraction into a drone hold.
pub(super) fn mining_yield_confirmed(
    previous: &PilotView,
    current: &PilotView,
    drone_id: u64,
) -> bool {
    if previous.name != current.name || previous.system_id != current.system_id {
        return false;
    }
    match (&previous.drone_cargo, &current.drone_cargo) {
        (None, None) => current.ore > previous.ore,
        (Some(before), Some(after))
            if before.binding == after.binding
                && before.validate_projection(previous).is_ok()
                && after.validate_projection(current).is_ok() =>
        {
            let before = before.holds.iter().find(|hold| hold.drone_id == drone_id);
            let after = after.holds.iter().find(|hold| hold.drone_id == drone_id);
            matches!((before, after), (Some(before), Some(after)) if after.ore_units > before.ore_units)
        }
        _ => false,
    }
}

pub(super) fn mining_target(pilot: &PilotView, drone: &DroneView, now: u64) -> Option<u64> {
    if drone.class != DroneClass::Mining
        || drone.hit_points == 0
        || !drone.signal_strength.is_finite()
        || drone.signal_strength <= 0.
        || now.saturating_sub(drone.updated_at_ms) > 1500
        || !matches!(pilot.location, Location::Belt | Location::Station)
    {
        return None;
    }
    if pilot.drone_bay.is_some() {
        let bay = drone_bay_scene::validated_bay(pilot)?;
        if bay.constrained(drone.id).is_some() {
            return None;
        }
    }
    if let Some(cargo) = &pilot.drone_cargo {
        cargo.validate_projection(pilot).ok()?;
        if cargo.operations.iter().any(|operation| {
            operation.drone_id == drone.id
                && (operation.active()
                    || cargo
                        .cleanup
                        .as_ref()
                        .is_some_and(|cleanup| cleanup.operation_id == operation.operation_id))
        }) {
            return None;
        }
    }
    let DroneMode::Mining { target_id } = drone.mode else {
        return None;
    };
    let cycle = drone.mining_cycle.as_ref()?;
    if cycle.completes_at_ms <= cycle.started_at_ms
        || now < cycle.started_at_ms
        || now >= cycle.completes_at_ms
    {
        return None;
    }
    Some(target_id)
}

#[derive(Default)]
pub(super) struct ConfirmedImpacts {
    previous: Option<(u64, FrameScene)>,
    flashes: BTreeMap<(String, u64), (u64, u64)>,
}
impl ConfirmedImpacts {
    pub(super) fn update(&mut self, frame: &Frame) {
        if !frame.presentation_live() {
            self.previous = None;
            self.flashes.clear();
            return;
        }
        if let Some((time, _)) = &self.previous {
            if *time == frame.snapshot_time {
                return;
            }
            if *time > frame.snapshot_time {
                self.previous = None;
                self.flashes.clear();
            }
        }
        if let Some((time, old)) = &self.previous {
            if frame.snapshot_time.saturating_sub(*time) <= 1500 {
                for pilot in &frame.scene.pilots {
                    let Some(prior) = old
                        .pilots
                        .iter()
                        .find(|p| p.name == pilot.name && p.system_id == pilot.system_id)
                    else {
                        continue;
                    };
                    for drone in fleet(pilot) {
                        if !mining_yield_confirmed(prior, pilot, drone.id) {
                            continue;
                        }
                        let Some(before) = fleet(prior).into_iter().find(|d| d.id == drone.id)
                        else {
                            continue;
                        };
                        let Some(target) = mining_target(prior, before, *time) else {
                            continue;
                        };
                        let Some(cycle) = before.mining_cycle.as_ref() else {
                            continue;
                        };
                        if cycle.completes_at_ms > frame.snapshot_time || drone.hit_points == 0 {
                            continue;
                        }
                        if drone.mining_cycle.as_ref().is_none_or(|next| {
                            next.started_at_ms < cycle.completes_at_ms
                                || next.completes_at_ms <= cycle.completes_at_ms
                        }) {
                            continue;
                        }

                        if mining_target(pilot, drone, frame.snapshot_time) != Some(target) {
                            continue;
                        }
                        if !matches!(drone.mode,DroneMode::Mining{target_id} if target_id==target) {
                            continue;
                        }
                        if pilot
                            .drone_bay
                            .as_ref()
                            .is_some_and(|b| b.constrained(drone.id).is_some())
                        {
                            continue;
                        }
                        let previous_ore =
                            old.asteroids.iter().find(|a| a.id == target).map(|a| a.ore);
                        let current_ore = frame
                            .scene
                            .asteroids
                            .iter()
                            .find(|a| a.id == target)
                            .map(|a| a.ore);
                        if matches!((previous_ore,current_ore),(Some(a),Some(b)) if b<a) {
                            self.flashes.insert(
                                (pilot.name.clone(), drone.id),
                                (target, frame.snapshot_time),
                            );
                        }
                    }
                }
            } else {
                self.flashes.clear();
            }
        }
        self.flashes
            .retain(|_, (_, time)| frame.now.saturating_sub(*time) < 350);
        self.previous = Some((frame.snapshot_time, (*frame.scene).clone()));
    }
    pub(super) fn strength(&self, pilot: &str, drone: u64, target: u64, now: u64) -> f32 {
        self.flashes
            .get(&(pilot.to_owned(), drone))
            .filter(|(id, _)| *id == target)
            .map_or(0., |(_, time)| {
                1. - (now.saturating_sub(*time) as f32 / 350.).clamp(0., 1.)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pilot() -> PilotView {
        serde_json::from_value(serde_json::json!({"name":"fixture","credits":0,"ore":0,"location":Location::Belt,"system_id":3,"drones":[{
            "id":8,"class":"mining","hit_points":100,"signal_strength":1.,
            "position":[100.,200.,300.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],
            "orientation_xyzw":[0.,0.,0.,1.],"mode":{"kind":"mining","target_id":1},
            "mining_cycle":{"started_at_ms":1000,"completes_at_ms":1100},"updated_at_ms":1000
        }]})).unwrap()
    }
    #[test]
    fn cargo_owned_yield_drives_impact_without_ship_credit_and_never_borrows_peer_or_transfer() {
        let snapshot = crate::sound_scene::tests::cargo_mining_snapshot();
        let mut f = frame();
        Arc::make_mut(&mut f.scene).pilots = snapshot.pilots;
        Arc::make_mut(&mut f.scene).asteroids = snapshot.asteroids;
        let mut tracker = ConfirmedImpacts::default();
        tracker.update(&f);
        let before = f.scene.pilots[0].clone();
        f.now = 1100;
        f.snapshot_time = 1100;
        let scene = Arc::make_mut(&mut f.scene);
        scene.asteroids[0].ore = 90;
        let pilot = &mut scene.pilots[0];
        pilot.drones[0].updated_at_ms = 1100;
        pilot.drones[0].mining_cycle = Some(arkline_protocol::DroneMiningCycle {
            started_at_ms: 1100,
            completes_at_ms: 9100,
        });
        let cargo = pilot.drone_cargo.as_mut().unwrap();
        cargo.holds[0].ore_units = 10;
        cargo.incoming_reserved_units = 10;
        assert_eq!(pilot.ore, before.ore);
        assert!(mining_yield_confirmed(&before, pilot, 6));
        assert!(!mining_yield_confirmed(&before, pilot, 7));
        tracker.update(&f);
        assert_eq!(tracker.strength("Pilot", 6, 1, 1100), 1.);
        let loaded = f.scene.pilots[0].clone();
        let mut transferred = loaded.clone();
        transferred.ore += 10;
        let cargo = transferred.drone_cargo.as_mut().unwrap();
        cargo.holds[0].ore_units = 0;
        cargo.incoming_reserved_units = 0;
        assert!(!mining_yield_confirmed(&loaded, &transferred, 6));
        let mut invalid = loaded.clone();
        invalid
            .drone_cargo
            .as_mut()
            .unwrap()
            .binding
            .registration
            .receiver_revision = 999;
        assert!(!mining_yield_confirmed(&before, &invalid, 6));
        assert_eq!(mining_target(&invalid, &invalid.drones[0], 1100), None);
        f.paused = true;
        tracker.update(&f);
        assert_eq!(tracker.strength("Pilot", 6, 1, 1100), 0.);
        f.paused = false;
        tracker.update(&f);
        assert_eq!(tracker.strength("Pilot", 6, 1, 1100), 0.);
    }
    #[test]
    fn queued_same_id_ore_service_disables_work_without_silencing_other_miners() {
        use arkline_protocol::*;
        let mut snapshot = crate::sound_scene::tests::cargo_mining_snapshot();
        let pilot = &mut snapshot.pilots[0];
        let mut peer = pilot.drones[0].clone();
        peer.id = 7;
        pilot.drones.push(peer);
        pilot.drone_bay.as_mut().unwrap().cradles[6].presence = DroneBayPresence::Active;
        let cargo = pilot.drone_cargo.as_mut().unwrap();
        cargo.holds[0].ore_units = 10;
        cargo.incoming_reserved_units = 10;
        cargo.operations.push(DroneDropoffOperation {
            operation_id: 1,
            binding: cargo.binding.clone(),
            drone_id: 6,
            requested_resume_mining: true,
            cancel_requested: false,
            followup: DroneDropoffFollowup::ResumeMining {
                intent_generation: 0,
                target_id: 1,
                job_id: None,
            },
            requested_units: 10,
            committed_units: 0,
            remaining_units: 10,
            phase: DroneDropoffPhase::Queued,
            held_phase: None,
            accepted_at_ms: 1000,
            updated_at_ms: 1000,
            next_batch_at_ms: None,
            message: "Queued".into(),
            receipts: vec![],
        });
        pilot
            .drone_cargo
            .as_ref()
            .unwrap()
            .validate_projection(pilot)
            .unwrap();
        assert_eq!(mining_target(pilot, &pilot.drones[0], 1050), None);
        assert_eq!(mining_target(pilot, &pilot.drones[1], 1050), Some(1));
    }
    #[test]
    fn actual_work_head_follows_full_attitude_and_never_uses_body_center() {
        let p = pilot();
        let mut d = p.drones[0].clone();
        let origin = work_head_world(&p, &d, 1000, None).unwrap();
        assert!((origin[0] - 102.45).abs() < 1e-5);
        assert!((origin[1] - 199.9).abs() < 1e-5);
        d.orientation_xyzw = Some(
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)
                .to_array()
                .map(f64::from),
        );
        let turned = work_head_world(&p, &d, 1000, None).unwrap();
        assert!((turned[0] - 100.1).abs() < 1e-5);
        assert!((turned[1] - 202.45).abs() < 1e-5);
        d.orientation_xyzw = Some([0.; 4]);
        assert!(work_head_world(&p, &d, 1000, None).is_none());
    }
    #[test]
    fn beam_requires_fresh_live_cycle_not_an_expired_prediction() {
        let p = pilot();
        let mut d = p.drones[0].clone();
        assert_eq!(mining_target(&p, &d, 1050), Some(1));
        assert_eq!(mining_target(&p, &d, 1100), None);
        d.mode = DroneMode::Hold;
        assert_eq!(mining_target(&p, &d, 1050), None);
        d.mode = DroneMode::Mining { target_id: 1 };
        d.updated_at_ms = 0;
        assert_eq!(mining_target(&p, &d, 3000), None);
        d.updated_at_ms = 1000;
        d.signal_strength = f64::NAN;
        assert_eq!(mining_target(&p, &d, 1050), None);
        d.signal_strength = 1.;
        d.hit_points = 0;
        assert_eq!(mining_target(&p, &d, 1050), None);
    }
    fn frame() -> Frame {
        let mut f = super::super::tests::frame_for_scene_clone_test();
        let scene = Arc::make_mut(&mut f.scene);
        scene.pilots = vec![pilot()];
        scene.asteroids=vec![serde_json::from_value(serde_json::json!({"id":1,"name":"fixture rock","system_id":3,"position":[120.,200.,300.],"radius":5.,"ore":100})).unwrap()];
        f
    }
    #[test]
    fn impact_requires_confirmed_yield_and_depletion_and_never_replays_after_pause() {
        let mut f = frame();
        let mut tracker = ConfirmedImpacts::default();
        tracker.update(&f);
        f.now = 1100;
        f.snapshot_time = 1100;
        {
            let s = Arc::make_mut(&mut f.scene);
            s.pilots[0].ore = 10;
            s.asteroids[0].ore = 90;
            s.pilots[0].drones[0].updated_at_ms = 1100;
            s.pilots[0].drones[0].mining_cycle = Some(arkline_protocol::DroneMiningCycle {
                started_at_ms: 1100,
                completes_at_ms: 9100,
            });
        }
        tracker.update(&f);
        assert_eq!(tracker.strength("fixture", 8, 1, 1100), 1.);
        tracker.update(&f);
        assert!(tracker.strength("fixture", 8, 1, 1200) < 1.);
        assert_eq!(tracker.strength("fixture", 8, 1, 1450), 0.);
        f.paused = true;
        tracker.update(&f);
        assert_eq!(tracker.strength("fixture", 8, 1, 1100), 0.);
        f.paused = false;
        tracker.update(&f);
        assert_eq!(tracker.strength("fixture", 8, 1, 1100), 0.);
        let mut no_yield = ConfirmedImpacts::default();
        let mut a = frame();
        no_yield.update(&a);
        a.now = 1100;
        a.snapshot_time = 1100;
        no_yield.update(&a);
        assert_eq!(no_yield.strength("fixture", 8, 1, 1100), 0.);
        a.connected = false;
        no_yield.update(&a);
        a.connected = true;
        Arc::make_mut(&mut a.scene).pilots[0].ore = 10;
        no_yield.update(&a);
        assert_eq!(no_yield.strength("fixture", 8, 1, 1100), 0.);
    }
    #[test]
    fn orbit_tangent_body_keeps_actual_gimbal_axis_on_asteroid_and_clear_of_hull() {
        let p = pilot();
        let mut d = p.drones[0].clone();
        // +X body tangent to orbit, asteroid lies90degrees to port.
        let target = [100., 200., 200.];
        let joint = head_transform(&p, &d, 1050, Some(target)).unwrap();
        let start = work_head_world(&p, &d, 1050, Some(target))
            .expect("side working arc clears actual body");
        let axis = joint.transform_vector3(Vec3::X).normalize();
        let toward =
            Vec3::from_array(std::array::from_fn(|i| (target[i] - start[i]) as f32)).normalize();
        assert!(axis.dot(toward) > 0.99999);
        assert!(axis.dot(Vec3::X).abs() < 0.03);
        // Whole-drone authoritative pose is unchanged; only head batches move.
        assert_eq!(d.orientation_xyzw, Some([0., 0., 0., 1.]));
        d.orientation_xyzw = Some(Quat::from_rotation_x(0.9).to_array().map(f64::from));
        let start = work_head_world(&p, &d, 1050, Some(target)).unwrap();
        let joint = head_transform(&p, &d, 1050, Some(target)).unwrap();
        let axis = drone_attitude(&d).unwrap() * joint.transform_vector3(Vec3::X).normalize();
        let toward =
            Vec3::from_array(std::array::from_fn(|i| (target[i] - start[i]) as f32)).normalize();
        assert!(axis.dot(toward) > 0.99999);
    }
    #[test]
    fn unchanged_due_peer_never_borrows_another_miners_yield() {
        let mut f = frame();
        let mut peer = f.scene.pilots[0].drones[0].clone();
        peer.id = 9;
        Arc::make_mut(&mut f.scene).pilots[0].drones.push(peer);
        let mut tracker = ConfirmedImpacts::default();
        tracker.update(&f);
        f.now = 1100;
        f.snapshot_time = 1100;
        let s = Arc::make_mut(&mut f.scene);
        s.pilots[0].ore = 10;
        s.asteroids[0].ore = 90;
        s.pilots[0].drones[0].mining_cycle = Some(arkline_protocol::DroneMiningCycle {
            started_at_ms: 1100,
            completes_at_ms: 9100,
        });
        tracker.update(&f);
        assert_eq!(tracker.strength("fixture", 8, 1, 1100), 1.);
        assert_eq!(tracker.strength("fixture", 9, 1, 1100), 0.);
    }
    #[test]
    fn backward_work_bore_is_not_drawn_through_its_own_actual_body() {
        let p = pilot();
        let d = &p.drones[0];
        assert!(work_head_world(&p, d, 1050, Some([0., 199.9, 300.])).is_none());
    }
    #[test]
    fn pause_keeps_confirmed_joint_pose_while_disabling_work() {
        let mut f = frame();
        let p = f.scene.pilots[0].clone();
        let d = &p.drones[0];
        let index = drone_models::model(d.class).work_head_batch_start.unwrap();
        let before = part_transform(&f, &p, d, index);
        f.paused = true;
        assert_eq!(part_transform(&f, &p, d, index), before);
        assert!(target_position(&f, &p, d).is_none());
        f.paused = false;
        f.connected = false;
        assert_eq!(part_transform(&f, &p, d, index), before);
        assert!(target_position(&f, &p, d).is_none());
    }
}
