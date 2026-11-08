//! 宿主输入：工作区拖放、外部打开和关闭确认。
//!
//! 移动、导入和附加复用文件与侧栏已有的服务请求。
//! 系统拖出、外部打开、目录揭示和托盘没有 Nana 窗口命令，只记录宿主请求。
//! 保存、打开和重定向文件夹对话框都排队为 `OpenFileDialog`，完成事件再写回状态。

use nana_ui::InputPayload;
use nana_ui_platform::WindowId;
use nana_ui_platform::host::WindowCommand;

use crate::host_api::{ExternalOpenRequest, HostInputRequest, HostRequest};

use super::{ShellMessage, ShellViewModel};

#[path = "input_reduce.rs"]
mod reduce;
#[path = "source_prompt.rs"]
mod source_prompt;
#[path = "input_support.rs"]
mod support;
#[path = "thumbnail_prompt.rs"]
mod thumbnail_prompt;
#[path = "input_view.rs"]
mod view;

pub(crate) use reduce::{begin_relocate_dialog, reduce_message};
pub(crate) use support::{CloseDecision, decide_close};
pub(crate) use view::close_prompt;

/// 用仓库根和条目相对路径拼绝对路径，供打开和定位使用。
pub(super) fn repository_absolute(root: &str, relative: &str) -> String {
    support::join_repository_path(root, relative, None)
}

/// 打开和定位要用的绝对路径，对照 Vue `resolveWorkspaceEntryPath`。
///
/// 有去掉空白后的本机路径就用它。否则虚拟条目，或当前仓库不是本地文件系统时，返回空串：
/// 不把仓库根拼到虚拟路径上，打开和定位都不发出。列表、播放集、播放队列和已经读回的素材详情
/// 按这个顺序认这条；都没有，而且仓库是本地文件系统时，才拼仓库根。
pub(crate) fn resolve_entry_absolute(model: &super::ShellViewModel, path: &str) -> String {
    let facts = entry_host_facts(model, path);
    if let Some(local) = facts
        .local
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return local.to_string();
    }
    let filesystem = model
        .workspace
        .active_repository()
        .is_some_and(repository_is_filesystem);
    if facts.virtual_entry || !filesystem {
        return String::new();
    }
    let root = model
        .workspace
        .active_repository()
        .map(|item| item.path.clone())
        .unwrap_or_default();
    repository_absolute(&root, path)
}

struct EntryHostFacts {
    virtual_entry: bool,
    local: Option<String>,
}

/// 列表优先，然后是点开的播放集、播放队列，最后是已经读回的素材详情。
fn entry_host_facts(model: &super::ShellViewModel, path: &str) -> EntryHostFacts {
    let ctx = super::files::FileContext::from_model(model);
    if let Some((virtual_entry, local)) = model.files.host_facts(&ctx, path) {
        return EntryHostFacts {
            virtual_entry,
            local,
        };
    }
    if let Some(item) = model
        .player
        .listed
        .as_ref()
        .and_then(|detail| detail.items.iter().find(|item| item.path == path))
    {
        return EntryHostFacts {
            virtual_entry: item.is_virtual,
            local: item.local_absolute_path.clone(),
        };
    }
    if let Some(item) = model.player.queue.iter().find(|item| item.path == path) {
        return EntryHostFacts {
            virtual_entry: item.is_virtual,
            local: item.local_absolute_path.clone(),
        };
    }
    if let Some((virtual_entry, local)) = model.inspect.noted_host_facts(path) {
        return EntryHostFacts {
            virtual_entry,
            local,
        };
    }
    EntryHostFacts {
        virtual_entry: false,
        local: None,
    }
}

/// 本地文件系统仓库才能把仓库根拼到相对路径上。来源仓库没有 `localRootPath`。
fn repository_is_filesystem(repository: &super::WorkspaceRepository) -> bool {
    let id = repository.backend_plugin_id.as_str();
    id == "filesystem"
        || id == "local"
        || id.contains("local-filesystem")
        || repository
            .capabilities
            .iter()
            .any(|item| item == "localRootPath")
}
pub(crate) use view::{FileDropFlags, accept_file_drops, file_drop_flags, file_drop_message};

