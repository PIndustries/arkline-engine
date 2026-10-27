//! Exact composed-source visual effect geometry, in body metres.
//!
//! This is NOT source/gameplay admission, an authored propulsion nozzle, or a
//! force/mass/thermal simulation. The caller must retain authoritative connected,
//! fresh acceleration/warp-state and docking/grounding gates. No ShipClass-only
//! path is changed. Convert metres to scene units exactly once at integration.

pub(crate) const SOURCE_SHA256: &str =
    "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113";
const MAX_MAIN_LENGTH_M: f64 = 12.0 * 72.0 / 7.0;
const MAIN_RADIUS_M: f64 = 1.8; // pulse <=1.1 gives1.98 < actual bore2.2
const REACTION_RADIUS_M: f64 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Outward {
    Forward,
    Aft,
    Up,
    Down,
    Starboard,
    Port,
}
impl Outward {
    pub(crate) fn direction(self) -> [f64; 3] {
        match self {
            Self::Forward => [1., 0., 0.],
            Self::Aft => [-1., 0., 0.],
            Self::Up => [0., 1., 0.],
            Self::Down => [0., -1., 0.],
            Self::Starboard => [0., 0., 1.],
            Self::Port => [0., 0., -1.],
        }
    }
}

/// Convert confirmed body-axis acceleration into the OPPOSITE exhaust axis.
/// Forward acceleration is already represented by the main engines.
pub(crate) fn reaction_outward(axis: usize, acceleration: f64) -> Option<Outward> {
    if !acceleration.is_finite() || acceleration.abs() < 0.01 {
        return None;
    }
    match (axis, acceleration > 0.) {
        (0, true) => None,
        (0, false) => Some(Outward::Forward),
        (1, true) => Some(Outward::Down),
        (1, false) => Some(Outward::Up),
        (2, true) => Some(Outward::Port),
        (2, false) => Some(Outward::Starboard),
        _ => None,
    }
}

/// An ellipsoid with its forward/tip endpoint fixed to the measured source lip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Plume {
    pub tip_m: [f64; 3],
    pub center_m: [f64; 3],
    pub outward: Outward,
    pub half_length_m: f64,
    pub radius_m: f64,
}
impl Plume {
    fn anchored(tip_m: [f64; 3], outward: Outward, length_m: f64, radius_m: f64) -> Self {
        let half_length_m = length_m * 0.5;
        let direction = outward.direction();
        Self {
            tip_m,
            center_m: std::array::from_fn(|i| tip_m[i] + direction[i] * half_length_m),
            outward,
            half_length_m,
            radius_m,
        }
    }
}

/// Explicit installed-byte binding only; capability, class, revision or a
/// server's source-admitted flag alone must not select this geometry.
pub(crate) struct SourceGeometry(());
impl SourceGeometry {
    pub(crate) fn for_verified_glb_sha256(hash: &str) -> Option<Self> {
        (hash == SOURCE_SHA256).then_some(Self(()))
    }

    /// Candidate variant positions for the two known legacy navigation lamps.
    /// Integration must version the authoritative exterior definition as well;
    /// never silently relocate an edited/custom fixture or mutate its circuitry.
    pub(crate) fn navigation_fixture_position(
        &self,
        id: &str,
        legacy_position_m: [f64; 3],
    ) -> Option<[f64; 3]> {
        let side = match id {
            "pioneer.exterior.nav.port" => -1.,
            "pioneer.exterior.nav.starboard" => 1.,
            _ => return None,
        };
        if legacy_position_m != [-24., 2.8, side * 18.95] {
            return None;
        }
        Some([-24., 7.2, side * (33.79999923706055 + 0.18)])
    }

    /// Existing stage lengths may be supplied after conversion to body metres.
    /// The pulse applies to both CENTER OFFSET and length, never to radius alone
    /// or length alone while leaving center behind. Arbitrary startup lengths
    /// down to positive zero stay on the aft side of the lip.
    pub(crate) fn main(&self, length_m: f64, pulse: f64, layer: f64) -> Vec<Plume> {
        if !length_m.is_finite()
            || !pulse.is_finite()
            || !layer.is_finite()
            || length_m <= 0.
            || layer <= 0.
        {
            return Vec::new();
        }
        let pulse = pulse.clamp(0.9, 1.1);
        let length = length_m.min(MAX_MAIN_LENGTH_M) * pulse;
        let radius = MAIN_RADIUS_M * pulse * layer.min(1.);
        [-12.5, 12.5]
            .into_iter()
            .map(|z| Plume::anchored([-35.9, 0., z], Outward::Aft, length, radius))
            .collect()
    }

