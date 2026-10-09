//! 把库内文件拖出窗口。
//!
//! Tauri 走 `plugin:drag|start_drag`，Windows 实现是 `drag` crate 的 `DoDragDrop`。
//! Nana 没有对应窗口命令，所以在窗口线程上借用 HWND 再发起同样的复制拖放。
//! 取消拖放不算失败。拿不到句柄或系统调用失败才写错误。

use std::cell::RefCell;

use crate::shell::status::FailureSource;
use crate::shell::ShellViewModel;

thread_local! {
    static RESULT: RefCell<Option<Result<(), String>>> = const { RefCell::new(None) };
}

/// 把上一次系统拖放的结果写回壳层。失败写进状态区；拖出开始时状态区已经清掉了上一次失败，成功不用再清。
pub fn apply_result(shell: &mut ShellViewModel) {
    let Some(result) = RESULT.with(|slot| slot.borrow_mut().take()) else {
        return;
    };
    match result {
        Ok(()) => shell.input.external_drag_result = Some(true),
        Err(error) => {
            eprintln!("Nana 文件拖出失败：{error}");
            shell.status.fail(FailureSource::Host, format!("拖出失败：{error}"));
            shell.input.external_drag_result = Some(false);
        }
    }
    shell.mark_surface_dirty();
}

/// 在窗口线程上发起系统拖放。排队成功只表示请求已交给宿主，结果稍后由 `apply_result` 写回。
pub fn queue(window: &nana_ui::WindowHandle, paths: &[String]) -> Result<(), String> {
    let files = file_list(paths)?;
    #[cfg(windows)]
    {
        queue_windows(window, files)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, files);
        eprintln!("Nana 当前平台没有文件拖出接口");
        Err("当前平台没有文件拖出接口".into())
    }
}

fn file_list(paths: &[String]) -> Result<Vec<std::path::PathBuf>, String> {
    if paths.is_empty() || paths.iter().all(|path| path.trim().is_empty()) {
        return Err("没有可拖出的绝对路径".into());
    }
    Ok(paths.iter().map(std::path::PathBuf::from).collect())
}

#[cfg(windows)]
fn queue_windows(window: &nana_ui::WindowHandle, files: Vec<std::path::PathBuf>) -> Result<(), String> {
    let redraw = window.clone();
    let mut request = window.with_native_handle(move |handle| {
        let result = drag::start_drag(
            &handle,
            drag::DragItem::Files(files),
            drag::Image::Raw(include_bytes!("../../src-tauri/icons/32x32.png").to_vec()),
            |result, cursor| {
                eprintln!("Nana 文件拖出结束：{result:?} @ {},{}", cursor.x, cursor.y);
            },
            drag::Options { mode: drag::DragMode::Copy, ..drag::Options::default() },
        )
        .map_err(|error| error.to_string());
        RESULT.with(|slot| *slot.borrow_mut() = Some(result));
        let _ = redraw.request_redraw();
    });
    match request.try_take() {
        Some(Err(error)) => {
            eprintln!("Nana 文件拖出拿不到窗口句柄：{error}");
            Err(format!("拿不到窗口句柄：{error}"))
        }
        Some(Ok(())) | None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_paths_fail_without_a_system_call() {
        assert_eq!(file_list(&[]).unwrap_err(), "没有可拖出的绝对路径");
        assert_eq!(file_list(&[" ".into()]).unwrap_err(), "没有可拖出的绝对路径");
        assert_eq!(file_list(&["C:\\a.png".into()]).unwrap().len(), 1);
    }

    /// 失败写进状态区；成功只记结果，上一次失败由下一次拖出开始时清掉。
    #[test]
    fn apply_result_records_failures_in_the_status_area() {
        RESULT.with(|slot| *slot.borrow_mut() = Some(Err("OLE".into())));
        let mut shell = ShellViewModel::default();
        apply_result(&mut shell);
        let failure = shell.status.failure().expect("拖出失败要进状态区");
        assert_eq!((failure.source, failure.message.as_str()), (FailureSource::Host, "拖出失败：OLE"));
        assert_eq!(shell.input.external_drag_result, Some(false));
        RESULT.with(|slot| *slot.borrow_mut() = Some(Ok(())));
        apply_result(&mut shell);
        assert_eq!(shell.input.external_drag_result, Some(true));
        shell.reduce(crate::shell::ShellMessage::Input(crate::shell::input::InputMessage::StartExternalDrag {
            paths: vec!["C:\\repo\\a.png".into()],
            trash: false,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
        }));
        assert!(shell.status.failure().is_none(), "拖出开始时清掉上一次失败");
    }
}
