//! Permanent read-only onboard cargo. Generic ore units are not mass or volume;
//! personal station storage and lifetime job totals never inflate the ship hold.
use super::*;
use crate::drone_telemetry::{Availability, TelemetryTime};

#[derive(Debug, PartialEq)]
struct CargoReadings {
    owner: String,
    ship: &'static str,
    vessel_id: Option<String>,
    used: u64,
    capacity: u64,
    free: u64,
    fill: f32,
    invalid: bool,
    job: Option<arkline_protocol::ResourceJob>,
    time: TelemetryTime,
}
impl CargoReadings {
    fn new(pilot: &PilotView, snapshot: &Snapshot, time: TelemetryTime) -> Self {
        let capacity = arkline_protocol::cargo_capacity(pilot.ship_class);
        let used = pilot.ore;
        Self {
            owner: pilot.name.clone(),
            ship: pilot.ship_class.spec().name,
            vessel_id: pilot
                .cockpit_mechanisms
                .as_ref()
                .map(|s| s.vessel_id.clone())
                .or_else(|| {
                    pilot
                        .reactor_configuration
                        .as_ref()
                        .map(|s| s.vessel_id.clone())
                })
                .filter(|id| !id.is_empty()),
            used,
            capacity,
            free: capacity.saturating_sub(used),
            fill: if capacity == 0 {
                0.
            } else {
                (used as f64 / capacity as f64).clamp(0., 1.) as f32
            },
            invalid: capacity == 0 || used > capacity,
            job: snapshot
                .resource_jobs
                .iter()
                .find(|job| job.pilot == pilot.name)
                .cloned(),
            time,
        }
    }
    fn contents(&self) -> String {
        format!("Ore {} units", self.used)
    }
    fn identity(&self) -> String {
        self.vessel_id.as_ref().map_or_else(
            || format!("{} / {} / vessel ID unavailable", self.ship, self.owner),
            |id| format!("{} / {id}", self.ship),
        )
    }
}

fn text(p: &egui::Painter, r: egui::Rect, value: &str, size: f32, color: egui::Color32) {
    let galley = p.layout_no_wrap(value.into(), egui::FontId::monospace(size), color);
    let factor = (r.width() / galley.size().x.max(1.))
        .min(r.height() / galley.size().y.max(1.))
        .min(1.);
    let galley = if factor < 1. {
        p.layout_no_wrap(value.into(), egui::FontId::monospace(size * factor), color)
    } else {
        galley
    };
    p.galley(
        egui::pos2(r.left(), r.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}

fn matching_unload<'a>(
    pilot: &PilotView,
    vessel_id: Option<&str>,
    op: Option<&'a arkline_protocol::CargoUnloadOperation>,
) -> Option<&'a arkline_protocol::CargoUnloadOperation> {
    op.filter(|op| {
        op.pilot == pilot.name
            && op.ship_class == pilot.ship_class
            && vessel_id == Some(op.vessel_id.as_str())
    })
}

