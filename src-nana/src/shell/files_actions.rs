// 文件变更对话框和变更结果。
// 新建、重命名、复制、移动、导入、删除和硬链接确认都从这里提交。
// 空白名称和空白来源直接返回，不会把状态标成进行中。

impl FilesState {
    pub(super) fn open_dialog(&mut self, ctx: &FileContext, dialog: FileDialog) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能打开对话框");
            return false;
        }
        match dialog {
            FileDialog::CreateDirectory | FileDialog::CreateFile => {
                if !self.can_create(ctx) {
                    eprintln!("Nana 当前视图不能新建");
                    return false;
                }
                self.name_draft.clear();
            }
            FileDialog::Rename => {
                if !self.can_rename(ctx) {
                    eprintln!("Nana 当前选择不能重命名");
                    return false;
                }
                let path = self.primary.clone().unwrap_or_default();
                self.name_draft = self.row_name(&path);
                self.rename_path = Some(path);
            }
            FileDialog::Copy | FileDialog::Move => {
                if !self.can_transfer(ctx) {
                    eprintln!("Nana 当前选择不能转移");
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
            }
            FileDialog::Closed | FileDialog::Hardlink => return false,
        }
        self.dialog = dialog;
        true
    }

    pub(super) fn open_eagle(&mut self, ctx: &FileContext, mode: &str) -> bool {
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

    pub(super) fn close_dialog(&mut self) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能关闭对话框");
            return false;
        }
        self.dialog = FileDialog::Closed;
        self.name_draft.clear();
        self.rename_path = None;
        true
    }

    pub(super) fn set_draft(&mut self, value: String) {
        match self.dialog {
            FileDialog::CreateDirectory | FileDialog::CreateFile | FileDialog::Rename => self.name_draft = value,
            FileDialog::Copy | FileDialog::Move => self.target_draft = value,
            FileDialog::Import | FileDialog::ImportArchive | FileDialog::ImportEagle => self.import_draft = value,
            FileDialog::Closed | FileDialog::Hardlink => {}
        }
    }

    pub(super) fn submit_dialog(&mut self, ctx: &FileContext) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能重复提交");
            return false;
        }
        match self.dialog {
            FileDialog::CreateDirectory => self.submit_named(ctx, true),
            FileDialog::CreateFile => self.submit_named(ctx, false),
            FileDialog::Rename => self.submit_rename(ctx),
            FileDialog::Copy => self.submit_copy(ctx),
            FileDialog::Move => self.submit_move(ctx),
            FileDialog::Import => self.submit_import(ctx),
            FileDialog::ImportArchive => self.submit_archive(ctx),
            FileDialog::ImportEagle => self.submit_eagle(ctx),
            FileDialog::Hardlink => self.confirm_hardlink(ctx),
            FileDialog::Closed => false,
        }
    }

    fn submit_named(&mut self, ctx: &FileContext, directory: bool) -> bool {
        let name = self.name_draft.trim().to_string();
        if name.is_empty() {
            return false;
        }
        if !self.can_create(ctx) {
            eprintln!("Nana 当前视图不能新建");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = parent_path(&self.current_path);
        self.begin(if directory { "正在新建文件夹…" } else { "正在新建文件…" });
        self.effects.push(if directory {
            FilesEffect::CreateDirectory { repo_id, parent, name }
        } else {
            FilesEffect::CreateFile { repo_id, parent, name }
        });
        true
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
        self.begin("正在重命名…");
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
        self.begin("正在复制…");
        self.effects.push(FilesEffect::Copy { repo_id, sources, parent });
        true
    }

    fn submit_move(&mut self, ctx: &FileContext) -> bool {
        if self.copy_sources.is_empty() {
            return false;
        }
        if !self.can_transfer(ctx) {
            eprintln!("Nana 当前选择不能移动");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let parent = normalize_path(&self.target_draft);
        let sources = self.copy_sources.clone();
        self.begin("正在移动…");
        self.effects.push(FilesEffect::Move { repo_id, sources, parent });
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
        self.begin("正在导入…");
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
        self.begin("正在导入压缩包…");
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
        self.begin("正在导入 Eagle…");
        self.effects.push(FilesEffect::ImportEagle { repo_id, parent, library_path, mode });
        true
    }

    pub(super) fn delete_selected(&mut self, ctx: &FileContext) -> bool {
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
        self.begin(if ctx.trash { "正在永久删除…" } else { "正在删除…" });
        self.effects.push(FilesEffect::Delete { repo_id, paths, mode });
        true
    }

    pub(super) fn restore_selected(&mut self, ctx: &FileContext) -> bool {
        if self.selected.is_empty() {
            return false;
        }
        self.mutate_trash(ctx, "restore", self.selected.clone(), "正在还原…")
    }

    pub(super) fn mutate_trash(&mut self, ctx: &FileContext, action: &str, paths: Vec<String>, activity: &str) -> bool {
        let allowed = if action == "restore" { self.can_restore(ctx) } else { self.can_empty_trash(ctx) };
        if !allowed {
            eprintln!("Nana 当前视图不能执行回收站操作：{action}");
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        self.begin(activity);
        self.effects.push(FilesEffect::MutateTrash { repo_id, action: action.to_string(), paths });
        true
    }

    pub(super) fn confirm_hardlink(&mut self, ctx: &FileContext) -> bool {
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
        self.begin("正在确认硬链接…");
        self.effects.push(FilesEffect::ConfirmHardlink { repo_id, candidate_id: prompt.id });
        true
    }

    /// 跳过只记在本地，不调用服务，接着显示下一条未跳过的候选。
    pub(super) fn skip_hardlink(&mut self) -> bool {
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

    pub(super) fn apply_mutation_snapshot(&mut self, ctx: &FileContext, snapshot: FileBrowserSnapshot, created_name: Option<String>) {
        self.mutating = false;
        self.activity.clear();
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
        self.rows = snapshot.entries.iter().map(FileRow::from_entry).collect();
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

    pub(super) fn note_mutation_failed(&mut self, error: String) {
        eprintln!("Nana 文件变更失败：{error}");
        self.mutating = false;
        self.activity.clear();
        self.error = error;
    }

    pub(super) fn note_protocol_finished(&mut self, ctx: &FileContext, result: Result<(), String>, reload: bool, hardlinks: bool) {
        self.mutating = false;
        self.activity.clear();
        match result {
            Ok(()) => {
                self.finish_dialog_success();
                if reload {
                    let path = self.current_path.clone();
                    self.queue_browse(ctx, &path, false, true, true);
                }
                if hardlinks {
                    if let Some(repo_id) = ctx.repo_id.clone() {
                        self.effects.push(FilesEffect::LoadHardlinks { repo_id });
                    }
                }
            }
            Err(error) => {
                eprintln!("Nana 文件变更失败：{error}");
                self.error = error;
            }
        }
    }

    pub(super) fn note_hardlinks(&mut self, result: Result<Vec<HardlinkPrompt>, String>) {
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

    /// 结构更新静默重读硬链接候选。空仓库只记日志。
    /// 不打开对话框，不改加载态和页面错误。
    pub(super) fn refresh_hardlinks_silent(&mut self, repo_id: &str) {
        if repo_id.is_empty() {
            eprintln!("Nana 静默刷新硬链接候选缺少仓库");
            return;
        }
        self.effects.push(FilesEffect::RefreshHardlinks { repo_id: repo_id.to_string() });
    }

    /// 静默替换硬链接候选。成功只换列表；失败只记日志。
    /// 不改对话框、已跳过候选和页面错误。
    pub(super) fn note_hardlinks_silent(&mut self, result: Result<Vec<HardlinkPrompt>, String>) {
        match result {
            Ok(prompts) => {
                self.hardlinks = prompts;
            }
            Err(error) => {
                eprintln!("Nana 静默刷新硬链接候选失败：{error}");
            }
        }
    }

    pub(super) fn note_hardlink_confirmed(&mut self, result: Result<String, String>) {
        self.mutating = false;
        self.activity.clear();
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

    fn begin(&mut self, activity: &str) {
        self.mutating = true;
        self.error.clear();
        self.activity = activity.to_string();
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
