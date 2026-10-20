//! Presentation viewpoint fitted to the canonical Pioneer bridge cavity.
//! These body-metre measurements do not register the separate walkable decks.
use super::{Mat4, Quat, ShipClass, Vec3};
use std::{ops::Range, sync::OnceLock};

const PIONEER_GLB_SHA256: &str = "8125644f007537437942d34169e527c08ddd2dfaed575c874e2263ff2b7cb123";
const NAVIGATION_GLASS_PRIMITIVE: usize = 6;
const NAVIGATION_GLASS_MESH: usize = 10 + NAVIGATION_GLASS_PRIMITIVE;

pub(super) fn eye_body_meters(class: ShipClass) -> Vec3 {
    if class == ShipClass::Pioneer {
        // One canonical seated joint supplies docked and flight eye placement.
        Vec3::from_array(
            arkline_protocol::pilot_joints(0.0)
                .eye_m
                .map(|value| value as f32),
        )
    } else {
        let spec = class.spec();
        Vec3::new(
            spec.length_m as f32 * 0.0043 + 0.005,
            spec.height_m as f32 * 0.0062 + 0.015,
            0.0,
        ) * 100.0
    }
}

/// Local display coordinates stay attached to the ship during head look.
pub(super) fn mount(origin: Vec3, rotation: Quat, class: ShipClass) -> Mat4 {
    let scale = if class == ShipClass::Pioneer {
        0.01
    } else {
        0.025
    };
    Mat4::from_scale_rotation_translation(
        Vec3::splat(scale),
        rotation,
        origin + rotation * (eye_body_meters(class) / 100.0),
    )
}

/// The eight measured source bridge panes form an aperture in the seated view.
/// All other hull faces and exterior views retain their original opaque draws.
pub(super) fn canopy_ranges(mesh: usize) -> Option<&'static [Range<i32>]> {
    static VERIFIED: OnceLock<bool> = OnceLock::new();
    static APERTURE: [Range<i32>; 0] = [];
    if super::mesh_index(mesh) != NAVIGATION_GLASS_MESH {
        return None;
    }
    let verified = VERIFIED.get_or_init(|| {
        super::pioneer_asset_sha256() == PIONEER_GLB_SHA256
            && super::pioneer_asset()
                .ok()
                .and_then(|parts| parts.get(NAVIGATION_GLASS_PRIMITIVE))
                .is_some_and(|part| {
                    part.material_index == Some(NAVIGATION_GLASS_PRIMITIVE)
                        && part.material_name == "Navigation glass"
                        && part.vertices.len() == 288
                })
    });
    verified.then_some(APERTURE.as_slice())
}
