//! Accessible projection from retained nodes.

use super::*;

#[derive(Default)]
struct ProjectionMemo {
    transforms: hashbrown::HashMap<StableNodeId, AccessibleTransform>,
    bounds: hashbrown::HashMap<StableNodeId, Option<LayoutBox>>,
    chain: Vec<StableNodeId>,
}

#[derive(Clone, Copy)]
struct AccessibleTransform {
    own: ([f32; 6], [f32; 2]),
    children: ([f32; 6], [f32; 2]),
    blocks_3d: bool,
}

impl UiWorld {
    fn visible_accessibility_bounds(
        &self,
        id: StableNodeId,
        memo: &mut ProjectionMemo,
    ) -> Option<LayoutBox> {
        if let Some(bounds) = memo.bounds.get(&id) {
            return *bounds;
        }
        let bounds = self.compute_accessibility_bounds(id, memo);
        memo.bounds.insert(id, bounds);
        bounds
    }

    fn compute_accessibility_bounds(
        &self,
        id: StableNodeId,
        memo: &mut ProjectionMemo,
    ) -> Option<LayoutBox> {
        let local = if matches!(
            self.nodes.visual(id),
            Some(StandardVisual::ModalFrame { .. })
        ) {
            match self.component_geometry(id) {
                Some(crate::ComponentGeometry::ModalFrame { surface, .. }) => surface,
                _ => self.nodes.get(id)?.layout,
            }
        } else {
            self.nodes.get(id)?.layout
        };
        let mut bounds = self.accessibility_viewport_bounds(id, local, memo)?;
        if self.clip_visuals == 0 {
            return Some(bounds);
        }
        let mut parent = self.nodes.get(id)?.hierarchy.parent;
        while let Some(ancestor) = parent {
            if matches!(
                self.nodes.visual(ancestor),
                Some(StandardVisual::EmptyState { .. })
            ) {
                bounds = intersect_layout_boxes(
                    bounds,
                    self.accessibility_viewport_bounds(
                        ancestor,
                        self.nodes.get(ancestor)?.layout,
                        memo,
                    )?,
                )?;
            }
            if matches!(
                self.nodes.visual(ancestor),
                Some(StandardVisual::ModalFrame { .. })
            ) && let Some(crate::ComponentGeometry::ModalFrame { surface, body, .. }) =
                self.component_geometry(ancestor)
            {
                bounds = intersect_layout_boxes(
                    bounds,
                    self.accessibility_viewport_bounds(ancestor, surface, memo)?,
                )?;
                if let Some(StandardVisual::ModalFrame { slots, .. }) = self.nodes.visual(ancestor)
                    && slots
                        .body
                        .is_some_and(|body_root| self.is_descendant_or_self(id, body_root))
                {
                    bounds = intersect_layout_boxes(
                        bounds,
                        self.accessibility_viewport_bounds(ancestor, body, memo)?,
                    )?;
                }
            }
            parent = self.nodes.get(ancestor)?.hierarchy.parent;
        }
        Some(bounds)
    }
}

impl UiWorld {
    // Match build_hit_forest's composition from the current retained state.
    // A committed transform may precede the hit projection; that projection
    // is not an authority for accessibility coordinates. Memoizing ancestors
    // also avoids repeating hit-index ancestor walks for every projected node.
    fn accessibility_ancestor_transform(
        &self,
        id: StableNodeId,
        memo: &mut ProjectionMemo,
    ) -> Option<[f32; 6]> {
        if let Some(transform) = memo.transforms.get(&id) {
            return Some(transform.own.0);
        }
        memo.chain.clear();
        let mut current = Some(id);
        let mut cumulative = (IDENTITY_AFFINE, [0.0, 0.0]);
        let mut blocks_3d = false;
        while let Some(node) = current {
            if let Some(transform) = memo.transforms.get(&node) {
                cumulative = transform.children;
                blocks_3d = transform.blocks_3d;
                break;
            }
            memo.chain.push(node);
            current = self.nodes.get(node)?.hierarchy.parent;
        }
        while let Some(node) = memo.chain.pop() {
            let record = self.nodes.get(node)?;
            let bounds = record.layout;
            let layout = self.hit_motion_layout(node);
            if layout.position == PositionSpec::Fixed {
                // Viewport-relative; ancestors cannot scroll or clip this branch.
                cumulative = (IDENTITY_AFFINE, [0.0, 0.0]);
                blocks_3d = false;
            }
            let local = self.input_local_scene_transform(node, &layout, bounds, blocks_3d);
            cumulative = then_hit(cumulative, local);
            let own = cumulative;
            let scroll = record.scroll_offset;
            cumulative = then_hit(
                cumulative,
                ([1.0, 0.0, 0.0, 1.0, -scroll.x, -scroll.y], [0.0, 0.0]),
            );
            blocks_3d |= layout.fails_closed_3d_context();
            memo.transforms.insert(
                node,
                AccessibleTransform {
                    own,
                    children: cumulative,
                    blocks_3d,
                },
            );
        }
        memo.transforms.get(&id).map(|transform| transform.own.0)
    }

