//! 预览与播放的对照场景：播放集页、底部播放条、当前队列和预览页。
//!
//! 场景名和 `tmp/vue-mock/scenes/player.ts` 同名，数据和 Vue 夹具保持一致：
//! 演示播放列表、两首 `music/track-0N.mp3`、三个官方播放器贡献，侧栏播放集默认收起。
//! 15 页里的「播放集」「播放中」也由这里填数据，状态都走产品归约，不手写会话字段。

use crate::backend::services::repository::{PlaylistDetail, PlaylistItem, PlaylistPlayerContribution, PlaylistSummary};

use super::super::player::PlayerMessage;
use super::super::sidebar::SidebarPlaylist;
use super::super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::REPO_ID;

const PLAYLIST_ID: &str = "playlist-demo";
const NOW: &str = "2026-10-08T08:00:00Z";

/// 本面板的离屏对照场景。和 15 页同名的「playlists」「playback-running」不在这里登记。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("playlist-open", playlist_open_scene()),
        ("playback-queue", playback_queue_scene()),
        ("still-playback", still_playback_scene()),
        ("outside-playback", outside_playback_scene()),
        ("live-preview", live_preview_scene()),
        ("live-asmr", live_asmr_scene()),
    ]
}

/// 15 页的播放集：点开了演示播放列表，详情还在读取，主区停在「选择一个播放集」。
pub(super) fn seed_playlists(model: &mut ShellViewModel) {
    load_official_players(model);
    // Vue 在打开新建对话框时才选默认播放器类型，页面上还没有选中值。
    model.selected_new_playlist_player_type_id = None;
    open_playlist(model, &demo_summary(), None);
}

/// 15 页的播放中：点开演示播放列表后双击第一条。`audio` 类型没有播放插件，
/// 会话停在「缺少对应播放插件」，播放意图仍在，和 Vue 的 `isPlaying` 一致。
pub(super) fn seed_playback(model: &mut ShellViewModel) {
    load_official_players(model);
    open_playlist(model, &demo_summary(), Some(demo_items()));
    play_first(model);
    model.page = ShellPage::PlaybackRunning;
}

/// 只点开播放集，不开始播放。页面身份记成「播放中」，离屏验收按它找标题「演示播放列表」。
fn playlist_open_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::Playlists);
    open_playlist(&mut model, &demo_summary(), Some(demo_items()));
    model.page = ShellPage::PlaybackRunning;
    model
}

/// 播放中打开当前队列浮层。
fn playback_queue_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::PlaybackRunning);
    model.reduce(ShellMessage::Player(PlayerMessage::ToggleQueue));
    model.page = ShellPage::PlaybackRunning;
    model
}

/// 图片幻灯片播放 `media/missing.png`。文件不存在，会话停在「图片无法播放」，
/// 停留时长和适应 / 填充设置照常出现。
fn still_playback_scene() -> ShellViewModel {
    let summary = PlaylistSummary {
        player_type_id: "momobako.playlist.image-slideshow".into(),
        player_plugin_id: "momobako.preview.media".into(),
        player_label: "图片幻灯片".into(),
        file_class: "image".into(),
        item_count: 1,
        ..demo_summary()
    };
    single_item_playback(summary, "media/missing.png")
}

/// 外部播放器贡献的 wma。Nana 没有对应运行时，播放条写明不支持，不编造时长。
fn outside_playback_scene() -> ShellViewModel {
    let summary = PlaylistSummary {
        player_type_id: FOREIGN_TYPE.into(),
        player_plugin_id: "momobako.player.foreign".into(),
        player_label: "外部条目".into(),
        file_class: "audio".into(),
        item_count: 1,
        ..demo_summary()
    };
    let mut model = ShellViewModel::for_page(ShellPage::Playlists);
    let mut players = official_players();
    players.push(foreign_player());
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(players)));
    open_playlist(&mut model, &summary, Some(vec![item(0, "media/voice.wma")]));
    play_first(&mut model);
    model.page = ShellPage::PlaybackRunning;
    model
}

/// 只有一个条目的播放集，点开后播放第一条。
fn single_item_playback(summary: PlaylistSummary, path: &str) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::Playlists);
    load_official_players(&mut model);
    open_playlist(&mut model, &summary, Some(vec![item(0, path)]));
    play_first(&mut model);
    model.page = ShellPage::PlaybackRunning;
    model
}

/// 侧栏登记播放集并点开它；`items` 为空时详情还没回来。
fn open_playlist(model: &mut ShellViewModel, summary: &PlaylistSummary, items: Option<Vec<PlaylistItem>>) {
    model.sidebar.playlists = vec![SidebarPlaylist {
        id: summary.playlist_id.clone(),
        name: summary.name.clone(),
        player_label: summary.player_label.clone(),
        player_type_id: summary.player_type_id.clone(),
        item_count: summary.item_count,
    }];
    // Vue 侧栏的播放集默认收起，点的是收起态里那颗按钮。
    model.sidebar.playlists_expanded = false;
    if !model.sidebar.select_playlist(&mut model.workspace, &summary.playlist_id) {
        eprintln!("Nana 验收场景没有活动仓库，播放集未打开：{}", summary.playlist_id);
        return;
    }
    model.reduce(ShellMessage::SelectPlaylist(summary.playlist_id.clone()));
    model.workspace.panel = WorkspacePanel::Playlist;
    if let Some(items) = items {
        let detail = PlaylistDetail { playlist: summary.clone(), items };
        model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail)));
    }
}

