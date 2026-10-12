//! Native GitHub device authorization. Credentials never enter project files or logs.
use serde::Deserialize;
use serde_json::Value;
use std::{
    io::Read,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const DEVICE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/user";
const VERIFY_URL: &str = "https://github.com/login/device";
const MAX_RESPONSE: u64 = 64 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthPhase {
    Unconfigured,
    Restoring,
    SignedOut,
    Requesting,
    AwaitingAuthorization,
    SignedIn,
    Error,
}
#[derive(Clone, Debug)]
pub struct AuthState {
    pub phase: AuthPhase,
    pub status: String,
    pub login: Option<String>,
    pub verification_url: Option<String>,
    pub user_code: Option<String>,
    pub expires_at_ms: Option<u64>,
    pub persistent: bool,
}
impl AuthState {
    fn status(phase: AuthPhase, status: &str) -> Self {
        Self {
            phase,
            status: status.into(),
            login: None,
            verification_url: None,
            user_code: None,
            expires_at_ms: None,
            persistent: false,
        }
    }
}
// Intentionally no Debug: token values are never formatted.
#[derive(serde::Serialize, Deserialize)]
struct Credential {
    token: String,
    expires_at_ms: Option<u64>,
}
enum Command {
    Start(u64),
    Cancel(u64),
    SignOut(u64),
    Shutdown,
}
enum Event {
    State(u64, AuthState),
    Authorized(u64, AuthState, Credential),
}
pub struct GithubAuth {
    state: AuthState,
    token: Option<Credential>,
    generation: Arc<AtomicU64>,
    commands: SyncSender<Command>,
    events: Receiver<Event>,
    configured: bool,
}
impl GithubAuth {
    pub fn new(client_id: String) -> Self {
        let client_id = if client_id.trim().is_empty() {
            std::env::var("ARKLINE_GITHUB_CLIENT_ID").unwrap_or_default()
        } else {
            client_id
        };
        let configured = valid_client_id(&client_id);
        let (tx, rx) = mpsc::sync_channel(8);
        let (events, receive) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        if configured {
            let current = generation.clone();
            std::thread::spawn(move || worker(client_id, rx, events, current));
        }
        Self {
            state: AuthState::status(
                if configured {
                    AuthPhase::Restoring
                } else {
                    AuthPhase::Unconfigured
                },
                if configured {
                    "Checking OS credential store…"
                } else {
                    "Configure a registered GitHub OAuth client ID with device flow enabled."
                },
            ),
            token: None,
            generation,
            commands: tx,
            events: receive,
            configured,
        }
    }
    pub fn state(&self) -> &AuthState {
        &self.state
    }
    pub(crate) fn access_token(&self) -> Option<&str> {
        self.token
            .as_ref()
            .filter(|c| !expired(c))
            .map(|c| c.token.as_str())
    }
    pub fn poll(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::State(g, s) if g == self.generation.load(Ordering::Acquire) => {
                    self.state = s
                }
                Event::Authorized(g, s, c) if g == self.generation.load(Ordering::Acquire) => {
                    self.state = s;
                    self.token = Some(c);
                }
                _ => {}
            }
        }
        if self.token.as_ref().is_some_and(expired) {
            self.token = None;
            self.state = AuthState::status(
                AuthPhase::SignedOut,
                "GitHub session expired. Sign in again.",
            );
        }
    }
    pub fn start(&mut self) {
        if !self.configured {
            return;
        }
        if matches!(
            self.state.phase,
            AuthPhase::Restoring | AuthPhase::Requesting | AuthPhase::AwaitingAuthorization
        ) {
            return;
        }
        self.token = None;
        let g = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.state = AuthState::status(AuthPhase::Requesting, "Requesting a GitHub sign-in code…");
        if self.commands.try_send(Command::Start(g)).is_err() {
            self.state = AuthState::status(
                AuthPhase::Error,
                "Sign-in worker unavailable; reopen this panel to retry.",
            );
        }
    }
    pub fn cancel(&mut self) {
        let g = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.token = None;
        self.state = AuthState::status(
            AuthPhase::SignedOut,
            "Cancelling sign-in and removing its credential…",
        );
        if self.commands.try_send(Command::Cancel(g)).is_err() {
            self.state.status = "Cancelled in memory; credential cleanup could not be queued. Use Sign out again before closing.".into();
        }
    }
    pub fn sign_out(&mut self) {
        self.token = None;
        let g = self.generation.fetch_add(1, Ordering::AcqRel) + 1;
        self.state = AuthState::status(
            AuthPhase::SignedOut,
            "Signed out in memory; removing the stored credential…",
        );
        if self.commands.try_send(Command::SignOut(g)).is_err() {
            self.state.status="Signed out in memory. OS credential removal could not be queued; retry before closing.".into();
        }
    }
}
impl Drop for GithubAuth {
    fn drop(&mut self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        let _ = self.commands.try_send(Command::Shutdown);
    }
}
fn valid_client_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 128
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.')
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}
fn expired(c: &Credential) -> bool {
    c.expires_at_ms.is_some_and(|e| now_ms() >= e)
}
fn http_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(15))
        .user_agent("Arkline-Editor")
        .build()
        .map_err(|_| "Could not initialize GitHub HTTPS client".into())
}
fn json(request: reqwest::blocking::RequestBuilder) -> Result<Value, String> {
    let response = request
        .header("Accept", "application/json")
        .send()
        .map_err(|_| "GitHub network request failed; retry when connected".to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "GitHub request returned HTTP {}",
            response.status().as_u16()
        ));
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read GitHub response".to_string())?;
    if bytes.len() as u64 > MAX_RESPONSE {
        return Err("GitHub response exceeded its limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "GitHub returned invalid JSON".into())
}
fn login(client: &reqwest::blocking::Client, token: &str) -> Result<String, String> {
    if token.is_empty() || token.len() > 4096 || !token.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("Invalid GitHub credential".into());
    }
    let value = json(
        client
            .get(USER_URL)
            .bearer_auth(token)
            .header("X-GitHub-Api-Version", "2022-11-28"),
    )?;
    let login = value["login"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 100
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or("GitHub identity response was invalid")?;
    Ok(login.to_string())
}
struct Device {
    code: String,
    user_code: String,
    expires: u64,
    interval: u64,
}
fn device(value: &Value) -> Result<Device, String> {
    if value.get("error").is_some() {
        return Err(token_reply(value, 5)
            .err()
            .unwrap_or_else(|| "GitHub did not issue a sign-in code".into()));
    }
    if value["verification_uri"].as_str() != Some(VERIFY_URL) {
        return Err("GitHub device verification URL was invalid".into());
    }
    let code = value["device_code"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 256)
        .ok_or("Invalid GitHub device code")?;
    let user_code = value["user_code"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() <= 32
                && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .ok_or("Invalid GitHub user code")?;
    let expires = value["expires_in"]
        .as_u64()
        .filter(|n| (1..=900).contains(n))
        .ok_or("Invalid GitHub device expiry")?;
    let interval = value["interval"]
        .as_u64()
        .filter(|n| (1..=900).contains(n))
        .ok_or("Invalid GitHub polling interval")?;
    Ok(Device {
        code: code.into(),
        user_code: user_code.into(),
        expires,
        interval,
    })
}
enum Reply {
    Pending(u64),
    Authorized(Credential),
}
fn token_reply(value: &Value, interval: u64) -> Result<Reply, String> {
    if let Some(error) = value["error"].as_str() {
        return match error {
            "authorization_pending" => Ok(Reply::Pending(interval)),
            "slow_down" => Ok(Reply::Pending(
                interval
                    .saturating_add(5)
                    .max(value["interval"].as_u64().unwrap_or(0)),
            )),
            "access_denied" => Err("GitHub sign-in was declined".into()),
            "expired_token" | "token_expired" => {
                Err("GitHub sign-in code expired; start again".into())
            }
            "device_flow_disabled" => {
                Err("Enable device flow in this GitHub OAuth app registration".into())
            }
            "incorrect_client_credentials" => Err("GitHub OAuth client ID was rejected".into()),
            _ => Err("GitHub authorization could not be completed; start again".into()),
        };
    }
    if value["token_type"]
        .as_str()
        .is_none_or(|s| !s.eq_ignore_ascii_case("bearer"))
    {
        return Err("Unsupported GitHub token type".into());
    }
    let token = value["access_token"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 4096 && s.bytes().all(|b| b.is_ascii_graphic()))
        .ok_or("Invalid GitHub token response")?;
    let expires_at_ms = match value.get("expires_in") {
        None => None,
        Some(v) => Some(
            now_ms().saturating_add(
                v.as_u64()
                    .filter(|s| *s > 0 && *s <= 31_536_000)
                    .ok_or("Invalid GitHub token expiry")?
                    .saturating_mul(1000),
            ),
        ),
    };
    // Refresh credentials are intentionally not retained: reauthorize after expiry.
    Ok(Reply::Authorized(Credential {
        token: token.into(),
        expires_at_ms,
    }))
}
fn credential_entry(id: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new("industries.p.arkline.editor.github", id)
}
fn clear(id: &str) -> bool {
    matches!(
        credential_entry(id).and_then(|e| e.delete_credential()),
        Ok(()) | Err(keyring::Error::NoEntry)
    )
}
// Run on the same worker as persistence, even if the Authorized event was already
// queued. Generation filtering alone cannot undo a completed keychain write.
fn cancel_pending(g: u64, tx: &mpsc::Sender<Event>, clear: impl FnOnce() -> bool) {
    let ok = clear();
    send_state(
        tx,
        g,
        AuthPhase::SignedOut,
        if ok {
            "Sign-in cancelled; stored credential removed."
        } else {
            "Cancelled in memory; OS credential removal failed. Use Sign out again before closing."
        },
    );
}
fn prepare_sign_in(clear: impl FnOnce() -> bool) -> Result<(), String> {
    if clear() {
        Ok(())
    } else {
        Err("Cannot remove the previous stored GitHub credential. Retry Sign out before signing in to another account.".into())
    }
}
fn send_state(tx: &mpsc::Sender<Event>, g: u64, phase: AuthPhase, message: &str) {
    let _ = tx.send(Event::State(g, AuthState::status(phase, message)));
}
fn finish(
    id: &str,
    client: &reqwest::blocking::Client,
    c: Credential,
    g: u64,
    current: &AtomicU64,
    tx: &mpsc::Sender<Event>,
    save: bool,
) -> Result<(), String> {
    let login = login(client, &c.token)?;
    if current.load(Ordering::Acquire) != g {
        return Ok(());
    }
    let persistent = if save {
        credential_entry(id)
            .and_then(|e| {
                e.set_password(&serde_json::to_string(&c).expect("credential serializable"))
            })
            .is_ok()
    } else {
        true
    };
    if current.load(Ordering::Acquire) != g {
        if save {
            clear(id);
        }
        return Ok(());
    }
    let mut state = AuthState::status(
        AuthPhase::SignedIn,
        if persistent {
            "Signed in to GitHub; credential protected by the OS store."
        } else {
            "Signed in for this session only: OS credential storage unavailable."
        },
    );
    state.login = Some(login);
    state.persistent = persistent;
    state.expires_at_ms = c.expires_at_ms;
    let _ = tx.send(Event::Authorized(g, state, c));
    Ok(())
}
fn worker(id: String, rx: Receiver<Command>, tx: mpsc::Sender<Event>, current: Arc<AtomicU64>) {
    let client = match http_client() {
        Ok(c) => c,
        Err(e) => {
            send_state(&tx, 0, AuthPhase::Error, &e);
            return;
        }
    };
    match credential_entry(&id).and_then(|e| e.get_password()) {
        Ok(raw) => {
            let stored = if raw.len() <= 8192 {
                serde_json::from_str::<Credential>(&raw).ok()
            } else {
                None
            };
            match stored.filter(|c| !expired(c)) {
                Some(c) => {
                    if finish(&id, &client, c, 0, &current, &tx, false).is_err() {
                        send_state(&tx,0,AuthPhase::SignedOut,"Stored GitHub session could not be verified. Sign in again or retry after reconnecting.");
                    }
                }
                None => {
                    clear(&id);
                    send_state(
                        &tx,
                        0,
                        AuthPhase::SignedOut,
                        "Stored GitHub session expired or was invalid. Sign in again.",
                    );
                }
            }
        }
        Err(keyring::Error::NoEntry) => send_state(
            &tx,
            0,
            AuthPhase::SignedOut,
            "Sign in to GitHub to connect this workspace.",
        ),
        Err(_) => send_state(
            &tx,
            0,
            AuthPhase::SignedOut,
            "OS credential store unavailable. Unlock it and retry; existing credentials must be cleared before a new sign-in.",
        ),
    }
    while let Ok(command) = rx.recv() {
        match command {
            Command::Shutdown => break,
            Command::Cancel(g) => cancel_pending(g, &tx, || clear(&id)),
            Command::SignOut(g) => {
                let ok = clear(&id);
                send_state(
                    &tx,
                    g,
                    AuthPhase::SignedOut,
                    if ok {
                        "Signed out; local stored credential removed. GitHub app authorization can be revoked in GitHub settings."
                    } else {
                        "Signed out in memory; OS credential removal failed. Retry sign-out before closing."
                    },
                );
            }
            Command::Start(g) => {
                if current.load(Ordering::Acquire) != g {
                    continue;
                }
                // Serial with Cancel/SignOut: an older account must never silently
                // return after a failed replacement or memory-only new session.
                if let Err(e) = prepare_sign_in(|| clear(&id)) {
                    send_state(&tx, g, AuthPhase::Error, &e);
                    continue;
                }
                if let Err(e) = authorize(&id, &client, g, &current, &tx) {
                    send_state(&tx, g, AuthPhase::Error, &e);
                }
            }
        }
    }
}
fn authorize(
    id: &str,
    client: &reqwest::blocking::Client,
    g: u64,
    current: &AtomicU64,
    tx: &mpsc::Sender<Event>,
) -> Result<(), String> {
    let response = json(
        client
            .post(DEVICE_URL)
            .form(&[("client_id", id), ("scope", "read:user read:project")]),
    )?;
    let d = device(&response)?;
    let deadline = Instant::now() + Duration::from_secs(d.expires);
    let mut state = AuthState::status(
        AuthPhase::AwaitingAuthorization,
        "Open GitHub and enter this code. Waiting for approval…",
    );
    state.user_code = Some(d.user_code);
    state.verification_url = Some(VERIFY_URL.into());
    state.expires_at_ms = Some(now_ms() + d.expires * 1000);
    let _ = tx.send(Event::State(g, state));
    let mut interval = d.interval;
    loop {
        let next = Instant::now() + Duration::from_secs(interval.min(901));
        while Instant::now() < next {
            if current.load(Ordering::Acquire) != g {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err("GitHub sign-in code expired; start again".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        let value = json(client.post(TOKEN_URL).form(&[
            ("client_id", id),
            ("device_code", &d.code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]))?;
        if current.load(Ordering::Acquire) != g {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err("GitHub sign-in code expired; start again".into());
        }
        match token_reply(&value, interval)? {
            Reply::Pending(i) => interval = i,
            Reply::Authorized(c) => return finish(id, client, c, g, current, tx, true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_delivery_removes_already_saved_credential_and_replacement_clears_old_account() {
        let mut stored = Some("old-account".to_owned());
        assert!(prepare_sign_in(|| {
            stored = None;
            true
        })
        .is_ok());
        assert!(stored.is_none()); // failed replacement cannot restore the old account
        stored = Some("new-account".to_owned()); // finish saved before UI cancellation
        let (tx, rx) = mpsc::channel();
        cancel_pending(2, &tx, || {
            stored = None;
            true
        });
        assert!(stored.is_none());
        assert!(matches!(
            rx.recv().unwrap(),
            Event::State(
                2,
                AuthState {
                    phase: AuthPhase::SignedOut,
                    ..
                }
            )
        ));
        assert!(prepare_sign_in(|| false).is_err());
        cancel_pending(3, &tx, || false);
        match rx.recv().unwrap() {
            Event::State(3, state) => assert!(state.status.contains("removal failed")),
            _ => panic!("expected explicit cleanup failure"),
        }
    }
    #[test]
    fn device_validation_and_slowdown_fail_closed() {
        let mut v = serde_json::json!({"device_code":"fixture","user_code":"ABCD-EFGH","verification_uri":VERIFY_URL,"expires_in":900,"interval":5});
        assert!(device(&v).is_ok());
        v["verification_uri"] = Value::String("https://evil.test".into());
        assert!(device(&v).is_err());
        assert!(matches!(
            token_reply(&serde_json::json!({"error":"slow_down","interval":12}), 5).unwrap(),
            Reply::Pending(12)
        ));
        assert!(matches!(
            token_reply(&serde_json::json!({"error":"slow_down"}), 12).unwrap(),
            Reply::Pending(17)
        ));
        for e in [
            "access_denied",
            "expired_token",
            "device_flow_disabled",
            "incorrect_client_credentials",
        ] {
            assert!(token_reply(&serde_json::json!({"error":e}), 5).is_err());
        }
        assert!(token_reply(
            &serde_json::json!({"access_token":"secret","token_type":"mac"}),
            5
        )
        .is_err());
    }
    #[test]
    fn successful_reply_requires_bearer_and_records_expiry_without_refresh_token() {
        let now = now_ms();
        let reply = token_reply(&serde_json::json!({"access_token":"fixture-token","token_type":"bearer","expires_in":60,"refresh_token":"discarded-fixture"}),5).unwrap();
        let Reply::Authorized(c) = reply else {
            panic!("expected authorization")
        };
        assert_eq!(c.token, "fixture-token");
        assert!(c.expires_at_ms.unwrap() >= now + 60_000);
        assert!(!serde_json::to_string(&c)
            .unwrap()
            .contains("discarded-fixture"));
        for value in [
            serde_json::json!({"access_token":"bad\ntoken","token_type":"bearer"}),
            serde_json::json!({"access_token":"fixture","token_type":"bearer","expires_in":0}),
        ] {
            assert!(token_reply(&value, 5).is_err());
        }
        assert!(device(&serde_json::json!({"error":"device_flow_disabled"}))
            .err()
            .unwrap()
            .contains("Enable device flow"));
    }
    #[test]
    fn stale_worker_authorization_cannot_restore_cancelled_session() {
        let (commands, _) = mpsc::sync_channel(1);
        let (tx, events) = mpsc::channel();
        let mut auth = GithubAuth {
            state: AuthState::status(AuthPhase::SignedOut, ""),
            token: None,
            generation: Arc::new(AtomicU64::new(4)),
            commands,
            events,
            configured: true,
        };
        tx.send(Event::Authorized(
            3,
            AuthState::status(AuthPhase::SignedIn, ""),
            Credential {
                token: "fixture".into(),
                expires_at_ms: None,
            },
        ))
        .ok();
        auth.poll();
        assert!(auth.access_token().is_none());
        assert_eq!(auth.state.phase, AuthPhase::SignedOut);
        tx.send(Event::Authorized(
            4,
            AuthState::status(AuthPhase::SignedIn, ""),
            Credential {
                token: "fixture".into(),
                expires_at_ms: Some(1),
            },
        ))
        .ok();
        auth.poll();
        assert!(auth.access_token().is_none());
    }
    #[test]
    fn simulated_http_bounds_and_redirect_rejection() {
        use std::{io::Write, net::TcpListener};
        for (status, body) in [
            (
                "200 OK",
                "{\"error\":\"authorization_pending\"}".to_string(),
            ),
            ("302 Found", "".into()),
            ("200 OK", "x".repeat(MAX_RESPONSE as usize + 1)),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let worker = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut b = [0; 2048];
                let _ = s.read(&mut b);
                write!(s,"HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: https://evil.test\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
            });
            let result = json(http_client().unwrap().get(format!("http://{address}")));
            if status == "200 OK" && result.is_ok() {
                assert!(matches!(
                    token_reply(&result.unwrap(), 5).unwrap(),
                    Reply::Pending(5)
                ));
            } else {
                assert!(result.is_err());
            }
            worker.join().unwrap();
        }
    }
}
