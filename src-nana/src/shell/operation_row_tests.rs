//! 文件变更在任务弹层里的仓库操作行：从文件页的操作开始，看任务弹层的行和侧栏「任务」的计数。
//! 删除和回收站变更成功以后，侧栏「回收站」计数跟着静默重读的仓库摘要走。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    AssetSummary, FileBrowserEntry, FileBrowserSnapshot, FileTreeNode, RepositoryBackendSummary,
    RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
};

use super::files::{FileDialog, FilesMessage, ProtocolFolderTree};
use super::repo_operation::{CLIMB_INTERVAL_MS, FINISH_LINGER_MS};
use super::workspace_dialogs::HardlinkView;
use super::workspace_refresh::SilentMessage;
use super::{
    FilesEffect, HardlinkPrompt, LibraryCategory, ShellMessage, ShellPage, ShellViewModel,
    WorkspaceEffect, WorkspacePanel,
};

mod hardlinks;
mod operations;
mod sidebar_tree;

/// 列表模式的文件页：assets 文件夹、cover.png 和 notes/page.pdf，资源库可写。
fn files_page() -> ShellViewModel {
    super::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "files-list-mode")
        .map(|(_, model)| model)
        .expect("列表模式的文件页场景")
}

fn files(model: &mut ShellViewModel, message: FilesMessage) {
    model.reduce(ShellMessage::Files(message));
}

/// 任务弹层里的仓库操作行：(种类, 阶段, 百分比)。
fn operation_row(model: &ShellViewModel) -> Option<(String, String, f64)> {
    model
        .task_rows()
        .into_iter()
        .find(|row| row.id == "workspace-operation")
        .map(|row| (row.label, row.detail, row.value))
}

/// 当前目录重读回来的结果。
fn reloaded(model: &ShellViewModel) -> ShellMessage {
    reloaded_at(model, model.files.current_path.clone())
}

/// 复制后的宿主重读请求目标目录；ViewModel 在快照被接受前仍保留旧目录。
fn reloaded_target(model: &ShellViewModel) -> ShellMessage {
    let path = model
        .files
        .pending_path()
        .map(str::to_string)
        .unwrap_or_else(|| model.files.current_path.clone());
    reloaded_at(model, path)
}

fn reloaded_at(model: &ShellViewModel, current_path: String) -> ShellMessage {
    ShellMessage::FileBrowserLoaded(Ok(FileBrowserSnapshot {
        repo_id: model.workspace.active_repo_id.clone().expect("活动仓库"),
        root_path: "C:/repo".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path,
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries: Vec::new(),
    }))
}

/// 启动后读到一条硬链接候选的文件页（inbox/cover.png 与 cover.png 哈希一致），之前排下的请求清掉。
fn hardlink_page() -> (ShellViewModel, String) {
    let mut model = super::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "hardlink-dialog")
        .map(|(_, model)| model)
        .expect("硬链接确认场景");
    let repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
    let _ = model.files.take_effects();
    let _ = model.workspace.take_effects();
    (model, repo_id)
}

fn protocol_tree(path: &str, nodes: Vec<FileTreeNode>) -> Option<ProtocolFolderTree> {
    Some(ProtocolFolderTree {
        repo_id: "acceptance-repo".into(),
        current_path: path.into(),
        trash: false,
        nodes,
    })
}

fn finished(folder_tree: Option<ProtocolFolderTree>) -> FilesMessage {
    FilesMessage::ProtocolFinished {
        result: Ok(()),
        reload: true,
        hardlinks: false,
        folder_tree,
    }
}

fn named_file(path: &str, name: &str, kind: &str) -> FileBrowserEntry {
    let mut entry = browser_file(path);
    entry.name = name.into();
    entry.kind = kind.into();
    entry
}

/// 侧栏目录树的路径，父级在子级前面。
fn folder_paths(model: &ShellViewModel) -> Vec<String> {
    fn walk(folders: &[super::SidebarFolder], out: &mut Vec<String>) {
        for folder in folders {
            out.push(folder.path.clone());
            walk(&folder.children, out);
        }
    }
    let mut out = Vec::new();
    walk(&model.sidebar.folders, &mut out);
    out
}

fn tree_node(path: &str, file_count: usize, children: Vec<FileTreeNode>) -> FileTreeNode {
    FileTreeNode {
        path: path.into(),
        label: path.rsplit('/').next().unwrap_or(path).into(),
        file_count,
        children,
    }
}

fn browser_of(model: &ShellViewModel) -> FileBrowserSnapshot {
    match reloaded(model) {
        ShellMessage::FileBrowserLoaded(Ok(browser)) => browser,
        _ => unreachable!("重读目录的消息"),
    }
}

/// 测试里的回收站文件行。只填路径和名字，其余保持空。
fn browser_file(path: &str) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: path.into(),
        kind: "file".into(),
        extension: None,
        size_bytes: None,
        size_label: None,
        modified_at: None,
        asset_id: None,
        status: None,
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

/// 测试用的仓库摘要：只改回收站计数，素材列表空着，分类计数跟着这份摘要走。
fn snapshot_with_trash(repo_id: &str, trash_count: i64) -> RepositorySnapshot {
    RepositorySnapshot {
        repository: RepositorySummary {
            repo_id: repo_id.into(),
            name: "默认资源库".into(),
            path: "C:/acceptance".into(),
            backend: RepositoryBackendSummary {
                plugin_id: "momobako.source.local-filesystem".into(),
                kind: "local-filesystem".into(),
                name: "本地文件系统".into(),
                capabilities: vec!["write".into()],
            },
            status: "ready".into(),
            asset_count: 0,
            updated_at: String::new(),
            local_cache: None,
            authentication: None,
        },
        folder_label: String::new(),
        folders: Vec::new(),
        assets: Vec::<AssetSummary>::new(),
        playlists: Vec::new(),
        quick_access: Vec::new(),
        tag_groups: Vec::new(),
        metadata_fields: Vec::new(),
        recent_revision_count: 0,
        overview: RepositoryOverview {
            total_size_bytes: 0,
            total_size_label: "0 B".into(),
            file_count: 0,
            folder_count: 0,
            trash_count,
            readme_content: None,
        },
    }
}

mod folder_tree_reread_tests;
