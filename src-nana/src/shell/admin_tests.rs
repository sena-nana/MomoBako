//! 设置、插件、日志、任务和仓库动作的状态机测试。
//!
//! 这些分支来自 Vue 页面。不启动仓库服务，服务调用只检查留下的副作用。

use crate::backend::services::repository::{
    ApiDefinition, ApiDesignSnapshot, CacheConfig, CacheSnapshot, PluginConfigSnapshot, PluginDependencyStatus,
    PluginManifest, RepositoryAction, RepositoryActionStep, RepositoryBackendSummary, RepositorySummary,
    SystemLogLocation, SystemLogRecord, SystemLogSource, TaskProgressSnapshot,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;

use super::super::files::{FileDialog, FilesMessage};
use super::super::player::{PlayerCandidate, AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE};
use super::super::workspace::{WorkspacePanel, WorkspaceRepository};
use super::super::{ShellMessage, ShellPage, ShellViewModel};
use super::support::{self, action_can_run, audio_choices};
use super::tool_native::{self, ImportAction};
use super::{AdminEffect, AdminMessage, OperationProgress, SourceAuthCall, ToolPageEntry};

fn send(model: &mut ShellViewModel, message: AdminMessage) {
    model.reduce(ShellMessage::Admin(message));
}

fn plugin(id: &str, source: &str, category: &str, kind: &str) -> PluginManifest {
    PluginManifest {
        plugin_id: id.into(),
        package_format_version: None,
        package_hash: None,
        provenance: None,
        trust_level: None,
        deployment: None,
        target_triple: None,
        legacy_plugin_ids: Vec::new(),
        name: id.into(),
        version: "1.0.0".into(),
        r#type: None,
        kind: kind.into(),
        category: category.into(),
        description: String::new(),
        capabilities: vec!["search-me".into()],
        enabled: true,
        sdk: "backend".into(),
        entry: serde_json::Value::Null,
        contributes: serde_json::json!({
            "settings": {
                "fields": [
                    {"key": "limit", "label": "上限", "type": "number"},
                    {"key": "mode", "label": "模式", "type": "select", "options": [{"value": "a"}]},
                    {"key": "raw", "label": "原始", "type": "json"},
                    {"key": "on", "label": "开关", "type": "boolean"}
                ]
            }
        }),
        source: source.into(),
        runtime: "native-dylib".into(),
        permissions: Vec::new(),
        requires: Vec::new(),
        optional: Vec::new(),
        hooks: Vec::new(),
        compat: Default::default(),
        status: "ready".into(),
        dependency_status: PluginDependencyStatus::default(),
        disable_reason: None,
        degraded: false,
        degradation_reason: None,
        archive_path: None,
    }
}

fn log_record(id: &str, timestamp: &str, level: &str, kind: &str, plugin_id: &str, repo_id: &str, message: &str) -> SystemLogRecord {
    SystemLogRecord {
        id: id.into(),
        timestamp: timestamp.into(),
        level: level.into(),
        category: "plugin".into(),
        action: "run".into(),
        message: message.into(),
        source: SystemLogSource {
            kind: kind.into(),
            label: Some("宿主".into()),
            plugin_id: (!plugin_id.is_empty()).then(|| plugin_id.to_string()),
            repo_id: (!repo_id.is_empty()).then(|| repo_id.to_string()),
        },
        location: SystemLogLocation { module_path: Some("app".into()), file: Some("main.rs".into()), line: Some(12) },
        context: serde_json::json!({"token": "other"}),
    }
}

fn action(id: &str, status: &str, enabled: bool) -> RepositoryAction {
    RepositoryAction {
        action_id: id.into(),
        repo_id: "repo".into(),
        source: "builtin".into(),
        source_action_id: None,
        name: id.into(),
        status: status.into(),
        enabled,
        raw: serde_json::Value::Null,
        unsupported_reason: None,
        sort_order: 0,
        created_at: String::new(),
        updated_at: String::new(),
        steps: vec![RepositoryActionStep {
            step_id: "step".into(),
            action_id: id.into(),
            repo_id: "repo".into(),
            step_kind: "copy".into(),
            label: "复制".into(),
            status: status.into(),
            config: serde_json::Value::Null,
            raw: serde_json::Value::Null,
            unsupported_reason: None,
            sort_order: 0,
        }],
        last_run: None,
    }
}

fn connection() -> ExternalApiConnectionStatus {
    ExternalApiConnectionStatus {
        base_url: "http://127.0.0.1:9".into(),
        token: "1234567890abcdef".into(),
        version: "1".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        ready: true,
        connection_file_path: "external-api.json".into(),
    }
}

fn repository(id: &str, plugin_id: &str, name: &str, kind: &str) -> RepositorySummary {
    RepositorySummary {
        repo_id: id.into(),
        name: id.into(),
        path: format!("C:/{id}"),
        backend: RepositoryBackendSummary {
            plugin_id: plugin_id.into(),
            kind: kind.into(),
            name: name.into(),
            capabilities: Vec::new(),
        },
        status: "ready".into(),
        asset_count: 0,
        updated_at: String::new(),
        local_cache: None,
        authentication: None,
    }
}

fn candidate(plugin_id: &str, label: &str) -> PlayerCandidate {
    PlayerCandidate {
        plugin_id: plugin_id.into(),
        player_type_id: AUDIO_SEQUENCE_TYPE.into(),
        capability_id: Some(AUDIO_CAPABILITY.into()),
        label: label.into(),
        file_class: "audio".into(),
        extensions: vec!["mp3".into()],
        supports_seek: true,
        supports_volume: true,
    }
}

#[test]
fn plugins_group_search_and_confirm_delete() {
    let mut model = ShellViewModel::default();
    let mut user = plugin("user.one", "user", "source", "filesystem");
    user.name = "导入".into();
    let mut custom = plugin("custom.one", "system", "made-up", "service");
    custom.capabilities = vec!["only-custom".into()];
    let parser = plugin("parser.one", "", "parser", "parser");
    model.reduce(ShellMessage::PluginsLoaded(Ok(vec![user, custom, parser])));
    assert_eq!(model.page, ShellPage::PluginSettings);
    assert_eq!(model.detail, "3 个插件 · 原生贡献接口优先");
    assert_eq!(
        model.admin.grouped_plugins().into_iter().map(|(category, _)| category).collect::<Vec<_>>(),
        ["source", "parser", "unclassified"]
    );
    send(&mut model, AdminMessage::SetKeyword("only-custom".into()));
    assert_eq!(model.admin.grouped_plugins(), vec![("unclassified".into(), vec!["custom.one".into()])]);

    send(&mut model, AdminMessage::RequestDelete("parser.one".into()));
    assert!(model.admin.pending_delete.is_none());
    send(&mut model, AdminMessage::RequestDelete("user.one".into()));
    assert_eq!(model.admin.pending_delete.as_deref(), Some("user.one"));
    send(&mut model, AdminMessage::CancelDelete);
    assert!(model.admin.pending_delete.is_none());
    send(&mut model, AdminMessage::RequestDelete("user.one".into()));
    send(&mut model, AdminMessage::ConfirmDelete);
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeletePlugin(id)) if id == "user.one"));
    model.reduce(ShellMessage::Admin(AdminMessage::PluginsReplaced(Ok(vec![plugin("parser.one", "", "parser", "parser")]))));
    assert_eq!(model.admin.action_message, "插件已删除。");
    assert_eq!(model.page, ShellPage::PluginSettings);
}

