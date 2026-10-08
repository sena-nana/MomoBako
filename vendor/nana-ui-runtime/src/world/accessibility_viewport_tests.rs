use super::*;
use nana_ui_core::PaintTransform;

#[test]
fn hidden_accessibility_containers_keep_visible_descendants_connected() {
    for hidden_root in [false, true] {
        let mut world = fixture();
        let mut queue = MutationQueue::new();
        for id in 1..=3 {
            let mut layout = LayoutStyle::default();
            layout.paint.visibility = Some(if id == 2 || (id == 1 && hidden_root) {
                nana_ui_core::VisibilitySpec::Hidden
            } else {
                nana_ui_core::VisibilitySpec::Visible
            });
            queue.set_style(
                node(id),
                NodeStyle {
                    layout: Arc::new(layout),
                    ..Default::default()
                },
            );
        }
        queue.set_accessibility(
            node(2),
            AccessibilityState {
                role: AccessibilityRole::Dialog,
                label: Some("Hidden container label".into()),
                description: Some("Hidden description".into()),
                value: Some("Hidden value".into()),
                disabled: true,
                modal: true,
                busy: true,
                editable: true,
                ..Default::default()
            },
        );
        world.commit(queue).unwrap();
        world
            .resolve_styles(&world.document_order(document()))
            .unwrap();
        world.take_system_work();
        let snapshot = |world: &UiWorld| {
            world
                .project_accessibility(document())
                .into_iter()
                .map(|item| (item.id, item))
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        let mut retained = snapshot(&world);
        let mut cursor = Some(node(3));
        while let Some(id) = cursor {
            let entry = retained
                .get(&id)
                .expect("visible control must have a complete path to the accessibility root");
            if let Some(parent) = entry.parent {
                assert!(
                    retained
                        .get(&parent)
                        .expect("missing accessible parent")
                        .children
                        .contains(&id)
                );
            }
            cursor = entry.parent;
        }
        let container = &retained[&node(2)];
        assert_eq!(container.role, AccessibilityRole::Generic);
        assert!(
            container.label.is_none()
                && container.value.is_none()
                && container.description.is_none()
        );
        assert!(
            !container.disabled
                && !container.modal
                && !container.busy
                && !container.editable
                && !container.focused
        );
        assert_eq!(container.children, [node(3)]);
        assert_eq!(retained[&node(3)].role, AccessibilityRole::Button);

        for visibility in [
            nana_ui_core::VisibilitySpec::Visible,
            nana_ui_core::VisibilitySpec::Hidden,
        ] {
            let mut style = world.record(node(2)).style.clone();
            Arc::make_mut(&mut style.layout).paint.visibility = Some(visibility);
            let mut queue = MutationQueue::new();
            queue.set_style(node(2), style);
            world.commit(queue).unwrap();
            let work = world.take_system_work();
            world.resolve_styles(&work.style).unwrap();
            let delta = world.project_accessibility_delta(&work);
            for id in delta.removed {
                retained.remove(&id);
            }
            for entry in delta.updated {
                retained.insert(entry.id, entry);
            }
            assert_eq!(retained, snapshot(&world));
        }
        for visibility in [
            nana_ui_core::VisibilitySpec::Hidden,
            nana_ui_core::VisibilitySpec::Visible,
        ] {
            let mut style = world.record(node(3)).style.clone();
            Arc::make_mut(&mut style.layout).paint.visibility = Some(visibility);
            let mut queue = MutationQueue::new();
            queue.set_style(node(3), style);
            world.commit(queue).unwrap();
            let work = world.take_system_work();
            world.resolve_styles(&work.style).unwrap();
            let delta = world.project_accessibility_delta(&work);
            for id in delta.removed {
                retained.remove(&id);
            }
            for entry in delta.updated {
                retained.insert(entry.id, entry);
            }
            assert_eq!(retained, snapshot(&world));
        }
    }
}

fn node(id: u64) -> StableNodeId {
    StableNodeId::new(id).unwrap()
}
fn document() -> DocumentId {
    DocumentId::new(1).unwrap()
}

fn fixture() -> UiWorld {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(
            node(id),
            document(),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    for (id, bounds) in [
        (
            1,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 400.0,
                height: 300.0,
            },
        ),
        (
            2,
            LayoutBox {
                x: 10.0,
                y: 200.0,
                width: 300.0,
                height: 200.0,
            },
        ),
        (
            3,
            LayoutBox {
                x: 30.0,
                y: 260.0,
                width: 100.0,
                height: 40.0,
            },
        ),
    ] {
        queue.write_layout(node(id), bounds);
    }
    for id in [1, 2] {
        queue.set_interaction(
            node(id),
            InteractionState {
                pointer_events: false,
                focusable: false,
            },
        );
    }
    queue.set_accessibility(
        node(3),
        AccessibilityState {
            role: AccessibilityRole::Button,
            ..Default::default()
        },
    );
    queue.set_scroll_offset(node(1), ScrollOffset { x: 4.0, y: 160.0 });
    queue.set_scroll_offset(node(2), ScrollOffset { x: 6.0, y: 20.0 });
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document());
    world
}

