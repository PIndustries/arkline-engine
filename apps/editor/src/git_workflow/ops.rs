//! Bounded Git operations. The caller owns the explicit trust decision and runs one job at a time.
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const FILE_LIMIT: u64 = 32 * 1024 * 1024;
const TOTAL_FILE_LIMIT: u64 = 128 * 1024 * 1024;
const LOCAL_TIMEOUT: Duration = Duration::from_secs(20);
const NETWORK_TIMEOUT: Duration = Duration::from_secs(90);
thread_local! { static JOB_DEADLINE: Cell<Option<Instant>> = const { Cell::new(None) }; }
thread_local! {
    static PRIVATE_INDEX: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    static OWNED_INDEX_LOCK: RefCell<Option<(PathBuf, FileIdentity)>> = const { RefCell::new(None) };
}
static NEXT_INDEX_TRANSACTION: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
thread_local! {
    static BEFORE_INDEX_LOCK: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    static AFTER_INDEX_LOCK: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    static BEFORE_COMMIT_CAS: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn set_before_index_lock_hook(hook: impl FnOnce() + 'static) {
    BEFORE_INDEX_LOCK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
pub(super) fn set_after_index_lock_hook(hook: impl FnOnce() + 'static) {
    AFTER_INDEX_LOCK.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[cfg(test)]
pub(super) fn set_before_commit_cas_hook(hook: impl FnOnce() + 'static) {
    BEFORE_COMMIT_CAS.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
}

#[derive(Clone, Debug, PartialEq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    created: Option<std::time::SystemTime>,
}

impl FileIdentity {
    fn of(metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                created: metadata.created().ok(),
            }
        }
    }
}

fn owns_index_lock(path: &Path) -> bool {
    OWNED_INDEX_LOCK.with(|owned| {
        owned
            .borrow()
            .as_ref()
            .is_some_and(|(owned_path, identity)| {
                path == owned_path
                    && fs::symlink_metadata(path).is_ok_and(|metadata| {
                        metadata.is_file() && FileIdentity::of(&metadata) == *identity
                    })
            })
    })
}

struct Deadline(Option<Instant>);
impl Drop for Deadline {
    fn drop(&mut self) {
        JOB_DEADLINE.with(|v| v.set(self.0));
    }
}

#[derive(Clone, Debug)]
pub(super) struct Entry {
    pub path: String,
    pub original_path: Option<String>,
    pub index: char,
    pub worktree: char,
}

impl Entry {
    pub fn staged(&self) -> bool {
        !matches!(self.index, ' ' | '?' | '!')
    }
    pub fn unstaged(&self) -> bool {
        self.worktree != ' ' || self.index == '?'
    }
    pub fn conflicted(&self) -> bool {
        self.index == 'U'
            || self.worktree == 'U'
            || matches!((self.index, self.worktree), ('A', 'A') | ('D', 'D'))
    }
}

#[derive(Clone, Debug)]
pub(super) struct Remote {
    pub name: String,
    /// Expanded push destination. Unsupported/credential-bearing URLs are redacted.
    pub push_url: String,
}

#[derive(Clone, Debug)]
pub(super) struct Snapshot {
    pub repo: PathBuf,
    pub branch: String,
    pub head: String,
    pub entries: Vec<Entry>,
    pub branches: Vec<String>,
    pub remotes: Vec<Remote>,
    pub fingerprint: String,
    /// Exact real-index bytes reviewed with this status; absent index has a distinct digest.
    pub index_digest: String,
}

#[derive(Clone, Debug)]
pub(super) enum Request {
    Inspect {
        path: PathBuf,
    },
    Clone {
        url: String,
        destination: PathBuf,
    },
    Diff {
        snapshot: Snapshot,
        paths: Vec<String>,
        staged: bool,
    },
    Stage {
        snapshot: Snapshot,
        paths: Vec<String>,
    },
    Unstage {
        snapshot: Snapshot,
        paths: Vec<String>,
    },
    Commit {
        snapshot: Snapshot,
        paths: Vec<String>,
        message: String,
    },
    CreateBranch {
        snapshot: Snapshot,
        name: String,
    },
    SwitchBranch {
        snapshot: Snapshot,
        name: String,
    },
    Push {
        snapshot: Snapshot,
        remote: String,
    },
    PrHandoff {
        snapshot: Snapshot,
        remote: String,
    },
}

#[derive(Debug, Default)]
pub(super) struct Outcome {
    pub snapshot: Option<Snapshot>,
    pub message: String,
    pub diff: Option<String>,
    pub handoff_url: Option<String>,
}

struct Output {
    bytes: Vec<u8>,
    stderr: Vec<u8>,
    success: bool,
}

fn checked(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Cancelled. Refresh repository status before another operation; a started mutation may have completed.".into())
    } else if JOB_DEADLINE.with(|v| v.get().is_some_and(|deadline| Instant::now() >= deadline)) {
        Err("Git job exceeded its two-minute limit. Refresh status; a started mutation or partial clone may remain.".into())
    } else {
        Ok(())
    }
}

fn command(repo: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    // These apply even to inspection: opening an untrusted checkout must not execute its hooks,
    // fsmonitor, external diff or text conversion. Credential helpers are deliberately retained.
    cmd.args([
        "--no-pager",
        "--literal-pathspecs",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "core.splitIndex=false",
        "-c",
        "submodule.recurse=false",
        "-c",
        "diff.external=",
        "-c",
        "diff.submodule=short",
        "-c",
        "color.ui=false",
        "-c",
        "credential.interactive=false",
        "-c",
        "protocol.allow=never",
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "protocol.https.allow=always",
        "-c",
        "protocol.ssh.allow=always",
        "-c",
        "protocol.file.allow=always",
    ]);
    if let Some(repo) = repo {
        cmd.arg("-C").arg(repo);
    }
    for name in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_PARAMETERS",
        "GIT_CONFIG_COUNT",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
    ] {
        cmd.env_remove(name);
    }
    PRIVATE_INDEX.with(|index| {
        if let Some(path) = index.borrow().as_ref() {
            cmd.env("GIT_INDEX_FILE", path);
        }
    });
    cmd.env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GIT_PAGER", "cat")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd
}

