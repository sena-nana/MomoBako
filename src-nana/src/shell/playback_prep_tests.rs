//! 在线条目准备播放源的状态机测试：预览和播放条遇到虚拟条目时排下准备请求，结果按代次丢弃，
//! 失败给重试，准备好时当场改「最近使用」；单击只看详情，不准备播放源。

use crate::backend::services::repository::{AssetDetail, AssetSummary, PlaylistItem};

use super::super::files::{FileRow, FilesMessage};
use super::super::inspect::{InspectMessage, PreviewBody};
use super::super::library::LibraryEffect;
use super::super::player::{decode_loaded, PlayerEffect, PlayerMessage};
use super::super::player_view::bar::props_for_test;
use super::super::{InspectEffect, ShellMessage, ShellViewModel};
use super::SourcePrep;

const REPO: &str = "acceptance-repo";
const TRACK: &str = "晴天.mp3";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 在线单曲的素材详情：虚拟条目，本地没有文件。
fn online_detail(row: &FileRow) -> AssetDetail {
    AssetDetail {
        summary: AssetSummary {
            asset_id: row.asset_id.clone().unwrap_or_default(),
            repo_id: REPO.into(),
            path: row.path.clone(),
            filename: row.name.clone(),
            extension: "mp3".into(),
            size_bytes: 0,
            size_label: "0 B".into(),
            status: "ready".into(),
            modified_at: row.modified_at.clone(),
            last_accessed_at: None,
            version: 1,
            tags: Vec::new(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: true,
            provider_id: row.provider_id.clone(),
            provider_item_id: row.provider_item_id.clone(),
            source_payload: row.source_payload.clone(),
            local_absolute_path: None,
        },
        metadata: Vec::new(),
        revisions: Vec::new(),
    }
}

fn track_row(model: &ShellViewModel) -> FileRow {
    model.files.rows.iter().find(|row| row.path == TRACK).cloned().expect("在线单曲在根目录")
}

/// 预览排下的音视频读取：文件、代次、是不是在线条目。
fn media_requests(model: &mut ShellViewModel) -> Vec<(String, u64, bool)> {
    model
        .inspect
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            InspectEffect::LoadMedia { path, generation, virtual_entry, .. } => Some((path, generation, virtual_entry)),
            _ => None,
        })
        .collect()
}

fn tone() -> Result<crate::backend::services::repository::PlaybackSessionState, String> {
    crate::shell::preview_media_parts(REPO, crate::shell::audio_decode::fixture_mp3()).map(|parts| parts.session)
}

fn media_loaded(path: &str, generation: u64, result: Result<crate::backend::services::repository::PlaybackSessionState, String>, prep: SourcePrep) -> ShellMessage {
    ShellMessage::Inspect(InspectMessage::MediaLoaded { path: path.into(), generation, result, pcm: None, frames: None, prep })
}

/// 单击在线单曲只在右侧详情里看元数据：不准备播放源（不下载）、不接管播放条；双击打开预览才准备，请求标明是在线条目。
#[test]
fn only_an_opened_preview_prepares_an_online_track() {
    let mut model = scene("online-preview-preparing");
    model.reduce(ShellMessage::OpenDirectory(String::new()));
    let row = track_row(&model);
    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(TRACK.into())));
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(online_detail(&row))));
    assert!(media_requests(&mut model).is_empty(), "单击不读预览内容，不准备播放源");
    assert!(model.player.current_item().is_none(), "单击不接管播放条");

    model.reduce(ShellMessage::Files(FilesMessage::OpenRow(TRACK.into())));
    model.reduce(ShellMessage::AssetDetailLoaded(Ok(online_detail(&row))));
    let requests = media_requests(&mut model);
    assert_eq!(requests.len(), 1);
    assert_eq!((requests[0].0.as_str(), requests[0].2), (TRACK, true), "在线条目先准备播放源");
}

