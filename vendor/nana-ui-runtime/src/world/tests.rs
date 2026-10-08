#[cfg(feature = "graph-canvas")]
use super::geometry::*;
use super::*;
use crate::TextInputState;
use crate::{
    AnimatableProperty, AnimationClass, AnimationEventKind, AnimationFillMode, AnimationId,
    AnimationPlayback, AnimationSpec, Easing, MeasureTextShaper, MotionCurve,
    MotionEvaluatorBackend, MotionInterrupt, MotionTo, MotionValue,
};
use nana_ui_core::{
    CursorSpec, LayoutStyle, LengthSpec, OverflowSpec, PaintMat4, PaintTransform,
    PointerEventsSpec, SemanticColorRole,
};

fn node(value: u64) -> StableNodeId {
    StableNodeId::new(value).unwrap()
}

fn document(value: u64) -> DocumentId {
    DocumentId::new(value).unwrap()
}

fn hit_entry_transform(world: &UiWorld, document: DocumentId, id: StableNodeId) -> [f32; 6] {
    find_hit_transform(&world.hit_test_index[&document], id).expect("hit entry")
}

/// Compile a theme from loose palette / metric overrides, the way
/// `AppContext::set_style_tokens` does, so a world-level test can install one
/// without going through the framework.
fn test_theme(
    mode: ThemeMode,
    metrics: nana_ui_core::ThemeMetrics,
    palette: nana_ui_core::SemanticPalette,
    titlebar: nana_ui_core::SemanticColor,
) -> std::sync::Arc<nana_ui_core::CompiledTheme> {
    std::sync::Arc::new(
        nana_ui_core::ThemeDefinition::for_mode(mode)
            .with_metrics(metrics)
            .with_palette(palette)
            .with_titlebar(Some(titlebar))
            .compile()
            .expect("a test theme compiles"),
    )
}

#[test]
fn batch_builds_reparents_and_detaches_hierarchy() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=4 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    queue.insert(node(1), node(4), Some(node(3)));
    let report = world.commit(queue).unwrap();
    assert_eq!(report.created, 4);
    assert_eq!(report.inserted, 3);
    assert_eq!(report.reparented, 0);
    assert_eq!(
        world.node(node(1)).unwrap().children,
        vec![node(2), node(4), node(3)]
    );

    let mut queue = MutationQueue::new();
    queue.insert(node(2), node(3), None);
    queue.detach(node(4));
    let report = world.commit(queue).unwrap();
    assert_eq!(report.reparented, 1);
    assert_eq!(report.detached, 1);
    assert_eq!(world.node(node(1)).unwrap().children, vec![node(2)]);
    assert_eq!(world.node(node(2)).unwrap().children, vec![node(3)]);
    assert_eq!(world.node(node(4)).unwrap().parent, None);
    assert_eq!(world.mount_state(node(4)), Some(MountState::Mounted));
    assert!(world.contains(node(4)));
    assert!(!world.document_order(document(1)).contains(&node(4)));
    assert!(world.extract_nodes(&[node(4)]).is_empty());

    let mut attach = MutationQueue::new();
    attach.insert(node(1), node(4), None);
    world.commit(attach).unwrap();
    assert_eq!(world.node(node(4)).unwrap().parent, Some(node(1)));
    assert!(world.document_order(document(1)).contains(&node(4)));
}

#[test]
fn document_roots_indexes_live_parentless_nodes_without_scanning() {
    fn scanned(world: &UiWorld, document: DocumentId) -> Vec<StableNodeId> {
        let mut roots = world
            .nodes
            .keys()
            .filter(|id| {
                let node = world.nodes.get(*id).expect("live key");
                node.document == document
                    && world.presence_live(*id)
                    && node.hierarchy.parent.is_none()
            })
            .collect::<Vec<_>>();
        roots.sort_unstable();
        roots
    }

    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "column".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    for row in 0..400u64 {
        let row_id = node(3 + row);
        queue.create(row_id, document(1), NodeKind::Element { tag: "row".into() });
        queue.insert(node(2), row_id, None);
    }
    world.commit(queue).unwrap();
    assert_eq!(world.document_roots(document(1)), vec![node(1)]);
    assert_eq!(
        world.document_roots(document(1)),
        scanned(&world, document(1))
    );

    let detached = node(13);
    let mut detach = MutationQueue::new();
    detach.detach(detached);
    world.commit(detach).unwrap();
    assert_eq!(world.node(detached).unwrap().parent, None);
    assert!(!world.presence_live(detached));
    assert_eq!(world.document_roots(document(1)), vec![node(1)]);
    assert_eq!(
        world.document_roots(document(1)),
        scanned(&world, document(1))
    );

    let mut extra = MutationQueue::new();
    extra.create(
        node(1000),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    world.commit(extra).unwrap();
    assert_eq!(world.document_roots(document(1)), vec![node(1), node(1000)]);
    assert_eq!(
        world.document_roots(document(1)),
        scanned(&world, document(1))
    );

    let mut park = MutationQueue::new();
    park.park_subtree(node(1000));
    world.commit(park).unwrap();
    assert_eq!(world.node(node(1000)).unwrap().parent, None);
    assert!(!world.presence_live(node(1000)));
    assert_eq!(world.document_roots(document(1)), vec![node(1)]);
    assert_eq!(
        world.document_roots(document(1)),
        scanned(&world, document(1))
    );

    let mut despawn = MutationQueue::new();
    despawn.despawn_subtree(node(1));
    world.commit(despawn).unwrap();
    assert!(world.document_roots(document(1)).is_empty());
    assert_eq!(
        world.document_roots(document(1)),
        scanned(&world, document(1))
    );
}

fn box_at(x: f32, y: f32, width: f32, height: f32) -> LayoutBox {
    LayoutBox {
        x,
        y,
        width,
        height,
    }
}

/// Hit entries built so far. The counter accumulates until a drain, so
/// callers compare deltas around the build they are measuring.
fn hit_entries_built(world: &UiWorld) -> usize {
    world
        .last_work_counters()
        .hit_test_nodes_rebuilt
        .unwrap_or_default()
}

/// Flatten the hit forest into a comparable projection. `HitEntry` has no
/// `PartialEq`, and this captures everything pointer dispatch reads.
fn hit_shape(world: &UiWorld, document: DocumentId) -> Vec<(Vec<u64>, [u32; 6], bool, i32)> {
    fn walk(
        index: &HitIndex,
        id: StableNodeId,
        path: &mut Vec<u64>,
        out: &mut Vec<(Vec<u64>, [u32; 6], bool, i32)>,
    ) {
        let node = &index.entries[&id];
        path.push(id.get());
        out.push((
            path.clone(),
            find_hit_transform(index, id).unwrap().map(f32::to_bits),
            node.entry.hittable,
            node.entry.z_index,
        ));
        for child in node.children.iter().flatten() {
            walk(index, *child, path, out);
        }
        path.pop();
    }
    let mut out = Vec::new();
    let index = &world.hit_test_index[&document];
    for root in index.roots.iter().flatten() {
        walk(index, *root, &mut Vec::new(), &mut out);
    }
    out
}

/// Probe a coordinate grid so equivalence is asserted on what dispatch sees,
/// not just on internal layout of the index.
fn hit_probe_grid(world: &UiWorld, document: DocumentId) -> Vec<Vec<StableNodeId>> {
    let mut probes = Vec::new();
    for step_y in 0..12 {
        for step_x in 0..12 {
            let x = step_x as f32 * 9.0;
            let y = step_y as f32 * 9.0;
            probes.push(world.hit_test_candidates(document, x, y));
        }
    }
    probes
}

/// Grow a document of `rows` leaves under one column and return the world.
fn world_with_rows(rows: u64) -> UiWorld {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "column".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    for row in 0..rows {
        let row_id = node(3 + row);
        queue.create(row_id, document(1), NodeKind::Element { tag: "row".into() });
        queue.insert(node(2), row_id, None);
    }
    world.commit(queue).unwrap();
    world
}

#[test]
fn retired_ledger_stays_bounded_under_create_despawn_churn() {
    let mut world = UiWorld::new();
    let mut root = MutationQueue::new();
    root.create(node(1), document(1), NodeKind::Document);
    world.commit(root).unwrap();

    // Mirror a v-for churning its rows: allocate monotonically, despawn the
    // whole batch, repeat. Retirement stays permanent, so a stale handle
    // still cannot alias, but the ledger must not grow with the churn.
    let mut next = 2u64;
    for _ in 0..200 {
        let batch_start = next;
        let mut create = MutationQueue::new();
        for _ in 0..50 {
            let id = node(next);
            create.create(id, document(1), NodeKind::Element { tag: "row".into() });
            create.insert(node(1), id, None);
            next += 1;
        }
        world.commit(create).unwrap();
        let mut despawn = MutationQueue::new();
        for id in batch_start..next {
            despawn.despawn_subtree(node(id));
        }
        world.commit(despawn).unwrap();
        world.take_system_work();
    }

    let retired = world.retired_ids();
    assert_eq!(retired, 200 * 50);
    // Consecutive allocation coalesces into a single run regardless of how
    // many IDs churned through it.
    assert_eq!(world.retired_id_runs(), 1);

    // The contract still holds: a stale handle cannot be revived.
    let mut revive = MutationQueue::new();
    revive.create(node(2), document(1), NodeKind::Text);
    assert!(matches!(
        world.commit(revive),
        Err(UiWorldError::RetiredNode(_))
    ));
    assert!(world.is_retired(node(2)));
    assert!(world.is_retired(node(next - 1)));
    assert!(!world.is_retired(node(next)));
    assert!(!world.is_retired(node(1)));
}

#[test]
fn retired_runs_coalesce_across_gaps_in_any_insert_order() {
    let mut retired = RetiredIds::default();
    for value in [10u64, 12, 11, 30, 1, 2, 29, 31] {
        retired.insert(node(value));
    }
    assert_eq!(retired.len(), 8);
    // {1,2} {10,11,12} {29,30,31}
    assert_eq!(retired.runs(), 3);
    for value in [1u64, 2, 10, 11, 12, 29, 30, 31] {
        assert!(retired.contains(node(value)), "{value} must be retired");
    }
    for value in [3u64, 9, 13, 28, 32, u64::MAX] {
        assert!(!retired.contains(node(value)), "{value} must be live");
    }
    // Re-inserting is idempotent and does not double-count.
    retired.insert(node(11));
    assert_eq!(retired.len(), 8);
    assert_eq!(retired.runs(), 3);
    // Bridging two runs merges them.
    retired.insert(node(13));
    for value in [
        14u64, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
    ] {
        retired.insert(node(value));
    }
    assert_eq!(retired.runs(), 2);
}

#[test]
fn batch_validation_cost_tracks_the_batch_not_the_retained_world() {
    let small = 200u64;
    let large = 10_000u64;

    let mut small_world = world_with_rows(small);
    let mut large_world = world_with_rows(large);
    assert_eq!(small_world.len(), small as usize + 2);
    assert_eq!(large_world.len(), large as usize + 2);

    // A multi-mutation batch skips the single-mutation fast path, so this is
    // the ValidationPlan route Vue takes on every flush.
    let batch = |target: StableNodeId| {
        let mut queue = MutationQueue::new();
        queue.set_text(
            target,
            TextContent {
                value: "updated".into(),
            },
        );
        queue.set_style(
            target,
            NodeStyle {
                layout: std::sync::Arc::new(crate::LayoutStyle {
                    z_index: Some(3),
                    ..crate::LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        queue.set_scroll_offset(target, ScrollOffset { x: 0.0, y: 4.0 });
        queue
    };

    small_world.commit(batch(node(3 + small - 1))).unwrap();
    large_world.commit(batch(node(3 + large - 1))).unwrap();

    // The 50x larger world must not cost 50x to validate. Before the overlay
    // rewrite both numbers tracked `len()` exactly.
    assert_eq!(validation_scanned(&mut small_world), 0);
    assert_eq!(validation_scanned(&mut large_world), 0);
}

/// Nodes validation visited since the previous drain, which is the window a
/// frame consumes.
fn validation_scanned(world: &mut UiWorld) -> usize {
    world
        .take_system_work()
        .validation_nodes_scanned
        .unwrap_or_default()
}

/// Build a nested grid document with clipping, transforms, z-index and a
/// scroller so scoped patches are exercised against the tricky cases.
fn hit_fixture(columns: u64, rows: u64) -> UiWorld {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.write_layout(node(1), box_at(0.0, 0.0, 100.0, 100.0));
    for column in 0..columns {
        let column_id = node(10 + column * 100);
        queue.create(
            column_id,
            document(1),
            NodeKind::Element {
                tag: "column".into(),
            },
        );
        queue.insert(node(1), column_id, None);
        queue.write_layout(column_id, box_at(column as f32 * 10.0, 0.0, 10.0, 100.0));
        queue.set_style(
            column_id,
            NodeStyle {
                layout: Arc::new(LayoutStyle {
                    overflow_y: OverflowSpec::Scroll,
                    ..LayoutStyle::default()
                }),
                ..NodeStyle::default()
            },
        );
        for row in 0..rows {
            let cell = node(11 + column * 100 + row);
            queue.create(cell, document(1), NodeKind::Element { tag: "cell".into() });
            queue.insert(column_id, cell, None);
            queue.write_layout(
                cell,
                box_at(column as f32 * 10.0, row as f32 * 8.0, 10.0, 8.0),
            );
            if row % 3 == 0 {
                let mut raised = NodeStyle::default();
                Arc::make_mut(&mut raised.layout).z_index = Some(2);
                queue.set_style(cell, raised);
            }
        }
    }
    world.commit(queue).unwrap();
    world.take_system_work();
    world.rebuild_hit_test(document(1));
    world
}

#[test]
fn hit_test_matches_the_first_collected_candidate() {
    let world = hit_fixture(6, 8);
    // The short-circuit walk must agree with the collecting walk everywhere,
    // including misses and clipped regions.
    for step_y in 0..30 {
        for step_x in 0..30 {
            let x = step_x as f32 * 2.5 - 5.0;
            let y = step_y as f32 * 3.5 - 5.0;
            assert_eq!(
                world.hit_test(document(1), x, y),
                world
                    .hit_test_candidates(document(1), x, y)
                    .into_iter()
                    .next(),
                "hit_test disagreed at ({x}, {y})"
            );
        }
    }
}

#[test]
fn every_hit_query_is_counted_once() {
    let world = hit_fixture(2, 4);
    let before = world.hit_test_queries();
    world.hit_test(document(1), 5.0, 5.0);
    world.hit_test(document(1), 500.0, 500.0);
    world.hit_test_candidates(document(1), 15.0, 5.0);
    assert_eq!(world.hit_test_queries() - before, 3);
}

#[test]
fn tree_depth_is_bounded_where_the_tree_is_written() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    for id in 2..=(MAX_TREE_DEPTH as u64 + 8) {
        create.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    world.commit(create).unwrap();

    // A chain built one level at a time is rejected at the limit, not at a
    // stack overflow inside style resolution or layout.
    let mut depth = 1u64;
    let rejected = loop {
        let parent = node(depth);
        let child = node(depth + 1);
        let mut link = MutationQueue::new();
        link.insert(parent, child, None);
        match world.commit(link) {
            Ok(_) => depth += 1,
            Err(error) => break error,
        }
        assert!(depth < MAX_TREE_DEPTH as u64 + 8, "depth was never bounded");
    };
    assert!(matches!(rejected, UiWorldError::TreeTooDeep { .. }));
    assert_eq!(depth as usize, MAX_TREE_DEPTH);

    // The rejected batch left nothing behind, and the accepted tree still
    // resolves and lays out without recursing past the bound.
    assert_eq!(
        world.node(node(MAX_TREE_DEPTH as u64 + 1)).unwrap().parent,
        None
    );
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.rebuild_hit_test(document(1));

    // Splicing an already-deep detached subtree under a deep parent is
    // rejected on the combined height, not just the parent's depth.
    let mut detached = MutationQueue::new();
    detached.insert(
        node(MAX_TREE_DEPTH as u64 + 1),
        node(MAX_TREE_DEPTH as u64 + 2),
        None,
    );
    world.commit(detached).unwrap();
    let mut splice = MutationQueue::new();
    splice.insert(
        node(MAX_TREE_DEPTH as u64),
        node(MAX_TREE_DEPTH as u64 + 1),
        None,
    );
    assert!(matches!(
        world.commit(splice),
        Err(UiWorldError::TreeTooDeep { .. })
    ));
}

#[test]
fn scoped_hit_patch_matches_a_full_rebuild_for_a_leaf_layout_change() {
    let mut world = hit_fixture(6, 8);
    let target = node(11 + 3 * 100 + 5);

    let mut move_cell = MutationQueue::new();
    move_cell.write_layout(target, box_at(31.0, 41.0, 8.0, 6.0));
    world.commit(move_cell).unwrap();
    let work = world.take_system_work();
    // The frame driver patches from the INPUT set, which layout writeback
    // marks on exactly the nodes whose box moved.
    let dirty = work.input_hit_test.clone();
    assert_eq!(dirty, vec![target]);

    let before_patch = hit_entries_built(&world);
    assert!(world.rebuild_hit_test_scoped(document(1), &dirty));
    let patched_built = hit_entries_built(&world) - before_patch;
    let patched_shape = hit_shape(&world, document(1));
    let patched_probe = hit_probe_grid(&world, document(1));

    let before_full = hit_entries_built(&world);
    world.rebuild_hit_test(document(1));
    let full_built = hit_entries_built(&world) - before_full;
    assert_eq!(patched_shape, hit_shape(&world, document(1)));
    assert_eq!(patched_probe, hit_probe_grid(&world, document(1)));

    // The whole point: the patch built a subtree, not the document.
    assert_eq!(patched_built, 1);
    assert!(
        patched_built < full_built / 4,
        "scoped patch built {patched_built} entries, full rebuild built {full_built}"
    );
}

#[test]
fn scoped_hit_reparent_keeps_new_owner_in_both_patch_orders() {
    for (source, destination) in [(10, 110), (110, 10)] {
        let mut world = hit_fixture(2, 4);
        let target = node(source + 1);
        let mut mutation = MutationQueue::new();
        mutation.insert(node(destination), target, None);
        mutation.write_layout(target, box_at(21.0, 11.0, 8.0, 6.0));
        world.commit(mutation).unwrap();
        let work = world.take_system_work();
        assert!(world.rebuild_hit_test_scoped(document(1), &work.input_hit_test));
        let patched = hit_probe_grid(&world, document(1));
        let shape = hit_shape(&world, document(1));
        assert!(
            world.hit_test_index[&document(1)]
                .entries
                .contains_key(&target)
        );
        world.rebuild_hit_test(document(1));
        assert_eq!(patched, hit_probe_grid(&world, document(1)));
        assert_eq!(shape, hit_shape(&world, document(1)));
    }
}

#[test]
fn scoped_hit_patch_handles_visibility_insertion_and_removal() {
    let mut world = hit_fixture(4, 6);
    let column = node(10 + 100);
    let cell = node(11 + 100 + 2);

    // Hiding a cell drops it from the parent's children.
    let mut hide = MutationQueue::new();
    hide.set_style(
        cell,
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                hidden: true,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(hide).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut dirty = work.layout.clone();
    dirty.extend(work.input_hit_test.iter().copied());
    dirty.extend(work.style.iter().copied());
    dirty.sort_unstable();
    dirty.dedup();
    assert!(world.rebuild_hit_test_scoped(document(1), &dirty));
    let patched = hit_probe_grid(&world, document(1));
    world.rebuild_hit_test(document(1));
    assert_eq!(patched, hit_probe_grid(&world, document(1)));
    assert!(
        !world
            .hit_test_candidates(document(1), 12.0, 18.0)
            .contains(&cell)
    );

    // Inserting a new child must land in Hierarchy order, not at the end.
    let inserted = node(9_001);
    let mut insert = MutationQueue::new();
    insert.create(
        inserted,
        document(1),
        NodeKind::Element { tag: "new".into() },
    );
    insert.insert(column, inserted, Some(node(11 + 100)));
    insert.write_layout(inserted, box_at(10.0, 0.0, 10.0, 8.0));
    world.commit(insert).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut dirty = work.layout.clone();
    dirty.extend(work.input_hit_test.iter().copied());
    dirty.sort_unstable();
    dirty.dedup();
    assert!(world.rebuild_hit_test_scoped(document(1), &dirty));
    let patched = hit_probe_grid(&world, document(1));
    world.rebuild_hit_test(document(1));
    assert_eq!(patched, hit_probe_grid(&world, document(1)));
    assert!(
        world
            .hit_test_candidates(document(1), 12.0, 3.0)
            .contains(&inserted)
    );

    // Despawning the subtree keeps the index consistent with a rebuild.
    let mut despawn = MutationQueue::new();
    despawn.despawn_subtree(inserted);
    world.commit(despawn).unwrap();
    let work = world.take_system_work();
    let mut dirty = work.layout.clone();
    dirty.extend(work.input_hit_test.iter().copied());
    dirty.sort_unstable();
    dirty.dedup();
    assert!(world.rebuild_hit_test_scoped(document(1), &dirty));
    let patched = hit_probe_grid(&world, document(1));
    world.rebuild_hit_test(document(1));
    assert_eq!(patched, hit_probe_grid(&world, document(1)));
}

#[test]
fn overlay_validation_walks_hosts_not_every_entity() {
    let rows = 5_000u64;
    let mut world = world_with_rows(rows);

    // One real overlay host with a modal surface: validation may walk hosts
    // and document order, but must never enumerate the whole world per node.
    let host = node(3);
    let surface = node(3 + rows);
    let mut open = MutationQueue::new();
    open.create(
        surface,
        document(1),
        NodeKind::Element {
            tag: "dialog".into(),
        },
    );
    open.insert(host, surface, None);
    open.set_accessibility(
        surface,
        AccessibilityState {
            role: AccessibilityRole::Dialog,
            modal: true,
            ..AccessibilityState::default()
        },
    );
    open.set_overlay_host(
        host,
        OverlayHostState {
            active: Some(surface),
            restore_focus: None,
        },
    );
    world.commit(open).unwrap();
    // Close the measurement window on setup so the next reading is the
    // overlay-aware batch alone.
    validation_scanned(&mut world);

    // Text cannot change overlay references or roles: no host validation walk.
    let mut touch = MutationQueue::new();
    touch.set_text(node(4), TextContent { value: "a".into() });
    touch.set_text(node(5), TextContent { value: "b".into() });
    world.commit(touch).unwrap();
    assert_eq!(validation_scanned(&mut world), 0);

    // Despawning the surface clears host references without a world scan.
    let mut close = MutationQueue::new();
    close.despawn_subtree(surface);
    close.set_text(node(4), TextContent { value: "c".into() });
    world.commit(close).unwrap();
    let scanned = validation_scanned(&mut world);
    assert!(
        scanned < rows as usize,
        "overlay teardown scanned {scanned} of {} nodes",
        world.len()
    );
    assert_eq!(world.overlay_host(host).unwrap().active, None);
}

#[test]
fn removing_an_overlay_restores_focus_only_if_it_left_with_the_overlay() {
    // Focus on the menu, on an item inside it, on an editor elsewhere, or
    // cleared (0); the menu despawned (root first) or parked (focus dropped
    // first).
    for (focus, park) in [3, 5, 4, 0]
        .into_iter()
        .flat_map(|focus| [(focus, false), (focus, true)])
    {
        // No focus at all also gets the opener back: the world cannot tell
        // focus the user cleared from focus the framework dropped.
        let focus_in_menu = focus != 4;
        let mut world = UiWorld::new();
        let mut create = MutationQueue::new();
        create.create(node(1), document(1), NodeKind::Document);
        for (id, parent, tag) in [
            (2, 1, "div"),
            (3, 2, "button"),
            (4, 1, "textarea"),
            (5, 3, "button"),
        ] {
            create.create(node(id), document(1), NodeKind::Element { tag: tag.into() });
            create.insert(node(parent), node(id), None);
            create.set_interaction(
                node(id),
                InteractionState {
                    pointer_events: true,
                    focusable: true,
                },
            );
        }
        create.set_text_input(node(4), Some(TextInputState::new("value")));
        // Node 2 hosts a (non-modal) menu, node 3, opened from node 2.
        create.set_accessibility(
            node(3),
            AccessibilityState {
                role: AccessibilityRole::Menu,
                ..AccessibilityState::default()
            },
        );
        create.set_overlay_host(
            node(2),
            OverlayHostState {
                active: Some(node(3)),
                restore_focus: Some(node(2)),
            },
        );
        create.request_focus(document(1), (focus != 0).then(|| node(focus)));
        if focus == 4 {
            // The user went on typing elsewhere, mid-composition.
            create.set_ime(
                node(4),
                Some(ImeComposition {
                    text: "ni".into(),
                    selection: None,
                }),
            );
        }
        world.commit(create).unwrap();

        let mut close = MutationQueue::new();
        if park {
            close.park_subtree(node(3));
        } else {
            close.despawn_subtree(node(3));
        }
        world.commit(close).unwrap();
        if focus_in_menu {
            assert_eq!(
                world.focused(document(1)),
                Some(node(2)),
                "focus leaving with the menu (from {focus}, parked: {park}) returns to where it came from"
            );
        } else {
            assert_eq!(
                world.focused(document(1)),
                Some(node(4)),
                "the menu's host does not take focus back from the editor"
            );
            assert!(world.ime(node(4)).is_some(), "nor end its composition");
        }
        assert_eq!(world.text_input(node(4)).unwrap().value, "value");
    }
}

#[test]
fn focus_in_an_overlay_opened_from_a_closing_one_goes_back_to_the_opener() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    // Host 2 opens popover 3; its item 5 opened listbox 7, hosted by 6
    // outside the popover; the listbox's item 10 opened submenu 9, hosted
    // by 8 outside both.
    for (id, parent) in [
        (2, 1),
        (3, 2),
        (5, 3),
        (6, 1),
        (7, 6),
        (10, 7),
        (8, 1),
        (9, 8),
    ] {
        create.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
        create.insert(node(parent), node(id), None);
        create.set_interaction(
            node(id),
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
    }
    for id in [3, 7, 9] {
        create.set_accessibility(
            node(id),
            AccessibilityState {
                role: AccessibilityRole::Menu,
                ..AccessibilityState::default()
            },
        );
    }
    world.commit(create).unwrap();
    let mut open = MutationQueue::new();
    open.set_overlay_host(
        node(2),
        OverlayHostState {
            active: Some(node(3)),
            restore_focus: Some(node(2)),
        },
    );
    open.set_overlay_host(
        node(6),
        OverlayHostState {
            active: Some(node(7)),
            restore_focus: Some(node(5)),
        },
    );
    open.set_overlay_host(
        node(8),
        OverlayHostState {
            active: Some(node(9)),
            restore_focus: Some(node(10)),
        },
    );
    open.request_focus(document(1), Some(node(9)));
    world.commit(open).unwrap();

    let mut close = MutationQueue::new();
    close.despawn_subtree(node(3));
    world.commit(close).unwrap();
    assert_eq!(
        world.focused(document(1)),
        Some(node(2)),
        "focus two surfaces down left with the popover they were opened from"
    );
}

#[test]
fn focus_returning_from_a_nested_overlay_ends_the_composition_it_leaves() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    // Host 2 opens popover 3; its item 5 opened listbox 7 (hosted by 6),
    // whose filter field 11 the user is composing in.
    for (id, parent, tag) in [
        (2, 1, "div"),
        (3, 2, "div"),
        (5, 3, "div"),
        (6, 1, "div"),
        (7, 6, "div"),
        (11, 7, "textarea"),
    ] {
        create.create(node(id), document(1), NodeKind::Element { tag: tag.into() });
        create.insert(node(parent), node(id), None);
        create.set_interaction(
            node(id),
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
    }
    for id in [3, 7] {
        create.set_accessibility(
            node(id),
            AccessibilityState {
                role: AccessibilityRole::Menu,
                ..AccessibilityState::default()
            },
        );
    }
    create.set_text_input(node(11), Some(TextInputState::new("fi")));
    world.commit(create).unwrap();
    let mut open = MutationQueue::new();
    open.set_overlay_host(
        node(2),
        OverlayHostState {
            active: Some(node(3)),
            restore_focus: Some(node(2)),
        },
    );
    open.set_overlay_host(
        node(6),
        OverlayHostState {
            active: Some(node(7)),
            restore_focus: Some(node(5)),
        },
    );
    open.request_focus(document(1), Some(node(11)));
    open.set_ime(
        node(11),
        Some(ImeComposition {
            text: "lt".into(),
            selection: None,
        }),
    );
    world.commit(open).unwrap();
    assert!(world.ime(node(11)).is_some());

    let mut close = MutationQueue::new();
    close.despawn_subtree(node(3));
    world.commit(close).unwrap();
    assert_eq!(world.focused(document(1)), Some(node(2)));
    assert!(world.contains(node(11)), "the listbox is still there");
    assert_eq!(
        world.ime(node(11)),
        None,
        "focus left it, so its composition ended"
    );
}

#[test]
fn parked_subtree_leaves_every_document_projection_and_remounts_intact() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    create.create(node(3), document(1), NodeKind::Text);
    create.create(node(4), document(1), NodeKind::Document);
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    create.set_interaction(
        node(2),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    create.set_text_input(node(2), Some(TextInputState::new("value")));
    create.set_accessibility(
        node(2),
        AccessibilityState {
            role: AccessibilityRole::Dialog,
            label: Some(Arc::from("Action")),
            modal: true,
            ..AccessibilityState::default()
        },
    );
    create.set_overlay_host(
        node(1),
        OverlayHostState {
            active: Some(node(2)),
            restore_focus: Some(node(2)),
        },
    );
    create.request_focus(document(1), Some(node(2)));
    create.set_ime(
        node(2),
        Some(ImeComposition {
            text: "input".into(),
            selection: None,
        }),
    );
    create.capture_pointer(7, node(2));
    create.start_animation(AnimationSpec::new(
        crate::AnimationId::new(1).unwrap(),
        node(2),
        Duration::from_millis(10),
        Duration::from_millis(100),
        Duration::from_millis(10),
        Easing::Linear,
    ));
    world.commit(create).unwrap();
    world.take_system_work();

    let mut park = MutationQueue::new();
    park.park_subtree(node(1));
    world.commit(park).unwrap();
    assert_eq!(world.mount_state(node(1)), Some(MountState::Parked));
    assert_eq!(world.mount_state(node(2)), Some(MountState::Parked));
    assert_eq!(world.document_order(document(1)), vec![node(4)]);
    assert!(world.extract_nodes(&[node(1), node(2), node(3)]).is_empty());
    assert!(
        world
            .project_accessibility_nodes(&[node(1), node(2), node(3)])
            .is_empty()
    );
    assert!(world.event_route(node(2)).is_none());
    assert_eq!(world.focused(document(1)), None);
    assert_eq!(world.ime(node(2)), None);
    assert_eq!(world.pointer_capture(document(1), 7), None);
    assert_eq!(
        world.set_pointer_hover(document(1), 8, Some(node(2))),
        Err(UiWorldError::NotPointerInteractive(node(2)))
    );
    let mut refocus = MutationQueue::new();
    refocus.request_focus(document(1), Some(node(2)));
    assert_eq!(
        world.commit(refocus),
        Err(UiWorldError::NotFocusable(node(2)))
    );
    assert_eq!(world.next_animation_deadline(), None);
    assert_eq!(
        world.overlay_host(node(1)),
        Some(OverlayHostState::default())
    );
    let work = world.take_system_work();
    assert!(work.layout.is_empty());
    assert!(work.render_extraction.is_empty());
    assert_eq!(work.render_removals, vec![node(1), node(2), node(3)]);
    assert_eq!(work.accessibility_removals, vec![node(1), node(2), node(3)]);

    let mut remount = MutationQueue::new();
    remount.insert(node(4), node(1), None);
    world.commit(remount).unwrap();
    assert!(world.is_mounted(node(1)));
    assert!(world.is_mounted(node(2)));
    assert_eq!(world.node(node(2)).unwrap().parent, Some(node(1)));
    assert!(world.document_order(document(1)).contains(&node(3)));
    let work = world.take_system_work();
    assert!(work.render_extraction.contains(&node(1)));
    assert!(work.render_extraction.contains(&node(2)));
    assert!(work.accessibility.contains(&node(2)));
}

#[test]
fn only_the_top_reachable_modal_constrains_focus() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    for id in 1..=6 {
        create.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    create.insert(node(4), node(5), None);
    create.insert(node(5), node(6), None);
    for id in [node(3), node(6)] {
        create.set_interaction(
            id,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
    }
    for id in [node(2), node(5)] {
        create.set_accessibility(
            id,
            AccessibilityState {
                role: AccessibilityRole::Dialog,
                modal: true,
                ..AccessibilityState::default()
            },
        );
    }
    let mut lower = NodeStyle::default();
    Arc::make_mut(&mut lower.layout).z_index = Some(10);
    create.set_style(node(2), lower);
    let mut upper = NodeStyle::default();
    Arc::make_mut(&mut upper.layout).z_index = Some(20);
    create.set_style(node(5), upper);
    create.set_overlay_host(
        node(1),
        OverlayHostState {
            active: Some(node(2)),
            restore_focus: None,
        },
    );
    create.set_overlay_host(
        node(4),
        OverlayHostState {
            active: Some(node(5)),
            restore_focus: None,
        },
    );
    create.request_focus(document(1), Some(node(6)));
    world.commit(create).unwrap();
    assert_eq!(world.focused(document(1)), Some(node(6)));

    let mut lower_focus = MutationQueue::new();
    lower_focus.request_focus(document(1), Some(node(3)));
    assert_eq!(
        world.commit(lower_focus),
        Err(UiWorldError::NotFocusable(node(3)))
    );

    let mut park_upper = MutationQueue::new();
    park_upper.park_subtree(node(5));
    park_upper.request_focus(document(1), Some(node(3)));
    world.commit(park_upper).unwrap();
    assert_eq!(world.focused(document(1)), Some(node(3)));
}

#[test]
fn display_none_rejects_focus_from_staged_and_committed_styles() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "input".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.set_interaction(
        node(2),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    world.commit(create).unwrap();

    let mut hidden = NodeStyle::default();
    Arc::make_mut(&mut hidden.layout).display = Some(nana_ui_core::DisplaySpec::None);
    let mut hide_and_focus = MutationQueue::new();
    hide_and_focus.set_style(node(2), hidden.clone());
    hide_and_focus.request_focus(document(1), Some(node(2)));
    assert_eq!(
        world.commit(hide_and_focus),
        Err(UiWorldError::NotFocusable(node(2)))
    );
    assert_eq!(world.focused(document(1)), None);
    assert!(!matches!(
        world.node_style(node(2)).unwrap().layout.display,
        Some(nana_ui_core::DisplaySpec::None)
    ));

    let mut hide = MutationQueue::new();
    hide.set_style(node(2), hidden);
    world.commit(hide).unwrap();
    let mut focus = MutationQueue::new();
    focus.request_focus(document(1), Some(node(2)));
    assert_eq!(
        world.commit(focus),
        Err(UiWorldError::NotFocusable(node(2)))
    );
}

#[test]
fn display_none_is_omitted_from_document_extraction() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "panel".into(),
        },
    );
    create.insert(node(1), node(2), None);
    world.commit(create).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert!(
        world
            .extract_document(document(1))
            .iter()
            .any(|extracted| extracted.id == node(2))
    );

    let mut hidden = NodeStyle::default();
    Arc::make_mut(&mut hidden.layout).display = Some(nana_ui_core::DisplaySpec::None);
    let mut hide = MutationQueue::new();
    hide.set_style(node(2), hidden);
    world.commit(hide).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert!(
        world
            .extract_document(document(1))
            .iter()
            .all(|extracted| extracted.id != node(2))
    );
    let incremental = world.extract_nodes(&[node(2)]);
    assert_eq!(incremental.len(), 1);
    assert!(!incremental[0].style.visible);
}

#[test]
fn visibility_hidden_skips_extract_paint_and_hit_test() {
    use nana_ui_core::VisibilitySpec;

    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "panel".into(),
        },
    );
    create.create(
        node(3),
        document(1),
        NodeKind::Element {
            tag: "child".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    create.write_layout(node(2), box_at(10.0, 10.0, 50.0, 50.0));
    create.write_layout(node(3), box_at(5.0, 5.0, 30.0, 30.0));
    world.commit(create).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.rebuild_hit_test(document(1));
    assert!(
        world
            .extract_document(document(1))
            .iter()
            .any(|extracted| extracted.id == node(2))
    );
    assert!(
        world
            .hit_test_candidates(document(1), 20.0, 20.0)
            .contains(&node(2))
    );

    let mut hidden = NodeStyle::default();
    Arc::make_mut(&mut hidden.layout).paint.visibility = Some(VisibilitySpec::Hidden);
    let mut hide = MutationQueue::new();
    hide.set_style(node(2), hidden);
    world.commit(hide).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut dirty = work.input_hit_test.clone();
    dirty.extend(work.style.iter().copied());
    dirty.sort_unstable();
    dirty.dedup();
    assert!(world.rebuild_hit_test_scoped(document(1), &dirty));

    assert!(
        world
            .extract_document(document(1))
            .iter()
            .all(|extracted| extracted.id != node(2) && extracted.id != node(3))
    );
    let extracted = world.extract_nodes(&[node(2), node(3)]);
    assert_eq!(extracted.len(), 2);
    assert!(extracted.iter().all(|node| !node.style.visible));
    assert!(
        !world
            .hit_test_candidates(document(1), 20.0, 20.0)
            .contains(&node(2))
    );
    assert!(
        !world
            .hit_test_candidates(document(1), 15.0, 15.0)
            .contains(&node(3))
    );
}

#[test]
fn pointer_events_none_inherits_unless_child_is_explicit_auto() {
    use nana_ui_core::PointerEventsSpec;

    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "under".into(),
        },
    );
    create.create(
        node(3),
        document(1),
        NodeKind::Element {
            tag: "overlay".into(),
        },
    );
    create.create(
        node(4),
        document(1),
        NodeKind::Element {
            tag: "inherited".into(),
        },
    );
    create.create(
        node(5),
        document(1),
        NodeKind::Element {
            tag: "explicit-auto".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.insert(node(1), node(3), None);
    create.insert(node(3), node(4), None);
    create.insert(node(3), node(5), None);
    create.write_layout(node(2), box_at(0.0, 0.0, 80.0, 80.0));
    create.write_layout(node(3), box_at(0.0, 0.0, 80.0, 80.0));
    create.write_layout(node(4), box_at(10.0, 10.0, 20.0, 20.0));
    create.write_layout(node(5), box_at(40.0, 10.0, 20.0, 20.0));
    let mut auto = NodeStyle::default();
    Arc::make_mut(&mut auto.layout).pointer_events = Some(PointerEventsSpec::Auto);
    create.set_style(node(5), auto);
    world.commit(create).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.rebuild_hit_test(document(1));
    assert_eq!(world.hit_test(document(1), 50.0, 20.0), Some(node(5)));

    let mut none = NodeStyle::default();
    Arc::make_mut(&mut none.layout).pointer_events = Some(PointerEventsSpec::None);
    let mut skip = MutationQueue::new();
    skip.set_style(node(3), none);
    world.commit(skip).unwrap();
    let work = world.take_system_work();
    assert!(
        work.layout.is_empty(),
        "pointer-events is not a layout dirty"
    );
    assert!(work.input_hit_test.contains(&node(3)));
    assert!(work.input_hit_test.contains(&node(4)));
    world.resolve_styles(&work.style).unwrap();
    assert!(world.rebuild_hit_test_scoped(document(1), &work.input_hit_test));

    assert_ne!(world.hit_test(document(1), 50.0, 50.0), Some(node(3)));
    assert_eq!(
        world.hit_test(document(1), 15.0, 15.0),
        Some(node(2)),
        "unspecified child inherits none"
    );
    assert_eq!(
        world.hit_test(document(1), 50.0, 20.0),
        Some(node(5)),
        "explicit auto child is a target again"
    );
    assert_eq!(
        world.hit_test(document(1), 20.0, 50.0),
        Some(node(2)),
        "parent padding that is not on the auto child passes through"
    );
}

#[test]
fn cursor_inherits_and_explicit_child_value_wins() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "parent".into(),
        },
    );
    create.create(
        node(3),
        document(1),
        NodeKind::Element {
            tag: "inherited".into(),
        },
    );
    create.create(
        node(4),
        document(1),
        NodeKind::Element {
            tag: "explicit".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    create.insert(node(2), node(4), None);
    let mut parent = NodeStyle::default();
    Arc::make_mut(&mut parent.layout).cursor = Some(CursorSpec::Grab);
    create.set_style(node(2), parent);
    let mut explicit = NodeStyle::default();
    Arc::make_mut(&mut explicit.layout).cursor = Some(CursorSpec::Pointer);
    create.set_style(node(4), explicit);
    world.commit(create).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    assert_eq!(
        world.computed_style(node(1)).unwrap().cursor,
        CursorSpec::Default
    );
    assert_eq!(
        world.computed_style(node(2)).unwrap().cursor,
        CursorSpec::Grab
    );
    assert!(world.computed_style(node(2)).unwrap().cursor_specified);
    assert_eq!(
        world.computed_style(node(3)).unwrap().cursor,
        CursorSpec::Grab
    );
    assert_eq!(
        world.computed_style(node(4)).unwrap().cursor,
        CursorSpec::Pointer
    );

    let mut update = NodeStyle::default();
    Arc::make_mut(&mut update.layout).cursor = Some(CursorSpec::Move);
    let mut mutation = MutationQueue::new();
    mutation.set_style(node(2), update);
    world.commit(mutation).unwrap();
    let work = world.take_system_work();
    assert!(work.layout.is_empty(), "cursor changes do not relayout");
    assert!(
        work.input_hit_test.is_empty(),
        "cursor changes do not rebuild hit test"
    );
    assert!(
        work.render_extraction.is_empty(),
        "cursor changes do not rebuild scene nodes"
    );
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        world.computed_style(node(3)).unwrap().cursor,
        CursorSpec::Move
    );
}

#[test]
fn pointer_events_none_clears_hover_without_layout() {
    use nana_ui_core::PointerEventsSpec;

    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "panel".into(),
        },
    );
    create.create(
        node(3),
        document(1),
        NodeKind::Element {
            tag: "child".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    create.write_layout(node(2), box_at(0.0, 0.0, 40.0, 40.0));
    create.write_layout(node(3), box_at(4.0, 4.0, 16.0, 16.0));
    world.commit(create).unwrap();
    world.take_system_work();
    world
        .set_pointer_hover(document(1), 7, Some(node(3)))
        .unwrap();
    world.take_system_work();
    assert_eq!(world.pointer_hover(document(1), 7), Some(node(3)));

    let mut none = NodeStyle::default();
    Arc::make_mut(&mut none.layout).pointer_events = Some(PointerEventsSpec::None);
    let mut skip = MutationQueue::new();
    skip.set_style(node(2), none);
    world.commit(skip).unwrap();
    assert_eq!(world.pointer_hover(document(1), 7), None);
    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(3))),
        Err(UiWorldError::NotPointerInteractive(node(3)))
    );
    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(2))),
        Err(UiWorldError::NotPointerInteractive(node(2)))
    );
}

