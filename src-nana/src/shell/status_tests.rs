//! 全局状态区的回归：各领域的失败怎么记进来、什么时候清，状态区按「失败 > 忙碌 > 同步进度」显示，文件
//! 操作失败在文件列表显示着时不重复显示；侧栏顶部的状态条原地换内容、侧栏不重挂，和新挂的文档一致。
//! 文件夹树刷新本身的流程在 `tree_sync_tests.rs`。

use crate::shell::player::PlayerMessage;
use crate::shell::sidebar::SidebarMessage;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{InspectMessage, ShellMessage, ShellPage, ShellViewModel, WorkspacePanel};

use super::{Activity, FailureSource, StatusLine};

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("没有场景 {name}"))
        .1
}

/// 状态区现在的失败：来源和文案。
fn failure(model: &ShellViewModel) -> Option<(FailureSource, String)> {
    model.status.failure().map(|failure| (failure.source, failure.message.clone()))
}

/// 有仓库、停在文件页、什么都没在读的 ViewModel。
fn files_page() -> ShellViewModel {
    let model = scene("live-files-plain");
    assert_eq!(StatusLine::project(&model), StatusLine::Hidden, "场景本身不该有失败或忙碌");
    model
}

/// 定位失败：宿主执行请求时系统程序起不来。
fn reveal_fails(model: &mut ShellViewModel, error: &str) {
    model.reduce(ShellMessage::Input(crate::shell::input::InputMessage::RevealEntry { absolute_path: "C:\\acceptance\\cover.png".into() }));
    crate::host_bridge::perform(model, |_, _| Err(error.to_string()), |_| Ok(()), || Ok(()));
}

#[test]
fn host_failures_show_until_the_next_operation_starts() {
    let mut model = files_page();
    reveal_fails(&mut model, "系统找不到指定的文件。");
    assert_eq!(failure(&model), Some((FailureSource::Host, "定位失败：系统找不到指定的文件。".into())));
    assert_eq!(StatusLine::project(&model), StatusLine::Failure("定位失败：系统找不到指定的文件。".into()));

    // 和操作无关的消息不清：后台日志、缩略图、开合筛选栏。
    model.reduce(ShellMessage::ThumbnailPixels(Vec::new()));
    model.reduce(ShellMessage::Inspect(InspectMessage::ToggleFilterBar));
    assert!(failure(&model).is_some(), "无关的消息不该清掉失败");

    // 刷新资源库列表是一个新操作：清掉上一次失败，读列表时显示忙碌行。
    model.reduce(ShellMessage::Refresh);
    assert_eq!(failure(&model), None);
    assert_eq!(StatusLine::project(&model), StatusLine::Busy);
}

#[test]
fn each_domain_writes_the_failure_it_cannot_show_in_place() {
    let mut model = files_page();
    model.reduce(ShellMessage::Inspect(InspectMessage::MetadataSaved(Err("版本服务不可用".into()))));
    assert_eq!(failure(&model), Some((FailureSource::Metadata, "保存元数据失败：版本服务不可用".into())));

    model.reduce(ShellMessage::PlaylistDetailLoaded(Err("播放集不存在".into())));
    assert_eq!(failure(&model), Some((FailureSource::Playlist, "播放集操作失败：播放集不存在".into())), "后记的顶替先记的");

    model.reduce(ShellMessage::Player(PlayerMessage::MembershipSaved(Err("写入失败".into()))));
    assert_eq!(failure(&model), Some((FailureSource::Player, "更新播放集成员失败：写入失败".into())));

    model.reduce(ShellMessage::SettingsSaved(Err("磁盘已满".into())));
    assert_eq!(failure(&model), Some((FailureSource::Settings, "保存应用设置失败：磁盘已满".into())));

    model.reduce(ShellMessage::LogsLoaded(Err("日志库被占用".into())));
    assert_eq!(failure(&model), Some((FailureSource::Logs, "无法读取系统日志：日志库被占用".into())));

    let repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarTreeLoaded { repo_id, result: Err("拒绝访问。".into()) }));
    assert_eq!(failure(&model), Some((FailureSource::FolderTree, "拒绝访问。".into())));
}

