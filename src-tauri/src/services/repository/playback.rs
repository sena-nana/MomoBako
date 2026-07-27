//! Playback service helpers for downloader-backed playback preparation and playlist exports.

use super::*;

/// Downloads a playlist through the downloader plugin while reporting aggregated progress.
#[cfg(test)]
pub(crate) fn download_playlist_with_progress(
    service_root: &Path,
    request: DownloaderPlaylistRequest,
    emit: &mut dyn FnMut(DownloaderPlaylistProgressEvent) -> Result<(), String>,
) -> Result<serde_json::Value, String> {
    download_playlist_with_progress_cancellable(service_root, request, &NeverCancelled, emit)
}

/// Downloads a playlist and observes cooperative cancellation between tracks.
pub(crate) fn download_playlist_with_progress_cancellable(
    service_root: &Path,
    request: DownloaderPlaylistRequest,
    cancellation: &dyn CancellationCheck,
    emit: &mut dyn FnMut(DownloaderPlaylistProgressEvent) -> Result<(), String>,
) -> Result<serde_json::Value, String> {
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
        let source_payload = track
            .source_payload
            .clone()
            .or_else(|| default_source_payload.clone())
            .unwrap_or_else(|| serde_json::json!({}));
        let payload = serde_json::json!({
            "songId": track.song_id,
            "level": default_level,
            "destination": destination,
            "managedCacheRoot": managed_cache_root.clone(),
            "sourcePayload": source_payload,
        });
        let track_result = call_downloader_download_track_package(service_root, payload);
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
