//! 仓库后端摘要、运行状态、本地缓存与认证状态，以及后端请求解析和网易云缓存配置保留。

use super::*;

/// 插件目录里找不到后端插件时，资源库摘要使用的占位种类。
pub(super) const UNAVAILABLE_BACKEND_KIND: &str = "unavailable";

pub(in super::super) fn backend_summary_from_registry(
    registry: &PluginCatalog,
    plugin_id: &str,
) -> RepositoryBackendSummary {
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    if let Some(manifest) = registry.manifest(&normalized_plugin_id) {
        RepositoryBackendSummary {
            plugin_id: manifest.plugin_id.clone(),
            kind: manifest.kind.clone(),
            name: manifest.name.clone(),
            capabilities: manifest.capabilities.clone(),
        }
    } else {
        RepositoryBackendSummary {
            plugin_id: normalized_plugin_id,
            kind: UNAVAILABLE_BACKEND_KIND.to_string(),
            name: "Unavailable plugin".to_string(),
            capabilities: Vec::new(),
        }
    }
}

/// 后端插件是否已在插件目录注册。未注册时不知道存储该放在资源库目录还是服务目录，也调不了后端。
pub(in super::super) fn backend_summary_is_registered(backend: &RepositoryBackendSummary) -> bool {
    backend.kind != UNAVAILABLE_BACKEND_KIND
}

pub(in super::super) fn repository_runtime_status(
    path: &str,
    backend: &RepositoryBackendSummary,
    stored_status: &str,
) -> String {
    if backend_summary_supports_local_root_access(backend)
        || netease_cache_root_path(path, &backend.plugin_id).is_some()
    {
        if Path::new(path).is_dir() {
            "ready".to_string()
        } else {
            "missing".to_string()
        }
    } else if backend.plugin_id == NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        "missing".to_string()
    } else {
        stored_status.to_string()
    }
}

pub(in super::super) fn backend_uses_repository_root_metadata(
    service_root: &Path,
    repo_root: &Path,
    backend_plugin_id: &str,
) -> bool {
    let plugin_registry = plugin_catalog(service_root);
    let backend = backend_summary_from_registry(&plugin_registry, backend_plugin_id);
    backend_summary_supports_local_root_access(&backend)
        || netease_cache_root_path(&repo_root.to_string_lossy(), backend_plugin_id).is_some()
}

pub(in super::super) fn netease_cache_root_path(
    path: &str,
    backend_plugin_id: &str,
) -> Option<PathBuf> {
    if backend_plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return None;
    }
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.starts_with("netease-cloud-music://") {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

pub(in super::super) fn repository_local_cache_status(
    path: &str,
    backend_plugin_id: &str,
) -> Option<RepositoryLocalCacheStatus> {
    if backend_plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return None;
    }
    let cache_root = netease_cache_root_path(path, backend_plugin_id);
    let status = match cache_root.as_ref() {
        Some(root) if root.is_dir() => "ready",
        Some(_) => "missing",
        None => "unconfigured",
    };
    Some(RepositoryLocalCacheStatus {
        required: true,
        path: cache_root.map(|path| path.to_string_lossy().to_string()),
        status: status.to_string(),
    })
}

/// 仅公开 Source 认证的可用状态，不把后端配置或凭据引用交给宿主界面。
pub(in super::super) fn repository_authentication_status(
    registry: &PluginCatalog,
    backend_plugin_id: &str,
    backend_config: &serde_json::Value,
) -> Option<RepositoryAuthenticationStatus> {
    let manifest = registry.manifest(backend_plugin_id)?;
    manifest
        .contributes
        .pointer("/source/authentication")
        .filter(|value| value.is_object())?;

    let credential_present = backend_config
        .get("credentialRef")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let login_expired = backend_config
        .get("loginExpired")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(!credential_present);
    Some(RepositoryAuthenticationStatus {
        required: true,
        logged_in: credential_present && !login_expired,
        login_expired,
    })
}

