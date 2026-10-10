use anyhow::{bail, Context, Result};
use arkline_protocol::{Request, Response};
use arkline_server::Server;
use std::{
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const MAX_REQUEST_BYTES: usize = 16 * 1024;
fn main() -> Result<()> {
    let mut bind = "127.0.0.1:7777".to_string();
    let mut db = "data/arkline.sqlite".to_string();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--bind" => bind = args.next().context("--bind requires an address")?,
            "--db" => db = args.next().context("--db requires a path")?,
            "--help" | "-h" => {
                println!("arkline-server [--bind 127.0.0.1:7777] [--db data/arkline.sqlite]\nLocal prototype: plaintext transport. Keep on loopback or a trusted encrypted tunnel.");
                return Ok(());
            }
            _ => bail!("unknown argument: {arg}"),
        }
    }
    let server = Arc::new(Mutex::new(Server::open(db)?));
    let listener = TcpListener::bind(&bind)?;
    println!("Arkline server listening on {}", listener.local_addr()?);
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else {
            continue;
        };
        if active.fetch_add(1, Ordering::SeqCst) >= 64 {
            active.fetch_sub(1, Ordering::SeqCst);
            drop(stream);
            continue;
        }
        let server = server.clone();
        let active = active.clone();
        std::thread::spawn(move || {
            struct Guard(Arc<AtomicUsize>);
            impl Drop for Guard {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::SeqCst);
                }
            }
            let _guard = Guard(active);
            // Do not log requests: Connect contains a bearer credential.
            if let Err(error) = serve(stream, server) {
                eprintln!("Connection ended: {error}");
            }
        });
    }
    Ok(())
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
