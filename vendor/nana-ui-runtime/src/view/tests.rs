use super::*;
use crate::{
    AppContext, Button, Checkbox, DocumentId, Entity, LayoutViewport, LengthSpec, RangeField,
    StableNodeId, Stack, Text, TextChanged, TextInput,
};

fn setup() -> (AppContext, DocumentId, StableNodeId) {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    (cx, document, parent.stable_id())
}

fn node<C: crate::ComponentView>(cx: &AppContext, root: StableNodeId, path: &str) -> Entity<C> {
    let id = cx
        .resolve_assembly_path(root, path)
        .unwrap_or_else(|| panic!("no node at {path}"));
    Entity::from_stable_id(id)
}

fn text_of(cx: &AppContext, entity: Entity<Text>) -> String {
    cx.read(entity, |text| text.value.clone()).unwrap()
}

fn children(cx: &AppContext, id: StableNodeId) -> Vec<StableNodeId> {
    cx.world().node(id).unwrap().children.to_vec()
}

fn delta(before: ReactiveStats) -> ReactiveStats {
    let now = reactive_stats();
    ReactiveStats {
        signals: now.signals.wrapping_sub(before.signals),
        effects: now.effects.wrapping_sub(before.effects),
        scopes: now.scopes.wrapping_sub(before.scopes),
        signal_writes: now.signal_writes - before.signal_writes,
        effects_run: now.effects_run - before.effects_run,
        flushes: now.flushes - before.flushes,
        nodes_patched: now.nodes_patched - before.nodes_patched,
        commits: now.commits - before.commits,
        static_deps_mismatches: now.static_deps_mismatches - before.static_deps_mismatches,
    }
}

fn counter() -> impl IntoView {
    let count = signal(0u64);
    column()
        .gap(12.0)
        .children((
            crate::text!("计数 {count}").key("value"),
            button("加一")
                .key("inc")
                .on_activate(move || count.update(|c| *c += 1)),
        ))
        .key("counter")
}

#[test]
fn sfc_source_site_is_exposed_without_changing_rust_locations() {
    let (mut cx, _, parent) = setup();
    let value = signal(String::from("a"));
    static SOURCE: ViewSource = ViewSource {
        element: SourceLocation::new("View.vue", 3, 5),
        fields: &[("value", SourceLocation::new("View.vue", 3, 12))],
    };
    let view = cx
        .mount_view(parent, || text(value).source_site(&SOURCE))
        .unwrap();
    let info = cx.view_bindings(view.roots()[0]).expect("binding info");
    assert_eq!(info.source, Some(SourceLocation::new("View.vue", 3, 5)));
    assert_eq!(
        info.source_fields[0].1,
        SourceLocation::new("View.vue", 3, 12)
    );
    assert_eq!(info.element.file(), file!());
    assert_eq!(info.fields[0].1.file(), file!());
}

#[test]
fn a_click_updates_exactly_the_bound_node_in_one_commit() {
    let (mut cx, _, parent) = setup();
    let view = cx.mount_view(parent, counter).unwrap();
    let root = view.roots()[0];
    let value = node::<Text>(&cx, root, "value");
    let inc = node::<Button>(&cx, root, "inc");
    assert_eq!(text_of(&cx, value), "计数 0");

    let before = reactive_stats();
    cx.activate_button(inc).unwrap();
    cx.flush_reactive().unwrap();
    let delta = delta(before);
    assert_eq!(text_of(&cx, value), "计数 1");
    assert_eq!(delta.nodes_patched, 1);
    assert_eq!(delta.commits, 1);
    assert_eq!(delta.effects_run, 1);
}

#[test]
fn a_frame_flushes_writes_made_outside_input() {
    let (mut cx, _, parent) = setup();
    let count = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let value = signal(1);
            count.set(Some(value));
            text!("{value}").key("value")
        })
        .unwrap();
    let value = Entity::<Text>::from_stable_id(view.roots()[0]);
    count.get().unwrap().set(2);
    assert!(cx.has_pending_reactive());
    cx.take_system_work();
    assert_eq!(text_of(&cx, value), "2");
    assert!(!cx.has_pending_reactive());
}

#[test]
fn a_flush_with_nothing_written_runs_nothing() {
    let (mut cx, _, parent) = setup();
    cx.mount_view(parent, counter).unwrap();
    let before = reactive_stats();
    cx.flush_reactive().unwrap();
    cx.take_system_work();
    assert_eq!(delta(before).effects_run, 0);
}

#[test]
fn constant_props_create_no_effect() {
    let (mut cx, _, parent) = setup();
    let before = reactive_stats();
    cx.mount_view(parent, || {
        column()
            .gap(4.0)
            .children((text("静态"), button("按钮").disabled(true)))
    })
    .unwrap();
    let delta = delta(before);
    assert_eq!(delta.effects, 0);
    assert_eq!(delta.signals, 0);
}

#[test]
fn a_hundred_bound_nodes_commit_once() {
    let (mut cx, _, parent) = setup();
    let shared = std::cell::Cell::new(None);
    cx.mount_view(parent, || {
        let count = signal(0);
        shared.set(Some(count));
        column().children(
            (0..100)
                .map(move |_| text(count.map(|c| c.to_string())))
                .collect::<Vec<_>>(),
        )
    })
    .unwrap();
    let before = reactive_stats();
    shared.get().unwrap().set(7);
    cx.flush_reactive().unwrap();
    let delta = delta(before);
    assert_eq!(delta.nodes_patched, 100);
    assert_eq!(delta.commits, 1);
}

#[test]
fn three_bindings_on_one_node_stage_it_once() {
    let (mut cx, _, parent) = setup();
    let signals = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let label = signal(String::from("保存"));
            let disabled = signal(false);
            let loading = signal(false);
            signals.set(Some((label, disabled, loading)));
            button(label).disabled(disabled).loading(loading)
        })
        .unwrap();
    let (label, disabled, loading) = signals.get().unwrap();
    let before = reactive_stats();
    label.set("保存中".into());
    disabled.set(true);
    loading.set(true);
    cx.flush_reactive().unwrap();
    let delta = delta(before);
    assert_eq!(delta.effects_run, 1, "one merged effect per node");
    assert_eq!(delta.nodes_patched, 1);
    assert_eq!(delta.commits, 1);
    let button = Entity::<Button>::from_stable_id(view.roots()[0]);
    let state = cx
        .read(button, |b| (b.label.clone(), b.disabled, b.loading))
        .unwrap();
    assert_eq!(state, ("保存中".into(), true, true));
}

#[derive(Clone)]
struct Todo {
    id: u32,
    title: Signal<String>,
}

#[test]
fn a_keyed_list_keeps_row_identity_through_insert_remove_and_reorder() {
    let (mut cx, _, parent) = setup();
    let items = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let todos = signal(
                (1..=3)
                    .map(|id| Todo {
                        id,
                        title: signal(format!("任务 {id}")),
                    })
                    .collect::<Vec<_>>(),
            );
            items.set(Some(todos));
            each(todos, |todo| todo.id, |todo| text(todo.title))
        })
        .unwrap();
    let todos = items.get().unwrap();
    let list = view.roots()[0];
    let initial = children(&cx, list);
    assert_eq!(initial.len(), 3);

    // Reorder: same nodes, new order.
    todos.update(|list| list.reverse());
    cx.flush_reactive().unwrap();
    let reversed = children(&cx, list);
    assert_eq!(reversed, initial.iter().rev().copied().collect::<Vec<_>>());

    // Remove the middle row, insert a new one at the front.
    let fresh = view_scope_signal("任务 9");
    todos.update(|list| {
        list.remove(1);
        list.insert(
            0,
            Todo {
                id: 9,
                title: fresh,
            },
        );
    });
    cx.flush_reactive().unwrap();
    let after = children(&cx, list);
    assert_eq!(after.len(), 3);
    assert_eq!(&after[1..], &[reversed[0], reversed[2]]);
    assert!(
        !cx.world().contains(reversed[1]),
        "removed row is despawned"
    );
    assert_eq!(text_of(&cx, Entity::from_stable_id(after[0])), "任务 9");
}

/// A signal owned by no scope, like application state created at startup.
fn view_scope_signal(value: &str) -> Signal<String> {
    signal(value.to_owned())
}

#[test]
fn a_row_field_change_does_not_rerun_the_list() {
    let (mut cx, _, parent) = setup();
    let items = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let todos = signal(
                (1..=200)
                    .map(|id| Todo {
                        id,
                        title: signal(format!("任务 {id}")),
                    })
                    .collect::<Vec<_>>(),
            );
            items.set(Some(todos));
            each(todos, |todo| todo.id, |todo| text(todo.title))
        })
        .unwrap();
    let todos = items.get().unwrap();
    let before = reactive_stats();
    todos.with_untracked(|list| list[57].title.set("改过".into()));
    cx.flush_reactive().unwrap();
    let delta = delta(before);
    assert_eq!(delta.effects_run, 1, "only the row's text binding ran");
    assert_eq!(delta.nodes_patched, 1);
    let row = children(&cx, view.roots()[0])[57];
    assert_eq!(text_of(&cx, Entity::from_stable_id(row)), "改过");
}

#[test]
fn when_builds_and_drops_its_branch_and_disposes_its_scope() {
    let (mut cx, _, parent) = setup();
    let flag = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let open = signal(false);
            flag.set(Some(open));
            when(open, || {
                let local = signal(1);
                text!("打开 {local}")
            })
            .otherwise(|| text("关闭"))
        })
        .unwrap();
    let open = flag.get().unwrap();
    let block = view.roots()[0];
    let closed = children(&cx, block);
    assert_eq!(text_of(&cx, Entity::from_stable_id(closed[0])), "关闭");
    let baseline = reactive_stats();

    open.set(true);
    cx.flush_reactive().unwrap();
    let opened = children(&cx, block);
    assert_eq!(opened.len(), 1);
    assert!(!cx.world().contains(closed[0]));
    assert_eq!(text_of(&cx, Entity::from_stable_id(opened[0])), "打开 1");

    open.set(false);
    cx.flush_reactive().unwrap();
    assert!(!cx.world().contains(opened[0]));
    let back = reactive_stats();
    assert_eq!(
        (back.signals, back.effects, back.scopes),
        (baseline.signals, baseline.effects, baseline.scopes),
        "the dropped branch released its signal, effect and scope"
    );
}

#[test]
fn unmount_releases_every_signal_effect_scope_and_handler() {
    let (mut cx, _, parent) = setup();
    let handlers = cx.event_handler_count();
    let before = reactive_stats();
    let view = cx
        .mount_view(parent, || {
            let todos = signal(vec![1u32, 2, 3]);
            let open = signal(true);
            column().children((
                counter(),
                each(todos, |id| *id, |id| text(format!("{id}"))),
                when(open, || text("x")),
            ))
        })
        .unwrap();
    assert!(cx.event_handler_count() > handlers);
    view.unmount(&mut cx).unwrap();
    let after = reactive_stats();
    assert_eq!(
        (after.signals, after.effects, after.scopes),
        (before.signals, before.effects, before.scopes)
    );
    assert_eq!(cx.event_handler_count(), handlers);
    assert!(children(&cx, parent).is_empty());
}

#[test]
fn despawning_a_mounted_root_disposes_the_view() {
    let (mut cx, _, parent) = setup();
    let before = reactive_stats();
    let view = cx.mount_view(parent, counter).unwrap();
    let mut despawn = crate::MutationQueue::new();
    despawn.despawn_subtree(view.roots()[0]);
    cx.commit_mutations(despawn).unwrap();
    let after = reactive_stats();
    assert_eq!(
        (after.signals, after.effects, after.scopes),
        (before.signals, before.effects, before.scopes)
    );
}

#[test]
fn dropping_the_context_releases_its_views() {
    let before = reactive_stats();
    {
        let (mut cx, _, parent) = setup();
        cx.mount_view(parent, counter).unwrap();
        cx.mount_view(parent, || {
            each(signal(vec![1u8, 2]), |n| *n, |n| text(format!("{n}")))
        })
        .unwrap();
    }
    let after = reactive_stats();
    assert_eq!(
        (after.signals, after.effects, after.scopes),
        (before.signals, before.effects, before.scopes)
    );
}

#[test]
fn models_write_back_through_their_signal() {
    let (mut cx, _, parent) = setup();
    let signals = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let volume = signal(0.25);
            let name = signal(String::new());
            let agreed = signal(false);
            signals.set(Some((volume, name, agreed)));
            column().children((
                slider(0.0, 1.0, 0.05).model(volume).key("volume"),
                text_input().model(name).key("name"),
                checkbox("同意").model(agreed).key("agreed"),
            ))
        })
        .unwrap();
    let (volume, name, agreed) = signals.get().unwrap();
    let root = view.roots()[0];
    let range = node::<RangeField>(&cx, root, "volume");
    assert_eq!(cx.read(range, |r| r.value).unwrap(), 0.25);

    cx.set_range_value(range, 0.5).unwrap();
    cx.flush_reactive().unwrap();
    assert_eq!(volume.get_untracked(), 0.5);

    volume.set(0.75);
    cx.flush_reactive().unwrap();
    assert_eq!(cx.read(range, |r| r.value).unwrap(), 0.75);

    let input = node::<TextInput>(&cx, root, "name");
    cx.update_component(input, |field, cx| {
        field.state.replace_value("小明");
        cx.emit(TextChanged {
            value: field.state.value.clone(),
            selection: field.state.selection,
        });
    })
    .unwrap();
    let before = reactive_stats();
    cx.flush_reactive().unwrap();
    assert_eq!(name.get_untracked(), "小明");
    assert_eq!(
        delta(before).nodes_patched,
        0,
        "echoing the edit back leaves the field alone"
    );

    let check = node::<Checkbox>(&cx, root, "agreed");
    cx.toggle_checkbox(check).unwrap();
    cx.flush_reactive().unwrap();
    assert!(agreed.get_untracked());
}

#[test]
fn visible_takes_a_node_out_of_layout_without_dropping_it() {
    let (mut cx, document, parent) = setup();
    let flag = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let shown = signal(true);
            flag.set(Some(shown));
            column().children((
                widget(Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(40.0))))
                    .visible(shown)
                    .key("first"),
                widget(Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(40.0))))
                    .key("second"),
            ))
        })
        .unwrap();
    let root = view.roots()[0];
    let first = cx.resolve_assembly_path(root, "first").unwrap();
    let second = cx.resolve_assembly_path(root, "second").unwrap();
    let viewport = LayoutViewport::new(320.0, 240.0);
    cx.layout_document(document, viewport).unwrap();
    let top = cx.world().layout_box(first).unwrap().y;
    assert_eq!(cx.world().layout_box(second).unwrap().y, top + 40.0);

    flag.get().unwrap().set(false);
    cx.flush_reactive().unwrap();
    cx.layout_document(document, viewport).unwrap();
    assert!(cx.world().contains(first));
    assert_eq!(cx.world().layout_box(second).unwrap().y, top);
}

#[test]
fn computed_props_follow_their_inputs() {
    let (mut cx, _, parent) = setup();
    let source = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let items = signal(vec![1, 2, 3]);
            source.set(Some(items));
            let total = computed(move || items.with(|v| v.iter().sum::<i32>()));
            text!("合计 {total}")
        })
        .unwrap();
    let text = Entity::<Text>::from_stable_id(view.roots()[0]);
    assert_eq!(text_of(&cx, text), "合计 6");
    source.get().unwrap().update(|v| v.push(4));
    cx.flush_reactive().unwrap();
    assert_eq!(text_of(&cx, text), "合计 10");
}

