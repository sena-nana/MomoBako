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
//! - `files_thumbnail_ensure.rs`：浏览时逐条请后端生成还没有的缩略图；
//! - `files_virtual_prefetch.rs`：来源目录还有下一页时，空闲后再读下一页，只生成缩略图、不画进列表；
//! - `files_display.rs`：素材展示方式和它的持久化；
//! - `files_operation.rs`：任务弹层「仓库操作」行上属于文件表面的部分。
//!
//! 框选手势、修饰键、系统对话框、拖放手势和缩略图解码不在这里完成。

use crate::backend::services::repository::{
    AssetSummary, FileBrowserEntry, FileBrowserSnapshot, FileTreeNode,
};

use super::repo_operation::OperationRequests;
use super::workspace::{LibraryCategory, WorkspacePanel};

#[path = "files_browse.rs"]
mod browse;
#[path = "files_display.rs"]
mod display;
#[path = "files_thumbnail_ensure.rs"]
mod ensure;
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
#[path = "files_virtual_prefetch.rs"]
mod virtual_prefetch;

#[path = "local_time.rs"]
pub(crate) mod local_time;

use browse::{BrowsePending, ImportFollowup};
use export::ExportDraft;
use operation::FilesOperation;

pub use display::{DisplayMode, display_mode_path};
pub use ensure::EnsuredThumbnail;
pub use hardlink::HardlinkPrompt;
pub(crate) use hardlink::hardlink_label;
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
    /// 来源条目已经落在本机的文件。打开和定位优先用它，不拼仓库根（Vue `localAbsolutePath`）。
    pub local_absolute_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub hardlink_state: Option<String>,
    pub extension: Option<String>,
    /// 元数据或解码后的原始宽高。卡片排版不直接用它：文件夹保持正方形，文件要等缩略图画出来
    /// （`texture_ready`）才用解码出的尺寸，见 `files_cards.rs`。
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// 宿主已经为这条缩略图登记了纹理（或者这一帧就登记）。宿主卸掉纹理时回到假，卡片画类型图标。
    pub texture_ready: bool,
    /// 元数据色板，最多五枚 `#RRGGBB`。
    pub palette: Vec<String>,
    /// 列表里的大小文案。空字符串表示目录没有单独的大小。
    pub size_label: String,
    /// 修改时间原文。空字符串在详情里写成「未记录」。
    pub modified_at: String,
    pub tags: Vec<String>,
    /// 这条还出现在哪些目录（Vue `aliasPaths`，含自身路径）。元数据「多归属位置」读这一份：
    /// 继续加载会换掉上一页目录快照，智能文件夹结果也不在那一页里。
    pub alias_paths: Vec<String>,
    pub thumbnail_custom: bool,
    pub provider_id: Option<String>,
    /// 条目在来源里的标识（单曲是歌曲标识，歌单目录是歌单标识）。来源载荷和元数据里没有条目自己那一种标识时，
    /// 来源条目动作退回用它（`entry_actions::source_number`）。
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
            local_absolute_path: entry.local_absolute_path.clone(),
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
            alias_paths: entry.alias_paths.clone(),
            thumbnail_custom: entry.thumbnail_custom,
            provider_id: entry.provider_id.clone(),
            provider_item_id: entry.provider_item_id.clone(),
            source_payload: entry.source_payload.clone(),
            metadata: entry.metadata.clone(),
        }
    }

    /// Vue `assetSummaryToFileEntry`：分类视图的一行来自仓库摘要。摘要里没有元数据，色板和尺寸等详情读回再补。
    pub(super) fn from_asset(asset: &AssetSummary) -> Self {
        let extension = {
            let extension = asset.extension.trim();
            (!extension.is_empty()).then(|| extension.to_ascii_lowercase())
        };
        Self::from_entry(&FileBrowserEntry {
            path: asset.path.clone(),
            name: asset.filename.clone(),
            kind: "file".into(),
            extension,
            size_bytes: Some(asset.size_bytes),
            size_label: Some(asset.size_label.clone()),
            modified_at: Some(asset.modified_at.clone()),
            asset_id: Some(asset.asset_id.clone()),
            status: Some(asset.status.clone()),
            thumbnail_path: asset.thumbnail_path.clone(),
            thumbnail_custom: false,
            hardlink_group_id: asset.hardlink_group_id.clone(),
            hardlink_state: asset.hardlink_state.clone(),
            tags: asset.tags.clone(),
            alias_paths: vec![asset.path.clone()],
            folder_metadata: None,
            metadata: std::collections::BTreeMap::new(),
            is_virtual: asset.is_virtual,
            provider_id: asset.provider_id.clone(),
            provider_item_id: asset.provider_item_id.clone(),
            source_payload: asset.source_payload.clone(),
            local_absolute_path: asset.local_absolute_path.clone(),
        })
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.path)
    }

    /// 与 Vue `entryDisplayTitle` 相同：文件去掉末尾的扩展名（不分大小写），目录保留原名。
    pub fn display_title(&self) -> String {
        entry_display_title(&self.kind, &self.name, self.extension.as_deref())
    }
}

