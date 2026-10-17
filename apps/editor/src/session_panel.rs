//! Switch the editor between its local world and a loopback session.
use arkline_client::{EmbeddedClient, EmbeddedRealm, EmbeddedSessionMode, EmbeddedStatus};
use arkline_protocol::SceneLayoutRevision;
use eframe::egui::{self, Color32};
use std::net::SocketAddr;

const LOOPBACK_DEFAULT: &str = "127.0.0.1:7779";
const WORLD_OPEN_RECOVERY: &str = "The selected world could not open. Your project world is preserved; choose Local, Project world, then Use local world to return.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Choice {
    #[default]
    Local,
    Host,
    Join,
}

#[derive(Default)]
pub struct SessionPanel {
    pub open: bool,
    initialized: bool,
    choice: Choice,
    realm: EmbeddedRealm,
    pilot: String,
    bind_address: String,
    join_address: String,
    join_secret: String,
    reveal_join_secret: bool,
    reveal_editor_secret: bool,
    error: Option<String>,
}

impl SessionPanel {
    pub fn show(&mut self, ctx: &egui::Context, client: &mut EmbeddedClient) {
        if !self.initialized {
            self.pilot = client.pilot().to_owned();
            self.realm = client.realm();
            self.choice = match client.session_mode() {
                EmbeddedSessionMode::Local => Choice::Local,
                EmbeddedSessionMode::Host { .. } => Choice::Host,
                EmbeddedSessionMode::Join { .. } => Choice::Join,
            };
            self.initialized = true;
        }
        if self.bind_address.is_empty() {
            self.bind_address = LOOPBACK_DEFAULT.to_owned();
        }
        if self.join_address.is_empty() {
            self.join_address = LOOPBACK_DEFAULT.to_owned();
        }

        // Use a local open flag so the window's mutable borrow does not
        // overlap edits to the rest of this panel state in the closure.
        let mut open = self.open;
        egui::Window::new("Multiplayer session")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(410.0)
            .max_height((ctx.available_rect().height() - 40.0).max(100.0))
            .vscroll(true)
            .show(ctx, |ui| self.contents(ui, client));
        self.open = open;
    }

