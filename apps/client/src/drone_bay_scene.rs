//! Presentation of confirmed persistent stock. This module never creates a drone,
//! transfers HP, changes a cradle, or completes a launch/recovery operation.
use super::*;
use arkline_protocol::{
    drone_bay_geometry::BayPose, DroneBayMotionFrame, DroneBayPresence, DroneBayPublicState,
};

fn carrier_pose(pilot: &PilotView, now: u64) -> BayPose {
    BayPose {
        position_m: display_position(pilot, now),
        orientation_xyzw: ship_orientation_at(pilot, now).to_array().map(f64::from),
    }
}

fn scene_transform(space: Space, pose: BayPose) -> Option<Mat4> {
    if !pose.valid() {
        return None;
    }
    let model = transform(
        space.scene(pose.position_m),
        Vec3::ONE,
        Quat::from_array(pose.orientation_xyzw.map(|v| v as f32)).normalize(),
    );
    model.is_finite().then_some(model)
}

/// Only Stored stock appears here. Active entities remain in the existing fleet
/// draw and use constrained_transform, so one authoritative ID is drawn once.
pub(super) fn stored_instances(
    pilot: &PilotView,
    bay: &DroneBayPublicState,
    space: Space,
    now: u64,
) -> Vec<(u64, arkline_protocol::DroneClass, Mat4)> {
    let parent = carrier_pose(pilot, now);
    bay.cradles
        .iter()
        .filter(|entry| {
            entry.presence == DroneBayPresence::Stored
                && entry.hit_points > 0
                && !fleet(pilot).iter().any(|drone| drone.id == entry.drone_id)
        })
        .filter_map(|entry| {
            Some((
                entry.drone_id,
                entry.class,
                scene_transform(space, parent.compose(entry.body_pose)?)?,
            ))
        })
        .collect()
}

pub(super) fn constrained_transform(
    pilot: &PilotView,
    bay: &DroneBayPublicState,
    drone_id: u64,
    space: Space,
    now: u64,
) -> Option<Mat4> {
    let motion = bay.constrained(drone_id)?;
    let (pose, _) = motion.sample(now)?;
    let world = match motion.frame {
        DroneBayMotionFrame::Body => carrier_pose(pilot, now).compose(pose)?,
        DroneBayMotionFrame::World => pose,
    };
    scene_transform(space, world)
}

/// Same accepted pose sampler for mesh and every camera. No second extrapolation
/// is applied after a constrained segment has been sampled.
pub(super) fn camera_sample(
    pilot: &PilotView,
    drone: &arkline_protocol::DroneView,
    now: u64,
) -> Option<arkline_protocol::DroneView> {
    let mut result = drone.clone();
    drone_attitude(drone)?;
    if pilot.drone_cargo.is_some() {
        let cargo = validated_cargo(pilot)?;
        if cargo
            .motion
            .as_ref()
            .is_some_and(|m| m.drone_id == drone.id)
        {
            let world = cargo_world_pose(pilot, cargo, now)?;
            result.position = world.position_m;
            result.orientation_xyzw = Some(world.orientation_xyzw);
            result.heading = world.transform_vector([1., 0., 0.])?;
            result.velocity = [0.; 3];
            result.updated_at_ms = now;
            return Some(result);
        }
    }
    if pilot.drone_bay.is_some() {
        let bay = validated_bay(pilot)?;
        if let Some(motion) = bay.constrained(drone.id) {
            let (pose, _) = motion.sample(now)?;
            let world = match motion.frame {
                DroneBayMotionFrame::Body => carrier_pose(pilot, now).compose(pose)?,
                DroneBayMotionFrame::World => pose,
            };
            result.position = world.position_m;
            result.orientation_xyzw = Some(world.orientation_xyzw);
            result.heading = world.transform_vector([1., 0., 0.])?;
            result.velocity = [0.; 3];
            result.updated_at_ms = now;
        }
    }
    Some(result)
}

pub(super) fn validated_cargo(
    pilot: &PilotView,
) -> Option<&arkline_protocol::DroneCargoPublicState> {
    let cargo = pilot.drone_cargo.as_ref()?;
    cargo.validate_projection(pilot).ok()?;
    ore_receiver_source::installed().ok()?;
    Some(cargo)
}

fn cargo_world_pose(
    pilot: &PilotView,
    cargo: &arkline_protocol::DroneCargoPublicState,
    now: u64,
) -> Option<BayPose> {
    let motion = cargo.motion.as_ref()?;
    let (pose, _) = cargo.sample_motion(now)?;
    match motion.frame {
        DroneBayMotionFrame::Body => carrier_pose(pilot, now).compose(pose),
        DroneBayMotionFrame::World => Some(pose),
    }
}

pub(super) fn validated_bay(pilot: &PilotView) -> Option<&DroneBayPublicState> {
    use arkline_protocol::drone_bay_layout as geometry;
    let bay = pilot.drone_bay.as_ref()?;
    let host = pilot.flight.as_ref()?.docking.as_ref()?;
    if host.version != arkline_protocol::DOCKING_VERSION || pilot.ship_class != ShipClass::Pioneer {
        return None;
    }
    let expected = arkline_protocol::DroneBayBinding {
        vessel_id: host.host_vessel_id.clone(),
        geometry_revision: geometry::DRONE_BAY_REGISTRATION_REVISION,
        source_sha256: geometry::DRONE_BAY_SOURCE_SHA256.into(),
    };
    bay.validate_projection(
        &expected,
        &geometry::home_poses(),
        &geometry::mechanism_ids(),
        &pilot.drones,
    )
    .ok()?;
    Some(bay)
}

