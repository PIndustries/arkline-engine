//! Focused workspaces over existing authoritative services and commands.
use super::*;

struct PanelDestination {
    tab: ServiceTab,
    label: &'static str,
    title: &'static str,
    keywords: &'static str,
}

const PANEL_DESTINATIONS: &[PanelDestination] = &[
    PanelDestination {
        tab: ServiceTab::Navigation,
        label: "Navigation",
        title: "Flight · Navigation",
        keywords: "map route target warp orbit approach travel",
    },
    PanelDestination {
        tab: ServiceTab::Settings,
        label: "Preferences",
        title: "Flight · Preferences",
        keywords: "settings sound audio volume voice controls input appearance theme lighting shadow quality panel positioning",
    },
    PanelDestination {
        tab: ServiceTab::Hangar,
        label: "Fleet",
        title: "Ship · Fleet",
        keywords: "hangar vessel ship selection ownership",
    },
    PanelDestination {
        tab: ServiceTab::Modules,
        label: "Modules",
        title: "Ship · Modules",
        keywords: "turret mining equipment capacitor target lock manual",
    },
    PanelDestination {
        tab: ServiceTab::Cargo,
        label: "Cargo & storage",
        title: "Ship · Cargo & storage",
        keywords: "hold ore inventory transfer deposit",
    },
    PanelDestination {
        tab: ServiceTab::Layouts,
        label: "Layouts",
        title: "Ship · Layouts",
        keywords: "ship hull deck room equipment engineering design reference",
    },
    PanelDestination {
        tab: ServiceTab::Sandbox,
        label: "Surface tools",
        title: "Ship · Surface tools",
        keywords: "sandbox placement props ground",
    },
    PanelDestination {
        tab: ServiceTab::ShipComputer,
        label: "Mining",
        title: "Operations · Mining",
        keywords: "routine autopilot automation goal delivered haul ai computer",
    },
    PanelDestination {
        tab: ServiceTab::Drones,
        label: "Drones",
        title: "Operations · Drones",
        keywords: "fleet launch recall assignment camera feeds recon attack mining",
    },
    PanelDestination {
        tab: ServiceTab::Activity,
        label: "Activity",
        title: "Operations · Activity",
        keywords: "events log history messages",
    },
    PanelDestination {
        tab: ServiceTab::Ledger,
        label: "Audit",
        title: "Operations · Audit",
        keywords: "ledger transactions receipts conservation integrity",
    },
    PanelDestination {
        tab: ServiceTab::Station,
        label: "Services",
        title: "Station · Services",
        keywords: "boarding interior seat hatch airlock lift quarters",
    },
    PanelDestination {
        tab: ServiceTab::Exchange,
        label: "Exchange",
        title: "Station · Exchange",
        keywords: "market trading trade buy sell",
    },
    PanelDestination {
        tab: ServiceTab::Corporation,
        label: "Corporation",
        title: "Station · Corporation",
        keywords: "corp membership invitation organization",
    },
];

fn panel_matches(destination: &PanelDestination, query: &str) -> bool {
    let searchable = format!("{} {}", destination.title, destination.keywords).to_lowercase();
    query
        .split_whitespace()
        .all(|token| searchable.contains(&token.to_lowercase()))
}

fn group(tab: ServiceTab) -> usize {
    match tab {
        ServiceTab::Navigation | ServiceTab::Settings => 0,
        ServiceTab::Hangar
        | ServiceTab::Modules
        | ServiceTab::Cargo
        | ServiceTab::Layouts
        | ServiceTab::Sandbox => 1,
        ServiceTab::ShipComputer
        | ServiceTab::Drones
        | ServiceTab::Activity
        | ServiceTab::Ledger => 2,
        ServiceTab::Station | ServiceTab::Exchange | ServiceTab::Corporation => 3,
    }
}

impl Cockpit {
    /// A local navigation action: never submits a ship, avatar or clock command.
    pub(super) fn close_operations_workspace(&mut self) {
        self.services_open = false;
        self.overview_open = false;
        self.service_tab = ServiceTab::Navigation;
        self.release_workspace_input();
    }