fn button_bounds(world: &UiWorld) -> LayoutBox {
    world
        .project_accessibility(document())
        .into_iter()
        .find(|n| n.id == node(3))
        .unwrap()
        .bounds
}
fn assert_hit_at_accessible_center(world: &UiWorld) {
    let bounds = button_bounds(world);
    assert_eq!(
        world.hit_test(
            document(),
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0
        ),
        Some(node(3))
    );
}

#[test]
fn fixed_node_accessibility_bounds_ignore_ancestor_scroll() {
    let mut world = fixture();
    let mut queue = MutationQueue::new();
    queue.set_style(
        node(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                position: nana_ui_core::PositionSpec::Fixed,
                ..LayoutStyle::default()
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document());
    let bounds = button_bounds(&world);
    assert_eq!(
        bounds.y, 260.0,
        "Fixed boxes are viewport-relative and must not pick up ancestor scroll"
    );
    assert_hit_at_accessible_center(&world);
}

#[test]
fn accessible_bounds_include_nested_scroll_without_changing_layout() {
    let world = fixture();
    let bounds = button_bounds(&world);
    assert_eq!(
        bounds,
        LayoutBox {
            x: 20.0,
            y: 80.0,
            width: 100.0,
            height: 40.0
        }
    );
    assert_eq!(world.layout_box(node(3)).unwrap().y, 260.0);
    assert_hit_at_accessible_center(&world);
}

#[test]
fn accessible_bounds_follow_incremental_scroll_hit_updates() {
    let mut world = fixture();
    let before = button_bounds(&world);
    let mut queue = MutationQueue::new();
    queue.set_scroll_offset(node(1), ScrollOffset { x: 4.0, y: 180.0 });
    world.commit(queue).unwrap();
    world.update_hit_test_scroll(document(), node(1), [0.0, -20.0]);
    assert_eq!(button_bounds(&world).y, before.y - 20.0);
    assert_hit_at_accessible_center(&world);
}

#[test]
fn accessible_bounds_use_the_accumulated_affine_transform() {
    let mut world = fixture();
    let mut queue = MutationQueue::new();
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform: Some(PaintTransform {
                    a: 1.5,
                    d: 1.5,
                    e: 15.0,
                    f: 25.0,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    queue.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform: Some(PaintTransform {
                    e: 12.0,
                    f: -10.0,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document());
    let bounds = button_bounds(&world);
    assert_eq!(bounds.width, 150.0);
    assert_eq!(bounds.height, 60.0);
    assert_hit_at_accessible_center(&world);
}

#[test]
fn scrolling_publishes_moved_descendants_in_accessibility_delta() {
    let mut world = fixture();
    world.take_system_work();
    let mut queue = MutationQueue::new();
    queue.set_scroll_offset(node(1), ScrollOffset { x: 4.0, y: 180.0 });
    world.commit(queue).unwrap();
    world.update_hit_test_scroll(document(), node(1), [0.0, -20.0]);
    let work = world.take_system_work();
    let delta = world.project_accessibility_delta(&work);
    let button = delta
        .updated
        .iter()
        .find(|item| item.id == node(3))
        .expect("moving a scroll ancestor updates the cached descendant bounds");
    assert_eq!(button.bounds.y, 60.0);
    assert_hit_at_accessible_center(&world);
}

#[test]
fn non_hittable_text_without_a_hit_entry_keeps_scrolled_accessibility_bounds() {
    let mut world = fixture();
    let mut queue = MutationQueue::new();
    queue.set_accessibility(
        node(3),
        AccessibilityState {
            role: AccessibilityRole::Text,
            ..Default::default()
        },
    );
    queue.set_interaction(
        node(3),
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
    );
    world.commit(queue).unwrap();
    let indexed = button_bounds(&world);
    // Accessible text can be projected before a hit entry is built, and must
    // still carry all ancestor scroll offsets.
    world.hit_test_index.clear();
    let fallback = button_bounds(&world);
    assert_eq!(fallback.x, 20.0);
    assert_eq!(fallback.y, 80.0);
    assert_eq!(fallback, indexed);
}

#[test]
fn batched_accessibility_bounds_are_order_independent_and_refresh_after_scroll() {
    let mut world = fixture();
    for offset in [180.0, 120.0] {
        let mut queue = MutationQueue::new();
        queue.set_scroll_offset(node(1), ScrollOffset { x: 7.0, y: offset });
        world.commit(queue).unwrap();
        world.rebuild_hit_test(document());
        let indexed = world.project_accessibility_nodes(&[node(1), node(2), node(3)]);
        world.hit_test_index.clear();
        // A descendant queried first must populate its ancestor transforms
        // without applying an ancestor's own scroll to its own geometry.
        let mut reverse = world.project_accessibility_nodes(&[node(3), node(2), node(1)]);
        reverse.reverse();
        assert_eq!(reverse, indexed);
        assert_eq!(
            world.project_accessibility_nodes(&[node(1), node(2), node(3)]),
            indexed
        );
    }
}

#[test]
fn accessible_projection_uses_committed_transforms_before_hit_rebuild() {
    let mut world = fixture();
    let previous = button_bounds(&world);
    let mut queue = MutationQueue::new();
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform: Some(PaintTransform {
                    e: 100.0,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    let before_rebuild = button_bounds(&world);
    assert_eq!(before_rebuild.x, previous.x + 100.0);
    world.rebuild_hit_test(document());
    assert_eq!(before_rebuild, button_bounds(&world));
    assert_hit_at_accessible_center(&world);
}

/// The accessibility delta must be seeded by the nodes whose box actually
/// MOVED, never by the scheduled-layout set.
///
/// Layout invalidation propagates to ancestors, so resizing any single row puts
/// the document root in `work.layout`. Seeding the subtree expansion from that
/// set therefore walks the entire document and re-projects every node, for a
/// one-row change, on every layout-touching frame. The work counters cannot see
/// it: `accessibility_nodes_updated` reports the scheduled ACCESSIBILITY set
/// (`schedule.rs`), which stays at 1 while the projection does N.
///
/// `RuntimeDocument::apply_hit_test_work` already refuses `work.layout` for
/// exactly this reason; this is the same rule for the accessibility seed.
#[test]
fn accessibility_delta_seeds_from_moved_boxes_not_scheduled_layout() {
    const ROWS: u64 = 200;
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(), NodeKind::Document);
    queue.create(node(2), document(), NodeKind::Element { tag: "div".into() });
    queue.insert(node(1), node(2), None);
    let row_style = |height: f32| NodeStyle {
        layout: Arc::new(LayoutStyle {
            height: Some(nana_ui_core::LengthSpec::Px(height)),
            ..LayoutStyle::default()
        }),
        ..NodeStyle::default()
    };
    for row in 0..ROWS {
        let row_id = node(3 + row * 2);
        let label_id = node(4 + row * 2);
        queue.create(row_id, document(), NodeKind::Element { tag: "div".into() });
        queue.create(label_id, document(), NodeKind::Text);
        queue.insert(node(2), row_id, None);
        queue.insert(row_id, label_id, None);
        queue.set_style(row_id, row_style(20.0));
    }
    world.commit(queue).unwrap();
    let mount = world.take_system_work();
    world.resolve_styles(&mount.style).unwrap();
    let _ = world.project_accessibility_delta(&mount);

    // One paint/layout change on the LAST row. Nothing below it can shift.
    let last_row = node(3 + (ROWS - 1) * 2);
    let mut queue = MutationQueue::new();
    queue.set_style(last_row, row_style(26.0));
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    // Precondition: the scheduled-layout set really does reach the root, so
    // this test would be vacuous if it did not.
    assert!(
        work.layout.contains(&node(1)),
        "layout invalidation must propagate to the document root for this to \
         be the seed that matters; got {:?}",
        work.layout
    );

    let delta = world.project_accessibility_delta(&work);
    assert!(
        delta.updated.len() <= 8,
        "one row changed, so the accessibility delta must stay bounded; \
         projected {} of {} nodes",
        delta.updated.len(),
        ROWS * 2 + 2
    );
}

/// The other direction, as an equivalence rather than a spot check: applying
/// the incremental delta must leave the retained accessibility tree identical
/// to a full projection.
///
/// This is the case the subtree expansion exists for. A scroll marks INPUT on
/// the scroller ALONE -- descendants keep their `LayoutBox` and carry no
/// ACCESSIBILITY bit -- yet every descendant moved in viewport space. Narrowing
/// the seed far enough to drop `input_hit_test` would leave the host holding
/// stale bounds for the entire scrolled subtree, and the counters would still
/// read clean.
#[test]
fn scrolling_publishes_the_moved_subtree_and_matches_a_full_projection() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(), NodeKind::Document);
    queue.create(node(2), document(), NodeKind::Element { tag: "div".into() });
    queue.insert(node(1), node(2), None);
    queue.set_scroll_metrics(
        node(2),
        Some(crate::ScrollMetrics {
            content_width: 100.0,
            content_height: 400.0,
            viewport_width: 100.0,
            viewport_height: 100.0,
            origin_x: 0.0,
            origin_y: 0.0,
        }),
    );
    for row in 0..6u64 {
        let row_id = node(3 + row);
        queue.create(row_id, document(), NodeKind::Element { tag: "div".into() });
        queue.insert(node(2), row_id, None);
        queue.write_layout(
            row_id,
            LayoutBox {
                x: 0.0,
                y: row as f32 * 20.0,
                width: 100.0,
                height: 20.0,
            },
        );
    }
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 400.0,
        },
    );
    queue.write_layout(
        node(2),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    );
    world.commit(queue).unwrap();
    world
        .resolve_styles(&world.document_order(document()))
        .unwrap();
    world.take_system_work();

    let snapshot = |world: &UiWorld| {
        world
            .project_accessibility(document())
            .into_iter()
            .map(|node| (node.id, node))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let mut retained = snapshot(&world);

    for offset in [40.0f32, 0.0, 80.0] {
        let mut queue = MutationQueue::new();
        queue.set_scroll_offset(node(2), crate::ScrollOffset { x: 0.0, y: offset });
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        // Precondition: a scroll must NOT mark the descendants dirty itself,
        // or the expansion this guards would be doing no work.
        assert_eq!(
            work.accessibility,
            Vec::new(),
            "a scroll marks no ACCESSIBILITY bit; the seed expansion is what \
             must publish the moved subtree"
        );
        let delta = world.project_accessibility_delta(&work);
        for id in delta.removed {
            retained.remove(&id);
        }
        for entry in delta.updated {
            retained.insert(entry.id, entry);
        }
        assert_eq!(
            retained,
            snapshot(&world),
            "incremental delta diverged from a full projection after scrolling \
             to {offset}"
        );
    }
}
