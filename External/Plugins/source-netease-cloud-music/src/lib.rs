use std::{
    ffi::{c_char, CString},
    fs,
    future::Future,
    path::PathBuf,
};

use base64::{engine::general_purpose, Engine as _};
use momobako_backend_plugin_sdk::{
    free_c_string, read_request, response_error, response_ok, PluginRuntimeContext,
};
use ncm_api_rs::{create_client, ApiResponse, Query};
use qrcode::{render::svg, QrCode};
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MANIFEST: &str = include_str!("../manifest.json");
const DEFAULT_API_BASE_URL: &str = "";
const CREATED_CATEGORY_PATH: &str = "创建的歌单";
const SUBSCRIBED_CATEGORY_PATH: &str = "收藏的歌单";
const PROVIDER_ID: &str = "netease-cloud-music";
const NCM_REQUEST_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginPayload {
    #[serde(default)]
    repo_root: Option<String>,
    #[serde(default)]
    directory_path: Option<String>,
    #[serde(default)]
    entry_path: Option<String>,
    #[serde(default)]
    config: Option<serde_json::Value>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    qrimg: Option<bool>,
    #[serde(default)]
    timestamp: Option<i64>,
    #[serde(default)]
    cookie: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct StoredSession {
    cookie: String,
    account_id: i64,
    user_name: Option<String>,
    nickname: Option<String>,
    avatar_url: Option<String>,
    fetched_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveredFile {
    absolute_path: Option<String>,
    relative_path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    created_at: Option<String>,
    modified_at: String,
    is_virtual: bool,
    provider_id: Option<String>,
    provider_item_id: Option<String>,
    source_payload: Option<serde_json::Value>,
    local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum FileSystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileSystemEntry {
    path: String,
    name: String,
    kind: FileSystemEntryKind,
    extension: Option<String>,
    size_bytes: Option<i64>,
    modified_at: Option<String>,
    is_virtual: bool,
    provider_id: Option<String>,
    provider_item_id: Option<String>,
    source_payload: Option<serde_json::Value>,
    local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileTreeNode {
    path: String,
    label: String,
    children: Vec<FileTreeNode>,
}

#[derive(Debug, Clone)]
struct RuntimeContext {
    plugin_data_dir: PathBuf,
    api_base_url: String,
    default_level: String,
    repo_backend_config: RepoBackendConfigOverride,
}

#[derive(Debug, Clone)]
struct RepoConfig {
    cookie: String,
    account_id: i64,
    nickname: Option<String>,
    user_name: Option<String>,
    default_level: String,
}

#[derive(Debug, Clone, Default)]
struct RepoBackendConfigOverride {
    cookie: Option<String>,
    account_id: Option<i64>,
    nickname: Option<String>,
    user_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QrCreateEnvelope {
    data: Option<QrCreateData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QrCreateData {
    qrurl: String,
    #[serde(default)]
    qrimg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QrCheckEnvelope {
    code: i64,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    cookie: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoginStatusData {
    #[serde(default)]
    account: Option<AccountInfo>,
    #[serde(default)]
    profile: Option<ProfileInfo>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AccountInfo {
    id: i64,
    #[serde(default)]
    user_name: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProfileInfo {
    #[serde(default)]
    user_id: Option<i64>,
    #[serde(default)]
    nickname: Option<String>,
    #[serde(default)]
    avatar_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserAccountEnvelope {
    #[serde(default)]
    account: Option<AccountInfo>,
    #[serde(default)]
    profile: Option<ProfileInfo>,
}

#[derive(Debug, Deserialize)]
struct UserPlaylistsEnvelope {
    #[serde(default)]
    playlist: Vec<PlaylistSummaryItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PlaylistSummaryItem {
    id: i64,
    name: String,
    track_count: i64,
    #[serde(default)]
    subscribed: bool,
    #[serde(default)]
    user_id: Option<i64>,
    #[serde(default)]
    creator: Option<PlaylistCreator>,
    #[serde(default)]
    update_time: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PlaylistCreator {
    #[serde(default)]
    user_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct PlaylistDetailEnvelope {
    playlist: Option<PlaylistDetailItem>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PlaylistDetailItem {
    id: i64,
    name: String,
    track_count: i64,
    #[serde(default)]
    tracks: Vec<SongItem>,
    #[serde(default)]
    track_ids: Vec<TrackIdItem>,
    #[serde(default)]
    update_time: Option<i64>,
}

#[derive(Debug, Deserialize, Clone)]
struct TrackIdItem {
    id: i64,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SongItem {
    id: i64,
    name: String,
    #[serde(default)]
    ar: Vec<SongArtist>,
    #[serde(default)]
    al: Option<SongAlbum>,
    #[serde(default)]
    dt: Option<i64>,
    #[serde(default)]
    privilege: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Clone)]
struct SongArtist {
    name: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SongAlbum {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    pic_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SongDetailEnvelope {
    #[serde(default)]
    songs: Vec<SongItem>,
}

#[derive(Debug, Clone)]
struct PlaylistFolder {
    category_path: String,
    folder_path: String,
    playlist_id: i64,
    playlist_name: String,
    playlist_category: &'static str,
    playlist_track_count: i64,
    modified_at: String,
    account_id: i64,
    account_cookie: String,
    load_error: Option<String>,
    tracks: Vec<DiscoveredSong>,
}

#[derive(Debug, Clone)]
struct DiscoveredSong {
    relative_path: String,
    filename: String,
    modified_at: String,
    payload: serde_json::Value,
}

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut c_char {
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
    let payload: PluginPayload =
        serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
    let runtime = runtime_context(request.runtime, payload.config.clone())?;

    match request.method.as_str() {
        "auth.createQrSession" => auth_create_qr_session(&runtime, payload),
        "auth.pollQrSession" => auth_poll_qr_session(&runtime, payload),
        "auth.getLoginStatus" => auth_get_login_status(&runtime, payload),
        "auth.clearLogin" => auth_clear_login(&runtime),
        "filesystem.ensureAttachable" => Ok(serde_json::json!({})),
        "filesystem.prepareRepositoryRoot" => Ok(serde_json::json!({})),
        "filesystem.listFiles" => list_files(&runtime),
        "filesystem.listTree" => list_tree(&runtime),
        "filesystem.listDirectory" => list_directory(
            &runtime,
            payload.directory_path.as_deref().unwrap_or_default(),
        ),
        "filesystem.statEntry" => {
            stat_entry(&runtime, payload.entry_path.as_deref().unwrap_or_default())
        }
        "filesystem.createDirectory"
        | "filesystem.createFile"
        | "filesystem.renameEntry"
        | "filesystem.moveEntry"
        | "filesystem.deleteEntry" => Err("网易云音乐资源库当前为只读虚拟源".to_string()),
        method => Err(format!("unsupported method: {method}")),
    }
}

fn runtime_context(
    runtime: PluginRuntimeContext,
    config: Option<serde_json::Value>,
) -> Result<RuntimeContext, String> {
    let plugin_data_dir = PathBuf::from(runtime.plugin_data_dir);
    fs::create_dir_all(&plugin_data_dir).map_err(io_error)?;
    let api_base_url = normalize_ncm_domain(
        config
            .as_ref()
            .and_then(|value| value.get("apiBaseUrl"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| {
                runtime
                    .plugin_config
                    .get("apiBaseUrl")
                    .and_then(serde_json::Value::as_str)
            }),
    );
    let default_level = config
        .as_ref()
        .and_then(|value| value.get("defaultLevel"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            runtime
                .plugin_config
                .get("defaultLevel")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("standard")
        .to_string();
    Ok(RuntimeContext {
        plugin_data_dir,
        api_base_url,
        default_level,
        repo_backend_config: repo_backend_config_override(config.as_ref()),
    })
}

fn repo_config(runtime: &RuntimeContext) -> Result<RepoConfig, String> {
    if let Some(cookie) = runtime
        .repo_backend_config
        .cookie
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
    {
        let stored = load_session(runtime).ok();
        let account_id = runtime
            .repo_backend_config
            .account_id
            .or_else(|| stored.as_ref().map(|value| value.account_id))
            .ok_or_else(|| "网易云资源库缺少 accountId，请重新登录并刷新资源库".to_string())?;
        return Ok(RepoConfig {
            cookie,
            account_id,
            nickname: runtime
                .repo_backend_config
                .nickname
                .clone()
                .or_else(|| stored.as_ref().and_then(|value| value.nickname.clone())),
            user_name: runtime
                .repo_backend_config
                .user_name
                .clone()
                .or_else(|| stored.as_ref().and_then(|value| value.user_name.clone())),
            default_level: runtime.default_level.clone(),
        });
    }

    let value = load_session(runtime)?;
    Ok(RepoConfig {
        cookie: value.cookie,
        account_id: value.account_id,
        nickname: value.nickname,
        user_name: value.user_name,
        default_level: runtime.default_level.clone(),
    })
}

fn auth_create_qr_session(
    runtime: &RuntimeContext,
    payload: PluginPayload,
) -> Result<serde_json::Value, String> {
    let key_response = ncm_call(
        runtime,
        payload.cookie.as_deref(),
        |client, query| async move { client.login_qr_key(&query).await },
    )?;
    let key = extract_qr_unikey(&key_response.body).ok_or_else(|| {
        format!(
            "二维码 key 接口未返回 unikey: {}",
            compact_json(&key_response.body)
        )
    })?;
    let create_key = key.clone();
    let create: QrCreateEnvelope = ncm_decode(ncm_call(
        runtime,
        payload.cookie.as_deref(),
        |client, query| async move {
            client
                .login_qr_create(&query.param("key", &create_key))
                .await
        },
    )?)?;
    let create = create
        .data
        .ok_or_else(|| "二维码创建接口未返回 data".to_string())?;
    let qrimg = if payload.qrimg.unwrap_or(true) {
        Some(qr_svg_data_url(&create.qrurl)?)
    } else {
        None
    };
    Ok(serde_json::json!({
        "unikey": key,
        "qrurl": create.qrurl,
        "qrimg": qrimg.or(create.qrimg)
    }))
}

fn auth_poll_qr_session(
    runtime: &RuntimeContext,
    payload: PluginPayload,
) -> Result<serde_json::Value, String> {
    let key = payload.key.ok_or_else(|| "missing key".to_string())?;
    let check_key = key.clone();
    let api_response = ncm_call(
        runtime,
        payload.cookie.as_deref(),
        |client, query| async move { client.login_qr_check(&query.param("key", &check_key)).await },
    )?;
    let response_cookies = api_response.cookie.clone();
    let response: QrCheckEnvelope = ncm_decode(api_response)?;
    if response.code != 803 {
        return Ok(serde_json::json!({
            "unikey": key,
            "code": response.code,
            "message": response.message
        }));
    }
    let cookie = response
        .cookie
        .or_else(|| join_response_cookies(&response_cookies))
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "扫码成功但未返回 cookie".to_string())?;
    let login = fetch_login_status(runtime, &cookie)?;
    let account = login
        .account
        .or_else(|| {
            fetch_user_account(runtime, &cookie)
                .ok()
                .and_then(|value| value.account)
        })
        .ok_or_else(|| "登录状态未返回账号信息".to_string())?;
    let profile = login.profile.or_else(|| {
        fetch_user_account(runtime, &cookie)
            .ok()
            .and_then(|value| value.profile)
    });
    let session = StoredSession {
        cookie: cookie.clone(),
        account_id: account.id,
        user_name: account.user_name.clone(),
        nickname: profile.as_ref().and_then(|value| value.nickname.clone()),
        avatar_url: profile.as_ref().and_then(|value| value.avatar_url.clone()),
        fetched_at: now_rfc3339()?,
    };
    save_session(runtime, &session)?;
    Ok(serde_json::json!({
        "code": response.code,
        "cookie": cookie,
        "account": account,
        "profile": profile,
        "backendConfig": {
          "apiBaseUrl": runtime.api_base_url,
          "cookie": session.cookie,
          "accountId": session.account_id.to_string(),
          "nickname": session.nickname,
          "userName": session.user_name,
          "defaultLevel": runtime.default_level
        }
    }))
}

fn auth_get_login_status(
    runtime: &RuntimeContext,
    _payload: PluginPayload,
) -> Result<serde_json::Value, String> {
    let cookie = runtime
        .repo_backend_config
        .cookie
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| load_session(runtime).ok().map(|value| value.cookie));
    let Some(cookie) = cookie else {
        return Ok(serde_json::json!({
            "loggedIn": false,
            "loginExpired": true
        }));
    };
    match fetch_login_status(runtime, &cookie) {
        Ok(status) => Ok(serde_json::json!({
            "loggedIn": status.account.is_some(),
            "loginExpired": status.account.is_none(),
            "account": status.account,
            "profile": status.profile
        })),
        Err(error) => Ok(serde_json::json!({
            "loggedIn": false,
            "loginExpired": true,
            "error": error
        })),
    }
}

fn auth_clear_login(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let path = session_file_path(runtime);
    if path.exists() {
        fs::remove_file(path).map_err(io_error)?;
    }
    Ok(serde_json::json!({ "cleared": true }))
}

fn list_files(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let folders = discover_playlist_folders(runtime)?;
    let files = folders
        .into_iter()
        .flat_map(|folder| {
            folder.tracks.into_iter().map(|track| DiscoveredFile {
                absolute_path: None,
                relative_path: track.relative_path,
                filename: track.filename,
                extension: "mp3".to_string(),
                size_bytes: 0,
                created_at: None,
                modified_at: track.modified_at,
                is_virtual: true,
                provider_id: Some(PROVIDER_ID.to_string()),
                provider_item_id: track
                    .payload
                    .get("songId")
                    .and_then(|value| value.as_i64())
                    .map(|value| value.to_string()),
                source_payload: Some(track.payload),
                local_absolute_path: None,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_value(files).map_err(|error| error.to_string())
}

fn list_tree(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let folders = match discover_playlist_folders(runtime) {
        Ok(folders) => folders,
        Err(_error) if current_login_expired(runtime) => {
            let tree = vec![
                FileTreeNode {
                    path: CREATED_CATEGORY_PATH.to_string(),
                    label: CREATED_CATEGORY_PATH.to_string(),
                    children: Vec::new(),
                },
                FileTreeNode {
                    path: SUBSCRIBED_CATEGORY_PATH.to_string(),
                    label: SUBSCRIBED_CATEGORY_PATH.to_string(),
                    children: Vec::new(),
                },
            ];
            return serde_json::to_value(tree).map_err(|encode_error| encode_error.to_string());
        }
        Err(error) => return Err(error),
    };
    let mut created_children = Vec::new();
    let mut subscribed_children = Vec::new();
    for folder in folders {
        let node = FileTreeNode {
            path: folder.folder_path.clone(),
            label: folder.playlist_name.clone(),
            children: Vec::new(),
        };
        if folder.playlist_category == "created" {
            created_children.push(node);
        } else {
            subscribed_children.push(node);
        }
    }
    let tree = vec![
        FileTreeNode {
            path: CREATED_CATEGORY_PATH.to_string(),
            label: CREATED_CATEGORY_PATH.to_string(),
            children: created_children,
        },
        FileTreeNode {
            path: SUBSCRIBED_CATEGORY_PATH.to_string(),
            label: SUBSCRIBED_CATEGORY_PATH.to_string(),
            children: subscribed_children,
        },
    ];
    serde_json::to_value(tree).map_err(|error| error.to_string())
}

fn list_directory(
    runtime: &RuntimeContext,
    directory_path: &str,
) -> Result<serde_json::Value, String> {
    let normalized = normalize_path(directory_path);
    let folders = match discover_playlist_folders(runtime) {
        Ok(folders) => folders,
        Err(_error) if current_login_expired(runtime) => {
            let entries = if normalized.is_empty() {
                vec![
                    category_entry(CREATED_CATEGORY_PATH, "created", true),
                    category_entry(SUBSCRIBED_CATEGORY_PATH, "subscribed", true),
                ]
            } else if normalized == CREATED_CATEGORY_PATH || normalized == SUBSCRIBED_CATEGORY_PATH
            {
                Vec::new()
            } else {
                Vec::new()
            };
            return serde_json::to_value(entries).map_err(|encode_error| encode_error.to_string());
        }
        Err(error) => return Err(error),
    };
    let entries = if normalized.is_empty() {
        vec![
            category_entry(CREATED_CATEGORY_PATH, "created", false),
            category_entry(SUBSCRIBED_CATEGORY_PATH, "subscribed", false),
        ]
    } else if normalized == CREATED_CATEGORY_PATH || normalized == SUBSCRIBED_CATEGORY_PATH {
        folders
            .iter()
            .filter(|folder| folder.category_path == normalized)
            .map(folder_entry)
            .collect::<Vec<_>>()
    } else if let Some(folder) = folders
        .iter()
        .find(|folder| folder.folder_path == normalized)
    {
        folder.tracks.iter().map(track_entry).collect::<Vec<_>>()
    } else {
        return Err(format!("directory not found: {directory_path}"));
    };
    serde_json::to_value(entries).map_err(|error| error.to_string())
}

fn stat_entry(runtime: &RuntimeContext, entry_path: &str) -> Result<serde_json::Value, String> {
    let normalized = normalize_path(entry_path);
    let folders = match discover_playlist_folders(runtime) {
        Ok(folders) => folders,
        Err(error) if current_login_expired(runtime) => {
            if normalized == CREATED_CATEGORY_PATH {
                return serde_json::to_value(category_entry(
                    CREATED_CATEGORY_PATH,
                    "created",
                    true,
                ))
                .map_err(|encode_error| encode_error.to_string());
            }
            if normalized == SUBSCRIBED_CATEGORY_PATH {
                return serde_json::to_value(category_entry(
                    SUBSCRIBED_CATEGORY_PATH,
                    "subscribed",
                    true,
                ))
                .map_err(|encode_error| encode_error.to_string());
            }
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    if normalized == CREATED_CATEGORY_PATH {
        return serde_json::to_value(category_entry(CREATED_CATEGORY_PATH, "created", false))
            .map_err(|error| error.to_string());
    }
    if normalized == SUBSCRIBED_CATEGORY_PATH {
        return serde_json::to_value(category_entry(
            SUBSCRIBED_CATEGORY_PATH,
            "subscribed",
            false,
        ))
        .map_err(|error| error.to_string());
    }
    if let Some(folder) = folders
        .iter()
        .find(|folder| folder.folder_path == normalized)
    {
        return serde_json::to_value(folder_entry(folder)).map_err(|error| error.to_string());
    }
    if let Some(track) = folders
        .iter()
        .flat_map(|folder| folder.tracks.iter())
        .find(|track| track.relative_path == normalized)
    {
        return serde_json::to_value(track_entry(track)).map_err(|error| error.to_string());
    }
    Err(format!("entry not found: {entry_path}"))
}

fn discover_playlist_folders(runtime: &RuntimeContext) -> Result<Vec<PlaylistFolder>, String> {
    let config = repo_config(runtime)?;
    ensure_login_valid(runtime, &config.cookie)?;
    let playlist_items = fetch_user_playlists(runtime, &config)?;
    let mut folders = Vec::new();
    let mut created_names = std::collections::BTreeSet::new();
    let mut subscribed_names = std::collections::BTreeSet::new();

    for playlist in playlist_items {
        let is_created = playlist
            .creator
            .as_ref()
            .and_then(|creator| creator.user_id)
            .or(playlist.user_id)
            .unwrap_or_default()
            == config.account_id
            && !playlist.subscribed;
        let category_path = if is_created {
            CREATED_CATEGORY_PATH.to_string()
        } else {
            SUBSCRIBED_CATEGORY_PATH.to_string()
        };
        let playlist_category = if is_created { "created" } else { "subscribed" };
        let unique_folder_name = unique_name(
            &sanitize_name(&playlist.name),
            playlist.id,
            if is_created {
                &mut created_names
            } else {
                &mut subscribed_names
            },
        );
        let folder_path = join_path(&category_path, &unique_folder_name);
        match fetch_playlist_detail(runtime, &config.cookie, playlist.id) {
            Ok(detail) => {
                let tracks = hydrate_playlist_tracks(
                    runtime,
                    &config,
                    &detail,
                    playlist_category,
                    &unique_folder_name,
                )?;
                folders.push(PlaylistFolder {
                    category_path,
                    folder_path,
                    playlist_id: playlist.id,
                    playlist_name: unique_folder_name,
                    playlist_category,
                    playlist_track_count: detail.track_count,
                    modified_at: millis_to_rfc3339(detail.update_time.or(playlist.update_time))?,
                    account_id: config.account_id,
                    account_cookie: config.cookie.clone(),
                    load_error: None,
                    tracks,
                });
            }
            Err(error) => {
                folders.push(PlaylistFolder {
                    category_path,
                    folder_path,
                    playlist_id: playlist.id,
                    playlist_name: unique_folder_name,
                    playlist_category,
                    playlist_track_count: playlist.track_count,
                    modified_at: millis_to_rfc3339(playlist.update_time)?,
                    account_id: config.account_id,
                    account_cookie: config.cookie.clone(),
                    load_error: Some(error),
                    tracks: Vec::new(),
                });
            }
        }
    }

    Ok(folders)
}

fn hydrate_playlist_tracks(
    runtime: &RuntimeContext,
    config: &RepoConfig,
    detail: &PlaylistDetailItem,
    playlist_category: &str,
    unique_folder_name: &str,
) -> Result<Vec<DiscoveredSong>, String> {
    let mut songs = if detail.tracks.len() >= detail.track_count as usize {
        detail.tracks.clone()
    } else {
        fetch_song_details(
            runtime,
            &config.cookie,
            &detail
                .track_ids
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
        )?
    };
    let folder_path = join_path(
        if playlist_category == "created" {
            CREATED_CATEGORY_PATH
        } else {
            SUBSCRIBED_CATEGORY_PATH
        },
        unique_folder_name,
    );
    let mut names = std::collections::BTreeSet::new();
    let mut tracks = Vec::new();
    for song in songs.drain(..) {
        let artists = song
            .ar
            .iter()
            .map(|artist| artist.name.clone())
            .collect::<Vec<_>>();
        let display_name = unique_name(
            &sanitize_name(&format!(
                "{} - {}.mp3",
                if artists.is_empty() {
                    "Unknown Artist".to_string()
                } else {
                    artists.join(", ")
                },
                song.name
            )),
            song.id,
            &mut names,
        );
        let relative_path = join_path(&folder_path, &display_name);
        let payload = serde_json::json!({
            "provider": PROVIDER_ID,
            "accountId": config.account_id.to_string(),
            "accountCookie": config.cookie,
            "playlistId": detail.id,
            "playlistName": detail.name,
            "playlistCategory": playlist_category,
            "songId": song.id,
            "songName": song.name,
            "artists": artists,
            "albumName": song.al.as_ref().and_then(|value| value.name.clone()),
            "durationMs": song.dt,
            "coverUrl": song.al.as_ref().and_then(|value| value.pic_url.clone()),
            "audioLevelAvailability": runtime.default_level,
            "privilege": song.privilege,
            "virtualEntry": true,
            "level": config.default_level
        });
        tracks.push(DiscoveredSong {
            relative_path,
            filename: display_name,
            modified_at: millis_to_rfc3339(detail.update_time)?,
            payload,
        });
    }
    Ok(tracks)
}

fn fetch_user_playlists(
    runtime: &RuntimeContext,
    config: &RepoConfig,
) -> Result<Vec<PlaylistSummaryItem>, String> {
    let uid = config.account_id.to_string();
    let response: UserPlaylistsEnvelope = ncm_decode(ncm_call(
        runtime,
        Some(config.cookie.as_str()),
        |client, query| async move {
            client
                .user_playlist(&query.param("uid", &uid).param("limit", "1000"))
                .await
        },
    )?)?;
    Ok(response.playlist)
}

fn fetch_playlist_detail(
    runtime: &RuntimeContext,
    cookie: &str,
    playlist_id: i64,
) -> Result<PlaylistDetailItem, String> {
    let playlist_id = playlist_id.to_string();
    let response: PlaylistDetailEnvelope = ncm_decode(ncm_call(
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

fn fetch_song_details(
    runtime: &RuntimeContext,
    cookie: &str,
    ids: &[i64],
) -> Result<Vec<SongItem>, String> {
    let mut songs = Vec::new();
    for chunk in ids.chunks(200) {
        let joined_ids = chunk
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let response: SongDetailEnvelope = ncm_decode(ncm_call(
            runtime,
            Some(cookie),
            |client, query| async move { client.song_detail(&query.param("ids", &joined_ids)).await },
        )?)?;
        songs.extend(response.songs);
    }
    Ok(songs)
}

fn fetch_login_status(runtime: &RuntimeContext, cookie: &str) -> Result<LoginStatusData, String> {
    ncm_decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move { client.login_status(&query).await },
    )?)
}

fn fetch_user_account(
    runtime: &RuntimeContext,
    cookie: &str,
) -> Result<UserAccountEnvelope, String> {
    ncm_decode(ncm_call(
        runtime,
        Some(cookie),
        |client, query| async move { client.user_account(&query).await },
    )?)
}

fn ensure_login_valid(runtime: &RuntimeContext, cookie: &str) -> Result<(), String> {
    let status = fetch_login_status(runtime, cookie)?;
    if status.account.is_none() {
        return Err("网易云登录已失效，请重新登录".to_string());
    }
    Ok(())
}

fn current_login_expired(runtime: &RuntimeContext) -> bool {
    let cookie = runtime
        .repo_backend_config
        .cookie
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| load_session(runtime).ok().map(|value| value.cookie));
    let Some(cookie) = cookie else {
        return true;
    };
    match fetch_login_status(runtime, &cookie) {
        Ok(status) => status.account.is_none(),
        Err(_) => true,
    }
}

fn repo_backend_config_override(config: Option<&serde_json::Value>) -> RepoBackendConfigOverride {
    let Some(config) = config else {
        return RepoBackendConfigOverride::default();
    };
    RepoBackendConfigOverride {
        cookie: config
            .get("cookie")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        account_id: config.get("accountId").and_then(value_to_i64),
        nickname: config
            .get("nickname")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        user_name: config
            .get("userName")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
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

fn ncm_decode<T: for<'de> Deserialize<'de>>(response: ApiResponse) -> Result<T, String> {
    serde_json::from_value(response.body).map_err(|error| error.to_string())
}

fn extract_qr_unikey(value: &serde_json::Value) -> Option<String> {
    value
        .get("unikey")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn compact_json(value: &serde_json::Value) -> String {
    let mut text = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
    const MAX_LEN: usize = 512;
    if text.len() > MAX_LEN {
        text.truncate(MAX_LEN);
        text.push_str("...");
    }
    text
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

fn qr_svg_data_url(url: &str) -> Result<String, String> {
    let code = QrCode::new(url.as_bytes()).map_err(|error| error.to_string())?;
    let svg = code
        .render::<svg::Color<'_>>()
        .min_dimensions(180, 180)
        .dark_color(svg::Color("#111111"))
        .light_color(svg::Color("#ffffff"))
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        general_purpose::STANDARD.encode(svg.as_bytes())
    ))
}

fn join_response_cookies(values: &[String]) -> Option<String> {
    let cookies = values
        .iter()
        .filter_map(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if cookies.is_empty() {
        None
    } else {
        Some(cookies.join("; "))
    }
}

fn category_entry(path: &str, category: &str, login_expired: bool) -> FileSystemEntry {
    FileSystemEntry {
        path: path.to_string(),
        name: path.to_string(),
        kind: FileSystemEntryKind::Directory,
        extension: None,
        size_bytes: None,
        modified_at: Some(now_rfc3339().unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())),
        is_virtual: true,
        provider_id: Some(PROVIDER_ID.to_string()),
        provider_item_id: None,
        source_payload: Some(serde_json::json!({
            "provider": PROVIDER_ID,
            "entryKind": "playlist-category",
            "playlistCategory": category,
            "loginExpired": login_expired
        })),
        local_absolute_path: None,
    }
}

fn folder_entry(folder: &PlaylistFolder) -> FileSystemEntry {
    FileSystemEntry {
        path: folder.folder_path.clone(),
        name: folder.playlist_name.clone(),
        kind: FileSystemEntryKind::Directory,
        extension: None,
        size_bytes: None,
        modified_at: Some(folder.modified_at.clone()),
        is_virtual: true,
        provider_id: Some(PROVIDER_ID.to_string()),
        provider_item_id: Some(folder.playlist_id.to_string()),
        source_payload: Some(serde_json::json!({
            "provider": PROVIDER_ID,
            "entryKind": "playlist-folder",
            "accountId": folder.account_id.to_string(),
            "accountCookie": folder.account_cookie,
            "playlistId": folder.playlist_id,
            "playlistName": folder.playlist_name,
            "playlistTrackCount": folder.playlist_track_count,
            "playlistCategory": folder.playlist_category,
            "playlistLoadError": folder.load_error
        })),
        local_absolute_path: None,
    }
}

fn track_entry(track: &DiscoveredSong) -> FileSystemEntry {
    FileSystemEntry {
        path: track.relative_path.clone(),
        name: track.filename.clone(),
        kind: FileSystemEntryKind::File,
        extension: Some("mp3".to_string()),
        size_bytes: Some(0),
        modified_at: Some(track.modified_at.clone()),
        is_virtual: true,
        provider_id: Some(PROVIDER_ID.to_string()),
        provider_item_id: track
            .payload
            .get("songId")
            .and_then(|value| value.as_i64())
            .map(|value| value.to_string()),
        source_payload: Some(track.payload.clone()),
        local_absolute_path: None,
    }
}

fn session_file_path(runtime: &RuntimeContext) -> PathBuf {
    runtime.plugin_data_dir.join("last-session.json")
}

fn save_session(runtime: &RuntimeContext, session: &StoredSession) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(session_file_path(runtime), raw).map_err(io_error)
}

fn load_session(runtime: &RuntimeContext) -> Result<StoredSession, String> {
    let raw = fs::read_to_string(session_file_path(runtime)).map_err(io_error)?;
    serde_json::from_str::<StoredSession>(&raw).map_err(|error| error.to_string())
}

fn normalize_path(value: &str) -> String {
    value
        .trim()
        .replace('\\', "/")
        .trim_matches('/')
        .to_string()
}

fn join_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn unique_name(base: &str, id: i64, names: &mut std::collections::BTreeSet<String>) -> String {
    if names.insert(base.to_string()) {
        return base.to_string();
    }
    let fallback = format!("{base} [{id}]");
    names.insert(fallback.clone());
    fallback
}

fn sanitize_name(value: &str) -> String {
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
        "Untitled".to_string()
    } else {
        normalized
    }
}

fn millis_to_rfc3339(value: Option<i64>) -> Result<String, String> {
    match value {
        Some(timestamp) if timestamp > 0 => {
            OffsetDateTime::from_unix_timestamp_nanos(i128::from(timestamp) * 1_000_000)
                .map_err(|error| error.to_string())?
                .format(&Rfc3339)
                .map_err(|error| error.to_string())
        }
        _ => now_rfc3339(),
    }
}

fn now_rfc3339() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

fn http_error(error: impl ToString) -> String {
    error.to_string()
}

fn value_to_i64(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|item| i64::try_from(item).ok()))
        .or_else(|| {
            value
                .as_str()
                .and_then(|item| item.trim().parse::<i64>().ok())
        })
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
                "momobako-source-netease-{name}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&root).expect("test root should be created");
            Self { root }
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
            api_base_url,
            default_level: "standard".to_string(),
            repo_backend_config: RepoBackendConfigOverride::default(),
        }
    }

    #[test]
    fn repo_config_prefers_repository_backend_config_without_saved_session() {
        let workspace = TestWorkspace::new("repo-config-backend-config");
        let runtime = RuntimeContext {
            plugin_data_dir: workspace.root.clone(),
            api_base_url: "https://example.test".to_string(),
            default_level: "higher".to_string(),
            repo_backend_config: RepoBackendConfigOverride {
                cookie: Some("MUSIC_U=repo-cookie".to_string()),
                account_id: Some(123456),
                nickname: Some("云村 Aura".to_string()),
                user_name: Some("Aura".to_string()),
            },
        };

        let config = repo_config(&runtime).expect("repository backend config should be enough");

        assert_eq!(config.cookie, "MUSIC_U=repo-cookie");
        assert_eq!(config.account_id, 123456);
        assert_eq!(config.nickname.as_deref(), Some("云村 Aura"));
        assert_eq!(config.user_name.as_deref(), Some("Aura"));
        assert_eq!(config.default_level, "higher");
    }

    #[test]
    fn current_login_expired_uses_repository_backend_cookie_when_session_file_is_missing() {
        let workspace = TestWorkspace::new("login-expired-backend-cookie");
        let runtime = RuntimeContext {
            plugin_data_dir: workspace.root.clone(),
            api_base_url: serve_json_once(
                r#"{"account":{"id":123456},"profile":{"nickname":"云村 Aura"}}"#,
            ),
            default_level: "standard".to_string(),
            repo_backend_config: RepoBackendConfigOverride {
                cookie: Some("MUSIC_U=repo-cookie".to_string()),
                account_id: Some(123456),
                nickname: None,
                user_name: None,
            },
        };

        assert!(!current_login_expired(&runtime));
    }

    #[test]
    fn auth_get_login_status_uses_repository_backend_cookie_when_session_file_is_missing() {
        let workspace = TestWorkspace::new("login-status-backend-cookie");
        let runtime = RuntimeContext {
            plugin_data_dir: workspace.root.clone(),
            api_base_url: serve_json_once(
                r#"{"account":{"id":123456,"userName":"Aura"},"profile":{"nickname":"云村 Aura"}}"#,
            ),
            default_level: "standard".to_string(),
            repo_backend_config: RepoBackendConfigOverride {
                cookie: Some("MUSIC_U=repo-cookie".to_string()),
                account_id: Some(123456),
                nickname: Some("云村 Aura".to_string()),
                user_name: Some("Aura".to_string()),
            },
        };

        let value = auth_get_login_status(
            &runtime,
            PluginPayload {
                repo_root: None,
                directory_path: None,
                entry_path: None,
                config: None,
                key: None,
                qrimg: None,
                timestamp: None,
                cookie: None,
            },
        )
        .expect("login status should use repository backend config");

        assert_eq!(value["loggedIn"], serde_json::json!(true));
        assert_eq!(value["loginExpired"], serde_json::json!(false));
        assert_eq!(value["account"]["id"], serde_json::json!(123456));
        assert_eq!(value["profile"]["nickname"], serde_json::json!("云村 Aura"));
    }

    #[test]
    fn extract_qr_unikey_supports_sdk_shape() {
        let sdk_shape = serde_json::json!({
            "code": 200,
            "unikey": "sdk-unikey"
        });

        assert_eq!(extract_qr_unikey(&sdk_shape).as_deref(), Some("sdk-unikey"));
    }

    #[test]
    fn normalize_ncm_domain_removes_legacy_api_path_suffixes() {
        assert_eq!(normalize_ncm_domain(Some("https://music.163.com/weapi")), "https://music.163.com");
        assert_eq!(
            normalize_ncm_domain(Some("https://interface.music.163.com/eapi/song/lyric")),
            "https://interface.music.163.com"
        );
        assert_eq!(normalize_ncm_domain(Some("https://music.163.com/api")), "https://music.163.com");
    }

    fn serve_json_once(body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server address should resolve");
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn list_directory_returns_category_placeholders_when_login_expired() {
        let workspace = TestWorkspace::new("login-expired-directory");
        let runtime = runtime_for_test(
            workspace.root.clone(),
            serve_json_once(r#"{"account":null,"profile":null}"#),
        );
        save_session(
            &runtime,
            &StoredSession {
                cookie: "MUSIC_U=expired".to_string(),
                account_id: 42,
                user_name: Some("mock-user".to_string()),
                nickname: Some("mock-nickname".to_string()),
                avatar_url: None,
                fetched_at: "2026-06-14T00:00:00Z".to_string(),
            },
        )
        .expect("session should be saved");

        let value =
            list_directory(&runtime, "").expect("directory listing should degrade gracefully");
        let entries = value
            .as_array()
            .expect("root directory should serialize as an array");

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["path"], serde_json::json!(CREATED_CATEGORY_PATH));
        assert_eq!(
            entries[1]["path"],
            serde_json::json!(SUBSCRIBED_CATEGORY_PATH)
        );
        assert_eq!(
            entries[0]["sourcePayload"]["loginExpired"],
            serde_json::json!(true)
        );
        assert_eq!(
            entries[1]["sourcePayload"]["loginExpired"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn stat_entry_marks_category_as_login_expired_when_session_is_invalid() {
        let workspace = TestWorkspace::new("login-expired-stat");
        let runtime = runtime_for_test(
            workspace.root.clone(),
            serve_json_once(r#"{"account":null,"profile":null}"#),
        );
        save_session(
            &runtime,
            &StoredSession {
                cookie: "MUSIC_U=expired".to_string(),
                account_id: 42,
                user_name: Some("mock-user".to_string()),
                nickname: Some("mock-nickname".to_string()),
                avatar_url: None,
                fetched_at: "2026-06-14T00:00:00Z".to_string(),
            },
        )
        .expect("session should be saved");

        let value = stat_entry(&runtime, CREATED_CATEGORY_PATH)
            .expect("category stat should degrade gracefully");

        assert_eq!(value["path"], serde_json::json!(CREATED_CATEGORY_PATH));
        assert_eq!(
            value["sourcePayload"]["entryKind"],
            serde_json::json!("playlist-category")
        );
        assert_eq!(
            value["sourcePayload"]["loginExpired"],
            serde_json::json!(true)
        );
    }

    fn serve_ncm_sdk_source_test_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server address should resolve");
        thread::spawn(move || {
            let mut playlist_detail_count = 0;
            for _ in 0..64 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buffer = [0_u8; 4096];
                let size = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..size]);
                let line = request.lines().next().unwrap_or_default();
                let path = line.split_whitespace().nth(1).unwrap_or("/");
                let (status, content_type, body) = if path.starts_with("/weapi/w/nuser/account/get") {
                    (
                        "200 OK",
                        "application/json",
                        r#"{"account":{"id":123456,"userName":"Aura"},"profile":{"nickname":"云村 Aura"}} "#
                            .to_string(),
                    )
                } else if path.starts_with("/weapi/user/playlist") {
                    (
                        "200 OK",
                        "application/json",
                        r#"{"playlist":[
                            {"id":9001,"name":"夜跑歌单","trackCount":2,"subscribed":false,"userId":123456,"creator":{"userId":123456},"updateTime":1718323200000},
                            {"id":9002,"name":"夜跑歌单","trackCount":1,"subscribed":false,"userId":123456,"creator":{"userId":123456},"updateTime":1718323200000},
                            {"id":9101,"name":"收藏歌单","trackCount":1,"subscribed":true,"userId":987654,"creator":{"userId":987654},"updateTime":1718323200000}
                        ]}"#
                            .to_string(),
                    )
                } else if path.starts_with("/eapi/v6/playlist/detail") {
                    let body = match playlist_detail_count % 3 {
                        0 => r#"{"playlist":{"id":9001,"name":"夜跑歌单","trackCount":2,"tracks":[],"trackIds":[{"id":2001},{"id":2002}],"updateTime":1718323200000}}"#,
                        1 => r#"{"playlist":{"id":9002,"name":"夜跑歌单","trackCount":1,"tracks":[{"id":2101,"name":"晴天","ar":[{"name":"周杰伦"}],"al":{"name":"叶惠美","picUrl":"https://example.test/cover-2101.jpg"},"dt":269000,"privilege":{"st":0}}],"trackIds":[{"id":2101}],"updateTime":1718323200000}}"#,
                        _ => r#"{"playlist":{"id":9101,"name":"收藏歌单","trackCount":1,"tracks":[{"id":2201,"name":"富士山下","ar":[{"name":"陈奕迅"}],"al":{"name":"What's Going On...?","picUrl":"https://example.test/cover-2201.jpg"},"dt":259000,"privilege":{"fee":0}}],"trackIds":[{"id":2201}],"updateTime":1718323200000}}"#,
                    };
                    playlist_detail_count += 1;
                    ("200 OK", "application/json", body.to_string())
                } else if path.starts_with("/weapi/v3/song/detail") {
                    (
                        "200 OK",
                        "application/json",
                        r#"{"songs":[
                            {"id":2001,"name":"稻香","ar":[{"name":"周杰伦"}],"al":{"name":"魔杰座","picUrl":"https://example.test/cover-2001.jpg"},"dt":223000,"privilege":{"st":0}},
                            {"id":2002,"name":"稻香","ar":[{"name":"周杰伦"}],"al":{"name":"魔杰座","picUrl":"https://example.test/cover-2002.jpg"},"dt":223000,"privilege":{"st":-1}}
                        ]}"#
                            .to_string(),
                    )
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

    fn serve_ncm_sdk_source_test_server_with_blocked_playlist() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("test server should bind");
        let addr = listener
            .local_addr()
            .expect("test server address should resolve");
        thread::spawn(move || {
            let mut playlist_detail_count = 0;
            for _ in 0..64 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buffer = [0_u8; 4096];
                let size = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..size]);
                let line = request.lines().next().unwrap_or_default();
                let path = line.split_whitespace().nth(1).unwrap_or("/");
                let (status, content_type, body) = if path.starts_with("/weapi/w/nuser/account/get") {
                    (
                        "200 OK",
                        "application/json",
                        r#"{"account":{"id":123456,"userName":"Aura"},"profile":{"nickname":"云村 Aura"}}"#
                            .to_string(),
                    )
                } else if path.starts_with("/weapi/user/playlist") {
                    (
                        "200 OK",
                        "application/json",
                        r#"{"playlist":[
                            {"id":9001,"name":"夜跑歌单","trackCount":2,"subscribed":false,"userId":123456,"creator":{"userId":123456},"updateTime":1718323200000},
                            {"id":9101,"name":"收藏歌单","trackCount":1,"subscribed":true,"userId":987654,"creator":{"userId":987654},"updateTime":1718323200000}
                        ]}"#
                            .to_string(),
                    )
                } else if path.starts_with("/eapi/v6/playlist/detail") {
                    let body = if playlist_detail_count == 0 {
                        r#"{"playlist":{"id":9001,"name":"夜跑歌单","trackCount":2,"tracks":[],"trackIds":[{"id":2001},{"id":2002}],"updateTime":1718323200000}}"#
                            .to_string()
                    } else {
                        r#"{"code":404,"msg":"歌单涉嫌违规，审核中"}"#.to_string()
                    };
                    let status = if playlist_detail_count == 0 {
                        "200 OK"
                    } else {
                        "404 Not Found"
                    };
                    playlist_detail_count += 1;
                    (status, "application/json", body)
                } else if path.starts_with("/weapi/v3/song/detail") {
                    (
                        "200 OK",
                        "application/json",
                        r#"{"songs":[
                            {"id":2001,"name":"稻香","ar":[{"name":"周杰伦"}],"al":{"name":"魔杰座","picUrl":"https://example.test/cover-2001.jpg"},"dt":223000,"privilege":{"st":0}},
                            {"id":2002,"name":"稻香","ar":[{"name":"周杰伦"}],"al":{"name":"魔杰座","picUrl":"https://example.test/cover-2002.jpg"},"dt":223000,"privilege":{"st":-1}}
                        ]}"#
                            .to_string(),
                    )
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
    fn list_directory_classifies_playlists_handles_name_collisions_and_exposes_track_metadata() {
        let workspace = TestWorkspace::new("directory-metadata");
        let runtime = RuntimeContext {
            plugin_data_dir: workspace.root.clone(),
            api_base_url: serve_ncm_sdk_source_test_server(),
            default_level: "lossless".to_string(),
            repo_backend_config: RepoBackendConfigOverride {
                cookie: Some("MUSIC_U=repo-cookie".to_string()),
                account_id: Some(123456),
                nickname: Some("云村 Aura".to_string()),
                user_name: Some("Aura".to_string()),
            },
        };

        let root = list_directory(&runtime, "")
            .expect("root listing should succeed")
            .as_array()
            .cloned()
            .expect("root should serialize as array");
        assert_eq!(root.len(), 2);
        assert_eq!(root[0]["path"], serde_json::json!(CREATED_CATEGORY_PATH));
        assert_eq!(root[1]["path"], serde_json::json!(SUBSCRIBED_CATEGORY_PATH));

        let created = list_directory(&runtime, CREATED_CATEGORY_PATH)
            .expect("created playlists should load")
            .as_array()
            .cloned()
            .expect("created playlist list should serialize as array");
        assert_eq!(created.len(), 2);
        assert_eq!(created[0]["path"], serde_json::json!("创建的歌单/夜跑歌单"));
        assert_eq!(
            created[0]["sourcePayload"]["entryKind"],
            serde_json::json!("playlist-folder")
        );
        assert_eq!(
            created[0]["sourcePayload"]["playlistId"],
            serde_json::json!(9001)
        );
        assert_eq!(
            created[0]["sourcePayload"]["playlistName"],
            serde_json::json!("夜跑歌单")
        );
        assert_eq!(
            created[0]["sourcePayload"]["playlistTrackCount"],
            serde_json::json!(2)
        );
        assert_eq!(
            created[0]["sourcePayload"]["playlistCategory"],
            serde_json::json!("created")
        );
        assert_eq!(
            created[1]["path"],
            serde_json::json!("创建的歌单/夜跑歌单 [9002]")
        );

        let subscribed = list_directory(&runtime, SUBSCRIBED_CATEGORY_PATH)
            .expect("subscribed playlists should load")
            .as_array()
            .cloned()
            .expect("subscribed playlist list should serialize as array");
        assert_eq!(subscribed.len(), 1);
        assert_eq!(
            subscribed[0]["path"],
            serde_json::json!("收藏的歌单/收藏歌单")
        );
        assert_eq!(
            subscribed[0]["sourcePayload"]["playlistCategory"],
            serde_json::json!("subscribed")
        );

        let tracks = list_directory(&runtime, "创建的歌单/夜跑歌单")
            .expect("track listing should succeed")
            .as_array()
            .cloned()
            .expect("track list should serialize as array");
        assert_eq!(tracks.len(), 2);
        assert_eq!(
            tracks[0]["path"],
            serde_json::json!("创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3")
        );
        assert_eq!(
            tracks[1]["path"],
            serde_json::json!("创建的歌单/夜跑歌单/周杰伦 - 稻香.mp3 [2002]")
        );
        assert_eq!(tracks[0]["providerId"], serde_json::json!(PROVIDER_ID));
        assert_eq!(tracks[0]["isVirtual"], serde_json::json!(true));
        assert_eq!(
            tracks[0]["sourcePayload"]["provider"],
            serde_json::json!(PROVIDER_ID)
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["accountId"],
            serde_json::json!("123456")
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["playlistId"],
            serde_json::json!(9001)
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["playlistName"],
            serde_json::json!("夜跑歌单")
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["playlistCategory"],
            serde_json::json!("created")
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["songId"],
            serde_json::json!(2001)
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["songName"],
            serde_json::json!("稻香")
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["artists"],
            serde_json::json!(["周杰伦"])
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["albumName"],
            serde_json::json!("魔杰座")
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["durationMs"],
            serde_json::json!(223000)
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["coverUrl"],
            serde_json::json!("https://example.test/cover-2001.jpg")
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["audioLevelAvailability"],
            serde_json::json!("lossless")
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["privilege"],
            serde_json::json!({"st":0})
        );
        assert_eq!(
            tracks[0]["sourcePayload"]["virtualEntry"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn discover_playlist_folders_keeps_blocked_playlist_as_empty_folder() {
        let workspace = TestWorkspace::new("blocked-playlist");
        let runtime = RuntimeContext {
            plugin_data_dir: workspace.root.clone(),
            api_base_url: serve_ncm_sdk_source_test_server_with_blocked_playlist(),
            default_level: "standard".to_string(),
            repo_backend_config: RepoBackendConfigOverride {
                cookie: Some("MUSIC_U=repo-cookie".to_string()),
                account_id: Some(123456),
                nickname: Some("云村 Aura".to_string()),
                user_name: Some("Aura".to_string()),
            },
        };

        let subscribed = list_directory(&runtime, SUBSCRIBED_CATEGORY_PATH)
            .expect("subscribed playlists should still list")
            .as_array()
            .cloned()
            .expect("subscribed playlists should serialize as array");
        assert_eq!(subscribed.len(), 1);
        assert_eq!(
            subscribed[0]["path"],
            serde_json::json!("收藏的歌单/收藏歌单")
        );
        assert_eq!(
            subscribed[0]["sourcePayload"]["playlistTrackCount"],
            serde_json::json!(1)
        );
        assert_eq!(
            subscribed[0]["sourcePayload"]["playlistLoadError"],
            serde_json::json!("API error (code=404): 歌单涉嫌违规，审核中")
        );

        let blocked_tracks = list_directory(&runtime, "收藏的歌单/收藏歌单")
            .expect("blocked playlist folder should degrade to empty list")
            .as_array()
            .cloned()
            .expect("blocked playlist entries should serialize as array");
        assert!(blocked_tracks.is_empty());
    }
}
