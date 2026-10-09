//! 播放集页（常驻）：投影、信号和只建一次的视图，对齐 Vue `WorkspacePlaylistPage`。
//!
//! [`PlaylistPageView`] 是投影；[`PlaylistPageSignals`] 里页眉一个信号，条目放进按编号对照的 Store，
//! 另有条目顺序。页眉的标题、状态行、「播放」的禁用和面板圆角按字段绑定；条目行的文件名、路径或原因、
//! 能不能播放原地改；没点开时的虚线空框和面板用 `.visible` 互换。
//!
//! 信号放在播放集路由的常驻信号里（`route_playlists.rs`），主区块同步时 [`PlaylistPageSignals::write`]；
//! 播放条是那边登记的旧视图岛，这里只收它的占位节点。
//!
//! 条目列表放在 `ReorderList` 里：它按自己的直接子节点取行的盒子来拖动排序，`each` 总会多一层容器，
//! 所以条目没法用带键的 `each`（NanaUI 缺口）。列表改用按条目顺序做键的 `dynamic`：增删、重排时整个
//! 列表重建，当前播放这类字段原地改。换列表要分两步：顺序变了先在挂载回调里记下旧列表里的焦点（那时
//! 旧列表还在），再把顺序写进 `dynamic` 的键；新列表挂上后按键路径找回焦点。行和控件的键都带条目编号，
//! 条目挪了位置也找得到原来那个按钮。

use std::sync::Arc;

use nana_ui::runtime::view::{
    dynamic, fields, node_ref, on_mount, signal, store, watch_effect, widget, AnyView, FieldWrite, IntoView, Item, NodeRef,
    Signal, Store, StoreList, StorePath, StyledComponent,
};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, JustifySpec, LengthSpec, RadiusTier, ReorderItem, ReorderList, ReorderListEvent,
    SemanticColorRole, SemanticPaint, Stack, Text, TextHorizontalAlignment,
};
use nana_ui::{ButtonKind, Icon};

use crate::backend::services::repository::PlaylistItem;

use super::super::player::PlayerMessage;
use super::super::remount_state::{self, KeptState};
use super::super::view_part_sidebar::project::sync_rows;
use super::super::{ShellMessage, ShellViewModel};
use super::{bar, icons, key_part, playlist_plugin_missing, player_message};

/// 不能播放的条目整行的透明度。
const UNREADY_OPACITY: f32 = 0.72;

/// 播放集页要显示的东西。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaylistPageView {
    pub head: PageHead,
    pub items: Vec<PlaylistEntry>,
}

/// 页眉和面板：没点开时只有空框。
#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) struct PageHead {
    /// 已点开、详情已读回。
    pub listed: bool,
    pub name: String,
    /// 「播放器 · N 项」，没有对应播放插件时再加「 · 缺少对应播放插件」。
    pub subline: String,
    /// 有对应的播放插件：页眉和行上的播放才可点。
    pub has_player: bool,
    /// 面板圆角，跟外观设置里的圆角。
    pub radius: f32,
    /// 条目所属的播放集，移除条目时带上。
    pub playlist_id: String,
    /// 正在播放的条目。
    pub current: Option<String>,
}

/// 一个条目。编号是身份，其余字段原地改。字段不叫 `id`：`Item` 自己有同名方法。
#[derive(Clone, Debug, PartialEq, nana_ui::runtime::view::Store)]
pub(crate) struct PlaylistEntry {
    pub item_id: String,
    pub filename: String,
    pub path: String,
    /// 扩展名方块里的字：大写，没有扩展名时是一个空格。
    pub mark: String,
    /// 可以播放。不能播放的条目整行变淡，次行写原因。
    pub ready: bool,
    pub reason: String,
}

impl From<&PlaylistItem> for PlaylistEntry {
    fn from(item: &PlaylistItem) -> Self {
        let mark = item.extension.trim().to_ascii_uppercase();
        Self {
            item_id: item.playlist_item_id.clone(),
            filename: item.filename.clone(),
            path: item.path.clone(),
            mark: if mark.is_empty() { " ".into() } else { mark },
            ready: item.status == "ready",
            reason: item.status_reason.clone().unwrap_or_else(|| item.status.clone()),
        }
    }
}

