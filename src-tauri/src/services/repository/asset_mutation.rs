//! Asset path mutation helpers used by file browser operations.

use super::*;

pub(super) fn rename_file_asset_record(
    tx: &Transaction<'_>,
    repo_id: &str,
    source_path: &str,
    target_path: &str,
    new_name: &str,
    new_extension: &str,
    modified_at: &str,
) -> Result<(), rusqlite::Error> {
    let updated = tx.execute(
        r#"
        UPDATE assets
        SET path = ?3, filename = ?4, extension = ?5, modified_at = ?6, updated_at = ?6
        WHERE repo_id = ?1 AND path = ?2
        "#,
        params![
            repo_id,
            source_path,
            target_path,
            new_name,
            new_extension,
            modified_at
        ],
    )?;

    if updated == 0 {
        return Ok(());
    }

    tx.execute(
        r#"
        INSERT OR REPLACE INTO events (event_id, repo_id, asset_id, event_type, path, payload_json, created_at)
        SELECT
          ?4,
          repo_id,
          asset_id,
          'asset.renamed',
          ?2,
          ?3,
          ?5
        FROM assets
        WHERE repo_id = ?1 AND path = ?2
        "#,
        params![
            repo_id,
            target_path,
            serde_json::json!({ "sourcePath": source_path }).to_string(),
            format!("evt-asset-renamed-{}", slugify_repo_id(repo_id, target_path)),
            now_rfc3339()
        ],
    )?;
    tx.execute(
        r#"
        UPDATE hardlink_members
        SET path = ?3, verified_at = ?4
        WHERE repo_id = ?1
          AND asset_id = (
            SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2 LIMIT 1
          )
        "#,
        params![repo_id, target_path, target_path, now_rfc3339()],
    )?;

    Ok(())
}

pub(super) fn rename_directory_asset_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    source_path: &str,
    target_path: &str,
) -> Result<(), rusqlite::Error> {
    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id, path
        FROM assets
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        "#,
    )?;
    let prefix = format!("{source_path}/%");
    let rows = stmt.query_map(params![repo_id, source_path, prefix], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let assets = rows.collect::<Result<Vec<_>, _>>()?;
    let now = now_rfc3339();

    for (asset_id, old_path) in assets {
        let suffix = old_path.strip_prefix(source_path).unwrap_or("");
        let new_path = format!("{target_path}{suffix}");
        let filename = Path::new(&new_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| asset_id.clone());
        let extension = Path::new(&new_path)
            .extension()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();

        tx.execute(
            r#"
            UPDATE assets
            SET path = ?3, filename = ?4, extension = ?5, updated_at = ?6, modified_at = ?6
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, asset_id, new_path, filename, extension, now],
        )?;
        tx.execute(
            r#"
            UPDATE hardlink_members
            SET path = ?3, verified_at = ?4
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, asset_id, new_path, now],
        )?;
    }

    Ok(())
}

pub(super) fn move_directory_contents_to_parent(
    service_root: &Path,
    repo: &RepositoryRecord,
    repo_root: &Path,
    repo_id: &str,
    source_path: &str,
) -> Result<(), String> {
    let source_abs = resolve_repository_relative_path(repo_root, source_path)?;
    if !source_abs.exists() || !source_abs.is_dir() {
        return Err(format!("directory not found: {source_path}"));
    }

    let target_parent_path = parent_relative_path(source_path);
    let target_parent_abs = if target_parent_path.is_empty() {
        repo_root.to_path_buf()
    } else {
        resolve_repository_relative_path(repo_root, &target_parent_path)?
    };

    let mut children = Vec::new();
    for entry in fs::read_dir(&source_abs).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let child_name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&child_name) {
            continue;
        }

        let child_source_path = join_relative_path(source_path, &child_name);
        let child_target_path = join_relative_path(&target_parent_path, &child_name);
        let child_target_abs = target_parent_abs.join(&child_name);

        if child_target_abs.exists() {
            return Err(format!("target already exists: {child_target_path}"));
        }

        children.push((entry.path(), child_source_path, child_target_path));
    }

    if children.is_empty() {
        delete_backend_entry(service_root, repo, repo_root, source_path, true)?;
        return Ok(());
    }

    for (child_abs, _, child_target_path) in &children {
        let child_name = Path::new(child_target_path)
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid target path: {child_target_path}"))?;
        fs::rename(child_abs, target_parent_abs.join(child_name)).map_err(io_error)?;
    }
    fs::remove_dir(&source_abs).map_err(io_error)?;

    let storage_paths = ensure_repository_storage_paths(
        service_root,
        repo_id,
        repo_root,
        &repo.backend_record.plugin_id,
    )?;
    let mut connection = Connection::open(storage_paths.database_path).map_err(db_error)?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(db_error)?;
    let tx = connection.transaction().map_err(db_error)?;
    for (_, child_source_path, child_target_path) in &children {
        rename_directory_move_asset_records(&tx, repo_id, child_source_path, child_target_path)
            .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;

    Ok(())
}

pub(super) fn rename_directory_move_asset_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    source_path: &str,
    target_path: &str,
) -> Result<(), rusqlite::Error> {
    let existing = tx
        .query_row(
            r#"
            SELECT 1
            FROM assets
            WHERE repo_id = ?1 AND path = ?2
            LIMIT 1
            "#,
            params![repo_id, source_path],
            |_| Ok(()),
        )
        .optional()?;

    if existing.is_some() {
        let filename = Path::new(target_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| target_path.to_string());
        let extension = Path::new(target_path)
            .extension()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default();
        let now = now_rfc3339();

        tx.execute(
            r#"
            UPDATE assets
            SET path = ?3, filename = ?4, extension = ?5, updated_at = ?6, modified_at = ?6
            WHERE repo_id = ?1 AND path = ?2
            "#,
            params![repo_id, source_path, target_path, filename, extension, now],
        )?;
        tx.execute(
            r#"
            UPDATE hardlink_members
            SET path = ?3, verified_at = ?4
            WHERE repo_id = ?1 AND asset_id = (
              SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?3 LIMIT 1
            )
            "#,
            params![repo_id, source_path, target_path, now],
        )?;
        return Ok(());
    }

    rename_directory_asset_records(tx, repo_id, source_path, target_path)
}

pub(super) fn mark_file_asset_deleted(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    let asset_id = tx
        .query_row(
            "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2",
            params![repo_id, path],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    tx.execute(
        r#"
        UPDATE assets
        SET status = 'deleted', updated_at = ?3
        WHERE repo_id = ?1 AND path = ?2
        "#,
        params![repo_id, path, now_rfc3339()],
    )?;
    if let Some(asset_id) = asset_id {
        mark_hardlink_member_missing(tx, repo_id, &asset_id)?;
    }
    Ok(())
}

pub(super) fn mark_directory_assets_deleted(
    tx: &Transaction<'_>,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    let prefix = format!("{path}/%");
    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id
        FROM assets
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        "#,
    )?;
    let asset_rows = stmt.query_map(params![repo_id, path, prefix.as_str()], |row| {
        row.get::<_, String>(0)
    })?;
    let asset_ids = asset_rows.collect::<Result<Vec<_>, _>>()?;
    tx.execute(
        r#"
        UPDATE assets
        SET status = 'deleted', updated_at = ?4
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        "#,
        params![repo_id, path, prefix, now_rfc3339()],
    )?;
    for asset_id in asset_ids {
        mark_hardlink_member_missing(tx, repo_id, &asset_id)?;
    }
    Ok(())
}
