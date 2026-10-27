//! Actual source-mounted device projection and enclosed-crew automation boundary.
use super::*;
use arkline_protocol::carrier_camera_devices::{self as cameras, CameraAvailability};

fn camera_snapshot(f: &mut Fixture) -> (PilotView, CarrierInventoryPublicState) {
    let response = f.ok(Request::Snapshot);
    if let Ok(directory) = std::env::var("ARKLINE_CARRIER_CAMERA_SAMPLES") {
        let directory = std::path::PathBuf::from(directory);
        assert!(directory.is_dir());
        let path = directory.join(format!("response-{}.json", f.now));
        if !path.exists() {
            std::fs::write(path, serde_json::to_vec(&response).unwrap()).unwrap();
        }
    }
    let pilot = response.snapshot.unwrap().pilots.remove(0);
    let public = pilot.carrier_inventory.clone().unwrap();
    assert_eq!(public.sample_time_ms, f.now);
    assert_eq!(
        public.validate_projection(&pilot),
        Ok(CarrierInventoryAdmission::StationaryFixture)
    );
    let devices = public.cameras.as_ref().unwrap();
    assert_eq!(devices.sampled_at_ms, f.now);
    assert_eq!(devices.devices.len(), 10);
    let roots: BTreeMap<_, _> = public.machinery.iter().cloned().collect();
    for device in &devices.devices {
        assert_eq!(device.availability, CameraAvailability::Available);
        assert_eq!(
            device.parent_world_pose,
            public.parent.compose(roots[&device.parent_id])
        );
    }
    (pilot, public)
}

#[test]
fn actual_1703_camera_source_clock_supply_and_current_roots_are_cross_bound() {
    let mut f = parallel_tests::fixture();
    let (pilot, public) = camera_snapshot(&mut f);
    for mutation in 0..7 {
        let mut bad = public.clone();
        let camera = bad.cameras.as_mut().unwrap();
        match mutation {
            0 => camera.sampled_at_ms += 1,
            1 => camera.vessel_id.push_str("-other"),
            2 => camera.source_sha256 = "0".repeat(64),
            3 => camera.definition_sha256 = "0".repeat(64),
            4 => {
                camera.devices[0]
                    .parent_world_pose
                    .as_mut()
                    .unwrap()
                    .position_m[0] += 0.01
            }
            5 => camera.devices[0].availability = CameraAvailability::Unpowered,
            6 => {
                camera.devices.pop();
            }
            _ => unreachable!(),
        }
        assert!(
            bad.validate_projection(&pilot).is_err(),
            "Mutation {mutation}"
        );
    }
    for missing in [false, true] {
        let mut without_supply = pilot.clone();
        if missing {
            without_supply.power = None;
        } else {
            without_supply.power.as_mut().unwrap().charge_j = 0;
        }
        assert!(public.validate_projection(&without_supply).is_err());
        let mut corrected = public.clone();
        let reg = &corrected.binding().registration;
        corrected.cameras = Some(
            cameras::project(
                cameras::for_exact_source(reg.assembly_revision, &reg.assembly_source_sha256)
                    .unwrap(),
                &corrected.binding().vessel_id,
                corrected.parent,
                &corrected.machinery.iter().cloned().collect(),
                without_supply.power.as_ref(),
                corrected.sample_time_ms,
            )
            .unwrap(),
        );
        assert!(corrected
            .cameras
            .as_ref()
            .unwrap()
            .devices
            .iter()
            .all(|d| d.availability
                == if missing {
                    CameraAvailability::MissingPower
                } else {
                    CameraAvailability::Unpowered
                }));
        assert!(corrected.validate_projection(&without_supply).is_ok());
    }
    let mut old = public;
    old.cameras = None;
    let value = serde_json::to_value(&old).unwrap();
    assert!(value.get("cameras").is_none());
    assert_eq!(
        serde_json::from_value::<CarrierInventoryPublicState>(value).unwrap(),
        old
    );
    assert!(
        old.validate_projection(&pilot).is_ok(),
        "Historical camera-absent response remains inspectable"
    );
}

