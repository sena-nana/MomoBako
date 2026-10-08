use super::*;
use crate::{LayoutBox, Stack, Text};
use std::sync::{Arc, Mutex};

fn document() -> DocumentId {
    DocumentId::new(1).unwrap()
}

#[test]
fn keyed_input_props_preserve_selection_and_preedit_until_value_changes() {
    let mut cx = AppContext::new();
    let parent = cx.create_component(document(), Stack::column(0.0)).unwrap();
    let mut input = None;
    cx.mount(parent, |ui| {
        input = Some(ui.child("input", TextInput::new("12345"))?);
        Ok(())
    })
    .unwrap();
    let input = input.unwrap();
    assert!(cx.focus_node(document(), input.id).unwrap());
    cx.update_component(input, |field, _| {
        field.state.selection = TextSelection::new(1, 3)
    })
    .unwrap();
    let mut mutations = MutationQueue::new();
    mutations.set_ime(
        input.id,
        Some(crate::ImeComposition {
            text: "拼".into(),
            selection: None,
        }),
    );
    cx.commit_mutations(mutations).unwrap();
    cx.mount(parent, |ui| {
        ui.child("input", TextInput::new("12345").label("room"))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        cx.read(input, |field| field.state.selection).unwrap(),
        TextSelection::new(1, 3)
    );
    assert!(cx.world.ime(input.id).is_some());
    cx.mount(parent, |ui| {
        ui.child("input", TextInput::new("999"))?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        cx.read(input, |field| field.state.selection).unwrap(),
        TextSelection::caret(3)
    );
}

#[test]
fn keyed_scroll_props_preserve_drag_during_background_refresh() {
    let mut cx = AppContext::new();
    let parent = cx.create_component(document(), Stack::column(0.0)).unwrap();
    let mut scroll = None;
    cx.mount(parent, |ui| {
        scroll = Some(ui.child("scroll", ScrollView::new(ScrollAxes::Vertical))?);
        Ok(())
    })
    .unwrap();
    let scroll = scroll.unwrap();
    let drag = crate::ScrollbarDragState {
        pointer_id: 9,
        axis: nana_ui_core::ScrollbarAxis::Vertical,
        grab_offset: 2.0,
        initial_offset: ScrollOffset { x: 0.0, y: 20.0 },
    };
    cx.update_component(scroll, |view, _| {
        view.hovered = true;
        view.dragging = Some(drag);
    })
    .unwrap();
    cx.mount(parent, |ui| {
        ui.child(
            "scroll",
            ScrollView::new(ScrollAxes::Vertical).label("updated"),
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        cx.read(scroll, |view| (view.hovered, view.dragging))
            .unwrap(),
        (true, Some(drag))
    );
}

#[test]
fn rebuilding_the_same_keyed_tree_replaces_handlers_instead_of_stacking_them() {
    let mut cx = AppContext::new();
    let root = cx.create_component(document(), Stack::column(0.0)).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));

    // The same keyed build, run twice against a stable parent — the shape an
    // app repeats when it refreshes a region.
    let build_once = |cx: &mut AppContext, tag: u8| {
        let out = seen.clone();
        cx.build_child(root, move |ui| {
            let button = ui.child("save", Button::new("save"));
            ui.on(button, move |_, _: &Activate, _| {
                out.lock().unwrap().push(tag)
            });
            button
        })
        .unwrap()
    };

    let first = build_once(&mut cx, 1);
    let second = build_once(&mut cx, 2);
    assert_eq!(first.id, second.id, "keyed child is reused across builds");

    cx.update(second, |_, event| event.emit(Activate)).unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        vec![2],
        "the rebuilt handler replaces the first, so Activate fires once"
    );
}

#[test]
fn one_build_may_register_several_handlers_for_the_same_node_and_event() {
    let mut cx = AppContext::new();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (first, second) = (seen.clone(), seen.clone());
    let button = cx
        .build(document(), move |ui| {
            let button = ui.child("save", Button::new("save"));
            ui.on(button, move |_, _: &Activate, _| {
                first.lock().unwrap().push(1)
            });
            ui.on(button, move |_, _: &Activate, _| {
                second.lock().unwrap().push(2)
            });
            button
        })
        .unwrap();

    cx.update(button, |_, event| event.emit(Activate)).unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![1, 2]);
}

#[test]
fn keyed_binding_replaces_callback_and_preserves_additive_subscriptions() {
    let mut cx = AppContext::new();
    let button = cx.create_component(document(), Button::new("go")).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let out = seen.clone();
    cx.on(button, move |_, _: &Activate, _| {
        out.lock().unwrap().push(0)
    })
    .unwrap();
    for value in [1, 2] {
        let out = seen.clone();
        cx.on_keyed(button, "action", move |_, _: &Activate, _| {
            out.lock().unwrap().push(value)
        })
        .unwrap();
    }
    cx.update(button, |_, event| event.emit(Activate)).unwrap();
    assert_eq!(*seen.lock().unwrap(), vec![0, 2]);
}

