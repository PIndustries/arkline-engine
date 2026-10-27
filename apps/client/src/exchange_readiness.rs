//! UI explanations for existing authoritative exchange commands.
use super::*;

pub(super) enum ExchangeAction {
    List { units: u64, unit_price: u64 },
    Buy { order_id: u64, units: u64 },
    Cancel { order_id: u64 },
}
impl Cockpit {
    pub(super) fn incoming_ore_headroom(pilot: &PilotView) -> Result<u64, &'static str> {
        let capacity = cargo_capacity(pilot.ship_class);
        match &pilot.drone_cargo {
            Some(cargo) => {
                cargo.validate_projection(pilot).map_err(|_| {
                    "Incoming drone cargo is unconfirmed · wait for matching hold reservations."
                })?;
                cargo
                    .available_for_new_ore(pilot.ore, capacity)
                    .ok_or("Hold reservations exceed confirmed capacity · wait for authority.")
            }
            None => capacity
                .checked_sub(pilot.ore)
                .ok_or("Hold contents exceed confirmed capacity."),
        }
    }
    /// Command buttons must use live telemetry age, never frozen presentation time.
    pub(super) fn live_command_snapshot_fresh(&self) -> bool {
        self.connected
            && self.snapshot.as_ref().is_some_and(|snapshot| {
                self.timeline_origin.is_some_and(|origin| {
                    origin
                        .saturating_add(self.observed.elapsed().as_millis() as u64)
                        .saturating_sub(snapshot.server_time_ms)
                        <= 1500
                })
            })
    }
    pub(super) fn navigation_order_reason(&self) -> Option<&'static str> {
        if !self.connected {
            Some("Navigation orders unavailable · disconnected from authority.")
        } else if self.embedded_paused {
            Some("Navigation orders unavailable · resume simulation first. Contacts remain available for inspection.")
        } else if self.request_pending {
            Some("Navigation orders unavailable · awaiting the previous command response.")
        } else if !self.live_command_snapshot_fresh() {
            Some("Navigation orders unavailable · wait for a fresh authority snapshot. Contacts and last reported ranges remain visible.")
        } else if self.own().is_none() {
            Some("Navigation orders unavailable · waiting for the controlled vessel.")
        } else if self.own().is_some_and(|p| p.station_avatar.is_some()) {
            Some("Read-only navigation while on foot · physically return to the pilot seat for ship orders.")
        } else {
            None
        }
    }
    /// Public, source-bound state belongs to the actual controlled vessel. No
    /// controller-private stock is borrowed when observing/controlling a worker.
    fn bay_carrier_reason(&self, departure: bool) -> Option<&'static str> {
        let pilot = self.own()?;
        if let Some(cargo) = &pilot.drone_cargo {
            if !self.live_command_snapshot_fresh() || cargo.validate_projection(&pilot).is_err() {
                return Some("Ore receiver state unconfirmed · wait for current vessel/source telemetry. STOP remains available.");
            }
            if let Some(cleanup) = &cargo.cleanup {
                return Some(if cleanup.held {
                    "Ore receiver cleanup held · resume safe disconnection in Drone operations. STOP remains available."
                } else {
                    "Ore receiver cleanup · carrier motion stays locked until hardware is safe. STOP remains available."
                });
            }
            if let Some(id) = cargo.receiver_operation {
                return Some(
                    if cargo
                        .operation(id)
                        .is_some_and(|op| op.phase == arkline_protocol::DroneDropoffPhase::Held)
                    {
                        "Ore drop-off held · Resume or Cancel in Drone operations before moving. STOP remains available."
                    } else {
                        "Ore receiver handling · carrier motion locked through approach, transfer and safe departure. STOP remains available."
                    },
                );
            }
        }
        let bay = pilot.drone_bay.as_ref()?;
        if !self.live_command_snapshot_fresh() || !flight3d::bay_view_available(&pilot) {
            return Some("Drone bay state unconfirmed · wait for current vessel/source telemetry. STOP remains available.");
        }
        if let Some(cleanup) = &bay.machinery.cleanup {
            return Some(if cleanup.held {
                "Bay cleanup held · resume empty hardware in Drone operations before moving. STOP remains available."
            } else {
                "Bay cleanup in progress · empty hardware must return safe before moving. STOP remains available."
            });
        }
        if let Some(id) = bay.machinery.handling_operation {
            return Some(
                if bay.motions.iter().any(|m| {
                    m.operation_id == id && m.phase == arkline_protocol::DroneBayPhase::Held
                }) {
                    "Drone bay held · use Drone operations to Resume or Cancel. STOP remains available."
                } else {
                    "Drone bay handling · carrier motion locked; use Drone operations to Hold or Cancel. STOP remains available."
                },
            );
        }
        if let Some(batch) = bay.batch.as_ref().filter(|batch| batch.active()) {
            return Some(if batch.held {
                "Drone batch held · carrier motion locked between members; Resume or Cancel in Drone operations. STOP remains available."
            } else if batch.phase == arkline_protocol::DroneBayBatchPhase::Cancelling {
                "Drone batch cancelling · carrier remains locked until the queue and hardware are safe. STOP remains available."
            } else {
                "Drone batch running · carrier motion locked between members; Hold or Cancel in Drone operations. STOP remains available."
            });
        }
        (departure && bay.cradles.iter().any(|c|c.presence==arkline_protocol::DroneBayPresence::Active))
            .then_some("Drones deployed · recover all owned drones before warp, jump or docking. Normal flight and STOP remain available.")
    }
    pub(super) fn bay_maneuver_reason(&self) -> Option<&'static str> {
        self.bay_carrier_reason(false)
    }
    pub(super) fn bay_departure_reason(&self) -> Option<&'static str> {
        self.bay_carrier_reason(true)
    }
    pub(super) fn bay_command_reason(&self, request: &Request) -> Option<&'static str> {
        match request {
            Request::StartResourceJob { pilot, .. } => {
                let target = pilot.as_deref().unwrap_or_else(|| self.controlled_name());
                let viewed = self
                    .snapshot
                    .as_ref()?
                    .pilots
                    .iter()
                    .find(|p| p.name == target)?;
                if self.live_command_snapshot_fresh()
                    && viewed.drone_cargo.as_ref().is_some_and(|cargo| {
                        cargo.validate_projection(viewed).is_ok()
                            && (cargo.cleanup.is_some()
                                || cargo.operations.iter().any(|op| !op.phase.terminal()))
                    })
                {
                    return Some("Ore drop-off active on the job's target vessel · finish or cancel that service before starting its resource job.");
                }
                (self.live_command_snapshot_fresh()
                    && flight3d::bay_view_available(viewed)
                    && viewed.drone_bay.as_ref()?.batch.as_ref().is_some_and(|b| b.active()))
                    .then_some("Drone batch active on the job's target vessel · finish or cancel the batch before starting its resource job.")
            }
            Request::Navigate { mode } if !matches!(mode, NavigationMode::Stop) => {
                self.bay_maneuver_reason()
            }
            Request::PilotInput {
                translation,
                rotation,
            } if translation.iter().chain(rotation).any(|v| *v != 0.) => self.bay_maneuver_reason(),
            Request::Undock => self.bay_maneuver_reason(),
            Request::Warp { .. }
            | Request::Jump { .. }
            | Request::Dock
            | Request::SelectShip { .. }
            | Request::BoardShip { .. } => self.bay_departure_reason(),
            _ => None,
        }
    }

    /// Private cargo receipts belong to the authenticated vessel, never a remote worker.
    /// Unknown capabilities and identity mismatches are left to the authority.
    pub(super) fn cargo_departure_reason(&self) -> Option<&'static str> {
        if self.controlled_name() != self.pilot {
            return None;
        }
        let pilot = self.own()?;
        let state = self.assets.as_ref()?.cargo_unloading.as_ref()?;
        if state.schema_version != arkline_protocol::CARGO_UNLOADING_SCHEMA_VERSION {
            return None;
        }
        let operation = state.active()?;
        let docking = pilot.flight.as_ref()?.docking.as_ref()?;
        (operation.reserved_units > 0
            && operation.pilot == self.pilot
            && operation.ship_class == pilot.ship_class
            && operation.system_id == pilot.system_id
            && docking.version == arkline_protocol::DOCKING_VERSION
            && !docking.host_vessel_id.is_empty()
            && operation.vessel_id == docking.host_vessel_id
            && operation.berth_id == docking.berth_id)
            .then_some(
                "Cargo reserved · finish or cancel unloading in Cargo & storage before undocking.",
            )
    }

    /// Departure alone needs an unreserved hold. STOP, docking HOLD/RESUME,
    /// inspection, traversal and cargo controls keep their existing readiness.
    pub(super) fn departure_order_reason(&self) -> Option<&'static str> {
        self.navigation_order_reason()
            .or_else(|| self.cargo_departure_reason())
            .or_else(|| self.bay_maneuver_reason())
    }

    pub(super) fn exchange_readiness_reason(&self) -> Option<&'static str> {
        if !self.connected {
            Some("Disconnected · reconnect before trading.")
        } else if self.embedded_paused {
            Some("Resume simulation before trading. Opening Play does not advance time.")
        } else if self.request_pending {
            Some("Awaiting authority · previous command pending.")
        } else if !self.live_command_snapshot_fresh() {
            Some("Exchange data may be stale · wait for a fresh authority snapshot.")
        } else if self.manual_remote.is_some() {
            Some("Return to your own ship before trading.")
        } else if self.own().is_none() {
            Some("Waiting for the authenticated ship and inventory.")
        } else if self
            .own()
            .is_some_and(|pilot| pilot.location != Location::Docked)
        {
            Some("Dock at the station before trading.")
        } else {
            None
        }
    }

    pub(super) fn exchange_action_reason(&self, action: ExchangeAction) -> Option<&'static str> {
        if let Some(reason) = self.exchange_readiness_reason() {
            return Some(reason);
        }
        let pilot = self.own()?;
        match action {
            ExchangeAction::List { units, unit_price } => {
                if units == 0 {
                    Some("Choose at least one ore unit to list.")
                } else if unit_price == 0 {
                    Some("Choose a positive price per unit.")
                } else if units.checked_mul(unit_price).is_none() {
                    Some("Total listing price exceeds the supported value.")
                } else if units > pilot.ore {
                    Some("Not enough ore aboard to list this quantity.")
                } else {
                    None
                }
            }
            ExchangeAction::Buy { order_id, units } => {
                let available = match Self::incoming_ore_headroom(&pilot) {
                    Ok(available) => available,
                    Err(reason) => return Some(reason),
                };
                let Some(order) = self
                    .snapshot
                    .as_ref()?
                    .orders
                    .iter()
                    .find(|order| order.id == order_id)
                else {
                    return Some("Listing is no longer reported · wait for updated orders.");
                };
                if order.seller == self.pilot {
                    Some("You cannot buy your own listing.")
                } else if units == 0 {
                    Some("Choose at least one ore unit to buy.")
                } else if units > order.units {
                    Some("Listing contains fewer ore units than requested.")
                } else if units > available {
                    Some(if pilot.drone_cargo.is_some() {
                        "Not enough unreserved hold capacity · incoming drone ore keeps its reserved space."
                    } else {
                        "Not enough free hold capacity for this purchase."
                    })
                } else if let Some(cost) = order.unit_price.checked_mul(units) {
                    (cost > pilot.credits).then_some("Not enough credits for this purchase.")
                } else {
                    Some("Purchase price exceeds the supported value.")
                }
            }
            ExchangeAction::Cancel { order_id } => {
                let available = match Self::incoming_ore_headroom(&pilot) {
                    Ok(available) => available,
                    Err(reason) => return Some(reason),
                };
                let Some(order) = self
                    .snapshot
                    .as_ref()?
                    .orders
                    .iter()
                    .find(|order| order.id == order_id)
                else {
                    return Some("Listing is no longer reported · wait for updated orders.");
                };
                if order.seller != self.pilot {
                    Some("Only the seller can cancel this listing.")
                } else if order.units > available {
                    Some(if pilot.drone_cargo.is_some() {
                        "Free unreserved hold space for listed ore · incoming drone cargo cannot be displaced."
                    } else {
                        "Free hold space for all listed ore before cancelling."
                    })
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_live_readiness_keeps_readouts_but_blocks_stale_target_orders() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.selected_object = SelectedObject::Station;
        cockpit.frozen_time = Some(1000);
        cockpit.observed = crate::Instant::now() - std::time::Duration::from_secs(2);
        let (tx, rx) = mpsc::channel();
        cockpit.commands = Some(tx);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        let render = |c: &mut Cockpit, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(900., 800.))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| c.navigation_controls(ui));
                },
            )
        };
        let _ = render(&mut cockpit, vec![]); // settle the changed typography/layout before pointer coordinates
        let output = render(&mut cockpit, vec![]);
        let text: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(t) = &shape.shape {
                    Some((
                        t.galley.job.text.clone(),
                        t.pos + t.galley.rect.center().to_vec2(),
                    ))
                } else {
                    None
                }
            })
            .collect();
        assert!(text
            .iter()
            .any(|(t, _)| t.contains("fresh authority snapshot")));
        assert!(
            text.iter().any(|(t, _)| t.starts_with("Range ")),
            "readouts: {:?}",
            text.iter().map(|(t, _)| t).collect::<Vec<_>>()
        );
        let point = text.iter().find(|(t, _)| t == "Approach").unwrap().1;
        for pressed in [true, false] {
            render(
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
        assert!(rx.try_recv().is_err());
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
        assert!(matches!(cockpit.selected_object, SelectedObject::Station));
        cockpit.embedded_paused = true;
        assert!(cockpit
            .navigation_order_reason()
            .unwrap()
            .contains("resume simulation first"));
        cockpit.request_pending = true;
        assert!(cockpit
            .navigation_order_reason()
            .unwrap()
            .contains("resume simulation first"));
        cockpit.connected = false;
        assert!(cockpit
            .navigation_order_reason()
            .unwrap()
            .contains("disconnected"));
    }
    #[test]
    fn exchange_readiness_reports_stale_pause_pending_and_actual_action_limits() {
        let ctx = egui::Context::default();
        let mut cockpit = crate::tests::view_test_cockpit(&ctx);
        cockpit.snapshot.as_mut().unwrap().pilots[0].location = Location::Docked;
        cockpit.snapshot.as_mut().unwrap().pilots[0].ore = 10;
        cockpit.snapshot.as_mut().unwrap().pilots[0].credits = 100;
        cockpit.snapshot.as_mut().unwrap().orders = vec![
            SellOrder {
                id: 1,
                seller: "other".into(),
                units: 20,
                unit_price: 10,
            },
            SellOrder {
                id: 2,
                seller: cockpit.pilot.clone(),
                units: 5,
                unit_price: 10,
            },
        ];
        assert!(cockpit
            .exchange_action_reason(ExchangeAction::List {
                units: 10,
                unit_price: 1
            })
            .is_none());
        assert!(cockpit
            .exchange_action_reason(ExchangeAction::Buy {
                order_id: 1,
                units: 21
            })
            .unwrap()
            .contains("fewer"));
        assert!(cockpit
            .exchange_action_reason(ExchangeAction::Buy {
                order_id: 1,
                units: 11
            })
            .unwrap()
            .contains("credits"));
        assert!(cockpit
            .exchange_action_reason(ExchangeAction::Cancel { order_id: 1 })
            .unwrap()
            .contains("seller"));
        assert!(cockpit
            .exchange_action_reason(ExchangeAction::Cancel { order_id: 2 })
            .is_none());
        let capacity = cargo_capacity(cockpit.snapshot.as_ref().unwrap().pilots[0].ship_class);
        cockpit.snapshot.as_mut().unwrap().pilots[0].ore = capacity;
        assert!(cockpit
            .exchange_action_reason(ExchangeAction::Buy {
                order_id: 1,
                units: 1
            })
            .unwrap()
            .contains("capacity"));
        assert!(cockpit
            .exchange_action_reason(ExchangeAction::Cancel { order_id: 2 })
            .unwrap()
            .contains("hold space"));
        // Freeze visuals at the last snapshot while the live telemetry clock
        // advances. The visual freeze must not keep old listings actionable.
        let snapshot_time = cockpit.snapshot.as_ref().unwrap().server_time_ms;
        cockpit.frozen_time = Some(snapshot_time);
        cockpit.timeline_origin = Some(snapshot_time);
        cockpit.observed = Instant::now() - Duration::from_secs(2);
        assert_eq!(cockpit.presentation_time(), snapshot_time);
        let before = serde_json::to_value(&cockpit.snapshot).unwrap();
        for action in [
            ExchangeAction::List {
                units: 1,
                unit_price: 1,
            },
            ExchangeAction::Buy {
                order_id: 1,
                units: 1,
            },
            ExchangeAction::Cancel { order_id: 2 },
        ] {
            assert!(cockpit
                .exchange_action_reason(action)
                .unwrap()
                .contains("stale"));
        }
        cockpit.embedded_paused = true;
        assert!(cockpit
            .exchange_readiness_reason()
            .unwrap()
            .contains("Resume simulation"));
        cockpit.embedded_paused = false;
        cockpit.frozen_time = None;
        cockpit.request_pending = true;
        assert!(cockpit
            .exchange_readiness_reason()
            .unwrap()
            .contains("pending"));
        cockpit.connected = false;
        assert!(cockpit
            .exchange_readiness_reason()
            .unwrap()
            .contains("Disconnected"));
        assert_eq!(serde_json::to_value(&cockpit.snapshot).unwrap(), before);
    }
    #[test]
    fn cargo_departure_phase_identity_schema_and_readiness_matrix() {
        use arkline_protocol::CargoUnloadPhase::*;
        let ctx = egui::Context::default();
        let mut c = crate::tests::view_test_cockpit(&ctx);
        for phase in [Unloading, Held, Unloading, Cancelled, Completed] {
            crate::tests::cargo_departure_fixture(&mut c, phase);
            assert!(c.navigation_order_reason().is_none());
            assert_eq!(c.departure_order_reason().is_some(), phase.active());
        }
        for mismatch in [
            "schema",
            "pilot",
            "class",
            "vessel",
            "system",
            "berth",
            "docking-version",
            "no-docking",
            "empty-host",
            "no-remainder",
            "legacy",
            "no-assets",
            "remote",
        ] {
            let mut c = crate::tests::view_test_cockpit(&ctx);
            crate::tests::cargo_departure_fixture(&mut c, Held);
            let state = c.assets.as_mut().unwrap().cargo_unloading.as_mut().unwrap();
            let op = &mut state.operations[0];
            match mismatch {
                "schema" => state.schema_version = 99,
                "pilot" => op.pilot = "other".into(),
                "class" => op.ship_class = ShipClass::Prospector,
                "vessel" => op.vessel_id = "other-vessel".into(),
                "system" => op.system_id += 1,
                "berth" => op.berth_id += 1,
                "docking-version" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .flight
                        .as_mut()
                        .unwrap()
                        .docking
                        .as_mut()
                        .unwrap()
                        .version = 99
                }
                "no-docking" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .flight
                        .as_mut()
                        .unwrap()
                        .docking = None
                }
                "empty-host" => {
                    op.vessel_id.clear();
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .flight
                        .as_mut()
                        .unwrap()
                        .docking
                        .as_mut()
                        .unwrap()
                        .host_vessel_id
                        .clear();
                }
                "no-remainder" => op.reserved_units = 0,
                "legacy" => c.assets.as_mut().unwrap().cargo_unloading = None,
                "no-assets" => c.assets = None,
                "remote" => {
                    let snapshot = c.snapshot.as_mut().unwrap();
                    let mut worker = snapshot.pilots[0].clone();
                    worker.name = "Worker".into();
                    snapshot.pilots.push(worker);
                    snapshot
                        .remote_controls
                        .push(arkline_protocol::RemoteControlLease {
                            controller: c.pilot.clone(),
                            pilot: "Worker".into(),
                        });
                    c.manual_remote = Some("Worker".into());
                    assert_eq!(c.controlled_name(), "Worker");
                }
                _ => unreachable!(),
            }
            assert!(c.departure_order_reason().is_none(), "{mismatch}");
            let (tx, rx) = mpsc::channel();
            c.commands = Some(tx);
            c.send(Request::Undock);
            let request = rx
                .try_recv()
                .unwrap_or_else(|_| panic!("{mismatch} incorrectly blocked"));
            if mismatch == "remote" {
                assert!(
                    matches!(request, Request::RemoteControl { pilot, request } if pilot == "Worker" && matches!(*request, Request::Undock))
                );
            } else {
                assert!(matches!(request, Request::Undock));
            }
        }
        for gate in ["offline", "paused", "pending", "stale", "on-foot"] {
            let mut c = crate::tests::view_test_cockpit(&ctx);
            crate::tests::cargo_departure_fixture(&mut c, Held);
            match gate {
                "offline" => c.connected = false,
                "paused" => c.embedded_paused = true,
                "pending" => c.request_pending = true,
                "stale" => { c.frozen_time = Some(1000); c.observed = Instant::now() - Duration::from_secs(2); }
                "on-foot" => c.snapshot.as_mut().unwrap().pilots[0].station_avatar = Some(serde_json::from_value(serde_json::json!({"position":[0,0,0],"heading":0,"moving":false,"updated_at_ms":1000})).unwrap()),
                _ => unreachable!(),
            }
            assert!(c.navigation_order_reason().is_some(), "{gate}");
            assert_eq!(
                c.departure_order_reason(),
                c.navigation_order_reason(),
                "{gate}"
            );
        }
    }

    #[test]
    fn cargo_departure_send_guard_preserves_input_ai_and_authority_state() {
        let mut c = crate::tests::view_test_cockpit(&egui::Context::default());
        crate::tests::cargo_departure_fixture(&mut c, arkline_protocol::CargoUnloadPhase::Held);
        let (tx, rx) = mpsc::channel();
        c.commands = Some(tx);
        let input = pilot_input::Controls {
            translation: [1., 0., 0.],
            rotation: [0.; 3],
        };
        c.pilot_input.update(true, input);
        c.controls.publish(input);
        c.station_walk_active = true;
        c.ai_pin = Some((c.pilot.clone(), c.connection_generation));
        let ai_status = c.ship_ai.status().to_owned();
        let before = serde_json::to_value((&c.snapshot, &c.assets)).unwrap();
        for _ in 0..3 {
            c.send(Request::Undock);
        }
        assert!(c.notice.starts_with("Cargo reserved"));
        assert!(!c.request_pending);
        assert!(c.station_walk_active);
        assert!(c.ai_pin.is_some());
        assert_eq!(c.ship_ai.status(), ai_status);
        assert_eq!(
            c.pilot_input.update(false, Default::default()),
            Some(Default::default())
        );
        assert!(
            matches!(c.controls.take(), Some(Request::PilotInput { translation, .. }) if translation == [1., 0., 0.])
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(
            serde_json::to_value((&c.snapshot, &c.assets)).unwrap(),
            before
        );
        for request in [
            Request::CancelCargoUnload { operation_id: 1 },
            Request::ResumeCargoUnload { operation_id: 1 },
        ] {
            c.send(request.clone());
            assert_eq!(
                serde_json::to_value(rx.try_recv().unwrap()).unwrap(),
                serde_json::to_value(request).unwrap()
            );
            c.request_pending = false;
        }
    }
}

