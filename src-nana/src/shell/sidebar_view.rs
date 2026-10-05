//! 侧栏视图。快捷方式和播放集用 `SidebarRow`，目录与智能文件夹用 `TreeView`。

use std::sync::Arc;

use nana_ui::icons_tabler::{ARCHIVE, CLOCK, CLIPBOARD_LIST, FOLDERS, LOGS, PLUS, PUZZLE, REFRESH, SETTINGS, TAG, TRASH};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Icon, IconButton, IconGlyph, LengthSpec, SemanticColorRole, SidebarRow, SidebarRowState,
    Stack, Text, TextChanged, TextInput, TreeNode, TreeView, TreeViewEvent, ViewContext,
};

use super::files::{FileDialog, FilesMessage};
use super::sidebar::{PopoverMode, ShortcutId};
use super::SidebarMessage;
use super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

/// 仓库切换按钮，放在侧栏顶部。
pub fn sidebar_switcher(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let name = model
        .workspace
        .active_repository()
        .map(|repository| repository.name.clone())
        .unwrap_or_else(|| "无资源库".into());
    widget(SidebarRow::new(format!("资源库 · {name}"))).on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::OpenRepositorySwitcher));
    })
}

/// 快捷方式、目录树、智能文件夹和播放集。
pub fn sidebar_sections(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let locked = model.navigation_locked();
    let mut rows = Vec::new();
    rows.push(section_label("快捷方式"));
    for shortcut in shortcut_rows(model, locked) {
        rows.push(shortcut);
    }
    if !model.sidebar.quick_access.is_empty() {
        rows.push(section_label("快捷访问"));
        for shortcut in model.sidebar.quick_access.clone() {
            let id = shortcut.id.clone();
            rows.push(sidebar_row(&shortcut.label, false, locked, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenQuickAccess(id.clone())));
            }));
        }
    }
    let folder_locked = locked || model.workspace.active_repo_id.is_none() || model.workspace.panel == WorkspacePanel::Trash;
    let refresh_locked = locked || model.workspace.active_repo_id.is_none() || model.sidebar.tree_loading;
    let refresh_label = if model.sidebar.tree_loading { "正在刷新文件夹树" } else { "刷新文件夹树" };
    rows.push(section_bar(
        section_label("文件夹"),
        vec![
            tree_action(PLUS, "在当前目录新建文件夹", "folder-create", folder_locked, || {
                ShellMessage::Files(FilesMessage::OpenDialog(FileDialog::CreateDirectory))
            }),
            tree_action(REFRESH, refresh_label, "refresh-folder-tree", refresh_locked, || {
                ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree)
            }),
        ],
    ));
    rows.push(folder_body(model, locked).into_any());
    if !model.sidebar.tree_error.is_empty() {
        rows.push(hint(&model.sidebar.tree_error, "folder-tree-error"));
    }
    rows.push(section_label("智能文件夹"));
    rows.push(smart_body(model, locked).into_any());
    if !model.sidebar.smart_error.is_empty() {
        rows.push(hint(&model.sidebar.smart_error, "smart-folder-error"));
    }
    let create_locked = locked || model.workspace.active_repo_id.is_none() || model.playlist_players.is_empty();
    let playlist_count = model.sidebar.playlists.len();
    rows.push(section_bar(
        widget(Stack::row(6.0).grow(0.0).shrink(1.0)).children((
            button("播放集")
                .key("playlist-toggle")
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(sidebar_message(SidebarMessage::TogglePlaylists))),
            hint(&playlist_count.to_string(), "playlist-count"),
        )).into_any(),
        vec![tree_action(PLUS, "新建播放集", "playlist-create", create_locked, || ShellMessage::OpenPlaylistDialog)],
    ));
    rows.push(playlist_body(model, locked).into_any());
    widget(Stack::fill_column(10.0).padding_xy(12.0, 8.0)).children(rows)
}

