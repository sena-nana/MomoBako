//! Source 仓库状态同步与回写辅助。
//!
//! 该模块负责承接 source 插件补充提供的仓库级对象、托管回收站记录，
//! 并为元数据/仓库状态回写构造宿主快照。

use super::*;
use std::collections::BTreeSet;

/// 读取整个仓库的目录保护信息。
pub(super) fn load_directory_metadata_by_path(
    connection: &Connection,
    repo_id: &str,
) -> Result<BTreeMap<String, FolderMetadata>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT path, protected, password_tip
        FROM folder_metadata
        WHERE repo_id = ?1
        ORDER BY path COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
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

/// 读取 source 托管的回收站记录。
pub(super) fn load_source_trash_entries(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<SourceTrashEntryRecord>, rusqlite::Error> {
    let mut stmt = connection.prepare(
        r#"
        SELECT trash_path, original_path, kind, deleted_at, shared_asset_id
        FROM source_trash_entries
        WHERE repo_id = ?1
        ORDER BY trash_path COLLATE NOCASE
        "#,
    )?;
    let rows = stmt.query_map([repo_id], |row| {
        Ok(SourceTrashEntryRecord {
            trash_path: row.get(0)?,
            original_path: row.get(1)?,
            kind: row.get(2)?,
            deleted_at: row.get(3)?,
            shared_asset_id: row.get(4)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>()
}

/// 读取指定 trashPath 对应的 source 回收站记录。
pub(super) fn load_source_trash_entry(
    connection: &Connection,
    repo_id: &str,
    trash_path: &str,
) -> Result<Option<SourceTrashEntryRecord>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT trash_path, original_path, kind, deleted_at, shared_asset_id
            FROM source_trash_entries
            WHERE repo_id = ?1 AND trash_path = ?2
            "#,
            params![repo_id, trash_path],
            |row| {
                Ok(SourceTrashEntryRecord {
                    trash_path: row.get(0)?,
                    original_path: row.get(1)?,
                    kind: row.get(2)?,
                    deleted_at: row.get(3)?,
                    shared_asset_id: row.get(4)?,
                })
            },
        )
        .optional()
}

/// 读取当前仓库的完整 source 状态快照，用于 source 回写。
pub(super) fn load_source_repository_state_snapshot(
    connection: &Connection,
    repo_id: &str,
) -> Result<SourceRepositoryStateSnapshot, rusqlite::Error> {
    let smart_folders = load_smart_folders(connection, repo_id)?;
    let trash_entries = load_source_trash_entries(connection, repo_id)?
        .into_iter()
        .map(|entry| SourceTrashEntry {
            trash_path: entry.trash_path,
            original_path: entry.original_path,
            kind: entry.kind,
            deleted_at: entry.deleted_at,
            shared_asset_id: entry.shared_asset_id,
        })
        .collect();
    Ok(SourceRepositoryStateSnapshot {
        directory_metadata_by_path: load_directory_metadata_by_path(connection, repo_id)?,
        quick_access: load_repository_shortcuts(connection, repo_id)?,
        tag_groups: load_repository_tag_groups(connection, repo_id)?,
        smart_folders,
        repository_actions: load_repository_actions(connection, repo_id)?,
        trash_entries,
    })
}

/// 读取素材写回到 source 插件时需要的路径与 sharedAssetId。
pub(super) fn load_source_asset_writeback_target(
    connection: &Connection,
    repo_id: &str,
    asset_id: &str,
) -> Result<Option<(String, Option<String>)>, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT path, source_payload_json
            FROM assets
            WHERE repo_id = ?1 AND asset_id = ?2
            "#,
            params![repo_id, asset_id],
            |row| {
                let path = row.get::<_, String>(0)?;
                let source_payload_json = row.get::<_, Option<String>>(1)?;
                let shared_asset_id = parse_json_column_nullable(source_payload_json)?
                    .and_then(|value| value.get("sharedAssetId").cloned())
                    .and_then(|value| value.as_str().map(str::to_string));
                Ok((path, shared_asset_id))
            },
        )
        .optional()
}

/// 用 source 插件返回的快照全量替换宿主读模型。
pub(super) fn replace_source_repository_state(
    connection: &Connection,
    repo_id: &str,
    state: &SourceRepositoryStateSnapshot,
) -> Result<(), rusqlite::Error> {
    replace_folder_metadata_records(connection, repo_id, &state.directory_metadata_by_path)?;
    replace_repository_shortcuts(connection, repo_id, &state.quick_access)?;
    replace_repository_tag_groups(connection, repo_id, &state.tag_groups)?;
    replace_repository_smart_folders(connection, repo_id, &state.smart_folders)?;
    replace_repository_actions(connection, repo_id, &state.repository_actions)?;
    replace_source_trash_entries(connection, repo_id, &state.trash_entries)?;
    Ok(())
}

/// 将 source 托管回收站映射为现有文件浏览模型。
pub(super) fn list_source_trash_directory_entries(
    current_path: &str,
    source_entries: &[SourceTrashEntryRecord],
    asset_map: &BTreeMap<String, AssetPathRecord>,
    thumbnail_map: &BTreeMap<(String, String), ThumbnailRecord>,
) -> Vec<FileBrowserEntry> {
    let current = normalize_trash_relative_path(current_path, true).unwrap_or_default();
    let mut directory_names = BTreeSet::new();
    let mut entries = Vec::new();

    for entry in source_entries {
        let Some(suffix) = relative_suffix(&entry.trash_path, &current) else {
            continue;
        };
        if suffix.is_empty() {
            continue;
        }
        let segments = suffix.split('/').collect::<Vec<_>>();
        let child_name = segments[0].to_string();
        let child_path = if current.is_empty() {
            child_name.clone()
        } else {
            join_relative_path(&current, &child_name)
        };
        if segments.len() > 1 {
            if directory_names.insert(child_path.clone()) {
                entries.push(FileBrowserEntry {
                    path: child_path,
                    name: child_name,
                    kind: "directory".to_string(),
                    extension: None,
                    size_bytes: None,
                    size_label: None,
                    modified_at: Some(entry.deleted_at.clone()),
                    asset_id: None,
                    status: Some("deleted".to_string()),
                    thumbnail_path: None,
                    thumbnail_custom: false,
                    hardlink_group_id: None,
                    hardlink_state: None,
                    tags: Vec::new(),
                    alias_paths: Vec::new(),
                    folder_metadata: None,
                    metadata: BTreeMap::new(),
                    is_virtual: false,
                    provider_id: None,
                    provider_item_id: None,
                    source_payload: None,
                    local_absolute_path: None,
                });
            }
            continue;
        }
        entries.push(map_single_source_trash_entry(
            entry,
            asset_map,
            thumbnail_map,
        ));
    }

    entries.sort_by(
        |left, right| match (left.kind.as_str(), right.kind.as_str()) {
            ("directory", "file") => std::cmp::Ordering::Less,
            ("file", "directory") => std::cmp::Ordering::Greater,
            _ => left.name.to_lowercase().cmp(&right.name.to_lowercase()),
        },
    );
    entries
}

/// 判断仓库是否已经存在 source 托管回收站记录。
pub(super) fn repository_has_source_trash_entries(
    connection: &Connection,
    repo_id: &str,
) -> Result<bool, rusqlite::Error> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM source_trash_entries WHERE repo_id = ?1)",
            [repo_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
}

