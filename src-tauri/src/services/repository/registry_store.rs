//! Registry records, repository metadata, backend config, and storage roots.

use super::*;

pub(super) fn backend_summary_from_registry(
    registry: &BackendPluginRegistry,
    plugin_id: &str,
) -> RepositoryBackendSummary {
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    if let Some(manifest) = registry.manifest(&normalized_plugin_id) {
        RepositoryBackendSummary {
            plugin_id: manifest.plugin_id.clone(),
            kind: manifest.kind.clone(),
            name: manifest.name.clone(),
            capabilities: manifest.capabilities.clone(),
        }
    } else {
        RepositoryBackendSummary {
            plugin_id: normalized_plugin_id,
            kind: "unavailable".to_string(),
            name: "Unavailable plugin".to_string(),
            capabilities: Vec::new(),
        }
    }
}

pub(super) fn repository_runtime_status(
    path: &str,
    backend_plugin_id: &str,
    stored_status: &str,
) -> String {
    if backend_plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
        || netease_cache_root_path(path, backend_plugin_id).is_some()
    {
        if Path::new(path).is_dir() {
            "ready".to_string()
        } else {
            "missing".to_string()
        }
    } else if backend_plugin_id == NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        "missing".to_string()
    } else {
        stored_status.to_string()
    }
}

pub(super) fn netease_cache_root_path(path: &str, backend_plugin_id: &str) -> Option<PathBuf> {
    if backend_plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return None;
    }
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with("netease-cloud-music://") {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

pub(super) fn repository_local_cache_status(
    path: &str,
    backend_plugin_id: &str,
) -> Option<RepositoryLocalCacheStatus> {
    if backend_plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return None;
    }
    let cache_root = netease_cache_root_path(path, backend_plugin_id);
    let status = match cache_root.as_ref() {
        Some(root) if root.is_dir() => "ready",
        Some(_) => "missing",
        None => "unconfigured",
    };
    Some(RepositoryLocalCacheStatus {
        required: true,
        path: cache_root.map(|path| path.to_string_lossy().to_string()),
        status: status.to_string(),
    })
}

pub(super) fn ensure_netease_cache_ready(repo: &RepositoryRecord) -> Result<PathBuf, String> {
    if repo.backend_record.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return Err("repository is not a netease cloud music repository".to_string());
    }
    let cache_root = netease_cache_root_path(&repo.summary.path, &repo.backend_record.plugin_id)
        .ok_or_else(|| "网易云资源库缺少本地缓存目录，请先指定缓存目录".to_string())?;
    if !cache_root.is_dir() {
        return Err("网易云资源库缓存目录不可用，请重新指定缓存目录".to_string());
    }
    Ok(cache_root)
}

pub(super) fn parse_backend_request(
    service_root: &Path,
    request: &RepositoryMutationRequest,
) -> Result<RepositoryBackendRecord, String> {
    let plugin_id = request
        .backend_plugin_id
        .as_deref()
        .unwrap_or(LOCAL_FILESYSTEM_PLUGIN_ID)
        .trim();
    let registry = backend_plugin_registry(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    let registration = registry
        .registration(&normalized_plugin_id)
        .ok_or_else(|| format!("unsupported filesystem backend plugin: {plugin_id}"))?;
    let manifest = &registration.manifest;
    if !is_source_plugin(manifest) {
        return Err(format!(
            "plugin is not a repository source: {}",
            manifest.plugin_id
        ));
    }
    ensure_repository_backend_runtime_available(registration)?;
    let config = request
        .backend_config
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    if !config.is_object() {
        return Err("backend config must be a JSON object".to_string());
    }
    Ok(RepositoryBackendRecord {
        plugin_id: manifest.plugin_id.clone(),
        config,
    })
}

pub(super) fn import_backend_record(
    service_root: &Path,
    metadata: &RepositoryMetadataFileImport,
) -> Option<RepositoryBackendRecord> {
    let plugin_id = metadata
        .backend_plugin_id
        .as_deref()
        .unwrap_or(LOCAL_FILESYSTEM_PLUGIN_ID);
    let registry = backend_plugin_registry(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    registry
        .manifest(&normalized_plugin_id)
        .map(|manifest| RepositoryBackendRecord {
            plugin_id: manifest.plugin_id.clone(),
            config: metadata
                .backend_config
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
        })
}

pub(super) fn rewrite_repository_metadata_if_needed(
    service_root: &Path,
    metadata_path: &Path,
    metadata: &RepositoryMetadataFileImport,
    repo_root: &Path,
    next_root_path: Option<&Path>,
) -> Result<(), String> {
    let normalized_plugin_id = metadata
        .backend_plugin_id
        .as_deref()
        .map(|plugin_id| backend_plugin_registry(service_root).normalize_plugin_id(plugin_id))
        .unwrap_or_else(|| LOCAL_FILESYSTEM_PLUGIN_ID.to_string());
    let root_path = next_root_path
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| metadata.root_path.clone())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string());

    if metadata.root_path.as_deref() == Some(root_path.as_str())
        && metadata.backend_plugin_id.as_deref() == Some(normalized_plugin_id.as_str())
    {
        return Ok(());
    }

    let rewritten = RepositoryMetadataFile {
        repo_id: metadata.repo_id.clone(),
        name: metadata
            .name
            .clone()
            .unwrap_or_else(|| infer_repository_name(repo_root)),
        root_path,
        backend_plugin_id: normalized_plugin_id,
        backend_config: metadata
            .backend_config
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        created_at: metadata.created_at.clone().unwrap_or_else(now_rfc3339),
        schema_version: metadata.schema_version.unwrap_or(REPO_SCHEMA_VERSION),
    };
    let metadata_json = serde_json::to_string_pretty(&rewritten).map_err(json_error)?;
    fs::write(metadata_path, metadata_json).map_err(io_error)
}

