//! Actual CPU render geometry in metres. These checks do not claim native/GPU acceptance.
use super::*;

fn circular_fleet(classes: &[ShipClass]) -> FleetView {
    let revision = FleetLayoutRevision::CircularV1;
    FleetView {
        layout_revision: revision,
        layout: arkline_protocol::fleet_hangar_layout_for(revision),
        berths: classes
            .iter()
            .map(|class| {
                let pose = fleet_berth_pose(revision, *class);
                HangarBerth {
                    id: format!("measured-{class:?}"),
                    ship_class: *class,
                    position: pose.position,
                    yaw: pose.yaw,
                    boarding_point: pose.boarding_point,
                    active: *class == ShipClass::Pioneer,
                    can_board: true,
                }
            })
            .collect(),
    }
}

fn vertices(parts: &[ModelPart]) -> impl Iterator<Item = Vec3> + '_ {
    parts.iter().flat_map(|(mesh, model, _, _)| {
        mesh_vertices(*mesh)
            .into_iter()
            .map(move |vertex| model.transform_point3(vertex.p))
    })
}

#[test]
fn circular_floor_is_flat_finite_and_covers_the_authoritative_avatar_inset() {
    let mesh = floor_vertices();
    assert_eq!(mesh.len(), FLOOR_SEGMENTS * 12);
    for triangle in mesh.chunks_exact(3) {
        for vertex in triangle {
            assert!(vertex.p.is_finite() && vertex.n.is_finite());
            assert!(vertex.p.y >= -1. && vertex.p.y <= 0.);
            assert!((vertex.n.length() - 1.).abs() < 0.0001);
        }
        assert!(
            (triangle[1].p - triangle[0].p)
                .cross(triangle[2].p - triangle[0].p)
                .length_squared()
                > 1e-10
        );
    }
    for class in ALL_SHIP_CLASSES {
        let pose = fleet_berth_pose(FleetLayoutRevision::CircularV1, class);
        let chord_inset =
            pose.platform_radius_m * (1. - (std::f64::consts::PI / FLOOR_SEGMENTS as f64).cos());
        assert!(
            chord_inset < arkline_protocol::STATION_AVATAR_RADIUS_M,
            "{class:?}: rendered polygon must support every permitted avatar centre"
        );
    }
    assert!(
        embedded_primitive(floor_mesh_index()).is_none(),
        "floor must not become a hull primitive"
    );
    assert_eq!(mesh_vertices(floor_mesh_index()).len(), mesh.len());
}

#[test]
fn circular_instances_preserve_all_intrinsic_sizes_and_the_explicit_hover_gap() {
    let fleet = circular_fleet(&ALL_SHIP_CLASSES);
    let geometry = fleet_layout_geometry(fleet.layout_revision);
    for berth in &fleet.berths {
        let body: Vec<_> = hull_parts(berth).collect();
        let (lo, hi) = parts_bounds(&body);
        let extra_bottom_clearance = if berth.ship_class == ShipClass::Pioneer {
            2.40
        } else {
            0.
        };
        assert!(
            (lo.y - geometry.hover_gap_m as f32 - extra_bottom_clearance).abs() < 0.025,
            "{berth:?}: {lo:?}"
        );
        let undo_yaw =
            Mat4::from_rotation_translation(Quat::from_rotation_y(berth.yaw as f32), Vec3::ZERO);
        let local: Vec<_> = vertices(&body)
            .map(|vertex| undo_yaw.transform_point3(vertex - point(berth.position)))
            .collect();
        let min = local
            .iter()
            .copied()
            .fold(Vec3::splat(f32::INFINITY), Vec3::min);
        let max = local
            .iter()
            .copied()
            .fold(Vec3::splat(f32::NEG_INFINITY), Vec3::max);
        let spec = berth.ship_class.spec();
        let dimensions = Vec3::new(
            spec.length_m as f32,
            if berth.ship_class == ShipClass::Pioneer {
                20.35
            } else {
                spec.height_m as f32
            },
            spec.width_m as f32,
        );
        assert!(
            ((max - min) - dimensions).abs().max_element() < 0.03,
            "{:?}",
            berth.ship_class
        );
        assert!(hi.y < geometry.ceiling_y as f32 - 25.);
    }
    let pioneer = hull_parts(&fleet.berths[0]).collect::<Vec<_>>();
    let (lo, hi) = parts_bounds(&pioneer);
    assert!(((hi - lo) - Vec3::new(72., 20.35, 38.)).abs().max_element() < 0.002);
    assert!((lo.y - 6.40).abs() < 0.002);
}

