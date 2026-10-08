//! Shared Runtime layout grid algorithms.

use super::*;

pub(super) struct GridPlacedItem {
    pub(super) id: StableNodeId,
    pub(super) col: usize,
    pub(super) row: usize,
    pub(super) col_span: usize,
    pub(super) row_span: usize,
    pub(super) intrinsic: Size,
}

/// Parent track geometry for a subgrid item (already-resolved sizes, not templates).
#[derive(Debug, Clone)]
pub(super) struct InheritedGridTracks {
    pub(super) columns: Option<Vec<f32>>,
    pub(super) column_gap: f32,
    pub(super) rows: Option<Vec<f32>>,
    pub(super) row_gap: f32,
}

pub(super) struct Grid2DLayout {
    pub(super) col_sizes: Vec<f32>,
    pub(super) row_sizes: Vec<f32>,
    pub(super) col_gap: f32,
    pub(super) row_gap: f32,
    pub(super) items: Vec<GridPlacedItem>,
}

pub(super) fn grid_axis_extent(sizes: &[f32], gap: f32) -> f32 {
    sizes.iter().copied().sum::<f32>() + gap * sizes.len().saturating_sub(1) as f32
}

pub(super) fn explicit_column_tracks(
    style: &LayoutStyle,
    content_w: f32,
    col_gap: f32,
) -> Vec<GridTrack> {
    explicit_tracks(
        style.grid_columns_repeat.as_ref(),
        style.active_grid_columns(),
        style
            .grid_template_areas
            .as_ref()
            .map(GridTemplateAreas::column_count)
            .unwrap_or(0),
        content_w,
        col_gap,
        GridTrack::Fr(1.0),
    )
}

pub(super) fn explicit_row_tracks(
    style: &LayoutStyle,
    content_h: f32,
    row_gap: f32,
) -> Vec<GridTrack> {
    explicit_tracks(
        style.grid_rows_repeat.as_ref(),
        style.active_grid_rows(),
        style
            .grid_template_areas
            .as_ref()
            .map(GridTemplateAreas::row_count)
            .unwrap_or(0),
        content_h,
        row_gap,
        GridTrack::Auto,
    )
}

pub(super) fn explicit_tracks(
    repeat: Option<&GridRepeatAuto>,
    tracks: Option<&[GridTrack]>,
    area_count: usize,
    container: f32,
    gap: f32,
    area_fallback: GridTrack,
) -> Vec<GridTrack> {
    if let Some(repeat) = repeat {
        repeat.expand(container, gap)
    } else if let Some(tracks) = tracks {
        tracks.to_vec()
    } else if area_count > 0 {
        vec![area_fallback; area_count]
    } else {
        Vec::new()
    }
}

pub(super) fn expanded_repeat_line_names(
    repeat: Option<&GridRepeatAuto>,
    container: f32,
    gap: f32,
) -> Option<Vec<Vec<String>>> {
    let repeat = repeat.filter(|rep| rep.has_line_names())?;
    let names = repeat.expand_line_names(repeat.fill_count(container, gap));
    names.iter().any(|line| !line.is_empty()).then_some(names)
}

pub(super) fn implicit_grid_track(auto: Option<&[GridTrack]>, implicit_index: usize) -> GridTrack {
    match auto {
        Some(tracks) if !tracks.is_empty() => tracks[implicit_index % tracks.len()],
        _ => GridTrack::Auto,
    }
}

pub(super) fn ensure_grid_tracks(
    tracks: &mut Vec<GridTrack>,
    needed: usize,
    auto: Option<&[GridTrack]>,
    explicit: usize,
) {
    while tracks.len() < needed {
        let implicit_index = tracks.len().saturating_sub(explicit);
        tracks.push(implicit_grid_track(auto, implicit_index));
    }
}

/// 1-based CSS line → 0-based track boundary against the explicit grid.
/// Negative indexes count from the end (`-1` is the last line = `explicit` as
/// an exclusive track end).
pub(super) fn grid_line_boundary(index: i32, explicit: usize) -> i32 {
    if index > 0 {
        index - 1
    } else if index < 0 {
        explicit as i32 + index + 1
    } else {
        0
    }
}

pub(super) fn resolve_named_line(
    line: &GridLine,
    container: &LayoutStyle,
    columns: bool,
    after: Option<i32>,
    names: Option<&[Vec<String>]>,
) -> GridLine {
    match line {
        GridLine::Name(name) | GridLine::NthName(name, _) => {
            let occurrence = line.name_occurrence().unwrap_or(1) as u32;
            let index = if let Some(prev) = after {
                if columns {
                    container.named_column_line_after_from(name, prev, names)
                } else {
                    container.named_row_line_after_from(name, prev, names)
                }
            } else if columns {
                container.named_column_line_nth_from(name, occurrence, names)
            } else {
                container.named_row_line_nth_from(name, occurrence, names)
            };
            index.map(GridLine::Index).unwrap_or(GridLine::Auto)
        }
        other => other.clone(),
    }
}

