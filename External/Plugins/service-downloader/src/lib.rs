use std::{
    ffi::{c_char, CString},
    fs,
    future::Future,
    os::raw::c_char as raw_c_char,
    path::{Path, PathBuf},
    time::Duration,
};

use momobako_backend_plugin_sdk::{
    free_c_string, read_request, response_error, response_ok, PluginRuntimeContext,
};
use ncm_api_rs::{create_client, ApiResponse, Query};
use reqwest::blocking::Client;
use reqwest::header::{LOCATION, USER_AGENT};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use time::{format_description::well_known::Rfc3339, Duration as TimeDuration, OffsetDateTime};

const MANIFEST: &str = include_str!("../manifest.json");
const DEFAULT_API_BASE_URL: &str = "";
const DEFAULT_LEVEL: &str = "standard";
const DEFAULT_TEMP_TTL_MINUTES: i64 = 120;
const NCM_REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrepareTrackPlaybackPayload {
    account_cookie: Option<String>,
    song_id: i64,
    level: Option<String>,
    repo_id: Option<String>,
    entry_path: Option<String>,
    source_payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadTrackPackagePayload {
    account_cookie: Option<String>,
    song_id: i64,
    level: Option<String>,
    destination: DownloadDestination,
    source_payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadPlaylistPackagePayload {
    account_cookie: Option<String>,
    playlist_id: i64,
    playlist_name: Option<String>,
    #[serde(default)]
    tracks: Vec<DownloadPlaylistTrackPayload>,
    #[serde(default)]
    track_ids: Vec<i64>,
    level: Option<String>,
    destination: DownloadDestination,
    source_payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct DownloadPlaylistTrackPayload {
    song_id: i64,
    #[serde(default)]
    song_name: Option<String>,
    #[serde(default)]
    source_payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveLyricsPayload {
    account_cookie: Option<String>,
    song_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClearTrackCachePayload {
    song_id: i64,
    level: Option<String>,
    source_payload: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadDestination {
    kind: String,
    path: Option<String>,
    repo_id: Option<String>,
    parent_path: Option<String>,
}

#[derive(Debug)]
struct PluginConfig {
    api_base_url: String,
    default_level: String,
    temp_ttl_minutes: i64,
    filename_template: String,
}

#[derive(Debug)]
struct RuntimeContext {
    plugin_data_dir: PathBuf,
    config: PluginConfig,
}

#[derive(Debug, Deserialize)]
struct SongUrlEnvelope {
    data: Option<Vec<SongUrlItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SongUrlItem {
    id: Option<i64>,
    url: Option<String>,
    br: Option<i64>,
    size: Option<i64>,
    md5: Option<String>,
    #[serde(default)]
    free_trial_info: Option<serde_json::Value>,
    #[serde(default)]
    type_field: Option<String>,
    #[serde(rename = "type")]
    #[serde(default)]
    mime_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LyricsEnvelope {
    code: Option<i64>,
    #[serde(default)]
    lrc: Option<LyricField>,
    #[serde(default)]
    yrc: Option<LyricField>,
    #[serde(default)]
    klyric: Option<LyricField>,
}

#[derive(Debug, Deserialize)]
struct LyricField {
    lyric: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SongDetailEnvelope {
    songs: Option<Vec<SongDetailItem>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SongDetailItem {
    id: i64,
    name: String,
    #[serde(default)]
    ar: Vec<SongArtistItem>,
}

#[derive(Debug, Deserialize)]
struct SongArtistItem {
    name: String,
}

#[derive(Debug, Deserialize)]
struct PlaylistDetailEnvelope {
    playlist: Option<PlaylistDetailItem>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistDetailItem {
    name: Option<String>,
}

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut raw_c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut c_char {
    match handle_call(input) {
        Ok(value) => response_ok(value),
        Err(error) => response_error(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    unsafe { free_c_string(value) };
}

fn handle_call(input: *const c_char) -> Result<serde_json::Value, String> {
    let request = read_request(input)?;
    let runtime = runtime_context(request.runtime)?;

    match request.method.as_str() {
        "downloader.prepareTrackPlayback" => {
            let payload: PrepareTrackPlaybackPayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            prepare_track_playback(&runtime, payload)
        }
        "downloader.downloadTrackPackage" => {
            let payload: DownloadTrackPackagePayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            download_track_package(&runtime, payload)
        }
        "downloader.downloadPlaylistPackage" => {
            let payload: DownloadPlaylistPackagePayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            download_playlist_package(&runtime, payload)
        }
        "downloader.resolveLyrics" => {
            let payload: ResolveLyricsPayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            resolve_lyrics_value(&runtime, payload.account_cookie.as_deref(), payload.song_id)
        }
        "downloader.clearTrackCache" => {
            let payload: ClearTrackCachePayload =
                serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
            clear_track_cache(&runtime, payload)
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

fn runtime_context(runtime: PluginRuntimeContext) -> Result<RuntimeContext, String> {
    let plugin_data_dir = PathBuf::from(runtime.plugin_data_dir);
    fs::create_dir_all(plugin_data_dir.join("temp")).map_err(io_error)?;
    fs::create_dir_all(plugin_data_dir.join("exports")).map_err(io_error)?;
    let config = PluginConfig {
        api_base_url: normalize_ncm_domain(
            runtime
                .plugin_config
                .get("apiBaseUrl")
                .and_then(serde_json::Value::as_str),
        ),
        default_level: runtime
            .plugin_config
            .get("defaultLevel")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_LEVEL)
            .to_string(),
        temp_ttl_minutes: runtime
            .plugin_config
            .get("tempTtlMinutes")
            .and_then(serde_json::Value::as_i64)
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TEMP_TTL_MINUTES),
        filename_template: runtime
            .plugin_config
            .get("filenameTemplate")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("{{artists}} - {{songName}}")
            .to_string(),
    };
    let runtime = RuntimeContext {
        plugin_data_dir,
        config,
    };
    clear_expired_temp_files(&runtime)?;
    Ok(runtime)
}

fn prepare_track_playback(
    runtime: &RuntimeContext,
    payload: PrepareTrackPlaybackPayload,
) -> Result<serde_json::Value, String> {
    let level = payload
        .level
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(runtime.config.default_level.as_str());
    let account_id = payload
        .source_payload
        .as_ref()
        .and_then(|value| value.get("accountId"))
        .and_then(value_to_string)
        .unwrap_or_else(|| "anonymous".to_string());
    let account_cookie = resolved_account_cookie(
        payload.account_cookie.as_deref(),
        payload.source_payload.as_ref(),
    );
    let cache_key = hashed_cache_key(payload.song_id, level, &account_id);
    let temp_root = runtime.plugin_data_dir.join("temp");
    let audio_path = temp_root.join(format!("{cache_key}.mp3"));
    let lrc_path = temp_root.join(format!("{cache_key}.lrc"));
    let yrc_path = temp_root.join(format!("{cache_key}.yrc"));
    let expires_at =
        OffsetDateTime::now_utc() + TimeDuration::minutes(runtime.config.temp_ttl_minutes);

    if audio_path.exists() {
        let metadata = fs::metadata(&audio_path).map_err(io_error)?;
        if let Ok(modified) = metadata.modified() {
            let modified_at: OffsetDateTime = modified.into();
            if modified_at
                > OffsetDateTime::now_utc() - TimeDuration::minutes(runtime.config.temp_ttl_minutes)
            {
                return Ok(serde_json::json!({
                    "localPath": audio_path.to_string_lossy().to_string(),
                    "tempFilePath": audio_path.to_string_lossy().to_string(),
                    "mediaType": "audio/mpeg",
                    "expiresAt": expires_at.format(&Rfc3339).map_err(time_error)?,
                    "sizeBytes": metadata.len() as i64,
                    "lyricPath": if lrc_path.exists() { Some(lrc_path.to_string_lossy().to_string()) } else { None::<String> },
                    "wordLyricPath": if yrc_path.exists() { Some(yrc_path.to_string_lossy().to_string()) } else { None::<String> }
                }));
            }
        }
    }

    let song_url = fetch_song_url(runtime, account_cookie.as_deref(), payload.song_id, level)?;
    let audio_url = song_url
        .url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未获取到可播放音频地址".to_string())?;
    download_binary_to_path(audio_url, &audio_path)?;
    let lyrics = fetch_lyrics(runtime, account_cookie.as_deref(), payload.song_id)?;
    if let Some(lrc) = lyrics.0 {
        fs::write(&lrc_path, lrc).map_err(io_error)?;
    }
    if let Some(yrc) = lyrics.1 {
        fs::write(&yrc_path, yrc).map_err(io_error)?;
    }
    let metadata = fs::metadata(&audio_path).map_err(io_error)?;

    Ok(serde_json::json!({
        "localPath": audio_path.to_string_lossy().to_string(),
        "tempFilePath": audio_path.to_string_lossy().to_string(),
        "mediaType": guess_media_type(song_url.mime_hint.as_deref()),
        "expiresAt": expires_at.format(&Rfc3339).map_err(time_error)?,
        "sizeBytes": metadata.len() as i64,
        "bitrate": song_url.br,
        "sourceUrl": serde_json::Value::Null,
        "lyricPath": if lrc_path.exists() { Some(lrc_path.to_string_lossy().to_string()) } else { None::<String> },
        "wordLyricPath": if yrc_path.exists() { Some(yrc_path.to_string_lossy().to_string()) } else { None::<String> }
    }))
}

fn download_track_package(
    runtime: &RuntimeContext,
    payload: DownloadTrackPackagePayload,
) -> Result<serde_json::Value, String> {
    let level = payload
        .level
        .clone()
        .unwrap_or_else(|| runtime.config.default_level.clone());
    let account_cookie = resolved_account_cookie(
        payload.account_cookie.as_deref(),
        payload.source_payload.as_ref(),
    );
    let song_detail = fetch_song_detail(runtime, account_cookie.as_deref(), payload.song_id)?;
    let artists = artist_label(&song_detail.ar);
    let base_name = render_filename(
        &runtime.config.filename_template,
        &artists,
        song_detail.name.as_str(),
    );
    let target_root = resolve_download_root(runtime, &payload.destination)?;
    fs::create_dir_all(&target_root).map_err(io_error)?;
    let song_url = fetch_song_url(runtime, account_cookie.as_deref(), payload.song_id, &level)?;
    let audio_url = song_url
        .url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "未获取到可下载音频地址".to_string())?;
    let audio_path = target_root.join(format!("{base_name}.mp3"));
    download_binary_to_path(audio_url, &audio_path)?;
    let lyrics = fetch_lyrics(runtime, account_cookie.as_deref(), payload.song_id)?;
    let mut outputs = vec![audio_path.to_string_lossy().to_string()];
    if let Some(lrc) = lyrics.0 {
        let path = target_root.join(format!("{base_name}.lrc"));
        fs::write(&path, lrc).map_err(io_error)?;
        outputs.push(path.to_string_lossy().to_string());
    }
    if let Some(yrc) = lyrics.1 {
        let path = target_root.join(format!("{base_name}.yrc"));
        fs::write(&path, yrc).map_err(io_error)?;
        outputs.push(path.to_string_lossy().to_string());
    }

    Ok(serde_json::json!({
        "songId": payload.song_id,
        "paths": outputs,
        "destination": payload.destination.kind
    }))
}

fn download_playlist_package(
    runtime: &RuntimeContext,
    payload: DownloadPlaylistPackagePayload,
) -> Result<serde_json::Value, String> {
    let level = payload
        .level
        .clone()
        .unwrap_or_else(|| runtime.config.default_level.clone());
    let account_cookie = resolved_account_cookie(
        payload.account_cookie.as_deref(),
        payload.source_payload.as_ref(),
    );
    let playlist_name = payload
        .playlist_name
        .clone()
        .or_else(|| {
            fetch_playlist_name(runtime, account_cookie.as_deref(), payload.playlist_id).ok()
        })
        .unwrap_or_else(|| format!("playlist-{}", payload.playlist_id));
    let target_root = resolve_download_root(runtime, &payload.destination)?;
    let playlist_dir = target_root.join(sanitize_file_name(&playlist_name));
    fs::create_dir_all(&playlist_dir).map_err(io_error)?;

    let tracks = playlist_tracks(&payload);
    let mut completed = Vec::new();
    let mut failed = Vec::new();
    for track in tracks {
        let source_payload = merge_source_payloads(
            payload.source_payload.as_ref(),
            track.source_payload.as_ref(),
        );
        let result = download_track_package(
            runtime,
            DownloadTrackPackagePayload {
                account_cookie: payload.account_cookie.clone(),
                song_id: track.song_id,
                level: Some(level.clone()),
                destination: DownloadDestination {
                    kind: "localFolder".to_string(),
                    path: Some(playlist_dir.to_string_lossy().to_string()),
                    repo_id: None,
                    parent_path: None,
                },
                source_payload,
            },
        );
        match result {
            Ok(value) => completed.push(value),
            Err(error) => failed.push(serde_json::json!({
                "songId": track.song_id,
                "songName": track.song_name,
                "error": error,
            })),
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

fn resolve_lyrics_value(
    runtime: &RuntimeContext,
    account_cookie: Option<&str>,
    song_id: i64,
) -> Result<serde_json::Value, String> {
    let (lrc, yrc) = fetch_lyrics(runtime, account_cookie, song_id)?;
    Ok(serde_json::json!({
        "songId": song_id,
        "lrc": lrc,
        "yrc": yrc
    }))
}

fn clear_track_cache(
    runtime: &RuntimeContext,
    payload: ClearTrackCachePayload,
) -> Result<serde_json::Value, String> {
    let level = payload
        .level
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(runtime.config.default_level.as_str());
    let account_id = payload
        .source_payload
        .as_ref()
        .and_then(|value| value.get("accountId"))
        .and_then(value_to_string)
        .unwrap_or_else(|| "anonymous".to_string());
    let cache_key = hashed_cache_key(payload.song_id, level, &account_id);
    let temp_root = runtime.plugin_data_dir.join("temp");
    let paths = [
        temp_root.join(format!("{cache_key}.mp3")),
        temp_root.join(format!("{cache_key}.lrc")),
        temp_root.join(format!("{cache_key}.yrc")),
    ];
    let mut cleared = Vec::new();
    for path in paths {
        if path.exists() {
            fs::remove_file(&path).map_err(io_error)?;
            cleared.push(path.to_string_lossy().to_string());
        }
    }
    Ok(serde_json::json!({
        "songId": payload.song_id,
        "cleared": cleared
    }))
}

fn resolved_account_cookie(
    account_cookie: Option<&str>,
    source_payload: Option<&serde_json::Value>,
) -> Option<String> {
    account_cookie
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            source_payload
                .and_then(|value| value.get("accountCookie"))
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn playlist_tracks(payload: &DownloadPlaylistPackagePayload) -> Vec<DownloadPlaylistTrackPayload> {
    if !payload.tracks.is_empty() {
        return payload.tracks.clone();
    }
    payload
        .track_ids
        .iter()
        .copied()
        .map(|song_id| DownloadPlaylistTrackPayload {
            song_id,
            song_name: None,
            source_payload: None,
        })
        .collect()
}

fn merge_source_payloads(
    base: Option<&serde_json::Value>,
    overlay: Option<&serde_json::Value>,
) -> Option<serde_json::Value> {
    match (base, overlay) {
        (None, None) => None,
        (Some(value), None) => Some(value.clone()),
        (None, Some(value)) => Some(value.clone()),
        (
            Some(serde_json::Value::Object(base_map)),
            Some(serde_json::Value::Object(overlay_map)),
        ) => {
            let mut merged = base_map.clone();
            for (key, value) in overlay_map {
                merged.insert(key.clone(), value.clone());
            }
            Some(serde_json::Value::Object(merged))
        }
        (_, Some(value)) => Some(value.clone()),
    }
}

fn fetch_song_url(
    runtime: &RuntimeContext,
    account_cookie: Option<&str>,
    song_id: i64,
    level: &str,
) -> Result<SongUrlItem, String> {
    let song_id = song_id.to_string();
    let level = level.to_string();
    let response: SongUrlEnvelope = ncm_decode(ncm_call(
        runtime,
        account_cookie,
        |client, query| async move {
            client
                .song_url_v1(&query.param("id", &song_id).param("level", &level))
                .await
        },
    )?)?;
    response
        .data
        .and_then(|mut items| items.drain(..).next())
        .ok_or_else(|| "音频地址接口未返回数据".to_string())
}

fn fetch_song_detail(
    runtime: &RuntimeContext,
    account_cookie: Option<&str>,
    song_id: i64,
) -> Result<SongDetailItem, String> {
    let song_id = song_id.to_string();
    let response: SongDetailEnvelope = ncm_decode(ncm_call(
        runtime,
        account_cookie,
        |client, query| async move { client.song_detail(&query.param("ids", &song_id)).await },
    )?)?;
    response
        .songs
        .and_then(|mut songs| songs.drain(..).next())
        .ok_or_else(|| "歌曲详情接口未返回数据".to_string())
}

fn fetch_playlist_name(
    runtime: &RuntimeContext,
    account_cookie: Option<&str>,
    playlist_id: i64,
) -> Result<String, String> {
    let playlist_id = playlist_id.to_string();
    let response: PlaylistDetailEnvelope = ncm_decode(ncm_call(
        runtime,
        account_cookie,
        |client, query| async move {
            client
                .playlist_detail(&query.param("id", &playlist_id))
                .await
        },
    )?)?;
    response
        .playlist
        .and_then(|playlist| playlist.name)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "歌单详情接口未返回名称".to_string())
}

fn fetch_lyrics(
    runtime: &RuntimeContext,
    account_cookie: Option<&str>,
    song_id: i64,
) -> Result<(Option<String>, Option<String>), String> {
    let song_id = song_id.to_string();
    let lyric_song_id = song_id.clone();
    let response: LyricsEnvelope = ncm_decode(ncm_call(
        runtime,
        account_cookie,
        |client, query| async move { client.lyric_new(&query.param("id", &lyric_song_id)).await },
    )?)?;
    let lrc = response
        .lrc
        .and_then(|field| field.lyric)
        .filter(|value| !value.trim().is_empty());
    let yrc = response
        .yrc
        .and_then(|field| field.lyric)
        .filter(|value| !value.trim().is_empty());
    if lrc.is_some() || yrc.is_some() {
        return Ok((lrc, yrc));
    }
    let fallback_song_id = song_id.clone();
    let fallback: LyricsEnvelope = ncm_decode(ncm_call(
        runtime,
        account_cookie,
        |client, query| async move { client.lyric(&query.param("id", &fallback_song_id)).await },
    )?)?;
    let lrc = fallback
        .lrc
        .and_then(|field| field.lyric)
        .filter(|value| !value.trim().is_empty());
    let yrc = fallback
        .yrc
        .or(fallback.klyric)
        .and_then(|field| field.lyric)
        .filter(|value| !value.trim().is_empty());
    Ok((lrc, yrc))
}

fn http_client() -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("MomoBakoDownloader/1")
        .build()
        .map_err(http_error)
}

fn ncm_call<F, Fut>(
    runtime: &RuntimeContext,
    cookie: Option<&str>,
    build: F,
) -> Result<ApiResponse, String>
where
    F: FnOnce(ncm_api_rs::ApiClient, Query) -> Fut,
    Fut: Future<Output = Result<ApiResponse, ncm_api_rs::NcmError>>,
{
    let cookie = cookie
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    let mut query = if let Some(ref value) = cookie {
        Query::new().cookie(value)
    } else {
        Query::new()
    };
    if !runtime.config.api_base_url.trim().is_empty() {
        query.domain = Some(
            runtime
                .config
                .api_base_url
                .trim()
                .trim_end_matches('/')
                .to_string(),
        );
    }
    let client = create_client(cookie);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(http_error)?
        .block_on(async move {
            tokio::time::timeout(
                std::time::Duration::from_secs(NCM_REQUEST_TIMEOUT_SECS),
                build(client, query),
            )
            .await
        })
        .map_err(|_| format!("网易云接口请求超时（>{NCM_REQUEST_TIMEOUT_SECS} 秒），请重试"))?
        .map_err(http_error)
}

fn ncm_decode<T: for<'de> Deserialize<'de>>(response: ApiResponse) -> Result<T, String> {
    serde_json::from_value(response.body).map_err(|error| error.to_string())
}

fn normalize_ncm_domain(value: Option<&str>) -> String {
    let Some(raw) = value.map(str::trim).filter(|item| !item.is_empty()) else {
        return DEFAULT_API_BASE_URL.to_string();
    };
    let normalized = raw.trim_end_matches('/');
    if let Some((domain, _)) = normalized.split_once("/api/") {
        return domain.trim_end_matches('/').to_string();
    }
    if let Some((domain, _)) = normalized.split_once("/weapi/") {
        return domain.trim_end_matches('/').to_string();
    }
    if let Some((domain, _)) = normalized.split_once("/eapi/") {
        return domain.trim_end_matches('/').to_string();
    }
    if normalized.ends_with("/api") || normalized.ends_with("/weapi") || normalized.ends_with("/eapi")
    {
        return normalized
            .rsplit_once('/')
            .map(|(domain, _)| domain.to_string())
            .unwrap_or_default();
    }
    normalized.to_string()
}

fn download_binary_to_path(url: &str, target_path: &Path) -> Result<(), String> {
    let client = http_client()?;
    let response = client
        .get(url)
        .header(USER_AGENT, "Mozilla/5.0")
        .header(LOCATION, "")
        .send()
        .map_err(http_error)?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("下载资源失败: HTTP {}", status));
    }
    let bytes = response.bytes().map_err(http_error)?;
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    fs::write(target_path, bytes).map_err(io_error)
}

fn resolve_download_root(
    runtime: &RuntimeContext,
    destination: &DownloadDestination,
) -> Result<PathBuf, String> {
    match destination.kind.as_str() {
        "localFolder" => destination
            .path
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "localFolder 缺少 path".to_string()),
        "repository" => {
            let repo_key = destination
                .repo_id
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(sanitize_file_name)
                .or_else(|| {
                    destination
                        .path
                        .as_deref()
                        .filter(|value| !value.trim().is_empty())
                        .map(sanitize_file_name)
                })
                .unwrap_or_else(|| "repository".to_string());
            let repo_root = runtime
                .plugin_data_dir
                .join("exports")
                .join("repository-staging")
                .join(repo_key);
            let parent_path = destination
                .parent_path
                .as_deref()
                .map(|value| value.replace('\\', "/"))
                .unwrap_or_default();
            let normalized_parent = parent_path.trim_matches('/');
            if normalized_parent.is_empty() {
                Ok(repo_root)
            } else {
                Ok(repo_root.join(normalized_parent))
            }
        }
        other => Err(format!("unsupported destination kind: {other}")),
    }
}

fn clear_expired_temp_files(runtime: &RuntimeContext) -> Result<(), String> {
    let temp_root = runtime.plugin_data_dir.join("temp");
    if !temp_root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(temp_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        let modified = metadata.modified().map_err(io_error)?;
        let modified_at: OffsetDateTime = modified.into();
        if modified_at
            < OffsetDateTime::now_utc() - TimeDuration::minutes(runtime.config.temp_ttl_minutes)
        {
            let path = entry.path();
            if path.is_file() {
                let _ = fs::remove_file(path);
            }
        }
    }
    Ok(())
}

fn hashed_cache_key(song_id: i64, level: &str, account_id: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{song_id}:{level}:{account_id}"));
    format!("{:x}", hasher.finalize())
}

fn render_filename(template: &str, artists: &str, song_name: &str) -> String {
    let rendered = template
        .replace("{{artists}}", artists)
        .replace("{{songName}}", song_name);
    sanitize_file_name(&rendered)
}

fn artist_label(artists: &[SongArtistItem]) -> String {
    let joined = artists
        .iter()
        .map(|artist| artist.name.trim())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if joined.is_empty() {
        "Unknown Artist".to_string()
    } else {
        joined
    }
}

fn sanitize_file_name(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ => ch,
        })
        .collect::<String>()
        .trim()
        .to_string();
    if normalized.is_empty() {
        "untitled".to_string()
    } else {
        normalized
    }
}

fn guess_media_type(mime_hint: Option<&str>) -> &'static str {
    match mime_hint.unwrap_or_default() {
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "wav" => "audio/wav",
        _ => "audio/mpeg",
    }
}

fn value_to_string(value: &serde_json::Value) -> Option<String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .or_else(|| value.as_i64().map(|item| item.to_string()))
        .or_else(|| value.as_u64().map(|item| item.to_string()))
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

fn http_error(error: impl ToString) -> String {
    error.to_string()
}

fn time_error(error: impl ToString) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "momobako-downloader-{name}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("test root should be created");
            Self { root }
        }

        fn path(&self, child: &str) -> PathBuf {
            self.root.join(child)
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn runtime_for_test(plugin_data_dir: PathBuf, api_base_url: String) -> RuntimeContext {
        RuntimeContext {
            plugin_data_dir,
            config: PluginConfig {
                api_base_url,
                default_level: "standard".to_string(),
                temp_ttl_minutes: 120,
                filename_template: "{{artists}} - {{songName}}".to_string(),
            },
        }
    }

    #[test]
    fn normalize_ncm_domain_removes_legacy_api_path_suffixes() {
        assert_eq!(normalize_ncm_domain(Some("https://music.163.com/weapi")), "https://music.163.com");
        assert_eq!(
            normalize_ncm_domain(Some("https://interface.music.163.com/eapi/song/url")),
            "https://interface.music.163.com"
        );
        assert_eq!(normalize_ncm_domain(Some("https://music.163.com/api")), "https://music.163.com");
    }

    fn serve_downloader_test_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server address should resolve");
        thread::spawn(move || {
            let mut playlist_detail_count = 0;
            let mut song_detail_count = 0;
            let mut song_url_count = 0;
            let mut lyric_new_count = 0;
            let mut lyric_count = 0;
            for _ in 0..16 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buffer = [0_u8; 4096];
                let size = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..size]);
                let line = request.lines().next().unwrap_or_default();
                let path = line.split_whitespace().nth(1).unwrap_or("/");
                let (status, content_type, body) = if path.starts_with("/eapi/v6/playlist/detail") {
                    let body = if playlist_detail_count == 0 {
                        r#"{"playlist":{"name":"测试歌单"}}"#
                    } else {
                        r#"{"playlist":{"name":"测试歌单"}}"#
                    };
                    playlist_detail_count += 1;
                    ("200 OK", "application/json", body.to_string())
                } else if path.starts_with("/weapi/v3/song/detail") {
                    let body = match song_detail_count {
                        0 => r#"{"songs":[{"id":101,"name":"Song One","ar":[{"name":"Artist One"}]}]}"#,
                        1 => r#"{"songs":[{"id":202,"name":"Song Two","ar":[{"name":"Artist Two"}]}]}"#,
                        2 => r#"{"songs":[{"id":101,"name":"Song One","ar":[{"name":"Artist One"}]}]}"#,
                        3 => r#"{"songs":[{"id":101,"name":"Song One","ar":[{"name":"Artist One"}]}]}"#,
                        _ => r#"{"songs":[{"id":101,"name":"Song One","ar":[{"name":"Artist One"}]}]}"#,
                    };
                    song_detail_count += 1;
                    ("200 OK", "application/json", body.to_string())
                } else if path.starts_with("/eapi/song/enhance/player/url/v1") {
                    let body = match song_url_count {
                        0 => format!(
                            r#"{{"data":[{{"id":101,"url":"http://{addr}/binary/101.mp3","type":"mp3"}}]}}"#
                        ),
                        1 => r#"{"data":[{"id":202,"url":null,"type":"mp3"}]}"#.to_string(),
                        2 => format!(
                            r#"{{"data":[{{"id":101,"url":"http://{addr}/binary/101.mp3","type":"mp3"}}]}}"#
                        ),
                        _ => format!(
                            r#"{{"data":[{{"id":101,"url":"http://{addr}/binary/101.mp3","type":"mp3"}}]}}"#
                        ),
                    };
                    song_url_count += 1;
                    ("200 OK", "application/json", body)
                } else if path.starts_with("/eapi/song/lyric/v1") {
                    let body = match lyric_new_count {
                        0 => r#"{"lrc":{"lyric":"[00:01.00]Line 1"},"yrc":{"lyric":"[00:01.00](Line 1)"}}"#,
                        1 => r#"{"lrc":{"lyric":"[00:02.00]Broken song"}}"#,
                        2 => r#"{"lrc":{"lyric":"[00:01.00]Line 1"},"yrc":{"lyric":"[00:01.00](Line 1)"}}"#,
                        _ => r#"{"lrc":{"lyric":"[00:01.00]Line 1"},"yrc":{"lyric":"[00:01.00](Line 1)"}}"#,
                    };
                    lyric_new_count += 1;
                    ("200 OK", "application/json", body.to_string())
                } else if path.starts_with("/eapi/song/lyric") {
                    let body = match lyric_count {
                        0 => r#"{"lrc":{"lyric":"[00:01.00]Line 1"},"klyric":{"lyric":"[00:01.00](Line 1)"}}"#,
                        _ => r#"{"lrc":{"lyric":"[00:02.00]Broken song"}}"#,
                    };
                    lyric_count += 1;
                    ("200 OK", "application/json", body.to_string())
                } else if path.starts_with("/binary/101.mp3") {
                    ("200 OK", "audio/mpeg", "mock-mp3".to_string())
                } else {
                    ("404 Not Found", "text/plain", "not-found".to_string())
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn download_playlist_package_reports_partial_failures_without_stopping_successes() {
        let workspace = TestWorkspace::new("playlist-partial-failure");
        let export_root = workspace.path("exports");
        let runtime = runtime_for_test(
            workspace.path("plugin-data"),
            serve_downloader_test_server(),
        );

        let value = download_playlist_package(
            &runtime,
            DownloadPlaylistPackagePayload {
                account_cookie: None,
                playlist_id: 77,
                playlist_name: Some("测试歌单".to_string()),
                tracks: vec![
                    DownloadPlaylistTrackPayload {
                        song_id: 101,
                        song_name: Some("Song One".to_string()),
                        source_payload: None,
                    },
                    DownloadPlaylistTrackPayload {
                        song_id: 202,
                        song_name: Some("Song Two".to_string()),
                        source_payload: None,
                    },
                ],
                track_ids: Vec::new(),
                level: Some("standard".to_string()),
                destination: DownloadDestination {
                    kind: "localFolder".to_string(),
                    path: Some(export_root.to_string_lossy().to_string()),
                    repo_id: None,
                    parent_path: None,
                },
                source_payload: None,
            },
        )
        .expect("playlist package should return summary");

        assert_eq!(value["summary"]["total"], serde_json::json!(2));
        assert_eq!(value["summary"]["succeeded"], serde_json::json!(1));
        assert_eq!(value["summary"]["failed"], serde_json::json!(1));
        assert_eq!(value["completed"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["failed"].as_array().map(Vec::len), Some(1));
        assert_eq!(value["failed"][0]["songId"], serde_json::json!(202));
        assert_eq!(
            value["failed"][0]["songName"],
            serde_json::json!("Song Two")
        );

        let playlist_dir = export_root.join("测试歌单");
        assert!(playlist_dir.join("Artist One - Song One.mp3").is_file());
        assert!(playlist_dir.join("Artist One - Song One.lrc").is_file());
        assert!(playlist_dir.join("Artist One - Song One.yrc").is_file());
        assert!(!playlist_dir.join("Artist Two - Song Two.mp3").exists());
    }

    #[test]
    fn download_track_package_writes_into_repository_destination_subdirectory() {
        let workspace = TestWorkspace::new("repository-destination");
        let repository_root = workspace.path("target-repository");
        let runtime = runtime_for_test(
            workspace.path("plugin-data"),
            serve_downloader_test_server(),
        );

        let value = download_track_package(
            &runtime,
            DownloadTrackPackagePayload {
                account_cookie: None,
                song_id: 101,
                level: Some("standard".to_string()),
                destination: DownloadDestination {
                    kind: "repository".to_string(),
                    path: Some(repository_root.to_string_lossy().to_string()),
                    repo_id: Some("repo-target".to_string()),
                    parent_path: Some("Imports/Netease".to_string()),
                },
                source_payload: None,
            },
        )
        .expect("track package should write into repository destination");

        let target_dir = workspace
            .path("plugin-data")
            .join("exports")
            .join("repository-staging")
            .join("repo-target")
            .join("Imports")
            .join("Netease");
        assert!(target_dir.join("Artist One - Song One.mp3").is_file());
        assert!(target_dir.join("Artist One - Song One.lrc").is_file());
        assert!(target_dir.join("Artist One - Song One.yrc").is_file());
        assert!(!repository_root.join("Imports").join("Netease").exists());
        assert_eq!(value["destination"], serde_json::json!("repository"));
        assert_eq!(value["paths"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn prepare_track_playback_reuses_unexpired_temp_cache_without_network() {
        let workspace = TestWorkspace::new("playback-cache-hit");
        let plugin_data_dir = workspace.path("plugin-data");
        let temp_root = plugin_data_dir.join("temp");
        fs::create_dir_all(&temp_root).expect("temp root should be created");

        let cache_key = hashed_cache_key(101, "standard", "123456");
        let audio_path = temp_root.join(format!("{cache_key}.mp3"));
        let lrc_path = temp_root.join(format!("{cache_key}.lrc"));
        let yrc_path = temp_root.join(format!("{cache_key}.yrc"));
        fs::write(&audio_path, b"cached-audio").expect("cached audio should be written");
        fs::write(&lrc_path, "[00:01.00]cached lyric").expect("cached lyric should be written");
        fs::write(&yrc_path, "[00:01.00](cached lyric)")
            .expect("cached word lyric should be written");

        let runtime = runtime_for_test(plugin_data_dir, "http://127.0.0.1:9".to_string());
        let value = prepare_track_playback(
            &runtime,
            PrepareTrackPlaybackPayload {
                account_cookie: None,
                song_id: 101,
                level: Some("standard".to_string()),
                repo_id: Some("repo-demo".to_string()),
                entry_path: Some("Songs/demo.mp3".to_string()),
                source_payload: Some(serde_json::json!({
                    "accountId": "123456"
                })),
            },
        )
        .expect("cached playback should resolve without network");

        assert_eq!(
            value["localPath"],
            serde_json::json!(audio_path.to_string_lossy().to_string())
        );
        assert_eq!(
            value["tempFilePath"],
            serde_json::json!(audio_path.to_string_lossy().to_string())
        );
        assert_eq!(
            value["lyricPath"],
            serde_json::json!(lrc_path.to_string_lossy().to_string())
        );
        assert_eq!(
            value["wordLyricPath"],
            serde_json::json!(yrc_path.to_string_lossy().to_string())
        );
        assert_eq!(value["mediaType"], serde_json::json!("audio/mpeg"));
        assert_eq!(value["sizeBytes"], serde_json::json!(12));
    }

    #[test]
    fn download_track_package_uses_cookie_from_source_payload_when_explicit_cookie_is_missing() {
        let workspace = TestWorkspace::new("track-package-source-payload-cookie");
        let export_root = workspace.path("exports");
        let runtime = runtime_for_test(
            workspace.path("plugin-data"),
            serve_downloader_test_server(),
        );

        let value = download_track_package(
            &runtime,
            DownloadTrackPackagePayload {
                account_cookie: None,
                song_id: 101,
                level: Some("standard".to_string()),
                destination: DownloadDestination {
                    kind: "localFolder".to_string(),
                    path: Some(export_root.to_string_lossy().to_string()),
                    repo_id: None,
                    parent_path: None,
                },
                source_payload: Some(serde_json::json!({
                    "accountCookie": "MUSIC_U=payload-cookie"
                })),
            },
        )
        .expect("track package should fall back to source payload cookie");

        assert!(export_root.join("Artist One - Song One.mp3").is_file());
        assert_eq!(value["songId"], serde_json::json!(101));
        assert_eq!(value["paths"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn download_playlist_package_uses_cookie_from_source_payload_when_explicit_cookie_is_missing() {
        let workspace = TestWorkspace::new("playlist-package-source-payload-cookie");
        let export_root = workspace.path("exports");
        let runtime = runtime_for_test(
            workspace.path("plugin-data"),
            serve_downloader_test_server(),
        );

        let value = download_playlist_package(
            &runtime,
            DownloadPlaylistPackagePayload {
                account_cookie: None,
                playlist_id: 77,
                playlist_name: None,
                tracks: vec![DownloadPlaylistTrackPayload {
                    song_id: 101,
                    song_name: Some("Song One".to_string()),
                    source_payload: Some(serde_json::json!({
                        "accountCookie": "MUSIC_U=payload-cookie"
                    })),
                }],
                track_ids: Vec::new(),
                level: Some("standard".to_string()),
                destination: DownloadDestination {
                    kind: "localFolder".to_string(),
                    path: Some(export_root.to_string_lossy().to_string()),
                    repo_id: None,
                    parent_path: None,
                },
                source_payload: Some(serde_json::json!({
                    "accountCookie": "MUSIC_U=payload-cookie"
                })),
            },
        )
        .expect("playlist package should fall back to source payload cookie");

        let playlist_dir = export_root.join("测试歌单");
        assert!(playlist_dir.join("Artist One - Song One.mp3").is_file());
        assert_eq!(value["summary"]["succeeded"], serde_json::json!(1));
        assert_eq!(value["summary"]["failed"], serde_json::json!(0));
    }

    #[test]
    fn download_playlist_package_accepts_track_ids_payload() {
        let workspace = TestWorkspace::new("playlist-package-track-ids");
        let export_root = workspace.path("exports");
        let runtime = runtime_for_test(
            workspace.path("plugin-data"),
            serve_downloader_test_server(),
        );

        let value = download_playlist_package(
            &runtime,
            DownloadPlaylistPackagePayload {
                account_cookie: None,
                playlist_id: 77,
                playlist_name: Some("测试歌单".to_string()),
                tracks: Vec::new(),
                track_ids: vec![101],
                level: Some("standard".to_string()),
                destination: DownloadDestination {
                    kind: "localFolder".to_string(),
                    path: Some(export_root.to_string_lossy().to_string()),
                    repo_id: None,
                    parent_path: None,
                },
                source_payload: None,
            },
        )
        .expect("trackIds payload should work");

        let playlist_dir = export_root.join("测试歌单");
        assert!(playlist_dir.join("Artist One - Song One.mp3").is_file());
        assert_eq!(value["summary"]["succeeded"], serde_json::json!(1));
        assert_eq!(value["summary"]["failed"], serde_json::json!(0));
    }

    #[test]
    fn resolve_lyrics_value_returns_lrc_and_yrc_when_available() {
        let workspace = TestWorkspace::new("resolve-lyrics");
        let runtime = runtime_for_test(
            workspace.path("plugin-data"),
            serve_downloader_test_server(),
        );

        let value = resolve_lyrics_value(&runtime, None, 101).expect("lyrics should resolve");

        assert_eq!(value["songId"], serde_json::json!(101));
        assert_eq!(value["lrc"], serde_json::json!("[00:01.00]Line 1"));
        assert_eq!(value["yrc"], serde_json::json!("[00:01.00](Line 1)"));
    }
}
