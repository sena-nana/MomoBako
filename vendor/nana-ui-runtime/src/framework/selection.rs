//! AppContext selection operations.

use super::*;

/// Which fields under a subtree are currently reporting invalid input.
///
/// Produced by [`AppContext::validity_of`]. Ids are in document order, so
/// `first_invalid` is the field a submit handler should reveal and focus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FormValidity {
    /// Enabled fields whose accessible state reports invalid input.
    pub invalid: Vec<StableNodeId>,
}

impl FormValidity {
    /// No enabled field under the subtree is invalid.
    pub fn is_valid(&self) -> bool {
        self.invalid.is_empty()
    }

    /// First invalid field in document order.
    pub fn first_invalid(&self) -> Option<StableNodeId> {
        self.invalid.first().copied()
    }
}

impl AppContext {
    /// Declares that a node accepts drops of the listed kinds.
    ///
    /// Registration is what the framework owns: it can then answer *where* a
    /// drop would land. Deciding what a drop means stays with the application,
    /// as with `SecondaryPress`. Dropping the node releases the registration.
    pub fn set_drop_target<V: View>(
        &mut self,
        entity: Entity<V>,
        accepts: nana_ui_core::DropAccepts,
    ) -> Result<(), FrameworkError> {
        self.set_drop_target_node(entity.id, accepts)
    }

    /// Stops a node accepting drops. Returns whether it was accepting any.
    pub fn clear_drop_target<V: View>(&mut self, entity: Entity<V>) -> bool {
        self.clear_drop_target_node(entity.id)
    }

    /// Stops a node accepting drops by id. Clears hover if this node was it.
    pub fn clear_drop_target_node(&mut self, id: StableNodeId) -> bool {
        if self
            .world
            .drop_hover()
            .is_some_and(|(hover, _)| hover == id)
        {
            self.world.set_drop_hover(None);
        }
        self.world.clear_drop_target(id)
    }

    /// Innermost node whose box contains `(x, y)` and that accepts `kind`,
    /// with the effect it declared.
    ///
    /// A target covers its whole subtree, so a panel registers once instead of
    /// every descendant. Matching is by layout box rather than by hit-test: a
    /// drop surface is usually a plain container that takes no pointer events,
    /// and refusing files over it because it is not clickable would be wrong.
    /// Hidden overlay branches (`box_visible == false`) are skipped.
    /// Returns `None` when nothing there accepts the payload — reject the drop
    /// then rather than guessing a target.
    pub fn drop_target_at(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
        kind: &nana_ui_core::DropKind,
    ) -> Option<(StableNodeId, nana_ui_core::DropEffect)> {
        let depth_of = |mut id: StableNodeId| {
            let mut depth = 0_u32;
            while let Some(parent) = self.world.parent_id(id) {
                depth += 1;
                id = parent;
            }
            depth
        };
        self.world
            .drop_target_ids()
            .filter(|id| {
                self.world
                    .node(*id)
                    .is_some_and(|node| node.document == document)
            })
            .filter(|id| {
                self.world
                    .computed_style(*id)
                    .is_some_and(|style| style.box_visible)
            })
            .filter(|id| {
                self.world
                    .drop_target(*id)
                    .is_some_and(|accepts| accepts.accepts(kind))
            })
            .filter(|id| {
                self.world.layout_box(*id).is_some_and(|bounds| {
                    x >= bounds.x
                        && x < bounds.x + bounds.width
                        && y >= bounds.y
                        && y < bounds.y + bounds.height
                })
            })
            .max_by_key(|id| depth_of(*id))
            .and_then(|id| {
                self.world
                    .drop_target(id)
                    .map(|accepts| (id, accepts.declared_effect()))
            })
    }

    /// Current file-drop hover target, if any.
    pub fn drop_hover(&self) -> Option<(StableNodeId, nana_ui_core::DropEffect)> {
        self.world.drop_hover()
    }

    /// 给节点挂上 painter（Issue #217），不论它的样式由哪个组件写；优先于
    /// [`crate::NodeStyle::painter`]。`None` 取下。
    pub fn set_painter(
        &mut self,
        id: StableNodeId,
        painter: Option<crate::NodePainter>,
    ) -> Result<(), FrameworkError> {
        if !self.world.contains(id) {
            return Err(FrameworkError::MissingView(id));
        }
        let mut queue = crate::MutationQueue::new();
        queue.set_painter(id, painter);
        self.commit_mutations(queue).map(|_| ())
    }

