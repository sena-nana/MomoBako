//! Vertical list with thresholded reorder and optional tree-drop intents.
//!
//! Application code owns item identities, order, grouping, and persistence.
//! This type reports select, before-value reorder, and tree-drop results from
//! pointer geometry only.

use std::sync::Arc;

#[cfg(test)]
use nana_ui_core::UI_METRICS;
use nana_ui_core::{ControlSize, FlexDirection, LengthSpec, reorder_changes_position};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, ComponentView, InteractionState, LayoutBox,
    MutationQueue, NodeKind, NodeStyle, StableNodeId, StandardVisual, UiWorld,
};

const DRAG_THRESHOLD: f32 = nana_ui_core::space::XS;
const DEFAULT_SPACING: f32 = nana_ui_core::space::XXS;
const INSERT_INSET: f32 = nana_ui_core::space::XS;
const INSERT_THICKNESS: f32 = nana_ui_core::space::XXS;

/// Placement resolved for a tree drop target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeDropPosition {
    Before,
    Inside,
    After,
}

/// Framework-owned tree drop intent. Applications keep node semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeDropIntent {
    pub target: Arc<str>,
    pub position: TreeDropPosition,
}

/// Results published by [`ReorderList`]. Order and persistence stay with the
/// application.
#[derive(Debug, Clone, PartialEq)]
pub enum ReorderListEvent {
    Select(Arc<str>),
    /// Secondary (right) press resolved to a row body. Row surfaces project
    /// with `pointer_events: none` (the list owns drag and selection), so the
    /// hit node above the row can never reach a row handler while bubbling;
    /// [`AppContext::secondary_press_at`] resolves the row under the point and
    /// publishes this event on the list instead. `x`/`y` are the press point
    /// in window coordinates, ready to anchor a context menu.
    Secondary {
        source: Arc<str>,
        x: f32,
        y: f32,
    },
    Reorder {
        source: Arc<str>,
        before: Option<Arc<str>>,
    },
    TreeDrop {
        source: Arc<str>,
        intent: TreeDropIntent,
    },
    Cancelled,
}

/// Pointer phases consumed by [`ReorderList::apply_pointer`].
///
/// Escape, unfocus, and lost-touch should be delivered as [`Self::Cancel`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReorderListPointer {
    Down { x: f32, y: f32 },
    Move { x: f32, y: f32 },
    Up { x: f32, y: f32 },
    Cancel,
}

/// One row identity. Optional [`Self::tools`] is a live child that keeps its
/// own pointer handling; hits there do not start a drag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderItem {
    pub value: Arc<str>,
    pub label: Arc<str>,
    pub draggable: bool,
    pub drop_target: bool,
    pub selected: bool,
    pub disabled: bool,
    pub tools: Option<StableNodeId>,
}

impl ReorderItem {
    pub fn new(value: impl Into<Arc<str>>, label: impl Into<Arc<str>>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            draggable: true,
            drop_target: true,
            selected: false,
            disabled: false,
            tools: None,
        }
    }

    /// Sets whether this row can start a drag. Also sets [`Self::drop_target`]
    /// to the same value; call [`Self::drop_target`] afterwards for drop-only
    /// rows.
    pub fn draggable(mut self, draggable: bool) -> Self {
        self.draggable = draggable;
        self.drop_target = draggable;
        self
    }

    pub fn drop_target(mut self, drop_target: bool) -> Self {
        self.drop_target = drop_target;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Interactive trailing tools. Pointer hits inside this child's layout box
    /// do not begin a reorder gesture.
    pub fn tools(mut self, tools: StableNodeId) -> Self {
        self.tools = Some(tools);
        self
    }

    fn is_source(&self) -> bool {
        self.draggable && !self.disabled
    }

    fn is_drop_target(&self) -> bool {
        self.drop_target && !self.disabled
    }
}

#[derive(Debug, Clone, PartialEq)]
struct ReorderDrag {
    source: Arc<str>,
    start_x: f32,
    start_y: f32,
    x: f32,
    y: f32,
    moved: bool,
}

/// One painted row. Application identities stay on [`ReorderItem`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReorderRowPaint {
    pub label: Arc<str>,
    pub selected: bool,
    pub disabled: bool,
}