/// 准备失败给「重试」：重试换一个代次重新准备，失败那一次晚到的结果丢掉；准备好时播放条接管这一项（仍记着是在线
/// 条目），「最近使用」当场加一。
#[test]
fn a_failed_preview_retries_with_a_new_generation() {
    let mut model = scene("online-preview-failed");
    assert!(model.inspect.media_retry, "准备失败可以重试");
    assert!(matches!(&model.inspect.body, PreviewBody::Media(session) if session.status == "failed"));
    let failed_generation = model.inspect.generation;
    assert_eq!(model.sidebar.counts.recent, 0);

    model.reduce(ShellMessage::Inspect(InspectMessage::RetryMedia));
    let requests = media_requests(&mut model);
    assert_eq!(requests.len(), 1, "重试排下新的准备请求");
    let (path, generation, virtual_entry) = requests[0].clone();
    assert!(virtual_entry && generation != failed_generation);
    assert!(model.inspect.loading && !model.inspect.media_retry);

    model.reduce(media_loaded(&path, failed_generation, Err("晚到的失败".into()), SourcePrep::failed()));
    assert!(model.inspect.loading, "上一次的结果过期，不装");

    let prep = SourcePrep::prepared(REPO, TRACK, "2026-10-11T08:00:00Z");
    model.reduce(media_loaded(&path, generation, tone(), prep));
    assert!(matches!(&model.inspect.body, PreviewBody::Media(session) if session.status == "paused"));
    let item = model.player.current_item().cloned().expect("播放条接管预览的这一项");
    assert!(item.transient && item.is_virtual && item.path == TRACK);
    assert_eq!(model.sidebar.counts.recent, 1, "播放源准备好了，最近使用当场加一");
    assert!(model.library.take_effects().is_empty(), "后端已经写了访问，不再另记");
}

/// 后端准备好播放源时已经写下访问：结果晚到（预览已经换了文件）也照样改「最近使用」，只是不装进预览。
#[test]
fn a_stale_prepared_source_still_counts_as_an_access() {
    let mut model = scene("online-preview-preparing");
    let generation = model.inspect.generation;
    model.reduce(ShellMessage::OpenDirectory(String::new()));
    let prep = SourcePrep::prepared(REPO, TRACK, "2026-10-11T08:00:00Z");
    model.reduce(media_loaded(TRACK, generation, tone(), prep));
    assert!(model.player.current_item().is_none(), "过期的结果不接管播放条");
    assert_eq!(model.sidebar.counts.recent, 1);
}

/// 播放条装在线条目先准备播放源；准备失败时第二行写来源的原话，「播放」留着可点，点了换代次重新准备，
/// 上一次晚到的结果丢掉；停止以后在途的结果也丢掉。
#[test]
fn a_failed_online_item_retries_from_the_play_button() {
    let mut model = scene("online-playback-failed");
    assert_eq!(model.player.session.status, "failed");
    assert_eq!(model.player.session.error.as_deref(), Some("网易云接口请求超时，请检查网络后重试"));
    let props = props_for_test(&model);
    assert!(props.retry && !props.can_play, "「播放」可点（重试），上一首、下一首不可点");
    assert_eq!(props.subtitle(), "网易云接口请求超时，请检查网络后重试");

    model.reduce(ShellMessage::Player(PlayerMessage::SetPlaying(true)));
    let load = |model: &mut ShellViewModel| {
        model.player.take_effects().into_iter().find_map(|effect| match effect {
            PlayerEffect::LoadItem { item_id, generation, virtual_entry, .. } => Some((item_id, generation, virtual_entry)),
            _ => None,
        })
    };
    let (item_id, generation, virtual_entry) = load(&mut model).expect("重试排下新的准备请求");
    assert!(virtual_entry);
    assert_eq!(model.player.session.status, "loading");
    assert!(!props_for_test(&model).retry, "重试进行中不再标可以重试");

    let stale = ShellMessage::Player(PlayerMessage::ItemLoaded {
        item_id: item_id.clone(),
        generation: generation.wrapping_sub(1),
        still: false,
        result: Err("晚到的失败".into()),
        prep: SourcePrep::failed(),
    });
    model.reduce(stale);
    assert_eq!(model.player.session.status, "loading", "上一次的结果过期，不装");

    model.reduce(ShellMessage::Player(PlayerMessage::Stop { repo_id: None, clear_stored: false }));
    let loaded = decode_loaded(REPO, false, "mp3", crate::shell::audio_decode::fixture_mp3()).expect("解码");
    let prep = SourcePrep::prepared(REPO, TRACK, "2026-10-11T08:00:00Z");
    model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded { item_id, generation, still: false, result: Ok(loaded), prep }));
    assert_eq!(model.player.session.status, "ended", "停止以后在途的播放源不装");
    assert_eq!(model.sidebar.counts.recent, 1, "后端已经写了访问，照样改最近使用");
}