/// Input and output model parts are body-metres (the registered cabin recipe).
/// Dynamic source vertices retain their source units; the old model's100x scale
/// is retained after the exact body-space delta, never applied twice.
pub(super) fn apply_mechanisms(parts: &[ModelPart], pilot: &PilotView, now: u64) -> Vec<ModelPart> {
    let Some(bay) = validated_bay(pilot) else {
        return parts.to_vec();
    };
    let Ok(geometry) = flight_geometry() else {
        return parts.to_vec();
    };
    let held = bay.machinery.cleanup.as_ref().is_some_and(|c| c.held)
        || bay.machinery.handling_operation.is_some_and(|id| {
            bay.motions
                .iter()
                .any(|m| m.operation_id == id && m.phase == arkline_protocol::DroneBayPhase::Held)
        });
    let mut output: Vec<_> = parts
        .iter()
        .map(|&(mesh, model, color, emission)| {
            let transformed = geometry
                .bay_meshes
                .iter()
                .find(|m| m.mesh == mesh)
                .and_then(|mount| {
                    let state = bay
                        .machinery
                        .groups
                        .iter()
                        .find(|m| m.semantic_id == mount.group_id)?;
                    let current = if held {
                        state.confirmed_pose
                    } else {
                        state
                            .segment
                            .map(|s| s.sample(now).map(|v| v.0))
                            .unwrap_or(Some(state.confirmed_pose))?
                    };
                    let matrix = |p: BayPose| {
                        Mat4::from_rotation_translation(
                            Quat::from_array(p.orientation_xyzw.map(|v| v as f32)).normalize(),
                            Vec3::from_array(p.position_m.map(|v| v as f32)),
                        )
                    };
                    current
                        .valid()
                        .then(|| matrix(current) * matrix(mount.reference_pose).inverse() * model)
                })
                .unwrap_or(model);
            (mesh, transformed, color, emission)
        })
        .collect();
    if let Some(cargo) = validated_cargo(pilot) {
        let roots: Option<std::collections::BTreeMap<_, _>> = cargo
            .groups
            .iter()
            .map(|group| {
                Some((
                    group.semantic_id.clone(),
                    cargo.sample_group(&group.semantic_id, now)?,
                ))
            })
            .collect();
        if let Some(parts) = roots.and_then(|roots| receiver_body_parts(&roots).ok()) {
            output.extend(parts);
        }
    }
    output
}

