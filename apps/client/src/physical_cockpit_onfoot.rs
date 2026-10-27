//! The same source captures painted on the same moving physical planes while
//! walking in flight. This adds no input or command authority to those faces.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    ui: &egui::Ui,
    viewport: egui::Rect,
    surfaces: &[Option<CockpitSurface>; COCKPIT_SURFACE_SLOTS],
    renderer: &Arc<Mutex<Renderer>>,
    frame: &Frame,
    own: &PilotView,
    pose: &physical_cockpit::PoseSampler,
) {
    let mut feeds = Vec::new();
    physical_cockpit::append_feeds(surfaces, &mut feeds, renderer, frame, own, pose);
    if let Ok(renderer) = renderer.lock() {
        physical_cockpit::annotate_turret_rigs(&mut feeds, &renderer, frame, own);
    }
    let fresh = frame.connected && frame.now.saturating_sub(frame.snapshot_time) <= 1500;
    let capture_pass = ui.ctx().cumulative_pass_nr();
    let capture_ui_ms = ui.input(|input| (input.time.max(0.) * 1000.) as u64);
    let mut keys: Vec<_> = feeds
        .iter()
        .filter(|feed| {
            feed.drone.is_none_or(|drone| drone.signal_strength > 0.)
                && feed.surface.feed_geometry(feed.screen).is_some()
        })
        .map(|feed| feed.key)
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let keys = Arc::new(keys);
    #[cfg(test)]
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("physical_cockpit_onfoot_proof"), serde_json::json!({
        "snapshot_ms": frame.snapshot_time, "fractions": pose.fractions(), "state": pose.state(),
        "surfaces": surfaces.iter().enumerate().filter_map(|(index, surface)| surface.as_ref().map(|_| index)).collect::<Vec<_>>(),
        "turret_poses": renderer.lock().unwrap().turrets.keys().collect::<Vec<_>>(),
        "feeds": feeds.iter().map(|feed| serde_json::json!({
            "key": feed.key, "source_key": feed.source_key, "bay": feed.bay,
            "drone": feed.drone.map(|drone| drone.id), "turret": feed.turret, "exterior": feed.exterior.map(|s|s.component_id()),
            "nonrecursive": !feed.frame.cockpit && feed.frame.drone_camera,
            "size": feed_resolution(feed.screen.width().round() as i32, feed.screen.height().round() as i32),
        })).collect::<Vec<_>>()
    })));
    let painter = ui
        .painter()
        .with_clip_rect(viewport.intersect(ui.clip_rect()));
    for feed in feeds {
        let physical_cockpit::DisplayFeed {
            surface,
            screen,
            frame: feed,
            drone,
            turret,
            exterior,
            key,
            rig_status,
            ..
        } = feed;
        let signal = drone.is_none_or(|drone| drone.signal_strength > 0.);
        if fresh && signal {
            let renderer = renderer.clone();
            let surface = surface.clone();
            let keys = keys.clone();
            #[cfg(target_arch = "wasm32")]
            let renderer = send_wrapper::SendWrapper::new(renderer);
            #[cfg(target_arch = "wasm32")]
            let surface = send_wrapper::SendWrapper::new(surface);
            painter.add(egui::PaintCallback {
                rect: viewport,
                callback: Arc::new(egui_glow::CallbackFn::new(move |info, painter| {
                    if let Ok(mut renderer) = renderer.lock() {
                        unsafe {
                            renderer.paint_cached_feed_on_surface(
                                &feed,
                                key,
                                info,
                                keys.len(),
                                painter.intermediate_fbo(),
                                Some((&surface, screen)),
                                Some((capture_pass, capture_ui_ms, keys.as_slice())),
                            );
                        }
                    }
                })),
            });
        }
        let size = feed_resolution(
            screen.width().round() as i32,
            screen.height().round() as i32,
        );
        let captured = renderer.lock().ok().and_then(|renderer| {
            renderer
                .feed_cache
                .iter()
                .find(|cache| cache.key == key && (cache.width, cache.height) == size)
                .and_then(|cache| cache.captured_ui_ms)
        });
        surface.paint(ui, |overlay| {
            let overlay = overlay.with_clip_rect(screen);
            overlay.text(
                screen.left_bottom() + egui::vec2(3., -3.),
                egui::Align2::LEFT_BOTTOM,
                capture_label(fresh && signal, captured, capture_ui_ms),
                egui::FontId::monospace(10.),
                super::super::MUTED,
            );
            overlay.text(
                screen.left_top() + egui::vec2(3., 3.),
                egui::Align2::LEFT_TOP,
                exterior.map_or_else(
                    || {
                        turret.map_or_else(
                            || format!("#{} / POV", drone.map_or(0, |drone| drone.id)),
                            |slot| format!("TURRET {} / RENDERED POV", slot + 1),
                        )
                    },
                    |side| exterior_camera_label(side).into(),
                ),
                egui::FontId::monospace(11.),
                super::super::TEAL,
            );
            if let Some(slot) = turret {
                overlay.text(
                    screen.left_top() + egui::vec2(3., 17.),
                    egui::Align2::LEFT_TOP,
                    module_instrument(own, slot, frame.now),
                    egui::FontId::monospace(10.),
                    super::super::MUTED,
                );
            }
            if let Some(status) = &rig_status {
                overlay.text(
                    screen.center_bottom() - egui::vec2(0., 17.),
                    egui::Align2::CENTER_BOTTOM,
                    status,
                    egui::FontId::monospace(10.),
                    super::super::AMBER,
                );
            }
            if let Some(drone) = drone {
                paint_camera_signal(&overlay, screen, Some(drone), frame.now, fresh);
            }
        });
    }
}

fn capture_label(available: bool, captured: Option<u64>, now: u64) -> String {
    if !available {
        "UNAVAILABLE".into()
    } else {
        captured.filter(|captured| *captured <= now).map_or_else(
            || "WARMING UP".into(),
            |captured| format!("CACHED · {} ms", now.saturating_sub(captured)),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_labels_never_claim_live_or_reuse_future_capture_time() {
        assert_eq!(capture_label(true, None, 100), "WARMING UP");
        assert_eq!(capture_label(true, Some(200), 100), "WARMING UP");
        assert_eq!(capture_label(true, Some(60), 100), "CACHED · 40 ms");
        assert_eq!(capture_label(false, Some(60), 100), "UNAVAILABLE");
        assert_eq!(capture_label(false, None, 100), "UNAVAILABLE");
    }
}
