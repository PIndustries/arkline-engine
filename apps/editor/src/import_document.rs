//! Versioned project preview references. File operations never touch the authority.
use arkline_client::{ImportReport, ImportedGeometry};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_SCAN_ENTRIES: usize = 2048;
pub const MAX_SCAN_DEPTH: usize = 8;
pub const MAX_CANDIDATES: usize = 256;
static TEMP: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectFile {
    pub relative: String,
    pub bytes: u64,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalog {
    pub files: Vec<ProjectFile>,
    pub skipped: usize,
    pub truncated: bool,
}
impl Catalog {
    pub fn matching(&self, search: &str) -> impl Iterator<Item = &ProjectFile> {
        let search = search.trim().to_lowercase();
        self.files
            .iter()
            .filter(move |f| f.relative.to_lowercase().contains(&search))
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRecord {
    pub id: u64,
    pub revision: u64,
    pub sha256: String,
    pub source_reference: String,
    pub license_reference: String,
    pub frame: String,
    pub report: ImportReport,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetIndex {
    pub schema_version: u32,
    pub revision: u64,
    pub next_id: u64,
    pub asset: Option<AssetRecord>,
}
impl Default for AssetIndex {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            next_id: 1,
            asset: None,
        }
    }
}
impl AssetIndex {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 || self.next_id == 0 {
            return Err("Unsupported or invalid asset index".into());
        }
        if let Some(a) = &self.asset {
            if a.id == 0
                || a.id >= self.next_id
                || a.revision == 0
                || a.revision > self.revision
                || a.sha256.len() != 64
                || !a
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || a.frame != "body-meters-v1"
                || a.source_reference.len() > 1024
                || a.license_reference.len() > 1024
                || a.report.version != 1
            {
                return Err("Invalid asset identity, revision, hash or frame".into());
            }
            if a.report
                .min_m
                .iter()
                .chain(a.report.max_m.iter())
                .any(|v| !v.is_finite())
                || (0..3).any(|i| a.report.min_m[i] > a.report.max_m[i])
                || a.report.triangles == 0
                || a.report.triangles > 200_000
                || a.report.primitives == 0
                || a.report.primitives > 128
                || a.report.materials.len() > 128
                || a.report.materials.iter().any(|m| m.len() > 1024)
                || a.report.notes.len() > 32
                || a.report.notes.iter().any(|n| n.len() > 2048)
            {
                return Err("Invalid cached geometry report".into());
            }
        }
        Ok(())
    }
    pub fn accept(&self, v: &Verified, source: String, license: String) -> Result<Self, String> {
        let mut next = self.clone();
        next.revision = next.revision.checked_add(1).ok_or("Revision exhausted")?;
        let id = if let Some(a) = &next.asset {
            a.id
        } else {
            let id = next.next_id;
            next.next_id = next.next_id.checked_add(1).ok_or("Asset IDs exhausted")?;
            id
        };
        next.asset = Some(AssetRecord {
            id,
            revision: next.revision,
            sha256: v.sha256.clone(),
            source_reference: source,
            license_reference: license,
            frame: "body-meters-v1".into(),
            report: v.geometry.report.clone(),
        });
        next.validate()?;
        Ok(next)
    }
}
#[derive(Clone)]
pub struct Verified {
    pub bytes: Arc<Vec<u8>>,
    pub geometry: Arc<ImportedGeometry>,
    pub sha256: String,
}
impl Verified {
    pub fn decode(bytes: Vec<u8>) -> Result<Self, String> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err("GLB exceeds 16 MiB".into());
        }
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let geometry = Arc::new(ImportedGeometry::decode(&bytes)?);
        Ok(Self {
            bytes: Arc::new(bytes),
            geometry,
            sha256,
        })
    }
}
pub struct Loaded {
    pub index: AssetIndex,
    pub verified: Option<Verified>,
    pub unavailable: Option<String>,
}
fn ordinary(path: &Path, directory: bool) -> Result<(), String> {
    let m = fs::symlink_metadata(path)
        .map_err(|e| format!("Cannot inspect {}: {e}", path.display()))?;
    if m.file_type().is_symlink() || if directory { !m.is_dir() } else { !m.is_file() } {
        return Err("Asset paths must use ordinary files/directories, not symbolic links".into());
    }
    Ok(())
}
fn child(root: &Path, relative: &str) -> Result<PathBuf, String> {
    ordinary(root, true)?;
    let p = Path::new(relative);
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(':')
        || relative.len() > 1024
        || p.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Choose a project-relative GLB path without traversal".into());
    }
    let mut full = root.to_path_buf();
    let parts = p.components().collect::<Vec<_>>();
    for (i, c) in parts.iter().enumerate() {
        full.push(c.as_os_str());
        ordinary(&full, i + 1 < parts.len())?;
    }
    Ok(full)
}
fn read(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    ordinary(path, false)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Asset file exceeds byte budget".into());
    }
    Ok(bytes)
}
pub fn import(root: &Path, relative: &str) -> Result<Verified, String> {
    if !relative.to_ascii_lowercase().ends_with(".glb") {
        return Err("Choose a .glb file inside this project".into());
    }
    Verified::decode(read(&child(root, relative)?, MAX_BYTES)?)
}
/// Explicit picker/drop input is ephemeral. Only verified bytes are saved later;
/// the external path is never an asset binding and the source is never modified.
pub fn import_selected(path: &Path) -> Result<Verified, String> {
    if !path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
    {
        return Err("Choose one .glb file".into());
    }
    Verified::decode(read(path, MAX_BYTES)?)
}
/// Metadata-only, bounded discovery. The existing import path rechecks all path
/// components and geometry when a choice is inspected; a catalog is not trust.
pub fn catalog(root: &Path) -> Result<Catalog, String> {
    ordinary(root, true)?;
    let mut catalog = Catalog::default();
    let mut remaining = MAX_SCAN_ENTRIES;
    scan(root, Path::new(""), 0, &mut remaining, &mut catalog);
    catalog.files.sort_by(|a, b| a.relative.cmp(&b.relative));
    Ok(catalog)
}
fn scan(root: &Path, relative: &Path, depth: usize, remaining: &mut usize, out: &mut Catalog) {
    let entries = match fs::read_dir(root.join(relative)) {
        Ok(entries) => entries,
        Err(_) => {
            out.skipped += 1;
            return;
        }
    };
    let mut paths = Vec::new();
    for entry in entries {
        if *remaining == 0 {
            out.truncated = true;
            break;
        }
        *remaining -= 1;
        match entry {
            Ok(entry) => paths.push(entry),
            Err(_) => out.skipped += 1,
        }
    }
    paths.sort_by_key(|entry| entry.file_name());
    for entry in paths {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            out.skipped += 1;
            continue;
        };
        // Runtime/hidden state, Cargo output, and our private content store are
        // not source choices. Never follow symlinks, including directories.
        if name.starts_with('.')
            || name == "target"
            || name.contains([':', '\\'])
            || (relative.as_os_str().eq_ignore_ascii_case("Assets")
                && name.eq_ignore_ascii_case("Imported"))
        {
            continue;
        }
        let metadata = match fs::symlink_metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(_) => {
                out.skipped += 1;
                continue;
            }
        };
        if metadata.file_type().is_symlink() {
            out.skipped += 1;
            continue;
        }
        let child = relative.join(name);
        if metadata.is_dir() {
            if depth >= MAX_SCAN_DEPTH {
                out.truncated = true;
            } else {
                scan(root, &child, depth + 1, remaining, out);
            }
        } else if metadata.is_file()
            && child
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("glb"))
        {
            let Some(relative) = child.to_str().filter(|path| path.len() <= 1024) else {
                out.skipped += 1;
                continue;
            };
            if out.files.len() == MAX_CANDIDATES {
                out.truncated = true;
                continue;
            }
            out.files.push(ProjectFile {
                relative: relative.into(),
                bytes: metadata.len(),
            });
        }
    }
}
fn directory(parent: &Path, name: &str, create: bool) -> Result<PathBuf, String> {
    ordinary(parent, true)?;
    let p = parent.join(name);
    if create {
        match fs::create_dir(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.to_string()),
        }
    }
    ordinary(&p, true)?;
    Ok(p)
}
pub fn load(root: &Path) -> Result<Loaded, String> {
    ordinary(root, true)?;
    // Missing directories are a new project, never an unreadable-file fallback.
    let assets = root.join("Assets");
    match fs::symlink_metadata(&assets) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded {
                index: AssetIndex::default(),
                verified: None,
                unavailable: None,
            })
        }
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&assets, true)?,
    }
    let path = assets.join("ImportedAssets.json");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded {
                index: AssetIndex::default(),
                verified: None,
                unavailable: None,
            })
        }
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    let index: AssetIndex = serde_json::from_slice(&read(&path, 256 * 1024)?)
        .map_err(|e| format!("Unreadable asset index: {e}"))?;
    index.validate()?;
    let Some(a) = &index.asset else {
        return Ok(Loaded {
            index,
            verified: None,
            unavailable: None,
        });
    };
    let result = (|| {
        let dir = directory(&assets, "Imported", false)?;
        let v = Verified::decode(read(&dir.join(format!("{}.glb", a.sha256)), MAX_BYTES)?)?;
        if v.sha256 != a.sha256 {
            return Err("Saved geometry hash mismatch".into());
        }
        if v.geometry.report != a.report {
            return Err("Saved geometry/report mismatch".into());
        }
        Ok(v)
    })();
    match result {
        Ok(v) => Ok(Loaded {
            index,
            verified: Some(v),
            unavailable: None,
        }),
        Err(e) => Ok(Loaded {
            index,
            verified: None,
            unavailable: Some(e),
        }),
    }
}
fn atomic_write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    ordinary(dir, true)?;
    let destination = dir.join(name);
    match fs::symlink_metadata(&destination) {
        Ok(_) => ordinary(&destination, false)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let tmp = dir.join(format!(
        ".import-{}-{}.tmp",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        fs::rename(&tmp, &destination).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        fs::File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(tmp);
    }
    result
}
pub fn save(root: &Path, index: &AssetIndex, v: Option<&Verified>) -> Result<(), String> {
    index.validate()?;
    let assets = directory(root, "Assets", true)?;
    if let Some(a) = &index.asset {
        let v = v.ok_or("Cannot save unavailable geometry; import a verified replacement")?;
        if format!("{:x}", Sha256::digest(v.bytes.as_slice())) != a.sha256
            || v.sha256 != a.sha256
            || v.geometry.report != a.report
        {
            return Err("Submitted bytes do not match the accepted asset".into());
        }
        let dir = directory(&assets, "Imported", true)?;
        let name = format!("{}.glb", a.sha256);
        let path = dir.join(&name);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                if read(&path, MAX_BYTES)? != *v.bytes {
                    // Explicit Save may repair a damaged ordinary blob with verified
                    // bytes whose digest matches this exact content address.
                    atomic_write(&dir, &name, &v.bytes)?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                atomic_write(&dir, &name, &v.bytes)?
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    // Verified content is durable before this publication point. Orphan content is harmless.
    let bytes = serde_json::to_vec_pretty(index).map_err(|e| e.to_string())?;
    if bytes.len() > 256 * 1024 {
        return Err("Asset index exceeds byte budget".into());
    }
    atomic_write(&assets, "ImportedAssets.json", &bytes)
}
