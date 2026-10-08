//! AppContext scroll operations.

use super::*;

impl AppContext {
    /// Change follow mode and immediately move to the current end when enabled.
    /// Later layout passes also follow any newly measured content extent.
    pub fn set_scroll_follow_end(
        &mut self,
        scroll: Entity<ScrollView>,
        enabled: bool,
    ) -> Result<bool, FrameworkError> {
        self.update_scroll_view_visual(scroll, |view, _| {
            view.follow_end = enabled;
            if enabled {
                // An explicit return-to-latest command supersedes a deferred
                // reading anchor, even if no subsequent layout is necessary.
                view.pending_anchor = None;
            }
        })?;
        if enabled {
            self.apply_scroll_retention(scroll)
        } else {
            Ok(false)
        }
    }

    /// Capture a retained descendant row's position before changing list data.
    pub fn capture_scroll_anchor(
        &self,
        scroll: Entity<ScrollView>,
        row: StableNodeId,
    ) -> Result<Option<crate::ScrollAnchor>, FrameworkError> {
        self.read(scroll, |_| ())?;
        if !self.scroll_contains_row(scroll.id, row) {
            return Ok(None);
        }
        let Some(viewport) = self.world.layout_box(scroll.id) else {
            return Ok(None);
        };
        let Some(bounds) = self.world.layout_box(row) else {
            return Ok(None);
        };
        let offset = self.world.scroll_offset(scroll.id).unwrap_or_default();
        Ok(Some(crate::ScrollAnchor {
            row,
            viewport_y: bounds.y - viewport.y - offset.y,
        }))
    }

    /// Restore after the next layout. Removed rows retain the clamped current
    /// offset. An explicit anchor takes precedence over following the end once.
    pub fn restore_scroll_anchor(
        &mut self,
        scroll: Entity<ScrollView>,
        anchor: crate::ScrollAnchor,
    ) -> Result<(), FrameworkError> {
        if !anchor.viewport_y.is_finite() {
            return Err(FrameworkError::InvalidInput);
        }
        self.update_scroll_view_visual(scroll, |view, _| view.pending_anchor = Some(anchor))?;
        // Anchor restoration depends on measured geometry. Include this
        // container in the next scoped layout even when its style is unchanged.
        self.world.mark_layout(scroll.id);
        Ok(())
    }

    fn scroll_contains_row(&self, scroll: StableNodeId, row: StableNodeId) -> bool {
        let mut parent = self.world.parent_id(row);
        while let Some(id) = parent {
            if id == scroll {
                return true;
            }
            parent = self.world.parent_id(id);
        }
        false
    }

    pub(super) fn apply_scroll_retention(
        &mut self,
        scroll: Entity<ScrollView>,
    ) -> Result<bool, FrameworkError> {
        let (follow, anchor, dragging) = self.read(scroll, |view| {
            (
                view.follow_end,
                view.pending_anchor,
                view.dragging.is_some(),
            )
        })?;
        if dragging {
            return Ok(false);
        }
        let current = self.world.scroll_offset(scroll.id).unwrap_or_default();
        if let Some(anchor) = anchor {
            self.update_scroll_view_visual(scroll, |view, _| view.pending_anchor = None)?;
            if self.scroll_contains_row(scroll.id, anchor.row)
                && let (Some(viewport), Some(row)) = (
                    self.world.layout_box(scroll.id),
                    self.world.layout_box(anchor.row),
                )
            {
                return self.scroll_to(
                    scroll,
                    ScrollOffset {
                        x: current.x,
                        y: row.y - viewport.y - anchor.viewport_y,
                    },
                );
            }
            return Ok(false);
        }
        if follow && let Some(metrics) = self.world.scroll_metrics(scroll.id) {
            return self.scroll_to(
                scroll,
                ScrollOffset {
                    x: current.x,
                    y: metrics.max_offset().y,
                },
            );
        }
        Ok(false)
    }

    fn emit_user_scroll(&mut self, scroll: Entity<ScrollView>) -> Result<(), FrameworkError> {
        let offset = self.world.scroll_offset(scroll.id).unwrap_or_default();
        let at_end = self
            .world
            .scroll_metrics(scroll.id)
            .is_some_and(|metrics| offset.y >= metrics.max_offset().y - 2.0);
        self.update(scroll, |_, cx| {
            cx.emit(crate::UserScroll { offset, at_end })
        })
    }

