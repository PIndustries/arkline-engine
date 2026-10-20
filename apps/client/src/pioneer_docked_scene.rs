//! One negotiated station-metre scene for the docked Pioneer and its walking route.
//! Scene selection is presentation only; authoritative feet and actuator state stay intact.
use super::*;
use arkline_protocol::{
    body_to_station, camera_segment_station, deck_to_body, docked_binding_pose,
    docked_layout_sha256, dynamic_geometry, pilot_joints, registered_geometry,
    DockedTraversalState, PioneerDockedActuators, PioneerDockedBox, PioneerDockedGeometry,
    PioneerDockedMaterial, PioneerDockedPanelKind, PioneerDockedPilotJoints, PioneerDockedPose,
    RoomLayoutDocument,
};

pub(in crate::flight3d) const FOV_RADIANS: f32 = 55.0 * std::f32::consts::PI / 180.0;

#[derive(Clone)]
pub(in crate::flight3d) struct SceneCache {
    document: RoomLayoutDocument,
    geometry: Arc<PioneerDockedGeometry>,
    rooms: Arc<Vec<ModelPart>>,
}
impl SceneCache {
    pub(in crate::flight3d) fn new(document: RoomLayoutDocument) -> Result<Self, String> {
        let geometry = Arc::new(registered_geometry(&document)?);
        let mut rooms = Vec::new();
        for room in &document.rooms {
            let origin = deck_to_body(room.deck, [0.0; 3])
                .ok_or("Room refers to an unregistered Pioneer deck")?;
            let registration = Mat4::from_translation(vector(origin));
            rooms.extend(pioneer_interior::room_shell_parts(room).into_iter().map(
                |(mesh, matrix, color, emission)| (mesh, registration * matrix, color, emission),
            ));
        }
        Ok(Self {
            document,
            geometry,
            rooms: Arc::new(rooms),
        })
    }
    pub(in crate::flight3d) fn matches(&self, document: &RoomLayoutDocument) -> bool {
        self.document == *document
    }
}

/// One immutable body-metre recipe for station and flight presentation. Only
/// the caller's rigid body-to-view transform changes between environments.
pub(in crate::flight3d) fn body_parts(
    cache: &SceneCache,
    dynamic: &PioneerDockedGeometry,
) -> Result<Vec<ModelPart>, String> {
    let source = docked_pioneer_parts()?;
    let mut parts = source
        .iter()
        .map(|&(mesh, model, color, emission)| {
            (
                mesh,
                Mat4::from_scale(Vec3::splat(100.0)) * model,
                color,
                emission,
            )
        })
        .collect::<Vec<_>>();
    parts.extend(panel_parts(&cache.geometry, Mat4::IDENTITY, true));
    parts.extend(panel_parts(dynamic, Mat4::IDENTITY, false));
    parts.extend(cache.rooms.iter().copied());
    Ok(parts)
}

pub(in crate::flight3d) fn seated_body_parts(
    cache: &SceneCache,
    room: Option<&arkline_protocol::PioneerRoomState>,
    now: u64,
) -> Result<Arc<Vec<ModelPart>>, String> {
    let pose = PioneerDockedPose::private();
    let dynamic = dynamic_geometry(pose, actuator_pose(None, pose, room, now))?;
    body_parts(cache, &dynamic).map(Arc::new)
}

pub(in crate::flight3d) fn default_body_parts() -> Result<Arc<Vec<ModelPart>>, String> {
    static PARTS: std::sync::OnceLock<Result<Arc<Vec<ModelPart>>, String>> =
        std::sync::OnceLock::new();
    PARTS
        .get_or_init(|| {
            let cache = SceneCache::new(arkline_protocol::default_pioneer())?;
            seated_body_parts(&cache, None, 0)
        })
        .clone()
}

/// The same real backing plane and opaque-scene ray guard in every view.
pub(in crate::flight3d) fn physical_surface(
    rect: egui::Rect,
    vp: Mat4,
    mount: Mat4,
    index: usize,
    canvas: egui::Vec2,
    scene: Arc<CockpitOcclusion>,
) -> Option<CockpitSurface> {
    if index == 0 {
        return None;
    }
    let (center, half) = arkline_protocol::cockpit_screen(index)?;
    CockpitSurface::registered(rect, vp, mount, vector(center), vector(half), index, canvas)
        .map(|surface| surface.with_registered_occlusion(scene, vp, mount))
}

