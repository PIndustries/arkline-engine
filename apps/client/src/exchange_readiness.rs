//! UI explanations for existing authoritative exchange commands.
use super::*;

pub(super) enum ExchangeAction {
    List { units: u64, unit_price: u64 },
    Buy { order_id: u64, units: u64 },
    Cancel { order_id: u64 },
}
impl Cockpit {
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
                } else if pilot
                    .ore
                    .checked_add(units)
                    .is_none_or(|ore| ore > cargo_capacity(pilot.ship_class))
                {
                    Some("Not enough free hold capacity for this purchase.")
                } else if let Some(cost) = order.unit_price.checked_mul(units) {
                    (cost > pilot.credits).then_some("Not enough credits for this purchase.")
                } else {
                    Some("Purchase price exceeds the supported value.")
                }
            }
            ExchangeAction::Cancel { order_id } => {
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
                } else if pilot
                    .ore
                    .checked_add(order.units)
                    .is_none_or(|ore| ore > cargo_capacity(pilot.ship_class))
                {
                    Some("Free hold space for all listed ore before cancelling.")
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
        assert!(text.iter().any(|(t, _)| t.starts_with("Range ")));
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
}
