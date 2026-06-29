//! Repository file synchronization and revision bookkeeping.

use super::*;

pub(super) fn build_directory_records_from_files(
    repo_root: &Path,
    files: &[DiscoveredFile],
) -> Result<Vec<DirectoryRecord>, String> {
    let mut directories = BTreeMap::<String, DirectoryRecord>::new();
    collect_directory_records(repo_root, repo_root, &mut directories)?;
    for file in files {
        let mut current = Path::new(&file.relative_path).parent();
        while let Some(parent) = current {
            let raw = parent.to_string_lossy().replace('\\', "/");
            let path = if raw == "." { String::new() } else { raw };
            let parent_path = parent_relative_path(&path);
            let name = if path.is_empty() {
                String::new()
            } else {
                Path::new(&path)
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_default()
            };
            directories
                .entry(path.clone())
                .and_modify(|record| {
                    if record.updated_at < file.modified_at {
                        record.updated_at = file.modified_at.clone();
                    }
                })
                .or_insert_with(|| DirectoryRecord {
                    path: path.clone(),
                    parent_path,
                    name,
                    updated_at: file.modified_at.clone(),
                });
            current = parent.parent();
        }
    }
    let mut values = directories.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(values)
}

fn collect_directory_records(
    repo_root: &Path,
    current_dir: &Path,
    directories: &mut BTreeMap<String, DirectoryRecord>,
) -> Result<(), String> {
    let relative = current_dir
        .strip_prefix(repo_root)
        .map_err(|error| error.to_string())?
        .to_string_lossy()
        .replace('\\', "/");
    let path = if relative == "." { String::new() } else { relative };
    let parent_path = parent_relative_path(&path);
    let name = if path.is_empty() {
        String::new()
    } else {
        Path::new(&path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    let updated_at = fs::metadata(current_dir)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(system_time_to_rfc3339)
        .transpose()
        .map_err(time_error)?
        .unwrap_or_else(now_rfc3339);
    directories.insert(
        path.clone(),
        DirectoryRecord {
            path,
            parent_path,
            name,
            updated_at,
        },
    );
    for entry in fs::read_dir(current_dir).map_err(io_error)? {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_filesystem_error(&error) => continue,
            Err(error) => return Err(io_error(error)),
        };
        if !metadata.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }
        collect_directory_records(repo_root, &entry.path(), directories)?;
    }
    Ok(())
}

