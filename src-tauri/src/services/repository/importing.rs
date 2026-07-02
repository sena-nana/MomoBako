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

#[derive(Debug, Clone)]
struct EagleVisibleEntry {
    source_abs: PathBuf,
    target_relative_path: String,
    is_directory: bool,
}

#[derive(Debug, Clone)]
struct EagleTrashEntryPlan {
    source_abs: PathBuf,
    target_original_path: String,
    target_trash_path: String,
    kind: String,
    deleted_at: String,
}

#[derive(Debug, Clone)]
struct EagleMergePlan {
    visible_entries: Vec<EagleVisibleEntry>,
    trash_entries: Vec<EagleTrashEntryPlan>,
    include_tree: bool,
    summary: EagleLibraryImportSummary,
}

#[derive(Debug, Clone)]
struct SourceRepositoryIdentity {
    repo_id: String,
    name: String,
}

#[derive(Debug, Clone)]
struct SourceAssetRow {
    asset_id: String,
    path: String,
    filename: String,
    extension: String,
    size_bytes: i64,
    created_at: String,
    modified_at: String,
    hash: Option<String>,
    status: String,
    version: i64,
    updated_at: String,
    last_accessed_at: Option<String>,
    thumbnail_path: Option<String>,
    is_virtual: bool,
    provider_id: Option<String>,
    provider_item_id: Option<String>,
    source_payload_json: Option<String>,
    local_absolute_path: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportedAssetTarget {
    asset_id: String,
    path: String,
    status: String,
}

#[derive(Debug, Clone)]
struct SourceMetadataRow {
    key: String,
    value_type: String,
    value_json: String,
    version: i64,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct SourceTagRow {
    tag: String,
    normalized_tag: String,
}

#[derive(Debug, Clone)]
struct SourceAliasGroupRow {
    alias_group_id: String,
    source: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct SourceAliasMemberRow {
    alias_group_id: String,
    asset_id: String,
    path: String,
    role: String,
    created_at: String,
}

#[derive(Debug, Clone)]
struct SourceHardlinkMemberRow {
    group_id: String,
    asset_id: String,
    path: String,
    link_state: String,
    content_hash: String,
    size_bytes: i64,
}

#[derive(Debug, Clone)]
struct SourceFolderMetadataRow {
    path: String,
    protected: bool,
    password_tip: Option<String>,
    updated_at: String,
}

#[derive(Debug, Clone)]
struct SourceEntryThumbnailRow {
    path: String,
    kind: String,
    thumbnail_path: String,
    custom: bool,
    updated_at: String,
}

pub(super) fn import_archive_entries(
    state: &RepositoryState,
    request: FileArchiveImportRequest,
) -> Result<FileBrowserSnapshot, String> {
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
    execute_archive_import(&archive_path, &repo_root, &plan)?;
    finish_import_operation(state, &request.repo_id, parent_path, plan_has_directories(&plan))
}

pub(super) fn import_eagle_library(
    state: &RepositoryState,
    request: EagleLibraryImportRequest,
) -> Result<EagleLibraryImportResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    ensure_repository_supports_local_write_access(&repo, "importing Eagle libraries")?;

    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())?;
    let target_dir = resolve_repository_relative_path(&repo_root, &parent_path)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        return Err(format!("directory not found: {parent_path}"));
    }

    let library_root = canonicalize_local_path(Path::new(request.library_path.trim()))?;
    validate_eagle_library_root(&library_root)?;
    let mode = normalize_eagle_import_mode(&request.mode)?;

    let temp_workspace = create_temp_import_workspace(&state.root, "eagle-library")?;
    let converted_repo_root = temp_workspace.join("converted-repository");
    let persistent_report_path = state
        .root
        .join("import-reports")
        .join(format!("eagle-import-{}.json", slugify_repo_id(&request.repo_id, &now_rfc3339())));

    let import_result = (|| -> Result<EagleLibraryImportResponse, String> {
        run_eagle_converter(&library_root, &converted_repo_root, mode)?;
        let source_report_path = report_output_path_for(&converted_repo_root);
        if let Some(parent) = persistent_report_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        fs::copy(&source_report_path, &persistent_report_path).map_err(io_error)?;

        let source_repo_identity = load_source_repository_identity(&converted_repo_root)?;
        let plan = plan_eagle_merge(
            &converted_repo_root,
            &repo_root,
            &parent_path,
        )?;
        execute_eagle_visible_merge(&plan.visible_entries, &repo_root)?;
        merge_eagle_trash_entries(&repo_root, &plan.trash_entries)?;

        state.sync_repository(SyncRequest {
            repo_id: request.repo_id.clone(),
        })?;
        merge_eagle_repository_records(
            state,
            &repo,
            &converted_repo_root,
            &source_repo_identity,
            &parent_path,
            &plan.summary,
        )?;
        let snapshot = state.load_file_browser(FileBrowserRequest {
            repo_id: request.repo_id.clone(),
            directory_path: Some(parent_path.clone()),
            include_tree: Some(plan.include_tree),
            special_location: None,
            offset: None,
            limit: None,
        })?;
        Ok(EagleLibraryImportResponse {
            snapshot,
            report_path: persistent_report_path.to_string_lossy().to_string(),
            summary: plan.summary,
        })
    })();

