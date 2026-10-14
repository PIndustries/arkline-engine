//! Local candidate room-layout authoring. This document is not consumed by the
//! running interior renderer, collision model, equipment controls, or server.
use arkline_protocol::{pioneer_rooms, PIONEER_CONNECTED_ROOM, PIONEER_INTERIOR_SIZE};
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
const MAX_ROOMS: usize = 24;
const ROOM_LENGTH_M: f64 = 8.9333333333;
const ROOM_DEPTH_M: f64 = 5.4;
const ROOM_CLEAR_HEIGHT_M: f64 = 3.2;
const DOOR_WIDTH_M: f64 = 1.4;
const DOOR_HEIGHT_M: f64 = 2.4;
const DOOR_WALL_TOLERANCE_M: f64 = 0.25;
const RUNTIME_SOURCE: &str = "arkline-protocol::pioneer_interior";
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IoOperation {
    Save,
    Reload,
}

enum IoRequest {
    Save(PathBuf, RoomLayoutDocument),
    Reload(PathBuf),
}

struct IoResult {
    operation: IoOperation,
    result: Result<RoomLayoutDocument, String>,
}

#[derive(Debug, Clone, Copy)]
struct PendingOperation {
    kind: IoOperation,
    draft_revision: u64,
}

/// `source_room_id` uses the existing proposed room-plan IDs (D1-P1, etc.).
/// `id` is this document's stable local object ID; added rooms have no source ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomLayout {
    pub id: u64,
    pub source_room_id: Option<String>,
    /// Current protocol room identifier, when the runtime defines one.
    pub runtime_room_id: Option<String>,
    pub source_role: Option<String>,
    pub name: String,
    pub deck: u8,
    /// Deck-plan origin, meters from the current interior instance origin.
    pub origin_xz_m: [f64; 2],
    /// Candidate clear length, depth, and height in meters.
    pub clear_lwh_m: [f64; 3],
    /// Door center in deck-plan X/Z meters, matching the current runtime frame.
    pub door_center_xz_m: [f64; 2],
    pub door_clear_wh_m: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoomLayoutDocument {
    pub schema_version: u32,
    pub next_id: u64,
    pub rooms: Vec<RoomLayout>,
}

