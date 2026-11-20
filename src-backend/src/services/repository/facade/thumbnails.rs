//! 仓库状态的缩略图生成入口。

use super::*;

impl RepositoryState {
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
                // 文件夹只有自定义缩略图，没有可重新生成的：刷新和 ensure 一样交回现有的，不清掉自定义。
                "ensure" | "refresh" => {
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
                "clear" => {
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
            // 重新生成源文件的缩略图；有自定义缩略图时列表显示的还是它，交回的也是它。
            "refresh" => {
                let generated = ensure_thumbnail_for_file(
                    &repo,
                    &repo_root,
                    &thumbnail_root,
                    &file,
                    existing_thumbnail_path,
                    true,
                )?;
                match custom_record {
                    Some(record) => (Some(record.path), true),
                    None => (generated, false),
                }
            }
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
}
