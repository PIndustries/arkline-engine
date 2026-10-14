//! Candidate utility-route authoring. These paths are local design data only;
//! they do not describe installed ship systems or affect simulation behavior.
use crate::engineering_document::{Component, HALF_EXTENT};
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread,
};

const MAX_BYTES: u64 = 64 * 1024;
const MAX_PATHS: usize = 128;
const MAX_WAYPOINTS: usize = 128;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IoOperation {
    Save,
    Reload,
}

enum IoRequest {
    Save(PathBuf, ServicePathDocument, Vec<Component>),
    Reload(PathBuf),
}

struct IoResult {
    operation: IoOperation,
    result: Result<ServicePathDocument, String>,
}

#[derive(Debug, Clone, Copy)]
struct PendingOperation {
    kind: IoOperation,
    draft_revision: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Power,
    Data,
    Water,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePath {
    pub id: u64,
    pub name: String,
    pub kind: ServiceKind,
    pub source_component: u64,
    pub target_component: u64,
    /// Candidate polyline coordinates in the engineering envelope frame.
    pub waypoints_m: Vec<[f64; 3]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServicePathDocument {
    pub schema_version: u32,
    pub next_id: u64,
    pub paths: Vec<ServicePath>,
}

impl Default for ServicePathDocument {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_id: 1,
            paths: Vec::new(),
        }
    }
}

