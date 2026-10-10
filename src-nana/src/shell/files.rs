//! 文件浏览与变更状态（文件表面的 ViewModel）。
//!
//! 对应 Vue `workspace/files.ts`、`fileOperations.ts`、
//! `useFileBrowserPanelViewModel.ts`、`CopyTargetDialog.vue` 和
//! `HardlinkCandidateDialog.vue`。这里放列表行、对话框、服务请求、消息和状态本身，归约按功能分在子模块：
//! - `files_reduce.rs`：壳层入口、消息分派和 Escape 分层；
//! - `files_browse.rs`：读目录、分页和静默重读；
//! - `files_selection.rs`：单击、双击、右键和框选的选择规则；
//! - `files_mutation.rs`：变更守卫、对话框提交和变更结果；
//! - `files_hardlink.rs`：硬链接候选的读取、确认和跳过；
//! - `files_export.rs`：资源库导出对话框；
//! - `files_thumbnail.rs`：缩略图的空闲预取、尺寸写回和自定义缩略图；
//! - `files_display.rs`：素材展示方式和它的持久化；
//! - `files_operation.rs`：任务弹层「仓库操作」行上属于文件表面的部分。
//!
//! 框选手势、修饰键、系统对话框、拖放手势和缩略图解码不在这里完成。

use crate::backend::services::repository::{FileBrowserEntry, FileBrowserSnapshot};

use super::repo_operation::OperationRequests;
use super::workspace::{LibraryCategory, WorkspacePanel};

#[path = "files_browse.rs"]
mod browse;
#[path = "files_display.rs"]
mod display;
#[path = "files_export.rs"]
mod export;
#[path = "files_hardlink.rs"]
mod hardlink;
#[path = "files_menu_branch.rs"]
mod menu_branch;
#[path = "files_mutation.rs"]
mod mutation;
#[path = "files_operation.rs"]
mod operation;
#[path = "files_reduce.rs"]
mod reduce;
#[path = "files_selection.rs"]
mod selection;
#[path = "files_thumbnail.rs"]
mod thumbnail;

#[path = "local_time.rs"]
pub(crate) mod local_time;

use browse::BrowsePending;
use export::ExportDraft;
use operation::FilesOperation;

pub use display::{display_mode_path, DisplayMode};
pub(crate) use hardlink::hardlink_label;
pub use hardlink::HardlinkPrompt;
pub use menu_branch::BranchSource;
pub(crate) use menu_branch::MenuBranch;
pub(super) use operation::{FileOperation, ReadProgress};
pub use reduce::FilesLayer;
pub(super) use reduce::reduce_message;

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
    /// 元数据色板，最多五枚 `#RRGGBB`。
    pub palette: Vec<String>,
    /// 列表里的大小文案。空字符串表示目录没有单独的大小。
    pub size_label: String,
    /// 修改时间原文。空字符串在详情里写成「未记录」。
    pub modified_at: String,
    pub tags: Vec<String>,
    pub thumbnail_custom: bool,
    pub provider_id: Option<String>,
    /// 来源里的条目标识。来源载荷没有歌单标识时，「创建播放列表」用它拼播放集标识。
    pub provider_item_id: Option<String>,
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
            palette: super::palette::from_metadata_map(&entry.metadata),
            size_label: entry.size_label.clone().unwrap_or_default(),
            modified_at: entry.modified_at.clone().unwrap_or_default(),
            tags: entry.tags.clone(),
            thumbnail_custom: entry.thumbnail_custom,
            provider_id: entry.provider_id.clone(),
            provider_item_id: entry.provider_item_id.clone(),
            source_payload: entry.source_payload.clone(),
            metadata: entry.metadata.clone(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.path)
    }

    /// 与 Vue `entryDisplayTitle` 相同：文件去掉末尾的扩展名（不分大小写），目录保留原名。
    pub fn display_title(&self) -> String {
        let name = self.name.trim();
        if self.kind == "directory" {
            return name.to_string();
        }
        let Some(extension) = self.extension.as_deref().map(str::trim).filter(|ext| !ext.is_empty()) else {
            return name.to_string();
        };
        let suffix = format!(".{}", extension.to_lowercase());
        if name.to_lowercase().ends_with(&suffix) && name.len() >= suffix.len() {
            name[..name.len() - suffix.len()].to_string()
        } else {
            name.to_string()
        }
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

/// 点击行时的选择方式，由点下去时按着的修饰键决定（`input::HeldModifiers`）。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SelectionMode {
    #[default]
    Replace,
    Toggle,
    Range,
}

/// 文件表面的对话框。删除不弹框，回收站和普通删除只用请求里的 mode 区分。
/// 建文件走工具栏输入框，新建文件夹走侧栏文件夹对话框，移动走拖放，都不在这里。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum FileDialog {
    #[default]
    Closed,
    Rename,
    Copy,
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
    /// 启动和换仓库后在后台查一次候选（Vue `queueRepositoryBackgroundLoads`）。失败只记日志。
    CheckHardlinks { repo_id: String },
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
    /// 写入系统剪贴板，例如元数据里的来源链接。
    CopyText(String),
}

