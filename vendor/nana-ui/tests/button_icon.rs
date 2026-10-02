use nana_ui::{HeadlessInput, NanaTextShaper, runtime::*};
use nana_ui_core::{Icon, LengthSpec};
use nana_ui_platform::{InputModifiers, InputPayload, KeyInput, KeyState};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn layout(cx: &mut AppContext, doc: DocumentId, ids: &[StableNodeId]) {
    cx.resolve_styles(ids).unwrap();
    cx.shape_text(ids, &mut NanaTextShaper::default()).unwrap();
    cx.layout_document(doc, LayoutViewport::new(600.0, 400.0))
        .unwrap();
    cx.rebuild_hit_test(doc);
}

#[test]
fn button_leading_icon_measures_real_text_and_centers_the_group() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx
        .create_component(
            doc,
            Stack::column(8.0).align(nana_ui_core::AlignSpec::Start),
        )
        .unwrap();
    let plain = cx
        .create_detached_component(doc, Button::new("保存记忆"))
        .unwrap();
    let icon = cx
        .create_detached_component(
            doc,
            Button::new("保存记忆")
                .icon(Icon::File)
                .icon_size(15.0)
                .icon_gap(6.0),
        )
        .unwrap();
    cx.append_child(root, plain).unwrap();
    cx.append_child(root, icon).unwrap();
    let ids = [root.stable_id(), plain.stable_id(), icon.stable_id()];
    layout(&mut cx, doc, &ids);
    let a = cx.world().layout_box(plain.stable_id()).unwrap();
    let b = cx.world().layout_box(icon.stable_id()).unwrap();
    assert!((b.width - a.width - 21.0).abs() < 0.1, "{a:?} {b:?}");
    for width in [200.0, 64.0] {
        cx.update_component(icon, |view, _| {
            let style = Arc::make_mut(&mut view.style.layout);
            style.width = Some(LengthSpec::Px(width));
            style.font_size = Some(18.0);
            style.font_weight = Some(700);
        })
        .unwrap();
        layout(&mut cx, doc, &ids);
        let outer = cx.world().layout_box(icon.stable_id()).unwrap();
        let ComponentGeometry::Button {
            icon: Some((glyph, rect)),
            label,
            spinner,
            ..
        } = cx.world().component_geometry(icon.stable_id()).unwrap()
        else {
            panic!("icon geometry")
        };
        assert_eq!(glyph, Icon::File);
        assert_eq!(label.font_size, 18.0);
        assert_eq!(label.font_weight, Some(700));
        assert_eq!(rect.width, 15.0);
        assert!(spinner.is_none());
        assert!((label.bounds.x - rect.x - rect.width - 6.0).abs() < 0.1);
        assert!(
            ((rect.x + label.bounds.x + label.bounds.width) / 2.0 - (outer.x + outer.width / 2.0))
                .abs()
                < 0.1
        );
        assert!(label.bounds.x + label.bounds.width <= outer.x + outer.width);
    }
}

