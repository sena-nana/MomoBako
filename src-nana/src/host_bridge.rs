//! 剪贴板、外部打开、目录揭示、文件拖出和最小化到托盘。
//!
//! 打开交给系统默认程序：Windows 上直接调 `ShellExecuteW`，不经过 `cmd` 解析命令行。定位文件交给 NanaUI 的
//! `reveal_in_file_manager`（在所在文件夹里选中）。定位文件夹时 NanaUI 会直接打开该目录，和 Vue
//! `revealItemInDir` 不一致；Windows 上改用资源管理器的 `/select` 在上一级里选中该项。没有这个能力的系统
//! 记日志并返回失败，不把「打开目录」当成已经定位成功。拖出和托盘沿用已有的 `HostInputRequest`。
//! 失败时保留系统返回的错误。

use crate::host_api::{ExternalOpenRequest, HostInputRequest, HostRequest};
use crate::shell::ShellViewModel;
use crate::shell::status::FailureSource;

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
        match nana_ui_platform::write_shared_clipboard(&clipboard, text) {
            Ok(()) => true,
            Err(error) => {
                eprintln!("Nana 写入剪贴板失败：{error:?}");
                false
            }
        }
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

/// 执行已经记下的打开、拖出和托盘请求。队列每次都清空：宿主是它唯一的消费方。
pub fn perform(
    shell: &mut ShellViewModel,
    mut open: impl FnMut(&str, bool) -> Result<(), String>,
    mut drag: impl FnMut(&[String]) -> Result<(), String>,
    mut hide_to_tray: impl FnMut() -> Result<(), String>,
) {
    crate::drag_out::apply_result(shell);
    let requests = std::mem::take(&mut shell.input.host_requests);
    for request in requests {
        match request {
            HostRequest::OpenExternal(ExternalOpenRequest { target, reveal }) => {
                let result = open(&target, reveal);
                if let Err(error) = &result {
                    eprintln!("Nana 外部打开失败：{target}：{error}");
                }
                // 插件「打开目录」的结果写在插件面板上，别的打开失败写进状态区。
                if let Some(Err(error)) =
                    crate::shell::admin::settle_directory_open(shell, &target, result)
                {
                    let what = if reveal {
                        "定位失败"
                    } else {
                        "打开失败"
                    };
                    shell
                        .status
                        .fail(FailureSource::Host, format!("{what}：{error}"));
                }
            }
            HostRequest::Input(HostInputRequest::DragOut { paths }) => {
                if let Err(error) = drag(&paths) {
                    eprintln!("Nana 文件拖出失败：{error}");
                    shell
                        .status
                        .fail(FailureSource::Host, format!("拖出失败：{error}"));
                    shell.input.external_drag_result = Some(false);
                }
            }
            HostRequest::Input(HostInputRequest::MinimizeToTray) => match hide_to_tray() {
                Ok(()) => shell.status.resolve(FailureSource::Tray),
                Err(error) => {
                    eprintln!("Nana 最小化到托盘失败：{error}");
                    shell
                        .status
                        .fail(FailureSource::Tray, format!("最小化到托盘失败：{error}"));
                }
            },
        }
    }
}

/// 正式窗口用系统程序打开、拖出文件，或隐藏到托盘。
pub fn perform_live(
    shell: &mut ShellViewModel,
    window: &nana_ui::WindowHandle,
    tray_ready: bool,
    tray_error: Option<&str>,
) {
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

/// 用系统程序打开，或者在文件管理器里定位。文件交给 NanaUI；文件夹在上一级里选中。
fn system_open(target: &str, reveal: bool) -> Result<(), String> {
    if reveal {
        return reveal_target(target);
    }
    open_with_default_program(target)
}

/// 定位一条路径。文件走 NanaUI；文件夹在上一级里选中。路径不存在时不启动文件管理器。
fn reveal_target(target: &str) -> Result<(), String> {
    let path = std::path::Path::new(target);
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => select_directory(path),
        Ok(_) => nana_ui::reveal_in_file_manager(path).map_err(|error| reveal_failure(&error)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Err(reveal_failure(&nana_ui::RevealError::NotFound))
        }
        Err(error) => {
            eprintln!("Nana 定位前读路径失败：{target}：{error}");
            Err(format!("文件管理器不可用：{error}"))
        }
    }
}

