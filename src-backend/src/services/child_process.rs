//! 宿主起的命令行子进程。
//!
//! Nana 发布版是 Windows 图形界面程序，进程没有控制台。这时起一个命令行程序（ffmpeg、git、tar、进程插件的运行器），
//! 系统会给它另开一个控制台窗口：短命令在桌面上一闪而过，长驻的进程插件会一直挂着一个黑窗口。宿主起子进程都经
//! [`command`] 建：当前进程没有控制台时加上 `CREATE_NO_WINDOW`；有控制台时（开发时从终端运行）照旧
//! 共用这个控制台，输出还能看到。

use std::ffi::OsStr;
use std::process::Command;

/// 建一个子进程命令；当前进程没有控制台时不让它弹出控制台窗口。
pub fn command(program: impl AsRef<OsStr>) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    if !has_console() {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// 当前进程连着控制台时返回真。
#[cfg(windows)]
fn has_console() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetConsoleWindow() -> *mut std::ffi::c_void;
    }
    // SAFETY: GetConsoleWindow 没有参数，只读当前进程的控制台句柄，没有控制台时返回空指针。
    !unsafe { GetConsoleWindow() }.is_null()
}
