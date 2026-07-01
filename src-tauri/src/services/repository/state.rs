//! Repository state shell and shared entry points.

use super::*;
use std::collections::BTreeSet;
use std::sync::mpsc::Sender;
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub(crate) struct RepositoryStructureRefreshRequest {
    pub repo_id: String,
    pub reason: String,
}

/// Shared repository runtime state used by ViewModels, runtime services, and repository feature modules.
pub struct RepositoryState {
    pub(super) root: PathBuf,
    pub(super) registry_path: PathBuf,
    pub(super) initialized: Mutex<bool>,
    pub(super) preview_sources: Mutex<BTreeMap<String, PreviewFileSource>>,
    pub(super) structure_refresh_tx: Mutex<Option<Sender<RepositoryStructureRefreshRequest>>>,
    pub(super) refreshing_repo_ids: Mutex<BTreeSet<String>>,
    pub(super) app_handle: Mutex<Option<AppHandle>>,
}

impl RepositoryState {
    /// Creates repository state rooted at the service-data directory.
    pub fn from_root(root: PathBuf) -> Self {
        let registry_path = root.join(REGISTRY_FILE_NAME);
        Self {
            root,
            registry_path,
            initialized: Mutex::new(false),
            preview_sources: Mutex::new(BTreeMap::new()),
            structure_refresh_tx: Mutex::new(None),
            refreshing_repo_ids: Mutex::new(BTreeSet::new()),
            app_handle: Mutex::new(None),
        }
    }

    /// Returns the service root used by repository runtime state.
    pub fn root_path(&self) -> PathBuf {
        self.root.clone()
    }

    pub fn set_structure_refresh_sender(
        &self,
        sender: Sender<RepositoryStructureRefreshRequest>,
    ) -> Result<(), String> {
        let mut slot = self
            .structure_refresh_tx
            .lock()
            .map_err(|_| "structure refresh sender lock poisoned".to_string())?;
        *slot = Some(sender);
        Ok(())
    }

    pub fn set_app_handle(&self, app: AppHandle) -> Result<(), String> {
        let mut slot = self
            .app_handle
            .lock()
            .map_err(|_| "app handle lock poisoned".to_string())?;
        *slot = Some(app);
        Ok(())
    }

    pub fn queue_repository_structure_refresh(&self, repo_id: String, reason: &str) {
        let Ok(sender) = self.structure_refresh_tx.lock() else {
            return;
        };
        if let Some(sender) = sender.as_ref() {
            let _ = sender.send(RepositoryStructureRefreshRequest {
                repo_id,
                reason: reason.to_string(),
            });
        }
    }

    pub fn set_repository_structure_refreshing(
        &self,
        repo_id: &str,
        refreshing: bool,
    ) -> Result<(), String> {
        let mut refreshing_repo_ids = self
            .refreshing_repo_ids
            .lock()
            .map_err(|_| "structure refreshing set lock poisoned".to_string())?;
        if refreshing {
            refreshing_repo_ids.insert(repo_id.to_string());
        } else {
            refreshing_repo_ids.remove(repo_id);
        }
        Ok(())
    }

    pub fn repository_structure_refresh_in_progress(&self, repo_id: &str) -> bool {
        self.refreshing_repo_ids
            .lock()
            .map(|set| set.contains(repo_id))
            .unwrap_or(false)
    }

    pub fn emit_repository_structure_updated(&self, event: RepositoryStructureUpdatedEvent) {
        let Ok(app_handle) = self.app_handle.lock() else {
            return;
        };
        if let Some(app_handle) = app_handle.as_ref() {
            let _ = app_handle.emit("repository://structure-updated", event);
        }
    }

    pub fn repository_structure_indexed_at(&self, repo_id: &str) -> Result<Option<String>, String> {
        let repo = self.load_repository_record(repo_id)?;
        let connection = self.open_repository_connection(
            &repo.summary.repo_id,
            &repo.summary.path,
            &repo.backend_record,
        )?;
        latest_directory_indexed_at(&connection, repo_id).map_err(db_error)
    }

