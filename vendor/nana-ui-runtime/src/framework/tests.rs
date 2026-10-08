use super::*;
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};

use crate::{
    Activate, AnimationId, AnimationSpec, Button, Card, Checkbox, Easing, IconButton, List,
    ListItem, NodeStyle, RangeAdjustment, RangeChanged, RangeField, RangeInput, ScrollAxes,
    ScrollChanged, ScrollView, SegmentedControl, SegmentedOption, SegmentedSelectionRequested,
    Stack, StandardVisual, Switch, TabOption, Table, TableCell, TableCellFocused, TableNavigation,
    TableRow, Tabs, Text, TextArea, TextCaretIntent, TextChanged, TextCodeFold, TextContent,
    TextInlay, TextInput, TextSelection, ToggleChanged,
};

#[test]
fn content_sized_checkbox_reserves_indicator_and_label_width() {
    let mut context = AppContext::new();
    let document = DocumentId::new(915).unwrap();
    let root = context
        .create_component(document, Stack::column(4.0))
        .unwrap();
    for size in [
        nana_ui_core::ControlSize::Small,
        nana_ui_core::ControlSize::Medium,
        nana_ui_core::ControlSize::Large,
    ] {
        let mut checkbox = Checkbox::new("启用这条记忆", true).size(size);
        Arc::make_mut(&mut checkbox.style.layout).width = Some(LengthSpec::Shrink);
        let checkbox = context
            .create_detached_component(document, checkbox)
            .unwrap();
        context.append_child(root, checkbox).unwrap();
        context
            .shape_text(&[checkbox.id], &mut crate::MeasureTextShaper)
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
            .unwrap();
        context
            .shape_text_for_layout(document, &mut crate::MeasureTextShaper)
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
            .unwrap();
        let bounds = context.world().layout_box(checkbox.id).unwrap();
        let text = context.world().text_metrics(checkbox.id).unwrap();
        assert!(text.width > 0.0, "the label must have been shaped");
        assert!(
            bounds.width >= text.width + size.indicator_size() + size.indicator_gap() - 0.01,
            "checkbox must reserve its label and indicator: {bounds:?}, {text:?}"
        );
        let mut empty = Checkbox::new("", false).size(size);
        Arc::make_mut(&mut empty.style.layout).width = Some(LengthSpec::Shrink);
        let empty = context.create_detached_component(document, empty).unwrap();
        context.append_child(root, empty).unwrap();
        context
            .shape_text(&[empty.id], &mut crate::MeasureTextShaper)
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
            .unwrap();
        assert_eq!(
            context.world().layout_box(empty.id).unwrap().width,
            size.indicator_size(),
            "an empty label does not reserve a text gap"
        );
    }
}

#[derive(Debug)]
struct Counter {
    value: usize,
}

struct Increment(usize);
struct Cascade;

#[test]
fn scoped_layout_shrink_clamps_scroll_and_anchor_restore_requests_layout() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Always,
    );
    let viewport = crate::LayoutViewport::new(200.0, 120.0);
    context.layout_document(document, viewport).unwrap();
    context
        .scroll_to(scroll, ScrollOffset { x: 0.0, y: 80.0 })
        .unwrap();
    let rows = context.world.node(scroll.id).unwrap().children;
    let mut mutations = MutationQueue::new();
    for &row in &rows {
        let mut style = context.world.node_style(row).unwrap().clone();
        Arc::make_mut(&mut style.layout).height = Some(LengthSpec::Px(10.0));
        mutations.set_style(row, style);
    }
    context.commit_mutations(mutations).unwrap();
    context
        .layout_document_scoped(document, viewport, &rows)
        .unwrap();
    assert_eq!(context.world.scroll_offset(scroll.id).unwrap().y, 0.0);
    assert_eq!(
        context
            .world
            .scroll_metrics(scroll.id)
            .unwrap()
            .content_height,
        120.0
    );
    context.take_system_work();
    let anchor = context
        .capture_scroll_anchor(scroll, rows[0])
        .unwrap()
        .unwrap();
    context.restore_scroll_anchor(scroll, anchor).unwrap();
    let work = context.take_system_work();
    assert!(work.layout.contains(&scroll.id));
    context
        .layout_document_scoped(document, viewport, &work.layout)
        .unwrap();
    assert!(
        context
            .read(scroll, |view| view.pending_anchor.is_none())
            .unwrap()
    );
}

#[test]
fn scoped_scroll_targets_follow_document_order_and_skip_unrelated_branches() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let other = DocumentId::new(2).unwrap();
    let root = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let first = context
        .create_component(document, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    let second = context
        .create_component(document, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    context.append_child(root, second).unwrap();
    context.append_child(root, first).unwrap();
    let inner = context
        .create_component(document, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    context.append_child(first, inner).unwrap();
    let leaf = context
        .create_component(document, Text::new("changed"))
        .unwrap();
    context.append_child(inner, leaf).unwrap();
    for _ in 0..1000 {
        let unrelated = context
            .create_detached_component(document, Text::new("unrelated"))
            .unwrap();
        context.append_child(second, unrelated).unwrap();
    }
    let foreign = context
        .create_component(other, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    context.last_layout_scope = vec![leaf.id, leaf.id, foreign.id];
    assert_eq!(
        context.scoped_scroll_retention_targets(document),
        vec![first.id, inner.id]
    );
    context.last_layout_scope.push(second.id);
    assert_eq!(
        context.scoped_scroll_retention_targets(document),
        vec![second.id, first.id, inner.id]
    );
    context.last_layout_scope = vec![root.id];
    assert!(context.scoped_scroll_retention_targets(document).is_empty());
    let mut mutations = MutationQueue::new();
    mutations.park_subtree(first.id);
    context.commit_mutations(mutations).unwrap();
    context.last_layout_scope = vec![leaf.id];
    assert!(context.scoped_scroll_retention_targets(document).is_empty());
}

#[test]
fn deleting_a_child_releases_its_layout_cache_before_another_frame() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let root = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let child = context
        .create_detached_component(document, Text::new("child"))
        .unwrap();
    context.append_child(root, child).unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(320.0, 200.0))
        .unwrap();
    assert!(
        context
            .layout_cache
            .used_padding(document, child.id)
            .is_some()
    );
    let mut mutations = MutationQueue::new();
    mutations.despawn_subtree(child.id);
    context.commit_mutations(mutations).unwrap();
    assert!(
        context
            .layout_cache
            .used_padding(document, child.id)
            .is_none()
    );
    assert!(
        context
            .layout_cache
            .used_padding(document, root.id)
            .is_some()
    );
}

#[test]
fn closing_one_document_releases_its_layout_without_invalidating_another() {
    let mut context = AppContext::new();
    let first = DocumentId::new(1).unwrap();
    let second = DocumentId::new(2).unwrap();
    let first_root = context.create_component(first, Stack::column(0.0)).unwrap();
    let second_root = context
        .create_component(second, Stack::column(0.0))
        .unwrap();
    let viewport = crate::LayoutViewport::new(320.0, 200.0);
    context.layout_document(first, viewport).unwrap();
    context.layout_document(second, viewport).unwrap();
    assert!(
        context
            .layout_cache
            .used_padding(first, first_root.id)
            .is_some()
    );
    assert!(
        context
            .layout_cache
            .used_padding(second, second_root.id)
            .is_some()
    );
    context.remove_view(first_root).unwrap();
    assert!(
        context
            .layout_cache
            .used_padding(first, first_root.id)
            .is_none()
    );
    assert!(
        context
            .layout_cache
            .used_padding(second, second_root.id)
            .is_some()
    );
    let mut mutations = MutationQueue::new();
    mutations.park_subtree(second_root.id);
    context.commit_mutations(mutations).unwrap();
    assert!(
        context
            .layout_cache
            .used_padding(second, second_root.id)
            .is_none()
    );
    let replacement_root = context
        .create_component(second, Stack::column(0.0))
        .unwrap();
    context.append_child(replacement_root, second_root).unwrap();
    context.layout_document(second, viewport).unwrap();
    assert!(
        context
            .layout_cache
            .used_padding(second, second_root.id)
            .is_some()
    );
}

#[test]
fn bind_component_requires_an_existing_node_then_enables_read() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let id = StableNodeId::new(7).unwrap();
    let button = Button::new("Go");
    assert_eq!(
        context.bind_component(id, button.clone()),
        Err(FrameworkError::MissingView(id))
    );
    let mut queue = MutationQueue::new();
    queue.create(id, document, button.node_kind());
    context.commit_mutations(queue).unwrap();
    let entity = context.bind_component(id, button).unwrap();
    assert_eq!(entity.stable_id(), id);
    assert_eq!(
        context
            .read(entity, |button| button.label.to_string())
            .unwrap(),
        "Go"
    );
    assert_eq!(context.world().text(id), Some("Go"));
}

#[test]
fn mount_reuses_keyed_entities_and_drops_unused() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let card = context.create_component(document, Card::new()).unwrap();
    let mut title = None;
    let mut save = None;
    context
        .mount(card, |ui| {
            title = Some(ui.child("title", Text::new("Nana"))?);
            save = Some(ui.child("save", Button::new("Save"))?);
            Ok(())
        })
        .unwrap();
    let title = title.unwrap();
    let save = save.unwrap();
    let title_id = title.stable_id();
    let save_id = save.stable_id();
    context
        .mount(card, |ui| {
            ui.child("title", Text::new("Nana"))?;
            ui.child("save", Button::new("Saved"))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(title.stable_id(), title_id);
    assert_eq!(save.stable_id(), save_id);
    assert_eq!(
        context.read(save, |button| button.label.clone()).unwrap(),
        "Saved"
    );
    context
        .mount(card, |ui| {
            ui.child("title", Text::new("Nana"))?;
            Ok(())
        })
        .unwrap();
    assert!(context.world().contains(title_id));
    assert!(!context.world().contains(save_id));
}

#[test]
fn build_commits_nested_tree_once_and_installs_handlers() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let before = context.world().generation();
    let start = context
        .build(document, |ui| {
            let column = ui.child("column", Stack::column(12.0));
            ui.nest(column, |ui| {
                ui.child("title", Text::new("你好"));
                let start = ui.child("start", Button::new("开始"));
                ui.on(start, |_, _: &Activate, cx| {
                    cx.dispatch_program("start");
                });
                let row = ui.child("row", Stack::row(8.0));
                ui.nest(row, |ui| {
                    ui.child("open", Button::new("打开"));
                    ui.child("float", Button::new("浮窗"));
                });
                start
            })
        })
        .unwrap();
    assert_eq!(context.world().generation(), before + 1);

    let start_node = context.world().node(start.stable_id()).unwrap();
    let column = start_node.parent.expect("start lives under column");
    let column_children = context.world().node(column).unwrap().children;
    assert_eq!(column_children.len(), 3);
    assert_eq!(context.world().text(column_children[0]), Some("你好"));
    assert_eq!(column_children[1], start.stable_id());
    let row = column_children[2];
    let row_children = context.world().node(row).unwrap().children;
    assert_eq!(row_children.len(), 2);
    assert_eq!(
        context.read(start, |button| button.label.clone()).unwrap(),
        "开始"
    );
    assert!(context.activate_button(start).unwrap());
    let queued = context.take_program_messages();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].downcast_ref::<&str>().copied(), Some("start"));
}

#[test]
fn create_component_append_commits_once_per_call() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let before = context.world().generation();
    let column = context
        .create_component(document, Stack::column(12.0))
        .unwrap();
    let title = context
        .create_component(document, Text::new("你好"))
        .unwrap();
    let start = context
        .create_component(document, Button::new("开始"))
        .unwrap();
    context.append_child(column, title).unwrap();
    context.append_child(column, start).unwrap();
    assert_eq!(context.world().generation(), before + 5);
}

#[test]
fn a_placed_keyed_child_keeps_its_identity_across_reassembly() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let elsewhere = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let assemble = |context: &mut AppContext, keys: &[&str]| {
        context
            .mount(parent, |scope| {
                for key in keys {
                    scope.child(*key, Stack::column(0.0))?;
                }
                Ok(())
            })
            .unwrap();
    };
    assemble(&mut context, &["a", "b"]);
    let a = context.assembled_child(parent.stable_id(), "a").unwrap();
    let b = context.assembled_child(parent.stable_id(), "b").unwrap();
    context.place_assembled(a, elsewhere.stable_id()).unwrap();
    assert_eq!(context.assembly_key(parent.stable_id(), a), Some("a"));
    // Reassembling the declared parent reuses the identity and leaves the
    // placement alone.
    assemble(&mut context, &["a", "b"]);
    assert_eq!(context.assembled_child(parent.stable_id(), "a"), Some(a));
    assert_eq!(
        context.world().node(a).unwrap().parent,
        Some(elsewhere.stable_id())
    );
    assert_eq!(
        context.world().node(parent.stable_id()).unwrap().children,
        vec![b]
    );
    // Dropping the key despawns the node wherever it is placed.
    assemble(&mut context, &["b"]);
    assert!(!context.world().contains(a));
    assert!(
        context
            .world()
            .node(elsewhere.stable_id())
            .unwrap()
            .children
            .is_empty()
    );
    assert!(context.assembly_path(a).is_none());
}

/// `page / pane / slot` built under `parent`; returns `(page, pane, slot)`.
fn keyed_page(
    context: &mut AppContext,
    parent: Entity<Stack>,
) -> (StableNodeId, StableNodeId, StableNodeId) {
    context
        .build_child(parent, |ui| {
            let page = ui.child("page", Stack::column(0.0));
            ui.nest(page, |ui| {
                ui.child("marker", Stack::column(0.0));
                let pane = ui.child("pane", Stack::column(0.0));
                ui.nest(pane, |ui| ui.child("slot", Stack::column(0.0)).stable_id())
            })
        })
        .unwrap();
    let page = context
        .resolve_assembly_path(parent.stable_id(), "page")
        .unwrap();
    let pane = context.resolve_assembly_path(page, "pane").unwrap();
    let slot = context.resolve_assembly_path(pane, "slot").unwrap();
    (page, pane, slot)
}

#[test]
fn keys_with_the_path_separator_are_rejected_on_every_keyed_entry() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let nodes = context.world().len();
    let built = context.build_child(parent, |ui| {
        ui.child("ok", Stack::column(0.0));
        ui.child("a/b", Stack::column(0.0));
    });
    assert!(matches!(built, Err(FrameworkError::InvalidInput)));
    assert_eq!(
        context.world().len(),
        nodes,
        "a rejected build commits nothing"
    );
    let mounted = context.mount(parent, |scope| {
        scope.child("a/b", Stack::column(0.0))?;
        Ok(())
    });
    assert!(matches!(mounted, Err(FrameworkError::InvalidInput)));
    assert_eq!(context.world().len(), nodes);
}

#[test]
fn resolve_assembly_entity_checks_the_path_and_the_component_type() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    context
        .build_child(parent, |ui| {
            let page = ui.child("page", Stack::column(0.0));
            ui.nest(page, |ui| {
                ui.child("title", Text::new("标题"));
            });
        })
        .unwrap();
    let title = context
        .resolve_assembly_entity::<Text>(parent.stable_id(), "page/title")
        .unwrap();
    assert_eq!(
        context.assembly_path(title.stable_id()).as_deref(),
        Some("page/title")
    );
    assert!(matches!(
        context.resolve_assembly_entity::<Button>(parent.stable_id(), "page/title"),
        Err(FrameworkError::ViewType(_))
    ));
    assert!(matches!(
        context.resolve_assembly_entity::<Text>(parent.stable_id(), "page/missing"),
        Err(FrameworkError::MissingView(_))
    ));
}

#[test]
fn a_despawned_declared_parent_takes_its_placed_child() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let body = context
        .create_component(document, Stack::column(0.0))
        .unwrap()
        .stable_id();
    let (page, _, slot) = keyed_page(&mut context, parent);
    context.place_assembled(slot, body).unwrap();
    // Torn down without reassembly: the placed child goes with its declared
    // parent, and the placement parent survives.
    let mut teardown = MutationQueue::new();
    teardown.despawn_subtree(page);
    context.commit_mutations(teardown).unwrap();
    assert!(!context.world().contains(slot));
    assert!(context.world().node(body).unwrap().children.is_empty());
    assert!(context.assembly_path(slot).is_none());
    assert!(context.view_entity::<Stack>(slot).is_none());
}

#[test]
fn placing_a_node_back_under_its_declared_parent_ends_the_placement() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let body = context
        .create_component(document, Stack::column(0.0))
        .unwrap()
        .stable_id();
    let (_, pane, slot) = keyed_page(&mut context, parent);
    context.place_assembled(slot, body).unwrap();
    context.place_assembled(slot, pane).unwrap();
    assert_eq!(context.world().node(slot).unwrap().parent, Some(pane));
    let mut teardown = MutationQueue::new();
    teardown.despawn_subtree(body);
    context.commit_mutations(teardown).unwrap();
    assert!(context.world().contains(slot));
}

/// With no composition owner guard, a keyed node may be placed inside any
/// subtree. When that subtree dies first, the node dies with it; its
/// declared parent must forget it and reassemble cleanly.
#[test]
fn a_node_placed_into_a_subtree_that_dies_leaves_no_stale_records() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let other = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let (_, other_pane, _) = keyed_page(&mut context, other);
    let (_, pane, slot) = keyed_page(&mut context, parent);
    context.place_assembled(slot, other_pane).unwrap();
    let mut teardown = MutationQueue::new();
    teardown.despawn_subtree(other.stable_id());
    context.commit_mutations(teardown).unwrap();
    assert!(!context.world().contains(slot));
    assert!(context.assembled_child(pane, "slot").is_none());
    assert!(context.assembly_path(slot).is_none());
    let rebuilt = context.mount(Entity::<Stack>::from_stable_id(pane), |scope| {
        scope.child("slot", Stack::column(0.0))?;
        Ok(())
    });
    rebuilt.unwrap();
    let fresh = context.assembled_child(pane, "slot").unwrap();
    assert_ne!(fresh, slot);
    assert_eq!(context.world().node(pane).unwrap().children, vec![fresh]);
}

#[test]
fn place_assembled_rejects_unkeyed_nodes_and_cycles() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let loose = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    context
        .mount(parent, |scope| {
            scope.with_child("outer", Stack::column(0.0), |scope| {
                scope.child("inner", Stack::column(0.0)).map(|_| ())
            })?;
            Ok(())
        })
        .unwrap();
    let outer = context
        .assembled_child(parent.stable_id(), "outer")
        .unwrap();
    let inner = context.assembled_child(outer, "inner").unwrap();
    assert_eq!(
        context.place_assembled(loose.stable_id(), parent.stable_id()),
        Err(FrameworkError::InvalidInput)
    );
    assert!(matches!(
        context.place_assembled(outer, inner),
        Err(FrameworkError::InvalidComponentHierarchy { .. })
    ));
    assert_eq!(context.assembly_path(inner).as_deref(), Some("outer/inner"));
    assert_eq!(
        context.resolve_assembly_path(parent.stable_id(), "outer/inner"),
        Some(inner)
    );
    assert_eq!(
        context.resolve_assembly_path(parent.stable_id(), "outer/none"),
        None
    );
}

#[test]
fn build_rejects_duplicate_keys_without_committing() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let before = context.world().generation();
    let error = context
        .build(document, |ui| {
            let column = ui.child("column", Stack::column(8.0));
            ui.nest(column, |ui| {
                ui.child("save", Button::new("Save"));
                ui.child("save", Button::new("Saved"));
            });
        })
        .unwrap_err();
    assert!(matches!(error, FrameworkError::DuplicateAssemblyKey { .. }));
    assert_eq!(context.world().generation(), before);
    assert!(
        context
            .world()
            .node(StableNodeId::new(1).unwrap())
            .is_none()
    );
}

#[test]
fn build_detached_parks_roots_until_inserted() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (root, child) = context
        .build_detached(document, |ui| {
            let root = ui.child("root", Stack::column(8.0));
            let child = ui.nest(root, |ui| ui.child("child", Text::new("parked")));
            (root, child)
        })
        .unwrap();
    assert_eq!(
        context.world().mount_state(root.stable_id()),
        Some(crate::MountState::Parked)
    );
    assert_eq!(
        context.world().mount_state(child.stable_id()),
        Some(crate::MountState::Parked)
    );
    let host = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    context.append_child(host, root).unwrap();
    assert_eq!(
        context.world().mount_state(root.stable_id()),
        Some(crate::MountState::Mounted)
    );
    assert_eq!(
        context.world().mount_state(child.stable_id()),
        Some(crate::MountState::Mounted)
    );
    assert_eq!(
        context.world().node(root.stable_id()).unwrap().children,
        vec![child.stable_id()]
    );
}

#[test]
fn build_child_keys_are_reused_by_mount() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let card = context.create_component(document, Card::new()).unwrap();
    let save = context
        .build_child(card, |ui| ui.child("save", Button::new("Save")))
        .unwrap();
    let save_id = save.stable_id();
    context
        .mount(card, |ui| {
            ui.child("save", Button::new("Saved"))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(save.stable_id(), save_id);
    assert_eq!(
        context.read(save, |button| button.label.clone()).unwrap(),
        "Saved"
    );
}

#[test]
fn sidebar_row_activate_queues_a_program_message() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let row = context
        .create_component(document, SidebarRow::new("舞台"))
        .unwrap();
    context
        .on(row, |_row, _event: &Activate, cx| {
            cx.dispatch_program("stage");
        })
        .unwrap();
    assert!(context.activate_sidebar_row(row).unwrap());
    let queued = context.take_program_messages();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].downcast_ref::<&str>().copied(), Some("stage"));
    assert!(context.take_program_messages().is_empty());
}

#[test]
fn dispatch_program_keeps_the_latest_message_of_each_type() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let row = context
        .create_component(document, SidebarRow::new("舞台"))
        .unwrap();
    context
        .on(row, |_row, _event: &Activate, cx| {
            cx.dispatch_program("stage");
            cx.dispatch_program("functions");
            cx.dispatch_program(1_u8);
        })
        .unwrap();
    assert!(context.activate_sidebar_row(row).unwrap());
    assert!(context.has_program_messages());
    let queued = context.take_program_messages();
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].downcast_ref::<&str>().copied(), Some("functions"));
    assert_eq!(queued[1].downcast_ref::<u8>().copied(), Some(1));
    assert!(!context.has_program_messages());
}

#[test]
fn dispatch_program_all_keeps_every_message_of_one_type_in_order() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let row = context
        .create_component(document, SidebarRow::new("舞台"))
        .unwrap();
    context
        .on(row, |_row, _event: &Activate, cx| {
            // One application enum would be a single Rust type; coalescing
            // would drop the first action. `dispatch_program_all` keeps both.
            cx.dispatch_program_all("stage");
            cx.dispatch_program_all("functions");
        })
        .unwrap();
    assert!(context.activate_sidebar_row(row).unwrap());
    let queued = context.take_program_messages();
    assert_eq!(queued.len(), 2);
    assert_eq!(queued[0].downcast_ref::<&str>().copied(), Some("stage"));
    assert_eq!(queued[1].downcast_ref::<&str>().copied(), Some("functions"));
}

#[test]
fn plugin_register_activation_reaches_activate_node() {
    #[derive(Clone, PartialEq)]
    struct Ping;
    impl ComponentView for Ping {
        fn node_kind(&self) -> NodeKind {
            NodeKind::Element { tag: "ping".into() }
        }
        fn project(&self, _id: StableNodeId, _world: &UiWorld, _mutations: &mut MutationQueue) {}
    }
    impl crate::RegisterableComponent for Ping {
        const TYPE_ID: &'static str = "test.ping";
        const TAGS: &'static [&'static str] = &["ping"];
        fn from_semantic(_: &crate::SemanticSpec<'_>) -> Self {
            Ping
        }
    }
    fn activate_ping(
        context: &mut AppContext,
        entity: Entity<Ping>,
    ) -> Result<bool, FrameworkError> {
        context.update_component(entity, |_, cx| cx.emit(Activate))?;
        Ok(true)
    }
    struct PingExt;
    impl UiExtension for PingExt {
        fn name(&self) -> &'static str {
            "test.ping"
        }
        fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
            registrar.register_component::<Ping>()?;
            registrar.register_activation::<Ping>(activate_ping)
        }
    }

    let mut context = AppContext::new();
    context.install(&PingExt).unwrap();
    let ping = context
        .create_component(DocumentId::new(1).unwrap(), Ping)
        .unwrap();
    let hits = Arc::new(Mutex::new(0));
    let observed = Arc::clone(&hits);
    context
        .on(ping, move |_, _: &Activate, _| {
            *observed.lock().unwrap() += 1;
        })
        .unwrap();
    assert!(context.activate_node(ping.stable_id()).unwrap());
    assert_eq!(*hits.lock().unwrap(), 1);
}

#[test]
fn typed_view_update_delivers_closure_events_and_commits_one_batch() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let entity = context
        .create_view(document, NodeKind::Text, Counter { value: 0 })
        .unwrap();
    context
        .on(entity, |view, event: &Increment, cx| {
            view.value += event.0;
            let id = cx.entity().stable_id();
            cx.mutations().set_text(
                id,
                TextContent {
                    value: view.value.to_string().into(),
                },
            );
            cx.emit(Cascade);
        })
        .unwrap();
    context
        .on(entity, |view, _event: &Cascade, _cx| view.value += 1)
        .unwrap();

    context
        .update(entity, |_view, cx| cx.emit(Increment(2)))
        .unwrap();
    assert_eq!(context.read(entity, |view| view.value).unwrap(), 3);
    assert_eq!(context.world().generation(), 2);
    assert!(
        context
            .compat_world_mut()
            .take_system_work()
            .text
            .contains(&entity.stable_id())
    );
}

#[test]
fn forged_view_type_is_an_error_and_does_not_remove_state() {
    let mut context = AppContext::new();
    let entity = context
        .create_view(
            DocumentId::new(1).unwrap(),
            NodeKind::Document,
            Counter { value: 7 },
        )
        .unwrap();
    let wrong = Entity::<String>::from_stable_id(entity.stable_id());
    assert_eq!(
        context.update(wrong, |_, _| ()),
        Err(FrameworkError::ViewType(entity.stable_id()))
    );
    assert_eq!(context.read(entity, |view| view.value).unwrap(), 7);
}

#[test]
fn native_components_project_final_event_state_into_one_retained_tree() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let list = context
        .create_component(document, List::new().label("Actions"))
        .unwrap();
    let button = context
        .create_component(document, Button::new("Build"))
        .unwrap();
    let input = context
        .create_component(document, TextInput::new("你好ab").label("Name"))
        .unwrap();
    context.append_child(list, button).unwrap();
    context.append_child(list, input).unwrap();
    context
        .on(button, |button, _event: &Activate, _cx| {
            button.label = "Running".into();
        })
        .unwrap();
    let observed_change = Arc::new(Mutex::new(None));
    let observer = Arc::clone(&observed_change);
    context
        .on(input, move |_input, event: &TextChanged, _cx| {
            *observer.lock().unwrap() = Some(event.clone());
        })
        .unwrap();

    assert!(context.activate_button(button).unwrap());
    context
        .update_component(input, |input, _cx| {
            input.state.selection = TextSelection::new(0, "你".len());
        })
        .unwrap();
    assert!(context.replace_text_input_selection(input, "娜").unwrap());

    assert_eq!(context.world().text(button.stable_id()), Some("Running"));
    assert_eq!(
        context.world().text_input(input.stable_id()).unwrap().value,
        "娜好ab"
    );
    assert_eq!(context.world().text(input.stable_id()), Some("娜好ab"));
    assert_eq!(
        observed_change.lock().unwrap().as_ref().unwrap().selection,
        TextSelection::caret("娜".len())
    );
    assert_eq!(
        context.world().node(list.stable_id()).unwrap().children,
        vec![button.stable_id(), input.stable_id()]
    );
    let accessibility = context.world().project_accessibility(document);
    assert_eq!(accessibility[0].role, crate::AccessibilityRole::List);
    assert_eq!(accessibility[1].role, crate::AccessibilityRole::Button);
    assert_eq!(accessibility[1].label.as_deref(), Some("Running"));
    assert_eq!(accessibility[2].role, crate::AccessibilityRole::TextInput);
    assert_eq!(accessibility[2].value.as_deref(), Some("娜好ab"));
    let extracted_button = context
        .world()
        .extract_document(document)
        .into_iter()
        .find(|node| node.id == button.stable_id())
        .unwrap();
    assert_eq!(extracted_button.text.unwrap().value, "Running");

    let generation = context.world().generation();
    context.update_component(button, |_button, _cx| {}).unwrap();
    assert_eq!(context.world().generation(), generation);
}

#[test]
fn loading_button_owns_size_semantics_animation_and_activation_gate() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            Button::new("Deploy")
                .kind(nana_ui_core::ButtonKind::Warning)
                .size(nana_ui_core::ControlSize::Large)
                .loading(true)
                .invalid(true),
        )
        .unwrap();

    assert!(!context.activate_button(button).unwrap());
    assert_eq!(context.next_animation_deadline(), Some(Duration::ZERO));
    // The authored style names the step; the number is produced against the
    // installed metrics (Issue #101 F1).
    assert_eq!(
        context
            .world()
            .node_style(button.stable_id())
            .unwrap()
            .control_height,
        Some(nana_ui_core::ControlHeight::Min(
            nana_ui_core::ControlSize::Large
        ))
    );
    assert_eq!(
        context.world().extract_nodes(&[button.stable_id()])[0]
            .source_style
            .layout
            .min_height,
        Some(nana_ui_core::LengthSpec::Px(
            nana_ui_core::ControlSize::Large.height_in(nana_ui_core::UI_METRICS)
        ))
    );
    let accessibility = context.world().accessibility(button.stable_id()).unwrap();
    assert!(accessibility.disabled);
    assert!(accessibility.busy);
    assert!(accessibility.invalid);
    assert!(matches!(
        context.world().standard_visual(button.stable_id()),
        Some(StandardVisual::Button {
            kind: nana_ui_core::ButtonKind::Warning,
            size: nana_ui_core::ControlSize::Large,
            loading: true,
            invalid: true,
            ..
        })
    ));

    let frame = context.advance_animations(Duration::from_millis(400));
    assert_eq!(frame.component_updates, vec![button.stable_id()]);
    assert_eq!(frame.next_deadline, Some(Duration::from_millis(416)));
    assert!(matches!(
        context.world().standard_visual(button.stable_id()),
        Some(StandardVisual::Button {
            loading_phase,
            ..
        }) if (loading_phase - 0.5).abs() < f32::EPSILON
    ));

    context
        .update_component(button, |button, _cx| button.loading = false)
        .unwrap();
    assert_eq!(context.next_animation_deadline(), None);
    assert!(context.activate_button(button).unwrap());
}

#[test]
fn text_input_owns_editability_privacy_size_and_busy_semantics() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            TextInput::new("secret")
                .placeholder("Password")
                .size(nana_ui_core::ControlSize::Large)
                .read_only(true)
                .secure(true)
                .invalid(true),
        )
        .unwrap();

    assert!(context.focus_node(document, input.stable_id()).unwrap());
    assert!(!context.replace_text_input_selection(input, "x").unwrap());
    assert!(
        !context
            .set_ime_preedit(document, "输入".into(), None)
            .unwrap()
    );
    let node = context
        .world()
        .project_accessibility(document)
        .into_iter()
        .find(|node| node.id == input.stable_id())
        .unwrap();
    assert!(node.focused);
    assert!(!node.editable);
    assert!(node.invalid);
    assert_eq!(node.value, None);
    // Nor does an unlabelled one fall back to its text for a name.
    let unlabelled = context
        .create_component(document, TextInput::new("hunter2").secure(true))
        .unwrap();
    let projected = context.world().project_accessibility(document);
    let unlabelled = projected
        .iter()
        .find(|node| node.id == unlabelled.stable_id())
        .unwrap();
    assert_eq!(
        (unlabelled.label.as_deref(), unlabelled.value.as_deref()),
        (None, None)
    );
    assert!(
        projected
            .iter()
            .all(|node| node.label.as_deref() != Some("hunter2")),
        "no node names the field by its secret"
    );
    // The authored style names the step; the number is produced against the
    // installed metrics (Issue #101 F1).
    assert_eq!(
        context
            .world()
            .node_style(input.stable_id())
            .unwrap()
            .control_height,
        Some(nana_ui_core::ControlHeight::Min(
            nana_ui_core::ControlSize::Large
        ))
    );
    assert_eq!(
        context.world().extract_nodes(&[input.stable_id()])[0]
            .source_style
            .layout
            .min_height,
        Some(nana_ui_core::LengthSpec::Px(
            nana_ui_core::ControlSize::Large.height_in(nana_ui_core::UI_METRICS)
        ))
    );

    context
        .update_component(input, |input, _cx| {
            input.read_only = false;
            input.loading = true;
        })
        .unwrap();
    let state = context.world().accessibility(input.stable_id()).unwrap();
    assert!(state.disabled);
    assert!(state.busy);
    assert!(!state.editable);
    assert!(!context.replace_text_input_selection(input, "x").unwrap());

    context
        .update_component(input, |input, _cx| input.loading = false)
        .unwrap();
    assert_eq!(context.world().focused(document), Some(input.stable_id()));
    assert!(
        context
            .set_ime_preedit(document, "输入".into(), None)
            .unwrap()
    );
}

