//! Atomic mutation validation and application.

use super::*;

/// Validate-then-apply staging for one mutation batch.
///
/// Every field is an overlay over `source`, never a copy of it: validation cost
/// tracks the batch, not the retained world. `parked`, `pointer_captures`, and
/// `animations` fall back to `source` on a miss instead of being pre-filled, and
/// overlay-host walks use `UiWorld::overlay_host_ids`.
pub(super) struct ValidationPlan<'a> {
    pub(super) source: &'a UiWorld,
    pub(super) nodes: NodeMap<PlannedNode>,
    pub(super) removed: NodeSet,
    pub(super) newly_retired: NodeSet,
    /// Mount overrides staged by this batch. Absent means "ask `source`".
    pub(super) parked: NodeMap<bool>,
    detached: NodeMap<bool>,
    pub(super) interactions: NodeMap<InteractionState>,
    pub(super) styles: NodeMap<NodeStyle>,
    pub(super) focus: HashMap<DocumentId, Option<StableNodeId>>,
    pub(super) scope_focus: NodeMap<StableNodeId>,
    /// Cloned from `source` on first pointer-capture mutation. Batches that do
    /// not touch capture never pay for it.
    pub(super) pointer_captures: Option<HashMap<(DocumentId, u64), StableNodeId>>,
    /// Cloned from `source` on first animation mutation, same rationale.
    pub(super) animations: Option<HashMap<AnimationId, AnimationSpec>>,
    pub(super) text_inputs: NodeMap<Option<StagedText>>,
    pub(super) surface_open: NodeMap<bool>,
    pub(super) overlay_hosts: NodeMap<OverlayHostState>,
    pub(super) accessibility: NodeMap<AccessibilityState>,
    overlay_dependents: NodeMap<NodeSet>,
    affected_overlay_hosts: NodeSet,
    /// Nodes visited by whole-set walks during this validation. Reported through
    /// `UiWorld::validation_nodes_scanned` so a reintroduced world scan fails a
    /// test instead of silently costing a frame.
    pub(super) scanned: usize,
}

/// An editor's text and selections as a batch leaves them, for validating
/// what follows in the same batch. The text is a shared copy: staging a caret
/// move copies no text.
#[derive(Debug, Clone)]
pub(super) struct StagedText {
    pub value: crate::TextValue,
    pub selection: crate::TextSelection,
    pub additional: Vec<crate::TextSelection>,
}

impl<'a> ValidationPlan<'a> {
    pub(super) fn new(source: &'a UiWorld) -> Self {
        Self {
            source,
            nodes: Default::default(),
            removed: Default::default(),
            newly_retired: Default::default(),
            parked: Default::default(),
            detached: Default::default(),
            interactions: Default::default(),
            styles: Default::default(),
            focus: HashMap::new(),
            scope_focus: Default::default(),
            pointer_captures: None,
            animations: None,
            text_inputs: Default::default(),
            surface_open: Default::default(),
            overlay_hosts: Default::default(),
            accessibility: Default::default(),
            overlay_dependents: Default::default(),
            affected_overlay_hosts: Default::default(),
            scanned: 0,
        }
    }

    /// Staged mount state. A node this batch created has no `MountState` in
    /// `source`, and `mount_state` returning `None` correctly reads as unparked.
    pub(super) fn is_parked(&self, id: StableNodeId) -> bool {
        if let Some(&parked) = self.parked.get(&id) {
            return parked;
        }
        self.source.mount_state(id) == Some(MountState::Parked)
    }

    fn is_detached(&self, id: StableNodeId) -> bool {
        self.detached
            .get(&id)
            .copied()
            .unwrap_or_else(|| self.source.detached.contains(&id))
    }

    pub(super) fn set_parked(&mut self, id: StableNodeId, parked: bool) {
        self.parked.insert(id, parked);
    }

    pub(super) fn pointer_captures_mut(&mut self) -> &mut HashMap<(DocumentId, u64), StableNodeId> {
        if self.pointer_captures.is_none() {
            self.pointer_captures = Some(self.source.input.pointer_captures.clone());
        }
        self.pointer_captures
            .as_mut()
            .expect("initialized directly above")
    }

    pub(super) fn pointer_capture(
        &self,
        document: DocumentId,
        pointer_id: u64,
    ) -> Option<StableNodeId> {
        match &self.pointer_captures {
            Some(captures) => captures.get(&(document, pointer_id)).copied(),
            None => self.source.pointer_capture(document, pointer_id),
        }
    }

    pub(super) fn animations_mut(&mut self) -> &mut HashMap<AnimationId, AnimationSpec> {
        if self.animations.is_none() {
            self.animations = Some(
                self.source
                    .animations
                    .iter()
                    .map(|(&id, animation)| (id, animation.spec.clone()))
                    .collect(),
            );
        }
        self.animations
            .as_mut()
            .expect("initialized directly above")
    }

    /// Overlay hosts staged by this batch plus those already in `source`.
    pub(super) fn overlay_host_candidates(&mut self, document: DocumentId) -> Vec<StableNodeId> {
        let mut hosts = self
            .overlay_hosts
            .keys()
            .copied()
            .filter(|id| {
                self.planned_identity(*id)
                    .is_ok_and(|(doc, _)| doc == document)
            })
            .collect::<HashSet<_>>();
        hosts.extend(self.source.overlay_host_ids(document));
        self.scanned = self.scanned.saturating_add(hosts.len());
        hosts.into_iter().collect()
    }

    fn planned_identity(
        &self,
        id: StableNodeId,
    ) -> Result<(DocumentId, Option<StableNodeId>), UiWorldError> {
        self.require_exists(id)?;
        if let Some(node) = self.nodes.get(&id) {
            Ok((node.document, node.parent))
        } else {
            self.source.identity_and_parent(id)
        }
    }

    fn overlay_referencing(&mut self, target: StableNodeId) -> Vec<StableNodeId> {
        // Almost no node is an overlay's focus target: answer without
        // building the set.
        if !self.source.overlay_dependents.contains_key(&target)
            && !self.overlay_dependents.contains_key(&target)
        {
            return Vec::new();
        }
        let mut hosts = self
            .source
            .overlay_dependents
            .get(&target)
            .into_iter()
            .flatten()
            .copied()
            .collect::<NodeSet>();
        hosts.extend(
            self.overlay_dependents
                .get(&target)
                .into_iter()
                .flatten()
                .copied(),
        );
        self.scanned += hosts.len();
        hosts
            .into_iter()
            .filter(|host| {
                self.exists(*host)
                    && self
                        .overlay_hosts
                        .get(host)
                        .copied()
                        .or_else(|| self.source.overlay_host(*host))
                        .is_some_and(|state| {
                            state.active == Some(target) || state.restore_focus == Some(target)
                        })
            })
            .collect()
    }

    fn stage_overlay_host(&mut self, host: StableNodeId, state: OverlayHostState) {
        if let Some(previous) = self.overlay_hosts.insert(host, state) {
            for target in [previous.active, previous.restore_focus]
                .into_iter()
                .flatten()
            {
                if let Some(hosts) = self.overlay_dependents.get_mut(&target) {
                    hosts.remove(&host);
                    if hosts.is_empty() {
                        self.overlay_dependents.remove(&target);
                    }
                }
            }
        }
        for target in [state.active, state.restore_focus].into_iter().flatten() {
            self.overlay_dependents
                .entry(target)
                .or_default()
                .insert(host);
        }
        self.affected_overlay_hosts.insert(host);
    }

