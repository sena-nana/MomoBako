use std::sync::{Arc, Mutex};

use nana_ui_core::{LayoutStyle, OverflowSpec};

use super::*;
use crate::{
    ActionMenu, ActionMenuItem, Activate, Button, Card, ComponentGeometry, Dialog, Dock, DockAxis,
    DockNode, Entity, LayoutBox, MeasureTextShaper, ModalSlots, MutationQueue, NodeKind, NodeStyle,
    OverlayHost, OverlayHostState, RangeField, ScrollAxes, ScrollMetrics, ScrollView,
    SegmentedControl, SegmentedOption, SegmentedSelectionRequested, Table, TableCell, TableRow,
    Text, TextArea, TextChanged, TextFindScope, TextInput, TextSearchOptions, TextSelection,
    UserSelectSpec,
};
#[cfg(feature = "calendar")]
use crate::{CalendarHeatmap, CalendarHeatmapDatum};
#[cfg(feature = "graph-canvas")]
use crate::{
    GraphMinimap, GraphMinimapEvent, GraphModel, GraphNode, GraphPoint, GraphSize, GraphViewport,
};

fn wheel(x: f32, y: f32, delta_y: f32) -> Gesture {
    wheel_fixture! {
        x,
        y,
        delta_x: 0.0,
        delta_y,
        line_delta: true,
        modifiers: InputModifiers::default(),
    }
}

fn focused_untyped_text_input(
    context: &mut AppContext,
    value: &str,
) -> (DocumentId, crate::StableNodeId) {
    let document = DocumentId::new(1).unwrap();
    let id = crate::StableNodeId::new(1).unwrap();
    let mut create = MutationQueue::new();
    create.create(
        id,
        document,
        crate::NodeKind::Element {
            tag: "input".into(),
        },
    );
    create.set_interaction(
        id,
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    create.set_text_input(id, Some(crate::TextInputState::new(value)));
    create.set_accessibility(
        id,
        crate::AccessibilityState {
            role: crate::AccessibilityRole::TextInput,
            editable: true,
            ..crate::AccessibilityState::default()
        },
    );
    create.request_focus(document, Some(id));
    context.commit_mutations(create).unwrap();
    (document, id)
}

fn pointer(phase: PointerPhase, x: f32, y: f32) -> Gesture {
    pointer_with(phase, x, y, false)
}

fn pointer_with(phase: PointerPhase, x: f32, y: f32, activation_click: bool) -> Gesture {
    pointer_fixture! {
        phase,
        pointer_id: 1,
        pointer_type: PointerType::Mouse,
        x,
        y,
        screen_x: x,
        screen_y: y,
        button: 0,
        buttons: u16::from(phase == PointerPhase::Down),
        pressure: 0.0,
        tangential_pressure: 0.0,
        tilt_x: 0,
        tilt_y: 0,
        twist: 0,
        is_primary: true,
        activation_click,
        modifiers: InputModifiers::default(),
    }
}

#[test]
fn pointer_release_activates_the_retained_hit_target() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, Button::new("Build"))
        .unwrap();
    context
        .on(button, |button, _event: &Activate, _cx| {
            button.label = "Running".into();
        })
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 120.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 30.0, 30.0)
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Up, 30.0, 30.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(button.stable_id()), Some("Running"));
}

#[test]
fn text_area_resize_routes_grip_drag_cancel_and_park_without_editing_text() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("retained text")
                .height(100.0)
                .resize_vertical(true),
        )
        .unwrap();
    let viewport = crate::LayoutViewport::new(320.0, 500.0);
    context.resolve_styles(&[area.stable_id()]).unwrap();
    context
        .shape_text(&[area.stable_id()], &mut MeasureTextShaper)
        .unwrap();
    context.layout_document(document, viewport).unwrap();
    context.rebuild_hit_test(document);
    let Some(ComponentGeometry::TextInput {
        resize_grip: Some(grip),
        ..
    }) = context.world().component_geometry(area.stable_id())
    else {
        panic!("resize grip must be projected")
    };
    let x = grip.x + grip.width / 2.0;
    let y = grip.y + grip.height / 2.0;
    let initial = context.world().layout_box(area.stable_id()).unwrap().height;
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(&mut context, document, &pointer(PointerPhase::Down, x, y))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().pointer_capture(document, 1),
        Some(area.stable_id())
    );
    adapter
        .dispatch(
            &mut context,
            document,
            &pointer(PointerPhase::Move, x, y + 45.0),
        )
        .unwrap();
    context.layout_document(document, viewport).unwrap();
    assert!(
        (context.world().layout_box(area.stable_id()).unwrap().height - initial - 45.0).abs()
            < 0.01
    );
    adapter
        .dispatch(
            &mut context,
            document,
            &pointer(PointerPhase::Cancel, x, y + 45.0),
        )
        .unwrap();
    context.layout_document(document, viewport).unwrap();
    context.rebuild_hit_test(document);
    assert_eq!(
        context.world().layout_box(area.stable_id()).unwrap().height,
        initial
    );
    assert_eq!(context.world().pointer_capture(document, 1), None);
    context.resolve_styles(&[area.stable_id()]).unwrap();
    adapter
        .dispatch(&mut context, document, &pointer(PointerPhase::Down, x, y))
        .unwrap();
    context
        .update_text_area_resize(document, 1, x, y + 30.0)
        .unwrap();
    let other = context
        .create_component(document, Button::new("other capture"))
        .unwrap();
    let mut steal = MutationQueue::new();
    steal.capture_pointer(1, other.stable_id());
    context.commit_mutations(steal).unwrap();
    assert!(context.end_text_area_resize(document, 1, false).unwrap());
    assert_eq!(
        context.world().pointer_capture(document, 1),
        Some(other.stable_id())
    );
    context.remove_view(other).unwrap();
    context.resolve_styles(&[area.stable_id()]).unwrap();
    context.layout_document(document, viewport).unwrap();
    context.rebuild_hit_test(document);
    assert_eq!(
        context.world().layout_box(area.stable_id()).unwrap().height,
        initial
    );
    adapter
        .dispatch(&mut context, document, &pointer(PointerPhase::Down, x, y))
        .unwrap();
    let mut queue = MutationQueue::new();
    queue.park_subtree(area.stable_id());
    context.commit_mutations(queue).unwrap();
    assert!(
        !context
            .update_text_area_resize(document, 1, x, y + 70.0)
            .unwrap()
    );
    assert_eq!(context.world().pointer_capture(document, 1), None);
    assert_eq!(
        context.read(area, |area| area.state.value.clone()).unwrap(),
        "retained text"
    );
}

#[test]
fn macos_activation_click_does_not_activate_the_hit_target() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, Button::new("Build"))
        .unwrap();
    context
        .on(button, |button, _event: &Activate, _cx| {
            button.label = "Running".into();
        })
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 120.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer_with(PointerPhase::Down, 30.0, 30.0, true)
            )
            .unwrap()
            .prevent_default
    );
    let _ = adapter.dispatch(
        &mut context,
        document,
        &pointer_with(PointerPhase::Up, 30.0, 30.0, true),
    );
    assert_eq!(context.world().text(button.stable_id()), Some("Build"));
}

#[cfg(feature = "rich-text")]
#[test]
fn markdown_link_pointer_uses_the_painted_padded_content_and_cancels_stolen_capture() {
    use crate::{MarkdownDrawingCommand, NativeMarkdown, RichTextEvent};
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut markdown = NativeMarkdown::from_source("[Open](https://example.com)");
    let layout = Arc::make_mut(&mut markdown.style.layout);
    layout.padding = Some(nana_ui_core::LengthSpec::Px(28.0));
    layout.border_width = Some(3.0);
    let markdown = context.create_component(document, markdown).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    context
        .on(markdown, move |_, event: &RichTextEvent, _| {
            sink.lock().unwrap().push(event.clone())
        })
        .unwrap();
    context.resolve_styles(&[markdown.stable_id()]).unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(300.0, 160.0))
        .unwrap();
    context.rebuild_hit_test(document);
    let Some(ComponentGeometry::NativeMarkdown { drawing, .. }) =
        context.world().component_geometry(markdown.stable_id())
    else {
        panic!("markdown geometry")
    };
    let bounds = drawing
        .commands
        .iter()
        .find_map(|command| match command {
            MarkdownDrawingCommand::Text { bounds, .. } => Some(*bounds),
            _ => None,
        })
        .expect("painted link text");
    let x = bounds.x + 1.0;
    let y = bounds.y + bounds.height / 2.0;
    assert!(bounds.x >= 31.0);
    let mut adapter = TestInput::default();
    adapter
        .dispatch(&mut context, document, &pointer(PointerPhase::Down, x, y))
        .unwrap();
    adapter
        .dispatch(&mut context, document, &pointer(PointerPhase::Up, x, y))
        .unwrap();
    assert!(events.lock().unwrap().iter().any(|event| matches!(event, RichTextEvent::LinkActivated(url) if url.as_ref() == "https://example.com")));
    events.lock().unwrap().clear();
    adapter
        .dispatch(&mut context, document, &pointer(PointerPhase::Down, x, y))
        .unwrap();
    let other = context
        .create_component(document, Button::new("new owner"))
        .unwrap();
    let mut queue = MutationQueue::new();
    queue.capture_pointer(1, other.stable_id());
    context.commit_mutations(queue).unwrap();
    assert!(
        context
            .end_rich_text_pointer(document, 1, x, y, false)
            .unwrap()
    );
    assert_eq!(
        context.world().pointer_capture(document, 1),
        Some(other.stable_id())
    );
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn a_right_button_press_dispatches_a_secondary_press_without_activating() {
    use crate::SecondaryPress;
    use std::sync::{Arc, Mutex};

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, Button::new("Build"))
        .unwrap();
    let presses = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&presses);
    context
        .on(button, move |_button, press: &SecondaryPress, _cx| {
            observed.lock().unwrap().push(*press);
        })
        .unwrap();
    context
        .on(button, |button, _event: &Activate, _cx| {
            button.label = "Running".into();
        })
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 120.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    let mut secondary = pointer(PointerPhase::Down, 30.0, 30.0);
    if let Gesture::Event(InputPayload::Pointer(PointerInput { button, .. })) = &mut secondary {
        *button = 2;
    }
    assert!(
        adapter
            .dispatch(&mut context, document, &secondary)
            .unwrap()
            .prevent_default
    );
    let press = *presses
        .lock()
        .unwrap()
        .first()
        .expect("one secondary press");
    assert_eq!(press.target, button.stable_id());
    assert_eq!((press.x, press.y), (30.0, 30.0));

    // The release must not activate: no press was recorded for button 2.
    let mut release = pointer(PointerPhase::Up, 30.0, 30.0);
    if let Gesture::Event(InputPayload::Pointer(PointerInput { button, .. })) = &mut release {
        *button = 2;
    }
    adapter.dispatch(&mut context, document, &release).unwrap();
    assert_eq!(context.world().text(button.stable_id()), Some("Build"));
}

#[test]
#[cfg(feature = "graph-canvas")]
fn pointer_drag_on_a_graph_minimap_requests_viewport_navigation() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let model = GraphModel::new(
        vec![GraphNode::new(
            "node",
            "Node",
            GraphPoint::ZERO,
            GraphSize::new(200.0, 100.0),
        )],
        Vec::new(),
    )
    .expect("valid graph");
    let minimap = context
        .create_component(
            document,
            GraphMinimap::new(model).canvas_size(GraphSize::new(400.0, 200.0)),
        )
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&events);
    context
        .on(minimap, move |_minimap, event: &GraphMinimapEvent, _cx| {
            observed.lock().unwrap().push(event.clone());
        })
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        minimap.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    for (phase, x, y) in [
        (PointerPhase::Down, 50.0, 25.0),
        (PointerPhase::Move, 60.0, 30.0),
        (PointerPhase::Up, 60.0, 30.0),
    ] {
        assert!(
            adapter
                .dispatch(&mut context, document, &pointer(phase, x, y))
                .unwrap()
                .prevent_default
        );
    }
    assert_eq!(
        *events.lock().unwrap(),
        [
            GraphMinimapEvent::ViewportRequested(GraphViewport::new(
                GraphPoint::new(100.0, 50.0),
                1.0
            )),
            GraphMinimapEvent::ViewportRequested(GraphViewport::new(
                GraphPoint::new(80.0, 40.0),
                1.0
            )),
        ]
    );
    assert!(context.world().pointer_capture(document, 1).is_none());
}

#[test]
fn pointer_down_moves_focus_to_the_hit_text_input() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, Button::new("Other"))
        .unwrap();
    let input = context
        .create_component(document, TextInput::new("NanaUI"))
        .unwrap();
    assert!(context.focus_node(document, button.stable_id()).unwrap());
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 32.0,
        },
    );
    layout.write_layout(
        input.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 40.0,
            width: 160.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 24.0, 52.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().focused(document), Some(input.stable_id()));
}

#[test]
fn focused_textarea_caret_uses_text_color_and_clears_on_outside_press() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let editor = context
        .create_component(document, TextArea::new("draft"))
        .unwrap();
    let surface = context.create_component(document, Card::new()).unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        editor.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 72.0,
        },
    );
    layout.write_layout(
        surface.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 90.0,
            width: 200.0,
            height: 80.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    assert!(context.focus_node(document, editor.stable_id()).unwrap());
    let work = context.take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut MeasureTextShaper)
        .unwrap();
    context.rebuild_hit_test(document);

    let Some(ComponentGeometry::TextInput {
        caret, caret_color, ..
    }) = context.world().component_geometry(editor.stable_id())
    else {
        panic!("expected text input geometry");
    };
    let palette = nana_ui_core::SemanticPalette::dark();
    assert!(caret.is_some());
    assert_eq!(caret_color, palette.text.as_rgba_array());
    assert_ne!(caret_color, palette.accent.as_rgba_array());

    TestInput::default()
        .dispatch(
            &mut context,
            document,
            &pointer(PointerPhase::Down, 24.0, 120.0),
        )
        .unwrap();
    assert_eq!(context.world().focused(document), None);
    assert!(matches!(
        context.world().component_geometry(editor.stable_id()),
        Some(ComponentGeometry::TextInput { caret: None, .. })
    ));

    assert!(context.focus_node(document, editor.stable_id()).unwrap());
    TestInput::default()
        .dispatch(
            &mut context,
            document,
            &pointer(PointerPhase::Down, 400.0, 400.0),
        )
        .unwrap();
    assert_eq!(context.world().focused(document), None);
}

