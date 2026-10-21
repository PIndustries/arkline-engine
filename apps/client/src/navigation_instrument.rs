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
        Self {
            panel_id: NavigationPanelId::ShipNavigation,
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
    let scale = (rect.width() / 246.0).min(rect.height() / 118.0);
    let ink = egui::Color32::from_rgb(185, 226, 208);
    let line = |text: String, y: f32, size, color| {
        let mut job = egui::text::LayoutJob::simple(
            text,
            egui::FontId::monospace(size * scale),
            color,
            rect.width() - 6.0,
        );
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        job.wrap.overflow_character = Some('…');
        painter.galley(
            rect.min + egui::vec2(3.0, y * scale),
            painter.layout_job(job),
            color,
        );
    };
    line(authority_header.into(), 0.0, 14.0, color);
    line(
        match readings.reading(NavigationReadingId::SelectedCenterDistanceM) {
            Some(NavigationReadingValue::Meters(meters)) => super::super::format_distance(meters),
            _ => "RANGE UNAVAILABLE".into(),
        },
        18.0,
        if readings.selected_center_distance_m.is_some() {
            28.0
        } else {
            20.0
        },
        ink,
    );
    line("SELECTED CENTER DISTANCE".into(), 52.0, 12.0, ink);
    // Integer identity/counts retain exact formatting, not floating-point labels.
    line(
        format!(
            "SYSTEM {} · SHIPS {} · ROCKS {}",
            readings.system_id, readings.local_ships, readings.local_asteroids
        ),
        68.0,
        12.0,
        ink,
    );
    line(readings.confirmed_label(), 84.0, 13.0, ink);
    line(readings.pending_label(), 101.0, 13.0, ink);
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
                assert_eq!(text_shapes.len(), 6, "all six glance readings stay visible");
                for pair in text_shapes.windows(2) {
                    assert!(
                        pair[0].visual_bounding_rect().bottom()
                            <= pair[1].visual_bounding_rect().top(),
                        "reading bands overlap"
                    );
                }
                let scale = (rect.width() / 246.0).min(rect.height() / 118.0);
                assert!(text_shapes[0].galley.job.sections[0].format.font_id.size >= 14.0 * scale);
                if distance.is_some() {
                    assert!(
                        text_shapes[1].galley.job.sections[0].format.font_id.size >= 28.0 * scale
                    );
                }
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