#[test]
fn effects_that_requeue_each_other_stop_at_the_round_limit() {
    let (mut cx, _, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            let ping = signal(0u64);
            watch_effect(move || {
                let value = ping.get();
                ping.set(value + 1);
            });
            text("x")
        })
        .unwrap();
    cx.flush_reactive().unwrap();
    assert!(
        !cx.has_pending_reactive(),
        "the rest of the queue was dropped"
    );
    view.unmount(&mut cx).unwrap();
}

#[cfg(feature = "reactive-trace")]
#[test]
fn why_updated_names_the_write_that_caused_a_patch() {
    let (mut cx, _, parent) = setup();
    let view = cx.mount_view(parent, counter).unwrap();
    let root = view.roots()[0];
    let value = node::<Text>(&cx, root, "value");
    let inc = node::<Button>(&cx, root, "inc");
    cx.activate_button(inc).unwrap();
    cx.flush_reactive().unwrap();
    let why = cx
        .why_updated(value.stable_id())
        .expect("value was patched");
    assert_eq!(why.bindings.len(), 1);
    assert_eq!(why.bindings[0].0, "Text.value");
    assert_eq!(why.causes.len(), 1);
    assert_eq!(why.causes[0].written_at.file(), file!());
    assert!(why.causes[0].signal_created.is_some());
}

#[test]
fn a_routed_click_flushes_its_bindings_and_invalidates_the_frame() {
    use nana_ui_input::PointerPhase;

    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let view = cx.mount_view_root(document, counter).unwrap();
    let root = view.roots()[0];
    let value = node::<Text>(&cx, root, "value");
    let inc = node::<Button>(&cx, root, "inc");
    cx.layout_document(document, LayoutViewport::new(320.0, 240.0))
        .unwrap();
    cx.take_system_work();
    cx.rebuild_hit_test(document);
    let target = cx.world().layout_box(inc.stable_id()).unwrap();
    let (x, y) = (
        target.x + target.width / 2.0,
        target.y + target.height / 2.0,
    );

    let mut input = crate::HeadlessInput::bind(&mut cx, document);
    input.pointer(&mut cx, PointerPhase::Down, x, y).unwrap();
    let up = input.pointer(&mut cx, PointerPhase::Up, x, y).unwrap();
    assert_eq!(text_of(&cx, value), "计数 1");
    assert!(up.invalidated_work);
    assert!(!cx.has_pending_reactive());
}

#[derive(Clone, PartialEq, Debug)]
struct Theme(&'static str);

#[test]
fn provided_values_reach_rows_and_branches_built_later() {
    let (mut cx, _, parent) = setup();
    let source = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            provide(Theme("深色"));
            let rows = signal(vec![1u32]);
            let open = signal(false);
            source.set(Some((rows, open)));
            let label = |tag: &str| {
                let theme = use_context::<Theme>().map_or("无", |theme| theme.0);
                text(format!("{tag} {theme}"))
            };
            column().children((
                each(rows, |n| *n, move |n| label(&format!("行{n}"))),
                when(open, move || {
                    provide(Theme("浅色"));
                    column().children((label("分支"),))
                }),
            ))
        })
        .unwrap();
    let (rows, open) = source.get().unwrap();
    rows.update(|list| list.push(2));
    open.set(true);
    cx.flush_reactive().unwrap();
    let root = view.roots()[0];
    let [list, block] = children(&cx, root)[..] else {
        panic!("two blocks");
    };
    let texts: Vec<String> = children(&cx, list)
        .into_iter()
        .map(|id| text_of(&cx, Entity::from_stable_id(id)))
        .collect();
    assert_eq!(
        texts,
        ["行1 深色", "行2 深色"],
        "a row built later still sees the mount's value"
    );
    let branch = children(&cx, children(&cx, block)[0])[0];
    assert_eq!(
        text_of(&cx, Entity::from_stable_id(branch)),
        "分支 浅色",
        "the nearest provider wins"
    );
    assert_eq!(
        use_context::<Theme>(),
        None,
        "outside any scope nothing is provided"
    );
}

#[test]
fn a_constant_prop_is_written_once_and_binds_nothing() {
    let (mut cx, _, parent) = setup();
    let before = reactive_stats();
    let view = cx
        .mount_view(parent, || {
            let title = constant(String::from("标题"));
            column().children((text(title), text!("副 {title}")))
        })
        .unwrap();
    let delta = delta(before);
    assert_eq!(delta.signals, 1, "the constant's cell");
    assert_eq!(
        delta.effects, 1,
        "only the interpolation closure, which reads no dependency"
    );
    let root = view.roots()[0];
    let kids = children(&cx, root);
    assert_eq!(text_of(&cx, Entity::from_stable_id(kids[0])), "标题");
    assert_eq!(text_of(&cx, Entity::from_stable_id(kids[1])), "副 标题");
    view.unmount(&mut cx).unwrap();
    assert_eq!(reactive_stats().signals - before.signals, 0);
}

#[cfg(debug_assertions)]
#[test]
fn a_checked_binding_reports_reads_outside_its_declared_dependencies() {
    let (mut cx, _, parent) = setup();
    let signals = std::cell::Cell::new(None);
    cx.mount_view(parent, || {
        let declared = signal(1);
        let hidden = signal(2);
        let doubled = computed(move || declared.get() * 2);
        signals.set(Some((declared, hidden)));
        column().children((
            text(__checked("ok", [declared.dep()], move || {
                declared.get().to_string()
            })),
            text(__checked("wrong", [declared.dep()], move || {
                (declared.get() + hidden.get()).to_string()
            })),
            // Recomputing `doubled` reads `declared` in its own frame.
            text(__checked("computed", [doubled.dep()], move || {
                doubled.get().to_string()
            })),
        ))
    })
    .unwrap();
    let before = reactive_stats();
    let (declared, _) = signals.get().unwrap();
    declared.set(5);
    cx.flush_reactive().unwrap();
    assert_eq!(
        reactive_stats().static_deps_mismatches - before.static_deps_mismatches,
        1,
        "only the binding that read `hidden` is reported"
    );
}

#[test]
fn a_component_key_names_its_root_over_the_root_s_own() {
    let (mut cx, _, parent) = setup();
    let item = |label: &'static str| text(label).key("inner");
    let view = cx
        .mount_view(parent, move || {
            column().children((keyed("a", item("A")), keyed("b", item("B")), text("C")))
        })
        .unwrap();
    let root = view.roots()[0];
    assert_eq!(text_of(&cx, node(&cx, root, "a")), "A");
    assert_eq!(text_of(&cx, node(&cx, root, "b")), "B");
    assert_eq!(children(&cx, root).len(), 3);
    assert!(cx.resolve_assembly_path(root, "inner").is_none());
}

#[test]
fn on_mount_runs_once_the_nodes_are_in_the_tree() {
    use std::sync::{Arc, Mutex};
    let (mut cx, _, parent) = setup();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let shown_flag = std::cell::Cell::new(None);
    let (log, flag) = (Arc::clone(&seen), &shown_flag);
    cx.mount_view(parent, move || {
        let shown = signal(false);
        flag.set(Some(shown));
        let (outer, inner) = (Arc::clone(&log), Arc::clone(&log));
        on_mount(move |_| outer.lock().unwrap().push("mount"));
        column().children(when(shown, move || {
            let inner = Arc::clone(&inner);
            let node = node_ref();
            // The node is built and placed under its container by now.
            on_mount(move |cx| {
                let id = node.get_untracked().expect("the ref is set when built");
                if cx.world().node(id).is_some_and(|n| n.parent.is_some()) {
                    inner.lock().unwrap().push("branch");
                }
            });
            text("shown").node_ref(node)
        }))
    })
    .unwrap();
    assert_eq!(*seen.lock().unwrap(), ["mount"]);
    let shown = shown_flag.get().unwrap();
    shown.set(true);
    cx.flush_reactive().unwrap();
    assert_eq!(*seen.lock().unwrap(), ["mount", "branch"]);
    shown.set(false);
    cx.flush_reactive().unwrap();
    shown.set(true);
    cx.flush_reactive().unwrap();
    assert_eq!(
        *seen.lock().unwrap(),
        ["mount", "branch", "branch"],
        "a rebuilt branch mounts again"
    );
}

#[derive(Clone)]
struct VirtualRow {
    id: u32,
    title: String,
}

/// Rows built under the list, in order.
fn virtual_rows(cx: &AppContext, scroll: StableNodeId) -> Vec<String> {
    let list = children(cx, scroll)[0];
    children(cx, list)
        .into_iter()
        .map(|slot| {
            // placement container → row slot → the row's text
            let row_slot = children(cx, slot)[0];
            let text = children(cx, row_slot)[0];
            text_of(cx, Entity::from_stable_id(text))
        })
        .collect()
}

#[test]
fn each_virtual_builds_only_the_rows_in_view() {
    let (mut cx, document, parent) = setup();
    let items_flag = std::cell::Cell::new(None);
    let before = reactive_stats();
    let view = cx
        .mount_view(parent, || {
            let items = signal(
                (0..10_000)
                    .map(|id| VirtualRow {
                        id,
                        title: format!("行 {id}"),
                    })
                    .collect::<Vec<_>>(),
            );
            items_flag.set(Some(items));
            each_virtual(items, |row| row.id, 20.0, |row| text(row.title))
                .overscan(0.0)
                .height(200.0)
        })
        .unwrap();
    let scroll = view.roots()[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    // The first layout announces the viewport; the window follows it.
    cx.layout_document(document, viewport).unwrap();
    cx.flush_reactive().unwrap();
    let rows = virtual_rows(&cx, scroll);
    assert_eq!(rows.first().map(String::as_str), Some("行 0"));
    assert!(
        (10..=12).contains(&rows.len()),
        "a 200 px viewport of 20 px rows: {} rows",
        rows.len()
    );
    assert!(
        reactive_stats().scopes - before.scopes < 20,
        "only built rows own scopes"
    );

    cx.layout_document(document, viewport).unwrap();
    cx.scroll_to(
        Entity::from_stable_id(scroll),
        crate::ScrollOffset { x: 0.0, y: 2_000.0 },
    )
    .unwrap();
    cx.flush_reactive().unwrap();
    let rows = virtual_rows(&cx, scroll);
    assert!(rows.contains(&"行 100".to_owned()), "{rows:?}");
    assert!(
        !rows.contains(&"行 0".to_owned()),
        "scrolled away: {rows:?}"
    );

    // A data change moves the window's rows with it.
    items_flag.get().unwrap().update(|items| {
        items.retain(|row| row.id % 2 == 0);
    });
    cx.flush_reactive().unwrap();
    let rows = virtual_rows(&cx, scroll);
    assert!(
        rows.iter()
            .all(|row| { row.trim_start_matches("行 ").parse::<u32>().unwrap() % 2 == 0 }),
        "{rows:?}"
    );
}

#[test]
fn table_controls_bind_their_fields_and_model_both_ways() {
    use crate::{
        NumberChanged, NumberInput, Select, SelectChanged, SelectOption, Switch, ToggleChanged,
    };
    let (mut cx, _, parent) = setup();
    let signals = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let on = signal(false);
            let amount = signal(2.0f64);
            let choice: Signal<Option<std::sync::Arc<str>>> = signal(None);
            let activated = signal(0u32);
            signals.set(Some((on, amount, choice, activated)));
            column().children((
                switch("通知").model(on).key("switch"),
                number_input().model(amount).key("number"),
                select()
                    .options(vec![
                        SelectOption::new("a", "甲"),
                        SelectOption::new("b", "乙"),
                    ])
                    .model(choice)
                    .key("select"),
                list_item("行")
                    .on_activate(move || activated.update(|n| *n += 1))
                    .key("item"),
                progress(10.0).value(move || amount.get()).key("progress"),
            ))
        })
        .unwrap();
    let root = view.roots()[0];
    let (on, amount, choice, activated) = signals.get().unwrap();

    // signal → field
    on.set(true);
    amount.set(7.0);
    choice.set(Some("b".into()));
    cx.flush_reactive().unwrap();
    assert!(
        cx.read(node::<Switch>(&cx, root, "switch"), |s| s.checked)
            .unwrap()
    );
    assert_eq!(
        cx.read(node::<NumberInput>(&cx, root, "number"), |n| n.value())
            .unwrap(),
        7.0
    );
    assert_eq!(
        cx.read(node::<Select>(&cx, root, "select"), |s| s.value.clone())
            .unwrap()
            .as_deref(),
        Some("b")
    );
    assert_eq!(
        cx.read(node::<crate::Progress>(&cx, root, "progress"), |p| p.value)
            .unwrap(),
        7.0
    );

    // event → signal
    cx.update_component(node::<Switch>(&cx, root, "switch"), |_, cx| {
        cx.emit(ToggleChanged { checked: false })
    })
    .unwrap();
    cx.update_component(node::<NumberInput>(&cx, root, "number"), |_, cx| {
        cx.emit(NumberChanged { value: 3.0 })
    })
    .unwrap();
    cx.update_component(node::<Select>(&cx, root, "select"), |_, cx| {
        cx.emit(SelectChanged { value: "a".into() })
    })
    .unwrap();
    cx.activate_node(node::<crate::ListItem>(&cx, root, "item").stable_id())
        .unwrap();
    cx.flush_reactive().unwrap();
    assert!(!on.get_untracked());
    assert_eq!(amount.get_untracked(), 3.0);
    assert_eq!(choice.get_untracked().as_deref(), Some("a"));
    assert_eq!(activated.get_untracked(), 1);
}

#[test]
fn value_events_hand_their_handler_the_event_without_annotation() {
    use crate::{Switch, ToggleChanged};
    let (mut cx, _, parent) = setup();
    let seen = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let last = signal(None::<bool>);
            seen.set(Some(last));
            switch("通知")
                .on_change(move |e| last.set(Some(e.checked)))
                .key("switch")
        })
        .unwrap();
    let switch = Entity::<Switch>::from_stable_id(view.roots()[0]);
    cx.update_component(switch, |_, cx| cx.emit(ToggleChanged { checked: true }))
        .unwrap();
    cx.flush_reactive().unwrap();
    assert_eq!(seen.get().unwrap().get_untracked(), Some(true));
}

#[test]
fn a_measured_virtual_list_places_rows_at_their_laid_out_heights() {
    let (mut cx, document, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            let items = signal(
                (0..1_000)
                    .map(|id| VirtualRow {
                        id,
                        title: format!("行 {id}"),
                    })
                    .collect::<Vec<_>>(),
            );
            // Rows are 40 px; 20 is only the estimate.
            each_virtual(
                items,
                |row| row.id,
                20.0,
                |row| {
                    widget(
                        Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(40.0))),
                    )
                    .children(text(row.title))
                },
            )
            .measured()
            .overscan(0.0)
            .height(200.0)
        })
        .unwrap();
    let scroll = view.roots()[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    let list = |cx: &AppContext| children(cx, children(cx, scroll)[0]).len();
    // Layout announces the viewport; the estimate fills it with ten rows.
    cx.layout_document(document, viewport).unwrap();
    cx.flush_reactive().unwrap();
    assert!(
        (10..=12).contains(&list(&cx)),
        "{} rows by estimate",
        list(&cx)
    );
    // The next layout measures them at 40 px: half as many fill it.
    for _ in 0..3 {
        cx.layout_document(document, viewport).unwrap();
        cx.flush_reactive().unwrap();
    }
    assert!(
        (5..=7).contains(&list(&cx)),
        "{} rows once measured",
        list(&cx)
    );
}

