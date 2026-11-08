//! 设置、插件、日志和仓库动作的对照场景：设置页、插件管理、来源登录、拓展工具页、日志、任务和动作面板。
//!
//! 场景名和 `tmp/vue-mock/scenes/admin.ts` 的 Vue 场景同名，数据和 Vue 夹具一致：工作区是 `base()`
//! （`acceptance_base.rs`），插件清单读 `External/Plugins/*/manifest.json` 的真实内容，按 Vue 模拟 IPC 的
//! 口径改成已启用、依赖就绪；缓存、API 设计、外部连接和日志照 `tmp/vue-mock/ipc.ts` 与 `fixtures.ts` 的应答。
//! 状态一律经过真实消息归约，不直接拼界面字段。

use serde_json::{Value, json};

use nana_ui_platform::WindowId;
use nana_ui_platform::host::WindowCommand;

use crate::backend::services::repository::{
    FileBrowserSnapshot, PluginConfigSnapshot, PluginManifest, RepositoryAuthenticationStatus,
    RepositoryBackendSummary, RepositoryLocalCacheStatus, RepositoryStructureCacheState,
    RepositorySummary, SystemLogLocation, SystemLogPage, SystemLogRecord, SystemLogSource,
    TaskProgressSnapshot,
};

use super::super::admin::{AdminEffect, AdminMessage, SourceStep};
use super::super::input::InputMessage;
use super::super::{
    ShellMessage, ShellPage, ShellViewModel, ShortcutId, SidebarMessage, WorkspacePanel,
};
use super::base_scene::{Base, NOW, REPO_ID, REPO_NAME, REPO_PATH, settle, summary};
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
        ("file-import", file_import_scene()),
        ("eagle-import", eagle_import_scene()),
        ("eagle-import-busy", eagle_import_busy_scene()),
        ("eagle-import-blocked", eagle_import_blocked_scene()),
        ("downloader-settings", plugin_settings_scene(DOWNLOADER)),
        ("office-convert", plugin_settings_scene(OFFICE_CONVERT)),
        ("source-auth-gap", plugin_settings_scene(NETEASE)),
        ("source-auth-methods", source_auth_methods_scene()),
        ("foreign-tool", foreign_tool_scene()),
        ("repository-actions", repository_actions_scene()),
        ("logs-no-library", logs_no_library_scene()),
        ("extensions-no-library", extensions_no_library_scene()),
        ("actions-no-library", actions_no_library_scene()),
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
    Base {
        plugins_error: Some(LOAD_ERROR),
        ..Base::default()
    }
    .seed(model);
    model.admin.note_backends(&[local_summary()]);
    model.reduce(ShellMessage::Admin(bundle_failed(LOAD_ERROR)));
    model.admin.take_effects();
}

/// `logs`：日志面板，四条记录和 Vue 夹具 `logRecord(1..4)` 一致。
pub(super) fn seed_logs(model: &mut ShellViewModel) {
    seed_base(model);
    model.workspace.panel = WorkspacePanel::Logs;
    let records = vec![
        log_record(
            1,
            "info",
            "workspace.startup",
            "startupStart",
            "首屏启动流程开始。",
        ),
        log_record(
            2,
            "info",
            "repository",
            "syncSuccess",
            "首屏启动文件变化同步完成。",
        ),
        log_record(
            3,
            "warn",
            "plugin.frontend",
            "loadSlow",
            "插件前端加载较慢。",
        ),
        log_record(
            4,
            "error",
            "repository",
            "thumbnailFailed",
            "缩略图生成失败：cover.png",
        ),
    ];
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records,
        next_cursor: None,
    })));
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
        (
            "task-cancelling",
            "cancelling",
            "正在取消扫描",
            "等待 worker 退出",
            None,
        )
    } else {
        (
            "task-scan",
            "running",
            "扫描默认资源库",
            "已扫描 1,284 / 3,040 个文件",
            Some(42.0),
        )
    };
    model.reduce(ShellMessage::TaskProgressLoaded(vec![
        TaskProgressSnapshot {
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
        },
    ]));
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

