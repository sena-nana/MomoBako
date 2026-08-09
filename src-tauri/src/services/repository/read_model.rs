//! Repository read-model loaders for snapshots, assets, playlists, and metadata.

use super::*;

pub(super) fn load_directory_records(
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

pub(super) fn load_directory_records_for_parent(
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

pub(super) fn load_direct_file_counts_by_parent(
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

pub(super) fn has_directory_cache(
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

pub(super) fn latest_directory_indexed_at(
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

pub(super) fn replace_directory_records(
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

pub(super) fn upsert_directory_record(
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

pub(super) fn load_netease_directory_cache(
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

pub(super) fn load_netease_directory_entries_page(
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

pub(super) fn clear_netease_directory_cache(
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

pub(super) fn clear_netease_directory_cache_for_directory(
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

pub(super) fn replace_netease_directory_cache_page(
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

#[allow(dead_code)]
pub(super) fn delete_directory_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM directories WHERE repo_id = ?1 AND path = ?2",
        params![repo_id, path],
    )?;
    Ok(())
}

#[allow(dead_code)]
pub(super) fn rename_directory_records(
    tx: &Transaction<'_>,
    repo_id: &str,
    source_path: &str,
    target_path: &str,
) -> Result<(), rusqlite::Error> {
    let prefix = format!("{source_path}/%");
    let mut stmt = tx.prepare(
        r#"
        SELECT path, parent_path, name
        FROM directories
        WHERE repo_id = ?1 AND (path = ?2 OR path LIKE ?3)
        ORDER BY LENGTH(path) ASC
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, source_path, prefix], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let rows = rows.collect::<Result<Vec<_>, _>>()?;
    let now = now_rfc3339();
    for (old_path, _old_parent_path, _old_name) in rows {
        let suffix = old_path.strip_prefix(source_path).unwrap_or("");
        let new_path = format!("{target_path}{suffix}");
        let new_parent_path = parent_relative_path(&new_path);
        let new_name = if new_path.is_empty() {
            String::new()
        } else {
            Path::new(&new_path)
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default()
        };
        tx.execute(
            r#"
            UPDATE directories
            SET path = ?3, parent_path = ?4, name = ?5, updated_at = ?6
            WHERE repo_id = ?1 AND path = ?2
            "#,
            params![repo_id, old_path, new_path, new_parent_path, new_name, now],
        )?;
    }
    Ok(())
}

pub(super) fn dominant_folder_label(folders: &[FolderSummary], assets: &[AssetSummary]) -> String {
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

pub(super) fn build_repository_overview(
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
pub(super) fn count_repository_directories(repo_root: &Path) -> Result<i64, String> {
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

pub(super) fn read_repository_readme(repo_root: &Path) -> Result<Option<String>, String> {
    for candidate in ["README.md", "readme.md"] {
        let path = repo_root.join(candidate);
        if path.is_file() {
            return fs::read_to_string(path).map(Some).map_err(io_error);
        }
    }

    Ok(None)
}

pub(super) fn load_active_asset_count(connection: &Connection) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COUNT(*) FROM assets WHERE status != 'deleted'",
        [],
        |row| row.get(0),
    )
}

pub(super) fn load_asset_path_map(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<String, AssetPathRecord>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          a.path,
          a.asset_id,
          a.status,
          a.thumbnail_path,
          hm.group_id,
          hm.link_state,
          a.is_virtual,
          a.provider_id,
          a.provider_item_id,
          a.source_payload_json,
          a.local_absolute_path
        FROM assets a
        LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
        WHERE a.repo_id = ?1
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (
            path,
            asset_id,
            status,
            thumbnail_path,
            hardlink_group_id,
            hardlink_state,
            is_virtual,
            provider_id,
            provider_item_id,
            source_payload_json,
            local_absolute_path,
        ) = row?;
        map.insert(
            path,
            AssetPathRecord {
                asset_id,
                status,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
                is_virtual: is_virtual != 0,
                provider_id,
                provider_item_id,
                source_payload: parse_json_column_nullable(source_payload_json)?,
                local_absolute_path,
            },
        );
    }
    Ok(map)
}

pub(super) fn load_asset_path_map_for_paths(
    connection: &Connection,
    repo_id: &str,
    paths: &[String],
) -> Result<BTreeMap<String, AssetPathRecord>, rusqlite::Error> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        SELECT
          a.path,
          a.asset_id,
          a.status,
          a.thumbnail_path,
          hm.group_id,
          hm.link_state,
          a.is_virtual,
          a.provider_id,
          a.provider_item_id,
          a.source_payload_json,
          a.local_absolute_path
        FROM assets a
        LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
        WHERE a.repo_id = ?1
          AND a.path IN ({placeholders})
        "#
    ))?;
    let params = std::iter::once(repo_id).chain(paths.iter().map(String::as_str));
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (
            path,
            asset_id,
            status,
            thumbnail_path,
            hardlink_group_id,
            hardlink_state,
            is_virtual,
            provider_id,
            provider_item_id,
            source_payload_json,
            local_absolute_path,
        ) = row?;
        map.insert(
            path,
            AssetPathRecord {
                asset_id,
                status,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
                is_virtual: is_virtual != 0,
                provider_id,
                provider_item_id,
                source_payload: parse_json_column_nullable(source_payload_json)?,
                local_absolute_path,
            },
        );
    }
    Ok(map)
}

pub(super) fn load_entry_thumbnail_map(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<(String, String), ThumbnailRecord>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT path, kind, thumbnail_path, custom
        FROM entry_thumbnails
        WHERE repo_id = ?1
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (path, kind, thumbnail_path, custom) = row?;
        map.insert(
            (path, kind),
            ThumbnailRecord {
                path: thumbnail_path,
                custom: custom != 0,
            },
        );
    }
    Ok(map)
}

pub(super) fn load_entry_thumbnail_map_for_paths(
    connection: &Connection,
    repo_id: &str,
    entries: &[(String, String)],
) -> Result<BTreeMap<(String, String), ThumbnailRecord>, rusqlite::Error> {
    if entries.is_empty() {
        return Ok(BTreeMap::new());
    }

    let tuple_placeholders = std::iter::repeat("(?, ?)")
        .take(entries.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        SELECT path, kind, thumbnail_path, custom
        FROM entry_thumbnails
        WHERE repo_id = ?
          AND (path, kind) IN ({tuple_placeholders})
        "#
    ))?;
    let params = std::iter::once(repo_id).chain(
        entries
            .iter()
            .flat_map(|(path, kind)| [path.as_str(), kind.as_str()]),
    );
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (path, kind, thumbnail_path, custom) = row?;
        map.insert(
            (path, kind),
            ThumbnailRecord {
                path: thumbnail_path,
                custom: custom != 0,
            },
        );
    }
    Ok(map)
}

pub(super) fn load_entry_thumbnail_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
    kind: &str,
) -> Result<Option<ThumbnailRecord>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT thumbnail_path, custom
            FROM entry_thumbnails
            WHERE repo_id = ?1 AND path = ?2 AND kind = ?3
            "#,
            params![repo_id, path, kind],
            |row| {
                Ok(ThumbnailRecord {
                    path: row.get(0)?,
                    custom: row.get::<_, i64>(1)? != 0,
                })
            },
        )
        .optional()
}