fn kill_owned(child: &mut std::process::Child) {
    // Kill only the process group created above, including Git's transport/helper children.
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            // End option parsing before the negative PGID. Older procps can
            // misparse it as an option and signal our own process group.
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &child.id().to_string(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn run_raw(
    repo: Option<&Path>,
    args: &[OsString],
    cancel: &AtomicBool,
    timeout: Duration,
    filters: &[String],
) -> Result<Output, String> {
    checked(cancel)?;
    let mut cmd = command(repo);
    for filter in filters {
        for (key, value) in [
            ("clean", ""),
            ("smudge", ""),
            ("process", ""),
            ("required", "false"),
        ] {
            cmd.arg("-c").arg(format!("{filter}.{key}={value}"));
        }
    }
    let mut child = cmd.args(args).spawn().map_err(|e| {
        format!("Cannot start installed Git: {e}. Install Git or make it available on PATH.")
    })?;
    let stdout = child.stdout.take().ok_or("Git stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("Git stderr unavailable")?;
    let over_limit = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    for (is_error, mut stream) in [
        (false, Box::new(stdout) as Box<dyn Read + Send>),
        (true, Box::new(stderr) as Box<dyn Read + Send>),
    ] {
        let tx = tx.clone();
        let over_limit = over_limit.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let mut buffer = [0u8; 8192];
            let result = loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break Ok(bytes),
                    Ok(n) => {
                        if bytes.len() + n > OUTPUT_LIMIT {
                            over_limit.store(true, Ordering::Relaxed);
                        } else {
                            bytes.extend_from_slice(&buffer[..n]);
                        }
                    }
                    Err(e) => break Err(e.to_string()),
                }
            };
            let _ = tx.send((is_error, result));
        });
    }
    drop(tx);
    let start = Instant::now();
    let status = loop {
        if let Err(error) = checked(cancel) {
            kill_owned(&mut child);
            return Err(error);
        }
        if cancel.load(Ordering::Relaxed)
            || start.elapsed() >= timeout
            || over_limit.load(Ordering::Relaxed)
        {
            kill_owned(&mut child);
            return Err(if cancel.load(Ordering::Relaxed) {
                "Cancelled. Refresh repository status; a started mutation may have completed."
                    .into()
            } else if over_limit.load(Ordering::Relaxed) {
                "Git output exceeded the 2 MiB limit. Narrow the selection, then refresh status."
                    .into()
            } else {
                "Git timed out. Refresh status and check authentication/network in your Git client before retrying.".into()
            });
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                kill_owned(&mut child);
                return Err(format!("Cannot wait for Git: {e}"));
            }
        }
    };
    let mut output = Output {
        bytes: Vec::new(),
        stderr: Vec::new(),
        success: status.success(),
    };
    for _ in 0..2 {
        let (is_error, result) = rx.recv_timeout(Duration::from_secs(1)).map_err(|_| {
            kill_owned(&mut child);
            "Git helper output did not finish; refresh before retrying".to_string()
        })?;
        let bytes = result.map_err(|e| format!("Cannot read Git output: {e}"))?;
        if is_error {
            output.stderr = bytes;
        } else {
            output.bytes = bytes;
        }
    }
    if over_limit.load(Ordering::Relaxed) {
        return Err("Git output exceeded the 2 MiB limit".into());
    }
    checked(cancel)?;
    Ok(output)
}

fn run(
    repo: Option<&Path>,
    values: &[OsString],
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<Output, String> {
    // Git status/diff can invoke clean filters too. Discover only configuration KEY NAMES,
    // then neutralize every configured driver before running any command on file contents.
    // `git config` itself does not invoke file filters, hooks or credential helpers.
    let probe = run_raw(
        repo,
        &args(&[
            "config",
            "--null",
            "--name-only",
            "--get-regexp",
            "^filter\\..*\\.(clean|smudge|process|required)$",
        ]),
        cancel,
        LOCAL_TIMEOUT,
        &[],
    )?;
    let mut filters = BTreeSet::new();
    if probe.success {
        for key in probe.bytes.split(|v| *v == 0).filter(|v| !v.is_empty()) {
            let key = text(key)?;
            if let Some((driver, _)) = key.rsplit_once('.') {
                filters.insert(driver.to_string());
            }
        }
    } else if !probe.stderr.is_empty() {
        return Err(format!(
            "Cannot inspect Git filter configuration safely: {}",
            readable(&probe.stderr)
        ));
    }
    run_raw(
        repo,
        values,
        cancel,
        timeout,
        &filters.into_iter().collect::<Vec<_>>(),
    )
}

fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
fn text(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| {
        "Git returned a non-UTF-8 path or output; use your Git client for this checkout".into()
    })
}
fn readable(bytes: &[u8]) -> String {
    // Do not expose terminal controls or accidental credential-bearing URL diagnostics.
    String::from_utf8_lossy(bytes)
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect::<String>()
        .split_whitespace()
        .map(|word| {
            if word.contains("://")
                && word
                    .split("://")
                    .nth(1)
                    .is_some_and(|v| v.split('/').next().unwrap_or("").contains('@'))
            {
                "[credential-bearing URL redacted]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
fn must(repo: Option<&Path>, values: &[&str], cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let out = run(repo, &args(values), cancel, LOCAL_TIMEOUT)?;
    if out.success {
        Ok(out.bytes)
    } else {
        Err(format!("Git failed: {}", readable(&out.stderr)))
    }
}
fn optional(repo: &Path, values: &[&str], cancel: &AtomicBool) -> Result<Option<Vec<u8>>, String> {
    let out = run(Some(repo), &args(values), cancel, LOCAL_TIMEOUT)?;
    Ok(out.success.then_some(out.bytes))
}

fn parse_status(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let mut records = bytes.split(|b| *b == 0).filter(|r| !r.is_empty());
    let mut result = Vec::new();
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            return Err("Unsupported Git status record".into());
        }
        let index = record[0] as char;
        let worktree = record[1] as char;
        let path = text(&record[3..])?;
        validate_path(&path)?;
        let original_path = if matches!(index, 'R' | 'C') || matches!(worktree, 'R' | 'C') {
            let original = text(records.next().ok_or("Incomplete Git rename status")?)?;
            validate_path(&original)?;
            Some(original)
        } else {
            None
        };
        result.push(Entry {
            path,
            original_path,
            index,
            worktree,
        });
    }
    Ok(result)
}

fn validate_path(path: &str) -> Result<(), String> {
    if path.is_empty()
        || path.contains('\0')
        || Path::new(path)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        Err("Unsupported repository path; paths must stay inside the checkout".into())
    } else {
        Ok(())
    }
}

fn hash_file(
    hash: &mut Sha256,
    path: &Path,
    total: &mut u64,
    cancel: &AtomicBool,
) -> Result<(), String> {
    checked(cancel)?;
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            hash.update(b"missing");
            Ok(())
        }
        Err(e) => Err(format!("Cannot inspect checkout file: {e}")),
        Ok(meta) if meta.file_type().is_symlink() => {
            hash.update(b"symlink");
            let target = fs::read_link(path).map_err(|e| format!("Cannot read symlink: {e}"))?;
            hash.update(target.as_os_str().as_encoded_bytes());
            Ok(())
        }
        Ok(meta) if meta.is_dir() => {
            hash.update(b"directory/submodule");
            Ok(())
        }
        Ok(meta) if meta.is_file() => {
            *total += meta.len();
            if meta.len() > FILE_LIMIT || *total > TOTAL_FILE_LIMIT {
                return Err("Changed files exceed the safe inspection limit (32 MiB/file, 128 MiB total). Use your Git client for this checkout.".into());
            }
            hash.update(meta.len().to_le_bytes());
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                hash.update(meta.permissions().mode().to_le_bytes());
            }
            let mut file =
                fs::File::open(path).map_err(|e| format!("Cannot read changed file: {e}"))?;
            let mut bytes = [0u8; 65536];
            loop {
                checked(cancel)?;
                let n = file
                    .read(&mut bytes)
                    .map_err(|e| format!("Cannot read changed file: {e}"))?;
                if n == 0 {
                    break;
                }
                hash.update(&bytes[..n]);
            }
            Ok(())
        }
        Ok(_) => Err("Special files are not supported in the Git workspace".into()),
    }
}