    pub(super) fn validate(&mut self, mutations: &[UiMutation]) -> Result<(), UiWorldError> {
        // Staged accessibility is read only to check overlay hosts; with none
        // in the world or in this batch, skip copying it.
        let overlays = !self.source.overlay_host_nodes.is_empty()
            || mutations
                .iter()
                .any(|mutation| matches!(mutation, UiMutation::SetOverlayHost { .. }));
        for mutation in mutations {
            match mutation {
                UiMutation::Insert { child: id, .. }
                | UiMutation::Detach { id }
                | UiMutation::SetAccessibility { id, .. } => {
                    let hosts = self.overlay_referencing(*id);
                    self.affected_overlay_hosts.extend(hosts);
                }
                _ => {}
            }
            match mutation {
                UiMutation::Create { id, document, .. } => self.create(*id, *document)?,
                UiMutation::Insert {
                    parent,
                    child,
                    before,
                } => self.insert(*parent, *child, *before)?,
                UiMutation::Detach { id } => {
                    self.detach(*id)?;
                }
                UiMutation::ParkSubtree { root } => self.park(*root)?,
                UiMutation::DespawnSubtree { root } => self.despawn_subtree(*root)?,
                UiMutation::SetStyle { id, style } => {
                    self.require_exists(*id)?;
                    let layout = style.layout.as_ref();
                    if layout.opacity.is_some_and(|opacity| {
                        !opacity.is_finite() || !(0.0..=1.0).contains(&opacity)
                    }) || layout
                        .font_size
                        .is_some_and(|size| !size.is_finite() || size <= 0.0)
                        || layout
                            .letter_spacing
                            .is_some_and(|spacing| !spacing.is_finite())
                        || layout
                            .font_weight
                            .is_some_and(|weight| !(1..=1000).contains(&weight))
                        || layout.color.is_some_and(|color| {
                            color.into_iter().any(|channel| {
                                !channel.is_finite() || !(0.0..=1.0).contains(&channel)
                            })
                        })
                        || layout.background.is_some_and(|color| {
                            color.into_iter().any(|channel| {
                                !channel.is_finite() || !(0.0..=1.0).contains(&channel)
                            })
                        })
                        || layout.border_color.is_some_and(|color| {
                            color.into_iter().any(|channel| {
                                !channel.is_finite() || !(0.0..=1.0).contains(&channel)
                            })
                        })
                    {
                        return Err(UiWorldError::InvalidStyle(*id));
                    }
                    self.styles.insert(*id, style.clone());
                }
                UiMutation::SetTheme { .. } | UiMutation::SetThemeTokens { .. } => {}
                UiMutation::SetText { id, .. } => {
                    self.require_exists(*id)?;
                }
                UiMutation::WriteLayout { id, layout } => {
                    self.require_exists(*id)?;
                    if !layout.x.is_finite()
                        || !layout.y.is_finite()
                        || !layout.width.is_finite()
                        || !layout.height.is_finite()
                        || layout.width < 0.0
                        || layout.height < 0.0
                    {
                        return Err(UiWorldError::InvalidLayout(*id));
                    }
                }
                UiMutation::SetScrollOffset { id, offset } => {
                    self.require_exists(*id)?;
                    // Negative on an axis whose scroll origin is the right /
                    // bottom edge; the metrics clamp decides the range.
                    if !offset.x.is_finite() || !offset.y.is_finite() {
                        return Err(UiWorldError::InvalidScrollOffset(*id));
                    }
                }
                UiMutation::SetScrollMetrics { id, metrics } => {
                    self.require_exists(*id)?;
                    if metrics.is_some_and(|metrics| {
                        [
                            metrics.viewport_width,
                            metrics.viewport_height,
                            metrics.content_width,
                            metrics.content_height,
                        ]
                        .into_iter()
                        .any(|extent| !extent.is_finite() || extent < 0.0)
                            || [metrics.origin_x, metrics.origin_y]
                                .into_iter()
                                .any(|origin| !origin.is_finite() || origin > 0.0)
                    }) {
                        return Err(UiWorldError::InvalidScrollMetrics(*id));
                    }
                }
                UiMutation::SetInteraction { id, interaction } => {
                    self.require_exists(*id)?;
                    self.interactions.insert(*id, *interaction);
                }
                UiMutation::SetPainter { id, .. } => {
                    self.require_exists(*id)?;
                }
                UiMutation::SetCustomRender { id, content } => {
                    self.require_exists(*id)?;
                    if content.as_ref().is_some_and(|content| {
                        content.renderer.trim().is_empty() || content.resource.trim().is_empty()
                    }) {
                        return Err(UiWorldError::InvalidCustomRender(*id));
                    }
                }
                UiMutation::SetEventListener { id, event, .. } => {
                    self.require_exists(*id)?;
                    if event.trim().is_empty() {
                        return Err(UiWorldError::InvalidEventListener(*id));
                    }
                }
                UiMutation::SetComponentType { id, .. } => {
                    self.require_exists(*id)?;
                }
                UiMutation::SetStandardVisual { id, visual } => {
                    self.require_exists(*id)?;
                    let invalid_ratio = match visual {
                        Some(StandardVisual::Range { ratio, .. })
                        | Some(StandardVisual::Progress {
                            value_ratio: ratio, ..
                        })
                        | Some(StandardVisual::LevelMeter {
                            value_ratio: ratio, ..
                        }) => !ratio.is_finite() || !(0.0..=1.0).contains(ratio),
                        _ => false,
                    };
                    if invalid_ratio {
                        return Err(UiWorldError::InvalidStandardVisual(*id));
                    }
                }
                UiMutation::SetAccessibility { id, accessibility } => {
                    self.require_exists(*id)?;
                    if overlays {
                        self.accessibility.insert(*id, accessibility.clone());
                    }
                }
                UiMutation::SetLabelledBy { id, label } => {
                    self.require_exists(*id)?;
                    if let Some(label) = label {
                        self.require_exists(*label)?;
                    }
                }
                UiMutation::SetSurfaceOpen { id, open, .. } => {
                    self.require_exists(*id)?;
                    self.surface_open.insert(*id, *open);
                }
                UiMutation::SetOverlayHost { host, state } => {
                    let host_document = self.planned_identity(*host)?.0;
                    if let Some(active) = state.active
                        && self.planned_identity(active)?.1 != Some(*host)
                    {
                        return Err(UiWorldError::InvalidOverlayHost(*host));
                    }
                    if let Some(restore_focus) = state.restore_focus
                        && self.planned_identity(restore_focus)?.0 != host_document
                    {
                        return Err(UiWorldError::FocusDocument {
                            document: host_document,
                            target: restore_focus,
                        });
                    }
                    self.stage_overlay_host(*host, *state);
                }
                UiMutation::CapturePointer { pointer_id, target } => {
                    let document = self.node(*target)?.document;
                    if self.is_parked(*target) {
                        return Err(UiWorldError::NotPointerInteractive(*target));
                    }
                    self.pointer_captures_mut()
                        .insert((document, *pointer_id), *target);
                }
                UiMutation::ReleasePointer { pointer_id, target } => {
                    let document = self.node(*target)?.document;
                    if self.pointer_capture(document, *pointer_id) != Some(*target) {
                        return Err(UiWorldError::PointerCaptureMismatch {
                            pointer_id: *pointer_id,
                            target: *target,
                        });
                    }
                    self.pointer_captures_mut().remove(&(document, *pointer_id));
                }
                UiMutation::StartAnimation { animation } => {
                    self.node(animation.target)?;
                    if !animation.is_valid() {
                        return Err(UiWorldError::InvalidAnimation(animation.id));
                    }
                    if self.is_parked(animation.target) && !animation.uses_presentation_overlay() {
                        return Err(UiWorldError::InvalidAnimation(animation.id));
                    }
                    self.animations_mut()
                        .insert(animation.id, animation.clone());
                }
                UiMutation::StopAnimation { id } => {
                    // A finished run that fills forwards still holds its
                    // property; stopping it is how its owner gives it back.
                    if self.animations_mut().remove(id).is_none()
                        && self.source.presentation.get(id.track_id()).is_none()
                    {
                        return Err(UiWorldError::MissingAnimation(*id));
                    }
                }
                UiMutation::FinishAnimation { id } => {
                    if self.animations_mut().remove(id).is_none() {
                        return Err(UiWorldError::MissingAnimation(*id));
                    }
                }
                UiMutation::ReverseAnimation { id }
                | UiMutation::PauseAnimation { id }
                | UiMutation::ResumeAnimation { id } => {
                    if !self.animations_mut().contains_key(id) {
                        return Err(UiWorldError::MissingAnimation(*id));
                    }
                }
                UiMutation::RequestFocus { document, target } => {
                    if let Some(target) = target {
                        let node = self.node(*target)?;
                        if node.document != *document {
                            return Err(UiWorldError::FocusDocument {
                                document: *document,
                                target: *target,
                            });
                        }
                        let interaction =
                            self.interactions.get(target).copied().unwrap_or_else(|| {
                                self.source
                                    .nodes
                                    .get(*target)
                                    .map(|node| node.interaction)
                                    .unwrap_or_default()
                            });
                        let visible = self.focus_target_visible(*target)?;
                        if !interaction.focusable
                            || !visible
                            || !self.active_modal_allows_focus(*document, *target)?
                        {
                            return Err(UiWorldError::NotFocusable(*target));
                        }
                    }
                    self.focus.insert(*document, *target);
                    if let Some(target) = target {
                        self.remember_scope_focus(*target)?;
                    }
                }
                UiMutation::RestoreFocusWithin { root } => {
                    if let Some((document, target)) = self.restorable_scope_focus(*root)? {
                        self.focus.insert(document, Some(target));
                        self.remember_scope_focus(target)?;
                    }
                }
                UiMutation::SetIme { id, composition } => {
                    let document = self.node(*id)?.document;
                    if composition.is_some() && self.is_parked(*id) {
                        return Err(UiWorldError::NotFocused(*id));
                    }
                    let focused = self
                        .focus
                        .get(&document)
                        .copied()
                        .unwrap_or_else(|| self.source.focused(document));
                    if focused != Some(*id) {
                        return Err(UiWorldError::NotFocused(*id));
                    }
                    if composition.is_some() {
                        self.text_input(*id)?;
                    }
                    if let Some(ImeComposition {
                        text,
                        selection: Some((start, end)),
                    }) = composition
                        && (start > end
                            || *end > text.len()
                            || !text.is_char_boundary(*start)
                            || !text.is_char_boundary(*end)
                            || !crate::TextSelection::new(*start, *end).is_valid_for(text))
                    {
                        return Err(UiWorldError::InvalidIme(*id));
                    }
                }
                UiMutation::SetTextInput { id, state } => {
                    self.require_exists(*id)?;
                    if state
                        .as_ref()
                        .is_some_and(|state| !state.selection.is_valid_for(&state.value))
                    {
                        return Err(UiWorldError::InvalidTextInput(*id));
                    }
                    // Staged as the session will hold it: further cursors
                    // snapped and fused, so what validates after this in the
                    // batch edits the text the apply will edit.
                    let staged = state.as_ref().map(|state| {
                        let mut state = state.clone();
                        state.normalize_selections();
                        StagedText {
                            value: state.value,
                            selection: state.selection,
                            additional: state.additional_selections,
                        }
                    });
                    self.text_inputs.insert(*id, staged);
                }
                UiMutation::SetTextSelection { id, selection } => {
                    let mut staged = self.text_input(*id)?;
                    if !selection.is_valid_for(&staged.value) {
                        return Err(UiWorldError::InvalidTextInput(*id));
                    }
                    staged.selection = *selection;
                    self.text_inputs.insert(*id, Some(staged));
                }
                UiMutation::ReplaceTextSelection { id, text } => {
                    let staged = self.text_input(*id)?;
                    let mut state = crate::TextInputState {
                        value: staged.value,
                        selection: staged.selection,
                        additional_selections: staged.additional,
                    };
                    if !state.replace_selection(text) {
                        return Err(UiWorldError::InvalidTextInput(*id));
                    }
                    self.text_inputs.insert(
                        *id,
                        Some(StagedText {
                            value: state.value,
                            selection: state.selection,
                            additional: state.additional_selections,
                        }),
                    );
                }
                UiMutation::SetHighlightRequest { id, request } => {
                    self.require_exists(*id)?;
                    if request
                        .as_ref()
                        .is_some_and(|request| request.presenter.trim().is_empty())
                    {
                        return Err(UiWorldError::InvalidHighlightRequest(*id));
                    }
                }
                UiMutation::SetTextInputFoldCollapsed { id, .. }
                | UiMutation::SetTextInputInlays { id, .. }
                | UiMutation::SetTextInputSnippet { id, .. }
                | UiMutation::SetTextInputCompletions { id, .. }
                | UiMutation::SetTextInputCompletionView { id, .. }
                | UiMutation::SetTextInputCompletionDismissed { id }
                | UiMutation::SetTextInputCompletionReopened { id }
                | UiMutation::SetTextInputHover { id, .. }
                | UiMutation::SetTextInputDiagnosticHover { id, .. }
                | UiMutation::SetTextInputHoverScroll { id, .. }
                | UiMutation::SetTextInputSignature { id, .. } => {
                    self.require_exists(*id)?;
                }
            }
        }
        self.validate_overlay_hosts()?;
        Ok(())
    }

    pub(super) fn create(
        &mut self,
        id: StableNodeId,
        document: DocumentId,
    ) -> Result<(), UiWorldError> {
        if self.exists(id) {
            return Err(UiWorldError::DuplicateNode(id));
        }
        if self.source.is_retired(id) || self.newly_retired.contains(&id) {
            return Err(UiWorldError::RetiredNode(id));
        }
        self.removed.remove(&id);
        self.set_parked(id, false);
        self.nodes.insert(
            id,
            PlannedNode {
                document,
                parent: None,
                children: Vec::new(),
            },
        );
        Ok(())
    }

    pub(super) fn insert(
        &mut self,
        parent: StableNodeId,
        child: StableNodeId,
        before: Option<StableNodeId>,
    ) -> Result<(), UiWorldError> {
        let parent_document = self.node(parent)?.document;
        let child_node = self.node(child)?.clone();
        if child_node.document != parent_document {
            return Err(UiWorldError::CrossDocument { parent, child });
        }
        if parent == child || self.has_ancestor(parent, child)? {
            return Err(UiWorldError::Cycle { parent, child });
        }
        if before == Some(child) && child_node.parent == Some(parent) {
            return Ok(());
        }
        if let Some(before) = before
            && !self.node(parent)?.children.contains(&before)
        {
            return Err(UiWorldError::InvalidBefore { parent, before });
        }
        let depth = self
            .ancestor_depth(parent)?
            .saturating_add(self.subtree_height(child)?);
        if depth > MAX_TREE_DEPTH {
            return Err(UiWorldError::TreeTooDeep {
                parent,
                child,
                depth,
            });
        }
        self.detach(child)?;
        let siblings = &mut self.node_mut(parent)?.children;
        let index = before
            .and_then(|before| siblings.iter().position(|id| *id == before))
            .unwrap_or(siblings.len());
        siblings.insert(index, child);
        self.node_mut(child)?.parent = Some(parent);
        self.detached.insert(child, false);
        let parked = self.is_parked(parent);
        self.set_parked_subtree(child, parked)?;
        Ok(())
    }

    /// Levels from `id` up to its root, counting `id` itself.
    pub(super) fn ancestor_depth(&mut self, id: StableNodeId) -> Result<usize, UiWorldError> {
        let mut depth = 1;
        let mut cursor = self.node(id)?.parent;
        while let Some(ancestor) = cursor {
            depth += 1;
            if depth > MAX_TREE_DEPTH {
                // `has_ancestor` already rejected cycles, so this is genuine
                // depth rather than a loop.
                return Ok(depth);
            }
            cursor = self.node(ancestor)?.parent;
        }
        Ok(depth)
    }

    /// Levels from `root` down to its deepest descendant, counting `root`.
    /// Stops climbing once past the limit; the caller only needs to know that.
    pub(super) fn subtree_height(&mut self, root: StableNodeId) -> Result<usize, UiWorldError> {
        let mut frontier = vec![root];
        let mut height = 0;
        while !frontier.is_empty() {
            height += 1;
            if height > MAX_TREE_DEPTH {
                return Ok(height);
            }
            let mut next = Vec::new();
            for id in frontier {
                next.extend(self.node(id)?.children.iter().copied());
            }
            frontier = next;
        }
        Ok(height)
    }

