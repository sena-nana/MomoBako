//! 硬链接候选：显式读取、启动和换仓库后的后台检查、结构更新后的静默重读，以及确认框里的确认和跳过。
//!
//! 对应 Vue `HardlinkCandidateDialog.vue` 和 `listHardlinkCandidates` / `confirmHardlinkCandidate`：
//! 确认框显示第一个没跳过的候选，跳过只记在本地，不调用服务。确认成功以后照 Vue 重读候选、仓库摘要和
//! 当前目录（[`finish_confirm`]）。
//!
//! 读取是并发的（宿主有多个任务线程），回来的先后不一定是发出的先后。候选列表因此带一个版本
//! （`hardlink_epoch`）：确认结果回来、换仓库时加一，每次读取带着发出时的版本，回来时比现在旧就丢掉，
//! 确认掉的候选不会被确认以前发出的读取写回来，别的仓库的候选也进不来。确认结果回来以后到重读的候选
//! 回来之前，确认框照 Vue `isMutatingFiles` 一直处理中，不能对着要被换掉的旧列表再点。

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

    /// 候选列表现在的版本。新发出的读取带上它，回来时据此认过期。
    pub(in crate::shell) fn hardlink_epoch(&self) -> u64 {
        self.hardlink_epoch
    }

    /// 确认框处理中：确认请求在途，或者确认以后的候选重读还没回来。按钮禁用，点外面和 Escape 不跳过。
    pub(in crate::shell) fn hardlink_busy(&self) -> bool {
        self.mutating || self.hardlink_rereading
    }

    /// 换仓库：之前发出的读取都作废，正在等的重读也不等了。
    pub(in crate::shell) fn forget_hardlink_reads(&mut self) {
        self.hardlink_epoch = self.hardlink_epoch.wrapping_add(1);
        self.hardlink_rereading = false;
    }

    /// 验收页打开硬链接确认。路径来自调用方，不编造哈希。
    pub(in crate::shell) fn present_hardlink(&mut self, prompt: HardlinkPrompt) {
        self.export.open = false;
        self.dialog = FileDialog::Hardlink;
        self.skipped_hardlinks.clear();
        self.hardlinks = vec![prompt];
    }

    pub(in crate::shell) fn confirm_hardlink(&mut self, ctx: &FileContext) -> bool {
        if self.hardlink_busy() {
            eprintln!("Nana 文件变更或候选重读进行中，不能确认硬链接");
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
        if self.hardlink_busy() {
            eprintln!("Nana 文件变更或候选重读进行中，不能跳过硬链接");
            return false;
        }
        let Some(prompt) = self.current_hardlink().cloned() else {
            return false;
        };
        self.skipped_hardlinks.push(prompt.id);
        self.settle_hardlink_dialog();
        true
    }

    /// 复制以后读到的候选：换掉列表、清掉跳过，有候选就弹出确认；失败写进文件列。过期的结果丢掉。
    pub(in crate::shell) fn note_hardlinks(&mut self, epoch: u64, result: Result<Vec<HardlinkPrompt>, String>) {
        if !self.accepts_hardlinks(epoch, "复制以后") {
            return;
        }
        match result {
            Ok(prompts) => {
                self.hardlinks = prompts;
                self.skipped_hardlinks.clear();
                if self.current_hardlink().is_some() {
                    self.dialog = FileDialog::Hardlink;
                }
                self.settle_hardlink_dialog();
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
        self.effects.push(FilesEffect::CheckHardlinks { repo_id: repo_id.to_string(), epoch: self.hardlink_epoch });
    }

    /// 结构更新静默重读硬链接候选。空仓库只记日志。
    /// 不打开对话框，不改加载态和页面错误。
    pub(in crate::shell) fn refresh_hardlinks_silent(&mut self, repo_id: &str) {
        if repo_id.is_empty() {
            eprintln!("Nana 静默刷新硬链接候选缺少仓库");
            return;
        }
        self.effects.push(FilesEffect::RefreshHardlinks { repo_id: repo_id.to_string(), epoch: self.hardlink_epoch });
    }

    /// 后台查到（启动、换仓库、确认以后）或刷新文件夹树同步以后重读的候选：换掉列表，跳过的留着；没有别的对话框
    /// 开着时，有候选就弹出确认。Vue 的确认框按「第一个没跳过的候选」算出来，候选一到就出现。过期的结果丢掉。
    pub(in crate::shell) fn note_hardlinks_synced(&mut self, epoch: u64, prompts: Vec<HardlinkPrompt>) {
        if !self.accepts_hardlinks(epoch, "后台") {
            return;
        }
        self.hardlinks = prompts;
        if self.dialog == FileDialog::Closed && self.current_hardlink().is_some() {
            self.dialog = FileDialog::Hardlink;
        }
        self.settle_hardlink_dialog();
    }

    /// 后台读候选失败：只记日志，留着本地这份；确认以后在等的重读失败时也放开确认框。
    pub(in crate::shell) fn note_hardlinks_check_failed(&mut self, epoch: u64, error: &str) {
        if self.accepts_hardlinks(epoch, "后台") {
            eprintln!("Nana 后台读取硬链接候选失败：{error}");
        }
    }

    /// 静默替换硬链接候选。成功只换列表，不打开确认框（换掉以后没有候选可显示时收起）；失败只记日志。
    /// 不改已跳过候选和页面错误。过期的结果丢掉。
    pub(in crate::shell) fn note_hardlinks_silent(&mut self, epoch: u64, result: Result<Vec<HardlinkPrompt>, String>) {
        if !self.accepts_hardlinks(epoch, "静默") {
            return;
        }
        match result {
            Ok(prompts) => {
                self.hardlinks = prompts;
                self.settle_hardlink_dialog();
            }
            Err(error) => {
                eprintln!("Nana 静默刷新硬链接候选失败：{error}");
            }
        }
    }

    /// 确认的结果落到候选上：成功按 id 移除、没有剩余就关掉确认框；失败留着这条，写下错误。列表的版本加一，
    /// 确认以前发出的读取回来时都算过期。重读由 [`finish_confirm`] 排。
    pub(in crate::shell) fn note_hardlink_confirmed(&mut self, result: Result<String, String>) {
        self.mutating = false;
        self.hardlink_epoch = self.hardlink_epoch.wrapping_add(1);
        match result {
            Ok(id) => {
                self.hardlinks.retain(|prompt| prompt.id != id);
                self.skipped_hardlinks.retain(|item| item != &id);
                self.settle_hardlink_dialog();
            }
            Err(error) => {
                eprintln!("Nana 确认硬链接失败：{error}");
                self.error = error;
            }
        }
    }

    /// 读到的候选认不认：比现在的列表旧（确认以前或换仓库以前发出的）就丢掉。认下的结果是确认以后发出的，
    /// 等着的重读就此结束，确认框放开。
    fn accepts_hardlinks(&mut self, epoch: u64, kind: &str) -> bool {
        if epoch < self.hardlink_epoch {
            eprintln!("Nana 忽略过期的{kind}硬链接候选：版本 {epoch}，当前 {}", self.hardlink_epoch);
            return false;
        }
        self.hardlink_rereading = false;
        true
    }

    /// 确认框跟着列表走（Vue 的确认框就是 `currentHardlinkCandidate`）：没有可显示的候选时收起，不留一个画不出来的
    /// 打开状态。
    fn settle_hardlink_dialog(&mut self) {
        if self.dialog == FileDialog::Hardlink && self.current_hardlink().is_none() {
            self.dialog = FileDialog::Closed;
        }
    }
}

/// 确认硬链接的结果到了（Vue `confirmWorkspaceHardlinkCandidate`）。先落到候选上；确认时的仓库还是活动仓库时：
/// - 成功照 Vue `refreshWorkspace(repoId, { directory, hardlinkCandidates, repositorySnapshot })`：重读候选（换掉
///   列表、跳过的留着，还有没跳过的就接着弹确认），静默重读仓库摘要，不在回收站和智能文件夹时非静默地重读当前
///   目录（任务弹层「读取目录」），新文件的硬链接状态和侧栏计数跟着更新；
/// - 失败照 Vue 静默重读候选（读失败只记日志），确认框按最新的列表走，错误留在文件列。
///
/// 两种都照 Vue 在候选重读回来之前让确认框处理中：确认掉或已经失效的候选不会被再点一次。
pub(in crate::shell) fn finish_confirm(model: &mut ShellViewModel, repo_id: &str, result: Result<String, String>) {
    let confirmed = result.is_ok();
    model.files.note_hardlink_confirmed(result);
    if repo_id.is_empty() || model.workspace.active_repo_id.as_deref() != Some(repo_id) {
        eprintln!("Nana 确认硬链接回来时已经换了仓库，不重读：{repo_id}");
        return;
    }
    model.files.hardlink_rereading = true;
    if !confirmed {
        model.files.refresh_hardlinks_silent(repo_id);
        return;
    }
    model.files.check_hardlinks(repo_id);
    model.reload_after_mutation(repo_id);
}
