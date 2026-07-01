//! Repository query and playback source preparation workflows.

use super::*;

pub(super) fn load_snapshot(
    state: &RepositoryState,
    repo_id: &str,
) -> Result<RepositorySnapshot, String> {
    state.ensure_initialized()?;

    let repo = state.load_repository_record(repo_id)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let asset_count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets WHERE status != 'deleted'",
            [],
            |row| row.get(0),
        )
        .map_err(db_error)?;

    let thumbnail_root = state.repository_thumbnail_root(&repo)?;
    let folders = load_folder_summaries(&connection, repo_id).map_err(db_error)?;
    let assets = normalize_asset_summaries(
        &connection,
        &repo,
        &thumbnail_root,
        load_assets(&connection, repo_id).map_err(db_error)?,
    )?;
    let quick_access = load_repository_shortcuts(&connection, repo_id).map_err(db_error)?;
    let tag_groups = load_repository_tag_groups(&connection, repo_id).map_err(db_error)?;
    let playlists = load_playlists(&connection, repo_id).map_err(db_error)?;
    let metadata_fields = load_metadata_fields(&connection).map_err(db_error)?;
    let recent_revision_count: i64 = connection
        .query_row("SELECT COUNT(*) FROM revisions", [], |row| row.get(0))
        .map_err(db_error)?;
    let overview = build_repository_overview(&repo_root, &assets)?;

    Ok(RepositorySnapshot {
        repository: RepositorySummary {
            asset_count,
            ..repo.summary
        },
        folder_label: dominant_folder_label(&folders, &assets),
        folders,
        assets,
        playlists,
        quick_access,
        tag_groups,
        metadata_fields,
        recent_revision_count,
        overview,
    })
}

pub(super) fn load_asset_detail(
    state: &RepositoryState,
    repo_id: &str,
    asset_id: &str,
) -> Result<AssetDetail, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    load_asset_detail_from_connection(&connection, repo_id, asset_id).map_err(db_error)
}

pub(super) fn read_file(
    state: &RepositoryState,
    request: FileReadRequest,
) -> Result<Vec<u8>, String> {
    state.ensure_initialized()?;

    let repo = state.load_repository_record(&request.repo_id)?;
    if !repository_supports_local_read_access(&repo) {
        return Err(format!(
            "file preview read is not available for backend: {}",
            repo.summary.backend.plugin_id
        ));
    }

    let entry_path = normalize_entry_path(&request.path)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let file_path = resolve_repository_relative_path(&repo_root, &entry_path)?;
    if !file_path.exists() {
        return Err(format!("file not found: {entry_path}"));
    }
    if !file_path.is_file() {
        return Err(format!("path is not a file: {entry_path}"));
    }

    fs::read(file_path).map_err(io_error)
}

pub(super) fn prepare_preview_file_source(
    state: &RepositoryState,
    request: FileReadRequest,
) -> Result<FilePreviewSourceResponse, String> {
    state.ensure_initialized()?;

    let repo = state.load_repository_record(&request.repo_id)?;
    if !repository_supports_local_read_access(&repo) {
        return Err(format!(
            "file preview source is not available for backend: {}",
            repo.summary.backend.plugin_id
        ));
    }

    let entry_path = normalize_entry_path(&request.path)?;
    let repo_root = PathBuf::from(&repo.summary.path);
    let file_path = resolve_repository_relative_path(&repo_root, &entry_path)?;
    if !file_path.exists() {
        return Err(format!("file not found: {entry_path}"));
    }
    if !file_path.is_file() {
        return Err(format!("path is not a file: {entry_path}"));
    }

    let metadata = fs::metadata(&file_path).map_err(io_error)?;
    let modified_at = metadata
        .modified()
        .ok()
        .map(system_time_to_rfc3339)
        .transpose()
        .map_err(time_error)?;
    let extension = file_path
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let source_path = file_path.clone();
    let media_type = preview_media_type_for_extension(&extension).to_string();
    let token = preview_file_token(
        &repo.summary.repo_id,
        &repo.summary.path,
        &entry_path,
        metadata.len(),
        modified_at.as_deref().unwrap_or_default(),
    );

    state
        .preview_sources
        .lock()
        .map_err(|_| "preview source lock poisoned".to_string())?
        .insert(
            token.clone(),
            PreviewFileSource {
                path: source_path,
                media_type: media_type.clone(),
            },
        );

    Ok(FilePreviewSourceResponse {
        repo_id: request.repo_id,
        path: entry_path,
        token,
        source_url: None,
        local_path: Some(file_path.to_string_lossy().to_string()),
        media_type,
        size_bytes: metadata.len() as i64,
        modified_at,
    })
}