/// Vertical list. NanaUI does not mutate item order on reorder or tree drop.
#[derive(Debug, Clone, PartialEq)]
pub struct ReorderList {
    pub items: Vec<ReorderItem>,
    pub spacing: f32,
    pub size: ControlSize,
    pub tree_drop: bool,
    /// Declared at construction: live row children own painting and the list
    /// body follows their layout. Never inferred from the tree at project time.
    pub live_rows: bool,
    pub label: Option<Arc<str>>,
    pub style: NodeStyle,
    drag: Option<ReorderDrag>,
}

impl ReorderList {
    pub fn new(items: impl IntoIterator<Item = ReorderItem>) -> Self {
        Self {
            items: items.into_iter().collect(),
            spacing: DEFAULT_SPACING,
            size: ControlSize::Small,
            tree_drop: false,
            live_rows: false,
            label: None,
            style: NodeStyle::default(),
            drag: None,
        }
    }

    pub fn spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing.max(0.0);
        self
    }

    pub fn size(mut self, size: ControlSize) -> Self {
        self.size = size;
        self
    }

    pub fn tree_drop(mut self, enabled: bool) -> Self {
        self.tree_drop = enabled;
        self
    }

    /// Declare that the host attaches live row children which own painting.
    /// Retained `items` stay the drag/hit-test model and are never painted as
    /// self-drawn rows in this mode.
    pub fn live_rows(mut self, live_rows: bool) -> Self {
        self.live_rows = live_rows;
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn selected_value(&self) -> Option<&Arc<str>> {
        self.items
            .iter()
            .find(|item| item.selected)
            .map(|item| &item.value)
    }

    pub fn is_dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Uniform row rectangles stacked from the top of `bounds`.
    pub fn row_bounds(
        &self,
        bounds: LayoutBox,
        metrics: nana_ui_core::ThemeMetrics,
    ) -> Vec<LayoutBox> {
        let height = self.size.height_in(metrics);
        let spacing = self.spacing.max(0.0);
        self.items
            .iter()
            .enumerate()
            .map(|(index, _)| LayoutBox {
                x: bounds.x,
                y: bounds.y + index as f32 * (height + spacing),
                width: bounds.width,
                height,
            })
            .collect()
    }

    /// 4px threshold, source/target hit testing, and terminal events.
    pub fn apply_pointer(
        &mut self,
        pointer: ReorderListPointer,
        bounds: LayoutBox,
        metrics: nana_ui_core::ThemeMetrics,
    ) -> Option<ReorderListEvent> {
        let rows = self.row_bounds(bounds, metrics);
        self.apply_pointer_with_rows(pointer, &rows, &[])
    }

    /// Same as [`Self::apply_pointer`], with caller-supplied row boxes and
    /// reserved tool boxes that must not start a drag.
    pub fn apply_pointer_with_rows(
        &mut self,
        pointer: ReorderListPointer,
        rows: &[LayoutBox],
        exclude: &[LayoutBox],
    ) -> Option<ReorderListEvent> {
        match pointer {
            ReorderListPointer::Down { x, y } => self.begin(x, y, rows, exclude),
            ReorderListPointer::Move { x, y } => {
                self.move_to(x, y);
                None
            }
            ReorderListPointer::Up { x, y } => {
                self.move_to(x, y);
                self.finish(rows)
            }
            ReorderListPointer::Cancel => self.cancel(),
        }
    }

    pub fn paint_rows(&self) -> Arc<[ReorderRowPaint]> {
        self.items
            .iter()
            .map(|item| ReorderRowPaint {
                label: Arc::clone(&item.label),
                selected: item.selected,
                disabled: item.disabled,
            })
            .collect()
    }

    /// Insert-line (or inside highlight) for the active drop.
    pub fn insert_line(
        &self,
        bounds: LayoutBox,
        metrics: nana_ui_core::ThemeMetrics,
    ) -> Option<LayoutBox> {
        let drag = self.drag.as_ref().filter(|drag| drag.moved)?;
        let source = self.item_index(&drag.source)?;
        if !self.items.get(source).is_some_and(ReorderItem::is_source) {
            return None;
        }
        let rows = self.row_bounds(bounds, metrics);
        let drop_targets = self.drop_target_flags();
        if self.tree_drop {
            let (target, position) =
                tree_drop_target(&rows, &drop_targets, Some(source), drag.x, drag.y)?;
            return Some(tree_insert_line(rows[target], position));
        }
        let before = drop_before_index(&rows, &drop_targets, Some(source), drag.y);
        if !reorder_changes_position(self.items.len(), source, before) {
            return None;
        }
        Some(reorder_insert_line(bounds, &rows, before))
    }

    /// Clears an in-flight gesture. Escape, unfocus, and lost-touch use this.
    pub fn cancel(&mut self) -> Option<ReorderListEvent> {
        self.drag.take().map(|_| ReorderListEvent::Cancelled)
    }

    fn begin(
        &mut self,
        x: f32,
        y: f32,
        rows: &[LayoutBox],
        exclude: &[LayoutBox],
    ) -> Option<ReorderListEvent> {
        if self.drag.is_some() || !point_finite(x, y) {
            return None;
        }
        if exclude.iter().any(|reserved| reserved.contains(x, y)) {
            return None;
        }
        let sources = self
            .items
            .iter()
            .map(ReorderItem::is_source)
            .collect::<Vec<_>>();
        let index = item_at(rows, &sources, x, y)?;
        self.drag = Some(ReorderDrag {
            source: Arc::clone(&self.items[index].value),
            start_x: x,
            start_y: y,
            x,
            y,
            moved: false,
        });
        None
    }

    fn move_to(&mut self, x: f32, y: f32) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        if !point_finite(x, y) {
            return;
        }
        drag.x = x;
        drag.y = y;
        let dx = x - drag.start_x;
        let dy = y - drag.start_y;
        drag.moved |= dx * dx + dy * dy >= DRAG_THRESHOLD * DRAG_THRESHOLD;
    }

    fn finish(&mut self, rows: &[LayoutBox]) -> Option<ReorderListEvent> {
        let drag = self.drag.take()?;
        let source = self.item_index(&drag.source)?;
        if !self.items[source].is_source() {
            return None;
        }
        if !drag.moved {
            self.set_selected(drag.source.as_ref());
            return Some(ReorderListEvent::Select(drag.source));
        }
        let drop_targets = self.drop_target_flags();
        if self.tree_drop {
            let (target, position) =
                tree_drop_target(rows, &drop_targets, Some(source), drag.x, drag.y)?;
            let target = Arc::clone(&self.items[target].value);
            return Some(ReorderListEvent::TreeDrop {
                source: drag.source,
                intent: TreeDropIntent { target, position },
            });
        }
        let before = drop_before_index(rows, &drop_targets, Some(source), drag.y);
        if !reorder_changes_position(self.items.len(), source, before) {
            return None;
        }
        let before = before.map(|index| Arc::clone(&self.items[index].value));
        Some(ReorderListEvent::Reorder {
            source: drag.source,
            before,
        })
    }

    fn item_index(&self, value: &str) -> Option<usize> {
        self.items
            .iter()
            .position(|item| item.value.as_ref() == value)
    }

    fn drop_target_flags(&self) -> Vec<bool> {
        self.items.iter().map(ReorderItem::is_drop_target).collect()
    }

    fn set_selected(&mut self, value: &str) {
        for item in &mut self.items {
            item.selected = item.value.as_ref() == value && !item.disabled;
        }
    }

    fn selected_label(&self) -> Option<Arc<str>> {
        self.items
            .iter()
            .find(|item| item.selected)
            .map(|item| Arc::clone(&item.label))
    }

    fn intrinsic_height(&self, metrics: nana_ui_core::ThemeMetrics) -> f32 {
        let count = self.items.len().max(1) as f32;
        count * self.size.height_in(metrics) + (count - 1.0) * self.spacing.max(0.0)
    }
}

