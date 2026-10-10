//! 下载到其他资源库的一整趟：从网易云资源库的右键「下载歌单到其他资源库」出发、选好目标资源库，拿归约交给宿主的那次
//! 下载跑 [`run`]。领域服务换成假的（插件照真插件的样子把文件写进宿主给的暂存目录），文件系统是真的临时目录；断言
//! 暂存目录清没清、别处的文件在不在、导入和同步的请求、日志，再把结果交回归约看界面。

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::{run, DownloadServices};
use crate::backend::services::repository::{FileImportRequest, PluginCallRequest, SystemLogWriteRequest};
use crate::download_staging::tests::Scratch;
use crate::shell::admin::source_download::tests::{chosen_playlist_download as chosen_download, file_column_error};
use crate::shell::admin::source_download::DownloadRun;
use crate::shell::admin::source_target::TargetCandidate;
use crate::shell::admin::AdminMessage;
use crate::shell::status::StatusLine;
use crate::shell::ShellMessage;

/// 加进根目录的歌单目录。
const FOLDER: &str = "夜曲";

/// 假的领域服务：插件按 `plugin` 写暂存文件、交回结果；导入、建文件夹、同步交回给定的结果；请求和日志都记下来。
struct FakeServices {
    plugin: Box<dyn Fn(&Path) -> Result<Value, String>>,
    import_result: Result<Value, String>,
    folder_result: Result<(), String>,
    calls: RefCell<Vec<PluginCallRequest>>,
    folders: RefCell<Vec<String>>,
    imports: RefCell<Vec<FileImportRequest>>,
    syncs: RefCell<Vec<String>>,
    logs: RefCell<Vec<SystemLogWriteRequest>>,
}

impl FakeServices {
    fn new(plugin: impl Fn(&Path) -> Result<Value, String> + 'static) -> Self {
        Self {
            plugin: Box::new(plugin),
            import_result: Ok(json!({ "repoId": "third-repo", "entries": [] })),
            folder_result: Ok(()),
            calls: RefCell::new(Vec::new()),
            folders: RefCell::new(Vec::new()),
            imports: RefCell::new(Vec::new()),
            syncs: RefCell::new(Vec::new()),
            logs: RefCell::new(Vec::new()),
        }
    }

    fn log_actions(&self) -> Vec<String> {
        self.logs.borrow().iter().map(|log| log.action.clone()).collect()
    }
}

impl DownloadServices for FakeServices {
    fn call_plugin(&self, request: PluginCallRequest) -> Result<Value, String> {
        let staging = request.payload["destination"]["stagingPath"].as_str().map(PathBuf::from).expect("载荷里有暂存目录");
        assert!(staging.is_dir(), "宿主先建好暂存目录");
        assert_eq!(fs::read_dir(&staging).expect("读暂存目录").count(), 0, "交给插件的暂存目录是空的");
        self.calls.borrow_mut().push(request);
        (self.plugin)(&staging)
    }

    fn create_folder(&self, _repo_id: &str, name: &str) -> Result<(), String> {
        self.folders.borrow_mut().push(name.to_string());
        self.folder_result.clone()
    }

    fn import(&self, request: FileImportRequest) -> Result<Value, String> {
        self.imports.borrow_mut().push(request);
        self.import_result.clone()
    }

    fn sync(&self, repo_id: &str) -> Result<(), String> {
        self.syncs.borrow_mut().push(repo_id.to_string());
        Ok(())
    }

    fn log(&self, request: SystemLogWriteRequest) {
        self.logs.borrow_mut().push(request);
    }
}

/// 照网易云插件把歌单写进暂存目录的样子：`{暂存目录}/夜曲/` 下写这些文件，交回 `completed[].paths`。
fn stage_playlist(staging: &Path, names: &[&str]) -> Value {
    let folder = staging.join(FOLDER);
    fs::create_dir_all(&folder).expect("歌单文件夹");
    let paths: Vec<String> = names
        .iter()
        .map(|name| {
            let path = folder.join(name);
            fs::write(&path, name).expect("暂存文件");
            path.to_string_lossy().to_string()
        })
        .collect();
    json!({ "completed": [{ "paths": paths }], "failed": [] })
}

