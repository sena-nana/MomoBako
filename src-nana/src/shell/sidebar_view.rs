//! 侧栏视图：仓库头、文件管理分组和底部入口。
//!
//! 结构照 Vue `SecondaryPanel.vue`：仓库头 → 状态 → 快捷方式 → 快捷访问 → 动作 → 播放集 →
//! 文件夹 → 智能文件夹，底部是设置、拓展、任务和日志。文件夹树、仓库弹层和对话框在子模块里。
//! 状态是全局状态区（`status.rs`）：最近一次失败，没有失败时是忙碌行，再没有时是刷新文件夹树的同步进度。
//!
//! 侧栏常驻：树只建一次，文字、当前态、禁用和计数按 [`SidebarSignals`] 绑定；可有可无的分组、
//! 说明和角标用 `.visible`，节点留着、不占布局；列表用带键的 `each`。事件处理器只发意图消息，
//! 参数是建行时就定下的编号，或者点击时现读的信号。

use std::sync::Arc;

use nana_ui::icons_tabler::{
    ARCHIVE, BOOKMARK, CLIPBOARD_LIST, CLOCK_HOUR_3, FILE, LIST_TREE, LOADER_2, LOGS, PLAYER_PLAY, PLUS, PUZZLE, REFRESH,
    SELECTOR, SETTINGS, TAG, TRASH,
};
use nana_ui::runtime::view::{each, fields, widget, AnyView, El, IntoView, Item, Signal, Store, StoreList, StorePath};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, LengthSpec, ListItem, RadiusTier, SemanticColorRole, SidebarFrame, Stack,
};
use nana_ui::{ButtonKind, ControlSize, Icon};

use super::hot::{HotSignals, SpinField};
use super::status::StatusLine;
use super::player_view::key_part;
use super::sidebar::ShortcutId;
use super::view_part_sidebar::project::{
    playlist_key, FooterView, HeadView, NavView, PlaylistGroup, PlaylistRow, PlaylistRowStoreFields, QuickRow, QuickTarget,
    SidebarSignals,
};
use super::SidebarMessage;
use super::{ShellMessage, WorkspacePanel};

#[path = "sidebar_parts.rs"]
pub(super) mod parts;
#[path = "sidebar_tree_view.rs"]
mod tree;
#[path = "sidebar_popover_view.rs"]
mod popover;
#[path = "sidebar_dialogs.rs"]
mod dialogs;

pub use dialogs::{
    folder_delete_dialog, folder_dialog, playlist_create_dialog, repository_delete_dialog, smart_delete_dialog, smart_folder_dialog,
};
pub use popover::repository_popover;
pub use tree::folder_menu;

use parts::{bound_hint, footer_button, group_header, group_title, nav_row, tree_action, FooterLook, NavLook};

/// 五个快捷方式，顺序同 Vue，也是 [`NavView::shortcuts`] 的顺序。
const SHORTCUTS: [ShortcutId; 5] =
    [ShortcutId::All, ShortcutId::Uncategorized, ShortcutId::Untagged, ShortcutId::Recent, ShortcutId::Trash];

/// 播放集 Store 里的一行。
type PlaylistItem = Item<Store<Vec<PlaylistRow>>, String, PlaylistRow>;

/// 实况侧栏。Vue `.workspace-sidebar` 的内边距是 10px 8px，各块之间没有额外间距。
/// 资源区比侧栏宽度少一条发丝间隙（见 `render::workbench`），右内边距同样扣掉这一条，
/// 内容盒仍是 Vue 的 8..宽度-8。
pub fn sidebar_frame() -> SidebarFrame {
    let mut frame = SidebarFrame::new().gap(0.0);
    let layout = Arc::make_mut(&mut frame.style.layout);
    layout.padding_top = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(8.0 - super::render::WORKBENCH_GAP_PX));
    layout.padding_bottom = Some(LengthSpec::Px(10.0));
    layout.padding_left = Some(LengthSpec::Px(8.0));
    frame
}

