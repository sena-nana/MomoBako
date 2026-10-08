//! 侧栏视图：仓库头、文件管理分组和底部入口。
//!
//! 结构照 Vue `SecondaryPanel.vue`：仓库头 → 状态 → 快捷方式 → 快捷访问 → 动作 → 播放集 →
//! 文件夹 → 智能文件夹，底部是设置、拓展、任务和日志。文件夹树、仓库弹层和对话框在子模块里。

use std::sync::Arc;

use nana_ui::icons_tabler::{
    ARCHIVE, BOOKMARK, CLIPBOARD_LIST, CLOCK_HOUR_3, FILE, LIST_TREE, LOADER_2, LOGS, PLAYER_PLAY, PLUS, PUZZLE, REFRESH,
    SELECTOR, SETTINGS, TAG, TRASH,
};
use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, IconButton, LengthSpec, ListItem, RadiusTier, SemanticColorRole, SidebarFrame, Stack,
};
use nana_ui::{ButtonKind, ControlSize, Icon};

use super::sidebar::ShortcutId;
use super::SidebarMessage;
use super::{LibraryCategory, ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

#[path = "sidebar_parts.rs"]
pub(super) mod parts;
#[path = "sidebar_tree_view.rs"]
mod tree;
#[path = "sidebar_popover_view.rs"]
mod popover;
#[path = "sidebar_dialogs.rs"]
mod dialogs;

pub use dialogs::{
    bind_field_labels, folder_delete_dialog, folder_dialog, playlist_create_dialog, repository_delete_dialog, smart_delete_dialog,
    smart_folder_dialog,
};
pub use popover::repository_popover;
pub use tree::folder_menu;

use parts::{empty_hint, group_header, group_title, tree_action, FooterButton, NavRow};

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

/// 仓库头：当前仓库名加上下箭头，点开切换弹层。对应 `WorkspaceSidebarRepoHeader.vue`。
///
/// 无障碍名写成「资源库 · 仓库名」：Vue 的 `aria-label` 只有「资源库」，读屏听不到当前是哪个库，
/// 这里把仓库名一起读出来。
pub fn sidebar_switcher(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let name = model
        .workspace
        .active_repository()
        .map(|repository| repository.name.clone())
        .unwrap_or_else(|| "无资源库".into());
    let mut style = parts::row_style(28.0, 8.0, 8.0, 6.0, parts::ActiveTone::Accent, false);
    Arc::make_mut(&mut style.layout).font_weight = Some(600);
    let content = widget(Stack::fill_row(6.0).align(AlignSpec::Center)).children((
        parts::fill_label(name.clone(), 13.0, 600),
        widget(parts::inherit_icon(SELECTOR, 13.0)),
    ));
    let switcher = widget(ListItem::new(format!("资源库 · {name}")).style(style))
        .content(content)
        .key("repository-switcher")
        .on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::OpenRepositorySwitcher));
        });
    let top = Stack::column(0.0).with_layout(|layout| {
        layout.padding_bottom = Some(LengthSpec::Px(8.0));
        layout.border_bottom_width = Some(1.0);
    });
    let mut top_style = top.node_style();
    top_style.border = Some(SemanticColorRole::BorderSoft);
    widget(top.style(top_style)).children((widget(Stack::column(0.0).padding_xy(2.0, 0.0)).children((switcher,)),))
}

/// 文件管理区：状态、快捷方式、快捷访问、动作、播放集、文件夹和智能文件夹，分组间 10px。
pub fn sidebar_sections(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let locked = model.navigation_locked();
    let mut sections = Vec::new();
    if let Some(status) = status_line(model) {
        sections.push(status);
    }
    sections.push(shortcut_group(model, locked));
    if let Some(group) = quick_access_group(model, locked) {
        sections.push(group);
    }
    if let Some(group) = actions_group(model, locked) {
        sections.push(group);
    }
    sections.push(playlist_group(model, locked));
    if folder_sidebar_visible(model) {
        sections.push(tree::folder_group(model, locked));
    }
    sections.push(tree::smart_group(model, locked));
    widget(Stack::column(10.0).with_layout(|layout| {
        layout.padding_right = Some(LengthSpec::Px(2.0));
    }))
    .children(sections)
}