impl RoomLayoutDocument {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.next_id == 0 || self.rooms.len() > MAX_ROOMS {
            return Err(
                "Room layout schema must be 1, next_id positive, and contain at most 24 rooms"
                    .into(),
            );
        }
        let mut ids = HashSet::new();
        let mut source_ids = HashSet::new();
        for room in &self.rooms {
            if room.id == 0 || room.id >= self.next_id || !ids.insert(room.id) {
                return Err("Room IDs must be unique, positive, and below next_id".into());
            }
            if let Some(source_id) = &room.source_room_id {
                if source_id.trim().is_empty()
                    || source_id.len() > 32
                    || !source_ids.insert(source_id)
                {
                    return Err(format!(
                        "Room #{} has an invalid or duplicate source room ID",
                        room.id
                    ));
                }
            }
            if room
                .runtime_room_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty() || id.len() > 80)
            {
                return Err(format!(
                    "Room #{} has an invalid runtime room ID reference",
                    room.id
                ));
            }
            if room.name.trim().is_empty() || room.name.len() > 80 {
                return Err(format!(
                    "Room #{} needs a name of at most 80 bytes",
                    room.id
                ));
            }
            if room
                .source_role
                .as_ref()
                .is_some_and(|role| role.len() > 80)
            {
                return Err(format!("Room #{} source role exceeds 80 bytes", room.id));
            }
            if !(1..=2).contains(&room.deck) {
                return Err(format!("Room #{} deck must be 1 or 2", room.id));
            }
            let finite = room
                .origin_xz_m
                .iter()
                .chain(room.clear_lwh_m.iter())
                .chain(room.door_center_xz_m.iter())
                .chain(room.door_clear_wh_m.iter())
                .all(|v| v.is_finite());
            if !finite
                || room.clear_lwh_m.iter().any(|v| *v <= 0.0)
                || room.door_clear_wh_m.iter().any(|v| *v <= 0.0)
            {
                return Err(format!(
                    "Room #{} dimensions and positions must be finite; dimensions must be positive",
                    room.id
                ));
            }
            let [origin_x, origin_z] = room.origin_xz_m;
            let [length, depth, height] = room.clear_lwh_m;
            if origin_x < 0.0
                || origin_z < 0.0
                || origin_x + length > PIONEER_INTERIOR_SIZE[0]
                || origin_z + depth > PIONEER_INTERIOR_SIZE[1]
                || height > 12.0
            {
                return Err(format!(
                    "Room #{} clear box must fit the 32 × 14 m deck plan and remain at most 12 m high",
                    room.id
                ));
            }
            let [door_x, door_z] = room.door_center_xz_m;
            let [door_width, door_height] = room.door_clear_wh_m;
            let at_deck_boundary = (door_z - origin_z).abs() <= DOOR_WALL_TOLERANCE_M
                || (door_z - (origin_z + depth)).abs() <= DOOR_WALL_TOLERANCE_M;
            if door_x - door_width / 2.0 < origin_x
                || door_x + door_width / 2.0 > origin_x + length
                || !at_deck_boundary
                || door_height > height
            {
                return Err(format!(
                    "Room #{} door opening must fit the room width/height and lie on a front or rear room edge",
                    room.id
                ));
            }
            if self.rooms.iter().any(|other| {
                other.id != room.id
                    && other.deck == room.deck
                    && origin_x < other.origin_xz_m[0] + other.clear_lwh_m[0]
                    && other.origin_xz_m[0] < origin_x + length
                    && origin_z < other.origin_xz_m[1] + other.clear_lwh_m[1]
                    && other.origin_xz_m[1] < origin_z + depth
            }) {
                return Err(format!(
                    "Room #{} clear box overlaps another room on deck {}",
                    room.id, room.deck
                ));
            }
        }
        Ok(())
    }

    fn add_room(&mut self) -> Result<u64, String> {
        if self.rooms.len() >= MAX_ROOMS {
            return Err("Maximum 24 candidate rooms".into());
        }
        let id = self.next_id;
        let next_id = id.checked_add(1).ok_or("Room ID space exhausted")?;
        self.rooms.push(RoomLayout {
            id,
            source_room_id: None,
            runtime_room_id: None,
            source_role: None,
            name: format!("Candidate room {id}"),
            deck: 1,
            origin_xz_m: [0.2, 0.2],
            clear_lwh_m: [4.0, 4.0, 3.2],
            door_center_xz_m: [2.2, 0.2],
            door_clear_wh_m: [1.0, 2.0],
        });
        self.next_id = next_id;
        Ok(id)
    }
}

/// Mirrors the current twelve-room source as an editable baseline. Source IDs
/// match `docs/ships/pioneer/rooms/rooms.json`; role names and coordinates come
/// from `pioneer_rooms(deck)`. No runtime file is changed by this conversion.
pub fn current_source_document() -> RoomLayoutDocument {
    let source_ids = [
        ["D1-P1", "D1-S1", "D1-P2", "D1-S2", "D1-P3", "D1-S3"],
        ["D2-P1", "D2-S1", "D2-P2", "D2-S2", "D2-P3", "D2-S3"],
    ];
    let mut rooms = Vec::with_capacity(12);
    for deck in 1..=2u8 {
        for (index, (role, x, z)) in pioneer_rooms(deck).iter().enumerate() {
            let door_z = if *z < 7.0 { 5.65 } else { 8.2 };
            rooms.push(RoomLayout {
                id: rooms.len() as u64 + 1,
                source_room_id: Some(source_ids[usize::from(deck - 1)][index].into()),
                runtime_room_id: (deck == 1 && index == 0)
                    .then(|| PIONEER_CONNECTED_ROOM.room_id.into()),
                source_role: Some((*role).into()),
                name: (*role).into(),
                deck,
                origin_xz_m: [*x, *z],
                clear_lwh_m: [ROOM_LENGTH_M, ROOM_DEPTH_M, ROOM_CLEAR_HEIGHT_M],
                door_center_xz_m: [*x + ROOM_LENGTH_M / 2.0, door_z],
                door_clear_wh_m: [DOOR_WIDTH_M, DOOR_HEIGHT_M],
            });
        }
    }
    RoomLayoutDocument {
        schema_version: 1,
        next_id: 13,
        rooms,
    }
}

