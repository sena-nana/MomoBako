use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, BoxSizing, CalcBinOp, CalcExpr, ClearSpec, DirSpec, DisplaySpec, FlexDirection,
    FlexWrap, FloatSpec, GridLine, GridPlacement, GridRepeatAuto, GridTrack,
    GridTrackListUnsupported, JustifySpec, LayoutStyle, LengthSpec, LineHeightSpec, PositionSpec,
    TextOrientationSpec, WhiteSpaceSpec, WritingModeSpec,
};

use crate::{
    ComputedStyle, MutationQueue, NodeKind, NodeStyle, TextContent, TextMetrics, TextShaper,
    UiWorld,
};

use super::*;

fn id(value: u64) -> StableNodeId {
    StableNodeId::new(value).unwrap()
}

#[test]
fn isolated_leaf_preserves_relative_position_and_resolved_padding() {
    use crate::{AppContext, Stack};
    let document = DocumentId::new(42).unwrap();
    let mut context = AppContext::new();
    let root = context
        .create_component(document, Stack::column(0.0).width(LengthSpec::Px(200.0)))
        .unwrap();
    let leaf = context
        .create_detached_component(
            document,
            Stack::from_layout(LayoutStyle {
                width: Some(LengthSpec::Px(100.0)),
                height: Some(LengthSpec::Px(80.0)),
                padding: Some(LengthSpec::Percent(10.0)),
                position: PositionSpec::Relative,
                offset_left: Some(LengthSpec::Px(7.0)),
                layout_isolation: true,
                ..Default::default()
            }),
        )
        .unwrap();
    context.append_child(root, leaf).unwrap();
    let viewport = LayoutViewport::new(400.0, 300.0);
    context.layout_document(document, viewport).unwrap();
    let before = context.world().layout_box(leaf.stable_id()).unwrap();
    assert_eq!((before.x, before.width, before.height), (7.0, 100.0, 80.0));
    context
        .layout_document_scoped(document, viewport, &[leaf.stable_id()])
        .unwrap();
    assert_eq!(
        context.world().layout_box(leaf.stable_id()).unwrap(),
        before
    );
    let extracted = context.world().extract_nodes(&[leaf.stable_id()]);
    assert_eq!(
        extracted[0].source_style.layout.resolved_padding().left,
        20.0
    );
    context
        .update_component(root, |root, _| {
            *root = root.clone().width(LengthSpec::Px(300.0))
        })
        .unwrap();
    context
        .layout_document_scoped(document, viewport, &[root.stable_id()])
        .unwrap();
    assert_eq!(
        context.world().layout_box(leaf.stable_id()).unwrap(),
        before
    );
    let extracted = context.world().extract_nodes(&[leaf.stable_id()]);
    assert_eq!(
        extracted[0].source_style.layout.resolved_padding().left,
        30.0
    );
}

#[test]
fn scoped_layout_ignores_isolated_dirty_nodes_from_another_document() {
    let first = DocumentId::new(1).unwrap();
    let second = DocumentId::new(2).unwrap();
    let mut world = UiWorld::new();
    let mut mutations = MutationQueue::new();
    mutations.create(id(1), first, NodeKind::Document);
    mutations.create(id(10), second, NodeKind::Document);
    mutations.create(id(11), second, NodeKind::Element { tag: "div".into() });
    mutations.insert(id(10), id(11), None);
    mutations.set_style(
        id(11),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(100.0)),
                height: Some(LengthSpec::Px(100.0)),
                position: PositionSpec::Static,
                layout_isolation: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    world.commit(mutations).unwrap();
    let engine = RuntimeLayoutEngine;
    let mut cache = RetainedLayoutCache::default();
    engine
        .layout_document_scoped(
            &world,
            second,
            LayoutViewport::new(800.0, 600.0),
            &[],
            &mut cache,
            true,
        )
        .unwrap();
    let previous = cache.documents[&second].boxes[&id(11)];
    let emitted = engine
        .layout_document_scoped(
            &world,
            first,
            LayoutViewport::new(320.0, 240.0),
            &[id(11)],
            &mut cache,
            false,
        )
        .unwrap();
    assert!(
        emitted
            .iter()
            .all(|(node, _)| world.document_of(*node) == Some(first))
    );
    assert_eq!(cache.documents[&second].boxes[&id(11)], previous);
    engine
        .layout_document_scoped(
            &world,
            first,
            LayoutViewport::new(320.0, 240.0),
            &[],
            &mut cache,
            true,
        )
        .unwrap();
    assert_eq!(cache.documents[&second].boxes[&id(11)], previous);
    let isolated = engine
        .layout_document_scoped(
            &world,
            second,
            LayoutViewport::new(800.0, 600.0),
            &[id(11)],
            &mut cache,
            false,
        )
        .unwrap();
    assert_eq!(isolated, vec![(id(11), previous)]);
    cache.remove_document(first);
    assert!(!cache.documents.contains_key(&first));
    assert_eq!(cache.documents[&second].boxes[&id(11)], previous);
    let empty = DocumentId::new(3).unwrap();
    assert!(
        engine
            .layout_document_scoped(
                &world,
                empty,
                LayoutViewport::new(800.0, 600.0),
                &[],
                &mut cache,
                true,
            )
            .unwrap()
            .is_empty()
    );
    assert!(!cache.documents.contains_key(&empty));
}

