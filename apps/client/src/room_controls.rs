//! The panel displays server-confirmed commands and outputs, never predicted success.
use eframe::egui;

use arkline_protocol::{PioneerRoomCommand as Command, PioneerRoomState, VentMode};

pub(crate) fn panel_access(pilot: &arkline_protocol::PilotView) -> bool {
    let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
    pilot.ship_class == arkline_protocol::ShipClass::Pioneer
        && pilot.location == arkline_protocol::Location::Docked
        && pilot.station_avatar.as_ref().is_some_and(|a| {
            let position = if let Some(state) = &a.docked_traversal {
                if state.version != arkline_protocol::DOCKED_TRAVERSAL_VERSION
                    || state.support != arkline_protocol::DockedSupport::Deck1
                {
                    return false;
                }
                let Ok(pose) = arkline_protocol::docked_binding_pose(&state.binding) else {
                    return false;
                };
                let body = arkline_protocol::station_to_body(pose, state.feet_m);
                [
                    body[0] + 12.,
                    body[1] - arkline_protocol::deck_floor_y(1).unwrap_or(f64::NAN),
                    body[2] + 7.,
                ]
            } else {
                if !a.onboard_interior || a.interior_deck != d.deck {
                    return false;
                }
                a.position
            };
            position.iter().all(|coordinate| coordinate.is_finite())
                && position
                    .iter()
                    .zip(d.breaker_position)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    <= 9.
        })
}