pub struct Rooms {
    draft: RoomLayoutDocument,
    saved: RoomLayoutDocument,
    selected: Option<u64>,
    message: String,
    io_requests: Option<SyncSender<IoRequest>>,
    io_results: Receiver<IoResult>,
    pending: Option<PendingOperation>,
    draft_revision: u64,
    load_blocked: bool,
    explicit_reset: bool,
}

impl Rooms {
    pub fn open(project: &Path) -> Self {
        let source = current_source_document();
        let (request_tx, request_rx) = mpsc::sync_channel::<IoRequest>(1);
        let (result_tx, result_rx) = mpsc::sync_channel::<IoResult>(1);
        let worker = thread::Builder::new()
            .name("arkline-room-layout-io".into())
            .spawn(move || {
                while let Ok(request) = request_rx.recv() {
                    let (operation, result) = match request {
                        IoRequest::Save(project, document) => (
                            IoOperation::Save,
                            save(&project, &document).map(|()| document),
                        ),
                        IoRequest::Reload(project) => (IoOperation::Reload, load(&project)),
                    };
                    if result_tx.send(IoResult { operation, result }).is_err() {
                        break;
                    }
                }
            });
        let mut panel = Self {
            draft: source.clone(),
            saved: source,
            selected: None,
            message: "Loading saved room layout…".into(),
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
            panel.message = "Could not start room layout I/O worker. Reset explicitly before replacing any saved file.".into();
        }
        panel
    }

