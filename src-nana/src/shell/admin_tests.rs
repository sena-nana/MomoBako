//! 设置、插件、日志、任务、仓库动作和工具页的状态机测试。
//!
//! 期望值来自对应的 Vue 组件。不启动仓库服务，服务调用只检查留下的副作用。

use std::collections::BTreeMap;

use serde_json::json;

use crate::backend::services::repository::{
    ApiDesignSnapshot, CacheConfig, CacheSnapshot, PluginConfigSnapshot, PluginDependencyStatus, PluginManifest, RepositoryAction,
    RepositoryActionStep, RepositoryBackendSummary, RepositorySummary, SystemLogLocation, SystemLogPage, SystemLogRecord,
    SystemLogSource, TaskProgressSnapshot,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;

use super::super::files::{FileDialog, FilesMessage};
use super::super::player::{PlayerCandidate, AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE};
use super::super::workspace::{WorkspacePanel, WorkspaceRepository};
use super::super::{ShellMessage, ShellViewModel};
use super::api::{ApiMessage, HttpResponse};
use super::support::{self, action_can_run};
use super::tool_native::{self, ImportAction};
use super::{AdminEffect, AdminMessage, OperationProgress, PluginCallOrigin, ToolPageEntry};

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
        contributes: json!({
            "settings": {
                "fields": [
                    {"key": "limit", "label": "上限", "type": "number"},
                    {"key": "mode", "label": "模式", "type": "select", "options": [{"label": "甲", "value": "a"}]},
                    {"key": "raw", "label": "原始", "type": "json"},
                    {"key": "on", "label": "开关", "type": "boolean"},
                    {"key": "", "label": "缺键"}
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

/// 能加载前端模块并声明工具页的插件。
fn tool_plugin(id: &str, pages: serde_json::Value) -> PluginManifest {
    let mut manifest = plugin(id, "builtin", "service", "tool");
    manifest.sdk = "frontend".into();
    manifest.runtime = "vue-module".into();
    manifest.entry = json!({ "frontend": { "module": "dist/register.js" } });
    manifest.contributes = json!({ "toolPages": pages });
    manifest
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
        context: json!({"token": "other"}),
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
        base_url: "http://127.0.0.1:9/external/v1".into(),
        token: "1234567890abcdef".into(),
        version: "1".into(),
        started_at: "2026-01-01T00:00:00Z".into(),
        ready: true,
        connection_file_path: "external-api.json".into(),
    }
}

fn repository(id: &str, plugin_id: &str, name: &str) -> RepositorySummary {
    RepositorySummary {
        repo_id: id.into(),
        name: id.into(),
        path: format!("C:/{id}"),
        backend: RepositoryBackendSummary { plugin_id: plugin_id.into(), kind: "local".into(), name: name.into(), capabilities: Vec::new() },
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

fn cache() -> CacheSnapshot {
    CacheSnapshot { config: CacheConfig { metadata_capacity: 1, thumbnail_capacity: 2, query_capacity: 3 }, entries: Vec::new() }
}

fn api_design() -> ApiDesignSnapshot {
    ApiDesignSnapshot { transport: "tauri-ipc".into(), endpoints: Vec::new() }
}

/// 一次成功的设置页数据。
fn load_bundle(model: &mut ShellViewModel, plugins: Vec<PluginManifest>) {
    send(model, AdminMessage::SettingsBundleLoaded {
        plugins: Ok(plugins),
        hooks: Ok(Vec::new()),
        cache: Ok(cache()),
        api: Ok(api_design()),
        external: Ok(connection()),
    });
}

#[test]
fn settings_bundle_is_all_or_nothing_and_success_clears_the_error() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::Navigate(super::super::ShellPage::Settings));
    assert!(model.admin.loading_settings);
    assert!(model.admin.take_effects().iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)));
    send(&mut model, AdminMessage::SettingsBundleLoaded {
        plugins: Ok(vec![plugin("next", "system", "service", "service")]),
        hooks: Ok(Vec::new()),
        cache: Ok(cache()),
        api: Err("API 设计读取失败".into()),
        external: Ok(connection()),
    });
    assert!(!model.admin.loading_settings);
    assert!(model.admin.plugins.is_empty());
    assert!(model.admin.cache.is_none());
    assert!(model.admin.external.is_none());
    assert_eq!(model.admin.load_error, "API 设计读取失败");

    let tools = tool_plugin("momobako.tool.file-manager", json!([{ "toolPageId": "momobako.tool.file-manager", "label": "文件导入" }]));
    load_bundle(&mut model, vec![plugin("next", "system", "service", "service"), tools]);
    assert_eq!(model.admin.plugins.len(), 2);
    assert_eq!(model.admin.cache.as_ref().map(|item| item.config.query_capacity), Some(3));
    assert_eq!(model.admin.external.as_ref().map(|item| item.ready), Some(true));
    assert!(model.admin.load_error.is_empty());
    assert_eq!(model.admin.tool_pages.iter().map(|page| page.id.as_str()).collect::<Vec<_>>(), ["momobako.tool.file-manager"]);
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("momobako.tool.file-manager"));
}

#[test]
fn plugins_group_search_and_confirm_delete() {
    let mut model = ShellViewModel::default();
    let mut user = plugin("user.one", "user", "source", "filesystem");
    user.name = "导入".into();
    let mut custom = plugin("custom.one", "system", "made-up", "service");
    custom.capabilities = vec!["only-custom".into()];
    let parser = plugin("parser.one", "", "", "parser");
    load_bundle(&mut model, vec![user, custom, parser]);
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
    assert!(model.admin.managing);
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeletePlugin(id)) if id == "user.one"));
    send(&mut model, AdminMessage::PluginsReplaced(Err("删除被拒绝".into())));
    assert_eq!(model.admin.pending_delete.as_deref(), Some("user.one"));
    assert_eq!(model.admin.action_error, "删除被拒绝");
    send(&mut model, AdminMessage::ConfirmDelete);
    send(&mut model, AdminMessage::PluginsReplaced(Ok(vec![plugin("parser.one", "", "parser", "parser")])));
    assert_eq!(model.admin.action_message, "插件已删除。");
    assert!(model.admin.action_error.is_empty());
    assert!(model.admin.pending_delete.is_none());
    assert!(!model.admin.managing);
}

