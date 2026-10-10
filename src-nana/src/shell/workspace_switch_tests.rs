//! 换仓库和同一个仓库的刷新，对照 Vue `selectRepository` / `loadRepositories`：从切换列表、附加、新建、
//! 删除、来源动作完成以后的刷新和重定向这些入口走，送回服务结果，看启动页、侧栏、任务弹层、状态区和
//! 最后的工作区。搜索命中在别的仓库见 `search_tests.rs`。

use crate::backend::services::repository::{FileBrowserEntry, FileBrowserSnapshot};

use super::admin::{AdminEffect, AdminMessage};
use super::data_load_tests::{answer_settings, browser, snapshot, summary};
use super::repo_operation::FINISH_LINGER_MS;
use super::route_startup::StartupView;
use super::status::StatusLine;
use super::view_part::{body_mode, BodyMode};
use super::view_part_primary::RouteKey;
use super::workspace::{DeleteMode, StartupMode, StartupStepState};
use super::{
    FilesMessage, MainRegion, ShellMessage, ShellViewModel, SidebarEffect, SidebarMessage, StartupStatus, WorkspaceEffect,
};

const ALPHA: &str = "alpha";
const BETA: &str = "beta";

/// 启动走完、停在 alpha 的工作区；列表里还有 beta。侧栏、动作和设置包的读取都答过。
fn ready_on_alpha() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.last_active_repo_id = Some(ALPHA.into());
    let generation = model.workspace.prepare_initial_list();
    for message in [
        ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(ALPHA, "ready"), summary(BETA, "ready")]) },
        ShellMessage::RepositorySynced { generation, result: Ok(crate::shell::SyncCounts::default()) },
        ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot(ALPHA)) },
        ShellMessage::FileBrowserLoaded(Ok(browser(ALPHA))),
    ] {
        model.reduce(message);
    }
    answer_settings(&mut model);
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    model.workspace.take_effects();
    model.sidebar.take_effects();
    model.admin.take_effects();
    model.files.take_effects();
    model.sidebar.tree_loading = false;
    model.operation.tick(FINISH_LINGER_MS);
    model
}

/// 任务弹层的仓库操作行：(种类, 阶段, 百分比)。
fn operation_row(model: &ShellViewModel) -> Option<(String, String, f64)> {
    model.operation.shown().map(|row| (row.label.clone(), row.detail.clone(), row.value))
}

fn sync_requested(model: &mut ShellViewModel, repo_id: &str) -> bool {
    model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::SyncRepository { repo_id: id, .. } if id == repo_id))
}

/// 一个带文件的目录快照。
fn directory(repo_id: &str, path: &str, files: &[&str]) -> FileBrowserSnapshot {
    let mut snapshot = browser(repo_id);
    snapshot.current_path = path.into();
    snapshot.total_entries = files.len();
    snapshot.loaded_count = files.len();
    snapshot.entries = files
        .iter()
        .map(|file| FileBrowserEntry {
            path: (*file).into(),
            name: file.rsplit('/').next().unwrap_or(file).into(),
            kind: "file".into(),
            extension: Some("png".into()),
            size_bytes: None,
            size_label: None,
            modified_at: None,
            asset_id: None,
            status: None,
            thumbnail_path: None,
            thumbnail_custom: false,
            hardlink_group_id: None,
            hardlink_state: None,
            tags: Vec::new(),
            alias_paths: Vec::new(),
            folder_metadata: None,
            metadata: Default::default(),
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        })
        .collect();
    snapshot
}

/// 把换仓库的三步答完：同步、摘要、首屏目录。
fn finish_switch(model: &mut ShellViewModel, repo_id: &str) {
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(crate::shell::SyncCounts::default()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot(repo_id)) });
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory(repo_id, "", &["cover.png"]))));
}

/// 换仓库以后，上一个仓库后台查的硬链接候选晚到：读取是换仓库以前发出的，不认，新仓库里不弹别的仓库的确认。
#[test]
fn hardlink_candidates_of_the_previous_repository_are_dropped() {
    let mut model = ready_on_alpha();
    let epoch = model.files.hardlink_epoch();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectRepositoryFromSwitcher(BETA.into())));
    finish_switch(&mut model, BETA);
    let prompt = crate::shell::HardlinkPrompt {
        id: "alpha-link".into(),
        new_path: "inbox/a.png".into(),
        existing_path: "a.png".into(),
        size_label: "1 KB".into(),
    };
    model.reduce(ShellMessage::Files(FilesMessage::HardlinksChecked { epoch, result: Ok(vec![prompt]) }));
    assert_eq!(model.files.dialog, crate::shell::files::FileDialog::Closed, "别的仓库的候选不弹确认");
    assert!(model.files.current_hardlink().is_none());
}

