//! File import, copy, external download, and hardlink-copy helpers.

use super::*;

pub(super) fn ensure_local_filesystem_repository(
    repo: &RepositoryRecord,
    action: &str,
) -> Result<(), String> {
    if repo.backend_record.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID {
        Ok(())
    } else {
        Err(format!(
            "{action} is only supported for local filesystem repositories"
        ))
    }
}

pub(super) fn resolve_file_copy_target(
    repo_root: &Path,
    parent_path: Option<&str>,
    source_paths: &[String],
) -> Result<(String, PathBuf), String> {
    let parent_path = normalize_directory_path(parent_path.unwrap_or_default())?;
    let target_dir = resolve_repository_relative_path(repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }
    if source_paths.is_empty() {
        return Err("no source files were provided".to_string());
    }
    Ok((parent_path, target_dir))
}

#[derive(Debug, Clone)]
pub(super) struct FileImportPlanEntry {
    pub(super) source: PathBuf,
    pub(super) source_relative_path: Option<String>,
    pub(super) target: PathBuf,
    pub(super) target_relative_path: String,
    pub(super) is_directory: bool,
}

#[derive(Debug, Clone)]
pub(super) struct FileMovePlanEntry {
    pub(super) source_relative_path: String,
    pub(super) target_relative_path: String,
    pub(super) target_name: String,
    pub(super) is_directory: bool,
}

