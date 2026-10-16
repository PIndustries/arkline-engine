//! Local room authoring and explicit revision-checked application to the live ship.
use arkline_client::EmbeddedClient;
use arkline_protocol::{
    current_source_document, RoomLayout, RoomLayoutDocument, RoomLayoutState, PIONEER_INTERIOR_SIZE,
};
use eframe::egui;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError},
    thread,
};

const MAX_BYTES: u64 = 64 * 1024;
const MAX_ROOMS: usize = 24;
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

trait RoomLayoutAuthoring {
    fn add_room(&mut self) -> Result<u64, String>;
}
impl RoomLayoutAuthoring for RoomLayoutDocument {
    fn add_room(&mut self) -> Result<u64, String> {
        if self.rooms.len() >= MAX_ROOMS {
            return Err("Maximum 24 rooms".into());
        }
        let id = self.next_id;
        self.next_id = id.checked_add(1).ok_or("Room ID space exhausted")?;
        self.rooms.push(RoomLayout {
            id,
            source_room_id: None,
            runtime_room_id: None,
            source_role: None,
            name: format!("Room {id}"),
            deck: 1,
            origin_xz_m: [0.2, 0.2],
            clear_lwh_m: [4.0, 4.0, 3.2],
            door_center_xz_m: [2.2, 0.2],
            door_clear_wh_m: [1.4, 2.4],
        });
        Ok(id)
    }
}

struct SaveApply {
    generation: u64,
    revision: u64,
    document: RoomLayoutDocument,
}
struct PendingLive {
    id: u64,
    revision: u64,
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
    live: Option<RoomLayoutState>,
    based_on_revision: Option<u64>,
    live_generation: u64,
    save_apply: Option<SaveApply>,
    ready_apply: Option<SaveApply>,
    pending_live: Option<PendingLive>,
    save_apply_canceled: bool,
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
            live: None,
            based_on_revision: None,
            live_generation: 0,
            save_apply: None,
            ready_apply: None,
            pending_live: None,
            save_apply_canceled: false,
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

