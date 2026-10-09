//! 15 个验收页使用和产品窗口同一套表面。
//!
//! 只填已经存在的仓库、预览、播放和任务状态，不发新的领域请求。

use crate::backend::services::repository::{
    PlaybackSessionState, PlaylistDetail, PlaylistItem, PlaylistPlayerContribution, PlaylistSummary,
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
        ShellPage::PluginSettings => admin_scenes::seed_plugin_settings(model),
        ShellPage::TaskRunning => admin_scenes::seed_task(model, false),
        ShellPage::PlaybackRunning => {
            model.detail = "正在播放 · track-01.mp3".into();
            present_repository(model);
            model.workspace.panel = WorkspacePanel::Playlist;
            seed_playback(model);
        }
        ShellPage::TaskCancelling => admin_scenes::seed_task(model, true),
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
        ShellPage::Settings => admin_scenes::seed_settings(model),
        ShellPage::SettingsError => admin_scenes::seed_settings_error(model),
        ShellPage::Logs => admin_scenes::seed_logs(model),
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
        ("still-playback", playback_gap_scene("png", "missing.png", "momobako.playlist.image-slideshow", "图片幻灯片")),
        ("outside-playback", playback_gap_scene("wma", "voice.wma", "momobako.playlist.foreign", "外部条目")),
        ("live-preview", live_preview_scene()),
        ("live-asmr", live_asmr_scene()),
    ]
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
