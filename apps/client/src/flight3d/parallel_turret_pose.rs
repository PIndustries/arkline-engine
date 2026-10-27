//! Single procedural gun owner over fixed exact1703 GLB wells. All consumers
//! receive these SAME world-space instances; no extra catalog gun is appended.
use super::*;

pub(super) struct Resolved {
    pub parts: Vec<ModelPart>,
    pub muzzle: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    pub can_fire: bool,
    /// Some only for the shared authoritative aim/finite beam contract.
    pub beam_radius_m: Option<f32>,
}
fn constrained(rotation: Quat) -> Quat {
    let d = rotation * Vec3::X;
    let yaw =
        d.z.atan2(d.x)
            .clamp(-25_f32.to_radians(), 25_f32.to_radians());
    let pitch =
        d.y.atan2((d.x * d.x + d.z * d.z).sqrt())
            .clamp(0., 20_f32.to_radians());
    Quat::from_rotation_arc(
        Vec3::X,
        Vec3::new(
            pitch.cos() * yaw.cos(),
            pitch.sin(),
            pitch.cos() * yaw.sin(),
        ),
    )
}
fn physical_parts(
    class: ShipClass,
    raw: Quat,
    slot: u8,
    fraction: f64,
) -> (Vec<ModelPart>, Quat, Vec3) {
    let g = arkline_protocol::turret_stow_geometry::aim_fraction(fraction) as f32;
    let rotation = Quat::IDENTITY.slerp(constrained(raw), g);
    physical_parts_sampled(class, rotation, slot, fraction)
}
fn physical_parts_sampled(
    class: ShipClass,
    rotation: Quat,
    slot: u8,
    fraction: f64,
) -> (Vec<ModelPart>, Quat, Vec3) {
    let drop = arkline_protocol::turret_stow_geometry::drop_at(fraction) as f32 / 100.;
    let mut parts = turret_parts(class, rotation, slot);
    for (_, model, _, _) in &mut parts {
        *model = Mat4::from_translation(-Vec3::Y * drop) * *model;
    }
    let (mount, radius, _) = turret_mount(class, slot);
    let top = mount.y - drop;
    let bottom = 7.08 / 100.;
    if let Some(pedestal) = parts.last_mut() {
        pedestal.1 = transform(
            Vec3::new(mount.x, (top + bottom) * 0.5, mount.z),
            Vec3::new(radius * 0.7, (top - bottom) * 0.5, radius * 0.7),
            Quat::IDENTITY,
        );
    }
    for side in [-1., 1.] {
        let h =
            arkline_protocol::turret_stow_geometry::hatch_box(usize::from(slot), side, fraction)
                .expect("validated turret fraction");
        parts.push((
            8,
            transform(
                Vec3::from_array(h.center_m.map(|v| v as f32 / 100.)),
                Vec3::from_array(h.half_m.map(|v| v as f32 / 100.)),
                Quat::IDENTITY,
            ),
            Vec3::new(0.075, 0.095, 0.105),
            0.,
        ));
    }
    let muzzle = turret_muzzle(class, rotation, slot) - Vec3::Y * drop;
    (parts, rotation, muzzle)
}