/// `extensions`：拓展页，全部内置插件。工具页来自工具插件清单的 `toolPages`。
fn extensions_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.admin.note_backends(&[local_summary()]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(&mut model, bundled_plugins(|_| true));
    model
}

/// `file-import`：拓展页里点开「文件导入」。夹具仓库是「默认资源库」，当前目录是根，目标目录写成「/」。
///
/// 不打开文件夹或 ZIP 对话框，不写完成提示，也不编来源路径。按钮停在 Vue 的空闲文案。
fn file_import_scene() -> ShellViewModel {
    let mut model = extensions_scene();
    let page_id = super::super::admin::support::TOOL_FILE_MANAGER;
    model.reduce(ShellMessage::Admin(AdminMessage::SelectToolPage(
        page_id.into(),
    )));
    if model.admin.active_tool_page_id.as_deref() != Some(page_id) {
        eprintln!("Nana 文件导入场景没有切到文件导入工具页");
    }
    if !model.files.current_path.is_empty() {
        eprintln!(
            "Nana 文件导入场景的当前目录不是根目录：{}",
            model.files.current_path
        );
    }
    if model.files.dialog != super::super::files::FileDialog::Closed {
        eprintln!("Nana 文件导入场景不该打开导入对话框");
    }
    settle(&mut model);
    model
}

/// `eagle-import`：拓展页里点开「Eagle 导入」。夹具仓库是「默认资源库」，当前目录是根，目标目录写成「/」。
///
/// 库路径留空，页面按 register.js 写「尚未选择」。不打开目录对话框，不提交复制或剪切，不画结果卡，
/// 不写完成提示。复制按钮停在空闲文案，不写成「处理中...」。
fn eagle_import_scene() -> ShellViewModel {
    let mut model = extensions_scene();
    let page_id = super::super::admin::support::TOOL_EAGLE_IMPORTER;
    model.reduce(ShellMessage::Admin(AdminMessage::SelectToolPage(
        page_id.into(),
    )));
    if model.admin.active_tool_page_id.as_deref() != Some(page_id) {
        eprintln!("Nana Eagle 导入场景没有切到 Eagle 导入工具页");
    }
    if !model.files.current_path.is_empty() {
        eprintln!(
            "Nana Eagle 导入场景的当前目录不是根目录：{}",
            model.files.current_path
        );
    }
    if !model.admin.eagle_library_path.trim().is_empty() {
        eprintln!(
            "Nana Eagle 导入场景不该预先选中库路径：{}",
            model.admin.eagle_library_path
        );
    }
    if model.admin.eagle_import.is_some()
        || model.admin.eagle_importing
        || !model.admin.eagle_notice.is_empty()
    {
        eprintln!("Nana Eagle 导入场景不该带结果、处理中或完成提示");
    }
    if model.files.dialog != super::super::files::FileDialog::Closed {
        eprintln!("Nana Eagle 导入场景不该打开导入对话框");
    }
    settle(&mut model);
    model
}

/// `eagle-import-busy`：复制导入已经提交，协议结果还没回来。
///
/// 库目录用夹具仓库自己的路径 `C:/acceptance`，经「选择目录」写上。复制按钮在这期间是「处理中...」，
/// 剪切按钮不换字。不交回协议结果，所以没有完成提示，也没有结果卡。
fn eagle_import_busy_scene() -> ShellViewModel {
    let mut model = eagle_import_scene();
    model.reduce(ShellMessage::Admin(AdminMessage::ChooseEagleLibrary));
    if !complete_eagle_library_dialog(&mut model, REPO_PATH) {
        eprintln!("Nana Eagle 导入进行中场景没有选上库目录");
    }
    model.reduce(ShellMessage::Admin(AdminMessage::RunEagleImport(
        "copy".into(),
    )));
    if !model.admin.eagle_importing {
        eprintln!("Nana Eagle 导入进行中场景没有进入处理中");
    }
    if model.admin.eagle_import.is_some() || !model.admin.eagle_notice.is_empty() {
        eprintln!("Nana Eagle 导入进行中场景不该写出结果或完成提示");
    }
    if model.files.dialog != super::super::files::FileDialog::Closed {
        eprintln!("Nana Eagle 导入进行中场景不该打开导入对话框");
    }
    settle(&mut model);
    model
}

