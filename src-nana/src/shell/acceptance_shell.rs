//! 壳层与侧栏的对照场景：标题栏、侧栏、仓库切换、启动、空库和缺失仓库。
//!
//! 场景名和 `tmp/vue-mock/scenes/shell.ts` 的 Vue 场景同名，数据和 Vue 夹具（`fixtures.ts` 的 `base()`、
//! `liveVisual()`）一致：仓库在 `C:/acceptance`，「默认资源库」根目录有 assets、cover.png 和
//! notes/page.pdf，目录树 assets → covers。状态都走生产的启动、列表、侧栏消息，不直接拼画面；
//! 有仓库的场景从 `acceptance_base.rs` 的共用底子起步。换仓库停在启动页、刷新时的同步进度两个场景和
//! `tmp/vue-mock/scenes/states.ts` 同名。

use crate::backend::services::repository::RepositorySummary;

use super::super::admin::AdminMessage;
use super::super::files::DisplayMode;
use super::super::input::InputMessage;
use super::super::sidebar::GapMessage;
use super::super::workspace::{WorkspaceEffect, WorkspaceRepository};
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::base_scene::{
    acceptance_model, answer_admin_reads, dir, file, playlist_players, settle, summary, Base, REPO_ID, REPO_NAME, REPO_PATH,
};

/// Vue `error` 场景里同步命令抛出的错误。
const SYNC_ERROR: &str = "无法读取仓库目录，请检查路径和权限";
/// `status-error` 场景里系统文件管理器起不来时的原因。
const REVEAL_ERROR: &str = "系统找不到指定的文件。";
/// 换仓库场景里的第二个资源库。Vue `states.ts` 的 `SECOND_REPO_*` 用同一组值。
const SECOND_REPO_ID: &str = "second-repo";
const SECOND_REPO_NAME: &str = "第二资源库";

/// 本面板的离屏对照场景。加载、空库和启动失败三页由 `acceptance::seed` 调下面的 `seed_*`。
pub(super) fn models() -> Vec<(&'static str, ShellViewModel)> {
    vec![
        ("live-files-plain", live_files_plain()),
        ("live-visual", live_visual()),
        ("missing", missing()),
        ("repo-switcher", repo_switcher()),
        ("repo-delete-dialog", repo_delete_dialog()),
        ("folder-create-dialog", folder_create_dialog()),
        ("smart-folder-dialog", smart_folder_dialog()),
        ("playlist-create-dialog", playlist_create_dialog()),
        ("status-error", status_error()),
        ("folder-tree-refresh", folder_tree_refresh()),
        ("repo-switch-startup", repo_switch_startup()),
        ("repo-refresh-sync", repo_refresh_sync()),
    ]
}

/// 加载页：读完资源库列表、选中「默认资源库」，停在第 2 步同步文件变化。
pub(super) fn seed_loading(model: &mut ShellViewModel) {
    begin_sync(model, "默认资源库");
    settle(model);
}

/// 启动失败：同步文件变化时返回夹具的错误，停在第 2 步。
pub(super) fn seed_error(model: &mut ShellViewModel) {
    let generation = begin_sync(model, "默认资源库");
    model.workspace.note_sync_finished(generation, Err(SYNC_ERROR.into()));
    settle(model);
}

/// 空库：资源库列表为空，等完设置包和播放器类型、启动结束后进空库页，侧栏只剩没有仓库时的提示。
pub(super) fn seed_empty(model: &mut ShellViewModel) {
    model.workspace.last_active_repo_id = Some(REPO_ID.into());
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(Vec::new()) });
    answer_admin_reads(model, &playlist_players(), None);
    settle(model);
}

/// Vue `base()`：默认资源库，网格展示。
fn live_files_plain() -> ShellViewModel {
    Base::default().model()
}

/// Vue `liveVisual()`：动画素材，photos、audio 和 cover.png，自适应展示，没有目录树。
fn live_visual() -> ShellViewModel {
    Base {
        name: "动画素材",
        entries: vec![dir("photos"), dir("audio"), file("cover.png", 2_400_000)],
        tree: Vec::new(),
        mode: DisplayMode::Adaptive,
        ..Base::default()
    }
    .model()
}

/// Vue `missing`：唯一的资源库目录丢失，不同步，等完设置包和播放器类型就进缺失仓库页。
fn missing() -> ShellViewModel {
    let mut model = acceptance_model();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary("默认资源库", "missing")]) });
    answer_admin_reads(&mut model, &playlist_players(), None);
    settle(&mut model);
    model
}

