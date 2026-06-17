//! Plugin, cache, and API design operations.

use super::*;
use std::path::Path;

pub(super) fn call_plugin(
    state: &RepositoryState,
    request: PluginCallRequest,
) -> Result<PluginCallResult, String> {
    state.ensure_initialized()?;
    let payload = if request.payload.is_null() {
        serde_json::json!({})
    } else {
        request.payload
    };
    let hook_context =
        plugin_hook_execution_context(&state.root, &request.plugin_id, &request.method);
    let started_at = now_rfc3339();
    let target = plugin_hook_execution_target(&payload);
    let response = backend_plugin_registry(&state.root).call_with_runtime(
        &request.plugin_id,
        &request.method,
        payload,
    );
    if let Some((plugin_id, hook)) = hook_context {
        let (status, message, runtime) = match &response {
            Ok(result) => (
                "success".to_string(),
                "插件 Hook 已执行。".to_string(),
                result.runtime.clone(),
            ),
            Err(error) if is_plugin_call_blocked_error(error) => {
                ("blocked".to_string(), error.clone(), None)
            }
            Err(error) => ("failed".to_string(), error.clone(), None),
        };
        append_plugin_hook_execution_record(
            &state.root,
            PluginHookExecutionRecord {
                execution_id: plugin_hook_execution_id(
                    &plugin_id,
                    &hook.slot,
                    &hook.action,
                    &started_at,
                ),
                plugin_id,
                hook_slot: hook.slot,
                hook_action: hook.action,
                hook_label: hook.label,
                status,
                message,
                target,
                started_at,
                finished_at: now_rfc3339(),
                runtime,
            },
        )?;
    }
    let response = response?;
    Ok(PluginCallResult {
        plugin_id: response.plugin_id,
        method: request.method,
        payload: response.payload,
        runtime: response.runtime,
    })
}

pub(super) fn read_plugin_archive_text(
    state: &RepositoryState,
    request: PluginArchiveReadRequest,
) -> Result<PluginArchiveTextResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_management_registry(&state.root);
    let normalized_plugin_id = registry.normalize_plugin_id(&request.plugin_id);
    let registration = registry
        .registration(&normalized_plugin_id)
        .ok_or_else(|| format!("plugin not found: {}", request.plugin_id))?;
    let archive_path = registration.archive_path.as_path();
    let relative_path = safe_zip_relative_path(request.path.trim())?;
    let archive_entry_path =
        plugin_archive_entry_path(&registration.manifest_prefix, &relative_path);
    let text = read_plugin_archive_text_entry(archive_path, &archive_entry_path)?;
    Ok(PluginArchiveTextResponse {
        plugin_id: registration.manifest.plugin_id.clone(),
        path: archive_entry_path,
        text,
    })
}

pub(super) fn get_plugin_data_directory(
    state: &RepositoryState,
    plugin_id: String,
) -> Result<PluginDataDirectoryResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_management_registry(&state.root);
    let normalized_plugin_id = registry.normalize_plugin_id(&plugin_id);
    let registration = registry
        .registration(&normalized_plugin_id)
        .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
    let data_dir = ensure_plugin_data_dir(&state.root, &registration.manifest.plugin_id)?;
    Ok(PluginDataDirectoryResponse {
        plugin_id: registration.manifest.plugin_id.clone(),
        path: data_dir.to_string_lossy().to_string(),
    })
}

pub(super) fn prepare_plugin_data_file_preview_source(
    state: &RepositoryState,
    request: PluginDataFilePreviewSourceRequest,
) -> Result<PluginDataFilePreviewSourceResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_management_registry(&state.root);
    let normalized_plugin_id = registry.normalize_plugin_id(&request.plugin_id);
    let registration = registry
        .registration(&normalized_plugin_id)
        .ok_or_else(|| format!("plugin not found: {}", request.plugin_id))?;
    let data_dir = ensure_plugin_data_dir(&state.root, &registration.manifest.plugin_id)?;
    let canonical_data_dir = data_dir.canonicalize().map_err(io_error)?;
    let source_path = PathBuf::from(request.path.trim());
    if !source_path.is_absolute() {
        return Err("plugin data preview path must be absolute".to_string());
    }
    if !source_path.is_file() {
        return Err(format!(
            "plugin data preview file is not available: {}",
            source_path.to_string_lossy()
        ));
    }
    let canonical_source_path = source_path.canonicalize().map_err(io_error)?;
    if !canonical_source_path.starts_with(&canonical_data_dir) {
        return Err(format!(
            "plugin data preview path is outside plugin data directory: {}",
            source_path.to_string_lossy()
        ));
    }
    let media_type = request.media_type.trim();
    let media_type = if media_type.is_empty() {
        "application/octet-stream"
    } else {
        media_type
    };
    let metadata = fs::metadata(&canonical_source_path).map_err(io_error)?;
    let modified_at = metadata
        .modified()
        .ok()
        .map(system_time_to_rfc3339)
        .transpose()
        .map_err(time_error)?;
    let token = state.register_preview_source_path(canonical_source_path.clone(), media_type)?;
    Ok(PluginDataFilePreviewSourceResponse {
        plugin_id: registration.manifest.plugin_id.clone(),
        path: canonical_source_path.to_string_lossy().to_string(),
        token,
        source_url: None,
        media_type: media_type.to_string(),
        size_bytes: metadata.len() as i64,
        modified_at,
    })
}

