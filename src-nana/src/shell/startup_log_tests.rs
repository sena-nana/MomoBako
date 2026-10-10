//! 首屏加载日志照 Vue `lifecycle.ts` 的 `emitStartupLog`：从启动消息走一遍首屏启动，逐条比对日志的级别、
//! 消息和明细（`键: 值 · 键: 值`）。设置包在启动结束时读，回来以后记「应用配置与插件设置加载完成」和
//! 「首屏启动流程完成」；同步记录的明细是记录的 `context`。

use crate::backend::services::repository::{
    AssetSummary, PlaylistSummary, SystemLogLocation, SystemLogRecord, SystemLogSource,
};
use crate::shell::admin::AdminMessage;
use crate::shell::data_load_tests::{browser, bundle, snapshot, summary};
use crate::shell::host_events::HostMessage;
use crate::shell::{ShellMessage, ShellViewModel, StartupStatus, SyncCounts};

const REPO: &str = "repo-a";

fn asset(path: &str) -> AssetSummary {
    AssetSummary {
        asset_id: format!("asset-{path}"),
        repo_id: REPO.into(),
        path: path.into(),
        filename: path.into(),
        extension: "png".into(),
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

fn playlist() -> PlaylistSummary {
    PlaylistSummary {
        playlist_id: "pl".into(),
        repo_id: REPO.into(),
        name: "早晨".into(),
        player_type_id: "momobako.playlist.audio-sequence".into(),
        player_plugin_id: "momobako.player.audio".into(),
        player_label: "音频".into(),
        file_class: "audio".into(),
        item_count: 1,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// 后端同步写的一条 `repository.sync` 记录。
fn sync_record(message: &str, context: serde_json::Value) -> ShellMessage {
    ShellMessage::Host(HostMessage::LogRecorded(SystemLogRecord {
        id: "sync-1".into(),
        timestamp: "2026-10-10T08:00:00Z".into(),
        level: "info".into(),
        category: "repository.sync".into(),
        action: "scanStart".into(),
        message: message.into(),
        source: SystemLogSource { kind: "repository".into(), label: None, plugin_id: None, repo_id: Some(REPO.into()) },
        location: SystemLogLocation::default(),
        context,
    }))
}

/// 日志的级别、消息和明细，按记下的先后。
fn logs(model: &ShellViewModel) -> Vec<(&'static str, String, String)> {
    model
        .workspace
        .startup
        .logs
        .iter()
        .map(|log| (log.level, log.message.clone(), log.detail.clone().unwrap_or_default()))
        .collect()
}

fn entry(level: &'static str, message: &str, detail: &str) -> (&'static str, String, String) {
    (level, message.to_string(), detail.to_string())
}

/// 一个就绪仓库的首屏启动：14 条日志和 Vue 一条不差，同步、摘要、首屏目录、设置和流程完成都带明细。
#[test]
fn a_launch_logs_every_vue_startup_entry_with_its_detail() {
    let mut model = ShellViewModel::default();
    model.workspace.last_active_repo_id = Some(REPO.into());
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(REPO, "ready"), summary("repo-b", "missing")]) });
    model.reduce(sync_record("开始扫描资源库文件。", serde_json::json!({ "repoPath": "C:/repo-a", "hintPathCount": 0, "samplePaths": ["a.png"], "backendPluginId": null })));
    let counts = SyncCounts { scanned_files: 3, created_assets: 2, updated_assets: 1, deleted_assets: 0, created_events: 4, hardlink_candidates: 1 };
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(counts) });
    let mut index = snapshot(REPO);
    index.assets = vec![asset("a.png"), asset("b.png")];
    index.playlists = vec![playlist()];
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(index) });
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(REPO))));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(model.workspace.startup.logs.len(), 12, "设置包回来之前只记到「开始加载应用配置与插件设置」");
    model.reduce(bundle(Vec::new()));
    assert_eq!(
        logs(&model),
        vec![
            entry("info", "首屏启动流程开始。", "totalSteps: 4 · rememberedRepoId: repo-a"),
            entry("info", "首屏启动开始读取资源库列表。", "step: 1"),
            entry("info", "首屏启动资源库列表读取完成。", "step: 1 · repositoryCount: 2 · readyRepositoryCount: 1 · missingRepositoryCount: 1"),
            entry(
                "info",
                "首屏启动已选定资源库。",
                "repositoryCount: 2 · rememberedRepoId: repo-a · repositoryStatus: ready · repositoryName: 仓库 repo-a · repositoryPath: C:/repo-a",
            ),
            entry("info", "首屏启动开始同步文件变化。", "step: 2"),
            entry("info", "开始扫描资源库文件。", "hintPathCount: 0 · repoPath: C:/repo-a · samplePaths: [\"a.png\"]"),
            entry(
                "info",
                "首屏启动文件变化同步完成。",
                "step: 2 · scannedFiles: 3 · createdAssets: 2 · updatedAssets: 1 · deletedAssets: 0 · createdEvents: 4 · hardlinkCandidates: 1",
            ),
            entry("info", "首屏启动开始读取资源库摘要。", "step: 3"),
            entry("info", "首屏启动资源库摘要读取完成。", "step: 3 · assetCount: 2 · playlistCount: 1 · defaultAssetId: asset-a.png"),
            entry("info", "首屏启动开始加载首屏目录与关联数据。", "step: 4"),
            entry("info", "首屏启动首屏目录与关联数据加载完成。", "step: 4 · playlistCount: 1"),
            entry("info", "首屏启动开始加载应用配置与插件设置。", "step: 4 · repoId: repo-a"),
            entry("info", "首屏启动应用配置与插件设置加载完成。", "step: 4 · repoId: repo-a"),
            entry("info", "首屏启动流程完成。", "activeRepoId: repo-a · assetCount: 2 · playlistCount: 1"),
        ]
    );
    let before = model.workspace.startup.logs.len();
    model.reduce(bundle(Vec::new()));
    assert_eq!(model.workspace.startup.logs.len(), before, "之后再读设置包（设置页、刷新）不再记");
}