impl Default for ReorderList {
    fn default() -> Self {
        Self::new([])
    }
}

impl ComponentView for ReorderList {
    fn share_layouts(
        &mut self,
        share: &mut dyn FnMut(&mut std::sync::Arc<nana_ui_core::LayoutStyle>),
    ) {
        share(&mut self.style.layout);
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "reorder-list".into(),
        }
    }

    fn wants_child_reproject() -> bool {
        true
    }

    fn wants_metrics_reproject() -> bool {
        true
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        // Live row children paint themselves; retained items only feed
        // hit testing and reorder events.
        let live_rows = self.live_rows;
        let visual = StandardVisual::ReorderList {
            rows: if live_rows {
                Arc::<[ReorderRowPaint]>::from([])
            } else {
                self.paint_rows()
            },
            size: self.size,
            spacing: self.spacing,
            insert: None,
        };
        if world.standard_visual(id) != Some(visual.clone()) {
            mutations.set_standard_visual(id, Some(visual));
        }
        let mut style = self.style.clone();
        let layout = Arc::make_mut(&mut style.layout);
        if layout.width.is_none() {
            layout.width = Some(LengthSpec::Fill);
        }
        if live_rows {
            layout.direction = Some(FlexDirection::Column);
            if layout.gap.is_none() {
                layout.gap = Some(LengthSpec::Px(self.spacing.max(0.0)));
            }
            if layout.height.is_none() {
                layout.height = Some(LengthSpec::Shrink);
            }
        } else {
            // Border-box: declared padding is added to the self-drawn row
            // stack instead of clipping its last row.
            let padding = layout.resolved_padding();
            let content =
                self.intrinsic_height(world.theme_metrics()) + padding.top + padding.bottom;
            if layout.height.is_none() {
                layout.height = Some(LengthSpec::Px(content));
            }
            if layout.min_height.is_none() {
                layout.min_height = Some(LengthSpec::Px(content));
            }
        }
        project_common(
            id,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::List,
                label: self.label.clone().or_else(|| self.selected_label()),
                value: self.selected_value().cloned(),
                disabled: false,
                ..AccessibilityState::default()
            },
        );
    }
}

