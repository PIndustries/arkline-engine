//! Browser lifecycle and nonblocking WebSocket transport. No command replay.
use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{MessageEvent, WebSocket};

pub fn storage() -> Result<web_sys::Storage, String> {
    web_sys::window()
        .ok_or("Browser window unavailable")?
        .local_storage()
        .map_err(|_| "Browser storage is blocked")?
        .ok_or_else(|| "Browser storage unavailable".into())
}
pub fn read(path: &std::path::Path) -> Result<String, String> {
    storage()?
        .get_item(&format!("arkline:{}", path.display()))
        .map_err(|_| "Cannot read browser storage".to_string())?
        .ok_or_else(|| "Not found".into())
}
pub fn write(path: &std::path::Path, value: &str) -> Result<(), String> {
    storage()?
        .set_item(&format!("arkline:{}", path.display()), value)
        .map_err(|_| "Cannot save browser storage; check quota and privacy settings".into())
}
struct Socket {
    socket: WebSocket,
    messages: Rc<RefCell<VecDeque<Result<String, String>>>>,
    _message: Closure<dyn FnMut(MessageEvent)>,
}
impl Drop for Socket {
    fn drop(&mut self) {
        self.socket.set_onmessage(None);
        let _ = self.socket.close();
    }
}
impl Socket {
    async fn connect(server: &str) -> Result<Self, String> {
        if !server.starts_with("wss://")
            && !server.starts_with("ws://localhost:")
            && !server.starts_with("ws://127.0.0.1:")
        {
            return Err(
                "Browser servers require wss:// (local development may use ws://localhost:PORT/ws)"
                    .into(),
            );
        }
        let socket = WebSocket::new(server).map_err(|_| "Invalid WebSocket server address")?;
        let messages = Rc::new(RefCell::new(VecDeque::new()));
        let queue = messages.clone();
        let callback = Closure::wrap(Box::new(move |event: MessageEvent| {
            let mut queue = queue.borrow_mut();
            if queue.len() >= 4 {
                return;
            }
            queue.push_back(
                event
                    .data()
                    .as_string()
                    .ok_or_else(|| "Unsupported binary response".into())
                    .and_then(|text| {
                        if text.len() > 8 * 1024 * 1024 {
                            Err("Response exceeds 8 MiB limit".into())
                        } else {
                            Ok(text)
                        }
                    }),
            );
        }) as Box<dyn FnMut(MessageEvent)>);
        socket.set_onmessage(Some(callback.as_ref().unchecked_ref()));
        let result = Self {
            socket,
            messages,
            _message: callback,
        };
        let started = Instant::now();
        while result.socket.ready_state() == WebSocket::CONNECTING {
            if started.elapsed() > Duration::from_secs(10) {
                return Err("Connection timed out".into());
            }
            gloo_timers::future::TimeoutFuture::new(20).await;
        }
        if result.socket.ready_state() != WebSocket::OPEN {
            return Err("Connection failed. Check the server address, certificate and allowed browser origin.".into());
        }
        Ok(result)
    }
    async fn exchange(&self, request: &Request) -> Result<Response, String> {
        let text = serde_json::to_string(request).map_err(|e| e.to_string())?;
        if text.len() > 16384 {
            return Err("Request exceeds 16 KiB limit".into());
        }
        self.socket
            .send_with_str(&text)
            .map_err(|_| "Connection closed before sending")?;
        let started = Instant::now();
        loop {
            if let Some(message) = self.messages.borrow_mut().pop_front() {
                return serde_json::from_str(&message?)
                    .map_err(|e| format!("Invalid server response: {e}"));
            }
            if self.socket.ready_state() != WebSocket::OPEN {
                return Err("Server connection closed".into());
            }
            if started.elapsed() > Duration::from_secs(5) {
                return Err("Server response timed out".into());
            }
            gloo_timers::future::TimeoutFuture::new(10).await;
        }
    }
}
pub fn worker(
    server: String,
    pilot: String,
    dir: PathBuf,
    scope: Option<String>,
    commands: Receiver<Request>,
    updates: Sender<Update>,
    controls: pilot_input::Mailbox,
) {
    wasm_bindgen_futures::spawn_local(async move {
        let result = async {
            // Verify writable browser storage before asking the server to create an identity.
            let path = credentials_path(&dir, scope.as_deref().unwrap_or(&server), &pilot);
            write(&dir.join("storage-check"), "ok")?;
            let token = storage()?.get_item(&format!("arkline:{}", path.display())).map_err(|_| "Cannot read resume credential")?;
            let socket = Socket::connect(&server).await?;
            let mut response = socket.exchange(&Request::Connect { supports_pioneer_interior: true, supports_docked_traversal_v2: true, supports_fleet_hangar: true, supports_room_controls: true, world_layout_version: WORLD_LAYOUT_SCHEMA_VERSION, protocol_version: PROTOCOL_VERSION, pilot, resume_token: token }).await?;
            if !response.ok { return Err(response.message); }
            validate_world_admission_response(&response)?;
            if let Some(token) = response.resume_token.take() { write(&path, &token)?; }
            updates.send(Update::Ready).map_err(|e| e.to_string())?;
            updates.send(Update::Response(Box::new(response), true)).map_err(|e| e.to_string())?;
            let mut last_snapshot = Instant::now();
            loop {
                let request = match commands.try_recv() {
                    Ok(request) => Some((request, true)),
                    Err(mpsc::TryRecvError::Disconnected) => return Ok::<(),String>(()),
                    Err(mpsc::TryRecvError::Empty) => controls.take().map(|r| (r,false)).or_else(|| (last_snapshot.elapsed() >= Duration::from_millis(350)).then_some((Request::Snapshot,false))),
                };
                if let Some((request, visible)) = request {
                    if matches!(request, Request::BeginRemoteControl{..}|Request::EndRemoteControl) { controls.clear(); }
                    let response = socket.exchange(&request).await.map_err(|e| {
                        controls.clear();
                        if visible { format!("{e}. Action outcome unknown; reconnect and inspect state before retrying. The action was not replayed.") } else { format!("{e}. Reconnect to refresh authoritative state.") }
                    })?;
                    validate_world_response(&response)?;
                    last_snapshot = Instant::now();
                    updates.send(Update::Response(Box::new(response),visible)).map_err(|e| e.to_string())?;
                }
                gloo_timers::future::TimeoutFuture::new(50).await;
            }
        }.await;
        if let Err(error) = result {
            let _ = updates.send(Update::Failure(error));
        }
    });
}
pub fn start() {
    wasm_bindgen_futures::spawn_local(async {
        let window = web_sys::window().expect("window");
        let canvas = window
            .document()
            .and_then(|d| d.get_element_by_id("arkline_canvas"))
            .expect("Arkline canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("canvas element");
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions {
                    depth_buffer: 24,
                    ..Default::default()
                },
                Box::new(|cc| {
                    let test = web_sys::window()
                        .and_then(|w| w.location().search().ok())
                        .is_some_and(|q| q.split(['?', '&']).any(|part| part == "network=test"));
                    Ok(Box::new(Cockpit::new(
                        cc,
                        if test {
                            transport::TEST_SERVER
                        } else {
                            transport::OFFICIAL_SERVER
                        }
                        .into(),
                        PathBuf::from("browser"),
                        None,
                    )))
                }),
            )
            .await;
        if let Err(error) = result {
            if let Some(element) = window
                .document()
                .and_then(|d| d.get_element_by_id("loading"))
            {
                element.set_text_content(Some(&format!("Arkline could not start: {error:?}")));
            }
        } else if let Some(element) = window
            .document()
            .and_then(|d| d.get_element_by_id("loading"))
        {
            element.remove();
        }
    });
}
