//! controls geometry from committed node data.

use super::*;

pub(in crate::world) fn reorder_list_geometry(
    bounds: LayoutBox,
    rows: &[crate::ReorderRowPaint],
    size: ControlSize,
    spacing: f32,
    insert: Option<LayoutBox>,
    palette: &SemanticPalette,
    metrics: nana_ui_core::ThemeMetrics,
) -> crate::ComponentGeometry {
    let height = size.height_in(metrics);
    let spacing = spacing.max(0.0);
    let pad = nana_ui_core::space::MD;
    let rows = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let row_bounds = LayoutBox {
                x: bounds.x,
                y: bounds.y + index as f32 * (height + spacing),
                width: bounds.width,
                height,
            };
            let label = crate::ComponentTextRegion {
                bounds: LayoutBox {
                    x: row_bounds.x + pad,
                    y: row_bounds.y,
                    width: (row_bounds.width - pad * 2.0).max(0.0),
                    height: row_bounds.height,
                },
                content: Arc::clone(&row.label).into(),
                color: Some(if row.disabled {
                    palette.muted.as_rgba_array()
                } else {
                    palette.text.as_rgba_array()
                }),
                font_size: size.text_size(),
                font_weight: None,
            };
            let fill = row.selected.then_some(palette.selected.as_rgba_array());
            (row_bounds, label, fill)
        })
        .collect();
    crate::ComponentGeometry::ReorderList {
        rows,
        insert: insert.map(|line| (line, palette.accent.as_rgba_array())),
    }
}
