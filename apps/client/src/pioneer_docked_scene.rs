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
    immutable_parts: Arc<Vec<ModelPart>>,
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
        let source = docked_pioneer_parts()?;
        let mut immutable_parts = source
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
        immutable_parts.extend(panel_parts(&geometry, Mat4::IDENTITY, true));
        Ok(Self {
            document,
            geometry,
            immutable_parts: Arc::new(immutable_parts),
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
    let mut parts = cache.immutable_parts.as_ref().clone();
    parts.extend(panel_parts(dynamic, Mat4::IDENTITY, false));
    parts.extend(cache.rooms.iter().copied());
    Ok(parts)
}

pub(in crate::flight3d) fn seated_body_parts(
    cache: &SceneCache,
    room: Option<&arkline_protocol::PioneerRoomState>,
    now: u64,
) -> Result<Arc<Vec<ModelPart>>, String> {
    seated_body_parts_with_mechanisms(cache, room, now, [0.0; 7])
}

/// The caller samples the confirmed actuator state once and supplies exactly
/// these fractions to both this mesh recipe and the physical content planes.
pub(in crate::flight3d) fn seated_body_parts_with_mechanisms(
    cache: &SceneCache,
    room: Option<&arkline_protocol::PioneerRoomState>,
    now: u64,
    fractions: [f64; 7],
) -> Result<Arc<Vec<ModelPart>>, String> {
    let pose = PioneerDockedPose::private();
    let dynamic = dynamic_with_mechanisms(pose, actuator_pose(None, pose, room, now), fractions)?;
    body_parts(cache, &dynamic).map(Arc::new)
}

fn dynamic_with_mechanisms(
    pose: PioneerDockedPose,
    actuators: PioneerDockedActuators,
    fractions: [f64; 7],
) -> Result<PioneerDockedGeometry, String> {
    let mut dynamic = dynamic_geometry(pose, actuators)?;
    let mechanisms = arkline_protocol::cockpit_mechanism_geometry(pose, fractions)?;
    dynamic.panels.extend(mechanisms.panels);
    dynamic.supports.extend(mechanisms.supports);
    dynamic.portals.extend(mechanisms.portals);
    Ok(dynamic)
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
#[cfg(test)]
pub(in crate::flight3d) fn physical_surface(
    rect: egui::Rect,
    vp: Mat4,
    mount: Mat4,
    index: usize,
    canvas: egui::Vec2,
    scene: Arc<CockpitOcclusion>,
) -> Option<CockpitSurface> {
    physical_surface_with_mechanisms(rect, vp, mount, index, canvas, scene, [0.0; 7])
}

#[allow(clippy::too_many_arguments)]
pub(in crate::flight3d) fn physical_surface_with_mechanisms(
    rect: egui::Rect,
    vp: Mat4,
    mount: Mat4,
    index: usize,
    canvas: egui::Vec2,
    scene: Arc<CockpitOcclusion>,
    fractions: [f64; 7],
) -> Option<CockpitSurface> {
    if index == 0 {
        return None;
    }
    if let Some(slot) = index.checked_sub(8) {
        let fraction = *fractions.get(slot)?;
        let (center, half, axes) = arkline_protocol::cockpit_mechanism_screen(slot, fraction)?;
        let center = vector(center);
        let half = vector(half);
        let plane_mount = oriented_plane_mount(mount, center, axes);
        let canvas = egui::vec2(canvas.y * (half.z - 0.012) / (half.y - 0.012), canvas.y);
        return CockpitSurface::registered_mechanism(
            rect,
            vp,
            plane_mount,
            center,
            half,
            index,
            canvas,
        )
        .map(|surface| surface.with_registered_occlusion(scene, vp, plane_mount));
    }
    let (center, half) = arkline_protocol::cockpit_screen(index)?;
    let plane_mount = screen_plane_mount(index, mount, vector(center))?;
    let canvas = canonical_screen_canvas(index, vector(half), canvas);
    CockpitSurface::registered(
        rect,
        vp,
        plane_mount,
        vector(center),
        vector(half),
        index,
        canvas,
    )
    .map(|surface| surface.with_registered_occlusion(scene, vp, plane_mount))
}

// Preserve the physical proportions of text, charts and camera tiles on the
// rebuilt peripheral glass. The central computer keeps its existing UI contract.
fn canonical_screen_canvas(index: usize, half: Vec3, requested: egui::Vec2) -> egui::Vec2 {
    if matches!(index, 1 | 2 | 4 | 5 | 6 | 7) {
        egui::vec2(
            requested.y * (half.z - 0.008) / (half.y - 0.008),
            requested.y,
        )
    } else {
        requested
    }
}

// Mount the content and pointer plane in the canonical physical screen axes.
// Camera matrices, GPU programs and world authority are not changed here.
fn screen_plane_mount(index: usize, mount: Mat4, center: Vec3) -> Option<Mat4> {
    let axes = arkline_protocol::cockpit_screen_axes(index)?;
    Some(oriented_plane_mount(mount, center, axes))
}

fn oriented_plane_mount(mount: Mat4, center: Vec3, axes: [[f64; 3]; 3]) -> Mat4 {
    let rotation = Mat4::from_cols(
        vector(axes[0]).extend(0.0),
        vector(axes[1]).extend(0.0),
        vector(axes[2]).extend(0.0),
        glam::Vec4::W,
    );
    mount * Mat4::from_translation(center) * rotation * Mat4::from_translation(-center)
}

fn vector(point: [f64; 3]) -> Vec3 {
    Vec3::from_array(point.map(|value| value as f32))
}
pub(in crate::flight3d) fn body_frame(pose: PioneerDockedPose) -> Mat4 {
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
/// Symmetric presentation cubes preserve the exact protocol envelope while
/// keeping outward face winding for either handedness of authored box axes.
fn outward_box_matrix(bounds: &PioneerDockedBox) -> Mat4 {
    let matrix = box_matrix(bounds);
    if matrix.determinant() < 0.0 {
        matrix * Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0))
    } else {
        matrix
    }
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
/// Fitted skins stay inside the authoritative boxes. Leaf-local details inherit
/// the received actuator transform in station, walking and flight views alike.
fn airlock_finish(id: &str, bounds: &PioneerDockedBox, frame: Mat4) -> Vec<ModelPart> {
    use surface_roles::SurfaceRole;
    let mut parts = Vec::new();
    // Hatch axes are left-handed. Symmetric box bounds allow reversing local
    // X without changing the occupied envelope; fitted cube faces then retain
    // their outward winding and normals instead of exposing their back faces.
    let handedness = vector(bounds.axes[0])
        .cross(vector(bounds.axes[1]))
        .dot(vector(bounds.axes[2]))
        .signum();
    let mount = frame
        * box_matrix(bounds)
        * Mat4::from_scale(vector(bounds.half_m).recip())
        * Mat4::from_scale(Vec3::new(handedness, 1.0, 1.0));
    let h = vector(bounds.half_m);
    let graphite = Vec3::new(0.035, 0.052, 0.056);
    let alloy = Vec3::new(0.30, 0.36, 0.36);
    let accent = if id.contains("inner") {
        Vec3::new(0.035, 0.25, 0.15)
    } else {
        Vec3::new(0.48, 0.31, 0.08)
    };
    let mut block = |center: Vec3, half: Vec3, color: Vec3, role: SurfaceRole| {
        parts.push((
            role.cube(),
            mount * Mat4::from_scale_rotation_translation(half, Quat::IDENTITY, center),
            color,
            0.0,
        ));
    };
    block(
        Vec3::ZERO,
        Vec3::new(h.x - 0.0005, h.y - 0.0005, 0.020),
        graphite,
        SurfaceRole::Composite,
    );
    if id.contains("-leaf/") {
        // Both faces are finished, with deep panel joints rather than painted glow.
        for face in [-1.0, 1.0] {
            let z = face * (h.z - 0.020);
            for (y, height) in [(-0.43, 0.67), (0.73, 0.36)] {
                block(
                    Vec3::new(0.0, y, z),
                    Vec3::new(h.x - 0.027, height, 0.009),
                    alloy,
                    SurfaceRole::ExposedAlloy,
                );
            }
            block(
                Vec3::new(0.0, 0.245, face * (h.z - 0.007)),
                Vec3::new(h.x - 0.04, 0.045, 0.005),
                accent,
                SurfaceRole::EnamelTrim,
            );
            // Flush vertical stiffeners and recessed latch plate follow each leaf.
            for side in [-1.0, 1.0] {
                block(
                    Vec3::new(side * (h.x - 0.058), -0.42, face * (h.z - 0.006)),
                    Vec3::new(0.012, 0.58, 0.004),
                    alloy,
                    SurfaceRole::ExposedAlloy,
                );
            }
            block(
                Vec3::new(0.0, -0.10, face * (h.z - 0.006)),
                Vec3::new(0.045, 0.15, 0.004),
                graphite,
                SurfaceRole::CoatedPanel,
            );
            block(
                Vec3::new(0.0, 0.74, face * (h.z - 0.006)),
                Vec3::new(0.21, 0.075, 0.002),
                graphite,
                SurfaceRole::CoatedPanel,
            );
            // Small durable stencilled side identifiers, not simulated indicators.
            let word = if id.contains("inner") {
                "CABIN"
            } else {
                "LIFT"
            };
            let cell = 0.018;
            let width = (word.len() * 4 - 1) as f32 * cell;
            for (letter, ch) in word.chars().enumerate() {
                let rows = match ch {
                    'C' => [7, 4, 4, 4, 7],
                    'A' => [2, 5, 7, 5, 5],
                    'B' => [6, 5, 6, 5, 6],
                    'I' => [7, 2, 2, 2, 7],
                    'N' => [5, 7, 7, 7, 5],
                    'L' => [4, 4, 4, 4, 7],
                    'F' => [7, 4, 6, 4, 4],
                    'T' => [7, 2, 2, 2, 2],
                    _ => [0; 5],
                };
                for (row, bits) in rows.into_iter().enumerate() {
                    for col in 0..3 {
                        if bits & (1 << (2 - col)) != 0 {
                            let x = -width * 0.5 + (letter * 4 + col) as f32 * cell + cell * 0.5;
                            block(
                                Vec3::new(
                                    face * x,
                                    0.74 + (2.0 - row as f32) * cell,
                                    face * (h.z - 0.0015),
                                ),
                                Vec3::new(cell * 0.43, cell * 0.43, 0.001),
                                Vec3::new(0.72, 0.76, 0.69),
                                SurfaceRole::EnamelTrim,
                            );
                        }
                    }
                }
            }
        }
    } else {
        // Frame caps and inset seals never narrow the doorway or create a sill.
        let face_half = Vec3::new((h.x - 0.008).max(0.008), (h.y - 0.008).max(0.008), 0.006);
        for face in [-1.0, 1.0] {
            block(
                Vec3::new(0.0, 0.0, face * (h.z - 0.008)),
                face_half,
                alloy,
                SurfaceRole::ExposedAlloy,
            );
            if id.contains("/jamb/") {
                block(
                    Vec3::new(0.0, 0.36, face * (h.z - 0.0015)),
                    Vec3::new(h.x - 0.012, 0.22, 0.001),
                    accent,
                    SurfaceRole::EnamelTrim,
                );
            }
        }
    }
    parts
}

/// Recess construction into existing solid envelopes; never move the interior
/// boundary into legal avatar space. Same material/role as the original box.
fn recessed_enclosure_parts(
    panel: &arkline_protocol::PioneerDockedPanel,
    frame: Mat4,
) -> Option<Vec<ModelPart>> {
    let thin = match panel.id.as_str() {
        "cockpit/ceiling" => 1,
        "cockpit/canopy/header" | "cockpit/canopy/sill" => 0,
        _ => return None,
    };
    let half = vector(panel.bounds.half_m);
    let face: Vec<_> = (0..3).filter(|axis| *axis != thin).collect();
    let mount = frame * box_matrix(&panel.bounds) * Mat4::from_scale(half.recip());
    let (color, emission) = surface(panel.material);
    let mesh = panel_surface_role(panel.material).cube();
    let mut parts = Vec::new();
    let mut block = |center, half| {
        parts.push((
            mesh,
            mount * Mat4::from_scale_rotation_translation(half, Quat::IDENTITY, center),
            color,
            emission,
        ))
    };
    // Continuous backing closes every coffer; 35 mm maximum recess is away
    // from the room, in the positive local thin-axis direction.
    let recess = 0.035_f32.min(half[thin] * 0.7);
    let mut center = Vec3::ZERO;
    let mut backing = half;
    center[thin] = recess * 0.5;
    backing[thin] -= recess * 0.5;
    block(center, backing);
    for axis in face {
        let bays = ((half[axis] * 2.0 / 1.4).ceil() as usize).max(1);
        let rib_half = 0.030_f32.min(half[axis] * 0.1);
        for index in 0..=bays {
            let mut center = Vec3::ZERO;
            let mut rib = half;
            rib[axis] = rib_half;
            center[axis] = (-half[axis] + index as f32 * half[axis] * 2.0 / bays as f32)
                .clamp(-half[axis] + rib_half, half[axis] - rib_half);
            block(center, rib);
        }
    }
    Some(parts)
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
                // The exact solid remains the collision proxy. Canonical
                // recessed visual strips replace only this full glass face.
                && !arkline_protocol::cockpit_panel_has_visual_replacement(&panel.id)
                && (!skip_rooms || panel.kind != PioneerDockedPanelKind::RoomObstacle)
        })
        .flat_map(move |panel| {
            if panel.id.starts_with("boarding/inner-leaf/")
                || panel.id.starts_with("boarding/outer-leaf/")
                || panel.id.starts_with("boarding/inner/jamb/")
                || panel.id.starts_with("boarding/outer/jamb/")
                || panel.id == "boarding/inner/header"
                || panel.id == "boarding/outer/header"
            {
                airlock_finish(&panel.id, &panel.bounds, frame)
            } else if let Some(parts) = recessed_enclosure_parts(panel, frame) {
                parts
            } else if panel.material == PioneerDockedMaterial::FittedBulkhead {
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
                    frame * outward_box_matrix(&panel.bounds),
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
    fractions: [f64; 7],
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
        let mut surface = physical_surface_with_mechanisms(
            rect, vp, mount, index, canvas, scene.clone(), fractions,
        )?;
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
    let automatic_mouse_look = state.is_some()
        && state.is_some_and(|state| state.seat_transition.is_none())
        && view.on_foot_view == OnFootView::FirstPerson
        && !view.editor_free_camera_active;
    let mouse_delta = view.begin_first_person_mouse(ui, rect, automatic_mouse_look);
    if automatic_mouse_look {
        view.yaw += mouse_delta.x * 0.007;
        view.pitch = (view.pitch + mouse_delta.y * 0.005).clamp(-1.25, 1.25);
    }
    if hovered && dragging && !automatic_mouse_look {
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
    // Sitting confirms the seat's body frame. Discard the previous walking
    // steering target so the next seat exit cannot resurrect an old look/turn.
    // This does not initialize or relocate the camera, avatar or binding.
    if state.is_none() {
        view.yaw = heading as f32;
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
    // One confirmed, freshness-gated sample supplies the rendered mechanisms,
    // third-person camera collision and every content/pointer plane this frame.
    view.mechanism_host_bound = mechanism_host_matches(state, view.mechanism_pose.state());
    let fractions = if view.mechanism_host_bound {
        view.mechanism_pose.fractions()
    } else {
        [0.; 7]
    };
    let dynamic = match dynamic_with_mechanisms(pose, act, fractions) {
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
    // Mouse look is local presentation, not an acknowledgement of body motion.
    // Keep the physical avatar/eye anchor authoritative; only an on-foot,
    // settled first-person direction follows the already sampled local input.
    // Seat transitions and seated head-look retain the body frame.
    let look_heading = if first_person && state.is_some() && !transition {
        view.yaw
    } else {
        body_heading as f32
    };
    let yaw = look_heading - view.head_look[0];
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
    let mut parts = match if pilot.location != Location::Docked {
        body_parts(cache, &dynamic).map(|parts| {
            parts
                .into_iter()
                .map(|(mesh, model, color, emission)| {
                    (mesh, body_frame(pose) * model, color, emission)
                })
                .collect()
        })
    } else {
        scene_parts(cache, &dynamic, pose, fleet, eye, host)
    } {
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
        registered_surfaces(view, rect, vp, pose, &parts, fractions);
        // Leaving the pilot station revokes its active computer surface even
        // when the new lower console happens to remain in the walking view.
        if state.is_some() {
            // Retain a paint-only face without exposing any computer input,
            // hit-testing or command surface to a walking avatar.
            view.passive_console = view.docked_surfaces[3].take();
        }
        view.docked_layer = Some(ui.layer_id());
    }
    view.finish_first_person_mouse(ui, rect);
    #[cfg(not(target_arch = "wasm32"))]
    let scene_prepare_ns = scene_preparation.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    #[cfg(target_arch = "wasm32")]
    let scene_prepare_ns = 0;
    #[cfg(test)]
    let cull = !view.docked_disable_culling;
    #[cfg(not(test))]
    let cull = true;
    let space_frame = view.space_snapshot.as_ref().and_then(|snapshot| {
        moving_interior_frame(
            snapshot,
            pilot,
            now,
            live,
            rect.aspect_ratio(),
            pose,
            eye,
            look,
            &parts,
        )
        .ok()
    });
    view.mechanism_feed_frame = if view.mechanism_host_bound {
        space_frame.clone()
    } else {
        None
    };
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
                            if let Some(frame) = &space_frame {
                                renderer.paint(frame, None);
                            } else {
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
                    }
                })),
            });
    }
    ui.painter().text(
        rect.left_top() + egui::vec2(72.0, 64.0),
        egui::Align2::LEFT_TOP,
        if state.is_none() {
            ""
        } else if live && pilot.location != Location::Docked {
            "PIONEER INTERIOR · AUTOPILOT · W/S move · RMB turn"
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

fn mechanism_host_matches(
    traversal: Option<&DockedTraversalState>,
    mechanisms: Option<&arkline_protocol::PioneerCockpitMechanisms>,
) -> bool {
    mechanisms.is_some_and(|mechanisms| {
        traversal.is_none_or(|state| state.binding.host_vessel_id == mechanisms.vessel_id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mechanism_state_is_never_borrowed_from_owner_while_visiting_another_host() {
        let state = standing_state();
        let own = arkline_protocol::PioneerCockpitMechanisms::stowed("another-ship".into(), 1000);
        assert!(!mechanism_host_matches(Some(&state), Some(&own)));
        let host = arkline_protocol::PioneerCockpitMechanisms::stowed(
            state.binding.host_vessel_id.clone(),
            1000,
        );
        assert!(mechanism_host_matches(Some(&state), Some(&host)));
        assert!(mechanism_host_matches(None, Some(&own)));
        assert!(!mechanism_host_matches(Some(&state), None));
        assert!(!mechanism_host_matches(None, None));
    }
    #[test]
    fn mechanism_mesh_recipe_uses_exact_shared_fractions_and_stowed_wrapper() {
        let cache = SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let pose = PioneerDockedPose::private();
        let actuators = actuator_pose(None, pose, None, 0);
        let base = dynamic_geometry(pose, actuators).unwrap();
        for fractions in [
            [0.; 7],
            [0.5; 7],
            [1.; 7],
            [0., 0.2, 0.4, 0.6, 0.8, 1., 0.3],
        ] {
            let mut expected = base.clone();
            let mechanisms = arkline_protocol::cockpit_mechanism_geometry(pose, fractions).unwrap();
            expected.panels.extend(mechanisms.panels);
            expected.supports.extend(mechanisms.supports);
            expected.portals.extend(mechanisms.portals);
            let actual = dynamic_with_mechanisms(pose, actuators, fractions).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(
                seated_body_parts_with_mechanisms(&cache, None, 0, fractions)
                    .unwrap()
                    .as_ref(),
                &body_parts(&cache, &expected).unwrap()
            );
            // Geometry remains in body metres: placement is applied exactly
            // once later, by both on-foot meshes and their registered planes.
            for pose in [
                PioneerDockedPose::fleet(arkline_protocol::FleetLayoutRevision::LinearV1),
                PioneerDockedPose::fleet(arkline_protocol::FleetLayoutRevision::CircularV1),
            ] {
                assert_eq!(
                    arkline_protocol::cockpit_mechanism_geometry(pose, fractions).unwrap(),
                    arkline_protocol::cockpit_mechanism_geometry(
                        PioneerDockedPose::private(),
                        fractions
                    )
                    .unwrap()
                );
            }
        }
        assert_eq!(
            seated_body_parts(&cache, None, 0).unwrap().as_ref(),
            seated_body_parts_with_mechanisms(&cache, None, 0, [0.; 7])
                .unwrap()
                .as_ref()
        );
        for invalid in [f64::NAN, -0.01, 1.01] {
            assert!(seated_body_parts_with_mechanisms(&cache, None, 0, [invalid; 7]).is_err());
        }
    }

    #[test]
    fn mechanism_planes_match_shared_glass_pointer_depth_and_proportions() {
        let geometry = flight_geometry().unwrap();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 720.));
        for fraction in [0., 0.5, 1.] {
            let fractions = [fraction; 7];
            let mechanisms = arkline_protocol::cockpit_mechanism_geometry(
                PioneerDockedPose::private(),
                fractions,
            )
            .unwrap();
            for pose in [
                PioneerDockedPose::private(),
                PioneerDockedPose::fleet(arkline_protocol::FleetLayoutRevision::CircularV1),
            ] {
                let mount = body_frame(pose);
                let draws = panel_parts(&mechanisms, mount, false)
                    .into_iter()
                    .map(|part| {
                        cockpit_occlusion::DepthDraw::whole(&geometry.catalog, part.0, part.1)
                            .unwrap()
                    })
                    .collect();
                let scene = Arc::new(
                    CockpitOcclusion::new(
                        ShipClass::Pioneer,
                        geometry.catalog.clone(),
                        draws,
                        Vec::new(),
                    )
                    .unwrap(),
                );
                for slot in 0..7 {
                    let (center, half, axes) =
                        arkline_protocol::cockpit_mechanism_screen(slot, fraction).unwrap();
                    let center = vector(center);
                    let target = mount.transform_point3(center);
                    let eye = mount.transform_point3(center - vector(axes[0]) * 1.2);
                    let vp =
                        Mat4::perspective_rh_gl(FOV_RADIANS, viewport.aspect_ratio(), 0.02, 100.)
                            * Mat4::look_at_rh(eye, target, Vec3::Y);
                    let surface = physical_surface_with_mechanisms(
                        viewport,
                        vp,
                        mount,
                        slot + 8,
                        egui::vec2(300., 180.),
                        scene.clone(),
                        fractions,
                    )
                    .unwrap();
                    let canvas = surface.canvas_rect();
                    assert!(
                        (canvas.aspect_ratio()
                            - (half[2] as f32 - 0.012) / (half[1] as f32 - 0.012))
                            .abs()
                            < 1e-5
                    );
                    for (u, v) in [
                        (0.05, 0.05),
                        (0.5, 0.5),
                        (0.95, 0.95),
                        (0.05, 0.95),
                        (0.95, 0.05),
                    ] {
                        let point = canvas.min + canvas.size() * egui::vec2(u, v);
                        let projected = surface.project(point).unwrap();
                        let glass = mount.transform_point3(
                            center - vector(axes[0]) * 0.044
                                + vector(axes[1]) * ((0.5 - v) * 2. * (half[1] as f32 - 0.012))
                                + vector(axes[2]) * ((u - 0.5) * 2. * (half[2] as f32 - 0.012)),
                        );
                        let clip = vp * glass.extend(1.);
                        let ndc = clip.truncate() / clip.w;
                        let expected = viewport.min
                            + egui::vec2(
                                (ndc.x * 0.5 + 0.5) * viewport.width(),
                                (0.5 - ndc.y * 0.5) * viewport.height(),
                            );
                        assert!(
                            projected.distance(expected) < 0.03,
                            "slot {slot}, fraction {fraction}"
                        );
                        assert!(surface.unproject(projected).is_some_and(|hit| hit.distance(point) < 0.1),
                            "slot {slot}, fraction {fraction}, point {point:?}: content must remain in front of its own glass/case");
                    }
                    let square = egui::Rect::from_min_size(canvas.min, egui::vec2(1., 1.));
                    let (_, a, b) = surface.feed_geometry(square).unwrap();
                    assert!(((b.y - a.y).abs() - (b.z - a.z).abs()).abs() < 1e-5);
                    assert!(physical_surface_with_mechanisms(
                        viewport,
                        vp,
                        mount,
                        15,
                        canvas.size(),
                        scene.clone(),
                        fractions
                    )
                    .is_none());
                }
            }
        }
    }

    #[test]
    fn cached_body_parts_preserve_legacy_order_and_live_actuators() {
        let cache = SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let cloned = cache.clone();
        assert!(Arc::ptr_eq(&cache.immutable_parts, &cloned.immutable_parts));
        let pose = PioneerDockedPose::private();
        let closed = actuator_pose(None, pose, None, 0);
        let mut moved = actuator_pose(None, pose, None, 0);
        moved.hatch_fractions = Some([0.35, 0.8]);
        moved.room_door_open = true;
        moved.lift_floor_station_m -= 1.0;
        moved.chair_fold = 1.0;
        let mut outputs = Vec::new();
        for actuators in [closed, moved] {
            let dynamic = dynamic_geometry(pose, actuators).unwrap();
            let source = docked_pioneer_parts().unwrap();
            let mut expected = source
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
            expected.extend(panel_parts(&cache.geometry, Mat4::IDENTITY, true));
            let prefix_len = expected.len();
            assert_eq!(cache.immutable_parts.as_ref(), &expected);
            expected.extend(panel_parts(&dynamic, Mat4::IDENTITY, false));
            let dynamic_end = expected.len();
            expected.extend(cache.rooms.iter().copied());
            let actual = body_parts(&cache, &dynamic).unwrap();
            assert_eq!(actual, expected);
            outputs.push(actual[prefix_len..dynamic_end].to_vec());
        }
        assert_ne!(outputs[0], outputs[1]);
    }

    #[test]
    fn immutable_body_cache_is_bound_to_the_complete_layout_document() {
        let document = arkline_protocol::default_pioneer();
        let cache = SceneCache::new(document.clone()).unwrap();
        assert!(cache.matches(&document));
        let mut edited = document.clone();
        edited.rooms[0].name.push_str(" revised");
        assert!(!cache.matches(&edited));
        let replacement = SceneCache::new(edited.clone()).unwrap();
        assert!(replacement.matches(&edited));
        assert!(!replacement.matches(&document));
        assert!(!Arc::ptr_eq(
            &cache.immutable_parts,
            &replacement.immutable_parts
        ));
    }

    #[test]
    fn pioneer_completion_registered_panel_faces_keep_outward_winding_and_exact_collision_envelopes(
    ) {
        let cache = SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let pose = PioneerDockedPose::private();
        let dynamic = dynamic_geometry(pose, actuator_pose(None, pose, None, 0)).unwrap();
        let mut corrected = Vec::new();
        for panel in cache.geometry.panels.iter().chain(&dynamic.panels) {
            let original = box_matrix(&panel.bounds);
            let fitted = outward_box_matrix(&panel.bounds);
            assert!(fitted.determinant() > 0.0, "{}", panel.id);
            if original.determinant() < 0.0 {
                corrected.push(panel.id.as_str());
            }
            let old_corners: Vec<_> = [-1.0, 1.0]
                .into_iter()
                .flat_map(|x| {
                    [-1.0, 1.0].into_iter().flat_map(move |y| {
                        [-1.0, 1.0]
                            .into_iter()
                            .map(move |z| original.transform_point3(Vec3::new(x, y, z)))
                    })
                })
                .collect();
            for x in [-1.0, 1.0] {
                for y in [-1.0, 1.0] {
                    for z in [-1.0, 1.0] {
                        let corner = fitted.transform_point3(Vec3::new(x, y, z));
                        // Compare actual metre-space vertices. Inverting micrometre-thick
                        // label boxes amplifies f32 cancellation and loses physical units.
                        assert!(
                            old_corners
                                .iter()
                                .any(|old| (*old - corner).abs().max_element() <= 0.00001),
                            "{}: occupied envelope changed",
                            panel.id
                        );
                    }
                }
            }
        }
        assert!(corrected.contains(&"vestibule/floor"));
        assert!(corrected.iter().any(|id| id.starts_with("vestibule/wall/")));
        println!("Outward-winding presentation correction preserves bounds for: {corrected:?}");
    }

    #[test]
    fn airlock_finish_stays_inside_authoritative_frame_and_moving_leaf_boxes() {
        let cache = SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let mut boxes = cache
            .geometry
            .panels
            .iter()
            .filter(|p| p.id.starts_with("boarding/inner/") || p.id.starts_with("boarding/outer/"))
            .map(|p| (p.id.clone(), p.bounds.clone()))
            .collect::<Vec<_>>();
        for hatch in [
            arkline_protocol::PioneerDockedHatch::Inner,
            arkline_protocol::PioneerDockedHatch::Outer,
        ] {
            for step in 0..=20 {
                for (leaf, bounds) in arkline_protocol::hatch_leaf_bounds(hatch, step as f64 / 20.0)
                    .unwrap()
                    .into_iter()
                    .enumerate()
                {
                    boxes.push((
                        format!(
                            "boarding/{}-leaf/{leaf}",
                            if hatch == arkline_protocol::PioneerDockedHatch::Inner {
                                "inner"
                            } else {
                                "outer"
                            }
                        ),
                        bounds,
                    ));
                }
            }
        }
        assert!(boxes.len() >= 90);
        for (id, bounds) in boxes {
            let inverse = box_matrix(&bounds).inverse();
            for (_, model, _, emission) in airlock_finish(&id, &bounds, Mat4::IDENTITY) {
                assert!(
                    model.determinant() > 0.0,
                    "{id}: fitted faces must keep outward winding"
                );
                assert_eq!(
                    emission, 0.0,
                    "Static finish must not fabricate powered telemetry"
                );
                for x in [-1.0, 1.0] {
                    for y in [-1.0, 1.0] {
                        for z in [-1.0, 1.0] {
                            let local = (inverse * model).transform_point3(Vec3::new(x, y, z));
                            assert!(local.abs().max_element() <= 1.0001, "{id}: finish protrudes beyond canonical collision/sweep envelope: {local:?}");
                        }
                    }
                }
            }
        }
    }

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
    fn onfoot_surfaces_use_observed_pose_not_independent_ui_clock() {
        let mut pilot = pilot(Some(standing_state()));
        pilot.ship_class = ShipClass::Pioneer;
        let mut mechanisms = arkline_protocol::PioneerCockpitMechanisms::stowed(
            pilot
                .station_avatar
                .as_ref()
                .unwrap()
                .docked_traversal
                .as_ref()
                .unwrap()
                .binding
                .host_vessel_id
                .clone(),
            1000,
        );
        for screen in &mut mechanisms.screens {
            screen.target_deployed = true;
            screen.duration_ms = 1200;
        }
        pilot.cockpit_mechanisms = Some(mechanisms);
        let snapshot: Snapshot = serde_json::from_value(serde_json::json!({
            "server_time_ms":1000,"pilots":[pilot],"orders":[],"belt_ore":0,
            "ledger_sequence":0,"ledger_hash":"onfoot-mechanism-fixture"
        }))
        .unwrap();
        let pilot = &snapshot.pilots[0];
        let ctx = egui::Context::default();
        let mut view = HangarView::new(None);
        view.set_docked_traversal_enabled(true);
        view.on_foot_view = OnFootView::FirstPerson;
        view.observe_mechanisms(&snapshot, &pilot.name, true, 1600);
        frame(&mut view, &ctx, pilot, 1600, 0.);
        assert_eq!(view.mechanism_pose.fractions(), [0.5; 7]);
        assert!(view.mechanism_host_bound);
        let half = view.docked_surfaces[10].as_ref().unwrap().bounds();
        frame(&mut view, &ctx, pilot, 2200, 5.);
        assert_eq!(view.mechanism_pose.fractions(), [0.5; 7]);
        assert_eq!(view.docked_surfaces[10].as_ref().unwrap().bounds(), half);
        view.observe_mechanisms(&snapshot, &pilot.name, true, 2200);
        frame(&mut view, &ctx, pilot, 2200, 6.);
        assert_eq!(view.mechanism_pose.fractions(), [1.; 7]);
        assert_ne!(view.docked_surfaces[10].as_ref().unwrap().bounds(), half);
        view.disconnect_mechanisms();
        view.observe_mechanisms(&snapshot, &pilot.name, false, 2400);
        frame(&mut view, &ctx, pilot, 2400, 7.);
        assert_eq!(view.mechanism_pose.fractions(), [1.; 7]);
        assert!(
            view.mechanism_feed_frame.is_none(),
            "Docked cameras cannot invent a space snapshot"
        );
        view.reset_mechanisms();
        frame(&mut view, &ctx, pilot, 2400, 8.);
        assert_eq!(view.mechanism_pose.fractions(), [0.; 7]);
        assert!(!view.mechanism_host_bound);
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
                assert_eq!(
                    (view.yaw, view.pitch, view.zoom),
                    (pose.yaw as f32, -0.15, 1.4)
                );
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
    fn registered_first_person_look_does_not_wait_for_body_turn_acknowledgement() {
        let ctx = egui::Context::default();
        let mut view = HangarView::new(None);
        view.set_docked_traversal_enabled(true);
        let actor = pilot(Some(standing_state()));
        let source = serde_json::to_vec(&actor).unwrap();
        frame(&mut view, &ctx, &actor, 1000, 0.0);
        let initial = view.editor_camera_pose().unwrap();
        view.yaw += std::f32::consts::FRAC_PI_2;
        let expected = view.yaw;
        // Same authoritative snapshot: no simulated turn or transport reply.
        frame(&mut view, &ctx, &actor, 1000, 0.016);
        let current = view.editor_camera_pose().unwrap();
        let error = (current.1 - expected + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        assert!(
            error.abs() < 1e-4,
            "Local look must be visible in the next frame"
        );
        assert!(
            current.0.distance(initial.0) < 1e-4,
            "Looking cannot move the eye anchor"
        );
        assert_eq!(serde_json::to_vec(&actor).unwrap(), source);
        view.on_foot_view = OnFootView::ThirdPerson;
        frame(&mut view, &ctx, &actor, 1000, 0.032);
        let third = view.editor_camera_pose().unwrap();
        let heading = actor
            .station_avatar
            .as_ref()
            .unwrap()
            .docked_traversal
            .as_ref()
            .unwrap()
            .heading as f32;
        let error = (third.1 - heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        assert!(
            error.abs() < 1e-4,
            "Third person must still follow authoritative body heading"
        );
    }
    #[test]
    fn registered_seat_cycle_does_not_restore_an_old_walking_look_target() {
        let ctx = egui::Context::default();
        let mut view = HangarView::new(None);
        view.set_docked_traversal_enabled(true);
        let actor = pilot(Some(standing_state()));
        frame(&mut view, &ctx, &actor, 1000, 0.0);
        view.yaw += std::f32::consts::FRAC_PI_2;
        frame(&mut view, &ctx, &actor, 1000, 0.016);
        frame(&mut view, &ctx, &pilot(None), 1000, 0.032);
        // Same authoritative standing direction after leaving the forward seat.
        frame(&mut view, &ctx, &actor, 1000, 0.048);
        let heading = actor
            .station_avatar
            .as_ref()
            .unwrap()
            .docked_traversal
            .as_ref()
            .unwrap()
            .heading;
        assert!(
            view.walk_turn(heading).abs() < 1e-6,
            "Seat exit must not send turn toward the old walking look"
        );
        let yaw = view.editor_camera_pose().unwrap().1;
        assert!((yaw - heading as f32).abs() < 1e-4);
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

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
#[path = "pioneer_docked_gpu_tests.rs"]
mod native_tests;

#[cfg(test)]
mod composition_fit_tests {
    use super::*;

    #[test]
    fn mechanism_travel_grid_keeps_pocket_occlusion_pointer_and_fixed_canvas_coherent() {
        let eye = vector(pilot_joints(0.).eye_m);
        let cache = SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let geometry = flight_geometry().unwrap();
        let mut canvases = [None; 7];
        let mut partial_hidden = 0;
        let mut total = 0;
        for fraction in [0., 0.25, 0.5, 0.75, 1.] {
            let fractions = [fraction; 7];
            let parts = seated_body_parts_with_mechanisms(&cache, None, 0, fractions).unwrap();
            let opaque: Vec<_> = parts
                .iter()
                .filter(|part| docked_source_primitive(part.0) != Some(6))
                .collect();
            let draws = opaque
                .iter()
                .map(|part| {
                    cockpit_occlusion::DepthDraw::whole(&geometry.catalog, part.0, part.1).unwrap()
                })
                .collect();
            let scene = Arc::new(
                CockpitOcclusion::new(
                    ShipClass::Pioneer,
                    geometry.catalog.clone(),
                    Vec::new(),
                    draws,
                )
                .unwrap(),
            );
            let mut hidden = 0;
            for slot in 0..7 {
                let (center, half, axes) =
                    arkline_protocol::cockpit_mechanism_screen(slot, fraction).unwrap();
                // Independent direct-triangle oracle, not a second call to the
                // surface's BVH ray guard. The same geometry includes pockets.
                let samples: Vec<_> = (0..=4)
                    .flat_map(|row| {
                        (0..=4).map(move |column| {
                            egui::vec2(0.05 + column as f32 * 0.225, 0.05 + row as f32 * 0.225)
                        })
                    })
                    .map(|uv| {
                        let target = vector(center) - vector(axes[0]) * 0.044
                            + vector(axes[1]) * ((0.5 - uv.y) * 2. * (half[1] as f32 - 0.012))
                            + vector(axes[2]) * ((uv.x - 0.5) * 2. * (half[2] as f32 - 0.012));
                        let distance = target.distance(eye);
                        let ray = (target - eye).normalize();
                        let blocked = opaque.iter().any(|part| {
                            ray_mesh_hit(
                                eye,
                                ray,
                                part.1,
                                &geometry.catalog.uploads()[mesh_index(part.0)],
                            )
                            .is_some_and(|hit| hit < distance - 0.00001)
                        });
                        (uv, blocked)
                    })
                    .collect();
                for size in [egui::vec2(1280., 720.), egui::vec2(800., 650.)] {
                    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
                    let vp =
                        Mat4::perspective_rh_gl(FOV_RADIANS, viewport.aspect_ratio(), 0.02, 100.)
                            * Mat4::look_at_rh(eye, vector(center), Vec3::Y);
                    let surface = physical_surface_with_mechanisms(
                        viewport,
                        vp,
                        Mat4::IDENTITY,
                        slot + 8,
                        egui::vec2(300., 180.),
                        scene.clone(),
                        fractions,
                    )
                    .unwrap();
                    let canvas = surface.canvas_rect();
                    assert!(
                        (canvas.aspect_ratio()
                            - (half[2] as f32 - 0.012) / (half[1] as f32 - 0.012))
                            .abs()
                            < 1e-5
                    );
                    if let Some(expected) = canvases[slot] {
                        assert_eq!(
                            canvas.size(),
                            expected,
                            "Motion and viewport aspect cannot resize a source capture"
                        );
                    } else {
                        canvases[slot] = Some(canvas.size());
                    }
                    for &(uv, blocked) in &samples {
                        let point = canvas.min + canvas.size() * uv;
                        let projected = surface.project(point).unwrap();
                        let hit = surface.unproject(projected);
                        assert_eq!(hit.is_none(), blocked,
                            "slot {slot}, fraction {fraction}, viewport {size:?}, point {uv:?}: opaque pocket/body must reject input; clear glass must allow it");
                        if let Some(hit) = hit {
                            assert!(hit.distance(point) < 0.1);
                        }
                        if blocked {
                            hidden += 1;
                        }
                        total += 1;
                    }
                }
            }
            if fraction == 1. {
                assert_eq!(hidden, 0, "Fully deployed screen glass must be readable");
            } else {
                partial_hidden += hidden;
            }
            eprintln!(
                "Mechanism grid fraction {fraction}: {hidden}/350 samples occluded (two viewports)"
            );
        }
        assert_eq!(total, 1750);
        assert!(
            partial_hidden > 0,
            "Travel must exercise hidden pocket contents, not only deployed glass"
        );
    }

    #[test]
    fn prototype_seven_faces_keep_proportional_wings_and_legacy_ship_slots() {
        let requested = egui::vec2(300.0, 180.0);
        for index in [1, 2, 4, 5, 6, 7] {
            let (_, half) = arkline_protocol::cockpit_screen(index).unwrap();
            let canvas = canonical_screen_canvas(index, vector(half), requested);
            assert!(
                (canvas.x / canvas.y - (half[2] as f32 - 0.008) / (half[1] as f32 - 0.008)).abs()
                    < 1e-5
            );
        }
        assert!(arkline_protocol::cockpit_screen(COCKPIT_SURFACE_SLOTS).is_none());
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280., 720.));
        let vp = Mat4::perspective_rh_gl(COCKPIT_FOV, viewport.aspect_ratio(), 0.002, 100.)
            * Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::Y);
        for class in arkline_protocol::ALL_SHIP_CLASSES {
            if class == ShipClass::Pioneer {
                continue;
            }
            assert_eq!(cockpit_screens(class).len(), 6);
            for index in 6..COCKPIT_SURFACE_SLOTS {
                assert!(CockpitSurface::new(viewport, vp, class, index, requested).is_none());
            }
        }
    }

    #[test]
    fn prototype_recessed_ceiling_is_contained_in_registered_solids_in_every_placement() {
        let geometry = registered_geometry(&arkline_protocol::default_pioneer()).unwrap();
        let original = geometry.clone();
        let mut replaced = 0;
        for panel in &geometry.panels {
            for pose in [
                PioneerDockedPose::private(),
                PioneerDockedPose::fleet(arkline_protocol::FleetLayoutRevision::LinearV1),
                PioneerDockedPose::fleet(arkline_protocol::FleetLayoutRevision::CircularV1),
            ] {
                let frame = body_frame(pose);
                let Some(parts) = recessed_enclosure_parts(panel, frame) else {
                    continue;
                };
                assert_eq!(
                    panel.collision,
                    arkline_protocol::PioneerDockedCollision::Solid
                );
                assert!(parts.len() > 5 && parts.len() < 24);
                let (color, emission) = surface(panel.material);
                let inverse = (frame * box_matrix(&panel.bounds)).inverse();
                for (mesh, model, actual_color, actual_emission) in parts {
                    assert_eq!(mesh, panel_surface_role(panel.material).cube());
                    assert_eq!(actual_color, color);
                    assert_eq!(actual_emission, emission);
                    for vertex in mesh_vertices(mesh) {
                        let local = inverse.transform_point3(model.transform_point3(vertex.p));
                        assert!(
                            local.abs().max_element() <= 1.0005,
                            "{} escapes its preexisting solid: {local:?}",
                            panel.id
                        );
                    }
                }
                replaced += 1;
            }
        }
        // The new chamber has one recessed ceiling; forward frame construction
        // is already authored in the canonical geometry, not replaced here.
        assert_eq!(replaced, 3);
        assert_eq!(
            geometry, original,
            "Presentation must not rewrite authority geometry"
        );
    }

    #[test]
    fn recessed_enclosure_preserves_registered_accessible_boundary_regions() {
        let g = registered_geometry(&arkline_protocol::default_pioneer()).unwrap();
        let mut checked = 0;
        for pose in [
            PioneerDockedPose::private(),
            PioneerDockedPose::fleet(arkline_protocol::FleetLayoutRevision::LinearV1),
            PioneerDockedPose::fleet(arkline_protocol::FleetLayoutRevision::CircularV1),
        ] {
            let d = dynamic_geometry(pose, actuator_pose(None, pose, None, 0)).unwrap();
            for x in [
                27.3, 27.7, 28.1, 28.5, 29., 29.5, 30., 30.5, 31., 31.5, 31.8, 32.1, 32.5, 33.,
                34.48,
            ] {
                for z in [-4., -3.5, -2.1, -1.5, -0.75, 0., 0.75, 1.5, 2.1] {
                    let feet = [x, arkline_protocol::deck_floor_y(2).unwrap(), z];
                    let station = body_to_station(pose, feet);
                    if !arkline_protocol::capsule_clear_station(&g, &d, pose, station)
                        || arkline_protocol::support_at_station(&g, &d, pose, station, None, 0.01)
                            .is_none()
                    {
                        continue;
                    }
                    // Each visible box is a subset of a preexisting solid. This
                    // sample check complements that universal containment proof.
                    for panel in &g.panels {
                        let Some(parts) = recessed_enclosure_parts(panel, Mat4::IDENTITY) else {
                            continue;
                        };
                        for (_, model, _, _) in parts {
                            let bounds = PioneerDockedBox {
                                center_m: model.w_axis.truncate().to_array().map(f64::from),
                                half_m: [
                                    model.x_axis.truncate().length() as f64,
                                    model.y_axis.truncate().length() as f64,
                                    model.z_axis.truncate().length() as f64,
                                ],
                                axes: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                            };
                            assert!(!arkline_protocol::box_intersects_capsule(
                                bounds,
                                feet,
                                arkline_protocol::AVATAR_HEIGHT_M,
                                arkline_protocol::STATION_AVATAR_RADIUS_M
                            ));
                        }
                    }
                    checked += 1;
                }
            }
        }
        assert!(
            checked > 100,
            "Boundary coverage must include all three placements"
        );
        eprintln!("Registered clear/supported boundary samples preserved: {checked}");
    }
    #[test]
    fn all_seven_instruments_fit_forward_and_head_turn_views_with_clearance() {
        let eye = vector(pilot_joints(0.0).eye_m);
        let cache = SceneCache::new(arkline_protocol::default_pioneer()).unwrap();
        let parts = seated_body_parts(&cache, None, 0).unwrap();
        let geometry = flight_geometry().unwrap();
        let draws = parts
            .iter()
            .filter(|part| docked_source_primitive(part.0) != Some(6))
            .map(|part| {
                cockpit_occlusion::DepthDraw::whole(&geometry.catalog, part.0, part.1).unwrap()
            })
            .collect();
        let scene = Arc::new(
            CockpitOcclusion::new(
                ShipClass::Pioneer,
                geometry.catalog.clone(),
                Vec::new(),
                draws,
            )
            .unwrap(),
        );
        for size in [
            egui::vec2(1570., 1014.),
            egui::vec2(1120., 720.),
            egui::vec2(800., 650.),
            egui::vec2(1500., 638.),
        ] {
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
            for index in 1..8 {
                let yaw: f32 = match index {
                    6 => -0.95,
                    7 => 0.95,
                    _ => 0.,
                };
                let direction = Vec3::new(yaw.cos(), 0., yaw.sin());
                let vp = Mat4::perspective_rh_gl(FOV_RADIANS, viewport.aspect_ratio(), 0.02, 100.)
                    * Mat4::look_at_rh(eye, eye + direction, Vec3::Y);
                let (center, half) = arkline_protocol::cockpit_screen(index).unwrap();
                let surface = physical_surface(
                    viewport,
                    vp,
                    Mat4::IDENTITY,
                    index,
                    egui::vec2(300., 180.),
                    scene.clone(),
                )
                .unwrap();
                let canvas = surface.canvas_rect();
                if matches!(index, 1 | 2 | 4 | 5 | 6 | 7) {
                    let tile = egui::Rect::from_min_size(canvas.min, egui::vec2(1.0, 1.0));
                    let (_, a, b) = surface.feed_geometry(tile).unwrap();
                    let diagonal = b - a;
                    assert!(
                        (diagonal.y.abs() - diagonal.z.abs()).abs() < 1e-5,
                        "A square source glyph/video tile must remain square on the physical glass"
                    );
                } else {
                    assert_eq!(canvas.size(), egui::vec2(300.0, 180.0));
                }
                // The full live glass must remain accessible, not only its
                // corners: inclined neighboring cases can cross its interior.
                for point in (0..=4).flat_map(|row| {
                    (0..=4).map(move |column| {
                        canvas.min
                            + canvas.size() * egui::vec2(column as f32 / 4.0, row as f32 / 4.0)
                    })
                }) {
                    let projected = surface
                        .project(point)
                        .unwrap_or_else(|| panic!("Face {index}, {size:?}, canvas corner {point:?} must fit without clipping"));
                    assert!(
                        viewport.shrink(8.).contains(projected),
                        "Face {index}, {size:?}: {projected:?}"
                    );
                    // Check against the actual canonical glass face, not only
                    // a self-consistent project/unproject pair with wrong axes.
                    let axes = arkline_protocol::cockpit_screen_axes(index).unwrap();
                    let u = (point.x - canvas.left()) / canvas.width();
                    let v = (point.y - canvas.top()) / canvas.height();
                    let glass = vector(center) - vector(axes[0]) * (0.025 + 0.006 + 0.001)
                        + vector(axes[1]) * ((0.5 - v) * 2.0 * (half[1] as f32 - 0.008))
                        + vector(axes[2]) * ((u - 0.5) * 2.0 * (half[2] as f32 - 0.008));
                    let hit = surface.unproject(projected);
                    if hit.is_none_or(|hit| hit.distance(point) >= 0.1) {
                        let direction = (glass - eye).normalize();
                        let distance = glass.distance(eye);
                        let mut blockers: Vec<_> = cache
                            .geometry
                            .panels
                            .iter()
                            .filter_map(|panel| {
                                ray_mesh_hit(eye, direction, box_matrix(&panel.bounds), &cube())
                                    .filter(|t| *t < distance - 0.00001)
                                    .map(|t| (t, panel.id.as_str()))
                            })
                            .collect();
                        blockers.sort_by(|a, b| a.0.total_cmp(&b.0));
                        let (plane, lt, rb) = surface.feed_geometry(canvas).unwrap();
                        let (origin, ray) = screen_ray(plane, viewport, projected).unwrap();
                        let local = origin + ray * ((lt.x - origin.x) / ray.x);
                        let raw_uv = [
                            (local.z - lt.z) / (rb.z - lt.z),
                            (local.y - lt.y) / (rb.y - lt.y),
                        ];
                        let mut exact_hits: Vec<_> = parts
                            .iter()
                            .enumerate()
                            .filter(|(_, part)| docked_source_primitive(part.0) != Some(6))
                            .filter_map(|(draw, part)| {
                                ray_mesh_hit(
                                    eye,
                                    direction,
                                    part.1,
                                    &geometry.catalog.uploads()[mesh_index(part.0)],
                                )
                                .filter(|t| *t < distance + 0.001)
                                .map(|t| (t, draw, mesh_index(part.0)))
                            })
                            .collect();
                        exact_hits.sort_by(|a, b| a.0.total_cmp(&b.0));
                        panic!("Face {index}, {size:?}, point {point:?}, mapped {hit:?}, raw_uv {raw_uv:?}, distance {distance}, panels {blockers:?}, exact mesh hits {exact_hits:?}");
                    }
                    let clip = vp * glass.extend(1.0);
                    let ndc = clip.truncate() / clip.w;
                    let expected = viewport.min
                        + egui::vec2((ndc.x * 0.5 + 0.5) * size.x, (0.5 - ndc.y * 0.5) * size.y);
                    assert!(
                        projected.distance(expected) < 0.02,
                        "Face {index} UI plane must match its real canonical glass"
                    );
                }
                if index == 3 {
                    assert!(surface.bounds().width() / size.x < 0.45);
                    assert!(
                        surface.bounds().area() / viewport.area() < 0.16,
                        "Prototype computer area must stay bounded: {:?} in {size:?}",
                        surface.bounds()
                    );
                    assert!(
                        surface.bounds().top() > size.y * 0.55,
                        "Computer must stay in the low instrument band, leaving the forward view clear"
                    );
                }
            }
        }
    }
    #[test]
    fn runtime_perimeter_refit_clears_identified_source_brace_without_mutating_catalog() {
        let original = pioneer_asset().unwrap();
        let derived = super::super::pioneer_docked_source::source_meshes().unwrap();
        let eye = Vec3::new(33.22, 1., 3.);
        let direction = Vec3::new(1., -0.05, 0.);
        let metres = Mat4::from_scale(Vec3::splat(100.));
        assert!(ray_mesh_hit(eye, direction, metres, &original[1].vertices).is_some());
        assert!(
            ray_mesh_hit(eye, direction, metres, &derived[1]).is_none(),
            "Audited old diagonal must not remain across the primary aperture"
        );
        let geometry = registered_geometry(&arkline_protocol::default_pioneer()).unwrap();
        let post = geometry
            .panels
            .iter()
            .find(|p| p.id == "cockpit/chamber/canopy-starboard")
            .unwrap();
        assert!(
            ray_mesh_hit(
                Vec3::new(31.48, 1.35, 2.6),
                Vec3::X,
                box_matrix(&post.bounds),
                &cube()
            )
            .is_some(),
            "Replacement is actual retained geometry at the perimeter"
        );
    }
}
