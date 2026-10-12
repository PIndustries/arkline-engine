//! Read-only direct GitHub API workspace; credentials never enter display state or logs.
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
    token: Option<String>,
}
impl Default for GitHubWorkspace {
    fn default() -> Self {
        Self {
            state: WorkspaceState {
                repository: OFFICIAL_REPOSITORY.into(),
                ..Default::default()
            },
            pending: None,
            token: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl GitHubWorkspace {
    /// Changing identity invalidates every pending result and all account-scoped data.
    pub fn set_token(&mut self, token: Option<String>) -> bool {
        let token = token.filter(|t| !t.is_empty());
        if self.token == token {
            return false;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.pending = None;
        self.token = token;
        self.state = WorkspaceState {
            repository: self.state.repository.clone(),
            ..Default::default()
        };
        true
    }
    pub fn state(&self) -> &WorkspaceState {
        &self.state
    }
    pub fn request_refresh(&mut self, repository: &str) -> Result<(), String> {
        let repository = validate_repository(repository)?;
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let token = self.token.clone();
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
            let mut state = load(&repository, token.as_deref(), &cancel);
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
fn api_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .user_agent("Arkline-Editor")
        .build()
        .map_err(|_| "GitHub HTTP client unavailable.".into())
}
fn decode_response(status: u16, reader: impl Read, cancel: &AtomicBool) -> Result<Value, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Request cancelled.".into());
    }
    if !(200..300).contains(&status) {
        return Err(match status {
            401 => "GitHub sign-in was rejected; sign in again.",
            403 | 429 => "GitHub access or rate limit prevents this request.",
            404 => "GitHub repository or resource is unavailable to this account.",
            300..=399 => "GitHub redirect refused.",
            _ => "GitHub request failed; retry later.",
        }
        .into());
    }
    let mut bytes = Vec::new();
    reader
        .take(OUTPUT_LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "GitHub response could not be read before the deadline.".to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Request cancelled.".into());
    }
    if bytes.len() > OUTPUT_LIMIT {
        return Err("GitHub response exceeded its safe limit.".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "GitHub returned an invalid response.".into())
}
fn api(
    client: &reqwest::blocking::Client,
    path: &str,
    query: Option<Value>,
    token: Option<&str>,
    cancel: &AtomicBool,
) -> Result<Value, String> {
    if cancel.load(Ordering::Relaxed) {
        return Err("Request cancelled.".into());
    }
    // Callers supply internal paths only; never accept a URL or redirect credentials.
    if path.starts_with('/') || path.contains("://") || path.contains('#') {
        return Err("Invalid GitHub API path.".into());
    }
    let url = format!("https://api.github.com/{path}");
    let mut request = match query {
        Some(body) => client.post(url).json(&body),
        None => client.get(url),
    };
    request = request
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        let mut header = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| "Invalid GitHub credential format.".to_string())?;
        header.set_sensitive(true);
        request = request.header(reqwest::header::AUTHORIZATION, header);
    }
    let response = request
        .send()
        .map_err(|_| "GitHub connection failed or timed out.".to_string())?;
    decode_response(response.status().as_u16(), response, cancel)
}
fn load(repository: &str, token: Option<&str>, cancel: &AtomicBool) -> WorkspaceState {
    let mut state = WorkspaceState {
        repository: repository.into(),
        ..Default::default()
    };
    let client = match api_client() {
        Ok(client) => client,
        Err(error) => {
            state.error = Some(error);
            return state;
        }
    };
    let mut verified_token = None;
    if let Some(token) = token {
        match api(&client, "user", None, Some(token), cancel) {
            Ok(user) if user["login"].as_str().is_some_and(|s| !s.is_empty()) => {
                state.authenticated = true;
                state.login = user["login"].as_str().map(str::to_owned);
                verified_token = Some(token);
            }
            _ => state.error = Some("GitHub sign-in could not be verified. Public data is shown anonymously; sign in again or retry.".into()),
        }
    }
    {
        for (section, endpoint) in [
            ("Issues", "issues?state=all&sort=updated&per_page=60"),
            ("Pull requests", "pulls?state=all&sort=updated&per_page=40"),
            ("Milestones", "milestones?state=all&per_page=50"),
        ] {
            if cancel.load(Ordering::Relaxed) {
                return state;
            }
            match api(
                &client,
                &format!("repos/{repository}/{endpoint}"),
                None,
                verified_token,
                cancel,
            ) {
                Ok(v) if v.is_array() => {
                    let result = entries(v, section == "Issues");
                    match section {
                        "Issues" => state.issues = result,
                        "Pull requests" => state.pulls = result,
                        _ => state.milestones = result,
                    }
                }
                Ok(_) => state
                    .warnings
                    .push(format!("{section}: unsupported response format.")),
                Err(error) => state.warnings.push(format!("{section}: {error}")),
            }
        }
        let (owner, name) = repository.split_once('/').unwrap();
        let query = "query($owner:String!,$name:String!){repository(owner:$owner,name:$name){projectsV2(first:10){nodes{title url shortDescription items(first:50){nodes{content{... on Issue{title number url body} ... on PullRequest{title number url body}} fieldValues(first:10){nodes{... on ProjectV2ItemFieldSingleSelectValue{name field{... on ProjectV2SingleSelectField{name}}}}}}}}}}}";
        let projects = if verified_token.is_some() {
            api(
                &client,
                "graphql",
                Some(serde_json::json!({"query":query,"variables":{"owner":owner,"name":name}})),
                verified_token,
                cancel,
            )
        } else {
            Err("Sign-in required".into())
        };
        match projects {
        Ok(v) if v.get("errors").is_none() && v["data"]["repository"]["projectsV2"]["nodes"].is_array() => {
            for project in v["data"]["repository"]["projectsV2"]["nodes"].as_array().into_iter().flatten() {
                state.projects.push(Project { title:text(project,"title"),description:text(project,"shortDescription"),url:text(project,"url"),items:project["items"]["nodes"].as_array().into_iter().flatten().filter(|i| i["content"].is_object()).map(|i| {
                    let content=&i["content"];
                    let status=i["fieldValues"]["nodes"].as_array().into_iter().flatten().find(|v| v["field"]["name"].as_str()==Some("Status")).map(|v|text(v,"name")).unwrap_or_default();
                    Entry { title:text(content,"title"),body:text(content,"body"),url:text(content,"url"),number:content["number"].as_u64().unwrap_or_default(),state:status,labels:vec![] }
                }).collect() });
            }
        }
        _ => state.warnings.push("Projects unavailable. Sign-in with project access may be required; no permission changes were made.".into()),
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
    // Cached authenticated content must never be presented as anonymous/public,
    // nor carried across a different verified identity or repository.
    let same_identity = match (state.authenticated, previous.authenticated) {
        (false, false) => true,
        (true, true) => state.login.is_some() && state.login == previous.login,
        _ => false,
    };
    if !same_identity || state.repository != previous.repository {
        return;
    }
    let failed = |name: &str| state.warnings.iter().any(|w| w.starts_with(name));
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
        .map_err(|_| "Git executable could not start for the public wiki.".to_string())?;
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
                    "Public wiki request failed. Check repository access or connectivity.".into(),
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
    #[test]
    fn response_bounds_errors_and_cancellation_never_echo_body() {
        let cancel = AtomicBool::new(false);
        assert_eq!(
            decode_response(200, br#"{"login":"sample"}"#.as_slice(), &cancel).unwrap()["login"],
            "sample"
        );
        assert!(decode_response(200, vec![b' '; OUTPUT_LIMIT + 1].as_slice(), &cancel).is_err());
        assert!(decode_response(200, b"not json".as_slice(), &cancel).is_err());
        for status in [301, 401, 403, 404, 429, 500] {
            let error =
                decode_response(status, b"sensitive diagnostic".as_slice(), &cancel).unwrap_err();
            assert!(!error.contains("sensitive"));
        }
        cancel.store(true, Ordering::Relaxed);
        assert!(decode_response(200, b"{}".as_slice(), &cancel).is_err());
    }
    #[test]
    fn http_redirect_is_not_followed_and_error_body_is_not_displayed() {
        use std::{io::Write, net::TcpListener};
        let target = TcpListener::bind("127.0.0.1:0").unwrap();
        target.set_nonblocking(true).unwrap();
        let source = TcpListener::bind("127.0.0.1:0").unwrap();
        let source_addr = source.local_addr().unwrap();
        let target_addr = target.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut socket, _) = source.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0u8; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            write!(socket,"HTTP/1.1 302 Found\r\nLocation: http://{target_addr}/secret\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let response = api_client()
            .unwrap()
            .get(format!("http://{source_addr}/"))
            .send()
            .unwrap();
        assert_eq!(response.status().as_u16(), 302);
        assert!(decode_response(302, response, &AtomicBool::new(false))
            .unwrap_err()
            .contains("redirect"));
        worker.join().unwrap();
        assert!(matches!(target.accept(),Err(e) if e.kind()==std::io::ErrorKind::WouldBlock));
    }
    #[test]
    fn identity_change_discards_previous_cache_and_pending_response() {
        let mut workspace = GitHubWorkspace::default();
        workspace.set_token(Some("first-test-credential".into()));
        workspace.state.authenticated = true;
        workspace.state.login = Some("old-account".into());
        workspace.state.issues.push(Entry::default());
        let (tx, rx) = mpsc::channel();
        workspace.pending = Some(rx);
        let old_cancel = workspace.cancel.clone();
        workspace.set_token(Some("second-test-credential".into()));
        assert!(old_cancel.load(Ordering::Relaxed));
        assert!(tx.send(WorkspaceState::default()).is_err());
        workspace.poll();
        assert!(!workspace.state.authenticated);
        assert!(workspace.state.login.is_none() && workspace.state.issues.is_empty());
        workspace.set_token(None);
        assert!(workspace.token.is_none());
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
    #[test]
    fn revoked_or_different_identity_never_retains_authenticated_sections() {
        let previous = WorkspaceState {
            authenticated: true,
            login: Some("original-owner".into()),
            repository: OFFICIAL_REPOSITORY.into(),
            issues: vec![Entry {
                title: "Private issue".into(),
                ..Default::default()
            }],
            projects: vec![Project {
                title: "Private project".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        for (authenticated, login) in [
            (false, None),
            (true, Some("other-owner".into())),
            (true, None),
        ] {
            let mut current = WorkspaceState {
                authenticated,
                login,
                repository: OFFICIAL_REPOSITORY.into(),
                warnings: vec!["Issues: unavailable".into(), "Projects unavailable".into()],
                ..Default::default()
            };
            retain_failed_sections(&mut current, Some(&previous));
            assert!(current.issues.is_empty() && current.projects.is_empty());
        }
        let mut same = WorkspaceState {
            authenticated: true,
            login: previous.login.clone(),
            repository: OFFICIAL_REPOSITORY.into(),
            warnings: vec!["Projects unavailable".into()],
            ..Default::default()
        };
        retain_failed_sections(&mut same, Some(&previous));
        assert_eq!(same.projects[0].title, "Private project");
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
    #[ignore = "explicit live anonymous read-only GitHub API acceptance"]
    fn live_official_read_only_workspace() {
        let state = load(OFFICIAL_REPOSITORY, None, &AtomicBool::new(false));
        assert!(!state.authenticated);
        assert!(!state.issues.is_empty());
        assert!(state.warnings.iter().any(|w| w.starts_with("Projects")));
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
