//! A displayed, pinned shortcut over the existing authoritative interaction actions.
use super::*;
use crate::nearby_action_model::NearbyAction;

#[derive(Default)]
pub(super) struct NearbyGesture {
    displayed: Option<(u64, NearbyAction)>,
    armed: Option<(u64, NearbyAction)>,
    awaiting_release: bool,
}
fn prompt_fits_clip(prompt: egui::Rect, clip: egui::Rect) -> bool {
    prompt.is_finite() && prompt.is_positive() && clip.contains_rect(prompt)
}
fn same_target(a: &NearbyAction, b: &NearbyAction) -> bool {
    a.target_id == b.target_id && a.request == b.request
}
impl NearbyGesture {
    fn advance(
        &mut self,
        action: Option<&NearbyAction>,
        epoch: u64,
        allowed: bool,
        pressed: bool,
        released: bool,
        held: bool,
    ) -> Option<DockedTraversalRequest> {
        let usable = allowed && action.is_some_and(|a| a.blocked_reason.is_none());
        let matches = |pin: &(u64, NearbyAction)| {
            pin.0 == epoch && action.is_some_and(|a| same_target(&pin.1, a))
        };
        let previously_displayed = self.displayed.as_ref().is_some_and(matches);
        if !usable || self.armed.as_ref().is_some_and(|pin| !matches(pin)) {
            self.armed = None;
            self.awaiting_release = held;
        }
        let mut command = None;
        if usable {
            if pressed && !self.awaiting_release && previously_displayed {
                self.armed = action.cloned().map(|a| (epoch, a));
            }
            if released {
                if let Some(pin) = self.armed.take().filter(matches) {
                    command = Some(pin.1.request);
                }
            }
        }
        if released || !held {
            self.awaiting_release = false;
        }
        self.displayed = action.cloned().map(|a| (epoch, a));
        command
    }
    pub(super) fn cancel(&mut self) {
        self.displayed = None;
        self.armed = None;
        self.awaiting_release = true;
    }
}

