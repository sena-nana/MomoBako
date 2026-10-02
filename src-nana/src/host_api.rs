//! Nana 宿主与应用服务之间的无窗口依赖契约。
//!
//! 这些请求由 Runtime/ViewModel 产生，Windows 窗口、文件对话框、托盘和外部打开
//! 分别由具体宿主实现；因此 Nana 页面不需要取得 Tauri `AppHandle` 或窗口对象。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// 窗口生命周期与显示状态命令。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowCommand {
    Restore,
    Minimize,
    ToggleMaximize,
    Close,
    SetTitle(String),
}

/// 宿主通知，保持安静的默认级别，危险和失败状态才提高可见度。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub is_error: bool,
}

/// 文件对话框请求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileDialogRequest {
    OpenFile { extensions: Vec<String> },
    OpenDirectory,
    SaveFile { suggested_name: String },
}

/// Host-provided file drop payload. Paths are validated by the host before dispatch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileDropRequest {
    pub paths: Vec<String>,
}

/// Native input and close-confirmation requests kept outside the Runtime widget tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostInputRequest {
    FileDrop(FileDropRequest),
    ConfirmClose { dirty: bool },
    FocusMainWindow,
    RegisterShortcut { accelerator: String },
}

/// 请求宿主打开外部资源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalOpenRequest {
    pub target: String,
}

/// 可取消的长任务探针，服务层和 Nana 宿主共享同一个原子状态。
#[derive(Clone, Debug, Default)]
pub struct CancellationProbe(Arc<AtomicBool>);

impl CancellationProbe {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// 应用向宿主发出的最小请求集合。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostRequest {
    Window(WindowCommand),
    Notify(Notification),
    OpenFileDialog(FileDialogRequest),
    OpenExternal(ExternalOpenRequest),
    Input(HostInputRequest),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_probe_is_shared_and_monotonic() {
        let first = CancellationProbe::default();
        let second = first.clone();
        assert!(!first.is_cancelled());
        second.cancel();
        assert!(first.is_cancelled());
    }

    #[test]
    fn host_request_keeps_window_and_dialog_payloads() {
        let request = HostRequest::OpenFileDialog(FileDialogRequest::OpenFile {
            extensions: vec!["png".into(), "jpg".into()],
        });
        assert_eq!(
            request,
            HostRequest::OpenFileDialog(FileDialogRequest::OpenFile {
                extensions: vec!["png".into(), "jpg".into()],
            })
        );
    }

    #[test]
    fn host_input_keeps_drop_paths_and_close_state() {
        assert_eq!(
            HostRequest::Input(HostInputRequest::FileDrop(FileDropRequest {
                paths: vec!["C:/assets/a.png".into()],
            })),
            HostRequest::Input(HostInputRequest::FileDrop(FileDropRequest {
                paths: vec!["C:/assets/a.png".into()],
            }))
        );
        assert_eq!(
            HostInputRequest::ConfirmClose { dirty: true },
            HostInputRequest::ConfirmClose { dirty: true }
        );
    }
}
