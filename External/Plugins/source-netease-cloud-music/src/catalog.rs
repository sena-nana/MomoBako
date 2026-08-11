//! 网易云歌单到只读虚拟目录的映射。

use std::collections::BTreeSet;

use crate::{
    auth, client,
    models::{
        DirectoryPageResponse, DiscoveredFile, DiscoveredSong, FileSystemEntry,
        FileSystemEntryKind, FileTreeNode, PlaylistDetailItem, PlaylistFolder, RepoConfig,
        RuntimeContext, SongItem, CREATED_CATEGORY_PATH, PROVIDER_ID, SUBSCRIBED_CATEGORY_PATH,
    },
    util::{join_path, millis_to_rfc3339, normalize_path, now_rfc3339, sanitize_name, unique_name},
};

pub(crate) fn list_files(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let files = discover_playlist_folders(runtime)?
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

pub(crate) fn list_tree(runtime: &RuntimeContext) -> Result<serde_json::Value, String> {
    let folders = match discover_playlist_folder_summaries(runtime) {
        Ok((_, _, folders)) => folders,
        Err(_) if auth::current_login_expired(runtime) => Vec::new(),
        Err(error) => return Err(error),
    };
    let mut created = Vec::new();
    let mut subscribed = Vec::new();
    for folder in folders {
        let node = FileTreeNode {
            path: folder.folder_path,
            label: folder.playlist_name,
            children: Vec::new(),
        };
        if folder.playlist_category == "created" {
            created.push(node)
        } else {
            subscribed.push(node)
        }
    }
    serde_json::to_value(vec![
        FileTreeNode {
            path: CREATED_CATEGORY_PATH.into(),
            label: CREATED_CATEGORY_PATH.into(),
            children: created,
        },
        FileTreeNode {
            path: SUBSCRIBED_CATEGORY_PATH.into(),
            label: SUBSCRIBED_CATEGORY_PATH.into(),
            children: subscribed,
        },
    ])
    .map_err(|error| error.to_string())
}

pub(crate) fn list_directory(
    runtime: &RuntimeContext,
    directory_path: &str,
) -> Result<serde_json::Value, String> {
    serde_json::to_value(list_directory_entries(runtime, directory_path)?)
        .map_err(|error| error.to_string())
}

pub(crate) fn list_directory_page(
    runtime: &RuntimeContext,
    directory_path: &str,
    offset: usize,
    limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    let normalized = normalize_path(directory_path);
    let Some(limit) = limit.filter(|value| *value > 0) else {
        let entries = list_directory_entries(runtime, directory_path)?;
        return serde_json::to_value(DirectoryPageResponse {
            total_entries: entries.len(),
            entries,
        })
        .map_err(|error| error.to_string());
    };
    let page = if normalized.is_empty()
        || normalized == CREATED_CATEGORY_PATH
        || normalized == SUBSCRIBED_CATEGORY_PATH
    {
        let entries = list_directory_entries(runtime, directory_path)?;
        DirectoryPageResponse {
            total_entries: entries.len(),
            entries: entries.into_iter().skip(offset).take(limit).collect(),
        }
    } else {
        list_playlist_directory_page(runtime, &normalized, offset, limit)?
    };
    serde_json::to_value(page).map_err(|error| error.to_string())
}

fn list_directory_entries(
    runtime: &RuntimeContext,
    directory_path: &str,
) -> Result<Vec<FileSystemEntry>, String> {
    let normalized = normalize_path(directory_path);
    if normalized.is_empty() {
        let expired = auth::current_login_expired(runtime);
        return Ok(vec![
            category_entry(CREATED_CATEGORY_PATH, "created", expired),
            category_entry(SUBSCRIBED_CATEGORY_PATH, "subscribed", expired),
        ]);
    }
    if normalized == CREATED_CATEGORY_PATH || normalized == SUBSCRIBED_CATEGORY_PATH {
        let folders = match discover_playlist_folder_summaries(runtime) {
            Ok((_, _, folders)) => folders,
            Err(_) if auth::current_login_expired(runtime) => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        return Ok(folders
            .iter()
            .filter(|folder| folder.category_path == normalized)
            .map(folder_entry)
            .collect());
    }
    let folder = match discover_playlist_folder(runtime, &normalized) {
        Ok(folder) => folder,
        Err(_) if auth::current_login_expired(runtime) => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(folder.tracks.iter().map(track_entry).collect())
}

fn list_playlist_directory_page(
    runtime: &RuntimeContext,
    folder_path: &str,
    offset: usize,
    limit: usize,
) -> Result<DirectoryPageResponse, String> {
    let (config, cookie, folders) = discover_playlist_folder_summaries(runtime)?;
    let folder = folders
        .into_iter()
        .find(|folder| folder.folder_path == folder_path)
        .ok_or_else(|| format!("directory not found: {folder_path}"))?;
    match client::fetch_playlist_detail(runtime, &cookie, folder.playlist_id) {
        Ok(detail) => {
            let total_entries = playlist_track_total(&detail);
            let tracks = hydrate_playlist_track_page(
                runtime,
                &config,
                &cookie,
                &detail,
                folder.playlist_category,
                &folder.playlist_name,
                offset,
                limit,
            )?;
            Ok(DirectoryPageResponse {
                total_entries,
                entries: tracks.iter().map(track_entry).collect(),
            })
        }
        Err(_) => Ok(DirectoryPageResponse {
            total_entries: 0,
            entries: Vec::new(),
        }),
    }
}

pub(crate) fn stat_entry(
    runtime: &RuntimeContext,
    entry_path: &str,
) -> Result<serde_json::Value, String> {
    let normalized = normalize_path(entry_path);
    for (path, category) in [
        (CREATED_CATEGORY_PATH, "created"),
        (SUBSCRIBED_CATEGORY_PATH, "subscribed"),
    ] {
        if normalized == path {
            return serde_json::to_value(category_entry(
                path,
                category,
                auth::current_login_expired(runtime),
            ))
            .map_err(|error| error.to_string());
        }
    }
    let (_, _, folders) = discover_playlist_folder_summaries(runtime)?;
    if let Some(folder) = folders
        .iter()
        .find(|folder| folder.folder_path == normalized)
    {
        return serde_json::to_value(folder_entry(folder)).map_err(|error| error.to_string());
    }
    if let Some(folder) = folders
        .iter()
        .find(|folder| normalized.starts_with(&format!("{}/", folder.folder_path)))
    {
        let hydrated = discover_playlist_folder(runtime, &folder.folder_path)?;
        if let Some(track) = hydrated
            .tracks
            .iter()
            .find(|track| track.relative_path == normalized)
        {
            return serde_json::to_value(track_entry(track)).map_err(|error| error.to_string());
        }
    }
    Err(format!("entry not found: {entry_path}"))
}

fn discover_playlist_folder_summaries(
    runtime: &RuntimeContext,
) -> Result<(RepoConfig, String, Vec<PlaylistFolder>), String> {
    let (config, cookie) = auth::resolve_repository_credential(runtime)?;
    ensure_login_valid(runtime, &cookie)?;
    let items = client::fetch_user_playlists(runtime, &cookie, config.account_id)?;
    let mut folders = Vec::new();
    let mut created_names = BTreeSet::new();
    let mut subscribed_names = BTreeSet::new();
    for playlist in items {
        let is_created = playlist
            .creator
            .as_ref()
            .and_then(|value| value.user_id)
            .or(playlist.user_id)
            .unwrap_or_default()
            == config.account_id
            && !playlist.subscribed;
        let (category_path, category, names) = if is_created {
            (CREATED_CATEGORY_PATH, "created", &mut created_names)
        } else {
            (
                SUBSCRIBED_CATEGORY_PATH,
                "subscribed",
                &mut subscribed_names,
            )
        };
        let name = unique_name(&sanitize_name(&playlist.name), playlist.id, names);
        folders.push(PlaylistFolder {
            category_path: category_path.to_string(),
            folder_path: join_path(category_path, &name),
            playlist_id: playlist.id,
            playlist_name: name,
            playlist_category: category,
            playlist_track_count: playlist.track_count,
            playlist_cover_url: playlist.cover_img_url,
            modified_at: millis_to_rfc3339(playlist.update_time)?,
            account_id: config.account_id,
            load_error: None,
            tracks: Vec::new(),
        });
    }
    Ok((config, cookie, folders))
}

fn discover_playlist_folder(
    runtime: &RuntimeContext,
    path: &str,
) -> Result<PlaylistFolder, String> {
    let (config, cookie, folders) = discover_playlist_folder_summaries(runtime)?;
    let mut folder = folders
        .into_iter()
        .find(|folder| folder.folder_path == path)
        .ok_or_else(|| format!("directory not found: {path}"))?;
    match client::fetch_playlist_detail(runtime, &cookie, folder.playlist_id) {
        Ok(detail) => {
            folder.playlist_track_count = detail.track_count;
            if detail.update_time.is_some() {
                folder.modified_at = millis_to_rfc3339(detail.update_time)?
            }
            folder.tracks = hydrate_playlist_tracks(
                runtime,
                &config,
                &cookie,
                &detail,
                folder.playlist_category,
                &folder.playlist_name,
            )?;
        }
        Err(error) => folder.load_error = Some(error),
    }
    Ok(folder)
}

fn discover_playlist_folders(runtime: &RuntimeContext) -> Result<Vec<PlaylistFolder>, String> {
    let (config, cookie, mut folders) = discover_playlist_folder_summaries(runtime)?;
    for folder in &mut folders {
        match client::fetch_playlist_detail(runtime, &cookie, folder.playlist_id) {
            Ok(detail) => {
                folder.playlist_track_count = detail.track_count;
                if detail.update_time.is_some() {
                    folder.modified_at = millis_to_rfc3339(detail.update_time)?
                }
                folder.tracks = hydrate_playlist_tracks(
                    runtime,
                    &config,
                    &cookie,
                    &detail,
                    folder.playlist_category,
                    &folder.playlist_name,
                )?;
            }
            Err(error) => folder.load_error = Some(error),
        }
    }
    Ok(folders)
}

fn playlist_track_total(detail: &PlaylistDetailItem) -> usize {
    detail
        .track_ids
        .len()
        .max(detail.tracks.len())
        .max(detail.track_count.max(0) as usize)
}

fn hydrate_playlist_tracks(
    runtime: &RuntimeContext,
    config: &RepoConfig,
    cookie: &str,
    detail: &PlaylistDetailItem,
    playlist_category: &str,
    unique_folder_name: &str,
) -> Result<Vec<DiscoveredSong>, String> {
    let songs = load_playlist_songs_until(runtime, cookie, detail, playlist_track_total(detail))?;
    let category = if playlist_category == "created" {
        CREATED_CATEGORY_PATH
    } else {
        SUBSCRIBED_CATEGORY_PATH
    };
    let folder_path = join_path(category, unique_folder_name);
    build_tracks(
        runtime,
        config,
        detail,
        playlist_category,
        &folder_path,
        songs,
    )
}

fn hydrate_playlist_track_page(
    runtime: &RuntimeContext,
    config: &RepoConfig,
    cookie: &str,
    detail: &PlaylistDetailItem,
    playlist_category: &str,
    unique_folder_name: &str,
    offset: usize,
    limit: usize,
) -> Result<Vec<DiscoveredSong>, String> {
    let total = playlist_track_total(detail);
    if offset >= total || limit == 0 {
        return Ok(Vec::new());
    }
    let end = offset.saturating_add(limit).min(total);
    let songs = load_playlist_songs_until(runtime, cookie, detail, end)?;
    let category = if playlist_category == "created" {
        CREATED_CATEGORY_PATH
    } else {
        SUBSCRIBED_CATEGORY_PATH
    };
    let folder_path = join_path(category, unique_folder_name);
    Ok(build_tracks(
        runtime,
        config,
        detail,
        playlist_category,
        &folder_path,
        songs,
    )?
    .into_iter()
    .skip(offset)
    .collect())
}

fn load_playlist_songs_until(
    runtime: &RuntimeContext,
    cookie: &str,
    detail: &PlaylistDetailItem,
    end: usize,
) -> Result<Vec<SongItem>, String> {
    let end = end.min(playlist_track_total(detail));
    if end == 0 {
        return Ok(Vec::new());
    }
    if detail.tracks.len() >= end {
        return Ok(detail.tracks.iter().take(end).cloned().collect());
    }
    if detail.track_ids.is_empty() {
        return Ok(detail.tracks.iter().take(end).cloned().collect());
    }
    let ids = detail
        .track_ids
        .iter()
        .take(end)
        .map(|value| value.id)
        .collect::<Vec<_>>();
    client::fetch_song_details(runtime, cookie, &ids)
}

fn build_tracks(
    runtime: &RuntimeContext,
    config: &RepoConfig,
    detail: &PlaylistDetailItem,
    playlist_category: &str,
    folder_path: &str,
    songs: Vec<SongItem>,
) -> Result<Vec<DiscoveredSong>, String> {
    let mut names = BTreeSet::new();
    let modified_at = millis_to_rfc3339(detail.update_time)?;
    songs
        .into_iter()
        .map(|song| {
            let artists = song
                .ar
                .iter()
                .map(|value| value.name.clone())
                .collect::<Vec<_>>();
            let artist = if artists.is_empty() {
                "Unknown Artist".to_string()
            } else {
                artists.join(", ")
            };
            let display_name = unique_name(
                &sanitize_name(&format!("{artist} - {}.mp3", song.name)),
                song.id,
                &mut names,
            );
            Ok(DiscoveredSong {
                relative_path: join_path(folder_path, &display_name),
                filename: display_name,
                modified_at: modified_at.clone(),
                payload: serde_json::json!({
                    "provider": PROVIDER_ID,
                    "entryKind": "track",
                    "accountId": config.account_id.to_string(),
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
                }),
            })
        })
        .collect()
}

fn ensure_login_valid(runtime: &RuntimeContext, cookie: &str) -> Result<(), String> {
    if client::fetch_login_status(runtime, cookie)?
        .account
        .is_none()
    {
        Err("网易云登录已失效，请重新登录".to_string())
    } else {
        Ok(())
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
            "provider": PROVIDER_ID, "entryKind": "playlist-category",
            "playlistCategory": category, "loginExpired": login_expired
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
            "provider": PROVIDER_ID, "entryKind": "playlist-folder",
            "accountId": folder.account_id.to_string(), "playlistId": folder.playlist_id,
            "playlistName": folder.playlist_name, "playlistTrackCount": folder.playlist_track_count,
            "playlistCoverUrl": folder.playlist_cover_url, "playlistCategory": folder.playlist_category,
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
