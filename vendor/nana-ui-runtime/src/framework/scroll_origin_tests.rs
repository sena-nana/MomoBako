//! Scroll origin on axes that start at the right / bottom (CSSOM View
//! "scrolling area"): the initial view shows the start edge, the content
//! overflowing the other way is reachable through negative offsets, and the
//! overflow past the start edge is not.

use super::*;
use crate::{LayoutBox, NodeStyle, Text};
use nana_ui_core::{DirSpec, DisplaySpec, FlexDirection, OverflowSpec, WritingModeSpec};

const DOCUMENT: u64 = 1;

fn id(value: u64) -> StableNodeId {
    StableNodeId::new(value).unwrap()
}

fn document() -> DocumentId {
    DocumentId::new(DOCUMENT).unwrap()
}

fn style(layout: nana_ui_core::LayoutStyle) -> NodeStyle {
    NodeStyle {
        layout: Arc::new(layout),
        ..NodeStyle::default()
    }
}

/// A 200x100 L1 `overflow: auto` scroller (id 1) holding `count` fixed
/// items (ids 2..), each `item` wide by `item` high, laid out for real.
fn scroller(container: nana_ui_core::LayoutStyle, count: u64, item: f32) -> AppContext {
    let mut context = AppContext::new();
    let mut mutations = MutationQueue::new();
    mutations.create(id(1), document(), NodeKind::Element { tag: "div".into() });
    mutations.set_style(
        id(1),
        style(nana_ui_core::LayoutStyle {
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(100.0)),
            overflow_x: OverflowSpec::Auto,
            overflow_y: OverflowSpec::Auto,
            ..container
        }),
    );
    for index in 0..count {
        let child = id(index + 2);
        mutations.create(child, document(), NodeKind::Element { tag: "div".into() });
        mutations.set_style(
            child,
            style(nana_ui_core::LayoutStyle {
                width: Some(LengthSpec::Px(item)),
                height: Some(LengthSpec::Px(item)),
                flex_shrink: Some(0.0),
                ..nana_ui_core::LayoutStyle::default()
            }),
        );
        mutations.insert(id(1), child, None);
    }
    context.commit_mutations(mutations).unwrap();
    context
        .layout_document(document(), crate::LayoutViewport::new(200.0, 100.0))
        .unwrap();
    context.take_system_work();
    context.rebuild_hit_test(document());
    context
}

fn wheel(context: &mut AppContext, x: f32, y: f32) -> ScrollOffset {
    context
        .scroll_at(document(), 10.0, 10.0, ScrollOffset { x, y })
        .unwrap();
    context.rebuild_hit_test(document());
    context.world().scroll_offset(id(1)).unwrap()
}

fn row(reverse: bool, dir: Option<DirSpec>) -> nana_ui_core::LayoutStyle {
    nana_ui_core::LayoutStyle {
        display: Some(DisplaySpec::Flex),
        direction: Some(FlexDirection::Row),
        flex_reverse: reverse,
        dir,
        ..nana_ui_core::LayoutStyle::default()
    }
}

