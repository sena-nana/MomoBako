//! 把标题栏和实况点击产生的壳层消息落到 ViewModel，并给出要交给宿主的平台窗口命令。
//!
//! 最小化和最大化切换直接换成平台窗口命令。关闭不在这里直接关掉窗口，而是按关闭设置走
//! （`input::decide_close`）：要确认时弹关闭确认，收进托盘时交给宿主，直接关闭时才排关窗命令。

use nana_ui_platform::host::WindowCommand as PlatformWindowCommand;
use nana_ui_platform::WindowId;

use super::{ShellMessage, ShellViewModel, WindowAction};

/// 最小化和最大化切换对应的平台命令。关闭和别的消息返回 `None`，让调用方继续归约（关闭按关闭设置走）。
pub(crate) fn window_action_commands(
    message: &ShellMessage,
    id: WindowId,
    maximized: bool,
) -> Option<Vec<PlatformWindowCommand>> {
    let ShellMessage::WindowAction(action) = message else {
        return None;
    };
    let request = match action {
        WindowAction::Minimize => crate::host_api::WindowCommand::Minimize,
        WindowAction::ToggleMaximize => crate::host_api::WindowCommand::ToggleMaximize,
        WindowAction::Close => return None,
    };
    Some(vec![request.to_platform_command(id, maximized)])
}

/// 应用一次运行时点击排队的消息，返回要交给宿主的平台窗口命令。测试用它代替宿主的更新循环。
///
/// 与 `MomoBakoApplication::update` 共用窗口命令映射。关闭要确认时只弹关闭确认、不排关窗命令，
/// 确认以后才排。
pub fn commit_interaction(
    model: &mut ShellViewModel,
    message: ShellMessage,
    id: WindowId,
    maximized: bool,
) -> Vec<PlatformWindowCommand> {
    if let Some(commands) = window_action_commands(&message, id, maximized) {
        return commands;
    }
    model.reduce(message);
    model.input.take_platform_commands(id, maximized)
}
