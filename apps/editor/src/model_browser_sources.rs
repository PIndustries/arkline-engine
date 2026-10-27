//! Bounded source-model discovery. A catalog contains metadata, never decoded geometry.
use crate::import_document::{self as doc, Verified};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub(super) struct SourceFile {
    pub id: String,
    pub name: String,
    pub category: String,
    pub source: String,
    pub relative: String,
    pub bytes: u64,
    pub unavailable: Option<String>,
    root: PathBuf,
}
impl SourceFile {
    /// Recheck the saved file and decode it only after an explicit selection.
    pub fn load(&self) -> Result<Verified, String> {
        if let Some(reason) = &self.unavailable {
            return Err(reason.clone());
        }
        doc::preview(&self.root, &self.relative)
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct SourceCatalog {
    pub files: Vec<SourceFile>,
    pub skipped: usize,
    pub truncated: bool,
}
impl SourceCatalog {
    /// The browser can search metadata without reading or decoding any model.
    pub fn matching(&self, search: &str) -> impl Iterator<Item = &SourceFile> {
        let search = search.trim().to_lowercase();
        self.files.iter().filter(move |file| {
            [&file.name, &file.relative, &file.category, &file.source]
                .into_iter()
                .any(|value| value.to_lowercase().contains(&search))
        })
    }
}

struct Scan {
    catalog: SourceCatalog,
    remaining: usize,
    directories: HashSet<DirectoryIdentity>,
    files: HashSet<PathBuf>,
}

// Directory identity deduplicates case aliases on case-insensitive volumes while
// retaining distinct real directories on case-sensitive volumes. File identity
// deliberately uses paths: separate source paths remain separate model choices.
#[cfg(unix)]
type DirectoryIdentity = (u64, u64);
#[cfg(not(unix))]
type DirectoryIdentity = PathBuf;

fn directory_identity(path: &Path, metadata: &fs::Metadata) -> DirectoryIdentity {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        (metadata.dev(), metadata.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        path.to_path_buf()
    }
}

fn ordinary_root(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Cannot inspect model folder {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "Model folder must be an ordinary directory: {}",
            path.display()
        ));
    }
    fs::canonicalize(path)
        .map_err(|error| format!("Cannot resolve model folder {}: {error}", path.display()))
}

