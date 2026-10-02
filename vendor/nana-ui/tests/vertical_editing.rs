//! #59: editing vertical text. A `vertical-rl` TextArea lays its value out in
//! columns, draws its caret across the column, walks the column with Up/Down
//! and crosses columns with Left/Right, and a click lands on the glyph under
//! it — end to end, through the real engine.

use nana_ui::{HeadlessInput, NanaTextShaper, runtime::*};
use nana_ui_platform::{
    InputModifiers, InputPayload, KeyInput, KeyState, PointerId, WheelInput, WheelUnit,
};
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

/// A physical key press, as a platform reports it.
fn press(
    cx: &mut AppContext,
    doc: DocumentId,
    key: &str,
    modifiers: InputModifiers,
    shaper: &mut NanaTextShaper,
) {
    let event = InputPayload::Key(KeyInput {
        physical: nana_ui_platform::PhysicalKey(Cow::Owned(key.into())),
        logical: nana_ui_platform::LogicalKey(Cow::Owned(key.into())),
        state: KeyState::Pressed,
        repeat: false,
        modifiers,
    });
    HeadlessInput::bind(cx, doc)
        .route_shaped(cx, event, Some(shaper))
        .unwrap();
}

fn focus_of(cx: &AppContext, area: Entity<TextArea>) -> usize {
    cx.read(area, |view| view.state.selection.focus).unwrap()
}

fn settle(cx: &mut AppContext, doc: DocumentId, ids: &[StableNodeId]) {
    let mut shaper = NanaTextShaper::default();
    cx.resolve_styles(ids).unwrap();
    cx.shape_text(ids, &mut shaper).unwrap();
    cx.layout_document(doc, LayoutViewport::new(600.0, 400.0))
        .unwrap();
    if cx.shape_text_for_layout(doc, &mut shaper).unwrap() {
        cx.layout_document(doc, LayoutViewport::new(600.0, 400.0))
            .unwrap();
    }
    cx.shape_text(ids, &mut shaper).unwrap();
    cx.layout_document(doc, LayoutViewport::new(600.0, 400.0))
        .unwrap();
    cx.rebuild_hit_test(doc);
}

fn caret(cx: &AppContext, node: StableNodeId) -> LayoutBox {
    let Some(ComponentGeometry::TextInput {
        caret: Some(caret), ..
    }) = cx.world().component_geometry(node)
    else {
        panic!("a focused editor draws a caret")
    };
    caret
}