pub(super) fn metadata_defaults_for_files(
    service_root: &Path,
    files: &[DiscoveredFile],
    existing_metadata_by_path: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, String> {
    if files.is_empty() {
        return Ok(BTreeMap::new());
    }

    let registry = backend_plugin_registry(service_root);
    let providers = registry.metadata_default_providers();
    if providers.is_empty() {
        return Ok(BTreeMap::new());
    }

    let entries = files
        .iter()
        .map(|file| MetadataDefaultsBatchEntry {
            path: file.relative_path.clone(),
            name: file.filename.clone(),
            extension: file.extension.clone(),
            kind: "file".to_string(),
            metadata: existing_metadata_by_path.get(&file.relative_path).cloned(),
        })
        .collect::<Vec<_>>();
    let payload = serde_json::json!({ "entries": entries });
    let mut defaults_by_path = BTreeMap::<String, BTreeMap<String, serde_json::Value>>::new();

    for (plugin_id, action) in providers {
        let response = registry.call(&plugin_id, &action, payload.clone())?;
        let parsed = serde_json::from_value::<MetadataDefaultsBatchResponse>(response)
            .map_err(json_error)?;
        for (path, defaults) in parsed.defaults_by_path {
            if !files.iter().any(|file| file.relative_path == path) {
                continue;
            }
            defaults_by_path.entry(path).or_default().extend(defaults);
        }
    }

    Ok(defaults_by_path)
}

pub(super) fn sync_repository_files(
    service_root: &Path,
    tx: &Transaction<'_>,
    repo: &RepositoryRecord,
    skip_hardlink_candidate_paths: &HashSet<String>,
) -> Result<SyncResult, rusqlite::Error> {
    let repo_root = PathBuf::from(&repo.summary.path);
    let files = list_backend_files(service_root, repo, &repo_root).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;

    let mut existing_stmt = tx.prepare(
        r#"
        SELECT
          asset_id,
          path,
          status,
          thumbnail_path,
          size_bytes,
          created_at,
          modified_at,
          hash,
          is_virtual,
          provider_id,
          provider_item_id,
          source_payload_json,
          local_absolute_path
        FROM assets
        WHERE repo_id = ?1
        "#,
    )?;
    let existing_rows = existing_stmt.query_map([repo.summary.repo_id.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            ExistingAssetRecord {
                asset_id: row.get::<_, String>(0)?,
                status: row.get::<_, String>(2)?,
                thumbnail_path: row.get::<_, Option<String>>(3)?,
                size_bytes: row.get::<_, i64>(4)?,
                created_at: row.get::<_, String>(5)?,
                modified_at: row.get::<_, String>(6)?,
                hash: row.get::<_, Option<String>>(7)?,
                is_virtual: row.get::<_, i64>(8)? != 0,
                provider_id: row.get::<_, Option<String>>(9)?,
                provider_item_id: row.get::<_, Option<String>>(10)?,
                source_payload: parse_json_column_nullable(row.get::<_, Option<String>>(11)?)?,
                local_absolute_path: row.get::<_, Option<String>>(12)?,
            },
        ))
    })?;
    let existing = existing_rows.collect::<Result<Vec<_>, _>>()?;
    let existing_asset_ids = existing
        .iter()
        .map(|(_asset_id, _path, record)| record.asset_id.clone())
        .collect::<Vec<_>>();
    let existing_metadata_by_asset_id = load_metadata_maps_for_assets(tx, &existing_asset_ids)?;
    let existing_metadata_by_path = existing
        .iter()
        .filter_map(|(_asset_id, path, record)| {
            existing_metadata_by_asset_id
                .get(&record.asset_id)
                .cloned()
                .map(|metadata| (path.clone(), metadata))
        })
        .collect::<BTreeMap<_, _>>();
    let plugin_defaults_by_path =
        metadata_defaults_for_files(service_root, &files, &existing_metadata_by_path).map_err(
            |error| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    error,
                )))
            },
        )?;
    let mut existing_by_path = existing
        .into_iter()
        .map(|(_asset_id, path, record)| (path, record))
        .collect::<BTreeMap<_, _>>();
    let directory_records = build_directory_records_from_files(&repo_root, &files).map_err(
        |error| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                error,
            )))
        },
    )?;

    let now = now_rfc3339();
    let mut created_assets = 0_i64;
    let mut updated_assets = 0_i64;
    let mut deleted_assets = 0_i64;
    let mut created_events = 0_i64;

    for file in &files {
        if let Some(existing_record) = existing_by_path.remove(&file.relative_path) {
            let asset_id = existing_record.asset_id;
            let asset_created_at = existing_record.created_at.clone();
            let content_hash = if file.is_virtual {
                existing_record.hash.unwrap_or_default()
            } else if existing_record.size_bytes == file.size_bytes
                && existing_record.modified_at == file.modified_at
            {
                match existing_record.hash.filter(|hash| is_content_hash(hash)) {
                    Some(hash) => hash,
                    None => file_sha256_hash(file.absolute_path.as_deref().ok_or_else(|| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "missing absolute path for non-virtual file",
                        )))
                    })?)
                    .map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            error,
                        )))
                    })?,
                }
            } else {
                file_sha256_hash(file.absolute_path.as_deref().ok_or_else(|| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "missing absolute path for non-virtual file",
                    )))
                })?)
                .map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        error,
                    )))
                })?
            };
            tx.execute(
                r#"
                UPDATE assets
                SET filename = ?3, extension = ?4, size_bytes = ?5, modified_at = ?6, hash = ?7,
                    status = 'synced', updated_at = ?8, thumbnail_path = ?9, is_virtual = ?10,
                    provider_id = ?11, provider_item_id = ?12, source_payload_json = ?13, local_absolute_path = ?14
                WHERE repo_id = ?1 AND asset_id = ?2
                "#,
                params![
                    repo.summary.repo_id,
                    asset_id,
                    file.filename,
                    file.extension,
                    file.size_bytes,
                    file.modified_at,
                    if content_hash.is_empty() { None } else { Some(content_hash.as_str()) },
                    now,
                    existing_record.thumbnail_path,
                    if file.is_virtual { 1 } else { 0 },
                    file.provider_id,
                    file.provider_item_id,
                    file.source_payload.as_ref().map(|value| value.to_string()),
                    file.local_absolute_path
                ],
            )?;
            if existing_record.status == "deleted" {
                created_events += 1;
            }
            if !file.is_virtual && !content_hash.is_empty() {
                update_hardlink_member_verification(
                    tx,
                    &repo.summary.repo_id,
                    &asset_id,
                    &file.relative_path,
                    &content_hash,
                )?;
            }
            ensure_default_metadata(
                tx,
                &asset_id,
                &file.relative_path,
                &file.filename,
                &file.extension,
                &asset_created_at,
                file.created_at.as_deref(),
                &[],
                plugin_defaults_by_path.get(&file.relative_path),
                false,
            )?;
            sync_netease_source_metadata(
                tx,
                &asset_id,
                file.provider_id.as_deref(),
                file.source_payload.as_ref(),
            )?;
            updated_assets += 1;
            insert_event(
                tx,
                &repo.summary,
                &asset_id,
                "asset.scanned",
                &file.relative_path,
                serde_json::json!({
                    "sizeBytes": file.size_bytes,
                    "modifiedAt": file.modified_at
                }),
            )?;
            created_events += 1;
        } else {
            let asset_id = asset_id_for_path(&repo.summary.repo_id, &file.relative_path);
            let content_hash = if file.is_virtual {
                String::new()
            } else {
                file_sha256_hash(file.absolute_path.as_deref().ok_or_else(|| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "missing absolute path for non-virtual file",
                    )))
                })?)
                .map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        error,
                    )))
                })?
            };
            tx.execute(
                r#"
                INSERT INTO assets (
                  asset_id, repo_id, path, filename, extension, size_bytes,
                  created_at, modified_at, hash, status, version, updated_at, thumbnail_path,
                  is_virtual, provider_id, provider_item_id, source_payload_json, local_absolute_path
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'synced', 1, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
                "#,
                params![
                    asset_id,
                    repo.summary.repo_id,
                    file.relative_path,
                    file.filename,
                    file.extension,
                    file.size_bytes,
                    now,
                    file.modified_at,
                    content_hash,
                    now,
                    Option::<String>::None,
                    if file.is_virtual { 1 } else { 0 },
                    file.provider_id,
                    file.provider_item_id,
                    file.source_payload.as_ref().map(|value| value.to_string()),
                    file.local_absolute_path
                ],
            )?;
            if !file.is_virtual && !skip_hardlink_candidate_paths.contains(&file.relative_path) {
                record_hardlink_candidate_for_new_asset(
                    tx,
                    &repo.summary.repo_id,
                    &asset_id,
                    &file.relative_path,
                    &content_hash,
                    file.size_bytes,
                )?;
            }
            let palette = if file.is_virtual {
                Vec::new()
            } else {
                extract_image_palette(
                    file.absolute_path.as_deref().ok_or_else(|| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                            std::io::ErrorKind::Other,
                            "missing absolute path for non-virtual file",
                        )))
                    })?,
                    &file.extension,
                )
            };
            insert_default_metadata(
                tx,
                &asset_id,
                &file.relative_path,
                &file.filename,
                &file.extension,
                &now,
                file.created_at.as_deref(),
                &palette,
                plugin_defaults_by_path.get(&file.relative_path),
            )?;
            sync_netease_source_metadata(
                tx,
                &asset_id,
                file.provider_id.as_deref(),
                file.source_payload.as_ref(),
            )?;
            insert_event(
                tx,
                &repo.summary,
                &asset_id,
                "asset.created",
                &file.relative_path,
                serde_json::json!({
                    "origin": "scan"
                }),
            )?;
            created_assets += 1;
            created_events += 1;
        }
    }

    for (path, record) in existing_by_path {
        if record.status == "deleted" {
            continue;
        }
        tx.execute(
            r#"
            UPDATE assets
            SET status = 'deleted', updated_at = ?3
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo.summary.repo_id, record.asset_id, now],
        )?;
        mark_hardlink_member_missing(tx, &repo.summary.repo_id, &record.asset_id)?;
        insert_event(
            tx,
            &repo.summary,
            &record.asset_id,
            "asset.deleted",
            &path,
            serde_json::json!({
                "origin": "scan"
            }),
        )?;
        deleted_assets += 1;
        created_events += 1;
    }

    let hardlink_candidates =
        count_pending_hardlink_candidates(tx, &repo.summary.repo_id).unwrap_or(0);
    replace_directory_records(tx, &repo.summary.repo_id, &directory_records)?;
    rebuild_netease_directory_cache(tx, &repo.summary.repo_id, &files)?;

    Ok(SyncResult {
        repo_id: repo.summary.repo_id.clone(),
        scanned_files: files.len() as i64,
        created_assets,
        updated_assets,
        deleted_assets,
        created_events,
        hardlink_candidates,
    })
}

