use super::*;
use std::net::TcpListener;
fn response() -> Response {
    Response {
        physical_cargo: None,
        auth: None,
        assets: None,
        ok: true,
        message: "Connected".into(),
        resume_token: None,
        snapshot: Some(
            serde_json::from_value(serde_json::json!({
                "server_time_ms": 0, "pilots": [], "orders": [],
                "belt_ore": INITIAL_BELT_ORE, "ledger_sequence": 0,
                "ledger_hash": GENESIS_HASH,
            }))
            .unwrap(),
        ),
        ledger: None,
    }
}
fn receive(reader: &mut BufReader<TcpStream>) -> Request {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}
fn reply(reader: &mut BufReader<TcpStream>, response: Response) {
    let mut bytes = serde_json::to_vec(&response).unwrap();
    bytes.push(b'\n');
    reader.get_mut().write_all(&bytes).unwrap();
}
#[test]
fn fragments_survive_transient_read_timeouts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        assert!(matches!(receive(&mut reader), Request::Snapshot));
        let mut bytes = serde_json::to_vec(&response()).unwrap();
        bytes.push(b'\n');
        let split = bytes.len() / 2;
        reader.get_mut().write_all(&bytes[..split]).unwrap();
        std::thread::sleep(Duration::from_millis(350));
        reader.get_mut().write_all(&bytes[split..]).unwrap();
    });
    let mut reader = BufReader::new(TcpStream::connect(address).unwrap());
    assert!(
        exchange_with_timeout(&mut reader, &Request::Snapshot, Duration::from_secs(2))
            .unwrap()
            .ok
    );
    endpoint.join().unwrap();
}
#[test]
fn partial_response_timeout_has_actionable_message_and_bounded_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        receive(&mut reader);
        reader.get_mut().write_all(b"{\"ok\":").unwrap();
        std::thread::sleep(Duration::from_millis(400));
    });
    let mut reader = BufReader::new(TcpStream::connect(address).unwrap());
    let started = Instant::now();
    let error = exchange_with_timeout(&mut reader, &Request::Snapshot, Duration::from_millis(100))
        .unwrap_err();
    assert!(
        error.contains("timed out") && error.contains("sleep"),
        "{error}"
    );
    assert!(started.elapsed() < Duration::from_millis(350));
    endpoint.join().unwrap();
}
#[test]
fn recovery_resumes_credentials_and_discards_uncertain_actions_and_controls() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let dir = std::env::temp_dir().join(format!(
        "arkline-transport-recover-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    let (accepted_tx, accepted_rx) = mpsc::channel();
    let (close_tx, close_rx) = mpsc::channel();
    let (checked_tx, checked_rx) = mpsc::channel();
    let endpoint = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        assert!(matches!(
            receive(&mut reader),
            Request::Connect {
                resume_token: None,
                ..
            }
        ));
        let mut connected = response();
        connected.resume_token = Some("private-test-token".into());
        reply(&mut reader, connected);
        assert!(matches!(
            receive(&mut reader),
            Request::Buy {
                order_id: 7,
                units: 2
            }
        ));
        // The request could have committed. Lose its response, then permit recovery.
        accepted_tx.send(()).unwrap();
        close_rx.recv().unwrap();
        drop(reader);
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let mut reader = BufReader::new(stream);
        match receive(&mut reader) {
            Request::Connect {
                resume_token: Some(token),
                ..
            } => assert_eq!(token, "private-test-token"),
            _ => panic!("recovery must authenticate only"),
        }
        reply(&mut reader, response());
        assert!(
            matches!(receive(&mut reader), Request::Snapshot),
            "recovery replayed an action or stale flight controls"
        );
        reply(&mut reader, response());
        checked_tx.send(()).unwrap();
        // Keep socket alive until worker exit, rather than causing another recovery.
        let mut ignored = String::new();
        let _ = reader.read_line(&mut ignored);
    });
    let (commands_tx, commands_rx) = mpsc::channel();
    let (updates_tx, updates_rx) = mpsc::channel();
    let controls = pilot_input::Mailbox::default();
    let (server, directory, mailbox) = (address.clone(), dir.clone(), controls.clone());
    let worker_thread = std::thread::spawn(move || {
        worker(
            server,
            "RecoveryPilot".into(),
            directory,
            WorkerIdentity::default(),
            commands_rx,
            updates_tx,
            mailbox,
        )
    });
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Ready
    ));
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Response(_, true)
    ));
    commands_tx
        .send(Request::Buy {
            order_id: 7,
            units: 2,
        })
        .unwrap();
    accepted_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    commands_tx.send(Request::DepositOre { units: 25 }).unwrap();
    controls.publish_for(
        pilot_input::Controls {
            translation: [1.0, 0.0, 0.0],
            rotation: [0.0; 3],
        },
        Some("DelegatedWorker".into()),
    );
    close_tx.send(()).unwrap();
    match updates_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        Update::Reconnecting(message) => assert!(message.contains("outcome unknown")),
        _ => panic!("expected reconnect status"),
    }
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Ready
    ));
    match updates_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        Update::Response(response, true) => {
            assert!(response.message.contains("outcome unknown"));
            assert!(response.resume_token.is_none());
        }
        _ => panic!("expected refreshed state"),
    }
    checked_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    drop(commands_tx);
    worker_thread.join().unwrap();
    endpoint.join().unwrap();
    assert_eq!(
        fs::read_to_string(credentials_path(&dir, &address, "RecoveryPilot")).unwrap(),
        "private-test-token"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn trickling_bytes_cannot_extend_total_response_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let endpoint = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        receive(&mut reader);
        for _ in 0..8 {
            let _ = reader.get_mut().write_all(b" ");
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let mut reader = BufReader::new(TcpStream::connect(address).unwrap());
    let started = Instant::now();
    assert!(
        exchange_with_timeout(&mut reader, &Request::Snapshot, Duration::from_millis(130))
            .unwrap_err()
            .contains("timed out")
    );
    assert!(started.elapsed() < Duration::from_millis(350));
    drop(reader);
    endpoint.join().unwrap();
}

#[test]
fn snapshot_read_timeout_recovers_identity_without_replaying_snapshot_bytes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let dir = std::env::temp_dir().join(format!(
        "arkline-timeout-resume-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    let endpoint = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        assert!(matches!(receive(&mut reader), Request::Connect { .. }));
        let mut connected = response();
        connected.resume_token = Some("timeout-resume-token".into());
        reply(&mut reader, connected);
        assert!(matches!(receive(&mut reader), Request::Snapshot));
        // A live socket with an incomplete response reproduces the read-timeout path.
        reader.get_mut().write_all(b"{\"ok\":true,").unwrap();
        std::thread::sleep(Duration::from_millis(5200));
        let (next, _) = listener.accept().unwrap();
        let mut recovered = BufReader::new(next);
        match receive(&mut recovered) {
            Request::Connect {
                resume_token: Some(token),
                ..
            } => assert_eq!(token, "timeout-resume-token"),
            _ => panic!("expected resume authentication"),
        }
        reply(&mut recovered, response());
        drop(reader);
        let mut ignored = String::new();
        let _ = recovered.read_line(&mut ignored);
    });
    let (commands_tx, commands_rx) = mpsc::channel();
    let (updates_tx, updates_rx) = mpsc::channel();
    let directory = dir.clone();
    let client = std::thread::spawn(move || {
        worker(
            address,
            "TimeoutPilot".into(),
            directory,
            WorkerIdentity::default(),
            commands_rx,
            updates_tx,
            pilot_input::Mailbox::default(),
        )
    });
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Ready
    ));
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Response(_, true)
    ));
    match updates_rx.recv_timeout(Duration::from_secs(8)).unwrap() {
        Update::Reconnecting(message) => assert!(message.contains("timed out")),
        _ => panic!("expected transient timeout status"),
    }
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Ready
    ));
    match updates_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        Update::Response(response, true) => {
            assert!(response.message.contains("Connection restored"))
        }
        _ => panic!("expected recovery state"),
    }
    drop(commands_tx);
    client.join().unwrap();
    endpoint.join().unwrap();
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn unsupported_connect_layout_never_becomes_ready_or_saves_issued_credential() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let directory = std::env::temp_dir().join(format!(
        "arkline-invalid-admission-layout-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    assert!(!directory.exists());
    let credential = credentials_path(&directory, &address, "LayoutAdmissionPilot");
    let endpoint = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        assert!(matches!(
            receive(&mut reader),
            Request::Connect {
                resume_token: None,
                world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION,
                ..
            }
        ));
        let mut invalid_layout = SceneLayoutRevision::OrbitalV1.definition();
        invalid_layout.schema_version = WORLD_LAYOUT_SCHEMA_VERSION + 1;
        let mut connected = response();
        connected.resume_token = Some("must-not-be-saved-layout-token".into());
        connected.snapshot = Some(
            serde_json::from_value(serde_json::json!({
                "server_time_ms": 0, "pilots": [], "orders": [],
                "belt_ore": INITIAL_BELT_ORE, "ledger_sequence": 0,
                "ledger_hash": GENESIS_HASH, "world_layout": invalid_layout,
            }))
            .unwrap(),
        );
        reply(&mut reader, connected);
    });
    let (commands_tx, commands_rx) = mpsc::channel();
    let (updates_tx, updates_rx) = mpsc::channel();
    let client = std::thread::spawn(move || {
        worker(
            address,
            "LayoutAdmissionPilot".into(),
            directory,
            WorkerIdentity::default(),
            commands_rx,
            updates_tx,
            pilot_input::Mailbox::default(),
        )
    });
    match updates_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        Update::Failure(message) => assert!(message.to_lowercase().contains("layout"), "{message}"),
        _ => panic!("invalid admission must fail before Ready or Response"),
    }
    client.join().unwrap();
    endpoint.join().unwrap();
    assert!(matches!(
        updates_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert!(!credential.exists(), "invalid admission saved a credential");
    drop(commands_tx);
}

#[test]
fn unsupported_later_layout_is_not_published_and_preserves_existing_credential() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let directory = std::env::temp_dir().join(format!(
        "arkline-invalid-update-layout-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    assert!(!directory.exists());
    let credential = credentials_path(&directory, &address, "LayoutResumePilot");
    save_token(&credential, "existing-layout-test-credential").unwrap();
    let endpoint = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream);
        match receive(&mut reader) {
            Request::Connect {
                resume_token: Some(token),
                ..
            } => {
                assert_eq!(token, "existing-layout-test-credential");
            }
            _ => panic!("existing pilot must resume with its saved credential"),
        }
        let mut connected = response();
        connected.snapshot = Some(
            serde_json::from_value(serde_json::json!({
                "server_time_ms": 0, "pilots": [], "orders": [],
                "belt_ore": INITIAL_BELT_ORE, "ledger_sequence": 0,
                "ledger_hash": GENESIS_HASH,
                "world_layout": SceneLayoutRevision::OrbitalV1.definition(),
            }))
            .unwrap(),
        );
        reply(&mut reader, connected);
        assert!(matches!(receive(&mut reader), Request::Snapshot));
        let mut invalid_layout = SceneLayoutRevision::OrbitalV1.definition();
        invalid_layout.schema_version = WORLD_LAYOUT_SCHEMA_VERSION + 1;
        let mut changed = response();
        changed.message = "Unsupported updated realm".into();
        changed.resume_token = Some("untrusted-replacement-layout-token".into());
        changed.snapshot = Some(
            serde_json::from_value(serde_json::json!({
                "server_time_ms": 1, "pilots": [], "orders": [],
                "belt_ore": INITIAL_BELT_ORE, "ledger_sequence": 0,
                "ledger_hash": GENESIS_HASH, "world_layout": invalid_layout,
            }))
            .unwrap(),
        );
        reply(&mut reader, changed);
    });
    let (commands_tx, commands_rx) = mpsc::channel();
    let (updates_tx, updates_rx) = mpsc::channel();
    commands_tx.send(Request::Snapshot).unwrap();
    let client = std::thread::spawn(move || {
        worker(
            address,
            "LayoutResumePilot".into(),
            directory,
            WorkerIdentity::default(),
            commands_rx,
            updates_tx,
            pilot_input::Mailbox::default(),
        )
    });
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Ready
    ));
    match updates_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        Update::Response(response, true) => {
            assert!(response.resume_token.is_none());
            response
                .snapshot
                .as_ref()
                .unwrap()
                .world_layout
                .as_ref()
                .unwrap()
                .validate()
                .unwrap();
        }
        _ => panic!("expected validated initial state"),
    }
    match updates_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
        Update::Failure(message) => assert!(message.to_lowercase().contains("layout"), "{message}"),
        _ => panic!("invalid later snapshot must fail instead of publishing Response"),
    }
    client.join().unwrap();
    endpoint.join().unwrap();
    assert!(matches!(
        updates_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(
        fs::read_to_string(credential).unwrap(),
        "existing-layout-test-credential"
    );
    drop(commands_tx);
}

#[test]
fn ledger_layout_is_validated_and_must_agree_with_the_live_snapshot() {
    let snapshot = |layout: WorldLayoutDefinition| -> Snapshot {
        serde_json::from_value(serde_json::json!({
            "server_time_ms": 0, "pilots": [], "orders": [],
            "belt_ore": INITIAL_BELT_ORE, "ledger_sequence": 0,
            "ledger_hash": GENESIS_HASH, "world_layout": layout,
        }))
        .unwrap()
    };
    let mut invalid_layout = SceneLayoutRevision::OrbitalV1.definition();
    invalid_layout.schema_version = WORLD_LAYOUT_SCHEMA_VERSION + 1;
    let mut received = response();
    received.snapshot = None;
    received.ledger = Some(LedgerExport {
        protocol_version: PROTOCOL_VERSION,
        entries: vec![],
        snapshot: snapshot(invalid_layout),
    });
    assert!(received.snapshot.is_none());
    assert!(validate_world_response(&received)
        .unwrap_err()
        .to_lowercase()
        .contains("layout"));

    received.ledger.as_mut().unwrap().snapshot = snapshot(legacy_world_layout());
    received.snapshot = Some(snapshot(SceneLayoutRevision::OrbitalV1.definition()));
    let error = validate_world_response(&received).unwrap_err();
    assert!(error.contains("different realms"), "{error}");

    received.snapshot = Some(snapshot(legacy_world_layout()));
    validate_world_response(&received).unwrap();
    // A genuinely old snapshot with no descriptor is the same legacy realm.
    received.snapshot.as_mut().unwrap().world_layout = None;
    validate_world_response(&received).unwrap();
}

fn accept_layout_test_peer(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                // Darwin can inherit O_NONBLOCK from the listener. Per-stream
                // read deadlines require normal blocking mode on accepted peers.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                return stream;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() < deadline,
                    "layout test peer did not connect"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("cannot accept layout test peer: {error}"),
        }
    }
}

