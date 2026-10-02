use std::collections::{BTreeMap, BTreeSet};

#[cfg(all(feature = "hosted", not(target_os = "android")))]
use std::collections::VecDeque;

use accesskit::ActionData;
use accesskit::{
    Action, Invalid, Node, NodeId, Orientation, Rect, Role, TextDirection, TextPosition,
    TextSelection as AccessKitTextSelection, Toggled, TreeId, TreeInfo, TreeUpdate,
};
#[cfg(all(feature = "hosted", not(target_os = "android")))]
use nana_ui_runtime::AccessibilityUpdate;
use nana_ui_runtime::{
    AccessibilityDelta, AccessibilityNode, AccessibilityRole, SelectionOrientation, StableNodeId,
};

use accesskit::ActionRequest;
#[cfg(all(
    feature = "hosted",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
use accesskit::DeactivationHandler;
#[cfg(all(feature = "hosted", not(target_os = "android")))]
use accesskit::{ActionHandler, ActivationHandler};
#[cfg(all(feature = "hosted", not(target_os = "android")))]
use std::sync::{Arc, Mutex};

/// Stateful conversion from NanaUI's backend-neutral semantics to AccessKit.
///
/// The retained cache is necessary because AccessKit removals are expressed by
/// replacing a parent's child list, while NanaUI deltas also carry explicit
/// tombstones. The cache lets one runtime transaction become one coherent
/// platform update without leaking AccessKit into `nana-ui-runtime`.
pub(crate) struct AccessibilityProjector {
    nodes: BTreeMap<StableNodeId, AccessibilityNode>,
    text_runs: BTreeMap<StableNodeId, NodeId>,
    next_text_run_id: u64,
    roots: Vec<StableNodeId>,
    focused: BTreeSet<StableNodeId>,
    interactive: bool,
    scale_factor: f32,
    generation: Option<u64>,
    window_root: bool,
    #[cfg(test)]
    full_updates: std::cell::Cell<usize>,
    #[cfg(test)]
    global_reconciliations: std::cell::Cell<usize>,
}

impl AccessibilityProjector {
    #[cfg(test)]
    fn new(
        nodes: Vec<AccessibilityNode>,
        interactive: bool,
        scale_factor: f32,
    ) -> (Self, TreeUpdate) {
        Self::new_at_generation(nodes, interactive, scale_factor, None)
    }

    #[cfg(test)]
    fn new_at_generation(
        nodes: Vec<AccessibilityNode>,
        interactive: bool,
        scale_factor: f32,
        generation: Option<u64>,
    ) -> (Self, TreeUpdate) {
        let projector = Self::retain(nodes, interactive, scale_factor, generation, false);
        let update = projector.full_update();
        (projector, update)
    }

    pub(crate) fn retain(
        nodes: Vec<AccessibilityNode>,
        interactive: bool,
        scale_factor: f32,
        generation: Option<u64>,
        window_root: bool,
    ) -> Self {
        let nodes = nodes.into_iter().map(|node| (node.id, node)).collect();
        let roots = runtime_roots(&nodes);
        let focused = focused_nodes(&nodes);
        let mut projector = Self {
            nodes,
            text_runs: BTreeMap::new(),
            next_text_run_id: u64::MAX,
            roots,
            focused,
            interactive,
            scale_factor: scale_factor.max(0.01),
            generation,
            window_root,
            #[cfg(test)]
            full_updates: std::cell::Cell::new(0),
            #[cfg(test)]
            global_reconciliations: std::cell::Cell::new(0),
        };
        projector.reconcile_text_runs();
        projector
    }

    pub(crate) fn apply_delta(&mut self, delta: AccessibilityDelta) -> Option<TreeUpdate> {
        if self
            .generation
            .is_some_and(|generation| delta.generation <= generation)
        {
            return None;
        }
        self.generation = Some(delta.generation);
        Some(self.apply(delta))
    }

    pub(crate) fn synchronize_full(
        &mut self,
        nodes: Vec<AccessibilityNode>,
        scale_factor: f32,
        generation: Option<u64>,
    ) -> Option<TreeUpdate> {
        let scale_factor = scale_factor.max(0.01);
        if generation.is_some_and(|next| self.generation.is_some_and(|current| next <= current)) {
            if (scale_factor - self.scale_factor).abs() > f32::EPSILON {
                self.scale_factor = scale_factor;
                return Some(self.full_update());
            }
            return None;
        }
        if generation.is_some() {
            self.generation = generation;
        }
        self.synchronize(nodes, scale_factor)
    }

    pub(crate) fn apply(&mut self, delta: AccessibilityDelta) -> TreeUpdate {
        let incoming = delta
            .updated
            .into_iter()
            .map(|node| (node.id, node))
            .collect::<BTreeMap<_, _>>();
        let removed = delta
            .removed
            .into_iter()
            .filter(|id| !incoming.contains_key(id))
            .collect::<BTreeSet<_>>();
        let structure_changed = !removed.is_empty()
            || incoming.values().any(|node| {
                self.nodes.get(&node.id).is_none_or(|previous| {
                    previous.parent != node.parent || previous.children != node.children
                })
            });
        let text_roles_changed = structure_changed
            || incoming.values().any(|node| {
                self.nodes.get(&node.id).is_none_or(|previous| {
                    (previous.role == AccessibilityRole::TextInput)
                        != (node.role == AccessibilityRole::TextInput)
                })
            });
        let previous_text_runs = text_roles_changed.then(|| self.text_runs.clone());
        let mut changed = incoming.keys().copied().collect::<BTreeSet<_>>();
        changed.extend(
            removed
                .iter()
                .filter_map(|id| self.nodes.get(id).and_then(|node| node.parent)),
        );

        for id in &removed {
            if let Some(parent_id) = self.nodes.get(id).and_then(|node| node.parent)
                && let Some(parent) = self.nodes.get_mut(&parent_id)
            {
                parent.children.retain(|child| child != id);
            }
            self.nodes.remove(id);
            self.focused.remove(id);
            changed.remove(id);
        }

        for node in incoming.into_values() {
            let old_parent = self.nodes.get(&node.id).and_then(|previous| {
                (previous.parent != node.parent)
                    .then_some(previous.parent)
                    .flatten()
            });
            if let Some(old_parent) = old_parent {
                if let Some(parent) = self.nodes.get_mut(&old_parent) {
                    parent.children.retain(|child| *child != node.id);
                }
                changed.insert(old_parent);
            }
            if structure_changed
                && let Some(parent_id) = node.parent
                && let Some(parent) = self.nodes.get_mut(&parent_id)
                && !parent.children.contains(&node.id)
            {
                parent.children.push(node.id);
                changed.insert(parent_id);
            }
            if node.focused {
                self.focused.insert(node.id);
            } else {
                self.focused.remove(&node.id);
            }
            self.nodes.insert(node.id, node);
        }

        if structure_changed {
            self.drop_unreachable(&mut changed);
        }
        if text_roles_changed {
            self.reconcile_text_runs();
        }
        if structure_changed {
            let roots = runtime_roots(&self.nodes);
            if roots != self.roots {
                self.roots = roots;
                return self.full_update();
            }
        }
        if let Some(previous_text_runs) = previous_text_runs {
            changed.extend(
                previous_text_runs
                    .iter()
                    .chain(self.text_runs.iter())
                    .filter_map(|(id, _)| {
                        (previous_text_runs.get(id) != self.text_runs.get(id)).then_some(*id)
                    }),
            );
        }
        changed.retain(|id| self.nodes.contains_key(id));

        // Updating a parent removes stale AccessKit subtrees. Do not also ship
        // unreachable descendants: AccessKit would keep their children with a
        // dangling parent_and_index.
        TreeUpdate {
            nodes: changed
                .into_iter()
                .filter_map(|id| self.nodes.get(&id))
                .flat_map(|node| self.project_entries(node))
                .collect(),
            tree: None,
            tree_id: TreeId::ROOT,
            focus: self.focused_node_id(),
        }
    }

    fn drop_unreachable(&mut self, changed: &mut BTreeSet<StableNodeId>) {
        #[cfg(test)]
        self.global_reconciliations
            .set(self.global_reconciliations.get() + 1);
        let mut keep = BTreeSet::new();
        let mut stack = runtime_roots(&self.nodes);
        while let Some(id) = stack.pop() {
            if !keep.insert(id) {
                continue;
            }
            if let Some(node) = self.nodes.get(&id) {
                stack.extend(
                    node.children
                        .iter()
                        .copied()
                        .filter(|child| self.nodes.contains_key(child)),
                );
            }
        }
        if keep.len() != self.nodes.len() {
            self.nodes.retain(|id, _| keep.contains(id));
            self.focused.retain(|id| keep.contains(id));
            changed.retain(|id| keep.contains(id));
        }
        for node in self.nodes.values_mut() {
            let before = node.children.len();
            node.children.retain(|child| keep.contains(child));
            if node.children.len() != before {
                changed.insert(node.id);
            }
        }
    }

