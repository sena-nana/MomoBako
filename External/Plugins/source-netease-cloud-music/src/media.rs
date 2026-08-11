//! 网易云音频播放源、歌词、下载与缓存。

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use momobako_mutsuki_plugin_sdk::write_host_log_silently;
use reqwest::blocking::Client;
use sha1::{Digest, Sha1};
use time::{format_description::well_known::Rfc3339, Duration as TimeDuration, OffsetDateTime};

use crate::{
    auth, client,
    models::{
        ClearTrackCachePayload, DownloadDestination, DownloadPlaylistPackagePayload,
        DownloadPlaylistTrackPayload, DownloadTrackPackagePayload, PrepareTrackPlaybackPayload,
        ResolveLyricsPayload, RuntimeContext, SongItem,
    },
    util::{io_error, sanitize_name, time_error},
};

pub(crate) fn prepare_track_playback(
    runtime: &RuntimeContext,
    payload: PrepareTrackPlaybackPayload,
) -> Result<serde_json::Value, String> {
    let (config, cookie) = auth::resolve_repository_credential(runtime)?;
    let level = normalized_level(payload.level.as_deref(), &runtime.default_level);
    let cache_key = hashed_cache_key(payload.song_id, level, &config.account_id.to_string());
    let temp_root = playback_cache_root(runtime, payload.managed_cache_root.as_deref())?;
    let audio_path = temp_root.join(format!("{cache_key}.mp3"));
    let lrc_path = temp_root.join(format!("{cache_key}.lrc"));
    let yrc_path = temp_root.join(format!("{cache_key}.yrc"));
    if payload.force_refresh {
        for path in [&audio_path, &lrc_path, &yrc_path] {
            if path.exists() {
                fs::remove_file(path).map_err(io_error)?;
            }
        }
    }
    let expires_at = OffsetDateTime::now_utc() + TimeDuration::minutes(runtime.temp_ttl_minutes);
    if is_fresh(&audio_path, runtime.temp_ttl_minutes)? {
        return playback_response(
            &audio_path,
            &lrc_path,
            &yrc_path,
            "audio/mpeg",
            expires_at,
            None,
            true,
        );
    }

    let song_url = client::fetch_song_url(runtime, &cookie, payload.song_id, level)?;
    let url = song_url
        .url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未获取到可播放音频地址".to_string())?;
    download_binary_to_path(runtime, url, &audio_path)?;
    write_lyrics(runtime, &cookie, payload.song_id, &lrc_path, &yrc_path)?;
    playback_response(
        &audio_path,
        &lrc_path,
        &yrc_path,
        guess_media_type(song_url.mime_hint.as_deref()),
        expires_at,
        song_url.br,
        false,
    )
}

pub(crate) fn download_track_package(
    runtime: &RuntimeContext,
    payload: DownloadTrackPackagePayload,
) -> Result<serde_json::Value, String> {
    let (_, cookie) = auth::resolve_repository_credential(runtime)?;
    download_track(runtime, &cookie, payload)
}

fn download_track(
    runtime: &RuntimeContext,
    cookie: &str,
    payload: DownloadTrackPackagePayload,
) -> Result<serde_json::Value, String> {
    let level = normalized_level(payload.level.as_deref(), &runtime.default_level).to_string();
    let song = client::fetch_song_details(runtime, cookie, &[payload.song_id])?
        .into_iter()
        .next()
        .ok_or_else(|| "歌曲详情接口未返回数据".to_string())?;
    let base_name = render_filename(&runtime.filename_template, &artist_label(&song), &song.name);
    let target_root = resolve_download_root(
        runtime,
        &payload.destination,
        payload.managed_cache_root.as_deref(),
    )?;
    fs::create_dir_all(&target_root).map_err(io_error)?;
    let song_url = client::fetch_song_url(runtime, cookie, payload.song_id, &level)?;
    let url = song_url
        .url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未获取到可下载音频地址".to_string())?;
    let audio_path = target_root.join(format!("{base_name}.mp3"));
    download_binary_to_path(runtime, url, &audio_path)?;
    let (lrc, yrc) = client::fetch_lyrics(runtime, cookie, payload.song_id)?;
    let mut outputs = vec![audio_path.to_string_lossy().to_string()];
    if let Some(text) = lrc {
        let path = target_root.join(format!("{base_name}.lrc"));
        fs::write(&path, text).map_err(io_error)?;
        outputs.push(path.to_string_lossy().to_string());
    }
    if let Some(text) = yrc {
        let path = target_root.join(format!("{base_name}.yrc"));
        fs::write(&path, text).map_err(io_error)?;
        outputs.push(path.to_string_lossy().to_string());
    }
    Ok(serde_json::json!({
        "songId": payload.song_id,
        "paths": outputs,
        "destination": payload.destination.kind
    }))
}

