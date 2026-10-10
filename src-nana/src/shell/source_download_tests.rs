//! 来源下载的收尾，对照 Vue `sourceEntryActions.ts` 的 `runDownloadAction`：从网易云资源库的右键菜单出发下载歌单，
//! 插件交回的结果里有失败的条目时，状态区写出几项失败、去「日志」看原因，每一条写进系统日志；随后照 Vue
//! `context.refreshRepo()` 刷新资源库，这一轮刷新走完提示还在，用户开始下一个操作时才清掉。全部成功时不提示。

use serde_json::json;

use crate::shell::admin::{AdminEffect, AdminMessage};
use crate::shell::data_load_tests::{browser, snapshot};
use crate::shell::files::FilesMessage;
use crate::shell::files_view::EntryMenuView;
use crate::shell::input::InputMessage;
use crate::shell::status::{FailureSource, StatusLine};
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellViewModel, SyncCounts};

use super::{failed_items, DownloadRun};

/// 网易云资源库根目录里的单曲，场景自带。
const TRACK: &str = "晴天.mp3";
/// 加进根目录的歌单目录。
const FOLDER: &str = "夜曲";
/// 两项失败时状态区的提示。
const NOTICE: &str = "下载歌单「夜曲」有 2 项失败，原因见「日志」。";

/// 登录正常、列表里还有第二、第三资源库的网易云资源库，根目录多一个歌单目录「夜曲」（照来源目录的载荷：条目种类、
/// 歌单标识和歌单名）。选目标的对话框已经取消。
fn netease() -> ShellViewModel {
    let mut model = crate::shell::acceptance_gap_models()
        .into_iter()
        .find(|(scene, _)| *scene == "source-download-target")
        .map(|(_, model)| model)
        .expect("场景 source-download-target");
    model.reduce(ShellMessage::Admin(AdminMessage::CancelDownloadRepository));
    let mut folder = model.files.rows.iter().find(|row| row.path == TRACK).cloned().expect("晴天.mp3");
    folder.kind = "directory".into();
    folder.path = FOLDER.into();
    folder.name = FOLDER.into();
    folder.extension = None;
    folder.provider_item_id = Some("9001".into());
    folder.source_payload = Some(json!({ "entryKind": "playlist-folder", "playlistId": 9001, "playlistName": FOLDER }));
    model.files.rows.push(folder);
    model.admin.take_effects();
    model.workspace.take_effects();
    model
}

/// 右键 `path`，点菜单里文字是 `label` 的那一项（先关菜单，再发这一项的消息）。
fn click(model: &mut ShellViewModel, path: &str, label: &str) {
    model.reduce(ShellMessage::Files(FilesMessage::OpenEntryMenu { path: path.into(), x: 400.0, y: 470.0 }));
    let action = EntryMenuView::project(model)
        .and_then(|menu| menu.entries.into_iter().find(|entry| entry.label == label))
        .and_then(|entry| entry.action)
        .unwrap_or_else(|| panic!("菜单里没有能点的「{label}」"));
    model.reduce(ShellMessage::Files(FilesMessage::CloseEntryMenu));
    model.reduce(action.message());
}

/// 右键歌单目录 →「下载歌单到其他资源库」→ 选第三资源库，返回宿主拿去下载的那一次下载。
fn download_playlist(model: &mut ShellViewModel) -> DownloadRun {
    click(model, FOLDER, "下载歌单到其他资源库");
    model.reduce(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository("third-repo".into())));
    model
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::DownloadToRepository { run, .. } => Some(run),
            _ => None,
        })
        .expect("开始下载到第三资源库")
}

/// 插件交回的歌单结果：完成一首，失败两首（一首带歌曲名，一首只有标识）。
fn partly_failed() -> serde_json::Value {
    json!({
        "completed": [{ "songId": 186016, "paths": ["D:/stage/夜曲/周杰伦 - 晴天.mp3"] }],
        "failed": [
            { "songId": 185811, "songName": "七里香", "error": "未获取到可下载音频地址" },
            { "songId": 185812, "songName": null, "error": "网络超时" }
        ],
        "summary": { "total": 3, "succeeded": 1, "failed": 2 }
    })
}

fn notice(model: &ShellViewModel) -> Option<(FailureSource, String)> {
    model.status.failure().map(|failure| (failure.source, failure.message.clone()))
}

/// 两项失败：状态区写出几项失败、去哪里看原因；系统日志里每一项写一条，带上是哪一首、为什么失败、插件和来源资源库；
/// 照 Vue 接着刷新资源库，刷新的「加载资源库」占住进度行。
#[test]
fn a_partly_failed_download_says_how_many_failed_and_where_to_look() {
    let mut model = netease();
    let run = download_playlist(&mut model);
    assert_eq!((run.progress.label.as_str(), run.progress.subject.as_str()), ("下载歌单", FOLDER));
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Ok(partly_failed()) }));

    assert_eq!(notice(&model), Some((FailureSource::Download, NOTICE.into())));
    assert_eq!(StatusLine::project(&model), StatusLine::Failure(NOTICE.into()), "文件页上也要看得到");
    let logs = model
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::WriteLogs(logs) => Some(logs),
            _ => None,
        })
        .expect("失败的条目写进系统日志");
    let messages: Vec<&str> = logs.iter().map(|log| log.message.as_str()).collect();
    assert_eq!(
        messages,
        ["下载歌单「夜曲」里的「七里香」下载失败：未获取到可下载音频地址", "下载歌单「夜曲」里的「歌曲 185812」下载失败：网络超时"]
    );
    for log in &logs {
        assert_eq!((log.level.as_str(), log.category.as_str(), log.action.as_str()), ("error", "source", "downloadItemFailed"));
        assert_eq!(log.plugin_id.as_deref(), Some("momobako.netease.source"));
        assert_eq!(log.repo_id.as_deref(), Some("acceptance-repo"), "记在来源资源库名下");
        assert_eq!(log.source_kind.as_deref(), Some("host"));
    }
    assert_eq!(logs[1].context.as_ref().map(|context| context["item"]["songId"].clone()), Some(json!(185812)), "上下文带插件交回的原样内容");

    let row = model.operation.shown().expect("进度行");
    assert_eq!((row.label.as_str(), row.detail.as_str()), ("加载资源库", "读取已注册资源库"), "照 Vue context.refreshRepo() 刷新");
    assert!(model.workspace.refresh.as_ref().is_some_and(|run| run.keeps_status), "这一轮刷新是下载的收尾");
}