fn vector(point: [f64; 3]) -> Vec3 {
    Vec3::from_array(point.map(|value| value as f32))
}
fn body_frame(pose: PioneerDockedPose) -> Mat4 {
    Mat4::from_rotation_translation(
        Quat::from_rotation_y(-pose.yaw as f32),
        vector(pose.origin_m),
    )
}
fn box_matrix(bounds: &PioneerDockedBox) -> Mat4 {
    Mat4::from_cols(
        (vector(bounds.axes[0]) * bounds.half_m[0] as f32).extend(0.0),
        (vector(bounds.axes[1]) * bounds.half_m[1] as f32).extend(0.0),
        (vector(bounds.axes[2]) * bounds.half_m[2] as f32).extend(0.0),
        vector(bounds.center_m).extend(1.0),
    )
}
fn surface(material: PioneerDockedMaterial) -> (Vec3, f32) {
    match material {
        PioneerDockedMaterial::FittedBulkhead => (Vec3::new(0.46, 0.43, 0.35), 0.0),
        PioneerDockedMaterial::WallPanel => (Vec3::new(0.36, 0.34, 0.28), 0.0),
        PioneerDockedMaterial::GripDeck => (Vec3::new(0.018, 0.024, 0.026), 0.0),
        PioneerDockedMaterial::ServiceCeiling => (Vec3::new(0.022, 0.029, 0.033), 0.0),
        PioneerDockedMaterial::Graphite => (Vec3::new(0.045, 0.065, 0.072), 0.0),
        PioneerDockedMaterial::Titanium => (Vec3::new(0.17, 0.22, 0.23), 0.0),
        PioneerDockedMaterial::Carbon => (Vec3::new(0.018, 0.024, 0.027), 0.0),
        PioneerDockedMaterial::Emerald => (Vec3::new(0.025, 0.28, 0.15), 0.0),
        PioneerDockedMaterial::WorkingLight => (Vec3::new(0.40, 0.55, 0.45), 0.9),
        PioneerDockedMaterial::Amber => (Vec3::new(0.40, 0.26, 0.035), 0.35),
        PioneerDockedMaterial::Glass => (Vec3::new(0.012, 0.055, 0.075), 0.0),
    }
}
/// Semantic protocol materials retain one explicit response in every view.
fn panel_surface_role(material: PioneerDockedMaterial) -> surface_roles::SurfaceRole {
    use surface_roles::SurfaceRole;
    match material {
        PioneerDockedMaterial::FittedBulkhead => SurfaceRole::ArchitecturalPanel,
        PioneerDockedMaterial::WallPanel => SurfaceRole::CoatedPanel,
        PioneerDockedMaterial::GripDeck => SurfaceRole::GripDeck,
        PioneerDockedMaterial::ServiceCeiling => SurfaceRole::ServiceCeiling,
        PioneerDockedMaterial::Graphite => SurfaceRole::CoatedPanel,
        PioneerDockedMaterial::Titanium => SurfaceRole::ExposedAlloy,
        PioneerDockedMaterial::Carbon => SurfaceRole::Composite,
        PioneerDockedMaterial::Emerald => SurfaceRole::EnamelTrim,
        PioneerDockedMaterial::WorkingLight => SurfaceRole::FixtureDiffuser,
        PioneerDockedMaterial::Amber => SurfaceRole::EnamelTrim,
        PioneerDockedMaterial::Glass => SurfaceRole::InstrumentGlass,
    }
}
fn panel_parts(
    geometry: &PioneerDockedGeometry,
    frame: Mat4,
    skip_rooms: bool,
) -> impl Iterator<Item = ModelPart> + '_ {
    geometry
        .panels
        .iter()
        .filter(move |panel| {
            panel.collision != arkline_protocol::PioneerDockedCollision::TraversalBarrier
                && (!skip_rooms || panel.kind != PioneerDockedPanelKind::RoomObstacle)
        })
        .flat_map(move |panel| {
            if panel.material == PioneerDockedMaterial::FittedBulkhead {
                let center = vector(panel.bounds.center_m);
                let half = vector(panel.bounds.half_m);
                pioneer_interior::architectural_bulkhead_parts(
                    center - half,
                    center + half,
                    arkline_protocol::deck_floor_y(2).unwrap() as f32,
                    Vec3::new(0.46, 0.43, 0.35),
                    panel.id == "cockpit/rear-bulkhead/0" || panel.id == "connector/wall/-1",
                )
                .into_iter()
                .map(|(mesh, model, color, emission)| (mesh, frame * model, color, emission))
                .collect::<Vec<_>>()
            } else {
                let (color, emission) = surface(panel.material);
                vec![(
                    panel_surface_role(panel.material).cube(),
                    frame * box_matrix(&panel.bounds),
                    color,
                    emission,
                )]
            }
        })
}

