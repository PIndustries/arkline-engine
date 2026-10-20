//! Independent economic replay over the public export protocol, never database access.
use arkline_protocol::{Request, Response};
use arkline_verifier::{verify, Checkpoint};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpStream, ToSocketAddrs},
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;

/// Call on a background thread. Use a checkpoint path scoped to the universe,
/// not the pilot: it anchors history, not authentication. Never retries commands.
pub fn audit(server: &str, checkpoint_path: &Path) -> Result<String, String> {
    // Cockpit windows share a universe checkpoint. Serialize the whole read /
    // verify / replace cycle so a slower audit cannot overwrite a newer anchor.
    // Release explicitly when this audit ends: a concurrently forked child may
    // still hold the same open file description until its exec completes.
    let _lock = checkpoint_lock(checkpoint_path)?;
    let pinned: Option<Checkpoint> = match fs::read(checkpoint_path) {
        Ok(bytes) => Some(
            serde_json::from_slice(&bytes)
                .map_err(|e| format!("Invalid saved audit checkpoint: {e}"))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Cannot read audit checkpoint: {e}")),
    };
    let response = if server.starts_with("wss://") || server.starts_with("ws://") {
        super::transport::Connection::connect(server)?.exchange(&Request::ExportLedger)?
    } else {
        let deadline = Instant::now() + Duration::from_secs(15);
        let addresses = server
            .to_socket_addrs()
            .map_err(|e| format!("Cannot resolve audit server: {e}"))?;
        let mut connected = None;
        for address in addresses {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            if let Ok(socket) =
                TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(3)))
            {
                connected = Some(socket);
                break;
            }
        }
        let mut socket = connected.ok_or_else(|| "Cannot connect to audit server".to_string())?;
        socket
            .set_write_timeout(Some(Duration::from_secs(3)))
            .map_err(|e| e.to_string())?;
        let mut request = serde_json::to_vec(&Request::ExportLedger).map_err(|e| e.to_string())?;
        request.push(b'\n');
        socket
            .write_all(&request)
            .map_err(|e| format!("Cannot request ledger: {e}"))?;
        let mut bytes = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("Audit export timed out".into());
            }
            socket
                .set_read_timeout(Some(remaining))
                .map_err(|e| e.to_string())?;
            let mut chunk = [0u8; 8192];
            let n = socket
                .read(&mut chunk)
                .map_err(|e| format!("Cannot read ledger export: {e}"))?;
            if n == 0 {
                return Err("Audit server disconnected before completing export".into());
            }
            let end = chunk[..n].iter().position(|b| *b == b'\n');
            let count = end.unwrap_or(n);
            if bytes.len() + count > MAX_EXPORT_BYTES {
                return Err("Audit export exceeds the 64 MiB preview limit".into());
            }
            bytes.extend_from_slice(&chunk[..count]);
            if end.is_some() {
                break;
            }
        }
        serde_json::from_slice::<Response>(&bytes)
            .map_err(|e| format!("Invalid ledger response: {e}"))?
    };
    if !response.ok {
        return Err(format!(
            "Audit server rejected export: {}",
            response.message
        ));
    }
    let export = response
        .ledger
        .ok_or_else(|| "Audit response omitted ledger".to_string())?;
    let report = verify(&export, pinned.as_ref())
        .map_err(|e| format!("Economic verification failed: {e:#}"))?;
    save_checkpoint(checkpoint_path, &report.head)?;
    let anchor = if pinned.is_some() {
        "Previously pinned history matched."
    } else {
        "First local checkpoint pinned; earlier history has no independent trust anchor."
    };
    Ok(format!(
        "Verified economic history #{}: {} pilots, {} trades, {} credits, {} belt ore. {} Economic consistency only; does not prove mining occurred or operator honesty.",
        report.head.sequence, report.accounts, report.trades, report.total_credits, report.belt_ore, anchor
    ))
}

/// Closing only this handle can leave the lock held by an inherited or cloned
/// handle. Explicit unlock ends this audit's ownership on every return path.
struct CheckpointLock(fs::File);
impl Drop for CheckpointLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

fn checkpoint_lock(path: &Path) -> Result<CheckpointLock, String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create audit directory: {e}"))?;
    let filename = path
        .file_name()
        .ok_or_else(|| "Checkpoint needs a filename".to_string())?
        .to_string_lossy();
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(parent.join(format!(".{filename}.lock")))
        .map_err(|e| format!("Cannot open audit lock: {e}"))?;
    file.try_lock()
        .map_err(|e| format!("Another cockpit may be auditing this universe; try again: {e}"))?;
    Ok(CheckpointLock(file))
}