    fn synchronize(
        &mut self,
        nodes: Vec<AccessibilityNode>,
        scale_factor: f32,
    ) -> Option<TreeUpdate> {
        let next = nodes
            .into_iter()
            .map(|node| (node.id, node))
            .collect::<BTreeMap<_, _>>();
        let roots = runtime_roots(&next);
        let scale_factor = scale_factor.max(0.01);
        if (scale_factor - self.scale_factor).abs() > f32::EPSILON || roots != self.roots {
            self.nodes = next;
            self.focused = focused_nodes(&self.nodes);
            self.scale_factor = scale_factor;
            self.roots = roots;
            self.reconcile_text_runs();
            return Some(self.full_update());
        }
        let updated = next
            .values()
            .filter(|node| self.nodes.get(&node.id) != Some(*node))
            .cloned()
            .collect::<Vec<_>>();
        let removed = self
            .nodes
            .keys()
            .filter(|id| !next.contains_key(id))
            .copied()
            .collect::<Vec<_>>();
        if updated.is_empty() && removed.is_empty() {
            return None;
        }
        Some(self.apply(AccessibilityDelta {
            generation: 0,
            updated,
            removed,
        }))
    }

    pub(crate) fn full_update(&self) -> TreeUpdate {
        #[cfg(test)]
        self.full_updates.set(self.full_updates.get() + 1);
        let mut nodes = self
            .nodes
            .values()
            .flat_map(|node| self.project_entries(node))
            .collect::<Vec<_>>();
        if self.window_root || self.roots.len() != 1 {
            let mut root = Node::new(if self.window_root {
                Role::Window
            } else {
                Role::GenericContainer
            });
            root.set_children(self.roots.iter().copied().map(node_id).collect::<Vec<_>>());
            nodes.push((FOREST_ROOT_ID, root));
        }
        TreeUpdate {
            nodes,
            tree: Some(TreeInfo::new(self.tree_root_id())),
            tree_id: TreeId::ROOT,
            focus: self.focused_node_id(),
        }
    }

    fn tree_root_id(&self) -> NodeId {
        if !self.window_root && self.roots.len() == 1 {
            node_id(self.roots[0])
        } else {
            FOREST_ROOT_ID
        }
    }

    fn focused_node_id(&self) -> NodeId {
        self.focused
            .first()
            .copied()
            .map(node_id)
            .unwrap_or_else(|| {
                if self.window_root {
                    FOREST_ROOT_ID
                } else {
                    self.roots.first().copied().map_or(FOREST_ROOT_ID, node_id)
                }
            })
    }

    fn reconcile_text_runs(&mut self) {
        #[cfg(test)]
        self.global_reconciliations
            .set(self.global_reconciliations.get() + 1);
        let occupied = self
            .nodes
            .keys()
            .map(|id| NodeId(id.get()))
            .collect::<BTreeSet<_>>();
        self.text_runs.retain(|id, text_run| {
            self.nodes
                .get(id)
                .is_some_and(|node| node.role == AccessibilityRole::TextInput)
                && !occupied.contains(text_run)
        });
        let mut used = occupied;
        used.extend(self.text_runs.values().copied());
        let inputs = self
            .nodes
            .values()
            .filter(|node| node.role == AccessibilityRole::TextInput)
            .map(|node| node.id)
            .collect::<Vec<_>>();
        for id in inputs {
            if self.text_runs.contains_key(&id) {
                continue;
            }
            while self.next_text_run_id == 0 || used.contains(&NodeId(self.next_text_run_id)) {
                self.next_text_run_id = self.next_text_run_id.wrapping_sub(1);
            }
            let text_run = NodeId(self.next_text_run_id);
            self.next_text_run_id = self.next_text_run_id.wrapping_sub(1);
            used.insert(text_run);
            self.text_runs.insert(id, text_run);
        }
    }

    fn project_entries(&self, node: &AccessibilityNode) -> Vec<(NodeId, Node)> {
        project_node(
            node,
            self.text_runs.get(&node.id).copied(),
            self.interactive,
            self.scale_factor,
        )
    }

    pub(crate) fn project_action_request(
        &self,
        request: ActionRequest,
    ) -> Option<nana_ui_runtime::AccessibilityActionRequest> {
        if request.target_tree != TreeId::ROOT {
            return None;
        }
        let target = StableNodeId::new(request.target_node.0)?;
        let node = self.nodes.get(&target)?;
        if !self.interactive || node.disabled {
            return None;
        }
        let action = match request.action {
            Action::Click if supports_click(node.role) => {
                nana_ui_runtime::AccessibilityAction::Click
            }
            Action::Focus if supports_focus(node.role) => {
                nana_ui_runtime::AccessibilityAction::Focus
            }
            Action::SetValue if supports_set_value(node) => match request.data {
                Some(ActionData::Value(value)) => {
                    nana_ui_runtime::AccessibilityAction::SetValue(value.into())
                }
                Some(ActionData::NumericValue(value)) if value.is_finite() => {
                    nana_ui_runtime::AccessibilityAction::SetValue(value.to_string())
                }
                _ => return None,
            },
            Action::SetTextSelection => {
                let Some(ActionData::SetTextSelection(selection)) = request.data else {
                    return None;
                };
                let text_run = self.text_runs.get(&target)?;
                projected_text_selection(node, *text_run)?;
                if selection.anchor.node != *text_run || selection.focus.node != *text_run {
                    return None;
                }
                let value = node.value.as_deref().unwrap_or_default();
                nana_ui_runtime::AccessibilityAction::SetSelection(
                    nana_ui_runtime::TextSelection::new(
                        character_index_to_byte(value, selection.anchor.character_index)?,
                        character_index_to_byte(value, selection.focus.character_index)?,
                    ),
                )
            }
            _ => return None,
        };
        Some(nana_ui_runtime::AccessibilityActionRequest { target, action })
    }
}

const FOREST_ROOT_ID: NodeId = NodeId(0);

fn focused_nodes(nodes: &BTreeMap<StableNodeId, AccessibilityNode>) -> BTreeSet<StableNodeId> {
    nodes
        .values()
        .filter(|node| node.focused)
        .map(|node| node.id)
        .collect()
}

fn runtime_roots(nodes: &BTreeMap<StableNodeId, AccessibilityNode>) -> Vec<StableNodeId> {
    nodes
        .values()
        .filter(|node| node.parent.is_none())
        .map(|node| node.id)
        .collect()
}

#[cfg(all(feature = "hosted", not(target_os = "android")))]
#[derive(Clone)]
struct CurrentTree(Arc<Mutex<AccessibilityProjector>>);

#[cfg(all(feature = "hosted", not(target_os = "android")))]
impl ActivationHandler for CurrentTree {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        Some(self.0.lock().ok()?.full_update())
    }
}

#[cfg(all(feature = "hosted", not(target_os = "android")))]
impl CurrentTree {
    fn unpublished(interactive: bool, scale_factor: f32) -> Self {
        Self(Arc::new(Mutex::new(AccessibilityProjector::retain(
            Vec::new(),
            interactive,
            scale_factor,
            None,
            true,
        ))))
    }

    fn synchronize(&self, update: AccessibilityUpdate, scale_factor: f32) -> Option<TreeUpdate> {
        let mut projector = self.0.lock().expect("accessibility projector");
        match update {
            AccessibilityUpdate::Full { generation, nodes } => {
                projector.synchronize_full(nodes, scale_factor, generation)
            }
            AccessibilityUpdate::Delta(delta) => {
                debug_assert!(
                    (projector.scale_factor - scale_factor.max(0.01)).abs() <= f32::EPSILON
                );
                projector.apply_delta(delta)
            }
        }
    }
}

#[cfg(all(feature = "hosted", not(target_os = "android")))]
struct QueuedActions {
    requests: Arc<Mutex<VecDeque<ActionRequest>>>,
    window: Arc<dyn winit::window::Window>,
}

#[cfg(all(feature = "hosted", not(target_os = "android")))]
const MAX_PENDING_ACCESSIBILITY_ACTIONS: usize = 256;

#[cfg(all(feature = "hosted", not(target_os = "android")))]
fn enqueue_accessibility_action(
    requests: &mut VecDeque<ActionRequest>,
    request: ActionRequest,
) -> bool {
    if requests.len() >= MAX_PENDING_ACCESSIBILITY_ACTIONS {
        return false;
    }
    requests.push_back(request);
    true
}

#[cfg(all(feature = "hosted", not(target_os = "android")))]
impl ActionHandler for QueuedActions {
    fn do_action(&mut self, request: ActionRequest) {
        if let Ok(mut requests) = self.requests.lock()
            && enqueue_accessibility_action(&mut requests, request)
        {
            self.window.request_redraw();
        }
    }
}

