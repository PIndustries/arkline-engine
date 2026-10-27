//! Local panel authoring and a small, typed live equipment surface.
use crate::cockpit_document::{self as document, Assignment, Binding, Document, History, Panel};
use arkline_client::{EmbeddedClient, EmbeddedStatus};
use arkline_protocol::{
    AssetsView, PioneerRoomCommand, PioneerRoomState, Request, ShipClass, Snapshot,
};
use eframe::egui::{self, Color32};
use std::{
    path::Path,
    sync::mpsc::{self, Receiver, SyncSender, TryRecvError},
    time::{Duration, Instant},
};

#[cfg(test)]
#[path = "cockpit_tests.rs"]
mod tests;

#[derive(Clone)]
pub struct Live {
    generation: u64,
    pilot: String,
    running: bool,
    ready: bool,
    controls_ready: bool,
    notice: String,
    snapshot: Option<Snapshot>,
    assets: Option<AssetsView>,
}
impl Live {
    pub fn from_client(client: &EmbeddedClient) -> Self {
        Self {
            generation: client.session_generation(),
            pilot: client.pilot().into(),
            running: client.status() == &EmbeddedStatus::Running,
            ready: client.ready(),
            controls_ready: client.equipment_controls_ready(),
            notice: client.command_notice().into(),
            snapshot: client.snapshot(),
            assets: client.assets(),
        }
    }
    fn room(&self, vessel: &str) -> Result<&PioneerRoomState, &'static str> {
        if !self.ready {
            return Err("No active authority session; captured data is read only.");
        }
        let world = self
            .snapshot
            .as_ref()
            .ok_or("Waiting for an authoritative pilot snapshot.")?;
        let pilot = world
            .pilots
            .iter()
            .find(|p| p.name == self.pilot)
            .ok_or("Your pilot is not in this snapshot.")?;
        if pilot.ship_class != ShipClass::Pioneer {
            return Err("Live room bindings require your active Pioneer.");
        }
        let room = self
            .assets
            .as_ref()
            .and_then(|a| a.pioneer_room.as_ref())
            .ok_or("Room telemetry is unavailable.")?;
        if room.vessel_id != vessel {
            return Err("This vessel is not the reported active Pioneer.");
        }
        Ok(room)
    }
    fn command_room(&self, vessel: &str, fresh: bool) -> Result<&PioneerRoomState, &'static str> {
        let room = self.room(vessel)?;
        if !self.running {
            return Err("Play the simulation to operate the applied panel.");
        }
        if !self.controls_ready {
            return Err("The client transport/request gate is closed. Inspect session feedback.");
        }
        if !fresh {
            return Err("Snapshot updates are stale; controls are disabled.");
        }
        let pilot = self
            .snapshot
            .as_ref()
            .unwrap()
            .pilots
            .iter()
            .find(|p| p.name == self.pilot)
            .unwrap();
        if let Some(avatar) = pilot.station_avatar.as_ref() {
            let accessible = if let Some(state) = avatar.docked_traversal.as_ref() {
                let assets = self.assets.as_ref().unwrap();
                let document = assets
                    .pioneer_room_layout
                    .as_ref()
                    .map(|layout| layout.document.clone())
                    .unwrap_or_else(arkline_protocol::default_pioneer_room_layout);
                let fleet_layout = self
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.world_layout.as_ref())
                    .map_or_else(arkline_protocol::FleetLayoutRevision::default, |layout| {
                        layout.fleet_layout
                    });
                let expected = arkline_protocol::docked_traversal_binding(
                    vessel.into(),
                    &document,
                    fleet_layout,
                    assets.hangar.is_some(),
                );
                document.validate_live().is_ok()
                    && state.binding == expected
                    && state.actuators.binding == expected
                    && state.version == arkline_protocol::DOCKED_TRAVERSAL_VERSION
                    && self
                        .snapshot
                        .as_ref()
                        .and_then(|snapshot| snapshot.docked_traversal_capability.as_ref())
                        == Some(&arkline_protocol::docked_traversal_capability())
                    && state.support == arkline_protocol::DockedSupport::Deck1
                    && arkline_protocol::docked_binding_pose(&state.binding).is_ok()
            } else {
                avatar.onboard_interior && avatar.interior_deck == 1
            };
            if !accessible {
                return Err("Operate from the cockpit or aboard Pioneer deck one.");
            }
        }
        if !room.commissioned {
            return Err("Commission room power in Game → Equipment first.");
        }
        Ok(room)
    }
    fn current_vessel(&self) -> Option<&str> {
        let pilot = self
            .snapshot
            .as_ref()?
            .pilots
            .iter()
            .find(|p| p.name == self.pilot)?;
        if pilot.ship_class != ShipClass::Pioneer {
            return None;
        }
        self.assets
            .as_ref()?
            .pioneer_room
            .as_ref()
            .map(|r| r.vessel_id.as_str())
    }
    fn ships(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        if let Some(hangar) = self.assets.as_ref().and_then(|a| a.hangar.as_ref()) {
            for berth in &hangar.berths {
                result.push((
                    berth.id.clone(),
                    format!(
                        "{}{}",
                        berth.ship_class.spec().name,
                        if berth.active { " · active" } else { "" }
                    ),
                ));
            }
        }
        if let Some(id) = self.current_vessel() {
            if !result.iter().any(|(key, _)| key == id) {
                result.push((id.into(), "Pioneer · active".into()));
            }
        }
        result
    }
}
#[derive(Clone, Copy, PartialEq)]
enum FileKind {
    Open,
    Reload,
    Save,
}
struct FileRequest {
    id: u64,
    kind: FileKind,
    document: Option<Document>,
}
struct FileResult {
    id: u64,
    kind: FileKind,
    result: Result<Option<Document>, String>,
}
struct PendingFile {
    id: u64,
    kind: FileKind,
    revision: u64,
    submitted: Option<Document>,
}
struct PendingCommand {
    generation: u64,
    vessel: String,
    on: bool,
    updated_at: u64,
    started: Instant,
    timed_out: bool,
}
struct Applied {
    document: Document,
    revision: u64,
}