/// `direction: rtl` row of five 100px items in a 200px scrollport: the first
/// item sits at the right edge and the rest overflow to the left.
#[test]
fn an_rtl_row_scrolls_left_through_negative_offsets_from_its_start_edge() {
    let mut context = scroller(row(false, Some(DirSpec::Rtl)), 5, 100.0);
    assert_eq!(context.world().layout_box(id(2)).unwrap().x, 100.0);
    assert_eq!(context.world().layout_box(id(6)).unwrap().x, -300.0);
    assert_eq!(context.world().scroll_offset(id(1)).unwrap().x, 0.0);
    // Layout measures every scroll container, not only on the first wheel.
    assert_eq!(
        context.world().scroll_metrics(id(1)).map(|m| m.origin_x),
        Some(-300.0)
    );
    // The initial view is the start (right) edge: the first item is there.
    assert_eq!(
        context.world().hit_test(document(), 150.0, 50.0),
        Some(id(2))
    );

    // A programmatic offset is clamped to the same area.
    let mut far = MutationQueue::new();
    far.set_scroll_offset(
        id(1),
        ScrollOffset {
            x: -1_000.0,
            y: 0.0,
        },
    );
    context.commit_mutations(far).unwrap();
    assert_eq!(context.world().scroll_offset(id(1)).unwrap().x, -300.0);
    let mut back = MutationQueue::new();
    back.set_scroll_offset(id(1), ScrollOffset::default());
    context.commit_mutations(back).unwrap();

    // Scrolling right has nowhere to go: that is the start edge.
    assert!(
        context
            .scroll_at(document(), 10.0, 10.0, ScrollOffset { x: 50.0, y: 0.0 })
            .unwrap()
            .is_none()
    );
    let metrics = context.world().scroll_metrics(id(1)).unwrap();
    assert_eq!(metrics.content_width, 500.0);
    assert_eq!(metrics.origin_x, -300.0);
    assert_eq!(metrics.origin_y, 0.0);
    assert_eq!(metrics.max_offset().x, 0.0);

    assert_eq!(wheel(&mut context, -120.0, 0.0).x, -120.0);
    assert_eq!(wheel(&mut context, -1_000.0, 0.0).x, -300.0);
    // The far (left) end: the last item now fills the scrollport's left half.
    assert_eq!(
        context.world().hit_test(document(), 50.0, 50.0),
        Some(id(6))
    );
    assert_eq!(wheel(&mut context, 1_000.0, 0.0).x, 0.0);
}

/// `vertical-rl` stacks blocks from the right: four 80px-wide blocks in a
/// 200px-wide document overflow 120px to the left, while the inline axis
/// (top to bottom) keeps its origin at the top.
#[test]
fn a_vertical_rl_document_wider_than_its_viewport_scrolls_to_the_left() {
    let mut context = scroller(
        nana_ui_core::LayoutStyle {
            writing_mode: Some(WritingModeSpec::VerticalRl),
            ..nana_ui_core::LayoutStyle::default()
        },
        4,
        80.0,
    );
    assert_eq!(context.world().layout_box(id(2)).unwrap().x, 120.0);
    assert_eq!(context.world().layout_box(id(5)).unwrap().x, -120.0);
    assert_eq!(
        context.world().hit_test(document(), 150.0, 40.0),
        Some(id(2))
    );

    assert_eq!(wheel(&mut context, -500.0, 0.0).x, -120.0);
    let metrics = context.world().scroll_metrics(id(1)).unwrap();
    assert_eq!(metrics.content_width, 320.0);
    assert_eq!(metrics.origin_x, -120.0);
    assert_eq!(metrics.origin_y, 0.0);
    assert_eq!(
        context.world().hit_test(document(), 40.0, 40.0),
        Some(id(5))
    );
}

/// `flex-direction: row-reverse` in an LTR container also starts at the
/// right, and flips back with `direction: rtl`.
#[test]
fn a_row_reverse_overflow_scrolls_left_and_rtl_flips_it_back() {
    let mut context = scroller(row(true, None), 3, 100.0);
    assert_eq!(context.world().layout_box(id(2)).unwrap().x, 100.0);
    assert_eq!(wheel(&mut context, -500.0, 0.0).x, -100.0);
    assert_eq!(
        context.world().scroll_metrics(id(1)).unwrap().origin_x,
        -100.0
    );
    assert_eq!(
        context.world().hit_test(document(), 50.0, 50.0),
        Some(id(4))
    );

    let mut context = scroller(row(true, Some(DirSpec::Rtl)), 3, 100.0);
    assert_eq!(context.world().layout_box(id(2)).unwrap().x, 0.0);
    assert_eq!(wheel(&mut context, 500.0, 0.0).x, 100.0);
    assert_eq!(context.world().scroll_metrics(id(1)).unwrap().origin_x, 0.0);
}

/// `column-reverse` starts at the bottom and overflows upward.
#[test]
fn a_column_reverse_overflow_scrolls_up_through_negative_offsets() {
    let mut context = scroller(
        nana_ui_core::LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Column),
            flex_reverse: true,
            ..nana_ui_core::LayoutStyle::default()
        },
        3,
        50.0,
    );
    assert_eq!(context.world().layout_box(id(2)).unwrap().y, 50.0);
    assert_eq!(wheel(&mut context, 0.0, -500.0).y, -50.0);
    let metrics = context.world().scroll_metrics(id(1)).unwrap();
    assert_eq!((metrics.origin_x, metrics.origin_y), (0.0, -50.0));
    assert_eq!(
        context.world().hit_test(document(), 25.0, 25.0),
        Some(id(4))
    );
}

