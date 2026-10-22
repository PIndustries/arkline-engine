//! Isolated candidate only: export registered references and proposed enclosure
//! members; never write a world, registration or runtime geometry recipe.
use arkline_protocol::{pioneer_docked::*, *};
use serde_json::json;

const AXES: [[f64; 3]; 3] = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];

fn member(id: &str, lo: [f64; 3], hi: [f64; 3]) -> PioneerDockedPanel {
    PioneerDockedPanel {
        id: format!("candidate/{id}"),
        bounds: PioneerDockedBox {
            center_m: std::array::from_fn(|i| (lo[i] + hi[i]) * 0.5),
            half_m: std::array::from_fn(|i| (hi[i] - lo[i]) * 0.5),
            axes: AXES,
        },
        material: PioneerDockedMaterial::FittedBulkhead,
        collision: PioneerDockedCollision::Solid,
        cell: PioneerDockedCell::Cockpit,
        kind: PioneerDockedPanelKind::Structure,
    }
}

fn candidate() -> Vec<PioneerDockedPanel> {
    let g = registered_geometry(&default_pioneer()).expect("Valid canonical document");
    let bounds = |id: &str| g.panels.iter().find(|p| p.id == id).unwrap().bounds;
    // Nanometre rounding only removes arithmetic cancellation at box joins.
    let q = |v: f64| (v * 1e9).round() / 1e9;
    let rear = g
        .portals
        .iter()
        .find(|p| p.id == "cockpit/backwall-portal")
        .unwrap();
    let floor = deck_floor_y(2).unwrap();
    let seat_floor = bounds("cockpit/seat-floor");
    let walkway = bounds("cockpit/port-walkway");
    let header = bounds("cockpit/port-exit-header");
    let x0 = rear.center_m[0];
    let x1 = q(seat_floor.center_m[0] + seat_floor.half_m[0]);
    let exit0 = q(walkway.center_m[0] - walkway.half_m[0]);
    let exit1 = q(walkway.center_m[0] + walkway.half_m[0]);
    let header_y = q(header.center_m[1] - header.half_m[1]);
    // Authored clearances around unchanged anchors, not new gameplay anchors.
    let roof = q(header_y + 0.32);
    let port = q(rear.center_m[2] - rear.half_m[0] - 0.10);
    let starboard = q(pilot_joints(0.).root_m[2] + 1.8);
    let outside_port = q(port - 0.08);
    let outside_starboard = q(starboard + 0.08);
    vec![
        member(
            "overhead",
            [x0, roof, outside_port],
            [x1, q(roof + 0.10), outside_starboard],
        ),
        member(
            "starboard-wall",
            [x0, floor, starboard],
            [x1, roof, outside_starboard],
        ),
        member(
            "port-aft-wall",
            [x0, floor, outside_port],
            [exit0, roof, port],
        ),
        member(
            "port-forward-wall",
            [exit1, floor, outside_port],
            [x1, roof, port],
        ),
        member(
            "port-header",
            [exit0, header_y, outside_port],
            [exit1, roof, port],
        ),
    ]
}

fn state(pose: PioneerDockedPose, fold: f64) -> PioneerDockedActuators {
    PioneerDockedActuators {
        hatch_fractions: None,
        room_door_open: true,
        inner_hatch_open: true,
        outer_hatch_open: false,
        lift_floor_station_m: lift_geometry(pose).ship_floor_m,
        chair_fold: fold,
    }
}

