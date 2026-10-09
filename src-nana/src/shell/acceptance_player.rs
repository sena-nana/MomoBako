//! 预览与播放的对照场景：播放集页、底部播放条、当前队列和预览页。
//!
//! 场景名和 `tmp/vue-mock/scenes/player.ts` 同名，数据和 Vue 夹具保持一致：
//! 演示播放列表、两首 `music/track-0N.mp3`、内置插件清单登记的播放器，侧栏播放集默认收起。
//! 工作区从 `acceptance_base.rs` 的共用底子起步，播放集也经侧栏的读取结果进来。
//! 15 页里的「播放集」「播放中」也由这里填数据，状态都走产品归约，不手写会话字段。
//! 预览场景的文件字节（`preview_fixtures/cover.png`、`audio_fixtures/tone.mp3`、文本）和 Vue 夹具逐字节相同。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{
    AssetDetail, AssetSummary, FilePreviewSourceResponse, MetadataEntry, PlaylistDetail, PlaylistItem,
    PlaylistPlayerContribution, PlaylistSummary,
};

use super::super::inspect::InspectMessage;
use super::super::player::{PlayerEffect, PlayerMessage};
use super::super::InspectEffect;
use super::super::{ShellMessage, ShellPage, ShellViewModel, SidebarMessage};
use super::base_scene::{playlist_players, tagged_file, Base, NOW, REPO_ID};

const PLAYLIST_ID: &str = "playlist-demo";
/// 文本预览的内容。Vue 场景的 `SETTINGS_JSON` 是同一段。
const TEXT_FIXTURE: &str = "{\n  \"name\": \"MomoBako\",\n  \"theme\": \"light\",\n  \"corners\": \"smooth\"\n}\n";
/// 图片预览的字节。Vue 场景的 `COVER_PNG` 解出来是同一份。
const COVER_PNG: &[u8] = include_bytes!("preview_fixtures/cover.png");
/// 音频预览的字节。Vue 场景的 `TONE_MP3` 解出来是同一份。
const TONE_MP3: &[u8] = include_bytes!("audio_fixtures/tone.mp3");
/// Vue 夹具里 `cover.png` 登记的大小。
const COVER_SIZE: i64 = 2_400_000;

/// 本面板的离屏对照场景。和 15 页同名的「playlists」「playback-running」不在这里登记。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("playlist-open", playlist_open_scene()),
        ("playback-queue", playback_queue_scene()),
        ("still-playback", still_playback_scene()),
        ("outside-playback", outside_playback_scene()),
        ("live-preview", live_preview_scene()),
        ("live-asmr", live_asmr_scene()),
        ("preview-text", preview_text_scene()),
        ("preview-image", preview_image_scene()),
        ("preview-audio", preview_audio_scene()),
        ("preview-audio-failed", preview_audio_failed_scene()),
    ]
}

/// 15 页的播放集：点开了演示播放列表，详情还在读取，主区停在「选择一个播放集」。
pub(super) fn seed_playlists(model: &mut ShellViewModel) {
    demo_base().seed(model);
    // Vue 在打开新建对话框时才选默认播放器类型，页面上还没有选中值。
    model.selected_new_playlist_player_type_id = None;
    open_playlist(model, &demo_summary(), None);
}

/// 15 页的播放中：点开演示播放列表后双击第一条。`audio` 类型没有播放插件，
/// 会话停在「缺少对应播放插件」，播放意图仍在，和 Vue 的 `isPlaying` 一致。
pub(super) fn seed_playback(model: &mut ShellViewModel) {
    demo_base().seed(model);
    open_playlist(model, &demo_summary(), Some(demo_items()));
    play_first(model);
    model.page = ShellPage::PlaybackRunning;
}

/// 只点开播放集，不开始播放。页面身份记成「播放中」，离屏验收按它找标题「演示播放列表」。
fn playlist_open_scene() -> ShellViewModel {
    let mut model = demo_base().model();
    open_playlist(&mut model, &demo_summary(), Some(demo_items()));
    model.page = ShellPage::PlaybackRunning;
    model
}

/// Vue `playlistOpen()` 的底子：`base()` 加上演示播放列表。
fn demo_base() -> Base {
    Base { playlists: vec![demo_summary()], ..Base::default() }
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
    let mut model = single_item_playback(summary, "media/missing.png", playlist_players());
    fail_pending_loads(&mut model);
    model
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
    let mut players = playlist_players();
    players.push(foreign_player());
    single_item_playback(summary, "media/voice.wma", players)
}

