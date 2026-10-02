//! A `SelectableRichText` box is as tall as the text it paints.
//!
//! The scene paints the region wrapped, so a height counted from `\n` alone
//! clipped every paragraph that wrapped. The box now takes the height the
//! Runtime measured the node's plain text at, and pointer hits read the same
//! layout: a click on the second painted line selects from that line.
#![cfg(feature = "rich-text")]

use nana_ui::{HeadlessInput, NanaTextShaper, runtime::*};
use nana_ui_core::LengthSpec;
use nana_ui_platform::{
    InputModifiers, InputPayload, PointerId, PointerInput, PointerPhase, PointerType,
};
use std::sync::Arc;

const TEXT: &str = "Selectable rich text that is long enough to wrap onto several lines";

fn pointer(phase: PointerPhase, x: f32, y: f32) -> InputPayload {
    InputPayload::Pointer(PointerInput {
        phase,
        pointer_id: PointerId(1),
        pointer_type: PointerType::Mouse,
        x,
        y,
        screen_x: x,
        screen_y: y,
        button: 0,
        buttons: u16::from(phase != PointerPhase::Up),
        pressure: 1.0,
        tangential_pressure: 0.0,
        tilt_x: 0,
        tilt_y: 0,
        twist: 0,
        is_primary: true,
        activation_click: false,
        modifiers: InputModifiers::default(),
    })
}

fn settle(cx: &mut AppContext, doc: DocumentId, ids: &[StableNodeId]) {
    let viewport = LayoutViewport::new(600.0, 400.0);
    cx.resolve_styles(ids).unwrap();
    cx.shape_text(ids, &mut NanaTextShaper::default()).unwrap();
    cx.layout_document(doc, viewport).unwrap();
    if cx
        .shape_text_for_layout(doc, &mut NanaTextShaper::default())
        .unwrap()
    {
        cx.layout_document(doc, viewport).unwrap();
    }
    cx.rebuild_hit_test(doc);
}

fn narrow_column(width: f32) -> Stack {
    Stack::column(0.0).with_layout(|layout| layout.width = Some(LengthSpec::Px(width)))
}

#[test]
fn wrapped_rich_text_box_is_as_tall_as_its_measured_lines_and_hits_them() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, narrow_column(160.0)).unwrap();
    let text = cx
        .create_component(
            doc,
            SelectableRichText::new([RichSpan::plain("Selectable rich text "), {
                let mut bold = RichSpan::plain("that is long enough to wrap onto several lines");
                bold.strong = true;
                bold
            }]),
        )
        .unwrap();
    cx.append_child(root, text).unwrap();
    let id = text.stable_id();
    settle(&mut cx, doc, &[root.stable_id(), id]);

    assert_eq!(cx.world().text(id), Some(TEXT));
    let (_, layout) = cx
        .world()
        .text_layout(id)
        .expect("the plain text is measured by nana-text");
    let layout = Arc::clone(layout);
    assert!(
        layout.lines.len() >= 2,
        "the text wraps in a 160px column: {} line(s)",
        layout.lines.len()
    );
    let (_, measured_height) = layout.physical_size();
    let bounds = cx.world().layout_box(id).unwrap();
    assert_eq!(bounds.width, 160.0);
    assert!(
        (bounds.height - measured_height).abs() < 0.5,
        "box height {} is the measured {} of {} wrapped lines, not one line per `\\n`",
        bounds.height,
        measured_height,
        layout.lines.len()
    );

    // Press at the start of the second painted line and drag past the right
    // edge of the box on the last one: the selection runs from where the
    // second line starts in the source to the end of the text. The last line
    // is the shortest, so the end of a longer line above is nearer in a
    // straight line — the caret has to pick the line first.
    let second = &layout.lines[1];
    let last = layout.lines.last().unwrap();
    assert!(last.bounds.width < second.bounds.width);
    let press_y = bounds.y + second.bounds.y + second.bounds.height / 2.0;
    let end_x = bounds.x + bounds.width - 0.5;
    let end_y = bounds.y + last.bounds.y + last.bounds.height / 2.0;
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    for (phase, x, y) in [
        (PointerPhase::Down, bounds.x + 0.5, press_y),
        (PointerPhase::Move, end_x, end_y),
        (PointerPhase::Up, end_x, end_y),
    ] {
        adapter.route(&mut cx, pointer(phase, x, y)).unwrap();
    }
    let selected = cx
        .read(text, |view| view.selected_text())
        .unwrap()
        .expect("a drag across painted lines selects");
    assert_eq!(
        selected,
        TEXT[second.source.start..],
        "the selection starts on the second line as painted and ends with the text"
    );

    // The highlight covers the selected glyphs line by line, not the box.
    settle(&mut cx, doc, &[root.stable_id(), id]);
    let Some(ComponentGeometry::SelectableRichText { selection, .. }) =
        cx.world().component_geometry(id)
    else {
        panic!("selectable rich text geometry")
    };
    assert_eq!(
        selection.len(),
        layout.lines.len() - 1,
        "one highlight per selected line: {selection:?}"
    );
    for (rect, line) in selection.iter().zip(&layout.lines[1..]) {
        assert!((rect.y - (bounds.y + line.bounds.y)).abs() < 0.01);
        assert!((rect.height - line.bounds.height).abs() < 0.01);
        assert!(rect.x >= bounds.x - 0.01);
        assert!(rect.x + rect.width <= bounds.x + bounds.width + 0.01);
    }
    let last_rect = selection.last().unwrap();
    assert!(
        last_rect.width < bounds.width - 1.0,
        "the short last line is highlighted to its end, not across the box"
    );
}

