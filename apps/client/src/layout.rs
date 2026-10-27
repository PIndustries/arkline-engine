//! Local edge-relative overlay placement, independent of universe state.
use eframe::egui::{Pos2, Rect, Vec2};
#[cfg(not(target_arch = "wasm32"))]
use std::fs;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
/// Edge-relative HUD placement shared with local authored panels.
/// Inputs are validated by the owning document before use.
pub fn panel_geometry(
    bounds: Rect,
    anchor: [f32; 2],
    offset: Vec2,
    size: Vec2,
    actual: Vec2,
) -> (Pos2, Vec2) {
    let size = size
        .min(bounds.size() - Vec2::splat(30.0))
        .max(Vec2::splat(1.0));
    let actual = actual.min(bounds.size());
    let origin = bounds.min + Vec2::new(bounds.width() * anchor[0], bounds.height() * anchor[1]);
    let pos = origin + offset - Vec2::new(actual.x * anchor[0], actual.y * anchor[1]);
    (
        Pos2::new(
            pos.x.clamp(
                bounds.left(),
                (bounds.right() - actual.x).max(bounds.left()),
            ),
            pos.y
                .clamp(bounds.top(), (bounds.bottom() - actual.y).max(bounds.top())),
        ),
        size,
    )
}
/// egui 0.31 exposes the active drag ID but not a window resize response.
/// Match only its eight pinned edge/corner IDs, never text/scroll/button drags.
/// Real pointer regressions cover this contract when egui is upgraded.
pub fn window_resize_dragged(ctx: &eframe::egui::Context, layer: eframe::egui::LayerId) -> bool {
    let edge = eframe::egui::Id::new(layer).with("edge_drag");
    ctx.input(|i| i.pointer.is_decidedly_dragging())
        && ctx.dragged_id().is_some_and(|id| {
            [
                "left",
                "right",
                "top",
                "bottom",
                "left_top",
                "left_bottom",
                "right_top",
                "right_bottom",
            ]
            .iter()
            .any(|side| id == edge.with(side))
        })
}

fn unobscured_region(bounds: Rect, foreground: Rect, desired: Rect) -> Rect {
    let obstacle = foreground.intersect(bounds);
    if !obstacle.is_positive() || !desired.intersects(obstacle) {
        return bounds;
    }
    let regions = [
        Rect::from_min_max(Pos2::new(bounds.left(), obstacle.bottom()), bounds.max),
        Rect::from_min_max(bounds.min, Pos2::new(bounds.right(), obstacle.top())),
        Rect::from_min_max(Pos2::new(obstacle.right(), bounds.top()), bounds.max),
        Rect::from_min_max(bounds.min, Pos2::new(obstacle.left(), bounds.bottom())),
    ];
    regions
        .into_iter()
        .filter(|r| r.is_positive())
        .max_by(|a, b| {
            let usable = |r: &Rect| r.width() >= 260.0 && r.height() >= 180.0;
            usable(a)
                .cmp(&usable(b))
                .then_with(|| a.area().total_cmp(&b.area()))
        })
        .unwrap_or(bounds)
}