pub(crate) fn download_playlist_package(
    runtime: &RuntimeContext,
    mut payload: DownloadPlaylistPackagePayload,
) -> Result<serde_json::Value, String> {
    let (_, cookie) = auth::resolve_repository_credential(runtime)?;
    let detail = client::fetch_playlist_detail(runtime, &cookie, payload.playlist_id).ok();
    let playlist_name = payload
        .playlist_name
        .take()
        .or_else(|| detail.as_ref().map(|value| value.name.clone()))
        .unwrap_or_else(|| format!("playlist-{}", payload.playlist_id));
    let target_root = resolve_download_root(
        runtime,
        &payload.destination,
        payload.managed_cache_root.as_deref(),
    )?;
    let playlist_dir = target_root.join(sanitize_name(&playlist_name));
    fs::create_dir_all(&playlist_dir).map_err(io_error)?;
    let tracks = playlist_tracks(&payload, detail.as_ref());
    let level = normalized_level(payload.level.as_deref(), &runtime.default_level).to_string();
    let mut completed = Vec::new();
    let mut failed = Vec::new();
    for track in tracks {
        let result = download_track(
            runtime,
            &cookie,
            DownloadTrackPackagePayload {
                song_id: track.song_id,
                level: Some(level.clone()),
                destination: DownloadDestination {
                    kind: "localFolder".to_string(),
                    path: Some(playlist_dir.to_string_lossy().to_string()),
                    repo_id: None,
                    parent_path: None,
                },
                managed_cache_root: payload.managed_cache_root.clone(),
            },
        );
        match result {
            Ok(value) => completed.push(value),
            Err(error) => {
                write_host_log_silently(
                    &runtime.host_runtime,
                    "error",
                    "playlistTrackDownloadFailed",
                    "网易云歌单中的单曲下载失败。",
                    serde_json::json!({ "songId": track.song_id, "error": error }),
                );
                failed.push(serde_json::json!({
                    "songId": track.song_id, "songName": track.song_name, "error": error
                }));
            }
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

pub(crate) fn resolve_lyrics(
    runtime: &RuntimeContext,
    payload: ResolveLyricsPayload,
) -> Result<serde_json::Value, String> {
    let (_, cookie) = auth::resolve_repository_credential(runtime)?;
    let (lrc, yrc) = client::fetch_lyrics(runtime, &cookie, payload.song_id)?;
    Ok(serde_json::json!({ "songId": payload.song_id, "lrc": lrc, "yrc": yrc }))
}

pub(crate) fn clear_track_cache(
    runtime: &RuntimeContext,
    payload: ClearTrackCachePayload,
) -> Result<serde_json::Value, String> {
    let account_id = payload
        .account_id
        .or_else(|| {
            auth::resolve_repository_credential(runtime)
                .ok()
                .map(|(config, _)| config.account_id.to_string())
        })
        .unwrap_or_else(|| "anonymous".to_string());
    let level = normalized_level(payload.level.as_deref(), &runtime.default_level);
    let cache_key = hashed_cache_key(payload.song_id, level, &account_id);
    let root = playback_cache_root(runtime, payload.managed_cache_root.as_deref())?;
    let mut cleared = Vec::new();
    for extension in ["mp3", "lrc", "yrc"] {
        let path = root.join(format!("{cache_key}.{extension}"));
        if path.exists() {
            fs::remove_file(&path).map_err(io_error)?;
            cleared.push(path.to_string_lossy().to_string());
        }
    }
    Ok(serde_json::json!({ "songId": payload.song_id, "cleared": cleared }))
}

pub(crate) fn clear_expired_temp_files(runtime: &RuntimeContext) -> Result<(), String> {
    let temp_root = runtime.plugin_data_dir.join("temp");
    if !temp_root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(temp_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        let modified: OffsetDateTime = metadata.modified().map_err(io_error)?.into();
        if modified < OffsetDateTime::now_utc() - TimeDuration::minutes(runtime.temp_ttl_minutes)
            && entry.path().is_file()
        {
            if let Err(error) = fs::remove_file(entry.path()) {
                write_host_log_silently(
                    &runtime.host_runtime,
                    "warn",
                    "tempCleanupFailed",
                    "网易云临时文件清理失败。",
                    serde_json::json!({ "path": entry.path(), "error": error.to_string() }),
                );
            }
        }
    }
    Ok(())
}

fn playback_response(
    audio_path: &Path,
    lrc_path: &Path,
    yrc_path: &Path,
    media_type: &str,
    expires_at: OffsetDateTime,
    bitrate: Option<i64>,
    cached: bool,
) -> Result<serde_json::Value, String> {
    let size_bytes = fs::metadata(audio_path).map_err(io_error)?.len() as i64;
    Ok(serde_json::json!({
        "localPath": audio_path.to_string_lossy(),
        "tempFilePath": audio_path.to_string_lossy(),
        "mediaType": media_type,
        "expiresAt": expires_at.format(&Rfc3339).map_err(time_error)?,
        "sizeBytes": size_bytes,
        "bitrate": bitrate,
        "cached": cached,
        "sourceUrl": serde_json::Value::Null,
        "lyricPath": lrc_path.exists().then(|| lrc_path.to_string_lossy().to_string()),
        "wordLyricPath": yrc_path.exists().then(|| yrc_path.to_string_lossy().to_string())
    }))
}

fn write_lyrics(
    runtime: &RuntimeContext,
    cookie: &str,
    song_id: i64,
    lrc_path: &Path,
    yrc_path: &Path,
) -> Result<(), String> {
    let (lrc, yrc) = client::fetch_lyrics(runtime, cookie, song_id)?;
    if let Some(text) = lrc {
        fs::write(lrc_path, text).map_err(io_error)?
    }
    if let Some(text) = yrc {
        fs::write(yrc_path, text).map_err(io_error)?
    }
    Ok(())
}

fn download_binary_to_path(
    runtime: &RuntimeContext,
    url: &str,
    target: &Path,
) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "下载目标缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let temp = target.with_extension("download");
    let response = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|error| error.to_string())?
        .get(url)
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    let bytes = response.bytes().map_err(|error| error.to_string())?;
    fs::write(&temp, bytes).map_err(io_error)?;
    if target.exists() {
        fs::remove_file(target).map_err(io_error)?
    }
    fs::rename(temp, target).map_err(io_error).map_err(|error| {
        write_host_log_silently(
            &runtime.host_runtime,
            "error",
            "downloadMoveFailed",
            "网易云下载落盘失败。",
            serde_json::json!({ "target": target, "error": error }),
        );
        error
    })
}

fn resolve_download_root(
    runtime: &RuntimeContext,
    destination: &DownloadDestination,
    managed_cache_root: Option<&str>,
) -> Result<PathBuf, String> {
    match destination.kind.as_str() {
        "localFolder" => destination
            .path
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
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
            Ok(if parent.is_empty() {
                root
            } else {
                root.join(parent)
            })
        }
        other => Err(format!("unsupported destination kind: {other}")),
    }
}