impl crate::AppContext {
    pub fn is_reorder_list(&self, id: StableNodeId) -> bool {
        self.read(crate::Entity::<ReorderList>::from_stable_id(id), |_| ())
            .is_ok()
    }

    pub fn nearest_reorder_list(&self, mut id: StableNodeId) -> Option<StableNodeId> {
        loop {
            if self.is_reorder_list(id) {
                return Some(id);
            }
            id = self.world().parent_id(id)?;
        }
    }

    pub fn begin_reorder_list_pointer(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        target: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(list_id) = self.nearest_reorder_list(target) else {
            return Ok(false);
        };
        let Some(entity) = self.reorder_list_entity(list_id) else {
            return Ok(false);
        };
        let Some(bounds) = self.world().layout_box(list_id) else {
            return Ok(false);
        };
        let rows = self.reorder_row_boxes(list_id, bounds);
        let exclude = self.reorder_tool_boxes(entity);
        if exclude.iter().any(|reserved| reserved.contains(x, y)) {
            return Ok(false);
        }
        self.update_component(entity, |list, cx| {
            list.apply_pointer_with_rows(ReorderListPointer::Down { x, y }, &rows, &exclude);
            if list.is_dragging() {
                cx.mutations().capture_pointer(pointer_id, list_id);
                cx.mutations().request_focus(document, Some(list_id));
                true
            } else {
                false
            }
        })
    }

    pub fn update_reorder_list_pointer(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(entity) = self.reorder_list_entity(target) else {
            return Ok(false);
        };
        let Some(bounds) = self.world().layout_box(target) else {
            return Ok(false);
        };
        let rows = self.reorder_row_boxes(target, bounds);
        self.update_component(entity, |list, _| {
            list.apply_pointer_with_rows(ReorderListPointer::Move { x, y }, &rows, &[]);
            list.is_dragging()
        })
    }

    pub fn end_reorder_list_pointer(
        &mut self,
        document: crate::DocumentId,
        pointer_id: u64,
        x: f32,
        y: f32,
        cancel: bool,
    ) -> Result<bool, crate::FrameworkError> {
        let Some(target) = self.world().pointer_capture(document, pointer_id) else {
            return Ok(false);
        };
        let Some(entity) = self.reorder_list_entity(target) else {
            return Ok(false);
        };
        let bounds = self.world().layout_box(target);
        let rows = bounds
            .map(|bounds| self.reorder_row_boxes(target, bounds))
            .unwrap_or_default();
        self.update_component(entity, |list, cx| {
            let pointer = if cancel {
                ReorderListPointer::Cancel
            } else {
                ReorderListPointer::Up { x, y }
            };
            if let Some(event) = list.apply_pointer_with_rows(pointer, &rows, &[]) {
                cx.emit(event);
            }
            cx.mutations().release_pointer(pointer_id, target);
            true
        })
    }

