//! Station embodiment is confirmed by the server; camera motion never boards a ship.
use super::*;

fn live_room_name(
    layout: &arkline_protocol::RoomLayoutDocument,
    deck: u8,
    position: [f64; 3],
) -> Option<&str> {
    layout.rooms.iter().find_map(|room| {
        let [x, z] = room.origin_xz_m;
        let [length, depth, _] = room.clear_lwh_m;
        (room.deck == deck
            && position[0] >= x
            && position[0] <= x + length
            && position[2] >= z
            && position[2] <= z + depth)
            .then_some(room.name.as_str())
    })
}

pub(super) fn docked_support_label(support: DockedSupport) -> &'static str {
    match support {
        DockedSupport::Station => "STATION HANGAR",
        DockedSupport::Deck1 => "PIONEER DECK 1",
        DockedSupport::Deck2 => "PIONEER DECK 2",
        DockedSupport::Cockpit => "PIONEER COCKPIT",
        DockedSupport::Connector => "PIONEER AIRLOCK",
        DockedSupport::CoreStairs => "PIONEER ACCESS STAIRS",
        DockedSupport::BoardingLift => "BOARDING LIFT",
    }
}

pub(super) fn docked_state_label(state: &DockedTraversalState) -> String {
    match arkline_protocol::docked_traversal_deck(state) {
        Err(_) => "PIONEER REGISTRATION UNRESOLVED".into(),
        Ok(Some(deck)) if state.support == DockedSupport::CoreStairs => {
            format!("PIONEER ACCESS STAIRS · DECK {deck}")
        }
        _ => docked_support_label(state.support).into(),
    }
}

pub(super) fn docked_exit_guidance(state: &DockedTraversalState) -> &'static str {
    match state.support {
        DockedSupport::Station => "Station hangar: the boarding lift returns to your ship.",
        DockedSupport::BoardingLift => "Exit to hangar: close both hatches, then select Lift to station.",
        DockedSupport::Deck1 => "Exit to hangar: use the access stairs to Deck 2, then the port boarding vestibule beside the cockpit.",
        DockedSupport::CoreStairs if arkline_protocol::docked_traversal_deck(state) == Ok(Some(1)) =>
            "Exit to hangar: continue up to Deck 2, then the port boarding vestibule beside the cockpit.",
        DockedSupport::Connector => "Exit to hangar: close the inner hatch, open the outer hatch and step onto the lift. Close the outer hatch before Lift to station.",
        _ => "Exit to hangar: follow the cockpit's port boarding vestibule, then use the inner hatch, outer hatch and boarding lift.",
    }
}

fn station_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}