pub(super) fn apply_revision_state(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    target: &serde_json::Value,
    operation: &str,
    source: &str,
) -> Result<(), rusqlite::Error> {
    let target_map = target
        .as_object()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<BTreeMap<_, _>>();
    let before = load_metadata_map_from_transaction(tx, asset_id)?;
    let now = now_rfc3339();

    tx.execute("DELETE FROM metadata WHERE asset_id = ?1", [asset_id])?;
    for (key, value) in &target_map {
        tx.execute(
            r#"
            INSERT INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
            VALUES (?1, ?2, ?3, ?4, 1, ?5)
            "#,
            params![
                asset_id,
                key,
                infer_value_type(value),
                value.to_string(),
                now
            ],
        )?;
    }

    let next_version: i64 = tx.query_row(
        "SELECT version + 1 FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
        params![repo_id, asset_id],
        |row| row.get(0),
    )?;

    tx.execute(
        "UPDATE assets SET version = ?3, updated_at = ?4, modified_at = ?4 WHERE repo_id = ?1 AND asset_id = ?2",
        params![repo_id, asset_id, next_version, now],
    )?;
    tx.execute(
        r#"
        INSERT INTO revisions (
          revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
        params![
            format!("rev-{}-{}", asset_id, next_version),
            repo_id,
            asset_id,
            now,
            operation,
            serde_json::to_string(&before)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            serde_json::to_string(&target_map)
                .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
            source
        ],
    )?;

    Ok(())
}

pub(super) fn sync_netease_source_metadata(
    connection: &Connection,
    asset_id: &str,
    provider_id: Option<&str>,
    source_payload: Option<&serde_json::Value>,
) -> Result<(), rusqlite::Error> {
    if provider_id != Some(NETEASE_CLOUD_MUSIC_PROVIDER_ID) {
        return Ok(());
    }
    let Some(source_payload) = source_payload else {
        return Ok(());
    };
    for (key, value) in netease_source_metadata_patch(source_payload) {
        upsert_metadata_value(connection, asset_id, &key, &value)?;
    }
    Ok(())
}

fn netease_source_metadata_patch(
    source_payload: &serde_json::Value,
) -> BTreeMap<String, serde_json::Value> {
    const KEYS: &[&str] = &[
        "songId",
        "songName",
        "artists",
        "albumName",
        "coverUrl",
        "durationMs",
        "playlistId",
        "playlistName",
        "playlistCategory",
        "provider",
        "accountId",
    ];
    KEYS.iter()
        .filter_map(|key| {
            source_payload
                .get(*key)
                .cloned()
                .map(|value| ((*key).to_string(), value))
        })
        .collect()
}

fn rebuild_netease_directory_cache(
    connection: &Connection,
    repo_id: &str,
    files: &[DiscoveredFile],
) -> Result<(), rusqlite::Error> {
    clear_netease_directory_cache(connection, repo_id)?;
    let mut groups = BTreeMap::<String, Vec<FileSystemEntry>>::new();
    for file in files {
        if file.provider_id.as_deref() != Some(NETEASE_CLOUD_MUSIC_PROVIDER_ID) {
            continue;
        }
        let directory_path = parent_relative_path(&file.relative_path);
        if directory_path.is_empty() {
            continue;
        }
        groups.entry(directory_path).or_default().push(FileSystemEntry {
            path: file.relative_path.clone(),
            name: file.filename.clone(),
            kind: FileSystemEntryKind::File,
            extension: Some(file.extension.clone()),
            size_bytes: Some(file.size_bytes),
            modified_at: Some(file.modified_at.clone()),
            is_virtual: file.is_virtual,
            provider_id: file.provider_id.clone(),
            provider_item_id: file.provider_item_id.clone(),
            source_payload: file.source_payload.clone(),
            local_absolute_path: file.local_absolute_path.clone(),
        });
    }
    let refreshed_at = now_rfc3339();
    for (directory_path, entries) in groups {
        replace_netease_directory_cache_page(
            connection,
            repo_id,
            &directory_path,
            0,
            &entries,
            entries.len(),
            &refreshed_at,
        )?;
    }
    Ok(())
}

pub(super) fn load_latest_revision(
    tx: &Transaction<'_>,
    asset_id: &str,
) -> Result<Option<RevisionEntry>, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT revision_id, asset_id, timestamp, operation, before_json, after_json, source
        FROM revisions
        WHERE asset_id = ?1
        ORDER BY timestamp DESC
        LIMIT 1
        "#,
        [asset_id],
        |row| {
            let before_json: Option<String> = row.get(4)?;
            let after_json: Option<String> = row.get(5)?;
            Ok(RevisionEntry {
                revision_id: row.get(0)?,
                asset_id: row.get(1)?,
                timestamp: row.get(2)?,
                operation: row.get(3)?,
                before: parse_json_column_optional(before_json)?,
                after: parse_json_column_optional(after_json)?,
                source: row.get(6)?,
            })
        },
    )
    .optional()
}