fn validate(members: &[PioneerDockedPanel]) -> Result<usize, String> {
    let original = registered_geometry(&default_pioneer())?;
    let mut fitted = original.clone();
    fitted.panels.extend_from_slice(members);
    assert_eq!(fitted.supports, original.supports);
    assert_eq!(fitted.portals, original.portals);
    assert_eq!(&fitted.panels[..original.panels.len()], &original.panels);
    let mut samples = 0;
    // Candidate-only segment occlusion: do not claim that inherited canonical
    // instruments are readable merely because an overlay leaves their rays clear.
    for eye in [pilot_joints(0.).eye_m, pilot_joints(1.).eye_m] {
        for slot in 1..=5 {
            let (center, half) = cockpit_screen(slot).unwrap();
            let axes = cockpit_screen_axes(slot).unwrap();
            for (u, v) in [(-1., -1.), (-1., 1.), (1., -1.), (1., 1.), (0., 0.)] {
                let face: [f64; 3] = std::array::from_fn(|k| {
                    center[k] - 0.032 * axes[0][k]
                        + u * (half[1] - 0.008) * axes[1][k]
                        + v * (half[2] - 0.008) * axes[2][k]
                });
                for p in members {
                    let mut interval = [0_f64, 1_f64];
                    for k in 0..3 {
                        let delta = face[k] - eye[k];
                        let lo = p.bounds.center_m[k] - p.bounds.half_m[k];
                        let hi = p.bounds.center_m[k] + p.bounds.half_m[k];
                        if delta.abs() < 1e-12 {
                            if eye[k] < lo || eye[k] > hi {
                                interval = [1., 0.];
                                break;
                            }
                        } else {
                            let a = (lo - eye[k]) / delta;
                            let b = (hi - eye[k]) / delta;
                            interval[0] = interval[0].max(a.min(b));
                            interval[1] = interval[1].min(a.max(b));
                        }
                    }
                    if interval[0] <= interval[1] {
                        return Err(format!("Display {slot} ray blocked by {}", p.id));
                    }
                }
            }
        }
    }
    for pose in [
        PioneerDockedPose::private(),
        PioneerDockedPose::fleet(FleetLayoutRevision::LinearV1),
        PioneerDockedPose::fleet(FleetLayoutRevision::CircularV1),
    ] {
        let dynamic = dynamic_geometry(pose, state(pose, 1.))?;
        let route: [[f64; 2]; 12] = [
            [33.54, 3.],
            [33.36, 3.],
            [33.36, 0.],
            [32.10, 0.],
            [31.2, 0.],
            [32.10, 0.],
            [32.10, 4.2],
            [33.36, 4.2],
            [33.36, 3.],
            [33.36, 0.],
            [34.1, -0.5],
            [34.1, -7.8],
        ];
        for leg in route.windows(2) {
            let count =
                ((leg[1][0] - leg[0][0]).hypot(leg[1][1] - leg[0][1]) / 0.025_f64).ceil() as usize;
            for i in 0..=count {
                let t = i as f64 / count.max(1) as f64;
                let body = [
                    leg[0][0] + t * (leg[1][0] - leg[0][0]),
                    deck_floor_y(2).unwrap(),
                    leg[0][1] + t * (leg[1][1] - leg[0][1]),
                ];
                let station = body_to_station(pose, body);
                if !capsule_clear_station(&fitted, &dynamic, pose, station) {
                    return Err(format!("Blocked circulation at {body:?}"));
                }
                let support = support_at_station(&fitted, &dynamic, pose, station, None, 0.01)
                    .ok_or_else(|| format!("No support at {body:?}"))?;
                if (support.height_m - station[1]).abs() > 1e-8 {
                    return Err(format!("Deck offset at {body:?}"));
                }
                samples += 1;
            }
        }
        // All corners of the existing actuated pan, at 1,001 fold fractions.
        for i in 0..=1000 {
            let d = dynamic_geometry(pose, state(pose, i as f64 / 1000.))?;
            let pan = d.panels.iter().find(|p| p.id == "pilot-chair/pan").unwrap();
            for member in members {
                // Candidate members are axis aligned. Conservative OBB AABB
                // separation proves no intersection, rather than sampling corners.
                let radius: [f64; 3] = std::array::from_fn(|k| {
                    (0..3)
                        .map(|a| pan.bounds.axes[a][k].abs() * pan.bounds.half_m[a])
                        .sum()
                });
                if (0..3).all(|k| {
                    (pan.bounds.center_m[k] - member.bounds.center_m[k]).abs()
                        < radius[k] + member.bounds.half_m[k]
                }) {
                    return Err(format!("Chair sweep intersects {} at fold {i}", member.id));
                }
            }
        }
    }
    Ok(samples)
}

