//! Ship-relative contacts drawn on the starboard physical display.
use super::*;

pub(super) const RADAR_CANVAS_SIZE: egui::Vec2 = egui::vec2(300., 180.);

fn header_font(painter: &egui::Painter, rect: egui::Rect) -> f32 {
    let desired = rect.height() * 0.39;
    let width = |text: &str, size: f32| {
        painter
            .layout_no_wrap(
                text.into(),
                egui::FontId::monospace(size),
                egui::Color32::WHITE,
            )
            .size()
            .x
    };
    let available = rect.width() - 16.;
    let mut font =
        desired * (available / (width("RADAR", desired) + width("OFFLINE", desired))).min(1.);
    for _ in 0..8 {
        if width("RADAR", font) + width("OFFLINE", font) <= available {
            break;
        }
        font *= 0.995;
    }
    font
}

#[cfg(test)]
mod feasibility_tests {
    use super::*;
    #[test]
    fn radar_header_fits_narrow_and_wide_canvases() {
        for size in [
            egui::vec2(190., 90.),
            egui::vec2(300., 180.),
            egui::vec2(496., 180.),
        ] {
            for status in ["LIVE", "PAUSED", "STALE", "OFFLINE"] {
                let ctx = egui::Context::default();
                let rect = egui::Rect::from_min_size(egui::pos2(8., 8.), size).shrink(4.);
                let output = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let p = ui.painter();
                        let font = header_font(p, rect);
                        p.text(
                            rect.left_top() + egui::vec2(4., 2.),
                            egui::Align2::LEFT_TOP,
                            "RADAR",
                            egui::FontId::monospace(font),
                            egui::Color32::WHITE,
                        );
                        p.text(
                            rect.right_top() + egui::vec2(-4., 2.),
                            egui::Align2::RIGHT_TOP,
                            status,
                            egui::FontId::monospace(font),
                            egui::Color32::WHITE,
                        );
                    });
                });
                let text: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| {
                        if let egui::Shape::Text(t) = &s.shape {
                            Some(t)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert_eq!(text.len(), 2);
                assert!(text
                    .iter()
                    .all(|t| rect.contains_rect(t.visual_bounding_rect())));
                assert!(!text[0]
                    .visual_bounding_rect()
                    .intersects(text[1].visual_bounding_rect()));
            }
        }
    }
    #[test]
    fn radar_header_feasibility_in_actual_app_projection() {
        let ctx = egui::Context::default();
        let mut app = crate::tests::view_test_cockpit(&ctx);
        for i in 0..4 {
            app.observed = std::time::Instant::now();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 720.),
                    )),
                    time: Some(i as f64 / 60.),
                    focused: true,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut eframe::Frame::_new_kittest()),
            );
        }
        let surface = app
            .flight_view
            .physical_surface(5, RADAR_CANVAS_SIZE)
            .unwrap();
        let snapshot = app.snapshot.as_ref().unwrap();
        for (connected, paused, age, expected, legend) in [
            (true, false, 0, "LIVE", "SHIP RELATIVE"),
            (true, true, 0, "PAUSED", "SHIP RELATIVE / HELD"),
            (true, false, 1501, "STALE", "SHIP RELATIVE"),
            (false, false, 0, "OFFLINE", "SHIP RELATIVE / LAST"),
            (false, true, 0, "OFFLINE", "SHIP RELATIVE / LAST"),
        ] {
            assert_eq!(
                authority(snapshot, snapshot.server_time_ms + age, connected, paused),
                (expected, legend)
            );
            for range in [1., 1_000_000., 1e15] {
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("instrument_text_proof_enabled"), true);
                    d.insert_temp(
                        egui::Id::new("instrument_text_proof_rows"),
                        Vec::<serde_json::Value>::new(),
                    );
                });
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        paint(
                            ui,
                            &surface,
                            &app.own().unwrap(),
                            snapshot,
                            snapshot.server_time_ms + age,
                            connected,
                            paused,
                            range,
                        );
                    });
                });
                let rows = ctx.data(|d| {
                    d.get_temp::<Vec<serde_json::Value>>(egui::Id::new(
                        "instrument_text_proof_rows",
                    ))
                    .unwrap()
                });
                assert_eq!(rows.len(), 4);
                assert!(rows.iter().any(|r| r["text"] == expected));
                assert!(rows.iter().any(|r| r["text"] == legend));
                for (i, row) in rows.iter().enumerate() {
                    assert_eq!(row["ink_clipped"], false, "{row}");
                    assert_eq!(row["clear_samples"], 5, "{row}");
                    assert_eq!(row["elided"], false, "{row}");
                    if row["text"] == "RADAR" || row["text"] == expected {
                        assert!(
                            row["projected_font_em_px"].as_f64().unwrap() >= 14.,
                            "{row}"
                        );
                    }
                    let rect = |r: &serde_json::Value| {
                        let b = &r["ink_bounds"];
                        egui::Rect::from_min_max(
                            egui::pos2(
                                b[0].as_f64().unwrap() as f32,
                                b[1].as_f64().unwrap() as f32,
                            ),
                            egui::pos2(
                                b[2].as_f64().unwrap() as f32,
                                b[3].as_f64().unwrap() as f32,
                            ),
                        )
                    };
                    for other in &rows[i + 1..] {
                        assert!(!rect(row).intersects(rect(other)), "{row} overlaps {other}");
                    }
                }
                if !connected && !paused && range == 1_000_000. {
                    let report = serde_json::json!({"scope":"Production radar offline state on unchanged actual App projection","camera":app.flight_view.editor_camera_pose().map(|(p,y,t)|(p.to_array(),y,t)),"text":rows});
                    if let Ok(path) = std::env::var("ARKLINE_RADAR_FEASIBILITY_REPORT") {
                        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
                    }
                    eprintln!("{report}");
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint(
    ui: &egui::Ui,
    surface: &CockpitSurface,
    own: &PilotView,
    snapshot: &Snapshot,
    now: u64,
    connected: bool,
    paused: bool,
    range: f64,
) {
    let origin = display_position(own, now);
    let rotation = ship_orientation_at(own, now).inverse();
    let range = range.max(1.0);
    let project = |position: [f64; 3]| -> Option<(egui::Vec2, egui::Vec2)> {
        let relative = glam::DVec3::from_array(std::array::from_fn(|i| position[i] - origin[i]));
        if !relative.is_finite() || relative.length() > range {
            return None;
        }
        let body = rotation * (relative / range).as_vec3();
        Some((
            egui::vec2(body.z, -body.x * 0.55 - body.y * 0.8),
            egui::vec2(body.z, -body.x * 0.55),
        ))
    };
    surface.paint(ui, |painter| {
        let rect = surface.canvas_rect().shrink(4.);
        painter.rect_filled(rect, 0., egui::Color32::from_rgb(0, 20, 15));
        let (status, legend) = authority(snapshot, now, connected, paused);
        let color = if status == "LIVE" {
            super::super::TEAL
        } else {
            super::super::AMBER
        };
        // Fit the widest status, keeping the header stable across authority changes.
        let font = header_font(painter, rect);
        painter.text(
            rect.left_top() + egui::vec2(4., 2.),
            egui::Align2::LEFT_TOP,
            "RADAR",
            egui::FontId::monospace(font),
            color,
        );
        painter.text(
            rect.right_top() + egui::vec2(-4., 2.),
            egui::Align2::RIGHT_TOP,
            status,
            egui::FontId::monospace(font),
            color,
        );
        let map = egui::Rect::from_min_max(
            rect.min + egui::vec2(4., font * 1.2 + 6.),
            egui::pos2(rect.left() + rect.width() * 0.40, rect.bottom() - 4.),
        );
        let center = map.center();
        let radius = map.width().min(map.height()) * 0.43;
        for fraction in [0.25, 0.5, 1.] {
            let points = (0..=64)
                .map(|n| {
                    let angle = n as f32 / 64. * std::f32::consts::TAU;
                    center + egui::vec2(angle.sin(), angle.cos() * 0.55) * radius * fraction
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                egui::Stroke::new(0.8, color.gamma_multiply(0.4)),
            ));
        }
        let contact = |position, drone| {
            if let Some((point, baseline)) = project(position) {
                let point = center + point * radius;
                painter.line_segment(
                    [center + baseline * radius, point],
                    egui::Stroke::new(0.7, color.gamma_multiply(0.5)),
                );
                if drone {
                    painter.rect_stroke(
                        egui::Rect::from_center_size(point, egui::vec2(4., 4.)),
                        0.,
                        egui::Stroke::new(1., super::super::AMBER),
                        egui::StrokeKind::Inside,
                    );
                } else {
                    painter.circle_filled(point, 2., color);
                }
            }
        };
        for asteroid in snapshot
            .asteroids
            .iter()
            .filter(|a| a.system_id == own.system_id && a.ore > 0)
        {
            contact(asteroid.position, false);
        }
        for pilot in snapshot
            .pilots
            .iter()
            .filter(|p| p.system_id == own.system_id)
        {
            if pilot.name != own.name && pilot.location != Location::Docked {
                contact(display_position(pilot, now), false);
            }
            for drone in fleet(pilot) {
                contact(drone.position, true);
            }
        }
        painter.circle_filled(center, 3., color);
        let readout = egui::pos2(rect.left() + rect.width() * 0.43, rect.bottom() - 2.);
        painter.text(
            readout,
            egui::Align2::LEFT_BOTTOM,
            super::super::format_distance(range),
            egui::FontId::monospace(rect.height() * 0.20),
            color,
        );
        painter.text(
            readout - egui::vec2(0., rect.height() * 0.24),
            egui::Align2::LEFT_BOTTOM,
            legend,
            egui::FontId::monospace(rect.height() * 0.075),
            color,
        );
    });
}

fn authority(
    snapshot: &Snapshot,
    now: u64,
    connected: bool,
    paused: bool,
) -> (&'static str, &'static str) {
    if !connected {
        ("OFFLINE", "SHIP RELATIVE / LAST")
    } else if paused {
        ("PAUSED", "SHIP RELATIVE / HELD")
    } else if now.saturating_sub(snapshot.server_time_ms) > 1500 {
        ("STALE", "SHIP RELATIVE")
    } else {
        ("LIVE", "SHIP RELATIVE")
    }
}
