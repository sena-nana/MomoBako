use super::*;

#[test]
fn shared_inherited_styles_keep_local_overrides_and_old_snapshots_immutable() {
    let mut world = UiWorld::new();
    let document = DocumentId::new(1).unwrap();
    let id = |value| StableNodeId::new(value).unwrap();
    let mut queue = MutationQueue::new();
    for value in 1..=1000 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        if value > 1 {
            queue.insert(id(1), id(value), None);
        }
    }
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                font_size: Some(18.0),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let snapshot = Arc::clone(&world.record(id(1)).resolved.0);
    for value in 2..=1000 {
        assert!(Arc::ptr_eq(&snapshot, &world.record(id(value)).resolved.0));
    }
    let mut queue = MutationQueue::new();
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                color: Some([1.0, 0.0, 0.0, 1.0]),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                font_size: Some(20.0),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(snapshot.font_size, 18.0);
    assert_ne!(snapshot.color, Some([1.0, 0.0, 0.0, 1.0]));
    assert_eq!(world.computed_style(id(2)).unwrap().font_size, 20.0);
    assert_eq!(
        world.computed_style(id(2)).unwrap().color,
        Some([1.0, 0.0, 0.0, 1.0])
    );
    assert!(!Arc::ptr_eq(
        &world.record(id(1)).resolved.0,
        &world.record(id(2)).resolved.0
    ));
    for value in 3..=1000 {
        assert!(Arc::ptr_eq(
            &world.record(id(1)).resolved.0,
            &world.record(id(value)).resolved.0
        ));
    }
}

#[cfg(feature = "benchmark")]
#[test]
fn paired_style_control_produces_identical_resolved_values() {
    let mut outputs = Vec::new();
    for shared in [false, true] {
        let mut world = UiWorld::new();
        let document = DocumentId::new(1).unwrap();
        let id = |value| StableNodeId::new(value).unwrap();
        let mut queue = MutationQueue::new();
        for value in 1..=1000 {
            queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
            if value > 1 {
                queue.insert(id(value / 2), id(value), None);
            }
            if value % 7 == 0 {
                queue.set_style(
                    id(value),
                    NodeStyle {
                        layout: Arc::new(LayoutStyle {
                            font_size: Some(18.0),
                            color: Some([0.2, 0.3, 0.4, 1.0]),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                );
            }
        }
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        if shared {
            world.resolve_styles(&work.style).unwrap();
        } else {
            world
                .benchmark_resolve_styles_unshared(&work.style)
                .unwrap();
        }
        outputs.push(
            (1..=1000)
                .map(|value| world.computed_style(id(value)).unwrap().clone())
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(outputs[0], outputs[1]);
}
