// 文件变更对话框和变更结果。
// 新建、重命名、复制、移动、导入、删除和硬链接确认都从这里提交。
// 空白名称和空白来源直接返回，不会把状态标成进行中。

/// 导出对话框草稿。首页不打开它。
#[derive(Clone, Debug, Default)]
pub(super) struct ExportDraft {
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

/// 文件页里 Escape 能关掉的一层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilesLayer {
    EntryMenu,
    ImportMenu,
    Export,
    Hardlink,
    Dialog,
}

impl FilesState {
    /// 自定义缩略图不锁整页变更。失败由调度器记错误。
    pub(crate) fn queue_thumbnail(&mut self, effect: FilesEffect) {
        self.effects.push(effect);
    }

    /// 写下文件页错误。菜单和对话框失败都走这里。
    pub(crate) fn note_error(&mut self, error: String) {
        eprintln!("Nana 文件操作失败：{error}");
        self.error = error;
    }

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
                self.import_open = false;
                self.eagle_open = false;
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

    /// 验收页打开复制对话框。不改首页入口。
    pub(super) fn present_copy(&mut self, target: &str) {
        self.export.open = false;
        self.dialog = FileDialog::Copy;
        self.target_draft = target.to_string();
    }

    /// 验收页打开硬链接确认。路径来自调用方，不编造哈希。
    pub(super) fn present_hardlink(&mut self, prompt: HardlinkPrompt) {
        self.export.open = false;
        self.dialog = FileDialog::Hardlink;
        self.skipped_hardlinks.clear();
        self.hardlinks = vec![prompt];
    }