fn map_single_source_trash_entry(
    entry: &SourceTrashEntryRecord,
    asset_map: &BTreeMap<String, AssetPathRecord>,
    thumbnail_map: &BTreeMap<(String, String), ThumbnailRecord>,
) -> FileBrowserEntry {
    let asset_record = asset_map.get(&entry.original_path);
    let asset_id = asset_record.map(|record| record.asset_id.clone());
    let asset_thumbnail_path = asset_record.and_then(|record| record.thumbnail_path.clone());
    let hardlink_group_id = asset_record.and_then(|record| record.hardlink_group_id.clone());
    let hardlink_state = asset_record.and_then(|record| record.hardlink_state.clone());
    let is_virtual = asset_record
        .map(|record| record.is_virtual)
        .unwrap_or(false);
    let provider_id = asset_record.and_then(|record| record.provider_id.clone());
    let provider_item_id = asset_record.and_then(|record| record.provider_item_id.clone());
    let source_payload = asset_record.and_then(|record| record.source_payload.clone());
    let local_absolute_path = asset_record.and_then(|record| record.local_absolute_path.clone());
    let entry_thumbnail = thumbnail_map.get(&(entry.original_path.clone(), "file".to_string()));
    let thumbnail_path = entry_thumbnail
        .map(|record| record.path.clone())
        .or(asset_thumbnail_path);
    let thumbnail_custom = entry_thumbnail.map(|record| record.custom).unwrap_or(false);
    let extension = Path::new(&entry.original_path)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string());
    let name = Path::new(&entry.trash_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(entry.trash_path.as_str())
        .to_string();
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "deletedAt".to_string(),
        serde_json::Value::String(entry.deleted_at.clone()),
    );
    metadata.insert(
        "originalPath".to_string(),
        serde_json::Value::String(entry.original_path.clone()),
    );
    FileBrowserEntry {
        path: entry.trash_path.clone(),
        name,
        kind: "file".to_string(),
        extension,
        size_bytes: None,
        size_label: None,
        modified_at: Some(entry.deleted_at.clone()),
        asset_id,
        status: Some("deleted".to_string()),
        thumbnail_path,
        thumbnail_custom,
        hardlink_group_id,
        hardlink_state,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata,
        is_virtual,
        provider_id,
        provider_item_id,
        source_payload,
        local_absolute_path,
    }
}

fn replace_folder_metadata_records(
    connection: &Connection,
    repo_id: &str,
    entries: &BTreeMap<String, FolderMetadata>,
) -> Result<(), rusqlite::Error> {
    connection.execute("DELETE FROM folder_metadata WHERE repo_id = ?1", [repo_id])?;
    let now = now_rfc3339();
    for (path, metadata) in entries {
        connection.execute(
            r#"
            INSERT INTO folder_metadata (repo_id, path, protected, password_tip, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                repo_id,
                path,
                if metadata.protected { 1 } else { 0 },
                metadata.password_tip,
                now
            ],
        )?;
    }
    Ok(())
}

