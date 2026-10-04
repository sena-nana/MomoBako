//! 文件浏览与变更状态。
//!
//! 对应 Vue `workspace/files.ts`、`fileOperations.ts`、
//! `useFileBrowserPanelViewModel.ts`、`CopyTargetDialog.vue` 和
//! `HardlinkCandidateDialog.vue`。分页、选择、只读虚拟视图和变更守卫在这里归约。
//! 框选、修饰键、系统对话框、拖放、缩略图空闲预取和操作进度条不在这里完成。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::backend::services::repository::{FileBrowserEntry, FileBrowserSnapshot};
use crate::settings;

use super::workspace::{LibraryCategory, MainRegion, StartupStatus, WorkspacePanel};

/// 首屏分页，对应 `FILE_BROWSER_INITIAL_PAGE_SIZE`。
pub const INITIAL_PAGE_SIZE: usize = 80;
/// 继续加载的分页，对应 `FILE_BROWSER_APPEND_PAGE_SIZE`。
pub const APPEND_PAGE_SIZE: usize = 160;

/// 列表行。合并键是 `kind:path`，避免同名文件和文件夹互相覆盖。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileRow {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub asset_id: Option<String>,
    pub is_virtual: bool,
    pub thumbnail_path: Option<String>,
    pub hardlink_state: Option<String>,
    pub extension: Option<String>,
}

impl FileRow {
    pub fn from_entry(entry: &FileBrowserEntry) -> Self {
        Self {
            path: entry.path.clone(),
            name: entry.name.clone(),
            kind: entry.kind.clone(),
            asset_id: entry.asset_id.clone(),
            is_virtual: entry.is_virtual,
            thumbnail_path: entry.thumbnail_path.clone(),
            hardlink_state: entry.hardlink_state.clone(),
            extension: entry.extension.clone(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.path)
    }
}

/// 智能文件夹查询交给文件列表的只读行。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VirtualQuery {
    pub count: usize,
    pub rows: Vec<FileRow>,
}

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

/// 素材展示方式。未知值回到自适应。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum DisplayMode {
    #[default]
    Adaptive,
    Masonry,
    Grid,
    List,
}

impl DisplayMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Adaptive => "自适应",
            Self::Masonry => "瀑布流",
            Self::Grid => "网格",
            Self::List => "列表",
        }
    }

    pub fn storage_value(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::Masonry => "masonry",
            Self::Grid => "grid",
            Self::List => "list",
        }
    }

    /// 只接受 Vue 存下去的值和中文标签。空串和其他值都是自适应。
    pub fn parse(raw: &str) -> Self {
        match raw.trim() {
            "adaptive" | "自适应" => Self::Adaptive,
            "masonry" | "瀑布流" => Self::Masonry,
            "grid" | "网格" => Self::Grid,
            "list" | "列表" => Self::List,
            other => {
                if !other.is_empty() {
                    eprintln!("Nana 未知的素材展示方式，改用自适应：{other}");
                }
                Self::Adaptive
            }
        }
    }

    pub fn is_list(self) -> bool {
        self == Self::List
    }
}

/// 点击行时使用的选择方式。Nana 点击没有修饰键，所以由按钮显式指定。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SelectionMode {
    #[default]
    Replace,
    Toggle,
    Range,
}

impl SelectionMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Replace => "替换",
            Self::Toggle => "切换",
            Self::Range => "范围",
        }
    }
}

/// 文件表面的对话框。删除不弹框，回收站和普通删除只用请求里的 mode 区分。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileDialog {
    #[default]
    Closed,
    CreateDirectory,
    CreateFile,
    Rename,
    Copy,
    Move,
    Import,
    ImportArchive,
    ImportEagle,
    Hardlink,
}

/// 当前仓库和面板决定哪些变更允许发生。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileContext {
    pub repo_id: Option<String>,
    pub writable: bool,
    pub missing: bool,
    pub trash: bool,
    pub smart_folder: bool,
    pub category_virtual: bool,
}