#[test]
fn plugin_fields_reject_bad_json_and_reset_empty_numbers() {
    let mut model = ShellViewModel::default();
    let mut manifest = plugin("user.one", "user", "service", "service");
    manifest.contributes["settings"]["settingsPage"] = serde_json::json!({"label": "旧页面"});
    manifest.contributes["source"] = serde_json::json!({"authentication": {"kind": "oauth"}});
    model.admin.plugins = vec![manifest];
    send(&mut model, AdminMessage::MarkVueSettings("user.one".into()));
    let lines = support::settings_upgrade_lines(&model.admin.plugins[0], true);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("Vue"));
    assert!(lines.iter().any(|line| line.contains("账号与来源仍是 Vue 页面，需要升级为 Nana 原生设置")));
    let mut schema = plugin("schema.one", "system", "service", "service");
    schema.contributes["settings"]["settingsPage"] = serde_json::json!({"label": "本地文件系统"});
    assert!(support::settings_upgrade_lines(&schema, false).is_empty());
    schema.contributes["settings"].as_object_mut().expect("settings").remove("fields");
    assert!(support::settings_upgrade_lines(&schema, false).iter().any(|line| line.contains("设置字段")));

    send(&mut model, AdminMessage::JsonDraft { plugin_id: "user.one".into(), key: "raw".into(), value: "{".into() });
    send(&mut model, AdminMessage::SaveJson { plugin_id: "user.one".into(), key: "raw".into() });
    assert_eq!(model.admin.action_error, "原始 不是有效 JSON。");
    assert!(model.admin.take_effects().is_empty());

    send(&mut model, AdminMessage::ConfigInput { plugin_id: "user.one".into(), key: "limit".into(), text: "  ".into(), checked: None });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeleteConfig { key, .. }) if key == "limit"));
    send(&mut model, AdminMessage::ConfigInput { plugin_id: "user.one".into(), key: "on".into(), text: String::new(), checked: Some(true) });
    match model.admin.take_effects().pop() {
        Some(AdminEffect::SetConfig { value, .. }) => assert_eq!(value, serde_json::Value::Bool(true)),
        other => panic!("unexpected {other:?}"),
    }
    send(&mut model, AdminMessage::ConfigInput { plugin_id: "user.one".into(), key: "mode".into(), text: "\"a\"".into(), checked: None });
    match model.admin.take_effects().pop() {
        Some(AdminEffect::SetConfig { value, .. }) => assert_eq!(value, serde_json::json!("a")),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn plugin_settings_route_collapses_and_skips_loaded_pages() {
    let mut model = ShellViewModel::default();
    model.page = ShellPage::Settings;
    model.admin.plugins = vec![plugin("user.one", "user", "service", "service")];
    send(&mut model, AdminMessage::RoutePlugin("  ".into()));
    send(&mut model, AdminMessage::RoutePlugin("missing".into()));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::RoutePlugin("user.one".into()));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::LoadConfig(id)) if id == "user.one"));
    model.reduce(ShellMessage::PluginConfigLoaded(Ok(PluginConfigSnapshot {
        plugin_id: "user.one".into(),
        data_directory: "data".into(),
        schema: serde_json::Value::Null,
        values: [("raw".into(), serde_json::json!({"ok": true}))].into_iter().collect(),
    })));
    assert_eq!(model.page, ShellPage::Settings);
    assert!(model.admin.json_drafts["user.one"]["raw"].contains("ok"));
    send(&mut model, AdminMessage::RoutePlugin("user.one".into()));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::ToggleSettings("user.one".into()));
    assert!(model.admin.active_settings_plugin_id.is_none());
}

