//! 系统托盘，用 NanaUI 的 `install_tray`。
//!
//! 提示 MomoBako，左键显示窗口，菜单是「打开 MomoBako」和「退出」。Windows 上是通知区图标，macOS 上是
//! 菜单栏图标；其它平台没有托盘，装不上时最小化到托盘按失败报出，不假装窗口已经收起来。
//! 选中的菜单项经 NanaUI 的菜单激活队列取出，转成 [`ShellMessage::Tray`] 投递回事件循环，由宿主在
//! `update` 里显示或关闭窗口（`window_host::on_tray`）。退出走窗口关闭，最后一扇窗口关掉后进程结束。

use nana_ui::{MenuEntry, RuntimeProgramContext, Tray, TraySupport};

use crate::shell::ShellMessage;

/// 「打开 MomoBako」，也是左键点图标时报的 id。
const TRAY_OPEN: u32 = 0x4d42_0001;
/// 「退出」。
const TRAY_QUIT: u32 = 0x4d42_0002;
const TOOLTIP: &str = "MomoBako";

/// 托盘菜单选中的动作。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    Show,
    Quit,
}

/// 菜单 id 对应的动作。别的 id（不是托盘的菜单项）忽略。
fn menu_action(id: u32) -> Option<TrayAction> {
    match id {
        TRAY_OPEN => Some(TrayAction::Show),
        TRAY_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

fn tray() -> Tray {
    Tray {
        tooltip: TOOLTIP.into(),
        icon_png: crate::app_icon::SMALL_PNG.to_vec(),
        template_icon: false,
        menu: vec![
            MenuEntry::item(TRAY_OPEN, "打开 MomoBako"),
            MenuEntry::item(TRAY_QUIT, "退出"),
        ],
        primary: Some(TRAY_OPEN),
    }
}

/// 建立托盘。选中的菜单项转成壳层消息投递回事件循环。平台没有托盘或图标建不起来时返回原因。
pub fn install(context: &RuntimeProgramContext<ShellMessage>) -> Result<(), String> {
    let events = std::sync::Mutex::new(context.clone());
    let support = nana_ui::install_tray(&tray(), move || {
        let Ok(events) = events.lock() else {
            eprintln!("Nana 托盘消息通道锁已损坏");
            return;
        };
        for id in nana_ui::take_menu_activations() {
            if let Some(action) = menu_action(id) {
                events.dispatch(ShellMessage::Tray(action));
            }
        }
    });
    match support {
        TraySupport::Unavailable => Err("当前平台没有系统托盘，或托盘图标没有建起来".into()),
        TraySupport::NotificationArea | TraySupport::MenuBarExtra => Ok(()),
    }
}

/// 拿掉托盘图标。没装时什么都不做。
pub fn remove() {
    nana_ui::remove_tray();
}

/// 隐藏主窗口，进程继续运行。
pub fn hide(window: &nana_ui::WindowHandle) {
    let _request = window.set_visible(false);
}

/// 显示、取消最小化并聚焦。对应托盘左键和「打开 MomoBako」。
pub fn show(window: &nana_ui::WindowHandle) {
    let _visible = window.set_visible(true);
    let _minimized = window.set_minimized(false);
    let _focus = window.focus();
}

/// 关掉主窗口。NanaUI 在最后一扇窗口关闭后退出事件循环，从而结束进程。
pub fn quit(window: &nana_ui::WindowHandle) {
    let _request = window.close();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_to_tray_actions() {
        assert_eq!(menu_action(TRAY_OPEN), Some(TrayAction::Show));
        assert_eq!(menu_action(TRAY_QUIT), Some(TrayAction::Quit));
        assert_eq!(menu_action(7), None);
    }

    #[test]
    fn tray_opens_the_window_on_a_primary_click() {
        let tray = tray();
        assert_eq!(tray.tooltip, "MomoBako");
        assert_eq!(tray.primary, Some(TRAY_OPEN));
        assert_eq!(tray.menu.len(), 2);
        assert!(image::load_from_memory(&tray.icon_png).is_ok(), "托盘图标是 PNG");
    }
}
