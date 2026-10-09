//! 验收场景共用的工作区底子：Vue 夹具 `tmp/vue-mock/fixtures.ts` 的 `base()`。
//!
//! 资源库「默认资源库」在 `C:/acceptance`，根目录有 assets 文件夹、cover.png（2400000 B）和 notes/page.pdf
//! （1820 B，和 Vue 模拟一样也列在根目录），目录树 assets → covers，网格展示；侧栏计数是全部 2、未分类 1、
//! 未标签 2。起步照产品启动走真实消息：读资源库列表、同步、读仓库摘要、读首屏目录，启动结束时读设置包、
//! 插件列表到了再读播放器类型，再让侧栏读完目录树、智能文件夹和播放集，所有应答都和 Vue 模拟 IPC
//! （`ipc.ts`）的一致：插件是全部内置插件（`pluginFilter: () => true`）。场景要别的条目、目录树、
//! 播放集、播放器或仓库时改 [`Base`] 里对应的字段。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{
    AssetSummary, FileBrowserEntry, FileBrowserSnapshot, FileTreeNode, FolderSummary, PlaylistPlayerContribution, PlaylistSummary,
    RepositoryBackendSummary, RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
};

use super::super::admin::AdminEffect;
use super::super::files::DisplayMode;
use super::super::sidebar::SidebarTree;
use super::super::{ShellMessage, ShellPage, ShellViewModel, SidebarMessage};
use super::plugin_fixtures::{bundle_loaded, bundled_plugins};

pub(super) const REPO_ID: &str = "acceptance-repo";
pub(super) const REPO_NAME: &str = "默认资源库";
pub(super) const REPO_PATH: &str = "C:/acceptance";
/// Vue 夹具 `NOW`。
pub(super) const NOW: &str = "2026-10-08T08:00:00Z";
const BACKEND_PLUGIN_ID: &str = "momobako.source.local-filesystem";
/// 夹具固定时刻 2026-10-08T08:00:00Z 在 UTC+8 的本地时间。Vue 截图时把浏览器时钟固定在同一时刻。
const FIXED_CLOCK: &str = "16:00:00";
/// 内置插件清单里声明的播放器。Vue 对照页按真实清单加载插件，播放器列表也从这里来。
const PLAYER_MANIFESTS: [&str; 2] = [
    include_str!("../../../External/Plugins/media-preview/manifest.json"),
    include_str!("../../../External/Plugins/player-audio/manifest.json"),
];

/// 一个场景的工作区底子。默认就是 Vue `base()`。
pub(super) struct Base {
    /// 当前资源库的名字。
    pub name: &'static str,
    /// 根目录的条目，也是仓库摘要里的素材（文件那几条）。
    pub entries: Vec<FileBrowserEntry>,
    pub tree: Vec<FileTreeNode>,
    pub mode: DisplayMode,
    /// 侧栏的播放集。
    pub playlists: Vec<PlaylistSummary>,
    /// 插件登记的播放器。
    pub players: Vec<PlaylistPlayerContribution>,
    /// 资源库列表里当前仓库以外的仓库。
    pub others: Vec<RepositorySummary>,
}

impl Default for Base {
    fn default() -> Self {
        Self {
            name: REPO_NAME,
            entries: vec![dir("assets"), file("cover.png", 2_400_000), file("notes/page.pdf", 1_820)],
            tree: vec![tree_node("assets", vec![tree_node("assets/covers", Vec::new())])],
            mode: DisplayMode::Grid,
            playlists: Vec::new(),
            players: playlist_players(),
            others: Vec::new(),
        }
    }
}

impl Base {
    /// 新建一个验收模型并铺好底子，停在文件页。
    pub(super) fn model(self) -> ShellViewModel {
        let mut model = acceptance_model();
        model.page = ShellPage::FileList;
        self.seed(&mut model);
        model
    }

