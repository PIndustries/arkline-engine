//! Real owned vessels in authoritative station berths. No decorative population.
use super::*;
use arkline_protocol::{
    fleet_berth_pose, fleet_layout_geometry, FleetLayoutRevision, HangarBerth,
    HangarView as FleetView, ALL_SHIP_CLASSES, STATION_BOARD_RANGE_M,
};

const FLOOR_SEGMENTS: usize = 256;

// Appended after the embedded hull primitives: their established indices never move.
pub(super) fn floor_mesh_index() -> usize {
    10 + pioneer_asset().map_or(0, Vec::len)
}

pub(super) fn floor_vertices() -> Vec<Vertex> {
    let mut vertices = Vec::with_capacity(FLOOR_SEGMENTS * 12);
    for index in 0..FLOOR_SEGMENTS {
        let angle = |i: usize| i as f32 * std::f32::consts::TAU / FLOOR_SEGMENTS as f32;
        let a = Vec3::new(angle(index).cos(), 0., angle(index).sin());
        let b = Vec3::new(angle(index + 1).cos(), 0., angle(index + 1).sin());
        triangle(&mut vertices, Vec3::ZERO, b, a);
        triangle(&mut vertices, -Vec3::Y, a - Vec3::Y, b - Vec3::Y);
        triangle(&mut vertices, a, b, a - Vec3::Y);
        triangle(&mut vertices, b, b - Vec3::Y, a - Vec3::Y);
    }
    vertices
}

fn point(position: [f64; 3]) -> Vec3 {
    Vec3::from_array(position.map(|value| value as f32))
}

fn block(parts: &mut Parts, center: Vec3, half: Vec3, rotation: Quat, color: Vec3, emission: f32) {
    parts.draw(0, transform(center, half, rotation), color, emission);
}

fn ring(parts: &mut Parts, center: Vec3, radius: f32, half_width: f32, color: Vec3, emission: f32) {
    const SEGMENTS: usize = 128;
    for index in 0..SEGMENTS {
        let angle = (index as f32 + 0.5) * std::f32::consts::TAU / SEGMENTS as f32;
        let radial = Vec3::new(angle.cos(), 0., angle.sin());
        block(
            parts,
            center + radial * radius,
            Vec3::new(
                radius * (std::f32::consts::PI / SEGMENTS as f32).tan(),
                0.025,
                half_width,
            ),
            Quat::from_rotation_y(-(angle + std::f32::consts::FRAC_PI_2)),
            color,
            emission,
        );
    }
}

fn circular_fixtures() -> Vec<local_lighting::Light> {
    let mut lights = Vec::with_capacity(14);
    for class in ALL_SHIP_CLASSES {
        let pose = fleet_berth_pose(FleetLayoutRevision::CircularV1, class);
        let rotation = Quat::from_rotation_y(-pose.yaw as f32);
        for side in [-1., 1.] {
            lights.push(local_lighting::Light {
                position: point(pose.boarding_point) + rotation * Vec3::new(side * 3.1, 4.8, -1.0),
                range: 38.,
                color: Vec3::new(0.70, 0.86, 0.83),
                power: 4.0,
            });
        }
    }
    lights
}

/// Explicit work-light sources. Navigation strips and service windows emit visually only.
pub(super) fn fixture_lights(fleet: &FleetView) -> Option<Vec<local_lighting::Light>> {
    (fleet.layout_revision == FleetLayoutRevision::CircularV1).then(circular_fixtures)
}

