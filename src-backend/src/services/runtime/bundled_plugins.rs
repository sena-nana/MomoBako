//! 随附插件同步。
//!
//! 宿主随包带一组 `.momoplug`：Nana 安装版和便携版在 NanaUI 的 `runtime/plugins`，开发时是插件打包输出
//! `External/Plugins/.packages`。启动时由宿主交进这份清单，这里把服务目录的
//! `plugins/builtin` 同步成和它一模一样：内容变了的换新，清单里没有的删掉，内容没变的不动。插件目录的扫描、校验和
//! 加载仍由插件运行时从 `plugins/builtin` 读，见 `repository/plugin_runtime.rs`。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::services::repository::runtime_plugins_dir;

/// 插件包的扩展名。
const PACKAGE_EXTENSION: &str = "momoplug";

/// 一次同步做了什么，文件名按字母排序。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BundledPluginSync {
    /// 新放进去或换了新内容的包。
    pub copied: Vec<String>,
    /// 内容没变、没有动的包。
    pub unchanged: Vec<String>,
    /// 清单里已经没有、从 `plugins/builtin` 删掉的包。
    pub removed: Vec<String>,
}

/// 服务目录里随附插件的位置。
pub fn bundled_plugins_dir(service_root: &Path) -> PathBuf {
    runtime_plugins_dir(service_root).join("builtin")
}

/// 列出目录里的 `.momoplug`，按文件名排序；目录不存在时返回空清单。
pub fn list_plugin_packages(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = fs::read_dir(dir)
        .map_err(|error| format!("读不了随附插件目录 {}：{error}", dir.display()))?;
    let mut packages = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|error| format!("读不了随附插件目录 {}：{error}", dir.display()))?;
        let path = entry.path();
        if path.is_file() && is_package(&path) {
            packages.push(path);
        }
    }
    packages.sort();
    Ok(packages)
}

/// 把 `plugins/builtin` 同步成 `packages` 这份清单。
///
/// 算法：先逐个比对清单里的包，内容相同的跳过，不同的先写到同目录的 `*.new`、再换掉旧文件，所以中途失败时
/// 每个包要么是旧内容、要么是新内容；全部写好以后才删清单里没有的包。任何一步失败都立即返回错误，不删旧包。
/// 只动 `.momoplug` 文件，`plugins/builtin` 里的其他东西不碰。
pub fn sync_bundled_plugins(
    packages: &[PathBuf],
    service_root: &Path,
) -> Result<BundledPluginSync, String> {
    let target_root = bundled_plugins_dir(service_root);
    fs::create_dir_all(&target_root)
        .map_err(|error| format!("建不了随附插件目录 {}：{error}", target_root.display()))?;
    let mut report = BundledPluginSync::default();
    let mut bundled = BTreeSet::new();
    for source in packages {
        let name = package_file_name(source)?;
        if !bundled.insert(name.clone()) {
            return Err(format!("随附插件清单里有两个同名的包：{name}"));
        }
        let target = target_root.join(&name);
        if same_contents(source, &target)? {
            report.unchanged.push(name);
            continue;
        }
        replace_file(source, &target)?;
        report.copied.push(name);
    }
    let entries = fs::read_dir(&target_root)
        .map_err(|error| format!("读不了随附插件目录 {}：{error}", target_root.display()))?;
    for entry in entries {
        let entry = entry
            .map_err(|error| format!("读不了随附插件目录 {}：{error}", target_root.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !path.is_file() || !is_package(&path) || bundled.contains(&name) {
            continue;
        }
        fs::remove_file(&path)
            .map_err(|error| format!("删不掉已经不随包的插件 {}：{error}", path.display()))?;
        report.removed.push(name);
    }
    report.copied.sort();
    report.unchanged.sort();
    report.removed.sort();
    Ok(report)
}

fn is_package(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PACKAGE_EXTENSION))
}

fn package_file_name(source: &Path) -> Result<String, String> {
    if !is_package(source) {
        return Err(format!("随附插件不是 .momoplug：{}", source.display()));
    }
    source
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::to_string)
        .ok_or_else(|| format!("随附插件的文件名不是 UTF-8：{}", source.display()))
}

/// 两个文件内容相同时返回真；目标不存在时返回假。先比长度，长度相同再逐字节比。
fn same_contents(source: &Path, target: &Path) -> Result<bool, String> {
    let source_meta = fs::metadata(source)
        .map_err(|error| format!("读不了随附插件 {}：{error}", source.display()))?;
    let Ok(target_meta) = fs::metadata(target) else {
        return Ok(false);
    };
    if !target_meta.is_file() || source_meta.len() != target_meta.len() {
        return Ok(false);
    }
    let source_bytes = fs::read(source)
        .map_err(|error| format!("读不了随附插件 {}：{error}", source.display()))?;
    let target_bytes = fs::read(target)
        .map_err(|error| format!("读不了已同步的插件 {}：{error}", target.display()))?;
    Ok(source_bytes == target_bytes)
}

/// 先复制到同目录的临时文件，再换掉目标，避免留下写了一半的包。
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    let mut temporary = target.as_os_str().to_owned();
    temporary.push(".new");
    let temporary = PathBuf::from(temporary);
    fs::copy(source, &temporary).map_err(|error| {
        format!("复制随附插件 {} 失败：{error}", source.display())
    })?;
    if target.exists() {
        if let Err(error) = fs::remove_file(target) {
            let _ = fs::remove_file(&temporary);
            return Err(format!("换不掉旧的插件 {}：{error}", target.display()));
        }
    }
    fs::rename(&temporary, target).map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("放不进随附插件 {}：{error}", target.display())
    })
}

#[cfg(test)]
#[path = "bundled_plugins_tests.rs"]
mod tests;