impl PlaylistPageView {
    /// 从 ViewModel 取播放集页的投影。详情还没读回时只有空框。
    pub(crate) fn project(model: &ShellViewModel) -> Self {
        let Some(detail) = model.player.listed.as_ref() else {
            return Self { head: PageHead::default(), items: Vec::new() };
        };
        let has_player = !playlist_plugin_missing(model, &detail.playlist.player_type_id);
        let mut subline = format!("{} · {} 项", detail.playlist.player_label, detail.items.len());
        if !has_player {
            subline.push_str(" · 缺少对应播放插件");
        }
        Self {
            head: PageHead {
                listed: true,
                name: detail.playlist.name.clone(),
                subline,
                has_player,
                radius: crate::theme_map::radius_2xl(model.admin.corner_radius as f32),
                playlist_id: model.selected_playlist_id.clone().unwrap_or_else(|| detail.playlist.playlist_id.clone()),
                current: model.player.current_id.clone(),
            },
            items: detail.items.iter().map(PlaylistEntry::from).collect(),
        }
    }
}

/// 条目的键：编号。
fn entry_key(entry: &PlaylistEntry) -> String {
    entry.item_id.clone()
}

/// Store 里的一个条目。
type EntryItem = Item<Store<Vec<PlaylistEntry>>, String, PlaylistEntry>;

/// 播放集页的信号。句柄是 `Copy` 的 id，值在建它的作用域里。
#[derive(Clone, Copy)]
pub(crate) struct PlaylistPageSignals {
    head: Signal<PageHead>,
    entries: Store<Vec<PlaylistEntry>>,
    /// 投影里的条目顺序。
    order: Signal<Vec<String>>,
    /// 列表现在按哪个顺序建：顺序变了以后，记下焦点才跟上。
    shown: Signal<Vec<String>>,
    /// 现在的条目列表节点。
    list: NodeRef,
    /// 换列表之前记下的焦点和滚动，新列表挂上后取走。
    kept: Signal<Option<KeptState>>,
}

impl PlaylistPageSignals {
    /// 在当前作用域里建信号，初值是 `view`。
    pub(crate) fn new(view: PlaylistPageView) -> Self {
        let order = view.items.iter().map(entry_key).collect::<Vec<_>>();
        Self {
            head: signal(view.head),
            entries: store(view.items),
            order: signal(order.clone()),
            shown: signal(order),
            list: node_ref(),
            kept: signal(None),
        }
    }

    /// 写入投影：页眉值没变不写，条目按编号对照着改，顺序变了写进顺序。作用域已回收时只记日志。
    pub(crate) fn write(&self, view: PlaylistPageView) {
        if self.head.defined_at().is_none() {
            eprintln!("Nana 播放集页的信号已随作用域回收，跳过写入");
            return;
        }
        self.head.try_set_if_changed(view.head);
        let order = view.items.iter().map(entry_key).collect::<Vec<_>>();
        sync_rows(self.entries, entry_key, view.items);
        self.order.try_set_if_changed(order);
    }
}

/// 播放集页：没点开或详情还没读回时是虚线空框；点开后是 bg-elev 面板，里面是页眉、条目（或空框）和
/// `player_bar`。两块都留着，按是否点开互换；`surface` 为假（不在播放集面板）时两块都藏起来。
pub(crate) fn view(signals: PlaylistPageSignals, surface: Signal<bool>, player_bar: AnyView) -> AnyView {
    follow_order(signals);
    let head = signals.head;
    let shown = signals.shown;
    let initial = head.get_untracked();
    let unlisted = dashed_empty("选择一个播放集", "在左侧播放集区选择要查看或播放的列表。", "playlist-page-empty")
        .visible(move || surface.get() && !head.with(|head| head.listed));
    let no_items = dashed_empty(
        "播放集还是空的",
        "在文件浏览区右键文件，使用“加入播放列表”把内容加入这里。",
        "playlist-page-no-items",
    )
    .visible(move || shown.with(Vec::is_empty));
    let items = dynamic(shown, move |ids: &Vec<String>| item_list(ids, signals)).visible(move || !shown.with(Vec::is_empty));
    let panel = widget(Stack::column(16.0).padding(18.0).surface(SemanticColorRole::Surface).radius_px(initial.radius))
        .prop::<f32, PanelRadius>(move || head.with(|head| head.radius))
        .visible(move || surface.get() && head.with(|head| head.listed))
        .children((header(head), no_items, items, player_bar))
        .key("playlist-page");
    (unlisted, panel).into_any()
}