pub struct CockpitPanel {
    history: History,
    gesture: Option<Document>,
    saved: Option<Document>,
    applied: Option<Applied>,
    revision: u64,
    next_file: u64,
    requests: Option<SyncSender<FileRequest>>,
    results: Receiver<FileResult>,
    pending_file: Option<PendingFile>,
    blocked: bool,
    message: String,
    runtime_message: String,
    selected_vessel: String,
    selected_template: String,
    selected_control: u32,
    edit_shared: bool,
    operate: bool,
    show_in_game: bool,
    session: Option<(u64, String)>,
    last_tick: Option<u64>,
    fresh_at: Instant,
    pending_command: Option<PendingCommand>,
}
impl CockpitPanel {
    pub fn open(project: &Path) -> Self {
        let root = project.to_path_buf();
        Self::with_worker(move |kind, doc| match kind {
            FileKind::Save => {
                let doc = doc.ok_or("Missing submitted cockpit snapshot")?;
                document::save(&root, &doc)?;
                Ok(Some(doc))
            }
            _ => document::load(&root),
        })
    }
    fn with_worker(
        mut handle: impl FnMut(FileKind, Option<Document>) -> Result<Option<Document>, String>
            + Send
            + 'static,
    ) -> Self {
        let (requests, rx) = mpsc::sync_channel::<FileRequest>(1);
        let (tx, results) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("arkline-cockpit-files".into())
            .spawn(move || {
                while let Ok(request) = rx.recv() {
                    let result = handle(request.kind, request.document);
                    if tx
                        .send(FileResult {
                            id: request.id,
                            kind: request.kind,
                            result,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        let mut panel = Self {
            history: History::new(Document::default()),
            gesture: None,
            saved: None,
            applied: None,
            revision: 0,
            next_file: 0,
            requests: worker.ok().map(|_| requests),
            results,
            pending_file: None,
            blocked: true,
            message: String::new(),
            runtime_message: String::new(),
            selected_vessel: String::new(),
            selected_template: "airlock".into(),
            selected_control: 1,
            edit_shared: true,
            operate: false,
            show_in_game: true,
            session: None,
            last_tick: None,
            fresh_at: Instant::now(),
            pending_command: None,
        };
        panel.queue(FileKind::Open);
        panel
    }
    fn draft(&self) -> &Document {
        self.gesture.as_ref().unwrap_or(&self.history.current)
    }
    fn edit(&mut self, next: Document, continuous: bool) {
        if let Err(e) = next.validate() {
            self.message = e;
            return;
        }
        if continuous || self.gesture.is_some() {
            self.gesture = Some(next);
        } else {
            match self.history.apply(next) {
                Ok(true) => self.revision = self.revision.saturating_add(1),
                Ok(false) => (),
                Err(e) => self.message = e,
            }
        }
    }
    fn finish_gesture(&mut self) {
        if let Some(next) = self.gesture.take() {
            self.edit(next, false);
        }
    }
    fn move_history(&mut self, redo: bool) {
        self.finish_gesture();
        if self.history.move_history(redo) {
            self.revision = self.revision.saturating_add(1);
        }
    }
    fn apply(&mut self) {
        self.finish_gesture();
        match self.history.current.validate() {
            Ok(()) => {
                self.applied = Some(Applied {
                    document: self.history.current.clone(),
                    revision: self.revision,
                });
                self.runtime_message =
                    "Applied local panel content; no equipment command was sent.".into();
            }
            Err(e) => self.message = e,
        }
    }
    fn queue(&mut self, kind: FileKind) {
        if self.pending_file.is_some() || (kind == FileKind::Save && self.blocked) {
            return;
        }
        self.finish_gesture();
        let Some(next) = self.next_file.checked_add(1) else {
            self.blocked = true;
            self.message = "Reopen the editor before more cockpit file operations.".into();
            return;
        };
        let submitted = (kind == FileKind::Save).then(|| self.history.current.clone());
        let request = FileRequest {
            id: self.next_file,
            kind,
            document: submitted.clone(),
        };
        if self
            .requests
            .as_ref()
            .is_some_and(|tx| tx.try_send(request).is_ok())
        {
            self.pending_file = Some(PendingFile {
                id: self.next_file,
                kind,
                revision: self.revision,
                submitted,
            });
            self.next_file = next;
        } else {
            self.requests = None;
            self.blocked = true;
            self.message =
                "Cockpit file worker unavailable. Reopen and read the file before saving.".into();
        }
    }
    pub fn poll(&mut self, live: &Live) {
        self.poll_files();
        let identity = (live.generation, live.pilot.clone());
        if self.session.as_ref() != Some(&identity) {
            if self.session.is_some() {
                self.applied = None;
                self.operate = false;
                self.runtime_message =
                    "Session changed. Apply the panel again; no command was replayed.".into();
            }
            self.session = Some(identity);
            self.pending_command = None;
            self.last_tick = None;
        }
        let tick = live.snapshot.as_ref().map(|s| s.server_time_ms);
        if tick != self.last_tick {
            self.last_tick = tick;
            self.fresh_at = Instant::now();
        }
        if self.selected_vessel.is_empty() {
            let initial = live
                .current_vessel()
                .map(str::to_owned)
                .or_else(|| {
                    live.assets
                        .as_ref()?
                        .hangar
                        .as_ref()?
                        .berths
                        .iter()
                        .find(|berth| berth.active)
                        .map(|berth| berth.id.clone())
                })
                .or_else(|| live.ships().first().map(|(id, _)| id.clone()));
            if let Some(id) = initial {
                self.select_ship(&id);
            }
        }
        if let Some(pending) = &mut self.pending_command {
            if pending.generation != live.generation || !live.ready {
                self.runtime_message =
                    "Command outcome unknown after session interruption; nothing was replayed."
                        .into();
                self.pending_command = None;
            } else if live.room(&pending.vessel).is_ok_and(|r| {
                r.updated_at_ms > pending.updated_at && r.light_commanded == pending.on
            }) {
                self.runtime_message="A newer report matches the requested light command. Output is shown separately.".into();
                self.pending_command = None;
            } else if pending.started.elapsed() > Duration::from_secs(5) {
                pending.timed_out = true;
                self.runtime_message =
                    "No matching newer light report. Outcome unconfirmed; no automatic retry."
                        .into();
            }
        }
    }
    fn poll_files(&mut self) {
        if self.pending_file.is_none() {
            return;
        }
        let result = match self.results.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.pending_file = None;
                self.requests = None;
                self.blocked = true;
                self.message="Cockpit worker stopped. A save may have completed; reopen and read before saving again.".into();
                return;
            }
        };
        let pending = self.pending_file.take().unwrap();
        if result.id != pending.id || result.kind != pending.kind {
            self.requests = None;
            self.blocked = true;
            self.message =
                "Unexpected cockpit file result. Reopen and read before saving again.".into();
            return;
        }
        match result.result {
            Err(e) => {
                if pending.kind != FileKind::Save {
                    self.blocked = true;
                }
                self.message = e;
            }
            Ok(doc) => {
                if let Some(d) = &doc {
                    if let Err(e) = d.validate() {
                        self.blocked = true;
                        self.message = e;
                        return;
                    }
                }
                if pending.kind == FileKind::Save {
                    if doc != pending.submitted || doc.is_none() {
                        self.requests = None;
                        self.blocked = true;
                        self.message="Cockpit save receipt does not match its snapshot; reopen and read the file.".into();
                        return;
                    }
                    self.saved = doc;
                    self.message = if self.saved.as_ref() == Some(self.draft()) {
                        "Saved cockpit content."
                    } else {
                        "Saved submitted cockpit snapshot; newer edits remain unsaved."
                    }
                    .into();
                } else {
                    if pending.kind == FileKind::Reload && doc.is_none() {
                        self.blocked = true;
                        self.message="Saved cockpit file is missing; Save is locked until recovery is chosen.".into();
                        return;
                    }
                    let unchanged = self.revision == pending.revision && self.gesture.is_none();
                    if unchanged {
                        if let Some(d) = &doc {
                            if pending.kind == FileKind::Open {
                                self.history = History::new(d.clone());
                            } else {
                                self.edit(d.clone(), false);
                            }
                        }
                    }
                    self.saved = doc;
                    if !self
                        .history
                        .current
                        .templates
                        .contains_key(&self.selected_template)
                    {
                        self.selected_template = self
                            .history
                            .current
                            .templates
                            .keys()
                            .next()
                            .unwrap()
                            .clone();
                    }
                    if unchanged && !self.selected_vessel.is_empty() {
                        self.select_ship(&self.selected_vessel.clone());
                    }
                    self.message = if !unchanged {
                        "Loaded saved baseline; edits made during loading were kept."
                    } else if self.saved.is_none() {
                        "Built-in panel template. Assign it to a reported ship, then Apply."
                    } else {
                        "Loaded cockpit draft. Apply explicitly to use it in this session."
                    }
                    .into();
                }
                self.blocked = false;
            }
        }
    }
    fn select_ship(&mut self, vessel: &str) {
        self.finish_gesture();
        self.selected_vessel = vessel.into();
        if let Some(a) = self.history.current.ships.get(vessel) {
            self.selected_template = a.template.clone();
            self.edit_shared = a.override_panel.is_none();
        } else {
            self.edit_shared = true;
        }
    }
    fn panel<'a>(&self, doc: &'a Document) -> Option<&'a Panel> {
        if self.edit_shared {
            doc.templates.get(&self.selected_template)
        } else {
            doc.panel(&self.selected_vessel)
        }
    }
    fn displayed_panel(&self) -> Option<&Panel> {
        if self.operate {
            self.applied.as_ref()?.document.panel(&self.selected_vessel)
        } else {
            self.panel(self.draft())
        }
    }
    fn panel_mut<'a>(&self, doc: &'a mut Document) -> Option<&'a mut Panel> {
        if self.edit_shared {
            doc.templates.get_mut(&self.selected_template)
        } else {
            doc.editable_panel(&self.selected_vessel, false)
        }
    }
    fn fresh(&self) -> bool {
        self.last_tick.is_some() && self.fresh_at.elapsed() < Duration::from_secs(3)
    }
    fn light_request(&mut self, live: &Live, vessel: &str, control_id: u32) -> Option<Request> {
        if self.pending_command.is_some()
            || self.session.as_ref() != Some(&(live.generation, live.pilot.clone()))
        {
            return None;
        }
        let applied = self.applied.as_ref()?;
        let control = applied
            .document
            .panel(vessel)?
            .controls
            .iter()
            .find(|c| c.id == control_id)?;
        if !matches!(control.binding, Binding::Light { .. }) || applied.document.validate().is_err()
        {
            return None;
        }
        let room = match live.command_room(vessel, self.fresh()) {
            Ok(r) => r,
            Err(e) => {
                self.runtime_message = e.into();
                return None;
            }
        };
        let on = !room.light_commanded;
        self.pending_command = Some(PendingCommand {
            generation: live.generation,
            vessel: vessel.into(),
            on,
            updated_at: room.updated_at_ms,
            started: Instant::now(),
            timed_out: false,
        });
        self.runtime_message =
            "Light command requested; waiting for a newer report. No state is predicted.".into();
        Some(Request::SetPioneerRoom {
            action: PioneerRoomCommand::Light { on },
        })
    }
    pub fn show(&mut self, ctx: &egui::Context, live: &Live) -> Option<Request> {
        self.poll(live);
        let mut request = None;
        egui::SidePanel::left("cockpit_library").default_width(230.).width_range(210. ..=340.).show(ctx,|ui|{
            ui.heading("Cockpit");ui.label("1 · Choose a ship");
            let mut ships=live.ships();
            for id in self.history.current.ships.keys(){if !ships.iter().any(|(v,_)|v==id){ships.push((id.clone(),"Saved assignment · no live vessel".into()));}}
            if ships.is_empty(){ui.small("Open Game and board a ship to obtain its vessel ID. The shared template can be edited now.");}
            for (id,label) in ships {if ui.selectable_label(self.selected_vessel==id,label).on_hover_text(&id).clicked(){self.select_ship(&id);}}
            ui.separator();ui.label("2 · Choose a template");
            egui::ComboBox::from_id_salt("cockpit_template").selected_text(&self.selected_template).show_ui(ui,|ui|{
                for id in self.history.current.templates.keys(){ui.selectable_value(&mut self.selected_template,id.clone(),id);}
            });
            if ui.add_enabled(!self.selected_vessel.is_empty(),egui::Button::new("Assign template to ship")).clicked(){
                let mut next=self.draft().clone();next.ships.insert(self.selected_vessel.clone(),Assignment{template:self.selected_template.clone(),override_panel:None});self.edit(next,false);self.edit_shared=true;
            }
            if let Some(a)=self.draft().ships.get(&self.selected_vessel).cloned(){
                ui.small(format!("Assigned: {}",a.template));
                if a.override_panel.is_none(){
                    ui.colored_label(Color32::LIGHT_GREEN,"Inherits shared template");
                    if ui.button("Customize this ship").clicked(){let mut next=self.draft().clone();let panel=next.panel(&self.selected_vessel).unwrap().clone();next.ships.get_mut(&self.selected_vessel).unwrap().override_panel=Some(panel);self.edit(next,false);self.edit_shared=false;}
                }else{
                    ui.colored_label(Color32::YELLOW,"This ship has a full panel override");
                    if ui.button("Edit this ship override").clicked(){self.edit_shared=false;}
                    if ui.button("Use shared template").clicked(){let mut next=self.draft().clone();next.ships.get_mut(&self.selected_vessel).unwrap().override_panel=None;self.edit(next,false);self.edit_shared=true;}
                }
            }
            if ui.button("Edit shared template").clicked(){self.finish_gesture();self.edit_shared=true;}
            if ui.add_enabled(self.history.current.templates.len()<16,egui::Button::new("Duplicate template")).clicked(){
                let mut next=self.draft().clone();let mut n=1;while next.templates.contains_key(&format!("panel-{n}")){n+=1;}
                let id=format!("panel-{n}");let panel=next.templates[&self.selected_template].clone();next.templates.insert(id.clone(),panel);self.edit(next,false);self.selected_template=id;self.edit_shared=true;
            }
            ui.separator();ui.label("3 · Select a control");
            if let Some(panel)=self.displayed_panel().cloned(){for c in panel.controls {ui.selectable_value(&mut self.selected_control,c.id,&c.label);}}
            ui.separator();ui.small("Only the pressure gauge and room-light switch are supported. Panels are local project content.");
        });
        egui::SidePanel::right("cockpit_binding_inspector")
            .default_width(285.)
            .width_range(250. ..=400.)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.inspector(ui, live));
            });
        egui::CentralPanel::default().show(ctx,|ui|{
            ui.horizontal_wrapped(|ui|{
                ui.heading(if self.operate {"Operate applied panel"}else{"Edit panel"});
                ui.selectable_value(&mut self.operate,false,"Edit");
                ui.add_enabled_ui(self.applied.is_some(),|ui|{ui.selectable_value(&mut self.operate,true,"Operate applied");});
                if ui.add_enabled(self.history.can_undo(),egui::Button::new("Undo")).clicked(){self.move_history(false);}
                if ui.add_enabled(self.history.can_redo(),egui::Button::new("Redo")).clicked(){self.move_history(true);}
                if ui.button("Apply panel").clicked(){self.apply();}
                if ui.add_enabled(!self.blocked && self.pending_file.is_none() && self.requests.is_some(),egui::Button::new("Save cockpit")).clicked(){self.queue(FileKind::Save);}
                if ui.add_enabled(self.pending_file.is_none() && self.requests.is_some(),egui::Button::new("Reload cockpit")).clicked(){self.queue(FileKind::Reload);}
            });
            ui.horizontal_wrapped(|ui|{
                ui.label(if self.saved.as_ref()==Some(self.draft()) {"Saved"}else{"Unsaved draft"});
                if let Some(applied)=&self.applied {ui.label(format!("Applied revision {}{}",applied.revision,if &applied.document==self.draft(){""}else{" · draft differs"}));}else{ui.label("Not applied in this session");}
                if self.pending_file.is_some(){ui.spinner();ui.label("Cockpit file operation pending…");}
                ui.checkbox(&mut self.show_in_game,"Show applied panel in Game");
            });
            if self.blocked && self.pending_file.is_none(){
                ui.colored_label(Color32::YELLOW,"Save locked: repair the file and Reload, or explicitly allow replacement.");
                if ui.add_enabled(self.requests.is_some(),egui::Button::new("Allow replacing unreadable cockpit file")).clicked(){self.blocked=false;self.saved=None;self.message="Replacement allowed. Save cockpit will overwrite the unreadable file.".into();}
            }
            ui.label(&self.message);
            ui.separator();
            if self.operate {
                let panel=self.displayed_panel().cloned();
                if let Some(panel)=panel {if let Some(id)=self.draw_runtime(ui,&panel,live,&self.selected_vessel.clone()){request=self.light_request(live,&self.selected_vessel.clone(),id);}}
                else {ui.label("This ship has no applied panel. Assign a template and Apply first.");}
            }else{
                ui.small(if self.edit_shared {"Editing shared template · changes affect every inheriting ship after Apply."}else{"Editing only this ship’s full panel override."});
                ui.small("Drag controls to move them. Select a control to edit its label, geometry and binding. Preview clicks never operate equipment.");
                self.canvas(ui,live);
            }
            ui.separator();self.command_status(ui);
        });
        if !ctx.input(|i| i.pointer.primary_down()) {
            self.finish_gesture();
        }
        request
    }
    fn command_status(&mut self, ui: &mut egui::Ui) {
        ui.label(&self.runtime_message);
        if self.pending_command.as_ref().is_some_and(|p| p.timed_out)
            && ui.button("Dismiss unconfirmed request").clicked()
        {
            self.pending_command = None;
            self.runtime_message="Unconfirmed request dismissed. Nothing was retried; inspect the reported state before operating again.".into();
        }
    }
    fn inspector(&mut self, ui: &mut egui::Ui, live: &Live) {
        ui.heading("Control inspector");
        ui.monospace(&self.selected_vessel);
        if self.operate {
            ui.label("Applied panel");
            if self
                .applied
                .as_ref()
                .is_some_and(|a| &a.document != self.draft())
            {
                ui.colored_label(
                    Color32::YELLOW,
                    "Unapplied draft changes. Inspecting the applied panel.",
                );
            }
        }
        let Some(mut panel) = self.displayed_panel().cloned() else {
            ui.label(if self.operate {
                "This ship has no applied panel. Assign a template and Apply first."
            } else {
                "Choose a template or ship override."
            });
            return;
        };
        let mut changed = false;
        let mut dragging = false;
        ui.add_enabled_ui(!self.operate, |ui| {
            changed |= ui.text_edit_singleline(&mut panel.title).changed();
            ui.collapsing("Panel placement in Game", |ui| {
                for axis in 0..2 {
                    ui.horizontal(|ui| {
                        ui.label(if axis == 0 {
                            "Horizontal anchor"
                        } else {
                            "Vertical anchor"
                        });
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut panel.anchor[axis])
                                    .range(0. ..=1.)
                                    .speed(0.01),
                            )
                            .changed();
                    });
                }
                for axis in 0..2 {
                    ui.horizontal(|ui| {
                        ui.label(if axis == 0 { "Offset X" } else { "Offset Y" });
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut panel.offset[axis])
                                    .range(-4000. ..=4000.),
                            )
                            .changed();
                    });
                }
            });
        });
        let Some(control) = panel
            .controls
            .iter_mut()
            .find(|c| c.id == self.selected_control)
        else {
            ui.label("Select a control from the displayed panel.");
            return;
        };
        ui.separator();
        ui.strong(format!("Control {}", control.id));
        ui.add_enabled_ui(!self.operate, |ui| {
            ui.label("Label");
            changed |= ui.text_edit_singleline(&mut control.label).changed();
            for axis in 0..2 {
                ui.horizontal(|ui| {
                    ui.label(if axis == 0 { "X" } else { "Y" });
                    let r = ui.add(
                        egui::DragValue::new(&mut control.position[axis])
                            .range(0. ..=(1. - control.size[axis]))
                            .speed(0.005),
                    );
                    changed |= r.changed();
                    dragging |= r.dragged();
                });
                ui.horizontal(|ui| {
                    ui.label(if axis == 0 { "Width" } else { "Height" });
                    let r = ui.add(
                        egui::DragValue::new(&mut control.size[axis])
                            .range(0.1..=(1. - control.position[axis]))
                            .speed(0.005),
                    );
                    changed |= r.changed();
                    dragging |= r.dragged();
                });
            }
            ui.label("Binding");
            let mut gauge = matches!(control.binding, Binding::Pressure { .. });
            let old = gauge;
            egui::ComboBox::from_id_salt("cockpit_binding_type")
                .selected_text(if gauge {
                    "Pressure gauge"
                } else {
                    "Light switch"
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut gauge, true, "Pressure gauge");
                    ui.selectable_value(&mut gauge, false, "Light switch");
                });
            if old != gauge {
                control.binding = if gauge {
                    Binding::Pressure {
                        equipment: "pioneer.deck1.airlock.pressure".into(),
                        max_kpa: 120.,
                    }
                } else {
                    Binding::Light {
                        equipment: "pioneer.deck1.airlock.light".into(),
                    }
                };
                changed = true;
            }
            if let Binding::Pressure { max_kpa, .. } = &mut control.binding {
                ui.horizontal(|ui| {
                    ui.label("Maximum kPa");
                    changed |= ui
                        .add(egui::DragValue::new(max_kpa).range(1. ..=1000.))
                        .changed();
                });
            }
        });
        ui.monospace(control.binding.equipment());
        ui.small("Pioneer · deck 1 · airlock · authoritative room telemetry");
        let binding = control.binding.clone();
        if changed && !self.operate {
            let mut next = self.draft().clone();
            let Some(target) = self.panel_mut(&mut next) else {
                return;
            };
            *target = panel;
            self.edit(next, dragging);
        }
        ui.separator();
        ui.strong("Reported state");
        match live.room(&self.selected_vessel) {
            Err(e) => {
                ui.colored_label(Color32::YELLOW, e);
            }
            Ok(room) => {
                ui.label(format!("Measurement: {} simulation ms", room.updated_at_ms));
                if !live.running {
                    ui.small(
                        "Captured while simulation is paused/stopped; no elapsed-time prediction.",
                    );
                }
                match binding {
                    Binding::Pressure { .. } => {
                        ui.label(format!(
                            "Pressure: {:.2} kPa",
                            room.pressure_pa as f64 / 1000.
                        ));
                    }
                    Binding::Light { .. } => {
                        ui.label(format!(
                            "Commanded: {}",
                            if room.light_commanded { "On" } else { "Off" }
                        ));
                        ui.label(format!(
                            "Reported output: {}",
                            if room.light_on { "On" } else { "Off" }
                        ));
                    }
                }
                ui.label(format!(
                    "Room interlock: {}",
                    room.interlock.as_deref().unwrap_or("None reported")
                ));
            }
        }
        if let Err(e) = live.command_room(&self.selected_vessel, self.fresh()) {
            ui.small(e);
        }
        if !live.notice.is_empty() {
            ui.separator();
            ui.label("Session feedback");
            ui.small(&live.notice);
        }
    }
    fn canvas(&mut self, ui: &mut egui::Ui, live: &Live) {
        let Some(panel) = self.panel(self.draft()).cloned() else {
            return;
        };
        let available = ui.available_size();
        let width = available.x.max(1.);
        let size = egui::vec2(
            width,
            (width * panel.size[1] / panel.size[0]).min(available.y.max(80.)),
        );
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 8., Color32::from_rgb(9, 24, 29));
        let mut moved = None;
        for control in &panel.controls {
            let r = control_rect(rect, control);
            let response = ui.interact(
                r,
                ui.id().with(("cockpit_control", control.id)),
                egui::Sense::click_and_drag(),
            );
            let room = live.room(&self.selected_vessel).ok();
            let value = reading(&control.binding, room);
            ui.painter().rect_filled(
                r,
                4.,
                if self.selected_control == control.id {
                    Color32::from_rgb(24, 74, 72)
                } else {
                    Color32::from_rgb(22, 42, 48)
                },
            );
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                format!("{}\n{}", control.label, value),
                egui::FontId::proportional(17.),
                Color32::WHITE,
            );
            if response.clicked() || response.drag_started() {
                self.selected_control = control.id;
            }
            if response.dragged() {
                let delta = ui.input(|i| i.pointer.delta());
                let mut position = control.position;
                position[0] =
                    (position[0] + delta.x / rect.width()).clamp(0., 1. - control.size[0]);
                position[1] =
                    (position[1] + delta.y / rect.height()).clamp(0., 1. - control.size[1]);
                moved = Some((control.id, position));
            }
        }
        if let Some((id, position)) = moved {
            let mut next = self.draft().clone();
            if let Some(p) = self.panel_mut(&mut next) {
                p.controls.iter_mut().find(|c| c.id == id).unwrap().position = position;
            }
            self.edit(next, true);
        }
    }
    fn draw_runtime(
        &self,
        ui: &mut egui::Ui,
        panel: &Panel,
        live: &Live,
        vessel: &str,
    ) -> Option<u32> {
        ui.heading(&panel.title);
        let size = egui::vec2(ui.available_width().min(panel.size[0]), panel.size[1]);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let room = live.room(vessel);
        let enabled =
            live.command_room(vessel, self.fresh()).is_ok() && self.pending_command.is_none();
        let mut clicked = None;
        for control in &panel.controls {
            let r = control_rect(rect, control);
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(r), |ui| {
                ui.set_clip_rect(ui.clip_rect().intersect(r));
                match &control.binding {
                    Binding::Pressure { max_kpa, .. } => {
                        ui.label(&control.label);
                        if let Ok(room) = room {
                            ui.add(
                                egui::ProgressBar::new(
                                    (room.pressure_pa as f32 / 1000. / max_kpa).clamp(0., 1.),
                                )
                                .text(format!("{:.2} kPa", room.pressure_pa as f64 / 1000.)),
                            );
                        } else {
                            ui.label("Pressure unavailable");
                        }
                    }
                    Binding::Light { .. } => {
                        let label = format!(
                            "{} · {}",
                            control.label,
                            reading(&control.binding, room.ok())
                        );
                        if ui
                            .add_enabled(
                                enabled,
                                egui::Button::new(label).min_size(egui::vec2(r.width(), 36.)),
                            )
                            .clicked()
                        {
                            clicked = Some(control.id);
                        }
                    }
                }
            });
        }
        if let Err(e) = room {
            ui.colored_label(Color32::YELLOW, e);
        } else if let Err(e) = live.command_room(vessel, self.fresh()) {
            ui.small(e);
        }
        clicked
    }
    pub fn show_applied(&mut self, ctx: &egui::Context, live: &Live) -> Option<Request> {
        self.poll(live);
        if !self.show_in_game {
            return None;
        }
        let vessel = live.current_vessel()?.to_owned();
        let panel = self.applied.as_ref()?.document.panel(&vessel)?.clone();
        let bounds = ctx.available_rect().shrink(8.);
        let (pos, size) = arkline_client::cockpit_panel_geometry(
            bounds,
            panel.anchor,
            egui::Vec2::from(panel.offset),
            egui::Vec2::from(panel.size),
            egui::Vec2::from(panel.size) + egui::vec2(16., 36.),
        );
        let response = egui::Area::new(egui::Id::new("authored_cockpit_panel"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .constrain_to(bounds)
            .show(ctx, |ui| {
                ui.set_width(size.x);
                egui::Frame::window(ui.style())
                    .show(ui, |ui| {
                        let clicked = self.draw_runtime(ui, &panel, live, &vessel);
                        self.command_status(ui);
                        clicked
                    })
                    .inner
            });
        response
            .inner
            .and_then(|id| self.light_request(live, &vessel, id))
    }
}
fn control_rect(panel: egui::Rect, control: &document::Control) -> egui::Rect {
    egui::Rect::from_min_size(
        panel.min
            + egui::vec2(
                panel.width() * control.position[0],
                panel.height() * control.position[1],
            ),
        egui::vec2(
            panel.width() * control.size[0],
            panel.height() * control.size[1],
        ),
    )
}
fn reading(binding: &Binding, room: Option<&PioneerRoomState>) -> String {
    let Some(room) = room else {
        return "Unavailable".into();
    };
    match binding {
        Binding::Pressure { .. } => format!("{:.2} kPa", room.pressure_pa as f64 / 1000.),
        Binding::Light { .. } => format!(
            "command {} · output {}",
            if room.light_commanded { "On" } else { "Off" },
            if room.light_on { "On" } else { "Off" }
        ),
    }
}
