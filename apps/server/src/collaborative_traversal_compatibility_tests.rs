//! Collaborative viewers negotiate rendering without a pilot or recovery lease.
use super::*;
use arkline_protocol::PROTOCOL_VERSION;

fn directory() -> std::path::PathBuf {
    // Disposable fixtures remain available for inspection after a test run.
    std::env::temp_dir().join(format!(
        "arkline-collaborative-traversal-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ))
}

fn connect(name: &str) -> Request {
    Request::Connect {
        protocol_version: PROTOCOL_VERSION,
        world_layout_version: arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
        pilot: name.into(),
        resume_token: None,
        supports_room_controls: true,
        supports_fleet_hangar: true,
        supports_pioneer_interior: true,
        supports_docked_traversal_v2: true,
    }
}

#[test]
fn viewer_negotiation_defaults_false_and_never_acquires_a_game_session() {
    let mut authority = CollaborativeEditorHost::open(directory()).unwrap();
    let (host, _) = authority
        .join(CollaborationRole::Host, connect("TraversalHost"))
        .unwrap();
    let before = serde_json::to_value(authority.snapshot(host).unwrap().snapshot).unwrap();
    let (legacy, legacy_response) = authority
        .join_with_layout_capability(
            CollaborationRole::Viewer,
            Request::Snapshot,
            arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
        )
        .unwrap();
    let (capable, capable_response) = authority
        .join_with_capabilities(
            CollaborationRole::Viewer,
            Request::Snapshot,
            arkline_protocol::WORLD_LAYOUT_SCHEMA_VERSION,
            true,
        )
        .unwrap();
    assert!(!authority.sessions[&legacy].supports_docked_traversal_v2);
    assert!(authority.sessions[&capable].supports_docked_traversal_v2);
    assert!(authority.sessions[&legacy].server_session.is_none());
    assert!(authority.sessions[&capable].server_session.is_none());
    assert!(legacy_response.resume_token.is_none());
    assert!(capable_response.resume_token.is_none());
    assert_legacy_projection(legacy_response.snapshot.as_ref().unwrap());
    for viewer in [legacy, capable] {
        for _ in 0..2 {
            let snapshot = authority.snapshot(viewer).unwrap().snapshot;
            let response = authority.request(viewer, Request::Snapshot).unwrap();
            assert!(response.ok);
            if viewer == legacy {
                assert_legacy_projection(&snapshot);
                assert_legacy_projection(response.snapshot.as_ref().unwrap());
            }
        }
        assert!(authority.request(viewer, Request::ExportLedger).is_err());
        assert!(authority.request(viewer, Request::DisembarkShip).is_err());
        assert!(authority
            .request(viewer, connect("RejectedViewer"))
            .is_err());
    }
    let after = serde_json::to_value(authority.snapshot(host).unwrap().snapshot).unwrap();
    assert_eq!(
        after, before,
        "viewer polling must not recover or mutate another pilot"
    );
    authority.leave(legacy).unwrap();
    authority.leave(capable).unwrap();
    authority.leave(host).unwrap();
}

fn assert_legacy_projection(snapshot: &Snapshot) {
    assert!(snapshot.docked_traversal_capability.is_none());
    assert!(snapshot.pilots.iter().all(|pilot| pilot
        .station_avatar
        .as_ref()
        .is_none_or(|avatar| avatar.docked_traversal.is_none())));
}

fn populated_response_fixture() -> Response {
    use arkline_protocol::{
        DockedLiftState, DockedLiftStop, DockedSupport, DockedTraversalBinding,
        DockedTraversalCapability, DockedTraversalState, DockedVesselActuators,
        DOCKED_TRAVERSAL_VERSION,
    };
    let mut authority = CollaborativeEditorHost::open(directory()).unwrap();
    let (host, _) = authority
        .join(CollaborationRole::Host, connect("ProjectionHost"))
        .unwrap();
    authority
        .set_scene_state(host, EditorSceneState::StationHangar)
        .unwrap();
    let mut response = authority.request(host, Request::ExportLedger).unwrap();
    assert!(response.ok);
    let now = response.snapshot.as_ref().unwrap().server_time_ms;
    // Response-only metadata exercises the projection contract independently
    // of authoritative registration and movement; no raw World edits occur.
    let binding = DockedTraversalBinding {
        host_vessel_id: "response-fixture-owned-pioneer".into(),
        registration_revision: 1,
        layout_sha256: "a".repeat(64),
        fleet_layout: arkline_protocol::FleetLayoutRevision::CircularV1,
        fleet_hangar: true,
    };
    let state = DockedTraversalState {
        sprinting: false,
        version: DOCKED_TRAVERSAL_VERSION,
        binding: binding.clone(),
        feet_m: [123.25, 0.0, -64.5],
        heading: 0.25,
        support: DockedSupport::Station,
        surface_id: "response-fixture-station-floor".into(),
        moving: false,
        grounded: true,
        vertical_velocity_mps: 0.0,
        updated_at_ms: now,
        seat_transition: None,
        actuators: DockedVesselActuators {
            binding,
            inner_hatch_open: false,
            outer_hatch_open: false,
            lift: DockedLiftState {
                floor_station_m: 0.0,
                target: DockedLiftStop::Station,
                moving: false,
                updated_at_ms: now,
            },
            service_power_available: true,
        },
    };
    let capability = DockedTraversalCapability {
        version: DOCKED_TRAVERSAL_VERSION,
        registration_revision: 1,
        source_sha256: "b".repeat(64),
    };
    for snapshot in [
        response.snapshot.as_mut().unwrap(),
        &mut response.ledger.as_mut().unwrap().snapshot,
    ] {
        snapshot.docked_traversal_capability = Some(capability.clone());
        let avatar = snapshot.pilots[0].station_avatar.as_mut().unwrap();
        assert!(!avatar.onboard_interior && !avatar.moving);
        avatar.docked_traversal = Some(state.clone());
    }
    authority.leave(host).unwrap();
    response
}

fn without_traversal_metadata(mut response: serde_json::Value) -> serde_json::Value {
    erase_snapshot_traversal_metadata(response.get_mut("snapshot").unwrap());
    erase_snapshot_traversal_metadata(&mut response["ledger"]["snapshot"]);
    response
}

fn erase_snapshot_traversal_metadata(snapshot: &mut serde_json::Value) {
    snapshot
        .as_object_mut()
        .unwrap()
        .remove("docked_traversal_capability");
    for pilot in snapshot["pilots"].as_array_mut().unwrap() {
        if let Some(avatar) = pilot["station_avatar"].as_object_mut() {
            avatar.remove("docked_traversal");
        }
    }
}

#[test]
fn legacy_viewer_projection_strips_response_and_ledger_metadata_without_changing_source() {
    let source = populated_response_fixture();
    let source_json = serde_json::to_value(&source).unwrap();
    let mut legacy = source.clone();
    project_traversal_response(&mut legacy, CollaborationRole::Viewer, false);
    assert_legacy_projection(legacy.snapshot.as_ref().unwrap());
    assert_legacy_projection(&legacy.ledger.as_ref().unwrap().snapshot);
    assert!(source.snapshot.as_ref().unwrap().pilots[0]
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .is_some());
    assert!(source.ledger.as_ref().unwrap().snapshot.pilots[0]
        .station_avatar
        .as_ref()
        .unwrap()
        .docked_traversal
        .is_some());
    assert_eq!(serde_json::to_value(&source).unwrap(), source_json);
    assert_eq!(
        without_traversal_metadata(serde_json::to_value(&legacy).unwrap()),
        without_traversal_metadata(source_json),
        "legacy outer avatar, ledger, presence and all unrelated state remain exact",
    );
}

#[test]
fn capable_viewer_and_authenticated_roles_retain_complete_traversal_metadata() {
    let source = populated_response_fixture();
    let source_json = serde_json::to_value(&source).unwrap();
    for (role, capable) in [
        (CollaborationRole::Viewer, true),
        (CollaborationRole::Editor, false),
        (CollaborationRole::Host, false),
    ] {
        let mut response = source.clone();
        project_traversal_response(&mut response, role, capable);
        assert_eq!(serde_json::to_value(response).unwrap(), source_json);
    }
}
