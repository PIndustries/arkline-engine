//! Read-only navigation content from confirmed snapshot fields; no lock prediction.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavigationPanelId {
    ShipNavigation,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavigationReadingId {
    SelectedCenterDistanceM,
    SystemAzimuthRad,
    SpeedMps,
    SystemId,
    LocalShips,
    LocalAsteroids,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum NavigationReadingValue {
    Meters(f64),
    Radians(f32),
    MetersPerSecond(f64),
    Identifier(u64),
    Count(usize),
}

pub(super) struct NavigationReadings {
    pub(super) panel_id: NavigationPanelId,
    selected_center_distance_m: Option<f64>,
    system_id: u64,
    local_ships: usize,
    #[cfg(test)]
    local_asteroids: usize,
    confirmed_ids: Vec<u64>,
    pending: Vec<arkline_protocol::TargetLock>,
    now_ms: u64,
    docked: bool,
    heading_rad: Option<f32>,
    speed_mps: Option<f64>,
}

impl NavigationReadings {
    pub(super) fn from_pilot(
        own: &PilotView,
        snapshot: &Snapshot,
        selected_center_distance_m: Option<f64>,
        now_ms: u64,
    ) -> Self {
        let mut pending = std::collections::BTreeMap::new();
        // Plural state wins over duplicate compatibility fields, as in the overlay.
        for lock in own.target_locks.iter().chain(own.target_lock.iter()) {
            pending
                .entry(lock.asteroid_id)
                .or_insert_with(|| lock.clone());
        }
        let mut confirmed_ids = own.locked_asteroids.clone();
        confirmed_ids.extend(own.locked_asteroid);
        confirmed_ids.sort_unstable();
        confirmed_ids.dedup();
        // Conflicting compatibility fields cannot manufacture confirmed success.
        confirmed_ids.retain(|id| !pending.contains_key(id));
        let docked = own.location == Location::Docked;
        let heading_rad = own.flight.as_ref().filter(|_| !docked).and_then(|flight| {
            let h = flight.warp.as_ref().map_or(flight.heading, |warp| {
                arkline_protocol::sample_warp_heading(warp, now_ms)
            });
            (h.iter().all(|v| v.is_finite()) && h[0].hypot(h[2]) > 1e-8)
                .then(|| h[2].atan2(h[0]) as f32)
        });
        let speed_mps = own.flight.as_ref().and_then(|flight| {
            let velocity = flight.warp.as_ref().map_or(flight.velocity, |warp| {
                arkline_protocol::sample_warp(warp, now_ms).1
            });
            let speed = velocity.iter().map(|v| v * v).sum::<f64>().sqrt();
            speed.is_finite().then_some(speed)
        });
        Self {
            panel_id: NavigationPanelId::ShipNavigation,
            docked,
            heading_rad,
            speed_mps,
            selected_center_distance_m: selected_center_distance_m
                .filter(|m| m.is_finite() && *m >= 0.0),
            system_id: u64::from(own.system_id),
            local_ships: snapshot
                .pilots
                .iter()
                .filter(|p| {
                    p.system_id == own.system_id
                        && p.name != own.name
                        && p.location != Location::Docked
                })
                .count(),
            #[cfg(test)]
            local_asteroids: snapshot
                .asteroids
                .iter()
                .filter(|a| a.system_id == own.system_id)
                .count(),
            confirmed_ids,
            pending: pending.into_values().collect(),
            now_ms,
        }
    }

    #[cfg(test)]
    pub(super) fn reading(&self, id: NavigationReadingId) -> Option<NavigationReadingValue> {
        match id {
            NavigationReadingId::SystemAzimuthRad => {
                self.heading_rad.map(NavigationReadingValue::Radians)
            }
            NavigationReadingId::SpeedMps => {
                self.speed_mps.map(NavigationReadingValue::MetersPerSecond)
            }
            NavigationReadingId::SelectedCenterDistanceM => self
                .selected_center_distance_m
                .map(NavigationReadingValue::Meters),
            NavigationReadingId::SystemId => {
                Some(NavigationReadingValue::Identifier(self.system_id))
            }
            NavigationReadingId::LocalShips => {
                Some(NavigationReadingValue::Count(self.local_ships))
            }
            NavigationReadingId::LocalAsteroids => {
                Some(NavigationReadingValue::Count(self.local_asteroids))
            }
        }
    }

    #[cfg(test)]
    fn pending_label(&self) -> String {
        if self.pending.is_empty() {
            return "ACQUIRING 0".into();
        }
        format!(
            "ACQUIRING {} · {}",
            self.pending.len(),
            self.pending
                .iter()
                .map(|lock| {
                    let remaining = lock.completes_at_ms.saturating_sub(self.now_ms);
                    if remaining == 0 {
                        format!("#{} AWAIT CONFIRM", lock.asteroid_id)
                    } else {
                        format!("#{} {:.1}s", lock.asteroid_id, remaining as f64 / 1000.0)
                    }
                })
                .collect::<Vec<_>>()
                .join(" · ")
        )
    }
}

/// Same units/rounding as the other ship interfaces, separated from the number
/// so a narrow instrument can never hide its unit behind an ellipsis.
fn quantity_parts(value: Option<f64>, speed: bool) -> Option<(String, String)> {
    let value = value.filter(|v| v.is_finite() && *v >= 0.0)?;
    let formatted = if speed {
        super::super::format_speed(value)
    } else {
        super::super::format_distance(value)
    };
    let (number, unit) = formatted.rsplit_once(' ')?;
    let number = if number.len() > 6 {
        format!("{:.2e}", number.parse::<f64>().ok()?)
    } else {
        number.to_owned()
    };
    Some((number, unit.to_owned()))
}

/// Fit actual ink rather than row leading. No wrapping, elision or lost units.
fn face_text(
    painter: &egui::Painter,
    bounds: egui::Rect,
    content: String,
    size: f32,
    tone: egui::Color32,
    centered: bool,
) {
    if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
        return;
    }
    let mut font_size = size;
    let mut galley =
        painter.layout_no_wrap(content.clone(), egui::FontId::monospace(font_size), tone);
    for _ in 0..4 {
        let ink = galley.mesh_bounds;
        let fit = (bounds.width() / ink.width().max(0.01))
            .min(bounds.height() / ink.height().max(0.01))
            .min(1.0);
        if fit >= 1.0 {
            break;
        }
        font_size *= fit * 0.99;
        galley = painter.layout_no_wrap(content.clone(), egui::FontId::monospace(font_size), tone);
    }
    let ink = galley.mesh_bounds;
    let x = if centered {
        bounds.center().x - ink.width() * 0.5
    } else {
        bounds.left()
    };
    painter.galley(
        egui::pos2(x, bounds.center().y - ink.height() * 0.5) - ink.min.to_vec2(),
        galley,
        tone,
    );
}