/// 文件表面消息。壳层只保留一个 `ShellMessage::Files`。
#[derive(Clone, Debug)]
pub enum FilesMessage {
    SetDisplayMode(DisplayMode),
    /// 单击一行。选择方式按点下去时的修饰键：Shift 范围、Ctrl / Meta 切换，否则替换。
    ActivateRow(String),
    /// 双击：目录进入，文件打开预览。
    OpenRow(String),
    OpenEntryMenu { path: String, x: f32, y: f32 },
    CloseEntryMenu,
    /// 指针或键盘焦点进出了右键菜单里的分组（含它的子菜单），子菜单跟着展开或收起。
    MenuBranchChanged { branch: String, source: BranchSource, inside: bool },
    /// 需要二次确认的菜单项第一次被点：只变成待确认，菜单不关。
    ArmMenuConfirm(String),
    /// 列表区内容宽度变了。瀑布流按新宽度重新分列。
    ListResized(f32),
    /// 展开或收起元数据里的标签组。`expanded` 是点之前画出来的状态。
    ToggleTags { path: String, expanded: bool },
    /// 打开或关上标签菜单。
    ToggleTagMenu,
    /// 标签菜单里新建标签的输入。
    SetTagDraft(String),
    /// 复制一段文字到系统剪贴板。
    CopyText(String),
    /// 把输入的新标签加进草稿。
    SubmitTagDraft(String),
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
    /// 启动和换仓库后查到的候选：换掉列表，没有别的对话框时有候选就弹出确认。
    HardlinksChecked(Result<Vec<HardlinkPrompt>, String>),
    HardlinkConfirmed(Result<String, String>),
    NoteError(String),
    /// 缩略图已经写入。路径是缓存文件，用来重新解码纹理。
    ThumbnailSaved { path: String, thumbnail_path: Option<String>, custom: bool },
    /// 外部文件拖到文件列上：悬停、放下或离开。能不能放、放进哪个仓库由归约按当时的 ViewModel 决定。
    HostDrop(nana_ui::runtime::FileDropEvent),
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
    pub(super) selected: Vec<String>,
    pub(super) primary: Option<String>,
    pub(super) anchor: Option<String>,
    pub(super) dialog: FileDialog,
    /// 导出对话框。默认关闭，首页不提供入口。
    pub(super) export: ExportDraft,
    /// 对任务弹层「仓库操作」行的请求，归约结束时交给壳层的槽位。
    pub(super) operation: OperationRequests,
    operation_kind: Option<FilesOperation>,
    prefetch_due_ms: Option<u64>,
    prefetch_clock: u64,
    pub(super) name_draft: String,
    /// 工具栏里的新建文件名。不占用对话框草稿，建成以后才清空（Vue `handleCreateFile`）。
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
    /// 单击选中、还没打开预览的文件。和 Vue 的 `selectedFilePath` 对 `previewFileEntry`：
    /// 单击只在右侧详情里看，双击或「预览」才进入预览页。
    pub(super) select_only: Option<String>,
    /// 单击发起、还没回来的素材读取。回来时保持「只选中」；别处发起的读取回来就进入预览。
    pub(super) click_load: Option<String>,
    /// 右键菜单里指针和焦点所在的分组（「缩略图」「加入播放列表」），决定展开哪个子菜单。
    pub(super) menu_branch: MenuBranch,
    /// 右键菜单里等第二次点击确认的那一项（回收站的「彻底删除」）。
    pub(super) menu_pending: Option<String>,
    /// 列表区内容的实际宽度，瀑布流按它定列数。还没有布局回报时为 `None`。
    pub(super) list_width: Option<f32>,
    /// 元数据编辑里「新建标签」的输入草稿。
    pub(super) tag_draft: String,
    /// 用户手动展开或收起标签组的那个文件。别的文件按「有标签就展开」。
    pub(super) tags_open: Option<(String, bool)>,
}

impl FilesState {
    pub fn take_effects(&mut self) -> Vec<FilesEffect> {
        std::mem::take(&mut self.effects)
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
}

/// 与 Vue `normalizeRepositoryRelativePath` 相同：去空白、反斜杠转正斜杠、去掉首尾斜杠。
pub(super) fn normalize_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

/// 变更请求里的父目录：根目录写成 `None`。
fn parent_path(path: &str) -> Option<String> {
    let path = normalize_path(path);
    if path.is_empty() { None } else { Some(path) }
}

#[cfg(test)]
#[path = "files_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "files_ui_tests.rs"]
mod ui_tests;
#[cfg(test)]
#[path = "files_mount_tests.rs"]
mod mount_tests;
#[cfg(test)]
#[path = "files_virtual_tests.rs"]
mod virtual_tests;
#[cfg(test)]
#[path = "files_select_tests.rs"]
mod select_tests;