/// Vue `singleItemPlayback`：只有一个条目的播放集，点开后按页眉的「播放」从第一条开始。
fn single_item_playback(summary: PlaylistSummary, path: &str, players: Vec<PlaylistPlayerContribution>) -> ShellViewModel {
    let mut model = Base { playlists: vec![summary.clone()], players, ..Base::default() }.model();
    open_playlist(&mut model, &summary, Some(vec![item(0, path)]));
    play_first(&mut model);
    model.page = ShellPage::PlaybackRunning;
    model
}

/// 把排着的当前项读取请求按「仓库里没有这个文件」送回。其它副作用放回原处。
fn fail_pending_loads(model: &mut ShellViewModel) {
    let mut kept = Vec::new();
    for effect in model.player.take_effects() {
        let PlayerEffect::LoadItem { item_id, path, still, generation, .. } = effect else {
            kept.push(effect);
            continue;
        };
        let result = Err(format!("无法读取当前项：验收仓库里没有 {path}"));
        model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded { item_id, generation, still, result }));
    }
    model.player.requeue_effects(kept);
}

/// 点开侧栏里的播放集（Vue 侧栏的播放集默认收起，点的是收起态里那颗按钮），再按夹具送回详情；
/// `items` 为空时详情还没回来。
fn open_playlist(model: &mut ShellViewModel, summary: &PlaylistSummary, items: Option<Vec<PlaylistItem>>) {
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSidebarPlaylist(summary.playlist_id.clone())));
    if model.sidebar.active_playlist_id.as_deref() != Some(summary.playlist_id.as_str()) {
        eprintln!("Nana 验收场景没有活动仓库，播放集未打开：{}", summary.playlist_id);
        return;
    }
    // 详情请求在这里按夹具回答，宿主不会再派发。
    model.sidebar.take_effects();
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

const FOREIGN_TYPE: &str = "momobako.playlist.foreign";

/// 第三方插件登记的 wma 播放器。Vue 场景用同一份清单挂一个测试插件。
fn foreign_player() -> PlaylistPlayerContribution {
    PlaylistPlayerContribution {
        player_type_id: FOREIGN_TYPE.into(),
        label: "外部条目".into(),
        file_class: "audio".into(),
        supported_extensions: vec!["wma".into()],
        supports_seek: false,
        supports_volume: false,
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

/// ASMR 音频预览：根目录的 voice.mp3，元数据里只有库类型和条目类型，不写歌词、时长、封面或色板。
/// 和音频预览一样读好后接管播放条；资源库扩展在预览框下面画作品队列。
fn live_asmr_scene() -> ShellViewModel {
    let metadata = [("libraryKind", "asmr"), ("asmrEntryKind", "audio")];
    let mut model = preview_scene_with("voice.mp3", TONE_MP3.len() as i64, &metadata);
    answer_media(&mut model);
    model
}

/// 预览场景的起点：Vue `previewScene` 在 `base()` 的根目录里补上这个文件（仓库摘要的素材跟着多一条），
/// 像双击一样选中它并读回素材详情。之后的读取请求由各场景用夹具字节回答，和宿主派发后送回的消息一样走产品归约。
fn preview_scene(path: &str, size_bytes: i64) -> ShellViewModel {
    preview_scene_with(path, size_bytes, &[])
}

/// 同上，文件条目和素材详情都带上给定的文本元数据（Vue 夹具里条目的 `metadata`）。
fn preview_scene_with(path: &str, size_bytes: i64, metadata: &[(&str, &str)]) -> ShellViewModel {
    let mut base = Base::default();
    if !base.entries.iter().any(|entry| entry.path == path) {
        let metadata = metadata.iter().map(|(key, value)| ((*key).to_string(), Value::String((*value).into()))).collect::<BTreeMap<_, _>>();
        base.entries.push(tagged_file(path, size_bytes, &[], metadata));
    }
    let mut model = base.model();
    let asset_id = path.replace('/', "-");
    let mut detail = preview_detail(path, size_bytes);
    detail.metadata = metadata
        .iter()
        .map(|(key, value)| MetadataEntry {
            key: (*key).into(),
            value_type: "string".into(),
            value: serde_json::Value::String((*value).into()),
            version: 1,
            updated_at: NOW.into(),
        })
        .collect();
    model.reduce(ShellMessage::SelectFile { path: path.into(), asset_id: Some(asset_id) });
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(detail)));
    model
}

