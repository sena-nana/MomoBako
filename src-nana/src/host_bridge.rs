//! 剪贴板、外部打开、目录揭示、文件拖出和最小化到托盘。
//!
//! 打开和定位启动系统程序。拖出和托盘沿用已有的 `HostInputRequest`，
//! 由窗口宿主在 Windows 上调用系统接口。失败时保留系统返回的错误。

use std::process::Command;

use crate::host_api::{ExternalOpenRequest, HostInputRequest, HostRequest};
use crate::shell::ShellViewModel;

#[cfg(test)]
use std::cell::RefCell;

#[cfg(test)]
thread_local! {
    static COPIED: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 写入剪贴板。测试用进程内记录，正式窗口用系统剪贴板。
pub fn copy_text(text: &str) -> bool {
    #[cfg(test)]
    {
        COPIED.with(|slot| *slot.borrow_mut() = Some(text.to_string()));
        true
    }
    #[cfg(not(test))]
    {
        let clipboard = nana_ui_platform::default_shared_clipboard();
        nana_ui_platform::write_shared_clipboard(&clipboard, text).is_ok()
    }
}

/// 读取剪贴板文本。测试读进程内记录，正式窗口读系统剪贴板。失败记日志并返回空。
pub fn read_clipboard_text() -> Option<String> {
    #[cfg(test)]
    {
        COPIED.with(|slot| slot.borrow().clone())
    }
    #[cfg(not(test))]
    {
        let clipboard = nana_ui_platform::default_shared_clipboard();
        match nana_ui_platform::read_shared_clipboard(&clipboard) {
            Ok(text) => text,
            Err(error) => {
                eprintln!("Nana 读取剪贴板失败：{error:?}");
                None
            }
        }
    }
}

/// 执行已经记下的打开、拖出和托盘请求。其它请求留在队列里。
pub fn perform(
    shell: &mut ShellViewModel,
    mut open: impl FnMut(&str, bool) -> Result<(), String>,
    mut drag: impl FnMut(&[String]) -> Result<(), String>,
    mut hide_to_tray: impl FnMut() -> Result<(), String>,
) {
    crate::drag_out::apply_result(shell);
    let requests = std::mem::take(&mut shell.input.host_requests);
    let mut kept = Vec::new();
    for request in requests {
        match request {
            HostRequest::OpenExternal(ExternalOpenRequest { target, reveal }) => {
                if let Err(error) = open(&target, reveal) {
                    eprintln!("Nana 外部打开失败：{target}：{error}");
                    shell.input.error = if reveal {
                        format!("定位失败：{error}")
                    } else {
                        format!("打开失败：{error}")
                    };
                }
            }
            HostRequest::Input(HostInputRequest::DragOut { paths }) => {
                if let Err(error) = drag(&paths) {
                    eprintln!("Nana 文件拖出失败：{error}");
                    shell.input.error = format!("拖出失败：{error}");
                    shell.input.external_drag_result = Some(false);
                }
            }
            HostRequest::Input(HostInputRequest::MinimizeToTray) => {
                if let Err(error) = hide_to_tray() {
                    eprintln!("Nana 最小化到托盘失败：{error}");
                    shell.input.notice = format!("最小化到托盘失败：{error}");
                } else {
                    shell.input.notice.clear();
                }
            }
            other => kept.push(other),
        }
    }
    shell.input.host_requests = kept;
}

/// 正式窗口用系统程序打开、拖出文件，或隐藏到托盘。
pub fn perform_live(shell: &mut ShellViewModel, window: &nana_ui::WindowHandle, tray_ready: bool, tray_error: Option<&str>) {
    let window_for_drag = window.clone();
    perform(
        shell,
        system_open,
        move |paths| crate::drag_out::queue(&window_for_drag, paths),
        || {
            if tray_ready {
                crate::tray::hide(window);
                Ok(())
            } else {
                Err(tray_error.unwrap_or("系统托盘没有建起来").to_string())
            }
        },
    );
}

fn system_open(target: &str, reveal: bool) -> Result<(), String> {
    let mut command = if reveal { reveal_command(target) } else { open_command(target) };
    command.spawn().map(|_| ()).map_err(|error| error.to_string())
}

fn open_command(target: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", target]);
        command
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.arg(target);
        command
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        command.arg(target);
        command
    }
}