pub(super) fn prepare_entry_playback_source(
    state: &RepositoryState,
    request: EntryPlaybackRequest,
) -> Result<EntryPlaybackSourceResponse, String> {
    state.prepare_entry_playback_source_internal(request, None)
}

pub(super) fn prepare_entry_playback_source_with_progress(
    state: &RepositoryState,
    request: EntryPlaybackRequest,
    emit: &mut dyn FnMut(EntryPlaybackProgressEvent) -> Result<(), String>,
) -> Result<EntryPlaybackSourceResponse, String> {
    state.prepare_entry_playback_source_internal(request, Some(emit))
}

pub(super) fn search_assets(
    state: &RepositoryState,
    request: SearchRequest,
) -> Result<SearchResponse, String> {
    state.ensure_initialized()?;

    let normalized_query = request.query.trim().to_lowercase();
    if normalized_query.is_empty()
        && request.tag.is_none()
        && request
            .tags
            .as_ref()
            .map(|items| items.iter().all(|item| item.trim().is_empty()))
            .unwrap_or(true)
        && request.metadata_key.is_none()
        && request
            .exclude_query
            .as_ref()
            .map(|item| item.trim().is_empty())
            .unwrap_or(true)
        && request
            .exclude_path_prefixes
            .as_ref()
            .map(|items| items.iter().all(|item| item.trim().is_empty()))
            .unwrap_or(true)
        && request
            .exclude_tags
            .as_ref()
            .map(|items| items.iter().all(|item| item.trim().is_empty()))
            .unwrap_or(true)
        && request
            .exclude_formats
            .as_ref()
            .map(|items| items.iter().all(|item| item.trim().is_empty()))
            .unwrap_or(true)
        && request
            .exclude_metadata_filters
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .all(|item| item.key.trim().is_empty() || item.value.trim().is_empty())
            })
            .unwrap_or(true)
        && request
            .exclude_number_filters
            .as_ref()
            .map(|items| {
                items.iter().all(|item| {
                    item.key.trim().is_empty() || (item.min.is_none() && item.max.is_none())
                })
            })
            .unwrap_or(true)
        && request
            .exclude_date_filters
            .as_ref()
            .map(|items| {
                items.iter().all(|item| {
                    item.key.trim().is_empty() || (item.from.is_none() && item.to.is_none())
                })
            })
            .unwrap_or(true)
        && request
            .metadata_filters
            .as_ref()
            .map(|items| {
                items
                    .iter()
                    .all(|item| item.key.trim().is_empty() || item.value.trim().is_empty())
            })
            .unwrap_or(true)
        && request
            .formats
            .as_ref()
            .map(|items| items.iter().all(|item| item.trim().is_empty()))
            .unwrap_or(true)
        && request.min_rating.is_none()
    {
        return Ok(SearchResponse {
            query: request.query,
            results: Vec::new(),
        });
    }

    let repositories = state.load_repository_records()?;
    let mut results = Vec::new();

    for repo in repositories {
        if let Some(filter_repo_id) = &request.repo_id {
            if &repo.summary.repo_id != filter_repo_id {
                continue;
            }
        }
        let connection = state.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let repo_results =
            search_repository_assets(&connection, &repo.summary, &normalized_query, &request)
                .map_err(db_error)?;
        results.extend(repo_results);
    }

    Ok(SearchResponse {
        query: request.query,
        results,
    })
}