/// 目标资源库换成临时目录里的根目录（歌单文件夹还没有，要先建）。
fn target(run: &DownloadRun, scratch: &Scratch) -> TargetCandidate {
    let root = scratch.path().join("third");
    fs::create_dir_all(&root).expect("目标资源库根目录");
    TargetCandidate { path: root.to_string_lossy().to_string(), ..run.target().cloned().expect("目标资源库") }
}

/// 导入完成（跳过一个已有的同名文件）以后清掉这次暂存的文件和目录：别的下载留下的、旧的暂存文件、暂存目录以外的
/// 文件（插件报告了也不删）都还在，后者记一条警告。导入带 `skipExisting`、导入到歌单文件夹（先建），再同步；界面
/// 照常报跳过的条数。
#[test]
fn a_finished_import_clears_only_what_this_download_staged() {
    let scratch = Scratch::new("dispatch-cleared");
    let (mut model, run_info, payload, parent_path) = chosen_download();
    let root = scratch.path().join("staging");
    let older = scratch.file("staging/older-run/夜曲/周杰伦 - 晴天.mp3", "上次导入失败留下的");
    let legacy = scratch.file("staging/旧暂存.mp3", "插件旧的暂存文件");
    let user_file = scratch.file("Music/用户自己的.mp3", "用户自己的文件");
    let user_path = user_file.to_string_lossy().to_string();
    let services = FakeServices {
        import_result: Ok(json!({ "repoId": "third-repo", "entries": [], "skippedEntries": ["夜曲/周杰伦 - 晴天.mp3"] })),
        ..FakeServices::new(move |staging| {
            let mut result = stage_playlist(staging, &["周杰伦 - 晴天.mp3", "周杰伦 - 晴天.lrc", "周杰伦 - 七里香.mp3"]);
            result["completed"][0]["paths"].as_array_mut().expect("paths").push(json!(user_path));
            Ok(result)
        })
    };
    let target = target(&run_info, &scratch);

    let result = run(&services, &run_info, &target, payload, parent_path, &root);

    let outcome = result.clone().expect("下载完成");
    assert_eq!(outcome.skipped, ["夜曲/周杰伦 - 晴天.mp3"]);
    let staged = services.calls.borrow()[0].payload["destination"]["stagingPath"].as_str().map(PathBuf::from).expect("暂存目录");
    assert!(staged.starts_with(&root));
    assert!(!staged.exists(), "这次的暂存目录清掉了（包括目标里已有、跳过导入的那个文件）");
    assert_eq!(fs::read_to_string(&older).expect("别的下载留下的还在"), "上次导入失败留下的");
    assert_eq!(fs::read_to_string(&legacy).expect("旧暂存文件还在"), "插件旧的暂存文件");
    assert_eq!(fs::read_to_string(&user_file).expect("暂存目录以外的文件还在"), "用户自己的文件");

    assert_eq!(*services.folders.borrow(), [FOLDER], "目标里还没有歌单文件夹，先建");
    let imports = services.imports.borrow();
    assert_eq!((imports.len(), imports[0].skip_existing, imports[0].parent_path.as_deref()), (1, true, Some(FOLDER)));
    assert_eq!(imports[0].source_paths.len(), 4);
    assert_eq!(*services.syncs.borrow(), ["third-repo"]);
    assert_eq!(services.log_actions(), ["downloadStagingOutside"], "暂存目录以外的文件不删，记一条");
    assert_eq!(services.logs.borrow()[0].context.as_ref().map(|context| context["paths"].clone()), Some(json!([user_file.to_string_lossy()])));

    model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run: run_info, result }));
    let message = "下载歌单「夜曲」：目标资源库「第三资源库」已有 1 个同名文件，保留原文件、没有导入。";
    assert_eq!(StatusLine::project(&model), StatusLine::Warning(message.into()));
}