/// Column of `rows` fixed-height rows, each with one fixed-height label,
/// under document(1) → column(2). Row `r` is id(3 + r*2), label id(4 + r*2).
fn column_tree(rows: u64) -> (UiWorld, DocumentId) {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(300.0)),
                height: Some(LengthSpec::Fill),
                direction: Some(FlexDirection::Column),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for row in 0..rows {
        let row_id = id(3 + row * 2);
        let label_id = id(4 + row * 2);
        queue.create(row_id, document, NodeKind::Element { tag: "div".into() });
        queue.create(label_id, document, NodeKind::Text);
        queue.insert(id(2), row_id, None);
        queue.insert(row_id, label_id, None);
        queue.set_text(
            label_id,
            TextContent {
                value: "行".into()
            },
        );
        queue.set_style(
            row_id,
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(300.0)),
                    height: Some(LengthSpec::Px(20.0)),
                    direction: Some(FlexDirection::Row),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        queue.set_style(
            label_id,
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(40.0)),
                    height: Some(LengthSpec::Px(20.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    world.commit(queue).unwrap();
    (world, document)
}

fn resize_row(world: &mut UiWorld, row: u64, height: f32) {
    let mut queue = MutationQueue::new();
    queue.set_style(
        id(3 + row * 2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(300.0)),
                height: Some(LengthSpec::Px(height)),
                direction: Some(FlexDirection::Row),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
}

#[test]
fn returning_to_old_viewport_does_not_restore_sizes_from_before_a_content_change() {
    let (mut world, document) = column_tree(4);
    let mut root_style = NodeStyle::default();
    Arc::make_mut(&mut root_style.layout).height = Some(LengthSpec::Fill);
    let mut mutations = MutationQueue::new();
    mutations.set_style(id(1), root_style);
    let mut row_style = world.node_style(id(3)).unwrap().clone();
    Arc::make_mut(&mut row_style.layout).height = None;
    mutations.set_style(id(3), row_style);
    world.commit(mutations).unwrap();
    let mut retained = RetainedLayoutCache::default();
    let original = LayoutViewport::new(300.0, 800.0);
    let smaller = LayoutViewport::new(300.0, 600.0);
    let engine = RuntimeLayoutEngine;
    let boxes = engine
        .layout_document_scoped(&world, document, original, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &boxes);
    let mut label_style = world.node_style(id(4)).unwrap().clone();
    Arc::make_mut(&mut label_style.layout).height = Some(LengthSpec::Px(35.0));
    let mut mutations = MutationQueue::new();
    mutations.set_style(id(4), label_style);
    world.commit(mutations).unwrap();
    let boxes = engine
        .layout_document_scoped(&world, document, smaller, &[id(4)], &mut retained, false)
        .unwrap();
    write_changed_boxes(&mut world, &boxes);
    engine
        .layout_document_scoped(&world, document, original, &[id(1)], &mut retained, false)
        .unwrap();
    assert_eq!(
        retained.documents[&document].boxes,
        full_boxes(&world, document, original)
    );
}

#[test]
fn repeated_viewport_resize_keeps_intrinsic_history_bounded() {
    let (world, document) = column_tree(4);
    let engine = RuntimeLayoutEngine;
    let mut retained = RetainedLayoutCache::default();
    for step in 0..256 {
        let viewport = LayoutViewport::new(300.0 + step as f32, 800.0 + step as f32);
        engine
            .layout_document_scoped(
                &world,
                document,
                viewport,
                &[id(1)],
                &mut retained,
                step == 0,
            )
            .unwrap();
        assert_eq!(
            retained.documents[&document].boxes,
            full_boxes(&world, document, viewport)
        );
    }
    let memo = &retained.documents[&document];
    assert!(memo.intrinsics.len() <= world.len());
    let variants = memo
        .intrinsics
        .values()
        .map(|node| node.measurements.iter().flatten().count())
        .sum::<usize>();
    assert!(variants <= world.len() * 2);
}

fn full_boxes(
    world: &UiWorld,
    document: DocumentId,
    viewport: LayoutViewport,
) -> HashMap<StableNodeId, LayoutBox> {
    RuntimeLayoutEngine
        .layout_document(world, document, viewport)
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>()
}

fn write_changed_boxes(
    world: &mut UiWorld,
    emitted: &[(StableNodeId, LayoutBox)],
) -> Vec<StableNodeId> {
    let mut queue = MutationQueue::new();
    let mut written = Vec::new();
    for (id, box_) in emitted {
        if world.layout_box(*id) != Some(*box_) {
            queue.write_layout(*id, *box_);
            written.push(*id);
        }
    }
    if !written.is_empty() {
        world.commit(queue).unwrap();
    }
    written
}

#[test]
fn fixed_layout_island_reuses_outer_layout_and_resizing_reaches_siblings() {
    let (mut world, document) = column_tree(400);
    let viewport = LayoutViewport::new(300.0, 800.0);
    let island = id(203);
    let label = id(204);
    let mut style = world.node_style(island).unwrap().clone();
    Arc::make_mut(&mut style.layout).layout_isolation = true;
    let mut queue = MutationQueue::new();
    queue.set_style(island, style.clone());
    world.commit(queue).unwrap();
    world.take_system_work();
    let mut retained = RetainedLayoutCache::default();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    world.take_system_work();
    let mut queue = MutationQueue::new();
    queue.set_text(
        label,
        TextContent {
            value: "changed text".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert!(!work.layout.contains(&id(2)));
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(
            &world,
            document,
            viewport,
            &work.layout,
            &mut retained,
            false,
        )
        .unwrap();
    assert!(emitted.len() <= 2);
    assert_eq!(
        retained.documents[&document].boxes,
        full_boxes(&world, document, viewport)
    );
    write_changed_boxes(&mut world, &emitted);
    world.take_system_work();
    Arc::make_mut(&mut style.layout).height = Some(LengthSpec::Px(35.0));
    let mut queue = MutationQueue::new();
    queue.set_style(island, style);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert!(work.layout.contains(&id(2)));
    RuntimeLayoutEngine
        .layout_document_scoped(
            &world,
            document,
            viewport,
            &work.layout,
            &mut retained,
            false,
        )
        .unwrap();
    assert_eq!(
        retained.documents[&document].boxes,
        full_boxes(&world, document, viewport)
    );
}

#[test]
fn auto_sized_isolation_request_preserves_parent_layout_dependency() {
    let (mut world, document) = column_tree(8);
    let viewport = LayoutViewport::new(300.0, 800.0);
    let mut style = world.node_style(id(3)).unwrap().clone();
    let layout = Arc::make_mut(&mut style.layout);
    layout.layout_isolation = true;
    layout.height = None;
    let mut queue = MutationQueue::new();
    queue.set_style(id(3), style);
    world.commit(queue).unwrap();
    world.take_system_work();
    let mut retained = RetainedLayoutCache::default();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    world.take_system_work();
    let mut style = world.node_style(id(4)).unwrap().clone();
    Arc::make_mut(&mut style.layout).height = Some(LengthSpec::Px(60.0));
    let mut queue = MutationQueue::new();
    queue.set_style(id(4), style);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert!(
        work.layout.contains(&id(2)),
        "auto size must reach its parent"
    );
    RuntimeLayoutEngine
        .layout_document_scoped(
            &world,
            document,
            viewport,
            &work.layout,
            &mut retained,
            false,
        )
        .unwrap();
    assert_eq!(
        retained.documents[&document].boxes,
        full_boxes(&world, document, viewport)
    );
}

#[test]
fn scoped_layout_touches_only_the_change_closure_and_matches_full_recompute() {
    let (mut world, document) = column_tree(400);
    let viewport = LayoutViewport::new(300.0, 800.0);
    let mut retained = RetainedLayoutCache::default();

    // Production drains dirty work before layout; the create-time marks
    // must not leak into the scoped measurement below.
    let _ = world.take_system_work();

    // Bootstrap: full pass populates the retained cache with every box.
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    assert_eq!(emitted.len(), 802, "full pass emits every node");
    write_changed_boxes(&mut world, &emitted);
    let _ = world.take_system_work();

    // Change the LAST row: nothing shifts above it, so the scoped pass
    // must recompute only that row's ancestor chain.
    resize_row(&mut world, 399, 26.0);
    let work = world.take_system_work();
    assert!(!work.layout.is_empty());
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(
            &world,
            document,
            viewport,
            &work.layout,
            &mut retained,
            false,
        )
        .unwrap();
    assert!(
        emitted.len() < 16,
        "tail row change must stay O(depth), not relayout {} nodes",
        emitted.len()
    );
    for (node, box_) in full_boxes(&world, document, viewport) {
        assert_eq!(
            retained.documents[&document].boxes.get(&node),
            Some(&box_),
            "scoped layout diverged from full recompute at {node:?}"
        );
    }
    write_changed_boxes(&mut world, &emitted);
    let _ = world.take_system_work();

    // Change a MIDDLE row: every row below shifts; the scoped pass must
    // emit exactly the shifted set (rows and their labels) and still
    // match a full recompute. Rows above stay pruned.
    resize_row(&mut world, 200, 32.0);
    let work = world.take_system_work();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(
            &world,
            document,
            viewport,
            &work.layout,
            &mut retained,
            false,
        )
        .unwrap();
    assert!(emitted.len() > 16, "shifted rows must be re-emitted");
    // 199 shifted rows + labels + the change closure; the 400 nodes above
    // the change must stay pruned (well under the 802-node document).
    assert!(
        emitted.len() < 420,
        "rows above the change stay pruned, got {} of 802",
        emitted.len()
    );
    for (node, box_) in full_boxes(&world, document, viewport) {
        assert_eq!(
            retained.documents[&document].boxes.get(&node),
            Some(&box_),
            "shifted scoped layout diverged from full recompute at {node:?}"
        );
    }

    let written = write_changed_boxes(&mut world, &emitted);
    let extract = world.take_system_work();
    let changed_row = id(3 + 200 * 2);
    let changed_label = id(4 + 200 * 2);
    let row_above = id(3 + 199 * 2);
    let label_above = id(4 + 199 * 2);
    let shifted_row = id(3 + 201 * 2);
    let shifted_label = id(4 + 201 * 2);
    assert!(written.contains(&changed_row));
    assert!(written.contains(&shifted_row));
    assert!(written.contains(&shifted_label));
    assert!(extract.render_extraction.contains(&changed_row));
    assert!(extract.render_extraction.contains(&shifted_row));
    assert!(extract.render_extraction.contains(&shifted_label));
    assert!(
        !written.contains(&changed_label),
        "bit-identical label of the changed row must not be written"
    );
    assert!(!extract.render_extraction.contains(&changed_label));
    assert!(!extract.render_extraction.contains(&row_above));
    assert!(!extract.render_extraction.contains(&label_above));
}

#[test]
fn scoped_layout_materializes_far_fewer_inputs_than_the_document_for_a_tail_row() {
    let (mut world, document) = column_tree(400);
    let viewport = LayoutViewport::new(300.0, 800.0);
    let mut retained = RetainedLayoutCache::default();
    let _ = world.take_system_work();

    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    assert_eq!(emitted.len(), 802);
    assert_eq!(retained.documents[&document].materialized_inputs, 802);
    write_changed_boxes(&mut world, &emitted);
    let _ = world.take_system_work();

    resize_row(&mut world, 399, 26.0);
    let work = world.take_system_work();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(
            &world,
            document,
            viewport,
            &work.layout,
            &mut retained,
            false,
        )
        .unwrap();
    assert!(
        emitted.len() < 16,
        "tail row change must stay O(depth), not relayout {} nodes",
        emitted.len()
    );
    // Document + column + dirty row (+ label / path ancestors). Unshifted
    // siblings are classified from layout style, not full LayoutInput.
    assert!(
        retained.documents[&document].materialized_inputs <= 16,
        "tail row must not assemble unshifted siblings, materialized {} of 802",
        retained.documents[&document].materialized_inputs
    );
    for (node, box_) in full_boxes(&world, document, viewport) {
        assert_eq!(
            retained.documents[&document].boxes.get(&node),
            Some(&box_),
            "on-demand scoped layout diverged from full recompute at {node:?}"
        );
    }
}

#[test]
fn lays_out_shaped_controls_without_application_geometry() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    queue.create(
        id(2),
        document,
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Fill),
                direction: Some(FlexDirection::Column),
                padding: Some(LengthSpec::Px(12.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                padding_left: Some(LengthSpec::Px(8.0)),
                padding_right: Some(LengthSpec::Px(8.0)),
                min_height: Some(LengthSpec::Px(32.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text(
        id(2),
        TextContent {
            value: "Build".into(),
        },
    );
    world.commit(queue).unwrap();
    struct FixedShaper;
    impl TextShaper for FixedShaper {
        fn shape(
            &mut self,
            _id: StableNodeId,
            _text: &TextContent,
            _style: &ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            TextMetrics {
                width: 40.0,
                height: 18.0,
                ascent: None,
            }
        }
    }
    world.shape_text(&[id(2)], &mut FixedShaper).unwrap();

    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(320.0, 180.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(1)].width, 320.0);
    assert_eq!(layouts[&id(2)].x, 12.0);
    assert_eq!(layouts[&id(2)].y, 12.0);
    assert_eq!(layouts[&id(2)].width, 56.0);
    assert_eq!(layouts[&id(2)].height, 32.0);
}

#[test]
fn display_none_child_does_not_take_a_gap_slot() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for value in 2..=4 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(1), id(value), None);
    }
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(40.0)),
                direction: Some(FlexDirection::Row),
                gap: Some(LengthSpec::Px(10.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for value in [2, 4] {
        queue.set_style(
            id(value),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(50.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                display: Some(nana_ui_core::DisplaySpec::None),
                width: Some(LengthSpec::Px(50.0)),
                height: Some(LengthSpec::Px(40.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(200.0, 40.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(3)].width, 0.0);
    assert_eq!(layouts[&id(3)].height, 0.0);
    assert_eq!(layouts[&id(2)].x, 0.0);
    assert_eq!(layouts[&id(4)].x, 60.0);
}

#[test]
fn row_fill_uses_remaining_content_width() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for value in 2..=3 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(1), id(value), None);
    }
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(300.0)),
                height: Some(LengthSpec::Px(40.0)),
                direction: Some(FlexDirection::Row),
                gap: Some(LengthSpec::Px(10.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(50.0)),
                height: Some(LengthSpec::Fill),
                flex_shrink: Some(0.0),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Fill),
                height: Some(LengthSpec::Fill),
                margin_left: Some(LengthSpec::Px(10.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(300.0, 40.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(2)].width, 50.0);
    assert_eq!(layouts[&id(3)].x, 70.0);
    assert_eq!(layouts[&id(3)].width, 230.0);
}

#[test]
fn column_fill_width_subtracts_negative_margins_symmetrically() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(100.0)),
                direction: Some(FlexDirection::Column),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Fill),
                height: Some(LengthSpec::Px(10.0)),
                margin_left: Some(LengthSpec::Px(-10.0)),
                margin_right: Some(LengthSpec::Px(-10.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(200.0, 100.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(2)].x, -10.0);
    assert_eq!(layouts[&id(2)].width, 220.0);
}

#[test]
fn unspecified_flex_shrink_keeps_overflowing_definite_row() {
    // Issue #22: omitted flex-shrink is 0, not CSS initial 1.
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for value in 2..=3 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(1), id(value), None);
    }
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(40.0)),
                direction: Some(FlexDirection::Row),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for value in [2, 3] {
        queue.set_style(
            id(value),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(150.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(200.0, 40.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(2)].width, 150.0);
    assert_eq!(layouts[&id(3)].width, 150.0);
    assert_eq!(layouts[&id(3)].x, 150.0);
}

#[test]
fn row_space_between_auto_children_keep_the_trailing_control_inside() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for value in 2..=5 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
    }
    queue.create(id(6), document, NodeKind::Text);
    queue.insert(id(1), id(2), None);
    queue.insert(id(2), id(3), None);
    queue.insert(id(2), id(5), None);
    queue.insert(id(3), id(4), None);
    queue.insert(id(4), id(6), None);
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Fill),
                direction: Some(FlexDirection::Column),
                padding: Some(LengthSpec::Px(20.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                direction: Some(FlexDirection::Row),
                justify_content: JustifySpec::SpaceBetween,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(4),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                height: Some(LengthSpec::Px(16.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(5),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                padding_left: Some(LengthSpec::Px(8.0)),
                padding_right: Some(LengthSpec::Px(8.0)),
                min_height: Some(LengthSpec::Px(32.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text(
        id(6),
        TextContent {
            value: "Title".into(),
        },
    );
    queue.set_text(
        id(5),
        TextContent {
            value: "Open".into(),
        },
    );
    world.commit(queue).unwrap();
    struct FixedShaper;
    impl TextShaper for FixedShaper {
        fn shape(
            &mut self,
            id: StableNodeId,
            _text: &TextContent,
            _style: &ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            if id.get() == 6 {
                TextMetrics {
                    width: 180.0,
                    height: 16.0,
                    ascent: None,
                }
            } else {
                TextMetrics {
                    width: 74.0,
                    height: 16.0,
                    ascent: None,
                }
            }
        }
    }
    world.shape_text(&[id(6), id(5)], &mut FixedShaper).unwrap();

    let viewport = LayoutViewport::new(400.0, 200.0);
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, viewport)
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    let trailing = layouts[&id(5)];
    assert!(
        trailing.width > 0.0 && trailing.height > 0.0,
        "trailing control must be hittable, got {trailing:?}"
    );
    assert!(
        trailing.x >= 0.0 && trailing.x + trailing.width <= viewport.width + 0.5,
        "space-between must not push the trailing control outside the viewport, got {trailing:?} viewport={}",
        viewport.width
    );
    assert!(
        layouts[&id(3)].width < layouts[&id(2)].width,
        "auto-width row cluster must shrink instead of eating the header"
    );
    assert!(
        layouts[&id(4)].width < layouts[&id(2)].width,
        "nested auto-width heading must not fill the header, got {:?}",
        layouts[&id(4)]
    );
}

#[test]
fn absolute_panel_children_resolve_fill_against_the_panel_content_box() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for value in 1..=3 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
    }
    queue.insert(id(1), id(2), None);
    queue.insert(id(2), id(3), None);
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Fill),
                height: Some(LengthSpec::Fill),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                position: PositionSpec::Absolute,
                offset_left: Some(LengthSpec::Px(8.0)),
                width: Some(LengthSpec::Px(280.0)),
                height: Some(LengthSpec::Px(200.0)),
                padding: Some(LengthSpec::Px(8.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Fill),
                height: Some(LengthSpec::Px(32.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();

    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(1280.0, 900.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();

    assert_eq!(layouts[&id(2)].width, 280.0);
    assert_eq!(layouts[&id(3)].x, 16.0);
    assert_eq!(layouts[&id(3)].width, 264.0);
}

#[test]
fn fixed_content_shrink_accounts_for_flow_chrome_nesting_and_constraints() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for value in 2..=15 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
    }

    for child in [id(2), id(6), id(9), id(13), id(15)] {
        queue.insert(id(1), child, None);
    }
    for child in [id(3), id(4), id(5)] {
        queue.insert(id(2), child, None);
    }
    for child in [id(7), id(8)] {
        queue.insert(id(6), child, None);
    }
    for child in [id(10), id(12)] {
        queue.insert(id(9), child, None);
    }
    queue.insert(id(10), id(11), None);
    queue.insert(id(13), id(14), None);

    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Fill),
                direction: Some(FlexDirection::Column),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Shrink),
                direction: Some(FlexDirection::Row),
                gap: Some(LengthSpec::Px(3.0)),
                padding: Some(LengthSpec::Px(2.0)),
                border_width: Some(1.0),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for (node, width) in [(id(3), 20.0), (id(4), 30.0)] {
        queue.set_style(
            node,
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(width)),
                    height: Some(LengthSpec::Px(8.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    queue.set_style(
        id(5),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                position: PositionSpec::Absolute,
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(8.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(6),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Shrink),
                direction: Some(FlexDirection::Column),
                padding: Some(LengthSpec::Px(1.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for (node, width) in [(id(7), 40.0), (id(8), 25.0)] {
        queue.set_style(
            node,
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(width)),
                    height: Some(LengthSpec::Px(8.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    queue.set_style(
        id(9),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Shrink),
                direction: Some(FlexDirection::Row),
                gap: Some(LengthSpec::Px(2.0)),
                padding: Some(LengthSpec::Px(1.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(10),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Shrink),
                direction: Some(FlexDirection::Column),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for (node, width) in [(id(11), 35.0), (id(12), 10.0)] {
        queue.set_style(
            node,
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(width)),
                    height: Some(LengthSpec::Px(8.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    queue.set_style(
        id(13),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Shrink),
                min_width: Some(LengthSpec::Px(50.0)),
                max_width: Some(LengthSpec::Px(55.0)),
                padding: Some(LengthSpec::Px(2.0)),
                border_width: Some(1.0),
                box_sizing: BoxSizing::ContentBox,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(14),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(20.0)),
                height: Some(LengthSpec::Px(8.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        id(15),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Shrink),
                max_width: Some(LengthSpec::Px(60.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text(
        id(15),
        TextContent {
            value: "wide".into(),
        },
    );
    world.commit(queue).unwrap();

    struct WideText;
    impl TextShaper for WideText {
        fn shape(
            &mut self,
            _id: StableNodeId,
            _text: &TextContent,
            _style: &ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            TextMetrics {
                width: 100.0,
                height: 8.0,
                ascent: None,
            }
        }
    }
    world.shape_text(&[id(15)], &mut WideText).unwrap();

    let layout_at = |width| {
        RuntimeLayoutEngine
            .layout_document(&world, document, LayoutViewport::new(width, 240.0))
            .unwrap()
            .into_iter()
            .collect::<HashMap<_, _>>()
    };
    let narrow = layout_at(320.0);
    let wide = layout_at(640.0);

    for layouts in [&narrow, &wide] {
        assert_eq!(layouts[&id(2)].width, 59.0);
        assert_eq!(layouts[&id(6)].width, 42.0);
        assert_eq!(layouts[&id(9)].width, 49.0);
        assert_eq!(layouts[&id(10)].width, 35.0);
        assert_eq!(layouts[&id(13)].width, 50.0);
        assert_eq!(layouts[&id(15)].width, 60.0);
    }
    for node in [id(2), id(6), id(9), id(10), id(13), id(15)] {
        assert_eq!(narrow[&node].width, wide[&node].width);
    }
}

#[test]
fn row_wrap_breaks_to_the_next_line() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for value in 2..=5 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(1), id(value), None);
        queue.set_style(
            id(value),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(80.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                direction: Some(FlexDirection::Row),
                flex_wrap: FlexWrap::Wrap,
                gap: Some(LengthSpec::Px(8.0)),
                align_items: nana_ui_core::AlignSpec::Start,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(200.0, 160.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(2)].x, 0.0);
    assert_eq!(layouts[&id(3)].x, 88.0);
    assert_eq!(layouts[&id(4)].x, 0.0);
    assert_eq!(layouts[&id(4)].y, 48.0);
    assert_eq!(layouts[&id(5)].x, 88.0);
    assert_eq!(layouts[&id(5)].y, 48.0);
    assert_eq!(layouts[&id(1)].height, 88.0);
}

/// Lay out a wrapping row `width` wide holding `children`, and answer each
/// child's box.
fn wrapping_row(width: f32, children: &[LayoutStyle]) -> Vec<LayoutBox> {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for (index, style) in children.iter().enumerate() {
        let child = id(index as u64 + 2);
        queue.create(child, document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(1), child, None);
        queue.set_style(
            child,
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    height: Some(LengthSpec::Px(40.0)),
                    ..style.clone()
                }),
                ..NodeStyle::default()
            },
        );
    }
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(width)),
                direction: Some(FlexDirection::Row),
                flex_wrap: FlexWrap::Wrap,
                gap: Some(LengthSpec::Px(8.0)),
                align_items: AlignSpec::Start,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(width, 400.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    (0..children.len())
        .map(|index| layouts[&id(index as u64 + 2)])
        .collect()
}

/// Items join a wrapping line by the size they would have before growing
/// (CSS Flexbox §9.3): a growing item shares its line with the siblings that
/// fit beside it, then takes what is left of that line.
#[test]
fn a_growing_item_in_a_wrapping_row_shares_its_line_with_siblings_that_fit() {
    let grow = |width: f32| LayoutStyle {
        width: Some(LengthSpec::Px(width)),
        flex_grow: Some(1.0),
        ..LayoutStyle::default()
    };
    let fixed = LayoutStyle {
        width: Some(LengthSpec::Px(80.0)),
        ..LayoutStyle::default()
    };
    let boxes = wrapping_row(300.0, &[grow(100.0), fixed.clone()]);
    assert_eq!((boxes[0].y, boxes[1].y), (0.0, 0.0));
    assert_eq!(boxes[0].width, 300.0 - 8.0 - 80.0);
    assert_eq!(boxes[1].x, 300.0 - 80.0);

    // Two columns side by side while each gets its basis, stacked below it.
    let column = LayoutStyle {
        flex_basis: Some(LengthSpec::Px(140.0)),
        flex_grow: Some(1.0),
        flex_shrink: Some(1.0),
        ..LayoutStyle::default()
    };
    let wide = wrapping_row(300.0, &[column.clone(), column.clone()]);
    assert_eq!((wide[0].y, wide[1].y), (0.0, 0.0));
    assert_eq!((wide[0].width, wide[1].width), (146.0, 146.0));
    let narrow = wrapping_row(250.0, &[column.clone(), column]);
    assert_eq!((narrow[0].y, narrow[1].y), (0.0, 48.0));
    assert_eq!((narrow[0].width, narrow[1].width), (250.0, 250.0));

    // `flex: 1` with a floor: the floor, not the zero basis, decides the line.
    let floored = LayoutStyle {
        flex_basis: Some(LengthSpec::Px(0.0)),
        flex_grow: Some(1.0),
        min_width: Some(LengthSpec::Px(180.0)),
        ..LayoutStyle::default()
    };
    let floors = wrapping_row(300.0, &[floored.clone(), floored.clone()]);
    assert_eq!((floors[0].y, floors[1].y), (0.0, 48.0));
    let floors = wrapping_row(400.0, &[floored.clone(), floored]);
    assert_eq!((floors[0].y, floors[1].y), (0.0, 0.0));

    // A growing item with nothing to size it by is its content's size.
    let auto = wrapping_row(
        300.0,
        &[
            LayoutStyle {
                flex_grow: Some(1.0),
                ..LayoutStyle::default()
            },
            fixed,
        ],
    );
    assert_eq!((auto[0].y, auto[1].y), (0.0, 0.0));
}

/// `min-width: max-content` keeps a growing item at least as wide as its
/// content: the line shrinks its other items first.
#[test]
fn a_max_content_minimum_keeps_a_growing_item_at_its_content() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    let style = |layout: LayoutStyle| NodeStyle {
        layout: Arc::new(layout),
        ..NodeStyle::default()
    };
    for (child, parent, layout) in [
        (
            2,
            1,
            LayoutStyle {
                direction: Some(FlexDirection::Row),
                flex_grow: Some(1.0),
                flex_shrink: Some(1.0),
                min_width: Some(LengthSpec::MaxContent),
                ..LayoutStyle::default()
            },
        ),
        (
            3,
            2,
            LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(20.0)),
                flex_shrink: Some(0.0),
                ..LayoutStyle::default()
            },
        ),
        (
            4,
            1,
            LayoutStyle {
                width: Some(LengthSpec::Px(150.0)),
                height: Some(LengthSpec::Px(20.0)),
                flex_shrink: Some(1.0),
                ..LayoutStyle::default()
            },
        ),
    ] {
        queue.create(id(child), document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(parent), id(child), None);
        queue.set_style(id(child), style(layout));
    }
    queue.set_style(
        id(1),
        style(LayoutStyle {
            width: Some(LengthSpec::Px(300.0)),
            direction: Some(FlexDirection::Row),
            ..LayoutStyle::default()
        }),
    );
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(300.0, 100.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(2)].width, 200.0);
    assert_eq!(layouts[&id(4)].width, 100.0);
}

#[test]
fn a_wrapping_row_is_measured_in_the_order_it_is_placed() {
    // B 按 `order` 排在最前、独占一行,A 与 C 并排在第二行:两行高。按文档顺序
    // 量会折成 A / B / C 三行,容器高出一整行。
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for (value, full) in [(2, false), (3, true), (4, false)] {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(1), id(value), None);
        queue.set_style(
            id(value),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(80.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    order: if full { -1 } else { 0 },
                    flex_basis: full.then_some(LengthSpec::Percent(100.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                direction: Some(FlexDirection::Row),
                flex_wrap: FlexWrap::Wrap,
                gap: Some(LengthSpec::Px(8.0)),
                align_items: nana_ui_core::AlignSpec::Start,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(200.0, 400.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(3)].y, 0.0);
    assert_eq!(layouts[&id(2)].y, 48.0);
    assert_eq!(layouts[&id(4)].y, 48.0);
    assert_eq!(layouts[&id(1)].height, 88.0);
}

#[test]
fn grid_template_columns_split_free_space() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Element { tag: "div".into() });
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.create(id(3), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.insert(id(1), id(3), None);
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                display: Some(DisplaySpec::Grid),
                direction: Some(FlexDirection::Row),
                width: Some(LengthSpec::Px(800.0)),
                height: Some(LengthSpec::Px(400.0)),
                grid_columns: Some(vec![GridTrack::Px(220.0), GridTrack::Fr(1.0)]),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for value in [2, 3] {
        queue.set_style(
            id(value),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    height: Some(LengthSpec::Px(400.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    world.commit(queue).unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(800.0, 400.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(layouts[&id(2)].width, 220.0);
    assert_eq!(layouts[&id(3)].x, 220.0);
    assert_eq!(layouts[&id(3)].width, 580.0);
}

#[test]
fn style_tree_matches_document_layout_for_row_gap() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(400.0)),
            height: Some(LengthSpec::Px(80.0)),
            gap: Some(LengthSpec::Px(12.0)),
            align_items: nana_ui_core::AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![
            StyleLayoutNode {
                id: "a".into(),
                style: LayoutStyle {
                    width: Some(LengthSpec::Px(50.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            },
            StyleLayoutNode {
                id: "b".into(),
                style: LayoutStyle {
                    width: Some(LengthSpec::Px(50.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            },
        ],
        text: None,
    };
    let boxes = RuntimeLayoutEngine
        .layout_style_tree(
            &tree,
            LayoutViewport::new(400.0, 80.0),
            &mut crate::MeasureTextShaper,
        )
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert!((boxes["b"].x - 62.0).abs() < 0.01);
}

#[test]
fn child_em_width_uses_parent_computed_font_size() {
    let tree = StyleLayoutNode {
        id: "parent".into(),
        style: LayoutStyle {
            font_size: Some(32.0),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            direction: Some(FlexDirection::Row),
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "child".into(),
            style: LayoutStyle {
                width: Some(LengthSpec::Em(2.0)),
                height: Some(LengthSpec::Px(40.0)),
                ..LayoutStyle::default()
            },
            children: Vec::new(),
            text: None,
        }],
        text: None,
    };
    let boxes = RuntimeLayoutEngine
        .layout_style_tree(
            &tree,
            LayoutViewport::new(200.0, 80.0),
            &mut crate::MeasureTextShaper,
        )
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(
        boxes["child"].width, 64.0,
        "2em against parent font-size 32px must be 64px, not 32px"
    );
}

#[test]
fn child_em_padding_uses_parent_computed_font_size() {
    let tree = StyleLayoutNode {
        id: "parent".into(),
        style: LayoutStyle {
            font_size: Some(32.0),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(200.0)),
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "child".into(),
            style: LayoutStyle {
                padding: Some(LengthSpec::Em(1.0)),
                ..LayoutStyle::default()
            },
            children: vec![StyleLayoutNode {
                id: "inner".into(),
                style: LayoutStyle {
                    width: Some(LengthSpec::Px(10.0)),
                    height: Some(LengthSpec::Px(10.0)),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            }],
            text: None,
        }],
        text: None,
    };
    let boxes = RuntimeLayoutEngine
        .layout_style_tree(
            &tree,
            LayoutViewport::new(200.0, 200.0),
            &mut crate::MeasureTextShaper,
        )
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(
        boxes["inner"].x, 32.0,
        "1em padding against inherited 32px font-size must inset content 32px, not 16px"
    );
    assert_eq!(boxes["inner"].y, 32.0);
    assert_eq!(
        boxes["child"].width, 74.0,
        "1em padding on both sides must add 64px to the 10px content box"
    );
    assert_eq!(boxes["child"].height, 74.0);
}

#[test]
fn child_em_absolute_inset_uses_parent_computed_font_size() {
    let tree = StyleLayoutNode {
        id: "parent".into(),
        style: LayoutStyle {
            font_size: Some(32.0),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(200.0)),
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "child".into(),
            style: LayoutStyle {
                position: PositionSpec::Absolute,
                offset_top: Some(LengthSpec::Em(1.0)),
                offset_left: Some(LengthSpec::Em(1.0)),
                width: Some(LengthSpec::Px(40.0)),
                height: Some(LengthSpec::Px(40.0)),
                ..LayoutStyle::default()
            },
            children: Vec::new(),
            text: None,
        }],
        text: None,
    };
    let boxes = RuntimeLayoutEngine
        .layout_style_tree(
            &tree,
            LayoutViewport::new(200.0, 200.0),
            &mut crate::MeasureTextShaper,
        )
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(
        boxes["child"].x, 32.0,
        "1em left against inherited 32px font-size must place at 32px, not 16px"
    );
    assert_eq!(
        boxes["child"].y, 32.0,
        "1em top against inherited 32px font-size must place at 32px, not 16px"
    );
}

#[test]
fn child_em_min_height_uses_parent_computed_font_size() {
    let tree = StyleLayoutNode {
        id: "parent".into(),
        style: LayoutStyle {
            font_size: Some(32.0),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(200.0)),
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "child".into(),
            style: LayoutStyle {
                min_height: Some(LengthSpec::Em(2.0)),
                ..LayoutStyle::default()
            },
            children: Vec::new(),
            text: None,
        }],
        text: None,
    };
    let boxes = RuntimeLayoutEngine
        .layout_style_tree(
            &tree,
            LayoutViewport::new(200.0, 200.0),
            &mut crate::MeasureTextShaper,
        )
        .into_iter()
        .collect::<HashMap<_, _>>();
    assert_eq!(
        boxes["child"].height, 64.0,
        "2em min-height against parent font-size 32px must be 64px, not 32px"
    );
}

fn box_map(root: &StyleLayoutNode, vw: f32, vh: f32) -> HashMap<String, LayoutBox> {
    RuntimeLayoutEngine
        .layout_style_tree(
            root,
            LayoutViewport::new(vw, vh),
            &mut crate::MeasureTextShaper,
        )
        .into_iter()
        .collect()
}

fn px_box(id: &str, width: f32, height: f32) -> StyleLayoutNode {
    StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(width)),
            height: Some(LengthSpec::Px(height)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    }
}

#[test]
fn align_content_center_and_space_between_on_wrapped_row() {
    let children = (0..4)
        .map(|i| px_box(&format!("i{i}"), 80.0, 40.0))
        .collect::<Vec<_>>();
    let make = |align_content| StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            direction: Some(FlexDirection::Row),
            flex_wrap: FlexWrap::Wrap,
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(160.0)),
            gap: Some(LengthSpec::Px(8.0)),
            align_items: AlignSpec::Start,
            align_content,
            ..LayoutStyle::default()
        },
        children: children.clone(),
        text: None,
    };
    let center = box_map(&make(JustifySpec::Center), 200.0, 160.0);
    assert!((center["i0"].y - 36.0).abs() < 0.01);
    assert!((center["i1"].y - 36.0).abs() < 0.01);
    assert!((center["i2"].y - 84.0).abs() < 0.01);
    assert!((center["i3"].y - 84.0).abs() < 0.01);
    let between = box_map(&make(JustifySpec::SpaceBetween), 200.0, 160.0);
    assert!((between["i0"].y - 0.0).abs() < 0.01);
    assert!((between["i2"].y - 120.0).abs() < 0.01);
}

/// A row item is as tall as its content at the width the row gives it, not
/// at the width it was first measured with: a `Fill` column beside a fixed
/// side column holds a 16:9 box, whose height follows the column's used
/// width (CSS flexbox: the hypothetical cross size uses the used main size).
#[test]
fn a_row_item_is_as_tall_as_its_content_at_its_used_width() {
    let column = |grow: f32| StyleLayoutNode {
        id: "primary".into(),
        style: LayoutStyle {
            direction: Some(FlexDirection::Column),
            width: Some(LengthSpec::Fill),
            height: Some(LengthSpec::Shrink),
            min_width: Some(LengthSpec::Px(0.0)),
            flex_grow: Some(grow),
            flex_shrink: Some(1.0),
            align_items: AlignSpec::Stretch,
            ..LayoutStyle::default()
        },
        children: vec![
            StyleLayoutNode {
                id: "video".into(),
                style: LayoutStyle {
                    width: Some(LengthSpec::Fill),
                    aspect_ratio: Some(16.0 / 9.0),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            },
            px_box("title", 100.0, 20.0),
        ],
        text: None,
    };
    for grow in [1.0, 0.0] {
        let tree = StyleLayoutNode {
            id: "row".into(),
            style: LayoutStyle {
                direction: Some(FlexDirection::Row),
                width: Some(LengthSpec::Px(1152.0)),
                gap: Some(LengthSpec::Px(16.0)),
                align_items: AlignSpec::Start,
                ..LayoutStyle::default()
            },
            children: vec![column(grow), {
                let mut side = px_box("side", 402.0, 260.0);
                side.style.flex_shrink = Some(0.0);
                side
            }],
            text: None,
        };
        let boxes = box_map(&tree, 1280.0, 800.0);
        let used = 1152.0 - 16.0 - 402.0;
        assert!(
            (boxes["primary"].width - used).abs() < 0.01,
            "{:?}",
            boxes["primary"]
        );
        let video = used * 9.0 / 16.0;
        assert!(
            (boxes["video"].height - video).abs() < 0.01,
            "{:?}",
            boxes["video"]
        );
        assert!(
            (boxes["primary"].height - (video + 20.0)).abs() < 0.01,
            "grow {grow}: the column hugs its content at its used width, got {:?}",
            boxes["primary"]
        );
        assert!(
            (boxes["row"].height - (video + 20.0)).abs() < 0.01,
            "grow {grow}: and so does the row, got {:?}",
            boxes["row"]
        );
    }
}

/// `height: auto` stretches in a row that stretches its items, as an
/// unset height does; a content-sized keyword keeps the content's height.
#[test]
fn an_auto_cross_size_stretches_like_an_unset_one() {
    let item = |id: &str, height| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(40.0)),
            height,
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "row".into(),
        style: LayoutStyle {
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            align_items: AlignSpec::Stretch,
            ..LayoutStyle::default()
        },
        children: vec![
            item("unset", None),
            item("auto", Some(LengthSpec::Auto)),
            item("shrink", Some(LengthSpec::Shrink)),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert_eq!(boxes["unset"].height, 80.0);
    assert_eq!(boxes["auto"].height, 80.0);
    assert_eq!(boxes["shrink"].height, 0.0);
}

#[test]
fn display_contents_hoists_children_into_flex_row_gap() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(40.0)),
            gap: Some(LengthSpec::Px(10.0)),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "contents".into(),
            style: LayoutStyle {
                display: Some(DisplaySpec::Contents),
                ..LayoutStyle::default()
            },
            children: vec![px_box("a", 50.0, 40.0), px_box("b", 50.0, 40.0)],
            text: None,
        }],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 40.0);
    assert!(
        !boxes.contains_key("contents"),
        "display:contents must be absent from the box map"
    );
    assert!((boxes["a"].x - 0.0).abs() < 0.01);
    assert!((boxes["b"].x - 60.0).abs() < 0.01);
    assert_eq!(boxes["a"].width, 50.0);
    assert_eq!(boxes["b"].width, 50.0);
}

#[test]
fn grid_2d_auto_flow_wraps_fourth_item_to_second_row() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(100.0)),
            height: Some(LengthSpec::Px(100.0)),
            grid_columns: Some(vec![GridTrack::Px(50.0), GridTrack::Px(50.0)]),
            ..LayoutStyle::default()
        },
        children: (0..4)
            .map(|i| px_box(&format!("i{i}"), 50.0, 50.0))
            .collect(),
        text: None,
    };
    let boxes = box_map(&tree, 100.0, 100.0);
    assert_eq!(boxes["i0"].x, 0.0);
    assert_eq!(boxes["i0"].y, 0.0);
    assert_eq!(boxes["i1"].x, 50.0);
    assert_eq!(boxes["i1"].y, 0.0);
    assert_eq!(boxes["i2"].x, 0.0);
    assert_eq!(boxes["i2"].y, 50.0);
    assert_eq!(boxes["i3"].x, 50.0);
    assert_eq!(boxes["i3"].y, 50.0);
}

#[test]
fn grid_column_span_two_on_three_columns() {
    let first = StyleLayoutNode {
        id: "a".into(),
        style: LayoutStyle {
            height: Some(LengthSpec::Px(50.0)),
            grid_placement: GridPlacement {
                column_start: GridLine::Span(2),
                ..GridPlacement::default()
            }
            .into(),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(150.0)),
            height: Some(LengthSpec::Px(100.0)),
            grid_columns: Some(vec![
                GridTrack::Px(50.0),
                GridTrack::Px(50.0),
                GridTrack::Px(50.0),
            ]),
            ..LayoutStyle::default()
        },
        children: vec![first, px_box("b", 50.0, 50.0), px_box("c", 50.0, 50.0)],
        text: None,
    };
    let boxes = box_map(&tree, 150.0, 100.0);
    assert!((boxes["a"].x - 0.0).abs() < 0.01);
    assert!((boxes["a"].width - 100.0).abs() < 0.01);
    assert!((boxes["b"].x - 100.0).abs() < 0.01);
    assert!((boxes["b"].y - 0.0).abs() < 0.01);
    assert!((boxes["c"].x - 0.0).abs() < 0.01);
    assert!((boxes["c"].y - 50.0).abs() < 0.01);
}

