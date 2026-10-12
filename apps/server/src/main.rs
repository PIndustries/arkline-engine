mod tls;
mod websocket;
use anyhow::{bail, Context, Result};
use arkline_protocol::{Request, Response};
use arkline_server::Server;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
fn main() -> Result<()> {
    let mut auth_config = None;
    let mut auth_jwks = None;
    let mut auth_required = false;
    let mut bind = "127.0.0.1:7777".to_string();
    let mut ws_bind: Option<String> = None;
    let mut tls_cert = None;
    let mut tls_key = None;
    let mut tls_health = None;
    let mut tls_ca = None;
    let mut tls_server_name = None;
    let mut origins = vec![
        "https://arkline.network".into(),
        "https://www.arkline.network".into(),
        "https://play.arkline.network".into(),
        "https://test.arkline.network".into(),
    ];
    let mut max_pilots = 256usize;
    let mut local_development = None;
    let mut explicit_db = false;
    let mut local_origins = Vec::new();
    let mut db = "data/arkline.sqlite".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--auth-config" => {
                auth_config = Some(args.next().context("--auth-config requires JSON path")?)
            }
            "--auth-jwks" => {
                auth_jwks = Some(
                    args.next()
                        .context("--auth-jwks requires public JWKS path")?,
                )
            }
            "--auth-required" => auth_required = true,
            "--bind" => bind = args.next().context("--bind requires an address")?,
            "--ws-bind" => ws_bind = Some(args.next().context("--ws-bind requires an address")?),
            "--tls-health" => {
                tls_health = Some(args.next().context("--tls-health requires an address")?)
            }
            "--tls-ca" => tls_ca = Some(args.next().context("--tls-ca requires a PEM path")?),
            "--tls-server-name" => {
                tls_server_name = Some(
                    args.next()
                        .context("--tls-server-name requires a DNS name")?,
                )
            }
            "--tls-cert" => tls_cert = Some(args.next().context("--tls-cert requires a PEM path")?),
            "--tls-key" => tls_key = Some(args.next().context("--tls-key requires a PEM path")?),
            "--ws-origin" => {
                let origin = args
                    .next()
                    .context("--ws-origin requires an exact origin")?;
                local_origins.push(origin.clone());
                origins.push(origin);
            }
            "--ws-max-pilots" => {
                max_pilots = args
                    .next()
                    .context("--ws-max-pilots requires a count")?
                    .parse()?;
                if !(1..=1024).contains(&max_pilots) {
                    bail!("public pilot cap must be1..1024");
                }
            }
            "--local-development" => {
                local_development = Some(
                    args.next()
                        .context("--local-development requires an isolated directory")?,
                )
            }
            "--db" => {
                explicit_db = true;
                db = args.next().context("--db requires a path")?;
            }
            "--help" | "-h" => {
                println!("arkline-server [--bind 127.0.0.1:7777] [--db data/arkline.sqlite]\nExplicit isolated local world: --local-development DIR (loopback only, incompatible with --db and account auth).\nRaw TCP is plaintext and must stay private. Optional --ws-bind ADDRESS enables /ws. Add both --tls-cert PEM and --tls-key PEM for native TLS; without them the listener is plaintext and must stay private. --ws-origin ORIGIN adds an allowed browser origin; --ws-max-pilots COUNT (default256).\nVerified readiness: --tls-health ADDRESS --tls-ca PEM --tls-server-name DNS.\nOptional account admission: --auth-config JSON --auth-jwks PUBLIC_JWKS [--auth-required].");

                return Ok(());
            }
            _ => bail!("unknown argument: {arg}"),
        }
    }
    if let Some(address) = tls_health {
        return tls::check_health(
            &address,
            &tls_ca.context("--tls-health requires --tls-ca")?,
            &tls_server_name.context("--tls-health requires --tls-server-name")?,
        );
    }
    if tls_ca.is_some() || tls_server_name.is_some() {
        bail!("--tls-ca and --tls-server-name require --tls-health");
    }
    if local_development.is_some() {
        anyhow::ensure!(!auth_required && auth_config.is_none() && auth_jwks.is_none(),
            "local development and official account authentication are mutually exclusive");
        anyhow::ensure!(
            !explicit_db,
            "--local-development and --db are mutually exclusive"
        );
        arkline_server::local_development::validate_listeners(
            &bind,
            ws_bind.as_deref(),
            &local_origins,
        )?;
        origins = local_origins;
    }
    if auth_config.is_some() || auth_jwks.is_some() || auth_required {
        validate_identity_listeners(
            &bind,
            ws_bind.as_deref(),
            tls_cert.is_some() && tls_key.is_some(),
        )?;
    }
    let tls = tls::load(tls_cert.as_deref(), tls_key.as_deref(), ws_bind.is_some())?;
    let mut authority = if let Some(directory) = local_development {
        eprintln!("Local development realm: loopback only; pilot permissions remain enforced");
        Server::open_local_development(directory)?
    } else { Server::open(db)? };
    match (auth_config, auth_jwks) {
        (Some(config), Some(keys)) => {
            let config = read_bounded(&config, 16384)?;
            anyhow::ensure!(config.len() <= 16384, "auth configuration too large");
            let keys = read_bounded(&keys, arkline_server::access_tokens::MAX_JWKS_BYTES)?;
            authority.configure_auth(serde_json::from_slice(&config)?, &keys, auth_required)?;
        }
        (None, None) if !auth_required => {}
        _ => bail!("auth requires both --auth-config and --auth-jwks"),
    }
    let server = Arc::new(Mutex::new(authority));

    let tick_server = server.clone();
    let health = Arc::new(Health {
        started: Instant::now(),
        last_tick: AtomicU64::new(0),
    });
    let tick_health = health.clone();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(100));
        let Ok(mut server) = tick_server.lock() else {
            break;
        };
        if let Err(error) = server.tick() {
            eprintln!("Flight tick failed: {error}");
        } else {
            tick_health.last_tick.store(
                tick_health.started.elapsed().as_millis() as u64 + 1,
                Ordering::Release,
            );
        }
    });
    let listener = TcpListener::bind(&bind)?;
    println!("Arkline server listening on {}", listener.local_addr()?);
    let active = Arc::new(AtomicUsize::new(0));
    if let Some(bind) = ws_bind {
        let listener = TcpListener::bind(&bind)?;
        println!(
            "Arkline WebSocket listening on {} /ws",
            listener.local_addr()?
        );
        let server = server.clone();
        let active = active.clone();
        let origins = Arc::new(origins);
        let limits = Arc::new(Mutex::new(websocket::Limits::default()));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if !limits.lock().is_ok_and(|mut l| l.admit()) {
                    continue;
                }
                let Some(guard) = ConnectionGuard::claim(active.clone()) else {
                    continue;
                };
                let server = server.clone();
                let limits = limits.clone();
                let origins = origins.clone();
                let health = health.clone();
                let tls = tls.clone();
                std::thread::spawn(move || {
                    let _guard = guard;
                    // Never include handshake headers or message data in logs.
                    if websocket::serve_ws(stream, server, limits, origins, max_pilots, health, tls)
                        .is_err()
                    {
                        eprintln!("WebSocket connection ended");
                    }
                });
            }
        });
    }
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        let Some(guard) = ConnectionGuard::claim(active.clone()) else {
            continue;
        };
        let server = server.clone();
        std::thread::spawn(move || {
            let _guard = guard;
            // Do not log requests: Connect contains a bearer credential.
            if let Err(error) = serve(stream, server) {
                eprintln!("Connection ended: {error}");
            }
        });
    }
    Ok(())
}
struct Health {
    started: Instant,
    last_tick: AtomicU64,
}
impl Health {
    fn ready(&self) -> bool {
        let last = self.last_tick.load(Ordering::Acquire);
        last != 0 && (self.started.elapsed().as_millis() as u64).saturating_sub(last - 1) <= 3000
    }
}
struct ConnectionGuard(Arc<AtomicUsize>);
impl ConnectionGuard {
    fn claim(active: Arc<AtomicUsize>) -> Option<Self> {
        if active.fetch_add(1, Ordering::SeqCst) >= 64 {
            active.fetch_sub(1, Ordering::SeqCst);
            None
        } else {
            Some(Self(active))
        }
    }
}
impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}
struct TransportSession {
    server: Arc<Mutex<Server>>,
    pilot: Option<String>,
    account_expires: Option<u64>,
}
impl TransportSession {
    fn remaining(&self, limit: Duration) -> Option<Duration> {
        match self.account_expires {
            Some(expiry) => {
                let millis = expiry
                    .saturating_mul(1000)
                    .saturating_sub(arkline_server::now_ms());
                (millis > 0).then(|| limit.min(Duration::from_millis(millis)))
            }
            None => Some(limit),
        }
    }
}
impl Drop for TransportSession {
    fn drop(&mut self) {
        // Release presence on EOF, timeouts, invalid oversized frames, write
        // errors, and unwinding; taking the pilot makes cleanup idempotent.
        self.server
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .disconnect(&mut self.pilot);
    }
}
fn serve(mut stream: TcpStream, server: Arc<Mutex<Server>>) -> Result<()> {
    let mut session = TransportSession {
        server: server.clone(),
        pilot: None,
        account_expires: None,
    };
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let Some(timeout) = session.remaining(Duration::from_secs(120)) else {
            return Ok(());
        };
        reader.get_ref().set_read_timeout(Some(timeout))?;
        let mut bytes = Vec::new();
        loop {
            let Some(timeout) = session.remaining(Duration::from_secs(120)) else {
                return Ok(());
            };
            reader.get_ref().set_read_timeout(Some(timeout))?;
            let available = reader.fill_buf()?;
            if available.is_empty() {
                return Ok(());
            }
            let count = available
                .iter()
                .position(|b| *b == b'\n')
                .map_or(available.len(), |i| i + 1);
            if bytes.len() + count > MAX_REQUEST_BYTES {
                send(&mut stream, &Response::error("request exceeds 16384 bytes"))?;
                return Ok(());
            }
            let complete = available[count - 1] == b'\n';
            bytes.extend_from_slice(&available[..count]);
            reader.consume(count);
            if complete {
                break;
            }
        }
        let response = match serde_json::from_slice::<Request>(&bytes) {
            Ok(request) => server
                .lock()
                .map_err(|_| anyhow::anyhow!("server state lock poisoned"))?
                .handle_authenticated(&mut session.pilot, &mut session.account_expires, request),
            Err(_) => Response::error("invalid request JSON or command schema"),
        };
        send(&mut stream, &response)?;
    }
}
fn send(stream: &mut TcpStream, response: &Response) -> Result<()> {
    serde_json::to_writer(&mut *stream, response)?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    Ok(())
}

