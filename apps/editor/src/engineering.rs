use crate::engineering_document::{
    self as doc, EngineeringDocument, EngineeringHistory, PartClass,
};
use eframe::egui;
use std::path::Path;

pub struct Engineering {
    history: EngineeringHistory,
    draft: EngineeringDocument,
    saved: EngineeringDocument,
    selected: Option<u64>,
    message: String,
    viewport: crate::component_view::ComponentView,
}
impl Engineering {
    pub fn open(project: &Path) -> Self {
        let (document, message) = if project.join("Assets/Pioneer.engineering.json").exists() {
            match doc::load(project) {
                Ok(d) => (d, String::new()),
                Err(e) => (EngineeringDocument::default(), e),
            }
        } else {
            (EngineeringDocument::default(), String::new())
        };
        Self {
            history: EngineeringHistory::new(document.clone()).expect("validated document"),
            draft: document.clone(),
            saved: document,
            selected: None,
            viewport: Default::default(),
            message,
        }
    }
    pub fn selected_component(&self) -> Option<u64> {
        self.selected
            .filter(|id| self.draft.components.iter().any(|c| c.id == *id))
    }
    pub fn outliner(&mut self, ui: &mut egui::Ui) -> bool {
        let mut picked = false;
        ui.collapsing("Pioneer design components", |ui| {
            ui.small("Candidate design · separate from live equipment");
            if self.draft.components.is_empty() {
                ui.label("Add a component in Engineering to begin.");
            }
            for component in &self.draft.components {
                if ui
                    .selectable_label(
                        self.selected_component() == Some(component.id),
                        format!("#{} {}", component.id, component.name),
                    )
                    .clicked()
                {
                    self.selected = Some(component.id);
                    picked = true;
                }
            }
        });
        picked
    }
    pub fn components(&self) -> &[crate::engineering_document::Component] {
        &self.draft.components
    }
    pub fn viewport(&mut self, ui: &mut egui::Ui) {
        self.selected = self.selected_component();
        if let Err(error) = self.draft.validate() {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
            ui.label("Correct the candidate properties before previewing geometry.");
            return;
        }
        self.viewport
            .show(ui, &self.draft.components, &mut self.selected);
    }
    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path) {
        ui.heading("Pioneer engineering");
        ui.label("CANDIDATE DESIGN · local authoring only");
        ui.small("No server physics changes. Masses are user estimates, not verified CAD or runtime measurements. Empty means unknown, not a massless ship.");
        ui.small("Envelope: 72 m longitudinal X × 18 m vertical Y × 38 m lateral Z. Origin at envelope center; this is a design frame, not interior avatar coordinates.");
        self.history.preserve_allocator(&mut self.draft);
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.button("Add component").clicked() {
                match self.draft.add_component() { Ok(id) => {self.history.preserve_allocator(&mut self.draft);self.selected=Some(id);changed=true;self.message="New component uses illustrative 1 kg / 1 m box placeholders; enter measured design values".into();}, Err(e)=>self.message=e }
            }
            if ui.add_enabled(self.history.can_undo(), egui::Button::new("Undo")).clicked() {self.history.undo();self.draft=self.history.current().clone();}
            if ui.add_enabled(self.history.can_redo(), egui::Button::new("Redo")).clicked() {self.history.redo();self.draft=self.history.current().clone();}
        });
        egui::ScrollArea::vertical()
            .id_salt("engineering-parts")
            .max_height(140.)
            .show(ui, |ui| {
                for c in &self.draft.components {
                    if ui
                        .selectable_label(
                            self.selected == Some(c.id),
                            format!("#{} {} · {:.1} kg", c.id, c.name, c.mass_kg),
                        )
                        .clicked()
                    {
                        self.selected = Some(c.id);
                    }
                }
            });
        if let Some(c) = self
            .draft
            .components
            .iter_mut()
            .find(|c| Some(c.id) == self.selected)
        {
            ui.separator();
            ui.label(format!("Component #{}", c.id));
            changed |= ui.text_edit_singleline(&mut c.name).changed();
            ui.horizontal(|ui| {
                changed |= ui
                    .selectable_value(&mut c.class, PartClass::Structure, "Structure")
                    .changed();
                changed |= ui
                    .selectable_value(&mut c.class, PartClass::Equipment, "Equipment")
                    .changed();
            });
            ui.label("Material classification (user supplied)");
            changed |= ui.text_edit_singleline(&mut c.material).changed();
            changed |= ui
                .add(
                    egui::DragValue::new(&mut c.mass_kg)
                        .speed(1.)
                        .prefix("Mass ")
                        .suffix(" kg"),
                )
                .changed();
            ui.label("Center X / Y / Z (m)");
            ui.horizontal(|ui| {
                for value in &mut c.center_m {
                    changed |= ui.add(egui::DragValue::new(value).speed(0.1)).changed();
                }
            });
            ui.label("Full box extent X / Y / Z (m)");
            ui.horizontal(|ui| {
                for value in &mut c.extent_m {
                    changed |= ui.add(egui::DragValue::new(value).speed(0.1)).changed();
                }
            });
            if ui.button("Remove selected component").clicked() {
                let id = c.id;
                self.draft.components.retain(|c| c.id != id);
                self.selected = None;
                changed = true;
            }
        }
        if changed {
            if let Err(e) = self.history.apply(self.draft.clone()) {
                self.message = e;
            }
        }
        let validation = self.draft.validate();
        if let Err(e) = &validation {
            ui.colored_label(egui::Color32::LIGHT_RED, e);
        }
        ui.separator();
        match self.draft.report() {
            Ok(Some(r)) => {
                ui.label(format!("Candidate mass: {:.3} kg", r.mass_kg));
                ui.label(format!(
                    "Center of mass: X {:.3}, Y {:.3}, Z {:.3} m",
                    r.center_m[0], r.center_m[1], r.center_m[2]
                ));
                ui.label(format!(
                    "Diagonal inertia: [{:.2}, {:.2}, {:.2}] kg·m²",
                    r.diagonal_inertia_kg_m2[0],
                    r.diagonal_inertia_kg_m2[1],
                    r.diagonal_inertia_kg_m2[2]
                ));
                ui.small("Uniform axis-aligned solid boxes + parallel-axis theorem about candidate COM. Diagonal only; not principal moments, stress, collision or damage certification. Overlaps are permitted and counted independently.");
                ui.label(format!("Longitudinal thirds (component centers): −X {:.1} / middle {:.1} / +X {:.1} kg",r.longitudinal_mass_kg[0],r.longitudinal_mass_kg[1],r.longitudinal_mass_kg[2]));
            }
            Ok(None) => {
                ui.label("No component mass data yet.");
            }
            Err(_) => {
                ui.label("Report unavailable until invalid fields are corrected.");
            }
        }
        ui.label("Top-down component map · X horizontal / Z vertical");
        let width = ui.available_width().clamp(80., 480.);
        let (rect, _) =
            ui.allocate_exact_size(egui::vec2(width, width * 38. / 72.), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0., egui::Color32::from_rgb(8, 25, 19));
        if validation.is_ok() {
            for c in &self.draft.components {
                let pos = egui::pos2(
                    rect.center().x + (c.center_m[0] / 72.) as f32 * rect.width(),
                    rect.center().y - (c.center_m[2] / 38.) as f32 * rect.height(),
                );
                let size = egui::vec2(
                    (c.extent_m[0] / 72.) as f32 * rect.width(),
                    (c.extent_m[2] / 38.) as f32 * rect.height(),
                );
                let color = if self.selected == Some(c.id) {
                    egui::Color32::LIGHT_GREEN
                } else {
                    match c.class {
                        PartClass::Structure => egui::Color32::from_rgb(40, 125, 80),
                        PartClass::Equipment => egui::Color32::from_rgb(55, 120, 175),
                    }
                };
                painter.rect_stroke(
                    egui::Rect::from_center_size(pos, size),
                    0.,
                    egui::Stroke::new(1., color),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    pos,
                    egui::Align2::CENTER_CENTER,
                    c.id.to_string(),
                    egui::FontId::monospace(10.),
                    color,
                );
            }
        }
        ui.small(
            "Green: structure · blue: equipment · pale green: selected. Y depth is not shown.",
        );
        ui.separator();
        ui.label(if self.draft == self.saved {
            "No unsaved engineering changes"
        } else {
            "Unsaved engineering changes"
        });
        if ui
            .add_enabled(
                validation.is_ok(),
                egui::Button::new("Save engineering document"),
            )
            .clicked()
        {
            match doc::save(project, &self.draft) {
                Ok(()) => {
                    self.saved = self.draft.clone();
                    self.message = "Saved Assets/Pioneer.engineering.json".into();
                }
                Err(e) => self.message = e,
            }
        }
        if ui.button("Reload saved engineering").clicked() {
            match doc::load(project) {
                Ok(d) => {
                    let _ = self.history.apply(d.clone());
                    self.saved = d.clone();
                    self.draft = self.history.current().clone();
                    self.message = "Reloaded saved engineering".into();
                }
                Err(e) => self.message = e,
            }
        }
        ui.label(&self.message);
    }
}