/// 切换列表选了别的仓库：回到启动页、侧栏收起，第 2 步「扫描资源库文件」，任务弹层「加载资源库」；
/// 不写首屏启动日志；三步走完回到文件页、侧栏回来，不重读设置包，任务弹层这一行完成。
#[test]
fn switching_from_the_list_walks_the_startup_page_and_brings_the_sidebar_back() {
    let mut model = ready_on_alpha();
    assert_eq!(body_mode(&model), BodyMode::Workbench);
    let logs = model.workspace.startup.logs.len();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::OpenRepositorySwitcher));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectRepositoryFromSwitcher(BETA.into())));

    assert_eq!(model.sidebar.popover, super::sidebar::PopoverMode::Closed);
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some(BETA));
    assert_eq!((model.workspace.startup.status, model.workspace.startup.mode), (StartupStatus::Loading, StartupMode::Switch));
    assert_eq!(RouteKey::of(&model), RouteKey::Startup, "回到启动页");
    assert_eq!(body_mode(&model), BodyMode::Solo, "侧栏收起");
    let view = StartupView::project(&model);
    assert_eq!((view.text.title.as_str(), view.text.meta.as_str()), ("扫描资源库文件", "第 2 / 4 步"));
    assert_eq!(view.steps[0].state, StartupStepState::Done);
    assert_eq!(view.steps[1].state, StartupStepState::Current);
    assert_eq!(model.workspace.startup.logs.len(), logs, "换仓库不写首屏启动日志");
    assert_eq!(operation_row(&model), Some(("加载资源库".into(), "扫描资源库文件".into(), 28.0)));
    let effects = model.workspace.take_effects();
    assert!(effects.iter().any(|effect| matches!(effect, WorkspaceEffect::StopPlayback { previous_repo_id } if previous_repo_id == ALPHA)));
    assert!(effects.iter().any(|effect| matches!(effect, WorkspaceEffect::SyncRepository { repo_id, .. } if repo_id == BETA)));
    assert!(model.files.rows.is_empty(), "上一个仓库的条目清掉");
    assert!(model.sidebar.bound_repo_id().is_none(), "同步以前不读新仓库的侧栏");

    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(crate::shell::SyncCounts::default()) });
    assert_eq!(model.workspace.startup.step_label, "读取仓库摘要");
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::LoadSnapshot { repo_id, .. } if repo_id == BETA)));

    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot(BETA)) });
    assert_eq!(model.workspace.startup.step_label, "读取首屏目录");
    assert_eq!(model.workspace.startup.step_detail, "加载根目录文件、侧栏结构和首屏内容。");
    assert!(model.workspace.take_effects().contains(&WorkspaceEffect::LoadFirstScreen { repo_id: BETA.into() }));
    assert_eq!(operation_row(&model), Some(("加载资源库".into(), "加载资源索引".into(), 46.0)));
    assert!(
        model.sidebar.take_effects().iter().any(|effect| matches!(effect, SidebarEffect::LoadTree { repo_id } if repo_id == BETA)),
        "同步以后才读新仓库的目录树"
    );

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory(BETA, "", &["cover.png"]))));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
    assert_eq!(body_mode(&model), BodyMode::Workbench, "侧栏回来");
    assert_eq!(model.files.rows.len(), 1);
    assert!(!model.admin.take_effects().iter().any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)), "换仓库不重读设置包");
    assert_eq!(operation_row(&model).map(|row| row.2), Some(100.0), "首屏读完这一行完成");
    model.operation.tick(FINISH_LINGER_MS);
    assert!(operation_row(&model).is_none());
}