impl Cockpit {
    fn nearby_shortcut_input_reason(&self, ctx: &egui::Context) -> Option<&'static str> {
        if !self.embedded_input
            || !ctx.input(|i| i.focused && !i.viewport().minimized.unwrap_or(false))
        {
            Some("Return focus to the body viewport")
        } else if self.boot.visible {
            Some("Wait for the world viewport handoff")
        } else if self.controls_reference_open || self.services_open
            || self.service_tab == ServiceTab::Layouts
            || self.engineering_panel.is_some()
        {
            Some("Close management panels to use nearby shortcuts")
        } else if self.ship_observation || self.manual_remote.is_some() {
            Some("Return to the player body view")
        } else if self.workspace_input_waiting_release {
            Some("Release held controls before interacting")
        } else if ctx.wants_keyboard_input()
            || ctx.memory(|m| m.focused().is_some())
            || self.cockpit_computer.wants_keyboard_input()
        {
            Some("Text or computer input owns the keyboard")
        } else if ctx.input(|i| {
            i.pointer.any_down() || !i.modifiers.is_none() || i.key_down(egui::Key::Escape)
        }) {
            Some("Release mouse and modifier controls before interacting")
        } else {
            let surface = if self.in_station_hangar() {
                self.hangar_view.input_surface()
            } else {
                self.flight_view.input_surface()
            };
            if surface.is_some_and(|(rect, layer)| {
                ctx.pointer_latest_pos().is_some_and(|point| {
                    rect.contains(point) && ctx.layer_id_at(point) == Some(layer)
                })
            }) {
                None
            } else {
                Some("Point inside the body viewport to interact")
            }
        }
    }

    pub(super) fn nearby_interaction_prompt(&mut self, ui: &mut egui::Ui) {
        let action = self.nearby_ship_action();
        let input_reason = self.nearby_shortcut_input_reason(ui.ctx());
        let (pressed, released, held) = ui.input(|i| {
            let mut press = false;
            let mut release = false;
            for event in &i.events {
                if let egui::Event::Key {
                    key: crate::control_bindings::INTERACT,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } = event
                {
                    if *pressed && !repeat && modifiers.is_none() {
                        press = true;
                    }
                    if !pressed {
                        release = true;
                    }
                }
            }
            (press, release, i.key_down(crate::control_bindings::INTERACT))
        });
        let prompt = if let Some(action) = &action {
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("F · {}", action.label));
                if action.distance_m > 0.0 {
                    ui.small(format!("{:.1} m", action.distance_m));
                }
                if let Some(reason) = action.blocked_reason.or(input_reason) {
                    ui.colored_label(MUTED, reason);
                } else {
                    ui.small("Press and release · nearest reachable control");
                }
            })
            .response
        } else {
            ui.small("Nearby shortcut: no unambiguous reachable control · use the nearby buttons")
        };
        // Register a displayed target only after painting its complete prompt.
        // Expanded content, small viewports and overlapping windows cannot arm
        // an action whose text the player cannot actually see.
        let visible = prompt_fits_clip(prompt.rect, ui.clip_rect())
            && [
                prompt.rect.center(),
                prompt.rect.left_top(),
                prompt.rect.right_top(),
                prompt.rect.left_bottom(),
                prompt.rect.right_bottom(),
            ]
            .iter()
            .all(|point| ui.ctx().layer_id_at(*point) == Some(ui.layer_id()));
        let request = self.nearby_shortcut.advance(
            action.as_ref().filter(|_| visible),
            self.connection_generation,
            input_reason.is_none() && visible,
            pressed,
            released,
            held,
        );
        if let Some(request) = request {
            // Stop walking/thrust through the same bound-command path as the buttons.
            self.send(Request::DockedTraversal { request });
            self.workspace_input_waiting_release = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn action(open: bool, id: &str) -> NearbyAction {
        NearbyAction {
            request: DockedTraversalRequest::SetHatch {
                binding: docked_traversal_binding(
                    "owned:Pioneer".into(),
                    &default_pioneer(),
                    FleetLayoutRevision::CircularV1,
                    true,
                ),
                hatch: DockedHatch::Inner,
                open,
            },
            label: if open {
                "Open inner hatch"
            } else {
                "Close inner hatch"
            }
            .into(),
            blocked_reason: None,
            distance_m: 1.0,
            target_id: id.into(),
        }
    }
    #[test]
    fn nearby_interaction_prompt_must_fit_completely_before_arming() {
        let clip = egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(400.0, 100.0));
        assert!(prompt_fits_clip(
            egui::Rect::from_min_max(Pos2::new(8.0, 8.0), Pos2::new(390.0, 45.0)),
            clip
        ));
        assert!(!prompt_fits_clip(
            egui::Rect::from_min_max(Pos2::new(8.0, 90.0), Pos2::new(390.0, 125.0)),
            clip
        ));
        let a = action(true, "inner/cabin");
        let mut g = NearbyGesture::default();
        g.advance(Some(&a), 0, true, false, false, false);
        g.advance(Some(&a), 0, true, true, false, true);
        assert!(g.advance(None, 0, false, false, true, false).is_none());
    }

    #[test]
    fn nearby_interaction_gesture_pins_intent_side_and_connection_until_release() {
        let a = action(true, "inner/cabin");
        let mut g = NearbyGesture::default();
        assert!(g.advance(Some(&a), 0, true, false, false, false).is_none());
        assert!(g.advance(Some(&a), 0, true, true, false, true).is_none());
        assert_eq!(
            g.advance(Some(&a), 0, true, false, true, false),
            Some(a.request.clone())
        );
        assert!(g.advance(Some(&a), 0, true, false, true, false).is_none());
        for change in 0..3 {
            g.advance(Some(&a), 0, true, false, false, false);
            g.advance(Some(&a), 0, true, true, false, true);
            let next = if change == 0 {
                action(false, "inner/cabin")
            } else if change == 1 {
                action(true, "inner/vestibule")
            } else {
                a.clone()
            };
            assert!(g
                .advance(
                    Some(&next),
                    u64::from(change == 2),
                    true,
                    false,
                    true,
                    false
                )
                .is_none());
        }
    }
    #[test]
    fn nearby_interaction_gesture_cancels_on_focus_block_or_disappearing_target() {
        let a = action(true, "inner/cabin");
        for change in 0..3 {
            let mut g = NearbyGesture::default();
            g.advance(Some(&a), 0, true, false, false, false);
            g.advance(Some(&a), 0, true, true, false, true);
            let mut blocked = a.clone();
            blocked.blocked_reason = Some("Interlock");
            let next = if change == 0 {
                None
            } else if change == 1 {
                Some(&blocked)
            } else {
                Some(&a)
            };
            assert!(g
                .advance(next, 0, change != 2, false, false, true)
                .is_none());
            assert!(g.advance(Some(&a), 0, true, false, true, false).is_none());
            assert!(g.advance(Some(&a), 0, true, false, false, true).is_none());
        }
    }
    #[test]
    fn nearby_interaction_gesture_requires_visible_prompt_and_fresh_press() {
        let a = action(true, "inner/cabin");
        let mut g = NearbyGesture::default();
        assert!(g.advance(Some(&a), 0, true, true, false, true).is_none());
        assert!(g.advance(Some(&a), 0, true, false, true, false).is_none());
        g.cancel();
        assert!(g.advance(Some(&a), 0, true, true, false, true).is_none());
        assert!(g.advance(Some(&a), 0, true, false, true, false).is_none());
        g.advance(Some(&a), 0, true, true, false, true);
        assert_eq!(
            g.advance(Some(&a), 0, true, false, true, false),
            Some(a.request)
        );
    }
}
