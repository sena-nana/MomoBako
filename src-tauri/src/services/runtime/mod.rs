//! Repository runtime facade and runtime-layer composition.

mod executor;
pub(crate) mod external_api;
pub(crate) mod preview_server;
pub(crate) mod watcher;

use crate::services::mutsuki_host;
use crate::services::repository::RepositoryState;
use mutsuki_tauri_host::{MutsukiTauriConfig, PathsConfig, PluginSelection};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
};

pub use external_api::ExternalApiConnectionStatus;
pub(crate) use external_api::{
    build_external_connection_status, generate_external_api_token, start_external_api_server,
    write_external_connection_file,
};
pub(crate) use preview_server::start_preview_server;
pub(crate) use watcher::{start_structure_refresh_worker, sync_watched_paths, RepositoryWatcher};

const PREVIEW_HOST: &str = "127.0.0.1";

/// Runtime shell that coordinates repository-state access, preview hosting, and write serialization.
#[derive(Clone)]
pub struct RepositoryRuntime {
    pub(crate) repository_state: Arc<RepositoryState>,
    pub(crate) watcher_handle: Arc<Mutex<RepositoryWatcher>>,
    pub(crate) write_lock: Arc<Mutex<()>>,
    pub(crate) preview_addr: String,
    pub(crate) external_connection: ExternalApiConnectionStatus,
}

impl RepositoryRuntime {
    /// Starts the repository runtime and all long-lived background services.
    pub fn start() -> Result<Self, String> {
        let root = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(".service-data");
        let repository_state = Arc::new(RepositoryState::from_root(root.clone()));
        let write_lock = Arc::new(Mutex::new(()));
        repository_state.ensure_initialized()?;
        repository_state.shutdown_runtime_helpers()?;
        let structure_refresh_tx =
            start_structure_refresh_worker(repository_state.clone(), write_lock.clone())?;
        repository_state.set_structure_refresh_sender(structure_refresh_tx)?;

        let watcher_handle =
            RepositoryWatcher::start(repository_state.clone(), write_lock.clone())?;
        let preview_addr = start_preview_server(repository_state.clone())?;
        let external_token = generate_external_api_token()?;
        let external_addr = start_external_api_server(
            repository_state.clone(),
            write_lock.clone(),
            external_token.clone(),
        )?;
        let started_at = external_api::now_unix_millis().to_string();
        let external_connection =
            build_external_connection_status(&root, &external_addr, &external_token, &started_at);
        write_external_connection_file(&external_connection)?;

        Ok(Self {
            repository_state,
            watcher_handle,
            write_lock,
            preview_addr,
            external_connection,
        })
    }

    /// Returns the active repository-runtime root on disk.
    pub fn service_root(&self) -> PathBuf {
        self.repository_state.root_path()
    }

    /// 读取现有插件启用状态与配置，生成 ABI 初始化选择，不迁移 Momo 配置目录。
    pub fn mutsuki_plugin_selection(&self) -> Result<PluginSelection, String> {
        let service_root = self.service_root();
        let mut enabled_plugin_ids = BTreeSet::new();
        let mut configs = BTreeMap::new();
        for manifest in self.repository_state.list_plugins()? {
            if !manifest.enabled || manifest.runtime != "native-dylib" {
                continue;
            }
            enabled_plugin_ids.insert(manifest.plugin_id.clone());
            let snapshot = self
                .repository_state
                .get_plugin_config(manifest.plugin_id.clone())?;
            configs.insert(
                manifest.plugin_id.clone(),
                serde_json::json!({
                    "pluginId": manifest.plugin_id,
                    "pluginDataDir": snapshot.data_directory,
                    "serviceRootDir": service_root,
                    "pluginConfig": snapshot.values,
                }),
            );
        }
        Ok(PluginSelection {
            enabled_plugin_ids: Some(enabled_plugin_ids),
            configs,
        })
    }

    /// 将 Mutsuki 的运行缓存挂在现有 `.service-data` 下，同时保留原插件包目录。
    pub fn mutsuki_config(&self) -> Result<MutsukiTauriConfig, String> {
        let service_root = self.service_root();
        let runtime_root = service_root.join("mutsuki");
        let mut config = MutsukiTauriConfig::for_app("MomoBako");
        config.paths = PathsConfig {
            app_data_dir: runtime_root.clone(),
            config_dir: runtime_root.join("config"),
            data_dir: runtime_root.join("data"),
            cache_dir: runtime_root.join("cache"),
            logs_dir: runtime_root.join("logs"),
            plugins_dir: service_root.join("plugins"),
            resources_dir: runtime_root.join("resources"),
            runners_dir: runtime_root.join("runners"),
        };
        config.plugin_selection = self.mutsuki_plugin_selection()?;
        Ok(config)
    }

    /// 配置、安装、删除或启停后原子切换桌面插件 generation。
    pub async fn reload_mutsuki_plugins(&self) -> Result<(), String> {
        let runtime = self.clone();
        let selection = self
            .run_write(move |_| runtime.mutsuki_plugin_selection())
            .await?;
        tauri::async_runtime::spawn_blocking(move || mutsuki_host::reload_plugins(selection))
            .await
            .map_err(|error| error.to_string())?
    }

    /// Builds a browser-facing preview URL for a previously prepared source token.
    pub fn preview_source_url(&self, token: &str) -> String {
        format!("http://{}/preview/{token}", self.preview_addr)
    }

    /// Returns the latest external API connection payload, including readiness derived from state init.
    pub fn external_api_connection_status(&self) -> ExternalApiConnectionStatus {
        ExternalApiConnectionStatus {
            ready: self.repository_state.ensure_initialized().is_ok(),
            ..self.external_connection.clone()
        }
    }

    pub fn set_app_handle(&self, app: tauri::AppHandle) -> Result<(), String> {
        self.repository_state.set_app_handle(app)
    }

    /// Performs best-effort cleanup for long-lived helper processes before app shutdown.
    pub fn shutdown_helpers(&self) {
        let _ = self
            .repository_state
            .shutdown_runtime_helpers()
            .map_err(|error| {
                crate::app_log!(
                    "warn",
                    "runtime.helper",
                    "shutdownFailed",
                    "运行时辅助进程停止失败。",
                    serde_json::json!({ "error": error })
                )
            });
    }
}
