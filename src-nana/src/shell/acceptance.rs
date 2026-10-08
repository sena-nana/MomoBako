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
        ("copy-dialog", copy_dialog_scene()),
        ("hardlink-dialog", hardlink_dialog_scene()),
        ("export-dialog", export_dialog_scene()),
        ("live-files", live_files_scene()),
        ("live-files-selected", live_files_selected_scene()),
        ("live-menu", live_menu_scene()),
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

fn copy_dialog_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.files.present_copy("notes");
    model
}

fn hardlink_dialog_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.files.present_hardlink(super::files::HardlinkPrompt {
        id: "link-1".into(),
        new_path: "inbox/cover.png".into(),
        existing_path: "assets/cover.png".into(),
        size_label: "未返回".into(),
    });
    model
}

fn export_dialog_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.files.present_export();
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

/// 文件列和文件夹树。缩略图走实况 `Thumbnail`，不另画一套占位。
fn live_files_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    seed_browser(&mut model);
    model.files.display_mode = super::files::DisplayMode::Grid;
    model.files.import_open = true;
    model.files.eagle_open = true;
    model
}

/// 选中 `notes/page.pdf`。右侧详情要能看到页图、类型、大小、修改时间和元数据。
fn live_files_selected_scene() -> ShellViewModel {
    let mut model = live_files_scene();
    model.files.import_open = false;
    model.files.eagle_open = false;
    let path = "notes/page.pdf";
    let size = format!("{} B", PAGE_PDF.len());
    if let Some(row) = model.files.rows.iter_mut().find(|row| row.path == path) {
        row.size_label = size.clone();
    }
    model.files.set_drag_selection(vec![path.into()], Some(path.into()), Some(path.into()));
    model.inspect.begin_selection(path);
    model.inspect.loading = false;
    model.inspect.activity.clear();
    model.inspect.facts.extension = "pdf".into();
    model.inspect.facts.size_label = size;
    model
}

/// 右键菜单挂在实况文件列上。
fn live_menu_scene() -> ShellViewModel {
    let mut model = live_files_scene();
    model.files.entry_menu = Some(super::files::EntryMenu { path: "cover.png".into(), x: 280.0, y: 180.0 });
    model
}

fn seed_browser(model: &mut ShellViewModel) {
    let mut page = file_row("notes/page.pdf", "file");
    attach_page_thumbnail(&mut page);
    model.files.rows = vec![file_row("assets", "directory"), file_row("cover.png", "file"), page];
    model.files.total_entries = model.files.rows.len();
    model.sidebar.folders = vec![super::sidebar::SidebarFolder {
        path: "assets".into(),
        label: "assets".into(),
        children: vec![super::sidebar::SidebarFolder {
            path: "assets/covers".into(),
            label: "covers".into(),
            children: Vec::new(),
        }],
    }];
    model.sidebar.expanded_folders = vec!["assets".into()];
}

fn file_row(path: &str, kind: &str) -> super::files::FileRow {
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_string());
    super::files::FileRow {
        path: path.into(),
        name,
        kind: kind.into(),
        asset_id: Some(path.replace('/', "-")),
        is_virtual: false,
        thumbnail_path: None,
        hardlink_state: None,
        extension,
        pixel_width: 0,
        pixel_height: 0,
        texture_ready: false,
        thumbnail_rgba: None,
        page_rgba: None,
        palette: Vec::new(),
        size_label: String::new(),
        modified_at: String::new(),
        tags: Vec::new(),
        thumbnail_custom: false,
        provider_id: None,
        source_payload: None,
        metadata: Default::default(),
    }
}

/// 用已经画出的 PDF 页缩小成缩略图。`cover.png` 没有像素，保持类型图标。
fn attach_page_thumbnail(row: &mut super::files::FileRow) {
    let loaded = match super::inspect::native_preview::load("momobako.preview.pdf", PAGE_PDF) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("Nana 验收页图没有缩略图：{error}");
            return;
        }
    };
    let Some(frame) = loaded.frames.iter().find(|frame| {
        frame.error.is_none() && !frame.rgba.is_empty() && frame.width > 0 && frame.height > 0
    }) else {
        eprintln!("Nana 验收 PDF 没有可缩小的页图");
        return;
    };
    let box_px = super::thumbs::grid_page_box(frame.width, frame.height);
    // 网格和详情都是整页缩小。只裁页眉会变成一条 MOMOBAKO 横条。
    let Some(fitted) = super::thumbs::fit_page_sheet(
        frame.width,
        frame.height,
        &frame.rgba,
        box_px.preview_width.round().max(1.0) as u32,
        box_px.preview_height.round().max(1.0) as u32,
    ) else {
        eprintln!("Nana 验收页图缩小失败");
        return;
    };
    row.pixel_width = frame.width;
    row.pixel_height = frame.height;
    row.thumbnail_rgba = Some(fitted);
    match super::thumbs::fit_page_sheet(frame.width, frame.height, &frame.rgba, 268, 160) {
        Some(sheet) => row.page_rgba = Some(sheet),
        None => eprintln!("Nana 验收整页没有装进详情盒"),
    }
}

/// 验收用的最小 PDF。文本是解析器从流里读出的 `MomoBako`。
const PAGE_PDF: &[u8] = b"%PDF-1.4\n1 0 obj\n<< /Length 14 >>\nstream\n(MomoBako) Tj\nendstream\nendobj\n%%EOF\n";

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
