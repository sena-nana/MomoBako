//! Nana 宿主与应用服务之间的无窗口依赖契约。
//!
//! ViewModel 把外部打开、文件拖出和最小化到托盘排成 `HostRequest`，由窗口宿主（`host_bridge.rs`）
//! 执行；标题栏的窗口动作经 `WindowCommand` 映射成平台窗口命令。文件对话框直接排 NanaUI 平台命令
//! （`shell/input.rs`），不经这里。页面因此不需要取得窗口对象。播放器的媒体控制和播放会话控制器
//! 也在这里。

use crate::backend::services::repository::PlaybackSessionState;

/// 播放器只提供媒体控制，播放会话状态由宿主控制器拥有。装载（经仓库服务读字节、在任务里解码）和卸下
/// （停播放、清会话）由播放器状态机自己做（`shell/player_clip.rs`、`shell/player.rs`），不经这里。
pub trait PlaybackMediaPlugin {
    fn play(&mut self) -> Result<(), String>;
    fn pause(&mut self) -> Result<(), String>;
    fn seek(&mut self, position_ms: u64) -> Result<(), String>;
    fn set_volume(&mut self, volume: f32) -> Result<(), String>;
}

/// 单一 Runtime 播放会话控制器：播放条和系统媒体键的播放、暂停、跳转和音量都经它落到播放器上
/// （`shell/wav_player.rs` 的 `drive`），所有失败都会进入可解释的 failed 状态。
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

/// 标题栏和关窗策略发出的窗口动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WindowCommand {
    Minimize,
    ToggleMaximize,
    Close,
}

impl WindowCommand {
    /// 将宿主无关窗口动作映射为 Nana 平台命令；窗口对象仍只停留在适配层。
    pub fn to_platform_command(
        self,
        id: nana_ui_platform::WindowId,
        maximized: bool,
    ) -> nana_ui_platform::host::WindowCommand {
        match self {
            Self::Minimize => nana_ui_platform::host::WindowCommand::SetMinimized { id, minimized: true },
            Self::ToggleMaximize => nana_ui_platform::host::WindowCommand::SetMaximized { id, maximized: !maximized },
            Self::Close => nana_ui_platform::host::WindowCommand::Close(id),
        }
    }
}

/// 交给宿主执行的输入请求。关闭确认是浮层里的确认框，由 `InputState::pending_close` 驱动，不经宿主。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostInputRequest {
    /// 把库内绝对路径拖出窗口。Windows 上由宿主调用系统拖放。
    DragOut { paths: Vec<String> },
    /// 关闭行为是最小化到托盘。宿主隐藏窗口，进程继续留在托盘里。
    MinimizeToTray,
}

/// 网址能不能交给系统打开：只认 `http`、`https`、`mailto`、`tel` 四种协议。
///
/// 和 Vue 实际能打开的范围一致：Vue 走 Tauri opener 插件的 `opener:default`，默认范围
/// （`allow-default-urls`）只放行这四种。其余协议一律不开，例如 `file:`、`C:\…` 这样的盘符路径、
/// 自定义协议，以及 `javascript:`、`data:`、`vbscript:`，免得元数据里的链接直接启动本地程序。
pub fn openable_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.trim().split_once(':') else {
        return false;
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => rest.starts_with("//") && rest.len() > 2,
        "mailto" | "tel" => !rest.trim().is_empty(),
        _ => false,
    }
}

/// 请求宿主打开外部资源。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalOpenRequest {
    pub target: String,
    /// 为真时在文件管理器中定位，否则用系统默认程序打开。
    pub reveal: bool,
}

/// 应用向宿主发出的请求。宿主每次 `update` 后全部执行并清空（`host_bridge::perform`）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HostRequest {
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
        fn play(&mut self) -> Result<(), String> {
            if self.fail_play { Err("播放器插件不可用".into()) } else { Ok(()) }
        }

        fn pause(&mut self) -> Result<(), String> { Ok(()) }

        fn seek(&mut self, _: u64) -> Result<(), String> { Ok(()) }

        fn set_volume(&mut self, volume: f32) -> Result<(), String> {
            self.volume = volume;
            Ok(())
        }
    }

    fn paused(can_seek: bool) -> PlaybackSessionState {
        PlaybackSessionState {
            session_id: "session-1".into(),
            repo_id: "repo-1".into(),
            playlist_id: "playlist-1".into(),
            playlist_item_id: Some("item-1".into()),
            status: "paused".into(),
            current_time_ms: 0,
            duration_ms: Some(120_000),
            volume: 1.0,
            can_seek,
            can_volume: true,
            error: None,
            updated_at: "now".into(),
        }
    }

    /// 装好的条目：控制成功时控制器改会话，播放器报错或不支持的控制都进 failed 并留下原因。
    #[test]
    fn playback_controller_owns_state_and_records_plugin_failure() {
        let mut controller = PlaybackSessionController::new(MockPlayer { fail_play: false, volume: 1.0 }, paused(true));
        controller.seek(30_000).expect("能跳转");
        controller.set_volume(1.5).expect("能调音量");
        controller.play().expect("能播放");
        assert_eq!((controller.state().status.as_str(), controller.state().current_time_ms, controller.state().volume), ("playing", 30_000, 1.0));

        let mut failing = PlaybackSessionController::new(MockPlayer { fail_play: true, volume: 1.0 }, paused(false));
        assert!(failing.play().is_err());
        assert_eq!(failing.state().status, "failed");
        assert_eq!(failing.state().error.as_deref(), Some("播放器插件不可用"));
        assert!(failing.seek(1_000).is_err());
        assert_eq!(failing.state().error.as_deref(), Some("当前播放器不支持进度控制"));
    }

    /// 只开 http、https、mailto、tel；盘符路径、`file:`、自定义协议和脚本协议都不开。
    #[test]
    fn only_web_mail_and_phone_links_are_openable() {
        for url in ["https://example.com/cover", "HTTP://example.com", " https://x/?a=1&calc ", "mailto:a@b.c", "tel:+86123"] {
            assert!(super::openable_url(url), "{url} 应该能打开");
        }
        for url in [
            "C:\\Windows\\System32\\calc.exe",
            "file:///C:/Windows/System32/calc.exe",
            "eagle://item/1",
            "javascript:alert(1)",
            "DATA:text/html,x",
            "vbscript:x",
            "example.com",
            "https:",
            "https:example.com",
            "mailto:",
            "",
        ] {
            assert!(!super::openable_url(url), "{url} 不该交给系统打开");
        }
    }
}