#[test]
fn missing_snapshot_and_unknown_revision_admissions_never_publish_or_save_credentials() {
    for malformed in ["missing_snapshot", "unknown_revision"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let directory = std::env::temp_dir().join(format!(
            "arkline-malformed-admission-{}-{}",
            std::process::id(),
            listener.local_addr().unwrap().port()
        ));
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(directory.clone());
        assert!(!directory.exists());
        let credential = credentials_path(&directory, &address, "MalformedAdmissionPilot");
        let endpoint = std::thread::spawn(move || {
            let mut reader = BufReader::new(accept_layout_test_peer(&listener));
            assert!(matches!(
                receive(&mut reader),
                Request::Connect {
                    resume_token: None,
                    ..
                }
            ));
            let mut connected = response();
            connected.resume_token = Some("untrusted-malformed-admission-token".into());
            let mut wire = serde_json::to_value(connected).unwrap();
            if malformed == "missing_snapshot" {
                wire["snapshot"] = serde_json::Value::Null;
            } else {
                wire["snapshot"]["world_layout"] =
                    serde_json::to_value(SceneLayoutRevision::OrbitalV1.definition()).unwrap();
                wire["snapshot"]["world_layout"]["revision"] =
                    serde_json::json!("unknown_revision");
            }
            let mut bytes = serde_json::to_vec(&wire).unwrap();
            bytes.push(b'\n');
            reader.get_mut().write_all(&bytes).unwrap();
        });
        let (commands_tx, commands_rx) = mpsc::channel();
        let (updates_tx, updates_rx) = mpsc::channel();
        let client = std::thread::spawn(move || {
            worker(
                address,
                "MalformedAdmissionPilot".into(),
                directory,
                WorkerIdentity::default(),
                commands_rx,
                updates_tx,
                pilot_input::Mailbox::default(),
            )
        });
        match updates_rx.recv_timeout(Duration::from_secs(3)).unwrap() {
            Update::Failure(message) => {
                let expected = if malformed == "missing_snapshot" {
                    "snapshot"
                } else {
                    "unknown_revision"
                };
                assert!(message.to_lowercase().contains(expected), "{message}");
            }
            _ => panic!("malformed admission became Ready or published a Response"),
        }
        client.join().unwrap();
        endpoint.join().unwrap();
        assert!(matches!(
            updates_rx.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        assert!(
            !credential.exists(),
            "{malformed} saved an untrusted credential"
        );
        drop(commands_tx);
    }
}

#[test]
fn unknown_revision_on_resume_stops_after_one_attempt_and_preserves_saved_credential() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let directory = std::env::temp_dir().join(format!(
        "arkline-malformed-layout-resume-{}-{}",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    assert!(!directory.exists());
    let credential = credentials_path(&directory, &address, "MalformedResumePilot");
    let original = b"original-saved-layout-credential";
    save_token(&credential, std::str::from_utf8(original).unwrap()).unwrap();
    let (finished_tx, finished_rx) = mpsc::channel();
    let endpoint = std::thread::spawn(move || {
        let mut reader = BufReader::new(accept_layout_test_peer(&listener));
        match receive(&mut reader) {
            Request::Connect {
                resume_token: Some(token),
                ..
            } => assert_eq!(token.as_bytes(), original),
            _ => panic!("initial connection did not use the existing credential"),
        }
        reply(&mut reader, response());
        assert!(matches!(receive(&mut reader), Request::Snapshot));
        // A transport interruption, rather than invalid data, legitimately
        // starts recovery. Only the subsequent resume response is malformed.
        drop(reader);

        let mut reader = BufReader::new(accept_layout_test_peer(&listener));
        match receive(&mut reader) {
            Request::Connect {
                resume_token: Some(token),
                ..
            } => assert_eq!(token.as_bytes(), original),
            _ => panic!("recovery did not retain the existing credential"),
        }
        let mut resumed = response();
        resumed.resume_token = Some("must-not-replace-existing-credential".into());
        let mut wire = serde_json::to_value(resumed).unwrap();
        wire["snapshot"]["world_layout"] =
            serde_json::to_value(SceneLayoutRevision::OrbitalV1.definition()).unwrap();
        wire["snapshot"]["world_layout"]["revision"] = serde_json::json!("unknown_revision");
        let mut bytes = serde_json::to_vec(&wire).unwrap();
        bytes.push(b'\n');
        reader.get_mut().write_all(&bytes).unwrap();
        drop(reader);

        let mut accepted = 2;
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // Reply to any incorrect retry, so even a regressed worker reaches
            // a bounded failure instead of leaving this test stuck on I/O.
            match listener.accept() {
                Ok((stream, _)) => {
                    accepted += 1;
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    stream
                        .set_write_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut retry = BufReader::new(stream);
                    assert!(matches!(receive(&mut retry), Request::Connect { .. }));
                    retry.get_mut().write_all(&bytes).unwrap();
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(error) => panic!("cannot inspect resume retries: {error}"),
            }
            match finished_rx.recv_timeout(Duration::from_millis(5)) {
                Ok(()) => return accepted,
                Err(mpsc::RecvTimeoutError::Disconnected) => return accepted,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    assert!(
                        Instant::now() < deadline,
                        "worker did not terminate after malformed resume"
                    );
                }
            }
        }
    });
    let (commands_tx, commands_rx) = mpsc::channel();
    let (updates_tx, updates_rx) = mpsc::channel();
    commands_tx.send(Request::Snapshot).unwrap();
    let client = std::thread::spawn(move || {
        worker(
            address,
            "MalformedResumePilot".into(),
            directory,
            WorkerIdentity::default(),
            commands_rx,
            updates_tx,
            pilot_input::Mailbox::default(),
        )
    });
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Ready
    ));
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Response(_, true)
    ));
    assert!(matches!(
        updates_rx.recv_timeout(Duration::from_secs(3)).unwrap(),
        Update::Reconnecting(_)
    ));
    match updates_rx.recv_timeout(Duration::from_secs(4)).unwrap() {
        Update::Failure(message) => {
            assert!(message.contains("unknown_revision"), "{message}");
            assert!(message.contains("Resume rejected"), "{message}");
        }
        _ => panic!("malformed resume became Ready or published a Response"),
    }
    client.join().unwrap();
    finished_tx.send(()).unwrap();
    assert_eq!(
        endpoint.join().unwrap(),
        2,
        "malformed resume triggered another connection"
    );
    assert!(matches!(
        updates_rx.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(fs::read(credential).unwrap(), original);
    drop(commands_tx);
}
