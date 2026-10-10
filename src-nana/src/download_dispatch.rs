//! 来源「下载到其他资源库」的宿主一侧（Vue `runDownloadAction` 的 `writable-repository`）。
//!
//! 给这次下载新建一个暂存目录（`download_staging.rs`），写进载荷的 `destination.stagingPath` 再调用插件；把下载回来
//! 的文件（`paths` 和 `completed[].paths`）导入目标资源库，下载歌单时导入以歌单命名的文件夹，目标里还没有这个文件夹
//! 就先建（Vue 不建，导入报「directory not found」）。导入带 `skipExisting`：目标里已有同名文件的保留原文件、不导入，
//! 其余照常导入（Vue 的导入遇到一个同名就整批失败）。导入完成（包括跳过）以后清掉这次暂存的文件，再同步目标资源库；
//! 导入失败时暂存的文件留着，位置写进系统日志。哪一步失败都把原因交回，不留在进行中。

use std::path::Path;
use std::sync::Arc;

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;
use serde_json::{json, Value};

use crate::backend::services::mutsuki_runner::{PROTOCOL_ENTRY_IMPORT, PROTOCOL_REPOSITORY_SYNC};
use crate::backend::services::repository::{
    FileCreateRequest, FileImportRequest, PluginCallRequest, SyncRequest, SystemLogLocationInput, SystemLogWriteRequest,
};
use crate::backend::viewmodels::{FileBrowserViewModel, MutsukiTaskViewModel, PluginViewModel, SystemViewModel};
use crate::download_staging::{staging_root, Cleanup, StagingDir};
use crate::shell::admin::source_download::{DownloadOutcome, DownloadRun};
use crate::shell::admin::source_target::{result_paths, TargetCandidate};
use crate::shell::admin::AdminMessage;
use crate::shell::ShellMessage;
use crate::MomoBakoApplication;

/// 下载到其他资源库要用的领域服务。产品里是 `NativeServices` 的 ViewModel（[`LiveServices`]），测试里换成假的。
pub(crate) trait DownloadServices {
    /// 调用来源插件，交回插件结果的载荷。
    fn call_plugin(&self, request: PluginCallRequest) -> Result<Value, String>;
    /// 在目标资源库的根目录下建一个文件夹。
    fn create_folder(&self, repo_id: &str, name: &str) -> Result<(), String>;
    /// 把文件导入目标资源库，交回导入结果（`FileImportResponse` 的 JSON）。
    fn import(&self, request: FileImportRequest) -> Result<Value, String>;
    /// 同步目标资源库。
    fn sync(&self, repo_id: &str) -> Result<(), String>;
    /// 写一条系统日志。写不进去时自己记标准错误。
    fn log(&self, request: SystemLogWriteRequest);
}

/// 来源下载到其他资源库：交给任务线程做完一整趟（[`run`]），结果回 [`AdminMessage::DownloadFinished`]。没有目标
/// 资源库、服务没启动或者任务提交失败时当场按失败交回。
pub(crate) fn download_to_repository(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    run: DownloadRun,
    payload: Value,
    parent_path: Option<String>,
) {
    let Some(target) = run.target().cloned() else {
        eprintln!("Nana 下载到其他资源库没有目标资源库：{}", run.method);
        app.shell.reduce(finished(run, Err("没有选定目标资源库。".into())));
        return;
    };
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 来源下载需要领域服务，当前服务未启动：{}", run.method);
        app.shell.reduce(finished(run, Err("领域服务未启动".into())));
        return;
    };
    let live = LiveServices {
        plugin: services.plugin.clone(),
        browser: services.file_browser.clone(),
        tasks: services.tasks.clone(),
        system: services.system.clone(),
        executor: services.executor.clone(),
    };
    let managed_cache_root = payload.get("managedCacheRoot").and_then(Value::as_str);
    let root = staging_root(managed_cache_root, &services.runtime.service_root(), &target.repo_id);
    let failed_run = run.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = self::run(&live, &run, &target, payload, parent_path, &root);
        finished(run, result)
    })) {
        eprintln!("Nana 来源下载任务提交失败：{error}");
        app.shell.reduce(finished(failed_run, Err(format!("来源下载任务提交失败：{error}"))));
    }
}

