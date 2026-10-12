//! Real owned vessels in authoritative station berths. No decorative population.
use super::*;
use arkline_protocol::{HangarBerth, HangarView as FleetView, STATION_BOARD_RANGE_M};

pub(super) fn hull_transform(berth: &HangarBerth) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::splat(100.0),
        Quat::from_rotation_y(-berth.yaw as f32),
        Vec3::from_array(berth.position.map(|v| v as f32)),
    )
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
pub(super) fn parts(fleet: &FleetView, eye: Vec3) -> Vec<ModelPart> {
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
    // Human-scale concourse canopy and work lights make the walking lane legible.
    block(Vec3::new(0., 8., -8.), Vec3::new(l, 0.15, 11.), steel, 0.);
    for i in 0..24 {
        let x = -36.0 + (2.0 * l / 24.0) * i as f32;
        block(
            Vec3::new(x, 7.8, -8.),
            Vec3::new(3., 0.05, 0.35),
            Vec3::new(0.38, 0.55, 0.45),
            0.8,
        );
        block(Vec3::new(x, 4., -18.), Vec3::new(0.16, 4., 0.16), steel, 0.);
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
    for b in visible(fleet, eye) {
        let boarding = Vec3::from_array(b.boarding_point.map(|v| v as f32));
        p.draw(
            0,
            transform(
                boarding + Vec3::new(0., 0.03, 0.),
                Vec3::new(1.2, 0.03, 1.2),
                Quat::IDENTITY,
            ),
            if b.active {
                Vec3::new(0.03, 0.5, 0.2)
            } else {
                Vec3::new(0.12, 0.25, 0.2)
            },
            0.5,
        );
        let t = hull_transform(b);
        p.0.extend(
            cached_ship_parts(b.ship_class)
                .iter()
                .map(|&(mesh, m, c, e)| (mesh, t * m, c, e * 0.2)),
        );
        for slot in 0..2 {
            p.0.extend(
                turret_parts(b.ship_class, Quat::IDENTITY, slot)
                    .into_iter()
                    .map(|(mesh, m, c, e)| (mesh, t * m, c, e * 0.2)),
            );
        }
    }
    p.0
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
        let name = format!(
            "{}{}",
            b.ship_class.spec().name,
            if b.active { " · ACTIVE" } else { "" }
        );
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
pub(super) fn camera(fleet: &FleetView, target: Vec3, desired: Vec3) -> Vec3 {
    let mut t = 1.0f32;
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
        let spec = b.ship_class.spec();
        let half = Vec3::new(
            spec.length_m as f32,
            spec.height_m as f32,
            spec.width_m as f32,
        ) / 200.;
        if let Some(hit) = quarters::segment_box(
            inv.transform_point3(target),
            inv.transform_point3(desired),
            -half,
            half,
        ) {
            t = t.min((hit - 0.035).max(0.0));
        }
    }
    target.lerp(desired, t.max(0.025))
}

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
                        spec.height_m as f32,
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
        let target = Vec3::new(0., 1.8, 0.);
        let desired = Vec3::new(0., 1.8, 30.);
        assert!(camera(&f, target, desired).z < 6.);
        let first = f.berths[0].clone();
        f.berths = vec![first; 30];
        assert_eq!(visible(&f, Vec3::ZERO).len(), 16);
    }
}
