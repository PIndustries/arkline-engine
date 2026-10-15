//! Typed hull surface authoring. The fleet finish document remains a separate workspace.
use arkline_client::{MaterialProgramStatus, MaterialRevision};
use arkline_protocol::material_compile::lower_material;
use arkline_protocol::material_graph::{
    MaterialGraphDocument, MaterialNode, MaterialNodeOp, MaterialParameter, MaterialParameterValue,
    MaterialValueType, MATERIAL_VALUE_LIMIT,
};
use eframe::egui;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
        Arc, Condvar, Mutex, OnceLock, Weak,
    },
    thread,
};

const FILE_NAME: &str = "Pioneer.surface.json";
const MAX_BYTES: u64 = 128 * 1024;
const HISTORY_LIMIT: usize = 100;
const NODE_LIMIT: usize = 64;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
#[path = "surface_materials_tests.rs"]
mod tests;

#[derive(Clone)]
struct History {
    current: MaterialGraphDocument,
    undo: VecDeque<MaterialGraphDocument>,
    redo: VecDeque<MaterialGraphDocument>,
}
impl History {
    fn new(current: MaterialGraphDocument) -> Self {
        Self {
            current,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        }
    }
    // Incomplete drafts belong in history too. Only validated snapshots may reach disk/GPU.
    fn apply(&mut self, document: MaterialGraphDocument) -> bool {
        if document == self.current {
            return false;
        }
        Self::push(
            &mut self.undo,
            std::mem::replace(&mut self.current, document),
        );
        self.redo.clear();
        true
    }
    fn push(history: &mut VecDeque<MaterialGraphDocument>, document: MaterialGraphDocument) {
        if history.len() == HISTORY_LIMIT {
            history.pop_front();
        }
        history.push_back(document);
    }
    fn move_to(&mut self, redo: bool) -> bool {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let Some(next) = from.pop_back() else {
            return false;
        };
        Self::push(to, std::mem::replace(&mut self.current, next));
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IoKind {
    Open,
    Reload,
    Save,
}
enum IoAction {
    Open,
    Reload,
    Save(MaterialGraphDocument),
}
impl IoAction {
    fn kind(&self) -> IoKind {
        match self {
            Self::Open => IoKind::Open,
            Self::Reload => IoKind::Reload,
            Self::Save(_) => IoKind::Save,
        }
    }
}
struct IoRequest {
    id: u64,
    project_ticket: u64,
    action: IoAction,
}
struct IoResult {
    id: u64,
    kind: IoKind,
    result: Result<Option<MaterialGraphDocument>, String>,
}
struct PendingIo {
    id: u64,
    kind: IoKind,
    edit_revision: u64,
    submitted: Option<MaterialGraphDocument>,
}

#[derive(Default)]
struct ProjectOrder {
    state: Mutex<ProjectOrderState>,
    ready: Condvar,
}
#[derive(Default)]
struct ProjectOrderState {
    issued: u64,
    serving: u64,
}
impl ProjectOrder {
    fn shared(project: &Path) -> Arc<Self> {
        static PROJECTS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<ProjectOrder>>>> = OnceLock::new();
        let projects = PROJECTS.get_or_init(Mutex::default);
        let mut projects = projects.lock().unwrap_or_else(|e| e.into_inner());
        projects.retain(|_, order| order.strong_count() != 0);
        let order = projects.entry(project.to_path_buf()).or_default();
        if let Some(order) = order.upgrade() {
            return order;
        }
        let next = Arc::new(Self::default());
        *order = Arc::downgrade(&next);
        next
    }
    // Reserve under the queue lock, not later in the worker: accepted requests retain FIFO
    // order even if a newly opened panel's thread starts before the old worker is scheduled.
    fn submit(&self, sender: &SyncSender<IoRequest>, mut request: IoRequest) -> Result<(), String> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let next = state
            .issued
            .checked_add(1)
            .ok_or("Project surface operation IDs exhausted")?;
        request.project_ticket = state.issued;
        sender.try_send(request).map_err(|e| e.to_string())?;
        state.issued = next;
        Ok(())
    }
    fn enter(self: &Arc<Self>, ticket: u64) -> ProjectPermit {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while state.serving != ticket {
            state = self.ready.wait(state).unwrap_or_else(|e| e.into_inner());
        }
        // No queue/state mutex is held during filesystem access or user-controlled latency.
        drop(state);
        ProjectPermit(self.clone())
    }
}
struct ProjectPermit(Arc<ProjectOrder>);
impl Drop for ProjectPermit {
    fn drop(&mut self) {
        let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
        state.serving = state.serving.saturating_add(1);
        self.0.ready.notify_all();
    }
}

/// One panel, project-bound worker and monotonically increasing renderer revision stream.
pub struct SurfaceMaterials {
    history: History,
    saved: Option<MaterialGraphDocument>,
    io_requests: Option<SyncSender<IoRequest>>,
    io_results: Receiver<IoResult>,
    project_order: Arc<ProjectOrder>,
    pending: Option<PendingIo>,
    next_operation: u64,
    edit_revision: u64,
    generation: u64,
    gpu_revision: u64,
    last_good: Option<Arc<MaterialRevision>>,
    last_good_document: Option<MaterialGraphDocument>,
    diagnostics: Vec<String>,
    program_status: Option<MaterialProgramStatus>,
    load_blocked: bool,
    enabled: bool,
    message: String,
    selected: Option<u32>,
    connecting: Option<u32>,
    pan: egui::Vec2,
    zoom: f32,
    drag_document: Option<MaterialGraphDocument>,
    frame_requested: bool,
}
impl SurfaceMaterials {
    pub fn open(project: &Path, generation: u64) -> Self {
        let root = project.to_path_buf();
        Self::with_worker(project, generation, move |action| match action {
            IoAction::Open => load_optional(&root),
            IoAction::Reload => load(&root).map(Some),
            IoAction::Save(document) => save(&root, &document).map(|()| Some(document)),
        })
    }
    fn with_worker(
        project: &Path,
        generation: u64,
        mut handle: impl FnMut(IoAction) -> Result<Option<MaterialGraphDocument>, String>
            + Send
            + 'static,
    ) -> Self {
        let (requests, receiver) = mpsc::sync_channel::<IoRequest>(1);
        let (results, io_results) = mpsc::sync_channel(1);
        let project_order = ProjectOrder::shared(project);
        let worker_order = project_order.clone();
        let worker = thread::Builder::new()
            .name("arkline-surface-io".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    let permit = worker_order.enter(request.project_ticket);
                    let kind = request.action.kind();
                    let result = handle(request.action);
                    drop(permit);
                    if results
                        .send(IoResult {
                            id: request.id,
                            kind,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        let mut panel = Self {
            history: History::new(MaterialGraphDocument::pioneer_default()),
            saved: None,
            io_requests: worker.ok().map(|_| requests),
            io_results,
            project_order,
            pending: None,
            next_operation: 0,
            edit_revision: 0,
            generation,
            gpu_revision: 0,
            last_good: None,
            last_good_document: None,
            diagnostics: Vec::new(),
            program_status: None,
            load_blocked: true,
            enabled: false,
            message: "Loading hull surface…".into(),
            selected: None,
            connecting: None,
            pan: egui::vec2(20., 20.),
            zoom: 0.7,
            drag_document: None,
            frame_requested: true,
        };
        panel.refresh_revision();
        panel.queue(IoAction::Open);
        panel
    }
    /// Poll even when hidden. Invalid edits keep returning the most recent valid graph.
    pub fn revision(&mut self) -> Option<Arc<MaterialRevision>> {
        self.poll_io();
        self.enabled.then(|| self.last_good.clone()).flatten()
    }
    #[cfg(test)]
    fn document(&self) -> &MaterialGraphDocument {
        &self.history.current
    }
    #[cfg(test)]
    fn diagnostics(&self) -> &[String] {
        &self.diagnostics
    }
    /// The host supplies the actual active viewport's context-local status after painting.
    pub fn set_program_status(&mut self, status: MaterialProgramStatus) {
        self.program_status = Some(status);
    }
    fn draft(&self) -> MaterialGraphDocument {
        self.drag_document
            .as_ref()
            .unwrap_or(&self.history.current)
            .clone()
    }
    fn is_dirty(&self) -> bool {
        self.saved.as_ref() != Some(self.drag_document.as_ref().unwrap_or(&self.history.current))
    }
    fn apply(&mut self, document: MaterialGraphDocument) {
        if self.history.apply(document) {
            self.edit_revision = self.edit_revision.saturating_add(1);
            self.refresh_revision();
            self.message = "Hull surface edited · Save keeps the graph and binding".into();
        }
    }
    fn refresh_revision(&mut self) {
        // Layout is saved authoring data, but never a shader or parameter revision.
        let mut semantic = self.history.current.clone();
        semantic.layout.clear();
        if self.last_good_document.as_ref() == Some(&semantic) {
            self.diagnostics.clear();
            return;
        }
        let Some(next) = self.gpu_revision.checked_add(1) else {
            self.diagnostics = vec!["Surface revision IDs exhausted; reopen the project.".into()];
            return;
        };
        match MaterialRevision::new(self.generation, next, &self.history.current) {
            Ok(revision) => {
                self.gpu_revision = next;
                self.last_good = Some(Arc::new(revision));
                self.last_good_document = Some(semantic);
                self.diagnostics.clear();
            }
            Err(errors) => self.diagnostics = errors.into_iter().map(|e| e.to_string()).collect(),
        }
    }
    fn move_history(&mut self, redo: bool) {
        if self.history.move_to(redo) {
            self.edit_revision = self.edit_revision.saturating_add(1);
            self.drag_document = None;
            self.connecting = None;
            self.refresh_revision();
        }
    }
    fn can_save(&self) -> bool {
        self.pending.is_none()
            && self.io_requests.is_some()
            && !self.load_blocked
            && self.drag_document.is_none()
            && self.diagnostics.is_empty()
    }
    fn queue(&mut self, action: IoAction) {
        if self.pending.is_some() {
            return;
        }
        let Some(sender) = &self.io_requests else {
            self.load_blocked = true;
            self.message = "Surface file worker is unavailable. Reopen before saving.".into();
            return;
        };
        let Some(next) = self.next_operation.checked_add(1) else {
            self.load_blocked = true;
            self.message = "Surface operation IDs exhausted. Reopen the project.".into();
            return;
        };
        let pending = PendingIo {
            id: self.next_operation,
            kind: action.kind(),
            edit_revision: self.edit_revision,
            submitted: match &action {
                IoAction::Save(doc) => Some(doc.clone()),
                _ => None,
            },
        };
        match self.project_order.submit(
            sender,
            IoRequest {
                id: pending.id,
                project_ticket: 0,
                action,
            },
        ) {
            Ok(()) => {
                self.next_operation = next;
                self.pending = Some(pending);
            }
            Err(error) => {
                self.load_blocked = true;
                self.message = format!("Could not queue surface file operation: {error}");
            }
        }
    }
    fn save(&mut self) {
        if self.can_save() {
            self.queue(IoAction::Save(self.history.current.clone()));
        }
    }
    fn reload(&mut self) {
        self.queue(IoAction::Reload);
    }
    fn allow_replacement(&mut self) {
        if self.pending.is_none() && self.io_requests.is_some() && self.load_blocked {
            self.load_blocked = false;
            self.saved = None;
            self.message =
                "Replacement allowed. Save will replace this project's unreadable surface file."
                    .into();
        }
    }
    fn poll_io(&mut self) {
        if self.pending.is_none() {
            return;
        }
        let result = match self.io_results.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.io_requests = None;
                self.load_blocked = true;
                self.message = "Surface worker stopped. A pending save may have completed; reopen and read the file before saving again.".into();
                return;
            }
        };
        let pending = self.pending.take().expect("pending surface request");
        if pending.id != result.id || pending.kind != result.kind {
            self.io_requests = None;
            self.load_blocked = true;
            self.message =
                "Surface receipt mismatched its request. Reopen and read before saving.".into();
            return;
        }
        let document = match result.result {
            Ok(document) => document,
            Err(error) => {
                if pending.kind != IoKind::Save {
                    self.load_blocked = true;
                }
                self.message = format!("Surface {:?} failed: {error}", pending.kind);
                return;
            }
        };
        if let Some(document) = &document {
            if let Err(error) = validate_document(document) {
                self.load_blocked = true;
                self.message = error;
                return;
            }
        }
        if pending.kind == IoKind::Save {
            if document.is_none() || document != pending.submitted {
                self.io_requests = None;
                self.load_blocked = true;
                self.message = "Surface save receipt differed from the submitted snapshot. Reopen and read before saving.".into();
                return;
            }
            self.saved = document;
            self.load_blocked = false;
            self.message = if self.is_dirty() {
                "Saved submitted surface; newer edits remain unsaved."
            } else {
                "Saved Assets/Pioneer.surface.json"
            }
            .into();
            return;
        }
        if pending.kind == IoKind::Reload && document.is_none() {
            self.load_blocked = true;
            self.message = "Saved surface is missing. Saving remains locked.".into();
            return;
        }
        let unchanged = self.edit_revision == pending.edit_revision && self.drag_document.is_none();
        if unchanged {
            if let Some(document) = &document {
                if pending.kind == IoKind::Open {
                    self.history = History::new(document.clone());
                } else if self.history.apply(document.clone()) {
                    self.edit_revision = self.edit_revision.saturating_add(1);
                }
                self.connecting = None;
                self.selected = None;
                self.frame_requested = true;
                self.refresh_revision();
            }
        }
        self.saved = document;
        self.load_blocked = false;
        self.message = if !unchanged {
            "Loaded saved baseline; edits made during loading were kept."
        } else if self.saved.is_none() {
            "No saved hull surface. The starter graph is an unsaved draft."
        } else if pending.kind == IoKind::Open {
            "Loaded Assets/Pioneer.surface.json"
        } else {
            "Reloaded saved surface. Undo restores the previous draft."
        }
        .into();
    }
    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        self.poll_io();
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.enabled, "Apply hull surface");
            if ui
                .add_enabled(!self.history.undo.is_empty(), egui::Button::new("Undo"))
                .clicked()
            {
                self.move_history(false);
            }
            if ui
                .add_enabled(!self.history.redo.is_empty(), egui::Button::new("Redo"))
                .clicked()
            {
                self.move_history(true);
            }
            if ui
                .add_enabled(self.can_save(), egui::Button::new("Save surface"))
                .clicked()
            {
                self.save();
            }
            if ui
                .add_enabled(
                    self.pending.is_none() && self.io_requests.is_some(),
                    egui::Button::new("Reload surface"),
                )
                .clicked()
            {
                self.reload();
            }
            if ui.button("Frame graph").clicked() {
                self.frame_requested = true;
            }
            ui.label(if self.saved.is_none() {
                "Not saved"
            } else if self.is_dirty() {
                "Unsaved"
            } else {
                "Saved"
            });
            if let Some(pending) = &self.pending {
                ui.spinner();
                ui.label(match pending.kind {
                    IoKind::Open => "Loading…",
                    IoKind::Reload => "Reloading…",
                    IoKind::Save => "Saving…",
                });
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(33));
            }
        });
        if self.load_blocked && self.pending.is_none() {
            ui.colored_label(
                egui::Color32::YELLOW,
                "Save is locked until Reload succeeds or replacement is allowed.",
            );
            if ui
                .add_enabled(
                    self.io_requests.is_some(),
                    egui::Button::new("Allow replacing unreadable surface"),
                )
                .clicked()
            {
                self.allow_replacement();
            }
        }
        if !self.diagnostics.is_empty() {
            ui.colored_label(egui::Color32::YELLOW, "Incomplete graph: the viewport keeps the last valid surface. Repair the highlighted inputs before saving.");
        }
    }
    pub fn show_library(&mut self, ui: &mut egui::Ui) {
        ui.heading("Hull nodes");
        ui.small("Object coordinates are in body metres. Add nodes, then connect matching ports.");
        let nodes = [
            ("+ Body position", MaterialNodeOp::BodyPosition),
            ("+ Authored surface", MaterialNodeOp::AuthoredSurface),
            (
                "+ Hull region mask",
                MaterialNodeOp::BodyBoxMask {
                    position: 0,
                    min: 0,
                    max: 0,
                    feather: 0,
                },
            ),
            (
                "+ Transform position",
                MaterialNodeOp::TransformPosition {
                    position: 0,
                    scale: 0,
                    offset: 0,
                },
            ),
            (
                "+ Noise 3D",
                MaterialNodeOp::Noise3D {
                    position: 0,
                    seed: 0,
                },
            ),
            (
                "+ Remap",
                MaterialNodeOp::Remap {
                    value: 0,
                    in_min: 0,
                    in_max: 0,
                    out_min: 0,
                    out_max: 0,
                },
            ),
            (
                "+ Smoothstep",
                MaterialNodeOp::Smoothstep {
                    value: 0,
                    edge0: 0,
                    edge1: 0,
                },
            ),
            (
                "+ Surface",
                MaterialNodeOp::Surface {
                    base_color: 0,
                    roughness: 0,
                    metallic: 0,
                    emission: 0,
                },
            ),
            (
                "+ Blend surfaces",
                MaterialNodeOp::MixSurface {
                    a: 0,
                    b: 0,
                    factor: 0,
                },
            ),
        ];
        for (label, op) in nodes {
            if ui
                .add_enabled(
                    self.history.current.nodes.len() < NODE_LIMIT,
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.add_node(op);
            }
        }
        ui.separator();
        ui.label("Named inputs");
        for (label, value) in [
            ("+ Scalar", MaterialParameterValue::Scalar(0.5)),
            ("+ Vector 3", MaterialParameterValue::Vector3([1.; 3])),
            (
                "+ Linear color",
                MaterialParameterValue::LinearColor([0.3; 3]),
            ),
            ("+ Seed", MaterialParameterValue::Seed(7)),
        ] {
            if ui
                .add_enabled(
                    self.history.current.nodes.len() < NODE_LIMIT
                        && self.history.current.parameters.len() < NODE_LIMIT,
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.add_parameter(value);
            }
        }
        ui.collapsing("Reuse input", |ui| {
            let parameters = self.history.current.parameters.clone();
            for parameter in parameters {
                if ui
                    .add_enabled(
                        self.history.current.nodes.len() < NODE_LIMIT,
                        egui::Button::new(format!("+ {}", parameter.name)),
                    )
                    .clicked()
                {
                    self.add_node(MaterialNodeOp::Parameter {
                        parameter: parameter.id,
                    });
                }
            }
        });
        ui.separator();
        if ui.button("Restore hull section starter").clicked() {
            self.apply(MaterialGraphDocument::pioneer_default());
            self.selected = None;
            self.connecting = None;
            self.frame_requested = true;
        }
        ui.small("This graph shades one explicitly bound Pioneer hull material. UV, images, normal maps and decals are future work.");
        ui.small(format!(
            "{} / {NODE_LIMIT} nodes",
            self.history.current.nodes.len()
        ));
    }
    fn add_node(&mut self, op: MaterialNodeOp) {
        let mut draft = self.draft();
        if draft.nodes.len() >= NODE_LIMIT {
            self.message = "A hull surface supports at most 64 nodes.".into();
            return;
        }
        let Some(id) = draft
            .nodes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            self.message = "Node IDs exhausted; remove or repair the imported IDs.".into();
            return;
        };
        let offset = (draft.nodes.len() % 6) as f32 * 28.;
        draft.nodes.push(MaterialNode { id, op });
        draft.layout.insert(
            id,
            [
                (60. + offset - self.pan.x) / self.zoom,
                (60. + offset - self.pan.y) / self.zoom,
            ],
        );
        self.selected = Some(id);
        self.apply(draft);
    }
    fn add_parameter(&mut self, value: MaterialParameterValue) {
        let mut draft = self.draft();
        if draft.nodes.len() >= NODE_LIMIT || draft.parameters.len() >= NODE_LIMIT {
            return;
        }
        let Some(parameter) = draft
            .parameters
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            self.message = "Parameter IDs exhausted.".into();
            return;
        };
        let Some(id) = draft
            .nodes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
        else {
            self.message = "Node IDs exhausted.".into();
            return;
        };
        draft.parameters.push(MaterialParameter {
            id: parameter,
            name: format!("Input {parameter}"),
            value,
        });
        draft.nodes.push(MaterialNode {
            id,
            op: MaterialNodeOp::Parameter { parameter },
        });
        draft.layout.insert(
            id,
            [
                (60. - self.pan.x) / self.zoom,
                (60. - self.pan.y) / self.zoom,
            ],
        );
        self.selected = Some(id);
        self.apply(draft);
    }
    pub fn show_graph(&mut self, ui: &mut egui::Ui) {
        self.poll_io();
        ui.small(
            "Output → input connects · right-click input disconnects · drag headers · scroll zooms",
        );
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(
                ui.available_width().max(240.),
                ui.available_height().max(260.),
            ),
            egui::Sense::click_and_drag(),
        );
        let original_clip = ui.clip_rect();
        // Clip interactions as well as paint: panned nodes cannot capture toolbar clicks.
        ui.set_clip_rect(original_clip.intersect(rect));
        let painter = ui.painter().with_clip_rect(rect);
        painter.rect_filled(rect, 0., egui::Color32::from_rgb(19, 23, 30));
        let mut draft = self.draft();
        if self.frame_requested {
            let mut bounds = egui::Rect::NOTHING;
            for node in &draft.nodes {
                let p = node_position(&draft, node.id);
                bounds = bounds.union(egui::Rect::from_min_size(
                    egui::pos2(p[0], p[1]),
                    egui::vec2(194., node_height(node)),
                ));
            }
            if bounds.is_positive() {
                self.zoom = ((rect.width() - 48.) / bounds.width())
                    .min((rect.height() - 48.) / bounds.height())
                    .clamp(0.25, 1.2);
                self.pan = rect.size() * 0.5 - bounds.center().to_vec2() * self.zoom;
            }
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
                draft = self.draft();
            }
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.1 {
                let old = self.zoom;
                self.zoom = (old * (scroll * 0.002).exp()).clamp(0.25, 1.6);
                let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or(rect.center());
                let local = cursor - rect.min;
                self.pan = local - (local - self.pan) * (self.zoom / old);
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.connecting = None;
            }
        }
        let screen = |p: [f32; 2]| rect.min + self.pan + egui::vec2(p[0], p[1]) * self.zoom;
        for node in &draft.nodes {
            for (index, (_, ty, source)) in node.op.inputs().iter().enumerate() {
                if let Some(from) = draft.nodes.iter().find(|n| n.id == *source) {
                    draw_wire(
                        &painter,
                        screen(node_position(&draft, from.id)) + egui::vec2(194., 50.) * self.zoom,
                        screen(node_position(&draft, node.id))
                            + egui::vec2(0., 50. + index as f32 * 25.) * self.zoom,
                        port_color(*ty),
                    );
                }
            }
        }
        if let Some(source) = self.connecting {
            if let Some(node) = draft.nodes.iter().find(|n| n.id == source) {
                if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                    draw_wire(
                        &painter,
                        screen(node_position(&draft, source)) + egui::vec2(194., 50.) * self.zoom,
                        pointer,
                        node_output_type(&draft, node)
                            .map(port_color)
                            .unwrap_or(egui::Color32::GRAY),
                    );
                }
            }
        }
        let mut changed = false;
        let mut dragging = false;
        let mut drag_finished = false;
        let mut occupied = false;
        let mut connect = None;
        let mut disconnect = None;
        for node in &draft.nodes {
            let origin = screen(node_position(&draft, node.id));
            let body =
                egui::Rect::from_min_size(origin, egui::vec2(194., node_height(node)) * self.zoom);
            occupied |= ui.input(|i| {
                i.pointer
                    .hover_pos()
                    .is_some_and(|p| body.expand(10.).contains(p))
            });
            painter.rect_filled(body, 5., egui::Color32::from_rgb(37, 43, 54));
            painter.rect_stroke(
                body,
                5.,
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
            let header = egui::Rect::from_min_size(origin, egui::vec2(194., 30.) * self.zoom);
            painter.rect_filled(header, 5., egui::Color32::from_rgb(51, 61, 78));
            let title = if let MaterialNodeOp::Parameter { parameter } = node.op {
                draft
                    .parameters
                    .iter()
                    .find(|p| p.id == parameter)
                    .map(|p| p.name.as_str())
                    .unwrap_or("Missing input")
            } else {
                node.op.name()
            };
            painter.text(
                header.center(),
                egui::Align2::CENTER_CENTER,
                title,
                egui::FontId::proportional(12. * self.zoom),
                egui::Color32::WHITE,
            );
            let hit = ui.interact(
                header,
                ui.id().with(("surface_node", node.id)),
                egui::Sense::click_and_drag(),
            );
            if hit.clicked() || hit.drag_started() {
                self.selected = Some(node.id);
            }
            if hit.dragged() {
                let p = draft.layout.entry(node.id).or_insert([0., 0.]);
                let delta = ui.input(|i| i.pointer.delta()) / self.zoom;
                p[0] = (p[0] + delta.x).clamp(-100_000., 100_000.);
                p[1] = (p[1] + delta.y).clamp(-100_000., 100_000.);
                dragging = true;
                changed = true;
            }
            drag_finished |= hit.drag_stopped();
            if let Some(ty) = node_output_type(&draft, node) {
                let center = origin + egui::vec2(194., 50.) * self.zoom;
                painter.circle_filled(center, 5. * self.zoom, port_color(ty));
                if ui
                    .interact(
                        egui::Rect::from_center_size(center, egui::vec2(20., 20.)),
                        ui.id().with(("surface_out", node.id)),
                        egui::Sense::click(),
                    )
                    .clicked()
                {
                    self.connecting = Some(node.id);
                    self.selected = Some(node.id);
                }
                if node.op.inputs().is_empty() {
                    painter.text(
                        origin + egui::vec2(12., 53.) * self.zoom,
                        egui::Align2::LEFT_CENTER,
                        value_summary(&draft, node),
                        egui::FontId::proportional(10. * self.zoom),
                        port_color(ty),
                    );
                }
            }
            for (index, (name, ty, source)) in node.op.inputs().iter().enumerate() {
                let center = origin + egui::vec2(0., 50. + index as f32 * 25.) * self.zoom;
                painter.circle_filled(center, 5. * self.zoom, port_color(*ty));
                painter.text(
                    center + egui::vec2(12. * self.zoom, 0.),
                    egui::Align2::LEFT_CENTER,
                    name,
                    egui::FontId::proportional(11. * self.zoom),
                    if *source == 0 {
                        egui::Color32::from_rgb(255, 168, 110)
                    } else {
                        egui::Color32::WHITE
                    },
                );
                let hit = ui.interact(
                    egui::Rect::from_center_size(center, egui::vec2(20., 20.)),
                    ui.id().with(("surface_in", node.id, index)),
                    egui::Sense::click(),
                );
                if hit.clicked() {
                    self.selected = Some(node.id);
                    if let Some(source) = self.connecting {
                        connect = Some((source, node.id, index));
                    }
                }
                if hit.secondary_clicked() {
                    disconnect = Some((node.id, index));
                }
            }
        }
        // Egui retains the drag owner. Crossing a node must not interrupt a canvas drag.
        if response.dragged() && !dragging {
            self.pan += ui.input(|i| i.pointer.delta());
        }
        if response.clicked() && !occupied {
            self.selected = None;
            self.connecting = None;
        }
        if let Some((source, target, input)) = connect {
            match connect_nodes(&mut draft, source, target, input) {
                Ok(()) => {
                    changed = true;
                    self.connecting = None;
                }
                Err(error) => self.message = error,
            }
        }
        if let Some((target, input)) = disconnect {
            if let Some(node) = draft.nodes.iter_mut().find(|n| n.id == target) {
                let _ = node.op.set_input(input, 0);
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
        if self.connecting.is_some() {
            painter.text(
                rect.left_bottom() + egui::vec2(10., -10.),
                egui::Align2::LEFT_BOTTOM,
                "Choose a matching input · Esc cancels",
                egui::FontId::proportional(12.),
                egui::Color32::LIGHT_BLUE,
            );
        }
        ui.set_clip_rect(original_clip);
    }
    pub fn show_node_inspector(&mut self, ui: &mut egui::Ui) {
        ui.heading("Hull surface");
        ui.small("Target: Pioneer authored hull material slot 0. The asset content hash must match before the renderer applies this graph.");
        ui.collapsing("Exact target binding", |ui| {
            let binding = &self.history.current.binding;
            ui.monospace(&binding.asset_id);
            ui.small(format!("Material slot {}", binding.material_index));
            ui.add(egui::Label::new(egui::RichText::new(&binding.asset_sha256).monospace()).wrap());
        });
        let mut draft = self.draft();
        let mut changed = false;
        let mut delete = None;
        if let Some(id) = self.selected {
            if let Some(node) = draft.nodes.iter().find(|n| n.id == id).cloned() {
                ui.separator();
                ui.label(format!("{} · node {id}", node.op.name()));
                match node.op {
                    MaterialNodeOp::Parameter { parameter } => {
                        if let Some(parameter) =
                            draft.parameters.iter_mut().find(|p| p.id == parameter)
                        {
                            ui.label("Shared input name");
                            let mut name = parameter.name.clone();
                            if ui
                                .add(egui::TextEdit::singleline(&mut name).char_limit(64))
                                .changed()
                            {
                                // Empty names stay a visible invalid draft; no accidental lost keystrokes.
                                parameter.name = name;
                                changed = true;
                            }
                            changed |= edit_value(ui, &mut parameter.value);
                            ui.small(
                                "Editing this input updates every Parameter node that uses it.",
                            );
                        }
                    }
                    MaterialNodeOp::Constant { .. } => {
                        if let Some(MaterialNode {
                            op: MaterialNodeOp::Constant { value },
                            ..
                        }) = draft.nodes.iter_mut().find(|n| n.id == id)
                        {
                            changed |= edit_value(ui, value);
                        }
                    }
                    MaterialNodeOp::BodyPosition => {
                        ui.small("Stable object coordinates in body metres, before ship placement and origin shifts.");
                    }
                    MaterialNodeOp::AuthoredSurface => {
                        ui.small("The original authored base color, roughness, metallic and emission for this primitive. Use it outside your hull section mask to preserve the existing finish.");
                    }
                    MaterialNodeOp::BodyBoxMask { .. } => {
                        ui.small("A bounded hull region in body metres. Min must be below Max on every axis. Positive Feather blends inward from each face; the mask is zero outside the box.");
                        ui.small("Min, Max and Feather use direct typed parameters or constants. This gives the editor a finite, validated region.");
                    }
                    MaterialNodeOp::TransformPosition { .. } => {
                        ui.small("Multiply each position axis by Scale, then add Offset. Smaller scales produce larger pattern features.");
                    }
                    MaterialNodeOp::Noise3D { .. } => {
                        ui.small("Seeded spatial value noise. Seed and noise version are persisted; no animated time input.");
                    }
                    MaterialNodeOp::Remap { .. } => {
                        ui.small("Map one scalar interval to another. Input limits must differ. Use Smoothstep to make a soft mask.");
                    }
                    MaterialNodeOp::Smoothstep { .. } => {
                        ui.small("A smooth 0–1 mask between two increasing edges. Connect the mask to a surface blend.");
                    }
                    MaterialNodeOp::Surface { .. } => {
                        ui.small("A surface bundle: linear base color, roughness, metallic and linear emission color.");
                    }
                    MaterialNodeOp::MixSurface { .. } => {
                        ui.small("Blend the two surface value bundles with a 0–1 mask. This is parameter blending within the existing lighting model.");
                    }
                    MaterialNodeOp::SurfaceOutput { .. } => {
                        ui.small("When Apply hull surface is enabled, this output owns the bound Pioneer material slot. Authored Surface preserves its source values outside the box. Other material slots retain their existing finish.");
                    }
                }
                for (index, (name, ty, source)) in node.op.inputs().into_iter().enumerate() {
                    ui.separator();
                    ui.colored_label(port_color(ty), name);
                    let candidates: Vec<(u32, String)> = draft
                        .nodes
                        .iter()
                        .filter(|candidate| {
                            candidate.id != id && node_output_type(&draft, candidate) == Some(ty)
                        })
                        .map(|candidate| {
                            (
                                candidate.id,
                                format!("{} · {}", candidate.id, node_label(&draft, candidate)),
                            )
                        })
                        .collect();
                    let mut selected = source;
                    let selected_text = candidates
                        .iter()
                        .find(|(candidate, _)| *candidate == source)
                        .map(|(_, label)| label.as_str())
                        .unwrap_or("Unconnected");
                    egui::ComboBox::from_id_salt(("surface_source", id, index))
                        .selected_text(selected_text)
                        .width(212.)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut selected, 0, "Unconnected");
                            for (candidate, label) in candidates {
                                ui.selectable_value(&mut selected, candidate, label);
                            }
                        });
                    if selected != source {
                        if selected == 0 {
                            if let Some(target) = draft.nodes.iter_mut().find(|n| n.id == id) {
                                let _ = target.op.set_input(index, 0);
                                changed = true;
                            }
                        } else {
                            match connect_nodes(&mut draft, selected, id, index) {
                                Ok(()) => changed = true,
                                Err(error) => self.message = error,
                            }
                        }
                    }
                }
                ui.separator();
                if !matches!(node.op, MaterialNodeOp::SurfaceOutput { .. })
                    && ui.button("Delete node").clicked()
                {
                    delete = Some(id);
                }
            }
        } else {
            ui.small("Select a node to edit values or choose input connections.");
        }
        if let Some(id) = delete {
            draft.nodes.retain(|n| n.id != id);
            draft.layout.remove(&id);
            for node in &mut draft.nodes {
                for (index, (_, _, source)) in node.op.inputs().into_iter().enumerate() {
                    if source == id {
                        let _ = node.op.set_input(index, 0);
                    }
                }
            }
            // Keep shared parameters referenced by another node; remove unused authoring values.
            let used: BTreeSet<u32> = draft
                .nodes
                .iter()
                .filter_map(|node| match node.op {
                    MaterialNodeOp::Parameter { parameter } => Some(parameter),
                    _ => None,
                })
                .collect();
            draft.parameters.retain(|p| used.contains(&p.id));
            self.selected = None;
            self.connecting = None;
            changed = true;
        }
        if changed {
            self.apply(draft);
        }
        ui.separator();
        if let Some(revision) = &self.last_good {
            ui.small(format!(
                "Last valid surface revision {}",
                revision.revision()
            ));
        }
        for error in self.diagnostics.iter().take(8) {
            ui.colored_label(egui::Color32::from_rgb(255, 168, 110), error);
        }
        if self.diagnostics.len() > 8 {
            ui.small(format!(
                "{} more graph diagnostics",
                self.diagnostics.len() - 8
            ));
        }
        ui.small(&self.message);
        if let Some(status) = &self.program_status {
            ui.separator();
            ui.label("Active viewport GPU");
            ui.small(format!(
                "{:?} · {} compile attempts",
                status.phase, status.compilation_count
            ));
            if let Some(stamp) = status.applied {
                ui.small(format!("Applied {}:{}", stamp.generation, stamp.revision));
            }
            if let Some(stamp) = status.requested {
                ui.small(format!("Requested {}:{}", stamp.generation, stamp.revision));
            }
            for diagnostic in &status.diagnostics {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 168, 110),
                    format!("{:?}: {}", diagnostic.phase, diagnostic.message),
                );
                if !diagnostic.node_ids.is_empty() {
                    ui.small(format!("Nodes: {:?}", diagnostic.node_ids));
                }
            }
        } else {
            ui.small("GPU status awaits a painted viewport. Each viewport keeps its last linked program if compilation fails.");
        }
    }
}