/// 常驻侧栏：仓库头、分组和底部入口，放进工作区的资源区。底部入口平时的透明度读热信号。
///
/// 三个槽位的根节点（仓库头外框、分组外的滚动区、底部入口行）上不放绑定：侧栏外框装配时给它们
/// 打布局补丁，带绑定的根节点重投影会把补丁冲掉。
pub(crate) fn sidebar(signals: SidebarSignals, hot: HotSignals) -> AnyView {
    widget(sidebar_frame())
        .top(sidebar_switcher(signals.head))
        .body(sidebar_sections(signals, hot.spinner))
        .footer(sidebar_footer(signals.footer, hot.footer))
        .into_any()
}

/// 仓库头的无障碍名：「资源库 · 仓库名」。Vue 的 `aria-label` 只有「资源库」，读屏听不到当前是哪个库，
/// 这里把仓库名一起读出来。
fn switcher_label(name: &str) -> String {
    format!("资源库 · {name}")
}

/// 仓库头：当前仓库名加上下箭头，点开切换弹层。对应 `WorkspaceSidebarRepoHeader.vue`。
fn sidebar_switcher(head: Signal<HeadView>) -> AnyView {
    let name = head.with_untracked(|head| head.name.clone());
    let mut style = parts::row_style(28.0, 8.0, 8.0, 6.0, parts::ActiveTone::Accent, false);
    Arc::make_mut(&mut style.layout).font_weight = Some(600);
    let content = widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
        widget(parts::fill_text(name.clone(), 13.0, 600)).prop::<String, fields::text::value>(move || head.with(|head| head.name.clone())),
        widget(parts::inherit_icon(SELECTOR, 13.0)),
    ));
    let switcher = widget(ListItem::new(switcher_label(&name)).style(style))
        .prop::<String, fields::list_item::label>(move || head.with(|head| switcher_label(&head.name)))
        .content(content)
        .key("repository-switcher")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenRepositorySwitcher));
        });
    let top = Stack::column(0.0).with_layout(|layout| {
        layout.padding_bottom = Some(LengthSpec::Px(8.0));
        layout.border_bottom_width = Some(1.0);
    });
    let mut top_style = top.node_style();
    top_style.border = Some(SemanticColorRole::BorderSoft);
    widget(top.style(top_style))
        .children((widget(Stack::column(0.0).padding_xy(2.0, 0.0)).children((switcher,)),))
        .into_any()
}

/// 文件管理区：状态、快捷方式、快捷访问、动作、播放集、文件夹和智能文件夹，分组间 10px。
/// 没有内容的分组藏起来，不占间距。忙碌行的转圈读热信号 `spin`。
fn sidebar_sections(signals: SidebarSignals, spin: Signal<f32>) -> AnyView {
    let head = signals.head;
    widget(Stack::column(10.0).with_layout(|layout| {
        layout.padding_right = Some(LengthSpec::Px(2.0));
    }))
    .children((
        status_line(signals.status, spin),
        shortcut_group(signals.nav),
        quick_access_group(signals.quick, signals.nav),
        actions_group(signals.nav),
        playlist_group(signals.playlists, signals.playlist_rows),
        tree::folder_group(signals.folders, signals.folder_rows, move || head.with(|head| head.folders_visible)),
        tree::smart_group(signals.smart, signals.smart_rows),
    ))
    .into_any()
}