/// 页头和列表共用的标题。
///
/// 目录保留原名。文件只在名称以条目扩展名结尾时去掉这一段，扩展名空着时不从文件名里猜。
pub(crate) fn entry_display_title(kind: &str, name: &str, extension: Option<&str>) -> String {
    let name = name.trim();
    if kind == "directory" {
        return name.to_string();
    }
    let Some(extension) = extension.map(str::trim).filter(|ext| !ext.is_empty()) else {
        return name.to_string();
    };
    let suffix = format!(".{}", extension.to_lowercase());
    if name.to_lowercase().ends_with(&suffix) && name.len() >= suffix.len() {
        name[..name.len() - suffix.len()].to_string()
    } else {
        name.to_string()
    }
}

fn metadata_u32(
    metadata: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> u32 {
    match metadata.get(key) {
        Some(serde_json::Value::Number(number)) => {
            number.as_u64().unwrap_or(0).min(u32::MAX as u64) as u32
        }
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
            writable: repository
                .is_some_and(|item| repository_is_writable(&item.status, &item.capabilities)),
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

/// 结果卡上一格计数。缺失和真 0 页面上都是「0」，但缺失不是导入出来的 0。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EagleSummaryCount {
    /// 响应里没有这个字段，或值是 null。对应 Vue `value ?? 0`。
    Missing,
    /// 响应里的非负整数，含 0。
    Count(u64),
    /// 小数、负数或其他不能当条数的值。这一行不画，也不改成 0。
    NotCount,
}

impl EagleSummaryCount {
    /// 这一格要显示的文字。`NotCount` 不显示。
    pub fn label(self) -> Option<String> {
        match self {
            Self::Missing => Some("0".into()),
            Self::Count(value) => Some(value.to_string()),
            Self::NotCount => None,
        }
    }
}

/// 警告行。`kind` 是 Vue 的 `type || warningType || "warning"`，`text` 是 `warningText`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EagleImportWarning {
    pub kind: String,
    pub text: String,
}

/// Eagle 导入结果卡。有真值 `summary` 才构造；九个计数各自按响应取值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EagleImportReport {
    pub imported_files: EagleSummaryCount,
    pub imported_directories: EagleSummaryCount,
    /// `warnings` 数组的长度。不是数组时为 0。最多只展示前 20 条，条数仍用这个长度。
    pub warning_count: u64,
    /// 最多 20 条。文字只来自 Vue `warningText`，没有文字时是空字符串。
    pub warnings: Vec<EagleImportWarning>,
    /// `summary.importedTrashEntries`。
    pub imported_trash_entries: EagleSummaryCount,
    /// `summary.importedShortcuts`。
    pub imported_shortcuts: EagleSummaryCount,
    /// `summary.importedSmartFolders`。
    pub imported_smart_folders: EagleSummaryCount,
    /// `summary.importedRepositoryActions`。
    pub imported_repository_actions: EagleSummaryCount,
    /// `summary.importedTagGroups`。
    pub imported_tag_groups: EagleSummaryCount,
    /// `summary.importedAliasGroups`。
    pub imported_alias_groups: EagleSummaryCount,
    /// `summary.importedHardlinkGroups`。
    pub imported_hardlink_groups: EagleSummaryCount,
}

