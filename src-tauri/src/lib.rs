use std::{fs, path::PathBuf};
use tauri::{
    ipc::Channel,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    utils::config::Color,
    AppHandle, Manager, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const TRAY_OPEN_ID: &str = "tray-open";
const TRAY_QUIT_ID: &str = "tray-quit";
const BG: Color = Color(0x18, 0x18, 0x18, 0xFF);

mod models;
mod services;
#[cfg(test)]
mod tests;
mod viewmodels;
mod window_state;

use services::repository as repository_service;
use services::repository::{
    ApiDesignSnapshot, AssetDetail, BinaryFileWriteRequest, BinaryFileWriteResponse, CacheSnapshot,
    DownloaderPlaylistProgressEvent, DownloaderPlaylistRequest, EntryPlaybackProgressEvent,
    EntryPlaybackRequest, EntryPlaybackSourceResponse, FileBrowserRequest, FileBrowserSnapshot,
    FileCopyRequest, FileCreateRequest, FileDeleteRequest, FileImportRequest, FileMoveRequest,
    FilePreviewSourceResponse, FileReadRequest, FileRenameRequest, HardlinkCandidateResponse,
    HardlinkConfirmRequest, HardlinkConfirmResponse, MetadataUpdateRequest, MetadataUpdateResponse,
    NeteaseRepositoryCacheConfigureRequest, NeteaseRepositoryCacheConfigureResponse,
    PlaylistDetail, PlaylistItemRemoveRequest, PlaylistItemsAddRequest,
    PlaylistItemsByPathsAddRequest, PlaylistItemsOrderRequest, PlaylistMembershipIndex,
    PlaylistMembershipRequest, PlaylistMembershipSnapshot, PlaylistMutationRequest,
    PlaylistMutationResponse, PlaylistSummary, PluginArchiveReadRequest, PluginArchiveTextResponse,
    PluginCallRequest, PluginCallResult, PluginConfigDeleteRequest, PluginConfigSetRequest,
    PluginConfigSnapshot, PluginDataDirectoryResponse, PluginDataFilePreviewSourceRequest,
    PluginDataFilePreviewSourceResponse, PluginEnabledRequest, PluginHookExecutionListRequest,
    PluginHookExecutionListResponse, PluginInstallRequest, PluginManifest, PluginMutationResponse,
    RepositoryAction, RepositoryActionEnabledRequest, RepositoryActionMutationResponse,
    RepositoryActionRunRequest, RepositoryActionRunResponse, RepositoryExportRequest,
    RepositoryExportResponse, RepositoryFolderRequest, RepositoryMutationRequest,
    RepositoryMutationResponse, RepositoryRelocateRequest, RepositorySnapshot, RepositorySummary,
    RevisionActionRequest, RevisionActionResponse, SearchRequest, SearchResponse,
    SmartFolderMutationRequest, SmartFolderMutationResponse, SmartFolderResultSnapshot,
    SmartFolderTreeNode, SmartFolderUpdateRequest, SyncRequest, SyncResult, ThumbnailRequest,
    ThumbnailResponse, TrashMutationRequest,
};
use services::runtime::{ExternalApiConnectionStatus, RepositoryRuntime};
use viewmodels::{
    FileBrowserViewModel, PluginViewModel, RepositoryInteractionViewModel,
    RepositoryManagementViewModel, RepositoryQueryViewModel,
};

#[tauri::command]
async fn ping() -> Result<String, String> {
    Ok("pong".to_string())
}

#[tauri::command]
async fn list_repositories(
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<Vec<RepositorySummary>, String> {
    repository_query.list_repositories().await
}

#[tauri::command]
async fn get_repository_snapshot(
    repo_id: String,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<RepositorySnapshot, String> {
    repository_query.get_repository_snapshot(repo_id).await
}

#[tauri::command]
async fn get_asset_detail(
    repo_id: String,
    asset_id: String,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<AssetDetail, String> {
    repository_query.get_asset_detail(repo_id, asset_id).await
}

#[tauri::command]
async fn search_assets(
    request: SearchRequest,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<SearchResponse, String> {
    repository_query.search_assets(request).await
}

#[tauri::command]
async fn update_asset_metadata(
    request: MetadataUpdateRequest,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<MetadataUpdateResponse, String> {
    repository_query.update_asset_metadata(request).await
}

#[tauri::command]
async fn get_file_browser(
    request: FileBrowserRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.get_file_browser(request).await
}

#[tauri::command]
async fn list_smart_folders(
    repo_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<Vec<SmartFolderTreeNode>, String> {
    repository_interaction.list_smart_folders(repo_id).await
}

#[tauri::command]
async fn list_playlists(
    repo_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<Vec<PlaylistSummary>, String> {
    repository_interaction.list_playlists(repo_id).await
}

#[tauri::command]
async fn list_playlist_memberships(
    repo_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistMembershipIndex, String> {
    repository_interaction
        .list_playlist_memberships(repo_id)
        .await
}

#[tauri::command]
async fn create_playlist(
    request: PlaylistMutationRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistMutationResponse, String> {
    repository_interaction.create_playlist(request).await
}

#[tauri::command]
async fn update_playlist(
    request: repository_service::PlaylistUpdateRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistMutationResponse, String> {
    repository_interaction.update_playlist(request).await
}

#[tauri::command]
async fn delete_playlist(
    repo_id: String,
    playlist_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistMutationResponse, String> {
    repository_interaction
        .delete_playlist(repo_id, playlist_id)
        .await
}

#[tauri::command]
async fn get_playlist_detail(
    repo_id: String,
    playlist_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistDetail, String> {
    repository_interaction
        .get_playlist_detail(repo_id, playlist_id)
        .await
}

#[tauri::command]
async fn add_playlist_items(
    request: PlaylistItemsAddRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistDetail, String> {
    repository_interaction.add_playlist_items(request).await
}

#[tauri::command]
async fn add_playlist_items_by_paths(
    request: PlaylistItemsByPathsAddRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistDetail, String> {
    repository_interaction
        .add_playlist_items_by_paths(request)
        .await
}

#[tauri::command]
async fn reorder_playlist_items(
    request: PlaylistItemsOrderRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistDetail, String> {
    repository_interaction.reorder_playlist_items(request).await
}

#[tauri::command]
async fn remove_playlist_item(
    request: PlaylistItemRemoveRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistDetail, String> {
    repository_interaction.remove_playlist_item(request).await
}

#[tauri::command]
async fn set_playlist_membership(
    request: PlaylistMembershipRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<PlaylistMembershipSnapshot, String> {
    repository_interaction
        .set_playlist_membership(request)
        .await
}

#[tauri::command]
async fn create_smart_folder(
    request: SmartFolderMutationRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<SmartFolderMutationResponse, String> {
    repository_interaction.create_smart_folder(request).await
}

#[tauri::command]
async fn update_smart_folder(
    request: SmartFolderUpdateRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<SmartFolderMutationResponse, String> {
    repository_interaction.update_smart_folder(request).await
}

#[tauri::command]
async fn delete_smart_folder(
    repo_id: String,
    smart_folder_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<SmartFolderMutationResponse, String> {
    repository_interaction
        .delete_smart_folder(repo_id, smart_folder_id)
        .await
}

#[tauri::command]
async fn query_smart_folder(
    repo_id: String,
    smart_folder_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<SmartFolderResultSnapshot, String> {
    repository_interaction
        .query_smart_folder(repo_id, smart_folder_id)
        .await
}

#[tauri::command]
async fn list_repository_actions(
    repo_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<Vec<RepositoryAction>, String> {
    repository_interaction
        .list_repository_actions(repo_id)
        .await
}

#[tauri::command]
async fn get_repository_action(
    repo_id: String,
    action_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<RepositoryAction, String> {
    repository_interaction
        .get_repository_action(repo_id, action_id)
        .await
}

#[tauri::command]
async fn set_repository_action_enabled(
    request: RepositoryActionEnabledRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<RepositoryActionMutationResponse, String> {
    repository_interaction
        .set_repository_action_enabled(request)
        .await
}

#[tauri::command]
async fn run_repository_action(
    request: RepositoryActionRunRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<RepositoryActionRunResponse, String> {
    repository_interaction.run_repository_action(request).await
}

#[tauri::command]
async fn read_file(
    request: FileReadRequest,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<Vec<u8>, String> {
    repository_query.read_file(request).await
}

#[tauri::command]
async fn prepare_preview_file_source(
    request: FileReadRequest,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<FilePreviewSourceResponse, String> {
    repository_query.prepare_preview_file_source(request).await
}

#[tauri::command]
async fn prepare_entry_playback_source(
    request: EntryPlaybackRequest,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<EntryPlaybackSourceResponse, String> {
    repository_query
        .prepare_entry_playback_source(request)
        .await
}

#[tauri::command]
async fn prepare_entry_playback_source_with_progress(
    request: EntryPlaybackRequest,
    progress: Channel<EntryPlaybackProgressEvent>,
    repository_query: tauri::State<'_, RepositoryQueryViewModel>,
) -> Result<EntryPlaybackSourceResponse, String> {
    repository_query
        .prepare_entry_playback_source_with_progress(request, progress)
        .await
}

#[tauri::command]
async fn call_plugin(
    request: PluginCallRequest,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginCallResult, String> {
    plugin_vm.call_plugin(request).await
}

#[tauri::command]
async fn download_playlist_with_progress(
    request: DownloaderPlaylistRequest,
    progress: Channel<DownloaderPlaylistProgressEvent>,
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<serde_json::Value, String> {
    let service_root = runtime.service_root();
    let mut emit = |event: DownloaderPlaylistProgressEvent| {
        progress.send(event).map_err(|error| error.to_string())
    };
    execute_playlist_download_with_progress(&service_root, request, &mut emit)
}

fn execute_playlist_download_with_progress(
    service_root: &std::path::Path,
    request: DownloaderPlaylistRequest,
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
        match repository_service::call_downloader_download_track_package(service_root, payload) {
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

#[tauri::command]
async fn read_plugin_archive_text(
    request: PluginArchiveReadRequest,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginArchiveTextResponse, String> {
    plugin_vm.read_plugin_archive_text(request).await
}

#[tauri::command]
async fn get_plugin_data_directory(
    plugin_id: String,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginDataDirectoryResponse, String> {
    plugin_vm.get_plugin_data_directory(plugin_id).await
}

#[tauri::command]
async fn prepare_plugin_data_file_preview_source(
    request: PluginDataFilePreviewSourceRequest,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginDataFilePreviewSourceResponse, String> {
    plugin_vm
        .prepare_plugin_data_file_preview_source(request)
        .await
}

#[tauri::command]
async fn get_plugin_config(
    plugin_id: String,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginConfigSnapshot, String> {
    plugin_vm.get_plugin_config(plugin_id).await
}

#[tauri::command]
async fn set_plugin_config_value(
    request: PluginConfigSetRequest,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginConfigSnapshot, String> {
    plugin_vm.set_plugin_config_value(request).await
}

#[tauri::command]
async fn delete_plugin_config_value(
    request: PluginConfigDeleteRequest,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginConfigSnapshot, String> {
    plugin_vm.delete_plugin_config_value(request).await
}

#[tauri::command]
async fn write_binary_file(
    request: BinaryFileWriteRequest,
) -> Result<BinaryFileWriteResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let output_path = PathBuf::from(&request.path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&output_path, &request.bytes).map_err(|error| error.to_string())?;
        Ok(BinaryFileWriteResponse {
            path: request.path,
            size_bytes: i64::try_from(request.bytes.len())
                .map_err(|_| "written file is too large".to_string())?,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn create_directory(
    request: FileCreateRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.create_directory(request).await
}

#[tauri::command]
async fn create_file(
    request: FileCreateRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.create_file(request).await
}

#[tauri::command]
async fn import_entries(
    request: FileImportRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.import_entries(request).await
}

#[tauri::command]
async fn copy_entries(
    request: FileCopyRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.copy_entries(request).await
}

#[tauri::command]
async fn move_entries(
    request: FileMoveRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.move_entries(request).await
}

#[tauri::command]
async fn rename_entry(
    request: FileRenameRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.rename_entry(request).await
}

#[tauri::command]
async fn delete_entry(
    request: FileDeleteRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.delete_entry(request).await
}

#[tauri::command]
async fn mutate_trash(
    request: TrashMutationRequest,
    file_browser: tauri::State<'_, FileBrowserViewModel>,
) -> Result<FileBrowserSnapshot, String> {
    file_browser.mutate_trash(request).await
}

#[tauri::command]
async fn create_repository(
    request: RepositoryMutationRequest,
    app: AppHandle,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<RepositoryMutationResponse, String> {
    let response = repository_management.create_repository(request).await?;
    repository_management.refresh_thumbnail_scope(&app).await?;
    Ok(response)
}

#[tauri::command]
async fn import_repository(
    request: RepositoryMutationRequest,
    app: AppHandle,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<RepositoryMutationResponse, String> {
    let response = repository_management.import_repository(request).await?;
    repository_management.refresh_thumbnail_scope(&app).await?;
    Ok(response)
}

#[tauri::command]
async fn attach_repository_folder(
    request: RepositoryFolderRequest,
    app: AppHandle,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<RepositoryMutationResponse, String> {
    let response = repository_management
        .attach_repository_folder(request)
        .await?;
    repository_management.refresh_thumbnail_scope(&app).await?;
    Ok(response)
}

#[tauri::command]
async fn delete_repository(
    repo_id: String,
    app: AppHandle,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<(), String> {
    repository_management.delete_repository(repo_id).await?;
    repository_management.refresh_thumbnail_scope(&app).await
}

#[tauri::command]
async fn relocate_repository(
    request: RepositoryRelocateRequest,
    app: AppHandle,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<RepositoryMutationResponse, String> {
    let response = repository_management.relocate_repository(request).await?;
    repository_management.refresh_thumbnail_scope(&app).await?;
    Ok(response)
}

#[tauri::command]
async fn update_repository_backend_config(
    request: repository_service::RepositoryBackendConfigUpdateRequest,
    app: AppHandle,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<RepositoryMutationResponse, String> {
    let response = repository_management
        .update_repository_backend_config(request)
        .await?;
    repository_management.refresh_thumbnail_scope(&app).await?;
    Ok(response)
}

#[tauri::command]
async fn configure_netease_repository_cache(
    request: NeteaseRepositoryCacheConfigureRequest,
    app: AppHandle,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<NeteaseRepositoryCacheConfigureResponse, String> {
    let response = repository_management
        .configure_netease_repository_cache(request)
        .await?;
    repository_management.refresh_thumbnail_scope(&app).await?;
    Ok(response)
}

#[tauri::command]
async fn export_repository(
    request: RepositoryExportRequest,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<RepositoryExportResponse, String> {
    repository_management.export_repository(request).await
}

#[tauri::command]
async fn sync_repository(
    request: SyncRequest,
    repository_management: tauri::State<'_, RepositoryManagementViewModel>,
) -> Result<SyncResult, String> {
    repository_management.sync_repository(request).await
}

#[tauri::command]
async fn list_hardlink_candidates(
    repo_id: String,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<HardlinkCandidateResponse, String> {
    repository_interaction
        .list_hardlink_candidates(repo_id)
        .await
}

#[tauri::command]
async fn confirm_hardlink_candidate(
    request: HardlinkConfirmRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<HardlinkConfirmResponse, String> {
    repository_interaction
        .confirm_hardlink_candidate(request)
        .await
}

#[tauri::command]
async fn ensure_thumbnail(
    request: ThumbnailRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<ThumbnailResponse, String> {
    repository_interaction.ensure_thumbnail(request).await
}

#[tauri::command]
async fn undo_last_revision(
    request: RevisionActionRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<RevisionActionResponse, String> {
    repository_interaction.undo_last_revision(request).await
}

#[tauri::command]
async fn redo_last_revision(
    request: RevisionActionRequest,
    repository_interaction: tauri::State<'_, RepositoryInteractionViewModel>,
) -> Result<RevisionActionResponse, String> {
    repository_interaction.redo_last_revision(request).await
}

#[tauri::command]
async fn list_plugins(
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<Vec<PluginManifest>, String> {
    plugin_vm.list_plugins().await
}

#[tauri::command]
async fn list_plugin_hook_executions(
    request: Option<PluginHookExecutionListRequest>,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginHookExecutionListResponse, String> {
    plugin_vm.list_plugin_hook_executions(request).await
}

#[tauri::command]
async fn set_plugin_enabled(
    request: PluginEnabledRequest,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginMutationResponse, String> {
    plugin_vm.set_plugin_enabled(request).await
}

#[tauri::command]
async fn delete_plugin(
    plugin_id: String,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginMutationResponse, String> {
    plugin_vm.delete_plugin(plugin_id).await
}

#[tauri::command]
async fn install_plugin_from_archive(
    request: PluginInstallRequest,
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<PluginMutationResponse, String> {
    plugin_vm.install_plugin_from_archive(request).await
}

#[tauri::command]
async fn get_cache_snapshot(
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<CacheSnapshot, String> {
    plugin_vm.get_cache_snapshot().await
}

#[tauri::command]
async fn get_api_design_snapshot(
    plugin_vm: tauri::State<'_, PluginViewModel>,
) -> Result<ApiDesignSnapshot, String> {
    plugin_vm.get_api_design_snapshot().await
}

#[tauri::command]
async fn get_external_api_connection_status(
    runtime: tauri::State<'_, RepositoryRuntime>,
) -> Result<ExternalApiConnectionStatus, String> {
    Ok(runtime.external_api_connection_status())
}

fn allow_thumbnail_asset_roots(app: &AppHandle, paths: Vec<PathBuf>) -> Result<(), String> {
    for path in paths {
        app.asset_protocol_scope()
            .allow_directory(path, true)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn persist_main_window_state(app: &AppHandle) {
    let cache = app.state::<window_state::MainWindowStateCache>();
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        window_state::persist_main_window_state(app, &cache, &window);
    } else {
        window_state::persist_cached_main_window_state(app, &cache);
    }
}

fn quit_app(app: &AppHandle) {
    persist_main_window_state(app);
    app.exit(0);
}

fn setup_tray(app: &AppHandle) -> Result<(), tauri::Error> {
    let open = MenuItem::with_id(app, TRAY_OPEN_ID, "打开 MomoBako", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let icon = app.default_window_icon().cloned();
    let tray = TrayIconBuilder::with_id("main-tray")
        .menu(&menu)
        .tooltip("MomoBako")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_OPEN_ID => show_main_window(app),
            TRAY_QUIT_ID => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = icon {
        tray.icon(icon).build(app)?;
    } else {
        tray.build(app)?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .manage(window_state::MainWindowStateCache::default())
        .setup(|app| {
            let runtime = RepositoryRuntime::start()?;
            let file_browser = FileBrowserViewModel::new(runtime.clone());
            let plugin_vm = PluginViewModel::new(runtime.clone());
            let repository_interaction = RepositoryInteractionViewModel::new(runtime.clone());
            let repository_query = RepositoryQueryViewModel::new(runtime.clone());
            let repository_management = RepositoryManagementViewModel::new(runtime.clone());
            allow_thumbnail_asset_roots(
                app.handle(),
                tauri::async_runtime::block_on(runtime.repository_thumbnail_roots())?,
            )?;
            app.manage(runtime);
            app.manage(file_browser);
            app.manage(plugin_vm);
            app.manage(repository_interaction);
            app.manage(repository_query);
            app.manage(repository_management);
            setup_tray(app.handle())?;

            if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                let _ = window.set_background_color(Some(BG));
                if let Some(state) = window_state::load_main_window_state(app.handle()) {
                    window_state::restore_main_window_state(&window, state);
                }
                let _ = window.show();
                let cache = app.state::<window_state::MainWindowStateCache>();
                window_state::remember_main_window_state(&cache, &window);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != MAIN_WINDOW_LABEL {
                return;
            }
            let app_handle = window.app_handle();
            let cache = app_handle.state::<window_state::MainWindowStateCache>();
            match event {
                WindowEvent::Moved(_)
                | WindowEvent::Resized(_)
                | WindowEvent::ScaleFactorChanged { .. } => {
                    if let Some(webview_window) = window.get_webview_window(MAIN_WINDOW_LABEL) {
                        window_state::remember_main_window_state(&cache, &webview_window);
                    }
                }
                WindowEvent::CloseRequested { .. } => {
                    quit_app(&app_handle);
                }
                WindowEvent::Destroyed => {
                    persist_main_window_state(&app_handle);
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            list_repositories,
            get_repository_snapshot,
            get_asset_detail,
            search_assets,
            update_asset_metadata,
            get_file_browser,
            list_playlists,
            list_playlist_memberships,
            create_playlist,
            update_playlist,
            delete_playlist,
            get_playlist_detail,
            add_playlist_items,
            add_playlist_items_by_paths,
            reorder_playlist_items,
            remove_playlist_item,
            set_playlist_membership,
            list_smart_folders,
            create_smart_folder,
            update_smart_folder,
            delete_smart_folder,
            query_smart_folder,
            list_repository_actions,
            get_repository_action,
            set_repository_action_enabled,
            run_repository_action,
            read_file,
            prepare_preview_file_source,
            prepare_entry_playback_source,
            prepare_entry_playback_source_with_progress,
            call_plugin,
            download_playlist_with_progress,
            read_plugin_archive_text,
            get_plugin_data_directory,
            prepare_plugin_data_file_preview_source,
            get_plugin_config,
            set_plugin_config_value,
            delete_plugin_config_value,
            write_binary_file,
            create_directory,
            create_file,
            import_entries,
            copy_entries,
            move_entries,
            rename_entry,
            delete_entry,
            mutate_trash,
            create_repository,
            import_repository,
            attach_repository_folder,
            delete_repository,
            relocate_repository,
            update_repository_backend_config,
            configure_netease_repository_cache,
            export_repository,
            sync_repository,
            list_hardlink_candidates,
            confirm_hardlink_candidate,
            ensure_thumbnail,
            undo_last_revision,
            redo_last_revision,
            list_plugins,
            list_plugin_hook_executions,
            set_plugin_enabled,
            delete_plugin,
            install_plugin_from_archive,
            get_cache_snapshot,
            get_api_design_snapshot,
            get_external_api_connection_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