/// 设置、拓展、任务和日志。有运行中的任务时，任务按钮读出数量。
pub fn sidebar_footer(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let settings = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
    let extensions = !settings && model.workspace.panel == WorkspacePanel::Extensions;
    let logs = !settings && model.workspace.panel == WorkspacePanel::Logs;
    let task_label = if model.active_tasks > 0 {
        format!("任务 {}", model.active_tasks)
    } else {
        "任务".into()
    };
    widget(Stack::row(2.0)).children((
        footer_icon(SETTINGS, "设置".into(), settings, || ShellMessage::Navigate(ShellPage::Settings)),
        footer_icon(PUZZLE, "拓展".into(), extensions, || ShellMessage::SetWorkspacePanel(WorkspacePanel::Extensions)),
        footer_icon(CLIPBOARD_LIST, task_label, model.admin.popover_open, || {
            ShellMessage::Admin(super::admin::AdminMessage::ToggleTaskPopover)
        }),
        footer_icon(LOGS, "日志".into(), logs, || ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs)),
    ))
}

/// 仓库切换和附加本地文件夹。系统文件夹对话框尚未接通，路径在弹层里提交。
pub fn repository_popover(model: &ShellViewModel) -> Option<impl IntoView + use<'_>> {
    if model.sidebar.popover == PopoverMode::Closed {
        return None;
    }
    let submitting = model.sidebar.submitting;
    let error = (!model.sidebar.popover_error.is_empty()).then(|| text(model.sidebar.popover_error.clone()).key("repository-popover-error"));
    let body = if model.sidebar.popover == PopoverMode::Switcher {
        let mut rows = Vec::new();
        for repository in &model.workspace.repositories {
            let repo_id = repository.repo_id.clone();
            let active = model.workspace.active_repo_id.as_deref() == Some(repository.repo_id.as_str());
            rows.push(sidebar_row(&repository.name, active, submitting, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::SelectRepositoryFromSwitcher(repo_id.clone())));
            }).into_any());
        }
        rows.push(sidebar_row("添加资源库", false, submitting, |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::ShowRepositoryAddMenu));
        }).into_any());
        if model.workspace.active_repo_id.is_some() {
            rows.push(sidebar_row("删除当前资源库", false, submitting, |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::DeleteRepositoryFromSwitcher));
            }).into_any());
        }
        widget(Stack::column(6.0)).children(rows).into_any()
    } else {
        let path = model.sidebar.attach_path.clone();
        widget(Stack::column(8.0)).children((
            text("添加资源库").key("add-repository-title"),
            text("系统文件夹对话框尚未接通，可在这里填写本地文件夹。").key("add-repository-hint"),
            widget(TextInput::new(path).label("资源库文件夹")).on_cx(|_, event: &TextChanged, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::RepositoryAttachPathChanged(event.value.to_string())));
            }),
            button(if submitting { "正在附加…" } else { "附加资源库" })
                .key("attach-repository")
                .disabled(submitting)
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(sidebar_message(SidebarMessage::SubmitRepositoryAttach))),
            button("返回列表").key("repository-add-back").disabled(submitting).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenRepositorySwitcher));
            }),
        )).into_any()
    };
    Some(widget(Stack::fill_column(12.0).padding_xy(16.0, 16.0)).children((
        text("资源库").key("repository-popover-title"),
        body,
        error,
        button("关闭").key("repository-popover-close").disabled(submitting).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::CloseRepositoryPopover));
        }),
    )))
}

fn section_label(title: &'static str) -> AnyView {
    widget(Text::new(title).color(SemanticColorRole::Faint).font_size(11.0).font_weight(700)).into_any()
}

fn section_bar(title: AnyView, actions: Vec<AnyView>) -> AnyView {
    widget(Stack::bar(4.0).align(AlignSpec::Center).min_height(LengthSpec::Px(24.0)))
        .children((
            title,
            widget(Stack::spacer()),
            widget(Stack::row(2.0).grow(0.0).shrink(0.0)).children(actions),
        ))
        .into_any()
}

fn hint(copy: &str, key: impl Into<String>) -> AnyView {
    widget(Text::new(copy).color(SemanticColorRole::Muted).font_size(12.0)).key(key.into()).into_any()
}