    /// 在 `model` 上铺好底子：照产品启动依次送回列表、同步、摘要、首屏目录和侧栏的读取结果。
    /// `for_page` 已经定下的页面身份留着（启动结果会把页面切到文件页）。
    pub(super) fn seed(self, model: &mut ShellViewModel) {
        let page = model.page.clone();
        model.acceptance_scene = true;
        model.workspace.last_active_repo_id = Some(REPO_ID.into());
        model.files.display_mode = self.mode;
        let generation = model.workspace.prepare_initial_list();
        let mut repositories = vec![summary(self.name, "ready")];
        repositories.extend(self.others.iter().cloned());
        model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(repositories) });
        model.reduce(ShellMessage::StartupSyncFinished { generation, result: Ok(()) });
        model.reduce(ShellMessage::RepositorySnapshotLoaded(Ok(self.snapshot())));
        model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(self.entries.clone()))));
        answer_admin_reads(model, &self.players);
        let repo_id = REPO_ID.to_string();
        let playlists = self.playlists.clone();
        for message in [
            SidebarMessage::SidebarTreeLoaded { repo_id: repo_id.clone(), result: Ok(SidebarTree::from_nodes(&self.tree)) },
            SidebarMessage::SidebarSmartFoldersLoaded { repo_id: repo_id.clone(), result: Ok(Vec::new()) },
            SidebarMessage::SidebarPlaylistsLoaded { repo_id, result: Ok(playlists) },
        ] {
            model.reduce(ShellMessage::Sidebar(message));
        }
        // 上面这些请求都已经按夹具答过，宿主不会再派发。
        model.workspace.take_effects();
        model.sidebar.take_effects();
        model.page = page;
        settle(model);
    }

    /// 仓库摘要里的素材：条目里的文件，标签跟着条目走。
    fn assets(&self) -> Vec<AssetSummary> {
        self.entries.iter().filter(|entry| entry.kind == "file").map(asset).collect()
    }

    /// Vue 模拟 IPC 的 `get_repository_snapshot` 应答，总大小照模拟写死。
    fn snapshot(&self) -> RepositorySnapshot {
        let assets = self.assets();
        RepositorySnapshot {
            repository: summary(self.name, "ready"),
            folder_label: "根目录".into(),
            folders: self
                .tree
                .iter()
                .map(|node| FolderSummary { path: node.path.clone(), label: node.label.clone(), asset_count: node.file_count as i64 })
                .collect(),
            playlists: self.playlists.clone(),
            quick_access: Vec::new(),
            tag_groups: Vec::new(),
            metadata_fields: Vec::new(),
            recent_revision_count: 0,
            overview: RepositoryOverview {
                total_size_bytes: 2_401_820,
                total_size_label: "2.3 MB".into(),
                file_count: assets.len() as i64,
                folder_count: self.tree.len() as i64,
                trash_count: 0,
                readme_content: None,
            },
            assets,
        }
    }
}

/// 回答启动结束时排下的设置包读取，以及插件列表到了以后排下的播放器类型读取。
/// 没排下的读取不编造应答，只记日志：说明产品启动已经不读它们了。
fn answer_admin_reads(model: &mut ShellViewModel, players: &[PlaylistPlayerContribution]) {
    if !model.admin.take_effects().iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)) {
        eprintln!("Nana 验收底子：启动结束时没有读设置包");
        return;
    }
    model.reduce(ShellMessage::Admin(bundle_loaded(bundled_plugins(|_| true))));
    if !model.admin.take_effects().iter().any(|effect| matches!(effect, AdminEffect::LoadPlaylistPlayers)) {
        eprintln!("Nana 验收底子：插件列表到了以后没有读播放器类型");
        return;
    }
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(players.to_vec())));
}

/// 验收用的空白壳层：上次打开的是夹具仓库，和 Vue 的 `lastActiveRepositoryId` 一致。
pub(super) fn acceptance_model() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.acceptance_scene = true;
    model.workspace.last_active_repo_id = Some(REPO_ID.into());
    model
}

/// 把动效时钟拨过所有过渡，日志时间换成夹具的固定时刻，截图拿到静止画面。
pub(super) fn settle(model: &mut ShellViewModel) {
    model.motion.set_startup_percent(f32::from(model.workspace.startup.percent));
    let now = model.motion.now_ms();
    model.motion.advance(now.saturating_add(1_000));
    for log in &mut model.workspace.startup.logs {
        log.time = FIXED_CLOCK.into();
    }
}

