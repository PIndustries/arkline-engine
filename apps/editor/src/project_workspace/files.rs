//! Project-scoped, bounded file operations. Only `Save` mutates the filesystem.
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::ffi::OsString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

// A small capability boundary for supported native Unix targets. Each directory component is
// opened relative to the preceding held handle with O_NOFOLLOW; no user-controlled C strings or
// raw descriptors escape this module. Other targets retain the checked std path implementation.
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod anchored {
    use super::*;
    use std::ffi::{CString, OsStr};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::raw::{c_char, c_int};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;

    #[cfg(target_os = "linux")]
    const DIRECTORY: c_int = 0x10000;
    #[cfg(target_os = "macos")]
    const DIRECTORY: c_int = 0x100000;
    #[cfg(target_os = "linux")]
    const NOFOLLOW: c_int = 0x20000;
    #[cfg(target_os = "macos")]
    const NOFOLLOW: c_int = 0x100;
    #[cfg(target_os = "linux")]
    const CLOEXEC: c_int = 0x80000;
    #[cfg(target_os = "macos")]
    const CLOEXEC: c_int = 0x1000000;
    #[cfg(target_os = "linux")]
    const NONBLOCK: c_int = 0x800;
    #[cfg(target_os = "macos")]
    const NONBLOCK: c_int = 0x4;
    #[cfg(target_os = "linux")]
    const CREATE_EXCLUSIVE: c_int = 0x40 | 0x80;
    #[cfg(target_os = "macos")]
    const CREATE_EXCLUSIVE: c_int = 0x200 | 0x800;

    unsafe extern "C" {
        fn openat(directory: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
        fn renameat(
            old_directory: c_int,
            old_path: *const c_char,
            new_directory: c_int,
            new_path: *const c_char,
        ) -> c_int;
        fn unlinkat(directory: c_int, path: *const c_char, flags: c_int) -> c_int;
    }

    // Darwin's 64-bit dirent layout is defined by the installed SDK's sys/dirent.h.
    // Keep the stream opaque and inspect only the documented prefix/name bytes returned by libc.
    #[cfg(target_os = "macos")]
    #[repr(C)]
    struct DarwinDirent {
        _inode: u64,
        _seek_offset: u64,
        record_bytes: u16,
        name_bytes: u16,
        kind: u8,
        name: [c_char; 1024],
    }
    #[cfg(target_os = "macos")]
    unsafe extern "C" {
        #[cfg_attr(target_arch = "x86_64", link_name = "fdopendir$INODE64")]
        fn fdopendir(fd: c_int) -> *mut std::ffi::c_void;
        #[cfg_attr(not(target_arch = "aarch64"), link_name = "readdir$INODE64")]
        fn readdir(stream: *mut std::ffi::c_void) -> *mut DarwinDirent;
        fn closedir(stream: *mut std::ffi::c_void) -> c_int;
        fn __error() -> *mut c_int;
        fn readlinkat(
            directory: c_int,
            path: *const c_char,
            buffer: *mut c_char,
            capacity: usize,
        ) -> isize;
    }

    #[cfg(target_os = "macos")]
    struct DirectoryStream(*mut std::ffi::c_void);
    #[cfg(target_os = "macos")]
    impl Drop for DirectoryStream {
        fn drop(&mut self) {
            // SAFETY: this wrapper exclusively owns a successful fdopendir result.
            unsafe {
                closedir(self.0);
            }
        }
    }

    fn name(value: &OsStr) -> Result<CString, String> {
        CString::new(value.as_bytes())
            .map_err(|_| "Project path contains an unsupported NUL byte.".into())
    }
    fn open_at(directory: &File, name: &CString, flags: c_int) -> std::io::Result<File> {
        // SAFETY: directory owns a live descriptor, name is NUL-terminated and valid throughout
        // the call, flags are local constants, and a newly owned fd is transferred exactly once.
        let fd = unsafe {
            openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                flags | NOFOLLOW | CLOEXEC,
                0o600_u32,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: successful openat returned a new descriptor owned by this function.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn directory(root: &ProjectRoot, relative: &str) -> Result<File, String> {
        let relative = relative_path(relative)?;
        let mut directory = OpenOptions::new()
            .read(true)
            .custom_flags(DIRECTORY | NOFOLLOW | CLOEXEC)
            .open(&root.path)
            .map_err(|e| format!("Cannot hold project directory safely: {e}"))?;
        if !same_identity(
            &root.metadata,
            &directory.metadata().map_err(|e| e.to_string())?,
        ) {
            return Err(
                "Project root changed. Reopen the intended project folder before continuing."
                    .into(),
            );
        }
        for component in relative.components() {
            directory = open_at(&directory, &name(component.as_os_str())?, DIRECTORY)
                .map_err(|e| format!("Cannot hold project path safely: {e}"))?;
        }
        Ok(directory)
    }
    pub(super) fn entries(
        directory: &File,
        job: &Job<'_>,
    ) -> Result<(Vec<DirectoryItem>, bool), String> {
        #[cfg(target_os = "linux")]
        {
            let path = Path::new("/proc/self/fd").join(directory.as_raw_fd().to_string());
            standard_entries(&path, job)
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::IntoRawFd;
            let duplicate = directory
                .try_clone()
                .map_err(|e| format!("Cannot hold directory reader: {e}"))?
                .into_raw_fd();
            // SAFETY: duplicate is newly owned. fdopendir consumes it only on success.
            let pointer = unsafe { fdopendir(duplicate) };
            if pointer.is_null() {
                let error = std::io::Error::last_os_error();
                // SAFETY: fdopendir failed, so ownership of duplicate remains here.
                drop(unsafe { File::from_raw_fd(duplicate) });
                return Err(format!("Cannot enumerate held project folder: {error}"));
            }
            let stream = DirectoryStream(pointer);
            let mut entries = Vec::new();
            loop {
                job.check()?;
                // SAFETY: __error provides this thread's errno slot. Resetting it distinguishes
                // readdir's normal end-of-stream from a real I/O error; stream is live/exclusive.
                let item = unsafe {
                    *__error() = 0;
                    readdir(stream.0)
                };
                if item.is_null() {
                    let error = std::io::Error::last_os_error();
                    if error.raw_os_error() != Some(0) {
                        return Err(format!("Cannot enumerate project entry: {error}"));
                    }
                    return Ok((entries, false));
                }
                // SAFETY: libc returned a valid dirent until the next readdir call. Read only
                // its fixed prefix, validate the name's length against the record, then copy it.
                let (record_bytes, name_bytes, kind) = unsafe {
                    (
                        (*item).record_bytes as usize,
                        (*item).name_bytes as usize,
                        (*item).kind,
                    )
                };
                let offset = std::mem::offset_of!(DarwinDirent, name);
                if name_bytes >= 1024 || record_bytes < offset + name_bytes + 1 {
                    return Err("Invalid native directory entry; listing stopped safely.".into());
                }
                // SAFETY: bounds above prove the OS record contains these name bytes. No C
                // pointer or borrowed bytes outlive this iteration; OsStr is copied immediately.
                let name = unsafe {
                    std::slice::from_raw_parts(
                        std::ptr::addr_of!((*item).name).cast::<u8>(),
                        name_bytes,
                    )
                };
                if name == b"." || name == b".." {
                    continue;
                }
                if entries.len() >= LIST_SCAN_LIMIT {
                    return Ok((entries, true));
                }
                let name = OsStr::from_bytes(name).to_os_string();
                let (is_dir, is_symlink) = if kind == 0 {
                    let component = self::name(&name)?;
                    let mut byte = 0;
                    // SAFETY: live held fd, valid single-component CString, one-byte buffer.
                    let linked = unsafe {
                        readlinkat(directory.as_raw_fd(), component.as_ptr(), &mut byte, 1)
                    };
                    if linked >= 0 {
                        (false, true)
                    } else {
                        let file = open_at(directory, &component, NONBLOCK).map_err(|e| {
                            format!("Cannot inspect native directory entry safely: {e}")
                        })?;
                        (file.metadata().map_err(|e| e.to_string())?.is_dir(), false)
                    }
                } else {
                    (kind == 4, kind == 10)
                };
                entries.push(DirectoryItem {
                    name,
                    is_dir,
                    is_symlink,
                });
            }
        }
    }
    fn parent(root: &ProjectRoot, relative: &str) -> Result<(File, CString), String> {
        let relative = relative_path(relative)?;
        let child = relative.file_name().ok_or("Choose a project file")?;
        let parent = relative
            .parent()
            .and_then(Path::to_str)
            .ok_or("Unsupported project path")?;
        Ok((directory(root, parent)?, name(child)?))
    }
    pub(super) fn read(root: &ProjectRoot, relative: &str) -> Result<File, String> {
        let (directory, name) = parent(root, relative)?;
        let file = open_at(&directory, &name, NONBLOCK)
            .map_err(|e| format!("Cannot read project file safely: {e}"))?;
        if !file.metadata().map_err(|e| e.to_string())?.is_file() {
            return Err("Special files cannot be read or edited.".into());
        }
        Ok(file)
    }

    pub(super) struct Prepared {
        directory: File,
        target: CString,
        temporary: CString,
        active: bool,
    }
    impl Drop for Prepared {
        fn drop(&mut self) {
            if self.active {
                // SAFETY: both values are owned/live, name is a single component, and this only
                // removes the exclusively created temporary name from its original held directory.
                unsafe {
                    unlinkat(self.directory.as_raw_fd(), self.temporary.as_ptr(), 0);
                }
            }
        }
    }
    impl Prepared {
        pub(super) fn create(root: &ProjectRoot, relative: &str) -> Result<(Self, File), String> {
            let (directory, target) = parent(root, relative)?;
            for _ in 0..16 {
                let temporary = CString::new(format!(
                    ".arkline-save-{}-{}",
                    std::process::id(),
                    NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
                ))
                .map_err(|_| "Invalid atomic-save name")?;
                match open_at(&directory, &temporary, 1 | CREATE_EXCLUSIVE) {
                    Ok(file) => {
                        return Ok((
                            Self {
                                directory,
                                target,
                                temporary,
                                active: true,
                            },
                            file,
                        ))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(e) => return Err(format!("Cannot prepare atomic save safely: {e}")),
                }
            }
            Err("Cannot allocate a unique atomic-save file".into())
        }
        pub(super) fn replace(&mut self) -> Result<(), String> {
            // SAFETY: held descriptor and both NUL-terminated single-component names are live.
            // Same-directory rename atomically replaces the destination without following it.
            let result = unsafe {
                renameat(
                    self.directory.as_raw_fd(),
                    self.temporary.as_ptr(),
                    self.directory.as_raw_fd(),
                    self.target.as_ptr(),
                )
            };
            if result != 0 {
                return Err(format!(
                    "Atomic save failed; original file was preserved: {}",
                    std::io::Error::last_os_error()
                ));
            }
            self.active = false;
            Ok(())
        }
    }
}

const TEXT_LIMIT: u64 = 1024 * 1024;
const GLB_PREVIEW_LIMIT: u64 = 16 * 1024 * 1024;
const LIST_LIMIT: usize = 1000;
const LIST_SCAN_LIMIT: usize = 4000;
const SEARCH_VISIT_LIMIT: usize = 10_000;
const SEARCH_RESULT_LIMIT: usize = 200;
const PATH_BYTES_LIMIT: usize = 4096;
const BATCH_BYTES_LIMIT: usize = 256 * 1024;
const GIT_OUTPUT_LIMIT: u64 = 1024 * 1024;
const JOB_TIMEOUT: Duration = Duration::from_secs(10);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub(super) enum Request {
    List {
        root: PathBuf,
        relative: String,
        show_hidden: bool,
        show_ignored: bool,
    },
    Search {
        root: PathBuf,
        query: String,
        show_hidden: bool,
        show_ignored: bool,
    },
    Read {
        root: PathBuf,
        relative: String,
    },
    PreviewGlb {
        root: PathBuf,
        relative: String,
        expected: Stamp,
    },
    Save {
        root: PathBuf,
        relative: String,
        expected: Stamp,
        text: String,
    },
}

#[derive(Clone, Debug)]
pub(super) enum Outcome {
    Listed {
        relative: String,
        entries: Vec<Entry>,
        truncated: bool,
    },
    Found {
        paths: Vec<String>,
        truncated: bool,
    },
    Opened(Document),
    PreviewGlb {
        relative: String,
        bytes: Vec<u8>,
    },
    Saved {
        relative: String,
        stamp: Stamp,
    },
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub relative: String,
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub ignored: bool,
    pub restricted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DocumentKind {
    Text,
    Binary,
    Oversized,
    Restricted,
    Symlink,
}

#[derive(Clone, Debug)]
pub(super) struct Document {
    pub relative: String,
    pub kind: DocumentKind,
    pub text: Option<String>,
    pub stamp: Stamp,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Stamp {
    path: PathBuf,
    bytes: u64,
    modified: Option<SystemTime>,
    readonly: bool,
    identity: (u64, u64, i64, i64, u32),
    hash: [u8; 32],
}

struct Job<'a> {
    cancel: &'a AtomicBool,
    deadline: Instant,
}
impl Job<'_> {
    fn check(&self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            Err("Project file job cancelled. Open buffers were preserved; refresh to check any completed save.".into())
        } else if Instant::now() >= self.deadline {
            Err("Project file job exceeded its ten-second limit. Use a smaller folder or narrower search.".into())
        } else {
            Ok(())
        }
    }
}

pub(super) fn execute(request: Request, cancel: &AtomicBool) -> Result<Outcome, String> {
    let job = Job {
        cancel,
        deadline: Instant::now() + JOB_TIMEOUT,
    };
    job.check()?;
    match request {
        Request::List {
            root,
            relative,
            show_hidden,
            show_ignored,
        } => {
            let root = project_root(&root)?;
            let (entries, truncated) = list(&root, &relative, show_hidden, show_ignored, &job)?;
            Ok(Outcome::Listed {
                relative,
                entries,
                truncated,
            })
        }
        Request::Search {
            root,
            query,
            show_hidden,
            show_ignored,
        } => {
            let root = project_root(&root)?;
            search(&root, &query, show_hidden, show_ignored, &job)
        }
        Request::Read { root, relative } => {
            let root = project_root(&root)?;
            Ok(Outcome::Opened(read_document(&root, &relative, &job)?))
        }
        Request::PreviewGlb {
            root,
            relative,
            expected,
        } => {
            let root = project_root(&root)?;
            preview_glb(&root, &relative, &expected, &job)
        }
        Request::Save {
            root,
            relative,
            expected,
            text,
        } => {
            let root = project_root(&root)?;
            save(&root, &relative, &expected, &text, &job)
        }
    }
}

struct ProjectRoot {
    path: PathBuf,
    metadata: Metadata,
}
impl std::ops::Deref for ProjectRoot {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}
impl AsRef<Path> for ProjectRoot {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        left.file_type() == right.file_type() && left.created().ok() == right.created().ok()
    }
}

fn project_root(path: &Path) -> Result<ProjectRoot, String> {
    let metadata =
        fs::symlink_metadata(path).map_err(|e| format!("Cannot open project folder: {e}"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("Choose a real project directory, not a symlink or special file.".into());
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project folder: {e}"))?;
    if canonical
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .any(restricted)
    {
        return Err("Credential and Git-internal folders cannot be used as a project root.".into());
    }
    let canonical_metadata = fs::symlink_metadata(&canonical)
        .map_err(|e| format!("Cannot inspect project root: {e}"))?;
    if canonical_metadata.file_type().is_symlink() || !same_identity(&metadata, &canonical_metadata)
    {
        return Err("Project root changed while opening. Choose the intended folder again.".into());
    }
    Ok(ProjectRoot {
        path: canonical,
        metadata: canonical_metadata,
    })
}

fn relative_path(relative: &str) -> Result<PathBuf, String> {
    if relative.len() > PATH_BYTES_LIMIT || relative.contains('\0') || relative.contains('\\') {
        return Err(
            "Unsupported project path. Use a relative path shorter than 4096 bytes.".into(),
        );
    }
    let path = Path::new(relative);
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Project paths must stay inside the selected folder; parent and absolute paths are blocked.".into());
    }
    Ok(path.to_path_buf())
}

/// Resolve each ancestor without following symlinks. The final symlink may be shown as metadata.
fn resolve(
    root: &ProjectRoot,
    relative: &str,
    allow_final_symlink: bool,
) -> Result<PathBuf, String> {
    let relative = relative_path(relative)?;
    let root_now = fs::symlink_metadata(&root.path)
        .map_err(|e| format!("Cannot inspect project root: {e}"))?;
    if root_now.file_type().is_symlink() || !same_identity(&root.metadata, &root_now) {
        return Err(
            "Project root changed. Reopen the intended project folder before continuing.".into(),
        );
    }
    let mut current = root.path.clone();
    let count = relative.components().count();
    for (i, component) in relative.components().enumerate() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)
            .map_err(|e| format!("Cannot inspect project path: {e}"))?;
        if metadata.file_type().is_symlink() {
            if i + 1 == count && allow_final_symlink {
                return Ok(current);
            }
            return Err(
                "Symlink traversal is blocked. Open the actual directory as a separate project."
                    .into(),
            );
        }
        if i + 1 != count && !metadata.is_dir() {
            return Err("A project path ancestor is not a directory.".into());
        }
    }
    let canonical = current
        .canonicalize()
        .map_err(|e| format!("Cannot resolve project path: {e}"))?;
    if !canonical.starts_with(&root.path) {
        return Err("Resolved path leaves the selected project folder.".into());
    }
    Ok(current)
}

fn restricted(relative: &str) -> bool {
    relative.split('/').any(|part| {
        let name = part.to_ascii_lowercase();
        name.starts_with(".env")
            || matches!(
                name.as_str(),
                ".git"
                    | ".ssh"
                    | ".aws"
                    | ".arkline-studio"
                    | ".gnupg"
                    | ".netrc"
                    | ".git-credentials"
                    | ".npmrc"
                    | ".pypirc"
                    | "credentials"
                    | "credentials.json"
                    | "id_rsa"
                    | "id_dsa"
                    | "id_ecdsa"
                    | "id_ed25519"
            )
            || [
                ".key",
                ".pem",
                ".token",
                ".p12",
                ".pfx",
                ".keychain",
                ".kdbx",
            ]
            .iter()
            .any(|ext| name.ends_with(ext))
    })
}

struct DirectoryItem {
    name: OsString,
    is_dir: bool,
    is_symlink: bool,
}

#[cfg(not(target_os = "macos"))]
fn standard_entries(path: &Path, job: &Job<'_>) -> Result<(Vec<DirectoryItem>, bool), String> {
    let reader = fs::read_dir(path).map_err(|e| format!("Cannot list held project folder: {e}"))?;
    let mut entries = Vec::new();
    for item in reader {
        job.check()?;
        if entries.len() >= LIST_SCAN_LIMIT {
            return Ok((entries, true));
        }
        let item = item.map_err(|e| format!("Cannot list project entry: {e}"))?;
        let kind = item
            .file_type()
            .map_err(|e| format!("Cannot inspect project entry: {e}"))?;
        entries.push(DirectoryItem {
            name: item.file_name(),
            is_dir: kind.is_dir(),
            is_symlink: kind.is_symlink(),
        });
    }
    Ok((entries, false))
}

fn list(
    root: &ProjectRoot,
    relative: &str,
    show_hidden: bool,
    show_ignored: bool,
    job: &Job<'_>,
) -> Result<(Vec<Entry>, bool), String> {
    job.check()?;
    if restricted(relative) {
        return Err("Credential and Git-internal directories are name-only; their contents are not browsed.".into());
    }
    let directory = resolve(root, relative, false)?;
    if !fs::symlink_metadata(&directory)
        .map_err(|e| e.to_string())?
        .is_dir()
    {
        return Err("Choose a project folder to list.".into());
    }
    let mut entries = Vec::new();
    let mut path_bytes = 0;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let held = anchored::directory(root, relative)?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let (reader, mut truncated) = anchored::entries(&held, job)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let (reader, mut truncated) = standard_entries(&directory, job)?;
    for item in reader {
        job.check()?;
        let Some(name) = item.name.to_str() else {
            truncated = true;
            continue;
        };
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let child = if relative.is_empty() {
            name.into()
        } else {
            format!("{relative}/{name}")
        };
        if child.len() > PATH_BYTES_LIMIT {
            truncated = true;
            continue;
        }
        path_bytes += child.len() + 1;
        if entries.len() >= LIST_LIMIT || path_bytes > BATCH_BYTES_LIMIT {
            truncated = true;
            break;
        }
        entries.push(Entry {
            restricted: restricted(&child),
            relative: child,
            name: name.into(),
            is_dir: item.is_dir,
            is_symlink: item.is_symlink,
            ignored: false,
        });
    }
    let ignored = ignored_paths(root, &entries, job)?;
    let tracked_ancestors = tracked_ancestors(root, &entries, &ignored, job)?;
    for entry in &mut entries {
        entry.ignored =
            ignored.contains(&entry.relative) && !tracked_ancestors.contains(&entry.relative);
    }
    entries.retain(|e| show_ignored || !e.ignored);
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok((entries, truncated))
}

fn search(
    root: &ProjectRoot,
    query: &str,
    show_hidden: bool,
    show_ignored: bool,
    job: &Job<'_>,
) -> Result<Outcome, String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() || query.len() > 256 {
        return Err("Enter a file or folder name to search (up to 256 bytes).".into());
    }
    let mut queue = VecDeque::from([(String::new(), 0)]);
    let mut paths = Vec::new();
    let mut visited = 0;
    let mut truncated = false;
    while let Some((directory, depth)) = queue.pop_front() {
        job.check()?;
        let (entries, more) = list(root, &directory, show_hidden, show_ignored, job)?;
        truncated |= more;
        for entry in entries {
            job.check()?;
            visited += 1;
            if visited > SEARCH_VISIT_LIMIT || paths.len() >= SEARCH_RESULT_LIMIT {
                truncated = true;
                break;
            }
            if !entry.is_dir && entry.relative.to_lowercase().contains(&query) {
                paths.push(entry.relative.clone());
            }
            if entry.is_dir && !entry.is_symlink && !entry.restricted {
                if depth < 32 {
                    queue.push_back((entry.relative, depth + 1));
                } else {
                    truncated = true;
                }
            }
        }
        if visited > SEARCH_VISIT_LIMIT || paths.len() >= SEARCH_RESULT_LIMIT {
            break;
        }
    }
    paths.sort();
    Ok(Outcome::Found { paths, truncated })
}

