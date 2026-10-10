//! 来源条目动作在产品路径上的行为，对照 Vue `sourceEntryActions.ts` 和 `useEntryActionRepositoryDialog.ts`：从网易云
//! 资源库的右键菜单出发，看菜单项的禁用和原因、清理缓存的载荷、「下载到其他资源库」选目标、开始下载和收尾。

use serde_json::json;

use crate::shell::admin::source_download::DownloadOutcome;
use crate::shell::admin::{AdminEffect, AdminMessage, PluginCallOrigin};
use crate::shell::entry_actions::{NO_TARGET_REPOSITORY, SOURCE_UNAVAILABLE};
use crate::shell::files::FilesMessage;
use crate::shell::files_view::EntryMenuView;
use crate::shell::view_harness::ShellHarness;
use crate::shell::view_part_overlay::OverlayKey;
use crate::shell::{GapMessage, ShellMessage, ShellViewModel, SidebarMessage, WorkspaceEffect};

use super::result_paths;

const TRACK: &str = "晴天.mp3";

fn scene(name: &str) -> ShellViewModel {
    crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == name)
        .unwrap_or_else(|| panic!("场景 {name}"))
        .1
}

/// 登录正常、列表里还有第二、第三资源库的网易云资源库，选目标的对话框已经取消、右键菜单打开着。
fn menu_open() -> ShellViewModel {
    let mut model = scene("source-download-target");
    model.reduce(ShellMessage::Admin(AdminMessage::CancelDownloadRepository));
    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu { path: TRACK.into(), x: 400.0, y: 470.0 }));
    model.admin.take_effects();
    model
}

/// 菜单里每一项：（文字，禁用，是不是写原因的一行）。
fn menu(model: &ShellViewModel) -> Vec<(String, bool, bool)> {
    EntryMenuView::project(model)
        .expect("右键菜单开着")
        .entries
        .into_iter()
        .map(|entry| (entry.label, entry.disabled, entry.note))
        .collect()
}

/// 点菜单里文字是 `label` 的那一项：先关菜单，再发这一项的消息（和菜单项的点击处理一样）。
fn click(model: &mut ShellViewModel, label: &str) {
    let action = EntryMenuView::project(model)
        .and_then(|menu| menu.entries.into_iter().find(|entry| entry.label == label))
        .and_then(|entry| entry.action)
        .unwrap_or_else(|| panic!("菜单里没有能点的「{label}」"));
    model.reduce(ShellMessage::Files(FilesMessage::CloseEntryMenu));
    model.reduce(action.message());
}

fn row(label: &str, disabled: bool) -> (String, bool, bool) {
    (label.to_string(), disabled, false)
}

fn note(text: &str) -> (String, bool, bool) {
    (text.to_string(), false, true)
}

/// 登录过期：照 Vue `actionDisabled`，网易云的条目动作都禁用，紧跟着一行写「来源认证或本地缓存不可用」，只写一次；
/// 别的菜单项不受影响。
#[test]
fn an_expired_login_disables_source_actions_and_says_why() {
    let model = scene("source-menu-unavailable");
    let items = menu(&model);
    let start = items.iter().position(|item| item.0 == "下载单曲到本地").expect("有下载单曲");
    assert_eq!(
        items[start..start + 5],
        [
            row("下载单曲到本地", true),
            row("下载单曲到其他资源库", true),
            row("重新获取播放资源", true),
            row("清理播放缓存", true),
            note(SOURCE_UNAVAILABLE),
        ]
    );
    assert_eq!(items.iter().filter(|item| item.2).count(), 1, "原因只写一行");
    assert!(items.iter().any(|item| item.0 == "预览" && !item.1), "别的菜单项照常");

    let harness = ShellHarness::mount(model);
    assert!(harness.find(SOURCE_UNAVAILABLE).is_some(), "原因画在菜单里");
    let clear = harness.keyed("file-menu-plugin-momobako.netease.source-clear-track-cache").expect("清理播放缓存这一项");
    let disabled = harness.document().context().world().accessibility(clear).is_some_and(|state| state.disabled);
    assert!(disabled, "禁用的菜单项点不了");
}