#[test]
fn segmented_pointer_lease_consumes_release_and_only_requests_on_inside_up() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let control = context
        .create_component(document, SegmentedControl::new())
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let second = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    context
        .set_segmented_options(control, vec![first, second], Some(first))
        .unwrap();
    let requests = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&requests);
    context
        .on(
            control,
            move |_control, _event: &SegmentedSelectionRequested, _cx| {
                *observed.lock().unwrap() += 1;
            },
        )
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        control.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 180.0,
            height: 32.0,
        },
    );
    layout.write_layout(
        first.stable_id(),
        LayoutBox {
            x: 4.0,
            y: 3.0,
            width: 70.0,
            height: 26.0,
        },
    );
    layout.write_layout(
        second.stable_id(),
        LayoutBox {
            x: 76.0,
            y: 3.0,
            width: 70.0,
            height: 26.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.rebuild_hit_test(document);
    let mut adapter = TestInput::default();

    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 90.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().focused(document), Some(second.stable_id()));
    assert_eq!(
        context
            .read(control, SegmentedControl::focus_target)
            .unwrap(),
        Some(second.stable_id())
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Cancel, 90.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(*requests.lock().unwrap(), 0);

    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 20.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Up, 140.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(*requests.lock().unwrap(), 0);
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 20.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Up, 20.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(*requests.lock().unwrap(), 1);
    assert!(context.read(first, SegmentedOption::selected).unwrap());
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 20.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Cancel, 20.0, 12.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(*requests.lock().unwrap(), 1);
}

#[test]
fn document_tab_order_uses_one_roving_entry_and_wraps_both_directions() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let before = context
        .create_component(document, Button::new("Before"))
        .unwrap();
    let control = context
        .create_component(document, SegmentedControl::new())
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let second = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    let after = context
        .create_component(document, Button::new("After"))
        .unwrap();
    context
        .set_segmented_options(control, vec![first, second], Some(first))
        .unwrap();
    context.focus_node(document, before.stable_id()).unwrap();
    let tab = |shift| {
        key_fixture! {
            pressed: true,
            key: "Tab".into(),
            text: None,
            code: "Tab".into(),
            repeat: false,
            modifiers: InputModifiers {
                shift,
                ..InputModifiers::default()
            },
        }
    };
    let mut adapter = TestInput::default();
    for expected in [first.stable_id(), after.stable_id(), before.stable_id()] {
        assert!(
            adapter
                .dispatch(&mut context, document, &tab(false))
                .unwrap()
                .prevent_default
        );
        assert_eq!(context.world().focused(document), Some(expected));
    }
    assert!(
        adapter
            .dispatch(&mut context, document, &tab(true))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().focused(document), Some(after.stable_id()));
    assert!(context.focus_node(document, second.stable_id()).unwrap());
}

#[test]
fn focused_runtime_text_uses_keyboard_and_ime_state() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("Nana"))
        .unwrap();
    assert!(context.focus_node(document, input.stable_id()).unwrap());
    let key = |key: &str| {
        key_fixture! {
            pressed: true,
            key: key.into(),
            text: (key.chars().count() == 1).then(|| key.into()),
            code: key.into(),
            repeat: false,
            modifiers: InputModifiers::default(),
        }
    };

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(&mut context, document, &key("U"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(input.stable_id()), Some("NanaU"));
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Update {
                    text: "你".into(),
                    selection: None,
                },
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().ime(input.stable_id()).map(|ime| ime.text),
        Some("你")
    );
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Commit("你".into())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(input.stable_id()), Some("NanaU你"));
    assert_eq!(context.world().ime(input.stable_id()), None);
    assert!(
        adapter
            .dispatch(&mut context, document, &key("Backspace"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(input.stable_id()), Some("NanaU"));
}

#[test]
fn clipboard_shortcuts_move_text_between_the_editor_and_the_pasteboard() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("Nana"))
        .unwrap();
    assert!(context.focus_node(document, input.stable_id()).unwrap());

    let mut adapter = TestInput::default();
    let primary = |key: &str| {
        key_fixture! {
            pressed: true,
            key: key.into(),
            text: None,
            code: key.into(),
            repeat: false,
            modifiers: InputModifiers {
                control: true,
                ..InputModifiers::default()
            },
        }
    };

    // Nothing is selected yet, so a copy must not clear the pasteboard.
    assert!(
        !adapter
            .dispatch(&mut context, document, &primary("c"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(adapter.clipboard(), None);

    assert!(
        adapter
            .dispatch(&mut context, document, &primary("a"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &primary("x"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(input.stable_id()), Some(""));
    assert_eq!(adapter.clipboard(), Some("Nana"));

    assert!(
        adapter
            .dispatch(&mut context, document, &primary("v"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &primary("v"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(input.stable_id()), Some("NanaNana"));

    assert!(
        adapter
            .dispatch(&mut context, document, &primary("a"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &primary("c"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(input.stable_id()), Some("NanaNana"));
    assert_eq!(adapter.clipboard(), Some("NanaNana"));
}

fn document_text_pointer(phase: PointerPhase, x: f32, y: f32) -> Gesture {
    pointer_fixture! {
        phase,
        pointer_id: 7,
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
    }
}

fn mount_document_text(
    context: &mut AppContext,
    value: &str,
    user_select: UserSelectSpec,
) -> (DocumentId, crate::StableNodeId) {
    let document = DocumentId::new(1).unwrap();
    let mut style = NodeStyle::default();
    Arc::make_mut(&mut style.layout).user_select = Some(user_select);
    let label = context
        .create_component(document, Text::new(value).style(style))
        .unwrap();
    let node = label.stable_id();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.resolve_styles(&[node]).unwrap();
    (document, node)
}

#[expect(
    clippy::too_many_arguments,
    reason = "test helper: fixture plus one pointer event"
)]
fn dispatch_document_pointer(
    adapter: &mut TestInput,
    context: &mut AppContext,
    document: DocumentId,
    shaper: &mut MeasureTextShaper,
    phase: PointerPhase,
    x: f32,
    y: f32,
    millis: u64,
) {
    adapter
        .dispatch_with_shaper(
            context,
            document,
            &document_text_pointer(phase, x, y),
            Duration::from_millis(millis),
            Some(shaper),
        )
        .unwrap();
}

fn copy_shortcut() -> Gesture {
    key_fixture! {
        pressed: true,
        key: "c".into(),
        text: None,
        code: "c".into(),
        repeat: false,
        modifiers: InputModifiers {
            control: true,
            ..InputModifiers::default()
        },
    }
}

#[test]
fn user_select_text_drag_copies_and_empty_or_none_leave_the_pasteboard() {
    let primary = |key: &str| {
        key_fixture! {
            pressed: true,
            key: key.into(),
            text: None,
            code: key.into(),
            repeat: false,
            modifiers: InputModifiers {
                control: true,
                ..InputModifiers::default()
            },
        }
    };

    let mut context = AppContext::new();
    let (document, node) = mount_document_text(&mut context, "Hello copy", UserSelectSpec::Text);
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::with_clipboard("keep-me");
    dispatch_document_pointer(
        &mut adapter,
        &mut context,
        document,
        &mut shaper,
        PointerPhase::Down,
        2.0,
        16.0,
        1_000,
    );
    dispatch_document_pointer(
        &mut adapter,
        &mut context,
        document,
        &mut shaper,
        PointerPhase::Move,
        380.0,
        16.0,
        1_010,
    );
    dispatch_document_pointer(
        &mut adapter,
        &mut context,
        document,
        &mut shaper,
        PointerPhase::Up,
        380.0,
        16.0,
        1_020,
    );
    assert_eq!(
        context.document_selected_text(document).as_deref(),
        Some("Hello copy")
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &primary("c"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(adapter.clipboard(), Some("Hello copy"));
    assert!(
        !adapter
            .dispatch(&mut context, document, &primary("x"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(node), Some("Hello copy"));

    let mut none_context = AppContext::new();
    let (none_document, _) =
        mount_document_text(&mut none_context, "Hello copy", UserSelectSpec::None);
    let mut none_shaper = MeasureTextShaper;
    let mut none_adapter = TestInput::with_clipboard("keep-me");
    dispatch_document_pointer(
        &mut none_adapter,
        &mut none_context,
        none_document,
        &mut none_shaper,
        PointerPhase::Down,
        2.0,
        16.0,
        2_000,
    );
    dispatch_document_pointer(
        &mut none_adapter,
        &mut none_context,
        none_document,
        &mut none_shaper,
        PointerPhase::Move,
        380.0,
        16.0,
        2_010,
    );
    dispatch_document_pointer(
        &mut none_adapter,
        &mut none_context,
        none_document,
        &mut none_shaper,
        PointerPhase::Up,
        380.0,
        16.0,
        2_020,
    );
    assert!(none_context.document_selected_text(none_document).is_none());
    assert!(
        !none_adapter
            .dispatch(&mut none_context, none_document, &primary("c"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(none_adapter.clipboard(), Some("keep-me"));

    let mut empty_context = AppContext::new();
    let (empty_document, _) =
        mount_document_text(&mut empty_context, "Hello copy", UserSelectSpec::Text);
    let mut empty_shaper = MeasureTextShaper;
    let mut empty_adapter = TestInput::with_clipboard("keep-me");
    dispatch_document_pointer(
        &mut empty_adapter,
        &mut empty_context,
        empty_document,
        &mut empty_shaper,
        PointerPhase::Down,
        2.0,
        16.0,
        3_000,
    );
    dispatch_document_pointer(
        &mut empty_adapter,
        &mut empty_context,
        empty_document,
        &mut empty_shaper,
        PointerPhase::Up,
        2.0,
        16.0,
        3_010,
    );
    assert!(
        empty_context
            .document_selected_text(empty_document)
            .is_none()
    );
    assert!(
        !empty_adapter
            .dispatch(&mut empty_context, empty_document, &primary("c"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(empty_adapter.clipboard(), Some("keep-me"));
}

#[test]
fn user_select_all_click_copies_without_drag() {
    let mut context = AppContext::new();
    let (document, _) = mount_document_text(&mut context, "Hello copy", UserSelectSpec::All);
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::with_clipboard("keep-me");
    dispatch_document_pointer(
        &mut adapter,
        &mut context,
        document,
        &mut shaper,
        PointerPhase::Down,
        2.0,
        16.0,
        1_000,
    );
    dispatch_document_pointer(
        &mut adapter,
        &mut context,
        document,
        &mut shaper,
        PointerPhase::Up,
        2.0,
        16.0,
        1_010,
    );
    assert_eq!(
        context.document_selected_text(document).as_deref(),
        Some("Hello copy")
    );
    let copied = adapter
        .dispatch(&mut context, document, &copy_shortcut())
        .unwrap();
    assert!(copied.prevent_default);
    assert_eq!(adapter.clipboard(), Some("Hello copy"));
}

#[test]
fn a_read_only_field_copies_but_never_loses_text_to_a_cut() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("Nana").read_only(true))
        .unwrap();
    assert!(context.focus_node(document, input.stable_id()).unwrap());

    let mut adapter = TestInput::default();
    let primary = |key: &str| {
        key_fixture! {
            pressed: true,
            key: key.into(),
            text: None,
            code: key.into(),
            repeat: false,
            modifiers: InputModifiers {
                meta: true,
                ..InputModifiers::default()
            },
        }
    };

    assert!(
        adapter
            .dispatch(&mut context, document, &primary("a"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &primary("x"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(adapter.clipboard(), Some("Nana"));
    assert_eq!(context.world().text(input.stable_id()), Some("Nana"));

    adapter.set_clipboard("pasted");
    assert!(
        !adapter
            .dispatch(&mut context, document, &primary("v"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(input.stable_id()), Some("Nana"));
}

#[test]
fn focused_runtime_text_inserts_shifted_printable_characters() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("inspect "))
        .unwrap();
    assert!(context.focus_node(document, area.stable_id()).unwrap());
    let event = key_fixture! {
        pressed: true,
        key: "2".into(),
        text: Some("@".into()),
        code: "Digit2".into(),
        repeat: false,
        modifiers: InputModifiers {
            shift: true,
            ..InputModifiers::default()
        },
    };
    assert!(
        TestInput::default()
            .dispatch(&mut context, document, &event)
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text(area.stable_id()), Some("inspect @"));
}

#[test]
fn focused_runtime_textarea_ime_updates_multiline_state() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("第一行\n"))
        .unwrap();
    assert!(context.focus_node(document, area.stable_id()).unwrap());

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Update {
                    text: "第二".into(),
                    selection: Some((0, "第".len())),
                },
            )
            .unwrap()
            .prevent_default
    );
    let composition = context
        .world()
        .ime(area.stable_id())
        .expect("focused textarea keeps preedit on retained state");
    assert_eq!(composition.text, "第二");
    assert_eq!(composition.selection, Some((0, "第".len())));
    assert_eq!(context.world().text(area.stable_id()), Some("第一行\n"));

    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Commit("第二行".into())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().text(area.stable_id()),
        Some("第一行\n第二行")
    );
    assert_eq!(context.world().ime(area.stable_id()), None);

    context
        .update_component(area, |area, _cx| area.disabled = true)
        .unwrap();
    assert!(
        !adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Update {
                    text: "三".into(),
                    selection: None,
                },
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().text(area.stable_id()),
        Some("第一行\n第二行")
    );
    assert_eq!(context.world().ime(area.stable_id()), None);
}

#[test]
fn dispatch_ime_commits_a_focused_text_input_without_a_typed_view() {
    let mut context = AppContext::new();
    let (document, id) = focused_untyped_text_input(&mut context, "Nana");
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Update {
                    text: "世".into(),
                    selection: Some((0, "世".len())),
                },
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().ime(id).map(|ime| ime.text), Some("世"));
    assert_eq!(
        context.world().text_input(id).map(|state| state.value),
        Some("Nana")
    );

    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Commit("世界".into())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().text_input(id).map(|state| state.value),
        Some("Nana世界")
    );
    assert!(context.world().ime(id).is_none());
}

#[test]
fn dispatch_ime_disabled_commits_leftover_preedit_without_a_typed_view() {
    let mut context = AppContext::new();
    let (document, id) = focused_untyped_text_input(&mut context, "Nana");
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Update {
                    text: "世".into(),
                    selection: Some((0, "世".len())),
                },
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch_ime(&mut context, document, &CompositionInput::Disabled)
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().text_input(id).map(|state| state.value),
        Some("Nana世")
    );
    assert!(context.world().ime(id).is_none());
}

#[test]
fn dispatch_ime_cancelled_discards_leftover_preedit_without_commit() {
    let mut context = AppContext::new();
    let (document, id) = focused_untyped_text_input(&mut context, "Nana");
    let mut adapter = TestInput::default();
    adapter
        .dispatch_ime(
            &mut context,
            document,
            &CompositionInput::Update {
                text: "世".into(),
                selection: Some((0, "世".len())),
            },
        )
        .unwrap();
    assert!(
        adapter
            .dispatch_ime(&mut context, document, &CompositionInput::End)
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().text_input(id).map(|state| state.value),
        Some("Nana")
    );
    assert!(context.world().ime(id).is_none());
}

#[test]
fn dispatch_ime_deletes_surrounding_committed_text_and_skips_invalid_spans() {
    let mut context = AppContext::new();
    let (document, id) = focused_untyped_text_input(&mut context, "你好");
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Update {
                    text: "世".into(),
                    selection: Some((0, "世".len())),
                },
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::DeleteSurrounding {
                    before_bytes: "好".len(),
                    after_bytes: 0,
                },
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().text_input(id).map(|state| state.value),
        Some("你")
    );
    assert_eq!(
        context.world().ime(id).map(|ime| ime.text),
        Some("世"),
        "delete surrounding must not clear preedit"
    );

    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::DeleteSurrounding {
                    before_bytes: 1,
                    after_bytes: 0,
                },
            )
            .unwrap()
            .prevent_default,
        "focused editable still consumes an un-applicable span"
    );
    assert_eq!(
        context.world().text_input(id).map(|state| state.value),
        Some("你"),
        "invalid byte span must leave committed text unchanged"
    );
}

#[test]
fn wheel_routes_to_nearest_scrollview_and_bubbles_at_edge() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let outer = context
        .create_component(document, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    let inner = context
        .create_component(document, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    let cell = context
        .create_component(document, TableCell::new("row"))
        .unwrap();
    // Beside the inner scrollport, content reaching 300 down the outer.
    let tail = context
        .create_component(document, TableCell::new("tail"))
        .unwrap();
    context.append_child(outer, inner).unwrap();
    context.append_child(outer, tail).unwrap();
    context.append_child(inner, cell).unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        outer.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        },
    );
    layout.write_layout(
        inner.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 180.0,
            height: 80.0,
        },
    );
    // 140 of content in the inner's 80: it scrolls 60, the outer 200.
    layout.write_layout(
        cell.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 140.0,
        },
    );
    layout.write_layout(
        tail.stable_id(),
        LayoutBox {
            x: 190.0,
            y: 0.0,
            width: 10.0,
            height: 300.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(&mut context, document, &wheel(10.0, 10.0, -1.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().scroll_offset(inner.stable_id()).unwrap().y,
        60.0
    );
    assert_eq!(
        context.world().scroll_offset(outer.stable_id()).unwrap().y,
        0.0
    );
    context.take_system_work();
    context.rebuild_hit_test(document);

    assert!(
        adapter
            .dispatch(&mut context, document, &wheel(10.0, 10.0, -1.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.world().scroll_offset(inner.stable_id()).unwrap().y,
        60.0
    );
    assert_eq!(
        context.world().scroll_offset(outer.stable_id()).unwrap().y,
        60.0
    );
}

#[test]
fn wheel_on_overflow_auto_updates_scroll_offset() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroller = crate::StableNodeId::new(1).unwrap();
    let child = crate::StableNodeId::new(2).unwrap();
    let mut create = MutationQueue::new();
    create.create(scroller, document, NodeKind::Element { tag: "div".into() });
    create.create(child, document, NodeKind::Element { tag: "item".into() });
    create.insert(scroller, child, None);
    create.set_style(
        scroller,
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_y: OverflowSpec::Auto,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    create.write_layout(
        scroller,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        },
    );
    create.write_layout(
        child,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 300.0,
        },
    );
    context.commit_mutations(create).unwrap();
    let work = context.take_system_work();
    context.resolve_styles(&work.style).unwrap();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(&mut context, document, &wheel(10.0, 10.0, -1.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().scroll_offset(scroller).unwrap().y, 60.0);
    assert_eq!(context.world().scroll_offset(child).unwrap().y, 0.0);
    assert!(
        context
            .world()
            .node_style(scroller)
            .unwrap()
            .layout
            .overflow_y
            .scrolls()
    );
    assert!(
        !context.is_scroll_view(scroller),
        "L1 overflow must not stamp a ScrollView"
    );
}

#[test]
fn keyboard_routes_navigation_from_focused_table_cell() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let table = context.create_component(document, Table::new()).unwrap();
    let first_row = context.create_component(document, TableRow::new()).unwrap();
    let second_row = context.create_component(document, TableRow::new()).unwrap();
    let first = context
        .create_component(document, TableCell::new("one"))
        .unwrap();
    let second = context
        .create_component(document, TableCell::new("two"))
        .unwrap();
    context.append_child(table, first_row).unwrap();
    context.append_child(table, second_row).unwrap();
    context.append_child(first_row, first).unwrap();
    context.append_child(second_row, second).unwrap();
    let mut focus = MutationQueue::new();
    focus.request_focus(document, Some(first.stable_id()));
    context.commit_mutations(focus).unwrap();

    let event = key_fixture! {
        pressed: true,
        key: "ArrowDown".into(),
        text: None,
        code: "ArrowDown".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    };
    assert!(
        TestInput::default()
            .dispatch(&mut context, document, &event)
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().focused(document), Some(second.stable_id()));
}

#[test]
fn segmented_keyboard_routing_precedes_generic_navigation_and_activation() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let control = context
        .create_component(document, SegmentedControl::new())
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let disabled = context
        .create_detached_component(document, SegmentedOption::new("Split").disabled(true))
        .unwrap();
    let last = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    context
        .set_segmented_options(control, vec![first, disabled, last], Some(first))
        .unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&requests);
    context
        .on(
            control,
            move |_control, event: &SegmentedSelectionRequested, _cx| {
                observed.lock().unwrap().push(event.option);
            },
        )
        .unwrap();
    context.focus_node(document, first.stable_id()).unwrap();
    let key = |key: &str, repeat: bool, modifiers: InputModifiers| {
        key_fixture! {
            pressed: true,
            key: key.into(),
            text: None,
            code: key.into(),
            repeat,
            modifiers,
        }
    };
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &key("ArrowRight", false, InputModifiers::default()),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().focused(document), Some(last.stable_id()));
    assert_eq!(
        context.read(control, SegmentedControl::selected).unwrap(),
        Some(first.stable_id())
    );
    assert_eq!(&*requests.lock().unwrap(), &[last.stable_id()]);
    for modifiers in [
        InputModifiers {
            alt: true,
            ..InputModifiers::default()
        },
        InputModifiers {
            control: true,
            ..InputModifiers::default()
        },
        InputModifiers {
            shift: true,
            ..InputModifiers::default()
        },
        InputModifiers {
            meta: true,
            ..InputModifiers::default()
        },
    ] {
        assert!(
            !adapter
                .dispatch(&mut context, document, &key("Home", true, modifiers))
                .unwrap()
                .prevent_default
        );
    }
    assert_eq!(context.world().focused(document), Some(last.stable_id()));
    assert_eq!(requests.lock().unwrap().as_slice(), [last.stable_id()]);
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &key("Home", true, InputModifiers::default()),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().focused(document), Some(first.stable_id()));
    assert_eq!(
        requests.lock().unwrap().as_slice(),
        [last.stable_id(), first.stable_id()]
    );
    let count = requests.lock().unwrap().len();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &key("Space", true, InputModifiers::default()),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(requests.lock().unwrap().len(), count);
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &key("Enter", false, InputModifiers::default()),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(requests.lock().unwrap().last(), Some(&first.stable_id()));
    assert_eq!(
        context.read(control, SegmentedControl::selected).unwrap(),
        Some(first.stable_id())
    );
    assert!(
        context
            .set_segmented_selection(control, Some(last))
            .unwrap()
    );
    assert_eq!(
        context.read(control, SegmentedControl::selected).unwrap(),
        Some(last.stable_id())
    );
}

#[test]
fn range_field_quantizes_keyboard_and_cancels_captured_drag() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let range = context
        .create_component(document, RangeField::new(0.5, 0.0, 1.0, 0.1))
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        range.stable_id(),
        LayoutBox {
            x: 10.0,
            y: 10.0,
            width: 300.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.rebuild_hit_test(document);
    assert!(context.focus_node(document, range.stable_id()).unwrap());

    let key = |key: &str| {
        key_fixture! {
            pressed: true,
            key: key.into(),
            text: None,
            code: key.into(),
            repeat: false,
            modifiers: InputModifiers::default(),
        }
    };
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(&mut context, document, &key("ArrowRight"))
            .unwrap()
            .prevent_default
    );
    assert!((context.read(range, |range| range.value).unwrap() - 0.6).abs() < 1e-12);
    assert!(
        adapter
            .dispatch(&mut context, document, &key("Home"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.read(range, |range| range.value).unwrap(), 0.0);

    let track = match context.world().component_geometry(range.stable_id()) {
        Some(crate::ComponentGeometry::Range { track, .. }) => track,
        _ => panic!("range geometry expected"),
    };
    let drag_x = track.x + track.width * 0.8;
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, drag_x, 20.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.read(range, |range| range.value).unwrap(), 0.8);
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Cancel, drag_x, 20.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.read(range, |range| range.value).unwrap(), 0.0);
    assert_eq!(context.world().pointer_capture(document, 1), None);
}

#[test]
fn a_pointer_drag_on_a_range_commits_once_on_release() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let range = context
        .create_component(document, RangeField::new(0.0, 0.0, 1.0, 0.1))
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        range.stable_id(),
        LayoutBox {
            x: 10.0,
            y: 10.0,
            width: 300.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.rebuild_hit_test(document);
    let previews = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&previews);
    context
        .on(range, move |_, event: &crate::RangeInput, _| {
            observed.lock().unwrap().push(event.value);
        })
        .unwrap();
    let commits = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&commits);
    context
        .on(range, move |_, event: &crate::RangeChanged, _| {
            observed.lock().unwrap().push(event.value);
        })
        .unwrap();
    let track = match context.world().component_geometry(range.stable_id()) {
        Some(ComponentGeometry::Range { track, .. }) => track,
        _ => panic!("range geometry expected"),
    };
    let mut adapter = TestInput::default();
    for (phase, fraction) in [
        (PointerPhase::Down, 0.2),
        (PointerPhase::Move, 0.5),
        (PointerPhase::Move, 0.7),
        (PointerPhase::Up, 0.7),
    ] {
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(phase, track.x + track.width * fraction, 20.0),
            )
            .unwrap();
    }
    assert_eq!(previews.lock().unwrap().len(), 3);
    let commits = commits.lock().unwrap();
    assert_eq!(commits.len(), 1, "one commit per drag: {commits:?}");
    assert!((commits[0] - 0.7).abs() < 1e-9);
    assert_eq!(context.world().pointer_capture(document, 1), None);
}

#[test]
fn overlay_pointer_sequence_never_activates_the_underlay() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let underlay = context
        .create_component(document, Button::new("Underlay"))
        .unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let dialog = context
        .create_component(document, Dialog::new("Dialog"))
        .unwrap();
    context.append_child(host, dialog).unwrap();
    let activations = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&activations);
    context
        .on(underlay, move |_button, _event: &Activate, _cx| {
            *observed.lock().unwrap() += 1;
        })
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        underlay.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 300.0,
        },
    );
    layout.write_layout(
        dialog.stable_id(),
        LayoutBox {
            x: 100.0,
            y: 100.0,
            width: 100.0,
            height: 100.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.activate_overlay(host, dialog).unwrap();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 20.0, 20.0),
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Up, 20.0, 20.0),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(*activations.lock().unwrap(), 0);
}

