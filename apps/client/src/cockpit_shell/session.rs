//! Session and settings chrome with the existing authority header.

use super::*;

impl Cockpit {
    pub(crate) fn settings_panel(&mut self, ui: &mut egui::Ui) {
        if ui.button("Controls & ownership").clicked() {
            self.open_controls_reference();
        }
        let context = sound_settings::SoundContext {
            paused: self.embedded && self.embedded_paused,
            live_world: self.operations_fresh() && self.own().is_some(),
            listening: self.voice.is_listening(),
            speaking: self.voice.is_speaking(),
        };
        let action = self.sound_settings.ui(
            ui,
            &self.sound.output_status(),
            context,
            &self.sound_test_notice,
        );
        if action.changed {
            if let Err(error) = self.sound_settings.save(&self.data_dir) {
                self.settings_notice = error;
            }
        }
        if action.retry {
            self.sound.retry();
            self.sound_test_notice =
                "Audio retry requested · output status below reports the result".into();
        }
        if action.test {
            self.sound
                .set_mix(self.preferred_sound_mix(self.sound.current_mix()));
            self.sound_test_notice = match self.sound.test_output() {
                Ok(()) => "Two-tone test requested · listen to your output device".into(),
                Err(error) => error,
            };
        }
        ui.separator();
        ui.label("LIGHTING QUALITY");
        let previous = self.flight_view.lighting_quality;
        ui.horizontal_wrapped(|ui| {
            for quality in lighting_settings::LightingQuality::ALL {
                ui.selectable_value(
                    &mut self.flight_view.lighting_quality,
                    quality,
                    quality.label(),
                );
            }
        });
        ui.small("Low disables cast shadows. Balanced and High improve nearby shadows; drone screens use a smaller budget.");
        if previous != self.flight_view.lighting_quality {
            if let Err(error) = self.flight_view.lighting_quality.save(&self.data_dir) {
                self.settings_notice = error;
            }
        }
        ui.separator();
        ui.small("VIEW MODE");
        ui.horizontal_wrapped(|ui| self.view_selector(ui));
        ui.separator();
        ui.label(
            egui::RichText::new("COCKPIT APPEARANCE")
                .color(self.accent())
                .strong(),
        );
        ui.label("Choose a local interface accent. Your universe and pilot state are unchanged.");
        ui.separator();
        ui.label("Panel layout");
        if ui
            .checkbox(
                &mut self.layout.pinned,
                "Pin panels (disable moving and resizing)",
            )
            .changed()
        {
            self.layout.settings_changed();
        }
        if ui
            .checkbox(
                &mut self.layout.snap,
                "Snap panels to viewport edges and center",
            )
            .changed()
        {
            self.layout.settings_changed();
        }
        if ui.button("Reset panel layout").clicked() {
            self.layout.reset();
        }
        ui.small("Drag panel titles; release near an edge to snap. Layout saves locally and follows viewport resizing.");
        let previous = self.theme;
        ui.horizontal_wrapped(|ui| {
            for theme in theme::Theme::ALL {
                ui.selectable_value(
                    &mut self.theme,
                    theme,
                    egui::RichText::new(theme.name()).color(theme.accent()),
                );
            }
        });
        if self.theme != previous {
            self.theme.apply(ui.ctx());
            self.settings_notice = match self.theme.save(&self.data_dir) {
                Ok(()) => format!("{} appearance saved", self.theme.name()),
                Err(error) => error,
            };
        }
        ui.small("Near-black panels · High-contrast interface accents · Saved on this computer");
        ui.small(
            "World materials retain their physical colors; this setting changes the interface.",
        );
        if !self.settings_notice.is_empty() {
            ui.colored_label(MUTED, &self.settings_notice);
        }
    }
    // Opening a workspace changes presentation, never the user's saved preference.
    // An explicit Show/Hide while it is open overrides the temporary compact view.
    pub(crate) fn sync_workspace_identity(&mut self) {
        if !self.services_open {
            self.workspace_identity_override = None;
        }
    }
    pub(crate) fn identity_status_collapsed(&self) -> bool {
        if self.services_open {
            self.workspace_identity_override.unwrap_or(true)
        } else {
            self.identity_collapsed
        }
    }
    pub(crate) fn toggle_identity_status(&mut self) {
        let collapsed = !self.identity_status_collapsed();
        self.identity_collapsed = collapsed;
        if self.services_open {
            self.workspace_identity_override = Some(collapsed);
        }
    }
    /// The authority footer remains visible independently of this optional panel.
    pub(crate) fn identity_header(&mut self, ui: &mut egui::Ui) -> egui::Response {
        let wrap = self.services_open;
        let contents = |ui: &mut egui::Ui| {
            let button = ui
                .small_button(if self.identity_status_collapsed() {
                    "Show status"
                } else {
                    "Hide status"
                })
                .on_hover_text("Show/hide ship status · Ctrl+Shift+H (outside text fields)");
            if button.clicked() {
                self.toggle_identity_status();
            }
            if self.identity_status_collapsed() {
                let age = self.timeline_origin.map_or(u64::MAX, |origin| {
                    origin
                        .saturating_add(self.observed.elapsed().as_millis() as u64)
                        .saturating_sub(self.snapshot.as_ref().map_or(0, |s| s.server_time_ms))
                });
                let status = if !self.connected {
                    "DISCONNECTED"
                } else if self.embedded_paused {
                    "PAUSED"
                } else if age > 1500 {
                    "STALE"
                } else {
                    "CONNECTED"
                };
                ui.colored_label(
                    if status == "CONNECTED" {
                        self.accent()
                    } else {
                        AMBER
                    },
                    status,
                );
                ui.small(self.compact_control_label(status == "CONNECTED"));
                self.session_menu(ui);
            }
            button
        };
        if wrap {
            ui.horizontal_wrapped(contents).inner
        } else {
            ui.horizontal(contents).inner
        }
    }
    pub(crate) fn overlay_frame(&self, ctx: &egui::Context) -> egui::Frame {
        egui::Frame::window(&ctx.style())
            .fill(Color32::from_rgba_unmultiplied(3, 9, 6, 212))
            .stroke(Stroke::new(1.0, self.accent().gamma_multiply(0.32)))
            .inner_margin(8.0)
    }
    pub(crate) fn session_menu(&mut self, ui: &mut egui::Ui) {
        if self.embedded {
            ui.small("Embedded local world");
            return;
        }
        ui.menu_button("Session", |ui| {
            ui.small(&self.server);
            if self.connected {
                if ui.button("Open second cockpit").clicked() {
                    self.second_cockpit();
                    ui.close_menu();
                }
                if ui
                    .add_enabled(!self.request_pending, egui::Button::new("Disconnect"))
                    .clicked()
                {
                    self.disconnect_session();
                    ui.close_menu();
                }
            } else {
                if self.physical_session {
                    ui.small("Reopen the explicit local physical preview to reconnect.");
                }
                if ui
                    .add_enabled(
                        !self.connecting && !self.physical_session,
                        egui::Button::new("Reconnect"),
                    )
                    .clicked()
                {
                    self.connect();
                    ui.close_menu();
                }
                if ui.button("Change pilot / server").clicked() {
                    self.disconnect_session();
                    ui.close_menu();
                }
            }
        });
    }
}
