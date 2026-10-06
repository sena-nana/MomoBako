//! 剪贴板、外部打开和目录揭示。
//!
//! 拖出和托盘没有 Nana 平台入口，仍由壳层记下失败文案。

use std::process::Command;

use crate::host_api::{ExternalOpenRequest, HostRequest};
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

/// 执行已经记下的打开和定位请求。其它请求留在队列里。
pub fn perform(shell: &mut ShellViewModel, mut open: impl FnMut(&str, bool) -> Result<(), String>) {
    let requests = std::mem::take(&mut shell.input.host_requests);
    let mut kept = Vec::new();
    for request in requests {
        let HostRequest::OpenExternal(ExternalOpenRequest { target, reveal }) = request else {
            kept.push(request);
            continue;
        };
        if let Err(error) = open(&target, reveal) {
            eprintln!("Nana 外部打开失败：{target}：{error}");
            shell.input.error = if reveal {
                "定位失败：宿主目录揭示尚未接通".into()
            } else {
                "打开失败：宿主外部打开尚未接通".into()
            };
        }
    }
    shell.input.host_requests = kept;
}

/// 正式窗口用系统程序打开或定位。
pub fn perform_system(shell: &mut ShellViewModel) {
    perform(shell, system_open);
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
        perform(&mut model, |target, reveal| {
            seen.push((target.to_string(), reveal));
            Ok(())
        });
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
        perform(&mut model, |_, _| Err("spawn failed".into()));
        assert_eq!(model.input.error, "定位失败：宿主目录揭示尚未接通");
    }
}
