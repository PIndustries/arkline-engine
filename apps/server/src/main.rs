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
    let mut db = "data/arkline.sqlite".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
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
            "--ws-origin" => origins.push(
                args.next()
                    .context("--ws-origin requires an exact HTTPS origin")?,
            ),
            "--ws-max-pilots" => {
                max_pilots = args
                    .next()
                    .context("--ws-max-pilots requires a count")?
                    .parse()?;
                if !(1..=1024).contains(&max_pilots) {
                    bail!("public pilot cap must be1..1024");
                }
            }
            "--db" => db = args.next().context("--db requires a path")?,
            "--help" | "-h" => {
                println!("arkline-server [--bind 127.0.0.1:7777] [--db data/arkline.sqlite]\nRaw TCP is plaintext and must stay private. Optional --ws-bind ADDRESS enables /ws. Add both --tls-cert PEM and --tls-key PEM for native TLS; without them the listener is plaintext and must stay private. --ws-origin ORIGIN adds an allowed browser origin; --ws-max-pilots COUNT (default256).\nVerified readiness: --tls-health ADDRESS --tls-ca PEM --tls-server-name DNS.");
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
    let tls = tls::load(tls_cert.as_deref(), tls_key.as_deref(), ws_bind.is_some())?;
    let server = Arc::new(Mutex::new(Server::open(db)?));
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
    };
    stream.set_read_timeout(Some(Duration::from_secs(120)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    stream.set_nodelay(true)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    loop {
        let mut bytes = Vec::new();
        loop {
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
                .handle(&mut session.pilot, request),
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