fn ignored_paths(
    root: &ProjectRoot,
    entries: &[Entry],
    job: &Job<'_>,
) -> Result<BTreeSet<String>, String> {
    if entries.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut input = Vec::new();
    for entry in entries {
        // check-ignore accepts pathname input, not literal-pathspec magic. The explicit ./
        // prefix prevents a filename beginning with :(...) from being parsed as magic.
        input.extend_from_slice(b"./");
        input.extend_from_slice(entry.relative.as_bytes());
        input.push(0);
    }
    let Some(bytes) = git_output(root, &["check-ignore", "-z", "--stdin"], input, job)? else {
        return Ok(BTreeSet::new());
    };
    Ok(bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            std::str::from_utf8(p)
                .ok()
                .map(|p| p.strip_prefix("./").unwrap_or(p).to_owned())
        })
        .collect())
}

fn tracked_ancestors(
    root: &ProjectRoot,
    entries: &[Entry],
    ignored: &BTreeSet<String>,
    job: &Job<'_>,
) -> Result<BTreeSet<String>, String> {
    let directories: Vec<_> = entries
        .iter()
        .filter(|e| e.is_dir && !e.is_symlink && !e.restricted && ignored.contains(&e.relative))
        .map(|e| e.relative.as_str())
        .collect();
    if directories.is_empty() {
        return Ok(BTreeSet::new());
    }
    let mut args = vec!["ls-files", "--cached", "-z", "--"];
    args.extend_from_slice(&directories);
    let Some(bytes) = git_output(root, &args, Vec::new(), job)? else {
        return Ok(BTreeSet::new());
    };
    let tracked: BTreeSet<_> = bytes
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .filter_map(|p| std::str::from_utf8(p).ok().map(String::from))
        .collect();
    let mut ancestors = BTreeSet::new();
    for directory in directories {
        job.check()?;
        let prefix = format!("{directory}/");
        if tracked.contains(directory)
            || tracked
                .range(prefix.clone()..)
                .next()
                .is_some_and(|path| path.starts_with(&prefix))
        {
            ancestors.insert(directory.into());
        }
    }
    Ok(ancestors)
}