    let _ = fs::remove_dir_all(&temp_workspace);
    import_result
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
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("invalid zip archive: {error}"))?;
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
) -> Result<(), String> {
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| format!("invalid zip archive: {error}"))?;

    let mut directories = plan
        .iter()
        .filter(|entry| entry.is_directory)
        .cloned()
        .collect::<Vec<_>>();
    directories.sort_by_key(|entry| entry.target_relative_path.len());
    for entry in directories {
        let target_abs = resolve_repository_relative_path(repo_root, &entry.target_relative_path)?;
        fs::create_dir_all(target_abs).map_err(io_error)?;
    }

    let files = plan
        .iter()
        .filter(|entry| !entry.is_directory)
        .collect::<Vec<_>>();
    for entry in files {
        let archive_name = entry
            .archive_name
            .as_deref()
            .ok_or_else(|| format!("archive entry missing source: {}", entry.target_relative_path))?;
        let mut source = archive
            .by_name(archive_name)
            .map_err(|error| format!("failed to read zip entry {archive_name}: {error}"))?;
        let target_abs = resolve_repository_relative_path(repo_root, &entry.target_relative_path)?;
        if let Some(parent) = target_abs.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut output = File::create(&target_abs).map_err(io_error)?;
        std::io::copy(&mut source, &mut output).map_err(io_error)?;
    }
    Ok(())
}

fn validate_eagle_library_root(library_root: &Path) -> Result<(), String> {
    if !library_root.is_dir() {
        return Err(format!(
            "Eagle library is not a directory: {}",
            library_root.to_string_lossy()
        ));
    }
    for required in ["metadata.json", "images"] {
        if !library_root.join(required).exists() {
            return Err(format!("invalid EagleLibrary, missing: {required}"));
        }
    }
    Ok(())
}

fn normalize_eagle_import_mode(mode: &str) -> Result<&'static str, String> {
    match mode.trim() {
        "copy" => Ok("copy"),
        "move" => Ok("move"),
        value => Err(format!("unsupported eagle import mode: {value}")),
    }
}

fn create_temp_import_workspace(service_root: &Path, prefix: &str) -> Result<PathBuf, String> {
    let temp_root = service_root.join("temp-imports");
    fs::create_dir_all(&temp_root).map_err(io_error)?;
    let workspace = temp_root.join(format!(
        "{prefix}-{}-{}",
        std::process::id(),
        slugify_repo_id(prefix, &now_rfc3339())
    ));
    fs::create_dir_all(&workspace).map_err(io_error)?;
    Ok(workspace)
}

fn run_eagle_converter(
    library_root: &Path,
    output_root: &Path,
    mode: &str,
) -> Result<(), String> {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "failed to resolve repository root".to_string())?;
    let script_path = repo_root
        .join("External")
        .join("EagleLibraryChanger")
        .join("convert.py");
    if !script_path.is_file() {
        return Err(format!(
            "Eagle converter not found: {}",
            script_path.to_string_lossy()
        ));
    }

    let candidates = eagle_python_candidates(&repo_root);
    let mut last_error = String::new();
    for candidate in candidates {
        let output = match candidate {
            PythonCandidate::Executable(path) => Command::new(path)
                .args([
                    script_path.to_string_lossy().as_ref(),
                    "--input",
                    library_root.to_string_lossy().as_ref(),
                    "--output",
                    output_root.to_string_lossy().as_ref(),
                    "--name",
                    "Eagle Import Temp",
                    "--yes",
                    "--force",
                    "--mode",
                    mode,
                ])
                .output(),
            PythonCandidate::PyLauncher => Command::new("py")
                .args([
                    "-3",
                    script_path.to_string_lossy().as_ref(),
                    "--input",
                    library_root.to_string_lossy().as_ref(),
                    "--output",
                    output_root.to_string_lossy().as_ref(),
                    "--name",
                    "Eagle Import Temp",
                    "--yes",
                    "--force",
                    "--mode",
                    mode,
                ])
                .output(),
        };
        match output {
            Ok(result) if result.status.success() => return Ok(()),
            Ok(result) => {
                let stderr = String::from_utf8_lossy(&result.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
                last_error = if stderr.is_empty() { stdout } else { stderr };
            }
            Err(error) => {
                last_error = error.to_string();
            }
        }
    }
    Err(format!("failed to run Eagle converter: {last_error}"))
}

enum PythonCandidate {
    Executable(PathBuf),
    PyLauncher,
}

fn eagle_python_candidates(repo_root: &Path) -> Vec<PythonCandidate> {
    let mut result = Vec::new();
    let bundled = if cfg!(target_os = "windows") {
        repo_root
            .join("External")
            .join("EagleLibraryChanger")
            .join(".venv")
            .join("Scripts")
            .join("python.exe")
    } else {
        repo_root
            .join("External")
            .join("EagleLibraryChanger")
            .join(".venv")
            .join("bin")
            .join("python")
    };
    if bundled.is_file() {
        result.push(PythonCandidate::Executable(bundled));
    }
    result.push(PythonCandidate::Executable(PathBuf::from("python")));
    if cfg!(target_os = "windows") {
        result.push(PythonCandidate::PyLauncher);
    }
    result
}

fn report_output_path_for(output_root: &Path) -> PathBuf {
    output_root
        .parent()
        .unwrap_or(output_root)
        .join(format!("{}.import-report.json", output_root.file_name().unwrap_or_default().to_string_lossy()))
}

fn load_source_repository_identity(converted_repo_root: &Path) -> Result<SourceRepositoryIdentity, String> {
    let connection = open_unregistered_repository_connection(converted_repo_root)?;
    connection
        .query_row(
            "SELECT repo_id, name FROM repositories LIMIT 1",
            [],
            |row| {
                Ok(SourceRepositoryIdentity {
                    repo_id: row.get(0)?,
                    name: row.get(1)?,
                })
            },
        )
        .map_err(db_error)
}

