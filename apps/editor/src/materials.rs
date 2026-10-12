use crate::asset_document::{self, MaterialDocument, MaterialHistory};
use arkline_client::EmbeddedMaterialPreview;
use eframe::egui;
use std::path::Path;

pub struct Materials {
    history: MaterialHistory,
    saved: MaterialDocument,
    enabled: bool,
    message: String,
}
impl Materials {
    pub fn open(project: &Path) -> Self {
        let exists = project.join("Assets/Pioneer.material.json").exists();
        let loaded = exists.then(|| asset_document::load(project));
        let (document, enabled, message) = match loaded {
            Some(Ok(document)) => (document, true, "Loaded project materials".into()),
            Some(Err(error)) => (MaterialDocument::default(), false, error),
            None => (MaterialDocument::default(), false, String::new()),
        };
        Self {
            history: MaterialHistory::new(document.clone()).expect("validated material"),
            saved: document,
            enabled,
            message,
        }
    }
    pub fn preview(&self) -> Option<EmbeddedMaterialPreview> {
        self.enabled.then(|| {
            let doc = self.history.current();
            EmbeddedMaterialPreview {
                hull_rgb: doc.hull_color,
                trim_rgb: doc.trim_color,
                emissive_strength: doc.emissive_strength,
                roughness: doc.roughness,
                metallic: doc.metallic,
            }
        })
    }
    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path) {
        ui.heading("Project fleet materials");
        ui.small("Shared project finish · all matching fleet hulls");
        ui.checkbox(&mut self.enabled, "Use project materials");
        ui.collapsing("Material targets · 7 ship classes", |ui| {
            for class in arkline_protocol::ALL_SHIP_CLASSES {
                ui.label(class.spec().name);
            }
            ui.small("Shared hull/trim roles across these classes; this is not a per-vessel paint job. Inspector pilot selection does not change the target.");
        });
        let mut draft = self.history.current().clone();
        ui.separator();
        ui.label("Hull · linear RGB");
        // Color widgets can round-trip float values even without user input.
        // Commit each field only when its own control reports an edit.
        let mut hull_color = draft.hull_color;
        let mut changed = false;
        if ui.color_edit_button_rgb(&mut hull_color).changed() {
            draft.hull_color = hull_color;
            changed = true;
        }
        ui.label("Structural trim · linear RGB");
        let mut trim_color = draft.trim_color;
        if ui.color_edit_button_rgb(&mut trim_color).changed() {
            draft.trim_color = trim_color;
            changed = true;
        }
        let mut emissive_strength = draft.emissive_strength;
        if ui
            .add(egui::Slider::new(&mut emissive_strength, 0.0..=4.0).text("Emission"))
            .changed()
        {
            draft.emissive_strength = emissive_strength;
            changed = true;
        }
        changed |= optional_factor(ui, "Override roughness", &mut draft.roughness, 0.5);
        changed |= optional_factor(ui, "Override metallic", &mut draft.metallic, 0.0);
        ui.small("Unchecked retains each surface's authored value. Roughness controls highlight spread (renderer minimum 0.08); metallic controls metal-like reflection. No material textures are authored here.");
        ui.small("Emission scales the authored glow; it does not change reactor power or illumination from physical lights.");
        ui.horizontal_wrapped(|ui| {
            if ui.button("Industrial satin").clicked() {
                draft.roughness = Some(0.38);
                draft.metallic = Some(0.75);
                changed = true;
            }
            if ui.button("Weathered coating").clicked() {
                draft.roughness = Some(0.72);
                draft.metallic = Some(0.15);
                changed = true;
            }
        });
        ui.small(
            "Finish presets change roughness and metallic only; they preserve colors and emission.",
        );
        if ui.button("Inspection finish").clicked() {
            changed = true;
            draft.hull_color = [0.55, 0.6, 0.58];
            draft.trim_color = [0.015, 0.3, 0.115];
        }
        if ui.button("Authored defaults").clicked() {
            changed = true;
            draft = MaterialDocument::default();
        }
        if changed && &draft != self.history.current() {
            match self.history.apply(draft) {
                Ok(_) => {
                    self.enabled = true;
                    self.message = "Live finish updated; save to keep it after reopening".into();
                }
                Err(error) => self.message = error,
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.history.can_undo(), egui::Button::new("Undo"))
                .clicked()
            {
                self.history.undo();
                self.enabled = true;
            }
            if ui
                .add_enabled(self.history.can_redo(), egui::Button::new("Redo"))
                .clicked()
            {
                self.history.redo();
                self.enabled = true;
            }
        });
        ui.separator();
        ui.label(if self.history.current() == &self.saved {
            "No unsaved material changes"
        } else {
            "Unsaved material changes"
        });
        if ui.button("Save material").clicked() {
            match asset_document::save(project, self.history.current()) {
                Ok(()) => {
                    self.saved = self.history.current().clone();
                    self.message = "Saved Assets/Pioneer.material.json".into();
                }
                Err(error) => self.message = error,
            }
        }
        if ui.button("Reload saved material").clicked() {
            match asset_document::load(project) {
                Ok(document) => {
                    self.saved = document.clone();
                    if let Err(error) = self.history.apply(document) {
                        self.message = error;
                    } else {
                        self.enabled = true;
                        self.message =
                            "Reloaded saved material; Undo restores the previous edit".into();
                    }
                }
                Err(error) => self.message = error,
            }
        }
        ui.label(&self.message);
        ui.separator();
        ui.small("Edits apply to tagged hull/trim surfaces across the seven ship classes in hangar and flight, including 3D tactical and camera feeds. Stations, glass and interior fittings retain their authored materials. A 2D map has no hull mesh. Geometry, shader source and system connections are not editable here yet.");
    }
}

fn optional_factor(ui: &mut egui::Ui, label: &str, value: &mut Option<f32>, initial: f32) -> bool {
    let mut enabled = value.is_some();
    let mut changed = false;
    if ui.checkbox(&mut enabled, label).changed() {
        *value = enabled.then_some(initial);
        changed = true;
    }
    if let Some(current) = *value {
        let mut edited = current;
        if ui.add(egui::Slider::new(&mut edited, 0.0..=1.0)).changed() {
            *value = Some(edited);
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_material_panel_preserves_document_history_and_disabled_preview() {
        let document = MaterialDocument::default();
        let mut materials = Materials {
            history: MaterialHistory::new(document.clone()).unwrap(),
            saved: document.clone(),
            enabled: false,
            message: String::new(),
        };
        let context = egui::Context::default();
        for frame in 0..3 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 1200.0),
                )),
                time: Some(frame as f64 / 60.0),
                ..Default::default()
            };
            let _ = context.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    materials.show(ui, Path::new("unused-idle-test-project"));
                });
            });
            assert_eq!(materials.history.current(), &document);
            assert_eq!(materials.history.current(), &materials.saved);
            assert!(!materials.history.can_undo());
            assert!(!materials.history.can_redo());
            assert!(materials.preview().is_none());
            assert!(materials.message.is_empty());
        }
    }
}