#[test]
fn plugin_labels_follow_the_vue_taxonomy() {
    let mut manifest = plugin("user.one", "user", "", "webdav");
    assert_eq!(support::plugin_category(&manifest), "source");
    assert_eq!(support::category_label("library-kind"), "库类型");
    assert_eq!(support::category_label("made-up"), "未分类");
    assert_eq!(support::plugin_runtime_label("process"), "未知运行时");
    assert_eq!(support::plugin_source_label("user"), "用户插件");
    manifest.requires = vec!["a".into(), "b".into()];
    assert_eq!(support::dependency_label(&manifest), "必需 2 / 可选 0");
    assert_eq!(support::dependency_status_label("missing"), "缺失");
    assert_eq!(support::hook_status_label("blocked"), "已拦截");
    assert_eq!(support::plugin_status_label(&manifest), "已启用");
    manifest.degraded = true;
    assert_eq!(support::plugin_status_label(&manifest), "降级运行");
    manifest.enabled = false;
    assert_eq!(support::plugin_status_label(&manifest), "未启用");
    manifest.status = "error".into();
    assert_eq!(support::plugin_status_label(&manifest), "错误");
    assert_eq!(support::settings_fields(&manifest).len(), 4);
    assert_eq!(support::plugin_settings_label(&manifest), "插件设置");
    manifest.contributes["settings"]["settingsPage"] = json!({ "label": "下载服务", "description": "队列和目录" });
    assert_eq!(support::plugin_settings_label(&manifest), "下载服务");
    assert_eq!(support::plugin_settings_description(&manifest), "队列和目录");
    manifest.contributes = json!({ "source": { "authentication": { "kind": "qr" } } });
    assert_eq!(support::plugin_settings_label(&manifest), "账号与来源");
    assert_eq!(support::plugin_settings_description(&manifest), "管理来源账号、登录状态与关联仓库。");
}

