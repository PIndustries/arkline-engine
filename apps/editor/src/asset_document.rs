//! Pioneer material source documents. Workspace paths are fixed, never supplied by a document.
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_BYTES: u64 = 64 * 1024;
const HISTORY_LIMIT: usize = 100;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MaterialDocument {
    pub schema_version: u32,
    pub hull_color: [f32; 3],
    pub trim_color: [f32; 3],
    pub emissive_strength: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roughness: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metallic: Option<f32>,
}
impl Default for MaterialDocument {
    fn default() -> Self {
        Self {
            schema_version: 1,
            hull_color: [0.055, 0.075, 0.072],
            trim_color: [0.015, 0.3, 0.115],
            emissive_strength: 1.0,
            roughness: None,
            metallic: None,
        }
    }
}
impl MaterialDocument {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported Pioneer material schema version".into());
        }
        if self
            .hull_color
            .iter()
            .chain(&self.trim_color)
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("Material colors must be finite RGB values between 0 and 1".into());
        }
        if !self.emissive_strength.is_finite() || !(0.0..=4.0).contains(&self.emissive_strength) {
            return Err("Emissive strength must be finite and between 0 and 4".into());
        }
        for value in [self.roughness, self.metallic].into_iter().flatten() {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(
                    "Roughness and metallic overrides must be finite and between 0 and 1".into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct MaterialHistory {
    current: MaterialDocument,
    undo: VecDeque<MaterialDocument>,
    redo: VecDeque<MaterialDocument>,
}
impl MaterialHistory {
    pub fn new(document: MaterialDocument) -> Result<Self, String> {
        document.validate()?;
        Ok(Self {
            current: document,
            ..Self::default()
        })
    }
    pub fn current(&self) -> &MaterialDocument {
        &self.current
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    /// Unchanged edits do not consume history or discard the redo branch.
    pub fn apply(&mut self, document: MaterialDocument) -> Result<bool, String> {
        document.validate()?;
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
        let Some(previous) = self.undo.pop_back() else {
            return false;
        };
        push_bounded(
            &mut self.redo,
            std::mem::replace(&mut self.current, previous),
        );
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo.pop_back() else {
            return false;
        };
        push_bounded(&mut self.undo, std::mem::replace(&mut self.current, next));
        true
    }
}
fn push_bounded(history: &mut VecDeque<MaterialDocument>, document: MaterialDocument) {
    if history.len() == HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back(document);
}
fn ordinary_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect material directory: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Material directories must be ordinary directories, not symbolic links".into());
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
    let path = assets.join("Pioneer.material.json");
    match fs::symlink_metadata(&path) {
        Ok(m) if !m.is_file() || m.file_type().is_symlink() => {
            return Err("Material destination must be an ordinary file, not a symbolic link".into())
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("Cannot inspect material file: {e}")),
    }
    Ok(path)
}

pub fn load(project: &Path) -> Result<MaterialDocument, String> {
    let path = document_path(project, false)?;
    let file = fs::File::open(path).map_err(|e| format!("Cannot open Pioneer material: {e}"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Cannot read Pioneer material: {e}"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Pioneer material exceeds 64 KiB".into());
    }
    let document: MaterialDocument = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid Pioneer material JSON: {e}"))?;
    document.validate()?;
    Ok(document)
}

/// Validate before touching the workspace. Flush a new sibling before atomic replacement.
/// Intended for a trusted local project; this is not a sandbox against concurrent directory swaps.
pub fn save(project: &Path, document: &MaterialDocument) -> Result<(), String> {
    document.validate()?;
    let bytes =
        serde_json::to_vec_pretty(document).map_err(|e| format!("Cannot encode material: {e}"))?;
    let path = document_path(project, true)?;
    let parent = path.parent().ok_or("Material parent unavailable")?;
    for _ in 0..32 {
        let temp = parent.join(format!(
            ".Pioneer.material.{}.{}.tmp",
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
            Err(e) => return Err(format!("Cannot create temporary material: {e}")),
        };
        let written = file
            .write_all(&bytes)
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all());
        drop(file);
        let result = written.and_then(|_| fs::rename(&temp, &path));
        if let Err(error) = result {
            let _ = fs::remove_file(&temp);
            return Err(format!("Cannot save Pioneer material: {error}"));
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
    Err("Cannot allocate a unique material temporary file".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Project(PathBuf);
    impl Project {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "arkline-material-{}-{}",
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Project {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn legacy_materials_keep_authored_pbr_and_override_history_roundtrips() {
        let legacy = r#"{"schema_version":1,"hull_color":[0.1,0.2,0.3],"trim_color":[0.0,0.5,0.1],"emissive_strength":1.0}"#;
        let original: MaterialDocument = serde_json::from_str(legacy).unwrap();
        original.validate().unwrap();
        assert_eq!(original.roughness, None);
        assert_eq!(original.metallic, None);
        assert!(!serde_json::to_string(&original)
            .unwrap()
            .contains("roughness"));
        let mut modified = original.clone();
        modified.roughness = Some(0.2);
        modified.metallic = Some(0.9);
        let mut history = MaterialHistory::new(original.clone()).unwrap();
        history.apply(modified.clone()).unwrap();
        assert!(history.undo());
        assert_eq!(history.current(), &original);
        assert!(history.redo());
        assert_eq!(history.current(), &modified);
        let path = std::env::temp_dir().join(format!(
            "arkline-pbr-test-{}-{}",
            std::process::id(),
            TEMP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        save(&path, &modified).unwrap();
        assert_eq!(load(&path).unwrap(), modified);
        for bad in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            for roughness in [true, false] {
                let mut invalid = modified.clone();
                if roughness {
                    invalid.roughness = Some(bad);
                } else {
                    invalid.metallic = Some(bad);
                }
                assert!(save(&path, &invalid).is_err());
                assert_eq!(load(&path).unwrap(), modified);
            }
        }
        save(&path, &original).unwrap();
        assert_eq!(load(&path).unwrap(), original);
        fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn save_load_and_invalid_save_preserve_previous_document() {
        let project = Project::new();
        let mut document = MaterialDocument::default();
        save(&project.0, &document).unwrap();
        let before = fs::read(project.0.join("Assets/Pioneer.material.json")).unwrap();
        document.emissive_strength = f32::NAN;
        assert!(save(&project.0, &document).is_err());
        assert_eq!(
            before,
            fs::read(project.0.join("Assets/Pioneer.material.json")).unwrap()
        );
        assert_eq!(load(&project.0).unwrap(), MaterialDocument::default());
        document.emissive_strength = 4.;
        save(&project.0, &document).unwrap();
        assert_eq!(load(&project.0).unwrap(), document);
        assert_eq!(fs::read_dir(project.0.join("Assets")).unwrap().count(), 1);
    }
    #[test]
    fn rejects_invalid_schema_colors_and_oversized_load() {
        let project = Project::new();
        save(&project.0, &MaterialDocument::default()).unwrap();
        let path = project.0.join("Assets/Pioneer.material.json");
        fs::write(&path, vec![b' '; MAX_BYTES as usize + 1]).unwrap();
        assert!(load(&project.0).is_err());
        for value in [-0.1, 1.1, f32::INFINITY, f32::NAN] {
            let mut d = MaterialDocument::default();
            d.hull_color[0] = value;
            assert!(d.validate().is_err());
            d = MaterialDocument::default();
            d.trim_color[1] = value;
            assert!(d.validate().is_err());
        }
        let d = MaterialDocument {
            schema_version: 2,
            ..Default::default()
        };
        assert!(d.validate().is_err());
        fs::write(&path, serde_json::to_vec(&d).unwrap()).unwrap();
        assert!(load(&project.0).is_err());
    }
    #[test]
    fn validated_history_is_bounded_and_branching_discards_redo() {
        let mut history = MaterialHistory::default();
        for i in 1..=120 {
            let d = MaterialDocument {
                emissive_strength: i as f32 / 40.,
                ..Default::default()
            };
            assert!(history.apply(d).unwrap());
        }
        let last = history.current().clone();
        assert_eq!(history.undo.len(), 100);
        assert!(history.undo());
        assert!(history.can_redo());
        assert!(history.redo());
        assert_eq!(history.current(), &last);
        let invalid = MaterialDocument {
            emissive_strength: 9.,
            ..Default::default()
        };
        assert!(history.apply(invalid).is_err());
        assert_eq!(history.current(), &last);
        assert!(history.undo());
        assert!(!history.apply(history.current().clone()).unwrap());
        assert!(history.can_redo());
        history.apply(MaterialDocument::default()).unwrap();
        assert!(!history.can_redo());
        let mut count = 0;
        while history.undo() {
            count += 1;
        }
        assert_eq!(count, 100);
        assert!(!history.can_undo());
    }
    #[cfg(unix)]
    #[test]
    fn refuses_linked_assets_or_destination() {
        use std::os::unix::fs::symlink;
        let p = Project::new();
        let outside = Project::new();
        symlink(&outside.0, p.0.join("Assets")).unwrap();
        assert!(save(&p.0, &MaterialDocument::default()).is_err());
        fs::remove_file(p.0.join("Assets")).unwrap();
        fs::create_dir(p.0.join("Assets")).unwrap();
        let target = outside.0.join("keep");
        fs::write(&target, b"unchanged").unwrap();
        symlink(&target, p.0.join("Assets/Pioneer.material.json")).unwrap();
        assert!(save(&p.0, &MaterialDocument::default()).is_err());
        assert!(load(&p.0).is_err());
        assert_eq!(fs::read(target).unwrap(), b"unchanged");
    }
}