/// 条目顺序变了：先在挂载回调里记下旧列表里的焦点和滚动（那时旧列表还在），再把顺序写进列表的键。
/// 回调排在这一轮结构更新之后，写进键的顺序在下一轮换出新列表，新列表的挂载回调再把状态找回来。
fn follow_order(signals: PlaylistPageSignals) {
    let PlaylistPageSignals { order, shown, list, kept, .. } = signals;
    watch_effect(move || {
        let next = order.get();
        on_mount(move |cx| {
            if shown.with_untracked(|shown| *shown == next) {
                return;
            }
            if let Some(root) = list.get_untracked()
                && let Some(document) = cx.world().node(root).map(|node| node.document)
            {
                kept.set(Some(remount_state::capture(cx, document, &[root])));
            }
            shown.set(next);
        });
    });
}

/// 条目列表。整行可拖动排序，排序结果交回壳层持久化；当前播放的条目和文件名按字段绑在列表的条目上。
fn item_list(ids: &[String], signals: PlaylistPageSignals) -> AnyView {
    let PlaylistPageSignals { head, entries, shown, list, kept, .. } = signals;
    let keyed = entries.keyed(entry_key);
    let reorder = move || {
        let current = head.with(|head| head.current.clone());
        shown.with(|ids| {
            ids.iter()
                .filter_map(|id| keyed.at(id).try_with(|entry| entry.filename.clone()).map(|name| (id, name)))
                .map(|(id, name)| ReorderItem::new(id.clone(), name).selected(current.as_deref() == Some(id.as_str())))
                .collect::<Vec<_>>()
        })
    };
    on_mount(move |cx| {
        let mut state = None;
        kept.try_update(|slot| state = slot.take());
        let (Some(state), Some(root)) = (state, list.get_untracked()) else {
            return;
        };
        match cx.world().node(root).map(|node| node.document) {
            Some(document) => state.restore(cx, document, &[root]),
            None => eprintln!("Nana 播放集条目列表不在文档里，换列表前的焦点不再找回"),
        }
    });
    let rows = ids.iter().map(|id| entry_row(keyed.at(id), head)).collect::<Vec<_>>();
    widget(ReorderList::new(reorder()).live_rows(true).spacing(10.0).label("播放集条目"))
        .prop::<Vec<ReorderItem>, fields::reorder_list::items>(reorder)
        .node_ref(list)
        .key("playlist-reorder")
        .on_cx(|_, event: &ReorderListEvent, cx| {
            if let ReorderListEvent::Reorder { source, before } = event {
                cx.dispatch_program_all(player_message(PlayerMessage::Reorder {
                    source: source.to_string(),
                    before: before.as_ref().map(|value| value.to_string()),
                }));
            }
        })
        .children(rows)
        .into_any()
}