/// `eagle-import-blocked`：文件快照还是空回收站，面板已经回到 Eagle 导入页。
///
/// 夹具回收站没有条目，不编文件。四条原因按面板写：这里不是回收站面板、不是虚拟视图、仓库也能写，
/// 所以对不上那四句。快照仍标着回收站，状态机写出「当前目标不可导入。」。侧栏跟面板走，回到和空闲
/// Eagle 导入一样的默认资源库，不留回收站说明。不提交导入，不编造成功。
fn eagle_import_blocked_scene() -> ShellViewModel {
    let mut model = eagle_import_scene();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectShortcut(
        ShortcutId::Trash,
    )));
    if model.sidebar.take_browses().is_empty() {
        eprintln!("Nana Eagle 导入兜底场景没有排下回收站读取");
    }
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(empty_trash_snapshot())));
    if !model.files.trash {
        eprintln!("Nana Eagle 导入兜底场景的文件快照不是回收站");
    }
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Extensions));
    let page_id = super::super::admin::support::TOOL_EAGLE_IMPORTER;
    model.reduce(ShellMessage::Admin(AdminMessage::SelectToolPage(
        page_id.into(),
    )));
    if model.workspace.panel != WorkspacePanel::Extensions {
        eprintln!("Nana Eagle 导入兜底场景没有回到拓展页");
    }
    if model.admin.eagle_importing
        || model.admin.eagle_import.is_some()
        || !model.admin.eagle_notice.is_empty()
    {
        eprintln!("Nana Eagle 导入兜底场景不该进入处理中或写出结果");
    }
    settle(&mut model);
    model
}

/// 交回「选择目录」的路径。没有排下文件夹对话框时只记日志。
fn complete_eagle_library_dialog(model: &mut ShellViewModel, path: &str) -> bool {
    let commands = model.input.take_platform_commands(WindowId(1), false);
    let request_id = match commands.as_slice() {
        [WindowCommand::OpenFileDialog { request, .. }] => request.id,
        other => {
            eprintln!("Nana Eagle 导入场景没有排下目录对话框：{other:?}");
            return false;
        }
    };
    model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
        request_id,
        paths: vec![path.to_string()],
        failed: None,
    }));
    true
}

/// 回收站根目录的空快照。夹具没有回收站条目，不另造文件。
fn empty_trash_snapshot() -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: REPO_ID.into(),
        root_path: REPO_PATH.into(),
        backend_plugin_id: "momobako.source.local-filesystem".into(),
        backend_kind: "local-filesystem".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: Some(NOW.into()),
        current_path: String::new(),
        total_entries: 0,
        loaded_count: 0,
        next_offset: None,
        has_more: false,
        special_location: Some("trash".into()),
        tree: None,
        entries: Vec::new(),
    }
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
    let mut model = Base {
        others: vec![netease_summary()],
        ..Base::default()
    }
    .model();
    model
        .admin
        .note_backends(&[local_summary(), netease_summary()]);
    model.workspace.panel = WorkspacePanel::Extensions;
    load_bundle(&mut model, bundled_plugins(|id| id == NETEASE));
    open_plugin_settings(&mut model, NETEASE);
    model.reduce(ShellMessage::Admin(AdminMessage::BeginSourceAuth {
        plugin_id: NETEASE.into(),
        repo_id: None,
    }));
    model.admin.take_effects();
    model.reduce(ShellMessage::Admin(AdminMessage::SourceStepFinished {
        step: SourceStep::CreateSession,
        result: Ok(qr_session()),
    }));
    model.admin.take_effects();
    model
}