fn actuator_pose(
    state: Option<&DockedTraversalState>,
    pose: PioneerDockedPose,
    room: Option<&arkline_protocol::PioneerRoomState>,
    now: u64,
) -> PioneerDockedActuators {
    let upper = deck_to_body(2, [0.0; 3]).expect("Registered upper deck");
    PioneerDockedActuators {
        hatch_fractions: state.map(|state| state.actuators.hatch_fractions()),
        room_door_open: room.is_some_and(|room| room.door_open),
        inner_hatch_open: state.is_some_and(|state| state.actuators.inner_hatch_open),
        outer_hatch_open: state.is_some_and(|state| state.actuators.outer_hatch_open),
        lift_floor_station_m: state.map_or_else(
            || body_to_station(pose, upper)[1],
            |state| state.actuators.lift.floor_station_m,
        ),
        chair_fold: state.map_or(0.0, |state| {
            state
                .seat_transition
                .as_ref()
                .map_or(1.0, |phase| phase.chair_fold(now))
        }),
    }
}
fn standing(state: Option<&DockedTraversalState>, now: u64) -> f64 {
    state.map_or(0.0, |state| {
        state
            .seat_transition
            .as_ref()
            .map_or(1.0, |phase| phase.standing_fraction(now))
    })
}
fn pose_for(
    state: Option<&DockedTraversalState>,
    fleet: Option<&arkline_protocol::HangarView>,
) -> Result<PioneerDockedPose, String> {
    if let Some(state) = state {
        docked_binding_pose(&state.binding)
    } else {
        Ok(fleet.map_or_else(PioneerDockedPose::private, |fleet| {
            PioneerDockedPose::fleet(fleet.layout_revision)
        }))
    }
}
fn rotate_offset(offset: Vec3, heading: f64) -> Vec3 {
    Quat::from_rotation_y(-heading as f32) * offset
}
fn eye_at(joints: &PioneerDockedPilotJoints, feet: Vec3, heading: f64) -> Vec3 {
    feet + rotate_offset(vector(joints.eye_m) - vector(joints.root_m), heading)
}

/// All rendered joints and the first-person eye use the exact same feet root.
fn pilot_parts(joints: &PioneerDockedPilotJoints, feet: Vec3, heading: f64) -> Vec<ModelPart> {
    let root = vector(joints.root_m);
    let point = |point| feet + rotate_offset(vector(point) - root, heading);
    let frame = Mat4::from_rotation_translation(Quat::from_rotation_y(-heading as f32), feet);
    let suit = Vec3::new(0.055, 0.09, 0.105);
    let armor = Vec3::new(0.11, 0.16, 0.17);
    let mut parts = Vec::new();
    let hips = (point(joints.hips_m[0]) + point(joints.hips_m[1])) * 0.5;
    let neck = point(joints.neck_m);
    if let Some(model) = segment_model(hips, neck, 0.14) {
        parts.push((0, model, suit, 0.0));
    }
    parts.push((
        0,
        Mat4::from_translation(point(joints.head_m))
            * Mat4::from_quat(Quat::from_rotation_y(-heading as f32))
            * Mat4::from_scale(Vec3::new(0.10, 0.12, 0.10)),
        armor,
        0.0,
    ));
    for side in 0..2 {
        for (a, b, radius) in [
            (
                point(joints.hips_m[side]),
                point(joints.knees_m[side]),
                0.075,
            ),
            (
                point(joints.knees_m[side]),
                point(joints.feet_m[side]),
                0.06,
            ),
            (
                point(joints.shoulders_m[side]),
                point(joints.elbows_m[side]),
                0.05,
            ),
            (
                point(joints.elbows_m[side]),
                point(joints.hands_m[side]),
                0.045,
            ),
        ] {
            if let Some(model) = segment_model(a, b, radius) {
                parts.push((0, model, suit, 0.0));
            }
        }
        let local = vector(joints.feet_m[side]) - root + Vec3::new(0.025, 0.035, 0.0);
        parts.push((
            0,
            frame * Mat4::from_translation(local) * Mat4::from_scale(Vec3::new(0.11, 0.035, 0.07)),
            armor,
            0.0,
        ));
    }
    parts
}