#[test]
fn plugin_fields_reject_bad_json_and_reset_empty_numbers() {
    let mut model = ShellViewModel::default();
    load_bundle(&mut model, vec![plugin("user.one", "user", "service", "service")]);
    send(&mut model, AdminMessage::JsonDraft { plugin_id: "user.one".into(), key: "raw".into(), value: "{".into() });
    send(&mut model, AdminMessage::SaveJson { plugin_id: "user.one".into(), key: "raw".into() });
    assert_eq!(model.admin.action_error, "原始 不是有效 JSON。");
    assert!(model.admin.take_effects().is_empty());

    send(&mut model, AdminMessage::ConfigInput { plugin_id: "user.one".into(), key: "limit".into(), text: "  ".into(), checked: None });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeleteConfig { key, .. }) if key == "limit"));
    send(&mut model, AdminMessage::ConfigInput { plugin_id: "user.one".into(), key: "limit".into(), text: "12".into(), checked: None });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetConfig { value, .. }) if value == json!(12.0)));
    send(&mut model, AdminMessage::ConfigInput { plugin_id: "user.one".into(), key: "on".into(), text: String::new(), checked: Some(true) });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetConfig { value, .. }) if value == json!(true)));
    send(&mut model, AdminMessage::ConfigInput { plugin_id: "user.one".into(), key: "mode".into(), text: "\"a\"".into(), checked: None });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetConfig { value, .. }) if value == json!("a")));
    model.reduce(ShellMessage::PluginConfigLoaded(Ok(PluginConfigSnapshot {
        plugin_id: "user.one".into(),
        data_directory: "data".into(),
        schema: serde_json::Value::Null,
        values: [("mode".into(), json!("a"))].into_iter().collect(),
    })));
    assert_eq!(model.admin.action_message, "插件设置已保存。");
    send(&mut model, AdminMessage::ResetConfig { plugin_id: "user.one".into(), key: "mode".into() });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::DeleteConfig { key, .. }) if key == "mode"));
    model.reduce(ShellMessage::PluginConfigLoaded(Err(String::new())));
    assert_eq!(model.admin.action_error, "插件设置重置失败。");
}

#[test]
fn plugin_settings_toggle_route_and_directory() {
    let mut model = ShellViewModel::default();
    load_bundle(&mut model, vec![plugin("user.one", "user", "service", "service")]);
    send(&mut model, AdminMessage::RoutePlugin("  ".into()));
    send(&mut model, AdminMessage::RoutePlugin("missing".into()));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::RoutePlugin("user.one".into()));
    assert_eq!(model.admin.active_settings_plugin_id.as_deref(), Some("user.one"));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::LoadConfig(id)) if id == "user.one"));
    model.reduce(ShellMessage::PluginConfigLoaded(Ok(PluginConfigSnapshot {
        plugin_id: "user.one".into(),
        data_directory: "data".into(),
        schema: serde_json::Value::Null,
        values: [("raw".into(), json!({"ok": true}))].into_iter().collect(),
    })));
    assert!(model.admin.json_drafts["user.one"]["raw"].contains("ok"));
    assert!(!model.admin.managing);
    send(&mut model, AdminMessage::RoutePlugin("user.one".into()));
    assert!(model.admin.take_effects().is_empty());

    send(&mut model, AdminMessage::OpenDataDirectory("user.one".into()));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::OpenDataDirectory { name, .. }) if name == "user.one"));
    send(&mut model, AdminMessage::DataDirectoryFinished { name: "user.one".into(), result: Ok("C:/plugin".into()) });
    assert_eq!(model.admin.action_message, "已打开“user.one”设置目录。");
    send(&mut model, AdminMessage::DataDirectoryFinished { name: "user.one".into(), result: Err(String::new()) });
    assert_eq!(model.admin.action_error, "插件设置目录打开失败。");

    send(&mut model, AdminMessage::ToggleSettings("user.one".into()));
    assert!(model.admin.active_settings_plugin_id.is_none());
}