#[test]
fn toggle_and_install_keep_the_current_page() {
    let mut model = ShellViewModel::default();
    model.page = ShellPage::Settings;
    model.admin.plugins = vec![plugin("user.one", "user", "service", "service")];
    send(&mut model, AdminMessage::SetEnabled { plugin_id: "user.one".into(), enabled: false });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetEnabled { enabled: false, .. })));
    send(&mut model, AdminMessage::PluginsReplaced(Ok(vec![plugin("user.one", "user", "service", "service")])));
    assert_eq!(model.admin.action_message, "插件已禁用。");
    assert_eq!(model.page, ShellPage::Settings);
    send(&mut model, AdminMessage::InstallArchive(Some("  ".into())));
    send(&mut model, AdminMessage::ChooseArchive);
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::RequestOpenDialog)));
    assert!(support::open_dialog_available());
    assert_eq!(model.admin.action_message, "正在选择插件包…");
    send(&mut model, AdminMessage::InstallArchive(Some("plugin.momoplug".into())));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::Install(path)) if path == "plugin.momoplug"));
}

#[test]
fn logs_filter_sort_and_pause_without_dropping_records() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::LogsLoaded(Ok(crate::backend::services::repository::SystemLogPage {
        records: vec![
            log_record("b", "2020-02-01T00:00:00Z", "error", "helper", "plug", "repo", "失败"),
            log_record("a", "2020-01-01T00:00:00Z", "info", "host", "", "", "开始 needle"),
        ],
        next_cursor: None,
    })));
    assert_eq!(model.page, ShellPage::Logs);
    assert_eq!(model.log_entries[0], "error · plugin · 失败");
    assert_eq!(model.detail, "最近日志 · 2 条记录");
    assert!(model.admin.log_would_scroll);
    assert_eq!(model.admin.filtered_logs().iter().map(|record| record.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    send(&mut model, AdminMessage::ToggleLogLevel("error".into()));
    assert_eq!(model.admin.filtered_logs().len(), 1);
    assert_eq!(support::active_filter_count(&model.admin.log_levels, &model.admin.log_kinds, "", "", ""), 1);
    send(&mut model, AdminMessage::SetLogSearch("needle".into()));
    assert!(model.admin.filtered_logs().is_empty());
    send(&mut model, AdminMessage::ResetLogFilters);
    assert_eq!(support::active_filter_count(&model.admin.log_levels, &model.admin.log_kinds, "", "", ""), 0);
    send(&mut model, AdminMessage::SetLogPaused(true));
    assert!(!model.admin.log_would_scroll);
    send(&mut model, AdminMessage::SetLogSearch("不存在".into()));
    assert!(!model.admin.log_would_scroll);
    assert_eq!(model.admin.logs.len(), 2);
    let plugins = support::unique_sorted(model.admin.logs.iter().filter_map(|record| record.source.plugin_id.clone()));
    assert_eq!(plugins, ["plug"]);
}

#[test]
fn task_popover_merges_repository_operation_and_closes() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::TaskProgressLoaded(vec![TaskProgressSnapshot {
        task_id: "task-1".into(),
        protocol_id: "momobako.sync".into(),
        status: "running".into(),
        phase: Some("scanning".into()),
        label: Some("扫描".into()),
        current: None,
        total: None,
        percent: Some(10.0),
        error: None,
        updated_at: "20".into(),
    }]));
    send(&mut model, AdminMessage::SetOperation(Some(OperationProgress {
        label: "导入".into(),
        detail: "复制".into(),
        value: 8.0,
        indeterminate: false,
        updated_at_ms: 50,
    })));
    let rows = model.task_rows();
    assert_eq!(rows[0].id, "workspace-operation");
    assert_eq!(rows[0].source, "资源库");
    assert_eq!(rows[1].id, "task-1");
    send(&mut model, AdminMessage::ToggleTaskPopover);
    assert!(model.admin.popover_open);
    send(&mut model, AdminMessage::TaskOutside { inside: true });
    assert!(model.admin.popover_open);
    send(&mut model, AdminMessage::TaskOutside { inside: false });
    assert!(!model.admin.popover_open);
    send(&mut model, AdminMessage::ToggleTaskPopover);
    send(&mut model, AdminMessage::TaskEscape);
    assert!(!model.admin.popover_open);
    send(&mut model, AdminMessage::ToggleTaskPopover);
    send(&mut model, AdminMessage::TaskUnmount);
    assert!(!model.admin.popover_open);
    model.admin.operation = None;
    model.task_progress.clear();
    assert!(model.task_rows().is_empty());
}