/// `repository-actions`：切到仓库动作面板，再按夹具答空列表。
///
/// 启动绑侧栏时 Vue 模拟 IPC 的 `list_repository_actions` 答空数组，底子里已经答过一次。切到动作面板会再读
/// （`queue_actions`）。夹具没有导入动作，这次仍答空列表：页头是「动作 / 仓库动作 / 0 项」，下面是
/// 「当前仓库没有导入动作。」。不编动作名、步骤、插件或运行结果，也不把执行标成成功。侧栏「动作」入口
/// 只在有动作时出现，这里不出现。
fn repository_actions_scene() -> ShellViewModel {
    let mut model = Base::default().model();
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    let Some(repo_id) = model.repository_id.clone() else {
        eprintln!("Nana 动作场景没有活动仓库，不能读动作列表");
        settle(&mut model);
        return model;
    };
    let queued =
        model.admin.take_effects().into_iter().any(
            |effect| matches!(effect, AdminEffect::LoadActions { repo_id: id } if id == repo_id),
        );
    if !queued {
        eprintln!("Nana 动作场景切到面板没有排下读取");
    }
    model.reduce(ShellMessage::Admin(AdminMessage::ActionsLoaded {
        repo_id,
        result: Ok(Vec::new()),
    }));
    settle(&mut model);
    model
}

/// 空库启动已经走完：资源库列表是空的。日志、拓展和动作都从这里切过去，不另编仓库。
fn empty_library() -> ShellViewModel {
    let model = ShellViewModel::for_page(ShellPage::EmptyRepository);
    if !model.workspace.repositories.is_empty() || model.repository_id.is_some() {
        eprintln!(
            "Nana 无资源库场景不是空库：仓库 {} 个，活动仓库 {:?}",
            model.workspace.repositories.len(),
            model.repository_id
        );
    }
    model
}

/// `logs-no-library`：没有资源库时打开日志。切到面板会读历史，夹具没有记录，答空列表。
///
/// 空文案照 Vue `WorkspaceLogsPanel.vue`：「还没有系统日志」和
/// 「宿主、插件和辅助进程产生的关键操作会在这里持续汇总。」。不编四条启动日志，也不停在「正在加载」。
fn logs_no_library_scene() -> ShellViewModel {
    let mut model = empty_library();
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    let queued = model
        .admin
        .take_effects()
        .into_iter()
        .any(|effect| matches!(effect, AdminEffect::LoadLogs));
    if !queued {
        eprintln!("Nana 无资源库日志场景切到面板没有排下读取");
    }
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records: Vec::new(),
        next_cursor: None,
    })));
    settle(&mut model);
    model
}

/// `extensions-no-library`：没有资源库时打开拓展。空库启动已经读过真实插件清单，这里不再读，也不把插件清掉。
///
/// 工具页按清单顺序停在 API Playground。快照没有端点时页面自己用 Vue 的三个兜底接口，场景不另写接口或调用结果。
fn extensions_no_library_scene() -> ShellViewModel {
    let mut model = empty_library();
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Extensions));
    if model.admin.plugins.is_empty() {
        eprintln!("Nana 无资源库拓展场景没有读到插件清单");
    }
    settle(&mut model);
    model
}

/// `actions-no-library`：没有资源库时打开仓库动作。没有仓库就不读列表。
///
/// 空文案仍是 Vue `RepositoryActionsPanel.vue` 的「当前仓库没有导入动作。」。不编动作名、步骤或运行结果。
fn actions_no_library_scene() -> ShellViewModel {
    let mut model = empty_library();
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    if !model.admin.actions.is_empty() || model.admin.actions_loading {
        eprintln!("Nana 无资源库动作场景不是空列表");
    }
    settle(&mut model);
    model
}

