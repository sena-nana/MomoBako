//! 文件变更：变更守卫，对话框的打开、草稿和提交，工具栏建文件，删除和回收站，拖放的移动和导入，
//! 以及变更结果（快照或协议完成）的归约。
//!
//! 对应 Vue `fileOperations.ts`、`CopyTargetDialog.vue` 和文件面板工具栏。同一时间只有一次变更
//! （`mutating`）；空白名称和空白来源直接返回，不会把状态标成进行中。变更在任务弹层里的进度行见
//! `files_operation.rs`，硬链接确认见 `files_hardlink.rs`。

use crate::backend::services::mutsuki_runner::{
    PROTOCOL_ARCHIVE_IMPORT, PROTOCOL_EAGLE_IMPORT, PROTOCOL_ENTRY_COPY, PROTOCOL_ENTRY_DELETE,
    PROTOCOL_ENTRY_MOVE,
};
use crate::backend::services::repository::FileBrowserSnapshot;

use super::super::repo_operation::OperationPatch;
use super::{
    FileContext, FileDialog, FileOperation, FileRow, FilesEffect, FilesState, ProtocolFolderTree,
    ReadProgress, normalize_path, parent_path,
};

impl FilesState {
    pub fn enqueue(&mut self, effect: FilesEffect) {
        if let FilesEffect::Rename { new_name, .. } = &effect {
            self.note_rename_name(new_name);
        }
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
        // 成功以后按 Vue `moveWorkspaceEntries` 改选。失败的结果会把这份丢掉。
        self.move_reselect = Some((sources.clone(), parent.clone()));
        self.begin(Some(
            FileOperation::step("移动文件", "移动到目标文件夹", 36.0, Some(82.0))
                .runs(&[PROTOCOL_ENTRY_MOVE]),
        ));
        self.effects.push(FilesEffect::Move {
            repo_id,
            sources,
            parent,
        });
    }

    /// 拖放导入复用文件阶段的导入请求。空白来源不进入进行中。
    pub(crate) fn enqueue_import(
        &mut self,
        repo_id: String,
        parent: Option<String>,
        sources: Vec<String>,
    ) {
        if sources.is_empty() {
            return;
        }
        if repo_id.is_empty() {
            eprintln!("Nana 拖放导入缺少仓库");
            return;
        }
        self.arm_named_import(&sources);
        self.begin(Some(FileOperation::import()));
        self.effects.push(FilesEffect::Import {
            repo_id,
            parent,
            sources,
        });
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

    /// 文件菜单的复制或剪切。不打开路径输入框，只记下模式，壳层去排队和「选择目录」同一套文件夹对话框。
    pub(in crate::shell) fn open_eagle(&mut self, ctx: &FileContext, mode: &str) -> bool {
        if mode != "copy" && mode != "move" {
            eprintln!("Nana Eagle 导入模式无效：{mode}");
            return false;
        }
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能导入 Eagle");
            return false;
        }
        if !self.can_import(ctx) {
            eprintln!("Nana 当前视图不能导入 Eagle");
            return false;
        }
        self.import_open = false;
        self.eagle_open = false;
        self.eagle_menu_mode = Some(mode.to_string());
        true
    }

    /// 取走文件菜单刚记下的导入模式。没有菜单动作时是 `None`。
    pub(in crate::shell) fn take_eagle_menu_mode(&mut self) -> Option<String> {
        self.eagle_menu_mode.take()
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
            FileDialog::Import | FileDialog::ImportArchive | FileDialog::ImportEagle => {
                self.import_draft = value
            }
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
        self.note_rename_name(&name);
        self.effects.push(FilesEffect::Rename {
            repo_id,
            path,
            new_name: name,
        });
        true
    }

