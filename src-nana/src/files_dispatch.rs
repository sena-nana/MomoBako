//! 把文件浏览副作用交给已有的文件浏览 ViewModel 和 Mutsuki 协议。
//!
//! 新建、重命名和回收站变更返回目录快照。复制、移动、导入和删除只返回协议结果，
//! 成功后再重新读取目录，不用协议 JSON 改写本地行。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::{
    PROTOCOL_ARCHIVE_IMPORT, PROTOCOL_EAGLE_IMPORT, PROTOCOL_ENTRY_COPY, PROTOCOL_ENTRY_DELETE,
    PROTOCOL_ENTRY_IMPORT, PROTOCOL_ENTRY_MOVE,
};
use crate::backend::services::repository::{
    EagleLibraryImportRequest, FileArchiveImportRequest, FileBrowserRequest, FileCopyRequest, FileCreateRequest,
    FileDeleteRequest, FileImportRequest, FileMoveRequest, FileRenameRequest, HardlinkConfirmRequest,
    TrashMutationRequest,
};
use crate::shell::{
    display_mode_path, FilesEffect, FilesMessage, HardlinkPrompt, ShellMessage,
};
use crate::MomoBakoApplication;

/// 执行文件归约留下的请求。提交失败时把错误写回状态，避免按钮停在进行中。
pub fn dispatch_files_effects(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    for effect in app.shell.files.take_effects() {
        match effect {
            FilesEffect::Browse { repo_id, path, trash, offset, limit, .. } => {
                dispatch_browse(app, context, repo_id, path, trash, offset, limit);
            }
            FilesEffect::CreateDirectory { repo_id, parent, name } => {
                dispatch_create(app, context, repo_id, parent, name, true);
            }
            FilesEffect::CreateFile { repo_id, parent, name } => {
                dispatch_create(app, context, repo_id, parent, name, false);
            }
            FilesEffect::Rename { repo_id, path, new_name } => dispatch_rename(app, context, repo_id, path, new_name),
            FilesEffect::Copy { repo_id, sources, parent } => dispatch_copy(app, context, repo_id, sources, parent),
            FilesEffect::Move { repo_id, sources, parent } => dispatch_move(app, context, repo_id, sources, parent),
            FilesEffect::Import { repo_id, parent, sources } => dispatch_import(app, context, repo_id, parent, sources),
            FilesEffect::ImportArchive { repo_id, parent, archive_path } => {
                dispatch_archive(app, context, repo_id, parent, archive_path);
            }
            FilesEffect::ImportEagle { repo_id, parent, library_path, mode } => {
                dispatch_eagle(app, context, repo_id, parent, library_path, mode);
            }
            FilesEffect::Delete { repo_id, paths, mode } => dispatch_delete(app, context, repo_id, paths, mode),
            FilesEffect::MutateTrash { repo_id, action, paths } => dispatch_trash(app, context, repo_id, action, paths),
            FilesEffect::LoadHardlinks { repo_id } => dispatch_hardlinks(app, context, repo_id, false),
            FilesEffect::RefreshHardlinks { repo_id } => dispatch_hardlinks(app, context, repo_id, true),
            FilesEffect::ConfirmHardlink { repo_id, candidate_id } => {
                dispatch_confirm_hardlink(app, context, repo_id, candidate_id);
            }
            FilesEffect::LoadAsset { repo_id, asset_id } => dispatch_asset(app, context, repo_id, asset_id),
            FilesEffect::PersistDisplayMode => app.shell.files.save_display_mode_file(&display_mode_path()),
            FilesEffect::DecodeThumbnails { paths } => dispatch_thumbnails(context, paths),
        }
    }
}

fn dispatch_thumbnails(context: &RuntimeProgramContext<ShellMessage>, paths: Vec<String>) {
    if let Err(error) = context.run_task(Task::new(async move {
        let mut frames = Vec::new();
        for path in paths {
            match crate::shell::decode_thumbnail_file(&path) {
                Ok(frame) => frames.push(frame),
                Err(error) => eprintln!("Nana 缩略图解码失败：{path}：{error}"),
            }
        }
        ShellMessage::ThumbnailPixels(frames)
    })) {
        eprintln!("Nana 缩略图解码任务提交失败：{error}");
    }
}

fn dispatch_browse(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    trash: bool,
    offset: usize,
    limit: usize,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 文件浏览需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::FileBrowserLoaded(Err("领域服务未启动".into())));
        return;
    };
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let request = FileBrowserRequest {
        repo_id,
        directory_path: if trash || path.is_empty() { None } else { Some(path) },
        include_tree: Some(false),
        special_location: if trash { Some("trash".into()) } else { None },
        offset: Some(offset),
        limit: Some(limit),
    };
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
    })) {
        eprintln!("Nana 文件浏览任务提交失败：{error}");
        app.shell.reduce(ShellMessage::FileBrowserLoaded(Err(format!("文件浏览任务提交失败：{error}"))));
    }
}

