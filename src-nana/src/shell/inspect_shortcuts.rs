//! 库类型搜索快捷方式。
//!
//! 清单里的对象直接登记。ASMR 官方插件的字符串 id 映射成筛选和排序。
//! 没有筛选内容的字符串不进筛选栏。

use crate::backend::services::repository::PluginManifest;

use super::inspect::SortDirection;

/// 筛选栏上的一个库类型快捷方式。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchShortcut {
    pub id: String,
    pub label: String,
    pub metadata: String,
    pub sort_field: String,
    pub sort_direction: SortDirection,
}

/// 从已启用插件的 `libraryExtension.searchShortcuts` 收集快捷方式。
pub fn shortcuts_from_plugins(plugins: &[PluginManifest]) -> Vec<SearchShortcut> {
    let mut shortcuts = Vec::new();
    for plugin in plugins {
        if !plugin.enabled || matches!(plugin.status.as_str(), "disabled" | "unavailable" | "error") {
            continue;
        }
        let Some(extension) = plugin.contributes.get("libraryExtension") else {
            continue;
        };
        let kind = extension.get("libraryKind").and_then(|value| value.as_str()).unwrap_or("");
        let Some(list) = extension.get("searchShortcuts").and_then(|value| value.as_array()) else {
            continue;
        };
        for item in list {
            match parse_shortcut(kind, item) {
                Some(shortcut) => shortcuts.push(shortcut),
                None => eprintln!("Nana 库类型快捷方式缺少筛选：{} / {kind}", plugin.plugin_id),
            }
        }
    }
    shortcuts
}

fn parse_shortcut(kind: &str, item: &serde_json::Value) -> Option<SearchShortcut> {
    if let Some(id) = item.as_str() {
        return official_asmr(kind, id.trim());
    }
    let id = text(item, "id")?;
    let label = text(item, "label").unwrap_or_else(|| id.clone());
    let metadata = text(item, "metadataFilters").or_else(|| text(item, "metadata"))?;
    let sort = item.get("sort");
    let sort_field = sort.and_then(|value| text(value, "field")).unwrap_or_default();
    let sort_direction = if sort.and_then(|value| value.get("direction")).and_then(|value| value.as_str()) == Some("desc") {
        SortDirection::Desc
    } else {
        SortDirection::Asc
    };
    Some(SearchShortcut { id, label, metadata, sort_field, sort_direction })
}

/// ASMR 官方清单只写了 id。筛选文案和 `register.js` 里的四条快捷方式一致。
fn official_asmr(kind: &str, id: &str) -> Option<SearchShortcut> {
    if kind != "asmr" {
        return None;
    }
    let (label, metadata, sort_field, desc) = match id {
        "works" => ("ASMR 作品", "libraryKind=asmr", "metadata.workId", false),
        "lyrics" => ("含歌词", "libraryKind=asmr\nlyricStatus=local", "", false),
        "continue" => ("继续收听", "libraryKind=asmr\nlisteningStatus=listening", "metadata.lastListenedAt", true),
        "random" => ("随机一首", "libraryKind=asmr", "random", false),
        _ => return None,
    };
    Some(SearchShortcut {
        id: id.to_string(),
        label: label.to_string(),
        metadata: metadata.to_string(),
        sort_field: sort_field.to_string(),
        sort_direction: if desc { SortDirection::Desc } else { SortDirection::Asc },
    })
}

fn text(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|item| item.as_str()).map(str::trim).filter(|item| !item.is_empty()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(shortcuts: serde_json::Value) -> PluginManifest {
        PluginManifest {
            plugin_id: "momobako.library.asmr".into(),
            package_format_version: None,
            package_hash: None,
            provenance: None,
            trust_level: None,
            deployment: None,
            target_triple: None,
            legacy_plugin_ids: Vec::new(),
            name: "ASMR".into(),
            version: "0.2.0".into(),
            r#type: None,
            kind: "asmr".into(),
            category: "library-kind".into(),
            description: String::new(),
            capabilities: Vec::new(),
            enabled: true,
            sdk: "frontend".into(),
            entry: serde_json::Value::Null,
            contributes: serde_json::json!({
                "libraryExtension": { "libraryKind": "asmr", "searchShortcuts": shortcuts }
            }),
            source: "builtin".into(),
            runtime: "vue-module".into(),
            permissions: Vec::new(),
            requires: Vec::new(),
            optional: Vec::new(),
            hooks: Vec::new(),
            compat: Default::default(),
            status: "ready".into(),
            dependency_status: Default::default(),
            disable_reason: None,
            degraded: false,
            degradation_reason: None,
            archive_path: None,
        }
    }

    #[test]
    fn asmr_string_ids_become_filter_shortcuts() {
        let shortcuts = shortcuts_from_plugins(&[plugin(serde_json::json!(["works", "lyrics", "continue", "random", "missing"]))]);
        assert_eq!(shortcuts.len(), 4);
        assert_eq!(shortcuts[0].label, "ASMR 作品");
        assert_eq!(shortcuts[0].metadata, "libraryKind=asmr");
        assert_eq!(shortcuts[0].sort_field, "metadata.workId");
        assert_eq!(shortcuts[2].sort_direction, SortDirection::Desc);
        assert_eq!(shortcuts[3].sort_field, "random");
    }

    #[test]
    fn object_shortcuts_keep_their_filters() {
        let shortcuts = shortcuts_from_plugins(&[plugin(serde_json::json!([{
            "id": "circle",
            "label": "社团",
            "metadataFilters": "libraryKind=asmr\ncircle=test",
            "sort": { "field": "metadata.circle", "direction": "desc" }
        }]))]);
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].metadata, "libraryKind=asmr\ncircle=test");
        assert_eq!(shortcuts[0].sort_direction, SortDirection::Desc);
    }
}