/// 侧栏顶部的全局状态区，对应 `WorkspaceSidebarStatus.vue`：有失败时是错误条（`.workspace-state--error`），
/// 没有失败、在读资源库或素材详情时是忙碌行（`.workspace-state`：转圈加「正在同步仓库状态」），
/// 再没有时是刷新文件夹树的同步进度（`.workspace-state--progress`），都没有时整块藏起来、不占间距。
/// 三块都留着，按投影互换。
fn status_line(status: Signal<StatusLine>, spin: Signal<f32>) -> AnyView {
    let failure = move || status.with(|status| match status {
        StatusLine::Failure(message) => message.clone(),
        _ => String::new(),
    });
    let mut copy = parts::label_text(failure(), 12.0, 400, Some(SemanticColorRole::Danger)).line_height(18.0);
    {
        let layout = Arc::make_mut(&mut copy.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    let error = widget(
        Stack::column(0.0)
            .padding_xy(8.0, 6.0)
            .min_height(LengthSpec::Px(30.0))
            .justify(nana_ui::runtime::JustifySpec::Center)
            .radius(RadiusTier::Sm)
            .painter(super::shell_tint::SoftFill::err())
            .with_layout(state_margin),
    )
    .visible(move || status.with(|status| matches!(status, StatusLine::Failure(_))))
    .children((widget(copy).prop::<String, fields::text::value>(failure).key("sidebar-status-error"),))
    .key("sidebar-status");
    (error, busy_line(status, spin), progress_line(status, spin)).into_any()
}

/// 同步进度（`.workspace-state--progress`）：和忙碌行同一副底色和转圈，占满一行，左边是这一步的文案，
/// 右边是弱色的百分比（`.workspace-state__percent` 的 `margin-left: auto`）。
fn progress_line(status: Signal<StatusLine>, spin: Signal<f32>) -> AnyView {
    let progress = move || {
        status.with(|status| match status {
            StatusLine::Progress { label, percent } => (label.clone(), format!("{percent}%")),
            _ => (String::new(), String::new()),
        })
    };
    let (label, percent) = progress();
    let mut glyph = nana_ui::runtime::IconGlyph::new(LOADER_2).size(16.0).role(SemanticColorRole::Muted);
    Arc::make_mut(&mut glyph.style.layout).transform = Some(super::motion::spin_transform(spin.get_untracked()));
    let mut text = parts::label_text(label, 12.0, 400, Some(SemanticColorRole::Muted)).line_height(18.0);
    {
        let layout = Arc::make_mut(&mut text.style.layout);
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
    widget(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .min_height(LengthSpec::Px(30.0))
            .padding_xy(8.0, 0.0)
            .radius(RadiusTier::Sm)
            .surface(SemanticColorRole::Subtle)
            .with_layout(state_margin),
    )
    .visible(move || status.with(|status| matches!(status, StatusLine::Progress { .. })))
    .children((
        widget(glyph).prop::<f32, SpinField>(spin).key("sidebar-status-progress-spinner"),
        widget(text).prop::<String, fields::text::value>(move || progress().0).key("sidebar-status-progress-label"),
        widget(parts::label_text(percent, 12.0, 400, Some(SemanticColorRole::Faint)).line_height(18.0))
            .prop::<String, fields::text::value>(move || progress().1)
            .key("sidebar-status-progress-percent"),
    ))
    .key("sidebar-status-progress")
    .into_any()
}

/// 忙碌行：`--bg-subtle` 底、弱色 12px 字，前面一个 16px 的转圈，宽度随内容。转圈的角度读热信号。
fn busy_line(status: Signal<StatusLine>, spin: Signal<f32>) -> AnyView {
    let mut glyph = nana_ui::runtime::IconGlyph::new(LOADER_2).size(16.0).role(SemanticColorRole::Muted);
    Arc::make_mut(&mut glyph.style.layout).transform = Some(super::motion::spin_transform(spin.get_untracked()));
    widget(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .width(LengthSpec::Shrink)
            .min_height(LengthSpec::Px(30.0))
            .padding_xy(8.0, 0.0)
            .radius(RadiusTier::Sm)
            .surface(SemanticColorRole::Subtle)
            .with_layout(state_margin),
    )
    .visible(move || status.with(|status| *status == StatusLine::Busy))
    .children((
        widget(glyph).prop::<f32, SpinField>(spin).key("sidebar-status-spinner"),
        widget(parts::label_text("正在同步仓库状态", 12.0, 400, Some(SemanticColorRole::Muted)).line_height(18.0))
            .key("sidebar-status-busy-text"),
    ))
    .key("sidebar-status-busy")
    .into_any()
}

/// `.workspace-state` 的 `margin: 2px 0`。
fn state_margin(layout: &mut nana_ui_core::LayoutStyle) {
    layout.margin_top = Some(LengthSpec::Px(2.0));
    layout.margin_bottom = Some(LengthSpec::Px(2.0));
}

/// 五个快捷方式，无标题。当前项 `--accent-soft` 底、强调色，缺失仓库时整组禁用。
fn shortcut_group(nav: Signal<NavView>) -> AnyView {
    let rows = SHORTCUTS
        .into_iter()
        .enumerate()
        .map(|(index, id)| {
            let look = move || {
                nav.with(|nav| {
                    let (count, active) = nav.shortcuts[index];
                    NavLook { active, disabled: nav.locked, count: Some(count) }
                })
            };
            nav_row(id.label(), Some(shortcut_icon(id)), look, format!("shortcut-{}", shortcut_key(id)), move |cx| {
                cx.dispatch_program_all(sidebar_message(SidebarMessage::SelectShortcut(id)));
            })
        })
        .collect::<Vec<_>>();
    widget(Stack::column(1.0)).children(rows).key("sidebar-shortcuts").into_any()
}

/// 快捷方式图标，对应 Vue 的 lucide：archive、folder-tree、tag、clock-3、trash-2。
fn shortcut_icon(id: ShortcutId) -> Icon {
    match id {
        ShortcutId::All => ARCHIVE,
        ShortcutId::Uncategorized => LIST_TREE,
        ShortcutId::Untagged => TAG,
        ShortcutId::Recent => CLOCK_HOUR_3,
        ShortcutId::Trash => TRASH,
    }
}

fn shortcut_key(id: ShortcutId) -> &'static str {
    match id {
        ShortcutId::All => "all",
        ShortcutId::Uncategorized => "uncategorized",
        ShortcutId::Untagged => "untagged",
        ShortcutId::Recent => "recent",
        ShortcutId::Trash => "trash",
    }
}

/// 快捷访问：仓库摘要里的书签。智能文件夹用书签图标，文件用文件图标，其余用目录图标。
/// 没有书签时整组藏起来。
fn quick_access_group(quick: Signal<Vec<QuickRow>>, nav: Signal<NavView>) -> AnyView {
    let rows = each(quick, QuickRow::clone, move |row: QuickRow| {
        let icon = match row.target {
            QuickTarget::SmartFolder => BOOKMARK,
            QuickTarget::File => FILE,
            QuickTarget::Folder => LIST_TREE,
        };
        let look = move || nav.with(|nav| NavLook { active: false, disabled: nav.locked, count: None });
        let id = row.id.clone();
        nav_row(&row.label, Some(icon), look, format!("quick-access-{}", key_part(&row.id)), move |cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenQuickAccess(id.clone())));
        })
    })
    .gap(1.0);
    group(group_header(widget(group_title("快捷访问")).into_any(), Vec::new(), "quick-access-header"), vec![rows.into_any()])
        .visible(move || quick.with(|rows| !rows.is_empty()))
        .into_any()
}

