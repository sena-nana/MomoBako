//! A segmented option's icon has no box of its own — it is drawn inside the
//! leading inset — so layout and paint have to agree on what that inset costs.
//! The settings 「主题」 row is what caught the disagreement: `暗色` measured one
//! inset short of its own label and rendered as `暗…`.

use nana_ui::{NanaTextShaper, runtime::*};
use nana_ui_core::{AlignSpec, ControlSize, Icon, UI_METRICS, space};

fn layout(cx: &mut AppContext, doc: DocumentId, ids: &[StableNodeId]) {
    cx.resolve_styles(ids).unwrap();
    cx.shape_text(ids, &mut NanaTextShaper::default()).unwrap();
    cx.layout_document(doc, LayoutViewport::new(600.0, 400.0))
        .unwrap();
}

fn label_box(cx: &AppContext, id: StableNodeId) -> LayoutBox {
    let Some(ComponentGeometry::SelectionOption { label, .. }) = cx.world().component_geometry(id)
    else {
        panic!("selection option geometry")
    };
    label.bounds
}

#[test]
fn segmented_icon_option_pays_for_its_icon_and_keeps_the_whole_label() {
    let mut cx = AppContext::new();
    let doc = DocumentId::new(1).unwrap();
    let root = cx
        .create_component(doc, Stack::column(8.0).align(AlignSpec::Start))
        .unwrap();
    let control = cx
        .create_detached_component(doc, SegmentedControl::new())
        .unwrap();
    let plain = cx
        .create_detached_component(doc, SegmentedOption::new("暗色"))
        .unwrap();
    let with_icon = cx
        .create_detached_component(doc, SegmentedOption::new("暗色").icon(Icon::Moon))
        .unwrap();
    cx.append_child(root, control).unwrap();
    cx.set_segmented_options(control, vec![plain, with_icon], Some(with_icon))
        .unwrap();
    let ids = [
        root.stable_id(),
        control.stable_id(),
        plain.stable_id(),
        with_icon.stable_id(),
    ];
    layout(&mut cx, doc, &ids);

    let size = ControlSize::Medium;
    let plain_bounds = cx.world().layout_box(plain.stable_id()).unwrap();
    let icon_bounds = cx.world().layout_box(with_icon.stable_id()).unwrap();
    // Same label, so the icon option is exactly one icon and one gap wider —
    // it does not pay for the icon out of the label's share of the pill.
    assert!(
        (icon_bounds.width - plain_bounds.width - size.icon_size() - space::XS).abs() < 0.1,
        "{plain_bounds:?} {icon_bounds:?}"
    );

    let plain_label = label_box(&cx, plain.stable_id());
    let icon_label = label_box(&cx, with_icon.stable_id());
    assert!(
        (icon_label.width - plain_label.width).abs() < 0.1,
        "{plain_label:?} {icon_label:?}"
    );
    // Both options keep the same trailing inset; an icon on the left is no
    // reason to run the label into the right edge.
    let inset = size.padding_x_in(UI_METRICS) + space::XXS;
    for (option, label) in [(plain_bounds, plain_label), (icon_bounds, icon_label)] {
        assert!(
            (option.x + option.width - label.x - label.width - inset).abs() < 0.1,
            "{option:?} {label:?}"
        );
    }
}