/// `foreign-tool`：插件声明了宿主没有的工具页，只画页头和说明。Vue 里第三方工具页由插件自己的组件绘制，
/// 没有 Vue 对照图。
fn foreign_tool_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.workspace.panel = WorkspacePanel::Extensions;
    model.reduce(ShellMessage::Admin(AdminMessage::SetToolPages(vec![
        super::super::admin::ToolPageEntry {
            id: "user.custom.tool".into(),
            label: "自定义工具".into(),
            plugin_name: "示例插件".into(),
            description: "把当前目录交给外部流程。".into(),
            native: false,
        },
    ])));
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
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleSettings(
        plugin_id.into(),
    )));
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
        local_cache: Some(RepositoryLocalCacheStatus {
            required: true,
            path: Some("D:/MomoCache/netease-10086".into()),
            status: "ready".into(),
        }),
        authentication: Some(RepositoryAuthenticationStatus {
            required: true,
            logged_in: true,
            login_expired: false,
        }),
    }
}

/// `auth.createQrSession` 的模拟应答：会话键和扫码地址。
fn qr_session() -> Value {
    json!({ "unikey": "mock-unikey-0001", "qrurl": "https://music.163.com/login?codekey=mock-unikey-0001" })
}

#[cfg(test)]
mod file_import_scene_tests {
    use super::super::super::files::FileDialog;
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::workspace::WorkspacePanel;
    use super::file_import_scene;

    /// 夹具根目录上的文件导入页：目标是默认资源库和「/」，对话框保持关闭。
    #[test]
    fn file_import_scene_shows_the_fixture_target_without_a_dialog() {
        let model = file_import_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Extensions);
        assert_eq!(
            model.admin.active_tool_page_id.as_deref(),
            Some("momobako.tool.file-manager")
        );
        assert!(model.files.current_path.is_empty(), "当前目录应是根");
        assert_eq!(model.files.dialog, FileDialog::Closed);
        assert!(model.files.error.is_empty(), "没有导入错误");
        assert!(!model.player_surface_visible(), "拓展页不留播放条");

        let harness = ShellHarness::mount(model);
        for label in [
            "文件导入",
            "File Manager",
            "目标固定为当前工作区的当前目录。",
            "目标仓库",
            "默认资源库",
            "目标目录",
            "/",
            "从文件夹导入",
            "从 ZIP 导入",
            "ZIP 首版固定支持 .zip，并按解压导入保留内部目录结构。",
        ] {
            assert!(harness.find(label).is_some(), "文件导入页缺少「{label}」");
        }
        for label in [
            "处理中...",
            "文件夹导入完成。",
            "ZIP 导入完成。",
            "当前没有可用仓库。",
            "从 Eagle 导入",
        ] {
            assert!(
                harness.find(label).is_none(),
                "文件导入页不该出现「{label}」"
            );
        }
    }
}

#[cfg(test)]
mod eagle_import_scene_tests {
    use nana_ui_platform::WindowId;
    use nana_ui_platform::host::WindowCommand;

    use super::super::super::files::FileDialog;
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::workspace::WorkspacePanel;
    use super::eagle_import_scene;

    /// 夹具根目录上的 Eagle 导入页：没有库路径，停在「尚未选择」，对话框保持关闭。
    #[test]
    fn eagle_import_scene_stays_idle_without_a_library_path() {
        let mut model = eagle_import_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Extensions);
        assert_eq!(
            model.admin.active_tool_page_id.as_deref(),
            Some("momobako.tool.eagle-importer")
        );
        assert!(model.files.current_path.is_empty(), "当前目录应是根");
        assert!(
            model.admin.eagle_library_path.trim().is_empty(),
            "没有选中库路径"
        );
        assert!(model.admin.eagle_import.is_none(), "不编结果卡");
        assert!(!model.admin.eagle_importing, "没有进入处理中");
        assert!(model.admin.eagle_notice.is_empty(), "不写完成提示");
        assert!(model.files.error.is_empty(), "没有导入错误");
        assert_eq!(model.files.dialog, FileDialog::Closed);
        assert!(!model.player_surface_visible(), "拓展页不留播放条");
        let commands = model.input.take_platform_commands(WindowId(1), false);
        assert!(
            !commands
                .iter()
                .any(|command| matches!(command, WindowCommand::OpenFileDialog { .. })),
            "没有打开目录对话框：{commands:?}"
        );

