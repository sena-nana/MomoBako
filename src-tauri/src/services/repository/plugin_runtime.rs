//! Backend plugin registry, native runtime bridge, and plugin discovery.

use super::*;

use crate::services::logging::write_log;

pub(super) type PluginManifestFn = unsafe extern "C" fn() -> *mut c_char;
pub(super) type PluginCallFn = unsafe extern "C" fn(*const c_char) -> *mut c_char;
pub(super) type PluginFreeFn = unsafe extern "C" fn(*mut c_char);
pub(super) type PluginRegisterHostApiFn =
    unsafe extern "C" fn(Option<HostPluginCallFn>, Option<HostPluginFreeFn>);
pub(super) type HostPluginCallFn = unsafe extern "C" fn(*const c_char) -> *mut c_char;
pub(super) type HostPluginFreeFn = unsafe extern "C" fn(*mut c_char);

pub(super) struct NativePlugin {
    pub(super) _library: libloading::Library,
    pub(super) runtime_dir: PathBuf,
    pub(super) call: PluginCallFn,
    pub(super) free: PluginFreeFn,
}

#[derive(Debug)]
pub(super) struct DiscoveredPluginManifest {
    pub(super) manifest: PluginManifest,
    pub(super) archive_path: PathBuf,
    pub(super) manifest_prefix: String,
}

pub(super) struct BackendPluginRegistration {
    pub(super) manifest: PluginManifest,
    pub(super) archive_path: PathBuf,
    pub(super) manifest_prefix: String,
    pub(super) native: Option<NativePlugin>,
    pub(super) load_error: Option<String>,
}