pub(super) fn paint(
    ui: &egui::Ui,
    surface: &CockpitSurface,
    pilot: &PilotView,
    snapshot: &Snapshot,
    time: TelemetryTime,
    unload: Option<&arkline_protocol::CargoUnloadOperation>,
) {
    let data = CargoReadings::new(pilot, snapshot, time);
    let unload = matching_unload(pilot, data.vessel_id.as_deref(), unload);
    let accent = if time.availability == Availability::Live && !data.invalid {
        crate::TEAL
    } else {
        crate::AMBER
    };
    surface.paint(ui, |p| {
        let canvas = surface.canvas_rect();
        p.rect_filled(canvas, 0., egui::Color32::from_rgb(2, 14, 10));
        let r = canvas.shrink(canvas.height() * 0.035);
        let scale = canvas.height() / 240.;
        let row = |a: f32, b: f32| {
            egui::Rect::from_min_max(
                egui::pos2(r.left(), r.top() + a * r.height()),
                egui::pos2(r.right(), r.top() + b * r.height()),
            )
        };
        let foreground = egui::Color32::from_rgb(202, 228, 215);
        text(p, row(0., 0.12), "CARGO / ONBOARD", 25. * scale, accent);
        text(
            p,
            row(0.13, 0.21),
            &data.identity(),
            13. * scale,
            foreground,
        );
        text(
            p,
            row(0.22, 0.29),
            &format!("USED / CAPACITY · {}", data.time.availability.label()),
            13. * scale,
            accent,
        );
        text(
            p,
            row(0.30, 0.45),
            &format!("{} / {} units", data.used, data.capacity),
            34. * scale,
            foreground,
        );
        let track = row(0.47, 0.55);
        p.rect_filled(track, 2., egui::Color32::from_rgb(20, 43, 38));
        p.rect_filled(
            egui::Rect::from_min_size(
                track.min,
                egui::vec2(track.width() * data.fill, track.height()),
            ),
            2.,
            accent.gamma_multiply(0.6),
        );
        if data.invalid {
            text(
                p,
                track,
                "OVER CAPACITY / INVALID HOLD STATE",
                14. * scale,
                crate::AMBER,
            );
        }
        text(
            p,
            row(0.57, 0.65),
            &format!("FREE {} units · {}", data.free, data.contents()),
            16. * scale,
            foreground,
        );
        p.line_segment(
            [row(0.68, 0.68).left_top(), row(0.68, 0.68).right_top()],
            egui::Stroke::new(1., accent.gamma_multiply(0.4)),
        );
        if let Some(op) = unload {
            text(
                p,
                row(0.70, 0.78),
                &format!(
                    "{} #{} · {}",
                    op.phase.label().to_uppercase(),
                    op.operation_id,
                    data.time.availability.label()
                ),
                13. * scale,
                accent,
            );
            text(
                p,
                row(0.80, 0.88),
                &format!(
                    "CONFIRMED {} / {} · RESERVED {}",
                    op.committed_units, op.requested_units, op.reserved_units
                ),
                13. * scale,
                foreground,
            );
            text(
                p,
                row(0.90, 0.98),
                &format!("{} BATCH RECEIPTS · ORE UNITS", op.receipts.len()),
                12. * scale,
                foreground,
            );
        } else if let Some(job) = &data.job {
            text(
                p,
                row(0.70, 0.78),
                &format!(
                    "JOB {} · GOAL {} units",
                    crate::resource_jobs_ui::resource_phase_label(job.phase).to_uppercase(),
                    job.target_units
                ),
                13. * scale,
                foreground,
            );
            text(
                p,
                row(0.80, 0.88),
                &format!(
                    "JOB TOTALS: mined {} / stored {}",
                    job.mined_units, job.stored_units
                ),
                13. * scale,
                foreground,
            );
            text(
                p,
                row(0.90, 0.98),
                &format!(
                    "{} · transfers atomic",
                    job.target_id
                        .map_or_else(|| "No job target".into(), |id| format!("Asteroid #{id}"))
                ),
                12. * scale,
                foreground,
            );
        } else {
            text(
                p,
                row(0.71, 0.81),
                "NO RESOURCE JOB / CURRENT HOLD ONLY",
                14. * scale,
                foreground,
            );
            text(
                p,
                row(0.87, 0.98),
                "GENERIC ORE UNITS / READ ONLY",
                12. * scale,
                foreground,
            );
        }
    });
    #[cfg(test)]
    ui.ctx().data_mut(|d|d.insert_temp(egui::Id::new("cargo_instrument_proof"),serde_json::json!({
        "surface":7,"owner":data.owner,"ship":data.ship,"vessel_id":data.vessel_id,"used":data.used,"capacity":data.capacity,"free":data.free,"fill":data.fill,
        "invalid":data.invalid,"contents":data.contents(),"availability":data.time.availability.label(),"server_ms":data.time.server_ms,"job":data.job,"unload":unload
    })));
}

