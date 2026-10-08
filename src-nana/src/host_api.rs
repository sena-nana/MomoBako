//! Nana 宿主与应用服务之间的无窗口依赖契约。
//!
//! 这些请求由 Runtime/ViewModel 产生，Windows 窗口、文件对话框、托盘和外部打开
//! 分别由具体宿主实现；因此 Nana 页面不需要取得 Tauri `AppHandle` 或窗口对象。

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::backend::services::repository::PlaybackSessionState;

/// 播放器插件只提供媒体能力，播放会话状态和生命周期由宿主控制器拥有。
pub trait PlaybackMediaPlugin {
    fn load(&mut self, source: &str) -> Result<PlaybackMediaCapabilities, String>;
    fn play(&mut self) -> Result<(), String>;
    fn pause(&mut self) -> Result<(), String>;
    fn seek(&mut self, position_ms: u64) -> Result<(), String>;
    fn set_volume(&mut self, volume: f32) -> Result<(), String>;
    fn dispose(&mut self);
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PlaybackMediaCapabilities {
    pub duration_ms: Option<u64>,
    pub can_seek: bool,
    pub can_volume: bool,
}

/// 单一 Runtime 播放会话控制器，所有失败都会进入可解释的 failed 状态。
pub struct PlaybackSessionController<P> {
    plugin: P,
    state: PlaybackSessionState,
}

impl<P: PlaybackMediaPlugin> PlaybackSessionController<P> {
    pub fn new(plugin: P, state: PlaybackSessionState) -> Self {
        Self { plugin, state }
    }

    pub fn state(&self) -> &PlaybackSessionState {
        &self.state
    }

    pub fn load(&mut self, source: &str) -> Result<(), String> {
        match self.plugin.load(source) {
            Ok(capabilities) => {
                self.state.duration_ms = capabilities.duration_ms;
                self.state.can_seek = capabilities.can_seek;
                self.state.can_volume = capabilities.can_volume;
                self.state.status = "paused".into();
                self.state.error = None;
                Ok(())
            }
            Err(error) => self.fail(error),
        }
    }

    pub fn play(&mut self) -> Result<(), String> {
        self.plugin.play().map_err(|error| self.fail_message(error))?;
        self.state.status = "playing".into();
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.plugin.pause().map_err(|error| self.fail_message(error))?;
        self.state.status = "paused".into();
        Ok(())
    }

    pub fn seek(&mut self, position_ms: u64) -> Result<(), String> {
        if !self.state.can_seek {
            return self.fail("当前播放器不支持进度控制".into());
        }
        self.plugin.seek(position_ms).map_err(|error| self.fail_message(error))?;
        self.state.current_time_ms = position_ms;
        Ok(())
    }

    pub fn set_volume(&mut self, volume: f32) -> Result<(), String> {
        if !self.state.can_volume {
            return self.fail("当前播放器不支持音量控制".into());
        }
        let volume = volume.clamp(0.0, 1.0);
        self.plugin.set_volume(volume).map_err(|error| self.fail_message(error))?;
        self.state.volume = volume;
        Ok(())
    }

    pub fn dispose(&mut self) {
        self.plugin.dispose();
        self.state.status = "ended".into();
    }

    fn fail<T>(&mut self, error: String) -> Result<T, String> {
        self.state.status = "failed".into();
        self.state.error = Some(error.clone());
        Err(error)
    }

    fn fail_message(&mut self, error: String) -> String {
        self.state.status = "failed".into();
        self.state.error = Some(error.clone());
        error
    }
}

/// 窗口生命周期与显示状态命令。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WindowCommand {
    Restore,
    Minimize,
    ToggleMaximize,
    Close,
    SetTitle(String),
}

impl WindowCommand {
    /// 将宿主无关窗口动作映射为 Nana 平台命令；窗口对象仍只停留在适配层。
    pub fn to_platform_command(
        &self,
        id: nana_ui_platform::WindowId,
        maximized: bool,
    ) -> Option<nana_ui_platform::host::WindowCommand> {
        match self {
            Self::Restore => Some(nana_ui_platform::host::WindowCommand::SetMinimized { id, minimized: false }),
            Self::Minimize => Some(nana_ui_platform::host::WindowCommand::SetMinimized { id, minimized: true }),
            Self::ToggleMaximize => Some(nana_ui_platform::host::WindowCommand::SetMaximized { id, maximized: !maximized }),
            Self::Close => Some(nana_ui_platform::host::WindowCommand::Close(id)),
            Self::SetTitle(_) => None,
        }
    }
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
    /// 把库内绝对路径拖出窗口。Windows 上由宿主调用系统拖放。
    DragOut { paths: Vec<String> },
    ConfirmClose { dirty: bool },
    /// 关闭行为是最小化到托盘。宿主隐藏窗口，进程继续留在托盘里。
    MinimizeToTray,
    FocusMainWindow,
    RegisterShortcut { accelerator: String },
}

/// 请求宿主打开外部资源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalOpenRequest {
    pub target: String,
    /// 为真时在文件管理器中定位，否则用系统默认程序打开。
    pub reveal: bool,
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

    struct MockPlayer {
        fail_play: bool,
        volume: f32,
    }

    impl PlaybackMediaPlugin for MockPlayer {
        fn load(&mut self, _: &str) -> Result<PlaybackMediaCapabilities, String> {
            Ok(PlaybackMediaCapabilities {
                duration_ms: Some(120_000),
                can_seek: true,
                can_volume: true,
            })
        }

        fn play(&mut self) -> Result<(), String> {
            if self.fail_play { Err("播放器插件不可用".into()) } else { Ok(()) }
        }

        fn pause(&mut self) -> Result<(), String> { Ok(()) }

        fn seek(&mut self, _: u64) -> Result<(), String> { Ok(()) }

        fn set_volume(&mut self, volume: f32) -> Result<(), String> {
            self.volume = volume;
            Ok(())
        }

        fn dispose(&mut self) {}
    }

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

    #[test]
    fn playback_controller_owns_state_and_records_plugin_failure() {
        let state = PlaybackSessionState {
            session_id: "session-1".into(),
            repo_id: "repo-1".into(),
            playlist_id: "playlist-1".into(),
            playlist_item_id: Some("item-1".into()),
            status: "idle".into(),
            current_time_ms: 0,
            duration_ms: None,
            volume: 1.0,
            can_seek: false,
            can_volume: false,
            error: None,
            updated_at: "now".into(),
        };
        let mut controller = PlaybackSessionController::new(
            MockPlayer { fail_play: true, volume: 1.0 },
            state,
        );
        controller.load("track.mp3").expect("load should expose plugin capabilities");
        assert_eq!(controller.state().status, "paused");
        assert!(controller.play().is_err());
        assert_eq!(controller.state().status, "failed");
        assert_eq!(controller.state().error.as_deref(), Some("播放器插件不可用"));
    }
}