#[derive(Clone, PartialEq, Debug)]
struct Task {
    id: u64,
    title: String,
    done: bool,
}

#[derive(Clone)]
struct Board {
    tasks: Vec<Task>,
}

/// What `#[derive(Store)]` writes for `Task` and `Board`.
trait TaskFields: StorePath<Value = Task> {
    fn title(self) -> Subfield<Self, String> {
        Subfield::__new(self, 1, |task| &task.title, |task| &mut task.title)
    }
    fn done(self) -> Subfield<Self, bool> {
        Subfield::__new(self, 2, |task| &task.done, |task| &mut task.done)
    }
}
impl<P: StorePath<Value = Task>> TaskFields for P {}

trait BoardFields: StorePath<Value = Board> {
    fn tasks(self) -> Subfield<Self, Vec<Task>> {
        Subfield::__new(self, 0, |board| &board.tasks, |board| &mut board.tasks)
    }
}
impl<P: StorePath<Value = Board>> BoardFields for P {}

fn task(id: u64) -> Task {
    Task {
        id,
        title: format!("任务 {id}"),
        done: false,
    }
}

fn mount_board(cx: &mut AppContext, parent: StableNodeId, n: u64) -> (Store<Board>, StableNodeId) {
    let board = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let state = store(Board {
                tasks: (1..=n).map(task).collect(),
            });
            board.set(Some(state));
            state.tasks().keyed(|task| task.id).each(|task| {
                row().gap(4.0).children((
                    text(task.title()).key("title"),
                    checkbox("").checked(task.done()).key("done"),
                ))
            })
        })
        .unwrap();
    (board.get().unwrap(), view.roots()[0])
}

#[test]
fn a_store_field_write_reaches_only_the_binding_that_read_it() {
    let (mut cx, _, parent) = setup();
    let (board, list) = mount_board(&mut cx, parent, 200);
    let before = reactive_stats();
    board.tasks().keyed(|task| task.id).at(&57).done().set(true);
    cx.flush_reactive().unwrap();
    let delta = delta(before);
    assert_eq!(delta.effects_run, 1, "only row 57's checkbox binding ran");
    assert_eq!(delta.nodes_patched, 1);
    let row = children(&cx, list)[56];
    let done: Entity<Checkbox> = node(&cx, row, "done");
    assert!(cx.read(done, |checkbox| checkbox.checked).unwrap());
}

#[test]
fn store_list_operations_move_rows_without_rereading_them() {
    let (mut cx, _, parent) = setup();
    let (board, list) = mount_board(&mut cx, parent, 3);
    let tasks = board.tasks();
    let initial = children(&cx, list);

    let before = reactive_stats();
    tasks.push(task(4));
    cx.flush_reactive().unwrap();
    let pushed = delta(before);
    assert_eq!(
        pushed.effects_run, 3,
        "the list and the new row's two bindings ran; no existing row did"
    );
    assert_eq!(pushed.nodes_patched, 0);
    let after_push = children(&cx, list);
    assert_eq!(&after_push[..3], &initial[..]);
    assert_eq!(after_push.len(), 4);

    // Reverse by key: the same rows, reordered, still reading their items.
    tasks.sort_by_key(|task| std::cmp::Reverse(task.id));
    cx.flush_reactive().unwrap();
    let reversed = children(&cx, list);
    assert_eq!(
        reversed,
        after_push.iter().rev().copied().collect::<Vec<_>>()
    );
    tasks
        .keyed(|task| task.id)
        .at(&1)
        .title()
        .set("第一".into());
    cx.flush_reactive().unwrap();
    let title: Entity<Text> = node(&cx, reversed[3], "title");
    assert_eq!(text_of(&cx, title), "第一");

    tasks.retain(|task| task.id != 2);
    cx.flush_reactive().unwrap();
    let kept = children(&cx, list);
    assert_eq!(kept.len(), 3);
    assert!(!cx.world().contains(reversed[2]), "task 2's row is gone");
}

#[test]
fn whole_value_readers_follow_writes_below_them() {
    let (mut cx, _, _) = setup();
    let board = store(Board {
        tasks: (1..=3).map(task).collect(),
    });
    let done = std::rc::Rc::new(std::cell::Cell::new(0));
    let seen = done.clone();
    let _watch = watch_effect(move || {
        seen.set(
            board
                .tasks()
                .with(|tasks| tasks.iter().filter(|task| task.done).count()),
        );
    });
    board.tasks().keyed(|task| task.id).at(&2).done().set(true);
    cx.flush_reactive().unwrap();
    assert_eq!(done.get(), 1);
}

#[test]
fn a_store_creates_triggers_only_for_paths_read_and_drops_those_of_removed_rows() {
    let before = reactive_stats();
    let board = store(Board {
        tasks: (1..=1000).map(task).collect(),
    });
    assert_eq!(delta(before).signals, 1, "one cell, no trigger yet");

    let tasks = board.tasks().keyed(|task| task.id);
    let scope = reactive::create_scope(None);
    reactive::with_scope(scope, || {
        let _watch = watch_effect(move || {
            tasks.at(&5).done().get();
        });
    });
    // The read path's deep trigger only.
    assert_eq!(delta(before).signals, 2);
    board.tasks().retain(|task| task.id != 5);
    assert!(tasks.at(&6).try_with(|_| ()).is_some());
    assert!(tasks.at(&5).try_with(|_| ()).is_none());
    reactive::dispose_scope(scope);
    assert_eq!(
        delta(before).signals,
        1,
        "the removed row's trigger is released; untracked reads make none"
    );
}

fn opacity_at(cx: &AppContext, id: StableNodeId, now: std::time::Duration) -> f32 {
    match cx
        .world()
        .presentation_motion_value(id, crate::AnimatableProperty::Opacity, now)
    {
        Some(crate::MotionValue::Scalar(value)) => value,
        other => panic!("opacity of {id:?}: {other:?}"),
    }
}

fn translate_y_at(cx: &AppContext, id: StableNodeId, now: std::time::Duration) -> f32 {
    match cx
        .world()
        .presentation_motion_value(id, crate::AnimatableProperty::Transform, now)
    {
        Some(crate::MotionValue::Transform(transform)) => transform.f,
        other => panic!("transform of {id:?}: {other:?}"),
    }
}

#[test]
fn a_transitioned_branch_leaves_before_it_is_despawned_and_the_next_one_enters() {
    use std::time::Duration;
    let (mut cx, document, parent) = setup();
    let start = Duration::from_secs(10);
    cx.advance_animations(start);
    let flag = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let open = signal(true);
            flag.set(Some(open));
            when(open, || text("面板").key("panel"))
                .otherwise(|| text("空").key("empty"))
                .transition(Transition::fade(Duration::from_millis(200)))
        })
        .unwrap();
    let container = view.roots()[0];
    let panel = children(&cx, container)[0];
    assert_eq!(opacity_at(&cx, panel, start), 1.0, "no enter at mount");

    flag.get().unwrap().set(false);
    cx.flush_reactive().unwrap();
    let now = children(&cx, container);
    assert_eq!(now.len(), 2, "the old branch stays while it leaves");
    assert_eq!(now[0], panel);
    let empty = now[1];
    assert_eq!(
        opacity_at(&cx, empty, start),
        0.0,
        "the new branch enters from 0"
    );
    let halfway = start + Duration::from_millis(100);
    let fading = opacity_at(&cx, panel, halfway);
    assert!(fading > 0.0 && fading < 1.0, "{fading}");

    cx.layout_document(document, LayoutViewport::new(320.0, 240.0))
        .unwrap();
    let bounds = cx.world().layout_box(panel).unwrap();
    let hit = cx
        .world()
        .hit_test(document, bounds.x + 1.0, bounds.y + 1.0);
    assert!(
        hit.is_none_or(|hit| !cx.world().is_descendant_or_self(hit, panel)),
        "a leaving branch takes no pointer input"
    );

    cx.advance_animations(start + Duration::from_millis(250));
    assert!(!cx.world().contains(panel), "despawned once the leave ends");
    assert_eq!(children(&cx, container), vec![empty]);
    assert_eq!(
        opacity_at(&cx, empty, start + Duration::from_millis(250)),
        1.0
    );
}

#[test]
fn a_transitioned_list_keeps_removed_rows_in_place_and_slides_the_rest() {
    use std::time::Duration;
    let (mut cx, document, parent) = setup();
    let start = Duration::from_secs(10);
    cx.advance_animations(start);
    let items = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let list = signal(vec![1u32, 2, 3]);
            items.set(Some(list));
            each(
                list,
                |id| *id,
                |id| {
                    widget(
                        Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(20.0))),
                    )
                    .key(format!("r{id}"))
                },
            )
            .transition(
                Transition::fade(Duration::from_millis(200)).moves(Duration::from_millis(200)),
            )
        })
        .unwrap();
    let viewport = LayoutViewport::new(320.0, 240.0);
    cx.layout_document(document, viewport).unwrap();
    let list = view.roots()[0];
    let rows = children(&cx, list);
    let third_y = cx.world().layout_box(rows[2]).unwrap().y;

    items
        .get()
        .unwrap()
        .update(|list| list.retain(|id| *id != 2));
    cx.flush_reactive().unwrap();
    assert_eq!(children(&cx, list), rows, "row 2 leaves in place");
    cx.layout_document(document, viewport).unwrap();
    assert_eq!(cx.world().layout_box(rows[2]).unwrap().y, third_y);

    // The leave ends: row 2 goes, row 3 moves up 20 px and slides there.
    let ended = start + Duration::from_millis(250);
    cx.advance_animations(ended);
    assert!(!cx.world().contains(rows[1]));
    cx.layout_document(document, viewport).unwrap();
    assert_eq!(cx.world().layout_box(rows[2]).unwrap().y, third_y - 20.0);
    let offset = translate_y_at(&cx, rows[2], ended);
    assert!((offset - 20.0).abs() < 0.5, "starts where it was: {offset}");
    let settled = translate_y_at(&cx, rows[2], ended + Duration::from_millis(250));
    assert!(settled.abs() < 0.01, "{settled}");

    // Reordering slides every row that moved.
    items.get().unwrap().update(|list| list.reverse());
    cx.flush_reactive().unwrap();
    cx.layout_document(document, viewport).unwrap();
    let moved = translate_y_at(&cx, rows[0], ended);
    assert!(moved < -0.5, "row 1 slides down from above: {moved}");
}

/// Wait for a worker thread's waker to reach this thread's executor.
fn until_woken() {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !has_woken_tasks() {
        assert!(std::time::Instant::now() < deadline, "no task was woken");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn a_resource_loads_off_thread_and_refetches_when_its_source_changes() {
    let (mut cx, _, parent) = setup();
    let wakes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counted = wakes.clone();
    set_task_wake(move || {
        counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let (send, receive) = std::sync::mpsc::channel::<String>();
    let receive = std::sync::Arc::new(std::sync::Mutex::new(receive));
    let handles = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let id = signal(1u32);
            let user = resource(
                move || id.get(),
                move |id| {
                    let receive = receive.clone();
                    spawn_blocking(move || {
                        format!("{id}:{}", receive.lock().unwrap().recv().unwrap())
                    })
                },
            );
            handles.set(Some((id, user)));
            text(move || user.get().unwrap_or_else(|| "…".into())).key("name")
        })
        .unwrap();
    let (id, user) = handles.get().unwrap();
    let label = Entity::<Text>::from_stable_id(view.roots()[0]);
    cx.take_system_work();
    assert!(user.loading());
    assert_eq!(text_of(&cx, label), "…");

    send.send("甲".into()).unwrap();
    until_woken();
    assert!(
        wakes.load(std::sync::atomic::Ordering::SeqCst) > 0,
        "the host was woken"
    );
    cx.take_system_work();
    assert_eq!(text_of(&cx, label), "1:甲");
    assert!(!user.loading());

    // A new source replaces the fetch; the old value stays until it lands.
    id.set(2);
    cx.flush_reactive().unwrap();
    cx.take_system_work();
    assert!(user.loading());
    assert_eq!(text_of(&cx, label), "1:甲");
    send.send("乙".into()).unwrap();
    until_woken();
    cx.take_system_work();
    assert_eq!(text_of(&cx, label), "2:乙");

    let before = task_count();
    user.refetch();
    cx.flush_reactive().unwrap();
    assert_eq!(task_count(), before + 1);
    view.unmount(&mut cx).unwrap();
    assert_eq!(task_count(), before, "the fetch is dropped with the view");
    send.send("丙".into()).unwrap();
}

#[test]
fn suspense_shows_its_fallback_until_the_resources_inside_resolve() {
    let (mut cx, _, parent) = setup();
    let gate = std::rc::Rc::new(std::cell::RefCell::new(None::<std::task::Waker>));
    let open = std::rc::Rc::new(std::cell::Cell::new(false));
    let (gate_in, open_in) = (gate.clone(), open.clone());
    let view = cx
        .mount_view(parent, move || {
            suspense(
                || text("加载中").key("fallback"),
                move || {
                    let data = resource(
                        || (),
                        move |()| {
                            let (gate, open) = (gate_in.clone(), open_in.clone());
                            std::future::poll_fn(move |cx| {
                                if open.get() {
                                    std::task::Poll::Ready(42u32)
                                } else {
                                    *gate.borrow_mut() = Some(cx.waker().clone());
                                    std::task::Poll::Pending
                                }
                            })
                        },
                    );
                    text(move || format!("{:?}", data.get())).key("content")
                },
            )
        })
        .unwrap();
    cx.take_system_work();
    let root = view.roots()[0];
    let [wrapper, block] = children(&cx, root)[..] else {
        panic!("content wrapper and fallback block");
    };
    let hidden = |cx: &AppContext| {
        cx.world()
            .node_style(wrapper)
            .is_some_and(|style| style.layout.hidden)
    };
    assert!(hidden(&cx), "content is built but hidden");
    assert_eq!(children(&cx, block).len(), 1, "fallback shown");

    open.set(true);
    gate.borrow_mut().take().unwrap().wake();
    cx.take_system_work();
    assert!(!hidden(&cx));
    assert!(children(&cx, block).is_empty(), "fallback gone");
    let content = children(&cx, wrapper)[0];
    assert_eq!(text_of(&cx, Entity::from_stable_id(content)), "Some(42)");
}

#[test]
fn a_kept_alive_branch_comes_back_with_its_nodes_and_state() {
    let (mut cx, _, parent) = setup();
    let handles = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let open = signal(true);
            handles.set(Some(open));
            when(open, move || {
                let draft = signal(String::from("草稿"));
                text(draft).key("draft")
            })
            .otherwise(|| text("别处"))
            .keep_alive()
        })
        .unwrap();
    let open = handles.get().unwrap();
    let container = view.roots()[0];
    let draft_node = children(&cx, container)[0];
    let scopes = reactive_stats().scopes;

    open.set(false);
    cx.flush_reactive().unwrap();
    assert!(cx.world().contains(draft_node), "kept, not despawned");
    let shown = children(&cx, container);
    assert_eq!(shown.len(), 2, "the other branch and the hidden holder");
    assert!(!shown.contains(&draft_node));

    open.set(true);
    cx.flush_reactive().unwrap();
    assert_eq!(
        children(&cx, container)[0],
        draft_node,
        "the same node again"
    );
    assert_eq!(text_of(&cx, Entity::from_stable_id(draft_node)), "草稿");
    // The first branch was never rebuilt, and the second is kept now.
    assert_eq!(reactive_stats().scopes, scopes + 1);

    view.unmount(&mut cx).unwrap();
    assert!(!cx.world().contains(draft_node));
}