/// 眉题、22px 标题、13px 状态行；右上角是播放整张列表。
fn header(head: Signal<PageHead>) -> AnyView {
    let initial = head.get_untracked();
    let mut eyebrow = Text::new("播放集").color(SemanticColorRole::Faint).font_size(11.0).font_weight(600).line_height(17.05);
    Arc::make_mut(&mut eyebrow.style.layout).letter_spacing = Some(0.4);
    let titles = widget(Stack::column(0.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
        widget(eyebrow).key("playlist-page-eyebrow"),
        widget(spaced(Text::new(initial.name).color(SemanticColorRole::Text).font_size(22.0).font_weight(700).line_height(34.1), 4.0))
            .prop::<String, fields::text::value>(move || head.with(|head| head.name.clone()))
            .key("playlist-page-title"),
        widget(spaced(Text::new(initial.subline).color(SemanticColorRole::Muted).font_size(13.0).line_height(20.15), 8.0))
            .prop::<String, fields::text::value>(move || head.with(|head| head.subline.clone()))
            .key("playlist-page-status"),
    ));
    let play = widget(toolbar_button("播放", icons::PLAY, ButtonKind::Ghost, !initial.has_player, SemanticColorRole::Surface))
        .prop::<ToolbarLook, ToolbarDisabled>(move || {
            ToolbarLook::new(ButtonKind::Ghost, SemanticColorRole::Surface, !head.with(|head| head.has_player))
        })
        .key("playlist-play")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(player_message(PlayerMessage::PlayListed { item_id: None })));
    widget(Stack::bar(16.0).align(AlignSpec::Start))
        .children((titles, widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).children((play,))))
        .key("playlist-page-header")
        .into_any()
}

/// 文字上方留出 `margin`，对应 Vue 标题和状态行的上外边距。
fn spaced(mut text: Text, margin: f32) -> Text {
    Arc::make_mut(&mut text.style.layout).margin_top = Some(LengthSpec::Px(margin));
    text
}

/// 一条：拖动柄、扩展名方块、文件名和路径、播放与移除。不可播放的条目整行变淡，次行写原因，
/// 两种次行都留着、按能不能播放互换。
fn entry_row(item: EntryItem, head: Signal<PageHead>) -> AnyView {
    let entry = item.get_untracked();
    let id = entry.item_id.clone();
    let ready = move || item.try_with(|entry| entry.ready).unwrap_or(false);
    let path = widget(meta_line(&entry.path, SemanticColorRole::Muted))
        .prop::<String, fields::text::value>(move || item.try_with(|entry| entry.path.clone()).unwrap_or_default())
        .visible(ready)
        .key(format!("playlist-item-path-{}", key_part(&id)));
    let reason = widget(meta_line(&entry.reason, SemanticColorRole::Danger))
        .prop::<String, fields::text::value>(move || item.try_with(|entry| entry.reason.clone()).unwrap_or_default())
        .visible(move || !ready())
        .key(format!("playlist-item-status-{}", key_part(&id)));
    let meta = widget(Stack::column(4.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0))
        .children((item_title(item, &id, &entry.filename), path, reason));
    let play_id = id.clone();
    let remove_id = id.clone();
    let blocked = move || !ready() || !head.with(|head| head.has_player);
    let actions = widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).children((
        widget(toolbar_button("播放", icons::PLAY, ButtonKind::Ghost, blocked(), SemanticColorRole::Background))
            .prop::<ToolbarLook, ToolbarDisabled>(move || ToolbarLook::new(ButtonKind::Ghost, SemanticColorRole::Background, blocked()))
            .key(format!("playlist-item-play-{}", key_part(&id)))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(player_message(PlayerMessage::PlayListed { item_id: Some(play_id.clone()) }));
            }),
        widget(toolbar_button("移除", icons::TRASH_2, ButtonKind::Danger, false, SemanticColorRole::Background))
            .key(format!("playlist-remove-{}", key_part(&id)))
            .on_cx(move |_, _: &Activate, cx| {
                let playlist_id = head.with_untracked(|head| head.playlist_id.clone());
                cx.dispatch_program_all(ShellMessage::RemovePlaylistItem { playlist_id, item_id: remove_id.clone() });
            }),
    ));
    let row = Stack::row(12.0)
        .align(AlignSpec::Center)
        .width(LengthSpec::Fill)
        .surface(SemanticColorRole::Background)
        .radius(RadiusTier::Xl)
        .with_layout(|layout| {
            layout.padding_top = Some(LengthSpec::Px(10.0));
            layout.padding_bottom = Some(LengthSpec::Px(10.0));
            layout.padding_left = Some(LengthSpec::Px(12.0));
            layout.padding_right = Some(LengthSpec::Px(12.0));
        });
    widget(row)
        .prop::<bool, RowDim>(move || !ready())
        .children((drag_handle(&id), extension_mark(item, &id, &entry.mark), meta, actions))
        .key(format!("playlist-item-{}", key_part(&id)))
        .into_any()
}

