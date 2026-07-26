//! Repository full-scan synchronization and shared asset upsert helpers.

use super::*;

pub(super) fn build_directory_records_from_tree(
    repo_root: &Path,
    tree: &[FileTreeNode],
    files: &[DiscoveredFile],
) -> Result<Vec<DirectoryRecord>, String> {
    let mut directories = BTreeMap::<String, DirectoryRecord>::new();
    insert_directory_record(repo_root, "", &mut directories)?;
    for node in tree {
        collect_directory_records_from_tree(repo_root, node, &mut directories)?;
    }
    supplement_directory_records_from_files(&mut directories, files);
    directory_records_from_map(directories)
}

pub(super) fn supplement_directory_records_from_files(
    directories: &mut BTreeMap<String, DirectoryRecord>,
    files: &[DiscoveredFile],
) {
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
}

pub(super) fn directory_records_from_map(
    directories: BTreeMap<String, DirectoryRecord>,
) -> Result<Vec<DirectoryRecord>, String> {
    let mut values = directories.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(values)
}

fn collect_directory_records_from_tree(
    repo_root: &Path,
    node: &FileTreeNode,
    directories: &mut BTreeMap<String, DirectoryRecord>,
) -> Result<(), String> {
    let path = normalize_repository_relative_path(&node.path);
    if !path.is_empty() {
        insert_directory_record(repo_root, &path, directories)?;
    }
    for child in &node.children {
        collect_directory_records_from_tree(repo_root, child, directories)?;
    }
    Ok(())
}

pub(super) fn insert_directory_record(
    repo_root: &Path,
    raw_path: &str,
    directories: &mut BTreeMap<String, DirectoryRecord>,
) -> Result<(), String> {
    let path = normalize_repository_relative_path(raw_path);
    let parent_path = parent_relative_path(&path);
    let name = directory_record_name(&path);
    let updated_at = directory_updated_at(repo_root, &path)?;
    directories.insert(
        path.clone(),
        DirectoryRecord {
            path,
            parent_path,
            name,
            updated_at,
        },
    );
    Ok(())
}

