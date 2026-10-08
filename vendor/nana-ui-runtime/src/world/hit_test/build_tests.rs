use super::*;

#[test]
fn inactive_retained_roots_prune_and_reactivate_without_losing_siblings() {
    for nested in [false, true] {
        let mut world = UiWorld::new();
        let document = DocumentId::new(1).unwrap();
        let node = |value| StableNodeId::new(value).unwrap();
        let mut create = MutationQueue::new();
        if nested {
            create.create(
                node(10_001),
                document,
                NodeKind::Element {
                    tag: "container".into(),
                },
            );
            create.set_interaction(
                node(10_001),
                InteractionState {
                    pointer_events: false,
                    ..Default::default()
                },
            );
        }
        for value in 1..=10_000 {
            create.create(
                node(value),
                document,
                NodeKind::Element { tag: "div".into() },
            );
            create.write_layout(
                node(value),
                LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width: 40.0,
                    height: 40.0,
                },
            );
            let mut interaction = InteractionState::default();
            interaction.pointer_events = false;
            create.set_interaction(node(value), interaction);
            if nested {
                create.insert(node(10_001), node(value), None);
            }
        }
        world.commit(create).unwrap();
        world
            .resolve_styles(&world.document_order(document))
            .unwrap();
        world.rebuild_hit_test(document);
        let index = &world.hit_test_index[&document];
        let mut candidates = 0;
        index.root_bounds.visit(10.0, 10.0, &mut |_| {
            candidates += 1;
            false
        });
        assert_eq!(
            candidates, 0,
            "inactive sibling ranges reached the hit query"
        );
        assert_eq!(index.entries.len(), 10_000 + usize::from(nested));

        // Removing one entry must not drop the retained sibling slots just because
        // all their query bounds are inactive. A remaining slot can become active.
        let mut remove = MutationQueue::new();
        remove.despawn_subtree(node(1));
        world.commit(remove).unwrap();
        let mut enable = MutationQueue::new();
        enable.set_interaction(node(10_000), InteractionState::default());
        world.commit(enable).unwrap();
        assert!(world.rebuild_hit_test_scoped(document, &[node(10_000)]));
        assert_eq!(
            world.hit_test_candidates(document, 10.0, 10.0),
            [node(10_000)]
        );
        assert_eq!(
            world.hit_test_index[&document].entries.len(),
            9_999 + usize::from(nested)
        );
        world.rebuild_hit_test(document);
        assert_eq!(
            world.hit_test_candidates(document, 10.0, 10.0),
            [node(10_000)]
        );
        let mut disable = MutationQueue::new();
        disable.set_interaction(
            node(10_000),
            InteractionState {
                pointer_events: false,
                ..Default::default()
            },
        );
        world.commit(disable).unwrap();
        assert!(world.rebuild_hit_test_scoped(document, &[node(10_000)]));
        assert!(world.hit_test_candidates(document, 10.0, 10.0).is_empty());
        assert_eq!(
            world.hit_test_index[&document].entries.len(),
            9_999 + usize::from(nested)
        );
    }
}

