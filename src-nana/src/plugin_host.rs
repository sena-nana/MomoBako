//! 启动时接上随附插件、外部程序目录和插件宿主。
//!
//! 依次做三件事：
//! 1. 把随附插件同步进服务目录的 `plugins/builtin`（`momobako_backend::services::runtime::bundled_plugins`）。发布包
//!    以包里 manifest 列出的插件为准：把新版本解压到旧目录上时，`runtime/plugins` 里会留下旧版本的包，它们不算随附插件。
//! 2. 把随包程序目录和下载目录交给 `external_tools`，ffmpeg 缺了下载到缓存目录，不写进安装目录。
//! 3. 建插件运行时、加载一代插件，再装成进程里唯一的插件宿主。资源库的文件读写经它调本地文件系统等后端插件。
//!
//! 每一步失败都只记日志，应用照常启动：插件管理页会把加载不了的插件标成不可用并写原因。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nana_ui::nana_package::manifest::PackageManifest;
use serde_json::json;

use crate::app_paths::HostLocations;
use crate::backend::services::external_tools::{self, ToolLocations};
use crate::backend::services::mutsuki_host::{self, MomoPluginRuntime};
use crate::backend::services::mutsuki_runner::MomoTaskRuntime;
use crate::backend::services::runtime::bundled_plugins;
use crate::backend::RepositoryRuntime;

/// 依次同步随附插件、设置外部程序目录、启动插件宿主。
pub(crate) fn start(locations: &HostLocations, runtime: &RepositoryRuntime, tasks: &Arc<MomoTaskRuntime>) {
    sync_bundled_plugins(locations, runtime);
    configure_tools(locations);
    start_plugin_host(runtime, tasks);
}

fn sync_bundled_plugins(locations: &HostLocations, runtime: &RepositoryRuntime) {
    let Some(dir) = locations.bundled_plugins.as_deref() else {
        return;
    };
    let manifest = nana_ui::NanaApplication::package_manifest();
    if locations.is_packaged() && manifest.is_none() {
        momobako_backend::app_log!(
            "warn",
            "plugin.bundle",
            "manifestMissing",
            "发布包的 manifest 没有读到，随附插件按目录里的 .momoplug 同步。",
            json!({ "dir": dir.display().to_string() })
        );
    }
    let packages = match bundled_packages(dir, locations.is_packaged(), manifest) {
        Ok(Some(packages)) => packages,
        Ok(None) => {
            eprintln!("Nana 没有插件打包输出，随附插件不同步：{}", dir.display());
            return;
        }
        Err(error) => {
            report_failure("plugin.bundle", "listFailed", "随附插件列不出来，这次不同步。", &error);
            return;
        }
    };
    match bundled_plugins::sync_bundled_plugins(&packages, &runtime.service_root()) {
        Ok(report) => momobako_backend::app_log!(
            "info",
            "plugin.bundle",
            "synced",
            "随附插件已同步。",
            json!({
                "source": dir.display().to_string(),
                "copied": report.copied,
                "unchanged": report.unchanged.len(),
                "removed": report.removed,
            })
        ),
        Err(error) => report_failure("plugin.bundle", "syncFailed", "随附插件同步失败，插件目录保持上次的样子。", &error),
    }
}

/// 这次要同步的随附插件。发布包有 manifest 时用它列的插件，没有时（开发）列目录里的 `.momoplug`；
/// 开发时目录不存在（还没打包插件）返回 `None`，不同步也不删旧包。
fn bundled_packages(
    dir: &Path,
    packaged: bool,
    manifest: Option<&PackageManifest>,
) -> Result<Option<Vec<PathBuf>>, String> {
    if let Some(manifest) = manifest.filter(|_| packaged) {
        return Ok(Some(manifest.plugins.iter().map(|plugin| dir.join(&plugin.file)).collect()));
    }
    if !packaged && !dir.is_dir() {
        return Ok(None);
    }
    bundled_plugins::list_plugin_packages(dir).map(Some)
}

fn configure_tools(locations: &HostLocations) {
    let tools = ToolLocations {
        bundled: locations.bundled_tools.clone(),
        downloads: locations.tool_downloads.clone(),
    };
    if let Err(error) = external_tools::configure(tools) {
        report_failure("tools", "configureFailed", "外部程序目录没有设置上。", &error);
    }
}

fn start_plugin_host(runtime: &RepositoryRuntime, tasks: &Arc<MomoTaskRuntime>) {
    let host = Arc::new(MomoPluginRuntime::new(runtime.clone(), Arc::clone(tasks)));
    if let Err(error) = host.reload() {
        report_failure("plugin.host", "loadFailed", "插件没有加载起来，后端插件暂时不可用。", &error);
    }
    // 第一代加载失败也装上宿主：之后在插件管理页启停或安装插件时会重新加载。
    if let Err(error) = mutsuki_host::install_host(host) {
        report_failure("plugin.host", "installFailed", "插件宿主没有装上，后端插件不可用。", &error);
    }
}

/// 失败写进系统日志和标准错误（开发时看得到）。
fn report_failure(category: &str, action: &str, message: &str, error: &str) {
    eprintln!("Nana {message}{error}");
    momobako_backend::app_log!("error", category, action, message, json!({ "error": error }));
}

#[cfg(test)]
#[path = "plugin_host_tests.rs"]
mod tests;
