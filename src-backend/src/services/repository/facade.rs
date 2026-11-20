//! RepositoryState service facade methods grouped away from the module map.

use super::*;

mod files;
mod registry;
mod thumbnails;

impl RepositoryState {
    pub fn create_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::create_repository(self, request)
    }

    pub fn import_repository(
        &self,
        request: RepositoryMutationRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::import_repository(self, request)
    }

    pub fn attach_repository_folder(
        &self,
        request: RepositoryFolderRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::attach_repository_folder(self, request)
    }

    pub fn delete_repository(&self, request: RepositoryDeleteRequest) -> Result<(), String> {
        management::delete_repository(self, request)
    }

    pub fn relocate_repository(
        &self,
        request: RepositoryRelocateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::relocate_repository(self, request)
    }

    pub fn update_repository_backend_config(
        &self,
        request: RepositoryBackendConfigUpdateRequest,
    ) -> Result<RepositoryMutationResponse, String> {
        management::update_repository_backend_config(self, request)
    }

    pub fn configure_netease_repository_cache(
        &self,
        request: NeteaseRepositoryCacheConfigureRequest,
    ) -> Result<NeteaseRepositoryCacheConfigureResponse, String> {
        management::configure_netease_repository_cache(self, request)
    }

    pub fn export_repository(
        &self,
        request: RepositoryExportRequest,
    ) -> Result<RepositoryExportResponse, String> {
        management::export_repository(self, request)
    }

    pub fn load_snapshot(&self, repo_id: &str) -> Result<RepositorySnapshot, String> {
        query::load_snapshot(self, repo_id)
    }

    pub fn load_asset_detail(&self, repo_id: &str, asset_id: &str) -> Result<AssetDetail, String> {
        query::load_asset_detail(self, repo_id, asset_id)
    }

    pub fn list_playlists(&self, repo_id: &str) -> Result<Vec<PlaylistSummary>, String> {
        playlist::list_playlists(self, repo_id)
    }

    pub fn list_playlist_memberships(
        &self,
        repo_id: &str,
    ) -> Result<PlaylistMembershipIndex, String> {
        playlist::list_playlist_memberships(self, repo_id)
    }

    pub fn create_playlist(
        &self,
        request: PlaylistMutationRequest,
    ) -> Result<PlaylistMutationResponse, String> {
        playlist::create_playlist(self, request)
    }

    pub fn delete_playlist(
        &self,
        repo_id: &str,
        playlist_id: &str,
    ) -> Result<PlaylistMutationResponse, String> {
        playlist::delete_playlist(self, repo_id, playlist_id)
    }

    pub fn get_playlist_detail(
        &self,
        repo_id: &str,
        playlist_id: &str,
    ) -> Result<PlaylistDetail, String> {
        playlist::get_playlist_detail(self, repo_id, playlist_id)
    }

    pub fn add_playlist_items_by_paths(
        &self,
        request: PlaylistItemsByPathsAddRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::add_playlist_items_by_paths(self, request)
    }

    pub fn reorder_playlist_items(
        &self,
        request: PlaylistItemsOrderRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::reorder_playlist_items(self, request)
    }

    pub fn remove_playlist_item(
        &self,
        request: PlaylistItemRemoveRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::remove_playlist_item(self, request)
    }

    pub fn set_playlist_membership(
        &self,
        request: PlaylistMembershipRequest,
    ) -> Result<PlaylistMembershipSnapshot, String> {
        playlist::set_playlist_membership(self, request)
    }

    pub fn load_file_browser(
        &self,
        request: FileBrowserRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::load_file_browser(self, request)
    }

    pub fn load_repository_tree(&self, repo_id: &str) -> Result<RepositoryTreeSnapshot, String> {
        browser::load_repository_tree(self, repo_id)
    }

    pub fn read_file(&self, request: FileReadRequest) -> Result<Vec<u8>, String> {
        query::read_file(self, request)
    }

    pub fn record_entry_access(
        &self,
        request: EntryAccessRecordRequest,
    ) -> Result<EntryAccessRecordResponse, String> {
        query::record_entry_access(self, request)
    }

    pub fn clear_recent_access_history(
        &self,
        request: RecentAccessHistoryClearRequest,
    ) -> Result<RecentAccessHistoryClearResponse, String> {
        query::clear_recent_access_history(self, request)
    }

    pub fn prepare_preview_file_source(
        &self,
        request: FileReadRequest,
    ) -> Result<FilePreviewSourceResponse, String> {
        query::prepare_preview_file_source(self, request)
    }

    pub fn prepare_entry_playback_source_with_progress(
        &self,
        request: EntryPlaybackRequest,
        emit: &mut dyn FnMut(EntryPlaybackProgressEvent) -> Result<(), String>,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        query::prepare_entry_playback_source_with_progress(self, request, emit)
    }

    pub fn call_plugin(&self, request: PluginCallRequest) -> Result<PluginCallResult, String> {
        plugin::call_plugin(self, request)
    }

    pub fn get_plugin_data_directory(
        &self,
        plugin_id: String,
    ) -> Result<PluginDataDirectoryResponse, String> {
        plugin::get_plugin_data_directory(self, plugin_id)
    }

    pub fn get_plugin_config(&self, plugin_id: String) -> Result<PluginConfigSnapshot, String> {
        plugin::get_plugin_config(self, plugin_id)
    }

    pub fn set_plugin_config_value(
        &self,
        request: PluginConfigSetRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        plugin::set_plugin_config_value(self, request)
    }

    pub fn delete_plugin_config_value(
        &self,
        request: PluginConfigDeleteRequest,
    ) -> Result<PluginConfigSnapshot, String> {
        plugin::delete_plugin_config_value(self, request)
    }

    pub(super) fn load_plugin_config_values(
        &self,
        plugin_id: &str,
    ) -> Result<(PluginManifest, PathBuf, BTreeMap<String, serde_json::Value>), String> {
        let registry = plugin_catalog(&self.root);
        let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
        let registration = registry
            .registration(&normalized_plugin_id)
            .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
        let manifest = registration.manifest.clone();
        let data_dir = ensure_plugin_data_dir(&self.root, &manifest.plugin_id)?;
        let values = load_plugin_config_values(&data_dir)?;
        Ok((manifest, data_dir, values))
    }

    pub fn list_smart_folders(&self, repo_id: &str) -> Result<Vec<SmartFolderTreeNode>, String> {
        smart_folder::list_smart_folders(self, repo_id)
    }

    pub fn create_smart_folder(
        &self,
        request: SmartFolderMutationRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        smart_folder::create_smart_folder(self, request)
    }

    pub fn update_smart_folder(
        &self,
        request: SmartFolderUpdateRequest,
    ) -> Result<SmartFolderMutationResponse, String> {
        smart_folder::update_smart_folder(self, request)
    }

    pub fn delete_smart_folder(
        &self,
        repo_id: &str,
        smart_folder_id: &str,
    ) -> Result<SmartFolderMutationResponse, String> {
        smart_folder::delete_smart_folder(self, repo_id, smart_folder_id)
    }

    pub fn query_smart_folder(
        &self,
        repo_id: &str,
        smart_folder_id: &str,
    ) -> Result<SmartFolderResultSnapshot, String> {
        smart_folder::query_smart_folder(self, repo_id, smart_folder_id)
    }

    pub fn list_repository_actions(&self, repo_id: &str) -> Result<Vec<RepositoryAction>, String> {
        action::list_repository_actions(self, repo_id)
    }

    pub fn get_repository_action(
        &self,
        repo_id: &str,
        action_id: &str,
    ) -> Result<RepositoryAction, String> {
        action::get_repository_action(self, repo_id, action_id)
    }

    pub fn set_repository_action_enabled(
        &self,
        request: RepositoryActionEnabledRequest,
    ) -> Result<RepositoryActionMutationResponse, String> {
        action::set_repository_action_enabled(self, request)
    }

    pub fn run_repository_action(
        &self,
        request: RepositoryActionRunRequest,
    ) -> Result<RepositoryActionRunResponse, String> {
        action::run_repository_action(self, request)
    }

    pub fn search_assets(&self, request: SearchRequest) -> Result<SearchResponse, String> {
        query::search_assets(self, request)
    }

    pub fn list_hardlink_candidates(
        &self,
        repo_id: &str,
    ) -> Result<HardlinkCandidateResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let candidates = load_hardlink_candidates(&connection, repo_id).map_err(db_error)?;
        Ok(HardlinkCandidateResponse {
            repo_id: repo_id.to_string(),
            candidates,
        })
    }

    pub fn confirm_hardlink_candidate(
        &self,
        request: HardlinkConfirmRequest,
    ) -> Result<HardlinkConfirmResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        if !repository_supports_local_write_access(&repo) {
            return Err(
                "hardlink confirmation is only supported for repositories with local write access"
                    .to_string(),
            );
        }
        let repo_root = PathBuf::from(&repo.summary.path);
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;
        let candidate =
            load_hardlink_candidate_from_transaction(&tx, &request.repo_id, &request.candidate_id)
                .map_err(db_error)?
                .ok_or_else(|| format!("hardlink candidate not found: {}", request.candidate_id))?;

        let existing_abs = resolve_repository_relative_path(&repo_root, &candidate.existing_path)?;
        let new_abs = resolve_repository_relative_path(&repo_root, &candidate.new_path)?;
        let existing_file_current = current_file_matches_content(
            &existing_abs,
            &candidate.content_hash,
            candidate.size_bytes,
        )?;
        let new_file_current =
            current_file_matches_content(&new_abs, &candidate.content_hash, candidate.size_bytes)?;
        if !existing_file_current || !new_file_current {
            delete_hardlink_candidate(&tx, &request.repo_id, &request.candidate_id)
                .map_err(db_error)?;
            tx.commit().map_err(db_error)?;
            return Err("hardlink candidate is no longer valid".to_string());
        }

        replace_file_with_hardlink(&repo_root, &existing_abs, &new_abs)?;
        upsert_hardlink_member(
            &tx,
            &request.repo_id,
            &candidate.existing_asset_id,
            &candidate.existing_path,
            &candidate.content_hash,
            candidate.size_bytes,
            "linked",
        )
        .map_err(db_error)?;
        upsert_hardlink_member(
            &tx,
            &request.repo_id,
            &candidate.new_asset_id,
            &candidate.new_path,
            &candidate.content_hash,
            candidate.size_bytes,
            "linked",
        )
        .map_err(db_error)?;
        delete_hardlink_candidate(&tx, &request.repo_id, &request.candidate_id)
            .map_err(db_error)?;
        tx.commit().map_err(db_error)?;

        Ok(HardlinkConfirmResponse {
            repo_id: request.repo_id,
            candidate,
            state: "linked".to_string(),
        })
    }

    pub fn update_asset_metadata(
        &self,
        request: MetadataUpdateRequest,
    ) -> Result<MetadataUpdateResponse, String> {
        query::update_asset_metadata(self, request)
    }

    pub fn sync_repository(&self, request: SyncRequest) -> Result<SyncResult, String> {
        management::sync_repository_cancellable(self, request, &NeverCancelled)
    }

    pub(crate) fn sync_repository_cancellable(
        &self,
        request: SyncRequest,
        cancellation: &dyn CancellationCheck,
    ) -> Result<SyncResult, String> {
        management::sync_repository_cancellable(self, request, cancellation)
    }

    pub(crate) fn sync_repository_changed_paths(
        &self,
        repo_id: &str,
        changed_paths: &std::collections::BTreeSet<String>,
    ) -> Result<SyncResult, String> {
        management::sync_repository_changed_paths(self, repo_id, changed_paths)
    }

    pub(super) fn sync_repository_with_candidate_skips(
        &self,
        repo_id: &str,
        skip_hardlink_candidate_paths: &HashSet<String>,
    ) -> Result<SyncResult, String> {
        management::sync_repository_with_candidate_skips(
            self,
            repo_id,
            skip_hardlink_candidate_paths,
        )
    }
}
