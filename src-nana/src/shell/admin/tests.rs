//! 设置、插件、日志、任务、仓库动作和工具页的状态机测试。
//!
//! 不启动仓库服务，服务调用只检查留下的副作用。

use std::collections::BTreeMap;

use serde_json::json;

use crate::backend::services::repository::{
    ApiDesignSnapshot, CacheSnapshot, CacheUsage, PluginConfigSnapshot, PluginDependencyStatus,
    PluginManifest, RepositoryAction, RepositoryActionStep, RepositoryBackendSummary,
    RepositorySummary, SystemLogLocation, SystemLogPage, SystemLogRecord, SystemLogSource,
    TaskProgressSnapshot,
};
use crate::backend::services::runtime::ExternalApiConnectionStatus;

use super::super::files::{FileDialog, FilesMessage};
use super::super::player::{AUDIO_CAPABILITY, AUDIO_SEQUENCE_TYPE, PlayerCandidate};
use super::super::repo_operation::OperationStart;
use super::super::workspace::{WorkspacePanel, WorkspaceRepository};
use super::super::{ShellMessage, ShellViewModel};
use super::api::{ApiMessage, HttpResponse};
use super::support::{self, action_can_run};
use super::tool_native::{self, ImportButton, ImportView};
use super::{AdminEffect, AdminMessage, PluginCallOrigin, ToolPageEntry};

mod plugins;
mod settings;
mod tools;

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

/// 只声明工具页的插件（仅清单）。
fn tool_plugin(id: &str, pages: serde_json::Value) -> PluginManifest {
    let mut manifest = plugin(id, "builtin", "service", "tool");
    manifest.runtime = "manifest-only".into();
    manifest.entry = json!({});
    manifest.contributes = json!({ "toolPages": pages });
    manifest
}

fn log_record(
    id: &str,
    timestamp: &str,
    level: &str,
    kind: &str,
    plugin_id: &str,
    repo_id: &str,
    message: &str,
) -> SystemLogRecord {
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
        location: SystemLogLocation {
            module_path: Some("app".into()),
            file: Some("main.rs".into()),
            line: Some(12),
        },
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
        backend: RepositoryBackendSummary {
            plugin_id: plugin_id.into(),
            kind: "local".into(),
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

fn cache(repo_id: &str) -> CacheSnapshot {
    CacheSnapshot {
        repo_id: repo_id.into(),
        entries: vec![
            CacheUsage {
                kind: "thumbnails".into(),
                label: "缩略图".into(),
                path: "D:/r/.momo/thumbnails".into(),
                files: 3,
                bytes: 2048,
                modified_at: None,
            },
            CacheUsage {
                kind: "cache/office-preview".into(),
                label: "Office 预览 PDF".into(),
                path: "D:/r/.momo/cache/office-preview".into(),
                files: 2,
                bytes: 1_572_864,
                modified_at: None,
            },
        ],
    }
}

fn api_design() -> ApiDesignSnapshot {
    ApiDesignSnapshot {
        transport: "宿主进程内调用；外部客户端走本机回环 HTTP（/external/v1）".into(),
        endpoints: Vec::new(),
    }
}

/// 一次成功的设置页数据。
/// 送回一次成功的设置包。插件列表换新以后排下的播放器类型读取在这里取走。
fn load_bundle(model: &mut ShellViewModel, plugins: Vec<PluginManifest>) {
    send(
        model,
        AdminMessage::SettingsBundleLoaded {
            plugins: Ok(plugins),
            hooks: Ok(Vec::new()),
            api: Ok(api_design()),
            external: Ok(connection()),
        },
    );
    let effects = model.admin.take_effects();
    assert!(
        matches!(effects.as_slice(), [AdminEffect::LoadPlaylistPlayers]),
        "插件列表换新后应只排下播放器类型的读取：{effects:?}"
    );
}

fn audio_labels(view: &support::AudioView) -> Vec<String> {
    view.choices
        .iter()
        .map(|choice| choice.label.clone())
        .collect()
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
        cache_path: None,
        login_expired: false,
        logged_out: false,
    }];
}

fn import_button(buttons: &[ImportButton], id: &str) -> FilesMessage {
    buttons
        .iter()
        .find(|item| item.id == id)
        .map(|item| item.message.clone())
        .unwrap_or_else(|| panic!("missing {id}"))
}