/// 被盯着的领域字段：新写一次记一次，清掉时它记下的那一条一起清；别的来源的失败不受影响。
#[test]
fn watched_fields_record_new_writes_and_clear_with_their_field() {
    let mut model = files_page();
    let repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarTreeLoaded { repo_id: repo_id.clone(), result: Err("拒绝访问。".into()) }));
    assert_eq!(failure(&model).map(|(source, _)| source), Some(FailureSource::FolderTree));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarTreeLoaded {
        repo_id: repo_id.clone(),
        result: Ok(crate::shell::sidebar::SidebarTree::from(Vec::new())),
    }));
    assert_eq!(failure(&model), None, "文件夹树读回来了，它的失败一起清");

    // 归约以外写下的文件操作失败（宿主派发时写的）由同步前的观察收进来。
    model.files.error = "剪贴板写入失败".into();
    assert!(model.observe_failures(), "新写的文件操作失败要记下");
    assert_eq!(failure(&model), Some((FailureSource::Files, "剪贴板写入失败".into())));
    assert!(!model.observe_failures(), "同一份失败只记一次");
    model.files.error.clear();
    assert!(model.observe_failures());
    assert_eq!(failure(&model), None);

    // 字段清掉时只清它自己记下的那一条。
    model.files.error = "剪贴板写入失败".into();
    model.observe_failures();
    reveal_fails(&mut model, "拒绝访问。");
    model.files.error.clear();
    model.observe_failures();
    assert_eq!(failure(&model).map(|(source, _)| source), Some(FailureSource::Host), "别的来源的失败不该跟着清");
}

/// 文件操作失败写在文件列表的状态框里：列表显示着时状态区让位，换到别的面板时状态区显示它。
#[test]
fn file_failures_give_way_to_the_file_list() {
    let mut model = files_page();
    model.files.error = "无法移动文件：目标已存在".into();
    model.observe_failures();
    assert_eq!(StatusLine::project(&model), StatusLine::Hidden, "文件列表已经显示了，状态区不重复");
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    assert_eq!(StatusLine::project(&model), StatusLine::Failure("无法移动文件：目标已存在".into()));
}

/// 状态区的顺序：失败 > 忙碌。选中文件读详情时是忙碌行，详情读不到时换成失败。
#[test]
fn busy_shows_while_loading_and_failures_win() {
    let mut model = files_page();
    model.reduce(ShellMessage::SelectFile { path: "cover.png".into(), asset_id: None });
    assert_eq!(StatusLine::project(&model), StatusLine::Busy, "读素材详情时忙碌");
    reveal_fails(&mut model, "拒绝访问。");
    assert_eq!(StatusLine::project(&model), StatusLine::Failure("定位失败：拒绝访问。".into()), "失败排在忙碌前面");
    model.reduce(ShellMessage::AssetDetailLoaded(Err("素材不存在".into())));
    assert_eq!(StatusLine::project(&model), StatusLine::Failure("无法读取文件元数据：素材不存在".into()));
    assert!(!super::busy(&model), "详情失败以后不再忙碌");
}

/// 换仓库读摘要、刷新读列表时忙碌，结果到达就放下；启动以后读摘要失败进状态区。
#[test]
fn repository_reads_are_busy_until_their_result_arrives() {
    let mut model = files_page();
    let repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
    model.reduce(ShellMessage::SelectWorkspaceRepository(repo_id));
    assert!(model.workspace.snapshot_loading && super::busy(&model));
    model.reduce(ShellMessage::RepositorySnapshotLoaded(Err("索引损坏".into())));
    assert!(!model.workspace.snapshot_loading);
    assert_eq!(failure(&model), Some((FailureSource::Repository, "无法读取资源库摘要：索引损坏".into())));

    model.reduce(ShellMessage::Refresh);
    assert!(model.workspace.list_loading && super::busy(&model));
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::WorkspaceListLoaded { generation: generation - 1, result: Ok(Vec::new()) });
    assert!(model.workspace.list_loading, "过期的列表结果不放下忙碌");
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Err("服务未就绪".into()) });
    assert!(!model.workspace.list_loading);
    assert_eq!(failure(&model), Some((FailureSource::Repository, "无法读取资源库列表：服务未就绪".into())));
}

/// 归约开始时记下的序号分开新旧：这次归约里开始了操作，清掉以前的失败，这次新记的留着。
#[test]
fn an_operation_start_clears_only_older_failures() {
    let mut model = files_page();
    reveal_fails(&mut model, "旧的失败");
    let (before, seq) = (Activity::of(&model), model.status.seq());
    model.files.loading = true;
    model.settle_status(&before, seq, false);
    assert_eq!(failure(&model), None, "开始读目录，清掉以前的失败");

    let (before, seq) = (Activity::of(&model), model.status.seq());
    model.status.fail(FailureSource::Directory, "这次新记的失败");
    model.settle_status(&before, seq, true);
    assert_eq!(failure(&model), Some((FailureSource::Directory, "这次新记的失败".into())), "同一次归约里新记的失败不清");
}

