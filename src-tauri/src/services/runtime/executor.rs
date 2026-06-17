//! Blocking-task execution helpers for repository runtime operations.

use super::{sync_watched_paths, RepositoryRuntime};
use crate::services::repository::RepositoryState;
use std::path::PathBuf;

impl RepositoryRuntime {
    /// Runs a read-only repository operation on Tauri's blocking pool.
    pub async fn run_read<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        tauri::async_runtime::spawn_blocking(move || operation(&repository_state))
            .await
            .map_err(|error| error.to_string())?
    }

    /// Runs a write operation behind the runtime-wide write lock.
    pub async fn run_write<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        let write_lock = self.write_lock.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = write_lock
                .lock()
                .map_err(|_| "repository write lock poisoned".to_string())?;
            operation(&repository_state)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    /// Runs a repository-collection mutation and refreshes runtime watchers afterwards.
    pub async fn run_repository_collection_write<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        let watcher_handle = self.watcher_handle.clone();
        let write_lock = self.write_lock.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _guard = write_lock
                .lock()
                .map_err(|_| "repository write lock poisoned".to_string())?;
            let response = operation(&repository_state)?;
            sync_watched_paths(&repository_state, &watcher_handle)?;
            Ok(response)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    /// Returns all repository roots that are allowed to serve thumbnails to the UI shell.
    pub async fn repository_thumbnail_roots(&self) -> Result<Vec<PathBuf>, String> {
        let repository_state = self.repository_state.clone();
        tauri::async_runtime::spawn_blocking(move || {
            repository_state.list_repository_thumbnail_roots()
        })
        .await
        .map_err(|error| error.to_string())?
    }
}
