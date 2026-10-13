//! Station embodiment is confirmed by the server; camera motion never boards a ship.
use super::*;
impl Cockpit {
    pub(super) fn physical_engineering_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.engineering_panel.clone() else {
            return;
        };
        if !self.connected
            || self.manual_remote.is_some()
            || self
                .own()
                .as_ref()
                .is_none_or(|p| !crate::room_controls::panel_access(p))
            || self
                .assets
                .as_ref()
                .is_none_or(|a| a.pioneer_room.is_none())
        {
            self.engineering_panel = None;
            return;
        }
        let mut open = true;
        egui::Window::new("Airlock engineering")
            .id(egui::Id::new("physical_engineering"))
            .open(&mut open)
            .default_width(360.)
            .max_width(480.)
            .constrain_to(ctx.available_rect().shrink(8.))
            .vscroll(true)
            .frame(self.overlay_frame(ctx))
            .show(ctx, |ui| {
                ui.small(format!("Device: {id}.breaker"));
                self.pioneer_room_controls(ui, true);
            });
        if !open {
            self.engineering_panel = None;
        }
    }

    fn pioneer_room_controls(&mut self, ui: &mut egui::Ui, access: bool) {
        if self.manual_remote.is_some() {
            return;
        }
        let state = self.assets.as_ref().and_then(|a| a.pioneer_room.as_ref());
        if let Some(command) = crate::room_controls::show(ui, state, self.can_operate() && access) {
            self.send(Request::SetPioneerRoom { action: command });
        }
    }

    fn berth_ready(&self, id: &str) -> bool {
        self.can_operate()
            && self
                .assets
                .as_ref()
                .and_then(|a| a.hangar.as_ref())
                .and_then(|f| f.berths.iter().find(|b| b.id == id))
                .is_some_and(|b| {
                    b.can_board
                        && self.own().is_some_and(|p| {
                            p.station_avatar.as_ref().is_some_and(|a| {
                                !a.onboard_interior
                                    && !a.seated
                                    && a.jump_started_ms.is_none()
                                    && a.position
                                        .iter()
                                        .zip(b.boarding_point)
                                        .map(|(a, b)| (a - b).powi(2))
                                        .sum::<f64>()
                                        .sqrt()
                                        <= STATION_BOARD_RANGE_M
                            })
                        })
                })
    }

    pub(super) fn on_foot_view_selector(&mut self, ui: &mut egui::Ui) {
        use flight3d::hangar::OnFootView;
        ui.selectable_value(
            &mut self.hangar_view.on_foot_view,
            OnFootView::FirstPerson,
            "First person",
        );
        ui.selectable_value(
            &mut self.hangar_view.on_foot_view,
            OnFootView::ThirdPerson,
            "Third person",
        );
    }

    pub(super) fn activate_quarters_terminal(&mut self, action: flight3d::hangar::HangarAction) {
        use flight3d::hangar::HangarAction;
        if !self.in_station_hangar() || self.own().is_none_or(|p| p.station_avatar.is_none()) {
            return;
        }
        self.service_tab = match action {
            HangarAction::Engineering(id) => {
                if id == arkline_protocol::PIONEER_CONNECTED_ROOM.room_id
                    && self.can_operate()
                    && self.manual_remote.is_none()
                    && self
                        .assets
                        .as_ref()
                        .is_some_and(|a| a.pioneer_room.is_some())
                    && self
                        .own()
                        .as_ref()
                        .is_some_and(crate::room_controls::panel_access)
                {
                    self.engineering_panel = Some(id);
                }
                return;
            }
            HangarAction::Overview => ServiceTab::Navigation,
            HangarAction::Ship => ServiceTab::Hangar,
            HangarAction::Inventory => ServiceTab::Cargo,
            HangarAction::Corporation => ServiceTab::Corporation,
            HangarAction::Ai => ServiceTab::ShipComputer,
            HangarAction::BoardShip(id) => {
                if self.berth_ready(&id) {
                    self.send(Request::BoardShip { id });
                }
                return;
            }
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
                let q = if self.assets.as_ref().is_some_and(|a| a.hangar.is_some()) {
                    arkline_protocol::fleet_quarters_layout()
                } else {
                    quarters_layout(p.ship_class)
                };
                p.station_avatar.is_some_and(|a| {
                    !a.onboard_interior
                        && a.position[0] >= q.min[0]
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
        self.station_actions_with_engineering(ui, true);
    }
    fn station_actions_with_engineering(&mut self, ui: &mut egui::Ui, engineering: bool) {
        let Some(pilot) = self.own().filter(|p| p.location == Location::Docked) else {
            return;
        };
        let interior_available = self
            .assets
            .as_ref()
            .is_some_and(|a| a.pioneer_interior_available);
        if pilot.ship_class == ShipClass::Pioneer && !interior_available {
            ui.small("This server has not enabled the Pioneer walkable interior. Connect to a server with interior support; other hull interiors remain planned.");
        }
        if let Some(avatar) = &pilot.station_avatar {
            ui.horizontal(|ui| self.on_foot_view_selector(ui));
            if avatar.onboard_interior {
                let distance = avatar
                    .position
                    .iter()
                    .zip(PIONEER_INTERIOR_ENTRY)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                ui.strong(format!("ON FOOT · PIONEER DECK {}", avatar.interior_deck));
                let room = pioneer_rooms(avatar.interior_deck)
                    .iter()
                    .find(|(_, x, z)| {
                        avatar.position[0] >= *x
                            && avatar.position[0] <= x + 8.9333333333
                            && avatar.position[2] >= *z
                            && avatar.position[2] <= z + 5.4
                    })
                    .map_or("CORRIDOR / ACCESS CORE", |(name, _, _)| *name);
                ui.label(room);
                ui.small("WASD / arrows walk · Space jump · RMB turn / look");
                ui.small(
                    "Airlock engineering uses server-confirmed devices. Other room fixtures remain prototypes.",
                );
                if ui
                    .add_enabled(
                        self.can_operate()
                            && interior_available
                            && distance <= STATION_BOARD_RANGE_M
                            && avatar.interior_deck == 1
                            && avatar.jump_started_ms.is_none(),
                        egui::Button::new("Exit airlock to hangar"),
                    )
                    .clicked()
                {
                    self.send(Request::ExitInterior);
                }
                let lift_distance = avatar
                    .position
                    .iter()
                    .zip(PIONEER_LIFT_POINT)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let next_deck = if avatar.interior_deck == 1 { 2 } else { 1 };
                if ui
                    .add_enabled(
                        self.can_operate()
                            && self
                                .assets
                                .as_ref()
                                .is_some_and(|a| a.pioneer_interior_decks >= 2)
                            && lift_distance <= PIONEER_LIFT_RANGE
                            && avatar.jump_started_ms.is_none(),
                        egui::Button::new(format!("Use lift to deck {next_deck}")),
                    )
                    .clicked()
                {
                    self.send(Request::InteriorLift { deck: next_deck });
                }
                ui.small(format!("Access-core lift · {:.1} m", lift_distance));
                if avatar.interior_deck == 1 {
                    ui.small(format!("Return to airlock · {:.1} m", distance));
                } else {
                    ui.small("Airlock exit is on deck 1. Return via the access-core lift.");
                }
                if engineering {
                    self.pioneer_room_controls(ui, avatar.interior_deck == 1);
                }
                return;
            }

            let fleet = self.assets.as_ref().and_then(|a| a.hangar.clone());
            let point = fleet
                .as_ref()
                .and_then(|f| f.berths.iter().find(|b| b.active))
                .map_or(hangar_layout(pilot.ship_class).boarding_point, |b| {
                    b.boarding_point
                });
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
                    self.can_operate()
                        && distance <= STATION_BOARD_RANGE_M
                        && avatar.jump_started_ms.is_none(),
                    egui::Button::new("Enter ship"),
                )
                .clicked()
            {
                self.send(Request::EnterShip);
            }
            if pilot.ship_class == ShipClass::Pioneer
                && ui
                    .add_enabled(
                        self.can_operate()
                            && interior_available
                            && distance <= STATION_BOARD_RANGE_M
                            && avatar.jump_started_ms.is_none(),
                        egui::Button::new("Explore Pioneer interior"),
                    )
                    .clicked()
            {
                self.send(Request::EnterInterior);
            }
            if let Some(fleet) = &fleet {
                ui.separator();
                ui.strong("Owned vessels · physical boarding access");
                for berth in &fleet.berths {
                    let distance = avatar
                        .position
                        .iter()
                        .zip(berth.boarding_point)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f64>()
                        .sqrt();
                    if ui
                        .add_enabled(
                            self.can_operate()
                                && berth.can_board
                                && distance <= STATION_BOARD_RANGE_M
                                && avatar.jump_started_ms.is_none()
                                && !avatar.seated,
                            egui::Button::new(format!(
                                "Board {} · {:.1} m{}",
                                berth.ship_class.spec().name,
                                distance,
                                if berth.active { " · active" } else { "" }
                            )),
                        )
                        .clicked()
                    {
                        self.send(Request::BoardShip {
                            id: berth.id.clone(),
                        });
                    }
                }
                ui.small("Only your vessels at this station are shown. Corporate berths are not available yet.");
            }
            ui.small("WASD / arrows walk · Space jump · hold RMB to look");
            ui.small(
                "Follow the green path to your quarters. Approach a terminal and click to use it.",
            );
            ui.small("Return to the illuminated boarding marker to enter your ship.");
        } else {
            ui.strong("ABOARD · DOCKED");
            if pilot.ship_class == ShipClass::Pioneer
                && ui
                    .add_enabled(
                        self.can_operate() && interior_available,
                        egui::Button::new("Explore Pioneer interior"),
                    )
                    .clicked()
            {
                self.send(Request::EnterInterior);
            }

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
            if engineering && pilot.ship_class == ShipClass::Pioneer {
                self.pioneer_room_controls(ui, true);
            }
        }
    }
    pub(super) fn station_overlay(&mut self, ctx: &egui::Context) {
        if self.embedded {
            egui::TopBottomPanel::bottom("embedded_station_actions")
                .height_range(34.0..=148.0)
                .resizable(false)
                .frame(self.overlay_frame(ctx))
                .show(ctx, |ui| {
                    if let Some(pilot) = self.own().filter(|p| p.location == Location::Docked) {
                        if pilot.station_avatar.is_none() {
                            let interior_available = self
                                .assets
                                .as_ref()
                                .is_some_and(|assets| assets.pioneer_interior_available);
                            ui.horizontal_wrapped(|ui| {
                                ui.strong("ABOARD · DOCKED");
                                if pilot.ship_class == ShipClass::Pioneer
                                    && ui
                                        .add_enabled(
                                            self.can_operate() && interior_available,
                                            egui::Button::new("Explore interior").small(),
                                        )
                                        .clicked()
                                {
                                    self.send(Request::EnterInterior);
                                }
                                if ui
                                    .add_enabled(
                                        self.can_operate(),
                                        egui::Button::new("Leave ship").small(),
                                    )
                                    .clicked()
                                {
                                    self.send(Request::DisembarkShip);
                                }
                                if ui
                                    .add_enabled(
                                        self.can_operate(),
                                        egui::Button::new("Undock").small(),
                                    )
                                    .clicked()
                                {
                                    self.send(Request::Undock);
                                }
                                if pilot.ship_class == ShipClass::Pioneer
                                    && ui.small_button("Room engineering").clicked()
                                {
                                    self.services_open = true;
                                    self.service_tab = ServiceTab::Station;
                                }
                            });
                            self.power_gauge(ui);
                            return;
                        }
                    }

                    egui::ScrollArea::vertical()
                        .id_salt("embedded_station_action_scroll")
                        .auto_shrink([false, true])
                        .min_scrolled_height(0.0)
                        .max_height(112.0)
                        .show(ui, |ui| {
                            self.station_actions_with_engineering(ui, false);
                            if self
                                .own()
                                .is_some_and(|p| p.ship_class == ShipClass::Pioneer)
                                && ui.small_button("Room engineering").clicked()
                            {
                                self.services_open = true;
                                self.service_tab = ServiceTab::Station;
                            }
                            self.power_gauge(ui);
                        });
                });
            return;
        }

        // Window anchors otherwise use the entire screen, including editor panels.
        let bounds = ctx.screen_rect();
        egui::Window::new(if self.own().is_some_and(|p| p.station_avatar.is_some()) {
            "On-foot navigation"
        } else {
            "Hangar access"
        })
        .id(egui::Id::new("station_embodiment_controls"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -14.0])
        .constrain_to(bounds)
        .resizable(false)
        .collapsible(false)
        .movable(false)
        .frame(self.overlay_frame(ctx))
        .show(ctx, |ui| {
            self.station_actions_with_engineering(ui, false);
            if self
                .own()
                .is_some_and(|p| p.ship_class == ShipClass::Pioneer)
                && ui.button("Room engineering").clicked()
            {
                self.services_open = true;
                self.service_tab = ServiceTab::Station;
            }
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
                let down = |a, b| i.key_down(a) || i.key_down(b);
                let f = f64::from(down(egui::Key::W, egui::Key::ArrowUp))
                    - f64::from(down(egui::Key::S, egui::Key::ArrowDown));
                let s = f64::from(down(egui::Key::D, egui::Key::ArrowRight))
                    - f64::from(down(egui::Key::A, egui::Key::ArrowLeft));
                // Body-local walking avoids a camera-follow/auto-turn feedback loop:
                // strafing stays sideways and backwards walking never spins the camera.
                (
                    f,
                    s,
                    self.hangar_view.walk_turn(avatar.as_ref().unwrap().heading),
                )
            })
        } else {
            (0., 0., 0.)
        };
        if allowed
            && ctx.input(|i| {
                i.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Key {
                            key: egui::Key::Space,
                            pressed: true,
                            repeat: false,
                            ..
                        }
                    )
                })
            })
            && avatar
                .as_ref()
                .is_some_and(|a| a.jump_started_ms.is_none() && !a.seated)
        {
            self.send(Request::StationJump);
        }
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
    fn embedded_docked_strip_reserves_little_viewport_and_keeps_actions_live() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        cockpit.embedded_paused = false;
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.station_avatar = None;
        cockpit.assets = Some(AssetsView {
            pioneer_room: Some(arkline_protocol::PioneerRoomState::uncommissioned(
                "owned-pioneer".into(),
            )),
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 1,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);

        let frame = |cockpit: &mut Cockpit, events| {
            let mut viewport = egui::Rect::NOTHING;
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    cockpit.station_overlay(ctx);
                    viewport = ctx.available_rect();
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            );
            (output, viewport)
        };
        frame(&mut cockpit, vec![]);
        let (output, reserved) = frame(&mut cockpit, vec![]);
        let mut labels = Vec::new();
        for clipped in output.shapes {
            if let egui::Shape::Text(text) = clipped.shape {
                labels.push((
                    text.galley.job.text.clone(),
                    text.pos + text.galley.rect.center().to_vec2(),
                ));
            }
        }
        for label in ["Explore interior", "Leave ship", "Undock", "Room engineering"] {
            assert!(labels.iter().any(|(text, _)| text == label), "missing {label}");
        }
        assert!(reserved.bottom() < 650.0);
        assert!(650.0 - reserved.bottom() < 170.0, "bottom strip grew too tall: {reserved:?}");

        let point = labels.iter().find(|(text, _)| text == "Undock").unwrap().1;
        for pressed in [true, false] {
            frame(
                &mut cockpit,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        assert!(matches!(rx.try_recv().unwrap(), Request::Undock));
    }

    #[test]
    fn physical_berth_action_routes_exact_owned_id_and_rejects_unknown_or_distant() {
        let ctx = egui::Context::default();
        let mut c = crate::tests::view_test_cockpit(&ctx);
        let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar=Some(serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000})).unwrap());
        let (position, boarding_point) = arkline_protocol::fleet_berth(ShipClass::Pioneer);
        c.assets = Some(AssetsView {
            pioneer_room: None,
            hangar: Some(arkline_protocol::HangarView {
                layout: arkline_protocol::fleet_hangar_layout(),
                berths: vec![arkline_protocol::HangarBerth {
                    id: "owned-hull".into(),
                    ship_class: ShipClass::Pioneer,
                    position,
                    yaw: 0.,
                    boarding_point,
                    active: true,
                    can_board: true,
                }],
            }),
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        let (tx, rx) = mpsc::channel();
        c.commands = Some(tx);
        c.activate_quarters_terminal(flight3d::hangar::HangarAction::BoardShip(
            "foreign-hull".into(),
        ));
        assert!(rx.try_recv().is_err());
        c.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .position[0] = 100.;
        c.activate_quarters_terminal(flight3d::hangar::HangarAction::BoardShip(
            "owned-hull".into(),
        ));
        assert!(rx.try_recv().is_err());
        c.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .position[0] = 0.;
        c.activate_quarters_terminal(flight3d::hangar::HangarAction::BoardShip(
            "owned-hull".into(),
        ));
        assert!(matches!(rx.try_recv().unwrap(),Request::BoardShip{id} if id=="owned-hull"));
    }
    #[test]
    fn missing_resume_guidance_does_not_imply_retry_recovers_identity() {
        let message = resume_guidance("resume token required for existing pilot".into());
        assert!(
            message.contains("stored separately")
                && message.contains("Retrying alone cannot recover")
        );
        assert_eq!(
            resume_guidance("Connection refused".into()),
            "Connection refused"
        );
    }

    #[test]
    fn physical_engineering_opens_only_owned_accessible_room_and_closes_for_remote() {
        let ctx = egui::Context::default();
        let mut c = crate::tests::view_test_cockpit(&ctx);
        let pilot = &mut c.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.ship_class = ShipClass::Pioneer;
        pilot.station_avatar=Some(serde_json::from_value(serde_json::json!({"position":[6.,0.,3.],"heading":0.,"moving":false,"updated_at_ms":0,"onboard_interior":true,"interior_deck":1})).unwrap());
        c.assets = Some(AssetsView {
            pioneer_room: Some(arkline_protocol::PioneerRoomState::uncommissioned(
                "own".into(),
            )),
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        let action = || {
            flight3d::hangar::HangarAction::Engineering(
                arkline_protocol::PIONEER_CONNECTED_ROOM.room_id.into(),
            )
        };
        c.activate_quarters_terminal(flight3d::hangar::HangarAction::Engineering(
            "foreign".into(),
        ));
        assert!(c.engineering_panel.is_none());
        c.activate_quarters_terminal(action());
        assert!(c.engineering_panel.is_some());
        c.manual_remote = Some("worker".into());
        let _ = ctx.run(Default::default(), |ctx| c.physical_engineering_window(ctx));
        assert!(c.engineering_panel.is_none());
        c.activate_quarters_terminal(action());
        assert!(c.engineering_panel.is_none());
        c.manual_remote = None;
        c.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .position = [20., 0., 3.];
        c.activate_quarters_terminal(action());
        assert!(c.engineering_panel.is_none());
    }
    #[test]
    fn interior_button_requires_server_capability_and_sends_real_request() {
        fn frame(
            ctx: &egui::Context,
            c: &mut Cockpit,
            events: Vec<egui::Event>,
        ) -> Vec<(String, Pos2)> {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(900.0, 650.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| c.station_actions(ui));
                },
            )
            .shapes
            .into_iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = s.shape {
                    Some((
                        t.galley.job.text.clone(),
                        t.pos + t.galley.rect.center().to_vec2(),
                    ))
                } else {
                    None
                }
            })
            .collect()
        }
        for supported in [false, true] {
            let ctx = egui::Context::default();
            let mut c = crate::tests::view_test_cockpit(&ctx);
            c.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
            c.assets = Some(AssetsView {
                pioneer_room: None,
                hangar: None,
                pioneer_interior_available: supported,
                pioneer_interior_decks: if supported { 2 } else { 0 },
                ships: vec![],
                storage: vec![],
                corporation: None,
                invitations: vec![],
            });
            let (tx, rx) = mpsc::channel();
            c.commands = Some(tx);
            frame(&ctx, &mut c, vec![]);
            let labels = frame(&ctx, &mut c, vec![]);
            let point = labels
                .iter()
                .find(|(t, _)| t == "Explore Pioneer interior")
                .unwrap()
                .1;
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut c,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(
                matches!(rx.try_recv(), Ok(Request::EnterInterior)),
                supported
            );
            c.request_pending = false;
            c.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(StationAvatar {
                onboard_interior: true,
                interior_deck: 1,
                position: PIONEER_LIFT_POINT,
                jump_started_ms: None,
                seated: false,
                heading: 0.0,
                moving: false,
                updated_at_ms: 1000,
            });
            frame(&ctx, &mut c, vec![]);
            assert_eq!(c.operation_status().0, "ON FOOT / PIONEER DECK 1");
            assert!(c.operation_status().1.contains("airlock"));
            c.snapshot.as_mut().unwrap().pilots[0]
                .station_avatar
                .as_mut()
                .unwrap()
                .interior_deck = 2;
            assert_eq!(c.operation_status().0, "ON FOOT / PIONEER DECK 2");
            assert!(c.operation_status().1.contains("lift"));
            c.snapshot.as_mut().unwrap().pilots[0]
                .station_avatar
                .as_mut()
                .unwrap()
                .interior_deck = 1;
            let point = frame(&ctx, &mut c, vec![])
                .iter()
                .find(|(t, _)| t == "Use lift to deck 2")
                .unwrap()
                .1;
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut c,
                    vec![
                        egui::Event::PointerMoved(point),
                        egui::Event::PointerButton {
                            pos: point,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(
                matches!(rx.try_recv(), Ok(Request::InteriorLift { deck: 2 })),
                supported
            );
        }
    }
    #[test]
    fn arrows_match_wasd_and_space_sends_one_authoritative_jump() {
        let command_for = |key| {
            let ctx = egui::Context::default();
            let mut cockpit = crate::tests::view_test_cockpit(&ctx);
            let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
            p.location = Location::Docked;
            p.station_avatar = Some(StationAvatar {
                onboard_interior: false,
                interior_deck: 1,
                jump_started_ms: None,
                seated: false,
                position: hangar_layout(p.ship_class).boarding_point,
                heading: 0.,
                moving: false,
                updated_at_ms: 0,
            });
            let (tx, rx) = mpsc::channel();
            cockpit.commands = Some(tx);
            let _ = ctx.run(
                egui::RawInput {
                    focused: true,
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |ctx| cockpit.sample_station_walk(ctx, true),
            );
            (cockpit.controls.take(), rx.try_recv().ok())
        };
        for (wasd, arrow) in [
            (egui::Key::W, egui::Key::ArrowUp),
            (egui::Key::S, egui::Key::ArrowDown),
            (egui::Key::A, egui::Key::ArrowLeft),
            (egui::Key::D, egui::Key::ArrowRight),
        ] {
            assert_eq!(
                serde_json::to_value(command_for(wasd).0).unwrap(),
                serde_json::to_value(command_for(arrow).0).unwrap()
            );
        }
        assert!(matches!(
            command_for(egui::Key::Space).1,
            Some(Request::StationJump)
        ));
    }
    #[test]
    fn walk_controls_are_own_only_and_release_on_focus_loss() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar = Some(StationAvatar {
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
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
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
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
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
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
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
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