/// 收尾的刷新（读列表、同步、读摘要、重读当前目录）走完，提示还在：这一轮不是用户开始的新操作。刷新期间失败排在
/// 忙碌行前面。用户开始下一个操作（这里是定位文件）时照常清掉。
#[test]
fn the_notice_outlives_the_refresh_that_follows_the_download() {
    let mut model = netease();
    let run = download_playlist(&mut model);
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Ok(partly_failed()) }));
    let repositories = model.admin.repository_summaries.clone();
    assert!(!repositories.is_empty(), "场景里读过资源库列表");

    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::WorkspaceListLoaded { generation, result: Ok(repositories) });
    assert_eq!(notice(&model).map(|(_, message)| message).as_deref(), Some(NOTICE), "读完列表、开始同步时还在");
    assert_eq!(StatusLine::project(&model), StatusLine::Failure(NOTICE.into()), "失败排在忙碌行前面");
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced { generation, result: Ok(SyncCounts::default()) });
    model.reduce(ShellMessage::RepositorySnapshotLoaded { generation, result: Ok(snapshot("acceptance-repo")) });
    assert!(model.files.loading, "读完摘要非静默地重读当前目录");
    assert_eq!(notice(&model).map(|(_, message)| message).as_deref(), Some(NOTICE), "重读目录时还在");
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser("acceptance-repo"))));
    assert!(model.workspace.refresh.is_none(), "刷新走完了");
    assert_eq!(notice(&model), Some((FailureSource::Download, NOTICE.into())), "刷新走完提示还在");

    model.reduce(ShellMessage::Input(InputMessage::RevealEntry { absolute_path: "D:/MomoCache/netease/晴天.mp3".into() }));
    assert_eq!(notice(&model), None, "用户开始下一个操作时清掉");
}

/// 全部成功：不提示、不写日志，刷新照常是新操作，清掉以前的失败。
#[test]
fn a_fully_successful_download_says_nothing() {
    let mut model = netease();
    model.status.fail(FailureSource::Host, "定位失败：拒绝访问。");
    let run = download_playlist(&mut model);
    let result = json!({ "completed": [{ "paths": ["D:/stage/夜曲/周杰伦 - 晴天.mp3"] }], "failed": [] });
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Ok(result) }));
    assert_eq!(notice(&model), None, "全部成功不提示，刷新开始清掉以前的失败");
    assert!(model.admin.take_effects().iter().all(|effect| !matches!(effect, AdminEffect::WriteLogs(_))), "没有要写的日志");
    assert!(model.workspace.refresh.as_ref().is_some_and(|run| !run.keeps_status), "照常刷新");
}

/// 侧栏顶部的状态条画出提示，侧栏不重挂；和新挂的一样。
#[test]
fn the_sidebar_shows_the_notice() {
    let mut harness = ShellHarness::mount(netease());
    let sidebar = harness.sidebar_root();
    assert!(harness.find(NOTICE).is_none());
    let run = download_playlist(&mut harness.model);
    harness.apply(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Ok(partly_failed()) }));
    harness.flush();
    assert!(harness.find(NOTICE).is_some(), "提示画在侧栏顶部");
    assert_eq!(harness.sidebar_root(), sidebar, "侧栏不该重挂");
    harness.assert_same_as_fresh_mount();
}

/// 插件交回的失败条目：名称依次取歌曲名、名称、标题，都没有时是「歌曲 {标识}」或「第 N 项」；原因依次取 error、
/// message、reason，都没有时写明插件没有给出原因；整条是字符串时它就是原因。`failed` 不是数组时当作没有失败。
#[test]
fn failed_items_name_what_failed_and_why() {
    let payload = json!({
        "failed": [
            { "songName": " 七里香 ", "error": "版权" },
            { "name": "第二首", "message": "限流" },
            { "songId": "42", "reason": "地区限制" },
            { "error": "未知" },
            "整条只有原因",
            { "songId": 7 }
        ]
    });
    let items: Vec<(String, String)> = failed_items(&payload).into_iter().map(|item| (item.name, item.reason)).collect();
    assert_eq!(
        items,
        [
            ("七里香".to_string(), "版权".to_string()),
            ("第二首".into(), "限流".into()),
            ("歌曲 42".into(), "地区限制".into()),
            ("第 4 项".into(), "未知".into()),
            ("第 5 项".into(), "整条只有原因".into()),
            ("歌曲 7".into(), "插件没有给出原因".into()),
        ]
    );
    assert!(failed_items(&json!({ "failed": 3 })).is_empty());
    assert!(failed_items(&json!({ "paths": [] })).is_empty());
}
