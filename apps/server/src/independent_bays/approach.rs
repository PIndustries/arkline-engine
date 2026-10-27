//! Conservative carrier-envelope routing to an exact independent-bay entry.
//! This plans geometry only. Dynamic peers, lane reservations and actual clocks
//! are checked by the authority executor before admitting/advancing any segment.
use anyhow::{ensure, Context, Result};
use arkline_protocol::drone_bay_geometry::BayPose;
use arkline_protocol::independent_bay_geometry::{IndependentBayGeometry, IndependentBayKeyframe};
use arkline_protocol::IndependentBayPhase;

mod runtime;
pub use runtime::*;

const BODY_CLEARANCE_M: f64 = 0.25;
const CORNER_CLEARANCE_M: f64 = 0.05;
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2])
}
fn bounds_valid(bounds: [[f64; 3]; 2]) -> bool {
    (0..3).all(|i| {
        bounds[0][i].is_finite() && bounds[1][i].is_finite() && bounds[0][i] < bounds[1][i]
    })
}
fn outside(point: [f64; 3], bounds: [[f64; 3]; 2]) -> bool {
    (0..3).any(|i| point[i] < bounds[0][i] || point[i] > bounds[1][i])
}
/// Closed slab intersection: tangency is not admitted as clear space.
fn segment_clear(a: [f64; 3], b: [f64; 3], bounds: [[f64; 3]; 2]) -> bool {
    let mut lo = 0f64;
    let mut hi = 1f64;
    for i in 0..3 {
        let delta = b[i] - a[i];
        if delta == 0. {
            if a[i] < bounds[0][i] || a[i] > bounds[1][i] {
                return true;
            }
        } else {
            let x = (bounds[0][i] - a[i]) / delta;
            let y = (bounds[1][i] - a[i]) / delta;
            lo = lo.max(x.min(y));
            hi = hi.min(x.max(y));
            if lo > hi {
                return true;
            }
        }
    }
    false
}
pub fn independent_drone_radius(bounds: [[f64; 3]; 2]) -> Option<f64> {
    if !bounds_valid(bounds) {
        return None;
    }
    let radius = bounds[0][0]
        .abs()
        .max(bounds[1][0].abs())
        .hypot(bounds[0][1].abs().max(bounds[1][1].abs()))
        .hypot(bounds[0][2].abs().max(bounds[1][2].abs()));
    radius.is_finite().then_some(radius)
}
/// During stationary exterior attitude alignment, use the complete body's
/// corner-radius cube as a fixed-attitude conservative proxy. It encloses every
/// intermediate rotation; the tighter OBB helper remains translation-only.
pub fn independent_entry_swept_body(
    segment: arkline_protocol::drone_bay_geometry::BayMotionSegment,
    local_bounds_m: [[f64; 3]; 2],
) -> Option<super::IndependentBaySweptBody> {
    let radius = independent_drone_radius(local_bounds_m)?;
    segment.sample(segment.started_at_ms)?;
    segment.sample(segment.started_at_ms.checked_add(segment.duration_ms)?)?;
    if segment.source.orientation_xyzw == segment.target.orientation_xyzw
        || segment.source.orientation_xyzw == segment.target.orientation_xyzw.map(|v| -v)
    {
        return Some(super::IndependentBaySweptBody {
            segment,
            local_bounds_m,
        });
    }
    // Translation+rotation is deliberately unsupported by the entry planner.
    if segment.source.position_m != segment.target.position_m {
        return None;
    }
    let identity = [0., 0., 0., 1.];
    Some(super::IndependentBaySweptBody {
        segment: arkline_protocol::drone_bay_geometry::BayMotionSegment {
            source: BayPose {
                orientation_xyzw: identity,
                ..segment.source
            },
            target: BayPose {
                orientation_xyzw: identity,
                ..segment.target
            },
            ..segment
        },
        local_bounds_m: [[-radius; 3], [radius; 3]],
    })
}