#[cfg(test)]
#[path = "input_leave_tests.rs"]
mod leave_tests;
#[cfg(test)]
#[path = "input_open_tests.rs"]
mod open_tests;
#[cfg(test)]
#[path = "input_tests.rs"]
mod tests;

/// Tauri `onDragDropEvent` 的四个阶段。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HostDragPhase {
    Enter,
    Over,
    Leave,
    Drop,
}

/// 归约后要交给平台窗口的命令。窗口编号在 `update` 里补上。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PendingHostCommand {
    Close,
    OpenSaveDialog,
    OpenPluginDialog,
    OpenFolderDialog,
    OpenAttachDialog {
        eagle: bool,
    },
    OpenDownloadDialog,
    OpenThumbnailDialog,
    OpenSourceCacheDialog {
        plugin_name: String,
    },
    OpenEagleLibraryDialog,
    OpenRepositoryExportDialog {
        file_name: Option<String>,
        extension: String,
    },
    OpenImportFolderDialog,
    OpenImportArchiveDialog,
}

/// 工作区拖放、外部打开和关闭确认消息。
#[derive(Clone, Debug)]
pub enum InputMessage {
    BeginEntryDrag {
        path: String,
        selected: Vec<String>,
        x: f32,
        y: f32,
        writable: bool,
        trash: bool,
        smart_folder: bool,
        backend_kind: String,
        repo_root: String,
        hover_folder: Option<String>,
        over_browser: bool,
    },
    EntryDragMove {
        x: f32,
        y: f32,
        bounds_width: f32,
        bounds_height: f32,
        hover_folder: Option<String>,
        over_browser: bool,
    },
    EntryDragEnd {
        hover_folder: Option<String>,
        over_browser: bool,
        has_pointer: bool,
    },
    WindowPointerLeave {
        x: f32,
        y: f32,
    },
    WindowBlur,
    DragOver {
        internal_transfer: bool,
        writable: bool,
        files_panel: bool,
    },
    DragLeave {
        nested: bool,
    },
    BrowserDrop {
        internal_transfer: bool,
        paths: Vec<String>,
        writable: bool,
        trash: bool,
        has_snapshot: bool,
        repo_root: String,
    },
    FolderHover {
        path: String,
        trash: bool,
    },
    FolderLeave(String),
    FolderDrop {
        path: String,
        internal_transfer: bool,
        paths: Vec<String>,
    },
    /// 侧栏目录树收到的系统文件拖放。放下只在悬停正是树上的文件夹时导入。
    SidebarFileDrag {
        phase: HostDragPhase,
        paths: Vec<String>,
    },
    EmptyDragOver {
        has_active_id: bool,
        has_repository: bool,
    },
    EmptyDragLeave {
        nested: bool,
    },
    EmptyDrop {
        has_active_id: bool,
        has_repository: bool,
        paths: Vec<String>,
    },
    HostDrag {
        phase: HostDragPhase,
        paths: Vec<String>,
        has_repository: bool,
        missing_repository: bool,
        writable: bool,
        files_panel: bool,
        has_snapshot: bool,
        repo_root: String,
    },
    BoxSelect {
        paths: Vec<String>,
        append: bool,
    },
    /// 用系统程序打开条目。`path` 是仓库内相对路径，打开的同时照 Vue `openWorkspaceEntry` 记一次访问。
    OpenEntry {
        has_repo: bool,
        absolute_path: String,
        path: String,
    },
    RevealEntry {
        absolute_path: String,
    },
    OpenExternalUrl {
        url: String,
    },
    StartExternalDrag {
        paths: Vec<String>,
        trash: bool,
        backend_kind: String,
        repo_root: String,
    },
    ConfirmCloseAnswer(bool),
    FileDialogCompleted {
        request_id: u64,
        paths: Vec<String>,
        failed: Option<String>,
    },
    BeginDownloadFolder {
        plugin_id: String,
        method: String,
        payload: serde_json::Value,
        repository_id: Option<String>,
        progress: super::entry_actions::DownloadProgress,
    },
    BeginThumbnailFile {
        repo_id: String,
        path: String,
        kind: String,
    },
    PasteThumbnail {
        repo_id: String,
        path: String,
        kind: String,
    },
    ClearThumbnail {
        repo_id: String,
        path: String,
        kind: String,
    },
    ClearDrag,
}