/// Lays a popover over a button so the two never overlap, and reports the
/// shared activation counter of the button.
fn popover_over_button(
    context: &mut AppContext,
    document: DocumentId,
) -> (Entity<ActionMenu>, Arc<Mutex<u32>>) {
    let underlay = context
        .create_component(document, Button::new("Underlay"))
        .unwrap();
    let menu = context
        .create_component(document, ActionMenu::new().open(true))
        .unwrap();
    let activations = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&activations);
    context
        .on(underlay, move |_button, _event: &Activate, _cx| {
            *observed.lock().unwrap() += 1;
        })
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        underlay.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 60.0,
        },
    );
    layout.write_layout(
        menu.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 100.0,
            width: 200.0,
            height: 100.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.rebuild_hit_test(document);
    (menu, activations)
}

#[test]
fn outside_press_closes_the_popover_without_reaching_the_underlay() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (menu, activations) = popover_over_button(&mut context, document);
    let mut adapter = TestInput::default();

    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 20.0, 20.0),
            )
            .unwrap()
            .prevent_default
    );
    adapter
        .dispatch(
            &mut context,
            document,
            &pointer(PointerPhase::Up, 20.0, 20.0),
        )
        .unwrap();

    assert!(!context.read(menu, |menu| menu.popover.open).unwrap());
    // An app-owned trigger button sits outside the popover too, so letting
    // this press through would toggle the menu straight back open.
    assert_eq!(*activations.lock().unwrap(), 0);
}

#[test]
fn press_inside_the_popover_leaves_it_open() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (menu, _) = popover_over_button(&mut context, document);
    let item = context
        .create_component(document, ActionMenuItem::new("Rename"))
        .unwrap();
    context.append_child(menu, item).unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        item.stable_id(),
        LayoutBox {
            x: 4.0,
            y: 104.0,
            width: 192.0,
            height: 28.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.rebuild_hit_test(document);
    let mut adapter = TestInput::default();

    adapter
        .dispatch(
            &mut context,
            document,
            &pointer(PointerPhase::Down, 20.0, 110.0),
        )
        .unwrap();

    assert!(context.read(menu, |menu| menu.popover.open).unwrap());
}

/// A press on the open surface around a menu's items, where the surface
/// hangs out of a clipping pane over a later, higher sibling: the menu keeps
/// it (it stays open, it is not toggled shut) and the sibling under the
/// surface never sees it.
#[test]
fn a_press_on_the_hanging_surface_stays_in_the_menu() {
    use nana_ui_core::{LengthSpec, PositionSpec};
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let row = context
        .create_component(document, crate::Stack::fill_row(0.0))
        .unwrap();
    let pane = context
        .create_component(
            document,
            crate::Stack::column(0.0).with_layout(|layout| {
                layout.width = Some(LengthSpec::Px(200.0));
                layout.height = Some(LengthSpec::Px(480.0));
                layout.overflow_x = OverflowSpec::Hidden;
                layout.overflow_y = OverflowSpec::Hidden;
                layout.position = PositionSpec::Relative;
                layout.z_index = Some(1);
            }),
        )
        .unwrap();
    let underlay = context
        .create_component(
            document,
            Button::new("Underlay").layout(Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(440.0)),
                height: Some(LengthSpec::Px(480.0)),
                position: PositionSpec::Relative,
                z_index: Some(2),
                ..LayoutStyle::default()
            })),
        )
        .unwrap();
    let menu = context
        .create_component(document, ActionMenu::new().trigger("更多").width(280.0))
        .unwrap();
    let item = context
        .create_component(document, ActionMenuItem::new("稍后再看"))
        .unwrap();
    context.append_child(menu, item).unwrap();
    context.append_child(pane, menu).unwrap();
    context.append_child(row, pane).unwrap();
    context.append_child(row, underlay).unwrap();
    let activations = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&activations);
    context
        .on(underlay, move |_button, _event: &Activate, _cx| {
            *observed.lock().unwrap() += 1;
        })
        .unwrap();
    let viewport = crate::LayoutViewport::new(640.0, 480.0);
    context.layout_document(document, viewport).unwrap();
    context
        .update_component(menu, |menu, _| menu.popover.open = true)
        .unwrap();
    context.layout_document(document, viewport).unwrap();
    context.rebuild_hit_test(document);

    let item_box = context.world().layout_box(item.stable_id()).unwrap();
    let Some(ComponentGeometry::MenuSurface { surface, .. }) =
        context.world().component_geometry(menu.stable_id())
    else {
        panic!("the open menu's geometry");
    };
    let (x, y) = (
        (item_box.x + item_box.width + surface.x + surface.width) / 2.0,
        item_box.y + item_box.height / 2.0,
    );
    assert!(x > 200.0, "the press lands over the underlay: {surface:?}");
    let mut adapter = TestInput::default();
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        adapter
            .dispatch(&mut context, document, &pointer(phase, x, y))
            .unwrap();
    }
    assert!(context.read(menu, |menu| menu.popover.open).unwrap());
    assert_eq!(*activations.lock().unwrap(), 0);
}

