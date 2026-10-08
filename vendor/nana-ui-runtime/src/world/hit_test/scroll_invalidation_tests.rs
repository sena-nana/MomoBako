use super::*;
use nana_ui_core::OverflowSpec;

fn id(value: u64) -> StableNodeId {
    StableNodeId::new(value).unwrap()
}

fn setup() -> (UiWorld, DocumentId) {
    let document = DocumentId::new(1).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(id(1), document, NodeKind::Document);
    for value in [2, 3] {
        queue.create(
            id(value),
            document,
            NodeKind::Element { tag: "item".into() },
        );
    }
    queue.insert(id(1), id(2), None);
    queue.insert(id(2), id(3), None);
    queue.write_layout(
        id(2),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        },
    );
    queue.write_layout(
        id(3),
        LayoutBox {
            x: 0.0,
            y: 80.0,
            width: 100.0,
            height: 20.0,
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_y: OverflowSpec::Scroll,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    world.rebuild_hit_test(document);
    (world, document)
}

#[test]
fn simultaneous_scroll_and_child_resize_requires_geometry_rebuild() {
    let (mut world, document) = setup();
    let mut queue = MutationQueue::new();
    queue.set_scroll_offset(id(2), ScrollOffset { x: 0.0, y: 60.0 });
    queue.write_layout(
        id(3),
        LayoutBox {
            x: 40.0,
            y: 80.0,
            width: 60.0,
            height: 20.0,
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    let updates = world.take_scroll_hit_updates();
    assert!(!world.hit_test_work_is_scroll_only(&work.input_hit_test, &updates));
    assert!(world.rebuild_hit_test_scoped(document, &work.input_hit_test));
    assert_ne!(world.hit_test(document, 10.0, 25.0), Some(id(3)));
    assert_eq!(world.hit_test(document, 50.0, 25.0), Some(id(3)));

    let mut scroll = MutationQueue::new();
    scroll.set_scroll_offset(id(2), ScrollOffset { x: 0.0, y: 55.0 });
    world.commit(scroll).unwrap();
    let work = world.take_system_work();
    let updates = world.take_scroll_hit_updates();
    assert!(world.hit_test_work_is_scroll_only(&work.input_hit_test, &updates));
}

#[test]
fn same_scroller_geometry_change_is_not_erased_by_later_scroll_invalidation() {
    let (mut world, document) = setup();
    let mut queue = MutationQueue::new();
    queue.write_layout(
        id(2),
        LayoutBox {
            x: 10.0,
            y: 0.0,
            width: 90.0,
            height: 50.0,
        },
    );
    queue.set_scroll_offset(id(2), ScrollOffset { x: 0.0, y: 60.0 });
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    let updates = world.take_scroll_hit_updates();
    assert!(!world.hit_test_work_is_scroll_only(&work.input_hit_test, &updates));
    assert!(world.rebuild_hit_test_scoped(document, &work.input_hit_test));
    assert_ne!(world.hit_test(document, 5.0, 45.0), Some(id(2)));
    assert_eq!(world.hit_test(document, 15.0, 45.0), Some(id(2)));
}

#[test]
fn outer_scroll_moves_nested_viewport_clips_with_descendant_hit_targets() {
    let (mut world, document) = setup();
    let mut queue = MutationQueue::new();
    queue.set_style(
        id(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_y: OverflowSpec::Scroll,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    queue.create(
        id(4),
        document,
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.insert(id(3), id(4), None);
    queue.write_layout(
        id(4),
        LayoutBox {
            x: 0.0,
            y: 80.0,
            width: 80.0,
            height: 40.0,
        },
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    world.rebuild_hit_test(document);
    let mut scroll = MutationQueue::new();
    scroll.set_scroll_offset(id(2), ScrollOffset { x: 0.0, y: 60.0 });
    world.commit(scroll).unwrap();
    let work = world.take_system_work();
    let updates = world.take_scroll_hit_updates();
    assert!(world.hit_test_work_is_scroll_only(&work.input_hit_test, &updates));
    for (scroller, delta) in updates {
        world.update_hit_test_scroll(document, scroller, delta);
    }
    let patched = world.hit_test(document, 10.0, 25.0);
    assert_eq!(patched, Some(id(4)));
    assert_ne!(
        world.hit_test(document, 10.0, 45.0),
        Some(id(4)),
        "inner clip still excludes the area below its viewport"
    );
    world.rebuild_hit_test(document);
    assert_eq!(world.hit_test(document, 10.0, 25.0), patched);
}