// Existing authored diffuser centers in ship-body metres. Presentation light
// strength is not electrical watts and creates no simulated power consumer.
const BAY_FIXTURE_CENTERS_M: [[f64; 3]; 3] = [
    [-30.5, 4.43, 6.79],
    [-24.5, 4.43, 6.79],
    [-18.5, 4.43, 6.79],
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContactIndication {
    Stored,
    Supported,
    Released,
    Held,
    Empty,
    Destroyed,
}

fn held_operation(bay: &DroneBayPublicState) -> bool {
    bay.machinery.cleanup.as_ref().is_some_and(|c| c.held)
        || bay.machinery.handling_operation.is_some_and(|id| {
            bay.motions
                .iter()
                .any(|m| m.operation_id == id && m.phase == arkline_protocol::DroneBayPhase::Held)
        })
}
fn contact_indication(bay: &DroneBayPublicState, slot: u8, now: u64) -> Option<ContactIndication> {
    let cradle = bay.cradles.iter().find(|c| c.home_cradle == slot)?;
    if cradle.presence == DroneBayPresence::Destroyed {
        return Some(ContactIndication::Destroyed);
    }
    if cradle.presence == DroneBayPresence::Stored {
        return Some(ContactIndication::Stored);
    }
    let Some(motion) = bay.constrained(cradle.drone_id) else {
        return Some(ContactIndication::Empty);
    };
    if held_operation(bay) {
        return Some(ContactIndication::Held);
    }
    // Both tray top and drone bottom sit0.9m below their resolved root. Matching
    // roots indicate support; real authority disengagement lowers the tray0.20m.
    // This does not create a physical contact force or articulated latch state.
    let tray_id = format!("tray/s{slot:02}");
    let tray = bay
        .machinery
        .groups
        .iter()
        .find(|g| g.semantic_id == tray_id)?;
    let tray_pose = tray
        .segment
        .map_or(Some(tray.confirmed_pose), |s| s.sample(now).map(|v| v.0))?;
    let (body, _) = motion.sample(now)?;
    let supported_attitude = tray_pose.compose(BayPose {
        position_m: [0.; 3],
        orientation_xyzw: arkline_protocol::drone_bay_layout::home_pose(slot)?.orientation_xyzw,
    })?;
    let attitude_dot = body
        .orientation_xyzw
        .iter()
        .zip(supported_attitude.orientation_xyzw)
        .map(|(a, b)| a * b)
        .sum::<f64>()
        .abs();
    if motion.frame == DroneBayMotionFrame::Body
        && (attitude_dot - 1.).abs() <= 1e-6
        && body
            .position_m
            .iter()
            .zip(tray_pose.position_m)
            .all(|(a, b)| (a - b).abs() <= 0.02)
    {
        Some(ContactIndication::Supported)
    } else {
        Some(ContactIndication::Released)
    }
}
// Non-emissive status coating on existing retainers, not newly fitted LEDs.
fn contact_material(indication: ContactIndication) -> (Vec3, f32) {
    match indication {
        ContactIndication::Stored => (Vec3::new(0.16, 0.48, 0.30), 0.0),
        ContactIndication::Supported => (Vec3::new(0.86, 0.48, 0.12), 0.0),
        ContactIndication::Released => (Vec3::new(0.14, 0.64, 0.72), 0.0),
        ContactIndication::Held => (Vec3::new(0.90, 0.33, 0.07), 0.0),
        ContactIndication::Empty => (Vec3::new(0.23, 0.25, 0.26), 0.0),
        ContactIndication::Destroyed => (Vec3::new(0.36, 0.16, 0.12), 0.0),
    }
}

/// Same registered geometry and authoritative transforms as apply_mechanisms.
/// Only existing tray retainers gain contact/status material feedback. They do
/// NOT articulate. Stale/paused/disconnected callers pass live=false and retain
/// source materials; no decorative clock invents machinery activity.
pub(super) fn apply_presentation(
    parts: &[ModelPart],
    pilot: &PilotView,
    now: u64,
    live: bool,
) -> Vec<ModelPart> {
    let mut output = apply_mechanisms(parts, pilot, now);
    let Some(bay) = validated_bay(pilot).filter(|_| live) else {
        return output;
    };
    let (Ok(geometry), Ok(source)) = (flight_geometry(), pioneer_asset()) else {
        return output;
    };
    for (mesh, _, color, emission) in &mut output {
        let Some(mount) = geometry.bay_meshes.iter().find(|m| m.mesh == *mesh) else {
            continue;
        };
        let Some(slot) = mount
            .group_id
            .strip_prefix("tray/s")
            .and_then(|s| s.parse::<u8>().ok())
        else {
            continue;
        };
        let Some(primitive) = docked_source_primitive(*mesh) else {
            continue;
        };
        // Source partition binds precisely each tray's two retainers to this
        // material; never identify unrelated amber geometry by its color.
        if source
            .get(primitive)
            .is_none_or(|p| p.material_name != "Safety amber")
        {
            continue;
        }
        if let Some(indication) = contact_indication(bay, slot, now) {
            (*color, *emission) = contact_material(indication);
        }
    }
    output
}

/// Illumination at three existing authored diffuser centers, not new fixtures.
/// live includes connected, unpaused and fresh authoritative snapshot. Values
/// remain presentation-only; no new load or measured electrical output is claimed.
pub(super) fn fixture_lights(
    pilot: &PilotView,
    space: Space,
    now: u64,
    live: bool,
) -> Vec<local_lighting::Light> {
    let Some(bay) = validated_bay(pilot).filter(|_| live) else {
        return Vec::new();
    };
    let parent = carrier_pose(pilot, now);
    let handling_station = bay.machinery.handling_operation.and_then(|operation| {
        let motion = bay.motions.iter().find(|m| m.operation_id == operation)?;
        bay.cradles
            .iter()
            .find(|c| c.drone_id == motion.drone_id)
            .map(|c| usize::from(c.home_cradle) / 5)
    });
    let held = held_operation(bay);
    BAY_FIXTURE_CENTERS_M
        .iter()
        .enumerate()
        .filter_map(|(station, position)| {
            let position = parent.compose(BayPose {
                position_m: *position,
                orientation_xyzw: [0., 0., 0., 1.],
            })?;
            let active = handling_station == Some(station) || bay.machinery.cleanup.is_some();
            Some(local_lighting::Light {
                position: space.scene(position.position_m),
                range: 11. / 100.,
                color: if held && active {
                    Vec3::new(1., 0.55, 0.23)
                } else {
                    Vec3::new(0.78, 0.92, 1.)
                },
                power: if active {
                    if held {
                        1.1
                    } else {
                        2.2
                    }
                } else {
                    0.55
                },
            })
        })
        .collect()
}

pub(super) fn active_transform(
    pilot: &PilotView,
    drone: &arkline_protocol::DroneView,
    space: Space,
    now: u64,
) -> Option<Mat4> {
    if pilot.drone_cargo.is_some() {
        let cargo = validated_cargo(pilot)?;
        if cargo
            .motion
            .as_ref()
            .is_some_and(|m| m.drone_id == drone.id)
        {
            return scene_transform(space, cargo_world_pose(pilot, cargo, now)?);
        }
    }
    if pilot.drone_bay.is_some() {
        let bay = validated_bay(pilot)?;
        if bay.constrained(drone.id).is_some() {
            return constrained_transform(pilot, bay, drone.id, space, now);
        }
    }
    drone_attitude(drone)?;
    Some(drone_model_transform(space, drone, now))
}

pub(super) fn exterior_body_parts(
    pilot: &PilotView,
    now: u64,
    live: bool,
) -> Option<Vec<ModelPart>> {
    let source = docked_pioneer_parts().ok()?;
    let parts: Vec<_> = source
        .iter()
        .map(|&(mesh, model, color, emission)| {
            (
                mesh,
                Mat4::from_scale(Vec3::splat(100.)) * model,
                color,
                emission,
            )
        })
        .collect();
    Some(apply_presentation(&parts, pilot, now, live))
}

pub(super) fn selected_stored(
    pilot: &PilotView,
    selected: Option<u64>,
) -> Option<&arkline_protocol::DroneCradleView> {
    let id = selected?;
    validated_bay(pilot)?
        .cradles
        .iter()
        .find(|c| c.drone_id == id && c.presence == DroneBayPresence::Stored && c.hit_points > 0)
}

pub(super) fn camera_frame(
    frame: &Frame,
    pilot: &PilotView,
    source: crate::camera_sources::CameraSource,
    aspect: f32,
) -> Option<Frame> {
    use crate::camera_sources::CameraSource;
    validated_bay(pilot)?;
    if !aspect.is_finite() || aspect <= 0. {
        return None;
    }
    let x = match source {
        CameraSource::BayOverview => -24.5,
        CameraSource::BayRack(slot) if slot < 3 => {
            arkline_protocol::drone_bay_layout::home_poses()[usize::from(slot) * 5].position_m[0]
        }
        _ => return None,
    };
    let fov = if source == CameraSource::BayOverview {
        80_f32
    } else {
        75_f32
    }
    .to_radians();
    let parent = carrier_pose(pilot, frame.now);
    let eye = frame.space.scene(
        parent
            .compose(BayPose {
                position_m: [x, -1., -6.95],
                orientation_xyzw: [0., 0., 0., 1.],
            })?
            .position_m,
    );
    let direction = Vec3::from_array(parent.transform_vector([0., 0., 1.])?.map(|v| v as f32));
    let up = Vec3::from_array(parent.transform_vector([0., 1., 0.])?.map(|v| v as f32));
    if !eye.is_finite() || !direction.is_finite() || !up.is_finite() {
        return None;
    }
    let mut feed = frame.clone();
    feed.cockpit = false;
    feed.drone_camera = true;
    feed.camera_drone = None;
    feed.eye = eye;
    feed.rotation = ship_orientation_at(pilot, frame.now);
    let view = Mat4::look_at_rh(eye, eye + direction, up);
    feed.vp = Mat4::perspective_rh_gl(fov, aspect.max(0.1), 0.0005, 1200.) * view;
    feed.cockpit_vp = feed.vp;
    feed.planet_vp = Mat4::perspective_rh_gl(fov, aspect.max(0.1), 1., 300_000.) * view;
    feed.cockpit_occlusion = None;
    feed.docking = None;
    Some(feed)
}

/// Separate fixed observation mounts for the operations workspace. These do
/// not alter the pilot camera or omit geometry to see through closed doors.
pub(super) fn operations_camera_frame(
    frame: &Frame,
    pilot: &PilotView,
    camera: super::BayMonitorCamera,
    aspect: f32,
) -> Option<Frame> {
    validated_bay(pilot)?;
    if !aspect.is_finite() || aspect <= 0.0 {
        return None;
    }
    let (eye_body, target_body, fov): ([f64; 3], [f64; 3], f32) = match camera {
        super::BayMonitorCamera::Wide => ([-18.5, -1.0, -6.95], [-26.0, -1.0, -0.5], 90.0),
        super::BayMonitorCamera::Doorway => ([-32.7, -1.0, -6.9], [-40.0, -1.0, -3.5], 75.0),
        super::BayMonitorCamera::OreReceiver => {
            validated_cargo(pilot)?;
            ([-38.0, 1.6, 9.5], [-39.0, -1.0, 5.0], 90.0)
        }
    };
    let parent = carrier_pose(pilot, frame.now);
    let eye = frame.space.scene(
        parent
            .compose(BayPose {
                position_m: eye_body,
                orientation_xyzw: [0., 0., 0., 1.],
            })?
            .position_m,
    );
    let target = frame.space.scene(
        parent
            .compose(BayPose {
                position_m: target_body,
                orientation_xyzw: [0., 0., 0., 1.],
            })?
            .position_m,
    );
    let up = Vec3::from_array(parent.transform_vector([0., 1., 0.])?.map(|v| v as f32));
    if !eye.is_finite() || !target.is_finite() || !up.is_finite() {
        return None;
    }
    let view = Mat4::look_at_rh(eye, target, up);
    let mut feed = frame.clone();
    feed.cockpit = false;
    feed.drone_camera = true;
    feed.camera_drone = None;
    feed.eye = eye;
    feed.rotation = ship_orientation_at(pilot, frame.now);
    feed.vp = Mat4::perspective_rh_gl(fov.to_radians(), aspect, 0.0005, 1200.) * view;
    feed.cockpit_vp = feed.vp;
    feed.planet_vp = Mat4::perspective_rh_gl(fov.to_radians(), aspect, 1., 300_000.) * view;
    feed.cockpit_occlusion = None;
    feed.docking = None;
    Some(feed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::{drone_bay_layout as geometry, *};
    fn stored_pilot() -> PilotView {
        let mut pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"bay owner","credits":0,"ore":0,"ship_class":ShipClass::Pioneer,
            "location":{"kind":"station"},"drones":[],
            "flight":{"position":[1000.,2000.,3000.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],
                "orientation":[std::f64::consts::FRAC_1_SQRT_2,0.,0.,std::f64::consts::FRAC_1_SQRT_2],"mode":NavigationMode::Stop,"updated_at_ms":1000,
                "docking":{"version":1,"host_vessel_id":"vessel-1","berth_id":0,"operation_id":0,"phase":"free","segment":null,"clamps":0.,"updated_at_ms":1000,"hold_reason":null,"departing":false}}
        })).unwrap();
        pilot.drone_bay = Some(DroneBayPublicState {
            schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
            batch: None,
            batch_capability: None,
            binding: DroneBayBinding {
                vessel_id: "vessel-1".into(),
                geometry_revision: geometry::DRONE_BAY_REGISTRATION_REVISION,
                source_sha256: geometry::DRONE_BAY_SOURCE_SHA256.into(),
            },
            cradles: geometry::home_poses()
                .into_iter()
                .enumerate()
                .map(|(i, body_pose)| DroneCradleView {
                    drone_id: 100 + i as u64,
                    class: DroneBayState::expected_class(i as u8).unwrap(),
                    hit_points: 37,
                    home_cradle: i as u8,
                    presence: DroneBayPresence::Stored,
                    body_pose,
                })
                .collect(),
            motions: vec![],
            machinery: DroneBayMachineryView {
                cleanup: None,
                handling_operation: None,
                door_fraction: 0.,
                door_segment: None,
                groups: geometry::mechanism_ids()
                    .into_iter()
                    .zip(geometry::reference_poses())
                    .map(|(id, pose)| DroneBayMechanismView {
                        semantic_id: id.into(),
                        confirmed_pose: pose,
                        segment: None,
                    })
                    .collect(),
            },
        });
        pilot
    }
    fn receiver_pilot() -> PilotView {
        use arkline_protocol::drone_dropoff_geometry as receiver;
        let mut pilot = stored_pilot();
        let bay = pilot.drone_bay.as_mut().unwrap();
        bay.schema_version = DRONE_BAY_SCHEMA_VERSION;
        bay.batch_capability = Some(DRONE_BAY_BATCH_VERSION);
        pilot.drone_cargo = Some(DroneCargoPublicState {
            schema_version: DRONE_CARGO_SCHEMA_VERSION,
            binding: supported_drone_cargo_registration().bind("vessel-1".into()),
            holds: bay
                .cradles
                .iter()
                .filter(|c| c.class == DroneClass::Mining)
                .map(|c| DroneOreHoldView {
                    drone_id: c.drone_id,
                    ore_units: 0,
                    capacity_units: MINING_DRONE_CARGO_CAPACITY,
                    cycle_reserved_units: 0,
                })
                .collect(),
            incoming_reserved_units: 0,
            operations: vec![],
            receiver_operation: None,
            cleanup: None,
            motion: None,
            groups: receiver::mechanism_ids()
                .into_iter()
                .zip(receiver::reference_poses())
                .map(|(id, confirmed_pose)| DroneBayMechanismView {
                    semantic_id: id.semantic_id().into(),
                    confirmed_pose,
                    segment: None,
                })
                .collect(),
        });
        assert!(validated_cargo(&pilot).is_some());
        pilot
    }
    #[test]
    fn stale_stored_bodies_remain_attached_to_translating_banked_display_parent() {
        let mut pilot = receiver_pilot();
        pilot.flight.as_mut().unwrap().velocity = [120., 15., -30.];
        pilot.flight.as_mut().unwrap().mode = NavigationMode::Manual;
        let space = Space {
            origin: [1000., 2000., 3000.],
        };
        let now = 4000;
        for roll in [0.0_f32, 0.7, -1.2] {
            pilot.flight.as_mut().unwrap().orientation =
                Quat::from_rotation_x(roll).to_array().map(f64::from);
            let bay = validated_bay(&pilot).unwrap();
            let hull = Mat4::from_rotation_translation(
                ship_orientation_at(&pilot, now),
                space.position(&pilot, now),
            );
            for (id, _, body) in stored_instances(&pilot, bay, space, now) {
                let home = bay
                    .cradles
                    .iter()
                    .find(|c| c.drone_id == id)
                    .unwrap()
                    .body_pose;
                let local = hull.inverse() * body;
                assert!(
                    local
                        .transform_point3(Vec3::ZERO)
                        .distance(Vec3::from_array(home.position_m.map(|v| (v / 100.) as f32)))
                        < 1e-5
                );
                assert!(
                    local.transform_vector3(Vec3::X).distance(Vec3::from_array(
                        home.transform_vector([1., 0., 0.])
                            .unwrap()
                            .map(|v| v as f32)
                    )) < 1e-5
                );
            }
        }
    }
    #[test]
    fn authoritative_motion_clock_freezes_stale_paused_disconnected_and_resumes_fresh() {
        let mut frame = super::super::tests::frame_for_scene_clone_test();
        frame.snapshot_time = 1000;
        frame.now = 1400;
        frame.connected = true;
        frame.paused = false;
        assert_eq!(frame.authoritative_motion_time(), 1400);
        frame.now = 2501;
        assert_eq!(frame.authoritative_motion_time(), 1000);
        frame.now = 1400;
        frame.paused = true;
        assert_eq!(frame.authoritative_motion_time(), 1000);
        frame.paused = false;
        frame.connected = false;
        assert_eq!(frame.authoritative_motion_time(), 1000);
        frame.connected = true;
        frame.snapshot_time = 1300;
        assert_eq!(frame.authoritative_motion_time(), 1400);
    }
    #[test]
    fn receiver_camera_requires_its_source_and_keeps_parent_attitude_at_large_origins() {
        let mut pilot = receiver_pilot();
        pilot.flight.as_mut().unwrap().position = [299_195_741_400., -150., 1000.];
        let mut frame = super::super::tests::frame_for_scene_clone_test();
        frame.now = 1000;
        frame.space.origin = pilot.flight.as_ref().unwrap().position;
        let before = pilot.clone();
        let camera = operations_camera_frame(
            &frame,
            &pilot,
            super::super::BayMonitorCamera::OreReceiver,
            1.5,
        )
        .unwrap();
        let parent = carrier_pose(&pilot, 1000);
        let expected = parent
            .compose(BayPose {
                position_m: [-38.0, 1.6, 9.5],
                orientation_xyzw: [0., 0., 0., 1.],
            })
            .unwrap();
        assert!(camera.eye.distance(frame.space.scene(expected.position_m)) < 1e-6);
        assert!(camera.vp.is_finite());
        assert_eq!(pilot, before);
        pilot
            .drone_cargo
            .as_mut()
            .unwrap()
            .binding
            .registration
            .receiver_source_sha256 = "0".repeat(64);
        assert!(operations_camera_frame(
            &frame,
            &pilot,
            super::super::BayMonitorCamera::OreReceiver,
            1.5
        )
        .is_none());
        // Unsupported receiver data never disables the separately validated old bay view.
        assert!(
            operations_camera_frame(&frame, &pilot, super::super::BayMonitorCamera::Wide, 1.5)
                .is_some()
        );
        pilot.drone_cargo = None;
        assert!(operations_camera_frame(
            &frame,
            &pilot,
            super::super::BayMonitorCamera::OreReceiver,
            1.5
        )
        .is_none());
    }
    #[test]
    fn operations_monitor_keeps_real_parent_frame_and_captain_controls_independent() {
        use super::super::{bay_operations_frame, BayMonitorCamera, BayMonitorOptions, FlightView};
        let pilot = stored_pilot();
        let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "server_time_ms":1000,"pilots":[pilot],"orders":[],"belt_ore":0,
            "ledger_sequence":0,"ledger_hash":"test-only",
            "world_layout":SceneLayoutRevision::DockingV1.definition()
        }))
        .unwrap();
        let before = serde_json::to_vec(&snapshot).unwrap();
        let wide = bay_operations_frame(
            &snapshot,
            "bay owner",
            1100,
            true,
            false,
            BayMonitorCamera::Wide,
            1.6,
        )
        .unwrap();
        let door = bay_operations_frame(
            &snapshot,
            "bay owner",
            1100,
            true,
            false,
            BayMonitorCamera::Doorway,
            1.6,
        )
        .unwrap();
        assert!(!wide.cockpit && wide.drone_camera && wide.camera_drone.is_none());
        assert!(wide.cockpit_occlusion.is_none() && wide.docking.is_none());
        assert!(wide.vp.is_finite() && door.vp.is_finite());
        assert_ne!(wide.eye, door.eye);
        let parent = carrier_pose(&snapshot.pilots[0], 1100);
        let expected = parent
            .compose(BayPose {
                position_m: [-18.5, -1., -6.95],
                orientation_xyzw: [0., 0., 0., 1.],
            })
            .unwrap();
        assert!(wide
            .eye
            .abs_diff_eq(wide.space.scene(expected.position_m), 1e-5));
        assert_eq!(wide.scene.pilots[0], snapshot.pilots[0]);
        for (connected, paused, now) in [
            (true, true, 1200),
            (false, false, 1200),
            (true, false, 3000),
        ] {
            let view = bay_operations_frame(
                &snapshot,
                "bay owner",
                now,
                connected,
                paused,
                BayMonitorCamera::Wide,
                1.6,
            )
            .unwrap();
            assert_eq!(view.now, snapshot.server_time_ms);
            assert!(!view.presentation_live());
        }
        assert!(bay_operations_frame(
            &snapshot,
            "foreign",
            1100,
            true,
            false,
            BayMonitorCamera::Wide,
            1.6
        )
        .is_err());
        let mut invalid = snapshot.clone();
        invalid.pilots[0]
            .drone_bay
            .as_mut()
            .unwrap()
            .binding
            .source_sha256 = "unsupported".into();
        assert!(bay_operations_frame(
            &invalid,
            "bay owner",
            1100,
            true,
            false,
            BayMonitorCamera::Wide,
            1.6
        )
        .is_err());
        let mut onfoot = snapshot.clone();
        onfoot.pilots[0].station_avatar = Some(
            serde_json::from_value(serde_json::json!({
                "position":[0,0,0], "heading":0, "moving":false, "updated_at_ms":1000
            }))
            .unwrap(),
        );
        assert!(bay_operations_frame(
            &onfoot,
            "bay owner",
            1100,
            true,
            false,
            BayMonitorCamera::Wide,
            1.6
        )
        .err()
        .unwrap()
        .contains("pilot seat"));
        let mut legacy = snapshot.clone();
        legacy.world_layout = Some(SceneLayoutRevision::OrbitalV1.definition());
        assert!(bay_operations_frame(
            &legacy,
            "bay owner",
            1100,
            true,
            false,
            BayMonitorCamera::Wide,
            1.6
        )
        .is_err());
        let mut view = FlightView::new(None);
        view.camera_source = crate::camera_sources::CameraSource::AllTurrets;
        view.drone_camera = false;
        view.selected_drone = Some(999);
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                view.bay_operations_camera_ui(
                    ui,
                    &snapshot,
                    "bay owner",
                    BayMonitorOptions {
                        now: 1100,
                        connected: true,
                        paused: false,
                        camera: BayMonitorCamera::Wide,
                    },
                )
            });
        });
        assert_eq!(
            view.camera_source,
            crate::camera_sources::CameraSource::AllTurrets
        );
        assert!(!view.drone_camera);
        assert_eq!(view.selected_drone, Some(999));
        assert!(view.audio_listener.is_none());
        assert_eq!(serde_json::to_vec(&snapshot).unwrap(), before);
    }

    #[test]
    fn stored_bodies_keep_identity_and_full_parent_attitude_without_mutation() {
        let pilot = stored_pilot();
        let before = pilot.clone();
        let bay = validated_bay(&pilot).unwrap();
        let space = Space {
            origin: [1000., 2000., 3000.],
        };
        let parts = stored_instances(&pilot, bay, space, 1000);
        assert_eq!(parts.len(), 15);
        for (index, (id, class, model)) in parts.iter().enumerate() {
            assert_eq!(*id, 100 + index as u64);
            assert_eq!(*class, bay.cradles[index].class);
            let expected = carrier_pose(&pilot, 1000)
                .compose(bay.cradles[index].body_pose)
                .unwrap();
            assert!(model
                .transform_point3(Vec3::ZERO)
                .abs_diff_eq(space.scene(expected.position_m), 1e-5));
            let expected_forward = expected.transform_vector([1., 0., 0.]).unwrap();
            assert!(model
                .transform_vector3(Vec3::X)
                .abs_diff_eq(Vec3::from_array(expected_forward.map(|v| v as f32)), 1e-5));
        }
        assert_eq!(pilot, before);
    }
    #[test]
    fn invalid_or_foreign_projection_never_draws_duplicate_stored_stock() {
        let mut pilot = stored_pilot();
        pilot.drone_bay.as_mut().unwrap().binding.vessel_id = "other".into();
        assert!(validated_bay(&pilot).is_none());
        let mut pilot = stored_pilot();
        pilot.drone_bay.as_mut().unwrap().cradles[1].drone_id = 100;
        assert!(validated_bay(&pilot).is_none());
        let mut pilot = stored_pilot();
        pilot.drone_bay.as_mut().unwrap().schema_version = 999;
        assert!(validated_bay(&pilot).is_none());
    }
    #[test]
    fn bay_cameras_are_source_bound_nonrecursive_and_distinct_when_paused() {
        use crate::camera_sources::CameraSource;
        let pilot = stored_pilot();
        let mut frame = super::super::tests::frame_for_scene_clone_test();
        frame.own = pilot.name.clone();
        frame.space = Space {
            origin: [1000., 2000., 3000.],
        };
        Arc::get_mut(&mut frame.scene)
            .unwrap()
            .pilots
            .push(pilot.clone());
        let original_eye = frame.eye;
        let mut keys = std::collections::BTreeSet::new();
        for source in [
            CameraSource::BayOverview,
            CameraSource::BayRack(0),
            CameraSource::BayRack(1),
            CameraSource::BayRack(2),
        ] {
            let view = camera_frame(&frame, &pilot, source, 16. / 9.).unwrap();
            assert!(!view.cockpit && view.drone_camera && view.camera_drone.is_none());
            assert!(Arc::ptr_eq(&view.scene, &frame.scene));
            assert!(view.vp.is_finite());
            assert!(view
                .rotation
                .abs_diff_eq(ship_orientation_at(&pilot, frame.now), 1e-6));
            assert!(keys.insert(bay_camera_source_key(
                &pilot.name,
                &pilot.drone_bay.as_ref().unwrap().binding,
                source
            )));
        }
        assert_eq!(frame.eye, original_eye);
        assert!(camera_frame(&frame, &pilot, CameraSource::BayRack(3), 16. / 9.).is_none());
        assert!(camera_frame(&frame, &pilot, CameraSource::BayOverview, f32::NAN).is_none());
    }

    #[test]
    fn constrained_mesh_and_pov_share_pose_and_release_preserves_bank() {
        use arkline_protocol::{DroneBayMotionView, DroneBayPhase, DroneMode, DroneView};
        let mut pilot = stored_pilot();
        let body = pilot.drone_bay.as_ref().unwrap().cradles[0].body_pose;
        let world = carrier_pose(&pilot, 1000).compose(body).unwrap();
        let drone = DroneView {
            id: 100,
            class: arkline_protocol::DroneClass::Recon,
            hit_points: 37,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: world.position_m,
            velocity: [0.; 3],
            heading: world.transform_vector([1., 0., 0.]).unwrap(),
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
            orientation_xyzw: Some(world.orientation_xyzw),
        };
        pilot.drones = vec![drone.clone()];
        let bay = pilot.drone_bay.as_mut().unwrap();
        bay.cradles[0].presence = DroneBayPresence::Active;
        bay.machinery.handling_operation = Some(1);
        bay.motions.push(DroneBayMotionView {
            drone_id: 100,
            operation_id: 1,
            phase: DroneBayPhase::TransferringOut,
            frame: DroneBayMotionFrame::Body,
            confirmed_pose: body,
            segment: None,
            updated_at_ms: 1000,
        });
        assert!(validated_bay(&pilot).is_some());
        let sampled = camera_sample(&pilot, &drone, 1000).unwrap();
        let space = Space {
            origin: [1000., 2000., 3000.],
        };
        let constrained = active_transform(&pilot, &drone, space, 1000).unwrap();
        let (eye, view) = drone_camera_matrix(&sampled, space, 1.5);
        assert!(eye.abs_diff_eq(constrained.transform_point3(Vec3::ZERO), 1e-5));
        let bay = pilot.drone_bay.as_mut().unwrap();
        bay.motions.clear();
        bay.machinery.handling_operation = None;
        let released = active_transform(&pilot, &drone, space, 1000).unwrap();
        assert!(released.abs_diff_eq(constrained, 1e-5));
        assert!(drone_camera_matrix(&drone, space, 1.5)
            .1
            .abs_diff_eq(view, 1e-5));
        let mut invalid = drone.clone();
        invalid.orientation_xyzw = Some([0.; 4]);
        assert!(active_transform(&pilot, &invalid, space, 1000).is_none());
    }

    #[test]
    fn explicit_stored_camera_identity_never_falls_back_or_revives_destroyed_stock() {
        let mut pilot = stored_pilot();
        assert_eq!(selected_stored(&pilot, Some(107)).unwrap().home_cradle, 7);
        assert!(selected_stored(&pilot, Some(999)).is_none());
        assert!(selected_stored(&pilot, None).is_none());
        let entry = &mut pilot.drone_bay.as_mut().unwrap().cradles[7];
        entry.presence = DroneBayPresence::Destroyed;
        entry.hit_points = 0;
        assert!(selected_stored(&pilot, Some(107)).is_none());
        assert_eq!(selected_stored(&pilot, Some(108)).unwrap().drone_id, 108);
        let peer:arkline_protocol::DroneView=serde_json::from_value(serde_json::json!({"id":108,"class":"mining","hit_points":37,"signal_strength":1.,"position":[0.,0.,0.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],"mode":{"kind":"hold"},"updated_at_ms":1000})).unwrap();
        pilot.drones = vec![peer];
        let entry = &mut pilot.drone_bay.as_mut().unwrap().cradles[8];
        entry.presence = DroneBayPresence::Active;
        for split in [false, true] {
            assert!(selected_drone_feed_ids(&pilot, Some(107), split).is_empty());
            assert!(selected_drone_feed_ids(&pilot, Some(106), split).is_empty());
            assert_eq!(selected_drone_feed_ids(&pilot, Some(108), split), vec![108]);
            assert_eq!(selected_drone_feed_ids(&pilot, None, split), vec![108]);
        }
        pilot.drone_bay = None;
        assert_eq!(
            selected_drone_feed_ids(&pilot, Some(107), false),
            vec![108],
            "legacy fallback remains unchanged"
        );
    }

    #[test]
    fn reference_mechanisms_keep_exact_confirmed_source_parts() {
        let pilot = stored_pilot();
        let source = exterior_body_parts(&pilot, 1000, false).unwrap();
        let baseline = docked_pioneer_parts().unwrap();
        assert_eq!(source.len(), baseline.len());
        for (actual, original) in source.iter().zip(baseline.iter()) {
            assert_eq!(actual.0, original.0);
            assert!(actual
                .1
                .abs_diff_eq(Mat4::from_scale(Vec3::splat(100.)) * original.1, 1e-5));
        }
    }
    fn handling_pilot() -> PilotView {
        let mut pilot = stored_pilot();
        let body = pilot.drone_bay.as_ref().unwrap().cradles[7].body_pose;
        let world = carrier_pose(&pilot, 1000).compose(body).unwrap();
        pilot.drones.push(DroneView {
            id: 107,
            class: DroneClass::Mining,
            hit_points: 37,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: world.position_m,
            velocity: [0.; 3],
            heading: world.transform_vector([1., 0., 0.]).unwrap(),
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
            orientation_xyzw: Some(world.orientation_xyzw),
        });
        let bay = pilot.drone_bay.as_mut().unwrap();
        bay.cradles[7].presence = DroneBayPresence::Active;
        bay.machinery.handling_operation = Some(1);
        bay.motions.push(DroneBayMotionView {
            drone_id: 107,
            operation_id: 1,
            phase: DroneBayPhase::ReleasingCradle,
            frame: DroneBayMotionFrame::Body,
            confirmed_pose: body,
            segment: None,
            updated_at_ms: 1000,
        });
        assert!(validated_bay(&pilot).is_some());
        pilot
    }

    #[test]
    fn bay_contact_feedback_follows_real_support_not_phase_timer_or_stock_creation() {
        let mut pilot = handling_pilot();
        let bay = pilot.drone_bay.as_mut().unwrap();
        assert_eq!(
            contact_indication(bay, 7, 1000),
            Some(ContactIndication::Supported)
        );
        bay.motions[0].confirmed_pose.orientation_xyzw = [0., 0., 0., 1.];
        assert_eq!(
            contact_indication(bay, 7, 1000),
            Some(ContactIndication::Released)
        );
        bay.motions[0].confirmed_pose.orientation_xyzw =
            geometry::home_pose(7).unwrap().orientation_xyzw;
        let tray = bay
            .machinery
            .groups
            .iter_mut()
            .find(|g| g.semantic_id == "tray/s07")
            .unwrap();
        tray.confirmed_pose.position_m[1] -= 0.20;
        assert_eq!(
            contact_indication(bay, 7, 1000),
            Some(ContactIndication::Released)
        );
        bay.motions[0].phase = DroneBayPhase::Held;
        assert_eq!(
            contact_indication(bay, 7, 1000),
            Some(ContactIndication::Held)
        );
        assert_eq!(
            contact_indication(bay, 7, 99999),
            Some(ContactIndication::Held)
        );
        bay.motions.clear();
        bay.machinery.handling_operation = None;
        assert_eq!(
            contact_indication(bay, 7, 1000),
            Some(ContactIndication::Empty)
        );
        bay.cradles[7].presence = DroneBayPresence::Destroyed;
        bay.cradles[7].hit_points = 0;
        assert_eq!(
            contact_indication(bay, 7, 1000),
            Some(ContactIndication::Destroyed)
        );
        assert_eq!(
            contact_indication(bay, 6, 1000),
            Some(ContactIndication::Stored)
        );
        assert_eq!(contact_indication(bay, 99, 1000), None);
    }

    #[test]
    fn bay_contact_materials_preserve_every_mesh_transform_and_inactive_source() {
        let pilot = handling_pilot();
        let before = pilot.clone();
        let baseline = exterior_body_parts(&pilot, 1000, false).unwrap();
        // Reference source input, not already transformed mechanisms.
        let source: Vec<_> = docked_pioneer_parts()
            .unwrap()
            .iter()
            .map(|&(mesh, model, color, emission)| {
                (
                    mesh,
                    Mat4::from_scale(Vec3::splat(100.)) * model,
                    color,
                    emission,
                )
            })
            .collect();
        let inactive = apply_presentation(&source, &pilot, 1000, false);
        assert_eq!(inactive, baseline);
        let live = apply_presentation(&source, &pilot, 1000, true);
        assert_eq!(live.len(), baseline.len());
        let mut altered = 0;
        let geometry = flight_geometry().unwrap();
        let asset = pioneer_asset().unwrap();
        for (actual, original) in live.iter().zip(&baseline) {
            assert_eq!(actual.0, original.0);
            assert_eq!(actual.1, original.1);
            if actual.2 != original.2 || actual.3 != original.3 {
                altered += 1;
                let mount = geometry
                    .bay_meshes
                    .iter()
                    .find(|m| m.mesh == actual.0)
                    .unwrap();
                assert!(mount.group_id.starts_with("tray/s"));
                assert_eq!(
                    asset[docked_source_primitive(actual.0).unwrap()].material_name,
                    "Safety amber"
                );
            }
        }
        assert_eq!(
            altered, 15,
            "exactly one retainer material batch per authored tray"
        );
        assert_eq!(pilot, before);
        let mut foreign = pilot.clone();
        foreign.drone_bay.as_mut().unwrap().binding.vessel_id = "other".into();
        assert_eq!(apply_presentation(&source, &foreign, 1000, true), source);
    }

    #[test]
    fn bay_fixture_lights_are_authored_bound_fresh_static_and_phase_linked() {
        let document: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/ships/pioneer/drone-bay-authoring.json"
        ))
        .unwrap();
        for (station, center) in BAY_FIXTURE_CENTERS_M.iter().enumerate() {
            let name = format!("pioneer.drone_bay/lighting/{station}/emitter");
            let part = document["parts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["semantic_id"] == name)
                .unwrap();
            assert_eq!(part["material"], "Working lights");
            for (axis, coordinate) in center.iter().enumerate() {
                let min = part["bounds_body_m"][0][axis].as_f64().unwrap();
                let max = part["bounds_body_m"][1][axis].as_f64().unwrap();
                assert!((coordinate - (min + max) * 0.5).abs() < 1e-9);
            }
        }
        let mut pilot = handling_pilot();
        let space = Space {
            origin: [1000., 2000., 3000.],
        };
        let before = pilot.clone();
        let lights = fixture_lights(&pilot, space, 1000, true);
        assert_eq!(lights.len(), 3);
        assert!(lights[1].power > lights[0].power);
        for (light, center) in lights.iter().zip(BAY_FIXTURE_CENTERS_M) {
            let expected = carrier_pose(&pilot, 1000)
                .compose(BayPose {
                    position_m: center,
                    orientation_xyzw: [0., 0., 0., 1.],
                })
                .unwrap();
            assert!(light
                .position
                .abs_diff_eq(space.scene(expected.position_m), 1e-6));
            assert!(light.power.is_finite() && light.power > 0. && light.power <= 2.2);
        }
        assert!(fixture_lights(&pilot, space, 1000, false).is_empty());
        assert_eq!(pilot, before);
        pilot.drone_bay.as_mut().unwrap().motions[0].phase = DroneBayPhase::Held;
        let held = fixture_lights(&pilot, space, 1000, true);
        assert!(held[1].color.x > held[1].color.z);
        assert!(held[1].power < lights[1].power);
        pilot.drone_bay.as_mut().unwrap().binding.source_sha256 = "unsupported".into();
        assert!(fixture_lights(&pilot, space, 1000, true).is_empty());
    }
}