pub(super) fn update_asset_metadata(
    state: &RepositoryState,
    request: MetadataUpdateRequest,
) -> Result<MetadataUpdateResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let tx = connection.transaction().map_err(db_error)?;

    let current_version: i64 = tx
        .query_row(
            "SELECT version FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
            params![request.repo_id, request.asset_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?
        .ok_or_else(|| format!("asset not found: {}", request.asset_id))?;

    if current_version != request.expected_version {
        let asset = load_asset_detail_from_transaction(&tx, &request.repo_id, &request.asset_id)
            .map_err(db_error)?;
        return Ok(MetadataUpdateResponse {
            outcome: "conflict".to_string(),
            asset,
        });
    }

    let source = request.source.unwrap_or_else(|| "desktop".to_string());
    update_metadata_for_asset_in_transaction(
        &tx,
        &request.repo_id,
        &request.asset_id,
        &request.metadata,
        &source,
    )
    .map_err(db_error)?;
    tx.commit().map_err(db_error)?;
    let asset = state.load_asset_detail(&request.repo_id, &request.asset_id)?;

    Ok(MetadataUpdateResponse {
        outcome: "success".to_string(),
        asset,
    })
}

impl RepositoryState {
    fn prepare_entry_playback_source_internal(
        &self,
        request: EntryPlaybackRequest,
        mut emit: Option<&mut dyn FnMut(EntryPlaybackProgressEvent) -> Result<(), String>>,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        self.ensure_initialized()?;

        let repo = self.load_repository_record(&request.repo_id)?;
        let entry_path = normalize_entry_path(&request.path)?;
        emit_entry_playback_progress(
            &mut emit,
            "resolve",
            &request.repo_id,
            &entry_path,
            8,
            "解析媒体条目",
            false,
            None,
            None,
        )?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        let asset_map = load_asset_path_map(&connection, &request.repo_id).map_err(db_error)?;

        if let Some(asset) = asset_map.get(&entry_path) {
            if !asset.is_virtual {
                emit_entry_playback_progress(
                    &mut emit,
                    "preview",
                    &request.repo_id,
                    &entry_path,
                    82,
                    "准备本地预览源",
                    false,
                    Some(true),
                    None,
                )?;
                let preview = self.prepare_preview_file_source(FileReadRequest {
                    repo_id: request.repo_id.clone(),
                    path: entry_path.clone(),
                })?;
                let response = EntryPlaybackSourceResponse {
                    repo_id: request.repo_id,
                    path: entry_path,
                    media_type: preview.media_type,
                    source_url: None,
                    local_path: asset.local_absolute_path.clone(),
                    temp_file_path: None,
                    lyric_path: None,
                    lyric_source_url: None,
                    word_lyric_path: None,
                    word_lyric_source_url: None,
                    expires_at: None,
                    size_bytes: Some(preview.size_bytes),
                    modified_at: preview.modified_at,
                };
                emit_entry_playback_progress(
                    &mut emit,
                    "ready",
                    &response.repo_id,
                    &response.path,
                    100,
                    "播放源已就绪",
                    false,
                    Some(true),
                    None,
                )?;
                return Ok(response);
            }

            let metadata = load_metadata_map(&connection, &asset.asset_id).map_err(db_error)?;
            let source_payload = asset
                .source_payload
                .clone()
                .or_else(|| metadata.get("sourcePayload").cloned())
                .unwrap_or_else(|| serde_json::json!({}));
            return self.prepare_virtual_entry_playback_source(
                &repo,
                request.repo_id,
                entry_path,
                source_payload,
                &metadata,
                emit,
            );
        }

        emit_entry_playback_progress(
            &mut emit,
            "resolve",
            &request.repo_id,
            &entry_path,
            18,
            "从来源补取媒体信息",
            true,
            None,
            None,
        )?;
        let repo_root = PathBuf::from(&repo.summary.path);
        let entry = stat_backend_entry(&self.root, &repo, &repo_root, &entry_path)?;
        if !entry.is_virtual {
            return Err(format!("asset not found: {entry_path}"));
        }
        let metadata = BTreeMap::new();
        let source_payload = entry
            .source_payload
            .unwrap_or_else(|| serde_json::json!({}));
        self.prepare_virtual_entry_playback_source(
            &repo,
            request.repo_id,
            entry_path,
            source_payload,
            &metadata,
            emit,
        )
    }

    fn prepare_virtual_entry_playback_source(
        &self,
        repo: &RepositoryRecord,
        repo_id: String,
        entry_path: String,
        source_payload: serde_json::Value,
        metadata: &BTreeMap<String, serde_json::Value>,
        mut emit: Option<&mut dyn FnMut(EntryPlaybackProgressEvent) -> Result<(), String>>,
    ) -> Result<EntryPlaybackSourceResponse, String> {
        let song_id = source_payload
            .get("songId")
            .or_else(|| metadata.get("songId"))
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                source_payload
                    .get("songId")
                    .or_else(|| metadata.get("songId"))
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| value.parse::<i64>().ok())
            })
            .ok_or_else(|| "virtual entry is missing songId".to_string())?;
        let account_cookie = source_payload
            .get("accountCookie")
            .or_else(|| metadata.get("accountCookie"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let backend_account_cookie = repo
            .backend_record
            .config
            .get("cookie")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(account_cookie);
        emit_entry_playback_progress(
            &mut emit,
            "download",
            &repo_id,
            &entry_path,
            28,
            "下载临时音频",
            true,
            None,
            None,
        )?;
        let managed_cache_root = if repo.backend_record.plugin_id == NETEASE_CLOUD_MUSIC_PLUGIN_ID {
            Some(ensure_netease_cache_ready(repo)?)
        } else {
            None
        };
        let payload = serde_json::json!({
            "accountCookie": backend_account_cookie,
            "songId": song_id,
            "level": source_payload.get("level").cloned().unwrap_or_else(|| serde_json::json!("standard")),
            "repoId": repo_id,
            "entryPath": entry_path,
            "managedCacheRoot": managed_cache_root.as_ref().map(|path| path.to_string_lossy().to_string()),
            "sourcePayload": source_payload,
        });
        let response = call_downloader_prepare_track_playback(&self.root, payload)?;
        let cached = response.get("cached").and_then(serde_json::Value::as_bool);
        emit_entry_playback_progress(
            &mut emit,
            "preview",
            &repo_id,
            &entry_path,
            88,
            if cached == Some(true) {
                "使用已缓存音频"
            } else {
                "下载完成，准备预览源"
            },
            false,
            cached,
            None,
        )?;
        let media_type = response
            .get("mediaType")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("audio/mpeg")
            .to_string();
        let playback = EntryPlaybackSourceResponse {
            repo_id,
            path: entry_path,
            media_type,
            source_url: response
                .get("sourceUrl")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            local_path: response
                .get("localPath")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            temp_file_path: response
                .get("tempFilePath")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            lyric_path: response
                .get("lyricPath")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            lyric_source_url: None,
            word_lyric_path: response
                .get("wordLyricPath")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            word_lyric_source_url: None,
            expires_at: response
                .get("expiresAt")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
            size_bytes: response
                .get("sizeBytes")
                .and_then(serde_json::Value::as_i64),
            modified_at: response
                .get("modifiedAt")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        };
        emit_entry_playback_progress(
            &mut emit,
            "ready",
            &playback.repo_id,
            &playback.path,
            100,
            "播放源已就绪",
            false,
            cached,
            None,
        )?;
        Ok(playback)
    }
}
