//! Docked Pioneer decks, rendered from the server's shared collision plan.
use super::super::surface_roles::SurfaceRole;
use super::*;
use arkline_protocol::{
    RoomLayoutDocument, RoomLayoutState, StationObstacleKind, PIONEER_INTERIOR_ENTRY,
};
use std::sync::Arc;

#[cfg(test)]
fn block(p: &mut Parts, min: Vec3, max: Vec3, color: Vec3, emission: f32) {
    surface_block(p, min, max, color, emission, SurfaceRole::CoatedPanel);
}
fn surface_block(
    p: &mut Parts,
    min: Vec3,
    max: Vec3,
    color: Vec3,
    emission: f32,
    role: SurfaceRole,
) {
    p.draw(
        role.cube(),
        transform((min + max) * 0.5, (max - min) * 0.5, Quat::IDENTITY),
        color,
        emission,
    );
}
#[cfg(test)]
pub(super) fn parts(deck: u8) -> Vec<ModelPart> {
    cached_parts(deck).as_ref().clone()
}
pub(super) fn cached_parts(deck: u8) -> Arc<Vec<ModelPart>> {
    static DECKS: std::sync::OnceLock<[Arc<Vec<ModelPart>>; 2]> = std::sync::OnceLock::new();
    DECKS.get_or_init(|| {
        let document = arkline_protocol::default_pioneer();
        [
            Arc::new(build_parts(1, &document)),
            Arc::new(build_parts(2, &document)),
        ]
    })[usize::from(deck == 2)]
    .clone()
}

pub(super) struct LayoutPartsCache {
    revision: u64,
    document: RoomLayoutDocument,
    decks: [Arc<Vec<ModelPart>>; 2],
}

impl LayoutPartsCache {
    pub(super) fn new(state: &RoomLayoutState) -> Option<Self> {
        state.document.validate_live().ok()?;
        Some(Self {
            revision: state.revision,
            document: state.document.clone(),
            decks: [
                Arc::new(build_parts(1, &state.document)),
                Arc::new(build_parts(2, &state.document)),
            ],
        })
    }

    pub(super) fn matches(&self, state: &RoomLayoutState) -> bool {
        self.revision == state.revision && self.document == state.document
    }

    #[cfg(test)]
    pub(super) fn document(&self) -> &RoomLayoutDocument {
        &self.document
    }

    pub(super) fn parts(&self, deck: u8) -> Arc<Vec<ModelPart>> {
        self.decks[usize::from(deck == 2)].clone()
    }
}

fn build_parts(deck: u8, document: &RoomLayoutDocument) -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    let steel = Vec3::new(0.045, 0.065, 0.072);
    surface_block(
        &mut p,
        Vec3::new(0.0, -0.1, 0.0),
        Vec3::new(32.0, 0.0, 14.0),
        steel,
        0.0,
        SurfaceRole::GripDeck,
    );
    surface_block(
        &mut p,
        Vec3::new(0.0, 3.2, 0.0),
        Vec3::new(32.0, 3.3, 14.0),
        steel,
        0.0,
        SurfaceRole::Composite,
    );
    for obstacle in document.obstacles(deck) {
        p.0.extend(obstacle_model_parts(obstacle));
    }
    for room in document.rooms.iter().filter(|room| room.deck == deck) {
        p.0.extend(room_decoration_parts(room));
    }
    // Raised ceiling ribs and recessed light strips make the access path readable.
    for x in (2..32).step_by(3) {
        surface_block(
            &mut p,
            Vec3::new(x as f32, 3.05, 5.9),
            Vec3::new(x as f32 + 0.10, 3.2, 8.1),
            Vec3::new(0.10, 0.14, 0.15),
            0.0,
            SurfaceRole::ExposedAlloy,
        );
        surface_block(
            &mut p,
            Vec3::new(x as f32 + 0.15, 3.17, 6.4),
            Vec3::new(x as f32 + 1.4, 3.19, 7.6),
            Vec3::new(0.40, 0.55, 0.45),
            0.6,
            SurfaceRole::FixtureDiffuser,
        );
        surface_block(
            &mut p,
            Vec3::new(x as f32, 0.001, 5.9),
            Vec3::new(x as f32 + 0.015, 0.003, 8.1),
            Vec3::new(0.015, 0.025, 0.03),
            0.0,
            SurfaceRole::Composite,
        );
    }
    for x in (1..32).step_by(2) {
        surface_block(
            &mut p,
            Vec3::new(x as f32, 0.008, 6.95),
            Vec3::new(x as f32 + 0.7, 0.012, 7.05),
            Vec3::new(0.02, 0.38, 0.15),
            0.35,
            SurfaceRole::EnamelTrim,
        );
    }
    let lift = Vec3::from_array(arkline_protocol::PIONEER_LIFT_POINT.map(|v| v as f32));
    surface_block(
        &mut p,
        lift + Vec3::new(-0.9, 0.005, -0.9),
        lift + Vec3::new(0.9, 0.01, 0.9),
        Vec3::new(0.30, 0.23, 0.05),
        0.3,
        SurfaceRole::EnamelTrim,
    );
    let entry = if deck == 1 {
        PIONEER_INTERIOR_ENTRY
    } else {
        arkline_protocol::PIONEER_LIFT_POINT
    };
    let e = Vec3::from_array(entry.map(|v| v as f32));
    surface_block(
        &mut p,
        e + Vec3::new(-0.6, 0.01, -0.6),
        e + Vec3::new(0.6, 0.02, 0.6),
        Vec3::new(0.15, 0.5, 0.25),
        0.3,
        SurfaceRole::EnamelTrim,
    );
    if deck == arkline_protocol::PIONEER_CONNECTED_ROOM.deck {
        p.0.extend(connected_room_static_parts());
    }
    p.0
}

