//! The painter draws the paragraph Runtime *measured*, not one it lays out
//! again (#99).
//!
//! The handle only carries that promise when the box a node was measured into
//! is the box it paints into, and those two numbers are computed by different
//! code in different crates: `UiWorld::text_shape_constraints_for` subtracts
//! padding, border and a leading visual from the layout box, while the scene's
//! text primitive subtracts the same plus a trailing visual. Nothing makes
//! them agree by construction, so this asserts they do on the plain path —
//! otherwise `TextPipeline::prepare` would silently fall back for every node
//! and the retained layout would be dead weight.

use nana_text::TextWorkCounters;
use nana_ui::{NanaTextShaper, runtime::*};
use nana_ui_scene::{ScenePrimitiveKind, UiScene};

fn text_primitive(
    scene: &UiScene,
    node: StableNodeId,
) -> (nana_ui_scene::SceneRect, Option<RetainedTextLayout>) {
    let primitive = scene
        .primitives()
        .find(|primitive| {
            primitive.node == node && matches!(primitive.kind, ScenePrimitiveKind::Text { .. })
        })
        .expect("the node paints text");
    let ScenePrimitiveKind::Text { ref layout, .. } = primitive.kind else {
        unreachable!("filtered above")
    };
    (primitive.bounds, layout.clone())
}

fn settle(cx: &mut AppContext, doc: DocumentId, ids: &[StableNodeId]) {
    cx.resolve_styles(ids).unwrap();
    cx.shape_text(ids, &mut NanaTextShaper::default()).unwrap();
    cx.layout_document(doc, LayoutViewport::new(600.0, 400.0))
        .unwrap();
    // The constraint-aware pass: a node measured before layout knew its box is
    // re-resolved against the box it got, and that is the layout the scene
    // carries.
    if cx
        .shape_text_for_layout(doc, &mut NanaTextShaper::default())
        .unwrap()
    {
        cx.layout_document(doc, LayoutViewport::new(600.0, 400.0))
            .unwrap();
    }
}

#[test]
fn a_plain_text_node_is_measured_into_the_box_its_primitive_paints() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, Stack::column(8.0)).unwrap();
    let label = cx
        .create_component(doc, Text::new("A label wide enough to wrap somewhere"))
        .unwrap();
    cx.append_child(root, label).unwrap();
    let (root, label) = (root.stable_id(), label.stable_id());
    settle(&mut cx, doc, &[root, label]);

    let mut scene = UiScene::new();
    scene.apply_delta(cx.world().extract_document(doc), []);
    let (bounds, retained) = text_primitive(&scene, label);

    let retained = retained.expect("a plain text node retains the layout it was measured with");
    assert_eq!(
        retained.layout.constraints.max_width_px,
        Some(bounds.width),
        "the alignment box and the paint box are the same number, or the \
         painter falls back to laying the paragraph out again"
    );
}

/// A box too short for its text is an overflow, not a shorter paragraph.
///
/// `nana-text` *drops* the lines that do not fit `max_height_px`, so handing
/// it a declared height whenever one exists would delete text the box merely
/// clips — and since the painter now draws this very layout, deleted lines
/// would not be drawn at all. The height only goes to the engine when
/// truncation was asked for.
#[test]
fn a_short_box_clips_its_wrapped_text_instead_of_losing_lines() {
    fn lines(height: Option<f32>, ellipsis: bool) -> usize {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let root = cx.create_component(doc, Stack::column(0.0)).unwrap();
        let mut label = Text::new("A deliberately long label that has to wrap onto several lines");
        {
            let style = std::sync::Arc::make_mut(&mut label.style.layout);
            style.width = Some(nana_ui_core::LengthSpec::Px(180.0));
            style.height = height.map(nana_ui_core::LengthSpec::Px);
            style.text_overflow_ellipsis = ellipsis;
        }
        let label = cx.create_component(doc, label).unwrap();
        cx.append_child(root, label).unwrap();
        let (root, label) = (root.stable_id(), label.stable_id());
        settle(&mut cx, doc, &[root, label]);

        let mut scene = UiScene::new();
        scene.apply_delta(cx.world().extract_document(doc), []);
        let (_, retained) = text_primitive(&scene, label);
        retained
            .expect("a plain text node retains its layout")
            .layout
            .lines
            .len()
    }

    let natural = lines(None, false);
    assert!(natural >= 2, "the fixture has to wrap: {natural} lines");
    assert_eq!(
        lines(Some(24.0), false),
        natural,
        "a box one line tall does not delete the lines it cannot show"
    );
    assert!(
        lines(Some(24.0), true) < natural,
        "asking for an ellipsis is what makes the height a truncation budget"
    );
}