fn dispatch_create(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    parent: Option<String>,
    name: String,
    directory: bool,
) {
    let created_name = if directory { None } else { Some(name.clone()) };
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 新建条目需要领域服务，当前服务未启动");
        fail_snapshot(app, created_name, "领域服务未启动".into());
        return;
    };
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let request = FileCreateRequest { repo_id, parent_path: parent, name };
    let task_name = created_name.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = if directory {
            executor.block_on(browser.create_directory(request))
        } else {
            executor.block_on(browser.create_file(request))
        };
        snapshot_message(result, task_name)
    })) {
        eprintln!("Nana 新建条目任务提交失败：{error}");
        fail_snapshot(app, created_name, format!("新建条目任务提交失败：{error}"));
    }
}

fn dispatch_rename(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    new_name: String,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 重命名需要领域服务，当前服务未启动");
        fail_snapshot(app, None, "领域服务未启动".into());
        return;
    };
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    let request = FileRenameRequest { repo_id, path, new_name };
    if let Err(error) = context.run_task(Task::new(async move {
        snapshot_message(executor.block_on(browser.rename_entry(request)), None)
    })) {
        eprintln!("Nana 重命名任务提交失败：{error}");
        fail_snapshot(app, None, format!("重命名任务提交失败：{error}"));
    }
}

fn dispatch_copy(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    sources: Vec<String>,
    parent: Option<String>,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 复制需要领域服务，当前服务未启动");
        fail_protocol(app, "领域服务未启动".into());
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    let request = FileCopyRequest { repo_id, source_paths: sources, parent_path: parent, mode: Some("hardlinkPreferred".into()) };
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(tasks.execute(PROTOCOL_ENTRY_COPY, request)).map(|_| ());
        protocol_message(result, true)
    })) {
        eprintln!("Nana 复制任务提交失败：{error}");
        fail_protocol(app, format!("复制任务提交失败：{error}"));
    }
}

fn dispatch_move(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    sources: Vec<String>,
    parent: String,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 移动需要领域服务，当前服务未启动");
        fail_protocol(app, "领域服务未启动".into());
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    let request = FileMoveRequest { repo_id, source_paths: sources, parent_path: parent };
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(tasks.execute(PROTOCOL_ENTRY_MOVE, request)).map(|_| ());
        protocol_message(result, false)
    })) {
        eprintln!("Nana 移动任务提交失败：{error}");
        fail_protocol(app, format!("移动任务提交失败：{error}"));
    }
}

fn dispatch_import(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    parent: Option<String>,
    sources: Vec<String>,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 导入需要领域服务，当前服务未启动");
        fail_protocol(app, "领域服务未启动".into());
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    let request = FileImportRequest { repo_id, parent_path: parent, source_paths: sources };
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(tasks.execute(PROTOCOL_ENTRY_IMPORT, request)).map(|_| ());
        protocol_message(result, false)
    })) {
        eprintln!("Nana 导入任务提交失败：{error}");
        fail_protocol(app, format!("导入任务提交失败：{error}"));
    }
}

fn dispatch_archive(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    parent: Option<String>,
    archive_path: String,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 压缩包导入需要领域服务，当前服务未启动");
        fail_protocol(app, "领域服务未启动".into());
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    let request = FileArchiveImportRequest { repo_id, parent_path: parent, archive_path };
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(tasks.execute(PROTOCOL_ARCHIVE_IMPORT, request)).map(|_| ());
        protocol_message(result, false)
    })) {
        eprintln!("Nana 压缩包导入任务提交失败：{error}");
        fail_protocol(app, format!("压缩包导入任务提交失败：{error}"));
    }
}

fn dispatch_eagle(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    parent: Option<String>,
    library_path: String,
    mode: String,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana Eagle 导入需要领域服务，当前服务未启动");
        fail_protocol(app, "领域服务未启动".into());
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    let request = EagleLibraryImportRequest { repo_id, parent_path: parent, library_path, mode };
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(tasks.execute(PROTOCOL_EAGLE_IMPORT, request)).map(|_| ());
        protocol_message(result, false)
    })) {
        eprintln!("Nana Eagle 导入任务提交失败：{error}");
        fail_protocol(app, format!("Eagle 导入任务提交失败：{error}"));
    }
}

fn dispatch_delete(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    paths: Vec<String>,
    mode: Option<String>,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 删除需要领域服务，当前服务未启动");
        fail_protocol(app, "领域服务未启动".into());
        return;
    };
    let tasks = services.tasks.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let mut failed = None;
        for path in paths {
            let request = FileDeleteRequest { repo_id: repo_id.clone(), path: path.clone(), mode: mode.clone() };
            if let Err(error) = executor.block_on(tasks.execute(PROTOCOL_ENTRY_DELETE, request)) {
                eprintln!("Nana 删除条目失败 {path}：{error}");
                failed = Some(error);
                break;
            }
        }
        let result = match failed {
            Some(error) => Err(error),
            None => Ok(()),
        };
        protocol_message(result, false)
    })) {
        eprintln!("Nana 删除任务提交失败：{error}");
        fail_protocol(app, format!("删除任务提交失败：{error}"));
    }
}