pub(super) fn insert_event(
    tx: &Transaction<'_>,
    repo: &RepositorySummary,
    asset_id: &str,
    event_type: &str,
    path: &str,
    payload: serde_json::Value,
) -> Result<(), rusqlite::Error> {
    let event_id = format!(
        "evt-{}-{}",
        event_type.replace('.', "-"),
        slugify_repo_id(asset_id, path)
    );
    tx.execute(
        r#"
        INSERT OR REPLACE INTO events (event_id, repo_id, asset_id, event_type, path, payload_json, created_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        "#,
        params![
            event_id,
            repo.repo_id,
            asset_id,
            event_type,
            path,
            payload.to_string(),
            now_rfc3339()
        ],
    )?;
    Ok(())
}

pub(super) fn insert_default_metadata(
    tx: &Transaction<'_>,
    asset_id: &str,
    relative_path: &str,
    filename: &str,
    extension: &str,
    added_to_library_at: &str,
    file_created_at: Option<&str>,
    palette: &[String],
    plugin_defaults: Option<&BTreeMap<String, serde_json::Value>>,
) -> Result<(), rusqlite::Error> {
    ensure_default_metadata(
        tx,
        asset_id,
        relative_path,
        filename,
        extension,
        added_to_library_at,
        file_created_at,
        palette,
        plugin_defaults,
        true,
    )
}

pub(super) fn ensure_default_metadata(
    tx: &Transaction<'_>,
    asset_id: &str,
    _relative_path: &str,
    filename: &str,
    extension: &str,
    added_to_library_at: &str,
    file_created_at: Option<&str>,
    palette: &[String],
    plugin_defaults: Option<&BTreeMap<String, serde_json::Value>>,
    overwrite_existing: bool,
) -> Result<(), rusqlite::Error> {
    let mut defaults = vec![
        (
            "title".to_string(),
            serde_json::Value::String(filename.to_string()),
        ),
        ("favorite".to_string(), serde_json::Value::Bool(false)),
        (
            "type".to_string(),
            serde_json::Value::String(extension.to_string()),
        ),
        ("rating".to_string(), serde_json::json!(0)),
        (
            "comment".to_string(),
            serde_json::Value::String(String::new()),
        ),
        ("link".to_string(), serde_json::Value::String(String::new())),
        ("tagGroups".to_string(), serde_json::json!([])),
        (
            "addedToLibraryAt".to_string(),
            serde_json::Value::String(added_to_library_at.to_string()),
        ),
    ];
    if let Some(file_created_at) = file_created_at {
        defaults.push((
            "fileCreatedAt".to_string(),
            serde_json::Value::String(file_created_at.to_string()),
        ));
    }
    if let Some(primary_color) = palette.first() {
        defaults.push((
            "color".to_string(),
            serde_json::Value::String(primary_color.clone()),
        ));
        defaults.push(("palette".to_string(), serde_json::json!(palette)));
    }
    for (key, value) in defaults {
        if overwrite_existing {
            tx.execute(
                r#"
                INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![
                    asset_id,
                    key,
                    infer_value_type(&value),
                    value.to_string(),
                    added_to_library_at
                ],
            )?;
        } else {
            tx.execute(
                r#"
                INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![
                    asset_id,
                    key,
                    infer_value_type(&value),
                    value.to_string(),
                    added_to_library_at
                ],
            )?;
        }
    }
    if let Some(plugin_defaults) = plugin_defaults {
        for (key, value) in plugin_defaults {
            tx.execute(
                r#"
                INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![
                    asset_id,
                    key,
                    infer_value_type(value),
                    value.to_string(),
                    added_to_library_at
                ],
            )?;
        }
    }

    Ok(())
}

pub(super) fn upsert_metadata_value(
    connection: &Connection,
    asset_id: &str,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), rusqlite::Error> {
    let now = now_rfc3339();
    connection.execute(
        r#"
        INSERT INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        VALUES (?1, ?2, ?3, ?4, 1, ?5)
        ON CONFLICT(asset_id, key)
        DO UPDATE SET
          value_type = excluded.value_type,
          value_json = excluded.value_json,
          version = metadata.version + 1,
          updated_at = excluded.updated_at
        "#,
        params![
            asset_id,
            key,
            infer_value_type(value),
            value.to_string(),
            now
        ],
    )?;
    Ok(())
}

pub(super) fn delete_metadata_value(
    connection: &Connection,
    asset_id: &str,
    key: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM metadata WHERE asset_id = ?1 AND key = ?2",
        params![asset_id, key],
    )?;
    Ok(())
}