fn internal_path(repo: &Path, name: &str, cancel: &AtomicBool) -> Result<PathBuf, String> {
    let value = text(&must(
        Some(repo),
        &["rev-parse", "--git-path", name],
        cancel,
    )?)?;
    let path = PathBuf::from(value.trim_end_matches(['\r', '\n']));
    Ok(if path.is_absolute() {
        path
    } else {
        repo.join(path)
    })
}

fn read_index(path: &Path, cancel: &AtomicBool) -> Result<Option<Vec<u8>>, String> {
    checked(cancel)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("Cannot inspect Git index: {error}")),
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > FILE_LIMIT {
        return Err("The Git index is not a regular file under 32 MiB. Use your Git client for this checkout.".into());
    }
    let mut file =
        fs::File::open(path).map_err(|error| format!("Cannot read Git index: {error}"))?;
    if FileIdentity::of(&file.metadata().map_err(|error| error.to_string())?)
        != FileIdentity::of(&metadata)
    {
        return Err("Git index changed while being opened; refresh status".into());
    }
    let mut result = Vec::new();
    let mut buffer = [0u8; 65536];
    loop {
        checked(cancel)?;
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Cannot read Git index: {error}"))?;
        if count == 0 {
            break;
        }
        result.extend_from_slice(&buffer[..count]);
        if result.len() as u64 > FILE_LIMIT {
            return Err("Git index grew beyond the 32 MiB inspection limit; refresh status".into());
        }
    }
    Ok(Some(result))
}

fn digest_index(bytes: &Option<Vec<u8>>) -> String {
    let mut hash = Sha256::new();
    match bytes {
        Some(bytes) => {
            hash.update(b"present-index\0");
            hash.update(bytes);
        }
        None => hash.update(b"absent-index\0"),
    }
    format!("{:x}", hash.finalize())
}

fn current_branch(repo: &Path, cancel: &AtomicBool) -> Result<String, String> {
    let Some(bytes) = optional(repo, &["symbolic-ref", "--quiet", "HEAD"], cancel)? else {
        return Ok("(detached)".into());
    };
    let reference = text(&bytes)?;
    let reference = reference.trim_end_matches(['\r', '\n']);
    reference.strip_prefix("refs/heads/").filter(|name| !name.is_empty()).map(String::from)
        .ok_or_else(|| "HEAD points outside local branches. Select a local contribution branch in your Git client before using this workspace.".into())
}

fn reviewed_head(snapshot: &Snapshot, cancel: &AtomicBool) -> Result<(), String> {
    let head = optional(&snapshot.repo, &["rev-parse", "--verify", "HEAD"], cancel)?
        .map(|bytes| text(&bytes))
        .transpose()?
        .unwrap_or_default()
        .trim()
        .to_string();
    let branch = current_branch(&snapshot.repo, cancel)?;
    if head != snapshot.head || branch != snapshot.branch {
        return Err("Repository HEAD or branch changed since review. Refresh before retrying; no index was published.".into());
    }
    Ok(())
}

/// Own the normal Git index lock while running commands against an exact private copy.
/// Other Git processes cannot stage/commit/checkout into the reviewed transaction.
struct IndexTransaction {
    real_index: PathBuf,
    lock_path: PathBuf,
    lock_file: fs::File,
    original_permissions: Option<fs::Permissions>,
    private_directory: Option<PathBuf>,
    private_index: PathBuf,
    previous_owned_lock: Option<(PathBuf, FileIdentity)>,
}

impl IndexTransaction {
    fn acquire(snapshot: &Snapshot, cancel: &AtomicBool) -> Result<Self, String> {
        checked(cancel)?;
        #[cfg(test)]
        BEFORE_INDEX_LOCK.with(|slot| {
            if let Some(hook) = slot.borrow_mut().take() {
                hook();
            }
        });
        let real_index = internal_path(&snapshot.repo, "index", cancel)?;
        let mut lock_name = real_index.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let mut options = fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock_file = options.open(&lock_path).map_err(|_| "Git index is locked by another operation. Wait for that operation, then refresh; no lock was removed.".to_string())?;
        let identity = FileIdentity::of(&lock_file.metadata().map_err(|error| error.to_string())?);
        let previous_owned_lock =
            OWNED_INDEX_LOCK.with(|slot| slot.replace(Some((lock_path.clone(), identity))));
        let mut transaction = Self {
            real_index,
            lock_path,
            lock_file,
            original_permissions: None,
            private_directory: None,
            private_index: PathBuf::new(),
            previous_owned_lock,
        };
        // Full review comparison happens AFTER exclusive lock acquisition. The owned lock is
        // treated as absent only by this worker and only while its inode still matches.
        fresh(snapshot, cancel)?;
        no_in_progress_operation(&snapshot.repo, cancel)?;
        let bytes = read_index(&transaction.real_index, cancel)?;
        if digest_index(&bytes) != snapshot.index_digest {
            return Err("Staged index changed after review. Refresh and review the new staging; the other operation's index was preserved.".into());
        }
        transaction.original_permissions = if bytes.is_some() {
            Some(
                fs::symlink_metadata(&transaction.real_index)
                    .map_err(|error| format!("Cannot preserve Git index permissions: {error}"))?
                    .permissions(),
            )
        } else {
            None
        };
        reviewed_head(snapshot, cancel)?;
        let parent = transaction
            .real_index
            .parent()
            .ok_or("Cannot locate Git index directory")?;
        let mut directory = None;
        for _ in 0..16 {
            let sequence = NEXT_INDEX_TRANSACTION.fetch_add(1, Ordering::Relaxed);
            let candidate =
                parent.join(format!(".arkline-index-{}-{sequence}", std::process::id()));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&candidate) {
                Ok(()) => {
                    directory = Some(candidate);
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("Cannot create private Git index: {error}")),
            }
        }
        let directory = directory.ok_or("Cannot allocate a private Git index directory")?;
        transaction.private_index = directory.join("index");
        transaction.private_directory = Some(directory);
        if let Some(bytes) = bytes {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options
                .open(&transaction.private_index)
                .map_err(|error| format!("Cannot create private Git index: {error}"))?;
            file.write_all(&bytes)
                .map_err(|error| format!("Cannot copy reviewed Git index: {error}"))?;
        }
        // Custom index files must not retain split-index links whose shared file may be
        // resolved relative to a private directory. Materialize ONLY the private copy.
        if transaction.private_index.exists() {
            let _private = PrivateIndex::enter(&transaction.private_index);
            must(
                Some(&snapshot.repo),
                &["update-index", "--no-split-index"],
                cancel,
            )?;
        }
        #[cfg(test)]
        AFTER_INDEX_LOCK.with(|slot| {
            if let Some(hook) = slot.borrow_mut().take() {
                hook();
            }
        });
        Ok(transaction)
    }

    fn assert_owned(&self) -> Result<(), String> {
        if owns_index_lock(&self.lock_path) {
            Ok(())
        } else {
            Err("Git index lock changed outside the editor. Nothing was published; refresh and inspect it in your Git client.".into())
        }
    }

    fn publish(&mut self, snapshot: &Snapshot, cancel: &AtomicBool) -> Result<(), String> {
        checked(cancel)?;
        self.assert_owned()?;
        // Revalidate working-file bytes and the real index AFTER the private operation too.
        // A save in another app cannot silently stage different bytes than those reviewed.
        fresh(snapshot, cancel)?;
        if digest_index(&read_index(&self.real_index, cancel)?) != snapshot.index_digest {
            return Err("Real index changed outside its lock. The external index was preserved; refresh before retrying.".into());
        }
        let bytes = read_index(&self.private_index, cancel)?
            .ok_or("Private Git operation produced no index; the real index was preserved")?;
        self.lock_file
            .seek(SeekFrom::Start(0))
            .map_err(|error| error.to_string())?;
        self.lock_file
            .set_len(0)
            .map_err(|error| error.to_string())?;
        self.lock_file
            .write_all(&bytes)
            .map_err(|error| format!("Cannot prepare Git index publication: {error}"))?;
        if let Some(permissions) = &self.original_permissions {
            self.assert_owned()?;
            self.lock_file
                .set_permissions(permissions.clone())
                .map_err(|error| format!("Cannot preserve Git index permissions: {error}"))?;
        }
        self.lock_file
            .sync_all()
            .map_err(|error| format!("Cannot sync Git index publication: {error}"))?;
        checked(cancel)?;
        self.assert_owned()?;
        fs::rename(&self.lock_path, &self.real_index).map_err(|error| {
            format!("Cannot publish reviewed Git index: {error}; real index was not replaced")
        })?;
        Ok(())
    }
}

