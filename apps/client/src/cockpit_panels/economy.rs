//! Ledger audit and station exchange presentation.

use super::*;

impl Cockpit {
    pub(crate) fn ledger_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new("ECONOMIC LEDGER")
                    .color(self.accent())
                    .strong(),
            );
            if ui
                .add_enabled(
                    self.connected && self.audit_updates.is_none(),
                    egui::Button::new("Audit current ledger"),
                )
                .clicked()
            {
                let server = self.server.clone();
                let scope = self.credential_scope.as_deref().unwrap_or(&server);
                let path = self.data_dir.join(format!(
                    "audit-{}.json",
                    hex::encode(Sha256::digest(scope.as_bytes()))
                ));
                let (tx, rx) = mpsc::channel();
                self.audit_updates = Some(rx);
                #[cfg(not(target_arch = "wasm32"))]
                std::thread::spawn(move || {
                    let _ = tx.send(audit::audit(&server, &path));
                });
                #[cfg(target_arch = "wasm32")]
                { let _ = (server, path); let _ = tx.send(Err("Independent ledger export auditing currently requires the native client or arkline-verifier CLI.".into())); }
            }
            if self.audit_updates.is_some() {
                ui.spinner();
                ui.small("Checking independent export…");
            }
        });
        if let Some(snapshot) = &self.snapshot {
            ui.small(format!(
                "Latest server checkpoint: #{}",
                snapshot.ledger_sequence
            ));
            ui.add(
                egui::Label::new(
                    egui::RichText::new(&snapshot.ledger_hash)
                        .monospace()
                        .color(MUTED),
                )
                .wrap(),
            );
        }
        ui.separator();
        ui.label(
            egui::RichText::new("LAST COMPLETED AUDIT")
                .size(11.0)
                .color(MUTED),
        );
        match &self.audit_result {
            Some(Ok(result)) => {
                ui.colored_label(self.accent(), result);
            }
            Some(Err(error)) => {
                ui.colored_label(AMBER, format!("Audit failed: {error}"));
            }
            None => {
                ui.colored_label(MUTED, "No independent audit in this cockpit session. The first successful audit pins a local checkpoint; later audits must preserve its history.");
            }
        }
        ui.small("Server history may advance after this audit. A successful result applies to its reported head, not automatically to the latest checkpoint.");
        ui.small("An audit checks the exported economic rules and history. It does not prove every gameplay event occurred or certify server honesty.");
    }
    pub(crate) fn market(&mut self, ui: &mut egui::Ui) {
        use exchange_readiness::ExchangeAction;
        ui.label(
            egui::RichText::new("STATION EXCHANGE")
                .color(self.accent())
                .strong(),
        );
        let cargo_limit = self.own_cargo_capacity();
        let readiness = self.exchange_readiness_reason();
        ui.small("Player sell orders · ore · credits per unit");
        if let Some(reason) = readiness {
            ui.colored_label(AMBER, reason);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Sell");
            ui.add(egui::DragValue::new(&mut self.sell_amount).range(1..=cargo_limit));
            ui.label("at");
            ui.add(egui::DragValue::new(&mut self.price).range(1..=1_000_000));
            let reason = self.exchange_action_reason(ExchangeAction::List {
                units: self.sell_amount,
                unit_price: self.price,
            });
            if ui
                .add_enabled(reason.is_none(), egui::Button::new("List ore"))
                .on_disabled_hover_text(reason.unwrap_or(""))
                .clicked()
            {
                self.send(Request::PlaceSell {
                    units: self.sell_amount,
                    unit_price: self.price,
                });
            }
            if readiness.is_none() {
                if let Some(reason) = reason {
                    ui.small(reason);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Buy quantity");
            ui.add(egui::DragValue::new(&mut self.buy_amount).range(1..=cargo_limit));
        });
        let orders = self
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.orders.clone())
            .unwrap_or_default();
        egui::ScrollArea::vertical()
            .max_height(190.0)
            .show(ui, |ui| {
                if orders.is_empty() {
                    ui.colored_label(
                        MUTED,
                        if self.snapshot.is_none() {
                            "Waiting for exchange listings."
                        } else if readiness.is_some() {
                            "No listings in the last received snapshot."
                        } else {
                            "No listings yet. Mine ore and create the first order."
                        },
                    );
                }
                for order in orders {
                    ui.push_id(order.id, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!(
                                "#{}   {}   {} ore × {} CR",
                                order.id, order.seller, order.units, order.unit_price
                            ));
                            let (label, action) = if order.seller == self.pilot {
                                ("Cancel", ExchangeAction::Cancel { order_id: order.id })
                            } else {
                                (
                                    "Buy",
                                    ExchangeAction::Buy {
                                        order_id: order.id,
                                        units: self.buy_amount,
                                    },
                                )
                            };
                            let reason = self.exchange_action_reason(action);
                            if ui
                                .add_enabled(reason.is_none(), egui::Button::new(label))
                                .on_disabled_hover_text(reason.unwrap_or(""))
                                .clicked()
                            {
                                if order.seller == self.pilot {
                                    self.send(Request::CancelSell { order_id: order.id });
                                } else {
                                    self.send(Request::Buy {
                                        order_id: order.id,
                                        units: self.buy_amount,
                                    });
                                }
                            }
                            if readiness.is_none() {
                                if let Some(reason) = reason {
                                    ui.small(reason);
                                }
                            }
                        });
                    });
                    ui.separator();
                }
            });
    }
}
