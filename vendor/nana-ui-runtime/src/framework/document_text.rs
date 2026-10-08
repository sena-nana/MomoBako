//! Document-level text selection for `user-select: text | all | contain`.
//!
//! This is not a second TextInput: one grapheme-aligned range per document,
//! painted with author `::selection` colors when present (else theme accent),
//! copied through the same clipboard shortcut contract as editors.

use super::*;
use crate::text_editing::caret_offset_at_point;
use crate::{AccessibilityRole, DocumentTextSelection, StandardVisual, TextContent, TextShaper};

impl AppContext {
    /// UTF-8 of the current document selection, or `None` when empty.
    pub fn document_selected_text(&self, document: DocumentId) -> Option<String> {
        let selection = self.world.document_text_selection(document)?;
        if selection.is_empty() {
            return None;
        }
        let (start, end) = selection.ordered();
        let text = self.world.text(selection.node)?;
        if end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
            || start >= end
        {
            return None;
        }
        Some(text[start..end].to_owned())
    }

    pub fn clear_document_text_selection(&mut self, document: DocumentId) {
        self.world.set_document_text_selection(document, None);
        self.text_edit.document_select_drag = None;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn document_text_pointer_press(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
        shaper: &mut dyn TextShaper,
    ) -> Result<bool, FrameworkError> {
        let Some(node) = self.document_select_target_at(document, x, y) else {
            self.clear_document_text_selection(document);
            return Ok(false);
        };
        let Some(offset) = self.document_text_offset_at(node, x, y, shaper) else {
            self.clear_document_text_selection(document);
            return Ok(false);
        };
        self.text_edit.document_select_drag = Some((pointer_id, document, node, offset));
        if self
            .world
            .computed_style(node)
            .is_some_and(|style| style.user_select.selects_whole_node_on_press())
        {
            let len = self.world.text(node).map(str::len).unwrap_or(0);
            self.write_document_text_selection(document, node, 0, len, shaper);
        } else {
            self.write_document_text_selection(document, node, offset, offset, shaper);
        }
        Ok(true)
    }

    pub fn document_text_pointer_drag(
        &mut self,
        document: DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
        shaper: &mut dyn TextShaper,
    ) -> Result<bool, FrameworkError> {
        let Some((drag_id, drag_document, node, anchor)) = self.text_edit.document_select_drag
        else {
            return Ok(false);
        };
        if drag_id != pointer_id || drag_document != document {
            return Ok(false);
        }
        if self
            .world
            .computed_style(node)
            .is_some_and(|style| style.user_select.selects_whole_node_on_press())
        {
            let len = self.world.text(node).map(str::len).unwrap_or(0);
            self.write_document_text_selection(document, node, 0, len, shaper);
            return Ok(true);
        }
        // `text` and `contain` both keep the press node: offset updates on that
        // node's geometry even if the pointer is over a selectable neighbor.
        let Some(offset) = self.document_text_offset_at(node, x, y, shaper) else {
            return Ok(true);
        };
        self.write_document_text_selection(document, node, anchor, offset, shaper);
        Ok(true)
    }

    pub fn document_text_pointer_release(&mut self, pointer_id: u64) {
        if self
            .text_edit
            .document_select_drag
            .is_some_and(|(drag_id, _, _, _)| drag_id == pointer_id)
        {
            self.text_edit.document_select_drag = None;
        }
    }

    fn document_select_target_at(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
    ) -> Option<StableNodeId> {
        // Runtime `Text` is not hittable. A control or overlay that owns the
        // pointer must not start a drag-select underneath; a hittable layout
        // wrapper with no control semantics still yields nested `user-select`.
        if let Some(hit) = self.world.hit_test(document, x, y) {
            if self.is_document_select_target(hit) && self.document_select_hit(hit, x, y) {
                return Some(hit);
            }
            if self.document_select_consumes_pointer(hit) {
                return None;
            }
            return self.deepest_document_select_target(document, x, y, Some(hit));
        }
        self.deepest_document_select_target(document, x, y, None)
    }

    fn document_select_consumes_pointer(&self, id: StableNodeId) -> bool {
        if self.world.accessibility(id).is_some_and(|state| {
            matches!(
                state.role,
                AccessibilityRole::Button
                    | AccessibilityRole::Checkbox
                    | AccessibilityRole::Switch
                    | AccessibilityRole::Slider
                    | AccessibilityRole::ComboBox
                    | AccessibilityRole::TextInput
                    | AccessibilityRole::Tab
                    | AccessibilityRole::Radio
                    | AccessibilityRole::MenuItem
            )
        }) {
            return true;
        }
        matches!(
            self.world.standard_visual(id),
            Some(
                StandardVisual::Button { .. }
                    | StandardVisual::Checkbox { .. }
                    | StandardVisual::Switch { .. }
                    | StandardVisual::Range { .. }
                    | StandardVisual::TextInput { .. }
                    | StandardVisual::Select { .. }
                    | StandardVisual::SelectionOption { .. }
            )
        )
    }

    fn deepest_document_select_target(
        &self,
        document: DocumentId,
        x: f32,
        y: f32,
        under: Option<StableNodeId>,
    ) -> Option<StableNodeId> {
        let depth_of = |mut id: StableNodeId| {
            let mut depth = 0_u32;
            while let Some(parent) = self.world.parent_id(id) {
                depth += 1;
                id = parent;
            }
            depth
        };
        self.world
            .document_order(document)
            .into_iter()
            .filter(|&id| {
                under.is_none_or(|ancestor| self.world.is_descendant_or_self(id, ancestor))
                    && self.is_document_select_target(id)
                    && self.document_select_hit(id, x, y)
            })
            .max_by_key(|id| depth_of(*id))
    }

    fn is_document_select_target(&self, id: StableNodeId) -> bool {
        self.world.computed_style(id).is_some_and(|style| {
            style.box_visible
                && style.pointer_events.hittable()
                && style.user_select.allows_document_select()
        }) && self.world.text_input(id).is_none()
            && self.world.text(id).is_some_and(|text| !text.is_empty())
    }

    fn document_select_hit(&self, id: StableNodeId, x: f32, y: f32) -> bool {
        if let Some((local_x, local_y)) = self.world.pointer_layout_position(id, x, y) {
            return self
                .world
                .layout_box(id)
                .is_some_and(|bounds| bounds.contains(local_x, local_y));
        }
        self.world
            .viewport_layout_box(id)
            .or_else(|| self.world.layout_box(id))
            .is_some_and(|bounds| bounds.contains(x, y))
    }

    fn document_text_offset_at(
        &mut self,
        node: StableNodeId,
        x: f32,
        y: f32,
        shaper: &mut dyn TextShaper,
    ) -> Option<usize> {
        let (content, scroll) = self.world.document_text_pointer_context(node)?;
        let text = self.world.text(node)?.to_owned();
        if text.is_empty() {
            return Some(0);
        }
        let style = self.world.computed_style(node)?.clone();
        let constraints = self.world.text_shape_constraints(node);
        let shaped = TextContent { value: text.into() };
        let (layout_x, layout_y) = self
            .world
            .pointer_layout_position(node, x, y)
            .unwrap_or((x, y));
        let local_x = layout_x - content.x + scroll.x;
        let local_y = layout_y - content.y + scroll.y;
        if let Some((layout, box_width)) = self.world.vertical_document_text(node) {
            let (inline, block) = layout.line_space_point(local_x, local_y, box_width);
            return Some(
                layout
                    .hit_test_text(&shaped.value, inline, block)
                    .caret
                    .byte,
            );
        }
        // 静态可选文本只有 anchor / focus，没有 caret 可画，命中的 affinity
        // 在这里没有去处。
        if let Some(hit) =
            shaper.text_hit_at_point(node, &shaped, local_x, local_y, &style, constraints)
        {
            return Some(hit.offset);
        }
        Some(caret_offset_at_point(
            &shaped.value,
            local_x,
            local_y,
            |offset| shaper.text_position(node, &shaped, offset, &style, constraints),
        ))
    }

    fn write_document_text_selection(
        &mut self,
        document: DocumentId,
        node: StableNodeId,
        anchor: usize,
        focus: usize,
        shaper: &mut dyn TextShaper,
    ) {
        let Some(text) = self.world.text(node).map(str::to_owned) else {
            self.clear_document_text_selection(document);
            return;
        };
        let start = anchor.min(focus).min(text.len());
        let end = anchor.max(focus).min(text.len());
        let lines = self
            .world
            .document_text_highlight_lines(node, start, end, shaper);
        self.world.set_document_text_selection(
            document,
            Some(DocumentTextSelection {
                node,
                start,
                end,
                lines,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Button, ComputedStyle, LayoutBox, MeasureTextShaper, NodeStyle, ScrollAxes, ScrollOffset,
        ScrollView, Stack, Text, TextContent, TextMetrics, TextShapeConstraints, TextShaper,
    };
    use nana_ui_core::{LengthSpec, UserSelectSpec};
    use std::sync::Arc;

    struct ConstraintHighlightShaper;

    impl TextShaper for ConstraintHighlightShaper {
        fn shape(
            &mut self,
            id: StableNodeId,
            text: &TextContent,
            style: &ComputedStyle,
            constraints: TextShapeConstraints,
        ) -> TextMetrics {
            TextShaper::shape(&mut MeasureTextShaper, id, text, style, constraints)
        }

        fn text_highlights(
            &mut self,
            _id: StableNodeId,
            _text: &TextContent,
            selection: (usize, usize),
            _style: &ComputedStyle,
            constraints: TextShapeConstraints,
        ) -> Vec<LayoutBox> {
            if selection.0 >= selection.1 {
                return Vec::new();
            }
            vec![LayoutBox {
                x: 0.0,
                y: 0.0,
                width: constraints.max_width.unwrap_or(1000.0),
                height: 16.0,
            }]
        }
    }

    fn selectable_text(value: &str, font_size: Option<f32>, padding_left: Option<f32>) -> Text {
        let mut style = NodeStyle::default();
        {
            let layout = Arc::make_mut(&mut style.layout);
            layout.user_select = Some(UserSelectSpec::Text);
            if let Some(size) = font_size {
                layout.font_size = Some(size);
            }
            if let Some(padding) = padding_left {
                layout.padding_left = Some(LengthSpec::Px(padding));
            }
        }
        Text::new(value).style(style)
    }

    fn write_box(context: &mut AppContext, id: StableNodeId, layout: LayoutBox) {
        let mut mutations = MutationQueue::new();
        mutations.write_layout(id, layout);
        context.commit_mutations(mutations).unwrap();
        context.resolve_styles(&[id]).unwrap();
    }

    #[test]
    fn write_layout_then_shape_refreshes_document_selection_highlights() {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let label = context
            .create_component(document, selectable_text("Hello copy text", None, None))
            .unwrap();
        let node = label.stable_id();
        write_box(
            &mut context,
            node,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        let mut shaper = ConstraintHighlightShaper;
        context
            .document_text_pointer_press(document, 1, 2.0, 16.0, &mut shaper)
            .unwrap();
        context
            .document_text_pointer_drag(document, 1, 380.0, 16.0, &mut shaper)
            .unwrap();
        let before = context
            .world()
            .document_text_selection(document)
            .expect("selection")
            .lines
            .clone();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].width, 400.0);

        write_box(
            &mut context,
            node,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 96.0,
            },
        );
        let stale = context
            .world()
            .document_text_selection(document)
            .expect("selection")
            .lines
            .clone();
        assert_eq!(
            stale[0].width, 400.0,
            "WriteLayout keeps byte offsets; highlight rects wait for shape"
        );

        context.shape_text(&[node], &mut shaper).unwrap();
        let refreshed = context
            .world()
            .document_text_selection(document)
            .expect("selection")
            .lines
            .clone();
        assert_eq!(refreshed.len(), 1);
        assert_eq!(
            refreshed[0].width, 80.0,
            "shape after WriteLayout must rebuild highlight lines from the new content box"
        );
        assert_eq!(
            context.document_selected_text(document).as_deref(),
            Some("Hello copy text")
        );
    }

    #[test]
    fn document_select_hit_uses_content_box_not_padding() {
        let mut context = AppContext::new();
        let document = DocumentId::new(2).unwrap();
        let label = context
            .create_component(
                document,
                selectable_text("ABCDEFGHIJKLMNOPQRST", Some(10.0), Some(40.0)),
            )
            .unwrap();
        let node = label.stable_id();
        write_box(
            &mut context,
            node,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        let mut shaper = MeasureTextShaper;
        context
            .document_text_pointer_press(document, 1, 41.0, 8.0, &mut shaper)
            .unwrap();
        context
            .document_text_pointer_drag(document, 1, 200.0, 8.0, &mut shaper)
            .unwrap();
        let selected = context
            .document_selected_text(document)
            .expect("padded hit should select from the content origin");
        assert!(
            selected.starts_with("AB"),
            "padding must not shift the caret into later glyphs, got {selected:?}"
        );
        assert!(
            !selected.starts_with("EF"),
            "border-box hit would start ~4 glyphs later, got {selected:?}"
        );
    }

    #[test]
    fn document_select_target_follows_ancestor_scroll() {
        let mut context = AppContext::new();
        let document = DocumentId::new(3).unwrap();
        let scroll = context
            .create_component(document, ScrollView::new(ScrollAxes::Vertical))
            .unwrap();
        let label = context
            .create_component(document, selectable_text("Hello copy", None, None))
            .unwrap();
        context.append_child(scroll, label).unwrap();
        // Content below the label, so the scrolling area reaches 200.
        let spacer = context
            .create_component(document, Stack::column(0.0))
            .unwrap();
        context.append_child(scroll, spacer).unwrap();
        let mut mutations = MutationQueue::new();
        mutations.write_layout(
            scroll.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 80.0,
            },
        );
        mutations.write_layout(
            label.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 80.0,
                width: 200.0,
                height: 32.0,
            },
        );
        mutations.write_layout(
            spacer.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 112.0,
                width: 200.0,
                height: 88.0,
            },
        );
        context.commit_mutations(mutations).unwrap();
        context
            .resolve_styles(&[scroll.stable_id(), label.stable_id()])
            .unwrap();
        let mut scrolled = MutationQueue::new();
        scrolled.set_scroll_offset(scroll.stable_id(), ScrollOffset { x: 0.0, y: 60.0 });
        context.commit_mutations(scrolled).unwrap();

        let mut shaper = MeasureTextShaper;
        assert!(
            context
                .document_text_pointer_press(document, 1, 2.0, 25.0, &mut shaper)
                .unwrap(),
            "visual position after ancestor scroll must hit the text"
        );
        context
            .document_text_pointer_drag(document, 1, 180.0, 25.0, &mut shaper)
            .unwrap();
        assert_eq!(
            context.document_selected_text(document).as_deref(),
            Some("Hello copy")
        );
        assert!(
            !context
                .document_text_pointer_press(document, 2, 10.0, 90.0, &mut shaper)
                .unwrap(),
            "unscrolled layout box must not keep capturing after the ancestor scrolled"
        );
        assert!(context.document_selected_text(document).is_none());
    }