/// The exact installed wall/storage draw recipe for one protocol obstacle.
fn obstacle_model_parts(o: arkline_protocol::StationObstacle) -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    let steel = Vec3::new(0.045, 0.065, 0.072);
    let min = Vec3::from_array(o.min.map(|v| v as f32));
    let max = Vec3::from_array(o.max.map(|v| v as f32));
    let fixture = matches!(o.kind, StationObstacleKind::Storage);
    surface_block(
        &mut p,
        min,
        max,
        if fixture {
            Vec3::new(0.09, 0.13, 0.14)
        } else {
            steel
        },
        0.0,
        if fixture {
            SurfaceRole::Composite
        } else {
            SurfaceRole::CoatedPanel
        },
    );
    if fixture {
        surface_block(
            &mut p,
            Vec3::new(min.x + 0.1, max.y - 0.5, max.z + 0.005),
            Vec3::new(max.x - 0.1, max.y - 0.05, max.z + 0.02),
            Vec3::new(0.015, 0.25, 0.1),
            0.4,
            SurfaceRole::InstrumentGlass,
        );
    }
    p.0
}

/// Existing doorway surrounds and ceiling light belonging to one room.
fn room_decoration_parts(room: &arkline_protocol::RoomLayout) -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    let [x, z] = room.origin_xz_m.map(|v| v as f32);
    let [length, depth, height] = room.clear_lwh_m.map(|v| v as f32);
    let [door, _door_z] = room.door_center_xz_m.map(|v| v as f32);
    let [door_width, door_height] = room.door_clear_wh_m.map(|v| v as f32);
    let face = room.door_wall_z() as f32;
    for side in [-1.0, 1.0] {
        let jamb_x = door + side * door_width * 0.5;
        surface_block(
            &mut p,
            Vec3::new(jamb_x - 0.025, 0.0, face - 0.01),
            Vec3::new(jamb_x + 0.025, door_height, face + 0.17),
            Vec3::new(0.025, 0.35, 0.14),
            0.5,
            SurfaceRole::EnamelTrim,
        );
    }
    surface_block(
        &mut p,
        Vec3::new(door - door_width * 0.5, door_height - 0.04, face - 0.01),
        Vec3::new(door + door_width * 0.5, door_height, face + 0.17),
        Vec3::new(0.025, 0.35, 0.14),
        0.5,
        SurfaceRole::EnamelTrim,
    );
    surface_block(
        &mut p,
        Vec3::new(x + 0.6, (height - 0.05).min(3.15), z + depth * 0.35),
        Vec3::new(
            x + (length - 0.6).max(0.7),
            (height - 0.02).min(3.18),
            z + depth * 0.65,
        ),
        Vec3::new(0.5, 0.65, 0.55),
        0.7,
        SurfaceRole::FixtureDiffuser,
    );
    p.0
}

/// Existing airlock bodies and their service conduits, in installed draw order.
fn connected_room_static_parts() -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    p.0.extend(equipment_static_parts("pioneer.deck1.airlock.breaker"));
    surface_block(
        &mut p,
        Vec3::new(4.28, 2.9, 0.3),
        Vec3::new(4.32, 2.94, 5.3),
        Vec3::new(0.1, 0.14, 0.12),
        0.,
        SurfaceRole::ExposedAlloy,
    );
    surface_block(
        &mut p,
        Vec3::new(4.3, 2.9, 0.28),
        Vec3::new(10.8, 2.94, 0.32),
        Vec3::new(0.1, 0.14, 0.12),
        0.,
        SurfaceRole::ExposedAlloy,
    );
    p.0.extend(equipment_static_parts("pioneer.deck1.airlock.vent"));
    p.0
}

/// Exact existing room walls, storage, doorway surrounds and ceiling light.
/// Registered docked scenes supply shared slabs/access paths separately, so the
/// original full-deck slabs cannot cover the physical stairwell opening.
pub(super) fn room_shell_parts(room: &arkline_protocol::RoomLayout) -> Vec<ModelPart> {
    let mut parts: Vec<ModelPart> = room
        .obstacles()
        .into_iter()
        .flat_map(obstacle_model_parts)
        .collect();
    parts.extend(room_decoration_parts(room));
    parts
}

/// A cutaway of one default room using installed runtime recipes. Shared deck
/// floors, ceilings, corridor ribs/markings and access-core context are omitted.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn inspection_room_parts(source_id: &str) -> Result<Vec<ModelPart>, String> {
    let document = arkline_protocol::default_pioneer();
    let room = document
        .rooms
        .iter()
        .find(|room| room.source_room_id.as_deref() == Some(source_id))
        .ok_or_else(|| format!("Unknown Pioneer source room: {source_id}"))?;
    let mut parts = room_shell_parts(room);
    if room.runtime_room_id.as_deref() == Some(arkline_protocol::PIONEER_CONNECTED_ROOM.room_id) {
        parts.extend(connected_room_static_parts());
        let mut state =
            arkline_protocol::PioneerRoomState::uncommissioned("inspection-only".into());
        state.commissioned = true;
        state.breaker_closed = true;
        state.light_on = true;
        state.door_open = false;
        parts.extend(state_parts(room.deck, Some(&state)));
    }
    Ok(parts)
}

