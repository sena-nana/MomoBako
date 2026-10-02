//! Native browser content owned by the Scene host; application chrome stays in Runtime.
use nana_ui_runtime::StableNodeId;

pub use nana_window::{BrowserCommand, BrowserEvent, BrowserPolicy, BrowserRect, BrowserState};

#[derive(Debug, Clone)]
pub struct NativeBrowserRequest {
    pub id: String,
    pub node: StableNodeId,
    pub policy: BrowserPolicy,
    /// Latest observed page to restore when a node or native view is recreated.
    /// New native instances never replay Back, Stop, Focus, or Capture.
    pub restore_url: String,
    pub visible: bool,
    /// Monotonic command identity. Retained projection must not replay history operations.
    pub revision: u64,
    pub command: Option<BrowserCommand>,
}

#[derive(Debug, Clone)]
pub struct NativeBrowserEvent {
    pub id: String,
    pub node: StableNodeId,
    pub revision: u64,
    pub event: BrowserEvent,
}