fn git_output(
    root: &ProjectRoot,
    args: &[&str],
    input: Vec<u8>,
    job: &Job<'_>,
) -> Result<Option<Vec<u8>>, String> {
    job.check()?;
    let mut command = Command::new("git");
    command.arg("--no-pager");
    // check-ignore rejects --literal-pathspecs even for ordinary paths; ls-files needs it
    // because the directory arguments come from actual filesystem names.
    if args.first() != Some(&"check-ignore") {
        command.arg("--literal-pathspecs");
    }
    command
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "diff.external=",
            "-C",
        ])
        .arg(&root.path)
        .args(args)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_LITERAL_PATHSPECS")
        .env_remove("GIT_GLOB_PATHSPECS")
        .env_remove("GIT_NOGLOB_PATHSPECS")
        .env_remove("GIT_ICASE_PATHSPECS")
        .env_remove("GIT_CONFIG_PARAMETERS")
        .env("GIT_CONFIG_COUNT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("Cannot check Git ignore rules: {e}")),
    };
    let mut stdin = child.stdin.take().ok_or("Cannot open Git ignore input")?;
    let writer = thread::spawn(move || stdin.write_all(&input));
    let stdout = child.stdout.take().ok_or("Cannot read Git ignore result")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Cannot read Git ignore diagnostics")?;
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.take(GIT_OUTPUT_LIMIT + 1).read_to_end(&mut bytes);
        (result, bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stderr.take(GIT_OUTPUT_LIMIT + 1).read_to_end(&mut bytes);
        (result, bytes)
    });
    let status = loop {
        if let Err(error) = job.check() {
            let _ = child.kill();
            let _ = child.wait();
            // Git is killed before joining bounded pipe readers/writer.
            let _ = writer.join();
            let _ = out.join();
            let _ = err.join();
            return Err(error);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("Cannot wait for Git ignore check: {e}"));
            }
        }
    };
    let _ = writer.join();
    let (read_result, bytes) = out.join().map_err(|_| "Git ignore reader failed")?;
    let (_, diagnostics) = err
        .join()
        .map_err(|_| "Git ignore diagnostic reader failed")?;
    read_result.map_err(|e| format!("Cannot read Git ignore result: {e}"))?;
    if bytes.len() as u64 > GIT_OUTPUT_LIMIT || diagnostics.len() as u64 > GIT_OUTPUT_LIMIT {
        return Err("Git ignore output exceeded its limit.".into());
    }
    // A plain project folder or unavailable Git metadata has no ignored-file flag.
    if !status.success() && status.code() != Some(1) {
        if String::from_utf8_lossy(&diagnostics).contains("not a git repository") {
            return Ok(None);
        }
        return Err("Cannot evaluate Git ignore rules safely. Check repository ownership/configuration in Git, then refresh; no trust grants were changed.".into());
    }
    Ok(Some(bytes))
}