    // Hover/drag/retention own scrollbar state, not the style of a scrollport
    // borrowed by a Workspace region. Reuse the normal event/commit transaction.
    fn update_scroll_view_visual<R>(
        &mut self,
        entity: Entity<ScrollView>,
        update: impl FnOnce(&mut ScrollView, &mut ViewContext<'_, ScrollView>) -> R,
    ) -> Result<R, FrameworkError> {
        self.update_inner(entity, update, |scroll, world, mutations| {
            scroll.project_scrollbar_visual(entity.stable_id(), world, mutations);
        })
    }

    /// Send `scroll` a [`ScrollLaidOut`] after the next layout pass of its
    /// document.
    pub(crate) fn notify_laid_out(&mut self, scroll: Entity<ScrollView>) {
        self.laid_out_notices.insert(scroll.id);
    }

    pub fn scroll_to(
        &mut self,
        entity: Entity<ScrollView>,
        offset: ScrollOffset,
    ) -> Result<bool, FrameworkError> {
        if !offset.x.is_finite() || !offset.y.is_finite() {
            return Err(FrameworkError::InvalidComponentValue(entity.id));
        }
        let axes = self.read(entity, |scroll| scroll.axes)?;
        let offset = ScrollOffset {
            x: if matches!(axes, ScrollAxes::Horizontal | ScrollAxes::Both) {
                offset.x
            } else {
                0.0
            },
            y: if matches!(axes, ScrollAxes::Vertical | ScrollAxes::Both) {
                offset.y
            } else {
                0.0
            },
        };
        let offset = self.world.clamp_scroll_offset(entity.id, offset);
        if self.world.scroll_offset(entity.id) == Some(offset) {
            return Ok(false);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_scroll_offset(entity.id, offset);
        self.world.commit(mutations)?;
        self.update(entity, |_scroll, cx| {
            cx.emit(ScrollChanged { offset });
        })?;
        Ok(true)
    }

    /// Scrolls `target` into view inside `scroll`, moving the minimum distance.
    ///
    /// Uses the layout boxes published by the last layout pass, so call it
    /// after layout has run for the frame that created or moved `target`.
    /// A target already fully visible does not move the container.
    ///
    /// This does not materialize virtualized rows: for an off-screen row in a
    /// `materialize_virtual_*` list, query the target offset and materialize
    /// first, then call this once the row has a layout box.
    ///
    /// `margin` keeps that many logical pixels of context on the leading and
    /// trailing edge where the container has room for it.
    pub fn scroll_into_view(
        &mut self,
        scroll: Entity<ScrollView>,
        target: StableNodeId,
        margin: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(target_box) = self.world.layout_box(target) else {
            return Err(FrameworkError::MissingView(target));
        };
        let Some(view_box) = self.world.layout_box(scroll.id) else {
            return Err(FrameworkError::MissingView(scroll.id));
        };
        let offset = self.world.scroll_offset(scroll.id).unwrap_or_default();
        let margin = if margin.is_finite() {
            margin.max(0.0)
        } else {
            0.0
        };

        // Scrolling does not write back into `LayoutBox`, so a child's box is
        // its position within the content, independent of the current offset.
        // The offset is physical, so this reads the same on an axis whose
        // origin is the right / bottom; `scroll_to` clamps to the range.
        let axis = |target_start: f32,
                    target_extent: f32,
                    view_start: f32,
                    view_extent: f32,
                    current: f32| {
            let leading = target_start - view_start;
            let trailing = leading + target_extent;
            if leading - margin < current {
                leading - margin
            } else if trailing + margin > current + view_extent {
                // Never scroll so far that the leading edge leaves the viewport.
                (trailing + margin - view_extent).min(leading)
            } else {
                current
            }
        };

        let next = ScrollOffset {
            x: axis(
                target_box.x,
                target_box.width,
                view_box.x,
                view_box.width,
                offset.x,
            ),
            y: axis(
                target_box.y,
                target_box.height,
                view_box.y,
                view_box.height,
                offset.y,
            ),
        };
        self.scroll_to(scroll, next)
    }

    /// Publish measured scroll geometry and clamp an existing offset when the
    /// content or viewport shrinks. Metrics are Runtime-derived state, not a
    /// duplicate field on [`ScrollView`].
    pub fn set_scroll_metrics(
        &mut self,
        entity: Entity<ScrollView>,
        metrics: ScrollMetrics,
    ) -> Result<bool, FrameworkError> {
        self.read(entity, |_| ())?;
        if self.world.scroll_metrics(entity.id) == Some(metrics) {
            return Ok(false);
        }
        let previous = self.world.scroll_offset(entity.id).unwrap_or_default();
        let mut mutations = MutationQueue::new();
        mutations.set_scroll_metrics(entity.id, Some(metrics));
        self.world.commit(mutations)?;
        let offset = self.world.scroll_offset(entity.id).unwrap_or_default();
        if offset != previous {
            self.update(entity, |_scroll, cx| {
                cx.emit(ScrollChanged { offset });
            })?;
        }
        Ok(true)
    }

    /// Move one scroll container by logical-pixel content offsets.
    pub fn scroll_by(
        &mut self,
        entity: Entity<ScrollView>,
        delta: ScrollOffset,
    ) -> Result<bool, FrameworkError> {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return Err(FrameworkError::InvalidComponentValue(entity.id));
        }
        self.read(entity, |_| ())?;
        let current = self.world.scroll_offset(entity.id).unwrap_or_default();
        self.scroll_to(
            entity,
            ScrollOffset {
                x: current.x + delta.x,
                y: current.y + delta.y,
            },
        )
    }

    /// Route a logical-pixel scroll delta to the nearest hit scroll container.
    ///
    /// L2 [`ScrollView`] and L1 `overflow: auto|scroll` share [`ScrollOffset`].
    /// At a clamped edge the event bubbles to an enclosing container.
    /// Scrollbar chrome stays on [`ScrollView`] only.
    pub fn scroll_at(
        &mut self,
        document: DocumentId,
        x: f32,
        y: f32,
        delta: ScrollOffset,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        if !x.is_finite() || !y.is_finite() || !delta.x.is_finite() || !delta.y.is_finite() {
            return Err(FrameworkError::InvalidInput);
        }
        let hit = self.world.hit_test(document, x, y);
        self.scroll_from_hit(hit, delta)
    }

    /// [`Self::scroll_at`] with the point already hit-tested: scroll the
    /// nearest container at or above `hit` that can still move.
    pub fn scroll_from_hit(
        &mut self,
        hit: Option<StableNodeId>,
        delta: ScrollOffset,
    ) -> Result<Option<StableNodeId>, FrameworkError> {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return Err(FrameworkError::InvalidInput);
        }
        let mut current = hit;
        while let Some(id) = current {
            // The parent is read before scrolling: a scroll mutates the
            // container, never the hierarchy above it.
            let parent = self.world.parent_id(id);
            if self.scroll_node_by(id, delta)? {
                return Ok(Some(id));
            }
            current = parent;
        }
        Ok(None)
    }