pub(super) fn parse_backend_config_json(
    value: &str,
) -> Result<serde_json::Value, serde_json::Error> {
    let parsed = serde_json::from_str::<serde_json::Value>(value)?;
    if parsed.is_object() {
        Ok(parsed)
    } else {
        Ok(serde_json::json!({}))
    }
}

pub(super) fn preserve_netease_cache_config(
    existing: &RepositoryBackendRecord,
    next_backend_config: &mut serde_json::Value,
) {
    if existing.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return;
    }
    let Some(next_object) = next_backend_config.as_object_mut() else {
        return;
    };
    for key in ["sourceUri", "localCachePath"] {
        if next_object.contains_key(key) {
            continue;
        }
        if let Some(value) = existing.config.get(key) {
            next_object.insert(key.to_string(), value.clone());
        }
    }
}

pub(super) fn netease_source_uri_for_repo(repo: &RepositoryRecord) -> String {
    if repo.summary.path.starts_with("netease-cloud-music://") {
        return repo.summary.path.clone();
    }
    repo.backend_record
        .config
        .get("accountId")
        .and_then(normalized_netease_account_id)
        .map(|account_id| format!("netease-cloud-music://account/{account_id}"))
        .unwrap_or_else(|| repo.summary.path.clone())
}

pub(super) fn to_from_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
}

pub(super) fn migrate_registry_schema(registry: &Connection) -> Result<(), rusqlite::Error> {
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
    Ok(())
}

pub(super) fn migrate_repository_schema(connection: &Connection) -> Result<(), rusqlite::Error> {
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
    Ok(())
}

pub(super) fn ensure_backend_path_is_attachable(
    service_root: &Path,
    backend: &RepositoryBackendRecord,
    repo_root: &Path,
) -> Result<(), String> {
    let adapter = RuntimeFileSystemBackendAdapter {
        service_root: service_root.to_path_buf(),
        plugin_id: backend.plugin_id.clone(),
    };
    adapter.ensure_attachable(repo_root, &backend.config)
}

pub(super) fn initialize_repository_directory(
    service_root: &Path,
    repo_root: &Path,
    seed: &RepositorySeed<'_>,
    backend: &RepositoryBackendRecord,
) -> Result<(), String> {
    let adapter = RuntimeFileSystemBackendAdapter {
        service_root: service_root.to_path_buf(),
        plugin_id: backend.plugin_id.clone(),
    };
    adapter.prepare_repository_root(repo_root, &backend.config)?;
    let storage_paths =
        ensure_repository_storage_paths(service_root, seed.repo_id, repo_root, &backend.plugin_id)?;
    let meta_dir = storage_paths.metadata_dir;
    hide_repository_meta_dir(&meta_dir);

    let now = now_rfc3339();
    let metadata = RepositoryMetadataFile {
        repo_id: seed.repo_id.to_string(),
        name: seed.name.to_string(),
        root_path: repo_root.to_string_lossy().to_string(),
        backend_plugin_id: backend.plugin_id.clone(),
        backend_config: backend.config.clone(),
        created_at: now.clone(),
        schema_version: REPO_SCHEMA_VERSION,
    };
    let metadata_json = serde_json::to_string_pretty(&metadata).map_err(json_error)?;
    fs::write(meta_dir.join(REPO_METADATA_FILE_NAME), metadata_json).map_err(io_error)?;

    let connection = Connection::open(storage_paths.database_path).map_err(db_error)?;
    migrate_repository_schema(&connection).map_err(db_error)?;
    seed_repository_data(&connection, seed, &now)?;

    Ok(())
}