#[test]
fn escape_closes_focused_field_options_without_committing() {
    use crate::{
        Dropdown, DropdownOption, SearchDropdown, SearchDropdownEvent, SearchDropdownOption,
        Select, SelectOption,
    };
    use nana_ui_core::DropdownEvent;

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let select = context
        .create_component(
            document,
            Select::new(Some("a"))
                .options([
                    SelectOption::new("a", "Alpha"),
                    SelectOption::new("b", "Beta"),
                ])
                .opened(true),
        )
        .unwrap();
    let dropdown = context
        .create_component(
            document,
            Dropdown::single(Some("a"))
                .options([
                    DropdownOption::new("a", "Alpha"),
                    DropdownOption::new("b", "Beta"),
                ])
                .opened(true),
        )
        .unwrap();
    let search = context
        .create_component(
            document,
            SearchDropdown::new(Some("a"))
                .options([
                    SearchDropdownOption::new("a", "Alpha"),
                    SearchDropdownOption::new("b", "Beta"),
                ])
                .query("Beta")
                .opened(true),
        )
        .unwrap();
    let dropdown_events = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::clone(&dropdown_events);
    context
        .on(dropdown, move |_, event: &DropdownEvent<Arc<str>>, _| {
            events.lock().unwrap().push(event.clone());
        })
        .unwrap();
    let search_events = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::clone(&search_events);
    context
        .on(search, move |_, event: &SearchDropdownEvent, _| {
            events.lock().unwrap().push(event.clone());
        })
        .unwrap();
    context
        .update_component(select, |field, _| field.highlighted = Some(1))
        .unwrap();
    context
        .update_component(dropdown, |field, _| field.highlighted = Some(1))
        .unwrap();
    let selection = context
        .read(dropdown, |field| field.selection.clone())
        .unwrap();
    let search_state = context.read(search, |field| field.state.clone()).unwrap();
    let mut adapter = TestInput::default();
    let escape = key_fixture! {
        pressed: true,
        key: "Escape".into(),
        text: None,
        code: "Escape".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    };
    for target in [select.stable_id(), dropdown.stable_id(), search.stable_id()] {
        assert!(context.focus_node(document, target).unwrap());
        assert!(
            adapter
                .dispatch(&mut context, document, &escape)
                .unwrap()
                .prevent_default
        );
        assert!(
            !adapter
                .dispatch(&mut context, document, &escape)
                .unwrap()
                .prevent_default
        );
    }
    assert_eq!(
        context
            .read(select, |field| (field.opened, field.value.clone()))
            .unwrap(),
        (false, Some(Arc::from("a")))
    );
    assert_eq!(
        context
            .read(dropdown, |field| (field.opened, field.selection.clone()))
            .unwrap(),
        (false, selection)
    );
    assert_eq!(
        context
            .read(search, |field| (
                field.opened,
                field.value.clone(),
                field.query.clone(),
                field.state.clone()
            ))
            .unwrap(),
        (false, Some(Arc::from("a")), "Beta".into(), search_state)
    );
    assert_eq!(
        *dropdown_events.lock().unwrap(),
        vec![DropdownEvent::Closed]
    );
    assert_eq!(
        *search_events.lock().unwrap(),
        vec![SearchDropdownEvent::Closed]
    );
}

#[test]
fn escape_closes_the_popover() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (menu, _) = popover_over_button(&mut context, document);
    let mut adapter = TestInput::default();
    let escape = key_fixture! {
        pressed: true,
        key: "Escape".into(),
        text: None,
        code: "Escape".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    };

    assert!(
        adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
    assert!(!context.read(menu, |menu| menu.popover.open).unwrap());
    // The next Escape belongs to the application navigation layer. A
    // host must pass the per-event result rather than cache overlay state.
    assert!(
        !adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
}

#[test]
fn a_popover_that_opts_out_ignores_outside_presses_and_escape() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (menu, _) = popover_over_button(&mut context, document);
    context
        .update_component(menu, |menu, _| {
            menu.popover.close_on_outside = false;
            menu.popover.close_on_escape = false;
        })
        .unwrap();
    let mut adapter = TestInput::default();

    adapter
        .dispatch(
            &mut context,
            document,
            &pointer(PointerPhase::Down, 20.0, 20.0),
        )
        .unwrap();
    adapter
        .dispatch(
            &mut context,
            document,
            &key_fixture! {
                pressed: true,
                key: "Escape".into(),
                text: None,
                code: "Escape".into(),
                repeat: false,
                modifiers: InputModifiers::default(),
            },
        )
        .unwrap();

    assert!(context.read(menu, |menu| menu.popover.open).unwrap());
}

#[test]
fn menu_item_can_close_during_activation_without_releasing_input_or_wheel_barrier() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let background = context
        .create_component(document, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let menu = context
        .create_component(document, ActionMenu::new().open(true))
        .unwrap();
    let item = context
        .create_component(document, ActionMenuItem::new("Build"))
        .unwrap();
    context.append_child(background, host).unwrap();
    context.append_child(host, menu).unwrap();
    context.append_child(menu, item).unwrap();
    let host_id = host.stable_id();
    context
        .on(item, move |_item, _event: &Activate, cx| {
            cx.mutations()
                .set_overlay_host(host_id, OverlayHostState::default());
        })
        .unwrap();
    let mut layout = MutationQueue::new();
    for (id, x, y, width, height) in [
        (background.stable_id(), 0.0, 0.0, 300.0, 300.0),
        (menu.stable_id(), 100.0, 100.0, 100.0, 100.0),
        (item.stable_id(), 110.0, 110.0, 80.0, 32.0),
    ] {
        layout.write_layout(
            id,
            LayoutBox {
                x,
                y,
                width,
                height,
            },
        );
    }
    context.commit_mutations(layout).unwrap();
    context
        .set_scroll_metrics(
            background,
            ScrollMetrics {
                viewport_width: 300.0,
                viewport_height: 300.0,
                content_width: 300.0,
                content_height: 900.0,
                origin_x: 0.0,
                origin_y: 0.0,
            },
        )
        .unwrap();
    context.activate_overlay(host, menu).unwrap();
    let work = context.take_system_work();
    context.resolve_styles(&work.style).unwrap();
    context.rebuild_hit_test(document);
    let mut adapter = TestInput::default();

    assert!(
        adapter
            .dispatch(&mut context, document, &wheel(20.0, 20.0, -1.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context
            .world()
            .scroll_offset(background.stable_id())
            .unwrap()
            .y,
        0.0
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 120.0, 120.0),
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Up, 120.0, 120.0),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context
            .world()
            .overlay_host(host.stable_id())
            .unwrap()
            .active,
        None
    );
}

#[test]
fn overlay_keyboard_ignores_primary_tab_and_repeated_escape() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let dialog = context
        .create_component(document, Dialog::new("Settings"))
        .unwrap();
    let button = context
        .create_detached_component(document, Button::new("Save"))
        .unwrap();
    context.append_child(host, dialog).unwrap();
    context
        .set_modal_slots(
            dialog,
            ModalSlots {
                actions: vec![button.stable_id()],
                ..ModalSlots::default()
            },
        )
        .unwrap();
    context.activate_overlay(host, dialog).unwrap();
    let mut adapter = TestInput::default();
    let key = |key: &str, repeat: bool, modifiers: InputModifiers| {
        key_fixture! {
            pressed: true,
            key: key.into(),
            text: None,
            code: key.into(),
            repeat,
            modifiers,
        }
    };

    let primary_tab = key(
        "Tab",
        false,
        InputModifiers {
            control: true,
            ..InputModifiers::default()
        },
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &primary_tab)
            .unwrap()
            .prevent_default
    );
    let repeat_escape = key("Escape", true, InputModifiers::default());
    assert!(
        adapter
            .dispatch(&mut context, document, &repeat_escape)
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context
            .world()
            .overlay_host(host.stable_id())
            .unwrap()
            .active,
        Some(dialog.stable_id())
    );
    let key_release = key_fixture! {
        pressed: false,
        key: "a".into(),
        text: None,
        code: "KeyA".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    };
    assert!(
        adapter
            .dispatch(&mut context, document, &key_release)
            .unwrap()
            .prevent_default
    );
    let escape = key("Escape", false, InputModifiers::default());
    assert!(
        adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
    assert!(context.active_runtime_overlay(document).is_none());
    context.advance_animations(std::time::Duration::from_secs(1));
    assert_eq!(
        context
            .world()
            .overlay_host(host.stable_id())
            .unwrap()
            .active,
        None
    );
    assert!(
        !adapter
            .dispatch(&mut context, document, &primary_tab)
            .unwrap()
            .prevent_default
    );
}

#[test]
fn pointer_on_dock_handle_changes_split_ratio() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let first = context
        .create_component(document, Text::new("first"))
        .unwrap()
        .stable_id();
    let second = context
        .create_component(document, Text::new("second"))
        .unwrap()
        .stable_id();
    let dock = context
        .create_component(
            document,
            Dock::new(DockNode::split(
                DockAxis::Horizontal,
                0.4,
                DockNode::item("inspector", Some(first)),
                DockNode::item("console", Some(second)),
            )),
        )
        .unwrap();
    context.assemble_dock(dock).unwrap();
    let handle = context.world().node(dock.stable_id()).unwrap().children[1];
    let mut layout = MutationQueue::new();
    layout.write_layout(
        dock.stable_id(),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 200.0,
        },
    );
    layout.write_layout(
        handle,
        LayoutBox {
            x: 156.8,
            y: 0.0,
            width: 8.0,
            height: 200.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.rebuild_hit_test(document);

    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Down, 160.0, 20.0)
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Move, 200.0, 20.0)
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Up, 200.0, 20.0)
            )
            .unwrap()
            .prevent_default
    );
    let ratio = context
        .read(dock, |dock| match &dock.root {
            DockNode::Split { ratio, .. } => *ratio,
            _ => panic!("split"),
        })
        .unwrap();
    assert!((ratio - (0.4_f32 + 40.0 / 392.0).clamp(0.05, 0.95)).abs() < 0.001);
}

#[test]
fn keyboard_arrow_right_on_focused_dock_handle_changes_ratio() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let first = context
        .create_component(document, Text::new("first"))
        .unwrap()
        .stable_id();
    let second = context
        .create_component(document, Text::new("second"))
        .unwrap()
        .stable_id();
    let dock = context
        .create_component(
            document,
            Dock::new(DockNode::split(
                DockAxis::Horizontal,
                0.4,
                DockNode::item("inspector", Some(first)),
                DockNode::item("console", Some(second)),
            )),
        )
        .unwrap();
    context.assemble_dock(dock).unwrap();
    let handle = context.world().node(dock.stable_id()).unwrap().children[1];
    assert!(context.focus_node(document, handle).unwrap());

    let event = key_fixture! {
        pressed: true,
        key: "ArrowRight".into(),
        text: None,
        code: "ArrowRight".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    };
    assert!(
        TestInput::default()
            .dispatch(&mut context, document, &event)
            .unwrap()
            .prevent_default
    );
    let ratio = context
        .read(dock, |dock| match &dock.root {
            DockNode::Split { ratio, .. } => *ratio,
            _ => panic!("split"),
        })
        .unwrap();
    assert!((ratio - 0.45).abs() < 0.001);
}

#[test]
#[cfg(feature = "calendar")]
fn pointer_on_calendar_heatmap_sets_active_cell() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let heatmap = context
        .create_component(
            document,
            CalendarHeatmap::new([
                CalendarHeatmapDatum::<()>::new("2026-06-01", 2.0),
                CalendarHeatmapDatum::<()>::new("2026-06-03", 8.0),
            ]),
        )
        .unwrap();
    let model = context.read(heatmap, CalendarHeatmap::model).unwrap();
    let cell = model
        .cells
        .iter()
        .find(|cell| cell.date == "2026-06-03")
        .expect("June 3");
    context
        .commit_mutations({
            let mut mutations = MutationQueue::new();
            mutations.write_layout(
                heatmap.stable_id(),
                LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: model.width,
                    height: model.height,
                },
            );
            mutations
        })
        .unwrap();
    context.rebuild_hit_test(document);

    assert!(
        TestInput::default()
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Move, cell.x + 1.0, cell.y + 1.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.read(heatmap, |calendar| calendar.active).unwrap(),
        Some(
            model
                .cells
                .iter()
                .position(|item| item.date == "2026-06-03")
                .expect("index")
        )
    );
    assert!(
        TestInput::default()
            .dispatch(
                &mut context,
                document,
                &pointer(PointerPhase::Move, 400.0, 400.0)
            )
            .unwrap()
            .prevent_default
    );
    assert!(
        context
            .read(heatmap, |calendar| calendar.active)
            .unwrap()
            .is_none()
    );
}

fn edit_key(key: &str, text: Option<&str>, modifiers: InputModifiers) -> Gesture {
    key_fixture! {
        pressed: true,
        key: key.into(),
        text: text.map(str::to_string),
        code: key.into(),
        repeat: false,
        modifiers,
    }
}

fn plain_key(key: &str) -> Gesture {
    edit_key(key, None, InputModifiers::default())
}

fn shift_key(key: &str) -> Gesture {
    edit_key(
        key,
        None,
        InputModifiers {
            shift: true,
            ..InputModifiers::default()
        },
    )
}

fn meta_key(key: &str) -> Gesture {
    edit_key(
        key,
        None,
        InputModifiers {
            meta: true,
            ..InputModifiers::default()
        },
    )
}

fn textarea_selection(context: &AppContext, node: StableNodeId) -> (String, usize, usize) {
    let state = context.world().text_input(node).unwrap();
    (
        state.value.to_owned(),
        state.selection.anchor,
        state.selection.focus,
    )
}

