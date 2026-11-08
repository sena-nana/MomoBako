//! 壳层与侧栏的对照场景：标题栏、侧栏、仓库切换、启动、空库、缺失仓库，以及文件夹新建、重命名和处理。
//!
//! 场景名和 `tmp/vue-mock/scenes/shell.ts` 的 Vue 场景同名，数据和 Vue 夹具（`fixtures.ts` 的 `base()`、
//! `liveVisual()`）一致：仓库在 `C:/acceptance`，「默认资源库」根目录有 assets、cover.png 和
//! notes/page.pdf，目录树 assets → covers。状态都走生产的启动、列表、侧栏消息，不直接拼画面；
//! 有仓库的场景从 `acceptance_base.rs` 的共用底子起步。换仓库停在启动页、刷新时的同步进度两个场景和
//! `tmp/vue-mock/scenes/states.ts` 同名。

use crate::backend::services::repository::{
    RepositoryBackendSummary, RepositoryLocalCacheStatus, RepositorySummary,
};

use super::super::admin::AdminMessage;
use super::super::files::DisplayMode;
use super::super::input::InputMessage;
use super::super::sidebar::GapMessage;
use super::super::workspace::{WorkspaceEffect, WorkspaceRepository};
use super::super::{ShellMessage, ShellViewModel, SidebarMessage};
use super::base_scene::{
    Base, REPO_ID, REPO_NAME, REPO_PATH, acceptance_model, answer_admin_reads, dir, file,
    playlist_players, settle, summary,
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
        ("missing-source-cache", missing_source_cache()),
        ("repo-switcher", repo_switcher()),
        ("repo-delete-dialog", repo_delete_dialog()),
        ("folder-create-dialog", folder_create_dialog()),
        ("folder-rename-dialog", folder_rename_dialog()),
        ("folder-delete-dialog", folder_delete_dialog()),
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
    model
        .workspace
        .note_sync_finished(generation, Err(SYNC_ERROR.into()));
    settle(model);
}

/// 空库：资源库列表为空，等完设置包和播放器类型、启动结束后进空库页，侧栏只剩没有仓库时的提示。
pub(super) fn seed_empty(model: &mut ShellViewModel) {
    model.workspace.last_active_repo_id = Some(REPO_ID.into());
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(Vec::new()),
    });
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
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(vec![summary("默认资源库", "missing")]),
    });
    answer_admin_reads(&mut model, &playlist_players(), None);
    settle(&mut model);
    model
}

/// `missing-source-cache`：Vue `legacyNeteaseRepositoryWithoutCache`。
///
/// 仓库状态是 `missing`，`localCache.required` 且 `status` 是 `unconfigured`（没有缓存目录）。
/// 主按钮照 `MissingRepositoryState.vue` 写成「打开来源设置」。不编缓存文件、时间和曲目。
fn missing_source_cache() -> ShellViewModel {
    let mut model = acceptance_model();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(vec![legacy_netease_without_cache()]),
    });
    answer_admin_reads(&mut model, &playlist_players(), None);
    if !model
        .workspace
        .active_repository()
        .is_some_and(|repository| repository.is_source_cache_issue())
    {
        eprintln!("Nana 来源缓存未就绪场景没有落到缓存未就绪");
    }
    settle(&mut model);
    model
}

/// Vue `legacyNeteaseRepositoryWithoutCache`：旧网易云地址，缓存状态 `unconfigured`，目录为空。
fn legacy_netease_without_cache() -> RepositorySummary {
    RepositorySummary {
        repo_id: "netease-cloud-music-123456".into(),
        name: "云村 Aura".into(),
        path: "netease-cloud-music://account/123456".into(),
        backend: RepositoryBackendSummary {
            plugin_id: "momobako.netease.source".into(),
            kind: "netease-cloud-music".into(),
            name: "Netease Cloud Music".into(),
            capabilities: [
                "browse",
                "read",
                "sync",
                "virtual-entries",
                "authentication",
            ]
            .map(String::from)
            .to_vec(),
        },
        status: "missing".into(),
        asset_count: 2,
        updated_at: "2026-06-14T00:00:00Z".into(),
        local_cache: Some(RepositoryLocalCacheStatus {
            required: true,
            path: None,
            status: "unconfigured".into(),
        }),
        authentication: None,
    }
}

/// 点开仓库头：切换列表、添加资源库和删除当前资源库。
fn repo_switcher() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::OpenRepositorySwitcher,
    ));
    settle(&mut model);
    model
}

/// 切换列表里点「删除当前资源库」。
fn repo_delete_dialog() -> ShellViewModel {
    let mut model = repo_switcher();
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::DeleteRepositoryFromSwitcher,
    ));
    settle(&mut model);
    model
}