/// 换仓库失败停在启动页：当前步骤标成失败、写出原因、给「重试」，侧栏仍然收起，任务弹层这一行取消；
/// 「重试」照 Vue `ensureRepositoryWorkspace` 走完整的首屏启动（写启动日志、结束时读设置包）。
#[test]
fn a_failed_switch_stays_on_the_startup_page_until_retry_succeeds() {
    let mut model = ready_on_alpha();
    let logs = model.workspace.startup.logs.len();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectRepositoryFromSwitcher(BETA.into())));
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(crate::shell::SyncCounts::default()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Err("索引损坏".into()) });

    assert_eq!(model.workspace.startup.status, StartupStatus::Error);
    assert_eq!(model.workspace.main_region(), MainRegion::LoadError);
    assert_eq!(RouteKey::of(&model), RouteKey::Startup);
    assert_eq!(body_mode(&model), BodyMode::Solo);
    let view = StartupView::project(&model);
    assert!(view.text.retry, "失败给「重试」");
    assert_eq!(view.text.error.as_deref(), Some("索引损坏"));
    assert_eq!(view.steps[2].state, StartupStepState::Error, "停在第 3 步");
    assert_eq!(model.workspace.startup.logs.len(), logs, "换仓库失败也不写首屏启动日志");
    assert!(operation_row(&model).is_none(), "失败时任务弹层这一行取消");

    model.workspace.take_effects();
    model.reduce(ShellMessage::StartupRetry);
    assert_eq!((model.workspace.startup.status, model.workspace.startup.mode), (StartupStatus::Loading, StartupMode::Launch));
    assert_eq!(model.workspace.startup.logs.first().map(|log| log.message.as_str()), Some("首屏启动流程开始。"));
    let generation = model.workspace.list_generation;
    assert!(model.workspace.take_effects().contains(&WorkspaceEffect::RefreshRepositories { generation }));
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(ALPHA, "ready"), summary(BETA, "ready")]) });
    assert!(sync_requested(&mut model, BETA), "重试接着加载换过去的仓库");
    finish_switch(&mut model, BETA);
    assert_eq!(model.workspace.startup.status, StartupStatus::Loading, "重试是完整启动，读完首屏目录等设置包");
    answer_settings(&mut model);
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some(BETA));
    assert_eq!(body_mode(&model), BodyMode::Workbench);
}

/// 附加本地文件夹成功（Vue `attachRepository`）：任务弹层先是「挂载资源库」，成功以后新仓库放进列表、
/// 切过去，换仓库接手这一行。失败时回到添加菜单，这一行取消。
#[test]
fn attaching_a_folder_switches_to_the_new_repository() {
    let mut model = ready_on_alpha();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RepositoryAttachPathChanged("D:/新素材".into())));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SubmitRepositoryAttach));
    assert!(model.sidebar.take_effects().iter().any(|effect| matches!(effect, SidebarEffect::AttachRepository { path } if path == "D:/新素材")));
    assert_eq!(operation_row(&model), Some(("挂载资源库".into(), "检查文件夹并读取索引".into(), 8.0)));

    model.reduce(ShellMessage::Sidebar(SidebarMessage::RepositoryAttachFinished(Ok(summary("gamma", "ready")))));
    assert!(model.workspace.repositories.iter().any(|item| item.repo_id == "gamma"));
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("gamma"));
    assert_eq!(model.workspace.startup.mode, StartupMode::Switch);
    assert!(sync_requested(&mut model, "gamma"));
    assert_eq!(operation_row(&model).map(|row| row.0), Some("加载资源库".into()));
    finish_switch(&mut model, "gamma");
    assert_eq!(model.workspace.main_region(), MainRegion::HasRepository);
    assert_eq!(model.repository_name, "仓库 gamma");

    model.reduce(ShellMessage::Sidebar(SidebarMessage::RepositoryAttachPathChanged("D:/坏目录".into())));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SubmitRepositoryAttach));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RepositoryAttachFinished(Err("不是文件夹".into()))));
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("gamma"), "失败不换仓库");
    assert_eq!(model.sidebar.popover_error, "不是文件夹");
    assert!(operation_row(&model).is_none());
}

/// 新建资源库（Eagle Library，Vue `createNewRepository`）：「创建资源库 · 初始化资源库并扫描文件」，
/// 建完走到「切换到新资源库」再由换仓库接手。
#[test]
fn creating_a_repository_switches_to_it() {
    let mut model = ready_on_alpha();
    model.sidebar.attach_eagle = true;
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RepositoryAttachPathChanged("D:/Pics.library".into())));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SubmitRepositoryAttach));
    assert!(model.sidebar.take_effects().iter().any(|effect| matches!(effect, SidebarEffect::CreateBackendRepository { name, .. } if name == "Pics")));
    assert_eq!(operation_row(&model), Some(("创建资源库".into(), "初始化资源库并扫描文件".into(), 8.0)));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RepositoryAttachFinished(Ok(summary("eagle", "ready")))));
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("eagle"));
    assert_eq!(model.workspace.startup.step_label, "扫描资源库文件");
    assert!(sync_requested(&mut model, "eagle"));
}

