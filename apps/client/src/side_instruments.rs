//! Permanent, read-only wing instruments. Commands stay on the central computer.
//! Every value comes from the received pilot/snapshot: no simulated gauges or ETA.
use super::*;

#[derive(Clone, Copy)]
pub(super) enum Panel {
    Drones,
    MiningCargo,
}

fn drone_rows(pilot: &PilotView) -> Vec<String> {
    let mut drones = std::collections::BTreeMap::new();
    for drone in fleet(pilot) {
        // Use the same plural/singleton compatibility selection as the real
        // drone controls; an obsolete singleton cannot add an extra drone.
        drones.entry(drone.id).or_insert(drone);
    }
    let mut rows = vec![format!("{} DEPLOYED", drones.len())];
    if drones.is_empty() {
        rows.push("NO DEPLOYED DRONES".into());
    }
    for drone in drones.values().take(5) {
        let class = match drone.class {
            arkline_protocol::DroneClass::Recon => "RECON",
            arkline_protocol::DroneClass::Mining => "MINER",
            arkline_protocol::DroneClass::Attack => "ATTACK",
        };
        let mode = match &drone.mode {
            arkline_protocol::DroneMode::Hold => "HOLD".into(),
            arkline_protocol::DroneMode::Returning => "RETURN".into(),
            arkline_protocol::DroneMode::FlyTo { .. } => "FLY TO".into(),
            arkline_protocol::DroneMode::Orbit { .. } => "ORBIT".into(),
            arkline_protocol::DroneMode::Mining { target_id } => format!("MINE #{target_id}"),
            arkline_protocol::DroneMode::Attacking {
                target_drone_id, ..
            } => {
                format!("ATTACK #{target_drone_id}")
            }
        };
        rows.push(format!("#{} {class} / {mode}", drone.id));
    }
    if drones.len() > 5 {
        rows.push(format!("+{} MORE / CENTER DETAILS", drones.len() - 5));
    }
    rows
}

fn mining_rows(pilot: &PilotView, snapshot: &Snapshot) -> Vec<String> {
    let mut rows = vec![format!(
        "HOLD {} / {} ORE",
        pilot.ore,
        arkline_protocol::cargo_capacity(pilot.ship_class)
    )];
    rows.push(
        pilot
            .power
            .as_ref()
            .map_or("POWER DATA UNAVAILABLE".into(), |power| {
                format!(
                    "CAP {} / {} kJ",
                    power.charge_j / 1000,
                    power.capacity_j / 1000
                )
            }),
    );
    if let Some(job) = snapshot
        .resource_jobs
        .iter()
        .find(|job| job.pilot == pilot.name)
    {
        rows.push(super::super::resource_jobs_ui::resource_phase_label(job.phase).to_uppercase());
        rows.push(format!("MINED {} / {}", job.mined_units, job.target_units));
        rows.push(format!(
            "STORED {} / {}",
            job.stored_units, job.target_units
        ));
        rows.push(
            job.target_id
                .map_or("NO JOB TARGET".into(), |id| format!("TARGET #{id}")),
        );
    } else {
        rows.push("NO MINING ROUTINE".into());
        rows.push("NO JOB TARGET".into());
    }
    rows
}

fn fitted_text(
    painter: &egui::Painter,
    rect: egui::Rect,
    text: &str,
    size: f32,
    color: egui::Color32,
) {
    let mut font_size = size;
    let mut galley = painter.layout_no_wrap(text.into(), egui::FontId::monospace(font_size), color);
    for _ in 0..4 {
        let ink = galley.mesh_bounds;
        let fit = (rect.width() / ink.width().max(0.01))
            .min(rect.height() / ink.height().max(0.01))
            .min(1.0);
        if fit >= 1.0 {
            break;
        }
        font_size *= fit * 0.99;
        galley = painter.layout_no_wrap(text.into(), egui::FontId::monospace(font_size), color);
    }
    let ink = galley.mesh_bounds;
    painter.galley(
        egui::pos2(rect.left(), rect.center().y - ink.height() * 0.5) - ink.min.to_vec2(),
        galley,
        color,
    );
}