/// 仓库动作入口。有动作时才出现，右侧是动作数量。
fn actions_group(nav: Signal<NavView>) -> AnyView {
    let look = move || nav.with(|nav| NavLook { active: nav.actions_active, disabled: nav.locked, count: Some(nav.actions) });
    let row = nav_row("动作", Some(CLIPBOARD_LIST), look, "sidebar-actions".into(), |cx| {
        cx.dispatch_program_all(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    });
    group(group_header(widget(group_title("动作")).into_any(), Vec::new(), "actions-header"), vec![
        widget(Stack::column(1.0)).children((row,)).into_any(),
    ])
    .visible(move || nav.with(|nav| nav.actions > 0))
    .into_any()
}

/// 一个侧栏分组：标题和正文，间距 4px。对应 `.workspace-group`。正文里藏起来的块不占间距。
pub(super) fn group(header: AnyView, body: Vec<AnyView>) -> El<Stack, Vec<AnyView>> {
    let mut children = vec![header];
    children.extend(body);
    widget(Stack::column(4.0)).children(children)
}

/// 播放集分组。标题可点开合，默认收起；收起时只留标题。对应 `WorkspaceSidebarPlaylists.vue`。
fn playlist_group(state: Signal<PlaylistGroup>, rows: Store<Vec<PlaylistRow>>) -> AnyView {
    let tools = vec![tree_action(PLUS, "新建播放集", "playlist-create", move || state.with(|state| state.create_locked), |cx| {
        cx.dispatch_program_all(ShellMessage::OpenPlaylistDialog);
    })
    .into_any()];
    let hint = bound_hint(move || state.with(|state| state.hint.filter(|_| state.expanded)), "playlist-hint");
    let list = rows
        .keyed(playlist_key)
        .each(playlist_row)
        .gap(6.0)
        .key("playlist-list")
        .visible(move || state.with(|state| state.expanded && state.hint.is_none()));
    group(group_header(playlist_title(state), tools, "playlist-header"), vec![hint, list.into_any()]).into_any()
}

/// 「播放集」标题按钮的无障碍名：说明下一步动作。
fn playlist_toggle_label(expanded: bool) -> &'static str {
    if expanded { "收起播放集" } else { "展开播放集" }
}

