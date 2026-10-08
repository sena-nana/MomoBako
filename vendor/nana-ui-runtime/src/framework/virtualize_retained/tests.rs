use super::*;

fn document() -> DocumentId {
    DocumentId::new(1).unwrap()
}

fn project_layout(cx: &mut AppContext) {
    let work = cx.take_system_work();
    cx.resolve_styles(&work.style).unwrap();
    cx.layout_document(document(), crate::LayoutViewport::new(320.0, 100.0))
        .unwrap();
}

#[test]
fn the_mount_hook_binds_each_new_row_once_and_not_rows_already_mounted() {
    use std::sync::{Arc, Mutex};

    let mut cx = AppContext::new();
    let list = cx.create_component(document(), List::new()).unwrap();
    let mut items = VirtualListItems::<usize, TextInput>::default();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 100));

    let mounted = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&mounted);
    let materialize =
        |cx: &mut AppContext, items: &mut VirtualListItems<usize, TextInput>, offset: f32| {
            let seen = Arc::clone(&seen);
            cx.materialize_virtual_list_retained_with(
                list,
                items,
                &layout,
                VirtualViewport::vertical(offset, 40.0, 0.0),
                &[],
                |index| index,
                |key| Some(*key),
                |index, _| TextInput::new(format!("row {index}")),
                move |_cx, _entity, index, _key| {
                    seen.lock().unwrap().push(index);
                    Ok(())
                },
            )
            .unwrap()
        };

    materialize(&mut cx, &mut items, 0.0);
    let first = mounted.lock().unwrap().clone();
    assert!(!first.is_empty(), "the first window mounts rows");

    // Same window again: nothing is newly created, so nothing is reported.
    materialize(&mut cx, &mut items, 0.0);
    assert_eq!(*mounted.lock().unwrap(), first, "no row is bound twice");

    // Scrolling in fresh rows reports only those.
    materialize(&mut cx, &mut items, 200.0);
    let after = mounted.lock().unwrap().clone();
    assert!(after.len() > first.len());
    assert!(
        after[first.len()..]
            .iter()
            .all(|index| !first.contains(index)),
        "only rows that were not already mounted are reported"
    );
}

#[test]
fn retained_virtual_list_keeps_offscreen_editor_and_sparse_geometry() {
    let mut cx = AppContext::new();
    let list = cx.create_component(document(), List::new()).unwrap();
    let mut items = VirtualListItems::<usize, TextInput>::default();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 1_000_000));
    let materialize =
        |cx: &mut AppContext, items: &mut VirtualListItems<usize, TextInput>, offset| {
            let mut key_reads = 0;
            cx.materialize_virtual_list_retained_in(
                list,
                items,
                &layout,
                VirtualViewport::vertical(offset, 100.0, 0.0),
                &[],
                |index| {
                    key_reads += 1;
                    index
                },
                |key| Some(*key),
                |index, _| TextInput::new(format!("draft {index}")),
            )
            .unwrap();
            assert!(key_reads <= 8, "must not scan the million logical keys");
        };
    materialize(&mut cx, &mut items, 0.0);
    let editor = items.entity(&2).unwrap();
    cx.focus_node(document(), editor.id).unwrap();
    cx.set_ime_preedit(document(), "拼".into(), None).unwrap();
    materialize(&mut cx, &mut items, 10_000_000.0);
    assert_eq!(
        items.mounted_keys(),
        &[2, 500000, 500001, 500002, 500003, 500004]
    );
    assert_eq!(items.entity(&2), Some(editor));
    assert_eq!(cx.world().focused(document()), Some(editor.id));
    assert_eq!(cx.world().ime(editor.id).unwrap().text, "拼");
    assert_eq!(
        cx.read(editor, |field| field.state.value.clone()).unwrap(),
        "draft 2"
    );
    project_layout(&mut cx);
    assert_eq!(cx.world().layout_box(list.id).unwrap().height, 20_000_000.0);
    assert_eq!(
        cx.world().layout_box(items.containers[&2].id).unwrap().y,
        40.0
    );
    assert_eq!(
        cx.world()
            .layout_box(items.containers[&500000].id)
            .unwrap()
            .y,
        10_000_000.0
    );
    let generation = cx.world().generation();
    materialize(&mut cx, &mut items, 10_000_001.0);
    // This offset adds a partially visible row; return to the exact window,
    // then verify a repeated request performs no Runtime commit.
    materialize(&mut cx, &mut items, 10_000_000.0);
    let stable = cx.world().generation();
    assert!(stable >= generation);
    materialize(&mut cx, &mut items, 10_000_000.0);
    assert_eq!(cx.world().generation(), stable);
    cx.clear_ime(document()).unwrap();
    materialize(&mut cx, &mut items, 10_000_000.0);
    assert!(items.entity(&2).is_some(), "focus survives composition end");
    // Repeatedly traversing distant windows must release inactive rows rather
    // than accumulating one retained entity per visited logical item.
    let mut peak = 0;
    for step in 0..128 {
        materialize(&mut cx, &mut items, step as f32 * 20_000.0);
        peak = peak.max(items.mounted_keys().len());
    }
    assert!(
        peak <= 8,
        "virtual list retained {peak} rows after scrolling"
    );
    let mut mutations = MutationQueue::new();
    mutations.request_focus(document(), None);
    cx.commit_mutations(mutations).unwrap();
    materialize(&mut cx, &mut items, 10_000_000.0);
    assert_eq!(items.mounted_keys().len(), 5);
    assert!(!cx.world().contains(editor.id));
    assert!(cx.read(editor, |_| ()).is_err());
}