pub(super) fn validate_external_import_entries(
    source_paths: &[String],
    repo_root: &Path,
    target_dir: &Path,
) -> Result<Vec<FileImportPlanEntry>, String> {
    let repo_canonical = repo_root.canonicalize().map_err(io_error)?;
    let target_canonical_parent = target_dir.canonicalize().map_err(io_error)?;
    let mut planned_targets = Vec::<PathBuf>::new();
    let mut plan = Vec::with_capacity(source_paths.len());

    for source_path in source_paths {
        let source = PathBuf::from(source_path);
        if !source.exists() {
            return Err(format!(
                "source path does not exist: {}",
                source.to_string_lossy()
            ));
        }

        let name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid source path: {}", source.to_string_lossy()))?;
        let name = validate_new_entry_name(&name)?;
        let target = target_dir.join(&name);
        let target_relative_path = target
            .strip_prefix(repo_root)
            .map_err(path_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if target.exists() || planned_targets.iter().any(|planned| planned == &target) {
            return Err(format!("entry already exists: {name}"));
        }

        if source.is_dir() {
            let source_canonical = source.canonicalize().map_err(io_error)?;
            if source_canonical == repo_canonical || repo_canonical.starts_with(&source_canonical) {
                return Err("cannot import a repository folder into itself".to_string());
            }
            if target_canonical_parent.starts_with(&source_canonical) {
                return Err("cannot import a folder into one of its descendants".to_string());
            }
            plan.push(FileImportPlanEntry {
                source: source_canonical,
                source_relative_path: None,
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: true,
            });
        } else if source.is_file() {
            plan.push(FileImportPlanEntry {
                source,
                source_relative_path: None,
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: false,
            });
        } else {
            return Err(format!(
                "unsupported source path type: {}",
                source.to_string_lossy()
            ));
        }
        planned_targets.push(target);
    }

    Ok(plan)
}

pub(super) fn validate_repository_copy_entries(
    source_paths: &[String],
    repo_root: &Path,
    target_dir: &Path,
) -> Result<Vec<FileImportPlanEntry>, String> {
    let target_canonical_parent = target_dir.canonicalize().map_err(io_error)?;
    let mut planned_targets = Vec::<PathBuf>::new();
    let mut plan = Vec::with_capacity(source_paths.len());

    for source_path in source_paths {
        let source_relative = normalize_entry_path(source_path)?;
        let source = resolve_repository_relative_path(repo_root, &source_relative)?;
        if !source.exists() {
            return Err(format!("source path does not exist: {source_relative}"));
        }
        let source_canonical = source.canonicalize().map_err(io_error)?;
        let source_parent = source_canonical
            .parent()
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        if source_parent == target_canonical_parent {
            return Err("不能复制到原目录".to_string());
        }
        let name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        let name = validate_new_entry_name(&name)?;
        let target = target_dir.join(&name);
        let target_relative_path = target
            .strip_prefix(repo_root)
            .map_err(path_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if target.exists() || planned_targets.iter().any(|planned| planned == &target) {
            return Err(format!("entry already exists: {name}"));
        }

        if source.is_dir() {
            if target_canonical_parent.starts_with(&source_canonical) {
                return Err("cannot copy a folder into one of its descendants".to_string());
            }
            plan.push(FileImportPlanEntry {
                source: source_canonical,
                source_relative_path: Some(source_relative.clone()),
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: true,
            });
        } else if source.is_file() {
            plan.push(FileImportPlanEntry {
                source,
                source_relative_path: Some(source_relative.clone()),
                target: target.clone(),
                target_relative_path: target_relative_path.clone(),
                is_directory: false,
            });
        } else {
            return Err(format!("unsupported source path type: {source_relative}"));
        }
        planned_targets.push(target);
    }

    Ok(plan)
}

pub(super) fn validate_repository_move_entries(
    source_paths: &[String],
    repo_root: &Path,
    target_dir: &Path,
) -> Result<Vec<FileMovePlanEntry>, String> {
    let target_canonical_parent = target_dir.canonicalize().map_err(io_error)?;
    let mut planned_targets = Vec::<PathBuf>::new();
    let mut plan = Vec::with_capacity(source_paths.len());

    for source_path in source_paths {
        let source_relative = normalize_entry_path(source_path)?;
        let source = resolve_repository_relative_path(repo_root, &source_relative)?;
        if !source.exists() {
            return Err(format!("source path does not exist: {source_relative}"));
        }

        let source_canonical = source.canonicalize().map_err(io_error)?;
        let source_parent = source_canonical
            .parent()
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        if source_parent == target_canonical_parent {
            return Err("不能移动到原目录".to_string());
        }

        let name = source
            .file_name()
            .map(|value| value.to_string_lossy().to_string())
            .ok_or_else(|| format!("invalid source path: {source_relative}"))?;
        let target_name = validate_new_entry_name(&name)?;
        let target = target_dir.join(&target_name);
        let target_relative_path = target
            .strip_prefix(repo_root)
            .map_err(path_error)?
            .to_string_lossy()
            .replace('\\', "/");
        if target.exists() || planned_targets.iter().any(|planned| planned == &target) {
            return Err(format!("entry already exists: {target_name}"));
        }

        let is_directory = source.is_dir();
        if is_directory && target_canonical_parent.starts_with(&source_canonical) {
            return Err("文件夹不能移动到自身或其子文件夹内".to_string());
        }
        if !is_directory && !source.is_file() {
            return Err(format!("unsupported source path type: {source_relative}"));
        }

        planned_targets.push(target);
        plan.push(FileMovePlanEntry {
            source_relative_path: source_relative,
            target_relative_path,
            target_name,
            is_directory,
        });
    }

    Ok(plan)
}

pub(super) fn copy_external_entries_parallel(
    plan: Vec<FileImportPlanEntry>,
    hardlink_preferred: bool,
) -> Result<Vec<HardlinkCopyOutcome>, String> {
    if plan.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = plan.len().min(MAX_PARALLEL_IMPORTS);
    let queue = Arc::new(Mutex::new(plan.into_iter()));
    let outcomes = Arc::new(Mutex::new(Vec::new()));
    let mut handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let queue = queue.clone();
        let outcomes = outcomes.clone();
        handles.push(thread::spawn(move || loop {
            let Some(entry) = ({
                let mut entries = queue
                    .lock()
                    .map_err(|_| "import queue lock poisoned".to_string())?;
                entries.next()
            }) else {
                return Ok(());
            };

            let mut entry_outcomes = if entry.is_directory {
                copy_directory_recursive_with_mode(
                    &entry.source,
                    entry.source_relative_path.as_deref(),
                    &entry.target,
                    &entry.target_relative_path,
                    hardlink_preferred,
                )?
            } else {
                vec![copy_file_with_mode(
                    &entry.source,
                    entry.source_relative_path.as_deref(),
                    &entry.target,
                    &entry.target_relative_path,
                    hardlink_preferred,
                )?]
            };
            outcomes
                .lock()
                .map_err(|_| "import outcome lock poisoned".to_string())?
                .append(&mut entry_outcomes);
        }));
    }

    for handle in handles {
        match handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err("file import worker panicked".to_string()),
        }
    }

    let outcomes = Arc::try_unwrap(outcomes)
        .map_err(|_| "import outcome still shared".to_string())?
        .into_inner()
        .map_err(|_| "import outcome lock poisoned".to_string())?;
    Ok(outcomes)
}

pub(super) fn external_add_asset_response(
    request_id: String,
    mut imported: Vec<ExternalImportedAsset>,
    mut failed: Vec<ExternalAddAssetFailure>,
    total: usize,
) -> ExternalAddAssetResponse {
    imported.sort_by_key(|item| item.item_index);
    failed.sort_by_key(|item| item.item_index);
    let status = if imported.is_empty() {
        "failed"
    } else if failed.is_empty() {
        "success"
    } else {
        "partial"
    };
    ExternalAddAssetResponse {
        request_id,
        status: status.to_string(),
        summary: ExternalAddAssetSummary {
            total,
            imported: imported.len(),
            failed: failed.len(),
        },
        imported,
        failed,
    }
}

pub(super) fn external_failure(
    item_index: usize,
    code: &str,
    message: String,
    retryable: bool,
    details: Option<serde_json::Value>,
) -> ExternalAddAssetFailure {
    ExternalAddAssetFailure {
        item_index,
        code: code.to_string(),
        message,
        retryable,
        details,
    }
}

