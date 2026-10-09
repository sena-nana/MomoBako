//! 设置、插件与日志的对照场景：设置页、插件管理、来源登录、拓展工具页、日志和任务。
//!
//! 场景名和 `tmp/vue-mock/scenes/admin.ts` 的 Vue 场景同名，数据和 Vue 夹具一致：工作区是 `base()`
//! （`acceptance_base.rs`），插件清单读 `External/Plugins/*/manifest.json` 的真实内容，按 Vue 模拟 IPC 的
//! 口径改成已启用、依赖就绪；缓存、API 设计、外部连接和日志照 `tmp/vue-mock/ipc.ts` 与 `fixtures.ts` 的应答。
//! 状态一律经过真实消息归约，不直接拼界面字段。

use serde_json::{json, Value};

use crate::backend::services::repository::{
    PluginConfigSnapshot, PluginManifest, RepositoryAuthenticationStatus, RepositoryBackendSummary, RepositoryLocalCacheStatus,
    RepositorySummary, SystemLogLocation, SystemLogPage, SystemLogRecord, SystemLogSource, TaskProgressSnapshot,
};

use super::super::admin::{AdminMessage, SourceStep};
use super::super::{ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};
use super::base_scene::{summary, Base, NOW, REPO_ID, REPO_NAME};
use super::plugin_fixtures::{bundle_failed, bundle_loaded, bundled_plugins, players_of};
use super::seed_base;
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
    seed_base(model);
    model.admin.note_backends(&[local_summary()]);
    load_bundle(model, bundled_plugins(|_| true));
}

/// `settings-error`：设置页数据读取失败。Vue 场景让 `list_plugins` 一直失败：启动结束那次和打开设置页
/// 那次都读不到，`Promise.all` 整批失败，其它四份应答照常返回但不写入，所以手上一直没有插件。
pub(super) fn seed_settings_error(model: &mut ShellViewModel) {
    Base { plugins_error: Some(LOAD_ERROR), ..Base::default() }.seed(model);
    model.admin.note_backends(&[local_summary()]);
    model.reduce(ShellMessage::Admin(bundle_failed(LOAD_ERROR)));
    model.admin.take_effects();
}

/// `logs`：日志面板，四条记录和 Vue 夹具 `logRecord(1..4)` 一致。
pub(super) fn seed_logs(model: &mut ShellViewModel) {
    seed_base(model);
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
    seed_base(model);
    model.admin.note_backends(&[local_summary()]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(model, bundled_plugins(|id| id == LOCAL_FILESYSTEM));
    open_plugin_settings(model, LOCAL_FILESYSTEM);
}

/// `task-running` 和 `task-cancelling`：任务弹层打开，里面一个任务。
/// Vue 弹层不分运行和取消，取消中只是细节文字不同、进度不定。
pub(super) fn seed_task(model: &mut ShellViewModel, cancelling: bool) {
    seed_base(model);
    let (task_id, status, label, phase, percent) = if cancelling {
        ("task-cancelling", "cancelling", "正在取消扫描", "等待 worker 退出", None)
    } else {
        ("task-scan", "running", "扫描默认资源库", "已扫描 1,284 / 3,040 个文件", Some(42.0))
    };
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
        updated_at: NOW.into(),
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
    model.admin.note_backends(&[local_summary()]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(&mut model, bundled_plugins(|_| true));
    model
}

/// 拓展页插件管理里只有一个插件，并展开它的设置。下载服务、Office 转换和网易云来源都走这里。
fn plugin_settings_scene(plugin_id: &str) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.admin.note_backends(&[local_summary()]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(&mut model, bundled_plugins(|id| id == plugin_id));
    open_plugin_settings(&mut model, plugin_id);
    model
}

/// `source-auth-methods`：资源库列表里还有一个登录中的网易云仓库，再点「连接新账号」建好扫码会话。
fn source_auth_methods_scene() -> ShellViewModel {
    let mut model = Base { others: vec![netease_summary()], ..Base::default() }.model();
    model.admin.note_backends(&[local_summary(), netease_summary()]);
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

/// 一次写入设置页五份数据，和 Vue 模拟 IPC 的默认应答一致；插件列表换新后照产品重读播放器类型，
/// 只剩这几个插件时播放器也只剩它们登记的。
fn load_bundle(model: &mut ShellViewModel, plugins: Vec<PluginManifest>) {
    let players = players_of(&plugins);
    model.reduce(ShellMessage::Admin(bundle_loaded(plugins)));
    model.admin.take_effects();
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(players)));
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

/// 当前仓库的完整摘要，和 Vue 夹具 `repository("默认资源库")` 一致。
fn local_summary() -> RepositorySummary {
    summary(REPO_NAME, "ready")
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