/// 拖动柄：32px 方块里一枚弱色竖握把。整行都能拖，柄只是提示。
fn drag_handle(id: &str) -> AnyView {
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Px(32.0))
            .height(LengthSpec::Px(32.0))
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(nana_ui::runtime::IconGlyph::new(icons::GRIP_VERTICAL).size(16.0).role(SemanticColorRole::Faint)),))
    .key(format!("playlist-item-drag-{}", key_part(id)))
    .into_any()
}

/// 58px 扩展名方块：bg-subtle 底、12px 圆角、11px 粗体弱色字。点它打开预览。
fn extension_mark(item: EntryItem, id: &str, mark: &str) -> AnyView {
    let item_id = id.to_string();
    let mut button = Button::new(mark);
    button.style.background = Some(SemanticColorRole::Subtle);
    button.style.foreground = Some(SemanticColorRole::Muted);
    button.style.border = None;
    button.style.radius = Some(RadiusTier::Xl);
    button.style.control_height = None;
    button.style.control_padding_x = None;
    button.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    button.style.interaction.hovered.background = Some(SemanticColorRole::Hover);
    let layout = Arc::make_mut(&mut button.style.layout);
    for length in [&mut layout.width, &mut layout.height, &mut layout.min_width, &mut layout.min_height] {
        *length = Some(LengthSpec::Px(58.0));
    }
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.font_size = Some(11.0);
    layout.font_weight = Some(700);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    let style = button.style.clone();
    widget(Button::new(button.label).style(style))
        .prop::<String, fields::button::label>(move || item.try_with(|entry| entry.mark.clone()).unwrap_or_default())
        .key(format!("playlist-item-ext-{}", key_part(id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::OpenPreview { item_id: Some(item_id.clone()) }));
        })
        .into_any()
}

/// 文件名按钮：整行 32px 高的可点区域，14px 半粗正文色字靠左。点它打开这一条的预览，不开始播放。
/// Vue 的全局按钮把字居中，和下面靠左的路径错开，这里按设计意图靠左。
fn item_title(item: EntryItem, id: &str, title: &str) -> AnyView {
    let item_id = id.to_string();
    let filename = move || item.try_with(|entry| entry.filename.clone()).unwrap_or_default();
    let hit = widget(bar::hit_area(title, false, 0.0, 6.0))
        .prop::<Arc<str>, fields::icon_button::label>(move || Arc::from(filename()))
        .key(format!("playlist-item-title-{}", key_part(id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(player_message(PlayerMessage::OpenPreview { item_id: Some(item_id.clone()) }));
        });
    let mut label = Text::new(title).color(SemanticColorRole::Text).font_size(14.0).font_weight(600).line_height(21.7).truncating();
    Arc::make_mut(&mut label.style.layout).min_width = Some(LengthSpec::Px(0.0));
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .height(LengthSpec::Px(32.0))
            .min_height(LengthSpec::Px(32.0)),
    )
    .children((hit, widget(label).prop::<String, fields::text::value>(filename)))
    .into_any()
}

/// 12px 次行，超长省略。
fn meta_line(label: &str, role: SemanticColorRole) -> Text {
    let mut text = Text::new(label).color(role).font_size(12.0).line_height(18.6).truncating();
    let layout = Arc::make_mut(&mut text.style.layout);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    text
}

/// 工具栏按钮：34px 高、14px 图标在字前、6px 间距。普通操作透明底，危险操作是红字，悬停铺淡红。
/// 禁用时按 Vue 的整颗 0.45 不透明度处理：字色在 sRGB 里和按钮所在的底色 `base` 混。
fn toolbar_button(label: &str, icon: Icon, kind: ButtonKind, disabled: bool, base: SemanticColorRole) -> Button {
    let mut button = Button::new(label).kind(kind).icon(icon).icon_size(14.0).icon_gap(6.0).disabled(disabled);
    button.style.interaction.disabled = SemanticPaint::default();
    button.style.interaction.base.foreground_mix = ToolbarLook::new(kind, base, disabled).mix;
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.height = Some(LengthSpec::Px(34.0));
    layout.min_height = Some(LengthSpec::Px(34.0));
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.font_size = Some(14.0);
    layout.font_weight = Some(500);
    layout.width = Some(LengthSpec::Shrink);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    button
}