#[test]
fn overflow_auto_clips_descendant_hit_testing() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "port".into() },
    );
    queue.create(
        node(3),
        document(1),
        NodeKind::Element { tag: "item".into() },
    );
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.write_layout(node(2), box_at(0.0, 0.0, 100.0, 50.0));
    queue.write_layout(node(3), box_at(0.0, 80.0, 100.0, 20.0));
    queue.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_y: OverflowSpec::Auto,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    world.rebuild_hit_test(document(1));
    assert_ne!(world.hit_test(document(1), 10.0, 85.0), Some(node(3)));
    assert_eq!(world.hit_test(document(1), 10.0, 25.0), Some(node(2)));

    // The item ends at 100 in a 50px port: 50 is as far as it scrolls.
    let mut scroll = MutationQueue::new();
    scroll.set_scroll_offset(node(2), ScrollOffset { x: 0.0, y: 80.0 });
    world.commit(scroll).unwrap();
    assert_eq!(world.scroll_offset(node(2)).unwrap().y, 50.0);
    world.take_system_work();
    world.rebuild_hit_test(document(1));
    assert_eq!(world.hit_test(document(1), 10.0, 35.0), Some(node(3)));
    assert_eq!(world.layout_box(node(3)).unwrap().y, 80.0);
}

#[test]
fn overflow_x_hidden_does_not_clip_visible_y() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "port".into() },
    );
    queue.create(
        node(3),
        document(1),
        NodeKind::Element { tag: "item".into() },
    );
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.write_layout(node(2), box_at(0.0, 0.0, 100.0, 50.0));
    queue.write_layout(node(3), box_at(0.0, 80.0, 40.0, 20.0));
    queue.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_x: OverflowSpec::Hidden,
                overflow_y: OverflowSpec::Visible,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    world.rebuild_hit_test(document(1));
    assert_eq!(
        world.hit_test(document(1), 10.0, 85.0),
        Some(node(3)),
        "visible Y must not clip a descendant below the padding box"
    );

    let mut moved = MutationQueue::new();
    moved.write_layout(node(3), box_at(120.0, 10.0, 40.0, 20.0));
    world.commit(moved).unwrap();
    world.take_system_work();
    world.rebuild_hit_test(document(1));
    assert_ne!(
        world.hit_test(document(1), 130.0, 15.0),
        Some(node(3)),
        "hidden X must still clip a descendant to the right"
    );
}

#[test]
fn visibility_visible_child_unhides_inside_hidden_parent() {
    use nana_ui_core::VisibilitySpec;

    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "panel".into(),
        },
    );
    create.create(
        node(3),
        document(1),
        NodeKind::Element {
            tag: "child".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    create.write_layout(node(2), box_at(10.0, 10.0, 50.0, 50.0));
    create.write_layout(node(3), box_at(5.0, 5.0, 30.0, 30.0));
    let mut parent = NodeStyle::default();
    Arc::make_mut(&mut parent.layout).paint.visibility = Some(VisibilitySpec::Hidden);
    create.set_style(node(2), parent);
    let mut child = NodeStyle::default();
    Arc::make_mut(&mut child.layout).paint.visibility = Some(VisibilitySpec::Visible);
    create.set_style(node(3), child);
    world.commit(create).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.rebuild_hit_test(document(1));

    let parent_extracted = world.extract_nodes(&[node(2)]);
    assert_eq!(parent_extracted.len(), 1);
    assert!(!parent_extracted[0].style.visible);

    let child_extracted = world.extract_nodes(&[node(3)]);
    assert_eq!(child_extracted.len(), 1);
    assert!(child_extracted[0].style.visible);
    assert!(
        world
            .extract_document(document(1))
            .iter()
            .any(|extracted| extracted.id == node(3))
    );
    assert!(
        world
            .hit_test_candidates(document(1), 15.0, 15.0)
            .contains(&node(3))
    );
    assert!(
        !world
            .hit_test_candidates(document(1), 20.0, 20.0)
            .contains(&node(2))
    );
}

#[test]
fn focus_uses_authored_visibility_in_dirty_and_pending_styles() {
    use nana_ui_core::VisibilitySpec;

    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    for id in [node(2), node(3)] {
        create.create(
            id,
            document(1),
            NodeKind::Element {
                tag: "input".into(),
            },
        );
        create.set_interaction(
            id,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
        );
    }
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    world.commit(create).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    let mut hidden = NodeStyle::default();
    Arc::make_mut(&mut hidden.layout).paint.visibility = Some(VisibilitySpec::Hidden);
    let mut visible = NodeStyle::default();
    Arc::make_mut(&mut visible.layout).paint.visibility = Some(VisibilitySpec::Visible);
    let mut inherited_hidden = MutationQueue::new();
    inherited_hidden.set_style(node(2), hidden.clone());
    inherited_hidden.request_focus(document(1), Some(node(3)));
    assert_eq!(
        world.commit(inherited_hidden),
        Err(UiWorldError::NotFocusable(node(3)))
    );
    assert_eq!(world.focused(document(1)), None);

    // A descendant's explicit visibility wins even before computed styles run.
    let mut reveal_child = MutationQueue::new();
    reveal_child.set_style(node(2), hidden.clone());
    reveal_child.set_style(node(3), visible.clone());
    reveal_child.request_focus(document(1), Some(node(3)));
    world.commit(reveal_child).unwrap();
    assert_eq!(world.focused(document(1)), Some(node(3)));
    assert!(!world.is_overlay_reachable(node(2)));
    assert!(world.is_overlay_reachable(node(3)));
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert!(world.is_overlay_reachable(node(3)));

    let mut hide_child = MutationQueue::new();
    hide_child.request_focus(document(1), None);
    hide_child.set_style(node(3), NodeStyle::default());
    world.commit(hide_child).unwrap();
    assert!(!world.is_overlay_reachable(node(3)));
    let mut focus_hidden = MutationQueue::new();
    focus_hidden.request_focus(document(1), Some(node(3)));
    assert_eq!(
        world.commit(focus_hidden),
        Err(UiWorldError::NotFocusable(node(3)))
    );

    // Restoring the ancestor in the same transaction allows inherited focus.
    let mut reveal_parent = MutationQueue::new();
    reveal_parent.set_style(node(2), visible);
    reveal_parent.request_focus(document(1), Some(node(3)));
    world.commit(reveal_parent).unwrap();
    assert_eq!(world.focused(document(1)), Some(node(3)));
    assert!(world.is_overlay_reachable(node(3)));
}

#[test]
fn pointer_events_none_skips_hit_and_auto_child_punches_through() {
    use nana_ui_core::PointerEventsSpec;

    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "panel".into(),
        },
    );
    create.create(
        node(3),
        document(1),
        NodeKind::Element {
            tag: "child".into(),
        },
    );
    create.create(
        node(4),
        document(1),
        NodeKind::Element {
            tag: "inert".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.insert(node(2), node(3), None);
    create.insert(node(2), node(4), None);
    create.write_layout(node(2), box_at(10.0, 10.0, 50.0, 50.0));
    create.write_layout(node(3), box_at(15.0, 15.0, 20.0, 20.0));
    create.write_layout(node(4), box_at(40.0, 40.0, 10.0, 10.0));
    let mut parent = NodeStyle::default();
    Arc::make_mut(&mut parent.layout).pointer_events = Some(PointerEventsSpec::None);
    create.set_style(node(2), parent);
    let mut child = NodeStyle::default();
    Arc::make_mut(&mut child.layout).pointer_events = Some(PointerEventsSpec::Auto);
    create.set_style(node(3), child);
    world.commit(create).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.rebuild_hit_test(document(1));

    let painted = world.extract_nodes(&[node(2), node(3), node(4)]);
    assert_eq!(painted.len(), 3);
    assert!(painted.iter().all(|node| node.style.visible));
    assert!(
        world
            .extract_document(document(1))
            .iter()
            .any(|extracted| extracted.id == node(2)),
        "pointer-events:none must still paint"
    );

    assert_eq!(world.hit_test(document(1), 25.0, 25.0), Some(node(3)));
    assert!(
        !world
            .hit_test_candidates(document(1), 12.0, 12.0)
            .contains(&node(2))
    );
    assert!(
        !world
            .hit_test_candidates(document(1), 44.0, 44.0)
            .contains(&node(4)),
        "unset child must inherit none"
    );
}

#[test]
fn overlay_host_rejects_an_active_child_without_overlay_semantics() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "custom".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.set_overlay_host(
        node(1),
        OverlayHostState {
            active: Some(node(2)),
            restore_focus: None,
        },
    );

    assert_eq!(
        world.commit(create),
        Err(UiWorldError::InvalidOverlayHost(node(1)))
    );
    assert!(world.is_empty());
}

#[test]
fn overlay_host_rejects_a_non_modal_dialog_surface() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "dialog".into(),
        },
    );
    create.insert(node(1), node(2), None);
    create.set_accessibility(
        node(2),
        AccessibilityState {
            role: AccessibilityRole::Dialog,
            modal: false,
            ..AccessibilityState::default()
        },
    );
    create.set_overlay_host(
        node(1),
        OverlayHostState {
            active: Some(node(2)),
            restore_focus: None,
        },
    );

    assert_eq!(
        world.commit(create),
        Err(UiWorldError::InvalidOverlayHost(node(1)))
    );
    assert!(world.is_empty());
}

#[test]
fn inactive_nested_and_removed_modal_hosts_do_not_block_focus() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    for id in 10..=15 {
        create.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    create.insert(node(10), node(11), None);
    create.insert(node(10), node(12), None);
    create.insert(node(11), node(15), None);
    create.insert(node(12), node(13), None);
    create.insert(node(13), node(14), None);
    create.set_interaction(
        node(15),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    create.set_accessibility(
        node(14),
        AccessibilityState {
            role: AccessibilityRole::Dialog,
            modal: true,
            ..AccessibilityState::default()
        },
    );
    create.set_accessibility(
        node(11),
        AccessibilityState {
            role: AccessibilityRole::Menu,
            ..AccessibilityState::default()
        },
    );
    create.set_overlay_host(
        node(10),
        OverlayHostState {
            active: Some(node(11)),
            restore_focus: None,
        },
    );
    create.set_overlay_host(
        node(13),
        OverlayHostState {
            active: Some(node(14)),
            restore_focus: None,
        },
    );
    create.request_focus(document(1), Some(node(15)));
    world.commit(create).unwrap();
    assert_eq!(world.focused(document(1)), Some(node(15)));

    let mut remove = MutationQueue::new();
    remove.despawn_subtree(node(13));
    world.commit(remove).unwrap();
    assert_eq!(world.overlay_host(node(10)).unwrap().active, Some(node(11)));
    assert!(world.is_overlay_reachable(node(15)));
    assert!(!world.contains(node(13)));
    assert!(!world.contains(node(14)));
    let mut refocus = MutationQueue::new();
    refocus.request_focus(document(1), Some(node(15)));
    world.commit(refocus).unwrap();
    assert_eq!(world.focused(document(1)), Some(node(15)));
}

#[test]
fn planned_park_rejects_new_ime_and_remount_does_not_restore_preedit() {
    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(node(1), document(1), NodeKind::Document);
    create.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "input".into(),
        },
    );
    create.create(node(3), document(1), NodeKind::Document);
    create.insert(node(1), node(2), None);
    create.set_interaction(
        node(2),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    create.set_text_input(node(2), Some(TextInputState::new("value")));
    create.request_focus(document(1), Some(node(2)));
    world.commit(create).unwrap();

    let generation = world.generation();
    let mut invalid = MutationQueue::new();
    invalid.park_subtree(node(1));
    invalid.set_ime(
        node(2),
        Some(ImeComposition {
            text: "preedit".into(),
            selection: None,
        }),
    );
    assert_eq!(
        world.commit(invalid),
        Err(UiWorldError::NotFocused(node(2)))
    );
    assert_eq!(world.generation(), generation);
    assert!(world.is_mounted(node(2)));
    assert_eq!(world.focused(document(1)), Some(node(2)));
    assert_eq!(world.ime(node(2)), None);

    let mut park = MutationQueue::new();
    park.park_subtree(node(1));
    world.commit(park).unwrap();
    assert_eq!(world.focused(document(1)), None);
    assert_eq!(world.ime(node(2)), None);

    let mut remount = MutationQueue::new();
    remount.insert(node(3), node(1), None);
    world.commit(remount).unwrap();
    assert!(world.is_mounted(node(2)));
    assert_eq!(world.focused(document(1)), None);
    assert_eq!(world.ime(node(2)), None);
}

#[test]
fn invalid_batch_is_atomic() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    world.commit(queue).unwrap();

    let mut queue = MutationQueue::new();
    queue.create(node(2), document(1), NodeKind::Text);
    queue.insert(node(99), node(2), None);
    assert_eq!(
        world.commit(queue),
        Err(UiWorldError::MissingNode(node(99)))
    );
    assert!(!world.contains(node(2)));
    assert_eq!(world.len(), 1);

    let mut foreign = MutationQueue::new();
    foreign.create(node(2), document(2), NodeKind::Text);
    world.commit(foreign).unwrap();
    let generation = world.generation();
    let mut invalid_park = MutationQueue::new();
    invalid_park.park_subtree(node(1));
    invalid_park.insert(node(1), node(2), None);
    assert!(matches!(
        world.commit(invalid_park),
        Err(UiWorldError::CrossDocument { .. })
    ));
    assert_eq!(world.generation(), generation);
    assert_eq!(world.mount_state(node(1)), Some(MountState::Mounted));
    assert!(world.document_order(document(1)).contains(&node(1)));
}

#[test]
fn committed_text_selection_is_unicode_safe_and_batch_atomic() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "input".into(),
        },
    );
    queue.set_text_input(node(1), Some(TextInputState::new("你好ab")));
    queue.set_text_selection(node(1), crate::TextSelection::new(0, "你".len()));
    queue.replace_text_selection(node(1), "娜");
    world.commit(queue).unwrap();
    let state = world.text_input(node(1)).unwrap();
    assert_eq!(state.value, "娜好ab");
    assert_eq!(state.selection, crate::TextSelection::caret("娜".len()));

    let generation = world.generation();
    let mut invalid = MutationQueue::new();
    invalid.set_text_selection(node(1), crate::TextSelection::new(1, 1));
    assert_eq!(
        world.commit(invalid),
        Err(UiWorldError::InvalidTextInput(node(1)))
    );
    assert_eq!(world.generation(), generation);
    assert_eq!(world.text_input(node(1)).unwrap().value, "娜好ab");
    assert_eq!(world.text(node(1)), Some("娜好ab"));

    let accessibility = world.project_accessibility(document(1));
    assert_eq!(accessibility[0].value.as_deref(), Some("娜好ab"));
    assert!(world.extract_document(document(1))[0].editable);
    assert_eq!(
        world.text_input(node(1)).unwrap().selection,
        crate::TextSelection::caret("娜".len())
    );
}

#[test]
fn a_batch_validates_against_the_cursors_the_session_will_hold() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    // The second cursor sits inside "e\u{301}"; the session snaps it to 2.
    let mut state = TextInputState::new("abe\u{301}");
    state.selection = crate::TextSelection::caret(0);
    state.additional_selections = vec![crate::TextSelection::caret(3)];
    queue.set_text_input(node(1), Some(state));
    queue.replace_text_selection(node(1), "X");
    let end = "XabXe\u{301}".len();
    queue.set_text_selection(node(1), crate::TextSelection::caret(end));
    world.commit(queue).unwrap();
    let state = world.text_input(node(1)).unwrap();
    assert_eq!(state.value, "XabXe\u{301}");
    assert_eq!(state.selection, crate::TextSelection::caret(end));
}

#[test]
fn text_selection_and_ime_reject_partial_graphemes_atomically() {
    let value = "A👩‍💻e\u{301}";
    let emoji_interior = "A👩".len();
    let combining_interior = "A👩‍💻e".len();
    assert!(value.is_char_boundary(emoji_interior));
    assert!(value.is_char_boundary(combining_interior));
    assert!(!crate::TextSelection::caret(emoji_interior).is_valid_for(value));
    assert!(!crate::TextSelection::caret(combining_interior).is_valid_for(value));
    assert!(crate::TextSelection::caret("A👩‍💻".len()).is_valid_for(value));

    let mut world = UiWorld::new();
    let mut create = MutationQueue::new();
    create.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    create.set_text_input(node(1), Some(TextInputState::new(value)));
    create.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    world.commit(create).unwrap();

    let generation = world.generation();
    let mut invalid_selection = MutationQueue::new();
    invalid_selection.set_text_selection(node(1), crate::TextSelection::caret(emoji_interior));
    assert_eq!(
        world.commit(invalid_selection),
        Err(UiWorldError::InvalidTextInput(node(1)))
    );
    assert_eq!(world.generation(), generation);

    let mut focus = MutationQueue::new();
    focus.request_focus(document(1), Some(node(1)));
    world.commit(focus).unwrap();
    let generation = world.generation();
    let mut invalid_ime = MutationQueue::new();
    invalid_ime.set_ime(
        node(1),
        Some(ImeComposition {
            text: "e\u{301}".into(),
            selection: Some(("e".len(), "e".len())),
        }),
    );
    assert_eq!(
        world.commit(invalid_ime),
        Err(UiWorldError::InvalidIme(node(1)))
    );
    assert_eq!(world.generation(), generation);
    assert!(world.ime(node(1)).is_none());
}

#[test]
fn text_input_presentation_masks_graphemes_and_replaces_selection_with_preedit() {
    let value = "A👩‍💻界";
    let state = TextInputState {
        value: value.into(),
        selection: crate::TextSelection::new("A".len(), "A👩‍💻".len()),
        additional_selections: Vec::new(),
    };
    let masked = build_text_input_presentation_source(
        crate::TextInputView::of(&state.to_session()),
        None,
        "",
        true,
        false,
        TextInputEditorExtras::default(),
        false,
        None,
        None,
        None,
        None,
    );
    assert_eq!(masked.text.value, "•••");
    assert_eq!(masked.selection, Some(("•".len(), "••".len())));

    let preedit = build_text_input_presentation_source(
        crate::TextInputView::of(&state.to_session()),
        Some(
            ImeComposition {
                text: "输入".into(),
                selection: Some((0, "输".len())),
            }
            .view(),
        ),
        "",
        true,
        false,
        TextInputEditorExtras::default(),
        false,
        None,
        None,
        None,
        None,
    );
    assert_eq!(preedit.text.value, "•输入•");
    assert_eq!(preedit.preedit, Some(("•".len(), "•输入".len())));
    assert_eq!(preedit.caret, "•输".len());
}

