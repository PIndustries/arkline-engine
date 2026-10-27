//! Source-envelope label placement only; never changes physical eyes or authority.
use super::*;
use arkline_protocol::carrier_inventory_geometry::{
    source_envelope, CarrierEnvelope, CarrierEnvelopeState,
};
use arkline_protocol::drone_bay_geometry::BayPose;

fn upper_position(space: Space, parent: BayPose, envelope: CarrierEnvelope) -> Option<Vec3> {
    // support_m is inward ground support. The negative world-up normal asks
    // for the highest rotated corner, retaining the asymmetric body origin.
    let relative = BayPose {
        position_m: [0.; 3],
        ..parent
    };
    let upper = envelope.support_m(relative, [0., -1., 0.])?;
    let position = space.scene(parent.position_m) + Vec3::Y * (upper as f32 / 100. + 0.06);
    position.is_finite().then_some(position)
}

pub(super) fn position(
    pilot: &PilotView,
    space: Space,
    snapshot_time: u64,
    presentation_time: u64,
    advertised: Option<&arkline_protocol::CarrierInventoryRegistration>,
) -> Option<Vec3> {
    if pilot.carrier_inventory.is_none()
        && (advertised.is_none() || pilot.ship_class != ShipClass::Pioneer)
    {
        return Some(
            space.position(pilot, presentation_time)
                + Vec3::Y * (pilot.ship_class.spec().height_m as f32 / 200. + 0.06),
        );
    }
    let public = pilot.carrier_inventory.as_ref()?;
    if public.sample_time_ms != snapshot_time
        || advertised.is_some_and(|r| r != &public.binding().registration)
    {
        return None;
    }
    public.validate_projection(pilot).ok()?;
    let envelope = source_envelope(
        &public.binding().registration,
        CarrierEnvelopeState::Operational,
    )?;
    upper_position(space, public.parent, envelope)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn upper_support_preserves_asymmetry_rotation_and_au_rebase() {
        let bounds = CarrierEnvelope {
            bounds_m: [[-36., -9.001, -41.3], [36., 15.156822309505582, 41.3]],
        };
        let mut parent = BayPose {
            position_m: [149_597_870_700., -2000., 800.],
            orientation_xyzw: [0., 0., 0., 1.],
        };
        let space = Space {
            origin: parent.position_m,
        };
        let upright = upper_position(space, parent, bounds).unwrap();
        assert!(upright.distance(Vec3::new(0., 0.211_568_22, 0.)) < 1e-6);
        parent.orientation_xyzw = [1., 0., 0., 0.];
        let rolled = upper_position(space, parent, bounds).unwrap();
        assert!(rolled.distance(Vec3::new(0., 0.15001, 0.)) < 1e-6);
        parent.orientation_xyzw = [0.; 4];
        assert!(upper_position(space, parent, bounds).is_none());
    }
    #[test]
    fn legacy_label_height_unchanged_advertised_missing_hidden() {
        let ctx = egui::Context::default();
        let app = crate::tests::view_test_cockpit(&ctx);
        let pilot = app.own().unwrap();
        let space = Space { origin: [0.; 3] };
        assert_eq!(
            position(&pilot, space, 1000, 1300, None),
            Some(
                space.position(&pilot, 1300)
                    + Vec3::Y * (pilot.ship_class.spec().height_m as f32 / 200. + 0.06)
            )
        );
        let registration =
            &arkline_protocol::carrier_inventory_geometry::stationary_fixture_definition()
                .unwrap()
                .topology
                .registration;
        assert!(position(&pilot, space, 1000, 1300, Some(registration)).is_none());
        let mut unrelated = pilot.clone();
        unrelated.ship_class = ShipClass::Prospector;
        assert_eq!(
            position(&unrelated, space, 1000, 1300, Some(registration)),
            position(&unrelated, space, 1000, 1300, None)
        );
    }
}
