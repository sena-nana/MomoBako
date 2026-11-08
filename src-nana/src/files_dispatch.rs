//! 把文件浏览副作用交给已有的文件浏览 ViewModel 和 Mutsuki 协议。
//!
//! 新建、重命名和回收站变更返回目录快照。复制、移动、导入和删除走协议，成功后再重新读取目录，
//! 不用协议 JSON 改写本地行。复制、移动、删除的协议 JSON 若带文件夹树，只把树交给侧栏。
//! 任务都经 `task_guard` 派发：服务没起来、任务排不进去或任务 panic 时把错误写回状态，按钮不会停在进行中。

use nana_ui::RuntimeProgramContext;

#[path = "files_effect_dispatch.rs"]
mod files_effect_dispatch;
#[path = "files_protocol_dispatch.rs"]
mod files_protocol_dispatch;

pub(crate) use files_protocol_dispatch::protocol_folder_tree;

use crate::backend::services::mutsuki_runner::{
    PROTOCOL_ARCHIVE_IMPORT, PROTOCOL_EAGLE_IMPORT, PROTOCOL_ENTRY_COPY, PROTOCOL_ENTRY_DELETE,
    PROTOCOL_ENTRY_IMPORT, PROTOCOL_ENTRY_MOVE,
};
use crate::backend::services::repository::{
    EagleLibraryImportRequest, FileArchiveImportRequest, FileCopyRequest, FileDeleteRequest,
    FileImportRequest, FileMoveRequest,
};
use crate::shell::{display_mode_path, FilesEffect, FilesMessage, ShellMessage};
use crate::task_guard::spawn_service;
use crate::MomoBakoApplication;

