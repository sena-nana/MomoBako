//! 结构更新后的静默仓库列表和摘要。
//!
//! 只在启动就绪时写入列表、名称和侧栏计数。
//! 不改页面、不推进启动步骤，也不清空缺失仓库已经显示的内容。

use nana_ui::runtime::Task;
use nana_ui::RuntimeProgramContext;

use crate::backend::services::repository::{RepositorySnapshot, RepositorySummary};
use crate::MomoBakoApplication;

use super::sidebar::{ShortcutAsset, SidebarShortcut};
use super::{ShellMessage, ShellViewModel, StartupStatus, WorkspaceRepository};

/// 静默列表或摘要的服务结果。失败只记日志，不把页面改成错误。
#[derive(Clone, Debug)]
pub enum SilentMessage {
    Repositories(Result<Vec<RepositorySummary>, String>),
    Snapshot(Result<RepositorySnapshot, String>),
}

/// 归约静默列表或摘要。不是这两类消息时交回主归约。
pub(crate) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    let ShellMessage::SilentWorkspace(message) = message else {
        return Some(message);
    };
    match message {
        SilentMessage::Repositories(result) => apply_repositories(model, result),
        SilentMessage::Snapshot(result) => apply_snapshot(model, result),
    }
    None
}

/// 就绪后替换仓库列表。活动仓库缺失或不在列表中时保留现有内容。
fn apply_repositories(model: &mut ShellViewModel, result: Result<Vec<RepositorySummary>, String>) {
    let items = match result {
        Err(error) => {
            eprintln!("Nana 静默刷新仓库列表失败：{error}");
            return;
        }
        Ok(items) => items,
    };
    if model.workspace.startup.status != StartupStatus::Ready {
        eprintln!("Nana 启动未完成，忽略静默仓库列表");
        return;
    }
    let items: Vec<WorkspaceRepository> = items.iter().map(WorkspaceRepository::from_summary).collect();
    let active_id = model.workspace.active_repo_id.clone();
    model.workspace.repositories = items;
    let Some(active_id) = active_id else {
        eprintln!("Nana 静默列表没有活动仓库，只替换列表");
        return;
    };
    let Some(current) = model.workspace.repositories.iter().find(|item| item.repo_id == active_id) else {
        eprintln!("Nana 静默列表里没有当前仓库，保留现有内容：{active_id}");
        return;
    };
    if current.status == "missing" {
        eprintln!("Nana 静默列表显示仓库缺失，不重置内容：{active_id}");
        return;
    }
    model.repository_name = current.name.clone();
    model.repository_id = Some(current.repo_id.clone());
}

/// 就绪且仍是当前仓库时更新名称、摘要和快捷计数。文件夹树刷新重读的摘要也走这里。
pub(super) fn apply_snapshot(model: &mut ShellViewModel, result: Result<RepositorySnapshot, String>) {
    let snapshot = match result {
        Err(error) => {
            eprintln!("Nana 静默刷新仓库摘要失败：{error}");
            return;
        }
        Ok(snapshot) => snapshot,
    };
    if model.workspace.startup.status != StartupStatus::Ready {
        eprintln!("Nana 启动未完成，忽略静默仓库摘要：{}", snapshot.repository.repo_id);
        return;
    }
    if model.workspace.active_repo_id.as_deref() != Some(snapshot.repository.repo_id.as_str()) {
        eprintln!("Nana 静默摘要与当前仓库不一致，已忽略：{}", snapshot.repository.repo_id);
        return;
    }
    if model.navigation_locked() {
        eprintln!("Nana 资源库缺失，忽略静默摘要：{}", snapshot.repository.repo_id);
        return;
    }
    model.repository_name = snapshot.repository.name.clone();
    model.repository_id = Some(snapshot.repository.repo_id.clone());
    model.detail = format!(
        "{} 个文件 · {} 个文件夹 · {}",
        snapshot.overview.file_count,
        snapshot.overview.folder_count,
        snapshot.repository.status
    );
    let assets = snapshot
        .assets
        .iter()
        .map(|asset| ShortcutAsset {
            path: asset.path.clone(),
            untagged: asset.tags.is_empty(),
            accessed: asset.last_accessed_at.is_some(),
            deleted: asset.status == "deleted",
        })
        .collect::<Vec<_>>();
    let quick_access = snapshot
        .quick_access
        .iter()
        .map(|shortcut| SidebarShortcut {
            id: shortcut.shortcut_id.clone(),
            label: shortcut.label.clone(),
            target_kind: shortcut.target_kind.clone(),
            target_path: shortcut.target_path.clone(),
            target_id: shortcut.target_id.clone(),
        })
        .collect();
    model.sidebar.apply_snapshot(&assets, snapshot.overview.trash_count, quick_access);
}

