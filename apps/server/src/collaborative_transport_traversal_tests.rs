//! Traversal capability negotiation stays additive on the loopback wire.
use super::*;

#[test]
fn old_wire_join_defaults_traversal_capability_to_false() {
    let old: WireRequest =
        serde_json::from_str(r#"{"type":"join","secret":"viewer","world_layout_version":1}"#)
            .unwrap();
    assert!(matches!(
        old,
        WireRequest::Join {
            supports_docked_traversal_v2: false,
            connect: None,
            ..
        }
    ));
    let capable: WireRequest = serde_json::from_str(
        r#"{"type":"join","secret":"viewer","world_layout_version":1,"supports_docked_traversal_v2":true}"#,
    )
    .unwrap();
    assert!(matches!(
        capable,
        WireRequest::Join {
            supports_docked_traversal_v2: true,
            connect: None,
            ..
        }
    ));
}

#[test]
fn client_connect_wrapper_and_capable_api_send_distinct_traversal_negotiation() {
    for capability in [false, true] {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let server = thread::spawn(move || {
            let deadline = Instant::now() + CLIENT_IO_DEADLINE;
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(peer) => break peer,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "client did not connect before deadline"
                        );
                        thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("negotiation fixture accept failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            let line = read_line_deadline(&mut stream, Instant::now() + CLIENT_IO_DEADLINE)
                .unwrap()
                .expect("client sent its join");
            let request: WireRequest = serde_json::from_slice(&line).unwrap();
            let WireRequest::Join {
                supports_docked_traversal_v2,
                connect,
                ..
            } = request
            else {
                panic!("first client message must join");
            };
            assert!(
                connect.is_none(),
                "a viewer must not acquire a game session"
            );
            let mut response = Response::error("negotiation fixture");
            response.ok = true;
            write_line_deadline(
                &mut stream,
                &WireReply::Joined {
                    role: CollaborationRole::Viewer,
                    response,
                },
                Instant::now() + CLIENT_IO_DEADLINE,
            )
            .unwrap();
            supports_docked_traversal_v2
        });
        let (client, response) = if capability {
            DevelopmentEditorClient::connect_with_capabilities(address, "viewer", None, true)
        } else {
            DevelopmentEditorClient::connect(address, "viewer", None)
        }
        .unwrap();
        assert_eq!(client.role(), CollaborationRole::Viewer);
        assert!(response.ok);
        assert_eq!(server.join().unwrap(), capability);
        drop(client);
    }
}
