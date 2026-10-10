//! 打开资源库存储：定位 `.momo`、打开数据库，缺失或损坏时从后端重建。
//!
//! 重建要靠后端列文件。后端调不了、或者重建同步失败时，磁盘上不能留下「已经重建过」的标记
//! （元数据文件和这次新建的数据库），否则下次打开会把空库当成已重建，资源库就一直是空的。

use super::*;

/// 打开资源库存储的结果。
pub(super) enum RepositoryStorage {
    /// 存储可用，连接已就绪。
    Ready(Connection),
    /// 存储缺失或损坏、要从后端重建，但这次没建成（后端调不了或同步失败），下次打开会再试。
    RebuildPending(String),
}

/// 打开前看到的磁盘状态，决定要不要从后端重建。
#[derive(Clone, Copy)]
struct StorageCondition {
    database_missing: bool,
    metadata_missing: bool,
    database_rebuilt: bool,
}

impl StorageCondition {
    fn needs_rebuild(self) -> bool {
        self.database_missing || self.metadata_missing || self.database_rebuilt
    }

    /// 这次打开新建了数据库文件，重建没完成时要删掉它。
    fn created_database(self) -> bool {
        self.database_missing || self.database_rebuilt
    }
}

impl RepositoryState {
    /// 打开资源库数据库；需要重建却没建成时返回错误。
    pub(super) fn open_repository_connection(
        &self,
        repo_id: &str,
        repo_path: &str,
        backend_record: &RepositoryBackendRecord,
    ) -> Result<Connection, String> {
        match self.open_repository_storage(repo_id, repo_path, backend_record)? {
            RepositoryStorage::Ready(connection) => Ok(connection),
            RepositoryStorage::RebuildPending(error) => Err(error),
        }
    }

    /// 打开资源库存储，`.momo` 被删、数据库缺失或损坏时从后端重建。
    pub(super) fn open_repository_storage(
        &self,
        repo_id: &str,
        repo_path: &str,
        backend_record: &RepositoryBackendRecord,
    ) -> Result<RepositoryStorage, String> {
        let repo_root = Path::new(repo_path);
        let storage_paths = ensure_repository_storage_paths(
            &self.root,
            repo_id,
            repo_root,
            &backend_record.plugin_id,
        )?;
        let database_missing = !storage_paths.database_path.exists();
        let metadata_missing = !storage_paths
            .metadata_dir
            .join(REPO_METADATA_FILE_NAME)
            .exists();
        // 后端调不了就别开始重建：建出来的空库和元数据会被下次打开当成已重建。
        if database_missing || metadata_missing {
            if let Some(error) = self.rebuild_blocked_error(&backend_record.plugin_id) {
                return Ok(RepositoryStorage::RebuildPending(error));
            }
        }
        let (mut connection, database_rebuilt) =
            match self.try_open_repository_connection(&storage_paths.database_path) {
                Ok(connection) => (connection, false),
                Err(error)
                    if storage_paths.database_path.exists()
                        && is_database_locked_error_message(&error) =>
                {
                    return Err(error);
                }
                Err(_) if storage_paths.database_path.exists() => {
                    // 损坏的库只有后端能补回内容，后端调不了就先留着原文件，等插件恢复再删再建。
                    if let Some(error) = self.rebuild_blocked_error(&backend_record.plugin_id) {
                        return Ok(RepositoryStorage::RebuildPending(error));
                    }
                    // 数据库文件损坏时直接重建，再从后端重新同步内容。
                    fs::remove_file(&storage_paths.database_path).map_err(io_error)?;
                    (
                        self.try_open_repository_connection(&storage_paths.database_path)?,
                        true,
                    )
                }
                Err(error) => return Err(error),
            };
        let condition = StorageCondition {
            database_missing,
            metadata_missing,
            database_rebuilt,
        };
        match self.prepare_repository_storage(
            &mut connection,
            repo_id,
            repo_root,
            &storage_paths,
            condition,
        ) {
            Ok(()) => Ok(RepositoryStorage::Ready(connection)),
            Err(error) if condition.needs_rebuild() => {
                // 重建没完成：丢掉这次新建的库，元数据也还没写，下次打开会从头再建。
                drop(connection);
                if condition.created_database() {
                    discard_unfinished_database(&storage_paths.database_path);
                }
                Ok(RepositoryStorage::RebuildPending(error))
            }
            Err(error) => Err(error),
        }
    }

    /// 补身份记录；需要重建时从后端全量同步，同步成功后才写元数据文件。
    fn prepare_repository_storage(
        &self,
        connection: &mut Connection,
        repo_id: &str,
        repo_root: &Path,
        storage_paths: &RepositoryStoragePaths,
        condition: StorageCondition,
    ) -> Result<(), String> {
        let repo = self.load_repository_record(repo_id)?;
        if condition.needs_rebuild()
            || !repository_identity_record_is_current(connection, &repo).map_err(db_error)?
        {
            ensure_repository_identity_record(connection, &repo)?;
        }
        if !condition.needs_rebuild() {
            return Ok(());
        }
        // `.momo` 被删或被读链路部分重建后，这里补一次同步，确保首次打开就能看到内容。
        let tx = connection.transaction().map_err(db_error)?;
        sync_repository_files(
            &self.root,
            &tx,
            &repo,
            &HashSet::new(),
            &std::collections::BTreeSet::new(),
            &NeverCancelled,
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        // 元数据文件是「已经重建过」的标记，同步成功后才写；同步失败时下次打开还会重建。
        if condition.metadata_missing {
            write_repository_metadata(
                &storage_paths.metadata_dir,
                &repo.summary.repo_id,
                &repo.summary.name,
                repo_root,
                &repo.backend_record.plugin_id,
                &repo.backend_record.config,
                None,
            )?;
        }
        Ok(())
    }

    /// 后端插件接不了调用时，返回「要重建但建不了」的错误。
    fn rebuild_blocked_error(&self, plugin_id: &str) -> Option<String> {
        plugin_catalog(&self.root)
            .call_unavailable_reason(plugin_id)
            .map(|reason| {
                format!(
                    "repository storage needs a rebuild but its backend plugin is unavailable: {plugin_id} ({reason})"
                )
            })
    }

    fn try_open_repository_connection(&self, database_path: &Path) -> Result<Connection, String> {
        open_repository_database_connection(database_path)
    }
}

/// 删掉重建没完成时新建的数据库文件（连同 WAL、SHM），让下次打开重新重建。
fn discard_unfinished_database(database_path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let mut path = database_path.as_os_str().to_owned();
        path.push(suffix);
        let path = PathBuf::from(path);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => crate::app_log!(
                "warn",
                "repository.storage",
                "discardUnfinishedDatabaseFailed",
                "重建没完成，删除这次新建的数据库文件失败，下次打开可能看到空库。",
                serde_json::json!({
                    "path": path.to_string_lossy().to_string(),
                    "error": error.to_string(),
                })
            ),
        }
    }
}