#[test]
fn text_input_placeholder_uses_layout_color_and_opacity() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut field = TextInput::new("").placeholder("Hint");
    {
        let layout = Arc::make_mut(&mut field.style.layout);
        layout.placeholder_color = Some([1.0, 0.0, 0.0, 1.0]);
        layout.placeholder_opacity = Some(0.5);
    }
    let input = context.create_component(document, field).unwrap();
    let mut mutations = MutationQueue::new();
    mutations.write_layout(
        input.stable_id(),
        crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 32.0,
        },
    );
    context.commit_mutations(mutations).unwrap();
    context
        .compat_world_mut()
        .resolve_styles(&[input.stable_id()])
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&[input.stable_id()], &mut crate::MeasureTextShaper)
        .unwrap();

    match context.world().component_geometry(input.stable_id()) {
        Some(crate::ComponentGeometry::TextInput { text, .. }) => {
            assert_eq!(text.color, Some([1.0, 0.0, 0.0, 0.5]));
        }
        other => panic!("expected text input geometry, got {other:?}"),
    }
}

#[test]
fn card_icon_button_and_list_item_keep_visual_and_semantic_content_distinct() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let card = context
        .create_component(document, Card::new().label("Build actions"))
        .unwrap();
    let icon = context
        .create_component(
            document,
            IconButton::new(nana_ui_core::Icon::Add, "Add source"),
        )
        .unwrap();
    let item = context
        .create_component(document, ListItem::new("Camera").selected(true))
        .unwrap();
    context.append_child(card, icon).unwrap();
    context.append_child(card, item).unwrap();
    context
        .on(icon, |button, _event: &Activate, _cx| {
            button.selected = true;
        })
        .unwrap();
    context
        .on(item, |item, _event: &Activate, _cx| {
            item.selected = false;
        })
        .unwrap();

    assert!(context.activate_icon_button(icon).unwrap());
    assert!(context.activate_list_item(item).unwrap());
    assert_eq!(context.world().text(icon.stable_id()), Some(""));
    assert_eq!(
        context.world().standard_visual(icon.stable_id()),
        Some(StandardVisual::Icon {
            icon: nana_ui_core::Icon::Add,
            size: nana_ui_core::ControlSize::Medium.icon_size(),
            tooltip: None,
        })
    );
    assert_eq!(context.world().text(item.stable_id()), Some("Camera"));

    let nodes = context.world().project_accessibility(document);
    let icon_node = nodes
        .iter()
        .find(|node| node.id == icon.stable_id())
        .unwrap();
    assert_eq!(icon_node.role, crate::AccessibilityRole::Button);
    assert_eq!(icon_node.label.as_deref(), Some("Add source"));
    assert_eq!(
        context.world().extract_nodes(&[icon.stable_id()])[0]
            .source_style
            .layout
            .min_width,
        Some(nana_ui_core::LengthSpec::Px(
            nana_ui_core::UI_METRICS.icon_button_size
        ))
    );
    let item_node = nodes
        .iter()
        .find(|node| node.id == item.stable_id())
        .unwrap();
    assert_eq!(item_node.role, crate::AccessibilityRole::ListItem);
    assert_eq!(item_node.selected, Some(false));
    assert_eq!(
        context
            .world()
            .node_style(card.stable_id())
            .unwrap()
            .layout
            .padding_top,
        Some(nana_ui_core::LengthSpec::Px(
            nana_ui_core::UI_METRICS.panel_padding_y + 24.0
        ))
    );
    assert_eq!(
        context.world().node(card.stable_id()).unwrap().children,
        vec![icon.stable_id(), item.stable_id()]
    );
}

#[test]
fn text_area_reuses_utf8_editing_and_projects_multiline_semantics() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, TextArea::new("第一行\nsecond").label("Notes"))
        .unwrap();
    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::new("第一".len(), "第一行\n".len());
        })
        .unwrap();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&changes);
    context
        .on(area, move |_area, event: &TextChanged, _cx| {
            observed.lock().unwrap().push(event.clone());
        })
        .unwrap();

    assert!(context.replace_text_area_selection(area, "段落\n").unwrap());
    assert_eq!(
        context.world().text_input(area.stable_id()).unwrap().value,
        "第一段落\nsecond"
    );
    assert_eq!(changes.lock().unwrap().len(), 1);
    let accessibility = context.world().project_accessibility(document);
    assert_eq!(accessibility[0].role, crate::AccessibilityRole::TextInput);
    assert_eq!(accessibility[0].label.as_deref(), Some("Notes"));
    assert!(accessibility[0].multiline);
    assert_eq!(accessibility[0].value.as_deref(), Some("第一段落\nsecond"));
}

#[test]
fn text_area_projects_git_gutter_marks_into_the_visual() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("a\nb").git_gutter(Arc::from([
                crate::TextGitMark::new(1, crate::TextGitMarkKind::Added),
                crate::TextGitMark::new(2, crate::TextGitMarkKind::Deleted),
            ])),
        )
        .unwrap();

    let Some(StandardVisual::TextInput { git_marks, .. }) =
        context.world().standard_visual(area.stable_id())
    else {
        panic!("expected text input visual");
    };
    assert_eq!(
        git_marks.as_ref(),
        &[
            crate::TextGitMark::new(1, crate::TextGitMarkKind::Added),
            crate::TextGitMark::new(2, crate::TextGitMarkKind::Deleted),
        ]
    );
}

#[test]
fn text_area_projects_visual_state_and_deletes_a_whole_grapheme() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let emoji = "👩‍💻";
    let area = context
        .create_component(
            document,
            TextArea::new(format!("{emoji}\n界"))
                .placeholder("Write notes")
                .invalid(true)
                .height(144.0)
                .scroll_offset(ScrollOffset { x: 4.0, y: 12.0 }),
        )
        .unwrap();

    assert!(matches!(
        context.world().standard_visual(area.stable_id()),
        Some(StandardVisual::TextInput {
            placeholder,
            secure: false,
            invalid: true,
            ..
        }) if placeholder.as_ref() == "Write notes"
    ));
    assert_eq!(
        context.world().node_style(area.stable_id()).unwrap().border,
        Some(nana_ui_core::SemanticColorRole::Danger)
    );
    assert_eq!(
        context.world().scroll_offset(area.stable_id()),
        Some(ScrollOffset { x: 4.0, y: 12.0 })
    );
    assert_eq!(
        context
            .world()
            .node_style(area.stable_id())
            .unwrap()
            .layout
            .height,
        Some(LengthSpec::Px(144.0))
    );

    context
        .update_component(area, |area, _cx| {
            area.state.selection = TextSelection::caret(emoji.len());
        })
        .unwrap();
    assert!(context.focus_node(document, area.stable_id()).unwrap());
    assert!(context.delete_focused_text_backward(document).unwrap());
    let state = context.world().text_input(area.stable_id()).unwrap();
    assert_eq!(state.value, "\n界");
    assert_eq!(state.selection, TextSelection::caret(0));

    assert!(
        context
            .set_ime_preedit(document, "输入".into(), None)
            .unwrap()
    );
    assert!(context.commit_ime(document, "输入").unwrap());
    let state = context.world().text_input(area.stable_id()).unwrap();
    assert_eq!(state.value, "输入\n界");
    assert_eq!(state.selection, TextSelection::caret("输入".len()));
    assert_eq!(context.world().ime(area.stable_id()), None);

    context
        .update_component(area, |area, _cx| area.disabled = true)
        .unwrap();
    assert_eq!(context.world().focused(document), None);
    assert_eq!(context.world().ime(area.stable_id()), None);
    let accessibility = context.world().accessibility(area.stable_id()).unwrap();
    assert!(accessibility.disabled);
    assert!(accessibility.invalid);
    assert!(accessibility.multiline);
}

#[test]
fn native_table_projects_hierarchy_text_and_accessibility_roles() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let table = context
        .create_component(document, Table::new().label("Builds"))
        .unwrap();
    let row = context
        .create_component(document, TableRow::new().selected(true))
        .unwrap();
    let header = context
        .create_component(document, TableCell::new("Status").column_header(true))
        .unwrap();
    let cell = context
        .create_component(document, TableCell::new("Running").selected(true))
        .unwrap();
    context.append_child(table, row).unwrap();
    context.append_child(row, header).unwrap();
    context.append_child(row, cell).unwrap();
    let focused_events = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::clone(&focused_events);
    context
        .on(table, move |_table, event: &TableCellFocused, _cx| {
            events.lock().unwrap().push(event.clone());
        })
        .unwrap();

    assert!(
        context
            .navigate_table(table, TableNavigation::NextRow, 10)
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(header.stable_id()));
    assert!(
        context
            .navigate_table(table, TableNavigation::NextColumn, 10)
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(cell.stable_id()));
    assert_eq!(
        focused_events.lock().unwrap().last().unwrap(),
        &TableCellFocused {
            row: 0,
            column: 1,
            cell: cell.stable_id(),
        }
    );

    let accessibility = context.world().project_accessibility(document);
    assert_eq!(accessibility[0].role, crate::AccessibilityRole::Table);
    assert_eq!(accessibility[0].label.as_deref(), Some("Builds"));
    assert_eq!(accessibility[1].role, crate::AccessibilityRole::Row);
    assert_eq!(accessibility[1].selected, Some(true));
    assert_eq!(
        accessibility[2].role,
        crate::AccessibilityRole::ColumnHeader
    );
    assert_eq!(accessibility[3].role, crate::AccessibilityRole::Cell);
    assert_eq!(accessibility[3].label.as_deref(), Some("Running"));
    assert_eq!(context.world().text(cell.stable_id()), Some("Running"));
}

#[test]
fn native_toggle_and_slider_state_share_events_visuals_and_accessibility() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let checkbox = context
        .create_component(
            document,
            Checkbox::new("Notifications", false).invalid(true),
        )
        .unwrap();
    let switch = context
        .create_component(document, Switch::new("Auto build", true))
        .unwrap();
    let slider = context
        .create_component(
            document,
            RangeField::new(25.0, 0.0, 100.0, 1.0).label("Volume"),
        )
        .unwrap();
    let toggles = Arc::new(Mutex::new(Vec::new()));
    let checkbox_events = Arc::clone(&toggles);
    context
        .on(checkbox, move |_checkbox, event: &ToggleChanged, _cx| {
            checkbox_events.lock().unwrap().push(event.checked);
        })
        .unwrap();
    let slider_values = Arc::new(Mutex::new(Vec::new()));
    let values = Arc::clone(&slider_values);
    context
        .on(slider, move |_slider, event: &RangeChanged, _cx| {
            values.lock().unwrap().push(event.value);
        })
        .unwrap();

    assert!(context.toggle_checkbox(checkbox).unwrap());
    assert!(context.toggle_switch(switch).unwrap());
    assert!(context.set_range_value(slider, 150.0).unwrap());
    assert!(!context.set_range_value(slider, 100.0).unwrap());
    assert_eq!(
        context.set_range_value(slider, f64::NAN),
        Err(FrameworkError::InvalidComponentValue(slider.stable_id()))
    );

    assert_eq!(*toggles.lock().unwrap(), vec![true]);
    assert_eq!(*slider_values.lock().unwrap(), vec![100.0]);
    assert_eq!(
        context.world().standard_visual(checkbox.stable_id()),
        Some(StandardVisual::Checkbox {
            checked: true,
            indeterminate: false,
            size: nana_ui_core::ControlSize::Medium,
        })
    );
    assert_eq!(
        context.world().extract_nodes(&[checkbox.stable_id()])[0]
            .source_style
            .layout
            .min_height,
        Some(nana_ui_core::LengthSpec::Px(
            nana_ui_core::ControlSize::Medium.height_in(nana_ui_core::UI_METRICS)
        ))
    );
    assert_eq!(
        context.world().standard_visual(switch.stable_id()),
        Some(StandardVisual::Switch {
            thumb_progress: 1.0,
            label: Arc::from("Auto build"),
            hint: None,
            checked: false,
            control_position: nana_ui_core::SwitchControlPosition::End,
            size: nana_ui_core::ControlSize::Medium,
            loading: false,
            loading_phase: 0.0,
            invalid: false,
        })
    );
    assert_eq!(
        context.world().standard_visual(slider.stable_id()),
        Some(StandardVisual::Range {
            label: Some(Arc::from("Volume")),
            value: Arc::from("100"),
            unit: None,
            size: nana_ui_core::ControlSize::Medium,
            ratio: 1.0,
            invalid: false,
        })
    );
    let accessibility = context.world().project_accessibility(document);
    assert_eq!(accessibility[0].role, crate::AccessibilityRole::Checkbox);
    assert_eq!(accessibility[0].checked, Some(true));
    assert!(accessibility[0].invalid);
    assert_eq!(accessibility[1].role, crate::AccessibilityRole::Switch);
    assert_eq!(accessibility[1].checked, Some(false));
    assert_eq!(accessibility[2].role, crate::AccessibilityRole::Slider);
    assert_eq!(accessibility[2].value.as_deref(), Some("100"));

    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    let checkbox_paint = context
        .world()
        .extract_nodes(&[checkbox.stable_id()])
        .pop()
        .unwrap();
    assert_eq!(
        checkbox_paint.style.background,
        Some(nana_ui_core::SemanticPalette::dark().accent.as_rgba_array())
    );
    assert_eq!(
        checkbox_paint.style.border_color,
        Some(nana_ui_core::SemanticPalette::dark().danger.as_rgba_array())
    );
    context
        .compat_world_mut()
        .set_pointer_hover(document, 1, Some(checkbox.stable_id()))
        .unwrap();
    context.advance_animations(nana_ui_core::motion::HOVER_COLOR);
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    let hovered_checked = context
        .world()
        .extract_nodes(&[checkbox.stable_id()])
        .pop()
        .unwrap();
    assert_ne!(
        hovered_checked.style.background, checkbox_paint.style.background,
        "a selected toggle must expose a distinct hover state"
    );
}

#[test]
fn an_indeterminate_checkbox_reads_mixed_and_paints_as_engaged() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mixed = context
        .create_component(
            document,
            Checkbox::new("Notifications", false)
                .indeterminate(true)
                .size(nana_ui_core::ControlSize::Large),
        )
        .unwrap();
    assert_eq!(
        context.world().standard_visual(mixed.stable_id()),
        Some(StandardVisual::Checkbox {
            checked: false,
            indeterminate: true,
            size: nana_ui_core::ControlSize::Large,
        })
    );
    assert_eq!(
        context.world().extract_nodes(&[mixed.stable_id()])[0]
            .source_style
            .layout
            .min_height,
        Some(nana_ui_core::LengthSpec::Px(
            nana_ui_core::ControlSize::Large.height_in(nana_ui_core::UI_METRICS)
        ))
    );
    let accessibility = context.world().project_accessibility(document);
    assert_eq!(accessibility[0].checked, Some(false));
    assert!(
        accessibility[0].mixed,
        "a mixed checkbox must not read as merely unchecked"
    );

    // Mixed shares the engaged surface with checked, so a parent checkbox
    // is not mistaken for an empty one.
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    let paint = context
        .world()
        .extract_nodes(&[mixed.stable_id()])
        .pop()
        .unwrap();
    assert_eq!(
        paint.style.background,
        Some(nana_ui_core::SemanticPalette::dark().accent.as_rgba_array())
    );
}

#[test]
fn a_divider_is_an_inert_hairline_with_separator_semantics() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let horizontal = context
        .create_component(document, crate::Divider::horizontal())
        .unwrap();
    let vertical = context
        .create_component(
            document,
            crate::Divider::vertical().thickness(2.0).inset(8.0),
        )
        .unwrap();

    let layout = |entity: StableNodeId| {
        context
            .world()
            .node_style(entity)
            .map(|style| Arc::clone(&style.layout))
            .unwrap()
    };
    let horizontal_layout = layout(horizontal.stable_id());
    assert_eq!(
        horizontal_layout.width,
        Some(nana_ui_core::LengthSpec::Fill)
    );
    assert_eq!(
        horizontal_layout.height,
        Some(nana_ui_core::LengthSpec::Px(1.0))
    );
    let vertical_layout = layout(vertical.stable_id());
    assert_eq!(
        vertical_layout.width,
        Some(nana_ui_core::LengthSpec::Px(2.0))
    );
    assert_eq!(vertical_layout.height, Some(nana_ui_core::LengthSpec::Fill));
    assert_eq!(
        vertical_layout.margin_top,
        Some(nana_ui_core::LengthSpec::Px(8.0))
    );

    for divider in [horizontal.stable_id(), vertical.stable_id()] {
        let interaction = context.world().interaction(divider).unwrap();
        assert!(!interaction.pointer_events);
        assert!(!interaction.focusable);
        assert_eq!(
            context.world().accessibility(divider).map(|s| s.role),
            Some(crate::AccessibilityRole::Separator)
        );
    }
    assert_eq!(
        context
            .world()
            .accessibility(vertical.stable_id())
            .and_then(|state| state.orientation),
        Some(crate::SelectionOrientation::Vertical)
    );
}

#[test]
fn a_number_input_steps_snaps_and_settles_its_draft() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(1.0)
                .range(0.0, 2.0)
                .step(0.5)
                .precision(1)
                .label("Scale"),
        )
        .unwrap();
    let values = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&values);
    context
        .on(input, move |_input, event: &crate::NumberChanged, _cx| {
            observed.lock().unwrap().push(event.value);
        })
        .unwrap();

    assert_eq!(
        context.world().text_input(input.stable_id()).unwrap().value,
        "1.0"
    );
    assert!(context.step_number_input(input, 1).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 1.5);
    assert!(context.step_number_input(input, 2).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 2.0);
    // Already at the maximum: no event, no phantom change.
    assert!(!context.step_number_input(input, 1).unwrap());

    // A draft is only adopted on commit, and it snaps to the step grid.
    context
        .update_component(input, |input, _| {
            input.state.replace_value("0.7".to_owned());
        })
        .unwrap();
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 2.0);
    assert!(context.commit_number_input(input).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 0.5);

    // Nonsense restores the committed value instead of inventing one.
    context
        .update_component(input, |input, _| {
            input.state.replace_value("banana".to_owned());
        })
        .unwrap();
    assert!(context.commit_number_input(input).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 0.5);
    assert_eq!(
        context.world().text_input(input.stable_id()).unwrap().value,
        "0.5"
    );

    assert_eq!(*values.lock().unwrap(), vec![1.5, 2.0, 0.5]);
    let accessibility = context.world().project_accessibility(document);
    assert_eq!(accessibility[0].role, crate::AccessibilityRole::TextInput);
    assert_eq!(accessibility[0].numeric_value, Some(0.5));
    assert_eq!(accessibility[0].numeric_minimum, Some(0.0));
    assert_eq!(accessibility[0].numeric_maximum, Some(2.0));
    assert_eq!(accessibility[0].numeric_step, Some(0.5));
}

#[test]
fn a_disabled_number_input_refuses_both_steppers() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(4.0).range(0.0, 10.0).disabled(true),
        )
        .unwrap();
    assert!(!context.step_number_input(input, 1).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 4.0);
    let read_only = context
        .create_component(
            document,
            crate::NumberInput::new(4.0)
                .range(0.0, 10.0)
                .read_only(true),
        )
        .unwrap();
    assert!(!context.step_number_input(read_only, 1).unwrap());
}

#[test]
fn pressing_the_spinner_steps_and_pressing_the_text_does_not() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(4.0).range(0.0, 10.0).step(1.0),
        )
        .unwrap();
    let mut mutations = MutationQueue::new();
    mutations.write_layout(
        input.stable_id(),
        crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 32.0,
        },
    );
    context.commit_mutations(mutations).unwrap();
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut crate::MeasureTextShaper)
        .unwrap();

    let steppers = match context.world().component_geometry(input.stable_id()) {
        Some(crate::ComponentGeometry::TextInput {
            steppers: Some(steppers),
            ..
        }) => steppers,
        other => panic!("expected spinner geometry, got {other:?}"),
    };
    let point = |bounds: crate::LayoutBox| {
        (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        )
    };
    let (up_x, up_y) = point(steppers.increment);
    let (down_x, down_y) = point(steppers.decrement);
    assert_eq!(
        context.number_stepper_at(input.stable_id(), up_x, up_y),
        Some(1)
    );
    assert_eq!(
        context.number_stepper_at(input.stable_id(), down_x, down_y),
        Some(-1)
    );
    // The editable text area is not a stepper, so caret placement still wins.
    assert_eq!(
        context.number_stepper_at(input.stable_id(), 8.0, 16.0),
        None
    );

    // Focused, a press on the spinner places no caret; one on the text does.
    assert!(context.focus_node(document, input.stable_id()).unwrap());
    let caret = |context: &AppContext| {
        context
            .world()
            .text_input(input.stable_id())
            .unwrap()
            .selection
    };
    let before = caret(&context);
    for (x, y) in [(up_x, up_y), (down_x, down_y)] {
        // The editor owns the press (no document selection starts there),
        // but places no caret.
        assert!(
            context
                .text_editor_pointer_press(
                    document,
                    input.stable_id(),
                    1,
                    x,
                    y,
                    false,
                    false,
                    std::time::Duration::ZERO,
                    &mut crate::MeasureTextShaper,
                )
                .unwrap()
        );
        assert_eq!(caret(&context), before);
    }
    context.text_editor_pointer_release(1);
    assert!(
        context
            .text_editor_pointer_press(
                document,
                input.stable_id(),
                1,
                0.0,
                16.0,
                false,
                false,
                std::time::Duration::from_secs(1),
                &mut crate::MeasureTextShaper,
            )
            .unwrap()
    );
    assert_eq!(caret(&context), TextSelection::caret(0));
    context.text_editor_pointer_release(1);

    // Text, spinner, text again inside the double-click window: two single
    // clicks, so the second places a caret rather than selecting a word.
    for (x, y, at) in [(0.0, 16.0, 2000), (up_x, up_y, 2100), (0.0, 16.0, 2200)] {
        context
            .text_editor_pointer_press(
                document,
                input.stable_id(),
                1,
                x,
                y,
                false,
                false,
                std::time::Duration::from_millis(at),
                &mut crate::MeasureTextShaper,
            )
            .unwrap();
        context.text_editor_pointer_release(1);
    }
    assert!(caret(&context).is_collapsed());

    assert!(
        context
            .press_number_stepper(input.stable_id(), up_x, up_y)
            .unwrap()
    );
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 5.0);
    assert!(
        context
            .press_number_stepper(input.stable_id(), down_x, down_y)
            .unwrap()
    );
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 4.0);
    assert!(
        !context
            .press_number_stepper(input.stable_id(), 8.0, 16.0)
            .unwrap()
    );
}

#[test]
fn the_spinner_follows_the_draft_and_owns_presses_on_its_inert_half() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, crate::NumberInput::new(100.0).range(0.0, 100.0))
        .unwrap();
    let node = input.stable_id();
    let mut mutations = MutationQueue::new();
    mutations.write_layout(
        node,
        crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 32.0,
        },
    );
    context.commit_mutations(mutations).unwrap();
    assert!(context.focus_node(document, node).unwrap());
    let settle = |context: &mut AppContext| {
        let work = context.compat_world_mut().take_system_work();
        context
            .compat_world_mut()
            .resolve_styles(&work.style)
            .unwrap();
        context
            .compat_world_mut()
            .shape_text(&work.text, &mut crate::MeasureTextShaper)
            .unwrap();
        match context.world().component_geometry(node) {
            Some(crate::ComponentGeometry::TextInput {
                steppers: Some(steppers),
                ..
            }) => steppers,
            other => panic!("expected spinner geometry, got {other:?}"),
        }
    };
    let steppers = settle(&mut context);
    let (up_x, up_y) = (
        steppers.increment.x + steppers.increment.width / 2.0,
        steppers.increment.y + steppers.increment.height / 2.0,
    );
    assert!(!steppers.increment_enabled, "at the maximum");
    assert_eq!(context.number_stepper_at(node, up_x, up_y), None);

    // A press on the inert half is still the spinner's: no caret, no drag.
    let caret = |context: &AppContext| context.world().text_input(node).unwrap().selection;
    context.select_focused_text_range(document, 0, 0).unwrap();
    assert!(
        context
            .text_editor_pointer_press(
                document,
                node,
                1,
                up_x,
                up_y,
                false,
                false,
                std::time::Duration::ZERO,
                &mut crate::MeasureTextShaper,
            )
            .unwrap()
    );
    assert_eq!(caret(&context), TextSelection::caret(0));
    context.text_editor_pointer_release(1);

    // A typed draft moves the spinner with it: the pointer, the keyboard and
    // the drawn state all step from 50, not from the committed 100.
    context.select_all_focused_text(document).unwrap();
    context.replace_focused_text(document, "50").unwrap();
    let steppers = settle(&mut context);
    assert!(steppers.increment_enabled);
    assert_eq!(context.number_stepper_at(node, up_x, up_y), Some(1));
    assert_eq!(
        context.world().accessibility(node).unwrap().numeric_value,
        Some(50.0)
    );
    assert!(context.press_number_stepper(node, up_x, up_y).unwrap());
    assert_eq!(
        context.read(input, crate::NumberInput::value).unwrap(),
        51.0
    );
}

#[test]
fn a_continuous_draft_publishes_its_clamped_step_base() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::continuous(3.0).range(0.0, 100.0),
        )
        .unwrap();
    context.focus_node(document, input.stable_id()).unwrap();
    context.select_all_focused_text(document).unwrap();
    context.replace_focused_text(document, "500").unwrap();
    // The spinner and the accessible value read what a step would start
    // from: the draft clamped as a commit would clamp it.
    assert_eq!(
        context
            .world()
            .accessibility(input.stable_id())
            .unwrap()
            .numeric_value,
        Some(100.0)
    );
    assert!(!context.step_number_input(input, 1).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 3.0);
    assert!(context.step_number_input(input, -1).unwrap());
    assert_eq!(
        context.read(input, crate::NumberInput::value).unwrap(),
        99.0
    );
}

#[test]
fn a_step_that_snaps_back_onto_its_base_is_no_step_and_the_spinner_agrees() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(3.0).range(0.0, 10.0).step(3.0),
        )
        .unwrap();
    context.focus_node(document, input.stable_id()).unwrap();
    context.select_all_focused_text(document).unwrap();
    context.replace_focused_text(document, "9").unwrap();
    // 9 + 3 clamps to 10, which snaps back to 9: nothing moves or commits.
    assert!(!context.step_number_input(input, 1).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 3.0);
    // The published maximum is the reachable one, so the up half is inert.
    let state = context.world().accessibility(input.stable_id()).unwrap();
    assert_eq!(state.numeric_maximum, Some(9.0));
    assert_eq!(state.numeric_value, Some(9.0));
}

#[test]
fn an_infinite_bound_publishes_no_bound() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(5.0).range(f64::NEG_INFINITY, f64::INFINITY),
        )
        .unwrap();
    let state = context.world().accessibility(input.stable_id()).unwrap();
    assert_eq!((state.numeric_minimum, state.numeric_maximum), (None, None));
    assert!(context.step_number_input(input, 1).unwrap());
    assert!(context.step_number_input(input, -3).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 3.0);
}

#[test]
fn find_and_replace_cannot_put_a_control_character_in_a_number_draft() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, crate::NumberInput::new(15.0))
        .unwrap();
    context.focus_node(document, input.stable_id()).unwrap();
    context.select_all_focused_text(document).unwrap();
    assert!(
        !context
            .replace_focused_text_match(
                document,
                "15",
                crate::TextSearchOptions::default(),
                "15\n",
                false,
            )
            .unwrap()
    );
    assert_eq!(
        context
            .read(input, |input| input.state.value.to_string())
            .unwrap(),
        "15"
    );
}

#[test]
fn a_read_only_number_input_draws_an_inert_spinner() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(5.0)
                .range(0.0, 10.0)
                .read_only(true),
        )
        .unwrap();
    let mut mutations = MutationQueue::new();
    mutations.write_layout(
        input.stable_id(),
        crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 160.0,
            height: 32.0,
        },
    );
    context.commit_mutations(mutations).unwrap();
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut crate::MeasureTextShaper)
        .unwrap();
    let Some(crate::ComponentGeometry::TextInput {
        steppers: Some(steppers),
        ..
    }) = context.world().component_geometry(input.stable_id())
    else {
        panic!("expected spinner geometry");
    };
    assert!(!steppers.increment_enabled && !steppers.decrement_enabled);
    let (x, y) = (
        steppers.increment.x + steppers.increment.width / 2.0,
        steppers.increment.y + steppers.increment.height / 2.0,
    );
    assert_eq!(context.number_stepper_at(input.stable_id(), x, y), None);

    // Flipping read-only alone changes no text, yet the spinner repaints.
    context
        .update_component(input, |input, _| input.read_only = false)
        .unwrap();
    let work = context.compat_world_mut().take_system_work();
    assert!(work.render_extraction.contains(&input.stable_id()));
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    context
        .compat_world_mut()
        .shape_text(&work.text, &mut crate::MeasureTextShaper)
        .unwrap();
    assert_eq!(context.number_stepper_at(input.stable_id(), x, y), Some(1));
}

#[test]
fn accessibility_set_value_reaches_the_maximum_through_float_noise() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(0.2)
                .range(0.0, 0.3)
                .step(0.1)
                .precision(1),
        )
        .unwrap();
    // What a client computes as value + step, and an f32 bridge's 0.3.
    for request in [format!("{}", 0.2 + 0.1), format!("{}", f64::from(0.3_f32))] {
        assert!(
            context
                .apply_accessibility_action(
                    document,
                    AccessibilityActionRequest {
                        target: input.stable_id(),
                        action: AccessibilityAction::SetValue(request.clone()),
                    },
                )
                .unwrap(),
            "{request}"
        );
        assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 0.3);
    }
}

#[test]
fn a_hover_card_keeps_a_focused_number_input_editing() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let card = context
        .create_component(
            document,
            crate::HoverCard::new()
                .trigger("account")
                .preserve_editor_focus(true),
        )
        .unwrap();
    let action = context
        .create_component(document, crate::Button::new("refresh"))
        .unwrap();
    context.append_child(card, action).unwrap();
    let input = context
        .create_component(document, crate::NumberInput::new(1.0))
        .unwrap();
    context.focus_node(document, input.stable_id()).unwrap();
    assert!(context.preserves_hover_card_editor_focus(action.stable_id()));
}

#[test]
fn moving_focus_away_settles_a_pending_numeric_draft() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(1.0).range(0.0, 9.0).step(1.0),
        )
        .unwrap();
    let elsewhere = context
        .create_component(document, Button::new("Done"))
        .unwrap();
    assert!(context.focus_node(document, input.stable_id()).unwrap());
    context
        .update_component(input, |input, _| {
            input.state.replace_value("7".to_owned());
        })
        .unwrap();
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 1.0);

    assert!(context.focus_node(document, elsewhere.stable_id()).unwrap());
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 7.0);
}

