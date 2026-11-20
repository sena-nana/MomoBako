//! 网易云缓存根目录规范化、缓存状态合并与播放缓存迁移。

use super::*;

pub(in super::super) fn normalize_external_cache_root(path: &str) -> Result<PathBuf, String> {
    let cache_root = PathBuf::from(path);
    if cache_root.exists() {
        return canonicalize_local_path(&cache_root);
    }
    if let Some(parent) = cache_root.parent() {
        if parent.exists() {
            let parent = canonicalize_local_path(parent)?;
            if let Some(name) = cache_root.file_name() {
                return Ok(parent.join(name));
            }
        }
    }
    if cache_root.is_relative() {
        return Ok(std::env::current_dir().map_err(io_error)?.join(cache_root));
    }
    Ok(cache_root)
}

pub(in super::super) fn merge_netease_cache_state_contents(
    source: &Path,
    target: &Path,
) -> Result<usize, String> {
    fs::create_dir_all(target).map_err(io_error)?;
    if !source.exists() {
        return Ok(0);
    }
    let mut moved = 0;
    for entry in fs::read_dir(source).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name();
        let source_path = entry.path();
        let target_path = target.join(&file_name);
        if target_path.exists() {
            if source_path.is_dir() && target_path.is_dir() {
                moved += merge_netease_cache_state_contents(&source_path, &target_path)?;
            }
            continue;
        }
        fs::rename(&source_path, &target_path).map_err(io_error)?;
        moved += 1;
    }
    Ok(moved)
}

pub(in super::super) fn netease_playback_cache_dir(cache_root: &Path) -> PathBuf {
    repository_meta_dir(cache_root)
        .join("cache")
        .join("netease-playback")
}

pub(in super::super) fn downloader_legacy_temp_dir(service_root: &Path) -> PathBuf {
    plugin_data_dir(service_root, "momobako.service.downloader").join("temp")
}

pub(in super::super) fn netease_downloader_cache_key(
    song_id: i64,
    level: &str,
    account_id: &str,
) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{song_id}:{level}:{account_id}"));
    format!("{:x}", Sha1Digest::finalize(hasher))
}

pub(in super::super) fn value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub(in super::super) fn migrate_netease_playback_cache(
    service_root: &Path,
    repo: &RepositoryRecord,
    cache_root: &Path,
    migration: &mut NeteaseRepositoryCacheMigrationSummary,
) -> Result<(), String> {
    let legacy_temp_dir = downloader_legacy_temp_dir(service_root);
    if !legacy_temp_dir.exists() {
        return Ok(());
    }
    let account_id = repo
        .backend_record
        .config
        .get("accountId")
        .and_then(value_as_string)
        .unwrap_or_else(|| "anonymous".to_string());
    let default_level = repo
        .backend_record
        .config
        .get("defaultLevel")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("standard");
    let connection =
        match self_open_repository_connection_for_cache_migration(service_root, repo, cache_root) {
            Ok(connection) => connection,
            Err(_) => return Ok(()),
        };
    let asset_map = load_asset_path_map(&connection, &repo.summary.repo_id).map_err(db_error)?;
    let target_dir = netease_playback_cache_dir(cache_root);
    fs::create_dir_all(&target_dir).map_err(io_error)?;

    for asset in asset_map.values() {
        if asset.provider_id.as_deref() != Some("netease-cloud-music") {
            continue;
        }
        let Some(payload) = asset.source_payload.as_ref() else {
            migration.skipped_playback_cache_files += 1;
            continue;
        };
        let song_id = payload
            .get("songId")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                payload
                    .get("songId")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| value.parse::<i64>().ok())
            });
        let Some(song_id) = song_id else {
            migration.skipped_playback_cache_files += 1;
            continue;
        };
        let level = payload
            .get("level")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(default_level);
        let cache_key = netease_downloader_cache_key(song_id, level, &account_id);
        for extension in ["mp3", "lrc", "yrc"] {
            let source_path = legacy_temp_dir.join(format!("{cache_key}.{extension}"));
            if !source_path.exists() {
                continue;
            }
            let target_path = target_dir.join(format!("{cache_key}.{extension}"));
            if target_path.exists() {
                migration.skipped_playback_cache_files += 1;
                continue;
            }
            match fs::rename(&source_path, &target_path) {
                Ok(()) => migration.migrated_playback_cache_files += 1,
                Err(_) => migration.failed_playback_cache_files += 1,
            }
        }
    }
    Ok(())
}

pub(in super::super) fn self_open_repository_connection_for_cache_migration(
    service_root: &Path,
    repo: &RepositoryRecord,
    cache_root: &Path,
) -> Result<Connection, String> {
    let storage_paths = ensure_repository_storage_paths(
        service_root,
        &repo.summary.repo_id,
        cache_root,
        &repo.backend_record.plugin_id,
    )?;
    open_repository_database_connection(&storage_paths.database_path)
}
