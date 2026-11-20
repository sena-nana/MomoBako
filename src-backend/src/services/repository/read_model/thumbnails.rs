//! 资源路径映射、条目缩略图记录与缩略图路径规范化。

use super::*;

pub(in super::super) fn load_active_asset_count(
    connection: &Connection,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COUNT(*) FROM assets WHERE status != 'deleted'",
        [],
        |row| row.get(0),
    )
}

pub(in super::super) fn load_asset_path_map(
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

pub(in super::super) fn load_asset_path_map_for_paths(
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

pub(in super::super) fn load_entry_thumbnail_map(
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

pub(in super::super) fn load_entry_thumbnail_map_for_paths(
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

pub(in super::super) fn load_entry_thumbnail_record(
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

pub(in super::super) fn upsert_entry_thumbnail_record(
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

pub(in super::super) fn remove_entry_thumbnail_record(
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

pub(in super::super) fn update_asset_thumbnail_path(
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

pub(in super::super) fn normalize_asset_summaries(
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

pub(in super::super) fn normalize_asset_thumbnail_map(
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

pub(in super::super) fn normalize_asset_thumbnail_path(
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

pub(in super::super) fn normalize_entry_thumbnail_map(
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

pub(in super::super) fn normalize_entry_thumbnail_record(
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

pub(in super::super) fn normalize_thumbnail_path(
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
