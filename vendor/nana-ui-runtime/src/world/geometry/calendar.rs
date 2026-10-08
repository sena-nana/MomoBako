//! calendar geometry from committed node data.

use super::*;

pub(in crate::world) fn calendar_heatmap_geometry(
    bounds: LayoutBox,
    cells: &[crate::CalendarHeatmapCellPaint],
    month_labels: &[crate::CalendarHeatmapLabelPaint],
    day_labels: &[crate::CalendarHeatmapLabelPaint],
    cell_size: f32,
    max_level: u8,
    active: Option<usize>,
    active_title: Option<&str>,
    mode: ThemeMode,
    palette: &SemanticPalette,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::ComponentGeometry {
    let painted = cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let is_active = Some(index) == active;
            let fill = if is_active && cell.level >= max_level {
                palette.accent
            } else {
                let level = if is_active {
                    cell.level.saturating_add(1).max(1)
                } else {
                    cell.level
                };
                crate::calendar_cell_fill(mode, level, max_level)
            };
            (
                LayoutBox {
                    x: bounds.x + cell.x,
                    y: bounds.y + cell.y,
                    width: cell_size,
                    height: cell_size,
                },
                fill.as_rgba_array(),
            )
        })
        .collect::<Vec<_>>();
    let mut labels = Vec::with_capacity(month_labels.len() + day_labels.len());
    labels.extend(month_labels.iter().map(|label| {
        axis_label_region(
            bounds,
            &label.text,
            label.x,
            label.y,
            nana_ui_core::type_scale::HINT,
            true,
            palette,
            measure,
        )
    }));
    labels.extend(day_labels.iter().map(|label| {
        axis_label_region(
            bounds,
            &label.text,
            label.x,
            label.y,
            nana_ui_core::type_scale::HINT,
            false,
            palette,
            measure,
        )
    }));
    let hover = active.and_then(|index| cells.get(index)).map(|cell| {
        calendar_hover_chrome(
            bounds,
            cell,
            cell_size,
            active_title.unwrap_or(""),
            palette,
            measure,
        )
    });
    crate::ComponentGeometry::CalendarHeatmap {
        cells: painted,
        labels,
        hover,
    }
}

pub(in crate::world) fn calendar_hover_chrome(
    bounds: LayoutBox,
    cell: &crate::CalendarHeatmapCellPaint,
    cell_size: f32,
    title: &str,
    palette: &SemanticPalette,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::CalendarHoverGeometry {
    let pad_x = TooltipConfig::PADDING_X;
    let pad_y = TooltipConfig::PADDING_Y;
    let font_size = TooltipConfig::FONT_SIZE;
    let gap = TooltipConfig::default().gap;
    let max_width = TooltipConfig::default().max_width;
    let text_width = measure.width(title, font_size, None);
    let tooltip_width = (text_width + pad_x * 2.0).clamp(font_size + pad_x * 2.0, max_width);
    let tooltip_height = font_size + pad_y * 2.0;
    let ring = LayoutBox {
        x: bounds.x + cell.x - nana_ui_core::HAIRLINE,
        y: bounds.y + cell.y - nana_ui_core::HAIRLINE,
        width: cell_size + nana_ui_core::HAIRLINE * 2.0,
        height: cell_size + nana_ui_core::HAIRLINE * 2.0,
    };
    let tooltip_x = if cell.x > bounds.width / 2.0 {
        (cell.x + cell_size - tooltip_width).max(0.0)
    } else {
        cell.x.min((bounds.width - tooltip_width).max(0.0))
    };
    let tooltip_y = if cell.y < bounds.height / 2.0 {
        (cell.y + cell_size + gap).min((bounds.height - tooltip_height).max(0.0))
    } else {
        (cell.y - tooltip_height - gap).max(0.0)
    };
    let tooltip = LayoutBox {
        x: bounds.x + tooltip_x,
        y: bounds.y + tooltip_y,
        width: tooltip_width,
        height: tooltip_height,
    };
    crate::CalendarHoverGeometry {
        ring,
        tooltip,
        title: crate::ComponentTextRegion {
            bounds: LayoutBox {
                x: tooltip.x + pad_x,
                y: tooltip.y + pad_y,
                width: (tooltip_width - pad_x * 2.0).max(0.0),
                height: font_size,
            },
            content: crate::TextValue::from(title),
            color: Some(palette.text.as_rgba_array()),
            font_size,
            font_weight: None,
        },
        ring_color: palette.text.as_rgba_array(),
        tooltip_fill: palette.surface.as_rgba_array(),
        tooltip_border: palette.border_soft.as_rgba_array(),
    }
}

pub(in crate::world) fn axis_label_region(
    bounds: LayoutBox,
    text: &Arc<str>,
    x: f32,
    y: f32,
    font_size: f32,
    center: bool,
    palette: &SemanticPalette,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::ComponentTextRegion {
    let width = measure.width(text, font_size, None) + nana_ui_core::space::XXS;
    crate::ComponentTextRegion {
        bounds: LayoutBox {
            x: bounds.x + x - if center { width * 0.5 } else { 0.0 },
            y: bounds.y + y,
            width,
            height: font_size + nana_ui_core::space::XXS,
        },
        content: Arc::clone(text).into(),
        color: Some(palette.muted.as_rgba_array()),
        font_size,
        font_weight: None,
    }
}
