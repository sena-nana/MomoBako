//! 来源资源库的对照场景：网易云这类来源资源库里单曲的右键菜单，和「下载单曲到其他资源库」弹出的「选择目标资源库」；
//! 以及在线单曲（`is_virtual`，本地没有文件）准备播放源时的预览页和播放条。
//!
//! 当前资源库「网易云音乐」挂在网易云来源插件下，本地缓存在 `D:/MomoCache/netease`；根目录一首「晴天.mp3」，来源
//! 载荷是单曲（`entryKind: track`、`songId: 186016`）。插件是仓库自带的全部清单（`acceptance_base.rs`），网易云的
//! 条目动作照清单出：下载单曲（到本地、到其他资源库）、重新获取播放资源、清理播放缓存。状态都走产品的启动、
//! 右键和菜单消息。在线单曲的场景照产品先排下 `momobako.playback.prepare` 的读取请求，再按夹具送回失败，
//! 失败原因是网易云插件交回的原话（`External/Plugins/source-netease-cloud-music/src/client.rs`）。

use crate::backend::services::repository::{FileBrowserEntry, PlaylistItem, PlaylistSummary, RepositorySummary};

use super::super::files::FilesMessage;
use super::super::files_view::EntryMenuView;
use super::super::inspect::InspectMessage;
use super::super::player::{PlayerEffect, PlayerMessage};
use super::super::{InspectEffect, ShellMessage, ShellPage, ShellViewModel, SourcePrep};
use super::base_scene::{file, playlist_players, settle, summary, Base, SourceState, NOW, REPO_ID};
use super::files_scenes::asset_detail;
use super::player_scenes::{open_playlist, play_first};

/// 右键落点，和文件页的右键场景一样。
const MENU_POINT: (f32, f32) = (400.0, 470.0);
/// 根目录里的单曲。
const TRACK: &str = "晴天.mp3";
/// 网易云插件取不到播放地址时交回的原因：登录失效、网络超时。
const LOGIN_EXPIRED: &str = "网易云登录已失效，请重新登录";
const TIMEOUT: &str = "网易云接口请求超时，请检查网络后重试";

/// 本面板的离屏对照场景。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("source-menu-unavailable", unavailable_menu()),
        ("source-download-target", download_target()),
        ("online-preview-preparing", online_preview_preparing()),
        ("online-preview-failed", online_preview_failed()),
        ("online-playback-failed", online_playback_failed()),
    ]
}

/// 网易云的单曲：来源载荷带条目种类和歌曲标识。
fn track() -> FileBrowserEntry {
    let mut entry = file(TRACK, 4_200_000);
    entry.provider_id = Some("netease-cloud-music".into());
    entry.provider_item_id = Some("186016".into());
    entry.source_payload = Some(serde_json::json!({ "entryKind": "track", "songId": 186016, "songName": "晴天" }));
    entry
}

/// 在线单曲：和网易云目录给的一样是虚拟条目、大小 0，本地没有文件。
fn online_track() -> FileBrowserEntry {
    let mut entry = track();
    entry.is_virtual = true;
    entry.size_bytes = Some(0);
    entry.size_label = Some("0 B".into());
    entry
}

/// 资源库列表里能当下载目标的另一个本地资源库。
fn other(repo_id: &str, name: &'static str, path: &str) -> RepositorySummary {
    RepositorySummary { repo_id: repo_id.into(), name: name.into(), path: path.into(), ..summary(name, "ready") }
}

/// 网易云音乐资源库的底子，登录状态照 `logged_in` / `login_expired`。
fn netease_base(entries: Vec<FileBrowserEntry>, logged_in: bool, login_expired: bool) -> Base {
    Base {
        name: "网易云音乐",
        entries,
        tree: Vec::new(),
        source: Some(SourceState {
            plugin_id: "momobako.netease.source",
            kind: "netease-cloud-music",
            cache_path: "D:/MomoCache/netease",
            logged_in,
            login_expired,
        }),
        ..Base::default()
    }
}

/// 网易云音乐资源库，登录状态照 `logged_in` / `login_expired`；`others` 是列表里别的资源库。
fn netease(logged_in: bool, login_expired: bool, others: Vec<RepositorySummary>) -> ShellViewModel {
    Base { others, ..netease_base(vec![track()], logged_in, login_expired) }.model()
}

/// 右键单曲：选中它、打开菜单，再照 Vue 模拟 IPC 答上右键选中时读的素材详情。
fn open_menu(model: &mut ShellViewModel) {
    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu { path: TRACK.into(), x: MENU_POINT.0, y: MENU_POINT.1 }));
    match model.files.rows.iter().find(|row| row.path == TRACK).cloned() {
        Some(row) => model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(&row)))),
        None => eprintln!("Nana 来源场景里没有 {TRACK}"),
    }
    let _ = model.inspect.take_effects();
}

/// 登录过期：照 Vue `actionDisabled`，网易云的条目动作都禁用，后面一行写「来源认证或本地缓存不可用」。
fn unavailable_menu() -> ShellViewModel {
    let mut model = netease(true, true, vec![other("second-repo", "第二资源库", "C:/second")]);
    open_menu(&mut model);
    settle(&mut model);
    model
}

