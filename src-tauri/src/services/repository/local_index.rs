//! Local filesystem indexing and fallback search helpers.

use super::*;
use jwalk::{Error as JwalkError, WalkDir};

pub(super) fn collect_repository_files(repo_root: &Path) -> std::io::Result<Vec<DiscoveredFile>> {
    let mut files = Vec::new();
    if !repo_root.exists() {
        return Ok(files);
    }

    for entry in repository_recursive_walk(repo_root) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_recursive_walk_error(&error) => continue,
            Err(error) => return Err(std::io::Error::from(error)),
        };
        let path = entry.path();
        let file_name = entry.file_name().to_string_lossy();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_recursive_walk_error(&error) => continue,
            Err(error) => return Err(std::io::Error::from(error)),
        };
        if metadata.is_dir() {
            continue;
        }

        let relative = path
            .strip_prefix(repo_root)
            .ok()
            .map(|item| item.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|| file_name.to_string());
        let extension = path
            .extension()
            .map(|ext| ext.to_string_lossy().to_string())
            .unwrap_or_default();

        files.push(DiscoveredFile {
            absolute_path: Some(path),
            relative_path: relative,
            filename: file_name.to_string(),
            extension,
            size_bytes: metadata.len() as i64,
            created_at: metadata
                .created()
                .ok()
                .map(system_time_to_rfc3339)
                .transpose()
                .map_err(|error| std::io::Error::other(error.to_string()))?,
            modified_at: metadata
                .modified()
                .ok()
                .map(system_time_to_rfc3339)
                .transpose()
                .map_err(|error| std::io::Error::other(error.to_string()))?
                .unwrap_or_else(now_rfc3339),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
            status: None,
            shared_asset_id: None,
            tags: None,
            thumbnail_local_absolute_path: None,
        });
    }

    finalize_local_discovered_files(files).map_err(std::io::Error::other)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalFileSearchMode {
    Recursive,
    Ntfs,
    Everything,
}

pub(super) fn local_file_search_mode_from_config(
    config: &serde_json::Value,
) -> LocalFileSearchMode {
    match config
        .get(LOCAL_FILESYSTEM_FILE_SEARCH_MODE_KEY)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
    {
        Some("ntfs") => LocalFileSearchMode::Ntfs,
        Some("everything") => LocalFileSearchMode::Everything,
        _ => LocalFileSearchMode::Recursive,
    }
}

pub(super) fn collect_repository_files_with_mode(
    repo_root: &Path,
    config: &serde_json::Value,
) -> Result<Vec<DiscoveredFile>, String> {
    match local_file_search_mode_from_config(config) {
        LocalFileSearchMode::Recursive => collect_repository_files(repo_root).map_err(io_error),
        LocalFileSearchMode::Ntfs => {
            collect_repository_files_with_fallback(repo_root, "NTFS", collect_repository_files_ntfs)
        }
        LocalFileSearchMode::Everything => collect_repository_files_with_fallback(
            repo_root,
            "Everything",
            collect_repository_files_everything,
        ),
    }
}

pub(super) fn collect_repository_files_with_fallback(
    repo_root: &Path,
    label: &str,
    collect: fn(&Path) -> Result<Vec<DiscoveredFile>, String>,
) -> Result<Vec<DiscoveredFile>, String> {
    match collect(repo_root) {
        Ok(files) if files.is_empty() && repository_contains_visible_entries(repo_root) => {
            crate::app_log!(
                "warn",
                "repository.index",
                "fallbackToRecursiveScan",
                "索引文件搜索结果为空，已退回递归扫描。",
                serde_json::json!({
                    "label": label,
                    "repoRoot": repo_root.to_string_lossy().to_string(),
                })
            );
            collect_repository_files(repo_root).map_err(io_error)
        }
        Ok(files) => Ok(files),
        Err(error) => {
            crate::app_log!(
                "warn",
                "repository.index",
                "searchUnavailable",
                "索引文件搜索不可用，已退回递归扫描。",
                serde_json::json!({
                    "label": label,
                    "repoRoot": repo_root.to_string_lossy().to_string(),
                    "error": error,
                })
            );
            collect_repository_files(repo_root).map_err(io_error)
        }
    }
}