#[test]
fn hidden_container_keeps_clip_order_and_constant_work_scroll() {
    let mut world = UiWorld::new();
    let document = DocumentId::new(1).unwrap();
    let node = |value| StableNodeId::new(value).unwrap();
    let mut create = MutationQueue::new();
    for value in 1..=5 {
        create.create(
            node(value),
            document,
            NodeKind::Element { tag: "div".into() },
        );
        if value > 1 {
            create.insert(
                node(if value == 3 || value == 4 { 2 } else { 1 }),
                node(value),
                None,
            );
        }
        create.write_layout(
            node(value),
            LayoutBox {
                x: 0.0,
                y: if value == 3 { 50.0 } else { 0.0 },
                width: 40.0,
                height: 40.0,
            },
        );
        let mut layout = LayoutStyle::default();
        layout.paint.visibility = Some(if value == 2 {
            nana_ui_core::VisibilitySpec::Hidden
        } else {
            nana_ui_core::VisibilitySpec::Visible
        });
        if value == 2 {
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
        }
        create.set_style(
            node(value),
            NodeStyle {
                layout: Arc::new(layout),
                ..Default::default()
            },
        );
    }
    world.commit(create).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    world.rebuild_hit_test(document);
    assert!(
        !world
            .hit_test_candidates(document, 10.0, 55.0)
            .contains(&node(3))
    );
    assert_eq!(
        world.hit_test_candidates(document, 10.0, 10.0).first(),
        Some(&node(5))
    );
    world.take_scroll_hit_updates();
    let built_before = world.last_work_counters().hit_test_nodes_rebuilt;
    let mut scroll = MutationQueue::new();
    scroll.set_scroll_offset(node(2), ScrollOffset { x: 0.0, y: 50.0 });
    world.commit(scroll).unwrap();
    for (scroller, delta) in world.take_scroll_hit_updates() {
        world.update_hit_test_scroll(document, scroller, delta);
    }
    let hits = world.hit_test_candidates(document, 10.0, 10.0);
    assert!(hits.contains(&node(3)));
    assert!(!hits.contains(&node(4)));
    assert!(!hits.contains(&node(2)));
    assert_eq!(
        world.last_work_counters().hit_test_nodes_rebuilt,
        built_before
    );
    world.rebuild_hit_test(document);
    assert_eq!(hits, world.hit_test_candidates(document, 10.0, 10.0));
}

fn fixture() -> (UiWorld, DocumentId) {
    let mut world = UiWorld::new();
    let document = DocumentId::new(1).unwrap();
    let mut queue = MutationQueue::new();
    for value in 1..=128 {
        let id = StableNodeId::new(value).unwrap();
        queue.create(id, document, NodeKind::Element { tag: "div".into() });
        if value > 1 && value < 128 {
            queue.insert(StableNodeId::new(value / 2).unwrap(), id, None);
        }
        queue.write_layout(
            id,
            LayoutBox {
                x: (value % 7) as f32 * 12.0,
                y: (value % 11) as f32 * 8.0,
                width: 75.0,
                height: 45.0,
            },
        );
        let mut layout = LayoutStyle {
            z_index: Some((value % 3) as i32),
            transform: Some(nana_ui_core::PaintTransform {
                e: (value % 4) as f32,
                f: (value % 5) as f32,
                ..Default::default()
            }),
            ..Default::default()
        };
        if value == 3 {
            layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
            layout.overflow_y = nana_ui_core::OverflowSpec::Hidden;
        }
        if value == 1 || value == 4 {
            queue.set_scroll_offset(id, ScrollOffset { x: 7.0, y: 11.0 });
        }
        layout.paint.visibility = Some(if value == 2 || value == 6 {
            nana_ui_core::VisibilitySpec::Hidden
        } else {
            nana_ui_core::VisibilitySpec::Visible
        });
        queue.set_style(
            id,
            NodeStyle {
                layout: Arc::new(layout),
                ..Default::default()
            },
        );
    }
    world.commit(queue).unwrap();
    let order = world.document_order(document);
    world.resolve_styles(&order).unwrap();
    (world, document)
}