fn dispatch_trash(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    action: String,
    paths: Vec<String>,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 回收站变更需要领域服务，当前服务未启动");
        fail_snapshot(app, None, "领域服务未启动".into());
        return;
    };
    let browser = services.file_browser.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let calls: Vec<Option<String>> = if paths.is_empty() { vec![None] } else { paths.into_iter().map(Some).collect() };
        let mut last = None;
        for path in calls {
            let request = TrashMutationRequest { repo_id: repo_id.clone(), action: action.clone(), path };
            match executor.block_on(browser.mutate_trash(request)) {
                Ok(snapshot) => last = Some(snapshot),
                Err(error) => {
                    eprintln!("Nana 回收站变更失败：{error}");
                    return snapshot_message(Err(error), None);
                }
            }
        }
        match last {
            Some(snapshot) => snapshot_message(Ok(snapshot), None),
            None => snapshot_message(Err("回收站变更没有返回快照".into()), None),
        }
    })) {
        eprintln!("Nana 回收站变更任务提交失败：{error}");
        fail_snapshot(app, None, format!("回收站变更任务提交失败：{error}"));
    }
}

/// 拉取硬链接候选。`silent` 为真时结果是 `HardlinksRefreshed`，失败不经 `note_hardlinks` 写入页面错误。
fn dispatch_hardlinks(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    silent: bool,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 硬链接候选需要领域服务，当前服务未启动");
        app.shell.reduce(hardlinks_message(silent, Err("领域服务未启动".into())));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor.block_on(interaction.list_hardlink_candidates(repo_id)).map(|response| {
            response.candidates.into_iter().map(|candidate| HardlinkPrompt {
                id: candidate.candidate_id,
                new_path: candidate.new_path,
                existing_path: candidate.existing_path,
                size_label: candidate.size_label,
            }).collect()
        });
        hardlinks_message(silent, result)
    })) {
        eprintln!("Nana 硬链接候选任务提交失败：{error}");
        app.shell.reduce(hardlinks_message(silent, Err(format!("硬链接候选任务提交失败：{error}"))));
    }
}

fn hardlinks_message(silent: bool, result: Result<Vec<HardlinkPrompt>, String>) -> ShellMessage {
    files_message(if silent {
        FilesMessage::HardlinksRefreshed(result)
    } else {
        FilesMessage::HardlinksLoaded(result)
    })
}

fn dispatch_confirm_hardlink(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    candidate_id: String,
) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 确认硬链接需要领域服务，当前服务未启动");
        app.shell.reduce(files_message(FilesMessage::HardlinkConfirmed(Err("领域服务未启动".into()))));
        return;
    };
    let interaction = services.repository_interaction.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        let result = executor
            .block_on(interaction.confirm_hardlink_candidate(HardlinkConfirmRequest { repo_id, candidate_id: candidate_id.clone() }))
            .map(|_| candidate_id);
        files_message(FilesMessage::HardlinkConfirmed(result))
    })) {
        eprintln!("Nana 确认硬链接任务提交失败：{error}");
        app.shell.reduce(files_message(FilesMessage::HardlinkConfirmed(Err(format!("确认硬链接任务提交失败：{error}")))));
    }
}

fn dispatch_asset(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, asset_id: String) {
    let Some(services) = app.services.as_ref() else {
        eprintln!("Nana 文件元数据需要领域服务，当前服务未启动");
        app.shell.reduce(files_message(FilesMessage::NoteError("领域服务未启动".into())));
        return;
    };
    let query = services.repository_query.clone();
    let executor = services.executor.clone();
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::AssetDetailLoaded(executor.block_on(query.get_asset_detail(repo_id, asset_id)))
    })) {
        eprintln!("Nana 文件元数据任务提交失败：{error}");
        app.shell.reduce(files_message(FilesMessage::NoteError(format!("文件元数据任务提交失败：{error}"))));
    }
}

fn snapshot_message(
    result: Result<crate::backend::services::repository::FileBrowserSnapshot, String>,
    created_name: Option<String>,
) -> ShellMessage {
    files_message(FilesMessage::MutationSnapshot { result, created_name })
}

fn protocol_message(result: Result<(), String>, hardlinks: bool) -> ShellMessage {
    let reload = result.is_ok();
    files_message(FilesMessage::ProtocolFinished { result, reload, hardlinks: hardlinks && reload })
}

fn fail_snapshot(app: &mut MomoBakoApplication, created_name: Option<String>, error: String) {
    app.shell.reduce(snapshot_message(Err(error), created_name));
}

fn fail_protocol(app: &mut MomoBakoApplication, error: String) {
    app.shell.reduce(protocol_message(Err(error), false));
}

fn files_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}