fn save_checkpoint(path: &Path, checkpoint: &Checkpoint) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| format!("Cannot create audit directory: {e}"))?;
    let filename = path
        .file_name()
        .ok_or_else(|| "Checkpoint needs a filename".to_string())?
        .to_string_lossy();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = parent.join(format!(".{filename}.{}.{nonce}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // A failed create never removes a file we did not create.
    let mut file = options
        .open(&temporary)
        .map_err(|e| format!("Cannot create audit checkpoint: {e}"))?;
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        file.write_all(&serde_json::to_vec_pretty(checkpoint)?)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Verified history, but checkpoint persistence failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::*;
    use std::{
        io::{BufRead, BufReader},
        net::TcpListener,
    };

    fn serve(export: LedgerExport) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let task = std::thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(socket);
            let mut request = String::new();
            reader.read_line(&mut request).unwrap();
            assert!(matches!(
                serde_json::from_str::<Request>(&request).unwrap(),
                Request::ExportLedger
            ));
            let response = Response {
                auth: None,
                assets: None,
                ok: true,
                message: "Export".into(),
                resume_token: None,
                snapshot: None,
                ledger: Some(export),
            };
            serde_json::to_writer(reader.get_mut(), &response).unwrap();
            reader.get_mut().write_all(b"\n").unwrap();
        });
        (address, task)
    }

    #[test]
    fn successful_audit_pins_history_and_failure_preserves_anchor() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("arkline-ui-audit-{}-{nonce}", std::process::id()));
        let path = dir.join("checkpoint.json");
        let export = LedgerExport {
            protocol_version: PROTOCOL_VERSION,
            entries: vec![],
            snapshot: Snapshot {
                walk_sprint_v1: false,
                docked_traversal_capability: None,
                world_layout: None,
                exterior: None,
                resource_jobs: vec![],
                fleet_links: vec![],
                remote_controls: vec![],
                surface_props: vec![],
                traffic: vec![],
                stored_ore: 0,
                asteroids: vec![],
                server_time_ms: 0,
                online_pilots: vec![],
                pilots: vec![],
                orders: vec![],
                belt_ore: INITIAL_BELT_ORE,
                ledger_sequence: 0,
                ledger_hash: GENESIS_HASH.into(),
            },
        };
        let (server, task) = serve(export.clone());
        let message = audit(&server, &path).unwrap();
        task.join().unwrap();
        assert!(message.contains("First local checkpoint pinned"));
        let original = fs::read(&path).unwrap();
        drop(checkpoint_lock(&path).expect("successful audit must release checkpoint ownership"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let (server, task) = serve(export.clone());
        assert!(audit(&server, &path)
            .unwrap()
            .contains("Previously pinned history matched"));
        task.join().unwrap();
        let mut tampered = export;
        tampered.snapshot.belt_ore -= 1;
        let (server, task) = serve(tampered);
        let failure = audit(&server, &path).unwrap_err();
        assert!(
            failure.contains("snapshot belt mismatch"),
            "unexpected audit failure: {failure}"
        );
        task.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        drop(
            checkpoint_lock(&path).expect("verification failure must release checkpoint ownership"),
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn checkpoint_owner_drop_releases_lock_despite_live_duplicate() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "arkline-ui-audit-inherited-lock-{}-{nonce}",
            std::process::id()
        ));
        let path = dir.join("checkpoint.json");
        let owner = checkpoint_lock(&path).unwrap();
        // try_clone shares the same open file description, just as an inherited
        // descriptor does between fork and exec in another parallel test.
        let inherited = owner.0.try_clone().unwrap();
        let failure = checkpoint_lock(&path)
            .err()
            .expect("concurrent ownership must still be rejected");
        assert!(failure.contains("Another cockpit"), "{failure}");
        drop(owner);
        let next = checkpoint_lock(&path)
            .unwrap_or_else(|error| panic!("ended audit left inherited lock ownership: {error}"));
        drop(inherited);
        let failure = checkpoint_lock(&path)
            .err()
            .expect("closing an old duplicate must not release the new audit's lock");
        assert!(failure.contains("Another cockpit"), "{failure}");
        drop(next);
        drop(checkpoint_lock(&path).expect("new audit releases its own lock"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_checkpoint_failure_releases_lock_and_preserves_bytes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "arkline-ui-audit-invalid-lock-{}-{nonce}",
            std::process::id()
        ));
        let path = dir.join("checkpoint.json");
        drop(checkpoint_lock(&path).unwrap());
        let original = b"invalid checkpoint";
        fs::write(&path, original).unwrap();
        let failure = audit("127.0.0.1:1", &path).unwrap_err();
        assert!(
            failure.contains("Invalid saved audit checkpoint"),
            "unexpected audit failure: {failure}"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        drop(
            checkpoint_lock(&path).expect("early audit failure must release checkpoint ownership"),
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_cockpit_audit_cannot_replace_checkpoint() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "arkline-ui-audit-lock-{}-{nonce}",
            std::process::id()
        ));
        let path = dir.join("checkpoint.json");
        let lock = checkpoint_lock(&path).unwrap();
        let original = b"trusted-checkpoint";
        fs::write(&path, original).unwrap();
        // Must fail before reading the deliberately invalid checkpoint or
        // attempting any network access while another cockpit owns the lock.
        let failure = audit("127.0.0.1:1", &path).unwrap_err();
        assert!(failure.contains("Another cockpit"), "{failure}");
        assert_eq!(fs::read(&path).unwrap(), original);
        drop(lock);
        let next = checkpoint_lock(&path).unwrap();
        drop(next);
        fs::remove_dir_all(dir).unwrap();
    }
}