    /// Strength is the existing normalized visual amplitude, not force. Main
    /// engines cover forward acceleration; no duplicated aft reaction plume.
    pub(crate) fn reaction(&self, outward: Outward, strength: f64) -> Vec<Plume> {
        if !strength.is_finite() || strength <= 0. {
            return Vec::new();
        }
        let tips: &[[f64; 3]] = match outward {
            Outward::Forward => &[[35.720001220703125, -3., 0.]],
            Outward::Aft => &[],
            Outward::Up => &[
                [-10., 8.979999542236328, -33.],
                [-10., 8.979999542236328, 33.],
            ],
            Outward::Down => &[[-10., -9., 0.]],
            Outward::Starboard => &[[-10., 7.2, 33.79999923706055]],
            Outward::Port => &[[-10., 7.2, -33.79999923706055]],
        };
        let length = 5.76 + strength.min(1.) * 11.52;
        tips.iter()
            .map(|tip| Plume::anchored(*tip, outward, length, REACTION_RADIUS_M))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn geometry() -> SourceGeometry {
        SourceGeometry::for_verified_glb_sha256(SOURCE_SHA256).unwrap()
    }
    fn tip_is_fixed(p: Plume) {
        for (i, d) in p.outward.direction().into_iter().enumerate() {
            assert!((p.center_m[i] - d * p.half_length_m - p.tip_m[i]).abs() < 1e-12);
        }
    }
    #[test]
    fn exhaust_is_opposite_confirmed_acceleration_without_duplicate_main() {
        for axis in 0..3 {
            for acceleration in [-60., 60.] {
                if axis == 0 && acceleration > 0. {
                    assert_eq!(reaction_outward(axis, acceleration), None);
                } else {
                    let direction = reaction_outward(axis, acceleration).unwrap().direction();
                    assert!(direction[axis] * acceleration < 0.);
                }
            }
        }
        assert_eq!(reaction_outward(3, 60.), None);
        for invalid in [0., 0.001, f64::NAN, f64::INFINITY] {
            assert_eq!(reaction_outward(1, invalid), None);
        }
    }
    #[test]
    fn only_exact_composed_source_selects_new_geometry() {
        assert!(SourceGeometry::for_verified_glb_sha256("1701").is_none());
        assert!(SourceGeometry::for_verified_glb_sha256("Pioneer").is_none());
        assert!(SourceGeometry::for_verified_glb_sha256(
            "0307cf36ba5da9ff37059dd2dea59c73edfec350a62f79a3e8fb6426addb18f0"
        )
        .is_none());
        assert!(SourceGeometry::for_verified_glb_sha256(SOURCE_SHA256).is_some());
    }
    #[test]
    fn candidate_nav_rebinding_preserves_unknown_or_edited_fixtures() {
        let g = geometry();
        assert_eq!(
            g.navigation_fixture_position("pioneer.exterior.nav.port", [-24., 2.8, -18.95]),
            Some([-24., 7.2, -33.97999923706055])
        );
        assert_eq!(
            g.navigation_fixture_position("pioneer.exterior.nav.starboard", [-24., 2.8, 18.95]),
            Some([-24., 7.2, 33.97999923706055])
        );
        assert!(g
            .navigation_fixture_position("custom", [-24., 2.8, -18.95])
            .is_none());
        assert!(g
            .navigation_fixture_position("pioneer.exterior.nav.port", [0.; 3])
            .is_none());
    }
    #[test]
    fn startup_pulse_layers_and_peak_never_cross_lip_or_bore() {
        for length in [
            f64::MIN_POSITIVE,
            1e-9,
            0.001,
            1.,
            41.142857,
            MAX_MAIN_LENGTH_M,
            1e9,
        ] {
            for pulse in [-1., 0.9, 1., 1.1, 100.] {
                for layer in [0.16 / 0.58, 0.31 / 0.58, 1., 2.] {
                    for p in geometry().main(length, pulse, layer) {
                        tip_is_fixed(p);
                        assert_eq!(p.tip_m[0], -35.9);
                        assert!(p.center_m[0] + p.half_length_m <= -35.9 + 1e-12);
                        assert!(p.radius_m <= 1.98 + 1e-12);
                        assert!(p.half_length_m * 2. <= 135.772);
                    }
                }
            }
        }
    }
    #[test]
    fn idle_invalid_inputs_and_no_duplicate_aft_effect() {
        for invalid in [0., -1., f64::NAN, f64::INFINITY] {
            assert!(geometry().main(invalid, 1., 1.).is_empty());
            assert!(geometry().reaction(Outward::Up, invalid).is_empty());
        }
        assert!(geometry().main(1., f64::NAN, 1.).is_empty());
        assert!(geometry().main(1., 1., f64::NAN).is_empty());
        assert!(geometry().reaction(Outward::Aft, 1.).is_empty());
    }
    #[test]
    fn reaction_startup_and_peak_preserve_fixed_external_mounts() {
        for direction in [
            Outward::Forward,
            Outward::Up,
            Outward::Down,
            Outward::Starboard,
            Outward::Port,
        ] {
            for strength in [1e-9, 0.1, 0.5, 1., 100.] {
                let plumes = geometry().reaction(direction, strength);
                assert!(!plumes.is_empty());
                for p in plumes {
                    tip_is_fixed(p);
                    assert!(p.half_length_m * 2. <= 17.28 + 1e-12);
                    assert_eq!(p.radius_m, 0.45);
                }
            }
        }
    }
}