impl ServicePathDocument {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.paths.len() > MAX_PATHS || self.next_id == 0 {
            return Err(
                "Service path schema must be 1, next_id positive, and contain at most 128 paths"
                    .into(),
            );
        }
        let mut ids = HashSet::new();
        for path in &self.paths {
            if path.id == 0 || path.id >= self.next_id || !ids.insert(path.id) {
                return Err("Service path IDs must be unique, positive, and below next_id".into());
            }
            if path.name.trim().is_empty() || path.name.len() > 80 {
                return Err(format!(
                    "Path #{} needs a name of at most 80 bytes",
                    path.id
                ));
            }
            if path.source_component == 0
                || path.target_component == 0
                || path.source_component == path.target_component
            {
                return Err(format!(
                    "Path #{} needs two distinct component endpoints",
                    path.id
                ));
            }
            if path.waypoints_m.len() < 2 || path.waypoints_m.len() > MAX_WAYPOINTS {
                return Err(format!("Path #{} needs 2 to 128 waypoints", path.id));
            }
            for point in &path.waypoints_m {
                for axis in 0..3 {
                    if !point[axis].is_finite() || point[axis].abs() > HALF_EXTENT[axis] {
                        return Err(format!(
                            "Path #{} waypoint must be finite and inside the 72 × 18 × 38 m envelope",
                            path.id
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn validate_for(&self, components: &[Component]) -> Result<(), String> {
        self.validate()?;
        let component_ids: HashSet<_> = components.iter().map(|component| component.id).collect();
        for path in &self.paths {
            if !component_ids.contains(&path.source_component) {
                return Err(format!(
                    "Path #{} references missing source component #{}",
                    path.id, path.source_component
                ));
            }
            if !component_ids.contains(&path.target_component) {
                return Err(format!(
                    "Path #{} references missing target component #{}",
                    path.id, path.target_component
                ));
            }
        }
        Ok(())
    }

    fn add_path(&mut self, components: &[Component]) -> Result<u64, String> {
        if self.paths.len() >= MAX_PATHS {
            return Err("Maximum 128 candidate service paths".into());
        }
        let mut ids = components.iter().map(|component| component.id);
        let source = ids
            .next()
            .ok_or("Add at least two engineering components first")?;
        let target = ids
            .find(|id| *id != source)
            .ok_or("Add at least two engineering components first")?;
        let id = self.next_id;
        let next_id = id.checked_add(1).ok_or("Service path ID space exhausted")?;
        self.paths.push(ServicePath {
            id,
            name: format!("Candidate service path {id}"),
            kind: ServiceKind::Power,
            source_component: source,
            target_component: target,
            // No route geometry is invented. The author adds endpoint and
            // intermediate waypoint coordinates explicitly below.
            waypoints_m: Vec::new(),
        });
        self.next_id = next_id;
        Ok(id)
    }
}

/// Project-local authoring state. Draft edits remain in memory until saved.
pub struct ServicePaths {
    draft: ServicePathDocument,
    saved: ServicePathDocument,
    selected: Option<u64>,
    message: String,
    io_requests: Option<SyncSender<IoRequest>>,
    io_results: Receiver<IoResult>,
    pending: Option<PendingOperation>,
    draft_revision: u64,
    load_blocked: bool,
    explicit_reset: bool,
}

impl ServicePaths {
    pub fn open(project: &Path) -> Self {
        let (request_tx, request_rx) = mpsc::sync_channel::<IoRequest>(1);
        let (result_tx, result_rx) = mpsc::sync_channel::<IoResult>(1);
        let worker = thread::Builder::new()
            .name("arkline-service-path-io".into())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let (operation, result) = match request {
                        IoRequest::Save(project, document, components) => (
                            IoOperation::Save,
                            save(&project, &document, &components).map(|()| document),
                        ),
                        IoRequest::Reload(project) => (IoOperation::Reload, load(&project)),
                    };
                    if result_tx.send(IoResult { operation, result }).is_err() {
                        break;
                    }
                }
            });
        let mut panel = Self {
            draft: ServicePathDocument::default(),
            saved: ServicePathDocument::default(),
            selected: None,
            message: "Loading saved service paths…".into(),
            io_requests: worker.ok().map(|_| request_tx),
            io_results: result_rx,
            pending: None,
            draft_revision: 0,
            load_blocked: true,
            explicit_reset: false,
        };
        if panel.io_requests.is_some() {
            panel.queue(
                IoOperation::Reload,
                IoRequest::Reload(project.to_path_buf()),
            );
        } else {
            panel.message = "Could not start service path I/O worker. Reset explicitly before replacing any saved file.".into();
        }
        panel
    }

    fn queue(&mut self, operation: IoOperation, request: IoRequest) {
        if self.pending.is_some() {
            self.message = "A service path file operation is already pending.".into();
            return;
        }
        let Some(sender) = &self.io_requests else {
            self.load_blocked = true;
            self.message = "Service path I/O worker is unavailable; saving is locked.".into();
            return;
        };
        match sender.try_send(request) {
            Ok(()) => {
                self.pending = Some(PendingOperation {
                    kind: operation,
                    draft_revision: self.draft_revision,
                });
                self.message.clear();
            }
            Err(TrySendError::Full(_)) => {
                self.message = "Service path I/O queue is full; wait and retry.".into();
            }
            Err(TrySendError::Disconnected(_)) => {
                self.load_blocked = true;
                self.message = "Service path I/O worker stopped; saving is locked.".into();
            }
        }
    }

    fn poll_io(&mut self) {
        if self.pending.is_none() {
            return;
        }
        let pending = self.pending.unwrap();
        match self.io_results.try_recv() {
            Ok(IoResult { operation, result }) => {
                self.pending = None;
                if operation != pending.kind {
                    self.load_blocked = true;
                    self.message = "Service path I/O result did not match its pending operation; saving is locked.".into();
                    return;
                }
                match (operation, result) {
                    (IoOperation::Save, Ok(document)) => {
                        self.saved = document;
                        self.explicit_reset = false;
                        self.load_blocked = false;
                        self.message = if self.draft_revision == pending.draft_revision {
                            "Saved Assets/Pioneer.service_paths.json".into()
                        } else {
                            "Saved the submitted snapshot; newer draft edits remain unsaved.".into()
                        };
                    }
                    (IoOperation::Reload, Ok(document)) => {
                        if self.draft_revision == pending.draft_revision {
                            self.draft = document.clone();
                            self.selected = None;
                            self.message = "Reloaded saved service paths.".into();
                        } else {
                            self.message = "Reload finished, but the draft changed while it was pending; local edits were kept. Retry reload to replace them.".into();
                        }
                        self.saved = document;
                        self.explicit_reset = false;
                        self.load_blocked = false;
                    }
                    (IoOperation::Save, Err(error)) => {
                        self.message = format!("Save failed: {error}");
                    }
                    (IoOperation::Reload, Err(error)) => {
                        self.load_blocked = true;
                        self.explicit_reset = false;
                        self.message = format!(
                            "Saved service paths could not be loaded: {error}. Saving is locked until reload succeeds or you explicitly reset."
                        );
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.load_blocked = true;
                self.message = "Service path I/O worker stopped; saving is locked.".into();
            }
        }
    }

    fn reset_after_load_failure(&mut self) {
        self.draft = ServicePathDocument::default();
        self.saved = ServicePathDocument::default();
        self.selected = None;
        self.load_blocked = false;
        self.explicit_reset = true;
        self.message =
            "Explicit reset confirmed. Saving may replace the unreadable saved file.".into();
    }

    fn can_save(&self, components: &[Component]) -> bool {
        self.pending.is_none() && !self.load_blocked && self.draft.validate_for(components).is_ok()
    }

    fn record_draft_revision(&mut self, before: &ServicePathDocument) {
        if before != &self.draft {
            self.draft_revision = self.draft_revision.saturating_add(1);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path, components: &[Component]) {
        self.poll_io();
        let frame_draft = self.draft.clone();
        ui.heading("Service paths");
        ui.label("CANDIDATE DESIGN · local authoring only");
        ui.small("Power, data, and water paths are design proposals. They do not represent installed or simulated utilities.");
        ui.small("Waypoint coordinates use the candidate engineering envelope: X ±36 m, Y ±9 m, Z ±19 m.");

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.pending.is_none() && components.len() >= 2,
                    egui::Button::new("Add candidate path"),
                )
                .clicked()
            {
                match self.draft.add_path(components) {
                    Ok(id) => {
                        self.selected = Some(id);
                        self.message =
                            "Added an empty candidate route; author at least two waypoints.".into();
                    }
                    Err(error) => self.message = error,
                }
            }
            if ui
                .add_enabled(self.can_save(components), egui::Button::new("Save draft"))
                .clicked()
            {
                self.queue(
                    IoOperation::Save,
                    IoRequest::Save(
                        project.to_path_buf(),
                        self.draft.clone(),
                        components.to_vec(),
                    ),
                );
            }
            if ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Reload saved"))
                .clicked()
            {
                self.queue(
                    IoOperation::Reload,
                    IoRequest::Reload(project.to_path_buf()),
                );
            }
            if self.load_blocked
                && ui
                    .add_enabled(
                        self.pending.is_none(),
                        egui::Button::new("Reset draft; allow file replacement"),
                    )
                    .clicked()
            {
                self.reset_after_load_failure();
            }
            if let Some(operation) = self.pending {
                ui.small(match operation.kind {
                    IoOperation::Save => "Saving service paths…",
                    IoOperation::Reload => "Loading service paths…",
                });
            }
        });

        if self.load_blocked {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "Saving is locked because the saved file has not loaded successfully.",
            );
        }
        if let Err(error) = self.draft.validate_for(components) {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.label(if !self.explicit_reset && self.draft == self.saved {
            "No unsaved service path changes"
        } else {
            "Unsaved service path changes"
        });
        ui.separator();

        egui::ScrollArea::vertical()
            .id_salt("service_path_entries")
            .auto_shrink([false, true])
            .max_height(120.0)
            .show(ui, |ui| {
                for path in &self.draft.paths {
                    ui.push_id(path.id, |ui| {
                        if ui
                            .selectable_label(
                                self.selected == Some(path.id),
                                format!("#{} · {} · {:?}", path.id, path.name, path.kind),
                            )
                            .clicked()
                        {
                            self.selected = Some(path.id);
                        }
                    });
                }
            });

        let Some(path_id) = self
            .selected
            .filter(|id| self.draft.paths.iter().any(|path| path.id == *id))
        else {
            if self.draft.paths.is_empty() {
                ui.label("No candidate paths yet.");
            }
            ui.label(&self.message);
            self.record_draft_revision(&frame_draft);
            return;
        };

        let mut remove_path = false;
        let mut path_error: Option<String> = None;
        let path = self
            .draft
            .paths
            .iter_mut()
            .find(|path| path.id == path_id)
            .unwrap();
        ui.separator();
        ui.label(format!("Path #{}", path.id));
        ui.horizontal_wrapped(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut path.name);
            egui::ComboBox::from_id_salt(("service_kind", path.id))
                .selected_text(format!("{:?}", path.kind))
                .show_ui(ui, |ui| {
                    for kind in [ServiceKind::Power, ServiceKind::Data, ServiceKind::Water] {
                        ui.selectable_value(&mut path.kind, kind, format!("{kind:?}"));
                    }
                });
        });
        choose_component(
            ui,
            "Source component",
            path.id,
            true,
            &mut path.source_component,
            components,
        );
        choose_component(
            ui,
            "Target component",
            path.id,
            false,
            &mut path.target_component,
            components,
        );
        ui.small("Endpoint IDs resolve against the current Engineering draft. Save Engineering too before sharing or reopening so its component IDs remain available.");
        ui.small("Waypoints below are separately authored route geometry.");

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.pending.is_none(),
                    egui::Button::new("Add source-center waypoint"),
                )
                .clicked()
                && path.waypoints_m.len() < MAX_WAYPOINTS
            {
                if let Some(component) = components.iter().find(|c| c.id == path.source_component) {
                    path.waypoints_m.push(component.center_m);
                } else {
                    path_error =
                        Some("Choose a valid source component before adding its waypoint.".into());
                }
            }
            if ui
                .add_enabled(
                    self.pending.is_none(),
                    egui::Button::new("Add destination-center waypoint"),
                )
                .clicked()
                && path.waypoints_m.len() < MAX_WAYPOINTS
            {
                if let Some(component) = components.iter().find(|c| c.id == path.target_component) {
                    path.waypoints_m.push(component.center_m);
                } else {
                    path_error = Some(
                        "Choose a valid destination component before adding its waypoint.".into(),
                    );
                }
            }
            if ui
                .add_enabled(
                    self.pending.is_none() && !path.waypoints_m.is_empty(),
                    egui::Button::new("Remove last waypoint"),
                )
                .clicked()
            {
                path.waypoints_m.pop();
            }
        });
        for (index, point) in path.waypoints_m.iter_mut().enumerate() {
            ui.push_id(("service_waypoint", path.id, index), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Waypoint {} · m", index + 1));
                    for (axis, value) in point.iter_mut().enumerate() {
                        ui.label(["X", "Y", "Z"][axis]);
                        ui.add(egui::DragValue::new(value).speed(0.1));
                    }
                });
            });
        }
        if ui
            .add_enabled(
                self.pending.is_none(),
                egui::Button::new("Remove candidate path"),
            )
            .clicked()
        {
            remove_path = true;
        }
        // End the mutable borrow of the selected path before validating the
        // whole document or mutating the path collection below.
        let preview_path = path.clone();
        let current_validity = self.draft.validate_for(components);
        if let Some(error) = path_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Err(error) = current_validity {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if !remove_path {
            route_preview(ui, &preview_path, components);
        }
        if remove_path {
            self.draft.paths.retain(|path| path.id != path_id);
            self.selected = None;
        }
        ui.label(&self.message);
        self.record_draft_revision(&frame_draft);
    }
}

