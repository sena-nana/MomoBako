//! Repository action loading, toggling, and execution workflows.

use super::*;

pub(super) fn list_repository_actions(
    state: &RepositoryState,
    repo_id: &str,
) -> Result<Vec<RepositoryAction>, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    load_repository_actions(&connection, repo_id).map_err(db_error)
}

pub(super) fn get_repository_action(
    state: &RepositoryState,
    repo_id: &str,
    action_id: &str,
) -> Result<RepositoryAction, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(repo_id)?;
    let connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    load_repository_action(&connection, repo_id, action_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("repository action not found: {action_id}"))
}

pub(super) fn set_repository_action_enabled(
    state: &RepositoryState,
    request: RepositoryActionEnabledRequest,
) -> Result<RepositoryActionMutationResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let tx = connection.transaction().map_err(db_error)?;
    let action = load_repository_action(&tx, &request.repo_id, &request.action_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("repository action not found: {}", request.action_id))?;
    if request.enabled && action.status != "ready" {
        return Err("unsupported repository actions cannot be enabled".to_string());
    }
    let now = now_rfc3339();
    tx.execute(
        r#"
            UPDATE repository_actions
            SET enabled = ?3, updated_at = ?4
            WHERE repo_id = ?1 AND action_id = ?2
            "#,
        params![
            request.repo_id,
            request.action_id,
            if request.enabled { 1 } else { 0 },
            now
        ],
    )
    .map_err(db_error)?;
    let snapshot =
        load_source_repository_state_snapshot(&tx, &request.repo_id).map_err(db_error)?;
    let _ = write_backend_repository_state(
        &state.root,
        &repo,
        Path::new(&repo.summary.path),
        &snapshot,
    )?;
    tx.commit().map_err(db_error)?;
    let action = load_repository_action(&connection, &repo.summary.repo_id, &request.action_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("repository action not found: {}", request.action_id))?;
    Ok(RepositoryActionMutationResponse { action })
}

pub(super) fn run_repository_action(
    state: &RepositoryState,
    request: RepositoryActionRunRequest,
) -> Result<RepositoryActionRunResponse, String> {
    state.ensure_initialized()?;
    let repo = state.load_repository_record(&request.repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let action = load_repository_action(&connection, &request.repo_id, &request.action_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("repository action not found: {}", request.action_id))?;
    if action.status != "ready" {
        return Err("repository action contains unsupported steps".to_string());
    }
    if !action.enabled {
        return Err("repository action is disabled".to_string());
    }
    let target_asset_ids = resolve_action_target_asset_ids(&connection, &request)?;
    if target_asset_ids.is_empty() {
        return Err("repository action requires at least one target".to_string());
    }

    let started_at = now_rfc3339();
    let run_id = slugify_ascii_component(&format!(
        "action-run-{}-{}-{}",
        request.repo_id, request.action_id, started_at
    ));
    let target_json = serde_json::json!({
        "assetIds": target_asset_ids.clone(),
        "targetPaths": request.target_paths.clone().unwrap_or_default(),
    });
    let mut run_status = "success".to_string();
    let mut run_message = format!("已处理 {} 个目标", target_asset_ids.len());
    let tx = connection.transaction().map_err(db_error)?;

    tx.execute(
        r#"
        INSERT INTO repository_action_runs (
          run_id, action_id, repo_id, status, target_json, message, started_at, finished_at
        )
        VALUES (?1, ?2, ?3, 'running', ?4, NULL, ?5, NULL)
        "#,
        params![
            run_id,
            request.action_id,
            request.repo_id,
            serde_json::to_string(&target_json).map_err(json_error)?,
            started_at
        ],
    )
    .map_err(db_error)?;

    for step in &action.steps {
        let run_step_id = slugify_ascii_component(&format!("{}-{}", run_id, step.step_id));
        let step_started_at = now_rfc3339();
        let step_result = apply_repository_action_step(
            &tx,
            &request.repo_id,
            &target_asset_ids,
            step,
            &format!("repository-action:{}", action.action_id),
        );
        let (step_status, step_message) = match step_result {
            Ok(message) => ("success".to_string(), message),
            Err(error) => {
                run_status = "failed".to_string();
                run_message = error.clone();
                ("failed".to_string(), error)
            }
        };
        let step_finished_at = now_rfc3339();
        tx.execute(
            r#"
            INSERT INTO repository_action_run_steps (
              run_step_id, run_id, step_id, repo_id, status, message, started_at, finished_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
            params![
                run_step_id,
                run_id,
                step.step_id,
                request.repo_id,
                step_status,
                step_message,
                step_started_at,
                step_finished_at
            ],
        )
        .map_err(db_error)?;
        if run_status == "failed" {
            break;
        }
    }

    let finished_at = now_rfc3339();
    tx.execute(
        r#"
        UPDATE repository_action_runs
        SET status = ?3, message = ?4, finished_at = ?5
        WHERE repo_id = ?1 AND run_id = ?2
        "#,
        params![
            request.repo_id,
            run_id,
            run_status,
            run_message,
            finished_at
        ],
    )
    .map_err(db_error)?;
    tx.commit().map_err(db_error)?;

    let action = load_repository_action(&connection, &repo.summary.repo_id, &request.action_id)
        .map_err(db_error)?
        .ok_or_else(|| format!("repository action not found: {}", request.action_id))?;
    let run = action
        .last_run
        .clone()
        .ok_or_else(|| "repository action run was not recorded".to_string())?;
    Ok(RepositoryActionRunResponse { action, run })
}
