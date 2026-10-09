//! 文件夹树「刷新」的回归：照 Vue `refreshFileBrowserTree` 开始时清掉失败、进度走过扫描、写入、刷新三步、
//! 同步完重读摘要、硬链接候选和目录树再读当前目录，任何一段失败写进状态区；缺失仓库、回收站、虚拟视图、
//! 换仓库和过期结果各走各的分支；侧栏状态区原地显示同步进度，刷新按钮转圈并禁用。

use crate::backend::services::repository::{
    AssetSummary, FileBrowserSnapshot, FileTreeNode, RepositoryBackendSummary, RepositoryOverview, RepositorySnapshot,
    RepositoryStructureCacheState, RepositorySummary,
};
use crate::shell::files::{FileDialog, FilesEffect, HardlinkPrompt};
use crate::shell::sidebar::{ShortcutId, SidebarMessage, SidebarTree};
use crate::shell::status::{FailureSource, StatusLine};
use crate::shell::view_harness::ShellHarness;
use crate::shell::view_part_sidebar::project::SidebarView;
use crate::shell::{LibraryCategory, ShellMessage, ShellViewModel, WorkspacePanel};

use super::{SyncPhase, SyncProgress, TreeSyncEffect, TreeSyncMessage, TreeSyncRefresh};

const REPO_ID: &str = "acceptance-repo";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 点一下刷新按钮，取走同步请求的轮次。
fn click_refresh(model: &mut ShellViewModel) -> u64 {
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    match model.tree_sync.take_effects().as_slice() {
        [TreeSyncEffect::Sync { token, repo_id }] if repo_id == REPO_ID => *token,
        effects => panic!("刷新应只排一次当前仓库的同步：{effects:?}"),
    }
}

fn synced(model: &mut ShellViewModel, token: u64, result: Result<u64, String>) {
    model.reduce(ShellMessage::TreeSync(TreeSyncMessage::Synced { token, result }));
}

fn refreshed(model: &mut ShellViewModel, token: u64, result: TreeSyncRefresh) {
    model.reduce(ShellMessage::TreeSync(TreeSyncMessage::Refreshed { token, result }));
}

fn progress(model: &ShellViewModel) -> (SyncPhase, String, u8, u8) {
    let progress = &model.tree_sync.progress;
    (progress.phase, progress.label.clone(), progress.current, progress.percent)
}

fn failure(model: &ShellViewModel) -> Option<(FailureSource, String)> {
    model.status.failure().map(|failure| (failure.source, failure.message.clone()))
}

fn summary() -> RepositorySummary {
    RepositorySummary {
        repo_id: REPO_ID.into(),
        name: "默认资源库".into(),
        path: "C:/acceptance".into(),
        backend: RepositoryBackendSummary {
            plugin_id: "momobako.source.local-filesystem".into(),
            kind: "local-filesystem".into(),
            name: "本地文件系统".into(),
            capabilities: vec!["write".into()],
        },
        status: "ready".into(),
        asset_count: 3,
        updated_at: String::new(),
        local_cache: None,
        authentication: None,
    }
}

