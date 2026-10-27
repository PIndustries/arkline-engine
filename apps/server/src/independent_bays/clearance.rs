//! Continuous same-time clearance for registered rigid bodies whose accepted
//! segment keeps a fixed attitude. Translation uses the actual shared smoothstep
//! sampler. Rotating/invalid input fails closed; free craft use the separate
//! conservative dynamic-obstacle admission, never this trusted-route shortcut.
use arkline_protocol::drone_bay_geometry::BayMotionSegment;

#[derive(Clone, Copy, Debug)]
pub struct IndependentBaySweptBody {
    pub segment: BayMotionSegment,
    pub local_bounds_m: [[f64; 3]; 2],
}
#[derive(Clone, Copy)]
struct BoxShape {
    axes: [[f64; 3]; 3],
    half: [f64; 3],
    center_offset: [f64; 3],
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn shape(body: &IndependentBaySweptBody) -> Option<BoxShape> {
    let segment = body.segment;
    segment.sample(segment.started_at_ms)?;
    let end = segment.started_at_ms.checked_add(segment.duration_ms)?;
    segment.sample(end)?;
    // Only exact fixed attitude (including the quaternion sign equivalence)
    // is admitted. An epsilon rotation multiplied by a large envelope is not a
    // translation-only proof, however small the quaternion difference looks.
    if segment.target.orientation_xyzw != segment.source.orientation_xyzw
        && segment.target.orientation_xyzw != segment.source.orientation_xyzw.map(|v| -v)
    {
        return None;
    }
    let axes = [
        segment.source.transform_vector([1., 0., 0.])?,
        segment.source.transform_vector([0., 1., 0.])?,
        segment.source.transform_vector([0., 0., 1.])?,
    ];
    let [lo, hi] = body.local_bounds_m;
    if !(0..3).all(|i| lo[i].is_finite() && hi[i].is_finite() && hi[i] > lo[i]) {
        return None;
    }
    let half = std::array::from_fn(|i| (hi[i] - lo[i]) * 0.5);
    let center_offset = segment
        .source
        .transform_vector(std::array::from_fn(|i| (hi[i] + lo[i]) * 0.5))?;
    if !half.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some(BoxShape {
        axes,
        half,
        center_offset,
    })
}
/// Polynomial coefficients of smoothstep over this interval in normalized u.
/// Callers split at both segment starts/ends, so each interval is one polynomial.
fn smooth_coefficients(segment: BayMotionSegment, start: u64, end: u64) -> [f64; 4] {
    let finish = segment.started_at_ms + segment.duration_ms;
    if end <= segment.started_at_ms {
        return [0.; 4];
    }
    if start >= finish {
        return [1., 0., 0., 0.];
    }
    let a = start.saturating_sub(segment.started_at_ms) as f64 / segment.duration_ms as f64;
    let b = (end - start) as f64 / segment.duration_ms as f64;
    [
        a * a * (3. - 2. * a),
        (6. * a - 6. * a * a) * b,
        (3. - 6. * a) * b * b,
        -2. * b * b * b,
    ]
}
struct Pair<'a> {
    a: &'a IndependentBaySweptBody,
    b: &'a IndependentBaySweptBody,
    sa: BoxShape,
    sb: BoxShape,
    axes: Vec<[f64; 3]>,
}
impl Pair<'_> {
    fn projection_range(&self, axis: [f64; 3], start: u64, end: u64) -> Option<(f64, f64)> {
        let Self { a, b, sa, sb, .. } = self;
        let ac = smooth_coefficients(a.segment, start, end);
        let bc = smooth_coefficients(b.segment, start, end);
        let da = dot(
            std::array::from_fn(|i| {
                a.segment.target.position_m[i] - a.segment.source.position_m[i]
            }),
            axis,
        );
        let db = dot(
            std::array::from_fn(|i| {
                b.segment.target.position_m[i] - b.segment.source.position_m[i]
            }),
            axis,
        );
        let offset = dot(
            std::array::from_fn(|i| {
                a.segment.source.position_m[i] - b.segment.source.position_m[i]
                    + sa.center_offset[i]
                    - sb.center_offset[i]
            }),
            axis,
        );
        let mut c: [f64; 4] = std::array::from_fn(|i| da * ac[i] - db * bc[i]);
        c[0] += offset;
        // The cubic Bernstein control hull encloses every intermediate value. It
        // remains conservative when relative motion turns between equal endpoints.
        let hull = [
            c[0],
            c[0] + c[1] / 3.,
            c[0] + 2. * c[1] / 3. + c[2] / 3.,
            c.iter().sum(),
        ];
        if !hull.iter().all(|v| v.is_finite()) {
            return None;
        }
        // Include absolute-coordinate rounding from the actual sampler's
        // source + delta*smooth evaluation, not merely the relative polynomial.
        // At AU-sized origins an otherwise tiny subtraction can round to contact.
        let coordinate_scale = [
            a.segment.source,
            a.segment.target,
            b.segment.source,
            b.segment.target,
        ]
        .iter()
        .flat_map(|pose| pose.position_m)
        .map(f64::abs)
        .sum::<f64>()
            + sa.half
                .iter()
                .chain(&sb.half)
                .chain(&sa.center_offset)
                .chain(&sb.center_offset)
                .map(|v| v.abs())
                .sum::<f64>();
        let error = 1e-7 * (1. + c.iter().map(|v| v.abs()).sum::<f64>())
            + 128. * f64::EPSILON * coordinate_scale;
        Some((
            hull.iter().copied().fold(f64::INFINITY, f64::min) - error,
            hull.iter().copied().fold(f64::NEG_INFINITY, f64::max) + error,
        ))
    }
    fn interval_clear(&self, start: u64, end: u64, margin: f64, depth: u8) -> bool {
        for axis in &self.axes {
            let radius = (0..3)
                .map(|i| {
                    self.sa.half[i] * dot(self.sa.axes[i], *axis).abs()
                        + self.sb.half[i] * dot(self.sb.axes[i], *axis).abs()
                })
                .sum::<f64>()
                + margin;
            let Some((lo, hi)) = self.projection_range(*axis, start, end) else {
                return false;
            };
            if lo > radius || hi < -radius {
                return true;
            }
        }
        if end.saturating_sub(start) <= 1 || depth == 20 {
            return false;
        }
        let mid = start + (end - start) / 2;
        self.interval_clear(start, mid, margin, depth + 1)
            && self.interval_clear(mid, end, margin, depth + 1)
    }
}