#[test]
fn a_vertical_text_area_edits_in_columns() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mut view = TextArea::new("一二三四五六七八\n九十");
    {
        let style = Arc::make_mut(&mut view.style.layout);
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.font_size = Some(16.0);
        style.width = Some(nana_ui_core::LengthSpec::Px(200.0));
        style.height = Some(nana_ui_core::LengthSpec::Px(64.0));
        style.padding_top = Some(nana_ui_core::LengthSpec::Px(0.0));
        style.padding_bottom = Some(nana_ui_core::LengthSpec::Px(0.0));
    }
    let area = cx.create_component(doc, view).unwrap();
    let node = area.stable_id();
    settle(&mut cx, doc, &[node]);
    assert!(cx.focus_node(doc, node).unwrap());
    let mut shaper = NanaTextShaper::default();
    cx.select_focused_text_range(doc, 0, 0).unwrap();
    settle(&mut cx, doc, &[node]);

    let (content, _) = cx.world().text_input_pointer_context(node).unwrap();
    let at_start = caret(&cx, node);
    // A caret spans its column: the editor's line box, whatever line height
    // the TextArea asks for.
    let column = at_start.width;
    assert!(
        at_start.width > at_start.height,
        "a caret across a column is a horizontal bar: {at_start:?}"
    );
    assert!(
        (at_start.x + at_start.width - (content.x + content.width)).abs() < 0.5
            && (at_start.y - content.y).abs() < 0.5,
        "the first column is the rightmost, its caret at the top: {at_start:?} in {content:?}"
    );

    // The physical ↓ key walks the column.
    let plain = InputModifiers::default();
    press(&mut cx, doc, "ArrowDown", plain, &mut shaper);
    assert_eq!(focus_of(&cx, area), "一".len(), "one glyph down the column");

    // ← crosses to the next column, at the same depth.
    press(&mut cx, doc, "ArrowLeft", plain, &mut shaper);
    assert_eq!(
        focus_of(&cx, area),
        "一二三四五".len(),
        "五 heads the second column, so one down it is after 五"
    );
    settle(&mut cx, doc, &[node]);
    let in_second = caret(&cx, node);
    assert!(
        (in_second.x - (at_start.x - column)).abs() < 0.5
            && (in_second.y - (content.y + 16.0)).abs() < 0.5,
        "drawn one column left, one glyph down: {in_second:?}"
    );

    // A click just below the top of 七, in the second column, lands before it.
    let x = content.x + content.width - column * 1.5;
    let y = content.y + 33.0;
    cx.text_editor_pointer_press(
        doc,
        node,
        1,
        x,
        y,
        false,
        false,
        std::time::Duration::ZERO,
        &mut shaper,
    )
    .unwrap();
    cx.text_editor_pointer_release(1);
    assert_eq!(focus_of(&cx, area), "一二三四五六".len());

    // Modifiers follow the key into line space. Cmd+↓ runs to the end of
    // the line along the column, as Cmd+→ does in a horizontal editor — its
    // logical line, like there — not to the end of the document…
    let meta = InputModifiers {
        meta: true,
        ..InputModifiers::default()
    };
    press(&mut cx, doc, "ArrowDown", meta, &mut shaper);
    assert_eq!(focus_of(&cx, area), "一二三四五六七八".len());
    press(&mut cx, doc, "ArrowUp", meta, &mut shaper);
    assert_eq!(focus_of(&cx, area), 0, "and Cmd+↑ to its start");
    // …while Cmd+← in `vertical-rl` heads for the last column: the end of
    // the text, as Cmd+↓ is in a horizontal editor. Cmd+→ goes back to the
    // start.
    press(&mut cx, doc, "ArrowLeft", meta, &mut shaper);
    assert_eq!(focus_of(&cx, area), "一二三四五六七八\n九十".len());
    press(&mut cx, doc, "ArrowRight", meta, &mut shaper);
    assert_eq!(focus_of(&cx, area), 0);
}