#[test]
fn dynamic_keeps_at_most_max_views_alive() {
    let (mut cx, _, parent) = setup();
    let tab = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let current = signal(0u32);
            tab.set(Some(current));
            dynamic(current, |tab: &u32| text(format!("页 {tab}")))
                .keep_alive()
                .max(1)
        })
        .unwrap();
    let tab = tab.get().unwrap();
    let container = view.roots()[0];
    let first = children(&cx, container)[0];
    tab.set(1);
    cx.flush_reactive().unwrap();
    let second = children(&cx, container)[0];
    tab.set(2);
    cx.flush_reactive().unwrap();
    assert!(
        !cx.world().contains(first),
        "evicted: only one view is kept"
    );
    assert!(cx.world().contains(second));
    tab.set(1);
    cx.flush_reactive().unwrap();
    assert_eq!(children(&cx, container)[0], second);
    assert_eq!(text_of(&cx, Entity::from_stable_id(second)), "页 1");
}

#[test]
fn teleported_content_lives_under_the_target_and_dies_with_its_declaration() {
    let (mut cx, _, parent) = setup();
    let handles = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let layer = node_ref();
            let shown = signal(true);
            let target = signal(true);
            handles.set(Some((layer, shown, target)));
            column().children((
                widget(Stack::column(0.0)).node_ref(layer).key("layer"),
                when(shown, move || {
                    teleport(
                        move || target.get().then(|| layer.get()).flatten(),
                        text("浮层"),
                    )
                }),
            ))
        })
        .unwrap();
    let (layer, shown, target) = handles.get().unwrap();
    let layer = layer.get_untracked().unwrap();
    let root = view.roots()[0];
    let content = children(&cx, layer);
    assert_eq!(content.len(), 1, "placed under the layer at mount");
    assert_eq!(text_of(&cx, Entity::from_stable_id(content[0])), "浮层");

    target.set(false);
    cx.flush_reactive().unwrap();
    assert!(children(&cx, layer).is_empty(), "back in place");
    let block = children(&cx, root)[1];
    let anchor = children(&cx, block)[0];
    assert_eq!(children(&cx, anchor), content);

    target.set(true);
    cx.flush_reactive().unwrap();
    assert_eq!(children(&cx, layer), content);
    shown.set(false);
    cx.flush_reactive().unwrap();
    assert!(
        !cx.world().contains(content[0]),
        "gone with where it was declared"
    );
    assert!(children(&cx, layer).is_empty());
}

#[test]
fn an_error_boundary_shows_its_fallback_while_a_view_inside_failed() {
    let (mut cx, _, parent) = setup();
    let handle = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let input = signal(String::from("12"));
            handle.set(Some(input));
            error_boundary(
                |errors| text(format!("出错：{}", errors.join("；"))),
                move || {
                    dynamic(input, |input: &String| {
                        input
                            .parse::<u32>()
                            .map(|n| text(format!("数 {n}")))
                            .map_err(|_| format!("{input} 不是数"))
                    })
                },
            )
        })
        .unwrap();
    let input = handle.get().unwrap();
    let root = view.roots()[0];
    let [content, fallback] = children(&cx, root)[..] else {
        panic!("content and fallback blocks");
    };
    let hidden = |cx: &AppContext| {
        cx.world()
            .node_style(content)
            .is_some_and(|style| style.layout.hidden)
    };
    assert!(!hidden(&cx));
    assert!(children(&cx, fallback).is_empty());

    input.set("abc".into());
    cx.flush_reactive().unwrap();
    assert!(hidden(&cx), "content hidden while the error stands");
    let shown = children(&cx, fallback);
    assert_eq!(
        text_of(&cx, Entity::from_stable_id(shown[0])),
        "出错：abc 不是数"
    );

    input.set("7".into());
    cx.flush_reactive().unwrap();
    assert!(!hidden(&cx), "the failed branch was dropped with its error");
    assert!(children(&cx, fallback).is_empty());
}

#[test]
fn a_control_can_be_inspected_and_edited_field_by_field() {
    let (mut cx, _, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            let busy = signal(false);
            button("保存").disabled(busy).key("save")
        })
        .unwrap();
    let save = view.roots()[0];
    let inspection = cx.inspect(save).unwrap();
    assert_eq!(inspection.control, Some("Button"));
    assert!(inspection.element.is_some(), "it has bindings");
    let field = |inspection: &Inspection, name: &str| {
        inspection
            .fields
            .iter()
            .find(|field| field.name == name)
            .cloned()
            .unwrap()
    };
    let label = field(&inspection, "label");
    assert_eq!(label.value, "\"保存\"");
    assert!(label.bound_at.is_none(), "a constant");
    assert!(field(&inspection, "disabled").bound_at.is_some());

    cx.set_field(save, "label", "另存").unwrap();
    cx.set_field(save, "loading", "true").unwrap();
    let edited: Entity<Button> = Entity::from_stable_id(save);
    assert_eq!(
        cx.read(edited, |b| (b.label.clone(), b.loading)).unwrap(),
        ("另存".to_owned(), true)
    );
    assert!(
        cx.set_field(save, "loading", "也许")
            .unwrap_err()
            .contains("not `true`")
    );
    assert!(
        cx.set_field(save, "colour", "x")
            .unwrap_err()
            .contains("no field `colour`")
    );
    assert!(
        cx.set_field(parent, "label", "x").is_err(),
        "a stack is not a control"
    );
}

#[test]
fn a_store_with_history_undoes_one_handler_at_a_time() {
    let (mut cx, _, parent) = setup();
    let board = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let state = store_with_history(
                Board {
                    tasks: (1..=2).map(task).collect(),
                },
                10,
            );
            board.set(Some(state));
            state
                .tasks()
                .keyed(|task| task.id)
                .each(|task| text(task.title()))
        })
        .unwrap();
    let board = board.get().unwrap();
    let list = view.roots()[0];
    let tasks = board.tasks().keyed(|task| task.id);
    let title =
        |cx: &AppContext, row: usize| text_of(cx, Entity::from_stable_id(children(cx, list)[row]));
    assert!(!board.can_undo());

    // One handler: two writes, one step.
    tasks.at(&1).title().set("甲".into());
    tasks.at(&2).title().set("乙".into());
    cx.flush_reactive().unwrap();
    board.tasks().push(task(3));
    cx.flush_reactive().unwrap();
    assert_eq!(board.steps().len(), 2);
    assert_eq!(children(&cx, list).len(), 3);

    assert!(board.undo());
    cx.flush_reactive().unwrap();
    assert_eq!(children(&cx, list).len(), 2, "the push is undone");
    assert_eq!((title(&cx, 0), title(&cx, 1)), ("甲".into(), "乙".into()));
    assert!(board.undo());
    cx.flush_reactive().unwrap();
    assert_eq!(
        title(&cx, 0),
        "任务 1",
        "both writes of the first step undone"
    );
    assert!(!board.undo());
    assert!(board.can_redo());

    assert_eq!(board.travel(2), 2);
    cx.flush_reactive().unwrap();
    assert_eq!(children(&cx, list).len(), 3);
    // A new write after undoing drops what could be redone.
    board.undo();
    cx.flush_reactive().unwrap();
    tasks.at(&1).done().set(true);
    cx.flush_reactive().unwrap();
    assert!(!board.can_redo());
}

static CARD: StylePatch = StylePatch::new(r#"{"opacity":0.8,"padding":{"Px":12.0}}"#);
static DIM: StylePatch = StylePatch::new(r#"{"opacity":0.4}"#);
static CARD_SHEET: Sheet = Sheet::new(
    &[
        SheetRule {
            classes: &[0],
            patch: &CARD,
        },
        SheetRule {
            classes: &[1],
            patch: &DIM,
        },
    ],
    &[],
);
const CARD_CLASS: Class = Class::new(&CARD_SHEET, 0);
const DIM_CLASS: Class = Class::new(&CARD_SHEET, 1);

#[test]
fn compiled_styles_compose_once_per_class_set_and_follow_conditional_classes() {
    let (mut cx, _, parent) = setup();
    let dim = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let dimmed = signal(false);
            dim.set(Some(dimmed));
            column().children(
                (0..3)
                    .map(|_| {
                        widget(Stack::column(0.0))
                            .class(CARD_CLASS)
                            .class_when(DIM_CLASS, dimmed)
                    })
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap();
    let rows = children(&cx, view.roots()[0]);
    let layout = |cx: &AppContext, row: StableNodeId| {
        cx.read(Entity::<Stack>::from_stable_id(row), |stack| {
            std::sync::Arc::clone(&stack.style_ref().layout)
        })
        .unwrap()
    };
    let first = layout(&cx, rows[0]);
    assert_eq!(first.opacity, Some(0.8));
    assert_eq!(first.padding, Some(LengthSpec::Px(12.0)));
    assert!(
        std::sync::Arc::ptr_eq(&first, &layout(&cx, rows[2])),
        "every instance holds the one composed layout"
    );

    dim.get().unwrap().set(true);
    cx.flush_reactive().unwrap();
    let dimmed = layout(&cx, rows[1]);
    assert_eq!(dimmed.opacity, Some(0.4), "the later rule wins");
    assert_eq!(dimmed.padding, Some(LengthSpec::Px(12.0)));
}

#[test]
fn an_implicit_transition_plays_from_the_shown_value_to_the_new_one() {
    use std::time::Duration;
    let (mut cx, _, parent) = setup();
    let start = Duration::from_secs(10);
    cx.advance_animations(start);
    let dim = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let dimmed = signal(false);
            dim.set(Some(dimmed));
            widget(Stack::column(0.0))
                .class(CARD_CLASS)
                .class_when(DIM_CLASS, dimmed)
                .animate([Implicit::new(
                    crate::AnimatableProperty::Opacity,
                    Duration::from_millis(200),
                )])
        })
        .unwrap();
    let node = view.roots()[0];
    dim.get().unwrap().set(true);
    cx.flush_reactive().unwrap();
    assert_eq!(
        opacity_at(&cx, node, start),
        0.8,
        "starts from what was shown"
    );
    let halfway = opacity_at(&cx, node, start + Duration::from_millis(100));
    assert!(halfway < 0.8 && halfway > 0.4, "{halfway}");
    assert_eq!(
        opacity_at(&cx, node, start + Duration::from_millis(300)),
        0.4
    );
}

/// Kind, text and children of `id`, recursively: what two ways of building
/// the same tree must agree on.
fn shape(cx: &AppContext, id: StableNodeId) -> String {
    let node = cx.world().node(id).unwrap();
    let children: Vec<_> = node.children.iter().map(|&c| shape(cx, c)).collect();
    format!(
        "{:?}{:?}[{}]",
        node.kind,
        cx.world().text(id).unwrap_or(""),
        children.join(",")
    )
}

fn shell_view() -> impl IntoView {
    widget(crate::DesktopShell::new().title("T"))
        .title_leading(text("L"))
        .title_trailing(row().gap(6.0).children((text("A"), text("B"))))
        .navigation(text("nav"))
        .primary(text("page"))
}

#[test]
fn slots_build_the_shell_the_builder_and_its_assembler_build() {
    let (mut cx, document, _) = setup();
    let built = cx
        .build(document, |ui| {
            let leading = ui.detached(Text::new("L"));
            let trailing = ui.detached(Stack::row(6.0));
            ui.nest(trailing, |ui| {
                ui.child("a", Text::new("A"));
                ui.child("b", Text::new("B"));
            });
            let navigation = ui.detached(Text::new("nav"));
            let primary = ui.detached(Text::new("page"));
            ui.child(
                "shell",
                crate::DesktopShell::new()
                    .title("T")
                    .title_leading(leading.stable_id())
                    .title_trailing(trailing.stable_id())
                    .navigation(navigation.stable_id())
                    .primary(primary.stable_id()),
            )
        })
        .unwrap();
    cx.assemble_desktop_shell(built).unwrap();

    let view = cx.mount_view_root(document, shell_view).unwrap();
    assert_eq!(shape(&cx, view.roots()[0]), shape(&cx, built.stable_id()));
    assert!(shape(&cx, built.stable_id()).contains("\"nav\""));
}

#[test]
fn a_binding_that_changes_the_shell_assembles_it_again() {
    use nana_ui_core::{RegionId, WorkspaceModel, WorkspaceMutation};
    let model = |size: f32| {
        let mut model = WorkspaceModel::new();
        model.update(
            WorkspaceMutation::SetRegionSize(RegionId::Resources, size),
            std::time::Duration::ZERO,
        );
        model
    };
    let (mut cx, document, _) = setup();
    let size = std::cell::Cell::new(None);
    let view = cx
        .mount_view_root(document, || {
            let width = signal(240.0f32);
            size.set(Some(width));
            widget(crate::DesktopShell::new())
                .bind(move |shell| shell.model = model(width.get()))
                .navigation(text("nav"))
        })
        .unwrap();
    let shell = Entity::<crate::DesktopShell>::from_stable_id(view.roots()[0]);
    let workspace_model = |cx: &AppContext| {
        let workspace = cx.read(shell, |shell| shell.workspace).unwrap().unwrap();
        cx.read(Entity::<crate::Workspace>::from_stable_id(workspace), |w| {
            w.model.clone()
        })
        .unwrap()
    };
    assert_eq!(workspace_model(&cx), model(240.0));

    size.get().unwrap().set(320.0);
    cx.flush_reactive().unwrap();
    assert_ne!(model(240.0), model(320.0));
    assert_eq!(workspace_model(&cx), model(320.0));
}

#[test]
fn a_child_slot_comes_before_the_children_and_names_itself_to_its_parent() {
    let (mut cx, _, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            widget(crate::SidebarFrame::new())
                .child_slot(text("footer"), crate::SidebarFrame::footer)
                .children(text("body"))
        })
        .unwrap();
    let frame = view.roots()[0];
    let kids = children(&cx, frame);
    assert_eq!(kids.len(), 2);
    assert_eq!(cx.world().text(kids[0]), Some("footer"));
    assert_eq!(cx.world().text(kids[1]), Some("body"));
    let footer = cx
        .read(Entity::<crate::SidebarFrame>::from_stable_id(frame), |f| {
            f.footer
        })
        .unwrap();
    assert_eq!(footer, Some(kids[0]));
    assert_eq!(children(&cx, parent), vec![frame]);
}

#[test]
fn keys_inside_a_view_resolve_from_its_root_not_from_the_mount_parent() {
    let (mut cx, _, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            column()
                .key("page")
                .children(column().key("content").children(text("正文").key("body")))
        })
        .unwrap();
    let page = view.roots()[0];
    let body = cx.resolve_assembly_path(page, "content/body").unwrap();
    assert_eq!(text_of(&cx, Entity::from_stable_id(body)), "正文");
    // The root goes in unkeyed, so a later `mount` on the parent, which
    // drops the keys it does not name, cannot take the view with it.
    assert!(cx.resolve_assembly_path(parent, "page").is_none());
    cx.mount(Entity::<Stack>::from_stable_id(parent), |_| Ok(()))
        .unwrap();
    assert!(cx.world().contains(page));
}