#[test]
fn grid_justify_self_end_in_definite_column() {
    let mut item = px_box("item", 50.0, 50.0);
    item.style.justify_self = Some(AlignSpec::End);
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(50.0)),
            grid_columns: Some(vec![GridTrack::Px(200.0)]),
            ..LayoutStyle::default()
        },
        children: vec![item],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 50.0);
    assert!((boxes["item"].x - 150.0).abs() < 0.01);
    assert_eq!(boxes["item"].width, 50.0);
}

#[test]
fn grid_auto_fit_fills_two_minmax_tracks_in_500px() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(500.0)),
            height: Some(LengthSpec::Px(50.0)),
            grid_columns_repeat: Some(GridRepeatAuto {
                kind: GridTrackListUnsupported::RepeatAutoFit,
                tracks: vec![GridTrack::MinMax {
                    min_px: 200.0,
                    fr: 1.0,
                    max_px: None,
                }],
                ..Default::default()
            }),
            ..LayoutStyle::default()
        },
        children: vec![px_box("a", 50.0, 50.0), px_box("b", 50.0, 50.0)],
        text: None,
    };
    let boxes = box_map(&tree, 500.0, 50.0);
    assert!(
        (boxes["b"].x - 250.0).abs() < 0.5,
        "auto-fit minmax(200px,1fr) in 500px must keep 2 tracks, got b.x={}",
        boxes["b"].x
    );
    assert!((boxes["a"].x - 0.0).abs() < 0.5);
}

#[test]
fn white_space_pre_measures_explicit_newlines() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            font_size: Some(16.0),
            line_height: Some(LineHeightSpec::Absolute(20.0)),
            white_space: WhiteSpaceSpec::Pre,
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: Some("ab\ncd".into()),
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!(
        (boxes["root"].height - 40.0).abs() < 0.01,
        "pre + 2 lines × 20px line-height must be 40, got {}",
        boxes["root"].height
    );
}

#[test]
fn white_space_pre_wrap_keeps_newlines_and_wraps_long_lines() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            font_size: Some(16.0),
            line_height: Some(LineHeightSpec::Absolute(20.0)),
            white_space: WhiteSpaceSpec::PreWrap,
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: Some("ab\ncd".into()),
    };
    let boxes = box_map(&tree, 200.0, 120.0);
    assert!(
        (boxes["root"].height - 40.0).abs() < 0.01,
        "pre-wrap must keep explicit newlines (not Normal), got {}",
        boxes["root"].height
    );
}

#[test]
fn measure_text_pre_wrap_wraps_long_line_against_max_width() {
    let mut shaper = crate::MeasureTextShaper;
    let style = ComputedStyle {
        font_size: 16.0,
        line_height: Some(LineHeightSpec::Absolute(20.0)),
        ..ComputedStyle::default()
    };
    let metrics = shaper.shape(
        StableNodeId::new(1).unwrap(),
        &crate::TextContent {
            value: "abcdefghijklmnop\nq".into(),
        },
        &style,
        crate::TextShapeConstraints {
            max_width: Some(200.0),
            wrap: true,
            preserve_lines: true,
            ..crate::TextShapeConstraints::default()
        },
    );
    assert!(
        (metrics.height - 60.0).abs() < 0.01,
        "16em line in 200px + explicit second line → 60, got {}",
        metrics.height
    );
}

#[test]
fn aspect_ratio_square_from_definite_width() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(80.0)),
            aspect_ratio: Some(1.0),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let boxes = box_map(&tree, 400.0, 200.0);
    assert!(
        (boxes["root"].width - 80.0).abs() < 0.01 && (boxes["root"].height - 80.0).abs() < 0.01,
        "80px width + aspect-ratio 1 must be square, got {:?}",
        boxes["root"]
    );
}

#[test]
fn aspect_ratio_auto_width_uses_containing_block() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(400.0)),
            height: Some(LengthSpec::Px(200.0)),
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "block".into(),
            style: LayoutStyle {
                height: Some(LengthSpec::Px(80.0)),
                aspect_ratio: Some(1.0),
                ..LayoutStyle::default()
            },
            children: vec![StyleLayoutNode {
                id: "pct".into(),
                style: LayoutStyle {
                    width: Some(LengthSpec::Percent(50.0)),
                    height: Some(LengthSpec::Px(10.0)),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            }],
            text: None,
        }],
        text: None,
    };
    let boxes = box_map(&tree, 400.0, 200.0);
    assert!(
        (boxes["block"].width - 400.0).abs() < 0.01 && (boxes["block"].height - 80.0).abs() < 0.01,
        "block width:auto + height 80 + aspect-ratio 1 uses CB, not 80×80, got {:?}",
        boxes["block"]
    );
    assert!(
        (boxes["pct"].width - 200.0).abs() < 0.01,
        "% children resolve against the CB, not a shrink-wrapped 80, got {:?}",
        boxes["pct"]
    );
}

#[test]
fn aspect_ratio_row_stretch_does_not_overwrite_transferred_height() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(200.0)),
            align_items: AlignSpec::Stretch,
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "item".into(),
            style: LayoutStyle {
                width: Some(LengthSpec::Px(80.0)),
                aspect_ratio: Some(1.0),
                ..LayoutStyle::default()
            },
            children: Vec::new(),
            text: None,
        }],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 200.0);
    assert!(
        (boxes["item"].width - 80.0).abs() < 0.01 && (boxes["item"].height - 80.0).abs() < 0.01,
        "row stretch must not overwrite height transferred from width + ratio, got {:?}",
        boxes["item"]
    );
}

#[test]
fn aspect_ratio_column_stretch_fills_auto_height() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Column),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(200.0)),
            align_items: AlignSpec::Stretch,
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "item".into(),
            style: LayoutStyle {
                aspect_ratio: Some(1.0),
                ..LayoutStyle::default()
            },
            children: Vec::new(),
            text: None,
        }],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 200.0);
    assert!(
        (boxes["item"].width - 200.0).abs() < 0.01 && (boxes["item"].height - 200.0).abs() < 0.01,
        "column stretch width then ratio must fill auto height, got {:?}",
        boxes["item"]
    );
}

#[test]
fn grid_percent_and_fill_resolve_against_final_cell() {
    let fill = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Percent(100.0)),
            height: Some(LengthSpec::Fill),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(40.0)),
            grid_columns: Some(vec![GridTrack::Px(100.0), GridTrack::Fr(1.0)]),
            ..LayoutStyle::default()
        },
        children: vec![fill("a"), fill("b")],
        text: None,
    };
    let boxes = box_map(&tree, 300.0, 40.0);
    assert!(
        (boxes["a"].width - 100.0).abs() < 0.5 && (boxes["a"].height - 40.0).abs() < 0.5,
        "100%/Fill must fill the 100px track, not stay 0, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].width - 200.0).abs() < 0.5 && (boxes["b"].height - 40.0).abs() < 0.5,
        "100%/Fill must fill the 1fr cell, got {:?}",
        boxes["b"]
    );
}

#[test]
fn demote_fill_spec_treats_full_percent_calc_as_indefinite() {
    let calc_100 = LengthSpec::from_calc(CalcExpr::Min(
        Box::new(CalcExpr::Percent(100.0)),
        Box::new(CalcExpr::Percent(100.0)),
    ));
    assert!(calc_100.is_full_percent_fill());
    assert_eq!(demote_fill_spec(Some(calc_100)), None);
    assert_eq!(demote_fill_spec(Some(LengthSpec::Fill)), None);
    assert_eq!(
        demote_fill_spec(Some(LengthSpec::Px(40.0))),
        Some(LengthSpec::Px(40.0))
    );
}

#[test]
fn grid_cell_resolves_unsimplified_calc_against_cell() {
    let spec = LengthSpec::from_calc(CalcExpr::Binary {
        op: CalcBinOp::Add,
        left: Box::new(CalcExpr::Min(
            Box::new(CalcExpr::Px(100.0)),
            Box::new(CalcExpr::Percent(80.0)),
        )),
        right: Box::new(CalcExpr::Px(10.0)),
    });
    assert!(
        matches!(spec, LengthSpec::Calc(_)),
        "min() + px must stay as Calc AST"
    );

    let child = StyleLayoutNode {
        id: "child".into(),
        style: LayoutStyle {
            width: Some(spec),
            height: Some(LengthSpec::Px(30.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(400.0)),
            height: Some(LengthSpec::Px(40.0)),
            grid_columns: Some(vec![GridTrack::Px(400.0)]),
            align_items: AlignSpec::Start,
            justify_items: Some(AlignSpec::Start),
            ..LayoutStyle::default()
        },
        children: vec![child],
        text: None,
    };
    let boxes = box_map(&tree, 400.0, 40.0);
    assert!(
        (boxes["child"].width - 110.0).abs() < 0.5,
        "placed grid item must be 110px, got {:?}",
        boxes["child"]
    );
}

#[test]
fn empty_grid_item_stretches_into_track() {
    let empty = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle::default(),
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(40.0)),
            grid_columns: Some(vec![GridTrack::Px(100.0), GridTrack::Fr(1.0)]),
            // CSS `display:grid` initial align-items is stretch (css_map sets this).
            align_items: AlignSpec::Stretch,
            ..LayoutStyle::default()
        },
        children: vec![empty("a"), empty("b")],
        text: None,
    };
    let boxes = box_map(&tree, 300.0, 40.0);
    assert!(
        (boxes["a"].width - 100.0).abs() < 0.5 && (boxes["a"].height - 40.0).abs() < 0.5,
        "empty + stretch must fill the track, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].width - 200.0).abs() < 0.5 && (boxes["b"].height - 40.0).abs() < 0.5,
        "empty + stretch 1fr, got {:?}",
        boxes["b"]
    );
}

#[test]
fn same_side_floats_do_not_overlap() {
    let floated = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(60.0)),
            height: Some(LengthSpec::Px(40.0)),
            float: FloatSpec::Left,
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(80.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![floated("a"), floated("b")],
        text: None,
    };
    let boxes = box_map(&tree, 80.0, 80.0);
    assert!((boxes["a"].x - 0.0).abs() < 0.5);
    assert!((boxes["a"].y - 0.0).abs() < 0.5);
    assert!(
        (boxes["b"].y - 40.0).abs() < 0.5,
        "second left float must wrap below, got {:?}",
        boxes["b"]
    );
    assert!((boxes["b"].x - 0.0).abs() < 0.5);
}

#[test]
fn float_own_clear_starts_below_packed_same_side() {
    let left = |id: &str, clear: ClearSpec| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(60.0)),
            height: Some(LengthSpec::Px(40.0)),
            float: FloatSpec::Left,
            clear,
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![left("a", ClearSpec::None), left("b", ClearSpec::Left)],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!((boxes["a"].x - 0.0).abs() < 0.5);
    assert!((boxes["a"].y - 0.0).abs() < 0.5);
    assert!(
        (boxes["b"].y - 40.0).abs() < 0.5 && (boxes["b"].x - 0.0).abs() < 0.5,
        "float with clear:left must start below packed left, not beside it, got {:?}",
        boxes["b"]
    );
}

#[test]
fn subgrid_inherits_parent_column_track_sizes() {
    let cell = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Fill),
            height: Some(LengthSpec::Fill),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let sub = StyleLayoutNode {
        id: "sub".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            grid_columns_subgrid: true,
            grid_placement: GridPlacement {
                column_start: GridLine::Index(1),
                column_end: GridLine::Index(-1),
                ..GridPlacement::default()
            }
            .into(),
            align_items: AlignSpec::Stretch,
            ..LayoutStyle::default()
        },
        children: vec![cell("a"), cell("b")],
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(40.0)),
            grid_columns: Some(vec![GridTrack::Px(80.0), GridTrack::Px(120.0)]),
            align_items: AlignSpec::Stretch,
            ..LayoutStyle::default()
        },
        children: vec![sub],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 40.0);
    assert!(
        (boxes["a"].width - 80.0).abs() < 0.5 && (boxes["a"].x - 0.0).abs() < 0.5,
        "subgrid col 1 must inherit 80px, not split the 200px cell, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].width - 120.0).abs() < 0.5 && (boxes["b"].x - 80.0).abs() < 0.5,
        "subgrid col 2 must inherit 120px, got {:?}",
        boxes["b"]
    );
}

