//! A modal surface takes every press on it, even one within a resize
//! handle's slop of an edge underneath.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use nana_ui_core::{LengthSpec, RegionId};

use super::*;
use crate::{
    Activate, Button, DesktopShell, Dialog, Entity, LayoutViewport, ModalSlots, OverlayHost,
    SidebarFrame, Text, Workspace,
};

fn press(
    input: &mut TestInput,
    cx: &mut AppContext,
    doc: DocumentId,
    phase: PointerPhase,
    x: f32,
    y: f32,
) {
    input
        .dispatch_at(
            cx,
            doc,
            &pointer_fixture! {
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
                activation_click: false,
                modifiers: Default::default(),
            },
            Duration::from_millis(100),
        )
        .unwrap();
}

#[test]
fn a_dialog_button_beside_a_sidebar_handle_takes_the_press() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let navigation = cx
        .create_detached_component(doc, SidebarFrame::new())
        .unwrap();
    let primary = cx
        .create_detached_component(doc, Text::new("primary"))
        .unwrap();
    let dialog = cx
        .create_detached_component(doc, Dialog::new("确认"))
        .unwrap();
    let mut wide = Button::new("好");
    Arc::make_mut(&mut wide.style.layout).width = Some(LengthSpec::Fill);
    let button = cx.create_detached_component(doc, wide).unwrap();
    cx.set_modal_slots(
        dialog,
        ModalSlots {
            body: Some(button.stable_id()),
            ..Default::default()
        },
    )
    .unwrap();
    let shell = cx
        .create_component(
            doc,
            DesktopShell::new()
                .navigation(navigation.stable_id())
                .primary(primary.stable_id()),
        )
        .unwrap();
    cx.update_component(shell, |shell, _| shell.overlays.push(dialog.stable_id()))
        .unwrap();
    cx.assemble_desktop_shell(shell).unwrap();
    let host = Entity::<OverlayHost>::from_stable_id(
        cx.read(shell, |shell| shell.overlay.unwrap()).unwrap(),
    );
    cx.activate_overlay(host, dialog).unwrap();
    let pressed = Arc::new(AtomicUsize::new(0));
    let heard = Arc::clone(&pressed);
    cx.on(button, move |_, _: &Activate, _| {
        heard.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    cx.layout_document(doc, LayoutViewport::new(700.0, 500.0))
        .unwrap();
    cx.advance_animations(Duration::from_secs(5));
    cx.layout_document(doc, LayoutViewport::new(700.0, 500.0))
        .unwrap();
    cx.rebuild_hit_test(doc);

    let workspace = Entity::<Workspace>::from_stable_id(
        cx.read(shell, |shell| shell.workspace.unwrap()).unwrap(),
    );
    let handle = cx
        .read(workspace, |workspace| {
            workspace.handles.get(&RegionId::Resources).copied()
        })
        .unwrap()
        .expect("the sidebar has a resize handle");
    let handle_box = cx.world().layout_box(handle).unwrap();
    let button_box = cx.world().layout_box(button.stable_id()).unwrap();
    // Just past the handle's edge, on the dialog's button.
    let (x, y) = (
        handle_box.x + handle_box.width + 2.0,
        button_box.y + button_box.height / 2.0,
    );
    assert!(
        button_box.x < x && x < button_box.x + button_box.width,
        "the button {button_box:?} spans the handle {handle_box:?}"
    );
    assert_eq!(cx.world().hit_test(doc, x, y), Some(button.stable_id()));

    let mut input = TestInput::default();
    press(&mut input, &mut cx, doc, PointerPhase::Down, x, y);
    assert_ne!(
        cx.world().pointer_capture(doc, 1),
        Some(handle),
        "the handle under the dialog does not start a resize"
    );
    press(&mut input, &mut cx, doc, PointerPhase::Up, x, y);
    assert_eq!(
        pressed.load(Ordering::SeqCst),
        1,
        "the dialog's button was pressed"
    );
}