#[test]
fn a_focused_number_input_edits_its_draft_and_keeps_its_stepper_keys() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, crate::NumberInput::new(1.0).range(0.0, 100.0))
        .unwrap();
    let node = input.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let value = |context: &AppContext| context.read(input, crate::NumberInput::value).unwrap();
    let control = |key: &str, shift: bool| {
        edit_key(
            key,
            None,
            InputModifiers {
                control: true,
                shift,
                ..InputModifiers::default()
            },
        )
    };

    // ArrowUp steps the value; it is not a caret move.
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowUp"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(value(&context), 2.0);
    assert_eq!(textarea_selection(&context, node), ("2".into(), 1, 1));

    // Left moves the caret inside the draft and typing lands there.
    adapter
        .dispatch(&mut context, document, &plain_key("ArrowLeft"))
        .unwrap();
    assert_eq!(textarea_selection(&context, node), ("2".into(), 0, 0));
    adapter
        .dispatch(
            &mut context,
            document,
            &edit_key("1", Some("1"), InputModifiers::default()),
        )
        .unwrap();
    assert_eq!(textarea_selection(&context, node), ("12".into(), 1, 1));

    // Shift+ArrowUp selects to the start like any single-line field.
    assert!(
        adapter
            .dispatch(&mut context, document, &shift_key("ArrowUp"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("12".into(), 1, 0));
    assert_eq!(value(&context), 2.0, "a selecting key does not step");
    adapter
        .dispatch(&mut context, document, &plain_key("ArrowRight"))
        .unwrap();

    // Ctrl+Z / Ctrl+Shift+Z walk the draft's history.
    adapter
        .dispatch(&mut context, document, &control("z", false))
        .unwrap();
    assert_eq!(textarea_selection(&context, node).0, "2");
    adapter
        .dispatch(&mut context, document, &control("z", true))
        .unwrap();
    assert_eq!(textarea_selection(&context, node).0, "12");
    assert_eq!(value(&context), 2.0, "typing committed no number to redo");

    // Enter commits a pending draft instead of submitting a text field.
    adapter
        .dispatch(
            &mut context,
            document,
            &edit_key("Delete", None, InputModifiers::default()),
        )
        .unwrap();
    assert_eq!(textarea_selection(&context, node).0, "1");
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Enter"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(value(&context), 1.0);

    // At its bound the field still owns ArrowUp: it does not fall through
    // to routing that could carry focus out of it. The same holds while
    // an IME composition hides the editor.
    context.set_number_value(input, 100.0).unwrap();
    for composing in [false, true] {
        if composing {
            context
                .set_ime_preedit(document, "ｘ".into(), None)
                .unwrap();
        }
        assert!(
            adapter
                .dispatch(&mut context, document, &plain_key("ArrowUp"))
                .unwrap()
                .prevent_default,
            "composing: {composing}"
        );
        assert_eq!(value(&context), 100.0);
        assert_eq!(context.world().focused(document), Some(node));
    }
    context.clear_ime(document).unwrap();

    // Alt+Enter commits like Enter; it is not swallowed as a text
    // field's submit.
    adapter
        .dispatch(
            &mut context,
            document,
            &edit_key("0", Some("0"), InputModifiers::default()),
        )
        .unwrap();
    adapter
        .dispatch(
            &mut context,
            document,
            &edit_key(
                "Enter",
                None,
                InputModifiers {
                    alt: true,
                    ..InputModifiers::default()
                },
            ),
        )
        .unwrap();
    assert_eq!(value(&context), 100.0, "1000 clamps to the maximum");
    assert_eq!(textarea_selection(&context, node).0, "100");

    // An Enter with nothing to commit is not swallowed: a dialog or form
    // around the field can still confirm on it. The control text hosts
    // report with Enter and Escape never lands in the draft.
    for (key, text) in [("Enter", "\r"), ("Escape", "\u{1b}")] {
        assert!(
            !adapter
                .dispatch(
                    &mut context,
                    document,
                    &edit_key(key, Some(text), InputModifiers::default()),
                )
                .unwrap()
                .prevent_default,
            "{key}"
        );
        assert_eq!(textarea_selection(&context, node).0, "100", "{key}");
    }
}

#[test]
fn a_composing_text_input_keeps_its_navigation_keys() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("abc"))
        .unwrap();
    let node = input.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    context
        .set_ime_preedit(document, "に".into(), None)
        .unwrap();
    let mut adapter = TestInput::default();
    for key in ["ArrowUp", "ArrowDown", "ArrowLeft", "Home"] {
        assert!(
            adapter
                .dispatch(&mut context, document, &plain_key(key))
                .unwrap()
                .prevent_default,
            "{key}"
        );
        assert_eq!(context.world().focused(document), Some(node));
        assert_eq!(textarea_selection(&context, node), ("abc".into(), 3, 3));
    }
}

#[test]
fn a_composing_search_dropdown_keeps_its_list_navigation() {
    use crate::{SearchDropdown, SearchDropdownOption};
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let search = context
        .create_component(
            document,
            SearchDropdown::new(None::<&str>)
                .options([
                    SearchDropdownOption::new("a", "Alpha"),
                    SearchDropdownOption::new("b", "Beta"),
                ])
                .opened(true),
        )
        .unwrap();
    assert!(context.focus_node(document, search.stable_id()).unwrap());
    context
        .set_ime_preedit(document, "に".into(), None)
        .unwrap();
    let before = context.read(search, |field| field.highlighted).unwrap();
    TestInput::default()
        .dispatch(&mut context, document, &plain_key("ArrowDown"))
        .unwrap();
    assert_ne!(
        context.read(search, |field| field.highlighted).unwrap(),
        before,
        "a composite surface is not a plain editor: its list still moves"
    );
}

#[test]
fn arrow_keys_move_and_extend_the_focused_textarea_caret() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("abcdef"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();

    // The caret starts at the value end; Left steps back one grapheme.
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowLeft"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("abcdef".into(), 5, 5));
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Home"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("abcdef".into(), 0, 0));
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("End"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("abcdef".into(), 6, 6));

    // Shift+Left extends the selection; typing replaces it.
    assert!(
        adapter
            .dispatch(&mut context, document, &shift_key("ArrowLeft"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &shift_key("ArrowLeft"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("abcdef".into(), 6, 4));
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("x", Some("X"), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("abcdX".into(), 5, 5));
}

#[test]
fn vertical_arrows_fall_back_to_logical_lines_without_a_shaper() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("abc\ndefg\nhi"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();

    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowLeft"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowLeft"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndefg\nhi".into(), 9, 9)
    );

    // Up keeps the grapheme column: column 0 lands on the line start.
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowUp"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndefg\nhi".into(), 4, 4)
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowDown"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndefg\nhi".into(), 9, 9)
    );

    // Cmd+Up / Cmd+Down jump to the document edges.
    assert!(
        adapter
            .dispatch(&mut context, document, &meta_key("ArrowUp"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndefg\nhi".into(), 0, 0)
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &meta_key("ArrowDown"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndefg\nhi".into(), 11, 11)
    );
}

#[test]
fn delete_keys_remove_selections_words_and_line_spans() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("one two three"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();

    // Forward delete at the caret end is a no-op that still stays owned.
    let word_modifier = InputModifiers {
        alt: true,
        ..InputModifiers::default()
    };
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("Backspace", None, word_modifier)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "one two ");

    let meta = InputModifiers {
        meta: true,
        ..InputModifiers::default()
    };
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("Backspace", None, meta))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "");

    // Forward delete at the value end declines; typing still works.
    assert!(
        !adapter
            .dispatch(&mut context, document, &plain_key("Delete"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("h", Some("h"), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "h");
}

#[test]
fn code_editor_newline_copies_indent_and_completes_pairs() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("fn a() {\n  x").code_editor(true))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();

    // Enter after indented content copies the indentation.
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Enter"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("fn a() {\n  x\n  ".into(), 15, 15)
    );

    // Typing an open brace completes the pair and parks inside it.
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("{", Some("{"), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("fn a() {\n  x\n  {}".into(), 16, 16)
    );

    // Enter between the pair opens a middle line at the deeper level.
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Enter"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node).0,
        "fn a() {\n  x\n  {\n  \t\n  }"
    );
    assert_eq!(textarea_selection(&context, node).2, 20);
}

#[test]
fn code_editor_comment_toggle_and_tab_indent_the_line() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("  x").code_editor(true))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let meta = InputModifiers {
        meta: true,
        ..InputModifiers::default()
    };

    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("/", None, meta))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "  //x");
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("/", None, meta))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "  x");

    // Tab indents the caret line; Shift+Tab outdents again.
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Home"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Tab"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "\t  x");
    assert!(
        adapter
            .dispatch(&mut context, document, &shift_key("Tab"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "  x");
}

#[test]
fn plain_textarea_enter_inserts_a_bare_newline_without_pairing() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();

    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Enter"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "ab\n");
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("(", Some("("), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "ab\n(");
}

#[test]
fn pointer_press_places_the_caret_and_multi_click_selects() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("hello world"))
        .unwrap();
    let node = input.stable_id();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 32.0,
        },
    );
    layout.set_standard_visual(
        node,
        Some(crate::StandardVisual::TextInput {
            placeholder: std::sync::Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: std::sync::Arc::from([]),
            matches: std::sync::Arc::from([]),
            color_swatches: std::sync::Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: std::sync::Arc::from([]),
            git_marks: std::sync::Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    let click = |x: f32, y: f32, phase: PointerPhase| {
        pointer_fixture! {
            phase,
            pointer_id: 7,
            pointer_type: PointerType::Mouse,
            x,
            y,
            screen_x: x,
            screen_y: y,
            button: 0,
            buttons: u16::from(phase == PointerPhase::Down || phase == PointerPhase::Move),
            pressure: 1.0,
            tangential_pressure: 0.0,
            tilt_x: 0,
            tilt_y: 0,
            twist: 0,
            is_primary: true,
            activation_click: false,
            modifiers: InputModifiers::default(),
        }
    };

    // A press past the line end parks the caret on the line end.
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &click(190.0, 16.0, PointerPhase::Down),
                Duration::from_millis(1_000),
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &click(190.0, 16.0, PointerPhase::Up),
            Duration::from_millis(1_020),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(
        textarea_selection(&context, node),
        ("hello world".into(), 11, 11)
    );

    // A quick second press selects the word under the caret.
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &click(190.0, 16.0, PointerPhase::Down),
                Duration::from_millis(1_060),
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("hello world".into(), 6, 11)
    );
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &click(190.0, 16.0, PointerPhase::Up),
            Duration::from_millis(1_080),
            Some(&mut shaper),
        )
        .unwrap();
}

#[test]
fn pointer_drag_extends_the_selection_from_the_press_anchor() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("hello world"))
        .unwrap();
    let node = input.stable_id();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 32.0,
        },
    );
    layout.set_standard_visual(
        node,
        Some(crate::StandardVisual::TextInput {
            placeholder: std::sync::Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: std::sync::Arc::from([]),
            matches: std::sync::Arc::from([]),
            color_swatches: std::sync::Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: std::sync::Arc::from([]),
            git_marks: std::sync::Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    let pointer_event = |phase: PointerPhase, x: f32| {
        pointer_fixture! {
            phase,
            pointer_id: 3,
            pointer_type: PointerType::Mouse,
            x,
            y: 16.0,
            screen_x: x,
            screen_y: 16.0,
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
        }
    };

    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_event(PointerPhase::Down, 190.0),
            Duration::from_millis(2_000),
            Some(&mut shaper),
        )
        .unwrap();
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &pointer_event(PointerPhase::Move, 0.0),
                Duration::from_millis(2_010),
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("hello world".into(), 11, 0)
    );
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_event(PointerPhase::Up, 0.0),
            Duration::from_millis(2_020),
            Some(&mut shaper),
        )
        .unwrap();
}

/// 挂一个收集 TextChanged 的观察者，供查找/替换命令断言事件发射。
fn track_text_changed(context: &mut AppContext, area: Entity<TextArea>) -> Arc<Mutex<Vec<String>>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    context
        .on(area, move |_area, event: &TextChanged, _cx| {
            sink.lock().unwrap().push(event.value.to_string());
        })
        .unwrap();
    events
}

/// 多行编辑器拖拽移动测试的公共装配：两行 `abc\ndef`，字符宽 10、
/// 行高 12、零内边距（offset = 列×10 + 行×12 命中）。返回
/// `(context, document, node, 事件收集器)`。
fn drag_drop_editor() -> (
    AppContext,
    DocumentId,
    StableNodeId,
    Arc<Mutex<Vec<String>>>,
) {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("abc\ndef").style(crate::NodeStyle {
                layout: std::sync::Arc::new(nana_ui_core::LayoutStyle {
                    padding: Some(nana_ui_core::LengthSpec::Px(0.0)),
                    font_size: Some(10.0),
                    line_height: Some(nana_ui_core::LineHeightSpec::Absolute(12.0)),
                    min_height: None,
                    ..nana_ui_core::LayoutStyle::default()
                }),
                ..crate::NodeStyle::default()
            }),
        )
        .unwrap();
    let node = area.stable_id();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 64.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);
    (context, document, node, events)
}

/// 带 x/y 与修饰键的指针事件构造器。
fn drag_pointer_event(phase: PointerPhase, x: f32, y: f32, modifiers: InputModifiers) -> Gesture {
    pointer_fixture! {
        phase,
        pointer_id: 3,
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
        modifiers,
    }
}

/// 选中 `def`：End 到文档尾，Shift+Left 三次（偏移 7..4）。
fn select_trailing_def(
    adapter: &mut TestInput,
    context: &mut AppContext,
    document: DocumentId,
    shaper: &mut MeasureTextShaper,
) {
    adapter
        .dispatch_with_shaper(
            context,
            document,
            &plain_key("End"),
            Duration::from_millis(1_000),
            Some(shaper),
        )
        .unwrap();
    for _ in 0..3 {
        adapter
            .dispatch_with_shaper(
                context,
                document,
                &shift_key("ArrowLeft"),
                Duration::from_millis(1_000),
                Some(shaper),
            )
            .unwrap();
    }
}

/// 无修饰键的拖拽指针事件（按下/移动/释放）。
fn pointer_down(x: f32, y: f32) -> Gesture {
    drag_pointer_event(PointerPhase::Down, x, y, InputModifiers::default())
}

fn pointer_move(x: f32, y: f32) -> Gesture {
    drag_pointer_event(PointerPhase::Move, x, y, InputModifiers::default())
}

fn pointer_up(x: f32, y: f32) -> Gesture {
    drag_pointer_event(PointerPhase::Up, x, y, InputModifiers::default())
}

/// 拖拽移动主流程：选中 `def`（第二行 0..3 列 → 偏移 4..7），在选区
/// 内按下并拖到第一行行首释放 = 移动文本，选区落在插入文本上，整
/// 个移动只发一次变更（单步撤销的修订语义）。
#[test]
fn drag_selection_moves_text_in_one_revision() {
    let (mut context, document, node, events) = drag_drop_editor();
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    // 选中 "def"：End 到文档尾，Shift+Left 三次。
    select_trailing_def(&mut adapter, &mut context, document, &mut shaper);
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndef".into(), 7, 4)
    );

    // 选区内按下（"e"，offset 5）→ 不塌缩选区。
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_down(15.0, 18.0),
            Duration::from_millis(2_000),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndef".into(), 7, 4)
    );
    // 超过阈值拖到第一行行首（offset 0）。
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_move(0.0, 6.0),
            Duration::from_millis(2_010),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_up(0.0, 6.0),
            Duration::from_millis(2_020),
            Some(&mut shaper),
        )
        .unwrap();
    // "def" 移到文档头，选区落在插入文本上；单次变更（一步撤销）。
    assert_eq!(
        textarea_selection(&context, node),
        ("defabc\n".into(), 0, 3)
    );
    assert_eq!(*events.lock().unwrap(), vec!["defabc\n".to_owned()]);
}

/// 落点在源选区边界（target == start / target == end）是退化 no-op：
/// 文本与选区都保持原状，不产生变更事件。
#[test]
fn drag_to_selection_boundary_is_a_no_op() {
    let (mut context, document, node, events) = drag_drop_editor();
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    select_trailing_def(&mut adapter, &mut context, document, &mut shaper);
    // target == start（offset 4）：选区头。
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_down(15.0, 18.0),
            Duration::from_millis(2_000),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_move(2.0, 18.0),
            Duration::from_millis(2_010),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_up(2.0, 18.0),
            Duration::from_millis(2_020),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndef".into(), 7, 4)
    );
    // target == end（offset 7）：选区尾。
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_down(15.0, 18.0),
            Duration::from_millis(3_000),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_move(35.0, 18.0),
            Duration::from_millis(3_010),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_up(35.0, 18.0),
            Duration::from_millis(3_020),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndef".into(), 7, 4)
    );
    assert!(events.lock().unwrap().is_empty());
}

