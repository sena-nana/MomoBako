use tauri::{ipc::Channel, AppHandle};

mod app_shell;
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
use services::runtime::ExternalApiConnectionStatus;
use viewmodels::{
    FileBrowserViewModel, PluginViewModel, RepositoryInteractionViewModel,
    RepositoryManagementViewModel, RepositoryPlaybackViewModel, RepositoryQueryViewModel,
    SystemViewModel,
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
    repository_playback: tauri::State<'_, RepositoryPlaybackViewModel>,
) -> Result<EntryPlaybackSourceResponse, String> {
    repository_playback
        .prepare_entry_playback_source(request)
        .await
}

#[tauri::command]
async fn prepare_entry_playback_source_with_progress(
    request: EntryPlaybackRequest,
    progress: Channel<EntryPlaybackProgressEvent>,
    repository_playback: tauri::State<'_, RepositoryPlaybackViewModel>,
) -> Result<EntryPlaybackSourceResponse, String> {
    repository_playback
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
    repository_playback: tauri::State<'_, RepositoryPlaybackViewModel>,
) -> Result<serde_json::Value, String> {
    repository_playback
        .download_playlist_with_progress(request, progress)
        .await
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
    system_vm: tauri::State<'_, SystemViewModel>,
) -> Result<BinaryFileWriteResponse, String> {
    system_vm.write_binary_file(request).await
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
    system_vm: tauri::State<'_, SystemViewModel>,
) -> Result<ExternalApiConnectionStatus, String> {
    system_vm.get_external_api_connection_status().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    app_shell::builder()
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