    /// Initializes registry storage lazily before repository operations run.
    pub fn ensure_initialized(&self) -> Result<(), String> {
        let mut initialized = self
            .initialized
            .lock()
            .map_err(|_| "repository state lock poisoned".to_string())?;
        if *initialized {
            return Ok(());
        }

        fs::create_dir_all(&self.root).map_err(io_error)?;
        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        registry
            .execute_batch(REGISTRY_SCHEMA_SQL)
            .map_err(db_error)?;
        migrate_registry_schema(&registry).map_err(db_error)?;
        *initialized = true;
        Ok(())
    }

    /// Lists registered repositories together with runtime-derived status information.
    pub fn list_repositories(&self) -> Result<Vec<RepositorySummary>, String> {
        self.ensure_initialized()?;

        let registry = Connection::open(&self.registry_path).map_err(db_error)?;
        let mut stmt = registry
            .prepare(
                r#"
                SELECT repo_id, name, path, backend_plugin_id, backend_config_json, status, updated_at
                FROM repositories
                ORDER BY name COLLATE NOCASE
                "#,
            )
            .map_err(db_error)?;

        let plugin_registry = backend_plugin_registry(&self.root);
        let rows = stmt
            .query_map([], |row| {
                let repo_id: String = row.get(0)?;
                let path: String = row.get(2)?;
                let backend_plugin_id: String = row.get(3)?;
                let backend_plugin_id = plugin_registry.normalize_plugin_id(&backend_plugin_id);
                let backend = backend_summary_from_registry(&plugin_registry, &backend_plugin_id);
                let status =
                    repository_runtime_status(&path, &backend, row.get::<_, String>(5)?.as_str());
                let asset_count = if status == "missing" {
                    0
                } else {
                    load_asset_count(&self.root, &repo_id, &path, &backend_plugin_id).unwrap_or(0)
                };

                Ok(RepositorySummary {
                    repo_id,
                    name: row.get(1)?,
                    path: path.clone(),
                    backend,
                    status,
                    asset_count,
                    updated_at: row.get(6)?,
                    local_cache: repository_local_cache_status(&path, &backend_plugin_id),
                })
            })
            .map_err(db_error)?;

        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    /// Returns repository thumbnail roots that should be exposed to the desktop asset scope.
    pub fn list_repository_thumbnail_roots(&self) -> Result<Vec<PathBuf>, String> {
        self.load_repository_records()?
            .into_iter()
            .map(|repo| self.repository_thumbnail_root(&repo))
            .collect()
    }

    /// Opens a previously registered preview file source for the preview HTTP server.
    pub fn open_preview_file_source(&self, token: &str) -> Result<(File, String), String> {
        let source = self
            .preview_sources
            .lock()
            .map_err(|_| "preview source lock poisoned".to_string())?
            .get(token)
            .cloned()
            .ok_or_else(|| "preview source not found".to_string())?;
        if !source.path.is_file() {
            return Err("preview source file is no longer available".to_string());
        }
        let file = File::open(&source.path).map_err(io_error)?;
        Ok((file, source.media_type))
    }

    /// Registers an on-disk file as a temporary preview source and returns the generated token.
    pub fn register_preview_source_path(
        &self,
        source_path: PathBuf,
        media_type: &str,
    ) -> Result<String, String> {
        if !source_path.is_file() {
            return Err(format!(
                "preview source file is not available: {}",
                source_path.to_string_lossy()
            ));
        }
        let metadata = fs::metadata(&source_path).map_err(io_error)?;
        let modified_at = metadata
            .modified()
            .ok()
            .map(system_time_to_rfc3339)
            .transpose()
            .map_err(time_error)?;
        let token = preview_file_token(
            "playback",
            &source_path.to_string_lossy(),
            media_type,
            metadata.len(),
            modified_at.as_deref().unwrap_or_default(),
        );
        self.preview_sources
            .lock()
            .map_err(|_| "preview source lock poisoned".to_string())?
            .insert(
                token.clone(),
                PreviewFileSource {
                    path: source_path,
                    media_type: media_type.to_string(),
                },
            );
        Ok(token)
    }
}