/// Installed fixture bodies, kept separate from the shared room conduits.
fn equipment_static_parts(id: &str) -> Vec<ModelPart> {
    let mut p = Parts(Vec::new());
    let steel = Vec3::new(0.045, 0.065, 0.072);
    let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
    if id == "pioneer.deck1.airlock.breaker" {
        // Wall-flush service panel and ceiling conduit do not change walkable clearance.
        let panel = Vec3::from_array(d.breaker_position.map(|v| v as f32));
        surface_block(
            &mut p,
            panel - Vec3::new(0.04, 0.5, 0.35),
            panel + Vec3::new(-0.01, 0.5, 0.35),
            steel * 1.8,
            0.,
            SurfaceRole::ExposedAlloy,
        );
        for z in [2.72, 2.9, 3.08, 3.26] {
            surface_block(
                &mut p,
                Vec3::new(4.241, 1.15, z),
                Vec3::new(4.245, 1.65, z + 0.025),
                steel * 0.3,
                0.,
                SurfaceRole::Composite,
            );
        }
    }
    if id == "pioneer.deck1.airlock.vent" {
        let vent = Vec3::from_array(d.vent_position.map(|v| v as f32));
        surface_block(
            &mut p,
            vent - Vec3::new(0.42, 0.25, 0.04),
            vent + Vec3::new(0.42, 0.25, 0.04),
            steel * 1.5,
            0.,
            SurfaceRole::ExposedAlloy,
        );
        for i in 0..6 {
            let x = vent.x - 0.35 + i as f32 * 0.13;
            surface_block(
                &mut p,
                Vec3::new(x, vent.y - 0.2, vent.z + 0.04),
                Vec3::new(x + 0.045, vent.y + 0.2, vent.z + 0.06),
                steel * 0.3,
                0.,
                SurfaceRole::Composite,
            );
        }
    }
    p.0
}

/// Compatibility helper used by the legacy render fixtures.
#[cfg(test)]
pub(super) fn parts_with_room(
    deck: u8,
    room: Option<&arkline_protocol::PioneerRoomState>,
) -> Vec<ModelPart> {
    let mut parts = parts(deck);
    let Some(s) = room.filter(|_| deck == arkline_protocol::PIONEER_CONNECTED_ROOM.deck) else {
        return parts;
    };
    // Suppress the legacy decorative airlock strips: only actual outputs emit here.
    parts.retain(|(_, m, _, e)| {
        let c = m.w_axis;
        !(*e > 0. && c.x >= 4.2 && c.x <= 13.14 && c.z < 5.9)
    });
    let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
    let mut p = Parts(parts);
    for (point, half, color, on) in [
        (
            d.light_position,
            Vec3::new(1.5, 0.025, 0.16),
            Vec3::new(0.50, 0.65, 0.55),
            s.light_on,
        ),
        (
            d.emergency_position,
            Vec3::new(0.2, 0.08, 0.03),
            Vec3::new(1., 0.55, 0.27),
            s.emergency_light_on,
        ),
        (
            d.breaker_position,
            Vec3::new(0.012, 0.10, 0.12),
            Vec3::new(0.03, 0.6, 0.2),
            s.commissioned && s.breaker_closed,
        ),
        (
            d.vent_position,
            Vec3::new(0.05, 0.03, 0.07),
            Vec3::new(0.03, 0.5, 0.25),
            s.vent_running,
        ),
    ] {
        let center = Vec3::from_array(point.map(|v| v as f32));
        surface_block(
            &mut p,
            center - half,
            center + half,
            if on { color } else { Vec3::splat(0.025) },
            if on { 0.8 } else { 0. },
            SurfaceRole::FixtureDiffuser,
        );
    }
    if !s.door_open {
        surface_block(
            &mut p,
            Vec3::from_array(d.door_min.map(|v| v as f32)),
            Vec3::from_array(d.door_max.map(|v| v as f32)),
            Vec3::new(0.10, 0.14, 0.15),
            0.,
            SurfaceRole::CoatedPanel,
        );
    }
    p.0
}

/// Dynamic airlock outputs are returned separately from cached room geometry.
pub(super) fn state_parts(
    deck: u8,
    room: Option<&arkline_protocol::PioneerRoomState>,
) -> Vec<ModelPart> {
    let Some(s) = room.filter(|_| deck == arkline_protocol::PIONEER_CONNECTED_ROOM.deck) else {
        return Vec::new();
    };
    let mut parts = Vec::new();
    for id in [
        "pioneer.deck1.airlock.light",
        "pioneer.deck1.airlock.emergency",
        "pioneer.deck1.airlock.breaker",
        "pioneer.deck1.airlock.vent",
        "pioneer.deck1.airlock.door",
    ] {
        parts.extend(equipment_output_parts(id, s));
    }
    parts
}

