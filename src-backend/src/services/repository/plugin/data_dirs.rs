//! 插件设置与数据目录、钩子执行记录，以及辅助进程状态文件的清理。

use super::*;

pub(super) fn plugin_settings_path(service_root: &Path) -> PathBuf {
    service_root.join("plugin-state.json")
}

pub(in super::super) fn broken_plugin_manifest(archive_path: &Path, error: &str) -> PluginManifest {
    let file_stem = archive_path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("broken-plugin");
    PluginManifest {
        plugin_id: format!("broken.{}", slugify_ascii_component(file_stem)),
        package_format_version: None,
        package_hash: None,
        provenance: Some("legacy".to_string()),
        trust_level: Some("legacy".to_string()),
        deployment: Some("manifest".to_string()),
        target_triple: None,
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

pub(crate) fn plugin_data_root_dir(service_root: &Path) -> PathBuf {
    service_root.join("plugin-data")
}

pub(crate) fn plugin_data_dir(service_root: &Path, plugin_id: &str) -> PathBuf {
    plugin_data_root_dir(service_root).join(plugin_data_dir_name(plugin_id))
}

/// 将旧 Source 的运行目录迁至新 ID；旧 Library 数据目录不会被删除或改名。
pub(in super::super) fn migrate_builtin_plugin_data_dirs(
    service_root: &Path,
) -> Result<(), String> {
    let legacy = plugin_data_dir(service_root, LEGACY_NETEASE_CLOUD_MUSIC_PLUGIN_ID);
    if !legacy.is_dir() {
        return Ok(());
    }
    let current = plugin_data_dir(service_root, NETEASE_CLOUD_MUSIC_PLUGIN_ID);
    if !current.exists() {
        fs::rename(&legacy, &current).map_err(io_error)?;
        return Ok(());
    }
    copy_missing_plugin_data(&legacy, &current)
}

/// 合并旧目录中尚未迁移的运行文件；新 ID 下已有文件始终优先。
pub(super) fn copy_missing_plugin_data(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(io_error)?;
    for entry in fs::read_dir(source).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let file_type = entry.file_type().map_err(io_error)?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_missing_plugin_data(&entry.path(), &target)?;
        } else if file_type.is_file() && !target.exists() {
            fs::copy(entry.path(), target).map_err(io_error)?;
        }
    }
    Ok(())
}

pub(super) fn plugin_data_dir_name(plugin_id: &str) -> String {
    let slug = slugify_ascii_component(plugin_id);
    if slug.is_empty() {
        format!("plugin-{}", hash_text_sha256(plugin_id))
    } else {
        slug
    }
}

pub(super) fn plugin_hook_executions_path(service_root: &Path) -> PathBuf {
    service_root.join(PLUGIN_HOOK_EXECUTIONS_FILE_NAME)
}

pub(super) fn plugin_hook_execution_context(
    service_root: &Path,
    plugin_id: &str,
    method: &str,
) -> Option<(String, PluginHook)> {
    let registry = plugin_catalog(service_root);
    let normalized_plugin_id = registry.normalize_plugin_id(plugin_id);
    let manifest = registry.resolved_manifest(&normalized_plugin_id)?;
    manifest
        .hooks
        .iter()
        .find(|hook| hook.action == method)
        .cloned()
        .map(|hook| (manifest.plugin_id, hook))
}

