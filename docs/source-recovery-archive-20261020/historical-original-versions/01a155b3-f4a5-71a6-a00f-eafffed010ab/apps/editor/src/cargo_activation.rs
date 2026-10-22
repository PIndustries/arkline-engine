//! A saved candidate can initialize a separate, exclusively created project; never retune a world.
use super::*;
use arkline_protocol::WorldCargoConfig;
use arkline_server::editor::EditorWorld;
use sha2::{Digest, Sha256};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
#[path = "cargo_activation_tests.rs"]
mod tests;

pub(super) struct ActivationPreview {
    document: CandidateDocument,
    configuration: WorldCargoConfig,
    source_bytes: Vec<u8>,
    destination: PathBuf,
}

impl CargoAuthoring {
    pub(super) fn clear_activation_preview(&mut self) {
        self.activation = None;
        self.activation_confirmed = false;
    }

    fn saved_activation_candidate(&self) -> Result<(CandidateDocument, Vec<u8>), String> {
        if self.load_blocked {
            return Err("Repair the candidate file and reload before creating a test project.".into());
        }
        if self.is_dirty() {
            return Err("Save your candidate edits before previewing a new test project.".into());
        }
        let saved = self
            .disk_bytes
            .as_ref()
            .ok_or("Save a complete candidate before creating a test project.")?;
        if read_bytes(&self.project)?.as_ref() != Some(saved) {
            return Err("The saved candidate changed on disk. Reload and review it before creating a test project.".into());
        }
        let document = self.fields.document()?;
        document.shared_draft().validated_config()?;
        Ok((document, saved.clone()))
    }

    pub(super) fn prepare_activation_preview(&mut self) {
        self.clear_activation_preview();
        match self.saved_activation_candidate() {
            Ok((document, source_bytes)) => {
                let configuration = match document.shared_draft().validated_config() {
                    Ok(value) => value,
                    Err(error) => {
                        self.message = error;
                        return;
                    }
                };
                let stamp = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
                self.activation = Some(ActivationPreview {
                    document,
                    configuration,
                    source_bytes,
                    destination: self
                        .project
                        .join("CargoTests")
                        .join(format!("cargo-test-{stamp}-{id}")),
                });
                self.message = "Review the saved ratings and destination below. No project has been created.".into();
            }
            Err(error) => self.message = error,
        }
    }

    pub(super) fn create_test_project(&mut self) {
        let result = (|| {
            if !self.activation_confirmed {
                return Err("Confirm the reviewed saved candidate before creating a test project.".into());
            }
            let preview = self
                .activation
                .as_ref()
                .ok_or("Preview a complete saved candidate first.")?;
            let (document, source_bytes) = self.saved_activation_candidate()?;
            if document != preview.document || source_bytes != preview.source_bytes {
                return Err("The candidate changed after preview. Review a new preview first.".into());
            }
            create_project(&self.project, preview)
        })();
        match result {
            Ok(path) => {
                self.created_project = Some(path);
                self.clear_activation_preview();
                self.message = "Created a separate cargo test project with the reviewed ratings. Its world starts paused; your current session and balances are unchanged.".into();
            }
            Err(error) => {
                self.activation_confirmed = false;
                self.message = error;
            }
        }
    }

