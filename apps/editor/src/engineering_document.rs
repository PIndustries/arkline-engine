//! Local candidate mass model, never authoritative simulation or verified CAD.
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const MAX_BYTES: u64 = 64 * 1024;
const HISTORY_LIMIT: usize = 100;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
/// Candidate frame: X longitudinal, Y vertical, Z lateral. Origin at envelope center.
pub const HALF_EXTENT: [f64; 3] = [36., 9., 19.];
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PartClass {
    Structure,
    Equipment,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Component {
    pub id: u64,
    pub name: String,
    pub class: PartClass,
    pub material: String,
    pub mass_kg: f64,
    pub center_m: [f64; 3],
    pub extent_m: [f64; 3],
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineeringDocument {
    pub schema_version: u32,
    pub next_id: u64,
    pub components: Vec<Component>,
}
impl Default for EngineeringDocument {
    fn default() -> Self {
        Self {
            schema_version: 1,
            next_id: 1,
            components: Vec::new(),
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct MassReport {
    pub mass_kg: f64,
    pub center_m: [f64; 3],
    pub diagonal_inertia_kg_m2: [f64; 3],
    pub longitudinal_mass_kg: [f64; 3],
}
impl EngineeringDocument {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.components.len() > 128 {
            return Err("Schema must be 1, with at most 128 components".into());
        }
        let mut ids = HashSet::new();
        for c in &self.components {
            if c.id == 0 || c.id >= self.next_id || !ids.insert(c.id) {
                return Err("Component IDs must be unique, positive and below next_id".into());
            }
            if c.name.trim().is_empty() || c.name.len() > 80 || c.material.len() > 80 {
                return Err("Name required; name and material labels limited to 80 bytes".into());
            }
            if !c.mass_kg.is_finite() || !(0.001..=1e9).contains(&c.mass_kg) {
                return Err("Mass must be finite and between 0.001 and 1,000,000,000 kg".into());
            }
            for (axis, half) in HALF_EXTENT.iter().enumerate() {
                if !c.center_m[axis].is_finite()
                    || !c.extent_m[axis].is_finite()
                    || c.extent_m[axis] < 0.001
                    || c.center_m[axis].abs() + c.extent_m[axis] / 2. > *half
                {
                    return Err("Finite positive component boxes must fit inside the 72 × 18 × 38 m design envelope".into());
                }
            }
        }
        if self.next_id == 0 {
            return Err("next_id must be positive".into());
        }
        Ok(())
    }
    pub fn add_component(&mut self) -> Result<u64, String> {
        if self.components.len() >= 128 {
            return Err("Maximum 128 components".into());
        }
        let id = self.next_id;
        let next = id.checked_add(1).ok_or("Component ID space exhausted")?;
        self.components.push(Component {
            id,
            name: format!("Candidate component {id}"),
            class: PartClass::Structure,
            material: "Unspecified".into(),
            mass_kg: 1.,
            center_m: [0.; 3],
            extent_m: [1.; 3],
        });
        self.next_id = next;
        Ok(id)
    }
    pub fn report(&self) -> Result<Option<MassReport>, String> {
        self.validate()?;
        if self.components.is_empty() {
            return Ok(None);
        }
        let mass_kg: f64 = self.components.iter().map(|c| c.mass_kg).sum();
        let mut center_m = [0.; 3];
        for c in &self.components {
            for (a, v) in center_m.iter_mut().enumerate() {
                *v += c.mass_kg * c.center_m[a] / mass_kg;
            }
        }
        let mut diagonal_inertia_kg_m2 = [0.; 3];
        let mut longitudinal_mass_kg = [0.; 3];
        for c in &self.components {
            let d = std::array::from_fn::<_, 3, _>(|a| c.center_m[a] - center_m[a]);
            for (a, v) in diagonal_inertia_kg_m2.iter_mut().enumerate() {
                let b = (a + 1) % 3;
                let z = (a + 2) % 3;
                *v += c.mass_kg
                    * ((c.extent_m[b].powi(2) + c.extent_m[z].powi(2)) / 12.
                        + d[b].powi(2)
                        + d[z].powi(2));
            }
            longitudinal_mass_kg[if c.center_m[0] < -12. {
                0
            } else if c.center_m[0] > 12. {
                2
            } else {
                1
            }] += c.mass_kg;
        }
        Ok(Some(MassReport {
            mass_kg,
            center_m,
            diagonal_inertia_kg_m2,
            longitudinal_mass_kg,
        }))
    }
}
#[derive(Debug, Clone, Default)]
pub struct EngineeringHistory {
    current: EngineeringDocument,
    undo: VecDeque<EngineeringDocument>,
    redo: VecDeque<EngineeringDocument>,
}
impl EngineeringHistory {
    pub fn new(document: EngineeringDocument) -> Result<Self, String> {
        document.validate()?;
        Ok(Self {
            current: document,
            ..Self::default()
        })
    }
    pub fn current(&self) -> &EngineeringDocument {
        &self.current
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// Keep the allocator monotonic even when an invalid draft cannot enter history.
    /// Save the normalized document to retain this high-water mark across sessions.
    pub fn preserve_allocator(&mut self, document: &mut EngineeringDocument) {
        let next_id = self.current.next_id.max(document.next_id);
        self.current.next_id = next_id;
        document.next_id = next_id;
    }
    /// Unchanged edits do not consume history or discard the redo branch.
    pub fn apply(&mut self, mut document: EngineeringDocument) -> Result<bool, String> {
        document.validate()?;
        self.preserve_allocator(&mut document);
        if self.current == document {
            return Ok(false);
        }
        push_bounded(
            &mut self.undo,
            std::mem::replace(&mut self.current, document),
        );
        self.redo.clear();
        Ok(true)
    }
    pub fn undo(&mut self) -> bool {
        let Some(mut previous) = self.undo.pop_back() else {
            return false;
        };
        self.preserve_allocator(&mut previous);
        push_bounded(
            &mut self.redo,
            std::mem::replace(&mut self.current, previous),
        );
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(mut next) = self.redo.pop_back() else {
            return false;
        };
        self.preserve_allocator(&mut next);
        push_bounded(&mut self.undo, std::mem::replace(&mut self.current, next));
        true
    }
}
fn push_bounded(history: &mut VecDeque<EngineeringDocument>, document: EngineeringDocument) {
    if history.len() == HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back(document);
}
fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect engineering directory: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(
            "Engineering directories must be ordinary directories, not symbolic links".into(),
        );
    }
    Ok(())
}
fn document_path(project: &Path, create: bool) -> Result<PathBuf, String> {
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
    let path = assets.join("Pioneer.engineering.json");
    match fs::symlink_metadata(&path) {
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
            return Err(
                "Engineering destination must be an ordinary file, not a symbolic link".into(),
            )
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Cannot inspect engineering file: {e}")),
    }
    Ok(path)
}

pub fn load(project: &Path) -> Result<EngineeringDocument, String> {
    let path = document_path(project, false)?;
    let file = fs::File::open(path).map_err(|e| format!("Cannot open Pioneer engineering: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read Pioneer engineering: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Pioneer engineering exceeds 64 KiB".into());
    }
    let document: EngineeringDocument = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid Pioneer engineering JSON: {e}"))?;
    document.validate()?;
    Ok(document)
}

/// Validate before touching the workspace. Flush a new sibling before atomic replacement.
/// Intended for a trusted local project; this is not a sandbox against concurrent directory swaps.
pub fn save(project: &Path, document: &EngineeringDocument) -> Result<(), String> {
    document.validate()?;
    let bytes = serde_json::to_vec_pretty(document)
        .map_err(|e| format!("Cannot encode engineering: {e}"))?;
    if bytes.len() as u64 + 1 > MAX_BYTES {
        return Err("Engineering document exceeds 64 KiB".into());
    }
    let path = document_path(project, true)?;
    let parent = path.parent().ok_or("Engineering parent unavailable")?;
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".Pioneer.engineering.{}.{}.tmp",
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
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Cannot create temporary engineering: {e}")),
        };
        let written = file
            .write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all());
        drop(file);
        let result = written.and_then(|_| fs::rename(&temp, &path));
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(format!("Cannot save Pioneer engineering: {error}"));
        }
        // Directory sync improves crash durability on systems that support it.
        #[cfg(unix)]
        {
            if let Ok(directory) = fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        return Ok(());
    }
    Err("Cannot allocate a unique engineering temporary file".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn two_boxes() -> EngineeringDocument {
        let mut d = EngineeringDocument::default();
        d.add_component().unwrap();
        d.add_component().unwrap();
        for (i, c) in d.components.iter_mut().enumerate() {
            c.mass_kg = 12.;
            c.extent_m = [2.; 3];
            c.center_m = [if i == 0 { -3. } else { 3. }, 0., 0.];
        }
        d
    }
    #[test]
    fn mass_center_and_parallel_axis_inertia_are_computed_about_com() {
        let d = two_boxes();
        let r = d.report().unwrap().unwrap();
        assert_eq!(r.mass_kg, 24.);
        assert_eq!(r.center_m, [0.; 3]);
        assert_eq!(r.diagonal_inertia_kg_m2, [16., 232., 232.]);
        assert_eq!(EngineeringDocument::default().report().unwrap(), None);
        let mut shifted = d;
        for c in &mut shifted.components {
            c.center_m[0] += 10.;
        }
        let shifted = shifted.report().unwrap().unwrap();
        assert_eq!(shifted.center_m, [10., 0., 0.]);
        assert_eq!(shifted.diagonal_inertia_kg_m2, r.diagonal_inertia_kg_m2);
    }
    #[test]
    fn undo_and_old_backup_reload_do_not_reuse_component_ids() {
        let backup = EngineeringDocument::default();
        let mut h = EngineeringHistory::new(backup.clone()).unwrap();
        let mut draft = h.current().clone();
        let first = draft.add_component().unwrap();
        h.apply(draft).unwrap();
        assert!(h.undo());
        let mut draft = h.current().clone();
        let second = draft.add_component().unwrap();
        assert!(second > first);
        h.apply(draft).unwrap();
        h.apply(backup).unwrap();
        let mut draft = h.current().clone();
        let third = draft.add_component().unwrap();
        assert!(third > second);
        // A newly allocated ID in an invalid draft remains reserved too.
        h.preserve_allocator(&mut draft);
        draft.components[0].mass_kg = -1.;
        assert!(h.apply(draft).is_err());
        let mut replacement = h.current().clone();
        assert!(replacement.add_component().unwrap() > third);
    }
    #[test]
    fn invalid_mass_box_and_identity_are_rejected() {
        for bad in [f64::NAN, f64::INFINITY, 0., -1., 1e10] {
            let mut d = two_boxes();
            d.components[0].mass_kg = bad;
            assert!(d.validate().is_err());
        }
        for bad in [f64::NAN, 0., -1., 100.] {
            let mut d = two_boxes();
            d.components[0].extent_m[1] = bad;
            assert!(d.validate().is_err());
        }
        let mut d = two_boxes();
        d.components[0].center_m[0] = 36.;
        assert!(d.validate().is_err());
        let mut d = two_boxes();
        d.components[1].id = d.components[0].id;
        assert!(d.validate().is_err());
    }
    #[test]
    fn roundtrip_failed_save_preserves_old_and_history_branches() {
        let path = std::env::temp_dir().join(format!(
            "arkline-engineering-test-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let d = two_boxes();
        save(&path, &d).unwrap();
        assert_eq!(load(&path).unwrap(), d);
        let mut bad = d.clone();
        bad.components[0].mass_kg = -1.;
        assert!(save(&path, &bad).is_err());
        assert_eq!(load(&path).unwrap(), d);
        let mut h = EngineeringHistory::new(d.clone()).unwrap();
        let mut next = d.clone();
        next.components[0].mass_kg = 30.;
        h.apply(next.clone()).unwrap();
        assert!(h.undo());
        assert_eq!(h.current(), &d);
        assert!(h.redo());
        assert_eq!(h.current(), &next);
        assert!(h.apply(bad).is_err());
        assert_eq!(h.current(), &next);
        fs::remove_dir_all(path).unwrap();
    }
}