pub(super) fn seed_repository_data(
    connection: &Connection,
    seed: &RepositorySeed<'_>,
    now: &str,
) -> Result<(), String> {
    connection
        .execute(
            r#"
            INSERT OR REPLACE INTO repositories (repo_id, name, root_path, schema_version, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                seed.repo_id,
                seed.name,
                seed.root_path,
                REPO_SCHEMA_VERSION,
                now,
                now
            ],
        )
        .map_err(db_error)?;

    for asset in seed.assets {
        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO assets (
                  asset_id, repo_id, path, filename, extension, size_bytes,
                  created_at, modified_at, hash, status, version, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1, ?11)
                "#,
                params![
                    asset.asset_id,
                    seed.repo_id,
                    asset.path,
                    asset.filename,
                    asset.extension,
                    asset.size_bytes,
                    now,
                    asset.modified_at,
                    format!("sha256:{}", safe_prefix(asset.asset_id, 12)),
                    asset.status,
                    asset.modified_at
                ],
            )
            .map_err(db_error)?;

        for tag in asset.tags {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO tags (asset_id, tag, normalized_tag)
                    VALUES (?1, ?2, ?3)
                    "#,
                    params![asset.asset_id, tag, tag.to_lowercase()],
                )
                .map_err(db_error)?;
        }

        let before = serde_json::json!({});
        let mut after_map = BTreeMap::new();
        for (key, value_type, value_json) in asset.metadata {
            connection
                .execute(
                    r#"
                    INSERT OR REPLACE INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
                    VALUES (?1, ?2, ?3, ?4, 1, ?5)
                    "#,
                    params![asset.asset_id, key, value_type, value_json, asset.modified_at],
                )
                .map_err(db_error)?;
            let parsed_value: serde_json::Value =
                serde_json::from_str(value_json).map_err(json_error)?;
            after_map.insert((*key).to_string(), parsed_value);
        }

        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO revisions (
                  revision_id, repo_id, asset_id, timestamp, operation, before_json, after_json, source
                )
                VALUES (?1, ?2, ?3, ?4, 'metadata.seeded', ?5, ?6, 'seed')
                "#,
                params![
                    format!("rev-{}", asset.asset_id),
                    seed.repo_id,
                    asset.asset_id,
                    asset.modified_at,
                    before.to_string(),
                    serde_json::to_string(&after_map).map_err(json_error)?
                ],
            )
            .map_err(db_error)?;

        connection
            .execute(
                r#"
                INSERT OR REPLACE INTO events (
                  event_id, repo_id, asset_id, event_type, path, payload_json, created_at
                )
                VALUES (?1, ?2, ?3, 'asset.discovered', ?4, ?5, ?6)
                "#,
                params![
                    format!("evt-{}", asset.asset_id),
                    seed.repo_id,
                    asset.asset_id,
                    asset.path,
                    serde_json::json!({ "status": asset.status }).to_string(),
                    asset.modified_at
                ],
            )
            .map_err(db_error)?;
    }

    Ok(())
}

pub(super) fn upsert_registry_entry(
    registry: &Connection,
    repo_root: &Path,
    seed: &RepositorySeed<'_>,
    backend: &RepositoryBackendRecord,
) -> Result<(), String> {
    let now = now_rfc3339();
    registry
        .execute(
            r#"
            INSERT OR REPLACE INTO repositories (
              repo_id, name, path, backend_plugin_id, backend_config_json, status, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                seed.repo_id,
                seed.name,
                repo_root.to_string_lossy().to_string(),
                backend.plugin_id,
                backend.config.to_string(),
                seed.status,
                now,
                now
            ],
        )
        .map_err(db_error)?;
    Ok(())
}

pub(super) fn repository_state_storage_root(service_root: &Path) -> PathBuf {
    service_root.join("repositories")
}

pub(super) fn repository_state_storage_dir(service_root: &Path, repo_id: &str) -> PathBuf {
    repository_state_storage_root(service_root).join(repo_id)
}

