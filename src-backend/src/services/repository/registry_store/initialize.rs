//! 仓库目录初始化、数据播种、身份记录、注册表条目与存储路径准备。

use super::*;

pub(in super::super) fn ensure_backend_path_is_attachable(
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

pub(in super::super) fn initialize_repository_directory(
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

    let connection = open_repository_database_connection(&storage_paths.database_path)?;
    seed_repository_data(&connection, seed, &now)?;

    Ok(())
}

pub(in super::super) fn seed_repository_data(
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

pub(in super::super) fn ensure_repository_identity_record(
    connection: &Connection,
    repo: &RepositoryRecord,
) -> Result<(), String> {
    let now = now_rfc3339();
    connection
        .execute(
            r#"
            INSERT INTO repositories (repo_id, name, root_path, schema_version, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?5)
            ON CONFLICT(repo_id)
            DO UPDATE SET
              name = excluded.name,
              root_path = excluded.root_path,
              schema_version = excluded.schema_version,
              updated_at = excluded.updated_at
            "#,
            params![
                repo.summary.repo_id,
                repo.summary.name,
                repo.summary.path,
                REPO_SCHEMA_VERSION,
                now
            ],
        )
        .map_err(db_error)?;
    Ok(())
}

pub(in super::super) fn repository_identity_record_is_current(
    connection: &Connection,
    repo: &RepositoryRecord,
) -> Result<bool, rusqlite::Error> {
    connection
        .query_row(
            r#"
            SELECT name, root_path, schema_version
            FROM repositories
            WHERE repo_id = ?1
            "#,
            [&repo.summary.repo_id],
            |row| {
                let name: String = row.get(0)?;
                let root_path: String = row.get(1)?;
                let schema_version: i64 = row.get(2)?;
                Ok(name == repo.summary.name
                    && root_path == repo.summary.path
                    && schema_version == REPO_SCHEMA_VERSION)
            },
        )
        .optional()
        .map(|value| value.unwrap_or(false))
}

pub(in super::super) fn upsert_registry_entry(
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

pub(in super::super) fn repository_state_storage_root(service_root: &Path) -> PathBuf {
    service_root.join("repositories")
}

pub(in super::super) fn repository_state_storage_dir(
    service_root: &Path,
    repo_id: &str,
) -> PathBuf {
    repository_state_storage_root(service_root).join(repo_id)
}

pub(in super::super) fn ensure_repository_storage_paths(
    service_root: &Path,
    repo_id: &str,
    repo_root: &Path,
    backend_plugin_id: &str,
) -> Result<RepositoryStoragePaths, String> {
    let metadata_dir =
        if backend_uses_repository_root_metadata(service_root, repo_root, backend_plugin_id) {
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

pub(in super::super) fn ensure_repository_metadata_dirs(metadata_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(metadata_dir).map_err(io_error)?;
    for subdir in ["cache", "thumbnails", "logs", "indexes", REPO_TRASH_DIR] {
        fs::create_dir_all(metadata_dir.join(subdir)).map_err(io_error)?;
    }
    Ok(())
}

pub(in super::super) fn write_repository_metadata(
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