pub(super) fn resolve_item_grid_placement(
    container: &LayoutStyle,
    placement: &nana_ui_core::GridPlacement,
    explicit_cols: usize,
    explicit_rows: usize,
    col_names: Option<&[Vec<String>]>,
    row_names: Option<&[Vec<String>]>,
) -> (Option<i32>, usize, Option<i32>, usize) {
    if let Some(name) = placement.area.as_deref()
        && let Some(areas) = container.grid_template_areas.as_ref()
        && let Some((col, row, col_span, row_span)) = areas.lookup(name)
    {
        return (
            Some(col as i32),
            col_span.max(1),
            Some(row as i32),
            row_span.max(1),
        );
    }
    let col_start = resolve_named_line(&placement.column_start, container, true, None, col_names);
    let col_end_after = match (
        &col_start,
        placement.column_start.as_name(),
        placement.column_end.as_name(),
    ) {
        (GridLine::Index(s), Some(a), Some(b)) if a == b => Some(*s),
        _ => None,
    };
    let col_end = resolve_named_line(
        &placement.column_end,
        container,
        true,
        col_end_after,
        col_names,
    );
    let row_start = resolve_named_line(&placement.row_start, container, false, None, row_names);
    let row_end_after = match (
        &row_start,
        placement.row_start.as_name(),
        placement.row_end.as_name(),
    ) {
        (GridLine::Index(s), Some(a), Some(b)) if a == b => Some(*s),
        _ => None,
    };
    let row_end = resolve_named_line(
        &placement.row_end,
        container,
        false,
        row_end_after,
        row_names,
    );
    let (col_origin, col_span) = resolve_grid_axis(&col_start, &col_end, explicit_cols);
    let (row_origin, row_span) = resolve_grid_axis(&row_start, &row_end, explicit_rows);
    (col_origin, col_span, row_origin, row_span)
}

pub(super) fn resolve_grid_axis(
    start: &GridLine,
    end: &GridLine,
    explicit: usize,
) -> (Option<i32>, usize) {
    let span_of = |n: u16| (n as usize).max(1);
    match (start, end) {
        (GridLine::Index(s), GridLine::Index(e)) => {
            let s = grid_line_boundary(*s, explicit);
            let e = grid_line_boundary(*e, explicit);
            let span = if e > s { (e - s) as usize } else { 1 };
            (Some(s), span)
        }
        (GridLine::Index(s), GridLine::Span(n)) => {
            (Some(grid_line_boundary(*s, explicit)), span_of(*n))
        }
        (GridLine::Span(n), GridLine::Index(e)) => {
            let span = span_of(*n);
            (Some(grid_line_boundary(*e, explicit) - span as i32), span)
        }
        (GridLine::Span(n), GridLine::Auto)
        | (GridLine::Auto, GridLine::Span(n))
        | (GridLine::Span(n), GridLine::Span(_)) => (None, span_of(*n)),
        (GridLine::Index(s), GridLine::Auto) => (Some(grid_line_boundary(*s, explicit)), 1),
        (GridLine::Auto, GridLine::Index(e)) => {
            let end = grid_line_boundary(*e, explicit);
            (Some(end - 1), 1)
        }
        (GridLine::Auto, GridLine::Auto)
        | (GridLine::Name(_), _)
        | (_, GridLine::Name(_))
        | (GridLine::NthName(_, _), _)
        | (_, GridLine::NthName(_, _)) => (None, 1),
    }
}

/// Per-row occupied column ranges `[start, end)`, merged and sorted.
#[derive(Default)]
pub(super) struct GridOccupancy {
    pub(super) rows: HashMap<usize, Vec<(usize, usize)>>,
}

impl GridOccupancy {
    pub(super) fn range_free(ranges: &[(usize, usize)], start: usize, end: usize) -> bool {
        !ranges.iter().any(|&(a, b)| a < end && start < b)
    }

    pub(super) fn free(&self, row: usize, col: usize, row_span: usize, col_span: usize) -> bool {
        let end = col.saturating_add(col_span);
        for r in row..row.saturating_add(row_span) {
            if let Some(ranges) = self.rows.get(&r)
                && !Self::range_free(ranges, col, end)
            {
                return false;
            }
        }
        true
    }