#[derive(Clone, Debug)]
struct InternalSession {
    start_x: f32,
    start_y: f32,
    last_x: f32,
    last_y: f32,
    paths: Vec<String>,
    delegated: bool,
    repo_root: String,
    backend_kind: String,
}

/// 最近一次指针或按键事件按着的修饰键。运行时路由完输入以后由宿主记下（文档不保留修饰键），
/// 点选条目和框选照 Vue 的 `shiftKey`、`ctrlKey || metaKey` 读它。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HeldModifiers {
    pub shift: bool,
    /// Ctrl 或 Meta。
    pub command: bool,
}

impl HeldModifiers {
    /// Vue `selectionModeFromEvent`：Shift 是范围选择，Ctrl / Meta 是切换，否则替换。
    pub(crate) fn selection_mode(self) -> super::files::SelectionMode {
        if self.shift {
            super::files::SelectionMode::Range
        } else if self.command {
            super::files::SelectionMode::Toggle
        } else {
            super::files::SelectionMode::Replace
        }
    }
}

/// 和 `pointer_gesture` 一样只跟主键指针。别的指针离开窗口不打断这次拖动。
const PRIMARY_POINTER: u64 = 1;

/// Vue `useWorkspaceDragDrop` 的 `handleWindowPointerLeave`：鼠标离开窗口时，
/// 把内部拖动的最后位置交给已有的 `WindowPointerLeave`。拖得不够 72px 时那条归约什么都不做。
///
/// 离开事件不带坐标，运行时路由完又会清掉指针位置，所以用这次拖动记下的最后一点。
/// 没有内部拖动、或不是主键指针时不发。
pub(crate) fn pointer_leave_message(
    model: &ShellViewModel,
    payload: &InputPayload,
) -> Option<ShellMessage> {
    let InputPayload::PointerLeave { pointer_id } = payload else {
        return None;
    };
    if pointer_id.0 != PRIMARY_POINTER {
        return None;
    }
    if !model.input.internal_active {
        return None;
    }
    let Some(session) = model.input.session.as_ref() else {
        eprintln!("Nana 内部拖动没有会话，指针离开窗口无法交给系统拖放");
        return None;
    };
    if session.delegated {
        return None;
    }
    if !session.last_x.is_finite() || !session.last_y.is_finite() {
        eprintln!(
            "Nana 指针离开窗口时拖动位置无效：{}, {}",
            session.last_x, session.last_y
        );
        return None;
    }
    Some(ShellMessage::Input(InputMessage::WindowPointerLeave {
        x: session.last_x,
        y: session.last_y,
    }))
}

/// 实况树上还没松手的指针。`row` 为空时是框选。
#[derive(Clone, Debug)]
pub struct LiveGesture {
    pub row: Option<String>,
    pub origin_x: f32,
    pub origin_y: f32,
    pub x: f32,
    pub y: f32,
    pub armed: bool,
    /// 按下时 Ctrl 或 Meta 是否按着。文档不保留修饰键时为 false。
    pub append: bool,
}

/// 拖放会话、关闭确认和已排队的宿主命令。
#[derive(Clone, Debug, Default)]
pub struct InputState {
    pub internal_active: bool,
    pub external_active: bool,
    pub dragging_files: bool,
    pub dragging_repository_folder: bool,
    pub hover_folder: Option<String>,
    /// 这次悬停落在侧栏目录树上，不是文件列表里的同名目录。文件列离开时靠它留住目标。
    pub hover_on_sidebar: bool,
    pub dragged_paths: Vec<String>,
    pub drop_effect: String,
    pub empty_repository_error: String,
    /// 关闭确认框的文案。
    pub notice: String,
    pub pending_attach: bool,
    pub pending_close: bool,
    pub host_requests: Vec<HostRequest>,
    pub external_drag_result: Option<bool>,
    pub sidebar_dragging: bool,
    pub sidebar_resize_dirty: bool,
    pub live_gesture: Option<LiveGesture>,
    /// 最近一次输入按着的修饰键。
    pub modifiers: HeldModifiers,
    /// 模型画面上的指针拖拽：上一帧坐标。没有可旋转的网格时不记。
    pub model_drag: Option<(f32, f32)>,
    /// 列表上正在画的框选矩形。没在拖选框时藏着。
    pub(crate) selection_box: super::files_selection_box::SelectionBox,
    /// 下载菜单已经点过，正在等目录对话框。
    pub pending_download: Option<source_prompt::PendingDownload>,
    /// 自定义缩略图正在等选文件对话框。
    pub pending_thumbnail: Option<thumbnail_prompt::PendingThumbnail>,
    /// 已排队的 EagleLibrary 对话框还要不要接着导入。和对话框一一对应，先排队的先完成。
    /// `None` 只写入路径；`Some` 是文件菜单选中目录后要导入的模式。
    eagle_dialog_followup: Vec<Option<String>>,
    session: Option<InternalSession>,
    host_commands: Vec<PendingHostCommand>,
}