fn single(width: f32, text: &str) -> (AppContext, StableNodeId) {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, narrow_column(width)).unwrap();
    let node = cx
        .create_component(doc, SelectableRichText::new([RichSpan::plain(text)]))
        .unwrap();
    cx.append_child(root, node).unwrap();
    let id = node.stable_id();
    settle(&mut cx, doc, &[root.stable_id(), id]);
    (cx, id)
}

#[test]
fn text_that_fits_on_one_line_gets_exactly_its_measured_line() {
    let (cx, id) = single(400.0, "Short");
    let (_, layout) = cx.world().text_layout(id).unwrap();
    assert_eq!(layout.lines.len(), 1);
    let (_, measured_height) = layout.physical_size();
    let height = cx.world().layout_box(id).unwrap().height;
    assert!(
        (height - measured_height).abs() < 0.01,
        "a one-line box is its line ({measured_height}), not a floor above it ({height})"
    );
}

#[test]
fn empty_text_is_held_open_at_one_line() {
    let (cx, id) = single(400.0, "");
    let height = cx.world().layout_box(id).unwrap().height;
    assert_eq!(height, nana_ui_core::type_scale::LINE);
}

/// Markdown geometry is measured by the engine that draws it (#99): the
/// painter shapes a span's line as one run, so summing per-character advances
/// (or estimating unseen ones) put the next span where the drawn text is not.
#[test]
fn a_markdown_span_is_as_wide_as_the_run_the_painter_shapes() {
    use nana_ui_runtime::{MarkdownDrawingCommand, TextShapeConstraints, TextShaper};
    // Kerning pairs, a ligature and a joining script. No trailing space: a
    // measured width leaves hanging whitespace out, a span's geometry does not.
    let first = "AVATAR office WAVY";
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let markdown = cx
        .create_component(doc, NativeMarkdown::parse(&format!("{first}**bold** سلام")))
        .unwrap();
    let id = markdown.stable_id();
    settle(&mut cx, doc, &[id]);

    let world = cx.world();
    let Some(ComponentGeometry::NativeMarkdown { drawing, .. }) = world.component_geometry(id)
    else {
        panic!("a markdown node has markdown geometry");
    };
    let runs = drawing
        .commands
        .iter()
        .filter_map(|command| match command {
            MarkdownDrawingCommand::Text {
                bounds,
                text,
                size,
                weight,
                ..
            } => Some((*bounds, text.to_string(), *size, *weight)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(runs.len() >= 2, "one run per span: {runs:?}");
    let (bounds, text, size, weight) = &runs[0];
    assert_eq!(text, first);

    let mut style = world.computed_style(id).unwrap().clone();
    style.font_size = *size;
    style.font_weight = Some(*weight);
    let shaped = NanaTextShaper::default().shape(
        id,
        &TextContent {
            value: first.into(),
        },
        &style,
        TextShapeConstraints::default(),
    );
    assert!(
        (bounds.width - shaped.width).abs() < 0.01,
        "the span is {} wide in the geometry and {} wide when the painter shapes it",
        bounds.width,
        shaped.width
    );
    assert!(
        (runs[1].0.x - (bounds.x + bounds.width)).abs() < 0.01,
        "the next span starts where the drawn run ends: {runs:?}"
    );
}
