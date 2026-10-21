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
#[derive(Clone, Copy, Default)]
pub struct SoundContext {
    pub paused: bool,
    pub live_world: bool,
    pub listening: bool,
    pub speaking: bool,
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
    fn test_block_reason(&self, context: SoundContext) -> Option<&'static str> {
        if context.listening {
            Some("Game sound is muted during microphone listening · finish voice capture before testing")
        } else if self.muted {
            Some("Game sound is muted · turn off Mute game sound to test")
        } else if self.master == 0.0 {
            Some("Master is zero · raise the game Master level to test")
        } else if self.effects == 0.0 {
            Some("Effects is zero · raise the game Effects level to test")
        } else {
            None
        }
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        status: &super::sound_engine::OutputStatus,
        context: SoundContext,
        test_notice: &str,
    ) -> SoundAction {
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
        let blocked = self.test_block_reason(context);
        if let Some(reason) = blocked {
            ui.colored_label(egui::Color32::YELLOW, reason);
        }
        if context.paused {
            ui.small("World machinery is paused. The two-tone output test can still run.");
        } else if !context.live_world {
            ui.small("World sound is suppressed without fresh connected telemetry. The output test is independent of the world.");
        }
        if context.speaking {
            ui.small("Spoken reply active · game sound is temporarily reduced");
        }
        ui.horizontal_wrapped(|ui| {
            action.test = ui
                .add_enabled(blocked.is_none(), egui::Button::new("Test sound"))
                .on_hover_text("Two sustained mid-range tones through your saved game-sound levels")
                .clicked();
            action.retry = ui.button("Retry audio")
                .on_hover_text("Reopen game audio on the current system default output; does not change system settings")
                .clicked();
        });
        ui.label(format!("Output stream: {}", status.device));
        if !test_notice.is_empty() {
            ui.label(test_notice);
        }
        if status.tests_queued > 0 {
            if status.last_test_interrupted {
                ui.label("Last queued test interrupted by output recovery · test again when the stream opens");
            } else if status.tests_consumed + status.tests_interrupted >= status.tests_queued {
                ui.label(format!(
                    "Test #{} received by synthesis callback · hearing not confirmed",
                    status.tests_queued
                ));
            } else {
                ui.label(format!(
                    "Test #{} queued · waiting for synthesis callback",
                    status.tests_queued
                ));
            }
        }
        ui.small("Listen for two sustained tones. Callback activity and generated signal do not confirm audible speaker output.");
        #[cfg(not(target_arch = "wasm32"))]
        ui.small("Device errors or stalled callbacks trigger up to five automatic retries on the current default output. Retry audio starts a fresh attempt immediately. Queued sounds are discarded, never replayed.");
        #[cfg(target_arch = "wasm32")]
        ui.small("Browser audio may require a click on Retry audio to resume. Automatic device recovery is available in the desktop app.");
        ui.small("No system output setting is changed. Voice input and spoken replies have separate controls.");
        ui.collapsing("Audio diagnostics", |ui| {
            ui.label(&status.diagnostics);
            ui.small("Peak is the maximum since this audio engine started; RMS is the latest block. Both measure generated samples before device-format conversion.");
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn audio_feedback_effective_mute_and_paused_test_rules_are_explicit() {
        let mut settings = SoundSettings::default();
        assert!(settings
            .test_block_reason(SoundContext {
                paused: true,
                ..Default::default()
            })
            .is_none());
        assert!(settings
            .test_block_reason(SoundContext {
                listening: true,
                ..Default::default()
            })
            .unwrap()
            .contains("microphone"));
        settings.muted = true;
        assert!(settings
            .test_block_reason(SoundContext::default())
            .unwrap()
            .contains("muted"));
        settings.muted = false;
        settings.master = 0.;
        assert!(settings
            .test_block_reason(SoundContext::default())
            .unwrap()
            .contains("Master"));
        settings.master = 0.4;
        settings.effects = 0.;
        assert!(settings
            .test_block_reason(SoundContext::default())
            .unwrap()
            .contains("Effects"));
    }
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