/// 「播放集」标题按钮：标题加个数，点一下展开或收起。
fn playlist_title(state: Signal<PlaylistGroup>) -> AnyView {
    let initial = state.get_untracked();
    let mut count_text = parts::label_text(initial.count.to_string(), 11.0, 700, Some(SemanticColorRole::Muted));
    Arc::make_mut(&mut count_text.style.layout).letter_spacing = Some(0.0);
    let content = widget(Stack::row(6.0).align(AlignSpec::Center)).children((
        widget(group_title("播放集")),
        widget(count_text).prop::<String, fields::text::value>(move || state.with(|state| state.count.to_string())),
    ));
    let mut style = parts::row_style(24.0, 0.0, 0.0, 6.0, parts::ActiveTone::Accent, false);
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Shrink);
        layout.flex_grow = Some(0.0);
    }
    style.foreground = Some(SemanticColorRole::Faint);
    style.interaction.hovered.foreground = Some(SemanticColorRole::Faint);
    style.interaction.pressed.foreground = Some(SemanticColorRole::Faint);
    widget(ListItem::new(playlist_toggle_label(initial.expanded)).style(style))
        .prop::<String, fields::list_item::label>(move || state.with(|state| playlist_toggle_label(state.expanded).to_string()))
        .content(content)
        .key("playlist-toggle")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(sidebar_message(SidebarMessage::TogglePlaylists)))
        .into_any()
}

/// 展开后的一个播放集：名称、「播放器 · N 项」，右侧播放和删除。当前播放集 `--accent-soft` 底。
/// 名称、项数、当前态和能否播放按这一行的 Store 字段原地改。
fn playlist_row(item: PlaylistItem) -> AnyView {
    let row = item.get_untracked();
    let id = row.playlist_id;
    let mut name = parts::label_text(row.name.clone(), 13.0, 600, Some(SemanticColorRole::Text)).truncating();
    Arc::make_mut(&mut name.style.layout).width = Some(LengthSpec::Fill);
    let mut meta = parts::label_text(row.meta, 11.0, 400, Some(SemanticColorRole::Muted)).truncating();
    Arc::make_mut(&mut meta.style.layout).width = Some(LengthSpec::Fill);
    let mut main_style = parts::row_style(42.0, 8.0, 8.0, 0.0, parts::ActiveTone::Accent, false);
    {
        let layout = Arc::make_mut(&mut main_style.layout);
        layout.direction = Some(nana_ui_core::FlexDirection::Column);
        layout.align_items = AlignSpec::Start;
        layout.justify_content = nana_ui::runtime::JustifySpec::Center;
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.width = Some(LengthSpec::Px(0.0));
    }
    main_style.interaction.selected = main_style.interaction.hovered;
    let open_id = id.clone();
    let main = widget(ListItem::new(row.name).style(main_style))
        .prop::<String, fields::list_item::label>(item.name())
        .content(widget(Stack::column(0.0).width(LengthSpec::Fill)).children((
            widget(name).prop::<String, fields::text::value>(item.name()),
            widget(meta).prop::<String, fields::text::value>(item.meta()),
        )))
        .key(format!("playlist-open-{}", key_part(&id)))
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::OpenSidebarPlaylist(open_id.clone())));
        });
    let play_id = id.clone();
    let remove_id = id.clone();
    let actions = widget(Stack::row(2.0).align(AlignSpec::Center)).children((
        tree_action(PLAYER_PLAY, "播放播放集", "playlist-play", move || !item.playable().get(), move |cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::PlayPlaylist(play_id.clone()))));
        }),
        danger_tree_action(TRASH, "删除播放集", move |cx| {
            cx.dispatch_program_all(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::RemovePlaylist(remove_id.clone()))));
        }),
    ));
    widget(Stack::bar(6.0).align(AlignSpec::Center).padding(4.0).radius(RadiusTier::Md))
        .background(move || item.active().get().then_some(SemanticColorRole::AccentSoft))
        .children((main, actions))
        .key(format!("playlist-item-{}", key_part(&id)))
        .into_any()
}

