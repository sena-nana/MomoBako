//! Shared Runtime layout measure algorithms.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn intrinsic_size(
    id: StableNodeId,
    available: Size,
    parent_direction: Option<FlexDirection>,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    cache: &mut IntrinsicCache,
) -> Result<Size, UiWorldError> {
    intrinsic_size_scoped(
        id,
        available,
        parent_direction,
        viewport,
        parent_font_px,
        nodes,
        cache,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
/// Resolve the node's own width/height from its style alone.
///
/// `Some` on an axis means the used size does not depend on the node's
/// children: content-sized keywords (`min-content`, `max-content`,
/// `fit-content`, `shrink`) and an indefinite `Fill` all resolve to `None`.
fn resolved_size_specs(
    style: &nana_ui_core::LayoutStyle,
    available: Size,
    // What the node's percentage margins resolve against: its containing
    // block's inline size. See `UiWorld::containing_writing`.
    edge_base: f32,
    viewport: LayoutViewport,
    fonts: FontSizeContext,
) -> (Option<f32>, Option<f32>) {
    // `Fill` sizes the border box to the containing block minus the node's own
    // margins — negative margins widen it, matching the stretch path below;
    // percentages keep resolving against the raw containing block.
    let margin = style.resolved_margin_against_fonts(Some(edge_base), fonts);
    let width = resolve_axis(
        demote_fill_spec_if_indefinite(style.width, available.width),
        available.width,
        available.width - margin.left - margin.right,
        viewport,
        fonts,
    );
    let height = resolve_axis(
        demote_fill_spec_if_indefinite(style.height, available.height),
        available.height,
        available.height - margin.top - margin.bottom,
        viewport,
        fonts,
    );
    (width, height)
}

/// Compose the used size once the content-derived defaults are known.
///
/// `default_width` / `default_height` are consumed only through `unwrap_or`, so
/// a caller that already has both specs resolved may pass anything for them —
/// see the definite-size short circuit in [`intrinsic_size_scoped`].
#[allow(clippy::too_many_arguments)]
fn finish_intrinsic_size(
    style: &nana_ui_core::LayoutStyle,
    fonts: FontSizeContext,
    viewport: LayoutViewport,
    available: Size,
    edge_base: f32,
    chrome: Size,
    parent_direction: Option<FlexDirection>,
    default_width: f32,
    default_height: f32,
) -> Size {
    let (width_spec, height_spec) =
        resolved_size_specs(style, available, edge_base, viewport, fonts);
    let width_from_spec = width_spec.is_some();
    let height_from_spec = height_spec.is_some();
    let vp = Some((viewport.width, viewport.height));
    let min_width = style.resolved_min_width_fonts(Some(available.width), vp, fonts);
    let min_height = style.resolved_min_height_fonts(Some(available.height), vp, fonts);
    let mut width = width_spec.unwrap_or(default_width).max(min_width);
    let mut height = height_spec.unwrap_or(default_height).max(min_height);
    if matches!(style.box_sizing, BoxSizing::ContentBox) {
        if style.width.is_some_and(LengthSpec::is_definite_declared) {
            width += chrome.width;
        }
        if style.height.is_some_and(LengthSpec::is_definite_declared) {
            height += chrome.height;
        }
    }
    if style.aspect_ratio.is_some_and(|r| r.is_finite() && r > 0.0) {
        let stretch_fit_width = !width_from_spec
            && style.stretch_fit_inline()
            && !matches!(parent_direction, Some(FlexDirection::Row))
            && available.width > 0.5;
        if stretch_fit_width {
            width = available.width.max(min_width);
        }
        let mut content_w =
            if width_from_spec || stretch_fit_width || (!height_from_spec && width > 0.0) {
                Some((width - chrome.width).max(0.0))
            } else {
                None
            };
        let mut content_h = if height_from_spec {
            Some((height - chrome.height).max(0.0))
        } else {
            None
        };
        style.apply_aspect_ratio_used(&mut content_w, &mut content_h);
        if let Some(content_w) = content_w {
            width = content_w + chrome.width;
        }
        if let Some(content_h) = content_h {
            height = content_h + chrome.height;
        }
        width = width.max(min_width);
        height = height.max(min_height);
    }
    if let Some(max) = style.resolved_max_width_fonts(Some(available.width), vp, fonts) {
        width = width.min(max);
    }
    if let Some(max) = style.resolved_max_height_fonts(Some(available.height), vp, fonts) {
        height = height.min(max);
    }
    Size::new(width, height)
}

pub(super) fn intrinsic_size_scoped(
    id: StableNodeId,
    available: Size,
    parent_direction: Option<FlexDirection>,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    cache: &mut IntrinsicCache,
    scope: Option<&ScopeContext<'_>>,
) -> Result<Size, UiWorldError> {
    measure_node(
        id,
        None,
        available,
        parent_direction,
        viewport,
        parent_font_px,
        nodes,
        cache,
        scope,
    )
}

/// A flex item measured at the main size its line gave it (`main`, border
/// box, along `direction`): its hypothetical cross size with the used main
/// size (CSS flexbox §9.4). A `Fill` column that shrank beside a fixed one
/// is as tall as its content at its used width, not at the width it was
/// first measured with. `available` is the container's content box, what
/// the item's percentages resolve against.
#[allow(clippy::too_many_arguments)]
pub(super) fn intrinsic_size_at_main(
    id: StableNodeId,
    main: f32,
    direction: FlexDirection,
    available: Size,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    cache: &mut IntrinsicCache,
    scope: Option<&ScopeContext<'_>>,
) -> Result<Size, UiWorldError> {
    let Some(style) = nodes.style(id) else {
        return Ok(Size::default());
    };
    let mut forced = (*style).clone();
    // Border box, so the chrome is inside `main` whatever the item's
    // `box-sizing`; the cross axis is not definite (the caller checks), so
    // `box-sizing` changes nothing there.
    forced.box_sizing = BoxSizing::BorderBox;
    let main = Some(LengthSpec::Px(main.max(0.0)));
    match direction {
        FlexDirection::Row => forced.width = main,
        FlexDirection::Column => forced.height = main,
    }
    measure_node(
        id,
        Some(Arc::new(forced)),
        available,
        Some(direction),
        viewport,
        parent_font_px,
        nodes,
        cache,
        scope,
    )
}

/// Whether a flex item's cross size is its content's at its main size, so
/// a line that gives it another main size than it was measured with must
/// measure it again ([`intrinsic_size_at_main`]).
pub(super) fn cross_follows_used_main(
    style: &nana_ui_core::LayoutStyle,
    direction: FlexDirection,
    measured: Size,
    used: Size,
) -> bool {
    let cross = match direction {
        FlexDirection::Row => style.height,
        FlexDirection::Column => style.width,
    };
    let content_sized =
        cross.is_none_or(|spec| spec == LengthSpec::Auto || spec.is_content_sized());
    // A row item with an aspect ratio of its own takes its height from its
    // used width after placement (`fill_auto_height_from_aspect_ratio`).
    let transferred = direction == FlexDirection::Row && aspect_ratio_is_usable(style);
    content_sized
        && !transferred
        && (main_extent(measured, direction) - main_extent(used, direction)).abs() > 0.01
}

/// [`intrinsic_size_scoped`], or with `forced` for the node's own style (a
/// flex item at its used main size): a forced measurement is not cached or
/// planned, since neither is keyed by it; its descendants are measured as
/// usual.
#[allow(clippy::too_many_arguments)]
fn measure_node(
    id: StableNodeId,
    forced: Option<Arc<nana_ui_core::LayoutStyle>>,
    available: Size,
    parent_direction: Option<FlexDirection>,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    cache: &mut IntrinsicCache,
    scope: Option<&ScopeContext<'_>>,
) -> Result<Size, UiWorldError> {
    let cache_key = (id, available.width.to_bits(), available.height.to_bits());
    let unforced = forced.is_none();
    if unforced && let Some(size) = cache.get(&cache_key) {
        return Ok(*size);
    }
    // A subtree outside the affected closure has no change inside it, so its
    // intrinsic size under the same constraints is unchanged.
    if unforced
        && let Some(scope) = scope
        && !scope.affected.contains(&id)
        && let Some(size) = scope
            .retained
            .intrinsics
            .get(&id)
            .and_then(|memo| memo.get(cache_key.1, cache_key.2))
    {
        cache.insert(cache_key, size);
        return Ok(size);
    }
    let Some(node) = nodes.get(id)? else {
        return Ok(Size::default());
    };
    let style_arc = forced.unwrap_or_else(|| node.style.clone());
    let child_ids = node.children.clone();
    let text_metrics = node.text_metrics;
    let (writing, containing_writing) = (node.writing, node.containing_writing);
    let text_natural_width = text_metrics.and_then(|_| nodes.world.text_natural_width(id));
    let style = style_arc.as_ref();
    if style.omits_box() {
        return Ok(Size::default());
    }
    let fonts = fonts_of(style, parent_font_px);
    let child_font_px = fonts.element_px;
    // Percent edges and sizes resolve against the containing block's inline
    // size, in the containing block's writing mode.
    let edge_base = containing_writing.inline_size(available.width, available.height);
    let padding = style.resolved_padding_against_fonts(Some(edge_base), fonts);
    let border = style.resolved_border_edges();
    let chrome = Size::new(
        padding.left + padding.right + border.left + border.right,
        padding.top + padding.bottom + border.top + border.bottom,
    );
    // Measure descendants against this node's declared content box, not its
    // parent's full budget. Percent padding still resolves against the parent.
    let margin = style.resolved_margin_against_fonts(Some(edge_base), fonts);
    let content_axis = |spec: Option<LengthSpec>, available: f32, margins: f32, chrome: f32| {
        let resolved = resolve_axis(
            demote_fill_spec_if_indefinite(spec, available),
            available,
            available - margins,
            viewport,
            fonts,
        );
        let extent = resolved.unwrap_or(available);
        if resolved.is_some()
            && matches!(style.box_sizing, BoxSizing::ContentBox)
            && spec.is_some_and(LengthSpec::is_definite_declared)
        {
            extent.max(0.0)
        } else {
            (extent - chrome).max(0.0)
        }
    };
    let mut content_available = Size::new(
        content_axis(
            style.width,
            available.width,
            margin.left + margin.right,
            chrome.width,
        ),
        content_axis(
            style.height,
            available.height,
            margin.top + margin.bottom,
            chrome.height,
        ),
    );
    // Width limits constrain wrapping descendants before their heights are
    // measured. Applying them only to the final box leaves an auto-height
    // parent sized for wider text than its children can actually use.
    let vp = Some((viewport.width, viewport.height));
    let min_width = style.resolved_min_width_fonts(Some(available.width), vp, fonts)
        + if matches!(style.box_sizing, BoxSizing::ContentBox)
            && style.width.is_some_and(LengthSpec::is_definite_declared)
        {
            chrome.width
        } else {
            0.0
        };
    let mut measured_width = (content_available.width + chrome.width).max(min_width);
    if let Some(max) = style.resolved_max_width_fonts(Some(available.width), vp, fonts) {
        measured_width = measured_width.min(max);
    }
    content_available.width = (measured_width - chrome.width).max(0.0);
    // A node whose own width and height both resolve from its style needs no
    // measurement of its children: the content-derived defaults below are
    // consumed only through `unwrap_or`, so they would be discarded.
    //
    // This is what made a dirty frame O(document). Layout invalidation
    // propagates to ancestors, so a single edit puts every container above it
    // in the change closure, and each one dropped its cached intrinsic and
    // re-measured all of its children -- a full sibling scan per level, to
    // arrive at a size its own style had already fixed.
    let (spec_width, spec_height) =
        resolved_size_specs(style, available, edge_base, viewport, fonts);
    if spec_width.is_some() && spec_height.is_some() {
        let size = finish_intrinsic_size(
            style,
            fonts,
            viewport,
            available,
            edge_base,
            chrome,
            parent_direction,
            0.0,
            0.0,
        );
        if unforced {
            cache.insert(cache_key, size);
        }
        return Ok(size);
    }

    // The container is content-sized on at least one axis, so it owes a look at
    // its children. Everything it needs from them may still be unchanged; see
    // [`MeasurePlan`].
    if unforced
        && let Some(scope) = scope
        && let Some(plan) = scope
            .retained
            .measure_plans
            .get(&id)
            .and_then(|plans| plans.get(available))
        && plan.inputs_match(
            available,
            parent_direction,
            viewport,
            parent_font_px,
            &style_arc,
            &child_ids,
            text_metrics,
            text_natural_width,
            writing,
        )
        // An ancestor can rewrite a child's effective style without touching
        // the child (overlay hosting, an open menu surface), which would move
        // the measurement with every per-child input still comparing equal.
        && nodes.world.children_layout_style_is_local(id)
        && measure_plan_children_unchanged(plan, viewport, child_font_px, nodes, cache, scope)?
    {
        #[cfg(any(test, feature = "benchmark"))]
        super::plan_stats::note_measure_plan_reused();
        cache.insert(cache_key, plan.size);
        return Ok(plan.size);
    }

    let (mut flow_children, descendant_dependent_flow) =
        collect_flow_children_reporting(&child_ids, nodes, style.display)?;
    let grid_measure = uses_2d_grid(style, &flow_children, nodes);
    let ifc = !grid_measure
        && !style
            .display
            .is_some_and(|d| d.is_flex_container() || d.is_grid_container())
        && flow_children
            .iter()
            .any(|id| nodes.style(*id).is_some_and(|s| s.is_inline_level()));
    let direction = used_flow_direction(style, writing, ifc);
    let wrap = style.flex_wrap;
    let wrapping = ifc
        || match direction {
            FlexDirection::Row => matches!(wrap, FlexWrap::Wrap | FlexWrap::WrapReverse),
            FlexDirection::Column => {
                matches!(wrap, FlexWrap::Wrap | FlexWrap::WrapReverse)
                    && content_available.height > 0.5
            }
        };
    let grid_tracks = match direction {
        FlexDirection::Row => style.active_grid_columns(),
        FlexDirection::Column => style.active_grid_rows(),
    };
    // Line breaks and grid auto-placement depend on which item comes next, so
    // measure those in the order placement lays the children out; measuring
    // in document order while placing by `order` sizes the container for lines
    // it never has. A single unwrapped line sums the same in any order.
    if grid_measure || wrapping || grid_tracks.is_some_and(|tracks| !tracks.is_empty()) {
        sort_by_order(&mut flow_children, nodes);
    }
    let mut child_sizes = Vec::with_capacity(flow_children.len());
    for child in &flow_children {
        // Resolving the child style is a map lookup plus an `Arc` clone, so keep
        // it behind the grid check rather than filtering it away afterwards.
        let child_available = if grid_measure {
            nodes
                .style(*child)
                .map(|child_style| {
                    grid_item_measure_available(child_style.as_ref(), content_available)
                })
                .unwrap_or(content_available)
        } else {
            content_available
        };
        #[cfg(any(test, feature = "benchmark"))]
        super::plan_stats::note_child_measured();
        child_sizes.push(intrinsic_size_scoped(
            *child,
            child_available,
            Some(direction),
            viewport,
            child_font_px,
            nodes,
            cache,
            scope,
        )?);
    }
    let parent_box = gap_containing_block(style, content_available);
    let gap = style.main_gap_against_fonts(direction, parent_box, fonts);
    let cross_gap = style.cross_gap_against_fonts(direction, parent_box, fonts);
    // This node is every child's containing block.
    let child_edge_base = writing
        .logical_size(content_available.width, content_available.height)
        .0;
    let child_margin = |child, nodes: &LayoutInputMap<'_>| {
        nodes
            .style(child)
            .map(|style| {
                style.resolved_margin_against_fonts(
                    Some(child_edge_base),
                    fonts_of(&style, child_font_px),
                )
            })
            .unwrap_or_default()
    };
    // Items measured again at the main size their line gave them:
    // `(child, used main, size)`, re-checked by a measure plan.
    let mut hypothetical: Vec<(StableNodeId, f32, Size)> = Vec::new();
    let children = if uses_2d_grid(style, &flow_children, nodes) {
        let grid = layout_grid_2d(
            style,
            writing,
            &flow_children,
            &child_sizes,
            content_available,
            fonts,
            nodes,
            None,
        );
        // Columns run along the inline axis, rows along the block one.
        let (width, height) = writing.physical_size(
            grid_axis_extent(&grid.col_sizes, grid.col_gap),
            grid_axis_extent(&grid.row_sizes, grid.row_gap),
        );
        Size::new(width, height)
    } else if let Some(tracks) = grid_tracks.filter(|tracks| !tracks.is_empty()) {
        let auto_sizes = auto_track_contributions(
            &flow_children,
            tracks,
            content_available,
            direction == FlexDirection::Column,
            viewport,
            child_font_px,
            nodes,
            cache,
            scope,
        )?;
        let budget = main_extent(content_available, direction);
        let resolved = resolve_grid_track_sizes(tracks, budget, gap, &auto_sizes);
        grid_intrinsic_size(
            direction,
            &resolved,
            &child_sizes,
            &flow_children,
            child_edge_base,
            gap,
            child_font_px,
            nodes,
        )
    } else if wrapping {
        wrap_intrinsic_size(
            direction,
            wrap,
            &flow_children,
            &child_sizes,
            content_available,
            child_edge_base,
            gap,
            cross_gap,
            grid_tracks,
            viewport,
            child_font_px,
            nodes,
        )
    } else {
        // With a definite main size the line hands out free space (or takes
        // back overflow), and an item's cross size is its content's at the
        // main size it ends up with, as placement lays it out.
        let definite_main = match direction {
            FlexDirection::Row => spec_width.is_some(),
            FlexDirection::Column => spec_height.is_some(),
        };
        let mut used_sizes = child_sizes.clone();
        if definite_main {
            distribute_flex_main(
                &flow_children,
                &mut used_sizes,
                direction,
                content_available,
                child_edge_base,
                gap,
                viewport,
                child_font_px,
                nodes,
            );
            for (index, child) in flow_children.iter().enumerate() {
                let Some(child_style) = nodes.style(*child) else {
                    continue;
                };
                if !cross_follows_used_main(
                    &child_style,
                    direction,
                    child_sizes[index],
                    used_sizes[index],
                ) {
                    continue;
                }
                let at_main = intrinsic_size_at_main(
                    *child,
                    main_extent(used_sizes[index], direction),
                    direction,
                    content_available,
                    viewport,
                    child_font_px,
                    nodes,
                    cache,
                    scope,
                )?;
                set_cross_extent(
                    &mut used_sizes[index],
                    direction,
                    cross_extent(at_main, direction),
                );
                hypothetical.push((*child, main_extent(used_sizes[index], direction), at_main));
            }
        }
        let gaps = gap * flow_children.len().saturating_sub(1) as f32;
        let mut main = gaps;
        let mut cross = 0.0f32;
        for ((child, size), used) in flow_children.iter().zip(&child_sizes).zip(&used_sizes) {
            let margin = child_margin(*child, nodes);
            main += main_extent(*size, direction)
                + main_start_margin(margin, direction)
                + main_end_margin(margin, direction);
            cross = cross.max(cross_extent(*used, direction) + cross_margin(margin, direction));
        }
        match direction {
            FlexDirection::Row => Size::new(main.max(0.0), cross),
            FlexDirection::Column => Size::new(cross, main.max(0.0)),
        }
    };
    let text = text_metrics.unwrap_or_default();
    // Text that wrapped is as wide as the lines it wrapped to. The width it
    // asks for is its lines unwrapped, as far as this box gives it room: a
    // box that shrinks to its content widens again once its limit grows,
    // and then the text rewraps to the new width.
    let text_width = text_natural_width.map_or(text.width, |natural| {
        text.width.max(natural.min(content_available.width))
    });
    let mut content = Size::new(
        children.width.max(text_width),
        children.height.max(text.height),
    );
    #[cfg(feature = "rich-text")]
    if let Some(crate::StandardVisual::NativeMarkdown { blocks, .. }) =
        nodes.world.standard_visual(id)
    {
        let geometry = nodes.world.markdown_layout(
            id,
            &blocks,
            crate::LayoutBox {
                x: 0.0,
                y: 0.0,
                width: content_available.width,
                height: 0.0,
            },
        );
        content = Size::new(
            crate::rich_text::markdown_content_width(&blocks, &geometry),
            geometry.bounds.height,
        );
    }
    if text_metrics.is_none()
        && flow_children.is_empty()
        && let Some(fs) = style.font_size.filter(|value| *value > 0.0)
    {
        content.height = content
            .height
            .max(text_line_box_height_px(fs, style.line_height));
    }
    if let Some(crate::StandardVisual::Button {
        label,
        icon,
        trailing_icon,
        icon_size,
        icon_gap,
        loading,
        ..
    }) = nodes.world.standard_visual(id)
    {
        let leading = loading || icon.is_some();
        let trailing = trailing_icon.is_some();
        // Each glyph brings its own size, and a gap to whatever it stands
        // beside: the label, or the other glyph when there is no label.
        let glyphs = usize::from(leading) + usize::from(trailing);
        let parts = glyphs + usize::from(!label.is_empty());
        if glyphs > 0 {
            content.width += glyphs as f32 * icon_size + (parts - 1) as f32 * icon_gap;
            content.height = content.height.max(icon_size);
        }
    }
    if let Some(crate::StandardVisual::Checkbox { size, .. }) = nodes.world.standard_visual(id) {
        content.width += size.indicator_size()
            + if nodes.world.text(id).is_some_and(|label| !label.is_empty()) {
                size.indicator_gap()
            } else {
                0.0
            };
        content.height = content.height.max(size.indicator_size());
    }
    let max_content_w = content.width + chrome.width;
    let stacked_min_w = child_sizes
        .iter()
        .zip(&flow_children)
        .map(|(size, child)| {
            let margin = child_margin(*child, nodes);
            size.width + margin.left + margin.right
        })
        .fold(0.0f32, f32::max)
        + chrome.width;
    // nowrap row: min-content cannot be narrower than the packed sum.
    // wrap / column / block: min-content is the largest child (plus chrome).
    let min_content_w = if wrapping || direction.is_column() {
        stacked_min_w
    } else {
        max_content_w
    };
    let default_width = match style.width {
        Some(LengthSpec::MinContent) => min_content_w,
        Some(LengthSpec::MaxContent) | Some(LengthSpec::Shrink) => max_content_w,
        Some(LengthSpec::FitContent) => max_content_w.min(available.width).max(stacked_min_w),
        _ if parent_direction.is_none()
            && !style.width.is_some_and(LengthSpec::is_content_sized)
            && !flow_children.is_empty() =>
        {
            available.width
        }
        _ => max_content_w,
    };
    let default_height = content.height + chrome.height;
    let size = finish_intrinsic_size(
        style,
        fonts,
        viewport,
        available,
        edge_base,
        chrome,
        parent_direction,
        default_width,
        default_height,
    );
    // Record only on a scoped pass. A full pass rebuilds every container in
    // the document, so recording there costs an `Arc` clone per child and a
    // sort per container across the whole tree -- 1.79 -> 2.55 ms on the
    // 5,000-node canonical layout, measured. It buys one frame: the next
    // scoped pass would have found a plan waiting. `layout_document`
    // (css-parity, `layout_style_tree`, Vue `measure_layout`) throws the map
    // away entirely, and `force_full` has just cleared the retained cache, so
    // in both cases the plans would be built for nobody.
    if unforced && scope.is_some() {
        // Only the plain in-flow path is cacheable, for the same reasons the
        // placement plan is narrow. The grid-track path is excluded on top of
        // that because `auto_track_contributions` measures children against
        // constraints OTHER than `content_available`, and the plan re-checks a
        // child only against the one it recorded.
        let cacheable = !descendant_dependent_flow
            && !grid_measure
            && !ifc
            && grid_tracks.is_none_or(|tracks| tracks.is_empty())
            && nodes.world.children_layout_style_is_local(id);
        let recorded = cacheable.then(|| {
            // `flow_children` is a subsequence of `child_ids` -- that is what
            // `descendant_dependent_flow` being false means -- so one cursor
            // pairs each direct child with its measurement, if it has one.
            let mut flow_cursor = 0usize;
            let mut entries: Vec<MeasuredChild> = child_ids
                .iter()
                .copied()
                .map(|child| {
                    let intrinsic = (flow_children.get(flow_cursor) == Some(&child)).then(|| {
                        let size = child_sizes[flow_cursor];
                        flow_cursor += 1;
                        size
                    });
                    MeasuredChild {
                        child,
                        style: nodes.style(child),
                        intrinsic,
                        at_main: hypothetical
                            .iter()
                            .find(|(item, _, _)| *item == child)
                            .map(|(_, main, size)| (*main, *size)),
                    }
                })
                .collect();
            entries.sort_unstable_by_key(|entry| entry.child);
            MeasurePlan {
                available,
                parent_direction,
                viewport,
                parent_font_px,
                style: Arc::clone(&style_arc),
                writing,
                children: Arc::clone(&child_ids),
                text_metrics,
                text_natural_width,
                child_available: content_available,
                child_direction: direction,
                entries,
                size,
            }
        });
        let slots = nodes.measure_plans.entry(id).or_default();
        match recorded {
            Some(plan) => slots.insert(plan),
            // Leaving the entry empty retires the plans recorded while this
            // container was still on the cacheable path.
            None => slots.clear(),
        }
    }
    if unforced {
        cache.insert(cache_key, size);
    }
    Ok(size)
}

/// Re-check only the children the change closure reaches.
///
/// Everything outside the closure is unchanged by construction: a
/// layout-affecting `set_style` marks the node LAYOUT-dirty, which is what puts
/// it in the closure, and the caller has already confirmed that no
/// ancestor-derived adjustment can move a child's style without touching the
/// child.
///
/// Both halves matter. The intrinsic size alone misses a child whose margins
/// changed -- margins are part of the container's content extent but not of the
/// child's own measurement. The style alone misses a child that grew because
/// its OWN content grew, which is the case `content_growth_under_a_child_moves_
/// its_siblings` exists to hold.
fn measure_plan_children_unchanged(
    plan: &MeasurePlan,
    viewport: LayoutViewport,
    child_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    cache: &mut IntrinsicCache,
    scope: &ScopeContext<'_>,
) -> Result<bool, UiWorldError> {
    for affected in scope.affected.iter().copied() {
        let Some(entry) = plan.entry(affected) else {
            continue;
        };
        // Pointer first, value second -- the value compare only ever runs for
        // children in the change closure, so it stays off the per-sibling path.
        let matches = match (&nodes.style(entry.child), &entry.style) {
            (None, None) => true,
            (Some(current), Some(cached)) => {
                Arc::ptr_eq(current, cached) || layout_inputs_equal(current, cached)
            }
            _ => false,
        };
        if !matches {
            return Ok(false);
        }
        // A child the flow collection dropped contributes nothing, and its
        // style just compared equal, so it is still dropped.
        if let Some(cached) = entry.intrinsic {
            let measured = intrinsic_size_scoped(
                entry.child,
                plan.child_available,
                Some(plan.child_direction),
                viewport,
                child_font_px,
                nodes,
                cache,
                Some(scope),
            )?;
            if measured != cached {
                return Ok(false);
            }
        }
        // An item the line gave another main size is as tall as its content
        // at that size, which can change while its measurement above did not.
        if let Some((main, cached)) = entry.at_main {
            let measured = intrinsic_size_at_main(
                entry.child,
                main,
                plan.child_direction,
                plan.child_available,
                viewport,
                child_font_px,
                nodes,
                cache,
                Some(scope),
            )?;
            if measured != cached {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
