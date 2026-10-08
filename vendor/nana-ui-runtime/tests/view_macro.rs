//! `view!` expands to the function API: a template and its hand-written
//! equivalent mount the same retained tree and update it the same way.
#![cfg(feature = "view-macro")]

use nana_ui_runtime::view::{
    EachExt, IntoView, Signal, WhenExt, button, checkbox, column, each, row, signal, slider, text,
    text_input, when, widget,
};
use nana_ui_runtime::{
    AppContext, DocumentId, Entity, RangeChanged, StableNodeId, Stack, Text, view,
};

/// A mounted page's signals and its tree.
trait Probe {
    fn tree(&self) -> nana_ui_runtime::view::AnyView;
    fn poke(&self);
}

/// Every node under `root`: kind, text, children, in document order.
fn dump(cx: &AppContext, root: StableNodeId) -> String {
    fn walk(cx: &AppContext, id: StableNodeId, depth: usize, out: &mut String) {
        let node = cx.world().node(id).unwrap();
        out.push_str(&"  ".repeat(depth));
        out.push_str(&format!("{:?}", node.kind));
        if let Some(text) = cx.world().text(id) {
            out.push_str(&format!(" {text:?}"));
        }
        out.push('\n');
        for child in node.children.iter() {
            walk(cx, *child, depth + 1, out);
        }
    }
    let mut out = String::new();
    walk(cx, root, 0, &mut out);
    out
}

fn assert_same(template: &dyn Fn() -> Box<dyn Probe>, functions: &dyn Fn() -> Box<dyn Probe>) {
    let mut cx_t = AppContext::new();
    let mut cx_f = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mut probes = Vec::new();
    let t = cx_t
        .mount_view_root(document, || {
            let probe = template();
            let tree = probe.tree();
            probes.push(probe);
            tree
        })
        .unwrap();
    let f = cx_f
        .mount_view_root(document, || {
            let probe = functions();
            let tree = probe.tree();
            probes.push(probe);
            tree
        })
        .unwrap();
    let before = dump(&cx_t, t.roots()[0]);
    assert_eq!(before, dump(&cx_f, f.roots()[0]));
    for probe in &probes {
        probe.poke();
    }
    cx_t.flush_reactive().unwrap();
    cx_f.flush_reactive().unwrap();
    let after = dump(&cx_t, t.roots()[0]);
    assert_eq!(after, dump(&cx_f, f.roots()[0]));
    assert_ne!(before, after, "poking changed something");
}

#[derive(Clone)]
struct Todo {
    id: u32,
    title: &'static str,
}

fn todo_row(todo: Todo, done: bool) -> impl IntoView {
    row()
        .gap(4.0)
        .children((text(todo.title), checkbox("完成").checked(done)))
}

struct Page {
    count: Signal<u64>,
    loading: Signal<bool>,
    mode: Signal<u8>,
    todos: Signal<Vec<Todo>>,
    volume: Signal<f64>,
    shown: Signal<bool>,
    form: Form,
}

/// The three ways to write the page.
#[derive(Clone, Copy, PartialEq)]
enum Form {
    Template,
    /// Function calls with tuple children.
    Tuple,
    /// Function calls in their block, data-first spelling.
    Block,
}

impl Page {
    fn new(form: Form) -> Self {
        Self {
            count: signal(0),
            loading: signal(true),
            mode: signal(0),
            todos: signal(vec![
                Todo {
                    id: 1, title: "一"
                },
                Todo {
                    id: 2, title: "二"
                },
            ]),
            volume: signal(0.5),
            shown: signal(true),
            form,
        }
    }
}

impl Probe for Page {
    fn tree(&self) -> nana_ui_runtime::view::AnyView {
        let Page {
            count,
            loading,
            mode,
            todos,
            volume,
            shown,
            ..
        } = *self;
        if self.form == Form::Block {
            return column()
                .gap(12)
                .with(|c| {
                    c.add(nana_ui_runtime::text!("计数 {count}"));
                    c.add(
                        button("加一")
                            .on_activate(move || count.update(|c| *c += 1))
                            .disabled(loading),
                    );
                    c.add(loading.then_show(|| text("加载中")).otherwise(move || {
                        (move || mode.get() == 1)
                            .then_show(|| text("模式一"))
                            .otherwise(|| text("完成"))
                    }));
                    c.add(
                        slider(0.0, 1.0, 0.05)
                            .label("音量")
                            .model(volume)
                            .on::<RangeChanged>(|_: &RangeChanged| {}),
                    );
                    c.add(
                        text_input()
                            .label("名字")
                            .placeholder("名字")
                            .visible(shown)
                            .key("name"),
                    );
                    c.add(todos.each(|t| t.id, |t| todo_row(t, false)));
                    c.add(widget(Stack::row(2.0)).with(|r| {
                        let first = "a";
                        r.add(text(first));
                        r.add(text(move || format!("b{}", count.get())));
                    }));
                    c.add(text("尾"));
                })
                .into_any();
        }
        if self.form == Form::Template {
            view! {
                <Column gap=12>
                    <Text>"计数 {count}"</Text>
                    <Button @activate={count.update(|c| *c += 1)} disabled={loading}>"加一"</Button>
                    <Text v-if={loading}>"加载中"</Text>
                    <Text v-else-if={mode.get() == 1}>"模式一"</Text>
                    <Text v-else>"完成"</Text>
                    <Slider min=0 max=1 step=0.05 label="音量" v-model={volume} on:RangeChanged={|_: &RangeChanged| {}} />
                    <TextInput label="名字" placeholder="名字" v-show={shown} key="name" />
                    <TodoRow v-for={t in todos} key={t.id} todo={t} done={false} />
                    <Widget of={Stack::row(2.0)}>
                        <Text>"a"</Text>
                        <Text>{format!("b{}", count.get())}</Text>
                    </Widget>
                    {text("尾")}
                </Column>
            }
            .into_any()
        } else {
            column()
                .gap(12.0)
                .children((
                    nana_ui_runtime::text!("计数 {count}"),
                    button("加一")
                        .on_activate(move || {
                            count.update(|c| *c += 1);
                        })
                        .disabled(loading),
                    when(loading, move || text("加载中")).otherwise(move || {
                        when(move || mode.get() == 1, move || text("模式一"))
                            .otherwise(move || text("完成"))
                    }),
                    slider(0.0, 1.0, 0.05)
                        .label("音量")
                        .model(volume)
                        .on::<RangeChanged>(|_: &RangeChanged| {}),
                    text_input()
                        .label("名字")
                        .placeholder("名字")
                        .visible(shown)
                        .key("name"),
                    each(todos, move |t: &Todo| t.id, move |t| todo_row(t, false)),
                    widget(Stack::row(2.0))
                        .children((text("a"), text(move || format!("b{}", count.get())))),
                    text("尾"),
                ))
                .into_any()
        }
    }

