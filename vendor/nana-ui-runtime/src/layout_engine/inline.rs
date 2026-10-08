//! Shared Runtime layout inline algorithms.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn pack_wrap_lines(
    children: &[StableNodeId],
    sizes: &[Size],
    direction: FlexDirection,
    content: Size,
    // Percent base of the children's margins: the container's inline size.
    edge_base: f32,
    gap: f32,
    grid_tracks: Option<&[GridTrack]>,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &LayoutInputMap<'_>,
    break_on_blocks: bool,
) -> Vec<Vec<usize>> {
    // Lines fill the main axis; percentage margins resolve against the
    // containing block's inline size, which is the main axis only for a row
    // in `horizontal-tb` (CSS Box Model §5).
    let content_main = main_extent(content, direction);
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut line_main = 0.0f32;
    for (index, child) in children.iter().enumerate() {
        let Some(style) = nodes.style(*child) else {
            continue;
        };
        let block_break = break_on_blocks && !style.is_inline_level();
        if block_break && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            line_main = 0.0;
        }
        let margin = style.resolved_margin_against_fonts(
            Some(edge_base),
            fonts_of(style.as_ref(), parent_font_px),
        );
        let main = packing_main_size(
            style.as_ref(),
            sizes[index],
            direction,
            content_main,
            edge_base,
            viewport,
            parent_font_px,
            grid_tracks.and_then(|tracks| tracks.get(index).copied()),
        );
        let outer =
            main + main_start_margin(margin, direction) + main_end_margin(margin, direction);
        let need = if current.is_empty() {
            outer
        } else {
            line_main + gap + outer
        };
        if !current.is_empty() && need > content_main + 0.5 {
            lines.push(std::mem::take(&mut current));
            line_main = 0.0;
        }
        if current.is_empty() {
            line_main = outer;
        } else {
            line_main += gap + outer;
        }
        current.push(index);
        if block_break && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            line_main = 0.0;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

pub(super) fn ifc_item_outer(
    style: &LayoutStyle,
    size: Size,
    content_width: f32,
    viewport: LayoutViewport,
    parent_font_px: f32,
) -> (f32, f32) {
    let direction = FlexDirection::Row;
    let margin =
        style.resolved_margin_against_fonts(Some(content_width), fonts_of(style, parent_font_px));
    let main = packing_main_size(
        style,
        size,
        direction,
        content_width,
        // A horizontal line box: its width is the inline size.
        content_width,
        viewport,
        parent_font_px,
        None,
    );
    let outer_main =
        main + main_start_margin(margin, direction) + main_end_margin(margin, direction);
    let outer_cross = cross_extent(size, direction) + cross_margin(margin, direction);
    (outer_main, outer_cross)
}

/// IFC wrap using the existing content-width line packer, with per-line available
/// width reduced by sibling float occupancy (shrink-to-avoid-float).
#[allow(clippy::too_many_arguments)]
pub(super) fn pack_ifc_line_boxes(
    children: &[StableNodeId],
    sizes: &[Size],
    content_origin: Point,
    content_width: f32,
    gap: f32,
    cross_gap: f32,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &LayoutInputMap<'_>,
    packed_floats: &PackedFloats,
) -> Vec<LineBoxSlot> {
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut line_main = 0.0f32;
    let mut line_y = 0.0f32;
    let mut left_inset = 0.0f32;
    let mut available = content_width;
    let refresh = |line_y: f32, left_inset: &mut f32, available: &mut f32| {
        let (left, right) = packed_floats.insets_at_y(content_origin, content_width, line_y);
        *left_inset = left;
        *available = (content_width - left - right).max(0.0);
    };
    refresh(line_y, &mut left_inset, &mut available);
    let flush = |lines: &mut Vec<LineBoxSlot>,
                 current: &mut Vec<usize>,
                 line_main: &mut f32,
                 line_y: &mut f32,
                 left_inset: &mut f32,
                 available: &mut f32| {
        if current.is_empty() {
            return;
        }
        let line_cross = current
            .iter()
            .map(|&index| {
                nodes
                    .style(children[index])
                    .map(|style| {
                        ifc_item_outer(
                            style.as_ref(),
                            sizes[index],
                            content_width,
                            viewport,
                            parent_font_px,
                        )
                        .1
                    })
                    .unwrap_or(0.0)
            })
            .fold(0.0f32, f32::max);
        lines.push(LineBoxSlot {
            indices: std::mem::take(current),
            main_start: *left_inset,
            main_available: *available,
            cross_y: *line_y,
            pin_cross: true,
        });
        *line_main = 0.0;
        *line_y += line_cross + cross_gap;
        refresh(*line_y, left_inset, available);
    };
    for (index, child) in children.iter().enumerate() {
        let Some(style) = nodes.style(*child) else {
            continue;
        };
        let style_ref = style.as_ref();
        let clear_y = clear_offset(
            style_ref.clear,
            packed_floats.left_bottom,
            packed_floats.right_bottom,
        );
        if clear_y > line_y + 0.5 {
            flush(
                &mut lines,
                &mut current,
                &mut line_main,
                &mut line_y,
                &mut left_inset,
                &mut available,
            );
            line_y = clear_y;
            refresh(line_y, &mut left_inset, &mut available);
        }
        let block_break = !style_ref.is_inline_level();
        if block_break {
            flush(
                &mut lines,
                &mut current,
                &mut line_main,
                &mut line_y,
                &mut left_inset,
                &mut available,
            );
            let (_, outer_cross) = ifc_item_outer(
                style_ref,
                sizes[index],
                content_width,
                viewport,
                parent_font_px,
            );
            lines.push(LineBoxSlot {
                indices: vec![index],
                main_start: 0.0,
                main_available: content_width,
                cross_y: line_y,
                pin_cross: true,
            });
            line_y += outer_cross + cross_gap;
            refresh(line_y, &mut left_inset, &mut available);
            continue;
        }
        let (outer, _) = ifc_item_outer(
            style_ref,
            sizes[index],
            content_width,
            viewport,
            parent_font_px,
        );
        let need = if current.is_empty() {
            outer
        } else {
            line_main + gap + outer
        };
        if !current.is_empty() && need > available + 0.5 {
            flush(
                &mut lines,
                &mut current,
                &mut line_main,
                &mut line_y,
                &mut left_inset,
                &mut available,
            );
        }
        if current.is_empty() && outer > available + 0.5 {
            while outer > available + 0.5 {
                match packed_floats.next_bottom_after(content_origin, line_y) {
                    Some(next) if next > line_y + 0.5 => {
                        line_y = next;
                        refresh(line_y, &mut left_inset, &mut available);
                    }
                    _ => break,
                }
            }
        }
        if current.is_empty() {
            line_main = outer;
        } else {
            line_main += gap + outer;
        }
        current.push(index);
    }
    flush(
        &mut lines,
        &mut current,
        &mut line_main,
        &mut line_y,
        &mut left_inset,
        &mut available,
    );
    if lines.is_empty() {
        lines.push(LineBoxSlot {
            indices: Vec::new(),
            main_start: 0.0,
            main_available: content_width,
            cross_y: 0.0,
            pin_cross: true,
        });
    }
    lines
}

#[allow(clippy::too_many_arguments)]
pub(super) fn wrap_intrinsic_size(
    direction: FlexDirection,
    wrap: FlexWrap,
    children: &[StableNodeId],
    sizes: &[Size],
    available: Size,
    // Percent base of the children's margins: the container's inline size.
    edge_base: f32,
    gap: f32,
    cross_gap: f32,
    grid_tracks: Option<&[GridTrack]>,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &LayoutInputMap<'_>,
) -> Size {
    let content_main = main_extent(available, direction);
    let mut lines = pack_wrap_lines(
        children,
        sizes,
        direction,
        available,
        edge_base,
        gap,
        grid_tracks,
        viewport,
        parent_font_px,
        nodes,
        false,
    );
    if matches!(wrap, FlexWrap::WrapReverse) {
        lines.reverse();
    }
    let mut cross = 0.0f32;
    let mut max_main = 0.0f32;
    for (line_index, line) in lines.iter().enumerate() {
        let mut line_main = 0.0f32;
        let mut line_cross = 0.0f32;
        for (item_index, &index) in line.iter().enumerate() {
            let Some(style) = nodes.style(children[index]) else {
                continue;
            };
            let margin = style.resolved_margin_against_fonts(
                Some(edge_base),
                fonts_of(style.as_ref(), parent_font_px),
            );
            let main = packing_main_size(
                style.as_ref(),
                sizes[index],
                direction,
                content_main,
                edge_base,
                viewport,
                parent_font_px,
                grid_tracks.and_then(|tracks| tracks.get(index).copied()),
            );
            let outer_main =
                main + main_start_margin(margin, direction) + main_end_margin(margin, direction);
            line_main += outer_main;
            if item_index > 0 {
                line_main += gap;
            }
            line_cross = line_cross
                .max(cross_extent(sizes[index], direction) + cross_margin(margin, direction));
        }
        max_main = max_main.max(line_main);
        cross += line_cross;
        if line_index + 1 < lines.len() {
            cross += cross_gap;
        }
    }
    match direction {
        FlexDirection::Row => Size::new(max_main, cross),
        FlexDirection::Column => Size::new(cross, max_main),
    }
}