    pub(super) fn occupy(&mut self, row: usize, col: usize, row_span: usize, col_span: usize) {
        let end = col.saturating_add(col_span);
        for r in row..row.saturating_add(row_span) {
            let ranges = self.rows.entry(r).or_default();
            ranges.push((col, end));
            ranges.sort_unstable();
            let mut merged: Vec<(usize, usize)> = Vec::new();
            for (a, b) in ranges.drain(..) {
                if let Some(last) = merged.last_mut()
                    && a <= last.1
                {
                    last.1 = last.1.max(b);
                    continue;
                }
                merged.push((a, b));
            }
            *ranges = merged;
        }
    }
}

pub(super) fn search_grid_auto_slot(
    occupied: &GridOccupancy,
    row_origin: Option<usize>,
    col_origin: Option<usize>,
    row_span: usize,
    col_span: usize,
    col_wrap: usize,
    row_wrap: usize,
    start_row: usize,
    start_col: usize,
    column_flow: bool,
) -> (usize, usize) {
    if let (Some(row), Some(col)) = (row_origin, col_origin) {
        return (row, col);
    }
    let search_limit = 4096usize;
    if column_flow {
        let row_wrap = row_wrap.max(row_span);
        if let Some(col) = col_origin {
            for row in start_row..start_row.saturating_add(search_limit) {
                if occupied.free(row, col, row_span, col_span) {
                    return (row, col);
                }
            }
            // Past the scanned region — never silently reuse `start_row`.
            return (start_row.saturating_add(search_limit), col);
        }
        if let Some(row) = row_origin {
            for c in 0..search_limit {
                if occupied.free(row, c, row_span, col_span) {
                    return (row, c);
                }
            }
            return (row, search_limit);
        }
        let mut col = start_col;
        for _ in 0..search_limit {
            let row_begin = if col == start_col { start_row } else { 0 };
            let last = row_wrap.saturating_sub(row_span);
            if row_begin <= last {
                for row in row_begin..=last {
                    if occupied.free(row, col, row_span, col_span) {
                        return (row, col);
                    }
                }
            }
            col += 1;
        }
        (row_wrap, col)
    } else {
        let col_wrap = col_wrap.max(col_span);
        if let Some(row) = row_origin {
            let last = col_wrap.saturating_sub(col_span);
            for col in 0..=last {
                if occupied.free(row, col, row_span, col_span) {
                    return (row, col);
                }
            }
            // Implicit columns beyond the explicit wrap — never (row, 0).
            for col in last.saturating_add(1)..last.saturating_add(1).saturating_add(search_limit) {
                if occupied.free(row, col, row_span, col_span) {
                    return (row, col);
                }
            }
            return (row, last.saturating_add(1).saturating_add(search_limit));
        }
        if let Some(col) = col_origin {
            for row in start_row..start_row.saturating_add(search_limit) {
                if occupied.free(row, col, row_span, col_span) {
                    return (row, col);
                }
            }
            return (start_row.saturating_add(search_limit), col);
        }
        let mut row = start_row;
        for _ in 0..search_limit {
            let col_begin = if row == start_row { start_col } else { 0 };
            let last = col_wrap.saturating_sub(col_span);
            if col_begin <= last {
                for col in col_begin..=last {
                    if occupied.free(row, col, row_span, col_span) {
                        return (row, col);
                    }
                }
            }
            row += 1;
        }
        (row, start_col)
    }
}

pub(super) fn collapse_unoccupied_tracks(
    tracks: &[GridTrack],
    items: &mut [GridPlacedItem],
    columns: bool,
) -> Vec<GridTrack> {
    let n = tracks.len();
    if n == 0 {
        return Vec::new();
    }
    let mut used = vec![false; n];
    for item in items.iter() {
        let (origin, span) = if columns {
            (item.col, item.col_span)
        } else {
            (item.row, item.row_span)
        };
        let end = origin.saturating_add(span).min(n);
        for occupied in used.iter_mut().take(end).skip(origin) {
            *occupied = true;
        }
    }
    if used.iter().all(|occupied| *occupied) {
        return tracks.to_vec();
    }
    let mut map = vec![0usize; n];
    let mut next = Vec::new();
    for (index, track) in tracks.iter().copied().enumerate() {
        if used[index] {
            map[index] = next.len();
            next.push(track);
        }
    }
    if next.is_empty() {
        return tracks.to_vec();
    }
    for item in items.iter_mut() {
        if columns {
            if item.col < n {
                item.col = map[item.col];
            }
        } else if item.row < n {
            item.row = map[item.row];
        }
    }
    next
}