#[test]
fn retained_virtual_list_reorder_resize_and_delete_follow_business_keys() {
    let mut cx = AppContext::new();
    let list = cx.create_component(document(), List::new()).unwrap();
    let mut items = VirtualListItems::<usize, TextInput>::default();
    let mut layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 100));
    cx.materialize_virtual_list_retained_in(
        list,
        &mut items,
        &layout,
        VirtualViewport::vertical(0.0, 100.0, 0.0),
        &[],
        |index| index,
        |key| Some(*key),
        |_, _| TextInput::new("draft"),
    )
    .unwrap();
    let editor = items.entity(&2).unwrap();
    let container = items.containers[&2];
    cx.focus_node(document(), editor.id).unwrap();
    layout.update_item_extent(0, 40.0);
    cx.materialize_virtual_list_retained_in(
        list,
        &mut items,
        &layout,
        VirtualViewport::vertical(1000.0, 100.0, 0.0),
        &[],
        |index| {
            if index == 8 {
                2
            } else if index == 2 {
                8
            } else {
                index
            }
        },
        |key| Some(if *key == 2 { 8 } else { *key }),
        |_, _| TextInput::new("other"),
    )
    .unwrap();
    assert_eq!(items.entity(&2), Some(editor));
    assert_eq!(items.containers[&2], container);
    project_layout(&mut cx);
    assert_eq!(cx.world().layout_box(container.id).unwrap().y, 180.0);
    // Stale inverse points at a different key after deletion: never pin it.
    cx.materialize_virtual_list_retained_in(
        list,
        &mut items,
        &layout,
        VirtualViewport::vertical(1000.0, 100.0, 0.0),
        &[],
        |index| index + 100,
        |_| Some(8),
        |_, _| TextInput::new("replacement"),
    )
    .unwrap();
    assert!(!cx.world().contains(editor.id));
    assert!(!cx.world().contains(container.id));
    assert!(cx.world().focused(document()).is_none());
}