fn read_bounded(path: &str, limit: usize) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    anyhow::ensure!(
        bytes.len() <= limit,
        "identity configuration exceeds size limit"
    );
    Ok(bytes)
}

fn validate_identity_listeners(tcp: &str, ws: Option<&str>, tls: bool) -> Result<()> {
    let loopback = |address: &str| {
        address
            .parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.ip().is_loopback())
    };
    anyhow::ensure!(
        loopback(tcp),
        "account authentication requires loopback raw TCP; expose verified WSS instead"
    );
    anyhow::ensure!(
        ws.is_none_or(|address| tls || loopback(address)),
        "account authentication requires TLS for non-loopback WebSocket listeners"
    );
    Ok(())
}
#[cfg(test)]
mod identity_listener_tests {
    use super::*;
    #[test]
    fn account_tokens_require_encrypted_remote_transport() {
        assert!(validate_identity_listeners("127.0.0.1:7777", Some("0.0.0.0:8443"), true).is_ok());
        assert!(
            validate_identity_listeners("127.0.0.1:7777", Some("127.0.0.1:7778"), false).is_ok()
        );
        assert!(validate_identity_listeners("[::1]:7777", None, false).is_ok());
        assert!(validate_identity_listeners("0.0.0.0:7777", None, true).is_err());
        assert!(
            validate_identity_listeners("127.0.0.1:7777", Some("0.0.0.0:7778"), false).is_err()
        );
        assert!(validate_identity_listeners("localhost:7777", None, false).is_err());
    }
}