pub(super) fn paint(
    painter: &egui::Painter,
    rect: egui::Rect,
    authority_header: &str,
    color: egui::Color32,
    readings: &NavigationReadings,
) {
    debug_assert_eq!(readings.panel_id, NavigationPanelId::ShipNavigation);
    let scale = rect.height() / 118.0;
    let ink = egui::Color32::from_rgb(202, 228, 215);
    let muted = egui::Color32::from_rgb(114, 147, 137);
    let origin = |u: f32, v: f32| rect.min + egui::vec2(u * rect.width(), v * rect.height());
    let text = |bounds, content, size, tone, centered| {
        face_text(painter, bounds, content, size * scale, tone, centered)
    };
    text(
        egui::Rect::from_min_max(origin(0.02, 0.005), origin(0.98, 0.13)),
        authority_header.into(),
        13.,
        color,
        false,
    );
    let dial = arkline_protocol::PIONEER_DIAL_CENTERS[0];
    let center = origin(dial[0] as f32, dial[1] as f32);
    let radius = rect.height() * arkline_protocol::PIONEER_DIAL_RADIUS_HEIGHT as f32;
    painter.circle_filled(center, radius, egui::Color32::from_rgb(5, 12, 13));
    painter.circle_stroke(
        center,
        radius * 0.94,
        egui::Stroke::new(1.2, color.gamma_multiply(0.5)),
    );
    for tick in 0..24 {
        let angle = tick as f32 * std::f32::consts::TAU / 24.;
        let dir = egui::vec2(angle.sin(), -angle.cos());
        painter.line_segment(
            [
                center + dir * radius * 0.87,
                center + dir * radius * if tick % 6 == 0 { 0.68 } else { 0.78 },
            ],
            egui::Stroke::new(if tick % 6 == 0 { 1.4 } else { 0.7 }, muted),
        );
    }
    if let Some(angle) = readings.heading_rad {
        let dir = egui::vec2(angle.sin(), -angle.cos());
        painter.line_segment(
            [center - dir * radius * 0.45, center + dir * radius * 0.72],
            egui::Stroke::new(2., color),
        );
        painter.circle_filled(center, 2. * scale, color);
    }
    let (heading, heading_size) = if readings.docked {
        ("DOCKED".into(), 15.)
    } else {
        readings
            .heading_rad
            .map_or(("NO AZIMUTH".into(), 11.), |r| {
                (
                    format!(
                        "{:03}°",
                        (r.to_degrees().rem_euclid(360.).round() as u32) % 360
                    ),
                    21.,
                )
            })
    };
    let heading_window = egui::Rect::from_min_max(
        center + egui::vec2(-radius * 0.83, -radius * 0.38),
        center + egui::vec2(radius * 0.83, radius * 0.38),
    );
    // Recessed-looking digital window keeps the real needle from crossing the
    // heading glyphs. Needle and tick positions remain unchanged around it.
    painter.rect_filled(heading_window, 2., egui::Color32::from_rgb(4, 10, 12));
    text(heading_window, heading, heading_size, ink, true);
    text(
        egui::Rect::from_min_max(origin(0.06, 0.705), origin(0.44, 0.78)),
        "+X 0° / +Z 90°".into(),
        10.,
        muted,
        true,
    );
    let [[u0, v0], [u1, v1]] = arkline_protocol::PIONEER_RANGE_MODULE_RECT;
    let range =
        egui::Rect::from_min_max(origin(u0 as f32, v0 as f32), origin(u1 as f32, v1 as f32));
    painter.rect_filled(range, 2., egui::Color32::from_rgb(4, 11, 13));
    let range_parts = quantity_parts(readings.selected_center_distance_m, false);
    text(
        egui::Rect::from_min_max(origin(0.60, 0.18), origin(0.95, 0.28)),
        range_parts
            .as_ref()
            .map_or("RANGE".into(), |(_, u)| format!("RANGE · {u}")),
        12.,
        color,
        false,
    );
    text(
        egui::Rect::from_min_max(origin(0.60, 0.29), origin(0.95, 0.51)),
        range_parts
            .as_ref()
            .map_or("UNAVAILABLE".into(), |(v, _)| v.clone()),
        if range_parts.is_some() { 28. } else { 12. },
        ink,
        false,
    );
    let speed = quantity_parts(readings.speed_mps, true)
        .map_or("SPEED UNAVAILABLE".into(), |(v, u)| format!("{v} {u}"));
    text(
        egui::Rect::from_min_max(origin(0.60, 0.53), origin(0.95, 0.70)),
        speed,
        20.,
        ink,
        false,
    );
    // Small identifiers stay secondary; primary measured values occupy the
    // actual dial/range backings. Full target lists remain in target management.
    text(
        egui::Rect::from_min_max(origin(0.02, 0.79), origin(0.98, 0.855)),
        format!(
            "SYS {} · {} CONTACTS",
            readings.system_id, readings.local_ships
        ),
        10.,
        muted,
        false,
    );
    let awaiting = readings
        .pending
        .iter()
        .filter(|lock| lock.completes_at_ms <= readings.now_ms)
        .count();
    text(
        egui::Rect::from_min_max(origin(0.02, 0.88), origin(0.44, 0.99)),
        format!("LOCKED {}", readings.confirmed_ids.len()),
        13.,
        ink,
        false,
    );
    text(
        egui::Rect::from_min_max(origin(0.46, 0.88), origin(0.98, 0.99)),
        if awaiting == 0 {
            format!("ACQ {}", readings.pending.len())
        } else {
            format!("ACQ{} · WAIT CONF{}", readings.pending.len(), awaiting)
        },
        12.,
        color,
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_quantity_units_and_frame_identity_are_explicit() {
        let au = arkline_protocol::AU_METERS;
        for (value, unit) in [
            (999.0, "m"),
            (1000.0, "km"),
            (1_000_000.0, "km"),
            (au * 0.01, "AU"),
            (au * 2.0, "AU"),
            (f64::MAX, "AU"),
        ] {
            let (number, actual_unit) = quantity_parts(Some(value), false).unwrap();
            assert_eq!(actual_unit, unit);
            assert!(number.parse::<f64>().unwrap().is_finite());
        }
        for (value, unit) in [(0.0, "m/s"), (1000.0, "km/s"), (au * 0.001, "AU/s")] {
            assert_eq!(quantity_parts(Some(value), true).unwrap().1, unit);
        }
        for value in [None, Some(-1.0), Some(f64::NAN), Some(f64::INFINITY)] {
            assert!(quantity_parts(value, false).is_none());
        }
        let ctx = egui::Context::default();
        let cockpit = crate::tests::view_test_cockpit(&ctx);
        let snapshot = cockpit.snapshot.unwrap();
        let mut own = snapshot.pilots[0].clone();
        own.location = Location::Station;
        for (heading, degrees) in [
            ([1.0, 0.0, 0.0], 0.0_f32),
            ([0.0, 0.0, 1.0], 90.0),
            ([-1.0, 0.0, 0.0], 180.0),
            ([0.0, 0.0, -1.0], 270.0),
        ] {
            let flight = own.flight.as_mut().unwrap();
            flight.heading = heading;
            flight.velocity = [3.0, 4.0, 0.0];
            flight.warp = None;
            let readings = NavigationReadings::from_pilot(&own, &snapshot, None, 2500);
            assert_eq!(
                readings.reading(NavigationReadingId::SpeedMps),
                Some(NavigationReadingValue::MetersPerSecond(5.0))
            );
            let Some(NavigationReadingValue::Radians(angle)) =
                readings.reading(NavigationReadingId::SystemAzimuthRad)
            else {
                panic!("Missing system azimuth");
            };
            assert!((angle.to_degrees().rem_euclid(360.0) - degrees).abs() < 0.001);
        }
        own.location = Location::Docked;
        assert_eq!(
            NavigationReadings::from_pilot(&own, &snapshot, None, 2500)
                .reading(NavigationReadingId::SystemAzimuthRad),
            None
        );
        own.location = Location::Station;
        own.flight.as_mut().unwrap().heading = [0.0, 1.0, 0.0];
        assert_eq!(
            NavigationReadings::from_pilot(&own, &snapshot, None, 2500)
                .reading(NavigationReadingId::SystemAzimuthRad),
            None
        );
    }

    #[test]
    fn navigation_card_merges_actual_locks_without_predicting_success_and_fits() {
        let ctx = egui::Context::default();
        let cockpit = crate::tests::view_test_cockpit(&ctx);
        let mut snapshot = cockpit.snapshot.unwrap();
        let own = &mut snapshot.pilots[0];
        own.locked_asteroids = vec![7, 8, 8, 9];
        own.locked_asteroid = Some(8);
        own.target_locks = vec![
            arkline_protocol::TargetLock {
                asteroid_id: 9,
                started_at_ms: 1000,
                completes_at_ms: 2000,
            },
            arkline_protocol::TargetLock {
                asteroid_id: 10,
                started_at_ms: 1000,
                completes_at_ms: 4000,
            },
        ];
        own.target_lock = Some(own.target_locks[1].clone());
        let mut compatibility = snapshot.pilots[0].clone();
        compatibility.locked_asteroid = Some(12);
        compatibility.target_lock = Some(arkline_protocol::TargetLock {
            asteroid_id: 13,
            started_at_ms: 1000,
            completes_at_ms: 5000,
        });
        let compatibility_readings =
            NavigationReadings::from_pilot(&compatibility, &snapshot, None, 2500);
        assert!(compatibility_readings.confirmed_ids.contains(&12));
        assert!(compatibility_readings
            .pending
            .iter()
            .any(|lock| lock.asteroid_id == 13));
        // The protocol currently uses u32 system IDs; the content identity contract
        // also preserves any future u64 identifier exactly, without floating conversion.
        let mut identity_contract =
            NavigationReadings::from_pilot(&snapshot.pilots[0], &snapshot, None, 2500);
        identity_contract.system_id = u64::MAX;
        assert_eq!(
            identity_contract.reading(NavigationReadingId::SystemId),
            Some(NavigationReadingValue::Identifier(u64::MAX))
        );
        let before = serde_json::to_value(&snapshot).unwrap();
        for size in [
            egui::vec2(256.0, 128.0),
            egui::vec2(246.0, 118.0),
            egui::vec2(200.0, 100.0),
            egui::vec2(300.0, 180.0),
        ] {
            for distance in [
                Some(5000.0),
                Some(2.0 * arkline_protocol::AU_METERS),
                Some(f64::MAX),
                None,
            ] {
                let readings =
                    NavigationReadings::from_pilot(&snapshot.pilots[0], &snapshot, distance, 2500);
                assert_eq!(readings.confirmed_ids, [7, 8]);
                assert_eq!(
                    readings
                        .pending
                        .iter()
                        .map(|lock| lock.asteroid_id)
                        .collect::<Vec<_>>(),
                    [9, 10]
                );
                assert!(readings.pending_label().contains("#9 AWAIT CONFIRM"));
                assert!(readings.pending_label().contains("#10 1.5s"));
                assert_eq!(
                    readings.reading(NavigationReadingId::SelectedCenterDistanceM),
                    distance.map(NavigationReadingValue::Meters)
                );
                assert_eq!(
                    readings.reading(NavigationReadingId::SystemId),
                    Some(NavigationReadingValue::Identifier(u64::from(
                        snapshot.pilots[0].system_id
                    )))
                );
                assert_eq!(
                    readings.reading(NavigationReadingId::LocalShips),
                    Some(NavigationReadingValue::Count(0))
                );
                assert_eq!(
                    readings.reading(NavigationReadingId::LocalAsteroids),
                    Some(NavigationReadingValue::Count(
                        snapshot
                            .asteroids
                            .iter()
                            .filter(|a| a.system_id == snapshot.pilots[0].system_id)
                            .count()
                    ))
                );
                let rect = egui::Rect::from_min_size(egui::pos2(8.0, 8.0), size).shrink(5.0);
                let output = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        paint(
                            &ui.painter().with_clip_rect(rect),
                            rect,
                            "NAV / PAUSED",
                            super::super::super::AMBER,
                            &readings,
                        );
                    });
                });
                let text_shapes: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            Some(text)
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(
                    text_shapes.len() >= 8,
                    "dedicated bearing/range plus real lock status"
                );
                let labels: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            assert!(
                                rect.contains_rect(text.visual_bounding_rect()),
                                "{:?}",
                                text.galley.job.text
                            );
                            Some(text.galley.job.text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect();
                assert!(labels.contains(&"NAV / PAUSED"));
                assert!(labels.contains(&"LOCKED 2"));
                assert!(labels.contains(&"ACQ2 · WAIT CONF1"));
                let parts = quantity_parts(distance, false);
                assert!(labels.contains(
                    &parts
                        .as_ref()
                        .map_or("UNAVAILABLE".into(), |(v, _)| v.clone())
                        .as_str()
                ));
                assert!(labels.contains(
                    &parts
                        .as_ref()
                        .map_or("RANGE".into(), |(_, u)| format!("RANGE · {u}"))
                        .as_str()
                ));
                for text in &text_shapes {
                    assert!(!text.galley.elided, "Elided real instrument reading");
                    let rendered: String = text
                        .galley
                        .rows
                        .iter()
                        .flat_map(|row| row.glyphs.iter())
                        .map(|g| g.chr)
                        .collect();
                    assert_eq!(rendered, text.galley.job.text, "Lost visible glyphs");
                    let primary = parts.as_ref().is_some_and(|(v, u)| {
                        text.galley.job.text == *v || text.galley.job.text == format!("RANGE · {u}")
                    });
                    if primary {
                        let [[u0, v0], [u1, v1]] = arkline_protocol::PIONEER_RANGE_MODULE_RECT;
                        let module = egui::Rect::from_min_max(
                            rect.min
                                + egui::vec2(u0 as f32 * rect.width(), v0 as f32 * rect.height()),
                            rect.min
                                + egui::vec2(u1 as f32 * rect.width(), v1 as f32 * rect.height()),
                        );
                        assert!(
                            module.contains_rect(text.visual_bounding_rect()),
                            "Primary reading escaped its physical module"
                        );
                    }
                }
                assert_eq!(serde_json::to_value(&snapshot).unwrap(), before);
            }
        }
    }
}
