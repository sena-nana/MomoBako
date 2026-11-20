//! 侧栏消息归约与快捷方式计数、路径和 ID 辅助函数。

use super::*;

/// 快捷方式计数。路径不含 `/` 记为未分类，没有标签记为未标签，有访问时间记为最近使用。
pub(super) fn count_shortcuts(assets: &[ShortcutAsset], trash_count: i64) -> ShortcutCounts {
    let mut counts = ShortcutCounts {
        trash: usize::try_from(trash_count).unwrap_or(0),
        ..ShortcutCounts::default()
    };
    for asset in assets.iter().filter(|asset| !asset.deleted) {
        counts.all += 1;
        if !asset.path.contains('/') {
            counts.uncategorized += 1;
        }
        if asset.untagged {
            counts.untagged += 1;
        }
        if asset.accessed {
            counts.recent += 1;
        }
    }
    counts
}

/// 把仓库路径收成 `/` 分隔、去掉首尾斜杠的相对路径。
pub(super) fn normalize_workspace_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

/// 仓库相对路径的父目录。根上的文件返回空路径。
pub fn parent_path(path: &str) -> String {
    let normalized = normalize_workspace_path(path);
    match normalized.rfind('/') {
        Some(index) => normalized[..index].to_string(),
        None => String::new(),
    }
}

pub(super) fn toggle_id(ids: &mut Vec<String>, id: &str) {
    if let Some(index) = ids.iter().position(|item| item == id) {
        ids.remove(index);
    } else {
        ids.push(id.to_string());
    }
}

pub(super) fn folder_paths(nodes: &[SidebarFolder]) -> Vec<String> {
    let mut paths = Vec::new();
    fn walk(nodes: &[SidebarFolder], paths: &mut Vec<String>) {
        for node in nodes {
            paths.push(node.path.clone());
            walk(&node.children, paths);
        }
    }
    walk(nodes, &mut paths);
    paths
}

pub(super) fn smart_ids(nodes: &[SidebarSmartFolder]) -> Vec<String> {
    let mut ids = Vec::new();
    fn walk(nodes: &[SidebarSmartFolder], ids: &mut Vec<String>) {
        for node in nodes {
            ids.push(node.id.clone());
            walk(&node.children, ids);
        }
    }
    walk(nodes, &mut ids);
    ids
}

/// 展开当前智能文件夹的祖先，不展开它自己。
pub(super) fn smart_ancestor_ids(nodes: &[SidebarSmartFolder], id: &str) -> Vec<String> {
    fn find(nodes: &[SidebarSmartFolder], id: &str, trail: &mut Vec<String>) -> bool {
        for node in nodes {
            if node.id == id {
                return true;
            }
            trail.push(node.id.clone());
            if find(&node.children, id, trail) {
                return true;
            }
            trail.pop();
        }
        false
    }
    let mut trail = Vec::new();
    if find(nodes, id, &mut trail) {
        trail
    } else {
        Vec::new()
    }
}