#[test]
fn repository_actions_follow_selection_and_refuse_when_not_runnable() {
    let mut model = ShellViewModel::default();
    model.repository_id = Some("repo".into());
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    assert!(model.admin.actions_loading);
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::LoadActions { repo_id }) if repo_id == "repo"));
    model.repository_id = Some("other".into());
    send(&mut model, AdminMessage::ActionsLoaded { repo_id: "repo".into(), result: Ok(vec![action("import", "ready", true)]) });
    assert!(model.admin.actions.is_empty());
    model.repository_id = Some("repo".into());
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Files));
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Actions));
    let _ = model.admin.take_effects();
    send(&mut model, AdminMessage::ActionsLoaded {
        repo_id: "repo".into(),
        result: Ok(vec![action("import", "blocked", false), action("copy", "ready", true)]),
    });
    assert_eq!(model.admin.active_action_id.as_deref(), Some("import"));
    assert_eq!(support::action_status_label("blocked", false), "不支持");
    assert!(!action_can_run("ready", true, 0, false));
    send(&mut model, AdminMessage::RunAction(Some("copy".into())));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::ActionsLoaded { repo_id: "repo".into(), result: Err("读取失败".into()) });
    assert_eq!(model.admin.actions.len(), 2);
    assert_eq!(model.admin.actions_error, "读取失败");
    model.files.set_selected_paths(vec!["a.txt".into()]);
    send(&mut model, AdminMessage::SelectAction("copy".into()));
    assert_eq!(model.workspace.panel, WorkspacePanel::Actions);
    send(&mut model, AdminMessage::RunAction(None));
    assert!(matches!(
        model.admin.take_effects().pop(),
        Some(AdminEffect::RunAction { action_id, paths, .. }) if action_id == "copy" && paths == ["a.txt"]
    ));
    send(&mut model, AdminMessage::ActionRunFinished { result: Ok(action("copy", "ready", true)) });
    assert!(!model.admin.actions_running);
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::ReloadBrowser { repo_id }) if repo_id == "repo"));
}