    #[test]
    fn hittable_wrapper_still_starts_document_selection_on_nested_text() {
        let mut context = AppContext::new();
        let document = DocumentId::new(9).unwrap();
        let wrapper = context
            .create_component(document, Stack::column(0.0).hittable())
            .unwrap();
        let label = context
            .create_component(document, selectable_text("Hello copy", None, None))
            .unwrap();
        context.append_child(wrapper, label).unwrap();
        write_box(
            &mut context,
            wrapper.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        write_box(
            &mut context,
            label.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        context.rebuild_hit_test(document);
        assert_eq!(
            context.pointer_target(document, 2.0, 16.0),
            Some(wrapper.stable_id()),
            "hittable layout wrapper owns the pointer"
        );
        let mut shaper = MeasureTextShaper;
        assert!(
            context
                .document_text_pointer_press(document, 1, 2.0, 16.0, &mut shaper)
                .unwrap(),
            "wrapper without control semantics must still start nested text selection"
        );
        context
            .document_text_pointer_drag(document, 1, 180.0, 16.0, &mut shaper)
            .unwrap();
        assert_eq!(
            context.document_selected_text(document).as_deref(),
            Some("Hello copy")
        );
    }

    #[test]
    fn button_over_text_does_not_start_document_selection() {
        let mut context = AppContext::new();
        let document = DocumentId::new(10).unwrap();
        let label = context
            .create_component(document, selectable_text("Hello copy", None, None))
            .unwrap();
        let button = context
            .create_component(document, Button::new("Go"))
            .unwrap();
        write_box(
            &mut context,
            label.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        write_box(
            &mut context,
            button.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        context.rebuild_hit_test(document);
        assert_eq!(
            context.pointer_target(document, 20.0, 16.0),
            Some(button.stable_id()),
            "button in front must own the pointer"
        );
        let mut shaper = MeasureTextShaper;
        assert!(
            !context
                .document_text_pointer_press(document, 1, 20.0, 16.0, &mut shaper)
                .unwrap(),
            "a control over text must not start a drag-select"
        );
        assert!(context.document_selected_text(document).is_none());
    }

    #[test]
    fn hittable_control_in_front_does_not_start_document_selection() {
        let mut context = AppContext::new();
        let document = DocumentId::new(4).unwrap();
        let label = context
            .create_component(document, selectable_text("Hello copy", None, None))
            .unwrap();
        let blocker = context
            .create_component(document, Stack::column(0.0).hittable())
            .unwrap();
        write_box(
            &mut context,
            label.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        write_box(
            &mut context,
            blocker.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        context.rebuild_hit_test(document);
        assert_eq!(
            context.pointer_target(document, 20.0, 16.0),
            Some(blocker.stable_id()),
            "hittable overlay must own the pointer"
        );
        let mut shaper = MeasureTextShaper;
        assert!(
            !context
                .document_text_pointer_press(document, 1, 20.0, 16.0, &mut shaper)
                .unwrap(),
            "document selection must not steal a click from a control in front"
        );
        assert!(context.document_selected_text(document).is_none());
    }

    #[test]
    fn user_select_none_clears_document_selection_after_style_resolve() {
        let mut context = AppContext::new();
        let document = DocumentId::new(5).unwrap();
        let label = context
            .create_component(document, selectable_text("Hello copy", None, None))
            .unwrap();
        let node = label.stable_id();
        write_box(
            &mut context,
            node,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        let mut shaper = MeasureTextShaper;
        context
            .document_text_pointer_press(document, 1, 2.0, 16.0, &mut shaper)
            .unwrap();
        context
            .document_text_pointer_drag(document, 1, 180.0, 16.0, &mut shaper)
            .unwrap();
        assert_eq!(
            context.document_selected_text(document).as_deref(),
            Some("Hello copy")
        );

        let mut style = context.world().node_style(node).cloned().unwrap();
        Arc::make_mut(&mut style.layout).user_select = Some(UserSelectSpec::None);
        let mut mutations = MutationQueue::new();
        mutations.set_style(node, style);
        context.commit_mutations(mutations).unwrap();
        context.resolve_styles(&[node]).unwrap();
        assert!(
            context.document_selected_text(document).is_none(),
            "user-select:none must drop the document selection, not leave a copyable range"
        );
    }

    #[test]
    fn document_select_all_selects_whole_text_on_press() {
        let mut context = AppContext::new();
        let document = DocumentId::new(6).unwrap();
        let mut style = NodeStyle::default();
        Arc::make_mut(&mut style.layout).user_select = Some(UserSelectSpec::All);
        let label = context
            .create_component(document, Text::new("Hello copy").style(style))
            .unwrap();
        let node = label.stable_id();
        write_box(
            &mut context,
            node,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        let mut shaper = MeasureTextShaper;
        assert!(
            context
                .document_text_pointer_press(document, 1, 2.0, 16.0, &mut shaper)
                .unwrap()
        );
        assert_eq!(
            context.document_selected_text(document).as_deref(),
            Some("Hello copy"),
            "user-select:all must select the whole text node on press"
        );
        context
            .document_text_pointer_drag(document, 1, 20.0, 16.0, &mut shaper)
            .unwrap();
        assert_eq!(
            context.document_selected_text(document).as_deref(),
            Some("Hello copy"),
            "user-select:all stays atomic while dragging"
        );
    }

    #[test]
    fn document_select_contain_does_not_extend_into_neighbor() {
        let mut context = AppContext::new();
        let document = DocumentId::new(7).unwrap();
        let mut contain_style = NodeStyle::default();
        Arc::make_mut(&mut contain_style.layout).user_select = Some(UserSelectSpec::Contain);
        let mut neighbor_style = NodeStyle::default();
        Arc::make_mut(&mut neighbor_style.layout).user_select = Some(UserSelectSpec::Text);
        let left = context
            .create_component(document, Text::new("LEFT").style(contain_style))
            .unwrap();
        let right = context
            .create_component(document, Text::new("RIGHT").style(neighbor_style))
            .unwrap();
        write_box(
            &mut context,
            left.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 32.0,
            },
        );
        write_box(
            &mut context,
            right.stable_id(),
            LayoutBox {
                x: 200.0,
                y: 0.0,
                width: 200.0,
                height: 32.0,
            },
        );
        let mut shaper = MeasureTextShaper;
        assert!(
            context
                .document_text_pointer_press(document, 1, 2.0, 16.0, &mut shaper)
                .unwrap()
        );
        context
            .document_text_pointer_drag(document, 1, 300.0, 16.0, &mut shaper)
            .unwrap();
        let selected = context
            .document_selected_text(document)
            .expect("contain must keep a range on the press node");
        assert!(
            selected.contains('L'),
            "contain drag should stay on LEFT, got {selected:?}"
        );
        assert!(
            !selected.contains('R'),
            "contain must not extend into a neighbor, got {selected:?}"
        );
    }

    #[test]
    fn document_select_uses_author_selection_colors() {
        let mut context = AppContext::new();
        let document = DocumentId::new(8).unwrap();
        let mut style = NodeStyle::default();
        {
            let layout = Arc::make_mut(&mut style.layout);
            layout.user_select = Some(UserSelectSpec::Text);
            layout.selection_background = Some([1.0, 0.0, 0.0, 1.0]);
            layout.selection_color = Some([0.0, 1.0, 0.0, 1.0]);
        }
        let label = context
            .create_component(document, Text::new("Hello copy").style(style))
            .unwrap();
        let node = label.stable_id();
        write_box(
            &mut context,
            node,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 32.0,
            },
        );
        let mut shaper = MeasureTextShaper;
        context
            .document_text_pointer_press(document, 1, 2.0, 16.0, &mut shaper)
            .unwrap();
        context
            .document_text_pointer_drag(document, 1, 180.0, 16.0, &mut shaper)
            .unwrap();
        let extracted = &context.world().extract_nodes(&[node])[0];
        assert_eq!(
            extracted.document_text_selection_color,
            [1.0, 0.0, 0.0, 1.0]
        );
        assert!(
            extracted
                .text_spans
                .iter()
                .any(|span| span.color == [0.0, 1.0, 0.0, 1.0] && span.start < span.end),
            "author ::selection color must overlay the selected range, spans={:?}",
            extracted.text_spans
        );
    }
}