fn equipment_output_parts(id: &str, state: &arkline_protocol::PioneerRoomState) -> Vec<ModelPart> {
    let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
    let mut p = Parts(Vec::new());
    let fixture = match id {
        "pioneer.deck1.airlock.light" => Some((
            d.light_position,
            Vec3::new(1.5, 0.025, 0.16),
            Vec3::new(0.50, 0.65, 0.55),
            state.light_on,
        )),
        "pioneer.deck1.airlock.emergency" => Some((
            d.emergency_position,
            Vec3::new(0.2, 0.08, 0.03),
            Vec3::new(1., 0.55, 0.27),
            state.emergency_light_on,
        )),
        "pioneer.deck1.airlock.breaker" => Some((
            d.breaker_position,
            Vec3::new(0.012, 0.10, 0.12),
            Vec3::new(0.03, 0.6, 0.2),
            state.commissioned && state.breaker_closed,
        )),
        "pioneer.deck1.airlock.vent" => Some((
            d.vent_position,
            Vec3::new(0.05, 0.03, 0.07),
            Vec3::new(0.03, 0.5, 0.25),
            state.vent_running,
        )),
        _ => None,
    };
    if let Some((point, half, color, on)) = fixture {
        let center = Vec3::from_array(point.map(|v| v as f32));
        surface_block(
            &mut p,
            center - half,
            center + half,
            if on { color } else { Vec3::splat(0.025) },
            if on { 0.8 } else { 0. },
            SurfaceRole::FixtureDiffuser,
        );
    }
    if id == "pioneer.deck1.airlock.door" && !state.door_open {
        surface_block(
            &mut p,
            Vec3::from_array(d.door_min.map(|v| v as f32)),
            Vec3::from_array(d.door_max.map(|v| v as f32)),
            Vec3::new(0.10, 0.14, 0.15),
            0.,
            SurfaceRole::CoatedPanel,
        );
    }
    p.0
}

/// Inspect a physical catalog entry using exactly its installed body and output recipes.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn equipment_parts(
    id: &str,
    state: &arkline_protocol::PioneerRoomState,
) -> Option<Vec<ModelPart>> {
    arkline_protocol::pioneer_room_catalog()
        .equipment
        .iter()
        .find(|entry| entry.id == id && entry.position.is_some())?;
    let mut parts = equipment_static_parts(id);
    parts.extend(equipment_output_parts(id, state));
    Some(parts)
}

fn point_in_panel(point: egui::Pos2, quad: &[egui::Pos2; 4]) -> bool {
    let mut positive = false;
    let mut negative = false;
    for i in 0..4 {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let cross = (b.x - a.x) * (point.y - a.y) - (b.y - a.y) * (point.x - a.x);
        positive |= cross > 0.01;
        negative |= cross < -0.01;
    }
    !(positive && negative)
}
fn panel_projection(
    rect: egui::Rect,
    vp: Mat4,
    eye: Vec3,
    pilot: &PilotView,
    room: Option<&arkline_protocol::PioneerRoomState>,
    layout: Option<&RoomLayoutDocument>,
) -> Option<[egui::Pos2; 4]> {
    if !crate::room_controls::panel_access(pilot) || room.is_none() {
        return None;
    }
    let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
    let center = Vec3::from_array(d.breaker_position.map(|v| v as f32)) + Vec3::X * 0.03;
    let default_layout;
    let document = if let Some(document) = layout {
        document
    } else {
        default_layout = fallback_layout_document();
        &default_layout
    };
    let blocked = document.obstacles(1).iter().any(|o| {
        quarters::segment_box(
            eye,
            center,
            Vec3::from_array(o.min.map(|v| v as f32)),
            Vec3::from_array(o.max.map(|v| v as f32)),
        )
        .is_some_and(|t| t < 0.999)
    }) || room.is_some_and(|s| {
        !s.door_open
            && quarters::segment_box(
                eye,
                center,
                Vec3::from_array(d.door_min.map(|v| v as f32)),
                Vec3::from_array(d.door_max.map(|v| v as f32)),
            )
            .is_some_and(|t| t < 0.999)
    });
    if eye.x <= center.x || blocked {
        return None;
    }
    let corners = [
        center + Vec3::new(0., -0.5, -0.35),
        center + Vec3::new(0., 0.5, -0.35),
        center + Vec3::new(0., 0.5, 0.35),
        center + Vec3::new(0., -0.5, 0.35),
    ];
    let mut quad = [egui::Pos2::ZERO; 4];
    for (i, p) in corners.into_iter().enumerate() {
        quad[i] = project_point(vp, rect, p)?;
        if !rect.contains(quad[i]) {
            return None;
        }
    }
    let a = quad[1] - quad[0];
    let b = quad[3] - quad[0];
    if (a.x * b.y - a.y * b.x).abs() < 12. {
        return None;
    }
    Some(quad)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn device_panel(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vp: Mat4,
    eye: Vec3,
    pilot: &PilotView,
    room: Option<&arkline_protocol::PioneerRoomState>,
    layout: Option<&RoomLayoutDocument>,
    live: bool,
    unobstructed: bool,
) -> Option<HangarAction> {
    let quad = panel_projection(rect, vp, eye, pilot, room, layout)?;
    let hovered = unobstructed
        && ui
            .ctx()
            .pointer_hover_pos()
            .is_some_and(|p| point_in_panel(p, &quad));
    let color = if live {
        egui::Color32::from_rgb(60, 220, 140)
    } else {
        egui::Color32::GRAY
    };
    ui.painter_at(rect).add(egui::Shape::closed_line(
        quad.to_vec(),
        egui::Stroke::new(if hovered { 2. } else { 1. }, color),
    ));
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        let top = quad.iter().min_by(|a, b| a.y.total_cmp(&b.y)).unwrap();
        ui.painter_at(rect).text(
            *top - egui::vec2(0., 8.),
            egui::Align2::LEFT_BOTTOM,
            if live {
                "AIRLOCK ENGINEERING · click panel"
            } else {
                "AIRLOCK ENGINEERING · offline"
            },
            egui::FontId::proportional(13.),
            color,
        );
        if live && ui.input(|i| i.pointer.primary_clicked()) {
            return Some(HangarAction::Engineering(
                arkline_protocol::PIONEER_CONNECTED_ROOM.room_id.into(),
            ));
        }
    }
    None
}
#[cfg(test)]
pub(super) fn camera(target: Vec3, desired: Vec3) -> Vec3 {
    camera_in_layout(target, desired, 1, &fallback_layout_document())
}

