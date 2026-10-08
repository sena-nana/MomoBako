//! 15 个验收页使用和产品窗口同一套表面。
//!
//! 只填已经存在的仓库、预览、播放和任务状态，不发新的领域请求。

use crate::backend::services::repository::{PluginDependencyStatus, PluginManifest, TaskProgressSnapshot};

use super::inspect::InspectMessage;
use super::workspace::WorkspaceRepository;
use super::{ShellPage, ShellViewModel, WorkspacePanel};

const REPO_ID: &str = "acceptance-repo";

/// 按页面填上对应的产品表面。加载页保持启动步骤，其余页面进入已有仓库或空库。
pub(super) fn seed(model: &mut ShellViewModel) {
    match model.page {
        ShellPage::Loading => {}
        ShellPage::EmptyRepository => {
            model.detail = "可从文件夹或拖放导入资源".into();
            model.workspace.present_empty();
        }
        ShellPage::Error => {
            model.detail = "无法读取仓库目录，请检查路径和权限".into();
            model.workspace.startup.fail(model.detail.clone());
        }
        ShellPage::FileList => {
            model.detail = "12 个文件 · 按名称排序".into();
            present_repository(model);
        }
        ShellPage::SelectedFile => {
            model.selected_path = Some("assets/cover.png".into());
            model.detail = "PNG 图片 · 1920 × 1080 · 2.4 MB".into();
            present_repository(model);
            model.inspect.begin_selection("assets/cover.png");
        }
        ShellPage::Playlists => {
            model.detail = "正在加载播放列表".into();
            present_repository(model);
            model.workspace.panel = WorkspacePanel::Playlist;
            player_scenes::seed_playlists(model);
        }
        ShellPage::PluginSettings => {
            model.detail = "官方插件 · Nana 原生贡献接口 · 已加载 3 项配置".into();
            present_repository(model);
            model.workspace.panel = WorkspacePanel::Extensions;
        }
        ShellPage::TaskRunning => {
            model.detail = "扫描默认资源库 · 1,284 / 3,040 个文件".into();
            present_repository(model);
            open_task(model, "task-scan", "running", "扫描默认资源库");
        }
        ShellPage::PlaybackRunning => {
            model.detail = "正在播放 · track-01.mp3".into();
            present_repository(model);
            model.workspace.panel = WorkspacePanel::Playlist;
            player_scenes::seed_playback(model);
        }
        ShellPage::TaskCancelling => {
            model.detail = "正在取消扫描 · worker 尚未退出".into();
            present_repository(model);
            open_task(model, "task-cancelling", "cancelling", "正在取消扫描");
        }
        ShellPage::Conflict => {
            model.selected_path = Some("assets/cover.png".into());
            model.detail = "远端修改时间较新，需要选择保留本地或远端版本".into();
            present_repository(model);
            model.inspect.begin_selection("assets/cover.png");
            model.inspect.conflict = model.detail.clone();
        }
        ShellPage::UnsavedEdit => {
            model.selected_path = Some("notes/readme.md".into());
            model.dirty = true;
            model.detail = "Markdown · 3 行未保存 · 最后保存于 2 分钟前".into();
            present_repository(model);
            model.inspect.begin_selection("notes/readme.md");
            model.inspect.reduce(true, Some(REPO_ID), InspectMessage::SetComment("3 行未保存".into()));
        }
        ShellPage::Settings => {
            model.detail = "主题、缩略图缓存和默认播放器".into();
            present_repository(model);
        }
        ShellPage::SettingsError => {
            model.detail = "缩略图缓存上限必须在 64–16384 MB 之间".into();
            model.settings_error = Some(model.detail.clone());
            present_repository(model);
        }
        ShellPage::Logs => {
            model.detail = "最近 24 小时 · 18 条记录 · 0 个错误".into();
            present_repository(model);
            model.workspace.panel = WorkspacePanel::Logs;
        }
    }
}

/// 进入一个可写的本地仓库，侧栏和文件表面按产品规则显示。
fn present_repository(model: &mut ShellViewModel) {
    model.repository_id = Some(REPO_ID.into());
    model.workspace.present_repository(WorkspaceRepository {
        repo_id: REPO_ID.into(),
        name: model.repository_name.clone(),
        path: "C:/acceptance".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
    });
}

/// 任务弹层使用已经存在的进度行，取消按钮键是 `admin-task-cancel-{id}`。
fn open_task(model: &mut ShellViewModel, task_id: &str, status: &str, label: &str) {
    model.active_tasks = 1;
    model.active_task_ids = vec![task_id.into()];
    model.admin.popover_open = true;
    model.task_progress = vec![TaskProgressSnapshot {
        task_id: task_id.into(),
        protocol_id: "momobako.sync".into(),
        status: status.into(),
        phase: Some(status.into()),
        label: Some(label.into()),
        current: Some(1),
        total: Some(2),
        percent: Some(42.0),
        error: None,
        updated_at: "0".into(),
    }];
}

use files_scenes::{file_row, seed_browser, PAGE_PDF};

