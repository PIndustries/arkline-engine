//! Local cockpit content; never a simulation or command script.
use arkline_protocol::{pioneer_room_catalog, EquipmentKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub const FILE: &str = "Assets/Cockpit.panels.json";
const MAX_BYTES: u64 = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub schema_version: u32,
    pub templates: BTreeMap<String, Panel>,
    pub ships: BTreeMap<String, Assignment>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub template: String,
    pub override_panel: Option<Panel>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Panel {
    pub title: String,
    pub anchor: [f32; 2],
    pub offset: [f32; 2],
    pub size: [f32; 2],
    pub controls: Vec<Control>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub id: u32,
    pub label: String,
    pub position: [f32; 2],
    pub size: [f32; 2],
    pub binding: Binding,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Binding {
    Pressure { equipment: String, max_kpa: f32 },
    Light { equipment: String },
}
impl Binding {
    pub fn equipment(&self) -> &str {
        match self {
            Self::Pressure { equipment, .. } | Self::Light { equipment } => equipment,
        }
    }
}
impl Default for Document {
    fn default() -> Self {
        Self {
            schema_version: 1,
            ships: BTreeMap::new(),
            templates: BTreeMap::from([(
                "airlock".into(),
                Panel {
                    title: "Pioneer airlock".into(),
                    anchor: [0.5, 1.],
                    offset: [0., -16.],
                    size: [360., 200.],
                    controls: vec![
                        Control {
                            id: 1,
                            label: "Room pressure".into(),
                            position: [0.05, 0.08],
                            size: [0.9, 0.4],
                            binding: Binding::Pressure {
                                equipment: "pioneer.deck1.airlock.pressure".into(),
                                max_kpa: 120.,
                            },
                        },
                        Control {
                            id: 2,
                            label: "Main light".into(),
                            position: [0.05, 0.55],
                            size: [0.9, 0.36],
                            binding: Binding::Light {
                                equipment: "pioneer.deck1.airlock.light".into(),
                            },
                        },
                    ],
                },
            )]),
        }
    }
}
fn text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
impl Document {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported cockpit document version".into());
        }
        if self.templates.is_empty() || self.templates.len() > 16 || self.ships.len() > 64 {
            return Err("Cockpit document needs 1–16 templates and at most 64 ships".into());
        }
        for (id, panel) in &self.templates {
            if !text(id) {
                return Err("Invalid cockpit template ID".into());
            }
            panel.validate()?;
        }
        for (vessel, assignment) in &self.ships {
            if !text(vessel) || !self.templates.contains_key(&assignment.template) {
                return Err("Invalid vessel or unknown cockpit template".into());
            }
            if let Some(panel) = &assignment.override_panel {
                panel.validate()?;
            }
        }
        Ok(())
    }
    pub fn panel(&self, vessel: &str) -> Option<&Panel> {
        let assignment = self.ships.get(vessel)?;
        assignment
            .override_panel
            .as_ref()
            .or_else(|| self.templates.get(&assignment.template))
    }
    pub fn editable_panel(&mut self, vessel: &str, shared: bool) -> Option<&mut Panel> {
        let assignment = self.ships.get_mut(vessel)?;
        if !shared {
            return assignment.override_panel.as_mut();
        }
        self.templates.get_mut(&assignment.template)
    }
}
impl Panel {
    pub fn validate(&self) -> Result<(), String> {
        if !text(&self.title) || self.controls.is_empty() || self.controls.len() > 8 {
            return Err("A cockpit panel needs a title and 1–8 controls".into());
        }
        if self
            .anchor
            .iter()
            .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
            || self
                .offset
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 4000.)
            || self
                .size
                .iter()
                .any(|v| !v.is_finite() || !(180. ..=1200.).contains(v))
        {
            return Err("Invalid cockpit panel placement or size".into());
        }
        let catalog = pioneer_room_catalog();
        let mut ids = BTreeSet::new();
        for control in &self.controls {
            if control.id == 0 || !ids.insert(control.id) || !text(&control.label) {
                return Err("Cockpit controls need unique IDs and labels".into());
            }
            for axis in 0..2 {
                let p = control.position[axis];
                let s = control.size[axis];
                if !p.is_finite() || !s.is_finite() || p < 0. || s < 0.1 || p + s > 1.00001 {
                    return Err("Control rectangles must fit inside their panel".into());
                }
            }
            let kind = match &control.binding {
                Binding::Pressure { max_kpa, .. } => {
                    if !max_kpa.is_finite() || !(1. ..=1000.).contains(max_kpa) {
                        return Err("Gauge maximum must be 1–1000 kPa".into());
                    }
                    EquipmentKind::PressureSensor
                }
                Binding::Light { .. } => EquipmentKind::Light,
            };
            if !catalog
                .equipment
                .iter()
                .any(|d| d.id == control.binding.equipment() && d.kind == kind)
            {
                return Err("Unknown or incompatible cockpit equipment binding".into());
            }
        }
        Ok(())
    }
}