/// Source-bound volume display; never reads prototype ore or invents mass.
pub(super) fn paint_physical(
    ui: &egui::Ui,
    surface: &CockpitSurface,
    pilot: &PilotView,
    snapshot: &Snapshot,
    time: TelemetryTime,
    view: &arkline_protocol::physical_cargo::control::PhysicalControlPublicView,
) {
    if crate::physical_cargo_ui::validate(view, snapshot, &pilot.name).is_err() {
        return;
    }
    let live = time.availability == Availability::Live;
    let accent = if live { crate::TEAL } else { crate::AMBER };
    surface.paint(ui, |p| {
        let canvas = surface.canvas_rect();
        p.rect_filled(canvas, 0., egui::Color32::from_rgb(2, 14, 10));
        let r = canvas.shrink(canvas.height() * 0.035);
        let row = |a: f32, b: f32| {
            egui::Rect::from_min_max(
                egui::pos2(r.left(), r.top() + a * r.height()),
                egui::pos2(r.right(), r.top() + b * r.height()),
            )
        };
        let scale = canvas.height() / 240.;
        let fg = egui::Color32::from_rgb(202, 228, 215);
        text(
            p,
            row(0., 0.12),
            "CARGO / PHYSICAL VOLUME",
            22. * scale,
            accent,
        );
        text(
            p,
            row(0.14, 0.25),
            &format!("SOURCE 1704 · {}", time.availability.label()),
            14. * scale,
            accent,
        );
        text(
            p,
            row(0.28, 0.44),
            &crate::physical_cargo_ui::litres(view.ship_payload_cm3.0),
            30. * scale,
            fg,
        );
        text(
            p,
            row(0.45, 0.55),
            &format!(
                "CAPACITY {}",
                crate::physical_cargo_ui::litres(view.ship_capacity_cm3.0)
            ),
            17. * scale,
            fg,
        );
        let track = row(0.59, 0.67);
        let fill = view.ship_payload_cm3.0 as f32 / view.ship_capacity_cm3.0 as f32;
        p.rect_filled(track, 2., egui::Color32::from_rgb(20, 43, 38));
        p.rect_filled(
            egui::Rect::from_min_size(track.min, egui::vec2(track.width() * fill, track.height())),
            2.,
            accent.gamma_multiply(0.7),
        );
        text(
            p,
            row(0.73, 0.83),
            &format!(
                "DRONE {}",
                crate::physical_cargo_ui::litres(view.drone_payload_cm3.0)
            ),
            17. * scale,
            fg,
        );
        text(
            p,
            row(0.88, 0.98),
            "READ ONLY · MASS RATING UNKNOWN",
            12. * scale,
            fg,
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (PilotView, Snapshot, TelemetryTime) {
        let app = crate::tests::view_test_cockpit(&egui::Context::default());
        let s = app.snapshot.unwrap();
        let time = TelemetryTime::from_observation(
            Some(s.server_time_ms),
            Some(0),
            s.server_time_ms,
            true,
            false,
        );
        (s.pilots[0].clone(), s, time)
    }
    #[test]
    fn empty_partial_full_and_overcapacity_keep_actual_units_and_clamp_only_graph() {
        let (mut p, s, t) = fixture();
        let cap = arkline_protocol::cargo_capacity(p.ship_class);
        for used in [0, cap / 4, cap, cap + 25, u64::MAX] {
            p.ore = used;
            let d = CargoReadings::new(&p, &s, t);
            assert_eq!(d.used, used);
            assert_eq!(d.capacity, cap);
            assert_eq!(d.free, cap.saturating_sub(used));
            assert_eq!(d.invalid, used > cap);
            assert!((0.0..=1.0).contains(&d.fill));
            if used >= cap {
                assert_eq!(d.fill, 1.);
            }
            if used == 0 {
                assert_eq!(d.fill, 0.);
            }
            assert_eq!(d.contents(), format!("Ore {used} units"));
        }
    }
    #[test]
    fn station_storage_and_job_lifetime_totals_are_not_onboard_contents() {
        let (mut p, mut s, t) = fixture();
        p.ore = 250;
        s.stored_ore = 90_000;
        s.resource_jobs = vec![arkline_protocol::ResourceJob {
            drone_cargo: None,
            controller: "controller".into(),
            id: 1,
            pilot: p.name.clone(),
            target_units: 5000,
            mined_units: 4000,
            stored_units: 3000,
            target_id: Some(7),
            phase: arkline_protocol::ResourceJobPhase::Depositing,
            message: "".into(),
            started_at_ms: 1,
            updated_at_ms: 1000,
        }];
        let d = CargoReadings::new(&p, &s, t);
        assert_eq!(d.used, 250);
        assert_eq!(d.free, 750);
        assert_eq!(d.job.as_ref().unwrap().stored_units, 3000);
        s.resource_jobs[0].pilot = "someone else".into();
        assert!(CargoReadings::new(&p, &s, t).job.is_none());
    }
    #[test]
    fn freshness_keeps_last_confirmed_values_without_invented_transfer_progress() {
        let (p, s, _) = fixture();
        for (connected, paused, age, expected) in [
            (true, false, 0, Availability::Live),
            (true, true, 9000, Availability::Paused),
            (true, false, 9000, Availability::Stale),
            (false, false, 0, Availability::Disconnected),
        ] {
            let t = TelemetryTime::from_observation(Some(1000), Some(age), 1000, connected, paused);
            let d = CargoReadings::new(&p, &s, t);
            assert_eq!(d.used, p.ore);
            assert_eq!(d.time.availability, expected);
        }
    }
    #[test]
    fn stable_vessel_identity_is_not_synthesized_from_owner() {
        let (mut p, s, t) = fixture();
        assert!(CargoReadings::new(&p, &s, t).vessel_id.is_none());
        p.cockpit_mechanisms = Some(arkline_protocol::PioneerCockpitMechanisms::stowed(
            "vessel-confirmed".into(),
            1000,
        ));
        assert_eq!(
            CargoReadings::new(&p, &s, t).vessel_id.as_deref(),
            Some("vessel-confirmed")
        );
    }
    #[test]
    fn foreign_or_missing_vessel_identity_never_displays_private_unload_receipts() {
        use arkline_protocol::*;
        let (p, _, _) = fixture();
        let op = CargoUnloadOperation {
            operation_id: 1,
            pilot: p.name.clone(),
            vessel_id: "confirmed-vessel".into(),
            ship_class: p.ship_class,
            system_id: p.system_id,
            berth_id: 0,
            destination: CargoUnloadDestination::Personal,
            destination_owner: p.name.clone(),
            requested_units: 57,
            committed_units: 25,
            reserved_units: 32,
            phase: CargoUnloadPhase::Held,
            next_batch_at_ms: 1000,
            job_id: None,
            message: "Held".into(),
            receipts: vec![],
        };
        assert!(matching_unload(&p, Some("confirmed-vessel"), Some(&op)).is_some());
        assert!(matching_unload(&p, None, Some(&op)).is_none());
        assert!(matching_unload(&p, Some("another-vessel"), Some(&op)).is_none());
        let mut foreign = p;
        foreign.name = "Remote worker".into();
        assert!(matching_unload(&foreign, Some("confirmed-vessel"), Some(&op)).is_none());
        assert_eq!(op.committed_units, 25);
    }
}