#[test]
fn actual_1703_all_twenty_deploy_while_on_foot_and_camera_tracks_current_crane() {
    let mut f = parallel_tests::fixture();
    let (_, initial) = camera_snapshot(&mut f);
    let initial_cameras = initial.cameras.unwrap();
    f.request(CarrierInventoryRequest::DeployAll {
        binding: f.binding(),
        batch_id: 1,
    });
    let w = f.world();
    let account = &w.accounts["Pilot"];
    let binding = docked_traversal_binding(
        account.vessel_id(ShipClass::Pioneer).unwrap().into(),
        &default_pioneer_room_layout(),
        w.scene_layout.definition().fleet_layout,
        account.fleet_hangar,
    );
    f.ok(Request::DockedTraversal {
        request: DockedTraversalRequest::LeaveSeat {
            binding: binding.clone(),
        },
    });
    for _ in 0..3 {
        f.coarse_step();
    }
    let standing = f.world().pilots["Pilot"]
        .view
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap()
        .clone();
    assert!(standing.seat_transition.is_none());
    for _ in 0..8 {
        f.ok(Request::DockedTraversal {
            request: DockedTraversalRequest::Walk {
                binding: binding.clone(),
                forward: 1.,
                strafe: 0.,
                turn: 0.,
                sprint: false,
            },
        });
        f.step();
    }
    let w = f.world();
    let walked = w.pilots["Pilot"]
        .view
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .as_ref()
        .unwrap();
    assert_ne!(standing.feet_m, walked.feet_m);
    assert!(matches!(
        walked.support,
        DockedSupport::Cockpit
            | DockedSupport::Connector
            | DockedSupport::CoreStairs
            | DockedSupport::Deck1
            | DockedSupport::Deck2
    ));
    for request in [
        DockedTraversalRequest::SetHatch {
            binding: binding.clone(),
            hatch: DockedHatch::Outer,
            open: true,
        },
        DockedTraversalRequest::CallLift {
            binding: binding.clone(),
            stop: DockedLiftStop::Station,
        },
    ] {
        let refused =
            f.server
                .handle_at(&mut f.session, Request::DockedTraversal { request }, f.now);
        assert!(!refused.ok && refused.message.contains("locked"));
    }
    let mut moved_camera = false;
    let mut completed = false;
    for step in 0..450 {
        f.coarse_step();
        let w = f.world();
        let s = state(&w, "Pilot").unwrap();
        assert!(
            w.pilots["Pilot"].view.station_avatar.is_some(),
            "No automatic reseat"
        );
        assert!(!s.inventory.deployments[0].held);
        assert!(s.runs.values().all(|r| !r.held));
        if step % 5 == 0 {
            let (_, current) = camera_snapshot(&mut f);
            let camera = current.cameras.unwrap();
            moved_camera |= camera
                .devices
                .iter()
                .zip(&initial_cameras.devices)
                .any(|(a, b)| a.parent_world_pose != b.parent_world_pose);
        }
        if s.inventory.deployments[0].phase == CarrierDeploymentPhase::Completed
            && s.runs.is_empty()
            && s.departures.is_empty()
        {
            completed = true;
            break;
        }
    }
    assert!(completed && moved_camera);
    let (pilot, public) = camera_snapshot(&mut f);
    assert!(pilot.station_avatar.is_some());
    assert_eq!(pilot.drones.len(), 20);
    assert!(public
        .inventory
        .entries
        .iter()
        .all(|e| e.location == CarrierDroneLocation::Deployed && e.hit_points == 100));
    assert!(public.inventory.reservations.is_empty() && public.inventory.cleanups.is_empty());
    assert_eq!(pilot.ore, 0);
    assert_eq!(pilot.credits, STARTER_CREDITS);
}
