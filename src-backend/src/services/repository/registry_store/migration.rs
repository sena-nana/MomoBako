//! 注册表与仓库数据库的结构迁移、旧网易云记录迁移与后端插件推断。

use super::*;

pub(in super::super) fn migrate_registry_schema(
    registry: &Connection,
) -> Result<(), rusqlite::Error> {
    let mut stmt = registry.prepare("PRAGMA table_info(repositories)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|column| column == "backend_plugin_id") {
        registry.execute(
            "ALTER TABLE repositories ADD COLUMN backend_plugin_id TEXT NOT NULL DEFAULT 'momobako.local-filesystem'",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "backend_config_json") {
        registry.execute(
            "ALTER TABLE repositories ADD COLUMN backend_config_json TEXT NOT NULL DEFAULT '{}'",
            [],
        )?;
    }
    registry.execute(
        "UPDATE repositories SET backend_plugin_id = ?1 WHERE backend_plugin_id = ?2",
        params![
            NETEASE_CLOUD_MUSIC_PLUGIN_ID,
            LEGACY_NETEASE_CLOUD_MUSIC_PLUGIN_ID
        ],
    )?;
    Ok(())
}

/// 幂等迁移网易云仓库 ID、公开配置和仓库元数据；凭据写入失败时保留旧配置以便重试。
pub(in super::super) fn migrate_netease_repository_records(
    service_root: &Path,
    registry: &Connection,
) -> Result<(), String> {
    let mut stmt = registry
        .prepare(
            r#"
            SELECT repo_id, name, path, backend_config_json, created_at
            FROM repositories
            WHERE backend_plugin_id = ?1
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([NETEASE_CLOUD_MUSIC_PLUGIN_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)?;
    drop(stmt);

    for (repo_id, name, path, config_json, created_at) in rows {
        let mut config = parse_backend_config_json(&config_json).map_err(json_error)?;
        let has_legacy_cookie = ["cookie", "accountCookie"].iter().any(|key| {
            config
                .get(key)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        });
        if has_legacy_cookie {
            match plugin_catalog(service_root).call(
                NETEASE_CLOUD_MUSIC_PLUGIN_ID,
                "auth.migrateRepositoryCredential",
                serde_json::json!({ "config": config }),
            ) {
                Ok(response) => {
                    let credential_ref = response
                        .get("credentialRef")
                        .and_then(serde_json::Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            "credential migration did not return credentialRef".to_string()
                        })?;
                    let object = config
                        .as_object_mut()
                        .ok_or_else(|| "repository backend config must be an object".to_string())?;
                    object.remove("cookie");
                    object.remove("accountCookie");
                    object.insert(
                        "credentialRef".to_string(),
                        serde_json::json!(credential_ref),
                    );
                    registry
                        .execute(
                            r#"
                            UPDATE repositories
                            SET backend_config_json = ?2, updated_at = ?3
                            WHERE repo_id = ?1
                            "#,
                            params![repo_id, config.to_string(), now_rfc3339()],
                        )
                        .map_err(db_error)?;
                }
                Err(error) => {
                    crate::app_log!(
                        "warn",
                        "repository.migration",
                        "sourceCredentialMigrationDeferred",
                        "来源凭据迁移暂未完成，将在下次启动重试。",
                        serde_json::json!({ "repoId": repo_id, "error": error })
                    );
                    continue;
                }
            }
        }

        let storage = ensure_repository_storage_paths(
            service_root,
            &repo_id,
            Path::new(&path),
            NETEASE_CLOUD_MUSIC_PLUGIN_ID,
        )?;
        if let Err(error) = write_repository_metadata(
            &storage.metadata_dir,
            &repo_id,
            &name,
            Path::new(&path),
            NETEASE_CLOUD_MUSIC_PLUGIN_ID,
            &config,
            Some(created_at),
        ) {
            crate::app_log!(
                "warn",
                "repository.migration",
                "sourceMetadataMigrationDeferred",
                "来源仓库元数据迁移暂未完成，将在下次启动重试。",
                serde_json::json!({ "repoId": repo_id, "error": error })
            );
        }
    }
    Ok(())
}

pub(super) fn infer_backend_plugin_id_for_path(
    service_root: &Path,
    path: &str,
) -> Result<String, String> {
    let trimmed = path.trim();
    let requires_local_root = !trimmed.contains("://");
    let registry = plugin_catalog(service_root);
    let matching_manifests = registry
        .list_manifests()
        .into_iter()
        .filter(is_source_plugin)
        .filter(|manifest| {
            manifest
                .capabilities
                .iter()
                .any(|value| value == LOCAL_ROOT_PATH_CAPABILITY)
                == requires_local_root
        })
        .collect::<Vec<_>>();
    let unavailable = matching_manifests
        .iter()
        .filter(|manifest| {
            registry
                .registration(&manifest.plugin_id)
                .is_some_and(|registration| {
                    ensure_repository_backend_runtime_available(registration).is_err()
                })
        })
        .map(|manifest| manifest.plugin_id.clone())
        .collect::<Vec<_>>();
    let mut candidates = matching_manifests
        .into_iter()
        .filter(|manifest| {
            registry
                .registration(&manifest.plugin_id)
                .is_some_and(|registration| {
                    ensure_repository_backend_runtime_available(registration).is_ok()
                })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.plugin_id.cmp(&right.plugin_id));

    match candidates.as_slice() {
        [manifest] => Ok(manifest.plugin_id.clone()),
        [] if requires_local_root => {
            if unavailable.is_empty() {
                Err(
                    "no installed repository source plugin supports local repository roots"
                        .to_string(),
                )
            } else {
                Err(format!(
                    "repository source plugin runtime is not available: {}",
                    unavailable.join(", ")
                ))
            }
        }
        [] => Err("backend plugin id is required for non-local repository paths".to_string()),
        _ => Err(format!(
            "multiple repository source plugins match this path, backend plugin id is required: {}",
            candidates
                .iter()
                .map(|manifest| manifest.plugin_id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub(in super::super) fn migrate_repository_schema(
    connection: &Connection,
) -> Result<(), rusqlite::Error> {
    configure_repository_connection(connection)?;
    connection.execute_batch(REPOSITORY_SCHEMA_SQL)?;
    let mut stmt = connection.prepare("PRAGMA table_info(assets)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    if !columns.iter().any(|column| column == "thumbnail_path") {
        connection.execute("ALTER TABLE assets ADD COLUMN thumbnail_path TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "is_virtual") {
        connection.execute(
            "ALTER TABLE assets ADD COLUMN is_virtual INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !columns.iter().any(|column| column == "provider_id") {
        connection.execute("ALTER TABLE assets ADD COLUMN provider_id TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "provider_item_id") {
        connection.execute("ALTER TABLE assets ADD COLUMN provider_item_id TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "source_payload_json") {
        connection.execute("ALTER TABLE assets ADD COLUMN source_payload_json TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "local_absolute_path") {
        connection.execute("ALTER TABLE assets ADD COLUMN local_absolute_path TEXT", [])?;
    }
    if !columns.iter().any(|column| column == "last_accessed_at") {
        connection.execute("ALTER TABLE assets ADD COLUMN last_accessed_at TEXT", [])?;
    }
    connection.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS directories (
          repo_id TEXT NOT NULL,
          path TEXT NOT NULL,
          parent_path TEXT NOT NULL,
          name TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, path),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_directories_repo_parent
        ON directories(repo_id, parent_path, name);

        CREATE INDEX IF NOT EXISTS idx_assets_repo_hash ON assets(repo_id, hash);

        CREATE TABLE IF NOT EXISTS entry_thumbnails (
          repo_id TEXT NOT NULL,
          path TEXT NOT NULL,
          kind TEXT NOT NULL,
          thumbnail_path TEXT NOT NULL,
          custom INTEGER NOT NULL DEFAULT 0,
          updated_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, path, kind)
        );

        CREATE TABLE IF NOT EXISTS source_trash_entries (
          repo_id TEXT NOT NULL,
          trash_path TEXT NOT NULL,
          original_path TEXT NOT NULL,
          kind TEXT NOT NULL,
          deleted_at TEXT NOT NULL,
          shared_asset_id TEXT,
          PRIMARY KEY(repo_id, trash_path),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS netease_directory_cache (
          repo_id TEXT NOT NULL,
          directory_path TEXT NOT NULL,
          total_entries INTEGER NOT NULL,
          refreshed_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, directory_path),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS netease_directory_entries (
          repo_id TEXT NOT NULL,
          directory_path TEXT NOT NULL,
          order_index INTEGER NOT NULL,
          path TEXT NOT NULL,
          name TEXT NOT NULL,
          kind TEXT NOT NULL,
          extension TEXT,
          size_bytes INTEGER,
          modified_at TEXT,
          is_virtual INTEGER NOT NULL DEFAULT 0,
          provider_id TEXT,
          provider_item_id TEXT,
          source_payload_json TEXT,
          local_absolute_path TEXT,
          PRIMARY KEY(repo_id, directory_path, order_index),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_netease_directory_entries_repo_path
        ON netease_directory_entries(repo_id, path);

        CREATE TABLE IF NOT EXISTS hardlink_groups (
          group_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          content_hash TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_groups_repo_hash_size
        ON hardlink_groups(repo_id, content_hash, size_bytes);

        CREATE TABLE IF NOT EXISTS hardlink_members (
          group_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          asset_id TEXT NOT NULL,
          path TEXT NOT NULL,
          link_state TEXT NOT NULL,
          linked_at TEXT NOT NULL,
          verified_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, asset_id),
          FOREIGN KEY(group_id) REFERENCES hardlink_groups(group_id),
          FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
        );

        CREATE INDEX IF NOT EXISTS idx_hardlink_members_repo_path
        ON hardlink_members(repo_id, path);

        CREATE TABLE IF NOT EXISTS hardlink_candidates (
          candidate_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          new_asset_id TEXT NOT NULL,
          new_path TEXT NOT NULL,
          existing_asset_id TEXT NOT NULL,
          existing_path TEXT NOT NULL,
          content_hash TEXT NOT NULL,
          size_bytes INTEGER NOT NULL,
          created_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE UNIQUE INDEX IF NOT EXISTS idx_hardlink_candidates_unique
        ON hardlink_candidates(repo_id, new_asset_id, existing_asset_id);

        CREATE TABLE IF NOT EXISTS asset_alias_groups (
          alias_group_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          source TEXT NOT NULL,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS asset_alias_members (
          alias_group_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          asset_id TEXT NOT NULL,
          path TEXT NOT NULL,
          role TEXT NOT NULL,
          created_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, asset_id),
          FOREIGN KEY(alias_group_id) REFERENCES asset_alias_groups(alias_group_id),
          FOREIGN KEY(asset_id) REFERENCES assets(asset_id)
        );

        CREATE INDEX IF NOT EXISTS idx_asset_alias_members_group
        ON asset_alias_members(repo_id, alias_group_id, path);

        CREATE TABLE IF NOT EXISTS repository_shortcuts (
          shortcut_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          label TEXT NOT NULL,
          target_kind TEXT NOT NULL,
          target_path TEXT,
          target_id TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_shortcuts_repo_order
        ON repository_shortcuts(repo_id, sort_order, label);

        CREATE TABLE IF NOT EXISTS tag_groups (
          tag_group_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          name TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS tag_group_members (
          tag_group_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          tag TEXT NOT NULL,
          normalized_tag TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          PRIMARY KEY(tag_group_id, normalized_tag),
          FOREIGN KEY(tag_group_id) REFERENCES tag_groups(tag_group_id)
        );

        CREATE INDEX IF NOT EXISTS idx_tag_group_members_repo_tag
        ON tag_group_members(repo_id, normalized_tag);

        CREATE TABLE IF NOT EXISTS folder_metadata (
          repo_id TEXT NOT NULL,
          path TEXT NOT NULL,
          protected INTEGER NOT NULL DEFAULT 0,
          password_tip TEXT,
          updated_at TEXT NOT NULL,
          PRIMARY KEY(repo_id, path),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE TABLE IF NOT EXISTS smart_folders (
          smart_folder_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          parent_id TEXT,
          name TEXT NOT NULL,
          filter_json TEXT NOT NULL,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id),
          FOREIGN KEY(parent_id) REFERENCES smart_folders(smart_folder_id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_smart_folders_repo_parent
        ON smart_folders(repo_id, parent_id, sort_order, name);

        CREATE TABLE IF NOT EXISTS repository_actions (
          action_id TEXT PRIMARY KEY,
          repo_id TEXT NOT NULL,
          source TEXT NOT NULL,
          source_action_id TEXT,
          name TEXT NOT NULL,
          status TEXT NOT NULL,
          enabled INTEGER NOT NULL DEFAULT 1,
          raw_json TEXT NOT NULL,
          unsupported_reason TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          created_at TEXT NOT NULL,
          updated_at TEXT NOT NULL,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_actions_repo_order
        ON repository_actions(repo_id, sort_order, name);

        CREATE TABLE IF NOT EXISTS repository_action_steps (
          step_id TEXT PRIMARY KEY,
          action_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          step_kind TEXT NOT NULL,
          label TEXT NOT NULL,
          status TEXT NOT NULL,
          config_json TEXT NOT NULL,
          raw_json TEXT NOT NULL,
          unsupported_reason TEXT,
          sort_order INTEGER NOT NULL DEFAULT 0,
          FOREIGN KEY(action_id) REFERENCES repository_actions(action_id) ON DELETE CASCADE,
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_action_steps_action_order
        ON repository_action_steps(action_id, sort_order);

        CREATE TABLE IF NOT EXISTS repository_action_runs (
          run_id TEXT PRIMARY KEY,
          action_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          status TEXT NOT NULL,
          target_json TEXT NOT NULL,
          message TEXT,
          started_at TEXT NOT NULL,
          finished_at TEXT,
          FOREIGN KEY(action_id) REFERENCES repository_actions(action_id),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );

        CREATE INDEX IF NOT EXISTS idx_repository_action_runs_action_time
        ON repository_action_runs(action_id, started_at DESC);

        CREATE TABLE IF NOT EXISTS repository_action_run_steps (
          run_step_id TEXT PRIMARY KEY,
          run_id TEXT NOT NULL,
          step_id TEXT NOT NULL,
          repo_id TEXT NOT NULL,
          status TEXT NOT NULL,
          message TEXT,
          started_at TEXT NOT NULL,
          finished_at TEXT,
          FOREIGN KEY(run_id) REFERENCES repository_action_runs(run_id) ON DELETE CASCADE,
          FOREIGN KEY(step_id) REFERENCES repository_action_steps(step_id),
          FOREIGN KEY(repo_id) REFERENCES repositories(repo_id)
        );
        "#,
    )?;
    connection.execute_batch(
        r#"
        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'rating', 'number', '0', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'comment', 'string', '""', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'link', 'string', '""', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'tagGroups', 'json', '[]', 1, updated_at
        FROM assets;

        INSERT OR IGNORE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
        SELECT asset_id, 'addedToLibraryAt', 'string', json_quote(created_at), 1, updated_at
        FROM assets;
        "#,
    )?;
    connection.execute(
        r#"
        UPDATE playlists
        SET player_plugin_id = ?1
        WHERE player_type_id = 'momobako.playlist.audio-sequence'
          AND player_plugin_id = ?2
        "#,
        params![AUDIO_PLAYER_PLUGIN_ID, LEGACY_AUDIO_PLAYER_PLUGIN_ID],
    )?;
    connection.execute(
        r#"
        INSERT INTO schema_version(component, version)
        VALUES ('repository', ?1)
        ON CONFLICT(component) DO UPDATE SET version = excluded.version
        "#,
        [REPO_SCHEMA_VERSION],
    )?;
    Ok(())
}

pub(in super::super) fn ensure_repository_schema_current(
    connection: &Connection,
) -> Result<(), rusqlite::Error> {
    configure_repository_connection(connection)?;
    if repository_schema_needs_migration(connection)? {
        migrate_repository_schema(connection)?;
    }
    Ok(())
}

pub(super) fn repository_schema_needs_migration(
    connection: &Connection,
) -> Result<bool, rusqlite::Error> {
    if !sqlite_table_exists(connection, "schema_version")?
        || !sqlite_table_exists(connection, "repositories")?
        || !sqlite_table_exists(connection, "assets")?
    {
        return Ok(true);
    }

    let version = connection
        .query_row(
            "SELECT version FROM schema_version WHERE component = 'repository'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(0);
    Ok(version < REPO_SCHEMA_VERSION)
}

pub(super) fn sqlite_table_exists(
    connection: &Connection,
    table_name: &str,
) -> Result<bool, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT EXISTS(
              SELECT 1 FROM sqlite_master
              WHERE type = 'table' AND name = ?1
            )
            "#,
            [table_name],
            |row| row.get::<_, i64>(0),
        )
        .map(|exists| exists != 0)
}