#[test]
fn retained_virtual_list_tracks_descendant_focus_and_cleans_descendant_views() {
    let mut cx = AppContext::new();
    let list = cx.create_component(document(), List::new()).unwrap();
    let mut items = VirtualListItems::<usize, Stack>::default();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 100));
    let fill = |cx: &mut AppContext, items: &mut VirtualListItems<usize, Stack>, offset| {
        cx.materialize_virtual_list_retained_in(
            list,
            items,
            &layout,
            VirtualViewport::vertical(offset, 100.0, 0.0),
            &[],
            |index| index,
            |key| Some(*key),
            |_, _| Stack::column(0.0),
        )
        .unwrap();
    };
    fill(&mut cx, &mut items, 0.0);
    let row = items.entity(&2).unwrap();
    let mut editor = None;
    cx.mount(row, |ui| {
        editor = Some(ui.child("editor", TextInput::new("nested"))?);
        Ok(())
    })
    .unwrap();
    let editor = editor.unwrap();
    cx.focus_node(document(), editor.id).unwrap();
    fill(&mut cx, &mut items, 1000.0);
    assert!(cx.world().contains(editor.id));
    let mut mutations = MutationQueue::new();
    mutations.request_focus(document(), None);
    cx.commit_mutations(mutations).unwrap();
    fill(&mut cx, &mut items, 1000.0);
    assert!(!cx.world().contains(editor.id));
    assert!(!cx.views.contains_key(&editor.id));
    assert_eq!(
        cx.views.len(),
        11,
        "five components and placement containers plus list"
    );
}

#[test]
fn retained_virtual_list_rejects_duplicate_keys_without_partial_publish() {
    let mut cx = AppContext::new();
    let list = cx.create_component(document(), List::new()).unwrap();
    let layout = VirtualListLayout::new([20.0; 20]);
    let mut items = VirtualListItems::<usize, TextInput>::default();
    cx.materialize_virtual_list_retained_in(
        list,
        &mut items,
        &layout,
        VirtualViewport::vertical(0.0, 40.0, 0.0),
        &[],
        |index| index,
        |key| Some(*key),
        |_, _| TextInput::new("draft"),
    )
    .unwrap();
    let before = cx.world().generation();
    let revision = items.materializer.revision();
    let entity = items.entity(&0);
    assert_eq!(
        cx.materialize_virtual_list_retained_in(
            list,
            &mut items,
            &layout,
            VirtualViewport::vertical(80.0, 100.0, 0.0),
            &[],
            |_| 99,
            |_| None,
            |_, _| panic!("invalid plans must not build")
        ),
        Err(FrameworkError::InvalidVirtualization)
    );
    assert_eq!(cx.world().generation(), before);
    assert_eq!(items.materializer.revision(), revision);
    assert_eq!(items.entity(&0), entity);
}

#[test]
fn retained_virtual_tree_collapse_releases_focused_descendants() {
    let mut cx = AppContext::new();
    let list = cx.create_component(document(), List::new()).unwrap();
    let mut items = VirtualTreeItems::<usize, TextInput>::default();
    let mut layout = VirtualTreeLayout::uniform(20.0, [2, 0, 0, 0, 0]);
    cx.materialize_virtual_tree_retained_in(
        list,
        &mut items,
        &layout,
        VirtualViewport::vertical(0.0, 60.0, 0.0),
        &[],
        |index| index,
        |key| Some(*key),
        |_, _| TextInput::new("tree"),
    )
    .unwrap();
    let editor = items.entity(&1).unwrap();
    cx.focus_node(document(), editor.id).unwrap();
    assert!(layout.collapse(0));
    cx.materialize_virtual_tree_retained_in(
        list,
        &mut items,
        &layout,
        VirtualViewport::vertical(0.0, 60.0, 0.0),
        &[],
        |index| [0, 3, 4][index],
        |key| [0, 3, 4].iter().position(|candidate| candidate == key),
        |_, _| TextInput::new("tree"),
    )
    .unwrap();
    assert_eq!(items.mounted_keys(), &[0, 3, 4]);
    assert!(!cx.world().contains(editor.id));
    assert!(cx.world().focused(document()).is_none());
}

