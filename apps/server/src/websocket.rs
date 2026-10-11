//! Bounded WebSocket transport with optional native TLS.
use super::*;
use std::{
    io::{self, Cursor, Read},
    time::Instant,
};
use tungstenite::{
    accept_hdr_with_config,
    handshake::server::{ErrorResponse, Request as Upgrade, Response as UpgradeResponse},
    protocol::WebSocketConfig,
    Message,
};
const MAX_REPLY: usize = 8 * 1024 * 1024;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const MESSAGE_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) struct Bucket {
    tokens: f64,
    capacity: f64,
    rate: f64,
    last: Instant,
}
impl Bucket {
    fn new(capacity: f64, rate: f64) -> Self {
        Self {
            tokens: capacity,
            capacity,
            rate,
            last: Instant::now(),
        }
    }
    fn take(&mut self) -> bool {
        let now = Instant::now();
        self.tokens = (self.tokens + now.duration_since(self.last).as_secs_f64() * self.rate)
            .min(self.capacity);
        self.last = now;
        if self.tokens < 1.0 {
            false
        } else {
            self.tokens -= 1.0;
            true
        }
    }
}
pub(super) struct Limits {
    connections: Bucket,
    requests: Bucket,
    registrations: Bucket,
    exports: Bucket,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            connections: Bucket::new(16., 4.),
            requests: Bucket::new(600., 300.),
            registrations: Bucket::new(5., 5. / 60.),
            exports: Bucket::new(2., 1.),
        }
    }
}
impl Limits {
    pub(super) fn admit(&mut self) -> bool {
        self.connections.take()
    }
}
struct DeadlineStream {
    socket: tls::Transport,
    deadline: Instant,
    handshake_remaining: Option<usize>,
    prefix: Cursor<Vec<u8>>,
    write_deadline: Instant,
}
impl Read for DeadlineStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.prefix.position() < self.prefix.get_ref().len() as u64 {
            return self.prefix.read(bytes);
        }
        let _remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "message deadline"))?;
        self.socket.set_read_deadline(self.deadline);
        if self.handshake_remaining == Some(0) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "handshake too large",
            ));
        }
        let len = bytes
            .len()
            .min(self.handshake_remaining.unwrap_or(usize::MAX));
        let n = self.socket.read(&mut bytes[..len])?;
        if let Some(left) = &mut self.handshake_remaining {
            *left -= n;
        }
        Ok(n)
    }
}
impl Write for DeadlineStream {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let _remaining = self
            .write_deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or_else(|| io::Error::new(io::ErrorKind::TimedOut, "write deadline"))?;
        self.socket.set_write_deadline(self.write_deadline);
        self.socket.write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}
