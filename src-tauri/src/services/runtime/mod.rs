//! Repository runtime facade and runtime-layer composition.

mod executor;
pub(crate) mod external_api;
pub(crate) mod preview_server;
pub(crate) mod watcher;

use crate::services::repository::RepositoryState;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub use external_api::ExternalApiConnectionStatus;
pub(crate) use external_api::{
    build_external_connection_status, generate_external_api_token, start_external_api_server,
    write_external_connection_file,
};
pub(crate) use preview_server::start_preview_server;
pub(crate) use watcher::{start_structure_refresh_worker, sync_watched_paths, RepositoryWatcher};

const PREVIEW_HOST: &str = "127.0.0.1";

/// Runtime shell that coordinates repository-state access, preview hosting, and write serialization.
#[derive(Clone)]
pub struct RepositoryRuntime {
    pub(crate) repository_state: Arc<RepositoryState>,
    pub(crate) watcher_handle: Arc<Mutex<RepositoryWatcher>>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) preview_addr: String,
    pub(crate) external_connection: ExternalApiConnectionStatus,
}

impl RepositoryRuntime {
    /// Starts the repository runtime and all long-lived background services.
    pub fn start() -> Result<Self, String> {
        let root = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(".service-data");
        let repository_state = Arc::new(RepositoryState::from_root(root.clone()));
        let write_lock = Arc::new(Mutex::new(()));
        repository_state.ensure_initialized()?;
        repository_state.shutdown_runtime_helpers()?;
        let structure_refresh_tx =
            start_structure_refresh_worker(repository_state.clone(), write_lock.clone())?;
        repository_state.set_structure_refresh_sender(structure_refresh_tx)?;

        let watcher_handle =
            RepositoryWatcher::start(repository_state.clone(), write_lock.clone())?;
        let preview_addr = start_preview_server(repository_state.clone())?;
        let external_token = generate_external_api_token()?;
        let external_addr = start_external_api_server(
            repository_state.clone(),
            write_lock.clone(),
            external_token.clone(),
        )?;
        let started_at = external_api::now_unix_millis().to_string();
        let external_connection =
            build_external_connection_status(&root, &external_addr, &external_token, &started_at);
        write_external_connection_file(&external_connection)?;

        Ok(Self {
            repository_state,
            watcher_handle,
            write_lock,
            preview_addr,
            external_connection,
        })
    }

    /// Returns the active repository-runtime root on disk.
    pub fn service_root(&self) -> PathBuf {
        self.repository_state.root_path()
    }

    /// Builds a browser-facing preview URL for a previously prepared source token.
    pub fn preview_source_url(&self, token: &str) -> String {
        format!("http://{}/preview/{token}", self.preview_addr)
    }

    /// Returns the latest external API connection payload, including readiness derived from state init.
    pub fn external_api_connection_status(&self) -> ExternalApiConnectionStatus {
        ExternalApiConnectionStatus {
            ready: self.repository_state.ensure_initialized().is_ok(),
            ..self.external_connection.clone()
        }
    }

    pub fn set_app_handle(&self, app: tauri::AppHandle) -> Result<(), String> {
        self.repository_state.set_app_handle(app)
    }

    /// Performs best-effort cleanup for long-lived helper processes before app shutdown.
    pub fn shutdown_helpers(&self) {
        let _ = self
            .repository_state
            .shutdown_runtime_helpers()
            .map_err(|error| eprintln!("runtime helper shutdown failed: {error}"));
    }
}
