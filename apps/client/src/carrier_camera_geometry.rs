//! Mounted-device geometry and confirmed-parent sampling. No feed allocation,
//! source admission, electrical state inference or documentary-camera fallback.
pub(crate) const SOURCE_SHA256: &str =
    "57059d059a267da00ec2ae4d4a4dfd27393cd2280239bb241ffb751ea506b113";
pub(crate) const DEVICE_REVISION: u32 = 1;
pub(crate) const HULL_PARENT: &str = "hull.body";

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Device {
    pub id: String,
    pub source_sha256: String,
    pub revision: u32,
    /// Exact semantic source parent; future crane cameras must bind their own
    /// moving mechanism, never use the hull transform or a documentary eye.
    pub parent_id: String,
    pub eye_m: [f64; 3],
    pub look_at_m: [f64; 3],
    pub vertical_fov_radians: f64,
    pub near_m: f64,
    pub far_m: f64,
    // Retained authored fixed-source fixture geometry; crane meshes are in GLB.
    #[allow(dead_code)]
    pub geometry: Vec<BoxPart>,
}
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)] // Retained fixed-source authoring/proof API, not duplicated runtime meshes.
pub(crate) struct BoxPart {
    pub name: &'static str,
    pub center_m: [f64; 3],
    pub half_m: [f64; 3],
    /// Column basis axes, in parent-local metres. Render/color/depth/rays must
    /// all retain these opaque housing and bracket parts.
    pub axes: [[f64; 3]; 3],
}
fn fixed_geometry(eye: [f64; 3], target: [f64; 3], ceiling: bool) -> Vec<BoxPart> {
    let normalize = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let f = normalize(std::array::from_fn(|i| target[i] - eye[i]));
    let right = normalize(cross(f, [0., 1., 0.]));
    let up = cross(right, f);
    let center = std::array::from_fn(|i| eye[i] - f[i] * 0.12);
    let identity = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let mut parts = vec![BoxPart {
        name: "housing",
        center_m: center,
        half_m: [0.10, 0.065, 0.075],
        axes: [f, up, right],
    }];
    if ceiling {
        let y = 2.8499999046325684;
        parts.push(BoxPart {
            name: "mount stem",
            center_m: [center[0], (center[1] + y) / 2., center[2]],
            half_m: [0.025, (y - center[1]) / 2., 0.025],
            axes: identity,
        });
        parts.push(BoxPart {
            name: "mount plate",
            center_m: [center[0], y - 0.01, center[2]],
            half_m: [0.085, 0.01, 0.085],
            axes: identity,
        });
    } else {
        let z = eye[2].signum() * 19.5;
        parts.push(BoxPart {
            name: "mount stem",
            center_m: [center[0], center[1], (center[2] + z) / 2.],
            half_m: [0.025, 0.025, (center[2] - z).abs() / 2.],
            axes: identity,
        });
        parts.push(BoxPart {
            name: "mount plate",
            center_m: [center[0], center[1], z + z.signum() * 0.01],
            half_m: [0.085, 0.085, 0.01],
            axes: identity,
        });
    }
    parts
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Pose {
    pub position_m: [f64; 3],
    pub orientation_xyzw: [f64; 4],
}
impl Pose {
    fn transform(self, point: [f64; 3]) -> Option<[f64; 3]> {
        let q = self.orientation_xyzw;
        let n = q.iter().map(|v| v * v).sum::<f64>();
        if !self
            .position_m
            .iter()
            .chain(q.iter())
            .chain(point.iter())
            .all(|v| v.is_finite())
            || (n - 1.).abs() > 1e-6
        {
            return None;
        }
        let xyz = [q[0], q[1], q[2]];
        let t = cross(xyz, point).map(|v| v * 2.);
        let u = cross(xyz, t);
        let result = std::array::from_fn(|i| self.position_m[i] + point[i] + q[3] * t[i] + u[i]);
        result.iter().all(|v| v.is_finite()).then_some(result)
    }
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// This is supplied by an authoritative source-qualified device projection.
/// Supply is the actual valid vessel bus state, explicitly unmetered (no watts
/// claim). The existence of Device/loaded hull never synthesizes supply or health.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CameraHealth {
    /// No camera damage/sensor model exists; UI must label this honestly.
    NotSimulated,
    ReportedFault,
}
pub(crate) struct Confirmed<'a> {
    pub vessel_id: &'a str,
    pub device_id: &'a str,
    pub source_sha256: &'a str,
    pub device_revision: u32,
    pub parent_id: &'a str,
    pub parent: Pose,
    pub sampled_at_ms: u64,
    pub installed: bool,
    pub powered: bool,
    pub health: CameraHealth,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CameraFrame {
    pub eye_world_m: [f64; 3],
    pub target_world_m: [f64; 3],
    pub up_world: [f64; 3],
    pub vertical_fov_radians: f64,
    pub near_m: f64,
    pub far_m: f64,
}
impl Device {
    pub(crate) fn frame(
        &self,
        state: &Confirmed<'_>,
        now: u64,
        connected: bool,
        expected_vessel_id: &str,
    ) -> Option<CameraFrame> {
        if !connected
            || state.vessel_id != expected_vessel_id
            || !state.installed
            || !state.powered
            || state.health == CameraHealth::ReportedFault
            || state.sampled_at_ms > now
            || now - state.sampled_at_ms > 1500
            || state.device_id != self.id
            || state.source_sha256 != self.source_sha256
            || state.device_revision != self.revision
            || state.parent_id != self.parent_id
            || !self.vertical_fov_radians.is_finite()
            || !(0.1..=2.5).contains(&self.vertical_fov_radians)
            || !self.near_m.is_finite()
            || !self.far_m.is_finite()
            || self.near_m <= 0.
            || self.far_m <= self.near_m
        {
            return None;
        }
        let eye = state.parent.transform(self.eye_m)?;
        let target = state.parent.transform(self.look_at_m)?;
        if eye
            .iter()
            .zip(target)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f64>()
            < 1e-10
        {
            return None;
        }
        let up = Pose {
            position_m: [0.; 3],
            ..state.parent
        }
        .transform([0., 1., 0.])?;
        let forward = std::array::from_fn(|i| target[i] - eye[i]);
        if cross(forward, up).iter().map(|v| v * v).sum::<f64>() < 1e-10 {
            return None;
        }
        Some(CameraFrame {
            eye_world_m: eye,
            target_world_m: target,
            up_world: up,
            vertical_fov_radians: self.vertical_fov_radians,
            near_m: self.near_m,
            far_m: self.far_m,
        })
    }
}

/// Current composed source only. These are fixed devices; new local-reloader
/// camera attachments require NEW measured source-parent definitions and proofs.
#[allow(dead_code)] // Immutable1702 fixture export remains supported; runtime uses crane_devices.
pub(crate) fn fixed_devices(verified_installed_sha256: &str) -> Option<Vec<Device>> {
    if verified_installed_sha256 != SOURCE_SHA256 {
        return None;
    }
    let mut result = Vec::new();
    for (side, sign) in [("port", -1.), ("starboard", 1.)] {
        for (slot, x) in [-26., -18., -10., -2., 6.].into_iter().enumerate() {
            result.push(Device {
                id: format!("pioneer.carrier.camera.ready.{side}.{slot}"),
                source_sha256: SOURCE_SHA256.into(),
                revision: DEVICE_REVISION,
                parent_id: HULL_PARENT.into(),
                eye_m: [x, 2.5, sign * 33.3],
                look_at_m: [x, 0., sign * 25.8],
                vertical_fov_radians: 50_f64.to_radians(),
                near_m: 0.03,
                far_m: 150.,
                geometry: fixed_geometry([x, 2.5, sign * 33.3], [x, 0., sign * 25.8], true),
            });
        }
        result.push(Device {
            id: format!("pioneer.carrier.camera.gallery.{side}"),
            source_sha256: SOURCE_SHA256.into(),
            revision: DEVICE_REVISION,
            parent_id: HULL_PARENT.into(),
            eye_m: [-20., 6.8, sign * 19.8],
            look_at_m: [-26., 4.5, sign * 27.],
            vertical_fov_radians: 60_f64.to_radians(),
            near_m: 0.03,
            far_m: 100.,
            geometry: fixed_geometry([-20., 6.8, sign * 19.8], [-26., 4.5, sign * 27.], false),
        });
    }
    Some(result)
}

/// The rigid housings are already authored in the exact GLB. Do not draw a
/// second procedural copy: the shared color/depth/ray source provides them.
pub(crate) fn crane_devices(verified_installed_sha256: &str) -> Option<Vec<Device>> {
    const SOURCE: &str = "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a";
    if verified_installed_sha256 != SOURCE {
        return None;
    }
    Some(
        (0..10)
            .map(|cell| Device {
                id: format!(
                    "pioneer.carrier.camera.loader.{}.{}",
                    if cell < 5 { "port" } else { "starboard" },
                    cell % 5
                ),
                source_sha256: SOURCE.into(),
                revision: 1,
                parent_id: format!("cell{cell:02}/loader/trolley"),
                eye_m: [1., 7.7, 0.],
                look_at_m: [0., 3.5, 0.],
                vertical_fov_radians: 110_f64.to_radians(),
                near_m: 0.03,
                far_m: 100.,
                geometry: vec![],
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn identity() -> Pose {
        Pose {
            position_m: [0.; 3],
            orientation_xyzw: [0., 0., 0., 1.],
        }
    }
    fn state(d: &Device) -> Confirmed<'_> {
        Confirmed {
            vessel_id: "owned",
            device_id: &d.id,
            source_sha256: &d.source_sha256,
            device_revision: d.revision,
            parent_id: &d.parent_id,
            parent: identity(),
            sampled_at_ms: 1000,
            installed: true,
            powered: true,
            health: CameraHealth::NotSimulated,
        }
    }
    #[test]
    fn source_exact_unique_identity_and_no_obsolete_crane_attachment() {
        assert!(fixed_devices("1702").is_none());
        let devices = fixed_devices(SOURCE_SHA256).unwrap();
        assert_eq!(devices.len(), 12);
        let ids: std::collections::BTreeSet<_> = devices.iter().map(|d| &d.id).collect();
        assert_eq!(ids.len(), 12);
        assert!(devices.iter().all(|d| d.parent_id == HULL_PARENT));
        for d in &devices {
            assert!(d.frame(&state(d), 1000, true, "owned").is_some());
        }
    }
    #[test]
    fn missing_supply_health_binding_and_stale_samples_fail_closed() {
        let d = &fixed_devices(SOURCE_SHA256).unwrap()[0];
        for case in 0..9 {
            let mut s = state(d);
            match case {
                0 => s.installed = false,
                1 => s.powered = false,
                2 => s.health = CameraHealth::ReportedFault,
                3 => s.device_id = "different",
                4 => s.parent_id = "wrong.crane",
                5 => s.source_sha256 = "different",
                6 => s.device_revision += 1,
                7 => s.sampled_at_ms = 3000,
                _ => s.vessel_id = "other",
            };
            assert!(d.frame(&s, 1000, true, "owned").is_none());
        }
        assert!(d.frame(&state(d), 2501, true, "owned").is_none());
        assert!(d.frame(&state(d), 1000, false, "owned").is_none());
    }
    #[test]
    fn parented_camera_translates_and_rotates_once_without_extrapolation() {
        let mut d = fixed_devices(SOURCE_SHA256).unwrap().remove(0);
        // Generic parent contract test, NOT a proved crane hardware attachment.
        d.parent_id = "test.moving.source.parent".into();
        d.eye_m = [1., 0., 0.];
        d.look_at_m = [2., 0., 0.];
        let mut s = state(&d);
        s.parent = Pose {
            position_m: [10., 20., 30.],
            orientation_xyzw: [
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
        };
        let f = d.frame(&s, 1000, true, "owned").unwrap();
        for (a, b) in f.eye_world_m.into_iter().zip([10., 21., 30.]) {
            assert!((a - b).abs() < 1e-10);
        }
        assert_eq!(
            d.frame(&s, 1000, true, "owned"),
            d.frame(&s, 1100, true, "owned")
        );
        assert!((f.up_world[0] + 1.).abs() < 1e-10);
        s.parent.orientation_xyzw = [0.; 4];
        assert!(d.frame(&s, 1000, true, "owned").is_none());
    }
    #[test]
    fn exact_parallel_source_has_ten_real_moving_parents_without_duplicate_meshes() {
        assert!(crane_devices(SOURCE_SHA256).is_none());
        let devices =
            crane_devices("79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a")
                .unwrap();
        assert_eq!(devices.len(), 10);
        for (cell, d) in devices.iter().enumerate() {
            assert_eq!(d.parent_id, format!("cell{cell:02}/loader/trolley"));
            assert!(d.geometry.is_empty());
            assert!(d.frame(&state(d), 1000, true, "owned").is_some());
            let mut wrong = state(d);
            wrong.parent_id = HULL_PARENT;
            assert!(d.frame(&wrong, 1000, true, "owned").is_none());
        }
    }
}
