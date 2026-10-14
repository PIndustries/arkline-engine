use crate::asset_document::{self, MaterialDocument, MaterialHistory};
use crate::material_graph::{MaterialGraph, Node, NodeId, NodeKind, ValueType};
use arkline_client::EmbeddedMaterialPreview;
use eframe::egui;
use std::path::Path;

pub struct Materials {
    history: MaterialHistory,
    saved: MaterialDocument,
    enabled: bool,
    message: String,
    selected: Option<NodeId>,
    connecting: Option<NodeId>,
    pan: egui::Vec2,
    zoom: f32,
    drag_document: Option<MaterialDocument>,
    frame_requested: bool,
}
impl Materials {
    pub fn open(project: &Path) -> Self {
        let exists = project.join("Assets/Pioneer.material.json").exists();
        let loaded = exists.then(|| asset_document::load(project));
        let (document, enabled, message) = match loaded {
            Some(Ok(document)) => (document, true, "Loaded project materials".into()),
            Some(Err(error)) => (MaterialDocument::default(), false, error),
            None => (MaterialDocument::default(), false, String::new()),
        };
        Self {
            history: MaterialHistory::new(document.clone()).expect("validated material"),
            saved: document,
            enabled,
            message,
            selected: Some(6),
            connecting: None,
            pan: egui::vec2(20., 20.),
            zoom: 0.85,
            drag_document: None,
            frame_requested: true,
        }
    }
    pub fn preview(&self) -> Option<EmbeddedMaterialPreview> {
        if !self.enabled {
            return None;
        }
        let values = self.history.current().evaluated().ok()?;
        Some(EmbeddedMaterialPreview {
            hull_rgb: values.hull,
            trim_rgb: values.trim,
            emissive_strength: values.emission,
            roughness: values.roughness,
            metallic: values.metallic,
        })
    }
    fn draft_graph(&self) -> MaterialDocument {
        let mut draft = self
            .drag_document
            .as_ref()
            .unwrap_or(self.history.current())
            .clone();
        if draft.graph.is_none() {
            draft.graph = Some(MaterialGraph::from_values(&draft.values()));
        }
        draft
    }
    fn apply(&mut self, draft: MaterialDocument) {
        match self.history.apply(draft) {
            Ok(true) => {
                self.enabled = true;
                self.message = "Material updated · save to keep changes".into();
            }
            Ok(false) => {}
            Err(error) => self.message = error,
        }
    }
    fn save(&mut self, project: &Path) {
        // Persist the visible starter graph too, so the first Save is a real graph document.
        let draft = self.draft_graph();
        self.apply(draft);
        match asset_document::save(project, self.history.current()) {
            Ok(()) => {
                self.saved = self.history.current().clone();
                self.message = "Saved Assets/Pioneer.material.json".into();
            }
            Err(error) => self.message = error,
        }
    }
    fn reload(&mut self, project: &Path) {
        match asset_document::load(project) {
            Ok(document) => {
                self.saved = document.clone();
                self.apply(document);
                self.enabled = true;
                self.drag_document = None;
                self.connecting = None;
                self.message = "Reloaded saved material · Undo restores the previous edit".into();
            }
            Err(error) => self.message = error,
        }
    }
    fn move_history(&mut self, redo: bool) {
        let changed = if redo {
            self.history.redo()
        } else {
            self.history.undo()
        };
        if changed {
            // An imported graph may reuse IDs for different kinds. A pending source
            // belongs to its original revision and must not survive a history move.
            self.connecting = None;
            self.drag_document = None;
            self.enabled = true;
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui, project: &Path) {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.enabled, "Apply to fleet");
            if ui
                .add_enabled(self.history.can_undo(), egui::Button::new("Undo"))
                .clicked()
            {
                self.move_history(false);
            }
            if ui
                .add_enabled(self.history.can_redo(), egui::Button::new("Redo"))
                .clicked()
            {
                self.move_history(true);
            }
            if ui.button("Save").clicked() {
                self.save(project);
            }
            if ui.button("Reload").clicked() {
                self.reload(project);
            }
            ui.label(if self.history.current() == &self.saved {
                "Saved"
            } else {
                "Unsaved"
            });
            if ui.button("Frame graph").clicked() {
                self.frame_requested = true;
            }
        });
    }
    /// Compatibility panel for hosts that have not installed the split graph workspace.
    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path) {
        ui.heading("Fleet material");
        self.show_library(ui);
        self.show_graph(ui, project);
        self.show_node_inspector(ui, project);
    }
    pub fn show_library(&mut self, ui: &mut egui::Ui) {
        ui.heading("Node library");
        ui.small("Add a node, then connect matching ports.");
        for (label, kind) in [
            (
                "+ Color",
                NodeKind::Color {
                    value: [0.5, 0.5, 0.5],
                },
            ),
            ("+ Scalar", NodeKind::Scalar { value: 0.5 }),
            (
                "+ Mix Color",
                NodeKind::MixColor {
                    a: None,
                    b: None,
                    factor: None,
                    amount: 0.5,
                },
            ),
            (
                "+ Multiply Scalar",
                NodeKind::MultiplyScalar {
                    a: None,
                    b: None,
                    value_a: 1.,
                    value_b: 1.,
                },
            ),
        ] {
            if ui.button(label).clicked() {
                let mut draft = self.draft_graph();
                let graph = draft.graph.as_mut().unwrap();
                let id = match graph.next_ids(1) {
                    Ok(ids) => ids[0],
                    Err(error) => {
                        self.message = error;
                        continue;
                    }
                };
                // New nodes enter the visible canvas, independent of its pan and zoom.
                let offset = (graph.nodes.len() % 5) as f32 * 24.;
                graph.nodes.push(Node {
                    id,
                    position: [
                        (70. + offset - self.pan.x) / self.zoom,
                        (70. + offset - self.pan.y) / self.zoom,
                    ],
                    kind,
                });
                self.selected = Some(id);
                self.apply(draft);
            }
        }
        ui.separator();
        ui.label("Finish presets");
        for (name, roughness, metallic) in [
            ("Industrial satin", 0.38, 0.75),
            ("Weathered coating", 0.72, 0.15),
        ] {
            if ui.button(name).clicked() {
                self.apply_finish_preset(roughness, metallic);
            }
        }
        if ui.button("Reset material").clicked() {
            self.apply(MaterialDocument::default());
            self.selected = Some(6);
            self.connecting = None;
        }
        ui.separator();
        ui.small("Color → blue ports\nScalar → gold ports");
        ui.small("Shared fleet hull and trim. PBR finish and authored glow strength.");
    }
    fn apply_finish_preset(&mut self, roughness: f32, metallic: f32) {
        let mut draft = self.draft_graph();
        let graph = draft.graph.as_mut().unwrap();
        let ids = match graph.next_ids(2) {
            Ok(ids) => ids,
            Err(error) => {
                self.message = error;
                return;
            }
        };
        graph.nodes.push(Node {
            id: ids[0],
            position: [260., 400.],
            kind: NodeKind::Scalar { value: roughness },
        });
        graph.nodes.push(Node {
            id: ids[1],
            position: [260., 520.],
            kind: NodeKind::Scalar { value: metallic },
        });
        let output = graph
            .nodes
            .iter_mut()
            .find(|n| matches!(n.kind, NodeKind::Output { .. }))
            .unwrap();
        let _ = output.kind.set_input(3, Some(ids[0]));
        let _ = output.kind.set_input(4, Some(ids[1]));
        self.apply(draft);
    }
    pub fn show_graph(&mut self, ui: &mut egui::Ui, project: &Path) {
        self.toolbar(ui, project);
        ui.small("Click output, then input to connect · right-click input to disconnect · drag headers to move · drag empty space to pan · scroll to zoom");
        let desired = egui::vec2(
            ui.available_width().max(240.),
            ui.available_height().max(240.),
        );
        let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click_and_drag());
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0., egui::Color32::from_rgb(19, 23, 30));
        let mut draft = self.draft_graph();
        let graph = draft.graph.as_mut().unwrap();
        if self.frame_requested {
            let mut bounds = egui::Rect::NOTHING;
            for node in &graph.nodes {
                let height = if node.kind.inputs().is_empty() {
                    88.
                } else {
                    64. + 26. * node.kind.inputs().len() as f32
                };
                bounds = bounds.union(egui::Rect::from_min_size(
                    egui::pos2(node.position[0], node.position[1]),
                    egui::vec2(180., height),
                ));
            }
            self.zoom = ((rect.width() - 60.) / bounds.width())
                .min((rect.height() - 60.) / bounds.height())
                .clamp(0.35, 1.2);
            self.pan = rect.size() * 0.5 - bounds.center().to_vec2() * self.zoom;
            self.frame_requested = false;
        }
        if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p))) {
            let redo = ui.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                )
            });
            let undo =
                !redo && ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z));
            if redo || undo {
                self.move_history(redo);
                // Refresh after the history action; drawing and editing must use the restored graph.
                draft = self.draft_graph();
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                let old = self.zoom;
                let new = (old * (scroll * 0.002).exp()).clamp(0.35, 1.6);
                let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                let local = cursor - rect.min;
                self.pan = local - (local - self.pan) * (new / old);
                self.zoom = new;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.connecting = None;
            }
        }
        let graph = draft.graph.as_mut().unwrap();
        if self
            .connecting
            .is_some_and(|id| graph.node(id).and_then(|n| n.kind.output_type()).is_none())
        {
            self.connecting = None;
        }
        let screen = |p: [f32; 2], pan: egui::Vec2, zoom: f32| {
            rect.min + pan + egui::vec2(p[0], p[1]) * zoom
        };
        let spacing = 32. * self.zoom;
        let x0 = rect.left() + self.pan.x.rem_euclid(spacing);
        let y0 = rect.top() + self.pan.y.rem_euclid(spacing);
        let mut x = x0;
        while x < rect.right() {
            let mut y = y0;
            while y < rect.bottom() {
                painter.circle_filled(egui::pos2(x, y), 1., egui::Color32::from_gray(47));
                y += spacing;
            }
            x += spacing;
        }
        for node in &graph.nodes {
            for (index, (_, ty, source)) in node.kind.inputs().iter().enumerate() {
                if let Some(source) = source {
                    if let Some(from) = graph.node(*source) {
                        let a = screen(from.position, self.pan, self.zoom)
                            + egui::vec2(180., 48.) * self.zoom;
                        let b = screen(node.position, self.pan, self.zoom)
                            + egui::vec2(0., 48. + index as f32 * 26.) * self.zoom;
                        draw_wire(&painter, a, b, port_color(*ty));
                    }
                }
            }
        }
        if let Some(source) = self.connecting {
            if let Some(node) = graph.node(source) {
                if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                    let a = screen(node.position, self.pan, self.zoom)
                        + egui::vec2(180., 48.) * self.zoom;
                    draw_wire(
                        &painter,
                        a,
                        pointer,
                        port_color(node.kind.output_type().unwrap()),
                    );
                }
            }
        }
        let mut changed = false;
        let mut dragging = false;
        let mut drag_finished = false;
        let mut occupied = false;
        let mut connect_action = None;
        let mut disconnect_action = None;
        for node in &mut graph.nodes {
            let origin = screen(node.position, self.pan, self.zoom);
            let inputs = node.kind.inputs();
            let height = if inputs.is_empty() {
                88.
            } else {
                64. + 26. * inputs.len() as f32
            };
            let body = egui::Rect::from_min_size(origin, egui::vec2(180., height) * self.zoom);
            occupied |= ui.input(|i| {
                i.pointer
                    .hover_pos()
                    .is_some_and(|p| body.expand(12.).contains(p))
            });
            painter.rect_filled(body, 6., egui::Color32::from_rgb(37, 43, 54));
            painter.rect_stroke(
                body,
                6.,
                egui::Stroke::new(
                    if self.selected == Some(node.id) {
                        2.
                    } else {
                        1.
                    },
                    if self.selected == Some(node.id) {
                        egui::Color32::from_rgb(89, 179, 223)
                    } else {
                        egui::Color32::from_gray(75)
                    },
                ),
                egui::StrokeKind::Inside,
            );
            let header = egui::Rect::from_min_size(origin, egui::vec2(180., 30.) * self.zoom);
            painter.rect_filled(header, 6., egui::Color32::from_rgb(51, 61, 78));
            painter.text(
                header.center(),
                egui::Align2::CENTER_CENTER,
                node.kind.name(),
                egui::FontId::proportional(13. * self.zoom),
                egui::Color32::WHITE,
            );
            let hit = ui.interact(
                header,
                ui.id().with(("material_node", node.id)),
                egui::Sense::click_and_drag(),
            );
            if hit.clicked() || hit.drag_started() {
                self.selected = Some(node.id);
            }
            if hit.dragged() {
                let delta = ui.input(|i| i.pointer.delta()) / self.zoom;
                node.position[0] = (node.position[0] + delta.x).clamp(-100_000., 100_000.);
                node.position[1] = (node.position[1] + delta.y).clamp(-100_000., 100_000.);
                dragging = true;
                changed = true;
            }
            drag_finished |= hit.drag_stopped();
            if let Some(ty) = node.kind.output_type() {
                let center = origin + egui::vec2(180., 48.) * self.zoom;
                painter.circle_filled(center, 5. * self.zoom, port_color(ty));
                let hit = ui.interact(
                    egui::Rect::from_center_size(center, egui::vec2(22., 22.)),
                    ui.id().with(("material_out", node.id)),
                    egui::Sense::click(),
                );
                if hit.clicked() {
                    self.connecting = Some(node.id);
                    self.selected = Some(node.id);
                }
                match node.kind {
                    NodeKind::Color { value } => {
                        let color = egui::Color32::from_rgb(
                            (value[0] * 255.) as u8,
                            (value[1] * 255.) as u8,
                            (value[2] * 255.) as u8,
                        );
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                origin + egui::vec2(14., 43.) * self.zoom,
                                egui::vec2(138., 24.) * self.zoom,
                            ),
                            3.,
                            color,
                        );
                    }
                    NodeKind::Scalar { value } => {
                        painter.text(
                            origin + egui::vec2(14., 50.) * self.zoom,
                            egui::Align2::LEFT_CENTER,
                            format!("{value:.3}"),
                            egui::FontId::proportional(12. * self.zoom),
                            egui::Color32::LIGHT_GRAY,
                        );
                    }
                    _ => {}
                }
            }
            for (index, (name, ty, source)) in inputs.into_iter().enumerate() {
                let center = origin + egui::vec2(0., 48. + index as f32 * 26.) * self.zoom;
                painter.circle_filled(center, 5. * self.zoom, port_color(ty));
                painter.text(
                    center + egui::vec2(13. * self.zoom, 0.),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(12. * self.zoom),
                    if source.is_some() {
                        egui::Color32::WHITE
                    } else {
                        egui::Color32::GRAY
                    },
                );
                let hit = ui.interact(
                    egui::Rect::from_center_size(center, egui::vec2(22., 22.)),
                    ui.id().with(("material_in", node.id, index)),
                    egui::Sense::click(),
                );
                if hit.clicked() {
                    self.selected = Some(node.id);
                    if let Some(source) = self.connecting {
                        connect_action = Some((source, node.id, index));
                    }
                }
                if hit.secondary_clicked() {
                    disconnect_action = Some((node.id, index));
                }
            }
        }
        if response.dragged() && !occupied && !dragging {
            self.pan += ui.input(|i| i.pointer.delta());
        }
        if response.clicked() && !occupied {
            self.connecting = None;
            self.selected = None;
        }
        if let Some((source, target, index)) = connect_action {
            match graph.connect(source, target, index) {
                Ok(()) => {
                    changed = true;
                    self.connecting = None;
                }
                Err(error) => self.message = error,
            }
        }
        if let Some((target, index)) = disconnect_action {
            if let Some(n) = graph.nodes.iter_mut().find(|n| n.id == target) {
                let _ = n.kind.set_input(index, None);
                changed = true;
            }
        }
        if dragging {
            self.drag_document = Some(draft);
        } else if drag_finished {
            self.drag_document = None;
            self.apply(draft);
        } else if changed {
            self.apply(draft);
        }
        if let Some(source) = self.connecting {
            painter.text(
                rect.left_bottom() + egui::vec2(12., -12.),
                egui::Align2::LEFT_BOTTOM,
                format!("Connecting node {source} · choose a matching input (Esc cancels)"),
                egui::FontId::proportional(12.),
                egui::Color32::LIGHT_BLUE,
            );
        }
    }
    pub fn show_node_inspector(&mut self, ui: &mut egui::Ui, _project: &Path) {
        ui.heading("Material properties");
        let mut draft = self.draft_graph();
        let graph = draft.graph.as_mut().unwrap();
        let mut changed = false;
        if let Some(node) = self
            .selected
            .and_then(|id| graph.nodes.iter_mut().find(|n| n.id == id))
        {
            ui.label(format!("{} · node {}", node.kind.name(), node.id));
            ui.separator();
            match &mut node.kind {
                NodeKind::Color { value } => {
                    ui.label("Linear RGB");
                    let mut edited = *value;
                    if ui.color_edit_button_rgb(&mut edited).changed() {
                        *value = edited;
                        changed = true;
                    }
                }
                NodeKind::Scalar { value } => {
                    changed |= ui
                        .add(egui::Slider::new(value, 0.0..=4.0).text("Value"))
                        .changed();
                    ui.small("Use 0–1 for roughness and metallic; 0–4 for emission.");
                }
                NodeKind::MixColor {
                    a,
                    b,
                    factor,
                    amount,
                } => {
                    ui.small("Blend A and B. Unconnected colors use black and white.");
                    changed |= ui
                        .add_enabled(
                            factor.is_none(),
                            egui::Slider::new(amount, 0.0..=1.0).text("Factor"),
                        )
                        .changed();
                    ui.small(format!(
                        "A: {} · B: {}",
                        connection_text(*a),
                        connection_text(*b)
                    ));
                }
                NodeKind::MultiplyScalar {
                    a,
                    b,
                    value_a,
                    value_b,
                } => {
                    changed |= ui
                        .add_enabled(
                            a.is_none(),
                            egui::Slider::new(value_a, 0.0..=4.0).text("A fallback"),
                        )
                        .changed();
                    changed |= ui
                        .add_enabled(
                            b.is_none(),
                            egui::Slider::new(value_b, 0.0..=4.0).text("B fallback"),
                        )
                        .changed();
                }
                NodeKind::Output { .. } => {
                    ui.small("This output drives tagged fleet hull/trim surfaces, finish and authored glow strength.");
                    ui.small("Disconnect roughness or metallic to retain each surface's authored finish.");
                }
            }
            ui.separator();
            for (index, (name, _, source)) in node.kind.inputs().into_iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{name}: {}", connection_text(source)));
                    if source.is_some() && ui.small_button("Disconnect").clicked() {
                        let _ = node.kind.set_input(index, None);
                        changed = true;
                    }
                });
            }
            if !matches!(node.kind, NodeKind::Output { .. }) && ui.button("Delete node").clicked() {
                let id = node.id;
                graph.remove(id);
                self.selected = None;
                self.connecting = None;
                changed = true;
            }
        } else {
            ui.small("Select a node to edit its values.");
        }
        if changed {
            self.apply(draft);
        }
        ui.separator();
        if let Ok(v) = self.history.current().evaluated() {
            ui.label("Live output");
            ui.small(format!("Hull  {:.2}  {:.2}  {:.2}\nTrim  {:.2}  {:.2}  {:.2}\nEmission {:.2}\nRoughness {}\nMetallic {}",v.hull[0],v.hull[1],v.hull[2],v.trim[0],v.trim[1],v.trim[2],v.emission,factor_text(v.roughness),factor_text(v.metallic)));
        }
        ui.separator();
        ui.small(&self.message);
    }
}
fn connection_text(id: Option<NodeId>) -> String {
    id.map_or_else(|| "unconnected".into(), |id| format!("node {id}"))
}
fn factor_text(value: Option<f32>) -> String {
    value.map_or_else(|| "authored".into(), |v| format!("{v:.2}"))
}
fn port_color(ty: ValueType) -> egui::Color32 {
    match ty {
        ValueType::Color => egui::Color32::from_rgb(87, 174, 241),
        ValueType::Scalar => egui::Color32::from_rgb(237, 187, 83),
    }
}
fn draw_wire(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, color: egui::Color32) {
    let bend = ((b.x - a.x).abs() * 0.5).max(45.);
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        [a, a + egui::vec2(bend, 0.), b - egui::vec2(bend, 0.), b],
        false,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(2., color),
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn history_moves_clear_pending_connections_across_imported_node_kinds() {
        let mut materials = Materials::open(Path::new("unused-imported-kind-project"));
        let mut imported = materials.draft_graph();
        let graph = imported.graph.as_mut().unwrap();
        // Exchange the starter Color and Output IDs, including every reference.
        let remap = |id| {
            if id == 1 {
                6
            } else if id == 6 {
                1
            } else {
                id
            }
        };
        for node in &mut graph.nodes {
            node.id = remap(node.id);
            for (index, (_, _, source)) in node.kind.inputs().into_iter().enumerate() {
                node.kind.set_input(index, source.map(remap)).unwrap();
            }
        }
        materials.apply(imported.clone());
        materials.apply(MaterialDocument::default());
        materials.connecting = Some(1); // Color in starter graph, Output in imported graph.
        materials.move_history(false);
        assert_eq!(materials.history.current(), &imported);
        assert_eq!(materials.connecting, None);
        materials.connecting = Some(6); // Color in imported graph, Output in starter graph.
        materials.move_history(true);
        assert_eq!(materials.history.current(), &MaterialDocument::default());
        assert_eq!(materials.connecting, None);
    }
    #[test]
    fn imported_extreme_ids_reject_presets_without_changing_document_or_redo() {
        for id in [u32::MAX, u32::MAX - 1] {
            let mut materials = Materials::open(Path::new("unused-id-overflow-project"));
            let mut draft = materials.draft_graph();
            draft.graph.as_mut().unwrap().nodes.push(Node {
                id,
                position: [10., 10.],
                kind: NodeKind::Scalar { value: 0.5 },
            });
            materials.apply(draft.clone());
            let mut newer = draft.clone();
            newer.graph.as_mut().unwrap().nodes[0].position[0] += 1.;
            materials.apply(newer.clone());
            assert!(materials.history.undo());
            assert_eq!(materials.history.current(), &draft);
            assert!(materials.history.can_redo());
            materials.apply_finish_preset(0.38, 0.75);
            assert_eq!(materials.history.current(), &draft);
            assert!(materials.history.can_redo());
            assert!(materials.message.contains("node IDs"));
            assert!(materials.history.redo());
            assert_eq!(materials.history.current(), &newer);
        }
    }
    #[test]
    fn idle_material_panel_preserves_document_history_and_disabled_preview() {
        let mut materials = Materials::open(Path::new("unused-idle-test-project"));
        let original = materials.history.current().clone();
        let context = egui::Context::default();
        for frame in 0..3 {
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 900.),
                    )),
                    time: Some(frame as f64 / 60.),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        materials.show_graph(ui, Path::new("unused-idle-test-project"));
                    });
                },
            );
            assert_eq!(materials.history.current(), &original);
            assert!(!materials.history.can_undo());
            assert!(materials.preview().is_none());
        }
    }
    #[test]
    fn graph_edits_drive_preview_and_undo_restores_renderer_values() {
        let mut materials = Materials::open(Path::new("unused-preview-test-project"));
        let mut draft = materials.draft_graph();
        if let NodeKind::Color { value } = &mut draft.graph.as_mut().unwrap().nodes[0].kind {
            *value = [0.9, 0.1, 0.2];
        }
        materials.apply(draft);
        assert_eq!(materials.preview().unwrap().hull_rgb, [0.9, 0.1, 0.2]);
        materials.history.undo();
        assert_eq!(
            materials.preview().unwrap().hull_rgb,
            MaterialDocument::default().hull_color
        );
    }
    #[test]
    fn port_clicks_connect_typed_nodes_and_update_output() {
        let mut materials = Materials::open(Path::new("unused-port-test-project"));
        materials.pan = egui::Vec2::ZERO;
        materials.zoom = 1.;
        materials.frame_requested = false;
        let mut draft = materials.draft_graph();
        let g = draft.graph.as_mut().unwrap();
        g.nodes.iter_mut().find(|n| n.id == 1).unwrap().position = [40., 80.];
        g.nodes.iter_mut().find(|n| n.id == 1).unwrap().kind = NodeKind::Color {
            value: [0.8, 0.2, 0.1],
        };
        g.nodes.iter_mut().find(|n| n.id == 6).unwrap().position = [380., 80.];
        // Remove the initial hull link, so the actual port gesture must recreate it.
        g.nodes
            .iter_mut()
            .find(|n| n.id == 6)
            .unwrap()
            .kind
            .set_input(0, None)
            .unwrap();
        materials.apply(draft);
        let ctx = egui::Context::default();
        let mut origin = egui::Pos2::ZERO;
        let run = |materials: &mut Materials, events: Vec<egui::Event>, origin: &mut egui::Pos2| {
            let _=ctx.run(egui::RawInput{screen_rect:Some(egui::Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(900.,800.))),events,..Default::default()},|ctx|{egui::CentralPanel::default().show(ctx,|ui|{materials.toolbar(ui,Path::new("unused-port-test-project"));ui.small("Click output, then input to connect · right-click input to disconnect · drag headers to move · drag empty space to pan · scroll to zoom");*origin=ui.cursor().min;});});
        };
        // The graph canvas origin is derived from its actual layout, never assumed screen chrome.
        run(&mut materials, vec![], &mut origin);
        let output = origin + egui::vec2(220., 128.);
        let input = origin + egui::vec2(380., 128.);
        let render = |materials: &mut Materials, events| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        materials.show_graph(ui, Path::new("unused-port-test-project"));
                    });
                },
            );
        };
        render(&mut materials, vec![]);
        for p in [output, input] {
            for pressed in [true, false] {
                render(
                    &mut materials,
                    vec![
                        egui::Event::PointerMoved(p),
                        egui::Event::PointerButton {
                            pos: p,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                );
            }
        }
        assert_eq!(
            materials
                .history
                .current()
                .graph
                .as_ref()
                .unwrap()
                .node(6)
                .unwrap()
                .kind
                .inputs()[0]
                .2,
            Some(1)
        );
        assert_eq!(materials.preview().unwrap().hull_rgb, [0.8, 0.2, 0.1]);
        let project = std::env::temp_dir().join(format!(
            "arkline-port-roundtrip-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&project).unwrap();
        materials.save(&project);
        let reopened = Materials::open(&project);
        assert_eq!(reopened.history.current(), materials.history.current());
        assert_eq!(reopened.preview().unwrap().hull_rgb, [0.8, 0.2, 0.1]);
        std::fs::remove_dir_all(project).unwrap();
    }
}
