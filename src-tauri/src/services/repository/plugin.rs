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

pub(crate) fn parse_plugin_manifest(raw: &str) -> Result<PluginManifest, String> {
    parse_plugin_manifest_with_source(raw, None)
}

pub(super) fn parse_plugin_manifest_with_source(
    raw: &str,
    source_override: Option<&str>,
) -> Result<PluginManifest, String> {
    let mut manifest = serde_json::from_str::<PluginManifest>(raw).map_err(json_error)?;
    manifest.plugin_id = normalized_builtin_plugin_id(&manifest.plugin_id).to_string();
    if manifest.category.trim().is_empty() {
        manifest.category = plugin_category_for_kind(&manifest.kind).to_string();
    }
    if manifest.contributes.is_null() {
        manifest.contributes = serde_json::json!({});
    }
    if let Some(source) = source_override {
        manifest.source = source.to_string();
    }
    manifest.legacy_plugin_ids = plugin_legacy_ids(&manifest);
    manifest.compat.legacy_plugin_ids = plugin_legacy_ids(&manifest);
    if manifest.compat.sdk_version.trim().is_empty() {
        manifest.compat.sdk_version = PLUGIN_SDK_VERSION.to_string();
    }
    Ok(manifest)
}

pub(super) fn plugin_legacy_ids(manifest: &PluginManifest) -> Vec<String> {
    let mut values = manifest.legacy_plugin_ids.clone();
    values.extend(manifest.compat.legacy_plugin_ids.clone());
    values.sort();
    values.dedup();
    values
}

