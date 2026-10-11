//! Local audio preferences; never part of shared ship or economy state.
use eframe::egui;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{Read, Write};
use std::path::Path;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SoundSettings {
    pub master: f32,
    pub ambience: f32,
    pub effects: f32,
    pub muted: bool,
}
impl Default for SoundSettings {
    fn default() -> Self {
        Self {
            master: 0.45,
            ambience: 0.35,
            effects: 0.65,
            muted: false,
        }
    }
}
#[derive(Default)]
pub struct SoundAction {
    pub test: bool,
    pub retry: bool,
    pub changed: bool,
}
impl SoundSettings {
    fn valid(&self) -> bool {
        [self.master, self.ambience, self.effects]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }
    pub fn load(data_dir: &Path) -> Self {
        let result = (|| -> Option<Self> {
            #[cfg(target_arch = "wasm32")]
            let bytes = super::browser::read(&data_dir.join("sound.json"))
                .ok()?
                .into_bytes();
            #[cfg(not(target_arch = "wasm32"))]
            let bytes = {
                let mut bytes = Vec::new();
                fs::File::open(data_dir.join("sound.json"))
                    .ok()?
                    .take(16_385)
                    .read_to_end(&mut bytes)
                    .ok()?;
                bytes
            };
            if bytes.len() > 16_384 {
                return None;
            }
            let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
            let gain = |key: &str| {
                let v = value.get(key)?.as_f64()?;
                (v.is_finite() && (0.0..=1.0).contains(&v)).then_some(v as f32)
            };
            let settings = Self {
                master: gain("master")?,
                ambience: gain("ambience")?,
                effects: gain("effects")?,
                muted: value.get("muted")?.as_bool()?,
            };
            settings.valid().then_some(settings)
        })();
        result.unwrap_or_default()
    }
    pub fn save(&self, data_dir: &Path) -> Result<(), String> {
        if !self.valid() {
            return Err("Sound levels must be finite values between 0 and 1".into());
        }
        #[cfg(target_arch = "wasm32")]
        {
            super::browser::write(&data_dir.join("sound.json"), &serde_json::json!({"master":self.master,"ambience":self.ambience,"effects":self.effects,"muted":self.muted}).to_string())
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
            let temporary = data_dir.join(format!(
                ".sound-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let result = (|| -> std::io::Result<()> {
                let mut options = fs::OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&temporary)?;
                let json = serde_json::json!({"master":self.master,"ambience":self.ambience,"effects":self.effects,"muted":self.muted});
                file.write_all(json.to_string().as_bytes())?;
                file.sync_all()?;
                fs::rename(&temporary, data_dir.join("sound.json"))
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            result.map_err(|e| format!("Could not save sound preferences: {e}"))
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, status: &str) -> SoundAction {
        let mut action = SoundAction::default();
        if !self.valid() {
            *self = Self::default();
            action.changed = true;
        }
        ui.heading("Sound");
        action.changed |= ui.checkbox(&mut self.muted, "Mute game sound").changed();
        action.changed |= ui
            .add(egui::Slider::new(&mut self.master, 0.0..=1.0).text("Master"))
            .changed();
        action.changed |= ui
            .add(egui::Slider::new(&mut self.ambience, 0.0..=1.0).text("Ambience"))
            .changed();
        action.changed |= ui
            .add(egui::Slider::new(&mut self.effects, 0.0..=1.0).text("Effects"))
            .changed();
        ui.horizontal_wrapped(|ui| {
            action.test = ui.button("Test sound").clicked();
            action.retry = ui.button("Retry audio").clicked();
        });
        ui.label(format!("Output: {status}"));
        ui.small("Uses the system default output. Voice input and spoken replies have separate controls.");
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn directory() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "arkline-sound-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }
    #[test]
    fn preferences_round_trip_and_invalid_save_preserves_existing_file() {
        let dir = directory();
        assert_eq!(SoundSettings::load(&dir), SoundSettings::default());
        let settings = SoundSettings {
            master: 0.25,
            ambience: 0.0,
            effects: 1.0,
            muted: true,
        };
        settings.save(&dir).unwrap();
        assert_eq!(SoundSettings::load(&dir), settings);
        let original = fs::read(dir.join("sound.json")).unwrap();
        for invalid in [f32::NAN, f32::INFINITY, -0.1, 1.01] {
            let mut broken = settings;
            broken.master = invalid;
            assert!(broken.save(&dir).is_err());
            assert_eq!(fs::read(dir.join("sound.json")).unwrap(), original);
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(dir.join("sound.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn malformed_out_of_range_missing_and_oversized_preferences_fall_back() {
        let dir = directory();
        fs::create_dir_all(&dir).unwrap();
        for json in [
            "not json".to_owned(),
            "{}".to_owned(),
            r#"{"master":1.1,"ambience":0.3,"effects":0.5,"muted":false}"#.to_owned(),
            r#"{"master":0.4,"ambience":null,"effects":0.5,"muted":false}"#.to_owned(),
            " ".repeat(16_385),
        ] {
            fs::write(dir.join("sound.json"), json).unwrap();
            assert_eq!(SoundSettings::load(&dir), SoundSettings::default());
        }
        fs::remove_dir_all(dir).unwrap();
    }
}