/// 归约后交给宿主的服务请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FilesEffect {
    Browse {
        repo_id: String,
        path: String,
        trash: bool,
        offset: usize,
        limit: usize,
        append: bool,
    },
    CreateDirectory {
        repo_id: String,
        parent: Option<String>,
        name: String,
    },
    CreateFile {
        repo_id: String,
        parent: Option<String>,
        name: String,
    },
    Rename {
        repo_id: String,
        path: String,
        new_name: String,
    },
    Copy {
        repo_id: String,
        sources: Vec<String>,
        parent: Option<String>,
    },
    Move {
        repo_id: String,
        sources: Vec<String>,
        parent: String,
    },
    Import {
        repo_id: String,
        parent: Option<String>,
        sources: Vec<String>,
    },
    ImportArchive {
        repo_id: String,
        parent: Option<String>,
        archive_path: String,
    },
    ImportEagle {
        repo_id: String,
        parent: Option<String>,
        library_path: String,
        mode: String,
    },
    Delete {
        repo_id: String,
        paths: Vec<String>,
        mode: Option<String>,
    },
    MutateTrash {
        repo_id: String,
        action: String,
        paths: Vec<String>,
    },
    /// 复制以后读候选。`epoch` 是请求时候选列表的版本，结果按它认过期（见 `files_hardlink.rs`）。
    LoadHardlinks {
        repo_id: String,
        epoch: u64,
    },
    /// 结构更新静默重读候选。不打开对话框，失败不写入页面错误。
    RefreshHardlinks {
        repo_id: String,
        epoch: u64,
    },
    /// 启动和换仓库后在后台查一次候选（Vue `queueRepositoryBackgroundLoads`），确认以后也用它重读。失败只记日志。
    CheckHardlinks {
        repo_id: String,
        epoch: u64,
    },
    ConfirmHardlink {
        repo_id: String,
        candidate_id: String,
    },
    LoadAsset {
        repo_id: String,
        asset_id: String,
    },
    PersistDisplayMode,
    /// 读取目录里还没解码的缩略图文件。
    DecodeThumbnails {
        paths: Vec<String>,
    },
    /// 浏览时给还没有缩略图的条目要一张（Vue `loadThumbnailsForSnapshot`）。
    /// `source_url` 为空是本地文件的 `ensure`；有值是来源封面的 `save + sourceUrl`，地址必须是条目上的真实 http(s)。
    /// 一次只有一条在途，结果回来再排下一条（`files_thumbnail_ensure.rs`）。
    EnsureThumbnail {
        repo_id: String,
        path: String,
        source_url: Option<String>,
    },
    /// 来源目录还有下一页时，空闲后再读这一页，只为生成缩略图，不画进列表（Vue `queueVirtualThumbnailPrefetch`）。
    PrefetchVirtualPage {
        repo_id: String,
        path: String,
        trash: bool,
        offset: usize,
        limit: usize,
    },
    /// 作品信息或候选上的「保存封面」：`thumbnails:ensure` 的 `save + sourceUrl`。
    /// 地址是条目或候选上已有的 http(s)，不走浏览队列，免得和逐条生成抢在途标记。
    SaveCover {
        repo_id: String,
        path: String,
        source_url: String,
    },
    /// 保存、清除或刷新一条自定义缩略图。字节只在剪贴板 data URL 时带上。
    MutateThumbnail {
        repo_id: String,
        path: String,
        kind: String,
        action: String,
        source_path: Option<String>,
        image_bytes: Option<Vec<u8>>,
    },
    /// 压缩包导出。调用前必须已有真实输出路径。
    ExportArchive {
        repo_id: String,
        format: String,
        output_path: String,
        compression: String,
        encrypt: bool,
        password: String,
    },
    /// Git 导出。远端、分支和说明来自对话框，空值在调度时省略。
    ExportGit {
        repo_id: String,
        remote: String,
        branch: String,
        message: String,
    },
    /// 写入系统剪贴板，例如元数据里的来源链接。
    CopyText(String),
}

/// 复制、移动、删除协议 JSON 里的文件夹树。
///
/// `current_path` 用来认出分类视图路径对不上。没有 `tree` 时不构造。
#[derive(Clone, Debug)]
pub struct ProtocolFolderTree {
    pub repo_id: String,
    pub current_path: String,
    /// 快照是回收站时为真，用来和在途读取对上。
    pub trash: bool,
    pub nodes: Vec<FileTreeNode>,
}