pub(super) fn sync_thumbnail_palette_metadata(
    connection: &Connection,
    asset_id: &str,
    thumbnail_path: Option<&str>,
) -> Result<(), rusqlite::Error> {
    let Some(thumbnail_path) = thumbnail_path else {
        return delete_metadata_value(connection, asset_id, "thumbnailPalette");
    };

    let colors = extract_thumbnail_palette(Path::new(thumbnail_path)).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;
    if colors.is_empty() {
        return delete_metadata_value(connection, asset_id, "thumbnailPalette");
    }

    upsert_metadata_value(
        connection,
        asset_id,
        "thumbnailPalette",
        &serde_json::json!(colors),
    )
}

pub(super) fn extract_thumbnail_palette(path: &Path) -> Result<Vec<String>, String> {
    let image = image::open(path).map_err(|error| format!("thumbnail palette error: {error}"))?;
    let thumbnail = image.thumbnail(48, 48).to_rgb8();
    if thumbnail.width() == 0 || thumbnail.height() == 0 {
        return Ok(Vec::new());
    }

    let mut buckets = HashMap::<u16, (u64, u64, u64, usize)>::new();
    for pixel in thumbnail.pixels() {
        let [r, g, b] = pixel.0;
        let key = (((r as u16) >> 4) << 8) | (((g as u16) >> 4) << 4) | ((b as u16) >> 4);
        let entry = buckets.entry(key).or_insert((0, 0, 0, 0));
        entry.0 += r as u64;
        entry.1 += g as u64;
        entry.2 += b as u64;
        entry.3 += 1;
    }

    let mut ranked = buckets
        .into_values()
        .filter(|(_, _, _, count)| *count > 0)
        .map(|(r, g, b, count)| {
            (
                count,
                (r / count as u64) as u8,
                (g / count as u64) as u8,
                (b / count as u64) as u8,
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0));

    let mut palette = Vec::new();
    for (_, r, g, b) in ranked {
        if palette
            .iter()
            .any(|(pr, pg, pb)| color_distance_sq((r, g, b), (*pr, *pg, *pb)) < 720)
        {
            continue;
        }
        palette.push((r, g, b));
        if palette.len() == 5 {
            break;
        }
    }

    if palette.is_empty() {
        let mut totals = (0_u64, 0_u64, 0_u64, 0_u64);
        for pixel in thumbnail.pixels() {
            let [r, g, b] = pixel.0;
            totals.0 += r as u64;
            totals.1 += g as u64;
            totals.2 += b as u64;
            totals.3 += 1;
        }
        if totals.3 > 0 {
            palette.push((
                (totals.0 / totals.3) as u8,
                (totals.1 / totals.3) as u8,
                (totals.2 / totals.3) as u8,
            ));
        }
    }

    Ok(palette
        .into_iter()
        .map(|(r, g, b)| format!("#{r:02X}{g:02X}{b:02X}"))
        .collect())
}

pub(super) fn color_distance_sq(left: (u8, u8, u8), right: (u8, u8, u8)) -> i32 {
    let dr = left.0 as i32 - right.0 as i32;
    let dg = left.1 as i32 - right.1 as i32;
    let db = left.2 as i32 - right.2 as i32;
    dr * dr + dg * dg + db * db
}

pub(super) fn hardlink_group_id_for(repo_id: &str, content_hash: &str, size_bytes: i64) -> String {
    format!(
        "hardlink-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            content_hash.as_bytes(),
            size_bytes.to_string().as_bytes()
        ])
    )
}