fn circular_station_parts() -> &'static [ModelPart] {
    static PARTS: std::sync::OnceLock<Vec<ModelPart>> = std::sync::OnceLock::new();
    PARTS.get_or_init(|| {
        let revision = FleetLayoutRevision::CircularV1;
        let geometry = fleet_layout_geometry(revision);
        let center = point(geometry.center);
        let radius = geometry.outer_radius_m as f32;
        let ceiling = geometry.ceiling_y as f32;
        let steel = Vec3::new(0.075, 0.105, 0.12);
        let deck = Vec3::new(0.13, 0.17, 0.185);
        let rib = Vec3::new(0.17, 0.22, 0.235);
        let amber = Vec3::new(0.50, 0.32, 0.055);
        let mut parts = Parts(Vec::new());
        // Flat capped geometry. The inscribed 256-gon stays within the circle and its
        // chord inset is smaller than the authoritative 0.3 m avatar boundary margin.
        let mut disc = |position: Vec3, r: f32, thickness: f32, color| {
            parts.draw(
                floor_mesh_index(),
                transform(position, Vec3::new(r, thickness, r), Quat::IDENTITY),
                color,
                0.,
            );
        };
        disc(center, geometry.concourse_radius_m as f32, 0.65, deck);
        // A clearly recessed maintenance pit is decoration, not another walkable floor.
        disc(center - Vec3::Y * 18., radius + 8., 1.0, steel * 0.55);
        disc(center + Vec3::Y * (ceiling + 1.), radius + 8., 1.0, steel);
        for class in ALL_SHIP_CLASSES {
            let pose = fleet_berth_pose(revision, class);
            disc(
                Vec3::new(pose.position[0] as f32, 0., pose.position[2] as f32),
                pose.platform_radius_m as f32,
                0.65,
                deck,
            );
        }
        // The bridge endpoints and width are the same ones used by fleet_walkable.
        for class in ALL_SHIP_CLASSES {
            let pose = fleet_berth_pose(revision, class);
            let board = point(pose.boarding_point);
            let delta = (board - center) * Vec3::new(1., 0., 1.);
            let direction = delta.normalize();
            let start = geometry.concourse_radius_m as f32 - 1.;
            let end = delta.length() + 3.;
            let rotation = Quat::from_rotation_y(-direction.z.atan2(direction.x));
            block(
                &mut parts,
                center + direction * ((start + end) * 0.5) - Vec3::Y * 0.25,
                Vec3::new(
                    (end - start) * 0.5,
                    0.25,
                    geometry.bridge_half_width_m as f32,
                ),
                rotation,
                deck,
                0.,
            );
            for side in [-1., 1.] {
                block(
                    &mut parts,
                    center
                        + direction * ((start + end) * 0.5)
                        + rotation
                            * Vec3::new(
                                0.,
                                0.008,
                                side * (geometry.bridge_half_width_m as f32 - 0.18),
                            ),
                    Vec3::new((end - start) * 0.5, 0.008, 0.08),
                    rotation,
                    amber,
                    0.25,
                );
            }
            let hull_rotation = Quat::from_rotation_y(-pose.yaw as f32);
            let spec = class.spec();
            // Floor outlines identify the authoritative hull exclusion zone, including
            // empty reserved slots. They do not fabricate an owned vessel.
            for side in [-1., 1.] {
                block(
                    &mut parts,
                    point(pose.position) * Vec3::new(1., 0., 1.)
                        + hull_rotation
                            * Vec3::new(0., 0.012, side * (spec.width_m as f32 * 0.5 + 0.3)),
                    Vec3::new(spec.length_m as f32 * 0.5 + 0.3, 0.012, 0.09),
                    hull_rotation,
                    amber,
                    0.15,
                );
                block(
                    &mut parts,
                    point(pose.position) * Vec3::new(1., 0., 1.)
                        + hull_rotation
                            * Vec3::new(side * (spec.length_m as f32 * 0.5 + 0.3), 0.012, 0.),
                    Vec3::new(0.09, 0.012, spec.width_m as f32 * 0.5 + 0.3),
                    hull_rotation,
                    amber,
                    0.15,
                );
            }
        }
        ring(
            &mut parts,
            center + Vec3::Y * 0.015,
            geometry.concourse_radius_m as f32 - 0.65,
            0.12,
            amber,
            0.2,
        );
        // A closed industrial drum. Tangent wall panels overlap at the ribs; the
        // interior remains outside every measured platform and full-size hull.
        const WALL_SEGMENTS: usize = 128;
        for index in 0..WALL_SEGMENTS {
            let angle = index as f32 * std::f32::consts::TAU / WALL_SEGMENTS as f32;
            let radial = Vec3::new(angle.cos(), 0., angle.sin());
            let rotation = Quat::from_rotation_y(-(angle + std::f32::consts::FRAC_PI_2));
            let half_tangent = radius * (std::f32::consts::PI / WALL_SEGMENTS as f32).tan() + 0.25;
            block(
                &mut parts,
                center + radial * radius + Vec3::Y * (ceiling * 0.5 - 9.),
                Vec3::new(half_tangent, ceiling * 0.5 + 9., 2.),
                rotation,
                steel,
                0.,
            );
            block(
                &mut parts,
                center + radial * (radius - 2.5) + Vec3::Y * (ceiling * 0.5),
                Vec3::new(0.65, ceiling * 0.5, 0.8),
                rotation,
                rib,
                0.,
            );
            // Recessed service panels/windows and overhead ribs have no walking obstruction.
            if index % 4 == 0 {
                block(
                    &mut parts,
                    center + radial * (radius - 2.1) + Vec3::Y * 14.,
                    Vec3::new(half_tangent * 0.55, 4., 0.12),
                    rotation,
                    Vec3::new(0.08, 0.18, 0.20),
                    0.18,
                );
                block(
                    &mut parts,
                    center + radial * (radius - 3.) + Vec3::Y * 24.,
                    Vec3::new(4., 0.15, 0.2),
                    rotation,
                    Vec3::new(0.65, 0.82, 0.8),
                    0.45,
                );
            }
            if index % 8 == 0 {
                block(
                    &mut parts,
                    center + radial * (radius * 0.5) + Vec3::Y * (ceiling - 1.),
                    Vec3::new(radius * 0.5, 0.65, 0.9),
                    Quat::from_rotation_y(-angle),
                    rib,
                    0.,
                );
            }
        }
        for fixture in circular_fixtures() {
            block(
                &mut parts,
                fixture.position,
                Vec3::new(1.4, 0.07, 0.26),
                Quat::IDENTITY,
                fixture.color,
                0.9,
            );
            block(
                &mut parts,
                fixture.position + Vec3::Y * ((ceiling - fixture.position.y) * 0.5),
                Vec3::new(0.018, (ceiling - fixture.position.y) * 0.5, 0.018),
                Quat::IDENTITY,
                steel,
                0.,
            );
        }
        parts.0
    })
}

