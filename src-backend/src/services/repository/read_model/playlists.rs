//! 文件夹摘要、快捷方式、播放列表、标签组与仓库动作的读取。

use super::*;

pub(in super::super) fn load_folder_summaries(
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

pub(in super::super) fn load_repository_shortcuts(
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

pub(in super::super) fn load_playlists(
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

pub(in super::super) fn load_playlist_summary(
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

pub(in super::super) fn load_playlist_memberships(
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

pub(in super::super) fn load_playlist_detail(
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
            let metadata = path
                .as_ref()
                .map(|_| load_metadata_map(connection, &asset_id))
                .transpose()?
                .map(|values| serde_json::Value::Object(values.into_iter().collect()));
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
                metadata,
                local_absolute_path,
            })
        })
        .collect::<Result<Vec<_>, rusqlite::Error>>()?;

    Ok(PlaylistDetail { playlist, items })
}

pub(in super::super) fn resolve_playlist_item_status(
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

pub(in super::super) fn playlist_player_supports_extension(
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

pub(in super::super) fn next_playlist_sort_order(
    connection: &Connection,
    repo_id: &str,
) -> Result<i64, rusqlite::Error> {
    connection.query_row(
        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM playlists WHERE repo_id = ?1",
        [repo_id],
        |row| row.get(0),
    )
}

pub(in super::super) fn next_playlist_item_sort_order(
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

pub(in super::super) fn validate_playlist_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("playlist name cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

pub(in super::super) fn validate_playlist_id(id: &str) -> Result<String, String> {
    let trimmed = id.trim();
    if trimmed.is_empty() {
        return Err("playlist id cannot be empty".to_string());
    }
    Ok(trimmed.to_string())
}

pub(in super::super) fn playlist_id_for(repo_id: &str, name: &str) -> String {
    format!(
        "playlist-{}",
        sha256_hex(&[repo_id.as_bytes(), name.trim().as_bytes()])
    )
}

pub(in super::super) fn playlist_item_id_for(playlist_id: &str, asset_id: &str) -> String {
    format!(
        "playlist-item-{}",
        sha256_hex(&[playlist_id.as_bytes(), asset_id.as_bytes()])
    )
}

pub(in super::super) fn normalize_id_list(values: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .filter(|item| seen.insert(item.clone()))
        .collect()
}

pub(in super::super) fn load_repository_tag_groups(
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

pub(in super::super) fn load_repository_actions(
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

pub(in super::super) fn load_repository_action(
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

pub(in super::super) fn load_repository_action_steps(
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

pub(in super::super) fn load_repository_action_last_run(
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

pub(in super::super) fn resolve_action_target_asset_ids(
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

pub(in super::super) fn apply_repository_action_step(
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
