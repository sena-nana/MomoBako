//! 宿主输入：工作区拖放、外部打开和关闭确认。
//!
//! 移动、导入和附加复用文件与侧栏已有的服务请求。
//! 系统拖出、外部打开、目录揭示和托盘没有 Nana 窗口命令，只记录宿主请求。
//! 保存、打开和重定向文件夹对话框都排队为 `OpenFileDialog`，完成事件再写回状态。

use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::WindowId;

use crate::host_api::{ExternalOpenRequest, HostInputRequest, HostRequest};

use super::ShellViewModel;

#[path = "input_support.rs"]
mod support;
#[path = "input_reduce.rs"]
mod reduce;
#[path = "input_view.rs"]
mod view;
#[path = "source_prompt.rs"]
mod source_prompt;
#[path = "thumbnail_prompt.rs"]
mod thumbnail_prompt;

pub(crate) use source_prompt::playlist_name_dialog;

pub(crate) use reduce::{begin_relocate_dialog, reduce_message};
pub(crate) use support::{decide_close, CloseDecision};
pub(crate) use view::close_prompt;

/// 用仓库根和条目相对路径拼绝对路径，供打开和定位使用。
pub(super) fn repository_absolute(root: &str, relative: &str) -> String {
    support::join_repository_path(root, relative, None)
}
pub(crate) use view::{accept_file_drops, file_drop_flags, file_drop_message, FileDropFlags};

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
    OpenAttachDialog,
    OpenDownloadDialog,
    OpenThumbnailDialog,
    OpenSourceCacheDialog,
    OpenRepositoryExportDialog { file_name: Option<String>, extension: String },
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
    WindowPointerLeave { x: f32, y: f32 },
    WindowBlur,
    DragOver { internal_transfer: bool, writable: bool, files_panel: bool },
    DragLeave { nested: bool },
    BrowserDrop {
        internal_transfer: bool,
        paths: Vec<String>,
        writable: bool,
        trash: bool,
        has_snapshot: bool,
        repo_root: String,
    },
    FolderHover { path: String, trash: bool },
    FolderLeave(String),
    FolderDrop { path: String, internal_transfer: bool, paths: Vec<String> },
    EmptyDragOver { has_active_id: bool, has_repository: bool },
    EmptyDragLeave { nested: bool },
    EmptyDrop { has_active_id: bool, has_repository: bool, paths: Vec<String> },
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
    BoxSelect { paths: Vec<String>, append: bool },
    OpenEntry { has_repo: bool, absolute_path: String },
    RevealEntry { absolute_path: String },
    OpenExternalUrl { url: String },
    StartExternalDrag { paths: Vec<String>, trash: bool, backend_kind: String, repo_root: String },
    ConfirmCloseAnswer(bool),
    FileDialogCompleted { request_id: u64, paths: Vec<String>, failed: Option<String> },
    BeginDownloadFolder {
        plugin_id: String,
        method: String,
        payload: serde_json::Value,
        repository_id: Option<String>,
    },
    OpenSourcePlaylist {
        plugin_id: String,
        method: String,
        payload: serde_json::Value,
        repository_id: Option<String>,
    },
    SourcePlaylistDraft(String),
    SubmitSourcePlaylist,
    CloseSourcePlaylist,
    BeginThumbnailFile { repo_id: String, path: String, kind: String },
    PasteThumbnail { repo_id: String, path: String, kind: String },
    ClearThumbnail { repo_id: String, path: String, kind: String },
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
    /// 下载菜单已经点过，正在等目录对话框。
    pub pending_download: Option<source_prompt::PendingDownload>,
    /// 来源播放列表正在问名称。
    pub source_playlist: Option<source_prompt::SourcePlaylistPrompt>,
    /// 自定义缩略图正在等选文件对话框。
    pub pending_thumbnail: Option<thumbnail_prompt::PendingThumbnail>,
    session: Option<InternalSession>,
    host_commands: Vec<PendingHostCommand>,
}

impl InputState {
    /// 按关闭设置记录确认、托盘或真正关闭。重复确认不叠加请求。
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
                self.host_requests.push(HostRequest::Input(HostInputRequest::ConfirmClose { dirty }));
            }
            CloseDecision::HoldForTray => {
                self.pending_close = false;
                self.notice.clear();
                self.host_requests.push(HostRequest::Input(HostInputRequest::MinimizeToTray));
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
        self.host_commands.push(PendingHostCommand::OpenPluginDialog);
    }

    /// 排队重定向用的文件夹对话框。平台稍后把它换成 `OpenFileDialog`。
    pub(crate) fn queue_folder_dialog(&mut self) {
        self.host_commands.push(PendingHostCommand::OpenFolderDialog);
    }

    /// 排队添加资源库用的文件夹对话框。
    pub(crate) fn queue_attach_dialog(&mut self) {
        self.host_commands.push(PendingHostCommand::OpenAttachDialog);
    }

    /// 排队下载目标目录对话框。取消时不调用插件。
    pub(crate) fn queue_download_dialog(&mut self) {
        self.host_commands.push(PendingHostCommand::OpenDownloadDialog);
    }

    /// 排队自定义缩略图的选文件对话框。
    pub(crate) fn queue_thumbnail_dialog(&mut self) {
        self.host_commands.push(PendingHostCommand::OpenThumbnailDialog);
    }

    /// 排队来源缓存目录对话框。取消时不改认证页草稿。
    pub(crate) fn queue_source_cache_dialog(&mut self) {
        self.host_commands.push(PendingHostCommand::OpenSourceCacheDialog);
    }

    /// 排队资源库压缩包的保存对话框。取消时不调用导出协议。
    pub(crate) fn queue_repository_export_dialog(&mut self, file_name: Option<String>, extension: String) {
        self.host_commands.push(PendingHostCommand::OpenRepositoryExportDialog { file_name, extension });
    }

    /// 把排队命令换成当前窗口的平台命令。
    pub(crate) fn take_platform_commands(&mut self, id: WindowId, maximized: bool) -> Vec<WindowCommand> {
        let pending = std::mem::take(&mut self.host_commands);
        pending
            .into_iter()
            .filter_map(|command| match command {
                PendingHostCommand::Close => {
                    crate::host_api::WindowCommand::Close.to_platform_command(id, maximized)
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
                PendingHostCommand::OpenAttachDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::attach_folder_request(),
                }),
                PendingHostCommand::OpenDownloadDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::download_folder_request(),
                }),
                PendingHostCommand::OpenThumbnailDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::thumbnail_file_request(),
                }),
                PendingHostCommand::OpenSourceCacheDialog => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::source_cache_folder_request(),
                }),
                PendingHostCommand::OpenRepositoryExportDialog { file_name, extension } => Some(WindowCommand::OpenFileDialog {
                    id,
                    request: support::repository_export_save_request(file_name, &extension),
                }),
            })
            .collect()
    }

    fn finish_internal(&mut self) {
        self.session = None;
        self.internal_active = false;
        self.dragged_paths.clear();
        self.hover_folder = None;
    }

    fn clear_flags(&mut self) {
        self.finish_internal();
        self.external_active = false;
        self.dragging_files = false;
        self.dragging_repository_folder = false;
        self.drop_effect.clear();
    }

    fn remember_external(&mut self, target: &str, reveal: bool) {
        self.host_requests.push(HostRequest::OpenExternal(ExternalOpenRequest {
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
