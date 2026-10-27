//! Resolve one real mounted camera into the EXISTING workspace feed cache.
//! No textures, timers, machinery extrapolation, or documentary fallback.
use super::carrier_camera_geometry::{crane_devices, CameraFrame, Confirmed};
use std::hash::{Hash, Hasher};

const SOURCE: &str = "79ab43221861f74383ca36ca6a742ff5588e08003eb7b10f9d8b6f0d5469664a";
const DEFINITION: &str = "425597be16b9c252d0071d5ab1e4d2afb03d5905dc0b2fbe99fe955c94824f0d";

pub(crate) struct Installed<'a> {
    pub vessel_id: &'a str,
    pub assembly_revision: u32,
    /// From verified installed bytes AND the matching recorded registration.
    pub source_sha256: &'a str,
    pub definition_sha256: &'a str,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unavailable {
    Hidden,
    Source,
    Device,
    SampleClock,
    Dimensions,
    ParentPowerOrConnection,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ResolvedFeed {
    pub camera: CameraFrame,
    pub capture_key: u64,
    /// Set Frame.now AND register_clocks to this value. All source/body motion
    /// must use the same snapshot as the confirmed parent, not presentation now.
    pub scene_time_ms: u64,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Resolution {
    pub feed: Option<ResolvedFeed>,
    /// Destroy/retire this entry through the existing renderer. Never keep
    /// painting an old LIVE image after source, supply, session or device loss.
    pub retire_key: Option<u64>,
    pub unavailable: Option<Unavailable>,
    /// Invalidate painted time for a changed equal-time confirmed pose without
    /// reallocating the cache. The seam must ALSO fingerprint full-scene receipts.
    pub invalidate_pixels: bool,
}
#[derive(Default)]
pub(crate) struct Resolver {
    epoch: u64,
    active: Option<u64>,
    clock_identity: Option<u64>,
    latest_sample_ms: Option<u64>,
    last_frame: Option<CameraFrame>,
}
impl Resolver {
    fn unavailable(&mut self, reason: Unavailable) -> Resolution {
        let retire_key = self.active.take();
        if retire_key.is_some() {
            self.epoch = self.epoch.wrapping_add(1);
        }
        Resolution {
            feed: None,
            retire_key,
            unavailable: Some(reason),
            invalidate_pixels: false,
        }
    }
    /// Call when the network/world session resets, even if the next world has
    /// identical source/vessel/device IDs and a reset simulation clock.
    pub(crate) fn reset_session(&mut self) -> Option<u64> {
        self.epoch = self.epoch.wrapping_add(1);
        self.clock_identity = None;
        self.latest_sample_ms = None;
        self.last_frame = None;
        self.active.take()
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve(
        &mut self,
        installed: &Installed<'_>,
        selected_device: &str,
        state: Option<&Confirmed<'_>>,
        scene_snapshot_ms: u64,
        now_ms: u64,
        connected: bool,
        visible: bool,
        dimensions: [u32; 2],
    ) -> Resolution {
        if installed.assembly_revision != 1703
            || installed.source_sha256 != SOURCE
            || installed.definition_sha256 != DEFINITION
            || installed.vessel_id.is_empty()
        {
            return self.unavailable(Unavailable::Source);
        }
        // This clock is the independently accepted scene snapshot. Remember it
        // even when that newer snapshot reports missing power/parent/camera.
        // Otherwise an older powered sample could resurrect invalidated pixels.
        if scene_snapshot_ms > now_ms {
            return self.unavailable(Unavailable::SampleClock);
        }
        let mut clock_hash = std::collections::hash_map::DefaultHasher::new();
        (
            installed.vessel_id,
            installed.source_sha256,
            installed.definition_sha256,
        )
            .hash(&mut clock_hash);
        let clock_identity = clock_hash.finish();
        let same_identity = self.clock_identity == Some(clock_identity);
        if same_identity
            && self
                .latest_sample_ms
                .is_some_and(|latest| scene_snapshot_ms < latest)
        {
            return self.unavailable(Unavailable::SampleClock);
        }
        let equal_time = same_identity && self.latest_sample_ms == Some(scene_snapshot_ms);
        self.clock_identity = Some(clock_identity);
        self.latest_sample_ms = Some(scene_snapshot_ms);
        if !visible {
            return self.unavailable(Unavailable::Hidden);
        }
        // Feed-resolution policy remains with the existing renderer. Refuse
        // nonsensical/extreme inputs; this resolver never allocates a target.
        if dimensions.contains(&0) || dimensions.iter().any(|d| *d > 8192) {
            return self.unavailable(Unavailable::Dimensions);
        }
        let Some(device) = crane_devices(installed.source_sha256)
            .and_then(|devices| devices.into_iter().find(|d| d.id == selected_device))
        else {
            return self.unavailable(Unavailable::Device);
        };
        let Some(state) = state else {
            return self.unavailable(Unavailable::ParentPowerOrConnection);
        };
        if state.sampled_at_ms != scene_snapshot_ms {
            return self.unavailable(Unavailable::SampleClock);
        }
        let Some(camera) = device.frame(state, now_ms, connected, installed.vessel_id) else {
            return self.unavailable(Unavailable::ParentPowerOrConnection);
        };
        let equal_time_changed =
            equal_time && self.last_frame.is_some_and(|previous| previous != camera);
        self.last_frame = Some(camera);
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        (
            "carrier-mounted-workspace",
            self.epoch,
            installed.vessel_id,
            installed.source_sha256,
            installed.definition_sha256,
            &device.id,
            device.revision,
            &device.parent_id,
            dimensions,
        )
            .hash(&mut hash);
        let capture_key = hash.finish();
        let invalidate_pixels = equal_time_changed && self.active == Some(capture_key);
        let retire_key = self.active.filter(|key| *key != capture_key);
        self.active = Some(capture_key);
        Resolution {
            feed: Some(ResolvedFeed {
                camera,
                capture_key,
                scene_time_ms: state.sampled_at_ms,
                width: dimensions[0],
                height: dimensions[1],
            }),
            retire_key,
            unavailable: None,
            invalidate_pixels,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::carrier_camera_geometry::{CameraHealth, Pose};
    use super::*;
    fn installed() -> Installed<'static> {
        Installed {
            vessel_id: "owned",
            assembly_revision: 1703,
            source_sha256: SOURCE,
            definition_sha256: DEFINITION,
        }
    }
    fn state() -> Confirmed<'static> {
        Confirmed {
            vessel_id: "owned",
            device_id: "pioneer.carrier.camera.loader.port.0",
            source_sha256: SOURCE,
            device_revision: 1,
            parent_id: "cell00/loader/trolley",
            parent: Pose {
                position_m: [10., 0., 20.],
                orientation_xyzw: [0., 0., 0., 1.],
            },
            sampled_at_ms: 1000,
            installed: true,
            powered: true,
            health: CameraHealth::NotSimulated,
        }
    }
    fn resolve(r: &mut Resolver, s: &Confirmed<'_>, now: u64) -> Resolution {
        r.resolve(
            &installed(),
            s.device_id,
            Some(s),
            s.sampled_at_ms,
            now,
            true,
            true,
            [640, 360],
        )
    }
    #[test]
    fn actual_motion_reuses_one_key_and_same_scene_sample_clock() {
        let mut r = Resolver::default();
        let mut s = state();
        let first = resolve(&mut r, &s, 1000).feed.unwrap();
        s.sampled_at_ms = 1100;
        s.parent.position_m[2] += 3.;
        let next = resolve(&mut r, &s, 1150);
        assert_eq!(next.feed.unwrap().capture_key, first.capture_key);
        assert_eq!(next.retire_key, None);
        assert_eq!(next.feed.unwrap().scene_time_ms, 1100);
        assert_eq!(
            next.feed.unwrap().camera.eye_world_m[2],
            first.camera.eye_world_m[2] + 3.
        );
    }
    #[test]
    fn power_loss_disconnect_stale_and_reconnect_cannot_reuse_live_pixels() {
        for failure in 0..3 {
            let mut r = Resolver::default();
            let mut s = state();
            let key = resolve(&mut r, &s, 1000).feed.unwrap().capture_key;
            if failure == 0 {
                s.powered = false;
            }
            let out = r.resolve(
                &installed(),
                s.device_id,
                Some(&s),
                1000,
                if failure == 2 { 2501 } else { 1000 },
                failure != 1,
                true,
                [640, 360],
            );
            assert!(out.feed.is_none());
            assert_eq!(out.retire_key, Some(key));
            s.powered = true;
            assert_ne!(resolve(&mut r, &s, 1000).feed.unwrap().capture_key, key);
        }
    }
    #[test]
    fn selection_resize_hidden_and_world_reset_retire_only_previous_entry() {
        let mut r = Resolver::default();
        let mut s = state();
        let key = resolve(&mut r, &s, 1000).feed.unwrap().capture_key;
        s.device_id = "pioneer.carrier.camera.loader.starboard.4";
        s.parent_id = "cell09/loader/trolley";
        let changed = resolve(&mut r, &s, 1000);
        assert_eq!(changed.retire_key, Some(key));
        let key = changed.feed.unwrap().capture_key;
        let resized = r.resolve(
            &installed(),
            s.device_id,
            Some(&s),
            1000,
            1000,
            true,
            true,
            [800, 450],
        );
        assert_eq!(resized.retire_key, Some(key));
        let key = resized.feed.unwrap().capture_key;
        assert_eq!(r.reset_session(), Some(key));
        assert_ne!(resolve(&mut r, &s, 1000).feed.unwrap().capture_key, key);
        let hidden = r.resolve(
            &installed(),
            s.device_id,
            Some(&s),
            1000,
            1000,
            true,
            false,
            [640, 360],
        );
        assert!(hidden.feed.is_none());
        assert!(hidden.retire_key.is_some());
    }
    #[test]
    fn mismatched_source_definition_vessel_parent_and_snapshot_are_unavailable() {
        let mut r = Resolver::default();
        let s = state();
        let mut binding = installed();
        binding.definition_sha256 = "other";
        assert_eq!(
            r.resolve(
                &binding,
                s.device_id,
                Some(&s),
                1000,
                1000,
                true,
                true,
                [640, 360]
            )
            .unavailable,
            Some(Unavailable::Source)
        );
        assert_eq!(
            r.resolve(
                &installed(),
                s.device_id,
                Some(&s),
                999,
                1000,
                true,
                true,
                [640, 360]
            )
            .unavailable,
            Some(Unavailable::SampleClock)
        );
        assert!(r
            .resolve(
                &installed(),
                "documentary",
                Some(&s),
                1000,
                1000,
                true,
                true,
                [640, 360]
            )
            .feed
            .is_none());
        let mut wrong = state();
        wrong.vessel_id = "other";
        assert!(resolve(&mut r, &wrong, 1000).feed.is_none());
        wrong = state();
        wrong.parent_id = "hull.body";
        assert!(resolve(&mut r, &wrong, 1000).feed.is_none());
    }
    #[test]
    fn out_of_order_snapshot_retires_feed_but_equal_time_fifo_is_valid() {
        let mut r = Resolver::default();
        let mut s = state();
        let key = resolve(&mut r, &s, 1000).feed.unwrap().capture_key;
        s.parent.position_m[2] += 1.;
        assert_eq!(resolve(&mut r, &s, 1000).feed.unwrap().capture_key, key);
        s.sampled_at_ms = 900;
        assert_eq!(
            resolve(&mut r, &s, 1000).unavailable,
            Some(Unavailable::SampleClock)
        );
        r.reset_session();
        assert!(resolve(&mut r, &s, 1000).feed.is_some());
    }
    #[test]
    fn changed_equal_time_pose_invalidates_pixels_without_reallocating_or_budget_change() {
        let mut r = Resolver::default();
        let mut s = state();
        let first = resolve(&mut r, &s, 1000);
        let key = first.feed.unwrap().capture_key;
        assert!(!first.invalidate_pixels);
        assert!(!resolve(&mut r, &s, 1000).invalidate_pixels);
        s.parent.position_m[0] += 0.25;
        let changed = resolve(&mut r, &s, 1000);
        assert!(changed.invalidate_pixels);
        assert_eq!(changed.feed.unwrap().capture_key, key);
        assert_eq!(changed.retire_key, None);
        assert!(!resolve(&mut r, &s, 1000).invalidate_pixels);
        s.powered = false;
        let off = resolve(&mut r, &s, 1000);
        assert_eq!(off.retire_key, Some(key));
        assert!(off.feed.is_none());
        s.powered = true;
        assert_ne!(resolve(&mut r, &s, 1000).feed.unwrap().capture_key, key);
    }
    #[test]
    fn newer_unavailable_scene_prevents_older_powered_or_parent_sample_resurrection() {
        for failure in 0..4 {
            let mut r = Resolver::default();
            let mut s = state();
            assert!(resolve(&mut r, &s, 1000).feed.is_some());
            s.sampled_at_ms = 1100;
            if failure == 0 {
                s.powered = false;
            }
            if failure == 1 {
                s.parent.orientation_xyzw = [0.; 4];
            }
            let newer = r.resolve(
                &installed(),
                s.device_id,
                if failure == 2 { None } else { Some(&s) },
                1100,
                1100,
                failure != 3,
                true,
                [640, 360],
            );
            assert!(newer.feed.is_none());
            assert!(newer.retire_key.is_some());
            let old = state();
            assert_eq!(
                resolve(&mut r, &old, 1100).unavailable,
                Some(Unavailable::SampleClock)
            );
            r.reset_session();
            assert!(resolve(&mut r, &old, 1100).feed.is_some());
        }
    }
    #[test]
    fn future_scene_clock_cannot_poison_next_valid_sample() {
        let mut r = Resolver::default();
        let s = state();
        assert_eq!(
            r.resolve(
                &installed(),
                s.device_id,
                None,
                2000,
                1000,
                true,
                true,
                [640, 360]
            )
            .unavailable,
            Some(Unavailable::SampleClock)
        );
        assert!(resolve(&mut r, &s, 1000).feed.is_some());
    }
    #[test]
    fn hidden_newer_scene_does_not_allow_old_pixels_on_reopening() {
        let mut r = Resolver::default();
        let s = state();
        assert!(resolve(&mut r, &s, 1000).feed.is_some());
        assert_eq!(
            r.resolve(
                &installed(),
                s.device_id,
                None,
                1100,
                1100,
                true,
                false,
                [0, 0]
            )
            .unavailable,
            Some(Unavailable::Hidden)
        );
        assert_eq!(
            resolve(&mut r, &s, 1100).unavailable,
            Some(Unavailable::SampleClock)
        );
    }
}