/// 双击第一条：从它开始播放整张列表。
fn play_first(model: &mut ShellViewModel) {
    let first = model.player.listed.as_ref().and_then(|detail| detail.items.first()).map(|item| item.playlist_item_id.clone());
    let Some(first) = first else {
        eprintln!("Nana 验收播放集是空的，没有可播放条目");
        return;
    };
    model.reduce(ShellMessage::Player(PlayerMessage::PlayListed { item_id: Some(first) }));
}

/// 三个官方插件登记的播放器：音频顺序播放、图片幻灯片、视频顺序播放。
fn load_official_players(model: &mut ShellViewModel) {
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(official_players())));
}

fn official_players() -> Vec<PlaylistPlayerContribution> {
    let audio = ["mp3", "wav", "ogg", "flac", "m4a", "aac", "opus"];
    let image = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "avif", "svg"];
    let video = ["mp4", "mov", "mkv", "webm", "avi", "m4v"];
    vec![
        contribution("momobako.playlist.audio-sequence", "音频顺序播放", "audio", &audio, true),
        contribution("momobako.playlist.image-slideshow", "图片幻灯片", "image", &image, false),
        contribution("momobako.playlist.video-sequence", "视频顺序播放", "video", &video, true),
    ]
}

const FOREIGN_TYPE: &str = "momobako.playlist.foreign";

/// 第三方插件登记的 wma 播放器。Vue 场景用同一份清单挂一个测试插件。
fn foreign_player() -> PlaylistPlayerContribution {
    contribution(FOREIGN_TYPE, "外部条目", "audio", &["wma"], false)
}

fn contribution(player_type_id: &str, label: &str, file_class: &str, extensions: &[&str], media: bool) -> PlaylistPlayerContribution {
    PlaylistPlayerContribution {
        player_type_id: player_type_id.into(),
        label: label.into(),
        file_class: file_class.into(),
        supported_extensions: extensions.iter().map(|extension| (*extension).to_string()).collect(),
        supports_seek: media,
        supports_volume: media,
        supports_preview_navigation: true,
        description: None,
    }
}

fn demo_summary() -> PlaylistSummary {
    PlaylistSummary {
        playlist_id: PLAYLIST_ID.into(),
        repo_id: REPO_ID.into(),
        name: "演示播放列表".into(),
        player_type_id: "audio".into(),
        player_plugin_id: "momobako.player.audio".into(),
        player_label: "音频".into(),
        file_class: "audio".into(),
        item_count: 2,
        sort_order: 0,
        created_at: NOW.into(),
        updated_at: NOW.into(),
    }
}

fn demo_items() -> Vec<PlaylistItem> {
    vec![item(0, "music/track-01.mp3"), item(1, "music/track-02.mp3")]
}

/// 播放列表条目。编号从 01 开始，和 Vue 夹具的 `item-01` 一致。
fn item(index: usize, path: &str) -> PlaylistItem {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = filename.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default();
    PlaylistItem {
        playlist_item_id: format!("item-{:02}", index + 1),
        playlist_id: PLAYLIST_ID.into(),
        asset_id: format!("asset-{}", index + 1),
        path: path.into(),
        filename,
        extension,
        thumbnail_path: None,
        status: "ready".into(),
        status_reason: None,
        sort_order: index as i64,
        added_at: NOW.into(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        metadata: None,
        local_absolute_path: None,
    }
}

/// 文件列让开后的 PDF 页和元数据。页面文字来自实况 PDF 解析，不编造页数。
/// 页位图走宿主纹理槽，离屏会话没有 `Application::prepare` 上传，这里不挂空纹理。
fn live_preview_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    super::seed_browser(&mut model);
    model.files.display_mode = super::super::files::DisplayMode::List;
    let path = "notes/page.pdf";
    model.inspect.begin_selection(path);
    super::super::inspect::native_preview::begin(&mut model.inspect, REPO_ID, path, "momobako.preview.pdf".into(), "PDF".into());
    let loaded = super::super::inspect::native_preview::load("momobako.preview.pdf", super::PAGE_PDF).expect("验收 PDF");
    let generation = model.inspect.generation;
    model.reduce(ShellMessage::Inspect(super::super::inspect::InspectMessage::NativeLoaded {
        path: path.into(),
        generation,
        result: Ok(loaded),
    }));
    model.inspect.facts = super::super::inspect::FileFacts {
        extension: "pdf".into(),
        size_label: format!("{} B", super::PAGE_PDF.len()),
        ..super::super::inspect::FileFacts::default()
    };
    model.reduce(ShellMessage::Inspect(super::super::inspect::InspectMessage::SetComment("PDF 页".into())));
    // 页位图留在解码结果里。离屏会话不走 prepare，挂 file-preview 会引用未注册的宿主纹理。
    model.preview_pixels = None;
    model.preview_token = None;
    model
}

/// ASMR 音频预览。只有库类型和条目类型，不写歌词、时长、封面或色板。
fn live_asmr_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    let path = "works/voice.mp3";
    let mut row = super::file_row(path, "file");
    row.metadata.insert("libraryKind".into(), serde_json::Value::String("asmr".into()));
    row.metadata.insert("asmrEntryKind".into(), serde_json::Value::String("audio".into()));
    model.files.rows = vec![row];
    model.files.total_entries = 1;
    model.files.set_drag_selection(vec![path.into()], Some(path.into()), Some(path.into()));
    model.selected_path = Some(path.into());
    model.detail = path.into();
    model.inspect.begin_selection(path);
    model.inspect.loading = false;
    model.inspect.activity.clear();
    model.inspect.facts.extension = "mp3".into();
    model
}