pub(super) fn get_plugin_config(
    state: &RepositoryState,
    plugin_id: String,
) -> Result<PluginConfigSnapshot, String> {
    state.ensure_initialized()?;
    let (manifest, data_dir, values) = state.load_plugin_config_values(&plugin_id)?;
    Ok(plugin_config_snapshot(&manifest, data_dir, values))
}

pub(super) fn set_plugin_config_value(
    state: &RepositoryState,
    request: PluginConfigSetRequest,
) -> Result<PluginConfigSnapshot, String> {
    state.ensure_initialized()?;
    let key = normalize_plugin_config_key(&request.key)?;
    let (manifest, data_dir, mut values) = state.load_plugin_config_values(&request.plugin_id)?;
    let schema = plugin_settings_schema(&manifest);
    validate_plugin_config_value(&schema, &key, &request.value)?;
    values.insert(key, request.value);
    save_plugin_config_values(&data_dir, &values)?;
    Ok(plugin_config_snapshot(&manifest, data_dir, values))
}

pub(super) fn delete_plugin_config_value(
    state: &RepositoryState,
    request: PluginConfigDeleteRequest,
) -> Result<PluginConfigSnapshot, String> {
    state.ensure_initialized()?;
    let key = normalize_plugin_config_key(&request.key)?;
    let (manifest, data_dir, mut values) = state.load_plugin_config_values(&request.plugin_id)?;
    values.remove(&key);
    save_plugin_config_values(&data_dir, &values)?;
    Ok(plugin_config_snapshot(&manifest, data_dir, values))
}

pub(super) fn list_plugins(state: &RepositoryState) -> Result<Vec<PluginManifest>, String> {
    state.ensure_initialized()?;
    Ok(default_plugins(&state.root))
}

pub(super) fn list_plugin_hook_executions(
    state: &RepositoryState,
    request: PluginHookExecutionListRequest,
) -> Result<PluginHookExecutionListResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_management_registry(&state.root);
    let plugin_id = request
        .plugin_id
        .as_deref()
        .map(|value| registry.normalize_plugin_id(value));
    Ok(PluginHookExecutionListResponse {
        records: load_plugin_hook_execution_records(
            &state.root,
            plugin_id.as_deref(),
            request.limit,
        )?,
    })
}

pub(super) fn set_plugin_enabled(
    state: &RepositoryState,
    request: PluginEnabledRequest,
) -> Result<PluginMutationResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_management_registry(&state.root);
    let normalized_plugin_id = registry.normalize_plugin_id(&request.plugin_id);
    let manifest = registry
        .manifest(&normalized_plugin_id)
        .ok_or_else(|| format!("plugin not found: {}", request.plugin_id))?;

    if !request.enabled
        && is_repository_backend_plugin(manifest)
        && state.repository_backend_in_use(&normalized_plugin_id)?
    {
        return Err(format!(
            "plugin is used by an existing repository: {}",
            manifest.plugin_id
        ));
    }

    let mut settings = load_plugin_settings(&state.root)?;
    settings
        .plugins
        .entry(normalized_plugin_id)
        .or_default()
        .enabled = Some(request.enabled);
    save_plugin_settings(&state.root, &settings)?;

    Ok(PluginMutationResponse {
        plugins: default_plugins(&state.root),
    })
}

pub(super) fn delete_plugin(
    state: &RepositoryState,
    plugin_id: String,
) -> Result<PluginMutationResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_management_registry(&state.root);
    let normalized_plugin_id = registry.normalize_plugin_id(&plugin_id);
    let registration = registry
        .registration(&normalized_plugin_id)
        .ok_or_else(|| format!("plugin not found: {plugin_id}"))?;
    if registration.manifest.source != "user" {
        return Err(format!(
            "built-in plugins cannot be deleted: {}",
            registration.manifest.plugin_id
        ));
    }
    if is_repository_backend_plugin(&registration.manifest)
        && state.repository_backend_in_use(&normalized_plugin_id)?
    {
        return Err(format!(
            "plugin is used by an existing repository: {}",
            registration.manifest.plugin_id
        ));
    }

    ensure_runtime_plugin_archive(&state.root, &registration.archive_path)?;
    fs::remove_file(&registration.archive_path).map_err(io_error)?;

    let mut settings = load_plugin_settings(&state.root)?;
    settings.plugins.remove(&normalized_plugin_id);
    save_plugin_settings(&state.root, &settings)?;

    Ok(PluginMutationResponse {
        plugins: default_plugins(&state.root),
    })
}

pub(super) fn install_plugin_from_archive(
    state: &RepositoryState,
    request: PluginInstallRequest,
) -> Result<PluginMutationResponse, String> {
    state.ensure_initialized()?;
    install_plugin_archive(&state.root, Path::new(request.package_path.trim()))?;

    Ok(PluginMutationResponse {
        plugins: default_plugins(&state.root),
    })
}

pub(super) fn get_cache_snapshot(state: &RepositoryState) -> Result<CacheSnapshot, String> {
    state.ensure_initialized()?;
    Ok(CacheSnapshot {
        config: CacheConfig {
            metadata_capacity: 2_048,
            thumbnail_capacity: 512,
            query_capacity: 128,
        },
        entries: default_cache_entries(),
    })
}

pub(super) fn get_api_design_snapshot(
    state: &RepositoryState,
) -> Result<ApiDesignSnapshot, String> {
    state.ensure_initialized()?;
    Ok(ApiDesignSnapshot {
        transport: "REST over local repository service, gRPC-ready contract design".to_string(),
        endpoints: default_api_definitions(&state.root),
    })
}