#[test]
fn range_accessibility_set_value_uses_quantized_typed_action() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let range = context
        .create_component(
            document,
            RangeField::new(0.25, 0.0, 1.0, 0.25)
                .label("Opacity")
                .unit("%"),
        )
        .unwrap();
    let values = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&values);
    context
        .on(range, move |_range, event: &RangeChanged, _cx| {
            observed.lock().unwrap().push(event.value);
        })
        .unwrap();

    assert!(
        context
            .apply_accessibility_action(
                document,
                AccessibilityActionRequest {
                    target: range.stable_id(),
                    action: AccessibilityAction::SetValue("0.62".into()),
                },
            )
            .unwrap()
    );
    assert_eq!(*values.lock().unwrap(), vec![0.5]);
    let node = context.world().project_accessibility(document).remove(0);
    assert_eq!(node.numeric_minimum, Some(0.0));
    assert_eq!(node.numeric_maximum, Some(1.0));
    assert_eq!(node.numeric_step, Some(0.25));
    assert_eq!(node.numeric_value, Some(0.5));
}

#[test]
fn accessibility_actions_edit_a_number_input_through_its_numeric_policy() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(
            document,
            crate::NumberInput::new(1.0)
                .range(0.0, 10.0)
                .step(0.5)
                .precision(1),
        )
        .unwrap();
    let node = input.stable_id();
    let values = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&values);
    context
        .on(input, move |_input, event: &crate::NumberChanged, _cx| {
            observed.lock().unwrap().push(event.value);
        })
        .unwrap();
    let act = |context: &mut AppContext, action| {
        context
            .apply_accessibility_action(
                document,
                AccessibilityActionRequest {
                    target: node,
                    action,
                },
            )
            .unwrap()
    };
    let draft = |context: &AppContext| {
        context
            .read(input, |input| input.state.value.to_string())
            .unwrap()
    };

    // Click focuses the field so an IME / TalkBack session can attach.
    assert!(act(&mut context, AccessibilityAction::Click));
    assert_eq!(context.world().focused(document), Some(node));

    // SetValue commits a number within bounds, snapped to the step grid.
    assert!(act(
        &mut context,
        AccessibilityAction::SetValue("7.3".into())
    ));
    assert_eq!(context.read(input, crate::NumberInput::value).unwrap(), 7.5);
    assert_eq!(draft(&context), "7.5");
    assert!(act(
        &mut context,
        AccessibilityAction::SetValue("10".into())
    ));
    assert_eq!(*values.lock().unwrap(), vec![7.5, 10.0]);

    // A number the field already holds is taken: success, nothing emitted.
    assert!(act(
        &mut context,
        AccessibilityAction::SetValue("10".into())
    ));
    assert_eq!(*values.lock().unwrap(), vec![7.5, 10.0]);

    // Out of bounds is refused and changes nothing, a pending draft included.
    context
        .update_component(input, |input, _| input.state.replace_value("7"))
        .unwrap();
    for request in ["150", "-1", "abc"] {
        assert!(
            !act(&mut context, AccessibilityAction::SetValue(request.into())),
            "{request}"
        );
        assert_eq!(draft(&context), "7", "{request}");
    }
    assert_eq!(
        context.read(input, crate::NumberInput::value).unwrap(),
        10.0
    );
    assert_eq!(*values.lock().unwrap(), vec![7.5, 10.0]);
    context
        .update_component(input, |input, _| input.state.replace_value("10.0"))
        .unwrap();

    // SetSelection selects inside the draft; out-of-range selections refuse.
    assert!(act(
        &mut context,
        AccessibilityAction::SetSelection(TextSelection::new(0, 2))
    ));
    let selection = context.world().text_input(node).unwrap().selection;
    assert_eq!((selection.anchor, selection.focus), (0, 2));
    assert!(!act(
        &mut context,
        AccessibilityAction::SetSelection(TextSelection::new(0, 99))
    ));

    // A read-only field refuses SetValue but still selects, so its text can
    // be copied.
    context
        .update_component(input, |input, _| input.read_only = true)
        .unwrap();
    assert!(!act(
        &mut context,
        AccessibilityAction::SetValue("3".into())
    ));
    assert_eq!(
        context.read(input, crate::NumberInput::value).unwrap(),
        10.0
    );
    assert!(act(
        &mut context,
        AccessibilityAction::SetSelection(TextSelection::new(0, 4))
    ));
}

#[test]
fn failed_component_projection_keeps_typed_state_and_world_unchanged() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let slider = context
        .create_component(document, RangeField::new(25.0, 0.0, 100.0, 1.0))
        .unwrap();
    let generation = context.world().generation();
    let visual = context.world().standard_visual(slider.stable_id());

    assert!(
        context
            .update_component(slider, |slider, _cx| slider.value = f64::NAN)
            .is_err()
    );
    assert_eq!(context.read(slider, |slider| slider.value).unwrap(), 25.0);
    assert_eq!(context.world().generation(), generation);
    assert_eq!(context.world().standard_visual(slider.stable_id()), visual);
}

#[test]
fn overlay_host_switches_exclusive_visibility_and_restores_focus() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let base = context
        .create_component(document, Button::new("Open"))
        .unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let dialog = context
        .create_component(
            document,
            Dialog::new("Settings").close_policy(nana_ui_core::DialogClosePolicy {
                close_on_outside: false,
                ..nana_ui_core::DialogClosePolicy::default()
            }),
        )
        .unwrap();
    let menu = context
        .create_component(document, crate::ActionMenu::new().open(true))
        .unwrap();
    let menu_item = context
        .create_component(document, crate::ActionMenuItem::new("Build"))
        .unwrap();
    context.append_child(host, dialog).unwrap();
    context.append_child(host, menu).unwrap();
    context.append_child(menu, menu_item).unwrap();
    let mut focus = MutationQueue::new();
    focus.request_focus(document, Some(base.stable_id()));
    context.compat_world_mut().commit(focus).unwrap();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&changes);
    context
        .on(host, move |_host, event: &OverlayChanged, _cx| {
            observed.lock().unwrap().push(event.active);
        })
        .unwrap();
    let activations = Arc::new(Mutex::new(0));
    let observed_activations = Arc::clone(&activations);
    context
        .on(menu_item, move |_item, _event: &Activate, _cx| {
            *observed_activations.lock().unwrap() += 1;
        })
        .unwrap();

    let initial_work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&initial_work.style)
        .unwrap();
    let initial = context.world().extract_document(document);
    assert!(!initial.iter().any(|node| node.id == dialog.stable_id()));
    assert!(!initial.iter().any(|node| node.id == menu.stable_id()));

    assert!(context.activate_overlay(host, dialog).unwrap());
    let dialog_work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&dialog_work.style)
        .unwrap();
    assert_eq!(context.world().focused(document), Some(dialog.stable_id()));
    let generation = context.world().generation();
    assert_eq!(
        context.append_child(menu, dialog),
        Err(FrameworkError::World(
            crate::UiWorldError::InvalidOverlayHost(host.stable_id())
        ))
    );
    assert_eq!(context.world().generation(), generation);
    assert_eq!(
        context.world().node(dialog.stable_id()).unwrap().parent,
        Some(host.stable_id())
    );
    assert!(
        context
            .world()
            .project_accessibility(document)
            .iter()
            .any(|node| node.id == dialog.stable_id() && node.modal)
    );
    let mut escape_modal = MutationQueue::new();
    escape_modal.request_focus(document, Some(base.stable_id()));
    assert_eq!(
        context.compat_world_mut().commit(escape_modal),
        Err(crate::UiWorldError::NotFocusable(base.stable_id()))
    );
    assert_eq!(context.world().focused(document), Some(dialog.stable_id()));
    assert!(
        !context
            .dismiss_dialog(host, nana_ui_core::DialogCloseTrigger::Outside)
            .unwrap()
    );
    let mut capture = MutationQueue::new();
    capture.capture_pointer(7, dialog.stable_id());
    context.compat_world_mut().commit(capture).unwrap();
    assert!(context.activate_overlay(host, menu).unwrap());
    let menu_work = context.compat_world_mut().take_system_work();
    assert!(menu_work.accessibility.contains(&host.stable_id()));
    context
        .compat_world_mut()
        .resolve_styles(&menu_work.style)
        .unwrap();
    let visible = context.world().extract_document(document);
    assert!(!visible.iter().any(|node| node.id == dialog.stable_id()));
    assert!(visible.iter().any(|node| node.id == menu.stable_id()));
    assert_eq!(
        context.world().focused(document),
        Some(menu_item.stable_id())
    );
    assert!(context.activate_action_menu_item(menu_item).unwrap());
    assert_eq!(*activations.lock().unwrap(), 1);
    assert_eq!(context.world().pointer_capture(document, 7), None);
    assert!(
        context
            .compat_world_mut()
            .take_pointer_capture_changes()
            .iter()
            .any(|change| change.pointer_id == 7 && !change.captured)
    );

    assert!(!context.dismiss_overlay(host).unwrap());
    assert!(context.active_runtime_overlay(document).is_none());
    context.advance_animations(nana_ui_core::motion::MENU_POP);
    let dismissed_work = context.compat_world_mut().take_system_work();
    assert!(dismissed_work.accessibility.contains(&host.stable_id()));
    context
        .compat_world_mut()
        .resolve_styles(&dismissed_work.style)
        .unwrap();
    assert_eq!(context.world().focused(document), Some(base.stable_id()));
    assert_eq!(
        context
            .world()
            .overlay_host(host.stable_id())
            .unwrap()
            .active,
        None
    );
    assert_eq!(
        changes.lock().unwrap().as_slice(),
        [Some(dialog.stable_id()), Some(menu.stable_id()), None]
    );
}

#[test]
fn destroying_the_active_overlay_clears_authority_and_restores_focus() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let base = context
        .create_component(document, Button::new("Open"))
        .unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let dialog = context
        .create_component(document, Dialog::new("Temporary"))
        .unwrap();
    context.append_child(host, dialog).unwrap();
    let mut focus = MutationQueue::new();
    focus.request_focus(document, Some(base.stable_id()));
    context.compat_world_mut().commit(focus).unwrap();
    context.activate_overlay(host, dialog).unwrap();

    context.remove_view(dialog).unwrap();

    assert_eq!(context.world().focused(document), Some(base.stable_id()));
    assert_eq!(
        context.world().overlay_host(host.stable_id()),
        Some(crate::OverlayHostState::default())
    );
    assert!(!context.dismiss_overlay(host).unwrap());
}

#[test]
fn segmented_options_reconcile_atomically_and_roving_selection_skips_disabled() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let control = context
        .create_component(document, SegmentedControl::new().label("Preview mode"))
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let disabled = context
        .create_detached_component(document, SegmentedOption::new("Split").disabled(true))
        .unwrap();
    let last = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    assert!(
        context
            .set_segmented_options(control, vec![first, disabled, last], Some(first))
            .unwrap()
    );

    let observed = Arc::new(Mutex::new(Vec::new()));
    let selected = Arc::clone(&observed);
    context
        .on(
            control,
            move |_control, event: &SegmentedSelectionRequested, _cx| {
                selected.lock().unwrap().push(event.option);
            },
        )
        .unwrap();
    context.focus_node(document, first.stable_id()).unwrap();
    assert!(
        context
            .navigate_focused_segmented(document, RovingFocusIntent::Next)
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(last.stable_id()));
    assert_eq!(
        context.read(control, |control| control.selected).unwrap(),
        Some(first.stable_id())
    );
    assert!(context.read(first, |option| option.selected).unwrap());
    assert!(!context.read(last, |option| option.selected).unwrap());
    assert_eq!(&*observed.lock().unwrap(), &[last.stable_id()]);
    // Activation is self-driving: the control commits the selection itself.
    assert!(context.activate_node(last.stable_id()).unwrap());
    assert_eq!(
        &*observed.lock().unwrap(),
        &[last.stable_id(), last.stable_id()]
    );
    assert_eq!(
        context.read(control, |control| control.selected).unwrap(),
        Some(last.stable_id())
    );
    assert!(!context.read(first, |option| option.selected).unwrap());
    assert!(context.read(last, |option| option.selected).unwrap());
    assert!(!context.activate_node(disabled.stable_id()).unwrap());
    // Re-publishing the selection the control already holds is a no-op, so an
    // application that echoes the event back costs nothing.
    let generation = context.world().generation();
    assert!(
        !context
            .set_segmented_selection(control, Some(last))
            .unwrap()
    );
    assert_eq!(context.world().generation(), generation);
    assert!(
        context
            .apply_accessibility_action(
                document,
                AccessibilityActionRequest {
                    target: last.stable_id(),
                    action: AccessibilityAction::Click,
                },
            )
            .unwrap()
    );
    assert!(context.read(last, |option| option.selected).unwrap());
    assert!(
        context
            .navigate_focused_segmented(document, RovingFocusIntent::Next)
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(first.stable_id()));
    assert_eq!(
        context.read(control, |control| control.selected).unwrap(),
        Some(last.stable_id())
    );

    let generation = context.world().generation();
    assert_eq!(
        context.set_segmented_options(control, vec![first, first], Some(first)),
        Err(FrameworkError::InvalidComponentValue(control.stable_id()))
    );
    assert_eq!(context.world().generation(), generation);

    assert!(
        context
            .set_segmented_options(control, vec![first, last], Some(first))
            .unwrap()
    );
    assert_eq!(
        context.world().mount_state(disabled.stable_id()),
        Some(crate::MountState::Parked)
    );
    assert!(
        !context
            .world()
            .project_accessibility(document)
            .iter()
            .any(|node| node.id == disabled.stable_id())
    );
    let accessibility = context.world().project_accessibility(document);
    assert_eq!(accessibility[0].role, crate::AccessibilityRole::RadioGroup);
    assert_eq!(accessibility[1].role, crate::AccessibilityRole::Radio);
    assert_eq!(accessibility[1].checked, Some(true));
    assert_eq!(accessibility[2].checked, Some(false));
    assert_eq!(context.next_animation_deadline(), None);
}

#[test]
fn updating_a_filled_tab_option_keeps_the_control_surface() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let control = context
        .create_component(
            document,
            Tabs::new("code")
                .size(nana_ui_core::ControlSize::Small)
                .fill(true)
                .options([
                    TabOption::new("code", "Code"),
                    TabOption::new("preview", "Preview"),
                ]),
        )
        .unwrap();
    let first = Entity::<SegmentedOption>::from_stable_id(
        context
            .read(control, |tabs| tabs.option_nodes()[0].1)
            .unwrap(),
    );
    context
        .update_component(first, |option, _| {
            *option = SegmentedOption::new("Code")
                .size(nana_ui_core::ControlSize::Small)
                .with_selected(true);
        })
        .unwrap();
    context
        .update_component(control, |tabs, _| {
            tabs.fill = true;
        })
        .unwrap();
    assert!(context.read(first, |option| option.fill).unwrap());
    assert_eq!(
        context
            .read(first, |option| option.style.layout.width)
            .unwrap(),
        Some(LengthSpec::Fill)
    );
    assert_eq!(
        context.read(first, |option| option.node_kind()).unwrap(),
        NodeKind::Element { tag: "tab".into() }
    );
}

#[test]
fn segmented_size_disabled_and_sequential_focus_share_one_authority() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let before = context
        .create_component(document, Button::new("Before"))
        .unwrap();
    let control = context
        .create_component(document, SegmentedControl::new())
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let second = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    let after = context
        .create_component(document, Button::new("After"))
        .unwrap();
    context
        .set_segmented_options(control, vec![first, second], Some(first))
        .unwrap();

    let generation = context.world().generation();
    assert!(
        context
            .set_segmented_size(control, nana_ui_core::ControlSize::Large)
            .unwrap()
    );
    assert_eq!(context.world().generation(), generation + 1);
    assert_eq!(
        context.read(control, |control| control.size).unwrap(),
        nana_ui_core::ControlSize::Large
    );
    assert_eq!(
        context.read(first, |option| option.size).unwrap(),
        nana_ui_core::ControlSize::Large
    );
    assert_eq!(
        context.read(second, |option| option.size).unwrap(),
        nana_ui_core::ControlSize::Large
    );
    // The track is a control and rounds like the buttons beside it, not like
    // the card it sits on.
    let radius = context.world().theme_metrics().radius_sm;
    assert_eq!(
        context.world().extract_nodes(&[control.stable_id()])[0]
            .source_style
            .layout
            .border_radius,
        Some(radius)
    );
    assert_eq!(
        context.world().extract_nodes(&[before.stable_id()])[0]
            .source_style
            .layout
            .border_radius,
        Some(radius)
    );
    // The pill's corners are concentric with the track's, so its radius is the
    // track's minus how far the pill sits inside it: the track's padding
    // *and* its 1px rule. Spelling both terms is the point — this assertion
    // used to read `radius - space::XXS`, which dropped the rule and left the
    // pill a pixel rounder than the corner it sits in.
    let pill_inset = nana_ui_core::space::XXS + nana_ui_core::HAIRLINE;
    assert_eq!(
        context
            .world()
            .node_style(first.stable_id())
            .unwrap()
            .layout
            .border_radius,
        Some((radius - pill_inset).max(0.0))
    );

    context.focus_node(document, before.stable_id()).unwrap();
    assert!(context.navigate_sequential_focus(document, false).unwrap());
    assert_eq!(context.world().focused(document), Some(first.stable_id()));
    assert!(context.navigate_sequential_focus(document, false).unwrap());
    assert_eq!(context.world().focused(document), Some(after.stable_id()));
    assert!(
        context
            .apply_accessibility_action(
                document,
                AccessibilityActionRequest {
                    target: second.stable_id(),
                    action: AccessibilityAction::Focus,
                },
            )
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(second.stable_id()));
    assert_eq!(
        context
            .read(control, |control| control.focus_target)
            .unwrap(),
        Some(second.stable_id())
    );
    assert!(
        context
            .set_segmented_selection(control, Some(second))
            .unwrap()
    );
    assert!(
        context
            .set_segmented_selection(control, Some(first))
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(second.stable_id()));
    assert_eq!(
        context
            .read(control, |control| control.focus_target)
            .unwrap(),
        Some(second.stable_id())
    );
    assert!(context.navigate_sequential_focus(document, false).unwrap());
    assert_eq!(context.world().focused(document), Some(after.stable_id()));

    context.focus_node(document, first.stable_id()).unwrap();
    assert!(
        context
            .set_segmented_option_disabled(control, first, true)
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(second.stable_id()));
    assert_eq!(
        context
            .read(control, |control| control.focus_target)
            .unwrap(),
        Some(second.stable_id())
    );
    assert_eq!(
        context.read(control, |control| control.selected).unwrap(),
        Some(first.stable_id())
    );
    assert!(context.read(first, |option| option.selected).unwrap());
    assert!(context.read(first, |option| option.disabled).unwrap());
    assert_eq!(
        context
            .world()
            .project_accessibility(document)
            .into_iter()
            .find(|node| node.id == first.stable_id())
            .unwrap()
            .checked,
        Some(true)
    );
    assert!(
        context
            .set_segmented_option_disabled(control, first, false)
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(second.stable_id()));
    assert_eq!(
        context
            .read(control, |control| control.focus_target)
            .unwrap(),
        Some(second.stable_id())
    );
}

#[test]
fn segmented_intrinsic_width_is_stable_across_viewports_sizes_icons_and_empty_groups() {
    struct FixedShaper;
    impl crate::TextShaper for FixedShaper {
        fn shape(
            &mut self,
            _id: StableNodeId,
            text: &TextContent,
            _style: &crate::ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> crate::TextMetrics {
            crate::TextMetrics {
                width: text.value.chars().count() as f32 * 7.0,
                height: 16.0,
                ascent: None,
            }
        }
    }

    for (index, size) in [
        nana_ui_core::ControlSize::Small,
        nana_ui_core::ControlSize::Medium,
        nana_ui_core::ControlSize::Large,
    ]
    .into_iter()
    .enumerate()
    {
        let mut context = AppContext::new();
        let document = DocumentId::new(index as u64 + 1).unwrap();
        let control = context
            .create_component(document, SegmentedControl::new().size(size))
            .unwrap();
        let icon = context
            .create_detached_component(
                document,
                SegmentedOption::new("Code").icon(nana_ui_core::Icon::File),
            )
            .unwrap();
        let plain = context
            .create_detached_component(document, SegmentedOption::new("Code"))
            .unwrap();
        context
            .set_segmented_options(control, vec![icon, plain], Some(icon))
            .unwrap();
        let mut shaper = FixedShaper;
        while context
            .shape_text_for_layout(document, &mut shaper)
            .unwrap()
        {}
        context
            .layout_document(document, crate::LayoutViewport::new(320.0, 100.0))
            .unwrap();
        let narrow = context.world().layout_box(control.stable_id()).unwrap();
        let icon_bounds = context.world().layout_box(icon.stable_id()).unwrap();
        let plain_bounds = context.world().layout_box(plain.stable_id()).unwrap();
        assert_eq!(narrow.height, size.height_in(nana_ui_core::UI_METRICS));
        assert!(icon_bounds.width > plain_bounds.width);
        assert!(
            (narrow.width - (icon_bounds.width + plain_bounds.width + nana_ui_core::space::MD))
                .abs()
                < 0.01
        );

        context
            .layout_document(document, crate::LayoutViewport::new(640.0, 100.0))
            .unwrap();
        assert_eq!(
            context.world().layout_box(control.stable_id()),
            Some(narrow)
        );
    }

    let mut context = AppContext::new();
    let document = DocumentId::new(9).unwrap();
    let empty = context
        .create_component(document, SegmentedControl::new())
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(640.0, 100.0))
        .unwrap();
    let empty = context.world().layout_box(empty.stable_id()).unwrap();
    assert_eq!(empty.width, 6.0);
    assert_eq!(
        empty.height,
        nana_ui_core::ControlSize::Medium.height_in(nana_ui_core::UI_METRICS)
    );
}

#[test]
fn segmented_request_focus_and_event_roll_back_together_when_focus_is_blocked() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let control = context
        .create_component(document, SegmentedControl::new())
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let second = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    context
        .set_segmented_options(control, vec![first, second], Some(first))
        .unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let dialog = context
        .create_component(document, Dialog::new("Settings"))
        .unwrap();
    context.append_child(host, dialog).unwrap();
    context.activate_overlay(host, dialog).unwrap();
    let generation = context.world().generation();

    assert!(matches!(
        context.request_segmented_selection(control, second),
        Err(FrameworkError::World(crate::UiWorldError::NotFocusable(id)))
            if id == second.stable_id()
    ));
    assert_eq!(context.world().generation(), generation);
    assert!(context.read(first, |option| option.selected).unwrap());
    assert!(!context.read(second, |option| option.selected).unwrap());
    assert_eq!(
        context
            .read(control, |control| control.focus_target)
            .unwrap(),
        Some(first.stable_id())
    );
    assert_eq!(
        context.read(control, |control| control.selected).unwrap(),
        Some(first.stable_id())
    );
}

#[test]
fn segmented_request_rolls_back_focus_when_an_event_handler_mutation_is_invalid() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let foreign_document = DocumentId::new(2).unwrap();
    let control = context
        .create_component(document, SegmentedControl::new())
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let second = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    let foreign = context
        .create_component(foreign_document, Button::new("Foreign"))
        .unwrap();
    context
        .set_segmented_options(control, vec![first, second], Some(first))
        .unwrap();
    context.focus_node(document, first.stable_id()).unwrap();
    context
        .on(
            control,
            move |_control, _event: &SegmentedSelectionRequested, cx| {
                cx.mutations()
                    .insert(foreign.stable_id(), second.stable_id(), None);
            },
        )
        .unwrap();
    let generation = context.world().generation();

    assert!(
        context
            .request_segmented_selection(control, second)
            .is_err()
    );
    assert_eq!(context.world().generation(), generation);
    assert_eq!(context.world().focused(document), Some(first.stable_id()));
    assert_eq!(
        context
            .read(control, SegmentedControl::focus_target)
            .unwrap(),
        Some(first.stable_id())
    );
    assert_eq!(
        context.read(control, SegmentedControl::selected).unwrap(),
        Some(first.stable_id())
    );
    assert!(context.read(first, SegmentedOption::selected).unwrap());
    assert!(!context.read(second, SegmentedOption::selected).unwrap());
}

#[test]
fn overlay_tab_trap_reuses_segmented_sequential_focus_authority() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let dialog = context
        .create_component(
            document,
            Dialog::new("Settings").initial_focus(crate::ModalInitialFocus::Surface),
        )
        .unwrap();
    let control = context
        .create_detached_component(document, SegmentedControl::new())
        .unwrap();
    let first = context
        .create_detached_component(document, SegmentedOption::new("Code"))
        .unwrap();
    let second = context
        .create_detached_component(document, SegmentedOption::new("Preview"))
        .unwrap();
    let action = context
        .create_detached_component(document, Button::new("Save"))
        .unwrap();
    context
        .set_segmented_options(control, vec![first, second], Some(first))
        .unwrap();
    context.append_child(host, dialog).unwrap();
    context
        .set_modal_slots(
            dialog,
            ModalSlots {
                body: Some(control.stable_id()),
                actions: vec![action.stable_id()],
                ..Default::default()
            },
        )
        .unwrap();
    context.activate_overlay(host, dialog).unwrap();
    assert_eq!(context.world().focused(document), Some(dialog.stable_id()));
    assert!(
        context
            .route_overlay_key(document, OverlayKey::Tab { reverse: false })
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(first.stable_id()));
    assert!(
        context
            .route_overlay_key(document, OverlayKey::Tab { reverse: false })
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(action.stable_id()));
    assert!(
        context
            .route_overlay_key(document, OverlayKey::Tab { reverse: false })
            .unwrap()
    );
    assert_eq!(context.world().focused(document), Some(first.stable_id()));
    assert!(context.focus_node(document, second.stable_id()).unwrap());
}

#[test]
fn native_scroll_view_projects_axes_and_typed_runtime_offset() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = context
        .create_component(
            document,
            ScrollView::new(ScrollAxes::Vertical).label("Builds"),
        )
        .unwrap();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&changes);
    context
        .on(scroll, move |_scroll, event: &ScrollChanged, _cx| {
            observed.lock().unwrap().push(event.offset);
        })
        .unwrap();
    context.compat_world_mut().take_system_work();

    assert!(
        context
            .scroll_to(scroll, ScrollOffset { x: 40.0, y: 120.0 })
            .unwrap()
    );
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()),
        Some(ScrollOffset { x: 0.0, y: 120.0 })
    );
    assert_eq!(
        *changes.lock().unwrap(),
        vec![ScrollOffset { x: 0.0, y: 120.0 }]
    );
    assert_eq!(
        context
            .world()
            .node_style(scroll.stable_id())
            .unwrap()
            .layout
            .overflow_y,
        nana_ui_core::OverflowSpec::Scroll
    );
    let work = context.compat_world_mut().take_system_work();
    assert_eq!(work.input_hit_test, vec![scroll.stable_id()]);
    assert_eq!(work.render_extraction, vec![scroll.stable_id()]);
    assert!(work.layout.is_empty());
    assert!(
        context
            .set_scroll_metrics(
                scroll,
                ScrollMetrics {
                    viewport_width: 100.0,
                    viewport_height: 100.0,
                    content_width: 100.0,
                    content_height: 250.0,
                    origin_x: 0.0,
                    origin_y: 0.0,
                },
            )
            .unwrap()
    );
    assert!(
        context
            .scroll_by(scroll, ScrollOffset { x: 0.0, y: 80.0 })
            .unwrap()
    );
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()).unwrap().y,
        150.0
    );
    assert!(
        context
            .set_scroll_metrics(
                scroll,
                ScrollMetrics {
                    viewport_width: 100.0,
                    viewport_height: 100.0,
                    content_width: 100.0,
                    content_height: 130.0,
                    origin_x: 0.0,
                    origin_y: 0.0,
                },
            )
            .unwrap()
    );
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()).unwrap().y,
        30.0
    );
    assert_eq!(
        changes.lock().unwrap().as_slice(),
        [
            ScrollOffset { x: 0.0, y: 120.0 },
            ScrollOffset { x: 0.0, y: 150.0 },
            ScrollOffset { x: 0.0, y: 30.0 },
        ]
    );
    assert!(
        !context
            .scroll_to(scroll, ScrollOffset { x: 0.0, y: 30.0 })
            .unwrap()
    );
    assert_eq!(
        context.scroll_to(
            scroll,
            ScrollOffset {
                x: 0.0,
                y: f32::NAN
            }
        ),
        Err(FrameworkError::InvalidComponentValue(scroll.stable_id()))
    );
}

#[test]
fn layout_publishes_scroll_metrics_and_clamps_wheel_offset() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut viewport = NodeStyle::default();
    {
        let layout = std::sync::Arc::make_mut(&mut viewport.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(120.0));
    }
    let scroll = context
        .create_component(
            document,
            ScrollView::new(ScrollAxes::Vertical).style(viewport),
        )
        .unwrap();
    for index in 0..5 {
        let mut row = NodeStyle::default();
        {
            let layout = std::sync::Arc::make_mut(&mut row.layout);
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Px(40.0));
        }
        let row = context
            .create_component(document, Text::new(format!("Row {index}")).style(row))
            .unwrap();
        context.append_child(scroll, row).unwrap();
    }
    context
        .layout_document(document, crate::LayoutViewport::new(200.0, 120.0))
        .unwrap();
    let metrics = context
        .world()
        .scroll_metrics(scroll.stable_id())
        .expect("layout publishes scroll metrics");
    assert!(
        metrics.content_height > metrics.viewport_height,
        "content {metrics:?} should overflow the viewport"
    );
    let max_y = (metrics.content_height - metrics.viewport_height).max(0.0);
    assert!(
        context
            .scroll_by(
                scroll,
                ScrollOffset {
                    x: 0.0,
                    y: max_y + 400.0
                }
            )
            .unwrap()
    );
    let offset = context.world().scroll_offset(scroll.stable_id()).unwrap();
    assert!(
        (offset.y - max_y).abs() < 0.01,
        "wheel offset {} should clamp to {max_y}",
        offset.y
    );
    assert_eq!(offset.x, 0.0);
}

/// 200x120 scrollport holding 200px of rows, so the vertical axis overflows
/// by 80px.
#[test]
fn scroll_into_view_moves_the_minimum_distance_and_leaves_visible_targets_alone() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    // 200x120 viewport over five 40px rows: 200px of content, max offset 80.
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Always,
    );
    let viewport = crate::LayoutViewport::new(200.0, 120.0);
    context.layout_document(document, viewport).unwrap();
    let rows = context.world.node(scroll.id).unwrap().children;

    // Rows 0..=2 fill the viewport already, so nothing moves.
    assert!(!context.scroll_into_view(scroll, rows[1], 0.0).unwrap());
    assert_eq!(context.world.scroll_offset(scroll.id).unwrap().y, 0.0);

    // The last row ends at 200: scroll just far enough to seat its bottom edge.
    assert!(context.scroll_into_view(scroll, rows[4], 0.0).unwrap());
    assert_eq!(context.world.scroll_offset(scroll.id).unwrap().y, 80.0);

    // Coming back up aligns the target's top edge, not the bottom.
    assert!(context.scroll_into_view(scroll, rows[0], 0.0).unwrap());
    assert_eq!(context.world.scroll_offset(scroll.id).unwrap().y, 0.0);

    // A margin keeps context around the target where the container has room.
    assert!(context.scroll_into_view(scroll, rows[3], 8.0).unwrap());
    assert_eq!(context.world.scroll_offset(scroll.id).unwrap().y, 48.0);
}

#[test]
fn scroll_into_view_reports_a_missing_target() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Always,
    );
    context
        .layout_document(document, crate::LayoutViewport::new(200.0, 120.0))
        .unwrap();
    let missing = StableNodeId::new(9_999).unwrap();
    assert!(matches!(
        context.scroll_into_view(scroll, missing, 0.0),
        Err(FrameworkError::MissingView(id)) if id == missing
    ));
}