        let harness = ShellHarness::mount(model);
        for label in [
            "Eagle 导入",
            "Eagle Importer",
            "导入目标固定为当前工作区的当前目录。",
            "目标仓库",
            "默认资源库",
            "目标目录",
            "/",
            "EagleLibrary",
            "尚未选择",
            "选择目录",
            "复制导入",
            "剪切导入",
        ] {
            assert!(harness.find(label).is_some(), "Eagle 导入页缺少「{label}」");
        }
        for label in [
            "处理中...",
            "复制导入完成，目标目录：/",
            "剪切导入完成，目标目录：/",
            "导入结果",
            "当前目标不可导入。",
            "请先选择 EagleLibrary 目录。",
            "从 Eagle 复制导入",
            "从 Eagle 剪切导入",
            "Eagle 资源库路径",
            "当前没有可用仓库。",
        ] {
            assert!(
                harness.find(label).is_none(),
                "Eagle 导入页不该出现「{label}」"
            );
        }
    }

    /// 复制导入停在进行中：按钮是「处理中...」，剪切不换字，没有完成提示和结果卡。
    #[test]
    fn eagle_import_scene_stays_on_copy_in_progress() {
        use super::eagle_import_busy_scene;

        let mut model = eagle_import_busy_scene();
        assert_eq!(
            model.admin.active_tool_page_id.as_deref(),
            Some("momobako.tool.eagle-importer")
        );
        assert_eq!(model.admin.eagle_library_path, "C:/acceptance");
        assert!(model.admin.eagle_importing, "复制导入还没回来");
        assert!(model.files.mutating, "文件变更还在进行");
        assert!(model.admin.eagle_import.is_none(), "不编结果卡");
        assert!(model.admin.eagle_notice.is_empty(), "不写完成提示");
        assert!(model.files.error.is_empty(), "进行中不是失败");
        assert_eq!(model.files.dialog, FileDialog::Closed);
        assert!(!model.player_surface_visible(), "拓展页不留播放条");
        let paths = model
            .files
            .rows
            .iter()
            .map(|row| row.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            ["assets", "cover.png", "notes/page.pdf"],
            "进行中不改夹具目录"
        );
        let commands = model.input.take_platform_commands(WindowId(1), false);
        assert!(commands.is_empty(), "目录对话框已经交回：{commands:?}");

        let harness = ShellHarness::mount(model);
        for label in [
            "Eagle 导入",
            "C:/acceptance",
            "选择目录",
            "处理中...",
            "剪切导入",
            "目标目录",
            "/",
        ] {
            assert!(
                harness.find(label).is_some(),
                "进行中的 Eagle 导入页缺少「{label}」"
            );
        }
        for label in [
            "复制导入",
            "尚未选择",
            "复制导入完成，目标目录：/",
            "剪切导入完成，目标目录：C:/acceptance",
            "导入结果",
            "当前目标不可导入。",
        ] {
            assert!(
                harness.find(label).is_none(),
                "进行中的 Eagle 导入页不该出现「{label}」"
            );
        }
    }

    /// 快照还是空回收站、面板已回拓展页：写出兜底句。侧栏按面板回到默认资源库，不留回收站说明。
    #[test]
    fn eagle_import_scene_shows_the_unnamed_import_block() {
        use super::eagle_import_blocked_scene;

        let model = eagle_import_blocked_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Extensions);
        assert_eq!(
            model.admin.active_tool_page_id.as_deref(),
            Some("momobako.tool.eagle-importer")
        );
        assert!(model.files.trash, "快照仍是回收站");
        assert!(model.files.rows.is_empty(), "夹具回收站没有条目");
        assert_ne!(
            model.workspace.panel,
            WorkspacePanel::Trash,
            "面板已经离开回收站"
        );
        assert!(!model.admin.eagle_importing, "没有提交导入");
        assert!(model.admin.eagle_import.is_none() && model.admin.eagle_notice.is_empty());
        assert!(
            model.admin.eagle_library_path.trim().is_empty(),
            "没有选库路径"
        );
        assert_eq!(model.files.dialog, FileDialog::Closed);
        assert!(!model.player_surface_visible(), "拓展页不留播放条");

        assert!(model.sidebar.browsing_trash, "快照仍把当前目录记成回收站");
        assert!(!model.sidebar.folders.is_empty(), "默认资源库的目录树还在");

        let harness = ShellHarness::mount(model);
        for label in [
            "Eagle 导入",
            "尚未选择",
            "当前目标不可导入。",
            "选择目录",
            "复制导入",
            "剪切导入",
            "assets",
        ] {
            assert!(
                harness.find(label).is_some(),
                "不能导入的 Eagle 页缺少「{label}」"
            );
        }
        for label in [
            "处理中...",
            "回收站视图不支持导入。",
            "当前仓库处于只读状态。",
            "当前没有可用仓库。",
            "虚拟视图不支持导入。",
            "导入结果",
            "复制导入完成",
            "剪切导入完成",
            "回收站条目在主视图中管理。",
            "当前仓库还没有子文件夹。",
        ] {
            assert!(
                harness.find(label).is_none(),
                "不能导入的 Eagle 页不该出现「{label}」"
            );
        }
    }
}