pub(in super::super) fn parse_backend_request(
    service_root: &Path,
    request: &RepositoryMutationRequest,
) -> Result<RepositoryBackendRecord, String> {
    let plugin_id = request
        .backend_plugin_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .map(Ok)
        .unwrap_or_else(|| infer_backend_plugin_id_for_path(service_root, &request.path))?;
    let registry = plugin_catalog(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(&plugin_id);
    let registration = registry
        .registration(&normalized_plugin_id)
        .ok_or_else(|| format!("unsupported filesystem backend plugin: {plugin_id}"))?;
    let manifest = &registration.manifest;
    if !is_source_plugin(manifest) {
        return Err(format!(
            "plugin is not a repository source: {}",
            manifest.plugin_id
        ));
    }
    ensure_repository_backend_runtime_available(registration)?;
    let config = request
        .backend_config
        .clone()
        .unwrap_or_else(|| serde_json::json!({}));
    if !config.is_object() {
        return Err("backend config must be a JSON object".to_string());
    }
    Ok(RepositoryBackendRecord {
        plugin_id: manifest.plugin_id.clone(),
        config,
    })
}

pub(in super::super) fn import_backend_record(
    service_root: &Path,
    metadata: &RepositoryMetadataFileImport,
    repo_root: &Path,
) -> Option<RepositoryBackendRecord> {
    let fallback_root_path = repo_root.to_string_lossy().to_string();
    let plugin_id = metadata
        .backend_plugin_id
        .as_deref()
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then_some(trimmed)
        })
        .map(str::to_string)
        .or_else(|| {
            infer_backend_plugin_id_for_path(
                service_root,
                metadata
                    .root_path
                    .as_deref()
                    .unwrap_or(fallback_root_path.as_str()),
            )
            .ok()
        })?;
    let registry = plugin_catalog(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(&plugin_id);
    registry
        .manifest(&normalized_plugin_id)
        .map(|manifest| RepositoryBackendRecord {
            plugin_id: manifest.plugin_id.clone(),
            config: metadata
                .backend_config
                .clone()
                .unwrap_or_else(|| serde_json::json!({})),
        })
}

pub(in super::super) fn rewrite_repository_metadata_if_needed(
    service_root: &Path,
    metadata_path: &Path,
    metadata: &RepositoryMetadataFileImport,
    repo_root: &Path,
    next_root_path: Option<&Path>,
) -> Result<(), String> {
    let root_path = next_root_path
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| metadata.root_path.clone())
        .unwrap_or_else(|| repo_root.to_string_lossy().to_string());
    let normalized_plugin_id = metadata
        .backend_plugin_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|plugin_id| plugin_catalog(service_root).normalize_plugin_id(plugin_id))
        .map(Ok)
        .unwrap_or_else(|| infer_backend_plugin_id_for_path(service_root, &root_path))?;

    if metadata.root_path.as_deref() == Some(root_path.as_str())
        && metadata.backend_plugin_id.as_deref() == Some(normalized_plugin_id.as_str())
    {
        return Ok(());
    }

    let rewritten = RepositoryMetadataFile {
        repo_id: metadata.repo_id.clone(),
        name: metadata
            .name
            .clone()
            .unwrap_or_else(|| infer_repository_name(repo_root)),
        root_path,
        backend_plugin_id: normalized_plugin_id,
        backend_config: metadata
            .backend_config
            .clone()
            .unwrap_or_else(|| serde_json::json!({})),
        created_at: metadata.created_at.clone().unwrap_or_else(now_rfc3339),
        schema_version: metadata.schema_version.unwrap_or(REPO_SCHEMA_VERSION),
    };
    let metadata_json = serde_json::to_string_pretty(&rewritten).map_err(json_error)?;
    fs::write(metadata_path, metadata_json).map_err(io_error)
}

pub(in super::super) fn parse_backend_config_json(
    value: &str,
) -> Result<serde_json::Value, serde_json::Error> {
    let parsed = serde_json::from_str::<serde_json::Value>(value)?;
    if parsed.is_object() {
        Ok(parsed)
    } else {
        Ok(serde_json::json!({}))
    }
}

pub(in super::super) fn preserve_netease_cache_config(
    existing: &RepositoryBackendRecord,
    next_backend_config: &mut serde_json::Value,
) {
    if existing.plugin_id != NETEASE_CLOUD_MUSIC_PLUGIN_ID {
        return;
    }
    let Some(next_object) = next_backend_config.as_object_mut() else {
        return;
    };
    for key in ["sourceUri", "localCachePath"] {
        if next_object.contains_key(key) {
            continue;
        }
        if let Some(value) = existing.config.get(key) {
            next_object.insert(key.to_string(), value.clone());
        }
    }
}

pub(in super::super) fn netease_source_uri_for_repo(repo: &RepositoryRecord) -> String {
    if repo.summary.path.starts_with("netease-cloud-music://") {
        return repo.summary.path.clone();
    }
    repo.backend_record
        .config
        .get("accountId")
        .and_then(normalized_netease_account_id)
        .map(|account_id| format!("netease-cloud-music://account/{account_id}"))
        .unwrap_or_else(|| repo.summary.path.clone())
}

pub(in super::super) fn to_from_sql_error(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(error))
}