#[test]
fn ifc_wraps_inline_around_left_float() {
    let inline = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(70.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![
            StyleLayoutNode {
                id: "float".into(),
                style: LayoutStyle {
                    float: FloatSpec::Left,
                    width: Some(LengthSpec::Px(80.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            },
            inline("a"),
            inline("b"),
            inline("c"),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!(
        (boxes["float"].x - 0.0).abs() < 0.5 && (boxes["float"].y - 0.0).abs() < 0.5,
        "float stays packed at origin, got {:?}",
        boxes["float"]
    );
    assert!(
        (boxes["a"].x - 80.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "first inline must start after the left float band, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].x - 80.0).abs() < 0.5 && (boxes["b"].y - 20.0).abs() < 0.5,
        "second inline wraps in the shortened band, got {:?}",
        boxes["b"]
    );
    assert!(
        (boxes["c"].y - 40.0).abs() < 0.5 && (boxes["c"].x - 0.0).abs() < 0.5,
        "third inline drops below the float to full width, got {:?}",
        boxes["c"]
    );
}

#[test]
fn flex_item_float_is_blockified_not_floated() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(40.0)),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![
            StyleLayoutNode {
                id: "a".into(),
                style: LayoutStyle {
                    float: FloatSpec::Left,
                    width: Some(LengthSpec::Px(50.0)),
                    height: Some(LengthSpec::Px(40.0)),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            },
            px_box("b", 50.0, 40.0),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 40.0);
    assert!(
        (boxes["a"].x - 0.0).abs() < 0.5 && (boxes["b"].x - 50.0).abs() < 0.5,
        "flex item float must stay a flex item, not pack as a float, got a={:?} b={:?}",
        boxes["a"],
        boxes["b"]
    );
}

#[test]
fn ifc_block_sibling_starts_new_line() {
    let inline = |id: &str, x: f32| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(x)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let block = StyleLayoutNode {
        id: "mid".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(40.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![inline("a", 40.0), block, inline("c", 40.0)],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!((boxes["a"].y - 0.0).abs() < 0.5);
    assert!(
        (boxes["mid"].y - 20.0).abs() < 0.5,
        "block sibling must break the IFC line, got {:?}",
        boxes["mid"]
    );
    assert!(
        (boxes["c"].y - 40.0).abs() < 0.5,
        "inline after block starts a new line, got {:?}",
        boxes["c"]
    );
}

#[test]
fn ifc_block_in_inline_unboxes_like_block_siblings() {
    let inline_block = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(40.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let mid = StyleLayoutNode {
        id: "mid".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(40.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "span".into(),
            style: LayoutStyle {
                display: Some(DisplaySpec::Inline),
                ..LayoutStyle::default()
            },
            children: vec![inline_block("a"), mid, inline_block("c")],
            text: None,
        }],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!(
        (boxes["a"].y - 0.0).abs() < 0.5,
        "first inline-block stays on the first line, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["mid"].y - 20.0).abs() < 0.5,
        "block inside inline must hoist onto its own line, got {:?}",
        boxes["mid"]
    );
    assert!(
        (boxes["c"].y - 40.0).abs() < 0.5,
        "trailing inline-block starts after the hoisted block, got {:?}",
        boxes["c"]
    );
}

#[test]
fn flex_item_inline_with_block_is_not_unboxed() {
    let mid = StyleLayoutNode {
        id: "mid".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(50.0)),
            height: Some(LengthSpec::Px(40.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(40.0)),
            gap: Some(LengthSpec::Px(10.0)),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![
            StyleLayoutNode {
                id: "span".into(),
                style: LayoutStyle {
                    display: Some(DisplaySpec::Inline),
                    ..LayoutStyle::default()
                },
                children: vec![mid],
                text: None,
            },
            px_box("b", 50.0, 40.0),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 40.0);
    assert!(
        boxes.contains_key("span"),
        "inline flex item must stay one flex item, got {:?}",
        boxes.keys().collect::<Vec<_>>()
    );
    assert!(
        (boxes["span"].x - 0.0).abs() < 0.5 && (boxes["span"].width - 50.0).abs() < 0.5,
        "blockified inline item keeps its block child, got {:?}",
        boxes["span"]
    );
    assert!(
        (boxes["mid"].x - boxes["span"].x).abs() < 0.5,
        "block descendant stays inside the flex item, got mid={:?} span={:?}",
        boxes["mid"],
        boxes["span"]
    );
    assert!(
        (boxes["b"].x - 60.0).abs() < 0.5,
        "second flex item follows the inline item, not a hoisted block, got {:?}",
        boxes["b"]
    );
}

#[test]
fn ifc_text_align_start_packs_to_right_in_rtl() {
    let inline = StyleLayoutNode {
        id: "a".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(40.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(40.0)),
            dir: Some(DirSpec::Rtl),
            text_align: nana_ui_core::TextAlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![inline],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 40.0);
    assert!(
        (boxes["a"].x - 160.0).abs() < 0.5,
        "text-align:start in rtl must pack to inline-start (right), got {:?}",
        boxes["a"]
    );
}

#[test]
fn ifc_rtl_places_first_tree_item_at_inline_start() {
    let inline = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(40.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(40.0)),
            dir: Some(DirSpec::Rtl),
            ..LayoutStyle::default()
        },
        children: vec![inline("a"), inline("c")],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 40.0);
    assert!(
        (boxes["a"].x - 160.0).abs() < 0.5,
        "first tree-order item sits at RTL inline-start (right), got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["c"].x - 120.0).abs() < 0.5,
        "second tree-order item sits to the left of the first, got {:?}",
        boxes["c"]
    );
}

fn floated_box(id: &str, side: FloatSpec, width: f32, height: f32) -> StyleLayoutNode {
    StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(width)),
            height: Some(LengthSpec::Px(height)),
            float: side,
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    }
}

fn inline_box(id: &str, width: f32, height: f32) -> StyleLayoutNode {
    StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(width)),
            height: Some(LengthSpec::Px(height)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    }
}

#[test]
fn ifc_line_box_shrinks_around_float_left() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![
            floated_box("fl", FloatSpec::Left, 80.0, 40.0),
            inline_box("a", 50.0, 20.0),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!((boxes["fl"].x - 0.0).abs() < 0.5);
    assert!(
        (boxes["a"].x - 80.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "IFC line box must start after the left float, not overlap, got {:?}",
        boxes["a"]
    );
    assert!(
        boxes["a"].x + 0.5 >= boxes["fl"].x + boxes["fl"].width
            || boxes["a"].y + 0.5 >= boxes["fl"].y + boxes["fl"].height,
        "inline vs float must not overlap, a={:?} fl={:?}",
        boxes["a"],
        boxes["fl"]
    );
}

#[test]
fn ifc_inlines_wrap_in_width_beside_float() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![
            floated_box("fl", FloatSpec::Left, 80.0, 40.0),
            inline_box("a", 70.0, 20.0),
            inline_box("b", 70.0, 20.0),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!(
        (boxes["a"].x - 80.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "first inline sits in the shortened line, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].x - 80.0).abs() < 0.5 && (boxes["b"].y - 20.0).abs() < 0.5,
        "70+70 exceeds remaining 120 so b wraps beside the float, got {:?}",
        boxes["b"]
    );
}

#[test]
fn ifc_uses_full_width_below_float() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![
            floated_box("fl", FloatSpec::Left, 80.0, 40.0),
            inline_box("a", 70.0, 20.0),
            inline_box("b", 70.0, 20.0),
            inline_box("c", 70.0, 20.0),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!(
        (boxes["c"].x - 0.0).abs() < 0.5 && (boxes["c"].y - 40.0).abs() < 0.5,
        "below the float the line box is full width, got {:?}",
        boxes["c"]
    );
}

#[test]
fn ifc_oversized_inline_drops_below_float() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![
            floated_box("fl", FloatSpec::Left, 80.0, 40.0),
            inline_box("a", 150.0, 20.0),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!(
        (boxes["a"].x - 0.0).abs() < 0.5 && (boxes["a"].y - 40.0).abs() < 0.5,
        "item wider than remaining width must drop below the float, got {:?}",
        boxes["a"]
    );
}

#[test]
fn ifc_line_box_shrinks_between_left_and_right_floats() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![
            floated_box("left", FloatSpec::Left, 80.0, 40.0),
            floated_box("right", FloatSpec::Right, 80.0, 40.0),
            inline_box("a", 40.0, 20.0),
        ],
        text: None,
    };
    let boxes = box_map(&tree, 300.0, 80.0);
    assert!((boxes["left"].x - 0.0).abs() < 0.5);
    assert!((boxes["right"].x - 220.0).abs() < 0.5);
    assert!(
        (boxes["a"].x - 80.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "line box sits between left and right floats, got {:?}",
        boxes["a"]
    );
    assert!(
        boxes["a"].x + boxes["a"].width <= boxes["right"].x + 0.5,
        "inline must not overlap the right float, a={:?} right={:?}",
        boxes["a"],
        boxes["right"]
    );
}

#[test]
fn in_flow_block_does_not_shrink_beside_float() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(80.0)),
            ..LayoutStyle::default()
        },
        children: vec![
            floated_box("fl", FloatSpec::Left, 80.0, 40.0),
            StyleLayoutNode {
                id: "block".into(),
                style: LayoutStyle {
                    display: Some(DisplaySpec::Block),
                    width: Some(LengthSpec::Px(100.0)),
                    height: Some(LengthSpec::Px(20.0)),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            },
        ],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 80.0);
    assert!(
        (boxes["block"].x - 0.0).abs() < 0.5 && (boxes["block"].y - 0.0).abs() < 0.5,
        "block formatting does not shrink beside floats, got {:?}",
        boxes["block"]
    );
}

#[test]
fn writing_mode_vertical_rl_ifc_advances_inline_down_block_from_right() {
    let inline = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(20.0)),
            height: Some(LengthSpec::Px(40.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(80.0)),
            height: Some(LengthSpec::Px(80.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            ..LayoutStyle::default()
        },
        children: vec![inline("a"), inline("b")],
        text: None,
    };
    let boxes = box_map(&tree, 80.0, 80.0);
    assert!(
        (boxes["a"].x - 60.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "vertical-rl first inline sits at block-start (right) and inline-start (top), got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].x - 60.0).abs() < 0.5 && (boxes["b"].y - 40.0).abs() < 0.5,
        "second inline advances down the inline axis, got {:?}",
        boxes["b"]
    );
}

#[test]
fn writing_mode_vertical_lr_ifc_places_first_line_on_the_left() {
    let inline = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(20.0)),
            height: Some(LengthSpec::Px(40.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Block),
            width: Some(LengthSpec::Px(80.0)),
            height: Some(LengthSpec::Px(80.0)),
            writing_mode: Some(WritingModeSpec::VerticalLr),
            ..LayoutStyle::default()
        },
        children: vec![inline("a"), inline("b")],
        text: None,
    };
    let boxes = box_map(&tree, 80.0, 80.0);
    assert!(
        (boxes["a"].x - 0.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "vertical-lr first inline sits at block-start (left), got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].x - 0.0).abs() < 0.5 && (boxes["b"].y - 40.0).abs() < 0.5,
        "second inline advances down the inline axis, got {:?}",
        boxes["b"]
    );
}

#[test]
fn writing_mode_vertical_flex_row_uses_inline_axis() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(80.0)),
            height: Some(LengthSpec::Px(80.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![px_box("a", 20.0, 40.0), px_box("b", 20.0, 40.0)],
        text: None,
    };
    let boxes = box_map(&tree, 80.0, 80.0);
    assert!(
        (boxes["a"].x - 60.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "flex-direction:row in vertical-rl follows the inline axis from the right, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].x - 60.0).abs() < 0.5 && (boxes["b"].y - 40.0).abs() < 0.5,
        "second flex item stacks down the inline axis, got {:?}",
        boxes["b"]
    );
}

#[test]
fn rtl_row_flex_reverses_item_order_and_flips_justify() {
    // Characterisation: this already worked before RTL grid support, but no
    // test held it and `docs/reference/layout.md` claimed the opposite. Lock it down so
    // the contract is readable from the layout layer, not just the CSS mapping.
    let item = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(40.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(20.0)),
            dir: Some(DirSpec::Rtl),
            ..LayoutStyle::default()
        },
        children: vec![item("a"), item("b")],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 20.0);
    assert!(
        (boxes["a"].x - 160.0).abs() < 0.5,
        "first item sits at inline-start, which rtl puts on the right, got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].x - 120.0).abs() < 0.5,
        "second item runs leftwards from the first, got {:?}",
        boxes["b"]
    );
}

#[test]
fn rtl_grid_mirrors_column_order_and_justify_self() {
    let cell = |id: &str, justify: AlignSpec| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(30.0)),
            height: Some(LengthSpec::Px(20.0)),
            justify_self: Some(justify),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(20.0)),
            grid_columns: Some(vec![GridTrack::Px(80.0), GridTrack::Px(120.0)]),
            dir: Some(DirSpec::Rtl),
            ..LayoutStyle::default()
        },
        children: vec![cell("a", AlignSpec::Start), cell("b", AlignSpec::Start)],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 20.0);
    // Column 1 (80px) is the rightmost track: 200 - 80 = 120.
    // `justify-self: start` inside it means that track's right edge: 200 - 30.
    assert!(
        (boxes["a"].x - 170.0).abs() < 0.5,
        "grid column 1 is the rightmost track in rtl, and start means its right edge, got {:?}",
        boxes["a"]
    );
    // Column 2 (120px) occupies 0..120; start is its right edge: 120 - 30.
    assert!(
        (boxes["b"].x - 90.0).abs() < 0.5,
        "grid column 2 sits to the left of column 1 in rtl, got {:?}",
        boxes["b"]
    );
}

/// Grid columns are tracks along the inline axis and rows along the block one
/// (CSS Grid §3). In `vertical-rl` the columns run down the page and the rows
/// stack from the right; with `direction: rtl` the columns start at the
/// bottom.
#[test]
fn a_vertical_grid_lays_columns_down_the_inline_axis_and_rows_from_the_right() {
    let cell = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            justify_self: Some(AlignSpec::Stretch),
            align_self: Some(AlignSpec::Stretch),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let grid = |dir| StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(100.0)),
            height: Some(LengthSpec::Px(200.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            dir: Some(dir),
            grid_columns: Some(vec![GridTrack::Px(40.0), GridTrack::Px(60.0)]),
            grid_rows: Some(vec![GridTrack::Px(20.0), GridTrack::Px(30.0)]),
            ..LayoutStyle::default()
        },
        children: vec![cell("a"), cell("b"), cell("c")],
        text: None,
    };
    let near = |boxes: &HashMap<String, LayoutBox>, id: &str, expected: (f32, f32, f32, f32)| {
        let got = boxes[id];
        assert!(
            (got.x - expected.0).abs() < 0.5
                && (got.y - expected.1).abs() < 0.5
                && (got.width - expected.2).abs() < 0.5
                && (got.height - expected.3).abs() < 0.5,
            "{id}: expected {expected:?}, got {got:?}"
        );
    };
    // Auto-flow fills row 1 (the rightmost 20px), column by column down the
    // page, then row 2 (the 30px to its left).
    let ltr = box_map(&grid(DirSpec::Ltr), 100.0, 200.0);
    near(&ltr, "a", (80.0, 0.0, 20.0, 40.0));
    near(&ltr, "b", (80.0, 40.0, 20.0, 60.0));
    near(&ltr, "c", (50.0, 0.0, 30.0, 40.0));
    // RTL: column 1 is at the bottom of the 200px inline axis.
    let rtl = box_map(&grid(DirSpec::Rtl), 100.0, 200.0);
    near(&rtl, "a", (80.0, 160.0, 20.0, 40.0));
    near(&rtl, "b", (80.0, 100.0, 20.0, 60.0));
    near(&rtl, "c", (50.0, 160.0, 30.0, 40.0));
}

/// Percentage margins resolve against the containing block's inline size
/// (CSS Box Model §5): its height in a vertical writing mode, not its width.
#[test]
fn percentage_margins_resolve_against_the_inline_size_in_a_vertical_mode() {
    let child = StyleLayoutNode {
        id: "a".into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(20.0)),
            height: Some(LengthSpec::Px(20.0)),
            margin_top: Some(LengthSpec::Percent(10.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(100.0)),
            height: Some(LengthSpec::Px(200.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![child],
        text: None,
    };
    let boxes = box_map(&tree, 100.0, 200.0);
    assert!(
        (boxes["a"].y - 20.0).abs() < 0.5,
        "10% of the 200px inline size, not of the 100px width: {:?}",
        boxes["a"]
    );
}

/// A wrapping column packs its lines with the same margins it then places
/// with: percentage margins resolve against the containing block's inline
/// size — the width, even though a column's lines fill its height.
#[test]
fn a_wrapping_column_packs_with_percentage_margins_of_the_width() {
    let item = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(30.0)),
            height: Some(LengthSpec::Px(40.0)),
            margin_top: Some(LengthSpec::Percent(10.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Column),
            flex_wrap: FlexWrap::Wrap,
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(100.0)),
            align_items: AlignSpec::Start,
            align_content: JustifySpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![item("a"), item("b")],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 100.0);
    // 10% of the 200px width is 20px: 20 + 40 + 20 + 40 overflows 100, so
    // `b` starts the second column.
    assert!(
        (boxes["a"].y - 20.0).abs() < 0.5 && (boxes["b"].y - 20.0).abs() < 0.5,
        "both sit 20px down their column: a={:?} b={:?}",
        boxes["a"],
        boxes["b"]
    );
    assert!(
        boxes["b"].x >= boxes["a"].x + 30.0 - 0.5,
        "b wrapped to the next column: a={:?} b={:?}",
        boxes["a"],
        boxes["b"]
    );
}

/// Tracks wider than the content box overflow past the inline-start edge —
/// the left one in RTL. Clamping the mirrored x at zero piles every
/// overflowing column onto the first instead.
#[test]
fn rtl_grid_tracks_overflow_the_start_edge_instead_of_stacking() {
    let cell = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(20.0)),
            grid_columns: Some(vec![GridTrack::Px(200.0), GridTrack::Px(200.0)]),
            dir: Some(DirSpec::Rtl),
            ..LayoutStyle::default()
        },
        children: vec![cell("a"), cell("b")],
        text: None,
    };
    let boxes = box_map(&tree, 300.0, 20.0);
    // Track 1 is the rightmost: 300 - 200 = 100.
    assert!(
        (boxes["a"].x - 100.0).abs() < 0.5,
        "first track should sit at 100, got {:?}",
        boxes["a"]
    );
    // Track 2 mirrors to 300 - 200 - 200 = -100, overflowing the left edge.
    assert!(
        (boxes["b"].x + 100.0).abs() < 0.5,
        "second track should overflow to -100, got {:?}",
        boxes["b"]
    );
    assert!(
        boxes["b"].x + 200.0 <= boxes["a"].x + 0.5,
        "the two tracks must not overlap: {:?} vs {:?}",
        boxes["a"],
        boxes["b"]
    );
}

#[test]
fn rtl_grid_keeps_ltr_geometry_when_direction_is_default() {
    let cell = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(30.0)),
            height: Some(LengthSpec::Px(20.0)),
            justify_self: Some(AlignSpec::Start),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(20.0)),
            grid_columns: Some(vec![GridTrack::Px(80.0), GridTrack::Px(120.0)]),
            ..LayoutStyle::default()
        },
        children: vec![cell("a"), cell("b")],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 20.0);
    assert!((boxes["a"].x - 0.0).abs() < 0.5, "got {:?}", boxes["a"]);
    assert!((boxes["b"].x - 80.0).abs() < 0.5, "got {:?}", boxes["b"]);
}

#[test]
fn rtl_column_flex_puts_cross_start_on_the_right() {
    let item = |id: &str, width: f32| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(width)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Column),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(60.0)),
            align_items: AlignSpec::Start,
            dir: Some(DirSpec::Rtl),
            ..LayoutStyle::default()
        },
        children: vec![item("a", 40.0), item("b", 60.0)],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 60.0);
    // Column flex: the cross axis is the inline axis, so rtl `align-items:
    // start` is the right edge. The main (block) axis is untouched.
    assert!(
        (boxes["a"].x - 160.0).abs() < 0.5 && (boxes["a"].y - 0.0).abs() < 0.5,
        "got {:?}",
        boxes["a"]
    );
    assert!(
        (boxes["b"].x - 140.0).abs() < 0.5 && (boxes["b"].y - 20.0).abs() < 0.5,
        "got {:?}",
        boxes["b"]
    );
}

#[test]
fn rtl_row_flex_cross_axis_stays_block_axis() {
    // A row container's cross axis is vertical; rtl must not touch it.
    let item = |id: &str, height: f32| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(40.0)),
            height: Some(LengthSpec::Px(height)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(60.0)),
            align_items: AlignSpec::Start,
            dir: Some(DirSpec::Rtl),
            ..LayoutStyle::default()
        },
        children: vec![item("a", 20.0), item("b", 30.0)],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 60.0);
    assert!((boxes["a"].y - 0.0).abs() < 0.5, "got {:?}", boxes["a"]);
    assert!((boxes["b"].y - 0.0).abs() < 0.5, "got {:?}", boxes["b"]);
}

