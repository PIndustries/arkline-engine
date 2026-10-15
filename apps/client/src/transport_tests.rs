use super::*;
use std::net::TcpListener;
fn response() -> Response {
    Response {
        auth: None,
        assets: None,
        ok: true,
        message: "Connected".into(),
        resume_token: None,
        snapshot: None,
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