pub(super) fn resolve_plugin_manifest_dependencies(manifests: &mut [PluginManifest]) {
    let by_id = manifests
        .iter()
        .map(|manifest| (manifest.plugin_id.clone(), manifest.clone()))
        .collect::<BTreeMap<_, _>>();
    let legacy_ids = manifests
        .iter()
        .flat_map(|manifest| {
            plugin_legacy_ids(manifest)
                .into_iter()
                .map(|legacy_id| (legacy_id, manifest.plugin_id.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<BTreeMap<_, _>>();

    for manifest in manifests {
        let required = resolve_plugin_dependency_list(&manifest.requires, &by_id, &legacy_ids);
        let optional = resolve_plugin_dependency_list(&manifest.optional, &by_id, &legacy_ids);
        let missing_required = dependency_ids_by_status(&required, "missing");
        let missing_optional = dependency_ids_by_status(&optional, "missing");
        let unavailable_required = unavailable_dependency_ids(&required);
        let unavailable_optional = unavailable_dependency_ids(&optional);

        manifest.dependency_status = PluginDependencyStatus {
            required,
            optional,
            missing_required: missing_required.clone(),
            missing_optional: missing_optional.clone(),
            disabled_required: unavailable_required.clone(),
            disabled_optional: unavailable_optional.clone(),
        };

        if !missing_required.is_empty() {
            manifest.enabled = false;
            manifest.status = "unavailable".to_string();
            manifest.disable_reason = Some(format!(
                "缺少必需依赖：{}。",
                missing_required.join("、")
            ));
        } else if !unavailable_required.is_empty() {
            manifest.enabled = false;
            manifest.status = "disabled".to_string();
            manifest.disable_reason = Some(format!(
                "必需依赖不可用：{}。",
                unavailable_required.join("、")
            ));
        } else if manifest.disable_reason.is_none() {
            if !manifest.enabled || manifest.status == "disabled" {
                manifest.disable_reason = Some("插件已被禁用。".to_string());
            } else if manifest.status == "unavailable" {
                manifest.disable_reason = Some("插件运行时不可用。".to_string());
            } else if manifest.status == "error" {
                manifest.disable_reason = Some("插件清单或运行时存在错误。".to_string());
            }
        }

        manifest.degraded = !missing_optional.is_empty() || !unavailable_optional.is_empty();
        manifest.degradation_reason = if manifest.degraded {
            Some(format!(
                "可选依赖不可用，部分能力降级：{}。",
                missing_optional
                    .iter()
                    .chain(unavailable_optional.iter())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("、")
            ))
        } else {
            None
        };
    }
}

fn resolve_plugin_dependency_list(
    dependency_ids: &[String],
    by_id: &BTreeMap<String, PluginManifest>,
    legacy_ids: &BTreeMap<String, String>,
) -> Vec<PluginDependencyState> {
    dependency_ids
        .iter()
        .map(|dependency_id| resolve_plugin_dependency(dependency_id, by_id, legacy_ids))
        .collect()
}

fn resolve_plugin_dependency(
    dependency_id: &str,
    by_id: &BTreeMap<String, PluginManifest>,
    legacy_ids: &BTreeMap<String, String>,
) -> PluginDependencyState {
    let normalized = legacy_ids
        .get(dependency_id)
        .cloned()
        .unwrap_or_else(|| dependency_id.to_string());
    let Some(dependency) = by_id.get(&normalized) else {
        return PluginDependencyState {
            plugin_id: dependency_id.to_string(),
            name: None,
            status: "missing".to_string(),
            enabled: false,
            available: false,
        };
    };
    let available =
        plugin_dependency_available(&normalized, by_id, legacy_ids, &mut HashSet::new());
    let status = if available {
        "ready"
    } else if dependency.status == "unavailable" || dependency.status == "error" {
        dependency.status.as_str()
    } else {
        "disabled"
    };
    PluginDependencyState {
        plugin_id: dependency.plugin_id.clone(),
        name: Some(dependency.name.clone()),
        status: status.to_string(),
        enabled: dependency.enabled,
        available,
    }
}

fn plugin_dependency_available(
    plugin_id: &str,
    by_id: &BTreeMap<String, PluginManifest>,
    legacy_ids: &BTreeMap<String, String>,
    visited: &mut HashSet<String>,
) -> bool {
    let normalized = legacy_ids
        .get(plugin_id)
        .cloned()
        .unwrap_or_else(|| plugin_id.to_string());
    if !visited.insert(normalized.clone()) {
        return true;
    }
    let Some(manifest) = by_id.get(&normalized) else {
        return false;
    };
    if !manifest.enabled || !matches!(manifest.status.as_str(), "ready" | "") {
        return false;
    }
    manifest.requires.iter().all(|dependency_id| {
        plugin_dependency_available(dependency_id, by_id, legacy_ids, visited)
    })
}

fn dependency_ids_by_status(dependencies: &[PluginDependencyState], status: &str) -> Vec<String> {
    dependencies
        .iter()
        .filter(|dependency| dependency.status == status)
        .map(|dependency| {
            dependency
                .name
                .clone()
                .unwrap_or_else(|| dependency.plugin_id.clone())
        })
        .collect()
}

fn unavailable_dependency_ids(dependencies: &[PluginDependencyState]) -> Vec<String> {
    dependencies
        .iter()
        .filter(|dependency| dependency.status != "ready" && dependency.status != "missing")
        .map(|dependency| {
            dependency
                .name
                .clone()
                .unwrap_or_else(|| dependency.plugin_id.clone())
        })
        .collect()
}

pub(super) fn normalized_builtin_plugin_id(plugin_id: &str) -> &str {
    match plugin_id.trim() {
        LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID => LOCAL_FILESYSTEM_PLUGIN_ID,
        other => other,
    }
}

pub(super) fn plugin_category_for_kind(kind: &str) -> &'static str {
    match kind {
        "filesystem" | "webdav" | "cloud" => "source",
        "preview" => "preview",
        "library-kind" => "library-kind",
        "parser" => "parser",
        "metadata" | "provider" | "search" | "watcher" | "download" | "ocr" | "asr" | "ai" => {
            "service"
        }
        _ => "service",
    }
}

pub(super) fn is_source_plugin(manifest: &PluginManifest) -> bool {
    manifest.category == "source"
        || matches!(manifest.kind.as_str(), "filesystem" | "webdav" | "cloud")
}

pub(super) fn ensure_repository_backend_runtime_available(
    registration: &BackendPluginRegistration,
) -> Result<(), String> {
    let manifest = &registration.manifest;
    if !manifest.enabled || manifest.status == "disabled" {
        return Err(format!("plugin is disabled: {}", manifest.plugin_id));
    }
    if embedded_local_filesystem_fallback_enabled(&manifest.plugin_id) {
        return Ok(());
    }
    if manifest.sdk != "backend" || manifest.runtime != "native-dylib" {
        return Err(format!(
            "plugin runtime is not available: {}",
            manifest.plugin_id
        ));
    }
    if registration.native.is_some() {
        return Ok(());
    }
    if let Some(error) = &registration.load_error {
        return Err(format!(
            "plugin runtime is not available: {} ({error})",
            manifest.plugin_id
        ));
    }
    Err(format!(
        "plugin runtime is not available: {}",
        manifest.plugin_id
    ))
}

fn plugin_settings_path(service_root: &Path) -> PathBuf {
    service_root.join("plugin-state.json")
}

pub(super) fn broken_plugin_manifest(archive_path: &Path, error: &str) -> PluginManifest {
    let file_stem = archive_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("broken-plugin");
    PluginManifest {
        plugin_id: format!("broken.{}", slugify_ascii_component(file_stem)),
        legacy_plugin_ids: Vec::new(),
        name: format!("Broken Plugin ({file_stem})"),
        version: "0.0.0".to_string(),
        r#type: Some(PluginTypeDefinition {
            layer: "integration-capability-hook".to_string(),
            kind: "broken".to_string(),
        }),
        kind: "broken".to_string(),
        category: "service".to_string(),
        description: format!("Failed to read plugin archive: {error}"),
        capabilities: Vec::new(),
        enabled: false,
        sdk: "backend".to_string(),
        entry: serde_json::json!({}),
        contributes: serde_json::json!({}),
        source: "user".to_string(),
        runtime: "manifest-only".to_string(),
        permissions: Vec::new(),
        requires: Vec::new(),
        optional: Vec::new(),
        hooks: Vec::new(),
        compat: PluginCompat {
            sdk_version: PLUGIN_SDK_VERSION.to_string(),
            legacy_plugin_ids: Vec::new(),
        },
        status: "error".to_string(),
        dependency_status: PluginDependencyStatus::default(),
        disable_reason: Some("插件清单读取失败。".to_string()),
        degraded: false,
        degradation_reason: None,
        archive_path: Some(archive_path.to_string_lossy().to_string()),
    }
}

pub(crate) fn runtime_plugins_dir(service_root: &Path) -> PathBuf {
    service_root.join("plugins")
}

pub(super) fn plugin_data_root_dir(service_root: &Path) -> PathBuf {
    service_root.join("plugin-data")
}

pub(crate) fn plugin_data_dir(service_root: &Path, plugin_id: &str) -> PathBuf {
    plugin_data_root_dir(service_root).join(plugin_data_dir_name(plugin_id))
}

fn plugin_data_dir_name(plugin_id: &str) -> String {
    let slug = slugify_ascii_component(plugin_id);
    if slug.is_empty() {
        format!("plugin-{}", hash_text_sha256(plugin_id))
    } else {
        slug
    }
}

fn plugin_hook_executions_path(service_root: &Path) -> PathBuf {
    service_root.join(PLUGIN_HOOK_EXECUTIONS_FILE_NAME)
}

fn plugin_hook_execution_context(
    service_root: &Path,
    plugin_id: &str,
    method: &str,
) -> Option<(String, PluginHook)> {
    let registry = plugin_management_registry(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    let manifest = registry.resolved_manifest(&normalized_plugin_id)?;
    manifest
        .hooks
        .iter()
        .find(|hook| hook.action == method)
        .cloned()
        .map(|hook| (manifest.plugin_id, hook))
}

fn plugin_hook_execution_target(payload: &serde_json::Value) -> serde_json::Value {
    const TARGET_KEYS: [&str; 8] = [
        "repoId",
        "assetId",
        "assetIds",
        "path",
        "paths",
        "playlistId",
        "query",
        "id",
    ];
    let Some(object) = payload.as_object() else {
        return serde_json::json!({});
    };
    let mut target = serde_json::Map::new();
    for key in TARGET_KEYS {
        if let Some(value) = object.get(key) {
            target.insert(key.to_string(), value.clone());
        }
    }
    serde_json::Value::Object(target)
}

fn plugin_hook_execution_id(
    plugin_id: &str,
    hook_slot: &str,
    hook_action: &str,
    started_at: &str,
) -> String {
    format!(
        "plugin-hook-{}",
        hash_text_sha256(&format!(
            "{plugin_id}\n{hook_slot}\n{hook_action}\n{started_at}"
        ))
    )
}

fn is_plugin_call_blocked_error(error: &str) -> bool {
    error.contains("plugin call blocked by dependency status")
        || error.contains("plugin is disabled")
        || error.contains("插件不可用")
}

fn append_plugin_hook_execution_record(
    service_root: &Path,
    record: PluginHookExecutionRecord,
) -> Result<(), String> {
    fs::create_dir_all(service_root).map_err(io_error)?;
    let raw = serde_json::to_string(&record).map_err(json_error)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(plugin_hook_executions_path(service_root))
        .map_err(io_error)?;
    writeln!(file, "{raw}").map_err(io_error)
}

fn load_plugin_hook_execution_records(
    service_root: &Path,
    plugin_id: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<PluginHookExecutionRecord>, String> {
    let path = plugin_hook_executions_path(service_root);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let limit = limit.unwrap_or(50).clamp(1, 200);
    let raw = fs::read_to_string(path).map_err(io_error)?;
    let mut records = raw
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            serde_json::from_str::<PluginHookExecutionRecord>(line).ok()
        })
        .filter(|record| {
            plugin_id
                .map(|expected| record.plugin_id == expected)
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        right
            .started_at
            .cmp(&left.started_at)
            .then_with(|| right.execution_id.cmp(&left.execution_id))
    });
    records.truncate(limit);
    Ok(records)
}

pub(super) fn ensure_plugin_data_dir(service_root: &Path, plugin_id: &str) -> Result<PathBuf, String> {
    let data_dir = plugin_data_dir(service_root, plugin_id);
    fs::create_dir_all(&data_dir).map_err(io_error)?;
    Ok(data_dir)
}

fn plugin_config_path(plugin_data_dir: &Path) -> PathBuf {
    plugin_data_dir.join("config.json")
}

pub(super) fn load_plugin_config_values(
    plugin_data_dir: &Path,
) -> Result<BTreeMap<String, serde_json::Value>, String> {
    let path = plugin_config_path(plugin_data_dir);
    if !path.is_file() {
        return Ok(BTreeMap::new());
    }
    let raw = fs::read_to_string(&path).map_err(io_error)?;
    if raw.trim().is_empty() {
        return Ok(BTreeMap::new());
    }
    serde_json::from_str::<BTreeMap<String, serde_json::Value>>(&raw).map_err(json_error)
}

pub(super) fn save_plugin_config_values(
    plugin_data_dir: &Path,
    values: &BTreeMap<String, serde_json::Value>,
) -> Result<(), String> {
    fs::create_dir_all(plugin_data_dir).map_err(io_error)?;
    let raw = serde_json::to_string_pretty(values).map_err(json_error)?;
    fs::write(plugin_config_path(plugin_data_dir), raw).map_err(io_error)
}

fn plugin_settings_schema(manifest: &PluginManifest) -> serde_json::Value {
    manifest
        .contributes
        .as_object()
        .and_then(|contributes| contributes.get("settings"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

fn plugin_config_snapshot(
    manifest: &PluginManifest,
    data_dir: PathBuf,
    values: BTreeMap<String, serde_json::Value>,
) -> PluginConfigSnapshot {
    PluginConfigSnapshot {
        plugin_id: manifest.plugin_id.clone(),
        data_directory: data_dir.to_string_lossy().to_string(),
        schema: plugin_settings_schema(manifest),
        values,
    }
}

fn normalize_plugin_config_key(key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        Err("plugin config key cannot be empty".to_string())
    } else {
        Ok(key.to_string())
    }
}

fn validate_plugin_config_value(
    schema: &serde_json::Value,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    let Some(field) = schema
        .get("fields")
        .and_then(|fields| fields.as_array())
        .and_then(|fields| {
            fields.iter().find(|field| {
                field.get("key").and_then(|field_key| field_key.as_str()) == Some(key)
            })
        })
    else {
        return Ok(());
    };

    let field_type = field
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or("string");
    match field_type {
        "boolean" if !value.is_boolean() => {
            Err(format!("plugin config value must be boolean: {key}"))
        }
        "number" if value.as_f64().is_none() => {
            Err(format!("plugin config value must be number: {key}"))
        }
        "string" if !value.is_string() => {
            Err(format!("plugin config value must be string: {key}"))
        }
        "select" => validate_plugin_select_config_value(field, key, value),
        _ => Ok(()),
    }
}

fn validate_plugin_select_config_value(
    field: &serde_json::Value,
    key: &str,
    value: &serde_json::Value,
) -> Result<(), String> {
    if !(value.is_string() || value.is_number() || value.is_boolean()) {
        return Err(format!(
            "plugin config value must be a scalar option: {key}"
        ));
    }
    let Some(options) = field.get("options").and_then(|options| options.as_array()) else {
        return Ok(());
    };
    let valid = options.iter().any(|option| {
        option
            .get("value")
            .map(|option_value| option_value == value)
            .unwrap_or(false)
    });
    if valid {
        Ok(())
    } else {
        Err(format!(
            "plugin config value is not an allowed option: {key}"
        ))
    }
}

fn plugin_runtime_cache_dir(service_root: &Path) -> PathBuf {
    service_root.join("plugin-cache")
}

pub(super) fn load_plugin_settings(service_root: &Path) -> Result<PluginSettings, String> {
    let path = plugin_settings_path(service_root);
    if !path.is_file() {
        return Ok(PluginSettings::default());
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    serde_json::from_str::<PluginSettings>(&raw).map_err(json_error)
}

pub(super) fn save_plugin_settings(
    service_root: &Path,
    settings: &PluginSettings,
) -> Result<(), String> {
    fs::create_dir_all(service_root).map_err(io_error)?;
    let raw = serde_json::to_string_pretty(settings).map_err(json_error)?;
    fs::write(plugin_settings_path(service_root), raw).map_err(io_error)
}

pub(super) fn apply_plugin_settings(manifest: &mut PluginManifest, settings: &PluginSettings) {
    let Some(entry) = settings.plugins.get(&manifest.plugin_id) else {
        if !manifest.enabled {
            manifest.status = "disabled".to_string();
        }
        return;
    };
    if let Some(enabled) = entry.enabled {
        manifest.enabled = enabled;
        manifest.status = if enabled {
            "ready".to_string()
        } else {
            "disabled".to_string()
        };
    } else if !manifest.enabled {
        manifest.status = "disabled".to_string();
    }
}

pub(crate) fn is_repository_backend_plugin(manifest: &PluginManifest) -> bool {
    is_source_plugin(manifest)
}

fn ensure_runtime_plugin_archive(service_root: &Path, archive_path: &Path) -> Result<(), String> {
    let runtime_root = runtime_plugins_dir(service_root);
    fs::create_dir_all(&runtime_root).map_err(io_error)?;
    let runtime_root = runtime_root.canonicalize().map_err(io_error)?;
    let archive_path = archive_path.canonicalize().map_err(io_error)?;
    if archive_path.starts_with(&runtime_root) {
        Ok(())
    } else {
        Err(format!(
            "plugin archive is outside the runtime plugin root: {}",
            archive_path.display()
        ))
    }
}

fn install_plugin_archive(service_root: &Path, package_path: &Path) -> Result<PluginManifest, String> {
    if package_path.as_os_str().is_empty() {
        return Err("plugin package path cannot be empty".to_string());
    }
    if package_path.extension().and_then(|value| value.to_str()) != Some("momoplug") {
        return Err("plugin package must use the .momoplug extension".to_string());
    }
    if !package_path.is_file() {
        return Err(format!(
            "plugin package not found: {}",
            package_path.display()
        ));
    }

    let file = File::open(package_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let (manifest_index, _) = find_zip_plugin_manifest(&mut archive)?;
    let mut manifest_raw = String::new();
    archive
        .by_index(manifest_index)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut manifest_raw)
        .map_err(io_error)?;

    let manifest = parse_plugin_manifest_with_source(&manifest_raw, Some("user"))?;
    if manifest.plugin_id.trim().is_empty() {
        return Err("plugin manifest is missing pluginId".to_string());
    }

    let existing_registry = plugin_management_registry(service_root);
    if existing_registry.manifest(&manifest.plugin_id).is_some() {
        return Err(format!("plugin already exists: {}", manifest.plugin_id));
    }

    let runtime_root = runtime_plugins_dir(service_root);
    fs::create_dir_all(&runtime_root).map_err(io_error)?;
    let install_name = format!(
        "{}-{}.momoplug",
        slugify_ascii_component(&manifest.plugin_id),
        manifest.version
    );
    let target_path = runtime_root.join(install_name);
    if target_path.exists() {
        return Err(format!(
            "plugin package already exists: {}",
            target_path.display()
        ));
    }
    fs::copy(package_path, &target_path).map_err(io_error)?;
    let mut installed = manifest;
    installed.archive_path = Some(target_path.to_string_lossy().to_string());
    Ok(installed)
}

fn find_zip_plugin_manifest<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Result<(usize, String), String> {
    let mut matches = Vec::new();
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|error| error.to_string())?
            .name()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if let Some(prefix) = name.strip_suffix("/manifest.json") {
            if !prefix.is_empty() && !prefix.split('/').any(|part| part == "..") {
                matches.push((index, format!("{prefix}/")));
            }
        }
    }
    if matches.len() == 1 {
        return Ok(matches.remove(0));
    }
    if matches.is_empty() {
        return Err(
            "plugin archive must contain exactly one root directory with manifest.json".to_string(),
        );
    }
    Err("plugin archive contains multiple manifest roots".to_string())
}

pub(crate) fn extract_zip_plugin<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    manifest_prefix: &str,
    staging_dir: &Path,
) -> Result<(), String> {
    let prefix = manifest_prefix.trim_start_matches('/').replace('\\', "/");
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let entry_name = entry
            .name()
            .replace('\\', "/")
            .trim_start_matches('/')
            .to_string();
        if !prefix.is_empty() && !entry_name.starts_with(&prefix) {
            continue;
        }
        let relative_name = if prefix.is_empty() {
            entry_name.as_str()
        } else {
            &entry_name[prefix.len()..]
        };
        if relative_name.is_empty() {
            continue;
        }
        let relative_path = safe_zip_relative_path(relative_name)?;
        let output_path = staging_dir.join(&relative_path);
        if entry.is_dir() {
            fs::create_dir_all(&output_path).map_err(io_error)?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(io_error)?;
        }
        let mut output = File::create(&output_path).map_err(io_error)?;
        std::io::copy(&mut entry, &mut output).map_err(io_error)?;
        output.flush().map_err(io_error)?;
    }
    Ok(())
}