    fn poke(&self) {
        self.count.set(3);
        self.loading.set(false);
        self.mode.set(1);
        self.todos.update(|list| {
            list.reverse();
            list.push(Todo {
                id: 3, title: "三"
            });
        });
        self.volume.set(0.25);
        self.shown.set(false);
    }
}

#[test]
fn a_template_mounts_and_updates_exactly_like_its_function_calls() {
    assert_same(&|| Box::new(Page::new(Form::Template)), &|| {
        Box::new(Page::new(Form::Tuple))
    });
}

#[test]
fn the_block_spelling_mounts_and_updates_like_the_template() {
    assert_same(&|| Box::new(Page::new(Form::Template)), &|| {
        Box::new(Page::new(Form::Block))
    });
}

/// Named slots: `<template #name>` in a template is `.name(view)` in Rust.
struct Slots {
    checked: Signal<bool>,
    template: bool,
}

impl Probe for Slots {
    fn tree(&self) -> nana_ui_runtime::view::AnyView {
        use nana_ui_runtime::DesktopShell;
        use nana_ui_runtime::view::{settings_row, switch};
        let checked = self.checked;
        if self.template {
            return view! {
                <Widget of={DesktopShell::new().title("T")}>
                    <template #title-trailing>
                        <Row gap=6><Text>"搜索"</Text></Row>
                    </template>
                    <template #navigation><Text>"导航"</Text></template>
                    <template #primary>
                        <SettingsRow label="高亮">
                            <template #control><Switch label="开关" checked={checked} /></template>
                        </SettingsRow>
                    </template>
                </Widget>
            }
            .into_any();
        }
        widget(DesktopShell::new().title("T"))
            .title_trailing(row().gap(6).children(text("搜索")))
            .navigation(text("导航"))
            .primary(settings_row("高亮").control(switch("开关").checked(checked)))
            .into_any()
    }

    fn poke(&self) {
        self.checked.set(true);
    }
}

#[test]
fn named_slots_mount_like_their_methods() {
    let mut cx_t = AppContext::new();
    let mut cx_f = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let t = cx_t
        .mount_view_root(document, || {
            Slots {
                checked: signal(false),
                template: true,
            }
            .tree()
        })
        .unwrap();
    let f = cx_f
        .mount_view_root(document, || {
            Slots {
                checked: signal(false),
                template: false,
            }
            .tree()
        })
        .unwrap();
    let dump_t = dump(&cx_t, t.roots()[0]);
    assert_eq!(dump_t, dump(&cx_f, f.roots()[0]));
    for needle in ["搜索", "导航", "高亮"] {
        assert!(dump_t.contains(needle), "{needle}: {dump_t}");
    }
}

#[test]
fn more_than_twelve_children_nest_into_tuples() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let view = cx
        .mount_view_root(document, || {
            view! {
                <Column>
                    "1" "2" "3" "4" "5" "6" "7" "8" "9" "10" "11" "12" "13" "14"
                </Column>
            }
        })
        .unwrap();
    let texts: Vec<String> = cx
        .world()
        .node(view.roots()[0])
        .unwrap()
        .children
        .iter()
        .map(|id| {
            cx.read(Entity::<Text>::from_stable_id(*id), |t| t.value.clone())
                .unwrap()
        })
        .collect();
    assert_eq!(texts, (1..=14).map(|n| n.to_string()).collect::<Vec<_>>());
}

#[derive(Clone)]
struct Item {
    id: u32,
    title: String,
}

fn item_row(item: Item, list: Signal<Vec<Item>>) -> impl IntoView {
    let id = item.id;
    view! {
        <Row gap=8>
            <Text>{item.title}</Text>
            <Button key="remove" @activate={list.update(|list| list.retain(|t| t.id != id))}>"删除"</Button>
        </Row>
    }
}

#[test]
fn a_template_page_adds_and_removes_rows_through_its_handlers() {
    use nana_ui_runtime::Button;

    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let state = std::cell::Cell::new(None);
    let view = cx
        .mount_view_root(document, || {
            let list: Signal<Vec<Item>> = signal(Vec::new());
            let next = signal(1u32);
            state.set(Some(list));
            let add = move || {
                let id = next.get_untracked();
                next.set(id + 1);
                list.update(|list| {
                    list.push(Item {
                        id,
                        title: format!("任务 {id}"),
                    })
                });
            };
            view! {
                <Column>
                    <Button key="add" @activate={add}>"添加"</Button>
                    <ItemRow v-for={item in list} key={item.id} item={item} list={list} />
                    <Text v-if={list.with(Vec::is_empty)}>"空"</Text>
                    <Text v-else>{format!("共 {} 项", list.with(Vec::len))}</Text>
                </Column>
            }
        })
        .unwrap();
    let root = view.roots()[0];
    let add = cx.resolve_assembly_entity::<Button>(root, "add").unwrap();
    let parts = |cx: &AppContext| cx.world().node(root).unwrap().children.to_vec();
    let text_of = |cx: &AppContext, id: StableNodeId| {
        cx.read(Entity::<Text>::from_stable_id(id), |t| t.value.clone())
            .unwrap()
    };
    let summary = |cx: &AppContext| {
        let block = parts(cx)[2];
        text_of(cx, cx.world().node(block).unwrap().children[0])
    };
    assert_eq!(summary(&cx), "空");
    cx.activate_button(add).unwrap();
    cx.activate_button(add).unwrap();
    cx.flush_reactive().unwrap();
    let rows = cx.world().node(parts(&cx)[1]).unwrap().children.to_vec();
    assert_eq!(rows.len(), 2);
    assert_eq!(summary(&cx), "共 2 项");
    let first_title = cx.world().node(rows[0]).unwrap().children[0];
    assert_eq!(text_of(&cx, first_title), "任务 1");
    let remove = cx
        .resolve_assembly_entity::<Button>(rows[0], "remove")
        .unwrap();
    cx.activate_button(remove).unwrap();
    cx.flush_reactive().unwrap();
    let rows_after = cx.world().node(parts(&cx)[1]).unwrap().children.to_vec();
    assert_eq!(rows_after, vec![rows[1]], "the other row kept its node");
    assert_eq!(summary(&cx), "共 1 项");
    assert_eq!(state.get().unwrap().with_untracked(Vec::len), 1);
}

