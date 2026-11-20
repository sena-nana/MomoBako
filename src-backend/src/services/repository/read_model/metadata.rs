//! 资源元数据、标签、别名路径的批量读取与事务内更新。

use super::*;

pub(in super::super) fn update_metadata_for_asset_in_transaction(
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
        let revision_id = format!("rev-{}-{}", target_asset_id, target_version);
        tx.execute(
            r#"
            INSERT INTO revisions (
              revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
            )
            VALUES (?1, ?2, ?3, ?4, 'metadata.updated', ?5, ?6, ?7)
            "#,
            params![
                revision_id,
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
        save_revision_cursor(tx, repo_id, &target_asset_id, Some(&revision_id), None)?;
    }
    Ok(())
}

pub(in super::super) fn load_assets(
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

pub(in super::super) fn load_tags(
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

pub(in super::super) fn load_metadata_entries(
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

pub(in super::super) fn load_metadata_map(
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

pub(in super::super) fn load_metadata_maps_for_assets(
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

pub(in super::super) fn load_alias_paths_for_assets(
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

pub(in super::super) fn load_folder_metadata_map_for_paths(
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

pub(in super::super) fn load_tags_for_assets(
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

pub(in super::super) fn load_metadata_map_from_transaction(
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

pub(in super::super) fn normalize_loaded_metadata(
    metadata: &mut BTreeMap<String, serde_json::Value>,
) {
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

pub(in super::super) fn normalize_metadata_entries(
    mut entries: Vec<MetadataEntry>,
) -> Vec<MetadataEntry> {
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

pub(in super::super) fn load_alias_member_asset_ids(
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

pub(in super::super) fn metadata_tags_from_tag_groups(
    value: Option<&serde_json::Value>,
) -> Vec<String> {
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

pub(in super::super) fn collect_metadata_tags(value: &serde_json::Value, tags: &mut Vec<String>) {
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

pub(in super::super) fn replace_asset_tags(
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