#[test]
fn direct_hit_index_matches_forest_order_bounds_and_queries() {
    let (mut world, document) = fixture();
    let roots = world.document_roots(document);
    let seeds = roots.into_iter().map(|id| (id, IDENTITY_AFFINE)).collect();
    let mut forest = world.build_hit_forest(seeds);
    for entry in &mut forest {
        sort_hit_children(entry);
    }
    forest.sort_by_key(|entry| (entry.z_index, entry.order));
    let reference = HitIndex::from_forest(forest);
    world.rebuild_hit_test(document);
    let direct = &world.hit_test_index[&document];
    assert_eq!(direct.roots, reference.roots);
    assert_eq!(direct.entries.len(), reference.entries.len());
    for (id, actual) in &direct.entries {
        let expected = &reference.entries[id];
        assert_eq!(actual.parent, expected.parent);
        assert_eq!(actual.children, expected.children);
        assert_eq!(actual.sibling_slot, expected.sibling_slot);
        assert_eq!(actual.bounds, expected.bounds);
        assert_eq!(actual.entry.transform, expected.entry.transform);
        assert_eq!(actual.entry.self_clips, expected.entry.self_clips);
        assert_eq!(actual.entry.child_clips, expected.entry.child_clips);
        assert!(actual.entry.children.is_empty());
    }
    let points = (-20..180)
        .step_by(5)
        .flat_map(|x| (-20..180).step_by(5).map(move |y| (x as f32, y as f32)))
        .collect::<Vec<_>>();
    let actual = points
        .iter()
        .map(|&(x, y)| world.hit_test_candidates(document, x, y))
        .collect::<Vec<_>>();
    world.hit_test_index.insert(document, reference);
    let expected = points
        .iter()
        .map(|&(x, y)| world.hit_test_candidates(document, x, y))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn scoped_hidden_ancestor_updates_match_full_rebuild() {
    for hidden_document_root in [false, true] {
        for update_parent in [false, true] {
            let mut world = UiWorld::new();
            let document = DocumentId::new(1).unwrap();
            let node = |value| StableNodeId::new(value).unwrap();
            let mut create = MutationQueue::new();
            for value in 1..=4 {
                let id = node(value);
                create.create(id, document, NodeKind::Element { tag: "div".into() });
                if value > 1 {
                    create.insert(node(if value == 2 { 1 } else { 2 }), id, None);
                }
                create.write_layout(
                    id,
                    LayoutBox {
                        x: 0.0,
                        y: 0.0,
                        width: 40.0,
                        height: 40.0,
                    },
                );
                let hidden = value == 2 || (value == 1 && hidden_document_root);
                create.set_style(
                    id,
                    NodeStyle {
                        layout: Arc::new(LayoutStyle {
                            paint: nana_ui_core::PaintStyle {
                                visibility: Some(if hidden {
                                    nana_ui_core::VisibilitySpec::Hidden
                                } else {
                                    nana_ui_core::VisibilitySpec::Visible
                                }),
                                ..Default::default()
                            }
                            .into(),
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                );
            }
            world.commit(create).unwrap();
            world
                .resolve_styles(&world.document_order(document))
                .unwrap();
            world.rebuild_hit_test(document);
            world.take_system_work();

            let dirty = node(if update_parent { 2 } else { 3 });
            let mut update = MutationQueue::new();
            if update_parent {
                let mut style = world.record(dirty).style.clone();
                Arc::make_mut(&mut style.layout).transform = Some(nana_ui_core::PaintTransform {
                    e: 100.0,
                    ..Default::default()
                });
                update.set_style(dirty, style);
            } else {
                update.write_layout(
                    dirty,
                    LayoutBox {
                        x: 100.0,
                        y: 0.0,
                        width: 40.0,
                        height: 40.0,
                    },
                );
            }
            world.commit(update).unwrap();
            let work = world.take_system_work();
            world.resolve_styles(&work.style).unwrap();
            let built_before = world
                .last_work_counters()
                .hit_test_nodes_rebuilt
                .unwrap_or_default();
            assert!(world.rebuild_hit_test_scoped(document, &[dirty]));
            let rebuilt = world
                .last_work_counters()
                .hit_test_nodes_rebuilt
                .unwrap_or_default()
                - built_before;
            assert_eq!(rebuilt, if update_parent { 3 } else { 1 });
            let probes = [10.0, 110.0].map(|x| world.hit_test_candidates(document, x, 10.0));
            assert!(
                !probes[0].contains(&node(3)),
                "stale hit: root_hidden={hidden_document_root}, parent_update={update_parent}"
            );
            assert!(probes[1].contains(&node(3)));
            assert!(probes[usize::from(update_parent)].contains(&node(4)));
            world.rebuild_hit_test(document);
            assert_eq!(
                probes,
                [10.0, 110.0].map(|x| world.hit_test_candidates(document, x, 10.0))
            );
            // A container's visibility changes its own hit eligibility, while
            // its visible descendants retain their hierarchy and appear once.
            for visibility in [
                nana_ui_core::VisibilitySpec::Visible,
                nana_ui_core::VisibilitySpec::Hidden,
                nana_ui_core::VisibilitySpec::Visible,
            ] {
                let mut style = world.record(node(2)).style.clone();
                Arc::make_mut(&mut style.layout).paint.visibility = Some(visibility);
                let mut update = MutationQueue::new();
                update.set_style(node(2), style);
                world.commit(update).unwrap();
                let work = world.take_system_work();
                world.resolve_styles(&work.style).unwrap();
                assert!(world.rebuild_hit_test_scoped(document, &[node(2)]));
                let scoped = [10.0, 110.0].map(|x| world.hit_test_candidates(document, x, 10.0));
                assert_eq!(scoped[1].iter().filter(|&&id| id == node(3)).count(), 1);
                world.rebuild_hit_test(document);
                assert_eq!(
                    scoped,
                    [10.0, 110.0].map(|x| world.hit_test_candidates(document, x, 10.0))
                );
            }
        }
    }
}

#[test]
fn viewport_fixed_projection_escapes_ancestor_clip_and_scroll_but_keeps_lifecycle() {
    let mut world = UiWorld::new();
    let document = DocumentId::new(1).unwrap();
    let id = |value| StableNodeId::new(value).unwrap();
    let mut queue = MutationQueue::new();
    for (value, x, y, size) in [
        (1, 0.0, 0.0, 30.0),
        (2, 100.0, 100.0, 60.0),
        (3, 110.0, 110.0, 20.0),
    ] {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.write_layout(
            id(value),
            LayoutBox {
                x,
                y,
                width: size,
                height: size,
            },
        );
        queue.set_interaction(
            id(value),
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
    }
    queue.insert(id(1), id(2), None);
    queue.insert(id(2), id(3), None);
    let outer = NodeStyle {
        layout: Arc::new(LayoutStyle {
            overflow_x: nana_ui_core::OverflowSpec::Hidden,
            overflow_y: nana_ui_core::OverflowSpec::Scroll,
            transform: Some(nana_ui_core::PaintTransform {
                e: 35.0,
                f: 25.0,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    let fixed = NodeStyle {
        layout: Arc::new(LayoutStyle {
            position: PositionSpec::Fixed,
            transform: Some(nana_ui_core::PaintTransform {
                e: 3.0,
                f: 4.0,
                ..Default::default()
            }),
            ..Default::default()
        }),
        ..Default::default()
    };
    queue.set_style(id(1), outer);
    queue.set_style(id(2), fixed.clone());
    world.commit(queue).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    world.rebuild_hit_test(document);
    assert_eq!(world.hit_test(document, 150.0, 150.0), Some(id(2)));
    assert_eq!(world.hit_test(document, 115.0, 115.0), Some(id(3)));
    assert_eq!(
        world.pointer_layout_position(id(3), 115.0, 115.0),
        Some((112.0, 111.0))
    );

    let mut scroll = MutationQueue::new();
    scroll.set_scroll_offset(id(1), ScrollOffset { x: 0.0, y: 70.0 });
    world.commit(scroll).unwrap();
    world.update_hit_test_scroll(document, id(1), [0.0, -70.0]);
    assert_eq!(world.hit_test(document, 115.0, 115.0), Some(id(3)));
    world.rebuild_hit_test(document);
    assert_eq!(world.hit_test(document, 115.0, 115.0), Some(id(3)));

    let mut scroll = MutationQueue::new();
    scroll.set_scroll_offset(id(2), ScrollOffset { x: 0.0, y: 10.0 });
    world.commit(scroll).unwrap();
    world.update_hit_test_scroll(document, id(2), [0.0, -10.0]);
    assert_eq!(world.hit_test(document, 115.0, 105.0), Some(id(3)));
    assert_eq!(
        world.pointer_layout_position(id(3), 115.0, 105.0),
        Some((112.0, 111.0))
    );

    let mut disabled = fixed.clone();
    Arc::make_mut(&mut disabled.layout).pointer_events = Some(PointerEventsSpec::None);
    let mut change = MutationQueue::new();
    change.set_style(id(2), disabled);
    world.commit(change).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    assert!(world.rebuild_hit_test_scoped(document, &[id(2)]));
    assert_eq!(world.hit_test(document, 115.0, 105.0), None);

    let mut change = MutationQueue::new();
    change.set_style(id(2), fixed);
    world.commit(change).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    assert!(world.rebuild_hit_test_scoped(document, &[id(2)]));
    assert_eq!(world.hit_test(document, 115.0, 105.0), Some(id(3)));
    let mut park = MutationQueue::new();
    park.park_subtree(id(1));
    world.commit(park).unwrap();
    world.resolve_styles(&[id(1), id(2), id(3)]).unwrap();
    if !world.rebuild_hit_test_scoped(document, &[id(1)]) {
        world.rebuild_hit_test(document);
    }
    assert_eq!(world.hit_test(document, 115.0, 105.0), None);
}

#[test]
fn viewport_fixed_hit_order_respects_structural_stacking_and_position_changes() {
    let mut world = UiWorld::new();
    let document = DocumentId::new(1).unwrap();
    let id = |value| StableNodeId::new(value).unwrap();
    let mut queue = MutationQueue::new();
    for value in 1..=4 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.write_layout(
            id(value),
            LayoutBox {
                x: 100.0,
                y: 100.0,
                width: 80.0,
                height: 80.0,
            },
        );
        queue.set_interaction(
            id(value),
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
    }
    queue.insert(id(1), id(2), None);
    queue.insert(id(2), id(3), None);
    let style = |position, z| NodeStyle {
        layout: Arc::new(LayoutStyle {
            position,
            z_index: z,
            ..Default::default()
        }),
        ..Default::default()
    };
    queue.set_style(id(1), style(PositionSpec::Relative, Some(10)));
    let fixed = style(PositionSpec::Fixed, Some(100));
    queue.set_style(id(2), fixed.clone());
    queue.set_style(id(4), style(PositionSpec::Relative, Some(20)));
    world.commit(queue).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    world.rebuild_hit_test(document);
    assert_eq!(
        world.hit_test(document, 120.0, 120.0),
        Some(id(3)),
        "a position:fixed overlay leaves its parent's stacking group, matching Scene paint"
    );

    let mut change = MutationQueue::new();
    change.set_style(id(1), style(PositionSpec::Relative, Some(30)));
    world.commit(change).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    if !world.rebuild_hit_test_scoped(document, &[id(1)]) {
        world.rebuild_hit_test(document);
    }
    assert_eq!(world.hit_test(document, 120.0, 120.0), Some(id(3)));

    // Both directions migrate the spatial root without retaining a stale copy.
    for position in [PositionSpec::Relative, PositionSpec::Fixed] {
        let mut change = MutationQueue::new();
        change.set_style(id(2), style(position, Some(100)));
        world.commit(change).unwrap();
        world
            .resolve_styles(&world.document_order(document))
            .unwrap();
        assert!(world.rebuild_hit_test_scoped(document, &[id(2)]));
        let candidates = world.hit_test_candidates(document, 120.0, 120.0);
        assert_eq!(candidates.iter().filter(|node| **node == id(2)).count(), 1);
        assert_eq!(candidates.first(), Some(&id(3)));
        let incremental = candidates;
        world.rebuild_hit_test(document);
        assert_eq!(
            incremental,
            world.hit_test_candidates(document, 120.0, 120.0)
        );
    }
    // No authored group: later ordinary paint must beat an earlier fixed node.
    let mut change = MutationQueue::new();
    change.set_style(id(1), style(PositionSpec::Relative, None));
    change.set_style(id(2), style(PositionSpec::Fixed, None));
    change.set_style(id(4), style(PositionSpec::Relative, None));
    world.commit(change).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    world.rebuild_hit_test(document);
    assert_eq!(world.hit_test(document, 120.0, 120.0), Some(id(4)));
}

#[test]
fn fixed_overlay_hits_above_later_siblings_of_an_isolating_parent() {
    let mut world = UiWorld::new();
    let document = DocumentId::new(1).unwrap();
    let id = |value| StableNodeId::new(value).unwrap();
    let mut queue = MutationQueue::new();
    for value in 1..=3 {
        queue.create(id(value), document, NodeKind::Element { tag: "div".into() });
        queue.write_layout(
            id(value),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 80.0,
                height: 80.0,
            },
        );
        queue.set_interaction(
            id(value),
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
    }
    queue.insert(id(1), id(2), None);
    queue.set_style(
        id(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                position: PositionSpec::Relative,
                z_index: Some(0),
                isolation: true,
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    queue.set_style(
        id(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                position: PositionSpec::Fixed,
                z_index: Some(1_000),
                ..Default::default()
            }),
            ..Default::default()
        },
    );
    world.commit(queue).unwrap();
    world
        .resolve_styles(&world.document_order(document))
        .unwrap();
    world.rebuild_hit_test(document);
    assert_eq!(
        world.hit_test(document, 40.0, 40.0),
        Some(id(2)),
        "a position:fixed menu must hit above a later sibling of its isolating parent"
    );
}

#[test]
fn viewport_fixed_overlap_preserves_select_menu_layer_and_inactive_fast_path() {
    let mut context = crate::AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let root = context
        .create_component(document, crate::Stack::column(0.0))
        .unwrap();
    let select = context
        .create_detached_component(
            document,
            crate::Select::new(Some("a"))
                .options([
                    crate::SelectOption::new("a", "Alpha"),
                    crate::SelectOption::new("b", "Beta"),
                ])
                .opened(true),
        )
        .unwrap();
    let mut button = crate::Button::new("Cover");
    let style = Arc::make_mut(&mut button.style.layout);
    style.position = PositionSpec::Fixed;
    style.offset_left = Some(LengthSpec::Px(0.0));
    style.offset_top = Some(LengthSpec::Px(0.0));
    style.width = Some(LengthSpec::Px(300.0));
    style.height = Some(LengthSpec::Px(200.0));
    style.z_index = Some(100);
    let fixed = context.create_detached_component(document, button).unwrap();
    context.append_child(root, select).unwrap();
    context.append_child(root, fixed).unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
        .unwrap();
    context.rebuild_hit_test(document);
    let geometry = context
        .world()
        .component_geometry(select.stable_id())
        .unwrap();
    let crate::ComponentGeometry::Select {
        menu: Some(menu), ..
    } = geometry
    else {
        panic!("open Select geometry");
    };
    let point = (menu.surface.x + 10.0, menu.surface.y + 10.0);
    assert_eq!(
        context.world().hit_test(document, point.0, point.1),
        Some(select.stable_id()),
        "Select menu paints at effective z1000"
    );
    assert_eq!(
        context.world().hit_test(document, 10.0, 10.0),
        Some(fixed.stable_id()),
        "the Select trigger must not inherit its menu's z1000"
    );
    assert!(context.world().hit_test_index[&document].viewport_hit_at(10.0, 10.0));
    context
        .update_component(fixed, |button, _| {
            Arc::make_mut(&mut button.style.layout).pointer_events = Some(PointerEventsSpec::None)
        })
        .unwrap();
    context.resolve_styles(&[fixed.stable_id()]).unwrap();
    context.rebuild_hit_test_for(document, &[fixed.stable_id()]);
    assert!(
        !context.world().hit_test_index[&document].viewport_hit_at(10.0, 10.0),
        "non-interactive tooltip-like roots must retain the first-hit fast path"
    );
    context
        .update_component(fixed, |button, _| {
            Arc::make_mut(&mut button.style.layout).position = PositionSpec::Relative
        })
        .unwrap();
    context.resolve_styles(&[fixed.stable_id()]).unwrap();
    context.rebuild_hit_test_for(document, &[fixed.stable_id()]);
    assert!(
        context.world().hit_test_index[&document]
            .viewport_roots
            .is_empty(),
        "removing the last fixed boundary clears its index membership"
    );
}