pub(super) fn layout_grid_2d(
    style: &LayoutStyle,
    writing: nana_ui_core::WritingContext,
    flow: &[StableNodeId],
    child_sizes: &[Size],
    content: Size,
    fonts: FontSizeContext,
    nodes: &LayoutInputMap<'_>,
    inherited: Option<&InheritedGridTracks>,
) -> Grid2DLayout {
    // Columns are tracks along the inline axis and rows along the block one
    // (CSS Grid §3): the page's width and height in `horizontal-tb`, its
    // height and width in a vertical mode. Everything below sizes tracks on
    // those logical extents; `place_grid_2d_items` turns them onto the page.
    let (content_inline, content_block) = writing.logical_size(content.width, content.height);
    let mut col_gap = style.resolved_column_gap_against_fonts(
        Some(content_inline).filter(|inline| *inline > 0.0),
        fonts,
    );
    let mut row_gap = style.resolved_row_gap_against_fonts(
        Some(content_block)
            .filter(|block| *block > 0.0)
            .or(Some(content_inline).filter(|inline| *inline > 0.0)),
        fonts,
    );
    let mut col_tracks = if style.is_subgrid_columns() {
        if let Some(sizes) = inherited
            .and_then(|grid| grid.columns.as_deref())
            .filter(|sizes| !sizes.is_empty())
        {
            col_gap = inherited.map(|grid| grid.column_gap).unwrap_or(col_gap);
            sizes.iter().copied().map(GridTrack::Px).collect()
        } else {
            // No parent tracks: `subgrid` computes to `none`. Do not invent auto tracks.
            Vec::new()
        }
    } else {
        explicit_column_tracks(style, content_inline, col_gap)
    };
    let mut row_tracks = if style.is_subgrid_rows() {
        if let Some(sizes) = inherited
            .and_then(|grid| grid.rows.as_deref())
            .filter(|sizes| !sizes.is_empty())
        {
            row_gap = inherited.map(|grid| grid.row_gap).unwrap_or(row_gap);
            sizes.iter().copied().map(GridTrack::Px).collect()
        } else {
            Vec::new()
        }
    } else {
        explicit_row_tracks(style, content_block, row_gap)
    };
    let explicit_cols = col_tracks.len();
    let explicit_rows = row_tracks.len();
    let auto_cols = style.grid_auto_columns.as_deref().filter(|t| !t.is_empty());
    let auto_rows = style.grid_auto_rows.as_deref().filter(|t| !t.is_empty());
    let auto_flow = style.grid_auto_flow.unwrap_or(GridAutoFlow::Row);
    let column_flow = auto_flow.is_column();
    let dense = auto_flow.is_dense();

    let default_placement = GridPlacement::default();
    let col_repeat_names =
        expanded_repeat_line_names(style.grid_columns_repeat.as_ref(), content_inline, col_gap);
    let row_repeat_names =
        expanded_repeat_line_names(style.grid_rows_repeat.as_ref(), content_block, row_gap);
    let col_names = col_repeat_names
        .as_deref()
        .or(style.grid_column_line_names.as_deref());
    let row_names = row_repeat_names
        .as_deref()
        .or(style.grid_row_line_names.as_deref());
    let mut pending = Vec::with_capacity(flow.len());
    for (id, intrinsic) in flow.iter().copied().zip(child_sizes.iter().copied()) {
        let child_style = nodes.style(id);
        let placement = child_style
            .as_ref()
            .map(|child| &*child.grid_placement)
            .unwrap_or(&default_placement);
        let (col_origin, col_span, row_origin, row_span) = resolve_item_grid_placement(
            style,
            placement,
            explicit_cols,
            explicit_rows,
            col_names,
            row_names,
        );
        pending.push((
            id,
            intrinsic,
            col_origin,
            col_span.max(1),
            row_origin,
            row_span.max(1),
        ));
    }

    let mut occupied = GridOccupancy::default();
    let mut items: Vec<GridPlacedItem> = Vec::with_capacity(pending.len());
    let mut placed = vec![false; pending.len()];

    let place_at = |items: &mut Vec<GridPlacedItem>,
                    col_tracks: &mut Vec<GridTrack>,
                    row_tracks: &mut Vec<GridTrack>,
                    occupied: &mut GridOccupancy,
                    id: StableNodeId,
                    intrinsic: Size,
                    row: usize,
                    col: usize,
                    row_span: usize,
                    col_span: usize| {
        ensure_grid_tracks(
            col_tracks,
            col.saturating_add(col_span),
            auto_cols,
            explicit_cols,
        );
        ensure_grid_tracks(
            row_tracks,
            row.saturating_add(row_span),
            auto_rows,
            explicit_rows,
        );
        occupied.occupy(row, col, row_span, col_span);
        items.push(GridPlacedItem {
            id,
            col,
            row,
            col_span,
            row_span,
            intrinsic,
        });
    };

    // Pass 1: both axes definite.
    for (index, &(id, intrinsic, col_origin, col_span, row_origin, row_span)) in
        pending.iter().enumerate()
    {
        let (Some(col), Some(row)) = (col_origin, row_origin) else {
            continue;
        };
        let col = col.max(0) as usize;
        let row = row.max(0) as usize;
        place_at(
            &mut items,
            &mut col_tracks,
            &mut row_tracks,
            &mut occupied,
            id,
            intrinsic,
            row,
            col,
            row_span,
            col_span,
        );
        placed[index] = true;
    }

    let mut cursor_row = 0usize;
    let mut cursor_col = 0usize;
    for (index, &(id, intrinsic, col_origin, col_span, row_origin, row_span)) in
        pending.iter().enumerate()
    {
        if placed[index] {
            continue;
        }
        if let Some(col) = col_origin {
            ensure_grid_tracks(
                &mut col_tracks,
                (col.max(0) as usize).saturating_add(col_span),
                auto_cols,
                explicit_cols,
            );
        }
        if let Some(row) = row_origin {
            ensure_grid_tracks(
                &mut row_tracks,
                (row.max(0) as usize).saturating_add(row_span),
                auto_rows,
                explicit_rows,
            );
        }
        if col_span > col_tracks.len() {
            ensure_grid_tracks(&mut col_tracks, col_span, auto_cols, explicit_cols);
        }
        if row_span > row_tracks.len() {
            ensure_grid_tracks(&mut row_tracks, row_span, auto_rows, explicit_rows);
        }
        let start_row = if dense { 0 } else { cursor_row };
        let start_col = if dense { 0 } else { cursor_col };
        let (row, col) = search_grid_auto_slot(
            &occupied,
            row_origin.map(|v| v.max(0) as usize),
            col_origin.map(|v| v.max(0) as usize),
            row_span,
            col_span,
            col_tracks.len(),
            row_tracks.len(),
            start_row,
            start_col,
            column_flow,
        );
        place_at(
            &mut items,
            &mut col_tracks,
            &mut row_tracks,
            &mut occupied,
            id,
            intrinsic,
            row,
            col,
            row_span,
            col_span,
        );
        if !dense {
            if column_flow {
                cursor_col = col;
                cursor_row = row.saturating_add(row_span);
            } else {
                cursor_row = row;
                cursor_col = col.saturating_add(col_span);
            }
        }
    }

    if style
        .grid_columns_repeat
        .as_ref()
        .is_some_and(|repeat| repeat.kind.is_auto_fit())
    {
        col_tracks = collapse_unoccupied_tracks(&col_tracks, &mut items, true);
    }
    if style
        .grid_rows_repeat
        .as_ref()
        .is_some_and(|repeat| repeat.kind.is_auto_fit())
    {
        row_tracks = collapse_unoccupied_tracks(&row_tracks, &mut items, false);
    }

    let mut col_auto = vec![0.0f32; col_tracks.len()];
    let mut row_auto = vec![0.0f32; row_tracks.len()];
    for item in &items {
        let margin = nodes
            .style(item.id)
            .map(|style| {
                style.resolved_margin_against_fonts(
                    Some(writing.inline_size(content.width, content.height)),
                    fonts_of(&style, fonts.element_px),
                )
            })
            .unwrap_or_default();
        let (inline, block) = writing.logical_size(item.intrinsic.width, item.intrinsic.height);
        let inline_margins =
            margin_at(margin, writing.inline_start()) + margin_at(margin, writing.inline_end());
        let block_margins =
            margin_at(margin, writing.block_start()) + margin_at(margin, writing.block_end());
        if item.col_span == 1 && item.col < col_auto.len() {
            col_auto[item.col] = col_auto[item.col].max(inline + inline_margins);
        }
        if item.row_span == 1 && item.row < row_auto.len() {
            row_auto[item.row] = row_auto[item.row].max(block + block_margins);
        }
    }
    let col_sizes = resolve_grid_track_sizes(&col_tracks, content_inline, col_gap, &col_auto);
    let mut row_sizes = resolve_grid_track_sizes(&row_tracks, content_block, row_gap, &row_auto);
    // Leftover definite block extent goes to *empty* auto rows so
    // `height:100%` / empty stretch have a cell, without inflating
    // content-sized auto rows.
    distribute_auto_track_leftover(&row_tracks, &mut row_sizes, content_block, row_gap);
    Grid2DLayout {
        col_sizes,
        row_sizes,
        col_gap,
        row_gap,
        items,
    }
}