/// 侧栏顶部的状态条：失败、忙碌和没有之间原地切换，侧栏不重挂；每一步都和新挂的一样。
#[test]
fn the_sidebar_status_strip_switches_in_place() {
    let mut harness = ShellHarness::mount(files_page());
    let sidebar = harness.sidebar_root();
    let strip = harness.keyed("sidebar-status").expect("错误条");
    let busy = harness.keyed("sidebar-status-busy").expect("忙碌行");
    assert!(harness.find("定位失败：系统找不到指定的文件。").is_none());

    reveal_fails(&mut harness.model, "系统找不到指定的文件。");
    harness.sync();
    harness.flush();
    assert!(harness.find("定位失败：系统找不到指定的文件。").is_some(), "失败要显示在侧栏顶部");
    harness.assert_same_as_fresh_mount();

    harness.apply(ShellMessage::SelectFile { path: "cover.png".into(), asset_id: None });
    harness.flush();
    assert!(harness.find("定位失败：系统找不到指定的文件。").is_none(), "选中别的文件清掉失败");
    assert!(harness.find("正在同步仓库状态").is_some(), "读详情时显示忙碌行");
    harness.assert_same_as_fresh_mount();

    assert_eq!(harness.sidebar_root(), sidebar, "侧栏不该重挂");
    assert_eq!(harness.keyed("sidebar-status"), Some(strip));
    assert_eq!(harness.keyed("sidebar-status-busy"), Some(busy));
}

/// 结构更新后的后台静默刷新（重读动作列表、静默重读目录）不是用户开始的操作，不清失败。
#[test]
fn background_refreshes_keep_the_failure() {
    let mut model = files_page();
    reveal_fails(&mut model, "拒绝访问。");
    let repo_id = model.workspace.active_repo_id.clone().expect("活动仓库");
    model.reduce(ShellMessage::Host(crate::shell::host_events::HostMessage::StructureUpdated { repo_id, reason: "watcher".into() }));
    assert!(model.admin.actions_loading, "结构更新会在后台重读动作列表");
    assert_eq!(failure(&model), Some((FailureSource::Host, "定位失败：拒绝访问。".into())), "后台刷新不该清掉失败");
}

#[test]
fn starts_follow_the_vue_guards() {
    use crate::shell::input::InputMessage;
    let starts = |message| super::starts_operation(&ShellMessage::Input(message));
    assert!(!starts(InputMessage::OpenEntry { has_repo: false, absolute_path: "C:\\a.png".into() }));
    assert!(!starts(InputMessage::RevealEntry { absolute_path: " ".into() }));
    assert!(starts(InputMessage::RevealEntry { absolute_path: "C:\\a.png".into() }));
    let drag = |trash: bool, backend: &str| InputMessage::StartExternalDrag {
        paths: vec!["a.png".into()],
        trash,
        backend_kind: backend.into(),
        repo_root: "C:\\repo".into(),
    };
    assert!(!starts(drag(true, "filesystem")), "回收站里不能拖出，不算开始");
    assert!(!starts(drag(false, "webdav")), "不是本地文件系统，不算开始");
    assert!(starts(drag(false, "filesystem")));
    assert!(!super::starts_operation(&ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree)), "刷新文件夹树看忙碌标志");
    assert!(!super::starts_operation(&ShellMessage::Navigate(ShellPage::Logs)));
}

/// 刷新文件夹树真的开始（同步排下、忙碌由假变真）才清掉上一次失败；按钮禁用时被拦下的点击不清。
#[test]
fn folder_tree_refresh_clears_only_when_it_starts() {
    let mut model = files_page();
    reveal_fails(&mut model, "系统找不到指定的文件。");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    assert!(model.tree_sync.running(), "刷新开始了");
    assert_eq!(failure(&model), None, "刷新开始时清掉上一次失败");

    model.status.fail(FailureSource::Host, "刷新期间的失败");
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    assert_eq!(failure(&model), Some((FailureSource::Host, "刷新期间的失败".into())), "刷新进行中再点被拦下，不清失败");
}