pub(super) fn ensure_repository_storage_paths(
    service_root: &Path,
    repo_id: &str,
    repo_root: &Path,
    backend_plugin_id: &str,
) -> Result<RepositoryStoragePaths, String> {
    let metadata_dir = if backend_plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
        || netease_cache_root_path(&repo_root.to_string_lossy(), backend_plugin_id).is_some()
    {
        migrate_legacy_meta_dir_if_needed(repo_root, backend_plugin_id)?;
        let metadata_dir = repository_meta_dir(repo_root);
        if repo_root.exists() {
            ensure_repository_metadata_dirs(&metadata_dir)?;
            hide_repository_meta_dir(&metadata_dir);
        }
        metadata_dir
    } else {
        let service_repo_dir = repository_state_storage_dir(service_root, repo_id);
        fs::create_dir_all(&service_repo_dir).map_err(io_error)?;
        let metadata_dir = service_repo_dir.join(REPO_META_DIR);
        ensure_repository_metadata_dirs(&metadata_dir)?;
        metadata_dir
    };
    Ok(RepositoryStoragePaths {
        database_path: metadata_dir.join(REPO_DB_FILE_NAME),
        metadata_dir,
    })
}

pub(super) fn ensure_repository_metadata_dirs(metadata_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(metadata_dir).map_err(io_error)?;
    for subdir in ["cache", "thumbnails", "logs", "indexes", REPO_TRASH_DIR] {
        fs::create_dir_all(metadata_dir.join(subdir)).map_err(io_error)?;
    }
    Ok(())
}

pub(super) fn write_repository_metadata(
    metadata_dir: &Path,
    repo_id: &str,
    name: &str,
    repo_root: &Path,
    backend_plugin_id: &str,
    backend_config: &serde_json::Value,
    created_at: Option<String>,
) -> Result<(), String> {
    fs::create_dir_all(metadata_dir).map_err(io_error)?;
    let metadata = RepositoryMetadataFile {
        repo_id: repo_id.to_string(),
        name: name.to_string(),
        root_path: repo_root.to_string_lossy().to_string(),
        backend_plugin_id: backend_plugin_id.to_string(),
        backend_config: backend_config.clone(),
        created_at: created_at.unwrap_or_else(now_rfc3339),
        schema_version: REPO_SCHEMA_VERSION,
    };
    let metadata_json = serde_json::to_string_pretty(&metadata).map_err(json_error)?;
    fs::write(metadata_dir.join(REPO_METADATA_FILE_NAME), metadata_json).map_err(io_error)
}

pub(super) fn normalize_external_cache_root(path: &str) -> Result<PathBuf, String> {
    let cache_root = PathBuf::from(path);
    if cache_root.exists() {
        return canonicalize_local_path(&cache_root);
    }
    if let Some(parent) = cache_root.parent() {
        if parent.exists() {
            let parent = canonicalize_local_path(parent)?;
            if let Some(name) = cache_root.file_name() {
                return Ok(parent.join(name));
            }
        }
    }
    if cache_root.is_relative() {
        return Ok(std::env::current_dir().map_err(io_error)?.join(cache_root));
    }
    Ok(cache_root)
}

pub(super) fn merge_netease_cache_state_contents(
    source: &Path,
    target: &Path,
) -> Result<usize, String> {
    fs::create_dir_all(target).map_err(io_error)?;
    if !source.exists() {
        return Ok(0);
    }
    let mut moved = 0;
    for entry in fs::read_dir(source).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name();
        let source_path = entry.path();
        let target_path = target.join(&file_name);
        if target_path.exists() {
            if source_path.is_dir() && target_path.is_dir() {
                moved += merge_netease_cache_state_contents(&source_path, &target_path)?;
            }
            continue;
        }
        fs::rename(&source_path, &target_path).map_err(io_error)?;
        moved += 1;
    }
    Ok(moved)
}

pub(super) fn netease_playback_cache_dir(cache_root: &Path) -> PathBuf {
    repository_meta_dir(cache_root)
        .join("cache")
        .join("netease-playback")
}

pub(super) fn downloader_legacy_temp_dir(service_root: &Path) -> PathBuf {
    plugin_data_dir(service_root, "momobako.service.downloader").join("temp")
}

pub(super) fn netease_downloader_cache_key(song_id: i64, level: &str, account_id: &str) -> String {
    let mut hasher = Sha1::new();
    hasher.update(format!("{song_id}:{level}:{account_id}"));
    format!("{:x}", Sha1Digest::finalize(hasher))
}

