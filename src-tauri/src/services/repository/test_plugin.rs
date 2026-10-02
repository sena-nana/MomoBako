//! 仅供后端单元测试使用的本地文件系统插件协议适配器。

use super::*;
use std::cell::Cell;

thread_local! {
    static LOCAL_FILESYSTEM_ADAPTER_ENABLED: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn enable_local_filesystem_adapter() {
    LOCAL_FILESYSTEM_ADAPTER_ENABLED.with(|enabled| enabled.set(true));
}

pub(crate) fn disable_local_filesystem_adapter() {
    LOCAL_FILESYSTEM_ADAPTER_ENABLED.with(|enabled| enabled.set(false));
}

pub(crate) fn local_filesystem_adapter_enabled() -> bool {
    LOCAL_FILESYSTEM_ADAPTER_ENABLED.with(Cell::get)
}

pub(crate) fn call_local_filesystem(
    method: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let repo_root = payload
        .get("repoRoot")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| format!("plugin call is missing repoRoot for {method}"))?;
    match method {
        "filesystem.ensureAttachable" => {
            if !repo_root.is_dir() {
                return Err(format!(
                    "repository folder does not exist: {}",
                    repo_root.to_string_lossy()
                ));
            }
            Ok(serde_json::json!({}))
        }
        "filesystem.prepareRepositoryRoot" => {
            fs::create_dir_all(&repo_root).map_err(io_error)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.listFiles" => serde_json::to_value(collect_files(&repo_root)?).map_err(json_error),
        "filesystem.listTree" => serde_json::to_value(build_tree(&repo_root)?).map_err(json_error),
        "filesystem.listDirectory" => {
            let path = payload_str(&payload, "directoryPath");
            serde_json::to_value(list_entries(&repo_root, path)?).map_err(json_error)
        }
        "filesystem.listDirectoryPage" => {
            let path = payload_str(&payload, "directoryPath");
            let offset = payload_usize(&payload, "offset");
            let limit = payload_usize(&payload, "limit");
            let entries = list_entries(&repo_root, path)?;
            let total_entries = entries.len();
            Ok(serde_json::json!({
                "entries": entries.into_iter().skip(offset).take(limit).collect::<Vec<_>>(),
                "totalEntries": total_entries,
            }))
        }
        "filesystem.createDirectory" => {
            let parent = resolve_repository_relative_path(&repo_root, payload_str(&payload, "parentPath"))?;
            let name = required_payload_str(&payload, "name")?;
            let target = parent.join(name);
            if target.exists() {
                return Err(format!("entry already exists: {name}"));
            }
            fs::create_dir(target).map_err(io_error)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.createFile" => {
            let parent = resolve_repository_relative_path(&repo_root, payload_str(&payload, "parentPath"))?;
            let name = required_payload_str(&payload, "name")?;
            let target = parent.join(name);
            if target.exists() {
                return Err(format!("entry already exists: {name}"));
            }
            OpenOptions::new().create_new(true).write(true).open(target).map_err(io_error)?;
            Ok(serde_json::json!({}))
        }
        "filesystem.statEntry" => {
            let path = required_payload_str(&payload, "entryPath")?;
            serde_json::to_value(stat_entry(&repo_root, path)?).map_err(json_error)
        }
        "filesystem.renameEntry" => {
            let source = required_payload_str(&payload, "sourcePath")?;
            let new_name = required_payload_str(&payload, "newName")?;
            let source_abs = resolve_repository_relative_path(&repo_root, source)?;
            let target_abs = source_abs
                .parent()
                .ok_or_else(|| "cannot rename repository root".to_string())?
                .join(new_name);
            if target_abs.exists() {
                return Err(format!("entry already exists: {new_name}"));
            }
            fs::rename(source_abs, target_abs).map_err(io_error)?;
            let target = join_relative_path(&parent_relative_path(source), new_name);
            serde_json::to_value(stat_entry(&repo_root, &target)?).map_err(json_error)
        }
        "filesystem.moveEntry" => {
            let source = required_payload_str(&payload, "sourcePath")?;
            let target_parent = required_payload_str(&payload, "targetParentPath")?;
            let source_abs = resolve_repository_relative_path(&repo_root, source)?;
            let name = source_abs
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .ok_or_else(|| format!("invalid source path: {source}"))?;
            let target_dir = resolve_repository_relative_path(&repo_root, target_parent)?;
            let target_abs = target_dir.join(&name);
            if target_abs.exists() {
                return Err(format!("entry already exists: {name}"));
            }
            fs::rename(source_abs, target_abs).map_err(io_error)?;
            let target = join_relative_path(target_parent, &name);
            serde_json::to_value(stat_entry(&repo_root, &target)?).map_err(json_error)
        }
        "filesystem.deleteEntry" => {
            let path = required_payload_str(&payload, "entryPath")?;
            let target = resolve_repository_relative_path(&repo_root, path)?;
            if !target.exists() {
                return Err(format!("entry not found: {path}"));
            }
            if payload.get("recursive").and_then(serde_json::Value::as_bool).unwrap_or(false) {
                fs::remove_dir_all(target).map_err(io_error)?;
            } else {
                fs::remove_file(target).map_err(io_error)?;
            }
            Ok(serde_json::json!({}))
        }
        _ => Err(format!("unsupported filesystem plugin method: {method}")),
    }
}

fn payload_str<'a>(payload: &'a serde_json::Value, key: &str) -> &'a str {
    payload.get(key).and_then(serde_json::Value::as_str).unwrap_or_default()
}