/// #59: a `vertical-rl` text node is laid out in columns, measured as a
/// column (width across, height down), and the painter draws the very layout
/// the Runtime measured — its line budget is the box's *height*, so the reuse
/// check has to compare that and not the width.
#[test]
fn a_vertical_text_node_is_measured_as_columns_and_painted_from_that_layout() {
    fn vertical_label(
        height: Option<f32>,
    ) -> (
        nana_ui_scene::SceneRect,
        RetainedTextLayout,
        TextWorkCounters,
    ) {
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let root = cx.create_component(doc, Stack::row(0.0)).unwrap();
        let mut label = Text::new("縦書きの段落");
        {
            let style = std::sync::Arc::make_mut(&mut label.style.layout);
            style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
            style.height = height.map(nana_ui_core::LengthSpec::Px);
        }
        let label = cx.create_component(doc, label).unwrap();
        cx.append_child(root, label).unwrap();
        let (root, label) = (root.stable_id(), label.stable_id());
        settle(&mut cx, doc, &[root, label]);
        let counters = cx.world().last_text_work_counters();

        let mut scene = UiScene::new();
        scene.apply_delta(cx.world().extract_document(doc), []);
        let (bounds, retained) = text_primitive(&scene, label);
        (
            bounds,
            retained.expect("a plain text node retains its layout"),
            counters,
        )
    }

    let (bounds, retained, counters) = vertical_label(None);
    let layout = &retained.layout;
    assert!(
        layout.is_vertical(),
        "the node asked for columns and got them"
    );
    assert!(!layout.unsupported_writing_mode);
    assert_eq!(counters.vertical_writing_fallbacks, 0, "{counters:?}");
    assert_eq!(
        layout.lines.len(),
        1,
        "an unconstrained column holds it all"
    );
    let (width, height) = layout.physical_size();
    assert!(
        height > width * 3.0,
        "six ideographs stand in one tall column: {width} x {height}"
    );
    assert!(
        (bounds.width - width).abs() < 0.5 && (bounds.height - height).abs() < 0.5,
        "the box is the column the text measured: {bounds:?} vs {width} x {height}"
    );
    assert_eq!(
        layout.constraints.max_height_px,
        Some(bounds.height),
        "the line budget and the paint box are the same number, or the \
         painter falls back to laying the paragraph out again"
    );

    // A box shorter than the column wraps it into more columns, stacked
    // right to left, and grows wider rather than losing text.
    let (short_bounds, short, _) = vertical_label(Some(height / 2.0 + 1.0));
    assert_eq!(
        short.layout.lines.len(),
        2,
        "the text wraps into two columns"
    );
    assert!(
        short_bounds.width > bounds.width * 1.5,
        "two columns are wider than one: {short_bounds:?}"
    );
}

/// #59: `text-orientation` inherits like `writing-mode` and reaches the
/// engine as the same enum a Rust caller sets: under an `upright` column
/// Latin stands up too, and the node lays out in the used direction `ltr`.
#[test]
fn an_inherited_upright_orientation_stands_every_character_up() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = Stack::row(0.0).with_layout(|style| {
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.dir = Some(nana_ui_core::DirSpec::Rtl);
        style.text_orientation = Some(nana_ui_core::TextOrientationSpec::Upright);
    });
    let root = cx.create_component(doc, root).unwrap();
    let label = cx.create_component(doc, Text::new("縦ab")).unwrap();
    cx.append_child(root, label).unwrap();
    let (root, label) = (root.stable_id(), label.stable_id());
    settle(&mut cx, doc, &[root, label]);

    let mut scene = UiScene::new();
    scene.apply_delta(cx.world().extract_document(doc), []);
    let (_, retained) = text_primitive(&scene, label);
    let layout = retained
        .expect("a plain text node retains its layout")
        .layout;
    assert!(layout.is_vertical());
    assert_eq!(
        layout.constraints.text_orientation,
        nana_ui_core::TextOrientationSpec::Upright
    );
    assert_eq!(
        layout.constraints.base_direction,
        nana_ui_core::DirSpec::Ltr,
        "upright makes the used direction ltr"
    );
    assert!(
        layout
            .runs
            .iter()
            .all(|run| run.orientation == nana_text::RunOrientation::Upright),
        "{:?}",
        layout
            .runs
            .iter()
            .map(|run| run.orientation)
            .collect::<Vec<_>>()
    );
}