#[test]
fn retained_virtual_list_failed_runtime_commit_keeps_the_previous_window() {
    #[derive(Clone, Debug, PartialEq)]
    struct Item {
        invalid: bool,
    }
    impl ComponentView for Item {
        fn node_kind(&self) -> NodeKind {
            TextInput::new("").node_kind()
        }
        fn project(&self, id: StableNodeId, world: &UiWorld, queue: &mut MutationQueue) {
            TextInput::new("draft").project(id, world, queue);
            if self.invalid {
                queue.insert(id, StableNodeId::new(999999).unwrap(), None);
            }
        }
    }
    let mut cx = AppContext::new();
    let list = cx.create_component(document(), List::new()).unwrap();
    let layout = VirtualListLayout::new([20.0; 20]);
    let mut items = VirtualListItems::<usize, Item>::default();
    cx.materialize_virtual_list_retained_in(
        list,
        &mut items,
        &layout,
        VirtualViewport::vertical(0.0, 40.0, 0.0),
        &[],
        |index| index,
        |key| Some(*key),
        |_, _| Item { invalid: false },
    )
    .unwrap();
    let generation = cx.world().generation();
    let revision = items.materializer.revision();
    let views = cx.views.len();
    let editor = items.entity(&0).unwrap();
    assert!(
        cx.materialize_virtual_list_retained_in(
            list,
            &mut items,
            &layout,
            VirtualViewport::vertical(200.0, 40.0, 0.0),
            &[],
            |index| index,
            |key| Some(*key),
            |_, _| Item { invalid: true }
        )
        .is_err()
    );
    assert_eq!(cx.world().generation(), generation);
    assert_eq!(items.materializer.revision(), revision);
    assert_eq!(cx.views.len(), views);
    assert_eq!(items.entity(&0), Some(editor));
    assert!(cx.world().contains(editor.id));
}

#[test]
fn retained_virtual_table_freezes_both_axes_and_preserves_nested_editor() {
    let mut cx = AppContext::new();
    let table = cx.create_component(document(), Table::new()).unwrap();
    let layout = VirtualTableLayout::new(
        std::iter::repeat_n(20.0, 1000000),
        (0..10000).map(|column| nana_ui_core::TableColumn::new(column.to_string(), 40.0)),
    );
    let mut items = VirtualTableItems::<usize, usize>::default();
    let fill = |cx: &mut AppContext, items: &mut VirtualTableItems<usize, usize>, offset| {
        cx.materialize_virtual_table_retained_in(
            table,
            items,
            &layout,
            VirtualViewport {
                offset,
                extent: [200.0, 100.0],
                overscan: [0.0; 2],
            },
            [1, 1],
            &[],
            |index| index,
            |key| Some(*key),
            |index| index,
            |key| Some(*key),
            |_, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}/{column}")),
        )
        .unwrap();
    };
    fill(&mut cx, &mut items, [0.0; 2]);
    let cell = items.cell_entity(&2, &2).unwrap();
    let mut editor = None;
    cx.mount(cell, |ui| {
        editor = Some(ui.child("editor", TextInput::new("draft"))?);
        Ok(())
    })
    .unwrap();
    let editor = editor.unwrap();
    cx.focus_node(document(), editor.id).unwrap();
    cx.set_ime_preedit(document(), "拼".into(), None).unwrap();
    fill(&mut cx, &mut items, [320000.0, 10000000.0]);
    assert_eq!(items.rows.len(), 6);
    assert_eq!(items.cells.len(), 36);
    assert_eq!(items.cell_entity(&2, &2), Some(cell));
    assert_eq!(cx.world().ime(editor.id).unwrap().text, "拼");
    project_layout(&mut cx);
    let header = items.row_entity(&0).unwrap();
    assert_eq!(
        cx.world()
            .node_style(header.id)
            .unwrap()
            .layout
            .transform
            .unwrap()
            .f,
        10000000.0
    );
    let frozen_column = items.cell_entity(&500001, &0).unwrap();
    assert_eq!(
        cx.world()
            .node_style(frozen_column.id)
            .unwrap()
            .layout
            .transform
            .unwrap()
            .e,
        320000.0
    );
    let generation = cx.world().generation();
    fill(&mut cx, &mut items, [320000.0, 10000000.0]);
    assert_eq!(
        cx.world().generation(),
        generation,
        "unchanged grid must not reorder children"
    );
    cx.focus_node(document(), items.cell_entity(&500001, &8001).unwrap().id)
        .unwrap();
    fill(&mut cx, &mut items, [320000.0, 10000000.0]);
    assert_eq!(items.cells.len(), 25);
    assert!(!cx.world().contains(editor.id));
    assert!(!cx.views.contains_key(&editor.id));
    assert!(!cx.event_dependencies.contains_key(&editor.id));
}