/// 清理播放缓存和重新获取播放资源照 Vue 带上当前资源库的本地缓存目录（`managedCacheRoot`），网易云插件据此清到、
/// 取到真正的播放缓存，不退回插件自己的临时目录。
#[test]
fn clearing_the_cache_passes_the_managed_cache_root() {
    for label in ["清理播放缓存", "重新获取播放资源"] {
        let mut model = menu_open();
        click(&mut model, label);
        let effects = model.admin.take_effects();
        let payload = effects
            .iter()
            .find_map(|effect| match effect {
                AdminEffect::CallPlugin { payload, repository_id, origin: PluginCallOrigin::FileMenu, .. } => {
                    assert_eq!(repository_id.as_deref(), Some("acceptance-repo"), "带上当前资源库");
                    Some(payload.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("「{label}」没有调用插件：{effects:?}"));
        assert_eq!(payload["managedCacheRoot"], "D:/MomoCache/netease", "{label}");
        assert_eq!(payload["songId"], 186016);
    }
}

/// 下载单曲到其他资源库：有两个候选时弹出「选择目标资源库」；选第三资源库以后对话框关掉，任务弹层是「下载单曲 ·
/// 正在下载 晴天.mp3」，载荷的去处写成第三资源库；插件交回以后照 Vue `context.refreshRepo()` 刷新当前资源库，
/// 刷新的「加载资源库」接着占住进度行。
#[test]
fn downloading_to_another_repository_asks_for_the_target_first() {
    let mut model = scene("source-download-target");
    assert_eq!(OverlayKey::of(&model), Some(OverlayKey::DownloadTarget));
    let pick = model.admin.download_target.as_ref().expect("等着选目标");
    let names: Vec<&str> = pick.candidates.iter().map(|candidate| candidate.name.as_str()).collect();
    assert_eq!(names, ["第二资源库", "第三资源库"], "只列就绪、可写、不是当前的资源库");
    assert!(model.admin.take_effects().iter().all(|effect| !matches!(effect, AdminEffect::DownloadToRepository { .. })), "选好以前不下载");
    assert!(model.operation.shown().is_none(), "选好以前不出进度行");

    model.reduce(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository("third-repo".into())));
    assert_eq!(OverlayKey::of(&model), None, "选好就关");
    let row = model.operation.shown().expect("进度行");
    assert_eq!((row.label.as_str(), row.detail.as_str(), row.value, row.indeterminate), ("下载单曲", "正在下载 晴天.mp3", 36.0, true));
    let effects = model.admin.take_effects();
    let (run, payload) = effects
        .iter()
        .find_map(|effect| match effect {
            AdminEffect::DownloadToRepository { run, payload, parent_path } => {
                assert_eq!(run.repository_id.as_deref(), Some("acceptance-repo"), "插件按当前资源库取来源凭据");
                let target = run.target().expect("目标资源库");
                assert_eq!((target.repo_id.as_str(), target.path.as_str(), parent_path), ("third-repo", "D:/third", &None));
                Some((run.clone(), payload.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("没有下载到其他资源库：{effects:?}"));
    assert_eq!(run.method, "media.downloadTrackPackage");
    assert_eq!(payload["destination"], json!({ "kind": "repository", "repoId": "third-repo", "path": "D:/third" }));
    assert_eq!(payload["managedCacheRoot"], "D:/MomoCache/netease");

    model.workspace.take_effects();
    let result = json!({ "paths": ["D:/stage/晴天.mp3", "D:/stage/晴天.lrc"] });
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Ok(DownloadOutcome::plugin(result)) }));
    let row = model.operation.shown().expect("进度行");
    assert_eq!((row.label.as_str(), row.detail.as_str()), ("加载资源库", "读取已注册资源库"), "刷新接手进度行");
    assert!(
        model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositories { .. })),
        "照 Vue context.refreshRepo() 刷新当前资源库"
    );
}

/// 第三方来源的单曲：来源载荷里只有条目种类、没有歌曲标识。右键清理播放缓存、下载到其他资源库时，`songId` 退回条目
/// 的来源标识（`providerItemId`），`playlistId` 不退回（Vue 把歌曲的标识也填成歌单标识）。
#[test]
fn a_track_without_a_song_id_is_identified_by_its_provider_item_id() {
    let mut model = menu_open();
    let row = model.files.rows.iter_mut().find(|row| row.path == TRACK).expect("晴天.mp3");
    row.source_payload = Some(json!({ "entryKind": "track", "songName": "晴天" }));
    assert_eq!(row.provider_item_id.as_deref(), Some("186016"));

    click(&mut model, "清理播放缓存");
    let effects = model.admin.take_effects();
    let payload = effects
        .iter()
        .find_map(|effect| match effect {
            AdminEffect::CallPlugin { payload, origin: PluginCallOrigin::FileMenu, .. } => Some(payload.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("清理播放缓存没有调用插件：{effects:?}"));
    assert_eq!(payload["songId"], 186016);
    assert!(payload.get("playlistId").is_none(), "单曲不拿自己的标识去填歌单标识");

    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu { path: TRACK.into(), x: 400.0, y: 470.0 }));
    click(&mut model, "下载单曲到其他资源库");
    model.reduce(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository("third-repo".into())));
    let effects = model.admin.take_effects();
    let payload = effects
        .iter()
        .find_map(|effect| match effect {
            AdminEffect::DownloadToRepository { payload, .. } => Some(payload.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("没有下载到其他资源库：{effects:?}"));
    assert_eq!(payload["songId"], 186016);
    assert!(payload.get("playlistId").is_none());
}

/// 取消、Escape 都不下载；导入或同步目标资源库失败时进度行收起，原因写进文件列。
#[test]
fn cancelling_or_failing_downloads_nothing_more() {
    let mut cancelled = scene("source-download-target");
    cancelled.reduce(ShellMessage::Admin(AdminMessage::CancelDownloadRepository));
    assert_eq!(OverlayKey::of(&cancelled), None);
    assert!(cancelled.admin.take_effects().iter().all(|effect| !matches!(effect, AdminEffect::DownloadToRepository { .. })));

    let mut escaped = scene("source-download-target");
    escaped.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(GapMessage::Escape)));
    assert!(escaped.admin.download_target.is_none(), "Escape 关掉选择");

    let mut failed = scene("source-download-target");
    failed.reduce(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository("second-repo".into())));
    let run = failed
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::DownloadToRepository { run, .. } => Some(run),
            _ => None,
        })
        .expect("开始下载到第二资源库");
    failed.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Err("导入目标资源库失败：磁盘已满".into()) }));
    assert!(failed.operation.shown().is_none(), "失败时进度行收起");
    assert_eq!(failed.files.error, "导入目标资源库失败：磁盘已满");
    assert_eq!(crate::shell::status::StatusLine::project(&failed), crate::shell::status::StatusLine::Hidden, "文件列已经写着，状态区不重复");
}