/// Scan repository assets first so overlapping project paths keep one source row.
/// Both roots share the same entry, depth and candidate budgets.
pub(super) fn scan(project: &Path, repository_assets: &Path) -> Result<SourceCatalog, String> {
    let project = ordinary_root(project)?;
    let mut scan = Scan {
        catalog: SourceCatalog::default(),
        remaining: doc::MAX_SCAN_ENTRIES,
        directories: HashSet::new(),
        files: HashSet::new(),
    };
    match ordinary_root(repository_assets) {
        Ok(root) => scan.directory(&root, Path::new(""), "Repository", 0),
        Err(_) => scan.catalog.skipped += 1,
    }
    scan.directory(&project, Path::new(""), "Project", 0);
    scan.catalog.files.sort_by(|a, b| {
        a.relative
            .to_lowercase()
            .cmp(&b.relative.to_lowercase())
            .then_with(|| a.source.cmp(&b.source))
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(scan.catalog)
}

impl Scan {
    fn directory(&mut self, root: &Path, relative: &Path, source: &str, depth: usize) {
        let path = root.join(relative);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => metadata,
            _ => {
                self.catalog.skipped += 1;
                return;
            }
        };
        if !self
            .directories
            .insert(directory_identity(&path, &metadata))
        {
            return;
        }
        let mut entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => {
                self.catalog.skipped += 1;
                return;
            }
        };
        let mut paths = Vec::new();
        loop {
            if self.remaining == 0 {
                self.catalog.truncated = true;
                break;
            }
            let Some(entry) = entries.next() else {
                break;
            };
            self.remaining -= 1;
            match entry {
                Ok(entry) => paths.push(entry),
                Err(_) => self.catalog.skipped += 1,
            }
        }
        paths.sort_by_key(|entry| entry.file_name());
        for entry in paths {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                self.catalog.skipped += 1;
                continue;
            };
            if name.starts_with('.')
                || name.eq_ignore_ascii_case("target")
                || name.eq_ignore_ascii_case("node_modules")
                || name.contains([':', '\\'])
            {
                continue;
            }
            let metadata = match fs::symlink_metadata(entry.path()) {
                Ok(metadata) => metadata,
                Err(_) => {
                    self.catalog.skipped += 1;
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                self.catalog.skipped += 1;
                continue;
            }
            let child = relative.join(name);
            if metadata.is_dir() {
                if depth >= doc::MAX_SCAN_DEPTH {
                    self.catalog.truncated = true;
                } else {
                    self.directory(root, &child, source, depth + 1);
                }
            } else if metadata.is_file() {
                self.file(root, &child, source, &metadata);
            }
        }
    }

    fn file(&mut self, root: &Path, relative: &Path, source: &str, metadata: &fs::Metadata) {
        let Some(extension) = relative.extension().and_then(|value| value.to_str()) else {
            return;
        };
        let extension = extension.to_ascii_lowercase();
        if !matches!(
            extension.as_str(),
            "glb"
                | "blend"
                | "gltf"
                | "obj"
                | "fbx"
                | "stl"
                | "dae"
                | "3ds"
                | "ply"
                | "usd"
                | "usda"
                | "usdc"
                | "usdz"
        ) {
            return;
        }
        let Some(relative_text) = relative.to_str().filter(|value| value.len() <= 1024) else {
            self.catalog.skipped += 1;
            return;
        };
        let canonical = match fs::canonicalize(root.join(relative)) {
            Ok(path) => path,
            Err(_) => {
                self.catalog.skipped += 1;
                return;
            }
        };
        if !canonical.starts_with(root) {
            // A directory replaced during discovery must not introduce an
            // out-of-project source. Selection still rechecks every component.
            self.catalog.skipped += 1;
            return;
        }
        let Some(identity) = canonical.to_str() else {
            self.catalog.skipped += 1;
            return;
        };
        if self.files.contains(&canonical) {
            return;
        }
        if self.catalog.files.len() >= doc::MAX_CANDIDATES {
            self.catalog.truncated = true;
            return;
        }
        let unavailable = if extension != "glb" {
            Some(format!(
                "{} source files cannot be previewed. Export a static GLB to inspect this model.",
                extension.to_ascii_uppercase()
            ))
        } else if metadata.len() > doc::MAX_BYTES {
            Some("GLB exceeds the 16 MiB preview limit.".into())
        } else {
            None
        };
        let file = SourceFile {
            id: format!("file:{identity}"),
            name: relative
                .file_name()
                .expect("a discovered file has a name")
                .to_string_lossy()
                .into_owned(),
            category: if canonical.ancestors().skip(1).any(|directory| {
                directory
                    .file_name()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Imported"))
                    && directory
                        .parent()
                        .and_then(Path::file_name)
                        .is_some_and(|name| name.eq_ignore_ascii_case("Assets"))
            }) {
                "Imported files"
            } else {
                "Source files"
            }
            .into(),
            source: source.into(),
            relative: relative_text.into(),
            bytes: metadata.len(),
            unavailable,
            root: root.to_path_buf(),
        };
        self.files.insert(canonical);
        self.catalog.files.push(file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const MODEL: &[u8] = include_bytes!("../tests/fixtures/import/wedge.glb");
    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    // Disposable fixture directories are retained for inspection; never remove
    // an existing project or source file during these catalog regressions.
    fn fixture() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "arkline-model-sources-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }
    fn write(root: &Path, relative: &str, bytes: &[u8]) {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn discovers_case_insensitive_formats_and_searches_source_metadata() {
        let project = fixture();
        let repository = fixture();
        write(&project, "Models/Scout.GLB", MODEL);
        for extension in [
            "blend", "gltf", "obj", "fbx", "stl", "dae", "3ds", "ply", "usd", "usda", "usdc",
            "usdz",
        ] {
            write(&repository, &format!("source.{extension}"), b"source model");
        }
        write(&project, "readme.txt", b"not a model");
        let catalog = scan(&project, &repository).unwrap();
        assert_eq!(catalog.files.len(), 13);
        assert!(!catalog.truncated);
        assert_eq!(catalog.matching(" scout.glb ").count(), 1);
        assert_eq!(catalog.matching("MODELS/").count(), 1);
        assert_eq!(catalog.matching("repository").count(), 12);
        assert_eq!(catalog.matching("missing").count(), 0);
        let glb = catalog.matching("Scout").next().unwrap();
        assert_eq!(glb.name, "Scout.GLB");
        assert_eq!(glb.relative, "Models/Scout.GLB");
        assert_eq!(glb.category, "Source files");
        assert_eq!(glb.source, "Project");
        assert_eq!(glb.bytes, MODEL.len() as u64);
        assert!(glb.unavailable.is_none());
        assert!(glb.load().unwrap().geometry.report.triangles > 0);
        for source in catalog.matching("repository") {
            let reason = source.unavailable.as_ref().unwrap();
            assert!(reason.contains("cannot be previewed"));
            assert_eq!(&source.load().err().unwrap(), reason);
        }
    }

    #[test]
    fn overlapping_roots_are_unique_but_distinct_equal_files_remain_choices() {
        let project = fixture();
        let assets = project.join("Assets");
        fs::create_dir(&assets).unwrap();
        write(&assets, "a.glb", MODEL);
        write(&assets, "nested/b.glb", MODEL);
        write(&project, "other.glb", MODEL);
        let catalog = scan(&project, &assets).unwrap();
        assert_eq!(catalog.files.len(), 3);
        assert_eq!(
            catalog
                .files
                .iter()
                .filter(|file| file.source == "Repository")
                .count(),
            2
        );
        let ids: HashSet<_> = catalog.files.iter().map(|file| &file.id).collect();
        assert_eq!(ids.len(), 3);
        let repeated = scan(&project, &assets).unwrap();
        assert_eq!(
            catalog
                .files
                .iter()
                .map(|file| &file.id)
                .collect::<Vec<_>>(),
            repeated
                .files
                .iter()
                .map(|file| &file.id)
                .collect::<Vec<_>>()
        );
        // On case-insensitive volumes these paths identify the same directory.
        // On case-sensitive volumes create two real directories and retain both.
        let lower = project.join("assets");
        if lower.exists() {
            let alias = scan(&project, &lower).unwrap();
            assert_eq!(alias.files.len(), 3);
            assert_eq!(
                catalog
                    .files
                    .iter()
                    .map(|file| &file.id)
                    .collect::<HashSet<_>>(),
                alias
                    .files
                    .iter()
                    .map(|file| &file.id)
                    .collect::<HashSet<_>>()
            );
        } else {
            fs::create_dir(&lower).unwrap();
            write(&lower, "a.glb", MODEL);
            assert_eq!(scan(&project, &assets).unwrap().files.len(), 4);
        }
    }

    #[test]
    fn saved_imports_remain_choices_while_hidden_and_symlink_paths_are_excluded() {
        let project = fixture();
        let repository = fixture();
        for relative in [
            ".hidden/a.glb",
            ".git/a.glb",
            "target/a.glb",
            "node_modules/a.glb",
        ] {
            write(&project, relative, MODEL);
        }
        write(&project, "Assets/visible.glb", MODEL);
        write(&project, "Assets/Imported/stored.glb", MODEL);
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            project.join("Assets/visible.glb"),
            project.join("alias.glb"),
        )
        .unwrap();
        let catalog = scan(&project, &repository).unwrap();
        assert_eq!(catalog.files.len(), 2);
        let stored = catalog.matching("Imported files").next().unwrap();
        assert_eq!(stored.relative, "Assets/Imported/stored.glb");
        assert_eq!(stored.category, "Imported files");
        assert!(stored.unavailable.is_none());
        assert!(stored.load().unwrap().geometry.report.triangles > 0);
        assert_eq!(catalog.matching("visible.glb").count(), 1);
        #[cfg(unix)]
        assert_eq!(catalog.skipped, 1);
    }

    #[test]
    fn calibration_source_loads_for_preview_without_weakening_strict_import() {
        let project = fixture();
        let repository = fixture();
        let bytes = include_bytes!("../../../assets/examples/calibration-cube.glb");
        write(&project, "Models/calibration.glb", bytes);
        let catalog = scan(&project, &repository).unwrap();
        let source = catalog.matching("calibration").next().unwrap();
        let verified = source.load().unwrap();
        assert_eq!(verified.geometry.report.triangles, 12);
        assert!(verified.geometry.uses_generated_normals());
        assert_eq!(verified.bytes.as_slice(), bytes);
        assert!(doc::import(&project, "Models/calibration.glb").is_err());
        assert_eq!(
            fs::read(project.join("Models/calibration.glb"))
                .unwrap()
                .as_slice(),
            bytes.as_slice()
        );
    }

    #[test]
    fn unavailable_size_and_selection_revalidation_preserve_metadata_only_scan() {
        let project = fixture();
        let repository = fixture();
        write(&project, "broken.glb", b"not decoded while scanning");
        let oversized = fs::File::create(project.join("large.glb")).unwrap();
        oversized.set_len(doc::MAX_BYTES + 1).unwrap();
        let catalog = scan(&project, &repository).unwrap();
        assert_eq!(catalog.files.len(), 2);
        let broken = catalog.matching("broken").next().unwrap();
        assert!(broken.unavailable.is_none());
        assert!(broken.load().is_err());
        let large = catalog.matching("large").next().unwrap();
        assert_eq!(large.bytes, doc::MAX_BYTES + 1);
        assert!(large.unavailable.as_ref().unwrap().contains("16 MiB"));
        assert!(large.load().is_err());
        write(&project, "broken.glb", MODEL);
        assert!(broken.load().is_ok(), "selection must recheck saved bytes");
    }

    #[test]
    fn candidate_and_depth_budgets_are_shared_across_both_roots() {
        let project = fixture();
        let repository = fixture();
        for index in 0..(doc::MAX_CANDIDATES / 2) {
            write(&repository, &format!("repo-{index:03}.blend"), b"source");
        }
        for index in 0..doc::MAX_CANDIDATES {
            write(
                &project,
                &format!("project-{index:03}.glb"),
                b"metadata only",
            );
        }
        let catalog = scan(&project, &repository).unwrap();
        assert_eq!(catalog.files.len(), doc::MAX_CANDIDATES);
        assert!(catalog.truncated);
        assert_eq!(
            catalog.matching("repository").count(),
            doc::MAX_CANDIDATES / 2
        );

        let deep = fixture();
        let relative = format!("{}deep.glb", "nested/".repeat(doc::MAX_SCAN_DEPTH + 2));
        write(&deep, &relative, MODEL);
        let catalog = scan(&deep, &fixture()).unwrap();
        assert!(catalog.files.is_empty());
        assert!(catalog.truncated);
    }

    #[test]
    fn entry_budget_bounds_non_model_directory_scans() {
        let project = fixture();
        let repository = fixture();
        for index in 0..(doc::MAX_SCAN_ENTRIES + 1) {
            write(&project, &format!("file-{index:04}.txt"), b"metadata only");
        }
        let catalog = scan(&project, &repository).unwrap();
        assert!(catalog.files.is_empty());
        assert!(catalog.truncated);
    }
}
