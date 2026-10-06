//! External asset import flows shared by the loopback API and repository state facade.

use super::*;

fn log_external_assets(action: &str, message: &str, context: serde_json::Value) {
    crate::app_log!(
        "warn",
        "repository.externalAssets",
        action,
        message,
        context
    );
}

pub(super) fn add_external_assets(
    state: &RepositoryState,
    request_id: String,
    request: ExternalAddAssetRequest,
) -> ExternalAddAssetResponse {
    let total = request.items.len();
    let mut imported = Vec::new();
    let mut failed = Vec::new();

    if request.items.is_empty() {
        failed.push(external_failure(
            0,
            "invalidInput",
            "items cannot be empty".to_string(),
            false,
            None,
        ));
        log_external_assets(
            "importRejected",
            "外部资源导入请求被拒绝。",
            serde_json::json!({
                "requestId": request_id.as_str(),
                "repoId": request.repo_id.as_str(),
                "code": "invalidInput",
                "message": "items cannot be empty",
            }),
        );
        return external_add_asset_response(request_id, imported, failed, total);
    }

    let context = match external_asset_import_context(state, &request_id, &request) {
        Ok(context) => context,
        Err(error) => {
            log_external_assets(
                "contextCreateFailed",
                "外部资源导入上下文创建失败。",
                serde_json::json!({
                    "requestId": request_id.as_str(),
                    "repoId": request.repo_id.as_str(),
                    "code": error.code,
                    "message": error.message.as_str(),
                    "retryable": error.retryable,
                    "total": total,
                }),
            );
            failed.extend((0..total).map(|item_index| {
                external_failure(
                    item_index,
                    error.code,
                    error.message.clone(),
                    error.retryable,
                    None,
                )
            }));
            return external_add_asset_response(request_id, imported, failed, total);
        }
    };

    let mut staged_assets = Vec::<PlannedExternalAsset>::new();
    let mut planned_targets = HashSet::<String>::new();

    for (item_index, item) in request.items.iter().enumerate() {
        match stage_external_asset_item(item_index, item, &context, &mut planned_targets) {
            Ok(staged) => staged_assets.push(staged),
            Err(failure) => failed.push(failure),
        }
    }

    if !staged_assets.is_empty() {
        let source_paths = staged_assets
            .iter()
            .map(|asset| asset.source_path.clone())
            .collect::<Vec<_>>();
        let metadata_by_target_path = staged_assets
            .iter()
            .filter_map(|asset| {
                asset
                    .metadata
                    .as_ref()
                    .map(|metadata| (asset.target_path.clone(), metadata.clone()))
            })
            .collect::<BTreeMap<_, _>>();
        let import_result = state.import_entries(FileImportRequest {
            repo_id: request.repo_id.clone(),
            parent_path: Some(context.parent_path.clone()),
            source_paths,
        });
        match import_result {
            Ok(_) => {
                if let Err(error) = apply_external_asset_metadata(
                    state,
                    &request.repo_id,
                    &metadata_by_target_path,
                    request.client.as_ref(),
                ) {
                    failed.extend(staged_assets.iter().map(|asset| {
                        external_failure(
                            asset.item_index,
                            "internalError",
                            error.clone(),
                            true,
                            None,
                        )
                    }));
                } else {
                    for asset in staged_assets {
                        imported.push(ExternalImportedAsset {
                            item_index: asset.item_index,
                            asset_id: Some(asset_id_for_path(&request.repo_id, &asset.target_path)),
                            path: asset.target_path,
                        });
                    }
                }
            }
            Err(error) => {
                let code = external_import_error_code(&error);
                failed.extend(staged_assets.iter().map(|asset| {
                    external_failure(asset.item_index, code, error.clone(), false, None)
                }));
            }
        }
    }

    if let Err(error) = fs::remove_dir_all(&context.staging_root) {
        log_external_assets(
            "stagingCleanupFailed",
            "外部资源导入临时目录清理失败。",
            serde_json::json!({
                "requestId": request_id.as_str(),
                "path": context.staging_root.to_string_lossy().to_string(),
                "error": error.to_string(),
            }),
        );
    }
    let response = external_add_asset_response(request_id, imported, failed, total);
    if response.status == "failed" || !response.failed.is_empty() {
        log_external_assets(
            "importCompletedWithFailures",
            "外部资源导入存在失败项。",
            serde_json::json!({
                "requestId": response.request_id.as_str(),
                "repoId": request.repo_id.as_str(),
                "status": response.status.as_str(),
                "total": response.summary.total,
                "imported": response.imported.len(),
                "failed": response.failed.len(),
            }),
        );
    } else {
        crate::app_log!(
            "info",
            "repository.externalAssets",
            "importCompleted",
            "外部资源导入完成。",
            serde_json::json!({
                "requestId": response.request_id.as_str(),
                "repoId": request.repo_id.as_str(),
                "total": response.summary.total,
                "imported": response.imported.len(),
            })
        );
    }
    response
}

