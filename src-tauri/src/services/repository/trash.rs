//! Trash manifest persistence and restore safety helpers.

use super::*;

pub(super) fn load_trash_manifest(repo_root: &Path) -> Result<TrashManifest, String> {
    let manifest_path = repository_trash_manifest_path(repo_root);
    if !manifest_path.exists() {
        return Ok(TrashManifest::default());
    }

    let raw = fs::read_to_string(manifest_path).map_err(io_error)?;
    if raw.trim().is_empty() {
        return Ok(TrashManifest::default());
    }
    serde_json::from_str::<TrashManifest>(&raw).map_err(json_error)
}

pub(super) fn save_trash_manifest(
    repo_root: &Path,
    manifest: &TrashManifest,
) -> Result<(), String> {
    let meta_dir = repository_meta_dir(repo_root);
    fs::create_dir_all(&meta_dir).map_err(io_error)?;
    let manifest_json = serde_json::to_string_pretty(manifest).map_err(json_error)?;
    fs::write(repository_trash_manifest_path(repo_root), manifest_json).map_err(io_error)
}

pub(super) fn trash_path_matches_or_descends(path: &str, ancestor: &str) -> bool {
    path == ancestor || path.starts_with(&format!("{ancestor}/"))
}

pub(super) fn relative_suffix(path: &str, ancestor: &str) -> Option<String> {
    if path == ancestor {
        Some(String::new())
    } else {
        path.strip_prefix(&format!("{ancestor}/"))
            .map(ToString::to_string)
    }
}

pub(super) fn find_trash_manifest_entry<'a>(
    manifest: &'a TrashManifest,
    trash_path: &str,
) -> Option<&'a TrashManifestEntry> {
    manifest
        .entries
        .iter()
        .filter(|entry| trash_path_matches_or_descends(trash_path, &entry.trash_path))
        .max_by_key(|entry| entry.trash_path.len())
}

pub(super) fn original_path_for_trash_path(entry: &TrashManifestEntry, trash_path: &str) -> String {
    match relative_suffix(trash_path, &entry.trash_path) {
        Some(suffix) if suffix.is_empty() => entry.original_path.clone(),
        Some(suffix) => join_relative_path(&entry.original_path, &suffix),
        None => entry.original_path.clone(),
    }
}

pub(super) fn remove_manifest_paths(manifest: &mut TrashManifest, trash_path: &str) {
    manifest
        .entries
        .retain(|entry| !trash_path_matches_or_descends(&entry.trash_path, trash_path));
}

pub(super) fn prune_empty_trash_parents(
    trash_root: &Path,
    restored_trash_path: &str,
) -> Result<(), String> {
    let mut current = parent_relative_path(restored_trash_path);
    while !current.is_empty() {
        let dir = resolve_trash_relative_path(trash_root, &current)?;
        if dir.exists() && dir.is_dir() && fs::read_dir(&dir).map_err(io_error)?.next().is_none() {
            fs::remove_dir(&dir).map_err(io_error)?;
            current = parent_relative_path(&current);
        } else {
            break;
        }
    }
    Ok(())
}

pub(super) fn ensure_restore_target_available(
    source_abs: &Path,
    target_abs: &Path,
    target_path: &str,
) -> Result<(), String> {
    if !target_abs.exists() {
        return Ok(());
    }
    let source_metadata = source_abs.metadata().map_err(io_error)?;
    if source_metadata.is_dir() && target_abs.is_dir() {
        return ensure_directory_merge_available(source_abs, target_abs, target_path);
    }
    Err(format!("target already exists: {target_path}"))
}

pub(super) fn ensure_directory_merge_available(
    source_dir: &Path,
    target_dir: &Path,
    target_path: &str,
) -> Result<(), String> {
    for entry in fs::read_dir(source_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let source_child = entry.path();
        let target_child = target_dir.join(entry.file_name());
        if !target_child.exists() {
            continue;
        }
        let source_metadata = entry.metadata().map_err(io_error)?;
        if source_metadata.is_dir() && target_child.is_dir() {
            ensure_directory_merge_available(&source_child, &target_child, target_path)?;
        } else {
            return Err(format!("target already exists: {target_path}"));
        }
    }
    Ok(())
}

pub(super) fn restore_path_to_target(
    source_abs: &Path,
    target_abs: &Path,
    target_path: &str,
) -> Result<(), String> {
    ensure_restore_target_available(source_abs, target_abs, target_path)?;
    if let Some(parent) = target_abs.parent() {
        fs::create_dir_all(parent).map_err(io_error)?;
    }

    if source_abs.is_dir() && target_abs.is_dir() {
        merge_directory_contents(source_abs, target_abs)
    } else {
        fs::rename(source_abs, target_abs).map_err(io_error)
    }
}

pub(super) fn merge_directory_contents(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    fs::create_dir_all(target_dir).map_err(io_error)?;
    for entry in fs::read_dir(source_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let source_child = entry.path();
        let target_child = target_dir.join(entry.file_name());
        if source_child.is_dir() && target_child.is_dir() {
            merge_directory_contents(&source_child, &target_child)?;
        } else {
            fs::rename(&source_child, &target_child).map_err(io_error)?;
        }
    }
    fs::remove_dir(source_dir).map_err(io_error)
}
