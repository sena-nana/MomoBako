//! ZIP 与 Eagle 资源库导入服务。
//!
//! 这里集中处理两类“导入到当前仓库目录”的宿主能力：
//! 1. ZIP 解压导入
//! 2. EagleLibrary 转换后合并导入

use super::*;

#[derive(Debug, Clone)]
struct ArchivePlanEntry {
    archive_name: Option<String>,
    target_relative_path: String,
    is_directory: bool,
}

pub(super) fn import_archive_entries_cancellable(
    state: &RepositoryState,
    request: FileArchiveImportRequest,
    cancellation: &dyn CancellationCheck,
) -> Result<FileBrowserSnapshot, String> {
    cancellation.checkpoint()?;
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_repository_supports_local_write_access(&repo, "importing archives")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
    let target_dir = resolve_repository_relative_path(&repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }

    let archive_path = canonicalize_local_path(Path::new(request.archive_path.trim()))?;
    if !archive_path.is_file() {
        return Err(format!(
            "archive file not found: {}",
            archive_path.to_string_lossy()
        ));
    }
    let extension = archive_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_default();
    if extension != "zip" {
        return Err("only .zip archives are supported".to_string());
    }

    let plan = plan_archive_import(&archive_path, &repo_root, &parent_path)?;
    if let Err(error) = execute_archive_import(&archive_path, &repo_root, &plan, cancellation) {
        let _ = state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
        return Err(error);
    }
    let snapshot = finish_import_operation(
        state,
        &request.repo_id,
        parent_path,
        plan_has_directories(&plan),
    )?;
    cancellation.checkpoint()?;
    Ok(snapshot)
}

pub(super) fn import_eagle_library_cancellable(
    state: &RepositoryState,
    request: EagleLibraryImportRequest,
    cancellation: &dyn CancellationCheck,
) -> Result<EagleLibraryImportResponse, String> {
    cancellation.checkpoint()?;
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_repository_supports_local_write_access(&repo, "importing Eagle libraries")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
    let target_dir = resolve_repository_relative_path(&repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }

    let response_value = plugin_catalog(&state.root).call(
        "momobako.service.eagle-importer",
        "eagleImporter.importLibrary",
        serde_json::to_value(&request).map_err(json_error)?,
    )?;
    let plugin_response =
        serde_json::from_value::<EagleLibraryPluginImportResponse>(response_value)
            .map_err(json_error)?;

    // 插件调用本身不可中断；返回后先恢复索引一致性，再对外观察取消。
    state.sync_repository(SyncRequest {
        repo_id: request.repo_id.clone(),
    })?;
    cancellation.checkpoint()?;
    let snapshot = state.load_file_browser(FileBrowserRequest {
        repo_id: request.repo_id.clone(),
        directory_path: Some(parent_path),
        include_tree: Some(true),
        special_location: None,
        offset: None,
        limit: None,
    })?;
    Ok(EagleLibraryImportResponse {
        snapshot,
        summary: plugin_response.summary,
        warnings: plugin_response.warnings,
    })
}

fn finish_import_operation(
    state: &RepositoryState,
    repo_id: &str,
    parent_path: String,
    include_tree: bool,
) -> Result<FileBrowserSnapshot, String> {
    state.sync_repository(SyncRequest {
        repo_id: repo_id.to_string(),
    })?;
    state.load_file_browser(FileBrowserRequest {
        repo_id: repo_id.to_string(),
        directory_path: Some(parent_path),
        include_tree: Some(include_tree),
        special_location: None,
        offset: None,
        limit: None,
    })
}

fn plan_has_directories(plan: &[ArchivePlanEntry]) -> bool {
    plan.iter().any(|entry| entry.is_directory)
}

fn plan_archive_import(
    archive_path: &Path,
    repo_root: &Path,
    parent_path: &str,
) -> Result<Vec<ArchivePlanEntry>, String> {
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("invalid zip archive: {error}"))?;
    let mut planned = BTreeMap::<String, ArchivePlanEntry>::new();

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| format!("failed to read zip entry: {error}"))?;
        let archive_name = entry.name().to_string();
        let normalized = normalize_archive_entry_path(&archive_name)?;
        if normalized.is_empty() {
            continue;
        }
        register_archive_plan_entry(
            &mut planned,
            parent_path,
            &normalized,
            entry.is_dir(),
            Some(archive_name.clone()),
        )?;
        for ancestor in archive_directory_ancestors(&normalized) {
            register_archive_plan_entry(&mut planned, parent_path, &ancestor, true, None)?;
        }
    }

    for target_relative_path in planned.keys() {
        let target_abs = resolve_repository_relative_path(repo_root, target_relative_path)?;
        if target_abs.exists() {
            let name = Path::new(target_relative_path)
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| target_relative_path.clone());
            return Err(format!("entry already exists: {name}"));
        }
    }

    Ok(planned.into_values().collect())
}