fn external_asset_import_context(
    state: &RepositoryState,
    request_id: &str,
    request: &ExternalAddAssetRequest,
) -> Result<ExternalAssetImportContext, ExternalRequestError> {
    if let Err(error) = state.ensure_initialized() {
        return Err(ExternalRequestError {
            code: "notReady",
            message: error,
            retryable: true,
        });
    }

    let repo = state
        .load_repository_record(&request.repo_id)
        .map_err(|error| {
            let code = if error.contains("repository not found") {
                "repoNotFound"
            } else {
                "internalError"
            };
            ExternalRequestError {
                code,
                message: error,
                retryable: false,
            }
        })?;
    if repo.summary.status != "ready" {
        return Err(ExternalRequestError {
            code: "repoUnavailable",
            message: format!("repository is not ready: {}", repo.summary.status),
            retryable: true,
        });
    }
    if let Err(error) =
        ensure_repository_supports_local_write_access(&repo, "adding external assets")
    {
        return Err(ExternalRequestError {
            code: "unsupportedRepositoryBackend",
            message: error,
            retryable: false,
        });
    }

    let repo_root = PathBuf::from(&repo.summary.path);
    let parent_path = normalize_directory_path(request.parent_path.as_deref().unwrap_or_default())
        .map_err(|error| ExternalRequestError {
            code: "invalidTargetPath",
            message: error,
            retryable: false,
        })?;
    let target_dir = match resolve_repository_relative_path(&repo_root, &parent_path) {
        Ok(value) if value.exists() && value.is_dir() => value,
        Ok(_) => {
            return Err(ExternalRequestError {
                code: "invalidTargetPath",
                message: format!("directory not found: {parent_path}"),
                retryable: false,
            });
        }
        Err(error) => {
            return Err(ExternalRequestError {
                code: "invalidTargetPath",
                message: error,
                retryable: false,
            });
        }
    };

    let staging_root = state
        .root
        .join("external-imports")
        .join(sanitize_external_component(request_id));
    fs::create_dir_all(&staging_root)
        .map_err(io_error)
        .map_err(|error| ExternalRequestError {
            code: "internalError",
            message: error,
            retryable: true,
        })?;

    Ok(ExternalAssetImportContext {
        parent_path,
        target_dir,
        staging_root,
    })
}

fn apply_external_asset_metadata(
    state: &RepositoryState,
    repo_id: &str,
    metadata_by_path: &BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    client: Option<&ExternalAddAssetClient>,
) -> Result<(), String> {
    if metadata_by_path.is_empty() {
        return Ok(());
    }
    let repo = state.load_repository_record(repo_id)?;
    let mut connection = state.open_repository_connection(
        &repo.summary.repo_id,
        &repo.summary.path,
        &repo.backend_record,
    )?;
    let tx = connection.transaction().map_err(db_error)?;
    let source = external_metadata_source(client);
    for (path, metadata) in metadata_by_path {
        let asset_id = tx
            .query_row(
                "SELECT asset_id FROM assets WHERE repo_id = ?1 AND path = ?2 AND status != 'deleted'",
                params![repo_id, path],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?
            .ok_or_else(|| format!("imported asset not found: {path}"))?;
        update_metadata_for_asset_in_transaction(&tx, repo_id, &asset_id, metadata, &source)
            .map_err(db_error)?;
    }
    tx.commit().map_err(db_error)?;
    Ok(())
}