fn finished(run: DownloadRun, result: Result<DownloadOutcome, String>) -> ShellMessage {
    ShellMessage::Admin(AdminMessage::DownloadFinished { run, result })
}

/// 下载到其他资源库的一整趟：建暂存目录、调用插件、建文件夹、导入、清暂存、同步。
pub(crate) fn run(
    services: &impl DownloadServices,
    run: &DownloadRun,
    target: &TargetCandidate,
    mut payload: Value,
    parent_path: Option<String>,
    staging_root: &Path,
) -> Result<DownloadOutcome, String> {
    let staging = StagingDir::create(staging_root).inspect_err(|error| eprintln!("Nana {error}"))?;
    if let Some(destination) = payload.get_mut("destination").and_then(Value::as_object_mut) {
        destination.insert("stagingPath".into(), json!(staging.path().to_string_lossy()));
    }
    let call = PluginCallRequest {
        plugin_id: run.plugin_id.clone(),
        method: run.method.clone(),
        repository_id: run.repository_id.clone(),
        payload,
    };
    let response = match services.call_plugin(call) {
        Ok(response) => response,
        Err(error) => {
            discard(services, run, &staging);
            return Err(error);
        }
    };
    let paths = result_paths(&response);
    if paths.is_empty() {
        eprintln!("Nana 来源下载没有交回文件，不导入：{}", run.method);
        discard(services, run, &staging);
        return Ok(DownloadOutcome::plugin(response));
    }
    let imported = match import(services, target, &paths, parent_path) {
        Ok(imported) => imported,
        Err(error) => {
            keep(services, run, target, &staging, &paths, &error);
            return Err(format!("{error}。下载的文件留在暂存目录，位置见「日志」"));
        }
    };
    let outcome = DownloadOutcome::imported(response, &imported);
    if !outcome.skipped.is_empty() {
        eprintln!("Nana 目标资源库已有同名文件，跳过 {} 个：{}", outcome.skipped.len(), target.repo_id);
    }
    report_cleanup(services, run, &staging, staging.clear(&paths));
    services.sync(&target.repo_id).map_err(|error| {
        eprintln!("Nana 同步目标资源库失败：{}：{error}", target.repo_id);
        format!("同步目标资源库失败：{error}")
    })?;
    Ok(outcome)
}

/// 把下载回来的文件导入目标资源库：下载歌单时先确保以歌单命名的文件夹在，再带 `skipExisting` 导入。
fn import(
    services: &impl DownloadServices,
    target: &TargetCandidate,
    paths: &[String],
    parent_path: Option<String>,
) -> Result<Value, String> {
    if let Some(folder) = parent_path.as_deref() {
        if !Path::new(&target.path).join(folder).is_dir() {
            services.create_folder(&target.repo_id, folder).map_err(|error| {
                eprintln!("Nana 在目标资源库建文件夹失败：{folder}：{error}");
                format!("在目标资源库建文件夹失败：{error}")
            })?;
        }
    }
    let request = FileImportRequest { repo_id: target.repo_id.clone(), parent_path, source_paths: paths.to_vec(), skip_existing: true };
    services.import(request).map_err(|error| {
        eprintln!("Nana 下载的文件导入目标资源库失败：{}：{error}", target.repo_id);
        format!("导入目标资源库失败：{error}")
    })
}

/// 插件失败或者什么也没下到：暂存目录是空的就收掉；还有东西（插件写了却没报告）就留着，写一条日志。
fn discard(services: &impl DownloadServices, run: &DownloadRun, staging: &StagingDir) {
    let left = staging.discard_if_empty();
    if left {
        let message = format!("{}「{}」的暂存目录里还有没交回的文件，没有清理：{}", run.progress.label, run.progress.subject, staging.path().display());
        eprintln!("Nana {message}");
        services.log(log(run, "warn", "downloadStagingLeft", message, None, json!({ "stagingPath": staging.path() })));
    }
}