fn register_archive_plan_entry(
    planned: &mut BTreeMap<String, ArchivePlanEntry>,
    parent_path: &str,
    normalized_path: &str,
    is_directory: bool,
    archive_name: Option<String>,
) -> Result<(), String> {
    let target_relative_path = prefix_relative_path(parent_path, normalized_path);
    if let Some(existing) = planned.get(&target_relative_path) {
        if existing.is_directory == is_directory {
            return Ok(());
        }
        return Err(format!(
            "archive target type conflict: {target_relative_path}"
        ));
    }

    for ancestor in archive_directory_ancestors(&target_relative_path) {
        if let Some(existing) = planned.get(&ancestor) {
            if !existing.is_directory {
                return Err(format!("archive target conflicts with file: {ancestor}"));
            }
        }
    }
    if !is_directory {
        let prefix = format!("{target_relative_path}/");
        if planned.keys().any(|path| path.starts_with(&prefix)) {
            return Err(format!(
                "archive target conflicts with directory: {target_relative_path}"
            ));
        }
    }

    planned.insert(
        target_relative_path.clone(),
        ArchivePlanEntry {
            archive_name,
            target_relative_path,
            is_directory,
        },
    );
    Ok(())
}

fn normalize_archive_entry_path(path: &str) -> Result<String, String> {
    let replaced = path.replace('\\', "/");
    let candidate = Path::new(&replaced);
    let mut parts = Vec::new();
    for component in candidate.components() {
        match component {
            Component::Normal(value) => {
                let part = value.to_string_lossy().trim().to_string();
                if part.is_empty() {
                    continue;
                }
                parts.push(validate_new_entry_name(&part)?);
            }
            Component::CurDir => {}
            Component::ParentDir => return Err("zip entry path cannot contain ..".to_string()),
            Component::RootDir | Component::Prefix(_) => {
                return Err("zip entry path cannot be absolute".to_string());
            }
        }
    }
    Ok(parts.join("/"))
}

fn archive_directory_ancestors(path: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = parent_relative_path(path);
    while !current.is_empty() {
        result.push(current.clone());
        current = parent_relative_path(&current);
    }
    result.reverse();
    result
}

fn execute_archive_import(
    archive_path: &Path,
    repo_root: &Path,
    plan: &[ArchivePlanEntry],
    cancellation: &dyn CancellationCheck,
) -> Result<(), String> {
    cancellation.checkpoint()?;
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("invalid zip archive: {error}"))?;

    let mut directories = plan
        .iter()
        .filter(|entry| entry.is_directory)
        .cloned()
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| entry.target_relative_path.len());
    for entry in directories {
        cancellation.checkpoint()?;
        let target_abs = resolve_repository_relative_path(repo_root, &entry.target_relative_path)?;
        fs::create_dir_all(target_abs).map_err(io_error)?;
    }

    let files = plan
        .iter()
        .filter(|entry| !entry.is_directory)
        .collect::<Vec<_>>();
    for entry in files {
        cancellation.checkpoint()?;
        let archive_name = entry.archive_name.as_deref().ok_or_else(|| {
            format!(
                "archive entry missing source: {}",
                entry.target_relative_path
            )
        })?;
        let mut source = archive
            .by_name(archive_name)
            .map_err(|error| format!("failed to read zip entry {archive_name}: {error}"))?;
        let target_abs = resolve_repository_relative_path(repo_root, &entry.target_relative_path)?;
        if let Some(parent) = target_abs.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        extract_file_atomically(&mut source, &target_abs, cancellation)?;
    }
    Ok(())
}

/// 解压到临时文件，取消或失败时不暴露残缺目标。
fn extract_file_atomically(
    source: &mut dyn Read,
    target: &Path,
    cancellation: &dyn CancellationCheck,
) -> Result<(), String> {
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("entry");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = target.with_file_name(format!(
        ".{file_name}.momobako-part-{}-{nonce}",
        std::process::id()
    ));
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error)?;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            cancellation.checkpoint()?;
            let read = source.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).map_err(io_error)?;
        }
        output.flush().map_err(io_error)?;
        cancellation.checkpoint()?;
        fs::rename(&temporary, target).map_err(io_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn prefix_relative_path(parent_path: &str, path: &str) -> String {
    let normalized = normalize_relative_path(path, true)
        .unwrap_or_else(|_| path.trim().replace('\\', "/").trim_matches('/').to_string());
    match (parent_path.is_empty(), normalized.is_empty()) {
        (true, true) => String::new(),
        (true, false) => normalized,
        (false, true) => parent_path.to_string(),
        (false, false) => join_relative_path(parent_path, &normalized),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_archive_entry_path, prefix_relative_path};

    #[test]
    fn normalize_archive_entry_path_rejects_parent_segments() {
        let error =
            normalize_archive_entry_path("../escape.txt").expect_err("path traversal should fail");
        assert!(error.contains(".."));
    }

    #[test]
    fn normalize_archive_entry_path_rejects_absolute_paths() {
        let error =
            normalize_archive_entry_path("/escape.txt").expect_err("absolute path should fail");
        assert!(error.contains("absolute"));
    }

    #[test]
    fn prefix_relative_path_prefixes_nested_entries() {
        assert_eq!(
            prefix_relative_path("imports/eagle", "images/demo.png"),
            "imports/eagle/images/demo.png"
        );
        assert_eq!(prefix_relative_path("imports/eagle", ""), "imports/eagle");
        assert_eq!(
            prefix_relative_path("", "images/demo.png"),
            "images/demo.png"
        );
    }
}
