//! 15 个验收页使用和产品窗口同一套表面。
//!
//! 只填已经存在的仓库、预览、播放和任务状态，不发新的领域请求。

use crate::backend::services::repository::{
    PlaybackSessionState, PlaylistDetail, PlaylistItem, PlaylistPlayerContribution, PlaylistSummary, PluginDependencyStatus,
    PluginManifest, TaskProgressSnapshot,
};

use super::inspect::InspectMessage;
use super::player::QueueItem;
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
            seed_playback(model);
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

/// 播放页走播放集表面：重排、移除和底部播放条，不再放上移下移按钮。
fn seed_playback(model: &mut ShellViewModel) {
    model.selected_playlist_id = Some("playlist-demo".into());
    model.sidebar.playlists = vec![super::SidebarPlaylist {
        id: "playlist-demo".into(),
        name: "演示播放列表".into(),
        player_label: "音频".into(),
        player_type_id: "audio".into(),
        item_count: 2,
    }];
    model.sidebar.playlists_expanded = true;
    model.sidebar.active_playlist_id = Some("playlist-demo".into());
    model.sidebar.playlist_player_type_ids = vec!["audio".into()];
    model.playlist_item_entries = vec!["track-01.mp3 · ready".into(), "track-02.mp3 · ready".into()];
    model.playlist_item_ids = vec!["item-01".into(), "item-02".into()];
    model.player.listed = Some(PlaylistDetail {
        playlist: PlaylistSummary {
            playlist_id: "playlist-demo".into(),
            repo_id: REPO_ID.into(),
            name: "演示播放列表".into(),
            player_type_id: "audio".into(),
            player_plugin_id: "audio".into(),
            player_label: "音频".into(),
            file_class: "audio".into(),
            item_count: 2,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
        },
        items: vec![track("item-01", "track-01.mp3"), track("item-02", "track-02.mp3")],
    });
    model.player.repo_id = Some(REPO_ID.into());
    model.player.current_id = Some("item-01".into());
    // 验收页没有音频插件。会话停在失败，不写成正在播放。
    model.player.can_play = false;
    model.player.wants_playing = false;
    model.player.queue = vec![queue_item("item-01", "track-01.mp3"), queue_item("item-02", "track-02.mp3")];
    model.player.session = PlaybackSessionState {
        session_id: format!("playback-{REPO_ID}"),
        repo_id: REPO_ID.into(),
        playlist_id: "playlist-demo".into(),
        playlist_item_id: Some("item-01".into()),
        status: "failed".into(),
        // 验收曲目没有读到的媒体时长。播放条显示 0:00，不留 1:24 / 3:48。
        current_time_ms: 0,
        duration_ms: None,
        volume: 0.8,
        can_seek: true,
        can_volume: true,
        error: Some("缺少对应播放插件".into()),
        updated_at: String::new(),
    };
}

fn track(id: &str, name: &str) -> PlaylistItem {
    PlaylistItem {
        playlist_item_id: id.into(),
        playlist_id: "playlist-demo".into(),
        asset_id: String::new(),
        path: format!("audio/{name}"),
        filename: name.into(),
        extension: "mp3".into(),
        thumbnail_path: None,
        status: "ready".into(),
        status_reason: None,
        sort_order: 0,
        added_at: String::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        metadata: None,
        local_absolute_path: None,
    }
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
        ("search-results", search_scene()),
        ("still-playback", playback_gap_scene("png", "missing.png", "momobako.playlist.image-slideshow", "图片幻灯片")),
        ("outside-playback", playback_gap_scene("wma", "voice.wma", "momobako.playlist.foreign", "外部条目")),
        ("live-preview", live_preview_scene()),
        ("live-asmr", live_asmr_scene()),
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

fn search_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::FileList);
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.filter_bar_open = true;
    model.inspect.query = "封面".into();
    model.inspect.filters.formats = vec!["png".into()];
    model.inspect.filters.tags = vec!["参考".into()];
    model.inspect.filters.colors = vec!["红色".into()];
    model.inspect.results = vec![super::inspect::SearchRow {
        repo_id: REPO_ID.into(),
        asset_id: "asset-cover".into(),
        path: "assets/cover.png".into(),
        filename: "cover.png".into(),
        repo_name: model.repository_name.clone(),
    }];
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

