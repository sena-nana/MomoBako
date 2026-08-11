//! 网易云来源的内部数据模型。

use std::path::PathBuf;

use momobako_mutsuki_plugin_sdk::PluginRuntimeContext;
use serde::{Deserialize, Serialize};

pub(crate) const DEFAULT_API_BASE_URL: &str = "";
pub(crate) const DEFAULT_LEVEL: &str = "standard";
pub(crate) const DEFAULT_TEMP_TTL_MINUTES: i64 = 120;
pub(crate) const CREATED_CATEGORY_PATH: &str = "创建的歌单";
pub(crate) const SUBSCRIBED_CATEGORY_PATH: &str = "收藏的歌单";
pub(crate) const PROVIDER_ID: &str = "netease-cloud-music";
pub(crate) const KEYRING_SERVICE: &str = "momobako.netease.source";

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PluginPayload {
    pub repo_root: Option<String>,
    pub directory_path: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub entry_path: Option<String>,
    pub key: Option<String>,
    pub qrimg: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredSession {
    pub credential_ref: String,
    pub account_id: i64,
    pub user_name: Option<String>,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LegacyStoredSession {
    pub cookie: String,
    pub account_id: i64,
    pub user_name: Option<String>,
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
    pub fetched_at: String,
}

#[derive(Debug)]
pub(crate) struct RuntimeContext {
    pub host_runtime: PluginRuntimeContext,
    pub plugin_data_dir: PathBuf,
    pub api_base_url: String,
    pub default_level: String,
    pub temp_ttl_minutes: i64,
    pub filename_template: String,
    pub repo_backend_config: RepoBackendConfigOverride,
}

impl RuntimeContext {
    pub(crate) fn new(
        host_runtime: PluginRuntimeContext,
        plugin_data_dir: PathBuf,
        call_config: Option<&serde_json::Value>,
    ) -> Self {
        let setting = |key: &str| {
            call_config
                .and_then(|value| value.get(key))
                .or_else(|| host_runtime.plugin_config.get(key))
        };
        let api_base_url = crate::client::normalize_ncm_domain(
            setting("apiBaseUrl").and_then(serde_json::Value::as_str),
        );
        let default_level = setting("defaultLevel")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_LEVEL)
            .to_string();
        let temp_ttl_minutes = setting("tempTtlMinutes")
            .and_then(serde_json::Value::as_i64)
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_TEMP_TTL_MINUTES);
        let filename_template = setting("filenameTemplate")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("{{artists}} - {{songName}}")
            .to_string();
        Self {
            host_runtime,
            plugin_data_dir,
            api_base_url,
            default_level,
            temp_ttl_minutes,
            filename_template,
            repo_backend_config: RepoBackendConfigOverride::from_config(call_config),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RepoConfig {
    pub credential_ref: String,
    pub account_id: i64,
    pub default_level: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RepoBackendConfigOverride {
    pub legacy_cookie: Option<String>,
    pub credential_ref: Option<String>,
    pub account_id: Option<i64>,
}

impl RepoBackendConfigOverride {
    fn from_config(config: Option<&serde_json::Value>) -> Self {
        let Some(config) = config else {
            return Self::default();
        };
        Self {
            legacy_cookie: string_field(config, "cookie")
                .or_else(|| string_field(config, "accountCookie")),
            credential_ref: string_field(config, "credentialRef"),
            account_id: config.get("accountId").and_then(crate::util::value_to_i64),
        }
    }
}

fn string_field(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DiscoveredFile {
    pub absolute_path: Option<String>,
    pub relative_path: String,
    pub filename: String,
    pub extension: String,
    pub size_bytes: i64,
    pub created_at: Option<String>,
    pub modified_at: String,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FileSystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileSystemEntry {
    pub path: String,
    pub name: String,
    pub kind: FileSystemEntryKind,
    pub extension: Option<String>,
    pub size_bytes: Option<i64>,
    pub modified_at: Option<String>,
    pub is_virtual: bool,
    pub provider_id: Option<String>,
    pub provider_item_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct FileTreeNode {
    pub path: String,
    pub label: String,
    pub children: Vec<FileTreeNode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DirectoryPageResponse {
    pub entries: Vec<FileSystemEntry>,
    pub total_entries: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountInfo {
    pub id: i64,
    pub user_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProfileInfo {
    pub nickname: Option<String>,
    pub avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LoginStatusData {
    pub account: Option<AccountInfo>,
    pub profile: Option<ProfileInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserAccountEnvelope {
    pub account: Option<AccountInfo>,
    pub profile: Option<ProfileInfo>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UserPlaylistsEnvelope {
    #[serde(default)]
    pub playlist: Vec<PlaylistSummaryItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistSummaryItem {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
    pub cover_img_url: Option<String>,
    #[serde(default)]
    pub subscribed: bool,
    pub user_id: Option<i64>,
    pub creator: Option<PlaylistCreator>,
    pub update_time: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistCreator {
    pub user_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct PlaylistDetailEnvelope {
    pub playlist: Option<PlaylistDetailItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlaylistDetailItem {
    pub id: i64,
    pub name: String,
    pub track_count: i64,
    #[serde(default)]
    pub tracks: Vec<SongItem>,
    #[serde(default)]
    pub track_ids: Vec<TrackIdItem>,
    pub update_time: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct TrackIdItem {
    pub id: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SongItem {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub ar: Vec<SongArtist>,
    pub al: Option<SongAlbum>,
    pub dt: Option<i64>,
    pub privilege: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
pub(crate) struct SongArtist {
    pub name: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SongAlbum {
    pub name: Option<String>,
    pub pic_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct SongDetailEnvelope {
    #[serde(default)]
    pub songs: Vec<SongItem>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlaylistFolder {
    pub category_path: String,
    pub folder_path: String,
    pub playlist_id: i64,
    pub playlist_name: String,
    pub playlist_category: &'static str,
    pub playlist_track_count: i64,
    pub playlist_cover_url: Option<String>,
    pub modified_at: String,
    pub account_id: i64,
    pub load_error: Option<String>,
    pub tracks: Vec<DiscoveredSong>,
}

#[derive(Debug, Clone)]
pub(crate) struct DiscoveredSong {
    pub relative_path: String,
    pub filename: String,
    pub modified_at: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PrepareTrackPlaybackPayload {
    pub song_id: i64,
    pub level: Option<String>,
    #[serde(default)]
    pub force_refresh: bool,
    pub repo_id: Option<String>,
    pub entry_path: Option<String>,
    pub managed_cache_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadTrackPackagePayload {
    pub song_id: i64,
    pub level: Option<String>,
    pub destination: DownloadDestination,
    pub managed_cache_root: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadPlaylistPackagePayload {
    pub playlist_id: i64,
    pub playlist_name: Option<String>,
    #[serde(default)]
    pub tracks: Vec<DownloadPlaylistTrackPayload>,
    #[serde(default)]
    pub track_ids: Vec<i64>,
    pub level: Option<String>,
    pub destination: DownloadDestination,
    pub managed_cache_root: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadPlaylistTrackPayload {
    pub song_id: i64,
    pub song_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResolveLyricsPayload {
    pub song_id: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClearTrackCachePayload {
    pub song_id: i64,
    pub level: Option<String>,
    pub managed_cache_root: Option<String>,
    pub account_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DownloadDestination {
    pub kind: String,
    pub path: Option<String>,
    pub repo_id: Option<String>,
    pub parent_path: Option<String>,
}