impl Drop for IndexTransaction {
    fn drop(&mut self) {
        if let Some(directory) = &self.private_directory {
            let _ = fs::remove_file(directory.join("index.lock"));
            let _ = fs::remove_file(directory.join("index"));
            let _ = fs::remove_dir(directory);
        }
        // Never unlink another process's replacement lock, even if someone removed ours.
        if owns_index_lock(&self.lock_path) {
            let _ = fs::remove_file(&self.lock_path);
        }
        OWNED_INDEX_LOCK.with(|slot| *slot.borrow_mut() = self.previous_owned_lock.take());
    }
}

struct PrivateIndex(Option<PathBuf>);
impl PrivateIndex {
    fn enter(path: &Path) -> Self {
        Self(PRIVATE_INDEX.with(|slot| slot.replace(Some(path.to_path_buf()))))
    }
}
impl Drop for PrivateIndex {
    fn drop(&mut self) {
        PRIVATE_INDEX.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

const OPERATION_MARKERS: [&str; 8] = [
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "rebase-merge",
    "rebase-apply",
    "sequencer",
    "BISECT_LOG",
    "index.lock",
];

fn no_in_progress_operation(repo: &Path, cancel: &AtomicBool) -> Result<(), String> {
    for name in OPERATION_MARKERS {
        let path = internal_path(repo, name, cancel)?;
        if fs::symlink_metadata(&path).is_ok() && !owns_index_lock(&path) {
            return Err("Another Git operation, merge/rebase/cherry-pick/bisect, or index lock is in progress. Finish it explicitly in your Git client, then refresh; this workspace will not complete or abort it.".into());
        }
    }
    Ok(())
}

fn no_symlink_ancestors(repo: &Path, relative: &str) -> Result<(), String> {
    let path = Path::new(relative);
    let mut parent = repo.to_path_buf();
    if let Some(parts) = path.parent() {
        for part in parts.components() {
            parent.push(part);
            if fs::symlink_metadata(&parent).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err("A changed path traverses a symlink directory. Use your Git client; the editor will not read or stage through it.".into());
            }
        }
    }
    Ok(())
}

pub(super) fn inspect(path: &Path, cancel: &AtomicBool) -> Result<Snapshot, String> {
    let path =
        fs::canonicalize(path).map_err(|e| format!("Cannot open repository directory: {e}"))?;
    if !path.is_dir() {
        return Err("Choose a repository directory".into());
    }
    let root = text(&must(
        Some(&path),
        &["rev-parse", "--show-toplevel"],
        cancel,
    )?)?;
    let repo = fs::canonicalize(root.trim_end_matches(['\r', '\n']))
        .map_err(|e| format!("Cannot resolve repository root: {e}"))?;
    let index_path = internal_path(&repo, "index", cancel)?;
    let index_digest = digest_index(&read_index(&index_path, cancel)?);
    let head = optional(&repo, &["rev-parse", "--verify", "HEAD"], cancel)?
        .map(|v| text(&v))
        .transpose()?
        .unwrap_or_default()
        .trim()
        .to_string();
    let branch = current_branch(&repo, cancel)?;
    // Do not recurse into submodule working files: their independent filter configuration is not
    // covered by this checkout's neutralization. Gitlink/index changes remain visible.
    let status = must(
        Some(&repo),
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=dirty",
        ],
        cancel,
    )?;
    let entries = parse_status(&status)?;
    let branches = text(&must(
        Some(&repo),
        &["for-each-ref", "--format=%(refname:strip=2)", "refs/heads"],
        cancel,
    )?)?
    .lines()
    .map(String::from)
    .collect();
    let remote_names = text(&must(Some(&repo), &["remote"], cancel)?)?;
    let mut remotes = Vec::new();
    for name in remote_names.lines() {
        let urls = text(&must(
            Some(&repo),
            &["remote", "get-url", "--push", "--all", "--", name],
            cancel,
        )?)?;
        let values: Vec<_> = urls.lines().collect();
        let push_url = if values.len() == 1 && validate_destination(values[0]).is_ok() {
            values[0].to_string()
        } else {
            "[unsupported or multiple push destinations]".into()
        };
        remotes.push(Remote {
            name: name.into(),
            push_url,
        });
    }
    let mut hash = Sha256::new();
    hash.update(repo.as_os_str().as_encoded_bytes());
    hash.update(branch.as_bytes());
    hash.update(head.as_bytes());
    hash.update(&status);
    hash.update(must(
        Some(&repo),
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00",
            "refs/heads",
        ],
        cancel,
    )?);
    // Hash configuration without displaying its values (which can contain credentials).
    hash.update(must(
        Some(&repo),
        &["config", "--null", "--list", "--show-origin"],
        cancel,
    )?);
    let mut total = 0;
    hash_file(&mut hash, &index_path, &mut total, cancel)?;
    for name in OPERATION_MARKERS {
        hash.update(name.as_bytes());
        let path = internal_path(&repo, name, cancel)?;
        if owns_index_lock(&path) {
            hash.update(b"missing");
        } else {
            hash_file(&mut hash, &path, &mut total, cancel)?;
        }
    }
    for entry in &entries {
        for name in std::iter::once(&entry.path).chain(entry.original_path.iter()) {
            no_symlink_ancestors(&repo, name)?;
            hash.update(name.as_bytes());
            hash_file(&mut hash, &repo.join(name), &mut total, cancel)?;
        }
    }
    if digest_index(&read_index(&index_path, cancel)?) != index_digest {
        return Err("Git index changed during status inspection. Refresh before reviewing or mutating files.".into());
    }
    // Prevent a mixed status/index/HEAD snapshot if another Git command finished mid-inspection.
    let latest_head = optional(&repo, &["rev-parse", "--verify", "HEAD"], cancel)?
        .map(|v| text(&v))
        .transpose()?
        .unwrap_or_default()
        .trim()
        .to_string();
    let latest_branch = current_branch(&repo, cancel)?;
    if head != latest_head || branch != latest_branch {
        return Err(
            "Git HEAD or branch changed during inspection. Refresh before reviewing files.".into(),
        );
    }
    Ok(Snapshot {
        repo,
        branch,
        head,
        entries,
        branches,
        remotes,
        fingerprint: format!("{:x}", hash.finalize()),
        index_digest,
    })
}

