//! 网易云单曲、歌单下载。
//!
//! 下载到用户选的本地目录（`localFolder`）时只新建文件：目录里已有的同名文件保留原样、不写入（音频已经在就不再
//! 下载），结果的 `skippedEntries` 列出这些文件，`paths` 只列这次写下的。下载到其他资源库（`repository`）时先
//! 放进插件管理的暂存目录，由宿主导入目标资源库。
//!
//! 下载以前先查一次登录状态：登录失效时网易云只给试听片段或者不给地址，不先查就会每首都说取不到地址。歌单的曲目
//! 取不到（网络、登录失效、歌单不存在）时整个调用报错、写明原因，不再什么也不下却交回成功。

use std::{
    collections::{BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

use momobako_mutsuki_plugin_sdk::write_host_log_silently;

use crate::{
    auth, client,
    files::{remove_written, write_file, WritePolicy},
    media::normalized_level,
    models::{
        DownloadDestination, DownloadPlaylistPackagePayload, DownloadPlaylistTrackPayload,
        DownloadTrackPackagePayload, PlaylistDetailItem, RuntimeContext, SongItem,
    },
    util::{io_error, sanitize_name},
};

/// 一次下载落在哪里、已有同名文件时怎么办。
#[derive(Debug, PartialEq)]
pub(crate) struct DownloadTarget {
    pub root: PathBuf,
    pub policy: WritePolicy,
}

/// 一首歌落盘的结果。
#[derive(Debug, Default, PartialEq)]
pub(crate) struct SavedTrack {
    /// 这次写下的文件。
    pub written: Vec<PathBuf>,
    /// 目标里已有同名文件，保留原样没有写入的。
    pub kept: Vec<PathBuf>,
}

pub(crate) fn download_track_package(
    runtime: &RuntimeContext,
    payload: DownloadTrackPackagePayload,
) -> Result<serde_json::Value, String> {
    let (_, cookie) = auth::resolve_repository_credential(runtime)?;
    auth::ensure_login_valid(runtime, &cookie)?;
    let level = normalized_level(payload.level.as_deref(), &runtime.default_level).to_string();
    let song = fetch_song(runtime, &cookie, payload.song_id)?;
    let target = resolve_download_target(
        runtime,
        &payload.destination,
        payload.managed_cache_root.as_deref(),
    )?;
    fs::create_dir_all(&target.root).map_err(io_error)?;
    let base = render_filename(&runtime.filename_template, &artist_label(&song), &song.name);
    let saved = download_song(runtime, &cookie, &song, &level, &target.root, target.policy, &base)?;
    let mut result = track_result(&song, &saved);
    result["destination"] = serde_json::json!(payload.destination.kind);
    Ok(result)
}

pub(crate) fn download_playlist_package(
    runtime: &RuntimeContext,
    mut payload: DownloadPlaylistPackagePayload,
) -> Result<serde_json::Value, String> {
    let (_, cookie) = auth::resolve_repository_credential(runtime)?;
    auth::ensure_login_valid(runtime, &cookie)?;
    let detail = playlist_detail(
        &payload,
        || client::fetch_playlist_detail(runtime, &cookie, payload.playlist_id),
        |error| {
            write_host_log_silently(
                &runtime.host_runtime,
                "warn",
                "playlistDetailUnavailable",
                "网易云歌单详情取不到，下载目录按歌单标识命名。",
                serde_json::json!({ "playlistId": payload.playlist_id, "error": error }),
            )
        },
    )?;
    let playlist_name = payload
        .playlist_name
        .take()
        .or_else(|| detail.as_ref().map(|value| value.name.clone()))
        .unwrap_or_else(|| format!("playlist-{}", payload.playlist_id));
    let target = resolve_download_target(
        runtime,
        &payload.destination,
        payload.managed_cache_root.as_deref(),
    )?;
    let playlist_dir = target.root.join(sanitize_name(&playlist_name));
    fs::create_dir_all(&playlist_dir).map_err(io_error)?;
    let tracks = playlist_tracks(&payload, detail.as_ref());
    let mut songs = known_songs(
        detail.as_ref(),
        &tracks,
        |ids| client::fetch_song_details(runtime, &cookie, ids),
        |error| {
            write_host_log_silently(
                &runtime.host_runtime,
                "warn",
                "songDetailsUnavailable",
                "网易云歌曲详情批量取不到，逐首下载时再取。",
                serde_json::json!({ "playlistId": payload.playlist_id, "error": error }),
            )
        },
    );
    let level = normalized_level(payload.level.as_deref(), &runtime.default_level).to_string();
    let mut claimed = BTreeSet::new();
    let mut completed = Vec::new();
    let mut failed = Vec::new();
    for track in tracks {
        let song = match songs.remove(&track.song_id) {
            Some(song) => Ok(song),
            None => fetch_song(runtime, &cookie, track.song_id),
        };
        let downloaded = song.map_err(|error| (None, error)).and_then(|song| {
            let rendered =
                render_filename(&runtime.filename_template, &artist_label(&song), &song.name);
            let base = claim_file_name(&rendered, song.id, &mut claimed);
            match download_song(runtime, &cookie, &song, &level, &playlist_dir, target.policy, &base) {
                Ok(saved) => Ok((song, saved)),
                Err(error) => Err((Some(song.name), error)),
            }
        });
        match downloaded {
            Ok((song, saved)) => completed.push(track_result(&song, &saved)),
            // 失败的一首只放进结果的 `failed`，不另写日志：宿主收尾时按结果逐条写（带动作名、歌单名和来源资源库）。
            Err((name, error)) => failed.push(failed_track(&track, name, &error)),
        }
    }
    Ok(serde_json::json!({
        "playlistId": payload.playlist_id,
        "playlistName": playlist_name,
        "completed": completed,
        "failed": failed,
        "summary": {
            "total": completed.len() + failed.len(),
            "succeeded": completed.len(),
            "failed": failed.len()
        }
    }))
}

/// 取一首歌的详情（文件名要歌手和歌名）。
fn fetch_song(runtime: &RuntimeContext, cookie: &str, song_id: i64) -> Result<SongItem, String> {
    client::fetch_song_details(runtime, cookie, &[song_id])?
        .into_iter()
        .next()
        .ok_or_else(|| format!("网易云没有返回歌曲 {song_id} 的详情，可能已下架"))
}

/// 下载歌单要的歌单详情。载荷没带曲目（`tracks`、`trackIds`）时曲目只能从详情里来：取不到就交回原因，整个下载报错、
/// 什么也不下。载荷带了曲目时详情只用来补歌单名：带了名字就不取，取不到时 `warn` 记一条、照常下载。
fn playlist_detail(
    payload: &DownloadPlaylistPackagePayload,
    fetch: impl FnOnce() -> Result<PlaylistDetailItem, String>,
    warn: impl FnOnce(&str),
) -> Result<Option<PlaylistDetailItem>, String> {
    let has_tracks = !payload.tracks.is_empty() || !payload.track_ids.is_empty();
    if has_tracks && payload.playlist_name.is_some() {
        return Ok(None);
    }
    match fetch() {
        Ok(detail) => Ok(Some(detail)),
        Err(error) if has_tracks => {
            warn(&error);
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

/// 歌单里每首歌的详情：先用歌单详情里带的 `tracks`，没带到的按标识一次批量取（`fetch` 内部每 200 首一批）。批量取
/// 失败时 `warn` 记一条，这些歌下载时再逐首取。
fn known_songs(
    detail: Option<&PlaylistDetailItem>,
    tracks: &[DownloadPlaylistTrackPayload],
    fetch: impl FnOnce(&[i64]) -> Result<Vec<SongItem>, String>,
    warn: impl FnOnce(&str),
) -> HashMap<i64, SongItem> {
    let wanted: BTreeSet<i64> = tracks.iter().map(|track| track.song_id).collect();
    let mut songs: HashMap<i64, SongItem> = detail
        .map(|detail| {
            detail
                .tracks
                .iter()
                .filter(|song| wanted.contains(&song.id))
                .map(|song| (song.id, song.clone()))
                .collect()
        })
        .unwrap_or_default();
    let missing: Vec<i64> = wanted
        .iter()
        .copied()
        .filter(|id| !songs.contains_key(id))
        .collect();
    if missing.is_empty() {
        return songs;
    }
    match fetch(&missing) {
        Ok(found) => songs.extend(found.into_iter().map(|song| (song.id, song))),
        Err(error) => warn(&error),
    }
    songs
}

/// 失败的一首：歌名先用载荷带来的，再用取到的歌曲详情里的，都没有时不写。
fn failed_track(
    track: &DownloadPlaylistTrackPayload,
    fetched_name: Option<String>,
    error: &str,
) -> serde_json::Value {
    serde_json::json!({
        "songId": track.song_id,
        "songName": track.song_name.clone().or(fetched_name),
        "error": error,
    })
}

/// 下载一首歌到 `root`：取播放地址和音频、取歌词，交给 [`save_track`] 落盘。
fn download_song(
    runtime: &RuntimeContext,
    cookie: &str,
    song: &SongItem,
    level: &str,
    root: &Path,
    policy: WritePolicy,
    base: &str,
) -> Result<SavedTrack, String> {
    save_track(
        runtime,
        root,
        base,
        policy,
        || {
            let song_url = client::fetch_song_url(runtime, cookie, song.id, level)?;
            let url = song_url
                .url
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "未获取到可下载音频地址".to_string())?;
            client::fetch_binary(url)
        },
        || client::fetch_lyrics(runtime, cookie, song.id),
    )
}

/// 把一首歌的音频（`{base}.mp3`）和歌词（`.lrc`、`.yrc`）写进 `root`。
///
/// 先取齐音频和歌词再写，取不到时什么也不写。保留已有文件时，同名音频已经在就不调用 `fetch_audio`、记为保留；
/// 歌词逐个看，已有的保留、没有的写下。写到一半失败时删掉这一首这次写下的文件再交回错误，已有的文件不碰。
pub(crate) fn save_track(
    runtime: &RuntimeContext,
    root: &Path,
    base: &str,
    policy: WritePolicy,
    fetch_audio: impl FnOnce() -> Result<Vec<u8>, String>,
    fetch_lyrics: impl FnOnce() -> Result<(Option<String>, Option<String>), String>,
) -> Result<SavedTrack, String> {
    let audio_path = root.join(format!("{base}.mp3"));
    let keep_audio = policy == WritePolicy::KeepExisting && audio_path.exists();
    let audio = if keep_audio { None } else { Some(fetch_audio()?) };
    let (lrc, yrc) = fetch_lyrics()?;
    let mut saved = SavedTrack::default();
    let mut files = Vec::new();
    match audio {
        Some(bytes) => files.push((audio_path, bytes)),
        None => saved.kept.push(audio_path),
    }
    for (extension, text) in [("lrc", lrc), ("yrc", yrc)] {
        if let Some(text) = text {
            files.push((root.join(format!("{base}.{extension}")), text.into_bytes()));
        }
    }
    for (path, bytes) in files {
        match write_file(runtime, &path, &bytes, policy) {
            Ok(true) => saved.written.push(path),
            Ok(false) => saved.kept.push(path),
            Err(error) => {
                remove_written(runtime, &saved.written);
                return Err(error);
            }
        }
    }
    Ok(saved)
}

/// 一首歌的结果：`paths` 是这次写下的文件，`skippedEntries` 是保留原样没有写入的同名文件（没有时不写这个键）。
fn track_result(song: &SongItem, saved: &SavedTrack) -> serde_json::Value {
    let mut result = serde_json::json!({
        "songId": song.id,
        "songName": song.name,
        "paths": path_strings(&saved.written),
    });
    if !saved.kept.is_empty() {
        result["skippedEntries"] = serde_json::json!(path_strings(&saved.kept));
    }
    result
}

fn path_strings(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}

/// 下载落在哪里：本地目录保留已有文件；下载到其他资源库时放进插件管理的暂存目录，暂存过的同名文件可以替换。
pub(crate) fn resolve_download_target(
    runtime: &RuntimeContext,
    destination: &DownloadDestination,
    managed_cache_root: Option<&str>,
) -> Result<DownloadTarget, String> {
    match destination.kind.as_str() {
        "localFolder" => destination
            .path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|path| DownloadTarget {
                root: PathBuf::from(path),
                policy: WritePolicy::KeepExisting,
            })
            .ok_or_else(|| "localFolder 缺少 path".to_string()),
        "repository" => {
            let repo_key = destination
                .repo_id
                .as_deref()
                .or(destination.path.as_deref())
                .map(sanitize_name)
                .unwrap_or_else(|| "repository".to_string());
            let root = managed_cache_root
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(|value| {
                    PathBuf::from(value)
                        .join(".momo/cache/download-staging")
                        .join(&repo_key)
                })
                .unwrap_or_else(|| {
                    runtime
                        .plugin_data_dir
                        .join("exports/repository-staging")
                        .join(&repo_key)
                });
            let parent = destination
                .parent_path
                .as_deref()
                .unwrap_or_default()
                .replace('\\', "/");
            let parent = parent.trim_matches('/');
            let root = if parent.is_empty() {
                root
            } else {
                root.join(parent)
            };
            Ok(DownloadTarget {
                root,
                policy: WritePolicy::Replace,
            })
        }
        other => Err(format!("unsupported destination kind: {other}")),
    }
}

/// 同一次下载里文件名撞上了（不同的歌渲染出同一个名字，只差大小写也算）时，后来的加上 ` [歌曲标识]`，和虚拟目录
/// 的命名一致（`util::unique_name`）。这样一次下载不会把自己前面写下的文件当成已有文件跳过。
pub(crate) fn claim_file_name(base: &str, song_id: i64, claimed: &mut BTreeSet<String>) -> String {
    if claimed.insert(base.to_lowercase()) {
        return base.to_string();
    }
    let fallback = format!("{base} [{song_id}]");
    claimed.insert(fallback.to_lowercase());
    fallback
}

/// 要下载的曲目：载荷带了 `tracks` 或 `trackIds` 就用它们，否则取歌单详情里的全部曲目。重复的歌曲标识只留第一个。
fn playlist_tracks(
    payload: &DownloadPlaylistPackagePayload,
    detail: Option<&crate::models::PlaylistDetailItem>,
) -> Vec<DownloadPlaylistTrackPayload> {
    let tracks = if !payload.tracks.is_empty() {
        payload.tracks.clone()
    } else {
        let ids = if !payload.track_ids.is_empty() {
            payload.track_ids.clone()
        } else {
            detail
                .map(|value| {
                    if value.track_ids.is_empty() {
                        value.tracks.iter().map(|item| item.id).collect()
                    } else {
                        value.track_ids.iter().map(|item| item.id).collect()
                    }
                })
                .unwrap_or_default()
        };
        ids.into_iter()
            .map(|song_id| DownloadPlaylistTrackPayload {
                song_id,
                song_name: None,
            })
            .collect()
    };
    let mut seen = BTreeSet::new();
    tracks
        .into_iter()
        .filter(|track| seen.insert(track.song_id))
        .collect()
}

fn artist_label(song: &SongItem) -> String {
    let value = song
        .ar
        .iter()
        .map(|artist| artist.name.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if value.is_empty() {
        "Unknown Artist".to_string()
    } else {
        value
    }
}

fn render_filename(template: &str, artists: &str, song_name: &str) -> String {
    sanitize_name(
        &template
            .replace("{{artists}}", artists)
            .replace("{{songName}}", song_name),
    )
}

#[cfg(test)]
#[path = "download_tests.rs"]
mod tests;
