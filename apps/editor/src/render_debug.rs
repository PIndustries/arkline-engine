//! Inspect and switch the real client view without changing the authoritative world.
use arkline_client::{EmbeddedClient, EmbeddedView};
use eframe::egui::{self, Color32};

#[derive(Default)]
pub struct RenderDebug {
    error: Option<String>,
}

impl RenderDebug {
    pub fn show(&mut self, ui: &mut egui::Ui, client: &mut EmbeddedClient, wireframe: &mut bool) {
        let state = client.debug_view_state();
        ui.heading("View and render inspection");
        ui.label(format!(
            "Current view: {}",
            state
                .current
                .map(EmbeddedView::label)
                .unwrap_or("Waiting for local state")
        ));
        ui.label(format!("Scene: {}", state.scene));
        ui.label(format!("Viewed pilot: {}", state.viewed_pilot));
        ui.small("Switch the camera while the world runs or stays paused. Boarding, disembarking and docking still use the simulation's normal controls.");
        ui.horizontal_wrapped(|ui| {
            for view in &state.available {
                if ui
                    .selectable_label(state.current == Some(*view), view.label())
                    .clicked()
                {
                    self.error = client.set_debug_view(*view).err();
                }
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(Color32::LIGHT_RED, error);
        }
        ui.separator();
        ui.checkbox(wireframe, "Mesh wireframe");
        let metrics = client.debug_render_metrics();
        ui.label(format!("Submitted mesh triangles: {}", metrics.triangles));
        ui.label(format!("Mesh draw calls: {}", metrics.draw_calls));
        ui.label(format!(
            "Wireframe draw calls: {}",
            metrics.wireframe_draw_calls
        ));
        ui.small("Counts describe the last GPU mesh scene pass, which may be a camera feed; sky and shadow passes are excluded. Wireframe adds rendering work. A 2D view does not submit this 3D mesh pass.");
        for sample in client.debug_view_render_metrics() {
            ui.separator();
            ui.strong(format!("{} · sample {} ms",sample.key,sample.frame));
            ui.label(format!("Visibility-tested draws: {} / {} submitted",sample.visibility.submitted_draws,sample.visibility.candidate_draws));
            ui.label(format!("Visibility-tested triangles: {} / {} submitted",sample.visibility.submitted_triangles,sample.visibility.candidate_triangles));
            ui.small(format!("CPU selection {:.3} ms · pass submission {:.3} ms",sample.visibility.cpu_prepare_ns as f64/1e6,sample.cpu_submit_ns as f64/1e6));
            ui.small(match (sample.gpu_elapsed_ns,sample.gpu_frame) {
                (Some(ns),Some(frame))=>format!("GPU pass {:.3} ms · completed sample {frame} ms",ns as f64/1e6),
                _=>"GPU pass timing unavailable or pending".into(),
            });
        }
        ui.small("Each camera selects independently. Visibility counts cover tested mesh groups; sky, effects and required shadow/depth passes may still run. GPU timing covers the enclosing pass and is read asynchronously. Cached feeds retain their prior sample timestamp.");
        ui.separator();
        ui.strong("Observed view transitions");
        ui.small("Most recent first. Simulation time stays fixed while paused.");
        if state.transitions.is_empty() {
            ui.label("No view transition observed in this session.");
        }
        for transition in state.transitions.iter().rev() {
            ui.group(|ui| {
                ui.label(format!(
                    "{} → {}",
                    transition
                        .from
                        .map(EmbeddedView::label)
                        .unwrap_or("No view"),
                    transition.to.map(EmbeddedView::label).unwrap_or("No view")
                ));
                ui.small(&transition.reason);
                ui.monospace(format!(
                    "#{} · observed at {} ms",
                    transition.sequence, transition.observed_at_ms
                ));
                ui.monospace(match transition.simulation_time_ms {
                    Some(time) => format!("Simulation: {time} ms"),
                    None => "Simulation: unavailable".into(),
                });
            });
        }
        ui.separator();
        ui.small("These are client-observed state changes, not GPU completion timings. Collision/interest overlays and asset streaming timelines are still being built.");
    }
}
