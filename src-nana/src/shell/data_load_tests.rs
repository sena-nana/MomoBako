//! 产品路径上的数据读取：从启动消息或用户操作出发，断言归约排下了哪些服务读取，
//! 再把读取结果送回，看界面要读的状态有没有接上。时机照 Vue 各 composable 读数据的时候。

use crate::backend::services::repository::{
    ApiDesignSnapshot, CacheConfig, CacheSnapshot, FileBrowserSnapshot, PlaylistPlayerContribution, PlaylistSummary, PluginManifest,
    RepositoryBackendSummary, RepositoryLocalCacheStatus, RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState,
    RepositorySummary,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;
use crate::shell::admin::{AdminEffect, AdminMessage};
use crate::shell::player::PlayerEffect;
use crate::shell::view_part_sidebar::project::SidebarView;
use crate::shell::{
    ShellMessage, ShellPage, ShellViewModel, SidebarEffect, SidebarMessage, StartupStatus, WorkspaceEffect, WorkspacePanel,
};

const REPO: &str = "repo-a";

pub(super) fn summary(repo_id: &str, status: &str) -> RepositorySummary {
    RepositorySummary {
        repo_id: repo_id.into(),
        name: format!("仓库 {repo_id}"),
        path: format!("C:/{repo_id}"),
        backend: RepositoryBackendSummary {
            plugin_id: "momobako.source.local-filesystem".into(),
            kind: "local-filesystem".into(),
            name: "本地文件系统".into(),
            capabilities: vec!["write".into(), "localRootPath".into()],
        },
        status: status.into(),
        asset_count: 0,
        updated_at: String::new(),
        local_cache: None,
        authentication: None,
    }
}

pub(super) fn snapshot(repo_id: &str) -> RepositorySnapshot {
    RepositorySnapshot {
        repository: summary(repo_id, "ready"),
        folder_label: "根目录".into(),
        folders: Vec::new(),
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
            trash_count: 0,
            readme_content: None,
        },
        assets: Vec::new(),
    }
}

pub(super) fn browser(repo_id: &str) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: repo_id.into(),
        root_path: format!("C:/{repo_id}"),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: String::new(),
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries: Vec::new(),
    }
}

/// 照产品启动走：读资源库列表、同步、读摘要、读首屏目录。每一步之前先清掉管理副作用，
/// 断言启动结束之前没有提前读设置包。
pub(super) fn started(repo_id: &str) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    start(&mut model, repo_id);
    model
}

/// 在已有的壳层上走一遍启动，见 [`started`]。
pub(super) fn start(model: &mut ShellViewModel, repo_id: &str) {
    let generation = model.workspace.prepare_initial_list();
    let steps = [
        ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(repo_id, "ready")]) },
        ShellMessage::StartupSyncFinished { generation, result: Ok(()) },
        ShellMessage::RepositorySnapshotLoaded(Ok(snapshot(repo_id))),
    ];
    for message in steps {
        model.reduce(message);
        assert!(
            !model.admin.take_effects().iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)),
            "启动还没结束就读了设置包"
        );
    }
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(repo_id))));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
}

fn plugin(plugin_id: &str, contributes: serde_json::Value) -> PluginManifest {
    serde_json::from_value(serde_json::json!({
        "pluginId": plugin_id,
        "name": plugin_id,
        "version": "1.0.0",
        "kind": "preview",
        "description": "",
        "capabilities": [],
        "enabled": true,
        "sdk": "frontend",
        "entry": {},
        "contributes": contributes,
        "source": "builtin",
        "runtime": "vue-module",
        "permissions": [],
        "compat": { "sdkVersion": "1" },
        "status": "ready",
    }))
    .expect("插件清单夹具")
}

fn audio_player() -> PlaylistPlayerContribution {
    PlaylistPlayerContribution {
        player_type_id: "momobako.playlist.audio-sequence".into(),
        label: "音频顺序播放".into(),
        file_class: "audio".into(),
        supported_extensions: vec!["mp3".into()],
        supports_seek: true,
        supports_volume: true,
        supports_preview_navigation: true,
        description: None,
    }
}

fn bundle(plugins: Vec<PluginManifest>) -> ShellMessage {
    ShellMessage::Admin(AdminMessage::SettingsBundleLoaded {
        plugins: Ok(plugins),
        hooks: Ok(Vec::new()),
        cache: Ok(CacheSnapshot { config: CacheConfig { metadata_capacity: 1, thumbnail_capacity: 1, query_capacity: 1 }, entries: Vec::new() }),
        api: Ok(ApiDesignSnapshot { transport: "native".into(), endpoints: Vec::new() }),
        external: Ok(ExternalApiConnectionStatus {
            base_url: "http://127.0.0.1:1".into(),
            token: "token".into(),
            version: "0".into(),
            started_at: String::new(),
            ready: true,
            connection_file_path: String::new(),
        }),
    })
}

