//! 文件变更：变更守卫，对话框的打开、草稿和提交，工具栏建文件，删除和回收站，拖放的移动和导入，
//! 以及变更结果（快照或协议完成）的归约。
//!
//! 对应 Vue `fileOperations.ts`、`CopyTargetDialog.vue` 和文件面板工具栏。同一时间只有一次变更
//! （`mutating`）；空白名称和空白来源直接返回，不会把状态标成进行中。变更在任务弹层里的进度行见
//! `files_operation.rs`，硬链接确认见 `files_hardlink.rs`。

use crate::backend::services::mutsuki_runner::{
    PROTOCOL_ARCHIVE_IMPORT, PROTOCOL_EAGLE_IMPORT, PROTOCOL_ENTRY_COPY, PROTOCOL_ENTRY_DELETE, PROTOCOL_ENTRY_MOVE,
};
use crate::backend::services::repository::FileBrowserSnapshot;

use super::super::repo_operation::OperationPatch;
use super::{normalize_path, parent_path, FileContext, FileDialog, FileOperation, FileRow, FilesEffect, FilesState};

impl FilesState {
    pub fn enqueue(&mut self, effect: FilesEffect) {
        self.mutating = true;
        self.effects.push(effect);
    }

    /// 拖放移动复用文件阶段的移动请求。空白来源不进入进行中。
    pub(crate) fn enqueue_move(&mut self, repo_id: String, sources: Vec<String>, parent: String) {
        if sources.is_empty() {
            return;
        }
        if repo_id.is_empty() {
            eprintln!("Nana 拖放移动缺少仓库");
            return;
        }
        self.begin(Some(FileOperation::step("移动文件", "移动到目标文件夹", 36.0, Some(82.0)).runs(&[PROTOCOL_ENTRY_MOVE])));
        self.effects.push(FilesEffect::Move { repo_id, sources, parent });
    }

    /// 拖放导入复用文件阶段的导入请求。空白来源不进入进行中。
    pub(crate) fn enqueue_import(&mut self, repo_id: String, parent: Option<String>, sources: Vec<String>) {
        if sources.is_empty() {
            return;
        }
        if repo_id.is_empty() {
            eprintln!("Nana 拖放导入缺少仓库");
            return;
        }
        self.begin(Some(FileOperation::import()));
        self.effects.push(FilesEffect::Import { repo_id, parent, sources });
    }

    /// 写下文件页错误。菜单和对话框失败都走这里。
    pub(crate) fn note_error(&mut self, error: String) {
        eprintln!("Nana 文件操作失败：{error}");
        self.error = error;
    }

    pub(in crate::shell) fn can_create(&self, ctx: &FileContext) -> bool {
        self.structure_allowed(ctx)
    }

    pub(in crate::shell) fn can_import(&self, ctx: &FileContext) -> bool {
        self.structure_allowed(ctx)
    }

    pub(in crate::shell) fn can_transfer(&self, ctx: &FileContext) -> bool {
        self.structure_allowed(ctx) && !self.selected.is_empty() && !self.selection_has_virtual()
    }

    pub(in crate::shell) fn can_rename(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some()
            && ctx.writable
            && !ctx.missing
            && !ctx.trash
            && !ctx.smart_folder
            && !self.mutating
            && self.selected.len() == 1
            && !self.selection_has_virtual()
    }

    pub(in crate::shell) fn can_delete(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some()
            && ctx.writable
            && !ctx.missing
            && !ctx.smart_folder
            && !self.mutating
            && !self.selected.is_empty()
            && !self.selection_has_virtual()
    }

    pub(in crate::shell) fn can_restore(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some()
            && ctx.writable
            && ctx.trash
            && !ctx.missing
            && !self.mutating
            && !self.selected.is_empty()
            && !self.selection_has_virtual()
    }

    pub(in crate::shell) fn can_empty_trash(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some() && ctx.writable && ctx.trash && !ctx.missing && !self.mutating
    }

    fn structure_allowed(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some()
            && ctx.writable
            && !ctx.missing
            && !ctx.trash
            && !ctx.smart_folder
            && !ctx.category_virtual
            && !self.mutating
    }

