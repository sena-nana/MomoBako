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
    /// 元数据或解码后的原始宽高。0 表示还不知道，按正方形排。
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// 宿主已经能为这条路径注册纹理。
    pub texture_ready: bool,
    /// 已有像素缩小后的缩略图。没有像素时为 `None`，视图改画类型图标。
    pub thumbnail_rgba: Option<(u32, u32, Vec<u8>)>,
    /// 详情预览用的整页像素，四周留着页边。网格缩略图是同一张页图的缩小。
    pub page_rgba: Option<(u32, u32, Vec<u8>)>,
    /// 元数据色板，最多五枚 `#RRGGBB`。
    pub palette: Vec<String>,
    /// 列表里的大小文案。空字符串表示目录没有单独的大小。
    pub size_label: String,
    /// 修改时间原文。空字符串在详情里写成「未记录」。
    pub modified_at: String,
    pub tags: Vec<String>,
    pub thumbnail_custom: bool,
    pub provider_id: Option<String>,
    pub source_payload: Option<serde_json::Value>,
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
}

/// 右键菜单锚点。坐标来自次级按键。
#[derive(Clone, Debug)]
pub(super) struct EntryMenu {
    pub path: String,
    pub x: f32,
    pub y: f32,
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
            pixel_width: metadata_u32(&entry.metadata, "width"),
            pixel_height: metadata_u32(&entry.metadata, "height"),
            texture_ready: false,
            thumbnail_rgba: None,
            page_rgba: None,
            palette: super::palette::from_metadata_map(&entry.metadata),
            size_label: entry.size_label.clone().unwrap_or_default(),
            modified_at: entry.modified_at.clone().unwrap_or_default(),
            tags: entry.tags.clone(),
            thumbnail_custom: entry.thumbnail_custom,
            provider_id: entry.provider_id.clone(),
            source_payload: entry.source_payload.clone(),
            metadata: entry.metadata.clone(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.path)
    }
}