    fn contents(&mut self, ui: &mut egui::Ui, client: &mut EmbeddedClient) {
        ui.heading("Session");
        ui.label(
            "The game world is shared authoritatively. Project and authoring files stay local.",
        );
        ui.small("Network access is loopback-only. Teammates can connect through an SSH tunnel.");
        ui.separator();

        let active = match client.session_mode() {
            EmbeddedSessionMode::Local => "Local world".to_owned(),
            EmbeddedSessionMode::Host { bind_addr } => format!("Host selected · {bind_addr}"),
            EmbeddedSessionMode::Join { address, .. } => format!("Join selected · {address}"),
        };
        ui.horizontal_wrapped(|ui| {
            ui.strong("Session mode");
            ui.label(active);
            ui.separator();
            ui.label(if client.can_control_clock() {
                "Clock control available"
            } else {
                "Host controls the clock"
            });
        });
        ui.label(format!(
            "Selected world: {}",
            if matches!(client.session_mode(), EmbeddedSessionMode::Join { .. }) {
                "Joined host world"
            } else {
                client.realm().label()
            }
        ));
        ui.label(format!(
            "Current layout: {}",
            match client.scene_layout_revision() {
                Some(SceneLayoutRevision::LegacyV1) =>
                    "LegacyV1 · original planet and linear berths",
                Some(SceneLayoutRevision::OrbitalV1) => {
                    "OrbitalV1 · measured planet and circular berths"
                }
                None if matches!(client.status(), EmbeddedStatus::Error(_)) => {
                    "Unavailable · selected world is not connected"
                }
                None => "Awaiting authority snapshot",
            }
        ));
        show_session_error(ui, client.status());

        if let Some(info) = client.session_info() {
            ui.label(&info.label);
            if let Some(address) = info.address {
                ui.monospace(address.to_string());
            }
            if info.is_host {
                if let Some(secret) = info.editor_secret.as_deref() {
                    ui.horizontal(|ui| {
                        ui.label("Editor secret");
                        if self.reveal_editor_secret {
                            ui.monospace(secret);
                        } else {
                            ui.label("••••••••••••");
                        }
                        if ui
                            .small_button(if self.reveal_editor_secret {
                                "Hide"
                            } else {
                                "Reveal"
                            })
                            .clicked()
                        {
                            self.reveal_editor_secret = !self.reveal_editor_secret;
                        }
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(secret.to_owned());
                        }
                    });
                    ui.small("The editor secret is shown only when you reveal it or copy it.");
                }
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.choice, Choice::Local, "Local");
            ui.selectable_value(&mut self.choice, Choice::Host, "Host");
            ui.selectable_value(&mut self.choice, Choice::Join, "Join");
        });
        self.realm_controls(ui);

        ui.horizontal(|ui| {
            let pilot_label = ui.label("Pilot");
            ui.add(
                egui::TextEdit::singleline(&mut self.pilot)
                    .desired_width(220.0)
                    .char_limit(24),
            )
            .labelled_by(pilot_label.id);
        });

        match self.choice {
            Choice::Local => {
                ui.label("Opens the selected preserved local world.");
                if ui.button("Use local world").clicked() {
                    self.switch(client, EmbeddedSessionMode::Local);
                }
            }
            Choice::Host => {
                ui.horizontal(|ui| {
                    ui.label("Loopback bind address");
                    ui.add(egui::TextEdit::singleline(&mut self.bind_address).desired_width(180.0));
                });
                ui.small("Hosting keeps this editor open and shares the selected preserved world.");
                if ui.button("Host shared world").clicked() {
                    match parse_loopback(&self.bind_address) {
                        Ok(bind_addr) => {
                            self.switch(client, EmbeddedSessionMode::Host { bind_addr });
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            Choice::Join => {
                ui.horizontal(|ui| {
                    ui.label("Loopback address");
                    ui.add(egui::TextEdit::singleline(&mut self.join_address).desired_width(180.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Editor secret");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.join_secret)
                            .password(!self.reveal_join_secret)
                            .desired_width(180.0),
                    );
                    if ui
                        .small_button(if self.reveal_join_secret {
                            "Hide"
                        } else {
                            "Show"
                        })
                        .clicked()
                    {
                        self.reveal_join_secret = !self.reveal_join_secret;
                    }
                });
                ui.small("Joining changes only this editor's session; it never replaces your local world.");
                if ui.button("Join shared world").clicked() {
                    match parse_loopback(&self.join_address) {
                        Ok(address) if !self.join_secret.is_empty() => {
                            let secret = std::mem::take(&mut self.join_secret);
                            self.reveal_join_secret = false;
                            self.switch(client, EmbeddedSessionMode::Join { address, secret });
                        }
                        Ok(_) => {
                            self.error =
                                Some("Enter the editor secret supplied by the host.".into())
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }

        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
    }

    fn realm_controls(&mut self, ui: &mut egui::Ui) {
        match self.choice {
            Choice::Local | Choice::Host => {
                ui.horizontal_wrapped(|ui| {
                    ui.strong("World");
                    for realm in [EmbeddedRealm::Project, EmbeddedRealm::OrbitalPreview] {
                        ui.selectable_value(&mut self.realm, realm, realm.label());
                    }
                });
                ui.small("Existing worlds keep their original planet and berths. Orbital station preview opens a separate persistent world; it does not convert the project world.");
            }
            Choice::Join => {
                ui.small("The remote host chooses the world layout. Your preserved local worlds stay on this computer.");
            }
        }
    }

    fn switch(&mut self, client: &mut EmbeddedClient, mode: EmbeddedSessionMode) {
        let pilot = self.pilot.trim().to_owned();
        let result = if matches!(mode, EmbeddedSessionMode::Join { .. }) {
            client.switch_session(mode, pilot)
        } else {
            client.switch_session_in_realm(mode, pilot, self.realm)
        };
        self.error = match result {
            Ok(()) => {
                self.reveal_editor_secret = false;
                None
            }
            Err(error) => Some(error),
        };
    }
}

fn show_session_error(ui: &mut egui::Ui, status: &EmbeddedStatus) {
    if let EmbeddedStatus::Error(error) = status {
        ui.colored_label(Color32::LIGHT_RED, error);
        ui.colored_label(Color32::LIGHT_RED, WORLD_OPEN_RECOVERY);
    }
}

fn parse_loopback(text: &str) -> Result<SocketAddr, String> {
    let address: SocketAddr = text
        .trim()
        .parse()
        .map_err(|_| "Enter a valid IP address and port, such as 127.0.0.1:7779.".to_owned())?;
    if !address.ip().is_loopback() {
        return Err(
            "Only loopback addresses are supported. Use an SSH tunnel for teammates.".into(),
        );
    }
    Ok(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_and_host_offer_realms_but_join_uses_the_remote_layout() {
        for choice in [Choice::Local, Choice::Host, Choice::Join] {
            let ctx = egui::Context::default();
            let mut panel = SessionPanel {
                choice,
                ..Default::default()
            };
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| panel.realm_controls(ui));
            });
            let labels: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect();
            for label in ["Project world", "Orbital station preview"] {
                assert_eq!(labels.contains(&label), choice != Choice::Join);
            }
            if choice == Choice::Join {
                assert!(labels
                    .iter()
                    .any(|label| label.contains("remote host chooses")));
            }
        }
    }

    #[test]
    fn asynchronous_open_error_shows_worker_reason_and_project_return_path() {
        let ctx = egui::Context::default();
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                show_session_error(
                    ui,
                    &EmbeddedStatus::Error("Cannot open world database".into()),
                );
            });
        });
        let labels: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&"Cannot open world database"));
        assert!(labels.contains(&WORLD_OPEN_RECOVERY));
    }
}