/// Vue `showFolderSidebar`：虚拟条目来源没有真实目录，不显示文件夹分组。
fn folder_sidebar_visible(model: &ShellViewModel) -> bool {
    model
        .workspace
        .active_repository()
        .is_none_or(|repository| !repository.capabilities.iter().any(|capability| capability == "virtual-entries"))
}

/// 侧栏顶部的错误条：目录树或智能文件夹读取失败时显示，对应 `.workspace-state--error`。
fn status_line(model: &ShellViewModel) -> Option<AnyView> {
    let error = [&model.sidebar.tree_error, &model.sidebar.smart_error].into_iter().find(|error| !error.is_empty())?;
    let mut copy = parts::label_text(error.clone(), 12.0, 400, Some(SemanticColorRole::Danger)).line_height(18.0);
    {
        let layout = Arc::make_mut(&mut copy.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    Some(
        widget(
            Stack::column(0.0)
                .padding_xy(8.0, 6.0)
                .min_height(LengthSpec::Px(30.0))
                .justify(nana_ui::runtime::JustifySpec::Center)
                .radius(RadiusTier::Sm)
                .painter(super::shell_tint::SoftFill::err()),
        )
        .children((widget(copy).key("sidebar-status-error"),))
        .key("sidebar-status")
        .into_any(),
    )
}

/// 五个快捷方式，无标题。当前项 `--accent-soft` 底、强调色，缺失仓库时整组禁用。
fn shortcut_group(model: &ShellViewModel, locked: bool) -> AnyView {
    let counts = model.sidebar.counts;
    let files = model.workspace.panel == WorkspacePanel::Files;
    let category = model.workspace.library_category;
    let items = [
        (ShortcutId::All, counts.all, files && category == LibraryCategory::All),
        (ShortcutId::Uncategorized, counts.uncategorized, files && category == LibraryCategory::Uncategorized),
        (ShortcutId::Untagged, counts.untagged, files && category == LibraryCategory::Untagged),
        (ShortcutId::Recent, counts.recent, files && category == LibraryCategory::Recent),
        (ShortcutId::Trash, counts.trash, model.workspace.panel == WorkspacePanel::Trash),
    ];
    let rows = items
        .into_iter()
        .map(|(id, count, active)| {
            NavRow { label: id.label().into(), icon: Some(shortcut_icon(id)), count: Some(count.to_string()), active, disabled: locked }
                .view(format!("shortcut-{}", shortcut_key(id)), move |cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::SelectShortcut(id)));
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
fn quick_access_group(model: &ShellViewModel, locked: bool) -> Option<AnyView> {
    if model.sidebar.quick_access.is_empty() {
        return None;
    }
    let rows = model
        .sidebar
        .quick_access
        .iter()
        .map(|shortcut| {
            let id = shortcut.id.clone();
            let icon = match shortcut.target_kind.as_str() {
                "smartFolder" => BOOKMARK,
                "file" => FILE,
                _ => LIST_TREE,
            };
            NavRow { label: shortcut.label.clone(), icon: Some(icon), count: None, active: false, disabled: locked }
                .view(format!("quick-access-{id}"), move |cx| {
                    cx.dispatch_program(sidebar_message(SidebarMessage::OpenQuickAccess(id.clone())));
                })
        })
        .collect::<Vec<_>>();
    Some(group(group_header(widget(group_title("快捷访问")).into_any(), Vec::new(), "quick-access-header"), vec![
        widget(Stack::column(1.0)).children(rows).into_any(),
    ]))
}

/// 仓库动作入口。有动作时才出现，右侧是动作数量。
fn actions_group(model: &ShellViewModel, locked: bool) -> Option<AnyView> {
    if model.admin.actions.is_empty() {
        return None;
    }
    let row = NavRow {
        label: "动作".into(),
        icon: Some(CLIPBOARD_LIST),
        count: Some(model.admin.actions.len().to_string()),
        active: model.workspace.panel == WorkspacePanel::Actions,
        disabled: locked,
    }
    .view("sidebar-actions".into(), |cx| cx.dispatch_program(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions)));
    Some(group(group_header(widget(group_title("动作")).into_any(), Vec::new(), "actions-header"), vec![
        widget(Stack::column(1.0)).children((row,)).into_any(),
    ]))
}

/// 一个侧栏分组：标题和正文，间距 4px。对应 `.workspace-group`。
pub(super) fn group(header: AnyView, body: Vec<AnyView>) -> AnyView {
    let mut children = vec![header];
    children.extend(body);
    widget(Stack::column(4.0)).children(children).into_any()
}

/// 播放集分组。标题可点开合，默认收起；收起时只留标题。对应 `WorkspaceSidebarPlaylists.vue`。
fn playlist_group(model: &ShellViewModel, locked: bool) -> AnyView {
    let has_repo = model.workspace.active_repo_id.is_some();
    let expanded = model.sidebar.playlists_expanded;
    let count = model.sidebar.playlists.len();
    let create_locked = !has_repo || locked || model.playlist_players.is_empty();
    let title = playlist_title(count, expanded);
    let tools = vec![tree_action(PLUS, "新建播放集", "playlist-create", create_locked, |cx| {
        cx.dispatch_program(ShellMessage::OpenPlaylistDialog);
    })];
    let mut body = Vec::new();
    if expanded {
        body.push(if !has_repo {
            empty_hint("先选择或添加一个资源库。", "playlist-empty")
        } else if locked {
            empty_hint("资源库修复后可继续使用播放集。", "playlist-missing")
        } else if !model.sidebar.playlists.is_empty() {
            playlist_list(model)
        } else if model.playlist_players.is_empty() {
            empty_hint("当前没有可用的播放插件类型。", "playlist-no-player")
        } else {
            empty_hint("还没有播放集。", "playlist-none")
        });
    }
    group(group_header(title, tools, "playlist-header"), body)
}

/// 「播放集」标题按钮：标题加个数，点一下展开或收起。无障碍名说明下一步动作。
fn playlist_title(count: usize, expanded: bool) -> AnyView {
    let label = if expanded { "收起播放集" } else { "展开播放集" };
    let mut count_text = parts::label_text(count.to_string(), 11.0, 700, Some(SemanticColorRole::Muted));
    Arc::make_mut(&mut count_text.style.layout).letter_spacing = Some(0.0);
    let content = widget(Stack::row(6.0).align(AlignSpec::Center)).children((widget(group_title("播放集")), widget(count_text)));
    let mut style = parts::row_style(24.0, 0.0, 0.0, 6.0, parts::ActiveTone::Accent, false);
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Shrink);
        layout.flex_grow = Some(0.0);
    }
    style.foreground = Some(SemanticColorRole::Faint);
    style.interaction.hovered.foreground = Some(SemanticColorRole::Faint);
    style.interaction.pressed.foreground = Some(SemanticColorRole::Faint);
    widget(ListItem::new(label).style(style))
        .content(content)
        .key("playlist-toggle")
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(sidebar_message(SidebarMessage::TogglePlaylists)))
        .into_any()
}