    /// 验收页只打开导出对话框本身。
    pub(super) fn present_export(&mut self) {
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

    pub(super) fn close_export(&mut self) {
        self.export.open = false;
        self.export.error.clear();
        self.export.notice.clear();
        self.export.busy = false;
    }

    /// 把对话框草稿交给导出协议。压缩包没有输出路径时只记失败，不调用宿主。
    pub(super) fn submit_export(&mut self, ctx: &FileContext) {
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
    pub(super) fn note_export(&mut self, result: Result<String, String>) {
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
    pub(super) fn set_export_field(&mut self, field: &str, value: String) {
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
                    self.operation = Some(FileOperation { value: 84.0, indeterminate: false, detail: "刷新文件索引".into() });
                    self.activity = "刷新文件索引".into();
                } else {
                    self.operation = None;
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
                self.operation = None;
            }
        }
    }

    fn operation_for_activity(activity: &str) -> Option<FileOperation> {
        let value = match activity {
            "正在复制…" => 32.0,
            "正在移动…" => 32.0,
            "正在导入…" | "正在导入压缩包…" | "正在导入 Eagle…" => 24.0,
            "正在删除…" | "正在永久删除…" | "正在还原…" | "正在清空回收站…" => 32.0,
            _ => return None,
        };
        let detail = match activity {
            "正在复制…" => "创建硬链接或复制文件",
            "正在移动…" => "移动文件",
            "正在导入…" => "导入文件到当前资源库",
            "正在导入压缩包…" => "预检压缩包条目",
            "正在导入 Eagle…" => "转换 EagleLibrary",
            _ => "正在处理",
        };
        Some(FileOperation { value, indeterminate: false, detail: detail.into() })
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

    /// 刷新文件夹树同步以后重读的候选：换掉列表，跳过的留着；没有别的对话框开着时，有候选就弹出确认。
    /// Vue 的确认框按「第一个没跳过的候选」算出来，候选一到就出现。
    pub(super) fn note_hardlinks_synced(&mut self, prompts: Vec<HardlinkPrompt>) {
        self.hardlinks = prompts;
        if self.dialog == FileDialog::Closed && self.current_hardlink().is_some() {
            self.dialog = FileDialog::Hardlink;
        }
    }

    /// 非静默地重读当前目录（回收站面板读回收站），选择留着、读完按新列表修剪。返回是否排下了读取：
    /// 文件正在变更、虚拟视图或没有仓库时不读。刷新文件夹树同步以后用，对应 Vue `loadFileBrowserForDirectory`。
    pub(super) fn reload_current(&mut self, ctx: &FileContext) -> bool {
        let path = self.current_path.clone();
        self.queue_browse(ctx, &path, false, true, false)
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
        self.operation = Self::operation_for_activity(activity);
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

    /// 单击只改选择。文件顺便读取素材，目录不进入。
    pub(super) fn select_row(&mut self, ctx: &FileContext, path: &str) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能选择条目");
            return false;
        }
        let Some(row) = self.visible_rows(ctx).into_iter().find(|row| row.path == path) else {
            eprintln!("Nana 找不到要点选的文件：{path}");
            return false;
        };
        let kind = row.kind.clone();
        let asset_id = row.asset_id.clone();
        self.select_visible(ctx, &row.path, self.selection_mode);
        if kind != "directory" {
            if let (Some(repo_id), Some(asset_id)) = (ctx.repo_id.clone(), asset_id) {
                self.effects.push(FilesEffect::LoadAsset { repo_id, asset_id });
            }
        }
        true
    }

    /// 双击进入非虚拟目录，或打开文件预览。
    pub(super) fn open_row(&mut self, ctx: &FileContext, path: &str) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能打开条目");
            return false;
        }
        let Some(row) = self.visible_rows(ctx).into_iter().find(|row| row.path == path) else {
            eprintln!("Nana 找不到要打开的文件：{path}");
            return false;
        };
        if row.kind == "directory" && !ctx.is_virtual() {
            return self.request_browse(ctx, &row.path);
        }
        let kind = row.kind.clone();
        let asset_id = row.asset_id.clone();
        let row_path = row.path.clone();
        self.select_visible(ctx, &row_path, SelectionMode::Replace);
        if kind != "directory" {
            if let (Some(repo_id), Some(asset_id)) = (ctx.repo_id.clone(), asset_id) {
                self.effects.push(FilesEffect::LoadAsset { repo_id, asset_id });
            }
        }
        true
    }

    /// 右键打开菜单。路径还没选中时先像单击一样替换成这一条，右侧详情跟着换。
    pub(super) fn open_entry_menu(&mut self, ctx: &FileContext, path: &str, x: f32, y: f32) {
        if self.visible_rows(ctx).iter().all(|row| row.path != path) {
            eprintln!("Nana 找不到要打开菜单的文件：{path}");
            return;
        }
        if !self.selected.iter().any(|item| item == path) {
            let mode = std::mem::replace(&mut self.selection_mode, SelectionMode::Replace);
            if self.select_row(ctx, path) {
                self.note_selected_only(path);
            }
            self.selection_mode = mode;
        }
        self.menu_branch = None;
        self.menu_pending = None;
        self.entry_menu = Some(EntryMenu { path: path.to_string(), x, y });
    }

    /// 把保存结果写回这一行，并解码新的缩略图文件。没有路径时不假装已经有图。
    pub(super) fn note_custom_thumbnail(&mut self, path: &str, thumbnail_path: Option<String>, custom: bool) {
        let mut found = false;
        for row in &mut self.rows {
            if row.path == path {
                apply_thumbnail(row, thumbnail_path.clone(), custom);
                found = true;
            }
        }
        for row in &mut self.virtual_rows {
            if row.path == path {
                apply_thumbnail(row, thumbnail_path.clone(), custom);
                found = true;
            }
        }
        if !found {
            eprintln!("Nana 自定义缩略图写回时找不到条目：{path}");
            return;
        }
        if let Some(thumb) = thumbnail_path.filter(|item| !item.trim().is_empty()) {
            self.effects.push(FilesEffect::DecodeThumbnails { paths: vec![thumb] });
        }
    }

    /// 重新解码这一条缩略图。自定义缩略图保存不走这里。
    pub(super) fn refresh_thumbnail(&mut self, path: &str) {
        if path.trim().is_empty() {
            eprintln!("Nana 刷新缩略图缺少路径");
            return;
        }
        self.effects.push(FilesEffect::DecodeThumbnails { paths: vec![path.to_string()] });
    }

    /// Escape 现在能关掉文件页的哪一层：右键菜单、导入菜单、导出对话框、硬链接确认、文件对话框。
    /// 导出中和变更进行中的对话框不关，没有可关的为 `None`。
    pub(crate) fn escape_layer(&self) -> Option<FilesLayer> {
        if self.entry_menu.is_some() {
            Some(FilesLayer::EntryMenu)
        } else if self.import_open {
            Some(FilesLayer::ImportMenu)
        } else if self.export.open && !self.export.busy {
            Some(FilesLayer::Export)
        } else if self.dialog == FileDialog::Hardlink && !self.mutating {
            Some(FilesLayer::Hardlink)
        } else if self.dialog != FileDialog::Closed && !self.mutating {
            Some(FilesLayer::Dialog)
        } else {
            None
        }
    }

    /// Escape 关掉文件页最上面的一层，顺序见 [`Self::escape_layer`]。没有可关的返回 false，交给壳层继续处理。
    pub(crate) fn dismiss_overlay(&mut self) -> bool {
        match self.escape_layer() {
            Some(FilesLayer::EntryMenu) => {
                self.entry_menu = None;
                self.menu_branch = None;
                self.menu_pending = None;
                true
            }
            Some(FilesLayer::ImportMenu) => {
                self.import_open = false;
                self.eagle_open = false;
                true
            }
            Some(FilesLayer::Export) => {
                self.close_export();
                true
            }
            Some(FilesLayer::Hardlink) => self.skip_hardlink(),
            Some(FilesLayer::Dialog) => self.close_dialog(),
            None => false,
        }
    }

    /// 展开或收起导入菜单。收起时 Eagle 的复制和剪切一起收起。
    pub(super) fn toggle_import_menu(&mut self) {
        self.import_open = !self.import_open;
        if !self.import_open {
            self.eagle_open = false;
        }
    }

    /// 「从 Eagle 导入」下面的复制和剪切。菜单本身保持打开。
    pub(super) fn toggle_eagle_menu(&mut self) {
        self.import_open = true;
        self.eagle_open = !self.eagle_open;
    }

    /// 用工具栏里的文件名直接新建，不再弹出另一层名称框。
    pub(super) fn submit_inline_file(&mut self, ctx: &FileContext) -> bool {
        let name = self.create_name.trim().to_string();
        if name.is_empty() {
            eprintln!("Nana 新建文件名是空的");
            return false;
        }
        self.name_draft = name;
        if !self.submit_named(ctx, false) {
            return false;
        }
        self.create_name.clear();
        true
    }
}