    /// Whether a node is an L2 [`ScrollView`]. Scrollbar drag and hover chrome
    /// key off this; wheel also routes to L1 `overflow: auto|scroll` boxes.
    pub fn is_scroll_view(&self, id: StableNodeId) -> bool {
        self.views
            .get(&id)
            .is_some_and(|view| view.is::<ScrollView>())
    }

    /// Whether L1 `overflow: auto|scroll` applies on either axis.
    pub fn overflow_scrolls(&self, id: StableNodeId) -> bool {
        self.world.node_style(id).is_some_and(|style| {
            style.layout.overflow_x.scrolls() || style.layout.overflow_y.scrolls()
        })
    }

    pub(super) fn overflow_axes(&self, id: StableNodeId) -> Option<(bool, bool)> {
        let style = self.world.node_style(id)?;
        let x = style.layout.overflow_x.scrolls();
        let y = style.layout.overflow_y.scrolls();
        (x || y).then_some((x, y))
    }

    /// Move a [`ScrollView`] or L1 overflow scroller by `delta`. Returns
    /// `false` at a clamped edge so the caller can bubble.
    pub(crate) fn scroll_node_by(
        &mut self,
        id: StableNodeId,
        delta: ScrollOffset,
    ) -> Result<bool, FrameworkError> {
        if !delta.x.is_finite() || !delta.y.is_finite() {
            return Err(FrameworkError::InvalidInput);
        }
        if self
            .views
            .get(&id)
            .is_some_and(|view| view.is::<TextArea>())
        {
            // An editor scrolls once it has shaped a value to scroll over.
            if self.world.text_scroll_metrics(id).is_none() {
                return Ok(false);
            }
            let current = self.world.scroll_offset(id).unwrap_or_default();
            let next = self.world.clamp_scroll_offset(
                id,
                ScrollOffset {
                    x: current.x + delta.x,
                    y: current.y + delta.y,
                },
            );
            if self.world.scroll_offset(id).unwrap_or_default() == next {
                return Ok(false);
            }
            let mut mutations = MutationQueue::new();
            mutations.set_scroll_offset(id, next);
            self.world.commit(mutations)?;
            // Pinned to the request, which the caret reveal then yields to.
            let applied = self.world.scroll_request(id).unwrap_or(next);
            self.world.set_text_viewport_pin(id, Some(applied));
            self.update_component(Entity::<TextArea>::from_stable_id(id), |area, _| {
                area.scroll_offset = applied;
            })?;
            return Ok(true);
        }
        if self.is_scroll_view(id) {
            let scroll = Entity::from_stable_id(id);
            let changed = self.scroll_by(scroll, delta)?;
            if changed {
                self.emit_user_scroll(scroll)?;
            }
            return Ok(changed);
        }
        let Some((scrolls_x, scrolls_y)) = self.overflow_axes(id) else {
            return Ok(false);
        };
        let current = self.world.scroll_offset(id).unwrap_or_default();
        let next = self.world.clamp_scroll_offset(
            id,
            ScrollOffset {
                x: if scrolls_x {
                    current.x + delta.x
                } else {
                    current.x
                },
                y: if scrolls_y {
                    current.y + delta.y
                } else {
                    current.y
                },
            },
        );
        if next == current {
            return Ok(false);
        }
        let mut mutations = MutationQueue::new();
        mutations.set_scroll_offset(id, next);
        self.world.commit(mutations)?;
        Ok(true)
    }