    pub(super) fn show_activation(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.heading("New cargo test project");
        ui.label("Use a complete saved candidate to create a separate local world. The current world is preserved; no balances or saved worlds are migrated.");
        let ready = !self.load_blocked
            && !self.is_dirty()
            && self.disk_bytes.is_some()
            && self
                .fields
                .document()
                .and_then(|doc| doc.shared_draft().validated_config())
                .is_ok();
        if ui
            .add_enabled(ready, egui::Button::new("Preview new cargo test project"))
            .clicked()
        {
            self.prepare_activation_preview();
        }
        if !ready {
            ui.small("Enter every hull and ore rating with its source and revision, then save the candidate to enable the preview.");
        }
        let mut create = false;
        if let Some(preview) = &self.activation {
            ui.label(format!("New project: {}", preview.destination.display()));
            ui.small("Creates a new paused orbital editor world with these saved ratings. Legacy count ceilings still apply. Existing worlds cannot be selected or overwritten.");
            let ore = &preview.configuration.ore;
            ui.label(format!(
                "Ore: {} kg/unit · {} m³/unit · {}",
                ore.mass_kg, ore.volume_m3, ore.provenance.reference
            ));
            egui::Grid::new("cargo_creation_capacity_preview")
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("Hull");
                    ui.strong("Payload / hold");
                    ui.strong("Maximum ore units / binding limit");
                    ui.end_row();
                    for hull in &preview.configuration.hulls {
                        ui.label(hull.ship_class.spec().name);
                        ui.label(format!("{} kg / {} m³", hull.max_cargo_mass_kg, hull.usable_volume_m3))
                            .on_hover_text(&hull.provenance.reference);
                        match preview.configuration.assessment(hull.ship_class, 0) {
                            Ok(load) => {
                                ui.label(format!(
                                    "{} / {}",
                                    load.max_units,
                                    load.binding_limits.iter().map(|limit| limit.label()).collect::<Vec<_>>().join(" + ")
                                ));
                            }
                            Err(error) => {
                                ui.colored_label(Color32::YELLOW, error);
                            }
                        }
                        ui.end_row();
                    }
                });
            ui.checkbox(
                &mut self.activation_confirmed,
                "Use these saved ratings in this new test project",
            );
            create = ui
                .add_enabled(
                    ready && self.activation_confirmed,
                    egui::Button::new("Create new cargo test project"),
                )
                .clicked();
        }
        if create {
            self.create_test_project();
        }
        if let Some(path) = &self.created_project {
            ui.label(format!("Created project: {}", path.display()));
            ui.small("Open this folder as a separate editor project to use its applied cargo ratings. It has not been opened automatically. Later candidate edits do not change that world's configuration.");
            if ui.button("Copy new project path").clicked() {
                ui.ctx().copy_text(path.display().to_string());
            }
        }
    }
}

fn create_private_directory(path: &Path) -> Result<(), String> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path).map_err(|error| format!("Cannot create new test project directory: {error}"))
}

fn write_new_file(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|error| error.to_string())?;
    file.write_all(bytes).and_then(|()| file.sync_all()).map_err(|error| error.to_string())
}

fn create_project(project: &Path, preview: &ActivationPreview) -> Result<PathBuf, String> {
    preview.document.validate()?;
    let configuration = preview.document.shared_draft().validated_config()?;
    if configuration != preview.configuration {
        return Err("The reviewed configuration no longer matches its candidate.".into());
    }
    if read_bytes(project)?.as_deref() != Some(preview.source_bytes.as_slice()) {
        return Err("The saved candidate changed after review; no test project was created.".into());
    }
    let parent = project.join("CargoTests");
    if preview.destination.parent() != Some(parent.as_path()) {
        return Err("The new test project must be a fresh CargoTests child.".into());
    }
    if !ordinary_directory(&parent)? {
        match create_private_directory(&parent) {
            Ok(()) => (),
            Err(error) if ordinary_directory(&parent)? => {
                // Another creator may have reserved only the ordinary parent.
                let _ = error;
            }
            Err(error) => return Err(error),
        }
    }
    // Exclusive directory creation refuses existing projects, files and symlinks.
    create_private_directory(&preview.destination)?;
    let result = (|| {
        create_private_directory(&preview.destination.join("Assets"))?;
        write_new_file(&preview.destination.join(FILE), &preview.source_bytes)?;
        let world = EditorWorld::create_with_cargo_configuration(
            preview.destination.join(".arkline-studio"),
            configuration.clone(),
        )
        .map_err(|error| format!("Cannot initialize configured editor world: {error}"))?;
        if !world.is_paused() {
            return Err("The created editor world did not start paused.".into());
        }
        drop(world);
        let receipt = serde_json::json!({
            "schema_version": 1,
            "action": "create_new_local_cargo_test_project",
            "candidate_file": FILE,
            "candidate_sha256": format!("{:x}", Sha256::digest(&preview.source_bytes)),
            "cargo_configuration": configuration,
            "world_database": ".arkline-studio/world.sqlite",
            "starts_paused": true,
            "scene_layout": "orbital_v1",
            "existing_world_migration": false,
        });
        let mut bytes = serde_json::to_vec_pretty(&receipt).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        write_new_file(&preview.destination.join("Cargo.activation.json"), &bytes)?;
        Ok(preview.destination.clone())
    })();
    result.map_err(|error: String| format!(
        "Creation did not finish at {}: {error} The newly reserved folder was retained for inspection; no existing world was changed.",
        preview.destination.display()
    ))
}