/// Full inverse rigid transform; subtract origins before rotation to avoid
/// unnecessarily rotating AU-sized absolute positions. No heading-only fallback.
pub fn independent_world_to_body(parent: BayPose, world: BayPose) -> Option<BayPose> {
    if !parent.valid() || !world.valid() {
        return None;
    }
    let [x, y, z, w] = parent.orientation_xyzw;
    let inverse = BayPose {
        position_m: [0.; 3],
        orientation_xyzw: [-x, -y, -z, w],
    };
    let orientation = inverse
        .compose(BayPose {
            position_m: [0.; 3],
            ..world
        })?
        .orientation_xyzw;
    Some(BayPose {
        position_m: inverse.transform_vector(std::array::from_fn(|i| {
            world.position_m[i] - parent.position_m[i]
        }))?,
        orientation_xyzw: orientation,
    })
}
/// The returned sequence starts at the real accepted body pose and ends at the
/// exact source entry pose/attitude. Translation retains the original attitude;
/// any final rotation is stationary outside the complete inflated carrier hull.
pub fn independent_entry_route(
    actual: BayPose,
    bay: &IndependentBayGeometry,
    carrier_bounds: [[f64; 3]; 2],
    canonical: &[IndependentBayKeyframe],
) -> Result<Vec<IndependentBayKeyframe>> {
    independent_entry_route_with_obstacles(actual, bay, carrier_bounds, canonical, &[])
}

fn independent_entry_route_with_obstacles(
    actual: BayPose,
    bay: &IndependentBayGeometry,
    carrier_bounds: [[f64; 3]; 2],
    canonical: &[IndependentBayKeyframe],
    obstacles: &[[[f64; 3]; 2]],
) -> Result<Vec<IndependentBayKeyframe>> {
    ensure!(
        actual.valid() && bounds_valid(carrier_bounds),
        "Invalid independent entry frame/envelope"
    );
    let entry = canonical
        .first()
        .context("Independent source route is empty")?;
    ensure!(
        entry.drone_pose.valid() && entry.milestones.exterior_clear,
        "Source route has no exterior entry"
    );
    let poses = exterior_entry_poses(
        actual,
        entry.drone_pose,
        bay.drone_bounds_local_m,
        carrier_bounds,
        obstacles,
    )?;
    let mut route = Vec::new();
    for pose in poses {
        let mut frame = entry.clone();
        if pose.orientation_xyzw == actual.orientation_xyzw {
            frame.label = "Carrier-clear exterior approach".into();
            frame.phase = IndependentBayPhase::Approaching;
        } else {
            frame.label = "Aligning outside bay entry".into();
            frame.phase = IndependentBayPhase::Aligning;
        }
        frame.drone_pose = pose;
        route.push(frame);
    }
    route.extend(canonical.iter().skip(1).cloned());
    Ok(route)
}

