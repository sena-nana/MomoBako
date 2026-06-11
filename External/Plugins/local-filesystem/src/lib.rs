use std::{
    ffi::CString,
    fs::{self, OpenOptions},
    os::raw::c_char,
    path::{Path, PathBuf},
};

use momobako_backend_plugin_sdk::{free_c_string, read_request, response_error, response_ok};
use serde::{Deserialize, Serialize};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MANIFEST: &str = include_str!("../manifest.json");

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiscoveredFile {
    absolute_path: PathBuf,
    relative_path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    modified_at: String,
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
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FileTreeNode {
    path: String,
    label: String,
    children: Vec<FileTreeNode>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileSystemPayload {
    repo_root: PathBuf,
    directory_path: Option<String>,
    parent_path: Option<String>,
    target_parent_path: Option<String>,
    entry_path: Option<String>,
    source_path: Option<String>,
    name: Option<String>,
    new_name: Option<String>,
    recursive: Option<bool>,
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
            let files = collect_files(&payload.repo_root)?;
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