/// 删除当前资源库以后（Vue `selectNextRepositoryAfterRemoval`）换到下一个仓库；删到没有仓库时回到空库页。
#[test]
fn deleting_the_current_repository_switches_to_the_next_one() {
    let mut model = ready_on_alpha();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::DeleteRepositoryFromSwitcher));
    model.reduce(ShellMessage::MissingConfirmDelete(DeleteMode::RecordOnly));
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::DeleteRepository { repo_id, .. } if repo_id == ALPHA)));
    model.reduce(ShellMessage::MissingDeleteFinished(Ok(())));
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some(BETA));
    assert_eq!(RouteKey::of(&model), RouteKey::Startup, "换到下一个仓库也走启动页");
    assert!(sync_requested(&mut model, BETA));
    finish_switch(&mut model, BETA);
    assert_eq!(model.workspace.main_region(), MainRegion::HasRepository);

    model.reduce(ShellMessage::Sidebar(SidebarMessage::DeleteRepositoryFromSwitcher));
    model.reduce(ShellMessage::MissingConfirmDelete(DeleteMode::RecordOnly));
    model.reduce(ShellMessage::MissingDeleteFinished(Ok(())));
    assert_eq!(model.workspace.main_region(), MainRegion::EmptyRepository);
    assert_eq!(RouteKey::of(&model), RouteKey::Empty);
}

/// 来源动作完成以后刷新（Vue `context.refreshRepo()` = `loadRepositories`）：同一个仓库先读列表、再同步、
/// 读摘要、重读当前目录；整轮状态区写「正在同步仓库状态」，任务弹层先「加载资源库」，重读目录时
/// 换成「读取目录」。不回启动页，侧栏留着并在后台重读。
#[test]
fn refreshing_the_same_repository_syncs_then_rereads_the_current_directory() {
    let mut model = ready_on_alpha();
    model.reduce(ShellMessage::Files(FilesMessage::OpenPath("pics".into())));
    model.files.take_effects();
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory(ALPHA, "pics", &["pics/a.png"]))));
    model.operation.tick(FINISH_LINGER_MS);

    model.reduce(ShellMessage::Admin(AdminMessage::FilePluginFinished { method: "source.clearCache".into(), result: Ok(serde_json::Value::Null) }));
    assert_eq!(StatusLine::project(&model), StatusLine::Busy, "状态区写「正在同步仓库状态」");
    let row = model.operation.shown().cloned().expect("加载资源库");
    assert_eq!((row.label.as_str(), row.detail.as_str(), row.indeterminate), ("加载资源库", "读取已注册资源库", true));
    let generation = model.workspace.list_generation;
    assert!(model.workspace.take_effects().contains(&WorkspaceEffect::RefreshRepositories { generation }));

    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(ALPHA, "ready"), summary(BETA, "ready")]) });
    assert_eq!(operation_row(&model), Some(("加载资源库".into(), "加载资源库摘要".into(), 38.0)));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready, "同一个仓库不回启动页");
    assert_eq!(body_mode(&model), BodyMode::Workbench);
    let generation = model.workspace.list_generation;
    assert!(sync_requested(&mut model, ALPHA), "先同步资源库");
    assert_eq!(StatusLine::project(&model), StatusLine::Busy);

    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(crate::shell::SyncCounts::default()) });
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::LoadSnapshot { repo_id, .. } if repo_id == ALPHA)));
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot(ALPHA)) });
    let browse = model.files.take_effects().into_iter().find_map(|effect| match effect {
        super::FilesEffect::Browse { repo_id, path, .. } => Some((repo_id, path)),
        _ => None,
    });
    assert_eq!(browse, Some((ALPHA.into(), "pics".into())), "重读刷新开始时所在的目录");
    assert!(model.sidebar.take_effects().iter().any(|effect| matches!(effect, SidebarEffect::LoadTree { repo_id } if repo_id == ALPHA)), "侧栏在后台重读");
    assert_eq!(operation_row(&model), Some(("读取目录".into(), "正在读取 pics".into(), 14.0)));
    assert_eq!(StatusLine::project(&model), StatusLine::Busy, "读目录时还在同步仓库状态");

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory(ALPHA, "pics", &["pics/a.png", "pics/b.png"]))));
    assert_eq!(StatusLine::project(&model), StatusLine::Hidden, "整轮结束放下忙碌");
    assert!(model.workspace.refresh.is_none());
    assert_eq!(model.files.rows.len(), 2);
    assert_eq!(operation_row(&model), Some(("读取目录".into(), "整理目录条目".into(), 100.0)));
}

