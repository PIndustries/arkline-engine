use anyhow::{bail, ensure, Context, Result};
use arkline_protocol::{LedgerExport, Request, Response};
use arkline_verifier::{verify, Checkpoint};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
    time::Duration,
};

const MAX_EXPORT_BYTES: u64 = 64 * 1024 * 1024;

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .context("output needs a filename")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        // Windows cannot replace a pathname while our temporary handle is open.
        drop(file);
        fs::rename(&temporary, path)?;
        // Opening a directory as File is supported on Unix, not Windows.
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut server = None;
    let mut file = None;
    let mut save = None;
    let mut checkpoint = None;
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            println!("arkline-verifier (--server HOST:PORT | --file EXPORT.json) [--save VERIFIED.json] [--checkpoint PIN.json]\nChecks economic consistency, not proof mining occurred or operator honesty.\nWithout a checkpoint, coherent history rewrites and rollbacks cannot be detected.");
            return Ok(());
        }
        let slot = match arg.as_str() {
            "--server" => &mut server,
            "--file" => &mut file,
            "--save" => &mut save,
            "--checkpoint" => &mut checkpoint,
            _ => bail!("unknown argument {arg}; use --help"),
        };
        ensure!(slot.is_none(), "duplicate argument {arg}");
        *slot = Some(
            args.next()
                .with_context(|| format!("missing value for {arg}"))?,
        );
    }
    ensure!(
        server.is_some() ^ file.is_some(),
        "provide exactly one of --server or --file"
    );
    if let (Some(save), Some(pin)) = (&save, &checkpoint) {
        ensure!(
            std::path::absolute(save)? != std::path::absolute(pin)?,
            "save and checkpoint must be different paths"
        );
    }
    let pinned: Option<Checkpoint> = match &checkpoint {
        Some(path) => match fs::read(path) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes).context("invalid checkpoint")?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("read checkpoint"),
        },
        None => None,
    };
    let export: LedgerExport = if let Some(server) = server {
        let mut socket = TcpStream::connect(&server).context("connect to ledger server")?;
        socket.set_read_timeout(Some(Duration::from_secs(15)))?;
        socket.set_write_timeout(Some(Duration::from_secs(15)))?;
        serde_json::to_writer(&mut socket, &Request::ExportLedger)?;
        socket.write_all(b"\n")?;
        let mut line = String::new();
        BufReader::new(socket.take(MAX_EXPORT_BYTES + 1)).read_line(&mut line)?;
        ensure!(
            line.len() as u64 <= MAX_EXPORT_BYTES,
            "export exceeds 64 MiB limit"
        );
        let response: Response = serde_json::from_str(&line).context("invalid server response")?;
        ensure!(response.ok, "server rejected export: {}", response.message);
        response.ledger.context("response missing ledger")?
    } else {
        let mut bytes = Vec::new();
        fs::File::open(file.unwrap())?
            .take(MAX_EXPORT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_EXPORT_BYTES,
            "export exceeds 64 MiB limit"
        );
        serde_json::from_slice(&bytes).context("invalid ledger export JSON")?
    };
    let report = verify(&export, pinned.as_ref())?;
    if let Some(path) = save {
        atomic_write(Path::new(&path), &serde_json::to_vec_pretty(&export)?)?;
    }
    if let Some(path) = &checkpoint {
        atomic_write(Path::new(path), &serde_json::to_vec_pretty(&report.head)?)?;
    }
    println!(
        "VERIFIED: {} accounts, {} trades, {} credits, {} belt ore, {} stored ore",
        report.accounts, report.trades, report.total_credits, report.belt_ore, report.stored_ore
    );
    println!("Head: {} {}", report.head.sequence, report.head.hash);
    match (checkpoint, pinned) {
        (None, _) => {
            println!("No checkpoint: coherent history rewrites and rollbacks cannot be detected.")
        }
        (Some(_), None) => {
            println!("First checkpoint pinned; prior history has no independently trusted anchor.")
        }
        (Some(_), Some(_)) => {
            println!("Pinned history matched; checkpoint advanced after complete verification.")
        }
    }
    println!("Economic consistency, not proof mining occurred or operator honesty. Locations are not audited.");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("VERIFICATION FAILED: {error:#}");
        std::process::exit(1);
    }
}