pub(super) fn plugin_hook_execution_target(payload: &serde_json::Value) -> serde_json::Value {
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

pub(super) fn plugin_hook_execution_id(
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

pub(super) fn is_plugin_call_blocked_error(error: &str) -> bool {
    error.contains("plugin call blocked by dependency status")
        || error.contains("plugin is disabled")
        || error.contains("插件不可用")
}

pub(super) fn append_plugin_hook_execution_record(
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

pub(super) fn load_plugin_hook_execution_records(
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

pub(in super::super) fn ensure_plugin_data_dir(
    service_root: &Path,
    plugin_id: &str,
) -> Result<PathBuf, String> {
    let data_dir = plugin_data_dir(service_root, plugin_id);
    fs::create_dir_all(&data_dir).map_err(io_error)?;
    Ok(data_dir)
}

pub(in super::super) fn shutdown_helper_state_dir(plugin_data_dir: &Path) -> Result<(), String> {
    let helper_root = plugin_data_dir.join("helpers");
    if !helper_root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&helper_root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let state_dir = entry.path();
        if !state_dir.is_dir() {
            continue;
        }
        let pid_path = state_dir.join("pid.txt");
        if !pid_path.is_file() {
            continue;
        }
        let pid_raw = fs::read_to_string(&pid_path).map_err(io_error)?;
        let Some(pid) = pid_raw.trim().parse::<u32>().ok() else {
            crate::app_log!(
                "warn",
                "runtime.helper",
                "invalidPid",
                "运行时辅助进程 PID 无效，开始清理状态文件。",
                serde_json::json!({
                    "stateDir": state_dir.to_string_lossy().to_string(),
                    "pid": pid_raw.trim(),
                })
            );
            remove_helper_file_if_exists(&pid_path);
            remove_helper_file_if_exists(&state_dir.join("status.json"));
            remove_helper_file_if_exists(&state_dir.join("port.txt"));
            remove_helper_file_if_exists(&state_dir.join("session.txt"));
            remove_helper_file_if_exists(&state_dir.join("office-convert-helper.ps1"));
            continue;
        };
        #[cfg(target_os = "windows")]
        {
            log_helper_shutdown_status(
                "taskkill",
                pid,
                &state_dir,
                child_process::command("taskkill")
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .status(),
            );
        }
        #[cfg(not(target_os = "windows"))]
        {
            log_helper_shutdown_status(
                "kill",
                pid,
                &state_dir,
                child_process::command("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status(),
            );
        }
        remove_helper_file_if_exists(&pid_path);
        remove_helper_file_if_exists(&state_dir.join("status.json"));
        remove_helper_file_if_exists(&state_dir.join("port.txt"));
        remove_helper_file_if_exists(&state_dir.join("session.txt"));
        remove_helper_file_if_exists(&state_dir.join("office-convert-helper.ps1"));
    }
    Ok(())
}

pub(super) fn log_helper_shutdown_status(
    command: &str,
    pid: u32,
    state_dir: &Path,
    result: std::io::Result<std::process::ExitStatus>,
) {
    match result {
        Ok(status) if status.success() => {
            crate::app_log!(
                "info",
                "runtime.helper",
                "shutdownSuccess",
                "运行时辅助进程已停止。",
                serde_json::json!({
                    "command": command,
                    "pid": pid,
                    "stateDir": state_dir.to_string_lossy().to_string(),
                })
            );
        }
        Ok(status) => {
            crate::app_log!(
                "warn",
                "runtime.helper",
                "shutdownStatusFailed",
                "运行时辅助进程停止命令返回失败状态。",
                serde_json::json!({
                    "command": command,
                    "pid": pid,
                    "status": status.to_string(),
                    "stateDir": state_dir.to_string_lossy().to_string(),
                })
            );
        }
        Err(error) => {
            crate::app_log!(
                "warn",
                "runtime.helper",
                "shutdownCommandFailed",
                "运行时辅助进程停止命令执行失败。",
                serde_json::json!({
                    "command": command,
                    "pid": pid,
                    "error": error.to_string(),
                    "stateDir": state_dir.to_string_lossy().to_string(),
                })
            );
        }
    }
}

pub(super) fn remove_helper_file_if_exists(path: &Path) {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            crate::app_log!(
                "warn",
                "runtime.helper",
                "stateFileCleanupFailed",
                "运行时辅助进程状态文件清理失败。",
                serde_json::json!({
                    "path": path.to_string_lossy().to_string(),
                    "error": error.to_string(),
                })
            );
        }
    }
}
