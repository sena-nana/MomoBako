//! AppContext lifecycle operations.

use super::*;

impl AppContext {
    /// Atomically attach and order a ListItem's typed direct-child slots.
    /// Every existing child must be named exactly once; arbitrary nested or
    /// duplicate slot identities are rejected before retained state changes.
    pub fn set_list_item_slots(
        &mut self,
        item: Entity<ListItem>,
        slots: ListItemSlots,
    ) -> Result<bool, FrameworkError> {
        self.read(item, |_| ())?;
        let ordered = [slots.leading, slots.content, slots.trailing]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let unique = ordered.iter().copied().collect::<HashSet<_>>();
        if unique.len() != ordered.len() {
            return Err(FrameworkError::InvalidListItemSlots {
                item: item.id,
                slot: None,
            });
        }
        let item_node = self
            .world
            .node(item.id)
            .ok_or(FrameworkError::MissingView(item.id))?;
        if item_node
            .children
            .iter()
            .any(|child| !unique.contains(child))
        {
            return Err(FrameworkError::InvalidListItemSlots {
                item: item.id,
                slot: item_node
                    .children
                    .iter()
                    .find(|child| !unique.contains(child))
                    .copied(),
            });
        }
        for &slot in &ordered {
            let Some(node) = self.world.node(slot) else {
                return Err(FrameworkError::InvalidListItemSlots {
                    item: item.id,
                    slot: Some(slot),
                });
            };
            if node.document != item_node.document
                || node.parent.is_some_and(|parent| parent != item.id)
            {
                return Err(FrameworkError::InvalidListItemSlots {
                    item: item.id,
                    slot: Some(slot),
                });
            }
        }
        let changed =
            item_node.children != ordered || self.read(item, |item| item.slots != slots)?;
        if !changed {
            return Ok(false);
        }
        let item_id = item.id;
        self.update_component(item, |item, cx| {
            item.slots = slots;
            for slot in &ordered {
                cx.mutations().insert(item_id, *slot, None);
            }
        })?;
        Ok(true)
    }

    /// Place the slots a view wrote into a modal surface
    /// (`TypeBehavior::slot_assembler`).
    pub(crate) fn assemble_modal_slots<C: ModalSurface>(
        &mut self,
        modal: Entity<C>,
    ) -> Result<bool, FrameworkError> {
        let slots = self.read(modal, |modal| modal.slots().clone())?;
        self.set_modal_slots(modal, slots)
    }

    /// Atomically validate and attach an EmptyState's application-owned action.
    /// Intrinsic icon and message content remain fields of EmptyState.
    pub fn set_empty_state_action(
        &mut self,
        empty: Entity<EmptyState>,
        action: Option<StableNodeId>,
    ) -> Result<bool, FrameworkError> {
        let current = self.read(empty, |empty| empty.action)?;
        let owned_current = self.validate_feedback_action(empty.id, current, action)?;
        let ordered = action.into_iter().collect::<Vec<_>>();
        let changed = current != action
            || self
                .world
                .node(empty.id)
                .is_some_and(|node| node.children != ordered);
        if !changed {
            return Ok(false);
        }
        let parent = empty.id;
        self.update_component(empty, |empty, cx| {
            empty.action = action;
            if let Some(current) = owned_current
                && Some(current) != action
            {
                cx.mutations().park_subtree(current);
            }
            if let Some(action) = action {
                cx.mutations().insert(parent, action, None);
            }
        })?;
        Ok(true)
    }

    /// Atomically validate and attach a FormField's application-owned control.
    pub fn set_form_field_control(
        &mut self,
        field: Entity<FormField>,
        control: Option<StableNodeId>,
    ) -> Result<bool, FrameworkError> {
        let current = self.read(field, |field| field.control)?;
        let owned_current = self.validate_feedback_action(field.id, current, control)?;
        let ordered = control.into_iter().collect::<Vec<_>>();
        let changed = current != control
            || self
                .world
                .node(field.id)
                .is_some_and(|node| node.children != ordered);
        if !changed {
            return Ok(false);
        }
        let parent = field.id;
        self.update_component(field, |field, cx| {
            field.control = control;
            if let Some(current) = owned_current
                && Some(current) != control
            {
                cx.mutations().park_subtree(current);
            }
            if let Some(control) = control {
                cx.mutations().insert(parent, control, None);
            }
        })?;
        Ok(true)
    }

    /// Atomically replaces a modal's application-owned direct children.
    /// Removed children remain alive and parked so their view state and handlers
    /// can be remounted by identity.
    pub fn set_modal_slots<C: ModalSurface>(
        &mut self,
        modal: Entity<C>,
        slots: ModalSlots,
    ) -> Result<bool, FrameworkError> {
        let current = self.read(modal, |modal| modal.slots().clone())?;
        let current_order = current.ordered();
        let ordered = slots.ordered();
        let unique = ordered.iter().copied().collect::<HashSet<_>>();
        if unique.len() != ordered.len() {
            return Err(FrameworkError::InvalidModalSlots {
                parent: modal.id,
                slot: None,
            });
        }
        let parent_node = self
            .world
            .node(modal.id)
            .ok_or(FrameworkError::MissingView(modal.id))?;
        // The view field is public API, but never trusted as ownership proof.
        // A builder-declared first mount is the only field/tree mismatch allowed.
        if parent_node.children != current_order
            && !(parent_node.children.is_empty() && current_order == ordered)
        {
            return Err(FrameworkError::InvalidModalSlots {
                parent: modal.id,
                slot: parent_node
                    .children
                    .first()
                    .copied()
                    .or(current_order.first().copied()),
            });
        }
        for slot in &ordered {
            let Some(node) = self.world.node(*slot) else {
                return Err(FrameworkError::InvalidModalSlots {
                    parent: modal.id,
                    slot: Some(*slot),
                });
            };
            if *slot == modal.id
                || node.document != parent_node.document
                || node.parent.is_some_and(|owner| owner != modal.id)
                || (node.parent.is_none()
                    && self.world.mount_state(*slot) != Some(crate::MountState::Parked))
            {
                return Err(FrameworkError::InvalidModalSlots {
                    parent: modal.id,
                    slot: Some(*slot),
                });
            }
        }
        if current == slots && parent_node.children == ordered {
            return Ok(false);
        }
        let removed = parent_node
            .children
            .iter()
            .copied()
            .filter(|child| !unique.contains(child))
            .collect::<Vec<_>>();
        let parent = modal.id;
        self.update_component(modal, |modal, cx| {
            *modal.slots_mut() = slots;
            for child in removed {
                cx.mutations().park_subtree(child);
            }
            for child in ordered {
                cx.mutations().insert(parent, child, None);
            }
        })?;
        Ok(true)
    }

