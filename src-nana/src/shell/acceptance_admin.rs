//! 设置、插件与日志的对照场景：设置页、插件管理、来源登录、拓展工具页、日志和任务。
//!
//! 场景名和 `tmp/vue-mock/scenes/admin.ts` 的 Vue 场景同名，数据和 Vue 夹具一致：插件清单读
//! `External/Plugins/*/manifest.json` 的真实内容，按 Vue 模拟 IPC 的口径改成已启用、依赖就绪；
//! 缓存、API 设计、外部连接和日志照 `tmp/vue-mock/ipc.ts` 与 `fixtures.ts` 的应答。
//! 状态一律经过真实消息归约，不直接拼界面字段。

use std::fs;
use std::path::PathBuf;

use serde_json::{json, Map, Value};

use crate::backend::services::repository::{
    ApiDesignSnapshot, CacheConfig, CacheSnapshot, PluginConfigSnapshot, PluginManifest, RepositoryAuthenticationStatus,
    RepositoryBackendSummary, RepositoryLocalCacheStatus, RepositorySummary, SystemLogLocation, SystemLogPage, SystemLogRecord,
    SystemLogSource, TaskProgressSnapshot,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;

use super::super::admin::{AdminMessage, SourceStep};
use super::super::workspace::WorkspaceRepository;
use super::super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::{present_repository, REPO_ID};

/// Vue 夹具的固定时间 `FIXED_NOW`。
const NOW: &str = "2026-10-08T08:00:00Z";
/// 设置数据读取失败时 `list_plugins` 的报错，和 Vue 场景 `settings-error` 一致。
pub(super) const LOAD_ERROR: &str = "读取插件目录失败：拒绝访问。 (os error 5)";
const LOCAL_FILESYSTEM: &str = "momobako.local-filesystem";
const DOWNLOADER: &str = "momobako.service.downloader";
const OFFICE_CONVERT: &str = "momobako.service.office-convert";
const NETEASE: &str = "momobako.netease.source";
/// 来源登录场景里已有的网易云仓库。
const NETEASE_REPO_ID: &str = "netease-cloud-music-10086";

/// 本面板的离屏对照场景。和 `ShellPage` 同名的场景在 `seed_*` 里，由 `acceptance::seed` 调用。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("logs-paused", logs_paused_scene()),
        ("extensions", extensions_scene()),
        ("downloader-settings", plugin_settings_scene(DOWNLOADER)),
        ("office-convert", plugin_settings_scene(OFFICE_CONVERT)),
        ("source-auth-gap", plugin_settings_scene(NETEASE)),
        ("source-auth-methods", source_auth_methods_scene()),
        ("foreign-tool", foreign_tool_scene()),
    ]
}

/// `settings`：设置页，全部内置插件，缓存、API 设计和外部连接都读到。
pub(super) fn seed_settings(model: &mut ShellViewModel) {
    present_repository(model);
    model.admin.note_backends(&[local_summary(model)]);
    load_bundle(model, bundled_plugins(|_| true));
}

/// `settings-error`：设置页数据读取失败。Vue 的 `Promise.all` 整批失败，其它四份应答照常返回但不写入。
pub(super) fn seed_settings_error(model: &mut ShellViewModel) {
    present_repository(model);
    model.admin.note_backends(&[local_summary(model)]);
    model.reduce(ShellMessage::Admin(AdminMessage::SettingsBundleLoaded {
        plugins: Err(LOAD_ERROR.into()),
        hooks: Ok(Vec::new()),
        cache: Ok(cache_snapshot()),
        api: Ok(api_design()),
        external: Ok(external_status()),
    }));
    model.admin.take_effects();
}

