//! 文件表面在任务弹层「仓库操作」行上的那部分：文件变更和非静默的读目录。
//!
//! 对应 Vue `fileOperations.ts` 和 `files.ts` 里的 `startOperationProgress` / `updateOperationProgress`：
//! 复制、导入、多选删除这些变更各有自己的种类和阶段；非静默、非追加的读目录出「读取目录」
//! （读回收站「读取回收站」，带目录树「读取文件树」）。请求排进 `FilesState::operation`，
//! 归约结束时由壳层交给槽位（`repo_operation.rs`）。换仓库时清掉上一个仓库的文件表面也在这里。

use super::super::repo_operation::{OperationPatch, OperationStart};
use super::{FileContext, FileDialog, FilesState, MenuBranch};

/// 文件变更在任务弹层里的进度行：开始时的种类、阶段和百分比，以及协议完成以后那一步。
#[derive(Clone, Debug, PartialEq)]
pub(in crate::shell) struct FileOperation {
    pub start: OperationStart,
    /// 协议完成、等着目录读回来时 Vue「刷新文件索引」那一步的百分比（复制、导入）。没有这一步的变更
    /// （删除、还原、移动）重读目录时进度行换成「读取目录」，和 Vue 刷新目录时的非静默读取一样。
    pub refresh_value: Option<f64>,
}

impl FileOperation {
    /// Vue 文件变更开始以后的第一步：种类、阶段说明和百分比。
    pub(in crate::shell) fn step(label: impl Into<String>, detail: impl Into<String>, value: f64, refresh_value: Option<f64>) -> Self {
        Self { start: OperationStart::new(label, detail, value), refresh_value }
    }

    /// 从文件夹导入，对话框和拖入都是（Vue `importEntriesToWorkspace`）。
    pub(in crate::shell) fn import() -> Self {
        Self::step("导入文件", "导入文件到当前资源库", 24.0, Some(84.0)).runs(&[crate::backend::services::mutsuki_runner::PROTOCOL_ENTRY_IMPORT])
    }

    /// 这次变更在后台跑的任务协议：任务弹层不再把这些任务单列一行（`OperationStart::runs`）。
    pub(in crate::shell) fn runs(mut self, protocols: &'static [&'static str]) -> Self {
        self.start = self.start.runs(protocols);
        self
    }
}

/// 文件表面这次占着仓库操作行的是什么：一次变更，或者一次非静默的读目录。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum FilesOperation {
    Mutation { refresh_value: Option<f64> },
    Read,
}

/// 读目录时任务弹层出不出进度行：Vue `loadFileBrowserForDirectory` 只有非静默、非追加的读取才出，
/// 标题看读的是回收站、带不带目录树。侧栏点文件夹、双击进入目录和面包屑在 Vue 里都是静默读取，
/// 不出这一行。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::shell) enum ReadProgress {
    #[default]
    Silent,
    Directory,
    Tree,
}

impl FilesState {
    /// 非静默地读目录，任务弹层出「读取目录」这一行：读回收站是「读取回收站」，带目录树是「读取文件树」；
    /// 说明写正在读的目录，根目录写「正在读取根目录」。
    pub(in crate::shell) fn report_read(&mut self, path: &str, trash: bool, read: ReadProgress) {
        let label = match read {
            ReadProgress::Silent => return,
            _ if trash => "读取回收站",
            ReadProgress::Tree => "读取文件树",
            ReadProgress::Directory => "读取目录",
        };
        let detail = if !path.is_empty() {
            format!("正在读取 {path}")
        } else if trash {
            "正在读取回收站".into()
        } else {
            "正在读取根目录".into()
        };
        self.operation.start(OperationStart::new(label, detail, 14.0).indeterminate());
        self.operation_kind = Some(FilesOperation::Read);
    }

    /// 变更开始：排下这次变更的进度行。
    pub(super) fn report_mutation(&mut self, operation: FileOperation) {
        self.operation.start(operation.start);
        self.operation_kind = Some(FilesOperation::Mutation { refresh_value: operation.refresh_value });
    }

    /// 协议完成以后重读目录时怎么报进度：复制、导入走到「刷新文件索引」，返回这一步的百分比和静默读取；
    /// 别的变更返回 `None` 和非静默读取（进度行换成「读取目录」）；没有进度行的变更静默读取。
    pub(super) fn reload_progress(&self) -> (Option<f64>, ReadProgress) {
        match self.operation_kind {
            Some(FilesOperation::Mutation { refresh_value: Some(value) }) => (Some(value), ReadProgress::Silent),
            Some(FilesOperation::Mutation { refresh_value: None }) => (None, ReadProgress::Directory),
            _ => (None, ReadProgress::Silent),
        }
    }

    /// 文件表面占着的进度行到头了：成功时读目录先走到「整理目录条目」92% 再完成，变更直接完成；
    /// 失败时取消。没有占着进度行时什么也不做。
    pub(super) fn settle_operation(&mut self, succeeded: bool) {
        let Some(kind) = self.operation_kind.take() else {
            return;
        };
        if !succeeded {
            self.operation.cancel();
            return;
        }
        if kind == FilesOperation::Read {
            self.operation.update(OperationPatch::step("整理目录条目", 92.0));
        }
        self.operation.finish();
    }

    /// 变更的结果到了：只收尾变更自己的进度行，在读的目录留给读目录收尾。
    pub(super) fn settle_mutation(&mut self, succeeded: bool) {
        if matches!(self.operation_kind, Some(FilesOperation::Mutation { .. })) {
            self.settle_operation(succeeded);
        }
    }

    /// 刷新资源库时重读目录（Vue `selectRepository` 同一个仓库的 `loadFileBrowserForDirectory`）：非静默，
    /// 选择留着，分类视图也读。
    pub(in crate::shell) fn request_refresh_read(&mut self, ctx: &FileContext, path: &str) -> bool {
        self.queue_browse(ctx, path, true, true, ReadProgress::Directory)
    }

    /// 换仓库：清掉上一个仓库的条目、选择、对话框和进行中的读取（Vue `resetActiveRepositoryContent`）。
    /// 展示方式、列表宽、导出草稿和工具栏里的新文件名这些和仓库无关的留着；文件表面占着的进度行取消。
    pub(in crate::shell) fn reset_for_repository(&mut self) {
        self.rows.clear();
        self.virtual_rows.clear();
        self.prune_decode_requests();
        self.current_path.clear();
        self.total_entries = 0;
        self.has_more = false;
        self.trash = false;
        self.loading = false;
        self.loading_more = false;
        self.error.clear();
        self.selected.clear();
        self.primary = None;
        self.anchor = None;
        self.dialog = FileDialog::Closed;
        self.pending = None;
        self.entry_menu = None;
        self.menu_branch = MenuBranch::default();
        self.menu_pending = None;
        self.select_only = None;
        self.click_load = None;
        self.rename_path = None;
        self.name_draft.clear();
        self.target_draft.clear();
        self.import_draft.clear();
        self.import_open = false;
        self.eagle_open = false;
        self.copy_sources.clear();
        self.hardlinks.clear();
        self.skipped_hardlinks.clear();
        self.forget_hardlink_reads();
        self.tag_draft.clear();
        self.tags_open = None;
        if self.operation_kind.take().is_some() {
            self.operation.cancel();
        }
    }
}