pub(super) fn hull_transform(berth: &HangarBerth) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::splat(100.0),
        Quat::from_rotation_y(-berth.yaw as f32),
        Vec3::from_array(berth.position.map(|v| v as f32)),
    )
}
/// Body only; articulated turrets have a separate envelope.
pub(super) fn hull_parts(berth: &HangarBerth) -> impl Iterator<Item = ModelPart> + '_ {
    let parent = hull_transform(berth);
    cached_ship_parts(berth.ship_class)
        .iter()
        .map(move |&(mesh, model, color, emission)| (mesh, parent * model, color, emission * 0.2))
}
pub(super) fn board_distance(avatar: &StationAvatar, berth: &HangarBerth) -> f64 {
    avatar
        .position
        .iter()
        .zip(berth.boarding_point)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}
pub(super) fn can_board(pilot: &PilotView, berth: &HangarBerth, live: bool) -> bool {
    live && berth.can_board
        && pilot.station_avatar.as_ref().is_some_and(|a| {
            !a.onboard_interior
                && !a.seated
                && a.jump_started_ms.is_none()
                && board_distance(a, berth) <= STATION_BOARD_RANGE_M
        })
}
/// The same capped list drives visible ships and their boarding labels.
pub(super) fn visible(fleet: &FleetView, eye: Vec3) -> Vec<&HangarBerth> {
    let mut berths: Vec<_> = fleet.berths.iter().collect();
    berths.sort_by(|a, b| {
        Vec3::from_array(a.position.map(|v| v as f32))
            .distance_squared(eye)
            .total_cmp(&Vec3::from_array(b.position.map(|v| v as f32)).distance_squared(eye))
    });
    berths.truncate(16);
    berths
}
#[cfg(test)]
pub(super) fn parts(fleet: &FleetView, eye: Vec3) -> Vec<ModelPart> {
    parts_except_hulls(fleet, eye, &[])
}
pub(super) fn parts_except_hulls(fleet: &FleetView, eye: Vec3, hidden: &[&str]) -> Vec<ModelPart> {
    if fleet.layout_revision == FleetLayoutRevision::CircularV1 {
        let mut parts = Parts(circular_station_parts().to_vec());
        append_vessels(&mut parts, fleet, eye, hidden);
        return parts.0;
    }
    let mut p = Parts(Vec::new());
    let l = fleet.layout.half_length as f32;
    let w = fleet
        .berths
        .iter()
        .map(|b| b.position[2].abs() as f32 + b.ship_class.spec().width_m as f32 / 2.0 + 12.0)
        .fold(fleet.layout.half_width as f32, f32::max);
    let h = fleet
        .berths
        .iter()
        .map(|b| b.ship_class.spec().height_m as f32 + 30.0)
        .fold(30.0, f32::max);
    let steel = Vec3::new(0.06, 0.085, 0.09);
    let mut block = |center: Vec3, half: Vec3, color: Vec3, emission: f32| {
        p.draw(0, transform(center, half, Quat::IDENTITY), color, emission)
    };
    block(Vec3::new(0., -0.5, 0.), Vec3::new(l, 0.5, w), steel, 0.);
    block(Vec3::new(0., h, 0.), Vec3::new(l, 0.5, w), steel, 0.);
    for side in [-1., 1.] {
        block(
            Vec3::new(side * l, h * 0.5, 0.),
            Vec3::new(0.5, h * 0.5, w),
            steel,
            0.,
        );
        block(
            Vec3::new(0., h * 0.5, side * w),
            Vec3::new(l, h * 0.5, 0.5),
            steel,
            0.,
        );
    }
    block(
        Vec3::new(0., 0.025, -8.),
        Vec3::new(l, 0.025, 0.18),
        Vec3::new(0.015, 0.32, 0.12),
        0.6,
    );
    // Slim suspended fixtures preserve the sightline to full-height vessels.
    // No solid posts are placed in the authoritative walking lane.
    for i in 0..24 {
        let x = -36.0 + (2.0 * l / 24.0) * i as f32;
        block(
            Vec3::new(x, 7.8, -8.),
            Vec3::new(3., 0.05, 0.35),
            Vec3::new(0.38, 0.55, 0.45),
            0.8,
        );
        // Suspension cables and ceiling ribs remain above walking clearance.
        block(
            Vec3::new(x, (h + 8.) * 0.5, -8.),
            Vec3::new(0.025, (h - 8.) * 0.5, 0.025),
            steel,
            0.,
        );
        block(
            Vec3::new(x, h - 0.3, 0.),
            Vec3::new(0.2, 0.3, w),
            steel * 1.4,
            0.,
        );
        block(
            Vec3::new(x, 0.008, 0.),
            Vec3::new(0.035, 0.008, w),
            steel * 0.45,
            0.,
        );
    }
    // The doorway faces the rear aisle; the route skirts the room's side wall.
    block(
        Vec3::new(12., 0.03, -23.),
        Vec3::new(0.1, 0.03, 15.),
        Vec3::new(0.02, 0.32, 0.12),
        0.5,
    );
    block(
        Vec3::new(6., 0.03, -39.),
        Vec3::new(6., 0.03, 0.1),
        Vec3::new(0.02, 0.32, 0.12),
        0.5,
    );
    for b in &fleet.berths {
        let x = b.boarding_point[0] as f32;
        for side in [-1., 1.] {
            block(
                Vec3::new(x + side * 3., 0.02, 1.),
                Vec3::new(0.10, 0.02, 2.),
                Vec3::new(0.32, 0.30, 0.12),
                0.,
            );
        }
    }
    append_vessels(&mut p, fleet, eye, hidden);
    p.0
}