/// Alt 拖拽 = 复制：原文本保留，选区落在插入的副本上。
#[test]
fn alt_drag_selection_copies_text() {
    let (mut context, document, node, events) = drag_drop_editor();
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    select_trailing_def(&mut adapter, &mut context, document, &mut shaper);
    let alt = InputModifiers {
        alt: true,
        ..Default::default()
    };
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Down, 15.0, 18.0, alt),
            Duration::from_millis(2_000),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Move, 0.0, 6.0, alt),
            Duration::from_millis(2_010),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Up, 0.0, 6.0, alt),
            Duration::from_millis(2_020),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(
        textarea_selection(&context, node),
        ("defabc\ndef".into(), 0, 3)
    );
    assert_eq!(*events.lock().unwrap(), vec!["defabc\ndef".to_owned()]);
}

/// 低于阈值：按下选区后小位移释放不移动文本，按原点击语义落 caret。
#[test]
fn selection_press_below_threshold_falls_back_to_click() {
    let (mut context, document, node, events) = drag_drop_editor();
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    select_trailing_def(&mut adapter, &mut context, document, &mut shaper);
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Down, 15.0, 18.0, InputModifiers::default()),
            Duration::from_millis(2_000),
            Some(&mut shaper),
        )
        .unwrap();
    // 2px 位移，未过阈值。
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Move, 13.0, 18.0, InputModifiers::default()),
            Duration::from_millis(2_010),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Up, 13.0, 18.0, InputModifiers::default()),
            Duration::from_millis(2_020),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndef".into(), 5, 5)
    );
    assert!(events.lock().unwrap().is_empty());
}

/// Esc 取消拖拽：文本与选区保持原状，后续释放不落文本。
#[test]
fn escape_cancels_selection_drag() {
    let (mut context, document, node, events) = drag_drop_editor();
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    select_trailing_def(&mut adapter, &mut context, document, &mut shaper);
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Down, 15.0, 18.0, InputModifiers::default()),
            Duration::from_millis(2_000),
            Some(&mut shaper),
        )
        .unwrap();
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &drag_pointer_event(PointerPhase::Move, 0.0, 6.0, InputModifiers::default()),
            Duration::from_millis(2_010),
            Some(&mut shaper),
        )
        .unwrap();
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &plain_key("Escape"),
                Duration::from_millis(2_015),
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &pointer_up(0.0, 6.0),
            Duration::from_millis(2_020),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(
        textarea_selection(&context, node),
        ("abc\ndef".into(), 7, 4)
    );
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn find_next_and_previous_select_matches_without_text_changed() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab AB ab"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);
    let sensitive = TextSearchOptions {
        case_sensitive: true,
        ..TextSearchOptions::default()
    };

    // 大小写敏感："ab" 只命中 0..2 与 6..8。
    assert!(
        context
            .find_next_focused_text_match(document, "ab", sensitive, TextFindScope::Document)
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("ab AB ab".into(), 0, 2)
    );
    assert!(
        context
            .find_next_focused_text_match(document, "ab", sensitive, TextFindScope::Document)
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("ab AB ab".into(), 6, 8)
    );
    // 越过末尾后环绕。
    assert!(
        context
            .find_next_focused_text_match(document, "ab", sensitive, TextFindScope::Document)
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("ab AB ab".into(), 0, 2)
    );
    // 大小写不敏感：从当前选区末端起下一个命中是 "AB"。
    assert!(
        context
            .find_next_focused_text_match(
                document,
                "ab",
                TextSearchOptions::default(),
                TextFindScope::Document
            )
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("ab AB ab".into(), 3, 5)
    );
    // 上一个回到第一个 "ab"。
    assert!(
        context
            .find_previous_focused_text_match(document, "ab", sensitive, TextFindScope::Document)
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("ab AB ab".into(), 0, 2)
    );
    // 纯移动：值不变、不发 TextChanged。
    assert!(events.lock().unwrap().is_empty());
    // 空 query 不命中。
    assert!(
        !context
            .find_next_focused_text_match(document, "", sensitive, TextFindScope::Document)
            .unwrap()
    );
    assert!(
        !context
            .find_previous_focused_text_match(document, "zz", sensitive, TextFindScope::Document)
            .unwrap()
    );
}

#[test]
fn replace_focused_text_match_replaces_only_a_matching_selection() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab ab"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);
    let options = TextSearchOptions::default();

    // 选中第一个 "ab" 后替换，并选中替换后的文本。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::new(0, 2);
        })
        .unwrap();
    assert!(
        context
            .replace_focused_text_match(document, "ab", options, "XY", false)
            .unwrap()
    );
    assert_eq!(textarea_selection(&context, node), ("XY ab".into(), 0, 2));
    assert_eq!(*events.lock().unwrap(), vec!["XY ab".to_string()]);

    // 选区不再是匹配（现在是 "XY"），替换拒绝且不发射事件。
    assert!(
        !context
            .replace_focused_text_match(document, "ab", options, "XY", false)
            .unwrap()
    );
    assert_eq!(textarea_selection(&context, node), ("XY ab".into(), 0, 2));

    // 宿主先查找下一个再替换。
    assert!(
        context
            .find_next_focused_text_match(document, "ab", options, TextFindScope::Document)
            .unwrap()
    );
    assert_eq!(textarea_selection(&context, node), ("XY ab".into(), 3, 5));
    assert!(
        context
            .replace_focused_text_match(document, "ab", options, "XY", false)
            .unwrap()
    );
    assert_eq!(textarea_selection(&context, node), ("XY XY".into(), 3, 5));
    assert_eq!(events.lock().unwrap().len(), 2);
}

#[test]
fn replace_all_focused_text_matches_reports_count_and_lands_on_first() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab cd ab"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);

    assert_eq!(
        context
            .replace_all_focused_text_matches(
                document,
                "ab",
                TextSearchOptions {
                    whole_word: true,
                    ..TextSearchOptions::default()
                },
                "X",
                TextFindScope::Document,
                false,
            )
            .unwrap(),
        2
    );
    assert_eq!(textarea_selection(&context, node), ("X cd X".into(), 0, 1));
    assert_eq!(*events.lock().unwrap(), vec!["X cd X".to_string()]);

    // 没有匹配时不修改、不发射事件、计数为 0。
    assert_eq!(
        context
            .replace_all_focused_text_matches(
                document,
                "ab",
                TextSearchOptions::default(),
                "X",
                TextFindScope::Document,
                false,
            )
            .unwrap(),
        0
    );
    assert_eq!(
        context
            .replace_all_focused_text_matches(
                document,
                "",
                TextSearchOptions::default(),
                "X",
                TextFindScope::Document,
                false
            )
            .unwrap(),
        0
    );
    assert_eq!(events.lock().unwrap().len(), 1);
}

#[test]
fn alt_arrow_keys_move_and_duplicate_the_caret_line() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab\ncd\nef"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);
    let mut adapter = TestInput::default();
    let alt = InputModifiers {
        alt: true,
        ..InputModifiers::default()
    };
    let alt_shift = InputModifiers {
        alt: true,
        shift: true,
        ..InputModifiers::default()
    };
    // 光标停在 "cd" 行内（偏移 4）。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(4);
        })
        .unwrap();

    // Alt+Up 把 "cd" 移到顶部，选区（光标）跟随移动后的文本。
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("ArrowUp", None, alt))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("cd\nab\nef".into(), 1, 1)
    );
    assert_eq!(*events.lock().unwrap(), vec!["cd\nab\nef".to_string()]);

    // Alt+Down 移回原位。
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("ArrowDown", None, alt))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("ab\ncd\nef".into(), 4, 4)
    );

    // Alt+Shift+Down 在下方复制当前行，光标落在副本上。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("ArrowDown", None, alt_shift)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("ab\ncd\ncd\nef".into(), 7, 7)
    );

    // 文档边缘：手势仍被消费（不回落为普通光标移动），但没有编辑。
    context
        .update_component(area, |area, _cx| {
            area.state = crate::TextInputState::new("top\nbottom");
        })
        .unwrap();
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(0);
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("ArrowUp", None, alt))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "top\nbottom");
}

#[test]
fn cmd_shift_k_ctrl_j_and_case_keys_transform_the_focused_editor() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab\ncd\nef"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);
    let mut adapter = TestInput::default();
    let ctrl_shift = InputModifiers {
        control: true,
        shift: true,
        ..InputModifiers::default()
    };
    let ctrl = InputModifiers {
        control: true,
        ..InputModifiers::default()
    };
    // 光标停在 "cd" 行内。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(4);
        })
        .unwrap();

    // Ctrl+Shift+K 删除光标所在行 "cd"。
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("k", None, ctrl_shift))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("ab\nef".into(), 3, 3));
    assert_eq!(*events.lock().unwrap(), vec!["ab\nef".to_string()]);

    // Ctrl+J 合并剩余两行（单空格接缝）。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(1);
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("j", None, ctrl))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "ab ef");
    assert_eq!(events.lock().unwrap().len(), 2);

    // Ctrl+Shift+U 转大写选区，Ctrl+U 转小写。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::new(0, 5);
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("u", None, ctrl_shift))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("AB EF".into(), 0, 5));
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("u", None, ctrl))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("ab ef".into(), 0, 5));
    assert_eq!(events.lock().unwrap().len(), 4);
}

#[test]
fn line_transformation_keys_decline_on_single_line_fields() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("abc"))
        .unwrap();
    let node = input.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();

    // 单行字段没有行块语义：这些键不消费，留给通用路由。
    let ctrl_shift = InputModifiers {
        control: true,
        shift: true,
        ..InputModifiers::default()
    };
    assert!(
        !adapter
            .dispatch(&mut context, document, &edit_key("k", None, ctrl_shift))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node).0, "abc");
}

#[test]
fn page_keys_page_by_logical_lines_without_a_shaper() {
    let value = (0..40)
        .map(|index| format!("l{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new(value.clone()))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(1);
        })
        .unwrap();
    let mut adapter = TestInput::default();

    // 无 shaper：固定 15 个逻辑行。第 15 行起点在 10 个 3 字节行 + 5 个
    // 4 字节行之后，保持第 1 列。
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("PageDown"))
            .unwrap()
            .prevent_default
    );
    let line15 = 10 * 3 + 5 * 4;
    assert_eq!(
        textarea_selection(&context, node),
        (value.clone(), line15 + 1, line15 + 1)
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("PageUp"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), (value.clone(), 1, 1));

    // Shift+PageDown 扩展选区（锚点保留在原列）。
    assert!(
        adapter
            .dispatch(&mut context, document, &shift_key("PageDown"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), (value, 1, line15 + 1));
}

#[test]
fn page_keys_with_a_shaper_move_one_viewport_and_clamp() {
    let value = (0..10)
        .map(|index| format!("l{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new(value.clone()))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 300.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(0);
        })
        .unwrap();
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();

    // 视口高于文档：一次 PageDown 钳制到文档末尾，PageUp 回到首行。
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &plain_key("PageDown"),
                Duration::ZERO,
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    // 目标列保持 0：落在最后一行行首（而非文档末尾偏移）。
    assert_eq!(textarea_selection(&context, node), (value.clone(), 27, 27));
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &plain_key("PageUp"),
                Duration::ZERO,
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), (value, 0, 0));
}

#[test]
fn goto_focused_text_matching_bracket_jumps_to_the_partner() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("fn main() {}"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);

    // 光标停在 '{' 之前：跳到配对的 '}' 上（纯移动，不发事件）。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(10);
        })
        .unwrap();
    assert!(
        context
            .goto_focused_text_matching_bracket(document)
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("fn main() {}".into(), 11, 11)
    );
    // 再跳一次回到 '{'。
    assert!(
        context
            .goto_focused_text_matching_bracket(document)
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("fn main() {}".into(), 10, 10)
    );
    assert!(events.lock().unwrap().is_empty());

    // 邻近没有括号时不消费。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(2);
        })
        .unwrap();
    assert!(
        !context
            .goto_focused_text_matching_bracket(document)
            .unwrap()
    );
}

#[test]
fn sort_focused_text_lines_sorts_dedups_and_emits_one_change() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("pear\napple\npear"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::new(0, "pear\napple\npear".len());
        })
        .unwrap();

    // 升序 + 去重，选区覆盖排序后的块。
    assert!(
        context
            .sort_focused_text_lines(document, false, true)
            .unwrap()
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("apple\npear".into(), 0, 10)
    );
    assert_eq!(*events.lock().unwrap(), vec!["apple\npear".to_string()]);

    // 降序还原顺序差异。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::new(0, "apple\npear".len());
        })
        .unwrap();
    assert!(
        context
            .sort_focused_text_lines(document, true, false)
            .unwrap()
    );
    assert_eq!(textarea_selection(&context, node).0, "pear\napple");

    // 单行无变化：拒绝且不发事件。
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(2);
        })
        .unwrap();
    assert!(
        !context
            .sort_focused_text_lines(document, false, false)
            .unwrap()
    );
    assert_eq!(events.lock().unwrap().len(), 2);
}

fn textarea_selections(
    context: &AppContext,
    node: StableNodeId,
) -> (String, (usize, usize), Vec<(usize, usize)>) {
    let state = context.world().text_input(node).unwrap();
    (
        state.value.to_owned(),
        (state.selection.anchor, state.selection.focus),
        state
            .additional_selections
            .iter()
            .map(|selection| (selection.anchor, selection.focus))
            .collect(),
    )
}

fn set_selections(
    context: &mut AppContext,
    area: Entity<TextArea>,
    primary: (usize, usize),
    additional: Vec<(usize, usize)>,
) {
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::new(primary.0, primary.1);
            area.state.additional_selections = additional
                .into_iter()
                .map(|(anchor, focus)| TextSelection::new(anchor, focus))
                .collect();
        })
        .unwrap();
}

#[test]
fn editor_render_options_default_off_and_opt_in_drives_derived_presentation() {
    // 四个渲染选项默认关闭：未开启时不产生任何派生标记。
    let defaults = TextArea::new("alpha beta\nalpha");
    assert!(!defaults.occurrence_highlight);
    assert!(!defaults.relative_line_numbers);
    assert!(!defaults.show_whitespace);
    assert!(defaults.wrap_guides.is_empty());

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("alpha beta\nalpha")
                .line_numbers(true)
                .relative_line_numbers(true)
                .occurrence_highlight(true)
                .show_whitespace(true)
                .wrap_guides(std::sync::Arc::from([4])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 40.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    let work = context.take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut MeasureTextShaper)
        .unwrap();

    let presentation = context
        .world()
        .text_input_presentation(node)
        .expect("presentation");
    // 出现高亮：光标（值末尾）停在第二行 "alpha" 上，该出现不画；
    // 全词匹配排除前缀，只剩第一行的 "alpha" 一条。
    assert_eq!(presentation.occurrence_marks.len(), 1);
    // 空白显示：行内一个空格（"alpha beta"），换行不标记。
    assert_eq!(presentation.whitespace_marks.len(), 1);
    // wrap guide：列 4 一个 x 位置。
    assert_eq!(presentation.wrap_guides.len(), 1);
    // 相对行号：光标（值末尾）在第 2 行，显示绝对 2；第 1 行距离 1。
    assert_eq!(presentation.line_numbers, vec![1, 2]);
}

