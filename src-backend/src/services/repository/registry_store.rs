//! Registry records, repository metadata, backend config, and storage roots.

use super::*;

mod backend;
mod initialize;
mod migration;
mod netease_cache;

pub(super) use backend::*;
pub(super) use initialize::*;
pub(super) use migration::*;
pub(super) use netease_cache::*;

pub(super) fn infer_repository_name(repo_root: &Path) -> String {
    repo_root
        .file_name()
        .map(|name| name.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string())
}

pub(super) fn repository_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(REPO_META_DIR)
}

pub(super) fn repository_trash_dir(repo_root: &Path) -> PathBuf {
    repository_meta_dir(repo_root).join(REPO_TRASH_DIR)
}

pub(super) fn repository_trash_manifest_path(repo_root: &Path) -> PathBuf {
    repository_meta_dir(repo_root).join(REPO_TRASH_MANIFEST_FILE_NAME)
}

pub(super) fn legacy_repository_meta_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(LEGACY_REPO_META_DIR)
}

pub(super) fn is_internal_repository_dir(name: &str) -> bool {
    name == REPO_META_DIR || name == LEGACY_REPO_META_DIR
}

pub(super) fn migrate_legacy_meta_dir_if_needed(
    repo_root: &Path,
    _backend_plugin_id: &str,
) -> Result<(), String> {
    let current_dir = repository_meta_dir(repo_root);
    if current_dir.exists() {
        hide_repository_meta_dir(&current_dir);
        return Ok(());
    }

    let legacy_dir = legacy_repository_meta_dir(repo_root);
    if legacy_dir.exists() {
        fs::rename(&legacy_dir, &current_dir).map_err(io_error)?;
        hide_repository_meta_dir(&current_dir);
    }

    Ok(())
}

/// 把 `.momo` 设成隐藏。已经隐藏就不动：每次打开存储都起 attrib 进程很慢，改属性还会触发一次文件监听事件。
pub(super) fn hide_repository_meta_dir(path: &Path) {
    #[cfg(not(target_os = "windows"))]
    let _ = path;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if fs::metadata(path)
            .is_ok_and(|metadata| metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0)
        {
            return;
        }
        let failure = match child_process::command("attrib")
            .arg("+H")
            .arg(path)
            .status()
        {
            Ok(status) if status.success() => None,
            Ok(status) => Some(format!("attrib exited with {status}")),
            Err(error) => Some(error.to_string()),
        };
        if let Some(error) = failure {
            crate::app_log!(
                "warn",
                "repository.storage",
                "hideMetaDirFailed",
                "隐藏资源库元数据目录失败。",
                serde_json::json!({ "path": path.to_string_lossy().to_string(), "error": error })
            );
        }
    }
}