/// 处理侧栏消息。其它消息原样返回，交给壳层的其余归约。
pub(in super::super) fn reduce_message(
    model: &mut super::super::ShellViewModel,
    message: super::super::ShellMessage,
) -> Option<super::super::ShellMessage> {
    let super::super::ShellMessage::Sidebar(message) = message else {
        return Some(message);
    };
    match message {
        SidebarMessage::SelectShortcut(id) => model.apply_shortcut(id),
        SidebarMessage::OpenQuickAccess(id) => model.apply_quick_access(id),
        SidebarMessage::ToggleFolder(path) => model.sidebar.toggle_folder(&path),
        SidebarMessage::OpenFolder(path) => model.apply_open_folder(path),
        SidebarMessage::RefreshFolderTree => super::super::tree_sync::begin(model),
        SidebarMessage::ToggleSmartFolder(id) => model.sidebar.toggle_smart_folder(&id),
        SidebarMessage::OpenSmartFolder(id) => model.apply_smart_folder(id),
        SidebarMessage::OpenSmartFolderDialog => {
            if model.navigation_locked() || model.workspace.active_repo_id.is_none() {
                eprintln!("Nana 当前不能新建智能文件夹");
            } else {
                model.sidebar.open_smart_dialog(None);
            }
        }
        SidebarMessage::OpenSmartFolderChild(parent_id) => {
            if model.navigation_locked() || model.workspace.active_repo_id.is_none() {
                eprintln!("Nana 当前不能新建子智能文件夹");
            } else {
                model.sidebar.open_smart_dialog(Some(parent_id));
            }
        }
        SidebarMessage::CloseSmartFolderDialog => model.sidebar.close_smart_dialog(),
        SidebarMessage::SetSmartFolderField { field, value } => {
            model.sidebar.set_smart_field(field, value)
        }
        SidebarMessage::SubmitSmartFolder => {
            model
                .sidebar
                .submit_smart_folder(model.workspace.active_repo_id.as_deref());
        }
        SidebarMessage::SmartFolderSaved { repo_id, result } => {
            model.apply_smart_saved(&repo_id, result)
        }
        SidebarMessage::TogglePlaylists => model.sidebar.toggle_playlists(),
        SidebarMessage::OpenSidebarPlaylist(id) => model.apply_playlist(id),
        SidebarMessage::OpenRepositorySwitcher => model.sidebar.open_switcher(),
        SidebarMessage::ShowRepositoryAddMenu => {
            model.sidebar.show_add_menu();
        }
        SidebarMessage::SelectRepositoryBackend(plugin_id) => {
            model.apply_repository_backend(&plugin_id)
        }
        SidebarMessage::BackToAddMenu => model.sidebar.back_to_add_menu(),
        SidebarMessage::CloseRepositoryPopover => {
            model.sidebar.close_popover();
        }
        SidebarMessage::SelectRepositoryFromSwitcher(repo_id) => {
            if model.sidebar.select_from_switcher(&repo_id) {
                model.workspace.select_repository(&repo_id);
                model.repository_id = model.workspace.active_repo_id.clone();
                if let Some(repository) = model.workspace.active_repository() {
                    model.repository_name = repository.name.clone();
                }
                model.bind_sidebar_repository();
                model.leave_settings_page();
            }
        }
        SidebarMessage::DeleteRepositoryFromSwitcher => {
            if model
                .sidebar
                .delete_from_switcher(model.workspace.active_repo_id.as_deref())
            {
                model.workspace.open_delete_dialog();
                model.leave_settings_page();
            }
        }
        SidebarMessage::RepositoryAttachPathChanged(path) => model.sidebar.set_attach_path(path),
        SidebarMessage::SubmitRepositoryAttach => {
            model.sidebar.submit_attach();
        }
        // 附加或新建资源库：成功以后把新仓库放进列表，再切过去。
        SidebarMessage::RepositoryAttachFinished(result) => {
            let created = result.as_ref().ok().cloned();
            model.sidebar.note_attach_finished(result.map(|_| ()));
            if let Some(summary) = created {
                model.workspace.upsert_repository(&summary);
                model.select_workspace_repository(&summary.repo_id);
            }
        }
        SidebarMessage::SidebarTreeLoaded { repo_id, result } => {
            model.sidebar.apply_tree(&repo_id, result)
        }
        SidebarMessage::SidebarSmartFoldersLoaded { repo_id, result } => {
            model.sidebar.apply_smart_folders(&repo_id, result)
        }
        SidebarMessage::SidebarSmartFolderQueried {
            repo_id,
            smart_folder_id,
            result,
        } => {
            model.apply_smart_query(&repo_id, &smart_folder_id, result);
        }
        SidebarMessage::SidebarPlaylistsLoaded {
            repo_id,
            result: Ok(playlists),
        } => {
            model.apply_playlist_list(&repo_id, &playlists, None);
        }
        SidebarMessage::SidebarPlaylistsLoaded {
            repo_id,
            result: Err(error),
        } => {
            model
                .sidebar
                .apply_playlists(&mut model.workspace, &repo_id, Err(error));
        }
        SidebarMessage::Gap(message) => gap::reduce(model, message),
        SidebarMessage::ClearRecent => model.clear_recent_access(),
        SidebarMessage::RecentCleared { repo_id, result } => {
            model.note_recent_cleared(&repo_id, result)
        }
    }
    None
}
