//! 把标题栏点击产生的壳层消息落到 ViewModel，并给出应排队的窗口命令。
//!
//! 最小化和最大化切换只排队宿主命令。关闭不在这里直接关掉窗口，而是交给
//! 现有的确认、未保存和托盘策略。

use nana_ui_platform::host::WindowCommand as PlatformWindowCommand;
use nana_ui_platform::WindowId;

use super::{ShellMessage, ShellViewModel, WindowAction};

/// 最小化和最大化切换对应的平台命令。关闭返回空，让调用方继续走确认策略。
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
    Some(request.to_platform_command(id, maximized).into_iter().collect())
}

/// 应用一次运行时点击排队的消息。
///
/// 与 `MomoBakoApplication::update` 共用窗口命令映射。关闭会写入确认文案，
/// 不发出 `WindowCommand::Close`。
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
