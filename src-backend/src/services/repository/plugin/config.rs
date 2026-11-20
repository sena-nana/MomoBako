//! 插件配置值的读写校验与插件启用设置。

use super::*;

pub(super) fn plugin_config_path(plugin_data_dir: &Path) -> PathBuf {
    plugin_data_dir.join("config.json")
}

pub(in super::super) fn load_plugin_config_values(
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

pub(in super::super) fn save_plugin_config_values(
    plugin_data_dir: &Path,
    values: &BTreeMap<String, serde_json::Value>,
) -> Result<(), String> {
    fs::create_dir_all(plugin_data_dir).map_err(io_error)?;
    let raw = serde_json::to_string_pretty(values).map_err(json_error)?;
    fs::write(plugin_config_path(plugin_data_dir), raw).map_err(io_error)
}

pub(super) fn plugin_settings_schema(manifest: &PluginManifest) -> serde_json::Value {
    manifest
        .contributes
        .as_object()
        .and_then(|contributes| contributes.get("settings"))
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}))
}

pub(super) fn plugin_config_snapshot(
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

pub(super) fn normalize_plugin_config_key(key: &str) -> Result<String, String> {
    let key = key.trim();
    if key.is_empty() {
        Err("plugin config key cannot be empty".to_string())
    } else {
        Ok(key.to_string())
    }
}

pub(super) fn validate_plugin_config_value(
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
        "string" if !value.is_string() => Err(format!("plugin config value must be string: {key}")),
        "select" => validate_plugin_select_config_value(field, key, value),
        _ => Ok(()),
    }
}

pub(super) fn validate_plugin_select_config_value(
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

pub(in super::super) fn load_plugin_settings(
    service_root: &Path,
) -> Result<PluginSettings, String> {
    let path = plugin_settings_path(service_root);
    if !path.is_file() {
        return Ok(PluginSettings::default());
    }
    let raw = fs::read_to_string(path).map_err(io_error)?;
    let mut settings = serde_json::from_str::<PluginSettings>(&raw).map_err(json_error)?;
    if let Some(legacy) = settings
        .plugins
        .remove(LEGACY_NETEASE_CLOUD_MUSIC_PLUGIN_ID)
    {
        settings
            .plugins
            .entry(NETEASE_CLOUD_MUSIC_PLUGIN_ID.to_string())
            .or_insert(legacy);
        save_plugin_settings(service_root, &settings)?;
    }
    Ok(settings)
}

pub(in super::super) fn save_plugin_settings(
    service_root: &Path,
    settings: &PluginSettings,
) -> Result<(), String> {
    fs::create_dir_all(service_root).map_err(io_error)?;
    let raw = serde_json::to_string_pretty(settings).map_err(json_error)?;
    fs::write(plugin_settings_path(service_root), raw).map_err(io_error)
}

pub(in super::super) fn apply_plugin_settings(
    manifest: &mut PluginManifest,
    settings: &PluginSettings,
) {
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