/// An RTL `ScrollView`: the thumb starts at the right end of the track, a
/// drag to the left end reaches the origin, and `scroll_into_view` reaches
/// items on the overflowing side.
#[test]
fn an_rtl_scroll_view_puts_its_thumb_at_the_start_edge() {
    let mut context = AppContext::new();
    let scroll = context
        .create_component(
            document(),
            ScrollView::new(ScrollAxes::Horizontal)
                .scrollbars(nana_ui_core::ScrollbarVisibility::Always)
                .style(style(nana_ui_core::LayoutStyle {
                    width: Some(LengthSpec::Px(200.0)),
                    height: Some(LengthSpec::Px(100.0)),
                    ..row(false, Some(DirSpec::Rtl))
                })),
        )
        .unwrap();
    let mut items = Vec::new();
    for index in 0..4 {
        let item = context
            .create_component(
                document(),
                Text::new(format!("Item {index}")).style(style(nana_ui_core::LayoutStyle {
                    width: Some(LengthSpec::Px(100.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    flex_shrink: Some(0.0),
                    ..nana_ui_core::LayoutStyle::default()
                })),
            )
            .unwrap();
        context.append_child(scroll, item).unwrap();
        items.push(item.stable_id());
    }
    context
        .layout_document(document(), crate::LayoutViewport::new(200.0, 100.0))
        .unwrap();
    let metrics = context.world().scroll_metrics(scroll.stable_id()).unwrap();
    assert_eq!((metrics.origin_x, metrics.content_width), (-200.0, 400.0));
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()).unwrap().x,
        0.0
    );

    let bar = match context.world().component_geometry(scroll.stable_id()) {
        Some(crate::ComponentGeometry::Scrollbar {
            horizontal: Some(bar),
            ..
        }) => bar,
        other => panic!("horizontal bar expected, got {other:?}"),
    };
    assert_eq!((bar.min_offset, bar.max_offset), (-200.0, 0.0));
    assert!(
        (bar.thumb.x + bar.thumb.width - (bar.track.x + bar.track.width)).abs() < 0.01,
        "the thumb starts at the right end: {bar:?}"
    );
    let track = bar.track_geometry(nana_ui_core::ScrollbarAxis::Horizontal);
    assert_eq!(track.offset_for_thumb_origin(track.origin), -200.0);

    assert!(context.scroll_into_view(scroll, items[3], 0.0).unwrap());
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()).unwrap().x,
        -200.0
    );
    assert!(context.scroll_into_view(scroll, items[0], 0.0).unwrap());
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()).unwrap().x,
        0.0
    );
}

/// Whoever changes what a scroll container holds — a commit that writes
/// boxes, removes children or restyles it, with no layout pass — leaves it
/// measured: the scrolling area follows and the offset is clamped into it.
#[test]
fn a_commit_that_changes_the_content_re_measures_its_scroll_container() {
    let mut context = scroller(row(false, Some(DirSpec::Rtl)), 5, 100.0);
    assert_eq!(wheel(&mut context, -1_000.0, 0.0).x, -300.0);

    let mut shrink = MutationQueue::new();
    for value in 4..7 {
        shrink.despawn_subtree(id(value));
    }
    context.commit_mutations(shrink).unwrap();
    let metrics = context.world().scroll_metrics(id(1)).unwrap();
    assert_eq!((metrics.origin_x, metrics.content_width), (0.0, 200.0));
    assert_eq!(context.world().scroll_offset(id(1)).unwrap().x, 0.0);

    // A host writing a box past the left edge widens the area again.
    let mut grow = MutationQueue::new();
    grow.write_layout(
        id(3),
        LayoutBox {
            x: -150.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    );
    context.commit_mutations(grow).unwrap();
    assert_eq!(
        context.world().scroll_metrics(id(1)).unwrap().origin_x,
        -150.0
    );

    // Turned back to LTR, the next layout places the items from the left,
    // and the origin follows the placement to the left edge — a restyle
    // alone leaves the boxes, and so the origin, where they were.
    let mut ltr = MutationQueue::new();
    ltr.set_style(
        id(1),
        style(nana_ui_core::LayoutStyle {
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(100.0)),
            overflow_x: OverflowSpec::Auto,
            overflow_y: OverflowSpec::Auto,
            ..row(false, Some(DirSpec::Ltr))
        }),
    );
    context.commit_mutations(ltr).unwrap();
    assert_eq!(
        context.world().scroll_metrics(id(1)).unwrap().origin_x,
        -150.0
    );
    context
        .layout_document(document(), crate::LayoutViewport::new(200.0, 100.0))
        .unwrap();
    assert_eq!(context.world().layout_box(id(2)).unwrap().x, 0.0);
    let metrics = context.world().scroll_metrics(id(1)).unwrap();
    assert_eq!((metrics.origin_x, metrics.content_width), (0.0, 200.0));
}