fn playback_cache_root(
    runtime: &RuntimeContext,
    managed_cache_root: Option<&str>,
) -> Result<PathBuf, String> {
    let path = managed_cache_root
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).join(".momo/cache/netease-playback"))
        .unwrap_or_else(|| runtime.plugin_data_dir.join("temp"));
    fs::create_dir_all(&path).map_err(io_error)?;
    Ok(path)
}

fn is_fresh(path: &Path, ttl_minutes: i64) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let modified: OffsetDateTime = fs::metadata(path)
        .map_err(io_error)?
        .modified()
        .map_err(io_error)?
        .into();
    Ok(modified > OffsetDateTime::now_utc() - TimeDuration::minutes(ttl_minutes))
}

fn playlist_tracks(
    payload: &DownloadPlaylistPackagePayload,
    detail: Option<&crate::models::PlaylistDetailItem>,
) -> Vec<DownloadPlaylistTrackPayload> {
    if !payload.tracks.is_empty() {
        return payload.tracks.clone();
    }
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

fn normalized_level<'a>(level: Option<&'a str>, fallback: &'a str) -> &'a str {
    level
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
}

fn hashed_cache_key(song_id: i64, level: &str, account_id: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{song_id}:{level}:{account_id}"));
    format!("{:x}", hasher.finalize())
}

fn guess_media_type(hint: Option<&str>) -> &'static str {
    match hint.unwrap_or_default() {
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        _ => "audio/mpeg",
    }
}