#[test]
fn a_slot_must_have_one_root() {
    let (mut cx, document, _) = setup();
    let before = cx.world().len();
    let error = cx
        .mount_view_root(document, || {
            widget(crate::DesktopShell::new()).primary((text("a"), text("b")))
        })
        .err();
    assert_eq!(error, Some(crate::FrameworkError::InvalidInput));
    assert_eq!(cx.world().len(), before);
}

#[test]
fn a_composite_in_a_slot_assembles_before_the_composite_that_takes_it() {
    use crate::SettingsPage;
    use nana_ui_core::{RadiusTier, SettingsModel, SettingsState, SettingsTab};
    let (mut cx, document, _) = setup();
    let model = SettingsModel::new("appearance", [SettingsTab::new("appearance", "外观")]).unwrap();
    let state = SettingsState::new(&model);
    let (_, page) = cx
        .mount_view_root(document, || {
            let page = entity_ref::<SettingsPage>();
            let shell = widget(crate::DesktopShell::new())
                .navigation(text("导航"))
                .primary(
                    widget(SettingsPage::new(model, state))
                        .entity_ref(page)
                        .content(text("页面")),
                );
            with_refs(shell, page)
        })
        .unwrap();
    // The shell rounds the content of its primary region. Were the page
    // assembled after the shell, its projection would take that back.
    assert_eq!(
        cx.world()
            .node_style(page.stable_id())
            .and_then(|style| style.radius),
        Some(RadiusTier::Lg)
    );
}

#[test]
fn unmounting_a_shell_view_removes_its_slots_and_chrome() {
    let (mut cx, document, _) = setup();
    let before = cx.world().len();
    let view = cx.mount_view_root(document, shell_view).unwrap();
    assert!(cx.world().len() > before + 5);
    view.unmount(&mut cx).unwrap();
    assert_eq!(cx.world().len(), before);
}

#[test]
fn a_leaf_composite_a_view_builds_is_assembled() {
    let (mut cx, _, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            widget(crate::Chip::new("附件").dismissible(true))
        })
        .unwrap();
    let chip = Entity::<crate::Chip>::from_stable_id(view.roots()[0]);
    let close = cx.read(chip, |chip| chip.close).unwrap();
    assert!(close.is_some());
    assert_eq!(children(&cx, chip.stable_id()).len(), 1);
}

/// A section built by hand with all its chrome, rows in its body.
fn hand_built_section(cx: &mut AppContext, document: DocumentId) -> StableNodeId {
    use crate::SidebarSection;
    let mut spec = SidebarSection::new("资源").count(2).collapsible(true);
    let disclosure = cx
        .create_detached_component(document, spec.disclosure_mark())
        .unwrap();
    let title = cx
        .create_detached_component(document, spec.title_label())
        .unwrap();
    let count = cx
        .create_detached_component(document, spec.count_label())
        .unwrap();
    let header = cx
        .create_detached_component(document, spec.header_item())
        .unwrap();
    cx.append_child(header, disclosure).unwrap();
    cx.append_child(header, title).unwrap();
    cx.append_child(header, count).unwrap();
    let body = cx
        .create_detached_component(document, SidebarSection::body_port())
        .unwrap();
    for row in ["行一", "行二"] {
        let row = cx
            .create_detached_component(document, Text::new(row))
            .unwrap();
        cx.append_child(body, row).unwrap();
    }
    spec = spec
        .disclosure(disclosure.stable_id())
        .title_slot(title.stable_id())
        .count_slot(count.stable_id())
        .header(header.stable_id())
        .body(body.stable_id());
    let section = cx.create_component(document, spec).unwrap();
    cx.append_child(section, header).unwrap();
    cx.append_child(section, body).unwrap();
    section.stable_id()
}

#[test]
fn a_view_section_builds_the_chrome_a_hand_built_one_has() {
    let (mut cx, document, _) = setup();
    let built = hand_built_section(&mut cx, document);
    let view = cx
        .mount_view_root(document, || {
            widget(
                crate::SidebarSection::new("资源")
                    .count(2)
                    .collapsible(true),
            )
            .children((text("行一"), text("行二")))
        })
        .unwrap();
    assert_eq!(shape(&cx, view.roots()[0]), shape(&cx, built));
    assert!(shape(&cx, built).contains("行二"));
}

#[test]
fn a_section_keeps_its_chrome_and_takes_rows_that_come_later() {
    let (mut cx, document, _) = setup();
    let state = std::cell::Cell::new(None);
    let view = cx
        .mount_view_root(document, || {
            let count = signal(1usize);
            let rows = signal(vec![1u32]);
            state.set(Some((count, rows)));
            widget(crate::SidebarSection::new("资源").count(1))
                .bind(move |section| section.count = Some(count.get()))
                .tools(text("工具"))
                .children(rows.each(|r| *r, |r| text(format!("行 {r}"))))
        })
        .unwrap();
    let section = Entity::<crate::SidebarSection>::from_stable_id(view.roots()[0]);
    let (header, body) = cx
        .read(section, |s| (s.header.unwrap(), s.body.unwrap()))
        .unwrap();
    assert_eq!(children(&cx, section.stable_id()), vec![header, body]);
    let header_texts: Vec<_> = children(&cx, header)
        .into_iter()
        .map(|id| cx.world().text(id).unwrap_or("").to_owned())
        .collect();
    assert_eq!(header_texts, ["资源", "1", "工具"]);
    let nodes = cx.world().len();

    let (count, rows) = state.get().unwrap();
    count.set(2);
    rows.update(|list| list.push(2));
    cx.flush_reactive().unwrap();
    assert_eq!(children(&cx, section.stable_id()), vec![header, body]);
    let count_label = children(&cx, header)[1];
    assert_eq!(cx.world().text(count_label), Some("2"));
    assert!(shape(&cx, body).contains("行 2"), "{}", shape(&cx, body));
    assert_eq!(cx.world().len(), nodes + 1, "one new row, no new chrome");
}

#[test]
fn a_frame_takes_its_top_body_and_footer_in_order() {
    let (mut cx, document, _) = setup();
    let view = cx
        .mount_view_root(document, || {
            widget(crate::SidebarFrame::new())
                .top(text("顶部"))
                .body(text("内容"))
                .footer(text("底部"))
        })
        .unwrap();
    let frame = Entity::<crate::SidebarFrame>::from_stable_id(view.roots()[0]);
    let (top, body, footer) = cx
        .read(frame, |f| {
            (f.top.unwrap(), f.body.unwrap(), f.footer.unwrap())
        })
        .unwrap();
    assert_eq!(children(&cx, frame.stable_id()), vec![top, body, footer]);
    assert!(
        cx.read(Entity::<crate::ScrollView>::from_stable_id(body), |_| ())
            .is_ok(),
        "the body is the frame's scrollport"
    );
    assert_eq!(cx.world().text(children(&cx, body)[0]), Some("内容"));
}

#[test]
fn app_shell_and_workspace_slots_build_what_their_assemblers_build() {
    use crate::{AppShell, Workspace};
    use nana_ui_core::RegionId;
    let (mut cx, document, _) = setup();
    let shell = cx
        .build(document, |ui| {
            let body = ui.detached(Text::new("正文"));
            let overlay = ui.detached(Stack::column(0.0));
            ui.child(
                "shell",
                AppShell::new()
                    .body(body.stable_id())
                    .overlay(overlay.stable_id()),
            )
        })
        .unwrap();
    cx.assemble_app_shell(shell).unwrap();
    let workspace = cx
        .build(document, |ui| {
            let primary = ui.detached(Text::new("主区"));
            let resources = ui.detached(Text::new("资源"));
            ui.child(
                "workspace",
                Workspace::new()
                    .slot(RegionId::Primary, primary.stable_id())
                    .slot(RegionId::Resources, resources.stable_id()),
            )
        })
        .unwrap();
    cx.assemble_workspace(workspace).unwrap();

    let views = cx
        .mount_view_root(document, || {
            (
                widget(AppShell::new())
                    .body(text("正文"))
                    .overlay(widget(Stack::column(0.0))),
                widget(Workspace::new())
                    .region(RegionId::Primary, text("主区"))
                    .region(RegionId::Resources, text("资源")),
            )
        })
        .unwrap();
    assert_eq!(shape(&cx, views.roots()[0]), shape(&cx, shell.stable_id()));
    assert_eq!(
        shape(&cx, views.roots()[1]),
        shape(&cx, workspace.stable_id())
    );
    assert!(shape(&cx, workspace.stable_id()).contains("资源"));
}

fn dock_layout(
    files: Option<StableNodeId>,
    preview: Option<StableNodeId>,
    log: Option<StableNodeId>,
) -> crate::DockNode {
    use crate::{DockAxis, DockNode};
    DockNode::split(
        DockAxis::Horizontal,
        0.3,
        DockNode::item("files", files),
        DockNode::tabs(
            ["preview", "log"],
            "preview",
            [("preview", preview), ("log", log)],
        ),
    )
}

#[test]
fn dock_panels_are_the_children_keyed_with_their_ids() {
    use crate::Dock;
    let (mut cx, document, _) = setup();
    let built = cx
        .build(document, |ui| {
            let files = ui.detached(Text::new("文件"));
            let preview = ui.detached(Text::new("预览"));
            let log = ui.detached(Text::new("日志"));
            ui.child(
                "dock",
                Dock::new(dock_layout(
                    Some(files.stable_id()),
                    Some(preview.stable_id()),
                    Some(log.stable_id()),
                )),
            )
        })
        .unwrap();
    cx.assemble_dock(built).unwrap();

    let tabs = std::cell::Cell::new(None);
    let view = cx
        .mount_view_root(document, || {
            let stacked = signal(false);
            tabs.set(Some(stacked));
            widget(Dock::new(dock_layout(None, None, None)))
                .bind(move |dock| {
                    if stacked.get() {
                        dock.root = crate::DockNode::tabs(
                            ["files", "preview", "log"],
                            "files",
                            [("files", None), ("preview", None), ("log", None)],
                        );
                    }
                })
                .children((
                    text("文件").key("files"),
                    text("预览").key("preview"),
                    text("日志").key("log"),
                ))
        })
        .unwrap();
    let dock = view.roots()[0];
    assert_eq!(shape(&cx, dock), shape(&cx, built.stable_id()));
    assert!(shape(&cx, dock).contains("文件"));

    let nodes = cx.world().len();
    tabs.get().unwrap().set(true);
    cx.flush_reactive().unwrap();
    let after = shape(&cx, dock);
    assert!(
        after.contains("文件"),
        "the panels follow the new layout: {after}"
    );
    let root = cx
        .read(Entity::<Dock>::from_stable_id(dock), |d| d.root.clone())
        .unwrap();
    assert!(
        matches!(&root, crate::DockNode::Tabs { contents, .. }
            if contents.iter().all(|(_, content)| content.is_some())),
        "every tab keeps its content: {root:?}"
    );
    assert!(cx.world().len() <= nodes + 4, "no panel is rebuilt");
}

#[test]
fn panes_and_leaf_composites_assemble_in_a_view_as_by_hand() {
    use crate::{ConfirmDialog, DatePicker, PaneSection, SplitPane};
    use nana_ui_core::{CivilDate, SplitAxis, SplitPaneModel};
    let (mut cx, document, _) = setup();
    let model = SplitPaneModel::new(SplitAxis::Horizontal, 200.0, 100.0, 400.0);
    let date = CivilDate::new(2026, 9, 29).unwrap();

    let split = cx
        .build(document, |ui| {
            let first = ui.detached(Text::new("左"));
            let second = ui.detached(Text::new("右"));
            ui.child(
                "split",
                SplitPane::from_model(&model, first.stable_id(), second.stable_id()),
            )
        })
        .unwrap();
    cx.assemble_split_pane(split).unwrap();
    let section = cx
        .build(document, |ui| {
            let header = ui.detached(Text::new("标题"));
            let body = ui.detached(Text::new("内容"));
            ui.child(
                "section",
                PaneSection::new()
                    .header(header.stable_id())
                    .body(body.stable_id()),
            )
        })
        .unwrap();
    cx.assemble_pane_section(section).unwrap();
    let dialog = cx
        .create_component(document, ConfirmDialog::new("删除？", "不能撤销"))
        .unwrap();
    cx.assemble_confirm_dialog(dialog).unwrap();
    let picker = cx
        .create_component(document, DatePicker::new(date))
        .unwrap();
    cx.assemble_date_picker(picker).unwrap();

    let views = cx
        .mount_view_root(document, || {
            (
                widget(SplitPane::new(&model))
                    .first(text("左"))
                    .second(text("右")),
                widget(PaneSection::new())
                    .header(text("标题"))
                    .body(text("内容")),
                widget(ConfirmDialog::new("删除？", "不能撤销")),
                widget(DatePicker::new(date)),
            )
        })
        .unwrap();
    let roots = views.roots();
    for (view, hand) in roots.iter().zip([
        split.stable_id(),
        section.stable_id(),
        dialog.stable_id(),
        picker.stable_id(),
    ]) {
        assert_eq!(shape(&cx, *view), shape(&cx, hand));
        assert!(!children(&cx, *view).is_empty(), "{}", shape(&cx, *view));
    }
}

#[cfg(feature = "rich-text")]
#[test]
fn markdown_in_a_view_gets_its_fence_children() {
    use crate::NativeMarkdown;
    let (mut cx, document, _) = setup();
    let source = "文字\n\n```rust\nfn main() {}\n```\n";
    let hand = cx
        .create_component(document, NativeMarkdown::from_source(source))
        .unwrap();
    cx.assemble_markdown(hand).unwrap();
    let view = cx
        .mount_view_root(document, || widget(NativeMarkdown::from_source(source)))
        .unwrap();
    assert_eq!(shape(&cx, view.roots()[0]), shape(&cx, hand.stable_id()));
    assert!(!children(&cx, hand.stable_id()).is_empty());
}

#[test]
fn settings_composites_assemble_in_a_view_as_by_hand() {
    use crate::{
        AboutMetadata, AboutSection, AppearanceSection, SettingsCollapsibleCard, SettingsPage,
        SettingsSidebar,
    };
    use nana_ui_core::{AppearanceSettings, SettingsModel, SettingsState, SettingsTab, ThemeMode};
    let (mut cx, document, _) = setup();
    let model = SettingsModel::new("appearance", [SettingsTab::new("appearance", "外观")]).unwrap();
    let state = SettingsState::new(&model);
    let metadata = AboutMetadata::new("产品", "1.0.0");

    let appearance = cx
        .create_component(
            document,
            AppearanceSection::new(ThemeMode::Dark, AppearanceSettings::default()),
        )
        .unwrap();
    cx.assemble_appearance_section(appearance).unwrap();
    let about = cx
        .create_component(document, AboutSection::new(metadata.clone()))
        .unwrap();
    cx.assemble_about_section(about).unwrap();
    let sidebar = cx
        .create_component(document, SettingsSidebar::new(model.clone(), state.clone()))
        .unwrap();
    cx.assemble_settings_sidebar(sidebar).unwrap();
    let card = cx
        .build(document, |ui| {
            let summary = ui.detached(Text::new("摘要"));
            let details = ui.detached(Text::new("详情"));
            ui.child(
                "card",
                SettingsCollapsibleCard::new(true)
                    .summary(summary.stable_id())
                    .details(details.stable_id()),
            )
        })
        .unwrap();
    cx.assemble_settings_collapsible_card(card).unwrap();
    let page = cx
        .build(document, |ui| {
            let content = ui.detached(Text::new("页面"));
            ui.child(
                "page",
                SettingsPage::new(model.clone(), state.clone()).content(content.stable_id()),
            )
        })
        .unwrap();
    cx.assemble_settings_page(page).unwrap();

    let views = cx
        .mount_view_root(document, || {
            (
                widget(AppearanceSection::new(
                    ThemeMode::Dark,
                    AppearanceSettings::default(),
                )),
                widget(AboutSection::new(metadata.clone())),
                widget(SettingsSidebar::new(model.clone(), state.clone())),
                widget(SettingsCollapsibleCard::new(true))
                    .summary(text("摘要"))
                    .details(text("详情")),
                widget(SettingsPage::new(model.clone(), state.clone())).content(text("页面")),
            )
        })
        .unwrap();
    for (view, hand) in views.roots().iter().zip([
        appearance.stable_id(),
        about.stable_id(),
        sidebar.stable_id(),
        card.stable_id(),
        page.stable_id(),
    ]) {
        assert_eq!(shape(&cx, *view), shape(&cx, hand));
        assert!(!children(&cx, *view).is_empty(), "{}", shape(&cx, *view));
    }
}