pub(super) fn upsert_entry_thumbnail_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
    kind: &str,
    thumbnail_path: &str,
    custom: bool,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        INSERT INTO entry_thumbnails (repo_id, path, kind, thumbnail_path, custom, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(repo_id, path, kind)
        DO UPDATE SET
          thumbnail_path = excluded.thumbnail_path,
          custom = excluded.custom,
          updated_at = excluded.updated_at
        "#,
        params![
            repo_id,
            path,
            kind,
            thumbnail_path,
            if custom { 1 } else { 0 },
            now_rfc3339()
        ],
    )?;
    Ok(())
}

pub(super) fn remove_entry_thumbnail_record(
    connection: &Connection,
    repo_id: &str,
    path: &str,
    kind: &str,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        DELETE FROM entry_thumbnails
        WHERE repo_id = ?1 AND path = ?2 AND kind = ?3
        "#,
        params![repo_id, path, kind],
    )?;
    Ok(())
}

pub(super) fn update_asset_thumbnail_path(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
    thumbnail_path: Option<&str>,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        r#"
        UPDATE assets
        SET thumbnail_path = ?3, updated_at = ?4
        WHERE repo_id = ?1 AND asset_id = ?2
        "#,
        params![repo_id, asset_id, thumbnail_path, now_rfc3339()],
    )?;
    Ok(())
}

pub(super) fn normalize_asset_summaries(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    assets: Vec<AssetSummary>,
) -> Result<Vec<AssetSummary>, String> {
    assets
        .into_iter()
        .map(|mut asset| {
            asset.thumbnail_path = normalize_asset_thumbnail_path(
                connection,
                repo,
                thumbnail_root,
                &asset.asset_id,
                &asset.path,
                asset.thumbnail_path,
            )?;
            Ok(asset)
        })
        .collect()
}

pub(super) fn normalize_asset_thumbnail_map(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    asset_map: BTreeMap<String, AssetPathRecord>,
) -> Result<BTreeMap<String, AssetPathRecord>, String> {
    asset_map
        .into_iter()
        .map(|(path, mut record)| {
            let thumbnail_path = normalize_asset_thumbnail_path(
                connection,
                repo,
                thumbnail_root,
                &record.asset_id,
                &path,
                record.thumbnail_path,
            )?;
            record.thumbnail_path = thumbnail_path;
            Ok((path, record))
        })
        .collect()
}

pub(super) fn normalize_asset_thumbnail_path(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    asset_id: &str,
    entry_path: &str,
    thumbnail_path: Option<String>,
) -> Result<Option<String>, String> {
    let original_path = thumbnail_path.clone();
    let normalized = normalize_thumbnail_path(
        repo,
        thumbnail_root,
        entry_path,
        "file",
        "generated",
        thumbnail_path,
    )?;
    if normalized != original_path {
        update_asset_thumbnail_path(
            connection,
            &repo.summary.repo_id,
            asset_id,
            normalized.as_deref(),
        )
        .map_err(db_error)?;
    }
    Ok(normalized)
}

pub(super) fn normalize_entry_thumbnail_map(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    thumbnail_map: BTreeMap<(String, String), ThumbnailRecord>,
) -> Result<BTreeMap<(String, String), ThumbnailRecord>, String> {
    thumbnail_map
        .into_iter()
        .filter_map(|((path, kind), record)| {
            match normalize_entry_thumbnail_record(
                connection,
                repo,
                thumbnail_root,
                &path,
                &kind,
                Some(record),
            ) {
                Ok(Some(record)) => Some(Ok(((path, kind), record))),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            }
        })
        .collect()
}

pub(super) fn normalize_entry_thumbnail_record(
    connection: &Connection,
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    entry_path: &str,
    kind: &str,
    record: Option<ThumbnailRecord>,
) -> Result<Option<ThumbnailRecord>, String> {
    let Some(record) = record else {
        return Ok(None);
    };
    let source = if record.custom { "custom" } else { "generated" };
    let original_path = record.path.clone();
    let normalized = normalize_thumbnail_path(
        repo,
        thumbnail_root,
        entry_path,
        kind,
        source,
        Some(record.path),
    )?;
    match normalized {
        Some(path) => {
            if path != original_path {
                upsert_entry_thumbnail_record(
                    connection,
                    &repo.summary.repo_id,
                    entry_path,
                    kind,
                    &path,
                    record.custom,
                )
                .map_err(db_error)?;
            }
            Ok(Some(ThumbnailRecord {
                path,
                custom: record.custom,
            }))
        }
        None => {
            remove_entry_thumbnail_record(connection, &repo.summary.repo_id, entry_path, kind)
                .map_err(db_error)?;
            Ok(None)
        }
    }
}