#[path = "acceptance_shell.rs"]
mod shell_scenes;
#[path = "acceptance_files.rs"]
mod files_scenes;
#[path = "acceptance_player.rs"]
mod player_scenes;
#[path = "acceptance_admin.rs"]
mod admin_scenes;
#[path = "acceptance_search.rs"]
mod search_scenes;

/// 补上的表面，给离屏验收出画面。数字和像素都不编造。
/// 各面板的对照场景放在各自的 `acceptance_*.rs` 里，这里只汇总。
pub fn gap_models() -> Vec<(&'static str, ShellViewModel)> {
    let mut scenes = base_gap_models();
    scenes.extend(shell_scenes::models());
    scenes.extend(files_scenes::models());
    scenes.extend(player_scenes::models());
    scenes.extend(admin_scenes::models());
    scenes.extend(search_scenes::models());
    scenes
}

fn base_gap_models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("downloader-settings", downloader_scene()),
        ("source-auth-gap", source_auth_scene()),
        ("source-auth-methods", source_auth_methods_scene()),
        ("office-convert", office_scene()),
        ("foreign-tool", foreign_tool_scene()),
    ]
}

fn downloader_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::Settings);
    let mut plugin = gap_plugin("momobako.service.downloader", "service", "download", "Download Service");
    plugin.contributes = serde_json::json!({
        "settings": { "settingsPage": { "label": "下载服务" }, "fields": [] }
    });
    model.admin.plugins = vec![plugin];
    model.admin.active_settings_plugin_id = Some("momobako.service.downloader".into());
    model
}

fn source_auth_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::Settings);
    let mut plugin = gap_plugin("momobako.source.example", "source", "source", "示例来源");
    plugin.contributes = serde_json::json!({
        "source": { "authentication": { "kind": "oauth" } }
    });
    model.workspace.repositories.push(super::workspace::WorkspaceRepository {
        repo_id: "source-repo".into(),
        name: "来源仓库".into(),
        path: "C:/acceptance/source".into(),
        status: "ready".into(),
        backend_plugin_id: "momobako.source.example".into(),
        capabilities: vec!["authentication".into()],
        cache_required: false,
        cache_status: String::new(),
    });
    model.admin.plugins = vec![plugin];
    model.admin.active_settings_plugin_id = Some("momobako.source.example".into());
    model
}

fn source_auth_methods_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::Settings);
    let mut plugin = gap_plugin("momobako.source.example", "source", "source", "示例来源");
    plugin.contributes = serde_json::json!({
        "source": {
            "authentication": {
                "kind": "qr",
                "createSessionMethod": "auth.createQrSession",
                "statusMethod": "auth.getLoginStatus",
                "pollSessionMethod": "auth.pollQrSession",
                "clearMethod": "auth.clearLogin",
                "repositoryProvisioning": { "requiresLocalCache": true, "repoIdPrefix": "source" }
            }
        }
    });
    model.workspace.repositories.push(WorkspaceRepository {
        repo_id: "source-repo".into(),
        name: "来源仓库".into(),
        path: "C:/acceptance/source".into(),
        status: "ready".into(),
        backend_plugin_id: "momobako.source.example".into(),
        capabilities: vec!["authentication".into()],
        cache_required: true,
        cache_status: String::new(),
    });
    model.admin.plugins = vec![plugin];
    model.admin.active_settings_plugin_id = Some("momobako.source.example".into());
    model.admin.source_auth.qr_text = Some("https://example.invalid/login".into());
    model.admin.source_auth.lines = vec!["已登录".into(), "账号 alice".into()];
    model
}

fn office_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::Settings);
    let mut plugin = gap_plugin("momobako.service.office-convert", "service", "office", "Office Convert");
    plugin.contributes = serde_json::json!({
        "settings": { "settingsPage": { "label": "Office 转换" }, "fields": [] }
    });
    model.admin.plugins = vec![plugin];
    model.admin.active_settings_plugin_id = Some("momobako.service.office-convert".into());
    model
}

fn foreign_tool_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::PluginSettings);
    model.admin.tool_pages = vec![super::admin::ToolPageEntry {
        id: "user.custom.tool".into(),
        label: "自定义工具".into(),
        plugin_name: "示例插件".into(),
        description: "把当前目录交给外部流程。".into(),
        native: false,
    }];
    model.admin.active_tool_page_id = Some("user.custom.tool".into());
    model
}

fn gap_plugin(id: &str, category: &str, kind: &str, name: &str) -> PluginManifest {
    PluginManifest {
        plugin_id: id.into(),
        package_format_version: None,
        package_hash: None,
        provenance: None,
        trust_level: None,
        deployment: None,
        target_triple: None,
        legacy_plugin_ids: Vec::new(),
        name: name.into(),
        version: "0.2.0".into(),
        r#type: None,
        kind: kind.into(),
        category: category.into(),
        description: String::new(),
        capabilities: Vec::new(),
        enabled: true,
        sdk: "backend".into(),
        entry: serde_json::Value::Null,
        contributes: serde_json::Value::Null,
        source: "builtin".into(),
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