impl FileContext {
    pub(super) fn from_model(model: &super::ShellViewModel) -> Self {
        let repository = model.workspace.active_repository();
        Self {
            repo_id: model.workspace.active_repo_id.clone(),
            writable: repository.is_some_and(|item| repository_is_writable(&item.status, &item.capabilities)),
            missing: model.navigation_locked(),
            trash: model.workspace.panel == WorkspacePanel::Trash,
            smart_folder: model.workspace.panel == WorkspacePanel::SmartFolder,
            category_virtual: model.workspace.panel == WorkspacePanel::Files
                && model.workspace.library_category != LibraryCategory::All,
        }
    }

    pub(super) fn is_virtual(&self) -> bool {
        self.smart_folder || self.category_virtual
    }
}

/// 仓库可写：状态不是 missing，且能力里有 `write`。
pub fn repository_is_writable(status: &str, capabilities: &[String]) -> bool {
    status != "missing" && capabilities.iter().any(|capability| capability == "write")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BrowsePending {
    repo_id: String,
    path: String,
    trash: bool,
    append: bool,
}

/// 归约后交给宿主的服务请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilesEffect {
    Browse { repo_id: String, path: String, trash: bool, offset: usize, limit: usize, append: bool },
    CreateDirectory { repo_id: String, parent: Option<String>, name: String },
    CreateFile { repo_id: String, parent: Option<String>, name: String },
    Rename { repo_id: String, path: String, new_name: String },
    Copy { repo_id: String, sources: Vec<String>, parent: Option<String> },
    Move { repo_id: String, sources: Vec<String>, parent: String },
    Import { repo_id: String, parent: Option<String>, sources: Vec<String> },
    ImportArchive { repo_id: String, parent: Option<String>, archive_path: String },
    ImportEagle { repo_id: String, parent: Option<String>, library_path: String, mode: String },
    Delete { repo_id: String, paths: Vec<String>, mode: Option<String> },
    MutateTrash { repo_id: String, action: String, paths: Vec<String> },
    LoadHardlinks { repo_id: String },
    ConfirmHardlink { repo_id: String, candidate_id: String },
    LoadAsset { repo_id: String, asset_id: String },
    PersistDisplayMode,
}

/// 文件表面消息。壳层只保留一个 `ShellMessage::Files`。
#[derive(Clone, Debug)]
pub enum FilesMessage {
    SetDisplayMode(DisplayMode),
    SetSelectionMode(SelectionMode),
    ActivateRow(String),
    OpenPath(String),
    LoadMore,
    OpenDialog(FileDialog),
    OpenEagle(String),
    DraftChanged(String),
    CloseDialog,
    SubmitDialog,
    DeleteSelected,
    RestoreSelected,
    RestoreAll,
    EmptyTrash,
    SkipHardlink,
    ConfirmHardlink,
    MutationSnapshot { result: Result<FileBrowserSnapshot, String>, created_name: Option<String> },
    ProtocolFinished { result: Result<(), String>, reload: bool, hardlinks: bool },
    HardlinksLoaded(Result<Vec<HardlinkPrompt>, String>),
    HardlinkConfirmed(Result<String, String>),
    NoteError(String),
}

/// 目录条目、选择、对话框和进行中的变更。
#[derive(Clone, Debug, Default)]
pub struct FilesState {
    pub(super) display_mode: DisplayMode,
    pub(super) rows: Vec<FileRow>,
    pub(super) virtual_rows: Vec<FileRow>,
    pub(super) current_path: String,
    pub(super) total_entries: usize,
    pub(super) has_more: bool,
    pub(super) trash: bool,
    pub(super) loading: bool,
    pub(super) loading_more: bool,
    pub(super) error: String,
    pub(super) mutating: bool,
    pub(super) activity: String,
    pub(super) selected: Vec<String>,
    pub(super) primary: Option<String>,
    pub(super) anchor: Option<String>,
    pub(super) selection_mode: SelectionMode,
    pub(super) dialog: FileDialog,
    pub(super) name_draft: String,
    pub(super) target_draft: String,
    pub(super) import_draft: String,
    pub(super) eagle_mode: String,
    pub(super) rename_path: Option<String>,
    copy_sources: Vec<String>,
    hardlinks: Vec<HardlinkPrompt>,
    skipped_hardlinks: Vec<String>,
    pending: Option<BrowsePending>,
    effects: Vec<FilesEffect>,
}