    /// 提交复制。父目录和来源在这一刻从草稿里取出，之后改目标目录不会改已经发出的请求。
    fn submit_copy(&mut self, ctx: &FileContext) -> bool {
        if self.copy_sources.is_empty() {
            eprintln!("Nana 复制没有来源，不提交");
            return false;
        }
        if !self.can_transfer(ctx) {
            eprintln!("Nana 当前选择不能复制");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&normalize_path(&self.target_draft));
        let sources = self.copy_sources.clone();
        // 成功以后按 Vue `finishFileTransfer` 改选。失败的结果会把这份丢掉。
        self.copy_reselect = Some((sources.clone(), parent.clone().unwrap_or_default()));
        self.begin(Some(
            FileOperation::step("复制文件", "创建硬链接或复制文件", 32.0, Some(84.0))
                .runs(&[PROTOCOL_ENTRY_COPY]),
        ));
        self.effects.push(FilesEffect::Copy {
            repo_id,
            sources,
            parent,
        });
        true
    }

    fn submit_import(&mut self, ctx: &FileContext) -> bool {
        self.import_picked_sources(ctx, split_sources(&self.import_draft))
    }

    /// 用系统选择器交回的文件夹导入。空白不提交，也不把对话框标成已经成功。
    pub(in crate::shell) fn import_picked_sources(
        &mut self,
        ctx: &FileContext,
        sources: Vec<String>,
    ) -> bool {
        let sources = sources
            .into_iter()
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        if sources.is_empty() {
            eprintln!("Nana 导入路径是空的");
            return false;
        }
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能导入");
            return false;
        }
        if !self.can_import(ctx) {
            eprintln!("Nana 当前视图不能导入");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        self.arm_named_import(&sources);
        self.begin(Some(FileOperation::import()));
        self.effects.push(FilesEffect::Import {
            repo_id,
            parent,
            sources,
        });
        true
    }

    fn submit_archive(&mut self, ctx: &FileContext) -> bool {
        let archive_path = self.import_draft.trim().to_string();
        self.import_picked_archive(ctx, &archive_path)
    }

    /// 用系统选择器交回的压缩包路径导入。空白不提交。
    pub(in crate::shell) fn import_picked_archive(
        &mut self,
        ctx: &FileContext,
        archive_path: &str,
    ) -> bool {
        let archive_path = archive_path.trim().to_string();
        if archive_path.is_empty() {
            eprintln!("Nana 压缩包路径是空的");
            return false;
        }
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能导入压缩包");
            return false;
        }
        if !self.can_import(ctx) {
            eprintln!("Nana 当前视图不能导入压缩包");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        self.arm_new_children_import(&parent);
        self.begin(Some(
            FileOperation::step("导入 ZIP", "预检压缩包条目", 24.0, Some(84.0))
                .runs(&[PROTOCOL_ARCHIVE_IMPORT]),
        ));
        self.effects.push(FilesEffect::ImportArchive {
            repo_id,
            parent,
            archive_path,
        });
        true
    }

    fn submit_eagle(&mut self, ctx: &FileContext) -> bool {
        let library_path = self.import_draft.trim().to_string();
        if library_path.is_empty() {
            return false;
        }
        let mode = self.eagle_mode.clone();
        self.import_eagle_library(ctx, &mode, &library_path)
    }

