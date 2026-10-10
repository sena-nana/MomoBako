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

use super::{failed_items, DownloadOutcome, DownloadRun};

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

/// 右键歌单目录「夜曲」→「下载歌单到其他资源库」→ 选第三资源库：交回场景和归约交给宿主的那次下载、载荷和导入到
/// 的文件夹。宿主一侧的测试从这里接着跑（`download_dispatch_tests.rs`）。
pub(crate) fn chosen_playlist_download() -> (ShellViewModel, DownloadRun, serde_json::Value, Option<String>) {
    let mut model = netease();
    click(&mut model, FOLDER, "下载歌单到其他资源库");
    model.reduce(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository("third-repo".into())));
    let (run, payload, parent_path) = model
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::DownloadToRepository { run, payload, parent_path } => Some((run, payload, parent_path)),
            _ => None,
        })
        .expect("开始下载到第三资源库");
    (model, run, payload, parent_path)
}

/// 文件列现在写着的错误（壳层以外的测试读不到 `FilesState` 的字段）。
pub(crate) fn file_column_error(model: &ShellViewModel) -> &str {
    &model.files.error
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

/// 只有插件载荷的下载结果（下载到本地，或者没有跳过同名文件）。
fn plugin(payload: serde_json::Value) -> Result<DownloadOutcome, String> {
    Ok(DownloadOutcome::plugin(payload))
}

/// 下载到其他资源库的结果：插件载荷加上导入时宿主交回的结果（`FileImportResponse` 的 JSON）。
fn imported(payload: serde_json::Value, import: serde_json::Value) -> Result<DownloadOutcome, String> {
    Ok(DownloadOutcome::imported(payload, &import))
}

/// 写进系统日志的那一批。
fn written_logs(model: &mut ShellViewModel) -> Vec<crate::backend::services::repository::SystemLogWriteRequest> {
    model
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::WriteLogs(logs) => Some(logs),
            _ => None,
        })
        .unwrap_or_default()
}

/// 两项失败：状态区写出几项失败、去哪里看原因；系统日志里每一项写一条，带上是哪一首、为什么失败、插件和来源资源库；
/// 照 Vue 接着刷新资源库，刷新的「加载资源库」占住进度行。
#[test]
fn a_partly_failed_download_says_how_many_failed_and_where_to_look() {
    let mut model = netease();
    let run = download_playlist(&mut model);
    assert_eq!((run.progress.label.as_str(), run.progress.subject.as_str()), ("下载歌单", FOLDER));
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(partly_failed()) }));

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
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(partly_failed()) }));
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
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(result) }));
    assert_eq!(notice(&model), None, "全部成功不提示，刷新开始清掉以前的失败");
    assert!(model.admin.take_effects().iter().all(|effect| !matches!(effect, AdminEffect::WriteLogs(_))), "没有要写的日志");
    assert!(model.workspace.refresh.as_ref().is_some_and(|run| !run.keeps_status), "照常刷新");
}

/// 再下一次同一个歌单：目标资源库里已有的同名文件保留原文件、不导入（导入带 `skipExisting`），其余照常导入。状态区
/// 用警告色写出目标资源库里已有几个同名文件，跳过的路径写一条警告日志，记在目标资源库名下。Vue 的导入遇到一个同名
/// 就整批失败，新下的文件一个也进不去。
#[test]
fn files_already_in_the_target_are_kept_and_reported() {
    let mut model = netease();
    let run = download_playlist(&mut model);
    let paths = ["D:/stage/夜曲/周杰伦 - 晴天.mp3", "D:/stage/夜曲/周杰伦 - 晴天.lrc", "D:/stage/夜曲/周杰伦 - 七里香.mp3"];
    let payload = json!({ "completed": [{ "paths": paths }], "failed": [] });
    let skipped = ["夜曲/周杰伦 - 晴天.mp3", "夜曲/周杰伦 - 晴天.lrc"];
    let import = json!({ "repoId": "third-repo", "currentPath": FOLDER, "entries": [], "skippedEntries": skipped });
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: imported(payload, import) }));

    let message = "下载歌单「夜曲」：目标资源库「第三资源库」已有 2 个同名文件，保留原文件、没有导入。";
    assert_eq!(StatusLine::project(&model), StatusLine::Warning(message.into()), "只有跳过时是提醒");
    let logs = written_logs(&mut model);
    assert_eq!(logs.len(), 1);
    assert_eq!((logs[0].level.as_str(), logs[0].action.as_str(), logs[0].message.as_str()), ("warn", "downloadImportSkipped", message));
    assert_eq!(logs[0].repo_id.as_deref(), Some("third-repo"), "记在目标资源库名下");
    assert_eq!(logs[0].context.as_ref().map(|context| context["skippedPaths"].clone()), Some(json!(skipped)));
    assert!(model.workspace.refresh.as_ref().is_some_and(|run| run.keeps_status), "收尾的刷新不清提醒");

    let mut clean = netease();
    let run = download_playlist(&mut clean);
    let import = json!({ "repoId": "third-repo", "currentPath": FOLDER, "entries": [] });
    let result = imported(json!({ "completed": [{ "paths": paths }], "failed": [] }), import);
    clean.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result }));
    assert_eq!(notice(&clean), None, "导入结果里没有跳过的，不提醒");
}

