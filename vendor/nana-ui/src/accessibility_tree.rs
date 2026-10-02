//! Public AccessKit projection for hosts that own their platform adapter
//! wiring (e.g. Android via `accesskit_android`): converts Runtime's
//! backend-neutral accessibility nodes into AccessKit `TreeUpdate`s with the
//! same projector desktop hosts use. macOS/Windows/Unix hosts should prefer
//! the bundled `hosted` adapters.

use accesskit::{ActionRequest, TreeUpdate};
use nana_ui_runtime::{AccessibilityDelta, AccessibilityNode};

use crate::accessibility::AccessibilityProjector;

/// Stateful Runtime → AccessKit tree projector.
pub struct AccessTreeProjector(AccessibilityProjector);

impl AccessTreeProjector {
    /// Retain `nodes`; call `full_update` when the adapter needs its initial tree.
    pub fn new(nodes: Vec<AccessibilityNode>, interactive: bool, scale_factor: f32) -> Self {
        Self(AccessibilityProjector::retain(
            nodes,
            interactive,
            scale_factor,
            None,
            false,
        ))
    }

    /// Replace the cached tree with `nodes` and produce a full update.
    pub fn synchronize_full(
        &mut self,
        nodes: Vec<AccessibilityNode>,
        scale_factor: f32,
    ) -> Option<TreeUpdate> {
        self.0.synchronize_full(nodes, scale_factor, None)
    }

    /// Apply one incremental transaction; `None` when it is stale.
    pub fn apply_delta(&mut self, delta: AccessibilityDelta) -> Option<TreeUpdate> {
        self.0.apply_delta(delta)
    }

    /// Rebuild the full tree update from the cached nodes.
    pub fn full_update(&self) -> TreeUpdate {
        self.0.full_update()
    }

    /// Translate a platform AccessKit action into Nana's backend-neutral
    /// request using the same capability and text-selection validation as the
    /// desktop adapter. Hosts that own their AccessKit integration (Android,
    /// embedded shells) can enqueue the returned request for Runtime.
    pub fn project_action(
        &self,
        request: ActionRequest,
    ) -> Option<nana_ui_runtime::AccessibilityActionRequest> {
        self.0.project_action_request(request)
    }
}
