//! Read-only navigation content from confirmed snapshot fields; no lock prediction.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavigationPanelId {
    ShipNavigation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NavigationReadingId {
    SelectedCenterDistanceM,
    SystemId,
    LocalShips,
    LocalAsteroids,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum NavigationReadingValue {
    Meters(f64),
    Identifier(u64),
    Count(usize),
}

pub(super) struct NavigationReadings {
    pub(super) panel_id: NavigationPanelId,
    selected_center_distance_m: Option<f64>,
    system_id: u64,
    local_ships: usize,
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

    pub(super) fn reading(&self, id: NavigationReadingId) -> Option<NavigationReadingValue> {
        match id {
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

    fn confirmed_label(&self) -> String {
        if self.confirmed_ids.is_empty() {
            return "LOCKED 0 / NONE CONFIRMED".into();
        }
        format!(
            "LOCKED {} · {}",
            self.confirmed_ids.len(),
            self.confirmed_ids
                .iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    }

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
    let text = |bounds: egui::Rect, content: String, size: f32, tone, align| {
        let mut job = egui::text::LayoutJob::simple(
            content,
            egui::FontId::monospace(size * scale),
            tone,
            bounds.width(),
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        let galley = painter.layout_job(job);
        let pos = match align {
            egui::Align::Center => bounds.center_top() - egui::vec2(galley.size().x * 0.5, 0.0),
            _ => bounds.left_top(),
        };
        painter.galley(pos, galley, tone);
    };
    text(
        egui::Rect::from_min_max(origin(0.02, 0.005), origin(0.98, 0.12)),
        authority_header.into(),
        13.,
        color,
        egui::Align::Min,
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
    let label = if readings.docked {
        "DOCKED".into()
    } else {
        readings.heading_rad.map_or("NO AZIMUTH".into(), |r| {
            format!("{:03.0}°", r.to_degrees().rem_euclid(360.))
        })
    };
    text(
        egui::Rect::from_min_max(
            center + egui::vec2(-radius * 0.83, -radius * 0.27),
            center + egui::vec2(radius * 0.83, radius * 0.15),
        ),
        label,
        15.,
        ink,
        egui::Align::Center,
    );
    text(
        egui::Rect::from_min_max(origin(0.10, 0.645), origin(0.40, 0.735)),
        "+X 0° / +Z 90°".into(),
        9.,
        muted,
        egui::Align::Center,
    );
    let [[u0, v0], [u1, v1]] = arkline_protocol::PIONEER_RANGE_MODULE_RECT;
    let range =
        egui::Rect::from_min_max(origin(u0 as f32, v0 as f32), origin(u1 as f32, v1 as f32));
    painter.rect_filled(range, 2., egui::Color32::from_rgb(4, 11, 13));
    text(
        egui::Rect::from_min_max(origin(0.60, 0.18), origin(0.95, 0.28)),
        "SELECTED RANGE".into(),
        12.,
        color,
        egui::Align::Min,
    );
    text(
        egui::Rect::from_min_max(origin(0.60, 0.30), origin(0.95, 0.51)),
        readings
            .selected_center_distance_m
            .map_or("RANGE UNAVAILABLE".into(), super::super::format_distance),
        if readings.selected_center_distance_m.is_some() {
            25.
        } else {
            12.
        },
        ink,
        egui::Align::Min,
    );
    text(
        egui::Rect::from_min_max(origin(0.60, 0.53), origin(0.95, 0.63)),
        readings.speed_mps.map_or("SPEED UNAVAILABLE".into(), |v| {
            super::super::format_speed(v)
        }),
        13.,
        ink,
        egui::Align::Min,
    );
    text(
        egui::Rect::from_min_max(origin(0.60, 0.63), origin(0.95, 0.70)),
        format!(
            "SYS {} · {} CONTACTS",
            readings.system_id, readings.local_ships
        ),
        9.,
        muted,
        egui::Align::Min,
    );
    text(
        egui::Rect::from_min_max(origin(0.02, 0.75), origin(0.98, 0.86)),
        readings.confirmed_label(),
        12.,
        ink,
        egui::Align::Min,
    );
    text(
        egui::Rect::from_min_max(origin(0.02, 0.875), origin(0.98, 0.99)),
        readings.pending_label(),
        12.,
        color,
        egui::Align::Min,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for size in [egui::vec2(256.0, 128.0), egui::vec2(300.0, 180.0)] {
            for distance in [Some(5000.0), None] {
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
                assert!(labels.contains(&"LOCKED 2 · #7 #8"));
                assert!(labels.iter().any(|label| label.starts_with("ACQUIRING 2")));
                assert!(labels.contains(
                    &distance
                        .map_or_else(
                            || "RANGE UNAVAILABLE".into(),
                            super::super::super::format_distance
                        )
                        .as_str()
                ));
                assert_eq!(serde_json::to_value(&snapshot).unwrap(), before);
            }
        }
    }
}