    fn queue(&mut self, operation: IoOperation, request: IoRequest) {
        if self.pending.is_some() {
            self.message = "A room layout file operation is already pending.".into();
            return;
        }
        let Some(sender) = &self.io_requests else {
            self.load_blocked = true;
            self.message = "Room layout I/O worker is unavailable; saving is locked.".into();
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
                self.message = "Room layout I/O queue is full; wait and retry.".into()
            }
            Err(TrySendError::Disconnected(_)) => {
                self.load_blocked = true;
                self.message = "Room layout I/O worker stopped; saving is locked.".into();
            }
        }
    }

    fn poll_io(&mut self) {
        let Some(pending) = self.pending else {
            return;
        };
        match self.io_results.try_recv() {
            Ok(IoResult { operation, result }) => {
                self.pending = None;
                if operation != pending.kind {
                    self.load_blocked = true;
                    self.message = "Room layout I/O result did not match its pending operation; saving is locked.".into();
                    return;
                }
                match (operation, result) {
                    (IoOperation::Save, Ok(document)) => {
                        self.saved = document;
                        self.explicit_reset = false;
                        self.load_blocked = false;
                        self.message = if self.draft_revision == pending.draft_revision {
                            "Saved Assets/Pioneer.rooms.json".into()
                        } else {
                            "Saved the submitted snapshot; newer draft edits remain unsaved.".into()
                        };
                    }
                    (IoOperation::Reload, Ok(document)) => {
                        if self.draft_revision == pending.draft_revision {
                            self.draft = document.clone();
                            self.selected = None;
                            self.message = "Reloaded saved room layout.".into();
                        } else {
                            self.message = "Reload finished, but the draft changed while it was pending; local edits were kept.".into();
                        }
                        self.saved = document;
                        self.explicit_reset = false;
                        self.load_blocked = false;
                    }
                    (IoOperation::Save, Err(error)) => {
                        self.message = format!("Save failed: {error}")
                    }
                    (IoOperation::Reload, Err(error)) => {
                        self.load_blocked = true;
                        self.explicit_reset = false;
                        self.message = format!(
                            "Saved room layout could not be loaded: {error}. Saving is locked until reload succeeds or you explicitly reset."
                        );
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                self.load_blocked = true;
                self.message = "Room layout I/O worker stopped; saving is locked.".into();
            }
        }
    }

    fn reset_after_load_failure(&mut self) {
        self.draft = current_source_document();
        self.saved = current_source_document();
        self.selected = None;
        self.load_blocked = false;
        self.explicit_reset = true;
        self.message = "Explicit reset to the current runtime source confirmed. Saving may replace the unreadable saved file.".into();
    }

    fn can_save(&self) -> bool {
        self.pending.is_none() && !self.load_blocked && self.draft.validate().is_ok()
    }

    fn record_draft_revision(&mut self, before: &RoomLayoutDocument) {
        if before != &self.draft {
            self.draft_revision = self.draft_revision.saturating_add(1);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path) {
        self.poll_io();
        let frame_draft = self.draft.clone();
        ui.heading("Room layout");
        ui.colored_label(
            egui::Color32::from_rgb(255, 191, 104),
            "CANDIDATE DESIGN · local authoring only",
        );
        ui.small("Edits are saved as project data; they do not change the playable interior, walls, doors, equipment, or server behavior.");
        ui.small(format!(
            "Source reference: {RUNTIME_SOURCE} · 2 decks · {:.0} × {:.0} m plan each",
            PIONEER_INTERIOR_SIZE[0], PIONEER_INTERIOR_SIZE[1]
        ));

        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.pending.is_none() && self.draft.rooms.len() < MAX_ROOMS, egui::Button::new("Add room")).clicked() {
                match self.draft.add_room() {
                    Ok(id) => { self.selected = Some(id); self.message = "Added a candidate room; set its position, dimensions, and door opening.".into(); }
                    Err(error) => self.message = error,
                }
            }
            if ui.add_enabled(self.can_save(), egui::Button::new("Save layout")).clicked() {
                self.queue(IoOperation::Save, IoRequest::Save(project.to_path_buf(), self.draft.clone()));
            }
            if ui.add_enabled(self.pending.is_none(), egui::Button::new("Reload saved")).clicked() {
                self.queue(IoOperation::Reload, IoRequest::Reload(project.to_path_buf()));
            }
            if self.load_blocked && ui.add_enabled(self.pending.is_none(), egui::Button::new("Reset to runtime source; allow replacement")).clicked() {
                self.reset_after_load_failure();
            }
            if let Some(pending) = self.pending {
                ui.small(match pending.kind { IoOperation::Save => "Saving…", IoOperation::Reload => "Loading…" });
            }
        });
        if self.load_blocked {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "Saving is locked because the saved room layout has not loaded successfully.",
            );
        }
        if let Err(error) = self.draft.validate() {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.label(if !self.explicit_reset && self.draft == self.saved {
            "No unsaved room layout changes"
        } else {
            "Unsaved room layout changes"
        });
        ui.separator();

        ui.strong("Candidate rooms · source IDs preserved where available");
        egui::ScrollArea::vertical()
            .id_salt("room_layout_entries")
            .auto_shrink([false, true])
            .max_height(112.0)
            .show(ui, |ui| {
                for room in &self.draft.rooms {
                    ui.push_id(room.id, |ui| {
                        let source = room.source_room_id.as_deref().unwrap_or("new");
                        if ui
                            .selectable_label(
                                self.selected == Some(room.id),
                                format!("D{} · {} · {}", room.deck, source, room.name),
                            )
                            .clicked()
                        {
                            self.selected = Some(room.id);
                        }
                    });
                }
            });

        let Some(room_id) = self
            .selected
            .filter(|id| self.draft.rooms.iter().any(|room| room.id == *id))
        else {
            ui.label("Select a room to edit; existing source rooms remain listed above.");
            self.draw_previews(ui, None);
            ui.label(&self.message);
            self.record_draft_revision(&frame_draft);
            return;
        };

        let mut remove_room = false;
        let mut room_error = None;
        {
            let room = self
                .draft
                .rooms
                .iter_mut()
                .find(|room| room.id == room_id)
                .unwrap();
            ui.separator();
            ui.strong(format!("Room #{}", room.id));
            if let Some(source_id) = &room.source_room_id {
                ui.small(format!(
                    "Source room ID: {source_id} · source role: {} · runtime ID: {}",
                    room.source_role.as_deref().unwrap_or("unknown"),
                    room.runtime_room_id.as_deref().unwrap_or("not defined")
                ));
            } else {
                ui.small("New candidate room · no current runtime room ID");
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut room.name);
                egui::ComboBox::from_id_salt(("room_deck", room.id))
                    .selected_text(format!("Deck {}", room.deck))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut room.deck, 1, "Deck 1");
                        ui.selectable_value(&mut room.deck, 2, "Deck 2");
                    });
            });
            ui.label("Deck-plan origin X / Z (m)");
            ui.horizontal_wrapped(|ui| {
                for (axis, value) in room.origin_xz_m.iter_mut().enumerate() {
                    ui.label(if axis == 0 { "X" } else { "Z" });
                    ui.add(
                        egui::DragValue::new(value)
                            .speed(0.1)
                            .range(0.0..=if axis == 0 { 32.0 } else { 14.0 }),
                    );
                }
            });
            ui.label("Clear size · length / depth / height (m)");
            ui.horizontal_wrapped(|ui| {
                for (axis, value) in room.clear_lwh_m.iter_mut().enumerate() {
                    ui.label(["L", "W", "H"][axis]);
                    ui.add(egui::DragValue::new(value).speed(0.1).range(0.01..=32.0));
                }
            });
            ui.label("Door center X / Z and clear width / height (m)");
            ui.horizontal_wrapped(|ui| {
                for (axis, value) in room.door_center_xz_m.iter_mut().enumerate() {
                    ui.label(if axis == 0 { "X" } else { "Z" });
                    ui.add(egui::DragValue::new(value).speed(0.1).range(-1.0..=33.0));
                }
                for (axis, value) in room.door_clear_wh_m.iter_mut().enumerate() {
                    ui.label(if axis == 0 { "W" } else { "H" });
                    ui.add(egui::DragValue::new(value).speed(0.1).range(0.01..=12.0));
                }
            });
            ui.small("Door location is a candidate opening on a front/rear room edge. Runtime door geometry remains fixed until a later integration.");
            if ui
                .add_enabled(
                    self.pending.is_none(),
                    egui::Button::new("Delete candidate room"),
                )
                .clicked()
            {
                remove_room = true;
            }
        }
        if let Err(error) = self.draft.validate() {
            room_error = Some(error);
        }
        if let Some(error) = room_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if remove_room {
            self.draft.rooms.retain(|room| room.id != room_id);
            self.selected = None;
        }
        self.draw_previews(ui, Some(room_id));
        ui.label(&self.message);
        self.record_draft_revision(&frame_draft);
    }

    fn draw_previews(&self, ui: &mut egui::Ui, selected: Option<u64>) {
        ui.separator();
        ui.strong("Deck plan · X/Z meters · candidate boxes only");
        let width = ui.available_width().clamp(120.0, 540.0);
        let height =
            (width * PIONEER_INTERIOR_SIZE[1] as f32 / PIONEER_INTERIOR_SIZE[0] as f32).max(64.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(8, 24, 19));
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, egui::Color32::from_gray(85)),
            egui::StrokeKind::Inside,
        );
        let current_deck = selected
            .and_then(|id| self.draft.rooms.iter().find(|room| room.id == id))
            .map_or(1, |room| room.deck);
        painter.text(
            rect.left_top() + egui::vec2(4.0, 3.0),
            egui::Align2::LEFT_TOP,
            format!("Deck {current_deck}"),
            egui::FontId::monospace(9.0),
            egui::Color32::LIGHT_GRAY,
        );
        let plot = rect.shrink2(egui::vec2(4.0, 12.0));
        let map = |x: f64, z: f64| {
            egui::pos2(
                plot.left() + (x / PIONEER_INTERIOR_SIZE[0]) as f32 * plot.width(),
                plot.top() + (z / PIONEER_INTERIOR_SIZE[1]) as f32 * plot.height(),
            )
        };
        for room in self
            .draft
            .rooms
            .iter()
            .filter(|room| room.deck == current_deck)
        {
            let min = map(room.origin_xz_m[0], room.origin_xz_m[1]);
            let max = map(
                room.origin_xz_m[0] + room.clear_lwh_m[0],
                room.origin_xz_m[1] + room.clear_lwh_m[1],
            );
            let bounds = egui::Rect::from_min_max(min, max);
            let color = if Some(room.id) == selected {
                egui::Color32::LIGHT_GREEN
            } else {
                egui::Color32::from_rgb(53, 135, 94)
            };
            painter.rect_stroke(
                bounds,
                0.0,
                egui::Stroke::new(if Some(room.id) == selected { 2.0 } else { 1.0 }, color),
                egui::StrokeKind::Inside,
            );
            painter.text(
                bounds.center(),
                egui::Align2::CENTER_CENTER,
                room.source_room_id.as_deref().unwrap_or("new"),
                egui::FontId::monospace(7.0),
                color,
            );
            let half = room.door_clear_wh_m[0] / 2.0;
            painter.line_segment(
                [
                    map(room.door_center_xz_m[0] - half, room.door_center_xz_m[1]),
                    map(room.door_center_xz_m[0] + half, room.door_center_xz_m[1]),
                ],
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
        }
        ui.small("Current source role names and room IDs are references; this project document is not a runtime layout override.");

        if let Some(room) =
            selected.and_then(|id| self.draft.rooms.iter().find(|room| room.id == id))
        {
            ui.strong(format!("Side view · {} · X/Y meters", room.name));
            let side_width = ui.available_width().clamp(120.0, 540.0);
            let side_height = 68.0;
            let (side, _) =
                ui.allocate_exact_size(egui::vec2(side_width, side_height), egui::Sense::hover());
            let painter = ui.painter_at(side);
            painter.rect_filled(side, 2.0, egui::Color32::from_rgb(8, 24, 19));
            let plot = side.shrink2(egui::vec2(22.0, 10.0));
            let px = |x: f64| {
                plot.left()
                    + ((x - room.origin_xz_m[0]) / room.clear_lwh_m[0]) as f32 * plot.width()
            };
            let py = |y: f64| plot.bottom() - (y / room.clear_lwh_m[2]) as f32 * plot.height();
            let shell = egui::Rect::from_min_max(
                egui::pos2(px(room.origin_xz_m[0]), py(room.clear_lwh_m[2])),
                egui::pos2(px(room.origin_xz_m[0] + room.clear_lwh_m[0]), py(0.0)),
            );
            painter.rect_stroke(
                shell,
                0.0,
                egui::Stroke::new(1.0, egui::Color32::LIGHT_GREEN),
                egui::StrokeKind::Inside,
            );
            let door_left = px(room.door_center_xz_m[0] - room.door_clear_wh_m[0] / 2.0);
            let door_right = px(room.door_center_xz_m[0] + room.door_clear_wh_m[0] / 2.0);
            painter.rect_stroke(
                egui::Rect::from_min_max(
                    egui::pos2(door_left, py(room.door_clear_wh_m[1])),
                    egui::pos2(door_right, py(0.0)),
                ),
                0.0,
                egui::Stroke::new(1.0, egui::Color32::WHITE),
                egui::StrokeKind::Inside,
            );
            painter.text(
                side.left_top() + egui::vec2(4.0, 3.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "Clear height {:.2} m · door {:.2} × {:.2} m",
                    room.clear_lwh_m[2], room.door_clear_wh_m[0], room.door_clear_wh_m[1]
                ),
                egui::FontId::monospace(8.0),
                egui::Color32::LIGHT_GRAY,
            );
        }
    }
}

fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect room layout directory: {error}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(
            "Room layout directories must be ordinary directories, not symbolic links".into(),
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
        return Ok(assets.join("Pioneer.rooms.json"));
    }
    ordinary_directory(&assets)?;
    let path = assets.join("Pioneer.rooms.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
            return Err(
                "Room layout destination must be an ordinary file, not a symbolic link".into(),
            );
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("Cannot inspect room layout file: {error}")),
    }
    Ok(path)
}

fn load(project: &Path) -> Result<RoomLayoutDocument, String> {
    let path = document_path(project, false)?;
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(current_source_document());
        }
        Err(error) => return Err(format!("Cannot open room layout: {error}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("Cannot read room layout: {error}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Room layout document exceeds 64 KiB".into());
    }
    let document: RoomLayoutDocument = serde_json::from_slice(&bytes)
        .map_err(|error| format!("Invalid room layout JSON: {error}"))?;
    document.validate()?;
    Ok(document)
}

fn save(project: &Path, document: &RoomLayoutDocument) -> Result<(), String> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|error| format!("Cannot encode room layout: {error}"))?;
    if bytes.len() as u64 + 1 > MAX_BYTES {
        return Err("Room layout document exceeds 64 KiB".into());
    }
    let path = document_path(project, true)?;
    let parent = path.parent().ok_or("Room layout parent unavailable")?;
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".Pioneer.rooms.{}.{}.tmp",
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
            Err(error) => return Err(format!("Cannot create room layout temporary file: {error}")),
        };
        let written = file
            .write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all());
        drop(file);
        if let Err(error) = written.and_then(|_| fs::rename(&temp, &path)) {
            let _ = fs::remove_file(&temp);
            return Err(format!("Cannot save room layout: {error}"));
        }
        #[cfg(unix)]
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
        return Ok(());
    }
    Err("Cannot allocate a unique room layout temporary file".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Project(PathBuf);
    impl Project {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "arkline-room-layout-{}-{}",
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

    fn panel_with_result_sender() -> (Rooms, SyncSender<IoResult>) {
        let (request_tx, _request_rx) = mpsc::sync_channel(1);
        let (result_tx, result_rx) = mpsc::sync_channel(1);
        (
            Rooms {
                draft: current_source_document(),
                saved: current_source_document(),
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
    fn source_baseline_preserves_twelve_runtime_room_references() {
        let document = current_source_document();
        assert_eq!(document.rooms.len(), 12);
        assert_eq!(document.rooms[0].source_room_id.as_deref(), Some("D1-P1"));
        assert_eq!(
            document.rooms[0].source_role.as_deref(),
            Some("AIRLOCK / EVA")
        );
        assert_eq!(
            document.rooms[0].runtime_room_id.as_deref(),
            Some("pioneer.deck1.airlock")
        );
        assert_eq!(document.rooms[6].source_room_id.as_deref(), Some("D2-P1"));
        document.validate().unwrap();
    }

    #[test]
    fn project_room_document_round_trips() {
        let project = Project::new();
        let document = current_source_document();
        save(&project.0, &document).unwrap();
        let loaded = load(&project.0).unwrap();
        assert_eq!(loaded.schema_version, document.schema_version);
        assert_eq!(loaded.next_id, document.next_id);
        assert_eq!(loaded.rooms.len(), document.rooms.len());
        for (loaded, expected) in loaded.rooms.iter().zip(&document.rooms) {
            assert_eq!(loaded.id, expected.id);
            assert_eq!(loaded.source_room_id, expected.source_room_id);
            assert_eq!(loaded.runtime_room_id, expected.runtime_room_id);
            assert_eq!(loaded.source_role, expected.source_role);
            assert_eq!(loaded.name, expected.name);
            assert_eq!(loaded.deck, expected.deck);
            for (actual, expected) in loaded
                .origin_xz_m
                .iter()
                .chain(&loaded.clear_lwh_m)
                .chain(&loaded.door_center_xz_m)
                .chain(&loaded.door_clear_wh_m)
                .zip(
                    expected
                        .origin_xz_m
                        .iter()
                        .chain(&expected.clear_lwh_m)
                        .chain(&expected.door_center_xz_m)
                        .chain(&expected.door_clear_wh_m),
                )
            {
                assert!(
                    (actual - expected).abs() <= 1e-9,
                    "room measurement changed by more than tolerance: {actual} vs {expected}"
                );
            }
        }
    }

    #[test]
    fn invalid_positions_dimensions_ids_and_doors_are_rejected() {
        let mut document = current_source_document();
        document.rooms[0].origin_xz_m[0] = f64::NAN;
        assert!(document.validate().is_err());
        let mut document = current_source_document();
        document.rooms[0].clear_lwh_m[0] = 100.0;
        assert!(document.validate().is_err());
        let mut document = current_source_document();
        document.rooms[0].door_clear_wh_m[0] = 100.0;
        assert!(document.validate().is_err());
        let mut document = current_source_document();
        document.rooms[0].id = document.rooms[1].id;
        assert!(document.validate().is_err());
        let mut document = current_source_document();
        document.rooms[0].door_center_xz_m[1] = 3.0;
        assert!(document.validate().is_err());
        let mut document = current_source_document();
        document.rooms[1].origin_xz_m = document.rooms[0].origin_xz_m;
        assert!(document.validate().is_err());
    }

    #[test]
    fn add_and_delete_preserve_monotonic_ids_and_source_reference() {
        let mut document = current_source_document();
        let id = document.add_room().unwrap();
        assert_eq!(id, 13);
        assert!(document.rooms[12].source_room_id.is_none());
        document.rooms.retain(|room| room.id != 4);
        let next_id = document.add_room().unwrap();
        assert_eq!(next_id, 14);
        document.rooms.retain(|room| room.id != id);
        document.validate().unwrap();
    }

    #[test]
    fn save_completion_keeps_newer_edits_as_unsaved() {
        let submitted = current_source_document();
        let (mut panel, result_tx) = panel_with_result_sender();
        panel.draft = submitted.clone();
        panel.saved = submitted.clone();
        panel.draft_revision = 4;
        panel.pending = Some(PendingOperation {
            kind: IoOperation::Save,
            draft_revision: 4,
        });
        panel.draft.rooms[0].name = "Changed during save".into();
        panel.draft_revision = 5;
        result_tx
            .send(IoResult {
                operation: IoOperation::Save,
                result: Ok(submitted.clone()),
            })
            .unwrap();

        panel.poll_io();

        assert_eq!(panel.saved, submitted);
        assert_eq!(panel.draft.rooms[0].name, "Changed during save");
        assert!(panel.message.contains("newer draft edits remain unsaved"));
        assert!(panel.can_save());
    }

    #[test]
    fn reload_completion_keeps_edits_made_after_reload_started() {
        let loaded = current_source_document();
        let (mut panel, result_tx) = panel_with_result_sender();
        panel.draft_revision = 8;
        panel.pending = Some(PendingOperation {
            kind: IoOperation::Reload,
            draft_revision: 8,
        });
        panel.draft.rooms[0].name = "Changed during reload".into();
        panel.draft_revision = 9;
        result_tx
            .send(IoResult {
                operation: IoOperation::Reload,
                result: Ok(loaded.clone()),
            })
            .unwrap();

        panel.poll_io();

        assert_eq!(panel.saved, loaded);
        assert_eq!(panel.draft.rooms[0].name, "Changed during reload");
        assert!(panel.message.contains("local edits were kept"));
        assert!(panel.can_save());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_assets_and_destination() {
        use std::os::unix::fs::symlink;
        let project = Project::new();
        let outside = Project::new();
        let document = current_source_document();
        symlink(&outside.0, project.0.join("Assets")).unwrap();
        assert!(save(&project.0, &document).is_err());
        fs::remove_file(project.0.join("Assets")).unwrap();
        fs::create_dir(project.0.join("Assets")).unwrap();
        let target = outside.0.join("preserve.json");
        fs::write(&target, b"preserve").unwrap();
        symlink(&target, project.0.join("Assets/Pioneer.rooms.json")).unwrap();
        assert!(save(&project.0, &document).is_err());
        assert_eq!(fs::read(target).unwrap(), b"preserve");
    }
}