#[test]
fn enable_and_install_report_vue_messages() {
    let mut model = ShellViewModel::default();
    load_bundle(&mut model, vec![plugin("user.one", "user", "service", "service")]);
    send(&mut model, AdminMessage::SetEnabled { plugin_id: "user.one".into(), enabled: false });
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::SetEnabled { enabled: false, .. })));
    send(&mut model, AdminMessage::PluginsReplaced(Ok(vec![plugin("user.one", "user", "service", "service")])));
    assert_eq!(model.admin.action_message, "插件已禁用。");
    send(&mut model, AdminMessage::ChooseArchive);
    assert!(model.admin.action_message.is_empty());
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::RequestOpenDialog)));
    send(&mut model, AdminMessage::InstallArchive(Some("  ".into())));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::InstallArchive(Some("plugin.momoplug".into())));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::Install(path)) if path == "plugin.momoplug"));
    send(&mut model, AdminMessage::PluginsReplaced(Err(String::new())));
    assert_eq!(model.admin.action_error, "插件安装失败。");
}

#[test]
fn theme_corners_and_external_connection() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::SettingsThemeChanged("sepia".into()));
    assert!(model.admin.take_effects().is_empty());
    model.reduce(ShellMessage::SettingsThemeChanged("dark".into()));
    assert_eq!(model.settings.theme, "dark");
    assert!(matches!(model.admin.take_effects().as_slice(), [AdminEffect::SaveSettings]));

    send(&mut model, AdminMessage::SetCornerStyle("square".into()));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::SetCornerStyle("round".into()));
    assert_eq!(model.admin.corner_style, "round");
    send(&mut model, AdminMessage::SetCornerRadius("100".into()));
    assert_eq!(model.admin.corner_radius, 20.0);
    send(&mut model, AdminMessage::SetCornerRadius("nope".into()));
    assert_eq!(model.admin.corner_radius, 20.0);
    assert!(model.admin.take_effects().iter().all(|effect| matches!(effect, AdminEffect::PersistCorners)));

    let directory = std::env::temp_dir().join(format!("momobako-corners-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("临时目录");
    let path = directory.join("corners.json");
    support::write_corners(&path, "round", 8.0);
    model.admin.load_corners_file(&path);
    assert_eq!((model.admin.corner_style.as_str(), model.admin.corner_radius), ("round", 8.0));
    let _ = std::fs::remove_dir_all(&directory);

    assert_eq!(support::external_status_label(None), "未加载");
    assert_eq!(support::mask_token(Some("1234567890abcdef")), "1234567890...abcdef");
    send(&mut model, AdminMessage::ExportExternal);
    assert_eq!(model.admin.external_error, "连接 JSON 尚未加载。");
    send(&mut model, AdminMessage::CopyExternal { label: "Base URL".into(), value: String::new() });
    assert_eq!(model.admin.external_error, "Base URL 尚未加载。");
    model.admin.external = Some(connection());
    send(&mut model, AdminMessage::CopyExternal { label: "Token".into(), value: "1234567890abcdef".into() });
    assert_eq!(model.admin.external_message, "Token 已复制。");
    assert!(model.admin.external_error.is_empty());
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::CopyText(text)) if text == "1234567890abcdef"));
    send(&mut model, AdminMessage::ExportExternal);
    match model.admin.take_effects().pop() {
        Some(AdminEffect::RequestSaveDialog { content }) => {
            let parsed: serde_json::Value = serde_json::from_str(&content).expect("连接 JSON");
            assert_eq!(parsed["token"], "1234567890abcdef");
            assert!(parsed.get("connectionFilePath").is_none());
        }
        other => panic!("unexpected {other:?}"),
    }
    send(&mut model, AdminMessage::CompleteExport(None));
    assert!(model.admin.take_effects().is_empty());
    send(&mut model, AdminMessage::CompleteExport(Some("external-api.json".into())));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::WriteFile { path, .. }) if path == "external-api.json"));
    send(&mut model, AdminMessage::WriteFinished(Ok(())));
    assert_eq!(model.admin.external_message, "external-api.json 已导出。");
    send(&mut model, AdminMessage::WriteFinished(Err("磁盘已满".into())));
    assert_eq!(model.admin.external_error, "导出失败：磁盘已满");
}