/// A vertical RTL box's inline axis runs from the bottom (CSS Writing Modes
/// §2.1): the first inline, and the first item of a `flex-direction: row`
/// container, sit at the foot of the column.
#[test]
fn writing_mode_vertical_rtl_starts_the_inline_axis_at_the_bottom() {
    let inline = |id: &str| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(20.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let container = |display, direction| StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(display),
            direction,
            width: Some(LengthSpec::Px(80.0)),
            height: Some(LengthSpec::Px(80.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            dir: Some(DirSpec::Rtl),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![inline("a"), inline("b")],
        text: None,
    };
    for (what, tree) in [
        (
            "inline formatting context",
            container(DisplaySpec::Block, None),
        ),
        (
            "flex-direction: row",
            container(DisplaySpec::Flex, Some(FlexDirection::Row)),
        ),
    ] {
        let boxes = box_map(&tree, 80.0, 80.0);
        assert!(
            (boxes["a"].y - 60.0).abs() < 0.5 && (boxes["b"].y - 40.0).abs() < 0.5,
            "{what}: the first item sits at the bottom, the next above it, got a={:?} b={:?}",
            boxes["a"],
            boxes["b"]
        );
        assert!(
            (boxes["a"].x - 60.0).abs() < 0.5,
            "{what}: block-start is still the right, got {:?}",
            boxes["a"]
        );
    }
}

/// In a vertical RTL `flex-direction: column` container the cross axis is the
/// inline one, so `align-items: start` is the bottom.
#[test]
fn writing_mode_vertical_rtl_column_flex_puts_cross_start_at_the_bottom() {
    let item = |id: &str, height: f32| StyleLayoutNode {
        id: id.into(),
        style: LayoutStyle {
            width: Some(LengthSpec::Px(20.0)),
            height: Some(LengthSpec::Px(height)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Column),
            width: Some(LengthSpec::Px(80.0)),
            height: Some(LengthSpec::Px(80.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            dir: Some(DirSpec::Rtl),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![item("a", 20.0), item("b", 30.0)],
        text: None,
    };
    let boxes = box_map(&tree, 80.0, 80.0);
    assert!(
        (boxes["a"].y - 60.0).abs() < 0.5 && (boxes["b"].y - 50.0).abs() < 0.5,
        "cross-start is the bottom, got a={:?} b={:?}",
        boxes["a"],
        boxes["b"]
    );
    assert!(
        boxes["a"].x > boxes["b"].x,
        "the main axis is the block one, from the right: a={:?} b={:?}",
        boxes["a"],
        boxes["b"]
    );
}

/// An RTL row that wraps fills its first line with its *first* items, laid
/// from the right: placement is flow-relative, and the page is only turned
/// when an origin is written. (Reversing the item list before wrapping put
/// the last items on the first line.)
#[test]
fn rtl_wrapping_row_fills_its_first_line_with_its_first_items() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            flex_wrap: FlexWrap::Wrap,
            width: Some(LengthSpec::Px(100.0)),
            height: Some(LengthSpec::Px(40.0)),
            align_items: AlignSpec::Start,
            dir: Some(DirSpec::Rtl),
            ..LayoutStyle::default()
        },
        children: (1..=5)
            .map(|index| px_box(&format!("item{index}"), 30.0, 20.0))
            .collect(),
        text: None,
    };
    let boxes = box_map(&tree, 100.0, 40.0);
    // Three 30px items to a 100px line: 1 2 3 on the first, 4 5 on the next.
    for (id, x, y) in [
        ("item1", 70.0, 0.0),
        ("item2", 40.0, 0.0),
        ("item3", 10.0, 0.0),
        ("item4", 70.0, 20.0),
        ("item5", 40.0, 20.0),
    ] {
        assert!(
            (boxes[id].x - x).abs() < 0.5 && (boxes[id].y - y).abs() < 0.5,
            "{id} at ({x}, {y}), got {:?}",
            boxes[id]
        );
    }
}

/// In `vertical-rl` the cross axis of a `flex-direction: row` container is
/// the block axis, which starts at the right: `align-items: start` puts every
/// item against the right edge, not the line against it and a narrower item
/// at the line's far side.
#[test]
fn vertical_rl_cross_start_is_the_right_edge_for_every_item() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(100.0)),
            height: Some(LengthSpec::Px(100.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![px_box("wide", 40.0, 30.0), px_box("narrow", 20.0, 30.0)],
        text: None,
    };
    let boxes = box_map(&tree, 100.0, 100.0);
    assert!(
        (boxes["wide"].x - 60.0).abs() < 0.5,
        "got {:?}",
        boxes["wide"]
    );
    assert!(
        (boxes["narrow"].x - 80.0).abs() < 0.5,
        "the narrow item hugs the right edge too, got {:?}",
        boxes["narrow"]
    );
    assert!(
        (boxes["narrow"].y - 30.0).abs() < 0.5,
        "got {:?}",
        boxes["narrow"]
    );
}

#[test]
fn writing_mode_vertical_shaper_keeps_horizontal_metrics() {
    let metrics = crate::MeasureTextShaper.shape(
        id(1),
        &TextContent {
            value: "Hello".into(),
        },
        &ComputedStyle {
            writing_mode: WritingModeSpec::VerticalRl,
            font_size: 10.0,
            ..ComputedStyle::default()
        },
        crate::TextShapeConstraints::default(),
    );
    assert!(
        metrics.width > metrics.height,
        "layout shaper must not swap metrics to fake glyph rotation, got {metrics:?}"
    );
}

#[test]
fn align_items_baseline_uses_shaped_ascent_not_approx_em() {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                display: Some(DisplaySpec::Flex),
                direction: Some(FlexDirection::Row),
                align_items: AlignSpec::Baseline,
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(80.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    for (value, font) in [(3u64, 20.0), (4u64, 20.0)] {
        queue.create(id(value), document, NodeKind::Text);
        queue.insert(id(2), id(value), None);
        queue.set_style(
            id(value),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    font_size: Some(font),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        queue.set_text(
            id(value),
            TextContent {
                value: if value == 3 { "low" } else { "high" }.into(),
            },
        );
    }
    world.commit(queue).unwrap();
    struct AscentShaper;
    impl TextShaper for AscentShaper {
        fn shape(
            &mut self,
            id: StableNodeId,
            _text: &TextContent,
            _style: &ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            if id.get() == 3 {
                TextMetrics {
                    width: 40.0,
                    height: 20.0,
                    ascent: Some(8.0),
                }
            } else {
                TextMetrics {
                    width: 40.0,
                    height: 20.0,
                    ascent: Some(16.0),
                }
            }
        }
    }
    world
        .shape_text(&[id(3), id(4)], &mut AscentShaper)
        .unwrap();
    let layouts = RuntimeLayoutEngine
        .layout_document(&world, document, LayoutViewport::new(200.0, 80.0))
        .unwrap()
        .into_iter()
        .collect::<HashMap<_, _>>();
    let approx = nana_ui_core::TEXT_APPROX_ASCENT_EM * 20.0;
    assert!(
        (layouts[&id(4)].y - 0.0).abs() < 0.5,
        "taller ascent anchors the line, got {:?}",
        layouts[&id(4)]
    );
    assert!(
        (layouts[&id(3)].y - 8.0).abs() < 0.5,
        "shaped ascent 8 vs 16 must shift y by 8, not 0.8em ({approx}), got {:?}",
        layouts[&id(3)]
    );
}

#[test]
fn named_line_nth_uses_second_foo() {
    let item = StyleLayoutNode {
        id: "cell".into(),
        style: LayoutStyle {
            height: Some(LengthSpec::Px(40.0)),
            grid_placement: GridPlacement {
                column_start: GridLine::NthName("foo".into(), 2),
                column_end: GridLine::Name("foo".into()),
                ..GridPlacement::default()
            }
            .into(),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(40.0)),
            grid_columns: Some(vec![GridTrack::Px(80.0), GridTrack::Px(120.0)]),
            grid_column_line_names: Some(vec![
                vec!["foo".into()],
                vec!["foo".into()],
                vec!["foo".into()],
            ]),
            ..LayoutStyle::default()
        },
        children: vec![item],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 40.0);
    assert!(
        (boxes["cell"].x - 80.0).abs() < 0.5 && (boxes["cell"].width - 120.0).abs() < 0.5,
        "foo 2 / next foo must be the 120px track, got {:?}",
        boxes["cell"]
    );
}

#[test]
fn auto_fill_nth_named_line_uses_expanded_copies() {
    let item = StyleLayoutNode {
        id: "cell".into(),
        style: LayoutStyle {
            height: Some(LengthSpec::Px(40.0)),
            grid_placement: GridPlacement {
                column_start: GridLine::NthName("mid".into(), 2),
                column_end: GridLine::Name("mid".into()),
                ..GridPlacement::default()
            }
            .into(),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Grid),
            width: Some(LengthSpec::Px(240.0)),
            height: Some(LengthSpec::Px(40.0)),
            grid_columns_repeat: Some(GridRepeatAuto {
                kind: GridTrackListUnsupported::RepeatAutoFill,
                tracks: vec![GridTrack::Px(80.0)],
                pattern_line_names: vec![vec!["mid".into()], Vec::new()],
                ..Default::default()
            }),
            // Pattern stored once — engine must expand, not resolve mid 2
            // against this single copy (which would miss and auto-place at 0).
            grid_column_line_names: Some(vec![vec!["mid".into()], Vec::new()]),
            ..LayoutStyle::default()
        },
        children: vec![item],
        text: None,
    };
    let boxes = box_map(&tree, 240.0, 40.0);
    assert!(
        (boxes["cell"].x - 80.0).abs() < 0.5 && (boxes["cell"].width - 80.0).abs() < 0.5,
        "mid 2 after auto-fit expansion must be the second 80px track, got {:?}",
        boxes["cell"]
    );
}

#[test]
fn grid_auto_slot_overflow_does_not_reuse_origin() {
    let occupied = {
        let mut occ = GridOccupancy::default();
        occ.occupy(0, 0, 1, 2);
        occ
    };
    let (row, col) = search_grid_auto_slot(&occupied, Some(0), None, 1, 1, 2, 1, 0, 0, false);
    assert!(
        !(row == 0 && col == 0),
        "full explicit row must not silently place at (0,0), got ({row},{col})"
    );
    assert_eq!(row, 0);
    assert!(col >= 2, "implicit column past wrap, got {col}");
}

fn spacing_tree(styles: &[(u64, LayoutStyle)]) -> HashMap<StableNodeId, LayoutBox> {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut q = MutationQueue::new();
    q.create(id(1), document, NodeKind::Document);
    for (i, (parent, layout)) in styles.iter().enumerate() {
        let node = id(i as u64 + 2);
        q.create(node, document, NodeKind::Element { tag: "div".into() });
        q.insert(id(*parent), node, None);
        q.set_style(
            node,
            NodeStyle {
                layout: Arc::new(layout.clone()),
                ..Default::default()
            },
        );
    }
    world.commit(q).unwrap();
    full_boxes(&world, document, LayoutViewport::new(400.0, 300.0))
}

#[test]
fn spacing_hug_includes_child_margins_on_both_axes() {
    for direction in [FlexDirection::Row, FlexDirection::Column] {
        let boxes = spacing_tree(&[
            (
                1,
                LayoutStyle {
                    width: Some(LengthSpec::Shrink),
                    height: Some(LengthSpec::Shrink),
                    direction: Some(direction),
                    ..Default::default()
                },
            ),
            (
                2,
                LayoutStyle {
                    width: Some(LengthSpec::Px(40.0)),
                    height: Some(LengthSpec::Px(20.0)),
                    margin: Some(LengthSpec::Px(10.0)),
                    ..Default::default()
                },
            ),
        ]);
        assert_eq!((boxes[&id(2)].width, boxes[&id(2)].height), (60.0, 40.0));
        assert_eq!((boxes[&id(3)].x, boxes[&id(3)].y), (10.0, 10.0));
    }
}

#[test]
fn spacing_percent_padding_uses_containing_width() {
    let boxes = spacing_tree(&[
        (
            1,
            LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                ..Default::default()
            },
        ),
        (
            2,
            LayoutStyle {
                width: Some(LengthSpec::Px(100.0)),
                padding: Some(LengthSpec::Percent(10.0)),
                ..Default::default()
            },
        ),
        (
            3,
            LayoutStyle {
                width: Some(LengthSpec::Px(10.0)),
                height: Some(LengthSpec::Px(10.0)),
                ..Default::default()
            },
        ),
    ]);
    assert_eq!(boxes[&id(4)].x - boxes[&id(3)].x, 20.0);
    assert_eq!(boxes[&id(4)].y - boxes[&id(3)].y, 20.0);
    assert_eq!(boxes[&id(3)].height, 50.0);
}

#[test]
fn spacing_negative_margin_extends_flex_fill_budget() {
    let boxes = spacing_tree(&[
        (
            1,
            LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                direction: Some(FlexDirection::Row),
                ..Default::default()
            },
        ),
        (
            2,
            LayoutStyle {
                width: Some(LengthSpec::Fill),
                margin_left: Some(LengthSpec::Px(-10.0)),
                margin_right: Some(LengthSpec::Px(-10.0)),
                height: Some(LengthSpec::Px(20.0)),
                ..Default::default()
            },
        ),
    ]);
    assert_eq!(boxes[&id(3)].width, 220.0);
    assert_eq!(boxes[&id(3)].x, -10.0);
}

#[test]
fn spacing_center_aligns_asymmetric_margin_box() {
    let boxes = spacing_tree(&[
        (
            1,
            LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(100.0)),
                direction: Some(FlexDirection::Row),
                align_items: AlignSpec::Center,
                ..Default::default()
            },
        ),
        (
            2,
            LayoutStyle {
                width: Some(LengthSpec::Px(20.0)),
                height: Some(LengthSpec::Px(20.0)),
                margin_top: Some(LengthSpec::Px(30.0)),
                margin_bottom: Some(LengthSpec::Px(10.0)),
                ..Default::default()
            },
        ),
    ]);
    assert_eq!(boxes[&id(3)].y, 50.0);
}

#[test]
fn spacing_grid_padding_and_margin_use_final_cell() {
    let boxes = spacing_tree(&[
        (
            1,
            LayoutStyle {
                display: Some(DisplaySpec::Grid),
                width: Some(LengthSpec::Px(300.0)),
                height: Some(LengthSpec::Px(100.0)),
                grid_columns: Some(vec![GridTrack::Px(100.0), GridTrack::Px(200.0)]),
                ..Default::default()
            },
        ),
        (
            2,
            LayoutStyle {
                width: Some(LengthSpec::Fill),
                margin: Some(LengthSpec::Px(10.0)),
                padding: Some(LengthSpec::Percent(10.0)),
                ..Default::default()
            },
        ),
        (
            3,
            LayoutStyle {
                width: Some(LengthSpec::Px(10.0)),
                height: Some(LengthSpec::Px(10.0)),
                ..Default::default()
            },
        ),
    ]);
    assert_eq!(boxes[&id(3)].x, 10.0);
    assert_eq!(boxes[&id(3)].width, 80.0);
    assert_eq!(boxes[&id(4)].x - boxes[&id(3)].x, 10.0);
}

#[test]
fn spacing_fixed_box_reflows_percent_padding_when_parent_resizes() {
    use crate::{AppContext, Stack};
    let document = DocumentId::new(42).unwrap();
    let mut context = AppContext::new();
    let root = context
        .create_component(document, Stack::column(0.0).width(LengthSpec::Px(200.0)))
        .unwrap();
    let child = context
        .create_detached_component(
            document,
            Stack::from_layout(LayoutStyle {
                width: Some(LengthSpec::Px(100.0)),
                height: Some(LengthSpec::Px(100.0)),
                padding: Some(LengthSpec::Percent(10.0)),
                ..Default::default()
            }),
        )
        .unwrap();
    let leaf = context
        .create_detached_component(document, Stack::column(0.0).height(LengthSpec::Px(10.0)))
        .unwrap();
    context.append_child(root, child).unwrap();
    context.append_child(child, leaf).unwrap();
    let viewport = LayoutViewport::new(400.0, 300.0);
    context.layout_document(document, viewport).unwrap();
    let before = context.world().layout_box(child.stable_id()).unwrap();
    assert_eq!(
        context.world().layout_box(leaf.stable_id()).unwrap().x,
        20.0
    );
    context
        .update_component(root, |root, _| {
            *root = root.clone().width(LengthSpec::Px(300.0))
        })
        .unwrap();
    context
        .layout_document_scoped(document, viewport, &[root.stable_id()])
        .unwrap();
    assert_eq!(
        context.world().layout_box(child.stable_id()).unwrap(),
        before
    );
    assert_eq!(
        context.world().layout_box(leaf.stable_id()).unwrap().x,
        30.0
    );
    let extracted = context.world().extract_nodes(&[child.stable_id()]);
    assert_eq!(
        extracted[0].source_style.layout.resolved_padding().left,
        30.0
    );
    assert_eq!(
        context
            .world()
            .node_style(child.stable_id())
            .unwrap()
            .layout
            .padding,
        Some(LengthSpec::Percent(10.0))
    );
}

#[test]
fn spacing_grid_content_box_keeps_padding_in_border_size() {
    let boxes = spacing_tree(&[
        (
            1,
            LayoutStyle {
                display: Some(DisplaySpec::Grid),
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(100.0)),
                grid_columns: Some(vec![GridTrack::Px(200.0)]),
                ..Default::default()
            },
        ),
        (
            2,
            LayoutStyle {
                width: Some(LengthSpec::Px(40.0)),
                height: Some(LengthSpec::Px(20.0)),
                box_sizing: BoxSizing::ContentBox,
                padding: Some(LengthSpec::Px(10.0)),
                margin: Some(LengthSpec::Px(5.0)),
                ..Default::default()
            },
        ),
    ]);
    assert_eq!((boxes[&id(3)].x, boxes[&id(3)].y), (5.0, 5.0));
    assert_eq!((boxes[&id(3)].width, boxes[&id(3)].height), (60.0, 40.0));
}

#[test]
fn lazy_layout_input_avoids_a_single_item_projection_batch() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), DocumentId::new(1).unwrap(), NodeKind::Text);
    world.commit(queue).unwrap();
    let expected = world.layout_inputs(&[id(1)]).unwrap().pop().unwrap();
    let before = world.last_work_counters().allocations;
    let mut inputs = LayoutInputMap::new(&world);
    assert_eq!(inputs.get(id(1)).unwrap(), Some(&expected));
    assert_eq!(inputs.get(id(1)).unwrap(), Some(&expected));
    assert!(inputs.get(id(2)).unwrap().is_none());
    assert_eq!(inputs.materialized, 1);
    // This counter covers projection output batches, not the map's own storage.
    assert_eq!(world.last_work_counters().allocations, before);
}

// ---------------------------------------------------------------------------
// Differential harness for scoped layout.
//
// Every scoped-layout optimization is a decision to NOT recompute something.
// That class of change fails silently: the boxes are simply stale, and no
// assertion in the rest of the suite looks at a node the optimizer decided to
// skip. So the guard has to be an equivalence, driven over enough container
// shapes that a "fast path" which is only valid for simple flex columns cannot
// slip through.
//
// The harness drives a sequence of mutations through the SCOPED entry point,
// exactly as `RuntimeDocument::flush` does (drain work -> scoped layout ->
// write back changed boxes), and after every step compares every node against
// a full recompute from scratch.
// ---------------------------------------------------------------------------

/// One container shape to run the mutation sequence against.
struct DiffShape {
    name: &'static str,
    container: LayoutStyle,
    /// Style for each row; `usize` is the row index.
    row: fn(usize) -> LayoutStyle,
}