/// 后台读取仓库列表。服务未启动时把错误交回静默归约，不增加列表代次。
pub(crate) fn dispatch_silent_list(app: &mut MomoBakoApplication, context: &RuntimeProgramContext<ShellMessage>) {
    let prepared = app.services.as_ref().map(|services| {
        (services.repository_query.clone(), services.executor.clone())
    });
    let Some((query, executor)) = prepared else {
        eprintln!("Nana 静默刷新仓库列表需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::SilentWorkspace(SilentMessage::Repositories(Err(
            "领域服务未启动".into(),
        ))));
        return;
    };
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::SilentWorkspace(SilentMessage::Repositories(executor.block_on(query.list_repositories())))
    })) {
        eprintln!("Nana 静默刷新仓库列表任务提交失败：{error}");
        app.shell.reduce(ShellMessage::SilentWorkspace(SilentMessage::Repositories(Err(
            format!("资源库列表任务提交失败：{error}"),
        ))));
    }
}

/// 后台读取仓库摘要。失败走静默消息，不把页面改成错误。
pub(crate) fn dispatch_silent_snapshot(
    app: &mut MomoBakoApplication,
    context: &RuntimeProgramContext<ShellMessage>,
    repo_id: String,
) {
    let prepared = app.services.as_ref().map(|services| {
        (services.repository_query.clone(), services.executor.clone())
    });
    let Some((query, executor)) = prepared else {
        eprintln!("Nana 静默刷新仓库摘要需要领域服务，当前服务未启动");
        app.shell.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Err(
            "领域服务未启动".into(),
        ))));
        return;
    };
    if let Err(error) = context.run_task(Task::new(async move {
        ShellMessage::SilentWorkspace(SilentMessage::Snapshot(
            executor.block_on(query.get_repository_snapshot(repo_id)),
        ))
    })) {
        eprintln!("Nana 静默刷新仓库摘要任务提交失败：{error}");
        app.shell.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Err(format!(
            "资源库摘要任务提交失败：{error}"
        )))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::services::repository::{
        AssetSummary, RepositoryBackendSummary, RepositoryOverview, RepositoryShortcut,
    };
    use crate::shell::{MainRegion, ShellPage, WorkspaceEffect, WorkspaceRepository};

    fn summary(repo_id: &str, name: &str, status: &str) -> RepositorySummary {
        RepositorySummary {
            repo_id: repo_id.into(),
            name: name.into(),
            path: "C:/lib".into(),
            backend: RepositoryBackendSummary {
                plugin_id: "local".into(),
                kind: "local".into(),
                name: "本地".into(),
                capabilities: vec!["write".into()],
            },
            status: status.into(),
            asset_count: 1,
            updated_at: String::new(),
            local_cache: None,
            authentication: None,
        }
    }

    fn repository(repo_id: &str, name: &str, status: &str) -> WorkspaceRepository {
        WorkspaceRepository::from_summary(&summary(repo_id, name, status))
    }

    fn asset() -> AssetSummary {
        AssetSummary {
            asset_id: "a".into(),
            repo_id: "repo".into(),
            path: "a.png".into(),
            filename: "a.png".into(),
            extension: "png".into(),
            size_bytes: 1,
            size_label: "1 B".into(),
            status: "ready".into(),
            modified_at: String::new(),
            last_accessed_at: Some("t".into()),
            version: 1,
            tags: Vec::new(),
            thumbnail_path: None,
            hardlink_group_id: None,
            hardlink_state: None,
            is_virtual: false,
            provider_id: None,
            provider_item_id: None,
            source_payload: None,
            local_absolute_path: None,
        }
    }

    fn snapshot(repo_id: &str, name: &str) -> RepositorySnapshot {
        RepositorySnapshot {
            repository: summary(repo_id, name, "ready"),
            folder_label: String::new(),
            folders: Vec::new(),
            assets: vec![asset()],
            playlists: Vec::new(),
            quick_access: vec![RepositoryShortcut {
                shortcut_id: "pin".into(),
                label: "固定".into(),
                target_kind: "folder".into(),
                target_path: Some("photos".into()),
                target_id: None,
            }],
            tag_groups: Vec::new(),
            metadata_fields: Vec::new(),
            recent_revision_count: 0,
            overview: RepositoryOverview {
                total_size_bytes: 1,
                total_size_label: "1 B".into(),
                file_count: 4,
                folder_count: 1,
                trash_count: 2,
                readme_content: None,
            },
        }
    }

    /// 用正式列表路径把存在状态设为已就绪，再清掉它附带的摘要请求。
    fn ready_shell() -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model.page = ShellPage::FileList;
        model.workspace.startup.finish();
        model.workspace.apply_repository_list(None, Ok(vec![repository("repo", "库", "ready")]));
        model.workspace.take_effects();
        model.repository_name = "库".into();
        model.repository_id = Some("repo".into());
        model.file_entries = vec!["keep.png".into()];
        model.detail = "保持".into();
        model.sidebar.tree_loading = false;
        model
    }

    fn send_list(model: &mut ShellViewModel, result: Result<Vec<RepositorySummary>, String>) {
        model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Repositories(result)));
    }

    #[test]
    fn silent_list_ignores_errors_and_unready_results() {
        let mut failed = ready_shell();
        send_list(&mut failed, Err("列表不可用".into()));
        assert_eq!(failed.workspace.repositories[0].name, "库");
        assert_eq!(failed.workspace.active_repo_id.as_deref(), Some("repo"));
        assert!(failed.workspace.missing_error.is_empty());
        assert_eq!(failed.page, ShellPage::FileList);
        assert_eq!(failed.workspace.startup.status, StartupStatus::Ready);
        assert_eq!(failed.file_entries, vec!["keep.png".to_string()]);
        assert_eq!(failed.detail, "保持");

        let mut loading = ready_shell();
        loading.workspace.startup.begin();
        send_list(&mut loading, Ok(vec![summary("repo", "别的", "ready")]));
        assert_eq!(loading.workspace.repositories[0].name, "库");
        assert_eq!(loading.workspace.active_repo_id.as_deref(), Some("repo"));
        assert_eq!(loading.page, ShellPage::FileList);
        assert_eq!(loading.workspace.startup.status, StartupStatus::Loading);
        assert_eq!(loading.repository_name, "库");
        assert_eq!(loading.workspace.list_generation, 0);
    }

    #[test]
    fn silent_list_renames_without_switching_repository() {
        let mut model = ready_shell();
        let generation = model.workspace.list_generation;
        send_list(&mut model, Ok(vec![summary("repo", "新库", "ready")]));
        assert_eq!(model.workspace.active_repo_id.as_deref(), Some("repo"));
        assert_eq!(model.workspace.repositories[0].name, "新库");
        assert_eq!(model.repository_name, "新库");
        assert_eq!(model.repository_id.as_deref(), Some("repo"));
        assert!(!model.workspace.stop_playback);
        assert!(model.workspace.effects.iter().all(|effect| !matches!(
            effect,
            WorkspaceEffect::StopPlayback { .. }
                | WorkspaceEffect::LoadSnapshot { .. }
                | WorkspaceEffect::SyncRepository { .. }
                | WorkspaceEffect::RefreshRepositories { .. }
        )));
        assert_eq!(model.page, ShellPage::FileList);
        assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
        assert_eq!(model.workspace.list_generation, generation);
        assert_eq!(model.file_entries, vec!["keep.png".to_string()]);
        assert_eq!(model.detail, "保持");
        assert_eq!(model.workspace.main_region(), MainRegion::HasRepository);
    }

    #[test]
    fn silent_list_keeps_content_when_repository_is_missing_or_absent() {
        let mut missing = ready_shell();
        let region = missing.workspace.main_region();
        send_list(&mut missing, Ok(vec![summary("repo", "丢失库", "missing")]));
        assert_eq!(missing.workspace.repositories[0].status, "missing");
        assert_eq!(missing.workspace.active_repo_id.as_deref(), Some("repo"));
        assert_eq!(missing.repository_name, "库");
        assert_eq!(missing.page, ShellPage::FileList);
        assert_eq!(missing.workspace.main_region(), region);
        assert_eq!(missing.workspace.main_region(), MainRegion::HasRepository);
        assert_eq!(missing.file_entries, vec!["keep.png".to_string()]);
        assert_eq!(missing.detail, "保持");
        assert!(missing.navigation_locked());
        assert!(missing.workspace.effects.is_empty());

        let mut absent = ready_shell();
        send_list(&mut absent, Ok(vec![summary("other", "别处", "ready")]));
        assert_eq!(absent.workspace.repositories[0].repo_id, "other");
        assert_eq!(absent.workspace.active_repo_id.as_deref(), Some("repo"));
        assert_eq!(absent.repository_name, "库");
        assert_eq!(absent.file_entries, vec!["keep.png".to_string()]);
        assert_eq!(absent.page, ShellPage::FileList);
        assert_eq!(absent.workspace.main_region(), MainRegion::HasRepository);
        assert!(absent.workspace.effects.is_empty());
    }

    #[test]
    fn silent_snapshot_updates_detail_without_loading_tree() {
        let mut model = ready_shell();
        model.sidebar.folders.push(crate::shell::SidebarFolder {
            path: "photos".into(),
            label: "照片".into(),
            children: Vec::new(),
        });
        model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Ok(snapshot("repo", "新库")))));
        assert_eq!(model.detail, "4 个文件 · 1 个文件夹 · ready");
        assert_eq!(model.repository_name, "新库");
        assert_eq!(model.repository_id.as_deref(), Some("repo"));
        assert_eq!(model.sidebar.counts.all, 1);
        assert_eq!(model.sidebar.counts.uncategorized, 1);
        assert_eq!(model.sidebar.counts.untagged, 1);
        assert_eq!(model.sidebar.counts.recent, 1);
        assert_eq!(model.sidebar.counts.trash, 2);
        assert_eq!(model.sidebar.quick_access.len(), 1);
        assert_eq!(model.sidebar.quick_access[0].id, "pin");
        assert_eq!(model.sidebar.quick_access[0].label, "固定");
        assert!(!model.sidebar.tree_loading);
        assert_eq!(model.sidebar.folders.len(), 1);
        assert!(model.sidebar.take_effects().is_empty());
        assert_eq!(model.page, ShellPage::FileList);
        assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
        assert!(model.workspace.effects.is_empty());
    }

    #[test]
    fn silent_snapshot_ignores_errors_and_other_repositories() {
        let mut model = ready_shell();
        model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Err("摘要不可用".into()))));
        assert_eq!(model.page, ShellPage::FileList);
        assert_ne!(model.page, ShellPage::Error);
        assert_eq!(model.detail, "保持");
        assert_eq!(model.sidebar.counts.all, 0);

        model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Ok(snapshot("other", "别处")))));
        assert_eq!(model.detail, "保持");
        assert_eq!(model.repository_name, "库");
        assert_eq!(model.sidebar.counts.all, 0);
        assert!(model.sidebar.quick_access.is_empty());
        assert!(!model.sidebar.tree_loading);
        assert_eq!(model.page, ShellPage::FileList);

        model.workspace.startup.begin();
        model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Ok(snapshot("repo", "新库")))));
        assert_eq!(model.detail, "保持");
        assert_eq!(model.workspace.startup.status, StartupStatus::Loading);
        assert_eq!(model.page, ShellPage::FileList);

        model.workspace.startup.finish();
        model.workspace.repositories[0].status = "missing".into();
        model.reduce(ShellMessage::SilentWorkspace(SilentMessage::Snapshot(Ok(snapshot("repo", "新库")))));
        assert_eq!(model.detail, "保持");
        assert_eq!(model.repository_name, "库");
        assert!(!model.sidebar.tree_loading);
        assert_eq!(model.sidebar.counts.all, 0);
        assert_eq!(model.page, ShellPage::FileList);
        assert_eq!(model.workspace.main_region(), MainRegion::HasRepository);
    }
}