#[test]
fn textarea_wheel_scrolls_internal_text_and_survives_projection() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("line\n".repeat(120)))
        .unwrap();
    let node = area.stable_id();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 80.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    let work = context.take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut MeasureTextShaper)
        .unwrap();
    context.rebuild_hit_test(document);
    let event = wheel_fixture! {
        x: 60.0,
        y: 40.0,
        delta_x: 0.0,
        delta_y: -120.0,
        line_delta: false,
        modifiers: Default::default(),
    };
    assert!(
        TestInput::default()
            .dispatch(&mut context, document, &event)
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        context.read(area, |area| area.scroll_offset.y).unwrap(),
        120.0
    );
    context
        .update_component(area, |area, _| area.invalid = true)
        .unwrap();
    assert_eq!(context.world().scroll_offset(node).unwrap().y, 120.0);
}

#[test]
fn pointer_press_on_fold_gutter_toggles_the_fold() {
    let value = "fn a() {\n    x();\n    y();\n}\nfn b() {}";
    let fold = crate::TextCodeFold::new(7, 28);
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut style = crate::NodeStyle::default();
    std::sync::Arc::make_mut(&mut style.layout).padding_left =
        Some(nana_ui_core::LengthSpec::Px(46.0));
    let area = context
        .create_component(
            document,
            TextArea::new(value)
                .code_editor(true)
                .line_numbers(true)
                .code_folds(std::sync::Arc::from([fold]))
                .style(style),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 40.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    let work = context.take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut MeasureTextShaper)
        .unwrap();
    context.rebuild_hit_test(document);
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();

    // 光标移到折叠起始行，避免聚焦滚动把该行推出视口。
    context
        .update_component(area, |area_view, _| {
            area_view.state.selection = crate::TextSelection::caret(3);
        })
        .unwrap();
    let work = context.take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut MeasureTextShaper)
        .unwrap();
    context.rebuild_hit_test(document);

    let click = |x: f32, y: f32, phase: PointerPhase| {
        pointer_fixture! {
            phase,
            pointer_id: 7,
            pointer_type: PointerType::Mouse,
            x,
            y,
            screen_x: x,
            screen_y: y,
            button: 0,
            buttons: u16::from(phase == PointerPhase::Down || phase == PointerPhase::Move),
            pressure: 1.0,
            tangential_pressure: 0.0,
            tilt_x: 0,
            tilt_y: 0,
            twist: 0,
            is_primary: true,
            activation_click: false,
            modifiers: InputModifiers::default(),
        }
    };

    // 折叠前：文档末行的 reveal 行距按 5 行计。
    let reveal_before = context
        .world()
        .text_input_reveal_scroll(node, value.len())
        .unwrap();
    let gutter = context
        .world()
        .component_geometry(node)
        .and_then(|geometry| match geometry {
            crate::ComponentGeometry::TextInput { folds, .. } => folds.gutters.first().copied(),
            _ => None,
        })
        .expect("fold gutter geometry");
    let center = (
        gutter.bounds.x + gutter.bounds.width / 2.0,
        gutter.bounds.y + gutter.bounds.height / 2.0,
    );

    // 点击 gutter 箭头：折叠该区间（消费事件、不落光标）。
    assert!(
        context
            .pointer_target(document, center.0, center.1)
            .is_some(),
        "no hit target at {:?}",
        center
    );
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &click(center.0, center.1, PointerPhase::Down),
                Duration::from_millis(1_000),
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &click(center.0, center.1, PointerPhase::Up),
            Duration::from_millis(1_010),
            Some(&mut shaper),
        )
        .unwrap();
    assert_eq!(context.world().text_fold_collapsed(node), vec![fold]);
    let work = context.take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut MeasureTextShaper)
        .unwrap();

    // 折叠后渲染行数减少：同一偏移的 reveal 行距按显示视图换算变小。
    let reveal_after = context
        .world()
        .text_input_reveal_scroll(node, value.len())
        .unwrap();
    assert!(reveal_after.y < reveal_before.y);

    // 再次点击箭头：展开。
    let gutter = context
        .world()
        .component_geometry(node)
        .and_then(|geometry| match geometry {
            crate::ComponentGeometry::TextInput { folds, .. } => folds.gutters.first().copied(),
            _ => None,
        })
        .expect("fold gutter geometry");
    let center = (
        gutter.bounds.x + gutter.bounds.width / 2.0,
        gutter.bounds.y + gutter.bounds.height / 2.0,
    );
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &click(center.0, center.1, PointerPhase::Down),
                Duration::from_millis(2_000),
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    adapter
        .dispatch_with_shaper(
            &mut context,
            document,
            &click(center.0, center.1, PointerPhase::Up),
            Duration::from_millis(2_010),
            Some(&mut shaper),
        )
        .unwrap();
    assert!(context.world().text_fold_collapsed(node).is_empty());
}

#[test]
fn escape_collapses_additional_cursors_and_single_cursor_escape_passes_through() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab\ncd\nef"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let escape = || {
        key_fixture! {
            pressed: true,
            key: "Escape".into(),
            text: None,
            code: "Escape".into(),
            repeat: false,
            modifiers: InputModifiers::default(),
        }
    };

    // 多光标：Esc 塌缩到主光标并消费事件。
    set_selections(&mut context, area, (1, 1), vec![(4, 4)]);
    assert!(
        adapter
            .dispatch(&mut context, document, &escape())
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("ab\ncd\nef".into(), (1, 1), vec![])
    );

    // 单光标：Esc 不消费（宿主继续处理），选区不变。
    assert!(
        !adapter
            .dispatch(&mut context, document, &escape())
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("ab\ncd\nef".into(), (1, 1), vec![])
    );
}

#[test]
fn escape_ends_snippet_session_before_collapsing_cursors() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab\ncd"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    set_selections(&mut context, area, (0, 0), vec![(5, 5)]);
    assert!(
        context
            .insert_focused_text_snippet(document, &crate::TextSnippet::new("s", "[$1]$0"),)
            .unwrap()
    );
    let mut adapter = TestInput::default();
    let escape = key_fixture! {
        pressed: true,
        key: "Escape".into(),
        text: None,
        code: "Escape".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    };

    // 第一个 Esc：只结束 snippet 会话，多光标保留。
    assert!(
        adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("[]ab\ncd".into(), (2, 2), vec![(7, 7)])
    );

    // 第二个 Esc：塌缩多光标。
    assert!(
        adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("[]ab\ncd".into(), (2, 2), vec![])
    );
}

#[test]
fn snippet_session_tab_routes_through_the_adapter_before_indent() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("").code_editor(true))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    assert!(
        context
            .insert_focused_text_snippet(document, &crate::TextSnippet::new("if", "if $1 {$0"),)
            .unwrap()
    );
    let mut adapter = TestInput::default();

    // 会话内 Tab 跳位（消费且不插入缩进）。
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Tab"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("if  {".into(), 3, 3));

    // 会话结束后 Tab 回到缩进行为。
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Tab"))
            .unwrap()
            .prevent_default
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Tab"))
            .unwrap()
            .prevent_default
    );
    assert_ne!(textarea_selection(&context, node).0, "if  {");
}

#[test]
fn multi_cursor_typing_deleting_and_newline_edit_every_selection() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab\ncd"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let events = track_text_changed(&mut context, area);
    let mut adapter = TestInput::default();
    set_selections(&mut context, area, (1, 1), vec![(4, 4)]);

    // 打字：每个光标各插入一个字符，一次事件。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("x", Some("x"), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("axb\ncxd".into(), (2, 2), vec![(6, 6)])
    );
    assert_eq!(events.lock().unwrap().len(), 1);

    // 退格：每个光标各删除一个字符。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("Backspace", None, InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        // 第二个光标删掉 c 后的 x，落在 c、d 之间（偏移 4）。
        ("ab\ncd".into(), (1, 1), vec![(4, 4)])
    );
    assert_eq!(events.lock().unwrap().len(), 2);

    // Enter：每个光标各换一行（无代码编辑，无自动缩进）。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("Enter", Some("\n"), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("a\nb\nc\nd".into(), (2, 2), vec![(6, 6)])
    );
    assert_eq!(events.lock().unwrap().len(), 3);
}

#[test]
fn alt_cmd_arrows_add_cursors_by_column_skip_duplicates_and_stay_at_edges() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("abcd\nef\nghij"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let alt_cmd = InputModifiers {
        alt: true,
        meta: true,
        ..InputModifiers::default()
    };
    set_selections(&mut context, area, (2, 2), vec![]);

    // Alt+Cmd+Down 在下一行按列加光标；列超出则贴到行尾。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("ArrowDown", None, alt_cmd)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("abcd\nef\nghij".into(), (2, 2), vec![(7, 7)])
    );

    // 再按一次：第二个光标下方按列对齐，第一行光标的候选与已有重复合。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("ArrowDown", None, alt_cmd)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("abcd\nef\nghij".into(), (2, 2), vec![(7, 7), (10, 10)])
    );

    // 文档边缘：手势仍被消费，但不再新增光标。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("ArrowDown", None, alt_cmd)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node).2,
        vec![(7, 7), (10, 10)]
    );

    // Alt+Cmd+Up 回程同样按列对齐（10 -> 7 -> 2 依次被已有光标去重）。
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("ArrowUp", None, alt_cmd))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node).2,
        vec![(7, 7), (10, 10)]
    );
}

#[test]
fn cmd_d_selects_occurrences_wrapping_and_skipping_covered_spans() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab cd ab"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let meta = InputModifiers {
        meta: true,
        ..InputModifiers::default()
    };
    set_selections(&mut context, area, (0, 2), vec![]);

    // Cmd+D 选中下一个 "ab"（全词匹配）。
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("d", None, meta))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("ab cd ab".into(), (0, 2), vec![(6, 8)])
    );

    // 全部出现都已有光标：不再新增（键不被消费）。
    assert!(
        !adapter
            .dispatch(&mut context, document, &edit_key("d", None, meta))
            .unwrap()
            .prevent_default
    );

    // 环形：只留末尾选区时，Cmd+D 绕回文档开头。
    set_selections(&mut context, area, (6, 8), vec![]);
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("d", None, meta))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("ab cd ab".into(), (6, 8), vec![(0, 2)])
    );

    // 全部选中：裸光标取光标下的词，选中所有出现。
    set_selections(&mut context, area, (1, 1), vec![]);
    assert!(
        context
            .select_all_focused_text_occurrences(document)
            .unwrap()
    );
    // 裸光标被它所在的词选区吸收（并集后主光标即该词）。
    assert_eq!(
        textarea_selections(&context, node),
        ("ab cd ab".into(), (0, 2), vec![(6, 8)])
    );

    // 收回到主光标；再次收回是空操作。
    assert!(context.collapse_focused_text_selections(document).unwrap());
    assert_eq!(textarea_selections(&context, node).2, vec![]);
    assert!(!context.collapse_focused_text_selections(document).unwrap());
}

#[test]
fn copy_joins_multi_cursor_selections_and_paste_hits_every_cursor() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab cd"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let meta = |key: &str| {
        edit_key(
            key,
            None,
            InputModifiers {
                meta: true,
                ..InputModifiers::default()
            },
        )
    };
    set_selections(&mut context, area, (0, 2), vec![(3, 5)]);

    // Cmd+C：多选区按序拼接（Zed 语义，换行连接）。
    assert!(
        adapter
            .dispatch(&mut context, document, &meta("c"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(adapter.clipboard(), Some("ab\ncd"));

    // Cmd+V：同一段文本插入到每个光标。
    set_selections(&mut context, area, (0, 0), vec![(5, 5)]);
    assert!(
        adapter
            .dispatch(&mut context, document, &meta("v"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("ab\ncdab cdab\ncd".into(), (5, 5), vec![(15, 15)])
    );

    // Cmd+X：多选区剪切一并删除。
    set_selections(&mut context, area, (0, 2), vec![(8, 10)]);
    assert!(
        adapter
            .dispatch(&mut context, document, &meta("x"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selections(&context, node).0, "\ncdab ab\ncd");
}

#[test]
fn ime_commit_scopes_to_the_primary_cursor_and_remaps_others() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("ab\ncd"))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    set_selections(&mut context, area, (2, 2), vec![(4, 4)]);

    assert!(
        adapter
            .dispatch_ime(
                &mut context,
                document,
                &CompositionInput::Commit("X".into())
            )
            .unwrap()
            .prevent_default
    );
    // 只有主光标收到提交文本，附加光标随编辑平移。
    assert_eq!(
        textarea_selections(&context, node),
        ("abX\ncd".into(), (3, 3), vec![(5, 5)])
    );
}

#[test]
fn single_line_fields_reject_multi_cursor_gestures() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("hi"))
        .unwrap();
    let node = input.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let alt_cmd = InputModifiers {
        alt: true,
        meta: true,
        ..InputModifiers::default()
    };
    let meta = InputModifiers {
        meta: true,
        ..InputModifiers::default()
    };

    // 命令层直接拒绝。
    assert!(
        !context
            .add_focused_text_cursor(document, false, None)
            .unwrap()
    );
    assert!(
        !context
            .select_focused_text_occurrence(document, false)
            .unwrap()
    );

    // Alt+Cmd+Down 回落到普通移动（meta=DocEnd），不加光标。
    // 先把光标挪到行首，DocEnd 才有位移。
    context
        .update_component(input, |input, _cx| {
            input.state.selection = TextSelection::caret(0);
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("ArrowDown", None, alt_cmd)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selections(&context, node).1, (2, 2));
    assert_eq!(textarea_selections(&context, node).2, vec![]);

    // Cmd+D 不消费、不产生附加光标。
    assert!(
        !adapter
            .dispatch(&mut context, document, &edit_key("d", None, meta))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selections(&context, node).2, vec![]);
}

#[test]
fn alt_click_adds_and_removes_cursors_and_plain_click_collapses() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("first\nsecond"))
        .unwrap();
    let node = area.stable_id();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 64.0,
        },
    );
    layout.set_standard_visual(
        node,
        Some(crate::StandardVisual::TextInput {
            placeholder: std::sync::Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: std::sync::Arc::from([]),
            matches: std::sync::Arc::from([]),
            color_swatches: std::sync::Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: std::sync::Arc::from([]),
            git_marks: std::sync::Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    context.commit_mutations(layout).unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document);

    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    let click = |x: f32, y: f32, alt: bool| {
        pointer_fixture! {
            phase: PointerPhase::Down,
            pointer_id: 7,
            pointer_type: PointerType::Mouse,
            x,
            y,
            screen_x: x,
            screen_y: y,
            button: 0,
            buttons: 1,
            pressure: 1.0,
            tangential_pressure: 0.0,
            tilt_x: 0,
            tilt_y: 0,
            twist: 0,
            is_primary: true,
            activation_click: false,
            modifiers: if alt {
                InputModifiers {
                    alt: true,
                    ..InputModifiers::default()
                }
            } else {
                InputModifiers::default()
            },
        }
    };
    struct Ctx<'a> {
        context: &'a mut AppContext,
        shaper: &'a mut MeasureTextShaper,
    }
    let mut ctx = Ctx {
        context: &mut context,
        shaper: &mut shaper,
    };
    let mut dispatch_down = |ctx: &mut Ctx, event: &Gesture, at: u64| -> bool {
        adapter
            .dispatch_with_shaper(
                ctx.context,
                document,
                event,
                Duration::from_millis(at),
                Some(&mut *ctx.shaper),
            )
            .unwrap()
            .prevent_default
    };

    // 先用普通点击探出 (2, 20) 落点的字符偏移（不依赖具体行高）。
    assert!(dispatch_down(&mut ctx, &click(2.0, 20.0, false), 1_000));
    let probe = textarea_selections(ctx.context, node).1;
    assert_eq!(probe.0, probe.1);
    // 把主光标挪到文档末尾，让目标点空出来。
    ctx.context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret("first\nsecond".len());
        })
        .unwrap();

    // Alt+点击同一点：新增一个光标。
    assert!(dispatch_down(&mut ctx, &click(2.0, 20.0, true), 2_000));
    assert_eq!(
        textarea_selections(ctx.context, node),
        ("first\nsecond".into(), (12, 12), vec![probe])
    );

    // 时间错开避免双击判定；再次 Alt+点击同一点：移除该光标。
    assert!(dispatch_down(&mut ctx, &click(2.0, 20.0, true), 3_000));
    assert_eq!(textarea_selections(ctx.context, node).2, vec![]);

    // Alt+点击第一行行首（主光标不在该处）：新增光标。
    assert!(dispatch_down(&mut ctx, &click(2.0, 4.0, true), 4_000));
    assert_eq!(textarea_selections(ctx.context, node).2, vec![(0, 0)]);

    // 普通点击：天然塌缩回单光标。
    assert!(dispatch_down(&mut ctx, &click(2.0, 4.0, false), 5_000));
    assert_eq!(
        textarea_selections(ctx.context, node),
        ("first\nsecond".into(), (0, 0), vec![])
    );
}