#[test]
fn table_controls_take_their_attributes_in_templates() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let flags = std::cell::Cell::new(None);
    let mounted = cx
        .mount_view(parent.stable_id(), || {
            let on = signal(true);
            let amount = signal(4.0f64);
            flags.set(Some(amount));
            view! {
                <Column gap=4>
                    <Switch v-model={on} key="switch">"通知"</Switch>
                    <NumberInput v-model={amount} label="数量" placeholder={"数量"} key="number" />
                    <Progress max=10 value={amount.get()} label="进度" key="progress" />
                    <Divider />
                </Column>
            }
        })
        .unwrap();
    let root = mounted.roots()[0];
    let amount = flags.get().unwrap();
    amount.set(6.0);
    cx.flush_reactive().unwrap();
    let progress = cx
        .resolve_assembly_entity::<nana_ui_runtime::Progress>(root, "progress")
        .unwrap();
    assert_eq!(
        cx.read(progress, |p| (p.value, p.max)).unwrap(),
        (6.0, 10.0)
    );
    let switch = cx
        .resolve_assembly_entity::<nana_ui_runtime::Switch>(root, "switch")
        .unwrap();
    assert_eq!(
        cx.read(switch, |s| (s.checked, s.label.clone())).unwrap(),
        (true, "通知".to_owned())
    );
}

#[test]
fn v_virtual_in_a_template_mounts_a_virtual_list() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let mounted = cx
        .mount_view(parent.stable_id(), || {
            let rows: Signal<Vec<u32>> = signal((0..10_000).collect());
            view! {
                <Text v-for={n in rows} key={*n} v-virtual={20}>"行 {n}"</Text>
            }
        })
        .unwrap();
    let scroll = mounted.roots()[0];
    assert!(
        cx.read(
            Entity::<nana_ui_runtime::ScrollView>::from_stable_id(scroll),
            |_| ()
        )
        .is_ok(),
        "the rows live in a ScrollView"
    );
}

#[test]
fn a_virtual_element_in_a_template_sizes_its_scroll_area() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let mounted = cx
        .mount_view(parent.stable_id(), || {
            let rows: Signal<Vec<u32>> = signal((0..10_000).collect());
            view! {
                <Virtual row_height=20 height=200 measured>
                    <Text v-for={n in rows} key={*n}>"行 {n}"</Text>
                </Virtual>
            }
        })
        .unwrap();
    let scroll = mounted.roots()[0];
    cx.layout_document(document, nana_ui_runtime::LayoutViewport::new(320.0, 600.0))
        .unwrap();
    assert_eq!(cx.world().layout_box(scroll).unwrap().height, 200.0);
}

#[test]
fn a_virtual_grid_in_a_template_scrolls_with_its_page() {
    use nana_ui_runtime::view::node_ref;
    use nana_ui_runtime::{ScrollAxes, ScrollView};
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let mounted = cx
        .mount_view(parent.stable_id(), || {
            let rows: Signal<Vec<u32>> = signal((0..10_000).collect());
            let page = node_ref();
            let scroll = ScrollView::new(ScrollAxes::Vertical)
                .with_layout(|l| l.height = Some(nana_ui_runtime::LengthSpec::Px(200.0)));
            view! {
                <Widget of={scroll} ref={page}>
                    <Virtual row_height=40 within={page} grid=100 gap=10 overscan=0 key="rows">
                        <Text v-for={n in rows} key={*n}>"格 {n}"</Text>
                    </Virtual>
                </Widget>
            }
        })
        .unwrap();
    let page = mounted.roots()[0];
    for _ in 0..4 {
        cx.layout_document(document, nana_ui_runtime::LayoutViewport::new(320.0, 600.0))
            .unwrap();
        cx.flush_reactive().unwrap();
    }
    let list = cx.resolve_assembly_path(page, "rows").unwrap();
    assert_eq!(cx.world().node(list).unwrap().parent, Some(page));
    let rows = cx.world().node(list).unwrap().children.len();
    // 50 px grid rows (40 and the gap) in the page's 200 px viewport.
    assert!((4..=5).contains(&rows), "{rows} grid rows built");
}

/// `list-ref` on `<Virtual>` hands the list's ref to the template's list.
#[test]
fn a_virtual_element_takes_a_list_ref() {
    use nana_ui_runtime::view::virtual_list_ref;
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let rows = virtual_list_ref::<u32>();
    let list_ref = rows.clone();
    cx.mount_view(parent.stable_id(), move || {
        let items: Signal<Vec<u32>> = signal((0..1_000).collect());
        view! {
            <Virtual row_height=20 height=100 list_ref={list_ref.clone()}>
                <Text v-for={n in items} key={*n}>"行 {n}"</Text>
            </Virtual>
        }
    })
    .unwrap();
    for _ in 0..3 {
        cx.layout_document(document, nana_ui_runtime::LayoutViewport::new(320.0, 600.0))
            .unwrap();
        cx.flush_reactive().unwrap();
    }
    assert_eq!(rows.item(&50).map(|item| item.offset), Some(1_000.0));
    assert!(rows.row(&0).is_some() && rows.row(&50).is_none());
}

