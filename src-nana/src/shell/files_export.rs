//! 资源库导出对话框：草稿、提交和导出协议的结果。
//!
//! 压缩包导出必须先有真实的输出路径（还没有时由 `files_reduce.rs` 先弹系统保存对话框），Git 导出的
//! 远端、分支和说明照对话框里填的交给协议。首页不提供入口，只有验收页打开它。

use super::{FileContext, FileDialog, FilesEffect, FilesState};

/// 导出对话框草稿。首页不打开它。
#[derive(Clone, Debug, Default)]
pub(in crate::shell) struct ExportDraft {
    pub open: bool,
    pub target: String,
    pub format: String,
    pub compression: String,
    pub encrypt: bool,
    pub password: String,
    pub remote: String,
    pub branch: String,
    pub message: String,
    pub error: String,
    /// 压缩包保存路径。只接受对话框或用户填入的路径。
    pub output_path: String,
    /// 宿主返回的成功说明。失败仍写在 `error`。
    pub notice: String,
    pub busy: bool,
}

impl FilesState {
    /// 验收页只打开导出对话框本身。
    pub(in crate::shell) fn present_export(&mut self) {
        self.dialog = FileDialog::Closed;
        self.export.open = true;
        self.export.target = "archive".into();
        self.export.format = "zip".into();
        self.export.compression = "balanced".into();
        self.export.output_path.clear();
        self.export.notice.clear();
        self.export.error.clear();
        self.export.busy = false;
    }

    pub(in crate::shell) fn close_export(&mut self) {
        self.export.open = false;
        self.export.error.clear();
        self.export.notice.clear();
        self.export.busy = false;
    }

    /// 把对话框草稿交给导出协议。压缩包没有输出路径时只记失败，不调用宿主。
    pub(in crate::shell) fn submit_export(&mut self, ctx: &FileContext) {
        if self.export.busy {
            eprintln!("Nana 资源库导出进行中，不能重复提交");
            return;
        }
        let Some(repo_id) = ctx.repo_id.clone().filter(|id| !id.trim().is_empty()) else {
            eprintln!("Nana 资源库导出缺少当前仓库");
            self.export.notice.clear();
            self.export.error = "没有当前资源库，不能导出。".into();
            return;
        };
        self.export.error.clear();
        self.export.notice.clear();
        if self.export.target == "git" {
            self.export.busy = true;
            self.export.notice = "正在导出到 Git…".into();
            self.effects.push(FilesEffect::ExportGit {
                repo_id,
                remote: self.export.remote.clone(),
                branch: self.export.branch.clone(),
                message: self.export.message.clone(),
            });
            return;
        }
        let output_path = self.export.output_path.trim().to_string();
        if output_path.is_empty() {
            eprintln!("Nana 压缩包导出还没有输出路径，没有调用导出命令");
            self.export.error = "压缩包导出还没有输出路径。".into();
            return;
        }
        self.export.busy = true;
        self.export.notice = "正在导出压缩包…".into();
        self.effects.push(FilesEffect::ExportArchive {
            repo_id,
            format: self.export.format.clone(),
            output_path,
            compression: self.export.compression.clone(),
            encrypt: self.export.encrypt,
            password: self.export.password.clone(),
        });
    }

    /// 写下导出协议的真实结果。成功不改成固定文案。
    pub(in crate::shell) fn note_export(&mut self, result: Result<String, String>) {
        self.export.busy = false;
        match result {
            Ok(notice) => {
                self.export.error.clear();
                self.export.notice = notice;
            }
            Err(error) => {
                eprintln!("Nana 资源库导出失败：{error}");
                self.export.notice.clear();
                self.export.error = if error.is_empty() { "资源库导出失败。".into() } else { error };
            }
        }
    }

    /// 只改导出对话框草稿。未知字段记日志，不改其它状态。
    pub(in crate::shell) fn set_export_field(&mut self, field: &str, value: String) {
        match field {
            "target" => self.export.target = if value == "git" { "git".into() } else { "archive".into() },
            "format" => self.export.format = value,
            "compression" => self.export.compression = value,
            "encrypt" => self.export.encrypt = value == "1",
            "password" => self.export.password = value,
            "remote" => self.export.remote = value,
            "branch" => self.export.branch = value,
            "message" => self.export.message = value,
            "output" => self.export.output_path = value,
            other => eprintln!("Nana 导出对话框没有这个字段：{other}"),
        }
    }
}