fn camera_eye(
    geometry: &PioneerDockedGeometry,
    dynamic: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    target: Vec3,
    desired: Vec3,
) -> Vec3 {
    let array = |point: Vec3| point.to_array().map(f64::from);
    let fraction =
        camera_segment_station(geometry, dynamic, pose, array(target), array(desired), 0.12)
            .unwrap_or(1.0);
    let distance = target.distance(desired) as f64;
    target.lerp(
        desired,
        (fraction - if distance > 0.0 { 0.02 / distance } else { 0.0 }).clamp(0.0, 1.0) as f32,
    )
}

fn scene_parts(
    cache: &SceneCache,
    dynamic: &PioneerDockedGeometry,
    pose: PioneerDockedPose,
    fleet: Option<&arkline_protocol::HangarView>,
    eye: Vec3,
    host_id: Option<&str>,
) -> Result<Vec<ModelPart>, String> {
    let body = body_frame(pose);
    let mut parts = if let Some(fleet) = fleet {
        let hidden = host_id.map_or_else(Vec::new, |id| vec![id]);
        fleet_hangar::parts_except_hulls(fleet, eye, &hidden)
    } else {
        // Keep the existing metre-scale bay/quarters source and replace only its
        // original Pioneer hull with the shared, locally clipped source recipe.
        let mut parts = bay_parts(ShipClass::Pioneer);
        parts.retain(|part| embedded_primitive(part.0).is_none());
        parts.extend(quarters::room_parts(ShipClass::Pioneer, 0));
        parts
    };
    parts.extend(
        body_parts(cache, dynamic)?
            .into_iter()
            .map(|(mesh, model, color, emission)| (mesh, body * model, color, emission)),
    );
    Ok(parts)
}

fn registered_surfaces(
    view: &mut HangarView,
    rect: egui::Rect,
    vp: Mat4,
    pose: PioneerDockedPose,
    parts: &[ModelPart],
) {
    let catalog = view
        .renderer
        .as_ref()
        .and_then(|renderer| {
            renderer
                .lock()
                .ok()
                .map(|renderer| renderer.depth_meshes.clone())
        })
        .or_else(|| {
            flight_geometry()
                .ok()
                .map(|geometry| geometry.catalog.clone())
        });
    let Some(catalog) = catalog else {
        return;
    };
    let opaque = parts
        .iter()
        .filter(|part| docked_source_primitive(part.0) != Some(6))
        .map(|part| cockpit_occlusion::DepthDraw::whole(&catalog, part.0, part.1))
        .collect::<Option<Vec<_>>>();
    let Some(opaque) = opaque else {
        return;
    };
    let Some(scene) =
        CockpitOcclusion::new(ShipClass::Pioneer, catalog, opaque, Vec::new()).map(Arc::new)
    else {
        return;
    };
    let mount = body_frame(pose);
    view.docked_surfaces = std::array::from_fn(|index| {
        // Slot zero was a canopy HUD, without a proved physical backing.
        if index == 0 {
            return None;
        }
        let canvas = if index == 3 {
            cockpit_computer::COMPUTER_CANVAS_SIZE
        } else if index == 5 {
            cockpit_radar::RADAR_CANVAS_SIZE
        } else {
            egui::vec2(300.0, 180.0)
        };
        let mut surface = physical_surface(rect, vp, mount, index, canvas, scene.clone())?;
        if let Some(renderer) = &view.docked_display_renderer {
            surface = surface.with_renderer(renderer.clone());
        }
        Some(surface)
    });
}

