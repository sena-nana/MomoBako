//! 系统托盘。
//!
//! 行为与 `src-tauri/src/app_shell.rs` 的托盘一致：提示 MomoBako，左键或双击显示窗口，
//! 菜单是「打开 MomoBako」和「退出」。Nana 没有托盘组件，Windows 上用通知图标。
//! 退出走窗口关闭，最后一扇窗口关掉后进程结束。最小化到托盘只隐藏窗口。

use std::sync::{Arc, Mutex};

pub const TRAY_OPEN_ID: &str = "tray-open";
pub const TRAY_QUIT_ID: &str = "tray-quit";
const TOOLTIP: &str = "MomoBako";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrayAction {
    Show,
    Quit,
}

/// 菜单 id 对应的动作。其它 id 忽略。
pub fn menu_action(id: &str) -> Option<TrayAction> {
    match id {
        TRAY_OPEN_ID => Some(TrayAction::Show),
        TRAY_QUIT_ID => Some(TrayAction::Quit),
        _ => None,
    }
}

/// 建立托盘。失败时调用方不能假装窗口已经收进托盘。
pub fn install(window: nana_ui::WindowHandle) -> Result<TrayHost, String> {
    #[cfg(windows)]
    {
        install_windows(window)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Err("当前平台没有系统托盘接口".into())
    }
}

pub struct TrayHost {
    #[cfg(windows)]
    _icon: tray_icon::TrayIcon,
    #[cfg(not(windows))]
    _private: (),
}

/// 隐藏主窗口，进程继续运行。
pub fn hide(window: &nana_ui::WindowHandle) {
    let _request = window.set_visible(false);
}

/// 显示、取消最小化并聚焦。对应托盘左键、双击和「打开 MomoBako」。
pub fn show(window: &nana_ui::WindowHandle) {
    let _visible = window.set_visible(true);
    let _minimized = window.set_minimized(false);
    let _focus = window.focus();
}

/// 关掉主窗口。Nana 在最后一扇窗口关闭后退出事件循环，从而结束进程。
pub fn quit(window: &nana_ui::WindowHandle) {
    let _request = window.close();
}

#[cfg(windows)]
fn install_windows(window: nana_ui::WindowHandle) -> Result<TrayHost, String> {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let icon = app_icon()?;
    let open = MenuItem::with_id(TRAY_OPEN_ID, "打开 MomoBako", true, None);
    let quit_item = MenuItem::with_id(TRAY_QUIT_ID, "退出", true, None);
    let menu = Menu::new();
    menu.append(&open).map_err(|error| format!("托盘菜单没有挂上：{error}"))?;
    menu.append(&quit_item).map_err(|error| format!("托盘菜单没有挂上：{error}"))?;

    let shared = Arc::new(Mutex::new(window));
    let clicks = Arc::clone(&shared);
    TrayIconEvent::set_event_handler(Some(move |event| {
        let show_window = matches!(
            event,
            TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
                | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
        );
        if show_window {
            dispatch(&clicks, TrayAction::Show);
        }
    }));
    let menu_window = Arc::clone(&shared);
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some(action) = menu_action(event.id().as_ref()) {
            dispatch(&menu_window, action);
        }
    }));

    let icon = TrayIconBuilder::new().with_id("main-tray")
        .with_menu(Box::new(menu))
        .with_tooltip(TOOLTIP)
        .with_menu_on_left_click(false)
        .with_icon(icon)
        .build()
        .map_err(|error| format!("托盘图标没有建起来：{error}"))?;
    Ok(TrayHost { _icon: icon })
}

#[cfg(windows)]
fn dispatch(window: &Mutex<nana_ui::WindowHandle>, action: TrayAction) {
    let Ok(window) = window.lock() else {
        eprintln!("Nana 托盘窗口句柄锁已损坏");
        return;
    };
    match action {
        TrayAction::Show => show(&window),
        TrayAction::Quit => quit(&window),
    }
}

#[cfg(windows)]
fn app_icon() -> Result<tray_icon::Icon, String> {
    let image = image::load_from_memory(include_bytes!("../../src-tauri/icons/32x32.png"))
        .map_err(|error| format!("托盘图标解码失败：{error}"))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    tray_icon::Icon::from_rgba(rgba.into_raw(), width, height).map_err(|error| format!("托盘图标无效：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_match_the_tauri_tray() {
        assert_eq!(menu_action(TRAY_OPEN_ID), Some(TrayAction::Show));
        assert_eq!(menu_action(TRAY_QUIT_ID), Some(TrayAction::Quit));
        assert_eq!(menu_action("其他"), None);
        assert_eq!(TOOLTIP, "MomoBako");
    }
}
