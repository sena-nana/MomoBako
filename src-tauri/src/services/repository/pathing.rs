//! Repository path normalization and trash path resolution.

use super::*;

pub(super) fn normalize_repository_root_for_backend(
    service_root: &Path,
    path: &str,
    backend: &RepositoryBackendRecord,
    must_exist: bool,
) -> Result<PathBuf, String> {
    let repo_root = PathBuf::from(path);
    let plugin_registry = plugin_catalog(service_root);
    let backend_summary = backend_summary_from_registry(&plugin_registry, &backend.plugin_id);
    if !backend_summary_supports_local_root_access(&backend_summary) {
        return Ok(repo_root);
    }

    if must_exist || repo_root.exists() {
        return canonicalize_local_path(&repo_root);
    }

    if let Some(parent) = repo_root.parent() {
        if parent.exists() {
            let parent = canonicalize_local_path(parent)?;
            if let Some(name) = repo_root.file_name() {
                return Ok(parent.join(name));
            }
        }
    }

    if repo_root.is_relative() {
        return Ok(std::env::current_dir().map_err(io_error)?.join(repo_root));
    }

    Ok(repo_root)
}

pub(super) fn canonicalize_local_path(path: &Path) -> Result<PathBuf, String> {
    let canonical = path.canonicalize().map_err(io_error)?;
    Ok(strip_windows_verbatim_prefix(canonical))
}

#[cfg(target_os = "windows")]
pub(super) fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(stripped) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{stripped}"));
    }
    if let Some(stripped) = value.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }
    path
}

#[cfg(not(target_os = "windows"))]
pub(super) fn strip_windows_verbatim_prefix(path: PathBuf) -> PathBuf {
    path
}

pub(super) fn normalize_directory_path(path: &str) -> Result<String, String> {
    normalize_relative_path(path, true)
}

pub(super) fn normalize_entry_path(path: &str) -> Result<String, String> {
    let normalized = normalize_relative_path(path, false)?;
    if normalized.is_empty() {
        return Err("path cannot be empty".to_string());
    }
    Ok(normalized)
}

pub(super) fn normalize_special_location(value: Option<&str>) -> Result<Option<String>, String> {
    match value.map(str::trim).filter(|item| !item.is_empty()) {
        Some("trash") => Ok(Some("trash".to_string())),
        Some(value) => Err(format!("unsupported file browser location: {value}")),
        None => Ok(None),
    }
}

pub(super) fn normalize_relative_path(path: &str, allow_empty: bool) -> Result<String, String> {
    let trimmed = path.trim().replace('\\', "/").trim_matches('/').to_string();
    if trimmed.is_empty() {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err("path cannot be empty".to_string())
        };
    }

    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err("path cannot escape repository root".to_string());
        }
        if is_internal_repository_dir(part) {
            return Err("internal repository directory is reserved".to_string());
        }
        parts.push(part);
    }

    let normalized = parts.join("/");
    if normalized.is_empty() && allow_empty {
        Ok(String::new())
    } else if normalized.is_empty() {
        Err("path cannot be empty".to_string())
    } else {
        Ok(normalized)
    }
}

pub(super) fn resolve_repository_relative_path(
    repo_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    if relative_path.is_empty() {
        return Ok(repo_root.to_path_buf());
    }

    let normalized = normalize_relative_path(relative_path, true)?;
    let mut path = repo_root.to_path_buf();
    for part in normalized.split('/') {
        if !part.is_empty() {
            path.push(part);
        }
    }
    Ok(path)
}

pub(super) fn resolve_trash_relative_path(
    trash_root: &Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    if relative_path.is_empty() {
        return Ok(trash_root.to_path_buf());
    }

    let normalized = normalize_trash_relative_path(relative_path, true)?;
    let mut path = trash_root.to_path_buf();
    for part in normalized.split('/') {
        if !part.is_empty() {
            path.push(part);
        }
    }
    Ok(path)
}

pub(super) fn normalize_trash_relative_path(
    path: &str,
    allow_empty: bool,
) -> Result<String, String> {
    let trimmed = path.trim().replace('\\', "/").trim_matches('/').to_string();
    if trimmed.is_empty() {
        return if allow_empty {
            Ok(String::new())
        } else {
            Err("path cannot be empty".to_string())
        };
    }

    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return Err("path cannot escape trash root".to_string());
        }
        parts.push(part);
    }

    let normalized = parts.join("/");
    if normalized.is_empty() && allow_empty {
        Ok(String::new())
    } else if normalized.is_empty() {
        Err("path cannot be empty".to_string())
    } else {
        Ok(normalized)
    }
}

pub(super) fn unique_trash_target_path(
    trash_root: &Path,
    entry_path: &str,
) -> Result<PathBuf, String> {
    let entry = Path::new(entry_path);
    let name = entry
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .ok_or_else(|| format!("invalid entry path: {entry_path}"))?;

    let parent_path = parent_relative_path(entry_path);
    let target_parent = resolve_trash_relative_path(trash_root, &parent_path)?;
    fs::create_dir_all(&target_parent).map_err(io_error)?;

    let mut target = target_parent.join(&name);
    if !target.exists() {
        return Ok(target);
    }

    let stem = entry
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| name.clone());
    let extension = entry
        .extension()
        .map(|value| value.to_string_lossy().to_string());
    let timestamp = trash_timestamp_suffix();
    let mut suffix = 1;
    while target.exists() {
        let candidate_name = match &extension {
            Some(extension) => format!("{stem} (deleted-{timestamp}-{suffix}).{extension}"),
            None => format!("{stem} (deleted-{timestamp}-{suffix})"),
        };
        target = target_parent.join(candidate_name);
        suffix += 1;
    }
    Ok(target)
}

pub(super) fn trash_timestamp_suffix() -> String {
    now_rfc3339()
        .replace(':', "")
        .replace('.', "-")
        .replace('Z', "z")
}

pub(super) fn trash_relative_path_for_target(
    trash_root: &Path,
    target_abs: &Path,
) -> Result<String, String> {
    target_abs
        .strip_prefix(trash_root)
        .map_err(path_error)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
}
