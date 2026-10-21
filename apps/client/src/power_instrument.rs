//! Read-only power content. IDs and numeric units are independent of display mounts.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PowerPanelId {
    ShipPower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PowerReadingId {
    CapacitorChargeJ,
    CapacitorCapacityJ,
    ReactorOutputW,
    WarpCostJ,
}

pub(super) struct PowerReadings {
    pub(super) panel_id: PowerPanelId,
    confirmed: Option<arkline_protocol::PowerState>,
    warp_cost_j: u64,
    modules: [String; 2],
}

impl PowerReadings {
    pub(super) fn from_pilot(pilot: &PilotView, now: u64) -> Self {
        Self {
            panel_id: PowerPanelId::ShipPower,
            confirmed: pilot.power.clone(),
            warp_cost_j: arkline_protocol::power_spec(pilot.ship_class).warp_cost_j,
            modules: std::array::from_fn(|index| module_instrument(pilot, index as u8, now)),
        }
    }

    pub(super) fn reading(&self, id: PowerReadingId) -> Option<u64> {
        match id {
            PowerReadingId::CapacitorChargeJ => self.confirmed.as_ref().map(|p| p.charge_j),
            PowerReadingId::CapacitorCapacityJ => self.confirmed.as_ref().map(|p| p.capacity_j),
            PowerReadingId::ReactorOutputW => self.confirmed.as_ref().map(|p| p.regen_w),
            PowerReadingId::WarpCostJ => Some(self.warp_cost_j),
        }
    }

    fn fill_fraction(&self) -> Option<f32> {
        let power = self.confirmed.as_ref()?;
        (power.capacity_j > 0)
            .then(|| (power.charge_j as f64 / power.capacity_j as f64).clamp(0.0, 1.0) as f32)
    }

    fn energy_status(&self) -> &'static str {
        match self.confirmed.as_ref() {
            None => "ENERGY STATE UNAVAILABLE",
            Some(p) if p.capacity_j == 0 => "CAPACITY UNAVAILABLE",
            Some(p) if p.charge_j >= p.capacity_j && p.charge_j < self.warp_cost_j => {
                "FULL / BELOW WARP COST"
            }
            Some(p) if p.charge_j < self.warp_cost_j => "LOW ENERGY / WARP COST NOT MET",
            Some(p) if p.charge_j >= p.capacity_j => "FULL / WARP ENERGY READY",
            Some(_) => "WARP ENERGY READY",
        }
    }
}