fn asset(path: &str) -> AssetSummary {
    AssetSummary {
        asset_id: path.replace('/', "-"),
        repo_id: REPO_ID.into(),
        path: path.into(),
        filename: path.rsplit('/').next().unwrap_or(path).into(),
        extension: path.rsplit_once('.').map(|(_, extension)| extension.to_string()).unwrap_or_default(),
        size_bytes: 1,
        size_label: "1 B".into(),
        status: "ready".into(),
        modified_at: String::new(),
        last_accessed_at: None,
        version: 1,
        tags: Vec::new(),
        thumbnail_path: None,
        hardlink_group_id: None,
        hardlink_state: None,
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

/// 同步后重读的摘要：磁盘上多了 `incoming/new.png`。
fn snapshot() -> RepositorySnapshot {
    RepositorySnapshot {
        repository: summary(),
        folder_label: "根目录".into(),
        folders: Vec::new(),
        assets: ["cover.png", "notes/page.pdf", "incoming/new.png"].map(asset).to_vec(),
        playlists: Vec::new(),
        quick_access: Vec::new(),
        tag_groups: Vec::new(),
        metadata_fields: Vec::new(),
        recent_revision_count: 0,
        overview: RepositoryOverview {
            total_size_bytes: 3,
            total_size_label: "3 B".into(),
            file_count: 3,
            folder_count: 2,
            trash_count: 0,
            readme_content: None,
        },
    }
}

/// 同步后重读的目录树：多了 `incoming`。
fn tree() -> SidebarTree {
    let node = |path: &str| FileTreeNode { path: path.into(), label: path.into(), file_count: 1, children: Vec::new() };
    SidebarTree::from_nodes(&[node("assets"), node("incoming")])
}

fn prompt() -> HardlinkPrompt {
    HardlinkPrompt { id: "candidate-1".into(), new_path: "incoming/new.png".into(), existing_path: "cover.png".into(), size_label: "1 B".into() }
}

/// 三份都读成功。`tree` 为假时和回收站一样不读树。
fn fresh(tree_too: bool) -> TreeSyncRefresh {
    TreeSyncRefresh { snapshot: Ok(snapshot()), hardlinks: Ok(vec![prompt()]), tree: tree_too.then(|| Ok(tree())) }
}

/// 当前目录重读的结果（根目录，空列表就够收尾）。
fn root_browser(trash: bool) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: REPO_ID.into(),
        root_path: "C:/acceptance".into(),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: String::new(),
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: trash.then(|| "trash".into()),
        tree: None,
        entries: Vec::new(),
    }
}

/// 排下的目录读取：（路径，是不是回收站）。
fn browses(model: &mut ShellViewModel) -> Vec<(String, bool)> {
    model
        .files
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            FilesEffect::Browse { path, trash, append: false, .. } => Some((path, trash)),
            _ => None,
        })
        .collect()
}

/// Vue `setSyncProgress` 的百分比：1/3、2/3、3/3 四舍五入。
#[test]
fn progress_percent_rounds_like_vue() {
    let mut progress = SyncProgress::default();
    assert_eq!(progress.shown(), None, "空闲时状态区不显示进度");
    for (phase, label, current, percent) in [
        (SyncPhase::Scanning, "扫描文件夹结构", 1, 33),
        (SyncPhase::Writing, "写入索引结果", 2, 67),
        (SyncPhase::Refreshing, "刷新文件夹树", 3, 100),
    ] {
        progress.set(phase, label, current);
        assert_eq!(progress.percent, percent);
        assert_eq!(progress.shown(), Some((label, percent)), "{label} 要在状态区显示");
    }
    progress.set(SyncPhase::Complete, "刷新完成", 3);
    assert_eq!(progress.shown(), None, "完成后状态区不再显示进度");
    progress.set(SyncPhase::Error, "扫描失败", 3);
    assert_eq!(progress.shown(), None, "失败由错误条显示，不显示进度");
}

/// 点下刷新：清掉上一次失败，开操作进度，进度停在「扫描文件夹结构」1/3，按钮转圈并禁用；进行中再点不重复同步。
#[test]
fn starting_a_refresh_clears_the_failure_and_shows_the_first_step() {
    let mut model = scene("status-error");
    assert!(failure(&model).is_some(), "场景里有一条定位失败");
    let token = click_refresh(&mut model);
    assert!(model.tree_sync.running());
    assert_eq!(failure(&model), None, "刷新开始时清掉上一次失败");
    assert_eq!(progress(&model), (SyncPhase::Scanning, "扫描文件夹结构".into(), 1, 33));
    assert_eq!(StatusLine::project(&model), StatusLine::Progress { label: "扫描文件夹结构".into(), percent: 33 });
    let operation = model.admin.operation.clone().expect("操作进度");
    assert_eq!((operation.label.as_str(), operation.detail.as_str(), operation.value), ("刷新文件树", "同步并读取目录结构", 12.0));
    let folders = SidebarView::project(&model).folders;
    assert!(folders.loading && folders.refresh_disabled, "同步期间刷新按钮转圈并禁用");

    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    assert!(model.tree_sync.take_effects().is_empty(), "进行中再点不再排同步");
    synced(&mut model, token, Ok(1));
    assert_eq!(progress(&model).0, SyncPhase::Refreshing, "原来这一轮照常往下走");
}

