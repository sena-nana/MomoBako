//! Control chrome sized to its own text measures that text through the same
//! `nana-text` engine that shapes the world (Issue #99), not by counting
//! characters: each box here is compared with what [`NanaTextShaper`] says
//! the painted run is.

use nana_ui::{NanaTextShaper, runtime::*};
use nana_ui_core::space;

const VIEWPORT: (f32, f32) = (720.0, 600.0);

fn frame(cx: &mut AppContext, doc: DocumentId, shaper: &mut NanaTextShaper) {
    let viewport = LayoutViewport::new(VIEWPORT.0, VIEWPORT.1);
    let order = cx.world().document_order(doc);
    cx.resolve_styles(&order).unwrap();
    cx.shape_text(&order, shaper).unwrap();
    cx.layout_document(doc, viewport).unwrap();
    if cx.shape_text_for_layout(doc, shaper).unwrap() {
        cx.layout_document(doc, viewport).unwrap();
    }
}

/// What the engine says `region` is wide, painted on `owner`: the owner's
/// family and spacing at the region's own size and weight. Measured under an
/// id no editor owns, so an editor's retained geometry cannot answer it.
fn measured(
    cx: &AppContext,
    shaper: &mut NanaTextShaper,
    owner: StableNodeId,
    region: &ComponentTextRegion,
) -> f32 {
    measured_text(
        cx,
        shaper,
        owner,
        &region.content,
        region.font_size,
        region.font_weight,
    )
}

fn measured_text(
    cx: &AppContext,
    shaper: &mut NanaTextShaper,
    owner: StableNodeId,
    text: &str,
    font_size: f32,
    font_weight: Option<u16>,
) -> f32 {
    let mut style = cx
        .world()
        .computed_style(owner)
        .expect("owner style")
        .clone();
    style.font_size = font_size;
    style.font_weight = font_weight;
    let probe = StableNodeId::new(u64::MAX).unwrap();
    let content = TextContent {
        value: text.to_owned().into(),
    };
    // The line's width, or the end caret when whitespace hangs past it.
    let width = shaper
        .shape(probe, &content, &style, TextShapeConstraints::default())
        .width;
    width
        .max(shaper.horizontal_offset(probe, &content, text.len(), &style))
        .ceil()
}

/// The old character-count estimate, so each test also shows the box moved
/// off it rather than agreeing with the engine by coincidence.
fn estimate(text: &str, font_size: f32) -> f32 {
    text.chars()
        .map(|ch| {
            if ch.is_ascii() {
                font_size * 0.62
            } else {
                font_size
            }
        })
        .sum::<f32>()
        .max(font_size)
}

#[test]
fn key_capture_badge_fits_its_measured_label() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let layer = cx
        .create_component(doc, KeyCaptureLayer::new().recording(true))
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::KeyCaptureLayer { badge, .. }) =
        cx.world().component_geometry(layer.stable_id())
    else {
        panic!("key capture geometry");
    };
    assert_eq!(badge.content.as_ref(), "Recording");
    assert_eq!(badge.font_weight, Some(600));
    let text = measured(&cx, &mut shaper, layer.stable_id(), &badge);
    assert_eq!(
        badge.bounds.width,
        (text + space::MD * 2.0).max(space::XXXL * 4.0)
    );
}

#[test]
fn action_menu_hint_is_as_wide_as_its_measured_text() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let item = cx
        .create_component(
            doc,
            ActionMenuItem::new("复制到剪贴板").hint("Ctrl+Shift+P"),
        )
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::ActionMenuItem {
        label,
        hint: Some(hint),
        ..
    }) = cx.world().component_geometry(item.stable_id())
    else {
        panic!("action menu item geometry with a hint");
    };
    let text = measured(&cx, &mut shaper, item.stable_id(), &hint);
    assert_eq!(hint.bounds.width, text);
    assert_ne!(hint.bounds.width, estimate(&hint.content, hint.font_size));
    assert!(label.bounds.x + label.bounds.width <= hint.bounds.x);
}

#[test]
fn labeled_value_columns_take_their_measured_widths() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let row = cx
        .create_component(doc, LabeledValue::new("Owner", "Engineering 工程"))
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::LabeledValue { label, value, .. }) =
        cx.world().component_geometry(row.stable_id())
    else {
        panic!("labeled value geometry");
    };
    // Both fit, so each column is its text's natural width.
    assert_eq!(
        label.bounds.width,
        measured(&cx, &mut shaper, row.stable_id(), &label)
    );
    assert_eq!(
        value.bounds.width,
        measured(&cx, &mut shaper, row.stable_id(), &value)
    );
    assert_ne!(
        value.bounds.width,
        estimate(&value.content, value.font_size)
    );
}

#[test]
fn command_palette_shortcut_is_as_wide_as_its_measured_text() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let palette = cx
        .create_component(
            doc,
            CommandPalette::new(
                "命令面板",
                [CommandPaletteItem::new("workspace.settings", "打开设置")
                    .shortcut("Ctrl+Alt+Delete")],
            ),
        )
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::CommandPalette { rows, .. }) =
        cx.world().component_geometry(palette.stable_id())
    else {
        panic!("command palette geometry");
    };
    let shortcut = rows[0].shortcut.as_ref().expect("shortcut region");
    let text = measured(&cx, &mut shaper, palette.stable_id(), shortcut);
    assert_eq!(shortcut.bounds.width, text);
    assert_ne!(
        shortcut.bounds.width,
        estimate(&shortcut.content, shortcut.font_size)
    );
}

