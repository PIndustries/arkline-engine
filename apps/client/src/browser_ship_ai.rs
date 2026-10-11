use arkline_protocol::{NavigationTarget, PilotView, Snapshot};
#[allow(dead_code)]
#[derive(Debug, PartialEq)]
pub enum AiIntent {
    GatherOre { target_units: u64 },
    CancelResourceJob,
    ReportStatus,
    OrbitDrone { id: u64, target: NavigationTarget },
    HoldDrone { id: u64 },
}
#[derive(Debug)]
pub struct AiReply {
    pub intent: Option<AiIntent>,
    pub message: String,
}

#[derive(Default)]
pub struct ShipAi {
    history: Vec<(String, String)>,
    reply: Option<Result<AiReply, String>>,
}
impl ShipAi {
    pub fn conversation(&self) -> &[(String, String)] {
        &self.history
    }
    pub fn endpoint_label(&self) -> &str {
        "Native SSH model adapter unavailable in browser"
    }
    pub fn status(&self) -> &str {
        "Use deterministic resource jobs below; typed model orders require the native client."
    }
    pub fn clear_context(&mut self) {
        self.cancel_pending();
        self.history.clear();
    }
    pub fn cancel_pending(&mut self) {
        self.reply = None;
    }
    pub fn request_with_context(
        &mut self,
        _prompt: &str,
        _pilot: &PilotView,
        _snapshot: &Snapshot,
        _selected: Option<NavigationTarget>,
    ) {
        self.reply = Some(Err(self.status().into()));
    }
    pub fn poll(&mut self) -> Option<Result<AiReply, String>> {
        self.reply.take()
    }
}