    pub(super) fn park(&mut self, root: StableNodeId) -> Result<(), UiWorldError> {
        self.detach(root)?;
        let subtree = self.subtree(root)?;
        self.set_parked_subtree(root, true)?;
        let parked = subtree.iter().copied().collect::<HashSet<_>>();
        let documents = subtree
            .iter()
            .map(|id| self.node(*id).map(|node| node.document))
            .collect::<Result<HashSet<_>, _>>()?;
        for document in documents {
            let focused = self
                .focus
                .get(&document)
                .copied()
                .unwrap_or_else(|| self.source.focused(document));
            if focused.is_some_and(|target| parked.contains(&target)) {
                self.focus.insert(document, None);
            }
        }
        if self.pointer_captures.is_some() || !self.source.input.pointer_captures.is_empty() {
            self.pointer_captures_mut()
                .retain(|_, target| !parked.contains(target));
        }
        if self.animations.is_some() || !self.source.animations.is_empty() {
            self.animations_mut().retain(|_, animation| {
                !parked.contains(&animation.target) || animation.uses_presentation_overlay()
            });
        }
        for id in subtree {
            self.clear_overlay_references(id);
        }
        Ok(())
    }

    pub(super) fn subtree(
        &mut self,
        root: StableNodeId,
    ) -> Result<Vec<StableNodeId>, UiWorldError> {
        let mut subtree = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let children = self.node(id)?.children.clone();
            stack.extend(children);
            subtree.push(id);
        }
        Ok(subtree)
    }

    pub(super) fn set_parked_subtree(
        &mut self,
        root: StableNodeId,
        parked: bool,
    ) -> Result<(), UiWorldError> {
        for id in self.subtree(root)? {
            self.set_parked(id, parked);
        }
        Ok(())
    }

    pub(super) fn detach(&mut self, id: StableNodeId) -> Result<(), UiWorldError> {
        let parent = self.node(id)?.parent;
        self.detached.insert(id, true);
        if let Some(parent) = parent {
            self.node_mut(parent)?.children.retain(|child| *child != id);
            self.node_mut(id)?.parent = None;
        }
        Ok(())
    }

    pub(super) fn despawn_subtree(&mut self, root: StableNodeId) -> Result<(), UiWorldError> {
        let subtree = self.subtree(root)?;
        let removed = subtree.iter().copied().collect::<HashSet<_>>();
        let documents = subtree
            .iter()
            .map(|id| self.node(*id).map(|node| node.document))
            .collect::<Result<HashSet<_>, _>>()?;
        for document in documents {
            let focused = self
                .focus
                .get(&document)
                .copied()
                .unwrap_or_else(|| self.source.focused(document));
            if focused.is_some_and(|target| removed.contains(&target)) {
                self.focus.insert(document, None);
            }
        }
        self.detach(root)?;
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            let children = self.node(id)?.children.clone();
            stack.extend(children);
            self.nodes.remove(&id);
            self.removed.insert(id);
            self.newly_retired.insert(id);
            self.parked.remove(&id);
            if self.pointer_captures.is_some() || !self.source.input.pointer_captures.is_empty() {
                self.pointer_captures_mut()
                    .retain(|_, target| *target != id);
            }
            if self.animations.is_some() || !self.source.animations.is_empty() {
                self.animations_mut()
                    .retain(|_, animation| animation.target != id);
            }
            self.text_inputs.remove(&id);
            self.clear_overlay_references(id);
        }
        Ok(())
    }

    pub(super) fn clear_overlay_references(&mut self, removed: StableNodeId) {
        for host in self.overlay_referencing(removed) {
            if host == removed || !self.exists(host) {
                continue;
            }
            let Some(mut state) = self
                .overlay_hosts
                .get(&host)
                .copied()
                .or_else(|| self.source.overlay_host(host))
            else {
                continue;
            };
            if state.active == Some(removed) {
                state.active = None;
                state.restore_focus = None;
            }
            if state.restore_focus == Some(removed) {
                state.restore_focus = None;
            }
            self.stage_overlay_host(host, state);
        }
    }

    pub(super) fn validate_overlay_hosts(&mut self) -> Result<(), UiWorldError> {
        let hosts = self
            .affected_overlay_hosts
            .iter()
            .copied()
            .collect::<Vec<_>>();
        self.scanned += hosts.len();
        for host in hosts {
            if !self.exists(host) {
                continue;
            }
            let Some(state) = self
                .overlay_hosts
                .get(&host)
                .copied()
                .or_else(|| self.source.overlay_host(host))
            else {
                continue;
            };
            let host_document = self.planned_identity(host)?.0;
            if let Some(active) = state.active
                && (!self.exists(active) || self.planned_identity(active)?.1 != Some(host))
            {
                return Err(UiWorldError::InvalidOverlayHost(host));
            }
            if let Some(active) = state.active {
                let accessibility = self
                    .accessibility
                    .get(&active)
                    .or_else(|| self.source.accessibility(active));
                if !accessibility.is_some_and(|accessibility| match accessibility.role {
                    AccessibilityRole::Dialog | AccessibilityRole::AlertDialog => {
                        accessibility.modal
                    }
                    AccessibilityRole::Region => {
                        !accessibility.modal
                            && accessibility
                                .label
                                .as_ref()
                                .is_some_and(|label| !label.is_empty())
                    }
                    AccessibilityRole::Menu
                    | AccessibilityRole::Tooltip
                    | AccessibilityRole::Status => true,
                    _ => false,
                }) {
                    return Err(UiWorldError::InvalidOverlayHost(host));
                }
            }
            if let Some(restore_focus) = state.restore_focus
                && (!self.exists(restore_focus)
                    || self.node(restore_focus)?.document != host_document)
            {
                return Err(UiWorldError::InvalidOverlayHost(host));
            }
        }
        Ok(())
    }

    pub(super) fn has_ancestor(
        &mut self,
        mut id: StableNodeId,
        candidate: StableNodeId,
    ) -> Result<bool, UiWorldError> {
        let mut visited = HashSet::new();
        loop {
            if id == candidate {
                return Ok(true);
            }
            if !visited.insert(id) {
                return Ok(false);
            }
            let Some(parent) = self.node(id)?.parent else {
                return Ok(false);
            };
            id = parent;
        }
    }

    fn require_exists(&self, id: StableNodeId) -> Result<(), UiWorldError> {
        // Scalar writes need staged identity, not a cloned child list.
        self.exists(id)
            .then_some(())
            .ok_or(UiWorldError::MissingNode(id))
    }

    pub(super) fn exists(&self, id: StableNodeId) -> bool {
        !self.removed.contains(&id) && (self.nodes.contains_key(&id) || self.source.contains(id))
    }

    pub(super) fn text_input(&mut self, id: StableNodeId) -> Result<StagedText, UiWorldError> {
        self.node(id)?;
        if let Some(state) = self.text_inputs.get(&id) {
            return state.clone().ok_or(UiWorldError::MissingTextInput(id));
        }
        self.source
            .text_input(id)
            .map(|view| StagedText {
                value: view.value_shared(),
                selection: view.selection,
                additional: view.additional_selections.to_vec(),
            })
            .ok_or(UiWorldError::MissingTextInput(id))
    }

    pub(super) fn overlay_branch_active(&mut self, id: StableNodeId) -> Result<bool, UiWorldError> {
        let Some(parent) = self.node(id)?.parent else {
            return Ok(true);
        };
        let state = self
            .overlay_hosts
            .get(&parent)
            .copied()
            .or_else(|| self.source.overlay_host(parent));
        Ok(state.is_none_or(|state| state.active == Some(id)))
    }

    pub(super) fn focus_target_visible(
        &mut self,
        mut id: StableNodeId,
    ) -> Result<bool, UiWorldError> {
        let mut visibility = None;
        loop {
            if self.is_parked(id) || self.is_detached(id) {
                return Ok(false);
            }
            let layout = self
                .styles
                .get(&id)
                .map(|style| style.layout.as_ref())
                .or_else(|| {
                    self.source
                        .node_style(id)
                        .map(|style| style.layout.as_ref())
                });
            visibility = visibility.or_else(|| layout.and_then(|layout| layout.paint.visibility));
            if layout.is_some_and(|layout| layout.omits_box())
                || !self.overlay_branch_active(id)?
                || self.node(id)?.parent.is_some_and(|parent| {
                    super::closed_menu_hides(self.source.standard_visual_ref(parent), id)
                })
            {
                return Ok(false);
            }
            let Some(parent) = self.node(id)?.parent else {
                return Ok(visibility != Some(nana_ui_core::VisibilitySpec::Hidden));
            };
            id = parent;
        }
    }

    pub(super) fn active_modal_allows_focus(
        &mut self,
        document: DocumentId,
        target: StableNodeId,
    ) -> Result<bool, UiWorldError> {
        let hosts = self.overlay_host_candidates(document);
        if hosts.is_empty() {
            return Ok(true);
        }
        // Document order only breaks ties between competing modals, so it stays
        // unevaluated until a first modal is found. Hosts that exist with no
        // active surface are the common case and must not pay for the walk.
        let mut order: Option<Vec<StableNodeId>> = None;
        let mut top = None;
        for host in hosts {
            if !self.exists(host) || self.is_parked(host) {
                continue;
            }
            let Some(state) = self
                .overlay_hosts
                .get(&host)
                .copied()
                .or_else(|| self.source.overlay_host(host))
            else {
                continue;
            };
            let Some(active) = state.active else {
                continue;
            };
            if !self
                .surface_open
                .get(&active)
                .copied()
                .unwrap_or(!self.source.surface_closed(active))
            {
                continue;
            }
            if !self.exists(active)
                || self.is_parked(active)
                || self.node(host)?.document != document
                || self.node(active)?.parent != Some(host)
                || !self.focus_target_visible(active)?
            {
                continue;
            }
            let modal = self
                .accessibility
                .get(&active)
                .or_else(|| self.source.accessibility(active))
                .is_some_and(|state| state.modal);
            if modal {
                let z = self
                    .styles
                    .get(&active)
                    .or_else(|| self.source.node_style(active))
                    .and_then(|style| style.layout.z_index)
                    .unwrap_or_default();
                if order.is_none() {
                    order = Some(self.planned_document_order(document)?);
                }
                let document_order = order
                    .as_ref()
                    .expect("document order resolved directly above")
                    .iter()
                    .position(|candidate| *candidate == active)
                    .unwrap_or_default();
                if top.is_none_or(|(top_z, top_order, _)| (z, document_order) > (top_z, top_order))
                {
                    top = Some((z, document_order, active));
                }
            }
        }
        top.map(|(_, _, active)| self.has_ancestor(target, active))
            .transpose()
            .map(|allowed| allowed.unwrap_or(true))
    }

    pub(super) fn planned_document_order(
        &mut self,
        document: DocumentId,
    ) -> Result<Vec<StableNodeId>, UiWorldError> {
        let ids = self
            .source
            .document_roots(document)
            .into_iter()
            .chain(
                self.nodes
                    .iter()
                    .filter_map(|(id, node)| (node.document == document).then_some(*id)),
            )
            .collect::<HashSet<_>>();
        self.scanned = self.scanned.saturating_add(ids.len());
        let mut roots = Vec::new();
        for id in ids {
            if self.exists(id)
                && !self.is_parked(id)
                && !self.is_detached(id)
                && self.node(id)?.document == document
                && self.node(id)?.parent.is_none()
            {
                roots.push(id);
            }
        }
        roots.sort_unstable();
        let mut order = Vec::new();
        let mut stack = roots.into_iter().rev().collect::<Vec<_>>();
        while let Some(id) = stack.pop() {
            if !self.exists(id) || self.is_parked(id) || self.is_detached(id) {
                continue;
            }
            order.push(id);
            let children = self.node(id)?.children.clone();
            stack.extend(children.into_iter().rev());
        }
        Ok(order)
    }

    pub(super) fn node(&mut self, id: StableNodeId) -> Result<&PlannedNode, UiWorldError> {
        self.ensure(id)?;
        Ok(self.nodes.get(&id).expect("ensured node must exist"))
    }

    pub(super) fn node_mut(&mut self, id: StableNodeId) -> Result<&mut PlannedNode, UiWorldError> {
        self.ensure(id)?;
        Ok(self.nodes.get_mut(&id).expect("ensured node must exist"))
    }

    pub(super) fn ensure(&mut self, id: StableNodeId) -> Result<(), UiWorldError> {
        if self.removed.contains(&id) {
            return Err(UiWorldError::MissingNode(id));
        }
        if self.nodes.contains_key(&id) {
            return Ok(());
        }
        let snapshot = self.source.node(id).ok_or(UiWorldError::MissingNode(id))?;
        self.nodes.insert(
            id,
            PlannedNode {
                document: snapshot.document,
                parent: snapshot.parent,
                children: snapshot.children,
            },
        );
        Ok(())
    }
}