/// #59: a wheel scrolls a vertical editor the way the page is turned. Its
/// scroll offset is physical, like every scroll container's: a `vertical-rl`
/// editor's columns start at the right and its later ones overflow to the
/// left, so reaching them takes a negative `x`, from 0 at the first column.
#[test]
fn a_wheel_scrolls_a_vertical_rl_editor_towards_its_later_columns() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    // Far more columns than the box is wide.
    let mut view = TextArea::new("一二三四五六七八九十".repeat(8));
    {
        let style = Arc::make_mut(&mut view.style.layout);
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.font_size = Some(16.0);
        style.width = Some(nana_ui_core::LengthSpec::Px(120.0));
        style.height = Some(nana_ui_core::LengthSpec::Px(64.0));
    }
    let area = cx.create_component(doc, view).unwrap();
    let node = area.stable_id();
    settle(&mut cx, doc, &[node]);
    let bounds = cx.world().layout_box(node).unwrap();
    let wheel = |delta_x: f32| {
        InputPayload::Wheel(WheelInput {
            pointer_id: PointerId(1),
            x: bounds.x + bounds.width / 2.0,
            y: bounds.y + bounds.height / 2.0,
            delta_x,
            delta_y: 0.0,
            unit: WheelUnit::Pixels,
            modifiers: InputModifiers::default(),
        })
    };
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    // A platform's positive horizontal delta scrolls the page leftwards,
    // which in `vertical-rl` is going on through the text.
    let metrics = cx.world().scroll_metrics(node).expect("measured editor");
    assert!(metrics.origin_x < 0.0, "columns overflow left: {metrics:?}");
    assert_eq!(metrics.max_offset().x, 0.0);
    adapter.route(&mut cx, wheel(40.0)).unwrap();
    let scrolled = cx.world().scroll_offset(node).unwrap_or_default();
    assert!(
        scrolled.x < 0.0,
        "towards the later columns on the left: {scrolled:?}"
    );
    // As far as the last column, and no further.
    adapter.route(&mut cx, wheel(4_000.0)).unwrap();
    assert_eq!(
        cx.world().scroll_offset(node).unwrap_or_default().x,
        metrics.origin_x
    );
    // And the editor is drawn and hit scrolled that far: a click at its left
    // edge lands in the last column, the end of the text.
    let mut shaper = NanaTextShaper::default();
    let text_box = cx.world().text_input_pointer_context(node).unwrap().0;
    cx.text_editor_pointer_press(
        doc,
        node,
        1,
        text_box.x + 1.0,
        text_box.y + text_box.height - 1.0,
        false,
        false,
        Duration::ZERO,
        &mut shaper,
    )
    .unwrap();
    cx.text_editor_pointer_release(1);
    let end = "一二三四五六七八九十".repeat(8).len();
    let area = Entity::<TextArea>::from_stable_id(node);
    assert!(
        focus_of(&cx, area) > end * 3 / 4,
        "a click at the left edge lands among the last columns: {}",
        focus_of(&cx, area)
    );
    // And rightwards comes back to the first column.
    adapter.route(&mut cx, wheel(-400.0)).unwrap();
    assert_eq!(cx.world().scroll_offset(node).unwrap_or_default().x, 0.0);
}

/// A vertical editor's scroll across its columns runs along the page's x
/// axis: the geometry hands it to the painter as a translation (#223), and
/// the value's unscrolled start stays put while the columns scroll.
#[test]
fn a_vertical_rl_editor_hands_its_column_scroll_to_the_painter() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mut view = TextArea::new("一二三四五六七八九十".repeat(8));
    {
        let style = Arc::make_mut(&mut view.style.layout);
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.font_size = Some(16.0);
        style.width = Some(nana_ui_core::LengthSpec::Px(120.0));
        style.height = Some(nana_ui_core::LengthSpec::Px(64.0));
    }
    let area = cx.create_component(doc, view).unwrap();
    let node = area.stable_id();
    settle(&mut cx, doc, &[node]);
    let scroll = |cx: &AppContext| {
        let Some(ComponentGeometry::TextInput {
            text,
            scroll: Some(scroll),
            ..
        }) = cx.world().component_geometry(node)
        else {
            panic!("a multiline editor hands its scroll over")
        };
        (text.bounds, scroll)
    };
    let (still, at_rest) = scroll(&cx);
    assert_eq!(at_rest.offset_x, 0.0);
    assert_eq!(at_rest.text_x, still.x);

    let bounds = cx.world().layout_box(node).unwrap();
    HeadlessInput::bind(&mut cx, doc)
        .route(
            &mut cx,
            InputPayload::Wheel(WheelInput {
                pointer_id: PointerId(1),
                x: bounds.x + bounds.width / 2.0,
                y: bounds.y + bounds.height / 2.0,
                delta_x: 40.37,
                delta_y: 0.0,
                unit: WheelUnit::Pixels,
                modifiers: InputModifiers::default(),
            }),
        )
        .unwrap();
    let scrolled = cx.world().scroll_offset(node).unwrap_or_default().x;
    assert!(scrolled < 0.0, "towards the later columns: {scrolled}");
    let (moved, by) = scroll(&cx);
    // The later columns are on the left: reaching them moves the text right.
    assert_eq!(by.offset_x, -scrolled);
    assert_eq!(by.text_x, at_rest.text_x, "the value's own start stays put");
    assert!(
        (by.text_x + by.offset_x - moved.x).abs() < 1.0e-3,
        "and the page box is that start moved by the scroll: {moved:?} vs {by:?}"
    );
}

