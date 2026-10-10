//! Local edge-relative overlay placement, independent of universe state.
use eframe::egui::{Pos2, Rect, Vec2};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug)]
struct Placement {
    anchor: [f32; 2],
    offset: Vec2,
    size: Vec2,
    actual: Vec2,
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
    retry_after: Option<std::time::Instant>,
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
        };
        if let Ok(bytes) = fs::read(&result.path) {
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
            touched: false,
            expected: None,
        });
        let size = p
            .size
            .min(bounds.size() - Vec2::splat(30.0))
            .max(Vec2::splat(1.0));
        let actual = p.actual.min(bounds.size());
        let origin =
            bounds.min + Vec2::new(bounds.width() * p.anchor[0], bounds.height() * p.anchor[1]);
        let pos = origin + p.offset - Vec2::new(actual.x * p.anchor[0], actual.y * p.anchor[1]);
        let result = (
            Pos2::new(
                pos.x.clamp(
                    bounds.left(),
                    (bounds.right() - actual.x).max(bounds.left()),
                ),
                pos.y
                    .clamp(bounds.top(), (bounds.bottom() - actual.y).max(bounds.top())),
            ),
            size,
        );
        p.expected = Some(result.0);
        result
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
        if !pointer_down
            && self.changed
            && self
                .retry_after
                .is_none_or(|time| std::time::Instant::now() >= time)
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
                        Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    return Err(error);
                }
            }
        }
        Ok(())
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
    fn save(&self) -> Result<(), String> {
        let entries:serde_json::Map<String,serde_json::Value>=self.entries.iter().map(|(key,p)|(key.clone(),serde_json::json!({"anchor":p.anchor,"offset":[p.offset.x,p.offset.y],"size":[p.size.x,p.size.y]}))).collect();
        let value = serde_json::json!({"pinned":self.pinned,"snap":self.snap,"panels":entries});
        fs::create_dir_all(self.path.parent().unwrap()).map_err(|e| e.to_string())?;
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", std::process::id()));
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        use std::io::Write;
        let result = (|| -> std::io::Result<()> {
            let mut file = opts.open(&temporary)?;
            file.write_all(value.to_string().as_bytes())?;
            file.sync_all()?;
            fs::rename(&temporary, &self.path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result.map_err(|e| format!("Layout could not be saved: {e}"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
        let mut loaded = Layout::load(&dir);
        let (position, _) = loaded.position("overview", small, [0.0, 0.0], Vec2::ZERO, Vec2::ZERO);
        assert!(position.distance(moved.min) < 1.0);
        fs::remove_dir_all(dir).unwrap();
    }
}