/// 登录正常、列表里还有两个本地资源库：右键单曲 →「下载单曲到其他资源库」，弹出「选择目标资源库」列出这两个。
fn download_target() -> ShellViewModel {
    let others = vec![other("second-repo", "第二资源库", "C:/second"), other("third-repo", "第三资源库", "D:/third")];
    let mut model = netease(true, false, others);
    open_menu(&mut model);
    let action = EntryMenuView::project(&model)
        .and_then(|menu| menu.entries.into_iter().find(|entry| entry.label == "下载单曲到其他资源库"))
        .and_then(|entry| entry.action);
    match action {
        Some(action) => {
            model.reduce(ShellMessage::Files(FilesMessage::CloseEntryMenu));
            model.reduce(action.message());
        }
        None => eprintln!("Nana 来源场景的右键菜单里没有「下载单曲到其他资源库」"),
    }
    settle(&mut model);
    model
}

/// 双击在线单曲打开预览：素材详情到了，预览照产品排下准备播放源的读取请求。交回这个请求（文件和代次）。
fn open_online_preview(model: &mut ShellViewModel) -> Option<(String, u64)> {
    let Some(row) = model.files.rows.iter().find(|row| row.path == TRACK).cloned() else {
        eprintln!("Nana 在线单曲场景里没有 {TRACK}");
        return None;
    };
    model.reduce(ShellMessage::SelectFile { path: TRACK.into(), asset_id: row.asset_id.clone() });
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(asset_detail(&row))));
    let request = model.inspect.take_effects().into_iter().find_map(|effect| match effect {
        InspectEffect::LoadMedia { path, generation, virtual_entry: true, .. } => Some((path, generation)),
        _ => None,
    });
    if request.is_none() {
        eprintln!("Nana 在线单曲的预览没有排下准备播放源的请求");
    }
    request
}

/// 预览在线单曲，播放源还在准备（来源插件在下载）：预览框是准备卡，写文件名和「准备音频」，进度不定；
/// 播放条还没被接管。和 Vue 经 Tauri 拿到进度时一样，准备期间看不到中间阶段。
fn online_preview_preparing() -> ShellViewModel {
    let mut model = netease_base(vec![online_track()], true, false).model();
    open_online_preview(&mut model);
    settle(&mut model);
    model
}

/// 预览在线单曲，准备播放源失败（网易云说登录失效）：预览框里是失败浮层，红色标题「无法预览该音频」，下面是
/// 插件交回的原话和「重试」。播放条不接管，照旧是「未选择播放内容」。
fn online_preview_failed() -> ShellViewModel {
    let mut model = netease_base(vec![online_track()], true, false).model();
    if let Some((path, generation)) = open_online_preview(&mut model) {
        let result = Err(LOGIN_EXPIRED.to_string());
        let prep = SourcePrep::failed();
        model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded { path, generation, result, pcm: None, frames: None, prep }));
    }
    settle(&mut model);
    model
}

/// 网易云歌单里的在线单曲，按页眉的「播放」开始放，准备播放源时网络超时：播放条第二行是插件交回的原话，
/// 「播放」留着可点（再点就是重试），上一首、下一首不可点。
fn online_playback_failed() -> ShellViewModel {
    let playlist = online_playlist();
    let mut model = Base { playlists: vec![playlist.clone()], players: playlist_players(), ..netease_base(vec![online_track()], true, false) }.model();
    open_playlist(&mut model, &playlist, Some(vec![online_item(&playlist)]));
    play_first(&mut model);
    let mut kept = Vec::new();
    for effect in model.player.take_effects() {
        match effect {
            PlayerEffect::LoadItem { item_id, still, generation, virtual_entry: true, .. } => {
                let result = Err(TIMEOUT.to_string());
                let prep = SourcePrep::failed();
                model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded { item_id, generation, still, result, prep }));
            }
            effect => kept.push(effect),
        }
    }
    model.player.requeue_effects(kept);
    model.page = ShellPage::PlaybackRunning;
    settle(&mut model);
    model
}

/// 网易云歌单建成的播放集，类型是音频播放器插件登记的「音频顺序播放」。
fn online_playlist() -> PlaylistSummary {
    PlaylistSummary {
        playlist_id: "netease-cloud-music-8001".into(),
        repo_id: REPO_ID.into(),
        name: "我喜欢的音乐".into(),
        player_type_id: "momobako.playlist.audio-sequence".into(),
        player_plugin_id: "momobako.player.audio".into(),
        player_label: "音频顺序播放".into(),
        file_class: "audio".into(),
        item_count: 1,
        sort_order: 0,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

/// 播放集里的在线单曲。
fn online_item(playlist: &PlaylistSummary) -> PlaylistItem {
    let entry = online_track();
    PlaylistItem {
        playlist_item_id: "item-01".into(),
        playlist_id: playlist.playlist_id.clone(),
        asset_id: entry.asset_id.clone().unwrap_or_default(),
        path: entry.path.clone(),
        filename: entry.name.clone(),
        extension: "mp3".into(),
        thumbnail_path: None,
        status: "ready".into(),
        status_reason: None,
        sort_order: 0,
        added_at: NOW.into(),
        is_virtual: true,
        provider_id: entry.provider_id.clone(),
        provider_item_id: entry.provider_item_id.clone(),
        source_payload: entry.source_payload.clone(),
        metadata: None,
        local_absolute_path: None,
    }
}