/// 只有一个候选时照 Vue 直接用它，不弹选择；一个都没有时菜单里「下载单曲到其他资源库」禁用，写出原因，别的照常。
#[test]
fn the_number_of_targets_decides_whether_to_ask() {
    let mut single = menu_open();
    single.workspace.repositories.retain(|repository| repository.repo_id != "third-repo");
    click(&mut single, "下载单曲到其他资源库");
    assert_eq!(OverlayKey::of(&single), None, "一个候选不弹选择");
    assert!(single
        .admin
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, AdminEffect::DownloadToRepository { run, .. }
            if run.target().is_some_and(|target| target.repo_id == "second-repo"))));

    let mut alone = menu_open();
    alone.workspace.repositories.retain(|repository| repository.repo_id == "acceptance-repo");
    let items = menu(&alone);
    let start = items.iter().position(|item| item.0 == "下载单曲到本地").expect("有下载单曲");
    assert_eq!(
        items[start..start + 5],
        [
            row("下载单曲到本地", false),
            row("下载单曲到其他资源库", true),
            row("重新获取播放资源", false),
            row("清理播放缓存", false),
            note(NO_TARGET_REPOSITORY),
        ]
    );
}

/// 对话框里点候选卡片发的是选中这个资源库；底栏只有「取消」。
#[test]
fn the_target_dialog_lists_cards_that_choose_a_repository() {
    let mut harness = ShellHarness::mount(scene("source-download-target"));
    harness.flush();
    assert!(harness.find("选择目标资源库").is_some(), "对话框标题");
    let card = harness.keyed("download-target-third-repo").expect("第三资源库的卡片");
    let (x, y) = harness.center(card);
    let messages = harness.click_messages(x, y);
    assert!(
        messages.iter().any(|message| matches!(message, ShellMessage::Admin(AdminMessage::ChooseDownloadRepository(id)) if id == "third-repo")),
        "点卡片选中这个资源库"
    );
    assert!(harness.find("取消").is_some());
}

/// 插件交回的文件：`paths` 和 `completed[].paths`，去掉空白和重复。
#[test]
fn result_paths_collect_direct_and_completed_files() {
    let payload = json!({
        "paths": ["D:/a.mp3", " ", "D:/a.lrc"],
        "completed": [{ "paths": ["D:/b.mp3", "D:/a.mp3"] }, { "songId": 3 }],
    });
    assert_eq!(result_paths(&payload), ["D:/a.mp3", "D:/a.lrc", "D:/b.mp3"]);
    assert!(result_paths(&json!({ "failed": [] })).is_empty());
}