#[test]
fn entity_refs_and_context_handlers_serve_code_that_works_by_hand() {
    #[derive(Debug, PartialEq)]
    struct Pressed(u32);
    let (mut cx, _, parent) = setup();
    let refs = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let label = entity_ref::<Text>();
            let press = entity_ref::<Button>();
            refs.set(Some((label, press)));
            column().children((
                text("未按").entity_ref(label),
                button("按").entity_ref(press).on_cx(
                    |button: &mut Button, _: &crate::Activate, cx| {
                        button.label = "已按".into();
                        cx.dispatch_program(Pressed(1));
                    },
                ),
            ))
        })
        .unwrap();
    let (label, press) = refs.get().unwrap();
    let (label, press) = (label.get().unwrap(), press.get().unwrap());
    assert_eq!(view.root::<Stack>().unwrap().stable_id(), view.roots()[0]);
    cx.update_component(label, |text, _| text.value = "手动".into())
        .unwrap();
    assert_eq!(text_of(&cx, label), "手动");

    cx.activate_button(press).unwrap();
    assert_eq!(cx.read(press, |b| b.label.clone()).unwrap(), "已按");
    let messages = cx.take_program_messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].downcast_ref::<Pressed>(), Some(&Pressed(1)));
}

#[test]
fn an_element_fills_its_entity_ref_and_its_node_refs_alike() {
    let (mut cx, _, parent) = setup();
    let refs = std::cell::Cell::new(None);
    cx.mount_view(parent, || {
        let (entity_first, node_first) = (entity_ref::<Button>(), node_ref());
        let (node_second, entity_second) = (node_ref(), entity_ref::<Text>());
        let anchor = node_ref();
        refs.set(Some((
            entity_first,
            node_first,
            node_second,
            entity_second,
            anchor,
        )));
        column().children((
            button("按").entity_ref(entity_first).node_ref(node_first),
            text("标题")
                .node_ref(node_second)
                .node_ref(anchor)
                .entity_ref(entity_second),
        ))
    })
    .unwrap();
    let (entity_first, node_first, node_second, entity_second, anchor) = refs.get().unwrap();
    let button = entity_first.get().expect("the entity ref").stable_id();
    assert_eq!(node_first.get_untracked(), Some(button));
    let label = entity_second.get().expect("the entity ref").stable_id();
    assert_eq!(node_second.get_untracked(), Some(label));
    assert_eq!(anchor.get_untracked(), Some(label));
    assert_ne!(button, label);
}

#[test]
fn with_refs_hands_back_resolved_entities_or_fails_whole() {
    let (mut cx, document, _) = setup();
    let (view, (label, [first, second])) = cx
        .mount_view_root(document, || {
            let label = entity_ref::<Text>();
            let buttons = [entity_ref::<Button>(), entity_ref::<Button>()];
            with_refs(
                column().children((
                    text("标题").entity_ref(label),
                    button("一").entity_ref(buttons[0]),
                    button("二").entity_ref(buttons[1]),
                )),
                (label, buttons),
            )
        })
        .unwrap();
    assert_eq!(text_of(&cx, label), "标题");
    assert_eq!(cx.read(second, |b| b.label.clone()).unwrap(), "二");
    assert_eq!(
        children(&cx, view.roots()[0]),
        [label.stable_id(), first.stable_id(), second.stable_id()]
    );

    let before = cx.world().len();
    let error = cx
        .mount_view_root(document, || {
            let hidden = entity_ref::<Text>();
            with_refs(
                column().children(when(false, move || text("不在").entity_ref(hidden))),
                hidden,
            )
        })
        .err();
    assert_eq!(error, Some(crate::FrameworkError::InvalidInput));
    assert_eq!(cx.world().len(), before, "a failed mount leaves nothing");
}

/// What a composite's child slot holds keeps the keys it declared, under
/// the composite: `item/leading` finds the list item's leading content.
#[test]
fn a_child_slot_keeps_the_keys_its_view_declared() {
    let (mut cx, _, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            list_item("标题")
                .leading(text("头").key("leading"))
                .trailing(row().key("tools").children(text("尾").key("count")))
        })
        .unwrap();
    let item = view.roots()[0];
    let leading = cx.resolve_assembly_path(item, "leading").unwrap();
    assert_eq!(cx.world().text(leading), Some("头"));
    let count = cx.resolve_assembly_path(item, "tools/count").unwrap();
    assert_eq!(cx.world().text(count), Some("尾"));
}

#[test]
fn list_item_slots_in_a_view_are_the_slots_set_by_hand() {
    use crate::{ListItem, ListItemSlots};
    let (mut cx, document, _) = setup();
    let item = cx
        .create_component(document, ListItem::new("文件"))
        .unwrap();
    let lead = cx.create_component(document, Text::new("▸")).unwrap();
    let tail = cx.create_component(document, Text::new("3")).unwrap();
    cx.set_list_item_slots(
        item,
        ListItemSlots {
            leading: Some(lead.stable_id()),
            content: None,
            trailing: Some(tail.stable_id()),
        },
    )
    .unwrap();
    let view = cx
        .mount_view_root(document, || {
            widget(ListItem::new("文件"))
                .leading(text("▸"))
                .trailing(text("3"))
        })
        .unwrap();
    let viewed = view.roots()[0];
    assert_eq!(shape(&cx, viewed), shape(&cx, item.stable_id()));
    let slots = cx
        .read(Entity::<ListItem>::from_stable_id(viewed), |item| {
            item.slots
        })
        .unwrap();
    assert_eq!(slots.leading, Some(children(&cx, viewed)[0]));
    assert_eq!(slots.trailing, Some(children(&cx, viewed)[1]));
}

#[test]
fn a_detached_view_is_built_and_placed_nowhere_until_something_places_it() {
    let (mut cx, document, _) = setup();
    let (view, (column_ref, spare)) = cx
        .mount_view_root(document, || {
            let column_ref = entity_ref::<Stack>();
            let spare = entity_ref::<Text>();
            let view = column()
                .entity_ref(column_ref)
                .children((text("在树里"), detached(text("备用").entity_ref(spare))));
            with_refs(view, (column_ref, spare))
        })
        .unwrap();
    assert_eq!(view.roots(), [column_ref.stable_id()]);
    assert_eq!(children(&cx, column_ref.stable_id()).len(), 1);
    assert!(cx.world().contains(spare.stable_id()));
    assert!(cx.world().node(spare.stable_id()).unwrap().parent.is_none());
    assert!(
        !cx.world()
            .document_order(document)
            .contains(&spare.stable_id())
    );

    cx.append_child(column_ref, spare).unwrap();
    assert_eq!(children(&cx, column_ref.stable_id())[1], spare.stable_id());
    assert_eq!(text_of(&cx, spare), "备用");
}

/// Every text under `id`, in document order.
fn texts_under(cx: &AppContext, id: StableNodeId) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(text) = cx.world().text(id).filter(|text| !text.is_empty()) {
        out.push(text.to_owned());
    }
    for child in children(cx, id) {
        out.extend(texts_under(cx, child));
    }
    out
}

fn virtual_items(n: u32) -> Vec<VirtualRow> {
    (0..n)
        .map(|id| VirtualRow {
            id,
            title: format!("行 {id}"),
        })
        .collect()
}

fn settle(cx: &mut AppContext, document: DocumentId, viewport: LayoutViewport) {
    for _ in 0..4 {
        cx.layout_document(document, viewport).unwrap();
        cx.flush_reactive().unwrap();
    }
}

#[test]
fn a_virtual_list_within_a_page_builds_the_rows_the_page_shows() {
    let (mut cx, document, parent) = setup();
    let header_shown = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let page = node_ref();
            let header = signal(true);
            header_shown.set(Some(header));
            widget(
                crate::ScrollView::new(crate::ScrollAxes::Vertical).with_layout(|l| {
                    l.height = Some(LengthSpec::Px(300.0));
                }),
            )
            .node_ref(page)
            .children(
                column().key("content").children((
                    widget(
                        Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(500.0))),
                    )
                    .visible(header),
                    each_virtual(
                        signal(virtual_items(10_000)),
                        |row| row.id,
                        20.0,
                        |row| text(row.title),
                    )
                    .within(page)
                    .overscan(0.0)
                    .key("rows"),
                )),
            )
        })
        .unwrap();
    let page = view.roots()[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    settle(&mut cx, document, viewport);
    let list = cx.resolve_assembly_path(page, "content/rows").unwrap();
    assert_eq!(
        cx.world().node(list).unwrap().parent,
        cx.resolve_assembly_path(page, "content"),
        "the list sits in the page's content, not in a scroll area of its own"
    );
    // Below the page's viewport: nothing past the list's first row.
    assert!(
        texts_under(&cx, list).len() <= 1,
        "{:?}",
        texts_under(&cx, list)
    );

    // 600 px down, the list's first 100 px are above the viewport.
    cx.scroll_to(
        Entity::from_stable_id(page),
        crate::ScrollOffset { x: 0.0, y: 600.0 },
    )
    .unwrap();
    settle(&mut cx, document, viewport);
    let rows = texts_under(&cx, list);
    assert_eq!(rows.first().map(String::as_str), Some("行 5"), "{rows:?}");
    assert!((15..=16).contains(&rows.len()), "{rows:?}");

    // Content above the list goes away with no scroll event: the window
    // follows where the list now sits.
    header_shown.get().unwrap().set(false);
    settle(&mut cx, document, viewport);
    let rows = texts_under(&cx, list);
    assert_eq!(rows.first().map(String::as_str), Some("行 30"), "{rows:?}");
}

#[test]
fn a_virtual_grid_fits_columns_to_its_width() {
    let (mut cx, document, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            each_virtual(
                signal(virtual_items(100)),
                |row| row.id,
                50.0,
                |row| text(row.title),
            )
            .grid(100.0, 10.0)
            .overscan(0.0)
            .height(200.0)
        })
        .unwrap();
    let scroll = view.roots()[0];
    let list = children(&cx, scroll)[0];
    let columns_at = |cx: &mut AppContext, width: f32| {
        settle(cx, document, LayoutViewport::new(width, 600.0));
        let list_width = cx.world().layout_box(list).unwrap().width;
        let expected = (((list_width + 10.0) / 110.0).floor() as usize).max(1);
        let rows = children(cx, list);
        (expected, rows.len(), texts_under(cx, rows[0]))
    };

    let (columns, rows, first) = columns_at(&mut cx, 360.0);
    assert!(columns >= 3, "{columns} columns in 360 px");
    assert_eq!(
        first,
        (0..columns)
            .map(|at| format!("行 {at}"))
            .collect::<Vec<_>>()
    );
    // 60 px a row (50 and the gap) in a 200 px viewport.
    assert!((4..=5).contains(&rows), "{rows} grid rows built");

    let (columns, _, first) = columns_at(&mut cx, 150.0);
    assert_eq!(columns, 1);
    assert_eq!(first, ["行 0"]);
}

/// A measured grid measures each cell at its column's width: a square cell
/// is as tall as the column is wide, and the rows do not overlap.
#[test]
fn a_measured_virtual_grid_measures_cells_at_their_column_width() {
    let (mut cx, document, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            each_virtual(
                signal(virtual_items(40)),
                |row| row.id,
                30.0,
                |_| {
                    widget(Stack::column(0.0).with_layout(|l| {
                        l.width = Some(LengthSpec::Fill);
                        l.aspect_ratio = Some(1.0);
                    }))
                },
            )
            .grid(100.0, 10.0)
            .measured()
            .overscan(0.0)
            .height(400.0)
            .width(340.0)
        })
        .unwrap();
    let scroll = view.roots()[0];
    let list = children(&cx, scroll)[0];
    settle(&mut cx, document, LayoutViewport::new(400.0, 600.0));
    // Three columns of (340 - 2 * 10) / 3 in 340 px.
    let column = (340.0 - 20.0) / 3.0;
    let mut placed = children(&cx, list)
        .into_iter()
        .map(|row| cx.world().layout_box(row).unwrap())
        .collect::<Vec<_>>();
    placed.sort_by(|a, b| a.y.total_cmp(&b.y));
    assert!(placed.len() >= 2, "{placed:?}");
    for pair in placed.windows(2) {
        assert!(
            (pair[0].height - (column + 10.0)).abs() < 0.5,
            "a grid row is a cell tall plus the gap: {placed:?}"
        );
        assert!(
            (pair[1].y - (pair[0].y + pair[0].height)).abs() < 0.5,
            "rows follow each other: {placed:?}"
        );
    }
}

/// A measured row is as tall as its content: something in it that fills
/// its parent's height has nothing definite to fill (the row's height is
/// the content's), instead of the list's height, which the rows' heights
/// make, so it grew every pass and the frame never settled.
#[test]
fn a_fill_height_inside_a_measured_row_does_not_feed_back_into_the_list() {
    let (mut cx, document, parent) = setup();
    let view = cx
        .mount_view(parent, || {
            each_virtual(
                signal(virtual_items(40)),
                |row| row.id,
                30.0,
                |_| {
                    column().children((
                        widget(Stack::column(0.0).with_layout(|l| {
                            l.height = Some(LengthSpec::Fill);
                        })),
                        widget(Stack::column(0.0).with_layout(|l| {
                            l.height = Some(LengthSpec::Px(24.0));
                        })),
                    ))
                },
            )
            .grid(100.0, 10.0)
            .measured()
            .overscan(0.0)
            .height(400.0)
            .width(340.0)
        })
        .unwrap();
    let scroll = view.roots()[0];
    let list = children(&cx, scroll)[0];
    let viewport = LayoutViewport::new(400.0, 600.0);
    settle(&mut cx, document, viewport);
    let heights = |cx: &AppContext| {
        children(cx, list)
            .into_iter()
            .map(|row| cx.world().layout_box(row).unwrap().height)
            .collect::<Vec<_>>()
    };
    let settled = heights(&cx);
    assert!(
        settled.iter().all(|height| (*height - 34.0).abs() < 0.5),
        "a cell 24 tall and the gap: {settled:?}"
    );
    settle(&mut cx, document, viewport);
    assert_eq!(heights(&cx), settled, "and it stays that tall");
}

