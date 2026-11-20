//! 目录记录、网易云目录缓存与仓库概览的读取和写入。

use super::*;

pub(in super::super) fn load_directory_records(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<DirectoryRecord>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT path, parent_path, name, updated_at
        FROM directories
        WHERE repo_id = ?1
        ORDER BY CASE WHEN path = '' THEN 0 ELSE 1 END, parent_path, name COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok(DirectoryRecord {
            path: row.get(0)?,
            parent_path: row.get(1)?,
            name: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(in super::super) fn load_directory_records_for_parent(
    connection: &Connection,
    repo_id: &str,
    parent_path: &str,
) -> Result<Vec<DirectoryRecord>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT path, parent_path, name, updated_at
        FROM directories
        WHERE repo_id = ?1 AND parent_path = ?2 AND path != ''
        ORDER BY name COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, parent_path], |row| {
        Ok(DirectoryRecord {
            path: row.get(0)?,
            parent_path: row.get(1)?,
            name: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(in super::super) fn load_direct_file_counts_by_parent(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<String, usize>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          CASE
            WHEN path = filename THEN ''
            ELSE substr(path, 1, length(path) - length(filename) - 1)
          END AS parent_path,
          COUNT(*) AS file_count
        FROM assets
        WHERE repo_id = ?1 AND status != 'deleted'
        GROUP BY parent_path
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;

    let mut counts = BTreeMap::new();
    for row in rows {
        let (parent_path, file_count) = row?;
        counts.insert(parent_path, usize::try_from(file_count).unwrap_or(0));
    }
    Ok(counts)
}

pub(in super::super) fn has_directory_cache(
    connection: &Connection,
    repo_id: &str,
) -> Result<bool, rusqlite::Error> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM directories WHERE repo_id = ?1 LIMIT 1)",
            [repo_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
}

pub(in super::super) fn latest_directory_indexed_at(
    connection: &Connection,
    repo_id: &str,
) -> Result<Option<String>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT MAX(updated_at) FROM directories WHERE repo_id = ?1",
            [repo_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(|value| value.flatten())
}

pub(in super::super) fn replace_directory_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    directories: &[DirectoryRecord],
) -> Result<(), rusqlite::Error> {
    tx.execute("DELETE FROM directories WHERE repo_id = ?1", [repo_id])?;
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO directories (repo_id, path, parent_path, name, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )?;
    for directory in directories {
        stmt.execute(params![
            repo_id,
            directory.path,
            directory.parent_path,
            directory.name,
            directory.updated_at
        ])?;
    }
    Ok(())
}

pub(in super::super) fn upsert_directory_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
    parent_path: &str,
    name: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        INSERT INTO directories (repo_id, path, parent_path, name, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(repo_id, path)
        DO UPDATE SET
          parent_path = excluded.parent_path,
          name = excluded.name,
          updated_at = excluded.updated_at
        "#,
        params![repo_id, path, parent_path, name, now_rfc3339()],
    )?;
    Ok(())
}

pub(in super::super) fn load_netease_directory_cache(
    connection: &Connection,
    repo_id: &str,
    directory_path: &str,
) -> Result<Option<NeteaseDirectoryCacheRecord>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT total_entries, refreshed_at
            FROM netease_directory_cache
            WHERE repo_id = ?1 AND directory_path = ?2
            "#,
            params![repo_id, directory_path],
            |row| {
                Ok(NeteaseDirectoryCacheRecord {
                    total_entries: row.get::<_, i64>(0)?.max(0) as usize,
                    refreshed_at: row.get(1)?,
                })
            },
        )
        .optional()
}

pub(in super::super) fn load_netease_directory_entries_page(
    connection: &Connection,
    repo_id: &str,
    directory_path: &str,
    offset: usize,
    limit: usize,
) -> Result<Vec<(usize, FileSystemEntry)>, rusqlite::Error> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut stmt = connection.prepare(
        r#"
        SELECT
          order_index,
          path,
          name,
          kind,
          extension,
          size_bytes,
          modified_at,
          is_virtual,
          provider_id,
          provider_item_id,
          source_payload_json,
          local_absolute_path
        FROM netease_directory_entries
        WHERE repo_id = ?1
          AND directory_path = ?2
          AND order_index >= ?3
          AND order_index < ?4
        ORDER BY order_index ASC
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            repo_id,
            directory_path,
            offset.min(i64::MAX as usize) as i64,
            offset.saturating_add(limit).min(i64::MAX as usize) as i64
        ],
        |row| {
            let kind = match row.get::<_, String>(3)?.as_str() {
                "directory" => FileSystemEntryKind::Directory,
                _ => FileSystemEntryKind::File,
            };
            Ok((
                row.get::<_, i64>(0)?.max(0) as usize,
                FileSystemEntry {
                    path: row.get(1)?,
                    name: row.get(2)?,
                    kind,
                    extension: row.get(4)?,
                    size_bytes: row.get(5)?,
                    modified_at: row.get(6)?,
                    is_virtual: row.get::<_, i64>(7)? != 0,
                    provider_id: row.get(8)?,
                    provider_item_id: row.get(9)?,
                    source_payload: parse_json_column_nullable(row.get::<_, Option<String>>(10)?)?,
                    local_absolute_path: row.get(11)?,
                    status: None,
                    shared_asset_id: None,
                    tags: None,
                    thumbnail_local_absolute_path: None,
                },
            ))
        },
    )?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(in super::super) fn clear_netease_directory_cache(
    connection: &Connection,
    repo_id: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM netease_directory_entries WHERE repo_id = ?1",
        [repo_id],
    )?;
    connection.execute(
        "DELETE FROM netease_directory_cache WHERE repo_id = ?1",
        [repo_id],
    )?;
    Ok(())
}