    /// Builds the cancel and confirm actions of a [`ConfirmDialog`] and places
    /// its slots, so the common case needs no slot plumbing.
    ///
    /// Slots given to the dialog before (a view's `.body`, `.close_action`,
    /// `.cancel`, `.secondary`, `.confirm`, or the matching builder methods)
    /// are placed as they are; a missing cancel or confirm action is made from
    /// [`crate::ConfirmDialog::cancel_label`] / `confirm_label`. Idempotent:
    /// afterwards it refreshes the label and disabled state of the buttons it
    /// made, never of ones it was given. Returns whether it placed the slots.
    /// A dialog populated with [`Self::set_confirm_slots`] keeps those slots.
    pub fn assemble_confirm_dialog(
        &mut self,
        dialog: Entity<crate::ConfirmDialog>,
    ) -> Result<bool, FrameworkError> {
        let document = self
            .world
            .node(dialog.id)
            .ok_or(FrameworkError::MissingView(dialog.id))?
            .document;
        let (confirm_label, cancel_label, danger, busy, existing, requested) =
            self.read(dialog, |dialog| {
                (
                    Arc::clone(&dialog.confirm_label),
                    Arc::clone(&dialog.cancel_label),
                    dialog.danger,
                    dialog.busy,
                    dialog.confirm_slots().cloned(),
                    dialog.requested_slots().clone(),
                )
            })?;

        if let Some(slots) = existing
            .filter(|slots| self.world.contains(slots.cancel) && self.world.contains(slots.confirm))
        {
            let made = [
                (requested.cancel.is_none(), slots.cancel, &cancel_label),
                (requested.confirm.is_none(), slots.confirm, &confirm_label),
            ];
            for (made, button, label) in made {
                if made {
                    self.update_component(
                        Entity::<crate::Button>::from_stable_id(button),
                        |button, _| {
                            button.label = label.to_string();
                            button.disabled = busy;
                        },
                    )?;
                }
            }
            return Ok(false);
        }

        let cancel = match requested.cancel {
            Some(cancel) => cancel,
            None => self
                .create_detached_component(
                    document,
                    crate::Button::new(cancel_label.as_ref())
                        .kind(nana_ui_core::ButtonKind::Ghost)
                        .disabled(busy),
                )?
                .stable_id(),
        };
        let confirm = match requested.confirm {
            Some(confirm) => confirm,
            None => self
                .create_detached_component(
                    document,
                    crate::Button::new(confirm_label.as_ref())
                        .kind(if danger {
                            nana_ui_core::ButtonKind::Danger
                        } else {
                            nana_ui_core::ButtonKind::Primary
                        })
                        .disabled(busy),
                )?
                .stable_id(),
        };
        self.set_confirm_slots(
            dialog,
            crate::ConfirmSlots {
                body: requested.body,
                close_action: requested.close_action,
                cancel,
                secondary: requested.secondary,
                confirm,
            },
        )?;
        Ok(true)
    }

    pub fn set_confirm_slots(
        &mut self,
        confirm: Entity<crate::ConfirmDialog>,
        slots: crate::ConfirmSlots,
    ) -> Result<bool, FrameworkError> {
        let modal_slots = slots.modal_slots();
        let current = self.read(confirm, |confirm| confirm.slots().clone())?;
        let ordered = modal_slots.ordered();
        let unique = ordered.iter().copied().collect::<HashSet<_>>();
        let parent_node = self
            .world
            .node(confirm.id)
            .ok_or(FrameworkError::MissingView(confirm.id))?;
        if unique.len() != ordered.len()
            || (parent_node.children != current.ordered()
                && !(parent_node.children.is_empty() && current.ordered() == ordered))
        {
            return Err(FrameworkError::InvalidModalSlots {
                parent: confirm.id,
                slot: None,
            });
        }
        for slot in &ordered {
            let node = self
                .world
                .node(*slot)
                .ok_or(FrameworkError::InvalidModalSlots {
                    parent: confirm.id,
                    slot: Some(*slot),
                })?;
            if node.document != parent_node.document
                || node.parent.is_some_and(|owner| owner != confirm.id)
                || (node.parent.is_none()
                    && self.world.mount_state(*slot) != Some(crate::MountState::Parked))
            {
                return Err(FrameworkError::InvalidModalSlots {
                    parent: confirm.id,
                    slot: Some(*slot),
                });
            }
        }
        let typed_changed =
            self.read(confirm, |confirm| confirm.confirm_slots() != Some(&slots))?;
        if current == modal_slots && parent_node.children == ordered && !typed_changed {
            return Ok(false);
        }
        let removed = parent_node
            .children
            .iter()
            .copied()
            .filter(|child| !unique.contains(child))
            .collect::<Vec<_>>();
        let parent = confirm.id;
        self.update_component(confirm, |confirm, cx| {
            *confirm.slots_mut() = modal_slots;
            confirm.set_confirm_slots_state(slots);
            for child in removed {
                cx.mutations().park_subtree(child);
            }
            for child in ordered {
                cx.mutations().insert(parent, child, None);
            }
        })?;
        Ok(true)
    }