#[test]
fn complete_fleet_hulls_and_articulated_turrets_clear_the_measured_platforms_and_roof() {
    let fleet = circular_fleet(&ALL_SHIP_CLASSES);
    let geometry = fleet_layout_geometry(fleet.layout_revision);
    let center = point(geometry.center);
    for berth in &fleet.berths {
        let pose = fleet_berth_pose(fleet.layout_revision, berth.ship_class);
        let mut complete = hull_parts(berth).collect::<Vec<_>>();
        for yaw in 0..16 {
            for pitch in [
                -std::f32::consts::FRAC_PI_2,
                -0.7,
                0.,
                0.7,
                std::f32::consts::FRAC_PI_2,
            ] {
                let aim = Quat::from_rotation_y(yaw as f32 * std::f32::consts::TAU / 16.)
                    * Quat::from_rotation_z(pitch);
                for slot in 0..2 {
                    complete.extend(turret_parts(berth.ship_class, aim, slot).into_iter().map(
                        |(mesh, model, color, emission)| {
                            (mesh, hull_transform(berth) * model, color, emission)
                        },
                    ));
                }
            }
        }
        for vertex in vertices(&complete) {
            let radial = (vertex - point(pose.position)) * Vec3::new(1., 0., 1.);
            assert!(
                radial.length() <= pose.platform_radius_m as f32 - 0.2,
                "{:?}: platform {vertex:?}",
                berth.ship_class
            );
            let wall = (vertex - center) * Vec3::new(1., 0., 1.);
            assert!(
                wall.length() < geometry.outer_radius_m as f32 - 3.,
                "{:?}: wall",
                berth.ship_class
            );
            assert!(
                vertex.y >= 0. && vertex.y < geometry.ceiling_y as f32 - 0.2,
                "{:?}: articulated clearance at {vertex:?}",
                berth.ship_class
            );
        }
    }
}

#[test]
fn only_owned_vessels_are_drawn_and_reserving_slots_does_not_move_the_station() {
    let empty = circular_fleet(&[]);
    let station = parts(&empty, Vec3::ZERO);
    assert!(!station.is_empty() && station.len() < 600);
    assert!(station
        .iter()
        .all(|(mesh, _, _, _)| embedded_primitive(*mesh).is_none()));
    let one = circular_fleet(&[ShipClass::Pioneer]);
    let body = hull_parts(&one.berths[0]).collect::<Vec<_>>();
    let populated = parts(&one, Vec3::ZERO);
    assert!(populated[..station.len()]
        .iter()
        .zip(&station)
        .all(|(a, b)| a == b));
    assert_eq!(
        populated
            .iter()
            .filter(|(mesh, _, _, _)| embedded_primitive(*mesh).is_some())
            .count(),
        body.len()
    );
    let mut switched = one.clone();
    switched.berths[0].active = false;
    let switched = parts(&switched, Vec3::ZERO);
    assert!(populated
        .iter()
        .zip(switched)
        .all(|(a, b)| a.0 == b.0 && a.1 == b.1));
}