#[cfg(test)]
mod repository_actions_scene_tests {
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::workspace::WorkspacePanel;
    use super::repository_actions_scene;

    /// 夹具没有动作：面板停在空状态。布局以后加载提示和执行按钮都不占画面。
    #[test]
    fn repository_actions_scene_stays_empty_without_fixture_actions() {
        let mut model = repository_actions_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Actions);
        assert!(model.admin.actions.is_empty(), "夹具没有导入动作");
        assert!(model.admin.active_action_id.is_none(), "空列表没有选中项");
        assert!(!model.admin.actions_loading, "空列表已经读回");
        assert!(!model.admin.actions_running, "没有在执行动作");
        assert!(model.admin.actions_error.is_empty(), "空列表不是读取失败");
        assert!(
            !model.player_surface_visible(),
            "动作面板不是文件浏览面板，不留播放条"
        );
        assert!(model.admin.take_effects().is_empty(), "读取已经按夹具答完");

        let harness = ShellHarness::mount(model);
        for label in ["仓库动作", "动作", "0 项", "当前仓库没有导入动作。"] {
            assert!(harness.find(label).is_some(), "动作页缺少「{label}」");
        }
        for label in [
            "正在加载动作",
            "执行",
            "不支持",
            "已停用",
            "可执行",
            "最近运行",
            "未选择播放内容",
        ] {
            assert!(harness.find(label).is_none(), "动作页不该出现「{label}」");
        }
    }
}

#[cfg(test)]
mod no_library_scene_tests {
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::workspace::{MainRegion, WorkspacePanel};
    use super::{actions_no_library_scene, extensions_no_library_scene, logs_no_library_scene};

    fn mount(model: super::super::super::ShellViewModel) -> ShellHarness {
        ShellHarness::mount(model)
    }

    fn require(harness: &ShellHarness, label: &str) {
        assert!(harness.find(label).is_some(), "缺少「{label}」");
    }

    fn absent(harness: &ShellHarness, label: &str) {
        assert!(harness.find(label).is_none(), "不该出现「{label}」");
    }

