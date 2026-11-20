//! 插件服务入口：插件调用、数据目录、配置读写、启用删除、安装与 API 设计快照。

use super::*;

pub(in super::super) fn call_plugin(
    state: &RepositoryState,
    request: PluginCallRequest,
) -> Result<PluginCallResult, String> {
    state.ensure_initialized()?;
    let mut payload = if request.payload.is_null() {
        serde_json::json!({})
    } else {
        request.payload
    };
    if let Some(repository_id) = request.repository_id.as_deref() {
        let repository = state.load_repository_record(repository_id)?;
        let catalog = plugin_catalog(&state.root);
        let requested_plugin_id = catalog.normalize_plugin_id(&request.plugin_id);
        let repository_plugin_id =
            catalog.normalize_plugin_id(&repository.backend_record.plugin_id);
        if requested_plugin_id != repository_plugin_id {
            return Err(format!(
                "repository backend mismatch: {repository_id} uses {repository_plugin_id}, not {requested_plugin_id}"
            ));
        }
        let object = payload.as_object_mut().ok_or_else(|| {
            "plugin payload must be an object when repositoryId is provided".to_string()
        })?;
        object.insert("repoId".to_string(), serde_json::json!(repository_id));
        object.insert(
            "repoRoot".to_string(),
            serde_json::json!(repository.summary.path),
        );
        object.insert("config".to_string(), repository.backend_record.config);
    }
    let hook_context =
        plugin_hook_execution_context(&state.root, &request.plugin_id, &request.method);
    let started_at = now_rfc3339();
    let target = plugin_hook_execution_target(&payload);
    let response =
        plugin_catalog(&state.root).call_with_runtime(&request.plugin_id, &request.method, payload);
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

pub(in super::super) fn get_plugin_data_directory(
    state: &RepositoryState,
    plugin_id: String,
) -> Result<PluginDataDirectoryResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_catalog(&state.root);
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

pub(in super::super) fn get_plugin_config(
    state: &RepositoryState,
    plugin_id: String,
) -> Result<PluginConfigSnapshot, String> {
    state.ensure_initialized()?;
    let (manifest, data_dir, values) = state.load_plugin_config_values(&plugin_id)?;
    Ok(plugin_config_snapshot(&manifest, data_dir, values))
}

pub(in super::super) fn set_plugin_config_value(
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

pub(in super::super) fn delete_plugin_config_value(
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

pub(in super::super) fn list_plugins(
    state: &RepositoryState,
) -> Result<Vec<PluginManifest>, String> {
    state.ensure_initialized()?;
    Ok(default_plugins(&state.root))
}

pub(in super::super) fn list_plugin_hook_executions(
    state: &RepositoryState,
    request: PluginHookExecutionListRequest,
) -> Result<PluginHookExecutionListResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_catalog(&state.root);
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

pub(in super::super) fn set_plugin_enabled(
    state: &RepositoryState,
    request: PluginEnabledRequest,
) -> Result<PluginMutationResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_catalog(&state.root);
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

pub(in super::super) fn delete_plugin(
    state: &RepositoryState,
    plugin_id: String,
) -> Result<PluginMutationResponse, String> {
    state.ensure_initialized()?;
    let registry = plugin_catalog(&state.root);
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

pub(in super::super) fn install_plugin_from_archive(
    state: &RepositoryState,
    request: PluginInstallRequest,
) -> Result<PluginMutationResponse, String> {
    state.ensure_initialized()?;
    install_plugin_archive(&state.root, Path::new(request.package_path.trim()))?;

    Ok(PluginMutationResponse {
        plugins: default_plugins(&state.root),
    })
}

pub(in super::super) fn get_api_design_snapshot(
    state: &RepositoryState,
) -> Result<ApiDesignSnapshot, String> {
    state.ensure_initialized()?;
    Ok(ApiDesignSnapshot {
        transport: "宿主进程内调用；外部客户端走本机回环 HTTP（/external/v1）".to_string(),
        endpoints: default_api_definitions(&state.root),
    })
}
