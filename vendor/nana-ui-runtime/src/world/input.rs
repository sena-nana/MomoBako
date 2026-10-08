//! Document focus and pointer ownership.

use super::*;

impl UiWorld {
    pub fn clear_pointer_interactions(&mut self, document: DocumentId) {
        let affected = self
            .input
            .pointer_hover
            .iter()
            .chain(&self.input.pointer_press)
            .filter_map(|(&(owner, _), &target)| (owner == document).then_some(target))
            .collect::<HashSet<_>>();
        self.input
            .pointer_hover
            .retain(|(owner, _), _| *owner != document);
        self.input
            .pointer_press
            .retain(|(owner, _), _| *owner != document);
        if !affected.is_empty() {
            self.generation = self.generation.wrapping_add(1);
            for target in affected {
                self.mark_interaction_style(target);
            }
        }
    }
}

impl UiWorld {
    pub fn release_pointer_press(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
    ) -> Option<StableNodeId> {
        let previous = self.input.pointer_press.remove(&(document, pointer_id));
        if let Some(previous) = previous {
            self.generation = self.generation.wrapping_add(1);
            self.mark_interaction_style(previous);
        }
        previous
    }
}

impl UiWorld {
    pub fn press_pointer(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        target: StableNodeId,
    ) -> Result<Option<StableNodeId>, UiWorldError> {
        self.validate_pointer_target(document, target)?;
        let previous = self
            .input
            .pointer_press
            .insert((document, pointer_id), target);
        if previous != Some(target) {
            if self.hover_transitions.contains_key(&target) {
                self.mark_hover_paint(target);
            }
            self.cancel_hover_transition(target);
            self.generation = self.generation.wrapping_add(1);
            if let Some(previous) = previous {
                self.mark_interaction_style(previous);
            }
            self.mark_interaction_style(target);
        }
        Ok(previous)
    }
}

impl UiWorld {
    /// Update per-pointer hover authority and invalidate only the old/new
    /// interaction paint. DOM adapters may derive enter/leave paths from the
    /// returned previous target and the retained hierarchy.
    pub fn set_pointer_hover(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        target: Option<StableNodeId>,
    ) -> Result<Option<StableNodeId>, UiWorldError> {
        if let Some(target) = target {
            self.validate_pointer_target(document, target)?;
        }
        let key = (document, pointer_id);
        let old = self.input.pointer_hover.get(&key).copied();
        let paints = [old, target].map(|id| id.map(|id| (id, self.hover_paint(id))));
        let previous = match target {
            Some(target) => self.input.pointer_hover.insert(key, target),
            None => self.input.pointer_hover.remove(&key),
        };
        if previous != target {
            for (id, paint) in paints.into_iter().flatten() {
                self.transition_hover(id, paint);
            }
            self.generation = self.generation.wrapping_add(1);
            if let Some(previous) = previous {
                self.mark_interaction_style(previous);
            }
            if let Some(target) = target {
                self.mark_interaction_style(target);
            }
        }
        Ok(previous)
    }
}

impl UiWorld {
    pub fn pointer_press(&self, document: DocumentId, pointer_id: u64) -> Option<StableNodeId> {
        self.input
            .pointer_press
            .get(&(document, pointer_id))
            .copied()
    }
}

impl UiWorld {
    pub fn pointer_hover(&self, document: DocumentId, pointer_id: u64) -> Option<StableNodeId> {
        self.input
            .pointer_hover
            .get(&(document, pointer_id))
            .copied()
    }
}

impl UiWorld {
    pub fn take_pointer_capture_changes(&mut self) -> Vec<PointerCaptureChange> {
        std::mem::take(&mut self.input.pending_pointer_capture_changes)
    }
}

impl UiWorld {
    pub fn pointer_captures(&self, document: DocumentId) -> Vec<(u64, StableNodeId)> {
        let mut captures = self
            .input
            .pointer_captures
            .iter()
            .filter_map(|(&(owner, pointer_id), &target)| {
                (owner == document).then_some((pointer_id, target))
            })
            .collect::<Vec<_>>();
        captures.sort_unstable_by_key(|(pointer_id, _)| *pointer_id);
        captures
    }
}