pub(super) fn paint(
    painter: &egui::Painter,
    rect: egui::Rect,
    authority_header: &str,
    color: egui::Color32,
    readings: &PowerReadings,
) {
    debug_assert_eq!(readings.panel_id, PowerPanelId::ShipPower);
    // The same logical card fits 256x128 and the current 300x180 canvas.
    let scale = (rect.width() / 246.0).min(rect.height() / 118.0);
    let ink = egui::Color32::from_rgb(185, 226, 208);
    let line = |text: String, y: f32, size: f32, color| {
        let mut job = egui::text::LayoutJob::simple(
            text,
            egui::FontId::monospace(size * scale),
            color,
            rect.width() - 6.0,
        );
        // Long authority/stop reasons visibly elide; no overflowing or invented state.
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        let galley = painter.layout_job(job);
        painter.galley(rect.left_top() + egui::vec2(3.0, y * scale), galley, color);
    };
    line(authority_header.to_owned(), 0.0, 10.0, color);
    let charge = readings.reading(PowerReadingId::CapacitorChargeJ);
    let capacity = readings.reading(PowerReadingId::CapacitorCapacityJ);
    let energy = match (charge, capacity) {
        (Some(charge), Some(capacity)) => format!(
            "{:.1} / {:.1} kJ",
            charge as f64 / 1000.0,
            capacity as f64 / 1000.0
        ),
        _ => "POWER UNAVAILABLE".into(),
    };
    line(energy, 15.0, 20.0, ink);
    if let Some(fraction) = readings.fill_fraction() {
        let bar = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(3.0, 42.0 * scale),
            egui::vec2(rect.width() - 6.0, 7.0 * scale),
        );
        painter.rect_filled(bar, 0.0, color.gamma_multiply(0.18));
        if fraction > 0.0 {
            painter.rect_filled(
                egui::Rect::from_min_size(
                    bar.min,
                    egui::vec2(bar.width() * fraction, bar.height()),
                ),
                0.0,
                color,
            );
        }
    }
    let reactor = readings
        .reading(PowerReadingId::ReactorOutputW)
        .map_or_else(
            || "REACTOR UNKNOWN".into(),
            |watts| format!("REACTOR {watts} W"),
        );
    let warp = readings
        .reading(PowerReadingId::WarpCostJ)
        .expect("ship-class spec reports warp cost");
    line(
        format!("{reactor} · WARP {:.1} kJ", warp as f64 / 1000.0),
        54.0,
        9.0,
        ink,
    );
    line(readings.modules[0].clone(), 69.0, 10.0, ink);
    line(readings.modules[1].clone(), 83.0, 10.0, ink);
    line(readings.energy_status().into(), 102.0, 9.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_capacity_below_warp_cost_never_claims_warp_ready() {
        let pilot: PilotView = serde_json::from_value(
            serde_json::json!({"name":"PowerQA", "credits":0, "ore":0, "location":Location::Belt}),
        )
        .unwrap();
        let mut readings = PowerReadings::from_pilot(&pilot, 1000);
        readings.warp_cost_j = 100;
        readings.confirmed = Some(arkline_protocol::PowerState {
            charge_j: 50,
            capacity_j: 50,
            regen_w: 1,
            updated_at_ms: 1000,
            regen_remainder_mj: 0,
        });
        assert_eq!(readings.fill_fraction(), Some(1.0));
        assert_eq!(readings.energy_status(), "FULL / BELOW WARP COST");
    }

    #[test]
    fn power_card_confirmed_units_unknown_bar_and_actual_text_fit() {
        let mut pilot: PilotView = serde_json::from_value(serde_json::json!({
            "name":"PowerQA", "credits":0, "ore":0, "location":Location::Belt,
            "system_id":2
        }))
        .unwrap();
        for size in [egui::vec2(256.0, 128.0), egui::vec2(300.0, 180.0)] {
            for known in [false, true] {
                pilot.power = known.then_some(arkline_protocol::PowerState {
                    charge_j: 120_000,
                    capacity_j: 240_000,
                    regen_w: 1200,
                    updated_at_ms: 1000,
                    regen_remainder_mj: 0,
                });
                let before = serde_json::to_value(&pilot).unwrap();
                let readings = PowerReadings::from_pilot(&pilot, 1000);
                assert_eq!(readings.panel_id, PowerPanelId::ShipPower);
                assert_eq!(
                    readings.reading(PowerReadingId::CapacitorChargeJ),
                    known.then_some(120_000)
                );
                assert_eq!(
                    readings.reading(PowerReadingId::ReactorOutputW),
                    known.then_some(1200)
                );
                assert_eq!(readings.fill_fraction(), known.then_some(0.5));
                let ctx = egui::Context::default();
                let rect = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), size).shrink(5.0);
                let output = ctx.run(egui::RawInput::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        paint(
                            &ui.painter().with_clip_rect(rect),
                            rect,
                            "POWER / PAUSED / SNAPSHOT HELD",
                            super::super::super::AMBER,
                            &readings,
                        );
                    });
                });
                let mut labels = Vec::new();
                let mut bars = 0;
                for shape in &output.shapes {
                    match &shape.shape {
                        egui::Shape::Text(text) => {
                            assert!(
                                rect.contains_rect(text.visual_bounding_rect()),
                                "{:?} outside {:?}",
                                text.galley.job.text,
                                rect
                            );
                            labels.push(text.galley.job.text.as_str());
                        }
                        egui::Shape::Rect(shape)
                            if (shape.rect.top()
                                - (rect.top()
                                    + 42.0
                                        * (rect.width() / 246.0).min(rect.height() / 118.0)))
                            .abs()
                                < 0.1 =>
                        {
                            bars += 1
                        }
                        _ => {}
                    }
                }
                assert!(labels.contains(&"POWER / PAUSED / SNAPSHOT HELD"));
                assert!(labels.contains(&if known {
                    "120.0 / 240.0 kJ"
                } else {
                    "POWER UNAVAILABLE"
                }));
                assert_eq!(
                    bars,
                    if known { 2 } else { 0 },
                    "unknown must not look like empty measured energy"
                );
                assert_eq!(serde_json::to_value(&pilot).unwrap(), before);
            }
        }
    }
}