fn node_position(document: &MaterialGraphDocument, id: u32) -> [f32; 2] {
    document.layout.get(&id).copied().unwrap_or([0., 0.])
}
fn node_height(node: &MaterialNode) -> f32 {
    (70. + node.op.inputs().len() as f32 * 25.).max(84.)
}
fn node_output_type(
    document: &MaterialGraphDocument,
    node: &MaterialNode,
) -> Option<MaterialValueType> {
    use MaterialNodeOp::*;
    match &node.op {
        BodyPosition | TransformPosition { .. } => Some(MaterialValueType::Position),
        Parameter { parameter } => document
            .parameters
            .iter()
            .find(|p| p.id == *parameter)
            .map(|p| p.value.value_type()),
        Constant { value } => Some(value.value_type()),
        Noise3D { .. } | Remap { .. } | Smoothstep { .. } | BodyBoxMask { .. } => {
            Some(MaterialValueType::Scalar)
        }
        Surface { .. } | MixSurface { .. } | AuthoredSurface => Some(MaterialValueType::Surface),
        SurfaceOutput { .. } => None,
    }
}
fn node_label(document: &MaterialGraphDocument, node: &MaterialNode) -> String {
    match node.op {
        MaterialNodeOp::Parameter { parameter } => document
            .parameters
            .iter()
            .find(|p| p.id == parameter)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Missing parameter".into()),
        _ => node.op.name().into(),
    }
}
fn value_summary(document: &MaterialGraphDocument, node: &MaterialNode) -> String {
    let value = match &node.op {
        MaterialNodeOp::Parameter { parameter } => document
            .parameters
            .iter()
            .find(|p| p.id == *parameter)
            .map(|p| &p.value),
        MaterialNodeOp::Constant { value } => Some(value),
        _ => None,
    };
    match value {
        Some(MaterialParameterValue::Scalar(value)) => format!("{value:.3}"),
        Some(MaterialParameterValue::Vector3(value)) => {
            format!("{:.2}, {:.2}, {:.2}", value[0], value[1], value[2])
        }
        Some(MaterialParameterValue::LinearColor(value)) => {
            format!("RGB {:.2}, {:.2}, {:.2}", value[0], value[1], value[2])
        }
        Some(MaterialParameterValue::Seed(value)) => format!("Seed {value}"),
        None => match node.op {
            MaterialNodeOp::BodyPosition => "Body metres".into(),
            MaterialNodeOp::AuthoredSurface => "Authored finish".into(),
            _ => String::new(),
        },
    }
}
fn edit_value(ui: &mut egui::Ui, value: &mut MaterialParameterValue) -> bool {
    match value {
        MaterialParameterValue::Scalar(value) => {
            ui.label("Scalar");
            ui.add(
                egui::DragValue::new(value)
                    .speed(0.01)
                    .range(-MATERIAL_VALUE_LIMIT..=MATERIAL_VALUE_LIMIT),
            )
            .changed()
        }
        MaterialParameterValue::Vector3(value) => {
            ui.label("Vector 3");
            let mut changed = false;
            for (axis, component) in ["X", "Y", "Z"].into_iter().zip(value) {
                ui.horizontal(|ui| {
                    ui.label(axis);
                    changed |= ui
                        .add(
                            egui::DragValue::new(component)
                                .speed(0.01)
                                .range(-MATERIAL_VALUE_LIMIT..=MATERIAL_VALUE_LIMIT),
                        )
                        .changed();
                });
            }
            changed
        }
        MaterialParameterValue::LinearColor(value) => {
            ui.label("Linear RGB");
            // The picker shows the 0–1 gamut; numeric channels retain supported HDR emission.
            let mut picker = value.map(|v| v.clamp(0., 1.));
            let mut changed = ui.color_edit_button_rgb(&mut picker).changed();
            if changed {
                *value = picker;
            }
            for (channel, component) in ["R", "G", "B"].into_iter().zip(value) {
                ui.horizontal(|ui| {
                    ui.label(channel);
                    changed |= ui
                        .add(
                            egui::DragValue::new(component)
                                .speed(0.005)
                                .range(0.0..=4.0),
                        )
                        .changed();
                });
            }
            changed
        }
        MaterialParameterValue::Seed(value) => {
            ui.label("Deterministic seed");
            ui.add(egui::DragValue::new(value).speed(1.)).changed()
        }
    }
}
fn connect_nodes(
    document: &mut MaterialGraphDocument,
    source: u32,
    target: u32,
    input: usize,
) -> Result<(), String> {
    let source_node = document
        .nodes
        .iter()
        .find(|n| n.id == source)
        .ok_or("Source node is missing")?;
    let source_type = node_output_type(document, source_node).ok_or("This node has no output")?;
    let target_node = document
        .nodes
        .iter()
        .find(|n| n.id == target)
        .ok_or("Target node is missing")?;
    let (_, expected, _) = target_node
        .op
        .inputs()
        .get(input)
        .copied()
        .ok_or("Target input is missing")?;
    if source_type != expected {
        return Err(format!(
            "Port types differ: {source_type:?} cannot connect to {expected:?}."
        ));
    }
    // The new edge target→source is a cycle iff source already depends on target.
    let mut pending = vec![source];
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if id == target {
            return Err("Connection would create a cycle.".into());
        }
        if visited.insert(id) {
            if let Some(node) = document.nodes.iter().find(|n| n.id == id) {
                pending.extend(
                    node.op
                        .inputs()
                        .into_iter()
                        .map(|(_, _, id)| id)
                        .filter(|id| *id != 0),
                );
            }
        }
    }
    document
        .nodes
        .iter_mut()
        .find(|n| n.id == target)
        .expect("checked target")
        .op
        .set_input(input, source)
}
fn port_color(ty: MaterialValueType) -> egui::Color32 {
    match ty {
        MaterialValueType::Scalar => egui::Color32::from_rgb(237, 187, 83),
        MaterialValueType::Vector3 => egui::Color32::from_rgb(165, 112, 235),
        MaterialValueType::Position => egui::Color32::from_rgb(109, 211, 230),
        MaterialValueType::LinearColor => egui::Color32::from_rgb(87, 174, 241),
        MaterialValueType::Seed => egui::Color32::from_rgb(224, 151, 226),
        MaterialValueType::Surface => egui::Color32::from_rgb(100, 215, 142),
    }
}
fn draw_wire(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, color: egui::Color32) {
    let bend = ((b.x - a.x).abs() * 0.5).max(36.);
    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
        [a, a + egui::vec2(bend, 0.), b - egui::vec2(bend, 0.), b],
        false,
        egui::Color32::TRANSPARENT,
        egui::Stroke::new(2., color),
    ));
}