fn replace_repository_shortcuts(
    connection: &Connection,
    repo_id: &str,
    entries: &[RepositoryShortcut],
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM repository_shortcuts WHERE repo_id = ?1",
        [repo_id],
    )?;
    let now = now_rfc3339();
    for (index, entry) in entries.iter().enumerate() {
        connection.execute(
            r#"
            INSERT INTO repository_shortcuts (
              shortcut_id, repo_id, label, target_kind, target_path, target_id, sort_order, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                entry.shortcut_id,
                repo_id,
                entry.label,
                entry.target_kind,
                entry.target_path,
                entry.target_id,
                index as i64,
                now
            ],
        )?;
    }
    Ok(())
}

fn replace_repository_tag_groups(
    connection: &Connection,
    repo_id: &str,
    entries: &[RepositoryTagGroup],
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM tag_group_members WHERE repo_id = ?1",
        [repo_id],
    )?;
    connection.execute("DELETE FROM tag_groups WHERE repo_id = ?1", [repo_id])?;
    let now = now_rfc3339();
    for (group_index, entry) in entries.iter().enumerate() {
        connection.execute(
            r#"
            INSERT INTO tag_groups (tag_group_id, repo_id, name, sort_order, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                entry.tag_group_id,
                repo_id,
                entry.name,
                group_index as i64,
                now,
                now
            ],
        )?;
        for (member_index, tag) in entry.tags.iter().enumerate() {
            connection.execute(
                r#"
                INSERT INTO tag_group_members (tag_group_id, repo_id, tag, normalized_tag, sort_order)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![
                    entry.tag_group_id,
                    repo_id,
                    tag,
                    tag.to_lowercase(),
                    member_index as i64
                ],
            )?;
        }
    }
    Ok(())
}

fn replace_repository_smart_folders(
    connection: &Connection,
    repo_id: &str,
    entries: &[SmartFolder],
) -> Result<(), rusqlite::Error> {
    connection.execute("DELETE FROM smart_folders WHERE repo_id = ?1", [repo_id])?;
    for entry in entries {
        connection.execute(
            r#"
            INSERT INTO smart_folders (
              smart_folder_id, repo_id, parent_id, name, filter_json, sort_order, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                entry.smart_folder_id,
                repo_id,
                entry.parent_id,
                entry.name,
                serde_json::to_string(&entry.filter)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
                entry.sort_order,
                entry.created_at,
                entry.updated_at,
            ],
        )?;
    }
    Ok(())
}

fn replace_repository_actions(
    connection: &Connection,
    repo_id: &str,
    entries: &[RepositoryAction],
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM repository_action_run_steps WHERE repo_id = ?1",
        [repo_id],
    )?;
    connection.execute(
        "DELETE FROM repository_action_runs WHERE repo_id = ?1",
        [repo_id],
    )?;
    connection.execute(
        "DELETE FROM repository_action_steps WHERE repo_id = ?1",
        [repo_id],
    )?;
    connection.execute(
        "DELETE FROM repository_actions WHERE repo_id = ?1",
        [repo_id],
    )?;
    for entry in entries {
        connection.execute(
            r#"
            INSERT INTO repository_actions (
              action_id, repo_id, source, source_action_id, name, status, enabled, raw_json,
              unsupported_reason, sort_order, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                entry.action_id,
                repo_id,
                entry.source,
                entry.source_action_id,
                entry.name,
                entry.status,
                if entry.enabled { 1 } else { 0 },
                serde_json::to_string(&entry.raw)
                    .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?,
                entry.unsupported_reason,
                entry.sort_order,
                entry.created_at,
                entry.updated_at,
            ],
        )?;
        for step in &entry.steps {
            connection.execute(
                r#"
                INSERT INTO repository_action_steps (
                  step_id, action_id, repo_id, step_kind, label, status, config_json, raw_json,
                  unsupported_reason, sort_order
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    step.step_id,
                    entry.action_id,
                    repo_id,
                    step.step_kind,
                    step.label,
                    step.status,
                    serde_json::to_string(&step.config).map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                    })?,
                    serde_json::to_string(&step.raw).map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                    })?,
                    step.unsupported_reason,
                    step.sort_order,
                ],
            )?;
        }
    }
    Ok(())
}

fn replace_source_trash_entries(
    connection: &Connection,
    repo_id: &str,
    entries: &[SourceTrashEntry],
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "DELETE FROM source_trash_entries WHERE repo_id = ?1",
        [repo_id],
    )?;
    for entry in entries {
        connection.execute(
            r#"
            INSERT INTO source_trash_entries (
              repo_id, trash_path, original_path, kind, deleted_at, shared_asset_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                repo_id,
                entry.trash_path,
                entry.original_path,
                entry.kind,
                entry.deleted_at,
                entry.shared_asset_id,
            ],
        )?;
    }
    Ok(())
}
