//! 把访问记录交给仓库交互 ViewModel（`record_entry_access`）。
//!
//! 本地文件直接读了字节（预览的文本、文档、音视频，播放条的当前项）或交给系统程序打开时，后端没有经手，
//! 由这里补一次访问记录，结果回到 `library.rs` 当场改「最近使用」。失败只记日志，不挡住打开和预览。
//! 任务都经 `task_guard` 派发。

use nana_ui::RuntimeProgramContext;
use serde_json::json;

use crate::backend::services::repository::EntryAccessRecordRequest;
use crate::shell::library::LibraryEffect;
use crate::shell::ShellMessage;
use crate::task_guard::spawn_service;
use crate::MomoBakoApplication;

/// 执行资源库素材归约留下的请求。
pub fn dispatch_library_effects(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    for effect in app.shell.library.take_effects() {
        match effect {
            LibraryEffect::RecordAccess { repo_id, path } => dispatch_record_access(app, context, repo_id, path),
        }
    }
}

fn dispatch_record_access(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, path: String) {
    let (failed_repo, failed_path) = (repo_id.clone(), path.clone());
    let failed = move |error| ShellMessage::EntryAccessRecorded { repo_id: failed_repo.clone(), path: failed_path.clone(), result: Err(error) };
    spawn_service(app.services.as_ref(), context, "记录访问", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || {
            let request = EntryAccessRecordRequest { repo_id: repo_id.clone(), path: path.clone() };
            let result = executor.block_on(interaction.record_entry_access(request)).map(|response| response.recorded_at);
            if let Err(error) = &result {
                momobako_backend::app_log!(
                    "warn",
                    "file",
                    "recordAccessFailed",
                    "记录最近使用失败。",
                    json!({ "repoId": repo_id, "path": path, "error": error })
                );
            }
            ShellMessage::EntryAccessRecorded { repo_id, path, result }
        }
    });
}