fn scroll_port(cx: &mut AppContext, width: f32, height: f32) -> Entity<crate::ScrollView> {
    let scroll = cx
        .create_component(document(), crate::ScrollView::new(crate::ScrollAxes::Both))
        .unwrap();
    cx.set_scroll_metrics(
        scroll,
        crate::ScrollMetrics {
            viewport_width: width,
            viewport_height: height,
            content_width: width.max(10_000.0),
            content_height: 10_000.0,
            origin_x: 0.0,
            origin_y: 0.0,
        },
    )
    .unwrap();
    scroll
}

#[test]
fn sync_virtual_list_skips_mutation_until_the_visible_range_changes() {
    use std::sync::{Arc, Mutex};

    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 320.0, 50.0);
    let list = cx.create_component(document(), List::new()).unwrap();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 100));
    let mut items = VirtualListItems::<usize, TextInput>::default();
    let mounted = Arc::new(Mutex::new(Vec::new()));
    let sync = |cx: &mut AppContext,
                items: &mut VirtualListItems<usize, TextInput>,
                seen: &Arc<Mutex<Vec<usize>>>| {
        let seen = Arc::clone(seen);
        cx.sync_virtual_list_retained_with(
            scroll,
            list,
            items,
            &layout,
            0.0,
            0,
            &[],
            |index| index,
            |key| Some(*key),
            |index, _| TextInput::new(format!("row {index}")),
            move |_cx, _entity, index, _key| {
                seen.lock().unwrap().push(index);
                Ok(())
            },
        )
        .unwrap()
    };

    let first = sync(&mut cx, &mut items, &mounted);
    assert_eq!(first.range, 0..3);
    let first_keys = items.mounted_keys().to_vec();
    let first_mounted = mounted.lock().unwrap().clone();
    assert_eq!(first_mounted, vec![0, 1, 2]);

    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 5.0 })
        .unwrap();
    let generation = cx.world().generation();
    let again = sync(&mut cx, &mut items, &mounted);
    assert_eq!(again.range, 0..3);
    assert_eq!(items.mounted_keys(), first_keys);
    assert_eq!(*mounted.lock().unwrap(), first_mounted);
    assert_eq!(
        cx.world().generation(),
        generation,
        "unchanged range must not submit Runtime mutation"
    );

    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 20.0 })
        .unwrap();
    let crossed = sync(&mut cx, &mut items, &mounted);
    assert_eq!(crossed.range, 1..4);
    assert_eq!(items.mounted_keys(), &[1, 2, 3]);
    assert_eq!(*mounted.lock().unwrap(), vec![0, 1, 2, 3]);
}

#[test]
fn sync_virtual_list_fingerprint_reacts_to_key_reordering() {
    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 320.0, 50.0);
    let list = cx.create_component(document(), List::new()).unwrap();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 4));
    let mut items = VirtualListItems::<usize, TextInput>::default();
    let mut keys = vec![0, 1, 2, 3];
    let sync = |cx: &mut AppContext,
                items: &mut VirtualListItems<usize, TextInput>,
                keys: &Vec<usize>,
                fingerprint| {
        cx.sync_virtual_list_retained_in(
            scroll,
            list,
            items,
            &layout,
            0.0,
            fingerprint,
            &[],
            |index| keys[index],
            |key| keys.iter().position(|candidate| candidate == key),
            |index, key| TextInput::new(format!("{index}:{key}")),
        )
        .unwrap()
    };

    sync(&mut cx, &mut items, &keys, 1);
    let generation = cx.world().generation();
    keys.swap(0, 1);
    sync(&mut cx, &mut items, &keys, 2);
    assert!(cx.world().generation() > generation);
    assert_eq!(items.mounted_keys(), &[1, 0, 2]);
}