/// Windows 资源管理器在上一级里选中这个目录的参数。不启动进程。
#[cfg(target_os = "windows")]
fn directory_select_argument(path: &std::path::Path) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let shown = explorer_path(&absolute);
    format!("/select,\"{shown}\"")
}

/// 去掉 `\\?\` 前缀，反斜杠，供资源管理器使用。
#[cfg(target_os = "windows")]
fn explorer_path(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('/', "\\");
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        path
    }
}

/// 在资源管理器里选中目录本身，而不是打开它。不等待资源管理器退出：它交出去以后自己会退，退出码也不是成败。
#[cfg(target_os = "windows")]
fn select_directory(path: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let argument = directory_select_argument(path);
    match std::process::Command::new("explorer.exe")
        .raw_arg(&argument)
        .spawn()
    {
        Ok(child) => {
            drop(child);
            Ok(())
        }
        Err(error) => {
            eprintln!("Nana 在上一级里选中目录失败：{}：{error}", path.display());
            Err(format!("文件管理器不可用：{error}"))
        }
    }
}

/// 当前系统的定位接口只会打开目录。不把打开当成选中成功。
#[cfg(not(target_os = "windows"))]
fn select_directory(path: &std::path::Path) -> Result<(), String> {
    eprintln!(
        "Nana 当前系统不能在上一级里选中目录，不假装定位成功：{}",
        path.display()
    );
    Err("文件管理器不能在上一级里选中这个目录".into())
}

/// 用系统默认程序打开文件或网址。
///
/// Windows 上直接调 `ShellExecuteW`，目标整串交给外壳，不经过 `cmd /C start`：`cmd` 会把网址里的 `&`、`|`
/// 当成命令分隔符，`https://x/?a=1&calc` 这样的链接会多执行一条命令。
#[cfg(target_os = "windows")]
fn open_with_default_program(target: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, PCWSTR, w};

    let file = HSTRING::from(target);
    // SAFETY: 参数都是有效的以零结尾的宽字符串或空指针，不传窗口句柄。
    let instance = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &file,
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // 返回值大于 32 表示成功，否则是 `SE_ERR_*` 错误码。
    let code = instance.0 as isize;
    if code > 32 {
        Ok(())
    } else {
        Err(shell_execute_failure(code))
    }
}

