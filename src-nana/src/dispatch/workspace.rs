//! 工作台归约留下的副作用：资源库列表、同步、摘要、首屏目录、重定向和删除，以及后台静默重读。

use nana_ui::RuntimeProgramContext;

use super::task_guard::spawn_service;
use crate::MomoBakoApplication;
use crate::backend::services::mutsuki_runner::{RepositoryRelocate, RepositorySync};
use crate::backend::services::repository::{
    RepositoryDeleteMode, RepositoryDeleteRequest, RepositoryRelocateRequest, SyncRequest,
};
use crate::shell::player::PlayerMessage;
use crate::shell::workspace_refresh::SilentMessage;
use crate::shell::{
    DeleteMode, ShellMessage, SyncCounts, WorkspaceEffect, last_repository_path, sidebar_prefs_path,
};

/// 读资源库列表（首屏启动第 1 步和刷新）。服务没起来、任务排不进去或 panic 时按失败交回，启动停在第 1 步给「重试」。
pub(crate) fn dispatch_list_repositories(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    generation: u64,
) {
    let failed = move |error| ShellMessage::WorkspaceListLoaded {
        generation,
        result: Err(error),
    };
    spawn_service(
        app.services.as_ref(),
        context,
        "资源库列表",
        failed,
        |services| {
            let query = services.repository_query.clone();
            move || ShellMessage::WorkspaceListLoaded {
                generation,
                result: query.list_repositories(),
            }
        },
    );
}

/// 执行工作台归约留下的副作用。服务都经 `task_guard` 派发：服务没起来、任务排不进去或 panic 时把错误写回状态机，
/// 启动和换仓库的步骤不会停在进行中。
pub(crate) fn effects(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    for effect in app.shell.workspace.take_effects() {
        match effect {
            WorkspaceEffect::PersistSidebar => {
                app.shell.workspace.save_prefs_file(&sidebar_prefs_path());
            }
            WorkspaceEffect::PersistLastRepository => {
                app.shell
                    .workspace
                    .save_last_repository_file(&last_repository_path());
            }
            WorkspaceEffect::StopPlayback { previous_repo_id } => {
                eprintln!("Nana 仓库切换，停止播放会话：{previous_repo_id}");
                app.shell.reduce(ShellMessage::Player(PlayerMessage::Stop {
                    repo_id: Some(previous_repo_id),
                    clear_stored: true,
                }));
            }
            WorkspaceEffect::RefreshRepositories { generation } => {
                dispatch_list_repositories(app, context, generation)
            }
            WorkspaceEffect::SyncRepository {
                repo_id,
                generation,
            } => {
                let failed = move |error| ShellMessage::RepositorySynced {
                    generation,
                    result: Err(error),
                };
                spawn_service(
                    app.services.as_ref(),
                    context,
                    "资源库同步",
                    failed,
                    |services| {
                        let tasks = services.tasks.clone();
                        move || {
                            let result = tasks
                                .run::<RepositorySync>(SyncRequest { repo_id })
                                .map(|output| SyncCounts::from(&output.result));
                            ShellMessage::RepositorySynced { generation, result }
                        }
                    },
                );
            }
            WorkspaceEffect::LoadSnapshot {
                repo_id,
                generation,
            } => {
                let failed = move |error| ShellMessage::RepositorySnapshotLoaded {
                    generation,
                    result: Err(error),
                };
                spawn_service(
                    app.services.as_ref(),
                    context,
                    "资源库摘要",
                    failed,
                    |services| {
                        let query = services.repository_query.clone();
                        move || ShellMessage::RepositorySnapshotLoaded {
                            generation,
                            result: query.get_repository_snapshot(repo_id),
                        }
                    },
                );
            }
            WorkspaceEffect::LoadFirstScreen { repo_id } => {
                super::app::dispatch_first_screen(app, context, repo_id)
            }
            WorkspaceEffect::RelocateRepository { repo_id, path } => {
                let failed = |error| ShellMessage::MissingRelocateFinished(Err(error));
                spawn_service(
                    app.services.as_ref(),
                    context,
                    "资源库重定向",
                    failed,
                    |services| {
                        let tasks = services.tasks.clone();
                        move || {
                            let result = tasks
                                .run::<RepositoryRelocate>(RepositoryRelocateRequest {
                                    repo_id,
                                    path,
                                })
                                .map(|output| output.result.repository);
                            ShellMessage::MissingRelocateFinished(result)
                        }
                    },
                );
            }
            WorkspaceEffect::DeleteRepository { repo_id, mode } => {
                let mode = match mode {
                    DeleteMode::RecordOnly => RepositoryDeleteMode::RecordOnly,
                    DeleteMode::DeleteMetadata => RepositoryDeleteMode::DeleteMetadata,
                    DeleteMode::DeleteFolder => RepositoryDeleteMode::DeleteFolder,
                };
                let failed = |error| ShellMessage::MissingDeleteFinished(Err(error));
                spawn_service(
                    app.services.as_ref(),
                    context,
                    "资源库删除",
                    failed,
                    |services| {
                        let management = services.repository_management.clone();
                        move || {
                            let request = RepositoryDeleteRequest { repo_id, mode };
                            ShellMessage::MissingDeleteFinished(
                                management.delete_repository(request),
                            )
                        }
                    },
                );
            }
            WorkspaceEffect::RefreshRepositoriesSilent => {
                dispatch_silent_list(app, context);
            }
            WorkspaceEffect::LoadSnapshotSilent { repo_id } => {
                dispatch_silent_snapshot(app, context, repo_id);
            }
        }
    }
}

/// 后台读取仓库列表。失败（含服务没起来、任务排不进去和 panic，`dispatch/task_guard.rs`）交回静默归约，不增加列表代次。
fn dispatch_silent_list(app: &MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let failed = |error| ShellMessage::SilentWorkspace(SilentMessage::Repositories(Err(error)));
    spawn_service(
        app.services.as_ref(),
        context,
        "静默资源库列表",
        failed,
        |services| {
            let query = services.repository_query.clone();
            move || {
                ShellMessage::SilentWorkspace(SilentMessage::Repositories(
                    query.list_repositories(),
                ))
            }
        },
    );
}

/// 后台读取仓库摘要。失败走静默消息，不把页面改成错误。
fn dispatch_silent_snapshot(
    app: &MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
) {
    let failed = |error| ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Err(error)));
    spawn_service(
        app.services.as_ref(),
        context,
        "静默资源库摘要",
        failed,
        |services| {
            let query = services.repository_query.clone();
            move || {
                ShellMessage::SilentWorkspace(SilentMessage::Snapshot(
                    query.get_repository_snapshot(repo_id),
                ))
            }
        },
    );
}
