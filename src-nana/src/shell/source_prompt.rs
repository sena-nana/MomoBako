//! 来源插件下载还缺的目录。
//!
//! 下载先排队系统文件夹对话框，选中后再 `call_plugin`。取消不调用。结果回来以后的收尾在 `source_download.rs`。

use serde_json::Value;

use super::super::admin::source_download::{DownloadDestination, DownloadRun};
use super::super::admin::{AdminEffect, PluginCallOrigin};
use super::super::ShellViewModel;

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

/// 把选中的目录写进 `destination` 再调用（任务弹层的进度行见 [`DownloadRun::begin`]）。没有待下载动作时只记日志。
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
    let destination = DownloadDestination::LocalFolder(trimmed.to_string());
    let run = DownloadRun::begin(model, pending.progress, pending.plugin_id.clone(), pending.method.clone(), pending.repository_id.clone(), destination);
    model.admin.push_effect(AdminEffect::CallPlugin {
        plugin_id: pending.plugin_id,
        method: pending.method,
        payload: pending.payload,
        repository_id: pending.repository_id,
        origin: PluginCallOrigin::Download(run),
    });
}
