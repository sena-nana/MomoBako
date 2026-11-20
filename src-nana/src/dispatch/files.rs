//! 把文件浏览副作用交给已有的文件浏览 ViewModel 和 Mutsuki 协议。
//!
//! 新建、重命名和回收站变更返回目录快照。复制、移动、导入和删除走协议，成功后再重新读取目录，
//! 不用协议 JSON 改写本地行。复制、移动、删除的协议 JSON 若带文件夹树，只把树交给侧栏。
//! 任务都经 `task_guard` 派发：服务没起来、任务排不进去或任务 panic 时把错误写回状态，按钮不会停在进行中。

use nana_ui::RuntimeProgramContext;

mod effects;
mod protocol;

#[cfg(test)]
pub(crate) use protocol::protocol_folder_tree;

use crate::MomoBakoApplication;
use crate::backend::services::mutsuki_runner::{
    ArchiveImport, EagleImport, EntryCopy, EntryDelete, EntryImport, EntryMove,
};
use crate::backend::services::repository::{
    EagleLibraryImportRequest, EntryDeleteRequest, FileArchiveImportRequest, FileCopyRequest,
    FileImportRequest, FileMoveRequest,
};
use crate::dispatch::task_guard::spawn_service;
use crate::shell::{FilesEffect, FilesMessage, ShellMessage, display_mode_path};

/// 执行文件归约留下的请求。
pub(crate) fn effects(
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
                effects::dispatch_browse(app, context, repo_id, path, trash, offset, limit);
            }
            FilesEffect::PrefetchVirtualPage {
                repo_id,
                path,
                trash,
                offset,
                limit,
            } => {
                effects::dispatch_virtual_page(app, context, repo_id, path, trash, offset, limit);
            }
            FilesEffect::CreateDirectory {
                repo_id,
                parent,
                name,
            } => {
                effects::dispatch_create(app, context, repo_id, parent, name, true);
            }
            FilesEffect::CreateFile {
                repo_id,
                parent,
                name,
            } => {
                effects::dispatch_create(app, context, repo_id, parent, name, false);
            }
            FilesEffect::Rename {
                repo_id,
                path,
                new_name,
            } => effects::dispatch_rename(app, context, repo_id, path, new_name),
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
                protocol::dispatch_protocol(app, context, "复制", true, true, move |tasks| {
                    tasks
                        .run::<EntryCopy>(request)
                        .map(|output| Some(output.result))
                });
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
                protocol::dispatch_protocol(app, context, "移动", false, true, move |tasks| {
                    tasks
                        .run::<EntryMove>(request)
                        .map(|output| Some(output.result))
                });
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
                protocol::dispatch_protocol(app, context, "导入", false, false, move |tasks| {
                    tasks
                        .run::<EntryImport>(request)
                        .map(|output| Some(output.result.snapshot))
                });
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
                protocol::dispatch_protocol(
                    app,
                    context,
                    "压缩包导入",
                    false,
                    false,
                    move |tasks| {
                        tasks
                            .run::<ArchiveImport>(request)
                            .map(|output| Some(output.result))
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
                        move || {
                            let result = tasks.run::<EagleImport>(request).map(|output| {
                                crate::shell::admin::eagle_import_report(&output.result)
                            });
                            files_message(FilesMessage::EagleImportSettled { result })
                        }
                    },
                );
            }
            FilesEffect::Delete {
                repo_id,
                paths,
                mode,
            } => {
                protocol::dispatch_protocol(app, context, "删除", false, true, move |tasks| {
                    if paths.is_empty() {
                        return Ok(None);
                    }
                    tasks
                        .run::<EntryDelete>(EntryDeleteRequest::Batch {
                            repo_id,
                            paths,
                            mode,
                        })
                        .map(|output| output.result.snapshot)
                });
            }
            FilesEffect::MutateTrash {
                repo_id,
                action,
                paths,
            } => protocol::dispatch_trash(app, context, repo_id, action, paths),
            FilesEffect::LoadHardlinks { repo_id, epoch } => protocol::dispatch_hardlinks(
                app,
                context,
                repo_id,
                protocol::HardlinkRead::Load,
                epoch,
            ),
            FilesEffect::RefreshHardlinks { repo_id, epoch } => protocol::dispatch_hardlinks(
                app,
                context,
                repo_id,
                protocol::HardlinkRead::Refresh,
                epoch,
            ),
            FilesEffect::CheckHardlinks { repo_id, epoch } => protocol::dispatch_hardlinks(
                app,
                context,
                repo_id,
                protocol::HardlinkRead::Check,
                epoch,
            ),
            FilesEffect::ConfirmHardlink {
                repo_id,
                candidate_id,
            } => {
                protocol::dispatch_confirm_hardlink(app, context, repo_id, candidate_id);
            }
            FilesEffect::LoadAsset { repo_id, asset_id } => {
                protocol::dispatch_asset(app, context, repo_id, asset_id)
            }
            FilesEffect::PersistDisplayMode => {
                app.shell.files.save_display_mode_file(&display_mode_path())
            }
            FilesEffect::DecodeThumbnails { paths } => effects::dispatch_thumbnails(context, paths),
            FilesEffect::EnsureThumbnail {
                repo_id,
                path,
                source_url,
            } => {
                effects::dispatch_ensure(app, context, repo_id, path, source_url);
            }
            FilesEffect::SaveCover {
                repo_id,
                path,
                source_url,
            } => {
                effects::dispatch_save_cover(app, context, repo_id, path, source_url);
            }
            FilesEffect::MutateThumbnail {
                repo_id,
                path,
                kind,
                action,
                source_path,
                image_bytes,
            } => {
                effects::dispatch_thumbnail(
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
                protocol::dispatch_export(
                    app,
                    context,
                    protocol::archive_export(
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
                protocol::dispatch_export(
                    app,
                    context,
                    protocol::git_export(repo_id, remote, branch, message),
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