/// 导入没有成功：暂存的文件留着，位置写进日志，方便用户找回。
fn keep(services: &impl DownloadServices, run: &DownloadRun, target: &TargetCandidate, staging: &StagingDir, paths: &[String], error: &str) {
    let message = format!(
        "{}「{}」没有导入目标资源库「{}」，下载的 {} 个文件留在暂存目录 {}，可以从这里找回。",
        run.progress.label,
        run.progress.subject,
        target.name,
        paths.len(),
        staging.path().display()
    );
    eprintln!("Nana {message}");
    let context = json!({ "stagingPath": staging.path(), "paths": paths, "error": error, "targetRepoId": target.repo_id });
    services.log(log(run, "warn", "downloadStagingKept", message, Some(target.repo_id.clone()), context));
}

/// 清理暂存以后没清干净的写日志：插件报告了暂存目录以外的文件（不删），或者有删不掉的。
fn report_cleanup(services: &impl DownloadServices, run: &DownloadRun, staging: &StagingDir, cleanup: Cleanup) {
    if !cleanup.outside.is_empty() {
        let message = format!(
            "{}「{}」交回的 {} 个文件不在这次下载的暂存目录里，宿主不清理它们。",
            run.progress.label,
            run.progress.subject,
            cleanup.outside.len()
        );
        eprintln!("Nana {message}");
        let context = json!({ "stagingPath": staging.path(), "paths": cleanup.outside });
        services.log(log(run, "warn", "downloadStagingOutside", message, None, context));
    }
    if cleanup.left || !cleanup.failed.is_empty() {
        let failed: Vec<Value> = cleanup.failed.iter().map(|(path, error)| json!({ "path": path, "error": error })).collect();
        let message = format!("{}「{}」的暂存目录没有清干净：{}", run.progress.label, run.progress.subject, staging.path().display());
        eprintln!("Nana {message}");
        services.log(log(run, "warn", "downloadStagingLeft", message, None, json!({ "stagingPath": staging.path(), "failed": failed })));
    }
}

/// 宿主写的一条暂存日志：分类 `source`，记在插件名下，资源库默认是来源资源库。
fn log(run: &DownloadRun, level: &str, action: &str, message: String, repo_id: Option<String>, context: Value) -> SystemLogWriteRequest {
    SystemLogWriteRequest {
        level: level.into(),
        category: "source".into(),
        action: action.into(),
        message,
        context: Some(context),
        repo_id: repo_id.or_else(|| run.repository_id.clone()),
        plugin_id: Some(run.plugin_id.clone()),
        source_kind: Some("host".into()),
        source_label: None,
        location: Some(SystemLogLocationInput { module_path: Some(module_path!().into()), file: Some(file!().into()), line: Some(line!()) }),
    }
}

/// 产品里的领域服务：都在任务线程里同步等结果。
struct LiveServices {
    plugin: PluginViewModel,
    browser: FileBrowserViewModel,
    tasks: MutsukiTaskViewModel,
    system: SystemViewModel,
    executor: Arc<tokio::runtime::Runtime>,
}

impl DownloadServices for LiveServices {
    fn call_plugin(&self, request: PluginCallRequest) -> Result<Value, String> {
        self.executor.block_on(self.plugin.call_plugin(request)).map(|response| response.payload)
    }

    fn create_folder(&self, repo_id: &str, name: &str) -> Result<(), String> {
        let request = FileCreateRequest { repo_id: repo_id.to_string(), parent_path: None, name: name.to_string() };
        self.executor.block_on(self.browser.create_directory(request)).map(|_| ())
    }

    fn import(&self, request: FileImportRequest) -> Result<Value, String> {
        self.executor.block_on(self.tasks.execute(PROTOCOL_ENTRY_IMPORT, request)).map(|(output, _)| output)
    }

    fn sync(&self, repo_id: &str) -> Result<(), String> {
        let request = SyncRequest { repo_id: repo_id.to_string() };
        self.executor.block_on(self.tasks.execute(PROTOCOL_REPOSITORY_SYNC, request)).map(|_| ())
    }

    fn log(&self, request: SystemLogWriteRequest) {
        let action = request.action.clone();
        if let Err(error) = self.executor.block_on(self.system.write_system_log(request)) {
            eprintln!("Nana 写系统日志失败：{action}：{error}");
        }
    }
}

#[cfg(test)]
#[path = "download_dispatch_tests.rs"]
mod tests;