#[test]
fn repository_backends_and_audio_player_choices() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::RepositoriesLoaded(Ok(vec![
        repository("one", "filesystem", "本地"),
        repository("two", "filesystem", "本地"),
        repository("three", "webdav", "WebDAV"),
    ])));
    assert_eq!(support::backend_summary(&model.admin.backends), "本地 (2) / WebDAV (1)");
    assert_eq!(support::backend_summary(&[]), "无");

    let names = |plugin_id: &str| (plugin_id == "momobako.player.audio").then(|| "官方音频播放器".to_string());
    let mut preferences = BTreeMap::new();
    let view = support::audio_view(&[candidate("momobako.player.audio", "音频顺序播放")], &preferences, &names);
    assert_eq!(view.choices.iter().map(|choice| choice.label.as_str()).collect::<Vec<_>>(), ["官方音频播放器 · momobako.player.audio"]);
    assert_eq!(view.selected, "momobako.player.audio");
    assert!(view.notice.is_none());
    preferences.insert(AUDIO_CAPABILITY.to_string(), "missing.player".to_string());
    let view = support::audio_view(&[candidate("momobako.player.audio", "音频顺序播放")], &preferences, &names);
    assert_eq!(view.choices[0].label, "missing.player（不可用）");
    assert_eq!(view.notice, Some(("所选播放器当前不可用，已回退到 官方音频播放器。".into(), false)));
    let view = support::audio_view(&[], &BTreeMap::new(), &names);
    assert!(view.choices.is_empty());
    assert_eq!(view.notice, Some(("官方音频播放器未启用或缺失，音频播放暂不可用。".into(), true)));

    model.player.candidates = vec![candidate("momobako.player.audio", "音频顺序播放")];
    send(&mut model, AdminMessage::SetAudioPlayer(Some(" momobako.player.audio ".into())));
    assert_eq!(model.player.preferences.get(AUDIO_CAPABILITY).map(String::as_str), Some("momobako.player.audio"));
}

#[test]
fn logs_filter_sort_pause_and_context() {
    let mut model = ShellViewModel::default();
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records: vec![
            log_record("b", "2020-02-01T00:00:00Z", "error", "helper", "plug", "repo", "失败"),
            log_record("a", "2020-01-01T00:00:00Z", "info", "host", "", "", "开始 needle"),
        ],
        next_cursor: None,
    })));
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
    assert_eq!(support::unique_sorted(model.admin.logs.iter().filter_map(|record| record.source.plugin_id.clone())), ["plug"]);
    send(&mut model, AdminMessage::ToggleLogContext("a".into()));
    assert!(model.admin.log_context_open.contains("a"));
    send(&mut model, AdminMessage::ToggleLogContext("a".into()));
    assert!(model.admin.log_context_open.is_empty());
    assert_eq!(support::level_label("warn"), "警告");
    assert_eq!(support::source_kind_label("frontend-plugin"), "前端插件");
    assert_eq!(support::location_label(&model.admin.logs[0]), "app:main.rs:12");

    for index in 0..502 {
        model.admin.merge_log(log_record(&format!("n{index:03}"), &format!("2021-01-01T00:{:02}:{:02}Z", index / 60, index % 60), "info", "host", "", "", "x"));
    }
    assert_eq!(model.admin.logs.len(), 500);
    assert_eq!(model.admin.logs[0].id, "n501");
}