/// `v-show` on `<Virtual>` shows and hides the list's scroll area.
#[test]
fn a_virtual_element_takes_v_show() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let shown = std::cell::Cell::new(None);
    let mounted = cx
        .mount_view(parent.stable_id(), || {
            let items: Signal<Vec<u32>> = signal((0..100).collect());
            let visible = signal(true);
            shown.set(Some(visible));
            view! {
                <Virtual row_height=20 height=100 v-show={visible}>
                    <Text v-for={n in items} key={*n}>"行 {n}"</Text>
                </Virtual>
            }
        })
        .unwrap();
    let visible = shown.get().unwrap();
    let list = Entity::<nana_ui_runtime::ScrollView>::from_stable_id(mounted.roots()[0]);
    let hidden = |cx: &AppContext| cx.read(list, |scroll| scroll.style.layout.hidden).unwrap();
    assert!(!hidden(&cx));
    visible.set(false);
    cx.flush_reactive().unwrap();
    assert!(hidden(&cx));
}

#[test]
fn media_controls_and_theme_roles_bind_from_a_template() {
    use nana_ui_runtime::view::{Signal, signal};
    use nana_ui_runtime::{EmptyState, Icon, IconButton, SemanticColorRole, Thumbnail};
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let state = std::cell::Cell::new(None);
    let mounted = cx
        .mount_view(parent.stable_id(), || {
            let cover: Signal<std::sync::Arc<str>> = signal("".into());
            let liked = signal(false);
            let likes = signal(0u32);
            state.set(Some((cover, liked, likes)));
            view! {
                <Column background={SemanticColorRole::Subtle} key="card">
                    <Thumbnail resource={cover} aspect=1.5 key="cover" />
                    <IconButton icon={Icon::Add} selected={liked} key="like"
                        @activate={likes.update(|n| *n += 1)}>"点赞"</IconButton>
                    <Text foreground={liked.get().then_some(SemanticColorRole::Accent)} key="count">
                        "{likes}"
                    </Text>
                    <EmptyState key="empty" message="稍后再试">
                        "加载失败"
                        <template #action><Button key="retry">"重试"</Button></template>
                    </EmptyState>
                </Column>
            }
        })
        .unwrap();
    let card = mounted.roots()[0];
    let at = |cx: &AppContext, path: &str| cx.resolve_assembly_path(card, path).unwrap();
    let (cover, liked, likes) = state.get().unwrap();

    cover.set("cover:1".into());
    liked.set(true);
    cx.flush_reactive().unwrap();
    let thumbnail = Entity::<Thumbnail>::from_stable_id(at(&cx, "cover"));
    assert_eq!(
        cx.read(thumbnail, |t| (t.resource.clone(), t.aspect))
            .unwrap(),
        ("cover:1".into(), 1.5)
    );
    let like = Entity::<IconButton>::from_stable_id(at(&cx, "like"));
    assert!(cx.read(like, |b| b.selected).unwrap());
    assert_eq!(
        cx.read(Entity::<Text>::from_stable_id(at(&cx, "count")), |t| t
            .style
            .foreground)
            .unwrap(),
        Some(SemanticColorRole::Accent)
    );
    assert_eq!(
        cx.read(Entity::<Stack>::from_stable_id(card), |s| {
            nana_ui_runtime::view::StyledComponent::node_style(s).background
        })
        .unwrap(),
        Some(SemanticColorRole::Subtle)
    );

    cx.activate_node(like.stable_id()).unwrap();
    cx.flush_reactive().unwrap();
    assert_eq!(likes.get_untracked(), 1);

    let empty = Entity::<EmptyState>::from_stable_id(at(&cx, "empty"));
    let action = cx
        .read(empty, |e| e.action)
        .unwrap()
        .expect("the action slot");
    assert_eq!(
        cx.world().node(action).unwrap().parent,
        Some(empty.stable_id())
    );
    assert_eq!(
        cx.read(empty, |e| (e.title.clone(), e.message.clone()))
            .unwrap(),
        ("加载失败".into(), Some("稍后再试".into()))
    );
}

#[test]
fn rows_dialogs_and_the_transport_bar_place_their_slots() {
    use nana_ui_runtime::{Dialog, ListItem, MediaTransportBar};
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let parent = cx.create_component(document, Stack::column(0.0)).unwrap();
    let mounted = cx
        .mount_view(parent.stable_id(), || {
            view! {
                <Column>
                    <ListItem label="视频" key="row">
                        <template #leading><Text key="cover">"封面"</Text></template>
                        <template #content><Text key="title">"标题"</Text></template>
                        <template #trailing><Button key="more">"更多"</Button></template>
                    </ListItem>
                    <Widget of={Dialog::new("投币")} key="dialog">
                        <template #body><Text key="body">"投几枚？"</Text></template>
                        <template #footer><Button key="confirm">"确认"</Button></template>
                    </Widget>
                    <Widget of={MediaTransportBar::new()} key="bar">
                        <template #leading><Button key="next">"下一P"</Button></template>
                        <template #secondary><Text key="danmaku">"弹幕"</Text></template>
                        <template #settings>
                            <Column key="menu">
                                <Button key="theatre">"剧场"</Button>
                                <Button key="stop">"停止播放"</Button>
                            </Column>
                        </template>
                    </Widget>
                </Column>
            }
        })
        .unwrap();
    cx.flush_reactive().unwrap();
    let root = mounted.roots()[0];
    let world = |cx: &AppContext, id| cx.world().node(id).unwrap().clone();
    let named = |cx: &AppContext, path: &str| cx.resolve_assembly_path(root, path).unwrap();

    let row = Entity::<ListItem>::from_stable_id(named(&cx, "row"));
    let children = world(&cx, row.stable_id()).children;
    assert_eq!(children.len(), 3, "leading, content and trailing, in order");
    assert_eq!(cx.world().text(children[1]), Some("标题"));

    let dialog = named(&cx, "dialog");
    let placed = world(&cx, dialog).children;
    assert_eq!(placed.len(), 2, "body and footer");
    assert_eq!(cx.world().text(placed[0]), Some("投几枚？"));

    let bar = Entity::<MediaTransportBar>::from_stable_id(named(&cx, "bar"));
    let (leading, secondary) = cx
        .read(bar, |bar| {
            (bar.leading().unwrap(), bar.secondary().unwrap())
        })
        .unwrap();
    let next = world(&cx, leading.stable_id()).children;
    assert_eq!(next.len(), 1, "the application's control sits after play");
    let row = world(&cx, secondary.stable_id()).children;
    assert!(
        row.iter().any(|id| cx.world().text(*id) == Some("弹幕")),
        "the second row holds the application's content"
    );
    let settings = cx.read(bar, |bar| bar.settings().unwrap()).unwrap();
    let items = world(&cx, settings.stable_id()).children;
    assert_eq!(
        items.len(),
        1,
        "the settings menu holds the application's items"
    );
    assert_eq!(world(&cx, items[0]).children.len(), 2);
}

