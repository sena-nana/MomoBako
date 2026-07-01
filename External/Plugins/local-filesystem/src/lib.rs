use std::{
    collections::HashSet,
    ffi::CString,
    fs::{self, OpenOptions},
    os::raw::c_char,
    path::{Component, Path, PathBuf},
};

use momobako_backend_plugin_sdk::{free_c_string, read_request, response_error, response_ok};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MANIFEST: &str = include_str!("../manifest.json");
const FILE_SEARCH_MODE_KEY: &str = "fileSearchMode";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveredFile {
    absolute_path: PathBuf,
    relative_path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    modified_at: String,
    is_virtual: bool,
    provider_id: Option<String>,
    provider_item_id: Option<String>,
    source_payload: Option<serde_json::Value>,
    local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
enum FileSystemEntryKind {
    Directory,
    File,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileSystemEntry {
    path: String,
    name: String,
    kind: FileSystemEntryKind,
    extension: Option<String>,
    size_bytes: Option<i64>,
    modified_at: Option<String>,
    is_virtual: bool,
    provider_id: Option<String>,
    provider_item_id: Option<String>,
    source_payload: Option<serde_json::Value>,
    local_absolute_path: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileTreeNode {
    path: String,
    label: String,
    children: Vec<FileTreeNode>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DirectoryPageResult {
    entries: Vec<FileSystemEntry>,
    total_entries: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileSystemPayload {
    repo_root: PathBuf,
    directory_path: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    parent_path: Option<String>,
    target_parent_path: Option<String>,
    entry_path: Option<String>,
    source_path: Option<String>,
    name: Option<String>,
    new_name: Option<String>,
    recursive: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileSearchMode {
    Recursive,
    Ntfs,
    Everything,
}

#[no_mangle]
pub extern "C" fn momobako_plugin_manifest() -> *mut c_char {
    CString::new(MANIFEST)
        .expect("manifest should not contain null bytes")
        .into_raw()
}

#[no_mangle]
pub extern "C" fn momobako_plugin_call(input: *const c_char) -> *mut c_char {
    match handle_call(input) {
        Ok(value) => response_ok(value),
        Err(error) => response_error(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn momobako_plugin_free(value: *mut c_char) {
    unsafe { free_c_string(value) };
}

fn handle_call(input: *const c_char) -> Result<serde_json::Value, String> {
    let request = read_request(input)?;
    let file_search_mode =
        file_search_mode_from_config(request.runtime.plugin_config.get(FILE_SEARCH_MODE_KEY));
    let payload: FileSystemPayload =
        serde_json::from_value(request.payload).map_err(|error| error.to_string())?;
    match request.method.as_str() {
        "filesystem.ensureAttachable" => {
            ensure_attachable(&payload.repo_root)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.prepareRepositoryRoot" => {
            fs::create_dir_all(&payload.repo_root).map_err(io_error)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.listFiles" => {
            let files = collect_files_with_mode(&payload.repo_root, file_search_mode)?;
            serde_json::to_value(files).map_err(|error| error.to_string())
        }
        "filesystem.listTree" => {
            let tree = build_directory_tree(&payload.repo_root)?;
            serde_json::to_value(tree).map_err(|error| error.to_string())
        }
        "filesystem.listDirectory" => {
            let directory_path = payload.directory_path.as_deref().unwrap_or_default();
            let current_dir = resolve_relative_path(&payload.repo_root, directory_path)?;
            if !current_dir.exists() || !current_dir.is_dir() {
                return Err(format!("directory not found: {directory_path}"));
            }
            let entries = local_directory_entries(&payload.repo_root, &current_dir)?;
            serde_json::to_value(entries).map_err(|error| error.to_string())
        }
        "filesystem.listDirectoryPage" => {
            let directory_path = payload.directory_path.as_deref().unwrap_or_default();
            let current_dir = resolve_relative_path(&payload.repo_root, directory_path)?;
            if !current_dir.exists() || !current_dir.is_dir() {
                return Err(format!("directory not found: {directory_path}"));
            }
            let entries = local_directory_entries(&payload.repo_root, &current_dir)?;
            let total_entries = entries.len();
            let offset = payload.offset.unwrap_or(0).min(total_entries);
            let limit = payload.limit.unwrap_or(usize::MAX);
            let page = DirectoryPageResult {
                entries: entries.into_iter().skip(offset).take(limit).collect(),
                total_entries,
            };
            serde_json::to_value(page).map_err(|error| error.to_string())
        }
        "filesystem.createDirectory" => {
            let parent_path = payload.parent_path.as_deref().unwrap_or_default();
            let name = payload.name.as_deref().ok_or("missing name")?;
            let parent_dir = resolve_relative_path(&payload.repo_root, parent_path)?;
            let target_dir = parent_dir.join(name);
            if target_dir.exists() {
                return Err(format!("entry already exists: {name}"));
            }
            fs::create_dir(&target_dir).map_err(io_error)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.createFile" => {
            let parent_path = payload.parent_path.as_deref().unwrap_or_default();
            let name = payload.name.as_deref().ok_or("missing name")?;
            let parent_dir = resolve_relative_path(&payload.repo_root, parent_path)?;
            let target_file = parent_dir.join(name);
            if target_file.exists() {
                return Err(format!("entry already exists: {name}"));
            }
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(target_file)
                .map_err(io_error)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.statEntry" => {
            let entry_path = payload.entry_path.as_deref().ok_or("missing entryPath")?;
            let entry = stat_entry(&payload.repo_root, entry_path)?;
            serde_json::to_value(entry).map_err(|error| error.to_string())
        }
        "filesystem.renameEntry" => {
            let source_path = payload.source_path.as_deref().ok_or("missing sourcePath")?;
            let new_name = payload.new_name.as_deref().ok_or("missing newName")?;
            let source_abs = resolve_relative_path(&payload.repo_root, source_path)?;
            if !source_abs.exists() {
                return Err(format!("entry not found: {source_path}"));
            }
            let target_abs = source_abs
                .parent()
                .ok_or_else(|| "cannot rename repository root".to_string())?
                .join(new_name);
            if target_abs.exists() {
                return Err(format!("entry already exists: {new_name}"));
            }
            fs::rename(&source_abs, &target_abs).map_err(io_error)?;
            let target_path = join_relative_path(&parent_relative_path(source_path), new_name);
            let entry = stat_entry(&payload.repo_root, &target_path)?;
            serde_json::to_value(entry).map_err(|error| error.to_string())
        }
        "filesystem.moveEntry" => {
            let source_path = payload.source_path.as_deref().ok_or("missing sourcePath")?;
            let target_parent_path = payload
                .target_parent_path
                .as_deref()
                .ok_or("missing targetParentPath")?;
            let source_abs = resolve_relative_path(&payload.repo_root, source_path)?;
            if !source_abs.exists() {
                return Err(format!("entry not found: {source_path}"));
            }
            let target_parent_abs = resolve_relative_path(&payload.repo_root, target_parent_path)?;
            if !target_parent_abs.exists() || !target_parent_abs.is_dir() {
                return Err(format!("directory not found: {target_parent_path}"));
            }
            let name = source_abs
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .ok_or_else(|| format!("invalid source path: {source_path}"))?;
            let target_abs = target_parent_abs.join(&name);
            if target_abs.exists() {
                return Err(format!("entry already exists: {name}"));
            }
            fs::rename(&source_abs, &target_abs).map_err(io_error)?;
            let target_path = join_relative_path(target_parent_path, &name);
            let entry = stat_entry(&payload.repo_root, &target_path)?;
            serde_json::to_value(entry).map_err(|error| error.to_string())
        }
        "filesystem.deleteEntry" => {
            let entry_path = payload.entry_path.as_deref().ok_or("missing entryPath")?;
            let entry_abs = resolve_relative_path(&payload.repo_root, entry_path)?;
            if !entry_abs.exists() {
                return Err(format!("entry not found: {entry_path}"));
            }
            if payload.recursive.unwrap_or(false) {
                fs::remove_dir_all(entry_abs).map_err(io_error)?;
            } else if entry_abs.is_dir() {
                fs::remove_dir(entry_abs).map_err(io_error)?;
            } else {
                fs::remove_file(entry_abs).map_err(io_error)?;
            }
            Ok(serde_json::json!({}))
        }
        method => Err(format!("unsupported method: {method}")),
    }
}

fn ensure_attachable(repo_root: &Path) -> Result<(), String> {
    if !repo_root.exists() {
        return Err(format!(
            "repository folder does not exist: {}",
            repo_root.to_string_lossy()
        ));
    }
    if !repo_root.is_dir() {
        return Err(format!(
            "repository path is not a folder: {}",
            repo_root.to_string_lossy()
        ));
    }
    Ok(())
}

fn collect_files(repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
    let mut files = Vec::new();
    collect_files_recursive(repo_root, repo_root, &mut files)?;
    Ok(files)
}

fn collect_files_with_mode(
    repo_root: &Path,
    mode: FileSearchMode,
) -> Result<Vec<DiscoveredFile>, String> {
    match mode {
        FileSearchMode::Recursive => collect_files(repo_root),
        FileSearchMode::Ntfs => collect_files_with_fallback(repo_root, "NTFS", collect_files_ntfs),
        FileSearchMode::Everything => {
            collect_files_with_fallback(repo_root, "Everything", collect_files_everything)
        }
    }
}

fn collect_files_with_fallback(
    repo_root: &Path,
    label: &str,
    collect: fn(&Path) -> Result<Vec<DiscoveredFile>, String>,
) -> Result<Vec<DiscoveredFile>, String> {
    match collect(repo_root) {
        Ok(files) => Ok(files),
        Err(error) => {
            eprintln!(
                "[local-filesystem] {label} file search unavailable for {}: {error}; falling back to recursive scan",
                repo_root.to_string_lossy()
            );
            collect_files(repo_root)
        }
    }
}

fn file_search_mode_from_config(value: Option<&Value>) -> FileSearchMode {
    match value.and_then(Value::as_str).map(str::trim) {
        Some("ntfs") => FileSearchMode::Ntfs,
        Some("everything") => FileSearchMode::Everything,
        _ => FileSearchMode::Recursive,
    }
}

fn collect_files_everything(repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
    collect_files_everything_impl(repo_root)
}

#[cfg(windows)]
fn collect_files_everything_impl(repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
    use std::time::Duration;

    use everything_ipc::{
        search::normalize_path_ev,
        wm::{EverythingClient, RequestFlags},
    };

    let query_root = canonical_index_root(repo_root);
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
        push_discovered_file(&query_root, &path, &mut files)?;
    }
    finalize_discovered_files(files)
}

#[cfg(not(windows))]
fn collect_files_everything_impl(_repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
    Err("Everything file search is only available on Windows".to_string())
}

#[cfg(windows)]
fn collect_files_ntfs(repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
    use ntfs_reader::{file_info::FileInfo, mft::Mft, volume::Volume};

    let volume_name = windows_volume_name(repo_root)
        .ok_or_else(|| "repository path has no Windows drive prefix".to_string())?;
    let query_root = canonical_index_root(repo_root);
    let volume = Volume::new(format!(r"\\.\{volume_name}:")).map_err(|error| error.to_string())?;
    let mft = Mft::new(volume).map_err(|error| error.to_string())?;
    let mut files = Vec::new();
    for file in mft.files() {
        let info = FileInfo::new(&mft, &file);
        let path = normalize_ntfs_info_path(&info.path, &volume_name);
        if info.is_directory || !path_is_under_root(&query_root, &path) {
            continue;
        }
        push_discovered_file(&query_root, &path, &mut files)?;
    }
    finalize_discovered_files(files)
}

#[cfg(not(windows))]
fn collect_files_ntfs(_repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
    Err("NTFS file search is only available on Windows".to_string())
}

fn push_discovered_file(
    repo_root: &Path,
    path: &Path,
    files: &mut Vec<DiscoveredFile>,
) -> Result<(), String> {
    if !path.is_file()
        || !path_is_under_root(repo_root, path)
        || is_inside_internal_repository_dir(repo_root, path)
    {
        return Ok(());
    }
    let metadata = fs::metadata(path).map_err(io_error)?;
    let relative_path = indexed_relative_path(repo_root, path)?;
    files.push(DiscoveredFile {
        absolute_path: path.to_path_buf(),
        filename: path
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| relative_path.clone()),
        extension: path
            .extension()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default(),
        size_bytes: metadata.len() as i64,
        modified_at: metadata
            .modified()
            .map_err(io_error)
            .and_then(system_time_to_rfc3339)?,
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: Some(path.to_string_lossy().to_string()),
        relative_path,
    });
    Ok(())
}

fn is_inside_internal_repository_dir(repo_root: &Path, path: &Path) -> bool {
    indexed_relative_path(repo_root, path)
        .map(|relative| {
            Path::new(&relative).components().any(|component| {
                matches!(component, Component::Normal(name) if is_internal_repository_dir(&name.to_string_lossy()))
            })
        })
        .unwrap_or(false)
}

#[cfg(windows)]
fn windows_volume_name(path: &Path) -> Option<String> {
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
fn normalize_ntfs_info_path(path: &Path, volume_name: &str) -> PathBuf {
    let text = path.to_string_lossy();
    let prefix = format!(r"\\.\{volume_name}:");
    if let Some(rest) = text.strip_prefix(&prefix) {
        return PathBuf::from(format!("{volume_name}:{rest}"));
    }
    path.to_path_buf()
}

fn canonical_index_root(repo_root: &Path) -> PathBuf {
    let root = repo_root
        .canonicalize()
        .unwrap_or_else(|_| repo_root.to_path_buf());
    normalize_verbatim_path(&root)
}

fn normalize_verbatim_path(path: &Path) -> PathBuf {
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

fn path_is_under_root(repo_root: &Path, path: &Path) -> bool {
    indexed_relative_path(repo_root, path).is_ok()
}

fn indexed_relative_path(repo_root: &Path, path: &Path) -> Result<String, String> {
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

    relative_path(repo_root, path)
}

fn finalize_discovered_files(files: Vec<DiscoveredFile>) -> Result<Vec<DiscoveredFile>, String> {
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

fn collect_files_recursive(
    repo_root: &Path,
    current: &Path,
    files: &mut Vec<DiscoveredFile>,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if is_internal_repository_dir(&name) {
                continue;
            }
            collect_files_recursive(repo_root, &path, files)?;
            continue;
        }
        if !path.is_file() {
            continue;
        }
        let metadata = fs::metadata(&path).map_err(io_error)?;
        let relative_path = relative_path(repo_root, &path)?;
        files.push(DiscoveredFile {
            absolute_path: path.clone(),
            filename: path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| relative_path.clone()),
            extension: path
                .extension()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default(),
            size_bytes: metadata.len() as i64,
            modified_at: metadata
                .modified()
                .map_err(io_error)
                .and_then(system_time_to_rfc3339)?,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: Some(path.to_string_lossy().to_string()),
            relative_path,
        });
    }
    Ok(())
}

fn build_directory_tree(repo_root: &Path) -> Result<Vec<FileTreeNode>, String> {
    let mut children = Vec::new();
    for entry in fs::read_dir(repo_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        if !metadata.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        children.push(build_directory_node(repo_root, &name)?);
    }

    children.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(children)
}

fn build_directory_node(repo_root: &Path, relative_path: &str) -> Result<FileTreeNode, String> {
    let abs_path = resolve_relative_path(repo_root, relative_path)?;
    let mut children = Vec::new();

    for entry in fs::read_dir(&abs_path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let metadata = entry.metadata().map_err(io_error)?;
        if !metadata.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }

        let child_path = join_relative_path(relative_path, &name);
        children.push(build_directory_node(repo_root, &child_path)?);
    }

    children.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(FileTreeNode {
        path: relative_path.to_string(),
        label: Path::new(relative_path)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| relative_path.to_string()),
        children,
    })
}

fn local_directory_entries(
    repo_root: &Path,
    current_dir: &Path,
) -> Result<Vec<FileSystemEntry>, String> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(current_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() && is_internal_repository_dir(&name) {
            continue;
        }
        let metadata = fs::metadata(&path).map_err(io_error)?;
        entries.push(FileSystemEntry {
            path: relative_path(repo_root, &path)?,
            name,
            kind: if metadata.is_dir() {
                FileSystemEntryKind::Directory
            } else {
                FileSystemEntryKind::File
            },
            extension: if metadata.is_file() {
                path.extension()
                    .map(|value| value.to_string_lossy().to_string())
            } else {
                None
            },
            size_bytes: if metadata.is_file() {
                Some(metadata.len() as i64)
            } else {
                None
            },
            modified_at: metadata
                .modified()
                .ok()
                .map(system_time_to_rfc3339)
                .transpose()?,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: Some(path.to_string_lossy().to_string()),
        });
    }
    entries.sort_by(|left, right| {
        let left_dir = matches!(left.kind, FileSystemEntryKind::Directory);
        let right_dir = matches!(right.kind, FileSystemEntryKind::Directory);
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(entries)
}

fn stat_entry(repo_root: &Path, entry_path: &str) -> Result<FileSystemEntry, String> {
    let entry_abs = resolve_relative_path(repo_root, entry_path)?;
    if !entry_abs.exists() {
        return Err(format!("entry not found: {entry_path}"));
    }
    let metadata = fs::metadata(&entry_abs).map_err(io_error)?;
    Ok(FileSystemEntry {
        path: entry_path.to_string(),
        name: entry_abs
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_else(|| entry_path.to_string()),
        kind: if metadata.is_dir() {
            FileSystemEntryKind::Directory
        } else {
            FileSystemEntryKind::File
        },
        extension: if metadata.is_file() {
            entry_abs
                .extension()
                .map(|value| value.to_string_lossy().to_string())
        } else {
            None
        },
        size_bytes: if metadata.is_file() {
            Some(metadata.len() as i64)
        } else {
            None
        },
        modified_at: metadata
            .modified()
            .ok()
            .map(system_time_to_rfc3339)
            .transpose()?,
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: Some(entry_abs.to_string_lossy().to_string()),
    })
}

fn resolve_relative_path(repo_root: &Path, path: &str) -> Result<PathBuf, String> {
    let relative = path.trim().replace('\\', "/");
    let mut target = repo_root.to_path_buf();
    for part in relative.split('/').filter(|part| !part.is_empty()) {
        if part == "." || part == ".." {
            return Err(format!("invalid repository-relative path: {path}"));
        }
        target.push(part);
    }
    Ok(target)
}

fn relative_path(repo_root: &Path, path: &Path) -> Result<String, String> {
    path.strip_prefix(repo_root)
        .map_err(|error| error.to_string())
        .map(|value| value.to_string_lossy().replace('\\', "/"))
}

fn parent_relative_path(path: &str) -> String {
    path.rfind('/')
        .map(|index| path[..index].to_string())
        .unwrap_or_default()
}

fn join_relative_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

fn is_internal_repository_dir(value: &str) -> bool {
    matches!(value, ".momo" | ".meta")
}

fn system_time_to_rfc3339(value: std::time::SystemTime) -> Result<String, String> {
    let datetime: OffsetDateTime = value.into();
    datetime.format(&Rfc3339).map_err(|error| error.to_string())
}

fn io_error(error: std::io::Error) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestWorkspace {
        root: PathBuf,
    }

    impl TestWorkspace {
        fn new(name: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after unix epoch")
                .as_nanos();
            let root = std::env::temp_dir().join(format!("momobako-plugin-{name}-{unique}"));
            fs::create_dir_all(&root).expect("test root should be created");
            Self { root }
        }
    }

    impl Drop for TestWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn discovered_file_serializes_virtual_compat_fields() {
        let file = DiscoveredFile {
            absolute_path: PathBuf::from("C:/Repo/demo.mp3"),
            relative_path: "demo.mp3".to_string(),
            filename: "demo.mp3".to_string(),
            extension: "mp3".to_string(),
            size_bytes: 12,
            modified_at: "2026-06-14T00:00:00Z".to_string(),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: Some("C:/Repo/demo.mp3".to_string()),
        };

        let value = serde_json::to_value(file).expect("file should serialize");

        assert_eq!(value.get("isVirtual"), Some(&serde_json::json!(false)));
        assert_eq!(
            value.get("localAbsolutePath"),
            Some(&serde_json::json!("C:/Repo/demo.mp3"))
        );
        assert_eq!(value.get("providerId"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("providerItemId"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("sourcePayload"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn filesystem_entry_serializes_virtual_compat_fields() {
        let entry = FileSystemEntry {
            path: "Albums".to_string(),
            name: "Albums".to_string(),
            kind: FileSystemEntryKind::Directory,
            extension: None,
            size_bytes: None,
            modified_at: Some("2026-06-14T00:00:00Z".to_string()),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: Some("C:/Repo/Albums".to_string()),
        };

        let value = serde_json::to_value(entry).expect("entry should serialize");

        assert_eq!(value.get("isVirtual"), Some(&serde_json::json!(false)));
        assert_eq!(
            value.get("localAbsolutePath"),
            Some(&serde_json::json!("C:/Repo/Albums"))
        );
        assert_eq!(value.get("providerId"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("providerItemId"), Some(&serde_json::Value::Null));
        assert_eq!(value.get("sourcePayload"), Some(&serde_json::Value::Null));
    }

    #[test]
    fn file_search_mode_config_defaults_to_recursive() {
        assert_eq!(
            file_search_mode_from_config(None),
            FileSearchMode::Recursive
        );
        assert_eq!(
            file_search_mode_from_config(Some(&serde_json::json!("unknown"))),
            FileSearchMode::Recursive
        );
        assert_eq!(
            file_search_mode_from_config(Some(&serde_json::json!("ntfs"))),
            FileSearchMode::Ntfs
        );
        assert_eq!(
            file_search_mode_from_config(Some(&serde_json::json!("everything"))),
            FileSearchMode::Everything
        );
    }

    #[test]
    fn recursive_search_keeps_existing_file_metadata_semantics() {
        let workspace = TestWorkspace::new("recursive-search");
        let repo_root = &workspace.root;
        fs::create_dir_all(repo_root.join("music")).expect("music dir should be created");
        fs::create_dir_all(repo_root.join(".momo")).expect("meta dir should be created");
        fs::create_dir_all(repo_root.join(".meta")).expect("legacy meta dir should be created");
        fs::write(repo_root.join("music").join("demo.flac"), b"audio")
            .expect("file should be written");
        fs::write(repo_root.join(".momo").join("hidden.flac"), b"skip")
            .expect("hidden file should be written");
        fs::write(repo_root.join(".meta").join("legacy.flac"), b"skip")
            .expect("legacy file should be written");

        let files = collect_files_with_mode(repo_root, FileSearchMode::Recursive)
            .expect("recursive search should succeed");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "music/demo.flac");
        assert_eq!(files[0].filename, "demo.flac");
        assert_eq!(files[0].extension, "flac");
        assert_eq!(files[0].size_bytes, 5);
        assert!(files[0].modified_at.contains('T'));
    }

    #[test]
    fn list_directory_page_returns_paginated_entries() {
        let workspace = TestWorkspace::new("directory-page");
        fs::create_dir_all(workspace.root.join("albums")).expect("albums dir should be created");
        fs::write(workspace.root.join("albums").join("b.flac"), b"b").expect("b should be written");
        fs::write(workspace.root.join("albums").join("a.flac"), b"a").expect("a should be written");

        let entries = local_directory_entries(&workspace.root, &workspace.root.join("albums"))
            .expect("directory entries should load");
        let total_entries = entries.len();
        let page = DirectoryPageResult {
            entries: entries.into_iter().skip(1).take(1).collect(),
            total_entries,
        };

        let value = serde_json::to_value(page).expect("page should serialize");

        assert_eq!(value.get("totalEntries"), Some(&serde_json::json!(2)));
        let entries = value
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .expect("entries should be an array");
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn unavailable_index_search_falls_back_to_recursive_scan() {
        fn unavailable(_repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
            Err("not available".to_string())
        }

        let workspace = TestWorkspace::new("fallback-search");
        fs::write(workspace.root.join("demo.mp3"), b"audio").expect("file should be written");

        let files = collect_files_with_fallback(&workspace.root, "Test", unavailable)
            .expect("fallback search should succeed");

        assert_eq!(files.len(), 1);
        assert_eq!(files[0].relative_path, "demo.mp3");
    }
}
