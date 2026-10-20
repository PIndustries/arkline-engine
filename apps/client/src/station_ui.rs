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
            || pilot.location != Location::Docked
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
        if assets.hangar.as_ref().is_some_and(|fleet| {
            !fleet.berths.iter().any(|berth| {
                berth.active && berth.id == host && berth.ship_class == ShipClass::Pioneer
            })
        }) {
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
                && pilot.location == Location::Docked
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
                self.on_foot_view_selector(ui);
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
            let ready = self.can_operate() && state.grounded && state.seat_transition.is_none();
            let seats = seat_anchors(pose);
            let seat_distance = station_distance(state.feet_m, seats.root_station_m);
            let secured = state.actuators.hatches_closed()
                && !state.actuators.lift.moving
                && (state.actuators.lift.floor_station_m - lift_geometry(pose).ship_floor_m).abs()
                    < 0.03;
            let hatches = hatch_anchors(pose);
            let inner_distance =
                station_distance(state.feet_m, hatches.inner_inside_station_m).min(
                    station_distance(state.feet_m, hatches.inner_outside_station_m),
                );
            let outer_distance =
                station_distance(state.feet_m, hatches.outer_inside_station_m).min(
                    station_distance(state.feet_m, hatches.outer_outside_station_m),
                );
            let calls = lift_call_anchors(pose);
            let lift_distance = station_distance(state.feet_m, calls.ship_station_m)
                .min(station_distance(state.feet_m, calls.station_station_m));
            ui.horizontal_wrapped(|ui| {
                if seat_distance <= 1.5
                    && ui
                        .add_enabled(
                            ready && secured && seat_distance <= 1.0,
                            egui::Button::new("Take pilot seat"),
                        )
                        .clicked()
                {
                    self.send(Request::DockedTraversal {
                        request: DockedTraversalRequest::TakeSeat {
                            binding: binding.clone(),
                        },
                    });
                }
                for (hatch, distance, open, label) in [
                    (
                        DockedHatch::Inner,
                        inner_distance,
                        state.actuators.inner_hatch_open,
                        "inner hatch",
                    ),
                    (
                        DockedHatch::Outer,
                        outer_distance,
                        state.actuators.outer_hatch_open,
                        "outer hatch",
                    ),
                ] {
                    let index = usize::from(hatch == DockedHatch::Outer);
                    let motion = state.actuators.hatches.as_ref().map(|h| &h[index]);
                    let open = motion.map_or(open, |h| {
                        if h.blocked {
                            !h.target_open
                        } else {
                            h.target_open
                        }
                    });
                    if distance <= 3.0 {
                        if let Some(motion) = motion {
                            ui.small(format!(
                                "{label}: {:.0}% {}",
                                motion.fraction * 100.0,
                                if motion.blocked {
                                    "held · clear sweep and retry"
                                } else if motion.moving {
                                    "moving"
                                } else {
                                    "stopped"
                                }
                            ));
                        }
                    }
                    let lift_at_ship = !state.actuators.lift.moving
                        && (state.actuators.lift.floor_station_m
                            - lift_geometry(pose).ship_floor_m)
                            .abs()
                            < 0.03;
                    let hatch_ready = if open {
                        !hatch_occupied(
                            pose,
                            match hatch {
                                DockedHatch::Inner => PioneerDockedHatch::Inner,
                                DockedHatch::Outer => PioneerDockedHatch::Outer,
                            },
                            state.feet_m,
                        )
                    } else {
                        match hatch {
                            DockedHatch::Inner => state.actuators.hatch_closed(1),
                            DockedHatch::Outer => state.actuators.hatch_closed(0) && lift_at_ship,
                        }
                    };
                    if distance <= 3.0
                        && ui
                            .add_enabled(
                                ready && hatch_ready && state.actuators.service_power_available,
                                egui::Button::new(format!(
                                    "{} {label}",
                                    if open { "Close" } else { "Open" }
                                )),
                            )
                            .clicked()
                    {
                        self.send(Request::DockedTraversal {
                            request: DockedTraversalRequest::SetHatch {
                                binding: binding.clone(),
                                hatch,
                                open: !open,
                            },
                        });
                    }
                }
                if lift_distance <= 3.0 || state.support == DockedSupport::BoardingLift {
                    for (stop, label) in [
                        (DockedLiftStop::Ship, "Lift to ship"),
                        (DockedLiftStop::Station, "Lift to station"),
                    ] {
                        if ui
                            .add_enabled(
                                ready
                                    && state.actuators.service_power_available
                                    && !state.actuators.lift.moving
                                    && state.actuators.hatches_closed(),
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            self.send(Request::DockedTraversal {
                                request: DockedTraversalRequest::CallLift {
                                    binding: binding.clone(),
                                    stop,
                                },
                            });
                        }
                    }
                }
            });
            ui.small(docked_exit_guidance(state));
            ui.small(
                if self
                    .snapshot
                    .as_ref()
                    .is_some_and(|snapshot| snapshot.walk_sprint_v1)
                {
                    "W/S or arrows move/turn · Q/E strafe · RMB turn · Shift sprint · Space jump"
                } else {
                    "W/S or arrows move/turn · Q/E strafe · RMB turn · Space jump"
                },
            );
            if !state.actuators.service_power_available {
                ui.small("Docking service power unavailable · lift held in place.");
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.strong("PIONEER · PILOT SEAT");
                if ui
                    .add_enabled(self.can_operate(), egui::Button::new("Leave pilot seat"))
                    .clicked()
                {
                    self.send(Request::DockedTraversal {
                        request: DockedTraversalRequest::LeaveSeat {
                            binding: binding.clone(),
                        },
                    });
                }
                if ui
                    .add_enabled(self.can_operate(), egui::Button::new("Undock"))
                    .clicked()
                {
                    self.send(Request::Undock);
                }
                self.view_selector(ui);
            });
            ui.small("Exit to hangar: leave the seat and walk to the port boarding vestibule, then use the hatches and lift. Shift + arrows look around while seated.");
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
        self.manual_remote.is_none()
            && self.own().is_some_and(|p| {
                p.location == Location::Docked
                    && (p.station_avatar.is_some()
                        // Registered cockpit geometry uses station metres;
                        // aboard external/tactical cameras keep their own renderer.
                        || if self.docked_navigation_present() {
                            self.view_mode == ViewMode::Cockpit
                        } else {
                            !self.embedded
                        })
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
                ui.small("W/S or Up/Down move · A/D or Left/Right turn · Q/E strafe · RMB look · Space jump");
                ui.small(
                    "Airlock engineering uses server-confirmed devices. Other room fixtures remain prototypes.",
                );
                if !primary_promoted {
                    self.interior_exit_action(ui, avatar, interior_available, distance);
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
            ui.small(
                "W/S or Up/Down move · A/D or Left/Right turn · Q/E strafe · hold RMB to look · Space jump",
            );
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
            if avatar.interior_deck != 1 {
                let lift_distance = avatar
                    .position
                    .iter()
                    .zip(PIONEER_LIFT_POINT)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let lift_enabled = self.can_operate()
                    && self
                        .assets
                        .as_ref()
                        .is_some_and(|assets| assets.pioneer_interior_decks >= 2)
                    && lift_distance <= PIONEER_LIFT_RANGE
                    && avatar.jump_started_ms.is_none();
                if ui
                    .add_enabled(lift_enabled, egui::Button::new("Lift to deck 1").small())
                    .clicked()
                {
                    self.send(Request::InteriorLift { deck: 1 });
                }
                ui.small(format!("Lift · {lift_distance:.1} m"));
            } else {
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
                egui::RichText::new("W/S move · A/D turn · Q/E strafe · RMB look · Space jump")
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
                egui::RichText::new("W/S move · A/D turn · Q/E strafe · RMB look · Space jump")
                    .small(),
            )
            .wrap(),
        );
    }

    pub(super) fn station_overlay(&mut self, ctx: &egui::Context) {
        if self.docked_navigation_present() {
            if self.embedded {
                egui::TopBottomPanel::bottom("embedded_docked_traversal_actions")
                    .height_range(48.0..=148.0)
                    .resizable(false)
                    .frame(self.overlay_frame(ctx))
                    .show(ctx, |ui| {
                        self.docked_station_actions(ui);
                    });
            } else {
                egui::Window::new("Pioneer docked journey")
                    .id(egui::Id::new("docked_traversal_actions"))
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -14.0])
                    .resizable(false)
                    .collapsible(false)
                    .movable(false)
                    .frame(self.overlay_frame(ctx))
                    .show(ctx, |ui| {
                        self.docked_station_actions(ui);
                    });
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
}
