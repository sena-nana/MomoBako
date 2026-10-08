//! charts geometry from committed node data.

use super::*;

pub(in crate::world) fn time_series_geometry(
    bounds: LayoutBox,
    values: &[f64],
    mode: ThemeMode,
) -> crate::ComponentGeometry {
    let chart = crate::TimeSeriesChart::new(values.iter().copied());
    let paint = crate::time_series_paint(mode);
    let local = LayoutBox {
        x: 0.0,
        y: 0.0,
        width: bounds.width,
        height: bounds.height,
    };
    let inset_x = crate::TimeSeriesChart::INSET_X;
    let grid = crate::TimeSeriesChart::grid_ys(local)
        .into_iter()
        .map(|y| LayoutBox {
            x: bounds.x + inset_x,
            y: bounds.y + y,
            width: (bounds.width - inset_x * 2.0).max(0.0),
            height: 1.0,
        })
        .collect();
    let points = chart
        .points(local)
        .into_iter()
        .map(|(x, y)| [bounds.x + x, bounds.y + y])
        .collect::<Vec<_>>();
    let baseline = bounds.y
        + (bounds.height - crate::TimeSeriesChart::INSET_Y).max(crate::TimeSeriesChart::INSET_Y);
    crate::ComponentGeometry::TimeSeriesChart {
        grid,
        area: area_under_polyline(&points, baseline),
        line: points,
        grid_color: paint.grid.as_rgba_array(),
        area_color: paint.area.as_rgba_array(),
        line_color: paint.line.as_rgba_array(),
    }
}

pub(in crate::world) fn timestamp_series_geometry(
    bounds: LayoutBox,
    samples: &[(i64, Option<f64>)],
    unit: Option<&str>,
    time_labels: Option<&(Arc<str>, Arc<str>)>,
    mode: ThemeMode,
) -> crate::ComponentGeometry {
    let chart = crate::TimeSeriesChart::from_samples(samples.iter().copied());
    let paint = crate::time_series_paint(mode);
    // Reserve vertical space for the scale and endpoint labels.
    let plot = LayoutBox {
        x: bounds.x,
        y: bounds.y + 18.0,
        width: bounds.width,
        height: (bounds.height - 38.0).max(1.0),
    };
    let segments: Vec<Vec<[f32; 2]>> = chart
        .segments(plot)
        .into_iter()
        .map(|run| {
            run.into_iter()
                .map(|(x, y)| [plot.x + x, plot.y + y])
                .collect()
        })
        .collect();
    let baseline = plot.y
        + (plot.height - crate::TimeSeriesChart::INSET_Y).max(crate::TimeSeriesChart::INSET_Y);
    let area = segments
        .iter()
        .flat_map(|run| area_under_polyline(run, baseline))
        .collect();
    let grid = crate::TimeSeriesChart::grid_ys(plot)
        .into_iter()
        .map(|y| LayoutBox {
            x: plot.x + crate::TimeSeriesChart::INSET_X,
            y: plot.y + y,
            width: (plot.width - 2.0 * crate::TimeSeriesChart::INSET_X).max(0.0),
            height: 1.0,
        })
        .collect();
    let label = |text: Arc<str>, x: f32, y: f32, width: f32| crate::ComponentTextRegion {
        bounds: LayoutBox {
            x,
            y,
            width,
            height: nana_ui_core::type_scale::LINE,
        },
        content: text.into(),
        color: Some(mode.palette().muted.as_rgba_array()),
        font_size: nana_ui_core::type_scale::HINT,
        font_weight: None,
    };
    let maximum = samples
        .iter()
        .filter_map(|sample| sample.1)
        .filter(|v| v.is_finite())
        .fold(1.0_f64, f64::max);
    let mut labels = vec![label(
        if samples
            .iter()
            .any(|sample| sample.1.is_some_and(f64::is_finite))
        {
            format!("0 – {maximum:.0} {}", unit.unwrap_or("")).into()
        } else {
            format!("— {}", unit.unwrap_or("")).into()
        },
        bounds.x + 8.0,
        bounds.y,
        (bounds.width - 16.0).max(0.0),
    )];
    if let Some((start, end)) = time_labels {
        let half = (bounds.width * 0.5 - 8.0).max(0.0);
        labels.push(label(
            start.clone(),
            bounds.x + 8.0,
            bounds.y + bounds.height - 16.0,
            half,
        ));
        labels.push(label(
            end.clone(),
            bounds.x + bounds.width * 0.5,
            bounds.y + bounds.height - 16.0,
            half,
        ));
    }
    crate::ComponentGeometry::TimestampSeriesChart {
        grid,
        area,
        segments,
        labels,
        grid_color: paint.grid.as_rgba_array(),
        area_color: paint.area.as_rgba_array(),
        line_color: paint.line.as_rgba_array(),
    }
}

