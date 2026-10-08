//! Shared Runtime layout flow algorithms.

use super::*;

/// In-flow children, plus whether the answer depended on anything below
/// the direct children.
///
/// The cached plans (`ContainerPlan`, `MeasurePlan`) re-check only the children
/// the change closure reaches, and they identify those children by id in the
/// container's own child list. That identification is only sound while the flow
/// list is a subsequence of the direct children whose membership is decided by
/// each child's own style. Two cases break it, and both are reported here:
///
/// - `display: contents` splices a child's OWN children into the flow list, so
///   the list is a function of a grandchild list the plan never recorded.
/// - an inline-level child under a block parent is unboxed or not depending on
///   whether its subtree contains a block, so a change arbitrarily deep can
///   move the child in or out of the flow list while its style stays put.
///
/// Reporting the condition rather than checking for it afterwards keeps this
/// off the per-sibling path: the collection already visits every child, and a
/// membership test over the produced list would be quadratic.
pub(super) fn collect_flow_children_reporting(
    children: &[StableNodeId],
    nodes: &mut LayoutInputMap<'_>,
    parent_display: Option<DisplaySpec>,
) -> Result<(Vec<StableNodeId>, bool), UiWorldError> {
    let mut out = Vec::new();
    let mut descendant_dependent = false;
    collect_flow_children_into(
        children,
        nodes,
        parent_display,
        &mut out,
        &mut descendant_dependent,
    )?;
    Ok((out, descendant_dependent))
}

pub(super) fn parent_unboxes_inline(parent_display: Option<DisplaySpec>) -> bool {
    !parent_display
        .is_some_and(|display| display.is_flex_container() || display.is_grid_container())
}

pub(super) fn collect_flow_children_into(
    children: &[StableNodeId],
    nodes: &mut LayoutInputMap<'_>,
    parent_display: Option<DisplaySpec>,
    out: &mut Vec<StableNodeId>,
    descendant_dependent: &mut bool,
) -> Result<(), UiWorldError> {
    for child in children.iter().copied() {
        let Some(style) = nodes.style(child) else {
            continue;
        };
        if style.omits_box() {
            continue;
        }
        if style.display.is_some_and(DisplaySpec::is_contents) {
            *descendant_dependent = true;
            let nested = match nodes.get(child)? {
                Some(node) => (*node.children).clone(),
                None => continue,
            };
            collect_flow_children_into(&nested, nodes, parent_display, out, descendant_dependent)?;
            continue;
        }
        if style.position.is_out_of_flow() {
            continue;
        }
        if parent_unboxes_inline(parent_display) && style.is_inline_level() {
            // Whichever way the test goes, the answer came from the child's
            // subtree, so a plan over this container cannot be re-checked from
            // the direct child list alone.
            *descendant_dependent = true;
            if inline_contains_block(child, nodes)? {
                let nested = match nodes.get(child)? {
                    Some(node) => (*node.children).clone(),
                    None => continue,
                };
                collect_flow_children_into(
                    &nested,
                    nodes,
                    parent_display,
                    out,
                    descendant_dependent,
                )?;
                continue;
            }
        }
        out.push(child);
    }
    Ok(())
}