/// 文件表面消息。壳层只保留一个 `ShellMessage::Files`。
#[derive(Clone, Debug)]
pub enum FilesMessage {
    SetDisplayMode(DisplayMode),
    /// 单击一行。选择方式按点下去时的修饰键：Shift 范围、Ctrl / Meta 切换，否则替换。
    ActivateRow(String),
    /// 双击：目录进入，文件打开预览。
    OpenRow(String),
    /// 预览页「返回」。只收起预览，不重读目录。
    ExitPreview,
    OpenEntryMenu {
        path: String,
        x: f32,
        y: f32,
    },
    CloseEntryMenu,
    /// 指针或键盘焦点进出了右键菜单里的分组（含它的子菜单），子菜单跟着展开或收起。
    MenuBranchChanged {
        branch: String,
        source: BranchSource,
        inside: bool,
    },
    /// 需要二次确认的菜单项第一次被点：只变成待确认，菜单不关。
    ArmMenuConfirm(String),
    /// 列表区内容宽度变了。瀑布流按新宽度重新分列。
    ListResized(f32),
    /// 展开或收起元数据里的标签组。`expanded` 是点之前画出来的状态。
    ToggleTags {
        path: String,
        expanded: bool,
    },
    /// 打开或关上标签菜单。
    ToggleTagMenu,
    /// 标签菜单里新建标签的输入。
    SetTagDraft(String),
    /// 复制一段文字到系统剪贴板。
    CopyText(String),
    /// 把输入的新标签加进草稿。
    SubmitTagDraft(String),
    /// 「刷新缩略图」：请后端按源文件重新生成这一条的缩略图。
    RefreshThumbnail(String),
    OpenPath(String),
    LoadMore,
    OpenDialog(FileDialog),
    OpenEagle(String),
    ToggleImportMenu,
    /// 键盘从「导入」打开菜单：只开不关，已经开着时不动。
    OpenImportMenu,
    ToggleEagleImport,
    SetCreateName(String),
    SubmitCreateFile,
    DraftChanged(String),
    CloseDialog,
    SubmitDialog,
    SetExportField {
        field: String,
        value: String,
    },
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
    MutationSnapshot {
        result: Result<FileBrowserSnapshot, String>,
        created_name: Option<String>,
    },
    /// 协议任务结束。`folder_tree` 是复制、移动、删除结果里的文件夹树；没有树时是 `None`，侧栏不动。
    ProtocolFinished {
        result: Result<(), String>,
        reload: bool,
        hardlinks: bool,
        folder_tree: Option<ProtocolFolderTree>,
    },
    /// Eagle 导入协议返回。`Ok(Some)` 是响应里的文件数、目录数、警告，以及真有的其余计数；`Ok(None)` 是成功但缺这三个字段；`Err` 是协议失败原因。
    EagleImportSettled {
        result: Result<Option<EagleImportReport>, String>,
    },
    /// 复制以后读到的候选。`epoch` 是请求时候选列表的版本，比现在旧的结果丢掉。
    HardlinksLoaded {
        epoch: u64,
        result: Result<Vec<HardlinkPrompt>, String>,
    },
    /// 静默刷新结果。只替换候选，不改对话框和页面错误。
    HardlinksRefreshed {
        epoch: u64,
        result: Result<Vec<HardlinkPrompt>, String>,
    },
    /// 启动和换仓库后查到的候选：换掉列表，没有别的对话框时有候选就弹出确认。
    HardlinksChecked {
        epoch: u64,
        result: Result<Vec<HardlinkPrompt>, String>,
    },
    /// 确认硬链接的结果：成功时是确认掉的候选。`repo_id` 是确认时的仓库。
    HardlinkConfirmed {
        repo_id: String,
        result: Result<String, String>,
    },
    NoteError(String),
    /// 缩略图已经写入。路径是缓存文件，用来重新解码纹理。
    ThumbnailSaved {
        path: String,
        thumbnail_path: Option<String>,
        custom: bool,
    },
    /// 浏览时生成缩略图的结果。`repo_id` 是请求时的仓库，已经换了仓库的结果不写回。
    ThumbnailEnsured {
        repo_id: String,
        path: String,
        result: Result<EnsuredThumbnail, String>,
    },
    /// 外部文件拖到文件列上：悬停、放下或离开。能不能放、放进哪个仓库由归约按当时的 ViewModel 决定。
    HostDrop(nana_ui::runtime::FileDropEvent),
}

