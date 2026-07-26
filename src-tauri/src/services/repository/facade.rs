//! RepositoryState service facade methods grouped away from the module map.

use super::*;

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

    pub fn update_playlist(
        &self,
        request: PlaylistUpdateRequest,
    ) -> Result<PlaylistMutationResponse, String> {
        playlist::update_playlist(self, request)
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

    pub fn add_playlist_items(
        &self,
        request: PlaylistItemsAddRequest,
    ) -> Result<PlaylistDetail, String> {
        playlist::add_playlist_items(self, request)
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

    pub fn prepare_entry_playback_source(
        &self,
        request: EntryPlaybackRequest,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        query::prepare_entry_playback_source(self, request)
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

    pub fn read_plugin_archive_text(
        &self,
        request: PluginArchiveReadRequest,
    ) -> Result<PluginArchiveTextResponse, String> {
        plugin::read_plugin_archive_text(self, request)
    }

    pub fn get_plugin_data_directory(
        &self,
        plugin_id: String,
    ) -> Result<PluginDataDirectoryResponse, String> {
        plugin::get_plugin_data_directory(self, plugin_id)
    }

    pub fn prepare_plugin_data_file_preview_source(
        &self,
        request: PluginDataFilePreviewSourceRequest,
    ) -> Result<PluginDataFilePreviewSourceResponse, String> {
        plugin::prepare_plugin_data_file_preview_source(self, request)
    }

    pub fn prepare_repository_cache_file_preview_source(
        &self,
        request: RepositoryCacheFilePreviewSourceRequest,
    ) -> Result<RepositoryCacheFilePreviewSourceResponse, String> {
        plugin::prepare_repository_cache_file_preview_source(self, request)
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
        management::sync_repository(self, request)
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

    pub fn ensure_thumbnail(&self, request: ThumbnailRequest) -> Result<ThumbnailResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let entry_path = normalize_entry_path(&request.path)?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let entry = stat_backend_entry(&self.root, &repo, &repo_root, &entry_path)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let thumbnail_root = self.repository_thumbnail_root(&repo)?;
        let action = request.action.as_deref().unwrap_or("ensure");
        let kind = match entry.kind {
            FileSystemEntryKind::Directory => "directory",
            FileSystemEntryKind::File => "file",
        };

        if kind == "directory" {
            let response = match action {
                "ensure" => {
                    let record = load_entry_thumbnail_record(
                        &connection,
                        &request.repo_id,
                        &entry_path,
                        kind,
                    )
                    .map_err(db_error)?;
                    normalize_entry_thumbnail_record(
                        &connection,
                        &repo,
                        &thumbnail_root,
                        &entry_path,
                        kind,
                        record,
                    )?
                    .map(|record| (Some(record.path), record.custom))
                    .unwrap_or((None, false))
                }
                "save" => {
                    let bytes = thumbnail_bytes_from_request(&request)?;
                    let thumbnail_path = save_custom_thumbnail_bytes(
                        &thumbnail_root,
                        &repo,
                        &entry_path,
                        kind,
                        &bytes,
                    )?;
                    upsert_entry_thumbnail_record(
                        &connection,
                        &request.repo_id,
                        &entry_path,
                        kind,
                        &thumbnail_path,
                        true,
                    )
                    .map_err(db_error)?;
                    (Some(thumbnail_path), true)
                }
                "saveGenerated" => {
                    let bytes = thumbnail_bytes_from_request(&request)?;
                    let thumbnail_path = save_thumbnail_bytes(
                        &thumbnail_root,
                        &repo,
                        &entry_path,
                        kind,
                        "generated",
                        &bytes,
                    )?;
                    upsert_entry_thumbnail_record(
                        &connection,
                        &request.repo_id,
                        &entry_path,
                        kind,
                        &thumbnail_path,
                        false,
                    )
                    .map_err(db_error)?;
                    (Some(thumbnail_path), false)
                }
                "clear" => {
                    remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                        .map_err(db_error)?;
                    (None, false)
                }
                "refresh" => {
                    remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                        .map_err(db_error)?;
                    (None, false)
                }
                value => return Err(format!("unsupported thumbnail action: {value}")),
            };

            return Ok(ThumbnailResponse {
                repo_id: request.repo_id,
                path: entry_path,
                asset_id: String::new(),
                kind: kind.to_string(),
                thumbnail_path: response.0,
                thumbnail_custom: response.1,
                metadata: None,
            });
        }

        let asset = connection
            .query_row(
                r#"
                SELECT asset_id, filename, extension, size_bytes, modified_at, thumbnail_path
                FROM assets
                WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'
                "#,
                params![&request.repo_id, &entry_path],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, Option<String>>(5)?,
                    ))
                },
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("asset not found: {}", request.path))?;

        let (asset_id, filename, extension, size_bytes, modified_at, existing_thumbnail_path) =
            asset;
        let existing_thumbnail_path = normalize_asset_thumbnail_path(
            &connection,
            &repo,
            &thumbnail_root,
            &asset_id,
            &entry_path,
            existing_thumbnail_path,
        )?;
        let file = DiscoveredFile {
            absolute_path: Some(resolve_repository_relative_path(&repo_root, &entry_path)?),
            relative_path: entry_path.clone(),
            filename,
            extension,
            size_bytes,
            created_at: None,
            modified_at,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
            status: None,
            shared_asset_id: None,
            tags: None,
            thumbnail_local_absolute_path: None,
        };
        let existing_record =
            load_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                .map_err(db_error)?;
        let custom_record = normalize_entry_thumbnail_record(
            &connection,
            &repo,
            &thumbnail_root,
            &entry_path,
            kind,
            existing_record,
        )?
        .filter(|record| record.custom);
        let (thumbnail_path, thumbnail_custom) = match action {
            "ensure" => {
                if let Some(record) = custom_record {
                    (Some(record.path), true)
                } else {
                    (
                        ensure_thumbnail_for_file(
                            &repo,
                            &repo_root,
                            &thumbnail_root,
                            &file,
                            existing_thumbnail_path,
                            false,
                        )?,
                        false,
                    )
                }
            }
            "refresh" => (
                ensure_thumbnail_for_file(
                    &repo,
                    &repo_root,
                    &thumbnail_root,
                    &file,
                    existing_thumbnail_path,
                    true,
                )?,
                false,
            ),
            "save" => {
                let bytes = thumbnail_bytes_from_request(&request)?;
                let thumbnail_path =
                    save_custom_thumbnail_bytes(&thumbnail_root, &repo, &entry_path, kind, &bytes)?;
                upsert_entry_thumbnail_record(
                    &connection,
                    &request.repo_id,
                    &entry_path,
                    kind,
                    &thumbnail_path,
                    true,
                )
                .map_err(db_error)?;
                (Some(thumbnail_path), true)
            }
            "saveGenerated" => {
                let bytes = thumbnail_bytes_from_request(&request)?;
                let thumbnail_path = save_thumbnail_bytes(
                    &thumbnail_root,
                    &repo,
                    &entry_path,
                    kind,
                    "generated",
                    &bytes,
                )?;
                remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                    .map_err(db_error)?;
                (Some(thumbnail_path), false)
            }
            "clear" => {
                remove_entry_thumbnail_record(&connection, &request.repo_id, &entry_path, kind)
                    .map_err(db_error)?;
                (existing_thumbnail_path, false)
            }
            value => return Err(format!("unsupported thumbnail action: {value}")),
        };

        if thumbnail_custom {
            update_asset_thumbnail_path(&connection, &request.repo_id, &asset_id, None)
                .map_err(db_error)?;
        } else {
            update_asset_thumbnail_path(
                &connection,
                &request.repo_id,
                &asset_id,
                thumbnail_path.as_deref(),
            )
            .map_err(db_error)?;
        }
        sync_thumbnail_palette_metadata(&connection, &asset_id, thumbnail_path.as_deref())
            .map_err(db_error)?;
        let metadata = load_metadata_map(&connection, &asset_id).map_err(db_error)?;

        Ok(ThumbnailResponse {
            repo_id: request.repo_id,
            path: entry_path,
            asset_id,
            kind: kind.to_string(),
            thumbnail_path,
            thumbnail_custom,
            metadata: Some(metadata),
        })
    }

    pub fn create_directory(
        &self,
        request: FileCreateRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::create_directory(self, request)
    }

    pub fn create_file(&self, request: FileCreateRequest) -> Result<FileBrowserSnapshot, String> {
        browser::create_file(self, request)
    }

    pub fn import_entries(
        &self,
        request: FileImportRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::import_entries(self, request)
    }

    pub fn import_archive_entries(
        &self,
        request: FileArchiveImportRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        importing::import_archive_entries(self, request)
    }

    pub fn import_eagle_library(
        &self,
        request: EagleLibraryImportRequest,
    ) -> Result<EagleLibraryImportResponse, String> {
        importing::import_eagle_library(self, request)
    }

    pub fn add_external_assets(
        &self,
        request_id: String,
        request: ExternalAddAssetRequest,
    ) -> ExternalAddAssetResponse {
        external_assets::add_external_assets(self, request_id, request)
    }

    pub fn copy_entries(&self, request: FileCopyRequest) -> Result<FileBrowserSnapshot, String> {
        browser::copy_entries(self, request)
    }

    pub fn move_entries(&self, request: FileMoveRequest) -> Result<FileBrowserSnapshot, String> {
        browser::move_entries(self, request)
    }

    pub(super) fn finish_file_copy_operation(
        &self,
        repo_id: &str,
        parent_path: String,
        include_tree: bool,
        outcomes: Vec<HardlinkCopyOutcome>,
    ) -> Result<FileBrowserSnapshot, String> {
        let skip_candidate_paths = hardlink_outcome_target_paths(&outcomes);
        self.sync_repository_with_candidate_skips(repo_id, &skip_candidate_paths)?;
        self.record_hardlink_copy_outcomes(repo_id, outcomes)?;

        self.load_file_browser(FileBrowserRequest {
            repo_id: repo_id.to_string(),
            directory_path: Some(parent_path),
            include_tree: Some(include_tree),
            special_location: None,
            offset: None,
            limit: None,
        })
    }

    pub(super) fn record_hardlink_copy_outcomes(
        &self,
        repo_id: &str,
        outcomes: Vec<HardlinkCopyOutcome>,
    ) -> Result<(), String> {
        if outcomes.is_empty() {
            return Ok(());
        }
        let repo = self.load_repository_record(repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;
        for outcome in outcomes {
            let Some(target_asset) =
                load_hardlink_asset_for_path(&tx, repo_id, &outcome.target_path)
                    .map_err(db_error)?
            else {
                continue;
            };
            upsert_hardlink_member(
                &tx,
                repo_id,
                &target_asset.asset_id,
                &outcome.target_path,
                &target_asset.content_hash,
                target_asset.size_bytes,
                &outcome.link_state,
            )
            .map_err(db_error)?;

            if outcome.link_state == "linked" {
                let Some(source_path) = outcome.source_path.as_deref() else {
                    continue;
                };
                let Some(source_asset) =
                    load_hardlink_asset_for_path(&tx, repo_id, source_path).map_err(db_error)?
                else {
                    continue;
                };
                if source_asset.content_hash == target_asset.content_hash
                    && source_asset.size_bytes == target_asset.size_bytes
                {
                    upsert_hardlink_member(
                        &tx,
                        repo_id,
                        &source_asset.asset_id,
                        source_path,
                        &target_asset.content_hash,
                        target_asset.size_bytes,
                        "linked",
                    )
                    .map_err(db_error)?;
                }
            }
        }
        tx.commit().map_err(db_error)
    }

    pub fn rename_entry(&self, request: FileRenameRequest) -> Result<FileBrowserSnapshot, String> {
        browser::rename_entry(self, request)
    }

    pub fn delete_entry(&self, request: FileDeleteRequest) -> Result<FileBrowserSnapshot, String> {
        browser::delete_entry(self, request)
    }

    pub fn mutate_trash(
        &self,
        request: TrashMutationRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::mutate_trash(self, request)
    }

    pub fn undo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        let revision = load_latest_revision(&tx, &request.asset_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("no revision found for asset: {}", request.asset_id))?;
        let previous_metadata =
            load_metadata_map_from_transaction(&tx, &request.asset_id).map_err(db_error)?;
        apply_revision_state(
            &tx,
            &request.repo_id,
            &request.asset_id,
            &revision.before,
            "revision.undo",
            "undo",
        )
        .map_err(db_error)?;
        let current_metadata =
            load_metadata_map_from_transaction(&tx, &request.asset_id).map_err(db_error)?;
        if let Some((path, shared_asset_id)) =
            load_source_asset_writeback_target(&tx, &request.repo_id, &request.asset_id)
                .map_err(db_error)?
        {
            write_backend_asset_metadata(
                &self.root,
                &repo,
                Path::new(&repo.summary.path),
                &path,
                shared_asset_id.as_deref(),
                &current_metadata,
                &previous_metadata,
                "undo",
            )?;
        }
        tx.commit().map_err(db_error)?;

        let asset = self.load_asset_detail(&request.repo_id, &request.asset_id)?;
        Ok(RevisionActionResponse {
            outcome: "success".to_string(),
            asset,
        })
    }

    pub fn redo_last_revision(
        &self,
        request: RevisionActionRequest,
    ) -> Result<RevisionActionResponse, String> {
        self.ensure_initialized()?;
        let repo = self.load_repository_record(&request.repo_id)?;
        let mut connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let tx = connection.transaction().map_err(db_error)?;

        let revision = load_latest_revision(&tx, &request.asset_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("no revision found for asset: {}", request.asset_id))?;
        let previous_metadata =
            load_metadata_map_from_transaction(&tx, &request.asset_id).map_err(db_error)?;
        apply_revision_state(
            &tx,
            &request.repo_id,
            &request.asset_id,
            &revision.after,
            "revision.redo",
            "redo",
        )
        .map_err(db_error)?;
        let current_metadata =
            load_metadata_map_from_transaction(&tx, &request.asset_id).map_err(db_error)?;
        if let Some((path, shared_asset_id)) =
            load_source_asset_writeback_target(&tx, &request.repo_id, &request.asset_id)
                .map_err(db_error)?
        {
            write_backend_asset_metadata(
                &self.root,
                &repo,
                Path::new(&repo.summary.path),
                &path,
                shared_asset_id.as_deref(),
                &current_metadata,
                &previous_metadata,
                "redo",
            )?;
        }
        tx.commit().map_err(db_error)?;

        let asset = self.load_asset_detail(&request.repo_id, &request.asset_id)?;
        Ok(RevisionActionResponse {
            outcome: "success".to_string(),
            asset,
        })
    }

    pub fn list_plugins(&self) -> Result<Vec<PluginManifest>, String> {
        plugin::list_plugins(self)
    }

    pub fn list_plugin_hook_executions(
        &self,
        request: PluginHookExecutionListRequest,
    ) -> Result<PluginHookExecutionListResponse, String> {
        plugin::list_plugin_hook_executions(self, request)
    }

    pub fn set_plugin_enabled(
        &self,
        request: PluginEnabledRequest,
    ) -> Result<PluginMutationResponse, String> {
        plugin::set_plugin_enabled(self, request)
    }

    pub fn delete_plugin(&self, plugin_id: String) -> Result<PluginMutationResponse, String> {
        plugin::delete_plugin(self, plugin_id)
    }

    pub fn install_plugin_from_archive(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginMutationResponse, String> {
        plugin::install_plugin_from_archive(self, request)
    }

    pub fn get_cache_snapshot(&self) -> Result<CacheSnapshot, String> {
        plugin::get_cache_snapshot(self)
    }

    pub fn get_api_design_snapshot(&self) -> Result<ApiDesignSnapshot, String> {
        plugin::get_api_design_snapshot(self)
    }

    pub(super) fn load_repository_record(&self, repo_id: &str) -> Result<RepositoryRecord, String> {
        let registry = open_registry_connection(&self.registry_path)?;
        let plugin_registry = plugin_catalog(&self.root);
        registry
            .query_row(
                r#"
                SELECT repo_id, name, path, backend_plugin_id, backend_config_json, status, updated_at
                FROM repositories
                WHERE repo_id = ?1
                "#,
                [repo_id],
                |row| {
                    let backend_plugin_id: String = row.get(3)?;
                    let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                    let path: String = row.get(2)?;
                    let stored_status: String = row.get(5)?;
                    let backend =
                        backend_summary_from_registry(&plugin_registry, &backend_plugin_id);
                    let status = repository_runtime_status(&path, &backend, stored_status.as_str());
                    let backend_config_json: String = row.get(4)?;
                    let backend_config = parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                    Ok(RepositoryRecord {
                        summary: RepositorySummary {
                            repo_id: row.get(0)?,
                            name: row.get(1)?,
                            path: path.clone(),
                            backend,
                            status,
                            asset_count: 0,
                            updated_at: row.get(6)?,
                            local_cache: repository_local_cache_status(&path, &backend_plugin_id),
                        },
                        backend_record: RepositoryBackendRecord {
                            plugin_id: backend_plugin_id,
                            config: backend_config,
                        },
                    })
                },
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("repository not found: {repo_id}"))
    }

    pub(super) fn load_repository_records(&self) -> Result<Vec<RepositoryRecord>, String> {
        self.ensure_initialized()?;
        let registry = open_registry_connection(&self.registry_path)?;
        let mut stmt = registry
            .prepare(
                r#"
                SELECT repo_id, name, path, backend_plugin_id, backend_config_json, status, updated_at
                FROM repositories
                ORDER BY name COLLATE NOCASE
                "#,
            )
            .map_err(db_error)?;
        let plugin_registry = plugin_catalog(&self.root);
        let rows = stmt
            .query_map([], |row| {
                let backend_plugin_id: String = row.get(3)?;
                let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                let path: String = row.get(2)?;
                let stored_status: String = row.get(5)?;
                let backend = backend_summary_from_registry(&plugin_registry, &backend_plugin_id);
                let status = repository_runtime_status(&path, &backend, stored_status.as_str());
                let backend_config_json: String = row.get(4)?;
                let backend_config =
                    parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                Ok(RepositoryRecord {
                    summary: RepositorySummary {
                        repo_id: row.get(0)?,
                        name: row.get(1)?,
                        path: path.clone(),
                        backend,
                        status,
                        asset_count: 0,
                        updated_at: row.get(6)?,
                        local_cache: repository_local_cache_status(&path, &backend_plugin_id),
                    },
                    backend_record: RepositoryBackendRecord {
                        plugin_id: backend_plugin_id,
                        config: backend_config,
                    },
                })
            })
            .map_err(db_error)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub(super) fn find_existing_repository_for_backend(
        &self,
        backend: &RepositoryBackendRecord,
    ) -> Result<Option<RepositorySummary>, String> {
        if backend.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
            return Ok(None);
        }
        let account_id = backend
            .config
            .get("accountId")
            .and_then(normalized_netease_account_id);
        let Some(account_id) = account_id else {
            return Ok(None);
        };
        Ok(self
            .load_repository_records()?
            .into_iter()
            .find(|record| {
                record.backend_record.plugin_id == NETEASE_CLOUD_MUSIC_PLUGIN_ID
                    && record
                        .backend_record
                        .config
                        .get("accountId")
                        .and_then(normalized_netease_account_id)
                        .as_deref()
                        == Some(account_id.as_str())
            })
            .map(|record| record.summary))
    }

    pub(super) fn repository_backend_in_use(&self, plugin_id: &str) -> Result<bool, String> {
        let registry = open_registry_connection(&self.registry_path)?;
        let plugin_registry = plugin_catalog(&self.root);
        let mut stmt = registry
            .prepare("SELECT backend_plugin_id FROM repositories")
            .map_err(db_error)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;

        for row in rows {
            let stored_plugin_id = row.map_err(db_error)?;
            if plugin_registry.normalize_plugin_id(&stored_plugin_id) == plugin_id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn open_repository_connection(
        &self,
        repo_id: &str,
        repo_path: &str,
        backend_record: &RepositoryBackendRecord,
    ) -> Result<Connection, String> {
        let repo_root = Path::new(repo_path);
        let storage_paths = ensure_repository_storage_paths(
            &self.root,
            repo_id,
            repo_root,
            &backend_record.plugin_id,
        )?;
        let database_missing = !storage_paths.database_path.exists();
        let metadata_missing = !storage_paths
            .metadata_dir
            .join(REPO_METADATA_FILE_NAME)
            .exists();
        let (mut connection, database_rebuilt) =
            match self.try_open_repository_connection(&storage_paths.database_path) {
                Ok(connection) => (connection, false),
                Err(error)
                    if storage_paths.database_path.exists()
                        && is_database_locked_error_message(&error) =>
                {
                    return Err(error);
                }
                Err(_) if storage_paths.database_path.exists() => {
                    // 数据库文件损坏时直接重建，再从后端重新同步内容。
                    fs::remove_file(&storage_paths.database_path).map_err(io_error)?;
                    (
                        self.try_open_repository_connection(&storage_paths.database_path)?,
                        true,
                    )
                }
                Err(error) => return Err(error),
            };
        let repo = self.load_repository_record(repo_id)?;
        if metadata_missing
            || database_missing
            || database_rebuilt
            || !repository_identity_record_is_current(&connection, &repo).map_err(db_error)?
        {
            ensure_repository_identity_record(&connection, &repo)?;
        }

        if metadata_missing {
            write_repository_metadata(
                &storage_paths.metadata_dir,
                &repo.summary.repo_id,
                &repo.summary.name,
                repo_root,
                &repo.backend_record.plugin_id,
                &repo.backend_record.config,
                None,
            )?;
        }
        if metadata_missing || database_missing || database_rebuilt {
            // `.momo` 被删或被读链路部分重建后，这里补一次同步，确保首次打开就能看到内容。
            let tx = connection.transaction().map_err(db_error)?;
            sync_repository_files(
                &self.root,
                &tx,
                &repo,
                &std::collections::HashSet::new(),
                &std::collections::BTreeSet::new(),
            )
            .map_err(db_error)?;
            tx.commit().map_err(db_error)?;
        }
        Ok(connection)
    }

    fn try_open_repository_connection(&self, database_path: &Path) -> Result<Connection, String> {
        open_repository_database_connection(database_path)
    }

    pub(super) fn repository_thumbnail_root(
        &self,
        repo: &RepositoryRecord,
    ) -> Result<PathBuf, String> {
        let repo_root = Path::new(&repo.summary.path);
        let storage_paths = ensure_repository_storage_paths(
            &self.root,
            &repo.summary.repo_id,
            repo_root,
            &repo.backend_record.plugin_id,
        )?;
        Ok(storage_paths.metadata_dir.join("thumbnails"))
    }
}
