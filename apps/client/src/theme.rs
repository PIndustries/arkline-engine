//! Local cockpit appearance preferences; independent of shared universe state.
use eframe::egui::{self, Color32};
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Emerald,
    Teal,
    Amber,
}
impl Theme {
    pub const ALL: [Self; 3] = [Self::Emerald, Self::Teal, Self::Amber];
    pub fn name(self) -> &'static str {
        match self {
            Self::Emerald => "Emerald",
            Self::Teal => "Teal",
            Self::Amber => "Amber",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Self::Emerald => "emerald",
            Self::Teal => "teal",
            Self::Amber => "amber",
        }
    }
    pub fn accent(self) -> Color32 {
        match self {
            Self::Emerald => Color32::from_rgb(111, 207, 162),
            Self::Teal => Color32::from_rgb(111, 201, 205),
            Self::Amber => Color32::from_rgb(224, 186, 116),
        }
    }
    pub fn panel(self) -> Color32 {
        Color32::from_rgb(18, 29, 40)
    }
    pub fn load(directory: &Path) -> Self {
        read_preferences(&directory.join("appearance.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| match value.get("theme").and_then(|v| v.as_str()) {
                Some("emerald") => Some(Self::Emerald),
                Some("teal") => Some(Self::Teal),
                Some("amber") => Some(Self::Amber),
                _ => None,
            })
            .unwrap_or_default()
    }
    pub fn save(self, directory: &Path) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            super::browser::write(
                &directory.join("appearance.json"),
                &serde_json::json!({"theme":self.key()}).to_string(),
            )
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            fs::create_dir_all(directory).map_err(|e| e.to_string())?;
            let temporary = directory.join(format!(
                ".appearance-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let result = (|| -> std::io::Result<()> {
                use std::io::Write;
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&temporary)?;
                file.write_all(
                    serde_json::json!({"theme":self.key()})
                        .to_string()
                        .as_bytes(),
                )?;
                file.sync_all()?;
                fs::rename(&temporary, directory.join("appearance.json"))
            })();
            if result.is_err() {
                let _ = fs::remove_file(temporary);
            }
            result.map_err(|e| {
                format!("Appearance changed for this session but could not be saved: {e}")
            })
        }
    }

    pub fn apply(self, context: &egui::Context) {
        let mut style = (*context.style()).clone();
        let accent = self.accent();
        let text = Color32::from_rgb(230, 239, 246);
        let edge = Color32::from_rgb(54, 75, 92);
        let control = Color32::from_rgb(25, 40, 54);
        style.visuals = egui::Visuals::dark();
        // Let widget state supply its text color so focus and disabled states
        // remain legible instead of applying one global color to every control.
        style.visuals.override_text_color = None;
        style.visuals.panel_fill = Color32::from_rgb(11, 19, 29);
        style.visuals.window_fill = self.panel();
        style.visuals.extreme_bg_color = Color32::from_rgb(7, 14, 23);
        style.visuals.faint_bg_color = Color32::from_rgb(21, 34, 46);
        style.visuals.code_bg_color = Color32::from_rgb(9, 21, 31);
        style.visuals.window_corner_radius = egui::CornerRadius::same(8);
        style.visuals.menu_corner_radius = egui::CornerRadius::same(5);
        style.visuals.window_stroke = egui::Stroke::new(1.0, edge);
        style.visuals.selection.bg_fill = accent.gamma_multiply(0.20);
        style.visuals.selection.stroke = egui::Stroke::new(1.25, accent);
        style.visuals.text_cursor.stroke = egui::Stroke::new(2.0, accent);
        style.visuals.slider_trailing_fill = true;
        style.visuals.warn_fg_color = Color32::from_rgb(239, 194, 119);
        style.visuals.error_fg_color = Color32::from_rgb(249, 143, 145);
        style.visuals.hyperlink_color = accent;
        let widgets = &mut style.visuals.widgets;
        widgets.noninteractive.bg_fill = self.panel();
        widgets.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
        widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, edge);
        widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text);
        widgets.inactive.bg_fill = control;
        widgets.inactive.weak_bg_fill = control;
        widgets.inactive.bg_stroke = egui::Stroke::new(1.0, edge);
        widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text);
        widgets.hovered.bg_fill = accent.gamma_multiply(0.17);
        widgets.hovered.weak_bg_fill = widgets.hovered.bg_fill;
        widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent.gamma_multiply(0.8));
        widgets.hovered.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
        widgets.active.bg_fill = accent.gamma_multiply(0.26);
        widgets.active.weak_bg_fill = widgets.active.bg_fill;
        widgets.active.bg_stroke = egui::Stroke::new(1.25, accent);
        widgets.active.fg_stroke = egui::Stroke::new(1.5, accent);
        widgets.open.bg_fill = accent.gamma_multiply(0.19);
        widgets.open.weak_bg_fill = widgets.open.bg_fill;
        widgets.open.bg_stroke = egui::Stroke::new(1.0, accent.gamma_multiply(0.75));
        widgets.open.fg_stroke = egui::Stroke::new(1.0, text);
        for widget in [
            &mut widgets.noninteractive,
            &mut widgets.inactive,
            &mut widgets.hovered,
            &mut widgets.active,
            &mut widgets.open,
        ] {
            widget.corner_radius = egui::CornerRadius::same(4);
            // Keep hover/focus feedback within its allotted layout rectangle.
            widget.expansion = 0.0;
        }
        style.animation_time = 0.12;
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size = egui::vec2(36.0, 32.0);
        style.spacing.window_margin = egui::Margin::same(16);
        style.spacing.menu_margin = egui::Margin::same(8);
        style.spacing.icon_width = 16.0;
        style.spacing.icon_width_inner = 10.0;
        style.spacing.icon_spacing = 7.0;
        for (kind, size) in [
            (egui::TextStyle::Heading, 22.0),
            (egui::TextStyle::Body, 15.0),
            (egui::TextStyle::Button, 14.0),
            (egui::TextStyle::Small, 12.0),
        ] {
            style
                .text_styles
                .insert(kind, egui::FontId::proportional(size));
        }
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
        context.set_style(style);
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
    fn persists_choices_and_falls_back_for_invalid_configuration() {
        let directory =
            std::env::temp_dir().join(format!("arkline-theme-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        assert_eq!(Theme::load(&directory), Theme::Emerald);
        for theme in Theme::ALL {
            theme.save(&directory).unwrap();
            assert_eq!(Theme::load(&directory), theme);
        }
        fs::write(directory.join("appearance.json"), "{invalid").unwrap();
        assert_eq!(Theme::load(&directory), Theme::Emerald);
        fs::write(directory.join("appearance.json"), r#"{"theme":"unknown"}"#).unwrap();
        assert_eq!(Theme::load(&directory), Theme::Emerald);
        fs::remove_dir_all(directory).unwrap();
    }
}