/// A row 40 px tall keyed `row`, holding a text keyed `title`.
fn keyed_row(row: VirtualRow) -> impl IntoView {
    widget(Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(40.0))))
        .key("row")
        .children(text(row.title).key("title"))
}

/// A branch a `dynamic` builds when it switches is keyed where the first
/// one was: inside a virtual row, `a/b` resolves to the branch shown, before
/// and after the switch, and after switching back to a kept branch.
#[test]
fn keys_in_a_switched_branch_resolve_by_path_inside_a_virtual_row() {
    let (mut cx, document, parent) = setup();
    let rows = virtual_list_ref::<u32>();
    let list_ref = rows.clone();
    let modes = std::cell::Cell::new(None);
    let shared = &modes;
    cx.mount_view(parent, move || {
        let mode = signal(0u32);
        shared.set(Some(mode));
        each_virtual(
            signal(virtual_items(100)),
            |row| row.id,
            40.0,
            move |row| {
                let title = row.title.clone();
                dynamic(mode, move |mode: &u32| {
                    text(format!("{title} {mode}")).key("b")
                })
                .key("a")
            },
        )
        .overscan(0.0)
        .height(200.0)
        .list_ref(list_ref.clone())
    })
    .unwrap();
    settle(&mut cx, document, LayoutViewport::new(320.0, 600.0));
    let mode = modes.get().unwrap();
    let shown = |cx: &AppContext| {
        let row = rows.row(&0).expect("row 0 is built");
        let id = cx
            .resolve_assembly_path(row, "a/b")
            .expect("a/b resolves in the row");
        text_of(cx, Entity::from_stable_id(id))
    };
    assert_eq!(shown(&cx), "行 0 0");
    mode.set(1);
    cx.flush_reactive().unwrap();
    assert_eq!(shown(&cx), "行 0 1", "the rebuilt branch is keyed too");
    mode.set(2);
    cx.flush_reactive().unwrap();
    assert_eq!(shown(&cx), "行 0 2");
}

/// The same outside a virtual list: a `dynamic` at the top of a view, a
/// kept-alive one switching back to a branch it kept, and rows an `each`
/// adds after it was built, each keyed under its container.
#[test]
fn keys_of_branches_and_rows_built_after_mount_resolve_by_path() {
    let (mut cx, _, parent) = setup();
    let signals = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let mode = signal(0u32);
            let items = signal(vec![1u32]);
            signals.set(Some((mode, items)));
            column().key("page").children((
                dynamic(mode, |mode: &u32| text(format!("{mode}")).key("b")).key("switch"),
                dynamic(mode, |mode: &u32| text(format!("{mode}")).key("b"))
                    .keep_alive()
                    .key("kept"),
                each(
                    items,
                    |id| *id,
                    |id| {
                        column()
                            .key(format!("r{id}"))
                            .children(text(format!("行 {id}")).key("title"))
                    },
                )
                .key("list"),
            ))
        })
        .unwrap();
    let (mode, items) = signals.get().unwrap();
    let root = view.roots()[0];
    let at = |cx: &AppContext, path: &str| {
        let id = cx
            .resolve_assembly_path(root, path)
            .unwrap_or_else(|| panic!("{path} resolves"));
        text_of(cx, Entity::from_stable_id(id))
    };
    assert_eq!(at(&cx, "switch/b"), "0");
    mode.set(1);
    items.set(vec![1, 2]);
    cx.flush_reactive().unwrap();
    assert_eq!(at(&cx, "switch/b"), "1");
    assert_eq!(at(&cx, "kept/b"), "1");
    assert_eq!(at(&cx, "list/r2/title"), "行 2", "a row added later");
    assert_eq!(at(&cx, "list/r1/title"), "行 1");
    mode.set(0);
    cx.flush_reactive().unwrap();
    assert_eq!(at(&cx, "kept/b"), "0", "the kept branch, shown again");
    assert_eq!(at(&cx, "switch/b"), "0");
}

/// `.visible` on a virtual list hides its box and keeps it: the scroll area
/// it owns, or the list itself when it scrolls `within` a page.
#[test]
fn a_virtual_list_is_shown_and_hidden_on_its_container() {
    let (mut cx, document, parent) = setup();
    let state = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let (own, within) = (signal(true), signal(true));
            let page = node_ref();
            state.set(Some((own, within)));
            column().children((
                each_virtual(signal(virtual_items(100)), |row| row.id, 40.0, keyed_row)
                    .height(200.0)
                    .visible(own)
                    .key("own"),
                widget(
                    crate::ScrollView::new(crate::ScrollAxes::Vertical)
                        .with_layout(|l| l.height = Some(LengthSpec::Px(200.0))),
                )
                .node_ref(page)
                .key("page")
                .children(
                    each_virtual(signal(virtual_items(100)), |row| row.id, 40.0, keyed_row)
                        .within(page)
                        .visible(within)
                        .key("list"),
                ),
            ))
        })
        .unwrap();
    settle(&mut cx, document, LayoutViewport::new(320.0, 600.0));
    let (own, within) = state.get().unwrap();
    let root = view.roots()[0];
    let hidden = |cx: &AppContext, path: &str| {
        let id = cx.resolve_assembly_path(root, path).unwrap();
        cx.world().layout_style(id).unwrap().hidden
    };
    let scroll = cx.resolve_assembly_path(root, "own").unwrap();
    assert!(
        cx.read(Entity::<crate::ScrollView>::from_stable_id(scroll), |_| ())
            .is_ok(),
        "the list's own container is its scroll area"
    );
    assert!(!hidden(&cx, "own") && !hidden(&cx, "page/list"));
    own.set(false);
    within.set(false);
    settle(&mut cx, document, LayoutViewport::new(320.0, 600.0));
    assert!(hidden(&cx, "own"), "its own scroll area");
    assert!(hidden(&cx, "page/list"), "the list inside the page");
    assert!(!hidden(&cx, "page"), "the page it scrolls within stays");
    own.set(true);
    settle(&mut cx, document, LayoutViewport::new(320.0, 600.0));
    assert!(!hidden(&cx, "own"));
}

/// The list's ref answers where every item is, built or not, and which node
/// a built item is in: its key scope, where what the row keys is found.
#[test]
fn a_virtual_list_ref_knows_every_item_and_each_built_row() {
    let (mut cx, document, parent) = setup();
    let rows = virtual_list_ref::<u32>();
    let list_ref = rows.clone();
    cx.mount_view(parent, move || {
        each_virtual(signal(virtual_items(1_000)), |row| row.id, 40.0, keyed_row)
            .overscan(0.0)
            .height(200.0)
            .list_ref(list_ref.clone())
    })
    .unwrap();
    settle(&mut cx, document, LayoutViewport::new(320.0, 600.0));
    assert_eq!(
        rows.item(&700),
        Some(VirtualItem {
            offset: 28_000.0,
            extent: 40.0
        }),
        "an item never built is placed by its estimate"
    );
    assert_eq!(rows.item(&1_000), None);
    assert_eq!(rows.row(&700), None, "not built");
    // Every built row is its own key scope: `row/title` is that row's.
    for id in 0..5 {
        let row = rows.row(&id).expect("built");
        let title = cx.resolve_assembly_path(row, "row/title").unwrap();
        assert_eq!(cx.world().text(title), Some(format!("行 {id}").as_str()));
    }
    assert_eq!(rows.first_visible(), Some((0, 0.0)));
}

/// Scrolling to a key builds the rows around it and puts it where asked;
/// the reading position it leaves is the one to come back to.
#[test]
fn a_virtual_list_scrolls_to_an_item_by_key() {
    let (mut cx, document, parent) = setup();
    let rows = virtual_list_ref::<u32>();
    let list_ref = rows.clone();
    let view = cx
        .mount_view(parent, move || {
            each_virtual(signal(virtual_items(1_000)), |row| row.id, 20.0, keyed_row)
                .measured()
                .overscan(0.0)
                .height(200.0)
                .list_ref(list_ref.clone())
        })
        .unwrap();
    let scroll = view.roots()[0];
    let list = children(&cx, scroll)[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    settle(&mut cx, document, viewport);

    rows.scroll_to(&500, VirtualAlignment::Start);
    settle(&mut cx, document, viewport);
    let shown = texts_under(&cx, list);
    assert_eq!(
        shown.first().map(String::as_str),
        Some("行 500"),
        "{shown:?}"
    );
    assert_eq!(rows.first_visible(), Some((500, 0.0)));
    let row = rows.row(&500).unwrap();
    let offset = cx.world().scroll_offset(scroll).unwrap().y;
    let top = cx.world().layout_box(row).unwrap().y - cx.world().layout_box(scroll).unwrap().y;
    assert!(
        (top - offset).abs() < 0.5,
        "item 500 at the top: {top} vs {offset}"
    );

    rows.scroll_to(&900, VirtualAlignment::End);
    settle(&mut cx, document, viewport);
    let (first, _) = rows.first_visible().unwrap();
    assert!(
        (896..=897).contains(&first),
        "item 900 at the bottom: {first}"
    );

    rows.scroll_to_inset(&500, 10.0);
    settle(&mut cx, document, viewport);
    assert_eq!(rows.first_visible(), Some((500, 10.0)));
}

/// Items inserted above the one at the top of the viewport keep it where
/// it is (loading older messages above a chat), measured or not; items
/// added below do not move anything.
#[test]
fn a_virtual_list_keeps_its_reading_position_when_items_are_inserted_above() {
    for measured in [false, true] {
        let (mut cx, document, parent) = setup();
        let rows = virtual_list_ref::<u32>();
        let list_ref = rows.clone();
        let items = std::cell::Cell::new(None);
        let view = cx
            .mount_view(parent, || {
                let data = signal(
                    (1_000..1_100)
                        .map(|id| VirtualRow {
                            id,
                            title: format!("行 {id}"),
                        })
                        .collect::<Vec<_>>(),
                );
                items.set(Some(data));
                let list = each_virtual(data, |row| row.id, 20.0, keyed_row)
                    .overscan(0.0)
                    .height(200.0)
                    .list_ref(list_ref.clone());
                if measured { list.measured() } else { list }
            })
            .unwrap();
        let scroll = view.roots()[0];
        let viewport = LayoutViewport::new(320.0, 600.0);
        settle(&mut cx, document, viewport);
        rows.scroll_to_inset(&1_050, 7.0);
        settle(&mut cx, document, viewport);
        let on_screen = |cx: &AppContext| {
            let row = rows.row(&1_050).expect("the anchor stays built");
            cx.world().layout_box(row).unwrap().y
                - cx.world().layout_box(scroll).unwrap().y
                - cx.world().scroll_offset(scroll).unwrap().y
        };
        let before = on_screen(&cx);
        assert!((before + 7.0).abs() < 0.5, "{before}");

        // A page of older items arrives above.
        items.get().unwrap().update(|data| {
            let older = (800..1_000).map(|id| VirtualRow {
                id,
                title: format!("行 {id}"),
            });
            data.splice(0..0, older);
        });
        settle(&mut cx, document, viewport);
        assert_eq!(
            rows.first_visible(),
            Some((1_050, 7.0)),
            "measured {measured}"
        );
        assert!((on_screen(&cx) - before).abs() < 0.5, "measured {measured}");

        // Newer items below move nothing.
        let offset = cx.world().scroll_offset(scroll).unwrap().y;
        items.get().unwrap().update(|data| {
            data.extend((1_100..1_110).map(|id| VirtualRow {
                id,
                title: format!("行 {id}"),
            }))
        });
        settle(&mut cx, document, viewport);
        assert_eq!(cx.world().scroll_offset(scroll).unwrap().y, offset);
    }
}

/// A list scrolling with an ancestor follows its ref to another scroll
/// area when it moves there (a sidebar list moved between layouts), and
/// lets go of the first.
#[test]
fn a_virtual_list_within_follows_its_ref_to_another_scroll_area() {
    let (mut cx, document, parent) = setup();
    let refs = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let (a, b, page) = (node_ref(), node_ref(), node_ref());
            refs.set(Some((a, b, page)));
            let area = |r| {
                widget(
                    crate::ScrollView::new(crate::ScrollAxes::Vertical)
                        .with_layout(|l| l.height = Some(LengthSpec::Px(200.0))),
                )
                .node_ref(r)
                .children(column().key("content"))
            };
            (
                area(a),
                area(b),
                teleport(
                    page,
                    each_virtual(
                        signal(virtual_items(1_000)),
                        |row| row.id,
                        20.0,
                        |row| text(row.title),
                    )
                    .within(page)
                    .overscan(0.0)
                    .key("rows"),
                ),
            )
        })
        .unwrap();
    let (a, b, page) = refs.get().unwrap();
    let viewport = LayoutViewport::new(320.0, 900.0);
    page.set(a.get_untracked());
    settle(&mut cx, document, viewport);
    let area_a = Entity::<crate::ScrollView>::from_stable_id(a.get_untracked().unwrap());
    let area_b = Entity::<crate::ScrollView>::from_stable_id(b.get_untracked().unwrap());
    // Teleported, the list keeps its key under the place it is declared.
    let list = cx.resolve_assembly_path(view.roots()[2], "rows").unwrap();
    let first = |cx: &AppContext| texts_under(cx, list).first().cloned();
    cx.scroll_to(area_a, crate::ScrollOffset { x: 0.0, y: 2_000.0 })
        .unwrap();
    settle(&mut cx, document, viewport);
    assert_eq!(first(&cx).as_deref(), Some("行 100"));

    // The list moves to the other area: that one's scrolling moves the
    // window now, and the first area's no longer does.
    page.set(b.get_untracked());
    settle(&mut cx, document, viewport);
    cx.scroll_to(area_b, crate::ScrollOffset { x: 0.0, y: 400.0 })
        .unwrap();
    settle(&mut cx, document, viewport);
    assert_eq!(first(&cx).as_deref(), Some("行 20"));
    cx.scroll_to(area_a, crate::ScrollOffset { x: 0.0, y: 0.0 })
        .unwrap();
    settle(&mut cx, document, viewport);
    assert_eq!(
        first(&cx).as_deref(),
        Some("行 20"),
        "the first area is let go of"
    );
}

#[test]
fn a_measured_virtual_list_keeps_measured_heights_when_rows_are_added() {
    let (mut cx, document, parent) = setup();
    let items = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let list = signal(virtual_items(100));
            items.set(Some(list));
            each_virtual(
                list,
                |row| row.id,
                20.0,
                |row| {
                    widget(
                        Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(40.0))),
                    )
                    .children(text(row.title))
                },
            )
            .measured()
            .overscan(0.0)
            .height(200.0)
        })
        .unwrap();
    let scroll = view.roots()[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    let first = |cx: &AppContext| texts_under(cx, children(cx, scroll)[0]).first().cloned();
    settle(&mut cx, document, viewport);
    // Ten 40 px rows down: rows 0–9 were measured and are scrolled away.
    cx.scroll_to(
        Entity::from_stable_id(scroll),
        crate::ScrollOffset { x: 0.0, y: 400.0 },
    )
    .unwrap();
    settle(&mut cx, document, viewport);
    assert_eq!(first(&cx).as_deref(), Some("行 10"));

    // A page of rows arrives. The rows above keep their measured 40 px, so
    // the same rows stay in view instead of those 400 px of 20 px estimates
    // would reach.
    items
        .get()
        .unwrap()
        .update(|list| list.extend(virtual_items(120).into_iter().skip(100)));
    settle(&mut cx, document, viewport);
    assert_eq!(first(&cx).as_deref(), Some("行 10"));
}