pub(crate) fn show(
    ui: &mut egui::Ui,
    state: Option<&PioneerRoomState>,
    enabled: bool,
) -> Option<Command> {
    let mut action = None;
    ui.separator();
    ui.strong("AIRLOCK / EVA · ENGINEERING");
    let Some(s) = state else {
        ui.small("Room systems unavailable on this server.");
        return None;
    };
    ui.small(if enabled {
        "Server-confirmed room state"
    } else {
        "Read-only / last confirmed state"
    });
    ui.label(format!(
        "Pressure {:.1} kPa · emergency reserve {:.0} s",
        s.pressure_pa as f64 / 1000.,
        s.emergency_remaining_ms as f64 / 1000.
    ));
    ui.label(format!(
        "Main light · requested {} · actual {}",
        if s.light_commanded { "ON" } else { "OFF" },
        if s.light_on { "ON" } else { "OFF" }
    ));
    ui.label(format!(
        "Ventilation · requested {:?} · actual {}",
        s.vent_commanded,
        if s.vent_running { "RUNNING" } else { "STOPPED" }
    ));
    ui.label(format!(
        "Door · requested {} ({}) · actual {}",
        if s.door_commanded_open {
            "OPEN"
        } else {
            "CLOSED"
        },
        if s.door_manual { "manual" } else { "powered" },
        if s.door_open { "OPEN" } else { "CLOSED" }
    ));
    ui.small(format!(
        "Breaker command: {}",
        if s.breaker_closed { "closed" } else { "open" }
    ));
    ui.small(format!("Gameplay loads: controller {} W · light {} W · vent {} W. Outputs above show what is running.", arkline_protocol::ROOM_CONTROLLER_W, arkline_protocol::ROOM_LIGHT_W, arkline_protocol::ROOM_VENT_W));
    if s.emergency_light_on {
        ui.colored_label(
            egui::Color32::from_rgb(245, 174, 70),
            "EMERGENCY LIGHT ACTIVE",
        );
    }
    if let Some(reason) = &s.interlock {
        ui.label(reason);
    }
    ui.add_enabled_ui(enabled, |ui| {
        ui.horizontal_wrapped(|ui| {
            let label = if !s.commissioned {
                "Commission room power"
            } else if s.breaker_closed {
                "Open breaker"
            } else {
                "Close breaker"
            };
            if ui.button(label).clicked() {
                action = Some(Command::Breaker {
                    closed: !s.commissioned || !s.breaker_closed,
                });
            }
            if ui
                .add_enabled(
                    s.commissioned,
                    egui::Button::new(if s.light_commanded {
                        "Switch light off"
                    } else {
                        "Switch light on"
                    }),
                )
                .clicked()
            {
                action = Some(Command::Light {
                    on: !s.light_commanded,
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Vent command:");
            for (mode, label) in [
                (VentMode::Hold, "Hold"),
                (VentMode::Equalize, "Equalize"),
                (VentMode::Exhaust, "Exhaust"),
            ] {
                if ui
                    .add_enabled(
                        s.commissioned,
                        egui::Button::new(label).selected(s.vent_commanded == mode),
                    )
                    .clicked()
                {
                    action = Some(Command::Vent { mode });
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            for (manual, label) in [(false, "Powered door"), (true, "Manual door")] {
                if ui
                    .add_enabled(
                        s.commissioned,
                        egui::Button::new(format!(
                            "{} · {}",
                            label,
                            if s.door_commanded_open && !s.door_open {
                                "cancel open request"
                            } else if !s.door_commanded_open && s.door_open {
                                "cancel close request"
                            } else if s.door_commanded_open {
                                "close"
                            } else {
                                "open"
                            }
                        )),
                    )
                    .clicked()
                {
                    action = Some(Command::Door {
                        open: !s.door_commanded_open,
                        manual,
                    });
                }
            }
        });
        if s.door_commanded_open != s.door_open {
            if ui
                .add_enabled(
                    s.commissioned && !s.door_manual,
                    egui::Button::new("Apply pending request manually"),
                )
                .on_hover_text("Keep the requested door target and use manual actuation. The server still enforces pressure and doorway occupancy interlocks.")
                .clicked()
            {
                action = Some(Command::Door {
                    open: s.door_commanded_open,
                    manual: true,
                });
            }
        }
        ui.small("Manual actuation does not bypass pressure or occupied-doorway interlocks.");
    });
    ui.small("Single-room pressure and electrical loads are simulated. Whole-ship atmosphere, plumbing and life support remain proposed. Door safety interlocks are enforced by the server.");
    ui.collapsing("Equipment and connections", |ui| {
        let catalog = arkline_protocol::pioneer_room_catalog();
        ui.small(format!("Vessel: {}", s.vessel_id));
        ui.small(
            "Connection records describe this fixed controller; they are not editable wiring.",
        );
        for device in catalog.equipment {
            ui.collapsing(device.label, |ui| {
                ui.monospace(device.id);
                if let Some([x, y, z]) = device.position {
                    ui.small(format!("Deck 1 · {x:.2}, {y:.2}, {z:.2} m"));
                } else {
                    ui.small("Logical endpoint · no separate physical fixture");
                }
                match device.active_load_w {
                    Some(w) => {
                        ui.small(format!("Conditional load: {w} W"));
                    }
                    None => {
                        ui.small("Load not independently metered");
                    }
                }
                for link in catalog
                    .connections
                    .iter()
                    .filter(|link| link.from_equipment == device.id)
                {
                    ui.small(format!(
                        "{:?}: {} to {} / {}",
                        link.kind, link.from_port, link.to_equipment, link.to_port
                    ));
                }
            });
        }
    });
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    fn click(label: &str, enabled: bool, state: &PioneerRoomState) -> Option<Command> {
        let ctx = egui::Context::default();
        let mut result = None;
        let mut run = |events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 650.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let Some(c) = show(ui, Some(state), enabled) {
                            result = Some(c);
                        }
                    });
                },
            )
        };
        run(vec![]);
        let output = run(vec![]);
        let point = output.shapes.into_iter().find_map(|s| match s.shape {
            egui::Shape::Text(t) if t.galley.job.text == label => {
                Some(t.pos + t.galley.rect.center().to_vec2())
            }
            _ => None,
        });
        let Some(point) = point else {
            return None;
        };
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
    #[test]
    fn commissioning_click_returns_command_only_when_live() {
        let s = PioneerRoomState::uncommissioned("owned".into());
        assert!(matches!(
            click("Commission room power", true, &s),
            Some(Command::Breaker { closed: true })
        ));
        assert!(click("Commission room power", false, &s).is_none());
        assert!(
            !s.commissioned,
            "click must not predict authoritative success"
        );
    }
    #[test]
    fn room_controls_pending_open_and_close_can_be_cancelled_without_predicting_success() {
        let mut s = PioneerRoomState::uncommissioned("owned".into());
        s.commissioned = true;
        s.door_open = false;
        s.door_commanded_open = true;
        s.pressure_pa = 0;
        s.interlock = Some("Pressure difference exceeds 5 kPa".into());
        for manual in [false, true] {
            let label = if manual {
                "Manual door · cancel open request"
            } else {
                "Powered door · cancel open request"
            };
            assert!(
                matches!(click(label, true, &s), Some(Command::Door { open: false, manual: mode }) if mode == manual)
            );
            assert!(click(label, false, &s).is_none());
        }
        assert!(
            s.door_commanded_open && !s.door_open,
            "UI must not predict cancellation or physical opening"
        );
        s.door_open = true;
        s.door_commanded_open = false;
        s.interlock = Some("Doorway occupied".into());
        assert!(matches!(
            click("Powered door · cancel close request", true, &s),
            Some(Command::Door {
                open: true,
                manual: false
            })
        ));
        assert!(
            !s.door_commanded_open && s.door_open,
            "UI must not predict cancellation or physical closing"
        );
    }
    #[test]
    fn room_controls_manual_pending_preserves_target_and_interlocks() {
        for open in [false, true] {
            let mut s = PioneerRoomState::uncommissioned("owned".into());
            s.commissioned = true;
            s.door_manual = false;
            s.door_commanded_open = open;
            s.door_open = !open;
            s.interlock = Some(
                if open {
                    "Pressure difference exceeds 5 kPa"
                } else {
                    "Doorway occupied"
                }
                .into(),
            );
            let before = serde_json::to_value(&s).unwrap();
            assert!(
                matches!(click("Apply pending request manually", true, &s), Some(Command::Door { open: target, manual: true }) if target == open)
            );
            assert!(click("Apply pending request manually", false, &s).is_none());
            assert_eq!(
                serde_json::to_value(&s).unwrap(),
                before,
                "UI must preserve confirmed outputs and interlock"
            );
            s.door_manual = true;
            assert!(click("Apply pending request manually", true, &s).is_none());
            s.door_manual = false;
            s.commissioned = false;
            assert!(click("Apply pending request manually", true, &s).is_none());
            s.commissioned = true;
            s.door_open = open;
            assert!(click("Apply pending request manually", true, &s).is_none());
        }
    }
    #[test]
    fn room_controls_show_requested_and_actual_outputs_when_interlocked() {
        let mut s = PioneerRoomState::uncommissioned("owned".into());
        s.commissioned = true;
        s.door_open = false;
        s.door_commanded_open = true;
        s.light_commanded = true;
        s.light_on = false;
        s.vent_commanded = VentMode::Equalize;
        s.vent_running = false;
        s.interlock = Some("Door motor has no power; manual actuation available".into());
        let ctx = egui::Context::default();
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(show(ui, Some(&s), false).is_none());
            });
        });
        let labels: Vec<_> = output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect();
        assert!(labels
            .iter()
            .any(|text| text == "Main light · requested ON · actual OFF"));
        assert!(labels
            .iter()
            .any(|text| text == "Ventilation · requested Equalize · actual STOPPED"));
        assert!(labels
            .iter()
            .any(|text| text == "Door · requested OPEN (powered) · actual CLOSED"));
        assert!(labels
            .iter()
            .any(|text| text == s.interlock.as_ref().unwrap()));
    }
    #[test]
    fn manual_door_is_explicit_and_vent_choice_is_real() {
        let mut s = PioneerRoomState::uncommissioned("owned".into());
        s.commissioned = true;
        assert!(matches!(
            click("Manual door · close", true, &s),
            Some(Command::Door {
                open: false,
                manual: true
            })
        ));
        assert!(matches!(
            click("Exhaust", true, &s),
            Some(Command::Vent {
                mode: VentMode::Exhaust
            })
        ));
    }
}
