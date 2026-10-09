//! 壳层与侧栏的对照场景：标题栏、侧栏、仓库切换、启动、空库和缺失仓库。
//!
//! 场景名和 `tmp/vue-mock/scenes/shell.ts` 的 Vue 场景同名，数据和 Vue 夹具（`fixtures.ts` 的 `base()`、
//! `liveVisual()`）一致：仓库在 `C:/acceptance`，「默认资源库」根目录有 assets、cover.png 和
//! notes/page.pdf，目录树 assets → covers。状态都走生产的启动、列表、侧栏消息，不直接拼画面。

use crate::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, FileTreeNode, PlaylistPlayerContribution, RepositoryStructureCacheState,
};

use super::super::files::DisplayMode;
use super::super::input::InputMessage;
use super::super::sidebar::{GapMessage, ShortcutAsset, SidebarTree};
use super::super::workspace::WorkspaceRepository;
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};

const REPO_ID: &str = "acceptance-repo";
const REPO_PATH: &str = "C:/acceptance";
const BACKEND_PLUGIN_ID: &str = "momobako.source.local-filesystem";
/// 夹具固定时刻 2026-10-08T08:00:00Z 在 UTC+8 的本地时间。Vue 截图时把浏览器时钟固定在同一时刻。
const FIXED_CLOCK: &str = "16:00:00";
/// Vue `error` 场景里同步命令抛出的错误。
const SYNC_ERROR: &str = "无法读取仓库目录，请检查路径和权限";
/// `status-error` 场景里系统文件管理器起不来时的原因。
const REVEAL_ERROR: &str = "系统找不到指定的文件。";
/// 内置插件清单里声明的播放器。Vue 对照页按真实清单加载插件，播放器列表也从这里来。
const PLAYER_MANIFESTS: [&str; 2] = [
    include_str!("../../../External/Plugins/media-preview/manifest.json"),
    include_str!("../../../External/Plugins/player-audio/manifest.json"),
];

/// 本面板的离屏对照场景。加载、空库和启动失败三页由 `acceptance::seed` 调下面的 `seed_*`。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("live-files-plain", live_files_plain()),
        ("live-visual", live_visual()),
        ("missing", missing()),
        ("repo-switcher", repo_switcher()),
        ("repo-delete-dialog", repo_delete_dialog()),
        ("folder-create-dialog", folder_create_dialog()),
        ("smart-folder-dialog", smart_folder_dialog()),
        ("playlist-create-dialog", playlist_create_dialog()),
        ("status-error", status_error()),
        ("folder-tree-refresh", folder_tree_refresh()),
    ]
}

/// 加载页：读完资源库列表、选中「默认资源库」，停在第 2 步同步文件变化。
pub(super) fn seed_loading(model: &mut ShellViewModel) {
    begin_sync(model, "默认资源库");
    settle(model);
}

/// 启动失败：同步文件变化时返回夹具的错误，停在第 2 步。
pub(super) fn seed_error(model: &mut ShellViewModel) {
    let generation = begin_sync(model, "默认资源库");
    model.workspace.note_sync_finished(generation, Err(SYNC_ERROR.into()));
    model.detail = SYNC_ERROR.into();
    settle(model);
}

/// 空库：资源库列表为空，启动完成后进空库页，侧栏只剩没有仓库时的提示。
pub(super) fn seed_empty(model: &mut ShellViewModel) {
    model.workspace.last_active_repo_id = Some(REPO_ID.into());
    let generation = model.workspace.prepare_initial_list();
    model.workspace.apply_repository_list(Some(generation), Ok(Vec::new()));
    model.bind_sidebar_repository();
    settle(model);
}

/// Vue `base()`：默认资源库，网格展示。
fn live_files_plain() -> ShellViewModel {
    let entries = vec![directory("assets"), file("cover.png", 2_400_000), file("notes/page.pdf", 1_820)];
    let tree = vec![FileTreeNode {
        path: "assets".into(),
        label: "assets".into(),
        file_count: 0,
        children: vec![FileTreeNode { path: "assets/covers".into(), label: "covers".into(), file_count: 0, children: Vec::new() }],
    }];
    ready_workspace("默认资源库", entries, &tree, DisplayMode::Grid)
}

