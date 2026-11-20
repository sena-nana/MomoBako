//! 产品路径上的数据读取：从启动消息或用户操作出发，断言归约排下了哪些服务读取，
//! 再把读取结果送回，看界面要读的状态有没有接上。

use crate::backend::services::repository::{
    ApiDesignSnapshot, FileBrowserSnapshot, PlaylistPlayerContribution, PlaylistSummary,
    PluginManifest, RepositoryAction, RepositoryBackendSummary, RepositoryLocalCacheStatus,
    RepositoryOverview, RepositorySnapshot, RepositoryStructureCacheState, RepositorySummary,
    SystemLogLocation, SystemLogPage, SystemLogRecord, SystemLogSource,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;
use crate::shell::admin::{AdminEffect, AdminMessage, LogsView};
use crate::shell::files::FileDialog;
use crate::shell::player::PlayerEffect;
use crate::shell::route_startup::StartupView;
use crate::shell::view_part_primary::RouteKey;
use crate::shell::view_part_sidebar::project::SidebarView;
use crate::shell::workspace::SettingsWait;
use crate::shell::{FilesEffect, FilesMessage, HardlinkPrompt};
use crate::shell::{
    ShellMessage, ShellPage, ShellViewModel, SidebarEffect, SidebarMessage, StartupStatus,
    WorkspaceEffect, WorkspacePanel,
};

mod panels;
mod startup;

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

/// 照产品启动走完：读资源库列表、同步、读摘要、读首屏目录，再回答首屏启动等的设置包和播放器类型（都是空的），
/// 进了工作区。
pub(super) fn started(repo_id: &str) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    start(&mut model, repo_id);
    model
}

/// 在已有的壳层上走一遍启动，见 [`started`]。
pub(super) fn start(model: &mut ShellViewModel, repo_id: &str) {
    launch(model, repo_id);
    answer_settings(model);
}

/// 照产品启动走到首屏目录：读资源库列表、同步、读摘要、读首屏目录。每一步之前先清掉管理副作用，断言读完首屏目录
/// 以前没有提前读设置包；读完以后启动停在第 4 步等设置包。排下的别的管理读取留着。
pub(super) fn launch(model: &mut ShellViewModel, repo_id: &str) {
    let generation = model.workspace.prepare_initial_list();
    let steps = [
        ShellMessage::WorkspaceListLoaded {
            generation,
            result: Ok(vec![summary(repo_id, "ready")]),
        },
        ShellMessage::RepositorySynced {
            generation,
            result: Ok(crate::shell::SyncCounts::default()),
        },
        ShellMessage::RepositorySnapshotLoaded {
            generation,
            result: Ok(snapshot(repo_id)),
        },
    ];
    let mut early = Vec::new();
    for message in steps {
        model.reduce(message);
        early.extend(model.admin.take_effects());
    }
    assert!(
        !early
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)),
        "读完首屏目录以前就读了设置包"
    );
    for effect in early {
        model.admin.push_effect(effect);
    }
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(repo_id))));
    assert_eq!(
        model.workspace.startup.status,
        StartupStatus::Loading,
        "首屏启动还在等设置包"
    );
    assert_eq!(model.workspace.startup.settings, SettingsWait::Bundle);
}

/// 回答首屏启动在等的设置包（没有插件）和插件列表到了以后的播放器类型（没有贡献），启动就此结束。
/// 排下的别的管理读取（仓库动作）留着，测试接着看。
pub(super) fn answer_settings(model: &mut ShellViewModel) {
    let mut kept = model.admin.take_effects();
    let before = kept.len();
    kept.retain(|effect| !matches!(effect, AdminEffect::LoadSettingsBundle));
    assert_eq!(before - kept.len(), 1, "首屏启动在等设置包");
    model.reduce(bundle(Vec::new()));
    let mut after = model.admin.take_effects();
    let before = after.len();
    after.retain(|effect| !matches!(effect, AdminEffect::LoadPlaylistPlayers));
    assert_eq!(before - after.len(), 1, "插件列表到了读播放器类型");
    kept.extend(after);
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(Vec::new())));
    kept.extend(model.admin.take_effects());
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    for effect in kept {
        model.admin.push_effect(effect);
    }
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
        "sdk": "backend",
        "entry": {},
        "contributes": contributes,
        "source": "builtin",
        "runtime": "manifest-only",
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

pub(super) fn bundle(plugins: Vec<PluginManifest>) -> ShellMessage {
    ShellMessage::Admin(AdminMessage::SettingsBundleLoaded {
        plugins: Ok(plugins),
        hooks: Ok(Vec::new()),
        api: Ok(ApiDesignSnapshot {
            transport: "native".into(),
            endpoints: Vec::new(),
        }),
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
    ShellMessage::Sidebar(SidebarMessage::SidebarPlaylistsLoaded {
        repo_id: repo_id.into(),
        result: Ok(playlists),
    })
}

fn log_record(id: &str, timestamp: &str) -> SystemLogRecord {
    SystemLogRecord {
        id: id.into(),
        timestamp: timestamp.into(),
        level: "info".into(),
        category: "repository".into(),
        action: "sync".into(),
        message: format!("日志 {id}"),
        source: SystemLogSource {
            kind: "host".into(),
            label: None,
            plugin_id: None,
            repo_id: None,
        },
        location: SystemLogLocation::default(),
        context: serde_json::json!({}),
    }
}

fn repository_action(action_id: &str) -> RepositoryAction {
    RepositoryAction {
        action_id: action_id.into(),
        repo_id: REPO.into(),
        source: "eagle".into(),
        source_action_id: None,
        name: "整理导入".into(),
        status: "ready".into(),
        enabled: true,
        raw: serde_json::Value::Null,
        unsupported_reason: None,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
        steps: Vec::new(),
        last_run: None,
    }
}