    pub(super) fn record_operations_keyboard_owner(&mut self, ctx: &egui::Context) {
        self.workspace_keyboard_owned_last_frame = ctx.wants_keyboard_input()
            || ctx.memory(|memory| memory.focused().is_some())
            || self.cockpit_computer.wants_keyboard_input();
    }

    pub(super) fn operations_keyboard_navigation(&mut self, ctx: &egui::Context) {
        // egui can surrender a TextEdit's focus at begin-pass on Escape. Preserve
        // its previous-frame ownership so the same key cannot also close a task.
        let previously_owned = std::mem::take(&mut self.workspace_keyboard_owned_last_frame);
        let open = self.services_open || self.service_tab == ServiceTab::Layouts;
        if !open
            || self.controls_reference_open
            || previously_owned
            || !self.embedded_input
            || ctx.wants_keyboard_input()
            || ctx.memory(|memory| memory.focused().is_some())
            || self.cockpit_computer.wants_keyboard_input()
            || !ctx.input(|i| i.focused && !i.pointer.any_down())
        {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, crate::control_bindings::WORKSPACE_BACK)) {
            self.close_operations_workspace();
        }
    }
    pub(super) fn operations_fresh(&self) -> bool {
        self.connected
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    <= 1500
            })
    }

    pub(super) fn manual_operations_summary(&self, ui: &mut egui::Ui) {
        let Some(ship) = self.own() else {
            return;
        };
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("CONTROL OWNERSHIP");
                ui.label(format!("Ship · {}", ship.name));
            });
            if let Some(job) = self.snapshot.as_ref().and_then(|snapshot| {
                snapshot
                    .resource_jobs
                    .iter()
                    .find(|job| job.pilot == ship.name && !job.phase.is_terminal())
            }) {
                ui.colored_label(
                    AMBER,
                    format!(
                        "Routine #{} active · manual orders cancel this mining routine",
                        job.id
                    ),
                );
                ui.small("Viewing telemetry or selecting a drone does not take control.");
            } else {
                ui.small("Manual operations · no active mining routine reported for this ship");
            }
            let reason = if !self.connected {
                Some("Disconnected · commands unavailable")
            } else if self.embedded_paused {
                Some("Paused · resume simulation to issue commands")
            } else if !self.operations_fresh() {
                Some("Stale telemetry · waiting for authority before issuing commands")
            } else if self.request_pending {
                Some("Command pending · waiting for authority confirmation")
            } else if ship.station_avatar.is_some() {
                Some("On foot · manual ship orders require the pilot seat")
            } else {
                None
            };
            if let Some(reason) = reason {
                ui.colored_label(MUTED, reason);
            }
        });
    }
    pub(super) fn operations_route(&self) -> &'static str {
        ["Flight", "Ship", "Operations", "Station"][group(self.service_tab)]
    }

    pub(super) fn operations_title(&self) -> &'static str {
        PANEL_DESTINATIONS
            .iter()
            .find(|destination| destination.tab == self.service_tab)
            .expect("Every service tab has a registered panel destination")
            .title
    }

    pub(super) fn operations_navigation(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("← Back to Flight")
                .on_hover_text("Escape returns to Flight when no text field owns the keyboard.")
                .clicked()
            {
                self.close_operations_workspace();
            }
            ui.small("Esc · Back to Flight");
            ui.separator();
        });
        let search_id = ui.make_persistent_id("operations_panel_search");
        let search_focused = ui
            .ctx()
            .memory(|memory| memory.focused() == Some(search_id));
        let focus_search = (self.services_open || self.service_tab == ServiceTab::Layouts)
            && !self.controls_reference_open
            && self.embedded_input
            && !self.cockpit_computer.wants_keyboard_input()
            && (search_focused
                || (!ui.ctx().wants_keyboard_input()
                    && ui.ctx().memory(|memory| memory.focused().is_none())))
            && ui
                .ctx()
                .input(|input| input.focused && !input.pointer.any_down())
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::COMMAND, crate::control_bindings::PANEL_SEARCH));
        ui.horizontal_wrapped(|ui| {
            ui.label("Search panels");
            let width = (ui.available_width() - 110.0).clamp(80.0, 320.0);
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.operations_search)
                    .id(search_id)
                    .char_limit(128)
                    .desired_width(width)
                    .hint_text("Mining, cargo, settings…"),
            );
            if focus_search {
                search.request_focus();
            }
            ui.small("Cmd/Ctrl+K");
            if ui
                .add_enabled(
                    !self.operations_search.is_empty(),
                    egui::Button::new("Clear search"),
                )
                .clicked()
            {
                self.operations_search.clear();
            }
        });
        if self.operations_search.trim().is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (index, label, default) in [
                    (0, "Flight", ServiceTab::Navigation),
                    (1, "Ship", ServiceTab::Hangar),
                    (2, "Operations", ServiceTab::ShipComputer),
                    (3, "Station", ServiceTab::Station),
                ] {
                    if ui
                        .selectable_label(group(self.service_tab) == index, label)
                        .clicked()
                    {
                        self.service_tab = default;
                    }
                }
            });
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for destination in PANEL_DESTINATIONS {
                    if group(destination.tab) == group(self.service_tab)
                        && ui
                            .selectable_label(
                                self.service_tab == destination.tab,
                                destination.label,
                            )
                            .clicked()
                    {
                        self.service_tab = destination.tab;
                        self.sound.cue(sound_engine::Cue::Ui);
                    }
                }
            });
        } else {
            ui.small(
                "Panel destinations only · current panel stays open until you select a result",
            );
            let matches: Vec<_> = PANEL_DESTINATIONS
                .iter()
                .filter(|destination| panel_matches(destination, &self.operations_search))
                .collect();
            if matches.is_empty() {
                ui.label("No matching panels. Try a panel name or clear the search.");
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("operations_panel_search_results")
                    .max_height(132.0)
                    .min_scrolled_height(0.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for destination in matches {
                            if ui
                                .selectable_label(
                                    self.service_tab == destination.tab,
                                    destination.title,
                                )
                                .clicked()
                            {
                                self.service_tab = destination.tab;
                                self.sound.cue(sound_engine::Cue::Ui);
                            }
                        }
                    });
            }
        }
        // Mining owns its concise local authority state; avoid repeating its
        // pause, pending and response notices above the same summary.
        if self.service_tab != ServiceTab::ShipComputer {
            if !self.connected {
                ui.colored_label(
                    AMBER,
                    "Disconnected · showing last received state; controls unavailable",
                );
            } else if self.embedded && self.embedded_paused {
                ui.colored_label(
                    MUTED,
                    "Simulation paused · choose Resume simulation to operate ship controls",
                );
            } else if self.snapshot.as_ref().is_none_or(|snapshot| {
                self.presentation_time()
                    .saturating_sub(snapshot.server_time_ms)
                    > 1500
            }) {
                ui.colored_label(
                    AMBER,
                    "Waiting for a world update · shown values may be out of date",
                );
            }
            if self.request_pending {
                ui.colored_label(AMBER, "Command pending · awaiting confirmation");
            }
            if !self.notice.is_empty() {
                ui.small(format!("Last response: {}", self.notice));
            }
        }
        ui.separator();
    }
}

