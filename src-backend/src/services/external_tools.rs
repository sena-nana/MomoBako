//! 外部程序（ffmpeg、ffprobe）的位置。
//!
//! 宿主启动时用 [`configure`] 交进两个目录：随包带的程序目录（Nana 包是 NanaUI 的 `runtime/tools`，只读）和下载
//! 程序用的可写目录（Nana 是应用缓存目录下的 `tools`）。找程序的顺序是随包目录、下载目录、可执行文件旁边
//! （ffmpeg-sidecar 原来的下载位置，开发时可能已经有）、`PATH`。都找不到时下载到下载目录，不写进安装目录。
//! 宿主没给下载目录时（Nana 解析不出应用路径时，后端单测也不设置）照 ffmpeg-sidecar 的默认下载到可执行文件旁边。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;

use crate::services::child_process;

/// 宿主交进来的程序目录。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolLocations {
    /// 随包带的程序目录，只读。
    pub bundled: Option<PathBuf>,
    /// 下载程序放的可写目录。
    pub downloads: Option<PathBuf>,
}

const FFMPEG: &str = "ffmpeg";
const FFPROBE: &str = "ffprobe";

static LOCATIONS: OnceLock<ToolLocations> = OnceLock::new();
static FFMPEG_READY: OnceLock<Result<(), String>> = OnceLock::new();

/// 设置一次程序目录；进程里只认第一次，之后再设返回错误。
pub fn configure(locations: ToolLocations) -> Result<(), String> {
    LOCATIONS
        .set(locations)
        .map_err(|_| "外部程序目录已经设置过，这次的不生效。".to_string())
}

/// 调用 ffmpeg 用的路径。
pub(crate) fn ffmpeg_path() -> PathBuf {
    resolve_tool(FFMPEG, LOCATIONS.get(), sidecar_dir().as_deref())
}

/// 调用 ffprobe 用的路径。
pub(crate) fn ffprobe_path() -> PathBuf {
    resolve_tool(FFPROBE, LOCATIONS.get(), sidecar_dir().as_deref())
}

/// 确认 ffmpeg 能用，不能用时下载一次。结果在进程里只算一次。
pub(crate) fn ensure_ffmpeg_ready() -> Result<(), String> {
    FFMPEG_READY
        .get_or_init(|| {
            let result = prepare_ffmpeg();
            if let Err(error) = &result {
                crate::app_log!(
                    "warn",
                    "tools.ffmpeg",
                    "unavailable",
                    "ffmpeg 不可用，视频和音频封面缩略图会跳过。",
                    serde_json::json!({ "error": error })
                );
            }
            result
        })
        .clone()
}

fn prepare_ffmpeg() -> Result<(), String> {
    if runs(&ffmpeg_path()) {
        return Ok(());
    }
    let Some(downloads) = LOCATIONS.get().and_then(|locations| locations.downloads.clone()) else {
        return ffmpeg_sidecar::download::auto_download()
            .map_err(|error| format!("ffmpeg setup error: {error}"));
    };
    download_ffmpeg_into(&downloads)?;
    let path = ffmpeg_path();
    if runs(&path) {
        crate::app_log!(
            "info",
            "tools.ffmpeg",
            "downloaded",
            "ffmpeg 已下载到缓存目录。",
            serde_json::json!({ "path": path.display().to_string() })
        );
        Ok(())
    } else {
        Err(format!("ffmpeg 下载以后仍然运行不了：{}", path.display()))
    }
}

fn download_ffmpeg_into(downloads: &Path) -> Result<(), String> {
    std::fs::create_dir_all(downloads).map_err(|error| {
        format!("建不了外部程序下载目录 {}：{error}", downloads.display())
    })?;
    let url = ffmpeg_sidecar::download::ffmpeg_download_url()
        .map_err(|error| format!("ffmpeg setup error: {error}"))?;
    let archive = ffmpeg_sidecar::download::download_ffmpeg_package(url, downloads)
        .map_err(|error| format!("ffmpeg 下载失败：{error}"))?;
    ffmpeg_sidecar::download::unpack_ffmpeg(&archive, downloads)
        .map_err(|error| format!("ffmpeg 解压失败：{error}"))
}

/// 程序能运行（`-version` 退出码为 0）时返回真。
fn runs(program: &Path) -> bool {
    child_process::command(program)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// 可执行文件所在目录，ffmpeg-sidecar 原来把程序下载到这里。
fn sidecar_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

/// 按随包目录、下载目录、可执行文件旁边的顺序找已经存在的程序，都没有时交回程序名，由系统按 `PATH` 找。
fn resolve_tool(
    name: &str,
    locations: Option<&ToolLocations>,
    sidecar_dir: Option<&Path>,
) -> PathBuf {
    let file_name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let candidates = locations
        .into_iter()
        .flat_map(|locations| [locations.bundled.as_deref(), locations.downloads.as_deref()])
        .chain([sidecar_dir])
        .flatten();
    for directory in candidates {
        let candidate = directory.join(&file_name);
        if candidate.is_file() {
            return candidate;
        }
    }
    PathBuf::from(name)
}

#[cfg(test)]
#[path = "external_tools_tests.rs"]
mod tests;
