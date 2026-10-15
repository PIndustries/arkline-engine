//! Read-only direct GitHub API workspace; credentials never enter display state or logs.
use serde_json::Value;
use std::{
    io::Read,
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
// Bounds apply before public wiki content is retained in memory.
const WIKI_PAGE_LIMIT: usize = 256 * 1024;
const WIKI_PAGE_COUNT: usize = 80;
const WIKI_RAW_ROOT: &str = "https://raw.githubusercontent.com/wiki/PIndustries/arkline-engine/";
/// Only flat official-wiki page names, never arbitrary URLs, query strings or paths.
fn wiki_slug(link: &str) -> Option<String> {
    let path = link.trim().split('#').next()?;
    let path = if path.starts_with("https://") {
        path.strip_prefix("https://github.com/PIndustries/arkline-engine/wiki/")?
    } else {
        path.strip_prefix("./").unwrap_or(path)
    };
    let mut decoded = Vec::new();
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hi = (bytes.next()? as char).to_digit(16)?;
            let lo = (bytes.next()? as char).to_digit(16)?;
            decoded.push((hi * 16 + lo) as u8);
        } else {
            decoded.push(byte);
        }
    }
    let decoded = String::from_utf8(decoded).ok()?;
    let path = decoded.strip_suffix(".md").unwrap_or(&decoded);
    if path.is_empty()
        || path.len() > 160
        || path == "."
        || path == ".."
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_ .".contains(&b))
    {
        return None;
    }
    Some(path.replace(' ', "-"))
}
fn wiki_links(body: &str) -> Vec<String> {
    let mut links = std::collections::BTreeSet::new();
    for tail in body.split("](").skip(1) {
        if let Some((link, _)) = tail.split_once(')') {
            if let Some(slug) = wiki_slug(link) {
                links.insert(slug);
            }
        }
        if links.len() >= WIKI_PAGE_COUNT {
            break;
        }
    }
    // GitHub's alternative [[Page]] and [[Label|Page]] wiki links.
    for tail in body.split("[[").skip(1) {
        if let Some((link, _)) = tail.split_once("]]") {
            if let Some(slug) = wiki_slug(link.rsplit('|').next().unwrap_or(link)) {
                links.insert(slug);
            }
        }
        if links.len() >= WIKI_PAGE_COUNT {
            break;
        }
    }
    links.into_iter().take(WIKI_PAGE_COUNT).collect()
}
fn wiki_body(reader: impl Read, remaining: usize, cancel: &AtomicBool) -> Result<String, String> {
    let limit = remaining.min(WIKI_PAGE_LIMIT);
    let mut bytes = Vec::new();
    let mut reader = reader.take(limit as u64 + 1);
    let mut chunk = [0; 8192];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Request cancelled.".into());
        }
        let count = reader
            .read(&mut chunk)
            .map_err(|_| "Wiki response could not be read before its deadline")?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
        if bytes.len() > limit {
            return Err("Wiki page or total content exceeded its safe limit.".into());
        }
    }
    String::from_utf8(bytes).map_err(|_| "Wiki page is not UTF-8".into())
}
/// Bounded linked-page traversal. No repository clone, files, credentials or images.
/// Pages absent from Home/sidebar/footer and their links are intentionally not indexed.
fn collect_wiki(
    cancel: &AtomicBool,
    mut fetch: impl FnMut(&str, usize) -> Result<Option<String>, String>,
) -> Result<Vec<WikiPage>, String> {
    let mut queue =
        std::collections::VecDeque::from(["Home".to_owned(), "_Sidebar".into(), "_Footer".into()]);
    let mut seen =
        std::collections::BTreeSet::from(["Home".to_owned(), "_Sidebar".into(), "_Footer".into()]);
    let mut pages = Vec::new();
    let mut total = 0;
    while let Some(title) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Request cancelled.".into());
        }
        let Some(body) = fetch(&title, OUTPUT_LIMIT - total)? else {
            continue;
        };
        if body.len() > WIKI_PAGE_LIMIT || body.len() > OUTPUT_LIMIT - total {
            return Err("Wiki content exceeded its safe limit.".into());
        }
        total += body.len();
        for link in wiki_links(&body) {
            if seen.len() >= WIKI_PAGE_COUNT {
                break;
            }
            if seen.insert(link.clone()) {
                queue.push_back(link);
            }
        }
        pages.push(WikiPage {
            url: format!("https://github.com/{OFFICIAL_REPOSITORY}/wiki/{title}"),
            title,
            body,
        });
    }
    if !pages.iter().any(|p| p.title == "Home") {
        return Err("Official wiki Home page unavailable.".into());
    }
    pages.sort_by(|a, b| {
        (a.title != "Home")
            .cmp(&(b.title != "Home"))
            .then(a.title.cmp(&b.title))
    });
    Ok(pages)
}
fn wiki(cancel: &AtomicBool) -> Result<Vec<WikiPage>, String> {
    let deadline = Instant::now() + COMMAND_TIMEOUT;
    let client = api_client()?;
    collect_wiki(cancel, |slug, remaining| {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err("Wiki retrieval timed out; refresh to retry.".into());
        }
        // A separate anonymous client: access tokens are never passed to raw content.
        let response = client
            .get(format!("{WIKI_RAW_ROOT}{slug}.md"))
            .timeout(timeout)
            .send()
            .map_err(|_| "Public wiki connection failed or timed out".to_string())?;
        if response.status().as_u16() == 404 {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(format!(
                "Public wiki returned HTTP {}",
                response.status().as_u16()
            ));
        }
        wiki_body(response, remaining, cancel).map(Some)
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wiki_links_are_same_repository_flat_decoded_and_bounded() {
        for valid in [
            "Ship Design",
            "Ship%20Design.md#rooms",
            "./Ship-Design",
            "https://github.com/PIndustries/arkline-engine/wiki/Ship-Design#rooms",
        ] {
            assert_eq!(wiki_slug(valid).as_deref(), Some("Ship-Design"));
        }
        for bad in [
            "../secret",
            "%2e%2e",
            "%2e%2e%2fsecret",
            "Page%2fOther",
            "https://evil.test/Home",
            "https://github.com/Other/repo/wiki/Home",
            "//evil/Home",
            "Page?token=anything",
            "%ZZ",
            "Page%00",
            "#only",
        ] {
            assert!(wiki_slug(bad).is_none(), "{bad}");
        }
        assert_eq!(
            wiki_links("[A](./Home.md#x) [[Label|Ship Design]] [bad](https://evil.test/)"),
            vec!["Home", "Ship-Design"]
        );
        let many = (0..500)
            .map(|n| format!("[Page](Page-{n})"))
            .collect::<String>();
        assert_eq!(wiki_links(&many).len(), WIKI_PAGE_COUNT);
    }
    #[test]
    fn wiki_content_limits_cancellation_and_discovery_never_require_disk() {
        let cancel = AtomicBool::new(false);
        assert_eq!(wiki_body(&b"body"[..], 4, &cancel).unwrap(), "body");
        assert!(wiki_body(&b"large"[..], 4, &cancel).is_err());
        assert!(wiki_body(
            vec![b'a'; WIKI_PAGE_LIMIT + 1].as_slice(),
            OUTPUT_LIMIT,
            &cancel
        )
        .is_err());
        let mut requests = 0;
        let pages = collect_wiki(&cancel, |title, _| {
            requests += 1;
            Ok(Some(if title == "Home" {
                (0..200).map(|n| format!("[Page](Page-{n})")).collect()
            } else {
                "[Home](Home)".into()
            }))
        })
        .unwrap();
        assert_eq!(requests, WIKI_PAGE_COUNT);
        assert_eq!(pages.len(), WIKI_PAGE_COUNT);
        assert_eq!(pages[0].title, "Home");
        let oversized = collect_wiki(&cancel, |title, _| {
            Ok(Some(format!(
                "[next]({title}x){}",
                "a".repeat(WIKI_PAGE_LIMIT - 180)
            )))
        });
        assert!(oversized.is_err());
        cancel.store(true, Ordering::Relaxed);
        assert!(wiki_body(&b"body"[..], OUTPUT_LIMIT, &cancel).is_err());
        assert!(collect_wiki(&cancel, |_, _| panic!("cancelled request must not fetch")).is_err());
    }
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
