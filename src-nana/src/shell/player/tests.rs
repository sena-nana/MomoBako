//! 播放列表成员、回退、会话和共用播放条的状态机测试。
//!
//! 覆盖播放列表和播放条的各个分支。不启动仓库服务，也不写用户目录。

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FileBrowserSnapshot, PlaylistDetail, PlaylistItem,
    PlaylistMembershipSnapshot, PlaylistPlayerContribution, PlaylistSummary,
    RepositoryBackendSummary, RepositoryOverview, RepositorySnapshot,
    RepositoryStructureCacheState, RepositorySummary,
};

use super::super::workspace::{LibraryCategory, WorkspacePanel, WorkspaceRepository};
use super::super::{
    InspectEffect, InspectMessage, ShellMessage, ShellPage, ShellViewModel, SidebarMessage,
};
use super::support::{
    AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE, NextStep, OFFICIAL_AUDIO_PLUGIN,
    normalize_image_duration_ms, normalize_object_fit, normalize_volume, read_preferences_file,
    read_sessions_file, read_settings_file, resolution_notice, resolve_player, shuffle_order,
    system_media_session_available,
};
use super::{PlaybackMode, PlayerCandidate, PlayerEffect, PlayerMessage, QueueItem};

mod resolution;
mod sessions;

fn shell(writable: bool) -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.repository_id = Some("repo".into());
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: "repo".into(),
        name: "库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: if writable {
            vec!["write".into()]
        } else {
            Vec::new()
        },
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    });
    model.workspace.active_repo_id = Some("repo".into());
    model
}

/// 侧栏读回仓库 `repo` 的播放集列表。侧栏还没绑到这个仓库时先照产品绑上。
fn load_list(model: &mut ShellViewModel, playlists: Vec<PlaylistSummary>) {
    if model.sidebar.bound_repo_id() != Some("repo") {
        model.bind_sidebar_repository();
        model.sidebar.take_effects();
    }
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::SidebarPlaylistsLoaded {
            repo_id: "repo".into(),
            result: Ok(playlists),
        },
    ));
}

/// 发一条播放消息。当前项的读取请求像 `dispatch::player` 一样当场读文件送回。
fn send(model: &mut ShellViewModel, message: PlayerMessage) {
    model.reduce(ShellMessage::Player(message));
    super::fulfill_loads(model);
}

fn summary(
    repo_id: &str,
    playlist_id: &str,
    player_type_id: &str,
    file_class: &str,
) -> PlaylistSummary {
    PlaylistSummary {
        playlist_id: playlist_id.into(),
        repo_id: repo_id.into(),
        name: "早晨".into(),
        player_type_id: player_type_id.into(),
        player_plugin_id: player_type_id.into(),
        player_label: "音频".into(),
        file_class: file_class.into(),
        item_count: 1,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn item(id: &str, status: &str) -> PlaylistItem {
    PlaylistItem {
        playlist_item_id: id.into(),
        playlist_id: "pl".into(),
        asset_id: format!("asset-{id}"),
        path: format!("audio/{id}.mp3"),
        filename: format!("{id}.mp3"),
        extension: "mp3".into(),
        thumbnail_path: None,
        status: status.into(),
        status_reason: None,
        sort_order: 0,
        added_at: String::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        metadata: None,
        local_absolute_path: None,
    }
}

fn detail(
    repo_id: &str,
    playlist_id: &str,
    player_type_id: &str,
    file_class: &str,
    items: Vec<PlaylistItem>,
) -> PlaylistDetail {
    let mut playlist = summary(repo_id, playlist_id, player_type_id, file_class);
    playlist.item_count = items.len() as i64;
    PlaylistDetail { playlist, items }
}

fn candidate(
    plugin_id: &str,
    player_type_id: &str,
    class: &str,
    extensions: &[&str],
) -> PlayerCandidate {
    PlayerCandidate {
        plugin_id: plugin_id.into(),
        player_type_id: player_type_id.into(),
        capability_id: (player_type_id == AUDIO_SEQUENCE_TYPE)
            .then(|| AUDIO_CAPABILITY.to_string()),
        label: plugin_id.into(),
        file_class: class.into(),
        extensions: extensions.iter().map(|item| (*item).to_string()).collect(),
        supports_seek: true,
        supports_volume: true,
    }
}

fn contribution(
    player_type_id: &str,
    class: &str,
    extensions: &[&str],
) -> PlaylistPlayerContribution {
    PlaylistPlayerContribution {
        player_type_id: player_type_id.into(),
        label: "插件播放器".into(),
        file_class: class.into(),
        supported_extensions: extensions.iter().map(|item| (*item).to_string()).collect(),
        supports_seek: true,
        supports_volume: true,
        supports_preview_navigation: false,
        description: None,
    }
}

fn queue_item(id: &str, class: &str) -> QueueItem {
    QueueItem {
        id: id.into(),
        playlist_id: "pl".into(),
        asset_id: format!("asset-{id}"),
        path: format!("media/{id}.{class}"),
        filename: format!("{id}.{class}"),
        extension: class.into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: "audio".into(),
        player_label: "音频".into(),
        file_class: class.into(),
        thumbnail_path: None,
        is_virtual: false,
        local_absolute_path: None,
    }
}

fn load_playlist(model: &mut ShellViewModel, repo_id: &str, items: Vec<PlaylistItem>) {
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail(
        repo_id, "pl", "audio", "audio", items,
    ))));
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "momobako-nana-player-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("创建临时目录");
    path
}

fn asset(path: &str, extension: &str) -> AssetDetail {
    let filename = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
    AssetDetail {
        summary: AssetSummary {
            asset_id: "asset".into(),
            repo_id: "repo".into(),
            path: path.into(),
            filename,
            extension: extension.into(),
            size_bytes: 12,
            size_label: "12 B".into(),
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
        },
        metadata: Vec::new(),
        revisions: Vec::new(),
    }
}

mod membership;
mod playlist_cache;
mod wav;

/// 新建播放集默认选的播放类型：按名称的 zh-CN 顺序排
/// （拼音：视频 < 图片 < 音频），取第一项。宿主给的顺序不算数。
#[cfg(windows)]
#[test]
fn playlist_players_follow_the_name_order() {
    let mut model = shell(true);
    let player = |id: &str, label: &str| PlaylistPlayerContribution {
        label: label.into(),
        ..contribution(id, id, &[])
    };
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(vec![
        player("image", "图片幻灯片"),
        player("audio", "音频顺序播放"),
        player("video", "视频顺序播放"),
    ])));
    let order = model
        .playlist_players
        .iter()
        .map(|player| player.player_type_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(order, ["video", "image", "audio"]);
    assert_eq!(
        model.selected_new_playlist_player_type_id.as_deref(),
        Some("video")
    );
}
