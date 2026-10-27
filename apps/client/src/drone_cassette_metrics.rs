//! Read-only telemetry on the five fixed cassette faces. Sticky mechanism IDs
//! select sources; snapshot receipt freshness is separate from pose-update age.
//! Full-face gauges use only confirmed telemetry; the lower glance zone retains
//! identity, task, speed and one real cycle. Schematics are not camera images.
use super::*;
use crate::drone_telemetry::{Availability, DroneReadout, GaugeReadout, TelemetryTime};
use arkline_protocol::{DroneClass, DroneView, PioneerCockpitMechanisms};

#[derive(Debug, PartialEq)]
struct Metrics {
    title: String,
    task: String,
    cycle_label: String,
    cycle_fraction: Option<f32>,
    status: &'static str,
    stale: bool,
    heading: Option<[f64; 2]>,
    health: Option<GaugeReadout>,
    signal: Option<GaugeReadout>,
}
fn metrics(id: Option<u64>, drone: Option<&DroneView>, time: TelemetryTime) -> Metrics {
    let mut result = Metrics {
        title: id.map_or_else(|| "DRONE BAY".into(), |id| format!("DRONE #{id}")),
        task: if id.is_some() {
            "TELEMETRY UNAVAILABLE"
        } else {
            "NO DRONE ASSIGNED"
        }
        .into(),
        cycle_label: "NO CONFIRMED CYCLE".into(),
        cycle_fraction: None,
        status: match time.availability {
            Availability::Live => "LIVE",
            Availability::Paused => "PAUSED",
            Availability::Stale => "STALE",
            Availability::Disconnected => "OFFLINE",
            Availability::Unavailable => "UNAVAILABLE",
        },
        stale: true,
        heading: None,
        health: None,
        signal: None,
    };
    let Some(d) = drone.filter(|d| Some(d.id) == id) else {
        result.status = if id.is_some() {
            "NO DATA"
        } else {
            "UNASSIGNED"
        };
        return result;
    };
    let data = DroneReadout::new(d, time);
    let class = match d.class {
        DroneClass::Recon => "RECON",
        DroneClass::Mining => "MINER",
        DroneClass::Attack => "ATTACK",
    };
    result.title = format!("{class} #{}", d.id);
    result.heading = data.heading_yaw_pitch_deg;
    result.health = Some(data.health);
    result.signal = Some(data.signal);
    result.task = format!(
        "{} | {}",
        data.task,
        data.speed
            .map_or_else(|| "SPEED —".into(), |v| format!("{v:.1} m/s"))
    );
    if let Some(cycle) = data.cycle {
        result.cycle_fraction = Some(cycle.fraction);
        result.cycle_label = cycle.label;
    }
    result.stale = time.availability != Availability::Live;
    // A fresh authority receipt can honestly report a lost/invalid link.
    // This does not change the confirmed cycle or pretend the snapshot is old.
    if time.availability == Availability::Live {
        if data.signal.fraction.is_none() {
            result.status = "SIGNAL —";
            result.stale = true;
        } else if d.signal_strength <= 0. {
            result.status = "NO SIGNAL";
            result.stale = true;
        }
    }
    result
}

pub(super) fn paint(
    ui: &egui::Ui,
    surfaces: &[Option<CockpitSurface>; COCKPIT_SURFACE_SLOTS],
    state: Option<&PioneerCockpitMechanisms>,
    own: &PilotView,
    time: TelemetryTime,
) {
    #[cfg(test)]
    let mut proof = Vec::new();
    for slot in 0..DRONE_CASSETTE_SURFACE_COUNT {
        let Some(surface) = &surfaces[DRONE_CASSETTE_SURFACE_START + slot] else {
            continue;
        };
        let id = state.and_then(|s| s.drone_ids[slot]);
        let drone = id.and_then(|id| fleet(own).into_iter().find(|d| d.id == id));
        let mut data = metrics(id, drone, time);
        if id.is_none() {
            data.title = format!("DRONE BAY {}", slot + 1);
        }
        if state.is_none() {
            data.task = "ASSIGNMENT UNAVAILABLE".into();
            data.status = "NO DATA";
        }
        surface.paint(ui, |p| paint_face(p, surface.canvas_rect(), &data));
        #[cfg(test)]
        proof.push(serde_json::json!({"bay":slot,"id":id,"title":data.title,"task":data.task,
            "status":data.status,"cycle_label":data.cycle_label,"cycle_fraction":data.cycle_fraction,
            "server_ms":time.server_ms,"pose_updated_ms":drone.map(|d|d.updated_at_ms),
            "heading_yaw_pitch":data.heading,"health":data.health.map(|g|serde_json::json!({"value":g.value,"maximum":g.maximum,"fraction":g.fraction})),
            "signal":data.signal.map(|g|serde_json::json!({"value":g.value,"fraction":g.fraction})),
            "glance_health":health_glance(data.health),"full_face":true,"canvas":[surface.canvas_rect().width(),surface.canvas_rect().height()]}));
    }
    #[cfg(test)]
    ui.ctx().data_mut(|d| {
        d.insert_temp(
            egui::Id::new("drone_cassette_readout_proof"),
            serde_json::json!(proof),
        )
    });
}

