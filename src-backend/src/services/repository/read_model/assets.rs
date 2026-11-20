//! 修订记录、元数据字段与资源详情、摘要的读取。

use super::*;

pub(in super::super) fn load_revision_entries(
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

pub(in super::super) fn load_metadata_fields(
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

pub(in super::super) fn load_asset_detail_from_connection(
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

pub(in super::super) fn load_asset_detail_from_transaction(
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

pub(in super::super) fn load_asset_summary(
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

pub(in super::super) fn load_asset_summary_from_transaction(
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