/// 用系统默认程序打开文件或网址。macOS 用 `open`，其余用 `xdg-open`，目标作为单独的参数传，不经过 shell。
#[cfg(not(target_os = "windows"))]
fn open_with_default_program(target: &str) -> Result<(), String> {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// `ShellExecuteW` 的错误码写成中文，认不出的带上原始错误码。
#[cfg(any(target_os = "windows", test))]
fn shell_execute_failure(code: isize) -> String {
    match code {
        0 | 8 => "系统资源不足".into(),
        2 => "文件不存在".into(),
        3 => "路径不存在".into(),
        5 => "没有访问权限".into(),
        11 => "不是有效的程序".into(),
        26 => "文件正被占用".into(),
        27 | 31 => "没有关联的打开程序".into(),
        _ => format!("系统打开失败（错误码 {code}）"),
    }
}

/// 定位失败的原因写成中文：路径不存在，或者文件管理器起不来（带系统原文）。
fn reveal_failure(error: &nana_ui::RevealError) -> String {
    match error {
        nana_ui::RevealError::NotFound => "路径不存在".into(),
        nana_ui::RevealError::Unavailable(reason) => format!("文件管理器不可用：{reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::input::InputMessage;
    use crate::shell::status::FailureSource;
    use crate::shell::{ShellMessage, ShellViewModel};

    /// 状态区现在的失败：来源和文案。
    fn failure(model: &ShellViewModel) -> Option<(FailureSource, &str)> {
        model
            .status
            .failure()
            .map(|failure| (failure.source, failure.message.as_str()))
    }

    #[test]
    fn perform_opens_and_reveals_without_spawning() {
        let mut model = crate::shell::ShellViewModel::default();
        model.workspace.active_repo_id = Some("repo".into());
        model.reduce(ShellMessage::Input(InputMessage::OpenEntry {
            has_repo: true,
            absolute_path: "C:\\a.png".into(),
            path: "a.png".into(),
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
        assert_eq!(
            seen,
            [("C:\\a.png".into(), false), ("C:\\a.png".into(), true)]
        );
        assert_eq!(failure(&model), None);
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
        perform(
            &mut model,
            |_, _| Err("spawn failed".into()),
            |_| Ok(()),
            || Ok(()),
        );
        assert_eq!(
            failure(&model),
            Some((FailureSource::Host, "定位失败：spawn failed"))
        );
        model.reduce(ShellMessage::Input(InputMessage::RevealEntry {
            absolute_path: "C:\\b.png".into(),
        }));
        assert_eq!(failure(&model), None, "下一次定位开始时清掉上一次失败");
    }

    #[test]
    fn drag_out_and_tray_report_real_failures() {
        let mut model = crate::shell::ShellViewModel::default();
        model.reduce(crate::shell::ShellMessage::Input(
            crate::shell::input::InputMessage::StartExternalDrag {
                paths: vec!["C:\\repo\\a.png".into()],
                trash: false,
                backend_kind: "filesystem".into(),
                repo_root: "C:\\repo".into(),
            },
        ));
        perform(&mut model, |_, _| Ok(()), |_| Err("OLE".into()), || Ok(()));
        assert_eq!(
            failure(&model),
            Some((FailureSource::Host, "拖出失败：OLE"))
        );
        assert_eq!(model.input.external_drag_result, Some(false));
        assert!(model.input.host_requests.is_empty());

        model.reduce(crate::shell::ShellMessage::Input(
            crate::shell::input::InputMessage::StartExternalDrag {
                paths: vec!["C:\\repo\\a.png".into()],
                trash: false,
                backend_kind: "filesystem".into(),
                repo_root: "C:\\repo".into(),
            },
        ));
        assert_eq!(failure(&model), None, "拖出开始时清掉上一次失败");
        perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
        assert_eq!(failure(&model), None);
        assert!(model.input.host_requests.is_empty());

        model.settings.close_behavior = "minimizeToTray".into();
        model.reduce(crate::shell::ShellMessage::WindowAction(
            crate::shell::WindowAction::Close,
        ));
        perform(
            &mut model,
            |_, _| Ok(()),
            |_| Ok(()),
            || Err("系统托盘没有建起来".into()),
        );
        assert_eq!(
            failure(&model),
            Some((FailureSource::Tray, "最小化到托盘失败：系统托盘没有建起来"))
        );
        assert!(
            model.input.notice.is_empty(),
            "关闭确认框的文案不再借来写托盘失败"
        );
        assert!(
            model
                .input
                .take_platform_commands(nana_ui_platform::WindowId(1), false)
                .is_empty()
        );

        model.reduce(crate::shell::ShellMessage::WindowAction(
            crate::shell::WindowAction::Close,
        ));
        perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
        assert_eq!(failure(&model), None, "收进托盘成功以后托盘失败作废");
        assert!(
            model
                .input
                .take_platform_commands(nana_ui_platform::WindowId(1), false)
                .is_empty()
        );
    }

    /// 定位文件夹是在上一级里选中。缺路径不启动资源管理器；参数里带 `/select`。
    #[test]
    fn reveal_directory_selects_the_item_in_its_parent() {
        let missing = reveal_target(r"C:\this\does\not\exist\momobako-reveal-dir");
        assert_eq!(missing.unwrap_err(), "路径不存在");
        #[cfg(target_os = "windows")]
        {
            assert_eq!(
                directory_select_argument(std::path::Path::new(r"C:\a b\models")),
                r#"/select,"C:\a b\models""#
            );
            assert_eq!(explorer_path(std::path::Path::new(r"\\?\C:\x")), r"C:\x");
        }
        #[cfg(not(target_os = "windows"))]
        {
            let error = select_directory(std::path::Path::new("/tmp/momobako-models")).unwrap_err();
            assert!(error.contains("不能在上一级"));
        }
    }

    /// 定位失败照 NanaUI 给的原因写中文：路径不存在、文件管理器起不来。
    #[test]
    fn reveal_failures_read_in_chinese() {
        assert_eq!(
            reveal_failure(&nana_ui::RevealError::NotFound),
            "路径不存在"
        );
        assert_eq!(
            reveal_failure(&nana_ui::RevealError::Unavailable("spawn failed".into())),
            "文件管理器不可用：spawn failed"
        );
    }

    /// 打开失败照 `ShellExecuteW` 的错误码写中文，认不出的带上原始错误码。
    #[test]
    fn open_failures_read_in_chinese() {
        assert_eq!(shell_execute_failure(2), "文件不存在");
        assert_eq!(shell_execute_failure(31), "没有关联的打开程序");
        assert_eq!(shell_execute_failure(29), "系统打开失败（错误码 29）");
    }

    /// 任何地方都不经过 `cmd` 启动程序：`cmd /C start` 会把参数里的 `&`、`|` 当成命令分隔符，
    /// 元数据里的链接就能多执行一条命令。
    #[test]
    fn nothing_launches_programs_through_cmd() {
        let needle = ["Command::new(\"", "cmd", "\")"].concat();
        let mut pending = vec![std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src")];
        while let Some(dir) = pending.pop() {
            for entry in std::fs::read_dir(&dir).expect("读源码目录") {
                let path = entry.expect("读目录项").path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().is_some_and(|extension| extension == "rs") {
                    let source = std::fs::read_to_string(&path).expect("读源码");
                    assert!(
                        !source.contains(&needle),
                        "{} 经过 cmd 启动程序",
                        path.display()
                    );
                }
            }
        }
    }

    /// 关闭要确认时，每次 `update` 后宿主都跑一遍 `perform`：问、取消、再问、再取消，队列一直是空的，
    /// 关闭确认只由 `pending_close` 驱动，确认后才排关窗命令。
    #[test]
    fn repeated_close_confirmations_leave_no_host_requests_behind() {
        let mut model = ShellViewModel::default();
        model.settings.close_behavior = "confirm".into();
        for _ in 0..3 {
            model.reduce(ShellMessage::WindowAction(
                crate::shell::WindowAction::Close,
            ));
            perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
            assert!(model.input.pending_close);
            assert!(
                model.input.host_requests.is_empty(),
                "关闭确认不进宿主请求队列"
            );
            model.reduce(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(false)));
            perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
            assert!(!model.input.pending_close);
            assert!(
                model
                    .input
                    .take_platform_commands(nana_ui_platform::WindowId(1), false)
                    .is_empty(),
                "取消不关窗"
            );
        }
        model.reduce(ShellMessage::WindowAction(
            crate::shell::WindowAction::Close,
        ));
        model.reduce(ShellMessage::Input(InputMessage::ConfirmCloseAnswer(true)));
        perform(&mut model, |_, _| Ok(()), |_| Ok(()), || Ok(()));
        assert!(model.input.host_requests.is_empty());
        let commands = model
            .input
            .take_platform_commands(nana_ui_platform::WindowId(1), false);
        assert!(matches!(
            commands.as_slice(),
            [nana_ui_platform::host::WindowCommand::Close(_)]
        ));
    }
}