/// 重读当前目录失败时照 Vue 退回根目录再读一次，读完收尾。
#[test]
fn a_failed_reread_falls_back_to_the_root_directory() {
    let mut model = ready_on_alpha();
    model.reduce(ShellMessage::Files(FilesMessage::OpenPath("pics".into())));
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory(ALPHA, "pics", &["pics/a.png"]))));
    model.files.take_effects();
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SelectRepositoryFromSwitcher(ALPHA.into())));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready, "切换列表选当前仓库：原地同步");
    let generation = model.workspace.list_generation;
    assert!(sync_requested(&mut model, ALPHA));
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(crate::shell::SyncCounts::default()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot(ALPHA)) });
    model.files.take_effects();
    model.reduce(ShellMessage::FileBrowserLoaded(Err("目录已删除".into())));
    let browse = model.files.take_effects().into_iter().find_map(|effect| match effect {
        super::FilesEffect::Browse { path, .. } => Some(path),
        _ => None,
    });
    assert_eq!(browse.as_deref(), Some(""), "退回根目录");
    assert_eq!(StatusLine::project(&model), StatusLine::Busy);
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory(ALPHA, "", &["cover.png"]))));
    assert!(model.workspace.refresh.is_none());
    assert_eq!(model.files.current_path, "");
    assert!(model.files.error.is_empty());
    assert_eq!(StatusLine::project(&model), StatusLine::Hidden);
}

/// 刷新时同步失败：原因写进状态区，放下忙碌，任务弹层这一行照 Vue 完成（`loadRepositories` 的完成）。
#[test]
fn a_failed_refresh_sync_lands_in_the_status_area() {
    let mut model = ready_on_alpha();
    model.reduce(ShellMessage::Admin(AdminMessage::FilePluginFinished { method: "source.clearCache".into(), result: Ok(serde_json::Value::Null) }));
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(ALPHA, "ready")]) });
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Err("拒绝访问。".into()) });
    assert_eq!(StatusLine::project(&model), StatusLine::Failure("同步资源库失败：拒绝访问。".into()));
    assert!(!super::status::busy(&model));
    assert_eq!(operation_row(&model).map(|row| row.2), Some(100.0));
    assert_eq!(model.workspace.main_region(), MainRegion::HasRepository, "失败不离开文件页");
}

/// 刷新以后当前仓库已经不在列表里：照 Vue 选第一项，是一次换仓库。
#[test]
fn a_refresh_without_the_current_repository_switches_to_the_first() {
    let mut model = ready_on_alpha();
    model.reduce(ShellMessage::Admin(AdminMessage::FilePluginFinished { method: "source.clearCache".into(), result: Ok(serde_json::Value::Null) }));
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(BETA, "ready")]) });
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some(BETA));
    assert_eq!(model.workspace.startup.mode, StartupMode::Switch);
    assert_eq!(RouteKey::of(&model), RouteKey::Startup);
    assert!(model.workspace.refresh.is_none() && !model.workspace.list_loading, "换仓库接手以后不再是刷新");
    assert!(sync_requested(&mut model, BETA));
}

/// 重定向缺失的仓库（Vue `relocateMissingRepository`）：「重定向资源库 · 校验资源库位置」，成功走到
/// 「更新资源库状态」，换上返回的摘要，再原地同步、读摘要、读目录；缺失页换成文件页，侧栏重新绑定。
#[test]
fn relocating_a_missing_repository_resyncs_it_in_place() {
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(vec![summary(ALPHA, "missing")]) });
    answer_settings(&mut model);
    assert_eq!(model.workspace.main_region(), MainRegion::MissingRepository);
    model.reduce(ShellMessage::MissingPathChanged("D:/找回的素材".into()));
    model.reduce(ShellMessage::MissingSubmitPath);
    assert_eq!(operation_row(&model), Some(("重定向资源库".into(), "校验资源库位置".into(), 12.0)));
    assert!(model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::RelocateRepository { path, .. } if path == "D:/找回的素材")));

    model.reduce(ShellMessage::MissingRelocateFinished(Ok(summary(ALPHA, "ready"))));
    assert_eq!(operation_row(&model), Some(("重定向资源库".into(), "更新资源库状态".into(), 64.0)));
    assert_eq!(model.workspace.main_region(), MainRegion::HasRepository, "缺失页换成文件页");
    let generation = model.workspace.list_generation;
    assert!(sync_requested(&mut model, ALPHA));
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(crate::shell::SyncCounts::default()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot(ALPHA)) });
    assert!(model.sidebar.take_effects().iter().any(|effect| matches!(effect, SidebarEffect::LoadTree { repo_id } if repo_id == ALPHA)), "侧栏重新绑定");
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(directory(ALPHA, "", &["cover.png"]))));
    assert!(model.workspace.refresh.is_none());
    assert_eq!(model.files.rows.len(), 1);
}