#[test]
fn tool_pages_fall_back_and_do_not_mount_vue_components() {
    let mut model = ShellViewModel::default();
    send(&mut model, AdminMessage::SetToolPages(Vec::new()));
    assert!(model.admin.active_tool_page_id.is_none());
    send(&mut model, AdminMessage::SetToolPages(vec![
        ToolPageEntry { id: "old".into(), label: "旧工具".into(), plugin_name: "vue".into(), description: String::new(), native: false },
        ToolPageEntry { id: "new".into(), label: "新工具".into(), plugin_name: "native".into(), description: String::new(), native: true },
    ]));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("old"));
    assert_eq!(support::tool_page_upgrade(&model.admin.tool_pages[0]), Some("工具页仍是 Vue 插件，需要升级为 Nana 原生工具页"));
    send(&mut model, AdminMessage::SelectToolPage("new".into()));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("new"));
    assert!(support::tool_page_upgrade(&model.admin.tool_pages[1]).is_none());
    send(&mut model, AdminMessage::SetToolPages(vec![ToolPageEntry {
        id: "new".into(), label: "新工具".into(), plugin_name: "native".into(), description: String::new(), native: true,
    }]));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("new"));
}

#[test]
fn settings_audio_corner_external_api_and_backends() {
    let mut model = ShellViewModel::default();
    model.player.candidates = vec![candidate("momobako.player.audio", "官方音频")];
    send(&mut model, AdminMessage::SetAudioPlayer(Some(" momobako.player.audio ".into())));
    assert_eq!(model.player.preferences.get(AUDIO_CAPABILITY).map(String::as_str), Some("momobako.player.audio"));
    let (choices, selected, notice) = audio_choices(&model.player.candidates, &model.player.preferences);
    assert_eq!(selected, "momobako.player.audio");
    assert!(notice.is_empty());
    assert!(support::audio_picker_enabled(&choices));
    model.player.preferences.insert(AUDIO_CAPABILITY.into(), "missing.player".into());
    let (choices, _, notice) = audio_choices(&model.player.candidates, &model.player.preferences);
    assert!(choices.iter().any(|choice| choice.unavailable && choice.label.contains("不可用")));
    assert!(notice.contains("已回退到"));
    assert!(support::clipboard_available());
    assert!(support::save_dialog_available());

    send(&mut model, AdminMessage::SetCornerStyle("square".into()));
    assert_eq!(model.admin.corner_style, support::default_corner_style());
    send(&mut model, AdminMessage::SetCornerRadius("100".into()));
    assert_eq!(model.admin.corner_radius, 20.0);
    send(&mut model, AdminMessage::SetCornerRadius("nope".into()));
    assert_eq!(model.admin.corner_radius, 20.0);
    assert!(matches!(model.admin.take_effects().last(), Some(AdminEffect::PersistCorners)));

    let directory = std::env::temp_dir().join(format!("momobako-corners-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&directory);
    let path = directory.join("corners.json");
    support::write_corners(&path, "round", 8.0);
    model.admin.load_corners_file(&path);
    assert_eq!(model.admin.corner_style, "round");
    assert_eq!(model.admin.corner_radius, 8.0);
    let _ = std::fs::remove_dir_all(&directory);

    model.reduce(ShellMessage::RepositoriesLoaded(Ok(vec![
        repository("one", "filesystem", "本地", "local"),
        repository("two", "filesystem", "本地", "local"),
    ])));
    assert_eq!(support::backend_summary(&model.admin.backends), "本地 (2)");
    model.reduce(ShellMessage::SystemStatusLoaded(Ok(connection())));
    assert_eq!(model.detail, "服务已就绪 · http://127.0.0.1:9");
    assert_eq!(support::mask_token(Some("1234567890abcdef")), "1234567890...abcdef");
    assert_eq!(support::external_status_label(None), "未加载");
    send(&mut model, AdminMessage::CopyExternal { label: "Base URL".into(), value: String::new() });
    assert_eq!(model.admin.external_error, "Base URL 尚未加载。");
    send(&mut model, AdminMessage::CopyExternal { label: "Token".into(), value: "1234567890abcdef".into() });
    assert_eq!(model.admin.external_message, "Token 已复制。");
    assert!(model.admin.external_error.is_empty());
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::CopyText(_))));
    send(&mut model, AdminMessage::ExportExternal);
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::RequestSaveDialog { .. })));
    assert!(model.admin.external_error.is_empty());
    assert_eq!(model.admin.external_message, "正在选择导出位置…");
    send(&mut model, AdminMessage::CompleteExport(None));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::CompleteExport(Some("external-api.json".into())));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::WriteFile { path, .. }) if path == "external-api.json"));
    send(&mut model, AdminMessage::WriteFinished(Ok(())));
    assert_eq!(model.admin.external_message, "external-api.json 已导出。");
    assert_eq!(model.repository_id.as_deref(), Some("one"));
    send(&mut model, AdminMessage::SelectRepository("  ".into()));
    assert_eq!(model.repository_id.as_deref(), Some("one"));
}