/// 三步走完：同步成功后进度到「刷新文件夹树」3/3，重读摘要、硬链接候选和目录树；都写进工作区以后重读当前
/// 目录，读完是「刷新完成」，操作进度收起，状态区不再显示进度。
#[test]
fn a_successful_refresh_reloads_the_workspace_then_completes() {
    let mut model = scene("live-files-plain");
    let token = click_refresh(&mut model);
    synced(&mut model, token, Ok(3));
    assert_eq!(progress(&model), (SyncPhase::Refreshing, "刷新文件夹树".into(), 3, 100));
    let operation = model.admin.operation.clone().expect("操作进度");
    assert_eq!((operation.detail.as_str(), operation.value), ("已扫描 3 个文件", 58.0));
    assert_eq!(model.tree_sync.take_effects(), vec![TreeSyncEffect::Refresh { token, repo_id: REPO_ID.into(), tree: true }]);

    refreshed(&mut model, token, fresh(true));
    assert_eq!(model.sidebar.counts.all, 3, "摘要重读后快捷计数跟上");
    assert!(model.sidebar.folders.iter().any(|folder| folder.path == "incoming"), "目录树换成重读的");
    assert_eq!(model.files.dialog, FileDialog::Hardlink, "新的硬链接候选弹出确认");
    assert_eq!(browses(&mut model), vec![(String::new(), false)], "重读当前目录");
    assert!(model.tree_sync.running(), "目录读完以前还在刷新");
    assert!(SidebarView::project(&model).folders.refresh_disabled);

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(root_browser(false))));
    assert!(!model.tree_sync.running());
    assert_eq!(progress(&model), (SyncPhase::Complete, "刷新完成".into(), 3, 100));
    assert!(model.admin.operation.is_none(), "完成后操作进度收起");
    assert_eq!(StatusLine::project(&model), StatusLine::Hidden);
    assert!(!SidebarView::project(&model).folders.refresh_disabled, "刷新完按钮可以再点");
}

/// 同步失败：写进状态区（来源是同步），取消操作进度，进度标成失败，不再往下读。
#[test]
fn a_failed_sync_lands_in_the_status_area() {
    let mut model = scene("live-files-plain");
    let token = click_refresh(&mut model);
    synced(&mut model, token, Err("拒绝访问。".into()));
    assert_eq!(failure(&model), Some((FailureSource::Sync, "刷新文件夹树失败：拒绝访问。".into())));
    assert_eq!(StatusLine::project(&model), StatusLine::Failure("刷新文件夹树失败：拒绝访问。".into()));
    assert_eq!(progress(&model), (SyncPhase::Error, "拒绝访问。".into(), 3, 100));
    assert!(!model.tree_sync.running());
    assert!(model.admin.operation.is_none());
    assert!(model.tree_sync.take_effects().is_empty(), "失败后不再重读");
}

/// 重读里有一份失败：照 `Promise.all`，成功的几份照样写进去，整轮按失败收尾，不读当前目录。
#[test]
fn a_failed_reload_keeps_what_arrived_and_stops_before_the_directory() {
    let mut model = scene("live-files-plain");
    let token = click_refresh(&mut model);
    synced(&mut model, token, Ok(3));
    model.tree_sync.take_effects();
    let mut partial = fresh(true);
    partial.hardlinks = Err("候选表被占用".into());
    refreshed(&mut model, token, partial);
    assert_eq!(failure(&model), Some((FailureSource::Sync, "刷新文件夹树失败：候选表被占用".into())));
    assert_eq!(model.sidebar.counts.all, 3, "读到的摘要照样写进去");
    assert!(model.sidebar.folders.iter().any(|folder| folder.path == "incoming"), "读到的目录树照样写进去");
    assert!(browses(&mut model).is_empty(), "失败后不读当前目录");
    assert!(!model.tree_sync.running());
    assert!(model.sidebar.tree_error.is_empty(), "整轮失败只记一条，不另记目录树失败");
}

/// 缺失仓库：按钮禁用，点了也不同步。
#[test]
fn a_missing_repository_cannot_refresh() {
    let mut model = scene("missing");
    assert!(SidebarView::project(&model).folders.refresh_disabled);
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    assert!(!model.tree_sync.running());
    assert!(model.tree_sync.take_effects().is_empty());
    assert_eq!(progress(&model).0, SyncPhase::Idle);
}