    /// Register a drop target by node id. Vue and other hosts that do not hold
    /// a typed [`Entity`] use this; dropping the node still releases it.
    pub fn set_drop_target_node(
        &mut self,
        id: StableNodeId,
        accepts: nana_ui_core::DropAccepts,
    ) -> Result<(), FrameworkError> {
        if !self.world.contains(id) {
            return Err(FrameworkError::MissingView(id));
        }
        self.world.set_drop_target(id, accepts);
        Ok(())
    }

    /// Resolve a platform file drag onto the registered drop target under the
    /// point, update hover chrome, and emit [`crate::FileDropEvent`]: the
    /// router's step for [`nana_ui_input::InputPayload::FileDrag`]. Returns whether
    /// anything changed, and the target the drag is over or landed on.
    pub(super) fn file_drag(
        &mut self,
        document: DocumentId,
        kind: nana_ui_core::FileDragKind,
        paths: &[std::path::PathBuf],
        position: Option<(f32, f32)>,
    ) -> Result<(bool, Option<StableNodeId>), FrameworkError> {
        let position = match kind {
            nana_ui_core::FileDragKind::Cancel => None,
            nana_ui_core::FileDragKind::Hover | nana_ui_core::FileDragKind::Drop => position,
        };
        let Some((x, y)) = position else {
            return Ok((self.clear_file_drag(document)?, None));
        };
        if !x.is_finite() || !y.is_finite() {
            return Err(FrameworkError::InvalidInput);
        }
        let target = self.drop_target_at(document, x, y, &nana_ui_core::DropKind::Files);
        let landed = target.map(|(id, _)| id);
        if kind == nana_ui_core::FileDragKind::Hover {
            return Ok((self.set_file_drag_hover(document, target, paths)?, landed));
        }
        // A node that was told `Hovered` is owed a terminal event. Releasing
        // over some *other* target — or over nothing — used to clear the
        // hover silently, leaving whatever state it set on hover latched
        // until an unrelated drag happened to enter it again. `Cancel`
        // already emits `Left`; the two terminal paths agree now.
        let previous = self.world.drop_hover();
        if let Some((id, _)) = previous.filter(|(id, _)| landed != Some(*id)) {
            self.emit_file_drop(id, crate::FileDropEvent::Left)?;
        }
        let cleared = self.world.set_drop_hover(None);
        let Some((id, effect)) = target else {
            return Ok((cleared, None));
        };
        self.emit_file_drop(
            id,
            crate::FileDropEvent::Dropped {
                paths: paths.iter().cloned().collect(),
                effect,
            },
        )?;
        Ok((true, landed))
    }

    fn set_file_drag_hover(
        &mut self,
        _document: DocumentId,
        target: Option<(StableNodeId, nana_ui_core::DropEffect)>,
        paths: &[std::path::PathBuf],
    ) -> Result<bool, FrameworkError> {
        let previous = self.world.drop_hover();
        if previous == target {
            return Ok(false);
        }
        if let Some((id, _)) =
            previous.filter(|_| previous.map(|(id, _)| id) != target.map(|(id, _)| id))
        {
            self.emit_file_drop(id, crate::FileDropEvent::Left)?;
        }
        self.world.set_drop_hover(target);
        if let Some((id, effect)) =
            target.filter(|_| previous.map(|(id, _)| id) != target.map(|(id, _)| id))
        {
            self.emit_file_drop(
                id,
                crate::FileDropEvent::Hovered {
                    paths: paths.iter().cloned().collect(),
                    effect,
                },
            )?;
        }
        Ok(true)
    }

    fn clear_file_drag(&mut self, _document: DocumentId) -> Result<bool, FrameworkError> {
        let Some((id, _)) = self.world.drop_hover() else {
            return Ok(false);
        };
        self.emit_file_drop(id, crate::FileDropEvent::Left)?;
        self.world.set_drop_hover(None);
        Ok(true)
    }