    /// 没有资源库的日志页：画出 Vue 的空标题和说明，不画出夹具里的四条日志，也不停在加载。
    #[test]
    fn logs_no_library_draws_the_vue_empty_copy() {
        let mut model = logs_no_library_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Logs);
        assert_eq!(model.workspace.main_region(), MainRegion::EmptyRepository);
        assert!(model.workspace.repositories.is_empty() && model.repository_id.is_none());
        assert!(model.admin.logs.is_empty() && !model.admin.logs_loading);
        assert!(model.admin.take_effects().is_empty(), "空列表已经答完");
        let harness = mount(model);
        for label in [
            "系统日志",
            "还没有系统日志",
            "宿主、插件和辅助进程产生的关键操作会在这里持续汇总。",
            "0 条缓存",
            "0 条命中",
            "无资源库",
        ] {
            require(&harness, label);
        }
        for label in [
            "正在加载系统日志",
            "还没有可用资源库",
            "当前资源库筛选",
            "首屏启动流程开始。",
            "当前筛选没有命中",
            "默认资源库",
        ] {
            absent(&harness, label);
        }
    }

    /// 没有资源库的拓展页：插件是空库启动读到的清单，不画成「没有匹配的插件」。
    #[test]
    fn extensions_no_library_keeps_the_loaded_plugins() {
        let mut model = extensions_no_library_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Extensions);
        assert_eq!(model.workspace.main_region(), MainRegion::EmptyRepository);
        assert!(model.workspace.repositories.is_empty() && model.repository_id.is_none());
        assert!(
            !model.admin.plugins.is_empty(),
            "空库启动应读到真实插件清单"
        );
        assert!(!model.admin.loading_settings, "设置包已经答完");
        assert!(
            model.admin.take_effects().is_empty(),
            "已有清单时不再读设置包"
        );
        let harness = mount(model);
        for label in [
            "文件系统与插件",
            "API Playground",
            "后端 API 调试",
            "插件工具",
            "无资源库",
        ] {
            require(&harness, label);
        }
        for label in [
            "没有匹配的插件",
            "还没有可用资源库",
            "当前资源库筛选",
            "默认资源库",
            "正在加载插件信息",
        ] {
            absent(&harness, label);
        }
    }

    /// 没有资源库的动作页：不读列表，空文案仍是 Vue 的「当前仓库没有导入动作。」。
    #[test]
    fn actions_no_library_draws_the_vue_empty_copy() {
        let mut model = actions_no_library_scene();
        assert_eq!(model.workspace.panel, WorkspacePanel::Actions);
        assert_eq!(model.workspace.main_region(), MainRegion::EmptyRepository);
        assert!(model.workspace.repositories.is_empty() && model.repository_id.is_none());
        assert!(model.admin.actions.is_empty() && !model.admin.actions_loading);
        assert!(model.admin.active_action_id.is_none());
        assert!(
            model.admin.take_effects().is_empty(),
            "没有仓库，不该留下读取"
        );
        assert!(!model.player_surface_visible(), "动作面板不留播放条");
        let harness = mount(model);
        for label in [
            "仓库动作",
            "动作",
            "0 项",
            "当前仓库没有导入动作。",
            "无资源库",
        ] {
            require(&harness, label);
        }
        for label in [
            "正在加载动作",
            "执行",
            "还没有可用资源库",
            "当前资源库筛选",
            "默认资源库",
            "未选择播放内容",
        ] {
            absent(&harness, label);
        }
    }
}

/// Vue 夹具 `logRecord`：时间是 07:5x:00Z，来源是 MomoBako 核心。
fn log_record(
    index: u32,
    level: &str,
    category: &str,
    action: &str,
    message: &str,
) -> SystemLogRecord {
    SystemLogRecord {
        id: format!("log-{index}"),
        timestamp: format!("2026-10-08T07:{:02}:00Z", 50 + index),
        level: level.into(),
        category: category.into(),
        action: action.into(),
        message: message.into(),
        source: SystemLogSource {
            kind: "core".into(),
            label: Some("MomoBako".into()),
            plugin_id: None,
            repo_id: Some(REPO_ID.into()),
        },
        location: SystemLogLocation {
            module_path: Some("momobako::repository".into()),
            file: None,
            line: None,
        },
        context: json!({}),
    }
}
