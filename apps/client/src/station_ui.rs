//! Station embodiment is confirmed by the server; camera motion never boards a ship.
use super::*;
impl Cockpit {
    pub(super) fn in_station_hangar(&self) -> bool {
        self.manual_remote.is_none() && self.own().is_some_and(|p| p.location == Location::Docked)
    }
    pub(super) fn station_actions(&mut self, ui: &mut egui::Ui) {
        let Some(pilot) = self.own().filter(|p| p.location == Location::Docked) else {
            return;
        };
        if let Some(avatar) = &pilot.station_avatar {
            let point = hangar_layout(pilot.ship_class).boarding_point;
            let distance = avatar
                .position
                .iter()
                .zip(point)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt();
            ui.strong("ON FOOT · STATION HANGAR");
            ui.small(format!(
                "{} · boarding access {:.1} m",
                pilot.ship_class.spec().name,
                distance
            ));
            if ui
                .add_enabled(
                    self.can_operate() && distance <= STATION_BOARD_RANGE_M,
                    egui::Button::new("Enter ship"),
                )
                .clicked()
            {
                self.send(Request::EnterShip);
            }
            ui.small("W/S walk · A/D strafe · ←/→ turn · hold RMB to look");
            ui.small("Walk to the illuminated boarding marker to enter. Board before undocking.");
        } else {
            ui.strong("ABOARD · DOCKED");
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.can_operate(), egui::Button::new("Leave ship"))
                    .clicked()
                {
                    self.send(Request::DisembarkShip);
                }
                if ui
                    .add_enabled(
                        self.can_operate(),
                        egui::Button::new("Undock active vessel"),
                    )
                    .clicked()
                {
                    self.send(Request::Undock);
                }
            });
            ui.small("Drag with RMB to inspect your ship · wheel to zoom");
        }
    }
    pub(super) fn station_overlay(&mut self, ctx: &egui::Context) {
        egui::Window::new("Hangar access")
            .id(egui::Id::new("station_embodiment_controls"))
            .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -14.0])
            .resizable(false)
            .collapsible(false)
            .movable(false)
            .frame(self.overlay_frame(ctx))
            .show(ctx, |ui| {
                self.station_actions(ui);
                self.power_gauge(ui);
            });
    }
    pub(super) fn sample_station_walk(&mut self, ctx: &egui::Context, in_hangar: bool) {
        let avatar = self.own().and_then(|p| p.station_avatar);
        let fresh = self.snapshot.as_ref().is_some_and(|s| {
            self.timeline_origin
                .map_or(s.server_time_ms, |t| {
                    t.saturating_add(self.observed.elapsed().as_millis() as u64)
                })
                .saturating_sub(s.server_time_ms)
                <= 1500
        });
        let over_world = self
            .hangar_view
            .input_surface()
            .is_some_and(|(rect, layer)| {
                ctx.pointer_latest_pos()
                    .is_some_and(|pos| rect.contains(pos) && ctx.layer_id_at(pos) == Some(layer))
            });
        let pointer_blocked = ctx.input(|i| i.pointer.primary_down()) && !over_world;
        let allowed = !pointer_blocked
            && in_hangar
            && avatar.is_some()
            && self.can_operate()
            && fresh
            && !ctx.wants_keyboard_input()
            && ctx.input(|i| {
                i.focused && !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.command
            });
        let (forward, strafe, turn) = if allowed {
            ctx.input(|i| {
                let axis = |a, b| {
                    f64::from(i.key_down(a) || i.key_pressed(a))
                        - f64::from(i.key_down(b) || i.key_pressed(b))
                };
                let f = axis(egui::Key::W, egui::Key::S);
                let s = axis(egui::Key::D, egui::Key::A);
                let delta = self.hangar_view.camera_yaw() - avatar.as_ref().unwrap().heading;
                let (sin, cos) = delta.sin_cos();
                (
                    (f * cos - s * sin).clamp(-1., 1.),
                    (f * sin + s * cos).clamp(-1., 1.),
                    axis(egui::Key::ArrowRight, egui::Key::ArrowLeft),
                )
            })
        } else {
            (0., 0., 0.)
        };
        let active = forward != 0. || strafe != 0. || turn != 0.;
        if active || self.station_walk_active {
            if in_hangar && avatar.is_some() && self.connected {
                self.controls.publish_station(forward, strafe, turn);
            } else {
                self.controls.clear();
            }
        }
        self.station_walk_active = active;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn walk_controls_are_own_only_and_release_on_focus_loss() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar = Some(StationAvatar {
            position: hangar_layout(p.ship_class).boarding_point,
            heading: 0.,
            moving: false,
            updated_at_ms: 0,
        });
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                events: vec![egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| cockpit.sample_station_walk(ctx, true),
        );
        assert!(
            matches!(cockpit.controls.take(),Some(Request::StationWalk{forward,strafe,..}) if forward.abs()+strafe.abs()>0.)
        );
        let _ = ctx.run(
            egui::RawInput {
                focused: false,
                ..Default::default()
            },
            |ctx| cockpit.sample_station_walk(ctx, true),
        );
        assert!(matches!(
            cockpit.controls.take(),
            Some(Request::StationWalk {
                forward: 0.,
                strafe: 0.,
                turn: 0.
            })
        ));
        let _ = ctx.run(
            egui::RawInput {
                focused: false,
                ..Default::default()
            },
            |ctx| cockpit.sample_station_walk(ctx, true),
        );
        assert!(cockpit.controls.take().is_none());
    }
    #[test]
    fn docked_hangar_requires_own_view_and_gathering_requires_embarked_pilot() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar = Some(StationAvatar {
            position: hangar_layout(p.ship_class).boarding_point,
            heading: 0.,
            moving: false,
            updated_at_ms: 0,
        });
        assert!(cockpit.in_station_hangar());
        assert!(cockpit.resource_start_reason().unwrap().contains("Enter"));
        cockpit.manual_remote = Some("worker".into());
        assert!(!cockpit.in_station_hangar());
    }
}
