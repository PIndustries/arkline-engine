//! Native public-client OAuth. Credentials stay in memory; no client secret is shipped.
use oauth2::{
    basic::BasicClient, reqwest, AuthUrl, AuthorizationCode, ClientId, CsrfToken,
    PkceCodeChallenge, RedirectUrl, Scope, TokenResponse, TokenUrl,
};
use serde::Deserialize;
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: String,
    pub issuer: String,
    pub authorization_url: String,
    pub token_url: String,
    pub client_id: String,
    pub scopes: Vec<String>,
}
impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|_| "Cannot read OAuth configuration")?
            .take(16_385)
            .read_to_end(&mut bytes)
            .map_err(|_| "Cannot read OAuth configuration")?;
        if bytes.len() > 16_384 {
            return Err("OAuth configuration is too large".into());
        }
        let config: Self =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid OAuth configuration")?;
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<(), String> {
        let issuer = safe_url(&self.issuer, "https")?;
        if issuer.query().is_some() {
            return Err("Issuer cannot contain a query".into());
        }
        for endpoint in [&self.authorization_url, &self.token_url] {
            let url = safe_url(endpoint, "https")?;
            if url.origin() != issuer.origin() || url.query().is_some() {
                return Err(
                    "OAuth endpoints must share the configured issuer origin and have no query"
                        .into(),
                );
            }
        }
        safe_url(&self.server, "wss")?;
        if self.client_id.trim().is_empty()
            || self.scopes.is_empty()
            || self
                .scopes
                .iter()
                .any(|s| s.is_empty() || s.contains(char::is_whitespace))
        {
            return Err("OAuth client ID and individual scopes are required".into());
        }
        Ok(())
    }
}
fn safe_url(raw: &str, scheme: &str) -> Result<oauth2::url::Url, String> {
    let url = oauth2::url::Url::parse(raw).map_err(|_| "Invalid OAuth endpoint")?;
    if url.scheme() != scheme
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "OAuth requires secure endpoints without embedded credentials or fragments".into(),
        );
    }
    Ok(url)
}
// Deliberately no Debug implementation: never format access tokens into UI/logs.
#[derive(Clone)]
pub struct Credential {
    pub token: String,
    pub server: String,
    pub pilot: String,
    expires: Instant,
}
impl Credential {
    pub fn matches(&self, server: &str, pilot: &str) -> bool {
        self.server == server && self.pilot == pilot && Instant::now() < self.expires
    }
}
enum Event {
    Open(String),
    Done(Result<Credential, String>),
}
#[derive(Default)]
pub struct Login {
    pub config: Option<Config>,
    pub credential: Option<Credential>,
    receiver: Option<Receiver<Event>>,
    cancel: Option<Arc<AtomicBool>>,
}
impl Drop for Login {
    fn drop(&mut self) {
        self.cancel();
    }
}
impl Login {
    pub fn pending(&self) -> bool {
        self.receiver.is_some()
    }
    pub fn configured(&self, server: &str) -> bool {
        self.config.as_ref().is_some_and(|c| c.server == server)
    }
    pub fn cancel(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.receiver = None;
    }
    pub fn begin(&mut self, server: &str, pilot: &str) -> Result<(), String> {
        self.cancel();
        self.credential = None;
        let config = self
            .config
            .clone()
            .filter(|c| c.server == server)
            .ok_or("Account sign-in is not configured for this server")?;
        let (sender, receiver) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let pilot = pilot.to_owned();
        std::thread::spawn(move || {
            let result = authorize(config, pilot, &sender, &stop);
            let _ = sender.send(Event::Done(result));
        });
        self.receiver = Some(receiver);
        self.cancel = Some(cancel);
        Ok(())
    }
    pub fn poll(&mut self, ctx: &eframe::egui::Context) -> Option<Result<(), String>> {
        let event = self.receiver.as_ref()?.try_recv();
        match event {
            Ok(Event::Open(url)) => {
                ctx.open_url(eframe::egui::OpenUrl::new_tab(url));
                None
            }
            Ok(Event::Done(result)) => {
                self.cancel();
                Some(result.map(|credential| {
                    self.credential = Some(credential);
                }))
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.cancel();
                Some(Err("Account sign-in stopped unexpectedly".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }
    }
}
fn authorize(
    config: Config,
    pilot: String,
    sender: &Sender<Event>,
    cancel: &AtomicBool,
) -> Result<Credential, String> {
    config.validate()?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .map_err(|_| "Cannot open local sign-in callback")?;
    listener
        .set_nonblocking(true)
        .map_err(|_| "Cannot configure sign-in callback")?;
    let address = listener
        .local_addr()
        .map_err(|_| "Cannot inspect sign-in callback")?;
    let redirect = format!("http://{address}/oauth/callback");
    let client = BasicClient::new(ClientId::new(config.client_id))
        .set_auth_uri(
            AuthUrl::new(config.authorization_url).map_err(|_| "Invalid authorization URL")?,
        )
        .set_token_uri(TokenUrl::new(config.token_url).map_err(|_| "Invalid token URL")?)
        .set_redirect_uri(RedirectUrl::new(redirect).map_err(|_| "Invalid callback URL")?);
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut authorization = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(challenge);
    for scope in config.scopes {
        authorization = authorization.add_scope(Scope::new(scope));
    }
    let (url, state) = authorization.url();
    sender
        .send(Event::Open(url.to_string()))
        .map_err(|_| "Sign-in cancelled")?;
    let code = await_callback(
        &listener,
        state.secret(),
        &config.issuer,
        cancel,
        Duration::from_secs(180),
    )?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Sign-in cancelled".into());
    }
    let http = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "Cannot create secure account connection")?;
    // Never display provider response bodies: they may contain credentials or account data.
    let token = client
        .exchange_code(AuthorizationCode::new(code))
        .set_pkce_verifier(verifier)
        .request(&|request| bounded_token_http(&http, request, cancel))
        .map_err(|_| "Account token exchange failed. Sign in again.")?;
    if cancel.load(Ordering::Relaxed) {
        return Err("Sign-in cancelled".into());
    }
    if token.token_type() != &oauth2::basic::BasicTokenType::Bearer {
        return Err("Account provider returned an unsupported token type".into());
    }
    let ttl = token
        .expires_in()
        .filter(|t| *t > Duration::from_secs(5))
        .ok_or("Account did not provide a usable token expiry")?
        .min(Duration::from_secs(3600));
    if token.access_token().secret().len() > 16_384 {
        return Err("Account token exceeds supported size".into());
    }
    Ok(Credential {
        token: token.access_token().secret().clone(),
        server: config.server,
        pilot,
        expires: Instant::now() + ttl - Duration::from_secs(5),
    })
}
fn await_callback(
    listener: &TcpListener,
    state: &str,
    issuer: &str,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<String, String> {
    let until = Instant::now() + timeout;
    while Instant::now() < until {
        if cancel.load(Ordering::Relaxed) {
            return Err("Sign-in cancelled".into());
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    continue;
                }
                let result = read_callback(
                    &mut stream,
                    state,
                    issuer,
                    cancel,
                    until.min(Instant::now() + Duration::from_secs(2)),
                );
                if cancel.load(Ordering::Relaxed) {
                    return Err("Sign-in cancelled".into());
                }
                if Instant::now() >= until {
                    return Err("Sign-in timed out. Try again.".into());
                }
                let (status, body) = if result.is_ok() {
                    ("200 OK", "Sign-in received. Return to Arkline.")
                } else {
                    (
                        "400 Bad Request",
                        "This sign-in callback was not accepted. Return to Arkline.",
                    )
                };
                let _ = write!(stream, "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}", body.len());
                // An unrelated localhost request must not consume a pending authorization.
                match result {
                    Ok(code) => return Ok(code),
                    Err(error) if error == "Sign-in was declined" => return Err(error),
                    Err(_) => {}
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(_) => return Err("Sign-in callback failed".into()),
        }
    }
    Err("Sign-in timed out. Try again.".into())
}
fn read_callback(
    stream: &mut TcpStream,
    state: &str,
    issuer: &str,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<String, String> {
    stream
        .set_nonblocking(true)
        .map_err(|_| "Callback configuration failed")?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 512];
    while bytes.len() < 8192 && !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
        if cancel.load(Ordering::Relaxed) {
            return Err("Sign-in cancelled".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("Callback request timed out".into());
        }
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&buf[..n]),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(remaining.min(Duration::from_millis(10)))
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err("Invalid callback request".into()),
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err("Sign-in cancelled".into());
    }
    if Instant::now() >= deadline {
        return Err("Callback request timed out".into());
    }
    let request = std::str::from_utf8(&bytes).map_err(|_| "Invalid callback encoding")?;
    parse_callback(request, state, issuer)
}
const MAX_TOKEN_RESPONSE_BYTES: usize = 65_536;
fn read_token_body(mut input: impl Read, cancel: &AtomicBool) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("Sign-in cancelled"));
        }
        let remaining = (MAX_TOKEN_RESPONSE_BYTES + 1 - bytes.len()).min(buf.len());
        let n = match input.read(&mut buf[..remaining]) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(std::io::Error::other("Token response read failed")),
        };
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&buf[..n]);
        if bytes.len() > MAX_TOKEN_RESPONSE_BYTES {
            return Err(std::io::Error::other(
                "Token response exceeds supported size",
            ));
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(std::io::Error::other("Sign-in cancelled"));
    }
    Ok(bytes)
}
fn bounded_token_http(
    http: &reqwest::blocking::Client,
    request: oauth2::HttpRequest,
    cancel: &AtomicBool,
) -> std::io::Result<oauth2::HttpResponse> {
    if cancel.load(Ordering::Relaxed) {
        return Err(std::io::Error::other("Sign-in cancelled"));
    }
    let request: reqwest::blocking::Request = request
        .try_into()
        .map_err(|_| std::io::Error::other("Invalid token request"))?;
    let response = http
        .execute(request)
        .map_err(|_| std::io::Error::other("Token HTTP transport failed"))?;
    if response
        .content_length()
        .is_some_and(|n| n > MAX_TOKEN_RESPONSE_BYTES as u64)
    {
        return Err(std::io::Error::other(
            "Token response exceeds supported size",
        ));
    }
    let mut result = oauth2::http::Response::builder()
        .status(response.status())
        .version(response.version());
    for (name, value) in response.headers() {
        result = result.header(name, value);
    }
    result
        .body(read_token_body(response, cancel)?)
        .map_err(|_| std::io::Error::other("Invalid token response"))
}
fn parse_callback(request: &str, expected_state: &str, issuer: &str) -> Result<String, String> {
    if request.len() >= 8192 || !request.contains("\r\n\r\n") {
        return Err("Invalid callback length".into());
    }
    let line = request.lines().next().ok_or("Missing callback request")?;
    let fields: Vec<_> = line.split_whitespace().collect();
    if fields.len() != 3
        || fields[0] != "GET"
        || !fields[1].starts_with("/oauth/callback?")
        || fields[2] != "HTTP/1.1"
    {
        return Err("Invalid callback route".into());
    }
    let url = oauth2::url::Url::parse(&format!("http://127.0.0.1{}", fields[1]))
        .map_err(|_| "Invalid callback URL")?;
    let mut params = std::collections::HashMap::new();
    for (key, value) in url.query_pairs() {
        if params.insert(key.to_string(), value.to_string()).is_some() {
            return Err("Duplicate callback parameter".into());
        }
    }
    if params.get("state").map(String::as_str) != Some(expected_state)
        || params.get("iss").is_some_and(|v| v != issuer)
    {
        return Err("Sign-in response did not match this request".into());
    }
    if params.contains_key("error") {
        return Err("Sign-in was declined".into());
    }
    params
        .remove("code")
        .filter(|s| !s.is_empty())
        .ok_or("Missing authorization code".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(query: &str) -> String {
        format!("GET /oauth/callback?{query} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
    }
    #[test]
    fn callback_rejects_csrf_duplicates_and_wrong_issuer() {
        let issuer = "https://identity.example";
        assert_eq!(
            parse_callback(&request("state=expected&code=ok"), "expected", issuer).unwrap(),
            "ok"
        );
        for q in [
            "state=wrong&code=ok",
            "state=expected&code=one&code=two",
            "state=expected&code=ok&iss=https://other.example",
            "state=expected&error=access_denied",
            "state=expected&code=",
        ] {
            assert!(parse_callback(&request(q), "expected", issuer).is_err());
        }
        assert!(parse_callback(
            "GET /other?state=expected&code=ok HTTP/1.1\r\n\r\n",
            "expected",
            issuer
        )
        .is_err());
    }
    #[test]
    fn callback_listener_ignores_unrelated_requests_then_accepts_matching_state() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let task = std::thread::spawn(move || {
            await_callback(
                &listener,
                "right",
                "https://id.example",
                &AtomicBool::new(false),
                Duration::from_secs(3),
            )
        });
        for state in ["wrong", "right"] {
            let mut socket = TcpStream::connect(address).unwrap();
            socket
                .write_all(request(&format!("state={state}&code=ok")).as_bytes())
                .unwrap();
            let mut reply = String::new();
            socket.read_to_string(&mut reply).unwrap();
            assert!(reply.starts_with(if state == "right" {
                "HTTP/1.1 200"
            } else {
                "HTTP/1.1 400"
            }));
        }
        assert_eq!(task.join().unwrap().unwrap(), "ok");
    }
    #[test]
    fn cancellation_releases_callback_without_waiting_for_timeout() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(await_callback(
            &listener,
            "s",
            "https://id.example",
            &AtomicBool::new(true),
            Duration::from_secs(180)
        )
        .unwrap_err()
        .contains("cancelled"));
    }
    #[test]
    fn callback_trickle_cannot_extend_deadline_and_cancel_interrupts_read() {
        for cancelled in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let stop = Arc::new(AtomicBool::new(false));
            let stop_writer = stop.clone();
            let writer = std::thread::spawn(move || {
                let mut stream = TcpStream::connect(address).unwrap();
                for i in 0..100 {
                    if stream.write_all(b"x").is_err() {
                        break;
                    }
                    if cancelled && i == 4 {
                        stop_writer.store(true, Ordering::Relaxed);
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            });
            let start = Instant::now();
            let error = await_callback(
                &listener,
                "state",
                "https://id.example",
                &stop,
                Duration::from_millis(100),
            )
            .unwrap_err();
            assert!(
                start.elapsed() < Duration::from_secs(1),
                "callback ignored absolute bound"
            );
            assert!(error.contains(if cancelled { "cancelled" } else { "timed out" }));
            writer.join().unwrap();
        }
    }
    #[test]
    fn token_body_bound_applies_before_json_parse_and_cancellation_is_checked() {
        let mut source = std::io::Cursor::new(vec![b'x'; MAX_TOKEN_RESPONSE_BYTES * 4]);
        assert!(read_token_body(&mut source, &AtomicBool::new(false)).is_err());
        assert_eq!(source.position() as usize, MAX_TOKEN_RESPONSE_BYTES + 1);
        let mut source = std::io::Cursor::new(b"valid".to_vec());
        assert!(read_token_body(&mut source, &AtomicBool::new(true)).is_err());
        assert_eq!(source.position(), 0);
        assert_eq!(
            read_token_body(std::io::Cursor::new(b"{}"), &AtomicBool::new(false)).unwrap(),
            b"{}"
        );
    }
    #[test]
    fn token_http_adapter_preserves_success_and_caps_declared_and_streamed_bodies() {
        for kind in 0..3 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let endpoint = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") && request.len() < 8192 {
                    if stream.read(&mut byte).unwrap_or(0) == 0 {
                        return;
                    }
                    request.push(byte[0]);
                }
                match kind {
                    0 => {
                        let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
                    }
                    1 => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            MAX_TOKEN_RESPONSE_BYTES + 1
                        );
                    }
                    _ => {
                        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n");
                        let _ = stream.write_all(&vec![b'x'; MAX_TOKEN_RESPONSE_BYTES * 2]);
                    }
                }
            });
            let http = reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap();
            let request = oauth2::http::Request::builder()
                .uri(format!("http://{address}/fixture-only"))
                .body(vec![])
                .unwrap();
            let response = bounded_token_http(&http, request, &AtomicBool::new(false));
            if kind == 0 {
                let response = response.unwrap();
                assert_eq!(response.status(), 200);
                assert_eq!(response.body(), b"{}");
            } else {
                assert!(response.is_err());
            }
            endpoint.join().unwrap();
        }
    }
    #[test]
    fn config_rejects_insecure_and_mixed_providers() {
        let mut c = Config {
            server: "wss://relay.example/ws".into(),
            issuer: "https://identity.example".into(),
            authorization_url: "https://identity.example/authorize".into(),
            token_url: "https://identity.example/token".into(),
            client_id: "arkline-native".into(),
            scopes: vec!["arkline.play".into()],
        };
        c.validate().unwrap();
        c.token_url = "https://attacker.example/token".into();
        assert!(c.validate().is_err());
        c.token_url = "http://identity.example/token".into();
        assert!(c.validate().is_err());
    }
    #[test]
    fn tokens_are_bound_to_endpoint_pilot_and_expiration() {
        let mut c = Credential {
            token: "secret".into(),
            server: "wss://test.example".into(),
            pilot: "A".into(),
            expires: Instant::now() + Duration::from_secs(10),
        };
        assert!(c.matches("wss://test.example", "A"));
        assert!(!c.matches("wss://prod.example", "A"));
        assert!(!c.matches("wss://test.example", "B"));
        c.expires = Instant::now();
        assert!(!c.matches("wss://test.example", "A"));
    }
}