fn open_unregistered_repository_connection(repo_root: &Path) -> Result<Connection, String> {
    let database_path = repository_meta_dir(repo_root).join(REPO_DB_FILE_NAME);
    let connection = Connection::open(database_path).map_err(db_error)?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(db_error)?;
    Ok(connection)
}

fn plan_eagle_merge(
    source_repo_root: &Path,
    destination_repo_root: &Path,
    parent_path: &str,
) -> Result<EagleMergePlan, String> {
    let source_meta_root = repository_meta_dir(source_repo_root);
    let source_trash_root = repository_trash_dir(source_repo_root);
    let source_thumbnail_root = source_meta_root.join("thumbnails");
    let destination_thumbnail_root = repository_meta_dir(destination_repo_root).join("thumbnails");
    let destination_manifest = load_trash_manifest(destination_repo_root)?;

    let mut summary = EagleLibraryImportSummary::default();
    let mut visible_entries = Vec::new();
    for entry in fs::read_dir(source_repo_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_name = entry.file_name();
        if file_name.to_string_lossy() == REPO_META_DIR {
            continue;
        }
        let name = validate_new_entry_name(&file_name.to_string_lossy())?;
        let target_relative_path = prefix_relative_path(parent_path, &name);
        let target_abs = resolve_repository_relative_path(destination_repo_root, &target_relative_path)?;
        if target_abs.exists() {
            return Err(format!("entry already exists: {name}"));
        }
        let source_abs = entry.path();
        let is_directory = source_abs.is_dir();
        if is_directory {
            summary.imported_directories += count_directories_including_self(&source_abs)?;
            summary.imported_files += count_files_recursively(&source_abs)?;
        } else {
            summary.imported_files += 1;
        }
        visible_entries.push(EagleVisibleEntry {
            source_abs,
            target_relative_path,
            is_directory,
        });
    }

    if source_thumbnail_root.exists() {
        for entry in fs::read_dir(&source_thumbnail_root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let target = destination_thumbnail_root.join(entry.file_name());
            if target.exists() {
                return Err(format!(
                    "thumbnail target already exists: {}",
                    target.to_string_lossy()
                ));
            }
        }
    }

    let source_manifest = load_trash_manifest(source_repo_root)?;
    let mut trash_entries = Vec::new();
    for entry in source_manifest.entries {
        let target_original_path = prefix_relative_path(parent_path, &entry.original_path);
        let target_trash_path = prefix_relative_path(parent_path, &entry.trash_path);
        let visible_target = resolve_repository_relative_path(destination_repo_root, &target_original_path)?;
        if visible_target.exists() {
            return Err(format!("target already exists: {target_original_path}"));
        }
        if destination_manifest
            .entries
            .iter()
            .any(|item| item.original_path == target_original_path || item.trash_path == target_trash_path)
        {
            return Err(format!("trash target already exists: {target_original_path}"));
        }
        let source_abs = resolve_trash_relative_path(&source_trash_root, &entry.trash_path)?;
        if !source_abs.exists() {
            return Err(format!("trash source missing: {}", entry.trash_path));
        }
        let destination_abs =
            resolve_trash_relative_path(&repository_trash_dir(destination_repo_root), &target_trash_path)?;
        if destination_abs.exists() {
            return Err(format!("trash file already exists: {target_trash_path}"));
        }
        summary.imported_trash_entries += 1;
        trash_entries.push(EagleTrashEntryPlan {
            source_abs,
            target_original_path,
            target_trash_path,
            kind: entry.kind,
            deleted_at: entry.deleted_at,
        });
    }

    Ok(EagleMergePlan {
        include_tree: visible_entries.iter().any(|entry| entry.is_directory),
        visible_entries,
        trash_entries,
        summary,
    })
}

fn count_files_recursively(path: &Path) -> Result<usize, String> {
    if path.is_file() {
        return Ok(1);
    }
    let mut total = 0usize;
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let child = entry.path();
        total += count_files_recursively(&child)?;
    }
    Ok(total)
}

fn count_directories_including_self(path: &Path) -> Result<usize, String> {
    if !path.is_dir() {
        return Ok(0);
    }
    let mut total = 1usize;
    for entry in fs::read_dir(path).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        total += count_directories_including_self(&entry.path())?;
    }
    Ok(total)
}

fn execute_eagle_visible_merge(
    visible_entries: &[EagleVisibleEntry],
    destination_repo_root: &Path,
) -> Result<(), String> {
    for entry in visible_entries {
        let target_abs = resolve_repository_relative_path(destination_repo_root, &entry.target_relative_path)?;
        if let Some(parent) = target_abs.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        fs::rename(&entry.source_abs, &target_abs).map_err(io_error)?;
    }
    Ok(())
}

fn merge_eagle_trash_entries(
    destination_repo_root: &Path,
    trash_entries: &[EagleTrashEntryPlan],
) -> Result<(), String> {
    let destination_trash_root = repository_trash_dir(destination_repo_root);
    let mut manifest = load_trash_manifest(destination_repo_root)?;
    for entry in trash_entries {
        let target_abs = resolve_trash_relative_path(&destination_trash_root, &entry.target_trash_path)?;
        if let Some(parent) = target_abs.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        fs::rename(&entry.source_abs, &target_abs).map_err(io_error)?;
        manifest.entries.push(TrashManifestEntry {
            original_path: entry.target_original_path.clone(),
            trash_path: entry.target_trash_path.clone(),
            deleted_at: entry.deleted_at.clone(),
            kind: entry.kind.clone(),
        });
    }
    save_trash_manifest(destination_repo_root, &manifest)
}

