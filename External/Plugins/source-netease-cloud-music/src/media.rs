//! 网易云音频播放源、歌词与播放缓存。下载在 `download.rs`。

use std::{
    fs,
    path::{Path, PathBuf},
};

use momobako_mutsuki_plugin_sdk::write_host_log_silently;
use sha1::{Digest, Sha1};
use time::{format_description::well_known::Rfc3339, Duration as TimeDuration, OffsetDateTime};

use crate::{
    auth, client, files,
    models::{
        ClearTrackCachePayload, PrepareTrackPlaybackPayload, ResolveLyricsPayload, RuntimeContext,
    },
    util::{io_error, time_error},
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

/// 把播放音频下到插件管理的播放缓存：先写临时文件再替换，缓存里不会留下写了一半的文件。
fn download_binary_to_path(
    runtime: &RuntimeContext,
    url: &str,
    target: &Path,
) -> Result<(), String> {
    let parent = target
        .parent()
        .ok_or_else(|| "下载目标缺少父目录".to_string())?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let bytes = client::fetch_binary(url)?;
    files::replace_file(runtime, target, &bytes)
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

pub(crate) fn normalized_level<'a>(level: Option<&'a str>, fallback: &'a str) -> &'a str {
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
