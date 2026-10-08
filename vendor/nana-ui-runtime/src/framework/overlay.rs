use std::cmp::Ordering;
use std::collections::HashMap;

use nana_ui_core::DialogCloseTrigger;

use super::{AppContext, Entity, FrameworkError};
use crate::framework::hooked;
use crate::{
    CommandPalette, ComponentView, ConfirmDialog, Dialog, DocumentId, Drawer, IconButton,
    ModalInitialFocus, ModalSurface, MutationQueue, RangeField, ScrollOffset, StableNodeId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeOverlayKind {
    Dialog,
    /// An interactive nonmodal region; outside pointers and Tab remain free.
    Panel,
    Menu,
    Tooltip,
    /// A passive announcement such as a toast. It never takes the pointer or
    /// the focus, so the workspace underneath stays fully usable while it
    /// shows.
    Status,
}

impl RuntimeOverlayKind {
    pub(crate) const fn blocks_pointer(self) -> bool {
        matches!(self, Self::Dialog | Self::Menu)
    }

    /// Tooltips and status announcements never take focus, so showing or
    /// hiding one must leave focus where the user put it.
    pub(crate) const fn is_passive(self) -> bool {
        matches!(self, Self::Tooltip | Self::Status)
    }

    const fn traps_focus(self) -> bool {
        matches!(self, Self::Dialog | Self::Menu)
    }
}

/// Overlay behaviour follows the announced role, so a component opts into a
/// kind by projecting the matching [`AccessibilityRole`].
pub(crate) const fn overlay_kind_for_role(
    role: crate::AccessibilityRole,
) -> Option<RuntimeOverlayKind> {
    match role {
        crate::AccessibilityRole::Dialog | crate::AccessibilityRole::AlertDialog => {
            Some(RuntimeOverlayKind::Dialog)
        }
        crate::AccessibilityRole::Region => Some(RuntimeOverlayKind::Panel),
        crate::AccessibilityRole::Menu => Some(RuntimeOverlayKind::Menu),
        crate::AccessibilityRole::Tooltip => Some(RuntimeOverlayKind::Tooltip),
        crate::AccessibilityRole::Status => Some(RuntimeOverlayKind::Status),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveRuntimeOverlay {
    pub host: StableNodeId,
    pub root: StableNodeId,
    pub kind: RuntimeOverlayKind,
    pub restore_focus: Option<StableNodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayPointerPhase {
    Move,
    PrimaryDown,
    PrimaryUp,
    Cancel,
    Wheel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPointerDecision {
    pub target: Option<StableNodeId>,
    pub prevent_default: bool,
    pub dismissed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKey {
    Escape,
    Tab { reverse: bool },
}

impl AppContext {
    pub fn active_runtime_overlay(&self, document: DocumentId) -> Option<ActiveRuntimeOverlay> {
        self.active_runtime_overlays(document).into_iter().next()
    }

    pub fn has_blocking_runtime_overlay(&self, document: DocumentId) -> bool {
        self.active_runtime_overlays(document)
            .into_iter()
            .any(|overlay| overlay.kind.blocks_pointer())
    }

    /// Whether an open blocking overlay (a modal dialog, a drawer) covers
    /// `node`: `node` is outside it. Such a node takes no press, not even
    /// one that reaches it through a resize handle's slop.
    pub(crate) fn covered_by_blocking_overlay(
        &self,
        document: DocumentId,
        node: StableNodeId,
    ) -> bool {
        self.active_runtime_overlays(document)
            .into_iter()
            .any(|overlay| {
                overlay.kind.blocks_pointer()
                    && !self.world.is_descendant_or_self(node, overlay.root)
            })
    }

    /// The resize handle within slop of `(x, y)` — a split pane's, a dock's
    /// or a workspace region's — that a press or the cursor there reaches.
    /// A handle under an open modal is not reachable: the modal's surface
    /// takes the press, whatever lies within slop of it underneath.
    pub(crate) fn reachable_handle_near(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
        target: Option<StableNodeId>,
    ) -> [Option<StableNodeId>; 3] {
        let reachable = |handle: StableNodeId| !self.covered_by_blocking_overlay(document, handle);
        [
            self.split_handle_near_hit(document, x, y, target)
                .filter(|handle| reachable(*handle)),
            hooked!(self, dock, |h| (h.handle_near_hit)(
                self, document, x, y, target
            ))
            .filter(|handle| reachable(*handle)),
            hooked!(self, workspace, |h| (h.handle_near_hit)(
                self, document, x, y, target
            ))
            .filter(|handle| reachable(*handle)),
        ]
    }

    pub fn route_overlay_pointer(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        phase: OverlayPointerPhase,
        x: f32,
        y: f32,
    ) -> Result<OverlayPointerDecision, FrameworkError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(FrameworkError::InvalidInput);
        }
        let sequence = (document, pointer_id);
        let leased = self
            .component_lifecycle
            .overlay_pointer_sequences
            .contains(&sequence);
        let blocking = self
            .active_runtime_overlays(document)
            .into_iter()
            .find(|overlay| overlay.kind.blocks_pointer());

        if blocking.is_none() && leased {
            if matches!(
                phase,
                OverlayPointerPhase::PrimaryUp | OverlayPointerPhase::Cancel
            ) {
                self.component_lifecycle
                    .overlay_pointer_sequences
                    .remove(&sequence);
                self.component_lifecycle
                    .overlay_outside_presses
                    .remove(&sequence);
            }
            return Ok(OverlayPointerDecision {
                target: None,
                prevent_default: true,
                dismissed: false,
            });
        }
        let Some(overlay) = blocking else {
            return Ok(OverlayPointerDecision {
                target: self.pointer_target(document, x, y),
                prevent_default: false,
                dismissed: false,
            });
        };

        let outside_press = if matches!(
            phase,
            OverlayPointerPhase::PrimaryUp | OverlayPointerPhase::Cancel
        ) {
            self.component_lifecycle
                .overlay_outside_presses
                .remove(&sequence)
        } else {
            None
        };
        if phase == OverlayPointerPhase::PrimaryDown {
            self.component_lifecycle
                .overlay_pointer_sequences
                .insert(sequence);
        } else if matches!(
            phase,
            OverlayPointerPhase::PrimaryUp | OverlayPointerPhase::Cancel
        ) {
            self.component_lifecycle
                .overlay_pointer_sequences
                .remove(&sequence);
        }

        let target = self.overlay_pointer_target(document, overlay.root, x, y);
        let mut dismissed = false;
        if phase == OverlayPointerPhase::PrimaryDown && target.is_none() {
            self.release_pointer(document, pointer_id);
            let token = self
                .component_lifecycle
                .overlay_activation_tokens
                .get(&overlay.host)
                .copied()
                .unwrap_or_default();
            self.component_lifecycle
                .overlay_outside_presses
                .insert(sequence, (overlay.root, token));
        } else if phase == OverlayPointerPhase::PrimaryUp
            && target.is_none()
            && outside_press
                == Some((
                    overlay.root,
                    self.component_lifecycle
                        .overlay_activation_tokens
                        .get(&overlay.host)
                        .copied()
                        .unwrap_or_default(),
                ))
        {
            dismissed = match overlay.kind {
                RuntimeOverlayKind::Dialog => {
                    if self.dialog_allows(overlay.root, DialogCloseTrigger::Outside) {
                        self.dismiss_overlay(Entity::from_stable_id(overlay.host))?
                    } else {
                        false
                    }
                }
                RuntimeOverlayKind::Menu => {
                    self.dismiss_overlay(Entity::from_stable_id(overlay.host))?
                }
                RuntimeOverlayKind::Tooltip
                | RuntimeOverlayKind::Status
                | RuntimeOverlayKind::Panel => false,
            };
        }
        Ok(OverlayPointerDecision {
            target,
            prevent_default: true,
            dismissed,
        })
    }

    pub fn route_overlay_key(
        &mut self,
        document: DocumentId,
        key: OverlayKey,
    ) -> Result<bool, FrameworkError> {
        match key {
            OverlayKey::Escape => {
                let Some(overlay) = self.active_runtime_overlay(document) else {
                    return Ok(false);
                };
                match overlay.kind {
                    RuntimeOverlayKind::Dialog => {
                        if self.dialog_allows(overlay.root, DialogCloseTrigger::Escape) {
                            self.dismiss_overlay(Entity::from_stable_id(overlay.host))?;
                        }
                    }
                    RuntimeOverlayKind::Panel => {
                        if !self
                            .views
                            .get(&overlay.root)
                            .and_then(|view| view.downcast_ref::<crate::Panel>())
                            .is_some_and(|panel| panel.close_on_escape)
                        {
                            return Ok(false);
                        }
                        self.dismiss_overlay(Entity::from_stable_id(overlay.host))?;
                    }
                    RuntimeOverlayKind::Menu => {
                        self.dismiss_overlay(Entity::from_stable_id(overlay.host))?;
                    }
                    RuntimeOverlayKind::Tooltip => {
                        self.leave_tooltip(overlay.host)?;
                        if self
                            .world
                            .overlay_host(overlay.host)
                            .is_some_and(|state| state.active == Some(overlay.root))
                        {
                            self.dismiss_overlay(Entity::from_stable_id(overlay.host))?;
                        }
                    }
                    // An announcement is not something Escape acts on, so it
                    // must not swallow the key from whatever sits underneath.
                    RuntimeOverlayKind::Status => return Ok(false),
                }
                Ok(true)
            }
            OverlayKey::Tab { reverse } => {
                let Some(overlay) = self
                    .active_runtime_overlays(document)
                    .into_iter()
                    .find(|overlay| overlay.kind.traps_focus())
                else {
                    return Ok(false);
                };
                let candidates = self
                    .sequential_focus_candidates(document)
                    .into_iter()
                    .filter(|candidate| {
                        self.overlay_descendant(overlay.root, *candidate)
                            && self.overlay_focus_candidate(document, *candidate)
                    })
                    .collect::<Vec<_>>();
                let candidates = if candidates.len() > 1 {
                    candidates
                        .into_iter()
                        .filter(|candidate| *candidate != overlay.root)
                        .collect::<Vec<_>>()
                } else {
                    candidates
                };
                if candidates.is_empty() {
                    return Ok(true);
                }
                let current = self.world.focused(document);
                let next = current
                    .and_then(|current| {
                        candidates
                            .iter()
                            .position(|candidate| *candidate == current)
                    })
                    .map(|index| {
                        if reverse {
                            (index + candidates.len() - 1) % candidates.len()
                        } else {
                            (index + 1) % candidates.len()
                        }
                    })
                    .unwrap_or_else(|| if reverse { candidates.len() - 1 } else { 0 });
                self.focus_node(document, candidates[next])?;
                Ok(true)
            }
        }
    }

    pub fn scroll_overlay_from(
        &mut self,
        document: DocumentId,
        target: StableNodeId,
        delta: ScrollOffset,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        let Some(root) = self
            .active_runtime_overlays(document)
            .into_iter()
            .find(|overlay| overlay.kind.blocks_pointer())
            .map(|overlay| overlay.root)
        else {
            return Ok(None);
        };
        if !self.overlay_descendant(root, target) {
            return Ok(None);
        }
        let mut current = Some(target);
        while let Some(id) = current {
            if self.scroll_node_by(id, delta)? {
                return Ok(Some(id));
            }
            if id == root {
                break;
            }
            current = self.world.parent_id(id);
        }
        Ok(None)
    }

    pub(super) fn prepare_blocking_overlay_activation(
        &mut self,
        document: DocumentId,
        root: StableNodeId,
    ) {
        let blocking = self
            .runtime_overlay_kind(root)
            .is_some_and(RuntimeOverlayKind::blocks_pointer);
        if !blocking {
            return;
        }
        let mut mutations = MutationQueue::new();
        let mut closed_tooltips = Vec::new();
        let tooltip_targets = self
            .component_lifecycle
            .tooltips
            .keys()
            .copied()
            .filter(|target| {
                self.world
                    .node(*target)
                    .is_some_and(|node| node.document == document)
            })
            .collect::<Vec<_>>();
        for target in tooltip_targets {
            let Some(lifecycle) = self.component_lifecycle.tooltips.get(&target) else {
                continue;
            };
            if !lifecycle.open && lifecycle.show_at.is_none() {
                continue;
            }
            let Some(mut button) = self
                .views
                .get(&target)
                .and_then(|view| view.downcast_ref::<IconButton>())
                .cloned()
            else {
                continue;
            };
            button.tooltip_open = false;
            button.project(target, &self.world, &mut mutations);
            mutations.set_overlay_host(target, crate::OverlayHostState::default());
            closed_tooltips.push((target, button));
        }
        let captures = self.world.pointer_captures(document);
        let mut cancelled_ranges = Vec::new();
        for (_, target) in &captures {
            if self.is_range_field(*target) {
                let Some(mut range) = self
                    .views
                    .get(target)
                    .and_then(|view| view.downcast_ref::<RangeField>())
                    .cloned()
                else {
                    continue;
                };
                if let Some(dragging) = range.dragging {
                    range.value = dragging.initial_value;
                }
                range.dragging = None;
                range.project(*target, &self.world, &mut mutations);
                cancelled_ranges.push((*target, range));
            }
        }
        for (pointer_id, target) in captures {
            if self.world.pointer_capture(document, pointer_id).is_some() {
                mutations.release_pointer(pointer_id, target);
            }
        }
        // This queue is derived only from retained tooltip/capture authority
        // and emits no application events. One atomic world commit prevents a
        // stale lifecycle record from producing partially applied cleanup.
        if !mutations.is_empty() && self.commit_mutations(mutations).is_err() {
            return;
        }
        for (target, button) in closed_tooltips {
            self.install_view(target, button);
            if let Some(lifecycle) = self.component_lifecycle.tooltips.get_mut(&target) {
                lifecycle.show_at = None;
                lifecycle.open = false;
            }
        }
        for (target, range) in cancelled_ranges {
            self.views.insert(target, Box::new(range));
        }
        self.world.clear_pointer_interactions(document);
    }

    pub(super) fn overlay_focus_candidate(&self, document: DocumentId, id: StableNodeId) -> bool {
        self.sequential_focus_candidate(document, id)
    }

    /// Focus the first reachable sequential stop inside a mounted subtree.
    /// Uses the same disabled, hidden and roving-group rules as Tab navigation.
    pub fn focus_first_in(
        &mut self,
        document: DocumentId,
        root: StableNodeId,
    ) -> Result<bool, FrameworkError> {
        let target = self
            .sequential_focus_candidates(document)
            .into_iter()
            .find(|id| self.overlay_descendant(root, *id));
        match target {
            Some(target) => self.focus_node(document, target),
            None => Ok(false),
        }
    }

    pub(super) fn first_overlay_focusable(
        &self,
        document: DocumentId,
        root: StableNodeId,
    ) -> Option<StableNodeId> {
        self.world
            .document_order(document)
            .into_iter()
            .find(|candidate| {
                self.sequential_focus_candidate(document, *candidate)
                    && self.overlay_descendant(root, *candidate)
                    && self.overlay_reachable_within(root, *candidate)
            })
    }

    fn active_runtime_overlays(&self, document: DocumentId) -> Vec<ActiveRuntimeOverlay> {
        // Start from the overlay-host index, not document order. This runs on
        // every pointer event -- `route_overlay_pointer` calls it to ask whether
        // anything blocks the pointer -- and walking the document to find hosts
        // allocated an id vector plus a full reverse-lookup map per event, on a
        // tree that usually has no overlay open at all. The index exists for
        // exactly this reason; see `UiWorld::overlay_host_ids`.
        let mut hosts: Vec<StableNodeId> = self.world.overlay_host_ids(document).collect();
        if hosts.is_empty() {
            return Vec::new();
        }
        let order = self.world.document_order(document);
        // Hosts and their active roots are both entries in document order. Build
        // the reverse lookup once so several overlays do not each rescan the
        // complete document with `position`.
        let order_index = order
            .iter()
            .enumerate()
            .map(|(index, id)| (*id, index))
            .collect::<HashMap<_, _>>();
        // Document order, so the stable sort below breaks ties the way it did
        // when this iterated the document directly.
        hosts.sort_by_key(|host| order_index.get(host).copied().unwrap_or(usize::MAX));
        let mut overlays = hosts
            .iter()
            .filter_map(|host| {
                let document_order = order_index.get(host).copied()?;
                let state = self.world.overlay_host(*host)?;
                let root = state.active?;
                if self.world.surface_closed(root) {
                    return None;
                }
                let node = self.world.node(root)?;
                if node.parent != Some(*host)
                    || !self.world.is_mounted(*host)
                    || !self.world.is_mounted(root)
                    || !self.world.is_overlay_reachable(root)
                {
                    return None;
                }
                let kind = self.runtime_overlay_kind(root)?;
                let root_order = order_index.get(&root).copied().unwrap_or(document_order);
                let z = self
                    .world
                    .node_style(root)
                    .and_then(|style| style.layout.z_index)
                    .unwrap_or_default();
                Some((
                    z,
                    root_order,
                    ActiveRuntimeOverlay {
                        host: *host,
                        root,
                        kind,
                        restore_focus: state.restore_focus,
                    },
                ))
            })
            .collect::<Vec<_>>();
        overlays.sort_by(|left, right| match right.0.cmp(&left.0) {
            Ordering::Equal => right.1.cmp(&left.1),
            ordering => ordering,
        });
        overlays
            .into_iter()
            .map(|(_, _, overlay)| overlay)
            .collect()
    }

    pub(super) fn runtime_overlay_kind(&self, id: StableNodeId) -> Option<RuntimeOverlayKind> {
        overlay_kind_for_role(self.world.accessibility(id)?.role)
    }

    pub(super) fn dialog_allows(&self, root: StableNodeId, trigger: DialogCloseTrigger) -> bool {
        let Some(view) = self.views.get(&root) else {
            return false;
        };
        #[cfg(feature = "image-viewer")]
        if view.downcast_ref::<crate::ImageViewer>().is_some() {
            return matches!(trigger, DialogCloseTrigger::Escape);
        }
        view.downcast_ref::<Dialog>()
            .is_some_and(|dialog| dialog.close_policy.allows(trigger))
            || view.downcast_ref::<ConfirmDialog>().is_some_and(|dialog| {
                !dialog.busy && dialog.behavior().close_policy.allows(trigger)
            })
            || view
                .downcast_ref::<Drawer>()
                .is_some_and(|drawer| drawer.behavior().close_policy.allows(trigger))
            || view.downcast_ref::<CommandPalette>().is_some()
    }

    pub(super) fn modal_action_context(
        &self,
        id: StableNodeId,
    ) -> Option<(
        StableNodeId,
        Option<StableNodeId>,
        bool,
        Option<crate::ConfirmIntent>,
    )> {
        let mut current = Some(id);
        while let Some(root) = current {
            let view = self.views.get(&root)?;
            if let Some(dialog) = view.downcast_ref::<Dialog>() {
                return Some((root, dialog.slots.close_action, false, None));
            }
            if let Some(dialog) = view.downcast_ref::<ConfirmDialog>() {
                let close_action = dialog
                    .slots()
                    .close_action
                    .filter(|action| self.overlay_descendant(*action, id));
                let action = dialog
                    .slots()
                    .actions
                    .iter()
                    .copied()
                    .find(|action| self.overlay_descendant(*action, id));
                let intent = dialog.confirm_slots().and_then(|slots| {
                    if action == Some(slots.cancel) {
                        Some(crate::ConfirmIntent::Cancel)
                    } else if action == slots.secondary {
                        Some(crate::ConfirmIntent::Secondary)
                    } else if action == Some(slots.confirm) {
                        Some(crate::ConfirmIntent::Confirm {
                            danger: dialog.danger,
                        })
                    } else {
                        None
                    }
                });
                return Some((
                    root,
                    close_action,
                    dialog.busy && (close_action.is_some() || action.is_some()),
                    intent,
                ));
            }
            if let Some(drawer) = view.downcast_ref::<Drawer>() {
                return Some((root, drawer.slots().close_action, false, None));
            }
            current = self.world.parent_id(root);
        }
        None
    }

    pub(super) fn confirm_busy_action_subtree(&self, id: StableNodeId) -> bool {
        self.modal_action_context(id)
            .is_some_and(|(_, _, busy, _)| busy)
    }

    pub(super) fn modal_initial_focus(
        &self,
        document: DocumentId,
        root: StableNodeId,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        let Some(view) = self.views.get(&root) else {
            return Ok(None);
        };
        let (initial, actions) = if let Some(dialog) = view.downcast_ref::<Dialog>() {
            (dialog.initial_focus, dialog.slots.actions.as_slice())
        } else if let Some(dialog) = view.downcast_ref::<ConfirmDialog>() {
            (
                dialog.behavior().initial_focus,
                dialog.slots().actions.as_slice(),
            )
        } else if let Some(drawer) = view.downcast_ref::<Drawer>() {
            (
                drawer.behavior().initial_focus,
                drawer.slots().actions.as_slice(),
            )
        } else {
            return Ok(None);
        };
        let candidate = |id| {
            self.sequential_focus_candidate(document, id) && self.overlay_reachable_within(root, id)
        };
        let fallback = || self.first_overlay_focusable(document, root);
        match initial {
            ModalInitialFocus::Surface => Ok(candidate(root).then_some(root).or_else(fallback)),
            ModalInitialFocus::FirstAction => Ok(actions
                .iter()
                .copied()
                .find(|id| candidate(*id))
                .or_else(fallback)),
            ModalInitialFocus::Target(target) => {
                if !self.overlay_descendant(root, target) || !candidate(target) {
                    return Err(FrameworkError::InvalidComponentHierarchy {
                        parent: root,
                        child: target,
                    });
                }
                Ok(Some(target))
            }
        }
    }

    pub(super) fn validate_modal_slots_for_activation(
        &self,
        root: StableNodeId,
    ) -> Result<(), FrameworkError> {
        let Some(view) = self.views.get(&root) else {
            return Err(FrameworkError::MissingView(root));
        };
        let expected = if let Some(dialog) = view.downcast_ref::<Dialog>() {
            dialog.slots.ordered()
        } else if let Some(dialog) = view.downcast_ref::<ConfirmDialog>() {
            if dialog.confirm_slots().is_none() {
                return Err(FrameworkError::InvalidModalSlots {
                    parent: root,
                    slot: None,
                });
            }
            dialog.slots().ordered()
        } else if let Some(drawer) = view.downcast_ref::<Drawer>() {
            drawer.slots().ordered()
        } else {
            return Ok(());
        };
        let actual = self
            .world
            .node(root)
            .ok_or(FrameworkError::MissingView(root))?
            .children;
        if actual != expected {
            return Err(FrameworkError::InvalidModalSlots {
                parent: root,
                slot: actual
                    .iter()
                    .find(|id| !expected.contains(id))
                    .copied()
                    .or_else(|| expected.iter().find(|id| !actual.contains(id)).copied()),
            });
        }
        Ok(())
    }

    pub(super) fn overlay_descendant(&self, root: StableNodeId, id: StableNodeId) -> bool {
        let mut current = Some(id);
        while let Some(candidate) = current {
            if candidate == root {
                return true;
            }
            current = self.world.parent_id(candidate);
        }
        false
    }

    fn overlay_pointer_target(
        &self,
        document: DocumentId,
        root: StableNodeId,
        x: f32,
        y: f32,
    ) -> Option<StableNodeId> {
        let candidates = self
            .world
            .hit_test_candidates(document, x, y)
            .into_iter()
            .filter(|candidate| {
                self.overlay_descendant(root, *candidate)
                    && self.world.is_overlay_reachable(*candidate)
            })
            .collect::<Vec<_>>();
        candidates
            .iter()
            .copied()
            .find(|candidate| *candidate != root)
            .or_else(|| {
                candidates.first().copied().filter(|candidate| {
                    *candidate != root
                        || match self.world.component_geometry(root) {
                            Some(crate::ComponentGeometry::ModalFrame { surface, .. }) => {
                                surface.contains(x, y)
                            }
                            _ => true,
                        }
                })
            })
    }

    pub(super) fn overlay_reachable_within(&self, root: StableNodeId, id: StableNodeId) -> bool {
        let mut child = id;
        let mut current = Some(id);
        while let Some(candidate) = current {
            if !self.world.presence_live(candidate)
                || self
                    .world
                    .node_style(candidate)
                    .is_some_and(|style| style.layout.omits_box())
            {
                return false;
            }
            if candidate == root {
                return true;
            }
            let parent = self.world.parent_id(candidate);
            if let Some(parent) = parent {
                if parent != root
                    && self
                        .world
                        .overlay_host(parent)
                        .is_some_and(|state| state.active != Some(child))
                {
                    return false;
                }
                child = parent;
            }
            current = parent;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActionMenu, ActionMenuItem, Button, LayoutBox, MountState, NodeKind, OverlayHost, Toast,
        ToastTone, Tooltip,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering as AtomicOrdering},
    };

    fn write_layout(context: &mut AppContext, id: StableNodeId, layout: LayoutBox) {
        let mut mutations = MutationQueue::new();
        mutations.write_layout(id, layout);
        context.commit_mutations(mutations).unwrap();
    }

    #[test]
    fn outside_dialog_dismisses_only_after_a_complete_outside_click_and_leases_sequence() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let base = context
            .create_component(document, Button::new("Open"))
            .unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let dialog = context
            .create_component(document, Dialog::new("Settings"))
            .unwrap();
        context.append_child(host, dialog).unwrap();
        write_layout(
            &mut context,
            base.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 300.0,
                height: 300.0,
            },
        );
        write_layout(
            &mut context,
            dialog.stable_id(),
            LayoutBox {
                x: 100.0,
                y: 100.0,
                width: 100.0,
                height: 100.0,
            },
        );
        context.focus_node(document, base.stable_id()).unwrap();
        context
            .set_pointer_hover(document, 7, Some(base.stable_id()))
            .unwrap();
        context
            .press_pointer(document, 7, base.stable_id())
            .unwrap();
        let mut capture = MutationQueue::new();
        capture.capture_pointer(7, base.stable_id());
        context.commit_mutations(capture).unwrap();
        context.activate_overlay(host, dialog).unwrap();
        assert_eq!(context.world.pointer_hover(document, 7), None);
        assert_eq!(context.world.pointer_press(document, 7), None);
        assert_eq!(context.world.pointer_capture(document, 7), None);
        context.rebuild_hit_test(document);

        context
            .route_overlay_pointer(document, 9, OverlayPointerPhase::PrimaryDown, 20.0, 20.0)
            .unwrap();
        context.dismiss_overlay(host).unwrap();
        context.activate_overlay(host, dialog).unwrap();
        let stale_up = context
            .route_overlay_pointer(document, 9, OverlayPointerPhase::PrimaryUp, 20.0, 20.0)
            .unwrap();
        assert!(!stale_up.dismissed);
        assert_eq!(
            context.world.overlay_host(host.stable_id()).unwrap().active,
            Some(dialog.stable_id())
        );

        let outside_down = context
            .route_overlay_pointer(document, 8, OverlayPointerPhase::PrimaryDown, 20.0, 20.0)
            .unwrap();
        assert!(!outside_down.dismissed);
        let inside_up = context
            .route_overlay_pointer(document, 8, OverlayPointerPhase::PrimaryUp, 150.0, 150.0)
            .unwrap();
        assert!(!inside_up.dismissed);
        assert_eq!(
            context.world.overlay_host(host.stable_id()).unwrap().active,
            Some(dialog.stable_id())
        );

        let down = context
            .route_overlay_pointer(document, 7, OverlayPointerPhase::PrimaryDown, 20.0, 20.0)
            .unwrap();
        assert!(down.prevent_default);
        assert!(!down.dismissed);
        assert_eq!(down.target, None);
        assert_eq!(context.world.focused(document), Some(dialog.stable_id()));

        let up = context
            .route_overlay_pointer(document, 7, OverlayPointerPhase::PrimaryUp, 20.0, 20.0)
            .unwrap();
        assert!(up.prevent_default);
        assert!(up.dismissed);
        assert_eq!(up.target, None);
        assert_eq!(context.world.focused(document), Some(base.stable_id()));
        assert!(
            context
                .route_overlay_pointer(document, 7, OverlayPointerPhase::Move, 20.0, 20.0,)
                .unwrap()
                .target
                .is_some()
        );
    }

    #[test]
    fn locked_dialog_consumes_outside_and_escape_without_closing() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let dialog = context
            .create_component(
                document,
                Dialog::new("Locked").close_policy(nana_ui_core::DialogClosePolicy {
                    close_disabled: true,
                    ..nana_ui_core::DialogClosePolicy::default()
                }),
            )
            .unwrap();
        context.append_child(host, dialog).unwrap();
        context.activate_overlay(host, dialog).unwrap();
        context.rebuild_hit_test(document);

        let outside = context
            .route_overlay_pointer(document, 1, OverlayPointerPhase::PrimaryDown, 50.0, 50.0)
            .unwrap();
        assert!(outside.prevent_default);
        assert!(!outside.dismissed);
        assert!(
            context
                .route_overlay_key(document, OverlayKey::Escape)
                .unwrap()
        );
        assert_eq!(
            context.world.overlay_host(host.stable_id()).unwrap().active,
            Some(dialog.stable_id())
        );
    }

    #[test]
    fn failed_activation_handler_preserves_pointer_state_and_old_overlay_authority() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let foreign_document = DocumentId::new(2).unwrap();
        let base = context
            .create_component(document, Button::new("Base"))
            .unwrap();
        let foreign = context
            .create_component(foreign_document, Button::new("Foreign"))
            .unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let dialog = context
            .create_component(document, Dialog::new("Settings"))
            .unwrap();
        context.append_child(host, dialog).unwrap();
        context
            .set_pointer_hover(document, 42, Some(base.stable_id()))
            .unwrap();
        context
            .press_pointer(document, 42, base.stable_id())
            .unwrap();
        let mut capture = MutationQueue::new();
        capture.capture_pointer(42, base.stable_id());
        context.commit_mutations(capture).unwrap();
        context
            .on(host, move |_host, _event: &crate::OverlayChanged, cx| {
                cx.mutations()
                    .request_focus(document, Some(foreign.stable_id()));
            })
            .unwrap();

        assert!(context.activate_overlay(host, dialog).is_err());
        assert_eq!(
            context.world.overlay_host(host.stable_id()).unwrap().active,
            None
        );
        assert_eq!(
            context.world.pointer_hover(document, 42),
            Some(base.stable_id())
        );
        assert_eq!(
            context.world.pointer_press(document, 42),
            Some(base.stable_id())
        );
        assert_eq!(
            context.world.pointer_capture(document, 42),
            Some(base.stable_id())
        );
    }

    #[test]
    fn activation_handler_final_authority_controls_token_cleanup_and_outside_lease() {
        for outcome in 0..3 {
            let mut context = AppContext::new();
            let document = DocumentId::new(1).unwrap();
            let base = context
                .create_component(document, Button::new("Base"))
                .unwrap();
            let host = context
                .create_component(document, OverlayHost::new())
                .unwrap();
            let requested = context
                .create_component(document, Dialog::new("Requested"))
                .unwrap();
            let replacement = context
                .create_component(document, Dialog::new("Replacement"))
                .unwrap();
            context.append_child(host, requested).unwrap();
            context.append_child(host, replacement).unwrap();
            context
                .set_pointer_hover(document, 42, Some(base.stable_id()))
                .unwrap();
            let mut capture = MutationQueue::new();
            capture.capture_pointer(42, base.stable_id());
            context.commit_mutations(capture).unwrap();
            let final_active = (outcome == 1).then_some(replacement.stable_id());
            let requested_id = requested.stable_id();
            context
                .on(host, move |_host, _event: &crate::OverlayChanged, cx| {
                    if outcome == 2 {
                        cx.mutations().park_subtree(requested_id);
                    } else {
                        cx.mutations().set_overlay_host(
                            host.stable_id(),
                            crate::OverlayHostState {
                                active: final_active,
                                restore_focus: None,
                            },
                        );
                    }
                    cx.mutations().request_focus(document, final_active);
                })
                .unwrap();

            assert_eq!(
                context.activate_overlay(host, requested).unwrap(),
                outcome == 1
            );
            assert_eq!(
                context.world.overlay_host(host.stable_id()).unwrap().active,
                final_active
            );
            if outcome == 1 {
                assert_eq!(context.world.pointer_hover(document, 42), None);
                assert_eq!(context.world.pointer_capture(document, 42), None);
                let first_token =
                    context.component_lifecycle.overlay_activation_tokens[&host.stable_id()];
                context
                    .route_overlay_pointer(document, 9, OverlayPointerPhase::PrimaryDown, 0.0, 0.0)
                    .unwrap();
                let mut close = MutationQueue::new();
                close.set_overlay_host(host.stable_id(), crate::OverlayHostState::default());
                close.request_focus(document, None);
                context.commit_mutations(close).unwrap();
                assert!(context.activate_overlay(host, replacement).unwrap());
                assert_ne!(
                    context.component_lifecycle.overlay_activation_tokens[&host.stable_id()],
                    first_token
                );
                let stale_up = context
                    .route_overlay_pointer(document, 9, OverlayPointerPhase::PrimaryUp, 0.0, 0.0)
                    .unwrap();
                assert!(!stale_up.dismissed);
                assert_eq!(
                    context.world.overlay_host(host.stable_id()).unwrap().active,
                    Some(replacement.stable_id())
                );
            } else {
                assert_eq!(
                    context.world.pointer_hover(document, 42),
                    Some(base.stable_id())
                );
                assert_eq!(
                    context.world.pointer_capture(document, 42),
                    Some(base.stable_id())
                );
                assert!(
                    !context
                        .component_lifecycle
                        .overlay_activation_tokens
                        .contains_key(&host.stable_id())
                );
            }
        }
    }

    #[test]
    fn invalid_handler_replacement_rolls_back_overlay_authority_before_returning() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let base = context
            .create_component(document, Button::new("Base"))
            .unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let requested = context
            .create_component(document, Dialog::new("Requested"))
            .unwrap();
        let invalid = context
            .create_component(
                document,
                ConfirmDialog::new("Invalid replacement", "Typed actions are missing"),
            )
            .unwrap();
        context.append_child(host, requested).unwrap();
        context.append_child(host, invalid).unwrap();
        context.focus_node(document, base.stable_id()).unwrap();
        context
            .set_pointer_hover(document, 7, Some(base.stable_id()))
            .unwrap();
        context
            .on(host, move |_host, _event: &crate::OverlayChanged, cx| {
                cx.mutations().set_overlay_host(
                    host.stable_id(),
                    crate::OverlayHostState {
                        active: Some(invalid.stable_id()),
                        restore_focus: None,
                    },
                );
                cx.mutations()
                    .request_focus(document, Some(invalid.stable_id()));
            })
            .unwrap();

        assert!(matches!(
            context.activate_overlay(host, requested),
            Err(FrameworkError::InvalidModalSlots { parent, slot: None })
                if parent == invalid.stable_id()
        ));
        assert_eq!(
            context.world.overlay_host(host.stable_id()).unwrap().active,
            None
        );
        assert_eq!(context.world.focused(document), Some(base.stable_id()));
        assert_eq!(
            context.world.pointer_hover(document, 7),
            Some(base.stable_id())
        );
        assert!(
            !context
                .component_lifecycle
                .overlay_activation_tokens
                .contains_key(&host.stable_id())
        );
    }

    #[test]
    fn hidden_handler_replacement_rolls_back_and_requested_overlay_can_retry() {
        for use_display_none in [false, true] {
            let mut context = AppContext::new();
            let document = DocumentId::new(1).unwrap();
            let base = context
                .create_component(document, Button::new("Base"))
                .unwrap();
            let host = context
                .create_component(document, OverlayHost::new())
                .unwrap();
            let requested = context
                .create_component(document, Dialog::new("Requested"))
                .unwrap();
            let replacement = context
                .create_component(document, Dialog::new("Hidden replacement"))
                .unwrap();
            context.append_child(host, requested).unwrap();
            context.append_child(host, replacement).unwrap();
            context.focus_node(document, base.stable_id()).unwrap();
            context
                .set_pointer_hover(document, 7, Some(base.stable_id()))
                .unwrap();
            let mut hidden_style = context
                .world
                .node_style(replacement.stable_id())
                .unwrap()
                .clone();
            let layout = Arc::make_mut(&mut hidden_style.layout);
            if use_display_none {
                layout.display = Some(nana_ui_core::DisplaySpec::None);
            } else {
                layout.hidden = true;
            }
            let calls = Arc::new(AtomicUsize::new(0));
            let observed = Arc::clone(&calls);
            context
                .on(host, move |_host, _event: &crate::OverlayChanged, cx| {
                    if observed.fetch_add(1, AtomicOrdering::Relaxed) == 0 {
                        cx.mutations().set_overlay_host(
                            host.stable_id(),
                            crate::OverlayHostState {
                                active: Some(replacement.stable_id()),
                                restore_focus: None,
                            },
                        );
                        cx.mutations()
                            .set_style(replacement.stable_id(), hidden_style.clone());
                        cx.mutations().request_focus(document, None);
                    }
                })
                .unwrap();

            assert_eq!(
                context.activate_overlay(host, requested),
                Err(FrameworkError::InvalidComponentValue(
                    replacement.stable_id()
                ))
            );
            assert_eq!(
                context.world.overlay_host(host.stable_id()).unwrap().active,
                None
            );
            assert_eq!(context.world.focused(document), Some(base.stable_id()));
            assert_eq!(
                context.world.pointer_hover(document, 7),
                Some(base.stable_id())
            );
            assert!(
                !context
                    .component_lifecycle
                    .overlay_activation_tokens
                    .contains_key(&host.stable_id())
            );

            assert!(context.activate_overlay(host, requested).unwrap());
            assert_eq!(
                context.world.overlay_host(host.stable_id()).unwrap().active,
                Some(requested.stable_id())
            );
        }
    }

    #[test]
    fn exhausted_activation_identity_fails_before_host_or_pointer_state_changes() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let base = context
            .create_component(document, Button::new("Base"))
            .unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let dialog = context
            .create_component(document, Dialog::new("Settings"))
            .unwrap();
        context.append_child(host, dialog).unwrap();
        context
            .set_pointer_hover(document, 42, Some(base.stable_id()))
            .unwrap();
        context.component_lifecycle.next_overlay_activation_token = u64::MAX;

        assert_eq!(
            context.activate_overlay(host, dialog),
            Err(FrameworkError::OverlayActivationTokenExhausted(
                host.stable_id()
            ))
        );
        assert_eq!(
            context.world.overlay_host(host.stable_id()).unwrap().active,
            None
        );
        assert_eq!(
            context.world.pointer_hover(document, 42),
            Some(base.stable_id())
        );
        assert!(
            !context
                .component_lifecycle
                .overlay_activation_tokens
                .contains_key(&host.stable_id())
        );
    }

    #[test]
    fn topmost_escape_and_tab_use_only_active_reachable_overlays() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let menu_host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let menu = context
            .create_component(document, ActionMenu::new().open(true))
            .unwrap();
        let first = context
            .create_component(document, ActionMenuItem::new("First"))
            .unwrap();
        let disabled = context
            .create_component(document, ActionMenuItem::new("Disabled").disabled(true))
            .unwrap();
        context.append_child(menu_host, menu).unwrap();
        context.append_child(menu, first).unwrap();
        context.append_child(menu, disabled).unwrap();
        context.activate_overlay(menu_host, menu).unwrap();

        assert!(
            context
                .route_overlay_key(document, OverlayKey::Tab { reverse: false })
                .unwrap()
        );
        assert_eq!(context.world.focused(document), Some(first.stable_id()));
        assert!(
            context
                .route_overlay_key(document, OverlayKey::Tab { reverse: true })
                .unwrap()
        );
        assert_eq!(context.world.focused(document), Some(first.stable_id()));

        let tooltip_host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let tooltip = context
            .create_component(document, Tooltip::new("Hint"))
            .unwrap();
        context.append_child(tooltip_host, tooltip).unwrap();
        context.activate_overlay(tooltip_host, tooltip).unwrap();
        assert_eq!(
            context.active_runtime_overlay(document).unwrap().kind,
            RuntimeOverlayKind::Tooltip
        );
        assert!(
            context
                .route_overlay_key(document, OverlayKey::Escape)
                .unwrap()
        );
        if context
            .world
            .overlay_host(tooltip_host.stable_id())
            .unwrap()
            .active
            .is_some()
        {
            context.dismiss_overlay(tooltip_host).unwrap();
        }
        assert_eq!(
            context.active_runtime_overlay(document).unwrap().kind,
            RuntimeOverlayKind::Menu
        );
    }

    #[test]
    fn parked_overlay_loses_authority_and_remount_stays_closed_without_a_deadline() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let dialog = context
            .create_component(document, Dialog::new("Parked"))
            .unwrap();
        context.append_child(host, dialog).unwrap();
        context.activate_overlay(host, dialog).unwrap();

        let mut park = MutationQueue::new();
        park.park_subtree(dialog.stable_id());
        context.commit_mutations(park).unwrap();
        assert_eq!(
            context.world.mount_state(dialog.stable_id()),
            Some(MountState::Parked)
        );
        assert!(context.active_runtime_overlay(document).is_none());

        let mut remount = MutationQueue::new();
        remount.insert(host.stable_id(), dialog.stable_id(), None);
        context.commit_mutations(remount).unwrap();
        assert_eq!(
            context.world.mount_state(dialog.stable_id()),
            Some(MountState::Mounted)
        );
        assert!(context.active_runtime_overlay(document).is_none());
        // Parking keeps the fade overlay until its completion deadline; one
        // advance retires it without reopening the dialog.
        assert!(context.next_animation_deadline().is_some());
        context.advance_animations(std::time::Duration::from_millis(500));
        assert!(context.active_runtime_overlay(document).is_none());
        assert_eq!(context.next_animation_deadline(), None);
    }

    #[test]
    fn dismiss_does_not_restore_focus_to_a_disabled_target() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let base = context
            .create_component(document, Button::new("Open"))
            .unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let dialog = context
            .create_component(document, Dialog::new("Settings"))
            .unwrap();
        context.append_child(host, dialog).unwrap();
        context.focus_node(document, base.stable_id()).unwrap();
        context.activate_overlay(host, dialog).unwrap();
        context
            .update_component(base, |button, _cx| button.disabled = true)
            .unwrap();

        context.dismiss_overlay(host).unwrap();

        assert_eq!(context.world.focused(document), None);
    }

    #[test]
    /// A toast never owns focus: it must not move focus when it shows or hides,
    /// and a control hidden while it shows cannot keep it from hiding again.
    fn a_toast_leaves_focus_alone_and_still_hides_after_its_origin_disappears() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let origin = context
            .create_component(document, Button::new("Import"))
            .unwrap();
        let other = context
            .create_component(document, Button::new("Other"))
            .unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let toast = context
            .create_component(document, Toast::new("Working", ToastTone::Info))
            .unwrap();
        context.append_child(host, toast).unwrap();
        context.focus_node(document, origin.stable_id()).unwrap();
        assert!(context.activate_overlay(host, toast).unwrap());
        assert_eq!(context.world.focused(document), Some(origin.stable_id()));

        context
            .update_component(origin, |button, _cx| {
                Arc::make_mut(&mut button.style.layout).hidden = true;
            })
            .unwrap();
        context.focus_node(document, other.stable_id()).unwrap();

        assert!(context.dismiss_overlay(host).unwrap());
        assert_eq!(
            context
                .world()
                .overlay_host(host.stable_id())
                .unwrap()
                .active,
            None
        );
        assert_eq!(context.world.focused(document), Some(other.stable_id()));

        assert!(context.activate_overlay(host, toast).unwrap());
        assert!(context.dismiss_overlay(host).unwrap());
        assert_eq!(context.world.focused(document), Some(other.stable_id()));
    }

    #[test]
    /// A host fills its whole region, so it may only take the pointer while a
    /// modal overlay is up. A toast has to leave the workspace usable.
    fn only_a_modal_overlay_makes_the_host_take_the_pointer() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let toast = context
            .create_component(document, Toast::new("Save failed", ToastTone::Danger))
            .unwrap();
        let menu = context
            .create_component(document, ActionMenu::new().open(true))
            .unwrap();
        context.append_child(host, toast).unwrap();
        context.append_child(host, menu).unwrap();

        let pointer_events = |context: &AppContext| {
            context
                .world()
                .interaction(host.stable_id())
                .unwrap()
                .pointer_events
        };

        assert!(!pointer_events(&context));
        assert!(context.activate_overlay(host, toast).unwrap());
        assert!(!pointer_events(&context));
        assert!(context.activate_overlay(host, menu).unwrap());
        assert!(pointer_events(&context));
        assert!(context.dismiss_overlay(host).unwrap());
        assert!(!pointer_events(&context));
    }

    #[test]
    /// Escape belongs to whatever the toast floats over, not to the toast.
    fn a_toast_does_not_swallow_escape() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let toast = context
            .create_component(document, Toast::new("Save failed", ToastTone::Danger))
            .unwrap();
        context.append_child(host, toast).unwrap();
        context.activate_overlay(host, toast).unwrap();

        assert!(
            !context
                .route_overlay_key(document, OverlayKey::Escape)
                .unwrap()
        );
        assert_eq!(
            context
                .world()
                .overlay_host(host.stable_id())
                .unwrap()
                .active,
            Some(toast.stable_id())
        );
    }

    #[test]
    fn overlay_rejects_unknown_views_and_hidden_hit_candidates() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let unknown = context
            .create_view(
                document,
                NodeKind::Element {
                    tag: "custom-overlay".into(),
                },
                String::from("unknown"),
            )
            .unwrap();
        context.append_child(host, unknown).unwrap();
        assert_eq!(
            context.activate_overlay(host, unknown),
            Err(FrameworkError::ViewType(unknown.stable_id()))
        );

        let menu = context
            .create_component(document, ActionMenu::new().open(true))
            .unwrap();
        let item = context
            .create_component(document, ActionMenuItem::new("Hidden"))
            .unwrap();
        context.append_child(host, menu).unwrap();
        context.append_child(menu, item).unwrap();
        write_layout(
            &mut context,
            menu.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 200.0,
            },
        );
        write_layout(
            &mut context,
            item.stable_id(),
            LayoutBox {
                x: 20.0,
                y: 20.0,
                width: 100.0,
                height: 32.0,
            },
        );
        context.activate_overlay(host, menu).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        context.rebuild_hit_test(document);
        context
            .update_component(item, |item, _cx| {
                Arc::make_mut(&mut item.style.layout).hidden = true;
            })
            .unwrap();

        let decision = context
            .route_overlay_pointer(document, 1, OverlayPointerPhase::PrimaryDown, 30.0, 30.0)
            .unwrap();
        assert_eq!(decision.target, Some(menu.stable_id()));
    }

    #[test]
    fn menu_hit_keeps_z_order_and_pointer_leases_cancel_independently() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let menu = context
            .create_component(document, ActionMenu::new().open(true))
            .unwrap();
        let mut lower_view = ActionMenuItem::new("Lower");
        Arc::make_mut(&mut lower_view.style.layout).z_index = Some(2_000);
        let lower = context.create_component(document, lower_view).unwrap();
        let mut upper_view = ActionMenuItem::new("Upper");
        Arc::make_mut(&mut upper_view.style.layout).z_index = Some(3_000);
        let upper = context.create_component(document, upper_view).unwrap();
        context.append_child(host, menu).unwrap();
        context.append_child(menu, lower).unwrap();
        context.append_child(menu, upper).unwrap();
        for id in [menu.stable_id(), lower.stable_id(), upper.stable_id()] {
            write_layout(
                &mut context,
                id,
                LayoutBox {
                    x: 10.0,
                    y: 10.0,
                    width: 100.0,
                    height: 100.0,
                },
            );
        }
        context.activate_overlay(host, menu).unwrap();
        let work = context.take_system_work();
        context.resolve_styles(&work.style).unwrap();
        context.rebuild_hit_test(document);
        assert_eq!(
            context
                .route_overlay_pointer(document, 1, OverlayPointerPhase::PrimaryDown, 20.0, 20.0,)
                .unwrap()
                .target,
            Some(upper.stable_id())
        );

        context
            .route_overlay_pointer(document, 2, OverlayPointerPhase::PrimaryDown, 20.0, 20.0)
            .unwrap();
        assert!(
            context
                .route_overlay_pointer(document, 1, OverlayPointerPhase::Cancel, 20.0, 20.0)
                .unwrap()
                .prevent_default
        );
        context.dismiss_overlay(host).unwrap();
        assert!(
            !context
                .route_overlay_pointer(document, 1, OverlayPointerPhase::Move, 20.0, 20.0)
                .unwrap()
                .prevent_default
        );
        assert!(
            context
                .route_overlay_pointer(document, 2, OverlayPointerPhase::PrimaryUp, 20.0, 20.0)
                .unwrap()
                .prevent_default
        );
    }

    #[test]
    fn dialog_tab_skips_the_surface_when_controls_exist_and_wraps_them() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let host = context
            .create_component(document, OverlayHost::new())
            .unwrap();
        let dialog = context
            .create_component(
                document,
                Dialog::new("Settings").initial_focus(crate::ModalInitialFocus::Surface),
            )
            .unwrap();
        let first = context
            .create_detached_component(document, Button::new("Cancel"))
            .unwrap();
        let last = context
            .create_detached_component(document, Button::new("Save"))
            .unwrap();
        context
            .set_modal_slots(
                dialog,
                crate::ModalSlots {
                    actions: vec![first.stable_id(), last.stable_id()],
                    ..Default::default()
                },
            )
            .unwrap();
        context.append_child(host, dialog).unwrap();
        context.activate_overlay(host, dialog).unwrap();
        assert_eq!(context.world.focused(document), Some(dialog.stable_id()));

        context
            .route_overlay_key(document, OverlayKey::Tab { reverse: false })
            .unwrap();
        assert_eq!(context.world.focused(document), Some(first.stable_id()));
        context
            .route_overlay_key(document, OverlayKey::Tab { reverse: true })
            .unwrap();
        assert_eq!(context.world.focused(document), Some(last.stable_id()));
        context
            .route_overlay_key(document, OverlayKey::Tab { reverse: false })
            .unwrap();
        assert_eq!(context.world.focused(document), Some(first.stable_id()));
    }

    #[test]
    fn removing_a_lower_overlay_never_restores_focus_outside_the_upper_modal() {
        for despawn in [false, true] {
            let mut context = AppContext::new();
            let document = DocumentId::new(1).unwrap();
            let base = context
                .create_component(document, Button::new("Open"))
                .unwrap();
            let lower_host = context
                .create_component(document, OverlayHost::new())
                .unwrap();
            let lower = context
                .create_component(document, Dialog::new("Lower"))
                .unwrap();
            let upper_host = context
                .create_component(document, OverlayHost::new())
                .unwrap();
            let upper = context
                .create_component(document, Dialog::new("Upper"))
                .unwrap();
            context.append_child(lower_host, lower).unwrap();
            context.append_child(upper_host, upper).unwrap();
            context.focus_node(document, base.stable_id()).unwrap();
            context.activate_overlay(lower_host, lower).unwrap();
            context.activate_overlay(upper_host, upper).unwrap();
            assert_eq!(context.world.focused(document), Some(upper.stable_id()));

            let mut remove = MutationQueue::new();
            if despawn {
                remove.despawn_subtree(lower.stable_id());
            } else {
                remove.park_subtree(lower.stable_id());
            }
            context.commit_mutations(remove).unwrap();

            assert_eq!(context.world.focused(document), Some(upper.stable_id()));
            assert_eq!(
                context
                    .world
                    .overlay_host(lower_host.stable_id())
                    .unwrap()
                    .active,
                None
            );
            assert_eq!(
                context
                    .world
                    .overlay_host(upper_host.stable_id())
                    .unwrap()
                    .restore_focus,
                None
            );
            context.dismiss_overlay(upper_host).unwrap();
            assert_eq!(context.world.focused(document), None);
        }
    }
}