fn merge_eagle_repository_records(
    state: &RepositoryState,
    destination_repo: &RepositoryRecord,
    source_repo_root: &Path,
    source_identity: &SourceRepositoryIdentity,
    parent_path: &str,
    summary: &EagleLibraryImportSummary,
) -> Result<(), String> {
    let source_connection = open_unregistered_repository_connection(source_repo_root)?;
    let destination_thumbnail_root = state.repository_thumbnail_root(destination_repo)?;
    merge_thumbnail_files(source_repo_root, &destination_thumbnail_root)?;

    let mut destination_connection = state.open_repository_connection(
        &destination_repo.summary.repo_id,
        &destination_repo.summary.path,
        &destination_repo.backend_record,
    )?;
    let tx = destination_connection.transaction().map_err(db_error)?;
    let asset_map =
        load_asset_path_map(&tx, &destination_repo.summary.repo_id).map_err(db_error)?;
    let imported_assets = import_source_assets(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
        &asset_map,
    )?;
    import_source_folder_metadata(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
    )?;
    import_source_entry_thumbnails(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
    )?;
    let smart_folder_id_map = import_source_smart_folders(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
    )?;
    import_source_shortcuts(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
        &smart_folder_id_map,
    )?;
    import_source_repository_actions(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
    )?;
    import_source_tag_groups(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
    )?;
    import_source_alias_groups(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
        &imported_assets,
    )?;
    import_source_hardlink_groups(
        &tx,
        &source_connection,
        source_identity,
        &destination_repo.summary.repo_id,
        parent_path,
        &imported_assets,
    )?;
    update_import_summary_counts(&tx, &source_connection, source_identity, summary)?;
    tx.commit().map_err(db_error)
}

fn merge_thumbnail_files(source_repo_root: &Path, destination_thumbnail_root: &Path) -> Result<(), String> {
    let source_thumbnail_root = repository_meta_dir(source_repo_root).join("thumbnails");
    if !source_thumbnail_root.exists() {
        return Ok(());
    }
    fs::create_dir_all(destination_thumbnail_root).map_err(io_error)?;
    for entry in fs::read_dir(&source_thumbnail_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let target = destination_thumbnail_root.join(entry.file_name());
        if target.exists() {
            return Err(format!(
                "thumbnail target already exists: {}",
                target.to_string_lossy()
            ));
        }
        fs::rename(entry.path(), target).map_err(io_error)?;
    }
    Ok(())
}

fn import_source_assets(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
    destination_asset_map: &BTreeMap<String, AssetPathRecord>,
) -> Result<BTreeMap<String, ImportedAssetTarget>, String> {
    let source_assets = load_source_assets(source_connection, &source_identity.repo_id)?;
    let mut imported = BTreeMap::new();
    for asset in source_assets {
        let destination_path = prefix_relative_path(parent_path, &asset.path);
        let destination_target = if asset.status == "deleted" {
            let asset_id = asset_id_for_path(destination_repo_id, &destination_path);
            insert_deleted_asset_row(tx, destination_repo_id, &asset_id, &destination_path, &asset)?;
            ImportedAssetTarget {
                asset_id,
                path: destination_path.clone(),
                status: asset.status.clone(),
            }
        } else {
            let destination_asset = destination_asset_map
                .get(&destination_path)
                .ok_or_else(|| format!("imported asset not found after sync: {destination_path}"))?;
            update_visible_asset_row(
                tx,
                destination_repo_id,
                &destination_asset.asset_id,
                &destination_path,
                &asset,
            )?;
            ImportedAssetTarget {
                asset_id: destination_asset.asset_id.clone(),
                path: destination_path.clone(),
                status: asset.status.clone(),
            }
        };
        replace_asset_metadata_rows(
            tx,
            source_connection,
            &asset.asset_id,
            &destination_target.asset_id,
        )?;
        replace_asset_tag_rows(
            tx,
            source_connection,
            &asset.asset_id,
            &destination_target.asset_id,
        )?;
        imported.insert(asset.path.clone(), destination_target);
    }
    Ok(imported)
}

