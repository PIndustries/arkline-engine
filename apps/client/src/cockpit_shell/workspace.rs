//! Cockpit action banks and the fixed physical-computer shell.

use super::*;

impl Cockpit {
    pub(crate) fn cockpit_actions(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own() else { return };
        let readiness = self.navigation_order_reason();
        let ready = readiness.is_none() && self.live_command_snapshot_fresh();
        let departure_ready = self.departure_order_reason().is_none();
        let docked = pilot.location == Location::Docked;
        let (status, help) = if !self.connected {
            (
                "OFFLINE",
                "Reconnect to the authority before issuing ship orders",
            )
        } else if self.embedded_paused {
            ("PAUSED", "Resume simulation before issuing ship orders")
        } else if self.request_pending {
            ("PENDING", "Command queued; awaiting the authority response")
        } else if !self.live_command_snapshot_fresh() {
            (
                "STALE",
                "Last received readings; waiting for a fresh authority snapshot",
            )
        } else if pilot.station_avatar.is_some() {
            (
                "ON FOOT",
                "Physically return to the pilot seat for ship orders",
            )
        } else if let Some(d) = pilot
            .flight
            .as_ref()
            .and_then(|f| f.docking.as_ref())
            .filter(|d| d.phase.busy())
        {
            (
                d.phase.label(),
                "Authority owns this movement; hold to brake, then resume when clear",
            )
        } else if let Some(reason) = self.bay_departure_reason() {
            (
                if self.bay_maneuver_reason().is_some() {
                    "BAY INTERLOCK"
                } else {
                    "DRONES DEPLOYED"
                },
                reason,
            )
        } else if docked && self.cargo_departure_reason().is_some() {
            ("CARGO RESERVED", self.cargo_departure_reason().unwrap())
        } else if docked {
            ("DOCKED", "Authority reports the ship docked")
        } else if matches!(pilot.location, Location::InTransit { .. }) {
            (
                "WARP",
                "Transit in progress; berth and stop controls unavailable",
            )
        } else if matches!(pilot.location, Location::Jumping { .. }) {
            (
                "JUMP",
                "Gate transit in progress; berth and stop controls unavailable",
            )
        } else if self.can_dock(&pilot) {
            (
                "PORT IN RANGE",
                "Authority reports the docking port within range",
            )
        } else {
            ("IN FLIGHT", "Approach the station docking port to dock")
        };
        let state_color =
            if ready && (!docked || departure_ready) && self.bay_departure_reason().is_none() {
                self.accent()
            } else {
                AMBER
            };
        let text_size = if self.cockpit_console_detail {
            14.0
        } else {
            22.0
        };
        let caption_size = if self.cockpit_console_detail {
            12.0
        } else {
            16.0
        };
        // Absolute canvas rectangles are shared with the physical cartridges.
        // Help/status length and compact/detail typography cannot move the hit faces.
        ui.allocate_space(Vec2::new(560.0, 82.0));
        for (label, x) in [("BERTH", 12.0), ("FLIGHT", 164.0), ("AUTHORITY", 280.0)] {
            ui.painter().text(
                Pos2::new(x, 12.0),
                egui::Align2::LEFT_TOP,
                label,
                egui::FontId::proportional(caption_size),
                MUTED,
            );
        }
        let (label, enabled, command) = if let Some(d) = pilot
            .flight
            .as_ref()
            .and_then(|f| f.docking.as_ref())
            .filter(|d| d.phase.busy())
        {
            let held = d.phase == DockingPhase::Held;
            (
                if held { "RESUME" } else { "HOLD" },
                ready && self.can_operate(),
                if held {
                    Request::ResumeDocking {
                        operation_id: d.operation_id,
                    }
                } else {
                    Request::CancelDocking {
                        operation_id: d.operation_id,
                    }
                },
            )
        } else if docked {
            (
                "UNDOCK",
                departure_ready && self.can_operate() && pilot.station_avatar.is_none(),
                Request::Undock,
            )
        } else {
            ("DOCK", ready && self.can_dock(&pilot), Request::Dock)
        };
        let berth_rect = egui::Rect::from_min_max(Pos2::new(12.0, 32.0), Pos2::new(144.0, 70.0));
        let mut berth_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("physical_berth")
                .max_rect(berth_rect),
        );
        if !enabled {
            berth_ui.disable();
        }
        let berth = berth_ui.put(
            berth_rect,
            egui::Button::new(egui::RichText::new(label).strong().size(text_size)).frame(false),
        );
        if berth.contains_pointer() {
            berth.show_tooltip_ui(|ui| {
                ui.set_max_width(400.0);
                ui.label(help);
            });
        }
        if berth.clicked() {
            self.send(command);
        }
        let braking = matches!(pilot.location, Location::Station | Location::Belt)
            && pilot
                .flight
                .as_ref()
                .is_some_and(|flight| matches!(flight.mode, NavigationMode::Stop));
        let stop_rect = egui::Rect::from_min_max(Pos2::new(164.0, 32.0), Pos2::new(260.0, 70.0));
        let mut stop_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("physical_stop")
                .max_rect(stop_rect),
        );
        if !(ready && self.can_fly()) {
            stop_ui.disable();
        }
        let stop = stop_ui.put(
            stop_rect,
            egui::Button::new(egui::RichText::new("STOP").size(text_size)).frame(false),
        );
        if stop.contains_pointer() {
            stop.show_tooltip_ui(|ui| {
                ui.set_max_width(400.0);
                ui.label(help);
            });
        }
        if stop.clicked() {
            self.send(Request::Navigate {
                mode: NavigationMode::Stop,
            });
        }
        // Real hover/pressed and confirmed braking feedback belongs to the
        // unchanged input face; no opaque UI rectangle conceals modeled relief.
        for (response, confirmed) in [(&berth, false), (&stop, braking)] {
            if response.hovered() || response.is_pointer_button_down_on() || confirmed {
                ui.painter().rect_stroke(
                    response.rect.shrink(1.0),
                    2.0,
                    egui::Stroke::new(
                        if response.is_pointer_button_down_on() {
                            2.0
                        } else {
                            1.0
                        },
                        self.accent(),
                    ),
                    egui::StrokeKind::Inside,
                );
            }
        }
        ui.painter().text(
            Pos2::new(280.0, 33.0),
            egui::Align2::LEFT_TOP,
            status,
            egui::FontId::proportional(caption_size),
            state_color,
        );
        if ui
            .put(
                egui::Rect::from_min_max(Pos2::new(280.0, 54.0), Pos2::new(420.0, 78.0)),
                egui::Button::new(egui::RichText::new("Expand panel").size(18.0)).small(),
            )
            .clicked()
        {
            self.services_open = true;
        }
        // Compact status already names the authority state. Keep detailed
        // guidance on both control faces, reserving rows for actual readings.
        if self.cockpit_console_detail {
            ui.label(egui::RichText::new(help).size(14.0).color(MUTED));
        } else {
            ui.skip_ahead_auto_ids(1); // Preserve the removed label's ID slot.
        }
        if self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .resource_jobs
                .iter()
                .any(|job| job.pilot == pilot.name && !job.phase.is_terminal())
        }) {
            ui.label(
                egui::RichText::new("Manual ship orders can cancel the active mining routine")
                    .size(14.0)
                    .color(AMBER),
            );
        }
    }
    /// Workspace selectors share their fixed canvas faces with the modeled
    /// cartridges. These change local UI context, never ship authority.
    pub(crate) fn cockpit_workspace_keys(&mut self, ui: &mut egui::Ui) {
        for (label, id, tab, left, right) in [
            (
                "FLIGHT",
                "cockpit.workspace.flight",
                ServiceTab::Navigation,
                12.0,
                180.0,
            ),
            (
                "DRONES",
                "cockpit.workspace.drones",
                ServiceTab::Drones,
                196.0,
                364.0,
            ),
            (
                "MINING",
                "cockpit.workspace.mining",
                ServiceTab::ShipComputer,
                380.0,
                548.0,
            ),
        ] {
            let rect = egui::Rect::from_min_max(Pos2::new(left, 330.0), Pos2::new(right, 374.0));
            let mut key_ui = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(rect));
            let response = key_ui.put(
                rect,
                egui::Button::new(egui::RichText::new(label).strong().size(24.0)).frame(false),
            );
            let selected = self.services_open && self.service_tab == tab;
            if selected || response.hovered() || response.is_pointer_button_down_on() {
                ui.painter().rect_stroke(
                    rect.shrink(2.0),
                    2.0,
                    egui::Stroke::new(
                        if selected || response.is_pointer_button_down_on() {
                            2.0
                        } else {
                            1.0
                        },
                        self.accent(),
                    ),
                    egui::StrokeKind::Inside,
                );
            }
            response.clone().on_hover_text(
                "Open workspace · view selection only; ship control remains unchanged",
            );
            if response.clicked() {
                self.service_tab = tab;
                self.services_open = true;
            }
        }
    }
    /// Build the physical computer in one fixed logical canvas. Perspective
    /// changes only its 3D vertices and ray map, never egui's allocation sizes.
    pub(crate) fn render_cockpit_computer(&mut self, ctx: &egui::Context, in_hangar: bool) {
        use flight3d::cockpit_computer::ComputerPermission;
        let docked = in_hangar && self.verified_docked_binding().is_some();
        let surface = if docked {
            self.hangar_view.computer_surface()
        } else {
            self.flight_view.computer_surface()
        };
        let hit: Box<dyn Fn(egui::Pos2) -> Option<egui::Pos2>> = if docked {
            Box::new(self.hangar_view.computer_hit_tester(ctx))
        } else {
            Box::new(self.flight_view.computer_hit_tester(ctx))
        };
        let active = self.connected
            && (docked
                || !in_hangar && self.flight_view.cockpit_presentation_active(self.view_mode));
        let input_allowed = !self.ship_observation
            && !self.controls_reference_open
            && !self.performance_open
            && !self.services_open
            && self.service_tab != ServiceTab::Layouts
            && !self.workspace_input_waiting_release
            && self.embedded_input
            && !self.embedded_paused
            && ctx.input(|input| input.focused && !input.viewport().minimized.unwrap_or(false));
        // A real glass hit may transfer focus from an editor/overlay field.
        // Layer/depth rejection happens for each coordinate in the event batch.
        // Context::input holds egui's exclusive context lock. The hit tester
        // queries that same context's layers, so call it only after the input
        // transaction has ended; otherwise the first glass press deadlocks.
        let pressed_positions = if active && input_allowed {
            ctx.input(|input| {
                input
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        egui::Event::PointerButton {
                            pos, pressed: true, ..
                        }
                        | egui::Event::Touch {
                            pos,
                            phase: egui::TouchPhase::Start,
                            ..
                        } => Some(*pos),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        } else {
            Vec::new()
        };
        let glass_pressed = pressed_positions
            .into_iter()
            .any(|point| hit(point).is_some());
        if glass_pressed {
            ctx.memory_mut(|memory| {
                if let Some(focused) = memory.focused() {
                    memory.surrender_focus(focused);
                }
            });
        }
        let permission = ComputerPermission {
            active,
            pointer: input_allowed,
            keyboard: input_allowed && !ctx.wants_keyboard_input(),
        };
        let mut computer = std::mem::take(&mut self.cockpit_computer);
        #[cfg(test)]
        let mut labels = None;
        let output = computer.run(ctx, surface.as_ref(), permission, hit, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(4.0, 3.0);
            if !self.cockpit_console_detail {
                for font in ui.style_mut().text_styles.values_mut() {
                    font.size = font.size.max(22.0);
                }
            }
            self.cockpit_actions(ui);
            let display_state = if !self.connected {
                "Disconnected · last received state · controls locked"
            } else if self.snapshot.is_none() {
                "Synchronizing · waiting for world data"
            } else if self.embedded_paused {
                "Paused · display held · controls locked"
            } else if !self.live_command_snapshot_fresh() {
                "Stale world data · values may be out of date"
            } else if self.ship_observation {
                "Observation only · pilot remains on foot · controls locked"
            } else if !input_allowed {
                "Live state · console input inactive"
            } else {
                "Live state"
            };
            if !(self.cockpit_console_detail && self.service_tab == ServiceTab::Displays) {
                ui.label(display_state);
            }
            if self.cockpit_console_detail {
                if self.service_tab == ServiceTab::Displays {
                    ui.horizontal(|ui| {
                        if ui.small_button("‹ Flight").clicked() {
                            self.cockpit_console_detail = false;
                        }
                        self.cockpit_detail_tabs(ui);
                    });
                } else {
                    if ui.small_button("Flight view").clicked() {
                        self.cockpit_console_detail = false;
                    }
                    self.power_gauge(ui);
                    self.cockpit_detail_tabs(ui);
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .min_scrolled_height(0.0)
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .id_salt("cockpit_console_content")
                    .max_height((322.0 - ui.cursor().top()).max(0.0))
                    .show(ui, |ui| {
                        if self.service_tab == ServiceTab::Displays {
                            self.captain_displays_compact(ui);
                        } else {
                            self.service_content(ui);
                        }
                    });
            } else {
                self.compact_flight_summary(ui);
                if ui
                    .put(
                        egui::Rect::from_min_max(Pos2::new(380.0, 300.0), Pos2::new(548.0, 324.0)),
                        egui::Button::new(egui::RichText::new("DETAILS").size(16.0)).small(),
                    )
                    .on_hover_text("Expand console pages")
                    .clicked()
                {
                    self.cockpit_console_detail = true;
                }
            }
            self.cockpit_workspace_keys(ui);
            #[cfg(test)]
            {
                // Observe the actual secondary-context widgets for CPU input
                // journeys. This never adds text or geometry to the parent UI.
                labels = Some(ui.ctx().graphics_mut(|graphics| {
                    graphics
                        .entry(ui.layer_id())
                        .all_entries()
                        .filter_map(|shape| match &shape.shape {
                            egui::Shape::Text(text) => {
                                let center = text.pos + text.galley.rect.center().to_vec2();
                                shape
                                    .clip_rect
                                    .contains(center)
                                    .then(|| (text.galley.text().to_owned(), center))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                }));
            }
        });
        self.cockpit_computer = computer;
        #[cfg(test)]
        if let Some(labels) = labels {
            self.computer_ui_labels = if active && surface.is_some() {
                labels
            } else {
                Vec::new()
            };
        }
        self.flight_view
            .set_computer_keyboard_owned(!docked && output.wants_keyboard_input);
        self.hangar_view
            .set_computer_keyboard_owned(docked && output.wants_keyboard_input);
        if output.wants_keyboard_input {
            let ime = output.platform_output.ime.and_then(|ime| {
                surface
                    .as_ref()
                    .and_then(|surface| project_computer_ime(surface, ime))
            });
            ctx.output_mut(|parent| parent.ime = ime);
        }
        if !output.diagnostics.is_empty() {
            self.settings_notice = format!("Cockpit display: {}", output.diagnostics.join("; "));
        }
        if active {
            if docked {
                self.hangar_view.submit_computer(ctx, output.primitives);
            } else {
                self.flight_view.submit_computer(ctx, output.primitives);
            }
        }
    }
}