fn diff_shapes() -> Vec<DiffShape> {
    fn plain_row(_: usize) -> LayoutStyle {
        LayoutStyle {
            width: Some(LengthSpec::Px(60.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        }
    }
    fn growing_row(index: usize) -> LayoutStyle {
        LayoutStyle {
            width: Some(LengthSpec::Px(60.0)),
            height: Some(LengthSpec::Px(20.0)),
            flex_grow: Some(if index.is_multiple_of(3) { 1.0 } else { 0.0 }),
            flex_shrink: Some(1.0),
            ..LayoutStyle::default()
        }
    }
    fn margined_row(index: usize) -> LayoutStyle {
        LayoutStyle {
            width: Some(LengthSpec::Px(60.0)),
            height: Some(LengthSpec::Px(20.0)),
            margin_top: Some(LengthSpec::Px(index as f32 % 4.0)),
            margin_bottom: Some(LengthSpec::Px(2.0)),
            ..LayoutStyle::default()
        }
    }
    fn auto_margin_row(index: usize) -> LayoutStyle {
        let mut style = plain_row(index);
        if index.is_multiple_of(5) {
            style.margin_top = Some(LengthSpec::Auto);
        }
        style
    }

    let column = |extra: fn(&mut LayoutStyle)| {
        let mut style = LayoutStyle {
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(400.0)),
            direction: Some(FlexDirection::Column),
            ..LayoutStyle::default()
        };
        extra(&mut style);
        style
    };

    vec![
        DiffShape {
            name: "column-plain",
            container: column(|_| {}),
            row: plain_row,
        },
        DiffShape {
            name: "column-gap",
            container: column(|s| s.gap = Some(LengthSpec::Px(7.0))),
            row: plain_row,
        },
        DiffShape {
            name: "column-justify-center",
            container: column(|s| s.justify_content = JustifySpec::Center),
            row: plain_row,
        },
        DiffShape {
            name: "column-justify-space-between",
            container: column(|s| s.justify_content = JustifySpec::SpaceBetween),
            row: plain_row,
        },
        DiffShape {
            name: "column-justify-end",
            container: column(|s| s.justify_content = JustifySpec::End),
            row: plain_row,
        },
        DiffShape {
            name: "column-align-center",
            container: column(|s| s.align_items = AlignSpec::Center),
            row: plain_row,
        },
        DiffShape {
            name: "column-align-baseline",
            container: column(|s| s.align_items = AlignSpec::Baseline),
            row: plain_row,
        },
        DiffShape {
            name: "column-wrap",
            container: column(|s| {
                s.flex_wrap = FlexWrap::Wrap;
                s.height = Some(LengthSpec::Px(120.0));
            }),
            row: plain_row,
        },
        DiffShape {
            name: "column-grow",
            container: column(|_| {}),
            row: growing_row,
        },
        DiffShape {
            name: "column-margins",
            container: column(|s| s.gap = Some(LengthSpec::Px(3.0))),
            row: margined_row,
        },
        DiffShape {
            name: "column-auto-margins",
            container: column(|_| {}),
            row: auto_margin_row,
        },
        // Reversed axes: placement is flow-relative and turned onto the page
        // where an origin is written, and the sequential replay does the same.
        // `margined_row`'s top and bottom margins differ, so on a reversed
        // column the leading margin is the bottom one.
        DiffShape {
            name: "column-reverse-margins",
            container: column(|s| {
                s.flex_reverse = true;
                s.gap = Some(LengthSpec::Px(3.0));
            }),
            row: margined_row,
        },
        DiffShape {
            name: "column-rtl-cross-start-right",
            container: column(|s| s.dir = Some(DirSpec::Rtl)),
            row: plain_row,
        },
        DiffShape {
            name: "row-rtl",
            container: LayoutStyle {
                width: Some(LengthSpec::Px(400.0)),
                height: Some(LengthSpec::Px(80.0)),
                direction: Some(FlexDirection::Row),
                dir: Some(DirSpec::Rtl),
                ..LayoutStyle::default()
            },
            row: plain_row,
        },
        DiffShape {
            name: "vertical-rl-row",
            container: column(|s| {
                s.direction = Some(FlexDirection::Row);
                s.writing_mode = Some(WritingModeSpec::VerticalRl);
            }),
            row: margined_row,
        },
        DiffShape {
            name: "vertical-rl-rtl-row",
            container: column(|s| {
                s.direction = Some(FlexDirection::Row);
                s.writing_mode = Some(WritingModeSpec::VerticalRl);
                s.dir = Some(DirSpec::Rtl);
                s.gap = Some(LengthSpec::Px(2.0));
            }),
            row: margined_row,
        },
        DiffShape {
            name: "row-plain",
            container: LayoutStyle {
                width: Some(LengthSpec::Px(400.0)),
                height: Some(LengthSpec::Px(80.0)),
                direction: Some(FlexDirection::Row),
                ..LayoutStyle::default()
            },
            row: plain_row,
        },
        // Content-driven containers: the definite-size short circuit in
        // `intrinsic_size_scoped` cannot fire, so these are the shapes that
        // reach `MeasurePlan` at all. Every edit below therefore has to survive
        // BOTH plans agreeing to skip work.
        DiffShape {
            name: "column-auto-height",
            container: column(|s| s.height = None),
            row: plain_row,
        },
        DiffShape {
            name: "column-auto-both",
            container: column(|s| {
                s.width = None;
                s.height = None;
            }),
            row: plain_row,
        },
        DiffShape {
            name: "column-auto-height-gap-margins",
            container: column(|s| {
                s.height = None;
                s.gap = Some(LengthSpec::Px(3.0));
            }),
            row: margined_row,
        },
        DiffShape {
            name: "column-auto-height-align-center",
            container: column(|s| {
                s.height = None;
                s.align_items = AlignSpec::Center;
            }),
            row: plain_row,
        },
        DiffShape {
            name: "column-auto-height-grow",
            container: column(|s| s.height = None),
            row: growing_row,
        },
        DiffShape {
            name: "row-auto-width",
            container: LayoutStyle {
                width: None,
                height: Some(LengthSpec::Px(80.0)),
                direction: Some(FlexDirection::Row),
                ..LayoutStyle::default()
            },
            row: plain_row,
        },
        DiffShape {
            name: "column-auto-height-padded",
            container: column(|s| {
                s.height = None;
                s.padding = Some(LengthSpec::Px(6.0));
                s.box_sizing = BoxSizing::ContentBox;
            }),
            row: plain_row,
        },
        DiffShape {
            name: "row-wrap",
            container: LayoutStyle {
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(200.0)),
                direction: Some(FlexDirection::Row),
                flex_wrap: FlexWrap::Wrap,
                ..LayoutStyle::default()
            },
            row: plain_row,
        },
    ]
}

/// Root = id(1), container = id(2), row r = id(3 + r*2), its label = id(4 + r*2).
fn diff_tree(shape: &DiffShape, rows: usize) -> (UiWorld, DocumentId) {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(shape.container.clone()),
            ..NodeStyle::default()
        },
    );
    for row in 0..rows {
        let row_id = id(3 + row as u64 * 2);
        let label_id = id(4 + row as u64 * 2);
        queue.create(row_id, document, NodeKind::Element { tag: "div".into() });
        queue.create(label_id, document, NodeKind::Text);
        queue.insert(id(2), row_id, None);
        queue.insert(row_id, label_id, None);
        queue.set_text(
            label_id,
            TextContent {
                value: format!("r{row}").into(),
            },
        );
        queue.set_style(
            row_id,
            NodeStyle {
                layout: Arc::new((shape.row)(row)),
                ..NodeStyle::default()
            },
        );
    }
    world.commit(queue).unwrap();
    (world, document)
}

/// Run one scoped pass the way the frame driver does, then assert the retained
/// cache agrees with a full recompute at EVERY node.
/// Emitted boxes and children measured BY THE SCOPED PASS. The counters have to
/// be read before the verification recompute below, which measures everything
/// by definition.
struct ScopedStep {
    emitted: usize,
    children_measured: usize,
    measure_plans_reused: usize,
    plans_reused: usize,
}

fn scoped_step_matches_full(
    world: &mut UiWorld,
    document: DocumentId,
    viewport: LayoutViewport,
    retained: &mut RetainedLayoutCache,
    label: &str,
) -> ScopedStep {
    let work = world.take_system_work();
    // Layout reads inherited writing context from the resolved style, as the
    // frame pipeline resolves it before layout.
    world.resolve_styles(&work.style).unwrap();
    super::plan_stats::reset();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(world, document, viewport, &work.layout, retained, false)
        .unwrap();
    let step = ScopedStep {
        emitted: emitted.len(),
        children_measured: super::plan_stats::children_measured(),
        measure_plans_reused: super::plan_stats::measure_plans_reused(),
        plans_reused: super::plan_stats::plans_reused(),
    };
    write_changed_boxes(world, &emitted);
    let _ = world.take_system_work();

    let expected = full_boxes(world, document, viewport);
    let cached = &retained.documents[&document].boxes;
    for (node, box_) in &expected {
        // A node that generates no box at all -- `display:contents` -- is never
        // placed, so the scoped pass never emits it and the retained map has no
        // entry, while the full pass fills its `document_order` slot with a
        // default. Absent and zero are the same statement here: the retained
        // map only ever loses an entry by never having had one, so this cannot
        // hide a box the scoped pass dropped.
        let cached = cached.get(node).copied().unwrap_or_default();
        assert_eq!(
            cached, *box_,
            "{label}: scoped layout diverged from full recompute at {node:?}"
        );
    }
    step
}

/// A reversed axis keeps the sequential replay: a child's position still
/// depends only on the children before it, measured back from the far edge.
/// Edit one row mid-list and the container plan has to be reused — and agree
/// with a full recompute — on every reversed shape.
#[test]
fn reversed_axes_keep_the_sequential_replay() {
    const ROWS: usize = 12;
    let viewport = LayoutViewport::new(320.0, 400.0);
    let reversed = [
        "column-reverse-margins",
        "column-rtl-cross-start-right",
        "vertical-rl-row",
        "vertical-rl-rtl-row",
    ];
    for shape in diff_shapes()
        .into_iter()
        .filter(|shape| reversed.contains(&shape.name))
    {
        let (mut world, document) = diff_tree(&shape, ROWS);
        let mut retained = RetainedLayoutCache::default();
        let _ = world.take_system_work();
        let emitted = RuntimeLayoutEngine
            .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
            .unwrap();
        write_changed_boxes(&mut world, &emitted);
        let _ = world.take_system_work();
        for (step, height) in [26.0f32, 14.0].into_iter().enumerate() {
            let row = ROWS / 2;
            let mut style = (shape.row)(row);
            style.height = Some(LengthSpec::Px(height));
            let mut queue = MutationQueue::new();
            queue.set_style(
                id(3 + row as u64 * 2),
                NodeStyle {
                    layout: Arc::new(style),
                    ..NodeStyle::default()
                },
            );
            world.commit(queue).unwrap();
            super::plan_stats::reset();
            let work = world.take_system_work();
            let emitted = RuntimeLayoutEngine
                .layout_document_scoped(
                    &world,
                    document,
                    viewport,
                    &work.layout,
                    &mut retained,
                    false,
                )
                .unwrap();
            let replayed = super::plan_stats::suffixes_replayed();
            write_changed_boxes(&mut world, &emitted);
            let _ = world.take_system_work();
            // The full comparison, after the counter was read.
            let expected = full_boxes(&world, document, viewport);
            let cached = &retained.documents[&document].boxes;
            for (node, box_) in &expected {
                assert_eq!(
                    cached.get(node).copied().unwrap_or_default(),
                    *box_,
                    "{} step {step}: scoped layout diverged at {node:?}",
                    shape.name
                );
            }
            assert!(
                replayed > 0,
                "{} step {step}: the container must replay its suffix, not relayout",
                shape.name
            );
        }
    }
}

/// An ancestor's `direction` reaches a row that never declared one: the row
/// inherits RTL and its label moves to the right end, while the row's own
/// style, its child list and its label's size are all unchanged. Only the
/// inherited writing context tells the row's cached placement plan it is
/// stale.
#[test]
fn an_inherited_direction_change_retires_a_descendant_containers_plan() {
    const ROWS: usize = 6;
    let viewport = LayoutViewport::new(320.0, 400.0);
    let shape = DiffShape {
        name: "rows-of-labels",
        container: LayoutStyle {
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(400.0)),
            direction: Some(FlexDirection::Column),
            ..LayoutStyle::default()
        },
        row: |_| LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            width: Some(LengthSpec::Px(300.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
    };
    let (mut world, document) = diff_tree(&shape, ROWS);
    let mut retained = RetainedLayoutCache::default();
    let _ = world.take_system_work();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    let _ = world.take_system_work();
    // A scoped pass first, so every row has a plan to be tempted by.
    let mut queue = MutationQueue::new();
    let mut row = (shape.row)(0);
    row.height = Some(LengthSpec::Px(22.0));
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(row),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    scoped_step_matches_full(&mut world, document, viewport, &mut retained, "prime");
    let label_before = retained.documents[&document].boxes[&id(6)];

    let mut container = shape.container.clone();
    container.dir = Some(DirSpec::Rtl);
    let mut queue = MutationQueue::new();
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(container),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "container turned rtl",
    );
    let label_after = retained.documents[&document].boxes[&id(6)];
    assert!(
        label_after.x > label_before.x,
        "the row inherited rtl and its label moved right: {label_before:?} -> {label_after:?}"
    );
}

/// The equivalence itself: for every container shape, a sequence of changes at
/// the head, middle and tail of the child list must keep scoped layout
/// identical to a full recompute.
#[test]
fn scoped_layout_matches_full_recompute_across_container_shapes_and_edits() {
    const ROWS: usize = 24;
    let viewport = LayoutViewport::new(320.0, 400.0);
    for shape in diff_shapes() {
        // If the equivalence below passed without ever consulting `MeasurePlan`
        // -- a guard that went permanently false, a slot that never matched --
        // the harness would be proving nothing about the cache it was extended
        // to cover. Every shape reaches it, content-driven container or not,
        // because the document root is itself content-driven.
        let mut measure_plans_reused = 0usize;
        let (mut world, document) = diff_tree(&shape, ROWS);
        let mut retained = RetainedLayoutCache::default();
        let _ = world.take_system_work();
        // Bootstrap exactly like the driver's first frame.
        let emitted = RuntimeLayoutEngine
            .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
            .unwrap();
        write_changed_boxes(&mut world, &emitted);
        let _ = world.take_system_work();

        // Edits at the tail, middle and head, each repeated so a change and a
        // change-back both go through the scoped path.
        for &row in &[ROWS - 1, ROWS / 2, 0, ROWS - 1, ROWS / 2] {
            for &height in &[26.0f32, 20.0, 13.0] {
                let mut style = (shape.row)(row);
                style.height = Some(LengthSpec::Px(height));
                let mut queue = MutationQueue::new();
                queue.set_style(
                    id(3 + row as u64 * 2),
                    NodeStyle {
                        layout: Arc::new(style),
                        ..NodeStyle::default()
                    },
                );
                world.commit(queue).unwrap();
                measure_plans_reused += scoped_step_matches_full(
                    &mut world,
                    document,
                    viewport,
                    &mut retained,
                    &format!("{} row {row} height {height}", shape.name),
                )
                .measure_plans_reused;
            }
        }

        // Edits that MOVE a child without resizing it. The intrinsic-size check
        // cannot see these, so they are what the per-child style comparison is
        // for: margin, alignment and order all change placement while the
        // measured box stays identical.
        for (label, mutate) in [
            (
                "margin",
                (|style: &mut LayoutStyle| style.margin_top = Some(LengthSpec::Px(9.0)))
                    as fn(&mut LayoutStyle),
            ),
            ("align-self", |style: &mut LayoutStyle| {
                style.align_self = Some(AlignSpec::End)
            }),
            ("order", |style: &mut LayoutStyle| style.order = -1),
            ("grow", |style: &mut LayoutStyle| {
                style.flex_grow = Some(4.0)
            }),
        ] {
            let row = ROWS / 4;
            let mut style = (shape.row)(row);
            mutate(&mut style);
            let mut queue = MutationQueue::new();
            queue.set_style(
                id(3 + row as u64 * 2),
                NodeStyle {
                    layout: Arc::new(style),
                    ..NodeStyle::default()
                },
            );
            world.commit(queue).unwrap();
            measure_plans_reused += scoped_step_matches_full(
                &mut world,
                document,
                viewport,
                &mut retained,
                &format!("{} {label}-only edit", shape.name),
            )
            .measure_plans_reused;
        }

        // A width change on a middle row: cross-axis, not main-axis.
        let mut style = (shape.row)(ROWS / 3);
        style.width = Some(LengthSpec::Px(90.0));
        let mut queue = MutationQueue::new();
        queue.set_style(
            id(3 + (ROWS / 3) as u64 * 2),
            NodeStyle {
                layout: Arc::new(style),
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        measure_plans_reused += scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            &format!("{} cross-axis width", shape.name),
        )
        .measure_plans_reused;

        // A change on the CONTAINER itself, which invalidates every child.
        let mut container = shape.container.clone();
        container.gap = Some(LengthSpec::Px(11.0));
        let mut queue = MutationQueue::new();
        queue.set_style(
            id(2),
            NodeStyle {
                layout: Arc::new(container),
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        measure_plans_reused += scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            &format!("{} container gap", shape.name),
        )
        .measure_plans_reused;

        // Structural edits: append, then detach and park rows, then edit and
        // put a row back while detached and parked subtrees sit in the world.
        let fresh = id(3 + ROWS as u64 * 2 + 100);
        let mut queue = MutationQueue::new();
        queue.create(fresh, document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(2), fresh, None);
        queue.set_style(
            fresh,
            NodeStyle {
                layout: Arc::new((shape.row)(1)),
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        measure_plans_reused += scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            &format!("{} append row", shape.name),
        )
        .measure_plans_reused;

        let mut queue = MutationQueue::new();
        queue.detach(id(3 + 5 * 2));
        world.commit(queue).unwrap();
        measure_plans_reused += scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            &format!("{} detach row 5", shape.name),
        )
        .measure_plans_reused;

        // Issue #228: a parked or detached node anywhere in the world used to
        // turn every container's plans off. Real documents always have one.
        let mut queue = MutationQueue::new();
        queue.park_subtree(id(3 + 9 * 2));
        world.commit(queue).unwrap();
        scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            &format!("{} park row 9", shape.name),
        );
        let mut reused_with_detached = 0;
        for &row in &[ROWS - 1, ROWS / 2 + 1, 1] {
            for &height in &[27.0f32, 20.0] {
                let mut style = (shape.row)(row);
                style.height = Some(LengthSpec::Px(height));
                let mut queue = MutationQueue::new();
                queue.set_style(
                    id(3 + row as u64 * 2),
                    NodeStyle {
                        layout: Arc::new(style),
                        ..NodeStyle::default()
                    },
                );
                world.commit(queue).unwrap();
                let step = scoped_step_matches_full(
                    &mut world,
                    document,
                    viewport,
                    &mut retained,
                    &format!(
                        "{} row {row} height {height} beside detached rows",
                        shape.name
                    ),
                );
                reused_with_detached += step.plans_reused + step.measure_plans_reused;
            }
        }
        assert!(
            reused_with_detached > 0,
            "{}: no plan was reused while detached rows sat in the world",
            shape.name
        );
        for (label, parked) in [("detached row 5", 5u64), ("parked row 9", 9)] {
            let mut queue = MutationQueue::new();
            queue.insert(id(2), id(3 + parked * 2), Some(id(3 + (parked + 1) * 2)));
            world.commit(queue).unwrap();
            scoped_step_matches_full(
                &mut world,
                document,
                viewport,
                &mut retained,
                &format!("{} reinsert {label}", shape.name),
            );
            let mut style = (shape.row)(parked as usize);
            style.height = Some(LengthSpec::Px(31.0));
            let mut queue = MutationQueue::new();
            queue.set_style(
                id(3 + parked * 2),
                NodeStyle {
                    layout: Arc::new(style),
                    ..NodeStyle::default()
                },
            );
            world.commit(queue).unwrap();
            scoped_step_matches_full(
                &mut world,
                document,
                viewport,
                &mut retained,
                &format!("{} resize reinserted {label}", shape.name),
            );
        }

        assert!(
            measure_plans_reused > 0,
            "{}: the measure plan was never reused, so this shape did not \
             exercise it",
            shape.name
        );
    }
}

/// The case the style-pointer check cannot see: a child's intrinsic size
/// changing while its own style stays byte-identical.
///
/// A text edit two levels down resizes a content-sized row without touching
/// that row's style. If the container reuses its cached plan here, every
/// sibling below keeps a stale position -- and no assertion about the edited
/// subtree would notice.
#[test]
fn content_growth_under_a_child_moves_its_siblings() {
    content_growth_under_a_child_moves_its_siblings_with(Some(LengthSpec::Px(600.0)));
}

/// The same case one level harder: the container is CONTENT-SIZED, so the
/// growing row also changes the container's own measurement.
///
/// This is what `MeasurePlan` has to get wrong to be dangerous. The row's own
/// style is byte-identical across the edit, so a plan that compared only styles
/// would hand back the container's previous height and every row below it would
/// keep a stale position -- silently, because nothing asserts about a node the
/// optimizer decided not to touch.
#[test]
fn content_growth_under_a_child_moves_its_siblings_when_the_container_hugs() {
    content_growth_under_a_child_moves_its_siblings_with(None);
}

fn content_growth_under_a_child_moves_its_siblings_with(container_height: Option<LengthSpec>) {
    let viewport = LayoutViewport::new(320.0, 600.0);
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(300.0)),
                // Fixed, so the PLACEMENT plan's own inputs stay identical when
                // a row grows and its per-child checks actually run. `None`
                // instead puts the container on the measure path as well, where
                // the growing row moves the container's own size.
                height: container_height,
                direction: Some(FlexDirection::Column),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    const ROWS: u64 = 12;
    for row in 0..ROWS {
        let row_id = id(3 + row * 2);
        let label_id = id(4 + row * 2);
        queue.create(row_id, document, NodeKind::Element { tag: "div".into() });
        queue.create(label_id, document, NodeKind::Text);
        queue.insert(id(2), row_id, None);
        queue.insert(row_id, label_id, None);
        queue.set_text(label_id, TextContent { value: "x".into() });
        // Content-sized: no width or height, so the label drives the row.
        queue.set_style(row_id, NodeStyle::default());
        queue.set_style(label_id, NodeStyle::default());
    }
    world.commit(queue).unwrap();

    // Give the labels real metrics so text actually drives the row height.
    let shape = |world: &mut UiWorld| {
        struct Shaper;
        impl TextShaper for Shaper {
            fn shape(
                &mut self,
                _id: StableNodeId,
                text: &TextContent,
                _style: &ComputedStyle,
                _constraints: crate::TextShapeConstraints,
            ) -> TextMetrics {
                TextMetrics {
                    width: text.value.len() as f32 * 8.0,
                    height: 16.0 * (1 + text.value.matches('\n').count()) as f32,
                    ascent: None,
                }
            }
        }
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world.shape_text(&work.text, &mut Shaper).unwrap();
    };
    shape(&mut world);

    let mut retained = RetainedLayoutCache::default();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    shape(&mut world);
    let _ = world.take_system_work();

    // Prime: a full pass records no measure plans, so without an edit first
    // the pass below would be building the plan rather than being caught out by
    // it. Grow a DIFFERENT row's text so the plan exists and is current.
    let mut queue = MutationQueue::new();
    queue.set_text(id(4), TextContent { value: "zz".into() });
    world.commit(queue).unwrap();
    shape(&mut world);
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "priming pass",
    );
    let _ = world.take_system_work();

    // Grow the SECOND row's text. Its own style never changes.
    let label = id(4 + 2);
    let before = world.node_style(id(3 + 2)).unwrap().layout.clone();
    let mut queue = MutationQueue::new();
    queue.set_text(
        label,
        TextContent {
            value: "yyyy\nyyyy\nyyyy".into(),
        },
    );
    world.commit(queue).unwrap();
    shape(&mut world);
    assert!(
        Arc::ptr_eq(&before, &world.node_style(id(3 + 2)).unwrap().layout),
        "the edited row's own style must be untouched, or this test is not \
         exercising the case the style check cannot see"
    );

    super::plan_stats::reset();
    let step = scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "text growth under a content-sized row",
    );
    if container_height.is_some() {
        assert!(
            super::plan_stats::plans_reused() > 0,
            "the container plan must be reached here, or the intrinsic check \
             this test exists to guard is never consulted"
        );
    } else {
        // The container's own height moved, so the measure plan must have been
        // REJECTED -- and rejecting it is the whole point. What has to be true
        // is that it was consulted and said no, which shows up as the container
        // re-measuring its children.
        assert!(
            step.children_measured >= ROWS as usize,
            "a hugging container whose child grew must re-measure its children; \
             it measured {}",
            step.children_measured
        );
    }
}