pub(super) fn hardlink_candidate_id_for(
    repo_id: &str,
    new_asset_id: &str,
    existing_asset_id: &str,
) -> String {
    format!(
        "hardlink-candidate-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            new_asset_id.as_bytes(),
            existing_asset_id.as_bytes()
        ])
    )
}

pub(super) fn ensure_hardlink_group(
    tx: &Transaction<'_>,
    repo_id: &str,
    content_hash: &str,
    size_bytes: i64,
) -> Result<String, rusqlite::Error> {
    let group_id = hardlink_group_id_for(repo_id, content_hash, size_bytes);
    let now = now_rfc3339();
    tx.execute(
        r#"
        INSERT INTO hardlink_groups (group_id, repo_id, content_hash, size_bytes, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?5)
        ON CONFLICT(repo_id, content_hash, size_bytes)
        DO UPDATE SET updated_at = excluded.updated_at
        "#,
        params![group_id, repo_id, content_hash, size_bytes, now],
    )?;
    tx.query_row(
        r#"
        SELECT group_id
        FROM hardlink_groups
        WHERE repo_id = ?1 AND content_hash = ?2 AND size_bytes = ?3
        "#,
        params![repo_id, content_hash, size_bytes],
        |row| row.get(0),
    )
}

pub(super) fn upsert_hardlink_member(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    path: &str,
    content_hash: &str,
    size_bytes: i64,
    link_state: &str,
) -> Result<(), rusqlite::Error> {
    let group_id = ensure_hardlink_group(tx, repo_id, content_hash, size_bytes)?;
    let now = now_rfc3339();
    tx.execute(
        r#"
        INSERT INTO hardlink_members (group_id, repo_id, asset_id, path, link_state, linked_at, verified_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
        ON CONFLICT(repo_id, asset_id)
        DO UPDATE SET
          group_id = excluded.group_id,
          path = excluded.path,
          link_state = excluded.link_state,
          verified_at = excluded.verified_at
        "#,
        params![group_id, repo_id, asset_id, path, link_state, now],
    )?;
    Ok(())
}

pub(super) fn update_hardlink_member_verification(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    path: &str,
    content_hash: &str,
) -> Result<(), rusqlite::Error> {
    let Some((group_id, expected_hash, current_state)) = tx
        .query_row(
            r#"
            SELECT hm.group_id, hg.content_hash, hm.link_state
            FROM hardlink_members hm
            JOIN hardlink_groups hg ON hg.group_id = hm.group_id
            WHERE hm.repo_id = ?1 AND hm.asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(());
    };
    let state = if expected_hash != content_hash {
        "broken"
    } else if current_state == "copiedFallback" {
        "copiedFallback"
    } else {
        "linked"
    };
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET path = ?4, link_state = ?5, verified_at = ?6
        WHERE repo_id = ?1 AND asset_id = ?2 AND group_id = ?3
        "#,
        params![repo_id, asset_id, group_id, path, state, now_rfc3339()],
    )?;
    Ok(())
}