#[test]
fn logs_follow_the_end_while_tracking_and_hold_when_paused() {
    let mut model = ShellViewModel::for_page(super::super::ShellPage::Logs);
    assert!(model.admin_logs_follow_end());
    send(&mut model, AdminMessage::SetLogPaused(true));
    assert!(!model.admin_logs_follow_end());
    model.admin.merge_log(log_record("late", "2026-10-08T08:00:00Z", "info", "host", "", "", "新记录"));
    assert!(!model.admin_logs_follow_end());
    send(&mut model, AdminMessage::SetLogPaused(false));
    model.admin.merge_log(log_record("later", "2026-10-08T08:01:00Z", "info", "host", "", "", "又一条"));
    assert!(model.admin_logs_follow_end());
    model.workspace.panel = WorkspacePanel::Files;
    assert!(!model.admin_logs_follow_end());
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
    assert_eq!((rows[0].id.as_str(), rows[0].source.as_str()), ("workspace-operation", "资源库"));
    assert_eq!((rows[1].id.as_str(), rows[1].source.as_str(), rows[1].detail.as_str()), ("task-1", "任务", "scanning"));
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
fn tool_pages_follow_frontend_manifests_and_keep_the_selection() {
    let mut backend = tool_plugin("backend.tool", json!([{ "toolPageId": "backend.page", "label": "后端" }]));
    backend.sdk = "backend".into();
    let mut disabled = tool_plugin("disabled.tool", json!([{ "toolPageId": "disabled.page", "label": "停用" }]));
    disabled.enabled = false;
    let pages = support::tool_pages_from_plugins(&[
        tool_plugin("a.tool", json!([{ "toolPageId": "zeta", "label": "Zeta", "order": 5 }, { "toolPageId": "  ", "label": "空" }])),
        tool_plugin("b.tool", json!([{ "toolPageId": "momobako.tool.api-playground", "label": "API Playground", "description": "调试" }])),
        tool_plugin("c.tool", json!([{ "toolPageId": "alpha", "label": "Alpha", "order": 5 }])),
        backend,
        disabled,
    ]);
    assert_eq!(pages.iter().map(|page| page.id.as_str()).collect::<Vec<_>>(), ["alpha", "zeta", "momobako.tool.api-playground"]);
    assert_eq!((pages[2].native, pages[2].plugin_name.as_str(), pages[2].description.as_str()), (true, "b.tool", "调试"));
    assert!(!pages[0].native);

    let mut model = ShellViewModel::default();
    send(&mut model, AdminMessage::SetToolPages(Vec::new()));
    assert!(model.admin.active_tool_page_id.is_none());
    let entry = |id: &str| ToolPageEntry { id: id.into(), label: id.into(), plugin_name: "tool".into(), description: String::new(), native: false };
    send(&mut model, AdminMessage::SetToolPages(vec![entry("old"), entry("new")]));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("old"));
    send(&mut model, AdminMessage::SelectToolPage("missing".into()));
    send(&mut model, AdminMessage::SelectToolPage("new".into()));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("new"));
    send(&mut model, AdminMessage::SetToolPages(vec![entry("new")]));
    assert_eq!(model.admin.active_tool_page_id.as_deref(), Some("new"));
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
fn builtin_import_pages_follow_the_file_machine() {
    let mut model = ShellViewModel::default();
    let blocked = tool_native::import_actions(&model, support::TOOL_FILE_MANAGER);
    let folder = import_action(&blocked, "folder");
    assert!(!folder.enabled);
    assert_eq!(tool_native::import_block_reason(&model), Some("当前没有可用仓库。"));
    assert!(tool_native::import_message(folder.enabled, folder.message.clone()).is_none());

    attach_repository(&mut model, vec!["write".into()]);
    let open = tool_native::import_actions(&model, support::TOOL_FILE_MANAGER);
    let folder = import_action(&open, "folder");
    assert!(folder.enabled);
    assert!(tool_native::import_block_reason(&model).is_none());
    assert!(matches!(&folder.message, FilesMessage::OpenDialog(FileDialog::Import)));
    model.reduce(tool_native::import_message(folder.enabled, folder.message.clone()).expect("folder"));
    assert!(matches!(model.files.dialog, FileDialog::Import));

    let eagle = tool_native::import_actions(&model, support::TOOL_EAGLE_IMPORTER);
    let copy = import_action(&eagle, "copy");
    assert!(matches!(&copy.message, FilesMessage::OpenEagle(mode) if mode == "copy"));
    model.reduce(tool_native::import_message(copy.enabled, copy.message.clone()).expect("copy"));
    assert_eq!(model.files.eagle_mode, "copy");

    model.workspace.repositories[0].capabilities.clear();
    assert_eq!(tool_native::import_block_reason(&model), Some("当前仓库处于只读状态。"));
    model.workspace.repositories[0].capabilities = vec!["write".into()];
    model.workspace.panel = WorkspacePanel::Trash;
    assert_eq!(tool_native::import_block_reason(&model), Some("回收站视图不支持导入。"));
    model.workspace.panel = WorkspacePanel::SmartFolder;
    assert_eq!(tool_native::import_block_reason(&model), Some("虚拟视图不支持导入。"));
}