fn load_source_assets(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<SourceAssetRow>, String> {
    let mut stmt = connection
        .prepare(
            r#"
            SELECT asset_id, path, filename, extension, size_bytes, created_at, modified_at, hash,
                   status, version, updated_at, last_accessed_at, thumbnail_path, is_virtual,
                   provider_id, provider_item_id, source_payload_json, local_absolute_path
            FROM assets
            WHERE repo_id = ?1
            ORDER BY path COLLATE NOCASE
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([repo_id], |row| {
            Ok(SourceAssetRow {
                asset_id: row.get(0)?,
                path: row.get(1)?,
                filename: row.get(2)?,
                extension: row.get(3)?,
                size_bytes: row.get(4)?,
                created_at: row.get(5)?,
                modified_at: row.get(6)?,
                hash: row.get(7)?,
                status: row.get(8)?,
                version: row.get(9)?,
                updated_at: row.get(10)?,
                last_accessed_at: row.get(11)?,
                thumbnail_path: row.get(12)?,
                is_virtual: row.get::<_, i64>(13)? != 0,
                provider_id: row.get(14)?,
                provider_item_id: row.get(15)?,
                source_payload_json: row.get(16)?,
                local_absolute_path: row.get(17)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn update_visible_asset_row(
    tx: &Transaction<'_>,
    destination_repo_id: &str,
    destination_asset_id: &str,
    destination_path: &str,
    asset: &SourceAssetRow,
) -> Result<(), String> {
    tx.execute(
        r#"
        UPDATE assets
        SET filename = ?4,
            extension = ?5,
            size_bytes = ?6,
            created_at = ?7,
            modified_at = ?8,
            hash = ?9,
            status = ?10,
            version = ?11,
            updated_at = ?12,
            last_accessed_at = ?13,
            thumbnail_path = ?14,
            is_virtual = ?15,
            provider_id = ?16,
            provider_item_id = ?17,
            source_payload_json = ?18,
            local_absolute_path = ?19
        WHERE repo_id = ?1 AND asset_id = ?2 AND path = ?3
        "#,
        params![
            destination_repo_id,
            destination_asset_id,
            destination_path,
            asset.filename,
            asset.extension,
            asset.size_bytes,
            asset.created_at,
            asset.modified_at,
            asset.hash,
            asset.status,
            asset.version,
            asset.updated_at,
            asset.last_accessed_at,
            asset.thumbnail_path,
            if asset.is_virtual { 1 } else { 0 },
            asset.provider_id,
            asset.provider_item_id,
            asset.source_payload_json,
            asset.local_absolute_path,
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

fn insert_deleted_asset_row(
    tx: &Transaction<'_>,
    destination_repo_id: &str,
    destination_asset_id: &str,
    destination_path: &str,
    asset: &SourceAssetRow,
) -> Result<(), String> {
    tx.execute(
        r#"
        INSERT INTO assets (
          asset_id, repo_id, path, filename, extension, size_bytes, created_at, modified_at, hash,
          status, version, updated_at, last_accessed_at, thumbnail_path, is_virtual, provider_id,
          provider_item_id, source_payload_json, local_absolute_path
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
        "#,
        params![
            destination_asset_id,
            destination_repo_id,
            destination_path,
            asset.filename,
            asset.extension,
            asset.size_bytes,
            asset.created_at,
            asset.modified_at,
            asset.hash,
            asset.status,
            asset.version,
            asset.updated_at,
            asset.last_accessed_at,
            asset.thumbnail_path,
            if asset.is_virtual { 1 } else { 0 },
            asset.provider_id,
            asset.provider_item_id,
            asset.source_payload_json,
            asset.local_absolute_path,
        ],
    )
    .map_err(db_error)?;
    Ok(())
}

fn replace_asset_metadata_rows(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_asset_id: &str,
    destination_asset_id: &str,
) -> Result<(), String> {
    tx.execute("DELETE FROM metadata WHERE asset_id = ?1", [destination_asset_id])
        .map_err(db_error)?;
    let mut stmt = source_connection
        .prepare(
            r#"
            SELECT key, value_type, value_json, version, updated_at
            FROM metadata
            WHERE asset_id = ?1
            ORDER BY key COLLATE NOCASE
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([source_asset_id], |row| {
            Ok(SourceMetadataRow {
                key: row.get(0)?,
                value_type: row.get(1)?,
                value_json: row.get(2)?,
                version: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(db_error)?;
    for row in rows {
        let row = row.map_err(db_error)?;
        tx.execute(
            r#"
            INSERT INTO metadata (asset_id, key, value_type, value_json, version, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                destination_asset_id,
                row.key,
                row.value_type,
                row.value_json,
                row.version,
                row.updated_at
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn replace_asset_tag_rows(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_asset_id: &str,
    destination_asset_id: &str,
) -> Result<(), String> {
    tx.execute("DELETE FROM tags WHERE asset_id = ?1", [destination_asset_id])
        .map_err(db_error)?;
    let mut stmt = source_connection
        .prepare(
            r#"
            SELECT tag, normalized_tag
            FROM tags
            WHERE asset_id = ?1
            ORDER BY normalized_tag COLLATE NOCASE
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([source_asset_id], |row| {
            Ok(SourceTagRow {
                tag: row.get(0)?,
                normalized_tag: row.get(1)?,
            })
        })
        .map_err(db_error)?;
    for row in rows {
        let row = row.map_err(db_error)?;
        tx.execute(
            r#"
            INSERT INTO tags (asset_id, tag, normalized_tag)
            VALUES (?1, ?2, ?3)
            "#,
            params![destination_asset_id, row.tag, row.normalized_tag],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn import_source_folder_metadata(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
) -> Result<(), String> {
    let mut stmt = source_connection
        .prepare(
            r#"
            SELECT path, protected, password_tip, updated_at
            FROM folder_metadata
            WHERE repo_id = ?1
            ORDER BY path COLLATE NOCASE
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([source_identity.repo_id.as_str()], |row| {
            Ok(SourceFolderMetadataRow {
                path: row.get(0)?,
                protected: row.get::<_, i64>(1)? != 0,
                password_tip: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(db_error)?;
    for row in rows {
        let row = row.map_err(db_error)?;
        let destination_path = prefix_relative_path(parent_path, &row.path);
        tx.execute(
            r#"
            INSERT OR REPLACE INTO folder_metadata (repo_id, path, protected, password_tip, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                destination_repo_id,
                destination_path,
                if row.protected { 1 } else { 0 },
                row.password_tip,
                row.updated_at
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn import_source_entry_thumbnails(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
) -> Result<(), String> {
    let mut stmt = source_connection
        .prepare(
            r#"
            SELECT path, kind, thumbnail_path, custom, updated_at
            FROM entry_thumbnails
            WHERE repo_id = ?1
            ORDER BY path COLLATE NOCASE
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([source_identity.repo_id.as_str()], |row| {
            Ok(SourceEntryThumbnailRow {
                path: row.get(0)?,
                kind: row.get(1)?,
                thumbnail_path: row.get(2)?,
                custom: row.get::<_, i64>(3)? != 0,
                updated_at: row.get(4)?,
            })
        })
        .map_err(db_error)?;
    for row in rows {
        let row = row.map_err(db_error)?;
        let destination_path = prefix_relative_path(parent_path, &row.path);
        tx.execute(
            r#"
            INSERT OR REPLACE INTO entry_thumbnails (repo_id, path, kind, thumbnail_path, custom, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                destination_repo_id,
                destination_path,
                row.kind,
                row.thumbnail_path,
                if row.custom { 1 } else { 0 },
                row.updated_at
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn import_source_smart_folders(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
) -> Result<BTreeMap<String, String>, String> {
    let source_folders =
        load_smart_folders(source_connection, &source_identity.repo_id).map_err(db_error)?;
    let mut id_map = BTreeMap::new();
    for folder in source_folders {
        let new_parent_id = folder
            .parent_id
            .as_ref()
            .and_then(|value| id_map.get(value))
            .cloned();
        let new_id = smart_folder_id_for(destination_repo_id, new_parent_id.as_deref(), &folder.name);
        let filter = prefix_smart_folder_filter(&folder.filter, parent_path);
        let filter_json = serde_json::to_string(&filter).map_err(json_error)?;
        tx.execute(
            r#"
            INSERT INTO smart_folders (
              smart_folder_id, repo_id, parent_id, name, filter_json, sort_order, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                new_id,
                destination_repo_id,
                new_parent_id,
                folder.name,
                filter_json,
                folder.sort_order,
                folder.created_at,
                folder.updated_at
            ],
        )
        .map_err(db_error)?;
        id_map.insert(folder.smart_folder_id, new_id);
    }
    Ok(id_map)
}

fn prefix_smart_folder_filter(filter: &SmartFolderFilter, parent_path: &str) -> SmartFolderFilter {
    let mut next = filter.clone();
    next.path_prefix = filter
        .path_prefix
        .as_deref()
        .map(|value| prefix_multiline_relative_paths(parent_path, value));
    next.exclude_path_prefixes = filter.exclude_path_prefixes.as_ref().map(|values| {
        values
            .iter()
            .map(|value| prefix_relative_path(parent_path, value))
            .collect()
    });
    next
}

fn import_source_shortcuts(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
    smart_folder_id_map: &BTreeMap<String, String>,
) -> Result<(), String> {
    let shortcuts =
        load_repository_shortcuts(source_connection, &source_identity.repo_id).map_err(db_error)?;
    for (index, shortcut) in shortcuts.into_iter().enumerate() {
        let target_path = shortcut
            .target_path
            .as_deref()
            .map(|value| prefix_relative_path(parent_path, value));
        let target_id = if shortcut.target_kind == "smartFolder" {
            shortcut
                .target_id
                .as_deref()
                .and_then(|value| smart_folder_id_map.get(value))
                .cloned()
                .or(shortcut.target_id)
        } else {
            shortcut.target_id
        };
        tx.execute(
            r#"
            INSERT INTO repository_shortcuts (
              shortcut_id, repo_id, label, target_kind, target_path, target_id, sort_order, created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                imported_entity_id("shortcut", destination_repo_id, &shortcut.shortcut_id, index),
                destination_repo_id,
                shortcut.label,
                shortcut.target_kind,
                target_path,
                target_id,
                index as i64,
                now_rfc3339()
            ],
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn import_source_repository_actions(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
) -> Result<(), String> {
    let actions =
        load_repository_actions(source_connection, &source_identity.repo_id).map_err(db_error)?;
    for (index, action) in actions.into_iter().enumerate() {
        let new_action_id = imported_entity_id("action", destination_repo_id, &action.action_id, index);
        tx.execute(
            r#"
            INSERT INTO repository_actions (
              action_id, repo_id, source, source_action_id, name, status, enabled, raw_json,
              unsupported_reason, sort_order, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
            params![
                new_action_id,
                destination_repo_id,
                action.source,
                action.source_action_id,
                action.name,
                action.status,
                if action.enabled { 1 } else { 0 },
                prefix_action_json_paths(&action.raw, parent_path).to_string(),
                action.unsupported_reason,
                index as i64,
                action.created_at,
                action.updated_at
            ],
        )
        .map_err(db_error)?;
        for (step_index, step) in action.steps.into_iter().enumerate() {
            tx.execute(
                r#"
                INSERT INTO repository_action_steps (
                  step_id, action_id, repo_id, step_kind, label, status, config_json,
                  raw_json, unsupported_reason, sort_order
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                "#,
                params![
                    imported_entity_id("action-step", destination_repo_id, &step.step_id, step_index),
                    new_action_id,
                    destination_repo_id,
                    step.step_kind,
                    step.label,
                    step.status,
                    prefix_action_json_paths(&step.config, parent_path).to_string(),
                    prefix_action_json_paths(&step.raw, parent_path).to_string(),
                    step.unsupported_reason,
                    step_index as i64
                ],
            )
            .map_err(db_error)?;
        }
    }
    Ok(())
}

fn prefix_action_json_paths(value: &serde_json::Value, parent_path: &str) -> serde_json::Value {
    fn rewrite(
        value: &serde_json::Value,
        parent_path: &str,
        key_hint: Option<&str>,
    ) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => serde_json::Value::Object(
                map.iter()
                    .map(|(key, nested)| (key.clone(), rewrite(nested, parent_path, Some(key.as_str()))))
                    .collect(),
            ),
            serde_json::Value::Array(items) => serde_json::Value::Array(
                items
                    .iter()
                    .map(|item| rewrite(item, parent_path, key_hint))
                    .collect(),
            ),
            serde_json::Value::String(text) if key_is_path_like(key_hint) => {
                serde_json::Value::String(prefix_path_like_string(parent_path, text))
            }
            _ => value.clone(),
        }
    }

    rewrite(value, parent_path, None)
}

fn key_is_path_like(key_hint: Option<&str>) -> bool {
    matches!(
        key_hint.unwrap_or_default(),
        "path"
            | "parentPath"
            | "directoryPath"
            | "targetPath"
            | "pathPrefix"
            | "excludePathPrefixes"
            | "paths"
            | "targetPaths"
    )
}

fn prefix_path_like_string(parent_path: &str, value: &str) -> String {
    if value.contains("://") {
        return value.to_string();
    }
    if value.contains('\n') {
        return prefix_multiline_relative_paths(parent_path, value);
    }
    match normalize_relative_path(value, true) {
        Ok(normalized) => prefix_relative_path(parent_path, &normalized),
        Err(_) => value.to_string(),
    }
}

fn import_source_tag_groups(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
) -> Result<(), String> {
    let groups =
        load_repository_tag_groups(source_connection, &source_identity.repo_id).map_err(db_error)?;
    for (index, group) in groups.into_iter().enumerate() {
        let tag_group_id = imported_entity_id("tag-group", destination_repo_id, &group.tag_group_id, index);
        tx.execute(
            r#"
            INSERT INTO tag_groups (tag_group_id, repo_id, name, sort_order, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#,
            params![
                tag_group_id,
                destination_repo_id,
                group.name,
                index as i64,
                now_rfc3339(),
                now_rfc3339()
            ],
        )
        .map_err(db_error)?;
        for (member_index, tag) in group.tags.into_iter().enumerate() {
            tx.execute(
                r#"
                INSERT INTO tag_group_members (tag_group_id, repo_id, tag, normalized_tag, sort_order)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
                params![tag_group_id, destination_repo_id, tag, tag.to_lowercase(), member_index as i64],
            )
            .map_err(db_error)?;
        }
    }
    Ok(())
}

fn import_source_alias_groups(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
    imported_assets: &BTreeMap<String, ImportedAssetTarget>,
) -> Result<(), String> {
    let groups = load_source_alias_groups(source_connection, &source_identity.repo_id)?;
    let members = load_source_alias_members(source_connection, &source_identity.repo_id)?;
    let member_map = members.into_iter().fold(
        BTreeMap::<String, Vec<SourceAliasMemberRow>>::new(),
        |mut map, item| {
            map.entry(item.alias_group_id.clone()).or_default().push(item);
            map
        },
    );
    for (index, group) in groups.into_iter().enumerate() {
        let Some(group_members) = member_map.get(&group.alias_group_id) else {
            continue;
        };
        let alias_group_id = imported_entity_id("alias-group", destination_repo_id, &group.alias_group_id, index);
        tx.execute(
            r#"
            INSERT INTO asset_alias_groups (alias_group_id, repo_id, source, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                alias_group_id,
                destination_repo_id,
                group.source,
                group.created_at,
                group.updated_at
            ],
        )
        .map_err(db_error)?;
        for member in group_members {
            let source_path = prefix_relative_path(parent_path, &member.path);
            let target = imported_assets
                .get(&member.path)
                .or_else(|| imported_assets.get(&source_path))
                .ok_or_else(|| format!("alias asset not found: {}", member.path))?;
            tx.execute(
                r#"
                INSERT INTO asset_alias_members (alias_group_id, repo_id, asset_id, path, role, created_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
                params![
                    alias_group_id,
                    destination_repo_id,
                    target.asset_id,
                    target.path,
                    member.role,
                    member.created_at
                ],
            )
            .map_err(db_error)?;
        }
    }
    Ok(())
}

fn load_source_alias_groups(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<SourceAliasGroupRow>, String> {
    let mut stmt = connection
        .prepare(
            r#"
            SELECT alias_group_id, source, created_at, updated_at
            FROM asset_alias_groups
            WHERE repo_id = ?1
            ORDER BY alias_group_id
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([repo_id], |row| {
            Ok(SourceAliasGroupRow {
                alias_group_id: row.get(0)?,
                source: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn load_source_alias_members(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<SourceAliasMemberRow>, String> {
    let mut stmt = connection
        .prepare(
            r#"
            SELECT alias_group_id, asset_id, path, role, created_at
            FROM asset_alias_members
            WHERE repo_id = ?1
            ORDER BY alias_group_id, role DESC, path COLLATE NOCASE
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([repo_id], |row| {
            Ok(SourceAliasMemberRow {
                alias_group_id: row.get(0)?,
                asset_id: row.get(1)?,
                path: row.get(2)?,
                role: row.get(3)?,
                created_at: row.get(4)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn import_source_hardlink_groups(
    tx: &Transaction<'_>,
    source_connection: &Connection,
    source_identity: &SourceRepositoryIdentity,
    destination_repo_id: &str,
    parent_path: &str,
    imported_assets: &BTreeMap<String, ImportedAssetTarget>,
) -> Result<(), String> {
    let members = load_source_hardlink_members(source_connection, &source_identity.repo_id)?;
    for member in members {
        let source_path = prefix_relative_path(parent_path, &member.path);
        let target = imported_assets
            .get(&member.path)
            .or_else(|| imported_assets.get(&source_path))
            .ok_or_else(|| format!("hardlink asset not found: {}", member.path))?;
        upsert_hardlink_member(
            tx,
            destination_repo_id,
            &target.asset_id,
            &target.path,
            &member.content_hash,
            member.size_bytes,
            &member.link_state,
        )
        .map_err(db_error)?;
    }
    Ok(())
}

fn load_source_hardlink_members(
    connection: &Connection,
    repo_id: &str,
) -> Result<Vec<SourceHardlinkMemberRow>, String> {
    let mut stmt = connection
        .prepare(
            r#"
            SELECT hm.group_id, hm.asset_id, hm.path, hm.link_state, hg.content_hash, hg.size_bytes
            FROM hardlink_members hm
            INNER JOIN hardlink_groups hg
              ON hg.group_id = hm.group_id AND hg.repo_id = hm.repo_id
            WHERE hm.repo_id = ?1
            ORDER BY hm.group_id, hm.path COLLATE NOCASE
            "#,
        )
        .map_err(db_error)?;
    let rows = stmt
        .query_map([repo_id], |row| {
            Ok(SourceHardlinkMemberRow {
                group_id: row.get(0)?,
                asset_id: row.get(1)?,
                path: row.get(2)?,
                link_state: row.get(3)?,
                content_hash: row.get(4)?,
                size_bytes: row.get(5)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}

fn update_import_summary_counts(
    _tx: &Transaction<'_>,
    _source_connection: &Connection,
    _source_identity: &SourceRepositoryIdentity,
    _summary: &EagleLibraryImportSummary,
) -> Result<(), String> {
    Ok(())
}

fn imported_entity_id(prefix: &str, repo_id: &str, source_id: &str, index: usize) -> String {
    format!(
        "{prefix}-{}",
        sha256_hex(&[
            repo_id.as_bytes(),
            source_id.as_bytes(),
            index.to_string().as_bytes(),
            now_rfc3339().as_bytes(),
        ])
    )
}

fn prefix_relative_path(parent_path: &str, path: &str) -> String {
    let normalized = normalize_relative_path(path, true).unwrap_or_else(|_| path.trim().replace('\\', "/").trim_matches('/').to_string());
    match (parent_path.is_empty(), normalized.is_empty()) {
        (true, true) => String::new(),
        (true, false) => normalized,
        (false, true) => parent_path.to_string(),
        (false, false) => join_relative_path(parent_path, &normalized),
    }
}

fn prefix_multiline_relative_paths(parent_path: &str, value: &str) -> String {
    value
        .lines()
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| prefix_relative_path(parent_path, item))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_archive_entry_path, prefix_action_json_paths, prefix_relative_path,
        prefix_smart_folder_filter,
    };
    use crate::services::repository::contracts::SmartFolderFilter;
    use serde_json::json;

    #[test]
    fn normalize_archive_entry_path_rejects_parent_segments() {
        let error = normalize_archive_entry_path("../escape.txt").expect_err("path traversal should fail");
        assert!(error.contains(".."));
    }

    #[test]
    fn normalize_archive_entry_path_rejects_absolute_paths() {
        let error = normalize_archive_entry_path("/escape.txt").expect_err("absolute path should fail");
        assert!(error.contains("absolute"));
    }

    #[test]
    fn prefix_relative_path_prefixes_nested_entries() {
        assert_eq!(prefix_relative_path("imports/eagle", "images/demo.png"), "imports/eagle/images/demo.png");
        assert_eq!(prefix_relative_path("imports/eagle", ""), "imports/eagle");
        assert_eq!(prefix_relative_path("", "images/demo.png"), "images/demo.png");
    }

    #[test]
    fn prefix_smart_folder_filter_rewrites_path_fields() {
        let filter = SmartFolderFilter {
            path_prefix: Some("images\nnested".to_string()),
            exclude_path_prefixes: Some(vec!["trash".to_string(), "drafts/mock".to_string()]),
            ..SmartFolderFilter::default()
        };

        let rewritten = prefix_smart_folder_filter(&filter, "imports/eagle");

        assert_eq!(
            rewritten.path_prefix.as_deref(),
            Some("imports/eagle/images\nimports/eagle/nested")
        );
        assert_eq!(
            rewritten.exclude_path_prefixes,
            Some(vec![
                "imports/eagle/trash".to_string(),
                "imports/eagle/drafts/mock".to_string(),
            ])
        );
    }

    #[test]
    fn prefix_action_json_paths_rewrites_known_path_keys_only() {
        let value = json!({
            "targetPath": "images/demo.png",
            "paths": ["images/demo.png", "nested/item.txt"],
            "excludePathPrefixes": "trash\nhidden",
            "url": "https://example.com/demo.png",
            "note": "keep-as-is"
        });

        let rewritten = prefix_action_json_paths(&value, "imports/eagle");

        assert_eq!(rewritten["targetPath"], json!("imports/eagle/images/demo.png"));
        assert_eq!(
            rewritten["paths"],
            json!(["imports/eagle/images/demo.png", "imports/eagle/nested/item.txt"])
        );
        assert_eq!(
            rewritten["excludePathPrefixes"],
            json!("imports/eagle/trash\nimports/eagle/hidden")
        );
        assert_eq!(rewritten["url"], json!("https://example.com/demo.png"));
        assert_eq!(rewritten["note"], json!("keep-as-is"));
    }
}