impl Cockpit {
    /// A capability is usable only for the exact embedded source, the active
    /// owned host and the current layout. An incompatible active avatar never
    /// falls through to historical teleport controls.
    pub(super) fn verified_docked_binding(&self) -> Option<DockedTraversalBinding> {
        if self.manual_remote.is_some() {
            return None;
        }
        let pilot = self.own()?;
        if pilot.name != self.pilot
            || (pilot.location != Location::Docked && !self.space_walk_available())
            || pilot.ship_class != ShipClass::Pioneer
        {
            return None;
        }
        // Existing legacy crew/station movement stays available until the
        // pilot returns aboard and explicitly starts this docked journey.
        if pilot
            .station_avatar
            .as_ref()
            .is_some_and(|avatar| avatar.docked_traversal.is_none())
        {
            return None;
        }
        let snapshot = self.snapshot.as_ref()?;
        let capability = snapshot.docked_traversal_capability.as_ref()?;
        if capability != &docked_traversal_capability() {
            return None;
        }
        let assets = self.assets.as_ref()?;
        let host = assets.pioneer_room.as_ref()?.vessel_id.clone();
        // In flight the active vessel is intentionally absent from station
        // berths. Registration/owned room/layout still bind the local interior.
        if pilot.location == Location::Docked
            && assets.hangar.as_ref().is_some_and(|fleet| {
                !fleet.berths.iter().any(|berth| {
                    berth.active && berth.id == host && berth.ship_class == ShipClass::Pioneer
                })
            })
        {
            return None;
        }
        let document = assets
            .pioneer_room_layout
            .as_ref()
            .map(|state| state.document.clone())
            .unwrap_or_else(default_pioneer_room_layout);
        document.validate_live().ok()?;
        let fleet_layout = snapshot_world_layout(snapshot).ok()?.fleet_layout;
        if assets
            .hangar
            .as_ref()
            .is_some_and(|fleet| fleet.layout_revision != fleet_layout)
        {
            return None;
        }
        let binding =
            docked_traversal_binding(host, &document, fleet_layout, assets.hangar.is_some());
        docked_binding_pose(&binding).ok()?;
        if let Some(state) = pilot
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref())
        {
            if state.version != DOCKED_TRAVERSAL_VERSION
                || state.binding != binding
                || state.actuators.binding != binding
                || !state.feet_m.iter().all(|coordinate| coordinate.is_finite())
                || !state.heading.is_finite()
            {
                return None;
            }
        }
        Some(binding)
    }

    pub(super) fn space_walk_available(&self) -> bool {
        self.manual_remote.is_none()
            && self.own().is_some_and(|pilot| {
                pilot.ship_class == ShipClass::Pioneer
                    && pilot.location != Location::Docked
                    && (pilot
                        .station_avatar
                        .as_ref()
                        .is_some_and(|avatar| avatar.docked_traversal.is_some())
                        || self.snapshot.as_ref().is_some_and(|snapshot| {
                            snapshot.resource_jobs.iter().any(|job| {
                                job.pilot == pilot.name
                                    && job.controller == self.pilot
                                    && !job.phase.is_terminal()
                            })
                        }))
            })
    }

    pub(super) fn docked_navigation_present(&self) -> bool {
        self.own().is_some_and(|pilot| {
            if pilot
                .station_avatar
                .as_ref()
                .is_some_and(|avatar| avatar.docked_traversal.is_some())
            {
                return true;
            }
            // An existing legacy avatar retains its current journey. A known
            // canonical seated Pioneer must never gain legacy teleport entry
            // merely because its capability, assets or binding are unresolved.
            pilot.name == self.pilot
                && self.manual_remote.is_none()
                && (pilot.location == Location::Docked || self.space_walk_available())
                && pilot.ship_class == ShipClass::Pioneer
                && pilot.station_avatar.is_none()
                && self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.docked_traversal_capability.is_some())
        })
    }

    fn docked_room_name(&self, state: &DockedTraversalState) -> Option<String> {
        if !matches!(state.support, DockedSupport::Deck1 | DockedSupport::Deck2) {
            return None;
        }
        let deck = arkline_protocol::docked_traversal_deck(state).ok()??;
        let pose = docked_binding_pose(&state.binding).ok()?;
        let body = arkline_protocol::station_to_body(pose, state.feet_m);
        let position = [
            body[0] + 12.0,
            body[1] - arkline_protocol::deck_floor_y(deck)?,
            body[2] + 7.0,
        ];
        let document = self
            .assets
            .as_ref()
            .and_then(|assets| assets.pioneer_room_layout.as_ref())
            .map(|layout| layout.document.clone())
            .unwrap_or_else(default_pioneer_room_layout);
        live_room_name(&document, deck, position).map(str::to_owned)
    }

    fn traversal_snapshot_fresh(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            self.presentation_time()
                .saturating_sub(snapshot.server_time_ms)
                <= 1500
        })
    }

    pub(super) fn traversal_transition_block_reason(&self) -> Option<&'static str> {
        if !self.connected {
            Some("Reconnect before changing seats or undocking")
        } else if self.embedded_paused {
            Some("Resume simulation before changing seats or undocking")
        } else if !self.traversal_snapshot_fresh() {
            Some("Waiting for a fresh world update before changing seats or undocking")
        } else if self.request_pending {
            Some("Waiting for command confirmation")
        } else {
            None
        }
    }

    pub(super) fn docked_leave_seat_block_reason(&self) -> Option<&'static str> {
        if let Some(reason) = self.traversal_transition_block_reason() {
            return Some(reason);
        }
        let pilot = self.own()?;
        let job = self.snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .resource_jobs
                .iter()
                .find(|job| job.pilot == pilot.name && !job.phase.is_terminal())
        });
        let owned = job.is_some_and(|job| job.controller == self.pilot);
        if job.is_some() && !owned {
            Some("Only your own mining routine permits interior traversal")
        } else if !owned
            && pilot.flight.as_ref().is_some_and(|flight| {
                !matches!(flight.mode, NavigationMode::Stop | NavigationMode::Manual)
                    || flight.velocity.iter().any(|value| value.abs() >= 0.001)
            })
        {
            Some("Stop vessel automation and motion before leaving the pilot seat")
        } else {
            None
        }
    }

    /// Mirrors TakeSeat's grounding/range checks and can_undock's boarding
    /// equipment checks. These are UI explanations; the server still decides.
    pub(super) fn docked_seat_block_reason(
        &self,
        state: &DockedTraversalState,
    ) -> Option<&'static str> {
        if let Some(reason) = self.traversal_transition_block_reason() {
            return Some(reason);
        }
        let Ok(pose) = docked_binding_pose(&state.binding) else {
            return Some("Pilot chair registration unavailable · reconnect");
        };
        if state.seat_transition.is_some() {
            Some("Finish the pilot-seat transition first")
        } else if !state.grounded {
            Some("Land before returning to the pilot seat")
        } else if station_distance(state.feet_m, seat_anchors(pose).root_station_m) > 1.0 {
            Some("Walk within 1 m of the pilot chair")
        } else if !state.actuators.hatches_closed() {
            Some("Close both boarding hatches before piloting")
        } else if state.actuators.lift.moving {
            Some("Wait for the boarding lift to stop at ship level")
        } else if (state.actuators.lift.floor_station_m - lift_geometry(pose).ship_floor_m).abs()
            > 0.015
        {
            Some("Stow the boarding lift at ship level before piloting")
        } else {
            None
        }
    }

    fn space_walk_routine_status(&self) -> &'static str {
        if !self.connected {
            "Disconnected · mining routine status unavailable"
        } else if !self.traversal_snapshot_fresh() {
            "Stale world data · mining routine status unavailable"
        } else if self.snapshot.as_ref().is_some_and(|snapshot| {
            self.own().is_some_and(|pilot| {
                snapshot.resource_jobs.iter().any(|job| {
                    job.pilot == pilot.name
                        && job.controller == self.pilot
                        && !job.phase.is_terminal()
                })
            })
        }) {
            if self.embedded_paused {
                "Mining routine paused · resume simulation to continue"
            } else {
                "Mining autopilot active"
            }
        } else {
            "No active owned mining routine · manual flight locked while on foot"
        }
    }

    fn docked_station_actions(&mut self, ui: &mut egui::Ui) -> bool {
        if !self.docked_navigation_present() {
            return false;
        }
        let Some(binding) = self.verified_docked_binding() else {
            ui.strong("Pioneer traversal unavailable");
            ui.small("The ship registration or room layout does not match this client. Reconnect with matching sources before moving.");
            return true;
        };
        let Some(pilot) = self.own() else {
            return true;
        };
        let Ok(pose) = docked_binding_pose(&binding) else {
            return true;
        };
        let in_space = pilot.location != Location::Docked;
        let transition_block = self.traversal_transition_block_reason();
        let state = pilot
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref());
        if pilot.station_avatar.is_some() && state.is_none() {
            ui.small("Return to the cockpit using the current station session before starting the registered journey.");
            return true;
        }
        if let Some(state) = state {
            ui.horizontal_wrapped(|ui| {
                ui.strong(docked_state_label(state));
                if state.seat_transition.is_some() {
                    ui.small("Pilot seat transition");
                } else if state.actuators.lift.moving {
                    ui.small("Lift moving");
                } else if state.sprinting {
                    ui.small("Sprinting");
                }
            });
            if let Some(room) = self.docked_room_name(state) {
                ui.small(format!("Room: {room}"));
            }
            let seats = seat_anchors(pose);
            let seat_distance = station_distance(state.feet_m, seats.root_station_m);
            let seat_block = self.docked_seat_block_reason(state);
            ui.horizontal_wrapped(|ui| {
                if seat_distance <= 1.5
                    && ui
                        .add_enabled(
                            seat_block.is_none(),
                            egui::Button::new(if in_space {
                                "Return to pilot seat"
                            } else {
                                "Take pilot seat"
                            }),
                        )
                        .clicked()
                {
                    self.send(Request::DockedTraversal {
                        request: DockedTraversalRequest::TakeSeat {
                            binding: binding.clone(),
                        },
                    });
                }
                if !in_space {
                    self.boarding_hatch_controls(ui, state);
                    self.boarding_lift_controls(ui, state);
                }
            });
            ui.small(format!("Pilot chair · {seat_distance:.1} m"));
            if transition_block.is_none() {
                if let Some(reason) = seat_block {
                    ui.small(reason);
                }
            }
            if in_space {
                ui.small(self.space_walk_routine_status());
                ui.small("Walk back to the cockpit chair to return to the pilot seat. Exterior access is locked in space.");
            } else {
                ui.small(docked_exit_guidance(state));
            }
            ui.small(
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.walk_sprint_v1)
                {
                    format!(
                        "W/S or arrows move/turn · Q/E strafe · {} · Shift sprint · Space jump",
                        self.on_foot_look_hint()
                    )
                } else {
                    format!(
                        "W/S or arrows move/turn · Q/E strafe · {} · Space jump",
                        self.on_foot_look_hint()
                    )
                },
            );
            if !state.actuators.service_power_available {
                ui.small("Docking service power unavailable · lift held in place.");
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.strong("PIONEER · PILOT SEAT");
                if ui
                    .add_enabled(
                        self.docked_leave_seat_block_reason().is_none(),
                        egui::Button::new("Leave pilot seat"),
                    )
                    .clicked()
                {
                    self.send(Request::DockedTraversal {
                        request: DockedTraversalRequest::LeaveSeat {
                            binding: binding.clone(),
                        },
                    });
                }
                if !in_space
                    && ui
                        .add_enabled(
                            transition_block.is_none() && self.departure_order_reason().is_none(),
                            egui::Button::new("Undock"),
                        )
                        .clicked()
                {
                    self.send(Request::Undock);
                }
            });
            if transition_block.is_none() {
                if let Some(reason) = self.docked_leave_seat_block_reason() {
                    ui.small(reason);
                }
            }
            if !in_space {
                if let Some(reason) = self.departure_order_reason() {
                    ui.colored_label(AMBER, reason);
                }
            }
            ui.small(if in_space { "Leave the pilot seat to walk inside. Manual flight controls are unavailable away from the seat. Exterior access stays locked." } else { "Exit to hangar: leave the seat and walk to the port boarding vestibule, then use the hatches and lift. Shift + arrows look around while seated." });
        }
        if let Some(reason) = transition_block {
            ui.small(reason);
        }
        true
    }

    pub(super) fn stop_station_walk(&mut self) {
        if !self.station_walk_active {
            return;
        }
        if self.connected {
            if let Some(binding) = self.docked_walk_binding.take() {
                self.controls.publish_docked(binding, 0.0, 0.0, 0.0, false);
            } else {
                self.controls.publish_station(0.0, 0.0, 0.0, false);
            }
        } else {
            self.controls.clear();
            self.docked_walk_binding = None;
        }
        self.station_walk_active = false;
    }
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
                self.ship_power_accounting(ui);
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
            && self.own().is_none_or(|pilot| {
                pilot
                    .station_avatar
                    .as_ref()
                    .is_none_or(|avatar| avatar.docked_traversal.is_none())
            })
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
        for (view, label) in [
            (OnFootView::FirstPerson, "First person"),
            (OnFootView::ThirdPerson, "Third person"),
        ] {
            if ui
                .selectable_label(
                    !self.ship_observation && self.hangar_view.on_foot_view == view,
                    label,
                )
                .clicked()
            {
                // Match the main view selector's body-return path: camera
                // choice changes presentation only and releases old input.
                self.hangar_view.on_foot_view = view;
                self.ship_observation = false;
                self.release_workspace_input();
            }
        }
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
                if self.can_operate() && !self.docked_navigation_present() {
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
                    let point = if let Some(state) = a.docked_traversal.as_ref() {
                        if state.support != DockedSupport::Station {
                            return false;
                        }
                        state.feet_m
                    } else {
                        if a.onboard_interior {
                            return false;
                        }
                        a.position
                    };
                    point[0] >= q.min[0]
                        && point[0] <= q.max[0]
                        && point[2] >= q.min[2]
                        && point[2] <= q.max[2]
                })
            })
    }
    pub(super) fn in_station_hangar(&self) -> bool {
        if self.ship_observation
            && self
                .own()
                .is_some_and(|pilot| pilot.station_avatar.is_some())
        {
            return false;
        }
        self.manual_remote.is_none()
            && self.own().is_some_and(|p| {
                if p.flight.as_ref().is_some_and(|f| f.docking.is_some()) {
                    return p
                        .station_avatar
                        .as_ref()
                        .is_some_and(|a| a.docked_traversal.is_some());
                }
                if p.location != Location::Docked {
                    // Only walking uses the ship-local interior renderer.
                    // A seated autopilot keeps FlightView's complete cockpit,
                    // instrument textures and drone-feed composition.
                    return p
                        .station_avatar
                        .as_ref()
                        .is_some_and(|avatar| avatar.docked_traversal.is_some());
                }
                p.station_avatar.is_some()
                    // Registered cockpit geometry uses station metres;
                    // aboard external/tactical cameras keep their own renderer.
                    || if self.docked_navigation_present() {
                        self.view_mode == ViewMode::Cockpit
                    } else {
                        !self.embedded
                    }
            })
    }
    pub(super) fn station_actions(&mut self, ui: &mut egui::Ui) {
        self.station_actions_with_engineering(ui, true);
    }
    fn station_actions_with_engineering(&mut self, ui: &mut egui::Ui, engineering: bool) {
        self.station_actions_inner(ui, engineering, false);
    }
    fn station_actions_inner(
        &mut self,
        ui: &mut egui::Ui,
        engineering: bool,
        primary_promoted: bool,
    ) {
        if self.docked_station_actions(ui) {
            return;
        }
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
                let room = self
                    .assets
                    .as_ref()
                    .and_then(|assets| assets.pioneer_room_layout.as_ref())
                    .map(|layout| {
                        live_room_name(&layout.document, avatar.interior_deck, avatar.position)
                            .unwrap_or("CORRIDOR / ACCESS CORE")
                    })
                    .unwrap_or_else(|| {
                        pioneer_rooms(avatar.interior_deck)
                            .iter()
                            .find(|(_, x, z)| {
                                avatar.position[0] >= *x
                                    && avatar.position[0] <= x + 8.9333333333
                                    && avatar.position[2] >= *z
                                    && avatar.position[2] <= z + 5.4
                            })
                            .map_or("CORRIDOR / ACCESS CORE", |(name, _, _)| *name)
                    });
                ui.label(room);
                ui.small(format!(
                    "W/S or Up/Down move · A/D or Left/Right turn · Q/E strafe · {} · Space jump",
                    self.on_foot_look_hint()
                ));
                ui.small(
                    "Airlock engineering uses server-confirmed devices. Other room fixtures remain prototypes.",
                );
                if !primary_promoted {
                    self.interior_exit_action(ui, avatar, interior_available, distance);
                    self.legacy_interior_lift_controls(ui, avatar);
                    if avatar.interior_deck == 1 {
                        ui.small(format!("Return to airlock · {:.1} m", distance));
                    } else {
                        ui.small("Airlock exit is on deck 1. Return via the access-core lift.");
                    }
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
            if !primary_promoted {
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
            }
            if let Some(fleet) = fleet.as_ref().filter(|_| !self.embedded || engineering) {
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
            ui.small(format!(
                "W/S or Up/Down move · A/D or Left/Right turn · Q/E strafe · {} · Space jump",
                self.on_foot_look_hint()
            ));
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
                        self.can_operate() && self.departure_order_reason().is_none(),
                        egui::Button::new("Undock active vessel"),
                    )
                    .clicked()
                {
                    self.send(Request::Undock);
                }
            });
            if let Some(reason) = self.departure_order_reason() {
                ui.colored_label(AMBER, reason);
            }
            ui.small("Drag with RMB to inspect your ship · wheel to zoom");
            if engineering && pilot.ship_class == ShipClass::Pioneer {
                self.pioneer_room_controls(ui, true);
            }
        }
    }

    fn interior_exit_action(
        &mut self,
        ui: &mut egui::Ui,
        avatar: &StationAvatar,
        interior_available: bool,
        distance: f64,
    ) {
        let door_open = self.airlock_door_open();
        let enabled = self.can_operate()
            && interior_available
            && door_open
            && distance <= STATION_BOARD_RANGE_M
            && avatar.interior_deck == 1
            && avatar.jump_started_ms.is_none();
        if ui
            .add_enabled(enabled, egui::Button::new("Exit airlock to hangar"))
            .clicked()
        {
            self.send(Request::ExitInterior);
        }
        if avatar.interior_deck == 1 && !door_open {
            self.open_airlock_control(ui, false);
        }
        if !self.connected {
            ui.small("Reconnect to leave the ship.");
        } else if self.request_pending {
            ui.small("Waiting for the server to confirm the current action.");
        } else if self.embedded_paused {
            ui.small("Simulation paused · resume to move or use the airlock.");
        } else if avatar.interior_deck != 1 {
            ui.small("The airlock is on deck 1. Use the access-core lift to return.");
        } else if !door_open {
            ui.small("The server reports the airlock closed. Open it before exiting.");
        } else if avatar.jump_started_ms.is_some() {
            ui.small("Wait until you land before using the airlock.");
        } else if !interior_available {
            ui.small("The server has disabled Pioneer interior exits.");
        } else if distance > STATION_BOARD_RANGE_M {
            ui.small(format!(
                "Walk to the deck 1 airlock · {:.1} m away. W/S move, A/D turn, Q/E strafe.",
                distance
            ));
        }
    }

    fn airlock_door_open(&self) -> bool {
        // Older servers omit room state and allow the historical direct exit path.
        self.assets
            .as_ref()
            .and_then(|assets| assets.pioneer_room.as_ref())
            .is_none_or(|room| room.door_open)
    }

    fn open_airlock_control(&mut self, ui: &mut egui::Ui, small: bool) {
        let has_room_state = self
            .assets
            .as_ref()
            .and_then(|assets| assets.pioneer_room.as_ref())
            .is_some();
        if !has_room_state {
            return;
        }
        let closed = !self.airlock_door_open();
        if !closed {
            return;
        }
        let in_control_range = self
            .own()
            .as_ref()
            .is_some_and(crate::room_controls::panel_access);
        let enabled = self.can_operate() && in_control_range;
        let button = egui::Button::new("Open airlock manually");
        let button = if small { button.small() } else { button };
        if ui.add_enabled(enabled, button).clicked() {
            self.send(Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: true,
                    manual: true,
                },
            });
        }
        if !self.connected {
            ui.small("Reconnect to operate the airlock.");
        } else if self.request_pending {
            ui.small("Waiting for the server to confirm the current action.");
        } else if self.embedded_paused {
            ui.small("Simulation paused · resume before opening the airlock.");
        } else if !in_control_range {
            ui.small("Move within 3 m of airlock controls · W/S move, A/D turn, Q/E strafe.");
        } else if let Some(reason) = self
            .assets
            .as_ref()
            .and_then(|assets| assets.pioneer_room.as_ref())
            .and_then(|room| room.interlock.as_deref())
        {
            ui.small(format!("Airlock interlock: {reason}"));
        } else {
            ui.small("Manual actuation still obeys the server pressure interlock.");
        }
    }

    /// Retained legacy interiors use the existing authoritative deck-lift request.
    /// Never present this route to a registered continuous-traversal avatar.
    fn legacy_interior_lift_controls(&mut self, ui: &mut egui::Ui, avatar: &StationAvatar) {
        if !avatar.onboard_interior || avatar.docked_traversal.is_some() {
            return;
        }
        let distance = avatar
            .position
            .iter()
            .zip(PIONEER_LIFT_POINT)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        let supported = self.assets.as_ref().is_some_and(|assets| {
            assets.pioneer_interior_available && assets.pioneer_interior_decks >= 2
        });
        let blocked = if !self.connected {
            Some("Reconnect to use the interior lift")
        } else if self.embedded_paused {
            Some("Resume simulation to use the interior lift")
        } else if self.request_pending {
            Some("Waiting for command confirmation")
        } else if !supported {
            Some("Two-deck interior lift unavailable on this server")
        } else if self.own().is_none_or(|pilot| {
            pilot.location != Location::Docked || pilot.ship_class != ShipClass::Pioneer
        }) {
            Some("Dock the Pioneer to use this interior lift")
        } else if avatar.seated {
            Some("Stand up before using the interior lift")
        } else if avatar.jump_started_ms.is_some() {
            Some("Land on the access-core lift landing first")
        } else if !distance.is_finite() || distance > PIONEER_LIFT_RANGE {
            Some("Move onto the marked access-core lift landing")
        } else if !(1..=2).contains(&avatar.interior_deck) {
            Some("Interior deck state unavailable")
        } else {
            None
        };
        let next_deck = if avatar.interior_deck == 1 { 2 } else { 1 };
        if ui
            .add_enabled(
                blocked.is_none(),
                egui::Button::new(format!("Use lift to deck {next_deck}")),
            )
            .clicked()
        {
            self.send(Request::InteriorLift { deck: next_deck });
        }
        ui.small(format!("Access-core lift · {distance:.1} m"));
        if let Some(reason) = blocked {
            ui.small(reason);
        }
    }

    fn interior_exit_prompt(&mut self, ui: &mut egui::Ui, avatar: &StationAvatar) {
        let distance = avatar
            .position
            .iter()
            .zip(PIONEER_INTERIOR_ENTRY)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        let interior_available = self
            .assets
            .as_ref()
            .is_some_and(|assets| assets.pioneer_interior_available);
        let door_open = self.airlock_door_open();
        let in_control_range = self
            .own()
            .as_ref()
            .is_some_and(crate::room_controls::panel_access);
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("AIRLOCK · DECK {}", avatar.interior_deck));
            self.legacy_interior_lift_controls(ui, avatar);
            if avatar.interior_deck == 1 {
                if door_open {
                    let exit_enabled = self.can_operate()
                        && interior_available
                        && distance <= STATION_BOARD_RANGE_M
                        && avatar.jump_started_ms.is_none();
                    if ui
                        .add_enabled(
                            exit_enabled,
                            egui::Button::new("Exit to hangar")
                                .min_size(Vec2::new(118.0, 26.0)),
                        )
                        .clicked()
                    {
                        self.send(Request::ExitInterior);
                    }
                } else {
                    let open_enabled = self.can_operate()
                        && in_control_range
                        && avatar.jump_started_ms.is_none();
                    if ui
                        .add_enabled(
                            open_enabled,
                            egui::Button::new("Open airlock manually")
                                .min_size(Vec2::new(152.0, 26.0)),
                        )
                        .clicked()
                    {
                        self.send(Request::SetPioneerRoom {
                            action: PioneerRoomCommand::Door {
                                open: true,
                                manual: true,
                            },
                        });
                    }
                }
                let legacy_room_state = self
                    .assets
                    .as_ref()
                    .is_none_or(|assets| assets.pioneer_room.is_none());
                ui.small(if door_open && legacy_room_state {
                    format!("Legacy open · {distance:.1} m")
                } else if door_open {
                    format!("Airlock · {distance:.1} m")
                } else {
                    "AIRLOCK CLOSED".into()
                })
                .on_hover_text(
                    "No Pioneer room-door state is available; the server preserves the legacy exit behavior.",
                );
            }
        });
        if !self.connected {
            ui.small("Reconnect to leave the ship.");
        } else if self.request_pending {
            ui.small("Waiting for server confirmation.");
        } else if self.embedded_paused {
            ui.small("Simulation paused · resume to move or use the airlock.");
        } else if avatar.interior_deck == 1 && !door_open && !in_control_range {
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Airlock closed · move within 3 m of its controls.")
                        .small(),
                )
                .wrap(),
            );
        } else if avatar.interior_deck == 1 && !door_open {
            let reason = self
                .assets
                .as_ref()
                .and_then(|assets| assets.pioneer_room.as_ref())
                .and_then(|room| room.interlock.as_deref());
            ui.small(reason.map_or_else(
                || "Open the airlock manually; pressure safety remains server controlled.".into(),
                |reason| format!("Airlock interlock: {reason}"),
            ));
        } else if avatar.interior_deck == 1 && distance > STATION_BOARD_RANGE_M {
            ui.small(format!(
                "Walk to the deck 1 airlock · {:.1} m away. W/S move, A/D turn, Q/E strafe.",
                distance
            ));
        } else if avatar.interior_deck != 1 {
            ui.small("The exit is on deck 1. W/S move, A/D turn, Q/E strafe; use the lift here.");
        } else if avatar.jump_started_ms.is_some() {
            ui.small("Wait until you land before using the airlock.");
        } else if !interior_available {
            ui.small("The server has disabled Pioneer interior exits.");
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "W/S move · A/D turn · Q/E strafe · {} · Space jump",
                    self.on_foot_look_hint()
                ))
                .small(),
            )
            .wrap(),
        );
    }
    fn hangar_boarding_prompt(
        &mut self,
        ui: &mut egui::Ui,
        pilot: &PilotView,
        avatar: &StationAvatar,
    ) {
        let point = self
            .assets
            .as_ref()
            .and_then(|assets| assets.hangar.as_ref())
            .and_then(|fleet| fleet.berths.iter().find(|berth| berth.active))
            .map_or(hangar_layout(pilot.ship_class).boarding_point, |berth| {
                berth.boarding_point
            });
        let distance = avatar
            .position
            .iter()
            .zip(point)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f64>()
            .sqrt();
        let access = self.can_operate()
            && distance <= STATION_BOARD_RANGE_M
            && avatar.jump_started_ms.is_none()
            && !avatar.seated;
        let interior_available = self
            .assets
            .as_ref()
            .is_some_and(|assets| assets.pioneer_interior_available);
        ui.horizontal_wrapped(|ui| {
            ui.strong("SHIP ACCESS");
            if ui
                .add_enabled(
                    access,
                    egui::Button::new("Enter ship").min_size(Vec2::new(84.0, 26.0)),
                )
                .clicked()
            {
                self.send(Request::EnterShip);
            }
            if pilot.ship_class == ShipClass::Pioneer
                && ui
                    .add_enabled(
                        access && interior_available,
                        egui::Button::new("Explore interior").min_size(Vec2::new(112.0, 26.0)),
                    )
                    .clicked()
            {
                self.send(Request::EnterInterior);
            }
            ui.small(format!("Boarding · {distance:.1} m"));
        });
        let reason = if !self.connected {
            Some("Reconnect to board your ship.")
        } else if self.request_pending {
            Some("Waiting for server confirmation.")
        } else if self.embedded_paused {
            Some("Simulation paused · resume to board or explore.")
        } else if avatar.seated {
            Some("Stand up before returning to the boarding marker.")
        } else if avatar.jump_started_ms.is_some() {
            Some("Land before boarding your ship.")
        } else if distance > STATION_BOARD_RANGE_M {
            Some("Return within 3 m of the illuminated boarding marker.")
        } else {
            None
        };
        if let Some(reason) = reason {
            ui.add(egui::Label::new(egui::RichText::new(reason).small()).wrap());
        }
        ui.add(
            egui::Label::new(
                egui::RichText::new(format!(
                    "W/S move · A/D turn · Q/E strafe · {} · Space jump",
                    self.on_foot_look_hint()
                ))
                .small(),
            )
            .wrap(),
        );
    }

    pub(super) fn boarding_control_block_reason(
        &self,
        state: &DockedTraversalState,
    ) -> Option<&'static str> {
        if !self.connected {
            Some("Reconnect to operate boarding equipment")
        } else if self.embedded_paused {
            Some("Resume simulation to operate boarding equipment")
        } else if self.snapshot.as_ref().is_none_or(|snapshot| {
            self.presentation_time()
                .saturating_sub(snapshot.server_time_ms)
                > 1500
        }) {
            Some("Waiting for a fresh world update")
        } else if self.request_pending {
            Some("Waiting for command confirmation")
        } else if self
            .own()
            .is_none_or(|pilot| pilot.location != Location::Docked)
        {
            Some("Exterior access locked in space")
        } else if self.snapshot.as_ref().is_some_and(|snapshot| {
            self.own().is_some_and(|pilot| {
                snapshot
                    .resource_jobs
                    .iter()
                    .any(|job| job.pilot == pilot.name && !job.phase.is_terminal())
            })
        }) {
            Some("Mining routine owns the ship · boarding equipment locked")
        } else if !state.grounded || state.seat_transition.is_some() {
            Some("Stand at the boarding control before operating it")
        } else if !state.actuators.service_power_available {
            Some("Docking service power unavailable · equipment held safely")
        } else {
            None
        }
    }

    pub(super) fn boarding_hatch_open_intent(state: &DockedTraversalState, index: usize) -> bool {
        state.actuators.hatches.as_ref().map_or(
            state.actuators.hatch_fractions()[index] < 1.0,
            |hatches| {
                if hatches[index].blocked {
                    hatches[index].target_open
                } else {
                    !hatches[index].target_open
                }
            },
        )
    }

    pub(super) fn boarding_hatch_block_reason(
        &self,
        state: &DockedTraversalState,
        hatch: DockedHatch,
        next_open: bool,
    ) -> Option<&'static str> {
        let Ok(pose) = docked_binding_pose(&state.binding) else {
            return Some("Hatch geometry unavailable · reconnect");
        };
        let (index, geometry_hatch) = match hatch {
            DockedHatch::Inner => (0, PioneerDockedHatch::Inner),
            DockedHatch::Outer => (1, PioneerDockedHatch::Outer),
        };
        let fraction = state.actuators.hatch_fractions()[index];
        let sweep = hatch_leaf_sweep_bounds(geometry_hatch, fraction, f64::from(next_open));
        let sweep_occupied = sweep.as_ref().is_ok_and(|bounds| {
            self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .pilots
                    .iter()
                    .filter_map(|pilot| pilot.station_avatar.as_ref()?.docked_traversal.as_ref())
                    .filter(|occupant| {
                        occupant.binding.host_vessel_id == state.binding.host_vessel_id
                    })
                    .any(|occupant| {
                        let feet = station_to_body(pose, occupant.feet_m);
                        bounds.iter().any(|bounds| {
                            box_intersects_capsule(
                                *bounds,
                                feet,
                                AVATAR_HEIGHT_M,
                                STATION_AVATAR_RADIUS_M,
                            )
                        })
                    })
            })
        });
        let doorway_occupied = !next_open
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                snapshot
                    .pilots
                    .iter()
                    .filter_map(|pilot| pilot.station_avatar.as_ref()?.docked_traversal.as_ref())
                    .filter(|occupant| {
                        occupant.binding.host_vessel_id == state.binding.host_vessel_id
                    })
                    .any(|occupant| hatch_occupied(pose, geometry_hatch, occupant.feet_m))
            });
        let other = 1 - index;
        self.boarding_control_block_reason(state).or_else(|| {
            if sweep.is_err() {
                Some("Hatch geometry unavailable · reconnect")
            } else if next_open && !state.actuators.hatch_closed(other) {
                Some("Close the other hatch first")
            } else if next_open
                && hatch == DockedHatch::Outer
                && (state.actuators.lift.moving
                    || (state.actuators.lift.floor_station_m - lift_geometry(pose).ship_floor_m)
                        .abs()
                        > 0.015)
            {
                Some("Call and stop the lift at ship level first")
            } else if doorway_occupied {
                Some("Step clear of the hatch doorway")
            } else if sweep_occupied {
                Some("Step clear of the hatch leaf sweep")
            } else {
                None
            }
        })
    }

    pub(super) fn boarding_lift_block_reason(
        &self,
        state: &DockedTraversalState,
    ) -> Option<&'static str> {
        self.boarding_control_block_reason(state).or_else(|| {
            if !state.actuators.hatches_closed() {
                Some("Close both boarding hatches before moving the lift")
            } else if state.actuators.lift.moving {
                Some("Boarding lift moving · remain on the platform")
            } else {
                None
            }
        })
    }

    /// Primary and detailed panels submit the same bound commands. These are
    /// snapshot interlocks, not local door animation or pressure simulation.
    fn boarding_hatch_controls(&mut self, ui: &mut egui::Ui, state: &DockedTraversalState) {
        if self
            .own()
            .is_none_or(|pilot| pilot.location != Location::Docked)
        {
            return;
        }
        let Ok(pose) = docked_binding_pose(&state.binding) else {
            return;
        };
        let anchors = hatch_anchors(pose);
        let fractions = state.actuators.hatch_fractions();
        let common_block = self.boarding_control_block_reason(state);
        let mut nearest = f64::INFINITY;
        let mut visible = false;
        for (hatch, index, name, inside, outside) in [
            (
                DockedHatch::Inner,
                0,
                "inner hatch",
                anchors.inner_inside_station_m,
                anchors.inner_outside_station_m,
            ),
            (
                DockedHatch::Outer,
                1,
                "outer hatch",
                anchors.outer_inside_station_m,
                anchors.outer_outside_station_m,
            ),
        ] {
            let distance =
                station_distance(state.feet_m, inside).min(station_distance(state.feet_m, outside));
            nearest = nearest.min(distance);
            if distance > 3.0 {
                continue;
            }
            visible = true;
            let fraction = fractions[index];
            let motion = state
                .actuators
                .hatches
                .as_ref()
                .map(|hatches| &hatches[index]);
            let next_open = Self::boarding_hatch_open_intent(state, index);
            let status = if motion.is_some_and(|motion| motion.blocked) {
                format!(
                    "Held {:.0}% → {}",
                    fraction * 100.0,
                    if motion.unwrap().target_open {
                        "open"
                    } else {
                        "closed"
                    }
                )
            } else if let Some(motion) = motion.filter(|motion| motion.moving) {
                format!(
                    "{} {:.0}%",
                    if motion.target_open {
                        "Opening"
                    } else {
                        "Closing"
                    },
                    fraction * 100.0
                )
            } else if fraction == 0.0 {
                "Closed".into()
            } else if fraction == 1.0 {
                "Open".into()
            } else {
                format!("Stopped {:.0}%", fraction * 100.0)
            };
            let blocked = self.boarding_hatch_block_reason(state, hatch, next_open);
            let purpose = if hatch == DockedHatch::Inner {
                "Cabin ↔ vestibule"
            } else {
                "Vestibule ↔ lift"
            };
            ui.small(format!("{purpose} · {status}"));
            let clicked = ui.push_id(("boarding_hatch", index), |ui| {
                ui.add_enabled(blocked.is_none(), egui::Button::new(format!("{} {name}", if next_open { "Open" } else { "Close" })))
                    .on_hover_text(format!("{name}: {purpose}; control {distance:.1} m away. Server-confirmed motion and interlocks."))
                    .clicked()
            }).inner;
            if clicked {
                self.send(Request::DockedTraversal {
                    request: DockedTraversalRequest::SetHatch {
                        binding: state.binding.clone(),
                        hatch,
                        open: next_open,
                    },
                });
            }
            if common_block.is_none() {
                if let Some(reason) = blocked {
                    ui.small(format!("{name}: {reason}"));
                }
            }
        }
        if visible {
            if let Some(reason) = common_block {
                ui.small(reason);
            }
        }
        if !visible
            && nearest.is_finite()
            && matches!(
                state.support,
                DockedSupport::Connector | DockedSupport::BoardingLift
            )
        {
            ui.small(format!(
                "Hatch control {nearest:.1} m away · walk within 3 m"
            ));
        }
    }

    /// The platform's real commands stay reachable without opening route details.
    /// Both presentations share authority gates; the server owns all movement.
    fn boarding_lift_controls(&mut self, ui: &mut egui::Ui, state: &DockedTraversalState) {
        if self
            .own()
            .is_none_or(|pilot| pilot.location != Location::Docked)
        {
            return;
        }
        let Ok(pose) = docked_binding_pose(&state.binding) else {
            return;
        };
        let calls = lift_call_anchors(pose);
        let nearest = station_distance(state.feet_m, calls.ship_station_m)
            .min(station_distance(state.feet_m, calls.station_station_m));
        if state.support != DockedSupport::BoardingLift && nearest > 3.0 {
            if nearest <= 12.0
                && matches!(
                    state.support,
                    DockedSupport::Station | DockedSupport::Connector
                )
            {
                ui.small(format!(
                    "Boarding-lift control · {nearest:.1} m away · walk within 3 m"
                ));
            }
            return;
        }
        let geometry = lift_geometry(pose);
        let lift = &state.actuators.lift;
        let target = match lift.target {
            DockedLiftStop::Ship => "ship",
            DockedLiftStop::Station => "station",
        };
        let at_ship = (lift.floor_station_m - geometry.ship_floor_m).abs() <= 1.0e-8;
        let at_station = (lift.floor_station_m - geometry.station_floor_m).abs() <= 1.0e-8;
        let status = if lift.moving {
            format!("Boarding lift moving to {target}")
        } else if at_ship {
            format!("Boarding lift at ship · target {target}")
        } else if at_station {
            format!("Boarding lift at station · target {target}")
        } else {
            format!("Boarding lift stopped between landings · target {target}")
        };
        ui.small(status);
        let blocked = self.boarding_lift_block_reason(state);
        for (stop, label, height) in [
            (DockedLiftStop::Ship, "Lift to ship", geometry.ship_floor_m),
            (
                DockedLiftStop::Station,
                "Lift to station",
                geometry.station_floor_m,
            ),
        ] {
            let at_stop = (state.actuators.lift.floor_station_m - height).abs() <= 1.0e-8;
            let response = ui.add_enabled(blocked.is_none() && !at_stop, egui::Button::new(label));
            let response = if at_stop && !state.actuators.lift.moving {
                response.on_disabled_hover_text(format!(
                    "Already at {} level",
                    match stop {
                        DockedLiftStop::Ship => "ship",
                        DockedLiftStop::Station => "station",
                    }
                ))
            } else {
                response
            };
            if response.clicked() {
                self.send(Request::DockedTraversal {
                    request: DockedTraversalRequest::CallLift {
                        binding: state.binding.clone(),
                        stop,
                    },
                });
            }
        }
        if let Some(reason) = blocked {
            ui.small(reason);
        }
    }

    fn docked_interaction_strip(&mut self, ui: &mut egui::Ui) {
        // Exactly one visible gesture owner, including first person. Never put
        // this prompt/input step inside the optional Help disclosure.
        self.nearby_interaction_prompt(ui);
        #[cfg(test)]
        ui.ctx().data_mut(|data| {
            data.insert_temp(
                egui::Id::new("ship_interaction_primary_prompt_bounds"),
                (ui.min_rect(), ui.clip_rect()),
            )
        });
        let Some(pilot) = self.own() else {
            return;
        };
        let in_space = pilot.location != Location::Docked;
        let Some(binding) = self.verified_docked_binding() else {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(AMBER, "Interior registration unavailable");
                if !self.embedded {
                    self.flight_workspace_shortcuts(ui);
                }
            });
            return;
        };
        let active_job = self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .resource_jobs
                .iter()
                .any(|job| job.pilot == pilot.name && !job.phase.is_terminal())
        });
        let state = pilot
            .station_avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref());
        let fresh = self.traversal_snapshot_fresh();
        let transition_block = self.traversal_transition_block_reason();
        let ready = transition_block.is_none();
        let routine_status = if !self.connected {
            "Disconnected · last known mining routine"
        } else if self.embedded_paused {
            "Mining routine paused"
        } else if !fresh {
            "Mining routine status stale"
        } else {
            "Mining autopilot active"
        };
        let compact_help = self.first_person_hud_active() && state.is_some();
        let help_id = egui::Id::new("registered_body_movement_help");
        let mut help_open = ui
            .ctx()
            .data_mut(|data| data.get_persisted::<bool>(help_id).unwrap_or(false));
        ui.horizontal_wrapped(|ui| {
            if let Some(state) = state {
                ui.strong(docked_state_label(state));
                if !in_space {
                    self.boarding_hatch_controls(ui, state);
                    self.boarding_lift_controls(ui, state);
                }
                if let Ok(pose) = docked_binding_pose(&binding) {
                    let distance =
                        station_distance(state.feet_m, seat_anchors(pose).root_station_m);
                    let seat_block = self.docked_seat_block_reason(state);
                    if ui
                        .add_enabled(
                            seat_block.is_none(),
                            egui::Button::new("Return to pilot seat"),
                        )
                        .on_hover_text(format!(
                            "Pilot chair: {distance:.1} m. {}",
                            seat_block.unwrap_or("Ready to return to the pilot seat")
                        ))
                        .clicked()
                    {
                        self.send(Request::DockedTraversal {
                            request: DockedTraversalRequest::TakeSeat {
                                binding: binding.clone(),
                            },
                        });
                    }
                    ui.small(format!("Pilot chair · {distance:.1} m"));
                    if transition_block.is_none() {
                        if let Some(reason) = seat_block {
                            ui.small(reason);
                        }
                    }
                }
                if state.seat_transition.is_some() {
                    ui.small("Seat transition");
                }
                if in_space {
                    ui.small("Exterior access locked");
                } else if let Some(room) = self.docked_room_name(state) {
                    ui.small(room);
                }
            } else {
                ui.strong("PILOT SEAT");
                if transition_block.is_none() {
                    if let Some(reason) = self.docked_leave_seat_block_reason() {
                        ui.small(reason);
                    }
                }
                if ui
                    .add_enabled(
                        self.docked_leave_seat_block_reason().is_none(),
                        egui::Button::new("Leave pilot seat"),
                    )
                    .clicked()
                {
                    self.send(Request::DockedTraversal {
                        request: DockedTraversalRequest::LeaveSeat {
                            binding: binding.clone(),
                        },
                    });
                }
                if !in_space
                    && ui
                        .add_enabled(
                            ready && self.departure_order_reason().is_none(),
                            egui::Button::new("Undock"),
                        )
                        .clicked()
                {
                    self.send(Request::Undock);
                }
                if in_space {
                    ui.small("Walk inside · exterior access locked");
                }
            }
            if in_space && state.is_some() {
                ui.small(self.space_walk_routine_status());
            } else if active_job {
                ui.small(routine_status);
            }
            if compact_help {
                ui.toggle_value(&mut help_open, "Help")
                    .on_hover_text("Movement, nearby controls and route");
            }
        });
        if !in_space && state.is_none() {
            if let Some(reason) = self.departure_order_reason() {
                ui.colored_label(AMBER, reason);
            }
        }
        if compact_help {
            ui.ctx()
                .data_mut(|data| data.insert_persisted(help_id, help_open));
        }
        if let Some(reason) = transition_block {
            ui.small(reason);
        }
        if state.is_some() {
            let movement_help = if self.snapshot.as_ref().is_some_and(|s| s.walk_sprint_v1) {
                format!(
                    "W/S move · Q/E strafe · {} · Shift sprint · Space jump",
                    self.on_foot_look_hint()
                )
            } else {
                format!(
                    "W/S move · Q/E strafe · {} · Space jump",
                    self.on_foot_look_hint()
                )
            };
            if compact_help {
                if help_open {
                    ui.group(|ui| {
                        ui.small(movement_help);
                        if !in_space {
                            self.docked_route_details(ui, fresh);
                        }
                    });
                }
            } else {
                ui.small(movement_help);
                if !in_space {
                    ui.collapsing("Nearby controls and route", |ui| {
                        self.docked_route_details(ui, fresh)
                    });
                }
            }
        }
    }

    fn docked_route_details(&mut self, ui: &mut egui::Ui, fresh: bool) {
        ui.small("Route: cabin → inner hatch → vestibule → outer hatch → lift. Close the other hatch before opening one.");
        egui::ScrollArea::vertical()
            .id_salt("nearby_ship_controls_scroll")
            .max_height(96.0)
            .min_scrolled_height(0.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.add_enabled_ui(fresh, |ui| self.docked_station_actions(ui));
            });
    }

    pub(super) fn station_overlay(&mut self, ctx: &egui::Context) {
        #[cfg(test)]
        ctx.data_mut(|data| {
            data.remove::<egui::Rect>(egui::Id::new("ship_interaction_toggle_rect"));
            data.remove::<egui::Id>(egui::Id::new("ship_interaction_toggle_id"));
            data.remove::<(egui::Rect, egui::Rect)>(egui::Id::new(
                "ship_interaction_primary_prompt_bounds",
            ));
        });
        if self.docked_navigation_present() {
            let seated = self
                .own()
                .is_some_and(|pilot| pilot.station_avatar.is_none());
            if self.embedded || seated {
                // Reserve space instead of covering the physical captain controls.
                // Desktop seated access keeps its existing collapsible state.
                let bounds = ctx.available_rect();
                let response = egui::TopBottomPanel::bottom("embedded_docked_traversal_actions")
                    .height_range(34.0..=220.0)
                    .resizable(false)
                    .frame(self.overlay_frame(ctx))
                    .show(ctx, |ui| {
                        if self.embedded {
                            self.docked_interaction_strip(ui);
                        } else {
                            let mut label_clicked = false;
                            let mut header =
                                egui::collapsing_header::CollapsingState::load_with_default_open(
                                    ctx,
                                    egui::Id::new("docked_traversal_actions").with("collapsing"),
                                    true,
                                )
                                .show_header(ui, |ui| {
                                    // A full-size, keyboard-accessible header target;
                                    // the arrow and label operate the same saved state.
                                    let response = ui.button("Ship interaction");
                                    label_clicked = response.clicked();
                                    #[cfg(test)]
                                    ctx.data_mut(|data| {
                                        data.insert_temp(
                                            egui::Id::new("ship_interaction_toggle_rect"),
                                            response.rect,
                                        );
                                        data.insert_temp(
                                            egui::Id::new("ship_interaction_toggle_id"),
                                            response.id,
                                        );
                                    });
                                });
                            if label_clicked {
                                header.toggle();
                            }
                            header.body(|ui| self.docked_interaction_strip(ui));
                        }
                    });
                #[cfg(test)]
                ctx.data_mut(|data| {
                    data.insert_temp(
                        egui::Id::new("ship_interaction_window_rect"),
                        response.response.rect,
                    );
                    data.insert_temp(egui::Id::new("ship_interaction_bounds"), bounds);
                });
                #[cfg(not(test))]
                let _ = (bounds, response);
            } else {
                // Keep the primary actions reachable above the authority footer.
                // On foot, use the upper-left side without a second center card.
                let on_foot = self
                    .own()
                    .is_some_and(|pilot| pilot.station_avatar.is_some());
                let (anchor, offset) = if on_foot {
                    (egui::Align2::LEFT_TOP, [14.0, 80.0])
                } else {
                    (egui::Align2::CENTER_BOTTOM, [0.0, -14.0])
                };
                if self.first_person_hud_active() {
                    // A saved third-person collapse must not hide the sole F
                    // prompt when switching into first-person body control.
                    let mut state =
                        egui::collapsing_header::CollapsingState::load_with_default_open(
                            ctx,
                            egui::Id::new("docked_traversal_actions").with("collapsing"),
                            true,
                        );
                    state.set_open(true);
                    state.store(ctx);
                }
                let _response = egui::Window::new("Ship interaction")
                    .id(egui::Id::new("docked_traversal_actions"))
                    .anchor(anchor, offset)
                    .constrain_to(ctx.available_rect())
                    .resizable(false)
                    .collapsible(!self.first_person_hud_active())
                    .movable(false)
                    .frame(self.overlay_frame(ctx))
                    .show(ctx, |ui| {
                        self.docked_interaction_strip(ui);
                    });
                #[cfg(test)]
                if let Some(response) = _response {
                    let bounds = ctx.available_rect();
                    ctx.data_mut(|data| {
                        data.insert_temp(
                            egui::Id::new("ship_interaction_window_rect"),
                            response.response.rect,
                        );
                        data.insert_temp(egui::Id::new("ship_interaction_bounds"), bounds);
                    });
                }
            }
            return;
        }
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
                                        self.can_operate()
                                            && self.departure_order_reason().is_none(),
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
                            if let Some(reason) = self.departure_order_reason() {
                                ui.colored_label(AMBER, reason);
                            }
                            self.power_gauge(ui);
                            return;
                        }
                    }

                    let on_foot_pilot = self.own().filter(|pilot| {
                        pilot.location == Location::Docked && pilot.station_avatar.is_some()
                    });
                    if let Some(pilot) = &on_foot_pilot {
                        let avatar = pilot
                            .station_avatar
                            .as_ref()
                            .expect("on-foot pilot filtered");
                        if avatar.onboard_interior {
                            self.interior_exit_prompt(ui, avatar);
                        } else {
                            self.hangar_boarding_prompt(ui, pilot, avatar);
                        }
                    }

                    egui::ScrollArea::vertical()
                        .id_salt("embedded_station_action_scroll")
                        .auto_shrink([false, true])
                        .min_scrolled_height(0.0)
                        .max_height(112.0)
                        .show(ui, |ui| {
                            self.station_actions_inner(ui, false, on_foot_pilot.is_some());
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
            let interior_avatar = self
                .own()
                .filter(|pilot| pilot.location == Location::Docked)
                .and_then(|pilot| pilot.station_avatar.clone())
                .filter(|avatar| avatar.onboard_interior);
            if let Some(avatar) = &interior_avatar {
                self.interior_exit_prompt(ui, avatar);
            }
            self.station_actions_inner(ui, false, interior_avatar.is_some());
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
        let state = avatar
            .as_ref()
            .and_then(|avatar| avatar.docked_traversal.as_ref());
        let binding = state.and_then(|_| self.verified_docked_binding());
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
            && self.manual_remote.is_none()
            && self.can_operate()
            && fresh
            && (state.is_none()
                || binding.is_some()
                    && over_world
                    && state.is_some_and(|state| state.seat_transition.is_none()))
            && !ctx.wants_keyboard_input()
            && !self.cockpit_computer.wants_keyboard_input()
            && ctx.input(|i| {
                i.focused && !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.command
            });
        // A registration change never retargets a queued walking sample. Send
        // one stop to the original binding before accepting the new route.
        if self.station_walk_active && self.docked_walk_binding != binding {
            self.stop_station_walk();
            return;
        }
        let (forward, strafe, turn) = if allowed {
            ctx.input(|i| {
                // Shift changes walking speed on foot; arrow controls remain
                // movement. Seated head-look is sampled by the cockpit view.
                let down = |key, arrow| i.key_down(key) || i.key_down(arrow);
                let forward = f64::from(down(egui::Key::W, egui::Key::ArrowUp))
                    - f64::from(down(egui::Key::S, egui::Key::ArrowDown));
                let strafe =
                    f64::from(i.key_down(egui::Key::E)) - f64::from(i.key_down(egui::Key::Q));
                let key_turn = f64::from(down(egui::Key::D, egui::Key::ArrowRight))
                    - f64::from(down(egui::Key::A, egui::Key::ArrowLeft));
                if key_turn != 0.0 {
                    self.hangar_view.follow_walk_turn(key_turn, i.stable_dt);
                }
                let heading = state.map_or(avatar.as_ref().unwrap().heading, |state| state.heading);
                let turn = if key_turn != 0.0 {
                    key_turn
                } else {
                    self.hangar_view.walk_turn(heading)
                };
                (forward, strafe, turn)
            })
        } else {
            (0.0, 0.0, 0.0)
        };
        let jump = allowed
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
            });
        if jump {
            if let (Some(state), Some(binding)) = (state, binding.as_ref()) {
                if state.grounded
                    && state.seat_transition.is_none()
                    && !(state.support == DockedSupport::BoardingLift
                        && state.actuators.lift.moving)
                {
                    self.send(Request::DockedTraversal {
                        request: DockedTraversalRequest::Jump {
                            binding: binding.clone(),
                        },
                    });
                }
            } else if avatar
                .as_ref()
                .is_some_and(|avatar| avatar.jump_started_ms.is_none() && !avatar.seated)
            {
                self.send(Request::StationJump);
            }
        }
        let sprint = allowed
            && (forward != 0.0 || strafe != 0.0)
            && self
                .snapshot
                .as_ref()
                .is_some_and(|snapshot| snapshot.walk_sprint_v1)
            && ctx.input(|input| input.modifiers.shift);
        let active = forward != 0.0 || strafe != 0.0 || turn != 0.0;
        if active {
            if let Some(binding) = binding {
                self.controls
                    .publish_docked(binding.clone(), forward, strafe, turn, sprint);
                self.docked_walk_binding = Some(binding);
            } else {
                self.controls.publish_station(forward, strafe, turn, sprint);
                self.docked_walk_binding = None;
            }
        } else if self.station_walk_active {
            self.stop_station_walk();
        }
        self.station_walk_active = active;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docked_fixture(ctx: &egui::Context, walking: bool) -> Cockpit {
        let mut cockpit = crate::tests::view_test_cockpit(ctx);
        cockpit.embedded = true;
        let snapshot = cockpit.snapshot.as_mut().unwrap();
        snapshot.docked_traversal_capability = Some(docked_traversal_capability());
        snapshot.walk_sprint_v1 = true;
        snapshot.pilots[0].location = Location::Docked;
        snapshot.pilots[0].station_avatar = None;
        cockpit.assets = Some(AssetsView {
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: Some(PioneerRoomState::uncommissioned("owned-pioneer".into())),
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        let binding = cockpit.verified_docked_binding().unwrap();
        if walking {
            let pose = docked_binding_pose(&binding).unwrap();
            let seats = seat_anchors(pose);
            let state = DockedTraversalState {
                sprinting: false,
                version: DOCKED_TRAVERSAL_VERSION,
                binding: binding.clone(),
                feet_m: seats.root_station_m,
                heading: seats.heading,
                support: DockedSupport::Cockpit,
                surface_id: "cockpit".into(),
                moving: false,
                grounded: true,
                vertical_velocity_mps: 0.0,
                updated_at_ms: 1000,
                seat_transition: None,
                actuators: DockedVesselActuators {
                    hatches: None,
                    binding,
                    inner_hatch_open: false,
                    outer_hatch_open: false,
                    lift: DockedLiftState {
                        floor_station_m: lift_geometry(pose).ship_floor_m,
                        target: DockedLiftStop::Ship,
                        moving: false,
                        updated_at_ms: 1000,
                    },
                    service_power_available: true,
                },
            };
            cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(StationAvatar {
                sprinting: false,
                docked_traversal: Some(state),
                onboard_interior: false,
                interior_deck: 1,
                jump_started_ms: None,
                seated: false,
                position: [0.0; 3],
                heading: 0.0,
                moving: false,
                updated_at_ms: 1000,
            });
        }
        cockpit
    }

    fn transition_test_frame(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        frame: u32,
        events: Vec<egui::Event>,
        body: impl Fn(&mut Cockpit, &mut egui::Ui),
    ) -> Vec<(String, Pos2)> {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(1000., 700.),
                )),
                time: Some(frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| body(cockpit, ui));
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

    fn transition_click(point: Pos2, pressed: bool) -> Vec<egui::Event> {
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

    #[test]
    fn seated_reserved_interaction_header_keyboard_toggles_same_saved_state_without_gameplay() {
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.animation_time = 0.);
        let mut cockpit = docked_fixture(&ctx, false);
        cockpit.embedded = false;
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let mut frame = |events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(960., 640.))),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    cockpit.station_overlay(ctx);
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            );
        };
        for _ in 0..3 {
            frame(vec![]);
        }
        let state_id = egui::Id::new("docked_traversal_actions").with("collapsing");
        assert!(
            egui::collapsing_header::CollapsingState::load(&ctx, state_id)
                .unwrap()
                .is_open()
        );
        for expected_open in [false, true] {
            let target = ctx
                .data(|data| data.get_temp::<egui::Id>(egui::Id::new("ship_interaction_toggle_id")))
                .unwrap();
            ctx.memory_mut(|memory| memory.request_focus(target));
            for pressed in [true, false] {
                frame(vec![egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }]);
            }
            assert_eq!(
                egui::collapsing_header::CollapsingState::load(&ctx, state_id)
                    .unwrap()
                    .is_open(),
                expected_open
            );
            assert!(
                rx.try_recv().is_err(),
                "header activation must not dispatch a ship command"
            );
        }
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }

    #[test]
    fn registered_first_person_keeps_one_primary_prompt_with_optional_movement_help() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        cockpit.hangar_view.on_foot_view = crate::flight3d::hangar::OnFootView::FirstPerson;
        assert!(cockpit.first_person_hud_active());
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let draw = |cockpit: &mut Cockpit, ui: &mut egui::Ui| cockpit.docked_interaction_strip(ui);
        transition_test_frame(&ctx, &mut cockpit, 0, vec![], draw);
        let labels = transition_test_frame(&ctx, &mut cockpit, 1, vec![], draw);
        assert_eq!(
            labels
                .iter()
                .filter(|(text, _)| text.starts_with("F · "))
                .count(),
            1
        );
        assert_eq!(
            labels
                .iter()
                .filter(|(text, _)| text == "Return to pilot seat")
                .count(),
            1
        );
        assert!(
            !labels.iter().any(|(text, _)| text.contains("W/S move")),
            "Movement details are optional, not a second always-on card"
        );
        let help = labels.iter().find(|(text, _)| text == "Help").unwrap().1;
        transition_test_frame(&ctx, &mut cockpit, 2, transition_click(help, true), draw);
        transition_test_frame(&ctx, &mut cockpit, 3, transition_click(help, false), draw);
        let labels = transition_test_frame(&ctx, &mut cockpit, 4, vec![], draw);
        assert!(labels.iter().any(|(text, _)| text.contains("W/S move")));
        assert_eq!(
            labels
                .iter()
                .filter(|(text, _)| text.starts_with("F · "))
                .count(),
            1
        );
        assert!(
            rx.try_recv().is_err(),
            "Opening instructions cannot issue gameplay"
        );
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }

    #[test]
    fn registered_first_person_primary_f_release_is_once_and_retains_input_gates() {
        for scenario in ["ready", "stale", "focus", "modal"] {
            let ctx = egui::Context::default();
            ctx.style_mut(|style| style.animation_time = 0.);
            let mut cockpit = docked_fixture(&ctx, true);
            cockpit.embedded_input = true;
            cockpit.hangar_view.on_foot_view = crate::flight3d::hangar::OnFootView::FirstPerson;
            cockpit.hangar_view.set_head_look_input_allowed(true);
            let (tx, rx) = mpsc::channel();
            cockpit.commands = Some(tx);
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            for index in 0..6 {
                if index == 3 {
                    if scenario == "stale" {
                        cockpit.frozen_time = Some(5000);
                    }
                    if scenario == "modal" {
                        cockpit.controls_reference_open = true;
                    }
                }
                let mut events = vec![egui::Event::PointerMoved(Pos2::new(500., 220.))];
                if index == 3 || index == 4 {
                    events.push(egui::Event::Key {
                        key: egui::Key::F,
                        physical_key: None,
                        pressed: index == 3,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(960., 640.),
                        )),
                        time: Some(index as f64 / 60.),
                        focused: !(scenario == "focus" && index >= 3),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        cockpit.station_overlay(ctx);
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let pilot = cockpit.own().unwrap();
                            cockpit.hangar_view.set_docked_traversal_enabled(true);
                            cockpit.hangar_view.set_parent_managed_body_hud(true);
                            cockpit.hangar_view.ui(
                                ui,
                                &pilot,
                                1000,
                                true,
                                None,
                                cockpit
                                    .assets
                                    .as_ref()
                                    .and_then(|assets| assets.pioneer_room.as_ref()),
                                None,
                                1000,
                                None,
                            );
                        });
                        cockpit.first_person_hud(ctx);
                    },
                );
            }
            let count = rx
                .try_iter()
                .filter(|r| {
                    matches!(
                        r,
                        Request::DockedTraversal {
                            request: DockedTraversalRequest::TakeSeat { .. }
                        }
                    )
                })
                .count();
            assert_eq!(count, usize::from(scenario == "ready"), "{scenario}");
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        }
    }

    #[test]
    fn registered_first_person_reopens_saved_collapsed_desktop_primary_actions() {
        let ctx = egui::Context::default();
        ctx.style_mut(|style| style.animation_time = 0.);
        let mut cockpit = docked_fixture(&ctx, true);
        cockpit.embedded = false;
        let id = egui::Id::new("docked_traversal_actions").with("collapsing");
        let mut state =
            egui::collapsing_header::CollapsingState::load_with_default_open(&ctx, id, true);
        state.set_open(false);
        state.store(&ctx);
        cockpit.hangar_view.on_foot_view = crate::flight3d::hangar::OnFootView::FirstPerson;
        let mut labels = vec![];
        for _ in 0..3 {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(960., 640.))),
                    ..Default::default()
                },
                |ctx| {
                    cockpit.station_overlay(ctx);
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            );
            labels = output
                .shapes
                .into_iter()
                .filter_map(|s| match s.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley.text().to_owned(),
                        s.clip_rect
                            .contains_rect(egui::Rect::from_min_size(t.pos, t.galley.size())),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        }
        assert!(egui::collapsing_header::CollapsingState::load(&ctx, id)
            .unwrap()
            .is_open());
        assert_eq!(
            labels
                .iter()
                .filter(|(text, visible)| *visible && text.starts_with("F · "))
                .count(),
            1
        );
        assert!(labels
            .iter()
            .any(|(text, visible)| *visible && text == "Return to pilot seat"));
    }

    #[test]
    fn nearby_interaction_model_pins_bound_seat_intent_and_revalidates_stale_state() {
        for walking in [false, true] {
            let ctx = egui::Context::default();
            let mut cockpit = docked_fixture(&ctx, walking);
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            let (tx, rx) = mpsc::channel();
            cockpit.commands = Some(tx);
            let action = cockpit
                .nearby_ship_action()
                .expect("Known pilot chair is discoverable");
            assert!(action.blocked_reason.is_none());
            let binding = cockpit.verified_docked_binding().unwrap();
            if walking {
                assert!(
                    matches!(&action.request, DockedTraversalRequest::TakeSeat {binding: actual} if *actual == binding)
                );
            } else {
                assert!(
                    matches!(&action.request, DockedTraversalRequest::LeaveSeat {binding: actual} if *actual == binding)
                );
            }
            cockpit.frozen_time = Some(5000);
            let stale = cockpit.nearby_ship_action().unwrap();
            assert!(stale.request == action.request);
            assert!(stale.target_id == action.target_id);
            assert!(stale.blocked_reason.unwrap().contains("fresh world update"));
            assert!(rx.try_recv().is_err());
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        }
    }

    #[test]
    fn nearby_interaction_hatch_intent_is_explicit_and_ambiguity_suppresses_shortcut() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        let state = cockpit.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .docked_traversal
            .as_mut()
            .unwrap();
        assert!(Cockpit::boarding_hatch_open_intent(state, 0));
        state.actuators.inner_hatch_open = true;
        assert!(!Cockpit::boarding_hatch_open_intent(state, 0));
        let binding = state.binding.clone();
        let first = crate::nearby_action_model::NearbyAction {
            request: DockedTraversalRequest::SetHatch {
                binding: binding.clone(),
                hatch: DockedHatch::Inner,
                open: false,
            },
            label: "Close inner hatch".into(),
            blocked_reason: None,
            distance_m: 1.0,
            target_id: "inner/inside".into(),
        };
        let mut second = first.clone();
        second.target_id = "outer/inside".into();
        second.distance_m = 1.1;
        assert!(crate::nearby_action_model::unambiguous_action(vec![
            first.clone(),
            second.clone()
        ])
        .is_none());
        second.distance_m = 2.0;
        assert!(
            crate::nearby_action_model::unambiguous_action(vec![first.clone(), second])
                .unwrap()
                .request
                == first.request
        );
    }

    #[test]
    fn contextual_interactions_seat_stow_matches_authority_in_both_panels() {
        for primary in [false, true] {
            for (offset, permitted) in [(0.02, false), (0.014, true)] {
                let ctx = egui::Context::default();
                let mut cockpit = docked_fixture(&ctx, true);
                let state = cockpit.snapshot.as_mut().unwrap().pilots[0]
                    .station_avatar
                    .as_mut()
                    .unwrap()
                    .docked_traversal
                    .as_mut()
                    .unwrap();
                state.actuators.lift.floor_station_m += offset;
                let binding = state.binding.clone();
                let before = serde_json::to_value(&cockpit.snapshot).unwrap();
                let (tx, rx) = mpsc::channel();
                cockpit.commands = Some(tx);
                let draw = |cockpit: &mut Cockpit, ui: &mut egui::Ui| {
                    if primary {
                        cockpit.docked_interaction_strip(ui);
                    } else {
                        assert!(cockpit.docked_station_actions(ui));
                    }
                };
                transition_test_frame(&ctx, &mut cockpit, 0, vec![], draw);
                let labels = transition_test_frame(&ctx, &mut cockpit, 1, vec![], draw);
                assert!(labels
                    .iter()
                    .any(|(text, _)| text.contains("Pilot chair · 0.0 m")));
                assert_eq!(
                    labels
                        .iter()
                        .any(|(text, _)| text.contains("Stow the boarding lift")),
                    !permitted
                );
                let label = if primary {
                    "Return to pilot seat"
                } else {
                    "Take pilot seat"
                };
                let point = labels.iter().find(|(text, _)| text == label).unwrap().1;
                for (frame, pressed) in [true, false].into_iter().enumerate() {
                    transition_test_frame(
                        &ctx,
                        &mut cockpit,
                        2 + frame as u32,
                        transition_click(point, pressed),
                        draw,
                    );
                }
                if permitted {
                    assert!(
                        matches!(rx.try_recv(), Ok(Request::DockedTraversal {request: DockedTraversalRequest::TakeSeat {binding: actual}}) if actual == binding)
                    );
                }
                assert!(rx.try_recv().is_err());
                assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
            }
        }
    }

    #[test]
    fn contextual_interactions_seat_reasons_match_actual_equipment_gates() {
        for (scenario, expected) in [
            ("airborne", "Land before"),
            ("hatch", "Close both boarding hatches"),
            ("moving", "Wait for the boarding lift"),
            ("unstowed", "Stow the boarding lift"),
            ("distant", "Walk within 1 m"),
            ("power_off", ""),
        ] {
            let ctx = egui::Context::default();
            let mut cockpit = docked_fixture(&ctx, true);
            let state = cockpit.snapshot.as_mut().unwrap().pilots[0]
                .station_avatar
                .as_mut()
                .unwrap()
                .docked_traversal
                .as_mut()
                .unwrap();
            match scenario {
                "airborne" => state.grounded = false,
                "hatch" => state.actuators.inner_hatch_open = true,
                "moving" => state.actuators.lift.moving = true,
                "unstowed" => state.actuators.lift.floor_station_m -= 0.02,
                "distant" => state.feet_m[0] += 2.0,
                "power_off" => state.actuators.service_power_available = false,
                _ => unreachable!(),
            }
            let state = state.clone();
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            let reason = cockpit.docked_seat_block_reason(&state);
            if expected.is_empty() {
                assert!(
                    reason.is_none(),
                    "Seat authority does not require service power"
                );
            } else {
                assert!(reason.unwrap().contains(expected), "{scenario}: {reason:?}");
            }
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        }
    }

    #[test]
    fn contextual_interactions_lift_reports_landing_target_and_control_distance() {
        for scenario in ["at_ship", "at_station", "moving", "between", "nearby"] {
            let ctx = egui::Context::default();
            let mut cockpit = docked_fixture(&ctx, true);
            let state = cockpit.snapshot.as_mut().unwrap().pilots[0]
                .station_avatar
                .as_mut()
                .unwrap()
                .docked_traversal
                .as_mut()
                .unwrap();
            let pose = docked_binding_pose(&state.binding).unwrap();
            let geometry = lift_geometry(pose);
            state.support = DockedSupport::BoardingLift;
            state.feet_m = lift_call_anchors(pose).ship_station_m;
            state.actuators.lift.target = DockedLiftStop::Station;
            if scenario == "at_station" {
                state.actuators.lift.floor_station_m = geometry.station_floor_m;
            }
            if scenario == "moving" || scenario == "between" {
                state.actuators.lift.floor_station_m =
                    (geometry.ship_floor_m + geometry.station_floor_m) * 0.5;
                state.actuators.lift.moving = scenario == "moving";
            }
            if scenario == "nearby" {
                state.support = DockedSupport::Connector;
                state.feet_m[0] += 8.0;
            }
            let state = state.clone();
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            let labels = transition_test_frame(&ctx, &mut cockpit, 0, vec![], |cockpit, ui| {
                cockpit.boarding_lift_controls(ui, &state)
            });
            let expected = match scenario {
                "at_ship" => "Boarding lift at ship · target station",
                "at_station" => "Boarding lift at station · target station",
                "moving" => "Boarding lift moving to station",
                "between" => "Boarding lift stopped between landings · target station",
                "nearby" => "walk within 3 m",
                _ => unreachable!(),
            };
            assert!(
                labels.iter().any(|(text, _)| text.contains(expected)),
                "{scenario}: {labels:?}"
            );
            if scenario == "nearby" {
                assert!(!labels.iter().any(|(text, _)| text == "Lift to ship"));
            }
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        }
    }

    #[test]
    fn pioneer_completion_registered_detailed_and_primary_stale_transitions_send_no_commands() {
        for primary in [false, true] {
            for walking in [false, true] {
                let ctx = egui::Context::default();
                let mut cockpit = docked_fixture(&ctx, walking);
                let snapshot_time = cockpit.snapshot.as_ref().unwrap().server_time_ms;
                cockpit.frozen_time = None;
                cockpit.timeline_origin = Some(snapshot_time.saturating_add(2000));
                cockpit.observed = Instant::now();
                let before = serde_json::to_value(&cockpit.snapshot).unwrap();
                let (tx, rx) = mpsc::channel();
                cockpit.commands = Some(tx);
                let draw = |cockpit: &mut Cockpit, ui: &mut egui::Ui| {
                    if primary {
                        cockpit.docked_interaction_strip(ui);
                    } else {
                        assert!(cockpit.docked_station_actions(ui));
                    }
                };
                transition_test_frame(&ctx, &mut cockpit, 0, vec![], draw);
                let labels = transition_test_frame(&ctx, &mut cockpit, 1, vec![], draw);
                assert!(labels
                    .iter()
                    .any(|(text, _)| text.contains("fresh world update")));
                let actions: &[&str] = if walking {
                    if primary {
                        &["Return to pilot seat"]
                    } else {
                        &["Take pilot seat"]
                    }
                } else {
                    &["Leave pilot seat", "Undock"]
                };
                for (index, action) in actions.iter().enumerate() {
                    let point = labels
                        .iter()
                        .find(|(text, _)| text.as_str() == *action)
                        .expect(action)
                        .1;
                    for (offset, pressed) in [true, false].into_iter().enumerate() {
                        transition_test_frame(
                            &ctx,
                            &mut cockpit,
                            2 + index as u32 * 2 + offset as u32,
                            transition_click(point, pressed),
                            draw,
                        );
                    }
                }
                assert!(rx.try_recv().is_err(), "stale transition emitted a command");
                assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
            }
        }
    }

    #[test]
    fn pioneer_completion_legacy_body_camera_selection_returns_from_observation_without_moving_avatar(
    ) {
        use flight3d::hangar::OnFootView;
        for (view, label) in [
            (OnFootView::FirstPerson, "First person"),
            (OnFootView::ThirdPerson, "Third person"),
        ] {
            let ctx = egui::Context::default();
            let mut cockpit = docked_fixture(&ctx, true);
            cockpit.snapshot.as_mut().unwrap().pilots[0]
                .station_avatar
                .as_mut()
                .unwrap()
                .docked_traversal = None;
            cockpit.ship_observation = true;
            cockpit.station_walk_active = true;
            cockpit.controls.publish_station(1., 0., 0., false);
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            let (tx, rx) = mpsc::channel();
            cockpit.commands = Some(tx);
            let draw = |cockpit: &mut Cockpit, ui: &mut egui::Ui| cockpit.station_actions(ui);
            transition_test_frame(&ctx, &mut cockpit, 0, vec![], draw);
            let labels = transition_test_frame(&ctx, &mut cockpit, 1, vec![], draw);
            let point = labels
                .iter()
                .find(|(text, _)| text == label)
                .expect(label)
                .1;
            for (offset, pressed) in [true, false].into_iter().enumerate() {
                transition_test_frame(
                    &ctx,
                    &mut cockpit,
                    2 + offset as u32,
                    transition_click(point, pressed),
                    draw,
                );
            }
            assert!(!cockpit.ship_observation);
            assert_eq!(cockpit.hangar_view.on_foot_view, view);
            assert!(cockpit.workspace_input_waiting_release);
            assert!(!cockpit.station_walk_active);
            assert!(matches!(
                cockpit.controls.take(),
                Some(Request::StationWalk {
                    forward: 0.,
                    strafe: 0.,
                    turn: 0.,
                    sprint: false
                })
            ));
            assert!(
                rx.try_recv().is_err(),
                "camera selection must not issue gameplay commands"
            );
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        }
    }

    #[test]
    fn pioneer_completion_space_walk_guidance_tracks_owned_active_and_terminal_routines() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        let pilot = cockpit.own().unwrap().name;
        let snapshot = cockpit.snapshot.as_mut().unwrap();
        snapshot.pilots[0].location = Location::Belt;
        snapshot.resource_jobs = vec![ResourceJob {
            drone_cargo: None,
            controller: cockpit.pilot.clone(),
            pilot,
            id: 8,
            target_units: 10000,
            mined_units: 10000,
            stored_units: 10000,
            target_id: None,
            phase: ResourceJobPhase::Mining,
            message: String::new(),
            started_at_ms: 1000,
            updated_at_ms: 1000,
        }];
        assert_eq!(
            cockpit.space_walk_routine_status(),
            "Mining autopilot active"
        );
        cockpit.embedded_paused = true;
        assert!(cockpit.space_walk_routine_status().contains("paused"));
        cockpit.embedded_paused = false;
        for phase in [
            ResourceJobPhase::Completed,
            ResourceJobPhase::Cancelled,
            ResourceJobPhase::Blocked,
        ] {
            cockpit.snapshot.as_mut().unwrap().resource_jobs[0].phase = phase;
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            assert!(cockpit
                .space_walk_routine_status()
                .contains("No active owned mining routine"));
            assert!(
                cockpit.space_walk_available(),
                "completed routine does not revoke an existing walk"
            );
            assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        }
    }

    #[test]
    fn registered_airlock_primary_controls_fit_real_overlay_and_send_bound_request() {
        for size in [Vec2::new(1500.0, 1000.0), Vec2::new(900.0, 650.0)] {
            for expanded in [false, true] {
                let ctx = egui::Context::default();
                let mut style = (*ctx.style()).clone();
                style.animation_time = 0.0;
                ctx.set_style(style);
                let mut cockpit = docked_fixture(&ctx, true);
                let state = cockpit.snapshot.as_mut().unwrap().pilots[0]
                    .station_avatar
                    .as_mut()
                    .unwrap()
                    .docked_traversal
                    .as_mut()
                    .unwrap();
                let binding = state.binding.clone();
                let pose = docked_binding_pose(&binding).unwrap();
                state.feet_m = hatch_anchors(pose).outer_inside_station_m;
                state.support = DockedSupport::Connector;
                state.surface_id = "boarding/vestibule".into();
                state.actuators.hatches = Some(std::array::from_fn(|_| DockedHatchState {
                    fraction: 0.0,
                    target_open: false,
                    moving: false,
                    blocked: false,
                    updated_at_ms: 1000,
                }));
                let body_feet = station_to_body(pose, state.feet_m);
                assert!(
                    hatch_leaf_sweep_bounds(PioneerDockedHatch::Outer, 0.0, 1.0)
                        .unwrap()
                        .iter()
                        .all(|bounds| !box_intersects_capsule(
                            *bounds,
                            body_feet,
                            AVATAR_HEIGHT_M,
                            STATION_AVATAR_RADIUS_M
                        )),
                    "Fixture must stand clear of the actual leaf sweep"
                );
                let before = serde_json::to_value(&cockpit.snapshot).unwrap();
                let (tx, rx) = mpsc::channel();
                cockpit.commands = Some(tx);
                let mut details_target = None;
                let mut outer_target = None;
                let mut details_seen = false;
                for pass in 0..10 {
                    let target = if expanded && (pass == 2 || pass == 3) {
                        details_target
                    } else if pass == 7 || pass == 8 {
                        outer_target
                    } else {
                        None
                    };
                    let events = target.map_or_else(Vec::new, |pos| {
                        vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: pass == 2 || pass == 7,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ]
                    });
                    let output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, size)),
                            time: Some(pass as f64 / 60.0),
                            events,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::TopBottomPanel::bottom("airlock_test_editor_status")
                                .exact_height(28.0)
                                .show(ctx, |ui| {
                                    ui.label("Editor status");
                                });
                            cockpit.station_overlay(ctx);
                            egui::CentralPanel::default().show(ctx, |_ui| {});
                        },
                    );
                    let usable =
                        egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(size.x, size.y - 28.0));
                    let mut first_inner = None;
                    let mut first_outer = None;
                    for shape in &output.shapes {
                        if let egui::Shape::Text(text) = &shape.shape {
                            let label = text.galley.text();
                            let center = text.pos + text.galley.rect.center().to_vec2();
                            if label == "Nearby controls and route" {
                                details_target = Some(center);
                            }
                            if label.starts_with("Route: cabin") {
                                details_seen = true;
                            }
                            if label == "Open inner hatch" && first_inner.is_none() {
                                first_inner = Some((center, shape.clip_rect));
                            }
                            if label == "Open outer hatch" && first_outer.is_none() {
                                first_outer = Some((center, shape.clip_rect));
                            }
                        }
                    }
                    if pass == 5 {
                        for (name, button) in [("inner", first_inner), ("outer", first_outer)] {
                            let (center, clip) =
                                button.expect("Both primary hatch actions must be emitted");
                            assert!(clip.contains(center) && usable.contains(center),
                                "Primary {name} hatch action clipped at {size:?}, details={expanded}: center={center:?} clip={clip:?}");
                        }
                        outer_target = first_outer.map(|(center, _)| center);
                        assert_eq!(
                            details_seen, expanded,
                            "Details must be exercised in both real overlay states"
                        );
                    }
                }
                let received = rx.try_recv();
                assert!(
                    matches!(&received, Ok(Request::DockedTraversal {
                    request: DockedTraversalRequest::SetHatch { binding: actual, hatch: DockedHatch::Outer, open: true }
                }) if actual == &binding),
                    "Visible primary Outer action must submit its real bound command at {size:?}, details={expanded}: {received:?}"
                );
                assert_eq!(
                    serde_json::to_value(&cockpit.snapshot).unwrap(),
                    before,
                    "UI must not animate or move the authoritative hatch locally"
                );
            }
        }
    }

    #[test]
    fn legacy_interior_compact_and_full_lift_share_both_directions_and_interlocks() {
        for compact in [true, false] {
            for deck in [1, 2] {
                for (blocked, explanation) in [
                    ("", ""),
                    ("paused", "Resume simulation"),
                    ("distant", "marked access-core lift landing"),
                    ("jumping", "Land on"),
                    ("seated", "Stand up"),
                    ("unsupported", "unavailable on this server"),
                    ("offline", "Reconnect"),
                ] {
                    let ctx = egui::Context::default();
                    let mut c = docked_fixture(&ctx, false);
                    c.embedded_paused = blocked == "paused";
                    c.connected = blocked != "offline";
                    if blocked == "unsupported" {
                        c.assets.as_mut().unwrap().pioneer_interior_decks = 1;
                    }
                    let avatar = StationAvatar {
                        sprinting: false,
                        docked_traversal: None,
                        onboard_interior: true,
                        interior_deck: deck,
                        jump_started_ms: (blocked == "jumping").then_some(1000),
                        seated: blocked == "seated",
                        position: if blocked == "distant" {
                            [1000.0, 0.0, 1000.0]
                        } else {
                            PIONEER_LIFT_POINT
                        },
                        heading: 0.0,
                        moving: false,
                        updated_at_ms: 1000,
                    };
                    c.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(avatar.clone());
                    let before = serde_json::to_value(&c.snapshot).unwrap();
                    let (tx, rx) = mpsc::channel();
                    c.commands = Some(tx);
                    let destination = if deck == 1 { 2 } else { 1 };
                    let label = format!("Use lift to deck {destination}");
                    let mut target = None;
                    let mut explanation_seen = false;
                    for pass in 0..4 {
                        let events = match (pass, target) {
                            (2, Some(pos)) => vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                            (3, Some(pos)) => vec![egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            }],
                            _ => vec![],
                        };
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
                                egui::CentralPanel::default().show(ctx, |ui| {
                                    if compact {
                                        c.interior_exit_prompt(ui, &avatar);
                                    } else {
                                        c.station_actions(ui);
                                    }
                                });
                            },
                        );
                        for shape in output.shapes {
                            if let egui::Shape::Text(text) = shape.shape {
                                if text.galley.text() == label {
                                    let center = text.pos + text.galley.rect.center().to_vec2();
                                    assert!(shape.clip_rect.contains(center));
                                    target = Some(center);
                                }
                                explanation_seen |= !explanation.is_empty()
                                    && text.galley.text().contains(explanation);
                            }
                        }
                    }
                    assert!(
                        target.is_some(),
                        "deck {deck}, compact {compact}: action missing"
                    );
                    if blocked.is_empty() {
                        assert!(
                            matches!(rx.try_recv(), Ok(Request::InteriorLift {deck}) if deck == destination)
                        );
                    } else {
                        assert!(explanation_seen, "Missing explanation for {blocked}");
                        assert!(rx.try_recv().is_err(), "{blocked} must not send travel");
                    }
                    assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
                }
            }
        }
    }

    #[test]
    fn boarding_lift_primary_action_sends_real_call_without_opening_details() {
        for blocked in ["", "paused", "hatch", "stale"] {
            let ctx = egui::Context::default();
            let mut cockpit = docked_fixture(&ctx, true);
            let state = cockpit.snapshot.as_mut().unwrap().pilots[0]
                .station_avatar
                .as_mut()
                .unwrap()
                .docked_traversal
                .as_mut()
                .unwrap();
            let pose = docked_binding_pose(&state.binding).unwrap();
            state.feet_m = lift_call_anchors(pose).station_station_m;
            state.feet_m[1] = lift_geometry(pose).station_floor_m;
            state.support = DockedSupport::BoardingLift;
            state.actuators.lift.floor_station_m = lift_geometry(pose).station_floor_m;
            if blocked == "hatch" {
                state.actuators.outer_hatch_open = true;
            }
            let binding = state.binding.clone();
            cockpit.embedded_paused = blocked == "paused";
            if blocked == "stale" {
                cockpit.frozen_time = Some(5000);
            }
            let before = serde_json::to_value(&cockpit.snapshot).unwrap();
            let (tx, rx) = mpsc::channel();
            cockpit.commands = Some(tx);
            let mut target = None;
            for pass in 0..4 {
                let events = match (pass, target) {
                    (2, Some(pos)) => vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    (3, Some(pos)) => vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    _ => vec![],
                };
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(1100.0, 160.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| cockpit.docked_interaction_strip(ui));
                    },
                );
                for shape in output.shapes {
                    if let egui::Shape::Text(text) = shape.shape {
                        if text.galley.text() == "Lift to ship" {
                            let center = text.pos + text.galley.rect.center().to_vec2();
                            assert!(
                                shape.clip_rect.contains(center),
                                "Primary action must fit the visible strip"
                            );
                            target = Some(center);
                        }
                    }
                }
            }
            assert!(
                target.is_some(),
                "Lift action must not require expanding route details"
            );
            if blocked.is_empty() {
                assert!(matches!(rx.try_recv(), Ok(Request::DockedTraversal {
                    request: DockedTraversalRequest::CallLift {binding: actual, stop: DockedLiftStop::Ship}
                }) if actual == binding));
            } else {
                assert!(
                    rx.try_recv().is_err(),
                    "{blocked} must not issue a lift request"
                );
            }
            assert_eq!(
                serde_json::to_value(&cockpit.snapshot).unwrap(),
                before,
                "The UI must not move the platform or actor locally"
            );
        }
    }

    fn docked_walk_frame(
        ctx: &egui::Context,
        cockpit: &mut Cockpit,
        focused: bool,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(900.0, 650.0),
                )),
                focused,
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let pilot = cockpit.own().unwrap();
                    cockpit
                        .hangar_view
                        .set_docked_traversal_enabled(cockpit.docked_navigation_present());
                    cockpit.hangar_view.ui(
                        ui,
                        &pilot,
                        1000,
                        true,
                        None,
                        cockpit
                            .assets
                            .as_ref()
                            .and_then(|assets| assets.pioneer_room.as_ref()),
                        None,
                        1000,
                        None,
                    );
                });
                cockpit.sample_station_walk(ctx, true);
            },
        );
    }

    #[test]
    fn docked_capability_and_exact_layout_bind_controls_without_legacy_conversion() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        assert!(cockpit.verified_docked_binding().is_some());
        cockpit
            .snapshot
            .as_mut()
            .unwrap()
            .docked_traversal_capability
            .as_mut()
            .unwrap()
            .source_sha256 = "wrong-source".into();
        assert!(cockpit.verified_docked_binding().is_none());
        assert!(
            cockpit.docked_navigation_present(),
            "an incompatible active journey must not enable legacy teleport controls"
        );
        cockpit
            .snapshot
            .as_mut()
            .unwrap()
            .docked_traversal_capability = Some(docked_traversal_capability());
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .docked_traversal
            .as_mut()
            .unwrap()
            .binding
            .layout_sha256 = "0".repeat(64);
        assert!(cockpit.verified_docked_binding().is_none());
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .docked_traversal = None;
        assert!(
            cockpit.verified_docked_binding().is_none(),
            "legacy crew/station embodiment stays legacy until an explicit seat exit"
        );
        assert!(!cockpit.docked_navigation_present());
    }

    #[test]
    fn unresolved_seated_registration_blocks_legacy_entry_without_changing_camera_authority() {
        let ctx = egui::Context::default();
        for missing_assets in [false, true] {
            let mut cockpit = docked_fixture(&ctx, false);
            let (tx, rx) = std::sync::mpsc::channel();
            cockpit.commands = Some(tx);
            if missing_assets {
                cockpit.assets = None;
            } else {
                cockpit
                    .snapshot
                    .as_mut()
                    .unwrap()
                    .docked_traversal_capability
                    .as_mut()
                    .unwrap()
                    .source_sha256 = "unmatched-source".into();
            }
            assert!(cockpit.docked_navigation_present());
            assert!(cockpit.verified_docked_binding().is_none());
            cockpit.view_mode = ViewMode::Cockpit;
            assert!(cockpit.in_station_hangar());
            cockpit.view_mode = ViewMode::External;
            assert!(!cockpit.in_station_hangar());
            let output = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    assert!(cockpit.docked_station_actions(ui));
                });
            });
            let labels: Vec<_> = output
                .shapes
                .into_iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = shape.shape {
                        Some(text.galley.job.text.clone())
                    } else {
                        None
                    }
                })
                .collect();
            assert!(labels
                .iter()
                .any(|label| label == "Pioneer traversal unavailable"));
            assert!(!labels
                .iter()
                .any(|label| label.contains("Explore") || label.contains("Enter ship")));
            assert!(cockpit.own().unwrap().station_avatar.is_none());
            assert!(rx.try_recv().is_err());
        }
    }

    #[test]
    fn registered_room_and_stair_labels_follow_bound_body_coordinates() {
        let ctx = egui::Context::default();
        let cockpit = docked_fixture(&ctx, true);
        let mut state = cockpit
            .own()
            .unwrap()
            .station_avatar
            .unwrap()
            .docked_traversal
            .unwrap();
        let pose = docked_binding_pose(&state.binding).unwrap();
        let layout = default_pioneer_room_layout();
        for room in &layout.rooms {
            let local = [
                room.origin_xz_m[0] + room.clear_lwh_m[0] * 0.5,
                0.0,
                room.origin_xz_m[1] + room.clear_lwh_m[1] * 0.5,
            ];
            state.support = if room.deck == 1 {
                DockedSupport::Deck1
            } else {
                DockedSupport::Deck2
            };
            state.feet_m = arkline_protocol::body_to_station(
                pose,
                arkline_protocol::deck_to_body(room.deck, local).unwrap(),
            );
            assert_eq!(
                cockpit.docked_room_name(&state).as_deref(),
                Some(room.name.as_str())
            );
        }
        state.support = DockedSupport::CoreStairs;
        for (body_y, deck) in [(-3.0, 1), (-1.0, 2)] {
            state.feet_m = arkline_protocol::body_to_station(pose, [0.0, body_y, 0.0]);
            assert_eq!(
                docked_state_label(&state),
                format!("PIONEER ACCESS STAIRS · DECK {deck}")
            );
            assert!(cockpit.docked_room_name(&state).is_none());
        }
        state.binding.layout_sha256 = "invalid".into();
        assert_eq!(
            docked_state_label(&state),
            "PIONEER REGISTRATION UNRESOLVED"
        );
    }

    #[test]
    fn docked_station_ui_sends_bound_seat_exit_and_omits_teleport_actions() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, false);
        let binding = cockpit.verified_docked_binding().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        cockpit.commands = Some(tx);
        let draw = |cockpit: &mut Cockpit, events| {
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
                    egui::CentralPanel::default().show(ctx, |ui| {
                        assert!(cockpit.docked_station_actions(ui));
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
            .collect::<Vec<_>>()
        };
        draw(&mut cockpit, vec![]);
        let labels = draw(&mut cockpit, vec![]);
        assert!(!labels.iter().any(|(label, _)| [
            "Leave ship",
            "Explore interior",
            "Enter ship",
            "Exit to hangar"
        ]
        .contains(&label.as_str())));
        let point = labels
            .iter()
            .find(|(label, _)| label == "Leave pilot seat")
            .unwrap()
            .1;
        for pressed in [true, false] {
            draw(
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
        assert!(
            matches!(rx.try_recv().unwrap(), Request::DockedTraversal { request: DockedTraversalRequest::LeaveSeat {binding: actual} } if actual == binding)
        );
    }

    #[test]
    fn docked_walk_focus_loss_emits_bound_stop_and_repeated_view_release_preserves_it() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        let binding = cockpit.verified_docked_binding().unwrap();
        docked_walk_frame(
            &ctx,
            &mut cockpit,
            true,
            vec![
                egui::Event::PointerMoved(Pos2::new(450.0, 325.0)),
                egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            egui::Modifiers::NONE,
        );
        assert!(
            matches!(cockpit.controls.take(), Some(Request::DockedTraversal { request: DockedTraversalRequest::Walk {binding: actual, forward:1.0, ..} }) if actual == binding)
        );
        docked_walk_frame(&ctx, &mut cockpit, false, vec![], egui::Modifiers::NONE);
        cockpit.controls.clear_flight();
        cockpit.controls.clear_flight();
        assert!(
            matches!(cockpit.controls.take(), Some(Request::DockedTraversal { request: DockedTraversalRequest::Walk {
                sprint: false,binding: actual, forward:0.0,strafe:0.0,turn:0.0} }) if actual == binding)
        );
        docked_walk_frame(&ctx, &mut cockpit, false, vec![], egui::Modifiers::NONE);
        assert!(cockpit.controls.take().is_none());
    }

    #[test]
    fn on_foot_shift_preserves_arrows_and_sprints_only_translation_when_advertised() {
        for canonical in [false, true] {
            for supported in [false, true] {
                for (key, expected) in [
                    (egui::Key::W, (1.0, 0.0, 0.0, supported)),
                    (egui::Key::ArrowUp, (1.0, 0.0, 0.0, supported)),
                    (egui::Key::Q, (0.0, -1.0, 0.0, supported)),
                    (egui::Key::ArrowRight, (0.0, 0.0, 1.0, false)),
                ] {
                    let ctx = egui::Context::default();
                    let mut cockpit = docked_fixture(&ctx, true);
                    cockpit.snapshot.as_mut().unwrap().walk_sprint_v1 = supported;
                    if !canonical {
                        cockpit.snapshot.as_mut().unwrap().pilots[0]
                            .station_avatar
                            .as_mut()
                            .unwrap()
                            .docked_traversal = None;
                    }
                    docked_walk_frame(&ctx, &mut cockpit, true, vec![], egui::Modifiers::NONE);
                    let look = cockpit.hangar_view.head_look_angles();
                    docked_walk_frame(
                        &ctx,
                        &mut cockpit,
                        true,
                        vec![
                            egui::Event::PointerMoved(Pos2::new(450.0, 325.0)),
                            egui::Event::Key {
                                key,
                                physical_key: None,
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::SHIFT,
                            },
                        ],
                        egui::Modifiers::SHIFT,
                    );
                    let actual = match cockpit.controls.take().unwrap() {
                        Request::StationWalk {
                            forward,
                            strafe,
                            turn,
                            sprint,
                        } if !canonical => (forward, strafe, turn, sprint),
                        Request::DockedTraversal {
                            request:
                                DockedTraversalRequest::Walk {
                                    forward,
                                    strafe,
                                    turn,
                                    sprint,
                                    ..
                                },
                        } if canonical => (forward, strafe, turn, sprint),
                        other => panic!("Unexpected on-foot input route: {other:?}"),
                    };
                    assert_eq!(
                        actual, expected,
                        "canonical={canonical}, supported={supported}, key={key:?}"
                    );
                    assert_eq!(
                        cockpit.hangar_view.head_look_angles(),
                        look,
                        "On-foot Shift arrows must not also change the head-look pose"
                    );
                }
            }
        }
    }

    #[test]
    fn sprint_releases_to_bound_neutral_on_focus_loss() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        let binding = cockpit.verified_docked_binding().unwrap();
        docked_walk_frame(&ctx, &mut cockpit, true, vec![], egui::Modifiers::NONE);
        docked_walk_frame(
            &ctx,
            &mut cockpit,
            true,
            vec![
                egui::Event::PointerMoved(Pos2::new(450.0, 325.0)),
                egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
            egui::Modifiers::SHIFT,
        );
        assert!(matches!(
            cockpit.controls.take(),
            Some(Request::DockedTraversal {
                request: DockedTraversalRequest::Walk { sprint: true, .. }
            })
        ));
        docked_walk_frame(&ctx, &mut cockpit, false, vec![], egui::Modifiers::SHIFT);
        cockpit.controls.clear_flight();
        assert!(
            matches!(cockpit.controls.take(), Some(Request::DockedTraversal {
            request: DockedTraversalRequest::Walk {binding: actual, forward: 0.0,
                strafe: 0.0, turn: 0.0, sprint: false} }) if actual == binding)
        );
        docked_walk_frame(&ctx, &mut cockpit, false, vec![], egui::Modifiers::SHIFT);
        assert!(cockpit.controls.take().is_none());
    }

    #[test]
    fn actual_text_editor_ownership_stops_sprint_on_the_original_binding() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        let binding = cockpit.verified_docked_binding().unwrap();
        docked_walk_frame(&ctx, &mut cockpit, true, vec![], egui::Modifiers::NONE);
        docked_walk_frame(
            &ctx,
            &mut cockpit,
            true,
            vec![
                egui::Event::PointerMoved(Pos2::new(450.0, 325.0)),
                egui::Event::Key {
                    key: egui::Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
            egui::Modifiers::SHIFT,
        );
        assert!(matches!(
            cockpit.controls.take(),
            Some(Request::DockedTraversal {
                request: DockedTraversalRequest::Walk { sprint: true, .. }
            })
        ));
        let mut text = String::from("Room label");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(900.0, 650.0),
                )),
                focused: true,
                modifiers: egui::Modifiers::SHIFT,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add(egui::TextEdit::singleline(&mut text))
                        .request_focus();
                    assert!(ctx.wants_keyboard_input());
                    cockpit.sample_station_walk(ctx, true);
                });
            },
        );
        assert!(
            matches!(cockpit.controls.take(), Some(Request::DockedTraversal {
            request: DockedTraversalRequest::Walk {binding: actual, forward: 0.0,
                strafe: 0.0, turn: 0.0, sprint: false} }) if actual == binding)
        );
    }

    #[test]
    fn seated_shift_arrows_keep_look_without_walking() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, false);
        docked_walk_frame(
            &ctx,
            &mut cockpit,
            true,
            vec![egui::Event::PointerMoved(Pos2::new(450.0, 325.0))],
            egui::Modifiers::NONE,
        );
        docked_walk_frame(
            &ctx,
            &mut cockpit,
            true,
            vec![
                egui::Event::PointerMoved(Pos2::new(450.0, 325.0)),
                egui::Event::Key {
                    key: egui::Key::ArrowRight,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::SHIFT,
                },
            ],
            egui::Modifiers::SHIFT,
        );
        assert_ne!(cockpit.hangar_view.head_look_angles(), [0.0; 2]);
        assert!(cockpit.controls.take().is_none());
    }

    #[test]
    fn docked_interaction_preserves_neutral_stop_when_authority_rejects_an_action() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        let binding = cockpit.verified_docked_binding().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        cockpit.commands = Some(tx);
        cockpit.docked_walk_binding = Some(binding.clone());
        cockpit.station_walk_active = true;
        cockpit
            .controls
            .publish_docked(binding.clone(), 1.0, 0.0, 0.0, false);
        cockpit.send(Request::DockedTraversal {
            request: DockedTraversalRequest::SetHatch {
                binding: binding.clone(),
                hatch: DockedHatch::Outer,
                open: true,
            },
        });
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::DockedTraversal {
                request: DockedTraversalRequest::SetHatch { .. }
            }
        ));
        assert!(
            matches!(cockpit.controls.take(), Some(Request::DockedTraversal {request: DockedTraversalRequest::Walk {
                sprint: false,binding: actual,forward:0.0,strafe:0.0,turn:0.0}}) if actual == binding)
        );
    }

    #[test]
    fn docked_registration_change_stops_original_route_before_accepting_other_input() {
        let ctx = egui::Context::default();
        let mut cockpit = docked_fixture(&ctx, true);
        let binding = cockpit.verified_docked_binding().unwrap();
        cockpit.docked_walk_binding = Some(binding.clone());
        cockpit.station_walk_active = true;
        cockpit
            .snapshot
            .as_mut()
            .unwrap()
            .docked_traversal_capability = None;
        docked_walk_frame(&ctx, &mut cockpit, true, vec![], egui::Modifiers::NONE);
        assert!(
            matches!(cockpit.controls.take(), Some(Request::DockedTraversal {request: DockedTraversalRequest::Walk {
                sprint: false,binding: actual,forward:0.0,strafe:0.0,turn:0.0}}) if actual == binding)
        );
        assert!(!cockpit.station_walk_active);
    }

    #[test]
    fn station_room_name_uses_live_room_bounds_and_name() {
        let mut layout = arkline_protocol::default_pioneer();
        let room = layout.rooms.iter_mut().find(|room| room.id == 2).unwrap();
        room.name = "LIVE TEST ROOM".into();
        let center = [
            room.origin_xz_m[0] + room.clear_lwh_m[0] * 0.5,
            0.0,
            room.origin_xz_m[1] + room.clear_lwh_m[1] * 0.5,
        ];
        let deck = room.deck;
        assert_eq!(
            live_room_name(&layout, deck, center),
            Some("LIVE TEST ROOM")
        );
        assert_eq!(
            live_room_name(&layout, deck + 1, center),
            Some("CREW / GALLEY")
        );
        assert_eq!(live_room_name(&layout, deck, [2.0, 0.0, 7.0]), None);
        assert_eq!(live_room_name(&layout, deck + 1, [2.0, 0.0, 7.0]), None);
    }

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
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: Some(arkline_protocol::PioneerRoomState::uncommissioned(
                "owned-pioneer".into(),
            )),
            pioneer_room_layout: None,
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
        for label in [
            "Explore interior",
            "Leave ship",
            "Undock",
            "Room engineering",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == label),
                "missing {label}"
            );
        }
        assert!(reserved.bottom() < 650.0);
        assert!(
            650.0 - reserved.bottom() < 170.0,
            "bottom strip grew too tall: {reserved:?}"
        );

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
    fn embedded_hangar_boarding_actions_stay_visible_and_obey_authority_gates() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        let (position, boarding_point) = arkline_protocol::fleet_berth(ShipClass::Pioneer);
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.ship_class = ShipClass::Pioneer;
        pilot.location = Location::Docked;
        pilot.station_avatar = Some(StationAvatar {
            sprinting: false,
            docked_traversal: None,
            onboard_interior: false,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: boarding_point,
            heading: 0.0,
            moving: false,
            updated_at_ms: 1000,
        });
        cockpit.assets = Some(AssetsView {
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: Some(arkline_protocol::HangarView {
                layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
                layout: arkline_protocol::fleet_hangar_layout(),
                berths: vec![arkline_protocol::HangarBerth {
                    id: "owned-pioneer".into(),
                    ship_class: ShipClass::Pioneer,
                    position,
                    yaw: 0.0,
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
        let (commands, requests) = mpsc::channel();
        cockpit.commands = Some(commands);
        let frame = |cockpit: &mut Cockpit, events| {
            let screen = egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(440.0, 190.0));
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    cockpit.station_overlay(ctx);
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            )
            .shapes
            .into_iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = shape.shape {
                    let bounds = egui::Rect::from_min_size(text.pos, text.galley.rect.size());
                    Some((
                        text.galley.job.text.clone(),
                        text.pos + text.galley.rect.center().to_vec2(),
                        screen.contains_rect(bounds) && shape.clip_rect.contains_rect(bounds),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
        };
        let click = |cockpit: &mut Cockpit, point: Pos2| {
            for pressed in [true, false] {
                frame(
                    cockpit,
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
        };
        frame(&mut cockpit, vec![]);
        let labels = frame(&mut cockpit, vec![]);
        for required in ["Enter ship", "Explore interior", "Boarding · 0.0 m"] {
            assert_eq!(
                labels
                    .iter()
                    .filter(|(text, _, _)| text == required)
                    .count(),
                1,
                "primary action must not be duplicated"
            );
            assert!(
                labels
                    .iter()
                    .any(|(text, _, visible)| text == required && *visible),
                "{required} must be fully visible at 440 × 190"
            );
        }
        assert!(labels
            .iter()
            .any(|(text, _, visible)| text.contains("W/S move") && *visible));
        assert!(
            !labels
                .iter()
                .any(|(text, _, _)| text.contains("Owned vessels")),
            "fleet choices belong in editor Station Services"
        );
        let enter = labels
            .iter()
            .find(|(text, _, visible)| text == "Enter ship" && *visible)
            .unwrap()
            .1;
        click(&mut cockpit, enter);
        assert!(matches!(requests.try_recv().unwrap(), Request::EnterShip));
        cockpit.request_pending = false;
        let labels = frame(&mut cockpit, vec![]);
        let explore = labels
            .iter()
            .find(|(text, _, visible)| text == "Explore interior" && *visible)
            .unwrap()
            .1;
        click(&mut cockpit, explore);
        assert!(matches!(
            requests.try_recv().unwrap(),
            Request::EnterInterior
        ));
        cockpit.request_pending = false;
        cockpit.embedded_paused = true;
        let labels = frame(&mut cockpit, vec![]);
        assert!(labels
            .iter()
            .any(|(text, _, visible)| text.contains("Simulation paused") && *visible));
        for required in ["Enter ship", "Explore interior"] {
            let point = labels
                .iter()
                .find(|(text, _, visible)| text == required && *visible)
                .expect("disabled primary action remains visible")
                .1;
            click(&mut cockpit, point);
        }
        assert!(requests.try_recv().is_err());
        cockpit.embedded_paused = false;
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .position[0] += 10.0;
        let labels = frame(&mut cockpit, vec![]);
        assert!(labels
            .iter()
            .any(|(text, _, visible)| text.contains("within 3 m") && *visible));
        let enter = labels
            .iter()
            .find(|(text, _, visible)| text == "Enter ship" && *visible)
            .unwrap()
            .1;
        click(&mut cockpit, enter);
        assert!(requests.try_recv().is_err(), "boarding remains range-gated");
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(900.0, 650.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| cockpit.station_actions(ui));
            },
        );
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape, egui::Shape::Text(text) if text.galley.job.text.contains("Owned vessels"))), "Station Services retains physical fleet choices");
    }

    #[test]
    fn embedded_interior_exit_stays_visible_and_explains_paused_gate() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        let avatar = StationAvatar {
            sprinting: false,
            docked_traversal: None,
            onboard_interior: true,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: PIONEER_INTERIOR_ENTRY,
            heading: 0.0,
            moving: false,
            updated_at_ms: 1000,
        };
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.station_avatar = Some(avatar);
        cockpit.assets = Some(AssetsView {
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let frame = |cockpit: &mut Cockpit, events, size: Vec2| {
            let screen_rect = egui::Rect::from_min_size(Pos2::ZERO, size);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen_rect),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    cockpit.station_overlay(ctx);
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            );
            output
                .shapes
                .into_iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = shape.shape {
                        let text_rect =
                            egui::Rect::from_min_size(text.pos, text.galley.rect.size());
                        Some((
                            text.galley.job.text.clone(),
                            text.pos + text.galley.rect.center().to_vec2(),
                            shape.clip_rect.contains_rect(text_rect)
                                && screen_rect.contains_rect(text_rect),
                        ))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        };

        let compact = Vec2::new(440.0, 190.0);
        let normal = Vec2::new(900.0, 650.0);
        frame(&mut cockpit, vec![], compact);
        let labels = frame(&mut cockpit, vec![], compact);
        let exit = labels
            .iter()
            .find(|(text, _, visible)| text == "Exit to hangar" && *visible)
            .expect("the airlock exit must be visible in the pinned strip")
            .1;
        assert!(labels
            .iter()
            .any(|(text, _, visible)| *visible && text.contains("W/S move")));
        assert!(labels
            .iter()
            .any(|(text, _, visible)| *visible && text.contains("Legacy open")));
        for pressed in [true, false] {
            frame(
                &mut cockpit,
                vec![
                    egui::Event::PointerMoved(exit),
                    egui::Event::PointerButton {
                        pos: exit,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                compact,
            );
        }
        assert!(matches!(rx.try_recv().unwrap(), Request::ExitInterior));

        cockpit.request_pending = false;
        cockpit.embedded_paused = true;
        let labels = frame(&mut cockpit, vec![], compact);
        let paused_exit = labels
            .iter()
            .find(|(text, _, visible)| text == "Exit to hangar" && *visible)
            .expect("the exit action remains visible while paused")
            .1;
        assert!(labels
            .iter()
            .any(|(text, _, visible)| *visible && text.contains("Simulation paused")));
        for pressed in [true, false] {
            frame(
                &mut cockpit,
                vec![
                    egui::Event::PointerMoved(paused_exit),
                    egui::Event::PointerButton {
                        pos: paused_exit,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                compact,
            );
        }
        assert!(rx.try_recv().is_err());

        cockpit.embedded_paused = false;
        frame(&mut cockpit, vec![], normal);
        let labels = frame(&mut cockpit, vec![], normal);
        assert!(labels
            .iter()
            .any(|(text, _, visible)| *visible && text == "Exit to hangar"));
        assert!(labels
            .iter()
            .any(|(text, _, visible)| *visible && text.contains("W/S move")));
    }

    #[test]
    fn closed_airlock_requires_nearby_authoritative_open_command_before_exit() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.embedded = true;
        let mut room = PioneerRoomState::uncommissioned("owned-pioneer".into());
        room.door_open = false;
        room.door_commanded_open = false;
        room.interlock = Some("Door closed".into());
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.station_avatar = Some(StationAvatar {
            sprinting: false,
            docked_traversal: None,
            onboard_interior: true,
            interior_deck: 1,
            jump_started_ms: None,
            seated: false,
            position: PIONEER_INTERIOR_ENTRY,
            heading: 0.0,
            moving: false,
            updated_at_ms: 1000,
        });
        cockpit.assets = Some(AssetsView {
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: Some(room),
            pioneer_room_layout: None,
            hangar: None,
            pioneer_interior_available: true,
            pioneer_interior_decks: 2,
            ships: vec![],
            storage: vec![],
            corporation: None,
            invitations: vec![],
        });
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let frame = |cockpit: &mut Cockpit, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(440.0, 190.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    cockpit.station_overlay(ctx);
                    egui::CentralPanel::default().show(ctx, |_| {});
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
            .collect::<Vec<_>>()
        };
        let click = |cockpit: &mut Cockpit, point: Pos2| {
            for pressed in [true, false] {
                frame(
                    cockpit,
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
        };

        frame(&mut cockpit, vec![]);
        let labels = frame(&mut cockpit, vec![]);
        assert!(!labels.iter().any(|(text, _)| text == "Exit to hangar"));
        assert!(labels
            .iter()
            .any(|(text, _)| text.contains("within 3 m of its controls")));
        let open = labels
            .iter()
            .find(|(text, _)| text == "Open airlock manually")
            .expect("closed door exposes its opening control")
            .1;
        click(&mut cockpit, open);
        assert!(rx.try_recv().is_err(), "opening is range-gated");

        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .position = [
            PIONEER_CONNECTED_ROOM.breaker_position[0],
            0.0,
            PIONEER_CONNECTED_ROOM.breaker_position[2],
        ];
        let labels = frame(&mut cockpit, vec![]);
        let open = labels
            .iter()
            .find(|(text, _)| text == "Open airlock manually")
            .expect("control action remains visible in range")
            .1;
        click(&mut cockpit, open);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::SetPioneerRoom {
                action: PioneerRoomCommand::Door {
                    open: true,
                    manual: true
                }
            }
        ));

        cockpit.request_pending = false;
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .position = PIONEER_INTERIOR_ENTRY;
        let state = cockpit
            .assets
            .as_mut()
            .unwrap()
            .pioneer_room
            .as_mut()
            .unwrap();
        state.door_open = true;
        state.door_commanded_open = true;
        state.interlock = None;
        let labels = frame(&mut cockpit, vec![]);
        assert!(!labels
            .iter()
            .any(|(text, _)| text == "Open airlock manually"));
        let exit = labels
            .iter()
            .find(|(text, _)| text == "Exit to hangar")
            .expect("exit is available only after server reports door open")
            .1;
        click(&mut cockpit, exit);
        assert!(matches!(rx.try_recv().unwrap(), Request::ExitInterior));
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
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: None,
            pioneer_room_layout: None,
            hangar: Some(arkline_protocol::HangarView {
                layout_revision: arkline_protocol::FleetLayoutRevision::LinearV1,
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
            carrier_inventory: None,
            drone_bay: None,
            drone_cargo: None,
            cargo_unloading: None,
            pioneer_room: Some(arkline_protocol::PioneerRoomState::uncommissioned(
                "own".into(),
            )),
            pioneer_room_layout: None,
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
                carrier_inventory: None,
                drone_bay: None,
                drone_cargo: None,
                cargo_unloading: None,
                pioneer_room: None,
                pioneer_room_layout: None,
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
                sprinting: false,
                docked_traversal: None,
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
                sprinting: false,
                docked_traversal: None,
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
    fn walk_keys_map_to_move_turn_and_strafe_axes() {
        fn sample(key: egui::Key) -> Request {
            let ctx = egui::Context::default();
            let mut cockpit = crate::tests::view_test_cockpit(&ctx);
            let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
            pilot.location = Location::Docked;
            pilot.station_avatar = Some(StationAvatar {
                sprinting: false,
                docked_traversal: None,
                onboard_interior: false,
                interior_deck: 1,
                jump_started_ms: None,
                seated: false,
                position: hangar_layout(pilot.ship_class).boarding_point,
                heading: 0.0,
                moving: false,
                updated_at_ms: 0,
            });
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
            cockpit.controls.take().unwrap()
        }

        for (key, expected) in [
            (egui::Key::W, (1., 0., 0.)),
            (egui::Key::ArrowUp, (1., 0., 0.)),
            (egui::Key::S, (-1., 0., 0.)),
            (egui::Key::ArrowDown, (-1., 0., 0.)),
            (egui::Key::A, (0., 0., -1.)),
            (egui::Key::ArrowLeft, (0., 0., -1.)),
            (egui::Key::D, (0., 0., 1.)),
            (egui::Key::ArrowRight, (0., 0., 1.)),
            (egui::Key::Q, (0., -1., 0.)),
            (egui::Key::E, (0., 1., 0.)),
        ] {
            let actual = match sample(key) {
                Request::StationWalk {
                    sprint: false,
                    forward,
                    strafe,
                    turn,
                } => (forward, strafe, turn),
                other => panic!("unexpected request: {other:?}"),
            };
            assert_eq!(actual, expected, "incorrect walk mapping for {key:?}");
        }
    }

    #[test]
    fn walk_controls_are_own_only_and_release_on_focus_loss() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar = Some(StationAvatar {
            sprinting: false,
            docked_traversal: None,
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
                sprint: false,
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
            sprinting: false,
            docked_traversal: None,
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
                sprint: false,
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
    fn embedded_boarded_docked_pilot_uses_cockpit_and_interiors_keep_hangar_scene() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let pilot = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        pilot.location = Location::Docked;
        pilot.station_avatar = None;
        cockpit.view_mode = ViewMode::Cockpit;
        assert!(
            cockpit.in_station_hangar(),
            "normal docked gameplay keeps its hangar scene"
        );
        cockpit.embedded = true;
        assert!(
            !cockpit.in_station_hangar(),
            "editor cockpit renders its boarded pilot"
        );
        cockpit.view_mode = ViewMode::External;
        assert!(
            !cockpit.in_station_hangar(),
            "editor camera changes preserve the cockpit scene"
        );
        cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(StationAvatar {
            sprinting: false,
            docked_traversal: None,
            onboard_interior: true,
            interior_deck: 2,
            position: arkline_protocol::PIONEER_LIFT_POINT,
            heading: 0.0,
            seated: false,
            moving: false,
            jump_started_ms: None,
            updated_at_ms: 0,
        });
        assert!(
            cockpit.in_station_hangar(),
            "editor interiors use their hangar renderer"
        );
        cockpit.snapshot.as_mut().unwrap().pilots[0]
            .station_avatar
            .as_mut()
            .unwrap()
            .onboard_interior = false;
        assert!(
            cockpit.in_station_hangar(),
            "editor station avatars use their hangar renderer"
        );
        cockpit.embedded = false;
        cockpit.view_mode = ViewMode::Cockpit;
        cockpit.snapshot.as_mut().unwrap().pilots[0].station_avatar = None;
        assert!(
            cockpit.in_station_hangar(),
            "game cockpit selection does not bypass the hangar"
        );
    }

    #[test]
    fn docked_hangar_requires_own_view_and_gathering_requires_embarked_pilot() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        let p = &mut cockpit.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Docked;
        p.station_avatar = Some(StationAvatar {
            sprinting: false,
            docked_traversal: None,
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
        assert!(cockpit
            .resource_start_reason()
            .unwrap()
            .contains("Return to the pilot seat"));
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
            sprinting: false,
            docked_traversal: None,
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
        cockpit.controls.publish_station(1., 0., 0., false);
        cockpit.station_walk_active = true;
        cockpit.activate_quarters_terminal(HangarAction::Sit);
        assert!(cockpit.controls.take().is_none());
        assert!(!cockpit.station_walk_active);
        assert!(matches!(
            rx.try_recv().unwrap(),
            Request::StationSit { seated: true }
        ));
    }
    #[test]
    fn cargo_departure_six_pointer_surfaces_explain_reservation_and_reenable_terminal_states() {
        use arkline_protocol::CargoUnloadPhase::*;
        for (width, surface) in [600., 900.]
            .into_iter()
            .flat_map(|width| (0..6).map(move |surface| (width, surface)))
        {
            let ctx = egui::Context::default();
            ctx.style_mut(|style| style.animation_time = 0.);
            let mut c = docked_fixture(&ctx, false);
            c.cockpit_console_detail = true;
            if matches!(surface, 2 | 3) {
                c.snapshot.as_mut().unwrap().docked_traversal_capability = None;
            }
            let (tx, rx) = mpsc::channel();
            c.commands = Some(tx);
            let frame = |c: &mut Cockpit, events| {
                c.observed = Instant::now();
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(width, 650.),
                        )),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ctx| {
                        if surface == 3 {
                            c.station_overlay(ctx);
                        }
                        egui::CentralPanel::default().show(ctx, |ui| match surface {
                            0 => {
                                assert!(c.docked_station_actions(ui));
                            }
                            1 => c.docked_interaction_strip(ui),
                            2 => c.station_actions_inner(ui, false, false),
                            3 => {}
                            4 => c.cockpit_actions(ui),
                            5 => c.flight_deck(ui),
                            _ => unreachable!(),
                        });
                    },
                )
            };
            for phase in [Unloading, Held, Unloading, Cancelled, Completed] {
                crate::tests::cargo_departure_fixture(&mut c, phase);
                c.request_pending = false;
                c.notice = "unchanged".into();
                let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
                frame(&mut c, vec![]);
                let output = frame(&mut c, vec![]);
                let labels: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(t) => {
                            let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                            shape
                                .clip_rect
                                .contains_rect(rect)
                                .then_some((t.galley.job.text.clone(), rect.center()))
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    labels
                        .iter()
                        .any(|(text, _)| text.starts_with("Cargo reserved")),
                    phase.active(),
                    "surface {surface}, {phase:?}: {labels:?}"
                );
                if surface == 4 {
                    assert_eq!(
                        labels.iter().any(|(t, _)| t == "CARGO RESERVED"),
                        phase.active()
                    );
                }
                let label = match surface {
                    2 => "Undock active vessel",
                    4 => "UNDOCK",
                    _ => "Undock",
                };
                let point = labels
                    .iter()
                    .find(|(text, _)| text == label)
                    .unwrap_or_else(|| panic!("surface {surface}: {labels:?}"))
                    .1;
                for pressed in [true, false] {
                    frame(&mut c, transition_click(point, pressed));
                }
                if phase.active() {
                    // A disabled widget must not even invoke the defensive send guard.
                    assert_eq!(c.notice, "unchanged", "surface {surface}");
                    assert!(!c.request_pending, "surface {surface}");
                } else {
                    assert!(
                        matches!(rx.try_recv(), Ok(Request::Undock)),
                        "surface {surface}, {phase:?}"
                    );
                    assert!(c.request_pending);
                }
                assert!(rx.try_recv().is_err(), "surface {surface}, {phase:?}");
                assert_eq!(
                    serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
                    before
                );
            }
        }
    }

    #[test]
    fn cargo_departure_does_not_disable_stop_docking_hold_resume_or_leave_seat() {
        for action in [
            "STOP",
            "HOLD",
            "RESUME",
            "Leave pilot seat",
            "Resume unloading",
            "Cancel remaining unload",
        ] {
            let ctx = egui::Context::default();
            let mut c = docked_fixture(&ctx, false);
            crate::tests::cargo_departure_fixture(&mut c, arkline_protocol::CargoUnloadPhase::Held);
            if matches!(action, "STOP" | "HOLD" | "RESUME") {
                let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
                p.location = Location::Station;
                p.flight.as_mut().unwrap().docking.as_mut().unwrap().phase = match action {
                    "HOLD" => DockingPhase::Approaching,
                    "RESUME" => DockingPhase::Held,
                    _ => DockingPhase::Free,
                };
            }
            assert!(c.navigation_order_reason().is_none());
            assert!(c.traversal_transition_block_reason().is_none());
            let (tx, rx) = mpsc::channel();
            c.commands = Some(tx);
            let body = |c: &mut Cockpit, ui: &mut egui::Ui| {
                if action == "Leave pilot seat" {
                    c.docked_station_actions(ui);
                } else if matches!(action, "Resume unloading" | "Cancel remaining unload") {
                    c.cargo_panel(ui);
                } else {
                    c.cockpit_actions(ui);
                }
            };
            transition_test_frame(&ctx, &mut c, 0, vec![], body);
            let labels = transition_test_frame(&ctx, &mut c, 1, vec![], body);
            let point = labels.iter().find(|(text, _)| text == action).unwrap().1;
            for (i, pressed) in [true, false].into_iter().enumerate() {
                transition_test_frame(
                    &ctx,
                    &mut c,
                    2 + i as u32,
                    transition_click(point, pressed),
                    body,
                );
            }
            let sent = rx.try_recv().unwrap_or_else(|_| panic!("{action} blocked"));
            assert!(
                match action {
                    "STOP" => matches!(
                        sent,
                        Request::Navigate {
                            mode: NavigationMode::Stop
                        }
                    ),
                    "HOLD" => matches!(sent, Request::CancelDocking { operation_id: 7 }),
                    "RESUME" => matches!(sent, Request::ResumeDocking { operation_id: 7 }),
                    "Resume unloading" =>
                        matches!(sent, Request::ResumeCargoUnload { operation_id: 1 }),
                    "Cancel remaining unload" =>
                        matches!(sent, Request::CancelCargoUnload { operation_id: 1 }),
                    _ => matches!(
                        sent,
                        Request::DockedTraversal {
                            request: DockedTraversalRequest::LeaveSeat { .. }
                        }
                    ),
                },
                "{action}"
            );
            assert!(rx.try_recv().is_err());
        }
    }
}