#[cfg(all(feature = "hosted", target_os = "windows"))]
fn native_adapter(
    window: &Arc<dyn winit::window::Window>,
    activation: CurrentTree,
    action: QueuedActions,
) -> accesskit_windows::SubclassingAdapter {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window
        .window_handle()
        .expect("AccessKit requires a live window handle");
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        panic!("hosted AccessKit on Windows requires an HWND");
    };
    accesskit_windows::SubclassingAdapter::new(
        accesskit_windows::HWND(handle.hwnd.get() as *mut _),
        activation,
        action,
    )
}

#[cfg(all(feature = "hosted", target_os = "macos"))]
fn native_adapter(
    window: &Arc<dyn winit::window::Window>,
    activation: CurrentTree,
    action: QueuedActions,
) -> accesskit_macos::SubclassingAdapter {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window
        .window_handle()
        .expect("AccessKit requires a live window handle");
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        panic!("hosted AccessKit on macOS requires an NSView");
    };
    unsafe { accesskit_macos::SubclassingAdapter::new(handle.ns_view.as_ptr(), activation, action) }
}

#[cfg(all(
    feature = "hosted",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
struct UnixDeactivation;

#[cfg(all(
    feature = "hosted",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
impl DeactivationHandler for UnixDeactivation {
    fn deactivate_accessibility(&mut self) {}
}

/// Wayland cannot report outer position; X11 can. `surface_position` is relative
/// to the window frame, so inner desktop origin is outer + surface origin.
#[cfg(all(
    feature = "hosted",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
fn unix_root_window_bounds(
    window: &dyn winit::window::Window,
    outer_position: winit::dpi::PhysicalPosition<i32>,
    inner_size: winit::dpi::PhysicalSize<u32>,
) -> (Rect, Rect) {
    let surface_position = window.surface_position();
    let inner_position = winit::dpi::PhysicalPosition::new(
        outer_position.x + surface_position.x,
        outer_position.y + surface_position.y,
    );
    let outer_origin: (f64, f64) = outer_position.cast::<f64>().into();
    let outer_size: (f64, f64) = window.outer_size().cast::<f64>().into();
    let inner_origin: (f64, f64) = inner_position.cast::<f64>().into();
    let inner_size: (f64, f64) = inner_size.cast::<f64>().into();
    (
        Rect::from_origin_size(outer_origin, outer_size),
        Rect::from_origin_size(inner_origin, inner_size),
    )
}

/// AT-SPI attaches over D-Bus; there is no HWND/NSView-style window handle.
#[cfg(all(
    feature = "hosted",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
fn native_adapter(
    _window: &Arc<dyn winit::window::Window>,
    activation: CurrentTree,
    action: QueuedActions,
) -> accesskit_unix::Adapter {
    accesskit_unix::Adapter::new(activation, action, UnixDeactivation)
}

#[cfg(all(
    feature = "hosted",
    not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "android",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))
))]
fn native_adapter(
    _window: &Arc<dyn winit::window::Window>,
    _activation: CurrentTree,
    _action: QueuedActions,
) -> Option<()> {
    None
}

#[cfg(all(feature = "hosted", any(target_os = "windows", target_os = "macos")))]
fn raise_accesskit_update<A>(adapter: &mut A, update: TreeUpdate)
where
    A: AccessKitUpdate,
{
    if let Some(events) = adapter.update_if_active(|| update) {
        events.raise();
    }
}

#[cfg(all(
    feature = "hosted",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
fn raise_accesskit_update(adapter: &mut accesskit_unix::Adapter, update: TreeUpdate) {
    adapter.update_if_active(|| update);
}

#[cfg(all(
    feature = "hosted",
    not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "android",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))
))]
fn raise_accesskit_update(_adapter: &mut Option<()>, _update: TreeUpdate) {}

#[cfg(all(feature = "hosted", any(target_os = "windows", target_os = "macos")))]
trait AccessKitUpdate {
    type Events: RaiseEvents;
    fn update_if_active(
        &mut self,
        update_factory: impl FnOnce() -> TreeUpdate,
    ) -> Option<Self::Events>;
}

#[cfg(all(feature = "hosted", any(target_os = "windows", target_os = "macos")))]
trait RaiseEvents {
    fn raise(self);
}

#[cfg(all(feature = "hosted", target_os = "windows"))]
impl AccessKitUpdate for accesskit_windows::SubclassingAdapter {
    type Events = accesskit_windows::QueuedEvents;
    fn update_if_active(
        &mut self,
        update_factory: impl FnOnce() -> TreeUpdate,
    ) -> Option<Self::Events> {
        accesskit_windows::SubclassingAdapter::update_if_active(self, update_factory)
    }
}

#[cfg(all(feature = "hosted", target_os = "windows"))]
impl RaiseEvents for accesskit_windows::QueuedEvents {
    fn raise(self) {
        accesskit_windows::QueuedEvents::raise(self);
    }
}

#[cfg(all(feature = "hosted", target_os = "macos"))]
impl AccessKitUpdate for accesskit_macos::SubclassingAdapter {
    type Events = accesskit_macos::QueuedEvents;
    fn update_if_active(
        &mut self,
        update_factory: impl FnOnce() -> TreeUpdate,
    ) -> Option<Self::Events> {
        accesskit_macos::SubclassingAdapter::update_if_active(self, update_factory)
    }
}

#[cfg(all(feature = "hosted", target_os = "macos"))]
impl RaiseEvents for accesskit_macos::QueuedEvents {
    fn raise(self) {
        accesskit_macos::QueuedEvents::raise(self);
    }
}

/// Per-window native adapter. Only actions with a backend-neutral hosted path
/// are advertised and accepted.
#[cfg(all(feature = "hosted", not(target_os = "android")))]
pub(crate) struct HostedAccessibility {
    #[cfg(all(feature = "hosted", target_os = "windows"))]
    adapter: accesskit_windows::SubclassingAdapter,
    #[cfg(all(feature = "hosted", target_os = "macos"))]
    adapter: accesskit_macos::SubclassingAdapter,
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    adapter: accesskit_unix::Adapter,
    #[cfg(not(any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    adapter: Option<()>,
    current_tree: CurrentTree,
    requests: Arc<Mutex<VecDeque<ActionRequest>>>,
}

#[cfg(all(feature = "hosted", not(target_os = "android")))]
impl HostedAccessibility {
    pub(crate) fn new(
        window: Arc<dyn winit::window::Window>,
        interactive: bool,
        scale_factor: f32,
    ) -> Self {
        // A native window must keep its own exposed root when focus moves into
        // a child. Generic Runtime roots are otherwise filtered by AccessKit,
        // and mounting/replacing documents must not replace the HWND provider.
        // A prepared document is not necessarily a presented document. Keep
        // activation limited to the native root until the first publication.
        let current_tree = CurrentTree::unpublished(interactive, scale_factor);
        let requests = Arc::new(Mutex::new(VecDeque::new()));
        let actions = QueuedActions {
            requests: Arc::clone(&requests),
            window: Arc::clone(&window),
        };
        let adapter = native_adapter(&window, current_tree.clone(), actions);
        Self {
            adapter,
            current_tree,
            requests,
        }
    }

    pub(crate) fn retained_generation(&self) -> Option<u64> {
        self.current_tree
            .0
            .lock()
            .expect("accessibility projector")
            .generation
    }

    pub(crate) fn process_event(
        &mut self,
        window: &dyn winit::window::Window,
        event: &winit::event::WindowEvent,
    ) {
        #[cfg(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        ))]
        match event {
            winit::event::WindowEvent::Moved(outer_position) => {
                let (outer, inner) =
                    unix_root_window_bounds(window, *outer_position, window.surface_size());
                self.adapter.set_root_window_bounds(outer, inner);
            }
            winit::event::WindowEvent::SurfaceResized(inner_size) => {
                let outer_position = window.outer_position().unwrap_or_default();
                let (outer, inner) = unix_root_window_bounds(window, outer_position, *inner_size);
                self.adapter.set_root_window_bounds(outer, inner);
            }
            winit::event::WindowEvent::Focused(is_focused) => {
                self.adapter.update_window_focus_state(*is_focused);
            }
            _ => {}
        }
        #[cfg(not(any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )))]
        {
            let _ = (window, event);
        }
    }

    pub(crate) fn scale_factor_changed(&self, scale_factor: f32) -> bool {
        (self
            .current_tree
            .0
            .lock()
            .expect("accessibility projector")
            .scale_factor
            - scale_factor.max(0.01))
        .abs()
            > f32::EPSILON
    }

    pub(crate) fn synchronize(&mut self, update: AccessibilityUpdate, scale_factor: f32) {
        // Drop the projector lock before entering native UIA/AccessKit code:
        // raising events can synchronously request a fresh activation tree.
        if let Some(update) = self.current_tree.synchronize(update, scale_factor) {
            raise_accesskit_update(&mut self.adapter, update);
        }
    }