/// `sample_ms` is accepted snapshot time; `presentation_ms` is used ONLY by the
/// unchanged legacy path. New physical travel never extrapolates beyond sample.
/// Unknown/invalid carrier source returns None (caller diagnoses unavailable),
/// not a second legacy gun. Exact historical default retains its raised rig.
#[allow(clippy::too_many_arguments)] // Explicit accepted/presentation clocks and world/source gates.
pub(super) fn resolve(
    pilot: &PilotView,
    space: Space,
    sample_ms: u64,
    presentation_ms: u64,
    slot: u8,
    raw: Quat,
    live: bool,
    world_registration: Option<&arkline_protocol::CarrierInventoryRegistration>,
) -> Option<Resolved> {
    if slot >= 2 {
        return None;
    }
    if pilot.ship_class == ShipClass::Pioneer
        && world_registration.is_some()
        && pilot.carrier_inventory.is_none()
    {
        return None;
    }
    let (parent, parts, rotation, muzzle, can_fire, _controlled) = if let Some(public) =
        &pilot.carrier_inventory
    {
        if world_registration.is_some_and(|r| r != &public.binding().registration) {
            return None;
        }
        if public.sample_time_ms != sample_ms
            || !arkline_protocol::parallel_turret_binding::exact_registration(
                &public.binding().registration,
            )
        {
            return None;
        }
        public.validate_projection(pilot).ok()?;
        let geometry = flight_geometry().ok()?;
        if !geometry
            .carriers
            .iter()
            .filter_map(|c| c.as_ref().ok())
            .any(|c| c.registration == public.binding().registration)
        {
            return None;
        }
        let mechanisms = pilot.cockpit_mechanisms.as_ref()?;
        if mechanisms.vessel_id != public.binding().vessel_id {
            return None;
        }
        let parent = Mat4::from_rotation_translation(
            Quat::from_array(public.parent.orientation_xyzw.map(|v| v as f32)).normalize(),
            space.scene(public.parent.position_m),
        );
        if mechanisms.turrets == arkline_protocol::TurretDeployment::default() {
            if !raw.is_finite() || (raw.length_squared() - 1.).abs() > 1e-3 {
                return None;
            }
            // Immutable old fixture replay: actual authority was not present.
            // Preserve raised visual geometry, explicitly NO firing authority.
            (
                parent,
                turret_parts(pilot.ship_class, raw, slot),
                raw,
                turret_muzzle(pilot.ship_class, raw, slot),
                false,
                false,
            )
        } else {
            let state =
                arkline_protocol::parallel_turret_binding::confirmed_state(pilot, sample_ms)
                    .ok()?;
            let aim = state
                .aims
                .as_ref()
                .and_then(|aims| aims.get(usize::from(slot)));
            let f = state.actuators[usize::from(slot)].fraction_at(sample_ms);
            let (parts, rotation, mut muzzle) = if let Some(aim) = aim {
                aim.validate_at(sample_ms).ok()?;
                if aim.sample_ms != sample_ms {
                    return None;
                }
                let sampled = Quat::from_array(aim.orientation_xyzw(f)?.map(|v| v as f32));
                physical_parts_sampled(pilot.ship_class, sampled, slot, f)
            } else {
                if !raw.is_finite() || (raw.length_squared() - 1.).abs() > 1e-3 {
                    return None;
                }
                physical_parts(pilot.ship_class, raw, slot, f)
            };
            if aim.is_some() {
                // Exact physical cap exit; same body-space ray origin as authority.
                muzzle += rotation * Vec3::X * (0.101 / 100.);
            }
            (
                parent,
                parts,
                rotation,
                muzzle,
                live && presentation_ms >= sample_ms
                    && presentation_ms.saturating_sub(sample_ms) <= 1500
                    && state.operational_at(sample_ms)
                    && aim.is_some_and(|a| {
                        a.status == arkline_protocol::parallel_turret_aim::TurretAimStatus::Ready
                    }),
                true,
            )
        }
    } else {
        if !raw.is_finite() || (raw.length_squared() - 1.).abs() > 1e-3 {
            return None;
        }
        let parent = transform(
            space.position(pilot, presentation_ms),
            Vec3::ONE,
            ship_orientation_at(pilot, presentation_ms),
        );
        (
            parent,
            turret_parts(pilot.ship_class, raw, slot),
            raw,
            turret_muzzle(pilot.ship_class, raw, slot),
            live,
            false,
        )
    };
    let parts: Vec<_> = parts
        .into_iter()
        .map(|(mesh, model, color, emission)| (mesh, parent * model, color, emission))
        .collect();
    if parts.iter().any(|p| !p.1.is_finite()) {
        return None;
    }
    Some(Resolved {
        parts,
        muzzle: parent.transform_point3(muzzle),
        forward: parent.transform_vector3(rotation * Vec3::X).normalize(),
        up: parent.transform_vector3(rotation * Vec3::Y).normalize(),
        can_fire,
        beam_radius_m: pilot
            .cockpit_mechanisms
            .as_ref()
            .filter(|m| {
                pilot.carrier_inventory.is_some()
                    && m.turrets.source_admitted
                    && m.turrets.aims.is_some()
            })
            .map(|_| 0.10),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn world_marker_missing_projection_never_leaves_a_legacy_gun_pick() {
        let ctx = egui::Context::default();
        let cockpit = crate::tests::view_test_cockpit(&ctx);
        let mut pilot = cockpit.snapshot.unwrap().pilots.remove(0);
        pilot.ship_class = ShipClass::Pioneer;
        pilot.carrier_inventory = None;
        let marker = arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
            .unwrap()
            .topology
            .registration
            .clone();
        let space = Space { origin: [0.; 3] };
        assert!(resolve(
            &pilot,
            space,
            1000,
            1000,
            0,
            Quat::IDENTITY,
            false,
            Some(&marker)
        )
        .is_none());
        assert!(resolve(&pilot, space, 1000, 1000, 0, Quat::IDENTITY, false, None).is_some());
        pilot.ship_class = ShipClass::Prospector;
        assert!(resolve(
            &pilot,
            space,
            1000,
            1000,
            0,
            Quat::IDENTITY,
            false,
            Some(&marker)
        )
        .is_some());
    }
    #[test]
    fn physical_rig_keeps_one_gun_and_two_shutters_through_all_phases() {
        for slot in 0..2 {
            let count = turret_parts(ShipClass::Pioneer, Quat::IDENTITY, slot).len();
            for f in [0., 0.075, 0.15, 0.45, 0.75, 0.9, 1.] {
                let (parts, aim, muzzle) =
                    physical_parts(ShipClass::Pioneer, Quat::from_rotation_y(1.), slot, f);
                assert_eq!(parts.len(), count + 2);
                assert!(parts.iter().all(|p| p.1.is_finite()));
                let expected = turret_muzzle(ShipClass::Pioneer, aim, slot)
                    - Vec3::Y * (arkline_protocol::turret_stow_geometry::drop_at(f) as f32 / 100.);
                assert!((muzzle - expected).length() < 1e-6);
                if f <= 0.75 {
                    assert!(aim.abs_diff_eq(Quat::IDENTITY, 1e-6));
                }
                for (side, part) in [-1., 1.].into_iter().zip(&parts[count..]) {
                    let box_ = arkline_protocol::turret_stow_geometry::hatch_box(
                        usize::from(slot),
                        side,
                        f,
                    )
                    .unwrap();
                    assert!(
                        (part.1.transform_point3(Vec3::ZERO) * 100.
                            - Vec3::from_array(box_.center_m.map(|x| x as f32)))
                        .length()
                            < 1e-5
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod authoritative_pose_equivalence_tests {
    use super::*;
    #[test]
    fn actual_rust_parts_match_all_source_reference_boxes_and_emitter_exit() {
        use sha2::{Digest, Sha256};
        let bytes = include_bytes!("parallel_turret_pose_fixture.json");
        assert_eq!(
            format!("{:x}", Sha256::digest(bytes)),
            "2e7e29ea8a8d7e6364265c25c9387f5ef1a96902973a2b9b47c64dd997ef3089"
        );
        let fixture: serde_json::Value = serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            fixture["source_glb_sha256"].as_str(),
            Some(arkline_protocol::parallel_turret_binding::SOURCE_SHA256)
        );
        let rows = fixture["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 210);
        let vector = |v: &serde_json::Value| {
            Vec3::new(
                v[0].as_f64().unwrap() as f32,
                v[1].as_f64().unwrap() as f32,
                v[2].as_f64().unwrap() as f32,
            )
        };
        let mut max_error = 0_f32;
        for row in rows {
            let aim = arkline_protocol::parallel_turret_aim::TurretAim {
                yaw_rad: row["yaw_rad"].as_f64().unwrap(),
                pitch_rad: row["pitch_rad"].as_f64().unwrap(),
                ..Default::default()
            };
            let fraction = row["fraction"].as_f64().unwrap();
            let slot = row["slot"].as_u64().unwrap() as u8;
            let q = Quat::from_array(aim.orientation_xyzw(fraction).unwrap().map(|v| v as f32));
            let (parts, rotation, muzzle) =
                physical_parts_sampled(ShipClass::Pioneer, q, slot, fraction);
            let expected = row["parts"].as_array().unwrap();
            assert_eq!(parts.len(), expected.len());
            for ((_, matrix, _, _), reference) in parts.iter().zip(expected) {
                let center = matrix.transform_point3(Vec3::ZERO) * 100.;
                for x in [-1., 1.] {
                    for y in [-1., 1.] {
                        for z in [-1., 1.] {
                            let expected_corner = vector(&reference["center_m"])
                                + vector(&reference["scaled_axes_m"][0]) * x
                                + vector(&reference["scaled_axes_m"][1]) * y
                                + vector(&reference["scaled_axes_m"][2]) * z;
                            max_error = max_error.max(
                                (matrix.transform_point3(Vec3::new(x, y, z)) * 100.)
                                    .distance(expected_corner),
                            );
                        }
                    }
                }
                max_error = max_error.max(center.distance(vector(&reference["center_m"])));
                for (i, axis) in [Vec3::X, Vec3::Y, Vec3::Z].into_iter().enumerate() {
                    max_error = max_error.max(
                        (matrix.transform_vector3(axis) * 100.)
                            .distance(vector(&reference["scaled_axes_m"][i])),
                    );
                }
            }
            let center = vector(&expected[0]["center_m"]);
            let forward = vector(&expected[0]["scaled_axes_m"][0]).normalize();
            let emitter = (muzzle + rotation * Vec3::X * (0.101 / 100.)) * 100.;
            max_error = max_error.max(emitter.distance(center + forward * 5.285));
            // The entire cap/body/barrel is behind the emitted forward plane.
            assert!((emitter - center).dot(forward) > 5.284);
        }
        eprintln!("210 poses / 2310 boxes / 18480 corners; max body-metre deviation={max_error}");
        assert!(
            max_error <= 0.00005,
            "source/client pose error {max_error}m"
        );
    }
}