/// 虚线空框：bg-subtle 底、border-strong 虚线、12px 圆角，至少 280px 高，标题和说明居中。
fn dashed_empty(title: &str, message: &str, key: &'static str) -> nana_ui::runtime::view::El<Stack, (AnyView, AnyView)> {
    let mut heading = Text::new(title).color(SemanticColorRole::Text).font_size(18.0).font_weight(700).line_height(27.9);
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut note = Text::new(message).color(SemanticColorRole::Muted).font_size(13.0).line_height(20.15);
    note.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    widget(
        Stack::column(10.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderStrong, 1.0)
            .radius(RadiusTier::Xl)
            .min_height(LengthSpec::Px(280.0))
            .with_layout(|layout| layout.border_style = Some(nana_ui_core::BorderStyle::Dashed)),
    )
    .children((widget(heading).key(key).into_any(), widget(note).key(format!("{key}-hint")).into_any()))
    .key(format!("{key}-frame"))
}

/// 工具栏按钮的禁用和字色：禁用时字色是正文色（危险操作是危险色）按 0.45 和底色混出来的。
#[derive(Clone, Copy, Debug, PartialEq)]
struct ToolbarLook {
    disabled: bool,
    mix: Option<nana_ui_core::SemanticColorMix>,
}

impl ToolbarLook {
    fn new(kind: ButtonKind, base: SemanticColorRole, disabled: bool) -> Self {
        let role = if kind == ButtonKind::Danger { SemanticColorRole::Danger } else { SemanticColorRole::Text };
        Self { disabled, mix: disabled.then(|| nana_ui_core::SemanticColorMix::new(role, base, bar::DISABLED_OPACITY)) }
    }
}

/// 工具栏按钮按 [`ToolbarLook`] 换禁用和字色。
struct ToolbarDisabled;

impl FieldWrite<Button, ToolbarLook> for ToolbarDisabled {
    const FIELD: &'static str = "Button.disabled+style.interaction.base.foreground_mix";

    fn write(target: &mut Button, look: ToolbarLook) {
        target.disabled = look.disabled;
        target.style.interaction.base.foreground_mix = look.mix;
    }

    fn differs(target: &Button, look: &ToolbarLook) -> bool {
        target.disabled != look.disabled || target.style.interaction.base.foreground_mix != look.mix
    }
}

/// 不能播放的条目整行变淡。
struct RowDim;

impl FieldWrite<Stack, bool> for RowDim {
    const FIELD: &'static str = "Stack.style.layout.opacity";

    fn write(target: &mut Stack, dim: bool) {
        Arc::make_mut(&mut <Stack as StyledComponent>::node_style_mut(target).layout).opacity = dim.then_some(UNREADY_OPACITY);
    }

    fn differs(target: &Stack, dim: &bool) -> bool {
        <Stack as StyledComponent>::node_style(target).layout.opacity != dim.then_some(UNREADY_OPACITY)
    }
}

/// 面板圆角，跟外观设置里的圆角。
struct PanelRadius;

impl FieldWrite<Stack, f32> for PanelRadius {
    const FIELD: &'static str = "Stack.style.layout.border_radius";

    fn write(target: &mut Stack, radius: f32) {
        Arc::make_mut(&mut <Stack as StyledComponent>::node_style_mut(target).layout).border_radius = Some(radius.max(0.0));
    }

    fn differs(target: &Stack, radius: &f32) -> bool {
        <Stack as StyledComponent>::node_style(target).layout.border_radius != Some(radius.max(0.0))
    }
}

#[cfg(test)]
#[path = "player_playlist_page_tests.rs"]
mod tests;