#[test]
fn a_virtual_row_scrolled_away_releases_its_scope() {
    let (mut cx, document, parent) = setup();
    use std::sync::atomic::{AtomicI32, Ordering::SeqCst};
    let live = std::sync::Arc::new(AtomicI32::new(0));
    let rows = std::sync::Arc::clone(&live);
    let view = cx
        .mount_view(parent, move || {
            each_virtual(
                signal(virtual_items(1_000)),
                |row| row.id,
                20.0,
                move |row| {
                    rows.fetch_add(1, SeqCst);
                    let rows = std::sync::Arc::clone(&rows);
                    on_cleanup(move || {
                        rows.fetch_sub(1, SeqCst);
                    });
                    text(row.title)
                },
            )
            .overscan(0.0)
            .height(200.0)
        })
        .unwrap();
    let scroll = view.roots()[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    settle(&mut cx, document, viewport);
    let list = children(&cx, scroll)[0];
    assert_eq!(
        cx.world().layout_box(list).unwrap().width,
        cx.world().layout_box(scroll).unwrap().width,
        "the list is as wide as its scroll area, so rows wrap to it"
    );
    let built = live.load(SeqCst);
    assert!((10..=12).contains(&built), "{built} rows live");

    cx.scroll_to(
        Entity::from_stable_id(scroll),
        crate::ScrollOffset {
            x: 0.0,
            y: 10_000.0,
        },
    )
    .unwrap();
    settle(&mut cx, document, viewport);
    assert_eq!(
        live.load(SeqCst),
        texts_under(&cx, list).len() as i32,
        "rows scrolled away were cleaned up, not kept alive"
    );
}

#[test]
fn a_scroll_area_told_to_follow_its_end_goes_there() {
    let (mut cx, document, parent) = setup();
    let follow = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let end = signal(false);
            follow.set(Some(end));
            widget(
                crate::ScrollView::new(crate::ScrollAxes::Vertical).with_layout(|l| {
                    l.height = Some(LengthSpec::Px(100.0));
                }),
            )
            .bind(move |scroll| scroll.follow_end = end.get())
            .children(widget(
                Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(1_000.0))),
            ))
        })
        .unwrap();
    let scroll = view.roots()[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    settle(&mut cx, document, viewport);
    assert_eq!(cx.world().scroll_offset(scroll).unwrap_or_default().y, 0.0);

    follow.get().unwrap().set(true);
    cx.flush_reactive().unwrap();
    let max = cx.world().scroll_metrics(scroll).unwrap().max_offset().y;
    assert!(max > 0.0);
    assert_eq!(cx.world().scroll_offset(scroll).unwrap().y, max);
}

#[test]
fn a_form_field_control_and_a_modal_close_action_mount_as_their_setters_place_them() {
    use crate::{Dialog, FormField, ModalSlots};
    let (mut cx, document, _) = setup();
    let field = cx
        .create_component(document, FormField::new("邮箱"))
        .unwrap();
    let input = cx
        .create_detached_component(document, TextInput::new(""))
        .unwrap();
    cx.set_form_field_control(field, Some(input.stable_id()))
        .unwrap();
    let dialog = cx
        .create_component(document, Dialog::new("重命名"))
        .unwrap();
    let body = cx
        .create_detached_component(document, Text::new("正文"))
        .unwrap();
    let close = cx
        .create_detached_component(document, Button::new("关闭"))
        .unwrap();
    cx.set_modal_slots(
        dialog,
        ModalSlots {
            body: Some(body.stable_id()),
            close_action: Some(close.stable_id()),
            ..ModalSlots::default()
        },
    )
    .unwrap();

    let views = cx
        .mount_view_root(document, || {
            (
                widget(FormField::new("邮箱")).control(text_input()),
                widget(Dialog::new("重命名"))
                    .body(text("正文"))
                    .close_action(button("关闭")),
            )
        })
        .unwrap();
    assert_eq!(shape(&cx, views.roots()[0]), shape(&cx, field.stable_id()));
    assert_eq!(shape(&cx, views.roots()[1]), shape(&cx, dialog.stable_id()));
    let viewed = Entity::<FormField>::from_stable_id(views.roots()[0]);
    assert_eq!(
        cx.read(viewed, |field| field.control).unwrap(),
        Some(children(&cx, viewed.stable_id())[0])
    );
}

#[test]
fn a_confirm_dialog_places_the_actions_it_is_given_and_makes_the_rest() {
    use crate::{ConfirmDialog, ConfirmSlots};
    let (mut cx, document, _) = setup();
    let by_hand = cx
        .create_component(document, ConfirmDialog::new("删除？", "不能撤销"))
        .unwrap();
    let parts = [
        cx.create_detached_component(document, Text::new("详情"))
            .unwrap()
            .stable_id(),
        cx.create_detached_component(document, Button::new("关闭"))
            .unwrap()
            .stable_id(),
        cx.create_detached_component(document, Button::new("算了"))
            .unwrap()
            .stable_id(),
        cx.create_detached_component(document, Button::new("稍后"))
            .unwrap()
            .stable_id(),
        cx.create_detached_component(document, Button::new("删掉"))
            .unwrap()
            .stable_id(),
    ];
    cx.set_confirm_slots(
        by_hand,
        ConfirmSlots {
            body: Some(parts[0]),
            close_action: Some(parts[1]),
            cancel: parts[2],
            secondary: Some(parts[3]),
            confirm: parts[4],
        },
    )
    .unwrap();

    let (view, (given, partly)) = cx
        .mount_view_root(document, || {
            let (given, partly) = (entity_ref::<ConfirmDialog>(), entity_ref::<ConfirmDialog>());
            with_refs(
                (
                    widget(ConfirmDialog::new("删除？", "不能撤销"))
                        .entity_ref(given)
                        .body(text("详情"))
                        .close_action(button("关闭"))
                        .cancel(button("算了"))
                        .secondary(button("稍后"))
                        .confirm(button("删掉")),
                    widget(
                        ConfirmDialog::new("删除？", "不能撤销")
                            .confirm_label("删除")
                            .cancel_label("保留"),
                    )
                    .entity_ref(partly)
                    .cancel(button("算了")),
                ),
                (given, partly),
            )
        })
        .unwrap();
    assert_eq!(shape(&cx, view.roots()[0]), shape(&cx, by_hand.stable_id()));
    // Only the missing confirm action is made, from the label.
    let labels = children(&cx, partly.stable_id())
        .into_iter()
        .map(|id| {
            cx.read(Entity::<Button>::from_stable_id(id), |button| {
                button.label.clone()
            })
            .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, ["算了", "删除"]);
    // Reassembly refreshes what it made and leaves the given buttons alone.
    cx.set_confirm_state(given, true, false).unwrap();
    cx.assemble_confirm_dialog(given).unwrap();
    cx.assemble_confirm_dialog(partly).unwrap();
    let cancel = children(&cx, given.stable_id())
        .into_iter()
        .find(|id| {
            cx.read(Entity::<Button>::from_stable_id(*id), |b| b.label == "算了")
                .unwrap_or(false)
        })
        .unwrap();
    assert!(
        !cx.read(Entity::<Button>::from_stable_id(cancel), |b| b.disabled)
            .unwrap()
    );
}

#[test]
fn a_pane_chrome_lays_out_its_tabs_actions_and_body() {
    use crate::{PaneChrome, PaneChromeAction, PaneChromeActionKind};
    let (mut cx, document, _) = setup();
    let split_shown = signal(true);
    let (view, (chrome, tabs, split, close, body)) = cx
        .mount_view_root(document, || {
            let refs = (
                entity_ref::<PaneChrome>(),
                entity_ref::<Text>(),
                entity_ref::<Button>(),
                entity_ref::<Button>(),
                entity_ref::<Text>(),
            );
            let (chrome, tabs, split, close, body) = refs;
            let view = widget(PaneChrome::new())
                .entity_ref(chrome)
                .tabs(text("main.rs").entity_ref(tabs))
                .action(
                    PaneChromeAction::new(PaneChromeActionKind::SplitHorizontal, "分栏"),
                    button("分栏").entity_ref(split),
                )
                .action(
                    PaneChromeAction::new(PaneChromeActionKind::CloseItem, "关闭")
                        .icon(nana_ui_core::Icon::Close),
                    button("关闭").entity_ref(close),
                )
                .body(text("编辑器").entity_ref(body))
                .bind(move |chrome: &mut PaneChrome| {
                    if !split_shown.get() {
                        chrome
                            .actions
                            .retain(|action| action.kind != PaneChromeActionKind::SplitHorizontal);
                    }
                });
            with_refs(view, refs)
        })
        .unwrap();
    let chrome_id = chrome.stable_id();
    assert_eq!(view.roots(), [chrome_id]);
    let [header, body_id] = children(&cx, chrome_id)[..] else {
        panic!("{}", shape(&cx, chrome_id));
    };
    assert_eq!(body_id, body.stable_id());
    assert_eq!(
        children(&cx, header),
        [tabs.stable_id(), split.stable_id(), close.stable_id()]
    );
    assert_eq!(cx.read(chrome, |c| c.header).unwrap(), Some(header));

    // An action a binding drops leaves the row; its control stays alive.
    split_shown.set(false);
    cx.flush_reactive().unwrap();
    assert_eq!(children(&cx, header), [tabs.stable_id(), close.stable_id()]);
    assert!(cx.world().contains(split.stable_id()));
    assert_eq!(children(&cx, chrome_id)[0], header);
}

#[test]
fn a_pane_chrome_leaves_the_children_of_a_header_it_is_given() {
    use crate::PaneChrome;
    let (mut cx, document, _) = setup();
    let view = cx
        .mount_view_root(document, || {
            widget(PaneChrome::new())
                .header(row().children((text("甲"), text("乙"))))
                .tabs(text("另建"))
                .body(text("正文"))
        })
        .unwrap();
    let chrome = view.roots()[0];
    let [header, _] = children(&cx, chrome)[..] else {
        panic!("{}", shape(&cx, chrome));
    };
    let texts = children(&cx, header)
        .into_iter()
        .map(|id| cx.world().text(id).unwrap_or("").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(texts, ["甲", "乙"]);
}

#[test]
fn a_modal_focuses_a_control_it_was_given_in_a_slot() {
    use crate::{ConfirmDialog, ModalInitialFocus};
    let (mut cx, document, _) = setup();
    let (_, (dialog, accept)) = cx
        .mount_view_root(document, || {
            let (dialog, accept) = (entity_ref::<ConfirmDialog>(), entity_ref::<Button>());
            with_refs(
                widget(ConfirmDialog::new("场景已停止", "要退出吗？"))
                    .entity_ref(dialog)
                    .confirm(button("退出").entity_ref(accept))
                    .initial_focus_on(accept),
                (dialog, accept),
            )
        })
        .unwrap();
    assert_eq!(
        cx.read(dialog, |dialog| dialog.behavior().initial_focus)
            .unwrap(),
        ModalInitialFocus::Target(accept.stable_id())
    );
}

#[test]
fn a_page_scrolled_past_its_list_stays_put_when_its_items_change() {
    let (mut cx, document, parent) = setup();
    let items = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let page = node_ref();
            let list = signal(virtual_items(100));
            items.set(Some(list));
            widget(
                crate::ScrollView::new(crate::ScrollAxes::Vertical).with_layout(|l| {
                    l.height = Some(LengthSpec::Px(300.0));
                }),
            )
            .node_ref(page)
            .children(
                column().key("content").children((
                    each_virtual(list, |row| row.id, 20.0, |row| text(row.title))
                        .within(page)
                        .overscan(0.0)
                        .key("rows"),
                    // Content after the list: the page scrolls past the list's end.
                    widget(
                        Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(120.0))),
                    ),
                )),
            )
        })
        .unwrap();
    let page = view.roots()[0];
    let viewport = LayoutViewport::new(320.0, 600.0);
    settle(&mut cx, document, viewport);
    cx.scroll_to(
        Entity::from_stable_id(page),
        crate::ScrollOffset {
            x: 0.0,
            y: 1_000_000.0,
        },
    )
    .unwrap();
    settle(&mut cx, document, viewport);
    let bottom = cx.world().scroll_offset(page).unwrap().y;

    // Two rows far above swap places: the list is as long as before and the
    // page scrolled past its end stays where it is.
    items.get().unwrap().update(|list| list.swap(0, 1));
    settle(&mut cx, document, viewport);
    assert_eq!(cx.world().scroll_offset(page).unwrap().y, bottom);
}

/// A container that listens for `SizeChanged` hears the size layout gave it,
/// once per change: its first layout and each resize, not every pass.
#[test]
fn a_listening_container_hears_the_size_layout_gave_it() {
    use std::sync::{Arc, Mutex};
    let (mut cx, document, parent) = setup();
    let sizes = Arc::new(Mutex::new(Vec::new()));
    let heard = Arc::clone(&sizes);
    let width = std::cell::Cell::new(None);
    let bound = &width;
    cx.mount_view(parent, move || {
        let two_columns = signal(false);
        bound.set(Some(two_columns));
        column()
            .on(move |event: &crate::SizeChanged| {
                heard.lock().unwrap().push((event.width, event.height));
                two_columns.set(event.width >= 360.0);
            })
            .children(widget(Stack::column(0.0).with_layout(|layout| {
                layout.height = Some(LengthSpec::Px(40.0));
            })))
    })
    .unwrap();
    let two_columns = width.get().unwrap();
    cx.layout_document(document, LayoutViewport::new(400.0, 300.0))
        .unwrap();
    assert_eq!(*sizes.lock().unwrap(), [(400.0, 40.0)]);
    cx.flush_reactive().unwrap();
    assert!(two_columns.get_untracked());
    // The same size again: nothing to hear.
    cx.layout_document(document, LayoutViewport::new(400.0, 300.0))
        .unwrap();
    assert_eq!(sizes.lock().unwrap().len(), 1);
    cx.layout_document(document, LayoutViewport::new(320.0, 300.0))
        .unwrap();
    assert_eq!(*sizes.lock().unwrap(), [(400.0, 40.0), (320.0, 40.0)]);
    cx.flush_reactive().unwrap();
    assert!(!two_columns.get_untracked());
}

/// A title bar is shown and hidden from a view like any styled element (a
/// fullscreen player hides the window's bar).
#[test]
fn a_title_bar_shows_and_hides_from_a_view() {
    let (mut cx, document, parent) = setup();
    let flag = std::cell::Cell::new(None);
    let view = cx
        .mount_view(parent, || {
            let shown = signal(true);
            flag.set(Some(shown));
            column().children((
                widget(crate::AppTitleBar::new("Nana").native_controls(false))
                    .visible(shown)
                    .key("bar"),
                widget(Stack::column(0.0).with_layout(|l| l.height = Some(LengthSpec::Px(40.0))))
                    .key("page"),
            ))
        })
        .unwrap();
    let root = view.roots()[0];
    let page = cx.resolve_assembly_path(root, "page").unwrap();
    let viewport = LayoutViewport::new(800.0, 400.0);
    cx.layout_document(document, viewport).unwrap();
    let below_bar = cx.world().layout_box(page).unwrap().y;
    assert!(below_bar > 0.0);
    flag.get().unwrap().set(false);
    cx.flush_reactive().unwrap();
    cx.layout_document(document, viewport).unwrap();
    assert_eq!(cx.world().layout_box(page).unwrap().y, 0.0);
}
