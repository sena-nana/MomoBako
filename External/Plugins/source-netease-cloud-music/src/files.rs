//! 下载文件落盘。
//!
//! 用户选的本地目录里只新建、不覆盖（`create_new`，检查和新建是同一步）；插件自己管理的播放缓存和暂存目录
//! 可以替换旧文件（先写临时文件再改名）。一首歌写到一半失败时，只删这一首这次写下的文件。

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use momobako_mutsuki_plugin_sdk::write_host_log_silently;

use crate::models::RuntimeContext;

/// 目标位置已有同名文件时怎么办。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WritePolicy {
    /// 用户的目录：已有的文件保留原样，不写入。
    KeepExisting,
    /// 插件自己管理的目录：替换之前写下的同名文件。
    Replace,
}

/// 按 `policy` 写一个文件，交回这次有没有写下它：保留已有文件时目标已经存在就不写，交回 `false`。
pub(crate) fn write_file(
    runtime: &RuntimeContext,
    path: &Path,
    bytes: &[u8],
    policy: WritePolicy,
) -> Result<bool, String> {
    match policy {
        WritePolicy::KeepExisting => write_new_file(runtime, path, bytes),
        WritePolicy::Replace => replace_file(runtime, path, bytes).map(|()| true),
    }
}

/// 只在目标不存在时新建并写入。写到一半失败时删掉这个刚新建的文件，别的文件不碰。
fn write_new_file(runtime: &RuntimeContext, path: &Path, bytes: &[u8]) -> Result<bool, String> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => {
            log_file_error(runtime, "downloadCreateFailed", "网易云下载无法新建文件。", path, &error);
            return Err(format!("无法新建文件 {}：{error}", path.display()));
        }
    };
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        log_file_error(runtime, "downloadWriteFailed", "网易云下载写入文件失败。", path, &error);
        remove_written(runtime, &[path.to_path_buf()]);
        return Err(format!("写入文件 {} 失败：{error}", path.display()));
    }
    Ok(true)
}

/// 先写同目录下的临时文件，再改名替换目标，目标处不会留下写了一半的文件。
pub(crate) fn replace_file(runtime: &RuntimeContext, path: &Path, bytes: &[u8]) -> Result<(), String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("下载目标缺少文件名：{}", path.display()))?
        .to_string_lossy()
        .to_string();
    let temp = path.with_file_name(format!("{name}.download"));
    if let Err(error) = fs::write(&temp, bytes) {
        log_file_error(runtime, "downloadWriteFailed", "网易云下载写入临时文件失败。", &temp, &error);
        remove_written(runtime, &[temp.clone()]);
        return Err(format!("写入临时文件 {} 失败：{error}", temp.display()));
    }
    let placed = match fs::remove_file(path) {
        Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
        _ => fs::rename(&temp, path),
    };
    if let Err(error) = placed {
        log_file_error(runtime, "downloadMoveFailed", "网易云下载落盘失败。", path, &error);
        remove_written(runtime, &[temp]);
        return Err(format!("文件 {} 落盘失败：{error}", path.display()));
    }
    Ok(())
}

/// 删掉这次写下的文件。已经不在的不算失败；删不掉的记日志，不改变调用方要交回的结果。
pub(crate) fn remove_written(runtime: &RuntimeContext, paths: &[PathBuf]) {
    for path in paths {
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => log_file_error(
                runtime,
                "downloadCleanupFailed",
                "网易云下载删除写了一半的文件失败。",
                path,
                &error,
            ),
        }
    }
}

fn log_file_error(
    runtime: &RuntimeContext,
    action: &str,
    message: &str,
    path: &Path,
    error: &std::io::Error,
) {
    write_host_log_silently(
        &runtime.host_runtime,
        "error",
        action,
        message,
        serde_json::json!({ "path": path, "error": error.to_string() }),
    );
}
