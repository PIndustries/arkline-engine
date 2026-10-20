//! Per-view submission decisions. This never changes simulation or another view's set.
use super::{DepthMeshCatalog, Mat4, ModelPart, Vec3};

/// Reject only when all transformed AABB corners lie outside one clip plane.
/// Unknown/non-finite bounds stay visible. Works for perspective/orthographic views,
/// including a camera inside a large bound and bounds crossing the near plane.
pub(crate) fn part_in_view(catalog: &DepthMeshCatalog, vp: Mat4, part: &ModelPart) -> bool {
    let Some((min, max)) = catalog.mesh_bounds(part.0) else {
        return true;
    };
    if !vp.is_finite() || !part.1.is_finite() || !min.is_finite() || !max.is_finite() {
        return true;
    }
    let transform = vp * part.1;
    let corners = [
        Vec3::new(min.x, min.y, min.z),
        Vec3::new(max.x, min.y, min.z),
        Vec3::new(min.x, max.y, min.z),
        Vec3::new(max.x, max.y, min.z),
        Vec3::new(min.x, min.y, max.z),
        Vec3::new(max.x, min.y, max.z),
        Vec3::new(min.x, max.y, max.z),
        Vec3::new(max.x, max.y, max.z),
    ]
    .map(|p| transform * p.extend(1.));
    if corners.iter().any(|p| !p.is_finite()) {
        return true;
    }
    !(0..6).any(|plane| {
        corners.iter().all(|p| {
            let d = match plane {
                0 => p.w + p.x,
                1 => p.w - p.x,
                2 => p.w + p.y,
                3 => p.w - p.y,
                4 => p.w + p.z,
                _ => p.w - p.z,
            };
            d < -1e-5 * (p.w.abs() + 1.)
        })
    })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewVisibility {
    pub candidate_draws: u64,
    pub submitted_draws: u64,
    pub candidate_triangles: u64,
    pub submitted_triangles: u64,
    pub cpu_prepare_ns: u64,
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use eframe::glow::{self, HasContext};
    use std::{
        collections::{BTreeMap, VecDeque},
        time::Instant,
    };

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ViewRenderMetrics {
        pub key: String,
        /// Simulation/sample timestamp of this submitted pass. Cache hits retain it.
        pub frame: u64,
        pub visibility: ViewVisibility,
        /// CPU time submitting the entire pass; this is not GPU completion time.
        pub cpu_submit_ns: u64,
        /// Last available asynchronous GPU elapsed result for this view.
        pub gpu_elapsed_ns: Option<u64>,
        pub gpu_frame: Option<u64>,
    }
    pub(crate) struct ViewProfileStart {
        key: String,
        frame: u64,
        cpu: Instant,
        query: Option<glow::Query>,
    }
    struct Pending {
        key: String,
        frame: u64,
        query: glow::Query,
    }
    pub(crate) struct ViewProfiler {
        samples: BTreeMap<String, ViewRenderMetrics>,
        pending: VecDeque<Pending>,
        active: bool,
        supported: bool,
    }
    impl ViewProfiler {
        pub(crate) fn new(gl: &glow::Context) -> Self {
            let version = gl.version();
            // WebGL/disjoint timing requires a different API. Do not invent a result.
            let supported = !version.is_embedded
                && ((version.major, version.minor) >= (3, 3)
                    || gl.supported_extensions().contains("GL_ARB_timer_query"));
            Self {
                samples: BTreeMap::new(),
                pending: VecDeque::new(),
                active: false,
                supported,
            }
        }
        pub(crate) fn samples(&self) -> Vec<ViewRenderMetrics> {
            self.samples.values().cloned().collect()
        }
        unsafe fn poll(&mut self, gl: &glow::Context) {
            // Availability is checked first; never block for a timing sample.
            let mut index = 0;
            while index < self.pending.len() {
                let pending = &self.pending[index];
                if gl.get_query_parameter_u32(pending.query, glow::QUERY_RESULT_AVAILABLE) == 0 {
                    index += 1;
                    continue;
                }
                let pending = self.pending.remove(index).unwrap();
                let elapsed = gl.get_query_parameter_u32(pending.query, glow::QUERY_RESULT);
                gl.delete_query(pending.query);
                if let Some(sample) = self.samples.get_mut(&pending.key) {
                    // The exposed GL API returns u32. Saturated samples are unavailable.
                    sample.gpu_elapsed_ns = (elapsed != u32::MAX).then_some(elapsed as u64);
                    sample.gpu_frame = sample.gpu_elapsed_ns.map(|_| pending.frame);
                }
            }
        }
        pub(crate) unsafe fn begin(
            &mut self,
            gl: &glow::Context,
            key: &str,
            frame: u64,
        ) -> ViewProfileStart {
            self.poll(gl);
            let query = if self.supported && !self.active && self.pending.len() < 8 {
                gl.create_query().ok().inspect(|&query| {
                    gl.begin_query(glow::TIME_ELAPSED, query);
                    self.active = true;
                })
            } else {
                None
            };
            ViewProfileStart {
                key: key.to_owned(),
                frame,
                cpu: Instant::now(),
                query,
            }
        }
        pub(crate) unsafe fn finish(
            &mut self,
            gl: &glow::Context,
            start: ViewProfileStart,
            visibility: ViewVisibility,
        ) {
            if let Some(query) = start.query {
                gl.end_query(glow::TIME_ELAPSED);
                self.active = false;
                self.pending.push_back(Pending {
                    key: start.key.clone(),
                    frame: start.frame,
                    query,
                });
            }
            let previous = self.samples.get(&start.key);
            let sample = ViewRenderMetrics {
                key: start.key.clone(),
                frame: start.frame,
                visibility,
                cpu_submit_ns: start.cpu.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                gpu_elapsed_ns: previous.and_then(|s| s.gpu_elapsed_ns),
                gpu_frame: previous.and_then(|s| s.gpu_frame),
            };
            self.samples.insert(start.key, sample);
        }
        pub(crate) unsafe fn destroy(&mut self, gl: &glow::Context) {
            for pending in self.pending.drain(..) {
                gl.delete_query(pending.query);
            }
            self.samples.clear();
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub use native::ViewRenderMetrics;
#[cfg(not(target_arch = "wasm32"))]
pub(super) use native::{ViewProfileStart, ViewProfiler};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn frustum_is_conservative_for_near_crossing_inside_and_unknown_bounds() {
        let catalog = DepthMeshCatalog::new(vec![Arc::from(super::super::cube())]);
        let vp = Mat4::perspective_rh_gl(1., 1., 0.1, 100.);
        let part = |position, scale| {
            (
                0,
                Mat4::from_scale_rotation_translation(scale, glam::Quat::IDENTITY, position),
                Vec3::ONE,
                0.,
            )
        };
        assert!(part_in_view(
            &catalog,
            vp,
            &part(Vec3::new(0., 0., -2.), Vec3::splat(0.2))
        ));
        assert!(!part_in_view(
            &catalog,
            vp,
            &part(Vec3::new(20., 0., -2.), Vec3::splat(0.2))
        ));
        assert!(!part_in_view(
            &catalog,
            vp,
            &part(Vec3::new(0., 0., 2.), Vec3::splat(0.2))
        ));
        assert!(part_in_view(
            &catalog,
            vp,
            &part(Vec3::ZERO, Vec3::splat(2.))
        ));
        assert!(part_in_view(
            &catalog,
            vp,
            &part(Vec3::new(0., 0., -0.1), Vec3::splat(0.15))
        ));
        assert!(part_in_view(
            &catalog,
            Mat4::NAN,
            &part(Vec3::ZERO, Vec3::ONE)
        ));
        assert!(part_in_view(
            &catalog,
            vp,
            &(usize::MAX, Mat4::IDENTITY, Vec3::ONE, 0.)
        ));
    }
    #[test]
    fn each_camera_selects_its_own_visible_set() {
        let catalog = DepthMeshCatalog::new(vec![Arc::from(super::super::cube())]);
        let projection = Mat4::perspective_rh_gl(1., 1., 0.1, 100.);
        let part = (
            0,
            Mat4::from_translation(Vec3::new(0., 0., -5.)),
            Vec3::ONE,
            0.,
        );
        let remote = projection * Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::Y);
        assert!(part_in_view(&catalog, projection, &part));
        assert!(!part_in_view(&catalog, remote, &part));
    }
}