#[test]
fn editor_extras_shape_and_derive_into_geometry() {
    let value = "甲乙\nthird\n末";
    let mut world = UiWorld::default();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([
                crate::TextDiagnosticSpan::new(
                    0,
                    "甲乙".len(),
                    crate::TextDiagnosticSeverity::Error,
                ),
                crate::TextDiagnosticSpan::new(
                    "甲乙\n".len(),
                    "third".len(),
                    crate::TextDiagnosticSeverity::Warning,
                ),
            ]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: true,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                height: Some(nana_ui_core::LengthSpec::Px(40.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 40.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();

    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 诊断切分到所在行并生成下划线条带（条带贴行底、高 2）。
    let line_height = presentation.line_height;
    assert_eq!(presentation.diagnostic_marks.len(), 2);
    assert_eq!(
        presentation.diagnostic_marks[0].severity,
        crate::TextDiagnosticSeverity::Error
    );
    assert_eq!(presentation.diagnostic_marks[0].rect.y, line_height - 2.0);
    assert_eq!(
        presentation.diagnostic_marks[1].severity,
        crate::TextDiagnosticSeverity::Warning
    );
    assert_eq!(presentation.diagnostic_marks[1].rect.y, 26.0);
    // 三个逻辑行的 y 起点。
    assert_eq!(presentation.line_tops, vec![0.0, 14.0, 28.0]);
    // 滚动查询：定位第 3 行（y=28）需要 scroll_y = 42 - 40 = 2。
    let scroll = world
        .text_input_reveal_scroll(node(1), "甲乙\nthird\n".len())
        .expect("reveal scroll");
    assert_eq!(scroll.y, 2.0);

    let mut scrolled = MutationQueue::new();
    scrolled.set_scroll_offset(node(1), scroll);
    world.commit(scrolled).unwrap();
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput {
        diagnostic_markers,
        ref line_labels,
        line_labels_color,
        ref text,
        ..
    } = geometry
    else {
        panic!("expected text input geometry");
    };
    assert_eq!(diagnostic_markers.len(), 2);
    assert_ne!(diagnostic_markers[0].1, diagnostic_markers[1].1);
    // 视口高 40、三行共 42px，reveal 把第 3 行推到视口底部。
    assert_eq!(line_labels.len(), 3);
    assert_eq!(line_labels[0].y, 40.0 - 42.0);
    assert_eq!(line_labels[1].y, 40.0 - 28.0);
    assert_eq!(line_labels[2].y, 40.0 - 14.0);
    assert_eq!(line_labels[0].number, 1);
    assert_eq!(line_labels_color[3], 1.0);
    // 文本区域随滚动整体上移两像素（scroll_y = 42 - 40）。
    assert_eq!(text.bounds.y, -2.0);
}

#[test]
fn diagnostic_labels_pick_line_winner_and_skip_hint_underline() {
    let value = "alpha\nbeta\n";
    let mut world = UiWorld::default();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([
                crate::TextDiagnosticSpan::new(0, 5, crate::TextDiagnosticSeverity::Error)
                    .with_message("bad ident"),
                crate::TextDiagnosticSpan::new(0, 2, crate::TextDiagnosticSeverity::Warning)
                    .with_message("also here"),
                crate::TextDiagnosticSpan::new(6, 4, crate::TextDiagnosticSeverity::Hint)
                    .with_message("hint text"),
            ]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: true,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                height: Some(nana_ui_core::LengthSpec::Px(40.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 40.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(
        presentation.diagnostic_marks.len(),
        2,
        "hint has no squiggle"
    );
    assert!(
        presentation
            .diagnostic_marks
            .iter()
            .all(|mark| mark.severity.draws_underline())
    );
    assert_eq!(presentation.diagnostic_labels.len(), 2);
    assert_eq!(presentation.diagnostic_labels[0].text, "bad ident");
    assert_eq!(
        presentation.diagnostic_labels[0].severity,
        crate::TextDiagnosticSeverity::Error
    );
    assert_eq!(presentation.diagnostic_labels[1].text, "hint text");
    assert_eq!(
        presentation.diagnostic_labels[1].severity,
        crate::TextDiagnosticSeverity::Hint
    );
    let hit = presentation
        .diagnostic_hits
        .iter()
        .find(|hit| hit.message == "bad ident")
        .expect("error hit");
    let hover = world
        .text_diagnostic_hit(node(1), hit.rect.x + 0.5, hit.rect.y + 0.5)
        .expect("hover");
    assert_eq!(hover.title, "bad ident");

    let mut hover_queue = MutationQueue::new();
    hover_queue.set_text_input_diagnostic_hover(node(1), Some(hover));
    world.commit(hover_queue).unwrap();
    world.take_system_work();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { hover_popup, .. } = geometry else {
        panic!("text input geometry");
    };
    assert_eq!(
        hover_popup
            .expect("diagnostic hover")
            .title
            .content
            .as_ref(),
        "bad ident"
    );

    let mut clear = MutationQueue::new();
    clear.set_text_input_diagnostic_hover(node(1), None);
    world.commit(clear).unwrap();
    world.take_system_work();
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { hover_popup, .. } = geometry else {
        panic!("text input geometry");
    };
    assert!(hover_popup.is_none());
}

/// 折叠测试编辑器："fn a() {\n    x();\n    y();\n}\nfn b() {}"。
/// 块折叠区间为 `{`（偏移 7）到 `}` 之后（28），隐藏三行
/// （两个语句行与 `}` 行）。
const FOLD_VALUE: &str = "fn a() {\n    x();\n    y();\n}\nfn b() {}";
const FOLD_BLOCK: crate::TextCodeFold = crate::TextCodeFold { start: 7, end: 28 };

/// world.rs 测试的折叠编辑器：多行 TextInput 视觉 + 可选行号与折叠
/// 区间，已布局。样式与布局随 `padding_left`/`line_numbers` 调整。
fn fold_editor_world(
    world: &mut UiWorld,
    folds: Arc<[crate::TextCodeFold]>,
    line_numbers: bool,
    padding_left: Option<f32>,
) {
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    let mut layout = nana_ui_core::LayoutStyle {
        height: Some(nana_ui_core::LengthSpec::Px(80.0)),
        width: Some(nana_ui_core::LengthSpec::Px(200.0)),
        font_size: Some(10.0),
        line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
        ..nana_ui_core::LayoutStyle::default()
    };
    if let Some(padding_left) = padding_left {
        layout.padding_left = Some(nana_ui_core::LengthSpec::Px(padding_left));
    }
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers,
            indent_guides: None,
            folds,
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(layout),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: FOLD_VALUE.into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 80.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
}

/// 内部派生渲染选项测试的编辑器 world：多行 TextInput 视觉 + 指定
/// 选项/值/选区/折叠，已布局（font 10、行高 14、200x80）。未 shape。
fn options_editor_world(
    world: &mut UiWorld,
    value: &str,
    selection: crate::TextSelection,
    folds: Arc<[crate::TextCodeFold]>,
    editor_options: crate::TextEditorRenderOptions,
    line_numbers: bool,
) {
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers,
            indent_guides: None,
            folds,
            git_marks: Arc::from([]),
            editor_options,
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                width: Some(nana_ui_core::LengthSpec::Px(200.0)),
                height: Some(nana_ui_core::LengthSpec::Px(80.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection,
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 80.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
}

/// sticky scroll 测试的编辑器 world：多行 TextInput + sticky 选项、
/// 指定值/折叠区间/滚动偏移，已布局（font 10、行高 14、200x80）。
/// 未 shape。
fn sticky_editor_world(
    world: &mut UiWorld,
    value: &str,
    folds: Arc<[crate::TextCodeFold]>,
    scroll_y: f32,
) {
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds,
            git_marks: Arc::from([]),
            editor_options: crate::TextEditorRenderOptions {
                sticky_scroll: true,
                ..Default::default()
            },
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                width: Some(nana_ui_core::LengthSpec::Px(200.0)),
                height: Some(nana_ui_core::LengthSpec::Px(80.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_scroll_offset(
        node(1),
        ScrollOffset {
            x: 0.0,
            y: scroll_y,
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 80.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
}

const STICKY_VALUE: &str = "fn outer() {\n    x();\n    fn inner() {\n        y();\n    }\n    z();\n}\n// tail\n// tail\n// tail\n";

/// 嵌套区间：外层从首行到 "// tail" 之前，内层完全落在外层体内。
fn sticky_folds() -> Arc<[crate::TextCodeFold]> {
    Arc::from([
        crate::TextCodeFold::new(0, STICKY_VALUE.find("\n// tail").unwrap()),
        crate::TextCodeFold::new(
            STICKY_VALUE.find("    fn inner").unwrap(),
            STICKY_VALUE.find("    z();").unwrap(),
        ),
    ])
}

#[test]
fn sticky_scroll_pins_innermost_region_head_at_content_top() {
    // 视口顶停在内层区间头行（显示行 2，top 28）之下、内层体结束行
    // 之上：钉最内层区间头（嵌套时最深者胜，外层头行被覆盖）。
    let mut world = UiWorld::default();
    sticky_editor_world(&mut world, STICKY_VALUE, sticky_folds(), 35.0);
    world
        .shape_text(&[node(1)], &mut FunctionalShaper::default())
        .unwrap();
    let crate::ComponentGeometry::TextInput { sticky_line, .. } =
        world.component_geometry(node(1)).expect("geometry")
    else {
        panic!("expected text input geometry");
    };
    let sticky = sticky_line.expect("sticky line");
    assert_eq!(sticky.text.content.as_ref(), "    fn inner() {");
    // 钉住行贴内容区顶：一整行高的不透明背景条 + 底缘 1px 分割线，
    // 覆盖滚动内容之上。
    assert_eq!(
        sticky.panel,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 14.0,
        }
    );
    assert_eq!(
        sticky.divider,
        LayoutBox {
            x: 0.0,
            y: 13.0,
            width: 200.0,
            height: 1.0,
        }
    );
    assert_eq!(sticky.background[3], 1.0);
}

#[test]
fn sticky_scroll_disappears_when_head_scrolls_back_or_feed_missing() {
    // 滚回顶部：区间头行自然可见，不钉。
    let mut world = UiWorld::default();
    sticky_editor_world(&mut world, STICKY_VALUE, sticky_folds(), 0.0);
    world
        .shape_text(&[node(1)], &mut FunctionalShaper::default())
        .unwrap();
    let crate::ComponentGeometry::TextInput { sticky_line, .. } =
        world.component_geometry(node(1)).expect("geometry")
    else {
        panic!("expected text input geometry");
    };
    assert!(sticky_line.is_none());

    // 选项开启但未喂折叠区间：永不出现。
    let mut world = UiWorld::default();
    sticky_editor_world(&mut world, STICKY_VALUE, Arc::from([]), 35.0);
    world
        .shape_text(&[node(1)], &mut FunctionalShaper::default())
        .unwrap();
    let crate::ComponentGeometry::TextInput { sticky_line, .. } =
        world.component_geometry(node(1)).expect("geometry")
    else {
        panic!("expected text input geometry");
    };
    assert!(sticky_line.is_none());
}

#[test]
fn sticky_scroll_pins_visible_head_after_a_collapsed_earlier_region() {
    // 前置区间处于折叠态时，其后方仍可见的函数头照常被钉住：显示偏移
    // 因前序折叠整体平移，但头行本身可见（只有落在隐藏区间内部的头行
    // 才跳过）。
    let value = format!(
        "{}{}",
        "fn a() {\n    p();\n    q();\n}\nfn b() {\n    r();\n    s();\n}\n",
        "// tail\n".repeat(8)
    );
    let fold_a = crate::TextCodeFold::new(0, value.find("fn b() {").unwrap());
    let fold_b = crate::TextCodeFold::new(
        value.find("fn b() {").unwrap(),
        value.find("\n// tail").unwrap(),
    );
    let mut world = UiWorld::default();
    sticky_editor_world(&mut world, &value, Arc::from([fold_a, fold_b]), 45.0);
    // 折叠前置区间 A：B 的头行显示偏移平移但仍然可见。
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([fold_a]));
    world.commit(queue).unwrap();
    world
        .shape_text(&[node(1)], &mut FunctionalShaper::default())
        .unwrap();
    let crate::ComponentGeometry::TextInput { sticky_line, .. } =
        world.component_geometry(node(1)).expect("geometry")
    else {
        panic!("expected text input geometry");
    };
    let sticky = sticky_line.expect("sticky line");
    // 折叠摘要 " …4" 与 B 的头行同属一个显示行（折叠吞掉中间换行），
    // 钉住行镜像该可见行全文——正是屏上 B 头行所在的那一行。
    assert_eq!(sticky.text.content.as_ref(), "fn a() { …4fn b() {");
}

/// git gutter 测试的编辑器 world：多行 TextInput 视觉 + 指定标记/行号/
/// 折叠/滚动/左内边距，已布局（font 10、行高 14、200x80）。未 shape。
fn git_gutter_editor_world(
    world: &mut UiWorld,
    value: &str,
    marks: Arc<[crate::TextGitMark]>,
    line_numbers: bool,
    scroll_y: f32,
    folds: Arc<[crate::TextCodeFold]>,
    padding_left: Option<f32>,
) {
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers,
            indent_guides: None,
            folds,
            git_marks: marks,
            editor_options: Default::default(),
        }),
    );
    let mut layout = nana_ui_core::LayoutStyle {
        width: Some(nana_ui_core::LengthSpec::Px(200.0)),
        height: Some(nana_ui_core::LengthSpec::Px(80.0)),
        font_size: Some(10.0),
        line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
        ..nana_ui_core::LayoutStyle::default()
    };
    if let Some(padding_left) = padding_left {
        layout.padding_left = Some(nana_ui_core::LengthSpec::Px(padding_left));
    }
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(layout),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    if scroll_y > 0.0 {
        queue.set_scroll_offset(
            node(1),
            ScrollOffset {
                x: 0.0,
                y: scroll_y,
            },
        );
    }
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 80.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
}

/// 取 TextInput 几何中的 git gutter 部分（其余调用方自行断言）。
fn git_geometry_of(world: &UiWorld) -> crate::TextGitGutterGeometry {
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { git_marks, .. } = geometry else {
        panic!("expected text input geometry");
    };
    git_marks
}

fn focus_editor(world: &mut UiWorld) {
    let mut focus = MutationQueue::new();
    focus.request_focus(document(1), Some(node(1)));
    world.commit(focus).unwrap();
}

#[test]
fn git_gutter_marks_render_three_kinds_at_their_line_tops() {
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        "甲乙\nthird\n末",
        Arc::from([
            crate::TextGitMark::new(1, crate::TextGitMarkKind::Added),
            crate::TextGitMark::new(2, crate::TextGitMarkKind::Modified),
            crate::TextGitMark::new(3, crate::TextGitMarkKind::Deleted),
        ]),
        true,
        0.0,
        Arc::from([]),
        None,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    // 文本空间：三类各自落在所在显示行的行顶，条高一行。
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.git_marks.len(), 3);
    let kinds = presentation
        .git_marks
        .iter()
        .map(|mark| mark.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            crate::TextGitMarkKind::Added,
            crate::TextGitMarkKind::Modified,
            crate::TextGitMarkKind::Deleted,
        ]
    );
    assert_eq!(presentation.git_marks[0].y, 0.0);
    assert_eq!(presentation.git_marks[1].y, 14.0);
    assert_eq!(presentation.git_marks[2].y, 28.0);
    assert!(
        presentation
            .git_marks
            .iter()
            .all(|mark| mark.height == 14.0)
    );

    // 节点空间：按种类分组，颜色取语义令牌（success/warning/danger，
    // 对应 git 惯例的绿/黄/红），竖条 2px 贴 gutter 最左缘。
    let git = git_geometry_of(&world);
    assert_eq!(git.added.len(), 1);
    assert_eq!(git.modified.len(), 1);
    assert_eq!(git.deleted.len(), 1);
    let palette = &world.style_model.palette;
    assert_eq!(git.added_color, palette.success.as_rgba_array());
    assert_eq!(git.modified_color, palette.warning.as_rgba_array());
    assert_eq!(git.deleted_color, palette.danger.as_rgba_array());
    assert_eq!(git.added[0].x, 0.0);
    assert_eq!(git.added[0].width, 2.0);
    assert_eq!(git.added[0].height, 14.0);
    assert_eq!(git.added[0].y, 0.0);
    assert_eq!(git.modified[0].y, 14.0);
    assert_eq!(git.deleted[0].y, 28.0);
}

#[test]
fn git_gutter_empty_marks_short_circuit_geometry_and_line_tops() {
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        "a\nb\nc",
        Arc::from([]),
        false,
        0.0,
        Arc::from([]),
        None,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.git_marks.is_empty());
    // 未开行号栏且无标记：行顶表不派生（零遍历短路）。
    assert!(presentation.line_tops.is_empty());
    let git = git_geometry_of(&world);
    assert!(git.added.is_empty());
    assert!(git.modified.is_empty());
    assert!(git.deleted.is_empty());
}

#[test]
fn git_gutter_skips_invalid_line_numbers() {
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        "a\nb\nc",
        Arc::from([
            crate::TextGitMark::new(0, crate::TextGitMarkKind::Added),
            crate::TextGitMark::new(2, crate::TextGitMarkKind::Modified),
            crate::TextGitMark::new(4, crate::TextGitMarkKind::Deleted),
            crate::TextGitMark::new(99, crate::TextGitMarkKind::Added),
        ]),
        false,
        0.0,
        Arc::from([]),
        None,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 行号 0 与超总行的标记静默跳过，仅有效行保留。
    assert_eq!(presentation.git_marks.len(), 1);
    assert_eq!(presentation.git_marks[0].y, 14.0);
    assert_eq!(
        presentation.git_marks[0].kind,
        crate::TextGitMarkKind::Modified
    );

    // 尾随换行不产生幻影行（与行号栏语义一致）：末空行上的标记无效。
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        "a\nb\n",
        Arc::from([
            crate::TextGitMark::new(3, crate::TextGitMarkKind::Added),
            crate::TextGitMark::new(2, crate::TextGitMarkKind::Deleted),
        ]),
        false,
        0.0,
        Arc::from([]),
        None,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.git_marks.len(), 1);
    assert_eq!(
        presentation.git_marks[0].kind,
        crate::TextGitMarkKind::Deleted
    );
}

#[test]
fn git_gutter_hides_marks_on_folded_lines_and_maps_visible_ones() {
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        FOLD_VALUE,
        Arc::from([
            crate::TextGitMark::new(1, crate::TextGitMarkKind::Added),
            crate::TextGitMark::new(3, crate::TextGitMarkKind::Modified),
            crate::TextGitMark::new(5, crate::TextGitMarkKind::Deleted),
        ]),
        false,
        0.0,
        Arc::from([FOLD_BLOCK]),
        None,
    );
    // 折叠隐藏 2-4 行（与既有折叠测试同一区间）。
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 行 3 被折叠隐藏：剔除；行 1、行 5 保留并映射到折叠后的显示行
    // （行 1 → 显示行 0，行 5 → 显示行 1）。
    assert_eq!(presentation.git_marks.len(), 2);
    assert_eq!(
        presentation.git_marks[0],
        crate::components::TextGitGutterMark {
            y: 0.0,
            height: 14.0,
            kind: crate::TextGitMarkKind::Added,
        }
    );
    assert_eq!(
        presentation.git_marks[1],
        crate::components::TextGitGutterMark {
            y: 14.0,
            height: 14.0,
            kind: crate::TextGitMarkKind::Deleted,
        }
    );
}

#[test]
fn git_gutter_clips_marks_to_the_viewport_when_scrolled() {
    let value = (0..10)
        .map(|index| format!("line{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let marks = (1..=10)
        .map(|line| crate::TextGitMark::new(line, crate::TextGitMarkKind::Added))
        .collect::<Arc<[crate::TextGitMark]>>();

    // 滚到底（60 = 10 行 × 14 − 80）：前 4 行滚出视口顶部不产生图元，
    // 行 5 只剩触边的部分，行 6-10 完整可见。
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        &value,
        Arc::clone(&marks),
        false,
        60.0,
        Arc::from([]),
        None,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let git = git_geometry_of(&world);
    assert_eq!(git.added.len(), 6);
    assert_eq!(git.added[0].y, -4.0);
    assert_eq!(git.added[5].y, 66.0);

    // 不滚动：视口底（80px）以下的行不产生图元。
    let mut world = UiWorld::default();
    git_gutter_editor_world(&mut world, &value, marks, false, 0.0, Arc::from([]), None);
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let git = git_geometry_of(&world);
    assert_eq!(git.added.len(), 6);
    assert_eq!(git.added[0].y, 0.0);
    assert_eq!(git.added[5].y, 70.0);
}

#[test]
fn git_gutter_coexists_with_line_numbers_and_fold_gutters() {
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        FOLD_VALUE,
        Arc::from([
            crate::TextGitMark::new(1, crate::TextGitMarkKind::Added),
            crate::TextGitMark::new(5, crate::TextGitMarkKind::Deleted),
        ]),
        true,
        0.0,
        Arc::from([FOLD_BLOCK]),
        Some(46.0),
    );
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 行号按折叠后的显示行派生（2 行），git 标记与其共享行顶表。
    assert_eq!(presentation.line_tops.len(), 2);
    assert_eq!(presentation.line_numbers, vec![1, 5]);
    assert_eq!(presentation.git_marks.len(), 2);

    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput {
        line_labels,
        folds,
        git_marks,
        ..
    } = geometry
    else {
        panic!("expected text input geometry");
    };
    assert_eq!(line_labels.len(), 2);
    assert_eq!(folds.gutters.len(), 1);
    // slot 与几何互不冲突：git 竖条占 gutter 最左 2px，折叠箭头从
    // border + 2px 起，行号标签右对齐于 padding 区。
    assert_eq!(git_marks.added[0].x + git_marks.added[0].width, 2.0);
    assert_eq!(folds.gutters[0].bounds.x, 2.0);
    assert_eq!(git_marks.added[0].y, folds.gutters[0].bounds.y);
}

#[test]
fn git_gutter_refeed_replaces_marks() {
    let mut world = UiWorld::default();
    git_gutter_editor_world(
        &mut world,
        "a\nb\nc",
        Arc::from([crate::TextGitMark::new(1, crate::TextGitMarkKind::Added)]),
        false,
        0.0,
        Arc::from([]),
        None,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    assert_eq!(git_geometry_of(&world).added.len(), 1);

    // 宿主重喂（git 状态推进）：新集合整组替换旧集合。
    let mut queue = MutationQueue::new();
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([
                crate::TextGitMark::new(2, crate::TextGitMarkKind::Modified),
                crate::TextGitMark::new(3, crate::TextGitMarkKind::Deleted),
            ]),
            editor_options: Default::default(),
        }),
    );
    world.commit(queue).unwrap();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let git = git_geometry_of(&world);
    assert!(git.added.is_empty());
    assert_eq!(git.modified.len(), 1);
    assert_eq!(git.modified[0].y, 14.0);
    assert_eq!(git.deleted.len(), 1);
    assert_eq!(git.deleted[0].y, 28.0);
}

#[test]
fn text_fold_display_view_substitutes_and_maps_offsets() {
    // 块折叠：起始行保留，隐藏三行（两个语句行与 `}` 行）替换为 ` …3`。
    let view = build_text_display_view(FOLD_VALUE, &[FOLD_BLOCK], &[]).unwrap();
    let marker = format!("{TEXT_FOLD_MARK_PREFIX}3");
    assert_eq!(view.value, format!("fn a() {{{marker}\nfn b() {{}}"));
    assert_eq!(view.spans.len(), 1);
    let span = &view.spans[0];
    assert_eq!(span.value_start, 8);
    assert_eq!(span.value_end, 28);
    assert!(matches!(
        &span.kind,
        TextDisplaySpanKind::Fold { hidden_lines, .. } if *hidden_lines == 3
    ));

    // 值↔显示双向映射：隐藏区间内部钳到折叠起始行行尾。
    assert_eq!(view.display_of(0), 0);
    assert_eq!(view.display_of(8), 8);
    assert_eq!(view.display_of(15), 8);
    assert_eq!(view.display_of(27), 8);
    assert_eq!(view.display_of(28), 8 + marker.len());
    assert_eq!(view.display_of(29), 8 + marker.len() + 1);
    assert_eq!(view.value_of(8), 8);
    assert_eq!(view.value_of(8 + marker.len() - 1), 8);
    assert_eq!(view.value_of(8 + marker.len()), 28);
    let span = &view.spans[0];
    assert!(view.span_hides(span, 15));
    assert!(!view.span_hides(span, 8));
    assert!(view.span_hides(span, 27));
    assert!(!view.span_hides(span, 28));

    // 嵌套折叠：子折叠的隐藏范围完全落在父折叠内，跳过不重复隐藏。
    let nested = build_text_display_view(
        FOLD_VALUE,
        &[FOLD_BLOCK, crate::TextCodeFold::new(12, 20)],
        &[],
    )
    .unwrap();
    assert_eq!(nested.spans.len(), 1);
    assert_eq!(nested.value, view.value);

    // 单行区间没有可隐藏的行：不可折叠。
    let single_line = crate::TextCodeFold::new(29, FOLD_VALUE.len());
    assert!(!single_line.collapsible_in(FOLD_VALUE));
    assert!(build_text_display_view(FOLD_VALUE, &[single_line], &[]).is_none());
}

/// 世界校验四例：非 char boundary 锚点、空文本、含 `'\n'`、越界锚点
/// 一律钳除；乱序输入排序、重复条目去重（钳除风格，同折叠先例）。
#[test]
fn inlay_world_validation_clamps_invalid_entries_and_normalizes() {
    let value = "héllo wörld";
    let normalized = normalize_text_inlays(
        value,
        &[
            crate::TextInlay::new(2, "非边界:"),
            crate::TextInlay::new(4, ""),
            crate::TextInlay::new(6, "a\nb"),
            crate::TextInlay::new(value.len() + 5, "越界:"),
            crate::TextInlay::new(6, "t:"),
            crate::TextInlay::new(6, "t:"),
            crate::TextInlay::new(0, "z:"),
        ],
    );
    assert_eq!(
        normalized,
        vec![
            crate::TextInlay::new(0, "z:"),
            crate::TextInlay::new(6, "t:")
        ]
    );
    // 合法条目原样保留；空输入零产出。
    assert_eq!(
        normalize_text_inlays(value, &[crate::TextInlay::new(4, "x:")]),
        vec![crate::TextInlay::new(4, "x:")]
    );
    assert!(normalize_text_inlays(value, &[]).is_empty());
}

/// 插入型显示 span：插入文本物化进显示串并参与布局；值↔显示映射在
/// inlay 两侧互逆（offset 免疫），插入区间内部钳到锚点（无 caret 边界，
/// 点击穿透吸附锚点字符）。
#[test]
fn inlay_display_view_inserts_and_maps_offsets() {
    let value = "f(x, y)";
    let view = build_text_display_view(
        value,
        &[],
        &[
            crate::TextInlay::new(2, "a:"),
            crate::TextInlay::new(5, "b:"),
        ],
    )
    .unwrap();
    assert_eq!(view.value, "f(a:x, b:y)");
    assert_eq!(view.spans.len(), 2);
    assert!(matches!(view.spans[0].kind, TextDisplaySpanKind::Inlay));
    assert!(matches!(view.spans[1].kind, TextDisplaySpanKind::Inlay));
    // 锚点映射到插入文本起点（插入文本渲染在锚点字符之前）；锚点之后
    // 的偏移平移插入长度。
    assert_eq!(view.display_of(0), 0);
    assert_eq!(view.display_of(2), 2);
    assert_eq!(view.display_of(3), 5);
    assert_eq!(view.display_of(5), 7);
    assert_eq!(view.display_of(6), 10);
    // 插入区间内部（含内部任意点）钳到锚点；右边界即锚点。
    assert_eq!(view.value_of(2), 2);
    assert_eq!(view.value_of(3), 2);
    assert_eq!(view.value_of(4), 2);
    assert_eq!(view.value_of(8), 5);
    assert_eq!(view.value_of(9), 5);
    // 往返互逆：光标/点击在 inlay 右侧行为不变（映射自动免疫）。
    for offset in [0, 2, 3, 5, 6, value.len()] {
        assert_eq!(view.value_of(view.display_of(offset)), offset);
    }
    // inlay 片段没有隐藏区间。
    assert!(!view.span_hides(&view.spans[0], 2));
}

/// 折叠隐藏区间内的 inlay 锚点（锚点字符被摘要替换）丢弃；折叠前后的
/// 锚点保留且显示位置平移正确（照抄 map_span 的隐藏丢弃先例）。
#[test]
fn inlays_inside_folded_regions_are_dropped() {
    let marker = format!("{TEXT_FOLD_MARK_PREFIX}3");
    let view = build_text_display_view(
        FOLD_VALUE,
        &[FOLD_BLOCK],
        &[
            crate::TextInlay::new(15, "x:"),
            crate::TextInlay::new(4, "p:"),
            crate::TextInlay::new(28, "q:"),
        ],
    )
    .unwrap();
    assert_eq!(view.value, format!("fn ap:() {{{marker}q:\nfn b() {{}}"));
    assert_eq!(view.spans.len(), 3);
    // 锚点 15 落在折叠隐藏区间 [8,28) 内：不出现。
    assert!(view.spans.iter().all(|span| span.value_start != 15));
    assert!(matches!(view.spans[0].kind, TextDisplaySpanKind::Inlay));
    assert!(matches!(
        view.spans[1].kind,
        TextDisplaySpanKind::Fold { .. }
    ));
    assert!(matches!(view.spans[2].kind, TextDisplaySpanKind::Inlay));
    // 折叠后锚点的插入位置在摘要之后。
    let fold_span = &view.spans[1];
    assert_eq!(
        view.spans[2].display_start,
        fold_span.display_start + fold_span.display_len
    );
}

/// 世界侧喂入：SetTextInputInlays 经世界校验（钳除 + 排序去重）后进
/// 显示视图；IME 组合期整体退场（同 swatch 先例），组合结束恢复；空表
/// 撤除条目。
#[test]
fn a_component_selection_set_normalizes_the_way_the_session_does() {
    // What a component holds and what the world's session holds are
    // compared on every projection; if they normalize differently, every
    // projection resends the whole state.
    let value = "a👨‍👩‍👧b cd";
    let family = "a".len().."a👨‍👩‍👧".len();
    let mut component = TextInputState {
        value: value.into(),
        selection: crate::TextSelection::caret(0),
        additional_selections: vec![
            crate::TextSelection::caret(value.len()),
            // Inside the family emoji: off a grapheme boundary.
            crate::TextSelection::caret(family.start + 4),
        ],
    };
    // The primary jumps onto another cursor, as bracket matching can.
    component.selection = crate::TextSelection::caret(value.len());
    component.normalize_selections();

    let mut session = TextInputState {
        value: value.into(),
        selection: crate::TextSelection::caret(0),
        additional_selections: vec![
            crate::TextSelection::caret(value.len()),
            crate::TextSelection::caret(family.start + 4),
        ],
    }
    .to_session();
    session.set_primary_selection(crate::TextSelection::caret(value.len()));

    let view = crate::TextInputView::of(&session);
    assert!(view == component, "{view:?} vs {component:?}");
    assert_eq!(
        component.selection,
        crate::TextSelection::caret(value.len())
    );
    assert_eq!(
        component.additional_selections,
        vec![crate::TextSelection::caret(family.start)],
        "the cursor inside the cluster snapped to its start; the one under the primary fused"
    );
}

#[test]
fn a_new_inlay_feed_is_never_answered_from_the_memoized_view() {
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([]), false, None);
    // Feeds of one size, fed and dropped in turn: a freed feed's address is
    // the likeliest one for the next, and the memoized view must not take
    // the new feed for the old.
    for (round, label) in ["a:", "b:", "c:", "d:"].into_iter().enumerate() {
        let mut queue = MutationQueue::new();
        queue.set_text_input_inlays(node(1), Arc::from([crate::TextInlay::new(4, label)]));
        world.commit(queue).unwrap();
        let view = world.text_display_view(node(1)).expect("inlay view");
        assert!(
            view.value.contains(label),
            "round {round}: the view shows the feed it was asked about"
        );
        let mut queue = MutationQueue::new();
        queue.set_text_input_inlays(node(1), Arc::from([]));
        world.commit(queue).unwrap();
        assert!(world.text_display_view(node(1)).is_none());
    }
}

#[test]
fn an_empty_preedit_is_not_a_composition_for_the_display() {
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_highlight_request(node(1), Some(crate::HighlightRequest::highlight("rs")));
    queue.set_text_input_inlays(node(1), Arc::from([crate::TextInlay::new(4, "p:")]));
    queue.request_focus(document(1), Some(node(1)));
    world.commit(queue).unwrap();
    world.resolve_presentations(&[node(1)]).unwrap();
    let highlighted = world.extract_nodes(&[node(1)])[0].text_spans.clone();
    assert!(!highlighted.is_empty());
    let mut queue = MutationQueue::new();
    // A platform reporting "nothing composed" between keystrokes.
    queue.set_ime(
        node(1),
        Some(ImeComposition {
            text: String::new(),
            selection: None,
        }),
    );
    world.commit(queue).unwrap();
    assert!(
        world.ime(node(1)).is_some(),
        "the IME is still attached: a submit waits for it"
    );
    let view = world
        .text_display_view(node(1))
        .expect("inlays stay while nothing is composed");
    assert!(view.value.contains("p:"));
    world.resolve_presentations(&[node(1)]).unwrap();
    assert_eq!(
        world.extract_nodes(&[node(1)])[0].text_spans,
        highlighted,
        "and so do the highlights"
    );
}

#[test]
fn inlay_feeds_validate_in_world_and_retire_during_ime() {
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_text_input_inlays(
        node(1),
        Arc::from([
            crate::TextInlay::new(FOLD_VALUE.len() + 5, "越界:"),
            crate::TextInlay::new(32, "p:"),
            crate::TextInlay::new(4, "p:"),
            crate::TextInlay::new(4, "p:"),
        ]),
    );
    world.commit(queue).unwrap();
    let view = world.text_display_view(node(1)).expect("inlay view");
    // 越界钳除、重复去重：只剩锚点 4 与 32 两条；插入文本进显示串。
    assert_eq!(view.spans.len(), 2);
    assert_eq!(view.value, "fn ap:() {\n    x();\n    y();\n}\nfn p:b() {}");

    // IME 组合期：inlay 整体退场（组合拼接改变字节布局，锚点漂移），
    // 显示视图回到原值。
    let mut queue = MutationQueue::new();
    queue.request_focus(document(1), Some(node(1)));
    queue.set_ime(
        node(1),
        Some(ImeComposition {
            text: "拼".into(),
            selection: None,
        }),
    );
    world.commit(queue).unwrap();
    assert!(
        world.text_display_view(node(1)).is_none(),
        "组合期 inlay 退场"
    );

    // 组合结束恢复。
    let mut queue = MutationQueue::new();
    queue.set_ime(node(1), None);
    world.commit(queue).unwrap();
    assert!(world.text_display_view(node(1)).is_some(), "组合结束恢复");

    // 空表撤除条目（零分配待机）。
    let mut queue = MutationQueue::new();
    queue.set_text_input_inlays(node(1), Arc::from([]));
    world.commit(queue).unwrap();
    assert!(world.text_display_view(node(1)).is_none(), "空表撤除");
}

/// 提取层：inlay 显示区间整段 Muted 灰字（同字号），值空间语法 span
/// 经重映射后不覆盖插入文本。
#[test]
fn inlay_display_regions_get_muted_spans_in_extraction() {
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_highlight_request(node(1), Some(crate::HighlightRequest::highlight("rs")));
    queue.set_text_input_inlays(node(1), Arc::from([crate::TextInlay::new(4, "p:")]));
    world.commit(queue).unwrap();
    world.resolve_presentations(&[node(1)]).unwrap();
    let extracted = &world.extract_nodes(&[node(1)])[0];
    let muted = world
        .style_model
        .color(SemanticColorRole::Muted)
        .as_rgba_array();
    let inlay_span = extracted
        .text_spans
        .iter()
        .find(|span| span.start == 4 && span.end == 6)
        .expect("inlay region colored");
    assert_eq!(inlay_span.color, muted, "inlay 整段 Muted 灰字");
    // 其余 span 不与 inlay 区间重叠（重映射起点越过插入文本）。
    assert!(
        extracted
            .text_spans
            .iter()
            .filter(|span| !(span.start == 4 && span.end == 6 && span.color == muted))
            .all(|span| span.end <= 4 || span.start >= 6),
        "值空间 span 不覆盖插入文本"
    );
}

#[test]
fn fold_state_survives_host_refeed_and_shift_rescue() {
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([FOLD_BLOCK]), false, None);
    assert!(world.text_fold_collapsed(node(1)).is_empty());

    // 折叠：只有喂入区间之内的条目才被接受。
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    assert_eq!(world.text_fold_collapsed(node(1)), vec![FOLD_BLOCK]);
    let view = world.text_display_view(node(1)).unwrap();
    assert_eq!(view.spans.len(), 1);

    // 上方插入 100 字节：值编辑平移折叠，宿主重喂平移后的区间——
    // 折叠态保留。
    let shifted_value = format!("{}{}", "a".repeat(100), FOLD_VALUE);
    let mut queue = MutationQueue::new();
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: shifted_value.clone().into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    assert_eq!(
        world.text_fold_collapsed(node(1)),
        vec![crate::TextCodeFold::new(107, 128)]
    );
    let mut queue = MutationQueue::new();
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: Arc::from([crate::TextCodeFold::new(107, 128)]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    world.commit(queue).unwrap();
    assert_eq!(
        world.text_fold_collapsed(node(1)),
        vec![crate::TextCodeFold::new(107, 128)]
    );

    // 宿主不再喂入折叠：视图状态整个移除（全部展开）。
    let mut queue = MutationQueue::new();
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    world.commit(queue).unwrap();
    assert!(world.text_fold_collapsed(node(1)).is_empty());
    assert!(world.text_display_view(node(1)).is_none());
}

#[test]
fn fold_unfolds_when_edited_inside_and_shifts_after_edit() {
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([FOLD_BLOCK]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();

    // 折叠之后的追加编辑：折叠按自身位移规则保持（编辑区间在其后，
    // 折叠整体在编辑区间之前，偏移不受影响）。
    let longer = format!("{}{}", FOLD_VALUE, "\n// tail");
    let mut queue = MutationQueue::new();
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: longer.into(),
            selection: crate::TextSelection::caret(FOLD_VALUE.len()),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    assert_eq!(world.text_fold_collapsed(node(1)), vec![FOLD_BLOCK]);

    // 被编辑区间与折叠相交 → 自动展开。
    let mut queue = MutationQueue::new();
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: format!("{}{}", &FOLD_VALUE[..15], &FOLD_VALUE[20..]).into(),
            selection: crate::TextSelection::caret(15),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    assert!(world.text_fold_collapsed(node(1)).is_empty());
}

#[test]
fn text_input_edits_apply_without_fold_or_snippet_view_state() {
    // 无折叠、无 snippet 的普通输入：值变更不依赖视图状态重映射，
    // 连续编辑后运行时组件与节点文本保持同步，且不会凭空生成视图状态。
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([]), false, None);
    assert!(world.text_fold_view_state(node(1)).is_none());
    assert!(world.text_snippet_session(node(1)).is_none());

    let mut queue = MutationQueue::new();
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: "typed".into(),
            selection: crate::TextSelection::caret(5),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    assert_eq!(world.record(node(1)).text.value, "typed");
    assert_eq!(world.nodes.text_input(node(1)).unwrap().value, "typed");

    // 二次编辑覆盖旧值，再清空输入：文本与视图状态依旧一致。
    let mut queue = MutationQueue::new();
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: "typed more".into(),
            selection: crate::TextSelection::caret(10),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    assert_eq!(world.record(node(1)).text.value, "typed more");
    let mut queue = MutationQueue::new();
    queue.set_text_input(node(1), None);
    world.commit(queue).unwrap();
    assert_eq!(world.record(node(1)).text.value, "");
    assert!(world.text_fold_view_state(node(1)).is_none());
    assert!(world.text_snippet_session(node(1)).is_none());
}

#[test]
fn text_input_edit_shifts_snippet_stops_outside_and_ends_session_inside() {
    // snippet 会话：跳位之外的编辑按最小变更区间平移跳位，会话保持；
    // 覆盖跳位的编辑使会话失效结束。
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_text_input_snippet(
        node(1),
        Some(crate::components::TextSnippetSession {
            exit_on_last: false,
            placeholders: Vec::new(),
            stops: vec![10, 20],
            selection_ends: Vec::new(),
            index: 0,
        }),
    );
    world.commit(queue).unwrap();

    // 开头插入 "// hi\n"：两个跳位都在编辑区间之后，整体 +6 平移。
    let mut queue = MutationQueue::new();
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: format!("// hi\n{FOLD_VALUE}").into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    assert_eq!(
        world.text_snippet_session(node(1)),
        Some(crate::components::TextSnippetSession {
            exit_on_last: false,
            placeholders: Vec::new(),
            stops: vec![16, 26],
            selection_ends: Vec::new(),
            index: 0,
        })
    );

    // 整值替换吞掉两个跳位：会话结束。
    let mut queue = MutationQueue::new();
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: "x".into(),
            selection: crate::TextSelection::caret(1),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    assert!(world.text_snippet_session(node(1)).is_none());
}

#[test]
fn code_fold_presentation_hides_lines_and_keeps_line_numbers() {
    // 嵌套折叠：宿主同时喂入父折叠与子折叠。
    let child = crate::TextCodeFold::new(12, 20);
    let mut world = UiWorld::default();
    fold_editor_world(&mut world, Arc::from([FOLD_BLOCK, child]), true, Some(46.0));
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();

    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    let marker = format!("{TEXT_FOLD_MARK_PREFIX}3");
    assert_eq!(
        presentation.display_value,
        format!("fn a() {{{marker}\nfn b() {{}}")
    );
    // 隐藏行不再产生行 top；行号保留原始编号。
    assert_eq!(presentation.line_tops.len(), 2);
    assert_eq!(presentation.line_numbers, vec![1, 5]);
    // 摘要标记携带折叠区间，供几何层生成点击命中框。
    assert_eq!(presentation.fold_marks.len(), 1);
    assert_eq!(presentation.fold_marks[0].fold, FOLD_BLOCK);

    // 几何层：gutter 箭头 + 摘要标记命中框，行号标签使用原始编号。
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput {
        folds, line_labels, ..
    } = &geometry
    else {
        panic!("text input geometry");
    };
    assert_eq!(folds.gutters.len(), 1);
    assert!(folds.gutters[0].collapsed);
    assert_eq!(folds.gutters[0].fold, FOLD_BLOCK);
    // 展开态下两个折叠各有一个箭头（子折叠起始行可见）。
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([]));
    world.commit(queue).unwrap();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput {
        folds: expanded, ..
    } = &geometry
    else {
        panic!("text input geometry");
    };
    assert_eq!(expanded.gutters.len(), 2);
    assert_eq!(folds.markers.len(), 1);
    assert_eq!(folds.markers[0].fold, FOLD_BLOCK);
    assert_eq!(
        line_labels.iter().map(|l| l.number).collect::<Vec<_>>(),
        vec![1, 5]
    );

    // gutter 命中测试返回折叠区间。
    let gutter = folds.gutters[0].bounds;
    assert_eq!(
        world.text_fold_hit(node(1), gutter.x + 1.0, gutter.y + 1.0),
        Some(FOLD_BLOCK)
    );

    // 展开后回到完整文本与 5 行。
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([]));
    world.commit(queue).unwrap();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world.text_input_presentation(node(1)).unwrap();
    assert_eq!(presentation.display_value, FOLD_VALUE);
    assert_eq!(presentation.line_tops.len(), 5);
}

