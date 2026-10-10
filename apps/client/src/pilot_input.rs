//! Latest-value direct controls; neutral release is emitted once, never queued repeatedly.
use arkline_protocol::Request;
use eframe::egui;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub translation: [f64; 3],
    pub rotation: [f64; 3],
}
impl Controls {
    fn active(self) -> bool {
        self.translation
            .iter()
            .chain(self.rotation.iter())
            .any(|v| *v != 0.0)
    }
    pub fn request(self) -> Request {
        Request::PilotInput {
            translation: self.translation,
            rotation: self.rotation,
        }
    }
}
/// A single replaceable control sample with immutable routing. Clearing or
/// publishing advances its generation; taking never substitutes a new target.
#[derive(Clone, Default)]
pub struct Mailbox(Arc<Mutex<MailState>>);
#[derive(Default)]
struct MailState {
    generation: u64,
    pending: Option<PendingControl>,
}
struct PendingControl {
    generation: u64,
    control: Controls,
    remote: Option<String>,
}
impl Mailbox {
    #[cfg(test)]
    pub fn publish(&self, control: Controls) {
        self.publish_for(control, None);
    }
    pub fn publish_for(&self, control: Controls, remote: Option<String>) {
        let mut state = self.0.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.pending = Some(PendingControl {
            generation: state.generation,
            control,
            remote,
        });
    }
    pub fn clear(&self) {
        let mut state = self.0.lock().unwrap();
        state.generation = state.generation.wrapping_add(1);
        state.pending = None;
    }
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.0.lock().unwrap().pending.is_none()
    }
    pub fn take(&self) -> Option<Request> {
        let mut state = self.0.lock().unwrap();
        let pending = state.pending.take()?;
        if pending.generation != state.generation {
            return None;
        }
        let request = pending.control.request();
        Some(match pending.remote {
            Some(pilot) => Request::RemoteControl {
                pilot,
                request: Box::new(request),
            },
            None => request,
        })
    }
}
#[derive(Default)]
pub struct PilotInput {
    active: bool,
    aiming: bool,
    captured: bool,
    capture_failed: bool,
}
impl PilotInput {
    pub fn relinquish(&mut self, mailbox: &Mailbox) {
        mailbox.clear();
        self.active = false;
    }
    pub fn release_mouse(&mut self, ctx: &egui::Context) {
        if self.captured {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
        }
        self.captured = false;
        self.aiming = false;
    }
    pub fn update(&mut self, allowed: bool, control: Controls) -> Option<Controls> {
        let control = if allowed {
            control
        } else {
            Controls::default()
        };
        let active = control.active();
        let emit = active || self.active;
        self.active = active;
        emit.then_some(control)
    }
    pub fn sample(
        &mut self,
        ctx: &egui::Context,
        flight: bool,
        surface: Option<(egui::Rect, egui::LayerId)>,
    ) -> Option<Controls> {
        let focused = ctx.input(|i| i.focused);
        let allowed = flight
            && focused
            && !ctx.wants_keyboard_input()
            && ctx.input(|i| !i.modifiers.ctrl && !i.modifiers.command && !i.modifiers.alt);
        let over_world = surface.is_some_and(|(rect, layer)| {
            ctx.pointer_latest_pos()
                .is_some_and(|p| rect.contains(p) && ctx.layer_id_at(p) == Some(layer))
        });
        let (held, pressed, escape, absolute_delta) = ctx.input(|i| {
            (
                i.pointer.secondary_down(),
                i.pointer.secondary_pressed(),
                i.key_pressed(egui::Key::Escape),
                i.pointer.delta(),
            )
        });
        if !allowed || !held || escape {
            self.release_mouse(ctx);
            if !held {
                self.capture_failed = false;
            }
        } else if self.captured && absolute_delta.length_sq() > 4.0 {
            // egui does not expose the backend grab result. If the OS still moves
            // the absolute pointer, restore it and use visible bounded aiming.
            self.release_mouse(ctx);
            self.capture_failed = true;
            self.aiming = true;
        } else if pressed && over_world {
            self.aiming = true;
            // macOS winit supports Locked. Other backends retain visible aiming
            // until their lock support can be checked through a result-bearing API.
            if cfg!(target_os = "macos") && !self.capture_failed {
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::Locked));
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
                self.captured = true;
            }
        }
        let control = ctx.input(|i| {
            let axis = |positive, negative| {
                f64::from(i.key_down(positive) || i.key_pressed(positive))
                    - f64::from(i.key_down(negative) || i.key_pressed(negative))
            };
            let aiming = self.aiming && (self.captured || over_world);
            let delta = if aiming {
                i.pointer.motion().unwrap_or_else(|| i.pointer.delta())
            } else {
                egui::Vec2::ZERO
            };
            let dt = i.stable_dt.max(1.0 / 240.0) as f64;
            // Mouse rates are normalized to angular speed, independent of render cadence.
            Controls {
                translation: [
                    axis(egui::Key::W, egui::Key::S),
                    axis(egui::Key::Space, egui::Key::C),
                    axis(egui::Key::D, egui::Key::A),
                ],
                rotation: if i.modifiers.shift {
                    [0.0; 3]
                } else {
                    [
                        axis(egui::Key::E, egui::Key::Q),
                        (axis(egui::Key::ArrowLeft, egui::Key::ArrowRight)
                            - delta.x as f64 * 0.002 / dt)
                            .clamp(-1.0, 1.0),
                        (axis(egui::Key::ArrowUp, egui::Key::ArrowDown)
                            - delta.y as f64 * 0.002 / dt)
                            .clamp(-1.0, 1.0),
                    ]
                },
            }
        });
        self.update(allowed && !escape, control)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shift_arrows_do_not_send_ship_rotation() {
        let ctx = egui::Context::default();
        let mut pilot = PilotInput::default();
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                modifiers: egui::Modifiers::SHIFT,
                events: vec![egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                }],
                ..Default::default()
            },
            |ctx| {
                assert_eq!(pilot.sample(ctx, true, None), None);
            },
        );
    }
    #[test]
    fn remote_target_is_pinned_and_return_never_relabels_queued_thrust() {
        let mailbox = Mailbox::default();
        mailbox.publish_for(
            Controls {
                translation: [1.0, 0.0, 0.0],
                rotation: [0.0; 3],
            },
            Some("WorkerA".into()),
        );
        let detached = mailbox.take().unwrap();
        mailbox.clear();
        mailbox.publish_for(Controls::default(), Some("WorkerB".into()));
        assert!(matches!(detached,Request::RemoteControl{pilot,..} if pilot=="WorkerA"));
        assert!(
            matches!(mailbox.take(),Some(Request::RemoteControl{pilot,..}) if pilot=="WorkerB")
        );
        mailbox.publish_for(Controls::default(), Some("WorkerB".into()));
        mailbox.clear();
        assert!(mailbox.take().is_none());
        mailbox.publish(Controls::default());
        assert!(matches!(mailbox.take(), Some(Request::PilotInput { .. })));
    }
    #[test]
    fn latest_generation_keeps_release_and_its_exact_target_together() {
        let mailbox = Mailbox::default();
        for i in 0..100 {
            mailbox.publish_for(
                Controls {
                    translation: [1.0, 0.0, 0.0],
                    rotation: [0.0; 3],
                },
                Some(format!("Worker{i}")),
            );
        }
        mailbox.publish_for(Controls::default(), Some("FinalWorker".into()));
        let Some(Request::RemoteControl { pilot, request }) = mailbox.take() else {
            panic!("remote routing lost")
        };
        assert_eq!(pilot, "FinalWorker");
        assert!(matches!(
            *request,
            Request::PilotInput {
                translation: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0]
            }
        ));
        assert!(mailbox.take().is_none());
    }
    #[test]
    fn release_and_suppression_emit_one_neutral_without_canceling_autopilot() {
        let thrust = Controls {
            translation: [1.0, 0.0, 0.0],
            rotation: [0.0; 3],
        };
        let mut input = PilotInput::default();
        assert_eq!(input.update(true, Controls::default()), None);
        assert_eq!(input.update(true, thrust), Some(thrust));
        assert_eq!(input.update(false, thrust), Some(Controls::default()));
        assert_eq!(input.update(false, thrust), None);
        assert_eq!(input.update(true, Controls::default()), None);
        assert_eq!(input.update(true, thrust), Some(thrust));
        assert_eq!(
            input.update(true, Controls::default()),
            Some(Controls::default())
        );
    }
    #[test]
    fn keyboard_axes_and_focus_loss_match_six_degree_controls() {
        let ctx = egui::Context::default();
        let mut pilot = PilotInput::default();
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                events: vec![
                    egui::Event::Key {
                        key: egui::Key::E,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Key {
                        key: egui::Key::ArrowRight,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                let controls = pilot.sample(ctx, true, None).unwrap();
                assert_eq!(controls.rotation, [1.0, -1.0, 0.0]);
            },
        );
        let _ = ctx.run(
            egui::RawInput {
                focused: false,
                ..Default::default()
            },
            |ctx| {
                assert_eq!(pilot.sample(ctx, true, None), Some(Controls::default()));
            },
        );
    }
    #[test]
    fn autopilot_handoff_discards_queued_release_but_held_controls_can_retake() {
        let mailbox = Mailbox::default();
        let mut pilot = PilotInput::default();
        let thrust = Controls {
            translation: [1.0, 0.0, 0.0],
            rotation: [0.0; 3],
        };
        pilot.update(true, thrust);
        mailbox.publish(pilot.update(true, Controls::default()).unwrap());
        pilot.relinquish(&mailbox);
        assert!(mailbox.is_empty());
        assert_eq!(pilot.update(true, Controls::default()), None);
        assert_eq!(pilot.update(true, thrust), Some(thrust));
    }
    #[test]
    fn focus_loss_releases_hidden_cursor_and_raw_motion_aims_without_pointer_motion() {
        let ctx = egui::Context::default();
        let mut pilot = PilotInput {
            active: true,
            aiming: true,
            captured: true,
            ..Default::default()
        };
        let output = ctx.run(
            egui::RawInput {
                focused: false,
                ..Default::default()
            },
            |ctx| {
                assert_eq!(pilot.sample(ctx, true, None), Some(Controls::default()));
            },
        );
        let commands = &output.viewport_output[&egui::ViewportId::ROOT].commands;
        assert!(commands
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::CursorGrab(egui::CursorGrab::None))));
        assert!(commands
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::CursorVisible(true))));
        assert!(!pilot.captured);
        let point = egui::pos2(50.0, 50.0);
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                events: vec![egui::Event::PointerMoved(point)],
                ..Default::default()
            },
            |_| {},
        );
        pilot.captured = true;
        pilot.aiming = true;
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                events: vec![
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Secondary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::MouseMoved(egui::vec2(10.0, 0.0)),
                ],
                ..Default::default()
            },
            |ctx| {
                let sample = pilot.sample(ctx, true, None).unwrap();
                assert!(sample.rotation[1] < 0.0);
            },
        );
    }
    #[test]
    fn mailbox_replaces_stale_thrust_with_release() {
        let mailbox = Mailbox::default();
        for i in 0..100 {
            mailbox.publish(Controls {
                translation: [i as f64 / 100.0, 0.0, 0.0],
                rotation: [0.0; 3],
            });
        }
        mailbox.publish(Controls::default());
        assert!(matches!(
            mailbox.take(),
            Some(Request::PilotInput {
                translation: [0.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 0.0]
            })
        ));
        assert!(mailbox.take().is_none());
    }
}