    pub(super) fn scrollbar_bar(
        &self,
        id: StableNodeId,
        axis: nana_ui_core::ScrollbarAxis,
    ) -> Option<crate::ScrollbarBar> {
        match self.world.component_geometry(id) {
            Some(crate::ComponentGeometry::Scrollbar {
                horizontal,
                vertical,
            }) => match axis {
                nana_ui_core::ScrollbarAxis::Horizontal => horizontal,
                nana_ui_core::ScrollbarAxis::Vertical => vertical,
            },
            _ => None,
        }
    }

    /// Which scrollbar axis of a scroll container a viewport point lands on.
    ///
    /// The vertical bar wins an overlap, matching its drawn order.
    pub fn scrollbar_axis_at(
        &self,
        id: StableNodeId,
        x: f32,
        y: f32,
    ) -> Option<nana_ui_core::ScrollbarAxis> {
        [
            nana_ui_core::ScrollbarAxis::Vertical,
            nana_ui_core::ScrollbarAxis::Horizontal,
        ]
        .into_iter()
        .find(|axis| {
            self.scrollbar_bar(id, *axis)
                .is_some_and(|bar| bar.contains(x, y))
        })
    }

    /// Find the innermost scroll container whose scrollbar is under a point.
    ///
    /// Scrollbars overlay content, so the hit-test target is usually a child of
    /// the container that owns the bar.
    pub fn scrollbar_target_near(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
    ) -> Option<(StableNodeId, nana_ui_core::ScrollbarAxis)> {
        self.scrollbar_target_near_hit(x, y, self.world.hit_test(document, x, y))
    }

    /// [`Self::scrollbar_target_near`] with the point already hit-tested.
    pub fn scrollbar_target_near_hit(
        &self,
        x: f32,
        y: f32,
        hit: Option<StableNodeId>,
    ) -> Option<(StableNodeId, nana_ui_core::ScrollbarAxis)> {
        let mut current = hit;
        while let Some(id) = current {
            if let Some(axis) = self.scrollbar_axis_at(id, x, y) {
                return Some((id, axis));
            }
            current = self.world.parent_id(id);
        }
        None
    }