    pub(in crate::shell) fn open_dialog(&mut self, ctx: &FileContext, dialog: FileDialog) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能打开对话框");
            return false;
        }
        match dialog {
            FileDialog::Rename => {
                if !self.can_rename(ctx) {
                    eprintln!("Nana 当前选择不能重命名");
                    return false;
                }
                let path = self.primary.clone().unwrap_or_default();
                self.name_draft = self.row_name(&path);
                self.rename_path = Some(path);
            }
            FileDialog::Copy => {
                if !self.can_transfer(ctx) {
                    eprintln!("Nana 当前选择不能复制");
                    return false;
                }
                self.copy_sources = self.selected.clone();
                self.target_draft = self.current_path.clone();
            }
            FileDialog::Import | FileDialog::ImportArchive | FileDialog::ImportEagle => {
                if !self.can_import(ctx) {
                    eprintln!("Nana 当前视图不能导入");
                    return false;
                }
                self.import_draft.clear();
                self.import_open = false;
                self.eagle_open = false;
            }
            FileDialog::Closed | FileDialog::Hardlink => return false,
        }
        self.dialog = dialog;
        true
    }

    pub(in crate::shell) fn open_eagle(&mut self, ctx: &FileContext, mode: &str) -> bool {
        if mode != "copy" && mode != "move" {
            eprintln!("Nana Eagle 导入模式无效：{mode}");
            return false;
        }
        if !self.open_dialog(ctx, FileDialog::ImportEagle) {
            return false;
        }
        self.eagle_mode = mode.to_string();
        true
    }

    pub(in crate::shell) fn close_dialog(&mut self) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能关闭对话框");
            return false;
        }
        self.dialog = FileDialog::Closed;
        self.name_draft.clear();
        self.rename_path = None;
        true
    }

    /// 验收页打开复制对话框。不改首页入口。
    pub(in crate::shell) fn present_copy(&mut self, target: &str) {
        self.export.open = false;
        self.dialog = FileDialog::Copy;
        self.target_draft = target.to_string();
    }

    /// 展开或收起导入菜单。收起时 Eagle 的复制和剪切一起收起。
    pub(in crate::shell) fn toggle_import_menu(&mut self) {
        self.import_open = !self.import_open;
        if !self.import_open {
            self.eagle_open = false;
        }
    }

    /// 「从 Eagle 导入」下面的复制和剪切。菜单本身保持打开。
    pub(in crate::shell) fn toggle_eagle_menu(&mut self) {
        self.import_open = true;
        self.eagle_open = !self.eagle_open;
    }

    pub(in crate::shell) fn set_draft(&mut self, value: String) {
        match self.dialog {
            FileDialog::Rename => self.name_draft = value,
            FileDialog::Copy => self.target_draft = value,
            FileDialog::Import | FileDialog::ImportArchive | FileDialog::ImportEagle => self.import_draft = value,
            FileDialog::Closed | FileDialog::Hardlink => {}
        }
    }

    pub(in crate::shell) fn submit_dialog(&mut self, ctx: &FileContext) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能重复提交");
            return false;
        }
        match self.dialog {
            FileDialog::Rename => self.submit_rename(ctx),
            FileDialog::Copy => self.submit_copy(ctx),
            FileDialog::Import => self.submit_import(ctx),
            FileDialog::ImportArchive => self.submit_archive(ctx),
            FileDialog::ImportEagle => self.submit_eagle(ctx),
            FileDialog::Hardlink => self.confirm_hardlink(ctx),
            FileDialog::Closed => false,
        }
    }

    fn submit_rename(&mut self, ctx: &FileContext) -> bool {
        let name = self.name_draft.trim().to_string();
        if name.is_empty() {
            return false;
        }
        if !self.can_rename(ctx) {
            eprintln!("Nana 当前选择不能重命名");
            return false;
        }
        let Some(path) = self.rename_path.clone() else {
            return false;
        };
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        self.begin(None);
        self.effects.push(FilesEffect::Rename { repo_id, path, new_name: name });
        true
    }

    fn submit_copy(&mut self, ctx: &FileContext) -> bool {
        if self.copy_sources.is_empty() {
            return false;
        }
        if !self.can_transfer(ctx) {
            eprintln!("Nana 当前选择不能复制");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&normalize_path(&self.target_draft));
        let sources = self.copy_sources.clone();
        self.begin(Some(FileOperation::step("复制文件", "创建硬链接或复制文件", 32.0, Some(84.0)).runs(&[PROTOCOL_ENTRY_COPY])));
        self.effects.push(FilesEffect::Copy { repo_id, sources, parent });
        true
    }

    fn submit_import(&mut self, ctx: &FileContext) -> bool {
        let sources = split_sources(&self.import_draft);
        if sources.is_empty() {
            return false;
        }
        if !self.can_import(ctx) {
            eprintln!("Nana 当前视图不能导入");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        self.begin(Some(FileOperation::import()));
        self.effects.push(FilesEffect::Import { repo_id, parent, sources });
        true
    }

    fn submit_archive(&mut self, ctx: &FileContext) -> bool {
        let archive_path = self.import_draft.trim().to_string();
        if archive_path.is_empty() {
            return false;
        }
        if !self.can_import(ctx) {
            eprintln!("Nana 当前视图不能导入压缩包");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        self.begin(Some(FileOperation::step("导入 ZIP", "预检压缩包条目", 24.0, Some(84.0)).runs(&[PROTOCOL_ARCHIVE_IMPORT])));
        self.effects.push(FilesEffect::ImportArchive { repo_id, parent, archive_path });
        true
    }

    fn submit_eagle(&mut self, ctx: &FileContext) -> bool {
        let library_path = self.import_draft.trim().to_string();
        if library_path.is_empty() {
            return false;
        }
        if self.eagle_mode != "copy" && self.eagle_mode != "move" {
            eprintln!("Nana Eagle 导入模式无效：{}", self.eagle_mode);
            return false;
        }
        if !self.can_import(ctx) {
            eprintln!("Nana 当前视图不能导入 Eagle");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        let mode = self.eagle_mode.clone();
        let label = if mode == "move" { "剪切导入 Eagle" } else { "复制导入 Eagle" };
        self.begin(Some(FileOperation::step(label, "转换 EagleLibrary", 24.0, Some(84.0)).runs(&[PROTOCOL_EAGLE_IMPORT])));
        self.effects.push(FilesEffect::ImportEagle { repo_id, parent, library_path, mode });
        true
    }

    /// 用工具栏里的文件名在当前目录建空文件（Vue `handleCreateFile`）。回收站和虚拟视图不建；
    /// 输入框等结果回来、建成了才清空，失败时名字留着。
    pub(in crate::shell) fn submit_inline_file(&mut self, ctx: &FileContext) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能新建文件");
            return false;
        }
        let name = self.create_name.trim().to_string();
        if name.is_empty() {
            eprintln!("Nana 新建文件名是空的");
            return false;
        }
        if !self.can_create(ctx) {
            eprintln!("Nana 当前视图不能新建");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        self.begin(None);
        self.effects.push(FilesEffect::CreateFile { repo_id, parent, name });
        true
    }

    pub(in crate::shell) fn delete_selected(&mut self, ctx: &FileContext) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        if !self.can_delete(ctx) {
            eprintln!("Nana 当前选择不能删除");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let paths = self.selected.clone();
        let mode = if ctx.trash { Some("permanentDelete".to_string()) } else { None };
        // Vue 只有多选删除走 `deleteWorkspaceEntries` 出进度行，单条删除不出。
        let operation = (paths.len() > 1)
            .then(|| FileOperation::step("删除文件", format!("准备处理 {} 个条目", paths.len()), 10.0, None).runs(&[PROTOCOL_ENTRY_DELETE]));
        self.begin(operation);
        self.effects.push(FilesEffect::Delete { repo_id, paths, mode });
        true
    }

    pub(in crate::shell) fn restore_selected(&mut self, ctx: &FileContext) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        // 和删除一样，Vue 只有多选还原（`restoreTrashEntries`）出进度行。
        let count = self.selected.len();
        let operation = (count > 1).then(|| FileOperation::step("还原文件", format!("准备还原 {count} 个条目"), 10.0, None));
        self.mutate_trash(ctx, "restore", self.selected.clone(), operation)
    }

    /// 回收站的还原、全部还原和清空。`operation` 是任务弹层里的进度行，Vue 不出的传 `None`。
    pub(in crate::shell) fn mutate_trash(&mut self, ctx: &FileContext, action: &str, paths: Vec<String>, operation: Option<FileOperation>) -> bool {
        let allowed = if action == "restore" { self.can_restore(ctx) } else { self.can_empty_trash(ctx) };
        if !allowed {
            eprintln!("Nana 当前视图不能执行回收站操作：{action}");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        self.begin(operation);
        self.effects.push(FilesEffect::MutateTrash { repo_id, action: action.to_string(), paths });
        true
    }

    /// 变更成功的快照。`created_name` 只有工具栏建文件带：建成了清空输入框，选择里有东西时选中新文件。
    pub(in crate::shell) fn apply_mutation_snapshot(&mut self, ctx: &FileContext, snapshot: FileBrowserSnapshot, created_name: Option<String>) {
        self.mutating = false;
        self.settle_mutation(true);
        if created_name.is_some() {
            self.create_name.clear();
        }
        let can_apply = !ctx.category_virtual || normalize_path(&self.current_path) == normalize_path(&snapshot.current_path);
        if !can_apply {
            eprintln!(
                "Nana 分类视图快照路径不一致，保留当前列表：{} / {}",
                self.current_path, snapshot.current_path
            );
            self.finish_dialog_success();
            return;
        }
        let had_selection = !self.selected.is_empty();
        let typed = created_name.unwrap_or_default();
        let mut incoming: Vec<FileRow> = snapshot.entries.iter().map(FileRow::from_entry).collect();
        self.carry_thumbnails(&mut incoming);
        self.rows = incoming;
        self.prune_decode_requests();
        self.current_path = normalize_path(&snapshot.current_path);
        self.total_entries = snapshot.total_entries;
        self.trash = snapshot.special_location.as_deref() == Some("trash");
        self.has_more = !ctx.is_virtual() && self.rows.len() < snapshot.total_entries;
        self.error.clear();
        self.finish_loading();
        let typed = typed.trim().to_string();
        if had_selection && !typed.is_empty() {
            if let Some(row) = self.rows.iter().find(|row| row.name == typed) {
                let path = row.path.clone();
                self.selected = vec![path.clone()];
                self.primary = Some(path.clone());
                self.anchor = Some(path);
                self.finish_dialog_success();
                return;
            }
        }
        let rows = self.rows.clone();
        self.prune_against(&rows);
        self.finish_dialog_success();
    }

    pub(in crate::shell) fn note_mutation_failed(&mut self, error: String) {
        eprintln!("Nana 文件变更失败：{error}");
        self.mutating = false;
        self.settle_mutation(false);
        self.error = error;
    }

    /// 协议变更的结果。成功且要重读目录时：复制和导入的进度行走到 Vue「刷新文件索引」那一步，目录读回来
    /// 才完成；多选删除和移动在 Vue 里重读目录是一次非静默读取，进度行换成「读取目录」。
    pub(in crate::shell) fn note_protocol_finished(&mut self, ctx: &FileContext, result: Result<(), String>, reload: bool, hardlinks: bool) {
        self.mutating = false;
        match result {
            Ok(()) => {
                self.finish_dialog_success();
                let path = self.current_path.clone();
                let (refresh, read) = self.reload_progress();
                if reload && self.queue_browse(ctx, &path, true, true, read) {
                    if let Some(value) = refresh {
                        self.operation.update(OperationPatch::step("刷新文件索引", value));
                    }
                } else {
                    self.settle_mutation(true);
                }
                if hardlinks {
                    if let Some(repo_id) = ctx.repo_id.clone() {
                        self.effects.push(FilesEffect::LoadHardlinks { repo_id, epoch: self.hardlink_epoch() });
                    }
                }
            }
            Err(error) => {
                eprintln!("Nana 文件变更失败：{error}");
                self.error = error;
                self.settle_mutation(false);
            }
        }
    }

    /// 开始一次文件变更：标进行中、清掉上一次的错误。`operation` 是这次变更在任务弹层里的进度行，
    /// Vue 不出进度的变更（建文件、重命名、单条删除和还原、全部还原、清空回收站、确认硬链接）传 `None`。
    pub(super) fn begin(&mut self, operation: Option<FileOperation>) {
        self.mutating = true;
        self.error.clear();
        if let Some(operation) = operation {
            self.report_mutation(operation);
        }
    }

    fn finish_dialog_success(&mut self) {
        self.dialog = FileDialog::Closed;
        self.name_draft.clear();
        self.rename_path = None;
        self.target_draft.clear();
        self.import_draft.clear();
        self.copy_sources.clear();
    }

    fn row_name(&self, path: &str) -> String {
        self.rows
            .iter()
            .chain(self.virtual_rows.iter())
            .find(|row| row.path == path)
            .map(|row| row.name.clone())
            .unwrap_or_default()
    }
}

/// 导入对话框里的来源：按换行和分号分开，去掉空白项。
fn split_sources(raw: &str) -> Vec<String> {
    raw.split(['\n', '\r', ';'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}