/// 执行文件归约留下的请求。
pub fn dispatch_files_effects(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    for effect in app.shell.files.take_effects() {
        match effect {
            FilesEffect::Browse {
                repo_id,
                path,
                trash,
                offset,
                limit,
                ..
            } => {
                files_effect_dispatch::dispatch_browse(
                    app, context, repo_id, path, trash, offset, limit,
                );
            }
            FilesEffect::PrefetchVirtualPage {
                repo_id,
                path,
                trash,
                offset,
                limit,
            } => {
                files_effect_dispatch::dispatch_virtual_page(
                    app, context, repo_id, path, trash, offset, limit,
                );
            }
            FilesEffect::CreateDirectory {
                repo_id,
                parent,
                name,
            } => {
                files_effect_dispatch::dispatch_create(app, context, repo_id, parent, name, true);
            }
            FilesEffect::CreateFile {
                repo_id,
                parent,
                name,
            } => {
                files_effect_dispatch::dispatch_create(app, context, repo_id, parent, name, false);
            }
            FilesEffect::Rename {
                repo_id,
                path,
                new_name,
            } => files_effect_dispatch::dispatch_rename(app, context, repo_id, path, new_name),
            FilesEffect::Copy {
                repo_id,
                sources,
                parent,
            } => {
                let request = FileCopyRequest {
                    repo_id,
                    source_paths: sources,
                    parent_path: parent,
                    mode: Some("hardlinkPreferred".into()),
                };
                files_protocol_dispatch::dispatch_protocol(
                    app,
                    context,
                    "复制",
                    true,
                    true,
                    move |tasks, executor| {
                        executor
                            .block_on(tasks.execute(PROTOCOL_ENTRY_COPY, request))
                            .map(|(value, _)| value)
                    },
                );
            }
            FilesEffect::Move {
                repo_id,
                sources,
                parent,
            } => {
                let request = FileMoveRequest {
                    repo_id,
                    source_paths: sources,
                    parent_path: parent,
                };
                files_protocol_dispatch::dispatch_protocol(
                    app,
                    context,
                    "移动",
                    false,
                    true,
                    move |tasks, executor| {
                        executor
                            .block_on(tasks.execute(PROTOCOL_ENTRY_MOVE, request))
                            .map(|(value, _)| value)
                    },
                );
            }
            FilesEffect::Import {
                repo_id,
                parent,
                sources,
            } => {
                let request = FileImportRequest {
                    repo_id,
                    parent_path: parent,
                    source_paths: sources,
                    skip_existing: false,
                };
                files_protocol_dispatch::dispatch_protocol(
                    app,
                    context,
                    "导入",
                    false,
                    false,
                    move |tasks, executor| {
                        executor
                            .block_on(tasks.execute(PROTOCOL_ENTRY_IMPORT, request))
                            .map(|(value, _)| value)
                    },
                );
            }
            FilesEffect::ImportArchive {
                repo_id,
                parent,
                archive_path,
            } => {
                let request = FileArchiveImportRequest {
                    repo_id,
                    parent_path: parent,
                    archive_path,
                };
                files_protocol_dispatch::dispatch_protocol(
                    app,
                    context,
                    "压缩包导入",
                    false,
                    false,
                    move |tasks, executor| {
                        executor
                            .block_on(tasks.execute(PROTOCOL_ARCHIVE_IMPORT, request))
                            .map(|(value, _)| value)
                    },
                );
            }
            FilesEffect::ImportEagle {
                repo_id,
                parent,
                library_path,
                mode,
            } => {
                let request = EagleLibraryImportRequest {
                    repo_id,
                    parent_path: parent,
                    library_path,
                    mode,
                };
                let failed =
                    |error| files_message(FilesMessage::EagleImportSettled { result: Err(error) });
                spawn_service(
                    app.services.as_ref(),
                    context,
                    "Eagle 导入",
                    failed,
                    |services| {
                        let tasks = services.tasks.clone();
                        let executor = services.executor.clone();
                        move || match executor
                            .block_on(tasks.execute(PROTOCOL_EAGLE_IMPORT, request))
                        {
                            Ok((value, _)) => files_message(FilesMessage::EagleImportSettled {
                                result: Ok(crate::shell::admin::eagle_import_report(&value)),
                            }),
                            Err(error) => files_message(FilesMessage::EagleImportSettled {
                                result: Err(error),
                            }),
                        }
                    },
                );
            }
            FilesEffect::Delete {
                repo_id,
                paths,
                mode,
            } => {
                files_protocol_dispatch::dispatch_protocol(
                    app,
                    context,
                    "删除",
                    false,
                    true,
                    move |tasks, executor| {
                        if paths.is_empty() {
                            return Ok(serde_json::Value::Null);
                        }
                        let mut last = None;
                        for path in paths {
                            let request = FileDeleteRequest {
                                repo_id: repo_id.clone(),
                                path: path.clone(),
                                mode: mode.clone(),
                            };
                            match executor.block_on(tasks.execute(PROTOCOL_ENTRY_DELETE, request)) {
                                Ok((value, _)) => last = Some(value),
                                Err(error) => {
                                    eprintln!("Nana 删除条目失败 {path}：{error}");
                                    return Err(error);
                                }
                            }
                        }
                        match last {
                            Some(value) => Ok(value),
                            None => {
                                eprintln!("Nana 删除没有返回结果");
                                Err("删除没有返回结果".into())
                            }
                        }
                    },
                );
            }
            FilesEffect::MutateTrash {
                repo_id,
                action,
                paths,
            } => files_protocol_dispatch::dispatch_trash(app, context, repo_id, action, paths),
            FilesEffect::LoadHardlinks { repo_id, epoch } => {
                files_protocol_dispatch::dispatch_hardlinks(
                    app,
                    context,
                    repo_id,
                    files_protocol_dispatch::HardlinkRead::Load,
                    epoch,
                )
            }
            FilesEffect::RefreshHardlinks { repo_id, epoch } => {
                files_protocol_dispatch::dispatch_hardlinks(
                    app,
                    context,
                    repo_id,
                    files_protocol_dispatch::HardlinkRead::Refresh,
                    epoch,
                )
            }
            FilesEffect::CheckHardlinks { repo_id, epoch } => {
                files_protocol_dispatch::dispatch_hardlinks(
                    app,
                    context,
                    repo_id,
                    files_protocol_dispatch::HardlinkRead::Check,
                    epoch,
                )
            }
            FilesEffect::ConfirmHardlink {
                repo_id,
                candidate_id,
            } => {
                files_protocol_dispatch::dispatch_confirm_hardlink(
                    app,
                    context,
                    repo_id,
                    candidate_id,
                );
            }
            FilesEffect::LoadAsset { repo_id, asset_id } => {
                files_protocol_dispatch::dispatch_asset(app, context, repo_id, asset_id)
            }
            FilesEffect::PersistDisplayMode => {
                app.shell.files.save_display_mode_file(&display_mode_path())
            }
            FilesEffect::DecodeThumbnails { paths } => {
                files_effect_dispatch::dispatch_thumbnails(context, paths)
            }
            FilesEffect::EnsureThumbnail {
                repo_id,
                path,
                source_url,
            } => {
                files_effect_dispatch::dispatch_ensure(app, context, repo_id, path, source_url);
            }
            FilesEffect::SaveCover {
                repo_id,
                path,
                source_url,
            } => {
                files_effect_dispatch::dispatch_save_cover(app, context, repo_id, path, source_url);
            }
            FilesEffect::MutateThumbnail {
                repo_id,
                path,
                kind,
                action,
                source_path,
                image_bytes,
            } => {
                files_effect_dispatch::dispatch_thumbnail(
                    app,
                    context,
                    repo_id,
                    path,
                    kind,
                    action,
                    source_path,
                    image_bytes,
                );
            }
            FilesEffect::ExportArchive {
                repo_id,
                format,
                output_path,
                compression,
                encrypt,
                password,
            } => {
                files_protocol_dispatch::dispatch_export(
                    app,
                    context,
                    files_protocol_dispatch::archive_export(
                        repo_id,
                        format,
                        output_path,
                        compression,
                        encrypt,
                        password,
                    ),
                );
            }
            FilesEffect::ExportGit {
                repo_id,
                remote,
                branch,
                message,
            } => {
                files_protocol_dispatch::dispatch_export(
                    app,
                    context,
                    files_protocol_dispatch::git_export(repo_id, remote, branch, message),
                );
            }
            FilesEffect::CopyText(text) => {
                if !crate::host_bridge::copy_text(&text) {
                    eprintln!("Nana 复制到系统剪贴板失败");
                    app.shell
                        .files
                        .note_error("复制失败：系统剪贴板写入失败".into());
                }
            }
        }
    }
}

fn files_message(message: FilesMessage) -> ShellMessage {
    ShellMessage::Files(message)
}

fn snapshot_message(
    result: Result<crate::backend::services::repository::FileBrowserSnapshot, String>,
    created_name: Option<String>,
) -> ShellMessage {
    files_message(FilesMessage::MutationSnapshot {
        result,
        created_name,
    })
}