#[test]
fn match_spans_shape_into_highlights_and_derive_into_geometry() {
    let value = "甲乙\nthird\n末";
    let mut world = UiWorld::default();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([
                crate::TextMatchSpan::new(0, "甲乙".len()),
                crate::TextMatchSpan::new("甲乙\n".len(), "third".len()).current(),
            ]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                height: Some(nana_ui_core::LengthSpec::Px(60.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 60.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();

    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 匹配高亮是整行高条带（区别于 2px 诊断下划线），当前匹配带标记。
    assert_eq!(presentation.match_marks.len(), 2);
    assert!(!presentation.match_marks[0].current);
    assert_eq!(
        presentation.match_marks[0].rect,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 20.0,
            height: 14.0,
        }
    );
    assert!(presentation.match_marks[1].current);
    assert_eq!(presentation.match_marks[1].rect.y, 14.0);

    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { match_markers, .. } = geometry else {
        panic!("expected text input geometry");
    };
    assert_eq!(match_markers.len(), 2);
    assert!(!match_markers[0].current);
    assert!(match_markers[1].current);
    // 当前匹配用更强的 accent 强调色，普通匹配用 accent 软色令牌。
    assert_ne!(match_markers[0].color, match_markers[1].color);
    assert_eq!(match_markers[0].color[3], 0.20);
    assert_eq!(match_markers[1].color[3], 0.45);
}

#[test]
fn atom_spans_cover_token_glyphs_with_label_and_close_hit() {
    let token = "[文件引用: note.txt | /tmp/note.txt]";
    let value = format!("see {token} end");
    let start = value.find(token).unwrap();
    let end = start + token.len();
    let mut world = UiWorld::default();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([crate::TextAtomSpan::new(start, end)
                .label("note.txt")
                .token("file-1")]),
            line_numbers: false,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                height: Some(nana_ui_core::LengthSpec::Px(40.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.clone().into(),
            selection: crate::TextSelection::caret(end),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 400.0,
            height: 40.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.atom_chips.len(), 1);
    assert_eq!(
        presentation.atom_chips[0].label.content.as_ref(),
        "note.txt"
    );
    assert_ne!(presentation.atom_chips[0].label.content.as_ref(), token);
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { atom_chips, .. } = geometry else {
        panic!("expected text input geometry");
    };
    assert_eq!(atom_chips.len(), 1);
    assert_eq!(atom_chips[0].label.content.as_ref(), "note.txt");
    let close = atom_chips[0].close;
    assert_eq!(
        world
            .text_atom_close_hit(node(1), close.x + 1.0, close.y + 1.0)
            .as_deref(),
        Some("file-1")
    );
}

/// 颜色装饰 swatch 测试的编辑器 world：多行 TextInput 视觉 + 指定
/// swatch span 与滚动偏移，已布局（font 10、行高 14、200x60）。
fn swatch_editor_world(
    world: &mut UiWorld,
    create: bool,
    swatches: Arc<[crate::TextColorSwatchSpan]>,
    scroll_y: f32,
) {
    let mut queue = MutationQueue::new();
    if create {
        queue.create(
            node(1),
            document(1),
            NodeKind::Element {
                tag: "textarea".into(),
            },
        );
    }
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: swatches,
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                width: Some(nana_ui_core::LengthSpec::Px(200.0)),
                height: Some(nana_ui_core::LengthSpec::Px(60.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: "甲乙\nthird\n末\nfour\nfifth\nsix".into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    if scroll_y > 0.0 {
        queue.set_scroll_offset(
            node(1),
            ScrollOffset {
                x: 0.0,
                y: scroll_y,
            },
        );
    }
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 60.0,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
}

#[test]
fn color_swatches_derive_into_inline_marks_and_clear_with_feed() {
    let mut world = UiWorld::default();
    swatch_editor_world(
        &mut world,
        true,
        Arc::from([
            crate::TextColorSwatchSpan::new(0, "甲乙".len(), [0.9, 0.2, 0.2, 0.5]),
            crate::TextColorSwatchSpan::new("甲乙\n".len(), "third".len(), [0.2, 0.9, 0.3, 1.0]),
        ]),
        0.0,
    );

    // 每个 span 在其末显示行内派生一个行高 65% 的覆盖方块，颜色按
    // 宿主给定值直传。
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.swatch_marks.len(), 2);
    assert_eq!(presentation.swatch_marks[0].color, [0.9, 0.2, 0.2, 0.5]);
    assert_eq!(presentation.swatch_marks[1].color, [0.2, 0.9, 0.3, 1.0]);
    for (mark, (line_top, span_end_x)) in presentation
        .swatch_marks
        .iter()
        .zip([(0.0, 20.0), (14.0, 50.0)])
    {
        assert_eq!(mark.rect.width, mark.rect.height);
        assert!((mark.rect.height - 9.1).abs() < 0.01);
        assert!(
            mark.rect.y >= line_top && mark.rect.y + mark.rect.height <= line_top + 14.0,
            "swatch vertically centered in its line"
        );
        // 右缘钳在 span 末端：不越出到 span 之后的文本上。
        assert!(mark.rect.x + mark.rect.width <= span_end_x + 0.01);
    }

    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { swatch_markers, .. } = geometry else {
        panic!("expected text input geometry");
    };
    assert_eq!(swatch_markers.len(), 2);
    assert_eq!(swatch_markers[0].1, [0.9, 0.2, 0.2, 0.5]);

    // 清空宿主 feed 后 swatch 从文本呈现与绘制几何中消失。
    swatch_editor_world(&mut world, false, Arc::from([]), 0.0);
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.swatch_marks.is_empty());
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { swatch_markers, .. } = geometry else {
        panic!("expected text input geometry");
    };
    assert!(swatch_markers.is_empty());
}

#[test]
fn color_swatches_clip_to_the_viewport_when_scrolled() {
    let mut world = UiWorld::default();
    swatch_editor_world(
        &mut world,
        true,
        Arc::from([
            crate::TextColorSwatchSpan::new(0, "甲乙".len(), [0.9, 0.2, 0.2, 1.0]),
            crate::TextColorSwatchSpan::new("甲乙\n".len(), "third".len(), [0.2, 0.9, 0.3, 1.0]),
        ]),
        28.0,
    );

    // 滚动两行后第一行的 swatch 完全在视口上方，不再产生图元；
    // 仍可见的 swatch 照常派生并随滚动平移（触边部分不丢弃，由节点
    // 裁剪收口）。
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput { swatch_markers, .. } = geometry else {
        panic!("expected text input geometry");
    };
    assert_eq!(swatch_markers.len(), 1);
    assert_eq!(swatch_markers[0].1, [0.2, 0.9, 0.3, 1.0]);
    assert!(swatch_markers[0].0.y + swatch_markers[0].0.height > 0.0);
}

#[test]
fn editor_chrome_derives_bracket_marks_caret_line_and_indent_guides() {
    let value = "(\n\tx)";
    let mut world = UiWorld::default();
    let mut create = MutationQueue::new();
    create.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    create.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: Some(Arc::from("\t")),
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    create.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                height: Some(nana_ui_core::LengthSpec::Px(60.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    create.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    create.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    create.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    create.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 60.0,
        },
    );
    world.commit(create).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();

    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 光标紧邻 '('：两端字符框，各一个字符宽。
    assert_eq!(
        presentation.bracket_marks,
        vec![
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 10.0,
                height: 14.0,
            },
            // ')' 在第二行第 2 列。
            LayoutBox {
                x: 20.0,
                y: 14.0,
                width: 10.0,
                height: 14.0,
            },
        ]
    );
    // 缩进参考线：第二行前导一个缩进单位，一条 1px 竖线。
    assert_eq!(
        presentation.indent_guides,
        vec![LayoutBox {
            x: 0.0,
            y: 14.0,
            width: 1.0,
            height: 14.0,
        }]
    );

    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput {
        caret_line,
        bracket_markers,
        indent_guides,
        ..
    } = geometry
    else {
        panic!("expected text input geometry");
    };
    // 未聚焦：括号标记与当前行条都不出现；参考线是静态结构标记。
    assert!(caret_line.is_none());
    assert!(bracket_markers.is_empty());
    assert_eq!(indent_guides.len(), 1);

    let mut focus = MutationQueue::new();
    focus.request_focus(document(1), Some(node(1)));
    world.commit(focus).unwrap();
    let geometry = world.component_geometry(node(1)).expect("geometry");
    let crate::ComponentGeometry::TextInput {
        caret_line,
        bracket_markers,
        indent_guides,
        ..
    } = geometry
    else {
        panic!("expected text input geometry");
    };
    // 聚焦且选区收起：光标所在行画整宽低对比背景条。
    let (line_rect, line_color) = caret_line.expect("caret line");
    assert_eq!(
        line_rect,
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 14.0,
        }
    );
    assert_eq!(line_color, world.style_model.palette.hover.as_rgba_array());
    // 括号两端出现描边框标记，共用同一颜色。
    assert_eq!(bracket_markers.len(), 2);
    assert_eq!(bracket_markers[0].0.y, 0.0);
    assert_eq!(bracket_markers[1].0.y, 14.0);
    assert_eq!(bracket_markers[0].1, bracket_markers[1].1);
    // 参考线不随焦点变化。
    assert_eq!(indent_guides.len(), 1);
}

#[test]
fn bracket_pair_colors_cycle_by_nesting_depth_and_dim_unmatched() {
    // FunctionalShaper：字符宽 = font_size（10），行高 14。
    let value = "({[]})]x(";
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        value,
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 配对括号按嵌套深度循环：`(` 0、`{` 1、`[` 2 / `]` 2、`}` 1、`)` 0；
    // 未配对的 `]`（偏移 6）与 `(`（偏移 8）标记为淡化深度哨兵。
    assert_eq!(
        presentation.bracket_color_spans.as_ref(),
        &[
            (0, 1, 0),
            (1, 2, 1),
            (2, 3, 2),
            (3, 4, 2),
            (4, 5, 1),
            (5, 6, 0),
            (6, 7, crate::components::TEXT_BRACKET_UNMATCHED_DEPTH),
            (8, 9, crate::components::TEXT_BRACKET_UNMATCHED_DEPTH),
        ][..]
    );
    // 提取层的字形色：深度色阶循环，未配对用 faint。
    let extracted = world.extract_nodes(&[node(1)]);
    let spans = &extracted[0].text_spans;
    let depth_color = |depth: usize| match depth {
        0 => world.style_model.palette.accent.as_rgba_array(),
        1 => world.style_model.palette.success.as_rgba_array(),
        2 => world.style_model.palette.warning.as_rgba_array(),
        crate::components::TEXT_BRACKET_UNMATCHED_DEPTH => {
            world.style_model.palette.faint.as_rgba_array()
        }
        _ => panic!("unexpected depth {depth}"),
    };
    for &(start, end, depth) in presentation.bracket_color_spans.iter() {
        let span = spans
            .iter()
            .find(|span| span.start == start && span.end == end)
            .unwrap_or_else(|| panic!("missing bracket span {start}..{end}"));
        assert_eq!(span.color, depth_color(depth));
    }
}

/// Bracket colouring is a function of the bracket characters alone. An edit
/// that touches none of them therefore cannot change the pairing -- only where
/// the brackets after it sit -- so it must not rescan the document: that scan
/// is O(document) and typing is the most frequent thing an editor does.
#[test]
fn an_edit_without_a_bracket_shifts_the_colour_spans_instead_of_rescanning() {
    let value = "fn main() {\n    let pair = [1, (2)];\n}\n";
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        value,
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    // Every edited value's answer must equal what a full scan of it says. A
    // world that has never seen the value takes the full-scan path.
    let full_scan = |value: &str| {
        let mut fresh = UiWorld::default();
        options_editor_world(
            &mut fresh,
            value,
            crate::TextSelection::caret(0),
            Arc::from([]),
            crate::TextEditorRenderOptions::default(),
            false,
        );
        fresh
            .shape_text(&[node(1)], &mut FunctionalShaper::default())
            .unwrap();
        fresh
            .text_input_presentation(node(1))
            .expect("presentation")
            .bracket_color_spans
            .clone()
    };
    let spans = |world: &UiWorld| {
        world
            .text_input_presentation(node(1))
            .expect("presentation")
            .bracket_color_spans
            .clone()
    };
    let edit = |world: &mut UiWorld, shaper: &mut FunctionalShaper, value: &str| {
        let mut queue = MutationQueue::new();
        queue.set_text_input(
            node(1),
            Some(crate::TextInputState {
                value: value.into(),
                selection: crate::TextSelection::caret(0),
                additional_selections: Vec::new(),
            }),
        );
        world.commit(queue).unwrap();
        crate::text_shape_stats::reset();
        world.shape_text(&[node(1)], shaper).unwrap();
        crate::text_shape_stats::snapshot().bracket_rescans
    };

    // Insert before the brackets, delete after them, and replace a run of the
    // same length: none of the three touches a bracket character.
    for edited in [
        "fn main2() {\n    let pair = [1, (2)];\n}\n",
        "fn main2() {\n    let x = [1, (2)];\n}\n",
        "fn MAIN2() {\n    let x = [1, (2)];\n}\n",
    ] {
        let rescans = edit(&mut world, &mut shaper, edited);
        assert_eq!(rescans, 0, "{edited:?} holds no new bracket");
        assert_eq!(spans(&world), full_scan(edited), "{edited:?}");
    }

    // Typing a bracket changes the pairing, so this one does rescan -- and the
    // rescan is what makes the nesting right again.
    let with_bracket = "fn MAIN2(()) {\n    let x = [1, (2)];\n}\n";
    let rescans = edit(&mut world, &mut shaper, with_bracket);
    assert_eq!(rescans, 1, "a new bracket needs the stack scan");
    assert_eq!(spans(&world), full_scan(with_bracket));

    // And deleting one, from a state that came out of a shift.
    let without = "fn MAIN2(() {\n    let x = [1, (2)];\n}\n";
    assert_eq!(edit(&mut world, &mut shaper, without), 1);
    assert_eq!(spans(&world), full_scan(without));
}

/// Replacing a CJK character with one that shares its UTF-8 prefix leaves the
/// changed byte range inside a character. The bracket cache slices the value
/// with that range, so it has to be a character boundary -- this used to panic
/// with "byte index 6 is not a char boundary".
#[test]
fn an_edit_inside_one_character_does_not_panic_the_bracket_cache() {
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "(你好)",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let spans = |world: &UiWorld| {
        world
            .text_input_presentation(node(1))
            .expect("presentation")
            .bracket_color_spans
            .clone()
    };
    let before = spans(&world);

    let mut edit = MutationQueue::new();
    edit.set_text_input(
        node(1),
        Some(crate::TextInputState {
            value: "(你奿)".into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(edit).unwrap();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    assert_eq!(
        spans(&world),
        before,
        "the brackets did not move, so neither do their spans"
    );
}

#[test]
fn bracket_pair_colors_follow_text_edits_and_option_off_disables() {
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "()",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(
        presentation.bracket_color_spans.as_ref(),
        &[(0, 1, 0), (1, 2, 0)][..]
    );

    // 文本变更后重算（嵌套深度随新文本更新）。
    let mut edit = MutationQueue::new();
    edit.set_text_input(
        node(1),
        Some(crate::TextInputState {
            value: "(())".into(),
            selection: crate::TextSelection::caret(0),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(edit).unwrap();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(
        presentation.bracket_color_spans.as_ref(),
        &[(0, 1, 0), (1, 2, 1), (2, 3, 1), (3, 4, 0)][..]
    );

    // 选项关闭：不着色。
    let mut quiet = UiWorld::default();
    options_editor_world(
        &mut quiet,
        "(())",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            bracket_pair_colors: false,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    quiet.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = quiet
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.bracket_color_spans.is_empty());
    assert!(quiet.extract_nodes(&[node(1)])[0].text_spans.is_empty());
}

#[test]
fn bracket_colors_override_syntax_spans_on_bracket_characters() {
    // 语法 span 覆盖整个文档时，括号字符被切分出来按配对色着色，
    // 其余字符保持语法色。
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "fn ()",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        false,
    );
    let mut queue = MutationQueue::new();
    queue.set_highlight_request(node(1), Some(crate::HighlightRequest::highlight("rs")));
    world.commit(queue).unwrap();
    let mut queue = MutationQueue::new();
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    world.commit(queue).unwrap();
    world.resolve_presentations(&[node(1)]).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let spans = &world.extract_nodes(&[node(1)])[0].text_spans;
    // "fn " 保持语法/默认色，"(" 与 ")" 各自独立成 span（配对色），
    // 语法 span 在括号处被切分。
    assert!(
        spans.iter().any(|span| span.start == 3 && span.end == 4)
            && spans.iter().any(|span| span.start == 4 && span.end == 5)
    );
    let bracket = spans.iter().find(|span| span.start == 3).unwrap();
    assert_eq!(
        bracket.color,
        world.style_model.palette.accent.as_rgba_array()
    );
}

#[test]
fn occurrence_highlight_marks_other_word_occurrences_when_focused() {
    // FunctionalShaper：字符宽 = font_size（10），行高 14。
    let value = "count = 1\ncount + counts\nx count";
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        value,
        crate::TextSelection::caret(2),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            occurrence_highlight: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    // 未聚焦：派生短路，零标记。
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.occurrence_marks.is_empty());
    let geometry = world.component_geometry(node(1)).unwrap();
    let crate::ComponentGeometry::TextInput {
        occurrence_markers, ..
    } = geometry
    else {
        panic!("expected text input geometry");
    };
    assert!(occurrence_markers.is_empty());

    focus_editor(&mut world);
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 光标所在出现（第一行）不画；全词匹配排除 "counts"；其余两处
    // 各一条单行矩形。
    assert_eq!(
        presentation.occurrence_marks,
        vec![
            LayoutBox {
                x: 0.0,
                y: 14.0,
                width: 50.0,
                height: 14.0,
            },
            LayoutBox {
                x: 20.0,
                y: 28.0,
                width: 50.0,
                height: 14.0,
            },
        ]
    );
    // 几何层在聚焦时输出淡底色（accent_soft）填充条带。
    let geometry = world.component_geometry(node(1)).unwrap();
    let crate::ComponentGeometry::TextInput {
        occurrence_markers, ..
    } = geometry
    else {
        panic!("expected text input geometry");
    };
    assert_eq!(occurrence_markers.len(), 2);
    assert_eq!(
        occurrence_markers[0].1,
        world.style_model.palette.accent_soft.as_rgba_array()
    );
}

/// minimap 几何断言辅助：取节点 1 的 minimap 竖条几何。
fn minimap_of(world: &UiWorld) -> crate::TextMinimapGeometry {
    match world.component_geometry(node(1)).unwrap() {
        crate::ComponentGeometry::TextInput {
            minimap: Some(minimap),
            ..
        } => minimap,
        other => panic!("expected minimap geometry, got {other:?}"),
    }
}

#[test]
fn minimap_off_short_circuits_collection_and_geometry() {
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "a\nbc",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    // 关闭时：零收集（空表）且几何层不产出竖条。
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.minimap_line_lengths.is_empty());
    let crate::ComponentGeometry::TextInput { minimap: None, .. } =
        world.component_geometry(node(1)).unwrap()
    else {
        panic!("minimap must be absent when the option is off");
    };
}

#[test]
fn minimap_collects_line_lengths_and_scales_bars_with_non_whitespace_length() {
    let mut world = UiWorld::default();
    // 行非空白长度 2/0/4/0/3：空行与纯空白行不产生行条。
    options_editor_world(
        &mut world,
        "ab\n\nabcd\n  \nxyz",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            minimap: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.minimap_line_lengths, vec![2, 0, 4, 0, 3]);

    let minimap = minimap_of(&world);
    // 内容区 200×80（无内边距）：面板为内容右缘 64px 覆盖条，分隔线 1px。
    assert_eq!(
        minimap.panel,
        LayoutBox {
            x: 136.0,
            y: 0.0,
            width: 64.0,
            height: 80.0,
        }
    );
    assert_eq!(
        minimap.separator,
        LayoutBox {
            x: 135.0,
            y: 0.0,
            width: 1.0,
            height: 80.0,
        }
    );
    assert_eq!(minimap.stride, 1);
    assert_eq!(minimap.line_count, 5);
    // 三条行条：宽度 ∝ 非空白长度（最长行 = 条宽 64），2px 高、2px 节距。
    let rows = minimap
        .bars
        .iter()
        .map(|bar| (bar.y, bar.width, bar.height))
        .collect::<Vec<_>>();
    assert_eq!(
        rows,
        vec![(0.0, 32.0, 2.0), (4.0, 64.0, 2.0), (8.0, 48.0, 2.0)]
    );
    // 颜色：面板 subtle、行条 faint；指示器 accent 半透明。
    assert_eq!(
        minimap.panel_color,
        world.style_model.palette.subtle.as_rgba_array()
    );
    assert_eq!(
        minimap.bar_color,
        world.style_model.palette.faint.as_rgba_array()
    );
    let accent = world.style_model.palette.accent.as_rgba_array();
    assert_eq!(
        minimap.indicator_color,
        [accent[0], accent[1], accent[2], accent[3] * 0.2]
    );
    // 5 行 × 14px = 70px 放得下：无指示器。
    assert!(minimap.indicator.is_none());
}

#[test]
fn minimap_samples_by_integer_stride_when_document_exceeds_strip() {
    let mut world = UiWorld::default();
    let value = (0..100)
        .map(|index| format!("line{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    options_editor_world(
        &mut world,
        &value,
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            minimap: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    let minimap = minimap_of(&world);
    assert_eq!(minimap.line_count, 100);
    // 容纳量 = 80 / 2 = 40 → 步长取整 ceil(100/40) = 3。
    assert_eq!(minimap.stride, 3);
    // 采样后条数 = ceil(100/3) = 34，槽位按 index / stride 落点。
    assert_eq!(minimap.bars.len(), 34);
    assert_eq!(minimap.bars[0].y, 0.0);
    assert_eq!(minimap.bars[1].y, 2.0);
    assert_eq!(minimap.bars[33].y, 66.0);
    assert!(minimap.bars.last().unwrap().y + 2.0 <= minimap.panel.height);
}

#[test]
fn minimap_indicator_follows_scroll_offset() {
    let mut world = UiWorld::default();
    let value = "\n".repeat(9) + "x";
    options_editor_world(
        &mut world,
        &value,
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            minimap: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    // 10 行 × 14px = 140px > 80px 视口：顶部指示器覆盖前 6 行
    // （ceil(80/14) = 6）→ 高 12px。
    let indicator = minimap_of(&world).indicator.expect("indicator");
    assert_eq!(indicator.y, 0.0);
    assert_eq!(indicator.height, 12.0);

    // 滚动一行（14px）：指示器按条距映射（2px/行 ÷ 步长）下移 2px。
    let mut queue = MutationQueue::new();
    queue.set_scroll_offset(node(1), ScrollOffset { x: 0.0, y: 14.0 });
    world.commit(queue).unwrap();
    let indicator = minimap_of(&world).indicator.expect("indicator");
    assert_eq!(indicator.y, 2.0);

    // 滚动到最底（60 = 140 − 80）：指示器底缘钳到文档末行
    // （10 行 × 2px 条距 = 20px），而非条底。
    let mut queue = MutationQueue::new();
    queue.set_scroll_offset(node(1), ScrollOffset { x: 0.0, y: 60.0 });
    world.commit(queue).unwrap();
    let indicator = minimap_of(&world).indicator.expect("indicator");
    assert!((indicator.y + indicator.height - 20.0).abs() < 0.01);
}

#[test]
fn minimap_shows_all_logical_lines_under_folds() {
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        FOLD_VALUE,
        crate::TextSelection::caret(0),
        Arc::from([FOLD_BLOCK]),
        crate::TextEditorRenderOptions {
            minimap: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    // 折叠隐藏 3 行（与主视图折叠态一致地喂入）。
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    // 主视图是折叠后的显示值，minimap 仍按原始 5 个逻辑行收集。
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.display_value.len() < FOLD_VALUE.len());
    assert_eq!(presentation.minimap_line_lengths, vec![6, 4, 4, 1, 7]);
    assert_eq!(minimap_of(&world).line_count, 5);
}

#[test]
fn minimap_scroll_target_centers_clicked_line() {
    let mut world = UiWorld::default();
    let value = (0..100)
        .map(|index| format!("line{index}"))
        .collect::<Vec<_>>()
        .join("\n");
    options_editor_world(
        &mut world,
        &value,
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            minimap: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();

    // 条内 y=5（步长 3 → 条距 2/3 px/行）：命中第 7 行，居中换算
    // scroll = 7 × 14 + 7 − 40 = 65。
    let target = world
        .text_minimap_scroll_target(node(1), 150.0, 5.0)
        .expect("target");
    assert_eq!(target.y, 65.0);
    assert_eq!(target.x, 0.0);
    // 顶部点击钳到 0；条外（x 或 y 越界）不命中。
    assert_eq!(
        world
            .text_minimap_scroll_target(node(1), 150.0, 0.0)
            .unwrap()
            .y,
        0.0
    );
    assert!(
        world
            .text_minimap_scroll_target(node(1), 100.0, 5.0)
            .is_none()
    );
    assert!(
        world
            .text_minimap_scroll_target(node(1), 150.0, 90.0)
            .is_none()
    );
}

#[test]
fn occurrence_highlight_requires_word_single_line_selection_option_and_caps() {
    let options = crate::TextEditorRenderOptions {
        occurrence_highlight: true,
        ..crate::TextEditorRenderOptions::default()
    };
    let style = ComputedStyle {
        font_size: 10.0,
        line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
        ..ComputedStyle::default()
    };
    let present = |value: &str, selection: crate::TextSelection, enabled: bool| {
        let state = TextInputState {
            value: value.into(),
            selection,
            additional_selections: Vec::new(),
        };
        let source = build_text_input_presentation_source(
            crate::TextInputView::of(&state.to_session()),
            None,
            "",
            false,
            true,
            TextInputEditorExtras {
                editor: if enabled {
                    options.clone()
                } else {
                    crate::TextEditorRenderOptions::default()
                },
                ..TextInputEditorExtras::default()
            },
            true,
            None,
            None,
            None,
            None,
        );
        let mut shaper = FunctionalShaper::default();
        shape_text_input_presentation(
            node(1),
            source,
            &style,
            crate::TextShapeConstraints::default(),
            &crate::components::TextOverlayMetrics::default(),
            &mut shaper,
        )
    };

    // 选项关闭：聚焦也不派生。
    let presentation = present("same same same", crate::TextSelection::caret(1), false);
    assert!(presentation.occurrence_marks.is_empty());

    // 光标不在词内（括号之间）：无标记。
    let presentation = present("fn () {}", crate::TextSelection::caret(5), true);
    assert!(presentation.occurrence_marks.is_empty());

    // 多行选区：无标记。
    let value = "one\ntwo";
    let presentation = present(value, crate::TextSelection::new(0, value.len()), true);
    assert!(presentation.occurrence_marks.is_empty());

    // 非空单行选区：选中文本按子串匹配（"cou" 命中 "counts" 内部），
    // 选区本身不重复画。
    let presentation = present("cou cou counts", crate::TextSelection::new(0, 3), true);
    assert_eq!(presentation.occurrence_marks.len(), 2);

    // 上限截断：250 处出现时最多画 199 条（200 上限内再扣掉光标处）。
    let value = vec!["w"; 250].join(" ");
    let presentation = present(&value, crate::TextSelection::caret(0), true);
    assert_eq!(presentation.occurrence_marks.len(), 199);
}

#[test]
fn occurrence_geometry_updates_without_reshaping_unchanged_text() {
    // Caret movement changes occurrence geometry, but must reuse the same text
    // layout. Geometry probes are delegated to the host, not counted as shapes.
    let style = ComputedStyle {
        font_size: 10.0,
        line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
        ..ComputedStyle::default()
    };
    let options = crate::TextEditorRenderOptions {
        occurrence_highlight: true,
        ..crate::TextEditorRenderOptions::default()
    };
    let value = "alpha beta gamma\nalpha beta gamma";
    let present = |selection: crate::TextSelection,
                   cache: &mut crate::text_layout_cache::TextLayoutCache| {
        let state = TextInputState {
            value: value.into(),
            selection,
            additional_selections: Vec::new(),
        };
        let source = build_text_input_presentation_source(
            crate::TextInputView::of(&state.to_session()),
            None,
            "",
            false,
            true,
            TextInputEditorExtras {
                editor: options.clone(),
                ..TextInputEditorExtras::default()
            },
            true,
            None,
            None,
            None,
            None,
        );
        let mut glyphs = crate::GlyphCache::default();
        let mut inner = FunctionalShaper::default();
        let mut shaper = CountingShaper::new(&mut inner, cache, &mut glyphs);
        shape_text_input_presentation(
            node(1),
            source,
            &style,
            crate::TextShapeConstraints::default(),
            &crate::components::TextOverlayMetrics::default(),
            &mut shaper,
        )
    };

    let mut cache = crate::text_layout_cache::TextLayoutCache::default();
    let first = present(crate::TextSelection::caret(1), &mut cache);
    assert_eq!(first.occurrence_marks.len(), 1);
    let (_, first_misses, _) = cache.take_counters();
    assert!(first_misses > 0, "first derivation must shape");

    let second = present(crate::TextSelection::caret(1), &mut cache);
    let (_, rerun_misses, _) = cache.take_counters();
    assert_eq!(rerun_misses, 0, "same layout inputs must not shape again");
    assert_eq!(second.occurrence_marks, first.occurrence_marks);

    let moved = present(
        crate::TextSelection::caret(value.len().saturating_sub(2)),
        &mut cache,
    );
    let (_, moved_misses, _) = cache.take_counters();
    assert_eq!(
        moved_misses, 0,
        "moving the caret must reuse unchanged text layout"
    );
    assert_ne!(moved.occurrence_marks, first.occurrence_marks);
}

#[test]
fn relative_line_numbers_show_absolute_cursor_line_and_display_distances() {
    let options = crate::TextEditorRenderOptions {
        relative_line_numbers: true,
        ..crate::TextEditorRenderOptions::default()
    };
    // 光标在显示行 1（偏移 2）：光标行显示绝对行号 2，两侧距离 1。
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "a\nb\nc",
        crate::TextSelection::caret(2),
        Arc::from([]),
        options.clone(),
        true,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.line_numbers, vec![1, 2, 1]);

    // 默认（相对关闭）：无折叠时行号表为空（几何层按索引 + 1 回退）。
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "a\nb\nc",
        crate::TextSelection::caret(2),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        true,
    );
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.line_numbers.is_empty());

    // 折叠态：距离按显示行计，光标行保留折叠校正后的绝对行号。
    // 折叠后显示 2 行（"fn a() { …3" / "fn b() {}"），光标在显示行 1
    // （原始第 5 行，值偏移 30）：显示 [1, 5]。
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        FOLD_VALUE,
        crate::TextSelection::caret(30),
        Arc::from([FOLD_BLOCK]),
        options,
        true,
    );
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.line_tops.len(), 2);
    assert_eq!(presentation.line_numbers, vec![1, 5]);
}

#[test]
fn whitespace_marks_locate_spaces_tabs_and_trailing_runs() {
    // "a b\n\tc  "：行内空格、行首 Tab、行尾两个空格。
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "a b\n\tc  ",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            show_whitespace: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    let rects: Vec<(f32, f32, crate::TextWhitespaceKind)> = presentation
        .whitespace_marks
        .iter()
        .map(|mark| (mark.rect.x, mark.rect.y, mark.kind))
        .collect();
    assert_eq!(
        rects,
        vec![
            (10.0, 0.0, crate::TextWhitespaceKind::Space),
            (0.0, 14.0, crate::TextWhitespaceKind::Tab),
            (20.0, 14.0, crate::TextWhitespaceKind::Space),
            (30.0, 14.0, crate::TextWhitespaceKind::Space),
        ]
    );

    // 折叠隐藏行后：隐藏行的空白不再产生标记（显示视图直接不含该
    // 行）。折叠视图两行显示值共 5 个空格、无 Tab。
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        FOLD_VALUE,
        crate::TextSelection::caret(0),
        Arc::from([FOLD_BLOCK]),
        crate::TextEditorRenderOptions {
            show_whitespace: true,
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert_eq!(presentation.whitespace_marks.len(), 5);
    assert!(
        presentation
            .whitespace_marks
            .iter()
            .all(|mark| mark.kind == crate::TextWhitespaceKind::Space)
    );

    // 选项关闭：零标记。
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "a b\n\tc  ",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions::default(),
        false,
    );
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    assert!(presentation.whitespace_marks.is_empty());
}

#[test]
fn wrap_guides_map_columns_to_x_and_skip_missing_columns() {
    let mut world = UiWorld::default();
    options_editor_world(
        &mut world,
        "ab\ncd",
        crate::TextSelection::caret(0),
        Arc::from([]),
        crate::TextEditorRenderOptions {
            wrap_guides: Arc::from([1, 2, 6, 0]),
            ..crate::TextEditorRenderOptions::default()
        },
        false,
    );
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    let presentation = world
        .text_input_presentation(node(1))
        .expect("presentation");
    // 列宽 = '0' 字形宽（FunctionalShaper 下 = font_size = 10）；整段
    // shape 宽度 50（5 字符 × 10）：列 1、2 保留，列 6（x=60）超宽丢
    // 弃，列 0 无意义丢弃。
    assert_eq!(presentation.wrap_guides, vec![10.0, 20.0]);
    // 几何层：贯穿内容区全高的 1px 竖线（静态，不随焦点变化）。
    let geometry = world.component_geometry(node(1)).unwrap();
    let crate::ComponentGeometry::TextInput {
        wrap_guides,
        whitespace_marks,
        ..
    } = geometry
    else {
        panic!("expected text input geometry");
    };
    assert_eq!(
        wrap_guides,
        vec![
            (
                LayoutBox {
                    x: 10.0,
                    y: 0.0,
                    width: 1.0,
                    height: 80.0,
                },
                world.style_model.palette.faint.as_rgba_array(),
            ),
            (
                LayoutBox {
                    x: 20.0,
                    y: 0.0,
                    width: 1.0,
                    height: 80.0,
                },
                world.style_model.palette.faint.as_rgba_array(),
            ),
        ]
    );
    // 选项全关的编辑器不产生空白/wrap guide 几何。
    assert!(whitespace_marks.is_empty());
}

#[test]
fn multiline_text_presentation_tracks_utf8_lines_selection_and_preedit() {
    let value = "甲乙\nthird\n末";
    let state = TextInputState {
        value: value.into(),
        selection: crate::TextSelection::new("甲".len(), "甲乙\nthird\n".len()),
        additional_selections: Vec::new(),
    };
    let style = ComputedStyle {
        font_size: 10.0,
        line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
        ..ComputedStyle::default()
    };
    let source = build_text_input_presentation_source(
        crate::TextInputView::of(&state.to_session()),
        None,
        "",
        false,
        true,
        TextInputEditorExtras::default(),
        false,
        None,
        None,
        None,
        None,
    );
    let mut shaper = FunctionalShaper::default();
    let presentation = shape_text_input_presentation(
        node(1),
        source,
        &style,
        crate::TextShapeConstraints::default(),
        &Default::default(),
        &mut shaper,
    );

    assert_eq!(presentation.caret_x, 0.0);
    assert_eq!(presentation.caret_y, 28.0);
    assert_eq!(presentation.line_height, 14.0);
    assert_eq!(presentation.selection_lines.len(), 2);
    assert_eq!(
        presentation.selection_lines[0],
        LayoutBox {
            x: 10.0,
            y: 0.0,
            width: 10.0,
            height: 14.0,
        }
    );
    assert_eq!(
        presentation.selection_lines[1],
        LayoutBox {
            x: 0.0,
            y: 14.0,
            width: 50.0,
            height: 14.0,
        }
    );

    let composing = TextInputState {
        value: "甲\n末".into(),
        selection: crate::TextSelection::caret("甲\n".len()),
        additional_selections: Vec::new(),
    };
    let source = build_text_input_presentation_source(
        crate::TextInputView::of(&composing.to_session()),
        Some(
            ImeComposition {
                text: "输\n入".into(),
                selection: None,
            }
            .view(),
        ),
        "",
        false,
        true,
        TextInputEditorExtras::default(),
        false,
        None,
        None,
        None,
        None,
    );
    let presentation = shape_text_input_presentation(
        node(1),
        source,
        &style,
        crate::TextShapeConstraints::default(),
        &Default::default(),
        &mut shaper,
    );
    assert_eq!(presentation.display_value, "甲\n输\n入末");
    assert_eq!(presentation.preedit_lines.len(), 2);
    assert_eq!(presentation.caret_y, 28.0);
}

#[test]
fn multi_cursor_presentation_merges_bands_and_paints_additional_carets() {
    let value = "甲乙\nthird\n末";
    let style = ComputedStyle {
        font_size: 10.0,
        line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
        ..ComputedStyle::default()
    };
    let present = |state: &TextInputState, ime: Option<&ImeComposition>| {
        let session = state.to_session();
        let source = build_text_input_presentation_source(
            crate::TextInputView::of(&session),
            ime.map(ImeComposition::view),
            "",
            false,
            true,
            TextInputEditorExtras::default(),
            false,
            None,
            None,
            None,
            None,
        );
        shape_text_input_presentation(
            node(1),
            source,
            &style,
            crate::TextShapeConstraints::default(),
            &Default::default(),
            &mut FunctionalShaper::default(),
        )
    };

    // 主选区（第一行到第二行）+ 两个收起的附加光标（第二、三行）。附加
    // 光标不落在主选区内：多选区集合互不重叠（会话会把重叠的熔成一个）。
    let state = TextInputState {
        value: value.into(),
        selection: crate::TextSelection::new("甲".len(), "甲乙\nth".len()),
        additional_selections: vec![
            crate::TextSelection::caret("甲乙\nthird".len()),
            crate::TextSelection::caret(value.len()),
        ],
    };
    let presentation = present(&state, None);
    // 主选区条带照旧 2 条；附加收起光标不产生条带。
    assert_eq!(presentation.selection_lines.len(), 2);
    // 每个收起的附加光标各一个 caret 坐标，按行分布。
    assert_eq!(
        presentation
            .additional_carets
            .iter()
            .map(|(_, y)| *y)
            .collect::<Vec<_>>(),
        vec![14.0, 28.0]
    );

    // 附加 range 选区：条带并入同一向量，不产生 caret。
    let ranged = TextInputState {
        value: value.into(),
        selection: crate::TextSelection::caret(0),
        additional_selections: vec![crate::TextSelection::new(
            "甲乙\nth".len(),
            "甲乙\nthird".len(),
        )],
    };
    let presentation = present(&ranged, None);
    assert_eq!(presentation.selection_lines.len(), 1);
    assert!(presentation.additional_carets.is_empty());

    // IME 组合期只挂主光标：附加光标与选区条带都隐藏。
    let presentation = present(
        &state,
        Some(&ImeComposition {
            text: "输".into(),
            selection: None,
        }),
    );
    assert!(presentation.additional_carets.is_empty());
    assert!(presentation.selection_lines.is_empty());
}

#[test]
fn text_input_geometry_keeps_all_multiline_decorations_and_single_line_contract() {
    let presentation = TextInputPresentation {
        selection: Some((2.0, 9.0)),
        selection_lines: vec![
            LayoutBox {
                x: 2.0,
                y: 0.0,
                width: 18.0,
                height: 14.0,
            },
            LayoutBox {
                x: 0.0,
                y: 14.0,
                width: 24.0,
                height: 14.0,
            },
        ],
        preedit: Some((4.0, 11.0)),
        preedit_lines: vec![
            LayoutBox {
                x: 4.0,
                y: 14.0,
                width: 16.0,
                height: 14.0,
            },
            LayoutBox {
                x: 0.0,
                y: 28.0,
                width: 8.0,
                height: 14.0,
            },
        ],
        ..TextInputPresentation::default()
    };
    let content = LayoutBox {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 42.0,
    };

    let (selection, preedit) =
        text_input_decorations(&presentation, true, content, 15.0, 14.0, 3.0, 5.0);
    assert_eq!(selection.len(), 2);
    assert_eq!(selection[0].x, 9.0);
    assert_eq!(selection[1].y, 29.0);
    assert_eq!(preedit.len(), 2);
    assert_eq!(preedit[0].y, 41.0);
    assert_eq!(preedit[1].y, 55.0);

    let (selection, preedit) =
        text_input_decorations(&presentation, false, content, 23.0, 14.0, 3.0, 5.0);
    assert_eq!(
        selection,
        vec![LayoutBox {
            x: 9.0,
            y: 23.0,
            width: 7.0,
            height: 14.0,
        }]
    );
    assert_eq!(
        preedit,
        vec![LayoutBox {
            x: 11.0,
            y: 35.0,
            width: 7.0,
            height: 2.0,
        }]
    );
}

#[test]
fn presentation_shaping_uses_resolved_wrap_only_for_multiline_editors() {
    #[derive(Default)]
    struct ConstraintProbe {
        positions: Vec<crate::TextShapeConstraints>,
    }

    impl TextShaper for ConstraintProbe {
        fn shape(
            &mut self,
            _id: StableNodeId,
            _text: &TextContent,
            _style: &ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            TextMetrics::default()
        }

        fn text_position(
            &mut self,
            _id: StableNodeId,
            _text: &TextContent,
            _byte_offset: usize,
            _style: &ComputedStyle,
            constraints: crate::TextShapeConstraints,
        ) -> (f32, f32, f32) {
            self.positions.push(constraints);
            (0.0, 0.0, 14.0)
        }
    }

    let state = TextInputState {
        value: "wrapped value".into(),
        selection: crate::TextSelection::caret("wrapped value".len()),
        additional_selections: Vec::new(),
    };
    let resolved = crate::TextShapeConstraints {
        max_width: Some(48.0),
        max_height: Some(20.0),
        wrap: true,
        ellipsis: true,
        max_lines: None,
        shaping: crate::TextShaping::Advanced,
        preserve_lines: false,
        wrap_break: nana_ui_core::TextWrapBreak::Word,
    };
    let style = ComputedStyle::default();
    let mut probe = ConstraintProbe::default();

    let multiline = build_text_input_presentation_source(
        crate::TextInputView::of(&state.to_session()),
        None,
        "",
        false,
        true,
        TextInputEditorExtras::default(),
        false,
        None,
        None,
        None,
        None,
    );
    shape_text_input_presentation(
        node(1),
        multiline,
        &style,
        resolved,
        &Default::default(),
        &mut probe,
    );
    assert_eq!(
        probe.positions.pop(),
        Some(crate::TextShapeConstraints {
            max_width: Some(48.0),
            max_height: None,
            wrap: true,
            ellipsis: false,
            max_lines: None,
            shaping: crate::TextShaping::Advanced,
            preserve_lines: false,
            wrap_break: nana_ui_core::TextWrapBreak::Word,
        })
    );

    let single_line = build_text_input_presentation_source(
        crate::TextInputView::of(&state.to_session()),
        None,
        "",
        false,
        false,
        TextInputEditorExtras::default(),
        false,
        None,
        None,
        None,
        None,
    );
    shape_text_input_presentation(
        node(1),
        single_line,
        &style,
        resolved,
        &Default::default(),
        &mut probe,
    );
    assert_eq!(
        probe.positions.pop(),
        Some(crate::TextShapeConstraints {
            max_width: None,
            max_height: None,
            wrap: false,
            ellipsis: false,
            max_lines: None,
            shaping: crate::TextShaping::Advanced,
            preserve_lines: false,
            wrap_break: nana_ui_core::TextWrapBreak::Word,
        })
    );
}

#[test]
fn animations_are_atomic_deadline_driven_and_replaceable() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Text);
    world.commit(queue).unwrap();

    let animation_id = AnimationId::new(1).unwrap();
    let missing_id = AnimationId::new(2).unwrap();
    let animation = AnimationSpec::new(
        animation_id,
        node(1),
        Duration::from_millis(100),
        Duration::from_millis(100),
        Duration::from_millis(16),
        Easing::EaseOutCubic,
    );
    let generation = world.generation();
    let mut invalid = MutationQueue::new();
    invalid.start_animation(animation.clone());
    invalid.stop_animation(missing_id);
    assert_eq!(
        world.commit(invalid),
        Err(UiWorldError::MissingAnimation(missing_id))
    );
    assert_eq!(world.generation(), generation);
    assert_eq!(world.next_animation_deadline(), None);

    let mut invalid_timing = MutationQueue::new();
    let mut invalid_spec = animation.clone();
    invalid_spec.timing.duration = Duration::ZERO;
    invalid_spec.playback.iteration_count = crate::AnimationIteration::INFINITE;
    invalid_timing.start_animation(invalid_spec);
    assert_eq!(
        world.commit(invalid_timing),
        Err(UiWorldError::InvalidAnimation(animation_id))
    );
    assert_eq!(world.generation(), generation);

    let mut start = MutationQueue::new();
    start.start_animation(animation.clone());
    world.commit(start).unwrap();
    assert_eq!(
        world.next_animation_deadline(),
        Some(Duration::from_millis(100))
    );
    assert!(
        world
            .advance_animations(Duration::from_millis(99))
            .samples
            .is_empty()
    );
    let first = world.advance_animations(Duration::from_millis(100));
    assert_eq!(first.samples.len(), 1);
    assert_eq!(first.samples[0].progress, 0.0);
    assert_eq!(first.next_deadline, Some(Duration::from_millis(116)));

    let mut replacement = animation.clone();
    replacement.target = node(2);
    replacement.timing.start = Duration::from_millis(150);
    replacement.curve = MotionCurve::Easing(Easing::Linear);
    let mut replace = MutationQueue::new();
    replace.start_animation(replacement);
    world.commit(replace).unwrap();
    let middle = world.advance_animations(Duration::from_millis(200));
    assert_eq!(middle.samples.len(), 1);
    assert_eq!(middle.samples[0].target, node(2));
    assert_eq!(middle.samples[0].progress, 0.5);
    assert!(!middle.samples[0].finished);

    let end = world.advance_animations(Duration::from_millis(250));
    assert_eq!(end.samples.len(), 1);
    assert_eq!(end.samples[0].progress, 1.0);
    assert!(end.samples[0].finished);
    assert_eq!(end.next_deadline, None);

    let mut start_then_stop = MutationQueue::new();
    start_then_stop.start_animation(animation.clone());
    start_then_stop.stop_animation(animation_id);
    world.commit(start_then_stop).unwrap();
    assert_eq!(world.next_animation_deadline(), None);
}

#[test]
fn despawning_animation_target_cancels_its_wakeup() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.start_animation(AnimationSpec::new(
        AnimationId::new(1).unwrap(),
        node(2),
        Duration::from_millis(10),
        Duration::from_secs(1),
        Duration::from_millis(16),
        Easing::Linear,
    ));
    world.commit(queue).unwrap();
    assert_eq!(
        world.next_animation_deadline(),
        Some(Duration::from_millis(10))
    );

    let mut remove = MutationQueue::new();
    remove.despawn_subtree(node(1));
    world.commit(remove).unwrap();
    assert_eq!(world.next_animation_deadline(), None);
    assert!(world.advance_animations(Duration::MAX).samples.is_empty());
}

