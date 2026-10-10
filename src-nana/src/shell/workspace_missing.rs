//! 缺失仓库页的动作和删除资源库对话框。
//!
//! 对应 Vue `MissingRepositoryState.vue`、`useMissingRepositoryActions.ts` 和 `RepositoryDeleteDialog.vue`：
//! 刷新、选新路径并重定向、打开来源设置，以及删除资源库的三种方式（只删记录、删 .momo 数据、删整个文件夹）
//! 能不能选、说明文案和删除结果。重定向的结果在 `workspace_switch.rs`。

use super::super::repo_operation::OperationStart;
use super::{DeleteDialog, DeleteMode, WorkspaceDialog, WorkspaceEffect, WorkspaceRepository, WorkspaceState};

impl WorkspaceState {
    pub fn refresh_missing(&mut self) {
        if self.missing_busy() {
            return;
        }
        self.missing_error.clear();
        self.request_repository_refresh();
    }

    pub fn choose_missing_path(&mut self) {
        if self.active_repo_id.is_none() || self.missing_busy() {
            return;
        }
        self.missing_error.clear();
        self.path_prompt = true;
    }

    pub fn set_path_draft(&mut self, value: String) {
        self.path_draft = value;
    }

    /// 空选择不进入重定向，避免对话框取消被当成一次修复。
    pub fn submit_missing_path(&mut self) {
        if self.missing_busy() {
            return;
        }
        let Some(repo_id) = self.active_repo_id.clone() else {
            return;
        };
        let path = self.path_draft.trim().to_string();
        if path.is_empty() {
            return;
        }
        self.path_prompt = false;
        self.relocating = true;
        self.missing_error.clear();
        self.operation.start(OperationStart::new("重定向资源库", "校验资源库位置", 12.0));
        self.effects.push(WorkspaceEffect::RelocateRepository { repo_id, path });
    }

    /// 来源缓存没就绪时「打开来源设置」：返回要打开设置的来源插件。忙着或不是缓存问题时不打开。
    pub fn open_source_settings(&mut self) -> Option<String> {
        if self.missing_busy() {
            return None;
        }
        let plugin_id = self
            .active_repository()
            .filter(|repository| repository.is_source_cache_issue())
            .map(|repository| repository.backend_plugin_id.clone())?;
        self.missing_error.clear();
        Some(plugin_id)
    }

    pub fn delete_dialog_open(&self) -> bool {
        self.dialogs.iter().any(|dialog| matches!(dialog, WorkspaceDialog::Delete(_)))
    }

    pub fn open_delete_dialog(&mut self) {
        let Some(repo_id) = self.active_repo_id.clone() else {
            return;
        };
        if self.missing_busy() || !self.repositories.iter().any(|repository| repository.repo_id == repo_id) {
            return;
        }
        self.missing_error.clear();
        if let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last_mut() {
            dialog.repo_id = repo_id;
            dialog.error.clear();
            return;
        }
        self.dialogs.push(WorkspaceDialog::Delete(DeleteDialog { repo_id, error: String::new() }));
    }

    pub fn close_delete_dialog(&mut self) {
        if self.deleting_mode.is_some() {
            return;
        }
        self.dialogs.pop();
    }

    pub fn confirm_delete(&mut self, mode: DeleteMode) -> bool {
        if self.deleting_mode.is_some() || !self.delete_mode_enabled(mode) {
            return false;
        }
        let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last() else {
            return false;
        };
        let repo_id = dialog.repo_id.clone();
        self.deleting_mode = Some(mode);
        self.effects.push(WorkspaceEffect::DeleteRepository { repo_id, mode });
        true
    }

    pub fn note_delete_finished(&mut self, result: Result<(), String>) {
        let mode = self.deleting_mode.take();
        let Some(mode) = mode else {
            return;
        };
        match result {
            Ok(()) => {
                let repo_id = self.dialogs.last().and_then(|dialog| match dialog {
                    WorkspaceDialog::Delete(dialog) => Some(dialog.repo_id.clone()),
                });
                self.dialogs.clear();
                if let Some(repo_id) = repo_id {
                    self.remove_repository(&repo_id);
                }
                let _ = mode;
            }
            Err(error) => {
                eprintln!("Nana 删除资源库失败：{error}");
                if let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last_mut() {
                    dialog.error = error;
                }
            }
        }
    }

    pub fn delete_mode_enabled(&self, mode: DeleteMode) -> bool {
        let Some(WorkspaceDialog::Delete(dialog)) = self.dialogs.last() else {
            return false;
        };
        let Some(repository) = self.repositories.iter().find(|item| item.repo_id == dialog.repo_id) else {
            return false;
        };
        match mode {
            DeleteMode::RecordOnly => true,
            DeleteMode::DeleteMetadata => can_delete_metadata(repository),
            DeleteMode::DeleteFolder => can_delete_folder(repository),
        }
    }

    pub fn delete_mode_detail(&self, mode: DeleteMode) -> String {
        let cache_required = self
            .dialogs
            .last()
            .and_then(|dialog| match dialog {
                WorkspaceDialog::Delete(dialog) => self.repositories.iter().find(|item| item.repo_id == dialog.repo_id),
            })
            .is_some_and(|repository| repository.cache_required);
        let enabled = self.delete_mode_enabled(mode);
        match mode {
            DeleteMode::RecordOnly => "只从 MomoBako 移除这条资源库记录，保留文件夹、.momo 和应用托管数据。".into(),
            DeleteMode::DeleteMetadata if enabled && cache_required => {
                "删除该资源库的 Momo 元数据目录与索引缓存，保留缓存文件夹中的其他用户内容。".into()
            }
            DeleteMode::DeleteMetadata if enabled => "删除该资源库的 .momo 数据目录，保留原文件夹与用户文件。".into(),
            DeleteMode::DeleteMetadata => "当前资源库的 .momo 目录不可直接访问，请先恢复路径后再删除元数据。".into(),
            DeleteMode::DeleteFolder if enabled => "递归删除当前资源库文件夹，目录内的 .momo 数据与用户文件会一起删除。".into(),
            DeleteMode::DeleteFolder => "当前资源库目录不可直接访问，暂时不能删除整个文件夹。".into(),
        }
    }

    pub fn missing_primary_label(&self) -> &'static str {
        if self.active_repository().is_some_and(|repository| repository.is_source_cache_issue()) {
            "打开来源设置"
        } else if self.relocating {
            "重定向中..."
        } else {
            "重定向"
        }
    }

    pub fn missing_delete_label(&self) -> &'static str {
        if self.deleting_mode.is_some() { "删除中..." } else { "删除资源库" }
    }
}

fn repository_uses_local_metadata(repository: &WorkspaceRepository) -> bool {
    repository.capabilities.iter().any(|capability| capability == "localRootPath")
        || (repository.cache_required && repository.cache_status != "unconfigured")
}

fn repository_metadata_accessible(repository: &WorkspaceRepository) -> bool {
    if repository.cache_required {
        repository.cache_status == "ready"
    } else {
        repository.status == "ready"
    }
}

fn can_delete_metadata(repository: &WorkspaceRepository) -> bool {
    if repository_uses_local_metadata(repository) {
        repository_metadata_accessible(repository)
    } else {
        true
    }
}

fn can_delete_folder(repository: &WorkspaceRepository) -> bool {
    repository_uses_local_metadata(repository) && repository_metadata_accessible(repository)
}