/// Common conservative exterior planner. It carries no bay identity or lifecycle
/// meaning; callers must bind the target and all physical hardware to their own
/// exact registered source. Accepted static boxes are retained for deterministic
/// reopen; dynamic peers are checked continuously by the caller.
pub(crate) fn exterior_entry_poses(
    actual: BayPose,
    target: BayPose,
    drone_bounds: [[f64; 3]; 2],
    carrier_bounds: [[f64; 3]; 2],
    obstacles: &[[[f64; 3]; 2]],
) -> Result<Vec<BayPose>> {
    ensure!(
        actual.valid() && bounds_valid(carrier_bounds),
        "Invalid independent entry frame/envelope"
    );
    ensure!(target.valid(), "Invalid exterior target");
    let radius = independent_drone_radius(drone_bounds).context("Invalid full drone envelope")?;
    let inflated = [
        carrier_bounds[0].map(|v| v - radius - BODY_CLEARANCE_M),
        carrier_bounds[1].map(|v| v + radius + BODY_CLEARANCE_M),
    ];
    ensure!(
        bounds_valid(inflated)
            && outside(actual.position_m, inflated)
            && outside(target.position_m, inflated),
        "Drone/entry is not clear of complete carrier envelope"
    );
    ensure!(
        obstacles.len() <= 108 && obstacles.iter().all(|b| bounds_valid(*b)),
        "Invalid static obstacle catalog"
    );
    let mut blocked = vec![inflated];
    blocked.extend(obstacles.iter().map(|b| {
        [
            b[0].map(|v| v - radius - BODY_CLEARANCE_M),
            b[1].map(|v| v + radius + BODY_CLEARANCE_M),
        ]
    }));
    ensure!(
        blocked.iter().all(|b| bounds_valid(*b)
            && outside(actual.position_m, *b)
            && outside(target.position_m, *b)),
        "Actual drone or entry is inside a static obstacle envelope"
    );
    // Only obstacle corners connected to the initial approach's bounding region
    // can help its path. Every edge is still checked against ALL obstacles.
    let mut region: [[f64; 3]; 2] = [
        std::array::from_fn(|i| {
            actual.position_m[i]
                .min(target.position_m[i])
                .min(inflated[0][i])
        }),
        std::array::from_fn(|i| {
            actual.position_m[i]
                .max(target.position_m[i])
                .max(inflated[1][i])
        }),
    ];
    let mut included = vec![false; blocked.len()];
    included[0] = true;
    loop {
        let mut changed = false;
        for (index, bounds) in blocked.iter().enumerate() {
            if !included[index]
                && (0..3).all(|i| bounds[0][i] <= region[1][i] && bounds[1][i] >= region[0][i])
            {
                included[index] = true;
                changed = true;
                for i in 0..3 {
                    region[0][i] = region[0][i].min(bounds[0][i] - CORNER_CLEARANCE_M);
                    region[1][i] = region[1][i].max(bounds[1][i] + CORNER_CLEARANCE_M);
                }
            }
        }
        if !changed {
            break;
        }
    }
    let mut points = vec![actual.position_m, target.position_m];
    for (_, bounds) in blocked.iter().enumerate().filter(|(i, _)| included[*i]) {
        for bits in 0..8 {
            let corner = std::array::from_fn(|i| {
                if bits & (1 << i) == 0 {
                    bounds[0][i] - CORNER_CLEARANCE_M
                } else {
                    bounds[1][i] + CORNER_CLEARANCE_M
                }
            });
            if blocked.iter().all(|b| outside(corner, *b)) {
                points.push(corner);
            }
        }
    }
    ensure!(
        points.iter().flatten().all(|v| v.is_finite()),
        "Independent entry waypoint overflow"
    );
    let mut distances = vec![f64::INFINITY; points.len()];
    let mut previous = vec![None; points.len()];
    let mut visited = vec![false; points.len()];
    distances[0] = 0.;
    for _ in 0..points.len() {
        let Some(node) = (0..points.len())
            .filter(|i| !visited[*i])
            .min_by(|a, b| distances[*a].total_cmp(&distances[*b]).then(a.cmp(b)))
        else {
            break;
        };
        if !distances[node].is_finite() {
            break;
        }
        if node == 1 {
            break;
        }
        visited[node] = true;
        for next in 0..points.len() {
            if visited[next]
                || next == node
                || !blocked
                    .iter()
                    .all(|b| segment_clear(points[node], points[next], *b))
            {
                continue;
            }
            let candidate = distances[node] + distance(points[node], points[next]);
            if candidate < distances[next] {
                distances[next] = candidate;
                previous[next] = Some(node);
            }
        }
    }
    ensure!(
        distances[1].is_finite(),
        "No carrier-clear route to independent bay entry"
    );
    let mut indices = vec![1usize];
    while *indices.last().unwrap() != 0 {
        indices.push(previous[*indices.last().unwrap()].context("Broken independent entry path")?);
    }
    indices.reverse();
    let mut route: Vec<_> = indices
        .into_iter()
        .map(|index| BayPose {
            position_m: points[index],
            orientation_xyzw: actual.orientation_xyzw,
        })
        .collect();
    if route.last() != Some(&target) {
        route.push(target);
    }
    Ok(route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::{
        independent_bay_geometry::{IndependentBayMechanism, IndependentBayMilestones},
        DroneClass,
    };
    fn pose(position_m: [f64; 3]) -> BayPose {
        BayPose {
            position_m,
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    pub(super) fn fixture() -> (IndependentBayGeometry, Vec<IndependentBayKeyframe>) {
        let gate = pose([20., 0., 0.]);
        let reference = pose([0.; 3]);
        let bay = IndependentBayGeometry {
            bay_id: 5,
            preferred_home: 5,
            compatible_classes: vec![DroneClass::Mining],
            ore_transfer_capable: true,
            stored_pose: reference,
            support_pose: gate,
            contact_pose: gate,
            gate_pose: gate,
            clear_pose: gate,
            mechanisms: vec![IndependentBayMechanism {
                semantic_id: "fixture/door".into(),
                reference_pose: reference,
                local_bounds_m: [[-1.; 3], [1.; 3]],
            }],
            working_bounds_m: [[-1.; 3], [21.; 3]],
            drone_bounds_local_m: [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]],
        };
        let frame = IndependentBayKeyframe {
            label: "Source entry".into(),
            phase: IndependentBayPhase::Approaching,
            drone_pose: gate,
            mechanism_poses: vec![("fixture/door".into(), reference)],
            milestones: IndependentBayMilestones {
                exterior_clear: true,
                hardware_safe: true,
                ..Default::default()
            },
        };
        (bay, vec![frame])
    }
    #[test]
    fn opposite_side_return_goes_around_whole_asymmetric_carrier() {
        let (bay, canonical) = fixture();
        let bounds = [[-5., -3., -7.], [6., 9., 8.]];
        let actual = pose([-20., 0., 0.]);
        let route = independent_entry_route(actual, &bay, bounds, &canonical).unwrap();
        assert!(route.len() > 2);
        assert_eq!(route[0].drone_pose, actual);
        assert_eq!(route.last().unwrap().drone_pose, canonical[0].drone_pose);
        let radius = independent_drone_radius(bay.drone_bounds_local_m).unwrap() + BODY_CLEARANCE_M;
        let inflated = [bounds[0].map(|v| v - radius), bounds[1].map(|v| v + radius)];
        assert!(route.windows(2).all(|pair| segment_clear(
            pair[0].drone_pose.position_m,
            pair[1].drone_pose.position_m,
            inflated
        )));
    }
    #[test]
    fn orientation_is_retained_during_translation_then_aligned_outside() {
        let (bay, canonical) = fixture();
        let mut actual = pose([30., 0., 0.]);
        actual.orientation_xyzw = [0., 1., 0., 0.];
        let route = independent_entry_route(actual, &bay, [[-5.; 3], [5.; 3]], &canonical).unwrap();
        assert_eq!(route[0].drone_pose, actual);
        assert_eq!(
            route[1].drone_pose.orientation_xyzw,
            actual.orientation_xyzw
        );
        let alignment = route.last().unwrap();
        assert_eq!(alignment.phase, IndependentBayPhase::Aligning);
        assert_eq!(
            alignment.drone_pose.position_m,
            route[route.len() - 2].drone_pose.position_m
        );
        assert_eq!(alignment.drone_pose, canonical[0].drone_pose);
    }
    #[test]
    fn unsafe_entry_or_actual_inside_hull_is_not_teleported_outside() {
        let (bay, mut canonical) = fixture();
        assert!(
            independent_entry_route(pose([0.; 3]), &bay, [[-5.; 3], [5.; 3]], &canonical).is_err()
        );
        canonical[0].drone_pose = pose([0.; 3]);
        assert!(
            independent_entry_route(pose([30.; 3]), &bay, [[-5.; 3], [5.; 3]], &canonical).is_err()
        );
    }
    #[test]
    fn rolled_parent_inverse_preserves_full_attitude_and_body_coordinates() {
        let parent = BayPose {
            position_m: [1e11, -2e11, 3e11],
            orientation_xyzw: [
                std::f64::consts::FRAC_1_SQRT_2,
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
        };
        let body = BayPose {
            position_m: [20., -8., 3.],
            orientation_xyzw: [0., 1., 0., 0.],
        };
        let world = parent.compose(body).unwrap();
        let returned = independent_world_to_body(parent, world).unwrap();
        assert!(distance(returned.position_m, body.position_m) < 1e-4);
        assert!(returned
            .orientation_xyzw
            .iter()
            .zip(body.orientation_xyzw)
            .all(|(a, b)| (a - b).abs() < 1e-12));
    }
    #[test]
    fn rotating_entry_proxy_contains_all_sampled_body_corners() {
        let source = pose([20., 0., 0.]);
        let target = BayPose {
            orientation_xyzw: [0., 1., 0., 0.],
            ..source
        };
        let segment = arkline_protocol::drone_bay_geometry::BayMotionSegment {
            source,
            target,
            started_at_ms: 0,
            duration_ms: 750,
        };
        let bounds = [[-2.5, -0.9, -3.], [2.5, 0.9, 3.]];
        let proxy = independent_entry_swept_body(segment, bounds).unwrap();
        for time in (0..=750).step_by(5) {
            let actual = segment.sample(time).unwrap().0;
            for bits in 0..8 {
                let corner = std::array::from_fn(|i| bounds[usize::from(bits & (1 << i) != 0)][i]);
                let offset = actual.transform_vector(corner).unwrap();
                assert!(
                    (0..3).all(|i| offset[i] >= proxy.local_bounds_m[0][i] - 1e-12
                        && offset[i] <= proxy.local_bounds_m[1][i] + 1e-12)
                );
            }
        }
        let mut combined = segment;
        combined.target.position_m[0] += 1.;
        assert!(independent_entry_swept_body(combined, bounds).is_none());
    }
}