#[test]
fn advance_animations_counts_due_scheduler_lookups_not_the_idle_set() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for index in 1..=64 {
        queue.create(node(index), document(1), NodeKind::Text);
    }
    for index in 1..=64 {
        let due = index == 1;
        queue.start_animation(AnimationSpec::new(
            AnimationId::new(index).unwrap(),
            node(index),
            if due {
                Duration::ZERO
            } else {
                Duration::from_secs(60)
            },
            Duration::from_millis(1),
            Duration::from_millis(16),
            Easing::Linear,
        ));
    }
    world.commit(queue).unwrap();

    let sparse = world.advance_animations(Duration::from_millis(1));
    assert_eq!(sparse.samples.len(), 1);
    assert_eq!(sparse.animation_deadlines_scanned, 1);
    assert_eq!(sparse.animations_considered, 1);

    let mut all_due = MutationQueue::new();
    for index in 2..=64 {
        all_due.start_animation(AnimationSpec::new(
            AnimationId::new(index).unwrap(),
            node(index),
            Duration::ZERO,
            Duration::from_millis(1),
            Duration::from_millis(16),
            Easing::Linear,
        ));
    }
    world.commit(all_due).unwrap();
    let full = world.advance_animations(Duration::from_millis(1));
    assert_eq!(full.samples.len(), 63);
    assert_eq!(full.animation_deadlines_scanned, 63);
    assert_eq!(full.animations_considered, 63);
}

fn opacity_overlay_spec(
    id: u64,
    target: StableNodeId,
    start_ms: u64,
    duration_ms: u64,
    from: f32,
    to: f32,
) -> AnimationSpec {
    AnimationSpec::new(
        AnimationId::new(id).unwrap(),
        target,
        Duration::from_millis(start_ms),
        Duration::from_millis(duration_ms),
        Duration::from_millis(16),
        Easing::Linear,
    )
    .with_property(AnimatableProperty::Opacity)
    .with_range(
        MotionValue::Scalar(from),
        MotionTo::Value(MotionValue::Scalar(to)),
    )
}

#[test]
fn compositor_opacity_keeps_logical_style_and_animates_presentation() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    world.commit(queue).unwrap();

    assert_eq!(
        world.logical_motion_value(node(1), AnimatableProperty::Opacity),
        Some(MotionValue::Scalar(1.0))
    );
    let started = world.advance_animations(Duration::ZERO);
    assert_eq!(started.samples.len(), 1);
    assert!(!started.samples[0].finished);
    assert_eq!(started.next_deadline, Some(Duration::from_millis(100)));

    let idle = world.advance_animations(Duration::from_millis(50));
    assert!(idle.samples.is_empty());
    assert_eq!(idle.animation_deadlines_scanned, 0);
    assert_eq!(idle.animations_considered, 0);
    let pair = world
        .presentation_pair(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(50),
        )
        .expect("pair");
    assert_eq!(pair.logical, MotionValue::Scalar(1.0));
    match pair.presentation {
        MotionValue::Scalar(value) => assert!((value - 0.5).abs() < 1e-5),
        other => panic!("expected scalar, got {other:?}"),
    }
    assert_eq!(
        world.node_style(node(1)).unwrap().layout.opacity,
        None,
        "logical UiWorld opacity must not be written each frame"
    );
}

#[test]
fn sibling_document_cpu_deadline_does_not_sample_a_static_world() {
    let mut animated = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(AnimationSpec::new(
        AnimationId::new(1).unwrap(),
        node(1),
        Duration::ZERO,
        Duration::from_millis(32),
        Duration::from_millis(16),
        Easing::Linear,
    ));
    animated.commit(queue).unwrap();
    animated.advance_animations(Duration::ZERO);
    assert!(animated.next_animation_deadline().is_some());

    let mut static_world = UiWorld::new();
    let mut idle = MutationQueue::new();
    idle.create(node(1), document(2), NodeKind::Document);
    static_world.commit(idle).unwrap();
    assert_eq!(static_world.next_animation_deadline(), None);

    let due = animated.next_animation_deadline().unwrap();
    let animated_frame = animated.advance_animations(due);
    assert!(animated_frame.has_updates());
    let static_frame = static_world.advance_animations(due);
    assert!(!static_frame.has_updates());
    assert_eq!(static_frame.animation_deadlines_scanned, 0);
    assert_eq!(static_frame.animations_considered, 0);
}

#[test]
fn compositor_clear_layer_request_drops_extract_flag() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    world.commit(queue).unwrap();
    world.request_compositor_layer(node(1));
    assert!(world.extract_nodes(&[node(1)])[0].compositor.request_layer);
    world.clear_compositor_layer_request(node(1));
    assert!(!world.extract_nodes(&[node(1)])[0].compositor.request_layer);
}

#[test]
fn compositor_completion_fires_from_deadline_without_per_frame_samples() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::ZERO);

    let done = world.advance_animations(Duration::from_millis(100));
    assert_eq!(done.samples.len(), 1);
    assert!(done.samples[0].finished);
    assert_eq!(done.animation_deadlines_scanned, 1);
    assert_eq!(done.animations_considered, 1);
    assert_eq!(done.events.len(), 1);
    assert_eq!(done.events[0].kind, AnimationEventKind::Finished);
    assert!(!world.animation_is_active(AnimationId::new(1).unwrap()));
    assert_eq!(
        world.presentation_applied_value(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(150)
        ),
        None
    );
}

#[test]
fn retarget_starts_from_current_presentation() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));

    let mut retarget = MutationQueue::new();
    retarget.start_animation(
        opacity_overlay_spec(1, node(1), 50, 100, 0.0, 0.0)
            .with_interrupt(MotionInterrupt::Retarget),
    );
    world.commit(retarget).unwrap();
    let pair = world
        .presentation_pair(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(50),
        )
        .expect("pair");
    match pair.presentation {
        MotionValue::Scalar(value) => {
            assert!(
                (value - 0.5).abs() < 1e-4,
                "retarget must continue from the current presentation, got {value}"
            );
        }
        other => panic!("expected scalar, got {other:?}"),
    }
    let later = world
        .presentation_motion_value(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(100),
        )
        .expect("presentation");
    match later {
        MotionValue::Scalar(value) => assert!(value < 0.5, "retargeting toward 0, got {value}"),
        other => panic!("expected scalar, got {other:?}"),
    }
}

#[test]
fn cancel_finish_and_fill_respect_applies() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    queue.start_animation(
        opacity_overlay_spec(2, node(2), 0, 100, 0.0, 1.0).with_playback(
            AnimationPlayback::running(
                crate::AnimationIteration::ONCE,
                crate::AnimationDirection::Normal,
                AnimationFillMode::Forwards,
            ),
        ),
    );
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(40));

    let mut cancel = MutationQueue::new();
    cancel.stop_animation(AnimationId::new(1).unwrap());
    world.commit(cancel).unwrap();
    let cancelled = world.advance_animations(Duration::from_millis(40));
    assert_eq!(cancelled.events[0].kind, AnimationEventKind::Cancelled);
    assert_eq!(
        world.presentation_applied_value(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(40)
        ),
        None
    );
    assert_eq!(
        world
            .presentation_pair(
                node(1),
                AnimatableProperty::Opacity,
                Duration::from_millis(40)
            )
            .unwrap()
            .presentation,
        MotionValue::Scalar(1.0)
    );

    let mut finish = MutationQueue::new();
    finish.finish_animation(AnimationId::new(2).unwrap());
    world.commit(finish).unwrap();
    let finished = world.advance_animations(Duration::from_millis(40));
    assert_eq!(finished.events[0].kind, AnimationEventKind::Finished);
    assert_eq!(
        world.presentation_applied_value(
            node(2),
            AnimatableProperty::Opacity,
            Duration::from_millis(40)
        ),
        Some(MotionValue::Scalar(1.0))
    );
}

#[test]
fn later_start_wins_same_property_overlay() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    queue.start_animation(opacity_overlay_spec(2, node(1), 40, 100, 0.2, 0.8));
    world.commit(queue).unwrap();
    let value = world
        .presentation_applied_value(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(50),
        )
        .expect("later track");
    match value {
        MotionValue::Scalar(v) => assert!((v - 0.26).abs() < 1e-5, "got {v}"),
        other => panic!("expected scalar, got {other:?}"),
    }
}

#[test]
fn parked_node_keeps_unfinished_presentation_overlay() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.start_animation(opacity_overlay_spec(1, node(2), 0, 100, 0.0, 1.0));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::ZERO);

    let mut park = MutationQueue::new();
    park.park_subtree(node(2));
    world.commit(park).unwrap();
    assert!(world.presentation_retains_unmounted(node(2), Duration::from_millis(40)));
    assert!(world.animation_is_active(AnimationId::new(1).unwrap()));
    assert_eq!(
        world
            .presentation_applied_value(
                node(2),
                AnimatableProperty::Opacity,
                Duration::from_millis(40)
            )
            .and_then(|value| match value {
                MotionValue::Scalar(v) => Some(v),
                _ => None,
            })
            .map(|v| (v - 0.4).abs() < 1e-5),
        Some(true)
    );

    let mut despawn = MutationQueue::new();
    despawn.despawn_subtree(node(2));
    world.commit(despawn).unwrap();
    assert!(!world.animation_is_active(AnimationId::new(1).unwrap()));
    assert!(!world.presentation_retains_unmounted(node(2), Duration::from_millis(40)));
}

fn progress_clock_spec(id: u64, target: StableNodeId) -> AnimationSpec {
    AnimationSpec::new(
        AnimationId::new(id).unwrap(),
        target,
        Duration::ZERO,
        Duration::from_secs(60),
        Duration::from_millis(16),
        Easing::Linear,
    )
}

#[test]
fn park_cancel_event_is_one_shot_and_later_advance_is_idle() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.start_animation(progress_clock_spec(1, node(2)));
    world.commit(queue).unwrap();
    assert_eq!(world.next_animation_deadline(), Some(Duration::ZERO));

    let mut park = MutationQueue::new();
    park.park_subtree(node(2));
    world.commit(park).unwrap();
    assert_eq!(world.next_animation_deadline(), None);
    let on_park = world.take_animation_events();
    assert_eq!(on_park.len(), 1);
    assert_eq!(on_park[0].kind, AnimationEventKind::Cancelled);
    assert_eq!(on_park[0].target, node(2));

    let remount = {
        let mut queue = MutationQueue::new();
        queue.insert(node(1), node(2), None);
        queue
    };
    world.commit(remount).unwrap();
    let idle = world.advance_animations(Duration::from_secs(1));
    assert!(
        !idle.has_updates(),
        "park Cancelled must not leak onto a later idle advance: {idle:?}"
    );
    assert!(idle.events.is_empty());
    assert!(idle.samples.is_empty());

    let mut again = UiWorld::new();
    let mut start = MutationQueue::new();
    start.create(node(1), document(1), NodeKind::Document);
    start.create(node(2), document(1), NodeKind::Text);
    start.insert(node(1), node(2), None);
    start.start_animation(progress_clock_spec(1, node(2)));
    start.start_animation(opacity_overlay_spec(2, node(2), 0, 100, 0.0, 1.0));
    again.commit(start).unwrap();

    let mut park = MutationQueue::new();
    park.park_subtree(node(2));
    again.commit(park).unwrap();
    assert!(again.presentation_retains_unmounted(node(2), Duration::from_millis(40)));
    assert!(again.animation_is_active(AnimationId::new(2).unwrap()));

    let cancel_frame = again.advance_animations(Duration::from_millis(40));
    assert!(
        cancel_frame
            .events
            .iter()
            .any(|event| event.kind == AnimationEventKind::Cancelled && event.id.get() == 1)
    );
    assert!(cancel_frame.has_updates());
    assert!(again.presentation_retains_unmounted(node(2), Duration::from_millis(40)));

    let mut only_progress = UiWorld::new();
    let mut start = MutationQueue::new();
    start.create(node(1), document(1), NodeKind::Document);
    start.create(node(2), document(1), NodeKind::Text);
    start.insert(node(1), node(2), None);
    start.start_animation(progress_clock_spec(1, node(2)));
    only_progress.commit(start).unwrap();
    let mut park = MutationQueue::new();
    park.park_subtree(node(2));
    only_progress.commit(park).unwrap();
    let cancel_frame = only_progress.advance_animations(Duration::from_millis(40));
    assert_eq!(cancel_frame.events.len(), 1);
    assert_eq!(cancel_frame.events[0].kind, AnimationEventKind::Cancelled);
    assert!(cancel_frame.has_updates());
    let steady = only_progress.advance_animations(Duration::from_secs(1));
    assert!(!steady.has_updates());
    assert!(steady.events.is_empty());
    assert_eq!(steady.animation_deadlines_scanned, 0);
}

#[test]
fn reverse_animation_retargets_toward_the_original_from() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));

    let mut reverse = MutationQueue::new();
    reverse.reverse_animation(AnimationId::new(1).unwrap());
    world.commit(reverse).unwrap();
    let start = world
        .presentation_applied_value(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(50),
        )
        .expect("still applies");
    match start {
        MotionValue::Scalar(value) => {
            assert!((value - 0.5).abs() < 1e-4, "got {value}");
        }
        other => panic!("expected scalar, got {other:?}"),
    }
    let later = world
        .presentation_applied_value(
            node(1),
            AnimatableProperty::Opacity,
            Duration::from_millis(100),
        )
        .expect("reversed");
    match later {
        MotionValue::Scalar(value) => assert!(value < 0.5, "got {value}"),
        other => panic!("expected scalar, got {other:?}"),
    }
}

fn transform_overlay_spec(id: u64, target: StableNodeId) -> AnimationSpec {
    AnimationSpec::new(
        AnimationId::new(id).unwrap(),
        target,
        Duration::ZERO,
        Duration::from_millis(100),
        Duration::from_millis(16),
        Easing::Linear,
    )
    .with_property(AnimatableProperty::Transform)
    .with_range(
        MotionValue::Transform(PaintTransform::default()),
        MotionTo::Value(MotionValue::Transform(PaintTransform {
            a: 1.0,
            b: 0.0,
            c: 0.0,
            d: 1.0,
            e: 40.0,
            f: 0.0,
        })),
    )
}

#[test]
fn compositor_descriptor_matches_evaluate_track_and_overlay() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    queue.start_animation(transform_overlay_spec(2, node(1)));
    world.commit(queue).unwrap();

    let opacity_id = AnimationId::new(1).unwrap();
    let transform_id = AnimationId::new(2).unwrap();
    let opacity_handle = world.motion_handle(opacity_id).expect("opacity binding");
    let transform_handle = world
        .motion_handle(transform_id)
        .expect("transform binding");
    let now = Duration::from_millis(50);
    let store = world.motion_descriptor_store();
    let opacity = store
        .evaluate(opacity_handle, now)
        .expect("opacity descriptor");
    let overlay = world
        .presentation_applied_value(node(1), AnimatableProperty::Opacity, now)
        .expect("overlay");
    assert_eq!(opacity.applied_value(), Some(overlay));
    match overlay {
        MotionValue::Scalar(v) => assert!((v - 0.5).abs() < 1e-5),
        other => panic!("{other:?}"),
    }
    let transform = store
        .evaluate(transform_handle, now)
        .expect("transform descriptor");
    match transform.applied_value() {
        Some(MotionValue::Transform(value)) => assert!((value.e - 20.0).abs() < 1e-5),
        other => panic!("{other:?}"),
    }
}

#[test]
fn retarget_and_cancel_update_motion_descriptors() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    world.commit(queue).unwrap();
    let id = AnimationId::new(1).unwrap();
    let handle = world.motion_handle(id).expect("bound");
    let generation = handle.generation();

    let mut retarget = MutationQueue::new();
    retarget.start_animation(
        opacity_overlay_spec(1, node(1), 50, 100, 0.0, 0.0)
            .with_interrupt(MotionInterrupt::Retarget),
    );
    world.commit(retarget).unwrap();
    let after = world.motion_handle(id).expect("still bound");
    assert_eq!(after.index(), handle.index());
    assert_eq!(after.generation(), generation);
    let descriptor = world
        .motion_descriptor_store()
        .get(after)
        .expect("updated descriptor");
    match descriptor.from {
        MotionValue::Scalar(v) => assert!((v - 0.5).abs() < 1e-4, "got {v}"),
        other => panic!("{other:?}"),
    }

    let mut cancel = MutationQueue::new();
    cancel.stop_animation(id);
    world.commit(cancel).unwrap();
    world.advance_animations(Duration::from_millis(50));
    assert!(world.motion_handle(id).is_none());
    assert!(world.motion_descriptor_store().get(handle).is_none());

    let mut restart = MutationQueue::new();
    restart.start_animation(opacity_overlay_spec(1, node(1), 50, 100, 0.2, 0.8));
    world.commit(restart).unwrap();
    let reused = world.motion_handle(id).expect("reallocated");
    assert_eq!(reused.index(), handle.index());
    assert_ne!(reused.generation(), generation);
    assert!(world.motion_descriptor_store().get(handle).is_none());
}

#[test]
fn paint_and_layout_tracks_do_not_allocate_descriptors() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(
        AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            node(1),
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Color)
        .with_range(
            MotionValue::Color([0.0, 0.0, 0.0, 1.0]),
            MotionTo::Value(MotionValue::Color([1.0, 1.0, 1.0, 1.0])),
        ),
    );
    queue.start_animation(
        AnimationSpec::new(
            AnimationId::new(2).unwrap(),
            node(1),
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Width)
        .with_range(
            MotionValue::Scalar(10.0),
            MotionTo::Value(MotionValue::Scalar(20.0)),
        ),
    );
    world.commit(queue).unwrap();
    assert!(world.motion_handle(AnimationId::new(1).unwrap()).is_none());
    assert!(world.motion_handle(AnimationId::new(2).unwrap()).is_none());
    assert!(world.motion_descriptor_store().is_empty());
    assert!(
        world
            .presentation_applied_value(
                node(1),
                AnimatableProperty::Width,
                Duration::from_millis(50)
            )
            .is_none()
    );
}

#[test]
fn compositor_idle_frames_do_not_rebuild_descriptor_slab() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 100, 0.0, 1.0));
    world.commit(queue).unwrap();
    world.advance_animations(Duration::ZERO);
    let epoch = world.motion_descriptor_store().structure_epoch();
    let capacity = world.motion_descriptor_store().slot_capacity();
    let idle = world.advance_animations(Duration::from_millis(50));
    assert!(idle.samples.is_empty());
    assert_eq!(idle.animation_deadlines_scanned, 0);
    assert_eq!(idle.animations_considered, 0);
    assert_eq!(world.motion_descriptor_store().structure_epoch(), epoch);
    assert_eq!(world.motion_descriptor_store().slot_capacity(), capacity);
    let handle = world
        .motion_handle(AnimationId::new(1).unwrap())
        .expect("bound");
    for ms in [10, 40, 70] {
        let sample = world
            .motion_descriptor_store()
            .evaluate(handle, Duration::from_millis(ms))
            .expect("steady eval");
        assert_eq!(
            sample.applied_value(),
            world.presentation_applied_value(
                node(1),
                AnimatableProperty::Opacity,
                Duration::from_millis(ms)
            )
        );
    }
    assert_eq!(world.motion_descriptor_store().structure_epoch(), epoch);
}

#[test]
fn compositor_steady_frame_work_counters_are_quiet() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(opacity_overlay_spec(1, node(1), 0, 400, 0.0, 1.0));
    world.commit(queue).unwrap();
    let _ = world.take_system_work();
    world.advance_animations(Duration::ZERO);
    let _ = world.take_system_work();
    let queries = world.presentation_values_cpu_sampled();
    let generation = world.generation();
    let steady = world.advance_animations(Duration::from_millis(80));
    assert!(steady.samples.is_empty());
    assert_eq!(steady.animation_deadlines_scanned, 0);
    assert_eq!(steady.animations_considered, 0);
    assert!(world.take_system_work().is_empty());
    assert_eq!(world.generation(), generation);
    assert_eq!(world.presentation_values_cpu_sampled(), queries);
    let frame = world.last_motion_frame_counters();
    assert_eq!(frame.motion_tracks_compositor, 1);
    assert_eq!(frame.motion_tracks_cpu, 0);
    assert!(
        frame.compositor_steady_is_quiet(),
        "compositor-only present must not mutate UiWorld: {frame:?}"
    );
}

#[test]
fn compositor_inspector_summary_matches_issue_shape() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(123), document(1), NodeKind::Document);
    queue.start_animation(
        transform_overlay_spec(1, node(123)).with_property(AnimatableProperty::Transform),
    );
    world.commit(queue).unwrap();
    world.advance_animations(Duration::from_millis(50));
    let entries = world.inspect_motion();
    let transform = entries
        .iter()
        .find(|entry| entry.property == AnimatableProperty::Transform)
        .expect("transform track");
    assert_eq!(transform.node, 123);
    assert_eq!(transform.class, AnimationClass::Compositor);
    assert_eq!(transform.evaluator, MotionEvaluatorBackend::Gpu);
    assert_eq!(transform.runtime_samples_this_frame, 0);
    assert!(transform.cpu_fallback_reason.is_none());
    assert!(transform.gpu_handle.is_some());
    assert_eq!(
        transform.format_summary(),
        "Node #123 transform\nClass: Compositor\nEvaluator: GPU\nLayer: none\nRuntime samples/frame: 0"
    );
}

#[test]
fn layout_inspector_reports_cpu_fallback() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.start_animation(
        AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            node(1),
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_property(AnimatableProperty::Width)
        .with_range(
            MotionValue::Scalar(10.0),
            MotionTo::Value(MotionValue::Scalar(20.0)),
        ),
    );
    world.commit(queue).unwrap();
    let entry = world
        .inspect_motion()
        .into_iter()
        .find(|entry| entry.property == AnimatableProperty::Width)
        .expect("width");
    assert_eq!(entry.evaluator, MotionEvaluatorBackend::Cpu);
    assert!(
        entry
            .cpu_fallback_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("width"))
    );
}

#[test]
fn infinite_animation_keeps_waking_and_paused_animation_does_not() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Text);
    queue.start_animation(
        AnimationSpec::new(
            AnimationId::new(1).unwrap(),
            node(1),
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_playback(AnimationPlayback::running(
            crate::AnimationIteration::Infinite,
            crate::AnimationDirection::Alternate,
            crate::AnimationFillMode::None,
        )),
    );
    queue.start_animation(
        AnimationSpec::new(
            AnimationId::new(2).unwrap(),
            node(2),
            Duration::ZERO,
            Duration::from_millis(100),
            Duration::from_millis(16),
            Easing::Linear,
        )
        .with_playback(AnimationPlayback {
            iteration_count: crate::AnimationIteration::ONCE,
            direction: crate::AnimationDirection::Normal,
            fill_mode: crate::AnimationFillMode::None,
            play_state: crate::AnimationPlayState::Paused,
            paused_at: Some(Duration::ZERO),
        }),
    );
    world.commit(queue).unwrap();

    let first = world.advance_animations(Duration::ZERO);
    assert_eq!(first.samples.len(), 2);
    let infinite = first
        .samples
        .iter()
        .find(|sample| sample.id == AnimationId::new(1).unwrap())
        .unwrap();
    let paused = first
        .samples
        .iter()
        .find(|sample| sample.id == AnimationId::new(2).unwrap())
        .unwrap();
    assert!(!infinite.finished);
    assert_eq!(infinite.progress, 0.0);
    assert!(!paused.finished);
    assert_eq!(paused.progress, 0.0);
    assert_eq!(first.next_deadline, Some(Duration::from_millis(16)));

    let later = world.advance_animations(Duration::from_millis(150));
    assert_eq!(later.samples.len(), 1);
    assert_eq!(later.samples[0].id, AnimationId::new(1).unwrap());
    assert!(!later.samples[0].finished);
    assert!((later.samples[0].progress - 0.5).abs() < f32::EPSILON);
}

#[test]
fn subtree_despawn_retires_ids_and_stale_handles_do_not_alias() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(node(id), document(1), NodeKind::Text);
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut queue = MutationQueue::new();
    queue.despawn_subtree(node(2));
    let report = world.commit(queue).unwrap();
    assert_eq!(report.despawned, 2);
    assert_eq!(
        world.node(node(1)).unwrap().children,
        Vec::<StableNodeId>::new()
    );
    assert!(!world.contains(node(2)));
    assert!(world.is_retired(node(2)));
    let work = world.take_system_work();
    assert_eq!(work.render_removals, vec![node(2), node(3)]);
    assert_eq!(work.accessibility_removals, vec![node(2), node(3)]);
    let accessibility = world.project_accessibility_delta(&work);
    assert_eq!(accessibility.generation, work.generation);
    assert_eq!(accessibility.removed, vec![node(2), node(3)]);
    assert_eq!(
        accessibility
            .updated
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![node(1)]
    );
    assert!(accessibility.updated[0].children.is_empty());

    let mut queue = MutationQueue::new();
    queue.create(node(2), document(1), NodeKind::Text);
    assert_eq!(world.commit(queue), Err(UiWorldError::RetiredNode(node(2))));
}

#[test]
fn batch_cannot_recreate_an_id_after_despawn() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    world.commit(queue).unwrap();

    let mut queue = MutationQueue::new();
    queue.despawn_subtree(node(1));
    queue.create(node(1), document(1), NodeKind::Document);
    assert_eq!(world.commit(queue), Err(UiWorldError::RetiredNode(node(1))));
    assert!(world.contains(node(1)));
    assert!(!world.is_retired(node(1)));
}

#[test]
fn dirty_work_is_incremental_and_static_world_stays_idle() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=4 {
        queue.create(node(id), document(1), NodeKind::Text);
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    queue.insert(node(3), node(4), None);
    let report = world.commit(queue).unwrap();
    assert_eq!(report.generation, 1);

    let initial = world.take_system_work();
    assert_eq!(initial.generation, 1);
    assert_eq!(initial.style, vec![node(1), node(2), node(3), node(4)]);
    assert_eq!(initial.render_extraction, initial.style);
    assert!(world.take_system_work().is_empty());

    let empty = world.commit(MutationQueue::new()).unwrap();
    assert_eq!(empty.generation, 1);
    assert!(world.take_system_work().is_empty());

    let mut queue = MutationQueue::new();
    queue.insert(node(2), node(3), None);
    let report = world.commit(queue).unwrap();
    assert_eq!(report.generation, 2);
    let work = world.take_system_work();
    assert_eq!(work.style, vec![node(3), node(4)]);
    assert_eq!(work.text, vec![node(3), node(4)]);
    assert_eq!(work.focus_ime, vec![node(3), node(4)]);
    assert_eq!(work.layout, vec![node(1), node(2), node(3), node(4)]);
    assert_eq!(work.render_extraction, work.layout);
    assert!(world.take_system_work().is_empty());
}

/// The component index is what lets the pointer path ask "every split pane in
/// this document" without walking it. Its failure mode is silent -- a missed
/// node just never gets its hover cleared -- so the two edges that maintain it
/// need a test each.
#[test]
fn the_component_index_follows_retyping_and_despawn() {
    let mut world = UiWorld::new();
    let split = ComponentTypeId::new("nana.split-pane").unwrap();
    let calendar = ComponentTypeId::new("nana.calendar-heatmap").unwrap();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.insert(node(1), node(2), None);
    queue.set_component_type(node(2), Some(split.clone()));
    world.commit(queue).unwrap();

    let indexed = |world: &UiWorld, component: &str| {
        let mut ids: Vec<_> = world.nodes_of_component(document(1), component).collect();
        ids.sort();
        ids
    };
    assert_eq!(indexed(&world, "nana.split-pane"), vec![node(2)]);

    // Retyping moves the node between buckets rather than leaving it in both.
    let mut queue = MutationQueue::new();
    queue.set_component_type(node(2), Some(calendar));
    world.commit(queue).unwrap();
    assert_eq!(indexed(&world, "nana.split-pane"), Vec::new());
    assert_eq!(indexed(&world, "nana.calendar-heatmap"), vec![node(2)]);

    // Despawn drops it, or the index grows forever and hands out dead ids.
    let mut queue = MutationQueue::new();
    queue.despawn_subtree(node(2));
    world.commit(queue).unwrap();
    assert_eq!(indexed(&world, "nana.calendar-heatmap"), Vec::new());
}

#[test]
fn set_component_type_is_noop_when_unchanged() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    world.commit(queue).unwrap();
    let _ = world.take_system_work();

    let button = ComponentTypeId::new("nana.button").unwrap();
    let mut queue = MutationQueue::new();
    queue.set_component_type(node(1), Some(button.clone()));
    world.commit(queue).unwrap();
    let _ = world.take_system_work();

    let mut queue = MutationQueue::new();
    queue.set_component_type(node(1), Some(button));
    world.commit(queue).unwrap();
    assert!(world.take_system_work().is_empty());

    let mut queue = MutationQueue::new();
    queue.set_component_type(node(1), Some(ComponentTypeId::new("nana.select").unwrap()));
    world.commit(queue).unwrap();
    assert_eq!(
        world.component_type(node(1)).map(ComponentTypeId::as_str),
        Some("nana.select")
    );

    let mut queue = MutationQueue::new();
    queue.set_component_type(node(1), None);
    world.commit(queue).unwrap();
    let mut queue = MutationQueue::new();
    queue.set_component_type(node(1), None);
    world.commit(queue).unwrap();
    assert!(world.component_type(node(1)).is_none());
    assert!(world.take_system_work().is_empty());
}

#[test]
fn scheduled_ui_frames_are_zero_after_static_settle_and_nonzero_when_paint_stays_dirty() {
    const TICKS: usize = 8;
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=4 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
        if id > 1 {
            queue.insert(node(id / 2), node(id), None);
        }
    }
    world.commit(queue).unwrap();
    let _ = world.take_system_work();
    assert_eq!(world.scheduled_ui_frames(TICKS), 0);

    let mut paint = MutationQueue::new();
    paint.set_style(
        node(4),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                background: Some([0.2, 0.4, 0.8, 1.0]),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(paint).unwrap();
    assert_ne!(world.scheduled_ui_frames(TICKS), 0);
    assert_eq!(world.scheduled_ui_frames(TICKS), 0);
}

#[test]
fn sibling_reorder_does_not_recompute_unchanged_descendant_styles() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=4 {
        queue.create(node(id), document(1), NodeKind::Text);
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    queue.insert(node(2), node(4), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut queue = MutationQueue::new();
    queue.insert(node(1), node(3), Some(node(2)));
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert!(work.style.is_empty());
    assert!(work.text.is_empty());
    assert!(work.focus_ime.is_empty());
    assert_eq!(work.input_hit_test, vec![node(3)]);
    assert_eq!(work.layout, vec![node(1)]);
    assert_eq!(work.render_extraction, vec![node(1), node(3)]);
}

#[test]
fn node_ids_spread_across_the_bits_a_table_indexes_with() {
    use std::hash::{Hash, Hasher};

    // A hash table takes the low bits of the hash for the bucket and the top
    // seven for the control byte it compares first. Node ids are a dense
    // counter, so a hasher that passed them through would give every id in a
    // document the same control byte and make the first comparison useless.
    const IDS: u64 = 1 << 14;
    let mut buckets = [0usize; 1 << 10];
    let mut tags = [0usize; 128];
    for value in 1..=IDS {
        let mut hasher = IdHasher::default();
        node(value).hash(&mut hasher);
        let hash = hasher.finish();
        buckets[(hash & 0x3ff) as usize] += 1;
        tags[(hash >> 57) as usize] += 1;
    }
    // An odd multiplier permutes the low bits, so a dense range lands one per
    // bucket exactly.
    assert!(
        buckets
            .iter()
            .all(|count| *count == IDS as usize / buckets.len()),
        "the low bits must be a permutation of a dense range"
    );
    let ideal = IDS as usize / tags.len();
    let (low, high) = (
        *tags.iter().min().expect("tags"),
        *tags.iter().max().expect("tags"),
    );
    assert!(
        low >= ideal / 2 && high <= ideal * 2,
        "the control byte must vary too, got {low}..={high} against {ideal}"
    );
}

#[test]
fn turning_a_container_moves_its_subtree_without_re_extracting_it() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(node(id), document(1), NodeKind::Text);
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut turn = MutationQueue::new();
    turn.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform: Some(nana_ui_core::PaintTransform {
                    a: 0.0,
                    b: 1.0,
                    c: -1.0,
                    d: 0.0,
                    ..nana_ui_core::PaintTransform::default()
                }),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(turn).unwrap();
    let work = world.take_system_work();
    // The whole subtree moved in viewport space, which hit-testing and
    // accessibility both have to know.
    assert_eq!(work.transform, vec![node(1), node(2), node(3)]);
    assert_eq!(work.input_hit_test, vec![node(1), node(2), node(3)]);
    // The renderer does not: a descendant's primitives live in its own space
    // and are projected by the chain above it, which the scene re-projects
    // from the retained node rather than rebuilding.
    assert_eq!(work.render_extraction, vec![node(1)]);
    assert!(work.layout.is_empty());
}

#[test]
fn fading_a_container_resolves_its_subtree_without_re_extracting_it() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(node(id), document(1), NodeKind::Text);
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut fade = MutationQueue::new();
    fade.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                opacity: Some(0.8),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(fade).unwrap();
    let work = world.take_system_work();
    // The accumulated opacity is part of every descendant's resolved style, so
    // they still resolve.
    assert_eq!(work.style, vec![node(1), node(2), node(3)]);
    // But nothing downstream of extraction reads it, and a container with
    // descendants to fade is an opacity group: its opacity composites once at
    // the group rather than reaching each descendant's primitive. Extracting
    // them again would rebuild the same bytes, every frame of the fade.
    assert_eq!(work.render_extraction, vec![node(1)]);
    assert!(work.layout.is_empty());
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        world.extract_nodes(&[node(3)])[0].style.opacity,
        0.8,
        "the descendant still resolves the accumulated opacity"
    );
}

