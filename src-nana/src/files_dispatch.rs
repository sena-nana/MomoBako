//! 把文件浏览副作用交给已有的文件浏览 ViewModel 和 Mutsuki 协议。
//!
//! 新建、重命名和回收站变更返回目录快照。复制、移动、导入和删除只返回协议结果，
//! 成功后再重新读取目录，不用协议 JSON 改写本地行。任务都经 `task_guard` 派发：服务没起来、任务排不进去或任务
//! panic 时把错误写回状态，按钮不会停在进行中。

use nana_ui::RuntimeProgramContext;

use crate::backend::services::mutsuki_runner::{
    PROTOCOL_ARCHIVE_IMPORT, PROTOCOL_EAGLE_IMPORT, PROTOCOL_ENTRY_COPY, PROTOCOL_ENTRY_DELETE,
    PROTOCOL_ENTRY_IMPORT, PROTOCOL_ENTRY_MOVE, PROTOCOL_REPOSITORY_EXPORT,
};
use crate::backend::services::repository::{
    EagleLibraryImportRequest, FileArchiveImportRequest, FileBrowserRequest, FileCopyRequest, FileCreateRequest,
    FileDeleteRequest, FileImportRequest, FileMoveRequest, FileRenameRequest, HardlinkConfirmRequest,
    RepositoryArchiveExportOptions, RepositoryExportRequest, RepositoryGitExportOptions, ThumbnailRequest,
    TrashMutationRequest,
};
use crate::shell::{
    display_mode_path, FilesEffect, FilesMessage, HardlinkPrompt, ShellMessage,
};
use crate::task_guard::{settle, spawn, spawn_service};
use crate::MomoBakoApplication;

/// 执行文件归约留下的请求。
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
            FilesEffect::Copy { repo_id, sources, parent } => {
                let request = FileCopyRequest { repo_id, source_paths: sources, parent_path: parent, mode: Some("hardlinkPreferred".into()) };
                dispatch_protocol(app, context, "复制", true, move |tasks, executor| {
                    executor.block_on(tasks.execute(PROTOCOL_ENTRY_COPY, request)).map(|_| ())
                });
            }
            FilesEffect::Move { repo_id, sources, parent } => {
                let request = FileMoveRequest { repo_id, source_paths: sources, parent_path: parent };
                dispatch_protocol(app, context, "移动", false, move |tasks, executor| {
                    executor.block_on(tasks.execute(PROTOCOL_ENTRY_MOVE, request)).map(|_| ())
                });
            }
            FilesEffect::Import { repo_id, parent, sources } => {
                let request = FileImportRequest { repo_id, parent_path: parent, source_paths: sources, skip_existing: false };
                dispatch_protocol(app, context, "导入", false, move |tasks, executor| {
                    executor.block_on(tasks.execute(PROTOCOL_ENTRY_IMPORT, request)).map(|_| ())
                });
            }
            FilesEffect::ImportArchive { repo_id, parent, archive_path } => {
                let request = FileArchiveImportRequest { repo_id, parent_path: parent, archive_path };
                dispatch_protocol(app, context, "压缩包导入", false, move |tasks, executor| {
                    executor.block_on(tasks.execute(PROTOCOL_ARCHIVE_IMPORT, request)).map(|_| ())
                });
            }
            FilesEffect::ImportEagle { repo_id, parent, library_path, mode } => {
                let request = EagleLibraryImportRequest { repo_id, parent_path: parent, library_path, mode };
                dispatch_protocol(app, context, "Eagle 导入", false, move |tasks, executor| {
                    executor.block_on(tasks.execute(PROTOCOL_EAGLE_IMPORT, request)).map(|_| ())
                });
            }
            FilesEffect::Delete { repo_id, paths, mode } => {
                dispatch_protocol(app, context, "删除", false, move |tasks, executor| {
                    for path in paths {
                        let request = FileDeleteRequest { repo_id: repo_id.clone(), path: path.clone(), mode: mode.clone() };
                        if let Err(error) = executor.block_on(tasks.execute(PROTOCOL_ENTRY_DELETE, request)) {
                            eprintln!("Nana 删除条目失败 {path}：{error}");
                            return Err(error);
                        }
                    }
                    Ok(())
                });
            }
            FilesEffect::MutateTrash { repo_id, action, paths } => dispatch_trash(app, context, repo_id, action, paths),
            FilesEffect::LoadHardlinks { repo_id, epoch } => dispatch_hardlinks(app, context, repo_id, HardlinkRead::Load, epoch),
            FilesEffect::RefreshHardlinks { repo_id, epoch } => dispatch_hardlinks(app, context, repo_id, HardlinkRead::Refresh, epoch),
            FilesEffect::CheckHardlinks { repo_id, epoch } => dispatch_hardlinks(app, context, repo_id, HardlinkRead::Check, epoch),
            FilesEffect::ConfirmHardlink { repo_id, candidate_id } => {
                dispatch_confirm_hardlink(app, context, repo_id, candidate_id);
            }
            FilesEffect::LoadAsset { repo_id, asset_id } => dispatch_asset(app, context, repo_id, asset_id),
            FilesEffect::PersistDisplayMode => app.shell.files.save_display_mode_file(&display_mode_path()),
            FilesEffect::DecodeThumbnails { paths } => dispatch_thumbnails(context, paths),
            FilesEffect::MutateThumbnail { repo_id, path, kind, action, source_path, image_bytes } => {
                dispatch_thumbnail(app, context, repo_id, path, kind, action, source_path, image_bytes);
            }
            FilesEffect::ExportArchive { repo_id, format, output_path, compression, encrypt, password } => {
                dispatch_export(app, context, archive_export(repo_id, format, output_path, compression, encrypt, password));
            }
            FilesEffect::ExportGit { repo_id, remote, branch, message } => {
                dispatch_export(app, context, git_export(repo_id, remote, branch, message));
            }
            FilesEffect::CopyText(text) => {
                if !crate::host_bridge::copy_text(&text) {
                    eprintln!("Nana 复制到系统剪贴板失败");
                    app.shell.files.note_error("复制失败：系统剪贴板写入失败".into());
                }
            }
        }
    }
}