pub(in crate::world) fn stacked_time_series_geometry(
    bounds: LayoutBox,
    title: &str,
    values: &[f64],
    layers: &[crate::TimeSeriesLayer],
    axis_labels: &[Arc<str>],
    active: Option<usize>,
    palette: &SemanticPalette,
    // A layer's role comes from application data, so it can be one of the
    // derived soft roles whose alpha the theme owns.
    opacity: nana_ui_core::OpacityTokens,
    measure: crate::text_width::ChromeTextMeasure<'_>,
) -> crate::ComponentGeometry {
    let plot = crate::TimeSeriesChart::stacked_plot(bounds);
    let clean = |value: f64| {
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    };
    let scale = values
        .iter()
        .copied()
        .chain(layers.iter().flat_map(|layer| layer.values.iter().copied()))
        .map(clean)
        .fold(1.0, f64::max);
    let maximum = values
        .iter()
        .map(|value| clean(*value) / scale)
        .chain((0..values.len()).map(|index| {
            layers
                .iter()
                .map(|layer| clean(layer.values.get(index).copied().unwrap_or(0.0)) / scale)
                .sum::<f64>()
        }))
        .fold(1.0 / scale, f64::max);
    let mut bars = Vec::new();
    let mut line = Vec::new();
    let mut labels = Vec::new();
    let mut grid = Vec::new();
    let slot = plot.width / values.len().max(1) as f32;
    let bar_width = (slot * 0.7).max(1.0);
    for (index, value) in values.iter().enumerate() {
        let x = plot.x + slot * (index as f32 + 0.5);
        let mut base = plot.y + plot.height;
        for layer in layers {
            let height = (clean(layer.values.get(index).copied().unwrap_or(0.0)) / scale / maximum)
                as f32
                * plot.height;
            if height > 0.0 {
                base -= height;
                bars.push((
                    LayoutBox {
                        x: x - bar_width / 2.0,
                        y: base,
                        width: bar_width,
                        height,
                    },
                    palette.get_in(layer.color, opacity).as_rgba_array(),
                ));
            }
        }
        line.push([
            x,
            plot.y + plot.height * (1.0 - (clean(*value) / scale / maximum) as f32),
        ]);
    }
    let text = |value: String, x, y, width| crate::ComponentTextRegion {
        bounds: LayoutBox {
            x,
            y,
            width,
            height: 14.0,
        },
        content: crate::TextValue::from(value),
        color: Some(palette.muted.as_rgba_array()),
        font_size: nana_ui_core::type_scale::HINT,
        font_weight: None,
    };
    for index in 0..=4 {
        let y = plot.y + plot.height * index as f32 / 4.0;
        grid.push(LayoutBox {
            x: plot.x,
            y,
            width: plot.width,
            height: 1.0,
        });
        let value = ((maximum * (4 - index) as f64 / 4.0) * scale).min(f64::MAX);
        let value = if value >= 1_000_000.0 {
            format!("{:.1}M", value / 1_000_000.0)
        } else if value >= 1_000.0 {
            format!("{:.1}k", value / 1_000.0)
        } else {
            format!("{value:.0}")
        };
        labels.push(text(value, bounds.x, y - 7.0, 44.0));
    }
    let stride = ((values.len() as f32 * 48.0 / plot.width.max(1.0)).ceil() as usize).max(1);
    for index in (0..values.len()).step_by(stride) {
        if let Some(label) = axis_labels.get(index) {
            labels.push(text(
                label.to_string(),
                plot.x + slot * (index as f32 + 0.5) - 22.0,
                plot.y + plot.height + 8.0,
                44.0,
            ));
        }
    }
    let legend_items = layers
        .iter()
        .map(|layer| {
            (
                layer.label.as_ref(),
                palette.get_in(layer.color, opacity).as_rgba_array(),
            )
        })
        .chain(std::iter::once((title, palette.text.as_rgba_array())))
        .map(|(label, color)| {
            let width = measure.width(label, nana_ui_core::type_scale::HINT, None);
            (label, color, width)
        })
        .collect::<Vec<_>>();
    let legend_width: f32 = legend_items.iter().map(|(_, _, width)| width + 26.0).sum();
    let mut x = bounds.x + (bounds.width - legend_width).max(0.0) / 2.0;
    let mut legend = Vec::new();
    for (label, color, width) in legend_items {
        let y = bounds.y + bounds.height - 15.0;
        legend.push((
            LayoutBox {
                x,
                y,
                width: 10.0,
                height: 10.0,
            },
            color,
        ));
        labels.push(text(label.to_string(), x + 14.0, y - 2.0, width));
        x += width + 26.0;
    }
    let marker = active
        .and_then(|index| line.get(index))
        .map(|point| LayoutBox {
            x: point[0] - 3.0,
            y: point[1] - 3.0,
            width: 6.0,
            height: 6.0,
        });
    crate::ComponentGeometry::StackedTimeSeriesChart {
        bars,
        legend,
        grid,
        line,
        labels,
        marker,
        grid_color: palette.border_soft.as_rgba_array(),
        line_color: palette.text.as_rgba_array(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_geometry_leaves_missing_interval_unfilled() {
        let geometry = timestamp_series_geometry(
            LayoutBox {
                x: 30.0,
                y: 20.0,
                width: 216.0,
                height: 148.0,
            },
            &[
                (0, Some(1.0)),
                (10, Some(2.0)),
                (20, None),
                (90, Some(2.0)),
                (100, Some(1.0)),
            ],
            Some("people"),
            Some(&(Arc::from("10:00"), Arc::from("11:00"))),
            ThemeMode::Dark,
        );
        let crate::ComponentGeometry::TimestampSeriesChart {
            area,
            segments,
            labels,
            ..
        } = geometry
        else {
            panic!("timestamp geometry");
        };
        assert_eq!(segments.len(), 2);
        let gap_start = segments[0].last().unwrap()[0];
        let gap_end = segments[1][0][0];
        assert!(gap_end > gap_start);
        assert!(!area.is_empty());
        assert!(
            area.iter()
                .all(|rect| rect.x + rect.width <= gap_start + 1.0 || rect.x >= gap_end)
        );
        assert_eq!(labels.len(), 3);
        assert!(
            labels.iter().all(
                |label| label.bounds.y >= 20.0 && label.bounds.y + label.bounds.height <= 168.0
            )
        );
    }
}