pub(super) struct BackendPluginRegistry {
    pub(super) service_root: PathBuf,
    pub(super) registrations: BTreeMap<String, BackendPluginRegistration>,
    pub(super) legacy_ids: BTreeMap<String, String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginSettings {
    pub(super) plugins: BTreeMap<String, PluginSettingsEntry>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PluginSettingsEntry {
    pub(super) enabled: Option<bool>,
}

impl BackendPluginRegistry {
    pub(super) fn load(service_root: &Path) -> Self {
        Self::load_with_options(service_root, true)
    }

    pub(super) fn load_for_management(service_root: &Path) -> Self {
        Self::load_with_options(service_root, false)
    }

    pub(super) fn load_with_options(service_root: &Path, load_native: bool) -> Self {
        let settings = load_plugin_settings(service_root).unwrap_or_default();
        let manifests = load_runtime_plugin_manifests(service_root);
        let mut registrations = BTreeMap::new();
        let mut legacy_ids = BTreeMap::new();
        for discovered in manifests {
            let mut manifest = discovered.manifest;
            apply_plugin_settings(&mut manifest, &settings);
            for legacy_id in plugin_legacy_ids(&manifest) {
                legacy_ids.insert(legacy_id, manifest.plugin_id.clone());
            }
            let (native, load_error) =
                if load_native && manifest.enabled && manifest.runtime == "native-dylib" {
                    match load_native_plugin(
                        &manifest,
                        &discovered.archive_path,
                        &discovered.manifest_prefix,
                        service_root,
                    ) {
                        Ok(native) => (Some(native), None),
                        Err(error) => (None, Some(error)),
                    }
                } else {
                    (None, None)
                };
            registrations.insert(
                manifest.plugin_id.clone(),
                BackendPluginRegistration {
                    manifest,
                    archive_path: discovered.archive_path,
                    manifest_prefix: discovered.manifest_prefix,
                    native,
                    load_error,
                },
            );
        }

        Self {
            service_root: service_root.to_path_buf(),
            registrations,
            legacy_ids,
        }
    }

    pub(super) fn list_manifests(&self) -> Vec<PluginManifest> {
        let mut manifests = self
            .registrations
            .values()
            .map(|registration| {
                let mut manifest = registration.manifest.clone();
                if manifest.runtime == "native-dylib"
                    && manifest.enabled
                    && registration.native.is_none()
                {
                    manifest.status = "unavailable".to_string();
                    manifest.disable_reason = Some("原生运行时不可用。".to_string());
                }
                manifest
            })
            .collect::<Vec<_>>();
        resolve_plugin_manifest_dependencies(&mut manifests);
        manifests
    }

    pub(super) fn resolved_manifests_by_id(&self) -> BTreeMap<String, PluginManifest> {
        self.list_manifests()
            .into_iter()
            .map(|manifest| (manifest.plugin_id.clone(), manifest))
            .collect()
    }

    pub(super) fn resolved_manifest(&self, plugin_id: &str) -> Option<PluginManifest> {
        let normalized = self.normalize_plugin_id(plugin_id);
        self.resolved_manifests_by_id().remove(normalized.as_str())
    }

    pub(super) fn manifest(&self, plugin_id: &str) -> Option<&PluginManifest> {
        let normalized = self.normalize_plugin_id(plugin_id);
        self.registrations
            .get(normalized.as_str())
            .map(|registration| &registration.manifest)
    }

    pub(super) fn registration(&self, plugin_id: &str) -> Option<&BackendPluginRegistration> {
        let normalized = self.normalize_plugin_id(plugin_id);
        self.registrations.get(normalized.as_str())
    }

    pub(super) fn normalize_plugin_id(&self, plugin_id: &str) -> String {
        let trimmed = plugin_id.trim();
        self.legacy_ids
            .get(trimmed)
            .cloned()
            .unwrap_or_else(|| trimmed.to_string())
    }

    pub(super) fn call(
        &self,
        plugin_id: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.call_with_runtime(plugin_id, method, payload)
            .map(|result| result.payload)
    }

    pub(super) fn call_with_runtime(
        &self,
        plugin_id: &str,
        method: &str,
        payload: serde_json::Value,
    ) -> Result<PluginRuntimeCallResult, String> {
        let normalized = self.normalize_plugin_id(plugin_id);
        let registration = self
            .registrations
            .get(normalized.as_str())
            .ok_or_else(|| format!("unsupported plugin: {plugin_id}"))?;
        let resolved_manifest = self
            .resolved_manifest(&normalized)
            .unwrap_or_else(|| registration.manifest.clone());
        if !resolved_manifest.enabled
            || matches!(
                resolved_manifest.status.as_str(),
                "disabled" | "unavailable" | "error"
            )
        {
            let reason = resolved_manifest
                .disable_reason
                .as_deref()
                .unwrap_or("插件不可用。");
            return Err(format!(
                "plugin call blocked by dependency status: {} {method} ({reason})",
                resolved_manifest.plugin_id
            ));
        }
        let runtime = plugin_call_runtime(&resolved_manifest);
        let plugin_data_dir =
            ensure_plugin_data_dir(&self.service_root, &resolved_manifest.plugin_id)?;
        let plugin_config = load_plugin_config_values(&plugin_data_dir)?;
        let plugin_runtime_dir = registration
            .native
            .as_ref()
            .map(|native| native.runtime_dir.to_string_lossy().to_string())
            .unwrap_or_default();
        let runtime_context = PluginCallHostRuntime {
            plugin_id: resolved_manifest.plugin_id.clone(),
            plugin_data_dir: plugin_data_dir.to_string_lossy().to_string(),
            service_root_dir: self.service_root.to_string_lossy().to_string(),
            plugin_runtime_dir,
            plugin_config,
        };
        let response = if let Some(native) = &registration.native {
            native.call(method, payload, runtime_context)?
        } else if let Some(error) = &registration.load_error {
            return Err(format!(
                "plugin runtime is not available: {} ({error})",
                registration.manifest.plugin_id
            ));
        } else {
            return Err(format!(
                "plugin runtime is not available: {}",
                registration.manifest.plugin_id
            ));
        };
        Ok(PluginRuntimeCallResult {
            plugin_id: resolved_manifest.plugin_id,
            payload: response,
            runtime,
        })
    }

    pub(super) fn playlist_players(&self) -> Vec<PlaylistPlayerRegistration> {
        let mut players = Vec::new();
        for registration in self.registrations.values() {
            if !registration.manifest.enabled || registration.manifest.status == "error" {
                continue;
            }
            let Some(contributes) = registration.manifest.contributes.as_object() else {
                continue;
            };
            let Some(raw_players) = contributes.get("playlistPlayers") else {
                continue;
            };
            let Ok(parsed) =
                serde_json::from_value::<Vec<PlaylistPlayerContribution>>(raw_players.clone())
            else {
                continue;
            };
            for player in parsed {
                players.push(PlaylistPlayerRegistration {
                    plugin_id: registration.manifest.plugin_id.clone(),
                    player_type_id: player.player_type_id,
                    label: player.label,
                    file_class: player.file_class,
                    supported_extensions: player
                        .supported_extensions
                        .into_iter()
                        .map(|value| value.trim().to_ascii_lowercase())
                        .filter(|value| !value.is_empty())
                        .collect(),
                    supports_seek: player.supports_seek,
                    supports_volume: player.supports_volume,
                    supports_preview_navigation: player.supports_preview_navigation,
                    description: player.description,
                });
            }
        }
        players.sort_by(|left, right| {
            left.label
                .to_lowercase()
                .cmp(&right.label.to_lowercase())
                .then_with(|| left.player_type_id.cmp(&right.player_type_id))
        });
        players
    }

    pub(super) fn playlist_player(
        &self,
        player_type_id: &str,
    ) -> Option<PlaylistPlayerRegistration> {
        let normalized = player_type_id.trim();
        self.playlist_players()
            .into_iter()
            .find(|player| player.player_type_id == normalized)
    }

    pub(super) fn metadata_default_providers(&self) -> Vec<(String, String)> {
        let mut providers = Vec::new();
        let resolved_manifests = self.resolved_manifests_by_id();
        for registration in self.registrations.values() {
            let Some(manifest) = resolved_manifests.get(&registration.manifest.plugin_id) else {
                continue;
            };
            if !manifest.enabled
                || matches!(
                    manifest.status.as_str(),
                    "disabled" | "unavailable" | "error"
                )
            {
                continue;
            }
            let Some(contributes) = manifest.contributes.as_object() else {
                continue;
            };
            let Some(defaults) = contributes
                .get("metadataDefaults")
                .and_then(|value| value.as_object())
            else {
                continue;
            };
            let Some(action) = defaults
                .get("action")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            providers.push((manifest.plugin_id.clone(), action.to_string()));
        }
        providers.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        providers
    }
}

impl NativePlugin {
    pub(super) fn call(
        &self,
        method: &str,
        payload: serde_json::Value,
        runtime: PluginCallHostRuntime,
    ) -> Result<serde_json::Value, String> {
        let request = PluginCallEnvelope {
            method: method.to_string(),
            payload,
            runtime,
        };
        let request_json = serde_json::to_string(&request).map_err(json_error)?;
        let request_cstring = CString::new(request_json)
            .map_err(|_| "plugin request contains an invalid null byte".to_string())?;
        let response_ptr = unsafe { (self.call)(request_cstring.as_ptr()) };
        if response_ptr.is_null() {
            return Err("plugin returned a null response".to_string());
        }
        let response_json = unsafe { CStr::from_ptr(response_ptr) }
            .to_string_lossy()
            .to_string();
        unsafe { (self.free)(response_ptr) };
        let response: PluginCallResponse =
            serde_json::from_str(&response_json).map_err(json_error)?;
        if response.ok {
            Ok(response.payload.unwrap_or_else(|| serde_json::json!({})))
        } else {
            Err(response
                .error
                .unwrap_or_else(|| "plugin call failed without an error message".to_string()))
        }
    }
}

pub(super) unsafe extern "C" fn host_plugin_call_bridge(input: *const c_char) -> *mut c_char {
    let response = match read_host_plugin_call(input).and_then(dispatch_host_plugin_call) {
        Ok(payload) => PluginCallResponse {
            ok: true,
            payload: Some(payload),
            error: None,
        },
        Err(error) => PluginCallResponse {
            ok: false,
            payload: None,
            error: Some(error),
        },
    };
    let json = serde_json::to_string(&response).unwrap_or_else(|error| {
        format!(
            "{{\"ok\":false,\"payload\":null,\"error\":\"failed to encode host plugin response: {error}\"}}"
        )
    });
    CString::new(json)
        .unwrap_or_else(|_| {
            CString::new(
                "{\"ok\":false,\"payload\":null,\"error\":\"host plugin response contained a null byte\"}",
            )
            .expect("static CString should be valid")
        })
        .into_raw()
}

pub(super) unsafe extern "C" fn host_plugin_free_bridge(value: *mut c_char) {
    if value.is_null() {
        return;
    }
    let _ = CString::from_raw(value);
}

fn read_host_plugin_call(input: *const c_char) -> Result<HostPluginCallEnvelope, String> {
    if input.is_null() {
        return Err("host plugin request pointer is null".to_string());
    }
    let raw = unsafe { CStr::from_ptr(input) }
        .to_str()
        .map_err(|error| error.to_string())?;
    serde_json::from_str(raw).map_err(json_error)
}

fn dispatch_host_plugin_call(request: HostPluginCallEnvelope) -> Result<serde_json::Value, String> {
    if request.plugin_id.trim() == "momobako.system" {
        return dispatch_internal_host_system_call(&request.method, request.payload);
    }
    backend_plugin_registry(Path::new(request.service_root_dir.trim())).call(
        &request.plugin_id,
        &request.method,
        request.payload,
    )
}

fn dispatch_internal_host_system_call(
    method: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    match method.trim() {
        "system.log.write" => {
            let request = serde_json::from_value::<SystemLogWriteRequest>(payload).map_err(json_error)?;
            let record = write_log(request)?;
            serde_json::to_value(record).map_err(json_error)
        }
        _ => Err(format!("unsupported internal host method: {method}")),
    }
}

pub(super) fn plugin_call_runtime(manifest: &PluginManifest) -> Option<PluginCallRuntime> {
    if !manifest.degraded {
        return None;
    }
    Some(PluginCallRuntime {
        degraded: true,
        degradation_reason: manifest.degradation_reason.clone(),
        dependency_status: manifest.dependency_status.clone(),
    })
}

pub(super) fn backend_plugin_registry(service_root: &Path) -> BackendPluginRegistry {
    BackendPluginRegistry::load(service_root)
}

pub(super) fn plugin_management_registry(service_root: &Path) -> BackendPluginRegistry {
    BackendPluginRegistry::load_for_management(service_root)
}

pub(super) fn call_downloader_prepare_track_playback(
    service_root: &Path,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[cfg(test)]
    if let Some(hook) = test_support::downloader_playback_hook()? {
        return hook(payload);
    }

    backend_plugin_registry(service_root).call(
        "momobako.service.downloader",
        "downloader.prepareTrackPlayback",
        payload,
    )
}

pub(crate) fn call_downloader_download_track_package(
    service_root: &Path,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    #[cfg(test)]
    if let Some(hook) = test_support::downloader_track_package_hook()? {
        return hook(payload);
    }

    backend_plugin_registry(service_root).call(
        "momobako.service.downloader",
        "downloader.downloadTrackPackage",
        payload,
    )
}

#[cfg(test)]
pub(crate) fn set_test_downloader_playback_hook(
    hook: Option<fn(serde_json::Value) -> Result<serde_json::Value, String>>,
) {
    test_support::set_test_downloader_playback_hook(hook);
}

#[cfg(test)]
pub(crate) fn set_test_downloader_track_package_hook(
    hook: Option<fn(serde_json::Value) -> Result<serde_json::Value, String>>,
) {
    test_support::set_test_downloader_track_package_hook(hook);
}

#[cfg(test)]
pub(crate) fn set_test_backend_stat_entry_hook(
    hook: Option<fn(&RepositoryRecord, &Path, &str) -> Option<Result<FileSystemEntry, String>>>,
) {
    test_support::set_test_backend_stat_entry_hook(hook);
}

pub(super) fn load_runtime_plugin_manifests(service_root: &Path) -> Vec<DiscoveredPluginManifest> {
    let mut manifests = load_plugin_manifests_from_runtime(runtime_plugins_dir(service_root));
    manifests.sort_by(|left, right| left.manifest.plugin_id.cmp(&right.manifest.plugin_id));
    manifests
}

pub(super) fn load_plugin_manifests_from_runtime(
    runtime_root: PathBuf,
) -> Vec<DiscoveredPluginManifest> {
    match read_plugin_manifests_from_dir(&runtime_root) {
        Ok(manifests) => manifests,
        Err(error) => {
            crate::app_log!(
                "error",
                "plugin.runtime",
                "manifestScanFailed",
                "读取运行时插件清单失败。",
                serde_json::json!({
                    "runtimeRoot": runtime_root.display().to_string(),
                    "error": error,
                })
            );
            Vec::new()
        }
    }
}

pub(super) fn read_plugin_manifests_from_dir(
    root: &Path,
) -> Result<Vec<DiscoveredPluginManifest>, String> {
    let mut manifests = Vec::new();
    if !root.is_dir() {
        return Ok(manifests);
    }
    for entry in fs::read_dir(root).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let plugin_path = entry.path();
        if plugin_path.is_dir() {
            match read_discovered_plugin_manifest_from_directory(&plugin_path) {
                Ok(Some(discovered)) => manifests.push(discovered),
                Ok(None) => {}
                Err(error) => manifests.push(DiscoveredPluginManifest {
                    manifest: broken_plugin_manifest(&plugin_path, &error),
                    archive_path: plugin_path,
                    manifest_prefix: String::new(),
                }),
            }
            continue;
        }
        if plugin_path.extension().and_then(|value| value.to_str()) != Some("momoplug") {
            continue;
        }
        match read_discovered_plugin_manifest_from_archive(&plugin_path) {
            Ok(discovered) => manifests.push(discovered),
            Err(error) => manifests.push(DiscoveredPluginManifest {
                manifest: broken_plugin_manifest(&plugin_path, &error),
                archive_path: plugin_path,
                manifest_prefix: String::new(),
            }),
        }
    }
    manifests.sort_by(|left, right| left.manifest.plugin_id.cmp(&right.manifest.plugin_id));
    Ok(manifests)
}

pub(super) fn read_discovered_plugin_manifest_from_directory(
    plugin_dir: &Path,
) -> Result<Option<DiscoveredPluginManifest>, String> {
    let manifest_path = plugin_dir.join("manifest.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&manifest_path).map_err(io_error)?;
    Ok(Some(DiscoveredPluginManifest {
        manifest: parse_plugin_manifest_with_source(&raw, None)?,
        archive_path: plugin_dir.to_path_buf(),
        manifest_prefix: String::new(),
    }))
}

pub(super) fn read_discovered_plugin_manifest_from_archive(
    archive_path: &Path,
) -> Result<DiscoveredPluginManifest, String> {
    let (raw, manifest_prefix) = read_plugin_manifest_from_archive(archive_path)?;
    Ok(DiscoveredPluginManifest {
        manifest: parse_plugin_manifest_with_source(&raw, None)?,
        archive_path: archive_path.to_path_buf(),
        manifest_prefix,
    })
}

#[cfg(test)]
pub fn install_local_filesystem_test_plugin_archive(service_root: &Path) {
    let runtime_plugin_root = runtime_plugins_dir(service_root);
    let archive_path = runtime_plugin_root.join("local-filesystem.momoplug");
    if archive_path.exists() {
        return;
    }
    if let Some(parent) = archive_path.parent() {
        fs::create_dir_all(parent).expect("plugin archive parent should be created");
    }
    let file = File::create(&archive_path).expect("plugin archive should be created");
    let mut archive = zip::ZipWriter::new(file);
    archive
        .start_file(
            "momobako-local-filesystem-0.1.0/manifest.json",
            zip::write::SimpleFileOptions::default(),
        )
        .expect("manifest entry should start");
    archive
        .write_all(
            serde_json::to_string_pretty(&serde_json::json!({
                "pluginId": LOCAL_FILESYSTEM_PLUGIN_ID,
                "legacyPluginIds": [LEGACY_LOCAL_FILESYSTEM_PLUGIN_ID],
                "name": "Local Filesystem",
                "version": "0.1.0",
                "type": {
                    "layer": "source",
                    "kind": "filesystem"
                },
                "kind": "filesystem",
                "category": "source",
                "description": "Test local filesystem backend.",
                "capabilities": ["browse", "read", "write", "watch", "sync", "localRootPath"],
                "enabled": true,
                "sdk": "backend",
                "entry": {},
                "source": "system",
                "runtime": "manifest-only",
                "permissions": [],
                "contributes": {
                    "settings": {
                        "schemaVersion": 1,
                        "settingsPage": {
                            "label": "本地文件系统",
                            "description": "配置本地资源库的文件检索方式。",
                            "order": 10
                        },
                        "fields": [
                            {
                                "key": LOCAL_FILESYSTEM_FILE_SEARCH_MODE_KEY,
                                "label": "文件检索方式",
                                "type": "select",
                                "description": "NTFS 与 Everything 不可用时会自动回退到现有扫描。",
                                "default": "recursive",
                                "options": [
                                    { "label": "现有扫描", "value": "recursive" },
                                    { "label": "NTFS 索引", "value": "ntfs" },
                                    { "label": "Everything 索引", "value": "everything" }
                                ]
                            }
                        ]
                    }
                },
                "compat": {
                    "sdkVersion": "1",
                    "legacyPluginIds": []
                },
                "status": "ready"
            }))
            .expect("manifest should encode")
            .as_bytes(),
        )
        .expect("manifest should write");
    archive.finish().expect("plugin archive should finish");
}

pub(super) fn default_cache_entries() -> Vec<CacheEntry> {
    vec![
        CacheEntry {
            cache_type: "metadata".to_string(),
            key: "repo-main-001:asset-01".to_string(),
            last_accessed_at: now_rfc3339(),
        },
        CacheEntry {
            cache_type: "query".to_string(),
            key: "tag=封面".to_string(),
            last_accessed_at: now_rfc3339(),
        },
        CacheEntry {
            cache_type: "thumbnail".to_string(),
            key: "asset-02".to_string(),
            last_accessed_at: now_rfc3339(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::logging::{set_global_logger, AppLogger};
    use crate::services::repository::SystemLogPage;
    use std::{
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        std::env::temp_dir().join(format!("momobako-plugin-runtime-{name}-{unique}"))
    }

    fn list_records(logger: &AppLogger) -> SystemLogPage {
        logger
            .list(Some(SystemLogQuery {
                limit: Some(20),
                ..SystemLogQuery::default()
            }))
            .expect("logs should list")
    }

    #[test]
    fn internal_system_log_bridge_writes_standard_record() {
        let root = test_root("system-log-write");
        let logger = Arc::new(AppLogger::new(root.clone()).expect("logger should initialize"));
        set_global_logger(Some(logger.clone())).expect("global logger should register");

        let payload = dispatch_host_plugin_call(HostPluginCallEnvelope {
            service_root_dir: root.display().to_string(),
            plugin_id: "momobako.system".to_string(),
            method: "system.log.write".to_string(),
            payload: serde_json::json!({
                "level": "warn",
                "category": "plugin.runtime",
                "action": "healthChanged",
                "message": "后端插件运行状态变化。",
                "pluginId": "momobako.service.test",
                "repoId": "repo-main-001",
                "sourceKind": "backend-plugin",
                "sourceLabel": "Test Backend Plugin",
                "context": {
                    "healthy": false,
                },
                "location": {
                    "modulePath": "plugin.runtime",
                    "file": "runtime.rs",
                    "line": 42,
                }
            }),
        })
        .expect("internal host call should succeed");

        let record: SystemLogRecord =
            serde_json::from_value(payload).expect("record payload should decode");
        assert_eq!(record.action, "healthChanged");
        assert_eq!(record.source.kind, "backend-plugin");
        assert_eq!(record.source.plugin_id.as_deref(), Some("momobako.service.test"));

        let page = list_records(logger.as_ref());
        assert_eq!(page.records.len(), 1);
        assert_eq!(page.records[0].message, "后端插件运行状态变化。");

        set_global_logger(None).expect("global logger should clear");
        let _ = std::fs::remove_dir_all(root);
    }
}