impl UiWorld {
    pub fn pointer_capture(&self, document: DocumentId, pointer_id: u64) -> Option<StableNodeId> {
        self.input
            .pointer_captures
            .get(&(document, pointer_id))
            .copied()
    }

    /// Release a capture during host lifecycle cancellation (disconnect,
    /// document replacement, or blur). This is an immediate input-state
    /// transition and records the same capture-change publication used by
    /// mutation-driven release.
    pub fn release_pointer_capture(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
    ) -> Option<StableNodeId> {
        let target = self
            .input
            .pointer_captures
            .remove(&(document, pointer_id))?;
        self.input
            .pending_pointer_capture_changes
            .push(PointerCaptureChange {
                pointer_id,
                target,
                captured: false,
            });
        self.generation = self.generation.wrapping_add(1);
        Some(target)
    }
}

impl UiWorld {
    pub fn event_route(&self, target: StableNodeId) -> Option<EventRoute> {
        if !self.is_mounted(target) {
            return None;
        }
        let mut bubble = Vec::new();
        let mut current = self.parent_id(target);
        while let Some(id) = current {
            bubble.push(id);
            current = self.parent_id(id);
        }
        let mut capture = bubble.clone();
        capture.reverse();
        Some(EventRoute {
            capture,
            target,
            bubble,
        })
    }
}

#[derive(Default)]
pub(super) struct WorldInputState {
    pub(super) focused: HashMap<DocumentId, StableNodeId>,
    /// Documents whose most recent input event came from a pointer.
    ///
    /// Absent means keyboard, which is also what a document that has seen no
    /// input at all reads as: a programmatic `focus_node` should show itself.
    pub(super) pointer_modality: HashSet<DocumentId>,
    /// Documents whose *current* focus arrived while [`Self::pointer_modality`]
    /// held, snapshotted when the focus was written.
    ///
    /// This is the `:focus-visible` heuristic, and it is one bit rather than a
    /// modality machine because only one question is ever asked of it: should
    /// the focused control *show* that it is focused. Clicking a button is the
    /// case where it should not — the user already knows where they clicked,
    /// and every design system that paints focus unconditionally ends up with
    /// buttons that keep a halo after a click.
    ///
    /// It cannot be read from `pointer_press` at focus time, which is what the
    /// first version did: the pointer-down path focuses the hit node *before*
    /// it records the press, so every click looked like keyboard navigation.
    ///
    /// Restoring focus after an overlay closes writes `focused` directly and
    /// leaves this alone, so the restored control inherits the answer the
    /// focus it is returning to had. That is deliberate: dismissing a dialog
    /// with Escape should put a visible focus back where it came from.
    pub(super) focus_from_pointer: HashSet<DocumentId>,
    pub(super) focus_scopes: HashMap<StableNodeId, Option<StableNodeId>>,
    pub(super) pointer_captures: HashMap<(DocumentId, u64), StableNodeId>,
    pub(super) pointer_hover: HashMap<(DocumentId, u64), StableNodeId>,
    pub(super) pointer_press: HashMap<(DocumentId, u64), StableNodeId>,
    pub(super) pending_pointer_capture_changes: Vec<PointerCaptureChange>,
}

impl UiWorld {
    pub(super) fn clear_surface_pointer_interactions(&mut self, root: StableNodeId) {
        let inactive = self.subtree_ids(root).into_iter().collect::<HashSet<_>>();
        let released = self
            .input
            .pointer_captures
            .iter()
            .filter_map(|(&(document, pointer_id), &target)| {
                inactive
                    .contains(&target)
                    .then_some((document, pointer_id, target))
            })
            .collect::<Vec<_>>();
        for (document, pointer_id, target) in released {
            self.input.pointer_captures.remove(&(document, pointer_id));
            self.input
                .pending_pointer_capture_changes
                .push(PointerCaptureChange {
                    pointer_id,
                    target,
                    captured: false,
                });
        }
        self.input
            .pointer_hover
            .retain(|_, target| !inactive.contains(target));
        self.input
            .pointer_press
            .retain(|_, target| !inactive.contains(target));
        for id in inactive {
            self.cancel_hover_transition(id);
        }
    }
}
