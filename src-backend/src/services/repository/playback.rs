//! Source-backed playlist download orchestration with host-owned progress and cancellation.

use super::*;

/// Downloads a playlist through its Source plugin while reporting aggregated progress.
#[cfg(test)]
pub(crate) fn download_playlist_with_progress(
    state: &RepositoryState,
    request: DownloaderPlaylistRequest,
    emit: &mut dyn FnMut(DownloaderPlaylistProgressEvent) -> Result<(), String>,
) -> Result<serde_json::Value, String> {
    download_playlist_with_progress_cancellable(state, request, &NeverCancelled, emit)
}

/// Downloads a playlist and observes cooperative cancellation between tracks.
pub(crate) fn download_playlist_with_progress_cancellable(
    state: &RepositoryState,
    request: DownloaderPlaylistRequest,
    cancellation: &dyn CancellationCheck,
    emit: &mut dyn FnMut(DownloaderPlaylistProgressEvent) -> Result<(), String>,
) -> Result<serde_json::Value, String> {
    let source_repository_id = request.source_repository_id.clone();
    let playlist_name = request.playlist_name.clone();
    let total = request.tracks.len();
    let default_level = request
        .level
        .clone()
        .unwrap_or_else(|| "standard".to_string());
    let default_source_payload = request.source_payload.clone();
    let managed_cache_root = request.managed_cache_root.clone();
    let destination = request.destination.clone();

    cancellation.checkpoint()?;
    emit(DownloaderPlaylistProgressEvent {
        phase: "start".to_string(),
        playlist_id: request.playlist_id,
        playlist_name: playlist_name.clone(),
        total,
        completed: 0,
        failed: 0,
        current_song_id: None,
        current_song_name: None,
        error: None,
    })?;

    let mut completed = Vec::new();
    let mut failed = Vec::new();
    for track in request.tracks {
        cancellation.checkpoint()?;
        let current_song_name = track.song_name.clone();
        let source_payload = sanitize_source_payload(
            track
                .source_payload
                .clone()
                .or_else(|| default_source_payload.clone())
                .unwrap_or_else(|| serde_json::json!({})),
        );
        let payload = serde_json::json!({
            "songId": track.song_id,
            "level": default_level,
            "destination": destination,
            "managedCacheRoot": managed_cache_root.clone(),
            "sourcePayload": source_payload,
        });
        let track_result =
            call_source_download_entry(state, source_repository_id.as_deref(), payload);
        cancellation.checkpoint()?;
        match track_result {
            Ok(value) => {
                completed.push(value);
                emit(DownloaderPlaylistProgressEvent {
                    phase: "track".to_string(),
                    playlist_id: request.playlist_id,
                    playlist_name: playlist_name.clone(),
                    total,
                    completed: completed.len(),
                    failed: failed.len(),
                    current_song_id: Some(track.song_id),
                    current_song_name,
                    error: None,
                })?;
            }
            Err(error) => {
                failed.push(serde_json::json!({
                    "songId": track.song_id,
                    "error": error,
                }));
                emit(DownloaderPlaylistProgressEvent {
                    phase: "track".to_string(),
                    playlist_id: request.playlist_id,
                    playlist_name: playlist_name.clone(),
                    total,
                    completed: completed.len(),
                    failed: failed.len(),
                    current_song_id: Some(track.song_id),
                    current_song_name,
                    error: failed
                        .last()
                        .and_then(|value| value.get("error"))
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string),
                })?;
            }
        }
    }

    let response = serde_json::json!({
        "playlistId": request.playlist_id,
        "playlistName": playlist_name,
        "completed": completed,
        "failed": failed,
        "summary": {
            "total": total,
            "succeeded": completed.len(),
            "failed": failed.len()
        }
    });
    cancellation.checkpoint()?;
    emit(DownloaderPlaylistProgressEvent {
        phase: "complete".to_string(),
        playlist_id: request.playlist_id,
        playlist_name: request.playlist_name,
        total,
        completed: completed.len(),
        failed: failed.len(),
        current_song_id: None,
        current_song_name: None,
        error: None,
    })?;
    Ok(response)
}

/// 仅把公开来源字段传给 Source，兼容旧请求时也会剥离历史凭据字段。
fn sanitize_source_payload(value: serde_json::Value) -> serde_json::Value {
    let mut object = value.as_object().cloned().unwrap_or_default();
    object.retain(|key, _| {
        let normalized = key.to_ascii_lowercase();
        !normalized.contains("cookie")
            && !normalized.contains("password")
            && !normalized.contains("secret")
            && normalized != "token"
    });
    serde_json::Value::Object(object)
}

/// 根据来源仓库清单解析下载方法；宿主不识别网易云方法或协议。
fn call_source_download_entry(
    state: &RepositoryState,
    source_repository_id: Option<&str>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[cfg(test)]
    if let Some(hook) = test_support::downloader_track_package_hook()? {
        return hook(payload);
    }

    let source_repository_id = source_repository_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "sourceRepositoryId is required for playlist downloads".to_string())?;
    let repository = state.load_repository_record(source_repository_id)?;
    let catalog = plugin_catalog(&state.root);
    let manifest = catalog
        .manifest(&repository.backend_record.plugin_id)
        .ok_or_else(|| {
            format!(
                "source plugin is unavailable: {}",
                repository.backend_record.plugin_id
            )
        })?;
    if !is_source_plugin(manifest) {
        return Err(format!(
            "repository backend is not a Source: {}",
            manifest.plugin_id
        ));
    }
    let method = manifest
        .contributes
        .get("source")
        .and_then(|source| source.get("media"))
        .and_then(|media| media.get("downloadEntryMethod"))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            format!(
                "source plugin does not declare contributes.source.media.downloadEntryMethod: {}",
                manifest.plugin_id
            )
        })?
        .to_string();
    state
        .call_plugin(PluginCallRequest {
            plugin_id: manifest.plugin_id.clone(),
            method,
            repository_id: Some(source_repository_id.to_string()),
            payload,
        })
        .map(|response| response.payload)
}
