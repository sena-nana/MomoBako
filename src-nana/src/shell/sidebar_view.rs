//! 侧栏视图。快捷方式和播放集用 `SidebarRow`，目录与智能文件夹用 `TreeView`。

use std::sync::Arc;

use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, SidebarRow, SidebarRowState, Stack, TextChanged, TextInput, TreeNode, TreeView,
    TreeViewEvent, ViewContext,
};

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
    rows.push(text("快捷方式").into_any());
    for shortcut in shortcut_rows(model, locked) {
        rows.push(shortcut.into_any());
    }
    if !model.sidebar.quick_access.is_empty() {
        rows.push(text("快捷访问").into_any());
        for shortcut in model.sidebar.quick_access.clone() {
            let id = shortcut.id.clone();
            rows.push(sidebar_row(&shortcut.label, false, locked, move |cx| {
                cx.dispatch_program(sidebar_message(SidebarMessage::OpenQuickAccess(id.clone())));
            }).into_any());
        }
    }
    rows.push(text("文件夹").into_any());
    let refresh_locked = locked || model.workspace.active_repo_id.is_none() || model.sidebar.tree_loading;
    rows.push(
        button(if model.sidebar.tree_loading { "正在刷新文件夹树" } else { "刷新文件夹树" })
            .key("refresh-folder-tree")
            .disabled(refresh_locked)
            .on_cx(|_, _: &Activate, cx| cx.dispatch_program(sidebar_message(SidebarMessage::RefreshFolderTree)))
            .into_any(),
    );
    rows.push(folder_body(model, locked).into_any());
    if !model.sidebar.tree_error.is_empty() {
        rows.push(text(model.sidebar.tree_error.clone()).key("folder-tree-error").into_any());
    }
    rows.push(text("智能文件夹").into_any());
    rows.push(smart_body(model, locked).into_any());
    if !model.sidebar.smart_error.is_empty() {
        rows.push(text(model.sidebar.smart_error.clone()).key("smart-folder-error").into_any());
    }
    let playlist_label = if model.sidebar.playlists_expanded { "收起播放集" } else { "展开播放集" };
    rows.push(sidebar_row(playlist_label, false, false, |cx| {
        cx.dispatch_program(sidebar_message(SidebarMessage::TogglePlaylists));
    }).into_any());
    rows.push(playlist_body(model, locked).into_any());
    widget(Stack::fill_column(8.0).padding_xy(12.0, 8.0)).children(rows)
}

/// 设置、拓展和日志仍留在侧栏底部。
pub fn sidebar_footer(model: &ShellViewModel) -> impl IntoView + use<'_> {
    let settings_active = matches!(model.page, ShellPage::Settings | ShellPage::SettingsError);
    let extensions_active = model.workspace.panel == WorkspacePanel::Extensions && !settings_active;
    let logs_active = model.workspace.panel == WorkspacePanel::Logs && !settings_active;
    widget(Stack::column(4.0)).children((
        sidebar_row("设置", settings_active, false, |cx| {
            cx.dispatch_program(ShellMessage::Navigate(ShellPage::Settings));
        }),
        sidebar_row("拓展", extensions_active, false, |cx| {
            cx.dispatch_program(ShellMessage::SetWorkspacePanel(WorkspacePanel::Extensions));
        }),
        sidebar_row("日志", logs_active, false, |cx| {
            cx.dispatch_program(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
        }),
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
        sidebar_row(format!("{} · {count}", id.label()), active, locked, move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::SelectShortcut(id)));
        })
    }).collect()
}

fn folder_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if model.workspace.active_repo_id.is_none() {
        return text("先选择或添加一个资源库。").key("folder-empty").into_any();
    }
    if locked {
        return text("资源库文件夹丢失，请先在主视图修复。").key("folder-missing").into_any();
    }
    if model.workspace.panel == WorkspacePanel::Trash || model.sidebar.browsing_trash {
        return text("回收站条目在主视图中管理。").key("folder-trash").into_any();
    }
    if model.sidebar.folders.is_empty() && !model.sidebar.tree_loading {
        return text("当前仓库还没有子文件夹。").key("folder-none").into_any();
    }
    let nodes = folder_nodes(&model.sidebar.folders, &model.sidebar.expanded_folders, &model.sidebar.current_directory);
    widget(TreeView::new(nodes)).on_cx(|_, event: &TreeViewEvent<Arc<str>>, cx| match event {
        TreeViewEvent::Toggle(id) => cx.dispatch_program(sidebar_message(SidebarMessage::ToggleFolder(id.to_string()))),
        TreeViewEvent::Select(id) => cx.dispatch_program(sidebar_message(SidebarMessage::OpenFolder(id.to_string()))),
    }).into_any()
}

fn smart_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if model.workspace.active_repo_id.is_none() {
        return text("先选择或添加一个资源库。").key("smart-empty").into_any();
    }
    if locked {
        return text("资源库修复后可继续使用智能文件夹。").key("smart-missing").into_any();
    }
    if model.sidebar.smart_folders.is_empty() {
        return text("还没有智能文件夹。").key("smart-none").into_any();
    }
    let active = model.sidebar.active_smart_folder_id.clone();
    let nodes = smart_nodes(&model.sidebar.smart_folders, &model.sidebar.expanded_smart_folders, active.as_deref());
    widget(TreeView::new(nodes)).on_cx(|_, event: &TreeViewEvent<Arc<str>>, cx| match event {
        TreeViewEvent::Toggle(id) => cx.dispatch_program(sidebar_message(SidebarMessage::ToggleSmartFolder(id.to_string()))),
        TreeViewEvent::Select(id) => cx.dispatch_program(sidebar_message(SidebarMessage::OpenSmartFolder(id.to_string()))),
    }).into_any()
}

fn playlist_body(model: &ShellViewModel, locked: bool) -> impl IntoView + use<'_> {
    if locked {
        return text("资源库修复后可继续使用播放集。").key("playlist-missing").into_any();
    }
    if model.sidebar.playlists_expanded && model.workspace.active_repo_id.is_none() {
        return text("先选择或添加一个资源库。").key("playlist-empty").into_any();
    }
    let mut rows = Vec::new();
    for playlist in &model.sidebar.playlists {
        let id = playlist.id.clone();
        let active = model.workspace.panel == WorkspacePanel::Playlist && model.sidebar.active_playlist_id.as_deref() == Some(playlist.id.as_str());
        let label = if model.sidebar.playlists_expanded {
            format!("{} · {} · {} 项", playlist.name, playlist.player_label, playlist.item_count)
        } else {
            playlist.name.clone()
        };
        rows.push(sidebar_row(&label, active, false, move |cx| {
            cx.dispatch_program(sidebar_message(SidebarMessage::OpenSidebarPlaylist(id.clone())));
        }).into_any());
    }
    widget(Stack::column(4.0)).children(rows).into_any()
}

fn sidebar_row(
    label: impl AsRef<str>,
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
    widget(SidebarRow::new(label).state(state)).on_cx(move |_, _: &Activate, cx| on_activate(cx)).into_any()
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