/// `logs`：日志面板，四条记录和 Vue 夹具 `logRecord(1..4)` 一致。
pub(super) fn seed_logs(model: &mut ShellViewModel) {
    present_repository(model);
    model.workspace.panel = WorkspacePanel::Logs;
    let records = vec![
        log_record(1, "info", "workspace.startup", "startupStart", "首屏启动流程开始。"),
        log_record(2, "info", "repository", "syncSuccess", "首屏启动文件变化同步完成。"),
        log_record(3, "warn", "plugin.frontend", "loadSlow", "插件前端加载较慢。"),
        log_record(4, "error", "repository", "thumbnailFailed", "缩略图生成失败：cover.png"),
    ];
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage { records, next_cursor: None })));
}

/// `plugin-settings`：拓展页的插件管理里展开本地文件系统插件的设置。
/// 插件只留这一个，工具区不出现，设置区在首屏里。
pub(super) fn seed_plugin_settings(model: &mut ShellViewModel) {
    present_repository(model);
    model.admin.note_backends(&[local_summary(model)]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(model, bundled_plugins(|id| id == LOCAL_FILESYSTEM));
    open_plugin_settings(model, LOCAL_FILESYSTEM);
}

/// `task-running` 和 `task-cancelling`：任务弹层打开，里面一个任务。
/// Vue 弹层不分运行和取消，取消中只是细节文字不同、进度不定。
pub(super) fn seed_task(model: &mut ShellViewModel, cancelling: bool) {
    present_repository(model);
    model.active_tasks = 1;
    let (task_id, status, label, phase, percent) = if cancelling {
        ("task-cancelling", "cancelling", "正在取消扫描", "等待 worker 退出", None)
    } else {
        ("task-scan", "running", "扫描默认资源库", "已扫描 1,284 / 3,040 个文件", Some(42.0))
    };
    model.active_task_ids = vec![task_id.into()];
    model.reduce(ShellMessage::TaskProgressLoaded(vec![TaskProgressSnapshot {
        task_id: task_id.into(),
        protocol_id: "momobako.sync".into(),
        status: status.into(),
        phase: Some(phase.into()),
        label: Some(label.into()),
        current: None,
        total: None,
        percent,
        error: None,
        updated_at: "0".into(),
    }]));
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    // 离屏会话不走帧时钟，把弹层的出现动效拨到结束，截图里是停稳的样子。
    model.motion.advance(60_000);
}

/// `logs-paused`：日志面板点了「暂停追踪」，日志列表不跟随末尾，停在顶部。
/// Vue 的跟随实际不滚动，这个场景用来和 Vue 对照版式。
fn logs_paused_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::Logs);
    model.reduce(ShellMessage::Admin(AdminMessage::SetLogPaused(true)));
    model
}

/// `extensions`：拓展页，全部内置插件。工具页来自前端插件的 `toolPages`。
fn extensions_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.admin.note_backends(&[local_summary(&model)]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(&mut model, bundled_plugins(|_| true));
    model
}

/// 拓展页插件管理里只有一个插件，并展开它的设置。下载服务、Office 转换和网易云来源都走这里。
fn plugin_settings_scene(plugin_id: &str) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.admin.note_backends(&[local_summary(&model)]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(&mut model, bundled_plugins(|id| id == plugin_id));
    open_plugin_settings(&mut model, plugin_id);
    model
}

/// `source-auth-methods`：已有一个登录中的网易云仓库，再点「连接新账号」建好扫码会话。
fn source_auth_methods_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: NETEASE_REPO_ID.into(),
        name: "Netease Cloud Music Source 10086".into(),
        path: "netease-cloud-music://account/10086".into(),
        status: "ready".into(),
        backend_plugin_id: NETEASE.into(),
        capabilities: vec!["read".into(), "list".into()],
        cache_required: true,
        cache_status: "ready".into(),
    });
    let local = local_summary(&model);
    model.admin.note_backends(&[local, netease_summary()]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(&mut model, bundled_plugins(|id| id == NETEASE));
    open_plugin_settings(&mut model, NETEASE);
    model.reduce(ShellMessage::Admin(AdminMessage::BeginSourceAuth { plugin_id: NETEASE.into(), repo_id: None }));
    model.admin.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::SourceStepFinished { step: SourceStep::CreateSession, result: Ok(qr_session()) }));
    model.admin.take_effects();
    model
}