impl InputState {
    /// 按关闭设置记录确认、托盘或真正关闭。确认框是浮层里的 `ConfirmDialog`，只看 `pending_close`
    /// 和文案，不排宿主请求；已经在问时再关不重复问。
    pub(crate) fn apply_close(&mut self, decision: CloseDecision, dirty: bool) {
        match decision {
            CloseDecision::CloseNow => {
                self.pending_close = false;
                self.notice.clear();
                self.host_commands.push(PendingHostCommand::Close);
            }
            CloseDecision::Ask { .. } => {
                if self.pending_close {
                    return;
                }
                self.pending_close = true;
                self.notice = if dirty {
                    "有未保存的修改，确认关闭？".into()
                } else {
                    "确认关闭 MomoBako？".into()
                };
            }
            CloseDecision::HoldForTray => {
                self.pending_close = false;
                self.notice.clear();
                self.host_requests
                    .push(HostRequest::Input(HostInputRequest::MinimizeToTray));
            }
        }
    }

    /// 用户回答关闭确认。没有待确认时不关闭。
    pub(crate) fn answer_close(&mut self, accept: bool) {
        if !self.pending_close {
            eprintln!("Nana 没有待确认的关闭");
            return;
        }
        self.pending_close = false;
        self.notice.clear();
        if accept {
            self.host_commands.push(PendingHostCommand::Close);
        }
    }

    pub(crate) fn queue_save_dialog(&mut self) {
        self.host_commands.push(PendingHostCommand::OpenSaveDialog);
    }

    pub(crate) fn queue_plugin_dialog(&mut self) {
        self.host_commands
            .push(PendingHostCommand::OpenPluginDialog);
    }

    /// 排队重定向用的文件夹对话框。平台稍后把它换成 `OpenFileDialog`。
    pub(crate) fn queue_folder_dialog(&mut self) {
        self.host_commands
            .push(PendingHostCommand::OpenFolderDialog);
    }

    /// 排队添加资源库用的文件夹对话框。
    ///
    /// `eagle` 为真时标题是「选择 Eagle Library 目录」；本地文件夹用「选择资源库文件夹」。
    pub(crate) fn queue_attach_dialog(&mut self, eagle: bool) {
        self.host_commands
            .push(PendingHostCommand::OpenAttachDialog { eagle });
    }

    /// 排队下载目标目录对话框。取消时不调用插件。
    pub(crate) fn queue_download_dialog(&mut self) {
        self.host_commands
            .push(PendingHostCommand::OpenDownloadDialog);
    }

    /// 排队自定义缩略图的选文件对话框。
    pub(crate) fn queue_thumbnail_dialog(&mut self) {
        self.host_commands
            .push(PendingHostCommand::OpenThumbnailDialog);
    }

    /// 排队来源缓存目录对话框。标题用点下去时的插件显示名。取消时不改认证页草稿。
    pub(crate) fn queue_source_cache_dialog(&mut self, plugin_name: String) {
        self.host_commands
            .push(PendingHostCommand::OpenSourceCacheDialog { plugin_name });
    }

    /// 排队 Eagle 导入页的库目录对话框。取消时不改页面上的路径，选中后也不导入。
    pub(crate) fn queue_eagle_library_dialog(&mut self) {
        self.queue_eagle_library(None);
    }

