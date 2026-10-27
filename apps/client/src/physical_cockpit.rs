//! Confirmed physical display mechanisms. The authority clock, not UI time,
//! drives hardware. Source captures and their destination transforms are separate.
use super::cockpit_occlusion::DepthDraw;
use super::*;
use arkline_protocol::PioneerCockpitMechanisms;

/// Nine dedicated views plus the unchanged four-view manual instrument.
pub(super) const MAX_CAPTURE_SOURCES: usize = 13;

#[derive(Default)]
pub(super) struct PoseSampler {
    epoch: u64,
    owner: String,
    vessel: String,
    snapshot_ms: Option<u64>,
    state: Option<PioneerCockpitMechanisms>,
    fractions: [f64; 9],
    available: bool,
}
impl PoseSampler {
    /// Called on every presentation frame as well as each accepted FIFO receipt.
    /// `now` is the authority presentation clock (frozen when paused), never UI time.
    pub fn reset_session(&mut self) {
        *self = Self {
            epoch: self.epoch.wrapping_add(1),
            ..Default::default()
        };
    }
    pub fn capture_identity(&self, source: u64) -> u64 {
        egui::Id::new((self.epoch, source)).value()
    }
    pub fn disconnect(&mut self) {
        self.available = false;
    }
    pub fn observe(&mut self, snapshot: &Snapshot, owner: &str, connected: bool, now: u64) {
        if self
            .snapshot_ms
            .is_some_and(|ms| snapshot.server_time_ms < ms)
        {
            return;
        }
        let pilot = snapshot.pilots.iter().find(|p| p.name == owner);
        let state = pilot
            .filter(|p| p.ship_class == ShipClass::Pioneer)
            .and_then(|p| p.cockpit_mechanisms.as_ref());
        let vessel = state.map_or("", |s| s.vessel_id.as_str());
        if self.owner != owner || self.vessel != vessel {
            *self = Self {
                epoch: self.epoch.wrapping_add(1),
                owner: owner.into(),
                vessel: vessel.into(),
                ..Default::default()
            };
        }
        // Distinct equal-time payloads apply in FIFO receipt order. There is no
        // general protocol revision that could detect arbitrary reordering.
        self.snapshot_ms = Some(snapshot.server_time_ms);
        self.state = state.cloned();
        self.available = connected && now.saturating_sub(snapshot.server_time_ms) <= 1500;
        if self.available {
            self.fractions = state.map_or([0.; 9], |s| s.all_fractions_at(now));
        }
    }
    pub fn fractions(&self) -> [f64; 9] {
        self.fractions
    }
    pub fn state(&self) -> Option<&PioneerCockpitMechanisms> {
        self.state.as_ref()
    }
}

pub(super) struct DisplayFeed<'a> {
    pub surface: CockpitSurface,
    pub screen: egui::Rect,
    pub frame: Frame,
    pub drone: Option<&'a arkline_protocol::DroneView>,
    pub turret: Option<u8>,
    pub exterior: Option<arkline_protocol::ExteriorCameraSide>,
    pub key: u64,
    #[cfg(test)]
    pub source_key: u64,
    pub primary: bool,
    #[cfg(test)]
    pub bay: usize,
    pub rig_status: Option<String>,
}
/// Exact capture dimensions distinguish a single view from a same-aspect tile.
/// Physical position/fraction is deliberately NOT part of this identity.
pub(super) fn capture_key(source: u64, screen: egui::Rect) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hash);
    feed_resolution(
        screen.width().round() as i32,
        screen.height().round() as i32,
    )
    .hash(&mut hash);
    screen.aspect_ratio().to_bits().hash(&mut hash);
    hash.finish()
}
fn drone_frame(frame: &Frame, drone: &arkline_protocol::DroneView, aspect: f32) -> Frame {
    let mut feed = frame.clone();
    let (eye, vp) = drone_mode_matrix(drone, frame.space, aspect, DroneCameraMode::Pov, frame.now);
    feed.eye = eye;
    feed.vp = vp;
    feed.planet_vp = Mat4::perspective_rh_gl(1.05, aspect.max(0.1), 1., 300_000.)
        * Mat4::perspective_rh_gl(1.05, aspect.max(0.1), 0.002, 1200.).inverse()
        * vp;
    feed.cockpit = false;
    feed.drone_camera = true;
    feed.camera_drone = Some(drone.id);
    feed.docking = None;
    feed
}
fn exterior_source_key(vessel_id: &str, side: arkline_protocol::ExteriorCameraSide) -> u64 {
    egui::Id::new(("exterior-camera", vessel_id, side.component_id())).value()
}