/// 目录条目、选择、对话框和进行中的变更。
#[derive(Clone, Debug, Default)]
pub struct FilesState {
    pub(super) display_mode: DisplayMode,
    pub(super) rows: Vec<FileRow>,
    pub(super) virtual_rows: Vec<FileRow>,
    /// 分类视图正在显示的素材。从仓库摘要筛出来，当前目录的 `rows` 留着，回到「全部」不用再读。
    pub(super) category_rows: Vec<FileRow>,
    pub(super) current_path: String,
    /// 已经画出来的目录（路径，是否回收站）。侧栏点文件夹会先改 `current_path`，
    /// 静默重读认的是这一份。`None` 表示还没有目录快照，第一次读取要显示「正在读取目录」。
    shown_directory: Option<(String, bool)>,
    pub(super) total_entries: usize,
    pub(super) has_more: bool,
    /// 这一段滚动已经为底部哨兵发过续读。列表长出新行，或滚离哨兵以后清掉。
    scroll_page_armed: bool,
    pub(super) trash: bool,
    pub(super) loading: bool,
    pub(super) loading_more: bool,
    pub(super) error: String,
    pub(super) mutating: bool,
    /// 多选还原成功后不采用变更快照，改为清空选择并非静默重读回收站（Vue `restoreTrashEntries`）。
    reload_trash_on_success: bool,
    /// 刚采用的目录快照里的文件夹树（仓库 id，节点）。壳层取走后写入侧栏；没有树时不放。
    adopted_tree: Option<(String, Vec<FileTreeNode>)>,
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
    /// 文档里还有缩略图节点引用的缩略图文件（视口和前后预留的行、详情卡片、播放条）。宿主每次文档变了
    /// 以后报一次；还没报过（没有视图）时为 `None`，预取不按它筛。
    mounted_thumbnails: Option<std::collections::HashSet<String>>,
    /// `mounted_thumbnails` 每变一次加一，宿主据此决定这一帧要不要回收纹理。
    mounted_version: u64,
    /// 已经请求解码、还没送回像素的缩略图文件。解码失败的留在这里，不反复重试。
    decode_requested: std::collections::HashSet<String>,
    /// 缩略图文件路径到取样色板。元数据没有色板时，详情预览和预览页用这一份（Vue `thumbnailPalettes`）。
    sampled_palettes: std::collections::HashMap<String, Vec<String>>,
    /// 浏览时生成缩略图的队列：排过哪些、在途的是哪条、列表换过没有（`files_thumbnail_ensure.rs`）。
    ensure: ensure::EnsureQueue,
    /// 来源目录下一页的缩略图预取。不画进列表，见 `files_virtual_prefetch.rs`。
    virtual_page: virtual_prefetch::VirtualPagePrefetch,
    pub(super) name_draft: String,
    /// 工具栏里的新建文件名。不占用对话框草稿，建成以后才清空（Vue `handleCreateFile`）。
    pub(super) create_name: String,
    pub(super) target_draft: String,
    pub(super) import_draft: String,
    pub(super) eagle_mode: String,
    /// 文件菜单点了复制或剪切，等壳层去排队和「选择目录」同一套文件夹对话框。
    pub(super) eagle_menu_mode: Option<String>,
    /// 导入菜单是否展开。从文件夹、ZIP 和 Eagle 只在展开后出现。
    pub(super) import_open: bool,
    /// Eagle 的复制和剪切。收在「从 Eagle 导入」下面。
    pub(super) eagle_open: bool,
    pub(super) rename_path: Option<String>,
    /// 这次重命名的新名字。成功快照里同名的那一条变成选择（Vue `renameWorkspaceEntry`）。
    rename_select: Option<String>,
    copy_sources: Vec<String>,
    hardlinks: Vec<HardlinkPrompt>,
    skipped_hardlinks: Vec<String>,
    /// 候选列表的版本：确认结果回来、换仓库时加一，之前发出的读取回来时已经过期，见 `files_hardlink.rs`。
    hardlink_epoch: u64,
    /// 确认结果回来以后在重读候选：确认框的按钮禁用，读回来（或读失败）才放开。
    pub(super) hardlink_rereading: bool,
    pending: Option<BrowsePending>,
    /// 导入提交时记下的改选。协议成功并排下重读以后挪到那次读取上，目录快照对上才用。
    import_followup: Option<ImportFollowup>,
    /// 这次拖放移动成功以后怎么改选：被移走的路径，和目标目录。失败就丢掉，不改选择。
    move_reselect: Option<(Vec<String>, String)>,
    /// 多选删除的路径。成功且当前列表里有文件夹时，另读侧栏树（Vue `deleteWorkspaceEntries` 的 `currentWithTree`）。
    multi_delete_paths: Option<Vec<String>>,
    /// 移走或多项删除成功后要另读的侧栏树仓库。壳层取走后发一次目录树读取。
    folder_tree_reread: Option<String>,
    /// 这次复制成功以后怎么改选：来源路径，和复制进去的目录。失败就丢掉，不改选择。
    copy_reselect: Option<(Vec<String>, String)>,
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
    /// 已经读回的素材详情的素材 id。空字符串表示还没有。详情卡只在和条目的素材 id 相同
    /// 时叠下面那份元数据（Vue `withActiveAssetMetadata`），不改列表行。
    detail_asset_id: String,
    /// 上面这份详情的元数据。同键后来的盖住先前的，和 `Object.fromEntries` 一样。
    detail_metadata: std::collections::BTreeMap<String, serde_json::Value>,
}

