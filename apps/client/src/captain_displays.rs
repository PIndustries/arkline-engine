//! Captain-selected monitor policy. Accepted snapshots alone describe hardware state.
use super::*;
use arkline_protocol::{CockpitScreenMode, CockpitScreenTarget, PioneerCockpitMechanisms};

fn mode_label(mode: CockpitScreenMode) -> &'static str {
    match mode {
        CockpitScreenMode::Auto => "AUTO",
        CockpitScreenMode::Open => "OPEN",
        CockpitScreenMode::Stowed => "STOWED",
    }
}

fn actual_label(state: &PioneerCockpitMechanisms, slot: usize, now: u64) -> String {
    let screen = state.screen(slot).expect("validated hardware slot");
    let fraction = screen.fraction_at(now);
    let motion = if screen.blocked.is_some() {
        "BLOCKED"
    } else if fraction <= 1e-6 {
        if screen.target_deployed {
            "DEPLOYING"
        } else {
            "STOWED"
        }
    } else if fraction >= 1. - 1e-6 {
        if screen.target_deployed {
            "DEPLOYED"
        } else {
            "STOWING"
        }
    } else if screen.target_deployed {
        "DEPLOYING"
    } else {
        "STOWING"
    };
    let override_label = if state.warp_stowed {
        "WARP OVERRIDE / "
    } else if slot < 5 && state.retiring_drone_slots[slot] {
        "RETIRING / "
    } else {
        ""
    };
    format!("{override_label}{motion} · {:.0}% open", fraction * 100.)
}

fn source_label(pilot: &PilotView, state: &PioneerCockpitMechanisms, slot: usize) -> String {
    if slot < 5 {
        state.drone_ids[slot].map_or_else(
            || "NO SOURCE".into(),
            |id| match pilot
                .drones
                .iter()
                .chain(pilot.drone.iter())
                .find(|drone| drone.id == id)
            {
                None => format!("Drone {id} · SOURCE REMOVED"),
                Some(drone) if drone.signal_strength <= 0. => format!("Drone {id} · NO SIGNAL"),
                Some(_) => format!("Drone {id} · source available"),
            },
        )
    } else if slot >= 7 {
        format!(
            "{} hull-side camera · {}",
            if slot == 7 { "PORT" } else { "STARBOARD" },
            if pilot.flight.is_some() {
                "rendered view"
            } else {
                "view unavailable"
            }
        )
    } else if pilot
        .mining_modules
        .iter()
        .any(|module| module.module_id as usize == slot - 5)
    {
        "Turret camera source available".into()
    } else {
        "TURRET SOURCE UNAVAILABLE".into()
    }
}

fn choices(
    ui: &mut egui::Ui,
    state: &PioneerCockpitMechanisms,
    target: CockpitScreenTarget,
    selected: Option<CockpitScreenMode>,
    enabled: bool,
    test_key: &str,
) -> Option<Request> {
    let mut request = None;
    for mode in [
        CockpitScreenMode::Auto,
        CockpitScreenMode::Open,
        CockpitScreenMode::Stowed,
    ] {
        let button = ui.add_enabled(
            enabled,
            egui::Button::new(mode_label(mode)).selected(selected == Some(mode)),
        );
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            let id = egui::Id::new("captain_display_buttons");
            let mut buttons = data
                .get_temp::<Vec<(String, egui::Rect)>>(id)
                .unwrap_or_default();
            buttons.push((format!("{test_key}/{}", mode_label(mode)), button.rect));
            data.insert_temp(id, buttons);
        });
        #[cfg(not(test))]
        let _ = test_key;
        if button.clicked() {
            request = Some(Request::SetCockpitScreenMode {
                vessel_id: state.vessel_id.clone(),
                target: target.clone(),
                mode,
            });
        }
    }
    request
}