pub(super) fn distribute_auto_track_leftover(
    tracks: &[GridTrack],
    sizes: &mut [f32],
    container: f32,
    gap: f32,
) {
    if container <= 0.5 || sizes.is_empty() || sizes.len() != tracks.len() {
        return;
    }
    let used = sizes.iter().copied().sum::<f32>() + gap * sizes.len().saturating_sub(1) as f32;
    let leftover = container - used;
    if leftover <= 0.5 {
        return;
    }
    // Only empty auto rows (no intrinsic). Content-sized auto rows stay
    // tight so `align-items:start` items keep their packed y (T-G26).
    let autos: Vec<usize> = tracks
        .iter()
        .enumerate()
        .filter(|(index, track)| matches!(track, GridTrack::Auto) && sizes[*index] <= 0.5)
        .map(|(index, _)| index)
        .collect();
    if autos.is_empty() {
        return;
    }
    let share = leftover / autos.len() as f32;
    for index in autos {
        sizes[index] += share;
    }
}

pub(super) fn grid_track_offsets(sizes: &[f32], gap: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(sizes.len());
    let mut acc = 0.0;
    for (index, size) in sizes.iter().copied().enumerate() {
        out.push(acc);
        acc += size;
        if index + 1 < sizes.len() {
            acc += gap;
        }
    }
    out
}