#[test]
fn assemble_confirm_dialog_builds_both_actions_and_routes_confirm_intent() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let dialog = context
        .create_component(
            document,
            crate::ConfirmDialog::new("删除", "无法撤销")
                .confirm_label("Delete")
                .cancel_label("Keep"),
        )
        .unwrap();
    context.append_child(host, dialog).unwrap();

    // One call replaces hand-building the two buttons and the slot wiring.
    assert!(context.assemble_confirm_dialog(dialog).unwrap());
    let slots = context
        .read(dialog, |dialog| dialog.confirm_slots().cloned())
        .unwrap()
        .expect("assemble published confirm slots");
    assert_eq!(
        context
            .read(Entity::<Button>::from_stable_id(slots.confirm), |button| {
                button.label.clone()
            })
            .unwrap(),
        "Delete"
    );

    let seen = Arc::new(Mutex::new(Vec::new()));
    let out = Arc::clone(&seen);
    context
        .on(dialog, move |_dialog, intent: &crate::ConfirmIntent, _| {
            out.lock().unwrap().push(*intent)
        })
        .unwrap();
    context.activate_overlay(host, dialog).unwrap();

    assert!(context.activate_node(slots.confirm).unwrap());
    assert!(context.activate_node(slots.cancel).unwrap());
    assert_eq!(
        &*seen.lock().unwrap(),
        &[
            crate::ConfirmIntent::Confirm { danger: false },
            crate::ConfirmIntent::Cancel
        ]
    );

    // Re-running only refreshes the existing actions.
    assert!(!context.assemble_confirm_dialog(dialog).unwrap());
}

#[test]
fn validity_reports_invalid_fields_in_document_order_and_ignores_disabled_ones() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let form = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let ok = context
        .create_detached_component(document, TextInput::new("ada"))
        .unwrap();
    let bad_name = context
        .create_detached_component(document, TextInput::new("").invalid(true))
        .unwrap();
    // Invalid but unreachable: it must not block submission.
    let bad_disabled = context
        .create_detached_component(document, TextInput::new("").invalid(true).disabled(true))
        .unwrap();
    let bad_email = context
        .create_detached_component(document, TextInput::new("x@").invalid(true))
        .unwrap();
    for child in [ok, bad_name, bad_disabled, bad_email] {
        context.append_child(form, child).unwrap();
    }

    let validity = context.validity_of(form.stable_id());
    assert!(!validity.is_valid());
    assert_eq!(
        validity.invalid,
        vec![bad_name.stable_id(), bad_email.stable_id()]
    );
    assert_eq!(validity.first_invalid(), Some(bad_name.stable_id()));

    // Clearing the fields the user can reach makes the form submittable, even
    // though the disabled one still carries `invalid`.
    for field in [bad_name, bad_email] {
        context
            .update_component(field, |input, _| input.invalid = false)
            .unwrap();
    }
    let validity = context.validity_of(form.stable_id());
    assert!(validity.is_valid());
    assert_eq!(validity.first_invalid(), None);
}

#[test]
fn a_drop_target_covers_its_subtree_and_only_the_kinds_it_accepts() {
    use nana_ui_core::{DropAccepts, DropEffect, DropKind};

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut panel_style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut panel_style.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(100.0));
    }
    let panel = context
        .create_component(document, Stack::column(0.0).style(panel_style))
        .unwrap();
    let inner = context
        .create_detached_component(document, Text::new("drop files here"))
        .unwrap();
    context.append_child(panel, inner).unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
        .unwrap();

    // Nothing is registered yet.
    assert!(
        context
            .drop_target_at(document, 20.0, 20.0, &DropKind::Files)
            .is_none()
    );

    context
        .set_drop_target(panel, DropAccepts::files().effect(DropEffect::Move))
        .unwrap();

    // A hit on the child resolves to the registered ancestor.
    let (target, effect) = context
        .drop_target_at(document, 20.0, 20.0, &DropKind::Files)
        .expect("the ancestor accepts the drop");
    assert_eq!(target, panel.stable_id());
    assert_eq!(effect, DropEffect::Move);

    // A kind it does not accept finds nothing.
    assert!(
        context
            .drop_target_at(document, 20.0, 20.0, &DropKind::custom("record"))
            .is_none()
    );

    // Outside the panel there is no target.
    assert!(
        context
            .drop_target_at(document, 380.0, 280.0, &DropKind::Files)
            .is_none()
    );

    assert!(context.clear_drop_target(panel));
    assert!(
        context
            .drop_target_at(document, 20.0, 20.0, &DropKind::Files)
            .is_none()
    );
}

/// Route one file-drag phase the way a host delivers it; whether it changed
/// anything.
fn route_file_drag(
    input: &mut crate::HeadlessInput,
    context: &mut AppContext,
    kind: nana_ui_core::FileDragKind,
    paths: &[std::path::PathBuf],
    position: Option<(f32, f32)>,
) -> bool {
    input
        .route(
            context,
            nana_ui_input::InputPayload::FileDrag(nana_ui_input::FileDragInput {
                kind,
                paths: paths.to_vec(),
                position,
                modifiers: Default::default(),
            }),
        )
        .unwrap()
        .handled
}

#[test]
fn file_drag_resolves_hover_and_drop_onto_the_registered_target() {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use nana_ui_core::{DropAccepts, DropEffect, FileDragKind};

    use crate::FileDropEvent;

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut panel_style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut panel_style.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(100.0));
    }
    let panel = context
        .create_component(document, Stack::column(0.0).style(panel_style))
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
        .unwrap();
    context
        .set_drop_target(panel, DropAccepts::files().effect(DropEffect::Copy))
        .unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&events);
    context
        .on(panel, move |_, event: &FileDropEvent, _| {
            log.lock().unwrap().push(event.clone());
        })
        .unwrap();

    let mut input = crate::HeadlessInput::bind(&mut context, document);
    let paths = [PathBuf::from("/tmp/note.md")];
    assert!(route_file_drag(
        &mut input,
        &mut context,
        FileDragKind::Hover,
        &paths,
        Some((20.0, 20.0))
    ));
    assert_eq!(
        context.drop_hover(),
        Some((panel.stable_id(), DropEffect::Copy))
    );
    assert!(route_file_drag(
        &mut input,
        &mut context,
        FileDragKind::Drop,
        &paths,
        Some((20.0, 20.0))
    ));
    assert!(context.drop_hover().is_none());
    assert!(!route_file_drag(
        &mut input,
        &mut context,
        FileDragKind::Hover,
        &paths,
        Some((380.0, 280.0))
    ));

    let log = events.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert!(matches!(
        &log[0],
        FileDropEvent::Hovered { effect, .. } if *effect == DropEffect::Copy
    ));
    assert!(matches!(&log[1], FileDropEvent::Dropped { .. }));
}

#[test]
fn a_drop_elsewhere_tells_the_hovered_target_it_was_left() {
    use std::{
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use nana_ui_core::{DropAccepts, DropEffect, FileDragKind};

    use crate::FileDropEvent;

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut panel_style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut panel_style.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(100.0));
    }
    let panel = context
        .create_component(document, Stack::column(0.0).style(panel_style))
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
        .unwrap();
    context
        .set_drop_target(panel, DropAccepts::files().effect(DropEffect::Copy))
        .unwrap();

    let events = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&events);
    context
        .on(panel, move |_, event: &FileDropEvent, _| {
            log.lock().unwrap().push(event.clone());
        })
        .unwrap();

    let mut input = crate::HeadlessInput::bind(&mut context, document);
    let paths = [PathBuf::from("/tmp/note.md")];
    route_file_drag(
        &mut input,
        &mut context,
        FileDragKind::Hover,
        &paths,
        Some((20.0, 20.0)),
    );
    // Released outside every target: the panel still has to hear that the
    // drag it was told about is over, the way a cancel tells it.
    route_file_drag(
        &mut input,
        &mut context,
        FileDragKind::Drop,
        &paths,
        Some((380.0, 280.0)),
    );

    let log = events.lock().unwrap();
    assert!(
        matches!(&log[0], FileDropEvent::Hovered { .. }),
        "expected a hover first, got {log:?}"
    );
    assert!(
        matches!(log.get(1), Some(FileDropEvent::Left)),
        "the hovered target was never told the drag left: {log:?}"
    );
}

#[test]
fn file_drag_drop_miss_redraws_so_hover_chrome_clears() {
    use std::path::PathBuf;

    use nana_ui_core::{DropAccepts, DropEffect, FileDragKind};

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut panel_style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut panel_style.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(100.0));
    }
    let panel = context
        .create_component(document, Stack::column(0.0).style(panel_style))
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(400.0, 300.0))
        .unwrap();
    context
        .set_drop_target(panel, DropAccepts::files().effect(DropEffect::Copy))
        .unwrap();

    let mut input = crate::HeadlessInput::bind(&mut context, document);
    let paths = [PathBuf::from("/tmp/note.md")];
    assert!(route_file_drag(
        &mut input,
        &mut context,
        FileDragKind::Hover,
        &paths,
        Some((20.0, 20.0))
    ));
    assert_eq!(
        context.drop_hover(),
        Some((panel.stable_id(), DropEffect::Copy))
    );
    assert!(
        route_file_drag(
            &mut input,
            &mut context,
            FileDragKind::Drop,
            &paths,
            Some((380.0, 280.0))
        ),
        "a drop miss must still request a redraw so hover chrome clears"
    );
    assert!(context.drop_hover().is_none());
}

fn overflowing_scroll_view(
    context: &mut AppContext,
    document: DocumentId,
    visibility: nana_ui_core::ScrollbarVisibility,
) -> Entity<ScrollView> {
    let mut viewport = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut viewport.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(120.0));
    }
    let scroll = context
        .create_component(
            document,
            ScrollView::new(ScrollAxes::Vertical)
                .scrollbars(visibility)
                .style(viewport),
        )
        .unwrap();
    for index in 0..5 {
        let mut row = NodeStyle::default();
        {
            let layout = Arc::make_mut(&mut row.layout);
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Px(40.0));
        }
        let row = context
            .create_component(document, Text::new(format!("Row {index}")).style(row))
            .unwrap();
        context.append_child(scroll, row).unwrap();
    }
    context
        .layout_document(document, crate::LayoutViewport::new(200.0, 120.0))
        .unwrap();
    scroll
}

fn vertical_bar(context: &AppContext, scroll: Entity<ScrollView>) -> Option<crate::ScrollbarBar> {
    match context.world().component_geometry(scroll.stable_id()) {
        Some(crate::ComponentGeometry::Scrollbar { vertical, .. }) => vertical,
        _ => None,
    }
}

#[test]
fn auto_hiding_scrollbars_appear_on_hover_and_follow_the_scroll_offset() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::AutoHide,
    );
    assert!(
        vertical_bar(&context, scroll).is_none(),
        "an idle auto-hiding container draws no bar"
    );

    context
        .set_pointer_hover_at(
            document,
            1,
            Some(scroll.stable_id()),
            std::time::Duration::ZERO,
        )
        .unwrap();
    let bar = vertical_bar(&context, scroll).expect("hover reveals the bar");
    // 120 of 200 content is visible, so the thumb takes 60% of the track.
    assert!(
        (bar.thumb.height - 72.0).abs() < 0.01,
        "thumb {:?}",
        bar.thumb
    );
    assert!(
        (bar.thumb.y - bar.track.y).abs() < 0.01,
        "thumb starts at the top"
    );
    assert!((bar.max_offset - 80.0).abs() < 0.01);
    assert_eq!(bar.track_background, None, "auto-hide draws no track");

    assert!(
        context
            .scroll_to(scroll, ScrollOffset { x: 0.0, y: 80.0 })
            .unwrap()
    );
    let bar = vertical_bar(&context, scroll).expect("still hovered");
    assert!(
        (bar.thumb.y + bar.thumb.height - (bar.track.y + bar.track.height)).abs() < 0.01,
        "a maxed offset pins the thumb to the track end: {:?}",
        bar.thumb
    );

    context
        .set_pointer_hover_at(document, 1, None, std::time::Duration::ZERO)
        .unwrap();
    assert!(
        vertical_bar(&context, scroll).is_none(),
        "leaving the container hides the bar again"
    );
}

#[test]
fn resident_scrollbars_draw_a_track_without_hover() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Always,
    );
    let bar = vertical_bar(&context, scroll).expect("resident bars need no hover");
    assert!(bar.track_background.is_some());
    assert!((bar.track.width - nana_ui_core::SCROLLBAR_METRICS.thickness).abs() < 0.01);
    assert!(
        (bar.track.x + bar.track.width - 200.0).abs() < 0.01,
        "bar hugs the right edge"
    );
}

/// Issue #101 F1: a control's **height** follows the installed theme.
///
/// This is the one a density setting actually moves. `ControlSize` was always
/// the intent, but `ControlSize::height()` resolved it against the
/// compile-time constant, so the control had fixed its height before any
/// theme existed. The step now lives on the node and resolves against the
/// installed metrics.
#[test]
fn an_installed_control_height_reaches_a_control_that_named_its_size() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let medium = context
        .create_component(document, crate::Button::new("Run"))
        .unwrap();
    let small = context
        .create_component(
            document,
            crate::Button::new("Run").size(nana_ui_core::ControlSize::Small),
        )
        .unwrap();

    let height_of = |context: &AppContext, entity: Entity<crate::Button>| {
        context.world().extract_nodes(&[entity.stable_id()])[0]
            .source_style
            .layout
            .min_height
    };
    let px = nana_ui_core::LengthSpec::Px;
    assert_eq!(
        height_of(&context, medium),
        Some(px(nana_ui_core::UI_METRICS.control_height))
    );
    assert_eq!(
        height_of(&context, small),
        Some(px(nana_ui_core::UI_METRICS.compact_control_height)),
        "the size step has to survive: a Small button is not a Medium one"
    );

    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.control_height = 44.0;
    metrics.compact_control_height = 36.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(height_of(&context, medium), Some(px(44.0)));
    assert_eq!(
        height_of(&context, small),
        Some(px(36.0)),
        "each step resolves against its own installed metric"
    );
}

/// Issue #101 F1: a control's **horizontal inset** follows the installed theme.
///
/// Same shape as height: `ControlPadding` names the step, construction no
/// longer spends `UI_METRICS.control_padding_x` / `field_padding_x` /
/// `list_item_padding_x`. A density setting has to be able to move all three.
#[test]
fn an_installed_control_padding_reaches_a_control_that_named_its_inset() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, crate::Button::new("Run"))
        .unwrap();
    let field = context
        .create_component(document, crate::TextInput::new("name"))
        .unwrap();
    let row = context
        .create_component(document, crate::ListItem::new("row"))
        .unwrap();

    let pad_of = |context: &AppContext, id| {
        context.world().extract_nodes(&[id])[0]
            .source_style
            .layout
            .padding_left
    };
    let px = nana_ui_core::LengthSpec::Px;
    assert_eq!(
        pad_of(&context, button.stable_id()),
        Some(px(nana_ui_core::UI_METRICS.control_padding_x))
    );
    assert_eq!(
        pad_of(&context, field.stable_id()),
        Some(px(nana_ui_core::UI_METRICS.field_padding_x))
    );
    assert_eq!(
        pad_of(&context, row.stable_id()),
        Some(px(nana_ui_core::UI_METRICS.list_item_padding_x))
    );

    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.control_padding_x = 20.0;
    metrics.field_padding_x = 18.0;
    metrics.list_item_padding_x = 14.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(pad_of(&context, button.stable_id()), Some(px(20.0)));
    assert_eq!(
        pad_of(&context, field.stable_id()),
        Some(px(18.0)),
        "a text field is not a medium button with different padding"
    );
    assert_eq!(
        pad_of(&context, row.stable_id()),
        Some(px(14.0)),
        "a list row keeps its own inset step under a density change"
    );
}

#[test]
fn an_installed_roomy_padding_is_no_longer_a_spacing_constant() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            crate::Button::new("Run").size(nana_ui_core::ControlSize::Large),
        )
        .unwrap();
    let pad_of = |context: &AppContext| {
        context.world().extract_nodes(&[button.stable_id()])[0]
            .source_style
            .layout
            .padding_left
    };
    let px = nana_ui_core::LengthSpec::Px;
    assert_eq!(
        pad_of(&context),
        Some(px(nana_ui_core::UI_METRICS.large_control_padding_x))
    );
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.large_control_padding_x = 22.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(pad_of(&context), Some(px(22.0)));
}

#[test]
fn an_installed_panel_padding_reaches_a_card_that_named_the_surface() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let card = context
        .create_component(document, crate::Card::new())
        .unwrap();
    let pad_of = |context: &AppContext| {
        let layout = &context.world().extract_nodes(&[card.stable_id()])[0]
            .source_style
            .layout;
        (layout.padding_left, layout.padding_top)
    };
    let px = nana_ui_core::LengthSpec::Px;
    assert_eq!(
        pad_of(&context),
        (
            Some(px(nana_ui_core::UI_METRICS.panel_padding_x)),
            Some(px(nana_ui_core::UI_METRICS.panel_padding_y)),
        )
    );
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.panel_padding_x = 24.0;
    metrics.panel_padding_y = 20.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(pad_of(&context), (Some(px(24.0)), Some(px(20.0))));
}

/// `colors_from_style` hands one button's colours to its style and leaves every
/// other icon button on its kind: a Primary beside it still paints its Accent
/// square, and the opted-in one paints no resting fill with the glyph and
/// hover wash its style names.
#[test]
fn an_icon_button_opts_into_its_own_colors_without_touching_its_siblings() {
    use nana_ui_core::SemanticColorRole as Role;
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let plain = context
        .create_component(
            document,
            crate::IconButton::new(nana_ui_core::Icon::Add, "Plain")
                .kind(nana_ui_core::ButtonKind::Primary),
        )
        .unwrap();
    let mut own = crate::IconButton::new(nana_ui_core::Icon::Add, "Own")
        .kind(nana_ui_core::ButtonKind::Primary)
        .colors_from_style();
    own.style.background = None;
    own.style.foreground = Some(Role::AccentText);
    own.style.interaction.hovered.background = Some(Role::AccentStrong);
    let own = context.create_component(document, own).unwrap();
    let style_of = |id| context.world().extract_nodes(&[id])[0].source_style.clone();
    let plain = style_of(plain.stable_id());
    assert_eq!(plain.background, Some(Role::Accent));
    assert_eq!(plain.foreground, Some(Role::AccentText));
    let own = style_of(own.stable_id());
    assert_eq!(own.background, None);
    assert_eq!(own.foreground, Some(Role::AccentText));
    assert_eq!(own.interaction.hovered.background, Some(Role::AccentStrong));
}

#[test]
fn an_installed_icon_button_size_reaches_the_square() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            crate::IconButton::new(nana_ui_core::Icon::Add, "Settings"),
        )
        .unwrap();
    let extent_of = |context: &AppContext| {
        let layout = &context.world().extract_nodes(&[button.stable_id()])[0]
            .source_style
            .layout;
        (layout.min_width, layout.min_height)
    };
    let px = nana_ui_core::LengthSpec::Px;
    let default = nana_ui_core::UI_METRICS.icon_button_size;
    assert_eq!(extent_of(&context), (Some(px(default)), Some(px(default))));
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.icon_button_size = 36.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(extent_of(&context), (Some(px(36.0)), Some(px(36.0))));
}

#[test]
fn an_installed_field_padding_reaches_a_textarea_block_inset() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(document, crate::TextArea::new("notes"))
        .unwrap();
    let pad_of = |context: &AppContext| {
        context.world().extract_nodes(&[area.stable_id()])[0]
            .source_style
            .layout
            .padding_top
    };
    let px = nana_ui_core::LengthSpec::Px;
    assert_eq!(
        pad_of(&context),
        Some(px(nana_ui_core::UI_METRICS.field_padding_x))
    );
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.field_padding_x = 18.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(pad_of(&context), Some(px(18.0)));
}

#[test]
fn an_installed_control_height_reaches_a_sidebar_row() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let row = context
        .create_component(document, crate::SidebarRow::new("舞台"))
        .unwrap();
    let height_of = |context: &AppContext| {
        context.world().extract_nodes(&[row.stable_id()])[0]
            .source_style
            .layout
            .height
    };
    let px = nana_ui_core::LengthSpec::Px;
    // Sidebar rows default to Small; Medium is `control_height`.
    assert_eq!(
        height_of(&context),
        Some(px(nana_ui_core::UI_METRICS.compact_control_height))
    );
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.compact_control_height = 36.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(height_of(&context), Some(px(36.0)));
}

#[test]
fn an_installed_control_height_reaches_a_segmented_control() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let control = context
        .create_component(document, crate::SegmentedControl::new())
        .unwrap();
    let height_of = |context: &AppContext| {
        context.world().extract_nodes(&[control.stable_id()])[0]
            .source_style
            .layout
            .height
    };
    let px = nana_ui_core::LengthSpec::Px;
    assert_eq!(
        height_of(&context),
        Some(px(nana_ui_core::UI_METRICS.control_height))
    );
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.control_height = 44.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(height_of(&context), Some(px(44.0)));
}

#[test]
fn an_installed_control_height_reaches_a_segmented_option() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let option = context
        .create_component(document, crate::SegmentedOption::new("Preview"))
        .unwrap();
    let height_of = |context: &AppContext| {
        context.world().extract_nodes(&[option.stable_id()])[0]
            .source_style
            .layout
            .height
    };
    let px = nana_ui_core::LengthSpec::Px;
    assert_eq!(
        height_of(&context),
        Some(px((nana_ui_core::UI_METRICS.control_height
            - nana_ui_core::space::SM)
            .max(0.0),))
    );
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.control_height = 44.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(
        height_of(&context),
        Some(px((44.0 - nana_ui_core::space::SM).max(0.0)))
    );
}

#[test]
fn an_installed_compact_height_reaches_a_dismissible_toast() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let toast = context
        .create_component(
            document,
            crate::Toast::new("Copied", crate::ToastTone::Info).dismissible(true),
        )
        .unwrap();
    let min_height_of = |context: &AppContext| {
        context.world().extract_nodes(&[toast.stable_id()])[0]
            .source_style
            .layout
            .min_height
    };
    let px = nana_ui_core::LengthSpec::Px;
    let default = crate::toast::PAD_Y * 2.0 + nana_ui_core::UI_METRICS.compact_control_height;
    assert_eq!(min_height_of(&context), Some(px(default)));
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.compact_control_height = 40.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(
        min_height_of(&context),
        Some(px(crate::toast::PAD_Y * 2.0 + 40.0))
    );
}

#[test]
fn an_installed_compact_height_reaches_a_context_menu_search_field() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let menu = context
        .create_component(
            document,
            crate::ContextMenu::new(24.0, 36.0)
                .items([crate::ContextMenuItem::new("open", "Open")])
                .searchable(true),
        )
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(400.0, 400.0))
        .unwrap();
    let search_height =
        |context: &AppContext| match context.world().extract_nodes(&[menu.stable_id()])[0]
            .component_geometry
            .as_deref()
        {
            Some(crate::ComponentGeometry::MenuSurface { search, .. }) => {
                search.as_ref().map(|region| region.bounds.height)
            }
            other => panic!("menu geometry, got {other:?}"),
        };
    assert_eq!(
        search_height(&context),
        Some(nana_ui_core::UI_METRICS.compact_control_height)
    );
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.compact_control_height = 40.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(search_height(&context), Some(40.0));
}

#[test]
fn an_installed_compact_height_reaches_a_tree_view() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let tree = context
        .create_component(
            document,
            crate::TreeView::new([nana_ui_core::TreeNode::leaf(
                std::sync::Arc::from("readme"),
                "README.md",
            )]),
        )
        .unwrap();
    let height_of = |context: &AppContext| {
        context.world().extract_nodes(&[tree.stable_id()])[0]
            .source_style
            .layout
            .height
    };
    let px = nana_ui_core::LengthSpec::Px;
    let default = nana_ui_core::UI_METRICS.compact_control_height;
    assert_eq!(height_of(&context), Some(px(default)));
    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.compact_control_height = 40.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(height_of(&context), Some(px(40.0)));
}

#[test]
fn an_installed_compact_height_reaches_a_sidebar_footer_button() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            crate::SidebarFooterButton::new("设置", nana_ui_core::Icon::Settings),
        )
        .unwrap();
    let extent_of = |context: &AppContext| {
        let layout = &context.world().extract_nodes(&[button.stable_id()])[0]
            .source_style
            .layout;
        (
            layout.width,
            layout.height,
            layout.min_width,
            layout.min_height,
        )
    };
    let install = |context: &mut AppContext, compact: f32| {
        let mut metrics = nana_ui_core::UI_METRICS;
        metrics.compact_control_height = compact;
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    };
    let px = nana_ui_core::LengthSpec::Px;
    let default = nana_ui_core::UI_METRICS.compact_control_height;
    assert_eq!(
        extent_of(&context),
        (
            Some(px(default)),
            Some(px(default)),
            Some(px(default)),
            Some(px(default))
        )
    );
    assert!(install(&mut context, 36.0));
    assert_eq!(
        extent_of(&context),
        (
            Some(px(36.0)),
            Some(px(36.0)),
            Some(px(36.0)),
            Some(px(36.0))
        )
    );
    // A spent `width: 28` would survive this shrink and the square would not.
    assert!(install(&mut context, 20.0));
    assert_eq!(
        extent_of(&context),
        (
            Some(px(20.0)),
            Some(px(20.0)),
            Some(px(20.0)),
            Some(px(20.0))
        )
    );
}

#[test]
fn an_installed_compact_height_reaches_a_settings_disclosure() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let summary = context
        .create_component(document, crate::Text::new("高级"))
        .unwrap();
    let details = context
        .create_component(document, crate::Text::new("明细"))
        .unwrap();
    let card = context
        .create_component(
            document,
            crate::SettingsCollapsibleCard::new(false)
                .summary(summary.stable_id())
                .details(details.stable_id()),
        )
        .unwrap();
    assert!(context.assemble_settings_collapsible_card(card).unwrap());
    let disclosure = context
        .read(card, |card| card.disclosure)
        .unwrap()
        .expect("disclosure");
    let extent_of = |context: &AppContext| {
        let layout = &context.world().extract_nodes(&[disclosure])[0]
            .source_style
            .layout;
        (
            layout.width,
            layout.height,
            layout.min_width,
            layout.min_height,
        )
    };
    let install = |context: &mut AppContext, compact: f32| {
        let mut metrics = nana_ui_core::UI_METRICS;
        metrics.compact_control_height = compact;
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    };
    let px = nana_ui_core::LengthSpec::Px;
    let default = nana_ui_core::UI_METRICS.compact_control_height;
    assert_eq!(
        extent_of(&context),
        (
            Some(px(default)),
            Some(px(default)),
            Some(px(default)),
            Some(px(default))
        )
    );
    assert!(install(&mut context, 36.0));
    assert_eq!(
        extent_of(&context),
        (
            Some(px(36.0)),
            Some(px(36.0)),
            Some(px(36.0)),
            Some(px(36.0))
        )
    );
    assert!(install(&mut context, 20.0));
    assert_eq!(
        extent_of(&context),
        (
            Some(px(20.0)),
            Some(px(20.0)),
            Some(px(20.0)),
            Some(px(20.0))
        )
    );
}

#[test]
fn an_installed_compact_height_reaches_a_thumbnail() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let thumb = context
        .create_component(document, crate::Thumbnail::empty())
        .unwrap();
    let extent_of = |context: &AppContext| {
        let layout = &context.world().extract_nodes(&[thumb.stable_id()])[0]
            .source_style
            .layout;
        (
            layout.width,
            layout.height,
            layout.min_width,
            layout.min_height,
            layout.border_radius,
        )
    };
    let install = |context: &mut AppContext, compact: f32, radius: f32| {
        let mut metrics = nana_ui_core::UI_METRICS;
        metrics.compact_control_height = compact;
        metrics.radius_xs = radius;
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    };
    let px = nana_ui_core::LengthSpec::Px;
    let default = nana_ui_core::UI_METRICS.compact_control_height;
    let radius = nana_ui_core::UI_METRICS.radius_xs;
    assert_eq!(
        extent_of(&context),
        (
            Some(px(default)),
            Some(px(default)),
            Some(px(default)),
            Some(px(default)),
            Some(radius)
        )
    );
    assert!(install(&mut context, 36.0, 8.0));
    assert_eq!(
        extent_of(&context),
        (
            Some(px(36.0)),
            Some(px(36.0)),
            Some(px(36.0)),
            Some(px(36.0)),
            Some(8.0)
        )
    );
    // A spent `width: 28` would survive this shrink and the 1:1 box would not.
    assert!(install(&mut context, 20.0, 8.0));
    assert_eq!(
        extent_of(&context),
        (
            Some(px(20.0)),
            Some(px(20.0)),
            Some(px(20.0)),
            Some(px(20.0)),
            Some(8.0)
        )
    );
    context
        .layout_document(document, crate::LayoutViewport::new(240.0, 80.0))
        .unwrap();
    let bounds = context.world().layout_box(thumb.stable_id()).unwrap();
    assert_eq!((bounds.width, bounds.height), (20.0, 20.0));
}

/// Issue #101 F1: a control's corner radius follows the **installed** theme.
///
/// The Appearance radius setting produces a `ThemeMetrics`; before this the
/// control had already turned `radius_sm` into a number at construction time,
/// so installing that theme moved nothing. The control now names the tier and
/// extraction resolves it, the same way the palette is resolved.
#[test]
fn an_installed_radius_reaches_a_control_that_named_the_tier() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, crate::Button::new("Run build"))
        .unwrap();

    let radius_of = |context: &AppContext| {
        context.world().extract_nodes(&[button.stable_id()])[0]
            .source_style
            .layout
            .border_radius
    };
    assert_eq!(
        radius_of(&context),
        Some(nana_ui_core::UI_METRICS.radius_sm)
    );

    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.radius_sm = 2.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(
        radius_of(&context),
        Some(2.0),
        "a named radius tier must resolve against the installed metrics"
    );

    // The same install also has to reach the chrome the Scene paints for a
    // node that authored no radius of its own (menu surfaces, modal frames,
    // palette rows). Those steps ride on the extracted node.
    let chrome = context.world().extract_nodes(&[button.stable_id()])[0].chrome_radii;
    assert_eq!(chrome.sm, 2.0);
    assert_eq!(chrome.md, nana_ui_core::UI_METRICS.radius_md);

    // The escape hatch stays an escape hatch: an explicit px radius is a
    // one-off local intent and must not be recaptured by the theme.
    let pinned = context
        .create_component(
            document,
            crate::Button::new("Pinned").style(crate::NodeStyle::default().radius_px(11.0)),
        )
        .unwrap();
    assert_eq!(
        context.world().extract_nodes(&[pinned.stable_id()])[0]
            .source_style
            .layout
            .border_radius,
        Some(11.0)
    );
}

/// Per-corner steps follow the installed theme the way a uniform one does.
///
/// Two blocks joined edge to edge round their outer corners and meet square,
/// and the only per-corner control was `paint.border_radii`, which takes
/// lengths. Spending the installed `radius_*` into it at construction left
/// those shapes behind when the Appearance radius changed, while every uniform
/// corner in the same window moved. An unnamed corner stays square through
/// the install.
#[test]
fn named_corner_steps_follow_an_installed_radius() {
    use nana_ui_core::RadiusTier::{Lg, Xs};
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let block = context
        .create_component(
            document,
            crate::Stack::column(0.0).style(crate::NodeStyle::default().corner_radii([
                Some(Lg),
                Some(Xs),
                None,
                Some(Lg),
            ])),
        )
        .unwrap();
    let corners_of = |context: &AppContext| {
        context.world().extract_nodes(&[block.stable_id()])[0]
            .source_style
            .layout
            .resolved_border_radii(100.0, 100.0)
    };
    let defaults = nana_ui_core::UI_METRICS;
    assert_eq!(
        corners_of(&context),
        [
            defaults.radius_lg,
            defaults.radius_xs,
            0.0,
            defaults.radius_lg
        ]
    );

    let mut metrics = defaults;
    metrics.radius_lg = 30.0;
    metrics.radius_xs = 1.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(corners_of(&context), [30.0, 1.0, 0.0, 30.0]);
}

