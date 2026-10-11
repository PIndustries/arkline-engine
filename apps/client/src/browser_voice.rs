#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum VoiceEvent {
    Ready,
    Partial(String),
    Final(String),
    Error(String),
}
#[derive(Default)]
pub struct Voice {
    _unavailable: (),
}
impl Voice {
    pub fn supported() -> bool {
        false
    }
    pub fn is_listening(&self) -> bool {
        false
    }
    pub fn is_speaking(&self) -> bool {
        false
    }
    pub fn status(&self) -> &str {
        "Speech requires the native macOS client."
    }
    pub fn start_listening(&mut self) {}
    pub fn finish_listening(&mut self) {}
    pub fn cancel(&mut self) {}
    pub fn stop_speaking(&mut self) {}
    pub fn speak(&mut self, _text: &str) {}
    pub fn poll(&mut self) -> Option<VoiceEvent> {
        None
    }
}