/// `foreign-tool`：第三方插件的工具页。Vue 直接挂插件自己的组件，Nana 只能画页头和说明，
/// 没有 Vue 对照图。
fn foreign_tool_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.workspace.panel = WorkspacePanel::Extensions;
    model.reduce(ShellMessage::Admin(AdminMessage::SetToolPages(vec![super::super::admin::ToolPageEntry {
        id: "user.custom.tool".into(),
        label: "自定义工具".into(),
        plugin_name: "示例插件".into(),
        description: "把当前目录交给外部流程。".into(),
        native: false,
    }])));
    model
}

/// 一次写入设置页五份数据，和 Vue 模拟 IPC 的默认应答一致。
fn load_bundle(model: &mut ShellViewModel, plugins: Vec<PluginManifest>) {
    model.reduce(ShellMessage::Admin(AdminMessage::SettingsBundleLoaded {
        plugins: Ok(plugins),
        hooks: Ok(Vec::new()),
        cache: Ok(cache_snapshot()),
        api: Ok(api_design()),
        external: Ok(external_status()),
    }));
    model.admin.take_effects();
}

/// 点插件卡片的「设置」，再交回 `get_plugin_config` 的模拟应答（没有保存过的值）。
fn open_plugin_settings(model: &mut ShellViewModel, plugin_id: &str) {
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings(plugin_id.into())));
    model.admin.take_effects();
    model.reduce(ShellMessage::PluginConfigLoaded(Ok(PluginConfigSnapshot {
        plugin_id: plugin_id.into(),
        data_directory: format!("C:/plugins/{plugin_id}"),
        schema: Value::Null,
        values: Default::default(),
    })));
}

/// 读仓库自带的插件清单，按目录名排序（Vite `import.meta.glob` 的顺序），改成已启用、
/// 依赖就绪的内置插件。读不到或解析失败的清单记日志后跳过。
fn bundled_plugins(keep: impl Fn(&str) -> bool) -> Vec<PluginManifest> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../External/Plugins");
    let mut manifests = match fs::read_dir(&root) {
        Ok(entries) => entries.filter_map(Result::ok).map(|entry| entry.path().join("manifest.json")).filter(|path| path.is_file()).collect::<Vec<_>>(),
        Err(error) => {
            eprintln!("Nana 验收读不到插件目录 {}：{error}", root.display());
            return Vec::new();
        }
    };
    manifests.sort();
    manifests
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).map_err(|error| eprintln!("Nana 验收读不到插件清单 {}：{error}", path.display())).ok()?;
            let mut manifest: Map<String, Value> =
                serde_json::from_str(&text).map_err(|error| eprintln!("Nana 验收插件清单不是 JSON {}：{error}", path.display())).ok()?;
            let plugin_id = manifest.get("pluginId").and_then(Value::as_str)?.to_string();
            if !keep(&plugin_id) {
                return None;
            }
            mock_install(&mut manifest);
            serde_json::from_value(Value::Object(manifest))
                .map_err(|error| eprintln!("Nana 验收插件清单字段不全 {plugin_id}：{error}"))
                .ok()
        })
        .collect()
}