/// 导入失败：暂存的文件原样留着，位置和文件写进日志（记在目标资源库名下），不同步；文件列写明下载失败、文件留在
/// 暂存目录。建歌单文件夹失败也一样。
#[test]
fn a_failed_import_keeps_the_staged_files_and_logs_where_they_are() {
    for (folder_fails, reason) in [(false, "导入目标资源库失败：磁盘已满"), (true, "在目标资源库建文件夹失败：拒绝访问")] {
        let scratch = Scratch::new("dispatch-kept");
        let (mut model, run_info, payload, parent_path) = chosen_download();
        let root = scratch.path().join("staging");
        let mut services = FakeServices::new(|staging| Ok(stage_playlist(staging, &["周杰伦 - 晴天.mp3", "周杰伦 - 晴天.lrc"])));
        if folder_fails {
            services.folder_result = Err("拒绝访问".into());
        } else {
            services.import_result = Err("磁盘已满".into());
        }
        let target = target(&run_info, &scratch);

        let result = run(&services, &run_info, &target, payload, parent_path, &root);

        let error = result.clone().expect_err("导入失败");
        assert_eq!(error, format!("{reason}。下载的文件留在暂存目录，位置见「日志」"));
        let staged = services.calls.borrow()[0].payload["destination"]["stagingPath"].as_str().map(PathBuf::from).expect("暂存目录");
        assert!(staged.join(FOLDER).join("周杰伦 - 晴天.mp3").is_file() && staged.join(FOLDER).join("周杰伦 - 晴天.lrc").is_file());
        assert!(services.syncs.borrow().is_empty(), "导入没成功不同步");
        let logs = services.logs.borrow();
        assert_eq!(logs.len(), 1);
        assert_eq!((logs[0].action.as_str(), logs[0].repo_id.as_deref()), ("downloadStagingKept", Some("third-repo")));
        assert!(logs[0].message.contains(&staged.display().to_string()), "日志写明位置：{}", logs[0].message);
        assert_eq!(logs[0].context.as_ref().map(|context| context["paths"].as_array().map(Vec::len)), Some(Some(2)));
        drop(logs);

        model.reduce(ShellMessage::Admin(AdminMessage::DownloadFinished { run: run_info, result }));
        assert_eq!(file_column_error(&model), format!("下载歌单「夜曲」失败：{error}"));
    }
}

/// 插件失败或者什么也没下到：空的暂存目录收掉，不导入；插件写了却没报告的东西留着，记一条。
#[test]
fn a_failed_or_empty_call_leaves_no_staging_folder_behind() {
    let scratch = Scratch::new("dispatch-discard");
    let (_, run_info, payload, parent_path) = chosen_download();
    let root = scratch.path().join("staging");
    let target = target(&run_info, &scratch);

    let failing = FakeServices::new(|_| Err("网易云登录已失效，请重新登录".into()));
    let error = run(&failing, &run_info, &target, payload.clone(), parent_path.clone(), &root).expect_err("插件失败");
    assert_eq!(error, "网易云登录已失效，请重新登录");
    assert_eq!(fs::read_dir(&root).expect("暂存根目录").count(), 0, "空的暂存目录收掉了");
    assert!(failing.imports.borrow().is_empty() && failing.logs.borrow().is_empty());

    let empty = FakeServices::new(|_| Ok(json!({ "completed": [], "failed": [] })));
    let outcome = run(&empty, &run_info, &target, payload.clone(), parent_path.clone(), &root).expect("空结果");
    assert_eq!(outcome.payload, json!({ "completed": [], "failed": [] }));
    assert_eq!(fs::read_dir(&root).expect("暂存根目录").count(), 0);
    assert!(empty.imports.borrow().is_empty() && empty.syncs.borrow().is_empty());

    let unreported = FakeServices::new(|staging| {
        fs::write(staging.join("半截.mp3.download"), "x").expect("没报告的文件");
        Err("网络超时".into())
    });
    run(&unreported, &run_info, &target, payload, parent_path, &root).expect_err("插件失败");
    assert_eq!(unreported.log_actions(), ["downloadStagingLeft"]);
    assert_eq!(fs::read_dir(&root).expect("暂存根目录").count(), 1, "有东西的暂存目录留着");
}

/// 暂存目录建不起来时不调用插件，原因交回。
#[test]
fn no_staging_folder_means_no_download() {
    let scratch = Scratch::new("dispatch-blocked");
    let (_, run_info, payload, parent_path) = chosen_download();
    let blocked = scratch.file("staging", "暂存根目录的位置是个文件");
    let services = FakeServices::new(|_| panic!("不该调用插件"));
    let error = run(&services, &run_info, &target(&run_info, &scratch), payload, parent_path, &blocked).expect_err("建不了");
    assert!(error.starts_with("建不了下载暂存目录"), "{error}");
    assert!(services.calls.borrow().is_empty());
}