    /// AccessKit consumes document/viewport logical coordinates. Layout boxes
    /// deliberately stay unscrolled; compose current ancestor transforms so
    /// pointer and accessibility consumers agree after nested scrolling.
    fn accessibility_viewport_bounds(
        &self,
        id: StableNodeId,
        bounds: LayoutBox,
        memo: &mut ProjectionMemo,
    ) -> Option<LayoutBox> {
        let transform = self.accessibility_ancestor_transform(id, memo)?;
        let [a, b, c, d, e, f] = transform;
        let corners = [
            (bounds.x, bounds.y),
            (bounds.x + bounds.width, bounds.y),
            (bounds.x, bounds.y + bounds.height),
            (bounds.x + bounds.width, bounds.y + bounds.height),
        ]
        .map(|(x, y)| (a * x + c * y + e, b * x + d * y + f));
        if corners
            .iter()
            .any(|(x, y)| !x.is_finite() || !y.is_finite())
        {
            return None;
        }
        let x = corners.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
        let y = corners.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
        let right = corners
            .iter()
            .map(|p| p.0)
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = corners
            .iter()
            .map(|p| p.1)
            .fold(f32::NEG_INFINITY, f32::max);
        Some(LayoutBox {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

#[cfg(test)]
#[path = "accessibility_viewport_tests.rs"]
mod viewport_tests;

impl UiWorld {
    /// What `id` says to assistive technology as the name of a node it
    /// labels: its own label, or its text.
    fn accessible_text(&self, id: StableNodeId) -> Option<Arc<str>> {
        let node = self.nodes.get(id)?;
        node.accessibility
            .label
            .clone()
            .filter(|label| !label.is_empty())
            .or_else(|| {
                (!node.text.value.is_empty()).then(|| Arc::<str>::from(node.text.value.as_str()))
            })
    }

    fn project_accessibility_node(
        &self,
        id: StableNodeId,
        memo: &mut ProjectionMemo,
    ) -> Option<AccessibilityNode> {
        if !self.is_mounted(id) {
            return None;
        }
        let (parent, children, kind, state, text_value, document, visible, box_visible, writing) = {
            let node = self.nodes.get(id)?;
            (
                node.hierarchy.parent,
                Arc::clone(&node.hierarchy.children),
                Arc::clone(&node.kind),
                if node.resolved.0.visible {
                    node.accessibility.clone()
                } else {
                    AccessibilityState::default()
                },
                if node.resolved.0.visible {
                    node.text.value.clone()
                } else {
                    crate::TextValue::default()
                },
                node.document,
                node.resolved.0.visible,
                node.resolved.0.box_visible,
                node.resolved.0.writing_context(),
            )
        };
        // Keep a neutral structural container so visible descendants never
        // reference an omitted parent. Hidden leaves and omitted layout subtrees
        // contribute neither semantics nor structure.
        if !visible && (!box_visible || children.is_empty()) {
            return None;
        }
        if matches!(kind.as_ref(), NodeKind::Comment) {
            return None;
        }
        let role = if !visible {
            AccessibilityRole::Generic
        } else {
            match (state.role, kind.as_ref()) {
                (AccessibilityRole::Generic, NodeKind::Document) => AccessibilityRole::Document,
                (AccessibilityRole::Generic, NodeKind::Text) => AccessibilityRole::Text,
                (role, _) => role,
            }
        };
        let secure = matches!(
            self.nodes.visual(id),
            Some(StandardVisual::TextInput { secure: true, .. })
        );
        // A node another one names (a settings row's label beside its
        // switch) keeps a label of its own first, as `aria-label` wins over
        // `aria-labelledby`; an empty one names nothing. Its own text (a
        // select's shown option) is its value then, not its name.
        let label = match self.labelled_by(id) {
            Some(source) if visible => state
                .label
                .clone()
                .filter(|label| !label.is_empty())
                .or_else(|| self.accessible_text(source)),
            // A password field's text is its secret, not a name for it.
            _ => state.label.clone().or_else(|| {
                (!secure && !text_value.is_empty()).then(|| Arc::<str>::from(text_value.as_str()))
            }),
        };
        let bounds = self.visible_accessibility_bounds(id, memo)?;
        Some(AccessibilityNode {
            id,
            parent,
            children: children
                .iter()
                .copied()
                .filter(|child| {
                    let child_id = *child;
                    self.nodes.get(child_id).is_some_and(|node| {
                        node.resolved.0.box_visible
                            && (node.resolved.0.visible || !node.hierarchy.children.is_empty())
                            && !matches!(node.kind.as_ref(), NodeKind::Comment)
                    }) && self.visible_accessibility_bounds(child_id, memo).is_some()
                })
                .collect(),
            role,
            label,
            description: state.description.clone(),
            value: if !visible || secure {
                None
            } else {
                self.nodes
                    .text_input(id)
                    .map(|input| input.value_shared())
                    .or_else(|| state.value.as_ref().map(crate::TextValue::from))
            },
            disabled: visible
                && (state.disabled
                    || self
                        .confirm_action_effect(id)
                        .is_some_and(|effect| effect.0)),
            checked: state.checked,
            mixed: state.mixed,
            orientation: state.orientation,
            selected: state.selected,
            multiline: state.multiline,
            editable: state.editable,
            selection: if visible {
                self.nodes.text_input(id).map(|input| input.selection)
            } else {
                None
            },
            modal: state.modal,
            busy: state.busy,
            invalid: state.invalid,
            numeric_minimum: state.numeric_minimum,
            numeric_maximum: state.numeric_maximum,
            numeric_step: state.numeric_step,
            numeric_value: state.numeric_value,
            focused: visible && self.input.focused.get(&document) == Some(&id),
            bounds,
            writing,
        })
    }
}

impl UiWorld {
    /// Project one complete incremental accessibility transaction, including
    /// tombstones for nodes removed from the retained world.
    pub fn project_accessibility_delta(&self, work: &SystemWork) -> AccessibilityDelta {
        // Scrolling and transforms leave LayoutBox unchanged and may carry no
        // ACCESSIBILITY dirty bit. Their hit-test subtrees nevertheless moved
        // in viewport space, so the native accessibility cache must see them.
        //
        // Scheduled-layout nodes are deliberately NOT seeds, for the same
        // reason `RuntimeDocument::apply_hit_test_work` refuses them: layout
        // invalidation propagates to ancestors, so any leaf resize puts the
        // document root in `work.layout`, and expanding a root seed walks the
        // whole document. The nodes whose box actually moved are already here
        // by a different route -- layout writeback commits `WriteLayout`,
        // which marks INPUT | RENDER | ACCESSIBILITY on exactly those nodes,
        // shifted descendants included.
        //
        // These sets exist to produce a sorted, de-duplicated id sequence. A
        // `BTreeSet` pays a tree insert and node allocation per id to do that;
        // sorting a flat vec once yields the identical sequence for far less.
        let mut affected = work.accessibility.clone();
        // A node named by another one's text is projected again with it.
        let named = affected
            .iter()
            .filter_map(|id| self.labels.get(id))
            .flatten()
            .copied()
            .collect::<Vec<_>>();
        affected.extend(named);
        let mut pending = work
            .input_hit_test
            .iter()
            .chain(&work.transform)
            .copied()
            .collect::<Vec<_>>();
        // `visited` is membership-only, so it does not need ordering at all.
        let mut visited = hashbrown::HashSet::new();
        while let Some(id) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            affected.push(id);
            if let Some(node) = self.nodes.get(id) {
                pending.extend(node.hierarchy.children.iter().copied());
            }
        }
        affected.sort_unstable();
        affected.dedup();
        let mut removed = work.accessibility_removals.clone();
        let mut memo = ProjectionMemo::default();
        let mut updated = Vec::new();
        for id in affected {
            if let Some(node) = self.project_accessibility_node(id, &mut memo) {
                updated.push(node);
            } else if self.nodes.contains(id) {
                removed.push(id);
            }
        }
        removed.sort_unstable();
        removed.dedup();
        AccessibilityDelta {
            generation: work.generation,
            updated,
            removed,
        }
    }
}

impl UiWorld {
    /// Project only accessibility nodes named by scheduled dirty work.
    pub fn project_accessibility_nodes(&self, ids: &[StableNodeId]) -> Vec<AccessibilityNode> {
        let mut memo = ProjectionMemo::default();
        let mut projected = Vec::with_capacity(ids.len());
        projected.extend(
            ids.iter()
                .filter_map(|&id| self.project_accessibility_node(id, &mut memo)),
        );
        projected
    }
}

impl UiWorld {
    /// Project visible semantics and their neutral structural ancestors from
    /// the same retained authority, preserving a connected accessibility tree.
    pub fn project_accessibility(&self, document: DocumentId) -> Vec<AccessibilityNode> {
        self.project_accessibility_nodes(&self.document_order(document))
    }
}