mod store_derive {
    use nana_ui_runtime::view::{Store, StoreList, StorePath, reactive_stats, store, text};
    use nana_ui_runtime::{AppContext, DocumentId, Entity, Stack, Text};

    #[derive(Clone, Store)]
    struct Todo {
        id: u64,
        title: String,
    }

    #[derive(Store)]
    struct App {
        todos: Vec<Todo>,
        heading: String,
    }

    #[test]
    fn derived_accessors_track_each_field() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let parent = cx
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let app = std::cell::Cell::new(None);
        let view = cx
            .mount_view(parent, || {
                let state = store(App {
                    todos: vec![
                        Todo {
                            id: 1,
                            title: "一".into(),
                        },
                        Todo {
                            id: 2,
                            title: "二".into(),
                        },
                    ],
                    heading: "标题".into(),
                });
                app.set(Some(state));
                state
                    .todos()
                    .keyed(|todo| todo.id)
                    .each(|todo| text(todo.title()))
            })
            .unwrap();
        let app = app.get().unwrap();
        let list = view.roots()[0];

        let before = reactive_stats();
        app.heading().set("新标题".into());
        app.todos()
            .keyed(|todo| todo.id)
            .at(&2)
            .title()
            .set("贰".into());
        cx.flush_reactive().unwrap();
        let after = reactive_stats();
        assert_eq!(after.effects_run - before.effects_run, 1);
        let rows = cx.world().node(list).unwrap().children.to_vec();
        let second: Entity<Text> = Entity::from_stable_id(rows[1]);
        assert_eq!(cx.read(second, |text| text.value.clone()).unwrap(), "贰");
        assert_eq!(app.heading().get_untracked(), "新标题");
        assert_eq!(app.todos().len(), 2);
    }
}

mod transition_block {
    use std::time::Duration;

    use nana_ui_runtime::view::signal;
    use nana_ui_runtime::{AppContext, DocumentId, Stack, view};

    #[test]
    fn a_transition_element_is_the_function_api_with_a_transition() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let parent = cx
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        cx.advance_animations(Duration::from_secs(1));
        let flag = std::cell::Cell::new(None);
        let view = cx
            .mount_view(parent, || {
                let open = signal(true);
                let items = signal(vec![1u32, 2]);
                flag.set(Some(open));
                view! {
                    <Column>
                        <Transition name="fade" duration=200>
                            <Text v-if={open}>"开"</Text>
                            <Text v-else>"关"</Text>
                        </Transition>
                        <TransitionGroup move=200>
                            <Text v-for={n in items} key={*n}>{n.to_string()}</Text>
                        </TransitionGroup>
                    </Column>
                }
            })
            .unwrap();
        let column = view.roots()[0];
        let block = cx.world().node(column).unwrap().children[0];
        flag.get().unwrap().set(false);
        cx.flush_reactive().unwrap();
        assert_eq!(
            cx.world().node(block).unwrap().children.len(),
            2,
            "the old branch leaves while the new one enters"
        );
        cx.advance_animations(Duration::from_secs(2));
        assert_eq!(cx.world().node(block).unwrap().children.len(), 1);
    }
}

mod suspense_block {
    use nana_ui_runtime::view::{poll_tasks, resource, text};
    use nana_ui_runtime::{AppContext, DocumentId, Stack, view};

    #[test]
    fn a_suspense_element_shows_content_once_it_resolves() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let parent = cx
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let view = cx
            .mount_view(parent, || {
                view! {
                    <Suspense fallback={text("加载中")}>
                        {{
                            let data = resource(|| (), |()| std::future::ready(7u32));
                            text(move || format!("{:?}", data.get()))
                        }}
                    </Suspense>
                }
            })
            .unwrap();
        poll_tasks();
        cx.take_system_work();
        let root = view.roots()[0];
        let children = cx.world().node(root).unwrap().children.to_vec();
        let fallback = cx.world().node(children[1]).unwrap().children.len();
        assert_eq!(fallback, 0, "resolved on the first poll");
    }
}

mod styles {
    use nana_ui_runtime::view::{IntoView, column, signal, text, widget};
    use nana_ui_runtime::{AppContext, DocumentId, LengthSpec, Stack, css, stylesheet, view};

    stylesheet! {
        mod panel_styles;
        .panel { padding: 6px; transition: opacity 150ms; }
        .panel.open { opacity: 0.5; }
        .title { flex-grow: 1; font-size: "1.5em"; }
    }