fn append_vessels(p: &mut Parts, fleet: &FleetView, eye: Vec3, hidden: &[&str]) {
    for b in visible(fleet, eye) {
        let boarding = Vec3::from_array(b.boarding_point.map(|v| v as f32));
        p.draw(
            0,
            transform(
                boarding + Vec3::new(0., 0.03, 0.),
                Vec3::new(1.2, 0.03, 1.2),
                Quat::from_rotation_y(-b.yaw as f32),
            ),
            if b.active {
                Vec3::new(0.03, 0.5, 0.2)
            } else {
                Vec3::new(0.12, 0.25, 0.2)
            },
            0.5,
        );
        let t = hull_transform(b);
        if !hidden.contains(&b.id.as_str()) {
            p.0.extend(hull_parts(b));
        }
        for slot in 0..2 {
            p.0.extend(
                turret_parts(b.ship_class, Quat::IDENTITY, slot)
                    .into_iter()
                    .map(|(mesh, m, c, e)| (mesh, t * m, c, e * 0.2)),
            );
        }
    }
}
#[allow(clippy::too_many_arguments)]
pub(super) fn labels(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vp: Mat4,
    eye: Vec3,
    pilot: &PilotView,
    fleet: &FleetView,
    unobstructed: bool,
    live: bool,
) -> Option<HangarAction> {
    let mut action = None;
    for b in visible(fleet, eye) {
        let point = Vec3::from_array(b.boarding_point.map(|v| v as f32)) + Vec3::Y * 1.8;
        if camera(fleet, eye, point).distance(point) > 0.05 {
            continue;
        }
        let Some(screen) = project_point(vp, rect, point) else {
            continue;
        };
        let distance = pilot.station_avatar.as_ref().map(|a| board_distance(a, b));
        let mut name = format!(
            "{}{}",
            b.ship_class.spec().name,
            if b.active { " · ACTIVE" } else { "" }
        );
        if fleet.layout_revision == FleetLayoutRevision::CircularV1 {
            let spec = b.ship_class.spec();
            name.push_str(&format!(
                "\n{:.0} × {:.0} × {:.0} m · floating berth",
                spec.length_m, spec.width_m, spec.height_m
            ));
        }
        ui.painter_at(rect).text(
            screen,
            egui::Align2::CENTER_BOTTOM,
            name,
            egui::FontId::proportional(14.),
            egui::Color32::from_rgb(135, 225, 175),
        );
        if let Some(distance) = distance {
            let label = if can_board(pilot, b, live) {
                "Board vessel".to_owned()
            } else {
                format!("Boarding access · {distance:.1} m")
            };
            let button =
                egui::Rect::from_center_size(screen + egui::vec2(0., 15.), egui::vec2(180., 25.));
            if rect.contains_rect(button)
                && ui.put(button, egui::Button::new(label)).clicked()
                && unobstructed
                && can_board(pilot, b, live)
            {
                action = Some(HangarAction::BoardShip(b.id.clone()));
            }
        }
    }
    action
}