/// 切到下一首时上一首还在准备：上一首晚到的播放源丢掉，不串到新曲目上；新曲目的结果照常装上。
#[test]
fn switching_tracks_drops_the_previous_prepared_source() {
    let mut model = scene("online-playback-failed");
    let mut detail = model.player.listed.clone().expect("点开的播放集");
    let mut second: PlaylistItem = detail.items[0].clone();
    second.playlist_item_id = "item-02".into();
    second.path = "稻香.mp3".into();
    second.filename = "稻香.mp3".into();
    detail.items.push(second);
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail)));
    let _ = model.player.take_effects();

    model.reduce(ShellMessage::Player(PlayerMessage::PlayItem { item_id: "item-01".into() }));
    model.reduce(ShellMessage::Player(PlayerMessage::PlayItem { item_id: "item-02".into() }));
    let requests: Vec<(String, u64)> = model
        .player
        .take_effects()
        .into_iter()
        .filter_map(|effect| match effect {
            PlayerEffect::LoadItem { item_id, generation, virtual_entry: true, .. } => Some((item_id, generation)),
            _ => None,
        })
        .collect();
    assert_eq!(requests.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["item-01", "item-02"]);

    let loaded = || decode_loaded(REPO, false, "mp3", crate::shell::audio_decode::fixture_mp3()).expect("解码");
    let (first, first_generation) = requests[0].clone();
    model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded {
        item_id: first,
        generation: first_generation,
        still: false,
        result: Ok(loaded()),
        prep: SourcePrep::prepared(REPO, TRACK, "2026-10-11T08:00:00Z"),
    }));
    assert_eq!(model.player.current_id.as_deref(), Some("item-02"));
    assert_eq!(model.player.session.status, "loading", "上一首的播放源不装到下一首上");

    let (second, second_generation) = requests[1].clone();
    let prep = SourcePrep::prepared(REPO, "稻香.mp3", "2026-10-11T08:00:05Z");
    model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded { item_id: second, generation: second_generation, still: false, result: Ok(loaded()), prep }));
    assert_eq!(model.player.session.status, "playing");
}

/// 本地条目照旧直接读字节，不准备播放源；装上了补一次访问记录（后端没有经手），在线条目不另记。
#[test]
fn a_local_item_records_its_access_after_loading() {
    let mut model = scene("online-playback-failed");
    let mut detail = model.player.listed.clone().expect("点开的播放集");
    detail.items[0].is_virtual = false;
    model.reduce(ShellMessage::PlaylistDetailLoaded(Ok(detail)));
    let _ = model.player.take_effects();
    model.reduce(ShellMessage::Player(PlayerMessage::PlayItem { item_id: "item-01".into() }));
    let request = model.player.take_effects().into_iter().find_map(|effect| match effect {
        PlayerEffect::LoadItem { item_id, generation, virtual_entry, .. } => Some((item_id, generation, virtual_entry)),
        _ => None,
    });
    let (item_id, generation, virtual_entry) = request.expect("读取请求");
    assert!(!virtual_entry, "本地条目不准备播放源");
    let loaded = decode_loaded(REPO, false, "mp3", crate::shell::audio_decode::fixture_mp3()).expect("解码");
    model.reduce(ShellMessage::Player(PlayerMessage::ItemLoaded { item_id, generation, still: false, result: Ok(loaded), prep: SourcePrep::default() }));
    assert_eq!(
        model.library.take_effects(),
        [LibraryEffect::RecordAccess { repo_id: REPO.into(), path: TRACK.into() }]
    );
}