pub(super) fn safe_zip_relative_path(value: &str) -> Result<PathBuf, String> {
    let mut path = PathBuf::new();
    for part in value.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains(':') || part.contains('\\') {
            return Err(format!("unsafe plugin archive path: {value}"));
        }
        path.push(part);
    }
    if path.as_os_str().is_empty() {
        Err(format!("unsafe plugin archive path: {value}"))
    } else {
        Ok(path)
    }
}

pub(super) fn load_native_plugin(
    manifest: &PluginManifest,
    archive_path: &Path,
    manifest_prefix: &str,
    service_root: &Path,
) -> Result<NativePlugin, String> {
    let library_name = manifest
        .entry
        .get("backend")
        .and_then(serde_json::Value::as_object)
        .and_then(|entry| entry.get("library"))
        .or_else(|| manifest.entry.get("library"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "native plugin is missing entry.library: {}",
                manifest.plugin_id
            )
        })?;
    let library_path =
        native_plugin_library_path(service_root, archive_path, manifest_prefix, library_name)?;
    let library = unsafe { libloading::Library::new(&library_path) }.map_err(|error| {
        format!(
            "failed to load plugin library {}: {error}",
            library_path.display()
        )
    })?;
    let (call, free) = unsafe {
        let manifest_fn: libloading::Symbol<PluginManifestFn> = library
            .get(b"momobako_plugin_manifest")
            .map_err(|error| format!("missing momobako_plugin_manifest: {error}"))?;
        let manifest_ptr = manifest_fn();
        if !manifest_ptr.is_null() {
            let free_fn: libloading::Symbol<PluginFreeFn> = library
                .get(b"momobako_plugin_free")
                .map_err(|error| format!("missing momobako_plugin_free: {error}"))?;
            free_fn(manifest_ptr);
        }
        let call = *library
            .get::<PluginCallFn>(b"momobako_plugin_call")
            .map_err(|error| format!("missing momobako_plugin_call: {error}"))?;
        let free = *library
            .get::<PluginFreeFn>(b"momobako_plugin_free")
            .map_err(|error| format!("missing momobako_plugin_free: {error}"))?;
        (call, free)
    };
    Ok(NativePlugin {
        _library: library,
        call,
        free,
    })
}

