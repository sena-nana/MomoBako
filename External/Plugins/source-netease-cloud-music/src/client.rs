//! 网易云 HTTP 客户端封装。

use std::future::Future;

use ncm_api_rs::{create_client, ApiResponse, NcmError, Query};
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

/// 一次网易云接口调用失败的原因，[`describe`] 把它写成用户能照着处理的话。
#[derive(Debug)]
pub(crate) enum CallError {
    /// 本插件给每次请求的 30 秒到了。
    Timeout,
    /// 网络或接口报的错。
    Ncm(NcmError),
    /// 发请求用的运行时起不来。
    Runtime(String),
}

/// 发送一次网易云 API 请求，并统一施加超时和域名覆盖；失败时直接交回写好的原因（[`describe`]）。
pub(crate) fn ncm_call<F, Fut>(
    runtime: &RuntimeContext,
    cookie: Option<&str>,
    build: F,
) -> Result<ApiResponse, String>
where
    F: FnOnce(ncm_api_rs::ApiClient, Query) -> Fut,
    Fut: Future<Output = Result<ApiResponse, NcmError>>,
{
    ncm_request(runtime, cookie, build).map_err(|error| describe(&error))
}

/// 同 [`ncm_call`]，失败时交回原因的种类，调用方可以按接口改写（例如歌单不存在）。
pub(crate) fn ncm_request<F, Fut>(
    runtime: &RuntimeContext,
    cookie: Option<&str>,
    build: F,
) -> Result<ApiResponse, CallError>
where
    F: FnOnce(ncm_api_rs::ApiClient, Query) -> Fut,
    Fut: Future<Output = Result<ApiResponse, NcmError>>,
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
        .map_err(|error| CallError::Runtime(error.to_string()))?
        .block_on(async move {
            tokio::time::timeout(
                std::time::Duration::from_secs(NCM_REQUEST_TIMEOUT_SECS),
                build(client, query),
            )
            .await
        })
        .map_err(|_| CallError::Timeout)?
        .map_err(CallError::Ncm)
}

/// 网易云接口失败的原因：网络连不上或超时、登录失效、请求太频繁、接口拒绝，各写成一句能照着处理的话。
pub(crate) fn describe(error: &CallError) -> String {
    match error {
        CallError::Timeout => {
            format!("网易云接口请求超时（>{NCM_REQUEST_TIMEOUT_SECS} 秒），请检查网络后重试")
        }
        CallError::Runtime(detail) => format!("网易云请求发不出去：{detail}"),
        CallError::Ncm(NcmError::Http(error)) if error.is_timeout() => {
            "网易云接口请求超时，请检查网络后重试".to_string()
        }
        CallError::Ncm(NcmError::Http(error)) if error.is_connect() => {
            format!("连不上网易云服务器，请检查网络：{error}")
        }
        CallError::Ncm(NcmError::Http(error)) => format!("网易云网络请求失败：{error}"),
        CallError::Ncm(NcmError::Timeout(detail)) => {
            format!("网易云接口请求超时，请检查网络后重试（{detail}）")
        }
        CallError::Ncm(NcmError::AuthRequired(detail)) => {
            format!("网易云登录已失效，请重新登录（{detail}）")
        }
        CallError::Ncm(NcmError::RateLimited(detail)) => {
            format!("网易云接口请求太频繁，请稍后再试（{detail}）")
        }
        CallError::Ncm(NcmError::Api { code, msg }) => {
            format!("网易云接口拒绝了请求（code {code}）：{msg}")
        }
        CallError::Ncm(error) => format!("网易云接口返回了无法识别的内容：{error}"),
    }
}

pub(crate) fn decode<T: for<'de> Deserialize<'de>>(response: ApiResponse) -> Result<T, String> {
    serde_json::from_value(response.body)
        .map_err(|error| format!("网易云接口返回了无法识别的内容：{error}"))
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

/// 取歌单详情。接口说没有这个歌单（code 404）或者正常返回却没有歌单时，原因写歌单不存在；别的照 [`describe`]。
pub(crate) fn fetch_playlist_detail(
    runtime: &RuntimeContext,
    cookie: &str,
    playlist_id: i64,
) -> Result<PlaylistDetailItem, String> {
    let id = playlist_id.to_string();
    let response = ncm_request(runtime, Some(cookie), |client, query| async move {
        client.playlist_detail(&query.param("id", &id)).await
    })
    .map_err(|error| playlist_detail_error(playlist_id, &error))?;
    let envelope: PlaylistDetailEnvelope = decode(response)?;
    envelope
        .playlist
        .ok_or_else(|| playlist_missing(playlist_id))
}

/// 歌单详情取不到的原因。
pub(crate) fn playlist_detail_error(playlist_id: i64, error: &CallError) -> String {
    match error {
        CallError::Ncm(NcmError::Api { code: 404, .. }) => playlist_missing(playlist_id),
        other => describe(other),
    }
}

fn playlist_missing(playlist_id: i64) -> String {
    format!("网易云上没有歌单 {playlist_id}，可能已被删除或设为私密")
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

/// 下载音频地址指向的文件内容（播放缓存和下载共用）。
pub(crate) fn fetch_binary(url: &str) -> Result<Vec<u8>, String> {
    let describe = |error: reqwest::Error| {
        if error.is_timeout() {
            format!("下载音频超时，请检查网络后重试：{error}")
        } else if error.is_connect() {
            format!("连不上音频服务器，请检查网络：{error}")
        } else {
            format!("下载音频失败：{error}")
        }
    };
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(http_error)?
        .get(url)
        .send()
        .map_err(describe)?
        .error_for_status()
        .map_err(describe)?;
    response.bytes().map(|bytes| bytes.to_vec()).map_err(describe)
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

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