    pub fn set_confirm_state(
        &mut self,
        confirm: Entity<crate::ConfirmDialog>,
        busy: bool,
        danger: bool,
    ) -> Result<bool, FrameworkError> {
        if self.read(confirm, |confirm| {
            confirm.busy == busy && confirm.danger == danger
        })? {
            return Ok(false);
        }
        let node = self
            .world
            .node(confirm.id)
            .ok_or(FrameworkError::MissingView(confirm.id))?;
        let document = node.document;
        let action_roots = self.read(confirm, |confirm| {
            confirm
                .slots()
                .close_action
                .into_iter()
                .chain(confirm.slots().actions.iter().copied())
                .collect::<HashSet<_>>()
        })?;
        let release_focus = busy
            && self.world.focused(document).is_some_and(|id| {
                action_roots
                    .iter()
                    .any(|root| self.overlay_descendant(*root, id))
            });
        let root = confirm.id;
        self.update_component(confirm, |confirm, cx| {
            confirm.busy = busy;
            confirm.danger = danger;
            if release_focus {
                cx.mutations().request_focus(document, Some(root));
            }
        })?;
        Ok(true)
    }

    /// Atomically validate and attach a LabeledValue's optional action child.
    /// The child retains its own activation handler; the summary never becomes
    /// an implicit action target.
    pub fn set_labeled_value_action(
        &mut self,
        summary: Entity<LabeledValue>,
        action: Option<StableNodeId>,
    ) -> Result<bool, FrameworkError> {
        let current = self.read(summary, |summary| summary.action)?;
        let owned_current = self.validate_feedback_action(summary.id, current, action)?;
        let ordered = action.into_iter().collect::<Vec<_>>();
        let changed = current != action
            || self
                .world
                .node(summary.id)
                .is_some_and(|node| node.children != ordered);
        if !changed {
            return Ok(false);
        }
        let parent = summary.id;
        self.update_component(summary, |summary, cx| {
            summary.action = action;
            if let Some(current) = owned_current
                && Some(current) != action
            {
                cx.mutations().park_subtree(current);
            }
            if let Some(action) = action {
                cx.mutations().insert(parent, action, None);
            }
        })?;
        Ok(true)
    }

    pub(super) fn validate_feedback_action(
        &self,
        parent: StableNodeId,
        current: Option<StableNodeId>,
        action: Option<StableNodeId>,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        let parent_node = self
            .world
            .node(parent)
            .ok_or(FrameworkError::MissingView(parent))?;
        if parent_node.children.len() > 1 {
            return Err(FrameworkError::InvalidFeedbackSlots {
                parent,
                slot: parent_node.children.get(1).copied(),
            });
        }
        let owned_current = parent_node.children.first().copied();
        match (current, owned_current) {
            (None, None) => {}
            (Some(declared), Some(owned)) if declared == owned => {}
            // A builder may declare one detached action before its first
            // explicit mount. Only the same requested identity may complete
            // that declaration; it is never treated as an owned child to park.
            (Some(declared), None) if action == Some(declared) => {}
            (declared, owned) => {
                return Err(FrameworkError::InvalidFeedbackSlots {
                    parent,
                    slot: declared.or(owned),
                });
            }
        }
        if let Some(owned) = owned_current {
            let node = self
                .world
                .node(owned)
                .ok_or(FrameworkError::InvalidFeedbackSlots {
                    parent,
                    slot: Some(owned),
                })?;
            if node.document != parent_node.document || node.parent != Some(parent) {
                return Err(FrameworkError::InvalidFeedbackSlots {
                    parent,
                    slot: Some(owned),
                });
            }
        }
        if let Some(slot) = action {
            let Some(node) = self.world.node(slot) else {
                return Err(FrameworkError::InvalidFeedbackSlots {
                    parent,
                    slot: Some(slot),
                });
            };
            if slot == parent
                || node.document != parent_node.document
                || node.parent.is_some_and(|owner| owner != parent)
            {
                return Err(FrameworkError::InvalidFeedbackSlots {
                    parent,
                    slot: Some(slot),
                });
            }
        }
        Ok(owned_current)
    }