/// Vue `liveVisual()`：动画素材，photos、audio 和 cover.png，自适应展示，没有目录树。
fn live_visual() -> ShellViewModel {
    let entries = vec![directory("photos"), directory("audio"), file("cover.png", 2_400_000)];
    ready_workspace("动画素材", entries, &[], DisplayMode::Adaptive)
}

/// Vue `missing`：唯一的资源库目录丢失，启动直接进缺失仓库页。
fn missing() -> ShellViewModel {
    let mut model = acceptance_model();
    let generation = model.workspace.prepare_initial_list();
    model.workspace.apply_repository_list(Some(generation), Ok(vec![repository("默认资源库", "missing")]));
    model.bind_sidebar_repository();
    settle(&mut model);
    model
}

/// 点开仓库头：切换列表、添加资源库和删除当前资源库。
fn repo_switcher() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher));
    settle(&mut model);
    model
}

/// 切换列表里点「删除当前资源库」。
fn repo_delete_dialog() -> ShellViewModel {
    let mut model = repo_switcher();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::DeleteRepositoryFromSwitcher));
    settle(&mut model);
    model
}

/// 文件夹分组的「在当前目录新建文件夹」，当前目录是根目录。
fn folder_create_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(String::new()))));
    settle(&mut model);
    model
}

/// 智能文件夹分组的「新建智能文件夹」。
fn smart_folder_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolderDialog));
    settle(&mut model);
    model
}

/// 播放集分组的「新建播放集」。
fn playlist_create_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::OpenPlaylistDialog);
    settle(&mut model);
    model
}

/// 在文件管理器里定位 cover.png 失败：系统程序起不来，失败显示在侧栏顶部的全局状态区，文件列表照常显示。
fn status_error() -> ShellViewModel {
    let mut model = live_files_plain();
    let target = format!("{REPO_PATH}/cover.png");
    model.reduce(ShellMessage::Input(InputMessage::RevealEntry { absolute_path: target }));
    crate::host_bridge::perform(&mut model, |_, _| Err(REVEAL_ERROR.into()), |_| Ok(()), || Ok(()));
    settle(&mut model);
    model
}

/// 点了文件夹分组的「刷新文件夹树」，仓库同步还在跑：侧栏状态区显示同步进度「扫描文件夹结构 33%」，
/// 刷新按钮转圈并禁用。Vue 模拟的同步当场返回，截不到这一刻，没有对照图。
fn folder_tree_refresh() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    model.tree_sync.take_effects();
    settle(&mut model);
    model
}

/// 验收用的空白壳层：上次打开的是夹具仓库，和 Vue 的 `lastActiveRepositoryId` 一致。
fn acceptance_model() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.acceptance_scene = true;
    model.workspace.last_active_repo_id = Some(REPO_ID.into());
    model
}

/// 启动流程走到第 2 步：读完资源库列表、选中仓库并开始同步。返回这一轮的代次。
fn begin_sync(model: &mut ShellViewModel, name: &str) -> u64 {
    model.workspace.last_active_repo_id = Some(REPO_ID.into());
    let generation = model.workspace.prepare_initial_list();
    model.workspace.apply_repository_list(Some(generation), Ok(vec![repository(name, "ready")]));
    generation
}

