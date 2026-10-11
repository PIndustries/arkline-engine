//! Station embodiment is confirmed by the server; camera motion never boards a ship.
use super::*;
impl Cockpit {
    pub(super) fn activate_quarters_terminal(&mut self, action: flight3d::hangar::HangarAction) {
        use flight3d::hangar::HangarAction;
        if !self.in_station_hangar() || self.own().is_none_or(|p| p.station_avatar.is_none()) {
            return;
        }
        self.service_tab = match action {
            HangarAction::Overview => ServiceTab::Navigation,
            HangarAction::Ship => ServiceTab::Hangar,
            HangarAction::Inventory => ServiceTab::Cargo,
            HangarAction::Corporation => ServiceTab::Corporation,
            HangarAction::Ai => ServiceTab::ShipComputer,
            HangarAction::Sit => {
                if self.can_operate() {
                    self.send(Request::StationSit { seated: true });
                }
                return;
            }
        };
        self.services_open = true;
    }

    pub(super) fn in_captains_quarters(&self) -> bool {
        self.in_station_hangar()
            && self.own().is_some_and(|p| {
                let q = quarters_layout(p.ship_class);
                p.station_avatar.is_some_and(|a| {
                    a.position[0] >= q.min[0]
                        && a.position[0] <= q.max[0]
                        && a.position[2] >= q.min[2]
                        && a.position[2] <= q.max[2]
                })
            })
    }
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
            ui.strong(if avatar.seated {
                "SEATED · CAPTAIN'S QUARTERS"
            } else if self.in_captains_quarters() {
                "ON FOOT · CAPTAIN'S QUARTERS"
            } else {
                "ON FOOT · STATION HANGAR"
            });
            if avatar.seated
                && ui
                    .add_enabled(self.can_operate(), egui::Button::new("Stand up"))
                    .clicked()
            {
                self.send(Request::StationSit { seated: false });
            }
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
            ui.small(
                "Follow the green path to your quarters. Approach a terminal and click to use it.",
            );
            ui.small("Return to the illuminated boarding marker to enter your ship.");
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
                let axis = |a, b| f64::from(i.key_down(a)) - f64::from(i.key_down(b));
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
            seated: false,
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
    fn walk_key_release_replaces_pending_thrust_with_neutral() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar = Some(StationAvatar {
            seated: false,
            position: hangar_layout(p.ship_class).boarding_point,
            heading: 0.,
            moving: false,
            updated_at_ms: 0,
        });
        let key = |pressed| egui::Event::Key {
            key: egui::Key::W,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        for events in [vec![key(true)], vec![key(false)]] {
            let _ = ctx.run(
                egui::RawInput {
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| cockpit.sample_station_walk(ctx, true),
            );
        }
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
                focused: true,
                ..Default::default()
            },
            |ctx| cockpit.sample_station_walk(ctx, true),
        );
        assert!(cockpit.controls.take().is_none());
        // Fast press/release in one render frame must not leave a walk lease.
        let _ = ctx.run(
            egui::RawInput {
                focused: true,
                events: vec![key(true), key(false)],
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
            seated: false,
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
    #[test]
    fn quarters_terminal_routes_services_and_seating_clears_movement() {
        use flight3d::hangar::HangarAction;
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar = Some(StationAvatar {
            seated: false,
            position: quarters_layout(p.ship_class).seat_point,
            heading: 0.,
            moving: false,
            updated_at_ms: 1000,
        });
        assert!(cockpit.in_captains_quarters());
        for (action, tab) in [
            (HangarAction::Overview, ServiceTab::Navigation),
            (HangarAction::Ship, ServiceTab::Hangar),
            (HangarAction::Inventory, ServiceTab::Cargo),
            (HangarAction::Corporation, ServiceTab::Corporation),
            (HangarAction::Ai, ServiceTab::ShipComputer),
        ] {
            cockpit.activate_quarters_terminal(action);
            assert!(cockpit.services_open && cockpit.service_tab == tab);
        }
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.controls.publish_station(1., 0., 0.);
        cockpit.station_walk_active = true;
        cockpit.activate_quarters_terminal(HangarAction::Sit);
        assert!(cockpit.controls.take().is_none());
        assert!(!cockpit.station_walk_active);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::StationSit { seated: true }
        ));
    }
}