/// 没有资源库和资源库缺失的启动也读设置包：设置包读失败也照 Vue 记完两条，流程完成的素材数和播放集数是 0。
#[test]
fn empty_and_missing_launches_close_the_log_after_the_settings_bundle() {
    let failed = || {
        ShellMessage::Admin(AdminMessage::SettingsBundleLoaded {
            plugins: Err("读取插件目录失败".into()),
            hooks: Ok(Vec::new()),
            cache: Err("读取插件目录失败".into()),
            api: Err("读取插件目录失败".into()),
            external: Err("读取插件目录失败".into()),
        })
    };
    let mut empty = ShellViewModel::default();
    let generation = empty.workspace.prepare_initial_list();
    empty.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(Vec::new()) });
    empty.reduce(failed());
    assert_eq!(
        logs(&empty)[3..],
        [
            entry("warn", "首屏启动未找到可加载的资源库。", "repositoryCount: 0"),
            entry("info", "首屏启动开始加载应用配置与插件设置。", "step: 4"),
            entry("info", "首屏启动应用配置与插件设置加载完成。", "step: 4"),
            entry("info", "首屏启动流程完成。", "assetCount: 0 · playlistCount: 0"),
        ]
    );

    let mut missing = ShellViewModel::default();
    let generation = missing.workspace.prepare_initial_list();
    missing.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary("gone", "missing")]) });
    missing.reduce(bundle(Vec::new()));
    let tail = &logs(&missing)[4..];
    assert_eq!(
        tail,
        [
            entry("warn", "首屏启动遇到缺失资源库。", "repositoryName: 仓库 gone · repositoryPath: C:/gone"),
            entry("info", "首屏启动开始加载应用配置与插件设置。", "step: 4 · repoId: gone"),
            entry("info", "首屏启动应用配置与插件设置加载完成。", "step: 4 · repoId: gone"),
            entry("info", "首屏启动流程完成。", "activeRepoId: gone · assetCount: 0 · playlistCount: 0"),
        ]
    );
}