pub(super) fn grid_span_extent(sizes: &[f32], start: usize, span: usize, gap: f32) -> f32 {
    if span == 0 || start >= sizes.len() {
        return 0.0;
    }
    let end = start.saturating_add(span).min(sizes.len());
    let sum: f32 = sizes[start..end].iter().copied().sum();
    sum + gap * (end - start).saturating_sub(1) as f32
}

pub(super) fn spanned_track_sizes(sizes: &[f32], start: usize, span: usize) -> Vec<f32> {
    if span == 0 || start >= sizes.len() {
        return Vec::new();
    }
    let end = start.saturating_add(span).min(sizes.len());
    sizes[start..end].to_vec()
}

pub(super) fn size_is_indefinite(spec: Option<LengthSpec>) -> bool {
    !spec.is_some_and(LengthSpec::is_definite_declared)
}

pub(super) fn align_in_grid_cell(
    align: AlignSpec,
    used: f32,
    cell: f32,
    stretch: bool,
) -> (f32, f32) {
    if stretch {
        return (0.0, cell.max(0.0));
    }
    if used + 1e-6 >= cell {
        return (0.0, used);
    }
    let offset = match align {
        AlignSpec::Start | AlignSpec::Stretch | AlignSpec::Baseline => 0.0,
        AlignSpec::Center => ((cell - used) / 2.0).max(0.0),
        AlignSpec::End => (cell - used).max(0.0),
    };
    (offset, used)
}

#[allow(clippy::too_many_arguments)]
/// Mirror an inline-axis start/end pair. `direction: rtl` makes inline-start the
/// physical right edge, so `justify-*: start` must resolve rightwards. Center,
/// stretch and baseline have no side to swap.
/// A resolved margin on one page edge.
pub(super) fn margin_at(
    margin: nana_ui_core::PaddingSpec,
    edge: nana_ui_core::PhysicalEdge,
) -> f32 {
    match edge {
        nana_ui_core::PhysicalEdge::Top => margin.top,
        nana_ui_core::PhysicalEdge::Right => margin.right,
        nana_ui_core::PhysicalEdge::Bottom => margin.bottom,
        nana_ui_core::PhysicalEdge::Left => margin.left,
    }
}