    /// 文件菜单的复制或剪切：同一套选目录对话框，选中后再按模式导入。
    pub(crate) fn queue_eagle_menu_import(&mut self, mode: String) {
        self.queue_eagle_library(Some(mode));
    }

    fn queue_eagle_library(&mut self, followup: Option<String>) {
        self.eagle_dialog_followup.push(followup);
        self.host_commands
            .push(PendingHostCommand::OpenEagleLibraryDialog);
    }

    /// 取出最早一个 EagleLibrary 对话框的后续导入模式。没有排队记录时只当选择目录。
    pub(crate) fn take_eagle_dialog_followup(&mut self) -> Option<String> {
        if self.eagle_dialog_followup.is_empty() {
            None
        } else {
            self.eagle_dialog_followup.remove(0)
        }
    }

    /// 排队实况「从文件夹导入」的系统选目录。取消或空白不导入。
    pub(crate) fn queue_import_folder_dialog(&mut self) {
        self.host_commands
            .push(PendingHostCommand::OpenImportFolderDialog);
    }

    /// 排队实况「从 ZIP 导入」的系统选文件。取消或空白不导入。
    pub(crate) fn queue_import_archive_dialog(&mut self) {
        self.host_commands
            .push(PendingHostCommand::OpenImportArchiveDialog);
    }

    /// 排队资源库压缩包的保存对话框。取消时不调用导出协议。
    pub(crate) fn queue_repository_export_dialog(
        &mut self,
        file_name: Option<String>,
        extension: String,
    ) {
        self.host_commands
            .push(PendingHostCommand::OpenRepositoryExportDialog {
                file_name,
                extension,
            });
    }

    /// 把排队命令换成当前窗口的平台命令。
    pub(crate) fn take_platform_commands(
        &mut self,
        id: WindowId,
        maximized: bool,
    ) -> Vec<WindowCommand> {
        let pending = std::mem::take(&mut self.host_commands);
        pending
            .into_iter()
            .filter_map(|command| match command {
                PendingHostCommand::Close => {
                    Some(crate::host_api::WindowCommand::Close.to_platform_command(id, maximized))
                }
                PendingHostCommand::OpenSaveDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::save_export_request(),
                }),
                PendingHostCommand::OpenPluginDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::open_plugin_request(),
                }),
                PendingHostCommand::OpenFolderDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::pick_folder_request(),
                }),
                PendingHostCommand::OpenAttachDialog { eagle } => {
                    Some(WindowCommand::OpenFileDialog {
                        id,
                        request: support::attach_folder_request(eagle),
                    })
                }
                PendingHostCommand::OpenDownloadDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::download_folder_request(),
                }),
                PendingHostCommand::OpenThumbnailDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::thumbnail_file_request(),
                }),
                PendingHostCommand::OpenSourceCacheDialog { plugin_name } => {
                    Some(WindowCommand::OpenFileDialog {
                        id,
                        request: support::source_cache_folder_request(&plugin_name),
                    })
                }
                PendingHostCommand::OpenEagleLibraryDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::eagle_library_folder_request(),
                }),
                PendingHostCommand::OpenRepositoryExportDialog {
                    file_name,
                    extension,
                } => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::repository_export_save_request(file_name, &extension),
                }),
                PendingHostCommand::OpenImportFolderDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::import_folder_request(),
                }),
                PendingHostCommand::OpenImportArchiveDialog => {
                    Some(WindowCommand::OpenFileDialog {
                        id,
                        request: support::import_archive_request(),
                    })
                }
            })
            .collect()
    }

    fn finish_internal(&mut self) {
        self.session = None;
        self.internal_active = false;
        self.dragged_paths.clear();
        self.hover_folder = None;
        self.hover_on_sidebar = false;
    }

    fn clear_flags(&mut self) {
        self.finish_internal();
        self.external_active = false;
        self.dragging_files = false;
        self.dragging_repository_folder = false;
        self.drop_effect.clear();
    }

    fn remember_external(&mut self, target: &str, reveal: bool) {
        self.host_requests
            .push(HostRequest::OpenExternal(ExternalOpenRequest {
                target: target.to_string(),
                reveal,
            }));
    }
}

impl ShellViewModel {
    pub(crate) fn close_is_dirty(&self) -> bool {
        self.dirty || self.inspect.dirty()
    }
}