#[test]
fn multi_cursor_code_editing_indents_comments_moves_lines_and_deletes_words() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("  a\n  b").code_editor(true))
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut adapter = TestInput::default();
    let control = InputModifiers {
        control: true,
        ..InputModifiers::default()
    };
    let alt = InputModifiers {
        alt: true,
        ..InputModifiers::default()
    };

    // Enter 自动缩进：两个缩进行上的光标各起新行并继承缩进。
    set_selections(&mut context, area, (3, 3), vec![(7, 7)]);
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("Enter", Some("\n"), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("  a\n  \n  b\n  ".into(), (6, 6), vec![(13, 13)])
    );

    // Ctrl+/ 注释切换：每个光标注释自己所在的行。
    context
        .update_component(area, |area, _cx| {
            area.state = crate::TextInputState::new("aa\nbb");
            area.state.selection = TextSelection::caret(1);
            area.state.additional_selections = vec![TextSelection::caret(4)];
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("/", None, control))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("//aa\n//bb".into(), (3, 3), vec![(8, 8)])
    );

    // Alt+Backspace 词删除：每个光标删到词首。
    context
        .update_component(area, |area, _cx| {
            area.state = crate::TextInputState::new("aa\nbb");
            area.state.selection = TextSelection::caret(1);
            area.state.additional_selections = vec![TextSelection::caret(4)];
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("Backspace", None, alt))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("a\nb".into(), (0, 0), vec![(2, 2)])
    );

    // Alt+Down 行移动：首行光标把行下移；末行光标在边缘保持不动。
    context
        .update_component(area, |area, _cx| {
            area.state = crate::TextInputState::new("aa\nbb\ncc");
            area.state.selection = TextSelection::caret(1);
            area.state.additional_selections = vec![TextSelection::caret(7)];
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(&mut context, document, &edit_key("ArrowDown", None, alt))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("bb\naa\ncc".into(), (4, 4), vec![(7, 7)])
    );
}

fn completion_items(labels: &[&str]) -> std::sync::Arc<[crate::TextCompletion]> {
    labels
        .iter()
        .map(|label| crate::TextCompletion::new(*label, "fn"))
        .collect::<Vec<_>>()
        .into()
}

/// 布局 + shape + 命中测试的完整几何环境（指针/滚轮路由需要）。
fn shape_completion_editor(context: &mut AppContext, document: DocumentId, node: StableNodeId) {
    context.compat_world_mut().resolve_styles(&[node]).unwrap();
    context
        .compat_world_mut()
        .shape_text(&[node], &mut MeasureTextShaper)
        .unwrap();
    context.rebuild_hit_test(document);
}

fn completion_popup_geometry(
    context: &AppContext,
    node: StableNodeId,
) -> crate::TextCompletionPopup {
    match context.world().component_geometry(node) {
        Some(crate::ComponentGeometry::TextInput {
            completion_popup, ..
        }) => completion_popup.expect("completion popup geometry"),
        _ => panic!("text input geometry"),
    }
}

#[test]
fn completion_popup_owns_navigation_and_accept_keys() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("let fo").completions(completion_items(&["food", "foobar"])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    set_selections(&mut context, area, (6, 6), vec![]);
    let mut adapter = TestInput::default();
    let selected = |context: &AppContext, node| {
        context
            .world()
            .text_completion_snapshot(node)
            .map(|snapshot| (snapshot.selected, snapshot.dismissed))
    };

    // Down：弹层消费（选区不动），候选选中移到第二条。
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowDown"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("let fo".into(), 6, 6));
    assert_eq!(selected(&context, node), Some((1, false)));

    // Up：回到第一条。
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("ArrowUp"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(selected(&context, node), Some((0, false)));

    // Enter：接受选中项，一次 TextChanged，光标落在插入末尾。
    let changes = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink = std::sync::Arc::clone(&changes);
    context
        .on(area, move |_view, event: &TextChanged, _cx| {
            sink.lock().unwrap().push(event.value.to_string());
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Enter"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("let food".into(), 8, 8)
    );
    assert_eq!(*changes.lock().unwrap(), vec!["let food".to_string()]);

    // 宿主重喂（组件重投影）：会话重新激活，Tab 同样接受。
    context
        .update_component(area, |view, _| {
            view.completions = completion_items(&["food"]);
        })
        .unwrap();
    assert_eq!(selected(&context, node), Some((0, false)));
    assert!(
        adapter
            .dispatch(&mut context, document, &plain_key("Tab"))
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("let food".into(), 8, 8)
    );

    // 打字穿透正常编辑：committed value 直接更新（弹层保持，过滤
    // 由宿主重喂驱动）。
    context
        .update_component(area, |view, _| {
            view.completions = completion_items(&["food"]);
        })
        .unwrap();
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &edit_key("s", Some("s"), InputModifiers::default())
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selection(&context, node),
        ("let foods".into(), 9, 9)
    );
}

#[test]
fn modified_keys_pass_through_while_completion_active() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("ab ab").completions(completion_items(&["ab"])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    set_selections(&mut context, area, (1, 1), vec![]);
    let mut adapter = TestInput::default();

    // Cmd+D 穿透：选中下一出现（多光标 +1），弹层保持。
    let meta_d = edit_key(
        "d",
        None,
        InputModifiers {
            meta: true,
            ..InputModifiers::default()
        },
    );
    assert!(
        adapter
            .dispatch(&mut context, document, &meta_d)
            .unwrap()
            .prevent_default
    );
    assert_eq!(
        textarea_selections(&context, node),
        ("ab ab".into(), (1, 1), vec![(3, 5)])
    );
    assert!(
        context
            .world()
            .text_completion_snapshot(node)
            .is_some_and(|snapshot| !snapshot.dismissed)
    );
}

#[test]
fn escape_closes_completion_after_snippet_and_before_collapse() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("ab\ncd").completions(completion_items(&["ab"])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    set_selections(&mut context, area, (0, 0), vec![(5, 5)]);
    assert!(
        context
            .insert_focused_text_snippet(document, &crate::TextSnippet::new("s", "[$1]$0"),)
            .unwrap()
    );
    // snippet 插入后宿主重喂（组件重投影路径）：弹层重新激活。
    context
        .update_component(area, |view, _| {
            view.completions = completion_items(&["ab"]);
        })
        .unwrap();
    let mut adapter = TestInput::default();
    let escape = key_fixture! {
        pressed: true,
        key: "Escape".into(),
        text: None,
        code: "Escape".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    };

    // 第一个 Esc：结束 snippet 会话（弹层与多光标保留）。
    assert!(
        adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
    assert!(
        context
            .world()
            .text_completion_snapshot(node)
            .is_some_and(|snapshot| !snapshot.dismissed)
    );
    assert_eq!(textarea_selections(&context, node).2.len(), 1);

    // 第二个 Esc：关闭补全弹层（多光标保留）。
    assert!(
        adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
    assert!(
        context
            .world()
            .text_completion_snapshot(node)
            .is_some_and(|snapshot| snapshot.dismissed)
    );
    assert_eq!(textarea_selections(&context, node).2.len(), 1);

    // 第三个 Esc：塌缩多光标。
    assert!(
        adapter
            .dispatch(&mut context, document, &escape)
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selections(&context, node).2, vec![]);
}

#[test]
fn completion_click_accepts_row_and_wheel_scrolls_overlay() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("").completions(completion_items(&["alpha", "beta", "gamma", "delta"])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 140.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    shape_completion_editor(&mut context, document, node);

    // 点击弹层第二行：接受该候选（beta），不落光标。
    let popup = completion_popup_geometry(&context, node);
    let row = &popup.rows[1];
    let click = |x: f32, y: f32, phase: PointerPhase| {
        pointer_fixture! {
            phase,
            pointer_id: 7,
            pointer_type: PointerType::Mouse,
            x,
            y,
            screen_x: x,
            screen_y: y,
            button: 0,
            buttons: u16::from(phase == PointerPhase::Down),
            pressure: 1.0,
            tangential_pressure: 0.0,
            tilt_x: 0,
            tilt_y: 0,
            twist: 0,
            is_primary: true,
            activation_click: false,
            modifiers: InputModifiers::default(),
        }
    };
    let mut shaper = MeasureTextShaper;
    let mut adapter = TestInput::default();
    assert!(
        adapter
            .dispatch_with_shaper(
                &mut context,
                document,
                &click(row.bounds.x + 2.0, row.bounds.y + 2.0, PointerPhase::Down),
                Duration::ZERO,
                Some(&mut shaper),
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(textarea_selection(&context, node), ("beta".into(), 4, 4));

    // 重喂十条候选并重建几何：滚轮落在弹层面板内滚动弹层（消费），
    // 不落到编辑器滚动。
    context
        .update_component(area, |view, _| {
            view.completions =
                completion_items(&["a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10"]);
            view.state.selection = crate::TextSelection::caret(4);
        })
        .unwrap();
    shape_completion_editor(&mut context, document, node);
    let popup = completion_popup_geometry(&context, node);
    let scroll = |context: &AppContext| {
        context
            .world()
            .text_completion_snapshot(node)
            .map(|snapshot| snapshot.scroll)
    };
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &wheel(popup.panel.x + 3.0, popup.panel.y + 3.0, 3.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(scroll(&context), Some(1));
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &wheel(popup.panel.x + 3.0, popup.panel.y + 3.0, -3.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(scroll(&context), Some(0));

    // hover 浮窗滚轮：正文按行滚动并被消费。
    context
        .update_component(area, |view, _| {
            view.hover = Some(crate::TextHover::new(0, "beta", "one\ntwo\nthree"));
        })
        .unwrap();
    shape_completion_editor(&mut context, document, node);
    let hover_panel = match context.world().component_geometry(node) {
        Some(crate::ComponentGeometry::TextInput { hover_popup, .. }) => {
            hover_popup.expect("hover popup").panel
        }
        _ => panic!("text input geometry"),
    };
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &wheel(hover_panel.x + 3.0, hover_panel.y + 3.0, 3.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text_hover_scroll(node), 1);
}

#[test]
fn hover_wheel_scrolls_without_editor_focus() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("alpha beta").hover(Some(crate::TextHover::new(
                6,
                "beta",
                "one\ntwo\nthree",
            ))),
        )
        .unwrap();
    let node = area.stable_id();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        node,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 140.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    shape_completion_editor(&mut context, document, node);
    let hover_panel = match context.world().component_geometry(node) {
        Some(crate::ComponentGeometry::TextInput { hover_popup, .. }) => {
            hover_popup.expect("hover popup").panel
        }
        _ => panic!("text input geometry"),
    };
    let mut adapter = TestInput::default();

    // 编辑器未聚焦：滚轮落在 hover 面板内仍滚动该面板（命中测试驱动，
    // hover 显示不要求焦点）。
    assert!(
        adapter
            .dispatch(
                &mut context,
                document,
                &wheel(hover_panel.x + 3.0, hover_panel.y + 3.0, 3.0)
            )
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text_hover_scroll(node), 1);

    // 面板外：不消费，落回编辑器/文档滚动。
    assert!(
        !adapter
            .dispatch(&mut context, document, &wheel(-50.0, -50.0, 3.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(context.world().text_hover_scroll(node), 1);
}

/// ← / → on a viewer that shows a gallery ask for the neighbouring image,
/// from the viewer or from a control inside it; at an end there is nowhere
/// to go and the key passes on.
#[cfg(feature = "image-viewer")]
#[test]
fn arrow_keys_step_through_an_image_viewer_gallery() {
    use crate::{ImageViewer, ImageViewerContent, ImageViewerEvent};
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let viewer = context
        .create_component(
            document,
            ImageViewer::new(ImageViewerContent::None).gallery(0, 3),
        )
        .unwrap();
    context.assemble_image_viewer(viewer).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&events);
    context
        .on(viewer, move |_viewer, event: &ImageViewerEvent, _cx| {
            observed.lock().unwrap().push(*event);
        })
        .unwrap();
    assert!(context.focus_node(document, viewer.stable_id()).unwrap());
    let mut adapter = TestInput::default();
    let mut press = |context: &mut AppContext, key: &str| {
        adapter
            .dispatch(context, document, &plain_key(key))
            .unwrap()
            .handled
    };
    // The first image: nothing before it.
    assert!(!press(&mut context, "ArrowLeft"));
    assert!(press(&mut context, "ArrowRight"));
    context
        .update_component(viewer, |viewer, _| {
            viewer.gallery = Some(crate::ImageViewerPosition::new(2, 3))
        })
        .unwrap();
    let close = context
        .world()
        .node(viewer.stable_id())
        .unwrap()
        .children
        .last()
        .copied()
        .unwrap();
    assert!(context.focus_node(document, close).unwrap());
    assert!(press(&mut context, "ArrowLeft"));
    // The last image: nothing after it.
    assert!(!press(&mut context, "ArrowRight"));
    assert_eq!(
        *events.lock().unwrap(),
        [ImageViewerEvent::Next, ImageViewerEvent::Previous]
    );
}