impl FilesState {
    pub fn take_effects(&mut self) -> Vec<FilesEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 取走这次采用的文件夹树。没有则侧栏保持原样。
    pub(in crate::shell) fn take_adopted_tree(&mut self) -> Option<(String, Vec<FileTreeNode>)> {
        self.adopted_tree.take()
    }

    /// 取走移走或多项删除之后要另读的侧栏树仓库。没有则不发读取。
    pub(in crate::shell) fn take_folder_tree_reread(&mut self) -> Option<String> {
        self.folder_tree_reread.take()
    }

    pub fn entry_names(&self) -> Vec<String> {
        self.rows.iter().map(|row| row.name.clone()).collect()
    }

    /// 现在画出来的行，不复制。智能文件夹是查询结果；分类视图是摘要筛出来的素材；其余是当前目录。
    pub(super) fn listed_rows(&self, ctx: &FileContext) -> Vec<&FileRow> {
        if ctx.smart_folder {
            self.virtual_rows.iter().collect()
        } else if ctx.category_virtual {
            self.category_rows.iter().collect()
        } else {
            self.rows.iter().collect()
        }
    }

    /// [`Self::listed_rows`] 的拥有副本，选择和菜单拿去用。
    pub(super) fn visible_rows(&self, ctx: &FileContext) -> Vec<FileRow> {
        self.listed_rows(ctx).into_iter().cloned().collect()
    }

    /// 当前列表里这条的虚拟标记和本机绝对路径。不在列表里时是 `None`。
    pub(crate) fn host_facts(
        &self,
        ctx: &FileContext,
        path: &str,
    ) -> Option<(bool, Option<String>)> {
        self.listed_rows(ctx)
            .into_iter()
            .find(|row| row.path == path)
            .map(|row| (row.is_virtual, row.local_absolute_path.clone()))
    }
}

/// 与 Vue `normalizeRepositoryRelativePath` 相同：去空白、反斜杠转正斜杠、去掉首尾斜杠。
pub(in crate::shell) fn normalize_path(path: &str) -> String {
    path.trim().replace('\\', "/").trim_matches('/').to_string()
}

impl FilesState {
    /// 列表上已经是这个目录（回收站和普通目录分开认）。还没有快照时不是。
    pub(crate) fn shows_directory(&self, path: &str, trash: bool) -> bool {
        self.shown_directory
            .as_ref()
            .is_some_and(|(shown, shown_trash)| {
                *shown_trash == trash && shown == &normalize_path(path)
            })
    }

    /// 当前列表的条数。静默重读同一层时用来不把已经续读出来的条目读少。
    pub(crate) fn listed_count(&self) -> usize {
        self.rows.len()
    }
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
#[path = "files_mount_tests.rs"]
mod mount_tests;
#[cfg(test)]
#[path = "files_rename_tests.rs"]
mod rename_tests;
#[cfg(test)]
#[path = "files_select_tests.rs"]
mod select_tests;
#[cfg(test)]
#[path = "files_ui_tests.rs"]
mod ui_tests;
#[cfg(test)]
#[path = "files_virtual_tests.rs"]
mod virtual_tests;