pub(super) fn paint(
    ui: &egui::Ui,
    surface: &CockpitSurface,
    panel: Panel,
    pilot: &PilotView,
    snapshot: &Snapshot,
    authority: &str,
    color: egui::Color32,
) {
    let (title, rows) = match panel {
        Panel::Drones => ("DRONES", drone_rows(pilot)),
        Panel::MiningCargo => ("SHIP / MINING", mining_rows(pilot, snapshot)),
    };
    surface.paint(ui, |painter| {
        let canvas = surface.canvas_rect();
        let rect = canvas.shrink(canvas.height() * 0.035);
        let scale = canvas.height() / 180.0;
        let row = |top: f32, bottom: f32| {
            egui::Rect::from_min_max(
                egui::pos2(rect.left(), rect.top() + rect.height() * top),
                egui::pos2(rect.right(), rect.top() + rect.height() * bottom),
            )
        };
        fitted_text(painter, row(0.0, 0.115), title, 18. * scale, color);
        fitted_text(painter, row(0.12, 0.20), authority, 12. * scale, color);
        painter.line_segment(
            [row(0.22, 0.22).left_top(), row(0.22, 0.22).right_top()],
            egui::Stroke::new(1., color.gamma_multiply(0.5)),
        );
        let step = 0.64 / rows.len().max(5) as f32;
        for (index, text) in rows.iter().enumerate() {
            let top = 0.25 + index as f32 * step;
            fitted_text(
                painter,
                row(top, top + step * 0.82),
                text,
                if index == 0 { 18. * scale } else { 15. * scale },
                egui::Color32::from_rgb(202, 228, 215),
            );
        }
        fitted_text(
            painter,
            row(0.93, 1.0),
            "READ ONLY / CENTER CONTROLS",
            10. * scale,
            egui::Color32::from_rgb(114, 147, 137),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (PilotView, Snapshot) {
        let app = crate::tests::view_test_cockpit(&egui::Context::default());
        let snapshot = app.snapshot.unwrap();
        (snapshot.pilots[0].clone(), snapshot)
    }

    #[test]
    fn wing_readouts_use_only_received_owned_state() {
        let (mut pilot, mut snapshot) = fixture();
        assert_eq!(drone_rows(&pilot), ["0 DEPLOYED", "NO DEPLOYED DRONES"]);
        assert!(mining_rows(&pilot, &snapshot).contains(&"NO MINING ROUTINE".into()));
        let drone: arkline_protocol::DroneView = serde_json::from_value(serde_json::json!({
            "id":42,"class":"mining","position":[0,0,0],"velocity":[0,0,0],
            "heading":[1,0,0],"mode":{"kind":"mining","target_id":7},"updated_at_ms":1000
        }))
        .unwrap();
        pilot.drones = vec![drone.clone()];
        pilot.drone = Some(arkline_protocol::DroneView {
            mode: arkline_protocol::DroneMode::Hold,
            ..drone
        });
        assert_eq!(drone_rows(&pilot), ["1 DEPLOYED", "#42 MINER / MINE #7"]);
        pilot.drone.as_mut().unwrap().id = 99;
        assert_eq!(drone_rows(&pilot), ["1 DEPLOYED", "#42 MINER / MINE #7"]);
        pilot.drones.clear();
        assert_eq!(drone_rows(&pilot), ["1 DEPLOYED", "#99 MINER / HOLD"]);
        let job = arkline_protocol::ResourceJob {
            controller: "fixture".into(),
            id: 1,
            pilot: "other pilot".into(),
            target_units: 5000,
            mined_units: 125,
            stored_units: 100,
            target_id: Some(7),
            phase: arkline_protocol::ResourceJobPhase::Mining,
            message: "".into(),
            started_at_ms: 1000,
            updated_at_ms: 1000,
        };
        snapshot.resource_jobs = vec![job];
        assert!(mining_rows(&pilot, &snapshot).contains(&"NO MINING ROUTINE".into()));
        snapshot.resource_jobs[0].pilot = pilot.name.clone();
        assert_eq!(
            mining_rows(&pilot, &snapshot),
            [
                "HOLD 25 / 1000 ORE",
                "POWER DATA UNAVAILABLE",
                "MINING",
                "MINED 125 / 5000",
                "STORED 100 / 5000",
                "TARGET #7"
            ]
        );
        snapshot.resource_jobs[0].phase = arkline_protocol::ResourceJobPhase::Blocked;
        assert_eq!(mining_rows(&pilot, &snapshot)[2], "BLOCKED");
    }
}