pub(super) fn external_import_error_code(error: &str) -> &'static str {
    if error.contains("entry already exists") {
        "duplicateTarget"
    } else if error.contains("directory not found")
        || error.contains("path escapes repository root")
        || error.contains("invalid path")
    {
        "invalidTargetPath"
    } else {
        "importRejected"
    }
}

pub(super) fn external_metadata_source(client: Option<&ExternalAddAssetClient>) -> String {
    let Some(client) = client else {
        return "external".to_string();
    };
    client
        .id
        .as_deref()
        .or(client.name.as_deref())
        .map(|value| format!("external:{value}"))
        .unwrap_or_else(|| "external".to_string())
}

#[derive(Debug)]
pub(super) struct ExternalRequestError {
    pub(super) code: &'static str,
    pub(super) message: String,
    pub(super) retryable: bool,
}

#[derive(Debug)]
pub(super) struct ExternalAssetImportContext {
    pub(super) parent_path: String,
    pub(super) target_dir: PathBuf,
    pub(super) staging_root: PathBuf,
}

#[derive(Debug)]
pub(super) struct PlannedExternalAsset {
    pub(super) item_index: usize,
    pub(super) source_path: String,
    pub(super) target_path: String,
    pub(super) metadata: Option<BTreeMap<String, serde_json::Value>>,
}

pub(super) fn external_item_filename(
    item: &ExternalAddAssetItem,
    item_index: usize,
) -> Result<String, String> {
    if let Some(filename) = item
        .filename
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return validate_new_entry_name(filename);
    }
    if let Some(url) = item.url.as_deref() {
        let url_path = url.split(['?', '#']).next().unwrap_or(url);
        if let Some(candidate) = url_path
            .rsplit('/')
            .find(|segment| !segment.trim().is_empty())
            .map(percent_decode_filename)
            .filter(|value| !value.trim().is_empty())
        {
            return validate_new_entry_name(&candidate);
        }
    }
    validate_new_entry_name(&format!("external-asset-{item_index}.bin"))
}

pub(super) fn percent_decode_filename(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                output.push(hex);
                index += 3;
                continue;
            }
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}

pub(super) fn stage_external_asset_item(
    item_index: usize,
    item: &ExternalAddAssetItem,
    context: &ExternalAssetImportContext,
    planned_targets: &mut HashSet<String>,
) -> Result<PlannedExternalAsset, ExternalAddAssetFailure> {
    if item.kind != "remoteUrl" {
        return Err(external_failure(
            item_index,
            "invalidInput",
            format!("unsupported item kind: {}", item.kind),
            false,
            None,
        ));
    }
    let Some(url) = item
        .url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Err(external_failure(
            item_index,
            "invalidInput",
            "remoteUrl item requires url".to_string(),
            false,
            None,
        ));
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(external_failure(
            item_index,
            "invalidInput",
            "remoteUrl only supports http and https URLs".to_string(),
            false,
            None,
        ));
    }

    let filename = external_item_filename(item, item_index)
        .map_err(|error| external_failure(item_index, "invalidInput", error, false, None))?;
    let target_path = join_relative_path(&context.parent_path, &filename);
    if context.target_dir.join(&filename).exists() || !planned_targets.insert(target_path.clone()) {
        return Err(external_failure(
            item_index,
            "duplicateTarget",
            format!("entry already exists: {filename}"),
            false,
            None,
        ));
    }

    let staged_path = context.staging_root.join(&filename);
    download_remote_asset(url, item.headers.as_ref(), &staged_path).map_err(|error| {
        external_failure(
            item_index,
            "downloadFailed",
            error,
            true,
            Some(serde_json::json!({ "url": url })),
        )
    })?;

    Ok(PlannedExternalAsset {
        item_index,
        source_path: staged_path.to_string_lossy().to_string(),
        target_path,
        metadata: item.metadata.clone(),
    })
}

pub(super) fn sanitize_external_component(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    let trimmed = sanitized.trim_matches('-');
    if trimmed.is_empty() {
        "request".to_string()
    } else {
        trimmed.to_string()
    }
}

pub(super) fn download_remote_asset(
    url: &str,
    headers: Option<&BTreeMap<String, String>>,
    output_path: &Path,
) -> Result<(), String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("remoteUrl only supports http and https URLs".to_string());
    }
    let client = reqwest::blocking::Client::builder()
        .user_agent("MomoBakoExternalImport/1")
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("download client error: {error}"))?;
    let mut request = client.get(url);
    if let Some(headers) = headers {
        for (name, value) in headers {
            if is_safe_external_header_name(name) && !value.contains(['\r', '\n']) {
                request = request.header(name, value);
            }
        }
    }
    let response = request
        .send()
        .map_err(|error| format!("download request failed: {error}"))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("download returned HTTP {status}"));
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }
    let mut file = File::create(output_path).map_err(io_error)?;
    let mut response = response;
    response
        .copy_to(&mut file)
        .map_err(|error| format!("download body error: {error}"))?;
    Ok(())
}

pub(super) fn is_safe_external_header_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-'))
}