/// 照 `tmp/vue-mock/ipc.ts` 的 `realPlugins`：已启用、就绪、内置来源、依赖全部可用。
fn mock_install(manifest: &mut Map<String, Value>) {
    let version = manifest.get("version").and_then(Value::as_str).unwrap_or("0").to_string();
    let dependencies = |key: &str| {
        manifest
            .get(key)
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(|id| json!({ "pluginId": id, "name": null, "status": "ready", "enabled": true, "available": true }))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let status = json!({
        "required": dependencies("requires"),
        "optional": dependencies("optional"),
        "missingRequired": [],
        "missingOptional": [],
        "disabledRequired": [],
        "disabledOptional": [],
    });
    manifest.insert("enabled".into(), Value::Bool(true));
    manifest.insert("status".into(), Value::String("ready".into()));
    manifest.entry("source").or_insert_with(|| Value::String("builtin".into()));
    manifest.insert("provenance".into(), Value::String("bundled".into()));
    manifest.insert("packageHash".into(), Value::String(format!("mock-{version}")));
    manifest.insert("dependencyStatus".into(), status);
}

fn cache_snapshot() -> CacheSnapshot {
    CacheSnapshot { config: CacheConfig { metadata_capacity: 512, thumbnail_capacity: 1024, query_capacity: 256 }, entries: Vec::new() }
}

fn api_design() -> ApiDesignSnapshot {
    ApiDesignSnapshot { transport: "tauri-ipc".into(), endpoints: Vec::new() }
}

fn external_status() -> ExternalApiConnectionStatus {
    ExternalApiConnectionStatus {
        base_url: "http://127.0.0.1:41595".into(),
        token: "mock-token".into(),
        version: "0.1.0".into(),
        started_at: NOW.into(),
        ready: true,
        connection_file_path: "C:/Users/acceptance/AppData/Roaming/com.momobako.desktop/external-api.json".into(),
    }
}

/// 当前仓库的完整摘要，和 Vue 夹具 `repository("默认资源库")` 一致。
fn local_summary(model: &ShellViewModel) -> RepositorySummary {
    RepositorySummary {
        repo_id: REPO_ID.into(),
        name: model.repository_name.clone(),
        path: "C:/acceptance".into(),
        backend: RepositoryBackendSummary {
            plugin_id: "momobako.source.local-filesystem".into(),
            kind: "local-filesystem".into(),
            name: "本地文件系统".into(),
            capabilities: ["write", "localRootPath", "list", "read", "move", "delete", "watch"].map(String::from).to_vec(),
        },
        status: "ready".into(),
        asset_count: 2,
        updated_at: NOW.into(),
        local_cache: None,
        authentication: None,
    }
}

/// 已登录的网易云仓库，缓存放在本地目录。
fn netease_summary() -> RepositorySummary {
    RepositorySummary {
        repo_id: NETEASE_REPO_ID.into(),
        name: "Netease Cloud Music Source 10086".into(),
        path: "netease-cloud-music://account/10086".into(),
        backend: RepositoryBackendSummary {
            plugin_id: NETEASE.into(),
            kind: "netease-cloud-music".into(),
            name: "Netease Cloud Music Source".into(),
            capabilities: vec!["read".into(), "list".into()],
        },
        status: "ready".into(),
        asset_count: 0,
        updated_at: NOW.into(),
        local_cache: Some(RepositoryLocalCacheStatus { required: true, path: Some("D:/MomoCache/netease-10086".into()), status: "ready".into() }),
        authentication: Some(RepositoryAuthenticationStatus { required: true, logged_in: true, login_expired: false }),
    }
}

/// `auth.createQrSession` 的模拟应答：会话键和扫码地址。
fn qr_session() -> Value {
    json!({ "unikey": "mock-unikey-0001", "qrurl": "https://music.163.com/login?codekey=mock-unikey-0001" })
}

/// Vue 夹具 `logRecord`：时间是 07:5x:00Z，来源是 MomoBako 核心。
fn log_record(index: u32, level: &str, category: &str, action: &str, message: &str) -> SystemLogRecord {
    SystemLogRecord {
        id: format!("log-{index}"),
        timestamp: format!("2026-10-08T07:{:02}:00Z", 50 + index),
        level: level.into(),
        category: category.into(),
        action: action.into(),
        message: message.into(),
        source: SystemLogSource { kind: "core".into(), label: Some("MomoBako".into()), plugin_id: None, repo_id: Some(REPO_ID.into()) },
        location: SystemLogLocation { module_path: Some("momobako::repository".into()), file: None, line: None },
        context: json!({}),
    }
}