/// A switch with no label is its track.
///
/// A row's trailing toggle has no text to give its node a width, so the
/// default `Fill` stretched an invisible hit box across the row and the size
/// step's inset ate the content box. Every caller had to spend the track width
/// and zero the padding by hand — and a number spent there stayed behind when
/// the installed track changed.
#[test]
fn a_switch_with_no_label_is_its_track() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let row = context
        .create_component(document, crate::Stack::row(0.0))
        .unwrap();
    let switch = context
        .create_component(document, crate::Switch::new("", true))
        .unwrap();
    context.append_child(row, switch).unwrap();
    let measure = |context: &mut AppContext| {
        context
            .layout_document(document, crate::LayoutViewport::new(320.0, 80.0))
            .unwrap();
        let node = context
            .world()
            .layout_box(switch.stable_id())
            .unwrap()
            .width;
        let control = match context.world().extract_nodes(&[switch.stable_id()])[0]
            .component_geometry
            .as_deref()
        {
            Some(crate::ComponentGeometry::Switch { control, .. }) => control.width,
            other => panic!("a switch derives switch geometry, got {other:?}"),
        };
        (node, control)
    };
    let track = nana_ui_core::SWITCH_METRICS.track_width;
    assert_eq!(measure(&mut context), (track, track));

    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.switch.track_width = 44.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    assert_eq!(
        measure(&mut context),
        (44.0, 44.0),
        "the box follows the installed track, not the one it was built with"
    );
}

/// Issue #101 F1: the switch track follows the **installed** theme.
///
/// Its width, height and label gap used to be four literals in
/// `world/geometry.rs`, so no theme or density could move them — and the scene
/// painter carried a hand-added `30 + 8` of its own, which meant moving the
/// track desynced the label inset silently. `ThemeMetrics::switch` owns them
/// now, and this is the assertion that says the installed value arrives.
#[test]
fn an_installed_switch_track_reaches_the_switch() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let switch = context
        .create_component(document, crate::Switch::new("Follow cursor", false))
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(320.0, 80.0))
        .unwrap();

    let track_of = |context: &AppContext| match context.world().extract_nodes(&[switch.stable_id()])
        [0]
    .component_geometry
    .as_deref()
    {
        Some(crate::ComponentGeometry::Switch { control, .. }) => (control.width, control.x),
        other => panic!("a switch derives switch geometry, got {other:?}"),
    };
    let (width, x) = track_of(&context);
    assert_eq!(width, nana_ui_core::SWITCH_METRICS.track_width);

    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.switch.track_width = 48.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap()
    );
    context
        .layout_document(document, crate::LayoutViewport::new(320.0, 80.0))
        .unwrap();
    let (wide, moved) = track_of(&context);
    assert_eq!(
        wide, 48.0,
        "the installed track width is the one that paints"
    );
    assert!(
        moved < x,
        "a trailing track that grew has to start further left, not overflow \
         its content box: {moved} vs {x}"
    );
}

/// A switch that spent its own inset keeps it.
///
/// `control_padding_x` overwrites the padding edges, and `Switch` used to set
/// it unconditionally — which made it the one intent field a switch could not
/// opt out of. A trailing switch has no label to supply its width, so the
/// default inset ate its content box and the track, which clamps to
/// `min(track_width, content width)`, painted a sliver.
#[test]
fn a_switch_that_authored_its_inset_keeps_its_track() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let track = nana_ui_core::SWITCH_METRICS.track_width;

    let mut bare = crate::Switch::new("", false);
    {
        let layout = std::sync::Arc::make_mut(&mut bare.style.layout);
        layout.width = Some(nana_ui_core::LengthSpec::Px(track));
        layout.padding_left = Some(nana_ui_core::LengthSpec::Px(0.0));
        layout.padding_right = Some(nana_ui_core::LengthSpec::Px(0.0));
    }
    let bare = context.create_component(document, bare).unwrap();
    // A labelled one alongside, to show the default still arrives for callers
    // that said nothing.
    let labelled = context
        .create_component(document, crate::Switch::new("Follow cursor", false))
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(320.0, 120.0))
        .unwrap();

    let control_width = |context: &AppContext, id: crate::StableNodeId| match context
        .world()
        .extract_nodes(&[id])[0]
        .component_geometry
        .as_deref()
    {
        Some(crate::ComponentGeometry::Switch { control, .. }) => control.width,
        other => panic!("a switch derives switch geometry, got {other:?}"),
    };
    assert_eq!(
        control_width(&context, bare.stable_id()),
        track,
        "a switch that spent its own inset gets a content box wide enough for \
         the whole track"
    );
    assert_eq!(
        control_width(&context, labelled.stable_id()),
        track,
        "and a labelled switch still gets the size step's inset"
    );
}

/// Issue #101 F1: a focused row's secondary text stays readable.
///
/// A `ListItem` focuses with `SemanticPaint::FOCUS_SURFACE`, which fills it
/// with `focus_surface` and moves the label to `focus_text`. The detail line
/// used to read `palette.muted` straight off the palette, so it kept a colour
/// resolved against the surface the row no longer had — 1.09:1 against the
/// light fill, which no single value of `muted` can fix, because it also has to
/// stay quiet on a white card. `foreground_secondary` lets the state say so.
#[test]
fn a_focused_rows_secondary_text_follows_its_label() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut view = crate::ListItem::new("nanalive.model3.json");
    view.detail = "current actor".into();
    let row = context.create_component(document, view).unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(320.0, 80.0))
        .unwrap();

    let colors = |context: &AppContext| match context.world().extract_nodes(&[row.stable_id()])[0]
        .component_geometry
        .as_deref()
    {
        Some(crate::ComponentGeometry::ListItem { detail, .. }) => {
            detail.as_ref().expect("the row declares a detail").color
        }
        other => panic!("a list item derives list-item geometry, got {other:?}"),
    };
    let palette = nana_ui_core::SemanticPalette::dark();
    assert_eq!(
        colors(&context),
        Some(palette.muted.as_rgba_array()),
        "an unfocused row's detail is still the muted role it always was"
    );

    assert!(context.focus_node(document, row.stable_id()).unwrap());
    context
        .layout_document(document, crate::LayoutViewport::new(320.0, 80.0))
        .unwrap();
    assert_eq!(
        colors(&context),
        Some(palette.focus_text.as_rgba_array()),
        "a focused row fills with focus_surface, so its detail has to follow \
         the label onto focus_text instead of staying resolved against the \
         surface underneath"
    );
}

/// Issue #101 F1/F10: the scrollbar follows the **installed** theme.
///
/// Its geometry used to come only from the `SCROLLBAR_METRICS` constant, so a
/// theme or density change could not move it. `ThemeMetrics` now owns the
/// scrollbar metrics, and this is the assertion that says the installed value
/// is the one that arrives.
#[test]
fn a_thicker_scrollbar_in_the_installed_theme_reaches_the_bar() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Always,
    );
    let default_width = vertical_bar(&context, scroll)
        .expect("resident bars need no hover")
        .track
        .width;
    assert!((default_width - nana_ui_core::SCROLLBAR_METRICS.thickness).abs() < 0.01);

    let mut metrics = nana_ui_core::UI_METRICS;
    metrics.scrollbar.thickness = nana_ui_core::SCROLLBAR_METRICS.thickness * 2.0;
    metrics.scrollbar.thumb_thickness = nana_ui_core::SCROLLBAR_METRICS.thumb_thickness * 2.0;
    assert!(
        context
            .set_style_tokens(
                nana_ui_core::ThemeMode::Dark,
                metrics,
                nana_ui_core::SemanticPalette::dark(),
                nana_ui_core::SemanticPalette::dark().surface,
            )
            .unwrap(),
        "the metrics changed, so installing them is not a no-op"
    );

    let widened = vertical_bar(&context, scroll)
        .expect("the bar survives a theme install")
        .track
        .width;
    assert!(
        (widened - default_width * 2.0).abs() < 0.01,
        "installed scrollbar thickness must reach the bar: {default_width} -> {widened}"
    );
}

#[test]
fn hidden_scrollbars_leave_wheel_scrolling_alone() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Hidden,
    );
    context
        .set_pointer_hover_at(
            document,
            1,
            Some(scroll.stable_id()),
            std::time::Duration::ZERO,
        )
        .unwrap();
    assert!(vertical_bar(&context, scroll).is_none());
    assert!(
        context
            .scroll_by(scroll, ScrollOffset { x: 0.0, y: 40.0 })
            .unwrap()
    );
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()),
        Some(ScrollOffset { x: 0.0, y: 40.0 })
    );
}

#[test]
fn dragging_the_thumb_moves_the_authoritative_scroll_offset() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Always,
    );
    let bar = vertical_bar(&context, scroll).expect("resident bar");
    let grab_x = bar.thumb.x + bar.thumb.width / 2.0;
    let grab_y = bar.thumb.y + bar.thumb.height / 2.0;
    assert_eq!(
        context.scrollbar_axis_at(scroll.stable_id(), grab_x, grab_y),
        Some(nana_ui_core::ScrollbarAxis::Vertical)
    );
    assert!(
        context
            .begin_scrollbar_drag(
                7,
                scroll.stable_id(),
                nana_ui_core::ScrollbarAxis::Vertical,
                grab_x,
                grab_y,
            )
            .unwrap()
    );
    assert_eq!(
        context.world().pointer_capture(document, 7),
        Some(scroll.stable_id())
    );
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()),
        Some(ScrollOffset::default()),
        "grabbing the thumb must not jump the content"
    );

    // Travel is 48px for 80px of content, so half the travel is 40px.
    assert!(
        context
            .update_scrollbar_drag(document, 7, grab_x, grab_y + 24.0)
            .unwrap()
    );
    let offset = context.world().scroll_offset(scroll.stable_id()).unwrap();
    assert!((offset.y - 40.0).abs() < 0.01, "offset {offset:?}");
    assert_eq!(offset.x, 0.0, "a vertical drag holds the other axis");

    assert!(
        context
            .update_scrollbar_drag(document, 7, grab_x, grab_y + 4000.0)
            .unwrap()
    );
    assert!(
        (context.world().scroll_offset(scroll.stable_id()).unwrap().y - 80.0).abs() < 0.01,
        "the drag clamps at the maximum offset"
    );

    assert!(context.end_scrollbar_drag(document, 7, false).unwrap());
    assert_eq!(context.world().pointer_capture(document, 7), None);
    assert!(
        !context
            .update_scrollbar_drag(document, 7, grab_x, grab_y)
            .unwrap(),
        "a released pointer no longer drives the bar"
    );
}

#[test]
fn pressing_bare_track_pages_toward_the_press_and_cancelling_restores_it() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = overflowing_scroll_view(
        &mut context,
        document,
        nana_ui_core::ScrollbarVisibility::Always,
    );
    let bar = vertical_bar(&context, scroll).expect("resident bar");
    let track_end = bar.track.y + bar.track.height - 1.0;
    assert!(
        context
            .begin_scrollbar_drag(
                3,
                scroll.stable_id(),
                nana_ui_core::ScrollbarAxis::Vertical,
                bar.thumb.x + 1.0,
                track_end,
            )
            .unwrap()
    );
    assert!(
        (context.world().scroll_offset(scroll.stable_id()).unwrap().y - 80.0).abs() < 0.01,
        "a press below the thumb centres it on the press"
    );
    assert!(context.end_scrollbar_drag(document, 3, true).unwrap());
    assert_eq!(
        context.world().scroll_offset(scroll.stable_id()),
        Some(ScrollOffset::default()),
        "cancel restores the offset the drag started from"
    );
}

#[test]
fn a_secondary_press_reaches_the_nearest_handler_above_the_hit_node() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut card = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut card.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(100.0));
    }
    let card = context
        .create_component(document, Card::new().style(card))
        .unwrap();
    let mut row = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut row.layout);
        layout.width = Some(LengthSpec::Px(200.0));
        layout.height = Some(LengthSpec::Px(40.0));
    }
    let row = context
        .create_component(document, Button::new("Row").style(row))
        .unwrap();
    context.append_child(card, row).unwrap();
    let presses = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&presses);
    context
        .on(card, move |_card, press: &SecondaryPress, _cx| {
            observed.lock().unwrap().push(*press);
        })
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(200.0, 100.0))
        .unwrap();
    context.rebuild_hit_test(document);

    assert_eq!(
        context.secondary_press_at(document, 20.0, 20.0).unwrap(),
        Some(card.stable_id()),
        "the press bubbles to the enclosing handler"
    );
    let press = *presses.lock().unwrap().first().expect("one press");
    assert_eq!(press.target, row.stable_id(), "it carries the hit node");
    assert_eq!((press.x, press.y), (20.0, 20.0));

    assert_eq!(
        context.secondary_press_at(document, 900.0, 900.0).unwrap(),
        None,
        "a press outside the tree hits nothing"
    );
    assert_eq!(presses.lock().unwrap().len(), 1);
}

#[test]
#[cfg(feature = "rich-text")]
fn selection_reads_back_from_editors_and_from_focused_rich_text() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let input = context
        .create_component(document, TextInput::new("Nana"))
        .unwrap();
    assert!(context.focus_node(document, input.stable_id()).unwrap());
    assert_eq!(
        context.focused_selected_text(document),
        None,
        "a caret selects nothing"
    );
    assert!(context.select_all_focused_text(document).unwrap());
    assert_eq!(
        context.focused_selected_text(document).as_deref(),
        Some("Nana")
    );
    assert!(
        !context.select_all_focused_text(document).unwrap(),
        "selecting all twice is not a change"
    );
    assert_eq!(
        context.cut_focused_text(document).unwrap().as_deref(),
        Some("Nana")
    );
    assert_eq!(context.world().text(input.stable_id()), Some(""));
    assert_eq!(context.cut_focused_text(document).unwrap(), None);

    let text = context
        .create_component(
            document,
            crate::SelectableRichText::new([crate::RichSpan::plain("Hello")]),
        )
        .unwrap();
    assert!(context.focus_node(document, text.stable_id()).unwrap());
    let area = crate::LayoutBox {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 20.0,
    };
    let caret = |index: usize| index as f32 * crate::rich_text::GRAPHEME_ADVANCE + 1.0;
    context
        .read(text, |text| {
            assert!(text.pointer_down(caret(0), 8.0, area));
            assert!(text.pointer_move(caret(4), 8.0, area));
            text.pointer_up(caret(4), 8.0, area)
        })
        .unwrap();
    assert_eq!(
        context.focused_selected_text(document).as_deref(),
        Some("Hell"),
        "a rich-text selection is what a host copy takes"
    );
    assert_eq!(
        context.cut_focused_text(document).unwrap(),
        None,
        "rich text is not editable, so nothing is cut"
    );
}

#[test]
fn a_secondary_press_without_a_handler_opens_nothing() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(120.0));
        layout.height = Some(LengthSpec::Px(40.0));
    }
    let button = context
        .create_component(document, Button::new("Build").style(style))
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(120.0, 40.0))
        .unwrap();
    context.rebuild_hit_test(document);
    let generation = context.world().generation();
    assert_eq!(
        context.secondary_press_at(document, 10.0, 10.0).unwrap(),
        None
    );
    assert_eq!(
        context.world().generation(),
        generation,
        "an unhandled secondary press must not touch the tree"
    );
    assert!(context.world().focused(document).is_none());
    let _ = button;
}

#[test]
fn scroll_view_with_forty_rows_dirties_forty_one_hit_targets() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let scroll = context
        .create_component(document, ScrollView::new(ScrollAxes::Vertical))
        .unwrap();
    for index in 0..40 {
        let row = context
            .create_component(document, Text::new(format!("Visible row {index}")))
            .unwrap();
        context.append_child(scroll, row).unwrap();
    }
    let _ = context.take_system_work();
    assert!(
        context
            .scroll_to(scroll, ScrollOffset { x: 0.0, y: 120.0 })
            .unwrap()
    );
    let work = context.take_system_work();
    // Scroller-only hit/extract; Scene recomposes descendants from offset.
    assert_eq!(work.input_hit_test.len(), 1);
    assert_eq!(work.render_extraction.len(), 1);
    assert!(work.layout.is_empty());
    let updates = context.take_scroll_hit_updates();
    assert!(
        context.hit_test_work_is_scroll_only(&work.input_hit_test, &updates),
        "pure scrolling must be recognized as patch-only"
    );
}

#[test]
fn native_theme_resolves_semantic_component_paint_without_layout_work() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            Button::new("Build").kind(nana_ui_core::ButtonKind::Primary),
        )
        .unwrap();
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    let dark = context
        .world()
        .extract_nodes(&[button.stable_id()])
        .pop()
        .unwrap();
    assert_eq!(
        dark.style.background,
        Some(
            nana_ui_core::SemanticPalette::dark()
                .accent_soft
                .as_rgba_array()
        )
    );
    context
        .compat_world_mut()
        .set_pointer_hover(document, 1, Some(button.stable_id()))
        .unwrap();
    context.advance_animations(nana_ui_core::motion::HOVER_COLOR);
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    assert_eq!(
        context
            .world()
            .extract_nodes(&[button.stable_id()])
            .pop()
            .unwrap()
            .style
            .background,
        Some(
            nana_ui_core::SemanticPalette::dark()
                .accent_soft_hover
                .as_rgba_array()
        )
    );
    context
        .compat_world_mut()
        .press_pointer(document, 1, button.stable_id())
        .unwrap();
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    assert_eq!(
        context
            .world()
            .extract_nodes(&[button.stable_id()])
            .pop()
            .unwrap()
            .style
            .background,
        Some(
            nana_ui_core::SemanticPalette::dark()
                .accent_soft_pressed
                .as_rgba_array()
        )
    );
    assert_eq!(
        context.release_pointer(document, 1),
        Some(button.stable_id())
    );
    context
        .compat_world_mut()
        .set_pointer_hover(document, 1, None)
        .unwrap();
    context.compat_world_mut().take_system_work();

    assert!(context.set_theme(ThemeMode::Light).unwrap());
    let work = context.compat_world_mut().take_system_work();
    assert!(work.style.is_empty());
    assert!(work.layout.is_empty());
    assert!(work.render_extraction.contains(&button.stable_id()));
    let light = context
        .world()
        .extract_nodes(&[button.stable_id()])
        .pop()
        .unwrap();
    assert_eq!(
        light.style.background,
        Some(
            nana_ui_core::SemanticPalette::light()
                .accent_soft
                .as_rgba_array()
        )
    );

    let mut focus = MutationQueue::new();
    focus.request_focus(document, Some(button.stable_id()));
    context.compat_world_mut().commit(focus).unwrap();
    let work = context.compat_world_mut().take_system_work();
    assert_eq!(work.focus_ime, vec![button.stable_id()]);
    assert_eq!(work.accessibility, vec![button.stable_id()]);
    assert!(context.compat_world_mut().take_system_work().is_empty());
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    assert!(context.compat_world_mut().take_system_work().is_empty());
    let focused = context
        .world()
        .extract_nodes(&[button.stable_id()])
        .pop()
        .unwrap();
    assert_eq!(focused.style.border_color, None);
    // Focus takes the background over from the resting accent fill. It used to
    // leave `accent_soft` in place, which is another way of saying a focused
    // button looked exactly like an unfocused one.
    assert_eq!(
        focused.style.background,
        Some(
            nana_ui_core::SemanticPalette::light()
                .focus_surface
                .as_rgba_array()
        )
    );

    context
        .update_component(button, |button, _cx| button.disabled = true)
        .unwrap();
    let work = context.compat_world_mut().take_system_work();
    assert_eq!(work.focus_ime, vec![button.stable_id()]);
    assert_eq!(work.accessibility, vec![button.stable_id()]);
    assert!(context.compat_world_mut().take_system_work().is_empty());
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    let post_resolve = context.compat_world_mut().take_system_work();
    assert_eq!(post_resolve.focus_ime, vec![button.stable_id()]);
    assert_eq!(post_resolve.accessibility, vec![button.stable_id()]);
    assert_eq!(post_resolve.render_extraction, vec![button.stable_id()]);
    assert_eq!(context.world().focused(document), None);
    let disabled = context
        .world()
        .extract_nodes(&[button.stable_id()])
        .pop()
        .unwrap();
    assert_eq!(
        disabled.style.background,
        Some(
            nana_ui_core::SemanticPalette::light()
                .subtle
                .as_rgba_array()
        )
    );

    let generation = context.world().generation();
    assert!(!context.set_theme(ThemeMode::Light).unwrap());
    assert_eq!(context.world().generation(), generation);
    let idle = context.compat_world_mut().take_system_work();
    assert!(
        idle.is_empty(),
        "unexpected work after theme no-op: {idle:?}"
    );
}

#[test]
fn view_mutations_schedule_host_driven_animation_frames() {
    let mut context = AppContext::new();
    let entity = context
        .create_view(
            DocumentId::new(1).unwrap(),
            NodeKind::Document,
            Counter { value: 0 },
        )
        .unwrap();
    let id = AnimationId::new(1).unwrap();
    context
        .update(entity, |_view, cx| {
            let target = cx.entity().stable_id();
            cx.mutations().start_animation(AnimationSpec::new(
                id,
                target,
                Duration::from_millis(40),
                Duration::from_millis(80),
                Duration::from_millis(10),
                Easing::Linear,
            ));
        })
        .unwrap();

    assert_eq!(
        context.next_animation_deadline(),
        Some(Duration::from_millis(40))
    );
    let frame = context.advance_animations(Duration::from_millis(80));
    assert_eq!(frame.samples.len(), 1);
    assert_eq!(frame.samples[0].target, entity.stable_id());
    assert_eq!(frame.samples[0].progress, 0.5);
    assert_eq!(frame.next_deadline, Some(Duration::from_millis(90)));
}

#[test]
fn remount_resumes_loading_lifecycle_in_a_retained_descendant() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let host = context
        .create_component(document, Button::new("Host"))
        .unwrap();
    let parent = context
        .create_component(document, Button::new("Parent"))
        .unwrap();
    let loading = context
        .create_detached_component(document, Button::new("Loading").loading(true))
        .unwrap();

    assert_eq!(context.next_animation_deadline(), None);
    context.append_child(parent, loading).unwrap();
    assert_eq!(context.next_animation_deadline(), Some(Duration::ZERO));

    let mut park = MutationQueue::new();
    park.park_subtree(parent.stable_id());
    context.commit_mutations(park).unwrap();
    assert_eq!(context.next_animation_deadline(), None);

    let mut remount = MutationQueue::new();
    remount.insert(host.stable_id(), parent.stable_id(), None);
    context.commit_mutations(remount).unwrap();
    assert_eq!(context.next_animation_deadline(), Some(Duration::ZERO));

    let frame = context.advance_animations(Duration::from_millis(400));
    assert!(frame.component_updates.contains(&loading.stable_id()));
    assert_eq!(
        context
            .read(loading, |button| button.loading_phase)
            .unwrap(),
        0.5
    );
    assert!(context.next_animation_deadline().is_some());
}

#[test]
fn icon_button_tooltip_uses_hover_clock_and_real_overlay_child() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            IconButton::new(nana_ui_core::Icon::About, "Details").tooltip(
                "More details",
                nana_ui_core::TooltipConfig {
                    placement: nana_ui_core::TooltipPlacement::Left,
                    delay_ms: 100,
                    gap: 6.0,
                    viewport_padding: 4.0,
                    max_width: 120.0,
                },
            ),
        )
        .unwrap();
    let tooltip = context.icon_button_tooltip(button).unwrap().unwrap();
    assert_eq!(
        context.world().node(tooltip.stable_id()).unwrap().parent,
        Some(button.stable_id())
    );
    assert_eq!(
        context.world().overlay_host(button.stable_id()),
        Some(crate::OverlayHostState::default())
    );
    context
        .layout_document(document, crate::LayoutViewport::new(160.0, 80.0))
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        crate::LayoutBox {
            x: 20.0,
            y: 50.0,
            width: 28.0,
            height: 28.0,
        },
    );
    context.commit_mutations(layout).unwrap();

    context
        .set_pointer_hover_at(
            document,
            1,
            Some(button.stable_id()),
            Duration::from_millis(10),
        )
        .unwrap();
    assert!(
        context
            .next_animation_deadline()
            .is_some_and(|time| time <= Duration::from_millis(110))
    );
    context.advance_animations(Duration::from_millis(109));
    assert_eq!(
        context
            .world()
            .overlay_host(button.stable_id())
            .unwrap()
            .active,
        None
    );
    assert!(
        context
            .advance_animations(Duration::from_millis(110))
            .component_updates
            .contains(&button.stable_id())
    );
    assert_eq!(
        context
            .world()
            .overlay_host(button.stable_id())
            .unwrap()
            .active,
        Some(tooltip.stable_id())
    );
    let tooltip_style = context.world().node_style(tooltip.stable_id()).unwrap();
    assert!(matches!(
        tooltip_style.layout.offset_left,
        Some(LengthSpec::Px(x)) if (4.0..=156.0).contains(&x)
    ));
    assert!(matches!(
        tooltip_style.layout.offset_top,
        Some(LengthSpec::Px(y)) if (4.0..=76.0).contains(&y)
    ));
    assert!(
        matches!(
            tooltip_style.layout.offset_left,
            Some(LengthSpec::Px(x)) if x >= 54.0
        ),
        "tooltip should flip to the anchor's right, got {:?}",
        tooltip_style.layout.offset_left
    );

    context
        .set_pointer_hover_at(document, 1, None, Duration::from_millis(111))
        .unwrap();
    assert_eq!(
        context.world().overlay_host(button.stable_id()),
        Some(crate::OverlayHostState::default())
    );
    context.advance_animations(Duration::from_millis(231));
    assert_eq!(context.next_animation_deadline(), None);
}

#[test]
fn parked_icon_button_closes_tooltip_projection_and_does_not_reopen_on_remount() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let host = context
        .create_component(document, Button::new("Host"))
        .unwrap();
    let button = context
        .create_component(
            document,
            IconButton::new(nana_ui_core::Icon::About, "Details").tooltip(
                "More details",
                nana_ui_core::TooltipConfig {
                    delay_ms: 0,
                    ..nana_ui_core::TooltipConfig::default()
                },
            ),
        )
        .unwrap();

    context
        .set_pointer_hover_at(document, 1, Some(button.stable_id()), Duration::ZERO)
        .unwrap();
    assert!(context.read(button, |button| button.tooltip_open).unwrap());
    assert!(matches!(
        context.world().standard_visual(button.stable_id()),
        Some(StandardVisual::Icon {
            tooltip: Some(crate::TooltipVisual { open: true, .. }),
            ..
        })
    ));

    let mut park = MutationQueue::new();
    park.park_subtree(button.stable_id());
    context.commit_mutations(park).unwrap();
    assert!(!context.read(button, |button| button.tooltip_open).unwrap());
    assert!(matches!(
        context.world().standard_visual(button.stable_id()),
        Some(StandardVisual::Icon {
            tooltip: Some(crate::TooltipVisual { open: false, .. }),
            ..
        })
    ));
    assert_eq!(
        context.world().overlay_host(button.stable_id()),
        Some(crate::OverlayHostState::default())
    );
    assert_eq!(context.next_animation_deadline(), None);

    let mut remount = MutationQueue::new();
    remount.insert(host.stable_id(), button.stable_id(), None);
    context.commit_mutations(remount).unwrap();
    assert!(
        !context
            .advance_animations(Duration::from_secs(1))
            .has_updates()
    );
    assert!(!context.read(button, |button| button.tooltip_open).unwrap());
    assert_eq!(
        context.world().overlay_host(button.stable_id()),
        Some(crate::OverlayHostState::default())
    );

    context
        .set_pointer_hover_at(
            document,
            2,
            Some(button.stable_id()),
            Duration::from_secs(2),
        )
        .unwrap();
    assert!(context.read(button, |button| button.tooltip_open).unwrap());
    context
        .update_component(button, |_button, cx| {
            cx.mutations().park_subtree(button.stable_id());
        })
        .unwrap();
    assert!(!context.read(button, |button| button.tooltip_open).unwrap());
    assert!(matches!(
        context.world().standard_visual(button.stable_id()),
        Some(StandardVisual::Icon {
            tooltip: Some(crate::TooltipVisual { open: false, .. }),
            ..
        })
    ));
}

#[test]
fn tooltip_default_delay_stays_closed_until_deadline_and_is_label_only() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            IconButton::new(nana_ui_core::Icon::About, "Details")
                .tooltip("More details", nana_ui_core::TooltipConfig::default()),
        )
        .unwrap();
    let tooltip = context.icon_button_tooltip(button).unwrap().unwrap();
    assert_eq!(
        context.world().text(tooltip.stable_id()),
        Some("More details")
    );
    let accessibility = context.world().accessibility(tooltip.stable_id()).unwrap();
    assert_eq!(accessibility.role, crate::AccessibilityRole::Tooltip);
    assert_eq!(accessibility.label.as_deref(), Some("More details"));
    assert!(
        !context
            .world()
            .interaction(tooltip.stable_id())
            .unwrap()
            .focusable
    );

    context
        .set_pointer_hover_at(
            document,
            1,
            Some(button.stable_id()),
            Duration::from_millis(10),
        )
        .unwrap();
    assert!(
        context
            .next_animation_deadline()
            .is_some_and(|time| time <= Duration::from_millis(360))
    );
    context.advance_animations(Duration::from_millis(359));
    assert_eq!(
        context
            .world()
            .overlay_host(button.stable_id())
            .unwrap()
            .active,
        None
    );
    assert_eq!(
        context.world().overlay_host(button.stable_id()),
        Some(crate::OverlayHostState::default())
    );
    assert!(
        context
            .advance_animations(Duration::from_millis(360))
            .component_updates
            .contains(&button.stable_id())
    );
    assert_eq!(
        context
            .world()
            .overlay_host(button.stable_id())
            .unwrap()
            .active,
        Some(tooltip.stable_id())
    );
    assert!(context.read(button, |button| button.tooltip_open).unwrap());
}

#[test]
fn tooltip_default_follows_pointer_as_a_compact_card() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            IconButton::new(nana_ui_core::Icon::About, "Details").tooltip(
                "More details",
                nana_ui_core::TooltipConfig {
                    delay_ms: 0,
                    ..nana_ui_core::TooltipConfig::default()
                },
            ),
        )
        .unwrap();
    let tooltip = context.icon_button_tooltip(button).unwrap().unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(200.0, 120.0))
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        button.stable_id(),
        crate::LayoutBox {
            x: 20.0,
            y: 50.0,
            width: 28.0,
            height: 28.0,
        },
    );
    context.commit_mutations(layout).unwrap();

    context.set_pointer_location(document, 1, Some((48.0, 72.0)));
    context
        .set_pointer_hover_at(document, 1, Some(button.stable_id()), Duration::ZERO)
        .unwrap();
    assert_eq!(
        context
            .world()
            .overlay_host(button.stable_id())
            .unwrap()
            .active,
        Some(tooltip.stable_id())
    );

    let tooltip_style = context.world().node_style(tooltip.stable_id()).unwrap();
    assert_eq!(
        tooltip_style.layout.padding_left,
        Some(LengthSpec::Px(TooltipConfig::PADDING_X))
    );
    assert_eq!(
        tooltip_style.layout.padding_top,
        Some(LengthSpec::Px(TooltipConfig::PADDING_Y))
    );
    assert_eq!(
        tooltip_style.layout.border_radius,
        Some(TooltipConfig::RADIUS)
    );
    assert_eq!(
        tooltip_style.border,
        Some(nana_ui_core::SemanticColorRole::BorderSoft)
    );
    assert!(
        matches!(
            tooltip_style.layout.offset_left,
            Some(LengthSpec::Px(x)) if (x - 48.0).abs() < 0.01
        ),
        "default tooltip should bind to the pointer x, got {:?}",
        tooltip_style.layout.offset_left
    );
    assert!(
        matches!(
            tooltip_style.layout.offset_top,
            Some(LengthSpec::Px(y)) if y < 72.0 - TooltipConfig::PADDING_Y
        ),
        "tooltip should sit above the pointer, got {:?}",
        tooltip_style.layout.offset_top
    );
}