/// 危险的标题工具：悬停换浅红底。对应 `.workspace-tree-action--danger`。
pub(super) fn danger_tree_action(
    icon: Icon,
    label: &'static str,
    on_activate: impl Fn(&mut nana_ui::runtime::ViewContext<IconButton>) + Send + 'static,
) -> AnyView {
    let mut button = IconButton::new(icon, label).size(ControlSize::Medium).kind(ButtonKind::Danger).with_tooltip(label);
    button.style = parts::icon_button_style(22.0, RadiusTier::Xs, SemanticColorRole::Faint, None, false);
    button.style.interaction.hovered.background = Some(SemanticColorRole::DangerSoftHover);
    button.style.interaction.hovered.foreground = Some(SemanticColorRole::Danger);
    widget(button.colors_from_style()).on_cx(move |_, _: &Activate, cx| on_activate(cx)).into_any()
}

/// 设置、拓展、任务和日志。当前入口 `--accent-soft` 底，其余平时 0.44 透明度，悬停底栏时为 1。
/// 任务入口外面套一层定位用的行，有任务时右上角显示数量角标，没有时角标藏起来。
fn sidebar_footer(footer: Signal<FooterView>, rest: Signal<f32>) -> AnyView {
    let look = move |active: fn(&FooterView) -> bool| move || footer.with(|footer| FooterLook { active: active(footer), highlight: false });
    let tasks = move || footer.with(|footer| FooterLook { active: footer.tasks_open, highlight: footer.tasks > 0 });
    let task = widget(Stack::row(0.0).with_layout(|layout| layout.position = nana_ui_core::PositionSpec::Relative)).children((
        footer_button(CLIPBOARD_LIST, "任务", "footer-tasks", tasks, rest, |cx| {
            cx.dispatch_program_all(ShellMessage::Admin(super::admin::AdminMessage::ToggleTaskPopover));
        }),
        task_badge(footer),
    ));
    widget(Stack::row(2.0).align(AlignSpec::Center))
        .key("sidebar-footer")
        .children((
            footer_button(SETTINGS, "设置", "footer-settings", look(|footer| footer.settings), rest, |cx| {
                cx.dispatch_program_all(ShellMessage::OpenSettings);
            }),
            footer_button(PUZZLE, "拓展", "footer-extensions", look(|footer| footer.extensions), rest, |cx| {
                cx.dispatch_program_all(ShellMessage::SetWorkspacePanel(WorkspacePanel::Extensions));
            }),
            task,
            footer_button(LOGS, "日志", "footer-logs", look(|footer| footer.logs), rest, |cx| {
                cx.dispatch_program_all(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
            }),
        ))
        .into_any()
}

/// 任务入口右上角的数量角标。对应 `.task-button__badge`：12px 高、强调色底、9px 粗体。
fn task_badge(footer: Signal<FooterView>) -> AnyView {
    let count = footer.with_untracked(|footer| footer.tasks);
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .justify(nana_ui::runtime::JustifySpec::Center)
            .surface(SemanticColorRole::Accent)
            .radius_px(999.0)
            .with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                layout.offset_top = Some(LengthSpec::Px(2.0));
                layout.offset_right = Some(LengthSpec::Px(2.0));
                layout.min_width = Some(LengthSpec::Px(12.0));
                layout.height = Some(LengthSpec::Px(12.0));
                layout.padding_left = Some(LengthSpec::Px(3.0));
                layout.padding_right = Some(LengthSpec::Px(3.0));
                layout.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
            }),
    )
    .visible(move || footer.with(|footer| footer.tasks > 0))
    .children((widget(parts::label_text(count.to_string(), 9.0, 700, Some(SemanticColorRole::AccentText)).line_height(12.0))
        .prop::<String, fields::text::value>(move || footer.with(|footer| footer.tasks.to_string())),))
    .key("footer-task-badge")
    .into_any()
}

/// 加载中的刷新按钮换成转圈图标。
pub(super) fn refresh_icon(loading: bool) -> Icon {
    if loading { LOADER_2 } else { REFRESH }
}

pub(super) fn sidebar_message(message: SidebarMessage) -> ShellMessage {
    ShellMessage::Sidebar(message)
}