#[test]
fn api_playground_falls_back_and_sends_http_and_plugin_calls() {
    let mut model = ShellViewModel::default();
    model.admin.external = Some(connection());
    send(&mut model, AdminMessage::Api(ApiMessage::SetKeyword("repositories".into())));
    let selected = super::api::selected_ref(&model).expect("兜底端点");
    assert_eq!(selected.path, "/external/v1/repositories");
    assert!(model.admin.api.include_auth);
    assert_eq!(super::api::request_url(&model), "http://127.0.0.1:9/external/v1/repositories");
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    match model.admin.take_effects().pop() {
        Some(AdminEffect::HttpRequest(request)) => {
            assert_eq!((request.method.as_str(), request.body.as_deref()), ("GET", None));
            assert!(request.headers.contains(&("Authorization".into(), "Bearer 1234567890abcdef".into())));
        }
        other => panic!("unexpected {other:?}"),
    }
    assert!(model.admin.api.sending);
    send(&mut model, AdminMessage::Api(ApiMessage::HttpFinished(Ok(HttpResponse {
        status: 200,
        status_text: "OK".into(),
        headers: vec![("Content-Type".into(), "application/json".into())],
        body: "{\"items\":[]}".into(),
    }))));
    assert_eq!(model.admin.api.response_status, "200 OK");
    assert!(model.admin.api.response_headers.contains("content-type"));
    assert!(model.admin.api.response_body.contains("\"items\": []"));
    assert!(!model.admin.api.sending);

    send(&mut model, AdminMessage::Api(ApiMessage::Copy));
    assert!(matches!(model.admin.take_effects().pop(), Some(AdminEffect::CopyText(text)) if text.starts_with("curl -X GET 'http://127.0.0.1:9/external/v1/repositories'")));
    assert_eq!(model.admin.api.notice, "请求已复制。");

    send(&mut model, AdminMessage::Api(ApiMessage::SetKeyword(String::new())));
    send(&mut model, AdminMessage::Api(ApiMessage::SelectEndpoint("external-http:POST:/external/v1/assets:add".into())));
    send(&mut model, AdminMessage::Api(ApiMessage::SetRequestText("{".into())));
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    assert!(model.admin.take_effects().is_empty());
    assert_eq!(model.admin.api.response_status, "ERROR");
    assert!(model.admin.api.error.starts_with("请求 JSON 无效"));

    model.admin.api_design = Some(ApiDesignSnapshot {
        transport: "tauri-ipc".into(),
        endpoints: vec![crate::backend::services::repository::ApiDefinition {
            group: "插件".into(),
            transport: String::new(),
            method: String::new(),
            path: String::new(),
            summary: "状态".into(),
            command: None,
            plugin_id: Some("momobako.service.downloader".into()),
            plugin_method: Some("downloader.getRuntimeStatus".into()),
            requires_auth: None,
            request_template: None,
        }],
    });
    send(&mut model, AdminMessage::Api(ApiMessage::SelectEndpoint("plugin-call:momobako.service.downloader:downloader.getRuntimeStatus".into())));
    assert_eq!(model.admin.api.request_text, "{}");
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    assert!(matches!(
        model.admin.take_effects().pop(),
        Some(AdminEffect::CallPlugin { method, origin: PluginCallOrigin::Playground, .. }) if method == "downloader.getRuntimeStatus"
    ));
    send(&mut model, AdminMessage::Api(ApiMessage::PluginFinished(Ok(json!({ "payload": { "running": true } })))));
    assert_eq!(model.admin.api.response_status, "OK");
    assert!(model.admin.api.response_headers.contains("downloader.getRuntimeStatus"));
    send(&mut model, AdminMessage::Api(ApiMessage::Send));
    send(&mut model, AdminMessage::Api(ApiMessage::PluginFinished(Err("插件未启用".into()))));
    assert_eq!((model.admin.api.response_status.as_str(), model.admin.api.error.as_str()), ("ERROR", "插件未启用"));
}