fn reveal_command(target: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("explorer");
        command.arg(format!("/select,{target}"));
        command
    }
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        command.args(["-R", target]);
        command
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let mut command = Command::new("xdg-open");
        let parent = std::path::Path::new(target).parent().unwrap_or(std::path::Path::new(target));
        command.arg(parent);
        command
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::input::InputMessage;
    use crate::shell::ShellMessage;

    #[test]
    fn perform_opens_and_reveals_without_spawning() {
        let mut model = crate::shell::ShellViewModel::default();
        model.workspace.active_repo_id = Some("repo".into());
        model.reduce(ShellMessage::Input(InputMessage::OpenEntry {
            has_repo: true,
            absolute_path: "C:\\a.png".into(),
        }));
        model.reduce(ShellMessage::Input(InputMessage::RevealEntry {
            absolute_path: "C:\\a.png".into(),
        }));
        let mut seen = Vec::new();
        perform(
            &mut model,
            |target, reveal| {
                seen.push((target.to_string(), reveal));
                Ok(())
            },
            |_| Ok(()),
            || Ok(()),
        );
        assert_eq!(seen, [("C:\\a.png".into(), false), ("C:\\a.png".into(), true)]);
        assert!(model.input.error.is_empty());
        assert!(model.input.host_requests.is_empty());
        assert!(copy_text("token"));
        COPIED.with(|slot| assert_eq!(slot.borrow().as_deref(), Some("token")));
    }

    #[test]
    fn perform_keeps_the_failure_copy_when_the_system_call_fails() {
        let mut model = crate::shell::ShellViewModel::default();
        model.reduce(ShellMessage::Input(InputMessage::RevealEntry {
            absolute_path: "C:\\missing.png".into(),
        }));
        perform(&mut model, |_, _| Err("spawn failed".into()), |_| Ok(()), || Ok(()));
        assert_eq!(model.input.error, "定位失败：spawn failed");
    }

    #[test]
    fn drag_out_and_tray_report_real_failures() {
        let mut model = crate::shell::ShellViewModel::default();
        model.reduce(crate::shell::ShellMessage::Input(crate::shell::input::InputMessage::StartExternalDrag {
            paths: vec!["C:\\repo\\a.png".into()],
            trash: false,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
        }));
        perform(&mut model, |_, _| Ok(()), |_| Err("OLE".into()), || Ok(()));
        assert_eq!(model.input.error, "拖出失败：OLE");
        assert_eq!(model.input.external_drag_result, Some(false));
        assert!(model.input.host_requests.is_empty());

        model.reduce(crate::shell::ShellMessage::Input(crate::shell::input::InputMessage::StartExternalDrag {
            paths: vec!["C:\\repo\\a.png".into()],
            trash: false,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
        }));
        model.input.error.clear();
        perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
        assert!(model.input.error.is_empty());
        assert!(model.input.host_requests.is_empty());

        model.settings.close_behavior = "minimizeToTray".into();
        model.reduce(crate::shell::ShellMessage::WindowAction(crate::shell::WindowAction::Close));
        perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Err("系统托盘没有建起来".into()));
        assert_eq!(model.input.notice, "最小化到托盘失败：系统托盘没有建起来");
        assert!(model.input.take_platform_commands(nana_ui_platform::WindowId(1), false).is_empty());

        model.reduce(crate::shell::ShellMessage::WindowAction(crate::shell::WindowAction::Close));
        perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
        assert!(model.input.notice.is_empty());
        assert!(model.input.take_platform_commands(nana_ui_platform::WindowId(1), false).is_empty());
    }
}