pub(super) fn normalize_repository_relative_path(path: &str) -> String {
    path.trim()
        .replace('\\', "/")
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

fn directory_record_name(path: &str) -> String {
    if path.is_empty() {
        String::new()
    } else {
        Path::new(path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

fn directory_updated_at(repo_root: &Path, path: &str) -> Result<String, String> {
    let directory_path = if path.is_empty() {
        repo_root.to_path_buf()
    } else {
        repo_root.join(Path::new(path))
    };
    let updated_at = fs::metadata(directory_path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .map(system_time_to_rfc3339)
        .transpose()
        .map_err(time_error)?
        .unwrap_or_else(now_rfc3339);
    Ok(updated_at)
}

pub(super) fn metadata_defaults_for_files(
    service_root: &Path,
    files: &[DiscoveredFile],
    existing_metadata_by_path: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, String> {
    if files.is_empty() {
        return Ok(BTreeMap::new());
    }

    let registry = plugin_catalog(service_root);
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
    let known_paths = files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let payload = serde_json::json!({ "entries": entries });
    let mut defaults_by_path = BTreeMap::<String, BTreeMap<String, serde_json::Value>>::new();

    for (plugin_id, action) in providers {
        let response = registry.call(&plugin_id, &action, payload.clone())?;
        let parsed = serde_json::from_value::<MetadataDefaultsBatchResponse>(response)
            .map_err(json_error)?;
        for (path, defaults) in parsed.defaults_by_path {
            if !known_paths.contains(&path) {
                continue;
            }
            defaults_by_path.entry(path).or_default().extend(defaults);
        }
    }

    Ok(defaults_by_path)
}

#[derive(Debug, Default)]
pub(super) struct AssetSyncApplyResult {
    pub(super) created_assets: i64,
    pub(super) updated_assets: i64,
    pub(super) created_events: i64,
}

pub(super) fn sync_sql_error(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        message.into(),
    )))
}

fn sync_file_sample_paths(files: &[DiscoveredFile]) -> Vec<String> {
    files
        .iter()
        .take(12)
        .map(|file| file.relative_path.clone())
        .collect()
}

fn write_sync_log(
    repo_id: &str,
    level: &str,
    action: &str,
    message: &str,
    context: serde_json::Value,
) {
    let _ = crate::services::logging::write_log(SystemLogWriteRequest {
        level: level.to_string(),
        category: "repository.sync".to_string(),
        action: action.to_string(),
        message: message.to_string(),
        context: Some(context),
        repo_id: Some(repo_id.to_string()),
        plugin_id: None,
        source_kind: Some("host".to_string()),
        source_label: Some("MomoBako".to_string()),
        location: Some(SystemLogLocationInput {
            module_path: Some(module_path!().to_string()),
            file: Some(file!().to_string()),
            line: Some(line!()),
        }),
    });
}

pub(super) fn load_existing_asset_records(
    tx: &Transaction<'_>,
    repo_id: &str,
) -> Result<Vec<(String, String, ExistingAssetRecord)>, rusqlite::Error> {
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
    let existing_rows = existing_stmt.query_map([repo_id], |row| {
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
    existing_rows.collect::<Result<Vec<_>, _>>()
}

fn discovered_file_absolute_path(file: &DiscoveredFile) -> Result<&Path, rusqlite::Error> {
    file.absolute_path
        .as_deref()
        .ok_or_else(|| sync_sql_error("missing absolute path for non-virtual file"))
}

fn discovered_file_content_hash(
    file: &DiscoveredFile,
    existing_record: Option<&ExistingAssetRecord>,
) -> Result<String, rusqlite::Error> {
    if file.is_virtual {
        return Ok(existing_record
            .and_then(|record| record.hash.clone())
            .unwrap_or_default());
    }

    if let Some(record) = existing_record {
        if record.size_bytes == file.size_bytes && record.modified_at == file.modified_at {
            if let Some(hash) = record.hash.as_ref().filter(|hash| is_content_hash(hash)) {
                return Ok(hash.clone());
            }
        }
    }

    file_sha256_hash(discovered_file_absolute_path(file)?).map_err(sync_sql_error)
}

fn merged_discovered_source_payload(
    file: &DiscoveredFile,
    existing_record: Option<&ExistingAssetRecord>,
) -> Option<serde_json::Value> {
    let fallback = file
        .source_payload
        .clone()
        .or_else(|| existing_record.and_then(|record| record.source_payload.clone()));
    let Some(shared_asset_id) = file.shared_asset_id.as_deref() else {
        return fallback;
    };
    let mut map = fallback
        .as_ref()
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    map.insert(
        "sharedAssetId".to_string(),
        serde_json::Value::String(shared_asset_id.to_string()),
    );
    Some(serde_json::Value::Object(map))
}

fn replace_discovered_asset_tags(
    connection: &Transaction<'_>,
    asset_id: &str,
    tags: Option<&[String]>,
) -> Result<(), rusqlite::Error> {
    let Some(tags) = tags else {
        return Ok(());
    };
    replace_asset_tags(connection, asset_id, tags)
}

fn sync_discovered_entry_thumbnail(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
    relative_path: &str,
    thumbnail_local_absolute_path: Option<&str>,
    managed_by_source: bool,
) -> Result<(), rusqlite::Error> {
    if let Some(thumbnail_path) = thumbnail_local_absolute_path {
        upsert_entry_thumbnail_record(
            connection,
            repo_id,
            relative_path,
            "file",
            thumbnail_path,
            false,
        )?;
        update_asset_thumbnail_path(connection, repo_id, asset_id, Some(thumbnail_path))?;
        return Ok(());
    }
    if managed_by_source {
        remove_entry_thumbnail_record(connection, repo_id, relative_path, "file")?;
        update_asset_thumbnail_path(connection, repo_id, asset_id, None)?;
    }
    Ok(())
}

fn clear_source_shared_asset_relations(
    tx: &Transaction<'_>,
    repo_id: &str,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        "DELETE FROM hardlink_candidates WHERE repo_id = ?1",
        [repo_id],
    )?;
    tx.execute("DELETE FROM hardlink_members WHERE repo_id = ?1", [repo_id])?;
    tx.execute("DELETE FROM hardlink_groups WHERE repo_id = ?1", [repo_id])?;
    tx.execute(
        "DELETE FROM asset_alias_members WHERE repo_id = ?1",
        [repo_id],
    )?;
    tx.execute(
        "DELETE FROM asset_alias_groups WHERE repo_id = ?1",
        [repo_id],
    )?;
    Ok(())
}

fn rebuild_source_shared_asset_relations(
    tx: &Transaction<'_>,
    repo_id: &str,
    files: &[DiscoveredFile],
    now: &str,
) -> Result<(), rusqlite::Error> {
    let mut groups = BTreeMap::<String, Vec<&DiscoveredFile>>::new();
    for file in files {
        let Some(shared_asset_id) = file.shared_asset_id.as_deref() else {
            continue;
        };
        groups
            .entry(shared_asset_id.to_string())
            .or_default()
            .push(file);
    }
    if groups.is_empty() {
        return Ok(());
    }

    clear_source_shared_asset_relations(tx, repo_id)?;
    for (shared_asset_id, members) in groups {
        if members.len() <= 1 {
            continue;
        }
        let alias_group_id = format!(
            "source-alias-{}",
            sha256_hex(&[repo_id.as_bytes(), shared_asset_id.as_bytes()])
        );
        tx.execute(
            r#"
            INSERT INTO asset_alias_groups (alias_group_id, repo_id, source, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![alias_group_id, repo_id, "source-shared-asset", now, now],
        )?;

        for (index, file) in members.iter().enumerate() {
            let asset_id = asset_id_for_path(repo_id, &file.relative_path);
            let role = if index == 0 { "primary" } else { "alias" };
            tx.execute(
                r#"
                INSERT INTO asset_alias_members (alias_group_id, repo_id, asset_id, path, role, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![alias_group_id, repo_id, asset_id, file.relative_path, role, now],
            )?;

            let hash_and_size = tx
                .query_row(
                    r#"
                    SELECT hash, size_bytes
                    FROM assets
                    WHERE repo_id = ?1 AND asset_id = ?2
                    "#,
                    params![repo_id, asset_id],
                    |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
                )
                .optional()?;
            let Some((Some(content_hash), size_bytes)) = hash_and_size else {
                continue;
            };
            if content_hash.is_empty() {
                continue;
            }
            upsert_hardlink_member(
                tx,
                repo_id,
                &asset_id,
                &file.relative_path,
                &content_hash,
                size_bytes,
                if index == 0 { "primary" } else { "linked" },
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn upsert_discovered_asset(
    tx: &Transaction<'_>,
    repo: &RepositoryRecord,
    file: &DiscoveredFile,
    existing_record: Option<ExistingAssetRecord>,
    skip_hardlink_candidate: bool,
    plugin_defaults: Option<&BTreeMap<String, serde_json::Value>>,
    source_metadata_keys: &[String],
    now: &str,
    event_origin: &str,
) -> Result<AssetSyncApplyResult, rusqlite::Error> {
    let mut result = AssetSyncApplyResult::default();
    let status = file.status.as_deref().unwrap_or("synced");
    let source_payload = merged_discovered_source_payload(file, existing_record.as_ref());
    let thumbnail_path = file.thumbnail_local_absolute_path.clone().or_else(|| {
        existing_record
            .as_ref()
            .and_then(|record| record.thumbnail_path.clone())
    });
    let source_manages_thumbnail =
        file.thumbnail_local_absolute_path.is_some() || file.shared_asset_id.is_some();

    if let Some(existing_record) = existing_record {
        let asset_id = existing_record.asset_id.clone();
        let asset_created_at = existing_record.created_at.clone();
        let content_hash = discovered_file_content_hash(file, Some(&existing_record))?;
        tx.execute(
            r#"
            UPDATE assets
            SET filename = ?3, extension = ?4, size_bytes = ?5, modified_at = ?6, hash = ?7,
                status = ?15, updated_at = ?8, thumbnail_path = ?9, is_virtual = ?10,
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
                thumbnail_path,
                if file.is_virtual { 1 } else { 0 },
                file.provider_id,
                file.provider_item_id,
                source_payload.as_ref().map(|value| value.to_string()),
                file.local_absolute_path,
                status,
            ],
        )?;
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
            plugin_defaults,
            false,
        )?;
        sync_mirrored_source_metadata(
            tx,
            &asset_id,
            source_payload.as_ref(),
            source_metadata_keys,
        )?;
        replace_discovered_asset_tags(tx, &asset_id, file.tags.as_deref())?;
        sync_discovered_entry_thumbnail(
            tx,
            &repo.summary.repo_id,
            &asset_id,
            &file.relative_path,
            thumbnail_path.as_deref(),
            source_manages_thumbnail,
        )?;
        insert_event(
            tx,
            &repo.summary,
            &asset_id,
            "asset.scanned",
            &file.relative_path,
            serde_json::json!({
                "sizeBytes": file.size_bytes,
                "modifiedAt": file.modified_at,
                "origin": event_origin
            }),
        )?;
        result.updated_assets = 1;
        result.created_events = 1;
        return Ok(result);
    }

    let asset_id = asset_id_for_path(&repo.summary.repo_id, &file.relative_path);
    let content_hash = discovered_file_content_hash(file, None)?;
    tx.execute(
        r#"
        INSERT INTO assets (
          asset_id, repo_id, path, filename, extension, size_bytes,
          created_at, modified_at, hash, status, version, updated_at, thumbnail_path,
          is_virtual, provider_id, provider_item_id, source_payload_json, local_absolute_path
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
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
            if content_hash.is_empty() {
                None
            } else {
                Some(content_hash.as_str())
            },
            status,
            now,
            thumbnail_path,
            if file.is_virtual { 1 } else { 0 },
            file.provider_id,
            file.provider_item_id,
            source_payload.as_ref().map(|value| value.to_string()),
            file.local_absolute_path
        ],
    )?;
    if !file.is_virtual && !skip_hardlink_candidate {
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
        extract_image_palette(discovered_file_absolute_path(file)?, &file.extension)
    };
    insert_default_metadata(
        tx,
        &asset_id,
        &file.relative_path,
        &file.filename,
        &file.extension,
        now,
        file.created_at.as_deref(),
        &palette,
        plugin_defaults,
    )?;
    sync_mirrored_source_metadata(tx, &asset_id, source_payload.as_ref(), source_metadata_keys)?;
    replace_discovered_asset_tags(tx, &asset_id, file.tags.as_deref())?;
    sync_discovered_entry_thumbnail(
        tx,
        &repo.summary.repo_id,
        &asset_id,
        &file.relative_path,
        thumbnail_path.as_deref(),
        source_manages_thumbnail,
    )?;
    insert_event(
        tx,
        &repo.summary,
        &asset_id,
        "asset.created",
        &file.relative_path,
        serde_json::json!({
            "origin": event_origin
        }),
    )?;
    result.created_assets = 1;
    result.created_events = 1;
    Ok(result)
}

pub(super) fn sync_repository_files(
    service_root: &Path,
    tx: &Transaction<'_>,
    repo: &RepositoryRecord,
    skip_hardlink_candidate_paths: &HashSet<String>,
    hint_paths: &std::collections::BTreeSet<String>,
) -> Result<SyncResult, rusqlite::Error> {
    let repo_root = PathBuf::from(&repo.summary.path);
    write_sync_log(
        &repo.summary.repo_id,
        "info",
        "scanStart",
        "开始扫描资源库文件。",
        serde_json::json!({
            "repoPath": repo.summary.path.as_str(),
            "backendPluginId": repo.backend_record.plugin_id.as_str(),
            "hintPathCount": hint_paths.len(),
        }),
    );
    let files = list_backend_files(service_root, repo, &repo_root).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;
    let files = supplement_discovered_files_with_hint_paths(
        service_root,
        repo,
        &repo_root,
        files,
        hint_paths,
    )
    .map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;
    write_sync_log(
        &repo.summary.repo_id,
        "info",
        "scanFilesDiscovered",
        "资源库文件扫描完成。",
        serde_json::json!({
            "scannedFiles": files.len(),
            "samplePaths": sync_file_sample_paths(&files),
        }),
    );

    let existing = load_existing_asset_records(tx, &repo.summary.repo_id)?;
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
    write_sync_log(
        &repo.summary.repo_id,
        "info",
        "metadataDefaultsStart",
        "开始计算默认元数据。",
        serde_json::json!({
            "scannedFiles": files.len(),
            "existingAssets": existing_metadata_by_path.len(),
        }),
    );
    let plugin_defaults_by_path =
        metadata_defaults_for_files(service_root, &files, &existing_metadata_by_path).map_err(
            |error| {
                rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    error,
                )))
            },
        )?;
    write_sync_log(
        &repo.summary.repo_id,
        "info",
        "metadataDefaultsSuccess",
        "默认元数据计算完成。",
        serde_json::json!({
            "defaultPathCount": plugin_defaults_by_path.len(),
        }),
    );
    let source_metadata_keys =
        source_metadata_mirror_keys(service_root, &repo.backend_record.plugin_id);
    let mut existing_by_path = existing
        .into_iter()
        .map(|(_asset_id, path, record)| (path, record))
        .collect::<BTreeMap<_, _>>();
    let tree = list_backend_tree(service_root, repo, &repo_root).map_err(|error| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            error,
        )))
    })?;
    let directory_records =
        build_directory_records_from_tree(&repo_root, &tree, &files).map_err(|error| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                error,
            )))
        })?;

    let now = now_rfc3339();
    let mut created_assets = 0_i64;
    let mut updated_assets = 0_i64;
    let mut deleted_assets = 0_i64;
    let mut created_events = 0_i64;
    write_sync_log(
        &repo.summary.repo_id,
        "info",
        "writeIndexStart",
        "开始写入资源索引。",
        serde_json::json!({
            "scannedFiles": files.len(),
            "directoryCount": directory_records.len(),
        }),
    );

    for file in &files {
        let apply_result = upsert_discovered_asset(
            tx,
            repo,
            file,
            existing_by_path.remove(&file.relative_path),
            skip_hardlink_candidate_paths.contains(&file.relative_path),
            plugin_defaults_by_path.get(&file.relative_path),
            &source_metadata_keys,
            &now,
            "scan",
        )?;
        created_assets += apply_result.created_assets;
        updated_assets += apply_result.updated_assets;
        created_events += apply_result.created_events;
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
    rebuild_source_shared_asset_relations(tx, &repo.summary.repo_id, &files, &now)?;
    if let Some(repository_state) =
        describe_backend_repository_state(service_root, repo, &repo_root).map_err(sync_sql_error)?
    {
        replace_source_repository_state(tx, &repo.summary.repo_id, &repository_state)?;
    }
    rebuild_netease_directory_cache(tx, &repo.summary.repo_id, &files)?;
    write_sync_log(
        &repo.summary.repo_id,
        "info",
        "syncComplete",
        "资源库同步写入完成。",
        serde_json::json!({
            "scannedFiles": files.len(),
            "createdAssets": created_assets,
            "updatedAssets": updated_assets,
            "deletedAssets": deleted_assets,
            "createdEvents": created_events,
            "hardlinkCandidates": hardlink_candidates,
            "samplePaths": sync_file_sample_paths(&files),
        }),
    );

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

/// 用 watcher 捕获到的变更路径补齐索引检索的短暂漏报，保证新增文件能立即入库。
pub(super) fn supplement_discovered_files_with_hint_paths(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    mut files: Vec<DiscoveredFile>,
    hint_paths: &std::collections::BTreeSet<String>,
) -> Result<Vec<DiscoveredFile>, String> {
    if hint_paths.is_empty() {
        return Ok(files);
    }

    let mut existing_paths = files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut visited_directories = std::collections::BTreeSet::new();
    for path in hint_paths {
        supplement_hint_path_files(
            service_root,
            repo,
            repo_root,
            path,
            &mut existing_paths,
            &mut visited_directories,
            &mut files,
        )?;
    }
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

/// 将 watcher 提供的路径提示补偿为可入库的文件结果，支持目录递归展开。
pub(super) fn supplement_hint_path_files(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    hint_path: &str,
    existing_paths: &mut std::collections::BTreeSet<String>,
    visited_directories: &mut std::collections::BTreeSet<String>,
    files: &mut Vec<DiscoveredFile>,
) -> Result<(), String> {
    let Ok(entry) = stat_backend_entry(service_root, repo, repo_root, hint_path) else {
        return Ok(());
    };

    match entry.kind {
        FileSystemEntryKind::File => {
            let file = entry.into_discovered_file(repo_root)?;
            if existing_paths.insert(file.relative_path.clone()) {
                files.push(file);
            }
        }
        FileSystemEntryKind::Directory => {
            if !visited_directories.insert(entry.path.clone()) {
                return Ok(());
            }
            let Ok(children) =
                list_backend_directory_entries(service_root, repo, repo_root, &entry.path)
            else {
                return Ok(());
            };
            for child in children {
                supplement_hint_path_files(
                    service_root,
                    repo,
                    repo_root,
                    &child.path,
                    existing_paths,
                    visited_directories,
                    files,
                )?;
            }
        }
    }

    Ok(())
}