fn native_plugin_library_path(
    service_root: &Path,
    archive_path: &Path,
    manifest_prefix: &str,
    library_name: &str,
) -> Result<PathBuf, String> {
    let file_name = native_plugin_library_file_name(library_name);
    let cache_root = plugin_runtime_cache_dir(service_root);
    fs::create_dir_all(&cache_root).map_err(io_error)?;
    let archive_hash = hash_file_sha256(archive_path)?;
    let cache_dir = cache_root.join(format!(
        "{}-{}",
        slugify_ascii_component(library_name),
        archive_hash
    ));
    let output_path = cache_dir.join(&file_name);
    if output_path.is_file() {
        return Ok(output_path);
    }
    fs::create_dir_all(&cache_dir).map_err(io_error)?;
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let prefixed_file_name = plugin_archive_entry_path(manifest_prefix, Path::new(&file_name));
    let prefixed_library_dir = plugin_archive_entry_path(
        manifest_prefix,
        Path::new(&format!("{library_name}/{file_name}")),
    );
    let prefixed_dist_file =
        plugin_archive_entry_path(manifest_prefix, Path::new(&format!("dist/{file_name}")));
    let prefixed_dist_library = plugin_archive_entry_path(
        manifest_prefix,
        Path::new(&format!("dist/{library_name}/{file_name}")),
    );
    let candidate_names = [
        file_name.clone(),
        format!("{library_name}/{file_name}"),
        format!("dist/{file_name}"),
        format!("dist/{library_name}/{file_name}"),
        prefixed_file_name,
        prefixed_library_dir,
        prefixed_dist_file,
        prefixed_dist_library,
    ];
    for candidate_name in candidate_names {
        if let Ok(mut entry) = archive.by_name(&candidate_name) {
            let mut output = File::create(&output_path).map_err(io_error)?;
            std::io::copy(&mut entry, &mut output).map_err(io_error)?;
            output.flush().map_err(io_error)?;
            return Ok(output_path);
        }
    }
    Err(format!(
        "native plugin library not found in archive: {library_name}"
    ))
}