#[test]
fn paint_only_style_change_does_not_schedule_subtree_layout() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(node(id), document(1), NodeKind::Text);
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut paint = MutationQueue::new();
    paint.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                color: Some([0.2, 0.4, 0.8, 1.0]),
                opacity: Some(0.8),
                ..LayoutStyle::default()
            }),
            foreground: Some(SemanticColorRole::Accent),
            background: None,
            border: None,
            interaction: crate::InteractionStyle::default(),
            ..NodeStyle::default()
        },
    );
    world.commit(paint).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.style, vec![node(1), node(2), node(3)]);
    assert!(work.state.is_empty());
    assert!(work.transform.is_empty());
    assert!(work.text.is_empty());
    assert!(work.layout.is_empty());
    assert!(work.input_hit_test.is_empty());
    assert_eq!(work.render_extraction, vec![node(1), node(2), node(3)]);

    let mut layout = MutationQueue::new();
    layout.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                width: Some(LengthSpec::Px(240.0)),
                ..LayoutStyle::default()
            }),
            foreground: None,
            background: None,
            border: None,
            interaction: crate::InteractionStyle::default(),
            ..NodeStyle::default()
        },
    );
    world.commit(layout).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.layout, vec![node(1), node(2), node(3)]);
    assert_eq!(work.input_hit_test, vec![node(2), node(3)]);
}

#[test]
fn text_change_with_unchanged_intrinsic_does_not_propagate_layout() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.set_text(
        node(3),
        TextContent {
            value: "abc".into(),
        },
    );
    world.commit(queue).unwrap();
    let mut shaper = FunctionalShaper::default();
    drain_scheduled_text(&mut world, &mut shaper);

    let mut same_size = MutationQueue::new();
    same_size.set_text(
        node(3),
        TextContent {
            value: "xyz".into(),
        },
    );
    world.commit(same_size).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.text, vec![node(3)]);
    assert_eq!(work.render_extraction, vec![node(3)]);
    assert!(work.layout.is_empty());
    world.resolve_styles(&work.style).unwrap();
    world.shape_text(&work.text, &mut shaper).unwrap();
    let after_shape = world.take_system_work();
    assert!(after_shape.layout.is_empty());
    assert!(!after_shape.text.contains(&node(1)));
    assert!(!after_shape.layout.contains(&node(1)));
    assert!(!after_shape.layout.contains(&node(2)));
}

#[test]
fn text_change_that_changes_intrinsic_propagates_layout() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.set_text(
        node(3),
        TextContent {
            value: "abc".into(),
        },
    );
    world.commit(queue).unwrap();
    let mut shaper = FunctionalShaper::default();
    drain_scheduled_text(&mut world, &mut shaper);

    let mut longer = MutationQueue::new();
    longer.set_text(
        node(3),
        TextContent {
            value: "abcdef".into(),
        },
    );
    world.commit(longer).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.text, vec![node(3)]);
    assert!(work.layout.is_empty());
    world.resolve_styles(&work.style).unwrap();
    world.shape_text(&work.text, &mut shaper).unwrap();
    let after_shape = world.take_system_work();
    assert_eq!(after_shape.layout, vec![node(1), node(2), node(3)]);
}

#[test]
fn wrapping_text_same_unconstrained_width_propagates_layout_when_wrap_height_changes() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.set_style(
        node(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                font_size: Some(10.0),
                width: Some(LengthSpec::Px(40.0)),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text(
        node(3),
        TextContent {
            value: "aaaaaaaa".into(),
        },
    );
    world.commit(queue).unwrap();
    let mut shaper = WordWrapShaper;
    drain_scheduled_text(&mut world, &mut shaper);
    let mut place = MutationQueue::new();
    place.write_layout(
        node(3),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 10.0,
        },
    );
    world.commit(place).unwrap();
    world.take_system_work();
    world.shape_text(&[node(3)], &mut shaper).unwrap();
    world.take_system_work();

    let mut wrapped = MutationQueue::new();
    wrapped.set_text(
        node(3),
        TextContent {
            value: "aa aa aa".into(),
        },
    );
    world.commit(wrapped).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.text, vec![node(3)]);
    assert!(work.layout.is_empty());
    world.resolve_styles(&work.style).unwrap();
    world.shape_text(&work.text, &mut shaper).unwrap();
    let after_shape = world.take_system_work();
    assert!(after_shape.layout.contains(&node(1)));
    assert!(after_shape.layout.contains(&node(2)));
    assert!(after_shape.layout.contains(&node(3)));
}

#[test]
fn em_padding_wrap_and_card_content_use_computed_font_size() {
    #[derive(Default)]
    struct ConstraintProbe {
        constraints: Vec<crate::TextShapeConstraints>,
    }

    impl TextShaper for ConstraintProbe {
        fn shape(
            &mut self,
            _id: StableNodeId,
            _text: &TextContent,
            _style: &ComputedStyle,
            constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            self.constraints.push(constraints);
            TextMetrics::default()
        }
    }

    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "card".into() },
    );
    let layout = Arc::new(LayoutStyle {
        font_size: Some(32.0),
        padding: Some(LengthSpec::Em(1.0)),
        ..LayoutStyle::default()
    });
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::clone(&layout),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        node(2),
        NodeStyle {
            layout,
            ..NodeStyle::default()
        },
    );
    queue.set_text(
        node(1),
        TextContent {
            value: "wrap".into(),
        },
    );
    queue.set_standard_visual(
        node(2),
        Some(StandardVisual::Card {
            title: None,
            kind: nana_ui_core::CardKind::Surface,
            loading: false,
            loading_phase: 0.0,
        }),
    );
    let box_200 = LayoutBox {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 80.0,
    };
    queue.write_layout(node(1), box_200);
    queue.write_layout(node(2), box_200);
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1), node(2)]).unwrap();

    let mut probe = ConstraintProbe::default();
    world.shape_text(&[node(1)], &mut probe).unwrap();
    assert_eq!(
        probe
            .constraints
            .last()
            .map(|constraints| constraints.max_width),
        Some(Some(136.0)),
        "1em padding at font-size 32px must wrap at 200-32-32, not 200-16-16"
    );

    let crate::ComponentGeometry::Card { content, .. } =
        world.component_geometry(node(2)).expect("card geometry")
    else {
        panic!("expected card geometry");
    };
    assert_eq!(content.x, 32.0);
    assert_eq!(content.y, 32.0);
    assert_eq!(content.width, 136.0);
    assert_eq!(content.height, 16.0);
}

#[test]
fn work_counters_are_queryable_after_drain_and_extract() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(node(id), document(1), NodeKind::Text);
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();

    let mut work = world.take_system_work();
    let counters = work.counters();
    assert_eq!(counters.entities_total, 3);
    assert_eq!(counters.entities_spawned, 3);
    assert_eq!(counters.entities_despawned, 0);
    assert_eq!(counters.entities_changed, 3);
    assert_eq!(counters.style_processed, 3);
    assert_eq!(counters.layout_nodes, 3);
    assert_eq!(counters.render_nodes_changed, 3);
    assert_eq!(counters.render_nodes_extracted, 3);
    assert_eq!(counters.input_targets, 0);
    assert!(counters.allocations > 0);
    assert!(counters.allocated_bytes > 0);
    assert_eq!(counters.text_shaped_runs, 0);
    assert_eq!(world.last_work_counters().entities_changed, 3);
    assert_eq!(world.last_work_counters().render_nodes_changed, 3);
    assert_eq!(world.last_work_counters().render_nodes_extracted, 0);

    world.resolve_styles(&work.style).unwrap();
    let extracted = world.extract_nodes(&work.render_extraction);
    work.record_extract(&extracted);
    world.record_extract(&extracted);
    assert_eq!(work.counters().render_nodes_extracted, extracted.len());
    assert_eq!(work.counters().render_nodes_changed, 3);
    assert_eq!(
        world.last_work_counters().render_nodes_extracted,
        extracted.len()
    );
    assert_eq!(world.last_work_counters().render_nodes_changed, 3);

    let idle = world.take_system_work();
    assert!(idle.is_empty());
    assert_eq!(idle.counters().entities_spawned, 0);
    assert_eq!(idle.counters().entities_changed, 0);
    assert_eq!(idle.counters().allocations, 0);
    assert_eq!(idle.counters().text_shaped_runs, 0);
    assert_eq!(world.last_work_counters().entities_changed, 3);
    assert_eq!(
        world.last_work_counters().render_nodes_extracted,
        extracted.len()
    );
}

#[test]
fn hot_path_allocations_and_text_shape_are_idle_zero_and_rise_on_mutation() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "hello".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert!(work.counters().allocations > 0);
    assert!(work.counters().allocated_bytes > 0);
    assert_eq!(work.counters().text_shaped_runs, 0);
    world.resolve_styles(&work.style).unwrap();
    world
        .shape_text(&work.text, &mut FunctionalShaper::default())
        .unwrap();
    let after_shape = world.last_work_counters();
    assert!(after_shape.text_shaped_runs > 0);
    assert!(after_shape.text_layout_cache_misses > 0);
    assert_eq!(after_shape.text_layout_cache_hits, 0);
    assert!(after_shape.allocations > 0);

    let _ = world.layout_inputs(&work.layout).unwrap();
    let after_layout = world.last_work_counters();
    assert!(after_layout.allocations >= after_shape.allocations);

    let idle = {
        let mut idle = None;
        for _ in 0..8 {
            let work = world.take_system_work();
            if work.is_empty() {
                idle = Some(work);
                break;
            }
        }
        idle.expect("mutation follow-up work must settle")
    };
    assert_eq!(idle.counters().allocations, 0);
    assert_eq!(idle.counters().allocated_bytes, 0);
    assert_eq!(idle.counters().text_shaped_runs, 0);
    assert_eq!(idle.counters().text_layout_cache_misses, 0);
    assert!(world.last_work_counters().allocations > 0);

    let mut patch = MutationQueue::new();
    patch.set_text(
        node(1),
        TextContent {
            value: "world".into(),
        },
    );
    world.commit(patch).unwrap();
    let mutated = world.take_system_work();
    assert!(!mutated.text.is_empty());
    assert!(mutated.counters().allocations > 0);
    world.resolve_styles(&mutated.style).unwrap();
    world
        .shape_text(&mutated.text, &mut FunctionalShaper::default())
        .unwrap();
    let mutated_shape = world.last_work_counters();
    assert!(mutated_shape.text_shaped_runs > 0);
    assert!(mutated_shape.text_layout_cache_misses > 0);
}

#[test]
fn text_layout_cache_miss_then_hit_and_shaper_without_glyph_backend_omits_glyph_cache() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "cache-me".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(work.counters().glyph_cache_hits, None);
    assert_eq!(work.counters().glyph_cache_misses, None);
    assert_eq!(work.counters().cache_eviction, None);

    // Default TextShaper::shape_cached ignores GlyphCache.
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&work.text, &mut shaper).unwrap();
    let missed = world.last_work_counters();
    assert!(missed.text_layout_cache_misses >= 1);
    assert_eq!(missed.text_layout_cache_hits, 0);
    assert!(missed.text_shaped_runs >= 1);
    assert_eq!(missed.glyph_cache_hits, None);
    assert_eq!(missed.glyph_cache_misses, None);
    assert_eq!(missed.cache_eviction, Some(0));

    // The same node again is not a cache question at all: nothing about it
    // changed, so the pass skips it on revision before building a key.
    world.shape_text(&work.text, &mut shaper).unwrap();
    let repeat = world.last_text_work_counters();
    assert_eq!(repeat.text_nodes_revision_skipped, 1);
    assert_eq!(repeat.text_source_clones, 0);

    // Another node with the same text and style is the cache hit.
    let mut twin = MutationQueue::new();
    twin.create(node(2), document(1), NodeKind::Text);
    twin.set_text(
        node(2),
        TextContent {
            value: "cache-me".into(),
        },
    );
    world.commit(twin).unwrap();
    let twin_work = world.take_system_work();
    world.resolve_styles(&twin_work.style).unwrap();
    world.shape_text(&twin_work.text, &mut shaper).unwrap();
    let hit = world.last_work_counters();
    assert!(hit.text_layout_cache_hits >= 1);
    assert_eq!(hit.text_layout_cache_misses, 0);
    assert_eq!(hit.text_shaped_runs, 0);
    assert_eq!(hit.glyph_cache_hits, None);
    assert_eq!(hit.cache_eviction, Some(0));

    let mut place = MutationQueue::new();
    place.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 24.0,
            height: 16.0,
        },
    );
    world.commit(place).unwrap();
    world.take_system_work();
    world
        .shape_text_for_layout(document(1), &mut shaper)
        .unwrap();
    let wrapped = world.last_work_counters();
    assert!(
        wrapped.text_layout_cache_misses >= 1,
        "max_width / wrap must miss the unconstrained cache entry"
    );
    let wrapped_hits = wrapped.text_layout_cache_hits;
    let wrapped_runs = wrapped.text_shaped_runs;
    crate::text_shape_stats::reset();
    world
        .shape_text_for_layout(document(1), &mut shaper)
        .unwrap();
    let wrapped_hit = world.last_work_counters();
    let skipped = crate::text_shape_stats::snapshot();
    assert_eq!(
        wrapped_hit.text_layout_cache_hits, wrapped_hits,
        "unchanged wrap constraints must skip cache lookup"
    );
    assert_eq!(wrapped_hit.text_shaped_runs, wrapped_runs);
    assert!(skipped.skipped_unchanged >= 1);
    assert_eq!(skipped.key_builds, 0);
    assert_eq!(skipped.cache_lookups, 0);
    assert_eq!(wrapped_hit.glyph_cache_hits, None);
}

#[test]
fn layout_scoped_shape_skips_empty_text_before_clone_or_lookup() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    crate::text_shape_stats::reset();
    world
        .shape_text_for_layout(document(1), &mut FunctionalShaper::default())
        .unwrap();
    let stats = crate::text_shape_stats::snapshot();
    let counters = world.last_work_counters();
    assert_eq!(stats.nonempty_text_nodes, 0);
    assert_eq!(stats.string_clones, 0);
    assert_eq!(stats.key_builds, 0);
    assert_eq!(stats.cache_lookups, 0);
    assert_eq!(counters.text_shaped_runs, 0);
    assert_eq!(counters.text_layout_cache_hits, 0);
    assert_eq!(counters.text_layout_cache_misses, 0);
}

#[test]
fn layout_scoped_shape_skips_plain_text_when_only_box_y_moves() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "row 0".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&work.text, &mut shaper).unwrap();
    let mut place = MutationQueue::new();
    place.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 16.0,
        },
    );
    world.commit(place).unwrap();
    world.take_system_work();
    world
        .shape_text_for_layout(document(1), &mut shaper)
        .unwrap();

    let mut shifted = MutationQueue::new();
    shifted.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 40.0,
            width: 80.0,
            height: 16.0,
        },
    );
    world.commit(shifted).unwrap();
    world.take_system_work();
    crate::text_shape_stats::reset();
    world
        .shape_text_for_layout_scoped(&[node(1)], &mut shaper)
        .unwrap();
    let skipped = crate::text_shape_stats::snapshot();
    let after_shift = world.last_work_counters();
    assert_eq!(after_shift.text_shaped_runs, 0);
    assert_eq!(after_shift.text_layout_cache_hits, 0);
    assert_eq!(after_shift.text_layout_cache_misses, 0);
    assert_eq!(skipped.string_clones, 0);
    assert_eq!(skipped.key_builds, 0);
    assert_eq!(skipped.cache_lookups, 0);
    assert_eq!(skipped.skipped_unchanged, 1);
}

#[test]
fn layout_scoped_shape_reshapes_when_wrap_width_or_text_changes() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "a long wrapping line of text".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&work.text, &mut shaper).unwrap();
    let mut place = MutationQueue::new();
    place.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 16.0,
        },
    );
    world.commit(place).unwrap();
    world.take_system_work();
    world
        .shape_text_for_layout(document(1), &mut shaper)
        .unwrap();

    let mut wider = MutationQueue::new();
    wider.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 16.0,
        },
    );
    world.commit(wider).unwrap();
    world.take_system_work();
    crate::text_shape_stats::reset();
    world
        .shape_text_for_layout_scoped(&[node(1)], &mut shaper)
        .unwrap();
    let after_width = world.last_work_counters();
    let width_stats = crate::text_shape_stats::snapshot();
    assert!(after_width.text_layout_cache_misses >= 1);
    assert_eq!(width_stats.skipped_unchanged, 0);
    assert_eq!(width_stats.key_builds, 1);
    assert_eq!(width_stats.cache_lookups, 1);

    let mut rewrite = MutationQueue::new();
    rewrite.set_text(
        node(1),
        TextContent {
            value: "short".into(),
        },
    );
    world.commit(rewrite).unwrap();
    let mutated = world.take_system_work();
    world.resolve_styles(&mutated.style).unwrap();
    crate::text_shape_stats::reset();
    world.shape_text(&mutated.text, &mut shaper).unwrap();
    let after_text = crate::text_shape_stats::snapshot();
    assert_eq!(after_text.skipped_unchanged, 0);
    assert!(after_text.key_builds >= 1);
    assert!(after_text.cache_lookups >= 1);
    // The scheduled pass already resolved the new text against the unchanged
    // box, so the layout-scoped pass has nothing left to do.
    crate::text_shape_stats::reset();
    world
        .shape_text_for_layout_scoped(&[node(1)], &mut shaper)
        .unwrap();
    let rescoped = crate::text_shape_stats::snapshot();
    assert_eq!(rescoped.skipped_unchanged, 1);
    assert_eq!(rescoped.key_builds, 0);
}

#[test]
fn layout_scoped_shape_duplicate_and_long_text_skip_without_rebuilding_keys() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.create(node(2), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "same".into(),
        },
    );
    queue.set_text(
        node(2),
        TextContent {
            value: "same".into(),
        },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.set_text(
        node(3),
        TextContent {
            value: "L".repeat(4096).into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&work.text, &mut shaper).unwrap();
    let scheduled = world.last_work_counters();
    assert!(scheduled.text_layout_cache_hits >= 1);
    assert!(scheduled.text_layout_cache_misses >= 2);

    for id in [1u64, 2, 3] {
        let mut place = MutationQueue::new();
        place.write_layout(
            node(id),
            LayoutBox {
                x: 0.0,
                y: (id as f32) * 20.0,
                width: 80.0,
                height: 16.0,
            },
        );
        world.commit(place).unwrap();
    }
    world.take_system_work();
    world
        .shape_text_for_layout(document(1), &mut shaper)
        .unwrap();
    crate::text_shape_stats::reset();
    world
        .shape_text_for_layout_scoped(&[node(1), node(2), node(3)], &mut shaper)
        .unwrap();
    let skipped = crate::text_shape_stats::snapshot();
    assert_eq!(skipped.skipped_unchanged, 3);
    assert_eq!(skipped.key_builds, 0);
    assert_eq!(skipped.cache_lookups, 0);
    assert_eq!(skipped.string_clones, 0);
}

#[test]
fn glyph_cache_miss_then_hit_on_measure_text_shaper() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(node(1), TextContent { value: "ab".into() });
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(work.counters().glyph_cache_hits, None);
    assert_eq!(work.counters().glyph_cache_misses, None);

    // text-table / framework bench production shaper, via UiWorld::shape_text.
    let mut shaper = MeasureTextShaper;
    world.shape_text(&work.text, &mut shaper).unwrap();
    let missed = world.last_work_counters();
    assert_eq!(missed.glyph_cache_misses, Some(2));
    assert_eq!(missed.glyph_cache_hits, Some(0));
    assert!(missed.text_layout_cache_misses >= 1);

    // Unchanged: skipped on revision, so neither cache is consulted again.
    world.shape_text(&work.text, &mut shaper).unwrap();
    assert_eq!(
        world.last_text_work_counters().text_nodes_revision_skipped,
        1
    );

    let mut patch = MutationQueue::new();
    patch.set_text(node(1), TextContent { value: "ba".into() });
    world.commit(patch).unwrap();
    let reused = world.take_system_work();
    world.resolve_styles(&reused.style).unwrap();
    world.shape_text(&reused.text, &mut shaper).unwrap();
    let hit = world.last_work_counters();
    assert_eq!(hit.glyph_cache_hits, Some(2));
    assert_eq!(hit.glyph_cache_misses, Some(0));
    assert!(hit.text_layout_cache_misses >= 1);
}

fn confirm_modal_visual() -> StandardVisual {
    StandardVisual::ModalFrame {
        title: Arc::from("Confirm"),
        description: None,
        body_text: None,
        kind: crate::ModalSurfaceKind::Confirm(nana_ui_core::DialogSize::Compact),
        busy: false,
        danger: false,
        slots: crate::ModalSlots::default(),
    }
}

fn clip_empty_state_visual() -> StandardVisual {
    StandardVisual::EmptyState {
        title: Arc::from("Empty"),
        message: None,
        icon: None,
        compact: true,
        action: None,
    }
}

fn open_menu_overlay_visual() -> StandardVisual {
    StandardVisual::MenuSurface {
        kind: crate::MenuSurfaceKind::HoverCard,
        open: true,
        trigger: None,
        trigger_icon: None,
        trigger_image: None,
        gap: 0.0,
        overlay: Some(crate::TriggeredMenuOverlay {
            placement: nana_ui_core::PopoverPlacement::Bottom,
            alignment: nana_ui_core::PopoverAlignment::Center,
            width: 240.0,
            padding: 0.0,
            gap: 6.0,
            trigger_content: None,
        }),
        query: None,
        rows: Arc::from([]),
        highlighted: None,
    }
}

#[test]
fn parking_or_removing_the_last_presence_node_returns_the_skip_path() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "dialog".into(),
        },
    );
    queue.create(
        node(3),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.create(
        node(4),
        document(1),
        NodeKind::Element {
            tag: "section".into(),
        },
    );
    queue.create(
        node(5),
        document(1),
        NodeKind::Element {
            tag: "hover-card".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    queue.insert(node(1), node(4), None);
    queue.insert(node(1), node(5), None);
    queue.set_standard_visual(node(2), Some(confirm_modal_visual()));
    queue.set_standard_visual(node(3), Some(clip_empty_state_visual()));
    queue.set_style(
        node(4),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                z_index: Some(4),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_standard_visual(node(5), Some(open_menu_overlay_visual()));
    world.commit(queue).unwrap();
    world.take_system_work();
    assert_eq!(world.confirm_modals, 1);
    assert_eq!(world.clip_visuals, 2);
    assert_eq!(world.z_index_nodes, 2);
    assert!(world.confirm_action_effect(node(3)).is_none());

    let mut park = MutationQueue::new();
    park.park_subtree(node(2));
    park.park_subtree(node(3));
    park.park_subtree(node(4));
    park.park_subtree(node(5));
    world.commit(park).unwrap();
    assert_eq!(world.confirm_modals, 0);
    assert_eq!(world.clip_visuals, 0);
    assert_eq!(world.z_index_nodes, 0);
    assert!(world.confirm_action_effect(node(1)).is_none());
    let remaining = world.extract_nodes(&[node(1)]);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].z_index, 0);

    let mut remount = MutationQueue::new();
    remount.insert(node(1), node(2), None);
    remount.insert(node(1), node(3), None);
    remount.insert(node(1), node(4), None);
    remount.insert(node(1), node(5), None);
    world.commit(remount).unwrap();
    assert_eq!(world.confirm_modals, 1);
    assert_eq!(world.clip_visuals, 2);
    assert_eq!(world.z_index_nodes, 2);

    let mut remove = MutationQueue::new();
    remove.detach(node(2));
    remove.detach(node(3));
    remove.detach(node(4));
    remove.detach(node(5));
    world.commit(remove).unwrap();
    assert_eq!(world.confirm_modals, 0);
    assert_eq!(world.clip_visuals, 0);
    assert_eq!(world.z_index_nodes, 0);
    assert!(world.confirm_action_effect(node(1)).is_none());
}

#[test]
fn transform_and_a11y_mutations_do_not_schedule_layout() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=7 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    queue.insert(node(2), node(4), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut transform = MutationQueue::new();
    transform.set_style(
        node(4),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform: Some(PaintTransform {
                    e: 8.0,
                    ..PaintTransform::default()
                }),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(transform).unwrap();
    let work = world.take_system_work();
    assert!(work.style.is_empty());
    assert!(work.state.is_empty());
    assert!(work.layout.is_empty());
    assert_eq!(work.counters().style_processed, 0);
    assert_eq!(work.counters().layout_nodes, 0);
    assert_eq!(work.transform, vec![node(4)]);
    assert_eq!(work.input_hit_test, vec![node(4)]);
    assert_eq!(work.render_extraction, vec![node(4)]);
    world.restore_system_work(work.clone());
    let restored = world.take_system_work();
    assert_eq!(restored.transform, work.transform);
    assert!(restored.style.is_empty());
    assert!(restored.layout.is_empty());

    let mut accessibility = MutationQueue::new();
    accessibility.set_accessibility(
        node(3),
        AccessibilityState {
            role: AccessibilityRole::Generic,
            label: Some(Arc::from("beta")),
            ..AccessibilityState::default()
        },
    );
    world.commit(accessibility).unwrap();
    let work = world.take_system_work();
    assert!(work.layout.is_empty());
    assert_eq!(work.accessibility, vec![node(3)]);
}

#[test]
fn frame_profiler_times_separable_runtime_stages() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    world.commit(queue).unwrap();
    let work = world.take_system_work();

    let mut profiler = crate::FrameProfiler::new();
    profiler.mark_runtime_unsupported();
    profiler.time(crate::FrameStage::Style, || {
        world.resolve_styles(&work.style).unwrap();
    });
    profiler.time(crate::FrameStage::TextShape, || {
        world
            .shape_text(&work.text, &mut FunctionalShaper::default())
            .unwrap();
    });
    if work.layout.is_empty() {
        profiler.skip(crate::FrameStage::Layout);
    } else {
        profiler.time(crate::FrameStage::Layout, || {
            world.layout_inputs(&work.layout).unwrap();
        });
    }
    profiler.time(crate::FrameStage::Extract, || {
        let extracted = world.extract_nodes(&work.render_extraction);
        world.record_extract(&extracted);
    });

    let profile = profiler.finish();
    assert_eq!(
        profile.stage(crate::FrameStage::Style).unwrap().status,
        crate::StageStatus::Ran
    );
    assert_eq!(
        profile.stage(crate::FrameStage::TextShape).unwrap().status,
        crate::StageStatus::Ran
    );
    assert_eq!(
        profile.stage(crate::FrameStage::Layout).unwrap().status,
        crate::StageStatus::Ran
    );
    assert_eq!(
        profile.stage(crate::FrameStage::GpuUpload).unwrap().status,
        crate::StageStatus::Unsupported
    );
    assert_eq!(
        profile.stage(crate::FrameStage::Batch).unwrap().status,
        crate::StageStatus::Unsupported
    );
}

fn drain_scheduled_text(world: &mut UiWorld, shaper: &mut impl TextShaper) {
    for _ in 0..8 {
        let work = world.take_system_work();
        if work.is_empty() {
            return;
        }
        world.resolve_styles(&work.style).unwrap();
        if !work.text.is_empty() {
            world.shape_text(&work.text, shaper).unwrap();
        }
    }
    panic!("scheduled text work did not settle");
}

#[test]
fn hit_test_respects_cumulative_transform_and_ancestor_clip() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        },
    );
    queue.write_layout(
        node(2),
        LayoutBox {
            x: 40.0,
            y: 0.0,
            width: 30.0,
            height: 30.0,
        },
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_x: OverflowSpec::Hidden,
                transform: Some(PaintTransform {
                    e: 10.0,
                    ..PaintTransform::default()
                }),
                ..LayoutStyle::default()
            }),
            foreground: None,
            background: None,
            border: None,
            interaction: crate::InteractionStyle::default(),
            ..NodeStyle::default()
        },
    );
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
    );
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document(1));

    assert_eq!(world.hit_test(document(1), 55.0, 10.0), Some(node(2)));
    assert_eq!(world.hit_test(document(1), 45.0, 10.0), None);
    assert_eq!(world.hit_test(document(1), 65.0, 10.0), None);
}

#[test]
fn hit_test_follows_perspective_rotate_y_homography() {
    let mat = PaintMat4::perspective(800.0)
        .expect("d")
        .then(PaintMat4::rotate_y(30_f32.to_radians()));
    let layout = LayoutBox {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 80.0,
    };
    let (transform, persp) = LayoutStyle {
        transform_3d: Some(mat),
        ..LayoutStyle::default()
    }
    .world_scene_transform(layout.x, layout.y, layout.width, layout.height)
    .expect("homography");
    assert!(
        persp[0].abs() > 1e-8 || persp[1].abs() > 1e-8,
        "perspective+rotateY must keep (g,h)"
    );
    assert!(
        transformed_contains(layout, transform, persp, 100.0, 40.0),
        "projected center must hit"
    );
    let corners = mat
        .around_origin(0.0, 0.0, 100.0, 40.0)
        .projected_corners(0.0, 0.0, 200.0, 80.0)
        .expect("corners");
    let max_x = corners.iter().map(|p| p[0]).fold(f32::MIN, f32::max);
    let min_y = corners.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
    let empty_corner_x = max_x - 1.0;
    let empty_corner_y = min_y + 1.0;
    assert!(
        !transformed_contains(layout, transform, persp, empty_corner_x, empty_corner_y),
        "trapezoid AABB empty corner must miss, probe=({empty_corner_x}, {empty_corner_y}) corners={corners:?}"
    );

    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.write_layout(node(1), layout);
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                transform_3d: Some(mat),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document(1));
    assert_eq!(world.hit_test(document(1), 100.0, 40.0), Some(node(1)));
    let local = (70.0, 20.0);
    let projected = world
        .layout_pointer_position(node(1), local.0, local.1)
        .unwrap();
    assert_eq!(
        world.hit_test(document(1), projected.0, projected.1),
        Some(node(1))
    );
    let restored = world
        .pointer_layout_position(node(1), projected.0, projected.1)
        .unwrap();
    assert!((restored.0 - local.0).abs() < 0.001);
    assert!((restored.1 - local.1).abs() < 0.001);
    assert!(
        world
            .pointer_layout_position(node(1), f32::NAN, 0.0)
            .is_none()
    );
    assert_eq!(
        world.hit_test(document(1), empty_corner_x, empty_corner_y),
        None
    );
}

#[test]
fn hit_test_skips_css_pointer_events_none() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        },
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                pointer_events: Some(PointerEventsSpec::None),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document(1));
    assert_eq!(world.hit_test(document(1), 10.0, 10.0), None);
}

#[test]
fn hit_test_walks_z_order_and_skips_clipped_subtrees() {
    fn box_at(x: f32, y: f32, width: f32, height: f32) -> LayoutBox {
        LayoutBox {
            x,
            y,
            width,
            height,
        }
    }
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "root".into() },
    );
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "clip".into() },
    );
    queue.create(
        node(3),
        document(1),
        NodeKind::Element {
            tag: "inside".into(),
        },
    );
    queue.create(
        node(4),
        document(1),
        NodeKind::Element {
            tag: "lower".into(),
        },
    );
    queue.create(
        node(5),
        document(1),
        NodeKind::Element {
            tag: "upper".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.insert(node(1), node(4), None);
    queue.insert(node(1), node(5), None);
    queue.write_layout(node(1), box_at(0.0, 0.0, 100.0, 100.0));
    queue.write_layout(node(2), box_at(0.0, 0.0, 40.0, 40.0));
    queue.write_layout(node(3), box_at(30.0, 0.0, 40.0, 20.0));
    queue.write_layout(node(4), box_at(50.0, 50.0, 40.0, 40.0));
    queue.write_layout(node(5), box_at(60.0, 50.0, 40.0, 40.0));
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
    );
    queue.set_interaction(
        node(2),
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
    );
    queue.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_x: OverflowSpec::Hidden,
                overflow_y: OverflowSpec::Hidden,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    let mut raised = NodeStyle::default();
    Arc::make_mut(&mut raised.layout).z_index = Some(2);
    queue.set_style(node(4), raised);
    world.commit(queue).unwrap();
    world.rebuild_hit_test(document(1));

    assert_eq!(world.hit_test(document(1), 35.0, 10.0), Some(node(3)));
    assert_eq!(world.hit_test(document(1), 50.0, 10.0), None);
    assert_eq!(world.hit_test(document(1), 70.0, 60.0), Some(node(4)));
    let overlap = world.hit_test_candidates(document(1), 70.0, 60.0);
    assert_eq!(overlap.first().copied(), Some(node(4)));
    assert!(overlap.contains(&node(5)));
}

/// Issue #101 §4 baseline: a palette-only theme switch is paint work. It
/// invalidates every live node for paint, but it must not reach Layout or
/// reshape text.
///
/// It also pins where the palette is re-read today. `SetTheme` schedules
/// RENDER without STYLE, so no style pass runs and the new colours are
/// resolved inside **extract** instead — the renderer-side theme read Issue
/// #100 §6 removes. The baseline records that as it is rather than asserting
/// the target shape.
#[test]
fn palette_only_theme_switch_is_paint_work_and_never_layout_or_reshape() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.set_style(
        node(1),
        NodeStyle {
            background: Some(SemanticColorRole::Accent),
            ..NodeStyle::default()
        },
    );
    queue.create(node(2), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    world.begin_frame_counters();
    let mut theme = MutationQueue::new();
    theme.set_theme(ThemeMode::Light);
    world.commit(theme).unwrap();
    let work = world.take_system_work();
    assert!(
        work.style.is_empty(),
        "SetTheme schedules RENDER, not STYLE"
    );
    assert!(work.layout.is_empty());
    world.resolve_styles(&work.style).unwrap();
    let counters = world.last_theme_work_counters();
    assert_eq!(
        counters.paint_nodes_from_style, 2,
        "both live nodes repaint"
    );
    assert_eq!(counters.layout_nodes_from_style, 0);
    assert_eq!(counters.text_nodes_from_style, 0, "colour is not a reshape");
    assert!(counters.is_paint_only());
    assert_eq!(
        counters.style_nodes_considered, 0,
        "no style pass ran for this switch"
    );

    let extracted = world.extract_nodes(&work.render_extraction);
    world.end_frame_counters();
    assert_eq!(
        extracted[0].style.background,
        Some(
            nana_ui_core::SemanticPalette::light()
                .accent
                .as_rgba_array()
        )
    );
    assert!(
        world.last_theme_work_counters().theme_reads > 0,
        "the palette is re-read, and today extract is where"
    );
}

/// The same switch with different metrics is Layout work as well. The two
/// classes are separate counters so a later dependency-scoped resolver can be
/// held to "palette-only stayed paint-only".
#[test]
fn metrics_change_adds_layout_invalidation_to_the_same_theme_install() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.control_height += 4.0;
    world.begin_frame_counters();
    let mut tokens = MutationQueue::new();
    tokens.set_theme_tokens(test_theme(
        ThemeMode::Dark,
        metrics,
        nana_ui_core::SemanticPalette::dark(),
        nana_ui_core::SemanticPalette::dark().surface,
    ));
    world.commit(tokens).unwrap();
    world.end_frame_counters();

    let counters = world.last_theme_work_counters();
    assert_eq!(counters.layout_nodes_from_style, 1);
    assert_eq!(counters.paint_nodes_from_style, 1);
    assert!(!counters.is_paint_only());
}

/// Issue #100 §15: a retained frame that changed nothing must not walk or
/// re-read the theme. `considered` counts nodes the pass evaluated, and it
/// splits exactly into resolved plus skipped.
#[test]
fn style_pass_splits_into_resolved_and_skipped_and_an_idle_pass_does_neither() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();

    world.begin_frame_counters();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.end_frame_counters();
    let first = world.last_theme_work_counters();
    assert_eq!(first.style_nodes_considered, 3);
    assert_eq!(first.style_nodes_resolved, 3);
    assert_eq!(first.style_nodes_skipped, 0);
    assert!(first.style_allocations > 0);

    // Resolving the same ids again answers every one from the value/epoch
    // fast path; nothing is published a second time.
    world.begin_frame_counters();
    world.resolve_styles(&[node(1), node(2), node(3)]).unwrap();
    world.end_frame_counters();
    let second = world.last_theme_work_counters();
    assert_eq!(second.style_nodes_considered, 3);
    assert_eq!(second.style_nodes_resolved, 0);
    assert_eq!(second.style_nodes_skipped, 3);
    assert_eq!(second.style_allocations, 0);

    // An empty pass is the steady frame: no walk, no token read.
    world.begin_frame_counters();
    world.resolve_styles(&[]).unwrap();
    world.end_frame_counters();
    assert!(world.last_theme_work_counters().is_idle());
}

/// A node pulled in only as a parent is counted once for the pass, not once
/// per descendant that needed it.
#[test]
fn parent_chain_nodes_are_counted_once_per_style_pass() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=4 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.insert(node(2), node(4), None);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    world.begin_frame_counters();
    // Both leaves walk up through the same two ancestors.
    world.resolve_styles(&[node(3), node(4)]).unwrap();
    world.end_frame_counters();
    let counters = world.last_theme_work_counters();
    assert_eq!(counters.style_nodes_considered, 4);
    assert_eq!(
        counters.style_nodes_considered,
        counters.style_nodes_resolved + counters.style_nodes_skipped
    );
}

