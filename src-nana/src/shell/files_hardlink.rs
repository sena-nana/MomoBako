//! 硬链接候选：显式读取、启动和换仓库后的后台检查、结构更新后的静默重读，以及确认框里的确认和跳过。
//!
//! 对应 Vue `HardlinkCandidateDialog.vue` 和 `listHardlinkCandidates` / `confirmHardlinkCandidate`：
//! 确认框显示第一个没跳过的候选，跳过只记在本地，不调用服务。确认成功以后照 Vue 重读候选、仓库摘要和
//! 当前目录（[`finish_confirm`]）。

use super::super::ShellViewModel;
use super::{FileContext, FileDialog, FilesEffect, FilesState};

/// 硬链接确认框只保留文案需要的字段。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HardlinkPrompt {
    pub id: String,
    pub new_path: String,
    pub existing_path: String,
    pub size_label: String,
}

impl HardlinkPrompt {
    pub fn message(&self) -> String {
        format!(
            "{} 与 {} 内容哈希一致，大小 {}。确认后会将新文件加入硬链接关联。",
            self.new_path, self.existing_path, self.size_label
        )
    }
}

/// 列表和详情里硬链接状态的文案。没有状态或状态未知时为空串。
pub(crate) fn hardlink_label(state: Option<&str>) -> &'static str {
    match state.unwrap_or("") {
        "primary" => "主归属",
        "linked" => "硬链接关联",
        "copied" | "copiedFallback" => "普通复制",
        "broken" => "关联异常",
        "missing" => "关联缺失",
        _ => "",
    }
}

impl FilesState {
    pub(in crate::shell) fn current_hardlink(&self) -> Option<&HardlinkPrompt> {
        self.hardlinks.iter().find(|prompt| !self.skipped_hardlinks.iter().any(|id| id == &prompt.id))
    }

    /// 验收页打开硬链接确认。路径来自调用方，不编造哈希。
    pub(in crate::shell) fn present_hardlink(&mut self, prompt: HardlinkPrompt) {
        self.export.open = false;
        self.dialog = FileDialog::Hardlink;
        self.skipped_hardlinks.clear();
        self.hardlinks = vec![prompt];
    }

    pub(in crate::shell) fn confirm_hardlink(&mut self, ctx: &FileContext) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能确认硬链接");
            return false;
        }
        let Some(prompt) = self.current_hardlink().cloned() else {
            return false;
        };
        let Some(repo_id) = ctx.repo_id.clone() else {
            eprintln!("Nana 确认硬链接没有活动仓库");
            return false;
        };
        self.begin(None);
        self.effects.push(FilesEffect::ConfirmHardlink { repo_id, candidate_id: prompt.id });
        true
    }

    /// 跳过只记在本地，不调用服务，接着显示下一条未跳过的候选。
    pub(in crate::shell) fn skip_hardlink(&mut self) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能跳过硬链接");
            return false;
        }
        let Some(prompt) = self.current_hardlink().cloned() else {
            return false;
        };
        self.skipped_hardlinks.push(prompt.id);
        if self.current_hardlink().is_none() {
            self.dialog = FileDialog::Closed;
        }
        true
    }

    pub(in crate::shell) fn note_hardlinks(&mut self, result: Result<Vec<HardlinkPrompt>, String>) {
        match result {
            Ok(prompts) => {
                self.hardlinks = prompts;
                self.skipped_hardlinks.clear();
                if self.current_hardlink().is_some() {
                    self.dialog = FileDialog::Hardlink;
                }
            }
            Err(error) => {
                eprintln!("Nana 读取硬链接候选失败：{error}");
                self.error = error;
            }
        }
    }

    /// 启动和换仓库后在后台查一次硬链接候选，和 Vue 后台读 `listHardlinkCandidates` 一致：
    /// 启动同步刚建出来的候选要让用户确认。
    pub(in crate::shell) fn check_hardlinks(&mut self, repo_id: &str) {
        self.effects.push(FilesEffect::CheckHardlinks { repo_id: repo_id.to_string() });
    }

    /// 结构更新静默重读硬链接候选。空仓库只记日志。
    /// 不打开对话框，不改加载态和页面错误。
    pub(in crate::shell) fn refresh_hardlinks_silent(&mut self, repo_id: &str) {
        if repo_id.is_empty() {
            eprintln!("Nana 静默刷新硬链接候选缺少仓库");
            return;
        }
        self.effects.push(FilesEffect::RefreshHardlinks { repo_id: repo_id.to_string() });
    }

    /// 刷新文件夹树同步以后重读的候选：换掉列表，跳过的留着；没有别的对话框开着时，有候选就弹出确认。
    /// Vue 的确认框按「第一个没跳过的候选」算出来，候选一到就出现。
    pub(in crate::shell) fn note_hardlinks_synced(&mut self, prompts: Vec<HardlinkPrompt>) {
        self.hardlinks = prompts;
        if self.dialog == FileDialog::Closed && self.current_hardlink().is_some() {
            self.dialog = FileDialog::Hardlink;
        }
    }

    /// 静默替换硬链接候选。成功只换列表；失败只记日志。
    /// 不改对话框、已跳过候选和页面错误。
    pub(in crate::shell) fn note_hardlinks_silent(&mut self, result: Result<Vec<HardlinkPrompt>, String>) {
        match result {
            Ok(prompts) => {
                self.hardlinks = prompts;
            }
            Err(error) => {
                eprintln!("Nana 静默刷新硬链接候选失败：{error}");
            }
        }
    }

    /// 确认的结果落到候选上：成功按 id 移除、没有剩余就关掉确认框；失败留着这条，写下错误。重读由
    /// [`finish_confirm`] 排。
    pub(in crate::shell) fn note_hardlink_confirmed(&mut self, result: Result<String, String>) {
        self.mutating = false;
        match result {
            Ok(id) => {
                self.hardlinks.retain(|prompt| prompt.id != id);
                self.skipped_hardlinks.retain(|item| item != &id);
                if self.current_hardlink().is_none() {
                    self.dialog = FileDialog::Closed;
                } else {
                    self.dialog = FileDialog::Hardlink;
                }
            }
            Err(error) => {
                eprintln!("Nana 确认硬链接失败：{error}");
                self.error = error;
            }
        }
    }
}

/// 确认硬链接的结果到了（Vue `confirmWorkspaceHardlinkCandidate`）。先落到候选上；确认时的仓库还是活动仓库时：
/// - 成功照 Vue `refreshWorkspace(repoId, { directory, hardlinkCandidates, repositorySnapshot })`：重读候选（换掉
///   列表、跳过的留着，还有没跳过的就接着弹确认），静默重读仓库摘要，不在回收站和智能文件夹时非静默地重读当前
///   目录（任务弹层「读取目录」），新文件的硬链接状态和侧栏计数跟着更新；
/// - 失败照 Vue 静默重读候选（读失败只记日志），确认框按最新的列表走，错误留在文件列。
pub(in crate::shell) fn finish_confirm(model: &mut ShellViewModel, repo_id: &str, result: Result<String, String>) {
    let confirmed = result.is_ok();
    model.files.note_hardlink_confirmed(result);
    if model.workspace.active_repo_id.as_deref() != Some(repo_id) {
        eprintln!("Nana 确认硬链接回来时已经换了仓库，不重读：{repo_id}");
        return;
    }
    if !confirmed {
        model.files.refresh_hardlinks_silent(repo_id);
        return;
    }
    model.files.check_hardlinks(repo_id);
    model.reload_after_mutation(repo_id);
}