#[test]
fn loading_components_schedule_only_while_loading() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let switch = context
        .create_component(document, Switch::new("Sync", false).loading(true))
        .unwrap();
    let card = context
        .create_component(document, crate::Card::new().loading(true))
        .unwrap();
    assert_eq!(context.next_animation_deadline(), Some(Duration::ZERO));
    let frame = context.advance_animations(Duration::ZERO);
    assert_eq!(
        frame
            .component_updates
            .iter()
            .copied()
            .collect::<HashSet<_>>(),
        HashSet::from([switch.stable_id(), card.stable_id()])
    );
    assert_eq!(
        context.next_animation_deadline(),
        Some(COMPONENT_FRAME_INTERVAL)
    );
    context
        .update_component(switch, |switch, _| switch.loading = false)
        .unwrap();
    context
        .update_component(card, |card, _| card.loading = false)
        .unwrap();
    assert_eq!(context.next_animation_deadline(), None);
    let _ = context.compat_world_mut().take_animation_events();
    assert!(
        !context
            .advance_animations(Duration::from_secs(1))
            .has_updates()
    );
}

#[test]
fn workspace_transitions_schedule_only_while_transitioning() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let layout = nana_ui_core::WorkspaceLayout::new([
        nana_ui_core::RegionState::new(
            nana_ui_core::RegionId::Resources,
            nana_ui_core::RegionRole::Resources,
        )
        .size(240.0)
        .collapsible(true),
        nana_ui_core::RegionState::new(
            nana_ui_core::RegionId::Primary,
            nana_ui_core::RegionRole::Primary,
        )
        .fill_priority(1),
    ])
    .expect("workspace layout");
    let workspace = context
        .create_component(
            document,
            Workspace::from_model(&nana_ui_core::WorkspaceModel::with_layout(layout), []),
        )
        .unwrap();
    context
        .update_component(workspace, |workspace, _| {
            assert!(workspace.model.update(
                WorkspaceMutation::SetRegionCollapsed(nana_ui_core::RegionId::Resources, true,),
                Duration::ZERO,
            ));
        })
        .unwrap();
    // 过渡登记进帧调度，deadline 立即生效。
    assert_eq!(context.next_animation_deadline(), Some(Duration::ZERO));
    let frame = context.advance_animations(Duration::ZERO);
    assert!(frame.component_updates.contains(&workspace.stable_id()));
    assert_eq!(
        context.next_animation_deadline(),
        Some(COMPONENT_FRAME_INTERVAL)
    );
    // 过渡结束的帧：回收过渡并撤销帧调度。
    let frame = context.advance_animations(nana_ui_core::WORKSPACE_REGION_TRANSITION_DURATION);
    assert!(frame.component_updates.contains(&workspace.stable_id()));
    assert_eq!(context.next_animation_deadline(), None);
    assert!(
        !context
            .advance_animations(nana_ui_core::WORKSPACE_REGION_TRANSITION_DURATION)
            .has_updates()
    );
    assert_eq!(
        context
            .read(workspace, |workspace| workspace
                .model
                .region_extent(&nana_ui_core::RegionId::Resources))
            .unwrap(),
        0.0
    );
}

#[test]
fn list_item_slots_are_unique_direct_children_in_canonical_order() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let item = context
        .create_component(document, ListItem::new("fallback"))
        .unwrap();
    let leading = context.create_component(document, Text::new("L")).unwrap();
    let content = context.create_component(document, Text::new("C")).unwrap();
    let trailing = context.create_component(document, Text::new("T")).unwrap();
    context.append_child(item, content).unwrap();
    context.append_child(item, trailing).unwrap();
    context.append_child(item, leading).unwrap();
    let slots = ListItemSlots {
        leading: Some(leading.stable_id()),
        content: Some(content.stable_id()),
        trailing: Some(trailing.stable_id()),
    };
    assert!(context.set_list_item_slots(item, slots).unwrap());
    assert_eq!(
        context.world().node(item.stable_id()).unwrap().children,
        vec![
            leading.stable_id(),
            content.stable_id(),
            trailing.stable_id()
        ]
    );
    assert!(!context.set_list_item_slots(item, slots).unwrap());

    let duplicate = ListItemSlots {
        leading: Some(leading.stable_id()),
        content: Some(leading.stable_id()),
        trailing: Some(trailing.stable_id()),
    };
    assert!(matches!(
        context.set_list_item_slots(item, duplicate),
        Err(FrameworkError::InvalidListItemSlots {
            item: invalid,
            slot: None
        }) if invalid == item.stable_id()
    ));
}

#[test]
fn composite_geometry_separates_text_controls_and_range_drag_axis() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let sized = |width, height| {
        let mut style = NodeStyle::default();
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(width));
        layout.height = Some(LengthSpec::Px(height));
        style
    };
    let switch = context
        .create_component(
            document,
            Switch::new("Automatic updates", false)
                .hint("Runs in the background")
                .style(sized(380.0, 52.0)),
        )
        .unwrap();
    let range = context
        .create_component(
            document,
            RangeField::new(50.0, 0.0, 100.0, 1.0)
                .label("Volume")
                .unit("%")
                .style(sized(300.0, 58.0)),
        )
        .unwrap();
    let card = context
        .create_component(
            document,
            Card::new().title("Overview").padding(28.0).height(120.0),
        )
        .unwrap();
    let body = context
        .create_component(document, Text::new("Body"))
        .unwrap();
    context.append_child(card, body).unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(640.0, 480.0))
        .unwrap();

    let crate::ComponentGeometry::Switch {
        label,
        hint: Some(hint),
        control,
        ..
    } = context
        .world()
        .component_geometry(switch.stable_id())
        .unwrap()
    else {
        panic!("switch geometry must include label, hint, and control");
    };
    assert!(label.bounds.x + label.bounds.width <= control.x);
    assert!(label.bounds.y + label.bounds.height <= hint.bounds.y);

    let crate::ComponentGeometry::Card {
        title: Some(title),
        content,
        ..
    } = context
        .world()
        .component_geometry(card.stable_id())
        .unwrap()
    else {
        panic!("card geometry must include title and content");
    };
    assert!(title.bounds.y + title.bounds.height <= content.y);
    assert!(title.bounds.width >= content.width - 0.01);
    assert!(context.world().layout_box(body.stable_id()).unwrap().y >= content.y);
    let card_layout = context
        .world()
        .node_style(card.stable_id())
        .unwrap()
        .layout
        .as_ref();
    assert_eq!(card_layout.padding_top, Some(LengthSpec::Px(52.0)));
    assert_eq!(card_layout.padding_bottom, Some(LengthSpec::Px(28.0)));

    let crate::ComponentGeometry::Range { track, .. } = context
        .world()
        .component_geometry(range.stable_id())
        .unwrap()
    else {
        panic!("range geometry must expose the interaction axis");
    };
    assert!(track.x > context.world().layout_box(range.stable_id()).unwrap().x);
    context
        .begin_range_drag(document, 7, range.stable_id(), track.x)
        .unwrap();
    assert_eq!(context.read(range, |range| range.value).unwrap(), 0.0);
    context
        .update_range_drag(document, 7, track.x + track.width)
        .unwrap();
    assert_eq!(context.read(range, |range| range.value).unwrap(), 100.0);
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum RangeEvent {
    Input(f64),
    Changed(f64),
}

fn record_range_events(
    context: &mut AppContext,
    range: Entity<RangeField>,
) -> Arc<Mutex<Vec<RangeEvent>>> {
    let events = Arc::new(Mutex::new(Vec::new()));
    let input = Arc::clone(&events);
    context
        .on(range, move |_, event: &RangeInput, _| {
            input.lock().unwrap().push(RangeEvent::Input(event.value));
        })
        .unwrap();
    let changed = Arc::clone(&events);
    context
        .on(range, move |_, event: &RangeChanged, _| {
            changed
                .lock()
                .unwrap()
                .push(RangeEvent::Changed(event.value));
        })
        .unwrap();
    events
}

fn laid_out_range(
    context: &mut AppContext,
    document: DocumentId,
) -> (Entity<RangeField>, f32, f32) {
    let range = context
        .create_component(document, RangeField::new(0.0, 0.0, 100.0, 1.0))
        .unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        range.stable_id(),
        crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 32.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    let Some(crate::ComponentGeometry::Range { track, .. }) =
        context.world().component_geometry(range.stable_id())
    else {
        panic!("range geometry must expose the interaction axis");
    };
    (range, track.x, track.width)
}

#[test]
fn a_range_drag_previews_with_input_and_commits_once_on_release() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (range, x, width) = laid_out_range(&mut context, document);
    let events = record_range_events(&mut context, range);

    context
        .begin_range_drag(document, 7, range.stable_id(), x + width * 0.25)
        .unwrap();
    context
        .update_range_drag(document, 7, x + width * 0.5)
        .unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec![RangeEvent::Input(25.0), RangeEvent::Input(50.0)],
        "a drag in flight only previews"
    );
    assert!(context.end_range_drag(document, 7, false).unwrap());
    assert_eq!(
        events.lock().unwrap().last(),
        Some(&RangeEvent::Changed(50.0)),
        "release commits the dragged value once"
    );
    assert_eq!(context.world().pointer_capture(document, 7), None);

    events.lock().unwrap().clear();
    context
        .begin_range_drag(document, 8, range.stable_id(), x + width * 0.5)
        .unwrap();
    assert!(context.end_range_drag(document, 8, false).unwrap());
    assert!(
        events.lock().unwrap().is_empty(),
        "a press that does not move the value commits nothing"
    );
}

#[test]
fn a_cancelled_or_disabled_range_drag_restores_without_committing() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (range, x, width) = laid_out_range(&mut context, document);
    let events = record_range_events(&mut context, range);

    context
        .begin_range_drag(document, 7, range.stable_id(), x + width * 0.75)
        .unwrap();
    assert!(context.end_range_drag(document, 7, true).unwrap());
    assert_eq!(
        *events.lock().unwrap(),
        vec![RangeEvent::Input(75.0), RangeEvent::Input(0.0)]
    );
    assert_eq!(context.read(range, |range| range.value).unwrap(), 0.0);

    events.lock().unwrap().clear();
    context
        .begin_range_drag(document, 8, range.stable_id(), x + width * 0.75)
        .unwrap();
    context
        .update_component(range, |range, _| range.disabled = true)
        .unwrap();
    assert!(context.end_range_drag(document, 8, false).unwrap());
    assert_eq!(
        *events.lock().unwrap(),
        vec![RangeEvent::Input(75.0), RangeEvent::Input(0.0)],
        "a field disabled mid-drag must not commit the stale drag"
    );
    assert_eq!(context.read(range, |range| range.value).unwrap(), 0.0);
}

#[test]
fn a_keyboard_step_commits_and_becomes_the_drag_cancel_target() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let (range, x, width) = laid_out_range(&mut context, document);
    let events = record_range_events(&mut context, range);

    assert!(
        context
            .adjust_range(range, RangeAdjustment::PageIncrement)
            .unwrap()
    );
    let stepped = context.read(range, |range| range.value).unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec![RangeEvent::Input(stepped), RangeEvent::Changed(stepped)]
    );

    events.lock().unwrap().clear();
    context
        .begin_range_drag(document, 7, range.stable_id(), x + width * 0.75)
        .unwrap();
    assert!(
        context
            .adjust_range(range, RangeAdjustment::Minimum)
            .unwrap()
    );
    assert!(context.end_range_drag(document, 7, true).unwrap());
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            RangeEvent::Input(75.0),
            RangeEvent::Input(0.0),
            RangeEvent::Changed(0.0),
        ],
        "cancel keeps a value committed during the drag"
    );
    assert_eq!(context.read(range, |range| range.value).unwrap(), 0.0);
}

#[test]
fn a_focused_range_keeps_its_rail_interaction_free() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let range = context
        .create_component(
            document,
            RangeField::new(25.0, 0.0, 100.0, 1.0)
                .label("Volume")
                .unit("%"),
        )
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(640.0, 480.0))
        .unwrap();
    context.focus_node(document, range.stable_id()).unwrap();
    let work = context.compat_world_mut().take_system_work();
    context
        .compat_world_mut()
        .resolve_styles(&work.style)
        .unwrap();
    let focused = context
        .world()
        .extract_nodes(&[range.stable_id()])
        .pop()
        .unwrap();
    // The rail paints with the resolved border colour, so focus must not
    // touch it: the thumb carries the focus ring instead (LiliaUI).
    assert_eq!(
        focused.style.border_color,
        Some(
            nana_ui_core::SemanticPalette::dark()
                .border_strong
                .as_rgba_array()
        ),
    );
}

#[test]
fn component_size_kind_and_fallback_geometry_preserve_design_contracts() {
    for size in [
        nana_ui_core::ControlSize::Small,
        nana_ui_core::ControlSize::Medium,
        nana_ui_core::ControlSize::Large,
    ] {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let switch = context
            .create_component(
                document,
                Switch::new("Automatic updates", false)
                    .hint("Runs in the background")
                    .size(size),
            )
            .unwrap();
        let range = context
            .create_component(
                document,
                RangeField::new(0.7, 0.0, 1.0, 0.1)
                    .label("Opacity")
                    .unit("%")
                    .size(size),
            )
            .unwrap();
        context
            .layout_document(document, crate::LayoutViewport::new(380.0, 120.0))
            .unwrap();
        assert_eq!(
            context
                .world()
                .layout_box(switch.stable_id())
                .unwrap()
                .width,
            380.0
        );
        let crate::ComponentGeometry::Switch { label, control, .. } = context
            .world()
            .component_geometry(switch.stable_id())
            .unwrap()
        else {
            panic!("switch geometry expected");
        };
        assert_eq!(label.font_size, size.text_size());
        assert!(label.bounds.x + label.bounds.width <= control.x);
        let switch_interaction = context
            .world()
            .node_style(switch.stable_id())
            .unwrap()
            .interaction;
        assert_ne!(switch_interaction.hovered, switch_interaction.pressed);
        assert_ne!(switch_interaction.pressed, switch_interaction.focused);
        let crate::ComponentGeometry::Range {
            label: Some(label),
            value,
            unit: Some(unit),
            track,
        } = context
            .world()
            .component_geometry(range.stable_id())
            .unwrap()
        else {
            panic!("range geometry expected");
        };
        assert_eq!(label.font_size, size.text_size());
        assert!(label.bounds.x + label.bounds.width <= track.x);
        assert!(track.x + track.width <= value.bounds.x);
        assert!(value.bounds.x + value.bounds.width <= unit.bounds.x + 0.01);
        assert_eq!(
            context.world().standard_visual(range.stable_id()),
            Some(StandardVisual::Range {
                label: Some(Arc::from("Opacity")),
                value: Arc::from("0.7"),
                unit: Some(Arc::from("%")),
                size,
                ratio: 0.7,
                invalid: false,
            })
        );
    }

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    for kind in [
        nana_ui_core::CardKind::Surface,
        nana_ui_core::CardKind::Outlined,
        nana_ui_core::CardKind::Raised,
        nana_ui_core::CardKind::Flat,
        nana_ui_core::CardKind::Selected,
    ] {
        let card = context
            .create_component(document, Card::new().kind(kind))
            .unwrap();
        let (background, border, border_width) = {
            let style = context.world().node_style(card.stable_id()).unwrap();
            (style.background, style.border, style.layout.border_width)
        };
        context
            .layout_document(document, crate::LayoutViewport::new(240.0, 120.0))
            .unwrap();
        let crate::ComponentGeometry::Card { elevation, .. } = context
            .world()
            .component_geometry(card.stable_id())
            .unwrap()
        else {
            panic!("card geometry expected");
        };
        assert_eq!(
            elevation,
            (kind == nana_ui_core::CardKind::Raised).then_some(
                crate::ComponentElevation::surface_shadow(nana_ui_core::ThemeMode::Dark)
            )
        );
        assert_eq!(
            background,
            match kind {
                nana_ui_core::CardKind::Surface | nana_ui_core::CardKind::Raised => {
                    Some(nana_ui_core::SemanticColorRole::Surface)
                }
                nana_ui_core::CardKind::Selected => {
                    Some(nana_ui_core::SemanticColorRole::Selected)
                }
                nana_ui_core::CardKind::Outlined | nana_ui_core::CardKind::Flat => None,
            }
        );
        assert_eq!(
            (border, border_width),
            match kind {
                nana_ui_core::CardKind::Outlined => {
                    (Some(nana_ui_core::SemanticColorRole::Border), Some(1.0))
                }
                nana_ui_core::CardKind::Selected => {
                    (Some(nana_ui_core::SemanticColorRole::BorderSoft), Some(1.0))
                }
                _ => (None, Some(0.0)),
            }
        );
    }

    let item = context
        .create_component(document, ListItem::new("Camera"))
        .unwrap();
    let item_interaction = context
        .world()
        .node_style(item.stable_id())
        .unwrap()
        .interaction;
    assert_ne!(item_interaction.selected, item_interaction.selected_hovered);
    let leading = context.create_component(document, Text::new("L")).unwrap();
    let trailing = context.create_component(document, Text::new("T")).unwrap();
    context.append_child(item, leading).unwrap();
    context.append_child(item, trailing).unwrap();
    context
        .set_list_item_slots(
            item,
            ListItemSlots {
                leading: Some(leading.stable_id()),
                content: None,
                trailing: Some(trailing.stable_id()),
            },
        )
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(240.0, 120.0))
        .unwrap();
    let crate::ComponentGeometry::ListItem {
        leading: Some(leading),
        content: Some(content),
        trailing: Some(trailing),
        detail: None,
    } = context
        .world()
        .component_geometry(item.stable_id())
        .unwrap()
    else {
        panic!("list item fallback geometry expected");
    };
    assert!(leading.x + leading.width <= content.x);
    assert!(content.x + content.width <= trailing.x);

    let disabled_range = context
        .create_component(
            document,
            RangeField::new(0.5, 0.0, 1.0, 0.1)
                .label("Opacity")
                .disabled(true),
        )
        .unwrap();
    assert_eq!(
        context
            .world()
            .node_style(disabled_range.stable_id())
            .unwrap()
            .interaction
            .disabled
            .foreground,
        Some(nana_ui_core::SemanticColorRole::Muted)
    );
}

#[test]
fn range_field_can_hide_the_value_readout_and_still_expose_the_numeric_value() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(300.0));
        layout.height = Some(LengthSpec::Px(32.0));
    }
    let range = context
        .create_component(
            document,
            RangeField::new(40.0, 0.0, 100.0, 1.0)
                .label("Seek")
                .show_value(false)
                .style(style),
        )
        .unwrap();
    context
        .layout_document(document, crate::LayoutViewport::new(640.0, 480.0))
        .unwrap();
    let bounds = context.world().layout_box(range.stable_id()).unwrap();
    let crate::ComponentGeometry::Range {
        value, unit, track, ..
    } = context
        .world()
        .component_geometry(range.stable_id())
        .unwrap()
    else {
        panic!("range geometry expected");
    };
    assert!(value.content.is_empty());
    assert!(unit.is_none());
    assert!(
        track.width > bounds.width * 0.5,
        "track={track:?} bounds={bounds:?}"
    );
    assert_eq!(
        context.world().standard_visual(range.stable_id()),
        Some(StandardVisual::Range {
            label: Some(Arc::from("Seek")),
            value: Arc::from(""),
            unit: None,
            size: nana_ui_core::ControlSize::Medium,
            ratio: 0.4,
            invalid: false,
        })
    );
    let accessibility = context.world().project_accessibility(document);
    let slider = accessibility
        .iter()
        .find(|node| node.id == range.stable_id())
        .unwrap();
    assert_eq!(slider.numeric_value, Some(40.0));
}

#[test]
fn observer_view_receives_source_event_and_owns_nested_events() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let source = context
        .create_view(document, NodeKind::Text, Counter { value: 0 })
        .unwrap();
    let observer = context
        .create_view(document, NodeKind::Text, Counter { value: 0 })
        .unwrap();
    context
        .observe(source, observer, |view, event: &Increment, cx| {
            view.value += event.0;
            cx.emit(Cascade);
        })
        .unwrap();
    context
        .on(observer, |view, _event: &Cascade, _cx| view.value += 1)
        .unwrap();
    context
        .update(source, |_view, cx| cx.emit(Increment(4)))
        .unwrap();
    assert_eq!(context.read(observer, |view| view.value).unwrap(), 5);
}

#[test]
fn action_context_extension_and_view_removal_have_explicit_ownership() {
    struct TestExtension {
        installed: Arc<std::sync::atomic::AtomicUsize>,
    }
    impl UiExtension for TestExtension {
        fn name(&self) -> &'static str {
            "test.extension"
        }

        fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
            let installed = Arc::clone(&self.installed);
            registrar.register_action(
                "counter.increment",
                ContextPredicate::always().all_of(["editor"]),
                move |_| {
                    installed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Ok(())
                },
            )
        }
    }

    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let extension = TestExtension {
        installed: Arc::clone(&count),
    };
    let mut context = AppContext::new();
    context.install(&extension).unwrap();
    assert_eq!(
        context.install(&extension),
        Err(FrameworkError::DuplicateExtension("test.extension".into()))
    );
    let action = ActionId::new("counter.increment");
    assert_eq!(
        context.dispatch_action(&action, &KeyContext::default()),
        Err(FrameworkError::ActionUnavailable(action.clone()))
    );
    context
        .dispatch_action(&action, &KeyContext::new(["editor"]))
        .unwrap();
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 1);

    let entity = context
        .create_view(
            DocumentId::new(1).unwrap(),
            NodeKind::Document,
            Counter { value: 9 },
        )
        .unwrap();
    let child = context
        .create_view(
            DocumentId::new(1).unwrap(),
            NodeKind::Text,
            Counter { value: 3 },
        )
        .unwrap();
    context
        .update(entity, |_, cx| {
            cx.mutations()
                .insert(entity.stable_id(), child.stable_id(), None);
        })
        .unwrap();
    let removed = context.remove_view(entity).unwrap();
    assert_eq!(removed.value, 9);
    assert!(!context.world().contains(entity.stable_id()));
    assert_eq!(
        context.read(child, |view| view.value),
        Err(FrameworkError::MissingView(child.stable_id()))
    );
}

#[test]
fn recursive_events_are_bounded_per_update() {
    let mut context = AppContext::new();
    let entity = context
        .create_view(
            DocumentId::new(1).unwrap(),
            NodeKind::Document,
            Counter { value: 0 },
        )
        .unwrap();
    context
        .on(entity, |_view, _event: &Cascade, cx| cx.emit(Cascade))
        .unwrap();
    assert_eq!(
        context.update(entity, |_view, cx| cx.emit(Cascade)),
        Err(FrameworkError::EventOverflow(entity.stable_id()))
    );
    assert_eq!(context.read(entity, |view| view.value).unwrap(), 0);
}

#[test]
fn extension_registration_is_atomic_on_conflict() {
    struct Conflict;
    impl UiExtension for Conflict {
        fn name(&self) -> &'static str {
            "conflict.extension"
        }

        fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
            registrar.register_action("unique.action", ContextPredicate::always(), |_| Ok(()))?;
            registrar.register_action("existing.action", ContextPredicate::always(), |_| Ok(()))
        }
    }

    let mut context = AppContext::new();
    context
        .register_action("existing.action", ContextPredicate::always(), |_| Ok(()))
        .unwrap();
    assert_eq!(
        context.install(&Conflict),
        Err(FrameworkError::DuplicateAction(ActionId::new(
            "existing.action"
        )))
    );
    assert_eq!(
        context.dispatch_action(&ActionId::new("unique.action"), &KeyContext::default()),
        Err(FrameworkError::MissingAction(ActionId::new(
            "unique.action"
        )))
    );
}

#[test]
fn presenter_extension_installs_onto_the_world() {
    struct Keyword;
    impl crate::TextPresenter for Keyword {
        fn name(&self) -> &'static str {
            "keyword"
        }

        fn present(&self, text: &str, _request: &crate::HighlightRequest) -> Vec<crate::TextSpan> {
            text.match_indices("fn")
                .map(|(start, token)| crate::TextSpan {
                    start,
                    end: start + token.len(),
                    color: nana_ui_core::SemanticColorRole::Accent,
                })
                .collect()
        }
    }
    struct HighlightExt;
    impl UiExtension for HighlightExt {
        fn name(&self) -> &'static str {
            "test.highlight"
        }

        fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
            registrar.register_presenter(Box::new(Keyword))
        }
    }

    let mut context = AppContext::new();
    context.install(&HighlightExt).unwrap();
    assert_eq!(
        context.install(&HighlightExt),
        Err(FrameworkError::DuplicateExtension("test.highlight".into()))
    );
    assert_eq!(
        context.register_presenter(Box::new(Keyword)),
        Err(FrameworkError::World(UiWorldError::DuplicatePresenter(
            "keyword".into()
        )))
    );
    let mut request = crate::HighlightRequest::highlight("rs");
    request.presenter = Arc::from("keyword");
    let entity = context
        .create_component(
            DocumentId::new(1).unwrap(),
            TextArea {
                highlight: Some(request),
                ..TextArea::new("fn main")
            },
        )
        .unwrap();
    assert_eq!(
        context
            .world()
            .highlight_request(entity.stable_id())
            .map(|request| request.language.as_ref()),
        Some("rs")
    );
    context
        .resolve_presentations(&[entity.stable_id()])
        .unwrap();
    assert_eq!(
        context
            .world()
            .text_presentation(entity.stable_id())
            .map(|presentation| presentation.spans.len()),
        Some(1)
    );
}

#[test]
fn builtin_and_plugin_components_share_one_registry() {
    let mut context = AppContext::new();
    assert!(context.resolve_component_tag("button").is_some());
    assert_eq!(
        context
            .resolve_component_tag("nana-gpu")
            .map(ComponentTypeId::as_str),
        Some("nana.gpu")
    );
    assert_eq!(
        context
            .resolve_component_tag("gpu-view")
            .map(ComponentTypeId::as_str),
        Some("nana.gpu-view")
    );
    assert_eq!(
        context
            .resolve_component_tag("chip")
            .map(ComponentTypeId::as_str),
        Some("nana.chip")
    );
    assert_eq!(
        context.resolve_component_tag("virtual-list"),
        None,
        "virtual windows use scroll-view, not a second type"
    );
    assert_eq!(
        context
            .resolve_component_tag("nana-button")
            .map(ComponentTypeId::as_str),
        Some("nana.button")
    );
    assert_eq!(
        context
            .resolve_component_tag("select")
            .map(ComponentTypeId::as_str),
        Some("nana.select")
    );
    assert_eq!(
        context
            .resolve_component_tag("nana-select")
            .map(ComponentTypeId::as_str),
        Some("nana.select")
    );
    assert_eq!(
        context
            .resolve_component_tag("tabs")
            .map(ComponentTypeId::as_str),
        Some("nana.tabs")
    );
    assert_eq!(
        context
            .resolve_component_tag("dock")
            .map(ComponentTypeId::as_str),
        Some("nana.dock")
    );
    assert_eq!(
        context
            .resolve_component_tag("form-field")
            .map(ComponentTypeId::as_str),
        Some("nana.form-field")
    );
    assert_eq!(
        context
            .resolve_component_tag("nana-form-field")
            .map(ComponentTypeId::as_str),
        Some("nana.form-field")
    );
    assert!(
        context.resolve_component_tag("form").is_none(),
        "HTML form stays a layout box; nana-form-field owns form-field"
    );
    assert!(
        context.resolve_component_tag("search").is_none(),
        "HTML search is a landmark; SearchDropdown owns search-dropdown"
    );
    assert_eq!(
        context
            .resolve_component_tag("search-dropdown")
            .map(ComponentTypeId::as_str),
        Some("nana.search-dropdown")
    );
    assert_eq!(
        context
            .resolve_component_tag("nana-search-dropdown")
            .map(ComponentTypeId::as_str),
        Some("nana.search-dropdown")
    );

    #[derive(Clone, PartialEq)]
    struct ProbeCard {
        title: String,
    }
    impl ComponentView for ProbeCard {
        fn node_kind(&self) -> NodeKind {
            NodeKind::Element {
                tag: "probe-card".into(),
            }
        }
        fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
            if world.text(id) != Some(self.title.as_str()) {
                mutations.set_text(
                    id,
                    crate::TextContent {
                        value: self.title.clone().into(),
                    },
                );
            }
        }
    }
    impl crate::RegisterableComponent for ProbeCard {
        const TYPE_ID: &'static str = "test.probe-card";
        const TAGS: &'static [&'static str] = &["nana-probe-card", "probe-card"];
        fn from_semantic(spec: &crate::SemanticSpec<'_>) -> Self {
            Self {
                title: spec
                    .attr("handle")
                    .unwrap_or_else(|| spec.display_label())
                    .to_owned(),
            }
        }
    }
    struct ProbePlugin;
    impl UiExtension for ProbePlugin {
        fn name(&self) -> &'static str {
            "test.probe"
        }
        fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
            registrar.register_component::<ProbeCard>()
        }
    }

    context.install(&ProbePlugin).unwrap();
    assert_eq!(
        context
            .resolve_component_tag("nana-probe-card")
            .map(ComponentTypeId::as_str),
        Some("test.probe-card")
    );

    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, Button::new("Save"))
        .unwrap();
    assert_eq!(
        context
            .world()
            .component_type(button.stable_id())
            .map(ComponentTypeId::as_str),
        Some("nana.button")
    );
    let select_type = context.resolve_component_tag("select").unwrap().clone();
    let select_layout = std::sync::Arc::new(nana_ui_core::LayoutStyle::default());
    let select_spec = crate::SemanticSpec::from_parts(&select_type, &select_layout);
    let select = context
        .create_component(document, Select::from_semantic(&select_spec))
        .unwrap();
    assert_eq!(
        context
            .world()
            .component_type(select.stable_id())
            .map(ComponentTypeId::as_str),
        Some("nana.select")
    );
    let dock = context
        .create_component(
            document,
            crate::Dock::new(crate::DockNode::item("dock", None)),
        )
        .unwrap();
    assert_eq!(
        context
            .world()
            .component_type(dock.stable_id())
            .map(ComponentTypeId::as_str),
        Some("nana.dock")
    );

    let id = StableNodeId::new(42).unwrap();
    let mut queue = MutationQueue::new();
    queue.create(
        id,
        document,
        NodeKind::Element {
            tag: "probe-card".into(),
        },
    );
    context.commit_mutations(queue).unwrap();
    let type_id = context.resolve_component_tag("probe-card").unwrap().clone();
    let layout = std::sync::Arc::new(nana_ui_core::LayoutStyle::default());
    let spec = crate::SemanticSpec {
        label: "User",
        ..crate::SemanticSpec::from_parts(&type_id, &layout)
    };
    let mut mutations = MutationQueue::new();
    assert_eq!(
        context.bind_semantic(id, &spec, &mut mutations).unwrap(),
        crate::ComponentBindKind::Projected
    );
    context.commit_mutations(mutations).unwrap();
    assert_eq!(context.world().text(id), Some("User"));
    assert_eq!(
        context
            .world()
            .component_type(id)
            .map(ComponentTypeId::as_str),
        Some("test.probe-card")
    );
}

/// Documented containers and chrome must carry a type identity, or Vue tag
/// resolution and devtools cannot name the node.
#[test]
#[cfg(feature = "charts")]
fn documented_containers_and_chrome_carry_a_type_identity() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    for (tag, type_id) in [
        ("list", "nana.list"),
        ("scroll-view", "nana.scroll-view"),
        ("table", "nana.table"),
        ("tr", "nana.table-row"),
        ("td", "nana.table-cell"),
        ("reorder-list", "nana.reorder-list"),
        ("time-series-chart", "nana.time-series-chart"),
        ("desktop-shell", "nana.desktop-shell"),
        ("app-title-bar", "nana.app-title-bar"),
        ("pane-chrome", "nana.pane-chrome"),
        ("sidebar-section", "nana.sidebar-section"),
        ("sidebar-footer", "nana.sidebar-footer"),
        (
            "settings-collapsible-card",
            "nana.settings-collapsible-card",
        ),
    ] {
        assert_eq!(
            context
                .resolve_component_tag(tag)
                .map(ComponentTypeId::as_str),
            Some(type_id),
            "tag `{tag}` must resolve"
        );
    }
    assert!(
        context.resolve_component_tag("scroll").is_none(),
        "aliases are pruned; scroll-view keeps the single tag"
    );

    let list = context
        .create_component(document, crate::List::new())
        .unwrap();
    assert_eq!(
        context
            .world()
            .component_type(list.stable_id())
            .map(ComponentTypeId::as_str),
        Some("nana.list")
    );
    let section = context
        .create_component(document, crate::SidebarSection::new("Files"))
        .unwrap();
    assert_eq!(
        context
            .world()
            .component_type(section.stable_id())
            .map(ComponentTypeId::as_str),
        Some("nana.sidebar-section")
    );
    let chart = context
        .create_component(document, crate::TimeSeriesChart::new([1.0, 2.0]))
        .unwrap();
    assert_eq!(
        context
            .world()
            .component_type(chart.stable_id())
            .map(ComponentTypeId::as_str),
        Some("nana.time-series-chart")
    );
}