/// Draw filtering uses this view's own projection. All candidates remain in the
/// shared static/dynamic collision/input scene, including offscreen wall faces.
unsafe fn paint_scene(
    renderer: &mut Renderer,
    vp: Mat4,
    eye: Vec3,
    parts: &[ModelPart],
    now: u64,
    cull: bool,
    scene_prepare_ns: u64,
) {
    #[cfg(not(target_arch = "wasm32"))]
    let preparation = std::time::Instant::now();
    let mut selected = Vec::with_capacity(parts.len());
    let mut lights = Vec::new();
    #[cfg(not(target_arch = "wasm32"))]
    let mut visibility = ViewVisibility::default();
    for part in parts {
        let keep = !cull || part_in_view(&renderer.depth_meshes, vp, part);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let triangles = renderer.meshes[mesh_index(part.0)].count as u64 / 3;
            visibility.candidate_draws += 1;
            visibility.candidate_triangles += triangles;
            if keep {
                visibility.submitted_draws += 1;
                visibility.submitted_triangles += triangles;
            }
        }
        if keep {
            selected.push(*part);
        }
        if mesh_index(part.0) == 0 && part.3 >= 0.7 && part.2.min_element() > 0.25 {
            lights.push(local_lighting::Light {
                position: part.1.w_axis.truncate(),
                range: 30.0,
                color: part.2,
                power: 2.5,
            });
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        visibility.cpu_prepare_ns = scene_prepare_ns
            .saturating_add(preparation.elapsed().as_nanos().min(u64::MAX as u128) as u64);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let profile = renderer.begin_view_profile("docked", now);
    #[cfg(target_arch = "wasm32")]
    let _ = (now, scene_prepare_ns);
    let opaque = selected
        .iter()
        .copied()
        .filter(|part| docked_source_primitive(part.0) != Some(6))
        .collect::<Vec<_>>();
    paint_with_fixture_lighting(renderer, vp, eye, None, &opaque, true, &[], Some(&lights));
    for part in selected
        .iter()
        .filter(|part| docked_source_primitive(part.0) == Some(6))
    {
        renderer.draw_registered_glass(part);
    }
    #[cfg(not(target_arch = "wasm32"))]
    renderer.finish_view_profile(profile, visibility);
    #[cfg(test)]
    assert_eq!(
        renderer.gl.get_error(),
        glow::NO_ERROR,
        "Registered Pioneer docked callback"
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn show(
    view: &mut HangarView,
    ui: &mut egui::Ui,
    pilot: &PilotView,
    now: u64,
    live: bool,
    fleet: Option<&arkline_protocol::HangarView>,
    room: Option<&arkline_protocol::PioneerRoomState>,
) -> Option<HangarAction> {
    view.clear_cockpit_presentation();
    let (rect, response) =
        ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
    view.surface = Some((rect, ui.layer_id()));
    let state = pilot
        .station_avatar
        .as_ref()
        .and_then(|avatar| avatar.docked_traversal.as_ref());
    let document = view
        .room_layout
        .as_ref()
        .map(|layout| layout.document.clone())
        .unwrap_or_else(arkline_protocol::default_pioneer);
    let prepare = || -> Result<(PioneerDockedPose, SceneCache), String> {
        if let Some(state) = state {
            if state.version != arkline_protocol::DOCKED_TRAVERSAL_VERSION
                || state.binding.layout_sha256 != docked_layout_sha256(&document)
            {
                return Err("Docked scene is waiting for its matching registered layout".into());
            }
        }
        Ok((pose_for(state, fleet)?, SceneCache::new(document.clone())?))
    };
    if view
        .docked_scene
        .as_ref()
        .is_none_or(|cache| cache.document != document)
    {
        match prepare() {
            Ok((_, cache)) => view.docked_scene = Some(cache),
            Err(error) => {
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    error,
                    egui::FontId::proportional(15.0),
                    egui::Color32::LIGHT_RED,
                );
                return None;
            }
        }
    }
    let pose = match pose_for(state, fleet) {
        Ok(pose) => pose,
        Err(error) => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(15.0),
                egui::Color32::LIGHT_RED,
            );
            return None;
        }
    };
    if state.is_some_and(|state| state.binding.layout_sha256 != docked_layout_sha256(&document)) {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Docked scene is waiting for its matching registered layout",
            egui::FontId::proportional(15.0),
            egui::Color32::LIGHT_RED,
        );
        return None;
    }
    let mut joints = pilot_joints(standing(state, now));
    let feet = state.map_or_else(
        || vector(body_to_station(pose, joints.root_m)),
        |state| vector(state.feet_m),
    );
    let heading = state.map_or(pose.yaw, |state| state.heading);
    let transition = state.is_none() || state.is_some_and(|state| state.seat_transition.is_some());
    let dt = ui.input(|input| input.stable_dt).clamp(0.0, 0.1);
    if !view.docked_initialized {
        view.docked_initialized = true;
        view.on_foot_view = OnFootView::FirstPerson;
        view.yaw = heading as f32;
        view.pitch = 0.0;
        view.zoom = 1.0;
        view.follow = Some(feet);
        view.follow_heading = Some(heading);
        view.head_look = [0.0; 2];
        view.gait = Default::default();
    }
    view.walking = state.is_some();
    let focused = ui.input(|input| input.focused);
    let hovered = focused
        && ui.ctx().pointer_hover_pos().is_some_and(|point| {
            rect.contains(point) && ui.ctx().layer_id_at(point) == Some(ui.layer_id())
        });
    let dragging =
        !view.editor_free_camera_active && response.dragged_by(egui::PointerButton::Secondary);
    if hovered && !view.editor_free_camera_active && view.on_foot_view == OnFootView::ThirdPerson {
        let scroll = ui.input(|input| input.smooth_scroll_delta.y);
        if scroll.is_finite() {
            view.zoom = (view.zoom * (-scroll * 0.002).exp()).clamp(0.45, 2.5);
        }
    }
    if hovered && dragging {
        let delta = ui.input(|input| {
            if input.pointer.any_pressed() {
                egui::Vec2::ZERO
            } else {
                input.pointer.delta()
            }
        });
        view.yaw += delta.x * 0.007;
        view.pitch = (view.pitch + delta.y * 0.005).clamp(-1.25, 1.25);
    }
    let allowed = hovered
        && !view.computer_keyboard_owned
        && view.head_look_input_allowed
        && !view.editor_free_camera_active
        && !ui.ctx().wants_keyboard_input()
        && ui.input(|input| !input.pointer.any_down() && !input.pointer.any_pressed());
    if allowed {
        let (held, axes) = ui.input(|input| {
            (
                pilot.station_avatar.is_none() && input.modifiers.shift,
                [
                    (input.key_down(egui::Key::ArrowLeft) as i32
                        - input.key_down(egui::Key::ArrowRight) as i32) as f32,
                    (input.key_down(egui::Key::ArrowUp) as i32
                        - input.key_down(egui::Key::ArrowDown) as i32) as f32,
                ],
            )
        });
        update_head_look(&mut view.head_look, held, axes, dt);
    }
    let smooth = if transition {
        feet
    } else {
        view.follow
            .unwrap_or(feet)
            .lerp(feet, 1.0 - (-dt * 18.0).exp())
    };
    view.follow = Some(smooth);
    view.follow_heading = Some(avatar_follow::heading(view.follow_heading, heading, dt));
    let body_heading = view.follow_heading.unwrap();
    let gait = view.gait.update(
        now,
        smooth,
        body_heading,
        live && state.is_some_and(|state| state.moving && state.grounded) && !transition,
        transition,
    );
    if state.is_some_and(|state| state.grounded) && !transition {
        gait.apply_registered(&mut joints);
    }
    let target = eye_at(&joints, smooth, body_heading);
    let act = actuator_pose(state, pose, room, now);
    let dynamic = match dynamic_geometry(pose, act) {
        Ok(dynamic) => dynamic,
        Err(error) => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(15.0),
                egui::Color32::LIGHT_RED,
            );
            return None;
        }
    };
    let cache = view.docked_scene.as_ref().unwrap();
    let first_person =
        view.on_foot_view == OnFootView::FirstPerson && !view.editor_free_camera_active;
    let yaw = body_heading as f32 - view.head_look[0];
    let pitch = (-view.pitch + view.head_look[1]).clamp(-1.45, 1.45);
    let direction = editor_camera_direction(yaw, pitch);
    let mut eye = if first_person {
        target
    } else {
        let desired = third_person_eye(target, body_heading as f32, view.pitch, 5.5 * view.zoom);
        camera_eye(&cache.geometry, &dynamic, pose, target, desired)
    };
    let mut look = if first_person {
        eye + direction
    } else {
        target
    };
    if let Some((position, yaw, pitch)) = view.editor_free_camera {
        eye = position;
        look = eye + editor_camera_direction(yaw, pitch);
    }
    let actual = (look - eye).normalize_or_zero();
    view.last_camera_pose = Some((
        eye,
        actual.z.atan2(actual.x),
        actual.y.clamp(-1.0, 1.0).asin(),
    ));
    let vp = view_projection(eye, look, rect.aspect_ratio());
    let host = state
        .map(|state| state.binding.host_vessel_id.as_str())
        .or_else(|| {
            fleet.and_then(|fleet| {
                fleet
                    .berths
                    .iter()
                    .find(|berth| berth.active)
                    .map(|berth| berth.id.as_str())
            })
        });
    #[cfg(not(target_arch = "wasm32"))]
    let scene_preparation = std::time::Instant::now();
    let mut parts = match scene_parts(cache, &dynamic, pose, fleet, eye, host) {
        Ok(parts) => parts,
        Err(error) => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                error,
                egui::FontId::proportional(15.0),
                egui::Color32::LIGHT_RED,
            );
            return None;
        }
    };
    if should_render_avatar(first_person, view.editor_free_camera_active, eye, smooth) {
        parts.extend(pilot_parts(&joints, smooth, body_heading));
    }
    if !view.editor_free_camera_active {
        registered_surfaces(view, rect, vp, pose, &parts);
        view.docked_layer = Some(ui.layer_id());
    }
    #[cfg(not(target_arch = "wasm32"))]
    let scene_prepare_ns = scene_preparation.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    #[cfg(target_arch = "wasm32")]
    let scene_prepare_ns = 0;
    #[cfg(test)]
    let cull = !view.docked_disable_culling;
    #[cfg(not(test))]
    let cull = true;
    if let Some(renderer) = &view.renderer {
        let renderer = renderer.clone();
        #[cfg(target_arch = "wasm32")]
        let renderer = send_wrapper::SendWrapper::new(renderer);
        ui.painter()
            .with_clip_rect(rect.intersect(ui.clip_rect()))
            .add(egui::PaintCallback {
                rect,
                callback: Arc::new(egui_glow::CallbackFn::new(move |_, _| {
                    if let Ok(mut renderer) = renderer.lock() {
                        unsafe {
                            paint_scene(
                                &mut renderer,
                                vp,
                                eye,
                                &parts,
                                now,
                                cull,
                                scene_prepare_ns,
                            );
                        }
                    }
                })),
            });
    }
    ui.painter().text(
        rect.left_top() + egui::vec2(14.0, 12.0),
        egui::Align2::LEFT_TOP,
        if state.is_none() {
            "PIONEER · PILOT SEAT · Shift + arrows: look"
        } else if live {
            "PIONEER · DOCKED · W/S move · RMB turn"
        } else {
            "PIONEER · OFFLINE · movement paused"
        },
        egui::FontId::proportional(13.0),
        egui::Color32::from_rgb(145, 190, 175),
    );
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(view: &mut HangarView, ctx: &egui::Context, pilot: &PilotView, now: u64, time: f64) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 720.0),
                )),
                time: Some(time),
                focused: false,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    view.ui(ui, pilot, now, true, None, None, None, now)
                });
            },
        );
    }
    fn pilot(state: Option<arkline_protocol::DockedTraversalState>) -> PilotView {
        let mut pilot:PilotView=serde_json::from_value(serde_json::json!({"name":"DockedCameraQA","credits":0,"ore":0,"location":Location::Docked})).unwrap();
        pilot.station_avatar = state.map(|state| StationAvatar {
            sprinting: false,
            docked_traversal: Some(state),
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: [10000.0, 0.0, 10000.0],
            heading: 2.4,
            moving: false,
            updated_at_ms: 1000,
        });
        pilot
    }
    fn standing_state() -> arkline_protocol::DockedTraversalState {
        let document = arkline_protocol::default_pioneer();
        let pose = PioneerDockedPose::private();
        let binding = arkline_protocol::docked_traversal_binding(
            "camera-fixture".into(),
            &document,
            arkline_protocol::FleetLayoutRevision::LinearV1,
            false,
        );
        arkline_protocol::DockedTraversalState {
            sprinting: false,
            version: arkline_protocol::DOCKED_TRAVERSAL_VERSION,
            binding: binding.clone(),
            feet_m: arkline_protocol::seat_anchors(pose).root_station_m,
            heading: pose.yaw,
            support: arkline_protocol::DockedSupport::Cockpit,
            surface_id: "camera-fixture".into(),
            moving: false,
            grounded: true,
            vertical_velocity_mps: 0.0,
            updated_at_ms: 1000,
            seat_transition: None,
            actuators: arkline_protocol::DockedVesselActuators {
                hatches: None,
                binding,
                inner_hatch_open: false,
                outer_hatch_open: false,
                lift: arkline_protocol::DockedLiftState {
                    floor_station_m: arkline_protocol::lift_geometry(pose).ship_floor_m,
                    target: arkline_protocol::DockedLiftStop::Ship,
                    moving: false,
                    updated_at_ms: 1000,
                },
                service_power_available: true,
            },
        }
    }
    #[test]
    fn registered_docked_camera_preserves_pose_ownership_across_region_flags_and_seat_joints() {
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(|| {
                let ctx = egui::Context::default();
                let mut view = HangarView::new(None);
                view.set_docked_traversal_enabled(true);
                let seated = pilot(None);
                frame(&mut view, &ctx, &seated, 1000, 0.0);
                let pose = PioneerDockedPose::private();
                let expected = vector(arkline_protocol::seat_anchors(pose).eye_station_m);
                assert!(view.editor_camera_pose().unwrap().0.distance(expected) < 1e-4);
                view.yaw = 0.3;
                view.pitch = -0.15;
                view.zoom = 1.4;
                view.head_look = [0.2, -0.1];
                view.set_head_look_input_allowed(false);
                let mut time = 0.1;
                for elapsed in [0, 225, 450, 675, 900] {
                    let mut state = standing_state();
                    state.seat_transition = Some(arkline_protocol::DockedSeatTransition {
                        phase: arkline_protocol::DockedSeatPhase::Leaving,
                        started_at_ms: 1000,
                        ends_at_ms: 1900,
                    });
                    let fraction = state
                        .seat_transition
                        .as_ref()
                        .unwrap()
                        .standing_fraction(1000 + elapsed);
                    let feet = vector(state.feet_m);
                    let mut actor = pilot(Some(state));
                    let avatar = actor.station_avatar.as_mut().unwrap();
                    avatar.onboard_interior = elapsed % 2 == 0;
                    avatar.interior_deck = if elapsed < 450 { 1 } else { 2 };
                    let source = serde_json::to_vec(&actor).unwrap();
                    frame(&mut view, &ctx, &actor, 1000 + elapsed, time);
                    time += 0.1;
                    assert_eq!((view.yaw, view.pitch, view.zoom), (0.3, -0.15, 1.4));
                    assert_eq!(view.head_look_angles(), [0.2, -0.1]);
                    assert!(
                        view.editor_camera_pose().unwrap().0.distance(eye_at(
                            &pilot_joints(fraction),
                            feet,
                            pose.yaw
                        )) < 1e-4
                    );
                    assert_eq!(serde_json::to_vec(&actor).unwrap(), source);
                }
                let mut state = standing_state();
                state.feet_m = body_to_station(pose, [18.0, -4.12, 0.0]);
                state.support = arkline_protocol::DockedSupport::Deck1;
                let actor = pilot(Some(state));
                frame(&mut view, &ctx, &actor, 2100, time);
                assert_eq!((view.yaw, view.pitch, view.zoom), (0.3, -0.15, 1.4));
                assert_eq!(view.head_look_angles(), [0.2, -0.1]);
                frame(&mut view, &ctx, &seated, 2200, time + 0.1);
                assert!(view.editor_camera_pose().unwrap().0.distance(expected) < 1e-4);
                assert_eq!((view.yaw, view.pitch, view.zoom), (0.3, -0.15, 1.4));
            });
            let _ = sender.send(result);
        });
        if let Err(panic) = receiver
            .recv_timeout(std::time::Duration::from_secs(15))
            .expect("Registered camera frames must finish without egui context reentry")
        {
            std::panic::resume_unwind(panic);
        }
    }
    #[test]
    fn registered_box_adapter_keeps_orthonormal_frame_and_exact_body_metre_extents() {
        let document = arkline_protocol::default_pioneer();
        let geometry = registered_geometry(&document).unwrap();
        let pose = PioneerDockedPose {
            origin_m: [200.0, 13.0, -150.0],
            yaw: 0.7,
        };
        let frame = body_frame(pose);
        for panel in &geometry.panels {
            let matrix = box_matrix(&panel.bounds);
            assert!(matrix.is_finite());
            assert!(
                matrix
                    .transform_point3(Vec3::ZERO)
                    .distance(vector(panel.bounds.center_m))
                    < 1e-4
            );
            for axis in 0..3 {
                let local =
                    Vec3::from_array(std::array::from_fn(|i| if i == axis { 1.0 } else { 0.0 }));
                let expected = vector(panel.bounds.center_m)
                    + vector(panel.bounds.axes[axis]) * panel.bounds.half_m[axis] as f32;
                assert!(matrix.transform_point3(local).distance(expected) < 1e-4);
                let world = frame.transform_point3(expected);
                let reference = vector(body_to_station(pose, expected.to_array().map(f64::from)));
                assert!(world.distance(reference) < 1e-3);
            }
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "pioneer_docked_gpu_tests.rs"]
mod native_tests;
