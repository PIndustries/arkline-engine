//! Native exchange, identity admission and the unchanged connection/recovery worker.

use super::*;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn terminal_transport_protocol_error(error: &str) -> bool {
    // These prefixes are emitted by our decoders. A new connection cannot make
    // an unsupported reply safe; timeouts and interrupted connections can recover.
    error.starts_with("Invalid server response:")
        || error.starts_with("Invalid server reply:")
        || error.starts_with("Server sent unsupported binary data")
        || error.starts_with("Server response exceeded the size limit")
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn exchange(
    reader: &mut BufReader<TcpStream>,
    request: &Request,
) -> Result<Response, String> {
    exchange_with_timeout(reader, request, Duration::from_secs(5))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn exchange_with_timeout(
    reader: &mut BufReader<TcpStream>,
    request: &Request,
    timeout: Duration,
) -> Result<Response, String> {
    let mut bytes = serde_json::to_vec(request).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    reader
        .get_mut()
        .write_all(&bytes)
        .map_err(|e| format!("Connection interrupted while sending request: {e}"))?;
    let deadline = Instant::now() + timeout;
    let mut line = Vec::new();
    const LIMIT: usize = 16 * 1024 * 1024;
    loop {
        if Instant::now() >= deadline {
            return Err("Server response timed out; the connection may have paused during sleep or a network interruption".into());
        }
        // Keep fragments across transient read timeouts, but bound the whole response.
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(250))
            .max(Duration::from_millis(1));
        reader
            .get_mut()
            .set_read_timeout(Some(remaining))
            .map_err(|e| e.to_string())?;
        let consumed = match reader.fill_buf() {
            Ok([]) => {
                return Err("Server closed the connection before completing its response".into())
            }
            Ok(buffer) => {
                let count = buffer
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(buffer.len(), |index| index + 1)
                    .min(LIMIT - line.len());
                line.extend_from_slice(&buffer[..count]);
                count
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                continue
            }
            Err(e) => {
                return Err(format!(
                    "Connection interrupted while receiving response: {e}"
                ))
            }
        };
        reader.consume(consumed);
        if line.last() == Some(&b'\n') {
            return serde_json::from_slice(&line)
                .map_err(|e| format!("Invalid server response: {e}"));
        }
        if line.len() >= LIMIT {
            return Err("Server response exceeded the size limit".into());
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn connect_transport(server: &str) -> Result<BufReader<TcpStream>, String> {
    let addresses: Vec<_> = server
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .collect();
    let stream = addresses
        .iter()
        .find_map(|address| TcpStream::connect_timeout(address, Duration::from_secs(3)).ok())
        .ok_or_else(|| {
            format!("Cannot connect to {server}. Start arkline-server, then reconnect.")
        })?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    stream.set_nodelay(true).map_err(|e| e.to_string())?;
    Ok(BufReader::new(stream))
}

/// Drop all requests accumulated before recovery, never replay actions or controls.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn discard_pending(
    commands: &Receiver<Request>,
    controls: &pilot_input::Mailbox,
) -> bool {
    controls.clear();
    loop {
        match commands.try_recv() {
            Ok(_) => {}
            Err(mpsc::TryRecvError::Empty) => return true,
            Err(mpsc::TryRecvError::Disconnected) => return false,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
pub(crate) struct WorkerIdentity {
    pub(crate) scope: Option<String>,
    pub(crate) account: Option<account_login::Credential>,
    pub(crate) link_existing: bool,
}
#[cfg(not(target_arch = "wasm32"))]
impl WorkerIdentity {
    pub(crate) fn request(
        &self,
        server: &str,
        pilot: &str,
        token: Option<String>,
    ) -> Result<Request, String> {
        if let Some(account) = &self.account {
            if !account.matches(server, pilot) {
                return Err("Account session expired or changed. Sign in again.".into());
            }
            Ok(Request::AccountConnect {
                access_token: account.token.clone(),
                pilot: pilot.into(),
                legacy_resume_token: if self.link_existing { token } else { None },
                link_existing: self.link_existing,
                protocol_version: PROTOCOL_VERSION,
                supports_pioneer_interior: true,
                supports_fleet_hangar: true,
                supports_room_controls: true,
                supports_docked_traversal_v2: true,
                world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            })
        } else {
            Ok(Request::Connect {
                pilot: pilot.into(),
                resume_token: token,
                protocol_version: PROTOCOL_VERSION,
                supports_pioneer_interior: true,
                supports_fleet_hangar: true,
                supports_room_controls: true,
                supports_docked_traversal_v2: true,
                world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
            })
        }
    }
}
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn worker(
    server: String,
    pilot: String,
    dir: PathBuf,
    identity: WorkerIdentity,
    commands: Receiver<Request>,
    updates: Sender<Update>,
    controls: pilot_input::Mailbox,
) {
    let run = || -> Result<(), String> {
        let path = credentials_path(&dir, identity.scope.as_deref().unwrap_or(&server), &pilot);
        let mut token = if identity.account.is_some() && !identity.link_existing {
            None
        } else {
            match fs::read_to_string(&path) {
                Ok(v) => Some(v),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(format!("Cannot read saved credential: {e}")),
            }
        };
        if identity.account.is_some() && identity.link_existing && token.is_none() {
            return Err("Cannot link: this device has no saved credential for that pilot. Use the original device or operator recovery.".into());
        }
        // Check storage before creating a server-side identity.
        if identity.account.is_none() {
            fs::create_dir_all(&dir)
                .map_err(|e| format!("Credential directory unavailable: {e}"))?;
        }
        let mut reader = transport::Connection::connect(&server)?;
        let mut response = reader.exchange(&identity.request(&server, &pilot, token.clone())?)?;
        if !response.ok {
            return Err(response.message);
        }
        validate_world_admission_response(&response)?;
        if identity.account.is_some() && response.resume_token.is_some() {
            return Err("Account admission returned an unexpected legacy credential".into());
        }
        if let Some(issued) = response.resume_token.take() {
            save_token(&path, &issued)?;
            token = Some(issued);
        }
        updates.send(Update::Ready).map_err(|e| e.to_string())?;
        updates
            .send(Update::Response(Box::new(response), true))
            .map_err(|e| e.to_string())?;
        let mut last_snapshot = Instant::now();
        let mut retries_left = 3u32;
        loop {
            if identity
                .account
                .as_ref()
                .is_some_and(|a| !a.matches(&server, &pilot))
            {
                reader.close();
                return Err("Account session expired. Sign in again.".into());
            }
            let (request, visible) = match commands.recv_timeout(Duration::from_millis(50)) {
                Ok(request) => (request, true),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(request) = controls.take() {
                        (request, false)
                    } else if last_snapshot.elapsed() >= Duration::from_millis(350) {
                        (Request::Snapshot, false)
                    } else {
                        continue;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            };
            // Lease transitions invalidate controls sampled under the old view,
            // including unwrapped own-ship samples queued before remote takeover.
            if matches!(
                request,
                Request::BeginRemoteControl { .. } | Request::EndRemoteControl
            ) {
                controls.clear();
            }
            let response = match reader.exchange(&request) {
                Ok(response) => response,
                Err(error) => {
                    if terminal_transport_protocol_error(&error) {
                        controls.clear();
                        reader.close();
                        return Err(error);
                    }
                    let uncertainty = if visible {
                        " Action outcome unknown; inspect refreshed state before retrying. The action will not be replayed."
                    } else {
                        ""
                    };
                    let explanation = format!("{error}.{uncertainty} Reconnecting; queued actions and flight input are discarded.");
                    updates
                        .send(Update::Reconnecting(explanation.clone()))
                        .map_err(|e| e.to_string())?;
                    reader.close();
                    if token.is_none() && identity.account.is_none() {
                        return Err(format!("{explanation} No saved identity is available for safe automatic recovery."));
                    }
                    let mut recovered = None;
                    let mut last_error = error;
                    while retries_left > 0 {
                        retries_left -= 1;
                        if !discard_pending(&commands, &controls) {
                            return Ok(());
                        }
                        // A dropped sender cancels recovery promptly; received requests are discarded.
                        let wait_until =
                            Instant::now() + Duration::from_millis(250 * (3 - retries_left) as u64);
                        while Instant::now() < wait_until {
                            match commands
                                .recv_timeout(wait_until.saturating_duration_since(Instant::now()))
                            {
                                Ok(_) => {}
                                Err(mpsc::RecvTimeoutError::Timeout) => break,
                                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                            }
                        }
                        let attempt = (|| {
                            let mut next = transport::Connection::connect(&server)?;
                            let mut resumed = next
                                .exchange(&identity.request(&server, &pilot, token.clone())?)
                                .map_err(|error| {
                                    if terminal_transport_protocol_error(&error) {
                                        format!("Resume rejected: {error}")
                                    } else {
                                        error
                                    }
                                })?;
                            if !resumed.ok {
                                return Err(format!("Resume rejected: {}", resumed.message));
                            }
                            validate_world_admission_response(&resumed)
                                .map_err(|error| format!("Resume rejected: {error}"))?;
                            if identity.account.is_some() && resumed.resume_token.is_some() {
                                return Err(
                                    "Account admission returned an unexpected legacy credential"
                                        .into(),
                                );
                            }
                            if let Some(issued) = resumed.resume_token.take() {
                                save_token(&path, &issued)?;
                                token = Some(issued);
                            }
                            Ok::<_, String>((next, resumed))
                        })();
                        match attempt {
                            Ok(value) => {
                                recovered = Some(value);
                                break;
                            }
                            Err(error) => {
                                let rejected = error.starts_with("Resume rejected:");
                                last_error = error;
                                if rejected {
                                    break;
                                }
                            }
                        }
                    }
                    let Some((next, mut resumed)) = recovered else {
                        return Err(format!("Automatic reconnect stopped: {last_error}.{uncertainty} Use Reconnect when the server is available."));
                    };
                    if !discard_pending(&commands, &controls) {
                        return Ok(());
                    }
                    reader = next;
                    resumed.message =
                        format!("Connection restored; authoritative state refreshed.{uncertainty}");
                    updates.send(Update::Ready).map_err(|e| e.to_string())?;
                    updates
                        .send(Update::Response(Box::new(resumed), true))
                        .map_err(|e| e.to_string())?;
                    last_snapshot = Instant::now();
                    continue;
                }
            };
            validate_world_response(&response)?;
            last_snapshot = Instant::now();
            updates
                .send(Update::Response(Box::new(response), visible))
                .map_err(|e| e.to_string())?;
        }
    };
    if let Err(error) = run() {
        let _ = updates.send(Update::Failure(error));
    }
}