fn apply_thumbnail(row: &mut FileRow, thumbnail_path: Option<String>, custom: bool) {
    row.thumbnail_path = thumbnail_path;
    row.thumbnail_custom = custom;
    row.texture_ready = false;
}

fn split_sources(raw: &str) -> Vec<String> {
    raw.split(['\n', '\r', ';'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn display_mode_from_file(raw: &str) -> DisplayMode {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        eprintln!("Nana 展示方式文件不是 JSON");
        return DisplayMode::Adaptive;
    };
    let Some(mode) = value.get("mode").and_then(|item| item.as_str()) else {
        eprintln!("Nana 展示方式文件缺少 mode");
        return DisplayMode::Adaptive;
    };
    DisplayMode::parse(mode)
}

/// 已有键保留原顺序，新键追加到末尾。
fn merge_rows(existing: &[FileRow], incoming: &[FileRow]) -> Vec<FileRow> {
    let mut seen: std::collections::HashSet<String> = existing.iter().map(FileRow::key).collect();
    let mut merged = existing.to_vec();
    for row in incoming {
        if seen.insert(row.key()) {
            merged.push(row.clone());
        }
    }
    merged
}

pub(super) fn reduce_message(model: &mut super::ShellViewModel, message: super::ShellMessage) -> Option<super::ShellMessage> {
    if let super::ShellMessage::AssetDetailLoaded(Ok(detail)) = &message {
        model.files.note_detail_arrived(&detail.summary.path);
    }
    let super::ShellMessage::Files(message) = message else {
        return Some(message);
    };
    // 拖放的意图按此刻的仓库条件（有没有仓库、可写、在文件面板）收成宿主拖放消息，交给输入归约。
    if let FilesMessage::HostDrop(event) = &message {
        let flags = super::input::file_drop_flags(model);
        let host = super::input::file_drop_message(&flags, event);
        return super::input::reduce_message(model, host);
    }
    // 压缩包导出还没有保存位置时，先弹系统保存对话框，不把空路径交给导出协议。
    let export = &model.files.export;
    let archive_without_path = export.target != "git" && export.output_path.trim().is_empty() && !export.busy;
    let message = if matches!(message, FilesMessage::SubmitExport) && archive_without_path {
        FilesMessage::ChooseExportOutput
    } else {
        message
    };
    if matches!(message, FilesMessage::ChooseExportOutput) {
        let extension = model.files.export.format.trim().to_string();
        let file_name = model.workspace.active_repository().and_then(|repository| {
            let name = repository.name.trim();
            if name.is_empty() || extension.is_empty() { None } else { Some(format!("{name}.{extension}")) }
        });
        model.input.queue_repository_export_dialog(file_name, extension);
        return None;
    }
    match &message {
        FilesMessage::ToggleTagMenu => {
            model.files.tag_draft.clear();
            let next = if model.inspect.tag_menu_open() {
                super::inspect::InspectMessage::CloseTagMenu
            } else {
                super::inspect::InspectMessage::OpenTagMenu { x: 0.0, y: 0.0 }
            };
            return Some(super::ShellMessage::Inspect(next));
        }
        FilesMessage::SubmitTagDraft(value) => {
            let tag = value.trim().to_string();
            if tag.is_empty() {
                return None;
            }
            model.files.tag_draft.clear();
            return Some(super::ShellMessage::Inspect(super::inspect::InspectMessage::AddTag(tag)));
        }
        _ => {}
    }
    let context = FileContext::from_model(model);
    let show_on_files = matches!(
        message,
        FilesMessage::OpenDialog(FileDialog::Import | FileDialog::ImportArchive | FileDialog::ImportEagle)
            | FilesMessage::OpenEagle(_)
    );
    let preview_path = match &message {
        FilesMessage::ActivateRow(path) | FilesMessage::OpenRow(path) | FilesMessage::OpenEntryMenu { path, .. } => Some(path.clone()),
        _ => None,
    };
    model.files.reduce(&context, message);
    if let Some(path) = preview_path {
        let file = model.files.visible_rows(&context).into_iter().find(|row| row.path == path && row.kind != "directory");
        if let Some(row) = file {
            if model.inspect.target_path.as_deref() != Some(row.path.as_str()) {
                model.inspect.begin_selection(&row.path);
            }
        }
    }
    if show_on_files && model.files.dialog != FileDialog::Closed {
        model.workspace.panel = super::workspace::WorkspacePanel::Files;
    }
    None
}

pub fn display_mode_path() -> PathBuf {
    settings::default_path().with_file_name("file-display.json")
}

/// 与 Vue `normalizeRepositoryRelativePath` 相同：去空白、反斜杠转正斜杠、去掉首尾斜杠。
pub(super) fn normalize_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

fn parent_path(path: &str) -> Option<String> {
    let path = normalize_path(path);
    if path.is_empty() { None } else { Some(path) }
}

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