#[test]
fn sync_virtual_tree_uses_the_same_range_gate_as_the_list() {
    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 320.0, 50.0);
    let tree = cx.create_component(document(), List::new()).unwrap();
    let layout = VirtualTreeLayout::uniform(20.0, [0; 20]);
    let mut items = VirtualTreeItems::<usize, TextInput>::default();
    let sync = |cx: &mut AppContext, items: &mut VirtualTreeItems<usize, TextInput>| {
        cx.sync_virtual_tree_retained_in(
            scroll,
            tree,
            items,
            &layout,
            0.0,
            0,
            &[],
            |index| index,
            |key| Some(*key),
            |_, _| TextInput::new("tree"),
        )
        .unwrap()
    };

    assert_eq!(sync(&mut cx, &mut items).range, 0..3);
    let keys = items.mounted_keys().to_vec();
    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 5.0 })
        .unwrap();
    let generation = cx.world().generation();
    assert_eq!(sync(&mut cx, &mut items).range, 0..3);
    assert_eq!(items.mounted_keys(), keys);
    assert_eq!(cx.world().generation(), generation);

    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 20.0 })
        .unwrap();
    assert_eq!(sync(&mut cx, &mut items).range, 1..4);
    assert_eq!(items.mounted_keys(), &[1, 2, 3]);
}

#[test]
fn sync_virtual_table_pins_frozen_cells_without_remounting_an_unchanged_range() {
    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 200.0, 50.0);
    let table = cx.create_component(document(), Table::new()).unwrap();
    let layout = VirtualTableLayout::new(
        std::iter::repeat_n(20.0, 100),
        (0..20).map(|column| nana_ui_core::TableColumn::new(column.to_string(), 40.0)),
    );
    let mut items = VirtualTableItems::<usize, usize>::default();
    let sync = |cx: &mut AppContext, items: &mut VirtualTableItems<usize, usize>| {
        cx.sync_virtual_table_retained_in(
            scroll,
            table,
            items,
            &layout,
            [0.0; 2],
            0,
            [1, 1],
            &[],
            |index| index,
            |key| Some(*key),
            |index| index,
            |key| Some(*key),
            |_, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}/{column}")),
        )
        .unwrap()
    };

    let first = sync(&mut cx, &mut items);
    assert_eq!(first.rows.range.start, 1);
    let rows = items.mounted_rows().to_vec();
    let cells = items.cells.len();
    let header = items.row_entity(&0).unwrap();

    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 5.0 })
        .unwrap();
    let again = sync(&mut cx, &mut items);
    assert_eq!(again.rows.range, first.rows.range);
    assert_eq!(again.columns.range, first.columns.range);
    assert_eq!(items.mounted_rows(), rows);
    assert_eq!(items.cells.len(), cells);
    assert_eq!(
        cx.world()
            .node_style(header.id)
            .unwrap()
            .layout
            .transform
            .unwrap()
            .f,
        5.0
    );

    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 20.0 })
        .unwrap();
    let crossed = sync(&mut cx, &mut items);
    assert_eq!(crossed.rows.range, 2..4);
    assert!(items.mounted_rows().contains(&0));
    assert!(items.mounted_rows().contains(&2));
    assert!(items.mounted_rows().contains(&3));
    assert!(!items.mounted_rows().contains(&1));
}

