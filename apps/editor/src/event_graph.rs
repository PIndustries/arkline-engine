//! Local event authoring. Saved drafts are distinct from the acknowledged runtime graph.
use arkline_client::EmbeddedClient;
use arkline_protocol::{
    EditorEventGraph, EditorGraphEdge, EditorGraphNode, EditorGraphPort, EditorNodeOp,
};
use eframe::egui::{self, Color32};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};
const LIMIT: u64 = 256 * 1024;
static TEMP: AtomicU64 = AtomicU64::new(0);

pub struct EventGraphPanel {
    draft: EditorEventGraph,
    selected: Option<u64>,
    from: u64,
    to: u64,
    port: EditorGraphPort,
    revision: Option<u64>,
    status: String,
    save: Option<mpsc::Receiver<Result<(), String>>>,
    pending: Option<u64>,
    applying: Option<u64>,
    pan: egui::Vec2,
    saved: Option<EditorEventGraph>,
    saving: Option<EditorEventGraph>,
    undo: Vec<EditorEventGraph>,
    redo: Vec<EditorEventGraph>,
    gesture: Option<EditorEventGraph>,
    suppress_history: bool,
    next_id: u64,
}
impl EventGraphPanel {
    pub fn open(project: &Path) -> Self {
        let loaded = load(project);
        let saved = loaded.as_ref().ok().cloned();
        let (draft, status) = match loaded {
            Ok(graph) => (
                graph,
                "Loaded saved draft; use Apply to install it in this local world.".into(),
            ),
            Err(error) => (example(), format!("Example draft (not applied). {error}")),
        };
        let next_id = draft
            .nodes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        Self {
            saved,
            saving: None,
            undo: vec![],
            redo: vec![],
            gesture: None,
            suppress_history: false,
            next_id,
            draft,
            selected: None,
            from: 1,
            to: 2,
            port: EditorGraphPort::Next,
            revision: None,
            status,
            save: None,
            pending: None,
            applying: None,
            pan: egui::Vec2::ZERO,
        }
    }
    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path, client: &mut EmbeddedClient) {
        let before = self.draft.clone();
        self.suppress_history = false;
        egui::ScrollArea::vertical()
            .id_salt("event_panel_scroll")
            .max_height(ui.available_height())
            .show(ui, |ui| self.show_contents(ui, project, client));
        self.record_edit(before, ui.input(|i| i.pointer.primary_down()));
    }
    fn show_contents(&mut self, ui: &mut egui::Ui, project: &Path, client: &mut EmbeddedClient) {
        if let Some(rx) = &self.save {
            if let Ok(result) = rx.try_recv() {
                if result.is_ok() {
                    self.saved = self.saving.take();
                } else {
                    self.saving = None;
                }
                self.status = result
                    .map(|_| "Draft saved; runtime is unchanged.".into())
                    .unwrap_or_else(|e| e);
                self.save = None;
            }
        }
        let state = client.graph_debug().cloned();
        if let Some(state) = &state {
            if self.revision.is_none() {
                self.revision = Some(state.debug.revision);
            }
            if let Some(ack) = &state.ack {
                if self.pending == Some(ack.id) {
                    self.status = match &ack.result {
                        Ok(()) => format!(
                            "Runtime acknowledged command {} · revision {}",
                            ack.id, state.debug.revision
                        ),
                        Err(e) => format!("Runtime rejected command {}: {e}", ack.id),
                    };
                    if ack.result.is_ok() && self.applying == Some(ack.id) {
                        self.revision = Some(state.debug.revision);
                    }
                    self.pending = None;
                    self.applying = None;
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.strong("Event flow");
            ui.label(if self.saved.as_ref() == Some(&self.draft) {
                "Saved"
            } else {
                "Unsaved draft"
            });
            ui.label(
                if state.as_ref().is_some_and(|s| s.debug.graph == self.draft) {
                    "Matches runtime"
                } else {
                    "Unapplied changes"
                },
            );
            if ui
                .add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
            {
                if let Some(previous) = self.undo.pop() {
                    self.redo.push(std::mem::replace(&mut self.draft, previous));
                    self.suppress_history = true;
                    self.gesture = None;
                }
            }
            if ui
                .add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                if let Some(next) = self.redo.pop() {
                    push_history(&mut self.undo, std::mem::replace(&mut self.draft, next));
                    self.suppress_history = true;
                    self.gesture = None;
                }
            }
            if ui.button("Reload saved draft").clicked() {
                match load(project) {
                    Ok(graph) => {
                        self.next_id = self.next_id.max(
                            graph
                                .nodes
                                .iter()
                                .map(|n| n.id)
                                .max()
                                .unwrap_or(0)
                                .saturating_add(1),
                        );
                        self.saved = Some(graph.clone());
                        self.draft = graph;
                        self.status = "Saved draft reloaded; runtime unchanged.".into();
                    }
                    Err(e) => self.status = e,
                }
            }

            if ui.button("Validate draft").clicked() {
                self.status = self
                    .draft
                    .validate()
                    .map(|_| "Draft is valid; not yet applied.".into())
                    .unwrap_or_else(|e| e);
            }
            if ui
                .add_enabled(self.save.is_none(), egui::Button::new("Save draft"))
                .clicked()
            {
                let (tx, rx) = mpsc::channel();
                let path = project.to_owned();
                let graph = self.draft.clone();
                self.saving = Some(graph.clone());
                self.save = Some(rx);
                self.status = "Saving draft…".into();
                std::thread::spawn(move || {
                    let _ = tx.send(save(&path, &graph));
                });
            }
            let valid = self.draft.validate().is_ok();
            if ui
                .add_enabled(
                    valid
                        && self.revision.is_some()
                        && self.pending.is_none()
                        && state.as_ref().is_some_and(|s| s.debug.paused),
                    egui::Button::new("Apply to runtime"),
                )
                .clicked()
            {
                self.queued(client.graph_replace(
                    self.revision.unwrap(),
                    self.draft.clone(),
                    false,
                ));
                self.applying = self.pending;
            }
            if ui
                .add_enabled(
                    state.is_some() && self.pending.is_none(),
                    egui::Button::new("Load runtime draft"),
                )
                .clicked()
            {
                let state = state.as_ref().unwrap();
                self.draft = state.debug.graph.clone();
                self.next_id = self.next_id.max(
                    self.draft
                        .nodes
                        .iter()
                        .map(|n| n.id)
                        .max()
                        .unwrap_or(0)
                        .saturating_add(1),
                );
                self.revision = Some(state.debug.revision);
                self.selected = None;
                self.status = "Runtime graph copied into draft. Saved file is unchanged.".into();
            }
            ui.label(format!(
                "Draft base revision {}",
                self.revision.map_or("unknown".into(), |r| r.to_string())
            ));
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(client.ready() && client.paused(), egui::Button::new("Play"))
                .clicked()
            {
                client.play();
            }
            if ui
                .add_enabled(
                    client.ready() && !client.paused(),
                    egui::Button::new("Pause"),
                )
                .clicked()
            {
                client.pause();
            }
            if ui
                .add_enabled(
                    client.ready() && client.paused(),
                    egui::Button::new("Step 50 ms"),
                )
                .clicked()
            {
                client.step();
            }
            if ui
                .add_enabled(
                    state.as_ref().is_some_and(|s| s.debug.paused) && self.pending.is_none(),
                    egui::Button::new("Step one node"),
                )
                .clicked()
            {
                self.queued(client.graph_step());
            }
            if ui
                .add_enabled(
                    state.is_some() && self.pending.is_none(),
                    egui::Button::new("Clear pending events"),
                )
                .clicked()
            {
                self.queued(client.graph_clear());
            }
            ui.small("Node step does not advance time. Delays need simulation time.");
        });
        ui.label(&self.status);
        ui.separator();
        ui.horizontal(|ui| {
            ui.menu_button("Add node", |ui| {
                for (label, op) in catalog() {
                    if ui.button(label).clicked() {
                        let id = self.next_id;
                        self.next_id = self.next_id.saturating_add(1);
                        self.draft.nodes.push(EditorGraphNode {
                            id,
                            label: label.into(),
                            position: [
                                30.0 + (id % 4) as f32 * 165.0,
                                40.0 + (id / 4) as f32 * 80.0,
                            ],
                            operation: op,
                            breakpoint: false,
                        });
                        self.selected = Some(id);
                        ui.close_menu();
                    }
                }
            });
            if ui.button("Center canvas").clicked() {
                self.pan = egui::Vec2::ZERO;
            }
            ui.small("Drag nodes to arrange · drag empty canvas to pan");
        });
        ui.small("Commands, committed ticks and graph nodes; internal subsystem events are not all instrumented.");
        let active = state.as_ref().and_then(|s| {
            s.debug.breakpoint_node.or_else(|| {
                let now = client.snapshot()?.server_time_ms;
                s.debug
                    .pending_nodes
                    .iter()
                    .find(|n| n.due_at_ms <= now)
                    .map(|n| n.node_id)
            })
        });
        let width = ui.available_width();
        if width > 720.0 {
            ui.columns(2, |columns| {
                self.canvas(&mut columns[0], active);
                egui::ScrollArea::vertical()
                    .id_salt("event_inspector")
                    .max_height(230.)
                    .show(&mut columns[1], |ui| self.inspector(ui, client));
            });
        } else {
            self.canvas(ui, active);
            egui::ScrollArea::vertical()
                .id_salt("event_inspector")
                .max_height(140.)
                .show(ui, |ui| self.inspector(ui, client));
        }
        if let Some(state) = state {
            ui.separator();
            ui.label(format!(
                "Runtime revision {} · {} · breakpoint {:?}",
                state.debug.revision,
                if state.debug.paused {
                    "Paused"
                } else {
                    "Running"
                },
                state.debug.breakpoint_node
            ));
            egui::CollapsingHeader::new("Pending queue and causal trace")
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("event_trace")
                        .max_height(110.)
                        .show(ui, |ui| {
                            ui.monospace(format!("Pending: {} (execution order)", state.debug.pending));
                            for pending in state.debug.pending_nodes.iter().take(16) {
                                ui.monospace(format!("node #{} · due {} ms · cause {}",pending.node_id,pending.due_at_ms,pending.cause.map_or("root".into(),|n|n.to_string())));
                            }
                            if state.debug.pending_nodes.len()>16 {
                                egui::CollapsingHeader::new(format!("{} more queued nodes",state.debug.pending_nodes.len()-16)).show(ui,|ui|{
                                    for pending in state.debug.pending_nodes.iter().skip(16){ui.monospace(format!("node #{} · due {} ms · cause {:?}",pending.node_id,pending.due_at_ms,pending.cause));}
                                });
                            }
                            ui.small(format!("Showing latest {} of {} retained trace entries (retention limit 256).",state.debug.trace.len().min(64),state.debug.trace.len()));
                            for trace in state.debug.trace.iter().rev().take(64) {
                                ui.colored_label(
                                    if trace.ok {
                                        Color32::LIGHT_GREEN
                                    } else {
                                        Color32::LIGHT_RED
                                    },
                                    format!(
                                        "rev{} #{} ← {} · node {:?} · {} ms · {} µs · {}: {}",
                                        trace.graph_revision,
                                        trace.sequence,
                                        trace.cause.map_or("root".into(), |v| v.to_string()),
                                        trace.node_id,
                                        trace.simulation_time_ms,
                                        trace.host_duration_us,
                                        trace.kind,
                                        trace.message
                                    ),
                                );
                            }
                        });
                });
        } else {
            ui.small("Waiting for the local runtime event debugger.");
        }
    }
    fn record_edit(&mut self, before: EditorEventGraph, dragging: bool) {
        if self.suppress_history {
            return;
        }
        if self.draft != before {
            if dragging {
                if self.gesture.is_none() {
                    self.gesture = Some(before);
                }
            } else {
                push_history(&mut self.undo, self.gesture.take().unwrap_or(before));
                self.redo.clear();
            }
        } else if !dragging {
            if let Some(start) = self.gesture.take() {
                if start != self.draft {
                    push_history(&mut self.undo, start);
                    self.redo.clear();
                }
            }
        }
    }
    fn queued(&mut self, result: Result<u64, String>) {
        match result {
            Ok(id) => {
                self.pending = Some(id);
                self.status = format!("Command {id} queued; awaiting runtime acknowledgement.");
            }
            Err(e) => self.status = e,
        }
    }
    fn canvas(&mut self, ui: &mut egui::Ui, active: Option<u64>) {
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), 210.), egui::Sense::drag());
        if response.dragged() {
            self.pan += response.drag_delta();
        }
        let origin = rect.min + self.pan;
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4., Color32::from_rgb(8, 15, 17));
        for edge in &self.draft.edges {
            let a = self.draft.nodes.iter().find(|n| n.id == edge.from);
            let b = self.draft.nodes.iter().find(|n| n.id == edge.to);
            if let (Some(a), Some(b)) = (a, b) {
                let a = origin + egui::vec2(a.position[0] + 140., a.position[1] + 23.);
                let b = origin + egui::vec2(b.position[0], b.position[1] + 23.);
                painter.arrow(
                    a,
                    b - a,
                    egui::Stroke::new(1.5, Color32::from_rgb(53, 170, 120)),
                );
                painter.text(
                    a.lerp(b, 0.5),
                    egui::Align2::CENTER_CENTER,
                    format!("{:?}", edge.port),
                    egui::FontId::monospace(10.),
                    Color32::WHITE,
                );
            }
        }
        for node in &mut self.draft.nodes {
            let r = egui::Rect::from_min_size(
                origin + egui::vec2(node.position[0], node.position[1]),
                egui::vec2(140., 48.),
            );
            if !r.intersects(rect) {
                continue;
            }
            let response = ui.interact(
                r.intersect(rect),
                ui.id().with(("event_node", node.id)),
                egui::Sense::click_and_drag(),
            );
            if response.clicked() || response.dragged() {
                self.selected = Some(node.id);
            }
            if response.dragged() {
                let d = response.drag_delta();
                node.position[0] = (node.position[0] + d.x).clamp(-100000., 100000.);
                node.position[1] = (node.position[1] + d.y).clamp(-100000., 100000.);
            }
            let color = if active == Some(node.id) {
                Color32::from_rgb(150, 100, 20)
            } else if self.selected == Some(node.id) {
                Color32::from_rgb(22, 82, 57)
            } else {
                Color32::from_rgb(28, 38, 43)
            };
            painter.rect_filled(r, 4., color);
            painter.text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                format!(
                    "{} #{}{}\n{}",
                    op_name(&node.operation),
                    node.id,
                    if node.breakpoint { " ●" } else { "" },
                    node.label.chars().take(20).collect::<String>()
                ),
                egui::FontId::proportional(12.),
                Color32::WHITE,
            );
        }
    }
    fn inspector(&mut self, ui: &mut egui::Ui, client: &mut EmbeddedClient) {
        if let Some(id) = self.selected {
            if let Some(node) = self.draft.nodes.iter_mut().find(|n| n.id == id) {
                ui.strong(format!(
                    "Selected node #{id} · {}",
                    op_name(&node.operation)
                ));
                ui.add(
                    egui::TextEdit::singleline(&mut node.label)
                        .char_limit(80)
                        .desired_width(190.),
                );
                ui.checkbox(&mut node.breakpoint, "Pause before this node");
                match &mut node.operation {
                    EditorNodeOp::Delay { ms } => {
                        ui.horizontal(|ui| {
                            ui.label("Delay ms");
                            ui.add(egui::DragValue::new(ms).range(0..=60000));
                        });
                    }
                    EditorNodeOp::Trace { message } => {
                        ui.add(
                            egui::TextEdit::singleline(message)
                                .char_limit(256)
                                .desired_width(190.),
                        );
                    }
                    EditorNodeOp::RoomLight { on } => {
                        ui.checkbox(on, "Light on");
                    }
                    EditorNodeOp::RoomBreaker { closed } => {
                        ui.checkbox(closed, "Breaker closed");
                    }
                    EditorNodeOp::RoomDoor { open } => {
                        ui.checkbox(open, "Door open (interlocks apply)");
                    }
                    _ => {}
                }
                if matches!(node.operation, EditorNodeOp::Manual)
                    && ui
                        .add_enabled(
                            self.pending.is_none()
                                && client.graph_debug().is_some_and(|s| {
                                    s.debug.graph.nodes.iter().any(|n| {
                                        n.id == id && matches!(n.operation, EditorNodeOp::Manual)
                                    })
                                }),
                            egui::Button::new("Trigger applied Manual node"),
                        )
                        .clicked()
                {
                    self.queued(client.graph_trigger(id));
                }
                if ui.button("Remove node and its connections").clicked() {
                    self.draft.nodes.retain(|n| n.id != id);
                    self.draft.edges.retain(|e| e.from != id && e.to != id);
                    self.selected = None;
                }
            }
        } else {
            ui.label("Select a node to edit its parameters and breakpoint.");
        }
        ui.separator();
        ui.strong("Typed connection");
        for (label, value) in [("From", &mut self.from), ("To", &mut self.to)] {
            egui::ComboBox::from_id_salt(label)
                .selected_text(format!("{label} #{value}"))
                .show_ui(ui, |ui| {
                    for node in &self.draft.nodes {
                        ui.selectable_value(value, node.id, format!("#{} {}", node.id, node.label));
                    }
                });
        }
        egui::ComboBox::from_id_salt("event_port")
            .selected_text(format!("{:?}", self.port))
            .show_ui(ui, |ui| {
                for port in [
                    EditorGraphPort::Next,
                    EditorGraphPort::True,
                    EditorGraphPort::False,
                ] {
                    ui.selectable_value(&mut self.port, port, format!("{port:?}"));
                }
            });
        if ui.button("Connect / replace this output").clicked() {
            let mut next = self.draft.clone();
            next.edges
                .retain(|e| e.from != self.from || e.port != self.port);
            next.edges.push(EditorGraphEdge {
                from: self.from,
                to: self.to,
                port: self.port,
            });
            match next.validate() {
                Ok(()) => {
                    self.draft = next;
                    self.status = "Draft connection updated; Apply to change runtime.".into();
                }
                Err(e) => self.status = e,
            }
        }
        if ui.button("Remove this output connection").clicked() {
            self.draft
                .edges
                .retain(|e| e.from != self.from || e.port != self.port);
        }
        ui.small("Actions use the connected local pilot and normal permission/safety checks. Editing a draft never runs it.");
    }
}
fn push_history(history: &mut Vec<EditorEventGraph>, graph: EditorEventGraph) {
    if history.len() == 64 {
        history.remove(0);
    }
    history.push(graph);
}
fn op_name(op: &EditorNodeOp) -> &'static str {
    match op {
        EditorNodeOp::Manual => "Manual",
        EditorNodeOp::Tick => "Tick",
        EditorNodeOp::Delay { .. } => "Delay",
        EditorNodeOp::IfDocked => "If docked",
        EditorNodeOp::RoomLight { .. } => "Room light",
        EditorNodeOp::RoomBreaker { .. } => "Room breaker",
        EditorNodeOp::RoomDoor { .. } => "Room door",
        EditorNodeOp::StopShip => "Stop ship",
        EditorNodeOp::Trace { .. } => "Trace",
    }
}
fn catalog() -> Vec<(&'static str, EditorNodeOp)> {
    vec![
        ("Manual", EditorNodeOp::Manual),
        ("Tick", EditorNodeOp::Tick),
        ("Delay", EditorNodeOp::Delay { ms: 500 }),
        ("If docked", EditorNodeOp::IfDocked),
        ("Room light", EditorNodeOp::RoomLight { on: true }),
        ("Room breaker", EditorNodeOp::RoomBreaker { closed: true }),
        ("Room door", EditorNodeOp::RoomDoor { open: true }),
        ("Stop ship", EditorNodeOp::StopShip),
        (
            "Trace",
            EditorNodeOp::Trace {
                message: "Reached node".into(),
            },
        ),
    ]
}
fn example() -> EditorEventGraph {
    EditorEventGraph {
        schema_version: 1,
        nodes: vec![
            EditorGraphNode {
                id: 1,
                label: "Start".into(),
                position: [20., 30.],
                operation: EditorNodeOp::Manual,
                breakpoint: false,
            },
            EditorGraphNode {
                id: 2,
                label: "Observe".into(),
                position: [200., 90.],
                operation: EditorNodeOp::Trace {
                    message: "Manual event received".into(),
                },
                breakpoint: false,
            },
        ],
        edges: vec![EditorGraphEdge {
            from: 1,
            to: 2,
            port: EditorGraphPort::Next,
        }],
    }
}
fn path(project: &Path, create: bool) -> Result<PathBuf, String> {
    for dir in [project.to_owned(), project.join("Events")] {
        if create && dir != project {
            match fs::create_dir(&dir) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        let m = fs::symlink_metadata(&dir).map_err(|e| e.to_string())?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err("Events paths must be ordinary directories.".into());
        }
    }
    let path = project.join("Events/Pioneer.events.json");
    match fs::symlink_metadata(&path) {
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
            return Err("Event graph must be an ordinary file.".into())
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
        _ => {}
    }
    Ok(path)
}
fn load(project: &Path) -> Result<EditorEventGraph, String> {
    let path = path(project, false)?;
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("Event draft exceeds256KiB.".into());
    }
    let graph: EditorEventGraph = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    graph.validate()?;
    Ok(graph)
}
fn save(project: &Path, graph: &EditorEventGraph) -> Result<(), String> {
    graph.validate()?;
    let bytes = serde_json::to_vec_pretty(graph).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > LIMIT {
        return Err("Event draft exceeds256KiB.".into());
    }
    let path = path(project, true)?;
    let temp = path.with_file_name(format!(
        ".Pioneer.events.{}.{}.tmp",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|e| e.to_string())?;
    let result = file.write_all(&bytes).and_then(|_| file.sync_all());
    drop(file);
    let result = result.and_then(|_| fs::rename(&temp, &path));
    if let Err(e) = result {
        let _ = fs::remove_file(&temp);
        return Err(e.to_string());
    }
    #[cfg(unix)]
    if let Ok(dir) = fs::File::open(path.parent().unwrap()) {
        let _ = dir.sync_all();
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn draft_roundtrip_and_invalid_replace_preserves_saved_graph() {
        let dir = std::env::temp_dir().join(format!(
            "arkline-event-document-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let graph = example();
        save(&dir, &graph).unwrap();
        assert_eq!(
            serde_json::to_value(load(&dir).unwrap()).unwrap(),
            serde_json::to_value(&graph).unwrap()
        );
        let mut invalid = graph.clone();
        invalid.edges[0].to = 999;
        assert!(save(&dir, &invalid).is_err());
        assert_eq!(load(&dir).unwrap().edges[0].to, 2);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn real_canvas_drag_moves_node_without_panning_or_rewiring() {
        let mut panel = EventGraphPanel::open(Path::new("/nonexistent-event-test"));
        let ctx = egui::Context::default();
        let origin = panel.draft.nodes[0].position;
        let edges = panel.draft.edges.clone();
        let mut frame = |events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 300.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| panel.canvas(ui, None));
                },
            );
        };
        frame(vec![]);
        frame(vec![]);
        let start = egui::pos2(70., 60.);
        let end = start + egui::vec2(40., 20.);
        frame(vec![
            egui::Event::PointerMoved(start),
            egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        frame(vec![egui::Event::PointerMoved(end)]);
        frame(vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert!(panel.draft.nodes[0].position[0] > origin[0] + 20.);
        assert_eq!(panel.pan, egui::Vec2::ZERO);
        assert_eq!(panel.draft.edges, edges);
    }
    #[test]
    fn drag_history_is_one_undo_step_and_draft_save_identity_is_exact() {
        let mut panel = EventGraphPanel::open(Path::new("/nonexistent-event-test"));
        let original = panel.draft.clone();
        for x in 0..10 {
            let before = panel.draft.clone();
            panel.draft.nodes[0].position[0] += x as f32;
            panel.record_edit(before, true);
        }
        panel.record_edit(panel.draft.clone(), false);
        assert_eq!(panel.undo.len(), 1);
        assert_eq!(panel.undo[0], original);
        panel.saved = Some(original.clone());
        assert_ne!(panel.saved.as_ref(), Some(&panel.draft));
        for i in 0..100 {
            let before = panel.draft.clone();
            panel.draft.nodes[0].label = format!("Node{i}");
            panel.record_edit(before, false);
        }
        assert_eq!(panel.undo.len(), 64);
    }
    #[cfg(unix)]
    #[test]
    fn symlink_events_directory_is_rejected() {
        use std::os::unix::fs::symlink;
        let dir = std::env::temp_dir().join(format!(
            "arkline-event-symlink-{}-{}",
            std::process::id(),
            TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        symlink(std::env::temp_dir(), dir.join("Events")).unwrap();
        assert!(save(&dir, &example()).is_err());
        fs::remove_dir_all(dir).unwrap();
    }
}