impl FilesState {
    pub fn take_effects(&mut self) -> Vec<FilesEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 仓库动作的 `selectedCount` 只数多选路径，不含单独的主选。
    pub(crate) fn selected_paths(&self) -> &[String] {
        &self.selected
    }

    /// 测试直接放入多选路径。生产选择仍走文件消息。
    #[cfg(test)]
    pub(crate) fn set_selected_paths(&mut self, paths: Vec<String>) {
        self.selected = paths;
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
        self.begin("正在移动…");
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
        self.begin("正在导入…");
        self.effects.push(FilesEffect::Import { repo_id, parent, sources });
    }

    /// 框选和拖放起点写入多选、主选和锚点。
    pub(crate) fn set_drag_selection(&mut self, paths: Vec<String>, primary: Option<String>, anchor: Option<String>) {
        self.selected = paths;
        self.primary = primary;
        self.anchor = anchor;
    }

    pub fn entry_names(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.name.clone()).collect()
    }

    /// 智能文件夹用查询结果；分类视图隐藏文件夹，但状态里仍保留它们。
    pub(super) fn visible_rows(&self, ctx: &FileContext) -> Vec<FileRow> {
        if ctx.smart_folder {
            return self.virtual_rows.clone();
        }
        if ctx.category_virtual {
            return self.rows.iter().filter(|row| row.kind != "directory").cloned().collect();
        }
        self.rows.clone()
    }

    pub(super) fn current_hardlink(&self) -> Option<&HardlinkPrompt> {
        self.hardlinks.iter().find(|prompt| !self.skipped_hardlinks.iter().any(|id| id == &prompt.id))
    }

    pub(super) fn can_create(&self, ctx: &FileContext) -> bool {
        self.structure_allowed(ctx)
    }

    pub(super) fn can_import(&self, ctx: &FileContext) -> bool {
        self.structure_allowed(ctx)
    }

    pub(super) fn can_transfer(&self, ctx: &FileContext) -> bool {
        self.structure_allowed(ctx) && !self.selected.is_empty() && !self.selection_has_virtual()
    }

    pub(super) fn can_rename(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some()
            && ctx.writable
            && !ctx.missing
            && !ctx.trash
            && !ctx.smart_folder
            && !self.mutating
            && self.selected.len() == 1
            && !self.selection_has_virtual()
    }

    pub(super) fn can_delete(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some()
            && ctx.writable
            && !ctx.missing
            && !ctx.smart_folder
            && !self.mutating
            && !self.selected.is_empty()
            && !self.selection_has_virtual()
    }

    pub(super) fn can_restore(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some()
            && ctx.writable
            && ctx.trash
            && !ctx.missing
            && !self.mutating
            && !self.selected.is_empty()
            && !self.selection_has_virtual()
    }

    pub(super) fn can_empty_trash(&self, ctx: &FileContext) -> bool {
        ctx.repo_id.is_some() && ctx.writable && ctx.trash && !ctx.missing && !self.mutating
    }

    pub(super) fn can_load_more(&self, ctx: &FileContext) -> bool {
        !ctx.is_virtual()
            && ctx.trash == self.trash
            && ctx.repo_id.is_some()
            && self.has_more
            && !self.loading
            && !self.loading_more
            && !self.mutating
    }

    /// 替换进行中的浏览。上一页还没回来时，新的目录请求覆盖 pending，而不是被丢掉。
    pub(super) fn request_browse(&mut self, ctx: &FileContext, path: &str) -> bool {
        self.queue_browse(ctx, path, false, false, false)
    }

    pub(super) fn load_more(&mut self, ctx: &FileContext) -> bool {
        if !self.can_load_more(ctx) {
            return false;
        }
        let repo_id = ctx.repo_id.clone().unwrap_or_default();
        let path = self.current_path.clone();
        self.pending = Some(BrowsePending { repo_id: repo_id.clone(), path: path.clone(), trash: self.trash, append: true });
        self.loading_more = true;
        self.effects.push(FilesEffect::Browse {
            repo_id,
            path,
            trash: self.trash,
            offset: self.rows.len(),
            limit: APPEND_PAGE_SIZE,
            append: true,
        });
        true
    }

    /// 应用目录快照。pending 与仓库、路径或回收站不一致时保留原列表。
    /// `pending == None` 表示启动或侧栏发起的替换。追加按 `kind:path` 去重。
    /// `has_more` 看合并后的条数是否小于 `total_entries`；虚拟视图永远不再分页。
    pub(super) fn apply_browser(&mut self, snapshot: &FileBrowserSnapshot, virtual_view: bool) -> bool {
        let append = self.pending.as_ref().is_some_and(|pending| pending.append);
        if let Some(pending) = &self.pending {
            let same = pending.repo_id == snapshot.repo_id
                && pending.path == normalize_path(&snapshot.current_path)
                && pending.trash == (snapshot.special_location.as_deref() == Some("trash"));
            if !same {
                eprintln!("Nana 忽略过期的文件列表：{}", snapshot.repo_id);
                self.finish_loading();
                return false;
            }
        }
        let incoming: Vec<FileRow> = snapshot.entries.iter().map(FileRow::from_entry).collect();
        if append {
            self.rows = merge_rows(&self.rows, &incoming);
        } else {
            self.rows = incoming;
            self.virtual_rows.clear();
        }
        self.current_path = normalize_path(&snapshot.current_path);
        self.trash = snapshot.special_location.as_deref() == Some("trash");
        self.total_entries = snapshot.total_entries;
        self.has_more = !virtual_view && self.rows.len() < snapshot.total_entries;
        self.error.clear();
        self.finish_loading();
        let rows = self.rows.clone();
        self.prune_against(&rows);
        true
    }

    /// 只有文件表面自己发起的加载失败才吃掉错误。启动失败仍交给壳层切到错误页。
    pub(super) fn note_load_failed(&mut self, error: &str) -> bool {
        if !self.loading && !self.loading_more {
            return false;
        }
        eprintln!("Nana 读取文件列表失败：{error}");
        self.error = error.to_string();
        self.finish_loading();
        true
    }

    pub(super) fn set_virtual_rows(&mut self, rows: Vec<FileRow>) {
        self.virtual_rows = rows;
        let rows = self.virtual_rows.clone();
        self.prune_against(&rows);
    }

    pub(super) fn reduce(&mut self, ctx: &FileContext, message: FilesMessage) {
        match message {
            FilesMessage::SetDisplayMode(mode) => {
                self.display_mode = mode;
                self.effects.push(FilesEffect::PersistDisplayMode);
            }
            FilesMessage::SetSelectionMode(mode) => self.selection_mode = mode,
            FilesMessage::ActivateRow(path) => {
                self.activate(ctx, &path);
            }
            FilesMessage::OpenPath(path) => {
                self.request_browse(ctx, &path);
            }
            FilesMessage::LoadMore => {
                self.load_more(ctx);
            }
            FilesMessage::OpenDialog(dialog) => {
                self.open_dialog(ctx, dialog);
            }
            FilesMessage::OpenEagle(mode) => {
                self.open_eagle(ctx, &mode);
            }
            FilesMessage::DraftChanged(value) => self.set_draft(value),
            FilesMessage::CloseDialog => {
                self.close_dialog();
            }
            FilesMessage::SubmitDialog => {
                self.submit_dialog(ctx);
            }
            FilesMessage::DeleteSelected => {
                self.delete_selected(ctx);
            }
            FilesMessage::RestoreSelected => {
                self.restore_selected(ctx);
            }
            FilesMessage::RestoreAll => {
                self.mutate_trash(ctx, "restoreAll", Vec::new(), "正在还原…");
            }
            FilesMessage::EmptyTrash => {
                self.mutate_trash(ctx, "empty", Vec::new(), "正在清空回收站…");
            }
            FilesMessage::SkipHardlink => {
                self.skip_hardlink();
            }
            FilesMessage::ConfirmHardlink => {
                self.confirm_hardlink(ctx);
            }
            FilesMessage::MutationSnapshot { result, created_name } => match result {
                Ok(snapshot) => self.apply_mutation_snapshot(ctx, snapshot, created_name),
                Err(error) => self.note_mutation_failed(error),
            },
            FilesMessage::ProtocolFinished { result, reload, hardlinks } => {
                self.note_protocol_finished(ctx, result, reload, hardlinks);
            }
            FilesMessage::HardlinksLoaded(result) => self.note_hardlinks(result),
            FilesMessage::HardlinkConfirmed(result) => self.note_hardlink_confirmed(result),
            FilesMessage::NoteError(error) => {
                eprintln!("Nana 文件操作失败：{error}");
                self.error = error;
            }
        }
    }

    /// 从展示方式文件读取。文件缺失、损坏或值未知时使用自适应。
    pub fn load_display_mode_file(&mut self, path: &Path) {
        match fs::read_to_string(path) {
            Ok(raw) => self.display_mode = display_mode_from_file(&raw),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.display_mode = DisplayMode::Adaptive;
            }
            Err(error) => {
                eprintln!("Nana 读取展示方式失败：{error}");
                self.display_mode = DisplayMode::Adaptive;
            }
        }
    }

    pub fn save_display_mode_file(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            if let Err(error) = fs::create_dir_all(parent) {
                eprintln!("Nana 创建展示方式目录失败：{error}");
            }
        }
        let body = format!("{{\"mode\":\"{}\"}}\n", self.display_mode.storage_value());
        if let Err(error) = fs::write(path, body) {
            eprintln!("Nana 保存展示方式失败：{error}");
        }
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

    fn selection_has_virtual(&self) -> bool {
        self.selected.iter().any(|path| {
            self.rows.iter().chain(self.virtual_rows.iter()).any(|row| row.path == *path && row.is_virtual)
        })
    }

    fn queue_browse(&mut self, ctx: &FileContext, path: &str, append: bool, keep_selection: bool, allow_category: bool) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能切换目录");
            return false;
        }
        if ctx.smart_folder || (ctx.category_virtual && !allow_category) {
            eprintln!("Nana 虚拟视图不能按目录分页");
            return false;
        }
        let Some(repo_id) = ctx.repo_id.clone() else {
            eprintln!("Nana 目录浏览没有活动仓库");
            return false;
        };
        let path = normalize_path(path);
        let trash = if append { self.trash } else { ctx.trash };
        self.pending = Some(BrowsePending { repo_id: repo_id.clone(), path: path.clone(), trash, append });
        if append {
            self.loading_more = true;
        } else {
            self.loading = true;
            self.loading_more = false;
            self.virtual_rows.clear();
            if !keep_selection {
                self.selected.clear();
                self.primary = None;
                self.anchor = None;
            }
        }
        self.error.clear();
        self.activity = "正在读取目录…".into();
        self.effects.push(FilesEffect::Browse {
            repo_id,
            path,
            trash,
            offset: if append { self.rows.len() } else { 0 },
            limit: if append { APPEND_PAGE_SIZE } else { INITIAL_PAGE_SIZE },
            append,
        });
        true
    }

    fn finish_loading(&mut self) {
        self.loading = false;
        self.loading_more = false;
        self.pending = None;
        self.activity.clear();
    }

    fn activate(&mut self, ctx: &FileContext, path: &str) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，不能打开条目");
            return false;
        }
        let Some(row) = self.visible_rows(ctx).into_iter().find(|row| row.path == path) else {
            return false;
        };
        if row.kind == "directory" && self.selection_mode == SelectionMode::Replace && !ctx.is_virtual() {
            return self.request_browse(ctx, &row.path);
        }
        self.select_visible(ctx, &row.path, self.selection_mode);
        if row.kind != "directory" {
            if let (Some(repo_id), Some(asset_id)) = (ctx.repo_id.clone(), row.asset_id) {
                self.effects.push(FilesEffect::LoadAsset { repo_id, asset_id });
            }
        }
        true
    }

    fn select_visible(&mut self, ctx: &FileContext, path: &str, mode: SelectionMode) {
        let rows = self.visible_rows(ctx);
        if !rows.iter().any(|row| row.path == path) {
            return;
        }
        let previous_primary = self.primary.clone();
        match mode {
            SelectionMode::Replace => self.replace_selection(path),
            SelectionMode::Toggle => self.toggle_selection(path),
            SelectionMode::Range => self.range_selection(&rows, path),
        }
        self.clear_rename_if_needed(previous_primary);
    }

    fn replace_selection(&mut self, path: &str) {
        self.selected = vec![path.to_string()];
        self.primary = Some(path.to_string());
        self.anchor = Some(path.to_string());
    }

    fn toggle_selection(&mut self, path: &str) {
        if let Some(index) = self.selected.iter().position(|item| item == path) {
            self.selected.remove(index);
        } else {
            self.selected.push(path.to_string());
        }
        if self.selected.iter().any(|item| item == path) {
            self.primary = Some(path.to_string());
            self.anchor = Some(path.to_string());
        } else {
            self.primary = self.selected.first().cloned();
            if self.anchor.as_deref() == Some(path) {
                self.anchor = self.primary.clone();
            }
        }
        if self.selected.is_empty() {
            self.primary = None;
            self.anchor = None;
        }
    }

    /// 没有锚点时范围选择退回替换，避免从列表头意外拉出一段。
    fn range_selection(&mut self, rows: &[FileRow], path: &str) {
        let Some(anchor) = self.anchor.clone() else {
            self.replace_selection(path);
            return;
        };
        let Some(anchor_index) = rows.iter().position(|row| row.path == anchor) else {
            self.replace_selection(path);
            return;
        };
        let Some(index) = rows.iter().position(|row| row.path == path) else {
            return;
        };
        let (start, end) = if anchor_index <= index { (anchor_index, index) } else { (index, anchor_index) };
        self.selected = rows[start..=end].iter().map(|row| row.path.clone()).collect();
        self.primary = Some(path.to_string());
    }

    fn prune_against(&mut self, rows: &[FileRow]) {
        let paths: HashSet<String> = rows.iter().map(|row| row.path.clone()).collect();
        let previous_primary = self.primary.clone();
        self.selected.retain(|path| paths.contains(path));
        if self.primary.as_ref().is_some_and(|path| !paths.contains(path)) {
            self.primary = self.selected.first().cloned();
        }
        if self.selected.is_empty() {
            self.primary = None;
            self.anchor = None;
        } else if self.anchor.as_ref().is_some_and(|path| !paths.contains(path)) {
            self.anchor = self.primary.clone();
        }
        self.clear_rename_if_needed(previous_primary);
    }

    fn clear_rename_if_needed(&mut self, previous_primary: Option<String>) {
        let multiple = self.selected.len() != 1;
        let primary_changed = self.primary != previous_primary;
        if multiple || (self.rename_path.is_some() && primary_changed) {
            self.rename_path = None;
            self.name_draft.clear();
            if self.dialog == FileDialog::Rename {
                self.dialog = FileDialog::Closed;
            }
        }
    }

}


