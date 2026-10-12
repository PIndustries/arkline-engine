//! Read-only GitHub workspace. Authentication stays with `gh`; no tokens enter editor state.
use serde_json::Value;
use std::{
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};
pub const OFFICIAL_REPOSITORY: &str = "PIndustries/arkline-engine";
pub const OFFICIAL_PROJECT_URL: &str = "https://github.com/orgs/PIndustries/projects/14";
const OUTPUT_LIMIT: usize = 2 * 1024 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(25);

#[derive(Clone, Debug, Default)]
pub struct Entry {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub url: String,
    pub state: String,
    pub labels: Vec<String>,
}
#[derive(Clone, Debug, Default)]
pub struct Project {
    pub title: String,
    pub description: String,
    pub url: String,
    pub items: Vec<Entry>,
}
#[derive(Clone, Debug, Default)]
pub struct WikiPage {
    pub title: String,
    pub body: String,
    pub url: String,
}
#[derive(Clone, Debug, Default)]
pub struct WorkspaceState {
    pub repository: String,
    pub loading: bool,
    pub refreshed_at_ms: Option<u64>,
    pub stale: bool,
    pub authenticated: bool,
    pub login: Option<String>,
    pub error: Option<String>,
    /// Section-specific failures never masquerade as an empty successful section.
    pub warnings: Vec<String>,
    pub issues: Vec<Entry>,
    pub pulls: Vec<Entry>,
    pub milestones: Vec<Entry>,
    pub projects: Vec<Project>,
    /// Official upstream wiki, even when viewing a linked fork.
    pub wiki: Vec<WikiPage>,
}
pub struct GitHubWorkspace {
    state: WorkspaceState,
    pending: Option<mpsc::Receiver<WorkspaceState>>,
    cancel: Arc<AtomicBool>,
}
impl Default for GitHubWorkspace {
    fn default() -> Self {
        Self {
            state: WorkspaceState {
                repository: OFFICIAL_REPOSITORY.into(),
                ..Default::default()
            },
            pending: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl GitHubWorkspace {
    pub fn state(&self) -> &WorkspaceState {
        &self.state
    }
    pub fn request_refresh(&mut self, repository: &str) -> Result<(), String> {
        let repository = validate_repository(repository)?;
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let (tx, rx) = mpsc::channel();
        let previous = if self.state.repository == repository {
            Some(self.state.clone())
        } else {
            None
        };
        self.state = previous.clone().unwrap_or_else(|| WorkspaceState {
            repository: repository.clone(),
            ..Default::default()
        });
        self.state.loading = true;
        self.state.stale = true;
        self.pending = Some(rx);
        std::thread::spawn(move || {
            let mut state = load(&repository, &cancel);
            retain_failed_sections(&mut state, previous.as_ref());
            if !cancel.load(Ordering::Relaxed) {
                let _ = tx.send(state);
            }
        });
        Ok(())
    }
    /// Call every frame; this only drains a channel and never waits for a process.
    pub fn poll(&mut self) {
        if let Some(receiver) = &self.pending {
            match receiver.try_recv() {
                Ok(state) => {
                    self.state = state;
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.state.loading = false;
                    self.state.error = Some("GitHub worker stopped; refresh to retry.".into());
                    self.pending = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
}
impl Drop for GitHubWorkspace {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
pub fn validate_repository(input: &str) -> Result<String, String> {
    let value = input.trim();
    let Some((owner, repo)) = value.split_once('/') else {
        return Err("Use a GitHub repository in owner/name format.".into());
    };
    if owner.is_empty()
        || owner.len() > 39
        || owner.starts_with('-')
        || owner.ends_with('-')
        || !owner
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || repo.is_empty()
        || repo.len() > 100
        || matches!(repo, "." | "..")
        || !repo
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
    {
        return Err("Use a valid GitHub owner/name, without a URL or extra path.".into());
    }
    Ok(value.into())
}
fn text(v: &Value, key: &str) -> String {
    v[key]
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(32_768)
        .collect()
}
fn entries(value: Value, issues: bool) -> Vec<Entry> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| !issues || v.get("pull_request").is_none())
        .take(100)
        .map(|v| Entry {
            number: v["number"].as_u64().unwrap_or_default(),
            title: text(v, "title"),
            body: {
                let body = text(v, "body");
                if body.is_empty() {
                    text(v, "description")
                } else {
                    body
                }
            },
            url: text(v, "html_url"),
            state: text(v, "state"),
            labels: v["labels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v["name"].as_str().map(str::to_owned))
                .collect(),
        })
        .collect()
}
fn executable(path: &std::path::Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}
fn discover_program(paths: impl IntoIterator<Item = PathBuf>, fallbacks: &[PathBuf]) -> PathBuf {
    let name = if cfg!(windows) { "gh.exe" } else { "gh" };
    paths
        .into_iter()
        .map(|p| p.join(name))
        .chain(fallbacks.iter().cloned())
        .find(|p| executable(p))
        .unwrap_or_else(|| name.into())
}
fn github_program() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    #[cfg(target_os = "macos")]
    let fallback = [
        PathBuf::from("/opt/homebrew/bin/gh"),
        PathBuf::from("/usr/local/bin/gh"),
    ];
    #[cfg(not(target_os = "macos"))]
    let fallback: [PathBuf; 0] = [];
    discover_program(std::env::split_paths(&path), &fallback)
}
fn api(args: &[String], cancel: &AtomicBool) -> Result<Value, String> {
    let mut command = Command::new(github_program());
    command
        .arg("api")
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat");
    let bytes = bounded(&mut command, cancel, COMMAND_TIMEOUT, OUTPUT_LIMIT)?;
    serde_json::from_slice(&bytes)
        .map_err(|_| "GitHub returned an invalid or unsupported response.".into())
}
fn load(repository: &str, cancel: &AtomicBool) -> WorkspaceState {
    let mut state = WorkspaceState {
        repository: repository.into(),
        ..Default::default()
    };
    match api(&["user".into()], cancel) {
        Ok(user) => {
            state.authenticated = true;
            state.login = user["login"].as_str().map(str::to_owned);
        }
        Err(_) => {
            state.error = Some("GitHub CLI authentication unavailable. Install GitHub CLI from https://cli.github.com and run gh auth login outside the editor, then refresh.".into());
        }
    }
    if state.authenticated {
        for (section, endpoint) in [
            ("Issues", "issues?state=all&sort=updated&per_page=60"),
            ("Pull requests", "pulls?state=all&sort=updated&per_page=40"),
            ("Milestones", "milestones?state=all&per_page=50"),
        ] {
            if cancel.load(Ordering::Relaxed) {
                return state;
            }
            match api(&[format!("repos/{repository}/{endpoint}")], cancel) {
                Ok(v) => {
                    let result = entries(v, section == "Issues");
                    match section {
                        "Issues" => state.issues = result,
                        "Pull requests" => state.pulls = result,
                        _ => state.milestones = result,
                    }
                }
                Err(error) => state.warnings.push(format!("{section}: {error}")),
            }
        }
        let (owner, name) = repository.split_once('/').unwrap();
        let query = "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){projectsV2(first:10){nodes{title url shortDescription items(first:50){nodes{content{... on Issue{title number url body} ... on PullRequest{title number url body}} fieldValues(first:10){nodes{... on ProjectV2ItemFieldSingleSelectValue{name field{... on ProjectV2SingleSelectField{name}}}}}}}}}}}";
        match api(&["graphql".into(), "-f".into(),format!("query={query}"),"-f".into(),format!("owner={owner}"),"-f".into(),format!("name={name}")], cancel) {
        Ok(v) if v.get("errors").is_none() => {
            for project in v["data"]["repository"]["projectsV2"]["nodes"].as_array().into_iter().flatten() {
                state.projects.push(Project { title:text(project,"title"),description:text(project,"shortDescription"),url:text(project,"url"),items:project["items"]["nodes"].as_array().into_iter().flatten().filter(|i| i["content"].is_object()).map(|i| {
                    let content=&i["content"];
                    let status=i["fieldValues"]["nodes"].as_array().into_iter().flatten().find(|v| v["field"]["name"].as_str()==Some("Status")).map(|v|text(v,"name")).unwrap_or_default();
                    Entry { title:text(content,"title"),body:text(content,"body"),url:text(content,"url"),number:content["number"].as_u64().unwrap_or_default(),state:status,labels:vec![] }
                }).collect() });
            }
        }
        _ => state.warnings.push("Projects unavailable. The gh account may need read:project permission; no permission changes were made.".into()),
    }
    }
    if !cancel.load(Ordering::Relaxed) {
        match wiki(cancel) {
            Ok(pages) => state.wiki = pages,
            Err(error) => state.warnings.push(format!("Official wiki: {error}")),
        }
    }
    state.refreshed_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64);
    state.stale = state.error.is_some() || !state.warnings.is_empty();
    state
}
fn retain_failed_sections(state: &mut WorkspaceState, previous: Option<&WorkspaceState>) {
    let Some(previous) = previous else {
        return;
    };
    let failed =
        |name: &str| state.error.is_some() || state.warnings.iter().any(|w| w.starts_with(name));
    if failed("Issues:") {
        state.issues = previous.issues.clone();
    }
    if failed("Pull requests:") {
        state.pulls = previous.pulls.clone();
    }
    if failed("Milestones:") {
        state.milestones = previous.milestones.clone();
    }
    if failed("Projects") {
        state.projects = previous.projects.clone();
    }
    if state
        .warnings
        .iter()
        .any(|w| w.starts_with("Official wiki:"))
    {
        state.wiki = previous.wiki.clone();
    }
}
/// Both output size and total process lifetime are bounded, including a stalled pipe.
fn bounded(
    command: &mut Command,
    cancel: &AtomicBool,
    timeout: Duration,
    limit: usize,
) -> Result<Vec<u8>, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|_| "Required GitHub CLI or Git executable could not start.".to_string())?;
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + timeout;
    let mut output = None;
    loop {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if cancel.load(Ordering::Relaxed) {
                "Request cancelled."
            } else {
                "GitHub operation timed out; refresh to retry."
            }
            .into());
        }
        if output.is_none() {
            match rx.try_recv() {
                Ok(Ok(bytes)) if bytes.len() <= limit => output = Some(bytes),
                Ok(_) | Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(
                        "GitHub response exceeded its safe limit or could not be read.".into(),
                    );
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        match child.try_wait() {
            Ok(Some(status)) if !status.success() => {
                return Err(
                    "GitHub request failed. Check CLI sign-in, repository access, or connectivity."
                        .into(),
                )
            }
            Ok(Some(_)) if output.is_some() => return Ok(output.unwrap()),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("GitHub process could not be monitored.".into());
            }
            _ => {}
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
struct TemporaryWiki(PathBuf);
impl Drop for TemporaryWiki {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn wiki(cancel: &AtomicBool) -> Result<Vec<WikiPage>, String> {
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "System clock unavailable")?
        .as_nanos();
    let temporary = TemporaryWiki(std::env::temp_dir().join(format!(
        "arkline-editor-wiki-{}-{stamp}",
        std::process::id()
    )));
    std::fs::create_dir(&temporary.0).map_err(|_| "Cannot create temporary wiki directory")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temporary.0, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot protect temporary wiki directory")?;
    }
    let git = || {
        let mut c = Command::new("git");
        c.env("GIT_TERMINAL_PROMPT", "0")
            .arg("-c")
            .arg("credential.helper=")
            .arg("-c")
            .arg("core.hooksPath=/dev/null");
        c
    };
    bounded(
        git()
            .args([
                "clone",
                "--depth=1",
                "--no-checkout",
                "--quiet",
                "https://github.com/PIndustries/arkline-engine.wiki.git",
            ])
            .arg(&temporary.0),
        cancel,
        deadline.saturating_duration_since(Instant::now()),
        OUTPUT_LIMIT,
    )?;
    let paths = bounded(
        git()
            .arg("-C")
            .arg(&temporary.0)
            .args(["ls-tree", "-r", "--name-only", "HEAD"]),
        cancel,
        deadline.saturating_duration_since(Instant::now()),
        OUTPUT_LIMIT,
    )?;
    let paths = String::from_utf8(paths).map_err(|_| "Wiki contains unsupported paths")?;
    let mut pages = Vec::new();
    let mut total = 0;
    for path in paths.lines().filter(|p| p.ends_with(".md")).take(80) {
        let body = bounded(
            git()
                .arg("-C")
                .arg(&temporary.0)
                .args(["show", &format!("HEAD:{path}")]),
            cancel,
            deadline.saturating_duration_since(Instant::now()),
            256 * 1024,
        )?;
        total += body.len();
        if total > OUTPUT_LIMIT {
            break;
        }
        let title = path.trim_end_matches(".md").to_owned();
        pages.push(WikiPage {
            url: format!(
                "https://github.com/{OFFICIAL_REPOSITORY}/wiki/{}",
                title.replace(' ', "%20")
            ),
            title,
            body: String::from_utf8(body).map_err(|_| "Wiki page is not UTF-8")?,
        });
    }
    pages.sort_by(|a, b| {
        (a.title != "Home")
            .cmp(&(b.title != "Home"))
            .then(a.title.cmp(&b.title))
    });
    Ok(pages)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn native_app_discovery_uses_explicit_fallback_without_changing_path() {
        let before = std::env::var_os("PATH");
        assert_eq!(
            discover_program(Vec::new(), &[PathBuf::from("/bin/sh")]),
            PathBuf::from("/bin/sh")
        );
        assert_eq!(
            discover_program(Vec::new(), &[PathBuf::from("/not-installed/gh")]),
            PathBuf::from("gh")
        );
        assert_eq!(std::env::var_os("PATH"), before);
    }
    #[test]
    fn repository_input_cannot_become_options_or_extra_paths() {
        for bad in [
            "--help",
            "https://github.com/a/b",
            "a/b/c",
            "../x",
            "a/b?x=y",
            "a/b\n--help",
            "a/..",
            "-a/b",
            "a/b;whoami",
        ] {
            assert!(validate_repository(bad).is_err(), "{bad}");
        }
        assert_eq!(
            validate_repository(" PIndustries/arkline-engine ").unwrap(),
            OFFICIAL_REPOSITORY
        );
    }
    #[test]
    fn issue_parser_excludes_pull_requests_and_preserves_body() {
        let list = entries(
            serde_json::json!([{"number":2,"title":"Issue","body":"Actual body","html_url":"https://github.com/a/b/issues/2","state":"open","labels":[{"name":"bug"}]},{"number":3,"pull_request":{},"title":"PR"}]),
            true,
        );
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].body, "Actual body");
        assert_eq!(list[0].labels, ["bug"]);
    }
    #[test]
    fn failed_refresh_retains_only_failed_sections() {
        let previous = WorkspaceState {
            issues: vec![Entry {
                title: "Saved issue".into(),
                ..Default::default()
            }],
            pulls: vec![Entry {
                title: "Old PR".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut current = WorkspaceState {
            warnings: vec!["Issues: timeout".into()],
            pulls: vec![Entry {
                title: "Fresh PR".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        retain_failed_sections(&mut current, Some(&previous));
        assert_eq!(current.issues[0].title, "Saved issue");
        assert_eq!(current.pulls[0].title, "Fresh PR");
    }
    #[cfg(unix)]
    #[test]
    fn process_timeout_output_limit_and_cancellation_are_bounded() {
        let cancel = AtomicBool::new(false);
        let start = Instant::now();
        assert!(bounded(
            Command::new("/bin/sleep").arg("2"),
            &cancel,
            Duration::from_millis(40),
            64
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(bounded(
            &mut Command::new("/usr/bin/yes"),
            &cancel,
            Duration::from_secs(2),
            64
        )
        .is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(bounded(
            Command::new("/bin/sleep").arg("2"),
            &cancel,
            Duration::from_secs(2),
            64
        )
        .is_err());
    }
    #[test]
    #[ignore = "explicit live read-only GitHub CLI acceptance"]
    fn live_official_read_only_workspace() {
        let state = load(OFFICIAL_REPOSITORY, &AtomicBool::new(false));
        assert!(state.authenticated, "{:?}", state.error);
        assert!(!state.issues.is_empty());
        assert!(!state.projects.is_empty(), "{:?}", state.warnings);
        assert!(
            state
                .wiki
                .iter()
                .any(|p| p.title == "Home" && !p.body.is_empty()),
            "{:?}",
            state.warnings
        );
        println!(
            "Read-only verified: {} issues, {} PRs, {} milestones, {} projects, {} wiki pages",
            state.issues.len(),
            state.pulls.len(),
            state.milestones.len(),
            state.projects.len(),
            state.wiki.len()
        );
    }
}