pub(super) fn mark_hardlink_member_missing(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET link_state = 'missing', verified_at = ?3
        WHERE repo_id = ?1 AND asset_id = ?2
        "#,
        params![repo_id, asset_id, now_rfc3339()],
    )?;
    Ok(())
}

pub(super) fn record_hardlink_candidate_for_new_asset(
    tx: &Transaction<'_>,
    repo_id: &str,
    new_asset_id: &str,
    new_path: &str,
    content_hash: &str,
    size_bytes: i64,
) -> Result<(), rusqlite::Error> {
    let existing = tx
        .query_row(
            r#"
            SELECT asset_id, path
            FROM assets
            WHERE repo_id = ?1
              AND asset_id != ?2
              AND hash = ?3
              AND size_bytes = ?4
              AND status != 'deleted'
            ORDER BY updated_at DESC
            LIMIT 1
            "#,
            params![repo_id, new_asset_id, content_hash, size_bytes],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((existing_asset_id, existing_path)) = existing else {
        return Ok(());
    };
    let candidate_id = hardlink_candidate_id_for(repo_id, new_asset_id, &existing_asset_id);
    tx.execute(
        r#"
        INSERT OR IGNORE INTO hardlink_candidates (
          candidate_id, repo_id, new_asset_id, new_path, existing_asset_id, existing_path,
          content_hash, size_bytes, created_at
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
        "#,
        params![
            candidate_id,
            repo_id,
            new_asset_id,
            new_path,
            existing_asset_id,
            existing_path,
            content_hash,
            size_bytes,
            now_rfc3339()
        ],
    )?;
    Ok(())
}

pub(super) fn count_pending_hardlink_candidates(
    tx: &Transaction<'_>,
    repo_id: &str,
) -> Result<i64, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT COUNT(*)
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1
        "#,
        [repo_id],
        |row| row.get(0),
    )
}