    /// Build the boxes a [`crate::PaneTree`] needs, and put the host's content
    /// inside them.
    ///
    /// The tree owns these nodes, which is the whole point: a number written
    /// onto a node the host also owns is overwritten by that node's own
    /// projection in the same pass, and that is why the split ratio never did
    /// anything. Keyed by the tree's own split / pane ids so a reshape reuses
    /// the boxes it still wants and despawns the rest.
    fn sync_pane_tree(&mut self, id: StableNodeId) -> Result<(), FrameworkError> {
        let root = self.read(Entity::<crate::PaneTree>::from_stable_id(id), |tree| {
            tree.root.clone()
        })?;
        let document = self
            .world
            .node(id)
            .ok_or(FrameworkError::MissingView(id))?
            .document;
        let plan = root.slot_plan();

        let mut owned = self
            .component_lifecycle
            .pane_tree_slots
            .remove(&id)
            .unwrap_or_default();
        let mut slots: HashMap<Arc<str>, StableNodeId> = HashMap::new();
        let mut mutations = MutationQueue::new();
        let mut created: Vec<(StableNodeId, crate::PaneSlot)> = Vec::new();

        for entry in &plan {
            let slot = match owned.remove(&entry.key) {
                Some(existing) if self.world.node(existing).is_some() => existing,
                _ => {
                    let slot = self.allocate_id();
                    let view = crate::PaneSlot::new(entry.style.clone());
                    mutations.create(slot, document, view.node_kind());
                    created.push((slot, view));
                    slot
                }
            };
            slots.insert(Arc::clone(&entry.key), slot);
        }
        // Whatever the new shape no longer names. Content the host owns is
        // re-parented below before this runs, so despawning a box never takes
        // a host node with it.
        let stale = owned.into_values().collect::<Vec<_>>();

        // Only what differs. This runs on every reprojection of every pane
        // tree, and an unconditional `insert` per box would dirty an idle
        // frame — `idle_project_does_not_dirty` is what says so.
        for entry in &plan {
            let slot = slots[&entry.key];
            let parent = match &entry.parent {
                Some(parent) => slots[parent],
                None => id,
            };
            if self.world.parent_id(slot) != Some(parent) {
                mutations.insert(parent, slot, None);
            }
            // In the same commit as the box itself, so a tree is laid out
            // correctly on the frame it appears rather than one frame later.
            // The view below carries the same value, so its own projection
            // agrees and changes nothing.
            if self.world.node_style(slot) != Some(&entry.style) {
                mutations.set_style(slot, entry.style.clone());
            }
            if let Some(content) = entry.content.filter(|c| self.world.node(*c).is_some())
                && self.world.parent_id(content) != Some(slot)
            {
                mutations.insert(slot, content, None);
            }
        }
        for slot in stale {
            mutations.despawn_subtree(slot);
        }
        if mutations.is_empty() {
            self.component_lifecycle.pane_tree_slots.insert(id, slots);
            return Ok(());
        }
        self.world.commit(mutations)?;
        for (slot, view) in created {
            self.install_view(slot, view);
        }
        for entry in &plan {
            let slot = slots[&entry.key];
            let style = entry.style.clone();
            if self.read(Entity::<crate::PaneSlot>::from_stable_id(slot), |pane| {
                pane.style == style
            })? {
                continue;
            }
            self.update_component(
                Entity::<crate::PaneSlot>::from_stable_id(slot),
                |pane, _| {
                    pane.style = style;
                },
            )?;
        }
        self.component_lifecycle.pane_tree_slots.insert(id, slots);
        Ok(())
    }

    pub(super) fn sync_component_lifecycle(
        &mut self,
        id: StableNodeId,
    ) -> Result<(), FrameworkError> {
        if let Some(sync) = self.behavior(id).and_then(|b| b.lifecycle) {
            sync(self, id)?;
        }
        self.sync_hover_card_focus(id);
        self.sync_sidebar_section_body_port(id);
        let tooltip = self
            .views
            .get(&id)
            .and_then(|view| view.downcast_ref::<IconButton>())
            .and_then(|button| (!button.disabled).then(|| button.tooltip.clone()).flatten());
        match (tooltip, self.component_lifecycle.tooltips.get(&id).copied()) {
            (Some(configured), None) => {
                let document = self
                    .world
                    .node(id)
                    .ok_or(FrameworkError::MissingView(id))?
                    .document;
                let overlay = self.allocate_id();
                let tooltip =
                    Tooltip::with_config(Arc::clone(&configured.label), configured.config);
                let mut mutations = MutationQueue::new();
                mutations.create(overlay, document, tooltip.node_kind());
                tooltip.project(overlay, &self.world, &mut mutations);
                mutations.insert(id, overlay, None);
                mutations.set_overlay_host(id, crate::OverlayHostState::default());
                self.world.commit(mutations)?;
                self.install_view(overlay, tooltip);
                self.component_lifecycle.tooltips.insert(
                    id,
                    TooltipLifecycle {
                        overlay,
                        show_at: None,
                        open: false,
                    },
                );
            }
            (Some(configured), Some(existing)) => {
                self.update_component(
                    Entity::<Tooltip>::from_stable_id(existing.overlay),
                    |tooltip, _| {
                        if tooltip.label != configured.label || tooltip.config != configured.config
                        {
                            *tooltip = Tooltip::with_config(
                                Arc::clone(&configured.label),
                                configured.config,
                            );
                        }
                    },
                )?;
            }
            (None, Some(existing)) => {
                let mut mutations = MutationQueue::new();
                mutations.set_overlay_host(id, crate::OverlayHostState::default());
                mutations.despawn_subtree(existing.overlay);
                self.world.commit(mutations)?;
                self.views.remove(&existing.overlay);
                self.component_lifecycle.tooltips.remove(&id);
            }
            (None, None) => {}
        }

        let desired_loading = self.views.get(&id).and_then(|view| {
            if view
                .downcast_ref::<Button>()
                .is_some_and(|button| button.loading)
            {
                Some(LoadingComponent::Button)
            } else if view
                .downcast_ref::<Switch>()
                .is_some_and(|switch| switch.loading)
            {
                Some(LoadingComponent::Switch)
            } else if view
                .downcast_ref::<crate::Card>()
                .is_some_and(|card| card.loading)
            {
                Some(LoadingComponent::Card)
            } else {
                None
            }
        });
        match desired_loading {
            Some(kind) => {
                self.component_lifecycle.loading.insert(id, kind);
                if self.world.is_mounted(id)
                    && let Some(spec) = crate::loading_animation(id, self.component_lifecycle.now)
                    && !self.world.animation_is_active(spec.id)
                {
                    let mut mutations = MutationQueue::new();
                    mutations.start_animation(spec);
                    self.world.commit(mutations)?;
                }
            }
            None => {
                self.component_lifecycle.loading.remove(&id);
                if let Some(spec) = crate::loading_animation(id, Duration::ZERO)
                    && self.world.animation_is_active(spec.id)
                {
                    let mut mutations = MutationQueue::new();
                    mutations.stop_animation(spec.id);
                    self.world.commit(mutations)?;
                }
            }
        }

        Ok(())
    }