#[test]
fn button_trailing_icon_follows_the_label_inside_one_centred_group() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx
        .create_component(
            doc,
            Stack::column(8.0).align(nana_ui_core::AlignSpec::Start),
        )
        .unwrap();
    let leading = cx
        .create_detached_component(
            doc,
            Button::new("舞台 1")
                .icon(Icon::File)
                .icon_size(15.0)
                .icon_gap(6.0),
        )
        .unwrap();
    let both = cx
        .create_detached_component(
            doc,
            Button::new("舞台 1")
                .icon(Icon::File)
                .trailing_icon(Icon::ChevronDown)
                .icon_size(15.0)
                .icon_gap(6.0),
        )
        .unwrap();
    cx.append_child(root, leading).unwrap();
    cx.append_child(root, both).unwrap();
    let ids = [root.stable_id(), leading.stable_id(), both.stable_id()];
    layout(&mut cx, doc, &ids);
    let a = cx.world().layout_box(leading.stable_id()).unwrap();
    let b = cx.world().layout_box(both.stable_id()).unwrap();
    assert!((b.width - a.width - 21.0).abs() < 0.1, "{a:?} {b:?}");

    for width in [240.0, 70.0] {
        cx.update_component(both, |view, _| {
            Arc::make_mut(&mut view.style.layout).width = Some(LengthSpec::Px(width));
        })
        .unwrap();
        layout(&mut cx, doc, &ids);
        let outer = cx.world().layout_box(both.stable_id()).unwrap();
        let ComponentGeometry::Button {
            icon: Some((_, lead)),
            trailing_icon: Some((glyph, trail)),
            label,
            ..
        } = cx.world().component_geometry(both.stable_id()).unwrap()
        else {
            panic!("both glyphs")
        };
        assert_eq!(glyph, Icon::ChevronDown);
        assert_eq!(trail.width, 15.0, "the label gives up width first");
        assert!((label.bounds.x - lead.x - lead.width - 6.0).abs() < 0.1);
        assert!((trail.x - label.bounds.x - label.bounds.width - 6.0).abs() < 0.1);
        assert!(
            ((lead.x + trail.x + trail.width) / 2.0 - (outer.x + outer.width / 2.0)).abs() < 0.1,
            "the group is centred"
        );
        assert!(trail.x + trail.width <= outer.x + outer.width + 0.01);
    }
}

#[test]
fn button_loading_replaces_icon_and_preserves_normal_activation_gates() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let button = cx
        .create_component(
            doc,
            Button::new("创建")
                .icon(Icon::Add)
                .icon_size(14.0)
                .icon_gap(6.0),
        )
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    cx.on(button, move |_, _: &Activate, _| {
        observed.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    let ids = [button.stable_id()];
    let mut adapter = HeadlessInput::bind(&mut cx, doc);
    let enter = InputPayload::Key(KeyInput::named(
        "Enter",
        "Enter",
        KeyState::Pressed,
        InputModifiers::default(),
    ));
    layout(&mut cx, doc, &ids);
    let width = cx.world().layout_box(ids[0]).unwrap().width;
    cx.focus_node(doc, ids[0]).unwrap();
    adapter.route(&mut cx, enter.clone()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    cx.update_component(button, |view, _| view.loading = true)
        .unwrap();
    layout(&mut cx, doc, &ids);
    assert_eq!(cx.world().layout_box(ids[0]).unwrap().width, width);
    let ComponentGeometry::Button { icon, spinner, .. } =
        cx.world().component_geometry(ids[0]).unwrap()
    else {
        panic!("button")
    };
    assert!(icon.is_none());
    assert_eq!(spinner.unwrap().width, 14.0);
    adapter.route(&mut cx, enter.clone()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    cx.update_component(button, |view, _| {
        view.loading = false;
        view.disabled = true;
    })
    .unwrap();
    adapter.route(&mut cx, enter.clone()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    cx.update_component(button, |view, _| view.disabled = false)
        .unwrap();
    cx.focus_node(doc, ids[0]).unwrap();
    adapter.route(&mut cx, enter.clone()).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn button_slot_changes_invalidate_layout_but_spinner_phase_only_repaints() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let button = cx
        .create_component(doc, Button::new("保存").loading(true))
        .unwrap();
    cx.take_system_work();
    let mut visual = cx.world().standard_visual(button.stable_id()).unwrap();
    if let StandardVisual::Button { loading_phase, .. } = &mut visual {
        *loading_phase = 0.4;
    }
    let mut changes = MutationQueue::default();
    changes.set_standard_visual(button.stable_id(), Some(visual));
    cx.commit_mutations(changes).unwrap();
    let work = cx.take_system_work();
    assert!(work.layout.is_empty());
    assert!(work.text.is_empty());
    cx.update_component(button, |view, _| {
        view.loading = false;
        view.icon = None;
    })
    .unwrap();
    assert!(
        cx.compat_world_mut()
            .take_system_work()
            .layout
            .contains(&button.stable_id())
    );
}