pub struct History {
    pub current: Document,
    undo: VecDeque<Document>,
    redo: VecDeque<Document>,
}
impl History {
    pub fn new(current: Document) -> Self {
        Self {
            current,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        }
    }
    pub fn apply(&mut self, next: Document) -> Result<bool, String> {
        next.validate()?;
        if next == self.current {
            return Ok(false);
        }
        push(&mut self.undo, std::mem::replace(&mut self.current, next));
        self.redo.clear();
        Ok(true)
    }
    pub fn move_history(&mut self, redo: bool) -> bool {
        let (from, to) = if redo {
            (&mut self.redo, &mut self.undo)
        } else {
            (&mut self.undo, &mut self.redo)
        };
        let Some(next) = from.pop_back() else {
            return false;
        };
        push(to, std::mem::replace(&mut self.current, next));
        true
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
fn push(queue: &mut VecDeque<Document>, doc: Document) {
    if queue.len() == 64 {
        queue.pop_front();
    }
    queue.push_back(doc);
}

fn directory(path: &Path) -> Result<bool, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!("Cannot inspect cockpit directory: {e}")),
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            Err("Cockpit directories must be ordinary directories".into())
        }
        Ok(_) => Ok(true),
    }
}
fn destination(project: &Path) -> Result<PathBuf, String> {
    let path = project.join(FILE);
    match fs::symlink_metadata(&path) {
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
            return Err("Cockpit destination must be an ordinary file".into())
        }
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            return Err(format!("Cannot inspect cockpit file: {e}"))
        }
        _ => (),
    }
    Ok(path)
}
pub fn load(project: &Path) -> Result<Option<Document>, String> {
    if !directory(project)? || !directory(&project.join("Assets"))? {
        return Ok(None);
    }
    let path = destination(project)?;
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot open cockpit file: {e}")),
    };
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Cockpit file exceeds 256 KiB".into());
    }
    let doc: Document =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid cockpit JSON: {e}"))?;
    doc.validate()?;
    Ok(Some(doc))
}
pub fn save(project: &Path, document: &Document) -> Result<(), String> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Cockpit file exceeds 256 KiB".into());
    }
    if !directory(project)? {
        return Err("Cockpit project directory is missing".into());
    }
    let assets = project.join("Assets");
    match fs::create_dir(&assets) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.to_string()),
    }
    directory(&assets)?;
    let path = destination(project)?;
    static ID: AtomicU64 = AtomicU64::new(0);
    let temp = assets.join(format!(
        ".cockpit-{}-{}.tmp",
        std::process::id(),
        ID.fetch_add(1, Ordering::Relaxed)
    ));
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    // Trusted local projects only; no protection against concurrent directory swaps.
    let mut file = opts
        .open(&temp)
        .map_err(|e| format!("Cannot create cockpit temporary file: {e}"))?;
    let result = (|| -> std::io::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result.map_err(|e| format!("Cannot save cockpit file: {e}"))
}