    /// 用已经选好的 EagleLibrary 目录提交导入。走现有的 `PROTOCOL_EAGLE_IMPORT`，
    /// 不打开路径对话框，也不记下导入条数。路径或模式无效、当前不能导入时只记日志。
    pub(in crate::shell) fn import_eagle_library(
        &mut self,
        ctx: &FileContext,
        mode: &str,
        library_path: &str,
    ) -> bool {
        let library_path = library_path.trim();
        if library_path.is_empty() {
            eprintln!("Nana 请先选择 EagleLibrary 目录");
            return false;
        }
        if mode != "copy" && mode != "move" {
            eprintln!("Nana Eagle 导入模式无效：{mode}");
            return false;
        }
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能导入 Eagle");
            return false;
        }
        if !self.can_import(ctx) {
            eprintln!("Nana 当前视图不能导入 Eagle");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        let mode = mode.to_string();
        let label = if mode == "move" {
            "剪切导入 Eagle"
        } else {
            "复制导入 Eagle"
        };
        self.arm_new_children_import(&parent);
        self.begin(Some(
            FileOperation::step(label, "转换 EagleLibrary", 24.0, Some(84.0))
                .runs(&[PROTOCOL_EAGLE_IMPORT]),
        ));
        self.effects.push(FilesEffect::ImportEagle {
            repo_id,
            parent,
            library_path: library_path.to_string(),
            mode,
        });
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
        self.effects.push(FilesEffect::CreateFile {
            repo_id,
            parent,
            name,
        });
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
        let mode = if ctx.trash {
            Some("permanentDelete".to_string())
        } else {
            None
        };
        // Vue 只有多选删除走 `deleteWorkspaceEntries` 出进度行，单条删除不出。
        let operation = (paths.len() > 1).then(|| {
            FileOperation::step(
                "删除文件",
                format!("准备处理 {} 个条目", paths.len()),
                10.0,
                None,
            )
            .runs(&[PROTOCOL_ENTRY_DELETE])
        });
        self.begin(operation);
        // 单条删除走 Vue `deleteWorkspaceEntry`，不另读侧栏树。多项才可能是 `currentWithTree`。
        if paths.len() > 1 {
            self.multi_delete_paths = Some(paths.clone());
        }
        self.effects.push(FilesEffect::Delete {
            repo_id,
            paths,
            mode,
        });
        true
    }

