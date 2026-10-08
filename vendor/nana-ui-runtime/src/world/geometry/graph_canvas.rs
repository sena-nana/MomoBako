//! graph-canvas geometry from committed node data.

use super::*;

pub(in crate::world) fn graph_canvas_geometry(
    bounds: LayoutBox,
    nodes: &[crate::GraphNodePaint],
    ports: &[crate::GraphPortPaint],
    edges: &[crate::GraphEdgePaint],
    connecting: Option<&crate::GraphEdgePaint>,
    grid_spacing: f32,
    viewport_offset_x: f32,
    viewport_offset_y: f32,
    viewport_zoom: f32,
    palette: &SemanticPalette,
) -> crate::ComponentGeometry {
    let mut painted_edges = Vec::new();
    let mut edge_labels = Vec::new();
    for edge in edges.iter().chain(connecting) {
        let color = graph_edge_stroke_color(palette, edge);
        painted_edges.push((sample_curve(bounds, edge.curve), color));
        if !edge.connecting
            && viewport_zoom >= 0.7
            && let Some(label) = edge.label.as_ref()
        {
            let center = cubic_point(edge.curve, 0.5);
            if let Some(label_bounds) = intersect_layout_boxes(
                bounds,
                LayoutBox {
                    x: bounds.x + center.x - 40.0,
                    y: bounds.y + center.y - 16.0,
                    width: 80.0,
                    height: 12.0,
                },
            ) {
                edge_labels.push(crate::ComponentTextRegion {
                    bounds: label_bounds,
                    content: Arc::clone(label).into(),
                    color: Some(palette.muted.as_rgba_array()),
                    font_size: nana_ui_core::type_scale::HINT,
                    font_weight: None,
                });
            }
        }
    }
    let mut separators = Vec::new();
    let nodes = nodes
        .iter()
        .filter_map(|node| {
            let raw = LayoutBox {
                x: bounds.x + node.x,
                y: bounds.y + node.y,
                width: node.width.max(0.0),
                height: node.height.max(0.0),
            };
            let node_bounds = intersect_layout_boxes(bounds, raw)?;
            let title_height = node.title_height.clamp(18.0, node_bounds.height.max(18.0));
            if node_bounds.width >= 32.0
                && node_bounds.height >= title_height
                && let Some(separator) = intersect_layout_boxes(
                    bounds,
                    LayoutBox {
                        x: node_bounds.x,
                        y: node_bounds.y + title_height,
                        width: node_bounds.width,
                        height: 1.0,
                    },
                )
            {
                separators.push(separator);
            }
            let label = crate::ComponentTextRegion {
                bounds: intersect_layout_boxes(
                    bounds,
                    LayoutBox {
                        x: raw.x + 10.0,
                        y: raw.y,
                        width: (raw.width - 20.0).max(0.0),
                        height: title_height.min(raw.height),
                    },
                )
                .unwrap_or(LayoutBox {
                    x: node_bounds.x,
                    y: node_bounds.y,
                    width: 0.0,
                    height: 0.0,
                }),
                content: Arc::clone(&node.label).into(),
                color: Some(palette.text.as_rgba_array()),
                font_size: (nana_ui_core::type_scale::META * viewport_zoom).clamp(
                    crate::graph_canvas::NODE_TITLE_SIZE_MIN,
                    nana_ui_core::type_scale::BODY,
                ),
                font_weight: Some(nana_ui_core::type_scale::MEDIUM),
            };
            let fill = if node.selected {
                palette.selected.as_rgba_array()
            } else if node.hovered {
                palette.hover.as_rgba_array()
            } else {
                palette.surface.as_rgba_array()
            };
            let border = if node.selected {
                Some(palette.border_strong.as_rgba_array())
            } else {
                Some(palette.border.as_rgba_array())
            };
            Some((node_bounds, label, fill, border))
        })
        .collect();
    let mut port_labels = Vec::new();
    let ports = ports
        .iter()
        .filter_map(|port| {
            let radius = port.radius.max(0.0);
            let disc = intersect_layout_boxes(
                bounds,
                LayoutBox {
                    x: bounds.x + port.x - radius,
                    y: bounds.y + port.y - radius,
                    width: radius * 2.0,
                    height: radius * 2.0,
                },
            )?;
            let kind = match port.kind {
                GraphPortKind::Input => palette.muted.as_rgba_array(),
                GraphPortKind::Output => palette.accent.as_rgba_array(),
                GraphPortKind::Bidirectional => palette.warning.as_rgba_array(),
            };
            if viewport_zoom >= 0.72 && !port.label.is_empty() {
                let (mut label, alignment) =
                    port_label_region(bounds, port, palette.muted.as_rgba_array());
                if let Some(clipped) = intersect_layout_boxes(bounds, label.bounds) {
                    label.bounds = clipped;
                    port_labels.push((label, alignment));
                }
            }
            Some((
                disc,
                palette.background.as_rgba_array(),
                kind,
                if port.selected { 2.4 } else { 1.6 },
            ))
        })
        .collect();
    crate::ComponentGeometry::GraphCanvas {
        nodes,
        separators,
        ports,
        port_labels,
        edges: painted_edges,
        edge_labels,
        grid: graph_grid_lines(
            bounds,
            grid_spacing,
            viewport_offset_x,
            viewport_offset_y,
            viewport_zoom,
        ),
        background: palette.background.as_rgba_array(),
        grid_color: {
            let mut color = palette.border_soft.as_rgba_array();
            color[3] *= 0.72;
            color
        },
        separator_color: palette.border_soft.as_rgba_array(),
    }
}