/// A single-line vertical field centres its one column across the box, the
/// way a horizontal field centres its line down it, and its caret follows.
#[test]
fn a_vertical_text_input_centres_its_column() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mut view = TextInput::new("縦書き");
    {
        let style = Arc::make_mut(&mut view.style.layout);
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.width = Some(nana_ui_core::LengthSpec::Px(80.0));
        style.height = Some(nana_ui_core::LengthSpec::Px(120.0));
    }
    let input = cx.create_component(doc, view).unwrap();
    let node = input.stable_id();
    settle(&mut cx, doc, &[node]);
    assert!(cx.focus_node(doc, node).unwrap());
    cx.select_focused_text_range(doc, 0, 0).unwrap();
    settle(&mut cx, doc, &[node]);

    let (content, _) = cx.world().text_input_pointer_context(node).unwrap();
    let at = caret(&cx, node);
    let centre = at.x + at.width / 2.0;
    assert!(
        (centre - (content.x + content.width / 2.0)).abs() < 0.5,
        "the column sits in the middle of the field: {at:?} in {content:?}"
    );
    let Some(ComponentGeometry::TextInput { text, .. }) = cx.world().component_geometry(node)
    else {
        panic!("an editor");
    };
    assert!(
        (text.bounds.x + text.bounds.width - (at.x + at.width)).abs() < 0.5,
        "and the painter anchors the column where the caret is: {:?} vs {at:?}",
        text.bounds
    );
}

/// #59: a vertical RTL field starts its line at the bottom (CSS Writing Modes
/// §2.1), and draws its value where its carets are.
///
/// A single-line field's geometry has no line budget, so the painter is given
/// a box exactly as long as the line — no slack for its own `start` alignment
/// to move the glyphs off the carets — and the frame puts that box against the
/// bottom. A wrapping area's geometry and painter share the content height as
/// their budget, so both align each column's `start` to the bottom themselves.
#[test]
fn a_vertical_rtl_editor_starts_its_lines_at_the_bottom() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let rtl_vertical = |style: &mut nana_ui_core::LayoutStyle, height: f32| {
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.dir = Some(nana_ui_core::DirSpec::Rtl);
        style.font_size = Some(16.0);
        style.width = Some(nana_ui_core::LengthSpec::Px(120.0));
        style.height = Some(nana_ui_core::LengthSpec::Px(height));
        style.padding_top = Some(nana_ui_core::LengthSpec::Px(0.0));
        style.padding_bottom = Some(nana_ui_core::LengthSpec::Px(0.0));
    };

    // Single line.
    let mut field = TextInput::new("縦書き");
    rtl_vertical(Arc::make_mut(&mut field.style.layout), 160.0);
    let field = cx.create_component(doc, field).unwrap();
    let node = field.stable_id();
    settle(&mut cx, doc, &[node]);
    assert!(cx.focus_node(doc, node).unwrap());
    cx.select_focused_text_range(doc, 0, 0).unwrap();
    settle(&mut cx, doc, &[node]);
    let (content, _) = cx.world().text_input_pointer_context(node).unwrap();
    let Some(ComponentGeometry::TextInput { text, .. }) = cx.world().component_geometry(node)
    else {
        panic!("an editor")
    };
    let bottom = content.y + content.height;
    assert!(
        (text.bounds.y + text.bounds.height - bottom).abs() < 0.5,
        "the line ends flush with the bottom: {:?} in {content:?}",
        text.bounds
    );
    assert!(
        (text.bounds.height - 48.0).abs() < 0.5,
        "exactly as long as the line, no slack to align in: {:?}",
        text.bounds
    );
    let at = caret(&cx, node);
    assert!(
        at.y >= text.bounds.y - 0.5 && at.y <= bottom + 0.5,
        "the caret is on the drawn line: {at:?} vs {:?}",
        text.bounds
    );
    // A click just above the bottom hits the end of the line, through the
    // same frame — negative inline scroll and all.
    let mut shaper = NanaTextShaper::default();
    cx.text_editor_pointer_press(
        doc,
        node,
        1,
        at.x + at.width / 2.0,
        bottom - 2.0,
        false,
        false,
        Duration::ZERO,
        &mut shaper,
    )
    .unwrap();
    cx.text_editor_pointer_release(1);
    assert_eq!(
        cx.read(field, |view| view.state.selection.focus).unwrap(),
        "縦書き".len()
    );

    // Wrapping area: six ideographs to a 100px column, so the second column
    // holds two and sits against the bottom.
    let mut area = TextArea::new("一二三四五六七八");
    rtl_vertical(Arc::make_mut(&mut area.style.layout), 100.0);
    let area = cx.create_component(doc, area).unwrap();
    let area_node = area.stable_id();
    settle(&mut cx, doc, &[area_node]);
    assert!(cx.focus_node(doc, area_node).unwrap());
    cx.select_focused_text_range(doc, "一二三四五六".len(), "一二三四五六".len())
        .unwrap();
    settle(&mut cx, doc, &[area_node]);
    let (content, _) = cx.world().text_input_pointer_context(area_node).unwrap();
    let before_seven = caret(&cx, area_node);
    assert!(
        (before_seven.y - (content.y + content.height - 32.0)).abs() < 0.5,
        "七八 is flush with the bottom, so 七 starts 32px above it: {before_seven:?} in {content:?}"
    );
}