#[test]
fn sync_virtual_list_releases_an_off_window_editor_after_blur() {
    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 320.0, 50.0);
    let list = cx.create_component(document(), List::new()).unwrap();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 100));
    let mut items = VirtualListItems::<usize, TextInput>::default();
    let sync = |cx: &mut AppContext, items: &mut VirtualListItems<usize, TextInput>| {
        cx.sync_virtual_list_retained_in(
            scroll,
            list,
            items,
            &layout,
            0.0,
            0,
            &[],
            |index| index,
            |key| Some(*key),
            |index, _| TextInput::new(format!("row {index}")),
        )
        .unwrap()
    };

    sync(&mut cx, &mut items);
    let editor = items.entity(&0).unwrap();
    cx.focus_node(document(), editor.id).unwrap();
    cx.set_ime_preedit(document(), "拼".into(), None).unwrap();
    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 20.0 })
        .unwrap();
    assert_eq!(sync(&mut cx, &mut items).range, 1..4);
    assert_eq!(items.entity(&0), Some(editor));

    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 25.0 })
        .unwrap();
    let generation = cx.world().generation();
    assert_eq!(sync(&mut cx, &mut items).range, 1..4);
    assert_eq!(items.entity(&0), Some(editor));
    assert_eq!(cx.world().generation(), generation);

    let mut mutations = MutationQueue::new();
    cx.clear_ime(document()).unwrap();
    mutations.request_focus(document(), None);
    cx.commit_mutations(mutations).unwrap();
    sync(&mut cx, &mut items);
    assert!(items.entity(&0).is_none());
    assert!(!cx.world().contains(editor.id));
    assert_eq!(items.mounted_keys(), &[1, 2, 3]);
}

#[test]
fn sync_virtual_table_releases_an_off_window_cell_after_blur() {
    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 200.0, 50.0);
    let table = cx.create_component(document(), Table::new()).unwrap();
    let layout = VirtualTableLayout::new(
        std::iter::repeat_n(20.0, 100),
        (0..20).map(|column| nana_ui_core::TableColumn::new(column.to_string(), 40.0)),
    );
    let mut items = VirtualTableItems::<usize, usize>::default();
    let sync = |cx: &mut AppContext, items: &mut VirtualTableItems<usize, usize>| {
        cx.sync_virtual_table_retained_in(
            scroll,
            table,
            items,
            &layout,
            [0.0; 2],
            0,
            [1, 1],
            &[],
            |index| index,
            |key| Some(*key),
            |index| index,
            |key| Some(*key),
            |_, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}/{column}")),
        )
        .unwrap()
    };

    sync(&mut cx, &mut items);
    let cell = items.cell_entity(&1, &2).unwrap();
    let mut editor = None;
    cx.mount(cell, |ui| {
        editor = Some(ui.child("editor", TextInput::new("draft"))?);
        Ok(())
    })
    .unwrap();
    let editor = editor.unwrap();
    cx.focus_node(document(), editor.id).unwrap();
    cx.set_ime_preedit(document(), "拼".into(), None).unwrap();
    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 20.0 })
        .unwrap();
    assert_eq!(sync(&mut cx, &mut items).rows.range, 2..4);
    assert_eq!(items.cell_entity(&1, &2), Some(cell));

    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 25.0 })
        .unwrap();
    let generation = cx.world().generation();
    assert_eq!(sync(&mut cx, &mut items).rows.range, 2..4);
    assert_eq!(items.cell_entity(&1, &2), Some(cell));
    assert!(cx.world().generation() > generation);
    let generation = cx.world().generation();
    assert_eq!(sync(&mut cx, &mut items).rows.range, 2..4);
    assert_eq!(cx.world().generation(), generation);

    let mut mutations = MutationQueue::new();
    cx.clear_ime(document()).unwrap();
    mutations.request_focus(document(), None);
    cx.commit_mutations(mutations).unwrap();
    sync(&mut cx, &mut items);
    assert!(items.cell_entity(&1, &2).is_none());
    assert!(!cx.world().contains(cell.id));
    assert!(!items.mounted_rows().contains(&1));
}

