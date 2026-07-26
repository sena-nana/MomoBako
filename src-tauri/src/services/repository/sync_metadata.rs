//! Repository metadata, revision and event synchronization helpers.

use super::*;

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
    let target_asset_ids = load_alias_member_asset_ids(tx, repo_id, asset_id)?;
    let now = now_rfc3339();

    for target_asset_id in target_asset_ids {
        let member_before = load_metadata_map_from_transaction(tx, &target_asset_id)?;
        tx.execute(
            "DELETE FROM metadata WHERE asset_id = ?1",
            [&target_asset_id],
        )?;
        for (key, value) in &target_map {
            tx.execute(
                r#"
                INSERT INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                VALUES (?1, ?2, ?3, ?4, 1, ?5)
                "#,
                params![
                    target_asset_id,
                    key,
                    infer_value_type(value),
                    value.to_string(),
                    now
                ],
            )?;
        }
        if target_map.contains_key("tagGroups") {
            replace_asset_tags(
                tx,
                &target_asset_id,
                &metadata_tags_from_tag_groups(target_map.get("tagGroups")),
            )?;
        }

        let next_version: i64 = tx.query_row(
            "SELECT version + 1 FROM assets WHERE repo_id = ?1 AND asset_id = ?2",
            params![repo_id, target_asset_id],
            |row| row.get(0),
        )?;

        tx.execute(
            "UPDATE assets SET version = ?3, updated_at = ?4, modified_at = ?4 WHERE repo_id = ?1 AND asset_id = ?2",
            params![repo_id, target_asset_id, next_version, now],
        )?;
        tx.execute(
            r#"
            INSERT INTO revisions (
              revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                format!("rev-{}-{}", target_asset_id, next_version),
                repo_id,
                target_asset_id,
                now,
                operation,
                serde_json::to_string(&member_before)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
                serde_json::to_string(&target_map)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
                source
            ],
        )?;
    }

    Ok(())
}

pub(super) fn source_metadata_mirror_keys(service_root: &Path, plugin_id: &str) -> Vec<String> {
    plugin_catalog(service_root)
        .manifest(plugin_id)
        .and_then(|manifest| manifest.contributes.as_object())
        .and_then(|contributes| contributes.get("source"))
        .and_then(serde_json::Value::as_object)
        .and_then(|source| source.get("metadataMirrorKeys"))
        .and_then(serde_json::Value::as_array)
        .map(|keys| {
            keys.iter()
                .filter_map(serde_json::Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn sync_mirrored_source_metadata(
    connection: &Connection,
    asset_id: &str,
    source_payload: Option<&serde_json::Value>,
    metadata_keys: &[String],
) -> Result<(), rusqlite::Error> {
    if metadata_keys.is_empty() {
        return Ok(());
    }
    let Some(source_payload) = source_payload else {
        return Ok(());
    };
    for key in metadata_keys {
        if let Some(value) = source_payload.get(key).cloned() {
            upsert_metadata_value(connection, asset_id, key, &value)?;
        }
    }
    Ok(())
}

pub(super) fn rebuild_netease_directory_cache(
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
        groups
            .entry(directory_path)
            .or_default()
            .push(FileSystemEntry {
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
                status: file.status.clone(),
                shared_asset_id: file.shared_asset_id.clone(),
                tags: file.tags.clone(),
                thumbnail_local_absolute_path: file.thumbnail_local_absolute_path.clone(),
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