/// #59: arrow keys in a vertical RTL editor follow the paragraph's reading
/// direction the way they do in a horizontal RTL one, turned a quarter.
///
/// The paragraph reads from the bottom, so ↑ is onward: past the top of a
/// column it goes on at the foot of the next, as ← past the left of an RTL
/// line goes on at the right of the next. ↓ is back the other way. Within a
/// column the step is visual, and CJK still sits top to bottom.
#[test]
fn arrow_keys_in_a_vertical_rtl_editor_follow_its_reading_direction() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mut area = TextArea::new("一二三四五六七八");
    {
        let style = Arc::make_mut(&mut area.style.layout);
        style.writing_mode = Some(nana_ui_core::WritingModeSpec::VerticalRl);
        style.dir = Some(nana_ui_core::DirSpec::Rtl);
        style.font_size = Some(16.0);
        style.width = Some(nana_ui_core::LengthSpec::Px(120.0));
        style.height = Some(nana_ui_core::LengthSpec::Px(100.0));
        style.padding_top = Some(nana_ui_core::LengthSpec::Px(0.0));
        style.padding_bottom = Some(nana_ui_core::LengthSpec::Px(0.0));
    }
    let area = cx.create_component(doc, area).unwrap();
    let node = area.stable_id();
    settle(&mut cx, doc, &[node]);
    assert!(cx.focus_node(doc, node).unwrap());
    cx.select_focused_text_range(doc, 0, 0).unwrap();
    settle(&mut cx, doc, &[node]);
    let mut shaper = NanaTextShaper::default();
    let plain = InputModifiers::default();

    // Down the first column, glyph by glyph.
    press(&mut cx, doc, "ArrowDown", plain, &mut shaper);
    assert_eq!(focus_of(&cx, area), "一".len());
    // Back up to its top, then on past it: the foot of the next column.
    press(&mut cx, doc, "ArrowUp", plain, &mut shaper);
    assert_eq!(focus_of(&cx, area), 0);
    press(&mut cx, doc, "ArrowUp", plain, &mut shaper);
    assert_eq!(
        focus_of(&cx, area),
        "一二三四五六七八".len(),
        "onward past the top of 一…六 is the foot of 七八"
    );
    // ↓ from the foot of 七八 goes back past its logical start, the
    // other way.
    press(&mut cx, doc, "ArrowDown", plain, &mut shaper);
    assert_eq!(
        focus_of(&cx, area),
        0,
        "back past the foot of 七八 is the top of the first column"
    );
}