/// Fit ink within a declared cell, never draw a long status into another gauge.
fn text(p: &egui::Painter, r: egui::Rect, value: &str, size: f32, color: egui::Color32) {
    let mut galley = p.layout_no_wrap(value.into(), egui::FontId::monospace(size), color);
    let fit = (r.width() / galley.size().x.max(1.))
        .min(r.height() / galley.size().y.max(1.))
        .min(1.);
    if fit < 1. {
        galley = p.layout_no_wrap(value.into(), egui::FontId::monospace(size * fit), color);
    }
    p.galley(
        r.left_center() - egui::vec2(0., galley.size().y * 0.5),
        galley,
        color,
    );
}

/// A glance summary of the same authoritative health gauge, never a new value.
fn health_glance(health: Option<GaugeReadout>) -> String {
    match health {
        Some(g) if g.fraction.is_some() => format!("HP {:.0}/{:.0}", g.value, g.maximum),
        Some(_) => "HP INVALID".into(),
        None => "HP —".into(),
    }
}

fn paint_face(p: &egui::Painter, r: egui::Rect, data: &Metrics) {
    let scale = r.height() / 180.;
    let at = |x: f32, y: f32| r.min + egui::vec2(x, y) * scale;
    let w = r.width() / scale;
    let cell = |x0, y0, x1, y1| egui::Rect::from_min_max(at(x0, y0), at(x1, y1));
    let ink = egui::Color32::from_rgb(202, 226, 226);
    let muted = egui::Color32::from_rgb(105, 146, 155);
    let accent = if data.stale {
        super::super::AMBER
    } else {
        super::super::TEAL
    };
    let line = egui::Stroke::new(scale, muted.gamma_multiply(0.55));
    p.rect_filled(r, 0., egui::Color32::from_rgb(3, 15, 18));
    p.line_segment([at(8., 93.), at(w - 8., 93.)], line);
    p.line_segment([at(151., 7.), at(151., 87.)], line);

    // Compass reference is world +X forward / +Z right, not magnetic north.
    // The rotating outline is a schematic heading arrow, never attitude/roll.
    let center = at(47., 45.);
    let radius = 30. * scale;
    p.circle_stroke(center, radius, line);
    for tick in 0..16 {
        let angle = tick as f32 * std::f32::consts::TAU / 16.;
        let direction = egui::vec2(angle.sin(), -angle.cos());
        p.line_segment(
            [
                center + direction * radius * 0.83,
                center + direction * radius,
            ],
            line,
        );
    }
    text(p, cell(35., 0., 63., 14.), "+X", 12. * scale, muted);
    text(p, cell(80., 37., 103., 52.), "+Z", 12. * scale, muted);
    let yaw = data
        .heading
        .filter(|h| h[1].abs() < 89.999_999)
        .map(|h| h[0]);
    if let Some(yaw) = yaw {
        let angle = (yaw as f32).to_radians();
        let forward = egui::vec2(angle.sin(), -angle.cos());
        let right = egui::vec2(angle.cos(), angle.sin());
        let point = |x: f32, y: f32| center + (right * x + forward * y) * scale;
        // Single outlined drone icon; no animation beyond confirmed heading.
        let outline = vec![
            point(0., 23.),
            point(7., 5.),
            point(18., -4.),
            point(18., -11.),
            point(5., -7.),
            point(0., -16.),
            point(-5., -7.),
            point(-18., -11.),
            point(-18., -4.),
            point(-7., 5.),
        ];
        p.add(egui::Shape::closed_line(
            outline,
            egui::Stroke::new(2. * scale, accent),
        ));
        p.line_segment(
            [point(0., -8.), point(0., 13.)],
            egui::Stroke::new(scale, accent),
        );
    } else {
        text(
            p,
            cell(23., 30., 72., 57.),
            "—",
            24. * scale,
            super::super::AMBER,
        );
    }
    let yaw_label = yaw.map_or_else(|| "YAW —".into(), |y| format!("YAW {y:+.0}°"));
    text(p, cell(7., 77., 104., 92.), &yaw_label, 15. * scale, ink);
    text(p, cell(104., 0., 149., 15.), "PITCH", 12. * scale, muted);
    p.line_segment([at(125., 21.), at(125., 70.)], line);
    for (y, len) in [(21., 9.), (33.25, 5.), (45.5, 9.), (57.75, 5.), (70., 9.)] {
        p.line_segment([at(125. - len, y), at(125. + len, y)], line);
    }
    if let Some([_, pitch]) = data.heading {
        let y = 45.5 - pitch as f32 / 90. * 24.5;
        p.line_segment(
            [at(111., y), at(139., y)],
            egui::Stroke::new(3. * scale, accent),
        );
    }
    let pitch = data
        .heading
        .map_or_else(|| "—".into(), |h| format!("{:+.0}°", h[1]));
    text(p, cell(108., 77., 148., 92.), &pitch, 15. * scale, ink);

    for (label, gauge, y) in [("HP", data.health, 5.), ("SIGNAL", data.signal, 49.)] {
        let value = match gauge {
            Some(g) if g.fraction.is_some() && label == "HP" => {
                format!("{:.0}/{:.0}", g.value, g.maximum)
            }
            Some(g) if g.fraction.is_some() => format!("{:.0}%", (g.value * 100.).floor()),
            Some(g) if g.value.is_finite() && label == "HP" => format!("{:.0} INVALID", g.value),
            Some(g) if g.value.is_finite() => format!("{:.2} INVALID", g.value),
            Some(_) => "INVALID".into(),
            None => "NO DATA".into(),
        };
        let valid = gauge.and_then(|g| g.fraction);
        let tone = if valid.is_none() || valid == Some(0.) {
            super::super::AMBER
        } else {
            accent
        };
        text(
            p,
            cell(161., y, w - 8., y + 20.),
            &format!("{label} {value}"),
            18. * scale,
            tone,
        );
        let track = cell(161., y + 24., w - 8., y + 35.);
        p.rect_filled(track, 1., egui::Color32::from_rgb(20, 40, 47));
        if let Some(fraction) = valid {
            p.rect_filled(
                egui::Rect::from_min_size(
                    track.min,
                    egui::vec2(track.width() * fraction, track.height()),
                ),
                1.,
                tone.gamma_multiply(0.75),
            );
        } else {
            p.line_segment(
                [track.left_bottom(), track.right_top()],
                egui::Stroke::new(scale, tone),
            );
        }
    }
    // The fixed cassette extends above a neutral pilot glance. Keep the
    // operational essentials in its lower visible band; upper gauges remain
    // detailed inspection views, not the only place to discover drone damage.
    text(
        p,
        cell(9., 96., 151., 120.),
        &data.title,
        21. * scale,
        super::super::TEAL,
    );
    text(
        p,
        cell(158., 96., w - 9., 120.),
        &health_glance(data.health),
        20. * scale,
        if data
            .health
            .is_some_and(|g| g.fraction.is_some_and(|v| v > 0.))
        {
            accent
        } else {
            super::super::AMBER
        },
    );
    text(
        p,
        cell(9., 122., w - 93., 148.),
        &data.task,
        21. * scale,
        if data.stale { accent } else { ink },
    );
    text(
        p,
        cell(w - 87., 122., w - 9., 148.),
        data.status,
        18. * scale,
        accent,
    );
    let track = cell(9., 151., w - 9., 175.);
    p.rect_filled(track, 2., egui::Color32::from_rgb(19, 41, 46));
    if let Some(fraction) = data.cycle_fraction {
        p.rect_filled(
            egui::Rect::from_min_size(
                track.min,
                egui::vec2(track.width() * fraction, track.height()),
            ),
            2.,
            egui::Color32::from_rgb(20, 84, 89),
        );
    }
    text(
        p,
        track.shrink2(egui::vec2(4. * scale, 0.)),
        &data.cycle_label,
        18. * scale,
        ink,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::DroneMode;
    fn drone() -> DroneView {
        DroneView {
            orientation_xyzw: None,
            id: 42,
            class: DroneClass::Mining,
            hit_points: 37,
            signal_strength: 0.7,
            mining_cycle: None,
            attack_ready_at_ms: None,
            position: [0.; 3],
            velocity: [3., 4., 0.],
            heading: [1., 0., 0.],
            mode: DroneMode::Hold,
            updated_at_ms: 1000,
        }
    }
    fn live() -> TelemetryTime {
        TelemetryTime::from_observation(Some(10_000), Some(20), 10_000, true, false)
    }
    #[test]
    fn lower_glance_health_is_confirmed_or_explicitly_unknown() {
        let mut d = drone();
        assert_eq!(
            health_glance(metrics(Some(42), Some(&d), live()).health),
            "HP 37/100"
        );
        assert_eq!(health_glance(None), "HP —");
        d.hit_points = 0;
        assert_eq!(
            health_glance(metrics(Some(42), Some(&d), live()).health),
            "HP 0/100"
        );
        d.hit_points = arkline_protocol::DRONE_HIT_POINTS + 1;
        assert_eq!(
            health_glance(metrics(Some(42), Some(&d), live()).health),
            "HP INVALID"
        );
    }

    #[test]
    fn fresh_stationary_hold_has_actual_speed_health_and_sticky_identity() {
        let d = drone();
        let m = metrics(Some(42), Some(&d), live());
        assert_eq!(m.title, "MINER #42");
        assert_eq!(m.health.unwrap().value, 37.);
        assert_eq!(m.task, "HOLD | 5.0 m/s");
        assert_eq!(m.status, "LIVE");
        assert!(!m.stale);
        assert_eq!(m.cycle_fraction, None);
        assert!(metrics(Some(43), Some(&d), live())
            .task
            .contains("UNAVAILABLE"));
    }
    #[test]
    fn fresh_receipt_does_not_hide_lost_or_invalid_signal() {
        let mut d = drone();
        d.signal_strength = 0.;
        let lost = metrics(Some(42), Some(&d), live());
        assert_eq!(lost.status, "NO SIGNAL");
        assert!(lost.stale);
        d.signal_strength = f64::NAN;
        assert_eq!(metrics(Some(42), Some(&d), live()).status, "SIGNAL —");
    }
    #[test]
    fn invalid_gauges_never_draw_false_full_bars_or_hide_confirmed_task() {
        let mut d = drone();
        d.hit_points = arkline_protocol::DRONE_HIT_POINTS + 1;
        for signal in [-0.1, 1.01, f64::INFINITY, f64::NAN] {
            d.signal_strength = signal;
            let m = metrics(Some(d.id), Some(&d), live());
            assert_eq!(m.health.unwrap().value, f64::from(d.hit_points));
            assert_eq!(m.health.unwrap().fraction, None);
            assert_eq!(m.signal.unwrap().fraction, None);
            assert_eq!(m.status, "SIGNAL —");
            assert!(m.task.starts_with("HOLD"));
        }
        d.signal_strength = 0.;
        d.hit_points = 0;
        let m = metrics(Some(d.id), Some(&d), live());
        assert_eq!(m.health.unwrap().fraction, Some(0.));
        assert_eq!(m.signal.unwrap().fraction, Some(0.));
        assert_eq!(m.status, "NO SIGNAL");
    }

    #[test]
    fn empty_face_text_is_bounded_and_separate_not_duplicate_identity_status() {
        let data = metrics(None, None, live());
        assert_eq!(data.title, "DRONE BAY");
        assert_eq!(data.status, "UNASSIGNED");
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(314., 180.));
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            paint_face(&ctx.layer_painter(egui::LayerId::background()), rect, &data);
        });
        let text_bounds: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some(t.galley.rect.translate(t.pos.to_vec2()))
                } else {
                    None
                }
            })
            .collect();
        assert!(text_bounds.len() >= 10);
        for (i, bounds) in text_bounds.iter().enumerate() {
            assert!(
                rect.expand(0.1).contains_rect(*bounds),
                "Text must fit physical canvas: {bounds:?}"
            );
            for other in text_bounds.iter().skip(i + 1) {
                assert!(
                    !bounds.intersects(*other),
                    "Text cells overlap: {bounds:?} / {other:?}"
                );
            }
        }
    }

    #[test]
    fn operational_identity_health_task_and_freshness_share_lower_glance_band() {
        let data = metrics(Some(42), Some(&drone()), live());
        let ctx = egui::Context::default();
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(314., 180.));
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            paint_face(&ctx.layer_painter(egui::LayerId::background()), rect, &data);
        });
        for wanted in [
            &data.title,
            &health_glance(data.health),
            &data.task,
            &data.status.to_string(),
        ] {
            let glyph = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if &t.galley.job.text == wanted => Some(t),
                    _ => None,
                })
                .find(|t| t.pos.y >= 96.)
                .expect("Operational text belongs in lower physical glance band");
            let bounds = glyph.galley.rect.translate(glyph.pos.to_vec2());
            assert!(bounds.top() >= 96. && bounds.bottom() <= 148.);
            assert!(
                glyph.galley.job.sections[0].format.font_id.size >= 18.,
                "Critical live fields must not be shrunk to unreadable labels"
            );
        }
    }

    #[test]
    fn unavailable_or_disconnected_never_claims_zero_live_telemetry() {
        assert_eq!(metrics(None, None, live()).status, "UNASSIGNED");
        assert!(metrics(Some(42), None, live()).stale);
        let t = TelemetryTime::from_observation(Some(1000), Some(20), 1000, false, false);
        assert_eq!(metrics(Some(42), Some(&drone()), t).status, "OFFLINE");
    }
}