fn fresh(snapshot: &Snapshot, cancel: &AtomicBool) -> Result<Snapshot, String> {
    let current = inspect(&snapshot.repo, cancel)?;
    if current.fingerprint != snapshot.fingerprint {
        Err("Repository changed since review. Refresh status, review the new diff and select files again before retrying.".into())
    } else {
        Ok(current)
    }
}

fn selected<'a>(snapshot: &'a Snapshot, paths: &[String]) -> Result<Vec<&'a Entry>, String> {
    if paths.is_empty() {
        return Err("Select at least one changed file".into());
    }
    let mut unique = BTreeSet::new();
    let mut entries = Vec::new();
    for path in paths {
        validate_path(path)?;
        if !unique.insert(path) {
            return Err("Duplicate file selection; refresh status".into());
        }
        entries.push(
            snapshot
                .entries
                .iter()
                .find(|e| e.path == *path)
                .ok_or("Selected path is no longer in status; refresh")?,
        );
    }
    if entries.iter().any(|e| e.conflicted()) {
        return Err("Resolve merge conflicts in your Git client, then refresh. Conflict staging is not supported here.".into());
    }
    Ok(entries)
}

fn path_args(values: &[&str], entries: &[&Entry]) -> Vec<OsString> {
    let mut result = args(values);
    result.push("--".into());
    let mut paths = BTreeSet::new();
    for entry in entries {
        paths.insert(entry.path.clone());
        if let Some(original) = &entry.original_path {
            paths.insert(original.clone());
        }
    }
    result.extend(paths.into_iter().map(OsString::from));
    result
}

fn check_paths(snapshot: &Snapshot, entries: &[&Entry], cancel: &AtomicBool) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for entry in entries {
        if snapshot.repo.join(&entry.path).is_dir() {
            return Err(
                "Submodule/directory changes are not supported here; use your Git client".into(),
            );
        }
        for name in std::iter::once(&entry.path).chain(entry.original_path.iter()) {
            validate_path(name)?;
            no_symlink_ancestors(&snapshot.repo, name)?;
            names.insert(name);
        }
    }
    let names: Vec<_> = names.into_iter().collect();
    // Consult both working and cached attributes: changed .gitattributes must not conceal a filter.
    for cached in [false, true] {
        for batch in names.chunks(100) {
            let mut values = args(&["check-attr", "-z"]);
            if cached {
                values.push("--cached".into());
            }
            values.extend(args(&["filter", "--"]));
            values.extend(batch.iter().map(|name| OsString::from(name.as_str())));
            let out = run(Some(&snapshot.repo), &values, cancel, LOCAL_TIMEOUT)?;
            if !out.success {
                return Err(format!(
                    "Cannot verify file filters: {}",
                    readable(&out.stderr)
                ));
            }
            let fields: Vec<_> = out.bytes.split(|v| *v == 0).collect();
            for record in fields.chunks(3).filter(|v| v.len() == 3) {
                if !matches!(record[2], b"unspecified" | b"unset") {
                    return Err("Selected file uses a Git clean filter or LFS. Use your Git client to preserve its filter/index semantics, then refresh.".into());
                }
            }
        }
    }
    Ok(())
}

fn mutation(
    snapshot: &Snapshot,
    values: Vec<OsString>,
    cancel: &AtomicBool,
    message: &str,
) -> Result<Outcome, String> {
    checked(cancel)?;
    fresh(snapshot, cancel)?;
    no_in_progress_operation(&snapshot.repo, cancel)?;
    let out = run(Some(&snapshot.repo), &values, cancel, LOCAL_TIMEOUT)?;
    if !out.success {
        return Err("Git operation failed. Refresh status, then check repository trust, index locks and configured identity/signing in your Git client. Helper diagnostics are withheld because they may contain credentials; no retry or cleanup was attempted.".into());
    }
    Ok(Outcome {
        snapshot: Some(inspect(&snapshot.repo, cancel)?),
        message: message.into(),
        ..Outcome::default()
    })
}

fn index_mutation(
    snapshot: &Snapshot,
    values: Vec<OsString>,
    cancel: &AtomicBool,
    message: &str,
) -> Result<Outcome, String> {
    let mut transaction = IndexTransaction::acquire(snapshot, cancel)?;
    transaction.assert_owned()?;
    reviewed_head(snapshot, cancel)?;
    // The only command which receives GIT_INDEX_FILE is the owned operation. Inspection,
    // review comparisons, and lock discovery continue to inspect the real index.
    let out = {
        let _private = PrivateIndex::enter(&transaction.private_index);
        run(Some(&snapshot.repo), &values, cancel, LOCAL_TIMEOUT)?
    };
    if !out.success {
        return Err("Git operation failed against its private reviewed index. Real staging was preserved. Refresh and check configured identity/signing in your Git client; helper diagnostics are withheld because they may contain credentials.".into());
    }
    transaction.assert_owned()?;
    transaction.publish(snapshot, cancel)?;
    drop(transaction);
    Ok(Outcome {
        snapshot: Some(inspect(&snapshot.repo, cancel)?),
        message: message.into(),
        ..Outcome::default()
    })
}