/// 右键 `path` →「{label}」→ 在「选择下载目录」里选 `folder`，返回宿主拿去调用插件的那一次下载（载荷的去处是这个
/// 本地目录）。
fn download_to_folder(model: &mut ShellViewModel, path: &str, label: &str, folder: &str) -> DownloadRun {
    click(model, path, label);
    let request_id = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false)
        .into_iter()
        .find_map(|command| match command {
            nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. } if request.title.as_deref() == Some("选择下载目录") => {
                Some(request.id)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("「{label}」没有打开「选择下载目录」"));
    model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted { request_id, paths: vec![folder.into()], failed: None }));
    model
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::CallPlugin { payload, origin: crate::shell::admin::PluginCallOrigin::Download(run), .. } => {
                assert_eq!(payload["destination"], json!({ "kind": "localFolder", "path": folder }));
                Some(run)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("「{label}」选好目录以后没有调用插件"))
}

/// 下载到本地目录：目录里已有的同名文件插件不覆盖（结果的 `skippedEntries`）。状态区照下载到其他资源库的样子用
/// 警告色写出已有几个同名文件、保留了原文件，保留的路径写一条警告日志，记在来源资源库名下；收尾的刷新不清它。
#[test]
fn files_already_in_the_local_folder_are_kept_and_reported() {
    let mut model = netease();
    let run = download_to_folder(&mut model, FOLDER, "下载歌单到本地", "D:/Music");
    let kept = ["D:/Music/夜曲/周杰伦 - 晴天.mp3", "D:/Music/夜曲/周杰伦 - 晴天.lrc"];
    let payload = json!({
        "completed": [
            { "songId": 186016, "paths": [], "skippedEntries": kept },
            { "songId": 185811, "paths": ["D:/Music/夜曲/周杰伦 - 七里香.mp3"] }
        ],
        "failed": []
    });
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(payload) }));

    let message = "下载歌单「夜曲」：下载目录里已有 2 个同名文件，保留原文件、没有写入。";
    assert_eq!(StatusLine::project(&model), StatusLine::Warning(message.into()), "只有保留时是提醒");
    let logs = written_logs(&mut model);
    assert_eq!(logs.len(), 1);
    assert_eq!((logs[0].level.as_str(), logs[0].action.as_str(), logs[0].message.as_str()), ("warn", "downloadLocalSkipped", message));
    assert_eq!(logs[0].repo_id.as_deref(), Some("acceptance-repo"), "下载到本地记在来源资源库名下");
    let context = logs[0].context.clone().unwrap_or_default();
    assert_eq!((context["skippedPaths"].clone(), context["folder"].clone()), (json!(kept), json!("D:/Music")));
    assert!(model.workspace.refresh.as_ref().is_some_and(|run| run.keeps_status), "收尾的刷新不清提醒");
}

