//! 来源插件下载还缺的目录。
//!
//! 下载先排队系统文件夹对话框，选中后再 `call_plugin`。取消不调用。

use serde_json::Value;

use super::super::admin::AdminMessage;
use super::super::repo_operation::{OperationPatch, OperationStart};
use super::super::{ShellMessage, ShellViewModel};

/// 已经点过下载、还在等目录的插件动作。
#[derive(Clone, Debug)]
pub struct PendingDownload {
    pub plugin_id: String,
    pub method: String,
    pub payload: Value,
    pub repository_id: Option<String>,
    /// 选好目录以后在任务弹层里出的进度行。
    pub progress: super::super::entry_actions::DownloadProgress,
}

/// 把选中的目录写进 `destination` 再调用。没有待下载动作时只记日志。
pub(super) fn submit_download(model: &mut ShellViewModel, path: String) {
    let Some(mut pending) = model.input.pending_download.take() else {
        eprintln!("Nana 下载目录没有对应的插件动作");
        return;
    };
    let trimmed = path.trim();
    if trimmed.is_empty() {
        eprintln!("Nana 取消下载目录选择");
        return;
    }
    if let Some(object) = pending.payload.as_object_mut() {
        object.insert(
            "destination".into(),
            serde_json::json!({ "kind": "localFolder", "path": trimmed }),
        );
    }
    // Vue `runDownloadAction`：「{动作名} · 准备下载 …」10%，调用插件时「正在下载 …」36% 不确定进度。
    let progress = &pending.progress;
    let operation = model.operation.start(OperationStart::new(progress.label.clone(), progress.preparing.clone(), 10.0));
    if let Some(running) = &progress.running {
        model.operation.update(operation, OperationPatch::step(running.clone(), 36.0).with_indeterminate(true));
    }
    model.admin.note_download_operation(pending.method.clone(), operation);
    model.reduce(ShellMessage::Admin(AdminMessage::CallFilePlugin {
        plugin_id: pending.plugin_id,
        method: pending.method,
        payload: pending.payload,
        repository_id: pending.repository_id,
    }));
}