pub(super) fn append_feeds<'a>(
    surfaces: &[Option<CockpitSurface>; COCKPIT_SURFACE_SLOTS],
    feeds: &mut Vec<DisplayFeed<'a>>,
    renderer: &Arc<Mutex<Renderer>>,
    frame: &Frame,
    own: &'a PilotView,
    pose: &PoseSampler,
) {
    if !pose.available {
        return;
    }
    let Some(state) = pose.state() else {
        return;
    };
    for slot in 0..9 {
        if pose.fractions[slot] <= 0. {
            continue;
        }
        let Some(surface) =
            surfaces[arkline_protocol::cockpit_mechanism_surface_slot(slot).unwrap()].as_ref()
        else {
            continue;
        };
        let screen = surface.canvas_rect().shrink(5.);
        let exterior = arkline_protocol::ExteriorCameraSide::from_hardware_slot(slot);
        let (feed, drone, turret) = if let Some(side) = exterior {
            let Some(feed) = exterior_camera_frame(frame, own, side, screen.aspect_ratio()) else {
                continue;
            };
            (feed, None, None)
        } else if slot < 5 {
            let Some(drone) =
                state.drone_ids[slot].and_then(|id| fleet(own).into_iter().find(|d| d.id == id))
            else {
                continue;
            };
            let Some(sampled) =
                drone_bay_scene::camera_sample(own, drone, frame.authoritative_motion_time())
            else {
                continue;
            };
            (
                drone_frame(frame, &sampled, screen.aspect_ratio()),
                Some(drone),
                None,
            )
        } else {
            let turret = (slot - 5) as u8;
            let rotation = renderer.lock().ok().and_then(|r| {
                r.turrets
                    .get(&(own.name.clone(), turret))
                    .map(|s| s.rotation)
            });
            let Some(feed) = rotation.and_then(|rotation| {
                turret_camera_frame(frame, own, turret, rotation, screen.aspect_ratio())
            }) else {
                continue;
            };
            (feed, None, Some(turret))
        };
        let source_key = exterior.map_or_else(
            || {
                camera_source_key(
                    &own.name,
                    drone.map(|d| d.id),
                    None,
                    DroneCameraMode::Pov,
                    turret,
                )
            },
            |side| exterior_source_key(&state.vessel_id, side),
        );
        feeds.push(DisplayFeed {
            surface: surface.clone(),
            screen,
            frame: feed,
            drone,
            turret,
            exterior,
            key: capture_key(pose.capture_identity(source_key), screen),
            #[cfg(test)]
            source_key,
            primary: false,
            #[cfg(test)]
            bay: arkline_protocol::cockpit_mechanism_surface_slot(slot).unwrap(),
            rig_status: None,
        });
    }
    debug_assert!(feeds.len() <= MAX_CAPTURE_SOURCES);
}
pub(super) fn backgrounds(
    ui: &egui::Ui,
    surfaces: &[Option<CockpitSurface>; COCKPIT_SURFACE_SLOTS],
    pose: &PoseSampler,
    own: &PilotView,
    now: u64,
    authority: &str,
) {
    for slot in 0..9 {
        if pose.fractions[slot] <= 0. {
            continue;
        }
        let Some(surface) =
            &surfaces[arkline_protocol::cockpit_mechanism_surface_slot(slot).unwrap()]
        else {
            continue;
        };
        let state = pose.state();
        let source = if slot < 5 {
            state.and_then(|s| s.drone_ids[slot]).map_or_else(
                || "DRONE UNASSIGNED".into(),
                |id| format!("DRONE #{id} / POV"),
            )
        } else if let Some(side) = arkline_protocol::ExteriorCameraSide::from_hardware_slot(slot) {
            exterior_camera_label(side).into()
        } else {
            format!("TURRET {} / POV", slot - 4)
        };
        let status = if !pose.available {
            authority.to_owned()
        } else if let Some(reason) = state
            .and_then(|s| s.screen(slot))
            .and_then(|s| s.blocked.as_ref())
        {
            format!("MOTION BLOCKED / {reason}")
        } else if state.is_some_and(|s| s.warp_stowed) {
            "WARP / STOWING".into()
        } else if slot >= 7 {
            if own.flight.is_some() {
                "WAITING FOR IMAGE"
            } else {
                "EXTERIOR VIEW UNAVAILABLE"
            }
            .into()
        } else if slot >= 5 {
            module_instrument(own, (slot - 5) as u8, now)
        } else {
            "WAITING FOR IMAGE".into()
        };
        surface.paint(ui, |p| {
            let rect = surface.canvas_rect();
            p.rect_filled(rect, 0., egui::Color32::from_rgb(2, 10, 12));
            p.text(
                rect.center() - egui::vec2(0., 10.),
                egui::Align2::CENTER_CENTER,
                &source,
                egui::FontId::monospace(13.),
                super::super::TEAL,
            );
            p.text(
                rect.center() + egui::vec2(0., 10.),
                egui::Align2::CENTER_CENTER,
                &status,
                egui::FontId::monospace(10.),
                super::super::AMBER,
            );
        });
    }
    if pose.state().is_some_and(|s| s.warp_stowed) {
        if let Some(surface) = &surfaces[7] {
            surface.paint(ui, |p| {
                let rect = surface.canvas_rect();
                p.text(
                    rect.center_bottom() - egui::vec2(0., 6.),
                    egui::Align2::CENTER_BOTTOM,
                    warp_status(pose.state().unwrap(), pose.fractions()),
                    egui::FontId::monospace(10.),
                    super::super::AMBER,
                );
            });
        }
    }
}