pub(super) fn place_grid_2d_items(
    grid: &Grid2DLayout,
    writing: nana_ui_core::WritingContext,
    content_origin: Point,
    content: Size,
    style: &LayoutStyle,
    viewport: LayoutViewport,
    child_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    intrinsic: &mut IntrinsicCache,
    output: &mut HashMap<StableNodeId, LayoutBox>,
    scope: Option<&ScopeContext<'_>>,
) -> Result<(), UiWorldError> {
    let col_off = grid_track_offsets(&grid.col_sizes, grid.col_gap);
    let row_off = grid_track_offsets(&grid.row_sizes, grid.row_gap);
    // Cells and items are placed flow-relative — columns from the
    // inline-start edge, rows from the block-start one, `justify-self` along
    // the inline axis and `align-self` along the block one, all as authored —
    // and turned onto the page by the writing context. An RTL grid's first
    // column is the rightmost; a track block narrower than the content box
    // packs against the start edge, and one wider overflows past the *end*
    // edge (no clamp: fixed-px tracks are never shrunk, so ordinary grids get
    // here).
    let box_extents = writing.logical_size(content.width, content.height);
    let page = |rect| {
        let (x, y, width, height) = writing.flow_rect_to_page(rect, box_extents);
        (Point { x, y }, Size::new(width, height))
    };
    for item in &grid.items {
        let Some(child_style) = nodes.style(item.id) else {
            continue;
        };
        let child_style = child_style.as_ref();
        let child_fonts = fonts_of(child_style, child_font_px);
        let cell_inline = col_off.get(item.col).copied().unwrap_or(0.0);
        let cell_block = row_off.get(item.row).copied().unwrap_or(0.0);
        let cell_inline_size =
            grid_span_extent(&grid.col_sizes, item.col, item.col_span, grid.col_gap);
        let cell_block_size =
            grid_span_extent(&grid.row_sizes, item.row, item.row_span, grid.row_gap);
        let (_, cell) = page((cell_inline, cell_block, cell_inline_size, cell_block_size));
        // Final tracks are the containing block for item padding and descendants.
        let measured = intrinsic_size_scoped(
            item.id,
            cell,
            Some(FlexDirection::Row),
            viewport,
            child_font_px,
            nodes,
            intrinsic,
            scope,
        )?;
        let margin = child_style.resolved_margin_against_fonts(
            Some(writing.inline_size(cell.width, cell.height)),
            child_fonts,
        );
        let inline_lead = margin_at(margin, writing.inline_start());
        let block_lead = margin_at(margin, writing.block_start());
        let inner_inline =
            (cell_inline_size - inline_lead - margin_at(margin, writing.inline_end())).max(0.0);
        let inner_block =
            (cell_block_size - block_lead - margin_at(margin, writing.block_end())).max(0.0);
        let justify = child_style.resolved_justify_self(style.justify_items);
        let align = child_style.resolved_align_self(style.align_items);
        let (inline_spec, block_spec) = if writing.is_vertical() {
            (child_style.height, child_style.width)
        } else {
            (child_style.width, child_style.height)
        };
        // An aspect ratio fills the height from a definite width, so a
        // horizontal item with one is not stretched down its block axis.
        let ratio_filled_height = aspect_ratio_is_usable(child_style)
            && child_style
                .width
                .is_some_and(LengthSpec::is_definite_declared);
        let stretch_inline = justify == AlignSpec::Stretch && size_is_indefinite(inline_spec);
        let stretch_block = align == AlignSpec::Stretch
            && size_is_indefinite(block_spec)
            && !(ratio_filled_height && !writing.is_vertical());
        // Measurement already resolved declared lengths, min/max and box-sizing
        // against the final cell. Re-resolving here would drop content-box chrome.
        let (measured_inline, measured_block) =
            writing.logical_size(measured.width, measured.height);
        let (off_inline, used_inline) =
            align_in_grid_cell(justify, measured_inline, inner_inline, stretch_inline);
        let (off_block, used_block) =
            align_in_grid_cell(align, measured_block, inner_block, stretch_block);
        let (offset, mut child_size) = page((
            cell_inline + inline_lead + off_inline,
            cell_block + block_lead + off_block,
            used_inline,
            used_block,
        ));
        let stretch_height = if writing.is_vertical() {
            stretch_inline
        } else {
            stretch_block
        };
        if !stretch_height {
            fill_auto_height_from_aspect_ratio(
                child_style,
                &mut child_size,
                Some(writing.inline_size(cell.width, cell.height)),
                child_fonts,
            );
        }
        let child_origin = Point {
            x: content_origin.x + offset.x,
            y: content_origin.y + offset.y,
        };
        if !subtree_unchanged(
            item.id,
            child_origin,
            child_size,
            cell,
            child_style,
            child_fonts,
            writing,
            scope,
        ) {
            let inherited = if child_style.is_subgrid_columns() || child_style.is_subgrid_rows() {
                Some(InheritedGridTracks {
                    columns: child_style
                        .is_subgrid_columns()
                        .then(|| spanned_track_sizes(&grid.col_sizes, item.col, item.col_span)),
                    column_gap: grid.col_gap,
                    rows: child_style
                        .is_subgrid_rows()
                        .then(|| spanned_track_sizes(&grid.row_sizes, item.row, item.row_span)),
                    row_gap: grid.row_gap,
                })
            } else {
                None
            };
            place_node_scoped(
                item.id,
                child_origin,
                child_size,
                cell,
                viewport,
                child_font_px,
                nodes,
                intrinsic,
                output,
                scope,
                inherited.as_ref(),
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn grid_intrinsic_size(
    direction: FlexDirection,
    tracks: &[f32],
    child_sizes: &[Size],
    children: &[StableNodeId],
    // Percent base of the children's margins: the container's inline size.
    edge_base: f32,
    gap: f32,
    parent_font_px: f32,
    nodes: &LayoutInputMap<'_>,
) -> Size {
    let gaps = gap * tracks.len().saturating_sub(1) as f32;
    let main = tracks.iter().sum::<f32>() + gaps;
    let mut cross = 0.0f32;
    for (index, child) in children.iter().enumerate() {
        let margin = nodes
            .style(*child)
            .map(|style| {
                style.resolved_margin_against_fonts(
                    Some(edge_base),
                    fonts_of(style.as_ref(), parent_font_px),
                )
            })
            .unwrap_or_default();
        let size = child_sizes.get(index).copied().unwrap_or_default();
        cross = cross.max(cross_extent(size, direction) + cross_margin(margin, direction));
    }
    match direction {
        FlexDirection::Row => Size::new(main, cross),
        FlexDirection::Column => Size::new(cross, main),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn auto_track_contributions(
    children: &[StableNodeId],
    tracks: &[GridTrack],
    content: Size,
    column_main: bool,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    cache: &mut IntrinsicCache,
    scope: Option<&ScopeContext<'_>>,
) -> Result<Vec<f32>, UiWorldError> {
    let n = tracks.len().min(children.len());
    let mut sizes = vec![0.0f32; n];
    if !tracks.iter().any(|track| matches!(track, GridTrack::Auto)) {
        return Ok(sizes);
    }
    let direction = if column_main {
        FlexDirection::Column
    } else {
        FlexDirection::Row
    };
    for index in 0..n {
        if !matches!(tracks[index], GridTrack::Auto) {
            continue;
        }
        let child = children[index];
        let Some(style) = nodes.style(child) else {
            continue;
        };
        let axis_spec = if column_main {
            style.height
        } else {
            style.width
        };
        let percent_base = if column_main {
            content.height
        } else {
            content.width
        };
        if let Some(px) = resolve_child_main(
            axis_spec,
            percent_base,
            viewport,
            fonts_of(style.as_ref(), parent_font_px),
        ) {
            sizes[index] = px.max(0.0);
            continue;
        }
        let available = if column_main {
            Size::new(content.width, 0.0)
        } else {
            Size::new(0.0, content.height)
        };
        let measured = intrinsic_size_demoted(
            child,
            available,
            Some(direction),
            viewport,
            parent_font_px,
            nodes,
            cache,
            scope,
            column_main,
        )?;
        sizes[index] = if column_main {
            measured.height
        } else {
            measured.width
        };
    }
    Ok(sizes)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn intrinsic_size_demoted(
    id: StableNodeId,
    available: Size,
    parent_direction: Option<FlexDirection>,
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    cache: &mut IntrinsicCache,
    scope: Option<&ScopeContext<'_>>,
    column_main: bool,
) -> Result<Size, UiWorldError> {
    // Auto-track throwaway: Fill / 100% on the track axis must not snap to the
    // grid's definite size. Measure against a near-zero available on that axis
    // after treating Fill/100% as content-sized via `available` 0.
    let _ = column_main;
    intrinsic_size_scoped(
        id,
        available,
        parent_direction,
        viewport,
        parent_font_px,
        nodes,
        cache,
        scope,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn apply_grid_main_sizes(
    children: &[StableNodeId],
    sizes: &mut [Size],
    direction: FlexDirection,
    content: Size,
    // Percent base of the children's margins: the container's inline size.
    edge_base: f32,
    gap: f32,
    tracks: &[GridTrack],
    viewport: LayoutViewport,
    parent_font_px: f32,
    nodes: &mut LayoutInputMap<'_>,
    intrinsic: &mut IntrinsicCache,
    scope: Option<&ScopeContext<'_>>,
) -> Result<(), UiWorldError> {
    let n = children.len();
    if n == 0 {
        return Ok(());
    }
    let mut margins = Vec::with_capacity(n);
    for child in children {
        let margin = nodes
            .style(*child)
            .map(|style| {
                style.resolved_margin_against_fonts(
                    Some(edge_base),
                    fonts_of(style.as_ref(), parent_font_px),
                )
            })
            .unwrap_or_default();
        margins.push(margin);
    }
    let track_n = n.min(tracks.len());
    let margin_total: f32 = margins
        .iter()
        .take(track_n)
        .map(|margin| main_start_margin(*margin, direction) + main_end_margin(*margin, direction))
        .sum();
    let budget = (main_extent(content, direction) - margin_total).max(0.0);
    let auto_sizes = auto_track_contributions(
        &children[..track_n],
        &tracks[..track_n],
        content,
        direction == FlexDirection::Column,
        viewport,
        parent_font_px,
        nodes,
        intrinsic,
        scope,
    )?;
    let mut resolved = resolve_grid_track_sizes(&tracks[..track_n], budget, gap, &auto_sizes);
    if resolved.len() < n {
        let used: f32 =
            resolved.iter().sum::<f32>() + gap * resolved.len().saturating_sub(1) as f32;
        let rem = (budget - used).max(0.0);
        let extra = n - resolved.len();
        let each = if extra > 0 { rem / extra as f32 } else { 0.0 };
        resolved.extend(std::iter::repeat_n(each, extra));
    }
    for (size, main) in sizes.iter_mut().zip(resolved) {
        set_main_extent(size, direction, main);
    }
    Ok(())
}