#[cfg(test)]
mod bay_carrier_interlock_tests {
    use super::*;
    use arkline_protocol::{drone_bay_layout as geometry, *};

    fn fixture(ctx: &egui::Context, state: &str) -> Cockpit {
        let mut c = crate::tests::view_test_cockpit(ctx);
        let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
        p.location = Location::Station;
        let f = p.flight.as_mut().unwrap();
        f.position = docking_gate_position(0);
        f.docking=Some(serde_json::from_value(serde_json::json!({"version":1,"host_vessel_id":"owned-pioneer","berth_id":0,"operation_id":0,"phase":"free","segment":null,"clamps":0.,"updated_at_ms":1000,"hold_reason":null,"departing":false})).unwrap());
        let binding = DroneBayBinding {
            vessel_id: "owned-pioneer".into(),
            geometry_revision: geometry::DRONE_BAY_REGISTRATION_REVISION,
            source_sha256: geometry::DRONE_BAY_SOURCE_SHA256.into(),
        };
        let mut bay = DroneBayPublicState {
            batch: None,
            batch_capability: None,
            schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
            binding,
            cradles: geometry::home_poses()
                .into_iter()
                .enumerate()
                .map(|(i, body_pose)| DroneCradleView {
                    drone_id: 101 + i as u64,
                    class: DroneBayState::expected_class(i as u8).unwrap(),
                    hit_points: 37,
                    home_cradle: i as u8,
                    presence: DroneBayPresence::Stored,
                    body_pose,
                })
                .collect(),
            motions: vec![],
            machinery: DroneBayMachineryView {
                cleanup: None,
                handling_operation: None,
                door_fraction: 0.,
                door_segment: None,
                groups: geometry::mechanism_ids()
                    .into_iter()
                    .zip(geometry::reference_poses())
                    .map(|(id, pose)| DroneBayMechanismView {
                        semantic_id: id.into(),
                        confirmed_pose: pose,
                        segment: None,
                    })
                    .collect(),
            },
        };
        match state {
            "active" | "handling" | "held" => {
                bay.cradles[0].presence = DroneBayPresence::Active;
                p.drones=vec![serde_json::from_value(serde_json::json!({"id":101,"class":"recon","hit_points":37,"signal_strength":1.,"position":f.position,"orientation_xyzw":[0.,0.,0.,1.],"velocity":[0.,0.,0.],"heading":[1.,0.,0.],"mode":{"kind":"hold"},"updated_at_ms":1000})).unwrap()];
                if state != "active" {
                    bay.machinery.handling_operation = Some(1);
                    bay.motions.push(DroneBayMotionView {
                        drone_id: 101,
                        operation_id: 1,
                        phase: if state == "held" {
                            DroneBayPhase::Held
                        } else {
                            DroneBayPhase::TransferringOut
                        },
                        frame: DroneBayMotionFrame::Body,
                        confirmed_pose: bay.cradles[0].body_pose,
                        segment: None,
                        updated_at_ms: 1000,
                    });
                }
            }
            "cleanup" | "cleanup-held" => {
                bay.cradles[0].presence = DroneBayPresence::Destroyed;
                bay.cradles[0].hit_points = 0;
                bay.machinery.cleanup = Some(DroneBayCleanupView {
                    operation_id: 1,
                    held: state == "cleanup-held",
                    reason: "Empty hardware remains".into(),
                });
            }
            "stored" => {}
            _ => panic!("unknown fixture"),
        }
        p.drone_bay = Some(bay);
        assert!(flight3d::bay_view_available(&c.own().unwrap()));
        c
    }
    #[test]
    fn bay_carrier_readiness_distinguishes_busy_deployed_stored_and_unknown() {
        for state in [
            "stored",
            "active",
            "handling",
            "held",
            "cleanup",
            "cleanup-held",
        ] {
            let ctx = egui::Context::default();
            let c = fixture(&ctx, state);
            let busy = !matches!(state, "stored" | "active");
            assert!(
                c.can_fly(),
                "seated STOP and bay controls must stay available: {state}"
            );
            assert!(
                c.navigation_order_reason().is_none(),
                "must not disable whole navigation parent"
            );
            assert_eq!(c.can_maneuver(), !busy, "{state}");
            assert_eq!(c.can_warp(), state == "stored", "{state}");
            assert_eq!(c.can_dock(&c.own().unwrap()), state == "stored", "{state}");
            assert_eq!(
                c.departure_order_reason().is_some(),
                busy,
                "Undock busy only: {state}"
            );
            assert_eq!(c.bay_departure_reason().is_some(), state != "stored");
            if busy {
                assert!(c.compact_control_label(true).contains("BAY INTERLOCK"));
                assert_eq!(c.operation_status().0, "DRONE BAY INTERLOCK");
            }
        }
        let ctx = egui::Context::default();
        let mut requested = fixture(&ctx, "held");
        requested.manual_remote = Some("unconfirmed-worker".into());
        assert_eq!(
            requested.compact_control_label(true),
            "REMOTE REQUEST / NO CONTROL",
            "bay status must not conceal missing remote authority"
        );
        requested.ship_observation = true;
        assert_eq!(requested.compact_control_label(true), "OBSERVATION ONLY");
        assert_eq!(
            requested.compact_control_label(false),
            "NO CONFIRMED CONTROL"
        );
        for invalid in ["source", "vessel", "schema", "stale"] {
            let ctx = egui::Context::default();
            let mut c = fixture(&ctx, "stored");
            match invalid {
                "source" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .drone_bay
                        .as_mut()
                        .unwrap()
                        .binding
                        .source_sha256 = "0".repeat(64)
                }
                "vessel" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .drone_bay
                        .as_mut()
                        .unwrap()
                        .binding
                        .vessel_id = "other".into()
                }
                "schema" => {
                    c.snapshot.as_mut().unwrap().pilots[0]
                        .drone_bay
                        .as_mut()
                        .unwrap()
                        .schema_version = 999
                }
                "stale" => {
                    c.observed = Instant::now() - Duration::from_secs(10);
                    c.frozen_time = Some(1000);
                }
                _ => unreachable!(),
            }
            assert!(!c.can_maneuver(), "{invalid}");
            assert!(!c.can_warp());
            assert!(c.bay_maneuver_reason().unwrap().contains("unconfirmed"));
        }
    }
    #[test]
    fn bay_command_guard_preserves_pending_ai_mailbox_and_allows_stop_and_bay_control() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx, "cleanup-held");
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        c.ai_pin = Some(("pending interpretation".into(), 7));
        c.voice_pin = Some(("pending voice".into(), 8));
        let before = serde_json::to_value(&c.snapshot).unwrap();
        let ai_status = c.ship_ai.status().to_owned();
        for request in [
            Request::Navigate {
                mode: NavigationMode::FlyTo {
                    position: [1., 2., 3.],
                },
            },
            Request::PilotInput {
                translation: [1., 0., 0.],
                rotation: [0.; 3],
            },
            Request::Warp {
                destination: Destination::Belt,
            },
            Request::Dock,
            Request::Undock,
            Request::Jump { gate_id: 1 },
        ] {
            c.controls.publish(pilot_input::Controls {
                translation: [1., 0., 0.],
                rotation: [0.; 3],
            });
            c.send(request);
            assert!(rx.try_recv().is_err());
            assert!(!c.request_pending);
            assert_eq!(c.ai_pin, Some(("pending interpretation".into(), 7)));
            assert_eq!(c.voice_pin, Some(("pending voice".into(), 8)));
            assert_eq!(c.ship_ai.status(), ai_status);
            assert!(
                matches!(
                    c.controls.take(),
                    Some(Request::PilotInput {
                        translation: [1., 0., 0.],
                        ..
                    })
                ),
                "blocked order must not relinquish input"
            );
            assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
        }
        let binding = c.own().unwrap().drone_bay.unwrap().binding;
        for request in [
            Request::Navigate {
                mode: NavigationMode::Stop,
            },
            Request::PilotInput {
                translation: [0.; 3],
                rotation: [0.; 3],
            },
            Request::DroneBay {
                request: DroneBayRequest::Resume {
                    binding: binding.clone(),
                    operation_id: 1,
                },
            },
            Request::DroneBay {
                request: DroneBayRequest::Hold {
                    binding: binding.clone(),
                    operation_id: 1,
                },
            },
            Request::DroneBay {
                request: DroneBayRequest::Cancel {
                    binding,
                    operation_id: 1,
                },
            },
        ] {
            c.send(request.clone());
            assert_eq!(
                serde_json::to_value(rx.try_recv().unwrap()).unwrap(),
                serde_json::to_value(request).unwrap()
            );
            c.request_pending = false;
        }
        assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
    }
    #[test]
    fn physical_berth_is_blocked_but_real_stop_remains_reachable_with_bay_status() {
        for state in [
            "stored",
            "active",
            "handling",
            "held",
            "cleanup",
            "cleanup-held",
        ] {
            for stop in [false, true] {
                let ctx = egui::Context::default();
                let mut c = fixture(&ctx, state);
                let (tx, rx) = std::sync::mpsc::channel();
                c.commands = Some(tx);
                let before = serde_json::to_value(&c.snapshot).unwrap();
                let mut frame = |events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                Pos2::ZERO,
                                Vec2::new(600., 220.),
                            )),
                            events,
                            focused: true,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| c.cockpit_actions(ui));
                        },
                    )
                };
                frame(vec![]);
                let output = frame(vec![]);
                if state != "stored" {
                    let expected = if state == "active" {
                        "DRONES DEPLOYED"
                    } else {
                        "BAY INTERLOCK"
                    };
                    assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text==expected)),"visible {expected}");
                }
                let pos = Pos2::new(if stop { 212. } else { 78. }, 51.);
                frame(vec![egui::Event::PointerMoved(pos)]);
                for pressed in [true, false] {
                    frame(vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }]);
                }
                if stop {
                    assert!(matches!(
                        rx.try_recv().unwrap(),
                        Request::Navigate {
                            mode: NavigationMode::Stop
                        }
                    ));
                } else if state == "stored" {
                    assert!(matches!(rx.try_recv().unwrap(), Request::Dock));
                } else {
                    assert!(rx.try_recv().is_err());
                    assert!(!c.request_pending);
                }
                assert!(rx.try_recv().is_err());
                assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
            }
        }
    }
    #[test]
    fn navigation_real_buttons_distinguish_maneuver_departure_and_stop_including_legacy() {
        for state in ["legacy", "stored", "active", "held", "cleanup-held"] {
            for label in ["Approach", "Warp to belt", "Stop"] {
                let ctx = egui::Context::default();
                let mut c = if state == "legacy" {
                    crate::tests::view_test_cockpit(&ctx)
                } else {
                    fixture(&ctx, state)
                };
                let before = serde_json::to_value(&c.snapshot).unwrap();
                let (tx, rx) = std::sync::mpsc::channel();
                c.commands = Some(tx);
                let mut frame = |events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                Pos2::ZERO,
                                Vec2::new(800., 800.),
                            )),
                            events,
                            focused: true,
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |ui| c.navigation_controls(ui));
                        },
                    )
                };
                frame(vec![]);
                let output = frame(vec![]);
                let pos = output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t)
                            if t.galley.job.text == label
                                && s.clip_rect.contains_rect(t.visual_bounding_rect()) =>
                        {
                            Some(t.visual_bounding_rect().center())
                        }
                        _ => None,
                    })
                    .expect("real navigation button must be fully visible");
                frame(vec![egui::Event::PointerMoved(pos)]);
                for pressed in [true, false] {
                    frame(vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }]);
                }
                let expected = label == "Stop"
                    || matches!(state, "legacy" | "stored")
                    || (state == "active" && label == "Approach");
                let request = rx.try_recv();
                if expected {
                    match (label, request.unwrap()) {
                        (
                            "Stop",
                            Request::Navigate {
                                mode: NavigationMode::Stop,
                            },
                        )
                        | (
                            "Approach",
                            Request::Navigate {
                                mode: NavigationMode::Approach { .. },
                            },
                        )
                        | (
                            "Warp to belt",
                            Request::Warp {
                                destination: Destination::Belt,
                            },
                        ) => {}
                        other => panic!("wrong navigation result {other:?}"),
                    }
                } else {
                    assert!(request.is_err(), "{state} / {label}");
                    assert!(!c.request_pending);
                }
                assert!(rx.try_recv().is_err());
                assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
            }
        }
    }
    #[test]
    fn actual_keyboard_input_releases_once_when_bay_interlock_arrives_and_recovers() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx, "active");
        c.view_mode = ViewMode::External;
        c.services_open = false;
        c.controls_reference_open = false;
        c.performance_open = false;
        c.service_tab = ServiceTab::Navigation;
        let mut tick = 0;
        let frame = |c: &mut Cockpit, tick: &mut u64, events| {
            *tick += 1;
            c.observed = Instant::now();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(1000., 700.),
                    )),
                    time: Some(*tick as f64 / 60.),
                    events,
                    focused: true,
                    ..Default::default()
                },
                |ctx| eframe::App::update(c, ctx, &mut eframe::Frame::_new_kittest()),
            );
        };
        frame(&mut c, &mut tick, vec![]);
        frame(&mut c, &mut tick, vec![]);
        c.controls.clear();
        frame(
            &mut c,
            &mut tick,
            vec![egui::Event::Key {
                key: egui::Key::W,
                physical_key: Some(egui::Key::W),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(
            matches!(
                c.controls.take(),
                Some(Request::PilotInput {
                    translation: [1., 0., 0.],
                    ..
                })
            ),
            "free deployed drone must not lock normal carrier thrust"
        );
        let bay = c.snapshot.as_mut().unwrap().pilots[0]
            .drone_bay
            .as_mut()
            .unwrap();
        bay.machinery.handling_operation = Some(1);
        bay.motions.push(DroneBayMotionView {
            drone_id: 101,
            operation_id: 1,
            phase: DroneBayPhase::Held,
            frame: DroneBayMotionFrame::Body,
            confirmed_pose: bay.cradles[0].body_pose,
            segment: None,
            updated_at_ms: 1000,
        });
        let before = serde_json::to_value(&c.snapshot).unwrap();
        frame(&mut c, &mut tick, vec![]);
        assert!(
            matches!(
                c.controls.take(),
                Some(Request::PilotInput {
                    translation: [0., 0., 0.],
                    rotation: [0., 0., 0.]
                })
            ),
            "interlock must release nonneutral input"
        );
        frame(&mut c, &mut tick, vec![]);
        assert!(
            c.controls.take().is_none(),
            "held W must not leak or repeat neutral"
        );
        assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
        let bay = c.snapshot.as_mut().unwrap().pilots[0]
            .drone_bay
            .as_mut()
            .unwrap();
        bay.motions.clear();
        bay.machinery.handling_operation = None;
        frame(&mut c, &mut tick, vec![]);
        assert!(
            matches!(
                c.controls.take(),
                Some(Request::PilotInput {
                    translation: [1., 0., 0.],
                    ..
                })
            ),
            "released bay restores permitted thrust"
        );
    }
    #[test]
    fn public_batch_interlock_covers_between_members_held_cancelling_and_completion() {
        let ctx = egui::Context::default();
        for (phase, held, complete) in [
            (DroneBayBatchPhase::Running, false, false),
            (DroneBayBatchPhase::Running, true, false),
            (DroneBayBatchPhase::Cancelling, false, false),
            (DroneBayBatchPhase::Completed, false, true),
        ] {
            let mut c = fixture(&ctx, if complete { "active" } else { "stored" });
            c.assets = None; // Never requires or borrows private/controller stock.
            let p = &mut c.snapshot.as_mut().unwrap().pilots[0];
            let bay = p.drone_bay.as_mut().unwrap();
            bay.schema_version = DRONE_BAY_SCHEMA_VERSION;
            bay.batch_capability = Some(DRONE_BAY_BATCH_VERSION);
            bay.batch = Some(DroneBayBatchOperation {
                batch_id: 1,
                binding: bay.binding.clone(),
                kind: DroneBayBatchKind::DeployClass {
                    class: DroneClass::Recon,
                },
                phase,
                held,
                members: vec![DroneBayBatchMember {
                    service_operation_ids: Vec::new(),
                    drone_id: 101,
                    home_cradle: 0,
                    operation_id: complete.then_some(1),
                    status: if complete {
                        DroneBayBatchMemberStatus::Launched
                    } else if phase == DroneBayBatchPhase::Cancelling {
                        DroneBayBatchMemberStatus::Cancelled
                    } else {
                        DroneBayBatchMemberStatus::Pending
                    },
                }],
                current_operation_id: None,
                current_service_operation_id: None,
                accepted_at_ms: 1,
                updated_at_ms: 1000,
                message: "Authoritative queue between members".into(),
            });
            assert!(flight3d::bay_view_available(&c.own().unwrap()));
            assert!(c.can_fly());
            assert_eq!(c.can_maneuver(), complete);
            assert!(c.bay_departure_reason().is_some());
            if complete {
                assert!(c
                    .bay_departure_reason()
                    .unwrap()
                    .contains("Drones deployed"));
            } else {
                assert!(c.bay_maneuver_reason().unwrap().contains("Drone batch"));
            }
            assert!(c
                .bay_command_reason(&Request::Navigate {
                    mode: NavigationMode::Stop
                })
                .is_none());
            assert!(c
                .bay_command_reason(&Request::PilotInput {
                    translation: [0.; 3],
                    rotation: [0.; 3]
                })
                .is_none());
            c.observed = Instant::now() - Duration::from_secs(3);
            assert!(c.bay_maneuver_reason().unwrap().contains("unconfirmed"));
            c.observed = Instant::now();
            c.snapshot.as_mut().unwrap().pilots[0]
                .drone_bay
                .as_mut()
                .unwrap()
                .batch_capability = None;
            assert!(c.bay_maneuver_reason().unwrap().contains("unconfirmed"));
        }
    }

    #[test]
    fn resource_job_batch_guard_uses_actual_target_and_preserves_pending_ai() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx, "stored");
        c.assets = None;
        let mut worker = c.own().unwrap();
        worker.name = "worker".into();
        worker
            .flight
            .as_mut()
            .unwrap()
            .docking
            .as_mut()
            .unwrap()
            .host_vessel_id = "worker-vessel".into();
        worker.drone_bay.as_mut().unwrap().binding.vessel_id = "worker-vessel".into();
        c.snapshot.as_mut().unwrap().pilots.push(worker);
        let owner = c.pilot.clone();
        let bay = c
            .snapshot
            .as_mut()
            .unwrap()
            .pilots
            .iter_mut()
            .find(|p| p.name == owner)
            .unwrap()
            .drone_bay
            .as_mut()
            .unwrap();
        bay.schema_version = DRONE_BAY_SCHEMA_VERSION;
        bay.batch_capability = Some(DRONE_BAY_BATCH_VERSION);
        bay.batch = Some(DroneBayBatchOperation {
            batch_id: 1,
            binding: bay.binding.clone(),
            kind: DroneBayBatchKind::DeployClass {
                class: DroneClass::Recon,
            },
            phase: DroneBayBatchPhase::Running,
            held: false,
            members: vec![DroneBayBatchMember {
                service_operation_ids: Vec::new(),
                drone_id: 101,
                home_cradle: 0,
                operation_id: None,
                status: DroneBayBatchMemberStatus::Pending,
            }],
            current_operation_id: None,
            current_service_operation_id: None,
            accepted_at_ms: 1,
            updated_at_ms: 1000,
            message: "queued".into(),
        });
        assert!(flight3d::bay_view_available(&c.own().unwrap()));
        let job = |pilot| Request::StartResourceJob {
            target_units: 10,
            pilot,
        };
        assert!(c.bay_command_reason(&job(None)).is_some());
        assert!(c.bay_command_reason(&job(Some(owner.clone()))).is_some());
        assert!(c.bay_command_reason(&job(Some("worker".into()))).is_none());
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        c.ai_pin = Some(("pending".into(), 7));
        let before = serde_json::to_value(&c.snapshot).unwrap();
        let status = c.ship_ai.status().to_owned();
        c.send(job(None));
        assert!(rx.try_recv().is_err());
        assert!(!c.request_pending);
        assert_eq!(c.ai_pin, Some(("pending".into(), 7)));
        assert_eq!(c.ship_ai.status(), status);
        assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
        // An explicit worker is independent of the controller's active batch.
        c.send(job(Some("worker".into())));
        assert!(
            matches!(rx.try_recv().unwrap(),Request::StartResourceJob{pilot:Some(p),..} if p=="worker")
        );
        c.request_pending = false;
        c.snapshot
            .as_mut()
            .unwrap()
            .remote_controls
            .push(RemoteControlLease {
                controller: owner.clone(),
                pilot: "worker".into(),
            });
        c.manual_remote = Some("worker".into());
        assert!(c.bay_command_reason(&job(None)).is_none());
        assert!(c.bay_command_reason(&job(Some(owner))).is_some());
        let free = fixture(&ctx, "active");
        assert!(
            free.bay_command_reason(&job(None)).is_none(),
            "free deployed drones do not reserve a batch"
        );
    }

    #[test]
    fn remote_view_uses_worker_public_bay_not_controller_inventory() {
        let ctx = egui::Context::default();
        let mut c = fixture(&ctx, "held");
        let public = c.own().unwrap().drone_bay.unwrap();
        let private = DroneBayState {
            batches: None,
            schema_version: DRONE_BAY_LEGACY_SCHEMA_VERSION,
            binding: public.binding.clone(),
            initialization: DroneBayInitialization {
                fixture_version: 1,
                seeded_at_ms: 1,
                first_drone_id: 101,
            },
            stock: public
                .cradles
                .iter()
                .map(|e| DroneStockEntry {
                    drone_id: e.drone_id,
                    class: e.class,
                    home_cradle: e.home_cradle,
                    status: if e.presence == DroneBayPresence::Active {
                        DroneStockStatus::Active
                    } else {
                        DroneStockStatus::Stored {
                            hit_points: e.hit_points,
                        }
                    },
                })
                .collect(),
            high_watermark: 1,
            operations: vec![DroneBayOperation {
                operation_id: 1,
                drone_id: 101,
                kind: DroneBayOperationKind::Launch,
                binding: public.binding.clone(),
                phase: DroneBayPhase::Held,
                held_phase: Some(DroneBayPhase::TransferringOut),
                accepted_at_ms: 1,
                updated_at_ms: 1000,
                job_id: None,
                message: "Owner bay held".into(),
            }],
            handling_operation: Some(1),
            cleanup: None,
        };
        c.assets=Some(serde_json::from_value(serde_json::json!({"drone_bay":private,"ships":[],"storage":[],"corporation":null,"invitations":[]})).unwrap());
        let mut worker = c.own().unwrap();
        worker.name = "worker".into();
        worker.drone_bay = None;
        worker.drones.clear();
        worker
            .flight
            .as_mut()
            .unwrap()
            .docking
            .as_mut()
            .unwrap()
            .host_vessel_id = "worker-vessel".into();
        c.snapshot.as_mut().unwrap().pilots.push(worker);
        c.snapshot
            .as_mut()
            .unwrap()
            .remote_controls
            .push(RemoteControlLease {
                controller: c.pilot.clone(),
                pilot: "worker".into(),
            });
        c.manual_remote = Some("worker".into());
        assert!(c.can_maneuver());
        assert!(c.can_warp());
        assert!(c.bay_maneuver_reason().is_none());
        let before = serde_json::to_value(&c.snapshot).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        c.commands = Some(tx);
        c.send(Request::Navigate {
            mode: NavigationMode::FlyTo {
                position: [1., 2., 3.],
            },
        });
        assert!(
            matches!(rx.try_recv().unwrap(),Request::RemoteControl {pilot,request} if pilot=="worker" && matches!(*request,Request::Navigate {mode:NavigationMode::FlyTo {..}}))
        );
        assert_eq!(serde_json::to_value(&c.snapshot).unwrap(), before);
    }
}