/// 素材详情和 Vue 夹具 `file()` / `asset()` 同一组字段：大小写成「N B」，修改时间是 NOW。
fn preview_detail(path: &str, size_bytes: i64) -> AssetDetail {
    let filename = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = filename.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default();
    AssetDetail {
        summary: AssetSummary {
            asset_id: path.replace('/', "-"),
            repo_id: REPO_ID.into(),
            path: path.into(),
            filename,
            extension,
            size_bytes,
            size_label: format!("{size_bytes} B"),
            status: "ready".into(),
            modified_at: NOW.into(),
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

/// 取走预览排下的读取请求，找出要回答的那一个。
fn take_effect<T>(model: &mut ShellViewModel, pick: impl Fn(InspectEffect) -> Option<T>) -> Option<T> {
    let found = model.inspect.take_effects().into_iter().find_map(pick);
    if found.is_none() {
        eprintln!("Nana 验收预览没有排下要回答的读取请求");
    }
    found
}

/// 文本预览：根目录的 settings.json。
fn preview_text_scene() -> ShellViewModel {
    let mut model = preview_scene("settings.json", TEXT_FIXTURE.len() as i64);
    let request = take_effect(&mut model, |effect| match effect {
        InspectEffect::LoadText { path, markdown, generation, .. } => Some((path, markdown, generation)),
        _ => None,
    });
    if let Some((path, markdown, generation)) = request {
        let result = Ok(crate::shell::prepare_text(TEXT_FIXTURE.as_bytes()));
        model.reduce(ShellMessage::Inspect(InspectMessage::BodyLoaded { path, markdown, generation, result }));
    }
    model
}

/// 图片预览：根目录的 cover.png。离屏会话不走宿主上传纹理，像素按内容图画。
fn preview_image_scene() -> ShellViewModel {
    let mut model = preview_scene("cover.png", COVER_SIZE);
    let request = take_effect(&mut model, |effect| match effect {
        InspectEffect::LoadImage { repo_id, path } => Some((repo_id, path)),
        _ => None,
    });
    if let Some((repo_id, path)) = request {
        let source = FilePreviewSourceResponse {
            repo_id,
            path,
            token: "preview-image".into(),
            source_url: None,
            local_path: None,
            media_type: "image/png".into(),
            size_bytes: COVER_SIZE,
            modified_at: Some(NOW.into()),
        };
        model.reduce(ShellMessage::PreviewPixelsLoaded { source, pixels: crate::shell::decode_preview_pixels(COVER_PNG) });
    }
    // 挂 file-preview 纹理槽会引用离屏没有注册的宿主纹理。
    model.preview_token = None;
    model
}

/// 音频预览：根目录的 track-01.mp3。读好后插成临时条目接管播放条；自动播放要宿主派发，
/// 截图停在 0:00 暂停，和 Vue 场景截图前把播放暂停到开头一致。
fn preview_audio_scene() -> ShellViewModel {
    let mut model = preview_scene("track-01.mp3", TONE_MP3.len() as i64);
    answer_media(&mut model);
    model
}

/// 音频预览解不开：`voice.opus` 的字节认不出容器，Nana 没有原生解码器。预览框里是失败浮层，写
/// 「无法预览该音频」和原因。Vue 的文件预览把这类失败写在播放条上，预览框里仍是唱片，没有对照图。
fn preview_audio_failed_scene() -> ShellViewModel {
    const BROKEN_AUDIO: &[u8] = b"not an audio container";
    let mut model = preview_scene("voice.opus", BROKEN_AUDIO.len() as i64);
    let request = take_effect(&mut model, |effect| match effect {
        InspectEffect::LoadMedia { path, generation, .. } => Some((path, generation)),
        _ => None,
    });
    if let Some((path, generation)) = request {
        // 和宿主一样经 `decode_media` 解码，失败原因来自真实的解码层。
        let result = crate::shell::player::decode_media(REPO_ID, "opus", BROKEN_AUDIO).map(|parts| parts.session);
        model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded { path, generation, result, pcm: None, frames: None }));
    }
    model
}

/// 用音频夹具回答预览排下的音视频读取，和宿主读完解码后送回的消息一样。
fn answer_media(model: &mut ShellViewModel) {
    let request = take_effect(model, |effect| match effect {
        InspectEffect::LoadMedia { path, generation, .. } => Some((path, generation)),
        _ => None,
    });
    let Some((path, generation)) = request else {
        return;
    };
    match crate::shell::preview_media_parts(REPO_ID, TONE_MP3) {
        Ok(parts) => model.reduce(ShellMessage::Inspect(InspectMessage::MediaLoaded {
            path,
            generation,
            result: Ok(parts.session),
            pcm: parts.pcm,
            frames: parts.frames,
        })),
        Err(error) => eprintln!("Nana 验收音频夹具解不开：{error}"),
    }
}