#[cfg(feature = "charts")]
#[test]
fn stacked_chart_legend_labels_take_their_measured_widths() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let chart = cx
        .create_component(
            doc,
            TimeSeriesChart::new([3.0, 5.0, 4.0]).stacked([
                TimeSeriesLayer::new(
                    "Revenue",
                    [1.0, 2.0, 1.5],
                    nana_ui_core::SemanticColorRole::Accent,
                ),
                TimeSeriesLayer::new(
                    "成本",
                    [2.0, 3.0, 2.5],
                    nana_ui_core::SemanticColorRole::Success,
                ),
            ]),
        )
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::StackedTimeSeriesChart { labels, .. }) =
        cx.world().component_geometry(chart.stable_id())
    else {
        panic!("stacked chart geometry");
    };
    for name in ["Revenue", "成本"] {
        let label = labels
            .iter()
            .find(|label| label.content.as_ref() == name)
            .expect("legend label");
        assert_eq!(
            label.bounds.width,
            measured(&cx, &mut shaper, chart.stable_id(), label)
        );
    }
}

#[cfg(feature = "calendar")]
#[test]
fn calendar_axis_labels_take_their_measured_widths() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let heatmap = cx
        .create_component(
            doc,
            CalendarHeatmap::<()>::new([
                CalendarHeatmapDatum::new("2026-06-01", 2.0),
                CalendarHeatmapDatum::new("2026-06-03", 8.0),
            ]),
        )
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::CalendarHeatmap { labels, .. }) =
        cx.world().component_geometry(heatmap.stable_id())
    else {
        panic!("calendar geometry");
    };
    assert!(!labels.is_empty());
    for label in &labels {
        assert_eq!(
            label.bounds.width,
            measured(&cx, &mut shaper, heatmap.stable_id(), label) + space::XXS
        );
    }
}

fn source_lines(count: usize) -> String {
    (1..=count)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn padding_left(cx: &AppContext, id: StableNodeId) -> f32 {
    match cx.world().node_style(id).unwrap().layout.padding_left {
        Some(nana_ui_core::LengthSpec::Px(px)) => px,
        other => panic!("gutter padding {other:?}"),
    }
}

#[test]
fn line_number_gutter_is_measured_once_an_engine_has_shaped() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let area = cx
        .create_component(doc, TextArea::new(source_lines(120)).line_numbers(true))
        .unwrap();
    // Projected before any engine existed: the estimate.
    let before = padding_left(&cx, area.stable_id());
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::TextInput {
        line_labels_font_size: label_size,
        ..
    }) = cx.world().component_geometry(area.stable_id())
    else {
        panic!("text input geometry");
    };
    let digit = (0..=9)
        .map(|digit| {
            measured_text(
                &cx,
                &mut shaper,
                area.stable_id(),
                &digit.to_string(),
                label_size,
                None,
            )
        })
        .fold(0.0, f32::max);
    let gutter = (18.0 + 3.0 * digit + 4.0).ceil();
    let first = padding_left(&cx, area.stable_id());
    assert_eq!(first, gutter);
    assert_ne!(first, before, "the engine reprojected the gutter");
    // An edit that keeps three digits reprojects to the same gutter: the
    // source does not shift sideways on the first keystroke.
    cx.update_component(area, |view, _| view.state.replace_value(source_lines(130)))
        .unwrap();
    frame(&mut cx, doc, &mut shaper);
    assert_eq!(padding_left(&cx, area.stable_id()), first);
}

#[test]
fn diagnostic_label_is_as_wide_as_its_measured_message() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let area = cx
        .create_component(
            doc,
            TextArea::new("alpha\nbeta").diagnostics(std::sync::Arc::from([
                TextDiagnosticSpan::new(0, 5, TextDiagnosticSeverity::Error)
                    .with_message("unknown identifier 未定义"),
            ])),
        )
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::TextInput {
        diagnostic_labels, ..
    }) = cx.world().component_geometry(area.stable_id())
    else {
        panic!("text input geometry");
    };
    let label = diagnostic_labels.first().expect("diagnostic label");
    let text = measured(&cx, &mut shaper, area.stable_id(), label);
    assert_eq!(label.bounds.width, text);
}

#[test]
fn signature_help_splits_at_measured_widths() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let area = cx
        .create_component(
            doc,
            TextArea::new("mix(").signature(Some(TextSignatureHelp::new(
                "mix",
                "Blends two values.",
                vec![
                    ("from".to_owned(), "start".to_owned()),
                    ("to".to_owned(), "end".to_owned()),
                ],
                1,
            ))),
        )
        .unwrap();
    let mut shaper = NanaTextShaper::default();
    frame(&mut cx, doc, &mut shaper);
    let Some(ComponentGeometry::TextInput {
        signature_popup: Some(popup),
        ..
    }) = cx.world().component_geometry(area.stable_id())
    else {
        panic!("text input geometry with a signature popup");
    };
    let id = area.stable_id();
    assert_eq!(popup.prefix.content.as_ref(), "mix(from, ");
    assert_eq!(
        popup.prefix.bounds.width,
        measured(&cx, &mut shaper, id, &popup.prefix)
    );
    // The prefix's trailing space is reserved, not overlapped by `to`.
    let without_space = measured_text(
        &cx,
        &mut shaper,
        id,
        "mix(from,",
        popup.prefix.font_size,
        popup.prefix.font_weight,
    );
    assert!(
        popup.prefix.bounds.width > without_space,
        "prefix {} vs {without_space} without its trailing space",
        popup.prefix.bounds.width
    );
    let active = popup.active.as_ref().expect("active parameter");
    assert_eq!(active.font_weight, Some(600));
    assert_eq!(active.bounds.width, measured(&cx, &mut shaper, id, active));
    assert_eq!(
        active.bounds.x,
        popup.prefix.bounds.x + popup.prefix.bounds.width
    );
}