// Select options extend the hit region without changing the field layout.
// Highlight and label changes remain paint-only.
fn select_menu_hit_shape(visual: Option<&StandardVisual>) -> Option<(ControlSize, usize)> {
    match visual {
        Some(StandardVisual::Select {
            opened: true,
            size,
            options,
            ..
        }) => Some((*size, options.len())),
        _ => None,
    }
}

impl UiWorld {
    pub(super) fn apply(&mut self, mutation: &UiMutation, report: &mut CommitReport) {
        // Invalidate before topology writes so old ancestors also lose stale
        // maxima. Insert's new parent is invalidated independently below.
        match mutation {
            UiMutation::WriteLayout { id, .. } => {
                // Marked with the commit's other writes when it ends.
                self.defer_scroll_content(*id);
                self.scroll_layout_touched = true;
            }
            UiMutation::Detach { id }
            | UiMutation::ParkSubtree { root: id }
            | UiMutation::DespawnSubtree { root: id } => {
                self.invalidate_scroll_topology(*id, None);
                self.scroll_layout_touched = true;
            }
            UiMutation::Insert { parent, child, .. } => {
                self.invalidate_scroll_topology(*child, Some(*parent));
                self.scroll_layout_touched = true;
            }
            UiMutation::SetStyle { id, style } => {
                let layout = &self.record(*id).style.layout;
                // A paint-only restyle shares the layout style it replaces.
                if !Arc::ptr_eq(layout, &style.layout) {
                    let omits_box = layout.omits_box() != style.layout.omits_box();
                    let visual = self.nodes.visual(*id);
                    let was = scroll_container(layout, visual);
                    let now = scroll_container(&style.layout, visual);
                    // By value: a `ScrollView` re-projects an equal style in
                    // a fresh `Arc` on every update.
                    let restyled = (was || now) && **layout != *style.layout;
                    if omits_box {
                        self.invalidate_scroll_content(*id);
                        self.scroll_layout_touched = true;
                    }
                    // A container that starts or stops scrolling, or
                    // restyles, is re-measured even when no box moved.
                    if restyled {
                        self.track_scroll_container(*id, now);
                    }
                }
            }
            UiMutation::SetStandardVisual { id, visual } => {
                let layout = &self.record(*id).style.layout;
                let was = scroll_container(layout, self.nodes.visual(*id));
                let now = scroll_container(layout, visual.as_ref());
                if was != now {
                    self.track_scroll_container(*id, now);
                }
            }
            _ => {}
        }
        match mutation {
            UiMutation::Create { id, document, kind } => {
                self.nodes.insert(
                    *id,
                    NodeRecord::new(*document, kind, initial_interaction(kind)),
                );
                self.dirty_entities.insert(*id);
                self.pending_work_revision = self.pending_work_revision.saturating_add(1);
                self.spawned_since_drain += 1;
                report.created += 1;
                self.refresh_root_membership(*id);
            }
            UiMutation::Insert {
                parent,
                child,
                before,
            } => {
                let old_parent = self
                    .identity_and_parent(*child)
                    .expect("validated child must exist")
                    .1;
                if old_parent == Some(*parent) && *before == Some(*child) {
                    return;
                }
                if let Some(old_parent) = old_parent {
                    let hierarchy = self.hierarchy_mut(old_parent);
                    Arc::make_mut(&mut hierarchy.children).retain(|id| id != child);
                    intern_empty_children(&mut hierarchy.children);
                }
                let parent_hierarchy = self.hierarchy_mut(*parent);
                let siblings = Arc::make_mut(&mut parent_hierarchy.children);
                let index = before
                    .and_then(|before| siblings.iter().position(|id| *id == before))
                    .unwrap_or(siblings.len());
                siblings.insert(index, *child);
                let _parent_hierarchy = parent_hierarchy;
                self.hierarchy_mut(*child).parent = Some(*parent);
                let parent_mount = self.record(*parent).mount;
                if self.record(*child).mount != parent_mount {
                    self.set_subtree_mount_state(*child, parent_mount);
                }
                if old_parent == Some(*parent) {
                    // Retained-order moves carry the entire subtree; descendants
                    // keep their inherited state and local geometry until layout
                    // writeback identifies actual changes.
                    self.mark(
                        *child,
                        DirtyMask::INPUT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
                    );
                } else {
                    self.mark_subtree(*child, DirtyMask::ALL);
                }
                self.mark_ancestors(
                    *parent,
                    DirtyMask::LAYOUT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
                );
                if let Some(old_parent) = old_parent {
                    self.mark_ancestors(
                        old_parent,
                        DirtyMask::LAYOUT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
                    );
                }
                if old_parent.is_some() {
                    report.reparented += 1;
                } else {
                    report.inserted += 1;
                }
                self.detached.remove(child);
                self.detached_mounted.remove(child);
                self.sync_subtree_presence(*child);
                self.refresh_root_membership(*child);
                self.note_structural_change(*parent);
                if let Some(old_parent) = old_parent {
                    self.note_structural_change(old_parent);
                }
            }
            UiMutation::Detach { id } => {
                if self.unlink_from_parent(*id) {
                    report.detached += 1;
                }
                self.leave_live_document(*id);
                self.refresh_root_membership(*id);
            }
            UiMutation::ParkSubtree { root } => {
                self.unlink_from_parent(*root);
                self.set_subtree_mount_state(*root, MountState::Parked);
                self.leave_live_document(*root);
                self.refresh_root_membership(*root);
            }
            UiMutation::DespawnSubtree { root } => {
                let root_snapshot = self.node(*root).expect("validated root must exist");
                if let Some(parent) = root_snapshot.parent {
                    let hierarchy = self.hierarchy_mut(parent);
                    Arc::make_mut(&mut hierarchy.children).retain(|child| child != root);
                    intern_empty_children(&mut hierarchy.children);
                    let _hierarchy = hierarchy;
                    self.mark_ancestors(
                        parent,
                        DirtyMask::LAYOUT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
                    );
                    self.note_structural_change(parent);
                }
                let mut stack = vec![*root];
                while let Some(id) = stack.pop() {
                    let snapshot = self.node(id).expect("validated subtree must exist");
                    stack.extend(snapshot.children.iter().rev().copied());
                    self.forget_visual_presence(id);
                    self.scroll_content_bounds
                        .get_mut()
                        .remove(id, snapshot.parent);
                    self.paint_recordings.get_mut().remove(&id);
                    if !self.painter_overrides.is_empty() {
                        self.painter_overrides.remove(&id);
                    }
                    self.write_overlay_host(id, None);
                    self.reindex_component(id, None);
                    self.forget_labelled_by(id);
                    let _removed = self.nodes.remove(id);
                    self.input.focus_scopes.retain(|root, target| {
                        if *target == Some(id) {
                            *target = None;
                        }
                        *root != id
                    });
                    self.dirty_entities.remove(&id);
                    self.non_scroll_hit_dirty.remove(&id);
                    self.remove_document_root(snapshot.document, id);
                    if self.input.focused.get(&snapshot.document) == Some(&id) {
                        self.input.focused.remove(&snapshot.document);
                    }
                    if let Some(index) = self.hit_test_index.get_mut(&snapshot.document) {
                        retain_hit_tree(index, id);
                    }
                    let released = self
                        .input
                        .pointer_captures
                        .iter()
                        .filter_map(|(&(document, pointer_id), &target)| {
                            (target == id).then_some((document, pointer_id))
                        })
                        .collect::<Vec<_>>();
                    for key @ (_, pointer_id) in released {
                        self.input.pointer_captures.remove(&key);
                        self.input
                            .pending_pointer_capture_changes
                            .push(PointerCaptureChange {
                                pointer_id,
                                target: id,
                                captured: false,
                            });
                    }
                    self.input.pointer_hover.retain(|_, target| *target != id);
                    self.input.pointer_press.retain(|_, target| *target != id);
                    self.cancel_animations_for_removed(id);
                    self.surface_motion.remove(&id);
                    self.closing_surfaces.remove(&id);
                    self.hover_transitions.remove(&id);
                    self.clear_overlay_references(id);
                    self.overlay_host_nodes.remove(&id);
                    self.drop_targets.remove(&id);
                    // Every other per-node side table is cleared here; this one
                    // grew for the life of the session on any view that mounts
                    // and unmounts promoted nodes.
                    self.compositor_layer_requests.remove(&id);
                    if self.drop_hover.is_some_and(|(hover, _)| hover == id) {
                        self.drop_hover = None;
                    }
                    self.detached.remove(&id);
                    self.detached_mounted.remove(&id);
                    self.retired.insert(id);
                    self.pending_render_removals.push(id);
                    self.pending_accessibility_removals.push(id);
                    self.despawned_since_drain += 1;
                    report.despawned += 1;
                }
            }
            UiMutation::SetStyle { id, style } => {
                let previous = self.record(*id).style.clone();
                let inherited_text_changed = previous.layout.font_size != style.layout.font_size
                    || previous.layout.font_weight != style.layout.font_weight
                    || previous.layout.font_italic != style.layout.font_italic
                    || previous.layout.font_family != style.layout.font_family
                    || previous.layout.line_height != style.layout.line_height
                    || previous.layout.letter_spacing != style.layout.letter_spacing
                    || previous.layout.font_features != style.layout.font_features
                    || previous.layout.font_variation_settings
                        != style.layout.font_variation_settings
                    || previous.layout.font_kerning != style.layout.font_kerning
                    || previous.layout.word_break != style.layout.word_break
                    || previous.layout.line_break != style.layout.line_break
                    // Writing mode and direction inherit into layout, not just
                    // text: a descendant container lays out along them too, and
                    // the INPUT bit re-projects the accessibility nodes that
                    // report which way their text reads.
                    || previous.layout.dir != style.layout.dir
                    || previous.layout.writing_mode != style.layout.writing_mode
                    || previous.layout.text_orientation != style.layout.text_orientation;
                let inherited_paint_changed = previous.foreground != style.foreground
                    || previous.layout.color != style.layout.color
                    || previous.layout.selection_background != style.layout.selection_background
                    || previous.layout.selection_color != style.layout.selection_color;
                let inherited_opacity_changed = previous.layout.opacity != style.layout.opacity;
                let paint_visibility_changed =
                    previous.layout.paint.visibility != style.layout.paint.visibility;
                let pointer_events_changed =
                    previous.layout.pointer_events != style.layout.pointer_events;
                let cursor_changed = previous.layout.cursor != style.layout.cursor;
                let user_select_changed = previous.layout.user_select != style.layout.user_select;
                let omits_box_changed = previous.layout.omits_box() != style.layout.omits_box();
                let transform_changed = previous.layout.transform != style.layout.transform
                    || previous.layout.transform_3d != style.layout.transform_3d
                    || previous.layout.transform_origin != style.layout.transform_origin
                    || previous.layout.transform_box != style.layout.transform_box
                    || previous.layout.css_perspective != style.layout.css_perspective
                    || previous.layout.preserve_3d != style.layout.preserve_3d
                    || previous.layout.unsupported_transform != style.layout.unsupported_transform;
                let stacking_changed = previous.layout.z_index != style.layout.z_index
                    || previous.layout.isolation != style.layout.isolation;
                let layout_changed =
                    layout_semantics_changed(previous.layout.as_ref(), style.layout.as_ref());
                if !super::text::same_text_constraint_inputs(&previous, style) {
                    self.nodes
                        .invalidate_text(*id, crate::text_node::TextDirty::CONSTRAINT);
                }
                if previous.text_horizontal_alignment != style.text_horizontal_alignment
                    && self
                        .nodes
                        .text_node(*id)
                        .is_some_and(|text| !text.layout.is_null())
                {
                    // Only a retained layout places lines by alignment; host
                    // metrics do not read it. It moves no box, so no layout
                    // scope reaches the text: schedule it explicitly.
                    self.nodes
                        .invalidate_text(*id, crate::text_node::TextDirty::CONSTRAINT);
                    self.mark(*id, DirtyMask::TEXT);
                }
                self.write_node_style(*id, style.clone());
                self.release_rewritten_holds(*id, &previous.layout, &style.layout);
                self.sync_node_presence(*id);

                if !style_excluding_transform_and_cursor_eq(&previous, style) {
                    self.mark(*id, DirtyMask::STYLE | DirtyMask::RENDER);
                }
                if previous.painter != style.painter {
                    // The hit index entry holds the painter (Issue #217).
                    self.mark(*id, DirtyMask::INPUT);
                    if style.painter.is_none() && !self.painter_overrides.contains_key(id) {
                        self.paint_recordings.get_mut().remove(id);
                    }
                }
                if inherited_paint_changed {
                    self.mark_subtree(*id, DirtyMask::STYLE | DirtyMask::RENDER);
                } else if inherited_opacity_changed {
                    // Descendants resolve a new accumulated opacity, but they
                    // do not paint differently for it: nothing downstream of
                    // extraction reads `ComputedStyle::opacity`. The renderer
                    // walks the ancestor chain itself, and a container with
                    // descendants to fade is an opacity group, whose opacity
                    // composites once at the group instead of reaching each
                    // descendant's primitive. Re-extracting the subtree every
                    // frame of a fade would rebuild primitives byte for byte
                    // the same.
                    self.mark_subtree(*id, DirtyMask::STYLE);
                }
                if inherited_text_changed {
                    self.mark_subtree(*id, super::motion::INHERITED_TEXT_DIRTY);
                }
                if omits_box_changed {
                    self.mark_subtree(
                        *id,
                        DirtyMask::STYLE
                            | DirtyMask::LAYOUT
                            | DirtyMask::INPUT
                            | DirtyMask::FOCUS_IME
                            | DirtyMask::ACCESSIBILITY
                            | DirtyMask::RENDER,
                    );
                    if let Some(parent) = self.parent_id(*id) {
                        self.mark(parent, DirtyMask::ACCESSIBILITY);
                    }
                } else if paint_visibility_changed {
                    self.mark_subtree(
                        *id,
                        DirtyMask::STYLE
                            | DirtyMask::INPUT
                            | DirtyMask::FOCUS_IME
                            | DirtyMask::ACCESSIBILITY
                            | DirtyMask::RENDER,
                    );
                    if let Some(parent) = self.parent_id(*id) {
                        self.mark(parent, DirtyMask::ACCESSIBILITY);
                    }
                }
                if pointer_events_changed {
                    // Inherited: unspecified descendants pick up the new used
                    // value. Not a layout dirty.
                    self.mark_subtree(*id, DirtyMask::STYLE | DirtyMask::INPUT);
                    self.clear_hover_for_pointer_events_none(*id);
                }
                if cursor_changed {
                    self.cursor_style_dirty = true;
                    // Cursor is inherited and consumed by the host from the
                    // resolved style; descendants need fresh computed values,
                    // but no layout, hit-test, or render extraction is required.
                    self.mark_subtree(*id, DirtyMask::STYLE);
                }
                if user_select_changed {
                    self.mark_subtree(*id, DirtyMask::STYLE | DirtyMask::RENDER);
                }
                if transform_changed {
                    // Scene extract and hit-test read `layout.transform`; LAYOUT
                    // does not, so paint-transform is not a layout dirty.
                    self.mark_subtree(*id, DirtyMask::TRANSFORM | DirtyMask::INPUT);
                    // Only this node is extracted again. A descendant's
                    // primitives are built in its own space and projected by
                    // the chain above it, and the renderer re-projects a
                    // retained descendant from the ancestor's new transform
                    // rather than rebuilding it. Extracting the subtree every
                    // frame of an animation would hand back the same geometry.
                    self.mark(*id, DirtyMask::RENDER);
                } else if stacking_changed {
                    self.mark_subtree(*id, DirtyMask::INPUT | DirtyMask::RENDER);
                }
                if layout_changed {
                    self.mark_subtree(
                        *id,
                        DirtyMask::LAYOUT
                            | DirtyMask::INPUT
                            | DirtyMask::ACCESSIBILITY
                            | DirtyMask::RENDER,
                    );
                }
                if (layout_changed || inherited_text_changed || omits_box_changed)
                    && let Some(parent) = self.parent_id(*id)
                {
                    self.mark_ancestors(parent, DirtyMask::LAYOUT | DirtyMask::RENDER);
                }
            }
            UiMutation::SetTheme { mode } => {
                self.apply_compiled_theme(nana_ui_core::builtin_theme_arc(*mode));
            }
            UiMutation::SetThemeTokens { theme } => {
                self.apply_compiled_theme(Arc::clone(theme));
            }
            UiMutation::SetText { id, text } => {
                // Re-setting the same text is not a content change: nothing
                // about the node's shaping or layout moved.
                if self.record(*id).text != *text {
                    self.record_mut(*id).text = text.clone();
                    self.invalidate_text_content(*id);
                }
                self.mark(
                    *id,
                    DirtyMask::TEXT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
                );
                if let Some(document) = self.nodes.get(*id).map(|node| node.document)
                    && self
                        .document_text_selections
                        .get(&document)
                        .is_some_and(|selection| selection.node == *id)
                {
                    self.set_document_text_selection(document, None);
                }
            }
            UiMutation::WriteLayout { id, layout } => {
                let record = self.record_mut(*id);
                // A box that only moved leaves its text's constraints alone;
                // one that changed size is a new container for it.
                let resized = record.layout.width.to_bits() != layout.width.to_bits()
                    || record.layout.height.to_bits() != layout.height.to_bits();
                let moved = record.layout.x.to_bits() != layout.x.to_bits()
                    || record.layout.y.to_bits() != layout.y.to_bits();
                record.layout = *layout;
                // A modal frame sizes its surface to its body slot's box: when
                // that box changes, the frame's clip, hit area and paint change
                // with it although the frame's own box did not.
                if (resized || moved)
                    && let Some(parent) = self.parent_id(*id)
                    && matches!(
                        self.nodes.visual(parent),
                        Some(StandardVisual::ModalFrame { slots, .. }) if slots.body == Some(*id)
                    )
                {
                    self.mark(
                        parent,
                        DirtyMask::INPUT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
                    );
                }
                if resized {
                    self.nodes
                        .invalidate_text(*id, crate::text_node::TextDirty::CONSTRAINT);
                }
                // Scoped layout already emits every recomputed box, including
                // shifted descendants. Mark only this node so a bit-identical
                // child is not extracted solely because an ancestor was written.
                self.mark(
                    *id,
                    DirtyMask::INPUT | DirtyMask::RENDER | DirtyMask::ACCESSIBILITY,
                );
            }
            UiMutation::SetScrollOffset { id, offset } => {
                self.scroll_to_clamped(*id, *offset);
                // Clamped again once the commit's boxes are measured, so an
                // offset restored with the content it scrolls to survives.
                self.scroll_requested.push((*id, *offset));
            }
            UiMutation::SetScrollMetrics { id, metrics } => {
                self.store_scroll_metrics(*id, *metrics);
            }
            UiMutation::SetInteraction { id, interaction } => {
                self.record_mut(*id).interaction = *interaction;
                if !interaction.pointer_events {
                    self.input.pointer_hover.retain(|_, target| target != id);
                    self.input.pointer_press.retain(|_, target| target != id);
                }
                self.mark(
                    *id,
                    DirtyMask::STATE
                        | DirtyMask::INPUT
                        | DirtyMask::FOCUS_IME
                        | DirtyMask::RENDER
                        | DirtyMask::ACCESSIBILITY,
                );
            }
            UiMutation::SetCustomRender { id, content } => {
                self.nodes.set_custom_render(*id, content.clone());
                self.mark(*id, DirtyMask::RENDER);
            }
            UiMutation::SetPainter { id, painter } => {
                let changed = match painter {
                    Some(painter) => self
                        .painter_overrides
                        .insert(*id, painter.clone())
                        .is_none_or(|previous| previous != *painter),
                    None => self.painter_overrides.remove(id).is_some(),
                };
                if changed && self.node_painter(*id).is_none() {
                    self.paint_recordings.get_mut().remove(id);
                }
                if changed {
                    self.mark(*id, DirtyMask::RENDER | DirtyMask::INPUT);
                }
            }
            UiMutation::SetEventListener { id, event, enabled } => {
                let mut listeners = self.nodes.event_listeners(*id).cloned().unwrap_or_default();
                listeners.set(event.clone(), *enabled);
                if listeners.is_empty() {
                    self.nodes.set_event_listeners(*id, None);
                } else {
                    self.nodes.set_event_listeners(*id, Some(listeners));
                }
            }
            UiMutation::SetComponentType { id, type_id } => {
                let current = self.nodes.component_type(*id);
                if current != type_id.as_ref() {
                    self.reindex_component(*id, type_id.as_ref());
                    self.nodes.set_component_type(*id, type_id.clone());
                }
            }
            UiMutation::SetStandardVisual { id, visual } => {
                if let (
                    Some(StandardVisual::Switch {
                        checked: old,
                        thumb_progress,
                        ..
                    }),
                    Some(StandardVisual::Switch { checked: next, .. }),
                ) = (self.nodes.visual(*id), visual.as_ref())
                    && old != next
                {
                    self.start_component_track(
                        *id,
                        crate::component_animation_kinds::SWITCH,
                        self.theme.duration(nana_ui_core::MotionRole::OverlayFade),
                        self.theme
                            .motion()
                            .easing(nana_ui_core::EasingRole::Standard),
                        crate::AnimatableProperty::Progress,
                        crate::MotionValue::Scalar(*thumb_progress),
                        crate::MotionValue::Scalar(f32::from(*next)),
                        crate::MotionInterrupt::Retarget,
                        None,
                    );
                }

                let button_layout_changed = match (self.nodes.visual(*id), visual.as_ref()) {
                    (
                        Some(StandardVisual::Button {
                            icon: a,
                            trailing_icon: ta,
                            icon_size: sa,
                            icon_gap: ga,
                            loading: la,
                            ..
                        }),
                        Some(StandardVisual::Button {
                            icon: b,
                            trailing_icon: tb,
                            icon_size: sb,
                            icon_gap: gb,
                            loading: lb,
                            ..
                        }),
                    ) => {
                        (a.is_some(), ta.is_some(), sa, ga, la)
                            != (b.is_some(), tb.is_some(), sb, gb, lb)
                    }
                    (_, Some(StandardVisual::Button { .. }))
                    | (Some(StandardVisual::Button { .. }), _) => true,
                    _ => false,
                };
                let (
                    text_input_presentation_changed,
                    empty_state_presentation_changed,
                    modal_presentation_changed,
                    modal_state_changed,
                    menu_state_changed,
                    select_hit_changed,
                    text_folds_changed,
                ) = {
                    let previous_visual = self.nodes.visual(*id);
                    let text_folds_changed = match (previous_visual, visual.as_ref()) {
                        (
                            Some(StandardVisual::TextInput {
                                folds: previous, ..
                            }),
                            Some(StandardVisual::TextInput { folds: next, .. }),
                        ) => previous != next,
                        (Some(StandardVisual::TextInput { .. }), _) => true,
                        (_, Some(StandardVisual::TextInput { folds, .. })) => !folds.is_empty(),
                        _ => false,
                    };
                    (
                        matches!(previous_visual, Some(StandardVisual::TextInput { .. }))
                            || matches!(visual, Some(StandardVisual::TextInput { .. })),
                        matches!(previous_visual, Some(StandardVisual::EmptyState { .. }))
                            || matches!(visual, Some(StandardVisual::EmptyState { .. })),
                        matches!(previous_visual, Some(StandardVisual::ModalFrame { .. }))
                            || matches!(visual, Some(StandardVisual::ModalFrame { .. })),
                        match (previous_visual, visual) {
                            (
                                Some(StandardVisual::ModalFrame {
                                    busy: old_busy,
                                    danger: old_danger,
                                    ..
                                }),
                                Some(StandardVisual::ModalFrame { busy, danger, .. }),
                            ) => old_busy != busy || old_danger != danger,
                            _ => false,
                        },
                        // Whether a menu's items take part in layout follows the
                        // surface's own open state, so opening it has to reach
                        // them the way an overlay host reaches its branch.
                        menu_surface_open(previous_visual) != menu_surface_open(visual.as_ref()),
                        select_menu_hit_shape(previous_visual)
                            != select_menu_hit_shape(visual.as_ref()),
                        text_folds_changed,
                    )
                };
                let text_path_changed = super::text_visual_key(self.nodes.visual(*id))
                    != super::text_visual_key(visual.as_ref());
                self.nodes.set_visual(*id, visual.clone());
                if text_path_changed {
                    // The text path or a leading indicator's inset changed; the
                    // box may not move, so schedule the text explicitly.
                    self.mark(*id, DirtyMask::TEXT);
                }
                self.sync_node_presence(*id);
                if !matches!(visual, Some(StandardVisual::TextInput { .. })) {
                    self.nodes.set_text_input_presentation(*id, None);
                    self.nodes.set_text_viewport_pin(*id, None);
                }
                if !matches!(visual, Some(StandardVisual::EmptyState { .. })) {
                    self.nodes.set_empty_state_text(*id, None);
                }
                if !matches!(visual, Some(StandardVisual::ModalFrame { .. })) {
                    self.nodes.set_modal_text(*id, None);
                }
                if text_folds_changed {
                    let offered = match visual.as_ref() {
                        Some(StandardVisual::TextInput { folds, .. }) if !folds.is_empty() => {
                            Some(Arc::clone(folds))
                        }
                        _ => None,
                    };
                    self.reconcile_text_fold_offered(*id, offered);
                }
                self.mark(
                    *id,
                    DirtyMask::RENDER
                        | if button_layout_changed {
                            DirtyMask::LAYOUT
                        } else {
                            0
                        }
                        | if text_input_presentation_changed
                            || empty_state_presentation_changed
                            || modal_presentation_changed
                        {
                            DirtyMask::TEXT | DirtyMask::LAYOUT
                        } else {
                            0
                        },
                );
                if select_hit_changed {
                    self.mark(*id, DirtyMask::INPUT);
                }
                if menu_state_changed {
                    self.mark_subtree(
                        *id,
                        DirtyMask::STYLE
                            | DirtyMask::LAYOUT
                            | DirtyMask::INPUT
                            | DirtyMask::FOCUS_IME
                            | DirtyMask::ACCESSIBILITY
                            | DirtyMask::RENDER,
                    );
                    self.mark_ancestors(*id, DirtyMask::LAYOUT | DirtyMask::RENDER);
                }
                if modal_state_changed {
                    self.mark_subtree(
                        *id,
                        DirtyMask::STYLE
                            | DirtyMask::INPUT
                            | DirtyMask::FOCUS_IME
                            | DirtyMask::ACCESSIBILITY
                            | DirtyMask::RENDER,
                    );
                }
            }
            UiMutation::SetLabelledBy { id, label } => {
                if self.labelled_by(*id) != *label {
                    self.set_labelled_by(*id, *label);
                    self.mark(*id, DirtyMask::ACCESSIBILITY);
                }
            }
            UiMutation::SetAccessibility { id, accessibility } => {
                let previous = &self.record(*id).accessibility;
                let interaction_style_changed = previous.disabled != accessibility.disabled
                    || previous.checked != accessibility.checked
                    || previous.selected != accessibility.selected
                    || previous.mixed != accessibility.mixed;
                // A numeric field's spinner is drawn from these: which halves
                // are live follows `editable` and the value against the range.
                let spinner_changed = previous.editable != accessibility.editable
                    || previous.numeric_value != accessibility.numeric_value
                    || previous.numeric_minimum != accessibility.numeric_minimum
                    || previous.numeric_maximum != accessibility.numeric_maximum;
                self.record_mut(*id).accessibility = accessibility.clone();
                self.mark(*id, DirtyMask::ACCESSIBILITY);
                if spinner_changed {
                    self.mark(*id, DirtyMask::RENDER);
                }
                if interaction_style_changed && !self.record(*id).style.interaction.is_empty() {
                    self.mark(*id, DirtyMask::STYLE | DirtyMask::RENDER);
                }
                if interaction_style_changed && self.node_painter(*id).is_some() {
                    self.mark_repaint(*id);
                }
            }
            UiMutation::SetSurfaceOpen { id, open, menu } => {
                self.set_surface_open(*id, *open, *menu);
            }
            UiMutation::SetOverlayHost { host, state } => {
                let previous = self.nodes.overlay_host(*host).copied();
                if previous == Some(*state) {
                    return;
                }
                self.write_overlay_host(*host, Some(*state));
                self.mark(*host, DirtyMask::ACCESSIBILITY);
                if let Some(inactive) = previous
                    .and_then(|previous| previous.active)
                    .filter(|active| Some(*active) != state.active)
                {
                    self.clear_surface_pointer_interactions(inactive);
                }
                let changed_roots = previous
                    .and_then(|previous| previous.active)
                    .into_iter()
                    .chain(state.active)
                    .collect::<HashSet<_>>();
                for root in changed_roots {
                    self.mark_subtree(
                        root,
                        DirtyMask::STYLE
                            | DirtyMask::LAYOUT
                            | DirtyMask::INPUT
                            | DirtyMask::FOCUS_IME
                            | DirtyMask::ACCESSIBILITY
                            | DirtyMask::RENDER,
                    );
                }
            }
            UiMutation::CapturePointer { pointer_id, target } => {
                let document = self.record(*target).document;
                let previous = self
                    .input
                    .pointer_captures
                    .insert((document, *pointer_id), *target);
                if previous == Some(*target) {
                    return;
                }
                if let Some(previous) = previous {
                    self.input
                        .pending_pointer_capture_changes
                        .push(PointerCaptureChange {
                            pointer_id: *pointer_id,
                            target: previous,
                            captured: false,
                        });
                }
                self.input
                    .pending_pointer_capture_changes
                    .push(PointerCaptureChange {
                        pointer_id: *pointer_id,
                        target: *target,
                        captured: true,
                    });
            }
            UiMutation::ReleasePointer { pointer_id, target } => {
                let document = self.record(*target).document;
                self.input.pointer_captures.remove(&(document, *pointer_id));
                self.input
                    .pending_pointer_capture_changes
                    .push(PointerCaptureChange {
                        pointer_id: *pointer_id,
                        target: *target,
                        captured: false,
                    });
            }
            UiMutation::StartAnimation { animation } => {
                self.install_animation(animation.clone());
            }
            UiMutation::StopAnimation { id } => {
                self.cancel_animation(*id);
            }
            UiMutation::FinishAnimation { id } => {
                self.finish_animation(*id);
            }
            UiMutation::ReverseAnimation { id } => {
                self.reverse_animation(*id);
            }
            UiMutation::PauseAnimation { id } => {
                self.pause_animation(*id);
            }
            UiMutation::ResumeAnimation { id } => {
                self.resume_animation(*id);
            }
            UiMutation::RequestFocus { document, target } => {
                if let Some(target) = target {
                    self.remember_scope_focus(*target);
                }
                let from_pointer = self.input.pointer_modality.contains(document);
                if from_pointer {
                    self.input.focus_from_pointer.insert(*document);
                } else {
                    self.input.focus_from_pointer.remove(document);
                }
                let old = match target {
                    Some(target) => self.input.focused.insert(*document, *target),
                    None => self.input.focused.remove(document),
                };
                if let Some(old) = old.filter(|old| Some(*old) != *target) {
                    self.release_focus(old);
                }
                if let Some(target) = target {
                    self.mark(*target, DirtyMask::STATE);
                    if !self.record(*target).style.interaction.focused.is_empty() {
                        self.mark(*target, DirtyMask::STYLE | DirtyMask::RENDER);
                    }
                    self.mark_focus_changed(*target);
                }
            }
            UiMutation::RestoreFocusWithin { root } => {
                let target = ValidationPlan::new(self)
                    .restorable_scope_focus(*root)
                    .ok()
                    .flatten();
                if let Some((document, target)) = target {
                    self.apply(
                        &UiMutation::RequestFocus {
                            document,
                            target: Some(target),
                        },
                        report,
                    );
                }
            }
            UiMutation::SetIme { id, composition } => {
                if let Some(editor) = self.nodes.editor_mut(*id) {
                    let before = editor.ime().map(|ime| ime.to_composition());
                    match composition {
                        Some(composition) if !composition.text.is_empty() => {
                            editor.empty_preedit = None;
                            editor.session.set_preedit(
                                &composition.text,
                                composition.selection.map(|(start, end)| start..end),
                            );
                        }
                        Some(composition) => {
                            editor.session.cancel_composition();
                            editor.empty_preedit = Some(composition.selection);
                        }
                        None => {
                            editor.session.cancel_composition();
                            editor.empty_preedit = None;
                        }
                    }
                    // One update per change of what the IME shows, the
                    // empty preedit included: the session counts only its
                    // compositions.
                    let mut work = editor.session.take_work();
                    work.composition_updates = 0;
                    let changed = editor.ime().map(|ime| ime.to_composition()) != before;
                    self.pending_edit_work.accumulate(work);
                    if changed {
                        self.pending_edit_work.composition_updates += 1;
                    }
                }
                self.nodes
                    .invalidate_text(*id, crate::text_node::TextDirty::EDIT_STATE);
                self.mark(
                    *id,
                    DirtyMask::TEXT | DirtyMask::FOCUS_IME | DirtyMask::RENDER,
                );
            }
            UiMutation::SetTextInput { id, state } => {
                let Some(state) = state else {
                    self.remove_ime(*id);
                    self.nodes.set_editor(*id, None);
                    self.record_mut(*id).text = TextContent::default();
                    self.invalidate_text_content(*id);
                    self.nodes.set_text_fold_view(*id, None);
                    self.nodes.set_text_inlays(*id, None);
                    self.nodes.set_text_snippet_session(*id, None);
                    self.nodes.set_text_completion_view(*id, None);
                    self.nodes.set_text_hover_view(*id, None);
                    self.nodes.set_text_signature(*id, None);
                    self.mark(
                        *id,
                        DirtyMask::TEXT
                            | DirtyMask::FOCUS_IME
                            | DirtyMask::RENDER
                            | DirtyMask::ACCESSIBILITY,
                    );
                    return;
                };
                // The component's own buffer: the session adopts it, so both
                // hold one copy of the text.
                let value = state.value.clone();
                // The text before, for the fold and snippet remaps: a shared
                // copy, so taking it costs nothing.
                let previous = self
                    .nodes
                    .editor(*id)
                    .map(|editor| editor.session.snapshot());
                match self.nodes.editor_mut(*id) {
                    // Mounting an editor puts its value in place; nobody
                    // edited it.
                    None => self.nodes.set_editor(
                        *id,
                        Some(crate::store::EditorRecord::new(
                            nana_text::EditSession::with_selections(
                                value,
                                state.selection,
                                state.additional_selections.iter().copied(),
                            ),
                        )),
                    ),
                    Some(editor) => {
                        let additional = state.additional_selections.clone();
                        let work = edit_keeping_preedit(editor, |session| {
                            session.assign(&value, Some((state.selection, additional)));
                        });
                        self.pending_edit_work.accumulate(work);
                    }
                }
                let next = self
                    .nodes
                    .editor(*id)
                    .map(|editor| editor.session.snapshot())
                    .unwrap_or_default();
                // The same text (a selection-only write, or a component's copy
                // of these bytes) leaves the content alone: only what the
                // edit state draws changed. A `SetText` since then wrote the
                // node something else, and this write puts the value back.
                if previous
                    .as_ref()
                    .is_some_and(|previous| previous.same_identity(&next))
                    && self.record(*id).text.value.same_identity(&next)
                {
                    self.nodes
                        .invalidate_text(*id, crate::text_node::TextDirty::EDIT_STATE);
                } else {
                    self.record_mut(*id).text = TextContent {
                        value: next.clone(),
                    };
                    self.invalidate_text_content(*id);
                }
                // 值变化后重映射折叠态与 snippet 会话：受影响的折叠自动
                // 展开，跳位失效即结束会话。
                if let Some(previous) = previous
                    && (self.nodes.text_fold_view(*id).is_some()
                        || self.nodes.text_snippet_session(*id).is_some())
                    && previous != next
                {
                    self.reconcile_text_view_state(*id, &previous, &next);
                }
                self.mark(
                    *id,
                    DirtyMask::TEXT
                        | DirtyMask::FOCUS_IME
                        | DirtyMask::RENDER
                        | DirtyMask::ACCESSIBILITY,
                );
            }
            UiMutation::SetTextSelection { id, selection } => {
                let editor = self
                    .nodes
                    .editor_mut(*id)
                    .expect("entity must have runtime component");
                let work = edit_keeping_preedit(editor, |session| {
                    session.set_primary_selection(*selection);
                });
                self.pending_edit_work.accumulate(work);
                self.nodes
                    .invalidate_text(*id, crate::text_node::TextDirty::EDIT_STATE);
                self.mark(
                    *id,
                    DirtyMask::TEXT
                        | DirtyMask::FOCUS_IME
                        | DirtyMask::RENDER
                        | DirtyMask::ACCESSIBILITY,
                );
            }
            UiMutation::ReplaceTextSelection { id, text } => {
                let editor = self
                    .nodes
                    .editor_mut(*id)
                    .expect("entity must have runtime component");
                let work = edit_keeping_preedit(editor, |session| {
                    replace_every_selection(session, text);
                });
                let value = editor.session.snapshot();
                self.pending_edit_work.accumulate(work);
                self.record_mut(*id).text = TextContent { value };
                self.invalidate_text_content(*id);
                self.mark(
                    *id,
                    DirtyMask::TEXT
                        | DirtyMask::FOCUS_IME
                        | DirtyMask::RENDER
                        | DirtyMask::ACCESSIBILITY,
                );
            }
            UiMutation::SetHighlightRequest { id, request } => {
                self.nodes.set_highlight(*id, request.clone());
                if request.is_none() {
                    self.nodes.set_text_presentation(*id, None);
                }
                self.mark(*id, DirtyMask::TEXT | DirtyMask::RENDER);
            }
            UiMutation::SetTextInputFoldCollapsed { id, folds } => {
                // 规范化：仅保留仍在宿主喂入区间内、且确实可折叠的条目。
                let offered = match self.nodes.visual(*id) {
                    Some(StandardVisual::TextInput { folds: offered, .. }) => Arc::clone(offered),
                    _ => Arc::from([]),
                };
                let mut collapsed: Vec<crate::TextCodeFold> = folds
                    .iter()
                    .copied()
                    .filter(|fold| offered.contains(fold))
                    .collect();
                collapsed.sort_by_key(|fold| (fold.start, fold.end));
                collapsed.dedup();
                let changed = self
                    .nodes
                    .text_fold_view(*id)
                    .map(|entry| entry.collapsed.as_slice() != collapsed.as_slice())
                    .unwrap_or(!collapsed.is_empty());
                if collapsed.is_empty() && offered.is_empty() {
                    self.nodes.set_text_fold_view(*id, None);
                } else {
                    self.nodes.set_text_fold_view(
                        *id,
                        Some(crate::store::TextFoldViewState { offered, collapsed }),
                    );
                }
                if changed {
                    self.mark(*id, DirtyMask::TEXT | DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputInlays { id, inlays } => {
                // 世界校验（钳除风格，同折叠先例）：锚点 char boundary 且
                // 不越界、文本非空且不含 '\n'，按 (offset, label) 排序去
                // 重。空集（含全部非法被钳除）移除条目，零分配待机。
                let value = self
                    .nodes
                    .text_input(*id)
                    .map(|state| state.value_shared())
                    .unwrap_or_default();
                let normalized = normalize_text_inlays(&value, inlays);
                let changed = self
                    .nodes
                    .text_inlays(*id)
                    .map(|fed| fed.as_ref() != normalized.as_slice())
                    .unwrap_or(!normalized.is_empty());
                if changed {
                    if normalized.is_empty() {
                        self.nodes.set_text_inlays(*id, None);
                    } else {
                        self.nodes.set_text_inlays(*id, Some(normalized.into()));
                    }
                    self.mark(*id, DirtyMask::TEXT | DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputSnippet { id, session } => {
                if self.nodes.text_snippet_session(*id) != session.as_ref() {
                    let previous_choices = self
                        .nodes
                        .text_snippet_session(*id)
                        .and_then(|s| s.choice_items());
                    let next_choices = session.as_ref().and_then(|s| s.choice_items());
                    if let Some(items) = next_choices {
                        self.nodes.set_text_completion_view(
                            *id,
                            Some(crate::store::TextCompletionViewState {
                                items,
                                selected: 0,
                                scroll: 0,
                                dismissed: false,
                            }),
                        );
                    } else if previous_choices.is_some() {
                        self.nodes.set_text_completion_view(*id, None);
                    }
                    self.nodes.set_text_snippet_session(*id, session.clone());
                    self.mark(*id, DirtyMask::TEXT | DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputCompletions { id, items } => {
                if items.is_empty() {
                    // 空列表关闭弹层：条目移除，零分配待机。
                    if self.nodes.text_completion_view(*id).is_some() {
                        self.nodes.set_text_completion_view(*id, None);
                        self.mark(*id, DirtyMask::RENDER);
                    }
                    return;
                }
                let next = match self.nodes.text_completion_view(*id) {
                    // 相同列表：无操作，键盘选中、滚动与 Esc 关闭态保持。
                    // 组件投影已过滤未变的喂入，这里的内容比较服务于直接
                    // 下发变更的调用方：内容相等即同一会话，不能降为指针
                    // 比较（换 Arc 重喂相同列表会被误判成新会话）。
                    Some(state) if state.items == *items => None,
                    // 不同列表：视为新会话（选中归零、重新打开）。
                    Some(_) | None => Some(crate::store::TextCompletionViewState {
                        items: Arc::clone(items),
                        selected: 0,
                        scroll: 0,
                        dismissed: false,
                    }),
                };
                if let Some(next) = next {
                    self.nodes.set_text_completion_view(*id, Some(next));
                    self.mark(*id, DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputCompletionView {
                id,
                selected,
                scroll,
            } => {
                let changed = self
                    .nodes
                    .text_completion_view(*id)
                    .is_some_and(|state| state.selected != *selected || state.scroll != *scroll);
                if changed {
                    if let Some(state) = self.nodes.text_completion_view_mut(*id) {
                        state.selected = *selected;
                        state.scroll = *scroll;
                    }
                    self.mark(*id, DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputCompletionDismissed { id } => {
                let changed = self
                    .nodes
                    .text_completion_view(*id)
                    .is_some_and(|state| !state.dismissed);
                if changed {
                    if let Some(state) = self.nodes.text_completion_view_mut(*id) {
                        state.dismissed = true;
                    }
                    self.mark(*id, DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputCompletionReopened { id } => {
                let changed = self
                    .nodes
                    .text_completion_view(*id)
                    .is_some_and(|state| state.dismissed);
                if changed {
                    if let Some(state) = self.nodes.text_completion_view_mut(*id) {
                        state.dismissed = false;
                        state.selected = 0;
                        state.scroll = 0;
                    }
                    self.mark(*id, DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputHover { id, hover } => {
                let changed = match (hover, self.nodes.text_hover_view(*id)) {
                    (None, None) => false,
                    (None, Some(_)) => true,
                    (Some(doc), Some(state)) => &state.doc != doc || state.scroll != 0,
                    (Some(_), None) => true,
                };
                if !changed {
                    return;
                }
                match hover {
                    Some(doc) => {
                        self.nodes.set_text_hover_view(
                            *id,
                            Some(crate::store::TextHoverViewState {
                                doc: doc.clone(),
                                scroll: 0,
                                diagnostic: false,
                            }),
                        );
                    }
                    None => self.nodes.set_text_hover_view(*id, None),
                }
                self.mark(*id, DirtyMask::RENDER);
            }
            UiMutation::SetTextInputDiagnosticHover { id, hover } => {
                let current = self.nodes.text_hover_view(*id);
                let changed = match (hover, current) {
                    (None, Some(state)) => state.diagnostic,
                    (None, None) => false,
                    (Some(doc), Some(state)) => !state.diagnostic || &state.doc != doc,
                    (Some(_), None) => true,
                };
                if !changed {
                    return;
                }
                match hover {
                    Some(doc) => {
                        let scroll = current
                            .filter(|state| state.diagnostic && state.doc == *doc)
                            .map(|state| state.scroll)
                            .unwrap_or(0);
                        self.nodes.set_text_hover_view(
                            *id,
                            Some(crate::store::TextHoverViewState {
                                doc: doc.clone(),
                                scroll,
                                diagnostic: true,
                            }),
                        );
                    }
                    None => {
                        if current.is_some_and(|state| state.diagnostic) {
                            self.nodes.set_text_hover_view(*id, None);
                        }
                    }
                }
                self.mark(*id, DirtyMask::RENDER);
            }
            UiMutation::SetTextInputSignature { id, help } => {
                let changed = match (help.as_ref(), self.nodes.text_signature(*id)) {
                    (None, None) => false,
                    (None, Some(_)) => true,
                    (Some(next), Some(fed)) => next != fed,
                    (Some(_), None) => true,
                };
                if changed {
                    self.nodes.set_text_signature(*id, help.clone());
                    self.mark(*id, DirtyMask::RENDER);
                }
            }
            UiMutation::SetTextInputHoverScroll { id, scroll } => {
                let changed = self
                    .nodes
                    .text_hover_view(*id)
                    .is_some_and(|state| state.scroll != *scroll);
                if changed {
                    if let Some(state) = self.nodes.text_hover_view_mut(*id) {
                        state.scroll = *scroll;
                    }
                    self.mark(*id, DirtyMask::RENDER);
                }
            }
        }
    }
}

impl UiWorld {
    /// A structural mutation that leaves the tree exactly as it is: an
    /// `Insert` that puts a child back where it already sits, or parking or
    /// detaching a root nothing holds any more.
    ///
    /// Applying one would still invalidate layout up the ancestor chain and
    /// retire the subtree from its document again. Refreshing a list by
    /// re-appending and re-parking what is already in place is the common
    /// case; skipping these is what makes it free.
    fn is_structural_noop(&self, mutation: &UiMutation) -> bool {
        match mutation {
            UiMutation::Insert {
                parent,
                child,
                before,
            } => {
                if self.nodes.get(*child).map(|node| node.hierarchy.parent) != Some(Some(*parent)) {
                    return false;
                }
                let Some(siblings) = self.nodes.get(*parent).map(|node| &node.hierarchy.children)
                else {
                    return false;
                };
                // `child`'s parent is `parent`, so it is in the list.
                match before {
                    None => siblings.last() == Some(child),
                    Some(before) if before == child => true,
                    Some(before) => siblings
                        .iter()
                        .position(|id| id == child)
                        .is_some_and(|index| siblings.get(index + 1) == Some(before)),
                }
            }
            UiMutation::ParkSubtree { root } => {
                self.unlinked_root(*root) && self.mount_state(*root) == Some(MountState::Parked)
            }
            UiMutation::Detach { id } => self.unlinked_root(*id),
            _ => false,
        }
    }

    /// Whether committing `queue` would change nothing: every mutation in it
    /// is a structural no-op. A no-op changes nothing, so judging each one
    /// against the state the batch starts from is exact.
    pub(crate) fn is_noop_batch(&self, queue: &MutationQueue) -> bool {
        queue
            .as_slice()
            .iter()
            .all(|mutation| self.is_structural_noop(mutation))
    }

    /// Detached or parked, with no parent: nothing left to unlink.
    fn unlinked_root(&self, id: StableNodeId) -> bool {
        self.detached.contains(&id)
            && self
                .nodes
                .get(id)
                .is_some_and(|node| node.hierarchy.parent.is_none())
    }

    pub(super) fn identity_and_parent(
        &self,
        id: StableNodeId,
    ) -> Result<(DocumentId, Option<StableNodeId>), UiWorldError> {
        let node = self.nodes.get(id).ok_or(UiWorldError::MissingNode(id))?;
        Ok((node.document, node.hierarchy.parent))
    }
}

impl UiWorld {
    /// Single-node creation and detached append have no staged cross-mutation
    /// state to simulate. Validate them directly so retained DOM/component
    /// construction does not scan the world or clone a growing child list.
    pub(super) fn validate_simple_mutation(
        &self,
        mutations: &[UiMutation],
    ) -> Result<bool, UiWorldError> {
        if let [UiMutation::Create { id, .. }] = mutations {
            if self.contains(*id) {
                return Err(UiWorldError::DuplicateNode(*id));
            }
            if self.is_retired(*id) {
                return Err(UiWorldError::RetiredNode(*id));
            }
            return Ok(true);
        }
        let [
            UiMutation::Insert {
                parent,
                child,
                before: None,
            },
        ] = mutations
        else {
            return Ok(false);
        };
        let (parent_document, _) = self.identity_and_parent(*parent)?;
        let (child_document, child_parent) = self.identity_and_parent(*child)?;
        if child_parent.is_some() {
            return Ok(false);
        }
        if parent_document != child_document {
            return Err(UiWorldError::CrossDocument {
                parent: *parent,
                child: *child,
            });
        }
        let mut ancestor = Some(*parent);
        let mut depth = 0usize;
        while let Some(id) = ancestor {
            if id == *child {
                return Err(UiWorldError::Cycle {
                    parent: *parent,
                    child: *child,
                });
            }
            depth += 1;
            ancestor = self.identity_and_parent(id)?.1;
        }
        // Near the depth limit the child's own height decides, and measuring it
        // is exactly the walk this fast path exists to avoid. Hand those rare
        // batches to the planner, which already bounds depth.
        Ok(depth < MAX_TREE_DEPTH)
    }
}

impl UiWorld {
    /// Borrowing variant of [`commit`]: validate-then-apply against a queue
    /// the caller still owns. Validation runs fully before the apply loop, so
    /// a rejected batch never lands partially and the caller may replay it.
    pub fn commit_ref(&mut self, queue: &MutationQueue) -> Result<CommitReport, UiWorldError> {
        self.commit_applying(queue, None)
    }

    /// `commit_ref` that also collects the root of every `Insert` and
    /// `ParkSubtree` it actually applied into `roots`.
    fn commit_applying(
        &mut self,
        queue: &MutationQueue,
        mut roots: Option<&mut Vec<StableNodeId>>,
    ) -> Result<CommitReport, UiWorldError> {
        let mut report = CommitReport::unchanged(self.generation, queue.len());
        if queue.is_empty() {
            return Ok(report);
        }
        let mut scanned = 0;
        let mut validated = Ok(());
        if !self.validate_simple_mutation(queue.as_slice())? {
            let mut plan = ValidationPlan::new(self);
            validated = plan.validate(queue.as_slice());
            scanned = plan.scanned;
        }
        self.validation_nodes_scanned = self.validation_nodes_scanned.saturating_add(scanned);
        validated?;
        // A skipped no-op changes nothing, so the generation moves with the
        // first mutation that does; a batch of nothing but no-ops keeps it.
        let mut applied = false;
        for mutation in queue.as_slice() {
            if self.is_structural_noop(mutation) {
                continue;
            }
            if !applied {
                applied = true;
                self.close_prior_animation_event_frame();
                self.generation = self.generation.wrapping_add(1);
                report.generation = self.generation;
            }
            self.apply(mutation, &mut report);
            if let Some(roots) = roots.as_deref_mut() {
                match mutation {
                    UiMutation::ParkSubtree { root } => roots.push(*root),
                    UiMutation::Insert { child, .. } => roots.push(*child),
                    _ => {}
                }
            }
        }
        if !applied {
            return Ok(report);
        }
        self.flush_scroll_content();
        self.remeasure_scroll_containers();
        for (id, offset) in std::mem::take(&mut self.scroll_requested) {
            if self.contains(id) {
                self.scroll_to_clamped(id, offset);
            }
        }
        Ok(report)
    }

    /// Move `id` to `offset` clamped to its scrolling area.
    fn scroll_to_clamped(&mut self, id: StableNodeId, offset: ScrollOffset) {
        let offset = self.clamp_scroll_offset(id, offset);
        let previous = self.record(id).scroll_offset;
        if previous != offset {
            self.record_mut(id).scroll_offset = offset;
            // Hit-index patch + Scene extract of this scroller only.
            // Descendants keep LayoutBox; paint uses scroll_offset.
            self.scroll_hit_updates
                .push((id, [previous.x - offset.x, previous.y - offset.y]));
            self.mark_scroll_compatible(id, DirtyMask::INPUT | DirtyMask::RENDER);
        }
    }

    /// Store `id`'s scrolling area and clamp its offset into it.
    fn store_scroll_metrics(&mut self, id: StableNodeId, metrics: Option<ScrollMetrics>) -> bool {
        self.nodes.set_scroll_metrics(id, metrics);
        let current = self.record(id).scroll_offset;
        let clamped = self.clamp_scroll_offset(id, current);
        if current == clamped {
            return false;
        }
        self.record_mut(id).scroll_offset = clamped;
        self.scroll_hit_updates
            .push((id, [current.x - clamped.x, current.y - clamped.y]));
        self.mark_scroll_compatible(id, DirtyMask::INPUT | DirtyMask::RENDER);
        true
    }

    /// Re-measure every scroll container whose content this commit moved,
    /// so none keeps a scrolling area its layout has left behind — whoever
    /// wrote the boxes. The content index already knows which: a container
    /// something under it moved is dirty there, so this costs one check per
    /// scroll container, not a walk per written box. A text editor's area
    /// follows its shaped value instead.
    fn remeasure_scroll_containers(&mut self) {
        let touched = std::mem::take(&mut self.scroll_layout_touched);
        if !touched && self.scroll_restyled.is_empty() {
            return;
        }
        let mut stale = std::mem::take(&mut self.scroll_restyled);
        if touched {
            self.scroll_containers
                .retain(|id| self.nodes.get(*id).is_some());
            let index = self.scroll_content_bounds.borrow();
            // All of them before any is measured: measuring an outer
            // container refreshes the inner ones' entries with it.
            stale.extend(
                self.scroll_containers
                    .iter()
                    .copied()
                    .filter(|id| index.stale(*id)),
            );
        }
        for id in stale {
            if !self.contains(id) {
                continue;
            }
            let scrolls = self.is_scroll_container(id);
            if scrolls && self.text_scroll_metrics(id).is_some() {
                continue;
            }
            // Nothing to measure before a box is laid out: keep what was
            // published. A container that stopped scrolling drops its area.
            let metrics = if scrolls {
                match self.layout_scroll_metrics(id) {
                    Some(metrics) => Some(metrics),
                    None => continue,
                }
            } else {
                None
            };
            let previous = self.nodes.scroll_metrics(id).copied();
            if previous == metrics {
                continue;
            }
            let viewport =
                |m: Option<ScrollMetrics>| m.map(|m| (m.viewport_width, m.viewport_height));
            if viewport(previous) != viewport(metrics) {
                self.scroll_resized.insert(id);
            }
            if self.store_scroll_metrics(id, metrics) {
                self.scroll_reclamped.insert(id);
            }
        }
    }

    fn track_scroll_container(&mut self, id: StableNodeId, scrolls: bool) {
        if scrolls {
            self.scroll_containers.insert(id);
        } else {
            self.scroll_containers.remove(&id);
        }
        self.scroll_restyled.push(id);
    }

    /// Scroll containers whose viewport size a re-measure changed since the
    /// last call, still in the world.
    pub(crate) fn take_scroll_resized(&mut self) -> Vec<StableNodeId> {
        let mut ids = std::mem::take(&mut self.scroll_resized)
            .into_iter()
            .filter(|id| self.contains(*id))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }

    /// Scroll containers whose offset a re-measure clamped since the last
    /// call, still in the world.
    pub(crate) fn take_scroll_reclamped(&mut self) -> Vec<StableNodeId> {
        let mut ids = std::mem::take(&mut self.scroll_reclamped)
            .into_iter()
            .filter(|id| self.contains(*id))
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids
    }
}

impl UiWorld {
    pub fn commit(&mut self, queue: MutationQueue) -> Result<CommitReport, UiWorldError> {
        self.commit_ref(&queue)
    }

    /// Commit through the normal atomic validator, then derive lifecycle work
    /// from the final topology. A descendant extracted from a parked ancestor
    /// must not inherit that ancestor's suspension bookkeeping.
    pub(crate) fn commit_with_mount_lifecycle(
        &mut self,
        queue: MutationQueue,
    ) -> Result<(CommitReport, HashSet<StableNodeId>, HashSet<StableNodeId>), UiWorldError> {
        // A root put back where it already was mounts or parks nothing, so
        // only the ones actually applied count.
        let mut roots = Vec::new();
        let mut report = self.commit_applying(&queue, Some(&mut roots))?;
        let mut parked = HashSet::new();
        let mut inserted = HashSet::new();
        let mut visited = HashSet::new();
        while let Some(id) = roots.pop() {
            if !self.contains(id) || !visited.insert(id) {
                continue;
            }
            // Insert roots can be nested in the final tree; visit their union
            // once instead of rescanning each ancestor's entire subtree.
            roots.extend(self.record(id).hierarchy.children.iter().copied());
            if self.mount_state(id) == Some(MountState::Parked) {
                parked.insert(id);
            } else if self.is_mounted(id) {
                inserted.insert(id);
            }
        }
        for &id in &parked {
            #[cfg(feature = "charts")]
            if let Some(mut visual) = self.standard_visual(id) {
                let active = match &mut visual {
                    StandardVisual::DonutChart { active, .. }
                    | StandardVisual::StackedTimeSeriesChart { active, .. } => Some(active),
                    _ => None,
                };
                if active.is_some_and(|active| active.take().is_some()) {
                    self.apply(
                        &UiMutation::SetStandardVisual {
                            id,
                            visual: Some(visual),
                        },
                        &mut report,
                    );
                    report.mutations += 1;
                }
            }
            let Some(StandardVisual::Icon {
                icon,
                size,
                tooltip: Some(mut tooltip),
            }) = self.standard_visual(id)
            else {
                continue;
            };
            if tooltip.open {
                tooltip.open = false;
                // This is a derived write to a validated, surviving node. Use
                // normal mutation application (dirty/extract accounting) within
                // the same generation, without a second observable commit.
                self.apply(
                    &UiMutation::SetStandardVisual {
                        id,
                        visual: Some(StandardVisual::Icon {
                            icon,
                            size,
                            tooltip: Some(tooltip),
                        }),
                    },
                    &mut report,
                );
                report.mutations += 1;
            }
        }
        Ok((report, parked, inserted))
    }
}

/// Replaces the text of every selection with `text` as one edit, a caret
/// after each insertion. The carets draw downstream: a Runtime selection
/// derived from bytes — every edit's included — does (see `TextSelection`).
fn replace_every_selection(session: &mut nana_text::EditSession, text: &str) {
    let primary = session.selection();
    let selections = session.selections().into_owned();
    let mut edits = Vec::with_capacity(selections.len());
    let mut carets = Vec::with_capacity(selections.len());
    let mut primary_caret = None;
    let mut shift = 0isize;
    for selection in selections {
        let range = selection.range();
        let start = (range.start as isize + shift) as usize;
        let caret = crate::TextSelection::caret(start + text.len());
        if selection == primary && primary_caret.is_none() {
            primary_caret = Some(caret);
        } else {
            carets.push(caret);
        }
        shift += text.len() as isize - range.len() as isize;
        edits.push((range, text));
    }
    let primary = primary_caret.unwrap_or_else(|| carets.remove(0));
    session.splice(&edits, primary, carets);
}

/// Applies `edit` to a composing session as Runtime's contract has it: the
/// preedit stands over the editor's current primary selection, so a write of
/// the text or selection moves it along instead of being refused (the
/// session refuses selection changes while composing) or cancelling it. The
/// preedit is lifted, the write applied, and the same preedit put back over
/// the resulting selection. Its lifting and restoring are not composition
/// updates: what the IME shows did not change.
///
/// Returns the session's work for the whole write.
fn edit_keeping_preedit(
    editor: &mut crate::store::EditorRecord,
    edit: impl FnOnce(&mut nana_text::EditSession),
) -> nana_text::TextWorkCounters {
    let preedit = editor
        .session
        .composition()
        .map(|composition| (composition.text.clone(), composition.selection.clone()));
    if preedit.is_some() {
        editor.session.cancel_composition();
    }
    edit(&mut editor.session);
    let restored = preedit.is_some();
    if let Some((text, selection)) = preedit {
        editor.session.set_preedit(&text, selection);
    }
    let mut work = editor.session.take_work();
    if restored {
        work.composition_updates = 0;
    }
    work
}