fn commit_selection(
    snapshot: &Snapshot,
    message: &str,
    cancel: &AtomicBool,
) -> Result<Outcome, String> {
    if snapshot.branch == "(detached)" {
        return Err("Select a local contribution branch before committing in the editor. Detached HEAD was preserved.".into());
    }
    let transaction = IndexTransaction::acquire(snapshot, cancel)?;
    transaction.assert_owned()?;
    reviewed_head(snapshot, cancel)?;
    let signing = run(
        Some(&snapshot.repo),
        &args(&["config", "--bool", "--get", "commit.gpgSign"]),
        cancel,
        LOCAL_TIMEOUT,
    )?;
    let signing = if signing.success {
        match text(&signing.bytes)?.trim() {
            "true" => true,
            "false" => false,
            _ => {
                return Err(
                    "Cannot interpret configured Git commit signing; check it in your Git client"
                        .into(),
                )
            }
        }
    } else if signing.stderr.is_empty() {
        false
    } else {
        return Err("Cannot read configured Git commit signing; check it in your Git client. No signing settings were changed.".into());
    };
    let new_commit = {
        let _private = PrivateIndex::enter(&transaction.private_index);
        let tree = text(&must(Some(&snapshot.repo), &["write-tree"], cancel)?)?
            .trim()
            .to_string();
        if !valid_sha(&tree) {
            return Err(
                "Git did not produce a valid reviewed tree; no commit was published".into(),
            );
        }
        let mut values = args(&["commit-tree", &tree]);
        if !snapshot.head.is_empty() {
            values.extend(args(&["-p", &snapshot.head]));
        }
        if signing {
            values.push("-S".into());
        }
        values.extend(args(&["-m", message]));
        let out = run(Some(&snapshot.repo), &values, cancel, LOCAL_TIMEOUT)?;
        if !out.success {
            return Err("Could not create the reviewed commit. Real staging and branch were preserved. Check configured identity/signing in your Git client; helper diagnostics are withheld because they may contain credentials.".into());
        }
        let sha = text(&out.bytes)?.trim().to_string();
        if !valid_sha(&sha) {
            return Err("Git did not return a valid commit identity; no branch was updated".into());
        }
        sha
    };
    // Pin both the commit's parent and the ref update's expected old object. reset --soft does
    // not honor index.lock, so ordinary `git commit` would otherwise reread a different parent.
    transaction.assert_owned()?;
    fresh(snapshot, cancel)?;
    reviewed_head(snapshot, cancel)?;
    #[cfg(test)]
    BEFORE_COMMIT_CAS.with(|slot| {
        if let Some(hook) = slot.borrow_mut().take() {
            hook();
        }
    });
    let branch = format!("refs/heads/{}", snapshot.branch);
    let expected = if snapshot.head.is_empty() {
        "0".repeat(new_commit.len())
    } else {
        snapshot.head.clone()
    };
    let out = run(
        Some(&snapshot.repo),
        &args(&[
            "update-ref",
            "--no-deref",
            "-m",
            "Arkline editor: commit reviewed selection",
            &branch,
            &new_commit,
            &expected,
        ]),
        cancel,
        LOCAL_TIMEOUT,
    )?;
    if !out.success {
        return Err("Branch changed while committing or its reference is locked. Reviewed commit was not published; existing branch and staging were preserved. Refresh before retrying.".into());
    }
    transaction.assert_owned()?;
    // Keep the real index byte-for-byte. Its reviewed tree already equals the new commit.
    if digest_index(&read_index(&transaction.real_index, cancel)?) != snapshot.index_digest {
        return Err("Commit was created, but an outside operation changed the real index despite its lock. External staging was preserved. Refresh and inspect before retrying.".into());
    }
    drop(transaction);
    Ok(Outcome { snapshot: Some(inspect(&snapshot.repo, cancel)?), message: format!("Reviewed staged selection committed as {new_commit}. Configured signing was respected; hooks were disabled. Push remains a separate action."), ..Outcome::default() })
}

fn validate_branch(repo: &Path, name: &str, cancel: &AtomicBool) -> Result<(), String> {
    if name.is_empty()
        || name.starts_with('-')
        || name == "HEAD"
        || name.contains("@{")
        || name.contains(['\n', '\r', '\0'])
    {
        return Err("Enter a valid contribution branch name".into());
    }
    must(Some(repo), &["check-ref-format", "--branch", name], cancel)?;
    Ok(())
}

fn validate_destination(url: &str) -> Result<(), String> {
    if url.is_empty()
        || url.contains(['\n', '\r', '\0'])
        || url.starts_with('-')
        || url.contains("::")
    {
        return Err(
            "Unsupported Git destination; choose HTTPS, SSH or an absolute local path".into(),
        );
    }
    if let Some(local) = url.strip_prefix("file://") {
        if !local.starts_with('/') || local.contains(['?', '#']) {
            return Err("Only absolute local file URLs are supported".into());
        }
        return Ok(());
    }
    if Path::new(url).is_absolute() {
        return Ok(());
    }
    if let Some(rest) = url.strip_prefix("https://") {
        let host = rest.split('/').next().unwrap_or("");
        if host.is_empty()
            || host.contains('@')
            || rest.contains(['?', '#', '\\'])
            || !host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
            || host.starts_with('-')
        {
            return Err("HTTPS Git destinations must not contain embedded credentials, query strings or custom helpers".into());
        }
        return Ok(());
    }
    if let Some(rest) = url.strip_prefix("ssh://") {
        let authority = rest.split('/').next().unwrap_or("");
        let host = authority.rsplit('@').next().unwrap_or("");
        let user = authority.split_once('@').map(|v| v.0).unwrap_or("git");
        if !user.is_empty()
            && !user.starts_with('-')
            && user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
            && !host.is_empty()
            && !host.starts_with('-')
            && host
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'))
            && !rest.contains(['?', '#', '\\', ' '])
        {
            return Ok(());
        }
        return Err("Unsupported SSH Git destination".into());
    }
    if let Some((authority, path)) = url.split_once(':') {
        if let Some((user, host)) = authority.split_once('@') {
            if !user.is_empty()
                && !host.is_empty()
                && !path.is_empty()
                && !path.starts_with('-')
                && user
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                && !user.starts_with('-')
                && host
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
                && !host.starts_with('-')
                && !path.contains(['?', '#', '\\'])
            {
                return Ok(());
            }
        }
    }
    Err("Unsupported Git destination. External transport helpers and embedded credentials are not allowed.".into())
}

fn push_url(snapshot: &Snapshot, remote: &str, cancel: &AtomicBool) -> Result<String, String> {
    if !snapshot.remotes.iter().any(|r| r.name == remote) {
        return Err("Select an existing remote, then refresh".into());
    }
    let value = text(&must(
        Some(&snapshot.repo),
        &["remote", "get-url", "--push", "--all", "--", remote],
        cancel,
    )?)?;
    let urls: Vec<_> = value.lines().collect();
    if urls.len() != 1 {
        return Err(
            "Remote has multiple push URLs; use your Git client to choose one explicitly".into(),
        );
    }
    validate_destination(urls[0])?;
    no_rewrite(Some(&snapshot.repo), urls[0], cancel)?;
    Ok(urls[0].into())
}

fn no_rewrite(repo: Option<&Path>, url: &str, cancel: &AtomicBool) -> Result<(), String> {
    let out = run(
        repo,
        &args(&[
            "config",
            "--null",
            "--get-regexp",
            "^url\\..*\\.(insteadof|pushinsteadof)$",
        ]),
        cancel,
        LOCAL_TIMEOUT,
    )?;
    if !out.success && !out.stderr.is_empty() {
        return Err("Cannot verify Git URL rewrite rules safely".into());
    }
    for record in out.bytes.split(|v| *v == 0).filter(|v| !v.is_empty()) {
        let record = text(record)?;
        let (_, prefix) = record
            .split_once('\n')
            .ok_or("Cannot parse Git URL rewrite rules")?;
        if url.starts_with(prefix) {
            return Err("A Git URL rewrite would change the displayed destination. Use your Git client to reconcile the remote explicitly; the editor will not push/clone to a different destination.".into());
        }
    }
    Ok(())
}

fn remote_head(
    snapshot: &Snapshot,
    url: &str,
    cancel: &AtomicBool,
) -> Result<Option<String>, String> {
    let branch = format!("refs/heads/{}", snapshot.branch);
    let out = run(
        Some(&snapshot.repo),
        &args(&["ls-remote", "--heads", "--", url, &branch]),
        cancel,
        NETWORK_TIMEOUT,
    )?;
    if !out.success {
        return Err("Cannot verify remote branch. Check the displayed destination, existing transport authentication and network in your Git client. Helper diagnostics are withheld because they may contain credentials.".into());
    }
    let text = text(&out.bytes)?;
    let mut lines = text.lines();
    let Some(line) = lines.next() else {
        return Ok(None);
    };
    if lines.next().is_some() {
        return Err("Remote returned ambiguous branch identity".into());
    }
    let (sha, name) = line
        .split_once('\t')
        .ok_or("Invalid remote branch identity")?;
    if name != branch || !valid_sha(sha) {
        return Err("Invalid remote branch identity".into());
    }
    Ok(Some(sha.into()))
}
fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|v| v.is_ascii_hexdigit())
}
fn contribution(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.branch == "(detached)" || snapshot.head.is_empty() {
        Err(
            "Create/select a local branch with a commit before pushing or preparing a pull request"
                .into(),
        )
    } else {
        Ok(())
    }
}