fn required_payload_str<'a>(payload: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    payload
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("plugin call is missing {key}"))
}

fn payload_usize(payload: &serde_json::Value, key: &str) -> usize {
    payload
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(usize::MAX)
}

fn collect_files(repo_root: &Path) -> Result<Vec<DiscoveredFile>, String> {
    let mut files = Vec::new();
    collect_files_recursive(repo_root, repo_root, &mut files)?;
    files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(files)
}

fn collect_files_recursive(root: &Path, current: &Path, files: &mut Vec<DiscoveredFile>) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(root, &path, files)?;
        } else if path.is_file() {
            let metadata = fs::metadata(&path).map_err(io_error)?;
            let relative = path.strip_prefix(root).map_err(path_error)?.to_string_lossy().replace('\\', "/");
            files.push(DiscoveredFile {
                absolute_path: Some(path.clone()),
                filename: name,
                extension: path.extension().map(|value| value.to_string_lossy().to_string()).unwrap_or_default(),
                relative_path: relative,
                size_bytes: metadata.len() as i64,
                created_at: None,
                modified_at: metadata.modified().ok().map(system_time_to_rfc3339).transpose().map_err(time_error)?.unwrap_or_else(now_rfc3339),
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
        }
    }
    Ok(())
}

fn build_tree(repo_root: &Path) -> Result<Vec<FileTreeNode>, String> {
    let mut nodes = Vec::new();
    for entry in fs::read_dir(repo_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() && !is_internal_repository_dir(&name) {
            nodes.push(build_tree_node(repo_root, &name)?);
        }
    }
    nodes.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(nodes)
}

fn build_tree_node(repo_root: &Path, relative: &str) -> Result<FileTreeNode, String> {
    let path = resolve_repository_relative_path(repo_root, relative)?;
    let mut children = Vec::new();
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.path().is_dir() && !is_internal_repository_dir(&name) {
            children.push(build_tree_node(repo_root, &join_relative_path(relative, &name))?);
        }
    }
    children.sort_by(|left, right| left.label.to_lowercase().cmp(&right.label.to_lowercase()));
    Ok(FileTreeNode {
        path: relative.to_string(),
        label: Path::new(relative).file_name().map(|value| value.to_string_lossy().to_string()).unwrap_or_else(|| relative.to_string()),
        file_count: count_files(repo_root, relative)?,
        children,
    })
}

fn count_files(repo_root: &Path, relative: &str) -> Result<usize, String> {
    let path = resolve_repository_relative_path(repo_root, relative)?;
    let mut count = 0;
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }
        count += if entry.path().is_dir() {
            count_files(repo_root, &join_relative_path(relative, &name))?
        } else {
            1
        };
    }
    Ok(count)
}

fn list_entries(repo_root: &Path, directory_path: &str) -> Result<Vec<FileSystemEntry>, String> {
    let current = resolve_repository_relative_path(repo_root, directory_path)?;
    if !current.is_dir() {
        return Err(format!("directory not found: {directory_path}"));
    }
    let mut entries = Vec::new();
    for entry in fs::read_dir(current).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_repository_dir(&name) {
            continue;
        }
        entries.push(entry_to_file_system_entry(repo_root, &entry.path())?);
    }
    entries.sort_by(|left, right| left.path.to_lowercase().cmp(&right.path.to_lowercase()));
    Ok(entries)
}

fn stat_entry(repo_root: &Path, relative: &str) -> Result<FileSystemEntry, String> {
    let path = resolve_repository_relative_path(repo_root, relative)?;
    if !path.exists() {
        return Err(format!("entry not found: {relative}"));
    }
    entry_to_file_system_entry(repo_root, &path)
}

fn entry_to_file_system_entry(repo_root: &Path, path: &Path) -> Result<FileSystemEntry, String> {
    let metadata = fs::metadata(path).map_err(io_error)?;
    let relative = path.strip_prefix(repo_root).map_err(path_error)?.to_string_lossy().replace('\\', "/");
    let name = path.file_name().map(|value| value.to_string_lossy().to_string()).unwrap_or_else(|| relative.clone());
    Ok(FileSystemEntry {
        path: relative,
        name,
        kind: if metadata.is_dir() { FileSystemEntryKind::Directory } else { FileSystemEntryKind::File },
        extension: metadata.is_file().then(|| path.extension().map(|value| value.to_string_lossy().to_string())).flatten(),
        size_bytes: metadata.is_file().then_some(metadata.len() as i64),
        modified_at: metadata.modified().ok().map(system_time_to_rfc3339).transpose().map_err(time_error)?,
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: Some(path.to_string_lossy().to_string()),
        status: None,
        shared_asset_id: None,
        tags: None,
        thumbnail_local_absolute_path: None,
    })
}