pub(in crate::world) fn graph_minimap_geometry(
    box_bounds: LayoutBox,
    model_bounds: GraphRect,
    nodes: &[GraphRect],
    indicator: Option<&GraphRect>,
    node_fill: Option<SemanticColorRole>,
    palette: &SemanticPalette,
    // The fill role is application data; the theme owns the soft alphas.
    opacity: nana_ui_core::OpacityTokens,
) -> crate::ComponentGeometry {
    let fill = palette
        .get_in(node_fill.unwrap_or(SemanticColorRole::Muted), opacity)
        .as_rgba_array();
    let mut indicator_fill = palette.accent.as_rgba_array();
    indicator_fill[3] *= 0.16;
    let indicator_border = palette.accent.as_rgba_array();
    let projection = crate::graph_minimap::GraphMinimapProjection::new(
        GraphSize::new(box_bounds.width, box_bounds.height),
        model_bounds,
    );
    let project = |rect: &GraphRect| {
        projection
            .map(|projection| projection.local_rect(*rect))
            .map(|rect| LayoutBox {
                x: box_bounds.x + rect.origin.x,
                y: box_bounds.y + rect.origin.y,
                width: rect.size.width,
                height: rect.size.height,
            })
    };
    crate::ComponentGeometry::GraphMinimap {
        nodes: nodes.iter().filter_map(&project).collect(),
        node_fill: fill,
        indicator: indicator
            .and_then(&project)
            .and_then(|mapped| intersect_layout_boxes(box_bounds, mapped)),
        indicator_fill,
        indicator_border,
    }
}

pub(in crate::world) fn graph_edge_stroke_color(
    palette: &SemanticPalette,
    edge: &crate::GraphEdgePaint,
) -> [f32; 4] {
    if edge.connecting {
        let mut accent = palette.accent.as_rgba_array();
        accent[3] *= 0.8;
        accent
    } else if edge.selected {
        palette.text.as_rgba_array()
    } else if edge.hovered {
        palette.muted.as_rgba_array()
    } else {
        palette.border_strong.as_rgba_array()
    }
}

pub(in crate::world) const CURVE_FLATNESS: f32 = 0.75;

pub(in crate::world) fn sample_curve(bounds: LayoutBox, curve: [GraphPoint; 4]) -> Vec<[f32; 2]> {
    let origin = [bounds.x, bounds.y];
    let mut points = vec![[origin[0] + curve[0].x, origin[1] + curve[0].y]];
    flatten_cubic(&mut points, curve, origin, 0);
    points
}