/// Vue 夹具 `repository(name, status)`：本地文件系统仓库，素材数照夹具写 2。
pub(super) fn summary(name: &str, status: &str) -> RepositorySummary {
    RepositorySummary {
        repo_id: REPO_ID.into(),
        name: name.into(),
        path: REPO_PATH.into(),
        backend: RepositoryBackendSummary {
            plugin_id: BACKEND_PLUGIN_ID.into(),
            kind: "local-filesystem".into(),
            name: "本地文件系统".into(),
            capabilities: ["write", "localRootPath", "list", "read", "move", "delete", "watch"].map(String::from).to_vec(),
        },
        status: status.into(),
        asset_count: 2,
        updated_at: NOW.into(),
        local_cache: None,
        authentication: None,
    }
}

/// Vue 夹具 `dir(path)`。
pub(super) fn dir(path: &str) -> FileBrowserEntry {
    entry(path, "directory", None)
}

/// Vue 夹具 `file(path, sizeBytes)`：大小写成「N B」，素材 id 把 `/` 换成 `-`，状态 ready。
pub(super) fn file(path: &str, size_bytes: i64) -> FileBrowserEntry {
    entry(path, "file", Some(size_bytes))
}

/// 带标签和文本元数据的文件，Vue 场景里 `{ ...file(..), tags, metadata }` 的写法。
pub(super) fn tagged_file(path: &str, size_bytes: i64, tags: &[&str], metadata: BTreeMap<String, Value>) -> FileBrowserEntry {
    FileBrowserEntry { tags: tags.iter().map(|tag| tag.to_string()).collect(), metadata, ..file(path, size_bytes) }
}

fn entry(path: &str, kind: &str, size_bytes: Option<i64>) -> FileBrowserEntry {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let is_file = kind == "file";
    let extension = if is_file { name.rsplit_once('.').map(|(_, extension)| extension.to_ascii_lowercase()) } else { None };
    FileBrowserEntry {
        path: path.into(),
        name,
        kind: kind.into(),
        extension,
        size_bytes,
        size_label: size_bytes.map(|bytes| format!("{bytes} B")),
        modified_at: Some(NOW.into()),
        asset_id: is_file.then(|| path.replace('/', "-")),
        status: is_file.then(|| "ready".to_string()),
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

fn tree_node(path: &str, children: Vec<FileTreeNode>) -> FileTreeNode {
    FileTreeNode { path: path.into(), label: path.rsplit('/').next().unwrap_or(path).into(), file_count: 0, children }
}

/// Vue 夹具 `asset(entry)`：仓库摘要里的一条素材。
fn asset(entry: &FileBrowserEntry) -> AssetSummary {
    AssetSummary {
        asset_id: entry.asset_id.clone().unwrap_or_default(),
        repo_id: REPO_ID.into(),
        path: entry.path.clone(),
        filename: entry.name.clone(),
        extension: entry.extension.clone().unwrap_or_default(),
        size_bytes: entry.size_bytes.unwrap_or_default(),
        size_label: entry.size_label.clone().unwrap_or_else(|| "0 B".into()),
        status: "ready".into(),
        modified_at: NOW.into(),
        last_accessed_at: None,
        version: 1,
        tags: entry.tags.clone(),
        thumbnail_path: entry.thumbnail_path.clone(),
        hardlink_group_id: None,
        hardlink_state: None,
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

/// 根目录的浏览结果。和 Vue 模拟一样，notes/page.pdf 也列在根目录。
fn browser(entries: Vec<FileBrowserEntry>) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: REPO_ID.into(),
        root_path: REPO_PATH.into(),
        backend_plugin_id: BACKEND_PLUGIN_ID.into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: Some(NOW.into()),
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

/// 内置插件清单里的播放器贡献，按清单顺序。清单读不出时记日志并跳过。
pub(super) fn playlist_players() -> Vec<PlaylistPlayerContribution> {
    PLAYER_MANIFESTS
        .iter()
        .flat_map(|raw| {
            let players = serde_json::from_str::<Value>(raw)
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

#[cfg(test)]
#[path = "acceptance_base_tests.rs"]
mod tests;