pub(super) fn quarters_offset() -> Vec3 {
    Vec3::new(
        0.,
        0.,
        (arkline_protocol::fleet_quarters_layout().max[2]
            - arkline_protocol::quarters_layout(ShipClass::Pioneer).max[2]) as f32,
    )
}

// The drum's ribs project 3.3 m inward and its roof beams hang 1.65 m below
// the nominal enclosure. Keep both player camera modes inside those details.
fn enclosure_fraction(fleet: &FleetView, target: Vec3, desired: Vec3) -> f32 {
    if fleet.layout_revision != FleetLayoutRevision::CircularV1 {
        return 1.;
    }
    let geometry = fleet_layout_geometry(fleet.layout_revision);
    let center = point(geometry.center);
    let radius = geometry.outer_radius_m as f32 - 4.;
    let a = (target - center) * Vec3::new(1., 0., 1.);
    let direction = (desired - target) * Vec3::new(1., 0., 1.);
    let aa = direction.length_squared();
    let mut limit = 1.0f32;
    if aa > 1e-8 && a.length_squared() < radius * radius {
        let bb = a.dot(direction);
        let discriminant = bb * bb - aa * (a.length_squared() - radius * radius);
        if discriminant >= 0. {
            let exit = (-bb + discriminant.sqrt()) / aa;
            limit = limit.min(exit.max(0.));
        }
    }
    let ceiling = geometry.ceiling_y as f32 - 3.;
    if desired.y > ceiling && desired.y > target.y {
        limit = limit.min(((ceiling - target.y) / (desired.y - target.y)).max(0.));
    }
    if desired.y < 0.6 && desired.y < target.y {
        limit = limit.min(((0.6 - target.y) / (desired.y - target.y)).max(0.));
    }
    limit.clamp(0., 1.)
}

pub(super) fn enclosure_camera(fleet: &FleetView, target: Vec3, desired: Vec3) -> Vec3 {
    target.lerp(desired, enclosure_fraction(fleet, target, desired))
}