    #[cfg(test)]
    pub(crate) fn draft_room_name_by_source_id(&self, source_room_id: &str) -> Option<&str> {
        self.draft
            .rooms
            .iter()
            .find(|room| room.source_room_id.as_deref() == Some(source_room_id))
            .map(|room| room.name.as_str())
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
                        self.ready_apply = self.save_apply.take();
                        self.saved = document;
                        self.explicit_reset = false;
                        self.load_blocked = false;
                        self.message = if self.draft_revision == pending.draft_revision {
                            "Saved Assets/Pioneer.rooms.json".into()
                        } else {
                            "Saved the submitted snapshot; newer draft edits remain unsaved.".into()
                        };
                        if self.save_apply_canceled {
                            self.message.push_str(" The original session ended; its requested live apply was canceled.");
                            self.save_apply_canceled = false;
                        }
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
                        self.save_apply = None;
                        self.save_apply_canceled = false;
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

    fn observe_live(&mut self, state: RoomLayoutState) {
        let unchanged_draft = self
            .live
            .as_ref()
            .is_some_and(|old| self.draft == old.document);
        let fresh_source = self.draft == current_source_document() && self.draft == self.saved;
        if !self.load_blocked && self.pending.is_none() {
            if unchanged_draft || fresh_source {
                if self.draft != state.document {
                    self.draft = state.document.clone();
                    self.draft_revision = self.draft_revision.saturating_add(1);
                }
                self.based_on_revision = Some(state.revision);
            } else if self.draft == state.document
                || (self.based_on_revision.is_none() && state.revision == 0)
            {
                self.based_on_revision = Some(state.revision);
            }
        }
        self.live = Some(state);
    }

    fn load_live_draft(&mut self) {
        if let Some(state) = &self.live {
            self.draft = state.document.clone();
            self.based_on_revision = Some(state.revision);
            self.draft_revision = self.draft_revision.saturating_add(1);
            self.selected = self
                .selected
                .filter(|id| self.draft.rooms.iter().any(|r| r.id == *id));
            self.explicit_reset = false;
            self.message = format!(
                "Loaded live revision {} into the draft; the project file changes only after Save.",
                state.revision
            );
        }
    }

    fn reset_live_session(&mut self, generation: u64) {
        if self.live_generation != generation {
            if self.pending_live.is_some() {
                self.message = "The previous session ended before a live acknowledgement was observed. Its edit outcome is unknown; rejoin and inspect that universe before retrying.".into();
            } else if self.save_apply.is_some() || self.ready_apply.is_some() {
                self.message = "The original session ended; its requested live apply was canceled. The project draft can still finish saving.".into();
            }
            self.save_apply_canceled |= self.save_apply.is_some();
            self.live_generation = generation;
            self.live = None;
            self.based_on_revision = None;
            self.save_apply = None;
            self.ready_apply = None;
            self.pending_live = None;
        }
    }

    /// Runs even when this inspector is hidden, so completed saves apply once.
    pub fn poll_live(&mut self, client: &mut EmbeddedClient) {
        self.reset_live_session(client.session_generation());
        self.poll_io();
        if let Some(pending) = &self.pending_live {
            if let Some(ack) = client.room_layout_ack().filter(|ack| ack.id == pending.id) {
                match &ack.result {
                    Ok(()) => {
                        self.based_on_revision = pending.revision.checked_add(1);
                        self.message = "Room layout accepted by the authority. Live geometry follows the confirmed asset revision.".into();
                    }
                    Err(error) => {
                        self.message = format!(
                            "Project draft saved; live apply did not confirm success: {error}. Refresh and load live state before retrying."
                        )
                    }
                }
                self.pending_live = None;
            }
        }
        if self.pending_live.is_some() && !client.ready() {
            self.pending_live = None;
            self.message = "The editor session stopped before a live acknowledgement was observed. The edit outcome is unknown; reconnect and load confirmed live state before retrying.".into();
        }
        if let Some(state) = client
            .assets()
            .and_then(|assets| assets.pioneer_room_layout)
        {
            self.observe_live(state);
        }
        if let Some(apply) = self.ready_apply.take() {
            if apply.generation != client.session_generation() {
                self.message = "Draft saved, but its original editor session ended. It was not applied to the new universe.".into();
            } else {
                match client.room_layout_replace(apply.revision, apply.document) {
                    Ok(id) => {
                        self.pending_live = Some(PendingLive {
                            id,
                            revision: apply.revision,
                        });
                        self.message =
                            "Project draft saved; awaiting live room-layout acknowledgement…"
                                .into();
                    }
                    Err(error) => {
                        self.message =
                            format!("Project draft saved; live apply could not be queued: {error}")
                    }
                }
            }
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, project: &Path, client: &mut EmbeddedClient) {
        self.poll_live(client);
        let frame_draft = self.draft.clone();
        // Keep conditional diagnostics from shifting numeric widget IDs.
        ui.scope(|ui| {
            ui.heading("Room layout");
            ui.colored_label(
                egui::Color32::from_rgb(255, 191, 104),
                "PIONEER · LIVE INTERIOR AUTHORING",
            );
            ui.small("Save + apply live updates the shared Pioneer blueprint, its walls and walking collision after authority approval. Equipment and utilities keep their existing contracts.");
            if let Some(live) = &self.live {
                ui.label(format!(
                    "Live revision {} · draft base {}",
                    live.revision,
                    self.based_on_revision
                        .map_or_else(|| "unlinked".into(), |r| r.to_string())
                ));
                if self.based_on_revision != Some(live.revision) {
                    ui.colored_label(egui::Color32::from_rgb(255, 191, 104), "This draft is not based on the latest live layout. Load live before editing/applying it.");
                }
            } else {
                ui.small("Waiting for the authority's room-layout state…");
            }
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
                if ui.add_enabled(self.can_save(), egui::Button::new("Save draft")).clicked() {
                    self.queue(IoOperation::Save, IoRequest::Save(project.to_path_buf(), self.draft.clone()));
                }
                let live_ready = client.ready() && self.pending_live.is_none() && self.can_save()
                    && self.live.as_ref().is_some_and(|live| self.based_on_revision == Some(live.revision))
                    && self.draft.validate_live().is_ok();
                if ui.add_enabled(live_ready, egui::Button::new("Save + apply live")).clicked() {
                    let apply = SaveApply { generation: client.session_generation(), revision: self.based_on_revision.unwrap(), document: self.draft.clone() };
                    self.queue(IoOperation::Save, IoRequest::Save(project.to_path_buf(), apply.document.clone()));
                    if self.pending.is_some() { self.save_apply = Some(apply); }
                }
                if ui.add_enabled(self.live.is_some() && self.pending.is_none() && self.pending_live.is_none(), egui::Button::new("Load live layout (replace draft)")).clicked() {
                    self.load_live_draft();
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
            if let Err(error) = self.draft.validate_live() {
                ui.small(format!("Draft can be saved; live apply requires: {error}"));
            }
            ui.label(if !self.explicit_reset && self.draft == self.saved {
                "No unsaved room layout changes"
            } else {
                "Unsaved room layout changes"
            });
            ui.separator();

        });

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
            ui.push_id(("room_editor", room.id), |ui| {
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
                        ui.push_id(("origin_xz_m", axis), |ui| {
                            ui.label(if axis == 0 { "X" } else { "Z" });
                            ui.add(
                                egui::DragValue::new(value)
                                    .speed(0.1)
                                    .range(0.0..=if axis == 0 { 32.0 } else { 14.0 }),
                            );
                        });
                    }
                });
                ui.label("Clear size · length / depth / height (m)");
                ui.horizontal_wrapped(|ui| {
                    for (axis, value) in room.clear_lwh_m.iter_mut().enumerate() {
                        ui.push_id(("clear_lwh_m", axis), |ui| {
                            ui.label(["L", "W", "H"][axis]);
                            ui.add(egui::DragValue::new(value).speed(0.1).range(0.01..=32.0));
                        });
                    }
                });
                ui.label("Door center X / Z and clear width / height (m)");
                ui.horizontal_wrapped(|ui| {
                    for (axis, value) in room.door_center_xz_m.iter_mut().enumerate() {
                        ui.push_id(("door_center_xz_m", axis), |ui| {
                            ui.label(if axis == 0 { "X" } else { "Z" });
                            ui.add(egui::DragValue::new(value).speed(0.1).range(-1.0..=33.0));
                        });
                    }
                    for (axis, value) in room.door_clear_wh_m.iter_mut().enumerate() {
                        ui.push_id(("door_clear_wh_m", axis), |ui| {
                            ui.label(if axis == 0 { "W" } else { "H" });
                            ui.add(egui::DragValue::new(value).speed(0.1).range(0.01..=12.0));
                        });
                    }
                });
                ui.small("Live doorways face the access corridor. The connected airlock geometry stays fixed; other room walls and openings follow the accepted layout.");
                if ui
                    .add_enabled(
                        self.pending.is_none(),
                        egui::Button::new("Delete candidate room"),
                    )
                    .clicked()
                {
                    remove_room = true;
                }
            });
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
        ui.small("This plan previews your draft. Save + apply live asks the authority to install it; clients and collision then follow the accepted revision.");

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
                live: None,
                based_on_revision: None,
                live_generation: 0,
                save_apply: None,
                ready_apply: None,
                pending_live: None,
                save_apply_canceled: false,
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
    fn peer_revision_follows_clean_live_draft_but_preserves_local_edits() {
        let (mut panel, _) = panel_with_result_sender();
        let initial = RoomLayoutState {
            revision: 0,
            document: current_source_document(),
        };
        panel.observe_live(initial);
        let mut changed = current_source_document();
        changed.rooms[2].clear_lwh_m[0] = 7.0;
        changed.validate_live().unwrap();
        panel.observe_live(RoomLayoutState {
            revision: 1,
            document: changed.clone(),
        });
        assert_eq!(panel.draft, changed);
        assert_eq!(panel.based_on_revision, Some(1));
        // Receiving peer state does not pretend to have saved the local file.
        assert_ne!(panel.saved, panel.draft);
        panel.draft.rooms[2].name = "Unsaved local design".into();
        let local = panel.draft.clone();
        changed.rooms[3].name = "Peer edit".into();
        panel.observe_live(RoomLayoutState {
            revision: 2,
            document: changed.clone(),
        });
        assert_eq!(panel.draft, local);
        assert_eq!(panel.based_on_revision, Some(1));
        panel.load_live_draft();
        assert_eq!(panel.draft, changed);
        assert_eq!(panel.based_on_revision, Some(2));
    }

    #[test]
    fn save_completion_applies_submitted_revision_and_keeps_newer_draft() {
        let (mut panel, result) = panel_with_result_sender();
        let submitted = panel.draft.clone();
        panel.pending = Some(PendingOperation {
            kind: IoOperation::Save,
            draft_revision: 0,
        });
        panel.save_apply = Some(SaveApply {
            generation: 3,
            revision: 8,
            document: submitted.clone(),
        });
        panel.draft.rooms[2].name = "Edit made during disk write".into();
        panel.draft_revision = 1;
        result
            .send(IoResult {
                operation: IoOperation::Save,
                result: Ok(submitted.clone()),
            })
            .unwrap();
        panel.poll_io();
        let ready = panel.ready_apply.take().unwrap();
        assert_eq!(ready.document, submitted);
        assert_eq!(ready.revision, 8);
        assert_eq!(ready.generation, 3);
        assert_eq!(panel.draft.rooms[2].name, "Edit made during disk write");
        assert!(panel.ready_apply.is_none());
    }

    #[test]
    fn failed_project_save_never_prepares_a_live_mutation() {
        let (mut panel, result) = panel_with_result_sender();
        panel.pending = Some(PendingOperation {
            kind: IoOperation::Save,
            draft_revision: 0,
        });
        panel.save_apply = Some(SaveApply {
            generation: 0,
            revision: 0,
            document: panel.draft.clone(),
        });
        result
            .send(IoResult {
                operation: IoOperation::Save,
                result: Err("disk write rejected".into()),
            })
            .unwrap();
        panel.poll_io();
        assert!(panel.save_apply.is_none());
        assert!(panel.ready_apply.is_none());
    }

    #[test]
    fn session_change_cancels_disk_pending_apply_and_reports_unconfirmed_issued_edit() {
        let (mut panel, result) = panel_with_result_sender();
        let document = panel.draft.clone();
        panel.pending = Some(PendingOperation {
            kind: IoOperation::Save,
            draft_revision: 0,
        });
        panel.save_apply = Some(SaveApply {
            generation: 0,
            revision: 0,
            document: document.clone(),
        });
        panel.reset_live_session(1);
        result
            .send(IoResult {
                operation: IoOperation::Save,
                result: Ok(document),
            })
            .unwrap();
        panel.poll_io();
        assert!(panel.ready_apply.is_none());
        assert!(panel.message.contains("live apply was canceled"));
        panel.pending_live = Some(PendingLive {
            id: 42,
            revision: 0,
        });
        panel.reset_live_session(2);
        assert!(panel.pending_live.is_none());
        assert!(panel.message.contains("outcome is unknown"));
        assert_eq!(panel.live_generation, 2);
    }

    #[test]
    fn numeric_input_keeps_door_focus_across_validation_frames() {
        use std::time::{Duration, Instant};
        let project = Project::new();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut client = EmbeddedClient::new(
            &cc,
            project.0.join(".arkline-studio"),
            "RoomButtonPilot".into(),
        );
        let mut panel = Rooms::open(&project.0);
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            client.poll();
            panel.poll_live(&mut client);
            if client.ready()
                && panel.live.is_some()
                && !panel.load_blocked
                && panel.pending.is_none()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(client.ready() && panel.based_on_revision == Some(0));
        let clock = client.snapshot().unwrap().server_time_ms;
        panel.selected = Some(panel.draft.rooms[8].id);
        let mut before = panel.draft.clone();
        assert!((before.rooms[8].door_center_xz_m[0] - 18.0).abs() < 1e-9);
        before.rooms[8].door_center_xz_m[0] = 18.0;
        let frame = |panel: &mut Rooms, client: &mut EmbeddedClient, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 950.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| panel.show(ui, &project.0, client));
                },
            )
        };

        let output = frame(&mut panel, &mut client, vec![]);
        let heading = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Text(t) if t.galley.text().starts_with("Door center X") => {
                    Some(t.pos.y + t.galley.size().y)
                }
                _ => None,
            })
            .unwrap();
        let point = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t)
                    if t.pos.y >= heading
                        && t.pos.y < heading + 35.0
                        && t.galley.text().parse::<f64>().is_ok() =>
                {
                    Some(t.pos + t.galley.size() * 0.5)
                }
                _ => None,
            })
            .min_by(|a, b| a.x.total_cmp(&b.x))
            .unwrap();
        for pressed in [true, false] {
            frame(
                &mut panel,
                &mut client,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        frame(&mut panel, &mut client, vec![]);
        let focused = ctx.memory(|m| m.focused()).expect("Door X is focused");
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        frame(
            &mut panel,
            &mut client,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text("1".into()),
            ],
        );
        assert_eq!(panel.draft.rooms[8].door_center_xz_m[0], 1.0);
        assert!(panel.draft.validate().is_err());
        frame(&mut panel, &mut client, vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(focused));
        frame(
            &mut panel,
            &mut client,
            vec![
                egui::Event::Text("8".into()),
                key(egui::Key::Enter, egui::Modifiers::NONE),
            ],
        );
        frame(&mut panel, &mut client, vec![]);
        assert_eq!(
            panel.draft, before,
            "Door X must return to 18 without changing Length or another room field"
        );
        for pressed in [true, false] {
            frame(
                &mut panel,
                &mut client,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        frame(&mut panel, &mut client, vec![]);

        let focused = ctx.memory(|m| m.focused()).unwrap();
        panel.load_blocked = true;
        panel.based_on_revision = None;
        frame(&mut panel, &mut client, vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), Some(focused));
        frame(
            &mut panel,
            &mut client,
            vec![
                key(egui::Key::A, egui::Modifiers::COMMAND),
                egui::Event::Text("20.5".into()),
            ],
        );
        frame(&mut panel, &mut client, vec![]);
        let mut decimal = before.clone();
        decimal.rooms[8].door_center_xz_m[0] = 20.5;
        assert_eq!(panel.draft, decimal);

        panel.selected = Some(panel.draft.rooms[9].id);
        frame(&mut panel, &mut client, vec![]);
        frame(&mut panel, &mut client, vec![egui::Event::Text("9".into())]);
        assert_eq!(
            panel.draft, decimal,
            "Room selection must not transfer the edit buffer"
        );
        assert_ne!(ctx.memory(|m| m.focused()), Some(focused));
        panel.load_blocked = false;
        client.poll();
        assert_eq!(client.snapshot().unwrap().server_time_ms, clock);
        assert_eq!(panel.live.as_ref().unwrap().revision, 0);
        assert!(client.paused());
        assert!(!project.0.join("Assets/Pioneer.rooms.json").exists());
        client.on_exit();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && client.status() != &arkline_client::EmbeddedStatus::Stopped
        {
            client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn save_apply_button_saves_project_and_updates_real_authority_while_paused() {
        use std::time::{Duration, Instant};
        let project = Project::new();
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut client = EmbeddedClient::new(
            &cc,
            project.0.join(".arkline-studio"),
            "RoomButtonPilot".into(),
        );
        let mut panel = Rooms::open(&project.0);
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            client.poll();
            panel.poll_live(&mut client);
            if client.ready()
                && panel.live.is_some()
                && !panel.load_blocked
                && panel.pending.is_none()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(client.ready() && panel.based_on_revision == Some(0));
        panel.draft.rooms[2].clear_lwh_m[0] = 7.0;
        panel.draft_revision += 1;
        let frame = |panel: &mut Rooms, client: &mut EmbeddedClient, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1400.0, 950.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| panel.show(ui, &project.0, client));
                },
            )
        };
        let output = frame(&mut panel, &mut client, vec![]);
        let point = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == "Save + apply live" => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("live apply control is visible");
        for pressed in [true, false] {
            frame(
                &mut panel,
                &mut client,
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            client.poll();
            panel.poll_live(&mut client);
            if panel.live.as_ref().is_some_and(|live| live.revision == 1)
                && panel.pending_live.is_none()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            panel.live.as_ref().unwrap().revision,
            1,
            "{}",
            panel.message
        );
        assert_eq!(
            panel.live.as_ref().unwrap().document.rooms[2].clear_lwh_m[0],
            7.0
        );
        assert_eq!(load(&project.0).unwrap().rooms[2].clear_lwh_m[0], 7.0);
        assert!(
            client.paused(),
            "applying room edits must not secretly start time"
        );
        assert!(project.0.join(".arkline-studio/room_layout.json").is_file());
        client.on_exit();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline
            && client.status() != &arkline_client::EmbeddedStatus::Stopped
        {
            client.poll();
            std::thread::sleep(Duration::from_millis(10));
        }
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
