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
    RegenerationRatingW,
    WarpCostJ,
    CargoUnits,
    CargoCapacityUnits,
}

pub(super) struct PowerReadings {
    pub(super) panel_id: PowerPanelId,
    confirmed: Option<arkline_protocol::PowerState>,
    warp_cost_j: u64,
    modules: [String; 2],
    cargo_units: u64,
    cargo_capacity_units: u64,
}

impl PowerReadings {
    pub(super) fn from_pilot(pilot: &PilotView, now: u64) -> Self {
        Self {
            panel_id: PowerPanelId::ShipPower,
            confirmed: pilot.power.clone(),
            warp_cost_j: arkline_protocol::power_spec(pilot.ship_class).warp_cost_j,
            modules: std::array::from_fn(|index| module_instrument(pilot, index as u8, now)),
            cargo_units: pilot.ore,
            cargo_capacity_units: arkline_protocol::cargo_capacity(pilot.ship_class),
        }
    }

    pub(super) fn reading(&self, id: PowerReadingId) -> Option<u64> {
        match id {
            PowerReadingId::CapacitorChargeJ => self.confirmed.as_ref().map(|p| p.charge_j),
            PowerReadingId::CapacitorCapacityJ => self.confirmed.as_ref().map(|p| p.capacity_j),
            PowerReadingId::RegenerationRatingW => self.confirmed.as_ref().map(|p| p.regen_w),
            PowerReadingId::WarpCostJ => Some(self.warp_cost_j),
            PowerReadingId::CargoUnits => Some(self.cargo_units),
            PowerReadingId::CargoCapacityUnits => Some(self.cargo_capacity_units),
        }
    }