fn camera_in_layout(target: Vec3, desired: Vec3, deck: u8, document: &RoomLayoutDocument) -> Vec3 {
    let desired = Vec3::new(
        desired.x.clamp(0.25, 31.75),
        desired.y.clamp(0.4, 2.95),
        desired.z.clamp(0.25, 13.75),
    );
    let mut limit = 1.0f32;
    for o in document.obstacles(deck) {
        if let Some(t) = quarters::segment_box(
            target,
            desired,
            Vec3::from_array(o.min.map(|v| v as f32)) - Vec3::splat(0.12),
            Vec3::from_array(o.max.map(|v| v as f32)) + Vec3::splat(0.12),
        ) {
            limit = limit.min((t - 0.03).max(0.0));
        }
    }
    target.lerp(desired, limit)
}

fn fallback_layout_document() -> RoomLayoutDocument {
    arkline_protocol::default_pioneer()
}

#[cfg(test)]
pub(super) fn camera_with_room(
    target: Vec3,
    desired: Vec3,
    deck: u8,
    room: Option<&arkline_protocol::PioneerRoomState>,
) -> Vec3 {
    camera_with_layout(target, desired, deck, None, room)
}

pub(super) fn camera_with_layout(
    target: Vec3,
    desired: Vec3,
    deck: u8,
    layout: Option<&RoomLayoutDocument>,
    room: Option<&arkline_protocol::PioneerRoomState>,
) -> Vec3 {
    let default_layout;
    let document = if let Some(document) = layout {
        document
    } else {
        default_layout = fallback_layout_document();
        &default_layout
    };
    let desired = camera_in_layout(target, desired, deck, document);
    if deck == 1 && room.is_some_and(|state| !state.door_open) {
        let airlock = arkline_protocol::PIONEER_CONNECTED_ROOM;
        if let Some(t) = quarters::segment_box(
            target,
            desired,
            Vec3::from_array(airlock.door_min.map(|v| v as f32)) - Vec3::splat(0.12),
            Vec3::from_array(airlock.door_max.map(|v| v as f32)) + Vec3::splat(0.12),
        ) {
            return target.lerp(desired, (t - 0.03).max(0.));
        }
    }
    desired
}

fn room_at(
    document: &RoomLayoutDocument,
    deck: u8,
    position: [f64; 3],
) -> Option<&arkline_protocol::RoomLayout> {
    document.rooms.iter().find(|room| {
        let [x, z] = room.origin_xz_m;
        let [length, depth, _] = room.clear_lwh_m;
        room.deck == deck
            && position[0] >= x
            && position[0] <= x + length
            && position[2] >= z
            && position[2] <= z + depth
    })
}

pub(super) fn labels(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    vp: Mat4,
    pilot: &PilotView,
    live: bool,
    layout: Option<&RoomLayoutDocument>,
) {
    let Some(a) = pilot.station_avatar.as_ref() else {
        return;
    };
    let pos = Vec3::from_array(a.position.map(|v| v as f32));
    let default_layout;
    let document = if let Some(document) = layout {
        document
    } else {
        default_layout = fallback_layout_document();
        &default_layout
    };
    let room = room_at(document, a.interior_deck, a.position);
    let room_name = room.map_or("ACCESS CORRIDOR", |room| room.name.as_str());
    let source_role = room
        .and_then(|room| room.source_role.as_deref())
        .unwrap_or("");
    let text = if source_role.eq_ignore_ascii_case("POWER CONTROL") {
        pilot
            .power
            .as_ref()
            .map_or("Power telemetry unavailable".into(), |p| {
                format!(
                    "Capacitor {} / {} J · reactor {} W",
                    p.charge_j, p.capacity_j, p.regen_w
                )
            })
    } else if source_role.eq_ignore_ascii_case("CARGO ACCESS") {
        format!(
            "Confirmed cargo {} ore / {} capacity",
            pilot.ore,
            arkline_protocol::cargo_capacity(pilot.ship_class)
        )
    } else {
        "Other fixtures remain proposed. Airlock device status is in Station engineering; lift is in the access core.".into()
    };
    ui.painter().text(
        rect.left_top() + egui::vec2(20.0, 135.0),
        egui::Align2::LEFT_TOP,
        format!(
            "{room_name}\n{} · {text}",
            if live {
                "LIVE SHIP DATA"
            } else {
                "LAST KNOWN DATA"
            }
        ),
        egui::FontId::proportional(14.0),
        egui::Color32::from_rgb(165, 230, 185),
    );
    for room in document
        .rooms
        .iter()
        .filter(|room| room.deck == a.interior_deck)
    {
        let at = Vec3::new(
            room.door_center_xz_m[0] as f32,
            (room.door_clear_wh_m[1] as f32 + 0.2).min(3.05),
            room.door_wall_z() as f32,
        );
        let blocked = document.obstacles(a.interior_deck).iter().any(|o| {
            quarters::segment_box(
                pos + Vec3::Y * 1.6,
                at,
                Vec3::from_array(o.min.map(|v| v as f32)),
                Vec3::from_array(o.max.map(|v| v as f32)),
            )
            .is_some_and(|t| t < 0.98)
        });
        if pos.distance(at) < 8.0 && !blocked {
            if let Some(screen) = project_point(vp, rect, at) {
                ui.painter().text(
                    screen,
                    egui::Align2::CENTER_CENTER,
                    room.name.as_str(),
                    egui::FontId::proportional(12.0),
                    egui::Color32::LIGHT_GREEN,
                );
            }
        }
    }
}
#[cfg(all(test, not(target_arch = "wasm32")))]
mod room_inspection_tests {
    use super::*;

