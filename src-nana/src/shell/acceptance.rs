//! 15 个验收页使用和产品窗口同一套表面。
//!
//! 只填已经存在的仓库、预览、播放和任务状态，不发新的领域请求。

use super::workspace::WorkspaceRepository;
use super::{InspectEffect, ShellPage, ShellViewModel, WorkspacePanel};

const REPO_ID: &str = "acceptance-repo";
/// 等检视延时最多推进的帧数。一帧 16ms，产品的搜索等 250ms、自动保存等 260ms。
const TIMER_FRAMES: usize = 64;

/// 按页面填上对应的产品表面。加载页保持启动步骤，其余页面进入已有仓库或空库。
pub(super) fn seed(model: &mut ShellViewModel) {
    match model.page {
        ShellPage::Loading => shell_scenes::seed_loading(model),
        ShellPage::EmptyRepository => {
            model.detail = "可从文件夹或拖放导入资源".into();
            shell_scenes::seed_empty(model);
        }
        ShellPage::Error => shell_scenes::seed_error(model),
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
        ShellPage::PluginSettings => admin_scenes::seed_plugin_settings(model),
        ShellPage::TaskRunning => admin_scenes::seed_task(model, false),
        ShellPage::PlaybackRunning => {
            model.detail = "正在播放 · track-01.mp3".into();
            present_repository(model);
            model.workspace.panel = WorkspacePanel::Playlist;
            player_scenes::seed_playback(model);
        }
        ShellPage::TaskCancelling => admin_scenes::seed_task(model, true),
        ShellPage::Conflict => files_scenes::seed_conflict(model),
        ShellPage::UnsavedEdit => files_scenes::seed_unsaved_edit(model),
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

/// 按产品帧时钟推进搜索和自动保存的延时，直到排下 `pick` 认得的请求。
/// 同一批里别的请求和场景无关，一起丢掉。等满 `TIMER_FRAMES` 帧还没有就返回空。
fn await_inspect_effect<T>(model: &mut ShellViewModel, pick: impl Fn(InspectEffect) -> Option<T>) -> Option<T> {
    for _ in 0..TIMER_FRAMES {
        super::poll_timers(model);
        if let Some(found) = model.inspect.take_effects().into_iter().find_map(&pick) {
            return Some(found);
        }
    }
    None
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
    let mut scenes = Vec::new();
    scenes.extend(shell_scenes::models());
    scenes.extend(files_scenes::models());
    scenes.extend(player_scenes::models());
    scenes.extend(admin_scenes::models());
    scenes.extend(search_scenes::models());
    scenes
}