/// 写入、清除自定义缩略图，或者请后端按源文件刷新缩略图。清除后文件没有路径时再刷新，恢复默认图。
#[allow(clippy::too_many_arguments)]
fn dispatch_thumbnail(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    kind: String,
    action: String,
    source_path: Option<String>,
    image_bytes: Option<Vec<u8>>,
) {
    let what = match action.as_str() {
        "refresh" => "刷新缩略图",
        "clear" => "取消自定义缩略图",
        _ => "自定义缩略图",
    };
    let failed = |error| files_message(FilesMessage::NoteError(error));
    spawn_service(app.services.as_ref(), context, what, failed, |services| {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || {
            let request = thumbnail_request(&repo_id, &path, &action, source_path.clone(), image_bytes.clone());
            let saved = match executor.block_on(browser.ensure_thumbnail(request)) {
                Ok(response) => response,
                Err(error) => {
                    eprintln!("Nana {what}失败：{path}：{error}");
                    return files_message(FilesMessage::NoteError(error));
                }
            };
            let saved = if action == "clear" && saved.thumbnail_path.is_none() && kind == "file" {
                let refresh = thumbnail_request(&repo_id, &path, "refresh", None, None);
                match executor.block_on(browser.ensure_thumbnail(refresh)) {
                    Ok(response) => response,
                    Err(error) => {
                        eprintln!("Nana 恢复默认缩略图失败：{error}");
                        return files_message(FilesMessage::NoteError(error));
                    }
                }
            } else {
                saved
            };
            files_message(FilesMessage::ThumbnailSaved {
                path,
                thumbnail_path: saved.thumbnail_path,
                custom: saved.thumbnail_custom,
            })
        }
    });
}

fn thumbnail_request(repo_id: &str, path: &str, action: &str, source_path: Option<String>, image_bytes: Option<Vec<u8>>) -> ThumbnailRequest {
    ThumbnailRequest {
        repo_id: repo_id.to_string(),
        path: path.to_string(),
        action: Some(action.to_string()),
        source_path,
        source_url: None,
        image_bytes,
        media_type: None,
    }
}