    #[cfg(test)]
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

fn gauge_fraction(value: Option<u64>, capacity: Option<u64>) -> Option<f32> {
    let (value, capacity) = (value?, capacity?);
    (capacity > 0).then(|| (value as f64 / capacity as f64).clamp(0.0, 1.0) as f32)
}

fn gauge_point(center: egui::Pos2, radius: f32, fraction: f32) -> egui::Pos2 {
    let angle = (135.0 + 270.0 * fraction).to_radians();
    center + egui::vec2(angle.cos(), angle.sin()) * radius
}

// Fit the entire real value, never an elided numeric approximation, inside
// the actual circular face. The corner test also accounts for glyph height.
fn dial_text(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    anchor: egui::Pos2,
    text: String,
    maximum_font: f32,
    color: egui::Color32,
) {
    let mut size = maximum_font;
    loop {
        let galley = painter.layout_no_wrap(text.clone(), egui::FontId::monospace(size), color);
        let bounds = egui::Rect::from_center_size(anchor, galley.size());
        let fits = [
            bounds.left_top(),
            bounds.right_top(),
            bounds.left_bottom(),
            bounds.right_bottom(),
        ]
        .iter()
        .all(|point| point.distance(center) <= radius);
        if fits || size <= 0.5 {
            painter.galley(bounds.min, galley, color);
            break;
        }
        size *= 0.92;
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_gauge(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    value: Option<u64>,
    capacity: Option<u64>,
    divisor: f64,
    label: &str,
    marker: Option<u64>,
    color: egui::Color32,
    scale: f32,
) {
    let fraction = gauge_fraction(value, capacity);
    painter.circle_filled(center, radius, egui::Color32::from_rgb(2, 14, 10));
    let track = (0..=60)
        .map(|i| gauge_point(center, radius * 0.93, i as f32 / 60.0))
        .collect();
    painter.add(egui::Shape::line(
        track,
        egui::Stroke::new(2.0 * scale, color.gamma_multiply(0.22)),
    ));
    for tick in 0..=10 {
        let f = tick as f32 / 10.0;
        painter.line_segment(
            [
                gauge_point(center, radius * 0.85, f),
                gauge_point(center, radius * 0.95, f),
            ],
            egui::Stroke::new(scale, color.gamma_multiply(0.65)),
        );
    }
    if let Some(fraction) = fraction {
        if fraction > 0.0 {
            let fill = (0..=60)
                .map(|i| gauge_point(center, radius * 0.93, fraction * i as f32 / 60.0))
                .collect();
            painter.add(egui::Shape::line(
                fill,
                egui::Stroke::new(3.0 * scale, color),
            ));
        }
        painter.line_segment(
            [
                gauge_point(center, radius * 0.82, fraction),
                gauge_point(center, radius * 0.96, fraction),
            ],
            egui::Stroke::new(1.5 * scale, color),
        );
    }
    if let (Some(marker), Some(capacity)) = (marker, capacity.filter(|capacity| *capacity > 0)) {
        if marker <= capacity {
            let f = marker as f64 / capacity as f64;
            painter.line_segment(
                [
                    gauge_point(center, radius * 0.82, f as f32),
                    gauge_point(center, radius * 0.96, f as f32),
                ],
                egui::Stroke::new(2.0 * scale, super::super::AMBER),
            );
        }
    }
    let value_label = if fraction.is_some() {
        let value = value.expect("known fraction has value");
        if divisor == 1.0 {
            value.to_string()
        } else {
            format!("{:.1}", value as f64 / divisor)
        }
    } else {
        "UNAVAILABLE".into()
    };
    dial_text(
        painter,
        center,
        radius * 0.78,
        center - egui::vec2(0.0, radius * 0.22),
        value_label,
        if fraction.is_some() { 24.0 } else { 10.0 } * scale,
        color,
    );
    if label == "LEGACY ORE UNITS" {
        // "u" denotes the existing legacy ore units, never kg or cubic metres.
        dial_text(
            painter,
            center,
            radius * 0.78,
            center + egui::vec2(0.0, radius * 0.20),
            "LEGACY u".into(),
            10.0 * scale,
            color,
        );
    } else {
        dial_text(
            painter,
            center,
            radius * 0.78,
            center + egui::vec2(0.0, radius * 0.24),
            label.into(),
            10.0 * scale,
            color,
        );
    }
    if let Some(capacity) = capacity.filter(|capacity| *capacity > 0) {
        let maximum = if divisor == 1.0 {
            capacity.to_string()
        } else {
            format!("{:.1}", capacity as f64 / divisor)
        };
        for (f, text) in [(0.0, "0".to_owned()), (1.0, maximum)] {
            dial_text(
                painter,
                center,
                radius * 0.96,
                gauge_point(center, radius * 0.65, f),
                text,
                9.0 * scale,
                color,
            );
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
    // Full-canvas normalized centers align with the canonical physical dial rims.
    let scale = (rect.width() / 246.0).min(rect.height() / 118.0);
    let line = |text: String, bounds: egui::Rect, size: f32| {
        let mut job = egui::text::LayoutJob::simple(
            text,
            egui::FontId::monospace(size * scale),
            color,
            bounds.width(),
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        painter.galley(bounds.min, painter.layout_job(job), color);
    };
    line(
        authority_header.into(),
        egui::Rect::from_min_max(rect.min + egui::vec2(3.0, 0.0), rect.max),
        13.0,
    );
    let radius = rect.height() * arkline_protocol::PIONEER_DIAL_RADIUS_HEIGHT as f32;
    let center = |index: usize| {
        let [x, y] = arkline_protocol::PIONEER_DIAL_CENTERS[index];
        rect.min + egui::vec2(rect.width() * x as f32, rect.height() * y as f32)
    };
    paint_gauge(
        painter,
        center(0),
        radius,
        readings.reading(PowerReadingId::CapacitorChargeJ),
        readings.reading(PowerReadingId::CapacitorCapacityJ),
        1000.0,
        "CAP kJ",
        readings.reading(PowerReadingId::WarpCostJ),
        color,
        scale,
    );
    paint_gauge(
        painter,
        center(1),
        radius,
        readings.reading(PowerReadingId::CargoUnits),
        readings.reading(PowerReadingId::CargoCapacityUnits),
        1.0,
        "LEGACY ORE UNITS",
        None,
        color,
        scale,
    );
    let regen = readings
        .reading(PowerReadingId::RegenerationRatingW)
        .map_or_else(
            || "REGEN RATING UNKNOWN".into(),
            |watts| format!("REGEN RATING {watts} W"),
        );
    line(
        format!(
            "WARP COST {:.1} kJ · {} · {regen}",
            readings.warp_cost_j as f64 / 1000.0,
            readings.energy_status()
        ),
        egui::Rect::from_min_max(rect.min + egui::vec2(3.0, rect.height() * 0.77), rect.max),
        10.0,
    );
    for index in 0..2 {
        let min = rect.min
            + egui::vec2(
                3.0 + rect.width() * index as f32 * 0.5,
                rect.height() * 0.88,
            );
        line(
            readings.modules[index].clone(),
            egui::Rect::from_min_size(min, egui::vec2(rect.width() * 0.5 - 6.0, 15.0 * scale)),
            11.0,
        );
    }
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
            "name":"PowerQA", "credits":0, "ore":350, "location":Location::Belt, "system_id":2
        }))
        .unwrap();
        for size in [
            egui::vec2(256.0, 128.0),
            egui::vec2(300.0, 180.0),
            egui::vec2(474.0, 180.0),
        ] {
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
                assert_eq!(
                    readings.reading(PowerReadingId::CapacitorChargeJ),
                    known.then_some(120_000)
                );
                assert_eq!(readings.reading(PowerReadingId::CargoUnits), Some(350));
                assert_eq!(
                    readings.reading(PowerReadingId::CargoCapacityUnits),
                    Some(arkline_protocol::cargo_capacity(pilot.ship_class))
                );
                assert_eq!(readings.fill_fraction(), known.then_some(0.5));
                assert_eq!(gauge_fraction(None, Some(100)), None);
                assert_eq!(gauge_fraction(Some(0), Some(0)), None);
                let rect = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), size);
                let ctx = egui::Context::default();
                let output = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        paint(
                            &ui.painter().with_clip_rect(rect),
                            rect,
                            "POWER / PAUSED",
                            super::super::super::AMBER,
                            &readings,
                        );
                    });
                });
                let mut labels = Vec::new();
                let cap_center = rect.min + egui::vec2(rect.width() * 0.25, rect.height() * 0.42);
                let mut cap_needles = 0;
                for shape in &output.shapes {
                    match &shape.shape {
                        egui::Shape::Text(text) => {
                            assert!(
                                rect.contains_rect(text.visual_bounding_rect()),
                                "{:?} outside {:?}",
                                text.galley.job.text,
                                rect
                            );
                            let bounds = text.visual_bounding_rect();
                            if bounds.center().y > rect.top() + rect.height() * 0.14
                                && bounds.center().y < rect.top() + rect.height() * 0.72
                            {
                                let centers =
                                    arkline_protocol::PIONEER_DIAL_CENTERS.map(|[x, y]| {
                                        rect.min
                                            + egui::vec2(
                                                rect.width() * x as f32,
                                                rect.height() * y as f32,
                                            )
                                    });
                                assert!(
                                    centers.iter().any(|center| [
                                        bounds.left_top(),
                                        bounds.right_top(),
                                        bounds.left_bottom(),
                                        bounds.right_bottom()
                                    ]
                                    .iter()
                                    .all(|corner| corner.distance(*center)
                                        <= rect.height()
                                            * arkline_protocol::PIONEER_DIAL_RADIUS_HEIGHT as f32)),
                                    "dial text escapes physical circle: {:?}",
                                    text.galley.job.text
                                );
                            }
                            labels.push(text.galley.job.text.as_str());
                        }
                        egui::Shape::LineSegment { points, .. }
                            if (points[0].distance(cap_center) - rect.height() * 0.28 * 0.82)
                                .abs()
                                < 0.001
                                && (points[1].distance(cap_center)
                                    - rect.height() * 0.28 * 0.96)
                                    .abs()
                                    < 0.001
                                && points[0].y < cap_center.y - rect.height() * 0.20 =>
                        {
                            cap_needles += 1
                        }
                        _ => {}
                    }
                }
                for label in ["POWER / PAUSED", "CAP kJ", "LEGACY u", "350"] {
                    assert!(labels.contains(&label));
                }
                assert!(labels.contains(&if known { "120.0" } else { "UNAVAILABLE" }));
                assert!(labels.iter().any(|label| label.contains("REGEN RATING")));
                assert_eq!(
                    cap_needles,
                    if known { 1 } else { 0 },
                    "unknown must not look like measured zero"
                );
                assert_eq!(serde_json::to_value(&pilot).unwrap(), before);
            }
        }
    }
}