/// Resolving design intent copies a `LayoutStyle`, which is by far the
/// largest heap event on the style path. It happens under `Arc::make_mut`,
/// so no allocator counter sees it — the style counters have to say it
/// themselves, or a baseline recorded from them understates the write path.
#[test]
fn resolving_layout_intent_reports_the_layout_it_had_to_copy() {
    fn allocated(style: NodeStyle) -> (usize, usize) {
        let mut world = UiWorld::new();
        let mut queue = MutationQueue::new();
        queue.create(
            node(1),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
        world.commit(queue).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();

        let mut styled = MutationQueue::new();
        styled.set_style(node(1), style);
        world.begin_frame_counters();
        world.commit(styled).unwrap();
        let work = world.take_system_work();
        world.resolve_styles(&work.style).unwrap();
        world.end_frame_counters();
        let counters = world.last_theme_work_counters();
        (counters.layout_copies, counters.layout_copied_bytes)
    }

    let plain = NodeStyle::default();
    let with_intent = NodeStyle {
        control_height: Some(nana_ui_core::ControlHeight::Min(
            nana_ui_core::ControlSize::Large,
        )),
        ..NodeStyle::default()
    };
    assert_eq!(
        allocated(plain),
        (0, 0),
        "a node with no design intent shares the layout it authored"
    );
    assert_eq!(
        allocated(with_intent),
        (1, size_of::<LayoutStyle>()),
        "a node with intent copies one layout, and reports its real size"
    );
}

/// A font-size change is Text work; the counter separates it from the paint
/// classes so "palette switch does not reshape" stays a real assertion.
#[test]
fn a_shaping_style_change_counts_as_text_invalidation() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "hello".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    let mut resize = MutationQueue::new();
    resize.set_style(
        node(1),
        NodeStyle {
            layout: LayoutStyle {
                font_size: Some(21.0),
                ..LayoutStyle::default()
            }
            .into(),
            ..NodeStyle::default()
        },
    );
    world.commit(resize).unwrap();
    world.begin_frame_counters();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    world.end_frame_counters();
    assert_eq!(world.last_theme_work_counters().text_nodes_from_style, 1);
}

#[test]
fn set_theme_marks_render_not_style_when_only_palette_roles_change() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.set_style(
        node(1),
        NodeStyle {
            background: Some(SemanticColorRole::Accent),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        world.extract_nodes(&[node(1)])[0].style.background,
        Some(nana_ui_core::SemanticPalette::dark().accent.as_rgba_array())
    );

    let mut theme = MutationQueue::new();
    theme.set_theme(ThemeMode::Light);
    world.commit(theme).unwrap();
    let work = world.take_system_work();
    assert!(work.style.is_empty());
    assert!(work.layout.is_empty());
    assert_eq!(work.render_extraction, vec![node(1)]);
    assert_eq!(
        world.extract_nodes(&work.render_extraction)[0]
            .style
            .background,
        Some(
            nana_ui_core::SemanticPalette::light()
                .accent
                .as_rgba_array()
        )
    );
}

#[test]
fn style_tokens_drive_surface_background_and_titlebar_extract_alphas() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "sidebar".into(),
        },
    );
    queue.set_style(
        node(1),
        NodeStyle {
            background: Some(SemanticColorRole::Surface),
            ..NodeStyle::default()
        },
    );
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "main".into() },
    );
    queue.set_style(
        node(2),
        NodeStyle {
            background: Some(SemanticColorRole::Background),
            ..NodeStyle::default()
        },
    );
    queue.create(
        node(3),
        document(1),
        NodeKind::Element { tag: "bar".into() },
    );
    queue.set_style(
        node(3),
        NodeStyle {
            background: Some(SemanticColorRole::Titlebar),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    let mut palette = SemanticPalette::dark();
    palette.surface.a = 0.5;
    let mut titlebar = palette.surface;
    titlebar.a = 1.0;
    let mut tokens = MutationQueue::new();
    tokens.set_theme_tokens(test_theme(
        ThemeMode::Dark,
        nana_ui_core::UI_METRICS,
        palette,
        titlebar,
    ));
    world.commit(tokens).unwrap();
    let work = world.take_system_work();
    assert!(work.style.is_empty());
    let extracted = world.extract_nodes(&[node(1), node(2), node(3)]);
    assert!(
        (extracted[0].style.background.unwrap()[3] - 0.5).abs() < f32::EPSILON,
        "sidebar Surface follows token alpha"
    );
    assert!(
        (extracted[1].style.background.unwrap()[3] - 1.0).abs() < f32::EPSILON,
        "main Background stays opaque for sidebar target"
    );
    assert!(
        (extracted[2].style.background.unwrap()[3] - 1.0).abs() < f32::EPSILON,
        "titlebar stays opaque when follows=false"
    );

    let mut reset = MutationQueue::new();
    reset.set_theme(ThemeMode::Dark);
    world.commit(reset).unwrap();
    let restored = world.extract_nodes(&[node(1)])[0].style.background.unwrap()[3];
    assert!(
        (restored - 1.0).abs() < f32::EPSILON,
        "set_theme restores opaque mode defaults"
    );
}

#[test]
fn grandchild_extract_inherits_ancestor_layout_color_without_restyling() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                color: Some([0.2, 0.4, 0.8, 1.0]),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        world.extract_nodes(&[node(3)])[0].style.color,
        Some([0.2, 0.4, 0.8, 1.0])
    );
    assert_eq!(
        world.extract_nodes(&[node(1), node(2), node(3)])[2]
            .style
            .color,
        Some([0.2, 0.4, 0.8, 1.0])
    );
}

#[test]
fn theme_change_refreshes_inherited_foreground_on_extract_only() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.set_style(
        node(1),
        NodeStyle {
            foreground: Some(SemanticColorRole::Accent),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        world.extract_nodes(&[node(3)])[0].style.color,
        Some(nana_ui_core::SemanticPalette::dark().accent.as_rgba_array())
    );

    let mut theme = MutationQueue::new();
    theme.set_theme(ThemeMode::Light);
    world.commit(theme).unwrap();
    let work = world.take_system_work();
    assert!(work.style.is_empty());
    assert_eq!(
        world.extract_nodes(&[node(3)])[0].style.color,
        Some(
            nana_ui_core::SemanticPalette::light()
                .accent
                .as_rgba_array()
        )
    );
}

#[test]
fn rejects_cycles_cross_document_parenting_and_invalid_sibling_anchor() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "main".into() },
    );
    queue.create(node(3), document(2), NodeKind::Document);
    queue.insert(node(1), node(2), None);
    world.commit(queue).unwrap();

    let mut cycle = MutationQueue::new();
    cycle.insert(node(2), node(1), None);
    assert_eq!(
        world.commit(cycle),
        Err(UiWorldError::Cycle {
            parent: node(2),
            child: node(1)
        })
    );

    let mut cross_document = MutationQueue::new();
    cross_document.insert(node(1), node(3), None);
    assert_eq!(
        world.commit(cross_document),
        Err(UiWorldError::CrossDocument {
            parent: node(1),
            child: node(3)
        })
    );

    let mut invalid_before = MutationQueue::new();
    invalid_before.insert(node(1), node(2), Some(node(3)));
    assert_eq!(
        world.commit(invalid_before),
        Err(UiWorldError::InvalidBefore {
            parent: node(1),
            before: node(3)
        })
    );
}

#[derive(Default)]
struct FunctionalShaper {
    calls: Vec<StableNodeId>,
}

impl TextShaper for FunctionalShaper {
    fn shape(
        &mut self,
        id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        _constraints: crate::TextShapeConstraints,
    ) -> TextMetrics {
        self.calls.push(id);
        TextMetrics {
            width: text.value.chars().count() as f32 * style.font_size,
            height: style.font_size,
            ascent: None,
        }
    }
}

struct WordWrapShaper;

impl TextShaper for WordWrapShaper {
    fn shape(
        &mut self,
        _id: StableNodeId,
        text: &TextContent,
        style: &ComputedStyle,
        constraints: crate::TextShapeConstraints,
    ) -> TextMetrics {
        let em = style.font_size.max(1.0);
        let intrinsic = text.value.chars().count() as f32 * em;
        let Some(max_width) = constraints.max_width.filter(|_| constraints.wrap) else {
            return TextMetrics {
                width: intrinsic,
                height: em,
                ascent: None,
            };
        };
        let mut lines = 1_u32;
        let mut line = 0.0;
        for word in text.value.split_whitespace() {
            let word_width = word.chars().count() as f32 * em;
            if line > 0.0 && line + em + word_width > max_width {
                lines += 1;
                line = word_width;
            } else {
                if line > 0.0 {
                    line += em;
                }
                line += word_width;
            }
        }
        TextMetrics {
            width: intrinsic.min(max_width),
            height: em * lines as f32,
            ascent: None,
        }
    }
}

#[test]
fn style_text_layout_input_focus_ime_hit_test_and_extraction_form_one_pipeline() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "input".into(),
        },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                opacity: Some(0.5),
                z_index: Some(2),
                ..LayoutStyle::default()
            }),
            foreground: Some(SemanticColorRole::Accent),
            background: None,
            border: None,
            interaction: crate::InteractionStyle::default(),
            ..NodeStyle::default()
        },
    );
    queue.set_style(
        node(3),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                opacity: Some(0.5),
                font_size: Some(20.0),
                ..LayoutStyle::default()
            }),
            foreground: None,
            background: None,
            border: None,
            interaction: crate::InteractionStyle::default(),
            ..NodeStyle::default()
        },
    );
    queue.set_text(
        node(3),
        TextContent {
            value: "输入".into(),
        },
    );
    queue.set_interaction(
        node(2),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    world.commit(queue).unwrap();

    let work = world.take_system_work();
    assert_eq!(work.text, vec![node(3)]);
    world.resolve_styles(&work.style).unwrap();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&work.text, &mut shaper).unwrap();
    assert_eq!(shaper.calls, vec![node(3)]);
    let layout = world.layout_inputs(&work.layout).unwrap();
    let text = layout.iter().find(|input| input.id == node(3)).unwrap();
    assert_eq!(text.parent, Some(node(2)));
    assert_eq!(text.text_metrics.unwrap().width, 40.0);

    let mut queue = MutationQueue::new();
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
    );
    queue.write_layout(
        node(2),
        LayoutBox {
            x: 10.0,
            y: 10.0,
            width: 80.0,
            height: 40.0,
        },
    );
    queue.write_layout(
        node(3),
        LayoutBox {
            x: 10.0,
            y: 10.0,
            width: 40.0,
            height: 20.0,
        },
    );
    queue.request_focus(document(1), Some(node(2)));
    queue.set_text_input(node(2), Some(TextInputState::new("")));
    queue.set_ime(
        node(2),
        Some(ImeComposition {
            text: "拼音".into(),
            selection: Some((0, 6)),
        }),
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.reconcile_focus(&work.focus_ime);
    world.rebuild_hit_test(document(1));
    assert_eq!(world.hit_test(document(1), 20.0, 20.0), Some(node(2)));

    let extracted = world.extract_document(document(1));
    let input = extracted.iter().find(|entry| entry.id == node(2)).unwrap();
    let text = extracted.iter().find(|entry| entry.id == node(3)).unwrap();
    assert!(input.focused);
    assert!(input.editable);
    assert_eq!(world.ime(node(2)).unwrap().text, "拼音");
    assert_eq!(text.style.foreground, SemanticColorRole::Accent);
    assert_eq!(text.style.opacity, 0.25);

    let mut queue = MutationQueue::new();
    queue.set_interaction(
        node(2),
        InteractionState {
            focusable: false,
            ..InteractionState::default()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.reconcile_focus(&work.focus_ime);
    assert_eq!(world.focused(document(1)), None);
    assert!(world.ime(node(2)).is_none());
}

#[test]
fn invalid_visual_input_is_rejected_atomically() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut queue = MutationQueue::new();
    queue.set_text(
        node(1),
        TextContent {
            value: "valid".into(),
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            width: -1.0,
            ..LayoutBox::default()
        },
    );
    assert_eq!(
        world.commit(queue),
        Err(UiWorldError::InvalidLayout(node(1)))
    );
    assert_eq!(world.generation(), 1);
    assert!(world.take_system_work().is_empty());
}

#[test]
fn custom_render_extension_requires_nonempty_backend_neutral_keys() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "gpu".into() },
    );
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut queue = MutationQueue::new();
    queue.set_custom_render(node(1), Some(CustomRenderNode::new("", "program", 0)));
    assert_eq!(
        world.commit(queue),
        Err(UiWorldError::InvalidCustomRender(node(1)))
    );
    assert!(world.custom_render(node(1)).is_none());
    assert!(world.take_system_work().is_empty());
}

#[test]
fn accessibility_projection_uses_runtime_hierarchy_focus_and_geometry() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.create(node(2), document(1), NodeKind::Text);
    queue.create(node(3), document(1), NodeKind::Comment);
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    queue.set_text(
        node(2),
        TextContent {
            value: "Build".into(),
        },
    );
    queue.set_interaction(
        node(1),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            role: AccessibilityRole::Button,
            label: Some(Arc::from("Build project")),
            value: None,
            disabled: false,
            checked: None,
            selected: None,
            multiline: false,
            editable: false,
            modal: false,
            ..AccessibilityState::default()
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 80.0,
            height: 28.0,
        },
    );
    queue.request_focus(document(1), Some(node(1)));
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    let projected = world.project_accessibility(document(1));
    assert_eq!(projected.len(), 2);
    assert_eq!(projected[0].role, AccessibilityRole::Button);
    assert_eq!(projected[0].label.as_deref(), Some("Build project"));
    assert!(projected[0].focused);
    assert_eq!(projected[0].children, vec![node(2)]);
    assert_eq!(projected[0].bounds.width, 80.0);
    assert_eq!(projected[1].role, AccessibilityRole::Text);
    assert_eq!(projected[1].label.as_deref(), Some("Build"));

    let mut queue = MutationQueue::new();
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            disabled: true,
            ..world.accessibility(node(1)).unwrap().clone()
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.accessibility, vec![node(1)]);
    assert!(work.style.is_empty());
    assert!(work.layout.is_empty());
}

#[test]
fn accessibility_delta_removes_and_restores_hidden_subtrees_atomically() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(world.project_accessibility(document(1)).len(), 3);

    let mut hide = MutationQueue::new();
    hide.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                hidden: true,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(hide).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let hidden = world.project_accessibility_delta(&work);
    assert_eq!(hidden.removed, vec![node(2), node(3)]);
    assert_eq!(
        hidden
            .updated
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![node(1)]
    );
    assert!(hidden.updated[0].children.is_empty());

    let mut show = MutationQueue::new();
    show.set_style(node(2), NodeStyle::default());
    world.commit(show).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let visible = world.project_accessibility_delta(&work);
    assert!(visible.removed.is_empty());
    assert_eq!(
        visible
            .updated
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![node(1), node(2), node(3)]
    );
    assert_eq!(visible.updated[0].children, vec![node(2)]);
    assert_eq!(visible.updated[1].children, vec![node(3)]);
}

/// #59: an ancestor's writing mode reaches its text's accessibility node, so
/// assistive technology walks a column top to bottom. The text itself does
/// not change or move, so only the inherited writing context can say so.
#[test]
fn accessibility_delta_carries_an_inherited_writing_mode() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let _ = world.project_accessibility_delta(&work);

    let mut vertical = MutationQueue::new();
    vertical.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                writing_mode: Some(nana_ui_core::WritingModeSpec::VerticalRl),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(vertical).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let delta = world.project_accessibility_delta(&work);
    let text = delta
        .updated
        .iter()
        .find(|entry| entry.id == node(3))
        .expect("the text's accessibility node is updated");
    assert!(text.writing.is_vertical());
}

#[test]
fn pointer_capture_and_event_routes_share_runtime_hierarchy_and_lifetime() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for id in 1..=3 {
        queue.create(
            node(id),
            document(1),
            NodeKind::Element { tag: "div".into() },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    world.commit(queue).unwrap();

    assert_eq!(
        world.event_route(node(3)).unwrap(),
        EventRoute {
            capture: vec![node(1), node(2)],
            target: node(3),
            bubble: vec![node(2), node(1)],
        }
    );

    let mut queue = MutationQueue::new();
    queue.capture_pointer(7, node(2));
    queue.capture_pointer(7, node(3));
    world.commit(queue).unwrap();
    assert_eq!(world.pointer_capture(document(1), 7), Some(node(3)));
    assert_eq!(
        world.take_pointer_capture_changes(),
        vec![
            PointerCaptureChange {
                pointer_id: 7,
                target: node(2),
                captured: true,
            },
            PointerCaptureChange {
                pointer_id: 7,
                target: node(2),
                captured: false,
            },
            PointerCaptureChange {
                pointer_id: 7,
                target: node(3),
                captured: true,
            },
        ]
    );

    let generation = world.generation();
    let mut invalid = MutationQueue::new();
    invalid.release_pointer(7, node(2));
    assert_eq!(
        world.commit(invalid),
        Err(UiWorldError::PointerCaptureMismatch {
            pointer_id: 7,
            target: node(2),
        })
    );
    assert_eq!(world.generation(), generation);
    assert_eq!(world.pointer_capture(document(1), 7), Some(node(3)));

    let mut remove = MutationQueue::new();
    remove.despawn_subtree(node(2));
    world.commit(remove).unwrap();
    assert_eq!(world.pointer_capture(document(1), 7), None);
    assert_eq!(
        world.take_pointer_capture_changes(),
        vec![PointerCaptureChange {
            pointer_id: 7,
            target: node(3),
            captured: false,
        }]
    );
    assert!(world.event_route(node(3)).is_none());
}

#[test]
fn event_listeners_are_runtime_query_authority() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.set_event_listener(node(1), "click", true);
    queue.set_event_listener(node(1), "input", true);
    world.commit(queue).unwrap();

    assert!(world.has_event(node(1), "click"));
    assert!(world.has_event(node(1), "input"));
    assert!(!world.has_event(node(1), "keydown"));
    assert!(
        world
            .event_targets(document(1))
            .contains(&(1, "click".into()))
    );

    let mut queue = MutationQueue::new();
    queue.set_event_listener(node(1), "click", false);
    world.commit(queue).unwrap();
    assert!(!world.has_event(node(1), "click"));
    assert!(world.has_event(node(1), "input"));

    let mut remove = MutationQueue::new();
    remove.despawn_subtree(node(1));
    world.commit(remove).unwrap();
    assert!(!world.has_event(node(1), "input"));
    assert!(world.event_targets(document(1)).is_empty());
}

#[test]
fn pointer_hover_and_press_are_runtime_owned_and_targeted() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Element { tag: "a".into() });
    queue.create(node(3), document(1), NodeKind::Element { tag: "b".into() });
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    for id in [node(2), node(3)] {
        queue.set_style(
            id,
            NodeStyle {
                interaction: crate::InteractionStyle {
                    hovered: crate::SemanticPaint {
                        background: Some(SemanticColorRole::Hover),
                        ..crate::SemanticPaint::default()
                    },
                    pressed: crate::SemanticPaint {
                        background: Some(SemanticColorRole::Active),
                        ..crate::SemanticPaint::default()
                    },
                    ..crate::InteractionStyle::default()
                },
                ..NodeStyle::default()
            },
        );
    }
    world.commit(queue).unwrap();
    world.take_system_work();

    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(2))),
        Ok(None)
    );
    assert_eq!(world.pointer_hover(document(1), 7), Some(node(2)));
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2)]);
    assert_eq!(work.style, vec![node(2)]);
    assert!(work.layout.is_empty());
    assert!(work.transform.is_empty());
    assert!(work.input_hit_test.is_empty());
    world.advance_animations(nana_ui_core::motion::HOVER_COLOR);
    world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        world.extract_nodes(&[node(2)])[0].style.background,
        Some(nana_ui_core::SemanticPalette::dark().hover.as_rgba_array())
    );
    let generation = world.generation();
    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(2))),
        Ok(Some(node(2)))
    );
    assert_eq!(world.generation(), generation);
    assert!(world.take_system_work().is_empty());

    assert_eq!(world.press_pointer(document(1), 7, node(2)), Ok(None));
    assert_eq!(world.pointer_press(document(1), 7), Some(node(2)));
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2)]);
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        world.extract_nodes(&[node(2)])[0].style.background,
        Some(nana_ui_core::SemanticPalette::dark().active.as_rgba_array())
    );
    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(3))),
        Ok(Some(node(2)))
    );
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2), node(3)]);
    assert_eq!(work.style, vec![node(2), node(3)]);
    assert_eq!(world.release_pointer_press(document(1), 7), Some(node(2)));

    let mut disable = MutationQueue::new();
    disable.set_interaction(
        node(3),
        InteractionState {
            pointer_events: false,
            focusable: false,
        },
    );
    world.commit(disable).unwrap();
    assert_eq!(world.pointer_hover(document(1), 7), None);
    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(3))),
        Err(UiWorldError::NotPointerInteractive(node(3)))
    );
}

#[test]
fn pointer_hover_without_interaction_style_dirties_state_not_style() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Element { tag: "a".into() });
    queue.create(node(3), document(1), NodeKind::Element { tag: "b".into() });
    queue.insert(node(1), node(2), None);
    queue.insert(node(1), node(3), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(2))),
        Ok(None)
    );
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2)]);
    assert!(work.style.is_empty());
    assert!(work.layout.is_empty());
    assert!(work.transform.is_empty());
    assert!(work.render_extraction.is_empty());
    assert_eq!(work.counters().style_processed, 0);

    assert_eq!(
        world.set_pointer_hover(document(1), 7, Some(node(3))),
        Ok(Some(node(2)))
    );
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2), node(3)]);
    assert!(work.style.is_empty());
    assert!(work.render_extraction.is_empty());
}

#[test]
fn state_only_invalidation_stays_observable_without_scheduling_a_frame() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Element { tag: "a".into() });
    queue.insert(node(1), node(2), None);
    world.commit(queue).unwrap();
    world.take_system_work();

    world
        .set_pointer_hover(document(1), 7, Some(node(2)))
        .unwrap();
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2)]);
    // Without interaction styling nothing downstream reads the hover, so the
    // drain must not claim a frame's worth of work.
    assert!(work.is_empty());

    let mut styled = MutationQueue::new();
    styled.set_style(
        node(2),
        NodeStyle {
            interaction: crate::InteractionStyle {
                hovered: crate::SemanticPaint {
                    background: Some(SemanticColorRole::Hover),
                    ..crate::SemanticPaint::default()
                },
                ..crate::InteractionStyle::default()
            },
            ..NodeStyle::default()
        },
    );
    world.commit(styled).unwrap();
    world.take_system_work();
    world.set_pointer_hover(document(1), 7, None).unwrap();
    world.take_system_work();

    world
        .set_pointer_hover(document(1), 7, Some(node(2)))
        .unwrap();
    // The same hover now has a consumer, and STYLE carries the frame.
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2)]);
    assert_eq!(work.style, vec![node(2)]);
    assert!(!work.is_empty());
}

#[test]
fn request_focus_dirties_state_without_requiring_style() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    queue.set_interaction(
        node(2),
        InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut focus = MutationQueue::new();
    focus.request_focus(document(1), Some(node(2)));
    world.commit(focus).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.state, vec![node(2)]);
    assert!(work.style.is_empty());
    assert!(work.transform.is_empty());
    assert!(work.layout.is_empty());
    assert_eq!(work.focus_ime, vec![node(2)]);
    assert_eq!(work.render_extraction, vec![node(2)]);
}

#[test]
fn scroll_offset_moves_descendant_hit_testing_without_rewriting_layout() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "scroll".into(),
        },
    );
    queue.create(
        node(3),
        document(1),
        NodeKind::Element { tag: "item".into() },
    );
    // Content reaching 200 down, beside the port's hit column, so an offset
    // of 60 is inside the scrolling area.
    queue.create(
        node(4),
        document(1),
        NodeKind::Element {
            tag: "spacer".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.insert(node(2), node(4), None);
    queue.write_layout(
        node(4),
        LayoutBox {
            x: 90.0,
            y: 0.0,
            width: 10.0,
            height: 200.0,
        },
    );
    queue.write_layout(
        node(2),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 50.0,
        },
    );
    queue.write_layout(
        node(3),
        LayoutBox {
            x: 0.0,
            y: 80.0,
            width: 100.0,
            height: 20.0,
        },
    );
    queue.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                overflow_y: OverflowSpec::Scroll,
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    world.rebuild_hit_test(document(1));

    let mut scroll = MutationQueue::new();
    scroll.set_scroll_offset(node(2), ScrollOffset { x: 0.0, y: 60.0 });
    world.commit(scroll).unwrap();
    assert_eq!(world.layout_box(node(3)).unwrap().y, 80.0);
    assert_eq!(world.scroll_offset(node(2)).unwrap().y, 60.0);
    let work = world.take_system_work();
    // Scroller-only hit/extract; Scene recomposes descendants from offset.
    assert_eq!(work.input_hit_test, vec![node(2)]);
    assert_eq!(work.render_extraction, vec![node(2)]);
    assert!(work.layout.is_empty());
    let scroll_updates = world.take_scroll_hit_updates();
    assert!(world.hit_test_work_is_scroll_only(&work.input_hit_test, &scroll_updates));
    for (scroller, delta) in scroll_updates {
        world.update_hit_test_scroll(document(1), scroller, delta);
    }
    assert_eq!(world.hit_test(document(1), 10.0, 25.0), Some(node(3)));
    assert_ne!(world.hit_test(document(1), 10.0, 85.0), Some(node(3)));
    // Scroller chrome is un-scrolled: a point on the chrome (not the
    // shifted item) still hits the scroller after the patch.
    assert_eq!(world.hit_test(document(1), 10.0, 45.0), Some(node(2)));
    assert_eq!(world.hit_test(document(1), 10.0, 5.0), Some(node(2)));
    let patched_scroller = hit_entry_transform(&world, document(1), node(2));
    let patched_item = hit_entry_transform(&world, document(1), node(3));
    assert_eq!(
        world.pointer_layout_position(node(3), 10.0, 25.0),
        Some((10.0, 85.0))
    );
    assert_eq!(
        world.layout_pointer_position(node(3), 10.0, 85.0),
        Some((10.0, 25.0))
    );
    // The in-place patch must agree with a full rebuild, including the
    // scroller's own transform.
    world.rebuild_hit_test(document(1));
    assert_eq!(
        patched_scroller,
        hit_entry_transform(&world, document(1), node(2))
    );
    assert_eq!(
        patched_item,
        hit_entry_transform(&world, document(1), node(3))
    );
    assert_eq!(world.hit_test(document(1), 10.0, 25.0), Some(node(3)));
    assert_ne!(world.hit_test(document(1), 10.0, 85.0), Some(node(3)));
    assert_eq!(world.hit_test(document(1), 10.0, 45.0), Some(node(2)));
    assert_eq!(world.hit_test(document(1), 10.0, 5.0), Some(node(2)));

    let mut metrics = MutationQueue::new();
    metrics.set_scroll_metrics(
        node(2),
        Some(ScrollMetrics {
            viewport_width: 100.0,
            viewport_height: 50.0,
            content_width: 100.0,
            content_height: 100.0,
            origin_x: 0.0,
            origin_y: 0.0,
        }),
    );
    world.commit(metrics).unwrap();
    assert_eq!(world.scroll_offset(node(2)).unwrap().y, 50.0);
    let work = world.take_system_work();
    // The metrics clamp re-anchors the offset; the scroller-only input
    // mark plus the recorded delta cover the hit index update.
    assert_eq!(work.input_hit_test, vec![node(2)]);
    assert!(work.layout.is_empty());

    let generation = world.generation();
    let mut invalid = MutationQueue::new();
    invalid.set_scroll_offset(
        node(2),
        ScrollOffset {
            x: 0.0,
            y: f32::NAN,
        },
    );
    assert_eq!(
        world.commit(invalid),
        Err(UiWorldError::InvalidScrollOffset(node(2)))
    );
    assert_eq!(world.generation(), generation);

    let mut invalid_metrics = MutationQueue::new();
    invalid_metrics.set_scroll_metrics(
        node(2),
        Some(ScrollMetrics {
            viewport_width: f32::NAN,
            viewport_height: 50.0,
            content_width: 100.0,
            content_height: 100.0,
            origin_x: 0.0,
            origin_y: 0.0,
        }),
    );
    assert_eq!(
        world.commit(invalid_metrics),
        Err(UiWorldError::InvalidScrollMetrics(node(2)))
    );
    let mut positive_origin = MutationQueue::new();
    positive_origin.set_scroll_metrics(
        node(2),
        Some(ScrollMetrics {
            viewport_width: 100.0,
            viewport_height: 50.0,
            content_width: 100.0,
            content_height: 100.0,
            origin_x: 0.0,
            origin_y: 1.0,
        }),
    );
    assert_eq!(
        world.commit(positive_origin),
        Err(UiWorldError::InvalidScrollMetrics(node(2)))
    );
    assert_eq!(world.generation(), generation);

    // An offset before the origin is not an error, just out of range.
    let mut before_origin = MutationQueue::new();
    before_origin.set_scroll_offset(node(2), ScrollOffset { x: 0.0, y: -1.0 });
    world.commit(before_origin).unwrap();
    assert_eq!(world.scroll_offset(node(2)).unwrap().y, 0.0);
}

#[test]
fn write_layout_does_not_extract_bit_identical_descendants() {
    fn box_at(x: f32, y: f32, width: f32, height: f32) -> LayoutBox {
        LayoutBox {
            x,
            y,
            width,
            height,
        }
    }
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "column".into(),
        },
    );
    queue.insert(node(1), node(2), None);
    // Three rows, each with a label. Middle-row size change shifts the
    // row below; the row above and the middle label stay bit-identical.
    for row in 0..3u64 {
        let row_id = node(3 + row * 2);
        let label_id = node(4 + row * 2);
        queue.create(row_id, document(1), NodeKind::Element { tag: "row".into() });
        queue.create(label_id, document(1), NodeKind::Text);
        queue.insert(node(2), row_id, None);
        queue.insert(row_id, label_id, None);
    }
    queue.write_layout(node(1), box_at(0.0, 0.0, 100.0, 60.0));
    queue.write_layout(node(2), box_at(0.0, 0.0, 100.0, 60.0));
    queue.write_layout(node(3), box_at(0.0, 0.0, 100.0, 20.0));
    queue.write_layout(node(4), box_at(0.0, 0.0, 40.0, 20.0));
    queue.write_layout(node(5), box_at(0.0, 20.0, 100.0, 20.0));
    queue.write_layout(node(6), box_at(0.0, 20.0, 40.0, 20.0));
    queue.write_layout(node(7), box_at(0.0, 40.0, 100.0, 20.0));
    queue.write_layout(node(8), box_at(0.0, 40.0, 40.0, 20.0));
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut changed = MutationQueue::new();
    // Parent grew; WriteLayout must not mark the whole subtree.
    changed.write_layout(node(2), box_at(0.0, 0.0, 100.0, 72.0));
    changed.write_layout(node(5), box_at(0.0, 20.0, 100.0, 32.0));
    changed.write_layout(node(7), box_at(0.0, 52.0, 100.0, 20.0));
    changed.write_layout(node(8), box_at(0.0, 52.0, 40.0, 20.0));
    world.commit(changed).unwrap();
    let work = world.take_system_work();
    assert!(work.render_extraction.contains(&node(2)));
    assert!(work.render_extraction.contains(&node(5)));
    assert!(work.render_extraction.contains(&node(7)));
    assert!(work.render_extraction.contains(&node(8)));
    assert!(!work.render_extraction.contains(&node(3)));
    assert!(!work.render_extraction.contains(&node(4)));
    assert!(
        !work.render_extraction.contains(&node(6)),
        "bit-identical middle-row label must not be extracted because its ancestor was written"
    );
    assert!(work.layout.is_empty());
    assert_eq!(work.input_hit_test, work.render_extraction);
    assert_eq!(work.accessibility, work.render_extraction);
}

#[test]
fn backend_neutral_text_geometry_treats_crlf_and_graphemes_atomically() {
    let text = TextContent {
        value: "A\r\n👩‍💻 e\u{301}".into(),
    };
    let style = ComputedStyle {
        font_size: 10.0,
        line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
        ..ComputedStyle::default()
    };
    let constraints = crate::TextShapeConstraints::default();
    let mut shaper = FunctionalShaper::default();
    let second_line = "A\r\n".len();

    assert_eq!(
        shaper.text_position(node(1), &text, second_line, &style, constraints),
        (0.0, 14.0, 14.0)
    );
    assert_eq!(
        shaper
            .text_highlights(node(1), &text, (0, "A".len()), &style, constraints)
            .len(),
        1
    );
    assert_eq!(
        shaper
            .text_highlights(
                node(1),
                &text,
                (0, second_line + "👩‍💻".len()),
                &style,
                constraints,
            )
            .len(),
        2
    );
    assert_eq!(
        shaper.text_position(
            node(1),
            &text,
            second_line + "👩".len(),
            &style,
            constraints
        ),
        (0.0, 0.0, 0.0)
    );
}

#[test]
#[cfg(feature = "calendar")]
#[cfg(feature = "charts")]
fn new_standard_visuals_derive_scene_geometry() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "calendar-heatmap".into(),
        },
    );
    queue.create(
        node(2),
        document(1),
        NodeKind::Element {
            tag: "time-series-chart".into(),
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 10.0,
            y: 20.0,
            width: 200.0,
            height: 120.0,
        },
    );
    queue.write_layout(
        node(2),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 108.0,
            height: 120.0,
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::CalendarHeatmap {
            cells: Arc::from([
                crate::CalendarHeatmapCellPaint {
                    x: 42.0,
                    y: 14.0,
                    level: 0,
                },
                crate::CalendarHeatmapCellPaint {
                    x: 42.0,
                    y: 28.0,
                    level: 4,
                },
            ]),
            month_labels: Arc::from([crate::CalendarHeatmapLabelPaint {
                text: Arc::from("6月"),
                x: 47.5,
                y: 0.0,
            }]),
            day_labels: Arc::from([crate::CalendarHeatmapLabelPaint {
                text: Arc::from("周一"),
                x: 0.0,
                y: 24.0,
            }]),
            cell_size: crate::CalendarHeatmap::<()>::CELL_SIZE,
            cell_radius: crate::CalendarHeatmap::<()>::CELL_RADIUS,
            max_level: 4,
            active: Some(1),
            active_title: Some(Arc::from("2026-06-03: 8")),
        }),
    );
    queue.set_standard_visual(
        node(2),
        Some(StandardVisual::TimeSeriesChart {
            values: Arc::from([0.0, 5.0, 10.0]),
        }),
    );
    world.commit(queue).unwrap();

    let crate::ComponentGeometry::CalendarHeatmap {
        cells,
        labels,
        hover,
    } = world
        .component_geometry(node(1))
        .expect("calendar geometry")
    else {
        panic!("expected calendar heatmap geometry");
    };
    assert_eq!(cells.len(), 2);
    assert_eq!(
        cells[0].0,
        LayoutBox {
            x: 52.0,
            y: 34.0,
            width: 11.0,
            height: 11.0,
        }
    );
    assert_eq!(
        cells[1].0,
        LayoutBox {
            x: 52.0,
            y: 48.0,
            width: 11.0,
            height: 11.0,
        }
    );
    assert_ne!(
        cells[0].1, cells[1].1,
        "active cell uses a stronger fill than the idle cell"
    );
    assert_eq!(labels.len(), 2);
    assert_eq!(labels[0].content.as_ref(), "6月");
    assert_eq!(labels[0].bounds.y, 20.0);
    assert!(
        (labels[0].bounds.x + labels[0].bounds.width * 0.5 - 57.5).abs() < 0.01,
        "month labels stay centered on the first week cell"
    );
    assert!(
        labels[0].bounds.width >= 10.0 + 10.0 * 0.62,
        "month CJK must not use the Latin 0.62em estimate"
    );
    assert_eq!(labels[1].bounds.x, 10.0);
    assert_eq!(labels[1].content.as_ref(), "周一");
    assert!(
        labels[1].bounds.width >= 22.0,
        "weekday CJK must keep a full-em box so 周一 is not clipped"
    );
    let hover = hover.expect("active cell paints hover chrome");
    assert_eq!(hover.title.content.as_ref(), "2026-06-03: 8");
    assert!(hover.tooltip.width < 176.0);

    let crate::ComponentGeometry::TimeSeriesChart {
        grid, area, line, ..
    } = world.component_geometry(node(2)).expect("chart geometry")
    else {
        panic!("expected time series geometry");
    };
    assert_eq!(grid.len(), 4);
    assert_eq!(grid[0].x, 8.0);
    assert_eq!(grid[0].height, 1.0);
    assert!(!area.is_empty());
    assert!(area.iter().all(|strip| strip.width <= 2.0 + f32::EPSILON));
    let expected_line = crate::TimeSeriesChart::new([0.0, 5.0, 10.0])
        .points(LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 108.0,
            height: 120.0,
        })
        .into_iter()
        .map(|(x, y)| [x, y])
        .collect::<Vec<_>>();
    assert_eq!(line, expected_line);
}

#[test]
#[cfg(feature = "graph-canvas")]
fn diagonal_stroke_segments_overlap_so_curves_do_not_break() {
    let bounds = LayoutBox {
        x: 0.0,
        y: 0.0,
        width: 200.0,
        height: 120.0,
    };
    let curve = [
        GraphPoint::new(10.0, 40.0),
        GraphPoint::new(80.0, 40.0),
        GraphPoint::new(120.0, 80.0),
        GraphPoint::new(190.0, 80.0),
    ];
    let points = sample_curve(bounds, curve);
    assert_curve_connected_and_flat(bounds, curve, &points);
}