pub(super) fn load_hardlink_asset_for_path(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<Option<HardlinkAssetRecord>, rusqlite::Error> {
    let record = tx
        .query_row(
            r#"
            SELECT asset_id, hash, size_bytes
            FROM assets
            WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'
            "#,
            params![repo_id, path],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(record.and_then(|(asset_id, hash, size_bytes)| {
        hash.filter(|value| is_content_hash(value))
            .map(|content_hash| HardlinkAssetRecord {
                asset_id,
                content_hash,
                size_bytes,
            })
    }))
}

pub(super) fn hardlink_outcome_target_paths(outcomes: &[HardlinkCopyOutcome]) -> HashSet<String> {
    outcomes
        .iter()
        .map(|outcome| outcome.target_path.clone())
        .collect()
}

pub(super) fn load_hardlink_candidates(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<HardlinkCandidate>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT hc.candidate_id, hc.repo_id, hc.new_asset_id, hc.new_path,
               hc.existing_asset_id, hc.existing_path, hc.content_hash,
               hc.size_bytes, hc.created_at
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1
        ORDER BY hc.created_at ASC
        "#,
    )?;
    let rows = stmt.query_map([repo_id], map_hardlink_candidate_row)?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_hardlink_candidate_from_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    candidate_id: &str,
) -> Result<Option<HardlinkCandidate>, rusqlite::Error> {
    tx.query_row(
        r#"
        SELECT hc.candidate_id, hc.repo_id, hc.new_asset_id, hc.new_path,
               hc.existing_asset_id, hc.existing_path, hc.content_hash,
               hc.size_bytes, hc.created_at
        FROM hardlink_candidates hc
        JOIN assets new_asset
          ON new_asset.repo_id = hc.repo_id
         AND new_asset.asset_id = hc.new_asset_id
         AND new_asset.path = hc.new_path
         AND new_asset.hash = hc.content_hash
         AND new_asset.size_bytes = hc.size_bytes
         AND new_asset.status != 'deleted'
        JOIN assets existing_asset
          ON existing_asset.repo_id = hc.repo_id
         AND existing_asset.asset_id = hc.existing_asset_id
         AND existing_asset.path = hc.existing_path
         AND existing_asset.hash = hc.content_hash
         AND existing_asset.size_bytes = hc.size_bytes
         AND existing_asset.status != 'deleted'
        WHERE hc.repo_id = ?1 AND hc.candidate_id = ?2
        "#,
        params![repo_id, candidate_id],
        map_hardlink_candidate_row,
    )
    .optional()
}

pub(super) fn delete_hardlink_candidate(
    tx: &Transaction<'_>,
    repo_id: &str,
    candidate_id: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        "DELETE FROM hardlink_candidates WHERE repo_id = ?1 AND candidate_id = ?2",
        params![repo_id, candidate_id],
    )?;
    Ok(())
}

pub(super) fn map_hardlink_candidate_row(
    row: &rusqlite::Row<'_>,
) -> Result<HardlinkCandidate, rusqlite::Error> {
    let size_bytes = row.get::<_, i64>(7)?;
    Ok(HardlinkCandidate {
        candidate_id: row.get(0)?,
        repo_id: row.get(1)?,
        new_asset_id: row.get(2)?,
        new_path: row.get(3)?,
        existing_asset_id: row.get(4)?,
        existing_path: row.get(5)?,
        content_hash: row.get(6)?,
        size_bytes,
        size_label: format_size_label(size_bytes),
        created_at: row.get(8)?,
    })
}