#[derive(Default)]
struct BoundedReply(Vec<u8>);
impl Write for BoundedReply {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        if self.0.len().saturating_add(b.len()) > MAX_REPLY {
            return Err(io::Error::other("response too large"));
        }
        self.0.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn encode(response: &Response) -> String {
    let mut out = BoundedReply::default();
    if serde_json::to_writer(&mut out, response).is_err() {
        return serde_json::to_string(&Response::error(
            "response exceeds public transport limit; use operator export",
        ))
        .unwrap();
    }
    String::from_utf8(out.0).expect("JSON is UTF-8")
}
fn reject(status: u16, message: &str) -> ErrorResponse {
    tungstenite::http::Response::builder()
        .status(status)
        .body(Some(message.into()))
        .unwrap()
}
pub(super) fn serve_ws(
    stream: TcpStream,
    server: Arc<Mutex<Server>>,
    limits: Arc<Mutex<Limits>>,
    origins: Arc<Vec<String>>,
    max_pilots: usize,
    health: Arc<Health>,
    tls: Option<Arc<tls::Identity>>,
) -> Result<()> {
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    stream.set_nodelay(true)?;
    let mut stream = DeadlineStream {
        socket: tls::Transport::accept(stream, tls, HANDSHAKE_TIMEOUT)?,
        deadline: Instant::now() + HANDSHAKE_TIMEOUT,
        handshake_remaining: Some(MAX_REQUEST_BYTES),
        prefix: Cursor::new(Vec::new()),
        write_deadline: Instant::now() + HANDSHAKE_TIMEOUT,
    };
    // Bound the complete HTTP header before routing health or handing the same bytes
    // to the WebSocket parser. Preserve any coalesced first frame in the prefix.
    let mut header = Vec::new();
    while !header.windows(4).any(|w| w == b"\r\n\r\n") {
        let mut bytes = [0u8; 1024];
        let n = stream.read(&mut bytes)?;
        if n == 0 {
            return Ok(());
        }
        header.extend_from_slice(&bytes[..n]);
    }
    if header.starts_with(b"GET /health HTTP/1.1\r\n") {
        let ready = health.ready();
        let (status, body) = if ready {
            ("200 OK", "{\"simulation_ready\":true}")
        } else {
            ("503 Service Unavailable", "{\"simulation_ready\":false}")
        };
        write!(stream, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}", body.len())?;
        return Ok(());
    }
    stream.prefix = Cursor::new(header);
    let config = WebSocketConfig::default()
        .read_buffer_size(4096)
        .write_buffer_size(0)
        .max_write_buffer_size(MAX_REPLY + 1024)
        .max_message_size(Some(MAX_REQUEST_BYTES))
        .max_frame_size(Some(MAX_REQUEST_BYTES));
    let mut ws = accept_hdr_with_config(
        stream,
        move |req: &Upgrade, response: UpgradeResponse| {
            if req.uri().path() != "/ws" || req.uri().query().is_some() {
                return Err(reject(404, "WebSocket endpoint is /ws"));
            }
            if let Some(origin) = req.headers().get("origin") {
                if !origin
                    .to_str()
                    .ok()
                    .is_some_and(|o| origins.iter().any(|allowed| allowed == o))
                {
                    return Err(reject(403, "Origin not permitted"));
                }
            }
            Ok(response)
        },
        Some(config),
    )
    .map_err(|_| anyhow::anyhow!("WebSocket handshake rejected"))?;
    ws.get_mut().handshake_remaining = None;
    let mut session = TransportSession {
        server: server.clone(),
        pilot: None,
    };
    let mut messages = Bucket::new(60., 30.);
    let mut exports = Bucket::new(1., 1. / 30.);
    let mut auth_failures = 0;
    loop {
        ws.get_mut().deadline = Instant::now() + MESSAGE_TIMEOUT;
        ws.get_mut().write_deadline = Instant::now() + MESSAGE_TIMEOUT + Duration::from_secs(10);
        let message = ws.read()?;
        ws.get_mut().write_deadline = Instant::now() + Duration::from_secs(10);
        if !messages.take()
            || !limits
                .lock()
                .map_err(|_| anyhow::anyhow!("budget unavailable"))?
                .requests
                .take()
        {
            ws.send(Message::Text(
                encode(&Response::error("public request rate exceeded")).into(),
            ))?;
            let _ = ws.close(None);
            return Ok(());
        }
        match message {
            Message::Text(text) => {
                let request: Request = match serde_json::from_str(&text) {
                    Ok(r) => r,
                    Err(_) => {
                        ws.send(Message::Text(
                            encode(&Response::error("invalid request JSON or command schema"))
                                .into(),
                        ))?;
                        continue;
                    }
                };
                let connecting = matches!(request, Request::Connect { .. });
                let response = if matches!(request, Request::ExportLedger)
                    && (!exports.take()
                        || !limits
                            .lock()
                            .map_err(|_| anyhow::anyhow!("budget unavailable"))?
                            .exports
                            .take())
                {
                    Response::error("public ledger export rate exceeded; retry later")
                } else {
                    let mut authority = server
                        .lock()
                        .map_err(|_| anyhow::anyhow!("server state lock poisoned"))?;
                    match authority.check_public_request(
                        session.pilot.as_deref(),
                        &request,
                        max_pilots,
                    ) {
                        Err(e) => Response::error(e.to_string()),
                        Ok(new_registration) => {
                            if new_registration
                                && !limits
                                    .lock()
                                    .map_err(|_| anyhow::anyhow!("budget unavailable"))?
                                    .registrations
                                    .take()
                            {
                                Response::error("public registration rate exceeded; retry later")
                            } else {
                                authority.handle(&mut session.pilot, request)
                            }
                        }
                    }
                };
                if connecting && !response.ok {
                    auth_failures += 1;
                }
                ws.send(Message::Text(encode(&response).into()))?;
                if auth_failures >= 5 {
                    let _ = ws.close(None);
                    return Ok(());
                }
            }
            Message::Ping(_) | Message::Pong(_) => {
                ws.flush()?;
            }
            Message::Close(_) => {
                let _ = ws.flush();
                return Ok(());
            }
            Message::Binary(_) => {
                ws.send(Message::Text(
                    encode(&Response::error("only JSON text messages are supported")).into(),
                ))?;
                let _ = ws.close(None);
                return Ok(());
            }
            Message::Frame(_) => unreachable!("raw frames are not exposed"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn health_requires_a_recent_successful_tick() {
        let health = Health {
            started: Instant::now() - Duration::from_secs(5),
            last_tick: std::sync::atomic::AtomicU64::new(0),
        };
        assert!(!health.ready());
        health.last_tick.store(1, Ordering::Release);
        assert!(!health.ready());
        health.last_tick.store(5001, Ordering::Release);
        assert!(health.ready());
    }
    #[test]
    fn rate_budget_is_bounded_and_refills_by_elapsed_time() {
        let mut bucket = Bucket::new(2., 10.);
        assert!(bucket.take());
        assert!(bucket.take());
        assert!(!bucket.take());
        bucket.last -= Duration::from_millis(100);
        assert!(bucket.take());
        assert!(!bucket.take());
    }
    #[test]
    fn reply_buffer_never_grows_past_cap() {
        let mut out = BoundedReply::default();
        out.write_all(&vec![b'x'; MAX_REPLY]).unwrap();
        assert!(out.write_all(b"x").is_err());
        assert_eq!(out.0.len(), MAX_REPLY);
    }
}