#[test]
fn measured_virtual_list_reads_row_heights_back_and_keeps_the_top_row_in_place() {
    // 行高只有布局之后才知道(换行、截断、展开):按估算挂上去,下一次同步
    // 从布局读回真实高度。视口顶上那一行不能因为上面的行变高而被推走。
    let heights = [50.0f32, 50.0, 30.0, 30.0, 30.0, 30.0];
    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 320.0, 40.0);
    let list = cx.create_component(document(), List::new()).unwrap();
    let mut layout = VirtualListLayout::new(std::iter::repeat_n(20.0, heights.len()));
    let mut items = VirtualListItems::<usize, crate::Stack>::measured();
    let row = |index: usize, _: &usize| {
        crate::Stack::column(0.0).style(crate::NodeStyle {
            layout: Arc::new(nana_ui_core::LayoutStyle {
                height: Some(LengthSpec::Px(heights[index])),
                ..Default::default()
            }),
            ..Default::default()
        })
    };
    let sync = |cx: &mut AppContext,
                items: &mut VirtualListItems<usize, crate::Stack>,
                layout: &mut VirtualListLayout| {
        cx.sync_virtual_list_measured_with(
            scroll,
            list,
            items,
            layout,
            100.0,
            0,
            &[],
            |index| index,
            |key| Some(*key),
            row,
            |_, _, _, _| Ok(()),
        )
        .unwrap()
    };

    // 估算 20:第 2 行的顶在 40。
    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 40.0 })
        .unwrap();
    sync(&mut cx, &mut items, &mut layout);
    assert!(items.pending_measure(), "fresh rows wait for a layout pass");
    project_layout(&mut cx);
    sync(&mut cx, &mut items, &mut layout);

    assert_eq!(layout.extent(0..1), 50.0);
    assert_eq!(layout.extent(0..2), 100.0);
    assert_eq!(
        cx.world().scroll_offset(scroll.id).unwrap().y,
        100.0,
        "the row that was at the top stays at the top"
    );
    project_layout(&mut cx);
    let placed = items
        .containers
        .get(&2)
        .and_then(|container| cx.world().layout_box(container.id))
        .zip(cx.world().layout_box(list.id))
        .map(|(row, list)| row.y - list.y);
    assert_eq!(
        placed,
        Some(100.0),
        "row 2 is placed after the measured rows"
    );
}

#[test]
fn sync_virtual_list_windows_a_list_below_other_scroll_content() {
    // 列表不必是整个滚动内容:页头在上面占 300,视口要先扣掉它才落到列表上。
    let mut cx = AppContext::new();
    let scroll = scroll_port(&mut cx, 320.0, 100.0);
    cx.update_component(scroll, |scroll, _| {
        Arc::make_mut(&mut scroll.style.layout).height = Some(LengthSpec::Px(100.0));
    })
    .unwrap();
    let header = cx
        .create_component(
            document(),
            crate::Stack::column(0.0).style(crate::NodeStyle {
                layout: Arc::new(nana_ui_core::LayoutStyle {
                    height: Some(LengthSpec::Px(300.0)),
                    flex_shrink: Some(0.0),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        )
        .unwrap();
    let list = cx.create_component(document(), List::new()).unwrap();
    cx.append_child(scroll, header).unwrap();
    cx.append_child(scroll, list).unwrap();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 100));
    let mut items = VirtualListItems::<usize, TextInput>::default();
    let sync = |cx: &mut AppContext, items: &mut VirtualListItems<usize, TextInput>| {
        cx.sync_virtual_list_retained_in(
            scroll,
            list,
            items,
            &layout,
            0.0,
            0,
            &[],
            |index| index,
            |key| Some(*key),
            |index, _| TextInput::new(format!("row {index}")),
        )
        .unwrap()
    };
    sync(&mut cx, &mut items);
    project_layout(&mut cx);
    let start =
        cx.world().layout_box(list.id).unwrap().y - cx.world().layout_box(scroll.id).unwrap().y;
    assert_eq!(start, 300.0);

    // 视口整个落在页头上:只留窗口保底的第一行,不按滚动偏移去挂列表中段。
    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 150.0 })
        .unwrap();
    assert_eq!(sync(&mut cx, &mut items).range, 0..1);
    // 视口下半截压到列表开头:只挂那一截里的行。
    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 250.0 })
        .unwrap();
    assert_eq!(sync(&mut cx, &mut items).range, 0..3);
    // 滚进列表 50:从第 2 行开始。
    cx.scroll_to(scroll, crate::ScrollOffset { x: 0.0, y: 350.0 })
        .unwrap();
    assert_eq!(sync(&mut cx, &mut items).range, 2..8);
}