pub(super) fn normalize_thumbnail_path(
    repo: &RepositoryRecord,
    thumbnail_root: &Path,
    entry_path: &str,
    kind: &str,
    source: &str,
    thumbnail_path: Option<String>,
) -> Result<Option<String>, String> {
    let Some(path) = thumbnail_path else {
        return Ok(None);
    };
    if thumbnail_path_is_valid(thumbnail_root, &path) {
        return Ok(Some(path));
    }

    let source_path = Path::new(&path);
    if !source_path.is_file() {
        return Ok(None);
    }

    let thumbnail_dir = thumbnail_root.join(thumbnail_repository_dir_name(
        &repo.summary.repo_id,
        &repo.summary.path,
    ));
    fs::create_dir_all(&thumbnail_dir).map_err(io_error)?;
    let target_path = thumbnail_dir.join(thumbnail_file_name(
        &repo.summary.repo_id,
        &repo.summary.path,
        entry_path,
        kind,
        source,
    ));
    if source_path != target_path {
        fs::copy(source_path, &target_path).map_err(io_error)?;
    }
    Ok(Some(target_path.to_string_lossy().to_string()))
}

pub(super) fn load_folder_summaries(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<FolderSummary>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          CASE
            WHEN instr(path, '/') > 0 THEN substr(path, 1, instr(path, '/') - 1)
            ELSE path
          END AS top_folder,
          COUNT(*) AS asset_count
        FROM assets
        WHERE repo_id = ?1 AND status != 'deleted'
        GROUP BY top_folder
        ORDER BY asset_count DESC, top_folder COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        let path: String = row.get(0)?;
        Ok(FolderSummary {
            label: path.clone(),
            path,
            asset_count: row.get(1)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_repository_shortcuts(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<RepositoryShortcut>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT shortcut_id, label, target_kind, target_path, target_id
        FROM repository_shortcuts
        WHERE repo_id = ?1
        ORDER BY sort_order, label COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok(RepositoryShortcut {
            shortcut_id: row.get(0)?,
            label: row.get(1)?,
            target_kind: row.get(2)?,
            target_path: row.get(3)?,
            target_id: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_playlists(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<PlaylistSummary>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          p.playlist_id,
          p.repo_id,
          p.name,
          p.player_type_id,
          p.player_plugin_id,
          p.player_label,
          p.file_class,
          COUNT(pi.playlist_item_id) AS item_count,
          p.sort_order,
          p.created_at,
          p.updated_at
        FROM playlists p
        LEFT JOIN playlist_items pi
          ON pi.repo_id = p.repo_id AND pi.playlist_id = p.playlist_id
        WHERE p.repo_id = ?1
        GROUP BY
          p.playlist_id, p.repo_id, p.name, p.player_type_id, p.player_plugin_id,
          p.player_label, p.file_class, p.sort_order, p.created_at, p.updated_at
        ORDER BY p.sort_order, p.updated_at DESC, p.name COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok(PlaylistSummary {
            playlist_id: row.get(0)?,
            repo_id: row.get(1)?,
            name: row.get(2)?,
            player_type_id: row.get(3)?,
            player_plugin_id: row.get(4)?,
            player_label: row.get(5)?,
            file_class: row.get(6)?,
            item_count: row.get(7)?,
            sort_order: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_playlist_summary(
    connection: &Connection,
    repo_id: &str,
    playlist_id: &str,
) -> Result<Option<PlaylistSummary>, rusqlite::Error> {
    load_playlists(connection, repo_id).map(|items| {
        items
            .into_iter()
            .find(|item| item.playlist_id == playlist_id)
    })
}

pub(super) fn load_playlist_memberships(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<String, Vec<String>>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT pi.asset_id, pi.playlist_id
        FROM playlist_items pi
        INNER JOIN playlists p
          ON p.repo_id = pi.repo_id AND p.playlist_id = pi.playlist_id
        WHERE pi.repo_id = ?1
        ORDER BY pi.asset_id, p.sort_order, p.updated_at DESC, p.name COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut memberships = BTreeMap::<String, Vec<String>>::new();
    for row in rows {
        let (asset_id, playlist_id) = row?;
        memberships.entry(asset_id).or_default().push(playlist_id);
    }
    Ok(memberships)
}

pub(super) fn load_playlist_detail(
    connection: &Connection,
    repo: &RepositoryRecord,
    registry: &PluginCatalog,
    repo_id: &str,
    playlist_id: &str,
) -> Result<PlaylistDetail, rusqlite::Error> {
    let playlist = load_playlist_summary(connection, repo_id, playlist_id)?
        .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
    let active_player = registry.playlist_player(&playlist.player_type_id);
    let mut stmt = connection.prepare(
        r#"
        SELECT
          pi.playlist_item_id,
          pi.playlist_id,
          pi.asset_id,
          pi.sort_order,
          pi.added_at,
          a.path,
          a.filename,
          a.extension,
          a.thumbnail_path,
          a.status,
          a.is_virtual,
          a.provider_id,
          a.provider_item_id,
          a.source_payload_json,
          a.local_absolute_path
        FROM playlist_items pi
        LEFT JOIN assets a
          ON a.repo_id = pi.repo_id AND a.asset_id = pi.asset_id
        WHERE pi.repo_id = ?1 AND pi.playlist_id = ?2
        ORDER BY pi.sort_order, pi.added_at
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, playlist_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<i64>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
            row.get::<_, Option<String>>(13)?,
            row.get::<_, Option<String>>(14)?,
        ))
    })?;

    let items = rows
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|row| {
            let (
                playlist_item_id,
                playlist_id,
                asset_id,
                sort_order,
                added_at,
                path,
                filename,
                extension,
                thumbnail_path,
                asset_status,
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload_json,
                local_absolute_path,
            ) = row;
            let extension = extension.unwrap_or_default();
            let path_value = path.clone().unwrap_or_default();
            let thumbnail_asset_id = asset_id.clone();
            let thumbnail_entry_path = path.clone().unwrap_or_default();
            let (status, status_reason) = resolve_playlist_item_status(
                &playlist,
                active_player.as_ref(),
                path.as_deref(),
                &extension,
                asset_status.as_deref(),
            );
            Ok(PlaylistItem {
                playlist_item_id,
                playlist_id,
                asset_id,
                path: path_value,
                filename: filename.unwrap_or_else(|| "(已失效文件)".to_string()),
                extension,
                thumbnail_path: thumbnail_path.and_then(|item| {
                    normalize_asset_thumbnail_path(
                        connection,
                        repo,
                        &repo
                            .summary
                            .path
                            .parse::<PathBuf>()
                            .unwrap_or_else(|_| PathBuf::from(&repo.summary.path))
                            .join(REPO_META_DIR)
                            .join("thumbnails"),
                        &thumbnail_asset_id,
                        &thumbnail_entry_path,
                        Some(item),
                    )
                    .ok()
                    .flatten()
                }),
                status,
                status_reason,
                sort_order,
                added_at,
                is_virtual: is_virtual.unwrap_or(0) != 0,
                provider_id,
                provider_item_id,
                source_payload: parse_json_column_nullable(source_payload_json)?,
                local_absolute_path,
            })
        })
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    Ok(PlaylistDetail { playlist, items })
}

pub(super) fn resolve_playlist_item_status(
    playlist: &PlaylistSummary,
    player: Option<&PlaylistPlayerRegistration>,
    path: Option<&str>,
    extension: &str,
    asset_status: Option<&str>,
) -> (String, Option<String>) {
    let Some(path) = path else {
        return (
            "missing".to_string(),
            Some("资源索引中已找不到该文件".to_string()),
        );
    };
    let Some(asset_status) = asset_status else {
        return (
            "missing".to_string(),
            Some("资源索引中已找不到该文件".to_string()),
        );
    };
    if asset_status == "deleted" {
        return (
            "trashed".to_string(),
            Some(format!("文件已移入回收站: {path}")),
        );
    }
    let Some(player) = player else {
        return (
            "pluginUnavailable".to_string(),
            Some(format!("缺少播放类型插件: {}", playlist.player_type_id)),
        );
    };
    if !playlist_player_supports_extension(player, extension) {
        return (
            "incompatible".to_string(),
            Some(format!("当前文件扩展名不再兼容: .{extension}")),
        );
    }
    ("ready".to_string(), None)
}

pub(super) fn playlist_player_supports_extension(
    player: &PlaylistPlayerRegistration,
    extension: &str,
) -> bool {
    let normalized = extension.trim().to_ascii_lowercase();
    !normalized.is_empty()
        && player
            .supported_extensions
            .iter()
            .any(|item| item == &normalized)
}

pub(super) fn next_playlist_sort_order(
    connection: &Connection,
    repo_id: &str,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM playlists WHERE repo_id = ?1",
        [repo_id],
        |row| row.get(0),
    )
}

pub(super) fn next_playlist_item_sort_order(
    connection: &Connection,
    repo_id: &str,
    playlist_id: &str,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM playlist_items WHERE repo_id = ?1 AND playlist_id = ?2",
        params![repo_id, playlist_id],
        |row| row.get(0),
    )
}

pub(super) fn validate_playlist_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("playlist name cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

pub(super) fn validate_playlist_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("playlist id cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

pub(super) fn playlist_id_for(repo_id: &str, name: &str) -> String {
    format!(
        "playlist-{}",
        sha256_hex(&[repo_id.as_bytes(), name.trim().as_bytes()])
    )
}

pub(super) fn playlist_item_id_for(playlist_id: &str, asset_id: &str) -> String {
    format!(
        "playlist-item-{}",
        sha256_hex(&[playlist_id.as_bytes(), asset_id.as_bytes()])
    )
}

pub(super) fn normalize_id_list(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

pub(super) fn load_repository_tag_groups(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<RepositoryTagGroup>, rusqlite::Error> {
    let mut group_stmt = connection.prepare(
        r#"
        SELECT tag_group_id, name
        FROM tag_groups
        WHERE repo_id = ?1
        ORDER BY sort_order, name COLLATE NOCASE
        "#,
    )?;
    let group_rows = group_stmt.query_map([repo_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let groups = group_rows.collect::<Result<Vec<_>, _>>()?;
    let mut result = Vec::new();
    for (tag_group_id, name) in groups {
        let mut member_stmt = connection.prepare(
            r#"
            SELECT tag
            FROM tag_group_members
            WHERE repo_id = ?1 AND tag_group_id = ?2
            ORDER BY sort_order, tag COLLATE NOCASE
            "#,
        )?;
        let member_rows =
            member_stmt.query_map(params![repo_id, tag_group_id.as_str()], |row| row.get(0))?;
        result.push(RepositoryTagGroup {
            tag_group_id,
            name,
            tags: member_rows.collect::<Result<Vec<String>, _>>()?,
        });
    }
    Ok(result)
}

pub(super) fn load_repository_actions(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<RepositoryAction>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT action_id
        FROM repository_actions
        WHERE repo_id = ?1
        ORDER BY sort_order, name COLLATE NOCASE
        "#,
    )?;
    let ids = stmt
        .query_map([repo_id], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .filter_map(
            |action_id| match load_repository_action(connection, repo_id, &action_id) {
                Ok(Some(action)) => Some(Ok(action)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            },
        )
        .collect()
}

pub(super) fn load_repository_action(
    connection: &Connection,
    repo_id: &str,
    action_id: &str,
) -> Result<Option<RepositoryAction>, rusqlite::Error> {
    let Some(base) = connection
        .query_row(
            r#"
            SELECT action_id, repo_id, source, source_action_id, name, status, enabled,
                   raw_json, unsupported_reason, sort_order, created_at, updated_at
            FROM repository_actions
            WHERE repo_id = ?1 AND action_id = ?2
            "#,
            params![repo_id, action_id],
            |row| {
                let raw_json: String = row.get(7)?;
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)? != 0,
                    parse_json_column(&raw_json)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, String>(11)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let steps = load_repository_action_steps(connection, repo_id, action_id)?;
    let last_run = load_repository_action_last_run(connection, repo_id, action_id)?;
    Ok(Some(RepositoryAction {
        action_id: base.0,
        repo_id: base.1,
        source: base.2,
        source_action_id: base.3,
        name: base.4,
        status: base.5,
        enabled: base.6,
        raw: base.7,
        unsupported_reason: base.8,
        sort_order: base.9,
        created_at: base.10,
        updated_at: base.11,
        steps,
        last_run,
    }))
}

pub(super) fn load_repository_action_steps(
    connection: &Connection,
    repo_id: &str,
    action_id: &str,
) -> Result<Vec<RepositoryActionStep>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT step_id, action_id, repo_id, step_kind, label, status,
               config_json, raw_json, unsupported_reason, sort_order
        FROM repository_action_steps
        WHERE repo_id = ?1 AND action_id = ?2
        ORDER BY sort_order, label COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, action_id], |row| {
        let config_json: String = row.get(6)?;
        let raw_json: String = row.get(7)?;
        Ok(RepositoryActionStep {
            step_id: row.get(0)?,
            action_id: row.get(1)?,
            repo_id: row.get(2)?,
            step_kind: row.get(3)?,
            label: row.get(4)?,
            status: row.get(5)?,
            config: parse_json_column(&config_json)?,
            raw: parse_json_column(&raw_json)?,
            unsupported_reason: row.get(8)?,
            sort_order: row.get(9)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_repository_action_last_run(
    connection: &Connection,
    repo_id: &str,
    action_id: &str,
) -> Result<Option<RepositoryActionRun>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT run_id, action_id, repo_id, status, target_json, message, started_at, finished_at
            FROM repository_action_runs
            WHERE repo_id = ?1 AND action_id = ?2
            ORDER BY started_at DESC
            LIMIT 1
            "#,
            params![repo_id, action_id],
            |row| {
                let target_json: String = row.get(4)?;
                Ok(RepositoryActionRun {
                    run_id: row.get(0)?,
                    action_id: row.get(1)?,
                    repo_id: row.get(2)?,
                    status: row.get(3)?,
                    target: parse_json_column(&target_json)?,
                    message: row.get(5)?,
                    started_at: row.get(6)?,
                    finished_at: row.get(7)?,
                })
            },
        )
        .optional()
}

pub(super) fn resolve_action_target_asset_ids(
    connection: &Connection,
    request: &RepositoryActionRunRequest,
) -> Result<Vec<String>, String> {
    let mut ids = request.asset_ids.clone().unwrap_or_default();
    for path in request.target_paths.clone().unwrap_or_default() {
        let entry_path = normalize_entry_path(&path)?;
        let asset_id = connection
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'",
                params![request.repo_id, entry_path],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("action target asset not found: {path}"))?;
        ids.push(asset_id);
    }
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for id in ids {
        let id = id.trim().to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let exists = connection
            .query_row(
                "SELECT 1 FROM assets WHERE repo_id = ?1 AND asset_id = ?2 AND status != 'deleted'",
                params![request.repo_id, id.as_str()],
                |_| Ok(()),
            )
            .optional()
            .map_err(db_error)?
            .is_some();
        if !exists {
            return Err(format!("action target asset not found: {id}"));
        }
        result.push(id);
    }
    Ok(result)
}

pub(super) fn apply_repository_action_step(
    tx: &Transaction<'_>,
    repo_id: &str,
    target_asset_ids: &[String],
    step: &RepositoryActionStep,
    source: &str,
) -> Result<String, String> {
    if step.status != "ready" {
        return Err(step
            .unsupported_reason
            .clone()
            .unwrap_or_else(|| "repository action step is unsupported".to_string()));
    }
    match step.step_kind.as_str() {
        "metadata.update" => {
            let metadata = step
                .config
                .get("metadata")
                .and_then(serde_json::Value::as_object)
                .ok_or_else(|| "metadata action step is missing metadata".to_string())?;
            let patch = metadata
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>();
            for asset_id in target_asset_ids {
                update_metadata_for_asset_in_transaction(tx, repo_id, asset_id, &patch, source)
                    .map_err(db_error)?;
            }
            Ok(format!("已更新 {} 个目标的元数据", target_asset_ids.len()))
        }
        "tagGroups.set" => {
            let tags = step
                .config
                .get("tags")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            let patch = BTreeMap::from([("tagGroups".to_string(), tags)]);
            for asset_id in target_asset_ids {
                update_metadata_for_asset_in_transaction(tx, repo_id, asset_id, &patch, source)
                    .map_err(db_error)?;
            }
            Ok(format!("已更新 {} 个目标的标签", target_asset_ids.len()))
        }
        value => Err(format!("unsupported repository action step kind: {value}")),
    }
}

pub(super) fn update_metadata_for_asset_in_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
    metadata: &BTreeMap<String, serde_json::Value>,
    source: &str,
) -> Result<(), rusqlite::Error> {
    let target_asset_ids = load_alias_member_asset_ids(tx, repo_id, asset_id)?;
    let sync_tags = metadata.contains_key("tagGroups");
    let synced_tags = if sync_tags {
        metadata_tags_from_tag_groups(metadata.get("tagGroups"))
    } else {
        Vec::new()
    };
    let now = now_rfc3339();
    for target_asset_id in target_asset_ids {
        let before_map = load_metadata_map_from_transaction(tx, &target_asset_id)?;
        for (key, value) in metadata {
            let value_type = infer_value_type(value);
            tx.execute(
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
                params![target_asset_id, key, value_type, value.to_string(), now],
            )?;
        }
        if sync_tags {
            replace_asset_tags(tx, &target_asset_id, &synced_tags)?;
        }
        let target_version: i64 = tx.query_row(
            "SELECT version + 1 FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
            params![repo_id, target_asset_id],
            |row| row.get(0),
        )?;
        tx.execute(
            r#"
            UPDATE assets
            SET version = ?3, updated_at = ?4, modified_at = ?4
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, target_asset_id, target_version, now],
        )?;
        let after_map = load_metadata_map_from_transaction(tx, &target_asset_id)?;
        tx.execute(
            r#"
            INSERT INTO revisions (
              revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
            )
            VALUES (?1, ?2, ?3, ?4, 'metadata.updated', ?5, ?6, ?7)
            "#,
            params![
                format!("rev-{}-{}", target_asset_id, target_version),
                repo_id,
                target_asset_id,
                now,
                serde_json::to_string(&before_map).map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                })?,
                serde_json::to_string(&after_map).map_err(|error| {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                })?,
                source
            ],
        )?;
    }
    Ok(())
}

pub(super) fn load_assets(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<AssetSummary>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT
          a.asset_id,
          a.repo_id,
          a.path,
          a.filename,
          a.extension,
          a.size_bytes,
          a.status,
          a.modified_at,
          a.last_accessed_at,
          a.version,
          a.thumbnail_path,
          hm.group_id,
          hm.link_state,
          a.is_virtual,
          a.provider_id,
          a.provider_item_id,
          a.source_payload_json,
          a.local_absolute_path
        FROM assets a
        LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
        WHERE a.repo_id = ?1 AND a.status != 'deleted'
        ORDER BY a.modified_at DESC, a.filename COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([repo_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, String>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, Option<String>>(10)?,
            row.get::<_, Option<String>>(11)?,
            row.get::<_, Option<String>>(12)?,
            row.get::<_, i64>(13)?,
            row.get::<_, Option<String>>(14)?,
            row.get::<_, Option<String>>(15)?,
            row.get::<_, Option<String>>(16)?,
            row.get::<_, Option<String>>(17)?,
        ))
    })?;

    let base_assets = rows.collect::<Result<Vec<_>, _>>()?;

    base_assets
        .into_iter()
        .map(
            |(
                asset_id,
                repo_id,
                path,
                filename,
                extension,
                size_bytes,
                status,
                modified_at,
                last_accessed_at,
                version,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload_json,
                local_absolute_path,
            )| {
                let tags = load_tags(connection, &asset_id)?;

                Ok(AssetSummary {
                    asset_id,
                    repo_id,
                    path,
                    filename,
                    extension,
                    size_bytes,
                    size_label: format_size_label(size_bytes),
                    status,
                    modified_at,
                    last_accessed_at,
                    version,
                    tags,
                    thumbnail_path,
                    hardlink_group_id,
                    hardlink_state,
                    is_virtual: is_virtual != 0,
                    provider_id,
                    provider_item_id,
                    source_payload: parse_json_column_nullable(source_payload_json)?,
                    local_absolute_path,
                })
            },
        )
        .collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_tags(
    connection: &Connection,
    asset_id: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT tag
        FROM tags
        WHERE asset_id = ?1
        ORDER BY normalized_tag COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_metadata_entries(
    connection: &Connection,
    asset_id: &str,
) -> Result<Vec<MetadataEntry>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT key, value_type, value_json, version, updated_at
        FROM metadata
        WHERE asset_id = ?1
        ORDER BY key COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| {
        let value_json: String = row.get(2)?;
        let value = parse_json_column(&value_json)?;
        Ok(MetadataEntry {
            key: row.get(0)?,
            value_type: row.get(1)?,
            value,
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;

    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_metadata_map(
    connection: &Connection,
    asset_id: &str,
) -> Result<BTreeMap<String, serde_json::Value>, rusqlite::Error> {
    let entries = load_metadata_entries(connection, asset_id)?;
    let mut metadata = entries
        .into_iter()
        .map(|entry| (entry.key, entry.value))
        .collect::<BTreeMap<_, _>>();
    normalize_loaded_metadata(&mut metadata);
    Ok(metadata)
}

pub(super) fn load_metadata_maps_for_assets(
    connection: &Connection,
    asset_ids: &[String],
) -> Result<BTreeMap<String, BTreeMap<String, serde_json::Value>>, rusqlite::Error> {
    if asset_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(asset_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        SELECT asset_id, key, value_json
        FROM metadata
        WHERE asset_id IN ({placeholders})
        ORDER BY key COLLATE NOCASE
        "#
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(asset_ids.iter()), |row| {
        let value_json: String = row.get(2)?;
        let value = parse_json_column(&value_json)?;
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, value))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (asset_id, key, value) = row?;
        map.entry(asset_id)
            .or_insert_with(BTreeMap::new)
            .insert(key, value);
    }
    for metadata in map.values_mut() {
        normalize_loaded_metadata(metadata);
    }
    Ok(map)
}

pub(super) fn load_alias_paths_for_assets(
    connection: &Connection,
    repo_id: &str,
    asset_ids: &[String],
) -> Result<BTreeMap<String, Vec<String>>, rusqlite::Error> {
    if asset_ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let placeholders = std::iter::repeat("?")
        .take(asset_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        WITH selected_assets(asset_id) AS (
          SELECT asset_id FROM assets WHERE asset_id IN ({placeholders})
        ),
        selected_groups(alias_group_id) AS (
          SELECT DISTINCT alias_group_id
          FROM asset_alias_members
          WHERE repo_id = ? AND asset_id IN (SELECT asset_id FROM selected_assets)
        )
        SELECT selected_assets.asset_id, member.path
        FROM selected_assets
        JOIN asset_alias_members selected_member
          ON selected_member.repo_id = ? AND selected_member.asset_id = selected_assets.asset_id
        JOIN asset_alias_members member
          ON member.repo_id = selected_member.repo_id
         AND member.alias_group_id = selected_member.alias_group_id
        JOIN selected_groups
          ON selected_groups.alias_group_id = member.alias_group_id
        ORDER BY member.role DESC, member.path COLLATE NOCASE
        "#
    ))?;
    let params = asset_ids
        .iter()
        .map(String::as_str)
        .chain(std::iter::once(repo_id))
        .chain(std::iter::once(repo_id));
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for row in rows {
        let (asset_id, path) = row?;
        map.entry(asset_id).or_default().push(path);
    }
    Ok(map)
}

pub(super) fn load_folder_metadata_map_for_paths(
    connection: &Connection,
    repo_id: &str,
    paths: &[String],
) -> Result<BTreeMap<String, FolderMetadata>, rusqlite::Error> {
    if paths.is_empty() {
        return Ok(BTreeMap::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(paths.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        SELECT path, protected, password_tip
        FROM folder_metadata
        WHERE repo_id = ?1
          AND path IN ({placeholders})
        "#
    ))?;
    let params = std::iter::once(repo_id).chain(paths.iter().map(String::as_str));
    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok((
            row.get::<_, String>(0)?,
            FolderMetadata {
                protected: row.get::<_, i64>(1)? != 0,
                password_tip: row.get(2)?,
            },
        ))
    })?;
    rows.collect::<Result<BTreeMap<_, _>, _>>()
}

pub(super) fn load_tags_for_assets(
    connection: &Connection,
    asset_ids: &[String],
) -> Result<BTreeMap<String, Vec<String>>, rusqlite::Error> {
    if asset_ids.is_empty() {
        return Ok(BTreeMap::new());
    }

    let placeholders = std::iter::repeat("?")
        .take(asset_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let mut stmt = connection.prepare(&format!(
        r#"
        SELECT asset_id, tag
        FROM tags
        WHERE asset_id IN ({placeholders})
        ORDER BY normalized_tag COLLATE NOCASE
        "#
    ))?;
    let rows = stmt.query_map(rusqlite::params_from_iter(asset_ids.iter()), |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;

    let mut map = BTreeMap::new();
    for row in rows {
        let (asset_id, tag) = row?;
        map.entry(asset_id).or_insert_with(Vec::new).push(tag);
    }
    Ok(map)
}

pub(super) fn load_metadata_map_from_transaction(
    tx: &Transaction<'_>,
    asset_id: &str,
) -> Result<BTreeMap<String, serde_json::Value>, rusqlite::Error> {
    let mut stmt = tx.prepare(
        r#"
        SELECT key, value_json
        FROM metadata
        WHERE asset_id = ?1
        ORDER BY key COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| {
        let value_json: String = row.get(1)?;
        let value = parse_json_column(&value_json)?;
        Ok((row.get::<_, String>(0)?, value))
    })?;

    let pairs = rows.collect::<Result<Vec<_>, _>>()?;
    let mut metadata = pairs.into_iter().collect::<BTreeMap<_, _>>();
    normalize_loaded_metadata(&mut metadata);
    Ok(metadata)
}

pub(super) fn normalize_loaded_metadata(metadata: &mut BTreeMap<String, serde_json::Value>) {
    let comment_is_empty = metadata
        .get("comment")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .unwrap_or_default()
        .is_empty();
    if comment_is_empty {
        if let Some(note) = metadata.get("note").and_then(|value| value.as_str()) {
            if !note.trim().is_empty() {
                metadata.insert(
                    "comment".to_string(),
                    serde_json::Value::String(note.to_string()),
                );
            }
        }
    }
}

pub(super) fn normalize_metadata_entries(mut entries: Vec<MetadataEntry>) -> Vec<MetadataEntry> {
    let comment_is_empty = entries
        .iter()
        .find(|entry| entry.key == "comment")
        .and_then(|entry| entry.value.as_str())
        .map(str::trim)
        .unwrap_or_default()
        .is_empty();
    if !comment_is_empty {
        return entries;
    }
    let Some(note) = entries
        .iter()
        .find(|entry| entry.key == "note")
        .and_then(|entry| entry.value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
    else {
        return entries;
    };
    if let Some(comment) = entries.iter_mut().find(|entry| entry.key == "comment") {
        comment.value = serde_json::Value::String(note);
    } else {
        entries.push(MetadataEntry {
            key: "comment".to_string(),
            value_type: "string".to_string(),
            value: serde_json::Value::String(note),
            version: 1,
            updated_at: now_rfc3339(),
        });
        entries.sort_by(|left, right| left.key.to_lowercase().cmp(&right.key.to_lowercase()));
    }
    entries
}

pub(super) fn load_alias_member_asset_ids(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<Vec<String>, rusqlite::Error> {
    let alias_group_id = tx
        .query_row(
            r#"
            SELECT alias_group_id
            FROM asset_alias_members
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;

    let Some(alias_group_id) = alias_group_id else {
        return Ok(vec![asset_id.to_string()]);
    };

    let mut stmt = tx.prepare(
        r#"
        SELECT asset_id
        FROM asset_alias_members
        WHERE repo_id = ?1 AND alias_group_id = ?2
        ORDER BY role DESC, path COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map(params![repo_id, alias_group_id], |row| row.get(0))?;
    let mut asset_ids = rows.collect::<Result<Vec<String>, _>>()?;
    if !asset_ids.iter().any(|item| item == asset_id) {
        asset_ids.push(asset_id.to_string());
    }
    Ok(asset_ids)
}

pub(super) fn metadata_tags_from_tag_groups(value: Option<&serde_json::Value>) -> Vec<String> {
    let mut tags = Vec::new();
    if let Some(value) = value {
        collect_metadata_tags(value, &mut tags);
    }
    let mut seen = HashSet::new();
    tags.into_iter()
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .collect()
}

pub(super) fn collect_metadata_tags(value: &serde_json::Value, tags: &mut Vec<String>) {
    match value {
        serde_json::Value::String(tag) => tags.push(tag.clone()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_metadata_tags(item, tags);
            }
        }
        serde_json::Value::Object(map) => {
            for key in ["tags", "items", "children"] {
                if let Some(value) = map.get(key) {
                    collect_metadata_tags(value, tags);
                }
            }
            if let Some(label) = map
                .get("label")
                .or_else(|| map.get("name"))
                .and_then(|value| value.as_str())
            {
                tags.push(label.to_string());
            }
        }
        _ => {}
    }
}

pub(super) fn replace_asset_tags(
    tx: &Transaction<'_>,
    asset_id: &str,
    tags: &[String],
) -> Result<(), rusqlite::Error> {
    tx.execute("DELETE FROM tags WHERE asset_id = ?1", [asset_id])?;
    for tag in tags {
        tx.execute(
            r#"
            INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
            VALUES (?1, ?2, ?3)
            "#,
            params![asset_id, tag, tag.to_lowercase()],
        )?;
    }
    Ok(())
}

pub(super) fn load_revision_entries(
    connection: &Connection,
    asset_id: &str,
) -> Result<Vec<RevisionEntry>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT revision_id, asset_id, timestamp, operation, before_json, after_json, source
        FROM revisions
        WHERE asset_id = ?1
        ORDER BY timestamp DESC
        LIMIT 12
        "#,
    )?;

    let rows = stmt.query_map([asset_id], |row| {
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
    })?;

    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_metadata_fields(
    connection: &Connection,
) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT DISTINCT key
        FROM metadata
        ORDER BY key COLLATE NOCASE
        "#,
    )?;

    let rows = stmt.query_map([], |row| row.get(0))?;
    rows.collect::<Result<Vec<_>, _>>()
}

pub(super) fn load_asset_detail_from_connection(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
) -> Result<AssetDetail, rusqlite::Error> {
    let summary = load_asset_summary(connection, repo_id, asset_id)?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
    let metadata = normalize_metadata_entries(load_metadata_entries(connection, asset_id)?);
    let revisions = load_revision_entries(connection, asset_id)?;

    Ok(AssetDetail {
        summary,
        metadata,
        revisions,
    })
}

pub(super) fn load_asset_detail_from_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<AssetDetail, rusqlite::Error> {
    let summary = load_asset_summary_from_transaction(tx, repo_id, asset_id)?
        .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;

    let mut metadata_stmt = tx.prepare(
        r#"
        SELECT key, value_type, value_json, version, updated_at
        FROM metadata
        WHERE asset_id = ?1
        ORDER BY key COLLATE NOCASE
        "#,
    )?;
    let metadata_rows = metadata_stmt.query_map([asset_id], |row| {
        let value_json: String = row.get(2)?;
        let value = parse_json_column(&value_json)?;
        Ok(MetadataEntry {
            key: row.get(0)?,
            value_type: row.get(1)?,
            value,
            version: row.get(3)?,
            updated_at: row.get(4)?,
        })
    })?;
    let metadata = normalize_metadata_entries(metadata_rows.collect::<Result<Vec<_>, _>>()?);

    let mut revision_stmt = tx.prepare(
        r#"
        SELECT revision_id, asset_id, timestamp, operation, before_json, after_json, source
        FROM revisions
        WHERE asset_id = ?1
        ORDER BY timestamp DESC
        LIMIT 12
        "#,
    )?;
    let revision_rows = revision_stmt.query_map([asset_id], |row| {
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
    })?;
    let revisions = revision_rows.collect::<Result<Vec<_>, _>>()?;

    Ok(AssetDetail {
        summary,
        metadata,
        revisions,
    })
}

pub(super) fn load_asset_summary(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
) -> Result<Option<AssetSummary>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT
              a.asset_id,
              a.repo_id,
              a.path,
              a.filename,
              a.extension,
              a.size_bytes,
              a.status,
              a.modified_at,
              a.last_accessed_at,
              a.version,
              a.thumbnail_path,
              hm.group_id,
              hm.link_state,
              a.is_virtual,
              a.provider_id,
              a.provider_item_id,
              a.source_payload_json,
              a.local_absolute_path
            FROM assets a
            LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
            WHERE a.repo_id = ?1 AND a.asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                    row.get::<_, Option<String>>(17)?,
                ))
            },
        )
        .optional()?
        .map(
            |(
                asset_id,
                repo_id,
                path,
                filename,
                extension,
                size_bytes,
                status,
                modified_at,
                last_accessed_at,
                version,
                thumbnail_path,
                hardlink_group_id,
                hardlink_state,
                is_virtual,
                provider_id,
                provider_item_id,
                source_payload_json,
                local_absolute_path,
            )| {
                let tags = load_tags(connection, &asset_id)?;
                Ok(AssetSummary {
                    asset_id,
                    repo_id,
                    path,
                    filename,
                    extension,
                    size_bytes,
                    size_label: format_size_label(size_bytes),
                    status,
                    modified_at,
                    last_accessed_at,
                    version,
                    tags,
                    thumbnail_path,
                    hardlink_group_id,
                    hardlink_state,
                    is_virtual: is_virtual != 0,
                    provider_id,
                    provider_item_id,
                    source_payload: parse_json_column_nullable(source_payload_json)?,
                    local_absolute_path,
                })
            },
        )
        .transpose()
}

pub(super) fn load_asset_summary_from_transaction(
    tx: &Transaction<'_>,
    repo_id: &str,
    asset_id: &str,
) -> Result<Option<AssetSummary>, rusqlite::Error> {
    let base = tx
        .query_row(
            r#"
            SELECT
              a.asset_id,
              a.repo_id,
              a.path,
              a.filename,
              a.extension,
              a.size_bytes,
              a.status,
              a.modified_at,
              a.last_accessed_at,
              a.version,
              a.thumbnail_path,
              hm.group_id,
              hm.link_state,
              a.is_virtual,
              a.provider_id,
              a.provider_item_id,
              a.source_payload_json,
              a.local_absolute_path
            FROM assets a
            LEFT JOIN hardlink_members hm ON hm.repo_id = a.repo_id AND hm.asset_id = a.asset_id
            WHERE a.repo_id = ?1 AND a.asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, Option<String>>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, Option<String>>(16)?,
                    row.get::<_, Option<String>>(17)?,
                ))
            },
        )
        .optional()?;

    let Some((
        asset_id,
        repo_id,
        path,
        filename,
        extension,
        size_bytes,
        status,
        modified_at,
        last_accessed_at,
        version,
        thumbnail_path,
        hardlink_group_id,
        hardlink_state,
        is_virtual,
        provider_id,
        provider_item_id,
        source_payload_json,
        local_absolute_path,
    )) = base
    else {
        return Ok(None);
    };

    let mut tag_stmt = tx.prepare(
        r#"
        SELECT tag
        FROM tags
        WHERE asset_id = ?1
        ORDER BY normalized_tag COLLATE NOCASE
        "#,
    )?;
    let tag_rows = tag_stmt.query_map([asset_id.as_str()], |row| row.get(0))?;
    let tags = tag_rows.collect::<Result<Vec<String>, _>>()?;

    Ok(Some(AssetSummary {
        asset_id,
        repo_id,
        path,
        filename,
        extension,
        size_bytes,
        size_label: format_size_label(size_bytes),
        status,
        modified_at,
        last_accessed_at,
        version,
        tags,
        thumbnail_path,
        hardlink_group_id,
        hardlink_state,
        is_virtual: is_virtual != 0,
        provider_id,
        provider_item_id,
        source_payload: parse_json_column_nullable(source_payload_json)?,
        local_absolute_path,
    }))
}