#[test]
fn scroll_retention_follows_new_extent_and_restores_row_without_user_event() {
    let mut cx = AppContext::new();
    let scroll = cx
        .create_component(
            document(),
            ScrollView::new(ScrollAxes::Vertical).follow_end(true),
        )
        .unwrap();
    let mut row = None;
    cx.mount(scroll, |ui| {
        row = Some(ui.child("row", Text::new("message"))?);
        Ok(())
    })
    .unwrap();
    let row = row.unwrap();
    let seen = Arc::new(Mutex::new(0));
    let out = seen.clone();
    cx.on(scroll, move |_, _: &crate::UserScroll, _| {
        *out.lock().unwrap() += 1
    })
    .unwrap();
    let mut mutations = MutationQueue::new();
    mutations.write_layout(
        scroll.id,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    );
    mutations.write_layout(
        row.id,
        LayoutBox {
            x: 0.0,
            y: 120.0,
            width: 100.0,
            height: 30.0,
        },
    );
    cx.commit_mutations(mutations).unwrap();
    cx.set_scroll_metrics(
        scroll,
        ScrollMetrics {
            viewport_width: 100.0,
            viewport_height: 100.0,
            content_width: 100.0,
            content_height: 200.0,
            origin_x: 0.0,
            origin_y: 0.0,
        },
    )
    .unwrap();
    cx.apply_scroll_retention(scroll).unwrap();
    assert_eq!(cx.world.scroll_offset(scroll.id).unwrap().y, 100.0);
    let anchor = cx.capture_scroll_anchor(scroll, row.id).unwrap().unwrap();
    assert_eq!(anchor.viewport_y, 20.0);
    cx.restore_scroll_anchor(scroll, anchor).unwrap();
    let mut mutations = MutationQueue::new();
    mutations.write_layout(
        row.id,
        LayoutBox {
            x: 0.0,
            y: 160.0,
            width: 100.0,
            height: 30.0,
        },
    );
    cx.commit_mutations(mutations).unwrap();
    cx.set_scroll_metrics(
        scroll,
        ScrollMetrics {
            viewport_width: 100.0,
            viewport_height: 100.0,
            content_width: 100.0,
            content_height: 300.0,
            origin_x: 0.0,
            origin_y: 0.0,
        },
    )
    .unwrap();
    cx.apply_scroll_retention(scroll).unwrap();
    assert_eq!(cx.world.scroll_offset(scroll.id).unwrap().y, 140.0);
    assert_eq!(*seen.lock().unwrap(), 0);
    cx.scroll_node_by(scroll.id, ScrollOffset { x: 0.0, y: -10.0 })
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), 1);
    cx.apply_scroll_retention(scroll).unwrap();
    assert_eq!(cx.world.scroll_offset(scroll.id).unwrap().y, 200.0);
}

#[test]
fn single_line_submission_preserves_value_and_excludes_ime_confirmation() {
    let mut cx = AppContext::new();
    let input = cx
        .create_component(document(), TextInput::new("1234"))
        .unwrap();
    cx.focus_node(document(), input.id).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let out = seen.clone();
    cx.on(input, move |_, event: &crate::TextSubmitted, _| {
        out.lock().unwrap().push(event.value.clone())
    })
    .unwrap();
    assert!(cx.submit_focused_text_input(document()).unwrap());
    cx.set_ime_preedit(document(), "拼".into(), None).unwrap();
    assert!(!cx.submit_focused_text_input(document()).unwrap());
    assert_eq!(*seen.lock().unwrap(), vec!["1234"]);
    assert_eq!(
        cx.read(input, |input| input.state.value.clone()).unwrap(),
        "1234"
    );
}

#[test]
fn explicit_follow_end_supersedes_a_pending_reading_anchor_without_layout() {
    let mut cx = AppContext::new();
    let scroll = cx
        .create_component(document(), ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    let mut row = None;
    cx.mount(scroll, |ui| {
        row = Some(ui.child("row", Text::new("message"))?);
        Ok(())
    })
    .unwrap();
    cx.set_scroll_metrics(
        scroll,
        ScrollMetrics {
            viewport_width: 100.0,
            viewport_height: 100.0,
            content_width: 100.0,
            content_height: 500.0,
            origin_x: 0.0,
            origin_y: 0.0,
        },
    )
    .unwrap();
    cx.restore_scroll_anchor(
        scroll,
        crate::ScrollAnchor {
            row: row.unwrap().id,
            viewport_y: 20.0,
        },
    )
    .unwrap();
    cx.set_scroll_follow_end(scroll, true).unwrap();
    assert_eq!(cx.world.scroll_offset(scroll.id).unwrap().y, 400.0);
    assert!(
        cx.read(scroll, |view| view.pending_anchor.is_none())
            .unwrap()
    );
}
