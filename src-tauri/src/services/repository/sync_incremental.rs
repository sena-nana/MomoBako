//! Watcher-driven incremental repository synchronization.

use super::*;

pub(super) fn sync_repository_changed_paths(
    service_root: &Path,
    tx: &Transaction<'_>,
    repo: &RepositoryRecord,
    changed_paths: &std::collections::BTreeSet<String>,
) -> Result<SyncResult, rusqlite::Error> {
    let repo_root = PathBuf::from(&repo.summary.path);
    let mut files = Vec::new();
    let mut existing_paths = std::collections::BTreeSet::new();
    let mut visited_directories = std::collections::BTreeSet::new();
    let mut existing_directory_paths = std::collections::BTreeSet::new();
    let mut missing_paths = std::collections::BTreeSet::new();

    for raw_path in changed_paths {
        let Ok(path) = normalize_entry_path(raw_path) else {
            continue;
        };
        match stat_backend_entry(service_root, repo, &repo_root, &path) {
            Ok(entry) => match entry.kind {
                FileSystemEntryKind::File => {
                    let file = entry
                        .into_discovered_file(&repo_root)
                        .map_err(sync_sql_error)?;
                    if existing_paths.insert(file.relative_path.clone()) {
                        files.push(file);
                    }
                }
                FileSystemEntryKind::Directory => {
                    existing_directory_paths.insert(entry.path.clone());
                    supplement_hint_path_files(
                        service_root,
                        repo,
                        &repo_root,
                        &entry.path,
                        &mut existing_paths,
                        &mut visited_directories,
                        &mut files,
                    )
                    .map_err(sync_sql_error)?;
                }
            },
            Err(_) => {
                missing_paths.insert(path);
            }
        }
    }

    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    let found_paths = files
        .iter()
        .map(|file| file.relative_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
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
    let source_metadata_keys =
        source_metadata_mirror_keys(service_root, &repo.backend_record.plugin_id);
    let plugin_defaults_by_path = metadata_defaults_for_files(
        service_root,
        &files,
        &existing_metadata_by_path,
        &source_metadata_keys,
    )
    .map_err(sync_sql_error)?;
    let mut existing_by_path = existing
        .into_iter()
        .map(|(_asset_id, path, record)| (path, record))
        .collect::<BTreeMap<_, _>>();

    let now = now_rfc3339();
    let mut created_assets = 0_i64;
    let mut updated_assets = 0_i64;
    let mut deleted_assets = 0_i64;
    let mut created_events = 0_i64;

    for file in &files {
        let apply_result = upsert_discovered_asset(
            tx,
            repo,
            file,
            existing_by_path.remove(&file.relative_path),
            false,
            plugin_defaults_by_path.get(&file.relative_path),
            &source_metadata_keys,
            &now,
            "watcher",
        )?;
        created_assets += apply_result.created_assets;
        updated_assets += apply_result.updated_assets;
        created_events += apply_result.created_events;
    }

    for path in &missing_paths {
        if found_paths
            .iter()
            .any(|found_path| path_matches_changed_root(found_path, path))
        {
            continue;
        }
        let deletion = mark_changed_path_assets_deleted(tx, &repo.summary, path, &now, "watcher")?;
        deleted_assets += deletion.deleted_assets;
        created_events += deletion.created_events;
        delete_changed_directory_records(tx, &repo.summary.repo_id, path)?;
    }

    upsert_changed_directory_records(
        tx,
        &repo_root,
        &repo.summary.repo_id,
        &files,
        &existing_directory_paths,
        &missing_paths,
    )?;

    let hardlink_candidates =
        count_pending_hardlink_candidates(tx, &repo.summary.repo_id).unwrap_or(0);

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

fn path_matches_changed_root(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|remaining| remaining.starts_with('/'))
}

#[derive(Debug, Default)]
struct AssetDeletionResult {
    deleted_assets: i64,
    created_events: i64,
}

fn active_assets_for_missing_path(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id, path
        FROM assets
        WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'
        "#,
    )?;
    let exact_rows = stmt.query_map(params![repo_id, path], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let exact = exact_rows.collect::<Result<Vec<_>, _>>()?;
    if !exact.is_empty() {
        return Ok(exact);
    }

    let prefix = format!("{path}/%");
    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id, path
        FROM assets
        WHERE repo_id = ?1 AND path LIKE ?2 AND status != 'deleted'
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, prefix], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

fn mark_changed_path_assets_deleted(
    tx: &Transaction<'_>,
    repo: &RepositorySummary,
    path: &str,
    now: &str,
    event_origin: &str,
) -> Result<AssetDeletionResult, rusqlite::Error> {
    let assets = active_assets_for_missing_path(tx, &repo.repo_id, path)?;
    let mut result = AssetDeletionResult::default();
    for (asset_id, asset_path) in assets {
        tx.execute(
            r#"
            UPDATE assets
            SET status = 'deleted', updated_at = ?3
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo.repo_id, asset_id, now],
        )?;
        mark_hardlink_member_missing(tx, &repo.repo_id, &asset_id)?;
        insert_event(
            tx,
            repo,
            &asset_id,
            "asset.deleted",
            &asset_path,
            serde_json::json!({
                "origin": event_origin
            }),
        )?;
        result.deleted_assets += 1;
        result.created_events += 1;
    }
    Ok(result)
}

fn delete_changed_directory_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    let prefix = format!("{path}/%");
    tx.execute(
        "DELETE FROM directories WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)",
        params![repo_id, path, prefix],
    )?;
    Ok(())
}

fn collect_directory_records_for_path(
    repo_root: &Path,
    path: &str,
    include_self: bool,
    directories: &mut BTreeMap<String, DirectoryRecord>,
) -> Result<(), String> {
    let mut current = if include_self {
        normalize_repository_relative_path(path)
    } else {
        parent_relative_path(&normalize_repository_relative_path(path))
    };
    loop {
        insert_directory_record(repo_root, &current, directories)?;
        if current.is_empty() {
            break;
        }
        current = parent_relative_path(&current);
    }
    Ok(())
}

fn upsert_changed_directory_records(
    tx: &Transaction<'_>,
    repo_root: &Path,
    repo_id: &str,
    files: &[DiscoveredFile],
    existing_directory_paths: &std::collections::BTreeSet<String>,
    missing_paths: &std::collections::BTreeSet<String>,
) -> Result<(), rusqlite::Error> {
    let mut directories = BTreeMap::<String, DirectoryRecord>::new();
    insert_directory_record(repo_root, "", &mut directories).map_err(sync_sql_error)?;
    supplement_directory_records_from_files(&mut directories, files);
    for path in existing_directory_paths {
        collect_directory_records_for_path(repo_root, path, true, &mut directories)
            .map_err(sync_sql_error)?;
    }
    for path in missing_paths {
        collect_directory_records_for_path(repo_root, path, false, &mut directories)
            .map_err(sync_sql_error)?;
    }

    for directory in directory_records_from_map(directories).map_err(sync_sql_error)? {
        tx.execute(
            r#"
            INSERT INTO directories (repo_id, path, parent_path, name, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(repo_id, path)
            DO UPDATE SET
              parent_path = excluded.parent_path,
              name = excluded.name,
              updated_at = excluded.updated_at
            "#,
            params![
                repo_id,
                directory.path,
                directory.parent_path,
                directory.name,
                directory.updated_at
            ],
        )?;
    }
    Ok(())
}