/// 单曲下载到本地、文件全都已经在：照样是保留的提醒，不说成「没有下载到任何条目」；有失败的条目时并进失败那一句。
#[test]
fn a_track_kept_in_full_is_not_reported_as_an_empty_download() {
    let mut track = netease();
    let run = download_to_folder(&mut track, TRACK, "下载单曲到本地", "D:/Music");
    let payload = json!({ "songId": 186016, "paths": [], "skippedEntries": ["D:/Music/周杰伦 - 晴天.mp3"], "destination": "localFolder" });
    track.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(payload) }));
    let message = "下载单曲「晴天.mp3」：下载目录里已有 1 个同名文件，保留原文件、没有写入。";
    assert_eq!(StatusLine::project(&track), StatusLine::Warning(message.into()));

    let mut playlist = netease();
    let run = download_to_folder(&mut playlist, FOLDER, "下载歌单到本地", "D:/Music");
    let mut payload = partly_failed();
    payload["completed"][0]["skippedEntries"] = json!(["D:/Music/夜曲/周杰伦 - 晴天.lrc"]);
    playlist.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(payload) }));
    let message = "下载歌单「夜曲」有 2 项失败，原因见「日志」；下载目录里已有 1 个同名文件，保留原文件、没有写入。";
    assert_eq!(StatusLine::project(&playlist), StatusLine::Failure(message.into()));
    let actions: Vec<String> = written_logs(&mut playlist).into_iter().map(|log| log.action).collect();
    assert_eq!(actions, ["downloadItemFailed", "downloadItemFailed", "downloadLocalSkipped"]);
}

/// 插件报错（取不到歌单的曲目、登录失效、网络超时）时：进度行收起，文件列写明是哪次下载、为什么失败，原因是插件
/// 交回的原话；状态区不重复，也不刷新资源库。插件没给原因时写明没有原因。
#[test]
fn a_failed_download_says_which_download_failed_and_why() {
    let mut model = netease();
    let run = download_to_folder(&mut model, FOLDER, "下载歌单到本地", "D:/Music");
    model.workspace.take_effects();
    let reason = "网易云上没有歌单 9001，可能已被删除或设为私密";
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Err(reason.into()) }));
    assert!(model.operation.shown().is_none(), "失败时进度行收起");
    assert_eq!(model.files.error, format!("下载歌单「夜曲」失败：{reason}"));
    assert_eq!(StatusLine::project(&model), StatusLine::Hidden, "文件列已经写着，状态区不重复");
    assert!(model.workspace.refresh.is_none(), "失败不刷新资源库");

    let mut silent = netease();
    let run = download_to_folder(&mut silent, TRACK, "下载单曲到本地", "D:/Music");
    silent.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: Err("  ".into()) }));
    assert_eq!(silent.files.error, "下载单曲「晴天.mp3」失败：插件没有给出原因。");
}

/// 有失败的条目，又跳过了同名文件：一句话里两样都写，按失败显示，日志里各自一条。
#[test]
fn failures_and_skipped_files_share_one_notice() {
    let mut model = netease();
    let run = download_playlist(&mut model);
    let import = json!({ "repoId": "third-repo", "currentPath": FOLDER, "entries": [], "skippedEntries": ["夜曲/周杰伦 - 晴天.mp3"] });
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: imported(partly_failed(), import) }));
    let message = "下载歌单「夜曲」有 2 项失败，原因见「日志」；目标资源库「第三资源库」已有 1 个同名文件，保留原文件、没有导入。";
    assert_eq!(StatusLine::project(&model), StatusLine::Failure(message.into()));
    let actions: Vec<String> = written_logs(&mut model).into_iter().map(|log| log.action).collect();
    assert_eq!(actions, ["downloadItemFailed", "downloadItemFailed", "downloadImportSkipped"]);
}