    fn installed_state() -> arkline_protocol::PioneerRoomState {
        let mut state =
            arkline_protocol::PioneerRoomState::uncommissioned("inspection-test".into());
        state.commissioned = true;
        state.breaker_closed = true;
        state.light_on = true;
        state.door_open = false;
        state
    }

    #[test]
    fn every_room_cutaway_is_an_ordered_subset_of_the_installed_deck() {
        let document = arkline_protocol::default_pioneer();
        let state = installed_state();
        assert_eq!(document.rooms.len(), 12);
        for room in &document.rooms {
            let source_id = room.source_room_id.as_deref().unwrap();
            let inspected = inspection_room_parts(source_id).unwrap();
            let mut installed = build_parts(room.deck, &document);
            installed.extend(state_parts(room.deck, Some(&state)));
            let mut cursor = 0;
            for part in &inspected {
                let offset = installed[cursor..]
                    .iter()
                    .position(|candidate| candidate == part)
                    .unwrap_or_else(|| {
                        panic!("{source_id} contains a new or reordered draw: {part:?}")
                    });
                cursor += offset + 1;
                assert!(part.1.is_finite() && part.1.determinant().abs() > 1e-20);
                assert!(part.2.is_finite() && part.3.is_finite());
            }
            // Source roles do not create fictional equipment. Only the connected
            // airlock has installed utilities; five existing deck-one rooms have
            // their already-prototyped storage cabinet and display strip.
            let expected = if source_id == "D1-P1" {
                28
            } else if room.deck == 1 {
                11
            } else {
                9
            };
            assert_eq!(inspected.len(), expected, "{source_id}");
        }
        assert!(inspection_room_parts("unknown-room").is_err());
    }

    #[test]
    fn room_cutaways_omit_shared_slabs_and_access_core_geometry() {
        let mut shared = Parts(Vec::new());
        let steel = Vec3::new(0.045, 0.065, 0.072);
        block(
            &mut shared,
            Vec3::new(0.0, -0.1, 0.0),
            Vec3::new(32.0, 0.0, 14.0),
            steel,
            0.0,
        );
        block(
            &mut shared,
            Vec3::new(0.0, 3.2, 0.0),
            Vec3::new(32.0, 3.3, 14.0),
            steel,
            0.0,
        );
        shared
            .0
            .extend(obstacle_model_parts(arkline_protocol::StationObstacle {
                kind: StationObstacleKind::Storage,
                min: [0.7, 0.0, 6.45],
                max: [1.0, 1.3, 7.55],
            }));
        for room in arkline_protocol::default_pioneer().rooms {
            let inspected = inspection_room_parts(room.source_room_id.as_deref().unwrap()).unwrap();
            assert!(shared.0.iter().all(|part| !inspected.contains(part)));
        }
    }

    #[test]
    fn airlock_cutaway_retains_actual_installed_bodies_conduits_and_outputs() {
        let inspected = inspection_room_parts("D1-P1").unwrap();
        let static_parts = connected_room_static_parts();
        assert!(inspected
            .windows(static_parts.len())
            .any(|parts| parts == static_parts.as_slice()));
        let outputs = state_parts(1, Some(&installed_state()));
        assert!(inspected.ends_with(&outputs));
        assert_eq!(
            outputs.len(),
            5,
            "Four installed outputs and the closed door"
        );
        for source in ["D1-S1", "D2-P1"] {
            let other = inspection_room_parts(source).unwrap();
            assert!(static_parts.iter().all(|part| !other.contains(part)));
        }
    }
}