fn admin_effects(model: &mut ShellViewModel) -> Vec<AdminEffect> {
    model.admin.take_effects()
}

/// Vue 启动流程在结束前 `loadSettingsData`：有仓库的启动走完首屏目录时读一次设置包。
#[test]
fn startup_reads_the_settings_bundle_when_it_finishes() {
    let mut model = started(REPO);
    let effects = admin_effects(&mut model);
    assert!(effects.iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)), "启动结束没有读设置包：{effects:?}");
    assert!(model.admin.loading_settings);
}

/// 没有资源库的启动也读：添加资源库的来源列表要用插件清单。
#[test]
fn an_empty_library_still_reads_the_settings_bundle() {
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(Vec::new()) });
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert!(admin_effects(&mut model).iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)));
}

/// 插件列表到了（启动、刷新、启停、安装、删除）就重读播放器类型；读回以后侧栏能新建播放集，
/// 插件类型的播放集能播放，新建对话框默认选第一个类型。
#[test]
fn plugin_lists_lead_to_player_types() {
    let mut model = started(REPO);
    admin_effects(&mut model);
    model.reduce(bundle(vec![plugin("momobako.player.audio", serde_json::json!({}))]));
    let effects = admin_effects(&mut model);
    assert!(matches!(effects.as_slice(), [AdminEffect::LoadPlaylistPlayers]), "插件列表到了应读播放器类型：{effects:?}");
    assert!(SidebarView::project(&model).playlists.create_locked, "播放器类型读回之前不能新建播放集");

    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![audio_player()])));
    assert_eq!(model.player.contributions.len(), 1, "播放器认得插件登记的类型");
    assert_eq!(model.selected_new_playlist_player_type_id.as_deref(), Some("momobako.playlist.audio-sequence"));
    assert!(!SidebarView::project(&model).playlists.create_locked, "读回播放器类型以后可以新建播放集");

    model.reduce(ShellMessage::Admin(AdminMessage::PluginsReplaced(Ok(Vec::new()))));
    assert!(admin_effects(&mut model).iter().any(|effect| matches!(effect, AdminEffect::LoadPlaylistPlayers)), "启停、安装、删除以后也重读");
    model.reduce(ShellMessage::Admin(AdminMessage::PluginsReplaced(Err("失败".into()))));
    assert!(admin_effects(&mut model).is_empty(), "插件操作失败时列表没换，不重读");
}

/// 侧栏底部的「设置」：进设置页，读设置包和应用设置，和 Vue 设置页挂载时一样。
#[test]
fn opening_settings_reads_the_bundle_and_app_settings() {
    let mut model = started(REPO);
    admin_effects(&mut model);
    model.reduce(ShellMessage::OpenSettings);
    assert_eq!(model.page, ShellPage::Settings);
    let effects = admin_effects(&mut model);
    assert!(effects.iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)));
    assert!(effects.iter().any(|effect| matches!(effect, AdminEffect::LoadAppSettings)));
}

/// 缺失仓库的「打开来源设置」：照 Vue `/settings?plugin=` 进设置页并展开来源插件的设置，
/// 不再走宿主副作用。
#[test]
fn source_cache_issue_opens_the_source_plugin_settings() {
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    let mut remote = summary("remote", "missing");
    remote.backend.plugin_id = "momobako.netease.source".into();
    remote.local_cache = Some(RepositoryLocalCacheStatus { required: true, path: None, status: "unconfigured".into() });
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![remote]) });
    model.reduce(bundle(vec![plugin("momobako.netease.source", serde_json::json!({}))]));
    admin_effects(&mut model);
    model.reduce(ShellMessage::MissingOpenSourceSettings);
    assert_eq!(model.page, ShellPage::Settings);
    assert_eq!(model.admin.active_settings_plugin_id.as_deref(), Some("momobako.netease.source"));
    let effects = admin_effects(&mut model);
    assert!(effects.iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)));
    assert!(effects.iter().any(|effect| matches!(effect, AdminEffect::LoadConfig(id) if id == "momobako.netease.source")));
    assert!(!model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositories { .. })));
}