fn warp_status(state: &PioneerCockpitMechanisms, fractions: [f64; 9]) -> &'static str {
    if fractions.iter().all(|f| *f <= 1e-6) {
        "WARP MODE / CAMERA DISPLAYS STOWED"
    } else if state
        .iter_screens()
        .zip(fractions)
        .any(|(a, f)| f > 1e-6 && a.blocked.is_some())
    {
        "WARP MODE / DISPLAY STOW BLOCKED"
    } else {
        "WARP MODE / CAMERA DISPLAYS STOWING"
    }
}

/// World labels are 2D annotations, not part of the camera image. Conservatively
/// suppress the entire annotation footprint if it would cross any actual panel
/// bounds from this frame. This is intentionally stricter than a center-ray test:
/// a lock circle or long label may cross the bezel even when its anchor does not.
pub(super) fn world_annotation_clear(
    surfaces: &[Option<CockpitSurface>; COCKPIT_SURFACE_SLOTS],
    point: egui::Pos2,
) -> bool {
    let annotation = egui::Rect::from_center_size(point, egui::vec2(280., 130.));
    !surfaces
        .iter()
        .flatten()
        .any(|surface| surface.bounds().expand(4.).intersects(annotation))
}

/// Diagnose the real near-muzzle observation rig against the exact installed
/// opaque own-ship geometry and both rendered turret poses. Never omit occluders.
pub(super) fn annotate_turret_rigs(
    feeds: &mut [DisplayFeed<'_>],
    renderer: &Renderer,
    frame: &Frame,
    own: &PilotView,
) {
    if !feeds.iter().any(|feed| feed.turret.is_some()) {
        return;
    }
    let ship = transform(
        frame.space.position(own, frame.now),
        Vec3::ONE,
        ship_orientation_at(own, frame.now),
    );
    let mut parts: Vec<ModelPart> = if own.ship_class == ShipClass::Pioneer {
        frame
            .canonical_pioneer_parts()
            .map(|parts| {
                parts
                    .iter()
                    .filter(|p| !is_registered_glass(p.0))
                    .map(|p| (p.0, frame.pioneer_body_mount() * p.1, p.2, p.3))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        cached_ship_parts(own.ship_class)
            .iter()
            .map(|p| (p.0, ship * p.1, p.2, p.3))
            .collect()
    };
    for slot in 0..2 {
        if let Some(state) = renderer.turrets.get(&(own.name.clone(), slot)) {
            parts.extend(
                parallel_turret_pose::resolve(
                    own,
                    frame.space,
                    frame.snapshot_time,
                    frame.now,
                    slot,
                    state.rotation,
                    false,
                    frame.scene.carrier_inventory_registration.as_ref(),
                )
                .into_iter()
                .flat_map(|r| r.parts),
            );
        }
    }
    let catalog = renderer.depth_meshes.clone();
    let draws: Option<Vec<_>> = parts
        .iter()
        .map(|p| DepthDraw::whole(&catalog, p.0, p.1))
        .collect();
    let Some(scene) =
        draws.and_then(|draws| CockpitOcclusion::new(own.ship_class, catalog, vec![], draws))
    else {
        return;
    };
    let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1., 1.));
    for feed in feeds.iter_mut().filter(|feed| feed.turret.is_some()) {
        let inverse = feed.frame.vp.inverse();
        let mut blocked = 0;
        for y in [-0.25, 0., 0.25] {
            for x in [-0.25, 0., 0.25] {
                let far = inverse.project_point3(Vec3::new(x, y, 0.9));
                let direction = (far - feed.frame.eye).normalize_or_zero();
                let point = feed.frame.eye + direction * 2.;
                let pointer = egui::pos2((x + 1.) * 0.5, (1. - y) * 0.5);
                if !scene.point_unblocked(feed.frame.vp, viewport, pointer, point) {
                    blocked += 1;
                }
            }
        }
        if blocked > 0 {
            feed.rig_status = Some(format!("CURRENT RIG OBSTRUCTED / SHIP {blocked}/9"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exterior_source_identity_is_vessel_component_not_drone_or_pilot() {
        use arkline_protocol::ExteriorCameraSide::{Port, Starboard};
        let port = exterior_source_key("owned-vessel", Port);
        assert_eq!(port, exterior_source_key("owned-vessel", Port));
        assert_ne!(port, exterior_source_key("owned-vessel", Starboard));
        assert_ne!(port, exterior_source_key("another-vessel", Port));
        for id in 0..15 {
            assert_ne!(
                port,
                camera_source_key("owned-vessel", Some(id), None, DroneCameraMode::Pov, None)
            );
        }
    }

    #[test]
    fn capture_identity_distinguishes_same_aspect_resize_not_position() {
        let a = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320., 180.));
        let b = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160., 90.));
        assert_ne!(capture_key(10, a), capture_key(10, b));
        assert_eq!(
            capture_key(10, a),
            capture_key(10, a.translate(egui::vec2(20., 30.)))
        );
        assert_ne!(capture_key(10, a), capture_key(11, a));
    }
    fn snapshot(ms: u64, state: Option<PioneerCockpitMechanisms>) -> Snapshot {
        serde_json::from_value(serde_json::json!({"server_time_ms":ms,
            "pilots":[{"name":"owner","credits":0,"ore":0,"location":Location::Belt,"ship_class":ShipClass::Pioneer,"cockpit_mechanisms":state}],
            "orders":[],"belt_ore":0,"ledger_sequence":0,"ledger_hash":"pure-fixture"})).unwrap()
    }
    fn deploying() -> PioneerCockpitMechanisms {
        let mut s = PioneerCockpitMechanisms::stowed("owned-vessel".into(), 1000);
        s.drone_ids = [Some(42), Some(43), Some(44), Some(45), Some(46)];
        for a in s.iter_screens_mut() {
            a.target_deployed = true;
            a.duration_ms = 1200;
        }
        s
    }
    #[test]
    fn physical_pose_uses_authority_time_freezes_stale_disconnect_and_pause() {
        let mut pose = PoseSampler::default();
        let s = snapshot(1000, Some(deploying()));
        pose.observe(&s, "owner", true, 1600);
        assert_eq!(pose.fractions(), [0.5; 9]);
        for _ in 0..3 {
            pose.observe(&s, "owner", true, 1600);
        }
        assert_eq!(pose.fractions(), [0.5; 9]);
        pose.observe(&s, "owner", false, 2000);
        assert_eq!(pose.fractions(), [0.5; 9]);
        pose.observe(&s, "owner", true, 9000);
        assert_eq!(pose.fractions(), [0.5; 9]);
        pose.observe(&snapshot(2200, Some(deploying())), "owner", true, 2200);
        assert_eq!(pose.fractions(), [1.; 9]);
    }
    #[test]
    fn equal_time_reversal_blocked_state_and_context_reconcile_without_launch_replay() {
        let mut pose = PoseSampler::default();
        let a = deploying();
        pose.observe(&snapshot(1600, Some(a.clone())), "owner", true, 1600);
        let mut b = a.clone();
        b.warp_stowed = true;
        for actuator in b.iter_screens_mut() {
            actuator.from_fraction = 0.5;
            actuator.target_deployed = false;
            actuator.started_at_ms = 1600;
            actuator.duration_ms = 600;
        }
        pose.observe(&snapshot(1600, Some(b.clone())), "owner", true, 1900);
        assert_eq!(pose.fractions(), [0.25; 9]);
        pose.observe(&snapshot(1600, Some(a.clone())), "owner", true, 1900);
        assert_eq!(
            pose.fractions(),
            [0.75; 9],
            "A→B→A at same timestamp is legitimate"
        );
        b.screens[0].blocked = Some("sweep occupied".into());
        pose.observe(&snapshot(2000, Some(b)), "owner", true, 2200);
        assert_eq!(pose.fractions()[0], 0.5);
        pose.observe(&snapshot(1500, Some(a)), "owner", true, 2200);
        assert_eq!(pose.fractions()[0], 0.5, "lower timestamp rejected");
        pose.observe(&snapshot(2200, None), "owner", true, 2200);
        assert_eq!(pose.fractions(), [0.; 9]);
    }

    #[test]
    fn warp_target_does_not_claim_stowed_during_travel_or_blockage() {
        let mut s = deploying();
        s.warp_stowed = true;
        assert!(warp_status(&s, [0.5; 9]).ends_with("STOWING"));
        s.screens[0].blocked = Some("occupied".into());
        assert!(warp_status(&s, [0.5; 9]).ends_with("BLOCKED"));
        assert!(warp_status(&s, [0.; 9]).ends_with("STOWED"));
    }

    #[test]
    fn reconnect_and_vessel_changes_cannot_reuse_old_world_pixels() {
        let mut pose = PoseSampler::default();
        let snapshot = snapshot(1000, Some(deploying()));
        pose.observe(&snapshot, "owner", true, 1600);
        let first = pose.capture_identity(42);
        pose.observe(&snapshot, "owner", true, 1800);
        assert_eq!(first, pose.capture_identity(42));
        pose.reset_session();
        pose.observe(&snapshot, "owner", true, 1800);
        assert_ne!(first, pose.capture_identity(42));
        let second = pose.capture_identity(42);
        let mut other = snapshot.clone();
        other.pilots[0]
            .cockpit_mechanisms
            .as_mut()
            .unwrap()
            .vessel_id = "other".into();
        pose.observe(&other, "owner", true, 1800);
        assert_ne!(second, pose.capture_identity(42));
    }
}
