//! Live inspection and bounded controls for catalogued Pioneer room equipment.
//!
//! This panel renders only server-reported state and catalog declarations. It
//! does not create or infer equipment geometry. The caller owns authorization,
//! transport, and server interlocks, and should pass `controls_enabled` only
//! when the existing session gate permits room commands.
use arkline_protocol::{
    pioneer_room_catalog, AssetsView, EquipmentKind, PioneerRoomCommand, Request, Snapshot,
    VentMode,
};
use eframe::egui::{self, RichText};

/// Separates live transport requests from local event-graph authoring.
#[derive(Debug, Clone)]
pub enum Action {
    Send(Request),
    AddToGraph(PioneerRoomCommand),
}

/// Show a selected catalog device and return a typed request when an enabled
/// supported control is activated. `snapshot` and `assets` must be the latest
/// authoritative data for the selected pilot; pass `None` when unavailable.
/// The stable catalog equipment id is used for selection, never a display label.
pub fn show(
    ui: &mut egui::Ui,
    snapshot: Option<&Snapshot>,
    assets: Option<&AssetsView>,
    selected_equipment_id: &str,
    controls_enabled: bool,
) -> Option<Action> {
    let catalog = pioneer_room_catalog();
    let Some(device) = catalog
        .equipment
        .iter()
        .find(|device| device.id == selected_equipment_id)
    else {
        ui.label("Select a device from the room equipment catalog.");
        return None;
    };

    let room = assets.and_then(|assets| assets.pioneer_room.as_ref());
    let mut action = None;
    ui.heading(device.label);
    ui.monospace(device.id);
    if let Some(position) = device.position {
        ui.small(format!(
            "Catalogued mount · deck 1 · x {:.2}, y {:.2}, z {:.2} m",
            position[0], position[1], position[2]
        ));
    } else {
        ui.small("Logical catalog endpoint · no physical mount is defined.");
    }
    if let Some(watts) = device.active_load_w {
        ui.small(format!("Conditional load rating: {watts} W"));
    } else {
        ui.small("Load is not independently metered.");
    }

    ui.separator();
    ui.strong("Live state");
    match room {
        Some(state) => {
            ui.small(format!(
                "Vessel {} · updated {} ms",
                state.vessel_id, state.updated_at_ms
            ));
            match device.kind {
                EquipmentKind::Supply => {
                    if let Some(snapshot) = snapshot {
                        row(
                            ui,
                            "Snapshot server time",
                            format!("{} ms", snapshot.server_time_ms),
                        );
                    }
                    ui.label("The room catalog has no separate live supply reading.");
                }
                EquipmentKind::Breaker => {
                    row(ui, "Commissioned", yes_no(state.commissioned));
                    row(
                        ui,
                        "Commanded / reported",
                        if state.breaker_closed {
                            "Closed"
                        } else {
                            "Open"
                        },
                    );
                    let label = if !state.commissioned {
                        "Commission room power"
                    } else if state.breaker_closed {
                        "Open breaker"
                    } else {
                        "Close breaker"
                    };
                    if ui
                        .add_enabled(controls_enabled, egui::Button::new(label))
                        .clicked()
                    {
                        action = Some(Action::Send(Request::SetPioneerRoom {
                            action: PioneerRoomCommand::Breaker {
                                closed: !state.commissioned || !state.breaker_closed,
                            },
                        }));
                    }
                    if ui.button("Add command to event draft").clicked() {
                        action = Some(Action::AddToGraph(PioneerRoomCommand::Breaker {
                            closed: !state.commissioned || !state.breaker_closed,
                        }));
                    }
                }
                EquipmentKind::Controller => {
                    row(ui, "Commissioned", yes_no(state.commissioned));
                    row(
                        ui,
                        "Interlock",
                        state.interlock.as_deref().unwrap_or("None reported"),
                    );
                    row(ui, "Updated", format!("{} ms", state.updated_at_ms));
                }
                EquipmentKind::Light => {
                    row(
                        ui,
                        "Command",
                        if state.light_commanded { "On" } else { "Off" },
                    );
                    row(ui, "Output", if state.light_on { "On" } else { "Off" });
                    if controls_enabled
                        && state.commissioned
                        && ui
                            .button(if state.light_commanded {
                                "Switch light off"
                            } else {
                                "Switch light on"
                            })
                            .clicked()
                    {
                        action = Some(Action::Send(Request::SetPioneerRoom {
                            action: PioneerRoomCommand::Light {
                                on: !state.light_commanded,
                            },
                        }));
                    }
                    if ui.button("Add command to event draft").clicked() {
                        action = Some(Action::AddToGraph(PioneerRoomCommand::Light {
                            on: !state.light_commanded,
                        }));
                    }
                }
                EquipmentKind::EmergencyLight => {
                    row(
                        ui,
                        "Output",
                        if state.emergency_light_on {
                            "Active"
                        } else {
                            "Off"
                        },
                    );
                    row(
                        ui,
                        "Reserve",
                        format!("{:.1} s", state.emergency_remaining_ms as f64 / 1000.),
                    );
                    ui.small("Automatic reserve system; no direct command is supported.");
                }
                EquipmentKind::Vent => {
                    row(ui, "Command", format!("{:?}", state.vent_commanded));
                    row(
                        ui,
                        "Output",
                        if state.vent_running {
                            "Running"
                        } else {
                            "Stopped"
                        },
                    );
                    if controls_enabled && state.commissioned {
                        ui.horizontal_wrapped(|ui| {
                            for (mode, label) in [
                                (VentMode::Hold, "Hold"),
                                (VentMode::Equalize, "Equalize"),
                                (VentMode::Exhaust, "Exhaust"),
                            ] {
                                if ui
                                    .add(
                                        egui::Button::new(label)
                                            .selected(state.vent_commanded == mode),
                                    )
                                    .clicked()
                                {
                                    action = Some(Action::Send(Request::SetPioneerRoom {
                                        action: PioneerRoomCommand::Vent { mode },
                                    }));
                                }
                            }
                        });
                    }
                }
                EquipmentKind::PressureSensor => {
                    row(
                        ui,
                        "Pressure",
                        format!("{:.2} kPa", state.pressure_pa as f64 / 1000.),
                    );
                    row(
                        ui,
                        "Measurement updated",
                        format!("{} ms", state.updated_at_ms),
                    );
                    ui.small("Scalar room pressure telemetry; no sensor calibration controls are implemented.");
                }
                EquipmentKind::Door => {
                    row(
                        ui,
                        "Commanded",
                        if state.door_commanded_open {
                            "Open"
                        } else {
                            "Closed"
                        },
                    );
                    row(
                        ui,
                        "Output",
                        if state.door_open { "Open" } else { "Closed" },
                    );
                    row(
                        ui,
                        "Actuation",
                        if state.door_manual {
                            "Manual"
                        } else {
                            "Powered"
                        },
                    );
                    if let Some(interlock) = &state.interlock {
                        ui.colored_label(egui::Color32::from_rgb(245, 174, 70), interlock);
                    }
                    if controls_enabled && state.commissioned {
                        ui.horizontal_wrapped(|ui| {
                            let open = !state.door_open;
                            for (manual, label) in [(false, "Powered"), (true, "Manual")] {
                                if ui
                                    .button(format!(
                                        "{label} {}",
                                        if open { "open" } else { "close" }
                                    ))
                                    .clicked()
                                {
                                    action = Some(Action::Send(Request::SetPioneerRoom {
                                        action: PioneerRoomCommand::Door { open, manual },
                                    }));
                                }
                            }
                        });
                    }
                    if ui.button("Add command to event draft").clicked() {
                        action = Some(Action::AddToGraph(PioneerRoomCommand::Door {
                            open: !state.door_open,
                            manual: state.door_manual,
                        }));
                    }
                }
            }
            if !controls_enabled
                && matches!(
                    device.kind,
                    EquipmentKind::Breaker
                        | EquipmentKind::Light
                        | EquipmentKind::Vent
                        | EquipmentKind::Door
                )
            {
                ui.small("Controls unavailable in the current session; showing last server-confirmed state.");
            }
        }
        None => {
            ui.label("No authoritative room telemetry is available for this pilot.");
            ui.small("Device controls remain unavailable until server state is present.");
        }
    }

    ui.separator();
    ui.strong("Connections");
    let incoming = catalog
        .connections
        .iter()
        .filter(|link| link.to_equipment == device.id);
    let outgoing = catalog
        .connections
        .iter()
        .filter(|link| link.from_equipment == device.id);
    let mut count = 0;
    for link in incoming {
        count += 1;
        ui.small(format!(
            "{:?} · {} / {} → {} / {}",
            link.kind, link.from_equipment, link.from_port, link.to_equipment, link.to_port
        ));
    }
    for link in outgoing {
        count += 1;
        ui.small(format!(
            "{:?} · {} / {} → {} / {}",
            link.kind, link.from_equipment, link.from_port, link.to_equipment, link.to_port
        ));
    }
    if count == 0 {
        ui.small("No connections are declared for this endpoint.");
    }
    if !matches!(
        device.kind,
        EquipmentKind::Breaker | EquipmentKind::Light | EquipmentKind::Vent | EquipmentKind::Door
    ) {
        ui.small("Read only · no supported direct equipment command is implemented.");
    }

    if room.is_some()
        && matches!(
            device.kind,
            EquipmentKind::Breaker | EquipmentKind::Light | EquipmentKind::Door
        )
    {
        ui.small("Draft only; Apply and Trigger remain explicit.");
    }
    action
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "Yes"
    } else {
        "No"
    }
}

