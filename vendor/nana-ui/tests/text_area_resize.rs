use nana_ui::{HeadlessInput, NanaTextShaper, runtime::*};
use nana_ui_platform::{
    InputModifiers, InputPayload, PointerId, PointerInput, PointerPhase, PointerType,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
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

fn layout(cx: &mut AppContext, doc: DocumentId, area: Entity<TextArea>) {
    cx.resolve_styles(&[area.stable_id()]).unwrap();
    cx.shape_text(&[area.stable_id()], &mut NanaTextShaper::default())
        .unwrap();
    cx.layout_document(doc, LayoutViewport::new(500.0, 500.0))
        .unwrap();
    cx.rebuild_hit_test(doc);
}
fn grip(cx: &AppContext, area: Entity<TextArea>) -> (f32, f32) {
    let ComponentGeometry::TextInput {
        resize_grip: Some(rect),
        ..
    } = cx.world().component_geometry(area.stable_id()).unwrap()
    else {
        panic!("resize geometry")
    };
    cx.world()
        .layout_pointer_position(area.stable_id(), rect.x + 7.0, rect.y + 7.0)
        .unwrap()
}

#[test]
fn textarea_resize_normal_pointer_clamps_preserves_text_and_cancel_restores_height() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mut view = TextArea::new("unchanged")
        .height(160.0)
        .resize_vertical(true);
    let style = Arc::make_mut(&mut view.style.layout);
    style.width = Some(nana_ui_core::LengthSpec::Px(280.0));
    style.min_height = Some(nana_ui_core::LengthSpec::Px(148.0));
    style.max_height = Some(nana_ui_core::LengthSpec::Px(240.0));
    let area = cx.create_component(doc, view).unwrap();
    let events = Arc::new(AtomicUsize::new(0));
    let event_count = Arc::clone(&events);
    cx.on(area, move |_, _: &TextChanged, _| {
        event_count.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    layout(&mut cx, doc, area);
    let initial = cx.read(area, |view| view.state.clone()).unwrap();
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    let (x, y) = grip(&cx, area);
    assert!(
        adapter
            .route(&mut cx, pointer(PointerPhase::Down, x, y))
            .unwrap()
            .prevent_default
    );
    assert_eq!(cx.world().pointer_capture(doc, 1), Some(area.stable_id()));
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, x + 100.0, y + 500.0))
        .unwrap();
    layout(&mut cx, doc, area);
    let bounds = cx.world().layout_box(area.stable_id()).unwrap();
    assert_eq!(bounds.width, 280.0);
    assert_eq!(bounds.height, 240.0);
    adapter
        .route(&mut cx, pointer(PointerPhase::Cancel, x, y))
        .unwrap();
    layout(&mut cx, doc, area);
    assert_eq!(
        cx.world().layout_box(area.stable_id()).unwrap().height,
        160.0
    );
    assert_eq!(cx.world().pointer_capture(doc, 1), None);
    let (x, y) = grip(&cx, area);
    adapter
        .route(&mut cx, pointer(PointerPhase::Down, x, y))
        .unwrap();
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, x, y - 100.0))
        .unwrap();
    adapter
        .route(&mut cx, pointer(PointerPhase::Up, x, y - 100.0))
        .unwrap();
    layout(&mut cx, doc, area);
    assert_eq!(
        cx.world().layout_box(area.stable_id()).unwrap().height,
        148.0
    );
    assert_eq!(cx.read(area, |view| view.state.clone()).unwrap(), initial);
    assert_eq!(events.load(Ordering::SeqCst), 0);
    cx.update_component(area, |view, _| {
        view.state.replace_value("external draft");
    })
    .unwrap();
    layout(&mut cx, doc, area);
    assert_eq!(
        cx.world().layout_box(area.stable_id()).unwrap().height,
        148.0
    );
    assert_eq!(cx.world().text(area.stable_id()), Some("external draft"));
}

#[test]
fn textarea_resize_readonly_allowed_disabled_rejected_and_removal_releases_capture() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let area = cx
        .create_component(
            doc,
            TextArea::new("read only")
                .height(160.0)
                .read_only(true)
                .resize_vertical(true),
        )
        .unwrap();
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    layout(&mut cx, doc, area);
    let (x, y) = grip(&cx, area);
    adapter
        .route(&mut cx, pointer(PointerPhase::Down, x, y))
        .unwrap();
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, x, y + 30.0))
        .unwrap();
    adapter
        .route(&mut cx, pointer(PointerPhase::Up, x, y + 30.0))
        .unwrap();
    layout(&mut cx, doc, area);
    assert_eq!(
        cx.world().layout_box(area.stable_id()).unwrap().height,
        190.0
    );
    cx.update_component(area, |view, _| view.disabled = true)
        .unwrap();
    layout(&mut cx, doc, area);
    let (x, y) = grip(&cx, area);
    adapter
        .route(&mut cx, pointer(PointerPhase::Down, x, y))
        .unwrap();
    assert_eq!(cx.world().pointer_capture(doc, 1), None);
    cx.update_component(area, |view, _| view.disabled = false)
        .unwrap();
    layout(&mut cx, doc, area);
    let (x, y) = grip(&cx, area);
    adapter
        .route(&mut cx, pointer(PointerPhase::Down, x, y))
        .unwrap();
    assert_eq!(cx.world().pointer_capture(doc, 1), Some(area.stable_id()));
    cx.remove_view(area).unwrap();
    assert_eq!(cx.world().pointer_capture(doc, 1), None);
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, x, y + 30.0))
        .unwrap();
}

#[test]
fn textarea_resize_uses_transformed_logical_coordinates_and_content_box_chrome() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let mut view = TextArea::new("transform")
        .height(100.0)
        .resize_vertical(true);
    let style = Arc::make_mut(&mut view.style.layout);
    style.width = Some(nana_ui_core::LengthSpec::Px(150.0));
    style.box_sizing = nana_ui_core::BoxSizing::ContentBox;
    style.min_height = None;
    style.transform = Some(nana_ui_core::PaintTransform {
        a: 2.0,
        d: 2.0,
        ..Default::default()
    });
    let area = cx.create_component(doc, view).unwrap();
    layout(&mut cx, doc, area);
    let height = cx.world().layout_box(area.stable_id()).unwrap().height;
    let (x, y) = grip(&cx, area);
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    adapter
        .route(&mut cx, pointer(PointerPhase::Down, x, y))
        .unwrap();
    adapter
        .route(&mut cx, pointer(PointerPhase::Move, x, y + 40.0))
        .unwrap();
    adapter
        .route(&mut cx, pointer(PointerPhase::Up, x, y + 40.0))
        .unwrap();
    layout(&mut cx, doc, area);
    assert!((cx.world().layout_box(area.stable_id()).unwrap().height - height - 20.0).abs() < 0.1);
}

/// A multiline editor's value keeps its line breaks whatever `white-space`
/// says. The scene has to say so: the renderer lays the same text out again,
/// and a paragraph measured as six lines and painted as one overflows nothing
/// — it silently loses the breaks the user typed.
#[test]
fn a_multiline_editors_text_primitive_says_its_newlines_are_line_breaks() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let view = TextArea::new("first\nsecond\nthird");
    let area = cx.create_component(doc, view).unwrap();
    layout(&mut cx, doc, area);
    let extracted = cx.world().extract_document(doc);
    let node = extracted
        .iter()
        .find(|node| node.id == area.stable_id())
        .expect("the editor must be extracted");
    assert!(
        node.text_preserve_lines,
        "a multiline editor preserves the newlines in its value"
    );
}
