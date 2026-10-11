//! Runtime API design snapshot builders.

use super::*;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginApiTestContribution {
    pub(super) method: String,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) payload: Option<serde_json::Value>,
    #[serde(default)]
    pub(super) request_template: Option<serde_json::Value>,
}

pub(super) fn default_api_definitions(service_root: &Path) -> Vec<ApiDefinition> {
    let mut definitions = Vec::new();
    definitions.extend(external_api_definitions());
    definitions.extend(plugin_api_definitions(service_root));
    definitions
}

pub(super) fn external_api_definitions() -> Vec<ApiDefinition> {
    vec![
        external_api_definition(
            "GET",
            "/external/v1/health",
            "检查外部 API 服务状态。",
            false,
            None,
        ),
        external_api_definition(
            "GET",
            "/external/v1/repositories",
            "列出可接收外部素材的本地仓库。",
            true,
            None,
        ),
        external_api_definition(
            "POST",
            "/external/v1/assets:add",
            "从远程 URL 添加素材到仓库。",
            true,
            Some(serde_json::json!({
                "repoId": "",
                "parentPath": "",
                "client": {
                    "id": "momobako.api-playground",
                    "name": "API Playground",
                    "version": "0.1.0"
                },
                "items": [
                    {
                        "kind": "remoteUrl",
                        "url": "https://example.com/image.png",
                        "filename": "image.png",
                        "metadata": {
                            "sourceUrl": "https://example.com/image.png"
                        }
                    }
                ]
            })),
        ),
    ]
}

pub(super) fn plugin_api_definitions(service_root: &Path) -> Vec<ApiDefinition> {
    let registry = plugin_catalog(service_root);
    let mut definitions = Vec::new();
    let mut seen = HashSet::<(String, String)>::new();

    for manifest in registry.list_manifests() {
        if !plugin_manifest_can_be_called(&manifest) {
            continue;
        }
        let Some(contributes) = manifest.contributes.as_object() else {
            continue;
        };

        if let Some(raw_tests) = contributes.get("apiTests") {
            if let Ok(tests) =
                serde_json::from_value::<Vec<PluginApiTestContribution>>(raw_tests.clone())
            {
                for test in tests {
                    if test.method.trim().is_empty() {
                        continue;
                    }
                    let method = test.method.trim().to_string();
                    if !seen.insert((manifest.plugin_id.clone(), method.clone())) {
                        continue;
                    }
                    let summary = test
                        .summary
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| format!("调用插件 API {method}。"));
                    let payload = test
                        .payload
                        .or(test.request_template)
                        .unwrap_or_else(|| serde_json::json!({}));
                    definitions.push(plugin_api_definition(
                        &manifest.plugin_id,
                        &manifest.name,
                        &method,
                        &summary,
                        payload,
                    ));
                }
            }
        }

        if let Some(action) = contributes
            .get("provider")
            .and_then(|provider| provider.get("lookup"))
            .and_then(|lookup| lookup.get("action"))
            .and_then(|action| action.as_str())
            .map(str::trim)
            .filter(|action| !action.is_empty())
        {
            if seen.insert((manifest.plugin_id.clone(), action.to_string())) {
                definitions.push(plugin_api_definition(
                    &manifest.plugin_id,
                    &manifest.name,
                    action,
                    &format!("查询 {} 元数据候选。", manifest.name),
                    serde_json::json!({ "id": "<externalId>" }),
                ));
            }
        }

        if let Some(action) = contributes
            .get("metadataDefaults")
            .and_then(|defaults| defaults.get("action"))
            .and_then(|action| action.as_str())
            .map(str::trim)
            .filter(|action| !action.is_empty())
        {
            if seen.insert((manifest.plugin_id.clone(), action.to_string())) {
                definitions.push(plugin_api_definition(
                    &manifest.plugin_id,
                    &manifest.name,
                    action,
                    &format!("生成 {} 元数据默认值。", manifest.name),
                    serde_json::json!({
                        "entries": [
                            {
                                "path": "work/track01.mp3",
                                "name": "track01.mp3",
                                "extension": "mp3",
                                "kind": "file"
                            }
                        ]
                    }),
                ));
            }
        }
    }

    definitions.sort_by(|left, right| {
        left.group
            .cmp(&right.group)
            .then_with(|| left.path.cmp(&right.path))
    });
    definitions
}

pub(super) fn plugin_manifest_can_be_called(manifest: &PluginManifest) -> bool {
    manifest.enabled
        && manifest.sdk == "backend"
        && !matches!(
            manifest.status.as_str(),
            "disabled" | "error" | "unavailable"
        )
}

pub(super) fn external_api_definition(
    method: &str,
    path: &str,
    summary: &str,
    requires_auth: bool,
    request_template: Option<serde_json::Value>,
) -> ApiDefinition {
    ApiDefinition {
        group: "External Asset API".to_string(),
        transport: "external-http".to_string(),
        method: method.to_string(),
        path: path.to_string(),
        summary: summary.to_string(),
        plugin_id: None,
        plugin_method: None,
        requires_auth: Some(requires_auth),
        request_template,
    }
}

pub(super) fn plugin_api_definition(
    plugin_id: &str,
    plugin_name: &str,
    method: &str,
    summary: &str,
    request_template: serde_json::Value,
) -> ApiDefinition {
    ApiDefinition {
        group: format!("Plugin API / {plugin_name}"),
        transport: "plugin-call".to_string(),
        method: "PLUGIN".to_string(),
        path: format!("{plugin_id}:{method}"),
        summary: summary.to_string(),
        plugin_id: Some(plugin_id.to_string()),
        plugin_method: Some(method.to_string()),
        requires_auth: None,
        request_template: Some(request_template),
    }
}