/// 下载歌单不列目录：选好目标以后任务弹层和单曲一样写「准备下载 夜曲」→「正在下载 夜曲」，载荷里是歌单标识和
/// 歌单名、没有 `tracks`，由插件按歌单标识取来源里完整的歌单。Vue 先分页列出目录、写「正在处理 N 个文件」。
#[test]
fn downloading_a_playlist_asks_the_source_for_the_whole_playlist() {
    let mut model = netease();
    click(&mut model, FOLDER, "下载歌单到其他资源库");
    model.reduce(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository("third-repo".into())));
    let row = model.operation.shown().expect("进度行");
    assert_eq!((row.label.as_str(), row.detail.as_str(), row.value, row.indeterminate), ("下载歌单", "正在下载 夜曲", 36.0, true));
    let effects = model.admin.take_effects();
    let payload = effects
        .iter()
        .find_map(|effect| match effect {
            AdminEffect::DownloadToRepository { payload, parent_path, .. } => {
                assert_eq!(parent_path.as_deref(), Some(FOLDER), "导入到以歌单命名的文件夹");
                Some(payload.clone())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("没有下载到其他资源库：{effects:?}"));
    assert_eq!((payload["playlistId"].clone(), payload["playlistName"].clone()), (json!(9001), json!(FOLDER)));
    assert!(payload.get("tracks").is_none(), "不列目录、不带曲目");
}

/// 插件交回的是一批、却一项也没有（网易云取不到歌单详情时什么也不下）：状态区用警告色提醒没有下载到任何条目，写一条
/// 警告日志，收尾的刷新不清它。单曲交回文件、批量结果有完成的条目时都不提醒。
#[test]
fn an_empty_batch_is_called_out_as_a_warning() {
    let mut model = netease();
    let run = download_playlist(&mut model);
    let empty = json!({ "completed": [], "failed": [], "summary": { "total": 0, "succeeded": 0, "failed": 0 } });
    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(empty) }));
    let warning = "下载歌单「夜曲」没有下载到任何条目。";
    assert_eq!(StatusLine::project(&model), StatusLine::Warning(warning.into()));
    assert_eq!(notice(&model), Some((FailureSource::Download, warning.into())));
    let logs = model
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::WriteLogs(logs) => Some(logs),
            _ => None,
        })
        .expect("写一条日志");
    assert_eq!(logs.len(), 1);
    assert_eq!((logs[0].level.as_str(), logs[0].action.as_str(), logs[0].message.as_str()), ("warn", "downloadEmpty", warning));
    assert!(model.workspace.refresh.as_ref().is_some_and(|run| run.keeps_status));

    let mut track = netease();
    click(&mut track, TRACK, "下载单曲到其他资源库");
    track.reduce(ShellMessage::Admin(AdminMessage::ChooseDownloadRepository("third-repo".into())));
    let run = track
        .admin
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            AdminEffect::DownloadToRepository { run, .. } => Some(run),
            _ => None,
        })
        .expect("开始下载单曲");
    let single = json!({ "songId": 186016, "paths": ["D:/stage/周杰伦 - 晴天.mp3"], "destination": "repository" });
    track.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(single) }));
    assert_eq!(notice(&track), None, "单曲交回了文件，不提醒");
}

/// 提醒画成警告色的提醒条，失败条藏着；侧栏不重挂，和新挂的一样。
#[test]
fn the_sidebar_shows_the_warning_in_its_own_tone() {
    let mut harness = ShellHarness::mount(netease());
    let run = download_playlist(&mut harness.model);
    harness.apply(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(json!({ "completed": [], "failed": [] })) }));
    harness.flush();
    assert!(harness.find("下载歌单「夜曲」没有下载到任何条目。").is_some(), "提醒画在侧栏顶部");
    let hidden = |key: &str| {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("侧栏缺少 {key}"));
        harness.document().context().world().node_style(id).is_some_and(|style| style.layout.hidden)
    };
    assert!(!hidden("sidebar-status-warning"), "提醒条显示着");
    assert!(hidden("sidebar-status"), "失败条藏着");
    harness.assert_same_as_fresh_mount();
}

/// 侧栏顶部的状态条画出提示，侧栏不重挂；和新挂的一样。
#[test]
fn the_sidebar_shows_the_notice() {
    let mut harness = ShellHarness::mount(netease());
    let sidebar = harness.sidebar_root();
    assert!(harness.find(NOTICE).is_none());
    let run = download_playlist(&mut harness.model);
    harness.apply(ShellMessage::Admin(AdminMessage::DownloadFinished { run, result: plugin(partly_failed()) }));
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