fn tool_page(id: &str, native: bool) -> ToolPageEntry {
    ToolPageEntry { id: id.into(), label: id.into(), plugin_name: "tool".into(), description: String::new(), native }
}

fn attach_repository(model: &mut ShellViewModel, capabilities: Vec<String>) {
    model.workspace.active_repo_id = Some("repo".into());
    model.workspace.repositories = vec![WorkspaceRepository {
        repo_id: "repo".into(),
        name: "资料库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities,
        cache_required: false,
        cache_status: String::new(),
    }];
}

fn import_action<'a>(actions: &'a [ImportAction], id: &str) -> &'a ImportAction {
    actions.iter().find(|item| item.id == id).unwrap_or_else(|| panic!("missing {id}"))
}

#[test]
fn builtin_tool_pages_follow_the_file_machine_and_api_snapshot() {
    for id in [support::TOOL_FILE_MANAGER, support::TOOL_EAGLE_IMPORTER, support::TOOL_API_PLAYGROUND] {
        assert!(support::tool_page_upgrade(&tool_page(id, false)).is_none());
    }
    assert_eq!(
        support::tool_page_upgrade(&tool_page("momobako.tool.other", false)),
        Some("工具页仍是 Vue 插件，需要升级为 Nana 原生工具页")
    );

    let mut model = ShellViewModel::default();
    let blocked = tool_native::import_actions(&model, support::TOOL_FILE_MANAGER);
    let folder = import_action(&blocked, "folder");
    assert!(!folder.enabled);
    assert_eq!(tool_native::import_block_reason(&model), Some("当前没有可用仓库。"));
    assert!(tool_native::import_message(folder.enabled, folder.message.clone()).is_none());
    model.reduce(ShellMessage::Files(folder.message.clone()));
    assert!(model.files.take_effects().is_empty());
    assert!(matches!(model.files.dialog, FileDialog::Closed));

    attach_repository(&mut model, vec!["write".into()]);
    let open = tool_native::import_actions(&model, support::TOOL_FILE_MANAGER);
    let folder = import_action(&open, "folder");
    assert!(folder.enabled);
    assert!(tool_native::import_block_reason(&model).is_none());
    match &folder.message {
        FilesMessage::OpenDialog(FileDialog::Import) => {}
        other => panic!("unexpected {other:?}"),
    }
    model.reduce(tool_native::import_message(folder.enabled, folder.message.clone()).expect("folder"));
    assert!(matches!(model.files.dialog, FileDialog::Import));
    assert!(model.files.take_effects().is_empty());

    let eagle = tool_native::import_actions(&model, support::TOOL_EAGLE_IMPORTER);
    let copy = import_action(&eagle, "copy");
    match &copy.message {
        FilesMessage::OpenEagle(mode) => assert_eq!(mode, "copy"),
        other => panic!("unexpected {other:?}"),
    }
    model.reduce(tool_native::import_message(copy.enabled, copy.message.clone()).expect("copy"));
    assert_eq!(model.files.eagle_mode, "copy");
    assert!(matches!(model.files.dialog, FileDialog::ImportEagle));

    assert_eq!(tool_native::api_lines(None), vec!["还没有 API 设计快照".to_string()]);
    assert_eq!(
        tool_native::api_lines(Some(&ApiDesignSnapshot { transport: "local".into(), endpoints: Vec::new() })),
        vec!["还没有 API 设计快照".to_string()]
    );
    let lines = tool_native::api_lines(Some(&ApiDesignSnapshot {
        transport: "local".into(),
        endpoints: vec![ApiDefinition {
            group: "system".into(),
            transport: "local".into(),
            method: "GET".into(),
            path: "/health".into(),
            summary: "健康".into(),
            command: None,
            plugin_id: None,
            plugin_method: None,
            requires_auth: None,
            request_template: None,
        }],
    }));
    assert_eq!(lines, vec!["GET /health · 健康".to_string()]);

    model.workspace.repositories[0].capabilities.clear();
    assert_eq!(tool_native::import_block_reason(&model), Some("当前仓库处于只读状态。"));
    model.workspace.repositories[0].capabilities = vec!["write".into()];
    model.workspace.panel = WorkspacePanel::Trash;
    assert_eq!(tool_native::import_block_reason(&model), Some("回收站视图不支持导入。"));
    model.workspace.panel = WorkspacePanel::SmartFolder;
    assert_eq!(tool_native::import_block_reason(&model), Some("虚拟视图不支持导入。"));
}