/// Nested scroll containers both follow a box written deep inside them: the
/// outer one's re-measure refreshes the inner one's content index on the
/// way, which must not make the inner one look clean.
#[test]
fn a_box_written_inside_nested_scroll_containers_re_measures_both() {
    let mut context = AppContext::new();
    let scrolling = style(nana_ui_core::LayoutStyle {
        overflow_x: OverflowSpec::Auto,
        overflow_y: OverflowSpec::Auto,
        ..nana_ui_core::LayoutStyle::default()
    });
    let at = |x: f32, y: f32, width: f32, height: f32| LayoutBox {
        x,
        y,
        width,
        height,
    };
    let mut build = MutationQueue::new();
    for (value, parent) in [(1, None), (2, Some(1)), (3, Some(2))] {
        build.create(
            id(value),
            document(),
            NodeKind::Element { tag: "div".into() },
        );
        if let Some(parent) = parent {
            build.insert(id(parent), id(value), None);
        }
    }
    build.set_style(id(1), scrolling.clone());
    build.set_style(id(2), scrolling);
    build.write_layout(id(1), at(0.0, 0.0, 200.0, 100.0));
    build.write_layout(id(2), at(0.0, 0.0, 200.0, 100.0));
    build.write_layout(id(3), at(0.0, 0.0, 200.0, 150.0));
    context.commit_mutations(build).unwrap();
    for outer_or_inner in [id(1), id(2)] {
        assert_eq!(
            context
                .world()
                .scroll_metrics(outer_or_inner)
                .unwrap()
                .content_height,
            150.0
        );
    }

    let mut grow = MutationQueue::new();
    grow.write_layout(id(3), at(0.0, 0.0, 200.0, 400.0));
    context.commit_mutations(grow).unwrap();
    for outer_or_inner in [id(1), id(2)] {
        assert_eq!(
            context
                .world()
                .scroll_metrics(outer_or_inner)
                .unwrap()
                .content_height,
            400.0
        );
    }

    // A container that stops scrolling drops its scrolling area.
    let mut plain = MutationQueue::new();
    plain.set_style(id(2), NodeStyle::default());
    context.commit_mutations(plain).unwrap();
    assert_eq!(context.world().scroll_metrics(id(2)), None);
}

/// An offset restored in the same commit as the content it scrolls to is
/// clamped to the scrolling area that commit leaves, not the one before it.
#[test]
fn an_offset_set_with_the_content_it_reaches_is_kept() {
    let mut context = scroller(row(false, Some(DirSpec::Rtl)), 3, 100.0);
    assert_eq!(
        context.world().scroll_metrics(id(1)).unwrap().origin_x,
        -100.0
    );
    let mut restore = MutationQueue::new();
    restore.set_scroll_offset(id(1), ScrollOffset { x: -250.0, y: 0.0 });
    restore.write_layout(
        id(4),
        LayoutBox {
            x: -300.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    );
    context.commit_mutations(restore).unwrap();
    assert_eq!(context.world().scroll_offset(id(1)).unwrap().x, -250.0);
}