fn choose_component(
    ui: &mut egui::Ui,
    label: &str,
    path_id: u64,
    source: bool,
    selected: &mut u64,
    components: &[Component],
) {
    let role = if source {
        "source_component"
    } else {
        "target_component"
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(label);
        let selected_label = components
            .iter()
            .find(|component| component.id == *selected)
            .map_or_else(
                || format!("Missing component #{}", *selected),
                |component| format!("#{} · {}", component.id, component.name),
            );
        egui::ComboBox::from_id_salt((role, path_id))
            .selected_text(selected_label)
            .show_ui(ui, |ui| {
                for component in components {
                    ui.selectable_value(
                        selected,
                        component.id,
                        format!("#{} · {}", component.id, component.name),
                    );
                }
            });
    });
}

#[derive(Clone, Copy)]
enum Projection {
    TopDown,
    Side,
}

fn route_preview(ui: &mut egui::Ui, path: &ServicePath, components: &[Component]) {
    ui.separator();
    ui.strong(format!("Selected route · {}", path.name));
    ui.small("Two-dimensional design projections only; no 3D gizmo, installation, or utility simulation.");
    let source = components
        .iter()
        .find(|component| component.id == path.source_component);
    let target = components
        .iter()
        .find(|component| component.id == path.target_component);
    if let Some(component) = source {
        ui.small(format!("Source #{} · {}", component.id, component.name));
    } else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            format!("Missing source component #{}", path.source_component),
        );
    }
    if let Some(component) = target {
        ui.small(format!("Target #{} · {}", component.id, component.name));
    } else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            format!("Missing target component #{}", path.target_component),
        );
    }
    draw_projection(ui, path, source, target, Projection::TopDown);
    draw_projection(ui, path, source, target, Projection::Side);
}