    pub(super) fn sync_sidebar_section_body_port(&mut self, id: StableNodeId) {
        let Some(body) = self
            .views
            .get(&id)
            .and_then(|view| view.downcast_ref::<SidebarSection>())
            .and_then(|section| section.body)
        else {
            return;
        };
        let Some(style) = self.world.node_style(body).cloned() else {
            return;
        };
        let Some(list) = self
            .views
            .get_mut(&body)
            .and_then(|view| view.downcast_mut::<List>())
        else {
            return;
        };
        if list.style != style {
            list.style = style;
        }
    }

    pub(super) fn retained_subtree(&self, root: StableNodeId) -> Vec<StableNodeId> {
        let mut subtree = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.world.node(id) else {
                continue;
            };
            stack.extend(node.children.iter().rev().copied());
            subtree.push(id);
        }
        subtree
    }

    /// Drops the interaction state a parked component must not keep. Returns
    /// whether that changed the component's view in place, which leaves its
    /// projection for the caller to refresh.
    pub(super) fn suspend_component_lifecycle(&mut self, id: StableNodeId) -> bool {
        let mut changed = false;
        #[cfg(feature = "rich-text")]
        {
            self.component_lifecycle
                .rich_text_presses
                .retain(|_, target| *target != id);
            if let Some(markdown) = self
                .views
                .get(&id)
                .and_then(|view| view.downcast_ref::<crate::NativeMarkdown>())
            {
                markdown.clear_selection();
            }
            if let Some(text) = self
                .views
                .get(&id)
                .and_then(|view| view.downcast_ref::<crate::SelectableRichText>())
            {
                text.clear_selection();
            }
        }
        self.component_lifecycle
            .text_area_resizes
            .retain(|_, target| *target != id);
        if let Some(area) = self
            .views
            .get_mut(&id)
            .and_then(|view| view.downcast_mut::<TextArea>())
            && let Some(drag) = area.resize_drag.take()
        {
            area.resized_height = drag.previous_height;
            changed = true;
        }
        #[cfg(feature = "image-viewer")]
        if let Some(viewer) = self
            .views
            .get_mut(&id)
            .and_then(|view| view.downcast_mut::<crate::ImageViewer>())
            && viewer.dragging.take().is_some()
        {
            changed = true;
        }
        #[cfg(feature = "charts")]
        if let Some(view) = self.views.get_mut(&id) {
            if let Some(chart) = view.downcast_mut::<crate::DonutChart>() {
                changed |= chart.active.take().is_some();
            } else if let Some(chart) = view.downcast_mut::<crate::TimeSeriesChart>() {
                changed |= chart.active.take().is_some();
            }
        }
        if let Some(button) = self
            .views
            .get_mut(&id)
            .and_then(|view| view.downcast_mut::<IconButton>())
            && std::mem::take(&mut button.tooltip_open)
        {
            changed = true;
        }
        if let Some(tooltip) = self.component_lifecycle.tooltips.get_mut(&id) {
            tooltip.show_at = None;
            tooltip.open = false;
        }
        changed
    }

    pub(super) fn resume_component_lifecycle(
        &mut self,
        id: StableNodeId,
    ) -> Result<(), FrameworkError> {
        if self.world.is_mounted(id)
            && self.component_lifecycle.loading.contains_key(&id)
            && let Some(spec) = crate::loading_animation(id, self.component_lifecycle.now)
            && !self.world.animation_is_active(spec.id)
        {
            let mut mutations = MutationQueue::new();
            mutations.start_animation(spec);
            self.world.commit(mutations)?;
        }
        // Parking cancels a component's own timeline; remounting restarts it.
        // Projections only start timelines and surface motion for mounted
        // nodes, and a remount may never project again on its own.
        if self.world.is_mounted(id) {
            if self
                .views
                .get(&id)
                .is_some_and(|view| super::reprojects_on_mount(view.as_ref().type_id()))
            {
                self.reproject_view(id, super::Projection::Always)?;
            }
            let mut mutations = MutationQueue::new();
            if self
                .views
                .get(&id)
                .is_some_and(|view| view.is::<crate::Skeleton>())
                && let Some(spec) =
                    crate::Skeleton::pulse_animation(id, self.component_lifecycle.now)
                && !self.world.animation_is_active(spec.id)
            {
                mutations.start_animation_with_playback(spec, crate::Skeleton::PULSE_PLAYBACK);
            }
            if self
                .views
                .get(&id)
                .is_some_and(|view| view.is::<crate::Spinner>())
                && let Some(spec) = crate::Spinner::spin_animation(id, self.component_lifecycle.now)
                && !self.world.animation_is_active(spec.id)
            {
                mutations.start_animation_with_playback(spec, crate::Spinner::SPIN_PLAYBACK);
            }
            if !mutations.is_empty() {
                self.world.commit(mutations)?;
            }
        }
        Ok(())
    }

    pub(super) fn enter_tooltip(
        &mut self,
        target: StableNodeId,
        now: Duration,
    ) -> Result<(), FrameworkError> {
        let Some(button) = self
            .views
            .get(&target)
            .and_then(|view| view.downcast_ref::<IconButton>())
        else {
            return Ok(());
        };
        let Some(configured) = button.tooltip.as_ref() else {
            return Ok(());
        };
        let Some(lifecycle) = self.component_lifecycle.tooltips.get_mut(&target) else {
            return Ok(());
        };
        lifecycle.show_at = now.checked_add(Duration::from_millis(configured.config.delay_ms));
        if lifecycle.show_at == Some(now) {
            self.open_tooltip(target)?;
        }
        Ok(())
    }

    pub(super) fn leave_tooltip(&mut self, target: StableNodeId) -> Result<(), FrameworkError> {
        let Some(lifecycle) = self.component_lifecycle.tooltips.get_mut(&target) else {
            return Ok(());
        };
        lifecycle.show_at = None;
        if !lifecycle.open {
            return Ok(());
        }
        lifecycle.open = false;
        self.update_component(
            Entity::<IconButton>::from_stable_id(target),
            |button, cx| {
                button.tooltip_open = false;
                cx.mutations()
                    .set_overlay_host(target, crate::OverlayHostState::default());
            },
        )?;
        Ok(())
    }

    pub(super) fn open_tooltip(&mut self, target: StableNodeId) -> Result<bool, FrameworkError> {
        let Some(lifecycle) = self.component_lifecycle.tooltips.get(&target).copied() else {
            return Ok(false);
        };
        if lifecycle.open {
            return Ok(false);
        }
        self.position_tooltip(target, lifecycle.overlay)?;
        if let Some(lifecycle) = self.component_lifecycle.tooltips.get_mut(&target) {
            lifecycle.show_at = None;
            lifecycle.open = true;
        }
        self.update_component(
            Entity::<IconButton>::from_stable_id(target),
            |button, cx| {
                button.tooltip_open = true;
                cx.mutations().set_overlay_host(
                    target,
                    crate::OverlayHostState {
                        active: Some(lifecycle.overlay),
                        restore_focus: None,
                    },
                );
            },
        )?;
        Ok(true)
    }

    /// Whether a pointer press should leave the current editor and its selection
    /// untouched. Activation still proceeds through the normal input pipeline.
    pub fn preserves_hover_card_editor_focus(&self, target: StableNodeId) -> bool {
        let Some(root) = self.enclosing_hover_card(target) else {
            return false;
        };
        let Some(document) = self.world.document_of(root) else {
            return false;
        };
        self.component_lifecycle
            .hover_cards
            .get(&root)
            .is_some_and(|state| state.preserve_editor_focus)
            // Editor identity still holds while IME owns the edit commands.
            // Do not use focused_text_editor, which intentionally excludes IME.
            && self
                .focused_plain_editor_node(document)
                .is_some_and(|editor| !self.overlay_descendant(root, editor))
    }

    fn sync_hover_card_focus(&mut self, id: StableNodeId) {
        let Some(card) = self
            .views
            .get(&id)
            .and_then(|view| view.downcast_ref::<crate::HoverCard>())
        else {
            return;
        };
        let open = card.open;
        let preserve = card.preserve_editor_focus;
        let focused = self
            .world
            .document_of(id)
            .and_then(|doc| self.world.focused(doc))
            .filter(|focus| !self.overlay_descendant(id, *focus));
        let lifecycle = self.component_lifecycle.hover_cards.entry(id).or_default();
        if open && !lifecycle.open && preserve {
            lifecycle.restore_focus = focused;
        }
        if !open {
            lifecycle.restore_focus = None;
            if lifecycle.open {
                lifecycle.show_at = None;
                lifecycle.close_at = None;
            }
        }
        lifecycle.open = open;
        lifecycle.preserve_editor_focus = preserve;
    }

    pub(super) fn remember_hover_card_entry(
        &mut self,
        document: DocumentId,
        previous: StableNodeId,
        focus: StableNodeId,
    ) {
        let Some(root) = self.enclosing_hover_card(focus) else {
            return;
        };
        if self.world.document_of(previous) != Some(document)
            || self.overlay_descendant(root, previous)
        {
            return;
        }
        if let Some(state) = self.component_lifecycle.hover_cards.get_mut(&root)
            && state.open
            && state.preserve_editor_focus
        {
            state.restore_focus = Some(previous);
        }
    }

    /// Snapshot the owner before mutations can detach it or its focused child.
    /// A hidden refresh action is the same focus loss as dismissing its card.
    pub(super) fn hover_card_focus_before_commit(
        &self,
        mutations: &MutationQueue,
        previous_focus: &HashMap<DocumentId, StableNodeId>,
    ) -> Vec<(DocumentId, StableNodeId, StableNodeId, Option<StableNodeId>)> {
        previous_focus
            .iter()
            .filter_map(|(&document, &focused)| {
                let root = self.enclosing_hover_card(focused)?;
                let state = self.component_lifecycle.hover_cards.get(&root)?;
                if !state.open || !state.preserve_editor_focus {
                    return None;
                }
                let explicit_focus = mutations.as_slice().iter().any(|mutation| match mutation {
                    crate::UiMutation::RequestFocus {
                        document: requested,
                        ..
                    } => *requested == document,
                    crate::UiMutation::RestoreFocusWithin { root } => {
                        self.world.document_of(*root) == Some(document)
                    }
                    _ => false,
                });
                (!explicit_focus).then_some((document, root, focused, state.restore_focus))
            })
            .collect()
    }

    pub(super) fn hover_card_focus_reachable(
        &self,
        root: StableNodeId,
        focused: StableNodeId,
    ) -> bool {
        matches!(
            self.world.standard_visual(root),
            Some(crate::StandardVisual::MenuSurface { open: true, .. })
        ) && self.world.is_overlay_reachable(focused)
            && self
                .world
                .interaction(focused)
                .is_some_and(|interaction| interaction.focusable)
    }

    /// Pointer entered a hover-card subtree: dismiss any other open card and
    /// schedule the delayed open.
    pub(super) fn enter_hover_card(
        &mut self,
        target: StableNodeId,
        now: Duration,
    ) -> Result<(), FrameworkError> {
        let delay = self
            .read(Entity::<crate::HoverCard>::from_stable_id(target), |card| {
                card.open_delay_ms
            })
            .unwrap_or_default();
        let others = self
            .component_lifecycle
            .hover_cards
            .iter()
            .filter(|(id, lifecycle)| lifecycle.open && **id != target)
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in others {
            self.close_hover_card(id)?;
        }
        let Some(lifecycle) = self.component_lifecycle.hover_cards.get_mut(&target) else {
            return Ok(());
        };
        lifecycle.close_at = None;
        if delay == 0 {
            lifecycle.show_at = None;
            self.open_hover_card(target)?;
            return Ok(());
        }
        lifecycle.show_at = now.checked_add(Duration::from_millis(delay));
        Ok(())
    }

    /// Pointer left the hover-card subtree: cancel a pending open and start
    /// the grace close for an open card.
    pub(super) fn leave_hover_card(
        &mut self,
        target: StableNodeId,
        now: Duration,
    ) -> Result<(), FrameworkError> {
        let close_delay = self
            .read(Entity::<crate::HoverCard>::from_stable_id(target), |card| {
                card.close_delay_ms
            })
            .unwrap_or_default();
        let Some(lifecycle) = self.component_lifecycle.hover_cards.get_mut(&target) else {
            return Ok(());
        };
        lifecycle.show_at = None;
        if !lifecycle.open {
            return Ok(());
        }
        lifecycle.close_at = now.checked_add(Duration::from_millis(close_delay));
        Ok(())
    }

    pub(super) fn open_hover_card(&mut self, target: StableNodeId) -> Result<bool, FrameworkError> {
        if self
            .component_lifecycle
            .hover_cards
            .get(&target)
            .is_none_or(|lifecycle| lifecycle.open)
        {
            return Ok(false);
        }
        self.update_component(
            Entity::<crate::HoverCard>::from_stable_id(target),
            |card, _| {
                card.open = true;
            },
        )?;
        if let Some(lifecycle) = self.component_lifecycle.hover_cards.get_mut(&target) {
            lifecycle.open = true;
            lifecycle.show_at = None;
        }
        Ok(true)
    }

    pub(super) fn close_hover_card(
        &mut self,
        target: StableNodeId,
    ) -> Result<bool, FrameworkError> {
        if !self
            .component_lifecycle
            .hover_cards
            .get(&target)
            .is_some_and(|lifecycle| lifecycle.open)
        {
            return Ok(false);
        }
        self.update_component(
            Entity::<crate::HoverCard>::from_stable_id(target),
            |card, _| {
                card.open = false;
            },
        )?;
        if let Some(lifecycle) = self.component_lifecycle.hover_cards.get_mut(&target) {
            lifecycle.open = false;
            lifecycle.show_at = None;
            lifecycle.close_at = None;
        }
        Ok(true)
    }

    /// Drop lifecycle entries whose component is gone.
    pub(super) fn sweep_hover_cards(&mut self) {
        self.component_lifecycle
            .hover_cards
            .retain(|target, _| self.views.contains_key(target));
    }

    pub(super) fn position_open_tooltips(
        &mut self,
        document: DocumentId,
    ) -> Result<(), FrameworkError> {
        let targets = self
            .component_lifecycle
            .tooltips
            .iter()
            .filter_map(|(&target, tooltip)| {
                (tooltip.open
                    && self
                        .world
                        .node(target)
                        .is_some_and(|node| node.document == document))
                .then_some((target, tooltip.overlay))
            })
            .collect::<Vec<_>>();
        for (target, overlay) in targets {
            self.position_tooltip(target, overlay)?;
        }
        #[cfg(feature = "charts")]
        self.position_chart_tooltips(document)?;
        Ok(())
    }

    pub(super) fn pointer_location_on(&self, target: StableNodeId) -> Option<(f32, f32)> {
        let document = self.world.document_of(target)?;
        self.component_lifecycle.pointer_positions.iter().find_map(
            |(&(owner, pointer_id), &position)| {
                (owner == document && self.world.pointer_hover(owner, pointer_id) == Some(target))
                    .then_some(position)
            },
        )
    }

    pub(super) fn reposition_follow_cursor_tooltip(
        &mut self,
        target: StableNodeId,
    ) -> Result<(), FrameworkError> {
        let Some(lifecycle) = self.component_lifecycle.tooltips.get(&target).copied() else {
            return Ok(());
        };
        if !lifecycle.open {
            return Ok(());
        }
        let follows = self
            .read(
                Entity::<Tooltip>::from_stable_id(lifecycle.overlay),
                |tooltip| tooltip.config.placement == TooltipPlacement::FollowCursor,
            )
            .unwrap_or(false);
        if follows {
            self.position_tooltip(target, lifecycle.overlay)?;
        }
        Ok(())
    }

    pub(super) fn position_tooltip(
        &mut self,
        target: StableNodeId,
        overlay: StableNodeId,
    ) -> Result<(), FrameworkError> {
        let anchor = self
            .world
            .viewport_layout_box(target)
            .ok_or(FrameworkError::MissingView(target))?;
        let document = self
            .world
            .node(target)
            .ok_or(FrameworkError::MissingView(target))?
            .document;
        let Some(viewport) = self.world.document_viewport(document) else {
            return Ok(());
        };
        let metrics = self.world.text_metrics(overlay).unwrap_or_default();
        let (config, mut style) = self
            .read(Entity::<Tooltip>::from_stable_id(overlay), |tooltip| {
                (tooltip.config, tooltip.style.clone())
            })?;
        let padding_x = TooltipConfig::PADDING_X;
        let padding_y = TooltipConfig::PADDING_Y;
        let desired_width = (metrics.width + padding_x * 2.0 + 2.0)
            .min(config.max_width)
            .max(0.0);
        let height = (metrics.height + padding_y * 2.0 + 2.0).max(0.0);
        let padding = config.viewport_padding.max(0.0);
        let left_available = (anchor.x - config.gap - padding).max(0.0);
        let right_available =
            (viewport.width - padding - (anchor.x + anchor.width + config.gap)).max(0.0);
        let cursor = self.pointer_location_on(target).unwrap_or((
            anchor.x + anchor.width / 2.0,
            anchor.y + anchor.height / 2.0,
        ));
        let (width, horizontal_side) = match config.placement {
            TooltipPlacement::Left => {
                let side = if left_available >= desired_width
                    || (left_available >= right_available && right_available < desired_width)
                {
                    TooltipPlacement::Left
                } else {
                    TooltipPlacement::Right
                };
                let available = if side == TooltipPlacement::Left {
                    left_available
                } else {
                    right_available
                };
                (desired_width.min(available), Some(side))
            }
            TooltipPlacement::Right => {
                let side = if right_available >= desired_width
                    || (right_available >= left_available && left_available < desired_width)
                {
                    TooltipPlacement::Right
                } else {
                    TooltipPlacement::Left
                };
                let available = if side == TooltipPlacement::Left {
                    left_available
                } else {
                    right_available
                };
                (desired_width.min(available), Some(side))
            }
            TooltipPlacement::Top | TooltipPlacement::Bottom | TooltipPlacement::FollowCursor => {
                (desired_width, None)
            }
        };
        let top = (
            anchor.x + (anchor.width - width) / 2.0,
            anchor.y - config.gap - height,
        );
        let right = (
            anchor.x + anchor.width + config.gap,
            anchor.y + (anchor.height - height) / 2.0,
        );
        let bottom = (
            anchor.x + (anchor.width - width) / 2.0,
            anchor.y + anchor.height + config.gap,
        );
        let left = (
            anchor.x - config.gap - width,
            anchor.y + (anchor.height - height) / 2.0,
        );
        let follow_above = (cursor.0, cursor.1 - height - config.gap);
        let follow_below = (cursor.0, cursor.1 + config.gap);
        let fits = |(x, y): (f32, f32)| {
            x >= padding
                && y >= padding
                && x + width <= viewport.width - padding
                && y + height <= viewport.height - padding
        };
        let preferred = match config.placement {
            TooltipPlacement::Top => top,
            TooltipPlacement::Right => right,
            TooltipPlacement::Bottom => bottom,
            TooltipPlacement::Left => left,
            TooltipPlacement::FollowCursor => follow_above,
        };
        let opposite = match config.placement {
            TooltipPlacement::Top => bottom,
            TooltipPlacement::Right => left,
            TooltipPlacement::Bottom => top,
            TooltipPlacement::Left => right,
            TooltipPlacement::FollowCursor => follow_below,
        };
        let (x, y) = if let Some(side) = horizontal_side {
            match side {
                TooltipPlacement::Left => left,
                TooltipPlacement::Right => right,
                TooltipPlacement::Top
                | TooltipPlacement::Bottom
                | TooltipPlacement::FollowCursor => unreachable!(),
            }
        } else if fits(preferred) || !fits(opposite) {
            preferred
        } else {
            opposite
        };
        let max_x = (viewport.width - padding - width).max(padding);
        let max_y = (viewport.height - padding - height).max(padding);
        let layout = Arc::make_mut(&mut style.layout);
        layout.offset_left = Some(LengthSpec::Px(x.clamp(padding, max_x)));
        layout.offset_top = Some(LengthSpec::Px(y.clamp(padding, max_y)));
        layout.width = Some(LengthSpec::Px(width));
        self.update_component(Entity::<Tooltip>::from_stable_id(overlay), |tooltip, _| {
            tooltip.style = style;
        })?;
        Ok(())
    }
}