/// The harness above proves correctness; this one proves the scoped pass is
/// actually incremental, so a "fix" that just relayouts everything cannot pass
/// both.
///
/// The property: an edit CONTAINED inside a fixed-size row cannot move any
/// other row, so the list container must not touch its other children at all.
/// Layout invalidation still propagates to the container (and to the document
/// root), so this is precisely the case where the dirty set says "the whole
/// spine changed" and the actual work owed is constant.
///
/// Both edit kinds are covered: one CONTAINED inside a fixed-size row, and one
/// that RESIZES the last row. The second shifts nothing either -- there is no
/// row after it -- so it must also stay flat, which is what the container's
/// suffix replay buys.
#[test]
fn scoped_layout_contained_edit_does_not_scan_siblings_as_the_document_grows() {
    // A fixed-height container takes the definite-size short circuit in
    // `intrinsic_size_scoped` and never measures a child at all, so it cannot
    // tell whether the MEASURE side is incremental. A hugging container has no
    // short circuit: it reaches `MeasurePlan` for every frame, and a regression
    // there shows up as a sibling scan that grows with the document.
    contained_edit_stays_flat(Some(LengthSpec::Px(40000.0)), false);
    contained_edit_stays_flat(None, false);
}

/// Issue #228: the same gate with a parked row in the world. Any parked or
/// detached node used to switch every container's plans off, and a real
/// document always has one.
#[test]
fn scoped_layout_contained_edit_stays_flat_beside_a_parked_row() {
    contained_edit_stays_flat(Some(LengthSpec::Px(40000.0)), true);
    contained_edit_stays_flat(None, true);
}