    pub(crate) fn take_actions(&self) -> Vec<nana_ui_runtime::AccessibilityActionRequest> {
        let requests = {
            let Ok(mut requests) = self.requests.lock() else {
                return Vec::new();
            };
            std::mem::take(&mut *requests)
        };
        let projector = self.current_tree.0.lock().expect("accessibility projector");
        requests
            .into_iter()
            .filter_map(|request| projector.project_action_request(request))
            .collect()
    }
}

fn project_node(
    node: &AccessibilityNode,
    text_run_id: Option<NodeId>,
    interactive: bool,
    scale_factor: f32,
) -> Vec<(NodeId, Node)> {
    let mut projected = Node::new(project_role(node.role, node.multiline));
    // AccessKit Label nodes expose their text through value (including the
    // native UIA Name property), unlike controls whose accessible name is label.
    if node.role != AccessibilityRole::Text
        && let Some(label) = &node.label
    {
        projected.set_label(label.to_string());
    }
    let value: Option<&str> = if node.role == AccessibilityRole::Text {
        node.label.as_deref().or(node.value.as_deref())
    } else {
        node.value.as_deref()
    };
    if let Some(value) = value {
        projected.set_value(value.to_string());
    }
    if let Some(description) = &node.description {
        projected.set_description(description.to_string());
    }
    let mut children = node
        .children
        .iter()
        .copied()
        .map(node_id)
        .collect::<Vec<_>>();
    children.extend(text_run_id);
    projected.set_children(children);
    projected.set_bounds(Rect {
        x0: f64::from(node.bounds.x * scale_factor),
        y0: f64::from(node.bounds.y * scale_factor),
        x1: f64::from((node.bounds.x + node.bounds.width) * scale_factor),
        y1: f64::from((node.bounds.y + node.bounds.height) * scale_factor),
    });
    if node.disabled {
        projected.set_disabled();
    }
    if node.modal {
        projected.set_modal();
    }
    if node.busy {
        projected.set_busy();
    }
    if node.invalid {
        projected.set_invalid(Invalid::True);
    }
    if let Some(value) = node.numeric_value {
        projected.set_numeric_value(value);
    }
    if let Some(minimum) = node.numeric_minimum {
        projected.set_min_numeric_value(minimum);
    }
    if let Some(maximum) = node.numeric_maximum {
        projected.set_max_numeric_value(maximum);
    }
    if let Some(step) = node.numeric_step {
        projected.set_numeric_value_step(step);
    }
    if node.role == AccessibilityRole::TextInput && !node.editable {
        projected.set_read_only();
    }
    if node.selected == Some(true) {
        projected.set_selected(true);
    }
    if node.mixed {
        projected.set_toggled(Toggled::Mixed);
    } else if let Some(checked) = node.checked {
        projected.set_toggled(if checked {
            Toggled::True
        } else {
            Toggled::False
        });
    }
    if let Some(orientation) = node.orientation {
        projected.set_orientation(match orientation {
            SelectionOrientation::Horizontal => Orientation::Horizontal,
            SelectionOrientation::Vertical => Orientation::Vertical,
        });
    }
    if interactive && !node.disabled && supports_click(node.role) {
        projected.add_action(Action::Click);
    }
    if interactive && !node.disabled && supports_focus(node.role) {
        projected.add_action(Action::Focus);
    }
    if interactive && !node.disabled && supports_set_value(node) {
        projected.add_action(Action::SetValue);
    }
    if let Some(selection) = text_run_id.and_then(|id| projected_text_selection(node, id)) {
        projected.set_text_selection(selection);
        if interactive && !node.disabled {
            projected.add_action(Action::SetTextSelection);
        }
    }

    let mut entries = vec![(node_id(node.id), projected)];
    if let Some(text_run_id) = text_run_id {
        let value = node.value.as_deref().unwrap_or_default();
        let mut text_run = Node::new(Role::TextRun);
        text_run.set_value(value.to_string());
        text_run.set_text_direction(text_direction(node.writing));
        text_run.set_character_lengths(
            value
                .chars()
                .map(|character| character.len_utf8() as u8)
                .collect::<Vec<_>>(),
        );
        entries.push((text_run_id, text_run));
    }
    entries
}

/// Which way a text run reads, from its inline-start edge: a vertical column
/// reads top to bottom, or bottom to top under `direction: rtl` (#59).
const fn text_direction(writing: nana_ui_core::WritingContext) -> TextDirection {
    match writing.inline_start() {
        nana_ui_core::PhysicalEdge::Left => TextDirection::LeftToRight,
        nana_ui_core::PhysicalEdge::Right => TextDirection::RightToLeft,
        nana_ui_core::PhysicalEdge::Top => TextDirection::TopToBottom,
        nana_ui_core::PhysicalEdge::Bottom => TextDirection::BottomToTop,
    }
}

const fn supports_click(role: AccessibilityRole) -> bool {
    matches!(
        role,
        AccessibilityRole::Button
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Switch
            | AccessibilityRole::TextInput
            | AccessibilityRole::Tab
            | AccessibilityRole::MenuItem
            | AccessibilityRole::Radio
    )
}

const fn supports_focus(role: AccessibilityRole) -> bool {
    matches!(
        role,
        AccessibilityRole::Button
            | AccessibilityRole::Checkbox
            | AccessibilityRole::Switch
            | AccessibilityRole::TextInput
            | AccessibilityRole::Slider
            | AccessibilityRole::ComboBox
            | AccessibilityRole::Tab
            | AccessibilityRole::MenuItem
            | AccessibilityRole::Radio
    )
}

const fn supports_set_value(node: &AccessibilityNode) -> bool {
    (matches!(node.role, AccessibilityRole::TextInput) && node.editable)
        || matches!(node.role, AccessibilityRole::Slider)
}

fn projected_text_selection(
    node: &AccessibilityNode,
    text_run_id: NodeId,
) -> Option<AccessKitTextSelection> {
    let selection = node.selection?;
    let value = node.value.as_deref()?;
    Some(AccessKitTextSelection {
        anchor: TextPosition {
            node: text_run_id,
            character_index: byte_to_character_index(value, selection.anchor)?,
        },
        focus: TextPosition {
            node: text_run_id,
            character_index: byte_to_character_index(value, selection.focus)?,
        },
    })
}

fn byte_to_character_index(value: &str, byte_offset: usize) -> Option<usize> {
    if byte_offset > value.len() || !value.is_char_boundary(byte_offset) {
        return None;
    }
    Some(value[..byte_offset].chars().count())
}

fn character_index_to_byte(value: &str, character_index: usize) -> Option<usize> {
    if character_index == value.chars().count() {
        return Some(value.len());
    }
    value
        .char_indices()
        .nth(character_index)
        .map(|(index, _)| index)
}

const fn node_id(id: StableNodeId) -> NodeId {
    NodeId(id.get())
}

