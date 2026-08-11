//! 网易云 HTTP 客户端封装。

use std::future::Future;

use ncm_api_rs::{create_client, ApiResponse, Query};
use serde::Deserialize;

use crate::{
    models::{
        LoginStatusData, PlaylistDetailEnvelope, PlaylistDetailItem, PlaylistSummaryItem,
        RuntimeContext, SongDetailEnvelope, SongItem, UserAccountEnvelope, UserPlaylistsEnvelope,
        DEFAULT_API_BASE_URL,
    },
    util::http_error,
};

const NCM_REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
pub(crate) struct QrCreateEnvelope {
    pub data: Option<QrCreateData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QrCreateData {
    pub qrurl: String,
    pub qrimg: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct QrCheckEnvelope {
    pub code: i64,
    pub message: Option<String>,
    pub cookie: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SongUrlEnvelope {
    pub data: Option<Vec<SongUrlItem>>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SongUrlItem {
    pub url: Option<String>,
    pub br: Option<i64>,
    #[serde(rename = "type")]
    pub mime_hint: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LyricsEnvelope {
    pub lrc: Option<LyricField>,
    pub yrc: Option<LyricField>,
    pub klyric: Option<LyricField>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct LyricField {
    pub lyric: Option<String>,
}

/// 发送一次网易云 API 请求，并统一施加超时和域名覆盖。
pub(crate) fn ncm_call<F, Fut>(
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
    let mut query = cookie
        .as_ref()
        .map_or_else(Query::new, |value| Query::new().cookie(value));
    if !runtime.api_base_url.trim().is_empty() {
        query.domain = Some(
            runtime
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

pub(crate) fn decode<T: for<'de> Deserialize<'de>>(response: ApiResponse) -> Result<T, String> {
    serde_json::from_value(response.body).map_err(|error| error.to_string())
}

pub(crate) fn fetch_login_status(
    runtime: &RuntimeContext,
    cookie: &str,
) -> Result<LoginStatusData, String> {
    decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move { client.login_status(&query).await },
    )?)
}

pub(crate) fn fetch_user_account(
    runtime: &RuntimeContext,
    cookie: &str,
) -> Result<UserAccountEnvelope, String> {
    decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move { client.user_account(&query).await },
    )?)
}

pub(crate) fn fetch_user_playlists(
    runtime: &RuntimeContext,
    cookie: &str,
    account_id: i64,
) -> Result<Vec<PlaylistSummaryItem>, String> {
    let uid = account_id.to_string();
    let response: UserPlaylistsEnvelope = decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move {
            client
                .user_playlist(&query.param("uid", &uid).param("limit", "1000"))
                .await
        },
    )?)?;
    Ok(response.playlist)
}

pub(crate) fn fetch_playlist_detail(
    runtime: &RuntimeContext,
    cookie: &str,
    playlist_id: i64,
) -> Result<PlaylistDetailItem, String> {
    let playlist_id = playlist_id.to_string();
    let response: PlaylistDetailEnvelope = decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move {
            client
                .playlist_detail(&query.param("id", &playlist_id))
                .await
        },
    )?)?;
    response
        .playlist
        .ok_or_else(|| "歌单详情接口未返回 playlist".to_string())
}

pub(crate) fn fetch_song_details(
    runtime: &RuntimeContext,
    cookie: &str,
    ids: &[i64],
) -> Result<Vec<SongItem>, String> {
    let mut songs = Vec::new();
    for chunk in ids.chunks(200) {
        let ids = chunk
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let response: SongDetailEnvelope = decode(ncm_call(
            runtime,
            Some(cookie),
            |client, query| async move { client.song_detail(&query.param("ids", &ids)).await },
        )?)?;
        songs.extend(response.songs);
    }
    Ok(songs)
}

pub(crate) fn fetch_song_url(
    runtime: &RuntimeContext,
    cookie: &str,
    song_id: i64,
    level: &str,
) -> Result<SongUrlItem, String> {
    let song_id = song_id.to_string();
    let level = level.to_string();
    let response: SongUrlEnvelope = decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move {
            client
                .song_url_v1(&query.param("id", &song_id).param("level", &level))
                .await
        },
    )?)?;
    response
        .data
        .and_then(|mut values| values.drain(..).next())
        .ok_or_else(|| "音频地址接口未返回数据".to_string())
}

pub(crate) fn fetch_lyrics(
    runtime: &RuntimeContext,
    cookie: &str,
    song_id: i64,
) -> Result<(Option<String>, Option<String>), String> {
    let id = song_id.to_string();
    let response: LyricsEnvelope = decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move { client.lyric_new(&query.param("id", &id)).await },
    )?)?;
    let lrc = lyric_text(response.lrc);
    let yrc = lyric_text(response.yrc);
    if lrc.is_some() || yrc.is_some() {
        return Ok((lrc, yrc));
    }

    let id = song_id.to_string();
    let fallback: LyricsEnvelope = decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move { client.lyric(&query.param("id", &id)).await },
    )?)?;
    Ok((
        lyric_text(fallback.lrc),
        lyric_text(fallback.yrc.or(fallback.klyric)),
    ))
}

fn lyric_text(field: Option<LyricField>) -> Option<String> {
    field
        .and_then(|value| value.lyric)
        .filter(|value| !value.trim().is_empty())
}

pub(crate) fn extract_qr_unikey(value: &serde_json::Value) -> Option<String> {
    value
        .get("unikey")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn normalize_ncm_domain(value: Option<&str>) -> String {
    let Some(raw) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return DEFAULT_API_BASE_URL.to_string();
    };
    let normalized = raw.trim_end_matches('/');
    for marker in ["/api/", "/weapi/", "/eapi/"] {
        if let Some((domain, _)) = normalized.split_once(marker) {
            return domain.trim_end_matches('/').to_string();
        }
    }
    for suffix in ["/api", "/weapi", "/eapi"] {
        if let Some(domain) = normalized.strip_suffix(suffix) {
            return domain.to_string();
        }
    }
    normalized.to_string()
}