/// 解码缩略图。每个文件单独接住 panic（图片解码器出错只丢这一张），解不出的记日志；整个任务排不进去时交回空的一批，
/// 这些行留着类型图标。
fn dispatch_thumbnails(context: &RuntimeProgramContext<ShellMessage>, paths: Vec<String>) {
    spawn(context, "缩略图解码", |_| ShellMessage::ThumbnailPixels(Vec::new()), move || {
        let mut frames = Vec::new();
        for path in paths {
            match settle("缩略图解码", || crate::shell::decode_thumbnail_file(&path), Err) {
                Ok(frame) => frames.push(frame),
                Err(error) => eprintln!("Nana 缩略图解码失败：{path}：{error}"),
            }
        }
        ShellMessage::ThumbnailPixels(frames)
    });
}

fn dispatch_browse(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    trash: bool,
    offset: usize,
    limit: usize,
) {
    let request = FileBrowserRequest {
        repo_id,
        directory_path: if trash || path.is_empty() { None } else { Some(path) },
        include_tree: Some(false),
        special_location: if trash { Some("trash".into()) } else { None },
        offset: Some(offset),
        limit: Some(limit),
    };
    let failed = |error| ShellMessage::FileBrowserLoaded(Err(error));
    spawn_service(app.services.as_ref(), context, "文件浏览", failed, |services| {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || ShellMessage::FileBrowserLoaded(executor.block_on(browser.get_file_browser(request)))
    });
}

fn dispatch_create(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    parent: Option<String>,
    name: String,
    directory: bool,
) {
    let created_name = if directory { None } else { Some(name.clone()) };
    let request = FileCreateRequest { repo_id, parent_path: parent, name };
    let failed_name = created_name.clone();
    let failed = move |error| snapshot_message(Err(error), failed_name.clone());
    spawn_service(app.services.as_ref(), context, "新建条目", failed, |services| {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || {
            let result = if directory {
                executor.block_on(browser.create_directory(request))
            } else {
                executor.block_on(browser.create_file(request))
            };
            snapshot_message(result, created_name)
        }
    });
}

fn dispatch_rename(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    path: String,
    new_name: String,
) {
    let request = FileRenameRequest { repo_id, path, new_name };
    let failed = |error| snapshot_message(Err(error), None);
    spawn_service(app.services.as_ref(), context, "重命名", failed, |services| {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || snapshot_message(executor.block_on(browser.rename_entry(request)), None)
    });
}

/// 复制、移动、导入、压缩包和 Eagle 导入、删除：在任务里跑 Mutsuki 协议，结果回 `ProtocolFinished`（`hardlinks` 为真时
/// 成功后再读硬链接候选）。
fn dispatch_protocol(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    what: &str,
    hardlinks: bool,
    run: impl FnOnce(crate::backend::viewmodels::MutsukiTaskViewModel, std::sync::Arc<tokio::runtime::Runtime>) -> Result<(), String> + Send + 'static,
) {
    let failed = |error| protocol_message(Err(error), false);
    spawn_service(app.services.as_ref(), context, what, failed, |services| {
        let tasks = services.tasks.clone();
        let executor = services.executor.clone();
        move || protocol_message(run(tasks, executor), hardlinks)
    });
}

fn dispatch_trash(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    action: String,
    paths: Vec<String>,
) {
    let failed = |error| snapshot_message(Err(error), None);
    spawn_service(app.services.as_ref(), context, "回收站变更", failed, |services| {
        let browser = services.file_browser.clone();
        let executor = services.executor.clone();
        move || {
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
        }
    });
}

/// 硬链接候选是为什么读的，结果按它交回不同的消息。
#[derive(Clone, Copy)]
enum HardlinkRead {
    /// 复制以后：弹出确认，失败写进文件列表。
    Load,
    /// 结构更新的静默重读：只换列表。
    Refresh,
    /// 启动和换仓库后的后台读取：没有别的对话框时弹出确认，失败只记日志。
    Check,
}

/// 拉取硬链接候选，结果按 `read` 交回，带着请求时候选列表的版本 `epoch`。
fn dispatch_hardlinks(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    read: HardlinkRead,
    epoch: u64,
) {
    let failed = move |error| hardlinks_message(read, epoch, Err(error));
    spawn_service(app.services.as_ref(), context, "硬链接候选", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor.block_on(interaction.list_hardlink_candidates(repo_id)).map(|response| {
                response.candidates.into_iter().map(|candidate| HardlinkPrompt {
                    id: candidate.candidate_id,
                    new_path: candidate.new_path,
                    existing_path: candidate.existing_path,
                    size_label: candidate.size_label,
                }).collect()
            });
            hardlinks_message(read, epoch, result)
        }
    });
}