/// The per-type upkeep [`crate::ComponentView::lifecycle`] names, as free
/// functions so each type links only its own.
pub(crate) mod lifecycle_hooks {
    use super::*;

    /// A scrollport told to follow its end goes there when told, not only
    /// when its content next grows: a "back to latest" control writes
    /// `follow_end` and the offset follows.
    pub(crate) fn scroll_view(
        context: &mut AppContext,
        entity: Entity<crate::ScrollView>,
    ) -> Result<(), FrameworkError> {
        let (follow, dragging) =
            context.read(entity, |view| (view.follow_end, view.dragging.is_some()))?;
        if !follow || dragging {
            return Ok(());
        }
        let id = entity.stable_id();
        let at_end = match (
            context.world.scroll_offset(id),
            context.world.scroll_metrics(id),
        ) {
            (Some(offset), Some(metrics)) => offset.y >= metrics.max_offset().y - 0.5,
            // Not laid out yet: the first layout places it.
            _ => true,
        };
        if !at_end {
            context.apply_scroll_retention(entity)?;
        }
        Ok(())
    }

    pub(crate) fn terminal(
        context: &mut AppContext,
        entity: Entity<crate::TerminalView>,
    ) -> Result<(), FrameworkError> {
        context.refresh_terminal_view(entity)?;
        if let Some(bounds) = context.world.layout_box(entity.stable_id()) {
            context.resize_terminal_view(entity, bounds.width, bounds.height)?;
        }
        Ok(())
    }