#[test]
fn file_plugin_call_keeps_repository_and_writes_activity() {
    let mut model = ShellViewModel::default();
    send(&mut model, AdminMessage::CallFilePlugin {
        plugin_id: "momobako.netease.source".into(),
        method: "media.clearTrackCache".into(),
        payload: json!({"songId": 9}),
        repository_id: Some("repo".into()),
    });
    match model.admin.take_effects().pop() {
        Some(AdminEffect::CallPlugin { method, repository_id, origin, .. }) => {
            assert_eq!(method, "media.clearTrackCache");
            assert_eq!(repository_id.as_deref(), Some("repo"));
            assert_eq!(origin, PluginCallOrigin::FileMenu);
        }
        other => panic!("unexpected {other:?}"),
    }
    send(&mut model, AdminMessage::FilePluginFinished { method: "media.clearTrackCache".into(), result: Ok(json!({})) });
    assert_eq!(model.files.activity, "已调用 media.clearTrackCache。");
    send(&mut model, AdminMessage::FilePluginFinished { method: "media.clearTrackCache".into(), result: Err(String::new()) });
    assert_eq!(model.files.error, "media.clearTrackCache 调用失败。");
}

#[test]
fn slashes_and_repeated_ids_in_external_data_still_mount() {
    assert_eq!(super::style::key_part("a/b%c"), "a%2Fb%25c");
    let mut model = ShellViewModel::for_page(super::super::ShellPage::PluginSettings);
    let mut odd = plugin("user/tool", "user", "service", "service");
    odd.contributes = json!({
        "settings": { "fields": [
            { "key": "path/root", "label": "根目录" },
            { "key": "path/root", "label": "重复的根目录" }
        ] },
        "toolPages": [{ "toolPageId": "user/page", "label": "工具" }]
    });
    odd.sdk = "frontend".into();
    odd.runtime = "vue-module".into();
    odd.entry = json!({ "frontend": { "module": "dist/register.js" } });
    load_bundle(&mut model, vec![odd.clone(), odd]);
    send(&mut model, AdminMessage::ToggleSettings("user/tool".into()));
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records: vec![log_record("log/1", "2026-01-01T00:00:00Z", "info", "host", "", "", "斜杠日志")],
        next_cursor: None,
    })));
    send(&mut model, AdminMessage::ToggleLogContext("log/1".into()));
    if let Err(error) = crate::acceptance_document_for_model(model.clone()) {
        panic!("拓展页的插件、字段和工具页标识带斜杠时挂载失败：{error:?}");
    }
    model.workspace.panel = WorkspacePanel::Logs;
    if let Err(error) = crate::acceptance_document_for_model(model) {
        panic!("日志标识带斜杠时挂载失败：{error:?}");
    }
}

#[test]
fn asmr_shortcuts_publish_onto_the_filter_bar() {
    let mut model = ShellViewModel::default();
    let mut manifest = plugin("momobako.library.asmr", "builtin", "library-kind", "asmr");
    manifest.contributes = json!({
        "libraryExtension": { "libraryKind": "asmr", "searchShortcuts": ["works", "missing"] }
    });
    load_bundle(&mut model, vec![manifest]);
    assert_eq!(model.inspect.shortcuts.len(), 1);
    assert_eq!(model.inspect.shortcuts[0].label, "ASMR 作品");
    assert_eq!(model.inspect.shortcuts[0].metadata, "libraryKind=asmr");
}