#[test]
fn plugin_component_registration_is_atomic_on_conflict() {
    #[derive(Clone, PartialEq)]
    struct StealButton;
    impl ComponentView for StealButton {
        fn node_kind(&self) -> NodeKind {
            NodeKind::Element {
                tag: "button".into(),
            }
        }
        fn project(&self, _id: StableNodeId, _world: &UiWorld, _mutations: &mut MutationQueue) {}
    }
    impl crate::RegisterableComponent for StealButton {
        const TYPE_ID: &'static str = "nana.button";
        const TAGS: &'static [&'static str] = &["stolen"];
        fn from_semantic(_spec: &crate::SemanticSpec<'_>) -> Self {
            Self
        }
    }
    struct Conflict;
    impl UiExtension for Conflict {
        fn name(&self) -> &'static str {
            "conflict.components"
        }
        fn install(&self, registrar: &mut ExtensionRegistrar) -> Result<(), FrameworkError> {
            registrar.register_component::<StealButton>()
        }
    }

    let mut context = AppContext::new();
    assert_eq!(
        context.install(&Conflict),
        Err(FrameworkError::DuplicateComponentType("nana.button".into()))
    );
    assert!(context.resolve_component_tag("stolen").is_none());
    assert!(context.resolve_component_tag("button").is_some());
}

#[cfg(feature = "syntax-highlighting")]
#[test]
fn new_context_installs_the_official_highlight_presenter() {
    let mut context = AppContext::new();
    assert!(context.world().has_presenter(crate::HIGHLIGHT_PRESENTER));
    let entity = context
        .create_component(
            DocumentId::new(1).unwrap(),
            crate::HostedTextarea::new("fn main() {}", "rs"),
        )
        .unwrap();
    assert_eq!(
        context
            .world()
            .highlight_request(entity.stable_id())
            .map(|request| request.presenter.as_ref()),
        Some(crate::HIGHLIGHT_PRESENTER)
    );
    context
        .resolve_presentations(&[entity.stable_id()])
        .unwrap();
    let spans = context
        .world()
        .text_presentation(entity.stable_id())
        .map(|presentation| presentation.spans.clone())
        .unwrap_or_default();
    assert!(
        spans.iter().any(|span| {
            matches!(
                span.color,
                nana_ui_core::SemanticColorRole::Accent
                    | nana_ui_core::SemanticColorRole::AccentStrong
            ) && &"fn main() {}"[span.start..span.end] == "fn"
        }),
        "default Syntect presenter must color rust `fn`, got {spans:?}"
    );
}

struct One<T>(Option<T>);

impl<T: Unpin> Stream for One<T> {
    type Item = T;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<T>> {
        Poll::Ready(self.0.take())
    }
}

#[test]
fn task_and_subscription_preserve_host_owned_async_work() {
    let waker = Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut future = Task::ready(2).map(|value| value + 1).into_future();
    assert_eq!(future.as_mut().poll(&mut cx), Poll::Ready(3));

    let subscription = Subscription::new("window.events", One(Some(7)));
    assert_eq!(subscription.id(), "window.events");
    let mut stream = subscription.into_stream();
    assert_eq!(stream.as_mut().poll_next(&mut cx), Poll::Ready(Some(7)));
    assert_eq!(stream.as_mut().poll_next(&mut cx), Poll::Ready(None));
}

#[test]
fn virtual_list_materializes_only_visible_items_and_reuses_overlap() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let list = context.create_component(document, List::new()).unwrap();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 10_000));
    let mut items = VirtualListItems::<usize, Text>::default();

    let first = context
        .materialize_virtual_list(
            list,
            &mut items,
            &layout,
            0.0,
            100.0,
            20.0,
            |index| index,
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    assert!(first.range.len() < 10);
    assert_eq!(
        context
            .world()
            .node(list.stable_id())
            .unwrap()
            .children
            .len(),
        first.range.len()
    );
    let overlap_key = first.range.end - 1;
    let overlap_entity = items.entity(&overlap_key).unwrap();
    let removed_key = first.range.start;
    let removed_entity = items.entity(&removed_key).unwrap();

    let next = context
        .materialize_virtual_list(
            list,
            &mut items,
            &layout,
            80.0,
            100.0,
            20.0,
            |index| index,
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    assert!(next.range.contains(&overlap_key));
    assert_eq!(items.entity(&overlap_key), Some(overlap_entity));
    assert!(!context.world().contains(removed_entity.stable_id()));
    assert_eq!(items.mounted_keys(), next.range.clone().collect::<Vec<_>>());
    let generation = context.world().generation();

    context
        .materialize_virtual_list(
            list,
            &mut items,
            &layout,
            80.0,
            100.0,
            20.0,
            |index| index,
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    assert_eq!(context.world().generation(), generation);
}

#[test]
fn virtual_list_rejects_foreign_item_ownership_without_mutating() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let list = context.create_component(document, List::new()).unwrap();
    let other_list = context.create_component(document, List::new()).unwrap();
    let layout = VirtualListLayout::new(std::iter::repeat_n(20.0, 100));
    let mut items = VirtualListItems::<usize, Text>::default();

    let first = context
        .materialize_virtual_list(
            list,
            &mut items,
            &layout,
            0.0,
            100.0,
            0.0,
            |index| index,
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    let moved = items.entity(&first.range.start).unwrap();
    context.append_child(other_list, moved).unwrap();
    let generation = context.world().generation();

    assert_eq!(
        context.materialize_virtual_list(
            list,
            &mut items,
            &layout,
            200.0,
            100.0,
            0.0,
            |index| index,
            |index, _| Text::new(format!("row {index}")),
        ),
        Err(FrameworkError::InvalidVirtualization)
    );
    assert_eq!(context.world().generation(), generation);
    assert_eq!(
        context.world().node(moved.stable_id()).unwrap().parent,
        Some(other_list.stable_id())
    );
    assert!(context.world().contains(moved.stable_id()));
}

#[test]
fn virtual_table_materializes_a_bounded_grid_and_reuses_both_axes() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let table = context.create_component(document, Table::new()).unwrap();
    let layout = VirtualTableLayout::new(
        std::iter::repeat_n(20.0, 10_000),
        (0..100).map(|index| nana_ui_core::TableColumn::new(index.to_string(), 80.0)),
    );
    let mut items = VirtualTableItems::<usize, usize>::default();

    let first = context
        .materialize_virtual_table(
            table,
            &mut items,
            &layout,
            (0.0, 0.0),
            (160.0, 100.0),
            (0.0, 20.0),
            |index| index,
            |index| index,
            |_index, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}:{column}")),
        )
        .unwrap();
    assert!(first.rows.range.len() < 10);
    assert!(first.columns.range.len() < 5);
    let overlap_row = first.rows.range.end - 1;
    let overlap_column = first.columns.range.end - 1;
    let overlap_row_entity = items.row_entity(&overlap_row).unwrap();
    let overlap_cell_entity = items.cell_entity(&overlap_row, &overlap_column).unwrap();
    let removed_row = first.rows.range.start;
    let removed_cell = items
        .cell_entity(&removed_row, &first.columns.range.start)
        .unwrap();

    let next = context
        .materialize_virtual_table(
            table,
            &mut items,
            &layout,
            (80.0, 40.0),
            (160.0, 100.0),
            (0.0, 20.0),
            |index| index,
            |index| index,
            |_index, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}:{column}")),
        )
        .unwrap();
    assert_eq!(items.row_entity(&overlap_row), Some(overlap_row_entity));
    assert_eq!(
        items.cell_entity(&overlap_row, &overlap_column),
        Some(overlap_cell_entity)
    );
    assert!(!context.world().contains(removed_cell.stable_id()));
    assert_eq!(
        items.mounted_rows(),
        next.rows.range.clone().collect::<Vec<_>>()
    );
    assert_eq!(
        items.mounted_columns(),
        next.columns.range.clone().collect::<Vec<_>>()
    );
    let retained_cells = next.rows.range.len() * next.columns.range.len();
    assert_eq!(
        next.rows
            .range
            .clone()
            .map(|row| context
                .world()
                .node(items.row_entity(&row).unwrap().stable_id())
                .unwrap()
                .children
                .len())
            .sum::<usize>(),
        retained_cells
    );
    let generation = context.world().generation();
    context
        .materialize_virtual_table(
            table,
            &mut items,
            &layout,
            (80.0, 40.0),
            (160.0, 100.0),
            (0.0, 20.0),
            |index| index,
            |index| index,
            |_index, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}:{column}")),
        )
        .unwrap();
    assert_eq!(context.world().generation(), generation);
}

#[test]
fn virtual_table_rejects_foreign_row_ownership_without_mutating() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let table = context.create_component(document, Table::new()).unwrap();
    let other_table = context.create_component(document, Table::new()).unwrap();
    let layout = VirtualTableLayout::new(
        std::iter::repeat_n(20.0, 100),
        (0..10).map(|index| nana_ui_core::TableColumn::new(index.to_string(), 80.0)),
    );
    let mut items = VirtualTableItems::<usize, usize>::default();
    let window = context
        .materialize_virtual_table(
            table,
            &mut items,
            &layout,
            (0.0, 0.0),
            (160.0, 100.0),
            (0.0, 0.0),
            |index| index,
            |index| index,
            |_index, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}:{column}")),
        )
        .unwrap();
    let moved = items.row_entity(&window.rows.range.start).unwrap();
    context.append_child(other_table, moved).unwrap();
    let generation = context.world().generation();

    assert_eq!(
        context.materialize_virtual_table(
            table,
            &mut items,
            &layout,
            (0.0, 200.0),
            (160.0, 100.0),
            (0.0, 0.0),
            |index| index,
            |index| index,
            |_index, _| TableRow::new(),
            |row, _, column, _| TableCell::new(format!("{row}:{column}")),
        ),
        Err(FrameworkError::InvalidVirtualization)
    );
    assert_eq!(context.world().generation(), generation);
    assert_eq!(
        context.world().node(moved.stable_id()).unwrap().parent,
        Some(other_table.stable_id())
    );
}

#[test]
fn virtual_tree_materializes_only_visible_rows_and_reuses_overlap_on_scroll_and_expand() {
    const ROW: f32 = 20.0;
    const VIEWPORT: f32 = 100.0;
    const OVERSCAN: f32 = 20.0;
    let cap = VirtualListLayout::uniform_window_item_cap(VIEWPORT, OVERSCAN, ROW);
    assert!(cap < 10_000);

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let tree = context.create_component(document, List::new()).unwrap();
    let mut keys = (0..10_000).collect::<Vec<_>>();
    let mut layout = VirtualTreeLayout::uniform(ROW, std::iter::repeat_n(0, keys.len()));
    let mut items = VirtualTreeItems::<usize, Text>::default();

    let first = context
        .materialize_virtual_tree(
            tree,
            &mut items,
            &layout,
            0.0,
            VIEWPORT,
            OVERSCAN,
            |index| keys[index],
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    assert!(first.range.len() <= cap);
    assert_eq!(
        context
            .world()
            .node(tree.stable_id())
            .unwrap()
            .children
            .len(),
        first.range.len()
    );
    let overlap_key = keys[first.range.end - 1];
    let overlap_entity = items.entity(&overlap_key).unwrap();
    let removed_key = keys[first.range.start];
    let removed_entity = items.entity(&removed_key).unwrap();

    let next = context
        .materialize_virtual_tree(
            tree,
            &mut items,
            &layout,
            80.0,
            VIEWPORT,
            OVERSCAN,
            |index| keys[index],
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    assert!(next.range.len() <= cap);
    assert!(items.mounted_keys().contains(&overlap_key));
    assert_eq!(items.entity(&overlap_key), Some(overlap_entity));
    assert!(!context.world().contains(removed_entity.stable_id()));
    assert_eq!(
        items.mounted_keys(),
        next.range
            .clone()
            .map(|index| keys[index])
            .collect::<Vec<_>>()
    );

    let parent = keys.iter().position(|key| *key == overlap_key).unwrap();
    let child_keys = [1_000_000usize, 1_000_001];
    assert!(layout.expand(
        parent,
        child_keys.map(|_| nana_ui_core::VirtualTreeRow {
            extent: ROW,
            descendant_count: 0,
        })
    ));
    keys.splice(parent + 1..parent + 1, child_keys);
    let expanded = context
        .materialize_virtual_tree(
            tree,
            &mut items,
            &layout,
            80.0,
            VIEWPORT,
            OVERSCAN,
            |index| keys[index],
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    assert!(expanded.range.len() <= cap);
    assert_eq!(items.entity(&overlap_key), Some(overlap_entity));
    assert!(
        context
            .world()
            .node(tree.stable_id())
            .unwrap()
            .children
            .len()
            <= cap
    );
    assert!(items.entity(&child_keys[0]).is_some());
    let generation = context.world().generation();
    context
        .materialize_virtual_tree(
            tree,
            &mut items,
            &layout,
            80.0,
            VIEWPORT,
            OVERSCAN,
            |index| keys[index],
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    assert_eq!(context.world().generation(), generation);
}

#[test]
fn virtual_tree_expand_keeps_live_children_below_geometric_cap() {
    const ROW: f32 = 20.0;
    const VIEWPORT: f32 = 100.0;
    const OVERSCAN: f32 = 20.0;
    const DESCENDANTS: usize = 10_000;
    let cap = VirtualListLayout::uniform_window_item_cap(VIEWPORT, OVERSCAN, ROW);
    assert!(cap < DESCENDANTS);

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let tree = context.create_component(document, List::new()).unwrap();
    let mut keys = vec![0usize, 1, 2];
    let mut layout = VirtualTreeLayout::uniform(ROW, [0, 0, 0]);
    let mut items = VirtualTreeItems::<usize, Text>::default();

    context
        .materialize_virtual_tree(
            tree,
            &mut items,
            &layout,
            0.0,
            VIEWPORT,
            OVERSCAN,
            |index| keys[index],
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();

    let child_keys = (1_000_000..1_000_000 + DESCENDANTS).collect::<Vec<_>>();
    assert!(layout.expand(
        0,
        child_keys.iter().map(|_| nana_ui_core::VirtualTreeRow {
            extent: ROW,
            descendant_count: 0,
        })
    ));
    keys.splice(1..1, child_keys);
    let descendant_count = layout
        .descendant_count(0)
        .expect("expanded parent keeps a descendant count");
    assert_eq!(descendant_count, DESCENDANTS);

    context
        .materialize_virtual_tree(
            tree,
            &mut items,
            &layout,
            0.0,
            VIEWPORT,
            OVERSCAN,
            |index| keys[index],
            |index, _| Text::new(format!("row {index}")),
        )
        .unwrap();
    let live = context
        .world()
        .node(tree.stable_id())
        .unwrap()
        .children
        .len();
    assert!(
        live <= cap,
        "live List children {live} exceed geometric cap {cap}"
    );
    assert!(
        live < descendant_count,
        "live List children {live} mounted every expanded descendant ({descendant_count})"
    );
}

#[test]
fn stack_presets_express_row_and_column_layout() {
    let row = Stack::row(8.0).node_style();
    let layout = row.layout;
    assert_eq!(layout.direction, Some(nana_ui_core::FlexDirection::Row));
    assert_eq!(layout.gap, Some(nana_ui_core::LengthSpec::Px(8.0)));
    assert_eq!(layout.align_items, nana_ui_core::AlignSpec::Center);
    assert_eq!(layout.width, Some(nana_ui_core::LengthSpec::Shrink));

    let fill_column = Stack::fill_column(0.0).node_style();
    assert_eq!(
        fill_column.layout.direction,
        Some(nana_ui_core::FlexDirection::Column)
    );
    assert_eq!(
        fill_column.layout.width,
        Some(nana_ui_core::LengthSpec::Fill)
    );
    assert_eq!(
        fill_column.layout.height,
        Some(nana_ui_core::LengthSpec::Fill)
    );
    assert_eq!(fill_column.layout.flex_grow, Some(1.0));
    assert_eq!(fill_column.layout.flex_shrink, Some(1.0));

    let outlined = Stack::column(4.0)
        .outline(nana_ui_core::SemanticColorRole::Border, 1.0)
        .node_style();
    assert_eq!(
        outlined.border,
        Some(nana_ui_core::SemanticColorRole::Border)
    );
    assert_eq!(outlined.layout.border_width, Some(1.0));
}

#[test]
fn card_kind_defaults_yield_to_explicit_style() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();

    let surface = context.create_component(document, Card::new()).unwrap();
    let style = context
        .world()
        .node_style(surface.stable_id())
        .cloned()
        .unwrap();
    assert_eq!(
        style.background,
        Some(nana_ui_core::SemanticColorRole::Surface)
    );
    assert_eq!(style.border, None);
    assert_eq!(style.layout.border_width, Some(0.0));

    let custom = NodeStyle::default().outline(nana_ui_core::SemanticColorRole::Border, 2.0);
    let outlined = context
        .create_component(
            document,
            Card::new()
                .kind(nana_ui_core::CardKind::Outlined)
                .style(custom),
        )
        .unwrap();
    let style = context
        .world()
        .node_style(outlined.stable_id())
        .cloned()
        .unwrap();
    assert_eq!(
        style.border,
        Some(nana_ui_core::SemanticColorRole::Border),
        "用户显式设置的边框不得被 kind 默认值覆盖"
    );
    assert_eq!(
        style.layout.border_width,
        Some(2.0),
        "用户显式设置的边框宽度不得被 kind 默认值覆盖"
    );
}

/// Memoizes a probe of the retained subtree (own child count) into text
/// state: exactly the stale-snapshot shape that `wants_child_reproject`
/// exists for. Two types share this projection; only one opts in.
#[derive(Debug, Clone, Default, PartialEq)]
struct ReprojectProbe;

#[derive(Debug, Clone, Default, PartialEq)]
struct PlainProbe;

fn project_child_count(id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
    let count = world.node(id).map(|node| node.children.len()).unwrap_or(0);
    let value = count.to_string();
    if world.text(id) != Some(value.as_str()) {
        mutations.set_text(
            id,
            crate::TextContent {
                value: value.into(),
            },
        );
    }
}

impl ComponentView for ReprojectProbe {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "reproject-probe".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_child_count(id, world, mutations);
    }

    fn wants_child_reproject() -> bool {
        true
    }
}

impl ComponentView for PlainProbe {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "plain-probe".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        project_child_count(id, world, mutations);
    }
}

#[test]
fn opt_in_component_reprojects_when_children_mount() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let probe: Entity<ReprojectProbe> = context.create_component(document, ReprojectProbe).unwrap();
    assert_eq!(context.world().text(probe.stable_id()), Some("0"));

    context
        .build_child(probe, |builder| {
            builder.child("row", crate::Text::new("row"));
        })
        .unwrap();

    assert_eq!(
        context
            .world()
            .node(probe.stable_id())
            .expect("probe node")
            .children
            .len(),
        1
    );
    assert_eq!(
        context.world().text(probe.stable_id()),
        Some("1"),
        "child mount must rerun project for opted-in components"
    );
}

#[test]
fn opt_in_component_reprojects_when_child_detaches() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let probe: Entity<ReprojectProbe> = context.create_component(document, ReprojectProbe).unwrap();
    context
        .build_child(probe, |builder| {
            builder.child("row", crate::Text::new("row"));
        })
        .unwrap();
    assert_eq!(context.world().text(probe.stable_id()), Some("1"));

    let row = context
        .world()
        .node(probe.stable_id())
        .expect("probe node")
        .children[0];
    let mut queue = MutationQueue::new();
    queue.detach(row);
    context.commit_mutations(queue).unwrap();

    assert_eq!(
        context.world().text(probe.stable_id()),
        Some("0"),
        "child detach must rerun project for opted-in components"
    );
}

#[test]
fn component_without_opt_in_keeps_data_change_schedule() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let probe: Entity<PlainProbe> = context.create_component(document, PlainProbe).unwrap();
    assert_eq!(context.world().text(probe.stable_id()), Some("0"));

    context
        .build_child(probe, |builder| {
            builder.child("row", crate::Text::new("row"));
        })
        .unwrap();

    assert_eq!(
        context
            .world()
            .node(probe.stable_id())
            .expect("probe node")
            .children
            .len(),
        1
    );
    assert_eq!(
        context.world().text(probe.stable_id()),
        Some("0"),
        "components that do not opt in must not reproject on child structure changes"
    );
}

/// 行内 inlay 下的水平光标移动:插入区间内部没有 caret 边界,Right/
/// WordRight 的显示目标落入区间内部时前进到区间末端重映射——一次按键
/// 跨过整个插入区间,值偏移步进为一;Left/WordLeft 逆向同理不卡死。
/// 断言以裸文本移动 oracle 等值:inlay 呈现不改变值空间移动结果。
fn move_caret_from(
    context: &mut AppContext,
    document: DocumentId,
    node: crate::StableNodeId,
    focus: usize,
    intent: TextCaretIntent,
) -> usize {
    context
        .update_component(Entity::<TextArea>::from_stable_id(node), |area, _| {
            area.state.selection = TextSelection::caret(focus);
        })
        .unwrap();
    assert!(
        context
            .move_focused_text_caret(document, intent, false, None)
            .unwrap(),
        "水平移动必须是有效移动,不得被插入区间钳成空操作"
    );
    context.world().text_input(node).unwrap().selection.focus
}

#[test]
fn caret_movement_steps_across_inlays_without_sticking() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let value = "f(x, y)";
    let area = context
        .create_component(
            document,
            TextArea::new(value).inlays(Arc::from([
                TextInlay::new(2, "a:"),
                TextInlay::new(5, "b:"),
            ])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());

    // 四个水平意图在 inlay 呈现下与裸文本 oracle 完全一致(移动免疫),
    // 起点覆盖锚点、锚点前后与第二处插入区间。
    for (intent, starts) in [
        (TextCaretIntent::Right, [2usize, 3, 4, 5].as_slice()),
        (TextCaretIntent::Left, [6, 5, 3, 2].as_slice()),
        (TextCaretIntent::WordRight, [2usize, 4, 5].as_slice()),
        (TextCaretIntent::WordLeft, [6, 5, 3].as_slice()),
    ] {
        for &start in starts {
            let expected =
                crate::text_editing::caret_focus(value, TextSelection::caret(start), intent)
                    .unwrap_or(start);
            let moved = move_caret_from(&mut context, document, node, start, intent);
            assert_eq!(
                moved, expected,
                "{intent:?} from {start} 越过 inlay 后应与裸文本一致"
            );
        }
    }

    // 评审探针场景:光标停在 inlay 锚点连按 Right,每次都前进一格。
    let mut focus = 2;
    for expected in [3, 4, 5, 6] {
        focus = move_caret_from(&mut context, document, node, focus, TextCaretIntent::Right);
        assert_eq!(focus, expected, "连按 Right 不得卡死在锚点");
    }
}

/// An inlay anchored on a character of several code points (CRLF, a skin
/// toned emoji), or one whose label is a single grapheme: Right still steps
/// one grapheme at a time, as over the bare text.
#[test]
fn caret_right_steps_past_inlays_on_multi_code_point_anchors() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let value = "a\r\nb\u{1F44D}\u{1F3FD}c";
    let area = context
        .create_component(
            document,
            TextArea::new(value).inlays(Arc::from([
                TextInlay::new(1, "p:"),
                TextInlay::new(4, "q"),
                TextInlay::new(12, "r"),
            ])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let mut focus = 0;
    for expected in [1, 3, 4, 12, 13] {
        focus = move_caret_from(&mut context, document, node, focus, TextCaretIntent::Right);
        assert_eq!(focus, expected, "Right from each boundary to the next");
    }
}

/// End on a line whose end carries an inlay (a type hint) stays on that
/// line: landing after the label maps back to the anchor, which is the end.
#[test]
fn line_end_stops_before_a_trailing_inlay_anchor() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let area = context
        .create_component(
            document,
            TextArea::new("ab\ncd").inlays(Arc::from([TextInlay::new(2, ": i32")])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    assert_eq!(
        move_caret_from(&mut context, document, node, 0, TextCaretIntent::LineEnd),
        2
    );
    context
        .move_focused_text_caret(document, TextCaretIntent::LineEnd, false, None)
        .unwrap();
    assert_eq!(
        context.world().text_input(node).unwrap().selection.focus,
        2,
        "a second End does not step onto the next line"
    );
}

/// 折叠摘要上的 Right 同源卡死(既有隐患的回归锚):摘要文本内部没有
/// caret 边界,Right 一次跨过整个摘要落到折叠后首字符,再按继续前进。
#[test]
fn caret_right_steps_across_fold_summaries() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let value = "fn a() {\n    x();\n    y();\n}\nfn b() {}";
    let fold = TextCodeFold::new(7, 28);
    let area = context
        .create_component(document, TextArea::new(value).code_folds(Arc::from([fold])))
        .unwrap();
    let node = area.stable_id();
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node, Arc::from([fold]));
    context.commit_mutations(queue).unwrap();
    assert!(context.focus_node(document, node).unwrap());

    // 折叠起始行行尾(值 8,摘要 ` …3` 占显示 [8,13)):Right 一次跨过
    // 整个摘要,落到折叠后首字符(值 28);再 Right 前进到 `fn b` 的
    // `f`(值 29)。
    let moved = move_caret_from(&mut context, document, node, 8, TextCaretIntent::Right);
    assert_eq!(moved, 28, "Right 一次跨过折叠摘要");
    let moved = move_caret_from(&mut context, document, node, moved, TextCaretIntent::Right);
    assert_eq!(moved, 29, "摘要之后继续逐字符前进");
}

/// WordRight past an inlay before the whitespace that ends a fold's first
/// line: the word after it is hidden, so the caret crosses the fold as
/// stepping onto its summary does, instead of landing in its hidden lines.
#[test]
fn word_right_past_an_inlay_crosses_a_fold_rather_than_entering_it() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let value = "ab \n    xy();\n}\nz";
    let fold = TextCodeFold::new(0, 15);
    let area = context
        .create_component(
            document,
            TextArea::new(value)
                .code_folds(Arc::from([fold]))
                .inlays(Arc::from([TextInlay::new(2, "k ")])),
        )
        .unwrap();
    let node = area.stable_id();
    let mut queue = MutationQueue::new();
    queue.set_text_input_fold_collapsed(node, Arc::from([fold]));
    context.commit_mutations(queue).unwrap();
    assert!(context.focus_node(document, node).unwrap());
    let moved = move_caret_from(&mut context, document, node, 2, TextCaretIntent::WordRight);
    assert_eq!(
        moved, 15,
        "to the end of the fold, not into its hidden lines"
    );
}

/// WordRight from before an inlay's anchor, stopping inside the label: the
/// label's words are not the text's, so it goes where the bare text would.
#[test]
fn word_right_into_an_inlay_from_before_its_anchor_moves_over_the_bare_text() {
    for (value, inlay, start) in [
        ("ab cd", TextInlay::new(3, "i32 "), 2),
        ("ab x", TextInlay::new(2, "cd "), 0),
    ] {
        let mut context = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let area = context
            .create_component(document, TextArea::new(value).inlays(Arc::from([inlay])))
            .unwrap();
        let node = area.stable_id();
        assert!(context.focus_node(document, node).unwrap());
        let expected = crate::text_editing::caret_focus(
            value,
            TextSelection::caret(start),
            TextCaretIntent::WordRight,
        )
        .unwrap();
        let moved = move_caret_from(
            &mut context,
            document,
            node,
            start,
            TextCaretIntent::WordRight,
        );
        assert_eq!(moved, expected, "{value:?} from {start}");
    }
}

/// WordRight that stops on a label's word boundary goes on to the end of the
/// word after the anchor, as over the bare text, not one grapheme.
#[test]
fn word_right_past_an_inlay_reaches_the_end_of_the_anchored_word() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let value = "value = 1";
    let area = context
        .create_component(
            document,
            TextArea::new(value).inlays(Arc::from([TextInlay::new(0, ": i32 ")])),
        )
        .unwrap();
    let node = area.stable_id();
    assert!(context.focus_node(document, node).unwrap());
    let moved = move_caret_from(&mut context, document, node, 0, TextCaretIntent::WordRight);
    assert_eq!(moved, 5);
}

#[test]
fn framework_dismissed_context_menu_clears_open_state_and_emits_dismiss() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let menu = context
        .create_component(
            document,
            ContextMenu::new(24.0, 36.0).items([crate::ContextMenuItem::new("open", "Open")]),
        )
        .unwrap();
    context.append_child(host, menu).unwrap();
    context.activate_overlay(host, menu).unwrap();
    assert!(context.read(menu, |menu| menu.open).unwrap());

    let dismissals = Arc::new(Mutex::new(0usize));
    let observed = Arc::clone(&dismissals);
    context
        .on(menu, move |_menu, event: &ContextMenuEvent, _cx| {
            if matches!(event, ContextMenuEvent::Dismiss) {
                *observed.lock().unwrap() += 1;
            }
        })
        .unwrap();

    assert!(
        context
            .route_overlay_key(document, OverlayKey::Escape)
            .unwrap()
    );
    assert!(
        !context.read(menu, |menu| menu.open).unwrap(),
        "framework dismissal must clear the menu's own presence flag"
    );
    assert_eq!(*dismissals.lock().unwrap(), 1);
}

#[test]
fn outside_press_dismisses_a_hosted_context_menu_and_reports_it_to_the_view() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let stage = context
        .create_component(document, Button::new("Stage"))
        .unwrap();
    let host = context
        .create_component(document, OverlayHost::new())
        .unwrap();
    let menu = context
        .create_component(
            document,
            ContextMenu::new(100.0, 0.0).items([crate::ContextMenuItem::new("open", "Open")]),
        )
        .unwrap();
    context.append_child(host, menu).unwrap();
    let mut layout = MutationQueue::new();
    layout.write_layout(
        stage.stable_id(),
        crate::LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 80.0,
            height: 40.0,
        },
    );
    layout.write_layout(
        menu.stable_id(),
        crate::LayoutBox {
            x: 100.0,
            y: 0.0,
            width: 120.0,
            height: 160.0,
        },
    );
    context.commit_mutations(layout).unwrap();
    context.activate_overlay(host, menu).unwrap();
    context.rebuild_hit_test(document);

    let dismissals = Arc::new(Mutex::new(0usize));
    let observed = Arc::clone(&dismissals);
    context
        .on(menu, move |_menu, event: &ContextMenuEvent, _cx| {
            if matches!(event, ContextMenuEvent::Dismiss) {
                *observed.lock().unwrap() += 1;
            }
        })
        .unwrap();

    context
        .route_overlay_pointer(document, 1, OverlayPointerPhase::PrimaryDown, 10.0, 10.0)
        .unwrap();
    let up = context
        .route_overlay_pointer(document, 1, OverlayPointerPhase::PrimaryUp, 10.0, 10.0)
        .unwrap();
    assert!(up.dismissed, "a press outside the surface closes the menu");
    assert!(!context.read(menu, |menu| menu.open).unwrap());
    assert_eq!(*dismissals.lock().unwrap(), 1);
}