fn repository_contains_visible_entries(repo_root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(repo_root) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }
        return true;
    }
    false
}

pub(super) fn collect_repository_files_everything(
    repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    collect_repository_files_everything_impl(repo_root)
}

#[cfg(windows)]
pub(super) fn collect_repository_files_everything_impl(
    repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    use std::time::Duration;

    use everything_ipc::{
        search::normalize_path_ev,
        wm::{EverythingClient, RequestFlags},
    };

    let query_root = canonical_repository_index_root(repo_root);
    let search_root = normalize_path_ev(&query_root).display().to_string();
    let query = format!(r#"file: path:"{search_root}\*""#);
    let everything = EverythingClient::new().map_err(|error| error.to_string())?;
    let list = everything
        .query_wait(&query)
        .request_flags(RequestFlags::FullPathAndFileName)
        .timeout(Duration::from_secs(10))
        .call()
        .map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    for item in list.iter() {
        let path = item
            .get_string(RequestFlags::FullPathAndFileName)
            .map(PathBuf::from)
            .ok_or_else(|| "Everything result is missing full path".to_string())?;
        push_local_discovered_file(&query_root, &path, &mut files)?;
    }
    finalize_local_discovered_files(files)
}

#[cfg(not(windows))]
pub(super) fn collect_repository_files_everything_impl(
    _repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    Err("Everything file search is only available on Windows".to_string())
}

#[cfg(windows)]
pub(super) fn collect_repository_files_ntfs(
    repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    use ntfs_reader::{file_info::FileInfo, mft::Mft, volume::Volume};

    let volume_name = windows_volume_name(repo_root)
        .ok_or_else(|| "repository path has no Windows drive prefix".to_string())?;
    let query_root = canonical_repository_index_root(repo_root);
    let volume = Volume::new(format!(r"\\.\{volume_name}:")).map_err(|error| error.to_string())?;
    let mft = Mft::new(volume).map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    for file in mft.files() {
        let info = FileInfo::new(&mft, &file);
        let path = normalize_ntfs_info_path(&info.path, &volume_name);
        if info.is_directory || !path_is_under_repository_root(&query_root, &path) {
            continue;
        }
        push_local_discovered_file(&query_root, &path, &mut files)?;
    }
    finalize_local_discovered_files(files)
}

#[cfg(not(windows))]
pub(super) fn collect_repository_files_ntfs(
    _repo_root: &Path,
) -> Result<Vec<DiscoveredFile>, String> {
    Err("NTFS file search is only available on Windows".to_string())
}

pub(super) fn push_local_discovered_file(
    repo_root: &Path,
    path: &Path,
    files: &mut Vec<DiscoveredFile>,
) -> Result<(), String> {
    if !path.is_file()
        || !path_is_under_repository_root(repo_root, path)
        || is_inside_internal_repository_dir(repo_root, path)
    {
        return Ok(());
    }
    let metadata = fs::metadata(path).map_err(io_error)?;
    let relative_path = indexed_repository_relative_path(repo_root, path)?;
    files.push(DiscoveredFile {
        absolute_path: Some(path.to_path_buf()),
        relative_path: relative_path.clone(),
        filename: path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or(relative_path),
        extension: path
            .extension()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default(),
        size_bytes: metadata.len() as i64,
        created_at: None,
        modified_at: metadata
            .modified()
            .map_err(io_error)
            .and_then(|value| system_time_to_rfc3339(value).map_err(time_error))?,
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: Some(path.to_string_lossy().to_string()),
        status: None,
        shared_asset_id: None,
        tags: None,
        thumbnail_local_absolute_path: None,
    });
    Ok(())
}

pub(super) fn is_inside_internal_repository_dir(repo_root: &Path, path: &Path) -> bool {
    indexed_repository_relative_path(repo_root, path)
        .map(|relative| {
            Path::new(&relative).components().any(|component| {
                matches!(component, Component::Normal(name) if is_internal_repository_dir(&name.to_string_lossy()))
            })
        })
        .unwrap_or(false)
}

#[cfg(windows)]
pub(super) fn windows_volume_name(path: &Path) -> Option<String> {
    use std::path::Prefix;

    for component in path.components() {
        if let Component::Prefix(prefix) = component {
            if let Prefix::Disk(value) = prefix.kind() {
                return Some((value as char).to_string());
            }
        }
    }
    None
}

#[cfg(windows)]
pub(super) fn normalize_ntfs_info_path(path: &Path, volume_name: &str) -> PathBuf {
    let text = path.to_string_lossy();
    let prefix = format!(r"\\.\{volume_name}:");
    if let Some(rest) = text.strip_prefix(&prefix) {
        return PathBuf::from(format!("{volume_name}:{rest}"));
    }
    path.to_path_buf()
}

pub(super) fn canonical_repository_index_root(repo_root: &Path) -> PathBuf {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    normalize_windows_verbatim_path(&root)
}

pub(super) fn normalize_windows_verbatim_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let text = path.to_string_lossy();
        if let Some(rest) = text.strip_prefix("\\\\?\\UNC\\") {
            return PathBuf::from(format!("\\\\{rest}"));
        }
        if let Some(rest) = text.strip_prefix("\\\\?\\") {
            return PathBuf::from(rest);
        }
    }

    path.to_path_buf()
}