#[test]
#[cfg(feature = "graph-canvas")]
fn long_zoomed_curves_keep_segment_spacing() {
    let bounds = LayoutBox {
        x: 0.0,
        y: 0.0,
        width: 4000.0,
        height: 800.0,
    };
    let curve = [
        GraphPoint::new(0.0, 40.0),
        GraphPoint::new(800.0, 40.0),
        GraphPoint::new(1600.0, 760.0),
        GraphPoint::new(2400.0, 760.0),
    ];
    let points = sample_curve(bounds, curve);
    assert_curve_connected_and_flat(bounds, curve, &points);
    let max_chord = max_polyline_chord(&points);
    assert!(
        max_chord > 8.0,
        "flat pieces may be longer than 8px, max chord {max_chord}"
    );
    assert!(
        points.len() < 96,
        "flatness-only flattening must not explode instances, got {}",
        points.len()
    );

    let straight = [
        GraphPoint::new(0.0, 40.0),
        GraphPoint::new(800.0, 40.0),
        GraphPoint::new(1600.0, 40.5),
        GraphPoint::new(2400.0, 40.0),
    ];
    let straight_points = sample_curve(bounds, straight);
    assert_curve_connected_and_flat(bounds, straight, &straight_points);
    assert!(
        straight_points.len() < 8,
        "a long nearly-straight cubic should stay a handful of capsules, got {}",
        straight_points.len()
    );
    assert!(max_polyline_chord(&straight_points) > 8.0);
}

#[cfg(feature = "graph-canvas")]
fn assert_curve_connected_and_flat(bounds: LayoutBox, curve: [GraphPoint; 4], points: &[[f32; 2]]) {
    assert!(points.len() > 1);
    let origin = [bounds.x, bounds.y];
    let start = [origin[0] + curve[0].x, origin[1] + curve[0].y];
    let end = [origin[0] + curve[3].x, origin[1] + curve[3].y];
    assert_eq!(points[0], start);
    let last = *points.last().expect("polyline");
    assert!((last[0] - end[0]).abs() < 1e-3 && (last[1] - end[1]).abs() < 1e-3);
    assert!(points.windows(2).all(|pair| {
        let chord = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
        chord.is_finite() && chord > 0.0
    }));
    for index in 0..=64 {
        let sample = cubic_point(curve, index as f32 / 64.0);
        let point = [origin[0] + sample.x, origin[1] + sample.y];
        let distance = points
            .windows(2)
            .map(|pair| distance_to_segment(point, pair[0], pair[1]))
            .fold(f32::INFINITY, f32::min);
        assert!(
            distance <= CURVE_FLATNESS + 0.05,
            "cubic sample t={} is {distance}px off the polyline",
            index as f32 / 64.0
        );
    }
}

#[cfg(feature = "graph-canvas")]
fn max_polyline_chord(points: &[[f32; 2]]) -> f32 {
    points
        .windows(2)
        .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
        .fold(0.0_f32, f32::max)
}

#[cfg(feature = "graph-canvas")]
fn distance_to_segment(point: [f32; 2], start: [f32; 2], end: [f32; 2]) -> f32 {
    let abx = end[0] - start[0];
    let aby = end[1] - start[1];
    let length_sq = abx * abx + aby * aby;
    if length_sq <= f32::EPSILON {
        return (point[0] - start[0]).hypot(point[1] - start[1]);
    }
    let t = ((point[0] - start[0]) * abx + (point[1] - start[1]) * aby) / length_sq;
    let t = t.clamp(0.0, 1.0);
    (point[0] - (start[0] + abx * t)).hypot(point[1] - (start[1] + aby * t))
}

#[test]
#[cfg(feature = "graph-canvas")]
fn hovered_graph_edge_uses_muted_gray_instead_of_accent() {
    let paint = |hovered, selected| crate::GraphEdgePaint {
        curve: [GraphPoint::ZERO; 4],
        selected,
        hovered,
        connecting: false,
        label: None,
    };
    let dark = SemanticPalette::dark();
    assert_eq!(
        graph_edge_stroke_color(&dark, &paint(true, false)),
        dark.muted.as_rgba_array()
    );
    assert_ne!(
        graph_edge_stroke_color(&dark, &paint(true, false)),
        dark.accent.as_rgba_array()
    );
    assert_eq!(
        graph_edge_stroke_color(&dark, &paint(false, true)),
        dark.text.as_rgba_array()
    );
    let light = SemanticPalette::light();
    assert_eq!(
        graph_edge_stroke_color(&light, &paint(true, false)),
        light.muted.as_rgba_array()
    );
    assert_ne!(
        graph_edge_stroke_color(&light, &paint(true, false)),
        light.accent.as_rgba_array()
    );
}

#[test]
fn idle_extract_shares_kind_style_and_children() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.create(node(3), document(1), NodeKind::Text);
    queue.insert(node(1), node(2), None);
    queue.insert(node(2), node(3), None);
    queue.set_text(node(3), TextContent { value: "hi".into() });
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    let first = world.extract_nodes(&[node(1), node(2), node(3)]);
    let second = world.extract_nodes(&[node(1), node(2), node(3)]);
    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
    assert!(Arc::ptr_eq(&first[0].kind, &first[1].kind));
    for (left, right) in first.iter().zip(&second) {
        assert!(Arc::ptr_eq(&left.kind, &right.kind));
        assert!(Arc::ptr_eq(&left.style, &right.style));
        assert!(Arc::ptr_eq(&left.children, &right.children));
    }
    assert_eq!(first[0].children.as_slice(), &[node(2)]);
    assert_eq!(first[1].children.as_slice(), &[node(3)]);
    assert!(first[2].children.is_empty());
    assert_eq!(
        first[2].text.as_ref().map(|text| text.value.as_str()),
        Some("hi")
    );
    assert!(first[0].text_spans.is_empty());
    assert!(first[1].text_spans.is_empty());
}

#[test]
fn dirty_extract_updates_changed_slots_and_keeps_idle_arcs() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.create(
        node(2),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    queue.insert(node(1), node(2), None);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let before = world.extract_nodes(&[node(1), node(2)]);

    let mut paint = MutationQueue::new();
    paint.set_style(
        node(2),
        NodeStyle {
            layout: Arc::new(LayoutStyle {
                background: Some([1.0, 0.0, 0.0, 1.0]),
                ..LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    world.commit(paint).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let painted = world.extract_nodes(&[node(1), node(2)]);
    assert_eq!(painted.len(), 2);
    assert!(Arc::ptr_eq(&before[0].kind, &painted[0].kind));
    assert!(Arc::ptr_eq(&before[0].children, &painted[0].children));
    assert!(Arc::ptr_eq(&before[0].style, &painted[0].style));
    assert!(Arc::ptr_eq(&before[1].kind, &painted[1].kind));
    assert!(Arc::ptr_eq(&before[1].children, &painted[1].children));
    assert!(!Arc::ptr_eq(&before[1].style, &painted[1].style));
    assert_eq!(painted[1].style.background, Some([1.0, 0.0, 0.0, 1.0]));

    let mut insert = MutationQueue::new();
    insert.create(
        node(3),
        document(1),
        NodeKind::Element { tag: "div".into() },
    );
    insert.insert(node(1), node(3), None);
    world.commit(insert).unwrap();
    world.take_system_work();
    let reparented = world.extract_nodes(&[node(1)]);
    assert_eq!(reparented.len(), 1);
    assert!(!Arc::ptr_eq(&painted[0].children, &reparented[0].children));
    assert_eq!(reparented[0].children.as_slice(), &[node(2), node(3)]);
    assert!(Arc::ptr_eq(&painted[0].kind, &reparented[0].kind));
}

/// 多行编辑器 + 聚焦 + 候选会话的测试节点（几何完整可用）。
fn overlay_editor(
    world: &mut UiWorld,
    value: &str,
    caret: usize,
    items: Arc<[crate::TextCompletion]>,
) -> StableNodeId {
    let mut queue = MutationQueue::new();
    queue.create(
        node(1),
        document(1),
        NodeKind::Element {
            tag: "textarea".into(),
        },
    );
    queue.set_standard_visual(
        node(1),
        Some(StandardVisual::TextInput {
            placeholder: Arc::from(""),
            size: nana_ui_core::ControlSize::Medium,
            secure: false,
            invalid: false,
            steppers: false,
            diagnostics: Arc::from([]),
            matches: Arc::from([]),
            color_swatches: Arc::from([]),
            atoms: Arc::from([]),
            line_numbers: false,
            indent_guides: None,
            folds: Arc::from([]),
            git_marks: Arc::from([]),
            editor_options: Default::default(),
        }),
    );
    queue.set_style(
        node(1),
        NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                height: Some(nana_ui_core::LengthSpec::Px(140.0)),
                width: Some(nana_ui_core::LengthSpec::Px(200.0)),
                font_size: Some(10.0),
                line_height: Some(nana_ui_core::LineHeightSpec::Absolute(14.0)),
                ..nana_ui_core::LayoutStyle::default()
            }),
            ..NodeStyle::default()
        },
    );
    queue.set_text_input(
        node(1),
        Some(TextInputState {
            value: value.into(),
            selection: crate::TextSelection::caret(caret),
            additional_selections: Vec::new(),
        }),
    );
    queue.set_accessibility(
        node(1),
        AccessibilityState {
            multiline: true,
            editable: true,
            ..AccessibilityState::default()
        },
    );
    queue.set_interaction(
        node(1),
        crate::InteractionState {
            pointer_events: true,
            focusable: true,
        },
    );
    queue.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 140.0,
        },
    );
    queue.set_text_input_completions(node(1), items);
    queue.request_focus(document(1), Some(node(1)));
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1)]).unwrap();
    world.take_system_work();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    node(1)
}

#[test]
fn completion_popup_anchors_below_flips_above_and_hit_tests_rows() {
    let mut world = UiWorld::default();
    let items: Arc<[crate::TextCompletion]> = (0..4)
        .map(|index| crate::TextCompletion::new(format!("item{index}"), "fn"))
        .collect::<Vec<_>>()
        .into();
    // 光标在第 0 行（行底 y=14）：弹层出现在光标行下方。
    let id = overlay_editor(&mut world, "a\nb\nc\nd\ne\nf", 1, items.clone());
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput {
        completion_popup, ..
    } = geometry
    else {
        panic!("text input geometry");
    };
    let popup = completion_popup.expect("popup below caret line");
    assert_eq!(popup.first_row, 0);
    assert_eq!(popup.selected, 0);
    assert_eq!(popup.rows.len(), 4);
    // 行高 14、内边距 4：面板顶 = 行底 14 + 间隙 4 = 18；宽度按
    // FunctionalShaper 度量（label 5 字×10=50，kind "fn"=20）+ 行距
    // 12 + 内边距 16。
    assert_eq!(popup.panel.y, 18.0);
    assert_eq!(popup.panel.height, 4.0 * 14.0 + 8.0);
    assert_eq!(popup.panel.width, 50.0 + 12.0 + 20.0 + 16.0);
    // 行命中返回绝对候选下标。
    let row = &popup.rows[1];
    assert_eq!(
        world.text_completion_hit(id, row.bounds.x + 1.0, row.bounds.y + 1.0),
        Some(1)
    );
    assert_eq!(world.text_completion_hit(id, -100.0, -100.0), None);

    // 光标移到最后一行（行顶 y=70）：下方放不下（70+14+4+64 > 138）
    // 且上方放得下（70-4-64 = 2 ≥ 2）→ 翻转到光标行上方。
    let mut queue = MutationQueue::new();
    queue.set_text_input(
        id,
        Some(TextInputState {
            value: "a\nb\nc\nd\ne\nf".into(),
            selection: crate::TextSelection::caret("a\nb\nc\nd\ne\nf".len()),
            additional_selections: Vec::new(),
        }),
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[id], &mut shaper).unwrap();
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput {
        completion_popup, ..
    } = geometry
    else {
        panic!("text input geometry");
    };
    let popup = completion_popup.expect("popup flips above caret line");
    assert_eq!(popup.panel.y, 2.0);

    // Esc 关闭：弹层几何消失（会话保留），命中也为 None。
    let mut queue = MutationQueue::new();
    queue.set_text_input_completion_dismissed(id);
    world.commit(queue).unwrap();
    world.take_system_work();
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput {
        completion_popup, ..
    } = geometry
    else {
        panic!("text input geometry");
    };
    assert!(completion_popup.is_none());
    assert!(!world.text_completion_panel_hit(id, 10.0, 10.0));

    // 宿主显式重开：清除关闭标记、选中归零，弹层几何恢复。
    let mut queue = MutationQueue::new();
    queue.set_text_input_completion_reopened(id);
    world.commit(queue).unwrap();
    world.take_system_work();
    let state = world
        .text_completion_view(id)
        .expect("completion session kept");
    assert!(!state.dismissed);
    assert_eq!(state.selected, 0);
    assert_eq!(state.scroll, 0);
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput {
        completion_popup, ..
    } = geometry
    else {
        panic!("text input geometry");
    };
    assert!(completion_popup.is_some(), "重开后弹层几何恢复");
}

#[test]
fn completion_feed_marks_render_once_and_same_list_is_a_short_circuit() {
    let mut world = UiWorld::default();
    let items: Arc<[crate::TextCompletion]> = vec![crate::TextCompletion::new("fn", "")].into();
    let _id = overlay_editor(&mut world, "fn", 2, items.clone());

    // 首次 shape 的失效先排干，只观察重喂本身的标记。
    world.take_system_work();

    // 重喂相同列表（组件重投影的常态）：无渲染失效（每帧短路）。
    let mut queue = MutationQueue::new();
    queue.set_text_input_completions(node(1), items.clone());
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.render_extraction, Vec::<StableNodeId>::new());

    // 新列表：标记渲染。
    let next: Arc<[crate::TextCompletion]> = vec![crate::TextCompletion::new("fnx", "")].into();
    let mut queue = MutationQueue::new();
    queue.set_text_input_completions(node(1), next);
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    assert_eq!(work.render_extraction, vec![node(1)]);
}

#[test]
fn hover_popup_anchors_scrolls_and_hit_tests_panel() {
    let mut world = UiWorld::default();
    // 锚点偏移定位到第二行 "let x"（y=14）：浮窗在该行下方。
    let mut queue = MutationQueue::new();
    let items: Arc<[crate::TextCompletion]> = Arc::from([]);
    let id = overlay_editor(&mut world, "fn main() {}\nlet x", 0, items);
    queue.set_text_input_hover(
        id,
        Some(crate::TextHover::new(
            "fn main() {}\n".len(),
            "let",
            "declare a binding\nline two",
        )),
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[id], &mut shaper).unwrap();

    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput { hover_popup, .. } = geometry else {
        panic!("text input geometry");
    };
    let popup = hover_popup.expect("hover popup");
    // 锚点行底 28 + 间隙 4；标题 + 两行正文 + 上下内边距 12。
    assert_eq!(popup.panel.y, 32.0);
    assert_eq!(popup.panel.width, 200.0);
    assert_eq!(popup.title.content.as_ref(), "let");
    assert_eq!(popup.body_rows.len(), 2);
    assert_eq!(popup.body_rows[0].content.as_ref(), "declare a binding");
    // 面板命中：滚轮路由的判定输入。
    assert!(world.text_hover_panel_hit(id, popup.panel.x + 2.0, popup.panel.y + 2.0));
    assert!(!world.text_hover_panel_hit(id, -1.0, -1.0));

    // 滚动一行：正文切片跳过第一行。
    let mut queue = MutationQueue::new();
    queue.set_text_input_hover_scroll(id, 1);
    world.commit(queue).unwrap();
    world.take_system_work();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[id], &mut shaper).unwrap();
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput { hover_popup, .. } = geometry else {
        panic!("text input geometry");
    };
    let popup = hover_popup.expect("hover popup");
    assert_eq!(popup.body_rows.len(), 1);
    assert_eq!(popup.body_rows[0].content.as_ref(), "line two");

    // 宿主撤掉 hover（None）：浮窗几何消失，滚动状态一并清除。
    let mut queue = MutationQueue::new();
    queue.set_text_input_hover(id, None);
    world.commit(queue).unwrap();
    world.take_system_work();
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput { hover_popup, .. } = geometry else {
        panic!("text input geometry");
    };
    assert!(hover_popup.is_none());
    assert_eq!(world.text_hover_scroll(id), 0);
}

#[test]
fn signature_popup_highlights_active_parameter_and_revokes() {
    let mut world = UiWorld::default();
    let mut queue = MutationQueue::new();
    let id = overlay_editor(&mut world, "mix(1.0, 2.0, 0.5)", 4, Arc::from([]));
    queue.set_text_input_signature(
        id,
        Some(crate::TextSignatureHelp::new(
            "mix",
            "linear blend",
            vec![
                ("a".into(), "start".into()),
                ("b".into(), "end".into()),
                ("t".into(), String::new()),
            ],
            0,
        )),
    );
    world.commit(queue).unwrap();
    world.take_system_work();
    let mut shaper = FunctionalShaper::default();
    world.shape_text(&[id], &mut shaper).unwrap();
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput {
        signature_popup, ..
    } = geometry
    else {
        panic!("text input geometry");
    };
    let popup = signature_popup.expect("signature popup");
    assert_eq!(popup.prefix.content.as_ref(), "mix(");
    assert_eq!(
        popup.active.as_ref().map(|region| region.content.as_ref()),
        Some("a")
    );
    assert_eq!(popup.suffix.content.as_ref(), ", b, t)");
    assert_eq!(
        popup.doc.as_ref().map(|region| region.content.as_ref()),
        Some("start")
    );
    assert!(popup.active.is_some());

    let mut queue = MutationQueue::new();
    queue.set_text_input_signature(id, None);
    world.commit(queue).unwrap();
    world.take_system_work();
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput {
        signature_popup, ..
    } = geometry
    else {
        panic!("text input geometry");
    };
    assert!(signature_popup.is_none());
}

// ---- Wave 4b-1：span 值→显示重映射 / 补全弹层文档行 ----

/// 固定区间 presenter：按预设值空间区间出 span（颜色角色互异便于断言）。
struct FixedSpanPresenter {
    spans: Vec<(usize, usize, SemanticColorRole)>,
}

impl crate::TextPresenter for FixedSpanPresenter {
    fn name(&self) -> &'static str {
        crate::HIGHLIGHT_PRESENTER
    }
    fn present(&self, _text: &str, _request: &crate::HighlightRequest) -> Vec<crate::TextSpan> {
        self.spans
            .iter()
            .map(|&(start, end, color)| crate::TextSpan { start, end, color })
            .collect()
    }
}

/// 折叠态语法高亮回归锚 + 重映射端点语义：折叠后剩余文本仍带语法色
/// （不再整批丢弃）；起点落在隐藏区间内部的 span 钳到摘要之后；跨折叠
/// 区间的 span 在边界切分；摘要文本保持中性色（无 span 覆盖摘要区间）。
#[test]
fn folded_editor_remaps_syntax_spans_into_display_space() {
    let mut world = UiWorld::default();
    world
        .register_presenter(Box::new(FixedSpanPresenter {
            // 探针值空间互不重叠（sanitize 会按序裁掉重叠尾段）：
            // (0,4) 完全在折叠前；(5,12) 尾段落入隐藏区间；(20,34) 起点
            // 在隐藏区间内部、终点在折叠之后。
            spans: vec![
                (0, 4, SemanticColorRole::Accent),
                (5, 12, SemanticColorRole::Success),
                (20, 34, SemanticColorRole::Warning),
            ],
        }))
        .unwrap();
    fold_editor_world(&mut world, Arc::from([FOLD_BLOCK]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_highlight_request(node(1), Some(crate::HighlightRequest::highlight("rs")));
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    world.resolve_presentations(&[node(1)]).unwrap();

    let extracted = &world.extract_nodes(&[node(1)])[0];
    // 显示视图：FOLD_VALUE 折叠 [8,28) → "fn a() { …3\nfn b() {}"，
    // 摘要 " …3" 占显示区间 [8,13)。
    let display = build_text_display_view(FOLD_VALUE, &[FOLD_BLOCK], &[]).unwrap();
    assert_eq!(display.spans[0].value_start, 8);
    assert_eq!(display.spans[0].display_start, 8);
    assert_eq!(display.spans[0].display_len, 5);
    let spans: Vec<(usize, usize)> = extracted
        .text_spans
        .iter()
        .map(|span| (span.start, span.end))
        .collect();
    // 回归锚：折叠态 spans 保留（现状守卫整丢）。期望端点：
    // (0,4) 原样；(5,12) → (5,8)（尾段止于摘要之前）；(20,34) →
    // (13,19)（起点钳到摘要之后）。
    assert_eq!(
        spans,
        vec![(0, 4), (5, 8), (13, 19)],
        "折叠后剩余文本仍带语法色，端点按显示视图重投"
    );
    // 摘要中性：没有任何 span 覆盖摘要区间 [8,13)。
    assert!(
        extracted
            .text_spans
            .iter()
            .all(|span| span.end <= 8 || span.start >= 13),
        "摘要文本不着色"
    );

    // 跨折叠区间的切分：整段跨越 [8,28) 的 span 在边界切分为可见前后
    // 两段（摘要不着色）。
    let mut world = UiWorld::default();
    world
        .register_presenter(Box::new(FixedSpanPresenter {
            spans: vec![(5, 33, SemanticColorRole::Accent)],
        }))
        .unwrap();
    fold_editor_world(&mut world, Arc::from([FOLD_BLOCK]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_highlight_request(node(1), Some(crate::HighlightRequest::highlight("rs")));
    queue.set_text_input_fold_collapsed(node(1), Arc::from([FOLD_BLOCK]));
    world.commit(queue).unwrap();
    world.resolve_presentations(&[node(1)]).unwrap();
    let spans: Vec<(usize, usize)> = world.extract_nodes(&[node(1)])[0]
        .text_spans
        .iter()
        .map(|span| (span.start, span.end))
        .collect();
    assert_eq!(spans, vec![(5, 8), (13, 18)], "跨折叠区间在边界切分");

    // 对照锚：未折叠时同一 presenter 的 span 保持值空间原样。
    let mut world = UiWorld::default();
    world
        .register_presenter(Box::new(FixedSpanPresenter {
            spans: vec![(0, 4, SemanticColorRole::Accent)],
        }))
        .unwrap();
    fold_editor_world(&mut world, Arc::from([FOLD_BLOCK]), false, None);
    let mut queue = MutationQueue::new();
    queue.set_highlight_request(node(1), Some(crate::HighlightRequest::highlight("rs")));
    world.commit(queue).unwrap();
    world.resolve_presentations(&[node(1)]).unwrap();
    let spans: Vec<(usize, usize)> = world.extract_nodes(&[node(1)])[0]
        .text_spans
        .iter()
        .map(|span| (span.start, span.end))
        .collect();
    assert_eq!(spans, vec![(0, 4)]);
}

/// `remap_span_to_display` 端点单测：隐藏区内起点钳后 / 跨区间切分 /
/// 尾段落入隐藏区间丢弃 / 完全隐藏丢弃 / 折叠前后的区间原样映射。
#[test]
fn remap_span_to_display_endpoints() {
    let view = build_text_display_view(FOLD_VALUE, &[FOLD_BLOCK], &[]).unwrap();
    // 隐藏区间 [8,28)，摘要占显示 [8,13)。
    let pieces = |start: usize, end: usize| remap_span_to_display((start, end), &view);
    // 折叠前后：恒等映射。
    assert_eq!(pieces(0, 4), vec![(0, 4)]);
    assert_eq!(pieces(28, 33), vec![(13, 18)]);
    // 起点在隐藏区间内部：钳到摘要之后。
    assert_eq!(pieces(11, 34), vec![(13, 19)]);
    // 跨折叠区间：边界切分，摘要不着色。
    assert_eq!(pieces(5, 30), vec![(5, 8), (13, 15)]);
    // 尾段落入隐藏区间：止于摘要之前。
    assert_eq!(pieces(2, 10), vec![(2, 8)]);
    // 完全隐藏：丢弃。
    assert!(pieces(10, 12).is_empty());
}

/// 补全弹层文档行：带 `doc` 的候选整行占两行高，文档行呈现为 label 行
/// 下方的次要色单行；无 doc 的候选保持单行高。
#[test]
fn completion_popup_doc_row_doubles_row_height_and_presents_doc_line() {
    let mut world = UiWorld::default();
    let items: Arc<[crate::TextCompletion]> = vec![
        crate::TextCompletion::new("normalize", "fn").detail("fn normalize(e: vec2f) -> vec2f"),
        crate::TextCompletion::new("normalize", "fn")
            .detail("fn normalize(e: vec2f) -> vec2f")
            .doc("返回与输入同方向的单位向量。"),
        crate::TextCompletion::new("max", "fn"),
    ]
    .into();
    let id = overlay_editor(&mut world, "nor", 3, items);
    let geometry = world.component_geometry(id).unwrap();
    let crate::ComponentGeometry::TextInput {
        completion_popup, ..
    } = geometry
    else {
        panic!("text input geometry");
    };
    let popup = completion_popup.expect("completion popup");
    assert_eq!(popup.rows.len(), 3);
    // 无 doc 的行单行高（14），带 doc 的行两行高（28）。
    assert_eq!(popup.rows[0].bounds.height, 14.0);
    assert_eq!(popup.rows[2].bounds.height, 14.0);
    let documented = &popup.rows[1];
    assert_eq!(documented.bounds.height, 28.0);
    let doc = documented.doc.as_ref().expect("doc row");
    assert_eq!(doc.content.as_ref(), "返回与输入同方向的单位向量。");
    // 文档行在 label 行下方，占满内容宽。
    assert_eq!(doc.bounds.y, documented.label.bounds.y + 14.0);
    assert_eq!(doc.bounds.height, 14.0);
    assert!(doc.bounds.width + 0.5 >= documented.label.bounds.width);
    // 无 doc 的候选没有文档行区域。
    assert!(popup.rows[0].doc.is_none());
    assert!(popup.rows[2].doc.is_none());
    // 面板高度 = 4 行内容（3 候选 + 1 文档行）+ 上下内边距。
    assert_eq!(popup.panel.height, 4.0 * 14.0 + 8.0);
    // 行命中仍按整行（含文档行）返回绝对下标。
    assert_eq!(
        world.text_completion_hit(
            id,
            documented.bounds.x + 1.0,
            documented.bounds.y + documented.bounds.height - 1.0
        ),
        Some(1)
    );
}

/// merge_inlay_glyph_spans 的重叠路径契约：基座 span 与 inlay 区间重叠
/// 时，inlay 色整段胜出（基座在 inlay 边界切分），重叠段不得被吞掉。
#[test]
fn inlay_glyph_merge_lets_inlay_color_win_on_overlap() {
    let red = [1.0, 0.0, 0.0, 1.0];
    let green = [0.0, 1.0, 0.0, 1.0];
    let muted = [0.5, 0.5, 0.5, 1.0];
    let merged = merge_inlay_glyph_spans(
        vec![
            ExtractedTextSpan {
                start: 0,
                end: 4,
                color: red,
            },
            ExtractedTextSpan {
                start: 6,
                end: 10,
                color: green,
            },
        ],
        &[(1usize, 3usize), (6, 8)],
        muted,
    );
    // 前半段重叠（基座 [0,4) × inlay [1,3)）：基座切分前后两段，inlay
    // 区间整段 muted；后半段（基座 [6,10) × inlay [6,8)）：起点重合时
    // 无前残余，inlay 整段 + 基座尾段。
    assert_eq!(
        merged,
        vec![
            ExtractedTextSpan {
                start: 0,
                end: 1,
                color: red,
            },
            ExtractedTextSpan {
                start: 1,
                end: 3,
                color: muted,
            },
            ExtractedTextSpan {
                start: 3,
                end: 4,
                color: red,
            },
            ExtractedTextSpan {
                start: 6,
                end: 8,
                color: muted,
            },
            ExtractedTextSpan {
                start: 8,
                end: 10,
                color: green,
            },
        ]
    );
}
#[test]
fn viewport_dependency_index_is_document_local_and_releases_removed_entries() {
    let first = DocumentId::new(1).unwrap();
    let second = DocumentId::new(2).unwrap();
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    for (id, document) in [(node(1), first), (node(2), second)] {
        queue.create(id, document, NodeKind::Element { tag: "div".into() });
        queue.set_style(
            id,
            NodeStyle {
                layout: std::sync::Arc::new(nana_ui_core::LayoutStyle {
                    position: nana_ui_core::PositionSpec::Fixed,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
    }
    world.commit(queue).unwrap();
    assert_eq!(
        world.viewport_basis_ids_for(first).collect::<Vec<_>>(),
        vec![node(1)]
    );
    assert_eq!(
        world.viewport_basis_ids_for(second).collect::<Vec<_>>(),
        vec![node(2)]
    );
    let mut queue = MutationQueue::new();
    queue.park_subtree(node(1));
    queue.despawn_subtree(node(2));
    world.commit(queue).unwrap();
    assert_eq!(world.viewport_basis_ids_for(first).count(), 0);
    assert_eq!(world.viewport_basis_ids_for(second).count(), 0);
    assert!(!world.uses_viewport_basis());
    assert!(world.viewport_basis.is_empty());
}

#[test]
fn indexed_sticky_headers_preserve_crlf_utf8_and_nested_scrolling() {
    let value = STICKY_VALUE
        .replace("x();", "工作();")
        .replace('\n', "\r\n");
    let folds = Arc::from([
        crate::TextCodeFold::new(0, value.find("\r\n// tail").unwrap()),
        crate::TextCodeFold::new(
            value.find("    fn inner").unwrap(),
            value.find("    z();").unwrap(),
        ),
    ]);
    let mut world = UiWorld::default();
    sticky_editor_world(&mut world, &value, folds, 35.0);
    world
        .shape_text(&[node(1)], &mut FunctionalShaper::default())
        .unwrap();
    let crate::ComponentGeometry::TextInput { sticky_line, .. } =
        world.component_geometry(node(1)).unwrap()
    else {
        panic!("text input")
    };
    let sticky = sticky_line.expect("nested header remains pinned");
    assert_eq!(sticky.text.content.trim_end(), "    fn inner() {");
    assert_eq!(sticky.panel.y, 0.0);
}

#[test]
fn modal_accessibility_bounds_keep_surface_and_descendant_clipping() {
    let mut world = UiWorld::default();
    let mut queue = MutationQueue::new();
    for id in [node(1), node(2)] {
        queue.create(id, document(1), NodeKind::Element { tag: "div".into() });
        queue.write_layout(
            id,
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 800.0,
                height: 600.0,
            },
        );
    }
    queue.insert(node(1), node(2), None);
    queue.set_standard_visual(node(1), Some(confirm_modal_visual()));
    world.commit(queue).unwrap();
    world.resolve_styles(&[node(1), node(2)]).unwrap();
    let crate::ComponentGeometry::ModalFrame { surface, .. } =
        world.component_geometry(node(1)).unwrap()
    else {
        panic!("modal")
    };
    let accessible = world.project_accessibility(document(1));
    assert_eq!(accessible.len(), 2);
    assert_eq!(accessible[0].bounds, surface);
    assert_eq!(
        accessible[1].bounds, surface,
        "descendant remains clipped to the modal surface"
    );
}

#[test]
fn an_invalid_measurement_neither_drops_nor_poisons_the_layout_cache() {
    struct NanOnce {
        failed: bool,
    }
    impl TextShaper for NanOnce {
        fn shape(
            &mut self,
            id: StableNodeId,
            text: &TextContent,
            style: &ComputedStyle,
            constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            let mut metrics = MeasureTextShaper.shape(id, text, style, constraints);
            if text.value == "bad" && !self.failed {
                self.failed = true;
                metrics.width = f32::NAN;
            }
            metrics
        }
    }
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "good".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut shaper = NanOnce { failed: false };
    world.shape_text(&work.text, &mut shaper).unwrap();

    let mut bad = MutationQueue::new();
    bad.create(node(2), document(1), NodeKind::Text);
    bad.set_text(
        node(2),
        TextContent {
            value: "bad".into(),
        },
    );
    bad.create(node(3), document(1), NodeKind::Text);
    bad.set_text(
        node(3),
        TextContent {
            value: "good".into(),
        },
    );
    world.commit(bad).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert!(world.shape_text(&work.text, &mut shaper).is_err());

    // The retry measures the failed text again instead of reading the NaN back,
    // and the text measured before the failure is still cached.
    world.shape_text(&work.text, &mut shaper).unwrap();
    let retried = world.last_work_counters();
    assert!(retried.text_layout_cache_hits >= 1, "{retried:?}");
    assert!(world.text_metrics(node(2)).unwrap().width.is_finite());
}

#[test]
fn text_measured_before_a_font_set_change_is_measured_again_after_it() {
    struct Fonts {
        generation: u64,
    }
    impl TextShaper for Fonts {
        fn shape(
            &mut self,
            id: StableNodeId,
            text: &TextContent,
            style: &ComputedStyle,
            constraints: crate::TextShapeConstraints,
        ) -> TextMetrics {
            let mut metrics = MeasureTextShaper.shape(id, text, style, constraints);
            metrics.height = 10.0 + 7.0 * self.generation as f32;
            metrics
        }

        fn font_generation(&self) -> u64 {
            self.generation
        }
    }
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Text);
    queue.set_text(
        node(1),
        TextContent {
            value: "label".into(),
        },
    );
    world.commit(queue).unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    let mut shaper = Fonts { generation: 0 };
    world.shape_text(&work.text, &mut shaper).unwrap();
    let mut place = MutationQueue::new();
    place.write_layout(
        node(1),
        LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 16.0,
        },
    );
    world.commit(place).unwrap();
    world.take_system_work();
    world
        .shape_text_for_layout(document(1), &mut shaper)
        .unwrap();
    assert_eq!(world.text_metrics(node(1)).unwrap().height, 10.0);

    // A registered face: the same text at the same box is a new measurement,
    // on the scoped pass as well as the scheduled one.
    shaper.generation = 1;
    world
        .shape_text_for_layout_scoped(&[node(1)], &mut shaper)
        .unwrap();
    assert_eq!(world.text_metrics(node(1)).unwrap().height, 17.0);
    shaper.generation = 2;
    world.shape_text(&[node(1)], &mut shaper).unwrap();
    assert_eq!(world.text_metrics(node(1)).unwrap().height, 24.0);
}

/// Issue #101 §1.4 F2: `ThemeMetrics::motion_fast_ms` was a dead field, and
/// the real hover duration was a `const` no theme could move. It is a token
/// now, and this is the proof that the *installed* value is the one played.
///
/// The assertion is on where the cross-fade has got to, not on a stored
/// number: at the old 120 ms default the fade would already be over, so a
/// regression that quietly went back to the constant shows up as a colour
/// that has arrived too early.
#[test]
fn an_installed_hover_duration_is_the_one_the_cross_fade_runs_for() {
    let mut world = UiWorld::new();
    let mut queue = MutationQueue::new();
    queue.create(node(1), document(1), NodeKind::Document);
    queue.create(node(2), document(1), NodeKind::Element { tag: "a".into() });
    queue.insert(node(1), node(2), None);
    queue.set_style(
        node(2),
        NodeStyle {
            background: Some(SemanticColorRole::Surface),
            interaction: crate::InteractionStyle {
                hovered: crate::SemanticPaint {
                    background: Some(SemanticColorRole::Hover),
                    ..crate::SemanticPaint::default()
                },
                ..crate::InteractionStyle::default()
            },
            ..NodeStyle::default()
        },
    );
    world.commit(queue).unwrap();
    world.take_system_work();

    let mut slow = nana_ui_core::ThemeDefinition::NANA_DARK;
    slow.motion.hover_color_ms = 500;
    let mut install = MutationQueue::new();
    install.set_theme_tokens(std::sync::Arc::new(
        slow.bump().compile().expect("compiles"),
    ));
    world.commit(install).unwrap();
    world.take_system_work();

    world
        .set_pointer_hover(document(1), 7, Some(node(2)))
        .unwrap();
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();

    let background = |world: &mut UiWorld| world.extract_nodes(&[node(2)])[0].style.background;
    let palette = nana_ui_core::SemanticPalette::dark();

    // Past the old constant, nowhere near the installed duration.
    world.advance_animations(nana_ui_core::motion::HOVER_COLOR);
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_ne!(
        background(&mut world),
        Some(palette.hover.as_rgba_array()),
        "at 120 ms of a 500 ms fade the hover colour must not have arrived yet"
    );

    world.advance_animations(std::time::Duration::from_millis(500));
    let work = world.take_system_work();
    world.resolve_styles(&work.style).unwrap();
    assert_eq!(
        background(&mut world),
        Some(palette.hover.as_rgba_array()),
        "the fade must finish at the installed duration"
    );
}

/// A fresh world's cached `StyleModelRef` and its `Arc<CompiledTheme>` have to
/// agree, or the first install is compared against the wrong thing and skipped.
///
/// The two are written from different places — `StyleModelRef::default()` and
/// `builtin_theme_arc` — so nothing but this test makes them the same value.
#[test]
fn a_fresh_world_caches_the_style_slice_of_the_theme_it_installed() {
    let world = UiWorld::new();
    assert_eq!(world.style_model(), world.theme().style_model());
    assert_eq!(
        world.theme().identity(),
        nana_ui_core::builtin_theme(ThemeMode::default()).identity()
    );

    // And the comparison an install makes is the one that would catch a drift:
    // installing the same built-in twice must be a no-op.
    let mut world = world;
    let mut queue = MutationQueue::new();
    queue.set_theme_tokens(nana_ui_core::builtin_theme_arc(ThemeMode::default()));
    world.commit(queue).unwrap();
    assert!(
        world.take_system_work().style.is_empty(),
        "re-installing the theme already in place must do no work"
    );
}