    fn emit_file_drop(
        &mut self,
        id: StableNodeId,
        event: crate::FileDropEvent,
    ) -> Result<(), FrameworkError> {
        let Some(emit) = self
            .views
            .get(&id)
            .and_then(|view| self.file_drops.get(&view.as_ref().type_id()))
            .cloned()
        else {
            return Ok(());
        };
        emit(self, id, event)
    }

    /// Collects the invalid fields under `root`, in document order.
    ///
    /// Reads the accessible state every control already publishes, so it covers
    /// each component carrying `invalid` without the application tracking a
    /// parallel copy. Disabled fields are excluded: a control the user cannot
    /// reach must not block submission.
    ///
    /// Validation itself stays the application's: this reports what the tree
    /// currently says, it does not decide what counts as valid.
    pub fn validity_of(&self, root: StableNodeId) -> FormValidity {
        let mut invalid = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let Some(node) = self.world.node(id) else {
                continue;
            };
            stack.extend(node.children.iter().rev().copied());
            if let Some(state) = self.world.accessibility(id)
                && state.invalid
                && !state.disabled
            {
                invalid.push(id);
            }
        }
        FormValidity { invalid }
    }

    /// Reconcile the complete ordered option set and its controlled selection
    /// in one retained transaction. Removed options are parked, preserving
    /// their typed state and application-owned event handlers.
    pub fn set_segmented_options(
        &mut self,
        control: Entity<SegmentedControl>,
        options: Vec<Entity<SegmentedOption>>,
        selected: Option<Entity<SegmentedOption>>,
    ) -> Result<bool, FrameworkError> {
        self.set_segmented_options_inner(control, options, selected)
    }

    pub(super) fn set_segmented_options_inner(
        &mut self,
        control: Entity<SegmentedControl>,
        options: Vec<Entity<SegmentedOption>>,
        selected: Option<Entity<SegmentedOption>>,
    ) -> Result<bool, FrameworkError> {
        self.read(control, |_| ())?;
        let option_ids = options.iter().map(|option| option.id).collect::<Vec<_>>();
        let unique = option_ids.iter().copied().collect::<HashSet<_>>();
        if unique.len() != option_ids.len()
            || selected.is_some_and(|selected| !unique.contains(&selected.id))
        {
            return Err(FrameworkError::InvalidComponentValue(control.id));
        }
        let control_node = self
            .world
            .node(control.id)
            .ok_or(FrameworkError::MissingView(control.id))?;
        if control_node.children.iter().any(|child| {
            !self
                .views
                .get(child)
                .is_some_and(|view| view.is::<SegmentedOption>())
        }) {
            return Err(FrameworkError::InvalidComponentHierarchy {
                parent: control.id,
                child: *control_node
                    .children
                    .iter()
                    .find(|child| {
                        !self
                            .views
                            .get(child)
                            .is_some_and(|view| view.is::<SegmentedOption>())
                    })
                    .unwrap(),
            });
        }
        for option in &options {
            self.read(*option, |_| ())?;
            let node = self
                .world
                .node(option.id)
                .ok_or(FrameworkError::MissingView(option.id))?;
            if node.document != control_node.document
                || node.parent.is_some_and(|parent| parent != control.id)
                || (node.parent.is_none()
                    && self.world.mount_state(option.id) != Some(crate::MountState::Parked))
            {
                return Err(FrameworkError::InvalidComponentHierarchy {
                    parent: control.id,
                    child: option.id,
                });
            }
        }
        let selected_id = selected.map(Entity::stable_id);
        let (size, chrome, fill) = self.read(control, |control| {
            (control.size, control.chrome, control.fill)
        })?;
        let current = self.read(control, |control| {
            (
                control.options.clone(),
                control.selected,
                control.focus_target,
            )
        })?;
        let mut enabled = Vec::new();
        for entity in &options {
            if !self.read(*entity, |option| option.disabled)? {
                enabled.push(entity.id);
            }
        }
        let focus_target = self
            .world
            .focused(control_node.document)
            .filter(|id| unique.contains(id) && enabled.contains(id))
            .or_else(|| {
                selected_id
                    .filter(|id| enabled.contains(id))
                    .or_else(|| enabled.first().copied())
            });
        let mut surface_stale = false;
        for entity in &options {
            if !self.read(*entity, |option| {
                option.size == size
                    && option.chrome == chrome
                    && option.fill == fill
                    && option.selected == (Some(entity.id) == selected_id)
            })? {
                surface_stale = true;
                break;
            }
        }
        if current == (option_ids.clone(), selected_id, focus_target)
            && control_node.children == option_ids
            && !surface_stale
        {
            return Ok(false);
        }

        let mut mutations = MutationQueue::new();
        let removed = control_node
            .children
            .iter()
            .copied()
            .filter(|id| !unique.contains(id))
            .collect::<Vec<_>>();
        for id in &removed {
            mutations.park_subtree(*id);
        }
        for id in &option_ids {
            mutations.insert(control.id, *id, None);
        }
        let mut staged_options = Vec::new();
        for option in options {
            let mut next = self.read(option, Clone::clone)?;
            next.selected = Some(option.id) == selected_id;
            next.synchronize_surface(size, chrome, fill);
            next.project(option.id, &self.world, &mut mutations);
            staged_options.push((option.id, next));
        }
        let mut next_control = self.read(control, Clone::clone)?;
        next_control.options = option_ids;
        next_control.selected = selected_id;
        next_control.focus_target = focus_target;
        next_control.project(control.id, &self.world, &mut mutations);
        self.commit_mutations(mutations)?;
        for (id, option) in staged_options {
            self.install_view(id, option);
        }
        self.views.insert(control.id, Box::new(next_control));
        Ok(true)
    }

    /// Register a control's option children as its options, the one whose
    /// `selected` flag is set as its selection. The view layer runs this
    /// after it builds a control and after a binding changes the control or
    /// one of its options (`TypeBehavior::slot_assembler`), so a view states
    /// options and selection as data; a hidden option stays a child, and a
    /// view disables it too. Returns whether anything changed.
    pub fn assemble_segmented_control(
        &mut self,
        control: Entity<SegmentedControl>,
    ) -> Result<bool, FrameworkError> {
        let children = self
            .world
            .node(control.id)
            .ok_or(FrameworkError::MissingView(control.id))?
            .children
            .clone();
        let mut options = Vec::with_capacity(children.len());
        let mut selected = None;
        for id in children {
            let option = Entity::<SegmentedOption>::from_stable_id(id);
            let Ok(flag) = self.read(option, |option| option.selected) else {
                continue;
            };
            if flag && selected.is_none() {
                selected = Some(option);
            }
            options.push(option);
        }
        self.set_segmented_options_inner(control, options, selected)
    }

    /// [`Self::assemble_segmented_control`] for the control `option` sits in.
    pub fn assemble_segmented_option(
        &mut self,
        option: Entity<SegmentedOption>,
    ) -> Result<bool, FrameworkError> {
        let Some(parent) = self.world.node(option.id).and_then(|node| node.parent) else {
            return Ok(false);
        };
        let control = Entity::<SegmentedControl>::from_stable_id(parent);
        if self.read(control, |_| ()).is_err() {
            return Ok(false);
        }
        self.assemble_segmented_control(control)
    }

    /// Publish controlled selection without replacing the option identities.
    pub fn set_segmented_selection(
        &mut self,
        control: Entity<SegmentedControl>,
        selected: Option<Entity<SegmentedOption>>,
    ) -> Result<bool, FrameworkError> {
        let ids = self.read(control, |control| control.options.clone())?;
        let options = ids.into_iter().map(Entity::from_stable_id).collect();
        self.set_segmented_options(control, options, selected)
    }

    /// Update the control density and every retained option in one commit.
    pub fn set_segmented_size(
        &mut self,
        control: Entity<SegmentedControl>,
        size: nana_ui_core::ControlSize,
    ) -> Result<bool, FrameworkError> {
        let mut next_control = self.read(control, Clone::clone)?;
        if next_control.size == size {
            return Ok(false);
        }
        next_control.apply_size(size);
        let mut mutations = MutationQueue::new();
        let mut staged_options = Vec::new();
        for id in &next_control.options {
            let entity = Entity::<SegmentedOption>::from_stable_id(*id);
            let mut option = self.read(entity, Clone::clone)?;
            option.synchronize_surface(size, next_control.chrome, next_control.fill);
            option.project(*id, &self.world, &mut mutations);
            staged_options.push((*id, option));
        }
        next_control.project(control.id, &self.world, &mut mutations);
        self.commit_mutations(mutations)?;
        for (id, option) in staged_options {
            self.install_view(id, option);
        }
        self.views.insert(control.id, Box::new(next_control));
        Ok(true)
    }

    /// Change one option's availability while preserving controlled checked
    /// state and atomically repairing the group's sequential tab stop.
    pub fn set_segmented_option_disabled(
        &mut self,
        control: Entity<SegmentedControl>,
        option: Entity<SegmentedOption>,
        disabled: bool,
    ) -> Result<bool, FrameworkError> {
        let control_node = self
            .world
            .node(control.id)
            .ok_or(FrameworkError::MissingView(control.id))?;
        if !control_node.children.contains(&option.id) {
            return Err(FrameworkError::InvalidComponentHierarchy {
                parent: control.id,
                child: option.id,
            });
        }
        let mut next_option = self.read(option, Clone::clone)?;
        if next_option.disabled == disabled {
            return Ok(false);
        }
        next_option.disabled = disabled;
        let mut next_control = self.read(control, Clone::clone)?;
        let mut enabled = Vec::new();
        for id in &next_control.options {
            let is_enabled = if *id == option.id {
                !disabled
            } else {
                !self.read(Entity::<SegmentedOption>::from_stable_id(*id), |option| {
                    option.disabled
                })?
            };
            if is_enabled {
                enabled.push(*id);
            }
        }
        next_control.focus_target = self
            .world
            .focused(control_node.document)
            .filter(|focused| next_control.options.contains(focused) && enabled.contains(focused))
            .or_else(|| {
                next_control
                    .selected
                    .filter(|selected| enabled.contains(selected))
                    .or_else(|| enabled.first().copied())
            });
        let document = control_node.document;
        let repair_focus = disabled && self.world.focused(document) == Some(option.id);
        let mut mutations = MutationQueue::new();
        next_option.project(option.id, &self.world, &mut mutations);
        next_control.project(control.id, &self.world, &mut mutations);
        if repair_focus {
            mutations.request_focus(document, next_control.focus_target);
        }
        self.commit_mutations(mutations)?;
        self.views.insert(option.id, Box::new(next_option));
        self.views.insert(control.id, Box::new(next_control));
        Ok(true)
    }

    /// Activation: commit `requested` as the selection and announce it.
    pub fn request_segmented_selection(
        &mut self,
        control: Entity<SegmentedControl>,
        requested: Entity<SegmentedOption>,
    ) -> Result<bool, FrameworkError> {
        self.segmented_selection_inner(control, requested, true)
    }

    /// Roving focus: move focus to `requested` and announce it without
    /// selecting. Arrow keys browse a segmented group; selection follows on
    /// activation, which is the manual-activation behaviour assistive
    /// technology expects from a tablist / radiogroup.
    fn move_segmented_focus(
        &mut self,
        control: Entity<SegmentedControl>,
        requested: Entity<SegmentedOption>,
    ) -> Result<bool, FrameworkError> {
        self.segmented_selection_inner(control, requested, false)
    }

    fn segmented_selection_inner(
        &mut self,
        control: Entity<SegmentedControl>,
        requested: Entity<SegmentedOption>,
        commit: bool,
    ) -> Result<bool, FrameworkError> {
        let is_child = self
            .world
            .node(control.id)
            .map(|node| node.children.contains(&requested.id))
            .ok_or(FrameworkError::MissingView(control.id))?;
        if !is_child {
            return Err(FrameworkError::InvalidComponentHierarchy {
                parent: control.id,
                child: requested.id,
            });
        }
        if self.read(requested, |option| option.disabled)? {
            return Ok(false);
        }
        let document = self.world.node(control.id).unwrap().document;
        // Self-driving, like `Checkbox`, `Switch`, `Select` and `Tabs`: the
        // control commits the selection itself and the event reports what
        // happened rather than asking for it. The commit rides the same
        // transaction as the focus move and the event, so a blocked focus or a
        // failing handler still rolls the selection back with them.
        let committed = self.update_component(control, |control, cx| {
            if commit {
                control.selected = Some(requested.id);
            }
            control.focus_target = Some(requested.id);
            cx.mutations().request_focus(document, Some(requested.id));
            cx.emit(SegmentedSelectionRequested {
                option: requested.id,
            });
            true
        })?;
        if commit && committed {
            // Publish the per-option surface flags the control just changed.
            // An application that wants to veto or redirect the choice writes
            // the selection it wants back with `set_segmented_selection`.
            self.set_segmented_selection(control, Some(requested))?;
            self.update_component(requested, |_, cx| cx.emit(crate::SegmentedOptionChosen))?;
        }
        Ok(committed)
    }

    /// Handle horizontal roving focus before range/table/text key routing.
    pub fn navigate_focused_segmented(
        &mut self,
        document: DocumentId,
        intent: RovingFocusIntent,
    ) -> Result<bool, FrameworkError> {
        let Some(focused) = self.world.focused(document) else {
            return Ok(false);
        };
        let Some(parent) = self.world.parent_id(focused) else {
            return Ok(false);
        };
        if self
            .views
            .get(&parent)
            .is_some_and(|view| view.is::<Tabs>())
        {
            return self.navigate_tabs(Entity::from_stable_id(parent), intent);
        }
        if !self
            .views
            .get(&parent)
            .is_some_and(|view| view.is::<SegmentedControl>())
        {
            return Ok(false);
        }
        let control = Entity::<SegmentedControl>::from_stable_id(parent);
        let (ids, policy) = self.read(control, |control| {
            (control.options.clone(), control.roving_focus)
        })?;
        let items = ids
            .iter()
            .map(|id| {
                (
                    *id,
                    self.views
                        .get(id)
                        .and_then(|view| view.downcast_ref::<SegmentedOption>())
                        .is_some_and(|option| !option.disabled),
                )
            })
            .collect::<Vec<_>>();
        let Some(target) = policy.resolve(&items, Some(focused), intent) else {
            return Ok(false);
        };
        self.move_segmented_focus(control, Entity::from_stable_id(target))
    }

    pub(super) fn is_roving_tab_stop(&self, id: StableNodeId) -> bool {
        if !self
            .views
            .get(&id)
            .is_some_and(|view| view.is::<SegmentedOption>())
        {
            return true;
        }
        let Some(parent) = self.world.parent_id(id) else {
            return false;
        };
        if let Some(control) = self
            .views
            .get(&parent)
            .and_then(|view| view.downcast_ref::<SegmentedControl>())
        {
            return control.focus_target == Some(id);
        }
        self.views
            .get(&parent)
            .and_then(|view| view.downcast_ref::<Tabs>())
            .is_some_and(|tabs| tabs.roving_target() == Some(id))
    }

    pub fn is_segmented_option_node(&self, id: StableNodeId) -> bool {
        self.views
            .get(&id)
            .is_some_and(|view| view.is::<SegmentedOption>())
    }

    pub(super) fn sequential_focus_candidate(
        &self,
        document: DocumentId,
        id: StableNodeId,
    ) -> bool {
        self.world.is_mounted(id)
            && self
                .world
                .node(id)
                .is_some_and(|node| node.document == document)
            && self
                .world
                .interaction(id)
                .is_some_and(|interaction| interaction.focusable)
            && self.is_roving_tab_stop(id)
            && !self.confirm_busy_action_subtree(id)
    }

    pub(super) fn sequential_focus_candidates(&self, document: DocumentId) -> Vec<StableNodeId> {
        self.world
            .document_order(document)
            .into_iter()
            .filter(|id| {
                !self.world.motion_blocks_input(*id)
                    && self.sequential_focus_candidate(document, *id)
                    && self.world.is_overlay_reachable(*id)
            })
            .collect()
    }

    /// Move through the backend-neutral sequential focus order. Roving groups
    /// contribute exactly their current tab stop while retaining programmatic
    /// focusability for every enabled option.
    pub fn navigate_sequential_focus(
        &mut self,
        document: DocumentId,
        reverse: bool,
    ) -> Result<bool, FrameworkError> {
        let candidates = self.sequential_focus_candidates(document);
        if candidates.is_empty() {
            return Ok(false);
        }
        let next = self
            .world
            .focused(document)
            .and_then(|current| candidates.iter().position(|id| *id == current))
            .map(|index| {
                if reverse {
                    (index + candidates.len() - 1) % candidates.len()
                } else {
                    (index + 1) % candidates.len()
                }
            })
            .unwrap_or_else(|| if reverse { candidates.len() - 1 } else { 0 });
        if self.world.focused(document) != Some(candidates[next]) {
            self.focus_node(document, candidates[next])?;
        }
        Ok(true)
    }

    /// Select one professional tab by application-owned value.
    pub fn select_tabs_value(
        &mut self,
        entity: Entity<Tabs>,
        value: impl AsRef<str>,
    ) -> Result<bool, FrameworkError> {
        let value = value.as_ref();
        self.update_component(entity, |tabs, cx| {
            if let Some(event) = tabs.select(value) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    /// Activate the focused or selected tab on a professional strip.
    pub fn activate_tabs(&mut self, entity: Entity<Tabs>) -> Result<bool, FrameworkError> {
        let value = self.read(entity, |tabs| {
            tabs.focus.clone().or_else(|| tabs.selected.clone())
        })?;
        let Some(value) = value else {
            return Ok(false);
        };
        self.select_tabs_value(entity, value.as_ref())
    }

    pub(super) fn activate_tabs_option(
        &mut self,
        entity: Entity<Tabs>,
        option: StableNodeId,
    ) -> Result<bool, FrameworkError> {
        let Some(value) = self.tabs_option_value(entity, option)? else {
            return Ok(false);
        };
        let changed = self.select_tabs_value(entity, value.as_ref())?;
        let document = self
            .world
            .node(entity.id)
            .ok_or(FrameworkError::MissingView(entity.id))?
            .document;
        let _ = self.focus_node(document, option)?;
        Ok(changed)
    }

    pub(super) fn tabs_option_value(
        &self,
        entity: Entity<Tabs>,
        option: StableNodeId,
    ) -> Result<Option<Arc<str>>, FrameworkError> {
        self.read(entity, |tabs| {
            tabs.option_nodes()
                .iter()
                .find(|(_, id)| *id == option)
                .map(|(value, _)| Arc::clone(value))
        })
    }

    /// Painted option boxes after layout, when every child has a real box.
    pub fn tabs_strip_paint(
        &self,
        entity: Entity<Tabs>,
    ) -> Result<Option<nana_ui_core::TabStripPaint<Arc<str>>>, FrameworkError> {
        self.read(entity, |tabs| {
            tabs.strip_paint_from_layout(&self.world, entity.id)
        })
    }

    pub(crate) fn sync_tabs_options(&mut self, entity: Entity<Tabs>) -> Result<(), FrameworkError> {
        let tabs = self.read(entity, Clone::clone)?;
        let node = self
            .world
            .node(entity.id)
            .ok_or(FrameworkError::MissingView(entity.id))?;
        let document = node.document;
        let current_children = node.children.clone();

        let mut unused = HashMap::<Arc<str>, VecDeque<StableNodeId>>::new();
        for (value, id) in tabs.option_nodes() {
            unused.entry(Arc::clone(value)).or_default().push_back(*id);
        }

        let mut next_nodes = Vec::with_capacity(tabs.options.len());
        let mut created = Vec::new();
        for option in &tabs.options {
            let reusable = unused
                .get_mut(&option.value)
                .and_then(|ids| ids.pop_front())
                .filter(|id| {
                    self.world.node(*id).is_some_and(|node| {
                        node.document == document
                            && (node.parent == Some(entity.id)
                                || (node.parent.is_none()
                                    && self.world.mount_state(*id) == Some(MountState::Parked)))
                    }) && self
                        .views
                        .get(id)
                        .is_some_and(|view| view.is::<SegmentedOption>())
                });
            if let Some(id) = reusable {
                next_nodes.push((Arc::clone(&option.value), id));
            } else {
                let id = self.allocate_id();
                created.push((id, crate::tabs::tab_selection_option(option, &tabs)));
                next_nodes.push((Arc::clone(&option.value), id));
            }
        }

        let next_ids = next_nodes.iter().map(|(_, id)| *id).collect::<Vec<_>>();
        let stale = current_children
            .iter()
            .copied()
            .filter(|id| !next_ids.contains(id))
            .collect::<Vec<_>>();
        let created_ids = created.iter().map(|(id, _)| *id).collect::<HashSet<_>>();

        let mut options_dirty =
            !created.is_empty() || current_children != next_ids || !stale.is_empty();
        if !options_dirty {
            for (option, (_, id)) in tabs.options.iter().zip(next_nodes.iter()) {
                let current =
                    self.read(Entity::<SegmentedOption>::from_stable_id(*id), Clone::clone)?;
                if current != crate::tabs::tab_selection_option(option, &tabs) {
                    options_dirty = true;
                    break;
                }
            }
        }
        if !options_dirty && tabs.option_nodes() == next_nodes.as_slice() {
            return Ok(());
        }

        let stale_subtrees = stale
            .iter()
            .map(|id| self.retained_subtree(*id))
            .collect::<Vec<_>>();
        let mut mutations = MutationQueue::new();
        for id in &stale {
            mutations.despawn_subtree(*id);
        }
        for (id, option) in &created {
            mutations.create(*id, document, option.node_kind());
            option.project(*id, &self.world, &mut mutations);
        }
        for id in &next_ids {
            mutations.insert(entity.id, *id, None);
        }

        let mut staged_options = Vec::new();
        for (option, (_, id)) in tabs.options.iter().zip(next_nodes.iter()) {
            if created_ids.contains(id) {
                continue;
            }
            let mut current =
                self.read(Entity::<SegmentedOption>::from_stable_id(*id), Clone::clone)?;
            let desired = crate::tabs::tab_selection_option(option, &tabs);
            if current != desired {
                current = desired;
                current.project(*id, &self.world, &mut mutations);
                staged_options.push((*id, current));
            }
        }

        self.commit_mutations(mutations)?;
        let mut removed = HashSet::new();
        for subtree in stale_subtrees {
            for id in subtree {
                removed.insert(id);
                self.views.remove(&id);
                self.component_lifecycle.tooltips.remove(&id);
                self.component_lifecycle.loading.remove(&id);
            }
        }
        self.remove_event_handlers_for(&removed);
        for (id, option) in created {
            self.install_view(id, option);
        }
        for (id, option) in staged_options {
            self.install_view(id, option);
        }
        if let Some(tabs) = self
            .views
            .get_mut(&entity.id)
            .and_then(|view| view.downcast_mut::<Tabs>())
        {
            tabs.option_nodes = next_nodes;
        }
        Ok(())
    }

    /// Move a tab so it sits before `before`. `None` appends it to the end.
    pub fn reorder_tabs(
        &mut self,
        entity: Entity<Tabs>,
        value: impl AsRef<str>,
        before: Option<&str>,
    ) -> Result<bool, FrameworkError> {
        let value = value.as_ref();
        self.update_component(entity, |tabs, cx| {
            if let Some(event) = tabs.reorder(value, before) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    /// Request that the application close a tab. The strip is not mutated.
    pub fn close_tab(
        &mut self,
        entity: Entity<Tabs>,
        value: impl AsRef<str>,
    ) -> Result<bool, FrameworkError> {
        let value = value.as_ref();
        self.update_component(entity, |tabs, cx| {
            if let Some(event) = tabs.request_close(value) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    /// Report a cross-strip transfer. Application code applies both strips.
    pub fn transfer_tab(
        &mut self,
        source: Entity<Tabs>,
        target_strip: impl AsRef<str>,
        value: impl AsRef<str>,
        before: Option<&str>,
    ) -> Result<bool, FrameworkError> {
        let target_strip = target_strip.as_ref();
        let value = value.as_ref();
        self.update_component(source, |tabs, cx| {
            if let Some(event) = tabs.transfer_to(target_strip, value, before) {
                cx.emit(event);
                true
            } else {
                false
            }
        })
    }

    pub fn navigate_tabs(
        &mut self,
        entity: Entity<Tabs>,
        intent: crate::RovingFocusIntent,
    ) -> Result<bool, FrameworkError> {
        let changed = self.update_component(entity, |tabs, cx| {
            if let Some(event) = tabs.navigate(intent) {
                cx.emit(event);
                true
            } else {
                false
            }
        })?;
        if let Some(target) = self.read(entity, Tabs::roving_target)? {
            let document = self
                .world
                .node(entity.id)
                .ok_or(FrameworkError::MissingView(entity.id))?
                .document;
            let _ = self.focus_node(document, target)?;
        }
        Ok(changed)
    }
}