fn clone_repo(url: &str, destination: &Path, cancel: &AtomicBool) -> Result<Outcome, String> {
    validate_destination(url)?;
    no_rewrite(None, url, cancel)?;
    if fs::symlink_metadata(destination).is_ok() {
        return Err("Clone destination already exists. Choose a new directory; nothing was removed or replaced.".into());
    }
    let parent = destination
        .parent()
        .ok_or("Choose a clone destination with an existing parent directory")?;
    let parent = fs::canonicalize(parent)
        .map_err(|e| format!("Clone parent directory is unavailable: {e}"))?;
    let name = destination
        .file_name()
        .ok_or("Choose a new directory name for the clone")?;
    let destination = parent.join(name);
    let mut values = args(&[
        "clone",
        "--no-checkout",
        "--no-recurse-submodules",
        "--origin",
        "origin",
        "--",
        url,
    ]);
    values.push(destination.as_os_str().into());
    let out = run(None, &values, cancel, NETWORK_TIMEOUT)?;
    if !out.success {
        return Err("Clone failed. Check destination, existing transport authentication and network in your Git client. The new directory may contain a partial clone; inspect it before retrying. Helper diagnostics are withheld because they may contain credentials.".into());
    }
    if optional(&destination, &["rev-parse", "--verify", "HEAD"], cancel)?.is_some() {
        // Load only the index first. Attribute inspection precedes any working-file checkout.
        must(Some(&destination), &["read-tree", "HEAD"], cancel)?;
        let names = must(Some(&destination), &["ls-files", "-z"], cancel)?;
        let probe = Snapshot {
            repo: destination.clone(),
            branch: String::new(),
            head: String::new(),
            entries: Vec::new(),
            branches: Vec::new(),
            remotes: Vec::new(),
            fingerprint: String::new(),
            index_digest: String::new(),
        };
        let entries = names
            .split(|v| *v == 0)
            .filter(|v| !v.is_empty())
            .map(|path| {
                Ok(Entry {
                    path: text(path)?,
                    original_path: None,
                    index: 'A',
                    worktree: ' ',
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        check_paths(&probe, &entries.iter().collect::<Vec<_>>(), cancel).map_err(|e| {
            format!(
                "Repository cloned without checkout at {}. {e}",
                destination.display()
            )
        })?;
        must(Some(&destination), &["checkout-index", "--all"], cancel)?;
    }
    Ok(Outcome { snapshot: Some(inspect(&destination, cancel)?), message: "Repository cloned into the new directory. Hooks and file filters were not executed; editor buffers were not changed.".into(), ..Outcome::default() })
}

fn github_repo(url: &str) -> Result<String, String> {
    let path = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("git@github.com:"))
        .ok_or(
            "Pull-request handoff supports clean github.com HTTPS or git@github.com remotes only",
        )?;
    let path = path.strip_suffix(".git").unwrap_or(path);
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || *part == "."
                || *part == ".."
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        })
    {
        return Err("Remote does not identify a valid GitHub owner/repository".into());
    }
    Ok(path.into())
}
fn encode(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(byte as char);
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}

pub(super) fn execute(request: Request, cancel: &AtomicBool) -> Result<Outcome, String> {
    let _deadline =
        Deadline(JOB_DEADLINE.with(|v| v.replace(Some(Instant::now() + Duration::from_secs(120)))));
    checked(cancel)?;
    match request {
        Request::Inspect { path } => Ok(Outcome { snapshot: Some(inspect(&path, cancel)?), message: "Repository status refreshed. Unsaved editor buffers are separate from files on disk.".into(), ..Outcome::default() }),
        Request::Clone { url, destination } => clone_repo(&url, &destination, cancel),
        Request::Diff { snapshot, paths, staged } => {
            let snapshot = fresh(&snapshot, cancel)?;
            let entries = selected(&snapshot, &paths)?;
            if entries.iter().any(|e| e.index == '?') {
                return Ok(Outcome { snapshot: Some(snapshot), message: "Untracked files have no Git diff yet. Review their saved contents in the editor before explicit staging.".into(), diff: Some("Untracked file: no index or committed version to compare.".into()), ..Outcome::default() });
            }
            let mut base = vec!["diff", "--no-ext-diff", "--no-textconv", "--no-color", "--no-renames"];
            if staged { base.push("--cached"); }
            let out = run(Some(&snapshot.repo), &path_args(&base, &entries), cancel, LOCAL_TIMEOUT)?;
            if !out.success { return Err(format!("Cannot read diff: {}", readable(&out.stderr))); }
            let diff = text(&out.bytes)?;
            Ok(Outcome { snapshot: Some(snapshot), message: "Saved-file Git diff. Binary changes are identified by Git; LFS content and external text conversions are not expanded.".into(), diff: Some(diff), ..Outcome::default() })
        }
        Request::Stage { snapshot, paths } => {
            let snapshot = fresh(&snapshot, cancel)?;
            let entries = selected(&snapshot, &paths)?;
            if entries.iter().any(|e| e.staged()) { return Err("Selected file already has staged changes, possibly partial staging. Its index was preserved. Use your Git client to reconcile it, then refresh.".into()); }
            check_paths(&snapshot, &entries, cancel)?;
            index_mutation(&snapshot, path_args(&["add"], &entries), cancel, "Selected saved files staged. Other index entries were preserved.")
        }
        Request::Unstage { snapshot, paths } => {
            let snapshot = fresh(&snapshot, cancel)?;
            let entries = selected(&snapshot, &paths)?;
            if entries.iter().any(|e| !e.staged() || e.unstaged()) { return Err("Unstage requires selected fully staged files without additional working changes. Partial staging was preserved; use your Git client to reconcile it.".into()); }
            if snapshot.head.is_empty() {
                index_mutation(&snapshot, path_args(&["rm", "--cached"], &entries), cancel, "Selected files unstaged; working files retained.")
            } else {
                index_mutation(&snapshot, path_args(&["restore", "--staged"], &entries), cancel, "Selected files unstaged; working files retained.")
            }
        }
        Request::Commit { snapshot, paths, message } => {
            let snapshot = fresh(&snapshot, cancel)?;
            let entries = selected(&snapshot, &paths)?;
            if message.trim().is_empty() || message.contains('\0') || message.len() > 16384 { return Err("Enter a nonempty commit message under 16 KiB".into()); }
            if entries.iter().any(|e| !e.staged() || e.unstaged()) { return Err("Commit requires selected staged files with no additional working changes. Review/stage saved changes explicitly; partial staging was preserved.".into()); }
            if snapshot.entries.iter().filter(|e| e.staged()).any(|e| !paths.contains(&e.path)) { return Err("Other files are already staged. Commit was blocked to preserve their exact index state. Select all intended staged files or reconcile staging in your Git client.".into()); }
            check_paths(&snapshot, &entries, cancel)?;
            let identity = run(Some(&snapshot.repo), &args(&["var", "GIT_AUTHOR_IDENT"]), cancel, LOCAL_TIMEOUT)?;
            if !identity.success { return Err("Git author identity is unavailable. Configure your name/email in your Git client, then retry. No identity was changed.".into()); }
            // No path-limited commit: that would take working-tree versions and bypass reviewed staging.
            commit_selection(&snapshot, &message, cancel)
        }
        Request::CreateBranch { snapshot, name } => branch_operation(snapshot, name, cancel, true),
        Request::SwitchBranch { snapshot, name } => branch_operation(snapshot, name, cancel, false),
        Request::Push { snapshot, remote } => {
            let snapshot = fresh(&snapshot, cancel)?;
            no_in_progress_operation(&snapshot.repo, cancel)?;
            contribution(&snapshot)?;
            let url = push_url(&snapshot, &remote, cancel)?;
            if let Some(remote_sha) = remote_head(&snapshot, &url, cancel)? {
                let ancestor = run(Some(&snapshot.repo), &args(&["merge-base", "--is-ancestor", &remote_sha, &snapshot.head]), cancel, LOCAL_TIMEOUT)?;
                if !ancestor.success { return Err("Remote branch diverged, advanced, or is not available locally. Fetch/reconcile it explicitly in your Git client; no force push was attempted.".into()); }
            }
            // Recheck after network preflight so changed local state cannot silently change what is published.
            fresh(&snapshot, cancel)?;
            let refspec = format!("{}:refs/heads/{}", snapshot.head, snapshot.branch);
            let out = run(Some(&snapshot.repo), &args(&["push", "--porcelain", "--no-verify", "--no-force", "--no-follow-tags", "--recurse-submodules=no", "--", &url, &refspec]), cancel, NETWORK_TIMEOUT)?;
            if !out.success { return Err("Normal push failed or was rejected. Check the displayed destination, remote divergence, existing transport authentication and network in your Git client. No force push was attempted; helper diagnostics are withheld because they may contain credentials.".into()); }
            if remote_head(&snapshot, &url, cancel)?.as_deref() != Some(snapshot.head.as_str()) { return Err("Push returned, but remote commit could not be reconciled with the reviewed local HEAD. Refresh and verify before any pull-request handoff.".into()); }
            let message = format!("Verified normal push: {remote} / {} → {}. No pull request was created.", snapshot.branch, snapshot.head);
            Ok(Outcome { snapshot: Some(inspect(&snapshot.repo, cancel)?), message, ..Outcome::default() })
        }
        Request::PrHandoff { snapshot, remote } => {
            let snapshot = fresh(&snapshot, cancel)?;
            no_in_progress_operation(&snapshot.repo, cancel)?;
            contribution(&snapshot)?;
            let url = push_url(&snapshot, &remote, cancel)?;
            let repo = github_repo(&url)?;
            if remote_head(&snapshot, &url, cancel)?.as_deref() != Some(snapshot.head.as_str()) { return Err("Push and verify this branch first. The remote branch does not equal reviewed local HEAD.".into()); }
            let out = run(Some(&snapshot.repo), &args(&["ls-remote", "--symref", "--", &url, "HEAD"]), cancel, NETWORK_TIMEOUT)?;
            if !out.success { return Err("Cannot verify GitHub default branch".into()); }
            let output = text(&out.bytes)?;
            let base = output.lines().find_map(|line| line.strip_prefix("ref: refs/heads/").and_then(|line| line.strip_suffix("\tHEAD"))).ok_or("Cannot identify GitHub default branch; open your repository in GitHub")?;
            if base == snapshot.branch { return Err("Select a contribution branch different from the GitHub default branch".into()); }
            fresh(&snapshot, cancel)?;
            let handoff_url = format!("https://github.com/{repo}/compare/{}...{}?quick_pull=1", encode(base), encode(&snapshot.branch));
            Ok(Outcome { snapshot: Some(snapshot), message: "Verified branch; GitHub compare form is ready. Choose Create draft pull request and submit there. Opening the link does not create or submit a pull request.".into(), handoff_url: Some(handoff_url), ..Outcome::default() })
        }
    }
}

fn branch_operation(
    snapshot: Snapshot,
    name: String,
    cancel: &AtomicBool,
    create: bool,
) -> Result<Outcome, String> {
    let snapshot = fresh(&snapshot, cancel)?;
    if !snapshot.entries.is_empty() {
        return Err("Branch changes require a clean checkout. Saved staged, unstaged and untracked work was preserved; reconcile it explicitly before switching.".into());
    }
    validate_branch(&snapshot.repo, &name, cancel)?;
    no_in_progress_operation(&snapshot.repo, cancel)?;
    let flags = must(Some(&snapshot.repo), &["ls-files", "-v", "-z"], cancel)?;
    if flags
        .split(|v| *v == 0)
        .filter(|v| !v.is_empty())
        .any(|v| v[0] == b'S' || v[0].is_ascii_lowercase())
    {
        return Err("Checkout has skip-worktree or assume-unchanged entries which may hide saved changes. Use your Git client before switching branches.".into());
    }
    let target = if create {
        "HEAD".to_string()
    } else {
        format!("refs/heads/{name}")
    };
    if !snapshot.head.is_empty() {
        branch_filter_check(&snapshot.repo, &target, cancel)?;
    }
    if create {
        mutation(
            &snapshot,
            args(&["switch", "--no-overwrite-ignore", "--no-track", "-c", &name]),
            cancel,
            "New local contribution branch selected. Editor buffers were not reloaded.",
        )
    } else {
        if !snapshot.branches.contains(&name) {
            return Err(
                "Select an existing local branch; remote branch creation is a separate action"
                    .into(),
            );
        }
        mutation(
            &snapshot,
            args(&["switch", "--no-overwrite-ignore", "--no-guess", &name]),
            cancel,
            "Local branch selected. Editor buffers were not reloaded.",
        )
    }
}

fn branch_filter_check(repo: &Path, target: &str, cancel: &AtomicBool) -> Result<(), String> {
    let names = must(
        Some(repo),
        &["ls-tree", "-r", "--name-only", "-z", target],
        cancel,
    )?;
    // Batches keep argument sizes bounded. Git >= 2.43 supports source-tree attribute inspection.
    let names = names
        .split(|v| *v == 0)
        .filter(|v| !v.is_empty())
        .map(text)
        .collect::<Result<Vec<_>, _>>()?;
    for batch in names.chunks(100) {
        let mut values = args(&[
            "check-attr",
            "-z",
            &format!("--source={target}"),
            "filter",
            "--",
        ]);
        values.extend(batch.iter().map(OsString::from));
        let out = run(Some(repo), &values, cancel, LOCAL_TIMEOUT)?;
        if !out.success {
            return Err("Cannot safely inspect target-branch filters. Branch switching needs Git 2.43+; use your Git client for this checkout.".into());
        }
        let fields = out.bytes.split(|v| *v == 0).collect::<Vec<_>>();
        for record in fields.chunks(3).filter(|v| v.len() == 3) {
            if !matches!(record[2], b"unspecified" | b"unset") {
                return Err("Target branch uses Git clean/smudge filters or LFS. Use your Git client to switch it; no file filters were executed.".into());
            }
        }
    }
    Ok(())
}
