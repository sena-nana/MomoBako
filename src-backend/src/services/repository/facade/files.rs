//! 仓库状态的文件条目操作、硬链接复制记录与修订撤销重做入口。

use super::*;

impl RepositoryState {
    pub fn create_directory(
        &self,
        request: FileCreateRequest,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::create_directory(self, request)
    }

    pub fn create_file(&self, request: FileCreateRequest) -> Result<FileBrowserSnapshot, String> {
        browser::create_file(self, request)
    }

    pub fn import_entries(&self, request: FileImportRequest) -> Result<FileImportResponse, String> {
        browser::import_entries_cancellable(self, request, &NeverCancelled)
    }

    pub(crate) fn import_entries_cancellable(
        &self,
        request: FileImportRequest,
        cancellation: &dyn CancellationCheck,
    ) -> Result<FileImportResponse, String> {
        browser::import_entries_cancellable(self, request, cancellation)
    }

    pub(crate) fn import_archive_entries_cancellable(
        &self,
        request: FileArchiveImportRequest,
        cancellation: &dyn CancellationCheck,
    ) -> Result<FileBrowserSnapshot, String> {
        importing::import_archive_entries_cancellable(self, request, cancellation)
    }

    pub(crate) fn import_eagle_library_cancellable(
        &self,
        request: EagleLibraryImportRequest,
        cancellation: &dyn CancellationCheck,
    ) -> Result<EagleLibraryImportResponse, String> {
        importing::import_eagle_library_cancellable(self, request, cancellation)
    }

    pub fn add_external_assets(
        &self,
        request_id: String,
        request: ExternalAddAssetRequest,
    ) -> ExternalAddAssetResponse {
        external_assets::add_external_assets(self, request_id, request)
    }

    pub(crate) fn copy_entries_cancellable(
        &self,
        request: FileCopyRequest,
        cancellation: &dyn CancellationCheck,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::copy_entries_cancellable(self, request, cancellation)
    }

    pub(crate) fn move_entries_cancellable(
        &self,
        request: FileMoveRequest,
        cancellation: &dyn CancellationCheck,
    ) -> Result<FileBrowserSnapshot, String> {
        browser::move_entries_cancellable(self, request, cancellation)
    }

    pub(in super::super) fn finish_file_copy_operation(
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

    pub(in super::super) fn record_hardlink_copy_outcomes(
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

        let cursor =
            load_revision_cursor(&tx, &request.repo_id, &request.asset_id).map_err(db_error)?;
        let revision = match cursor {
            Some(cursor) => cursor
                .current_revision_id
                .as_deref()
                .map(|revision_id| load_revision_by_id(&tx, revision_id))
                .transpose()
                .map_err(db_error)?
                .flatten(),
            None => load_latest_revision(&tx, &request.asset_id).map_err(db_error)?,
        }
        .ok_or_else(|| format!("no previous revision found for asset: {}", request.asset_id))?;
        let previous_revision = load_previous_revision(&tx, &revision).map_err(db_error)?;
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
        save_revision_cursor(
            &tx,
            &request.repo_id,
            &request.asset_id,
            previous_revision
                .as_ref()
                .map(|revision| revision.revision_id.as_str()),
            Some(&revision.revision_id),
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

        let cursor =
            load_revision_cursor(&tx, &request.repo_id, &request.asset_id).map_err(db_error)?;
        let revision_id = cursor
            .and_then(|cursor| cursor.redo_revision_id)
            .ok_or_else(|| format!("no redo revision found for asset: {}", request.asset_id))?;
        let revision = load_revision_by_id(&tx, &revision_id)
            .map_err(db_error)?
            .ok_or_else(|| format!("no redo revision found for asset: {}", request.asset_id))?;
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
        save_revision_cursor(
            &tx,
            &request.repo_id,
            &request.asset_id,
            Some(&revision.revision_id),
            None,
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
}