fn hardlinks_message(read: HardlinkRead, epoch: u64, result: Result<Vec<HardlinkPrompt>, String>) -> ShellMessage {
    files_message(match read {
        HardlinkRead::Load => FilesMessage::HardlinksLoaded { epoch, result },
        HardlinkRead::Refresh => FilesMessage::HardlinksRefreshed { epoch, result },
        HardlinkRead::Check => FilesMessage::HardlinksChecked { epoch, result },
    })
}

fn dispatch_confirm_hardlink(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
    candidate_id: String,
) {
    let failed_repo = repo_id.clone();
    let failed = move |error| files_message(FilesMessage::HardlinkConfirmed { repo_id: failed_repo.clone(), result: Err(error) });
    spawn_service(app.services.as_ref(), context, "确认硬链接", failed, |services| {
        let interaction = services.repository_interaction.clone();
        let executor = services.executor.clone();
        move || {
            let request = HardlinkConfirmRequest { repo_id: repo_id.clone(), candidate_id: candidate_id.clone() };
            let result = executor.block_on(interaction.confirm_hardlink_candidate(request)).map(|_| candidate_id);
            files_message(FilesMessage::HardlinkConfirmed { repo_id, result })
        }
    });
}

/// 选中文件后读素材详情。派发层自己的失败（服务没起来、任务排不进去、panic）写进文件列，不像服务交回的失败那样
/// 把整页换成错误页。
fn dispatch_asset(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, repo_id: String, asset_id: String) {
    let failed = |error| files_message(FilesMessage::NoteError(error));
    spawn_service(app.services.as_ref(), context, "文件元数据", failed, |services| {
        let query = services.repository_query.clone();
        let executor = services.executor.clone();
        move || ShellMessage::AssetDetailLoaded(executor.block_on(query.get_asset_detail(repo_id, asset_id)))
    });
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

fn files_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

/// 用对话框里的格式、路径和压缩选项组压缩包导出请求。
fn archive_export(
    repo_id: String,
    format: String,
    output_path: String,
    compression: String,
    encrypt: bool,
    password: String,
) -> RepositoryExportRequest {
    let password = password.trim().to_string();
    RepositoryExportRequest {
        repo_id,
        target: "archive".into(),
        archive: Some(RepositoryArchiveExportOptions {
            format,
            output_path,
            compression,
            encrypt,
            password: (!password.is_empty()).then_some(password),
        }),
        git: None,
    }
}

/// 用对话框里的远端、分支和提交说明组 Git 导出请求。空白字段省略。
fn git_export(repo_id: String, remote: String, branch: String, message: String) -> RepositoryExportRequest {
    RepositoryExportRequest {
        repo_id,
        target: "git".into(),
        archive: None,
        git: Some(RepositoryGitExportOptions {
            remote: filled(remote),
            branch: filled(branch),
            message: filled(message),
        }),
    }
}

fn filled(value: String) -> Option<String> {
    let value = value.trim().to_string();
    (!value.is_empty()).then_some(value)
}

/// 调用 `momobako.repository.export`。成功把宿主 message 写回对话框，失败写真实错误。
fn dispatch_export(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>, request: RepositoryExportRequest) {
    let failed = |error| files_message(FilesMessage::ExportFinished(Err(error)));
    spawn_service(app.services.as_ref(), context, "资源库导出", failed, |services| {
        let tasks = services.tasks.clone();
        let executor = services.executor.clone();
        move || {
            let result = executor.block_on(tasks.execute(PROTOCOL_REPOSITORY_EXPORT, request)).map(|(output, _)| export_notice(&output));
            files_message(FilesMessage::ExportFinished(result))
        }
    });
}

/// 只采用宿主返回的 message。没有说明时记日志，不编造成功。
fn export_notice(value: &serde_json::Value) -> String {
    value
        .get("message")
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            eprintln!("Nana 导出返回没有说明：{value}");
            "导出已返回，但没有结果说明。".into()
        })
}