#[test]
fn source_account_summary_names_methods_without_the_upgrade_copy() {
    let mut manifest = plugin("source.one", "system", "source", "filesystem");
    manifest.contributes["source"] = serde_json::json!({
        "authentication": {
            "kind": "qr",
            "createSessionMethod": "auth.createQrSession",
            "statusMethod": "auth.getLoginStatus"
        }
    });
    let lines = support::settings_upgrade_lines(&manifest, false);
    let joined = lines.join("\n");
    assert!(!joined.contains("需要升级为 Nana 原生设置"));
    assert!(joined.contains("qr"));
    assert!(joined.contains("auth.createQrSession"));
    assert!(joined.contains("auth.getLoginStatus"));

    manifest.contributes["source"] = serde_json::json!({"authentication": {}});
    assert!(support::settings_upgrade_lines(&manifest, false).iter().any(|line| line.contains("账号与来源仍是 Vue 页面，需要升级为 Nana 原生设置")));
}

#[test]
fn source_login_reduce_calls_create_session_and_skips_oauth_without_methods() {
    let mut model = ShellViewModel::default();
    let mut manifest = plugin("source.one", "system", "source", "filesystem");
    manifest.contributes["source"] = serde_json::json!({
        "authentication": {
            "kind": "qr",
            "createSessionMethod": "auth.createQrSession",
            "statusMethod": "auth.getLoginStatus",
            "clearMethod": "auth.clearLogin"
        }
    });
    let summary = support::settings_upgrade_lines(&manifest, false).join("\n");
    assert!(summary.contains("来源账号 qr：创建会话 auth.createQrSession，查询状态 auth.getLoginStatus。"));
    assert!(!summary.contains("需要升级为 Nana 原生设置"));
    assert_eq!(
        tool_native::source_auth_actions(&manifest).into_iter().map(|action| action.label).collect::<Vec<_>>(),
        vec!["创建登录会话", "查询登录状态", "退出登录"]
    );
    model.admin.plugins = vec![manifest];
    send(&mut model, AdminMessage::CallSourceAuth { plugin_id: "source.one".into(), slot: SourceAuthCall::CreateSession });
    match model.admin.take_effects().pop() {
        Some(AdminEffect::CallPlugin { plugin_id, method, payload }) => {
            assert_eq!(plugin_id, "source.one");
            assert_eq!(method, "auth.createQrSession");
            assert_eq!(payload, serde_json::json!({}));
        }
        other => panic!("unexpected {other:?}"),
    }
    send(&mut model, AdminMessage::CallSourceAuth { plugin_id: "  ".into(), slot: SourceAuthCall::CreateSession });
    assert!(model.admin.take_effects().is_empty());

    send(&mut model, AdminMessage::SourceAuthFinished {
        method: "auth.createQrSession".into(),
        result: Ok(serde_json::json!({"message": "请扫码", "status": "waiting", "qrImage": "data:image/png;base64,abc"})),
    });
    assert_eq!(model.admin.action_message, "已调用 auth.createQrSession。请扫码 waiting");
    assert!(!model.admin.action_message.contains("qrImage"));
    assert!(!model.admin.action_message.contains("base64"));
    send(&mut model, AdminMessage::SourceAuthFinished {
        method: "auth.getLoginStatus".into(),
        result: Ok(serde_json::json!({"qrimg": "x", "message": 1})),
    });
    assert_eq!(model.admin.action_message, "已调用 auth.getLoginStatus。");

    let mut oauth = plugin("oauth.one", "system", "source", "filesystem");
    oauth.contributes["source"] = serde_json::json!({"authentication": {"kind": "oauth"}});
    assert!(support::settings_upgrade_lines(&oauth, false).iter().any(|line| line.contains("账号与来源仍是 Vue 页面，需要升级为 Nana 原生设置")));
    assert!(tool_native::source_auth_actions(&oauth).is_empty());
    model.admin.plugins = vec![oauth];
    send(&mut model, AdminMessage::CallSourceAuth { plugin_id: "oauth.one".into(), slot: SourceAuthCall::CreateSession });
    assert!(model.admin.take_effects().is_empty());
}