// Orbit targets are inside their active hull: the walking first-hit resolver
// would return the target itself. Search the bounded ray's free intervals instead.
fn orbit_ray_eye(
    fleet: &FleetView,
    target: Vec3,
    direction: Vec3,
    requested_distance: f32,
) -> Option<Vec3> {
    let geometry = fleet_layout_geometry(fleet.layout_revision);
    let reach = geometry.outer_radius_m as f32 * 4. + geometry.ceiling_y as f32 * 2.;
    let far = enclosure_camera(fleet, target, target + direction * reach);
    let distance = target.distance(far);
    if distance < 0.01 {
        return None;
    }
    let requested = (requested_distance / distance).clamp(0., 1.);
    let inset = 0.25 / distance;
    let mut intervals = Vec::with_capacity(fleet.berths.len());
    let mut candidates = vec![requested, 1.];
    for berth in &fleet.berths {
        let inverse = hull_transform(berth).inverse();
        let a = inverse.transform_point3(target);
        let b = inverse.transform_point3(far);
        let (min, max) = camera_hull_bounds(berth.ship_class);
        let min = min - Vec3::splat(0.002);
        let max = max + Vec3::splat(0.002);
        if let (Some(near), Some(reverse)) = (
            quarters::segment_box(a, b, min, max),
            quarters::segment_box(b, a, min, max),
        ) {
            let end = 1. - reverse;
            intervals.push((near, end));
            candidates.extend([near - inset, end + inset]);
        }
    }
    candidates
        .into_iter()
        .filter(|candidate| {
            *candidate > 0.
                && *candidate <= 1.
                && intervals
                    .iter()
                    .all(|(near, end)| *candidate < *near || *candidate > *end)
        })
        .min_by(|a, b| (a - requested).abs().total_cmp(&(b - requested).abs()))
        .map(|fraction| target.lerp(far, fraction))
}

/// Circular orbit is bounded by real enclosure and hull dimensions. A close
/// zoom may move outward to clear a large hull; a blocked ray faces the core.
pub(super) fn orbit_camera(fleet: &FleetView, target: Vec3, desired: Vec3) -> Vec3 {
    if fleet.layout_revision != FleetLayoutRevision::CircularV1 {
        return desired;
    }
    let geometry = fleet_layout_geometry(fleet.layout_revision);
    let inward = ((point(geometry.center) - target) * Vec3::new(1., 0., 1.)).normalize_or_zero();
    let requested_distance = target.distance(desired);
    let requested_direction = (desired - target).normalize_or_zero();
    for direction in [requested_direction, inward, Vec3::Y] {
        if direction.length_squared() > 0.5 {
            if let Some(eye) = orbit_ray_eye(fleet, target, direction, requested_distance) {
                return eye;
            }
        }
    }
    // The shared catalog reserves this core column without any vessel footprint.
    point(geometry.center) + Vec3::Y * target.y.clamp(0.6, geometry.ceiling_y as f32 - 3.)
}

pub(super) fn camera(fleet: &FleetView, target: Vec3, desired: Vec3) -> Vec3 {
    let mut t = enclosure_fraction(fleet, target, desired);
    for o in arkline_protocol::fleet_station_obstacles() {
        let min = Vec3::from_array(o.min.map(|v| v as f32)) - Vec3::splat(0.15);
        let max = Vec3::from_array(o.max.map(|v| v as f32)) + Vec3::splat(0.15);
        if let Some(hit) = quarters::segment_box(target, desired, min, max) {
            t = t.min((hit - 0.035).max(0.0));
        }
    }
    let room = arkline_protocol::fleet_quarters_layout();
    if let Some(hit) = quarters::segment_box(
        target,
        desired,
        Vec3::new(room.min[0] as f32, 3.05, room.min[2] as f32),
        Vec3::new(room.max[0] as f32, 3.45, room.max[2] as f32),
    ) {
        t = t.min((hit - 0.035).max(0.0));
    }
    for b in &fleet.berths {
        let inv = hull_transform(b).inverse();
        let (min, max) = camera_hull_bounds(b.ship_class);
        if let Some(hit) = quarters::segment_box(
            inv.transform_point3(target),
            inv.transform_point3(desired),
            min - Vec3::splat(0.0015),
            max + Vec3::splat(0.0015),
        ) {
            t = t.min((hit - 0.035).max(0.0));
        }
    }
    target.lerp(
        desired,
        if fleet.layout_revision == FleetLayoutRevision::CircularV1 {
            t.clamp(0., 1.)
        } else {
            t.max(0.025)
        },
    )
}

// Local-space bounds are cached by class, never by a world-space berth pose. Every
// query applies the current inverse hull pose, so yaw and authoritative relocation
// cannot leave stale axis-aligned obstacles behind.
fn camera_hull_bounds(class: ShipClass) -> (Vec3, Vec3) {
    static BOUNDS: std::sync::OnceLock<Vec<(Vec3, Vec3)>> = std::sync::OnceLock::new();
    let bounds = BOUNDS.get_or_init(|| {
        ALL_SHIP_CLASSES
            .iter()
            .map(|class| {
                let mut parts = cached_ship_parts(*class).to_vec();
                for slot in 0..2 {
                    parts.extend(turret_parts(*class, Quat::IDENTITY, slot));
                }
                parts_bounds(&parts)
            })
            .collect()
    });
    bounds[ALL_SHIP_CLASSES
        .iter()
        .position(|item| *item == class)
        .unwrap()]
}

