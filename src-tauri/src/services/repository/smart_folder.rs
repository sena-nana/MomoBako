//! Smart-folder tree management and query workflows.

use super::*;

pub(super) fn list_smart_folders(
    state: &RepositoryState,
    repo_id: &str,
) -> Result<Vec<SmartFolderTreeNode>, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let folders = load_smart_folders(&connection, repo_id).map_err(db_error)?;
    Ok(build_smart_folder_tree(folders))
}

pub(super) fn create_smart_folder(
    state: &RepositoryState,
    request: SmartFolderMutationRequest,
) -> Result<SmartFolderMutationResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let name = validate_smart_folder_name(&request.name)?;
    validate_smart_folder_parent(
        &connection,
        &request.repo_id,
        request.parent_id.as_deref(),
        None,
    )
    .map_err(db_error)?;
    let smart_folder_id = request
        .smart_folder_id
        .as_deref()
        .map(validate_smart_folder_id)
        .transpose()?
        .unwrap_or_else(|| {
            smart_folder_id_for(&request.repo_id, request.parent_id.as_deref(), &name)
        });
    let filter = normalize_smart_folder_filter(request.filter);
    let filter_json = serde_json::to_string(&filter).map_err(json_error)?;
    let now = now_rfc3339();
    let sort_order =
        next_smart_folder_sort_order(&connection, &request.repo_id, request.parent_id.as_deref())
            .map_err(db_error)?;
    connection
        .execute(
            r#"
            INSERT INTO smart_folders (
              smart_folder_id, repo_id, parent_id, name, filter_json,
              sort_order, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            "#,
            params![
                smart_folder_id,
                request.repo_id,
                normalized_optional_id(request.parent_id.as_deref()),
                name,
                filter_json,
                sort_order,
                now
            ],
        )
        .map_err(db_error)?;

    let folders = load_smart_folders(&connection, &request.repo_id).map_err(db_error)?;
    let smart_folder = folders
        .iter()
        .find(|folder| folder.smart_folder_id == smart_folder_id)
        .cloned();
    Ok(SmartFolderMutationResponse {
        smart_folders: build_smart_folder_tree(folders),
        smart_folder,
    })
}

pub(super) fn update_smart_folder(
    state: &RepositoryState,
    request: SmartFolderUpdateRequest,
) -> Result<SmartFolderMutationResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let smart_folder_id = validate_smart_folder_id(&request.smart_folder_id)?;
    let name = validate_smart_folder_name(&request.name)?;
    let existing = load_smart_folder(&connection, &request.repo_id, &smart_folder_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("smart folder not found: {smart_folder_id}"))?;
    validate_smart_folder_parent(
        &connection,
        &request.repo_id,
        request.parent_id.as_deref(),
        Some(&smart_folder_id),
    )
    .map_err(db_error)?;
    let filter = normalize_smart_folder_filter(request.filter);
    let filter_json = serde_json::to_string(&filter).map_err(json_error)?;
    let now = now_rfc3339();
    let sort_order = if normalized_optional_id(request.parent_id.as_deref()) == existing.parent_id {
        existing.sort_order
    } else {
        next_smart_folder_sort_order(&connection, &request.repo_id, request.parent_id.as_deref())
            .map_err(db_error)?
    };
    connection
        .execute(
            r#"
            UPDATE smart_folders
            SET parent_id = ?3, name = ?4, filter_json = ?5,
                sort_order = ?6, updated_at = ?7
            WHERE repo_id = ?1 AND smart_folder_id = ?2
            "#,
            params![
                request.repo_id,
                smart_folder_id,
                normalized_optional_id(request.parent_id.as_deref()),
                name,
                filter_json,
                sort_order,
                now
            ],
        )
        .map_err(db_error)?;

    let folders = load_smart_folders(&connection, &request.repo_id).map_err(db_error)?;
    let smart_folder = folders
        .iter()
        .find(|folder| folder.smart_folder_id == smart_folder_id)
        .cloned();
    Ok(SmartFolderMutationResponse {
        smart_folders: build_smart_folder_tree(folders),
        smart_folder,
    })
}

pub(super) fn delete_smart_folder(
    state: &RepositoryState,
    repo_id: &str,
    smart_folder_id: &str,
) -> Result<SmartFolderMutationResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let smart_folder_id = validate_smart_folder_id(smart_folder_id)?;
    load_smart_folder(&connection, repo_id, &smart_folder_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("smart folder not found: {smart_folder_id}"))?;
    connection
        .execute(
            r#"
            WITH RECURSIVE deleting(id) AS (
              SELECT smart_folder_id
              FROM smart_folders
              WHERE repo_id = ?1 AND smart_folder_id = ?2
              UNION ALL
              SELECT child.smart_folder_id
              FROM smart_folders child
              INNER JOIN deleting ON child.parent_id = deleting.id
              WHERE child.repo_id = ?1
            )
            DELETE FROM smart_folders
            WHERE repo_id = ?1 AND smart_folder_id IN (SELECT id FROM deleting)
            "#,
            params![repo_id, smart_folder_id],
        )
        .map_err(db_error)?;
    let folders = load_smart_folders(&connection, repo_id).map_err(db_error)?;
    Ok(SmartFolderMutationResponse {
        smart_folders: build_smart_folder_tree(folders),
        smart_folder: None,
    })
}

pub(super) fn query_smart_folder(
    state: &RepositoryState,
    repo_id: &str,
    smart_folder_id: &str,
) -> Result<SmartFolderResultSnapshot, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let smart_folder_id = validate_smart_folder_id(smart_folder_id)?;
    let folders = load_smart_folders(&connection, repo_id).map_err(db_error)?;
    let smart_folder = folders
        .iter()
        .find(|folder| folder.smart_folder_id == smart_folder_id)
        .cloned()
        .ok_or_else(|| format!("smart folder not found: {smart_folder_id}"))?;
    let inherited_filter = inherited_smart_folder_filter(&folders, &smart_folder);
    let thumbnail_root = state.repository_thumbnail_root(&repo)?;
    let asset_map = normalize_asset_thumbnail_map(
        &connection,
        &repo,
        &thumbnail_root,
        load_asset_path_map(&connection, repo_id).map_err(db_error)?,
    )?;
    let results =
        query_smart_folder_entries(&connection, &repo.summary, &inherited_filter, &asset_map)
            .map_err(db_error)?;
    Ok(SmartFolderResultSnapshot {
        repo_id: repo_id.to_string(),
        smart_folder,
        inherited_filter,
        results,
    })
}