    /// The same sheet as a template's `<style>` and as `stylesheet!`, the
    /// same classes as attributes and as calls: the same layouts, before
    /// and after the conditional class turns on.
    #[test]
    fn a_stylesheet_styles_rust_elements_as_style_does_a_template() {
        let document = DocumentId::new(1).unwrap();
        let mount = |template: bool| {
            let mut cx = AppContext::new();
            let flag = std::cell::Cell::new(None);
            let view = cx
                .mount_view_root(document, || {
                    let open = signal(false);
                    flag.set(Some(open));
                    if template {
                        view! {
                            <style>
                                .panel { padding: 6px; transition: opacity 150ms; }
                                .panel.open { opacity: 0.5; }
                                .title { flex-grow: 1; font-size: "1.5em"; }
                            </style>
                            <Column class="panel" class:open={open}>
                                <Text class="title">"x"</Text>
                            </Column>
                        }
                        .into_any()
                    } else {
                        column()
                            .class(panel_styles::panel)
                            .class_when(panel_styles::open, open)
                            .children(text("x").class(panel_styles::title))
                            .into_any()
                    }
                })
                .unwrap();
            (cx, view, flag.get().unwrap())
        };
        let layouts = |cx: &AppContext, root| {
            let child = cx.world().node(root).unwrap().children[0];
            (layout_of(cx, root), layout_of(cx, child))
        };
        let (mut template, t, t_open) = mount(true);
        let (mut rust, r, r_open) = mount(false);
        let before = layouts(&template, t.roots()[0]);
        assert_eq!(before, layouts(&rust, r.roots()[0]));
        assert_eq!(before.0.padding, Some(LengthSpec::Px(6.0)));
        assert_eq!(before.1.flex_grow, Some(1.0));
        t_open.set(true);
        r_open.set(true);
        template.flush_reactive().unwrap();
        rust.flush_reactive().unwrap();
        let after = layouts(&template, t.roots()[0]);
        assert_eq!(after, layouts(&rust, r.roots()[0]));
        assert_eq!(after.0.opacity, Some(0.5));
    }

    fn layout_of(cx: &AppContext, id: nana_ui_runtime::StableNodeId) -> nana_ui_core::LayoutStyle {
        (*cx.world().node_style(id).unwrap().layout).clone()
    }