    fn reorder_list_entity(&self, id: StableNodeId) -> Option<crate::Entity<ReorderList>> {
        self.is_reorder_list(id)
            .then(|| crate::Entity::from_stable_id(id))
    }

    fn reorder_row_boxes(&self, id: StableNodeId, bounds: LayoutBox) -> Vec<LayoutBox> {
        let children = self
            .world()
            .node(id)
            .map(|node| node.children.clone())
            .unwrap_or_default();
        if children.is_empty() {
            let metrics = self.world().theme_metrics();
            return self
                .read(crate::Entity::<ReorderList>::from_stable_id(id), |list| {
                    list.row_bounds(bounds, metrics)
                })
                .unwrap_or_default();
        }
        children
            .into_iter()
            .filter_map(|child| self.world().layout_box(child))
            .collect()
    }

    fn reorder_tool_boxes(&self, entity: crate::Entity<ReorderList>) -> Vec<LayoutBox> {
        let tools = self
            .read(entity, |list| {
                list.items
                    .iter()
                    .filter_map(|item| item.tools)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        tools
            .into_iter()
            .filter_map(|id| self.world().layout_box(id))
            .collect()
    }

    /// Resolve a secondary press on `id` to the row body under the point and
    /// publish [`ReorderListEvent::Secondary`] on the list. Returns `Ok(false)`
    /// when `id` is not a reorder list, the point sits on a row's tool cluster
    /// (tools keep their own pointer handling) or on empty space, so the
    /// secondary-press walk keeps bubbling.
    pub fn emit_reorder_row_secondary(
        &mut self,
        id: StableNodeId,
        x: f32,
        y: f32,
    ) -> Result<bool, crate::FrameworkError> {
        if !point_finite(x, y) || !self.is_reorder_list(id) {
            return Ok(false);
        }
        let Some(entity) = self.reorder_list_entity(id) else {
            return Ok(false);
        };
        let Some(bounds) = self.world().layout_box(id) else {
            return Ok(false);
        };
        let rows = self.reorder_row_boxes(id, bounds);
        let exclude = self.reorder_tool_boxes(entity);
        if exclude.iter().any(|reserved| reserved.contains(x, y)) {
            return Ok(false);
        }
        let (values, enabled) = self
            .read(entity, |list| {
                (
                    list.items
                        .iter()
                        .map(|item| Arc::clone(&item.value))
                        .collect::<Vec<_>>(),
                    list.items
                        .iter()
                        .map(|item| !item.disabled)
                        .collect::<Vec<_>>(),
                )
            })
            .unwrap_or((Vec::new(), Vec::new()));
        let Some(index) = item_at(&rows, &enabled, x, y) else {
            return Ok(false);
        };
        let source = Arc::clone(&values[index]);
        self.update_component(entity, |_, cx| {
            cx.emit(ReorderListEvent::Secondary { source, x, y });
        })?;
        Ok(true)
    }
}

fn point_finite(x: f32, y: f32) -> bool {
    x.is_finite() && y.is_finite()
}

fn item_at(bounds: &[LayoutBox], enabled: &[bool], x: f32, y: f32) -> Option<usize> {
    bounds.iter().enumerate().find_map(|(index, row)| {
        (enabled.get(index).copied().unwrap_or(false) && row.contains(x, y)).then_some(index)
    })
}

fn drop_before_index(
    bounds: &[LayoutBox],
    drop_targets: &[bool],
    excluded: Option<usize>,
    y: f32,
) -> Option<usize> {
    bounds.iter().enumerate().find_map(|(index, row)| {
        (Some(index) != excluded
            && drop_targets.get(index).copied().unwrap_or(false)
            && y < row.y + row.height * 0.5)
            .then_some(index)
    })
}

fn tree_drop_target(
    bounds: &[LayoutBox],
    drop_targets: &[bool],
    excluded: Option<usize>,
    x: f32,
    y: f32,
) -> Option<(usize, TreeDropPosition)> {
    bounds
        .iter()
        .enumerate()
        .find(|(index, row)| {
            Some(*index) != excluded
                && drop_targets.get(*index).copied().unwrap_or(false)
                && row.contains(x, y)
        })
        .map(|(index, row)| {
            let offset = (y - row.y) / row.height.max(1.0);
            let position = if offset < 0.25 {
                TreeDropPosition::Before
            } else if offset > 0.75 {
                TreeDropPosition::After
            } else {
                TreeDropPosition::Inside
            };
            (index, position)
        })
}

fn reorder_insert_line(
    list_bounds: LayoutBox,
    rows: &[LayoutBox],
    before: Option<usize>,
) -> LayoutBox {
    let y = before
        .and_then(|index| rows.get(index).map(|row| row.y - INSERT_THICKNESS))
        .or_else(|| rows.last().map(|row| row.y + row.height + INSERT_THICKNESS))
        .unwrap_or(list_bounds.y);
    LayoutBox {
        x: list_bounds.x + INSERT_INSET,
        y,
        width: (list_bounds.width - INSERT_INSET * 2.0).max(0.0),
        height: INSERT_THICKNESS,
    }
}

fn tree_insert_line(target: LayoutBox, position: TreeDropPosition) -> LayoutBox {
    match position {
        TreeDropPosition::Before => LayoutBox {
            x: target.x + INSERT_INSET,
            y: target.y - 1.0,
            width: (target.width - INSERT_INSET * 2.0).max(0.0),
            height: INSERT_THICKNESS,
        },
        TreeDropPosition::After => LayoutBox {
            x: target.x + INSERT_INSET,
            y: target.y + target.height - 1.0,
            width: (target.width - INSERT_INSET * 2.0).max(0.0),
            height: INSERT_THICKNESS,
        },
        TreeDropPosition::Inside => LayoutBox {
            x: target.x + 3.0,
            y: target.y + INSERT_THICKNESS,
            width: (target.width - 6.0).max(0.0),
            height: (target.height - 4.0).max(0.0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DocumentId;
    use crate::framework::AppContext;

    fn document() -> DocumentId {
        DocumentId::new(1).unwrap()
    }

    fn sample() -> ReorderList {
        ReorderList::new([
            ReorderItem::new("a", "Alpha"),
            ReorderItem::new("b", "Beta"),
            ReorderItem::new("c", "Gamma"),
        ])
    }

    fn bounds() -> LayoutBox {
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 180.0,
            height: 3.0 * ControlSize::Small.height_in(UI_METRICS) + 2.0 * DEFAULT_SPACING,
        }
    }

    fn apply(list: &mut ReorderList, pointer: ReorderListPointer) -> Option<ReorderListEvent> {
        list.apply_pointer(pointer, bounds(), UI_METRICS)
    }

    #[test]
    fn click_without_move_selects() {
        let mut list = sample();
        assert_eq!(
            apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 42.0 }),
            None
        );
        assert!(list.is_dragging());
        assert_eq!(
            apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 45.0 }),
            None
        );
        assert!(list.insert_line(bounds(), UI_METRICS).is_none());
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 45.0 }),
            Some(ReorderListEvent::Select(Arc::from("b")))
        );
        assert_eq!(list.selected_value().map(Arc::as_ref), Some("b"));
        assert!(!list.is_dragging());
    }

    #[test]
    fn drag_past_threshold_reorders_with_before_value() {
        let mut list = sample();
        apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 12.0 });
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 80.0 });
        // Dropping past the last row puts the line one thickness below the
        // whole stack. Derived from the same constants `bounds()` is, so a
        // spacing or control-height token can move without editing a literal.
        assert_eq!(
            list.insert_line(bounds(), UI_METRICS),
            Some(LayoutBox {
                x: bounds().x + INSERT_INSET,
                y: bounds().y + bounds().height + INSERT_THICKNESS,
                width: bounds().width - INSERT_INSET * 2.0,
                height: INSERT_THICKNESS,
            })
        );
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 80.0 }),
            Some(ReorderListEvent::Reorder {
                source: Arc::from("a"),
                before: None,
            })
        );
        assert_eq!(
            list.items
                .iter()
                .map(|item| item.value.as_ref())
                .collect::<Vec<_>>(),
            ["a", "b", "c"]
        );
        assert!(!list.is_dragging());

        let mut list = sample();
        apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 72.0 });
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 12.0 });
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 12.0 }),
            Some(ReorderListEvent::Reorder {
                source: Arc::from("c"),
                before: Some(Arc::from("a")),
            })
        );
    }

    #[test]
    fn tree_drop_on_drop_only_row_is_inside() {
        let mut list = ReorderList::new([
            ReorderItem::new("a", "Alpha"),
            ReorderItem::new("b", "Beta")
                .draggable(false)
                .drop_target(true),
            ReorderItem::new("c", "Gamma"),
        ])
        .tree_drop(true);
        apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 12.0 });
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 42.0 });
        // The inside highlight sits on the second row, inset within it.
        let row = ControlSize::Small.height_in(UI_METRICS);
        assert_eq!(
            list.insert_line(bounds(), UI_METRICS),
            Some(LayoutBox {
                x: bounds().x + 3.0,
                y: bounds().y + row + DEFAULT_SPACING + INSERT_THICKNESS,
                width: bounds().width - 6.0,
                height: row - 4.0,
            })
        );
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 42.0 }),
            Some(ReorderListEvent::TreeDrop {
                source: Arc::from("a"),
                intent: TreeDropIntent {
                    target: Arc::from("b"),
                    position: TreeDropPosition::Inside,
                },
            })
        );
    }

    #[test]
    fn invalid_tree_drop_does_not_emit_reorder() {
        let mut list = sample().tree_drop(true);
        apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 12.0 });
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 90.0 });
        assert!(list.insert_line(bounds(), UI_METRICS).is_none());
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 90.0 }),
            None
        );

        let mut list = sample();
        apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 12.0 });
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 90.0 });
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 90.0 }),
            Some(ReorderListEvent::Reorder {
                source: Arc::from("a"),
                before: None,
            })
        );
    }

    #[test]
    fn disabled_and_non_draggable_sources_are_ignored() {
        let mut list = ReorderList::new([
            ReorderItem::new("a", "Alpha"),
            ReorderItem::new("b", "Beta").disabled(true),
            ReorderItem::new("c", "Gamma").draggable(false),
        ]);
        assert_eq!(
            apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 42.0 }),
            None
        );
        assert!(!list.is_dragging());
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 80.0 });
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 80.0 }),
            None
        );

        assert_eq!(
            apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 72.0 }),
            None
        );
        assert!(!list.is_dragging());
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 12.0 });
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 12.0 }),
            None
        );
    }

    #[test]
    fn reserved_tool_boxes_do_not_begin_a_drag() {
        let mut list = sample();
        let rows = list.row_bounds(bounds(), UI_METRICS);
        let exclude = [LayoutBox {
            x: 120.0,
            y: 0.0,
            width: 60.0,
            height: 28.0,
        }];
        assert_eq!(
            list.apply_pointer_with_rows(
                ReorderListPointer::Down { x: 140.0, y: 12.0 },
                &rows,
                &exclude
            ),
            None
        );
        assert!(!list.is_dragging());
        assert_eq!(
            list.apply_pointer_with_rows(
                ReorderListPointer::Down { x: 40.0, y: 12.0 },
                &rows,
                &exclude
            ),
            None
        );
        assert!(list.is_dragging());
    }

    #[test]
    fn cancel_clears_transient_drag_state() {
        let mut list = sample();
        apply(&mut list, ReorderListPointer::Down { x: 40.0, y: 12.0 });
        apply(&mut list, ReorderListPointer::Move { x: 40.0, y: 80.0 });
        assert!(list.is_dragging());
        assert!(list.insert_line(bounds(), UI_METRICS).is_some());
        assert_eq!(
            apply(&mut list, ReorderListPointer::Cancel),
            Some(ReorderListEvent::Cancelled)
        );
        assert!(!list.is_dragging());
        assert!(list.insert_line(bounds(), UI_METRICS).is_none());
        assert_eq!(list.cancel(), None);
        assert_eq!(
            apply(&mut list, ReorderListPointer::Up { x: 40.0, y: 80.0 }),
            None
        );
    }

    #[test]
    fn projects_a_pointer_focusable_list_surface() {
        let mut context = AppContext::new();
        let list = context.create_component(document(), sample()).unwrap();
        let id = list.stable_id();
        assert!(matches!(
            context.world().node(id).map(|node| node.kind),
            Some(NodeKind::Element { tag }) if tag == "reorder-list"
        ));
        assert_eq!(
            context.world().interaction(id),
            Some(InteractionState {
                pointer_events: true,
                focusable: true,
            })
        );
        assert_eq!(
            context.world().standard_visual(id),
            Some(StandardVisual::ReorderList {
                rows: sample().paint_rows(),
                size: ControlSize::Small,
                spacing: DEFAULT_SPACING,
                insert: None,
            })
        );
        let style = context.world().node_style(id).expect("projected style");
        assert_eq!(style.layout.width, Some(LengthSpec::Fill));
        assert_eq!(
            style.layout.height,
            Some(LengthSpec::Px(
                3.0 * ControlSize::Small.height_in(UI_METRICS) + 2.0 * DEFAULT_SPACING
            ))
        );
    }

    #[test]
    fn an_installed_compact_height_reaches_self_drawn_reorder_hit_rows() {
        let mut context = AppContext::new();
        let list = context.create_component(document(), sample()).unwrap();
        context
            .layout_document(document(), crate::LayoutViewport::new(180.0, 200.0))
            .unwrap();
        let mut metrics = nana_ui_core::UI_METRICS;
        metrics.compact_control_height = 40.0;
        assert!(
            context
                .set_style_tokens(
                    nana_ui_core::ThemeMode::Dark,
                    metrics,
                    nana_ui_core::SemanticPalette::dark(),
                    nana_ui_core::SemanticPalette::dark().surface,
                )
                .unwrap()
        );
        let bounds = context.world().layout_box(list.stable_id()).unwrap();
        // Compile-time Small rows are 28px; at y=35 that is row "b". Installed
        // 40px rows put the same point on row "a".
        let x = bounds.x + 40.0;
        let y = bounds.y + 35.0;
        assert!(
            context
                .begin_reorder_list_pointer(document(), 1, list.stable_id(), x, y)
                .unwrap()
        );
        assert!(
            context
                .end_reorder_list_pointer(document(), 1, x, y, false)
                .unwrap()
        );
        assert_eq!(
            context
                .read(list, |list| list.selected_value().map(Arc::clone))
                .unwrap()
                .as_deref(),
            Some("a")
        );
    }

    #[test]
    fn declared_live_rows_suppress_self_painted_rows() {
        let mut context = AppContext::new();
        let list = context
            .create_component(document(), sample().live_rows(true))
            .unwrap();
        let id = list.stable_id();
        assert_eq!(
            context.world().standard_visual(id),
            Some(StandardVisual::ReorderList {
                rows: Arc::from([]),
                size: ControlSize::Small,
                spacing: DEFAULT_SPACING,
                insert: None,
            })
        );
        let style = context.world().node_style(id).expect("projected style");
        assert_eq!(style.layout.height, Some(LengthSpec::Shrink));
    }

    #[test]
    fn secondary_press_on_a_row_body_publishes_the_row() {
        use std::sync::Mutex;

        let mut context = AppContext::new();
        let document = document();
        let mut style = NodeStyle::default();
        {
            let layout = Arc::make_mut(&mut style.layout);
            layout.width = Some(LengthSpec::Px(200.0));
            layout.height = Some(LengthSpec::Px(48.0));
        }
        let list = context
            .create_component(
                document,
                ReorderList::new([
                    ReorderItem::new("a", "Alpha"),
                    ReorderItem::new("b", "Beta"),
                ])
                .size(ControlSize::Small)
                .style(style),
            )
            .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&events);
        context
            .on(list, move |_list, event: &ReorderListEvent, _cx| {
                observed.lock().unwrap().push(event.clone());
            })
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(200.0, 48.0))
            .unwrap();
        context.rebuild_hit_test(document);

        // Row bodies are pointer-transparent; the secondary press still lands
        // on the list and resolves to the row under the point.
        assert_eq!(
            context.secondary_press_at(document, 100.0, 8.0).unwrap(),
            Some(list.stable_id())
        );
        assert_eq!(
            events.lock().unwrap().last().cloned(),
            Some(ReorderListEvent::Secondary {
                source: Arc::from("a"),
                x: 100.0,
                y: 8.0,
            })
        );
        assert_eq!(
            context.secondary_press_at(document, 100.0, 30.0).unwrap(),
            Some(list.stable_id())
        );
        assert_eq!(
            events.lock().unwrap().last().cloned(),
            Some(ReorderListEvent::Secondary {
                source: Arc::from("b"),
                x: 100.0,
                y: 30.0,
            })
        );
    }
}