#[cfg(test)]
#[path = "fleet_presentation_tests.rs"]
mod presentation_tests;

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PilotView, FleetView) {
        let pilot=serde_json::from_value(serde_json::json!({"name":"DockQA","credits":0,"ore":0,"location":Location::Docked,"station_avatar":{"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000}})).unwrap();
        let berths = arkline_protocol::ALL_SHIP_CLASSES
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let (position, boarding_point) = arkline_protocol::fleet_berth(*c);
                HangarBerth {
                    id: format!("vessel-{i}"),
                    ship_class: *c,
                    position,
                    yaw: 0.,
                    boarding_point,
                    active: i == 0,
                    can_board: true,
                }
            })
            .collect();
        (
            pilot,
            FleetView {
                layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
                layout: arkline_protocol::fleet_hangar_layout(),
                berths,
            },
        )
    }
    #[test]
    fn physical_boarding_requires_authority_range_ground_and_live_state() {
        let (mut p, mut f) = fixture();
        assert!(can_board(&p, &f.berths[0], true));
        assert!(!can_board(&p, &f.berths[1], true));
        assert!(!can_board(&p, &f.berths[0], false));
        f.berths[0].can_board = false;
        assert!(!can_board(&p, &f.berths[0], true));
        f.berths[0].can_board = true;
        p.station_avatar.as_mut().unwrap().jump_started_ms = Some(1000);
        assert!(!can_board(&p, &f.berths[0], true));
    }
    #[test]
    fn render_transform_preserves_meter_dimensions_and_fixed_berths() {
        let (_, fleet) = fixture();
        for b in &fleet.berths {
            let t = hull_transform(b);
            let spec = b.ship_class.spec();
            let (lo, hi) = parts_bounds(cached_ship_parts(b.ship_class));
            let bounds = t.transform_point3(hi) - t.transform_point3(lo);
            assert!(
                (bounds
                    - Vec3::new(
                        spec.length_m as f32,
                        if b.ship_class == ShipClass::Pioneer {
                            20.35
                        } else {
                            spec.height_m as f32
                        },
                        spec.width_m as f32
                    ))
                .abs()
                .max_element()
                    < 0.02
            );
        }
        let eye = Vec3::new(0., 2., -5.);
        let first = parts(&fleet, eye);
        let mut switched = fleet.clone();
        for b in &mut switched.berths {
            b.active = !b.active;
        }
        let second = parts(&switched, eye);
        assert_eq!(first.len(), second.len());
        assert!(first
            .iter()
            .zip(second)
            .all(|(a, b)| a.0 == b.0 && a.1 == b.1));
    }
    #[test]
    fn world_board_button_pointer_click_returns_real_vessel_id() {
        let (p, f) = fixture();
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900., 650.));
        let eye = Vec3::new(0., 3., -6.);
        let point = Vec3::new(0., 1.8, 0.);
        let vp = Mat4::perspective_rh_gl(0.8, rect.aspect_ratio(), 0.1, 1000.)
            * Mat4::look_at_rh(eye, point, Vec3::Y);
        let at = project_point(vp, rect, point).unwrap() + egui::vec2(0., 15.);
        let mut picked = None;
        for events in [
            vec![],
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        picked = labels(ui, rect, vp, eye, &p, &f, true, true).or(picked.take());
                    });
                },
            );
        }
        assert_eq!(picked, Some(HangarAction::BoardShip("vessel-0".into())));
    }
    #[test]
    fn camera_does_not_cross_hull_and_visible_list_is_bounded() {
        let (_, mut f) = fixture();
        // The raised asymmetric hull has a real 2.40m lower clearance at this
        // conservative berth. Test the hull's occupied height, not empty air.
        let y = f.berths[0].position[1] as f32;
        let target = Vec3::new(0., y, 0.);
        let desired = Vec3::new(0., y, 30.);
        assert!(camera(&f, target, desired).z < 6.);
        let below = Vec3::new(0., 1.8, 30.);
        assert!(camera(&f, Vec3::new(0., 1.8, 0.), below).abs_diff_eq(below, 0.001));
        let first = f.berths[0].clone();
        f.berths = vec![first; 30];
        assert_eq!(visible(&f, Vec3::ZERO).len(), 16);
    }
}