    #[test]
    fn a_template_style_and_css_compile_at_build_time() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let parent = cx
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let flag = std::cell::Cell::new(None);
        let view = cx
            .mount_view(parent, || {
                let open = signal(false);
                flag.set(Some(open));
                view! {
                    <style>
                        .panel { padding: 6px; }
                        .panel.open { opacity: 0.5; }
                    </style>
                    <Column class="panel" class:open={open}>
                        {widget(Stack::row(0.0)).css(css! { margin: 3px; opacity: 0.25 }).into_any()}
                        <Text>"x"</Text>
                    </Column>
                }
            })
            .unwrap();
        let panel = view.roots()[0];
        assert_eq!(layout_of(&cx, panel).padding, Some(LengthSpec::Px(6.0)));
        assert_eq!(layout_of(&cx, panel).opacity, None);
        flag.get().unwrap().set(true);
        cx.flush_reactive().unwrap();
        assert_eq!(layout_of(&cx, panel).opacity, Some(0.5));
        let inline = cx.world().node(panel).unwrap().children[0];
        assert_eq!(layout_of(&cx, inline).margin, Some(LengthSpec::Px(3.0)));
        assert_eq!(layout_of(&cx, inline).opacity, Some(0.25));
        let _ = text("");
    }

    stylesheet! {
        mod reset_styles;
        .reset { align-items: flex-start; height: auto; width: 50%; }
    }

    /// A declaration writing the value the default layout has still
    /// writes it: a row is built centered and a column hugs its content,
    /// and a class resets both. A length of another kind replaces the
    /// built one whole.
    #[test]
    fn a_class_writes_a_default_value_over_what_the_element_was_built_with() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let view = cx
            .mount_view_root(document, || {
                (
                    widget(Stack::row(0.0).width(LengthSpec::Px(100.0))).class(reset_styles::reset),
                    column().class(reset_styles::reset),
                )
            })
            .unwrap();
        let (row, column) = (view.roots()[0], view.roots()[1]);
        let row = layout_of(&cx, row);
        assert_eq!(row.align_items, nana_ui_core::AlignSpec::Start);
        assert_eq!(row.width, Some(LengthSpec::Percent(50.0)));
        let column = layout_of(&cx, column);
        assert_eq!(column.align_items, nana_ui_core::AlignSpec::Start);
        assert_eq!(column.height, Some(LengthSpec::Auto));
    }

    stylesheet! {
        mod block_styles;
        .strip { flex-direction: row; gap: 4px; width: auto; }
        .quiet { opacity: 0.5; }
    }

    /// `class` on `<Block>` (and on the other blocks) styles the container
    /// the list or the chain is built in, as `.class` on `each` / `when` /
    /// `each_virtual` does; the rows and branches keep theirs.
    #[test]
    fn a_block_class_styles_the_container_of_a_list_or_a_chain() {
        use nana_ui_runtime::view::{each, each_virtual, when};
        let document = DocumentId::new(1).unwrap();
        let mount = |template: bool| {
            let mut cx = AppContext::new();
            let flag = std::cell::Cell::new(None);
            let view = cx
                .mount_view_root(document, || {
                    let items = signal(vec![1u32, 2, 3]);
                    let open = signal(true);
                    flag.set(Some(open));
                    if template {
                        view! {
                            <style>
                                .strip { flex-direction: row; gap: 4px; width: auto; }
                                .quiet { opacity: 0.5; }
                            </style>
                            <Block class="strip">
                                <Text v-for={n in items} key={*n} class="quiet">{n.to_string()}</Text>
                            </Block>
                            <Block class="strip" class:quiet={open}>
                                <Text v-if={open}>"开"</Text>
                                <Text v-else>"关"</Text>
                            </Block>
                            <Virtual row_height=20 height=100 class="quiet">
                                <Text v-for={n in items} key={*n}>{n.to_string()}</Text>
                            </Virtual>
                        }
                        .into_any()
                    } else {
                        (
                            each(items, |n| *n, |n| text(n.to_string()).class(block_styles::quiet))
                                .class(block_styles::strip),
                            when(open, || text("开"))
                                .otherwise(|| text("关"))
                                .class(block_styles::strip)
                                .class_when(block_styles::quiet, open),
                            each_virtual(items, |n| *n, 20.0, |n| text(n.to_string()))
                                .height(100.0)
                                .class(block_styles::quiet),
                        )
                            .into_any()
                    }
                })
                .unwrap();
            (cx, view, flag.get().unwrap())
        };
        let (mut template, t, t_open) = mount(true);
        let (mut rust, r, r_open) = mount(false);
        let layouts = |cx: &AppContext, roots: &[nana_ui_runtime::StableNodeId]| {
            roots
                .iter()
                .map(|root| {
                    let rows = cx.world().node(*root).unwrap().children.to_vec();
                    (
                        layout_of(cx, *root),
                        rows.iter()
                            .map(|row| layout_of(cx, *row))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>()
        };
        let before = layouts(&template, t.roots());
        assert_eq!(before, layouts(&rust, r.roots()));
        let (list, rows) = &before[0];
        assert_eq!(list.direction, Some(nana_ui_core::FlexDirection::Row));
        assert_eq!(list.gap, Some(LengthSpec::Px(4.0)));
        assert_eq!(list.width, Some(LengthSpec::Auto));
        assert!(rows.iter().all(|row| row.opacity == Some(0.5)), "{rows:?}");
        assert_eq!(before[1].0.opacity, Some(0.5));
        assert_eq!(
            before[1].1[0].opacity, None,
            "the branch keeps its own style"
        );
        assert_eq!(
            before[2].0.opacity,
            Some(0.5),
            "the virtual list's scroll area"
        );
        assert_eq!(before[2].0.height, Some(LengthSpec::Px(100.0)));
        t_open.set(false);
        r_open.set(false);
        template.flush_reactive().unwrap();
        rust.flush_reactive().unwrap();
        let after = layouts(&template, t.roots());
        assert_eq!(after, layouts(&rust, r.roots()));
        assert_eq!(after[1].0.opacity, None);
        assert_eq!(after[1].0.direction, Some(nana_ui_core::FlexDirection::Row));
    }

    stylesheet! {
        mod teleport_styles;
        .fill { height: 100%; flex-grow: 1; }
    }

    /// `class` on `<Teleport>` and `.class` / `.css` on `teleport` style the
    /// anchor its content is built in, which it stays in without a target.
    #[test]
    fn a_teleport_class_styles_its_anchor() {
        use nana_ui_runtime::view::{node_ref, teleport};
        let document = DocumentId::new(1).unwrap();
        let mount = |spelling: u8| {
            let mut cx = AppContext::new();
            let view = cx
                .mount_view_root(document, || {
                    let nowhere = node_ref();
                    match spelling {
                        0 => view! {
                            <style>.fill { height: 100%; flex-grow: 1; }</style>
                            <Teleport to={nowhere} class="fill"><Text>"浮层"</Text></Teleport>
                        }
                        .into_any(),
                        1 => teleport(nowhere, text("浮层"))
                            .class(teleport_styles::fill)
                            .into_any(),
                        _ => teleport(nowhere, text("浮层"))
                            .css(css! { height: 100%; flex-grow: 1; })
                            .into_any(),
                    }
                })
                .unwrap();
            (cx, view)
        };
        let anchors = (0..3)
            .map(|spelling| {
                let (cx, view) = mount(spelling);
                let anchor = view.roots()[0];
                assert_eq!(cx.world().node(anchor).unwrap().children.len(), 1);
                layout_of(&cx, anchor)
            })
            .collect::<Vec<_>>();
        assert_eq!(anchors[0], anchors[1]);
        assert_eq!(anchors[0].height, Some(LengthSpec::Fill));
        assert_eq!(anchors[0].flex_grow, Some(1.0));
        assert_eq!(anchors[2].height, Some(LengthSpec::Fill));
        assert_eq!(anchors[2].flex_grow, Some(1.0));
    }

    /// The space in `.panel .open` is a descendant combinator, which L3
    /// views do not compile; without it `.panel.open` is a compound that
    /// applies. Quoted values are spliced bare.
    #[test]
    #[allow(deprecated)] // the descendant rule is a warning
    fn style_tokens_keep_the_space_between_selectors() {
        let mut cx = AppContext::new();
        let document = DocumentId::new(1).unwrap();
        let parent = cx
            .create_component(document, Stack::column(0.0))
            .unwrap()
            .stable_id();
        let flag = std::cell::Cell::new(None);
        let view = cx
            .mount_view(parent, || {
                let open = signal(false);
                flag.set(Some(open));
                view! {
                    <style>
                        .panel .open { opacity: 0.5; }
                        .panel.open { padding: "2px"; }
                    </style>
                    <Column class="panel" class:open={open}>
                        <Text>"x"</Text>
                    </Column>
                }
            })
            .unwrap();
        let panel = view.roots()[0];
        flag.get().unwrap().set(true);
        cx.flush_reactive().unwrap();
        assert_eq!(layout_of(&cx, panel).padding, Some(LengthSpec::Px(2.0)));
        assert_eq!(layout_of(&cx, panel).opacity, None);
    }
}

/// Each block written as a template and in Rust mounts the same tree and
/// changes it the same way.
mod blocks_mirror {
    use std::sync::Arc;
    use std::time::Duration;

    use nana_ui_runtime::view::{
        AnyView, EachExt, IntoView, NodeRef, Signal, Transition, WhenExt, column, each_virtual,
        error_boundary, node_ref, signal, suspense, teleport, text, widget,
    };
    use nana_ui_runtime::{AppContext, DocumentId, Stack, view};

    use super::dump;

    /// `make` creates the signals inside the mount; `poke` changes them.
    fn mirror<S: Copy + 'static>(
        make: impl Fn() -> S,
        template: impl Fn(S) -> AnyView,
        rust: impl Fn(S) -> AnyView,
        poke: impl Fn(S),
    ) {
        let run = |form: &dyn Fn(S) -> AnyView| {
            let mut cx = AppContext::new();
            cx.advance_animations(Duration::from_secs(1));
            let document = DocumentId::new(1).unwrap();
            let state = std::cell::Cell::new(None);
            let view = cx
                .mount_view_root(document, || {
                    let s = make();
                    state.set(Some(s));
                    form(s)
                })
                .unwrap();
            let root = view.roots()[0];
            let before = dump(&cx, root);
            poke(state.get().unwrap());
            cx.flush_reactive().unwrap();
            (before, dump(&cx, root))
        };
        let (template_before, template_after) = run(&template);
        let (rust_before, rust_after) = run(&rust);
        assert_eq!(template_before, rust_before);
        assert_eq!(template_after, rust_after);
    }

    #[test]
    fn transitions() {
        mirror(
            || (signal(true), signal(vec![1u32, 2])),
            |(open, items): (Signal<bool>, Signal<Vec<u32>>)| {
                view! {
                    <Column>
                        <Transition name="slide-up" duration=200>
                            <Text v-if={open}>"开"</Text>
                            <Text v-else>"关"</Text>
                        </Transition>
                        <TransitionGroup move=200>
                            <Text v-for={n in items} key={*n}>{n.to_string()}</Text>
                        </TransitionGroup>
                    </Column>
                }
                .into_any()
            },
            |(open, items)| {
                let ms = Duration::from_millis;
                column()
                    .children((
                        open.then_show(|| text("开"))
                            .otherwise(|| text("关"))
                            .transition(Transition::slide(0.0, 12.0, ms(200))),
                        items
                            .each(|n| *n, |n| text(n.to_string()))
                            .transition(Transition::fade(ms(150)).moves(ms(200))),
                    ))
                    .into_any()
            },
            |(open, items)| {
                open.set(false);
                items.update(|list| list.insert(0, 3));
            },
        );
    }

    #[test]
    fn keep_alive() {
        mirror(
            || signal(true),
            |a: Signal<bool>| {
                view! {
                    <Column>
                        <KeepAlive>
                            <Text v-if={a}>"甲"</Text>
                            <Text v-else>"乙"</Text>
                        </KeepAlive>
                    </Column>
                }
                .into_any()
            },
            |a| {
                column()
                    .children(
                        a.then_show(|| text("甲"))
                            .otherwise(|| text("乙"))
                            .keep_alive(),
                    )
                    .into_any()
            },
            |a| a.set(false),
        );
    }

    #[test]
    fn suspense_with_a_fallback_slot() {
        mirror(
            || signal(0u32),
            |n: Signal<u32>| {
                view! {
                    <Column>
                        <Suspense>
                            <template #fallback><Text>"加载中"</Text></template>
                            <Text>"第 {n} 次"</Text>
                        </Suspense>
                    </Column>
                }
                .into_any()
            },
            |n| {
                column()
                    .children(suspense(
                        || text("加载中"),
                        move || nana_ui_runtime::text!("第 {n} 次"),
                    ))
                    .into_any()
            },
            |n| n.set(1),
        );
    }

    #[test]
    fn teleport_to_a_node_of_the_view() {
        mirror(
            || (node_ref(), signal(0u32)),
            |(target, n): (NodeRef, Signal<u32>)| {
                view! {
                    <Column>
                        {widget(Stack::column(0.0)).node_ref(target)}
                        <Teleport to={target}><Text>"浮层 {n}"</Text></Teleport>
                    </Column>
                }
                .into_any()
            },
            |(target, n)| {
                column()
                    .children((
                        widget(Stack::column(0.0)).node_ref(target),
                        teleport(target, nana_ui_runtime::text!("浮层 {n}")),
                    ))
                    .into_any()
            },
            |(_, n)| n.set(1),
        );
    }

    #[test]
    fn error_boundary_and_its_fallback() {
        mirror(
            || signal(0u32),
            |n: Signal<u32>| {
                view! {
                    <Column>
                        <ErrorBoundary fallback={|errors: Vec<Arc<str>>| text(errors.len().to_string())}>
                            <Text>"正常 {n}"</Text>
                        </ErrorBoundary>
                    </Column>
                }
                .into_any()
            },
            |n| {
                column()
                    .children(error_boundary(
                        |errors: Vec<Arc<str>>| text(errors.len().to_string()),
                        move || nana_ui_runtime::text!("正常 {n}"),
                    ))
                    .into_any()
            },
            |n| n.set(1),
        );
    }

    #[test]
    fn virtual_rows() {
        mirror(
            || signal((0..50u32).collect::<Vec<_>>()),
            |rows: Signal<Vec<u32>>| {
                view! {
                    <Column>
                        <Virtual row_height=20 height=200 measured>
                            <Text v-for={n in rows} key={*n}>"行 {n}"</Text>
                        </Virtual>
                    </Column>
                }
                .into_any()
            },
            |rows| {
                column()
                    .children(
                        each_virtual(rows, |n| *n, 20.0, |n| nana_ui_runtime::text!("行 {n}"))
                            .measured()
                            .height(200.0),
                    )
                    .into_any()
            },
            |rows| rows.update(|list| list.insert(0, 99)),
        );
    }
}

/// `on:Event={|component, event, cx| …}` is `.on_cx`: the handler gets the
/// component and its context, as in Rust.
#[test]
fn a_three_argument_handler_gets_the_component_and_its_context() {
    use nana_ui_runtime::{Activate, Button};
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let view = cx
        .mount_view_root(document, || {
            view! {
                <Button on:Activate={|button: &mut Button, _: &Activate, cx| {
                    button.label = "已按".into();
                    cx.dispatch_program(7u32);
                }}>"按"</Button>
            }
        })
        .unwrap();
    let button = view.root::<Button>().unwrap();
    cx.activate_button(button).unwrap();
    assert_eq!(cx.read(button, |b| b.label.clone()).unwrap(), "已按");
    let messages = cx.take_program_messages();
    assert_eq!(messages[0].downcast_ref::<u32>(), Some(&7));
}

/// An application's own `EmptyState`, named like the built-in one.
mod kit {
    use nana_ui_runtime::view::{IntoView, column, text};

    pub fn empty_state(title: &'static str) -> impl IntoView {
        column().children(text(title))
    }
}

#[test]
fn a_tag_with_a_path_calls_its_function_even_when_the_name_is_built_in() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let mounted = cx
        .mount_view_root(document, || {
            view! {
                <Column>
                    <kit::EmptyState title={"自己的"}></kit::EmptyState>
                    <EmptyState title="内置" />
                </Column>
            }
        })
        .unwrap();
    let tree = dump(&cx, mounted.roots()[0]);
    let mut lines = tree.lines().skip(1);
    // The path picks the application's function: a column around a text.
    assert!(lines.next().unwrap().contains("stack"), "{tree}");
    assert!(lines.next().unwrap().contains("\"自己的\""), "{tree}");
    // The bare tag is the built-in control.
    assert!(lines.next().unwrap().contains("empty-state"), "{tree}");
}
