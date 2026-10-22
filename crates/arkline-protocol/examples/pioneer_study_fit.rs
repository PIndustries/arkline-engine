//! Read-only fit report. No world, rendering, registration or saved-pose writes.
use arkline_protocol::{pioneer_docked::*, *};
use serde_json::{json, Value};
use std::{error::Error, path::PathBuf};

fn vector(value: &Value) -> Result<[f64; 3], serde_json::Error> {
    serde_json::from_value(value.clone())
}
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets/studies/pioneer-forward-bay-proof/manifest.json")
        });
    let study: Value = serde_json::from_slice(&std::fs::read(path)?)?;
    let parts = study["components"]
        .as_array()
        .ok_or("Missing study components")?;
    let deck = parts
        .iter()
        .find(|p| p["id"] == "enclosure.deck")
        .ok_or("Missing study deck")?;
    let deck_min = vector(&deck["bounds_min"])?;
    let deck_max = vector(&deck["bounds_max"])?;
    let eye = pilot_joints(0.).eye_m;
    let study_eye = vector(&study["proof_eye"])?;
    let translation: [f64; 3] = std::array::from_fn(|i| eye[i] - study_eye[i]);
    let canonical_floor = deck_floor_y(2).ok_or("Missing canonical floor")?;
    let floor_error = deck_max[1] + translation[1] - canonical_floor;
    let geometry = registered_geometry(&default_pioneer())?;
    let rear = geometry
        .portals
        .iter()
        .find(|p| p.id == "cockpit/backwall-portal")
        .ok_or("Missing canonical rear portal")?;
    let rear_local: [f64; 3] = std::array::from_fn(|i| rear.center_m[i] - translation[i]);
    let rear_z = [
        rear_local[2] - rear.half_m[0],
        rear_local[2] + rear.half_m[0],
    ];
    let portal_fits = rear_z[0] >= deck_min[2] && rear_z[1] <= deck_max[2];
    let mounts: Vec<_> = (1..6)
        .filter_map(|slot| {
            cockpit_screen(slot).map(|(center, half)| {
                json!({
                    "slot":slot,"center_m":center,"half_m":half,"axes":cockpit_screen_axes(slot)
                })
            })
        })
        .collect();
    let study_center = vector(&study["instrument"]["center"])?;
    let mapped_center: [f64; 3] = std::array::from_fn(|i| study_center[i] + translation[i]);
    let (main_center, _) = cockpit_screen(3).ok_or("Missing canonical MFD")?;
    let center_delta: [f64; 3] = std::array::from_fn(|i| mapped_center[i] - main_center[i]);
    let pose = PioneerDockedPose::private();
    let anchors = seat_anchors(pose);
    let chair: Vec<_> = geometry
        .panels
        .iter()
        .filter(|p| p.id.starts_with("pilot-chair/"))
        .map(|p| {
            json!({"id":p.id,"center_m":p.bounds.center_m,
            "half_m":p.bounds.half_m,"axes":p.bounds.axes})
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "scope":"Read-only measurements, not registration or adoption approval",
            "canonical_registration_revision":PIONEER_DOCKED_REGISTRATION_REVISION,
            "canonical_source_sha256":PIONEER_DOCKED_SOURCE_SHA256,
            "study_sha256":study["sha256"],
            "eye_alignment_translation_m":translation,
            "study_floor_under_eye_alignment_m":deck_max[1]+translation[1],
            "canonical_floor_m":canonical_floor,"floor_error_m":floor_error,
            "floor_and_eye_translation_compatible":floor_error.abs()<1e-6,
            "canonical_seat_root_m":pilot_joints(0.).root_m,
            "canonical_seat_egress_m":station_to_body(pose,anchors.egress_station_m),
            "canonical_seated_eye_m":eye,"canonical_standing_eye_m":pilot_joints(1.).eye_m,
            "rear_portal_center_in_study_m":rear_local,"rear_portal_z_interval_in_study_m":rear_z,
            "study_deck_z_interval_m":[deck_min[2],deck_max[2]],
            "rear_portal_within_study_width":portal_fits,
            "mapped_study_instrument_center_m":mapped_center,
            "study_minus_canonical_mfd_center_m":center_delta,
            "canonical_instrument_mounts":mounts,"study_instrument_axes":study["instrument"]["axes"],
            "canonical_static_chair_parts":chair,
            "canonical_adoption_validated":false,
            "decision":"Keep canonical seat, floor, input frames and port-offset access; rebuild the study around them before integration"
        }))?
    );
    Ok(())
}
