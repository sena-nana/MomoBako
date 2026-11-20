//! 资源库读写的执行边界：读直接在调用线程上做，写在 Runtime 级写锁后面做，改动资源库集合的写完成后
//! 重新同步文件监听。宿主在自己的任务线程里调用，这里不再借 Tokio 的阻塞池。

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
    /// 只读操作。失败记日志后交回原因。
    pub fn read<T>(
        &self,
        operation: impl FnOnce(&RepositoryState) -> Result<T, String>,
    ) -> Result<T, String> {
        let result = operation(&self.repository_state);
        if let Err(error) = &result {
            log_runtime_operation_error("readFailed", "资源库读取操作失败。", error);
        }
        result
    }

    /// 写操作，拿着 Runtime 级写锁做，写入互相排队。
    pub fn write<T>(
        &self,
        operation: impl FnOnce(&RepositoryState) -> Result<T, String>,
    ) -> Result<T, String> {
        let result = self
            .write_lock
            .lock()
            .map_err(|_| "repository write lock poisoned".to_string())
            .and_then(|_guard| operation(&self.repository_state));
        if let Err(error) = &result {
            log_runtime_operation_error("writeFailed", "资源库写入操作失败。", error);
        }
        result
    }

    /// 改动资源库集合（新建、删除、换位置）的写操作，完成后按新的资源库列表重新同步文件监听。
    pub fn collection_write<T>(
        &self,
        operation: impl FnOnce(&RepositoryState) -> Result<T, String>,
    ) -> Result<T, String> {
        let result = self
            .write_lock
            .lock()
            .map_err(|_| "repository write lock poisoned".to_string())
            .and_then(|_guard| {
                let response = operation(&self.repository_state)?;
                sync_watched_paths(&self.repository_state, &self.watcher_handle)?;
                Ok(response)
            });
        if let Err(error) = &result {
            log_runtime_operation_error("collectionWriteFailed", "资源库集合写入操作失败。", error);
        }
        result
    }
}