/// 启动全部完成的工作区：同步、摘要、首屏目录，再让侧栏读完目录树、智能文件夹和播放集。
fn ready_workspace(name: &str, entries: Vec<FileBrowserEntry>, tree: &[FileTreeNode], mode: DisplayMode) -> ShellViewModel {
    let mut model = acceptance_model();
    model.files.display_mode = mode;
    let generation = begin_sync(&mut model, name);
    model.workspace.note_sync_finished(generation, Ok(()));
    // 摘要：和 `RepositorySnapshotLoaded` 一样记下仓库并重算快捷方式计数。
    model.workspace.note_index_finished(REPO_ID, Ok(()));
    model.repository_id = Some(REPO_ID.into());
    model.repository_name = name.into();
    model.bind_sidebar_repository();
    let assets = entries
        .iter()
        .filter(|entry| entry.kind == "file")
        .map(|entry| ShortcutAsset { path: entry.path.clone(), untagged: entry.tags.is_empty(), accessed: false, deleted: false })
        .collect::<Vec<_>>();
    model.sidebar.apply_snapshot(&assets, 0, Vec::new());
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(entries))));
    let players = playlist_players();
    let player_ids = players.iter().map(|player| player.player_type_id.clone()).collect::<Vec<_>>();
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(players)));
    let repo_id = REPO_ID.to_string();
    for message in [
        SidebarMessage::SidebarTreeLoaded { repo_id: repo_id.clone(), result: Ok(SidebarTree::from_nodes(tree)) },
        SidebarMessage::SidebarSmartFoldersLoaded { repo_id: repo_id.clone(), result: Ok(Vec::new()) },
        SidebarMessage::SidebarPlaylistsLoaded { repo_id: repo_id.clone(), result: Ok(Vec::new()) },
        SidebarMessage::SidebarPlaylistPlayersLoaded { repo_id, result: Ok(player_ids) },
    ] {
        model.reduce(ShellMessage::Sidebar(message));
    }
    settle(&mut model);
    model
}

/// 内置插件清单里的播放器贡献，按清单顺序。清单读不出时记日志并跳过。
fn playlist_players() -> Vec<PlaylistPlayerContribution> {
    PLAYER_MANIFESTS
        .iter()
        .flat_map(|raw| {
            let players = serde_json::from_str::<serde_json::Value>(raw)
                .ok()
                .and_then(|manifest| manifest.pointer("/contributes/playlistPlayers").cloned())
                .map(serde_json::from_value::<Vec<PlaylistPlayerContribution>>);
            match players {
                Some(Ok(players)) => players,
                Some(Err(error)) => {
                    eprintln!("Nana 验收播放器清单解析失败：{error}");
                    Vec::new()
                }
                None => {
                    eprintln!("Nana 验收插件清单里没有 playlistPlayers");
                    Vec::new()
                }
            }
        })
        .collect()
}

/// 把动效时钟拨过所有过渡，日志时间换成夹具的固定时刻，截图拿到静止画面。
fn settle(model: &mut ShellViewModel) {
    model.motion.set_startup_percent(f32::from(model.workspace.startup.percent));
    let now = model.motion.now_ms();
    model.motion.advance(now.saturating_add(1_000));
    for log in &mut model.workspace.startup.logs {
        log.time = FIXED_CLOCK.into();
    }
}

fn repository(name: &str, status: &str) -> WorkspaceRepository {
    WorkspaceRepository {
        repo_id: REPO_ID.into(),
        name: name.into(),
        path: REPO_PATH.into(),
        status: status.into(),
        backend_plugin_id: BACKEND_PLUGIN_ID.into(),
        capabilities: ["write", "localRootPath", "list", "read", "move", "delete", "watch"].map(String::from).to_vec(),
        cache_required: false,
        cache_status: String::new(),
    }
}

/// 根目录的浏览结果。和 Vue 夹具一样，notes/page.pdf 也列在根目录。
fn browser(entries: Vec<FileBrowserEntry>) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: REPO_ID.into(),
        root_path: REPO_PATH.into(),
        backend_plugin_id: BACKEND_PLUGIN_ID.into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: String::new(),
        total_entries: entries.len(),
        loaded_count: entries.len(),
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries,
    }
}

fn directory(path: &str) -> FileBrowserEntry {
    entry(path, "directory", None)
}

/// 夹具文件：大小写成「N B」，和 Vue `file()` 一样。
fn file(path: &str, size: i64) -> FileBrowserEntry {
    entry(path, "file", Some(size))
}

fn entry(path: &str, kind: &str, size: Option<i64>) -> FileBrowserEntry {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = (kind == "file").then(|| name.rsplit_once('.').map(|(_, extension)| extension.to_lowercase())).flatten();
    FileBrowserEntry {
        path: path.into(),
        name,
        kind: kind.into(),
        extension,
        size_bytes: size,
        size_label: size.map(|bytes| format!("{bytes} B")),
        modified_at: None,
        asset_id: (kind == "file").then(|| path.replace('/', "-")),
        status: None,
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: std::collections::BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}