/// Screen-space flatness (`edge_curve` is already in view space).
pub(in crate::world) fn flatten_cubic(
    points: &mut Vec<[f32; 2]>,
    [p0, p1, p2, p3]: [GraphPoint; 4],
    origin: [f32; 2],
    depth: u32,
) {
    let deviation = line_offset(p1, p0, p3).max(line_offset(p2, p0, p3));
    if depth >= 16 || deviation <= CURVE_FLATNESS {
        points.push([origin[0] + p3.x, origin[1] + p3.y]);
        return;
    }
    let mid = |a: GraphPoint, b: GraphPoint| GraphPoint::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
    let p01 = mid(p0, p1);
    let p12 = mid(p1, p2);
    let p23 = mid(p2, p3);
    let p012 = mid(p01, p12);
    let p123 = mid(p12, p23);
    let split = mid(p012, p123);
    flatten_cubic(points, [p0, p01, p012, split], origin, depth + 1);
    flatten_cubic(points, [split, p123, p23, p3], origin, depth + 1);
}

pub(in crate::world) fn line_offset(point: GraphPoint, start: GraphPoint, end: GraphPoint) -> f32 {
    let abx = end.x - start.x;
    let aby = end.y - start.y;
    let length_sq = abx * abx + aby * aby;
    if length_sq <= f32::EPSILON {
        return point.distance_squared(start).sqrt();
    }
    ((point.x - start.x) * aby - (point.y - start.y) * abx).abs() / length_sq.sqrt()
}

pub(in crate::world) fn graph_grid_lines(
    bounds: LayoutBox,
    base_spacing: f32,
    offset_x: f32,
    offset_y: f32,
    zoom: f32,
) -> Vec<LayoutBox> {
    if !base_spacing.is_finite() || base_spacing <= 0.0 {
        return Vec::new();
    }
    let mut spacing = base_spacing
        * if zoom.is_finite() && zoom > 0.0 {
            zoom
        } else {
            1.0
        };
    while spacing < 16.0 {
        spacing *= 2.0;
    }
    while spacing > 96.0 {
        spacing *= 0.5;
    }
    if !spacing.is_finite() || spacing < 1.0 {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut x = offset_x.rem_euclid(spacing);
    while x <= bounds.width {
        lines.push(LayoutBox {
            x: bounds.x + x,
            y: bounds.y,
            width: 1.0,
            height: bounds.height,
        });
        x += spacing;
    }
    let mut y = offset_y.rem_euclid(spacing);
    while y <= bounds.height {
        lines.push(LayoutBox {
            x: bounds.x,
            y: bounds.y + y,
            width: bounds.width,
            height: 1.0,
        });
        y += spacing;
    }
    lines
}

pub(in crate::world) fn port_label_region(
    bounds: LayoutBox,
    port: &crate::GraphPortPaint,
    color: [f32; 4],
) -> (crate::ComponentTextRegion, crate::TextHorizontalAlignment) {
    let width = crate::graph_canvas::PORT_LABEL_WIDTH;
    let (x, y, height, align) = match port.side {
        GraphPortSide::Top => (
            port.x - nana_ui_core::space::PAGE_TIGHT * 2.0,
            port.y + nana_ui_core::space::MD,
            nana_ui_core::type_scale::META,
            crate::TextHorizontalAlignment::Center,
        ),
        GraphPortSide::Right => (
            port.x - (width + nana_ui_core::space::MD),
            port.y - nana_ui_core::space::SM,
            nana_ui_core::type_scale::SECTION,
            crate::TextHorizontalAlignment::End,
        ),
        GraphPortSide::Bottom => (
            port.x - nana_ui_core::space::PAGE_TIGHT * 2.0,
            port.y - nana_ui_core::space::PAGE_TIGHT,
            nana_ui_core::type_scale::META,
            crate::TextHorizontalAlignment::Center,
        ),
        GraphPortSide::Left => (
            port.x + nana_ui_core::space::MD,
            port.y - nana_ui_core::space::SM,
            nana_ui_core::type_scale::SECTION,
            crate::TextHorizontalAlignment::Start,
        ),
    };
    (
        crate::ComponentTextRegion {
            bounds: LayoutBox {
                x: bounds.x + x,
                y: bounds.y + y,
                width,
                height,
            },
            content: Arc::clone(&port.label).into(),
            color: Some(color),
            font_size: crate::graph_canvas::PORT_LABEL_SIZE,
            font_weight: None,
        },
        align,
    )
}
