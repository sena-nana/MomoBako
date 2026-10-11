//! Blocking-task execution helpers for repository runtime operations.

use super::{sync_watched_paths, RepositoryRuntime};
use crate::services::repository::RepositoryState;

fn log_runtime_operation_error(action: &str, message: &str, error: &str) {
    crate::app_log!(
        "error",
        "runtime.executor",
        action,
        message,
        serde_json::json!({ "error": error })
    );
}

impl RepositoryRuntime {
    /// Runs a read-only repository operation on the host Tokio runtime's blocking pool.
    pub async fn run_read<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        match tokio::task::spawn_blocking(move || operation(&repository_state)).await {
            Ok(result) => {
                if let Err(error) = &result {
                    log_runtime_operation_error("readFailed", "资源库读取操作失败。", error);
                }
                result
            }
            Err(error) => {
                let error = error.to_string();
                log_runtime_operation_error("readTaskFailed", "资源库读取任务执行失败。", &error);
                Err(error)
            }
        }
    }

    /// Runs a write operation behind the runtime-wide write lock.
    pub async fn run_write<T, F>(&self, operation: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce(&RepositoryState) -> Result<T, String> + Send + 'static,
    {
        let repository_state = self.repository_state.clone();
        let write_lock = self.write_lock.clone();
        let result = tokio::task::spawn_blocking(move || {
            let _guard = write_lock
                .lock()
                .map_err(|_| "repository write lock poisoned".to_string())?;
            operation(&repository_state)
        })
        .await;
        match result {
            Ok(result) => {
                if let Err(error) = &result {
                    log_runtime_operation_error("writeFailed", "资源库写入操作失败。", error);
                }
                result
            }
            Err(error) => {
                let error = error.to_string();
                log_runtime_operation_error("writeTaskFailed", "资源库写入任务执行失败。", &error);
                Err(error)
            }
        }
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
        let result = tokio::task::spawn_blocking(move || {
            let _guard = write_lock
                .lock()
                .map_err(|_| "repository write lock poisoned".to_string())?;
            let response = operation(&repository_state)?;
            sync_watched_paths(&repository_state, &watcher_handle)?;
            Ok::<T, String>(response)
        })
        .await;
        match result {
            Ok(result) => {
                if let Err(error) = &result {
                    log_runtime_operation_error(
                        "collectionWriteFailed",
                        "资源库集合写入操作失败。",
                        error,
                    );
                }
                result
            }
            Err(error) => {
                let error = error.to_string();
                log_runtime_operation_error(
                    "collectionWriteTaskFailed",
                    "资源库集合写入任务执行失败。",
                    &error,
                );
                Err(error)
            }
        }
    }
}