pub(super) fn inline_contains_block(
    id: StableNodeId,
    nodes: &mut LayoutInputMap<'_>,
) -> Result<bool, UiWorldError> {
    let Some(node) = nodes.get(id)? else {
        return Ok(false);
    };
    let children = (*node.children).clone();
    for grandchild in children {
        let Some(style) = nodes.style(grandchild) else {
            continue;
        };
        if style.omits_box() || style.position.is_out_of_flow() {
            continue;
        }
        if style.display.is_some_and(DisplaySpec::is_contents) || style.is_inline_level() {
            if inline_contains_block(grandchild, nodes)? {
                return Ok(true);
            }
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

pub(super) fn collect_positioned_children(
    children: &[StableNodeId],
    nodes: &mut LayoutInputMap<'_>,
) -> Result<Vec<StableNodeId>, UiWorldError> {
    let mut out = Vec::new();
    collect_positioned_children_into(children, nodes, &mut out)?;
    Ok(out)
}

pub(super) fn collect_positioned_children_into(
    children: &[StableNodeId],
    nodes: &mut LayoutInputMap<'_>,
    out: &mut Vec<StableNodeId>,
) -> Result<(), UiWorldError> {
    for child in children.iter().copied() {
        let Some(style) = nodes.style(child) else {
            continue;
        };
        if style.omits_box() {
            continue;
        }
        if style.display.is_some_and(DisplaySpec::is_contents) {
            let nested = match nodes.get(child)? {
                Some(node) => (*node.children).clone(),
                None => continue,
            };
            collect_positioned_children_into(&nested, nodes, out)?;
            continue;
        }
        if style.position.is_out_of_flow() {
            out.push(child);
        }
    }
    Ok(())
}

pub(super) fn collect_floated_children(
    children: &[StableNodeId],
    nodes: &mut LayoutInputMap<'_>,
) -> Result<Vec<StableNodeId>, UiWorldError> {
    let mut out = Vec::new();
    collect_floated_children_into(children, nodes, &mut out)?;
    Ok(out)
}

pub(super) fn collect_floated_children_into(
    children: &[StableNodeId],
    nodes: &mut LayoutInputMap<'_>,
    out: &mut Vec<StableNodeId>,
) -> Result<(), UiWorldError> {
    for child in children.iter().copied() {
        let Some(style) = nodes.style(child) else {
            continue;
        };
        if style.omits_box() {
            continue;
        }
        if style.display.is_some_and(DisplaySpec::is_contents) {
            let nested = match nodes.get(child)? {
                Some(node) => (*node.children).clone(),
                None => continue,
            };
            collect_floated_children_into(&nested, nodes, out)?;
            continue;
        }
        if style.is_floated() && !style.position.is_out_of_flow() {
            out.push(child);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PackedFloat {
    pub(super) id: StableNodeId,
    pub(super) origin: Point,
    pub(super) size: Size,
    pub(super) side: FloatSpec,
    /// Margin-box left in the same space as [`Self::origin`].
    pub(super) occupy_x: f32,
    pub(super) occupy_y: f32,
    pub(super) occupy_w: f32,
    pub(super) occupy_h: f32,
}

#[derive(Debug, Default)]
pub(super) struct PackedFloats {
    pub(super) items: Vec<PackedFloat>,
    /// Occupied bottom of left floats after pack/wrap, relative to content origin.
    pub(super) left_bottom: f32,
    /// Occupied bottom of right floats after pack/wrap, relative to content origin.
    pub(super) right_bottom: f32,
}

impl PackedFloats {
    /// Left/right insets at content-relative `y` (line-box top), from sibling
    /// float margin boxes. Not ancestor intrusion / `shape-outside`.
    pub(super) fn insets_at_y(
        &self,
        content_origin: Point,
        content_width: f32,
        y: f32,
    ) -> (f32, f32) {
        let abs_y = content_origin.y + y;
        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for item in &self.items {
            let top = item.occupy_y;
            let bottom = item.occupy_y + item.occupy_h;
            if abs_y + 0.5 < top || abs_y >= bottom - 0.5 {
                continue;
            }
            match item.side {
                FloatSpec::Right => {
                    let occupy_left = item.occupy_x - content_origin.x;
                    right = right.max((content_width - occupy_left).max(0.0));
                }
                _ => {
                    let occupy_right = item.occupy_x + item.occupy_w - content_origin.x;
                    left = left.max(occupy_right.max(0.0));
                }
            }
        }
        (left, right)
    }

    /// Nearest float margin-box bottom below `y` among floats that occupy `y`.
    pub(super) fn next_bottom_after(&self, content_origin: Point, y: f32) -> Option<f32> {
        let abs_y = content_origin.y + y;
        let mut best: Option<f32> = None;
        for item in &self.items {
            let top = item.occupy_y;
            let bottom = item.occupy_y + item.occupy_h - content_origin.y;
            if top > abs_y + 0.5 || bottom <= y + 0.5 {
                continue;
            }
            best = Some(best.map_or(bottom, |value| value.min(bottom)));
        }
        best
    }
}

/// One wrap line: child indices plus the shortened IFC line box (full width when
/// not avoiding floats).
#[derive(Debug, Clone)]
pub(super) struct LineBoxSlot {
    pub(super) indices: Vec<usize>,
    pub(super) main_start: f32,
    pub(super) main_available: f32,
    /// Content-relative cross start after float drop / `clear` (IFC only).
    pub(super) cross_y: f32,
    pub(super) pin_cross: bool,
}

/// Geometric same-side pack/wrap. Bottoms are the occupied extent after wrapping,
/// not the pre-pack max of each float's own height (so `clear` clears the second row).
#[allow(clippy::too_many_arguments)]
pub(super) fn pack_floated_children(
    floated: &[StableNodeId],
    content_origin: Point,
    content: Size,
    // Percent base of the children's margins: the container's inline size.
    edge_base: f32,
    viewport: LayoutViewport,
    child_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    intrinsic: &mut IntrinsicCache,
    scope: Option<&ScopeContext<'_>>,
) -> Result<PackedFloats, UiWorldError> {
    let mut items = Vec::with_capacity(floated.len());
    let mut left_cursor_x = content_origin.x;
    let mut left_line_y = content_origin.y;
    let mut left_line_bottom = content_origin.y;
    let mut right_cursor_x = content_origin.x + content.width;
    let mut right_line_y = content_origin.y;
    let mut right_line_bottom = content_origin.y;
    for child in floated {
        let Some(child_style) = nodes.style(*child) else {
            continue;
        };
        let child_style = child_style.as_ref();
        let child_size = intrinsic_size_scoped(
            *child,
            content,
            Some(FlexDirection::Row),
            viewport,
            child_font_px,
            nodes,
            intrinsic,
            scope,
        )?;
        let child_fonts = fonts_of(child_style, child_font_px);
        let margin = child_style.resolved_margin_against_fonts(Some(edge_base), child_fonts);
        let outer_w = child_size.width + margin.left + margin.right;
        // A float's own `clear` uses packed bottoms of earlier floats, same
        // contract as in-flow clear (not the pre-pack max of each float).
        let clear_bottom = match child_style.clear {
            ClearSpec::None => None,
            ClearSpec::Left => Some(left_line_bottom),
            ClearSpec::Right => Some(right_line_bottom),
            ClearSpec::Both => Some(left_line_bottom.max(right_line_bottom)),
        };
        let (x, y) = match child_style.float {
            FloatSpec::Right => {
                if let Some(bottom) = clear_bottom
                    && bottom > right_line_y + 0.5
                {
                    right_cursor_x = content_origin.x + content.width;
                    right_line_y = bottom;
                }
                if right_cursor_x < content_origin.x + content.width - 0.5
                    && right_cursor_x - outer_w < content_origin.x - 0.5
                {
                    right_cursor_x = content_origin.x + content.width;
                    right_line_y = right_line_bottom;
                }
                let x = (right_cursor_x - child_size.width - margin.right).max(content_origin.x);
                let y = right_line_y + margin.top;
                right_cursor_x = x - margin.left;
                right_line_bottom = right_line_bottom.max(y + child_size.height + margin.bottom);
                (x, y)
            }
            _ => {
                if let Some(bottom) = clear_bottom
                    && bottom > left_line_y + 0.5
                {
                    left_cursor_x = content_origin.x;
                    left_line_y = bottom;
                }
                if left_cursor_x > content_origin.x + 0.5
                    && left_cursor_x + outer_w > content_origin.x + content.width + 0.5
                {
                    left_cursor_x = content_origin.x;
                    left_line_y = left_line_bottom;
                }
                let x = left_cursor_x + margin.left;
                let y = left_line_y + margin.top;
                left_cursor_x = x + child_size.width + margin.right;
                left_line_bottom = left_line_bottom.max(y + child_size.height + margin.bottom);
                (x, y)
            }
        };
        items.push(PackedFloat {
            id: *child,
            origin: Point { x, y },
            size: child_size,
            side: child_style.float,
            occupy_x: x - margin.left,
            occupy_y: y - margin.top,
            occupy_w: outer_w,
            occupy_h: child_size.height + margin.top + margin.bottom,
        });
    }
    Ok(PackedFloats {
        items,
        left_bottom: (left_line_bottom - content_origin.y).max(0.0),
        right_bottom: (right_line_bottom - content_origin.y).max(0.0),
    })
}