fn report() -> Result<serde_json::Value, String> {
    let members = candidate();
    let samples = validate(&members)?;
    let g = registered_geometry(&default_pioneer())?;
    let dynamic = dynamic_geometry(
        PioneerDockedPose::private(),
        state(PioneerDockedPose::private(), 0.),
    )?;
    let panels: Vec<_> = g.panels.iter().chain(&dynamic.panels)
        .filter(|p| p.cell == PioneerDockedCell::Cockpit)
        .chain(&members)
        .map(|p| json!({"id":p.id,"center":p.bounds.center_m,"half":p.bounds.half_m,"axes":p.bounds.axes,"material":format!("{:?}", p.material),"collision":format!("{:?}",p.collision)})).collect();
    let mounts: Vec<_> = (1..=5).map(|slot| {let (center,half)=cockpit_screen(slot).unwrap();json!({"slot":slot,"center":center,"half":half,"axes":cockpit_screen_axes(slot).unwrap()})}).collect();
    Ok(json!({
        "study_only":true,"canonical_registration_revision":PIONEER_DOCKED_REGISTRATION_REVISION,
        "canonical_source_sha256":PIONEER_DOCKED_SOURCE_SHA256,
        "panels":panels,"instruments":mounts,"proof_eye":pilot_joints(0.).eye_m,
        "standing_eye":pilot_joints(1.).eye_m,"deck_y":deck_floor_y(2),
        "circulation_supported_capsule_samples":samples,"chair_fold_samples_per_placement":1001,
        "placements_tested":3,"original_panels_supports_portals_unchanged":true,
        "candidate_clear_instrument_rays":50,
        "limits":"Isolated proposed overlay; not adopted registration, server geometry or completed visual fidelity"
    }))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", serde_json::to_string_pretty(&report()?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_adoption_conflicts_with_existing_legal_saved_positions() {
        let original = registered_geometry(&default_pioneer()).unwrap();
        let mut proposed = original.clone();
        proposed.panels.extend(candidate());
        let body = [32.1, deck_floor_y(2).unwrap(), 4.84];
        for pose in [
            PioneerDockedPose::private(),
            PioneerDockedPose::fleet(FleetLayoutRevision::LinearV1),
            PioneerDockedPose::fleet(FleetLayoutRevision::CircularV1),
        ] {
            let dynamic = dynamic_geometry(pose, state(pose, 1.)).unwrap();
            let feet = body_to_station(pose, body);
            assert!(capsule_clear_station(&original, &dynamic, pose, feet));
            assert!(support_at_station(&original, &dynamic, pose, feet, None, 0.01).is_some());
            assert!(!capsule_clear_station(&proposed, &dynamic, pose, feet),
                "A route proof must not be mistaken for compatibility of every existing saved position");
        }
    }
    #[test]
    fn fitted_enclosure_preserves_registered_access_support_and_chair_sweep() {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../assets/studies/pioneer-canonical-enclosure-proof/manifest.json"
        ))
        .unwrap();
        // Fail when either registered anchors or the candidate changed without
        // regenerating the exact mesh source committed in the manifest.
        assert_eq!(report().unwrap(), manifest["canonical_fit"]);
    }
    #[test]
    fn closing_real_access_or_intersecting_chair_is_rejected() {
        let mut closed = candidate();
        closed.push(member(
            "blocked-port-exit",
            [33.7, -0.32, -0.88],
            [34.5, 2.08, -0.80],
        ));
        assert!(validate(&closed)
            .unwrap_err()
            .contains("Blocked circulation"));
        let mut chair = candidate();
        chair.push(member(
            "blocked-pan",
            [32.94, 0.59, 2.9],
            [32.98, 0.64, 3.1],
        ));
        assert!(validate(&chair).unwrap_err().contains("Chair sweep"));
    }
}