/// 回收站面板：同步后不读目录树，重读的是回收站。
#[test]
fn the_trash_panel_reloads_the_trash_without_the_tree() {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectShortcut(ShortcutId::Trash)));
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(root_browser(true))));
    model.sidebar.take_effects();
    browses(&mut model);
    let token = click_refresh(&mut model);
    synced(&mut model, token, Ok(0));
    assert_eq!(model.tree_sync.take_effects(), vec![TreeSyncEffect::Refresh { token, repo_id: REPO_ID.into(), tree: false }]);
    refreshed(&mut model, token, fresh(false));
    assert_eq!(browses(&mut model), vec![(String::new(), true)], "回收站面板重读回收站");
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(root_browser(true))));
    assert_eq!(progress(&model).0, SyncPhase::Complete);
}

/// 虚拟视图（库分类）不按目录读：重读完直接收尾。
#[test]
fn virtual_views_finish_without_reading_a_directory() {
    let mut model = scene("live-files-plain");
    model.reduce(ShellMessage::SetLibraryCategory(LibraryCategory::Untagged));
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    let token = click_refresh(&mut model);
    synced(&mut model, token, Ok(2));
    refreshed(&mut model, token, fresh(true));
    assert!(browses(&mut model).is_empty());
    assert_eq!(progress(&model).0, SyncPhase::Complete);
    assert!(!model.tree_sync.running());
}

/// 过期的结果不算；刷新途中换了仓库，这一轮作废，进度回到空闲，不重读。
#[test]
fn stale_results_and_switched_repositories_are_dropped() {
    let mut model = scene("live-files-plain");
    let token = click_refresh(&mut model);
    synced(&mut model, token + 1, Ok(9));
    assert_eq!(progress(&model).0, SyncPhase::Scanning, "别的轮次的结果不算");
    refreshed(&mut model, token, fresh(true));
    assert_eq!(progress(&model).0, SyncPhase::Scanning, "还没同步完的轮次不收重读结果");

    model.workspace.active_repo_id = Some("other-repo".into());
    synced(&mut model, token, Ok(9));
    assert!(!model.tree_sync.running(), "换了仓库，这一轮作废");
    assert_eq!(progress(&model).0, SyncPhase::Idle);
    assert!(model.admin.operation.is_none());
    assert!(model.tree_sync.take_effects().is_empty());
    assert_eq!(failure(&model), None);
}

/// 侧栏状态区原地显示同步进度：扫描 33%、刷新 100%，失败时换成错误条；侧栏不重挂，每一步和新挂的一样。
#[test]
fn the_status_strip_shows_sync_progress_in_place() {
    let mut harness = ShellHarness::mount(scene("live-files-plain"));
    let sidebar = harness.sidebar_root();
    let line = harness.keyed("sidebar-status-progress").expect("同步进度行");
    // 刷新按钮的无障碍名也是「刷新文件夹树」，第三步的进度文案按多出来的那一个认。
    let named = |harness: &ShellHarness| harness.nodes().iter().filter(|node| node.label.as_deref() == Some("刷新文件夹树")).count();
    let buttons = named(&harness);
    harness.apply(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    harness.flush();
    let token = match harness.model.tree_sync.take_effects().as_slice() {
        [TreeSyncEffect::Sync { token, .. }] => *token,
        effects => panic!("没有排同步：{effects:?}"),
    };
    assert!(harness.find("扫描文件夹结构").is_some() && harness.find("33%").is_some(), "状态区写出第一步和百分比");
    let refresh = harness.node("刷新文件夹树");
    assert!(harness.nodes().iter().any(|node| node.id == refresh && node.disabled), "同步期间刷新按钮禁用");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::TreeSync(TreeSyncMessage::Synced { token, result: Ok(3) }));
    harness.flush();
    assert!(named(&harness) == buttons + 1 && harness.find("100%").is_some(), "进度走到第三步");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::TreeSync(TreeSyncMessage::Refreshed { token, result: TreeSyncRefresh { snapshot: Err("摘要不可用".into()), hardlinks: Ok(Vec::new()), tree: Some(Ok(tree())) } }));
    harness.flush();
    assert!(harness.find("刷新文件夹树失败：摘要不可用").is_some(), "失败换成错误条");
    assert!(harness.find("100%").is_none(), "失败时不再显示进度");
    harness.assert_same_as_fresh_mount();
    assert_eq!(harness.sidebar_root(), sidebar, "侧栏不该重挂");
    assert_eq!(harness.keyed("sidebar-status-progress"), Some(line), "进度行原地换内容");
}