/// 文件夹分组的「在当前目录新建文件夹」，当前目录是根目录。
fn folder_create_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(
        GapMessage::OpenFolderCreate(String::new()),
    )));
    settle(&mut model);
    model
}

/// `folder-rename-dialog`：目录树里的 assets 打开「重命名文件夹」。
///
/// 摘要和输入都是夹具里的显示名，不提交，目录还叫 assets。树里没有这个文件夹就不打开。
fn folder_rename_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    open_fixture_folder(&mut model, "assets", true);
    settle(&mut model);
    model
}

/// `folder-delete-dialog`：目录树里的 assets 打开「处理文件夹」。
///
/// 只画出两个处理方式和「取消」，不点确认，夹具目录还在，也不写成已经删除成功。
fn folder_delete_dialog() -> ShellViewModel {
    let mut model = live_files_plain();
    open_fixture_folder(&mut model, "assets", false);
    settle(&mut model);
    model
}

/// 用夹具目录树里已经有的文件夹打开重命名或处理对话框。`rename` 为假时打开处理文件夹。
/// 树里没有这条路径，或显示名是空的，只记日志，不打开。
fn open_fixture_folder(model: &mut ShellViewModel, path: &str, rename: bool) {
    let Some(label) = fixture_folder_label(model, path) else {
        eprintln!("Nana 文件夹对话框场景的夹具里没有 {path}，不打开对话框");
        return;
    };
    let message = if rename {
        GapMessage::OpenFolderRename {
            path: path.into(),
            label,
        }
    } else {
        GapMessage::OpenFolderDelete {
            path: path.into(),
            label,
        }
    };
    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(message)));
    let opened = if rename {
        model.sidebar.folder_dialog.open
    } else {
        model.sidebar.folder_delete_open()
    };
    if !opened {
        eprintln!("Nana 文件夹对话框场景没有打开 {path}");
    }
}