fn validate_document(document: &MaterialGraphDocument) -> Result<(), String> {
    // Disk admission uses the same lowering budgets as live revisions.
    lower_material(document).map(|_| ()).map_err(|errors| {
        errors
            .into_iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join("\n")
    })
}
fn ordinary_directory(path: &Path) -> Result<(), String> {
    let meta =
        fs::symlink_metadata(path).map_err(|e| format!("Cannot inspect surface directory: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("Surface directories must be ordinary directories, not symbolic links.".into());
    }
    Ok(())
}
fn file_path(project: &Path, create: bool) -> Result<PathBuf, String> {
    ordinary_directory(project)?;
    let assets = project.join("Assets");
    if create {
        match fs::create_dir(&assets) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("Cannot create Assets directory: {e}")),
        }
    }
    ordinary_directory(&assets)?;
    let path = assets.join(FILE_NAME);
    match fs::symlink_metadata(&path) {
        Ok(meta) if !meta.is_file() || meta.file_type().is_symlink() => {
            return Err("Surface destination must be an ordinary file, not a symbolic link.".into())
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Cannot inspect surface file: {e}")),
    }
    Ok(path)
}
fn load_optional(project: &Path) -> Result<Option<MaterialGraphDocument>, String> {
    // A missing project is an error; an ordinary project without Assets is a new draft.
    ordinary_directory(project)?;
    let assets = project.join("Assets");
    match fs::symlink_metadata(&assets) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot inspect surface path: {e}")),
        Ok(_) => ordinary_directory(&assets)?,
    }
    match fs::symlink_metadata(assets.join(FILE_NAME)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot inspect surface path: {e}")),
        Ok(_) => {}
    }
    load(project).map(Some)
}
fn load(project: &Path) -> Result<MaterialGraphDocument, String> {
    let file = fs::File::open(file_path(project, false)?)
        .map_err(|e| format!("Cannot open hull surface: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read hull surface: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Hull surface exceeds 128 KiB.".into());
    }
    let document: MaterialGraphDocument =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid hull surface JSON: {e}"))?;
    validate_document(&document)?;
    Ok(document)
}
fn save(project: &Path, document: &MaterialGraphDocument) -> Result<(), String> {
    validate_document(document)?;
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|e| format!("Cannot encode hull surface: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Hull surface exceeds 128 KiB.".into());
    }
    let path = file_path(project, true)?;
    let parent = path.parent().ok_or("Surface parent unavailable")?;
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".Pioneer.surface.{}.{}.tmp",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = match options.open(&temp) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot create temporary hull surface: {e}")),
        };
        let written = file
            .write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = written.and_then(|_| fs::rename(&temp, &path)) {
            let _ = fs::remove_file(&temp);
            return Err(format!("Cannot save hull surface: {error}"));
        }
        #[cfg(unix)]
        {
            if let Ok(directory) = fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        return Ok(());
    }
    Err("Cannot allocate a unique temporary hull surface file.".into())
}