    pub(in crate::shell) fn restore_selected(&mut self, ctx: &FileContext) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        // 和删除一样，Vue 只有多选还原（`restoreTrashEntries`）出进度行。
        // 多选成功后不采用变更快照，改去非静默重读回收站；单条还原仍采用快照。
        let count = self.selected.len();
        let operation = (count > 1).then(|| {
            FileOperation::step("还原文件", format!("准备还原 {count} 个条目"), 10.0, None)
        });
        let started = self.mutate_trash(ctx, "restore", self.selected.clone(), operation);
        if started {
            self.reload_trash_on_success = count > 1;
        }
        started
    }

    /// 回收站的还原、全部还原和清空。`operation` 是任务弹层里的进度行，Vue 不出的传 `None`。
    pub(in crate::shell) fn mutate_trash(
        &mut self,
        ctx: &FileContext,
        action: &str,
        paths: Vec<String>,
        operation: Option<FileOperation>,
    ) -> bool {
        let allowed = if action == "restore" {
            self.can_restore(ctx)
        } else {
            self.can_empty_trash(ctx)
        };
        if !allowed {
            eprintln!("Nana 当前视图不能执行回收站操作：{action}");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        self.begin(operation);
        self.effects.push(FilesEffect::MutateTrash {
            repo_id,
            action: action.to_string(),
            paths,
        });
        true
    }

    /// 变更成功的快照。`created_name` 只有工具栏建文件带：建成了清空输入框，选择里有东西时选中新文件。
    ///
    /// 多选还原不走这里的采用：Vue `restoreTrashEntries` 丢掉逐条 `mutateTrash` 的快照，清空选择后再非静默重读回收站。
    pub(in crate::shell) fn apply_mutation_snapshot(
        &mut self,
        ctx: &FileContext,
        snapshot: FileBrowserSnapshot,
        created_name: Option<String>,
    ) {
        let rename_name = self.rename_select.take();
        if self.reload_trash_on_success {
            self.hand_off_multi_restore(ctx);
            return;
        }
        self.mutating = false;
        self.settle_mutation(true);
        if created_name.is_some() {
            self.create_name.clear();
        }
        let can_apply = !ctx.category_virtual
            || normalize_path(&self.current_path) == normalize_path(&snapshot.current_path);
        if !can_apply {
            eprintln!(
                "Nana 分类视图快照路径不一致，保留当前列表：{} / {}",
                self.current_path, snapshot.current_path
            );
            // 列表不换，选择仍照 Vue 换成快照里的新名字。没有这个名字就留着原来的选择。
            if let Some(name) = rename_name.as_deref() {
                let incoming: Vec<FileRow> =
                    snapshot.entries.iter().map(FileRow::from_entry).collect();
                self.adopt_renamed(&incoming, name);
            }
            self.finish_dialog_success();
            return;
        }
        // 分类视图路径对不上、多选还原丢掉的快照都不进这里，侧栏树不动。
        self.note_adopted_tree(ctx, &snapshot);
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
        if let Some(name) = rename_name.as_deref() {
            let listed = self.rows.clone();
            if self.adopt_renamed(&listed, name) {
                self.finish_dialog_success();
                return;
            }
        }
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
        self.rename_select = None;
        self.reload_trash_on_success = false;
        self.mutating = false;
        self.settle_mutation(false);
        self.error = error;
    }

    /// 多选还原成功：丢掉变更快照，清空选择，非静默重读回收站。
    ///
    /// 任务弹层从「还原文件」换成「读取回收站」，文件列从「正在处理文件」换成「正在读取目录」。
    /// 单条还原、全部还原和清空仍采用快照，不进这里。重读排不下去时记日志，并收起还原的进度行。
    fn hand_off_multi_restore(&mut self, ctx: &FileContext) {
        self.reload_trash_on_success = false;
        self.mutating = false;
        let path = self.current_path.clone();
        if !ctx.trash {
            eprintln!("Nana 多选还原完成时已经不在回收站，不重读回收站");
            self.selected.clear();
            self.primary = None;
            self.anchor = None;
            self.settle_mutation(true);
            return;
        }
        if self.queue_browse(ctx, &path, false, true, ReadProgress::Directory) {
            return;
        }
        eprintln!("Nana 多选还原以后没能重读回收站：{path}");
        self.selected.clear();
        self.primary = None;
        self.anchor = None;
        self.settle_mutation(true);
    }

    /// 复制、移动、删除的协议结果带着文件夹树时，只把树交给侧栏。
    ///
    /// Vue `applyFileBrowserSnapshot` 在 `snapshot.tree` 有值时写 `fileTree`。复制的
    /// `finishFileTransfer`、移进当前目录的 `moveWorkspaceEntries`、单条 `deleteWorkspaceEntry`
    /// 都会调用它。移走和多选删除的协议 JSON 同样可能带树；这里也不改列表，选择仍走
    /// `note_protocol_finished`。选中里有可见文件夹时，这两处成功后另外再读一次侧栏树。
    /// 没有树不记。分类视图里快照目录和当前目录不一致、仓库不一致、
    /// 在途读取和这份快照对不上时不写。
    pub(in crate::shell) fn note_protocol_folder_tree(
        &mut self,
        ctx: &FileContext,
        folder_tree: Option<ProtocolFolderTree>,
    ) {
        let Some(tree) = folder_tree else {
            return;
        };
        // 结果回来时已经有别的目录读取在途，而且仓库、路径或回收站对不上：这份树过期。
        // 还没有在途读取时不挡。移走文件夹的快照目录是目标目录，选择仍留在当前目录。
        if let Some(pending) = &self.pending {
            let same = pending.repo_id == tree.repo_id
                && pending.path == normalize_path(&tree.current_path)
                && pending.trash == tree.trash;
            if !same {
                eprintln!(
                    "Nana 忽略过期的协议文件夹树：{} / {}",
                    tree.repo_id, tree.current_path
                );
                return;
            }
        }
        if ctx.repo_id.as_deref() != Some(tree.repo_id.as_str()) {
            eprintln!(
                "Nana 协议结果的文件夹树来自别的仓库，不写入侧栏：{}",
                tree.repo_id
            );
            return;
        }
        if ctx.category_virtual
            && normalize_path(&self.current_path) != normalize_path(&tree.current_path)
        {
            eprintln!(
                "Nana 分类视图协议结果路径不一致，不写入侧栏：{} / {}",
                self.current_path, tree.current_path
            );
            return;
        }
        self.adopted_tree = Some((tree.repo_id, tree.nodes));
    }

    /// 协议变更的结果。成功且要重读目录时：复制和导入的进度行走到 Vue「刷新文件索引」那一步，目录读回来
    /// 才完成；多选删除和移动在 Vue 里重读目录是一次非静默读取，进度行换成「读取目录」。
    /// 复制成功改读复制进去的目录，快照对上以后按源文件名改选。文件夹树在这之前已经记下，这里不改选择。
    pub(in crate::shell) fn note_protocol_finished(
        &mut self,
        ctx: &FileContext,
        result: Result<(), String>,
        reload: bool,
        hardlinks: bool,
    ) {
        self.mutating = false;
        // 另读与否要在丢掉移动来源和多选删除路径之前看：回收站不读；移走且来源里有可见文件夹，
        // 或多选删除且来源里有可见文件夹，才对应 Vue `currentWithTree` → `refreshRepositoryTree`。
        let reread_tree = result.is_ok() && self.folder_tree_reread_due(ctx);
        // 移动成功先改选（移进当前目录的那一支写进导入用的改选槽）。失败丢掉，选择不动。
        // 复制成功记下源文件名，并把这次重读指到目标目录。失败两份都丢掉，选择不动。
        let move_reselect = self.move_reselect.take();
        self.multi_delete_paths = None;
        let copy_reselect = self.copy_reselect.take();
        let mut browse_path = self.current_path.clone();
        if result.is_ok() {
            if let Some((sources, parent)) = move_reselect {
                self.prepare_move_selection(&sources, &parent);
            }
            if let Some((sources, parent)) = copy_reselect {
                let parent = normalize_path(&parent);
                if ctx.category_virtual && parent != normalize_path(&self.current_path) {
                    eprintln!("Nana 分类视图不采用别的目录的复制结果，不改选");
                } else {
                    self.prepare_copy_selection(&sources);
                    browse_path = parent;
                }
            }
        }
        // 导入，以及移进当前目录的移动，记下改选。成功且这次重读排得下时交给那次读取；失败或排不下就丢掉。
        let followup = match &result {
            Ok(()) => self.import_followup.take(),
            Err(_) => {
                self.import_followup = None;
                None
            }
        };
        match result {
            Ok(()) => {
                self.finish_dialog_success();
                let (refresh, read) = self.reload_progress();
                // 复制成功后浏览目标目录；挂起快照携带目标路径，等目标快照返回后再更新当前目录。
                // 这样协议文件夹树只更新侧栏，不会把当前目录提前切走。
                if reload && self.queue_browse(ctx, &browse_path, true, true, read) {
                    if let Some(pending) = self.pending.as_mut() {
                        pending.select_after = followup;
                    }
                    if let Some(value) = refresh {
                        self.operation
                            .update(OperationPatch::step("刷新文件索引", value));
                    }
                } else {
                    if followup.is_some() {
                        eprintln!("Nana 导入后的目录没有重读，不改选");
                    }
                    self.settle_mutation(true);
                }
                if hardlinks {
                    if let Some(repo_id) = ctx.repo_id.clone() {
                        self.effects.push(FilesEffect::LoadHardlinks {
                            repo_id,
                            epoch: self.hardlink_epoch(),
                        });
                    }
                }
                if reread_tree {
                    self.arm_folder_tree_reread(ctx);
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
        // 只有多选还原在 `restore_selected` 里再打开。别的变更成功后仍采用快照。
        self.reload_trash_on_success = false;
        self.rename_select = None;
        self.multi_delete_paths = None;
        self.folder_tree_reread = None;
        if let Some(operation) = operation {
            self.report_mutation(operation);
        }
    }

    /// 这次成功要不要另读侧栏树。
    ///
    /// Vue `defaultDirectoryRefreshPlan`：回收站是 `trash`，不调用 `refreshRepositoryTree`。
    /// 其余只有移到别的目录、或多选删除，并且当前可见条目里有文件夹时，才是 `currentWithTree`。
    /// 移进正在看的目录、单条删除、复制都不走这里。文件夹要在当前画出来的列表里，不在列表里的路径不算。
    fn folder_tree_reread_due(&self, ctx: &FileContext) -> bool {
        if ctx.trash {
            return false;
        }
        if let Some((sources, parent)) = &self.move_reselect {
            if normalize_path(parent) != normalize_path(&self.current_path)
                && self.paths_include_visible_directory(ctx, sources)
            {
                return true;
            }
        }
        self.multi_delete_paths.as_ref().is_some_and(|paths| {
            paths.len() > 1 && self.paths_include_visible_directory(ctx, paths)
        })
    }

    /// 这些路径里有没有当前列表上的文件夹（Vue `visibleEntries` 里 `kind === "directory"`）。
    fn paths_include_visible_directory(&self, ctx: &FileContext, paths: &[String]) -> bool {
        self.listed_rows(ctx)
            .into_iter()
            .any(|row| row.kind == "directory" && paths.iter().any(|path| path == &row.path))
    }

    /// 记下要另读的仓库。没有仓库时只记日志，不编一棵树。
    fn arm_folder_tree_reread(&mut self, ctx: &FileContext) {
        match ctx.repo_id.clone().filter(|repo_id| !repo_id.is_empty()) {
            Some(repo_id) => {
                eprintln!("Nana 选中里有文件夹，移走或多选删除另读侧栏树");
                self.folder_tree_reread = Some(repo_id);
            }
            None => eprintln!("Nana 移走或删除文件夹以后没有仓库，不另读侧栏树"),
        }
    }

    /// 快照带着文件夹树时记下，交给壳层写入侧栏（Vue `applyFileBrowserSnapshot`）。
    ///
    /// 没有 `tree` 时不记，侧栏保持原样。快照仓库和当前仓库不一致时只记日志，不写。
    fn note_adopted_tree(
        &mut self,
        ctx: &FileContext,
        snapshot: &crate::backend::services::repository::FileBrowserSnapshot,
    ) {
        let Some(tree) = snapshot.tree.clone() else {
            return;
        };
        if ctx.repo_id.as_deref() != Some(snapshot.repo_id.as_str()) {
            eprintln!(
                "Nana 目录快照的文件夹树来自别的仓库，不写入侧栏：{}",
                snapshot.repo_id
            );
            return;
        }
        self.adopted_tree = Some((snapshot.repo_id.clone(), tree));
    }

    /// 记下这次重命名要选中的新名字。空白只记日志，成功后不改选。
    fn note_rename_name(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() {
            eprintln!("Nana 重命名名字是空的，成功后不改选");
            self.rename_select = None;
            return;
        }
        self.rename_select = Some(name.to_string());
    }

    /// 在这份条目里按新名字选中。找不到时只记日志，返回 false，调用方再按原列表修剪。
    fn adopt_renamed(&mut self, rows: &[FileRow], name: &str) -> bool {
        let Some(path) = renamed_entry_path(rows, name) else {
            eprintln!("Nana 重命名结果里没有「{name}」，不改选");
            return false;
        };
        self.selected = vec![path.clone()];
        self.primary = Some(path.clone());
        self.anchor = Some(path);
        true
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

/// 快照条目里名字正好是 `name` 的路径。空白名字不算。同名时用第一条，和 Vue `find` 一样。
fn renamed_entry_path(rows: &[FileRow], name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    rows.iter()
        .find(|row| row.name == name)
        .map(|row| row.path.clone())
}

/// 导入对话框里的来源：按换行和分号分开，去掉空白项。
fn split_sources(raw: &str) -> Vec<String> {
    raw.split(['\n', '\r', ';'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}