fn native_plugin_library_file_name(library_name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("{library_name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("lib{library_name}.dylib")
    } else {
        format!("lib{library_name}.so")
    }
}

pub(super) fn embedded_local_filesystem_fallback_enabled(plugin_id: &str) -> bool {
    plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
}

pub(super) fn default_plugins(service_root: &Path) -> Vec<PluginManifest> {
    backend_plugin_registry(service_root).list_manifests()
}

pub(super) fn read_plugin_manifest_from_archive(
    archive_path: &Path,
) -> Result<(String, String), String> {
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let (manifest_index, manifest_prefix) = find_zip_plugin_manifest(&mut archive)?;
    let mut manifest_raw = String::new();
    archive
        .by_index(manifest_index)
        .map_err(|error| error.to_string())?
        .read_to_string(&mut manifest_raw)
        .map_err(io_error)?;
    Ok((manifest_raw, manifest_prefix))
}

fn read_plugin_archive_text_entry(archive_path: &Path, entry_path: &str) -> Result<String, String> {
    let file = File::open(archive_path).map_err(io_error)?;
    let mut archive = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    let mut archive_entry = archive
        .by_name(entry_path)
        .map_err(|_| format!("plugin archive entry not found: {entry_path}"))?;
    let mut text = String::new();
    archive_entry.read_to_string(&mut text).map_err(io_error)?;
    Ok(text)
}

pub(super) fn plugin_archive_entry_path(prefix: &str, relative_path: &Path) -> String {
    let relative = relative_path.to_string_lossy().replace('\\', "/");
    if prefix.is_empty() {
        relative
    } else {
        format!("{prefix}{relative}")
    }
}

fn hash_file_sha256(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn hash_text_sha256(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}