pub(in super::super) fn clear_netease_directory_cache_for_directory(
    connection: &Connection,
    repo_id: &str,
    directory_path: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM netease_directory_entries WHERE repo_id = ?1 AND directory_path = ?2",
        params![repo_id, directory_path],
    )?;
    connection.execute(
        "DELETE FROM netease_directory_cache WHERE repo_id = ?1 AND directory_path = ?2",
        params![repo_id, directory_path],
    )?;
    Ok(())
}

pub(in super::super) fn replace_netease_directory_cache_page(
    connection: &Connection,
    repo_id: &str,
    directory_path: &str,
    offset: usize,
    entries: &[FileSystemEntry],
    total_entries: usize,
    refreshed_at: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        INSERT INTO netease_directory_cache (repo_id, directory_path, total_entries, refreshed_at)
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(repo_id, directory_path)
        DO UPDATE SET
          total_entries = excluded.total_entries,
          refreshed_at = excluded.refreshed_at
        "#,
        params![repo_id, directory_path, total_entries as i64, refreshed_at],
    )?;
    connection.execute(
        r#"
        DELETE FROM netease_directory_entries
        WHERE repo_id = ?1
          AND directory_path = ?2
          AND order_index >= ?3
          AND order_index < ?4
        "#,
        params![
            repo_id,
            directory_path,
            offset as i64,
            offset.saturating_add(entries.len()) as i64
        ],
    )?;
    let mut stmt = connection.prepare(
        r#"
        INSERT INTO netease_directory_entries (
          repo_id, directory_path, order_index, path, name, kind, extension, size_bytes,
          modified_at, is_virtual, provider_id, provider_item_id, source_payload_json, local_absolute_path
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
        "#,
    )?;
    for (index, entry) in entries.iter().enumerate() {
        stmt.execute(params![
            repo_id,
            directory_path,
            offset.saturating_add(index) as i64,
            entry.path,
            entry.name,
            match entry.kind {
                FileSystemEntryKind::Directory => "directory",
                FileSystemEntryKind::File => "file",
            },
            entry.extension,
            entry.size_bytes,
            entry.modified_at,
            if entry.is_virtual { 1 } else { 0 },
            entry.provider_id,
            entry.provider_item_id,
            entry.source_payload.as_ref().map(|value| value.to_string()),
            entry.local_absolute_path
        ])?;
    }
    Ok(())
}

pub(in super::super) fn dominant_folder_label(
    folders: &[FolderSummary],
    assets: &[AssetSummary],
) -> String {
    if let Some(folder) = folders.first() {
        return folder.label.clone();
    }

    assets
        .first()
        .and_then(|asset| Path::new(&asset.path).parent())
        .map(|parent| parent.to_string_lossy().replace('\\', "/"))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "仓库根目录".to_string())
}

pub(in super::super) fn build_repository_overview(
    repo_root: &Path,
    assets: &[AssetSummary],
    trash_count: i64,
) -> Result<RepositoryOverview, String> {
    let file_count = assets.len() as i64;
    let total_size_bytes = assets.iter().map(|asset| asset.size_bytes).sum::<i64>();
    let folder_count = count_repository_directories(repo_root)?;
    let readme_content = read_repository_readme(repo_root)?;

    Ok(RepositoryOverview {
        total_size_bytes,
        total_size_label: format_size_label(total_size_bytes),
        file_count,
        folder_count,
        trash_count,
        readme_content,
    })
}

/// 统计资源库目录，并通过规范路径去重避免符号链接形成递归环。
pub(in super::super) fn count_repository_directories(repo_root: &Path) -> Result<i64, String> {
    if !repo_root.exists() {
        return Ok(0);
    }

    let mut total = 0;
    let mut pending = vec![repo_root.to_path_buf()];
    let mut visited = HashSet::from([fs::canonicalize(repo_root).map_err(io_error)?]);
    while let Some(directory) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) if directory != repo_root && is_skippable_filesystem_error(&error) => {
                continue;
            }
            Err(error) => return Err(io_error(error)),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if is_skippable_filesystem_error(&error) => continue,
                Err(error) => return Err(io_error(error)),
            };
            let name = entry.file_name().to_string_lossy().to_string();
            if is_internal_repository_dir(&name) {
                continue;
            }

            let path = entry.path();
            let metadata = match fs::metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) if is_skippable_filesystem_error(&error) => continue,
                Err(error) => return Err(io_error(error)),
            };
            if !metadata.is_dir() {
                continue;
            }

            total += 1;
            let canonical_path = match fs::canonicalize(&path) {
                Ok(path) => path,
                Err(error) if is_skippable_filesystem_error(&error) => continue,
                Err(error) => return Err(io_error(error)),
            };
            if visited.insert(canonical_path) {
                pending.push(path);
            }
        }
    }

    Ok(total)
}

pub(in super::super) fn read_repository_readme(repo_root: &Path) -> Result<Option<String>, String> {
    for candidate in ["README.md", "readme.md"] {
        let path = repo_root.join(candidate);
        if path.is_file() {
            return fs::read_to_string(path).map(Some).map_err(io_error);
        }
    }

    Ok(None)
}