/// True means strict separation throughout the complete interval, not only at
/// endpoints or sampled render frames. Inclusive tangency blocks. A bounded
/// unresolved interval, invalid pose or rotating body blocks conservatively.
pub fn independent_bay_sweeps_clear(
    a: IndependentBaySweptBody,
    b: IndependentBaySweptBody,
    start: u64,
    end: u64,
    margin_m: f64,
) -> bool {
    if end < start || !margin_m.is_finite() || margin_m < 0. {
        return false;
    }
    let (Some(a), Some(b)) = (
        prepare_independent_bay_body(a),
        prepare_independent_bay_body(b),
    ) else {
        return false;
    };
    prepared_independent_bay_sweeps_clear(&a, &b, start, end, margin_m)
}

/// Owns one immutable body and its exact validated shape. No source, owner or
/// operation-id lookup/cache exists here; changed input requires a new value.
#[derive(Clone, Copy)]
pub(crate) struct PreparedIndependentBayBody {
    body: IndependentBaySweptBody,
    shape: BoxShape,
}
pub(crate) fn prepare_independent_bay_body(
    body: IndependentBaySweptBody,
) -> Option<PreparedIndependentBayBody> {
    Some(PreparedIndependentBayBody {
        shape: shape(&body)?,
        body,
    })
}
pub(crate) fn prepared_independent_bay_sweeps_clear(
    a: &PreparedIndependentBayBody,
    b: &PreparedIndependentBayBody,
    start: u64,
    end: u64,
    margin_m: f64,
) -> bool {
    if end < start || !margin_m.is_finite() || margin_m < 0. {
        return false;
    }
    let (sa, sb) = (a.shape, b.shape);
    let (a, b) = (&a.body, &b.body);
    let mut axes: Vec<_> = sa.axes.into_iter().chain(sb.axes).collect();
    for first in sa.axes {
        for second in sb.axes {
            let axis = cross(first, second);
            let length = dot(axis, axis).sqrt();
            if length > 1e-10 {
                axes.push(axis.map(|v| v / length));
            }
        }
    }
    let mut cuts = vec![start, end];
    for segment in [a.segment, b.segment] {
        for time in [
            segment.started_at_ms,
            segment.started_at_ms + segment.duration_ms,
        ] {
            if time > start && time < end {
                cuts.push(time);
            }
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    let pair = Pair { a, b, sa, sb, axes };
    if cuts.len() == 1 {
        return pair.interval_clear(start, end, margin_m, 0);
    }
    cuts.windows(2)
        .all(|interval| pair.interval_clear(interval[0], interval[1], margin_m, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::drone_bay_geometry::BayPose;
    fn body(
        a: [f64; 3],
        b: [f64; 3],
        start: u64,
        duration: u64,
        orientation: [f64; 4],
    ) -> IndependentBaySweptBody {
        IndependentBaySweptBody {
            segment: BayMotionSegment {
                source: BayPose {
                    position_m: a,
                    orientation_xyzw: orientation,
                },
                target: BayPose {
                    position_m: b,
                    orientation_xyzw: orientation,
                },
                started_at_ms: start,
                duration_ms: duration,
            },
            local_bounds_m: [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]],
        }
    }
    const ID: [f64; 4] = [0., 0., 0., 1.];
    #[test]
    fn actual_oriented_bodies_clear_at_pitches_where_circumspheres_overlap() {
        let yaw = [
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
            0.,
            std::f64::consts::FRAC_1_SQRT_2,
        ];
        let a = body([0., 0., 0.], [0., 0., 20.], 0, 10000, yaw);
        let b = body([8., 0., 0.], [8., 0., 20.], 0, 10000, yaw);
        let radius = (2.5f64.powi(2) + 0.9f64.powi(2) + 3f64.powi(2)).sqrt();
        assert!(8. < 2. * radius);
        assert!(independent_bay_sweeps_clear(a, b, 0, 10000, 0.25));
        let a = body([0., 0., 0.], [20., 0., 0.], 0, 10000, ID);
        let b = body([0., 0., 7.4], [20., 0., 7.4], 0, 10000, ID);
        assert!(independent_bay_sweeps_clear(a, b, 0, 10000, 0.25));
    }
    #[test]
    fn crossing_between_clear_endpoints_and_inclusive_tangency_block() {
        let a = body([-20., 0., 0.], [20., 0., 0.], 0, 10000, ID);
        let b = body([0., 0., -20.], [0., 0., 20.], 0, 10000, ID);
        assert!(independent_bay_sweeps_clear(a, b, 0, 0, 0.));
        assert!(independent_bay_sweeps_clear(a, b, 10000, 10000, 0.));
        assert!(!independent_bay_sweeps_clear(a, b, 0, 10000, 0.));
        let b = body([-20., 0., 6.], [20., 0., 6.], 0, 10000, ID);
        assert!(!independent_bay_sweeps_clear(a, b, 0, 10000, 0.));
    }
    #[test]
    fn different_clocks_are_checked_at_same_time_not_as_overlapping_whole_paths() {
        let a = body([-20., 0., 0.], [20., 0., 0.], 0, 10000, ID);
        let b = body([0., 0., -20.], [0., 0., 20.], 10000, 10000, ID);
        assert!(independent_bay_sweeps_clear(a, b, 0, 20000, 0.25));
        assert_eq!(
            independent_bay_sweeps_clear(a, b, 0, 20000, 0.25),
            independent_bay_sweeps_clear(b, a, 0, 20000, 0.25)
        );
    }
    #[test]
    fn invalid_rotating_and_nonfinite_envelopes_fail_closed() {
        let a = body([0., 0., 0.], [0., 0., 10.], 0, 1000, ID);
        let mut b = body([100., 0., 0.], [100., 0., 10.], 0, 1000, ID);
        b.segment.target.orientation_xyzw = [0., 1., 0., 0.];
        assert!(!independent_bay_sweeps_clear(a, b, 0, 1000, 0.));
        b.segment.target.orientation_xyzw = ID;
        b.local_bounds_m[1][0] = f64::INFINITY;
        assert!(!independent_bay_sweeps_clear(a, b, 0, 1000, 0.));
        assert!(!independent_bay_sweeps_clear(a, a, 1000, 0, 0.));
    }
    #[test]
    fn fifteen_disjoint_routes_are_order_independent_and_intruder_blocks() {
        let routes: Vec<_> = (0..15)
            .map(|i| {
                body(
                    [0., 0., i as f64 * 8.],
                    [20., 0., i as f64 * 8.],
                    i * 100,
                    10000,
                    ID,
                )
            })
            .collect();
        for a in 0..15 {
            for b in 0..15 {
                if a != b {
                    assert!(independent_bay_sweeps_clear(
                        routes[a], routes[b], 0, 12000, 0.25
                    ));
                }
            }
        }
        let intruder = body([10., 0., -20.], [10., 0., 130.], 0, 12000, ID);
        assert!(routes
            .iter()
            .any(|route| !independent_bay_sweeps_clear(*route, intruder, 0, 12000, 0.25)));
    }
    #[test]
    fn absolute_world_rounding_cannot_turn_claimed_separation_into_tangency() {
        let a = body(
            [0., 0., 300_000_000_000.000_06],
            [0., 0., 300_000_000_020.000_1],
            0,
            1000,
            ID,
        );
        let b = body(
            [0., 0., 300_000_000_006.000_1],
            [0., 0., 300_000_000_026.000_2],
            0,
            1000,
            ID,
        );
        let at = a.segment.sample(500).unwrap().0.position_m[2];
        let bt = b.segment.sample(500).unwrap().0.position_m[2];
        assert_eq!(bt - at, 6.0);
        assert!(!independent_bay_sweeps_clear(a, b, 0, 1000, 0.));
    }
    #[test]
    fn tiny_rotation_is_not_hidden_by_tolerance_on_large_envelope() {
        let a = body([0.; 3], [0.; 3], 0, 1000, ID);
        let mut b = body([100.; 3], [100.; 3], 0, 1000, ID);
        b.local_bounds_m = [[-1e12, -1., -1.], [1e12, 1., 1.]];
        b.segment.target.orientation_xyzw = [0., 0., 1e-13, 1.];
        assert!(!independent_bay_sweeps_clear(a, b, 0, 1000, 0.));
        let mut b = body([100.; 3], [100.; 3], 0, 1000, ID);
        b.segment.target.orientation_xyzw = ID.map(|v| -v);
        assert!(independent_bay_sweeps_clear(a, b, 0, 1000, 0.));
    }
}

#[cfg(test)]
mod prepared_tests {
    use super::*;
    use arkline_protocol::drone_bay_geometry::BayPose;
    fn reference_uncached(
        a: IndependentBaySweptBody,
        b: IndependentBaySweptBody,
        start: u64,
        end: u64,
        margin_m: f64,
    ) -> bool {
        if end < start || !margin_m.is_finite() || margin_m < 0. {
            return false;
        }
        let (Some(sa), Some(sb)) = (shape(&a), shape(&b)) else {
            return false;
        };
        let mut axes: Vec<_> = sa.axes.into_iter().chain(sb.axes).collect();
        for first in sa.axes {
            for second in sb.axes {
                let axis = cross(first, second);
                let length = dot(axis, axis).sqrt();
                if length > 1e-10 {
                    axes.push(axis.map(|v| v / length));
                }
            }
        }
        let mut cuts = vec![start, end];
        for segment in [a.segment, b.segment] {
            for time in [
                segment.started_at_ms,
                segment.started_at_ms + segment.duration_ms,
            ] {
                if time > start && time < end {
                    cuts.push(time);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        let pair = Pair {
            a: &a,
            b: &b,
            sa,
            sb,
            axes,
        };
        if cuts.len() == 1 {
            return pair.interval_clear(start, end, margin_m, 0);
        }
        cuts.windows(2)
            .all(|interval| pair.interval_clear(interval[0], interval[1], margin_m, 0))
    }

    fn body(x: f64, angle: f64) -> IndependentBaySweptBody {
        let q = [0., (angle * 0.5).sin(), 0., (angle * 0.5).cos()];
        let source = BayPose {
            position_m: [x, 0., 0.],
            orientation_xyzw: q,
        };
        IndependentBaySweptBody {
            segment: BayMotionSegment {
                source,
                target: BayPose {
                    position_m: [x + 2., 1., 0.],
                    ..source
                },
                started_at_ms: 100,
                duration_ms: 1000,
            },
            local_bounds_m: [[-1.; 3], [1.; 3]],
        }
    }
    fn cached(
        a: IndependentBaySweptBody,
        b: IndependentBaySweptBody,
        start: u64,
        end: u64,
        margin: f64,
    ) -> bool {
        let (Some(a), Some(b)) = (
            prepare_independent_bay_body(a),
            prepare_independent_bay_body(b),
        ) else {
            return false;
        };
        prepared_independent_bay_sweeps_clear(&a, &b, start, end, margin)
    }
    #[test]
    fn prepared_matches_frozen_uncached_algorithm_for_fixed_attitudes_and_clocks() {
        let mut seed = 0x12ab_5678_u64;
        for _ in 0..200 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let gap = (seed % 400) as f64 / 40. - 5.;
            let angle = ((seed >> 10) % 628) as f64 / 100.;
            for origin in [0., 1e6, 1e12] {
                let a = body(origin, angle);
                let mut b = body(origin + gap, -angle);
                b.segment.started_at_ms = 350;
                for (start, end) in [
                    (0, 0),
                    (0, 50),
                    (100, 300),
                    (300, 900),
                    (0, 2000),
                    (2000, 1000),
                ] {
                    for margin in [0., 0.001] {
                        assert_eq!(
                            cached(a, b, start, end, margin),
                            reference_uncached(a, b, start, end, margin)
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn prepared_rejects_invalid_bounds_rotation_and_time_without_stale_shape_reuse() {
        let a = body(0., 0.);
        let good = body(8., 0.);
        let mut cases = Vec::new();
        let mut b = good;
        b.local_bounds_m[1][0] = f64::NAN;
        cases.push(b);
        let mut b = good;
        b.local_bounds_m = [[-f64::MAX; 3], [f64::MAX; 3]];
        cases.push(b);
        let mut b = good;
        b.local_bounds_m[1] = b.local_bounds_m[0];
        cases.push(b);
        let mut b = good;
        b.segment.started_at_ms = u64::MAX;
        cases.push(b);
        let mut b = good;
        b.segment.target.orientation_xyzw = [0., 0., 1e-13, 1.];
        cases.push(b);
        let mut b = good;
        b.segment.source.orientation_xyzw = [0.; 4];
        cases.push(b);
        for b in cases {
            assert!(prepare_independent_bay_body(b).is_none());
            assert!(!cached(a, b, 0, 1000, 0.));
            assert_eq!(
                cached(a, b, 0, 1000, 0.),
                reference_uncached(a, b, 0, 1000, 0.)
            );
        }
        let mut antipodal = good;
        antipodal.segment.target.orientation_xyzw =
            antipodal.segment.source.orientation_xyzw.map(|x| -x);
        assert_eq!(
            cached(a, antipodal, 0, 2000, 0.),
            reference_uncached(a, antipodal, 0, 2000, 0.)
        );
        for margin in [f64::NAN, -0.001, f64::INFINITY] {
            assert!(!cached(a, good, 0, 1000, margin));
        }
        let old = prepare_independent_bay_body(good).unwrap();
        let mut changed = good;
        changed.segment.source.position_m = [0.; 3];
        changed.segment.target.position_m = [0.; 3];
        let changed = prepare_independent_bay_body(changed).unwrap();
        let a = prepare_independent_bay_body(a).unwrap();
        assert!(prepared_independent_bay_sweeps_clear(&a, &old, 0, 1000, 0.));
        assert!(!prepared_independent_bay_sweeps_clear(
            &a, &changed, 0, 1000, 0.
        ));
    }
}