fn draw_projection(
    ui: &mut egui::Ui,
    path: &ServicePath,
    source: Option<&Component>,
    target: Option<&Component>,
    projection: Projection,
) {
    let (caption, vertical_half, vertical_axis) = match projection {
        Projection::TopDown => ("Top-down · X/Z (m)", HALF_EXTENT[2], 2),
        Projection::Side => ("Side · X/Y (m)", HALF_EXTENT[1], 1),
    };
    ui.label(caption);
    let width = ui.available_width().clamp(120.0, 540.0);
    // Keep both full-width projections compact enough that the selected route
    // remains visible in the editor's narrow right inspector.
    let height = (width * vertical_half as f32 / HALF_EXTENT[0] as f32).clamp(52.0, 78.0);
    let (outer, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, 2.0, egui::Color32::from_rgb(8, 24, 19));
    painter.rect_stroke(
        outer,
        2.0,
        egui::Stroke::new(1.0, egui::Color32::from_gray(65)),
        egui::StrokeKind::Inside,
    );
    let plot = outer.shrink2(egui::vec2(27.0, 16.0));
    let map = |point: [f64; 3]| {
        let x = ((point[0] + HALF_EXTENT[0]) / (2.0 * HALF_EXTENT[0])) as f32;
        let vertical = ((point[vertical_axis] + vertical_half) / (2.0 * vertical_half)) as f32;
        egui::pos2(
            plot.left() + x * plot.width(),
            plot.bottom() - vertical * plot.height(),
        )
    };
    let grid = egui::Color32::from_rgb(27, 52, 43);
    for tick in [-36.0, 0.0, 36.0] {
        let point = map([tick, 0.0, 0.0]);
        painter.line_segment(
            [
                egui::pos2(point.x, plot.top()),
                egui::pos2(point.x, plot.bottom()),
            ],
            egui::Stroke::new(1.0, grid),
        );
        painter.text(
            egui::pos2(point.x, outer.bottom() - 7.0),
            egui::Align2::CENTER_CENTER,
            format!("{tick:.0}"),
            egui::FontId::monospace(8.0),
            egui::Color32::from_gray(155),
        );
    }
    for tick in [-vertical_half, 0.0, vertical_half] {
        let value = match projection {
            Projection::TopDown => [0.0, 0.0, tick],
            Projection::Side => [0.0, tick, 0.0],
        };
        let point = map(value);
        painter.line_segment(
            [
                egui::pos2(plot.left(), point.y),
                egui::pos2(plot.right(), point.y),
            ],
            egui::Stroke::new(1.0, grid),
        );
        painter.text(
            egui::pos2(outer.left() + 13.0, point.y),
            egui::Align2::CENTER_CENTER,
            format!("{tick:.0}"),
            egui::FontId::monospace(8.0),
            egui::Color32::from_gray(155),
        );
    }

    if path.waypoints_m.len() >= 2 {
        for points in path.waypoints_m.windows(2) {
            painter.line_segment(
                [map(points[0]), map(points[1])],
                egui::Stroke::new(2.0, egui::Color32::from_rgb(100, 230, 166)),
            );
        }
    }
    for (index, point) in path.waypoints_m.iter().enumerate() {
        let center = map(*point);
        painter.circle_filled(center, 4.0, egui::Color32::from_rgb(255, 202, 89));
        painter.text(
            center + egui::vec2(0.0, -8.0),
            egui::Align2::CENTER_BOTTOM,
            (index + 1).to_string(),
            egui::FontId::monospace(8.0),
            egui::Color32::WHITE,
        );
    }
    for (component, label, color) in [
        (
            source,
            format!("S#{}", path.source_component),
            egui::Color32::LIGHT_GREEN,
        ),
        (
            target,
            format!("T#{}", path.target_component),
            egui::Color32::LIGHT_RED,
        ),
    ] {
        if let Some(component) = component {
            let center = map(component.center_m);
            painter.circle_filled(center, 5.0, color);
            painter.text(
                center + egui::vec2(6.0, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::monospace(8.0),
                color,
            );
        }
    }
}

fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect service path directory: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(
            "Service path directories must be ordinary directories, not symbolic links".into(),
        );
    }
    Ok(())
}