#[cfg(test)]
mod panel_search_tests {
    use super::*;

    fn frame(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        events: Vec<egui::Event>,
        unrelated: bool,
    ) -> Vec<(String, Pos2)> {
        let mut other = String::from("Other editor field");
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1000., 800.),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if unrelated {
                        ui.add(
                            egui::TextEdit::singleline(&mut other)
                                .id(egui::Id::new("other_panel_search_test")),
                        );
                    }
                    cockpit.operations_navigation(ui);
                });
            },
        )
        .shapes
        .into_iter()
        .filter_map(|shape| {
            if let egui::Shape::Text(text) = shape.shape {
                Some((
                    text.galley.job.text.clone(),
                    text.pos + text.galley.rect.center().to_vec2(),
                ))
            } else {
                None
            }
        })
        .collect()
    }

    fn click(point: Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    fn shortcut() -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: crate::control_bindings::PANEL_SEARCH,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }]
    }

    #[test]
    fn panel_search_registry_covers_all_destinations_and_token_matching() {
        assert_eq!(PANEL_DESTINATIONS.len(), 14);
        for (index, destination) in PANEL_DESTINATIONS.iter().enumerate() {
            assert!(PANEL_DESTINATIONS[..index]
                .iter()
                .all(|other| other.tab != destination.tab));
            assert!(panel_matches(destination, destination.label));
        }
        let results = |query| {
            PANEL_DESTINATIONS
                .iter()
                .filter(|destination| panel_matches(destination, query))
                .map(|destination| destination.tab)
                .collect::<Vec<_>>()
        };
        assert!(results("  OpERATIONS   RECALL ") == vec![ServiceTab::Drones]);
        assert!(results("station hatch") == vec![ServiceTab::Station]);
        assert!(results("ship engineering") == vec![ServiceTab::Layouts]);
        assert!(results("missing destination").is_empty());
    }

    #[test]
    fn panel_search_typing_no_match_clear_and_result_click_preserve_world() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.services_open = true;
        cockpit.embedded_input = true;
        cockpit.service_tab = ServiceTab::Cargo;
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        frame(&ctx, &mut cockpit, vec![], false);
        frame(&ctx, &mut cockpit, shortcut(), false);
        frame(
            &ctx,
            &mut cockpit,
            vec![egui::Event::Text("zznotapanel".into())],
            false,
        );
        assert_eq!(cockpit.operations_search, "zznotapanel");
        let labels = frame(&ctx, &mut cockpit, vec![], false);
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("No matching panels")));
        assert!(labels.iter().any(|(text, _)| text == "← Back to Flight"));
        assert!(cockpit.service_tab == ServiceTab::Cargo);
        let point = labels
            .iter()
            .find(|(text, _)| text == "Clear search")
            .unwrap()
            .1;
        for pressed in [true, false] {
            frame(&ctx, &mut cockpit, click(point, pressed), false);
        }
        assert!(cockpit.operations_search.is_empty());
        assert!(cockpit.service_tab == ServiceTab::Cargo);
        cockpit.operations_search = "recall".into();
        frame(
            &ctx,
            &mut cockpit,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            false,
        );
        assert!(cockpit.service_tab == ServiceTab::Cargo);
        frame(&ctx, &mut cockpit, vec![], false);
        let labels = frame(&ctx, &mut cockpit, vec![], false);
        let point = labels
            .iter()
            .find(|(text, _)| text == "Operations · Drones")
            .unwrap()
            .1;
        for pressed in [true, false] {
            frame(&ctx, &mut cockpit, click(point, pressed), false);
        }
        assert!(cockpit.service_tab == ServiceTab::Drones);
        assert!(rx.try_recv().is_err());
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }

    #[test]
    fn panel_search_shortcut_respects_unrelated_text_and_viewport_ownership() {
        for unrelated in [true, false] {
            let ctx = egui::Context::default();
            let mut cockpit = crate::tests::view_test_cockpit(&ctx);
            cockpit.services_open = true;
            cockpit.embedded_input = unrelated;
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            let (tx, rx) = mpsc::channel();
            cockpit.commands = Some(tx);
            frame(&ctx, &mut cockpit, vec![], unrelated);
            if unrelated {
                ctx.memory_mut(|memory| {
                    memory.request_focus(egui::Id::new("other_panel_search_test"))
                });
            }
            let previous = ctx.memory(|memory| memory.focused());
            frame(&ctx, &mut cockpit, shortcut(), unrelated);
            assert_eq!(ctx.memory(|memory| memory.focused()), previous);
            assert!(cockpit.operations_search.is_empty());
            assert!(rx.try_recv().is_err());
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        }
    }
}