fn metadata_u32(metadata: &std::collections::BTreeMap<String, serde_json::Value>, key: &str) -> u32 {
    match metadata.get(key) {
        Some(serde_json::Value::Number(number)) => number.as_u64().unwrap_or(0).min(u32::MAX as u64) as u32,
        Some(serde_json::Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
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
    /// 结构更新触发的重读。不打开加载态，失败也不把整页改成错误。
    silent: bool,
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
    /// 结构更新静默重读候选。不打开对话框，失败不写入页面错误。
    RefreshHardlinks { repo_id: String },
    ConfirmHardlink { repo_id: String, candidate_id: String },
    LoadAsset { repo_id: String, asset_id: String },
    PersistDisplayMode,
    /// 读取目录里还没解码的缩略图文件。
    DecodeThumbnails { paths: Vec<String> },
    /// 保存、清除或刷新一条自定义缩略图。字节只在剪贴板 data URL 时带上。
    MutateThumbnail { repo_id: String, path: String, kind: String, action: String, source_path: Option<String>, image_bytes: Option<Vec<u8>> },
    /// 压缩包导出。调用前必须已有真实输出路径。
    ExportArchive { repo_id: String, format: String, output_path: String, compression: String, encrypt: bool, password: String },
    /// Git 导出。远端、分支和说明来自对话框，空值在调度时省略。
    ExportGit { repo_id: String, remote: String, branch: String, message: String },
}

/// 文件表面消息。壳层只保留一个 `ShellMessage::Files`。
#[derive(Clone, Debug)]
pub enum FilesMessage {
    SetDisplayMode(DisplayMode),
    SetSelectionMode(SelectionMode),
    ActivateRow(String),
    /// 双击：目录进入，文件打开预览。
    OpenRow(String),
    OpenEntryMenu { path: String, x: f32, y: f32 },
    CloseEntryMenu,
    RefreshThumbnail(String),
    OpenPath(String),
    LoadMore,
    OpenDialog(FileDialog),
    OpenEagle(String),
    ToggleImportMenu,
    ToggleEagleImport,
    SetCreateName(String),
    SubmitCreateFile,
    DraftChanged(String),
    CloseDialog,
    SubmitDialog,
    SetExportField { field: String, value: String },
    CloseExport,
    SubmitExport,
    /// 排队保存对话框，让用户选出压缩包输出路径。
    ChooseExportOutput,
    /// 导出协议返回。成功文案来自宿主，失败是真实错误。
    ExportFinished(Result<String, String>),
    DeleteSelected,
    RestoreSelected,
    RestoreAll,
    EmptyTrash,
    SkipHardlink,
    ConfirmHardlink,
    MutationSnapshot { result: Result<FileBrowserSnapshot, String>, created_name: Option<String> },
    ProtocolFinished { result: Result<(), String>, reload: bool, hardlinks: bool },
    HardlinksLoaded(Result<Vec<HardlinkPrompt>, String>),
    /// 静默刷新结果。只替换候选，不改对话框和页面错误。
    HardlinksRefreshed(Result<Vec<HardlinkPrompt>, String>),
    HardlinkConfirmed(Result<String, String>),
    NoteError(String),
    /// 缩略图已经写入。路径是缓存文件，用来重新解码纹理。
    ThumbnailSaved { path: String, thumbnail_path: Option<String>, custom: bool },
}

/// 文件操作进度。宽度过渡由壳层动效时钟绘制。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FileOperation {
    pub value: f32,
    pub indeterminate: bool,
    pub detail: String,
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
    /// 导出对话框。默认关闭，首页不提供入口。
    pub(super) export: ExportDraft,
    pub(super) operation: Option<FileOperation>,
    prefetch_due_ms: Option<u64>,
    prefetch_clock: u64,
    pub(super) name_draft: String,
    /// 工具栏里的新建文件名。不占用对话框草稿。
    pub(super) create_name: String,
    pub(super) target_draft: String,
    pub(super) import_draft: String,
    pub(super) eagle_mode: String,
    /// 导入菜单是否展开。从文件夹、ZIP 和 Eagle 只在展开后出现。
    pub(super) import_open: bool,
    /// Eagle 的复制和剪切。收在「从 Eagle 导入」下面。
    pub(super) eagle_open: bool,
    pub(super) rename_path: Option<String>,
    copy_sources: Vec<String>,
    hardlinks: Vec<HardlinkPrompt>,
    skipped_hardlinks: Vec<String>,
    pending: Option<BrowsePending>,
    effects: Vec<FilesEffect>,
    pub(super) entry_menu: Option<EntryMenu>,
}

impl FilesState {
    pub(super) fn dialog_open(&self) -> bool {
        self.export.open || !matches!(self.dialog, FileDialog::Closed)
    }

    pub(super) fn operation_percent(&self) -> Option<f32> {
        self.operation.as_ref().map(|operation| operation.value)
    }

    pub fn operation_label(&self) -> Option<String> {
        self.operation.as_ref().map(|item| format!("{} · {}%", item.detail, item.value as i32))
    }

    pub(super) fn operation_indeterminate(&self) -> bool {
        self.operation.as_ref().is_some_and(|operation| operation.indeterminate)
    }

    /// 虚拟列表还有下一页时，空闲 `PREFETCH_IDLE_MS` 后再解码缩略图。
    pub fn schedule_thumbnail_prefetch(&mut self, has_more: bool, now_ms: u64) {
        self.prefetch_due_ms = has_more.then_some(now_ms.saturating_add(super::motion::PREFETCH_IDLE_MS));
    }

    pub fn poll_thumbnail_prefetch(&mut self, now_ms: u64, paths: &[String]) -> bool {
        if self.prefetch_due_ms.is_some_and(|due| now_ms >= due) {
            self.prefetch_due_ms = None;
            if !paths.is_empty() {
                self.effects.push(FilesEffect::DecodeThumbnails { paths: paths.to_vec() });
            }
            return true;
        }
        false
    }

    pub fn enqueue(&mut self, effect: FilesEffect) {
        self.mutating = true;
        self.effects.push(effect);
    }

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
        self.pending = Some(BrowsePending {
            repo_id: repo_id.clone(),
            path: path.clone(),
            trash: self.trash,
            append: true,
            silent: false,
        });
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
        self.arm_thumbnail_prefetch();
        true
    }

    /// 还有缩略图或下一页时，等空闲间隔再解码，不在快照到达的同一拍发出请求。
    fn arm_thumbnail_prefetch(&mut self) {
        let needs = self.has_more || !self.undecoded_thumbnail_paths().is_empty();
        self.schedule_thumbnail_prefetch(needs, self.prefetch_clock);
    }

    pub fn undecoded_thumbnail_paths(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter(|row| !row.texture_ready)
            .filter_map(|row| row.thumbnail_path.clone())
            .filter(|path| !path.trim().is_empty())
            .collect()
    }

    pub fn prefetch_pending(&self) -> bool {
        self.prefetch_due_ms.is_some()
    }

    /// 实况帧把空闲时钟向前拨。没有待预取时停在原地。
    pub fn tick_prefetch(&mut self, step_ms: u64) -> u64 {
        if self.prefetch_due_ms.is_some() {
            self.prefetch_clock = self.prefetch_clock.saturating_add(step_ms);
        }
        self.prefetch_clock
    }

    /// 用解码出的原始宽高替换布局。同一路径的行一起更新。
    pub(super) fn note_thumbnail_sizes(&mut self, frames: &[super::thumbs::ThumbnailFrame]) {
        for frame in frames {
            if frame.natural_width == 0 || frame.natural_height == 0 {
                continue;
            }
            for row in self.rows.iter_mut().chain(self.virtual_rows.iter_mut()) {
                if row.thumbnail_path.as_deref() == Some(frame.path.as_str()) {
                    row.pixel_width = frame.natural_width;
                    row.pixel_height = frame.natural_height;
                    row.texture_ready = true;
                }
            }
        }
    }

    /// 静默重读当前目录。不改变加载文案和选择；虚拟视图直接返回。
    pub(super) fn reload_silent(&mut self, ctx: &FileContext) -> bool {
        if self.mutating {
            eprintln!("Nana 文件变更进行中，跳过静默刷新");
            return false;
        }
        if ctx.smart_folder || ctx.category_virtual {
            eprintln!("Nana 虚拟视图不按目录静默刷新");
            return false;
        }
        let Some(repo_id) = ctx.repo_id.clone() else {
            eprintln!("Nana 静默刷新没有活动仓库");
            return false;
        };
        let path = self.current_path.clone();
        let trash = ctx.trash;
        let limit = INITIAL_PAGE_SIZE.max(self.rows.len());
        self.pending = Some(BrowsePending {
            repo_id: repo_id.clone(),
            path: path.clone(),
            trash,
            append: false,
            silent: true,
        });
        self.effects.push(FilesEffect::Browse {
            repo_id,
            path,
            trash,
            offset: 0,
            limit,
            append: false,
        });
        true
    }

    /// 只有文件表面自己发起的加载失败才吃掉错误。启动失败仍交给壳层切到错误页。
    /// 静默刷新失败只记日志，保留当前列表和页面。
    pub(super) fn note_load_failed(&mut self, error: &str) -> bool {
        if self.pending.as_ref().is_some_and(|pending| pending.silent) {
            eprintln!("Nana 静默刷新目录失败：{error}");
            self.pending = None;
            return true;
        }
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
                self.select_row(ctx, &path);
            }
            FilesMessage::OpenRow(path) => {
                self.open_row(ctx, &path);
            }
            FilesMessage::OpenEntryMenu { path, x, y } => self.open_entry_menu(ctx, &path, x, y),
            FilesMessage::CloseEntryMenu => self.entry_menu = None,
            FilesMessage::RefreshThumbnail(path) => self.refresh_thumbnail(&path),
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
            FilesMessage::ToggleImportMenu => self.toggle_import_menu(),
            FilesMessage::ToggleEagleImport => self.toggle_eagle_menu(),
            FilesMessage::SetCreateName(value) => self.create_name = value,
            FilesMessage::SubmitCreateFile => {
                self.submit_inline_file(ctx);
            }
            FilesMessage::DraftChanged(value) => self.set_draft(value),
            FilesMessage::CloseDialog => {
                self.close_dialog();
            }
            FilesMessage::SubmitDialog => {
                self.submit_dialog(ctx);
            }
            FilesMessage::SetExportField { field, value } => self.set_export_field(&field, value),
            FilesMessage::CloseExport => self.close_export(),
            FilesMessage::SubmitExport => self.submit_export(ctx),
            FilesMessage::ChooseExportOutput => {}
            FilesMessage::ExportFinished(result) => self.note_export(result),
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
            FilesMessage::HardlinksRefreshed(result) => self.note_hardlinks_silent(result),
            FilesMessage::HardlinkConfirmed(result) => self.note_hardlink_confirmed(result),
            FilesMessage::NoteError(error) => self.note_error(error),
            FilesMessage::ThumbnailSaved { path, thumbnail_path, custom } => self.note_custom_thumbnail(&path, thumbnail_path, custom),
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
        self.pending = Some(BrowsePending {
            repo_id: repo_id.clone(),
            path: path.clone(),
            trash,
            append,
            silent: false,
        });
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
        self.operation = None;
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

include!("files_actions.rs");

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

