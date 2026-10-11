//! Identical game messages over local TCP or authenticated TLS WebSockets.
pub const OFFICIAL_SERVER: &str = "wss://relay.arkline.network/ws";
pub const TEST_SERVER: &str = "wss://test-relay.arkline.network/ws";

#[cfg(not(target_arch = "wasm32"))]
pub use native::Connection;
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::super::*;
    use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};
    pub enum Connection {
        Tcp(BufReader<TcpStream>),
        WebSocket(Box<WebSocket<MaybeTlsStream<TcpStream>>>),
    }
    fn tls_connector() -> Result<tungstenite::Connector, String> {
        // Do not depend on another workspace binary to select a crypto provider.
        // Standalone desktop builds must have a complete, explicit TLS setup.
        let roots = rustls::RootCertStore {
            roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
        };
        let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| e.to_string())?
        .with_root_certificates(roots)
        .with_no_client_auth();
        Ok(tungstenite::Connector::Rustls(std::sync::Arc::new(config)))
    }
    impl Connection {
        pub fn connect(server: &str) -> Result<Self, String> {
            if server.starts_with("wss://") || server.starts_with("ws://") {
                let config = tungstenite::protocol::WebSocketConfig::default()
                    .max_message_size(Some(8 * 1024 * 1024))
                    .max_frame_size(Some(8 * 1024 * 1024));
                use tungstenite::client::IntoClientRequest;
                let mut request = server.into_client_request().map_err(|e| e.to_string())?;
                request.headers_mut().insert(
                    "User-Agent",
                    format!("Arkline/{}", arkline_protocol::BUILD_VERSION.trim())
                        .parse()
                        .map_err(|e| format!("Invalid client version: {e}"))?,
                );
                let host = request.uri().host().ok_or("WebSocket URL needs a host")?;
                let port = request
                    .uri()
                    .port_u16()
                    .unwrap_or(if server.starts_with("wss://") {
                        443
                    } else {
                        80
                    });
                let addresses = (host, port)
                    .to_socket_addrs()
                    .map_err(|e| format!("Cannot resolve server: {e}"))?;
                let mut connected = None;
                for address in addresses {
                    if let Ok(stream) = TcpStream::connect_timeout(&address, Duration::from_secs(3))
                    {
                        connected = Some(stream);
                        break;
                    }
                }
                let stream = connected.ok_or("Cannot connect to WebSocket server")?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_write_timeout(Some(Duration::from_secs(5)))
                    .map_err(|e| e.to_string())?;
                stream.set_nodelay(true).map_err(|e| e.to_string())?;
                let (ws, _) = tungstenite::client_tls_with_config(
                    request,
                    stream,
                    Some(config),
                    Some(tls_connector()?),
                )
                .map_err(|e| format!("WebSocket handshake failed: {e}"))?;
                Ok(Self::WebSocket(Box::new(ws)))
            } else {
                super::super::connect_transport(server).map(Self::Tcp)
            }
        }
        pub fn exchange(&mut self, request: &Request) -> Result<Response, String> {
            match self {
                Self::Tcp(reader) => super::super::exchange(reader, request),
                Self::WebSocket(ws) => {
                    let json = serde_json::to_string(request).map_err(|e| e.to_string())?;
                    if json.len() > 16 * 1024 {
                        return Err("Request exceeds 16 KiB limit".into());
                    }
                    ws.send(Message::Text(json.into()))
                        .map_err(|e| e.to_string())?;
                    let deadline = Instant::now() + Duration::from_secs(5);
                    loop {
                        if Instant::now() >= deadline {
                            return Err("WebSocket response deadline exceeded".into());
                        }
                        socket(ws.get_mut())?
                            .set_read_timeout(Some(
                                deadline
                                    .saturating_duration_since(Instant::now())
                                    .max(Duration::from_millis(1)),
                            ))
                            .map_err(|e| e.to_string())?;
                        match ws.read().map_err(|e| e.to_string())? {
                            Message::Text(text) => {
                                return serde_json::from_str(&text)
                                    .map_err(|e| format!("Invalid server reply: {e}"))
                            }
                            Message::Close(_) => return Err("Server closed the connection".into()),
                            Message::Binary(_) => {
                                return Err("Server sent unsupported binary data".into())
                            }
                            Message::Ping(_) => {
                                ws.flush().map_err(|e| e.to_string())?;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        pub fn close(&mut self) {
            match self {
                Self::Tcp(reader) => {
                    let _ = reader.get_mut().shutdown(std::net::Shutdown::Both);
                }
                Self::WebSocket(ws) => {
                    let _ = ws.close(None);
                }
            }
        }
    }
    fn socket(stream: &mut MaybeTlsStream<TcpStream>) -> Result<&mut TcpStream, String> {
        match stream {
            MaybeTlsStream::Plain(s) => Ok(s),
            MaybeTlsStream::Rustls(s) => Ok(&mut s.sock),
            _ => Err("Unsupported TLS backend".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use arkline_protocol::{Request, Response, PROTOCOL_VERSION};
    use std::{net::TcpListener, thread};
    #[test]
    #[ignore = "requires the official public relays; read-only snapshot checks"]
    fn native_verified_wss_reaches_both_official_relays() {
        for endpoint in [OFFICIAL_SERVER, TEST_SERVER] {
            let mut client = Connection::connect(endpoint).expect("verified native WSS handshake");
            let response = client
                .exchange(&Request::Snapshot)
                .expect("real snapshot response");
            assert!(response.ok);
            assert!(response.snapshot.is_some());
            client.close();
        }
    }
    #[test]
    fn websocket_round_trip_preserves_json_and_authentication_boundary() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("ws://{}/ws", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut websocket = tungstenite::accept(stream).unwrap();
            for expected_connect in [true, false] {
                let message = websocket.read().unwrap();
                assert!(message.is_text());
                let request: Request = serde_json::from_str(message.to_text().unwrap()).unwrap();
                if expected_connect {
                    assert!(
                        matches!(request, Request::Connect { protocol_version: PROTOCOL_VERSION, ref pilot, resume_token: None } if pilot == "web-pilot")
                    );
                } else {
                    assert!(matches!(request, Request::Snapshot));
                }
                let response = serde_json::json!({"ok":true,"message":"real test response","snapshot":null,"resume_token":null,"ledger":null,"assets":null});
                websocket
                    .send(tungstenite::Message::Text(response.to_string().into()))
                    .unwrap();
            }
        });
        let mut client = Connection::connect(&endpoint).unwrap();
        let response: Response = client
            .exchange(&Request::Connect {
                protocol_version: PROTOCOL_VERSION,
                pilot: "web-pilot".into(),
                resume_token: None,
            })
            .unwrap();
        assert!(response.ok);
        assert_eq!(
            client.exchange(&Request::Snapshot).unwrap().message,
            "real test response"
        );
        server.join().unwrap();
    }
    #[test]
    fn websocket_binary_reply_is_rejected_without_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = format!("ws://{}/ws", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let mut socket = tungstenite::accept(listener.accept().unwrap().0).unwrap();
            socket.read().unwrap();
            socket
                .send(tungstenite::Message::Binary(vec![1, 2, 3].into()))
                .unwrap();
        });
        let mut client = Connection::connect(&endpoint).unwrap();
        assert!(client
            .exchange(&Request::Snapshot)
            .unwrap_err()
            .contains("binary"));
        server.join().unwrap();
    }
}