#[test]
fn explicit_light_sources_are_bounded_and_have_visible_work_light_fixtures() {
    let fleet = circular_fleet(&ALL_SHIP_CLASSES);
    let lights = fixture_lights(&fleet).unwrap();
    assert_eq!(lights.len(), 14);
    let station = circular_station_parts();
    for light in lights {
        assert!(
            light.position.is_finite()
                && light.range > 0.
                && light.range <= 40.
                && light.power <= 4.
        );
        assert!(station
            .iter()
            .any(|(mesh, model, color, emission)| *mesh == 0
                && model.w_axis.truncate() == light.position
                && *color == light.color
                && *emission >= 0.7));
    }
    let mut legacy = fleet;
    legacy.layout_revision = FleetLayoutRevision::LinearV1;
    assert!(fixture_lights(&legacy).is_none());
}

#[test]
fn circular_camera_uses_each_rotated_hull_pose_and_never_forces_a_minimum_penetration() {
    for class in ALL_SHIP_CLASSES {
        let fleet = circular_fleet(&[class]);
        let berth = &fleet.berths[0];
        let t = hull_transform(berth);
        let (min, max) = camera_hull_bounds(class);
        let target = t.transform_point3(Vec3::new(0., 0., max.z + 0.002));
        let desired = t.transform_point3(Vec3::ZERO);
        let resolved = t
            .inverse()
            .transform_point3(camera(&fleet, target, desired));
        assert!(
            resolved.z >= max.z,
            "{class:?}: camera crossed rotated hull {resolved:?}"
        );
        assert!(min.x < max.x && min.z < max.z);
    }
}

#[test]
fn actual_hangar_orbit_stays_inside_the_drum_and_outside_every_owned_hull() {
    let ctx = egui::Context::default();
    let geometry = fleet_layout_geometry(FleetLayoutRevision::CircularV1);
    let center = point(geometry.center);
    let mut fleet = circular_fleet(&ALL_SHIP_CLASSES);
    for class in ALL_SHIP_CLASSES {
        for berth in &mut fleet.berths {
            berth.active = berth.ship_class == class;
        }
        let pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"CircularOrbitQA", "credits":0, "ore":0,
            "location":Location::Docked, "ship_class":class
        }))
        .unwrap();
        let pose = fleet_berth_pose(fleet.layout_revision, class);
        let mut view = super::super::HangarView::new(None);
        // Seed the already-entered fleet mode so ui exercises each requested
        // pitch/zoom rather than its deliberate first-frame reset defaults.
        view.fleet_mode = true;
        view.class = Some(class);
        for zoom in [0.45, 1., 2.5] {
            for pitch in [0.06, 0.32, 1.25] {
                for yaw in [
                    -0.65,
                    pose.yaw as f32,
                    pose.yaw as f32 + std::f32::consts::FRAC_PI_2,
                    pose.yaw as f32 + std::f32::consts::PI,
                    pose.yaw as f32 - std::f32::consts::FRAC_PI_2,
                ] {
                    view.zoom = zoom;
                    view.pitch = pitch;
                    view.yaw = yaw;
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(1000., 700.),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                view.ui(
                                    ui,
                                    &pilot,
                                    1000,
                                    true,
                                    Some(&fleet),
                                    None,
                                    None,
                                    1000,
                                    None,
                                );
                            });
                        },
                    );
                    let eye = view.last_camera_pose.unwrap().0;
                    let radial = ((eye - center) * Vec3::new(1., 0., 1.)).length();
                    assert!(
                        eye.is_finite()
                            && eye.y >= 0.599
                            && eye.y <= geometry.ceiling_y as f32 - 2.999
                            && radial <= geometry.outer_radius_m as f32 - 3.999,
                        "{class:?} zoom={zoom} pitch={pitch} yaw={yaw}: {eye:?}"
                    );
                    for berth in &fleet.berths {
                        let local = hull_transform(berth).inverse().transform_point3(eye);
                        let (min, max) = camera_hull_bounds(berth.ship_class);
                        assert!(
                            !(local.cmpge(min - Vec3::splat(0.0015)).all()
                                && local.cmple(max + Vec3::splat(0.0015)).all()),
                            "{class:?} zoom={zoom} pitch={pitch} yaw={yaw}: camera inside {:?}",
                            berth.ship_class
                        );
                    }
                }
            }
        }
    }
}
