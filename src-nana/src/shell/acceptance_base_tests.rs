//! 验收场景的工作区底子和 Vue 夹具 `base()` 对齐：侧栏计数、目录树、根目录条目和展示方式都从启动消息
//! 算出来，和 `tmp/vue-mock/scenes/*.ts` 同名场景的数据一致。

use crate::shell::files::DisplayMode;
use crate::shell::{ShellPage, ShellViewModel, StartupStatus};

use super::{REPO_ID, REPO_NAME};

/// 补上的对照场景里名叫 `name` 的那一个。
fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 侧栏五个快捷方式的计数：全部、未分类、未标签、最近使用、回收站。
fn counts(model: &ShellViewModel) -> [usize; 5] {
    let counts = model.sidebar.counts;
    [counts.all, counts.uncategorized, counts.untagged, counts.recent, counts.trash]
}

/// 底子本身：启动走完、活动仓库是夹具仓库、目录树 assets → covers、根目录三条，启动结束时读到的插件清单在。
fn assert_base(name: &str, model: &mut ShellViewModel) {
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready, "{name} 的启动没走完");
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some(REPO_ID), "{name} 的活动仓库不对");
    assert_eq!(model.repository_name, REPO_NAME, "{name} 的资源库名不对");
    let tree = model.sidebar.folders.iter().map(|folder| (folder.path.as_str(), folder.children.len())).collect::<Vec<_>>();
    assert_eq!(tree, [("assets", 1)], "{name} 的目录树应是 assets → covers");
    let rows = model.files.rows.iter().map(|row| row.path.as_str()).collect::<Vec<_>>();
    assert_eq!(rows, ["assets", "cover.png", "notes/page.pdf"], "{name} 的根目录条目不对");
    assert!(!model.admin.plugins.is_empty(), "{name} 应读到启动结束时的插件清单");
    assert!(!model.admin.loading_settings, "{name} 的设置包读取应已答完");
    assert!(model.workspace.take_effects().is_empty(), "{name} 的启动请求应已按夹具答完");
}

/// 管理、搜索、播放集和任务类场景都从 `base()` 起步：全部 2、未分类 1、未标签 2，有 assets 目录，网格展示。
#[test]
fn listed_scenes_start_from_the_vue_base() {
    let mut models = [
        ShellPage::Settings,
        ShellPage::SettingsError,
        ShellPage::Logs,
        ShellPage::PluginSettings,
        ShellPage::Playlists,
        ShellPage::PlaybackRunning,
        ShellPage::TaskRunning,
        ShellPage::TaskCancelling,
    ]
    .map(|page| (format!("{page:?}"), ShellViewModel::for_page(page)))
    .to_vec();
    for name in [
        "logs-paused",
        "extensions",
        "downloader-settings",
        "office-convert",
        "source-auth-gap",
        "source-auth-methods",
        "filter-bar",
        "search-empty",
        "playlist-open",
        "playback-queue",
        "outside-playback",
        "still-playback",
    ] {
        models.push((name.to_string(), scene(name)));
    }
    for (name, mut model) in models {
        assert_eq!(counts(&model), [2, 1, 2, 0, 0], "{name} 的侧栏计数和 Vue 不一致");
        assert_eq!(model.files.display_mode, DisplayMode::Grid, "{name} 应是网格展示");
        assert_base(&name, &mut model);
    }
}

/// 页面身份留着：设置、日志和播放中这些页起步以后还停在自己的页面上。
#[test]
fn seeding_keeps_the_page_identity() {
    for page in [ShellPage::Settings, ShellPage::Logs, ShellPage::TaskRunning, ShellPage::PlaybackRunning] {
        assert_eq!(ShellViewModel::for_page(page.clone()).page, page);
    }
}

/// 场景改了素材的标签，计数跟着仓库摘要走：元数据场景的 cover.png 有标签，未标签是 1；搜索的带标签夹具
/// 两个文件都有标签，未标签是 0。
#[test]
fn tagged_fixtures_change_the_untagged_count() {
    for (name, untagged) in [("files-selected-metadata", 1), ("filter-bar-active", 0), ("search-results", 0)] {
        let model = scene(name);
        assert_eq!(counts(&model), [2, 1, untagged, 0, 0], "{name} 的侧栏计数和 Vue 不一致");
    }
}

/// 播放集类场景的播放集经侧栏的读取结果进来；登录场景的资源库列表里还有网易云仓库。
#[test]
fn scene_specific_data_arrives_through_the_same_messages() {
    for name in ["playlist-open", "outside-playback", "still-playback"] {
        let model = scene(name);
        assert_eq!(model.sidebar.playlists.len(), 1, "{name} 侧栏应有一个播放集");
        assert_eq!(model.sidebar.active_playlist_id.as_deref(), Some("playlist-demo"), "{name} 应点开了它");
    }
    let outside = scene("outside-playback");
    assert!(outside.playlist_players.iter().any(|player| player.player_type_id == "momobako.playlist.foreign"), "外部播放器要经播放器类型的读取进来");
    assert!(outside.player.contributions.iter().any(|player| player.player_type_id == "momobako.playlist.foreign"), "播放器也要认得外部播放器");
    let methods = scene("source-auth-methods");
    let repositories = methods.workspace.repositories.iter().map(|item| item.repo_id.as_str()).collect::<Vec<_>>();
    assert_eq!(repositories, [REPO_ID, "netease-cloud-music-10086"]);
}