const fn project_role(role: AccessibilityRole, multiline: bool) -> Role {
    match role {
        AccessibilityRole::Document => Role::Document,
        AccessibilityRole::Text => Role::Label,
        AccessibilityRole::Button => Role::Button,
        AccessibilityRole::Checkbox => Role::CheckBox,
        AccessibilityRole::Switch => Role::Switch,
        AccessibilityRole::TextInput if multiline => Role::MultilineTextInput,
        AccessibilityRole::TextInput => Role::TextInput,
        AccessibilityRole::Slider => Role::Slider,
        AccessibilityRole::ComboBox => Role::ComboBox,
        AccessibilityRole::ProgressIndicator => Role::ProgressIndicator,
        AccessibilityRole::List => Role::List,
        AccessibilityRole::ListItem => Role::ListItem,
        AccessibilityRole::Table => Role::Table,
        AccessibilityRole::Row => Role::Row,
        AccessibilityRole::Cell => Role::Cell,
        AccessibilityRole::ColumnHeader => Role::ColumnHeader,
        AccessibilityRole::TabList => Role::TabList,
        AccessibilityRole::Tab => Role::Tab,
        AccessibilityRole::RadioGroup => Role::RadioGroup,
        AccessibilityRole::Radio => Role::RadioButton,
        AccessibilityRole::Separator => Role::Splitter,
        AccessibilityRole::Dialog => Role::Dialog,
        AccessibilityRole::AlertDialog => Role::AlertDialog,
        AccessibilityRole::Menu => Role::Menu,
        AccessibilityRole::MenuItem => Role::MenuItem,
        AccessibilityRole::Tooltip => Role::Tooltip,
        AccessibilityRole::Status => Role::Status,
        AccessibilityRole::Toolbar => Role::Toolbar,
        AccessibilityRole::Image => Role::Image,
        AccessibilityRole::Main => Role::Main,
        AccessibilityRole::Navigation => Role::Navigation,
        AccessibilityRole::Banner => Role::Banner,
        AccessibilityRole::ContentInfo => Role::ContentInfo,
        AccessibilityRole::Complementary => Role::Complementary,
        AccessibilityRole::Region => Role::Region,
        AccessibilityRole::Search => Role::Search,
        AccessibilityRole::Form => Role::Form,
        AccessibilityRole::Generic => Role::GenericContainer,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui_runtime::LayoutBox;

    #[test]
    fn static_text_exports_its_content_as_accesskit_value_after_updates() {
        let mut text = node(1, None, &[]);
        text.role = AccessibilityRole::Text;
        text.label = Some("Initial text".into());
        let (mut projector, initial) = AccessibilityProjector::new(vec![text.clone()], true, 1.0);
        assert_eq!(initial.nodes[0].1.role(), Role::Label);
        assert_eq!(initial.nodes[0].1.value(), Some("Initial text"));
        text.label = Some("Updated text".into());
        let update = projector
            .apply_delta(AccessibilityDelta {
                generation: 1,
                updated: vec![text.clone()],
                removed: vec![],
            })
            .unwrap();
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.nodes[0].1.value(), Some("Updated text"));
        text.label = None;
        text.value = Some("Value-only text".into());
        let update = projector
            .apply_delta(AccessibilityDelta {
                generation: 2,
                updated: vec![text],
                removed: vec![],
            })
            .unwrap();
        assert_eq!(update.nodes[0].1.value(), Some("Value-only text"));
    }

    #[cfg(all(feature = "hosted", not(target_os = "android")))]
    #[test]
    fn unpublished_window_exposes_only_its_root_until_the_first_publication() {
        let mut current = CurrentTree::unpublished(true, 1.0);
        let initial = current.request_initial_tree().unwrap();
        assert_eq!(initial.nodes.len(), 1);
        assert_eq!(initial.nodes[0].0, FOREST_ROOT_ID);
        assert_eq!(initial.nodes[0].1.role(), Role::Window);
        assert!(initial.nodes[0].1.children().is_empty());
        assert_eq!(initial.focus, FOREST_ROOT_ID);
        assert_eq!(current.0.lock().unwrap().generation, None);

        let mut control = node(2, Some(1), &[]);
        control.role = AccessibilityRole::Button;
        control.label = Some("Published button".into());
        current
            .synchronize(
                AccessibilityUpdate::Full {
                    generation: Some(7),
                    nodes: vec![node(1, None, &[2]), control],
                },
                1.0,
            )
            .unwrap();
        let published = current.request_initial_tree().unwrap();
        assert_eq!(published.nodes.len(), 3);
        assert_eq!(published.tree.unwrap().root, FOREST_ROOT_ID);
        assert!(
            published
                .nodes
                .iter()
                .any(|(id, value)| *id == NodeId(2) && value.label() == Some("Published button"))
        );
        assert_eq!(current.0.lock().unwrap().generation, Some(7));
    }

    fn node(id: u64, parent: Option<u64>, children: &[u64]) -> AccessibilityNode {
        AccessibilityNode {
            id: StableNodeId::new(id).unwrap(),
            parent: parent.and_then(StableNodeId::new),
            children: children
                .iter()
                .copied()
                .filter_map(StableNodeId::new)
                .collect(),
            role: AccessibilityRole::Generic,
            label: None,
            description: None,
            value: None,
            disabled: false,
            checked: None,
            mixed: false,
            orientation: None,
            selected: None,
            multiline: false,
            editable: false,
            selection: None,
            modal: false,
            busy: false,
            invalid: false,
            numeric_minimum: None,
            numeric_maximum: None,
            numeric_step: None,
            numeric_value: None,
            focused: false,
            bounds: LayoutBox::default(),
            writing: Default::default(),
        }
    }

    #[test]
    fn semantic_focus_updates_keep_the_index_in_sync_without_tree_reconciliation() {
        let mut first = node(2, Some(1), &[]);
        first.focused = true;
        let mut second = node(3, Some(1), &[]);
        let mut projector = AccessibilityProjector::retain(
            vec![node(1, None, &[2, 3]), first.clone(), second.clone()],
            true,
            1.0,
            Some(0),
            true,
        );
        for generation in 1..=8 {
            first.focused = generation % 2 == 0;
            second.focused = !first.focused;
            let update = projector
                .apply_delta(AccessibilityDelta {
                    generation,
                    updated: vec![first.clone(), second.clone()],
                    removed: vec![],
                })
                .unwrap();
            assert_eq!(update.focus, NodeId(if first.focused { 2 } else { 3 }));
            assert!(update.tree.is_none());
            assert_eq!(update.nodes.len(), 2);
        }
        assert_eq!(projector.global_reconciliations.get(), 1);
        let removed = projector
            .apply_delta(AccessibilityDelta {
                generation: 9,
                updated: vec![],
                removed: vec![StableNodeId::new(1).unwrap()],
            })
            .unwrap();
        assert_eq!(removed.focus, FOREST_ROOT_ID);
        assert_eq!(removed.nodes.len(), 1);
        assert!(projector.focused.is_empty());
    }

    #[test]
    fn native_window_root_survives_focus_and_document_replacement() {
        let (mut projector, _) = AccessibilityProjector::new(Vec::new(), true, 1.0);
        projector.window_root = true;
        let initial = projector.full_update();
        assert_eq!(initial.tree.unwrap().root, FOREST_ROOT_ID);
        assert_eq!(initial.focus, FOREST_ROOT_ID);
        assert_eq!(initial.nodes[0].1.role(), Role::Window);

        let root = node(1, None, &[2]);
        let mut editor = node(2, Some(1), &[]);
        editor.role = AccessibilityRole::TextInput;
        editor.editable = true;
        projector.synchronize(vec![root.clone(), editor.clone()], 1.0);
        editor.focused = true;
        editor.value = Some("Edited".into());
        let focused = projector.synchronize(vec![root, editor], 1.0).unwrap();
        assert!(focused.tree.is_none());
        assert_eq!(focused.focus, NodeId(2));
        let full = projector.full_update();
        assert_eq!(full.tree.unwrap().root, FOREST_ROOT_ID);
        let window = &full
            .nodes
            .iter()
            .find(|(id, _)| *id == FOREST_ROOT_ID)
            .unwrap()
            .1;
        assert_eq!(window.role(), Role::Window);
        assert_eq!(window.children(), &[NodeId(1)]);

        for nodes in [vec![node(3, None, &[]), node(4, None, &[])], vec![]] {
            let updated = projector.synchronize(nodes, 1.0).unwrap();
            assert_eq!(updated.tree.unwrap().root, FOREST_ROOT_ID);
            assert_eq!(updated.focus, FOREST_ROOT_ID);
            assert_eq!(updated.nodes.last().unwrap().1.role(), Role::Window);
        }
        assert_eq!(projector.full_update().nodes.len(), 1);
    }

    #[test]
    fn landmark_roles_project_to_accesskit_landmarks() {
        let cases = [
            (AccessibilityRole::Main, Role::Main),
            (AccessibilityRole::Navigation, Role::Navigation),
            (AccessibilityRole::Banner, Role::Banner),
            (AccessibilityRole::ContentInfo, Role::ContentInfo),
            (AccessibilityRole::Complementary, Role::Complementary),
            (AccessibilityRole::Region, Role::Region),
            (AccessibilityRole::Search, Role::Search),
            (AccessibilityRole::Form, Role::Form),
        ];
        for (role, expected) in cases {
            assert_eq!(project_role(role, false), expected);
            assert!(!supports_click(role));
            assert!(!supports_focus(role));
        }
    }

    #[test]
    fn full_projection_uses_stable_ids_and_runtime_focus() {
        let root = node(1, None, &[2]);
        let mut input = node(2, Some(1), &[]);
        input.role = AccessibilityRole::TextInput;
        input.multiline = true;
        input.focused = true;
        input.label = Some("Message".into());
        input.description = Some("Required details".into());
        input.value = Some("hello".into());
        input.bounds = LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 40.0,
        };

        let (_, update) = AccessibilityProjector::new(vec![root, input], true, 1.0);

        assert_eq!(update.tree.unwrap().root, NodeId(1));
        assert_eq!(update.focus, NodeId(2));
        let (_, input) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .unwrap();
        assert_eq!(input.role(), Role::MultilineTextInput);
        assert_eq!(input.label(), Some("Message"));
        assert_eq!(input.description(), Some("Required details"));
        assert_eq!(input.value(), Some("hello"));
        assert_eq!(input.bounds().unwrap().x1, 110.0);
    }

    #[test]
    fn focus_delta_moves_accesskit_focus_from_generic_root_to_text_input() {
        let root = node(1, None, &[2]);
        let mut input = node(2, Some(1), &[]);
        input.role = AccessibilityRole::TextInput;
        let (mut projector, initial) =
            AccessibilityProjector::new(vec![root, input.clone()], true, 1.0);
        assert_eq!(initial.focus, NodeId(1));

        input.focused = true;
        let update = projector.apply(AccessibilityDelta {
            generation: 2,
            updated: vec![input],
            removed: vec![],
        });
        assert!(update.tree.is_none());
        assert_eq!(update.focus, NodeId(2));
    }

    #[test]
    fn new_child_delta_reattaches_through_the_cached_parent() {
        let root = node(1, None, &[2]);
        let child = node(2, Some(1), &[]);
        let (mut projector, _) = AccessibilityProjector::new(vec![root, child], false, 1.0);

        let update = projector.apply(AccessibilityDelta {
            generation: 2,
            updated: vec![node(3, Some(1), &[])],
            removed: vec![],
        });

        assert!(update.tree.is_none());
        let (_, parent) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(1))
            .expect("parent must ship with a newly attached child");
        assert!(parent.children().contains(&NodeId(3)));
        assert!(update.nodes.iter().any(|(id, _)| *id == NodeId(3)));
    }

    #[test]
    fn tombstone_reprojects_parent_without_retaining_ghost_child() {
        let root = node(1, None, &[2]);
        let child = node(2, Some(1), &[]);
        let (mut projector, _) = AccessibilityProjector::new(vec![root, child], false, 1.0);

        let update = projector.apply(AccessibilityDelta {
            generation: 2,
            updated: vec![],
            removed: vec![StableNodeId::new(2).unwrap()],
        });

        assert!(update.tree.is_none());
        assert_eq!(update.focus, NodeId(1));
        let (_, root) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(1))
            .unwrap();
        assert!(root.children().is_empty());
        assert!(update.nodes.iter().all(|(id, _)| *id != NodeId(2)));
    }

    #[test]
    fn tombstone_parent_drops_cached_descendants_from_incremental_update() {
        let root = node(1, None, &[2]);
        let parent = node(2, Some(1), &[3]);
        let grandchild = node(3, Some(2), &[]);
        let (mut projector, _) =
            AccessibilityProjector::new(vec![root, parent, grandchild.clone()], false, 1.0);

        let update = projector.apply(AccessibilityDelta {
            generation: 2,
            updated: vec![grandchild],
            removed: vec![StableNodeId::new(2).unwrap()],
        });

        assert!(update.tree.is_none());
        assert!(update.nodes.iter().all(|(id, _)| *id != NodeId(2)));
        assert!(update.nodes.iter().all(|(id, _)| *id != NodeId(3)));
        let (_, root_node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(1))
            .unwrap();
        assert!(root_node.children().is_empty());
        assert!(!projector.nodes.contains_key(&StableNodeId::new(3).unwrap()));
        assert!(
            projector
                .synchronize(vec![node(1, None, &[])], 1.0)
                .is_none()
        );
    }

    #[test]
    fn tombstone_parent_can_reparent_a_child_onto_a_surviving_uncle() {
        let root = node(1, None, &[2, 3]);
        let parent = node(2, Some(1), &[4]);
        let uncle = node(3, Some(1), &[]);
        let child = node(4, Some(2), &[]);
        let (mut projector, _) = AccessibilityProjector::new(
            vec![root, parent, uncle.clone(), child.clone()],
            false,
            1.0,
        );

        let mut moved = child;
        moved.parent = Some(StableNodeId::new(3).unwrap());
        let mut next_uncle = uncle;
        next_uncle.children = vec![StableNodeId::new(4).unwrap()];
        let update = projector.apply(AccessibilityDelta {
            generation: 2,
            updated: vec![moved, next_uncle],
            removed: vec![StableNodeId::new(2).unwrap()],
        });

        assert!(update.tree.is_none());
        assert!(update.nodes.iter().any(|(id, _)| *id == NodeId(4)));
        assert!(update.nodes.iter().all(|(id, _)| *id != NodeId(2)));
        let (_, uncle_node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(3))
            .unwrap();
        assert!(uncle_node.children().contains(&NodeId(4)));
        let (_, root_node) = update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(1))
            .unwrap();
        assert_eq!(root_node.children(), [NodeId(3)].as_slice());
    }

    #[test]
    fn synchronize_emits_only_changed_nodes() {
        let root = node(1, None, &[2]);
        let child = node(2, Some(1), &[]);
        let (mut projector, _) =
            AccessibilityProjector::new(vec![root.clone(), child.clone()], false, 1.0);
        assert!(
            projector
                .synchronize(vec![root.clone(), child.clone()], 1.0)
                .is_none()
        );

        let mut changed = child;
        changed.label = Some("changed".into());
        let update = projector.synchronize(vec![root, changed], 1.0).unwrap();
        assert_eq!(update.nodes.len(), 1);
        assert_eq!(update.nodes[0].0, NodeId(2));
    }

    #[test]
    fn stale_delta_cannot_roll_back_the_platform_tree() {
        let root = node(1, None, &[2]);
        let child = node(2, Some(1), &[]);
        let (mut projector, _) =
            AccessibilityProjector::new(vec![root.clone(), child.clone()], false, 1.0);

        let mut current = child.clone();
        current.label = Some("current".into());
        current.bounds.x = 10.0;
        current.bounds.width = 5.0;
        assert!(
            projector
                .apply_delta(AccessibilityDelta {
                    generation: 2,
                    updated: vec![current],
                    removed: vec![],
                })
                .is_some()
        );

        let mut stale = child;
        stale.label = Some("stale".into());
        stale.bounds.x = 100.0;
        assert!(
            projector
                .apply_delta(AccessibilityDelta {
                    generation: 1,
                    updated: vec![stale.clone()],
                    removed: vec![],
                })
                .is_none()
        );
        assert_eq!(
            projector.nodes[&StableNodeId::new(2).unwrap()]
                .label
                .as_deref(),
            Some("current")
        );

        let update = projector
            .synchronize_full(vec![root, stale], 2.0, Some(1))
            .expect("DPI change must reproject retained current semantics");
        let projected = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .unwrap()
            .1;
        assert_eq!(projected.label(), Some("current"));
        assert_eq!(projected.bounds().unwrap().x0, 20.0);
        assert_eq!(projected.bounds().unwrap().x1, 30.0);
    }

    #[test]
    fn empty_snapshot_uses_platform_forest_until_a_new_root_arrives() {
        let (mut projector, _) = AccessibilityProjector::new(vec![node(1, None, &[])], false, 1.0);
        let empty = projector
            .synchronize_full(Vec::new(), 1.0, Some(2))
            .unwrap();
        assert_eq!(empty.tree.unwrap().root, FOREST_ROOT_ID);
        assert_eq!(empty.focus, FOREST_ROOT_ID);
        assert_eq!(projector.generation, Some(2));

        let update = projector
            .synchronize_full(vec![node(9, None, &[])], 1.0, Some(3))
            .unwrap();
        assert_eq!(update.tree.unwrap().root, NodeId(9));
        assert_eq!(update.focus, NodeId(9));
        assert_eq!(projector.generation, Some(3));
    }

    #[test]
    fn explicitly_enabled_empty_projector_accepts_the_first_async_root() {
        let (mut projector, initial) = AccessibilityProjector::new(Vec::new(), false, 1.0);
        assert_eq!(initial.tree.unwrap().root, FOREST_ROOT_ID);
        assert_eq!(initial.focus, FOREST_ROOT_ID);

        let update = projector
            .apply_delta(AccessibilityDelta {
                generation: 1,
                updated: vec![node(7, None, &[])],
                removed: vec![],
            })
            .unwrap();
        assert_eq!(update.tree.unwrap().root, NodeId(7));
        assert_eq!(update.focus, NodeId(7));
    }

    #[test]
    fn multiple_runtime_roots_use_a_collision_free_platform_forest_root() {
        let first = node(1, None, &[]);
        let second = node(9, None, &[]);
        let (mut projector, _) = AccessibilityProjector::new(vec![first], false, 1.0);
        let update = projector
            .apply_delta(AccessibilityDelta {
                generation: 1,
                updated: vec![second],
                removed: vec![],
            })
            .unwrap();
        assert_eq!(update.tree.unwrap().root, FOREST_ROOT_ID);
        let forest = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == FOREST_ROOT_ID)
            .unwrap()
            .1;
        assert_eq!(forest.children(), [NodeId(1), NodeId(9)]);

        let update = projector
            .apply_delta(AccessibilityDelta {
                generation: 2,
                updated: vec![],
                removed: vec![StableNodeId::new(9).unwrap()],
            })
            .unwrap();
        assert_eq!(update.tree.unwrap().root, NodeId(1));
        assert!(update.nodes.iter().all(|(id, _)| *id != FOREST_ROOT_ID));

        let update = projector
            .apply_delta(AccessibilityDelta {
                generation: 3,
                updated: vec![],
                removed: vec![StableNodeId::new(1).unwrap()],
            })
            .unwrap();
        assert_eq!(update.tree.unwrap().root, FOREST_ROOT_ID);
        assert_eq!(update.focus, FOREST_ROOT_ID);
        assert!(
            update
                .nodes
                .iter()
                .find(|(id, _)| *id == FOREST_ROOT_ID)
                .unwrap()
                .1
                .children()
                .is_empty()
        );
    }

    #[test]
    fn actions_are_only_advertised_for_enabled_interactive_nodes() {
        let root = node(1, None, &[2, 3]);
        let mut enabled = node(2, Some(1), &[]);
        enabled.role = AccessibilityRole::Button;
        let mut disabled = node(3, Some(1), &[]);
        disabled.role = AccessibilityRole::Button;
        disabled.disabled = true;

        let (_, update) = AccessibilityProjector::new(vec![root, enabled, disabled], true, 1.0);
        let enabled = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .unwrap()
            .1;
        let disabled = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(3))
            .unwrap()
            .1;
        assert!(enabled.supports_action(Action::Click));
        assert!(enabled.supports_action(Action::Focus));
        assert!(!enabled.supports_action(Action::SetValue));
        assert!(!disabled.supports_action(Action::Click));
        assert!(!disabled.supports_action(Action::Focus));
        assert!(!disabled.supports_action(Action::SetValue));

        let mut input = node(4, Some(1), &[]);
        input.role = AccessibilityRole::TextInput;
        input.editable = true;
        let (_, input) = project_node(&input, Some(NodeId(40)), true, 1.0)
            .into_iter()
            .next()
            .unwrap();
        assert!(input.supports_action(Action::Click));
        assert!(input.supports_action(Action::Focus));
        assert!(input.supports_action(Action::SetValue));

        let mut read_only = node(5, Some(1), &[]);
        read_only.role = AccessibilityRole::TextInput;
        let (_, read_only) = project_node(&read_only, Some(NodeId(50)), true, 1.0)
            .into_iter()
            .next()
            .unwrap();
        assert!(read_only.is_read_only());
        assert!(!read_only.supports_action(Action::SetValue));

        let mut malformed = node(6, Some(1), &[]);
        malformed.editable = true;
        let (_, malformed) = project_node(&malformed, None, true, 1.0)
            .into_iter()
            .next()
            .unwrap();
        assert!(!malformed.supports_action(Action::SetValue));

        let mut range = node(7, Some(1), &[]);
        range.role = AccessibilityRole::Slider;
        range.busy = true;
        range.invalid = true;
        range.numeric_minimum = Some(-1.0);
        range.numeric_maximum = Some(1.0);
        range.numeric_step = Some(0.25);
        range.numeric_value = Some(0.5);
        let (_, range) = project_node(&range, None, true, 1.0)
            .into_iter()
            .next()
            .unwrap();
        assert!(range.supports_action(Action::SetValue));
        assert!(range.is_busy());
        assert_eq!(range.invalid(), Some(Invalid::True));
        assert_eq!(range.min_numeric_value(), Some(-1.0));
        assert_eq!(range.max_numeric_value(), Some(1.0));
        assert_eq!(range.numeric_value_step(), Some(0.25));
        assert_eq!(range.numeric_value(), Some(0.5));

        let mut group = node(8, Some(1), &[9]);
        group.role = AccessibilityRole::RadioGroup;
        group.orientation = Some(SelectionOrientation::Horizontal);
        let (_, group) = project_node(&group, None, true, 1.0)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(group.role(), Role::RadioGroup);
        assert_eq!(group.orientation(), Some(Orientation::Horizontal));
        assert!(!group.supports_action(Action::Click));

        let mut radio = node(9, Some(8), &[]);
        radio.role = AccessibilityRole::Radio;
        radio.checked = Some(true);
        let (_, radio) = project_node(&radio, None, true, 1.0)
            .into_iter()
            .next()
            .unwrap();
        assert_eq!(radio.role(), Role::RadioButton);
        assert_eq!(radio.toggled(), Some(Toggled::True));
        assert!(radio.supports_action(Action::Click));
        assert!(radio.supports_action(Action::Focus));
    }

    #[cfg(all(feature = "hosted", not(target_os = "android")))]
    #[test]
    fn a_text_run_reads_the_way_its_writing_mode_runs() {
        use nana_ui_core::{DirSpec, WritingContext, WritingModeSpec};
        let direction = |writing| {
            let root = node(1, None, &[2]);
            let mut input = node(2, Some(1), &[]);
            input.role = AccessibilityRole::TextInput;
            input.editable = true;
            input.value = Some("縦書き".into());
            input.writing = writing;
            let (projector, update) = AccessibilityProjector::new(vec![root, input], true, 1.0);
            let text_run_id = projector.text_runs[&StableNodeId::new(2).unwrap()];
            let text_run = &update
                .nodes
                .iter()
                .find(|(id, _)| *id == text_run_id)
                .unwrap()
                .1;
            text_run.text_direction()
        };
        let context = WritingContext::new;
        assert_eq!(
            direction(context(WritingModeSpec::HorizontalTb, DirSpec::Ltr)),
            Some(TextDirection::LeftToRight)
        );
        assert_eq!(
            direction(context(WritingModeSpec::HorizontalTb, DirSpec::Rtl)),
            Some(TextDirection::RightToLeft)
        );
        assert_eq!(
            direction(context(WritingModeSpec::VerticalRl, DirSpec::Ltr)),
            Some(TextDirection::TopToBottom)
        );
        assert_eq!(
            direction(context(WritingModeSpec::VerticalLr, DirSpec::Rtl)),
            Some(TextDirection::BottomToTop)
        );
    }

    #[test]
    fn unicode_text_run_selection_round_trips_without_byte_index_loss() {
        let root = node(1, None, &[2]);
        let mut input = node(2, Some(1), &[]);
        input.role = AccessibilityRole::TextInput;
        input.editable = true;
        input.value = Some("你a".into());
        input.selection = Some(nana_ui_runtime::TextSelection::new(0, "你".len()));
        let (projector, update) = AccessibilityProjector::new(vec![root, input], true, 1.0);
        let text_run_id = projector.text_runs[&StableNodeId::new(2).unwrap()];
        let input = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .unwrap()
            .1;
        assert!(input.children().contains(&text_run_id));
        assert!(input.supports_action(Action::SetTextSelection));
        let selection = input.text_selection().unwrap();
        assert_eq!(selection.anchor.character_index, 0);
        assert_eq!(selection.focus.character_index, 1);

        let text_run = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == text_run_id)
            .unwrap()
            .1;
        assert_eq!(text_run.role(), Role::TextRun);
        assert_eq!(text_run.value(), Some("你a"));
        assert_eq!(text_run.character_lengths(), [3, 1]);

        let projected = projector
            .project_action_request(ActionRequest {
                action: Action::SetTextSelection,
                target_tree: TreeId::ROOT,
                target_node: NodeId(2),
                data: Some(ActionData::SetTextSelection(AccessKitTextSelection {
                    anchor: TextPosition {
                        node: text_run_id,
                        character_index: 2,
                    },
                    focus: TextPosition {
                        node: text_run_id,
                        character_index: 1,
                    },
                })),
            })
            .unwrap();
        assert_eq!(
            projected.action,
            nana_ui_runtime::AccessibilityAction::SetSelection(
                nana_ui_runtime::TextSelection::new("你a".len(), "你".len())
            )
        );

        for (node, character_index) in [(text_run_id, 3), (NodeId(2), 0)] {
            assert!(
                projector
                    .project_action_request(ActionRequest {
                        action: Action::SetTextSelection,
                        target_tree: TreeId::ROOT,
                        target_node: NodeId(2),
                        data: Some(ActionData::SetTextSelection(AccessKitTextSelection {
                            anchor: TextPosition {
                                node,
                                character_index,
                            },
                            focus: TextPosition {
                                node: text_run_id,
                                character_index: 0,
                            },
                        })),
                    })
                    .is_none()
            );
        }
    }

    #[test]
    fn synthetic_text_run_rekeys_if_a_runtime_id_collides() {
        let root = node(1, None, &[2]);
        let mut input = node(2, Some(1), &[]);
        input.role = AccessibilityRole::TextInput;
        let (mut projector, _) =
            AccessibilityProjector::new(vec![root.clone(), input.clone()], false, 1.0);
        let previous = projector.text_runs[&StableNodeId::new(2).unwrap()];
        assert_eq!(previous, NodeId(u64::MAX));

        let mut next_root = root;
        next_root
            .children
            .push(StableNodeId::new(u64::MAX).unwrap());
        let update = projector
            .synchronize(vec![next_root, input, node(u64::MAX, Some(1), &[])], 1.0)
            .unwrap();
        let replacement = projector.text_runs[&StableNodeId::new(2).unwrap()];
        assert_ne!(replacement, previous);
        assert_ne!(replacement, NodeId(u64::MAX));
        let input = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .unwrap()
            .1;
        assert!(input.children().contains(&replacement));
        assert!(!input.children().contains(&previous));
    }

    #[test]
    fn text_run_lifecycle_tracks_input_role_and_removal() {
        let root = node(1, None, &[2]);
        let mut input = node(2, Some(1), &[]);
        input.role = AccessibilityRole::TextInput;
        input.value = Some("".into());
        input.selection = Some(nana_ui_runtime::TextSelection::caret(0));
        let (mut projector, _) =
            AccessibilityProjector::new(vec![root.clone(), input.clone()], false, 1.0);
        let first_text_run = projector.text_runs[&StableNodeId::new(2).unwrap()];

        let mut generic = input.clone();
        generic.role = AccessibilityRole::Generic;
        generic.selection = None;
        let update = projector
            .synchronize(vec![root.clone(), generic], 1.0)
            .unwrap();
        let projected = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(2))
            .unwrap()
            .1;
        assert!(!projected.children().contains(&first_text_run));
        assert!(projector.text_runs.is_empty());

        let update = projector
            .synchronize(vec![root.clone(), input], 1.0)
            .unwrap();
        let second_text_run = projector.text_runs[&StableNodeId::new(2).unwrap()];
        assert_ne!(second_text_run, first_text_run);
        assert!(update.nodes.iter().any(|(id, _)| *id == second_text_run));

        let update = projector
            .synchronize(vec![node(1, None, &[])], 1.0)
            .unwrap();
        let projected_root = &update
            .nodes
            .iter()
            .find(|(id, _)| *id == NodeId(1))
            .unwrap()
            .1;
        assert!(projected_root.children().is_empty());
        assert!(projector.text_runs.is_empty());
    }

    #[test]
    fn scale_change_reprojects_logical_bounds_in_physical_pixels() {
        let mut root = node(1, None, &[]);
        root.bounds = LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 30.0,
            height: 40.0,
        };
        let (mut projector, _) = AccessibilityProjector::new(vec![root.clone()], false, 1.0);

        let update = projector.synchronize(vec![root], 2.0).unwrap();
        let bounds = update.nodes[0].1.bounds().unwrap();
        assert_eq!(bounds.x0, 20.0);
        assert_eq!(bounds.y0, 40.0);
        assert_eq!(bounds.x1, 80.0);
        assert_eq!(bounds.y1, 120.0);
        assert!(update.tree.is_some());
    }

    #[cfg(all(feature = "hosted", not(target_os = "android")))]
    #[test]
    fn native_activation_materializes_current_state_only_when_requested() {
        let children = (2..=10_001).collect::<Vec<_>>();
        let mut nodes = vec![node(1, None, &children)];
        nodes.extend(children.iter().map(|id| node(*id, Some(1), &[])));
        let mut editor = nodes[1].clone();
        editor.role = AccessibilityRole::TextInput;
        editor.editable = true;
        editor.value = Some("Before".into());
        nodes[1] = editor.clone();
        let current = CurrentTree(Arc::new(Mutex::new(AccessibilityProjector::retain(
            nodes,
            true,
            1.0,
            Some(0),
            true,
        ))));
        let mut activation = current.clone();
        assert_eq!(current.0.lock().unwrap().full_updates.get(), 0);
        for generation in 1..=4 {
            editor.value = Some(format!("Edited {generation}").into());
            editor.focused = true;
            let update = current
                .synchronize(
                    AccessibilityUpdate::Delta(AccessibilityDelta {
                        generation,
                        updated: vec![editor.clone()],
                        removed: vec![],
                    }),
                    1.0,
                )
                .unwrap();
            assert_eq!(update.focus, NodeId(2));
            assert_eq!(update.nodes.len(), 2); // Editor and its text run.
            assert!(update.tree.is_none());
            assert!(current.0.try_lock().is_ok()); // Native callbacks may reenter now.
        }
        assert_eq!(current.0.lock().unwrap().full_updates.get(), 0);
        let before = activation.request_initial_tree().unwrap();
        assert_eq!(current.0.lock().unwrap().global_reconciliations.get(), 1);
        assert_eq!(before.tree.as_ref().unwrap().root, FOREST_ROOT_ID);
        assert_eq!(
            before
                .nodes
                .iter()
                .find(|(id, _)| *id == NodeId(2))
                .unwrap()
                .1
                .value(),
            Some("Edited 4")
        );
        current
            .synchronize(
                AccessibilityUpdate::Delta(AccessibilityDelta {
                    generation: 5,
                    updated: vec![],
                    removed: vec![StableNodeId::new(2).unwrap()],
                }),
                1.0,
            )
            .unwrap();
        assert_eq!(current.0.lock().unwrap().full_updates.get(), 1);
        let after = activation.request_initial_tree().unwrap();
        assert_eq!(after.focus, FOREST_ROOT_ID);
        assert!(after.nodes.iter().all(|(id, _)| *id != NodeId(2)));
        assert!(before.nodes.iter().any(|(id, _)| *id == NodeId(2)));
        assert_eq!(after.nodes.len(), 10_001); // Runtime nodes plus the window, no orphan text run.
        assert_eq!(current.0.lock().unwrap().full_updates.get(), 2);
    }

    #[cfg(all(feature = "hosted", not(target_os = "android")))]
    #[test]
    fn activation_always_returns_the_latest_complete_tree() {
        let (initial, _) = AccessibilityProjector::new(vec![node(1, None, &[])], false, 1.0);
        let current = Arc::new(Mutex::new(initial));
        let mut activation = CurrentTree(Arc::clone(&current));

        assert_eq!(
            activation
                .request_initial_tree()
                .unwrap()
                .tree
                .unwrap()
                .root,
            NodeId(1)
        );
        assert_eq!(
            activation
                .request_initial_tree()
                .unwrap()
                .tree
                .unwrap()
                .root,
            NodeId(1)
        );

        let (replacement, _) = AccessibilityProjector::new(vec![node(9, None, &[])], false, 1.0);
        *current.lock().unwrap() = replacement;
        assert_eq!(
            activation
                .request_initial_tree()
                .unwrap()
                .tree
                .unwrap()
                .root,
            NodeId(9)
        );
    }

    #[cfg(all(feature = "hosted", not(target_os = "android")))]
    #[test]
    fn action_queue_is_bounded_and_preserves_accepted_fifo_order() {
        let mut requests = VecDeque::new();
        for index in 0..=MAX_PENDING_ACCESSIBILITY_ACTIONS {
            let accepted = enqueue_accessibility_action(
                &mut requests,
                ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node: NodeId(index as u64 + 1),
                    data: None,
                },
            );
            assert_eq!(accepted, index < MAX_PENDING_ACCESSIBILITY_ACTIONS);
        }
        assert_eq!(requests.len(), MAX_PENDING_ACCESSIBILITY_ACTIONS);
        assert_eq!(requests.front().unwrap().target_node, NodeId(1));
        assert_eq!(
            requests.back().unwrap().target_node,
            NodeId(MAX_PENDING_ACCESSIBILITY_ACTIONS as u64)
        );
    }

    #[cfg(all(feature = "hosted", not(target_os = "android")))]
    #[test]
    fn set_value_action_requires_a_text_payload() {
        let root = node(1, None, &[7]);
        let mut input = node(7, Some(1), &[]);
        input.role = AccessibilityRole::TextInput;
        input.editable = true;
        input.value = Some("旧值".into());
        input.selection = Some(nana_ui_runtime::TextSelection::caret("旧值".len()));
        let (projector, _) = AccessibilityProjector::new(vec![root, input], true, 1.0);

        let projected = projector
            .project_action_request(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: NodeId(7),
                data: Some(ActionData::Value("新的值".into())),
            })
            .unwrap();
        assert_eq!(projected.target, StableNodeId::new(7).unwrap());
        assert_eq!(
            projected.action,
            nana_ui_runtime::AccessibilityAction::SetValue("新的值".into())
        );

        assert!(
            projector
                .project_action_request(ActionRequest {
                    action: Action::SetValue,
                    target_tree: TreeId::ROOT,
                    target_node: NodeId(7),
                    data: None,
                })
                .is_none()
        );
    }

    #[cfg(all(feature = "hosted", not(target_os = "android")))]
    #[test]
    fn range_set_value_accepts_only_finite_numeric_payloads() {
        let root = node(1, None, &[8]);
        let mut range = node(8, Some(1), &[]);
        range.role = AccessibilityRole::Slider;
        range.numeric_minimum = Some(0.0);
        range.numeric_maximum = Some(10.0);
        range.numeric_step = Some(0.5);
        range.numeric_value = Some(4.0);
        let (projector, _) = AccessibilityProjector::new(vec![root, range], true, 1.0);

        let projected = projector
            .project_action_request(ActionRequest {
                action: Action::SetValue,
                target_tree: TreeId::ROOT,
                target_node: NodeId(8),
                data: Some(ActionData::NumericValue(4.5)),
            })
            .unwrap();
        assert_eq!(
            projected.action,
            nana_ui_runtime::AccessibilityAction::SetValue("4.5".into())
        );

        assert!(
            projector
                .project_action_request(ActionRequest {
                    action: Action::SetValue,
                    target_tree: TreeId::ROOT,
                    target_node: NodeId(8),
                    data: Some(ActionData::NumericValue(f64::NAN)),
                })
                .is_none()
        );
    }
}