impl Cockpit {
    fn captain_display_reason(&self, pilot: &PilotView) -> Option<&'static str> {
        if !self.connected {
            return Some("DISCONNECTED · last received state only");
        }
        if self.embedded_paused {
            return Some("PAUSED · resume to change monitor policy");
        }
        if self.request_pending {
            return Some("PENDING · waiting for authority confirmation");
        }
        if !self.live_command_snapshot_fresh() {
            return Some("STALE · wait for a fresh authority snapshot");
        }
        if !self.embedded_input || self.ship_observation {
            return Some("READ ONLY · this view does not own input");
        }
        if self.manual_remote.is_some()
            || self.controlled_remote().is_some()
            || pilot.name != self.pilot
        {
            return Some("LOCAL OWNER ONLY · remote control cannot change monitor policy");
        }
        if pilot.ship_class != ShipClass::Pioneer {
            return Some("Pioneer monitor hardware required");
        }
        let Some(state) = &pilot.cockpit_mechanisms else {
            return Some("Monitor authority unavailable");
        };
        if pilot.station_avatar.is_some()
            && self
                .verified_docked_binding()
                .is_none_or(|binding| binding.host_vessel_id != state.vessel_id)
        {
            return Some("OWN SHIP ONLY · return to your registered Pioneer interior");
        }
        None
    }

    pub(super) fn captain_displays_panel(&mut self, ui: &mut egui::Ui) {
        self.captain_displays_content(ui, false);
    }

    pub(super) fn captain_displays_compact(&mut self, ui: &mut egui::Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6., 3.);
            ui.spacing_mut().button_padding = egui::vec2(8., 3.);
            ui.spacing_mut().interact_size.y = 28.;
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(20.));
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(18.));
            self.captain_displays_content(ui, true);
        });
    }

    fn captain_displays_content(&mut self, ui: &mut egui::Ui, compact: bool) {
        if !compact {
            ui.heading("Captain · Displays");
            ui.small("Monitor motion only. Drones, turrets and mining routines keep working.");
        }
        let Some(pilot) = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .pilots
                    .iter()
                    .find(|pilot| pilot.name == self.pilot)
            })
            .cloned()
        else {
            ui.colored_label(AMBER, "NO CONFIRMED OWNER STATE");
            return;
        };
        let reason = self.captain_display_reason(&pilot);
        if !compact {
            ui.colored_label(
                if reason.is_some() {
                    AMBER
                } else {
                    self.accent()
                },
                reason.unwrap_or("LIVE · confirmed monitor policy"),
            );
        }
        let Some(state) = &pilot.cockpit_mechanisms else {
            return;
        };
        if !compact {
            ui.small(format!(
                "Vessel {} · fixed navigation, reactor and cassette readouts stay available",
                state.vessel_id
            ));
            if state.warp_stowed {
                ui.colored_label(
                    AMBER,
                    "WARP OVERRIDE · all nine stow; requested policies are retained.",
                );
            }
        }
        // Readouts describe the accepted snapshot, not an independent UI-clock
        // actuator. The physical renderer retains its shared sampled trajectory.
        let now = self.snapshot.as_ref().unwrap().server_time_ms;
        if !compact {
            ui.small("Hardware readout: last confirmed authority pose");
        }
        let enabled = reason.is_none();
        let mut request = None;
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("captain_display_buttons"),
                Vec::<(String, egui::Rect)>::new(),
            )
        });
        // Context and assignment identity are part of the actual widget ID: an
        // old held press cannot release onto a reassigned drone or new session.
        let slot_label = |slot: usize| {
            if slot < 5 {
                format!("D{}", slot + 1)
            } else if slot < 7 {
                format!("T{}", slot - 4)
            } else {
                if slot == 7 { "PORT" } else { "STBD" }.into()
            }
        };
        let mut selected_slot = self.captain_screen_slot.min(8);
        ui.push_id(("captain-displays", &self.pilot, self.connection_generation, &state.vessel_id, enabled, compact), |ui| {
            ui.push_id(("all", state.assignment_generations), |ui| {
                ui.horizontal(|ui| {
                    ui.strong(if compact { "ALL" } else { "All nine" }).on_hover_text("Monitor motion only; equipment keeps working. All readouts are the last confirmed authority pose.");
                    let selected = (0..9).all(|slot| state.mode(slot) == state.mode(0)).then(|| state.mode(0).unwrap());
                    request = choices(ui, state, CockpitScreenTarget::All { expected_assignment_generations: state.assignment_generations }, selected, enabled, "all");
                    if compact {
                        let status = reason.unwrap_or("LIVE · confirmed requested modes; last confirmed hardware pose");
                        ui.colored_label(if enabled { self.accent() } else { AMBER }, status.split(" · ").next().unwrap()).on_hover_text(status);
                    }
                });
            });
            ui.separator();
            if compact {
                ui.horizontal_wrapped(|ui| {
                    for slot in 0..9 {
                        let generation = (slot < 5).then(|| state.assignment_generations[slot]);
                        ui.push_id(("select", slot, generation), |ui| {
                            if ui.selectable_label(selected_slot == slot, slot_label(slot)).clicked() {
                                selected_slot = slot;
                            }
                        });
                    }
                });
                let slot = selected_slot;
                let generation = (slot < 5).then(|| state.assignment_generations[slot]);
                ui.label(source_label(&pilot, state, slot));
                ui.push_id(("selected", slot, generation), |ui| {
                    ui.horizontal(|ui| {
                        ui.strong(slot_label(slot));
                        if let Some(next) = choices(ui, state, CockpitScreenTarget::Slot { slot:slot as u8, expected_assignment_generation:generation }, state.mode(slot), enabled, &slot.to_string()) { request = Some(next); }
                    });
                });
                ui.label(actual_label(state, slot, now)).on_hover_text(state.screen(slot).unwrap().blocked.as_deref().unwrap_or("Last confirmed hardware pose. Selected action = confirmed requested mode. Warp and retirement override deployment."));
            } else {
                for slot in 0..9 {
                    let generation = (slot < 5).then(|| state.assignment_generations[slot]);
                    ui.push_id((slot, generation), |ui| {
                        ui.horizontal(|ui| {
                            ui.strong(slot_label(slot));
                            if let Some(next) = choices(ui, state, CockpitScreenTarget::Slot { slot:slot as u8, expected_assignment_generation:generation }, state.mode(slot), enabled, &slot.to_string()) { request = Some(next); }
                            ui.small(format!("Requested {}", mode_label(state.mode(slot).unwrap())));
                        });
                        ui.small(format!("{} · {}", actual_label(state, slot, now), source_label(&pilot, state, slot)));
                        if let Some(blocked) = &state.screen(slot).expect("validated hardware slot").blocked { ui.colored_label(AMBER, blocked); }
                    });
                }
                ui.small("AUTO follows confirmed activity. OPEN requests deployment; STOWED hides the screen. Warp and retirement take priority. Source availability is not a live-image claim.");
            }
        });
        self.captain_screen_slot = selected_slot;
        if let Some(request) = request {
            self.send(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(ctx: &egui::Context) -> Cockpit {
        let mut app = crate::tests::view_test_cockpit(ctx);
        app.embedded_input = true;
        app.frozen_time = Some(1000);
        app.timeline_origin = Some(1000);
        app.observed = Instant::now();
        app.service_tab = ServiceTab::Displays;
        let mut state = PioneerCockpitMechanisms::stowed("owned-vessel".into(), 1000);
        state.assignment_generations = [1, 2, 3, 4, 5];
        state.drone_ids = [Some(42), None, None, None, None];
        app.snapshot.as_mut().unwrap().pilots[0].cockpit_mechanisms = Some(state);
        app
    }
    fn frame(
        ctx: &egui::Context,
        app: &mut Cockpit,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(900., 1000.),
                )),
                time: Some(1.),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.service_content(ui));
            },
        );
        ctx.data(|data| {
            data.get_temp(egui::Id::new("captain_display_buttons"))
                .unwrap()
        })
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
    fn point(buttons: &[(String, egui::Rect)], name: &str) -> Pos2 {
        buttons
            .iter()
            .find(|(key, _)| key == name)
            .unwrap()
            .1
            .center()
    }
    #[test]
    fn actual_pointer_sends_only_pinned_monitor_policy_and_preserves_ai_and_snapshot() {
        for (key, slot, mode) in [
            ("0/OPEN", Some(0), CockpitScreenMode::Open),
            ("6/STOWED", Some(6), CockpitScreenMode::Stowed),
            ("7/OPEN", Some(7), CockpitScreenMode::Open),
            ("8/STOWED", Some(8), CockpitScreenMode::Stowed),
            ("all/AUTO", None, CockpitScreenMode::Auto),
        ] {
            let ctx = egui::Context::default();
            let mut app = app(&ctx);
            let (tx, rx) = mpsc::channel();
            app.commands = Some(tx);
            app.ai_pin = Some((app.pilot.clone(), app.connection_generation));
            app.voice_pin = app.ai_pin.clone();
            let ai = app.ai_pin.clone();
            let voice = app.voice_pin.clone();
            let ai_status = app.ship_ai.status().to_owned();
            let before = serde_json::to_value(&app.snapshot).unwrap();
            let buttons = frame(&ctx, &mut app, vec![]);
            let at = point(&buttons, key);
            frame(&ctx, &mut app, click(at, true));
            frame(&ctx, &mut app, click(at, false));
            let request = rx.try_recv().unwrap();
            match request {
                Request::SetCockpitScreenMode {
                    vessel_id,
                    target,
                    mode: actual,
                } => {
                    assert_eq!(vessel_id, "owned-vessel");
                    assert_eq!(actual, mode);
                    let expected = slot.map_or(
                        CockpitScreenTarget::All {
                            expected_assignment_generations: [1, 2, 3, 4, 5],
                        },
                        |slot| CockpitScreenTarget::Slot {
                            slot,
                            expected_assignment_generation: (slot < 5).then_some(slot as u64 + 1),
                        },
                    );
                    assert_eq!(target, expected);
                }
                other => panic!("Monitor button emitted unrelated order: {other:?}"),
            }
            assert!(rx.try_recv().is_err());
            assert!(app.request_pending);
            assert_eq!(app.ai_pin, ai);
            assert_eq!(app.voice_pin, voice);
            assert_eq!(app.ship_ai.status(), ai_status);
            assert_eq!(serde_json::to_value(&app.snapshot).unwrap(), before);
        }
    }
    #[test]
    fn held_press_cannot_follow_reassignment_session_stale_or_permission_change() {
        for key in ["all/OPEN", "0/OPEN"] {
            for case in 0..10 {
                let ctx = egui::Context::default();
                let mut app = app(&ctx);
                let (tx, rx) = mpsc::channel();
                app.commands = Some(tx);
                let buttons = frame(&ctx, &mut app, vec![]);
                let at = point(&buttons, key);
                frame(&ctx, &mut app, click(at, true));
                match case {
                    0 => {
                        app.snapshot.as_mut().unwrap().pilots[0]
                            .cockpit_mechanisms
                            .as_mut()
                            .unwrap()
                            .assignment_generations[0] += 1
                    }
                    1 => app.connection_generation += 1,
                    2 => app.observed = Instant::now() - Duration::from_secs(2),
                    3 => app.connected = false,
                    4 => app.embedded_paused = true,
                    5 => app.manual_remote = Some("visitor".into()),
                    6 => app.embedded_input = false,
                    7 => app.ship_observation = true,
                    8 => app.request_pending = true,
                    _ => app.pilot = "another session".into(),
                }
                frame(&ctx, &mut app, click(at, false));
                assert!(
                    rx.try_recv().is_err(),
                    "{key} case {case} must cancel old press"
                );
            }
        }
    }
    #[test]
    fn unavailable_and_overridden_rows_do_not_claim_live_image_or_completed_stow() {
        let ctx = egui::Context::default();
        let app = app(&ctx);
        let pilot = app.snapshot.as_ref().unwrap().pilots[0].clone();
        let mut state = pilot.cockpit_mechanisms.as_ref().unwrap().clone();
        assert!(source_label(&pilot, &state, 0).contains("REMOVED"));
        assert_eq!(source_label(&pilot, &state, 1), "NO SOURCE");
        state.modes[0] = CockpitScreenMode::Open;
        state.screens[0].from_fraction = 0.5;
        state.screens[0].duration_ms = 1200;
        state.warp_stowed = true;
        assert!(actual_label(&state, 0, 1000).contains("WARP OVERRIDE / STOWING"));
        state.screens[0].blocked = Some("person in sweep".into());
        assert!(actual_label(&state, 0, 9999).contains("BLOCKED · 50%"));
        state.warp_stowed = false;
        state.retiring_drone_slots[0] = true;
        assert!(actual_label(&state, 0, 1000).starts_with("RETIRING / BLOCKED"));
        assert_eq!(state.modes[0], CockpitScreenMode::Open);
    }
    #[test]
    fn legacy_single_drone_is_a_real_source_not_removed() {
        let ctx = egui::Context::default();
        let app = app(&ctx);
        let mut pilot = app.snapshot.as_ref().unwrap().pilots[0].clone();
        pilot.drone = Some(DroneView {
            orientation_xyzw: None,
            id: 42,
            class: DroneClass::Mining,
            hit_points: 100,
            signal_strength: 1.,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: BELT_POSITION,
            velocity: [0.; 3],
            heading: [1., 0., 0.],
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
        });
        assert_eq!(
            source_label(&pilot, pilot.cockpit_mechanisms.as_ref().unwrap(), 0),
            "Drone 42 · source available"
        );
    }
}