fn stamp(path: &Path, metadata: &Metadata, hash: [u8; 32]) -> Stamp {
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        (
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec(),
            metadata.mode(),
        )
    };
    #[cfg(not(unix))]
    let identity = (0, 0, 0, 0, 0);
    Stamp {
        path: path.into(),
        bytes: metadata.len(),
        modified: metadata.modified().ok(),
        readonly: metadata.permissions().readonly(),
        identity,
        hash,
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn open_regular(path: &Path) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    // Do not follow a file replaced by a symlink between inspection and opening.
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(target_os = "macos")]
        let nofollow = 0x100;
        #[cfg(not(target_os = "macos"))]
        let nofollow = 0x20000;
        options.custom_flags(nofollow);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Cannot open project file safely: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("Special files cannot be read or edited.".into());
    }
    Ok(file)
}

fn read_document(root: &ProjectRoot, relative: &str, job: &Job<'_>) -> Result<Document, String> {
    job.check()?;
    let path = resolve(root, relative, true)?;
    let before =
        fs::symlink_metadata(&path).map_err(|e| format!("Cannot inspect project file: {e}"))?;
    let kind = if before.file_type().is_symlink() {
        Some(DocumentKind::Symlink)
    } else if restricted(relative) {
        Some(DocumentKind::Restricted)
    } else if !before.is_file() {
        return Err("Folders and special files cannot be opened as text.".into());
    } else if before.len() > TEXT_LIMIT {
        Some(DocumentKind::Oversized)
    } else {
        None
    };
    if let Some(kind) = kind {
        return Ok(Document {
            relative: relative.into(),
            kind,
            text: None,
            stamp: stamp(&path, &before, [0; 32]),
            bytes: before.len(),
        });
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let mut file = anchored::read(root, relative)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let mut file = open_regular(&path)?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if stamp(&path, &before, [0; 32]) != stamp(&path, &opened, [0; 32]) {
        return Err("File changed while opening. Try opening it again.".into());
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut buffer = [0; 16 * 1024];
    loop {
        job.check()?;
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("Cannot read project file: {e}"))?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > TEXT_LIMIT as usize {
            return Err("File grew beyond the 1 MiB text limit while opening.".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    job.check()?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    let current = resolve(root, relative, false)?;
    let current_meta = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
    if stamp(&path, &before, [0; 32]) != stamp(&path, &after, [0; 32])
        || stamp(&path, &after, [0; 32]) != stamp(&current, &current_meta, [0; 32])
    {
        return Err(
            "File changed while opening. Your existing buffer was preserved; refresh explicitly."
                .into(),
        );
    }
    let hash = Sha256::digest(&bytes).into();
    let binary = bytes.contains(&0);
    let text = String::from_utf8(bytes).ok().filter(|_| !binary);
    Ok(Document {
        relative: relative.into(),
        kind: if text.is_some() {
            DocumentKind::Text
        } else {
            DocumentKind::Binary
        },
        text,
        stamp: stamp(&path, &after, hash),
        bytes: after.len(),
    })
}

fn preview_glb(
    root: &ProjectRoot,
    relative: &str,
    expected: &Stamp,
    job: &Job<'_>,
) -> Result<Outcome, String> {
    job.check()?;
    if restricted(relative) {
        return Err("Sensitive files cannot be sent to asset preview.".into());
    }
    if !Path::new(relative)
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("glb"))
    {
        return Err("This preview accepts saved GLB files only.".into());
    }
    let path = resolve(root, relative, false)?;
    let before =
        fs::symlink_metadata(&path).map_err(|e| format!("Cannot inspect preview file: {e}"))?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err("Only regular saved GLB files can be previewed.".into());
    }
    if before.len() > GLB_PREVIEW_LIMIT {
        return Err("GLB preview is limited to 16 MiB. The file remains unchanged.".into());
    }
    if stamp(&path, &before, expected.hash) != *expected {
        return Err(
            "Asset changed on disk since it was opened. Refresh the file before previewing.".into(),
        );
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let mut file = anchored::read(root, relative)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let mut file = open_regular(&path)?;
    let opened = file.metadata().map_err(|e| e.to_string())?;
    if stamp(&path, &before, [0; 32]) != stamp(&path, &opened, [0; 32]) {
        return Err(
            "Asset changed while preparing preview. Refresh the file before previewing.".into(),
        );
    }
    let mut bytes = Vec::with_capacity(before.len() as usize);
    let mut buffer = [0; 64 * 1024];
    loop {
        job.check()?;
        let count = file
            .read(&mut buffer)
            .map_err(|e| format!("Cannot read preview file: {e}"))?;
        if count == 0 {
            break;
        }
        if bytes.len() + count > GLB_PREVIEW_LIMIT as usize {
            return Err("Asset grew beyond the 16 MiB preview limit.".into());
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    job.check()?;
    let after = file.metadata().map_err(|e| e.to_string())?;
    let current = resolve(root, relative, false)?;
    let current_meta = fs::symlink_metadata(&current).map_err(|e| e.to_string())?;
    if stamp(&path, &before, [0; 32]) != stamp(&path, &after, [0; 32])
        || stamp(&path, &after, [0; 32]) != stamp(&current, &current_meta, [0; 32])
    {
        return Err(
            "Asset changed during preview read. Refresh the file; no candidate was staged.".into(),
        );
    }
    // Small documents already carry a full-content hash. Large metadata-only documents carry
    // no content hash; their explicit read is protected by the same file identity/stability checks.
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    if before.len() <= TEXT_LIMIT && hash != expected.hash {
        return Err("Asset contents changed since opening. Refresh before previewing.".into());
    }
    Ok(Outcome::PreviewGlb {
        relative: relative.into(),
        bytes,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
struct TempFile(PathBuf);
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn save(
    root: &ProjectRoot,
    relative: &str,
    expected: &Stamp,
    text: &str,
    job: &Job<'_>,
) -> Result<Outcome, String> {
    job.check()?;
    if text.len() as u64 > TEXT_LIMIT || text.contains('\0') {
        return Err("Only UTF-8 text up to 1 MiB without NUL bytes can be saved.".into());
    }
    let document = read_document(root, relative, job)?;
    if document.kind != DocumentKind::Text {
        return Err("This file is metadata-only and cannot be edited here.".into());
    }
    if document.stamp != *expected {
        return Err("File changed on disk since it was opened. Save blocked; your buffer and disk contents were preserved. Compare or reload explicitly.".into());
    }
    let path = resolve(root, relative, false)?;
    let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
    if metadata.permissions().readonly() {
        return Err("File is read-only. Save blocked; change its permissions outside the editor if intended.".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Err("File has multiple hard links. Atomic save is blocked to avoid changing link behavior.".into());
        }
    }
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let (mut temporary, mut file) = anchored::Prepared::create(root, relative)?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let (temporary, mut file) = {
        let parent = path.parent().ok_or("Project file has no parent folder")?;
        let mut prepared = None;
        for _ in 0..16 {
            job.check()?;
            let candidate = parent.join(format!(
                ".arkline-save-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => {
                    prepared = Some((TempFile(candidate), file));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(format!("Cannot prepare atomic save: {e}")),
            }
        }
        prepared.ok_or("Cannot allocate a unique atomic-save file")?
    };
    file.write_all(text.as_bytes())
        .map_err(|e| format!("Cannot write atomic-save file: {e}"))?;
    file.set_permissions(metadata.permissions())
        .map_err(|e| format!("Cannot preserve file permissions: {e}"))?;
    file.sync_all()
        .map_err(|e| format!("Cannot sync atomic-save file: {e}"))?;
    job.check()?;
    // Recheck both contents and the entire path immediately before the atomic replacement.
    let current = read_document(root, relative, job)?;
    if current.stamp != *expected {
        return Err("File changed during save preparation. Save blocked; your buffer and disk contents were preserved.".into());
    }
    resolve(root, relative, false)?;
    job.check()?;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    temporary.replace()?;
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fs::rename(&temporary.0, &path)
        .map_err(|e| format!("Atomic save failed; original file was preserved: {e}"))?;
    let written_metadata = file.metadata().map_err(|_| "Save completed, but its file metadata could not be read. Keep your buffer and refresh explicitly.")?;
    let written_stamp = stamp(
        &path,
        &written_metadata,
        Sha256::digest(text.as_bytes()).into(),
    );
    let saved = read_document(root, relative, job).map_err(|_| "Save may have completed, but the disk changed before verification. Your buffer is preserved; refresh explicitly before another save.")?;
    if saved.stamp != written_stamp {
        return Err("Save completed, but the file changed again before verification. Your buffer is preserved; compare or reload explicitly.".into());
    }
    Ok(Outcome::Saved {
        relative: relative.into(),
        stamp: written_stamp,
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