/// 分组标题右侧的 22px 图标。文字只作无障碍名称。
fn tree_action(
    icon: Icon,
    label: &'static str,
    key: &'static str,
    disabled: bool,
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    let mut button = IconButton::new(icon, label).disabled(disabled);
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(22.0));
    layout.height = Some(LengthSpec::Px(22.0));
    layout.min_width = Some(LengthSpec::Px(22.0));
    layout.min_height = Some(LengthSpec::Px(22.0));
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    widget(button).key(key).on_cx(move |_, _: &Activate, cx| cx.dispatch_program(message())).into_any()
}

fn footer_icon(
    icon: Icon,
    label: String,
    selected: bool,
    message: impl Fn() -> ShellMessage + Send + 'static,
) -> AnyView {
    let mut button = super::title_bar::shell_icon(icon, label, false).selected(selected);
    let layout = Arc::make_mut(&mut button.style.layout);
    layout.width = Some(LengthSpec::Px(26.0));
    layout.height = Some(LengthSpec::Px(26.0));
    layout.min_width = Some(LengthSpec::Px(26.0));
    layout.min_height = Some(LengthSpec::Px(26.0));
    widget(button).on_cx(move |_, _: &Activate, cx| cx.dispatch_program(message())).into_any()
}

fn shortcut_rows(model: &ShellViewModel, locked: bool) -> Vec<AnyView> {
    let counts = model.sidebar.counts;
    let items = [
        (ShortcutId::All, counts.all, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::All),
        (ShortcutId::Uncategorized, counts.uncategorized, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::Uncategorized),
        (ShortcutId::Untagged, counts.untagged, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::Untagged),
        (ShortcutId::Recent, counts.recent, model.workspace.panel == WorkspacePanel::Files && model.workspace.library_category == super::LibraryCategory::Recent),
        (ShortcutId::Trash, counts.trash, model.workspace.panel == WorkspacePanel::Trash),
    ];
    items.into_iter().map(|(id, count, active)| {
        let row = sidebar_row_with(Some(shortcut_icon(id)), id.label(), None, active, locked, move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::SelectShortcut(id)));
        });
        widget(Stack::bar(4.0).align(AlignSpec::Center)).children((
            widget(Stack::fill_row(0.0).grow(1.0).shrink(1.0).min_width(LengthSpec::Px(0.0))).children((row,)),
            widget(Text::new(count.to_string()).color(SemanticColorRole::Muted).font_size(12.0).font_weight(600)),
        )).into_any()
    }).collect()
}

fn shortcut_icon(id: ShortcutId) -> Icon {
    match id {
        ShortcutId::All => ARCHIVE,
        ShortcutId::Uncategorized => FOLDERS,
        ShortcutId::Untagged => TAG,
        ShortcutId::Recent => CLOCK,
        ShortcutId::Trash => TRASH,
    }
}

fn folder_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if model.workspace.active_repo_id.is_none() {
        return hint("先选择或添加一个资源库。", "folder-empty");
    }
    if locked {
        return hint("资源库文件夹丢失，请先在主视图修复。", "folder-missing");
    }
    if model.workspace.panel == WorkspacePanel::Trash || model.sidebar.browsing_trash {
        return hint("回收站条目在主视图中管理。", "folder-trash");
    }
    if model.sidebar.folders.is_empty() && !model.sidebar.tree_loading {
        return hint("当前仓库还没有子文件夹。", "folder-none");
    }
    let nodes = folder_nodes(&model.sidebar.folders, &model.sidebar.expanded_folders, &model.sidebar.current_directory);
    widget(TreeView::new(nodes)).on_cx(|_, event: &TreeViewEvent<Arc<str>>, cx| match event {
        TreeViewEvent::Toggle(id) => cx.dispatch_program(sidebar_message(SidebarMessage::ToggleFolder(id.to_string()))),
        TreeViewEvent::Select(id) => cx.dispatch_program(sidebar_message(SidebarMessage::OpenFolder(id.to_string()))),
    }).into_any()
}

