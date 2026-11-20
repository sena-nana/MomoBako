//! 插件清单解析、后端形态校验、依赖解析与插件分类判断。

use super::*;

#[allow(dead_code)]
pub(crate) fn parse_plugin_manifest(raw: &str) -> Result<PluginManifest, String> {
    parse_plugin_manifest_with_source(raw, None)
}

pub(in super::super) fn parse_plugin_manifest_with_source(
    raw: &str,
    source_override: Option<&str>,
) -> Result<PluginManifest, String> {
    let mut manifest = serde_json::from_str::<PluginManifest>(raw).map_err(json_error)?;
    manifest.plugin_id = normalized_builtin_plugin_id(&manifest.plugin_id).to_string();
    ensure_backend_plugin_form(&manifest)?;
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

/// 插件只有后端形态：`sdk` 是 `backend`，`runtime` 是 `native-dylib`、`process` 或 `manifest-only`。
/// 别的形态（包括已删除的前端模块插件）读清单时就拒绝，插件目录里显示为读取失败的插件。
pub(super) fn ensure_backend_plugin_form(manifest: &PluginManifest) -> Result<(), String> {
    if manifest.sdk == "backend"
        && matches!(
            manifest.runtime.as_str(),
            "native-dylib" | "process" | "manifest-only"
        )
    {
        return Ok(());
    }
    crate::app_log!(
        "warn",
        "plugin.runtime",
        "unsupportedPluginForm",
        "插件不是后端形态，不加载。",
        serde_json::json!({
            "pluginId": manifest.plugin_id,
            "sdk": manifest.sdk,
            "runtime": manifest.runtime,
        })
    );
    Err(format!(
        "unsupported plugin form: {} (sdk {}, runtime {}); plugins must be backend native-dylib, process or manifest-only",
        manifest.plugin_id, manifest.sdk, manifest.runtime
    ))
}

pub(in super::super) fn plugin_legacy_ids(manifest: &PluginManifest) -> Vec<String> {
    let mut values = manifest.legacy_plugin_ids.clone();
    values.extend(manifest.compat.legacy_plugin_ids.clone());
    values.sort();
    values.dedup();
    values
}

pub(in super::super) fn resolve_plugin_manifest_dependencies(manifests: &mut [PluginManifest]) {
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
            manifest.disable_reason =
                Some(format!("缺少必需依赖：{}。", missing_required.join("、")));
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

pub(super) fn resolve_plugin_dependency_list(
    dependency_ids: &[String],
    by_id: &BTreeMap<String, PluginManifest>,
    legacy_ids: &BTreeMap<String, String>,
) -> Vec<PluginDependencyState> {
    dependency_ids
        .iter()
        .map(|dependency_id| resolve_plugin_dependency(dependency_id, by_id, legacy_ids))
        .collect()
}

pub(super) fn resolve_plugin_dependency(
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

pub(super) fn plugin_dependency_available(
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
    manifest
        .requires
        .iter()
        .all(|dependency_id| plugin_dependency_available(dependency_id, by_id, legacy_ids, visited))
}

pub(super) fn dependency_ids_by_status(
    dependencies: &[PluginDependencyState],
    status: &str,
) -> Vec<String> {
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

pub(super) fn unavailable_dependency_ids(dependencies: &[PluginDependencyState]) -> Vec<String> {
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

pub(in super::super) fn normalized_builtin_plugin_id(plugin_id: &str) -> &str {
    match plugin_id.trim() {
        LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID => LOCAL_FILESYSTEM_PLUGIN_ID,
        other => other,
    }
}

pub(in super::super) fn plugin_category_for_kind(kind: &str) -> &'static str {
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

pub(in super::super) fn is_source_plugin(manifest: &PluginManifest) -> bool {
    manifest.category == "source"
        || matches!(manifest.kind.as_str(), "filesystem" | "webdav" | "cloud")
}

pub(in super::super) fn ensure_repository_backend_runtime_available(
    registration: &PluginCatalogEntry,
) -> Result<(), String> {
    let manifest = &registration.manifest;
    if !manifest.enabled || manifest.status == "disabled" {
        return Err(format!("plugin is disabled: {}", manifest.plugin_id));
    }
    #[cfg(test)]
    if manifest.plugin_id == LOCAL_FILESYSTEM_PLUGIN_ID
        && manifest.runtime == "manifest-only"
        && local_filesystem_adapter_enabled()
    {
        // 后端单测使用协议等价的 cfg(test) 本地适配器；生产仍要求独立 ABI Host。
        return Ok(());
    }
    if manifest.sdk != "backend" || manifest.runtime != "native-dylib" {
        return Err(format!(
            "plugin runtime is not available: {}",
            manifest.plugin_id
        ));
    }
    if let Some(error) = mutsuki_host::plugin_unavailable_reason(&manifest.plugin_id) {
        return Err(format!(
            "plugin runtime is not available: {} ({error})",
            manifest.plugin_id
        ));
    }
    Ok(())
}