/// 夹具目录树里这条路径的显示名。没有或是空名字时返回空。
fn fixture_folder_label(model: &ShellViewModel, path: &str) -> Option<String> {
    let mut pending = model.sidebar.folders.clone();
    while let Some(folder) = pending.pop() {
        if folder.path == path && !folder.label.is_empty() {
            return Some(folder.label);
        }
        pending.extend(folder.children);
    }
    None
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
    model.reduce(ShellMessage::Input(InputMessage::RevealEntry {
        absolute_path: target,
    }));
    crate::host_bridge::perform(
        &mut model,
        |_, _| Err(REVEAL_ERROR.into()),
        |_| Ok(()),
        || Ok(()),
    );
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
    let mut model = Base {
        others: vec![second],
        ..Base::default()
    }
    .model();
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::OpenRepositorySwitcher,
    ));
    model.reduce(ShellMessage::Sidebar(
        SidebarMessage::SelectRepositoryFromSwitcher(SECOND_REPO_ID.into()),
    ));
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
    let refresh = model
        .workspace
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            WorkspaceEffect::RefreshRepositories { generation } => Some(generation),
            _ => None,
        });
    match refresh {
        Some(generation) => {
            model.reduce(ShellMessage::WorkspaceListLoaded {
                generation,
                result: Ok(vec![summary(REPO_NAME, "ready")]),
            });
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
    model
        .workspace
        .apply_repository_list(Some(generation), Ok(vec![repository(name, "ready")]));
    generation
}

fn repository(name: &str, status: &str) -> WorkspaceRepository {
    WorkspaceRepository::from_summary(&summary(name, status))
}

#[cfg(test)]
mod missing_source_cache_scene_tests {
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::workspace::MainRegion;
    use super::{missing, missing_source_cache};

    /// 缓存状态不是 ready：主按钮是「打开来源设置」，不画重定向。本地文件夹丢失那一幅仍是「重定向」。
    #[test]
    fn missing_source_cache_scene_shows_the_settings_button() {
        let folder = missing();
        assert_eq!(folder.workspace.missing_primary_label(), "重定向");
        assert!(
            !folder
                .workspace
                .active_repository()
                .is_some_and(|repository| repository.is_source_cache_issue())
        );

        let model = missing_source_cache();
        assert_eq!(model.workspace.main_region(), MainRegion::MissingRepository);
        let repository = model.workspace.active_repository().expect("场景有仓库");
        assert!(
            repository.is_source_cache_issue(),
            "要用数据模型里的缓存未就绪"
        );
        assert_eq!(repository.cache_status, "unconfigured");
        assert!(repository.cache_path.is_none(), "夹具没有缓存目录");
        assert!(model.files.rows.is_empty(), "不编曲目");
        assert_eq!(model.workspace.missing_primary_label(), "打开来源设置");
        assert!(!model.player_surface_visible(), "缺失页不留播放条");

        let harness = ShellHarness::mount(model);
        for label in [
            "资源库丢失",
            "云村 Aura",
            "这个来源资源库需要在插件设置中配置本地缓存或重新认证。仓库记录和已有缓存不会被删除。",
            "netease-cloud-music://account/123456",
            "打开来源设置",
            "刷新",
            "删除资源库",
        ] {
            assert!(
                harness.find(label).is_some(),
                "缓存未就绪的缺失页缺少「{label}」"
            );
        }
        for label in [
            "重定向",
            "确认重定向",
            "资源库新位置",
            "晴天.mp3",
            "D:/MomoCache/netease",
        ] {
            assert!(
                harness.find(label).is_none(),
                "缓存未就绪的缺失页不该出现「{label}」"
            );
        }
    }
}

#[cfg(test)]
mod fixture_dialog_scene_tests {
    use super::super::super::view_harness::ShellHarness;
    use super::super::super::{GapMessage, ShellMessage, SidebarMessage};
    use super::{folder_delete_dialog, folder_rename_dialog, live_files_plain};

    fn root_paths(model: &super::super::super::ShellViewModel) -> Vec<&str> {
        model
            .files
            .rows
            .iter()
            .map(|row| row.path.as_str())
            .collect()
    }

    /// 文件夹重命名停在 assets 上：文案是「保存」，没有提交，目录还在。
    #[test]
    fn fixture_dialog_scene_shows_folder_rename() {
        let mut model = folder_rename_dialog();
        let dialog = &model.sidebar.folder_dialog;
        assert!(dialog.open && dialog.rename);
        assert_eq!(
            (
                dialog.target.as_str(),
                dialog.label.as_str(),
                dialog.value.as_str()
            ),
            ("assets", "assets", "assets")
        );
        assert_eq!(dialog.title(), "重命名文件夹");
        assert_eq!(dialog.action_label(), "保存");
        assert_eq!(dialog.summary(), "正在重命名 assets。");
        assert!(!dialog.submitting, "没有提交重命名");
        assert!(
            model.sidebar.take_folder_mutation().is_none(),
            "没有排下重命名"
        );
        assert_eq!(
            root_paths(&model),
            ["assets", "cover.png", "notes/page.pdf"]
        );

        let harness = ShellHarness::mount(model);
        for label in [
            "重命名文件夹",
            "正在重命名 assets。",
            "文件夹名称",
            "取消",
            "保存",
        ] {
            assert!(harness.find(label).is_some(), "重命名文件夹缺少「{label}」");
        }
        assert_eq!(harness.value(harness.input("文件夹名称")), "assets");

        let mut bare = live_files_plain();
        bare.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(
            GapMessage::OpenFolderRename {
                path: String::new(),
                label: "assets".into(),
            },
        )));
        assert!(
            !bare.sidebar.folder_dialog.open,
            "没有目标路径时不打开重命名"
        );
    }

    /// 处理文件夹停在确认上：两个选项和「取消」，不删除夹具，也不写成已经成功。
    #[test]
    fn fixture_dialog_scene_shows_folder_delete() {
        let mut model = folder_delete_dialog();
        assert!(model.sidebar.folder_delete_open());
        assert_eq!(model.sidebar.folder_delete_path, "assets");
        assert_eq!(model.sidebar.folder_delete_label, "assets");
        assert!(!model.sidebar.folder_delete_submitting, "没有确认删除");
        assert!(model.sidebar.folder_delete_error.is_empty());
        assert!(
            model.sidebar.take_folder_mutation().is_none(),
            "没有排下删除"
        );
        assert_eq!(model.sidebar.counts.trash, 0);
        assert_eq!(
            root_paths(&model),
            ["assets", "cover.png", "notes/page.pdf"]
        );

        let harness = ShellHarness::mount(model);
        for label in [
            "处理文件夹",
            "将处理文件夹“assets”。请选择内部内容的处理方式。",
            "转移到上级目录",
            "保留内部文件和子文件夹，只删除当前这一层目录。",
            "移入回收站",
            "将该目录及其全部内容移入回收站，可在回收站中还原。",
            "取消",
        ] {
            assert!(harness.find(label).is_some(), "处理文件夹缺少「{label}」");
        }
        assert!(harness.find("保存").is_none(), "处理文件夹没有保存按钮");

        let mut bare = live_files_plain();
        bare.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(
            GapMessage::OpenFolderDelete {
                path: String::new(),
                label: String::new(),
            },
        )));
        assert!(
            !bare.sidebar.folder_delete_open(),
            "没有目标时不打开处理文件夹"
        );
    }
}