/// 点开仓库头：切换列表、添加资源库和删除当前资源库。
fn repo_switcher() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher));
    settle(&mut model);
    model
}

/// 切换列表里点「删除当前资源库」。
fn repo_delete_dialog() -> ShellViewModel {
    let mut model = repo_switcher();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::DeleteRepositoryFromSwitcher));
    settle(&mut model);
    model
}

/// 文件夹分组的「在当前目录新建文件夹」，当前目录是根目录。
fn folder_create_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::OpenFolderCreate(String::new()))));
    settle(&mut model);
    model
}

/// 智能文件夹分组的「新建智能文件夹」。
fn smart_folder_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenSmartFolderDialog));
    settle(&mut model);
    model
}

/// 播放集分组的「新建播放集」。
fn playlist_create_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::OpenPlaylistDialog);
    settle(&mut model);
    model
}

/// 在文件管理器里定位 cover.png 失败：系统程序起不来，失败显示在侧栏顶部的全局状态区，文件列表照常显示。
fn status_error() -> ShellViewModel {
    let mut model = live_files_plain();
    let target = format!("{REPO_PATH}/cover.png");
    model.reduce(ShellMessage::Input(InputMessage::RevealEntry { absolute_path: target }));
    crate::host_bridge::perform(&mut model, |_, _| Err(REVEAL_ERROR.into()), |_| Ok(()), || Ok(()));
    settle(&mut model);
    model
}

/// 点了文件夹分组的「刷新文件夹树」，仓库同步还在跑：侧栏状态区显示同步进度「扫描文件夹结构 33%」，
/// 刷新按钮转圈并禁用。Vue 模拟的同步当场返回，截不到这一刻，没有对照图。
fn folder_tree_refresh() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    model.tree_sync.take_effects();
    settle(&mut model);
    model
}

/// 从仓库切换列表换到第二资源库，同步还没回来：回到启动页、收起侧栏，停在第 2 步「扫描资源库文件」。
fn repo_switch_startup() -> ShellViewModel {
    let second = RepositorySummary {
        repo_id: SECOND_REPO_ID.into(),
        name: SECOND_REPO_NAME.into(),
        path: "C:/second".into(),
        ..summary(SECOND_REPO_NAME, "ready")
    };
    let mut model = Base { others: vec![second], ..Base::default() }.model();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectRepositoryFromSwitcher(SECOND_REPO_ID.into())));
    // 同步请求排下了，宿主不答。
    model.workspace.take_effects();
    settle(&mut model);
    model
}

/// 刷新当前资源库（来源动作完成后的刷新）：读回资源库列表，同一个仓库开始同步，同步还没回来。
/// 侧栏状态区写「正在同步仓库状态」，任务弹层里是「加载资源库」那一行。和 Vue 场景一样点「任务」打开弹层。
fn repo_refresh_sync() -> ShellViewModel {
    let mut model = live_files_plain();
    model.workspace.request_repository_refresh();
    let refresh = model.workspace.take_effects().into_iter().find_map(|effect| match effect {
        WorkspaceEffect::RefreshRepositories { generation } => Some(generation),
        _ => None,
    });
    match refresh {
        Some(generation) => {
            model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(REPO_NAME, "ready")]) });
            // 同步请求排下了，宿主不答。
            model.workspace.take_effects();
        }
        None => eprintln!("Nana 刷新场景没有排下读资源库列表"),
    }
    model.reduce(ShellMessage::Admin(AdminMessage::ToggleTaskPopover));
    // 照产品帧时钟推进仓库操作行，和 Vue 截图时一样爬到上限 88（不定进度只画底色条，不写百分比）。
    for _ in 0..64 {
        model.operation.tick(220);
    }
    settle(&mut model);
    model
}

/// 启动流程走到第 2 步：读完资源库列表、选中仓库并开始同步。返回这一轮的代次。
fn begin_sync(model: &mut ShellViewModel, name: &str) -> u64 {
    model.workspace.last_active_repo_id = Some(REPO_ID.into());
    let generation = model.workspace.prepare_initial_list();
    model.workspace.apply_repository_list(Some(generation), Ok(vec![repository(name, "ready")]));
    generation
}

fn repository(name: &str, status: &str) -> WorkspaceRepository {
    WorkspaceRepository::from_summary(&summary(name, status))
}