pub(super) fn path_is_under_repository_root(repo_root: &Path, path: &Path) -> bool {
    indexed_repository_relative_path(repo_root, path).is_ok()
}

pub(super) fn indexed_repository_relative_path(
    repo_root: &Path,
    path: &Path,
) -> Result<String, String> {
    #[cfg(windows)]
    {
        let root_text = repo_root.to_string_lossy().replace('\\', "/");
        let path_text = path.to_string_lossy().replace('\\', "/");
        let root_text = root_text.trim_end_matches('/');
        let root_cmp = root_text.to_lowercase();
        let path_cmp = path_text.to_lowercase();
        if path_cmp == root_cmp {
            return Ok(String::new());
        }
        let prefix = format!("{root_cmp}/");
        if path_cmp.starts_with(&prefix) {
            return Ok(path_text[root_text.len() + 1..].to_string());
        }
    }

    path.strip_prefix(repo_root)
        .map_err(path_error)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
}

pub(super) fn finalize_local_discovered_files(
    files: Vec<DiscoveredFile>,
) -> Result<Vec<DiscoveredFile>, String> {
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for file in files {
        if seen.insert(file.relative_path.clone()) {
            unique.push(file);
        }
    }
    unique.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(unique)
}

pub(super) fn count_repository_directories(repo_root: &Path) -> Result<i64, String> {
    if !repo_root.exists() {
        return Ok(0);
    }

    let mut total = 0;
    for entry in repository_recursive_walk(repo_root) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) if is_skippable_recursive_walk_error(&error) => continue,
            Err(error) => return Err(io_error(std::io::Error::from(error))),
        };
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) if is_skippable_recursive_walk_error(&error) => continue,
            Err(error) => return Err(io_error(std::io::Error::from(error))),
        };
        if metadata.is_dir() {
            total += 1;
        }
    }

    Ok(total)
}

pub(super) fn read_repository_readme(repo_root: &Path) -> Result<Option<String>, String> {
    for candidate in ["README.md", "readme.md"] {
        let path = repo_root.join(candidate);
        if path.is_file() {
            let content = fs::read_to_string(path).map_err(io_error)?;
            return Ok(Some(content));
        }
    }

    Ok(None)
}

fn repository_recursive_walk(repo_root: &Path) -> WalkDir {
    WalkDir::new(repo_root)
        .min_depth(1)
        .skip_hidden(false)
        .follow_links(true)
        .process_read_dir(|_, _, _, children| {
            children.retain(|entry| {
                entry
                    .as_ref()
                    .map(|entry| !is_internal_repository_dir(&entry.file_name.to_string_lossy()))
                    .unwrap_or(true)
            });
        })
}

fn is_skippable_recursive_walk_error(error: &JwalkError) -> bool {
    error.depth() > 0 && error.io_error().is_some_and(is_skippable_filesystem_error)
}