/// #59: selecting vertical text hits and highlights the column the glyphs
/// are drawn in.
///
/// Static selection is otherwise answered by editor geometry, which stays
/// horizontal; asked of a column it would select along an invisible
/// horizontal line and paint the highlight across the page.
#[test]
fn selecting_vertical_text_follows_the_column() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, Stack::row(0.0)).unwrap();
    let text = "一二三四五六七八";
    let mut label = Text::new(text);
    {
        let style = std::sync::Arc::make_mut(&mut label.style.layout);
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.user_select = Some(nana_ui_core::UserSelectSpec::Text);
        style.font_size = Some(16.0);
        style.height = Some(nana_ui_core::LengthSpec::Px(64.0));
    }
    let label = cx.create_component(doc, label).unwrap();
    cx.append_child(root, label).unwrap();
    let (root, label) = (root.stable_id(), label.stable_id());
    settle(&mut cx, doc, &[root, label]);
    cx.rebuild_hit_test(doc);

    let bounds = cx.world().layout_box(label).expect("laid out");
    let (_, layout) = cx.world().text_layout(label).expect("retained");
    assert!(layout.is_vertical());
    assert_eq!(layout.lines.len(), 2, "four ideographs to a 64px column");

    // Down the right-hand column, which `vertical-rl` fills first: from the
    // top of 一 to the middle of 三.
    let column_x = bounds.x + bounds.width - 5.0;
    let mut shaper = NanaTextShaper::default();
    assert!(
        cx.document_text_pointer_press(doc, 1, column_x, bounds.y + 1.0, &mut shaper)
            .unwrap()
    );
    cx.document_text_pointer_drag(doc, 1, column_x, bounds.y + 40.0, &mut shaper)
        .unwrap();
    cx.document_text_pointer_release(1);
    assert_eq!(
        cx.document_selected_text(doc).as_deref(),
        Some("一二"),
        "the drag ran down the first column"
    );

    let selection = cx
        .world()
        .document_text_selection(doc)
        .expect("a selection");
    assert_eq!(selection.lines.len(), 1, "{:?}", selection.lines);
    let highlight = selection.lines[0];
    assert!(
        highlight.height > highlight.width,
        "a highlight down a column is tall, not wide: {highlight:?}"
    );
    assert!(
        (highlight.x + highlight.width - bounds.width).abs() < 0.5,
        "and it is the rightmost column: {highlight:?} in {bounds:?}"
    );
    assert!(highlight.y.abs() < 0.5 && (highlight.height - 32.0).abs() < 0.5);
}

/// A clamped paragraph reports that it dropped lines, so an "expand"
/// affordance appears only when there is more to show.
#[test]
fn a_clamped_paragraph_reports_the_lines_it_dropped() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, Stack::column(8.0)).unwrap();
    let clamped = |value: &str| {
        let mut text = Text::new(value);
        let style = std::sync::Arc::make_mut(&mut text.style.layout);
        style.width = Some(nana_ui_core::LengthSpec::Px(80.0));
        style.line_clamp = Some(2);
        text
    };
    let long = cx
        .create_component(
            doc,
            clamped("one two three four five six seven eight nine ten eleven twelve"),
        )
        .unwrap();
    let short = cx.create_component(doc, clamped("one")).unwrap();
    cx.append_child(root, long).unwrap();
    cx.append_child(root, short).unwrap();
    settle(
        &mut cx,
        doc,
        &[root.stable_id(), long.stable_id(), short.stable_id()],
    );
    assert_eq!(cx.text_truncated(long), Some(true));
    assert_eq!(cx.text_truncated(short), Some(false));
}

/// A listening clamped text says whether it cut lines once shaping knows,
/// and again only when that changes.
#[test]
fn a_clamped_text_announces_when_its_clamp_changes() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx.create_component(doc, Stack::column(8.0)).unwrap();
    let mut text = Text::new("one two three four five six seven eight nine ten eleven twelve");
    let style = std::sync::Arc::make_mut(&mut text.style.layout);
    style.width = Some(nana_ui_core::LengthSpec::Px(80.0));
    style.line_clamp = Some(2);
    let long = cx.create_component(doc, text).unwrap();
    cx.append_child(root, long).unwrap();
    let heard = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&heard);
    cx.on(long, move |_, event: &TextClamped, _| {
        sink.lock().unwrap().push(event.clamped)
    })
    .unwrap();
    settle(&mut cx, doc, &[root.stable_id(), long.stable_id()]);
    assert_eq!(*heard.lock().unwrap(), [true]);

    settle(&mut cx, doc, &[root.stable_id(), long.stable_id()]);
    assert_eq!(*heard.lock().unwrap(), [true], "no news, no event");

    cx.update_component(long, |text, _| text.value = "one".into())
        .unwrap();
    settle(&mut cx, doc, &[root.stable_id(), long.stable_id()]);
    assert_eq!(*heard.lock().unwrap(), [true, false]);
}