impl super::ShellViewModel {
    pub(super) fn files_surface_visible(&self) -> bool {
        self.workspace.startup.status == StartupStatus::Ready
            && self.workspace.main_region() == MainRegion::HasRepository
            && matches!(
                self.workspace.panel,
                WorkspacePanel::Files | WorkspacePanel::Trash | WorkspacePanel::SmartFolder
            )
    }
}

pub(super) fn reduce_message(model: &mut super::ShellViewModel, message: super::ShellMessage) -> Option<super::ShellMessage> {
    let super::ShellMessage::Files(message) = message else {
        return Some(message);
    };
    let context = FileContext::from_model(model);
    model.files.reduce(&context, message);
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

/// 已有键保留原顺序，新键追加到末尾。
fn merge_rows(existing: &[FileRow], incoming: &[FileRow]) -> Vec<FileRow> {
    let mut seen: HashSet<String> = existing.iter().map(FileRow::key).collect();
    let mut merged = existing.to_vec();
    for row in incoming {
        if seen.insert(row.key()) {
            merged.push(row.clone());
        }
    }
    merged
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

pub(super) fn hardlink_label(state: Option<&str>) -> &'static str {
    match state.unwrap_or("") {
        "primary" => "主归属",
        "linked" => "硬链接关联",
        "copied" | "copiedFallback" => "普通复制",
        "broken" => "关联异常",
        "missing" => "关联缺失",
        _ => "",
    }
}

include!("files_actions.rs");

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