#[test]
fn acceptance_scenes_do_not_show_live_admin_surfaces() {
    assert!(!ShellViewModel::for_page(ShellPage::Settings).admin_settings_visible());
    assert!(!ShellViewModel::for_page(ShellPage::Logs).admin_workspace_visible(WorkspacePanel::Logs));
    assert!(!ShellViewModel::for_page(ShellPage::PluginSettings).admin_settings_visible());
}

#[test]
fn settings_bundle_keeps_previous_data_when_one_request_fails() {
    let mut model = ShellViewModel::default();
    model.admin.plugins = vec![plugin("keep", "system", "service", "service")];
    send(&mut model, AdminMessage::SettingsBundleLoaded {
        plugins: Ok(vec![plugin("next", "system", "service", "service")]),
        hooks: Err("钩子失败".into()),
        cache: Ok(CacheSnapshot { config: CacheConfig { metadata_capacity: 1, thumbnail_capacity: 2, query_capacity: 3 }, entries: Vec::new() }),
        api: Ok(ApiDesignSnapshot { transport: "local".into(), endpoints: vec![ApiDefinition {
            group: "system".into(), transport: "local".into(), method: "GET".into(), path: "/health".into(), summary: "健康".into(),
            command: None, plugin_id: None, plugin_method: None, requires_auth: None, request_template: None,
        }] }),
    });
    assert_eq!(model.admin.plugins[0].plugin_id, "keep");
    assert!(model.admin.cache.is_none());
    assert_eq!(model.admin.action_error, "钩子失败");
}

#[test]
fn dependency_label_uses_status_counts_and_directory_failure_is_visible() {
    let mut manifest = plugin("user.one", "user", "service", "service");
    manifest.requires = vec!["a".into(), "b".into()];
    manifest.dependency_status.required = vec![];
    assert_eq!(support::dependency_label(&manifest), "必需 2 / 可选 0");
    assert_eq!(support::dependency_status_label("missing"), "缺失");
    assert_eq!(support::plugin_status_label(&manifest), "已启用");
    manifest.enabled = false;
    assert_eq!(support::plugin_status_label(&manifest), "未启用");
    let mut model = ShellViewModel::default();
    model.admin.plugins = vec![manifest];
    send(&mut model, AdminMessage::OpenDataDirectory("user.one".into()));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::OpenDataDirectory { name, .. }) if name == "user.one"));
    send(&mut model, AdminMessage::DataDirectoryFinished { name: "user.one".into(), result: Ok("C:/plugin".into()) });
    assert_eq!(model.admin.action_message, "已打开“user.one”设置目录。");
    assert!(model.admin.action_error.is_empty());
    assert!(support::reveal_directory_available());
}