/// 展开后的播放集：名称、「播放器 · N 项」，右侧播放和删除。当前播放集 `--accent-soft` 底。
fn playlist_list(model: &ShellViewModel) -> AnyView {
    let mut rows = Vec::new();
    for playlist in &model.sidebar.playlists {
        let id = playlist.id.clone();
        let active = model.workspace.panel == WorkspacePanel::Playlist && model.sidebar.active_playlist_id.as_deref() == Some(playlist.id.as_str());
        let item_count = model
            .player
            .listed
            .as_ref()
            .filter(|detail| detail.playlist.playlist_id == playlist.id)
            .map(|detail| detail.items.len() as i64)
            .unwrap_or(playlist.item_count);
        let mut name = parts::label_text(playlist.name.clone(), 13.0, 600, Some(SemanticColorRole::Text)).truncating();
        Arc::make_mut(&mut name.style.layout).width = Some(LengthSpec::Fill);
        let mut meta = parts::label_text(format!("{} · {} 项", playlist.player_label, item_count), 11.0, 400, Some(SemanticColorRole::Muted)).truncating();
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
        let main = widget(ListItem::new(playlist.name.clone()).style(main_style))
            .content(widget(Stack::column(0.0).width(LengthSpec::Fill)).children((widget(name), widget(meta))))
            .key(format!("playlist-open-{id}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenSidebarPlaylist(open_id.clone())));
            });
        let playable = !super::player_view::playlist_plugin_missing(model, &playlist.player_type_id);
        let play_id = id.clone();
        let remove_id = id.clone();
        let actions = widget(Stack::row(2.0).align(AlignSpec::Center)).children((
            tree_action(PLAYER_PLAY, "播放播放集", "playlist-play", !playable, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::PlayPlaylist(play_id.clone()))));
            }),
            danger_tree_action(TRASH, "删除播放集", move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::Gap(super::sidebar::GapMessage::RemovePlaylist(remove_id.clone()))));
            }),
        ));
        let item = Stack::bar(6.0).align(AlignSpec::Center).padding(4.0).radius(RadiusTier::Md);
        let item = if active { item.surface(SemanticColorRole::AccentSoft) } else { item };
        rows.push(widget(item).children((main, actions)).key(format!("playlist-item-{id}")).into_any());
    }
    widget(Stack::column(6.0)).children(rows).key("playlist-list").into_any()
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
pub fn sidebar_footer(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let settings = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
    let extensions = !settings && model.workspace.panel == WorkspacePanel::Extensions;
    let logs = !settings && model.workspace.panel == WorkspacePanel::Logs;
    let rest = model.motion.footer_opacity();
    let tasks_open = model.admin.popover_open;
    let task_count = model.active_tasks;
    let task_button = FooterButton { icon: CLIPBOARD_LIST, label: "任务", active: tasks_open, highlight: task_count > 0, rest_opacity: rest }
        .view("footer-tasks", |cx| cx.dispatch_program(ShellMessage::Admin(super::admin::AdminMessage::ToggleTaskPopover)));
    let task = if task_count > 0 { with_badge(task_button, task_count) } else { task_button };
    widget(Stack::row(2.0).align(AlignSpec::Center)).key("sidebar-footer").children((
        FooterButton { icon: SETTINGS, label: "设置", active: settings, highlight: false, rest_opacity: rest }
            .view("footer-settings", |cx| cx.dispatch_program(ShellMessage::Navigate(ShellPage::Settings))),
        FooterButton { icon: PUZZLE, label: "拓展", active: extensions, highlight: false, rest_opacity: rest }
            .view("footer-extensions", |cx| cx.dispatch_program(ShellMessage::SetWorkspacePanel(WorkspacePanel::Extensions))),
        task,
        FooterButton { icon: LOGS, label: "日志", active: logs, highlight: false, rest_opacity: rest }
            .view("footer-logs", |cx| cx.dispatch_program(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs))),
    ))
}

/// 任务入口右上角的数量角标。对应 `.task-button__badge`：12px 高、强调色底、9px 粗体。
fn with_badge(button: AnyView, count: usize) -> AnyView {
    let badge = widget(
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
    .children((widget(parts::label_text(count.to_string(), 9.0, 700, Some(SemanticColorRole::AccentText)).line_height(12.0)),))
    .key("footer-task-badge");
    widget(Stack::row(0.0).with_layout(|layout| layout.position = nana_ui_core::PositionSpec::Relative))
        .children((button, badge))
        .into_any()
}

/// 加载中的刷新按钮换成转圈图标。
pub(super) fn refresh_icon(loading: bool) -> Icon {
    if loading { LOADER_2 } else { REFRESH }
}

pub(super) fn sidebar_message(message: SidebarMessage) -> ShellMessage {
    ShellMessage::Sidebar(message)
}