fn playlist(playlist_id: &str) -> PlaylistSummary {
    PlaylistSummary {
        playlist_id: playlist_id.into(),
        repo_id: REPO.into(),
        name: "早晨".into(),
        player_type_id: "momobako.playlist.audio-sequence".into(),
        player_plugin_id: "momobako.player.audio".into(),
        player_label: "音频顺序播放".into(),
        file_class: "audio".into(),
        item_count: 0,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn sidebar_playlists(repo_id: &str, playlists: Vec<PlaylistSummary>) -> ShellMessage {
    ShellMessage::Sidebar(SidebarMessage::SidebarPlaylistsLoaded { repo_id: repo_id.into(), result: Ok(playlists) })
}

/// 启动绑定侧栏时读播放集列表；读回的列表同时交给播放器：读成员索引，文件右键有「加入播放列表」。
/// 换了仓库的旧结果不写。
#[test]
fn the_sidebar_playlist_list_reaches_the_player() {
    let mut model = started(REPO);
    let effects = model.sidebar.take_effects();
    assert!(
        effects.iter().any(|effect| matches!(effect, SidebarEffect::LoadPlaylists { repo_id } if repo_id == REPO)),
        "启动后侧栏要读播放集：{effects:?}"
    );
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![audio_player()])));
    model.reduce(sidebar_playlists(REPO, vec![playlist("pl-1")]));
    assert_eq!(model.sidebar.playlists.len(), 1);
    assert_eq!(model.player.playlists.len(), 1, "播放器拿到同一份播放集列表");
    assert!(model.player.take_effects().iter().any(|effect| matches!(effect, PlayerEffect::LoadMemberships { repo_id } if repo_id == REPO)));
    let actions = model.player.membership_actions("file", "mp3", "asset-1", false);
    assert_eq!(actions.iter().map(|action| action.label.as_str()).collect::<Vec<_>>(), ["加入 早晨"], "文件右键有加入播放列表");

    model.reduce(sidebar_playlists("repo-old", Vec::new()));
    assert_eq!(model.player.playlists.len(), 1, "别的仓库的旧结果不写");
}

/// 新建播放集：对话框打开时清空名称、默认选第一个类型；读回的列表进侧栏和播放器，并照 Vue 接着点开
/// 新建的那个。删除以后点开的那个不在了，回到文件面板。
#[test]
fn a_created_playlist_is_listed_and_opened() {
    let mut model = started(REPO);
    model.sidebar.take_effects();
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![audio_player()])));
    model.reduce(ShellMessage::NewPlaylistNameChanged("上次没建完".into()));
    model.reduce(ShellMessage::OpenPlaylistDialog);
    assert!(model.playlist_dialog_open);
    assert!(model.new_playlist_name.is_empty(), "每次打开都清空名称");
    assert_eq!(model.selected_new_playlist_player_type_id.as_deref(), Some("momobako.playlist.audio-sequence"));

    model.reduce(ShellMessage::NewPlaylistNameChanged("早晨".into()));
    model.reduce(ShellMessage::CreatePlaylist);
    model.reduce(ShellMessage::PlaylistsLoaded { repo_id: REPO.into(), result: Ok(vec![playlist("pl-new")]), open: Some("pl-new".into()) });
    assert_eq!(model.sidebar.playlists.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(), ["pl-new"]);
    assert_eq!(model.player.playlists.len(), 1);
    assert_eq!(model.workspace.panel, WorkspacePanel::Playlist, "新建以后点开它");
    assert!(model
        .sidebar
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, SidebarEffect::LoadPlaylistDetail { playlist_id, .. } if playlist_id == "pl-new")));

    model.reduce(ShellMessage::PlaylistsLoaded { repo_id: REPO.into(), result: Ok(Vec::new()), open: None });
    assert!(model.sidebar.playlists.is_empty());
    assert!(model.sidebar.active_playlist_id.is_none());
    assert_eq!(model.workspace.panel, WorkspacePanel::Files, "点开的播放集删掉以后回到文件面板");

    model.reduce(ShellMessage::PlaylistsLoaded { repo_id: REPO.into(), result: Err("磁盘已满".into()), open: None });
    assert_eq!(model.status.failure().map(|failure| failure.message.as_str()), Some("播放集操作失败：磁盘已满"));
}

/// 启动时恢复存下的播放会话：播放集列表和播放器类型都读回以后才读它的详情。
#[test]
fn startup_restores_the_stored_session_after_players_arrive() {
    let dir = std::env::temp_dir().join(format!("momobako-data-load-restore-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let sessions = dir.join("sessions.json");
    let stored = serde_json::json!({
        "repo-a": {
            "repoId": "repo-a",
            "playlistId": "pl-1",
            "playerTypeId": "momobako.playlist.audio-sequence",
            "currentItemId": "a",
            "currentTimeMs": 61000,
            "durationMs": 180000,
            "mode": "listLoop",
            "volume": 0.5,
            "isPlaying": false,
        }
    });
    std::fs::write(&sessions, stored.to_string()).expect("写会话");
    let mut model = ShellViewModel::default();
    model.player.load_files(&dir.join("missing-settings.json"), &sessions, &dir.join("missing-prefs.json"));
    start(&mut model, REPO);
    model.reduce(sidebar_playlists(REPO, vec![playlist("pl-1")]));
    let restores = |model: &mut ShellViewModel| {
        model.player.take_effects().into_iter().filter(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. })).count()
    };
    assert_eq!(restores(&mut model), 0, "插件类型的播放器还没读回");
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![audio_player()])));
    assert_eq!(restores(&mut model), 1, "播放器类型读回以后读存下的播放集详情");
    let _ = std::fs::remove_dir_all(dir);
}