    pub(crate) fn pane_tree(
        context: &mut AppContext,
        entity: Entity<crate::PaneTree>,
    ) -> Result<(), FrameworkError> {
        context.sync_pane_tree(entity.stable_id())
    }

    /// Start or stop the workspace transition animation to match the model.
    pub(crate) fn workspace(
        context: &mut AppContext,
        entity: Entity<Workspace>,
    ) -> Result<(), FrameworkError> {
        let id = entity.stable_id();
        let origin = context
            .views
            .get(&id)
            .and_then(|view| view.downcast_ref::<Workspace>())
            .and_then(|workspace| workspace.model.transition_origin());
        if let Some(origin) = origin {
            if context.world.is_mounted(id)
                && let Some(spec) = crate::workspace_animation(id, origin)
            {
                let restart = context
                    .world
                    .animation_timing_start(spec.id)
                    .is_none_or(|start| start != origin);
                if restart {
                    let mut mutations = MutationQueue::new();
                    mutations.start_animation(spec);
                    context.world.commit(mutations)?;
                }
            }
        } else if let Some(spec) = crate::workspace_animation(id, Duration::ZERO)
            && context.world.animation_is_active(spec.id)
        {
            let mut mutations = MutationQueue::new();
            mutations.stop_animation(spec.id);
            context.world.commit(mutations)?;
        }
        Ok(())
    }
}