    /// Grab a scrollbar. A press on bare track pages toward the point first, so
    /// the thumb is under the pointer when the drag starts.
    pub fn begin_scrollbar_drag(
        &mut self,
        pointer_id: u64,
        target: StableNodeId,
        axis: nana_ui_core::ScrollbarAxis,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(bar) = self.scrollbar_bar(target, axis) else {
            return Ok(false);
        };
        let entity = Entity::<ScrollView>::from_stable_id(target);
        self.read(entity, |_| ())?;
        let position = bar.axis_position(axis, x, y);
        let track = bar.track_geometry(axis);
        // Cancel restores what the press started from, including any track jump.
        let initial_offset = self.world.scroll_offset(target).unwrap_or_default();
        let grab_offset = if track.thumb_contains(position) {
            position - track.thumb_origin
        } else {
            // Centre the thumb on the press, then keep dragging from there.
            let hold = self.axis_hold(target, axis);
            let offset = track.offset_for_position(position);
            if self.scroll_to(entity, scroll_offset_on(axis, offset, hold))? {
                self.emit_user_scroll(entity)?;
            }
            track.thumb_length / 2.0
        };
        self.update_scroll_view_visual(entity, |scroll, cx| {
            scroll.dragging = Some(crate::ScrollbarDragState {
                pointer_id,
                axis,
                grab_offset,
                initial_offset,
            });
            cx.mutations().capture_pointer(pointer_id, target);
        })?;
        Ok(true)
    }

    /// The offset on the axis a drag is not touching, so it stays put.
    pub(super) fn axis_hold(&self, id: StableNodeId, axis: nana_ui_core::ScrollbarAxis) -> f32 {
        let offset = self.world.scroll_offset(id).unwrap_or_default();
        match axis {
            nana_ui_core::ScrollbarAxis::Horizontal => offset.y,
            nana_ui_core::ScrollbarAxis::Vertical => offset.x,
        }
    }

    pub fn update_scrollbar_drag(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world.pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        if !self.is_scroll_view(target) {
            return Ok(false);
        }
        let entity = Entity::<ScrollView>::from_stable_id(target);
        let Some(drag) = self.read(entity, |scroll| scroll.dragging)? else {
            return Ok(false);
        };
        if drag.pointer_id != pointer_id {
            return Ok(false);
        }
        let Some(bar) = self.scrollbar_bar(target, drag.axis) else {
            return Ok(false);
        };
        let track = bar.track_geometry(drag.axis);
        let offset =
            track.offset_for_thumb_origin(bar.axis_position(drag.axis, x, y) - drag.grab_offset);
        let hold = self.axis_hold(target, drag.axis);
        let changed = self.scroll_to(entity, scroll_offset_on(drag.axis, offset, hold))?;
        if changed {
            self.emit_user_scroll(entity)?;
        }
        Ok(changed)
    }

    pub fn end_scrollbar_drag(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        cancel: bool,
    ) -> Result<bool, FrameworkError> {
        let Some(target) = self.world.pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        if !self.is_scroll_view(target) {
            return Ok(false);
        }
        let entity = Entity::<ScrollView>::from_stable_id(target);
        let Some(drag) = self.read(entity, |scroll| scroll.dragging)? else {
            return Ok(false);
        };
        if drag.pointer_id != pointer_id {
            return Ok(false);
        }
        if cancel {
            self.scroll_to(entity, drag.initial_offset)?;
        }
        self.update_scroll_view_visual(entity, |scroll, cx| {
            scroll.dragging = None;
            cx.mutations().release_pointer(pointer_id, target);
        })?;
        Ok(true)
    }

    /// Reveal auto-hiding scrollbars for the container under the pointer.
    pub(super) fn sync_scroll_view_hover(
        &mut self,
        previous: Option<StableNodeId>,
        target: Option<StableNodeId>,
    ) -> Result<(), FrameworkError> {
        let entered = target.and_then(|id| self.enclosing_scroll_view(id));
        let left = previous.and_then(|id| self.enclosing_scroll_view(id));
        if left == entered {
            return Ok(());
        }
        if let Some(id) = left {
            self.set_scroll_view_hover(id, false)?;
        }
        if let Some(id) = entered {
            self.set_scroll_view_hover(id, true)?;
        }
        Ok(())
    }

    pub(super) fn set_scroll_view_hover(
        &mut self,
        id: StableNodeId,
        hovered: bool,
    ) -> Result<(), FrameworkError> {
        let entity = Entity::<ScrollView>::from_stable_id(id);
        if self.read(entity, |scroll| scroll.hovered)? == hovered {
            return Ok(());
        }
        self.update_scroll_view_visual(entity, |scroll, _| {
            scroll.hovered = hovered;
        })?;
        Ok(())
    }

    pub(super) fn enclosing_scroll_view(&self, id: StableNodeId) -> Option<StableNodeId> {
        let mut current = Some(id);
        while let Some(id) = current {
            if self.is_scroll_view(id) {
                return Some(id);
            }
            current = self.world.parent_id(id);
        }
        None
    }
}