fn smart_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if model.workspace.active_repo_id.is_none() {
        return hint("先选择或添加一个资源库。", "smart-empty");
    }
    if locked {
        return hint("资源库修复后可继续使用智能文件夹。", "smart-missing");
    }
    if model.sidebar.smart_folders.is_empty() {
        return hint("还没有智能文件夹。", "smart-none");
    }
    let active = model.sidebar.active_smart_folder_id.clone();
    let nodes = smart_nodes(&model.sidebar.smart_folders, &model.sidebar.expanded_smart_folders, active.as_deref());
    widget(TreeView::new(nodes)).on_cx(|_, event: &TreeViewEvent<Arc<str>>, cx| match event {
        TreeViewEvent::Toggle(id) => cx.dispatch_program(sidebar_message(SidebarMessage::ToggleSmartFolder(id.to_string()))),
        TreeViewEvent::Select(id) => cx.dispatch_program(sidebar_message(SidebarMessage::OpenSmartFolder(id.to_string()))),
    }).into_any()
}

fn playlist_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if !model.sidebar.playlists_expanded {
        return widget(Stack::column(0.0)).into_any();
    }
    if locked {
        return hint("资源库修复后可继续使用播放集。", "playlist-missing");
    }
    if model.workspace.active_repo_id.is_none() {
        return hint("先选择或添加一个资源库。", "playlist-empty");
    }
    let mut rows = Vec::new();
    for playlist in &model.sidebar.playlists {
        let id = playlist.id.clone();
        let active = model.workspace.panel == WorkspacePanel::Playlist && model.sidebar.active_playlist_id.as_deref() == Some(playlist.id.as_str());
        let subtitle = format!("{} · {} 项", playlist.player_label, playlist.item_count);
        rows.push(widget(Stack::column(0.0)).children((
            sidebar_row(&playlist.name, active, false, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenSidebarPlaylist(id.clone())));
            }),
            hint(&subtitle, format!("playlist-meta-{}", playlist.id)),
        )).into_any());
    }
    widget(Stack::column(6.0)).children(rows).into_any()
}

fn sidebar_row(
    label: impl AsRef<str>,
    active: bool,
    disabled: bool,
    on_activate: impl Fn(&mut ViewContext<SidebarRow>) + Send + 'static,
) -> AnyView {
    sidebar_row_with(None, label, None, active, disabled, on_activate)
}

fn sidebar_row_with(
    icon: Option<Icon>,
    label: impl AsRef<str>,
    trailing: Option<String>,
    active: bool,
    disabled: bool,
    on_activate: impl Fn(&mut ViewContext<SidebarRow>) + Send + 'static,
) -> AnyView {
    let state = if disabled {
        SidebarRowState::Disabled
    } else if active {
        SidebarRowState::Active
    } else {
        SidebarRowState::Idle
    };
    let label: Arc<str> = Arc::from(label.as_ref());
    let mut row = widget(SidebarRow::new(label).state(state));
    if let Some(icon) = icon {
        row = row.leading(widget(IconGlyph::new(icon).size(14.0)));
    }
    if let Some(trailing) = trailing {
        row = row.trailing(text(trailing));
    }
    row.on_cx(move |_, _: &Activate, cx| on_activate(cx)).into_any()
}

fn sidebar_message(message: SidebarMessage) -> ShellMessage {
    ShellMessage::Sidebar(message)
}

fn folder_nodes(folders: &[super::sidebar::SidebarFolder], expanded: &[String], current: &str) -> Vec<TreeNode<Arc<str>>> {
    folders.iter().map(|folder| {
        let id: Arc<str> = Arc::from(folder.path.as_str());
        let open = expanded.iter().any(|path| path == &folder.path);
        let mut node = if folder.children.is_empty() {
            TreeNode::leaf(id, folder.label.clone())
        } else {
            TreeNode::branch(id, folder.label.clone(), open, folder_nodes(&folder.children, expanded, current))
        };
        node.selected = folder.path == current;
        node
    }).collect()
}

fn smart_nodes(folders: &[super::sidebar::SidebarSmartFolder], expanded: &[String], active: Option<&str>) -> Vec<TreeNode<Arc<str>>> {
    folders.iter().map(|folder| {
        let id: Arc<str> = Arc::from(folder.id.as_str());
        let open = expanded.iter().any(|item| item == &folder.id);
        let mut node = if folder.children.is_empty() {
            TreeNode::leaf(id, folder.name.clone())
        } else {
            TreeNode::branch(id, folder.name.clone(), open, smart_nodes(&folder.children, expanded, active))
        };
        node.selected = active == Some(folder.id.as_str());
        node
    }).collect()
}