#[test]
fn retained_child_reconciliation_preserves_identity_focus_and_parked_state() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let first = context
        .create_detached_component(document, TextInput::new("draft"))
        .unwrap();
    let second = context
        .create_detached_component(document, Button::new("second"))
        .unwrap();
    context
        .reconcile_children(parent.id, &[first.id, second.id])
        .unwrap();
    context
        .on(second, |button, _: &Activate, _| {
            button.label = "activated".into()
        })
        .unwrap();
    context.focus_node(document, first.id).unwrap();
    assert!(
        context
            .reconcile_children(parent.id, &[second.id, first.id])
            .unwrap()
    );
    assert_eq!(context.world.focused(document), Some(first.id));
    let generation = context.world.generation();
    assert!(
        !context
            .reconcile_children(parent.id, &[second.id, first.id])
            .unwrap()
    );
    assert_eq!(context.world.generation(), generation);
    context.reconcile_children(parent.id, &[second.id]).unwrap();
    assert!(!context.world.is_mounted(first.id));
    assert_eq!(
        context
            .read(first, |input| input.state.value.clone())
            .unwrap(),
        "draft"
    );
    context
        .reconcile_children(parent.id, &[first.id, second.id])
        .unwrap();
    assert!(context.world.is_mounted(first.id));
    context.activate_button(second).unwrap();
    assert_eq!(
        context.read(second, |button| button.label.clone()).unwrap(),
        "activated"
    );
    assert_eq!(
        context.world.node(parent.id).unwrap().children.as_slice(),
        &[first.id, second.id]
    );
    let parked = context
        .create_detached_component(document, Stack::column(0.0))
        .unwrap();
    context.reconcile_children(parked.id, &[first.id]).unwrap();
    assert_eq!(
        context.world.node(first.id).unwrap().parent,
        Some(parked.id)
    );
    assert!(!context.world.is_mounted(first.id));
    context
        .reconcile_children(parent.id, &[parked.id, second.id])
        .unwrap();
    assert!(context.world.is_mounted(first.id));
}

#[test]
fn retained_child_reconciliation_extracts_live_editor_before_parking_ancestor() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let parked = context
        .create_detached_component(document, Stack::column(0.0))
        .unwrap();
    let first = context
        .create_detached_component(document, TextInput::new("draft"))
        .unwrap();
    context.append_child(parent, parked).unwrap();
    context.append_child(parked, first).unwrap();
    // Extract a live editor from an omitted ancestor in the same transaction.
    // The editor never leaves the live tree, so its editing session must survive.
    context.focus_node(document, first.id).unwrap();
    context.replace_text_input_selection(first, "!").unwrap();
    let edited = context.read(first, |input| input.state.clone()).unwrap();
    context
        .set_ime_preedit(document, "输入".into(), Some((0, 3)))
        .unwrap();
    let ime = context
        .world
        .ime(first.id)
        .map(|ime| ime.to_composition())
        .unwrap();
    context.reconcile_children(parent.id, &[first.id]).unwrap();
    assert!(!context.world.is_mounted(parked.id));
    assert_eq!(
        context.world.node(first.id).unwrap().parent,
        Some(parent.id)
    );
    assert_eq!(context.world.focused(document), Some(first.id));
    assert!(
        context
            .world
            .ime(first.id)
            .is_some_and(|current| current == ime)
    );
    assert_eq!(
        context.read(first, |input| input.state.clone()).unwrap(),
        edited
    );
    context.clear_ime(document).unwrap();
    assert!(context.undo_focused_text(document).unwrap());
    assert_eq!(
        context
            .read(first, |input| input.state.value.clone())
            .unwrap(),
        "draft"
    );
    assert!(context.redo_focused_text(document).unwrap());
    assert_eq!(
        context
            .read(first, |input| input.state.value.clone())
            .unwrap(),
        edited.value
    );
}

#[test]
fn retained_child_reconciliation_preserves_extracted_tooltip_and_loading_lifecycles() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let old = context
        .create_detached_component(document, Stack::column(0.0))
        .unwrap();
    let icon = context
        .create_detached_component(
            document,
            IconButton::new(nana_ui_core::Icon::About, "Details").tooltip(
                "Details",
                nana_ui_core::TooltipConfig {
                    delay_ms: 0,
                    ..Default::default()
                },
            ),
        )
        .unwrap();
    let loading = context
        .create_detached_component(document, Button::new("Work").loading(true))
        .unwrap();
    context.append_child(parent, old).unwrap();
    context.append_child(old, icon).unwrap();
    context.append_child(old, loading).unwrap();
    context
        .set_pointer_hover_at(document, 1, Some(icon.id), Duration::ZERO)
        .unwrap();
    let tooltip = context.world.overlay_host(icon.id).unwrap().active.unwrap();
    let deadline = context.next_animation_deadline();
    let generation = context.world.generation();
    let mut invalid = MutationQueue::new();
    invalid.park_subtree(old.id);
    invalid.insert(icon.id, old.id, None);
    assert!(context.commit_mutations(invalid).is_err());
    assert_eq!(context.world.generation(), generation);
    assert!(context.read(icon, |icon| icon.tooltip_open).unwrap());
    assert_eq!(
        context.world.overlay_host(icon.id).unwrap().active,
        Some(tooltip)
    );
    assert_eq!(context.next_animation_deadline(), deadline);

    context
        .reconcile_children(parent.id, &[icon.id, loading.id])
        .unwrap();
    assert!(!context.world.is_mounted(old.id));
    assert!(context.read(icon, |icon| icon.tooltip_open).unwrap());
    assert!(matches!(
        context.world.standard_visual(icon.id),
        Some(StandardVisual::Icon {
            tooltip: Some(crate::TooltipVisual { open: true, .. }),
            ..
        })
    ));
    assert_eq!(
        context.world.overlay_host(icon.id).unwrap().active,
        Some(tooltip)
    );
    assert_eq!(context.next_animation_deadline(), deadline);
    assert!(
        context
            .advance_animations(Duration::from_millis(160))
            .component_updates
            .contains(&loading.id)
    );

    let generation = context.world.generation();
    let mut park = MutationQueue::new();
    park.park_subtree(icon.id);
    park.park_subtree(loading.id);
    let report = context.commit_mutations(park).unwrap();
    assert_eq!(report.generation, generation + 1);
    assert!(!context.read(icon, |icon| icon.tooltip_open).unwrap());
    assert!(matches!(
        context.world.standard_visual(icon.id),
        Some(StandardVisual::Icon {
            tooltip: Some(crate::TooltipVisual { open: false, .. }),
            ..
        })
    ));
    assert_eq!(context.next_animation_deadline(), None);
    context
        .reconcile_children(parent.id, &[icon.id, loading.id])
        .unwrap();
    assert!(!context.read(icon, |icon| icon.tooltip_open).unwrap());
    assert!(context.next_animation_deadline().is_some());
}

#[test]
fn retained_child_reconciliation_rejects_invalid_batches_without_parking_siblings() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let root = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let parent = context
        .create_detached_component(document, Stack::column(0.0))
        .unwrap();
    let child = context
        .create_detached_component(document, TextInput::new("kept"))
        .unwrap();
    context.append_child(root, parent).unwrap();
    context.append_child(parent, child).unwrap();
    context.focus_node(document, child.id).unwrap();
    let foreign = context
        .create_component(DocumentId::new(2).unwrap(), Button::new("foreign"))
        .unwrap();
    for desired in [
        vec![child.id, child.id],
        vec![parent.id],
        vec![root.id],
        vec![foreign.id],
        vec![StableNodeId::new(u64::MAX).unwrap()],
    ] {
        let generation = context.world.generation();
        assert!(context.reconcile_children(parent.id, &desired).is_err());
        assert_eq!(context.world.generation(), generation);
        assert_eq!(
            context.world.node(parent.id).unwrap().children.as_slice(),
            &[child.id]
        );
        assert!(context.world.is_mounted(child.id));
        assert_eq!(context.world.focused(document), Some(child.id));
    }
}

#[test]
fn rejected_reconciliation_keeps_active_overlay_and_focus_lifecycle_untouched() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let host = context
        .create_component(document, crate::OverlayHost::new())
        .unwrap();
    let panel = context
        .create_detached_component(document, crate::Panel::new("Task"))
        .unwrap();
    context.append_child(host, panel).unwrap();
    context.activate_overlay(host, panel).unwrap();
    let foreign = context
        .create_component(DocumentId::new(2).unwrap(), Button::new("foreign"))
        .unwrap();
    let before = context.world.overlay_host(host.id).unwrap();
    let generation = context.world.generation();
    assert!(context.reconcile_children(host.id, &[foreign.id]).is_err());
    assert_eq!(context.world.overlay_host(host.id).unwrap(), before);
    assert_eq!(context.world.generation(), generation);
    assert!(context.world.is_mounted(panel.id));
    assert_eq!(context.world.node(panel.id).unwrap().parent, Some(host.id));
}

/// A secure field draws bullets, not its value. The visual-order arrow probe
/// must therefore not be asked about the value: the answer would come from a
/// layout that is not on screen, and the host would retain the plaintext in
/// its geometry and shape cache. Those fields step by grapheme instead, which
/// is what "left" means in a column of identical bullets.
#[test]
fn a_secure_field_never_probes_its_plaintext_for_a_visual_caret_step() {
    #[derive(Default)]
    struct RecordingShaper {
        probed: Vec<String>,
    }
    impl crate::TextShaper for RecordingShaper {
        fn shape(
            &mut self,
            _id: StableNodeId,
            text: &TextContent,
            _style: &crate::ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> crate::TextMetrics {
            crate::TextMetrics {
                width: text.value.chars().count() as f32 * 7.0,
                height: 16.0,
                ascent: None,
            }
        }

        fn text_caret_visual_step(
            &mut self,
            _id: StableNodeId,
            text: &TextContent,
            offset: usize,
            _affinity: crate::TextAffinity,
            rightwards: bool,
            _style: &crate::ComputedStyle,
            _constraints: crate::TextShapeConstraints,
        ) -> Option<crate::TextHit> {
            self.probed.push(text.value.to_string());
            let next = if rightwards {
                crate::text_editing::next_grapheme(&text.value, offset)
            } else {
                crate::text_editing::prev_grapheme(&text.value, offset)
            };
            next.map(crate::TextHit::downstream)
        }
    }

    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let secret = context
        .create_component(document, TextInput::new("hunter2").secure(true))
        .unwrap();
    let plain = context
        .create_component(document, TextInput::new("hunter2"))
        .unwrap();
    let mut shaper = RecordingShaper::default();

    // The secure field: the caret still moves, by grapheme.
    assert!(context.focus_node(document, secret.stable_id()).unwrap());
    context
        .update_component(secret, |input, _| {
            input.state.selection = TextSelection::caret(0);
        })
        .unwrap();
    assert!(
        context
            .move_focused_text_caret(document, TextCaretIntent::Right, false, Some(&mut shaper))
            .unwrap()
    );
    assert_eq!(
        context
            .world()
            .text_input(secret.stable_id())
            .unwrap()
            .selection
            .focus,
        1
    );
    assert!(
        shaper.probed.is_empty(),
        "the secure field's value must not reach a geometry probe: {:?}",
        shaper.probed
    );

    // The same field without `secure`: this one is probed, with its value.
    assert!(context.focus_node(document, plain.stable_id()).unwrap());
    context
        .update_component(plain, |input, _| {
            input.state.selection = TextSelection::caret(0);
        })
        .unwrap();
    assert!(
        context
            .move_focused_text_caret(document, TextCaretIntent::Right, false, Some(&mut shaper))
            .unwrap()
    );
    assert_eq!(shaper.probed, vec!["hunter2".to_owned()]);

    // A composition that ended sends an empty preedit rather than clearing the
    // slot. The display text is the value again then, so the arrow still asks
    // the geometry.
    assert!(
        context
            .set_ime_preedit(document, String::new(), None)
            .unwrap()
    );
    context
        .update_component(plain, |input, _| {
            input.state.selection = TextSelection::caret(0);
        })
        .unwrap();
    assert!(
        context
            .move_focused_text_caret(document, TextCaretIntent::Right, false, Some(&mut shaper))
            .unwrap()
    );
    assert_eq!(shaper.probed.len(), 2, "{:?}", shaper.probed);

    // A live preedit draws inline text the value does not contain -- and no
    // caret move runs at all then, which is the invariant the probe's guard
    // relies on instead of testing the preedit itself.
    assert!(
        context
            .set_ime_preedit(document, "ni".into(), None)
            .unwrap()
    );
    context
        .update_component(plain, |input, _| {
            input.state.selection = TextSelection::caret(0);
        })
        .unwrap();
    assert!(
        !context
            .move_focused_text_caret(document, TextCaretIntent::Right, false, Some(&mut shaper))
            .unwrap(),
        "a composing editor does not move its caret"
    );
    assert_eq!(shaper.probed.len(), 2, "{:?}", shaper.probed);
}

/// Issue #102: a component recipe reaches a control that was already on
/// screen when the theme was installed.
///
/// The `Button` path is the hard half. Its recipe decides which
/// `SemanticColorRole` the component *authors* onto its own node, and nothing
/// downstream can re-decide that — so the button has to be reprojected. Issue
/// #101 §1.6 measured reprojection and rejected it as a general mechanism
/// precisely because it produced zero mutations when nothing read the
/// installed theme; opting one component in, for the one thing only that
/// component can write, is the case §1.6 left open.
#[test]
fn an_installed_button_recipe_reaches_a_button_that_is_already_on_screen() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(
            document,
            crate::Button::new("Ship it").kind(nana_ui_core::ButtonKind::Primary),
        )
        .unwrap();

    let hovered_role = |context: &AppContext| {
        context
            .world()
            .node_style(button.stable_id())
            .expect("the button has a style")
            .interaction
            .hovered
            .background
    };
    assert_eq!(
        hovered_role(&context),
        Some(nana_ui_core::SemanticColorRole::AccentSoftHover)
    );

    let mut definition = nana_ui_core::ThemeDefinition::NANA_DARK;
    let primary = nana_ui_core::ButtonVariantDraft {
        hovered_background: Some(nana_ui_core::SemanticColorRole::Warning),
        ..definition
            .components
            .button
            .variant(nana_ui_core::ButtonKind::Primary)
    };
    definition.components.button = definition
        .components
        .button
        .with(nana_ui_core::ButtonKind::Primary, primary);
    let definition = definition.bump();

    assert!(context.set_theme_definition(&definition).unwrap());
    assert_eq!(
        hovered_role(&context),
        Some(nana_ui_core::SemanticColorRole::Warning),
        "a recipe change must reach a button that already exists"
    );
}

/// The other half, and the cheap one: a family recipe resolves in `extract`,
/// where the installed theme is already in hand, so it needs no reprojection.
#[test]
fn an_installed_family_recipe_reaches_a_live_node_without_reprojecting_it() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let bar = context
        .create_component(document, crate::Progress::new(0.5, 1.0))
        .unwrap();

    let foreground = |context: &AppContext| {
        context.world().extract_nodes(&[bar.stable_id()])[0].standard_visual_foreground
    };
    let palette = nana_ui_core::SemanticPalette::dark();
    assert_eq!(foreground(&context), Some(palette.accent.as_rgba_array()));

    let mut definition = nana_ui_core::ThemeDefinition::NANA_DARK;
    definition.components.families[nana_ui_core::ComponentRecipeId::Indicator.index()] =
        nana_ui_core::ComponentRecipeDraft::plain(nana_ui_core::SemanticColorRole::Success);
    let definition = definition.bump();

    assert!(context.set_theme_definition(&definition).unwrap());
    assert_eq!(
        foreground(&context),
        Some(palette.success.as_rgba_array()),
        "a family recipe resolves at extract, so the node needs no reprojection"
    );
}

/// Fail-closed reaches the host: an invalid definition installs nothing.
#[test]
fn a_theme_that_fails_validation_leaves_the_installed_one_alone() {
    let mut context = AppContext::new();
    let before = context.world().theme().clone();

    let mut broken = nana_ui_core::ThemeDefinition::NANA_DARK;
    broken.tokens.metrics.radius_md = -4.0;
    let broken = broken.bump();

    let error = context
        .set_theme_definition(&broken)
        .expect_err("a negative radius is not installable");
    assert!(
        error.to_string().contains("metrics.radius_md"),
        "the error has to name the token: {error}"
    );
    assert_eq!(
        *context.world().theme(),
        before,
        "a rejected theme must not have been partially applied"
    );
}

/// Issue #102: the elevation ramp is a theme token, so a modal's lift follows
/// the installed theme instead of a `background.r > 0.5` brightness sniff.
#[test]
fn the_overlay_shadow_is_a_theme_token_not_a_brightness_sniff() {
    let mut definition = nana_ui_core::ThemeDefinition::NANA_DARK;
    definition.effects.overlay = nana_ui_core::ShadowToken {
        color: nana_ui_core::SemanticColor::rgba(0.0, 0.0, 0.0, 0.9),
        offset_x: 0.0,
        offset_y: 21.0,
        blur_radius: 42.0,
        spread_radius: 0.0,
        inset: false,
    };
    let compiled = definition.bump().compile().expect("compiles");
    let elevation = crate::ComponentElevation::from_shadow(
        compiled.shadow(nana_ui_core::ElevationRole::Overlay),
    );
    assert_eq!(elevation.offset_y, 21.0);
    assert_eq!(elevation.color[3], 0.9);

    // A dark theme whose background happens to be pale used to flip to the
    // light shadow. It now keeps its own.
    let mut pale = nana_ui_core::SemanticPalette::dark();
    pale.background = nana_ui_core::SemanticColor::rgb8(240, 240, 240);
    let compiled = nana_ui_core::ThemeDefinition::NANA_DARK
        .with_palette(pale)
        .compile()
        .expect("compiles");
    assert_eq!(
        compiled
            .shadow(nana_ui_core::ElevationRole::Overlay)
            .color
            .a,
        nana_ui_core::EffectTokens::DARK.overlay.color.a
    );
}

/// A host that repaints a surface from business state needs to know whether
/// that repaint changed anything, so it can skip asking for a frame. The commit
/// already diffs; rewriting a component with the values it already holds must
/// leave nothing for the next flush to do.
#[test]
fn rewriting_a_component_with_its_own_values_leaves_no_pending_work() {
    let mut context = AppContext::new();
    let document = DocumentId::new(917).unwrap();
    let text = context
        .create_component(document, Text::new("输出 · 已连接"))
        .unwrap();
    let _ = context.compat_world_mut().take_system_work();
    assert!(
        !context.world().has_pending_work(),
        "a drained world owes no work"
    );

    context
        .update_component(text, |view, _| view.value = "输出 · 已连接".to_string())
        .unwrap();
    assert!(
        !context.world().has_pending_work(),
        "an unchanged rewrite must not dirty the node"
    );

    context
        .update_component(text, |view, _| view.value = "输出 · 已断开".to_string())
        .unwrap();
    assert!(
        context.world().has_pending_work(),
        "a real change must dirty the node"
    );
}

/// Removing content is a change too: a repaint that parks a subtree must read
/// as pending work, or a host that skips frames on "nothing changed" would
/// leave the removed rows on screen.
#[test]
fn parking_a_subtree_is_pending_work() {
    let mut context = AppContext::new();
    let document = DocumentId::new(918).unwrap();
    let root = context
        .create_component(document, Stack::column(4.0))
        .unwrap();
    let row = context
        .create_detached_component(document, Text::new("一行"))
        .unwrap();
    context.append_child(root, row).unwrap();
    let _ = context.compat_world_mut().take_system_work();
    assert!(!context.world().has_pending_work());

    let mut queue = crate::MutationQueue::new();
    queue.park_subtree(row.stable_id());
    context.commit_mutations(queue).unwrap();
    assert!(
        context.world().has_pending_work(),
        "parking a subtree must leave work for the next flush"
    );
}

thread_local! {
    static PROJECTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn projections() -> usize {
    PROJECTIONS.with(std::cell::Cell::get)
}

/// Counts its projections and shows `value` as its text.
#[derive(Debug, Clone, Default, PartialEq)]
struct CountingProbe {
    value: String,
}

impl ComponentView for CountingProbe {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "counting-probe".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        PROJECTIONS.with(|count| count.set(count.get() + 1));
        if world.text(id) != Some(self.value.as_str()) {
            mutations.set_text(
                id,
                crate::TextContent {
                    value: self.value.clone().into(),
                },
            );
        }
    }
}

/// `CountingProbe` that projects on every update.
#[derive(Debug, Clone, Default, PartialEq)]
struct AlwaysProbe;

impl ComponentView for AlwaysProbe {
    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "always-probe".into(),
        }
    }

    fn project(&self, _id: StableNodeId, _world: &UiWorld, _mutations: &mut MutationQueue) {
        PROJECTIONS.with(|count| count.set(count.get() + 1));
    }

    const ALWAYS_REPROJECT: bool = true;
}

/// Issue #228: an application refreshing a list rewrites every row with the
/// values it already has. None of that may project, commit or dirty.
#[test]
fn rewriting_a_list_with_its_own_values_does_no_work() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let list = context.create_component(document, List::new()).unwrap();
    let mut rows = Vec::new();
    for index in 0..40 {
        let item = context
            .create_detached_component(document, ListItem::new(format!("动作 {index}")))
            .unwrap();
        let switch = context
            .create_detached_component(document, Switch::new("", index % 2 == 0))
            .unwrap();
        let button = context
            .create_detached_component(document, Button::new("收藏"))
            .unwrap();
        context.append_child(list, item).unwrap();
        context.append_child(item, switch).unwrap();
        context.append_child(item, button).unwrap();
        rows.push((item, switch, button));
    }
    let _ = context.compat_world_mut().take_system_work();
    let generation = context.world().generation();

    for (index, (item, switch, button)) in rows.iter().enumerate() {
        context
            .update_component(*item, |item, _| {
                item.label = format!("动作 {index}");
                item.selected = false;
                Arc::make_mut(&mut item.style.layout).hidden = false;
            })
            .unwrap();
        context
            .update_component(*switch, |switch, _| switch.checked = index % 2 == 0)
            .unwrap();
        context
            .update_component(*button, |button, _| {
                button.label = "收藏".into();
                Arc::make_mut(&mut button.style.layout).hidden = false;
            })
            .unwrap();
    }

    assert_eq!(context.world().generation(), generation);
    assert!(!context.world().has_pending_work());
    assert!(context.take_system_work().is_empty());
}

#[test]
fn an_unchanged_update_skips_projection_and_reproject_component_forces_it() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let probe = context
        .create_component(document, CountingProbe::default())
        .unwrap();
    let before = projections();

    context
        .update_component(probe, |probe, _| probe.value = String::new())
        .unwrap();
    assert_eq!(projections(), before, "an equal component is not projected");

    context.reproject_component(probe).unwrap();
    assert_eq!(projections(), before + 1);

    context
        .update_component(probe, |probe, _| probe.value = "changed".into())
        .unwrap();
    assert_eq!(projections(), before + 2);
    assert_eq!(context.world().text(probe.stable_id()), Some("changed"));
}

#[test]
fn an_update_that_only_mutates_emits_or_dispatches_keeps_its_effect() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let root = context
        .create_component(document, Stack::column(0.0))
        .unwrap();
    let probe = context
        .create_detached_component(document, CountingProbe::default())
        .unwrap();
    context.append_child(root, probe).unwrap();

    // A queued mutation commits even though the component is unchanged.
    context
        .update_component(probe, |_, cx| {
            let id = cx.entity().stable_id();
            cx.mutations().park_subtree(id);
        })
        .unwrap();
    assert!(!context.world().is_mounted(probe.stable_id()));

    // An emitted event reaches its handler.
    let seen = Arc::new(Mutex::new(0));
    let counter = Arc::clone(&seen);
    context
        .on(probe, move |_, _: &crate::Activate, _| {
            *counter.lock().unwrap() += 1
        })
        .unwrap();
    context
        .update_component(probe, |_, cx| cx.emit(crate::Activate))
        .unwrap();
    assert_eq!(*seen.lock().unwrap(), 1);

    // A dispatched program message is delivered though nothing projects.
    context
        .update_component(probe, |_, cx| cx.dispatch_program(1_u32))
        .unwrap();
    context
        .update_component(probe, |_, cx| cx.dispatch_program(2_u32))
        .unwrap();
    let messages = context.take_program_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].downcast_ref::<u32>(), Some(&2));
}

#[test]
fn a_component_that_always_reprojects_projects_on_an_unchanged_update() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let probe = context.create_component(document, AlwaysProbe).unwrap();
    let before = projections();
    context.update_component(probe, |_, _| {}).unwrap();
    assert_eq!(projections(), before + 1);
}

/// Observer handlers change their view in place. Nothing else projects that
/// state: the observer's next unchanged write takes the no-op path.
#[test]
fn an_observer_changed_by_its_handler_is_projected() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let button = context
        .create_component(document, Button::new("go"))
        .unwrap();
    let probe = context
        .create_component(document, CountingProbe::default())
        .unwrap();
    context
        .observe(button, probe, |probe, _: &crate::Activate, _| {
            probe.value = "activated".into();
        })
        .unwrap();

    assert!(context.activate_button(button).unwrap());

    assert_eq!(context.world().text(probe.stable_id()), Some("activated"));
}

/// A switch flips itself on click. The application's next write of the value
/// it holds must land, even though the application's value did not change.
#[test]
fn a_switch_the_user_flipped_takes_the_application_value_back() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let switch = context
        .create_component(document, Switch::new("", false))
        .unwrap();
    assert!(context.toggle_switch(switch).unwrap());
    assert!(context.read(switch, |switch| switch.checked).unwrap());

    context
        .update_component(switch, |switch, _| switch.checked = false)
        .unwrap();

    assert!(!context.read(switch, |switch| switch.checked).unwrap());
    assert_eq!(
        context
            .world()
            .accessibility(switch.stable_id())
            .and_then(|state| state.checked),
        Some(false)
    );
}

/// Refreshing a row re-appends and re-parks what is already in place. Those
/// writes must not reach the world as moves.
#[test]
fn putting_a_child_back_where_it_is_or_reparking_it_is_no_work() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let root = context.create_component(document, Stack::row(0.0)).unwrap();
    let first = context
        .create_detached_component(document, Text::new("first"))
        .unwrap();
    let last = context
        .create_detached_component(document, Text::new("last"))
        .unwrap();
    let parked = context
        .create_detached_component(document, Text::new("parked"))
        .unwrap();
    context.append_child(root, first).unwrap();
    context.append_child(root, last).unwrap();
    let _ = context.take_system_work();
    let generation = context.world().generation();

    context.append_child(root, last).unwrap();
    let mut queue = MutationQueue::new();
    queue.insert(root.stable_id(), first.stable_id(), Some(last.stable_id()));
    queue.park_subtree(parked.stable_id());
    context.commit_mutations(queue).unwrap();
    context
        .update_component(parked, |_, cx| {
            let id = cx.entity().stable_id();
            cx.mutations().park_subtree(id);
        })
        .unwrap();

    assert_eq!(context.world().generation(), generation);
    assert!(!context.world().has_pending_work());
    assert!(context.take_system_work().is_empty());

    // Appending a child that is not last is a real move.
    context.append_child(root, first).unwrap();
    assert_eq!(
        context.world().node(root.stable_id()).unwrap().children,
        vec![last.stable_id(), first.stable_id()]
    );
    assert!(context.world().has_pending_work());
}

/// Assemblers run on every write and re-append their parts in order.
#[test]
fn rewriting_an_assembled_composite_with_its_own_values_is_no_work() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let tab = context
        .create_component(document, crate::FileTab::new("main.rs"))
        .unwrap();
    let path = context
        .create_component(document, crate::PathField::new("/tmp"))
        .unwrap();
    // Creation does not assemble; the first write does.
    context.reproject_component(tab).unwrap();
    context.reproject_component(path).unwrap();
    let _ = context.take_system_work();
    let generation = context.world().generation();

    context
        .update_component(tab, |tab, _| tab.label = "main.rs".into())
        .unwrap();
    context
        .update_component(path, |path, _| path.value = "/tmp".into())
        .unwrap();
    context.reproject_component(tab).unwrap();
    context.reproject_component(path).unwrap();

    assert_eq!(context.world().generation(), generation);
    assert!(context.take_system_work().is_empty());
}

/// Projections that emitted a mutation the world then ignored, on every
/// projection: each such write bumped the generation of an unchanged tree.
#[test]
fn reprojecting_an_unchanged_card_text_area_or_closed_search_writes_nothing() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let card = context.create_component(document, Card::new()).unwrap();
    let area = context
        .create_component(document, TextArea::new("draft"))
        .unwrap();
    let search = context
        .create_component(
            document,
            crate::SearchDropdown::new(Some("a")).options([
                crate::SearchDropdownOption::new("a", "Alpha"),
                crate::SearchDropdownOption::new("b", "Beta"),
            ]),
        )
        .unwrap();
    let generation = context.world().generation();

    context.reproject_component(card).unwrap();
    context.reproject_component(area).unwrap();
    context.reproject_component(search).unwrap();

    assert_eq!(context.world().generation(), generation);
}

/// Closing removes the query input, which clears the node text; the label
/// has to go in after it, not before.
#[test]
fn closing_a_search_dropdown_leaves_its_label_as_the_node_text() {
    let mut context = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let search = context
        .create_component(
            document,
            crate::SearchDropdown::new(Some("a"))
                .options([
                    crate::SearchDropdownOption::new("a", "Alpha"),
                    crate::SearchDropdownOption::new("b", "Beta"),
                ])
                .query("Be")
                .opened(true),
        )
        .unwrap();
    assert_eq!(context.world().text(search.stable_id()), Some("Be"));

    context
        .update_component(search, |search, _| search.opened = false)
        .unwrap();

    assert_eq!(context.world().text(search.stable_id()), Some("Alpha"));
}

/// `BuiltinComponents::Typed` installs identities up front and each type's
/// hooks when its first node is created, from any creation path.
#[test]
fn a_typed_context_installs_hooks_with_the_types_it_creates() {
    let mut context = AppContext::typed();
    let document = DocumentId::new(1).unwrap();

    // A button built from its type: stamped, and activatable.
    let button = context
        .create_component(document, Button::new("保存"))
        .unwrap();
    assert_eq!(
        context
            .world()
            .component_type(button.stable_id())
            .map(|id| id.as_str()),
        Some("nana.button")
    );
    let hits = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = Arc::clone(&hits);
    context
        .on(button, move |_, _: &Activate, _| {
            seen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        })
        .unwrap();
    assert!(context.activate_node(button.stable_id()).unwrap());
    assert_eq!(hits.load(std::sync::atomic::Ordering::Relaxed), 1);

    // A composite's own children (tab options) get their activation too.
    let tabs = context
        .create_component(
            document,
            Tabs::new("code").options([
                crate::TabOption::new("code", "代码"),
                crate::TabOption::new("docs", "文档"),
            ]),
        )
        .unwrap();
    let options = context
        .world()
        .node(tabs.stable_id())
        .unwrap()
        .children
        .to_vec();
    assert!(!options.is_empty());
    assert!(context.activate_node(*options.last().unwrap()).unwrap());
    assert_eq!(
        context
            .read(tabs, |tabs| tabs.selected.clone())
            .unwrap()
            .as_deref(),
        Some("docs")
    );

    // A self-assembling composite still assembles on write.
    let chip = context
        .create_component(document, Chip::new("附件"))
        .unwrap();
    assert!(context.read(chip, |chip| chip.close).unwrap().is_none());
    context
        .update_component(chip, |chip, _| chip.dismissible = true)
        .unwrap();
    assert!(context.read(chip, |chip| chip.close).unwrap().is_some());
}

#[test]
fn a_typed_context_refuses_to_build_a_builtin_from_a_tag() {
    let mut context = AppContext::typed();
    let document = DocumentId::new(1).unwrap();
    let id = StableNodeId::new(42).unwrap();
    let mut create = MutationQueue::new();
    create.create(
        id,
        document,
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    context.commit_mutations(create).unwrap();
    let type_id = context.resolve_component_tag("button").unwrap().clone();
    let layout = Arc::new(nana_ui_core::LayoutStyle::default());
    let spec = crate::SemanticSpec::from_parts(&type_id, &layout);
    let mut mutations = MutationQueue::new();
    assert!(matches!(
        context.prepare_semantic_binding(id, &spec, &mut mutations),
        Err(FrameworkError::InvalidComponentType)
    ));
    // The full context builds it (`from_world` is full under every test mode).
    let mut full = AppContext::from_world(crate::UiWorld::new());
    let mut create = MutationQueue::new();
    create.create(
        id,
        document,
        NodeKind::Element {
            tag: "button".into(),
        },
    );
    full.commit_mutations(create).unwrap();
    let mut mutations = MutationQueue::new();
    assert!(
        full.prepare_semantic_binding(id, &spec, &mut mutations)
            .is_ok()
    );
}