fn row(ui: &mut egui::Ui, label: &str, value: impl std::fmt::Display) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{label}: ")).color(egui::Color32::from_gray(160)));
        ui.label(value.to_string());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::PioneerRoomState;

    fn assets(state: PioneerRoomState) -> AssetsView {
        AssetsView {
            pioneer_room: Some(state),
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 1,
            ships: Vec::new(),
            storage: Vec::new(),
            corporation: None,
            invitations: Vec::new(),
        }
    }

    fn click(
        equipment_id: &str,
        label: &str,
        enabled: bool,
        assets: Option<&AssetsView>,
    ) -> Option<Action> {
        let ctx = egui::Context::default();
        let mut result = None;
        let mut run = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 700.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        result = show(ui, None, assets, equipment_id, enabled);
                    });
                },
            )
        };
        run(vec![]);
        let output = run(vec![]);
        let point = output
            .shapes
            .into_iter()
            .find_map(|shape| match shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.rect.center().to_vec2())
                }
                _ => None,
            })?;
        for pressed in [true, false] {
            run(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]);
        }
        result
    }

    fn room() -> PioneerRoomState {
        let mut state = PioneerRoomState::uncommissioned("test-pioneer".into());
        state.commissioned = true;
        state
    }

    #[test]
    fn equipment_buttons_emit_the_existing_typed_room_requests() {
        let uncommissioned = PioneerRoomState::uncommissioned("test-pioneer".into());
        let uncommissioned_assets = assets(uncommissioned.clone());
        assert!(matches!(
            click(
                "pioneer.deck1.airlock.breaker",
                "Commission room power",
                true,
                Some(&uncommissioned_assets)
            ),
            Some(Action::Send(Request::SetPioneerRoom {
                action: PioneerRoomCommand::Breaker { closed: true }
            }))
        ));
        assert!(
            !uncommissioned.commissioned,
            "the panel does not predict success"
        );

        let light_assets = assets(room());
        assert!(matches!(
            click(
                "pioneer.deck1.airlock.light",
                "Switch light off",
                true,
                Some(&light_assets)
            ),
            Some(Action::Send(Request::SetPioneerRoom {
                action: PioneerRoomCommand::Light { on: false }
            }))
        ));
        let vent_assets = assets(room());
        assert!(matches!(
            click(
                "pioneer.deck1.airlock.vent",
                "Exhaust",
                true,
                Some(&vent_assets)
            ),
            Some(Action::Send(Request::SetPioneerRoom {
                action: PioneerRoomCommand::Vent {
                    mode: VentMode::Exhaust
                }
            }))
        ));
        let door_assets = assets(room());
        assert!(matches!(
            click(
                "pioneer.deck1.airlock.door",
                "Manual close",
                true,
                Some(&door_assets)
            ),
            Some(Action::Send(Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: false,
                    manual: true
                }
            }))
        ));
    }

    #[test]
    fn disabled_controls_and_missing_room_telemetry_emit_no_request() {
        let mut closed = room();
        closed.breaker_closed = true;
        let current = assets(closed);
        assert!(click(
            "pioneer.deck1.airlock.breaker",
            "Open breaker",
            false,
            Some(&current)
        )
        .is_none());
        assert!(click(
            "pioneer.deck1.airlock.breaker",
            "Commission room power",
            true,
            None
        )
        .is_none());
    }

    #[test]
    fn paused_session_can_add_supported_commands_to_the_local_draft() {
        let uncommissioned = PioneerRoomState::uncommissioned("test-pioneer".into());
        let breaker_assets = assets(uncommissioned.clone());
        assert!(matches!(
            click(
                "pioneer.deck1.airlock.breaker",
                "Add command to event draft",
                false,
                Some(&breaker_assets)
            ),
            Some(Action::AddToGraph(PioneerRoomCommand::Breaker {
                closed: true
            }))
        ));
        assert!(
            !uncommissioned.commissioned,
            "drafting does not mutate room state"
        );

        let light_assets = assets(room());
        assert!(matches!(
            click(
                "pioneer.deck1.airlock.light",
                "Add command to event draft",
                false,
                Some(&light_assets)
            ),
            Some(Action::AddToGraph(PioneerRoomCommand::Light { on: false }))
        ));

        let door_assets = assets(room());
        assert!(matches!(
            click(
                "pioneer.deck1.airlock.door",
                "Add command to event draft",
                false,
                Some(&door_assets)
            ),
            Some(Action::AddToGraph(PioneerRoomCommand::Door {
                open: false,
                manual: false
            }))
        ));
    }
}