fn contained_edit_stays_flat(container_height: Option<LengthSpec>, parked_row: bool) {
    let hugging = container_height.is_none();
    let viewport = LayoutViewport::new(320.0, 4000.0);
    let shape = DiffShape {
        name: "column-plain",
        container: LayoutStyle {
            width: Some(LengthSpec::Px(300.0)),
            height: container_height,
            direction: Some(FlexDirection::Column),
            ..LayoutStyle::default()
        },
        row: |_| LayoutStyle {
            width: Some(LengthSpec::Px(60.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
    };
    let mut emitted_by_rows = Vec::new();
    for rows in [64usize, 512] {
        let (mut world, document) = diff_tree(&shape, rows);
        if parked_row {
            let mut queue = MutationQueue::new();
            queue.park_subtree(id(3 + 2));
            world.commit(queue).unwrap();
        }
        let mut retained = RetainedLayoutCache::default();
        let _ = world.take_system_work();
        let emitted = RuntimeLayoutEngine
            .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
            .unwrap();
        write_changed_boxes(&mut world, &emitted);
        let _ = world.take_system_work();

        // A full pass deliberately records no measure plans -- see the comment
        // at the recording site -- so the FIRST scoped pass after one measures
        // its children and builds them. That priming frame is O(document) by
        // construction; what this gate is about is the steady state after it.
        let mut queue = MutationQueue::new();
        queue.set_style(
            id(4),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(17.0)),
                    height: Some(LengthSpec::Px(5.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            "priming pass",
        );

        let row = rows - 1;
        // 1. Contained: edit the LABEL inside the last row. The row's own size
        //    is fixed, so nothing above or below it can move.
        let mut queue = MutationQueue::new();
        queue.set_style(
            id(4 + row as u64 * 2),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(31.0)),
                    height: Some(LengthSpec::Px(9.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        let contained = scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            "contained edit",
        );

        // 2. Resize the LAST row. Its size changes, so the container's cached
        //    placement is stale from that index on -- but there is no index
        //    after it, so the suffix is empty and the cost is still constant.
        let mut style = (shape.row)(row);
        style.height = Some(LengthSpec::Px(27.0));
        let mut queue = MutationQueue::new();
        queue.set_style(
            id(3 + row as u64 * 2),
            NodeStyle {
                layout: Arc::new(style),
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        let resized =
            scoped_step_matches_full(&mut world, document, viewport, &mut retained, "tail resize");
        // Resizing the last row changes a HUGGING container's own height, so
        // its measure plan is correctly rejected and it re-measures every
        // child. That is a real remaining gap -- the measure-side analogue of
        // `replay_sequential_suffix`, which does not exist -- not something
        // this gate should assert away, so it only holds the fixed-height
        // container to the flat tail resize.
        let measured = if hugging {
            contained.children_measured
        } else {
            contained.children_measured.max(resized.children_measured)
        };
        assert!(
            !hugging || resized.children_measured >= rows,
            "a hugging container that really did resize must re-measure its \
             children; if this stops being true the assertion above is \
             measuring nothing"
        );
        emitted_by_rows.push((rows, contained.emitted.max(resized.emitted), measured));
    }
    let (small_rows, small, small_measured) = emitted_by_rows[0];
    let (big_rows, big, big_measured) = emitted_by_rows[1];
    assert!(
        big <= small + 4,
        "a contained edit must emit a bounded set: {small_rows} rows emitted {small}, \
         {big_rows} rows emitted {big}"
    );
    // The point of the whole exercise: an 8x larger document must not make the
    // container re-measure 8x as many children.
    assert!(
        big_measured <= small_measured + 4,
        "a contained edit must not scale its sibling scan with the document: \
         {small_rows} rows measured {small_measured} children, \
         {big_rows} rows measured {big_measured}"
    );
}

/// Run the scoped passes that leave every plan recorded and current.
///
/// A full pass records no measure plans, so the first scoped pass after one is
/// the pass that BUILDS them -- it consults nothing and can be fooled by
/// nothing. Any test about what the plans do or refuse to do has to get past
/// that frame first, or it asserts about a code path it never reached.
///
/// The edit is applied and then reverted, so the tree ends exactly where it
/// started with the plans describing that state. Two passes are needed, not
/// one: the second pass rejects the plan the first recorded (that is what the
/// revert is) and records the one the test will meet.
///
/// Nothing here asserts that a plan came out -- an uncacheable container
/// correctly produces none. What proves the priming works is that the
/// deliberately broken implementations these tests exist to catch DO fail
/// them.
fn prime_measure_plans(
    world: &mut UiWorld,
    document: DocumentId,
    viewport: LayoutViewport,
    retained: &mut RetainedLayoutCache,
    victim: StableNodeId,
) {
    let original = world.node_style(victim).unwrap().layout.clone();
    let mut nudged = (*original).clone();
    nudged.margin_top = Some(LengthSpec::Px(
        match nudged.margin_top {
            Some(LengthSpec::Px(px)) => px,
            _ => 0.0,
        } + 3.0,
    ));
    for layout in [Arc::new(nudged), original] {
        let mut queue = MutationQueue::new();
        queue.set_style(
            victim,
            NodeStyle {
                layout,
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        scoped_step_matches_full(world, document, viewport, retained, "priming pass");
    }
}

/// Build a hugging column with `children` laid out under it, run one full pass
/// and return everything a scoped step needs.
fn hugging_container_world(
    rows: &[(u64, LayoutStyle, Vec<(u64, LayoutStyle)>)],
    container: LayoutStyle,
) -> (UiWorld, DocumentId) {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(container),
            ..NodeStyle::default()
        },
    );
    for (row, style, children) in rows {
        queue.create(id(*row), document, NodeKind::Element { tag: "div".into() });
        queue.insert(id(2), id(*row), None);
        queue.set_style(
            id(*row),
            NodeStyle {
                layout: Arc::new(style.clone()),
                ..NodeStyle::default()
            },
        );
        for (child, child_style) in children {
            queue.create(
                id(*child),
                document,
                NodeKind::Element { tag: "div".into() },
            );
            queue.insert(id(*row), id(*child), None);
            queue.set_style(
                id(*child),
                NodeStyle {
                    layout: Arc::new(child_style.clone()),
                    ..NodeStyle::default()
                },
            );
        }
    }
    world.commit(queue).unwrap();
    (world, document)
}

/// A row item measured again at the width its line gave it is re-checked at
/// that width by the cached plans: here an edit inside the item changes its
/// height only at its used width (its two boxes stop wrapping there), not
/// at the width it was first measured with.
#[test]
fn a_cached_row_rechecks_an_item_at_its_used_width() {
    let viewport = LayoutViewport::new(1280.0, 800.0);
    let row = LayoutStyle {
        direction: Some(FlexDirection::Row),
        width: Some(LengthSpec::Px(1152.0)),
        gap: Some(LengthSpec::Px(16.0)),
        align_items: AlignSpec::Start,
        ..LayoutStyle::default()
    };
    let primary = LayoutStyle {
        direction: Some(FlexDirection::Row),
        flex_wrap: FlexWrap::Wrap,
        width: Some(LengthSpec::Fill),
        min_width: Some(LengthSpec::Px(0.0)),
        flex_grow: Some(1.0),
        flex_shrink: Some(1.0),
        align_items: AlignSpec::Start,
        ..LayoutStyle::default()
    };
    let block = |width| LayoutStyle {
        width: Some(LengthSpec::Px(width)),
        height: Some(LengthSpec::Px(50.0)),
        ..LayoutStyle::default()
    };
    let side = LayoutStyle {
        width: Some(LengthSpec::Px(402.0)),
        height: Some(LengthSpec::Px(20.0)),
        flex_shrink: Some(0.0),
        ..LayoutStyle::default()
    };
    let (mut world, document) = hugging_container_world(
        &[
            (3, primary, vec![(10, block(400.0)), (11, block(400.0))]),
            (4, side, vec![]),
        ],
        row,
    );
    let mut retained = RetainedLayoutCache::default();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    let _ = world.take_system_work();
    prime_measure_plans(&mut world, document, viewport, &mut retained, id(4));
    assert_eq!(retained.documents[&document].boxes[&id(3)].height, 100.0);

    let mut queue = MutationQueue::new();
    queue.set_style(
        id(11),
        NodeStyle {
            layout: Arc::new(block(300.0)),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "an item's content stops wrapping at its used width",
    );
    assert_eq!(retained.documents[&document].boxes[&id(3)].height, 50.0);
}

/// `MeasurePlan` records the flow direction its container handed each child as
/// that child's `parent_direction`, and a child's own measurement can depend on
/// it -- `aspect-ratio` stretch-fits the inline axis only when the parent is not
/// a row.
///
/// Flipping the container from column to row leaves every child's own style,
/// child list and available size untouched, so a plan that did not record the
/// direction would hand back the column measurement forever.
#[test]
fn flipping_a_container_to_a_row_remeasures_children_that_depend_on_the_direction() {
    let viewport = LayoutViewport::new(320.0, 600.0);
    let hug = |direction| LayoutStyle {
        width: Some(LengthSpec::Px(300.0)),
        height: None,
        direction: Some(direction),
        ..LayoutStyle::default()
    };
    // `aspect_ratio` with no width of its own: the used width comes from the
    // stretch fit, which is disabled under a row parent.
    let ratio_row = LayoutStyle {
        aspect_ratio: Some(2.0),
        ..LayoutStyle::default()
    };
    let (mut world, document) = hugging_container_world(
        &[
            (3, ratio_row.clone(), vec![]),
            (4, ratio_row.clone(), vec![]),
            (5, ratio_row, vec![]),
        ],
        hug(FlexDirection::Column),
    );
    let mut retained = RetainedLayoutCache::default();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    let _ = world.take_system_work();
    prime_measure_plans(&mut world, document, viewport, &mut retained, id(5));
    let column_box = retained.documents[&document].boxes[&id(3)];

    let mut queue = MutationQueue::new();
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(hug(FlexDirection::Row)),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "container direction flipped to row",
    );
    assert_ne!(
        retained.documents[&document].boxes[&id(3)],
        column_box,
        "the child's measurement must actually depend on the parent direction, \
         or this test cannot detect a plan that ignores it"
    );
}

/// `display:contents` splices a child's own children into its parent's flow
/// list, so the parent's measurement is a function of a grandchild list that
/// neither plan records: the plans index their entries by DIRECT child id.
///
/// The edit that proves the refusal to cache such a container is load-bearing
/// is a change to a GRANDCHILD. It resizes what the container actually
/// measures while every direct child id, and every direct child's style, stays
/// byte-identical -- so the per-child checks have nothing to catch it with, and
/// only declining the plan outright is correct. (Flipping the spliced child
/// itself would be caught by the ordinary style compare, and proves nothing
/// about this guard.)
#[test]
fn a_display_contents_child_keeps_its_container_off_the_cached_plans() {
    let viewport = LayoutViewport::new(320.0, 600.0);
    let hug = LayoutStyle {
        width: Some(LengthSpec::Px(300.0)),
        height: None,
        direction: Some(FlexDirection::Column),
        ..LayoutStyle::default()
    };
    let leaf = |height: f32| LayoutStyle {
        width: Some(LengthSpec::Px(60.0)),
        height: Some(LengthSpec::Px(height)),
        ..LayoutStyle::default()
    };
    // Padding so unboxing actually changes the container's content height: as
    // `contents` the grandchildren stack bare, as a block the padding counts.
    let wrapper = |display: Option<DisplaySpec>| LayoutStyle {
        display,
        padding: Some(LengthSpec::Px(12.0)),
        box_sizing: BoxSizing::ContentBox,
        ..LayoutStyle::default()
    };
    let (mut world, document) = hugging_container_world(
        &[
            (3, leaf(20.0), vec![]),
            (
                4,
                wrapper(Some(DisplaySpec::Contents)),
                vec![(6, leaf(20.0)), (7, leaf(20.0))],
            ),
            (5, leaf(20.0), vec![]),
        ],
        hug,
    );
    let mut retained = RetainedLayoutCache::default();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    let _ = world.take_system_work();
    prime_measure_plans(&mut world, document, viewport, &mut retained, id(3));
    let spliced = retained.documents[&document].boxes[&id(2)];

    // A grandchild grows. Every DIRECT child of the container keeps its style
    // and its id, so the per-child checks see nothing at all.
    let mut queue = MutationQueue::new();
    queue.set_style(
        id(6),
        NodeStyle {
            layout: Arc::new(leaf(53.0)),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let before = world.node_style(id(4)).unwrap().layout.clone();
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "grandchild spliced in by display:contents grew",
    );
    assert!(
        Arc::ptr_eq(&before, &world.node_style(id(4)).unwrap().layout),
        "the spliced child's own style must be untouched, or this test is not \
         exercising the case the direct-child entries cannot see"
    );
    assert_ne!(
        retained.documents[&document].boxes[&id(2)].height,
        spliced.height,
        "the grandchild must actually move the container's height, or this test \
         cannot detect a plan that cached across it"
    );

    // And the ordinary case on the same tree: the spliced child becomes a real
    // box, which changes what the container measures through the flow list.
    let spliced = retained.documents[&document].boxes[&id(2)];
    let mut queue = MutationQueue::new();
    queue.set_style(
        id(4),
        NodeStyle {
            layout: Arc::new(wrapper(None)),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "display:contents child became a box",
    );
    assert_ne!(
        retained.documents[&document].boxes[&id(2)].height,
        spliced.height,
        "unboxing must change the container's own height, or this test cannot \
         detect a plan that cached across it"
    );
}

/// A container's own shaped text competes with its children for the content
/// size, and `set_text` moves that text without touching the container's style,
/// its child list, or any child.
///
/// So the container's own text metrics are a plan input in their own right. A
/// plan that recorded only style and children would hand back the height the
/// old string produced, and every child under it would keep a stale position.
#[test]
fn a_container_whose_own_text_grows_remeasures_itself() {
    struct Shaper;
    impl TextShaper for Shaper {
        fn shape(
            &mut self,
            _id: StableNodeId,
            text: &TextContent,
            _style: &ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            TextMetrics {
                width: text.value.len() as f32 * 8.0,
                height: 40.0 * (1 + text.value.matches('\n').count()) as f32,
                ascent: None,
            }
        }
    }
    let shape = |world: &mut UiWorld| {
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world.shape_text(&work.text, &mut Shaper).unwrap();
    };

    let viewport = LayoutViewport::new(320.0, 600.0);
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    // The container hugs, carries text of its own, AND has children. The text
    // is taller than the stacked children, so it is the text that decides the
    // container's height.
    queue.create(id(2), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(300.0)),
                height: None,
                direction: Some(FlexDirection::Column),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text(
        id(2),
        TextContent {
            value: "one".into(),
        },
    );
    for row in 0..4u64 {
        queue.create(
            id(3 + row),
            document,
            NodeKind::Element { tag: "div".into() },
        );
        queue.insert(id(2), id(3 + row), None);
        queue.set_style(
            id(3 + row),
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    width: Some(LengthSpec::Px(60.0)),
                    height: Some(LengthSpec::Px(10.0)),
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
    }
    world.commit(queue).unwrap();
    shape(&mut world);

    let mut retained = RetainedLayoutCache::default();
    let emitted = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &emitted);
    shape(&mut world);
    let _ = world.take_system_work();
    prime_measure_plans(&mut world, document, viewport, &mut retained, id(3));
    let short = retained.documents[&document].boxes[&id(2)];

    let before = world.node_style(id(2)).unwrap().layout.clone();
    let mut queue = MutationQueue::new();
    queue.set_text(
        id(2),
        TextContent {
            value: "one\ntwo\nthree".into(),
        },
    );
    world.commit(queue).unwrap();
    shape(&mut world);
    assert!(
        Arc::ptr_eq(&before, &world.node_style(id(2)).unwrap().layout),
        "the container's own style must be untouched, or this test is not \
         exercising the text-metrics input"
    );
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "the container's own text grew",
    );
    assert_ne!(
        retained.documents[&document].boxes[&id(2)].height,
        short.height,
        "the container's own text must decide its height, or this test cannot \
         detect a plan that ignores it"
    );
}

/// Two spellings of the same layout must not retire a plan; two different
/// layouts must.
///
/// `direction: None` and `direction: Some(Column)` are the same axis --
/// `used_flow_direction` is `unwrap_or(Column)` -- and in a real host they
/// alternate on the same node from one frame to the next, because more than one
/// writer seeds the field. A plan compared with `==` is retired every frame by
/// that alternation while nothing about the layout moved. `Some(Row)` is a
/// different layout and must still retire it.
#[test]
fn a_default_spelled_two_ways_keeps_the_plan_but_a_real_direction_change_retires_it() {
    let viewport = LayoutViewport::new(320.0, 600.0);
    let leaf = |direction| LayoutStyle {
        width: Some(LengthSpec::Px(60.0)),
        height: Some(LengthSpec::Px(20.0)),
        direction,
        ..LayoutStyle::default()
    };
    // Hugging container, so it is the measure plan that has to hold.
    let hug = LayoutStyle {
        width: Some(LengthSpec::Px(300.0)),
        height: None,
        direction: Some(FlexDirection::Column),
        ..LayoutStyle::default()
    };
    let rows: Vec<(u64, LayoutStyle, Vec<(u64, LayoutStyle)>)> =
        (3..=10).map(|row| (row, leaf(None), vec![])).collect();

    const ROWS: usize = 8;
    for (spelling, expect_remeasure) in [
        // Same axis, different spelling: the plan must survive, so the
        // container must not touch its children.
        (Some(FlexDirection::Column), false),
        // A real change: the plan must be retired and the children re-measured.
        (Some(FlexDirection::Row), true),
    ] {
        let (mut world, document) = hugging_container_world(&rows, hug.clone());
        let mut retained = RetainedLayoutCache::default();
        let emitted = RuntimeLayoutEngine
            .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
            .unwrap();
        write_changed_boxes(&mut world, &emitted);
        let _ = world.take_system_work();
        prime_measure_plans(&mut world, document, viewport, &mut retained, id(10));

        let mut queue = MutationQueue::new();
        queue.set_style(
            id(4),
            NodeStyle {
                layout: Arc::new(leaf(spelling)),
                ..NodeStyle::default()
            },
        );
        world.commit(queue).unwrap();
        let step = scoped_step_matches_full(
            &mut world,
            document,
            viewport,
            &mut retained,
            &format!("child direction respelled as {spelling:?}"),
        );
        // The global "plans reused" counter is too coarse -- other nodes reuse
        // their own plans either way. What this test is about is whether THIS
        // container had to walk its children.
        assert_eq!(
            step.children_measured >= ROWS,
            expect_remeasure,
            "direction {spelling:?}: children measured {} (expected a rescan: \
             {expect_remeasure})",
            step.children_measured
        );
    }
}

#[test]
fn scoped_layout_reveals_hidden_branch_with_new_positioned_children() {
    let document = DocumentId::new(1).unwrap();
    let viewport = LayoutViewport::new(320.0, 620.0);
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for node in 1..=3 {
        queue.create(id(node), document, NodeKind::Element { tag: "div".into() });
    }
    queue.insert(id(1), id(2), None);
    queue.insert(id(1), id(3), None);
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(320.0)),
                height: Some(LengthSpec::Px(620.0)),
                direction: Some(FlexDirection::Column),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                height: Some(LengthSpec::Px(30.0)),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    let branch = LayoutStyle {
        width: Some(LengthSpec::Fill),
        height: Some(LengthSpec::Px(300.0)),
        flex_shrink: Some(0.0),
        ..Default::default()
    };
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(branch.clone()),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    let mut retained = RetainedLayoutCache::default();
    let full = RuntimeLayoutEngine
        .layout_document_scoped(&world, document, viewport, &[], &mut retained, true)
        .unwrap();
    write_changed_boxes(&mut world, &full);
    let _ = world.take_system_work();
    let mut hidden = branch.clone();
    hidden.hidden = true;
    let mut queue = MutationQueue::new();
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(hidden),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    let hidden_boxes = RuntimeLayoutEngine
        .layout_document_scoped(
            &world,
            document,
            viewport,
            &work.layout,
            &mut retained,
            false,
        )
        .unwrap();
    write_changed_boxes(&mut world, &hidden_boxes);
    let _ = world.take_system_work();
    // An omitted box has no meaningful origin; only its zero extent matters.
    let hidden_box = world.layout_box(id(3)).unwrap();
    assert_eq!((hidden_box.width, hidden_box.height), (0.0, 0.0));
    let mut queue = MutationQueue::new();
    queue.create(id(4), document, NodeKind::Element { tag: "div".into() });
    queue.insert(id(3), id(4), None);
    queue.set_style(
        id(4),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                position: PositionSpec::Absolute,
                offset_top: Some(LengthSpec::Px(104.0)),
                width: Some(LengthSpec::Percent(100.0)),
                height: Some(LengthSpec::Px(104.0)),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                height: Some(LengthSpec::Px(208.0)),
                ..branch
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    scoped_step_matches_full(
        &mut world,
        document,
        viewport,
        &mut retained,
        "reveal with changed content",
    );
    assert_eq!(world.layout_box(id(4)).unwrap().height, 104.0);
    assert_eq!(world.layout_box(id(4)).unwrap().y, 134.0);
    assert_eq!(world.layout_box(id(3)).unwrap().height, 208.0);
}

#[cfg(feature = "rich-text")]
#[test]
fn constrained_auto_height_contains_wrapped_markdown_and_flow_actions() {
    use crate::{AppContext, NativeMarkdown, Stack};
    for (maximum, sizing, declared, minimum) in [
        (
            LengthSpec::Percent(76.0),
            BoxSizing::BorderBox,
            LengthSpec::FitContent,
            None,
        ),
        (
            LengthSpec::Min2(
                nana_ui_core::LengthAtom::Percent(76.0),
                nana_ui_core::LengthAtom::Px(620.0),
            ),
            BoxSizing::BorderBox,
            LengthSpec::FitContent,
            None,
        ),
        (
            LengthSpec::Px(620.0),
            BoxSizing::ContentBox,
            LengthSpec::Px(50.0),
            Some(LengthSpec::Px(100.0)),
        ),
        (
            LengthSpec::Px(620.0),
            BoxSizing::BorderBox,
            LengthSpec::Px(50.0),
            Some(LengthSpec::Px(100.0)),
        ),
    ] {
        let document = DocumentId::new(601).unwrap();
        let mut context = AppContext::new();
        let root = context
            .create_component(document, Stack::fill_column(0.0))
            .unwrap();
        let bubble = context
            .create_detached_component(
                document,
                Stack::column(6.0)
                    .width(declared)
                    .padding_xy(14.0, 10.0)
                    .with_layout(|layout| {
                        layout.max_width = Some(maximum);
                        layout.min_width = minimum;
                        layout.box_sizing = sizing;
                    }),
            )
            .unwrap();
        let markdown = context
            .create_detached_component(
                document,
                NativeMarkdown::parse(&"换行内容需要完整参与父容器的自然高度。".repeat(32)),
            )
            .unwrap();
        context.assemble_markdown(markdown).unwrap();
        let actions = context
            .create_detached_component(document, Stack::row(0.0).height(LengthSpec::Px(28.0)))
            .unwrap();
        let following = context
            .create_detached_component(document, Stack::row(0.0).height(LengthSpec::Px(22.0)))
            .unwrap();
        context.append_child(root, bubble).unwrap();
        context.append_child(bubble, markdown).unwrap();
        context.append_child(bubble, actions).unwrap();
        context.append_child(root, following).unwrap();
        for width in [1000.0, 640.0, 400.0, 1000.0] {
            context
                .layout_document(document, LayoutViewport::new(width, 1200.0))
                .unwrap();
            let world = context.world();
            let parent = world.layout_box(bubble.stable_id()).unwrap();
            if minimum.is_some() {
                let expected = if sizing == BoxSizing::ContentBox {
                    128.0
                } else {
                    100.0
                };
                assert_eq!(parent.width, expected);
            }
            let text = world.layout_box(markdown.stable_id()).unwrap();
            let action = world.layout_box(actions.stable_id()).unwrap();
            let next = world.layout_box(following.stable_id()).unwrap();
            assert!(
                action.y >= text.y + text.height,
                "{parent:?} {text:?} {action:?}"
            );
            assert!(
                action.y + action.height + 10.0 <= parent.y + parent.height + 0.1,
                "flow content and padding must fit at {width}: {parent:?} {text:?} {action:?}"
            );
            assert!(
                next.y >= parent.y + parent.height,
                "following row must start after the complete bubble"
            );
        }
    }
}

/// `writing-mode` and `direction` inherit (CSS Writing Modes §3): a container
/// that declares neither lays out in its parent's. A row inside a
/// `vertical-rl` box runs down the page.
#[test]
fn a_container_inherits_its_parents_writing_mode() {
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(200.0)),
            writing_mode: Some(WritingModeSpec::VerticalRl),
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "inner".into(),
            style: LayoutStyle {
                display: Some(DisplaySpec::Flex),
                direction: Some(FlexDirection::Row),
                width: Some(LengthSpec::Px(100.0)),
                height: Some(LengthSpec::Px(100.0)),
                align_items: AlignSpec::Start,
                ..LayoutStyle::default()
            },
            children: vec![px_box("a", 20.0, 20.0), px_box("b", 20.0, 20.0)],
            text: None,
        }],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 200.0);
    let inner = boxes["inner"];
    // Down the inner box's inline axis, against its right (block-start) edge.
    for (id, y) in [("a", 0.0), ("b", 20.0)] {
        assert!(
            (boxes[id].y - (inner.y + y)).abs() < 0.5
                && (boxes[id].x - (inner.x + inner.width - 20.0)).abs() < 0.5,
            "{id} at y={y} against the right edge of {inner:?}, got {:?}",
            boxes[id]
        );
    }
}

/// `text-orientation: upright` makes a vertical box's used direction `ltr`
/// (CSS Writing Modes §5.1): under a `vertical-rl; direction: rtl` parent that
/// sets it, a row starts at the top again instead of the bottom -- inherited,
/// like the writing mode itself.
#[test]
fn an_upright_vertical_box_lays_out_left_to_right() {
    let layout = |orientation| {
        let tree = StyleLayoutNode {
            id: "root".into(),
            style: LayoutStyle {
                display: Some(DisplaySpec::Block),
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(200.0)),
                writing_mode: Some(WritingModeSpec::VerticalRl),
                dir: Some(DirSpec::Rtl),
                text_orientation: orientation,
                ..LayoutStyle::default()
            },
            children: vec![StyleLayoutNode {
                id: "row".into(),
                style: LayoutStyle {
                    display: Some(DisplaySpec::Flex),
                    direction: Some(FlexDirection::Row),
                    width: Some(LengthSpec::Px(40.0)),
                    height: Some(LengthSpec::Px(100.0)),
                    align_items: AlignSpec::Start,
                    ..LayoutStyle::default()
                },
                children: vec![px_box("a", 20.0, 20.0)],
                text: None,
            }],
            text: None,
        };
        let boxes = box_map(&tree, 200.0, 200.0);
        boxes["a"].y - boxes["row"].y
    };
    assert!(
        (layout(None) - 80.0).abs() < 0.5,
        "rtl starts at the bottom"
    );
    assert!(
        layout(Some(TextOrientationSpec::Upright)).abs() < 0.5,
        "upright reads ltr: the row starts at the top"
    );
}

/// A box's logical edges land in the writing context it inherits, not only in
/// the one it declares: `padding-inline-start` on a box inside an RTL parent
/// pads its right edge, even though the box was built (and its edges first
/// resolved) without a `direction` of its own.
#[test]
fn logical_edges_land_in_the_inherited_direction() {
    let mut inner = LayoutStyle {
        display: Some(DisplaySpec::Flex),
        direction: Some(FlexDirection::Row),
        width: Some(LengthSpec::Px(100.0)),
        height: Some(LengthSpec::Px(40.0)),
        align_items: AlignSpec::Start,
        ..LayoutStyle::default()
    };
    inner.logical_padding.set_start(Some(LengthSpec::Px(30.0)));
    inner.bake_logical_edges();
    assert_eq!(inner.padding_left, Some(LengthSpec::Px(30.0)));
    let tree = StyleLayoutNode {
        id: "root".into(),
        style: LayoutStyle {
            display: Some(DisplaySpec::Flex),
            width: Some(LengthSpec::Px(200.0)),
            height: Some(LengthSpec::Px(100.0)),
            dir: Some(DirSpec::Rtl),
            align_items: AlignSpec::Start,
            ..LayoutStyle::default()
        },
        children: vec![StyleLayoutNode {
            id: "inner".into(),
            style: inner,
            children: vec![px_box("a", 20.0, 20.0)],
            text: None,
        }],
        text: None,
    };
    let boxes = box_map(&tree, 200.0, 100.0);
    let inner = boxes["inner"];
    let a = boxes["a"];
    assert!(
        (a.x - (inner.x + inner.width - 30.0 - 20.0)).abs() < 0.5,
        "a starts 30px in from the right edge of {inner:?}, got {a:?}"
    );
}

/// Inheriting a writing mode and direction is the same as declaring them: a
/// container of every kind -- an IFC, a wrapping flex row, a grid -- lays out
/// identically whether it says `writing-mode` / `direction` itself or gets
/// them from its parent.
#[test]
fn an_inherited_writing_context_lays_out_like_a_declared_one() {
    let inline_block = |id: String| StyleLayoutNode {
        id,
        style: LayoutStyle {
            display: Some(DisplaySpec::InlineBlock),
            width: Some(LengthSpec::Px(30.0)),
            height: Some(LengthSpec::Px(20.0)),
            ..LayoutStyle::default()
        },
        children: Vec::new(),
        text: None,
    };
    let containers: [(&str, LayoutStyle); 3] = [
        (
            "ifc",
            LayoutStyle {
                display: Some(DisplaySpec::Block),
                ..LayoutStyle::default()
            },
        ),
        (
            "flex-wrap",
            LayoutStyle {
                display: Some(DisplaySpec::Flex),
                direction: Some(FlexDirection::Row),
                flex_wrap: FlexWrap::Wrap,
                align_items: AlignSpec::Start,
                ..LayoutStyle::default()
            },
        ),
        (
            "grid",
            LayoutStyle {
                display: Some(DisplaySpec::Grid),
                grid_columns: Some(vec![GridTrack::Px(40.0), GridTrack::Px(40.0)]),
                ..LayoutStyle::default()
            },
        ),
    ];
    for mode in [
        WritingModeSpec::HorizontalTb,
        WritingModeSpec::VerticalRl,
        WritingModeSpec::VerticalLr,
    ] {
        for dir in [DirSpec::Ltr, DirSpec::Rtl] {
            for (kind, container) in &containers {
                let layout = |declared: bool| {
                    let mut inner = container.clone();
                    inner.width = Some(LengthSpec::Px(100.0));
                    inner.height = Some(LengthSpec::Px(100.0));
                    if declared {
                        inner.writing_mode = Some(mode);
                        inner.dir = Some(dir);
                    }
                    let tree = StyleLayoutNode {
                        id: "root".into(),
                        style: LayoutStyle {
                            display: Some(DisplaySpec::Block),
                            width: Some(LengthSpec::Px(200.0)),
                            height: Some(LengthSpec::Px(200.0)),
                            writing_mode: Some(mode),
                            dir: Some(dir),
                            ..LayoutStyle::default()
                        },
                        children: vec![StyleLayoutNode {
                            id: "inner".into(),
                            style: inner,
                            children: (0..5).map(|i| inline_block(format!("c{i}"))).collect(),
                            text: None,
                        }],
                        text: None,
                    };
                    box_map(&tree, 200.0, 200.0)
                };
                let (declared, inherited) = (layout(true), layout(false));
                for (id, want) in &declared {
                    assert_eq!(
                        inherited.get(id),
                        Some(want),
                        "{kind} in {mode:?} {dir:?}: {id} inherited vs declared"
                    );
                }
            }
        }
    }
}

/// A line that a float shortens does not shrink its items' percentage
/// margins: they resolve against the container's inline size, like every
/// other percentage edge (CSS Box Model §5), not against what the line has
/// left.
#[test]
fn a_float_shortened_line_keeps_the_containers_percent_base() {
    let layout = |display: DisplaySpec, margin: LengthSpec| {
        let tree = StyleLayoutNode {
            id: "root".into(),
            style: LayoutStyle {
                display: Some(DisplaySpec::Block),
                width: Some(LengthSpec::Px(200.0)),
                height: Some(LengthSpec::Px(100.0)),
                ..LayoutStyle::default()
            },
            children: vec![
                StyleLayoutNode {
                    id: "float".into(),
                    style: LayoutStyle {
                        width: Some(LengthSpec::Px(50.0)),
                        height: Some(LengthSpec::Px(40.0)),
                        float: FloatSpec::Left,
                        ..LayoutStyle::default()
                    },
                    children: Vec::new(),
                    text: None,
                },
                StyleLayoutNode {
                    id: "a".into(),
                    style: LayoutStyle {
                        display: Some(display),
                        width: Some(LengthSpec::Px(30.0)),
                        height: Some(LengthSpec::Px(20.0)),
                        margin_left: Some(margin),
                        ..LayoutStyle::default()
                    },
                    children: Vec::new(),
                    text: None,
                },
            ],
            text: None,
        };
        box_map(&tree, 200.0, 100.0)["a"]
    };
    for display in [DisplaySpec::InlineBlock, DisplaySpec::Block] {
        let percent = layout(display, LengthSpec::Percent(10.0));
        let px = layout(display, LengthSpec::Px(20.0));
        assert_eq!(
            percent, px,
            "{display:?}: 10% of the 200px container is 20px"
        );
    }
}

/// Percentage margins resolve against the containing block's inline size in
/// the containing block's own writing mode (CSS Writing Modes §7.3). A box
/// that sets a writing mode orthogonal to its parent's still resolves against
/// the parent's inline axis.
#[test]
fn an_orthogonal_box_resolves_percentage_margins_on_its_parents_inline_axis() {
    let case = |parent_mode, child_mode, width: f32, height: f32| {
        let tree = StyleLayoutNode {
            id: "root".into(),
            style: LayoutStyle {
                display: Some(DisplaySpec::Flex),
                direction: Some(FlexDirection::Column),
                width: Some(LengthSpec::Px(width)),
                height: Some(LengthSpec::Px(height)),
                writing_mode: Some(parent_mode),
                align_items: AlignSpec::Start,
                ..LayoutStyle::default()
            },
            children: vec![StyleLayoutNode {
                id: "a".into(),
                style: LayoutStyle {
                    width: Some(LengthSpec::Px(20.0)),
                    height: Some(LengthSpec::Px(20.0)),
                    margin_top: Some(LengthSpec::Percent(10.0)),
                    writing_mode: Some(child_mode),
                    ..LayoutStyle::default()
                },
                children: Vec::new(),
                text: None,
            }],
            text: None,
        };
        box_map(&tree, width, height)["a"].y
    };
    // Horizontal parent 200 wide: 10% of its width, whatever the child sets.
    let vertical_child = case(
        WritingModeSpec::HorizontalTb,
        WritingModeSpec::VerticalRl,
        200.0,
        100.0,
    );
    assert!((vertical_child - 20.0).abs() < 0.5, "got {vertical_child}");
    // Vertical parent 200 tall: 10% of its height.
    let horizontal_child = case(
        WritingModeSpec::VerticalRl,
        WritingModeSpec::HorizontalTb,
        100.0,
        200.0,
    );
    assert!(
        (horizontal_child - 20.0).abs() < 0.5,
        "got {horizontal_child}"
    );
}