pub(super) fn value_as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

pub(super) fn migrate_netease_playback_cache(
    service_root: &Path,
    repo: &RepositoryRecord,
    cache_root: &Path,
    migration: &mut NeteaseRepositoryCacheMigrationSummary,
) -> Result<(), String> {
    let legacy_temp_dir = downloader_legacy_temp_dir(service_root);
    if !legacy_temp_dir.exists() {
        return Ok(());
    }
    let account_id = repo
        .backend_record
        .config
        .get("accountId")
        .and_then(value_as_string)
        .unwrap_or_else(|| "anonymous".to_string());
    let default_level = repo
        .backend_record
        .config
        .get("defaultLevel")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("standard");
    let connection =
        match self_open_repository_connection_for_cache_migration(service_root, repo, cache_root) {
            Ok(connection) => connection,
            Err(_) => return Ok(()),
        };
    let asset_map = load_asset_path_map(&connection, &repo.summary.repo_id).map_err(db_error)?;
    let target_dir = netease_playback_cache_dir(cache_root);
    fs::create_dir_all(&target_dir).map_err(io_error)?;

    for asset in asset_map.values() {
        if asset.provider_id.as_deref() != Some("netease-cloud-music") {
            continue;
        }
        let Some(payload) = asset.source_payload.as_ref() else {
            migration.skipped_playback_cache_files += 1;
            continue;
        };
        let song_id = payload
            .get("songId")
            .and_then(serde_json::Value::as_i64)
            .or_else(|| {
                payload
                    .get("songId")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|value| value.parse::<i64>().ok())
            });
        let Some(song_id) = song_id else {
            migration.skipped_playback_cache_files += 1;
            continue;
        };
        let level = payload
            .get("level")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(default_level);
        let cache_key = netease_downloader_cache_key(song_id, level, &account_id);
        for extension in ["mp3", "lrc", "yrc"] {
            let source_path = legacy_temp_dir.join(format!("{cache_key}.{extension}"));
            if !source_path.exists() {
                continue;
            }
            let target_path = target_dir.join(format!("{cache_key}.{extension}"));
            if target_path.exists() {
                migration.skipped_playback_cache_files += 1;
                continue;
            }
            match fs::rename(&source_path, &target_path) {
                Ok(()) => migration.migrated_playback_cache_files += 1,
                Err(_) => migration.failed_playback_cache_files += 1,
            }
        }
    }
    Ok(())
}

pub(super) fn self_open_repository_connection_for_cache_migration(
    service_root: &Path,
    repo: &RepositoryRecord,
    cache_root: &Path,
) -> Result<Connection, String> {
    let storage_paths = ensure_repository_storage_paths(
        service_root,
        &repo.summary.repo_id,
        cache_root,
        &repo.backend_record.plugin_id,
    )?;
    let connection = Connection::open(storage_paths.database_path).map_err(db_error)?;
    migrate_repository_schema(&connection).map_err(db_error)?;
    Ok(connection)
}

pub(super) fn infer_repository_name(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .map(|name| name.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string())
}

pub(super) fn repository_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(REPO_META_DIR)
}

pub(super) fn repository_trash_dir(repo_root: &Path) -> PathBuf {
    repository_meta_dir(repo_root).join(REPO_TRASH_DIR)
}

pub(super) fn repository_trash_manifest_path(repo_root: &Path) -> PathBuf {
    repository_meta_dir(repo_root).join(REPO_TRASH_MANIFEST_FILE_NAME)
}

pub(super) fn legacy_repository_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(LEGACY_REPO_META_DIR)
}

pub(super) fn is_internal_repository_dir(name: &str) -> bool {
    name == REPO_META_DIR || name == LEGACY_REPO_META_DIR
}

pub(super) fn migrate_legacy_meta_dir_if_needed(
    repo_root: &Path,
    _backend_plugin_id: &str,
) -> Result<(), String> {
    let current_dir = repository_meta_dir(repo_root);
    if current_dir.exists() {
        hide_repository_meta_dir(&current_dir);
        return Ok(());
    }

    let legacy_dir = legacy_repository_meta_dir(repo_root);
    if legacy_dir.exists() {
        fs::rename(&legacy_dir, &current_dir).map_err(io_error)?;
        hide_repository_meta_dir(&current_dir);
    }

    Ok(())
}

pub(super) fn hide_repository_meta_dir(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("attrib").arg("+H").arg(path).status();
    }
}
