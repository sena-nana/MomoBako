//! 仓库状态的插件列表与管理、缓存和 API 快照，以及仓库记录读取。

use super::*;

impl RepositoryState {
    pub fn list_plugins(&self) -> Result<Vec<PluginManifest>, String> {
        plugin::list_plugins(self)
    }

    /// 返回当前启用插件贡献的播放类型，供原生宿主创建播放列表时选择。
    pub fn list_playlist_players(&self) -> Result<Vec<PlaylistPlayerContribution>, String> {
        Ok(plugin_catalog(&self.root)
            .playlist_players()
            .into_iter()
            .map(|player| PlaylistPlayerContribution {
                player_type_id: player.player_type_id,
                label: player.label,
                file_class: player.file_class,
                supported_extensions: player.supported_extensions,
                supports_seek: player.supports_seek,
                supports_volume: player.supports_volume,
                supports_preview_navigation: player.supports_preview_navigation,
                description: player.description,
            })
            .collect())
    }

    pub fn list_plugin_hook_executions(
        &self,
        request: PluginHookExecutionListRequest,
    ) -> Result<PluginHookExecutionListResponse, String> {
        plugin::list_plugin_hook_executions(self, request)
    }

    pub fn set_plugin_enabled(
        &self,
        request: PluginEnabledRequest,
    ) -> Result<PluginMutationResponse, String> {
        plugin::set_plugin_enabled(self, request)
    }

    pub fn delete_plugin(&self, plugin_id: String) -> Result<PluginMutationResponse, String> {
        plugin::delete_plugin(self, plugin_id)
    }

    pub fn install_plugin_from_archive(
        &self,
        request: PluginInstallRequest,
    ) -> Result<PluginMutationResponse, String> {
        plugin::install_plugin_from_archive(self, request)
    }

    pub fn get_cache_snapshot(&self, repo_id: &str) -> Result<CacheSnapshot, String> {
        cache_usage::get_cache_snapshot(self, repo_id)
    }

    pub fn get_api_design_snapshot(&self) -> Result<ApiDesignSnapshot, String> {
        plugin::get_api_design_snapshot(self)
    }

    pub(in super::super) fn load_repository_record(
        &self,
        repo_id: &str,
    ) -> Result<RepositoryRecord, String> {
        let registry = open_registry_connection(&self.registry_path)?;
        let plugin_registry = plugin_catalog(&self.root);
        registry
            .query_row(
                r#"
                SELECT repo_id, name, path, backend_plugin_id, backend_config_json, status, updated_at
                FROM repositories
                WHERE repo_id = ?1
                "#,
                [repo_id],
                |row| {
                    let backend_plugin_id: String = row.get(3)?;
                    let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                    let path: String = row.get(2)?;
                    let stored_status: String = row.get(5)?;
                    let backend =
                        backend_summary_from_registry(&plugin_registry, &backend_plugin_id);
                    let status = repository_runtime_status(&path, &backend, stored_status.as_str());
                    let backend_config_json: String = row.get(4)?;
                    let backend_config = parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                    let authentication = repository_authentication_status(
                        &plugin_registry,
                        &backend_plugin_id,
                        &backend_config,
                    );
                    Ok(RepositoryRecord {
                        summary: RepositorySummary {
                            repo_id: row.get(0)?,
                            name: row.get(1)?,
                            path: path.clone(),
                            backend,
                            status,
                            asset_count: 0,
                            updated_at: row.get(6)?,
                            local_cache: repository_local_cache_status(&path, &backend_plugin_id),
                            authentication,
                        },
                        backend_record: RepositoryBackendRecord {
                            plugin_id: backend_plugin_id,
                            config: backend_config,
                        },
                    })
                },
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("repository not found: {repo_id}"))
    }

    pub(in super::super) fn load_repository_records(
        &self,
    ) -> Result<Vec<RepositoryRecord>, String> {
        self.ensure_initialized()?;
        let registry = open_registry_connection(&self.registry_path)?;
        let mut stmt = registry
            .prepare(
                r#"
                SELECT repo_id, name, path, backend_plugin_id, backend_config_json, status, updated_at
                FROM repositories
                ORDER BY name COLLATE NOCASE
                "#,
            )
            .map_err(db_error)?;
        let plugin_registry = plugin_catalog(&self.root);
        let rows = stmt
            .query_map([], |row| {
                let backend_plugin_id: String = row.get(3)?;
                let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                let path: String = row.get(2)?;
                let stored_status: String = row.get(5)?;
                let backend = backend_summary_from_registry(&plugin_registry, &backend_plugin_id);
                let status = repository_runtime_status(&path, &backend, stored_status.as_str());
                let backend_config_json: String = row.get(4)?;
                let backend_config =
                    parse_backend_config_json(&backend_config_json).map_err(to_from_sql_error)?;
                let authentication = repository_authentication_status(
                    &plugin_registry,
                    &backend_plugin_id,
                    &backend_config,
                );
                Ok(RepositoryRecord {
                    summary: RepositorySummary {
                        repo_id: row.get(0)?,
                        name: row.get(1)?,
                        path: path.clone(),
                        backend,
                        status,
                        asset_count: 0,
                        updated_at: row.get(6)?,
                        local_cache: repository_local_cache_status(&path, &backend_plugin_id),
                        authentication,
                    },
                    backend_record: RepositoryBackendRecord {
                        plugin_id: backend_plugin_id,
                        config: backend_config,
                    },
                })
            })
            .map_err(db_error)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub(in super::super) fn find_existing_repository_for_backend(
        &self,
        backend: &RepositoryBackendRecord,
    ) -> Result<Option<RepositorySummary>, String> {
        if backend.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
            return Ok(None);
        }
        let account_id = backend
            .config
            .get("accountId")
            .and_then(normalized_netease_account_id);
        let Some(account_id) = account_id else {
            return Ok(None);
        };
        Ok(self
            .load_repository_records()?
            .into_iter()
            .find(|record| {
                record.backend_record.plugin_id == NETEASE_CLOUD_MUSIC_PLUGIN_ID
                    && record
                        .backend_record
                        .config
                        .get("accountId")
                        .and_then(normalized_netease_account_id)
                        .as_deref()
                        == Some(account_id.as_str())
            })
            .map(|record| record.summary))
    }

    pub(in super::super) fn repository_backend_in_use(
        &self,
        plugin_id: &str,
    ) -> Result<bool, String> {
        let registry = open_registry_connection(&self.registry_path)?;
        let plugin_registry = plugin_catalog(&self.root);
        let mut stmt = registry
            .prepare("SELECT backend_plugin_id FROM repositories")
            .map_err(db_error)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(db_error)?;

        for row in rows {
            let stored_plugin_id = row.map_err(db_error)?;
            if plugin_registry.normalize_plugin_id(&stored_plugin_id) == plugin_id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(in super::super) fn repository_thumbnail_root(
        &self,
        repo: &RepositoryRecord,
    ) -> Result<PathBuf, String> {
        let repo_root = Path::new(&repo.summary.path);
        let storage_paths = ensure_repository_storage_paths(
            &self.root,
            &repo.summary.repo_id,
            repo_root,
            &repo.backend_record.plugin_id,
        )?;
        Ok(storage_paths.metadata_dir.join("thumbnails"))
    }
}