fn document_path(project: &Path, create_assets: bool) -> Result<PathBuf, String> {
    ordinary_directory(project)?;
    let assets = project.join("Assets");
    if create_assets {
        match fs::create_dir(&assets) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("Cannot create Assets directory: {error}")),
        }
    } else if fs::symlink_metadata(&assets)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(assets.join("Pioneer.service_paths.json"));
    }
    ordinary_directory(&assets)?;
    let path = assets.join("Pioneer.service_paths.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(
                "Service path destination must be an ordinary file, not a symbolic link".into(),
            );
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot inspect service path file: {error}")),
    }
    Ok(path)
}

pub fn load(project: &Path) -> Result<ServicePathDocument, String> {
    let path = document_path(project, false)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(ServicePathDocument::default());
        }
        Err(error) => return Err(format!("Cannot open service paths: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read service paths: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Service path document exceeds 64 KiB".into());
    }
    let document: ServicePathDocument =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid service path JSON: {e}"))?;
    document.validate()?;
    Ok(document)
}

pub fn save(
    project: &Path,
    document: &ServicePathDocument,
    components: &[Component],
) -> Result<(), String> {
    document.validate_for(components)?;
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|e| format!("Cannot encode service paths: {e}"))?;
    if bytes.len() as u64 + 1 > MAX_BYTES {
        return Err("Service path document exceeds 64 KiB".into());
    }
    let path = document_path(project, true)?;
    let parent = path.parent().ok_or("Service path parent unavailable")?;
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".Pioneer.service_paths.{}.{}.tmp",
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
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create service path temporary: {error}")),
        };
        let written = file
            .write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = written.and_then(|_| fs::rename(&temp, &path)) {
            let _ = fs::remove_file(&temp);
            return Err(format!("Cannot save service paths: {error}"));
        }
        #[cfg(unix)]
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        return Ok(());
    }
    Err("Cannot allocate a unique service path temporary file".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Project(PathBuf);
    impl Project {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "arkline-service-paths-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn components() -> Vec<Component> {
        let mut document = crate::engineering_document::EngineeringDocument::default();
        document.add_component().unwrap();
        document.add_component().unwrap();
        document.components
    }

    fn document(components: &[Component]) -> ServicePathDocument {
        ServicePathDocument {
            schema_version: 1,
            next_id: 2,
            paths: vec![ServicePath {
                id: 1,
                name: "Power trunk proposal".into(),
                kind: ServiceKind::Power,
                source_component: components[0].id,
                target_component: components[1].id,
                waypoints_m: [[0., 0., 0.], [1., 1., 1.]].into(),
            }],
        }
    }

    fn panel_with_result_sender() -> (ServicePaths, SyncSender<IoResult>) {
        let (request_tx, _request_rx) = mpsc::sync_channel(1);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        (
            ServicePaths {
                draft: ServicePathDocument::default(),
                saved: ServicePathDocument::default(),
                selected: None,
                message: String::new(),
                io_requests: Some(request_tx),
                io_results: result_rx,
                pending: None,
                draft_revision: 0,
                load_blocked: false,
                explicit_reset: false,
            },
            result_tx,
        )
    }

    #[test]
    fn candidate_path_json_round_trips_through_atomic_project_storage() {
        let project = Project::new();
        let components = components();
        let doc = document(&components);
        save(&project.0, &doc, &components).unwrap();
        assert_eq!(load(&project.0).unwrap(), doc);
    }

    #[test]
    fn validates_waypoint_count_finiteness_and_envelope() {
        let components = components();
        let mut doc = document(&components);
        doc.paths[0].waypoints_m.pop();
        assert!(doc.validate_for(&components).is_err());
        doc = document(&components);
        doc.paths[0].waypoints_m[1][1] = f64::NAN;
        assert!(doc.validate_for(&components).is_err());
        doc.paths[0].waypoints_m[1] = [0., 10., 0.];
        assert!(doc.validate_for(&components).is_err());
        doc.paths[0].waypoints_m = vec![[0., 0., 0.]; MAX_WAYPOINTS + 1];
        assert!(doc.validate_for(&components).is_err());
    }

    #[test]
    fn rejects_unresolved_component_endpoints() {
        let components = components();
        let mut doc = document(&components);
        doc.paths[0].target_component = u64::MAX;
        assert!(doc
            .validate_for(&components)
            .unwrap_err()
            .contains("missing target component"));
    }

    #[test]
    fn save_completion_keeps_edits_made_after_submission_unsaved() {
        let components = components();
        let submitted = document(&components);
        let (mut panel, result_tx) = panel_with_result_sender();
        panel.draft = submitted.clone();
        panel.saved = submitted.clone();
        panel.draft_revision = 3;
        panel.pending = Some(PendingOperation {
            kind: IoOperation::Save,
            draft_revision: 3,
        });
        panel.draft.paths[0].name = "Edited while save was pending".into();
        panel.draft_revision = 4;
        result_tx
            .send(IoResult {
                operation: IoOperation::Save,
                result: Ok(submitted.clone()),
            })
            .unwrap();

        panel.poll_io();

        assert_eq!(panel.saved, submitted);
        assert_eq!(panel.draft.paths[0].name, "Edited while save was pending");
        assert!(panel.message.contains("newer draft edits remain unsaved"));
        assert!(panel.can_save(&components));
    }

    #[test]
    fn reload_completion_keeps_edits_made_after_reload_started() {
        let components = components();
        let loaded = document(&components);
        let mut local_edit = loaded.clone();
        local_edit.paths[0].name = "Local edit during reload".into();
        let (mut panel, result_tx) = panel_with_result_sender();
        panel.draft = loaded.clone();
        panel.saved = loaded.clone();
        panel.draft_revision = 7;
        panel.pending = Some(PendingOperation {
            kind: IoOperation::Reload,
            draft_revision: 7,
        });
        panel.draft = local_edit.clone();
        panel.draft_revision = 8;
        result_tx
            .send(IoResult {
                operation: IoOperation::Reload,
                result: Ok(loaded.clone()),
            })
            .unwrap();

        panel.poll_io();

        assert_eq!(panel.draft, local_edit);
        assert_eq!(panel.saved, loaded);
        assert!(panel.message.contains("local edits were kept"));
        assert!(panel.can_save(&components));
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_assets_and_destination_without_changing_target() {
        use std::os::unix::fs::symlink;

        let project = Project::new();
        let outside = Project::new();
        let components = components();
        let doc = document(&components);
        symlink(&outside.0, project.0.join("Assets")).unwrap();
        assert!(save(&project.0, &doc, &components).is_err());
        assert!(load(&project.0).is_err());

        fs::remove_file(project.0.join("Assets")).unwrap();
        fs::create_dir(project.0.join("Assets")).unwrap();
        let outside_file = outside.0.join("keep.json");
        fs::write(&outside_file, b"preserve me").unwrap();
        symlink(
            &outside_file,
            project.0.join("Assets/Pioneer.service_paths.json"),
        )
        .unwrap();
        assert!(save(&project.0, &doc, &components).is_err());
        assert!(load(&project.0).is_err());
        assert_eq!(fs::read(outside_file).unwrap(), b"preserve me");
    }

    #[test]
    fn unreadable_saved_file_blocks_save_until_explicit_reset() {
        let project = Project::new();
        let assets = project.0.join("Assets");
        fs::create_dir(&assets).unwrap();
        let saved_path = assets.join("Pioneer.service_paths.json");
        fs::write(&saved_path, b"not json").unwrap();

        let mut panel = ServicePaths::open(&project.0);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while panel.pending.is_some() && std::time::Instant::now() < deadline {
            panel.poll_io();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panel.poll_io();
        assert!(panel.pending.is_none(), "initial load did not complete");
        assert!(panel.load_blocked);
        assert!(!panel.can_save(&[]));
        assert_eq!(fs::read(&saved_path).unwrap(), b"not json");

        panel.reset_after_load_failure();
        assert!(panel.explicit_reset);
        assert!(panel.can_save(&[]));
        assert_eq!(fs::read(&saved_path).unwrap(), b"not json");
    }
}
