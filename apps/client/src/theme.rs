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
            Self::Emerald => Color32::from_rgb(70, 235, 133),
            Self::Teal => Color32::from_rgb(69, 220, 212),
            Self::Amber => Color32::from_rgb(255, 190, 77),
        }
    }
    pub fn panel(self) -> Color32 {
        let c = self.accent();
        Color32::from_rgb(7 + c.r() / 24, 7 + c.g() / 24, 7 + c.b() / 24)
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
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::from_rgb(5, 8, 7);
        style.visuals.window_fill = self.panel();
        style.visuals.extreme_bg_color = Color32::from_rgb(2, 4, 3);
        style.visuals.faint_bg_color = self.panel();
        style.visuals.selection.bg_fill = self.accent().gamma_multiply(0.23);
        style.visuals.selection.stroke = egui::Stroke::new(1.0, self.accent());
        style.visuals.hyperlink_color = self.accent();
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(17, 23, 20);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(17, 23, 20);
        style.visuals.widgets.hovered.bg_fill = self.accent().gamma_multiply(0.16);
        style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, self.accent());
        style.visuals.widgets.active.bg_fill = self.accent().gamma_multiply(0.24);
        style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, self.accent());
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
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