#[cfg(test)]
mod tests {
    fn panel_pilot() -> PilotView {
        serde_json::from_value(serde_json::json!({"name":"PanelQA","credits":0,"ore":0,"location":Location::Docked,
            "station_avatar":{"position":[6.,0.,3.],"heading":std::f64::consts::PI,"moving":false,"updated_at_ms":0,"onboard_interior":true,"interior_deck":1}})).unwrap()
    }
    #[test]
    fn physical_panel_click_uses_actual_quad_in_both_camera_views() {
        let pilot = panel_pilot();
        let room = arkline_protocol::PioneerRoomState::uncommissioned("owned".into());
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900., 650.));
        for eye in [Vec3::new(6., 1.65, 3.), Vec3::new(8., 2.1, 3.)] {
            let center = Vec3::from_array(
                arkline_protocol::PIONEER_CONNECTED_ROOM
                    .breaker_position
                    .map(|v| v as f32),
            );
            let vp = Mat4::perspective_rh_gl(1., rect.aspect_ratio(), 0.08, 100.)
                * Mat4::look_at_rh(eye, center, Vec3::Y);
            let quad = panel_projection(rect, vp, eye, &pilot, Some(&room), None).unwrap();
            let point = egui::pos2(
                quad.iter().map(|p| p.x).sum::<f32>() / 4.,
                quad.iter().map(|p| p.y).sum::<f32>() / 4.,
            );
            for (live, unobstructed, expected) in [
                (true, true, true),
                (false, true, false),
                (true, false, false),
            ] {
                let ctx = egui::Context::default();
                let mut action = None;
                for pressed in [None, Some(true), Some(false)] {
                    let mut events = vec![egui::Event::PointerMoved(point)];
                    if let Some(pressed) = pressed {
                        events.push(egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        });
                    }
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(rect),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                action = device_panel(
                                    ui,
                                    rect,
                                    vp,
                                    eye,
                                    &pilot,
                                    Some(&room),
                                    None,
                                    live,
                                    unobstructed,
                                )
                                .or(action.take());
                            });
                        },
                    );
                }
                assert_eq!(action.is_some(), expected);
                if let Some(HangarAction::Engineering(id)) = action {
                    assert_eq!(id, arkline_protocol::PIONEER_CONNECTED_ROOM.room_id);
                }
            }
        }
    }
    #[test]
    fn physical_panel_rejects_wrong_deck_distance_wall_and_backface() {
        let mut p = panel_pilot();
        let s = arkline_protocol::PioneerRoomState::uncommissioned("owned".into());
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900., 650.));
        let center = Vec3::new(4.28, 1.4, 3.);
        let project = |eye, p: &PilotView| {
            let vp = Mat4::perspective_rh_gl(1., rect.aspect_ratio(), 0.08, 100.)
                * Mat4::look_at_rh(eye, center, Vec3::Y);
            panel_projection(rect, vp, eye, p, Some(&s), None)
        };
        assert!(project(Vec3::new(6., 1.65, 3.), &p).is_some());
        assert!(project(Vec3::new(6., 1.65, 7.), &p).is_none());
        assert!(project(Vec3::new(3., 1.65, 3.), &p).is_none());
        p.station_avatar.as_mut().unwrap().interior_deck = 2;
        assert!(project(Vec3::new(6., 1.65, 3.), &p).is_none());
        p.station_avatar.as_mut().unwrap().interior_deck = 1;
        p.station_avatar.as_mut().unwrap().position = [12., 0., 3.];
        assert!(project(Vec3::new(6., 1.65, 3.), &p).is_none());
        let quad = [
            egui::pos2(0., 0.),
            egui::pos2(10., 3.),
            egui::pos2(8., 10.),
            egui::pos2(2., 8.),
        ];
        assert!(!point_in_panel(egui::pos2(9., 9.), &quad));
    }
    #[test]
    fn confirmed_device_outputs_control_light_emission_and_door_geometry() {
        let mut s = arkline_protocol::PioneerRoomState::uncommissioned("owned".into());
        let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
        let lamp = Vec3::from_array(d.light_position.map(|v| v as f32));
        let emission = |parts: Vec<ModelPart>| {
            parts
                .into_iter()
                .find(|(_, m, _, _)| m.w_axis.truncate().distance(lamp) < 0.001)
                .unwrap()
                .3
        };
        assert_eq!(emission(parts_with_room(1, Some(&s))), 0.);
        s.light_on = true;
        assert!(emission(parts_with_room(1, Some(&s))) > 0.7);
        let open = parts_with_room(1, Some(&s));
        s.door_open = false;
        assert_eq!(parts_with_room(1, Some(&s)).len(), open.len() + 1);
        let target = Vec3::new(8.66, 1.5, 4.5);
        let eye = Vec3::new(8.66, 1.5, 7.);
        assert!(camera_with_room(target, eye, 1, Some(&s)).z < 5.65);
        assert_eq!(parts_with_room(2, Some(&s)), parts(2));
        s.light_on = false;
        s.emergency_light_on = true;
        let emergency = Vec3::from_array(d.emergency_position.map(|v| v as f32));
        assert!(parts_with_room(1, Some(&s)).iter().any(|(_, m, c, e)| m
            .w_axis
            .truncate()
            .distance(emergency)
            < 0.001
            && c.x > c.y
            && *e > 0.7));
    }
    #[test]
    fn cached_decks_preserve_geometry_and_materials() {
        let document = arkline_protocol::default_pioneer();
        for deck in [1, 2] {
            assert_eq!(super::parts(deck), super::build_parts(deck, &document));
            assert!(Arc::ptr_eq(
                &super::cached_parts(deck),
                &super::cached_parts(deck)
            ));
        }
    }

    #[test]
    fn live_layout_changes_geometry_door_projection_and_cache_identity() {
        let mut document = arkline_protocol::default_pioneer();
        let baseline = super::build_parts(2, &document);
        let room = document.rooms.iter_mut().find(|room| room.id == 8).unwrap();
        room.door_clear_wh_m[0] += 0.35;
        let door_x = room.door_center_xz_m[0] as f32;
        let door_width = room.door_clear_wh_m[0] as f32;
        let positive_jamb_x = door_x + door_width * 0.5;
        let frame_z = room.door_wall_z() as f32 + 0.08;
        assert!(document.validate_live().is_ok());
        let edited = super::build_parts(2, &document);
        assert_ne!(baseline, edited);
        assert!(edited.iter().any(|(_, model, color, emission)| {
            *color == Vec3::new(0.025, 0.35, 0.14)
                && (*emission - 0.5).abs() < f32::EPSILON
                && (model.w_axis.x - positive_jamb_x).abs() < 0.001
                && (model.w_axis.z - frame_z).abs() < 0.001
        }));

        let state = RoomLayoutState {
            revision: 12,
            document: arkline_protocol::default_pioneer(),
        };
        let cache = LayoutPartsCache::new(&state).unwrap();
        assert!(cache.matches(&state));
        let changed_same_revision = RoomLayoutState {
            revision: 12,
            document,
        };
        assert!(!cache.matches(&changed_same_revision));
        let changed_revision = RoomLayoutState {
            revision: 13,
            document: state.document.clone(),
        };
        assert!(!cache.matches(&changed_revision));
        let other_session = LayoutPartsCache::new(&changed_same_revision).unwrap();
        assert_ne!(cache.parts(2).as_ref(), other_session.parts(2).as_ref());
    }

    #[test]
    fn removed_live_room_drops_its_bounds_and_prototype_fixture() {
        let mut document = arkline_protocol::default_pioneer();
        let removed = document.rooms.iter().find(|room| room.id == 2).unwrap();
        let center = [
            removed.origin_xz_m[0] + removed.clear_lwh_m[0] * 0.5,
            0.0,
            removed.origin_xz_m[1] + removed.clear_lwh_m[1] * 0.5,
        ];
        let [x, z] = removed.origin_xz_m;
        let fixture_min = [x + 1.0, 0.0, z + 1.0];
        let fixture_max = [x + 2.8, 1.4, z + 1.8];
        let fixture_center = Vec3::from_array([
            ((fixture_min[0] + fixture_max[0]) * 0.5) as f32,
            ((fixture_min[1] + fixture_max[1]) * 0.5) as f32,
            ((fixture_min[2] + fixture_max[2]) * 0.5) as f32,
        ]);
        let deck = removed.deck;
        let rendered_before = super::build_parts(deck, &document);
        let before = document.obstacles(deck);
        assert!(before
            .iter()
            .any(|obstacle| obstacle.min == fixture_min && obstacle.max == fixture_max));
        assert!(rendered_before.iter().any(|(_, model, color, emission)| {
            *color == Vec3::new(0.09, 0.13, 0.14)
                && *emission == 0.0
                && model.w_axis.truncate().distance(fixture_center) < 0.001
        }));
        document.rooms.retain(|room| room.id != 2);
        assert!(document.validate_live().is_ok());
        assert_eq!(room_at(&document, deck, center), None);
        assert!(!document
            .obstacles(deck)
            .iter()
            .any(|obstacle| obstacle.min == fixture_min && obstacle.max == fixture_max));
        let rendered_after = super::build_parts(deck, &document);
        assert!(!rendered_after.iter().any(|(_, model, color, emission)| {
            *color == Vec3::new(0.09, 0.13, 0.14)
                && *emission == 0.0
                && model.w_axis.truncate().distance(fixture_center) < 0.001
        }));
        let remaining = document.rooms.iter().find(|room| room.id == 3).unwrap();
        let remaining_center = [
            remaining.origin_xz_m[0] + remaining.clear_lwh_m[0] * 0.5,
            0.0,
            remaining.origin_xz_m[1] + remaining.clear_lwh_m[1] * 0.5,
        ];
        assert_eq!(
            room_at(&document, deck, remaining_center).map(|room| room.id),
            Some(3)
        );
        assert!(rendered_after.len() < rendered_before.len());
        let previous = LayoutPartsCache::new(&RoomLayoutState {
            revision: 1,
            document: arkline_protocol::default_pioneer(),
        })
        .unwrap();
        let next = LayoutPartsCache::new(&RoomLayoutState {
            revision: 1,
            document,
        })
        .unwrap();
        assert!(!previous.matches(&RoomLayoutState {
            revision: 1,
            document: next.document().clone(),
        }));
    }
    #[test]
    #[ignore = "Explicit CPU preparation measurement, not a GPU frame-rate benchmark"]
    fn static_deck_preparation_profile() {
        use std::hint::black_box;
        black_box(super::parts(1));
        let document = arkline_protocol::default_pioneer();
        let start = std::time::Instant::now();
        for _ in 0..2000 {
            black_box(super::build_parts(1, &document));
        }
        let rebuilt = start.elapsed();
        let start = std::time::Instant::now();
        for _ in 0..2000 {
            black_box(super::parts(1));
        }
        eprintln!(
            "2000 actual deck preparation calls: rebuild={rebuilt:?}, cached_copy={:?}, parts={}",
            start.elapsed(),
            super::parts(1).len()
        );
    }
    use super::*;
    #[test]
    fn camera_stays_below_ceiling_and_on_same_side_of_bulkhead() {
        let target = Vec3::new(10.0, 1.25, 7.0);
        let eye = camera(target, Vec3::new(10.0, 6.0, 2.0));
        assert!(eye.y <= 2.95 && eye.z > 5.8);
        assert!(!parts(1).is_empty());
    }
}