/// 播放页标题保持「演示播放列表」，条目换成还没有画面或 PCM 的扩展名。
fn playback_gap_scene(extension: &str, filename: &str, player_type_id: &str, player_label: &str) -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::PlaybackRunning);
    let path = format!("media/{filename}");
    let item = PlaylistItem {
        playlist_item_id: "gap-01".into(),
        playlist_id: "playlist-demo".into(),
        asset_id: String::new(),
        path: path.clone(),
        filename: filename.into(),
        extension: extension.into(),
        thumbnail_path: None,
        status: "ready".into(),
        status_reason: None,
        sort_order: 0,
        added_at: String::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        metadata: None,
        local_absolute_path: None,
    };
    if let Some(detail) = model.player.listed.as_mut() {
        detail.playlist.name = "演示播放列表".into();
        detail.playlist.player_type_id = player_type_id.into();
        detail.playlist.player_label = player_label.into();
        detail.playlist.file_class = extension.into();
        detail.playlist.item_count = 1;
        detail.items = vec![item];
    }
    model.playlist_item_ids = vec!["gap-01".into()];
    model.playlist_item_entries = vec![filename.into()];
    model.player.current_id = Some("gap-01".into());
    model.player.queue = vec![QueueItem {
        id: "gap-01".into(),
        playlist_id: "playlist-demo".into(),
        asset_id: String::new(),
        path,
        filename: filename.into(),
        extension: extension.into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: player_type_id.into(),
        player_label: player_label.into(),
        file_class: extension.into(),
        thumbnail_path: None,
    }];
    model.player.contributions = vec![PlaylistPlayerContribution {
        player_type_id: player_type_id.into(),
        label: player_label.into(),
        file_class: extension.into(),
        supported_extensions: vec![extension.into()],
        supports_seek: false,
        supports_volume: false,
        supports_preview_navigation: true,
        description: None,
    }];
    model.reduce(super::ShellMessage::Player(super::player::PlayerMessage::PlayItem { item_id: "gap-01".into() }));
    model
}

/// 文件列让开后的 PDF 页和元数据。页面文字来自实况 PDF 解析，不编造页数。
/// 页位图走宿主纹理槽，离屏会话没有 `Application::prepare` 上传，这里不挂空纹理。
fn live_preview_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    seed_browser(&mut model);
    model.files.display_mode = super::files::DisplayMode::List;
    let path = "notes/page.pdf";
    model.inspect.begin_selection(path);
    super::inspect::native_preview::begin(&mut model.inspect, REPO_ID, path, "momobako.preview.pdf".into(), "PDF".into());
    let loaded = super::inspect::native_preview::load("momobako.preview.pdf", PAGE_PDF).expect("验收 PDF");
    let generation = model.inspect.generation;
    model.reduce(super::ShellMessage::Inspect(super::inspect::InspectMessage::NativeLoaded {
        path: path.into(),
        generation,
        result: Ok(loaded),
    }));
    model.inspect.facts = super::inspect::FileFacts {
        extension: "pdf".into(),
        size_label: format!("{} B", PAGE_PDF.len()),
        ..super::inspect::FileFacts::default()
    };
    model.reduce(super::ShellMessage::Inspect(super::inspect::InspectMessage::SetComment("PDF 页".into())));
    // 页位图留在解码结果里。离屏会话不走 prepare，挂 file-preview 会引用未注册的宿主纹理。
    model.preview_pixels = None;
    model.preview_token = None;
    model
}

/// ASMR 音频预览。只有库类型和条目类型，不写歌词、时长、封面或色板。
fn live_asmr_scene() -> ShellViewModel {
    let mut model = ShellViewModel::for_page(ShellPage::SelectedFile);
    let path = "works/voice.mp3";
    let mut row = file_row(path, "file");
    row.metadata.insert("libraryKind".into(), serde_json::Value::String("asmr".into()));
    row.metadata.insert("asmrEntryKind".into(), serde_json::Value::String("audio".into()));
    model.files.rows = vec![row];
    model.files.total_entries = 1;
    model.files.set_drag_selection(vec![path.into()], Some(path.into()), Some(path.into()));
    model.selected_path = Some(path.into());
    model.detail = path.into();
    model.inspect.begin_selection(path);
    model.inspect.loading = false;
    model.inspect.activity.clear();
    model.inspect.facts.extension = "mp3".into();
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

fn queue_item(id: &str, name: &str) -> QueueItem {
    QueueItem {
        id: id.into(),
        playlist_id: "playlist-demo".into(),
        asset_id: String::new(),
        path: format!("audio/{name}"),
        filename: name.into(),
        extension: "mp3".into(),
        status: "ready".into(),
        status_reason: None,
        transient: false,
        player_type_id: "audio".into(),
        player_label: "音频".into(),
        file_class: "audio".into(),
        thumbnail_path: None,
    }
}