#[derive(Clone, Debug)]
struct Placement {
    anchor: [f32; 2],
    offset: Vec2,
    size: Vec2,
    actual: Vec2,
    chrome: Vec2,
    touched: bool,
    expected: Option<Pos2>,
}
pub struct Layout {
    pub generation: u64,
    pub reflow: bool,
    viewport: Option<Rect>,
    path: PathBuf,
    entries: BTreeMap<String, Placement>,
    pub pinned: bool,
    pub snap: bool,
    changed: bool,
    dragging: bool,
    retry_after: Option<web_time::Instant>,
    #[cfg(not(target_arch = "wasm32"))]
    pending_save: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
}
impl Layout {
    pub fn load(dir: &Path) -> Self {
        let mut result = Self {
            generation: 0,
            reflow: true,
            viewport: None,
            path: dir.join("layout.json"),
            entries: BTreeMap::new(),
            pinned: false,
            snap: true,
            changed: false,
            dragging: false,
            retry_after: None,
            #[cfg(not(target_arch = "wasm32"))]
            pending_save: None,
        };
        if let Ok(bytes) = read_preferences(&result.path) {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                result.pinned = v["pinned"].as_bool().unwrap_or(false);
                result.snap = v["snap"].as_bool().unwrap_or(true);
                if let Some(entries) = v["panels"].as_object() {
                    for (key, v) in entries {
                        let floats = |field: &str| -> Option<[f32; 2]> {
                            let a = v[field].as_array()?;
                            Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32])
                        };
                        if let (Some(anchor), Some(offset), Some(size)) =
                            (floats("anchor"), floats("offset"), floats("size"))
                        {
                            if anchor
                                .iter()
                                .chain(offset.iter())
                                .chain(size.iter())
                                .all(|v| v.is_finite())
                                && anchor.iter().all(|v| (0.0..=1.0).contains(v))
                                && size.iter().all(|v| *v >= 1.0 && *v < 10000.0)
                            {
                                result.entries.insert(
                                    key.clone(),
                                    Placement {
                                        anchor,
                                        offset: Vec2::from(offset),
                                        size: Vec2::from(size),
                                        actual: Vec2::from(size) + Vec2::new(16.0, 36.0),
                                        chrome: Vec2::new(16.0, 36.0),
                                        touched: false,
                                        expected: None,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }
        result
    }
    pub fn begin_frame(&mut self, bounds: Rect) {
        self.reflow = self.viewport.is_none_or(|old| old.size() != bounds.size());
        self.viewport = Some(bounds);
    }
    pub fn position(
        &mut self,
        key: &str,
        bounds: Rect,
        anchor: [f32; 2],
        offset: Vec2,
        size: Vec2,
    ) -> (Pos2, Vec2) {
        let p = self.entries.entry(key.into()).or_insert(Placement {
            anchor,
            offset,
            size,
            actual: size + Vec2::new(16.0, 36.0),
            chrome: Vec2::new(16.0, 36.0),
            touched: false,
            expected: None,
        });
        let result = panel_geometry(bounds, p.anchor, p.offset, p.size, p.actual);
        p.expected = Some(result.0);
        result
    }
    /// Constrain a task window around measured foreground authority chrome.
    /// Use the saved preferred size, not last frame's temporarily clamped size,
    /// so expanding/collapsing status cannot create a resize/placement oscillation.
    pub fn avoid_foreground(
        &mut self,
        key: &str,
        bounds: Rect,
        foreground: Rect,
        minimum_width: f32,
    ) -> (Pos2, Vec2, Rect) {
        let p = self
            .entries
            .get_mut(key)
            .expect("position must register task");
        let chrome = p.chrome;
        let preferred = p.size.max(Vec2::new(minimum_width, 1.0));
        let (desired, _) =
            panel_geometry(bounds, p.anchor, p.offset, preferred, preferred + chrome);
        let desired = Rect::from_min_size(desired, (preferred + chrome).min(bounds.size()));
        let available = unobscured_region(bounds, foreground.expand(8.0), desired);
        let size = preferred
            .min(available.size() - chrome)
            .max(Vec2::splat(1.0));
        let actual = size + chrome;
        let pos = Pos2::new(
            desired.left().clamp(
                available.left(),
                (available.right() - actual.x).max(available.left()),
            ),
            desired.top().clamp(
                available.top(),
                (available.bottom() - actual.y).max(available.top()),
            ),
        );
        p.expected = Some(pos);
        (pos, size, available)
    }

    /// Measure only window decoration, never promote a temporary content clamp
    /// into the user's preferred size. Font/theme metrics need not match defaults.
    pub fn observe_window_chrome(&mut self, key: &str, rect: Rect, content: Vec2) {
        if let Some(p) = self.entries.get_mut(key) {
            p.chrome = (rect.size() - content).max(Vec2::ZERO);
        }
    }

    pub fn record(
        &mut self,
        key: &str,
        bounds: Rect,
        rect: Rect,
        content: Option<Vec2>,
        pointer_down: bool,
    ) {
        let Some(p) = self.entries.get_mut(key) else {
            return;
        };
        let origin =
            bounds.min + Vec2::new(bounds.width() * p.anchor[0], bounds.height() * p.anchor[1]);
        let offset =
            rect.min + Vec2::new(rect.width() * p.anchor[0], rect.height() * p.anchor[1]) - origin;
        if pointer_down && !self.pinned && !self.reflow {
            if p.expected
                .is_some_and(|expected| expected.distance(rect.min) > 0.8)
                || content.is_some_and(|s| (s - p.size).length() > 1.0)
            {
                p.offset = offset;
                p.touched = true;
                if let Some(size) = content {
                    p.size = size;
                }
                self.changed = true;
                self.dragging = true;
            }
        } else if p.touched && self.snap {
            for axis in 0..2 {
                let start = if axis == 0 {
                    bounds.left()
                } else {
                    bounds.top()
                };
                let end = if axis == 0 {
                    bounds.right()
                } else {
                    bounds.bottom()
                };
                let min = if axis == 0 { rect.left() } else { rect.top() };
                let max = if axis == 0 {
                    rect.right()
                } else {
                    rect.bottom()
                };
                let center = (min + max) * 0.5;
                let candidate = if (min - start).abs() < 24.0 {
                    Some((0.0, 8.0))
                } else if (end - max).abs() < 24.0 {
                    Some((1.0, -8.0))
                } else if (center - (start + end) * 0.5).abs() < 24.0 {
                    Some((0.5, 0.0))
                } else {
                    None
                };
                if let Some((anchor, offset)) = candidate {
                    p.anchor[axis] = anchor;
                    p.offset[axis] = offset;
                }
            }
        }
        p.actual = rect.size();
    }
    pub fn finish_frame(&mut self, pointer_down: bool) -> Result<(), String> {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(receiver) = &self.pending_save {
            match receiver.try_recv() {
                Ok(result) => {
                    self.pending_save = None;
                    if let Err(error) = result {
                        self.changed = true;
                        self.retry_after =
                            Some(web_time::Instant::now() + std::time::Duration::from_secs(2));
                        return Err(error);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.pending_save = None;
                    self.changed = true;
                    self.retry_after =
                        Some(web_time::Instant::now() + std::time::Duration::from_secs(2));
                    return Err("Layout save worker stopped; retrying shortly".into());
                }
            }
        }
        if !pointer_down
            && self.changed
            && self
                .retry_after
                .is_none_or(|time| web_time::Instant::now() >= time)
        {
            self.dragging = false;
            for p in self.entries.values_mut() {
                p.touched = false;
            }
            match self.save() {
                Ok(()) => {
                    self.changed = false;
                    self.retry_after = None;
                }
                Err(error) => {
                    self.retry_after =
                        Some(web_time::Instant::now() + std::time::Duration::from_secs(2));
                    return Err(error);
                }
            }
        }
        Ok(())
    }
    /// Shutdown-only bounded drain. Normal frames never wait for disk writes.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn flush_on_exit(&mut self, budget: std::time::Duration) -> Result<(), String> {
        let deadline = std::time::Instant::now() + budget;
        loop {
            if let Some(receiver) = self.pending_save.take() {
                match receiver
                    .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        self.changed = true;
                        return Err(error);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        self.pending_save = Some(receiver);
                        return Err("Layout save is still pending at shutdown".into());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        self.changed = true;
                        return Err("Layout save worker stopped at shutdown".into());
                    }
                }
            }
            if !self.changed {
                return Ok(());
            }
            if std::time::Instant::now() >= deadline {
                return Err("Latest layout could not be saved within shutdown deadline".into());
            }
            self.retry_after = None;
            self.finish_frame(false)?;
        }
    }
    pub fn reset(&mut self) {
        self.generation += 1;
        self.entries.clear();
        self.reflow = true;
        self.changed = true;
        self.pinned = false;
    }
    pub fn settings_changed(&mut self) {
        self.changed = true;
    }
    fn save(&mut self) -> Result<(), String> {
        let entries:serde_json::Map<String,serde_json::Value>=self.entries.iter().map(|(key,p)|(key.clone(),serde_json::json!({"anchor":p.anchor,"offset":[p.offset.x,p.offset.y],"size":[p.size.x,p.size.y]}))).collect();
        let value = serde_json::json!({"pinned":self.pinned,"snap":self.snap,"panels":entries});
        #[cfg(target_arch = "wasm32")]
        {
            super::browser::write(&self.path, &value.to_string())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let path = self.path.clone();
            let bytes = value.to_string().into_bytes();
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("arkline-layout-save".into())
                .spawn(move || {
                    let _ = sender.send(write_preferences(&path, &bytes));
                })
                .map_err(|error| format!("Cannot start layout save: {error}"))?;
            self.pending_save = Some(receiver);
            Ok(())
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn write_preferences(path: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_SAVE: AtomicU64 = AtomicU64::new(0);
    fs::create_dir_all(path.parent().ok_or("Layout path has no parent")?)
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!(
        "{}.{}.tmp",
        std::process::id(),
        NEXT_SAVE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let result = (|| -> std::io::Result<()> {
        let mut file = opts.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result.map_err(|e| format!("Layout could not be saved: {e}"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
impl Layout {
    pub(crate) fn wait_for_save(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while self.changed || self.pending_save.is_some() {
            self.finish_frame(false).unwrap();
            assert!(
                std::time::Instant::now() < deadline,
                "layout save timed out"
            );
            std::thread::yield_now();
        }
    }
}

fn read_preferences(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(target_arch = "wasm32")]
    {
        super::browser::read(path).map(String::into_bytes)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        fs::read(path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authority_exclusion_is_stable_and_never_saves_automatic_clamps() {
        let bounds = Rect::from_min_max(Pos2::new(8., 8.), Pos2::new(892., 598.));
        let mut layout = Layout::load(&std::env::temp_dir().join("arkline-clearance-no-save"));
        layout.position(
            "task",
            bounds,
            [1., 0.5],
            Vec2::new(-16., 0.),
            Vec2::new(600., 500.),
        );
        let original = layout.entries["task"].clone();
        for obstacle in [
            Rect::from_min_size(Pos2::new(16., 14.), Vec2::new(490., 50.)),
            Rect::from_min_size(Pos2::new(16., 14.), Vec2::new(490., 470.)),
            Rect::from_min_size(Pos2::new(16., 240.), Vec2::new(490., 120.)),
        ] {
            let mut previous = None;
            for _ in 0..10 {
                layout.begin_frame(bounds);
                layout.position("task", bounds, [1., 0.5], Vec2::ZERO, Vec2::ZERO);
                let (pos, size, available) =
                    layout.avoid_foreground("task", bounds, obstacle, 600.);
                let rect = Rect::from_min_size(pos, size + Vec2::new(16., 36.));
                assert!(!rect.intersects(obstacle));
                assert!(available.contains_rect(rect));
                if let Some(old) = previous {
                    assert_eq!((pos, size, available), old);
                }
                previous = Some((pos, size, available));
                layout.record("task", bounds, rect, None, false);
                assert!(
                    !layout.changed,
                    "automatic constraint saved as a user resize"
                );
                assert_eq!(layout.entries["task"].offset, original.offset);
                assert_eq!(layout.entries["task"].size, original.size);
            }
        }
        let disjoint = Rect::from_min_size(Pos2::new(700., 100.), Vec2::new(180., 400.));
        let obstacle = Rect::from_min_size(Pos2::new(16., 14.), Vec2::new(490., 50.));
        assert_eq!(unobscured_region(bounds, obstacle, disjoint), bounds);
    }

    #[test]
    fn delayed_save_does_not_block_frames_or_discard_newer_preferences() {
        let dir = std::env::temp_dir().join(format!("arkline-layout-async-{}", std::process::id()));
        let mut layout = Layout::load(&dir);
        let (done, pending) = std::sync::mpsc::channel();
        layout.pending_save = Some(pending);
        layout.pinned = true;
        layout.settings_changed();
        // No completion is sent yet: frame processing must remain nonblocking,
        // retain the newest edit, and never enqueue another concurrent writer.
        for _ in 0..100 {
            layout.finish_frame(false).unwrap();
            assert!(layout.changed && layout.pending_save.is_some());
        }
        done.send(Ok(())).unwrap();
        layout.wait_for_save();
        assert!(Layout::load(&dir).pinned);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn failed_background_save_retains_changes_and_backs_off() {
        let mut layout = Layout::load(&std::env::temp_dir().join("arkline-save-error-unused"));
        let (done, pending) = std::sync::mpsc::channel();
        layout.pending_save = Some(pending);
        done.send(Err("disk unavailable".into())).unwrap();
        assert_eq!(layout.finish_frame(false).unwrap_err(), "disk unavailable");
        assert!(layout.changed && layout.retry_after.is_some());
        layout.finish_frame(false).unwrap();
        assert!(layout.pending_save.is_none());
    }
    #[test]
    fn shutdown_flush_saves_latest_after_older_writer_and_bounds_stalls() {
        let dir =
            std::env::temp_dir().join(format!("arkline-layout-shutdown-{}", std::process::id()));
        let mut layout = Layout::load(&dir);
        let (done, receiver) = std::sync::mpsc::channel();
        layout.pending_save = Some(receiver);
        layout.pinned = true;
        layout.settings_changed();
        assert!(layout.flush_on_exit(std::time::Duration::ZERO).is_err());
        assert!(layout.pending_save.is_some() && layout.changed);
        assert!(
            !dir.join("layout.json").exists(),
            "must not start second writer"
        );
        done.send(Ok(())).unwrap();
        layout
            .flush_on_exit(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(Layout::load(&dir).pinned);
        layout.pinned = false;
        layout.settings_changed();
        layout
            .flush_on_exit(std::time::Duration::from_secs(2))
            .unwrap();
        assert!(!Layout::load(&dir).pinned);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn reflow_ignores_pointer_capture_and_reset_restores_default_anchors() {
        let dir = std::env::temp_dir().join("arkline-layout-reflow-unused");
        let mut layout = Layout::load(&dir);
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 650.0));
        layout.begin_frame(rect);
        let (position, size) = layout.position(
            "overview",
            rect,
            [1.0, 0.0],
            Vec2::new(-8.0, 8.0),
            Vec2::new(300.0, 400.0),
        );
        layout.record(
            "overview",
            rect,
            Rect::from_min_size(position + Vec2::new(-100.0, 0.0), size),
            Some(size + Vec2::splat(10.0)),
            true,
        );
        assert!(
            !layout.changed,
            "reflow with pointer held must not overwrite layout"
        );
        layout.reset();
        assert_eq!(layout.generation, 1);
        let (reset, _) = layout.position(
            "overview",
            rect,
            [1.0, 0.0],
            Vec2::new(-8.0, 8.0),
            Vec2::new(300.0, 400.0),
        );
        assert_eq!(reset, position);
    }
    #[test]
    fn resize_keeps_right_and_bottom_anchors_and_move_persists() {
        let dir = std::env::temp_dir().join(format!("arkline-layout-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut layout = Layout::load(&dir);
        let small = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 650.0));
        let large = Rect::from_min_size(Pos2::ZERO, Vec2::new(1600.0, 1000.0));
        let (a, _) = layout.position(
            "overview",
            small,
            [1.0, 0.0],
            Vec2::new(-8.0, 8.0),
            Vec2::new(300.0, 400.0),
        );
        let (b, _) = layout.position("overview", large, [1.0, 0.0], Vec2::ZERO, Vec2::ZERO);
        assert!((b.x - a.x - 700.0).abs() < 0.01);
        let (a, _) = layout.position(
            "hud",
            small,
            [0.5, 1.0],
            Vec2::new(0.0, -8.0),
            Vec2::new(600.0, 130.0),
        );
        let (b, _) = layout.position("hud", large, [0.5, 1.0], Vec2::ZERO, Vec2::ZERO);
        assert!((b.y - a.y - 350.0).abs() < 0.01);
        layout.begin_frame(small);
        layout.begin_frame(small);
        let moved = Rect::from_min_size(Pos2::new(500.0, 40.0), Vec2::new(316.0, 436.0));
        layout.record(
            "overview",
            small,
            moved,
            Some(Vec2::new(300.0, 400.0)),
            true,
        );
        layout.record("overview", small, moved, None, false);
        layout.finish_frame(false).unwrap();
        layout.wait_for_save();
        let mut loaded = Layout::load(&dir);
        let (position, _) = loaded.position("overview", small, [0.0, 0.0], Vec2::ZERO, Vec2::ZERO);
        assert!(position.distance(moved.min) < 1.0);
        fs::remove_dir_all(dir).unwrap();
    }
}
