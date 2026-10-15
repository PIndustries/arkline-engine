//! The panel displays server-confirmed commands and outputs, never predicted success.
use eframe::egui;

use arkline_protocol::{PioneerRoomCommand as Command, PioneerRoomState, VentMode};

pub(crate) fn panel_access(pilot: &arkline_protocol::PilotView) -> bool {
    let d = arkline_protocol::PIONEER_CONNECTED_ROOM;
    pilot.ship_class == arkline_protocol::ShipClass::Pioneer
        && pilot.location == arkline_protocol::Location::Docked
        && pilot.station_avatar.as_ref().is_some_and(|a| {
            a.onboard_interior
                && a.interior_deck == d.deck
                && a.position
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
        "Main light: {} · ventilation: {} · door: {}",
        if s.light_on { "ON" } else { "OFF" },
        if s.vent_running { "RUNNING" } else { "STOPPED" },
        if s.door_open { "OPEN" } else { "CLOSED" }
    ));
    ui.small(format!(
        "Commanded: breaker {} · light {} · vent {:?}",
        if s.breaker_closed { "closed" } else { "open" },
        if s.light_commanded { "on" } else { "off" },
        s.vent_commanded
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
                            if s.door_open { "close" } else { "open" }
                        )),
                    )
                    .clicked()
                {
                    action = Some(Command::Door {
                        open: !s.door_open,
                        manual,
                    });
                }
            }
        });
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
        let point = output
            .shapes
            .into_iter()
            .find_map(|s| match s.shape {
                egui::Shape::Text(t) if t.galley.job.text == label => {
                    Some(t.pos + t.galley.rect.center().to_vec2())
                }
                _ => None,
            })
            .unwrap();
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
