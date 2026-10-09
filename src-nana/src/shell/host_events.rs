//! 宿主事件进入壳层。
//!
//! 日志和资源库结构更新走 `host_event_channel`，不经过 Tauri。
//! 启动中的 `repository.sync` 追加到启动日志。就绪后的结构更新静默刷新当前面板。
//! 列表、摘要和硬链接候选现在会静默重拉。失败不改页面，也不弹出硬链接对话框。

use crate::backend::services::host_events::HostEvent;
use crate::backend::services::repository::SystemLogRecord;

use super::files::FileContext;
use super::{ShellMessage, ShellViewModel, StartupStatus, WorkspaceEffect, WorkspacePanel};

const SYNC_LOG_CATEGORY: &str = "repository.sync";

/// 从领域服务送达的宿主事件。
#[derive(Clone, Debug)]
pub enum HostMessage {
    LogRecorded(SystemLogRecord),
    StructureUpdated { repo_id: String, reason: String },
}

impl HostMessage {
    /// 去掉与界面无关的负载，保留日志记录和结构更新的仓库与原因。
    pub fn from_event(event: HostEvent) -> Self {
        match event {
            HostEvent::LogRecorded(record) => Self::LogRecorded(record),
            HostEvent::RepositoryStructureUpdated(event) => Self::StructureUpdated {
                repo_id: event.repo_id,
                reason: event.reason,
            },
        }
    }
}

/// 归约一条宿主事件。调用方负责把随后的效果发给领域服务。
pub(crate) fn reduce_message(model: &mut ShellViewModel, message: ShellMessage) -> Option<ShellMessage> {
    let ShellMessage::Host(message) = message else {
        return Some(message);
    };
    match message {
        HostMessage::LogRecorded(record) => note_log(model, record),
        HostMessage::StructureUpdated { repo_id, reason } => refresh_structure(model, &repo_id, &reason),
    }
    None
}

/// 实时日志并入管理列表。同步分类在启动加载中才进入启动日志。
fn note_log(model: &mut ShellViewModel, record: SystemLogRecord) {
    let sync_log = record.category == SYNC_LOG_CATEGORY;
    let record_repo = record.source.repo_id.clone();
    let level = record.level.clone();
    let message = record.message.clone();
    model.admin.merge_log(record);
    if !sync_log {
        return;
    }
    let active = model.workspace.active_repo_id.as_deref().unwrap_or("");
    if let Some(repo_id) = record_repo.as_deref() {
        if !repo_id.is_empty() && !active.is_empty() && repo_id != active {
            eprintln!("Nana 忽略其他仓库的启动同步日志：{repo_id}");
            return;
        }
    }
    model.workspace.startup.append_sync_log(&level, message);
}

/// 只在启动完成后刷新当前仓库。缺失仓库和别的仓库直接返回。
fn refresh_structure(model: &mut ShellViewModel, repo_id: &str, reason: &str) {
    if model.workspace.startup.status != StartupStatus::Ready {
        eprintln!("Nana 启动未完成，忽略结构更新：{repo_id}");
        return;
    }
    if model.workspace.active_repo_id.as_deref() != Some(repo_id) {
        eprintln!("Nana 忽略其他仓库的结构更新：{repo_id}");
        return;
    }
    if model.navigation_locked() {
        eprintln!("Nana 资源库缺失，忽略结构更新：{repo_id}");
        return;
    }
    eprintln!("Nana 静默刷新资源库：{repo_id}，原因 {reason}");
    let panel = model.workspace.panel;
    model.sidebar.refresh_after_structure(repo_id, panel);
    model.admin.queue_actions(Some(repo_id.to_string()));
    if matches!(panel, WorkspacePanel::Files | WorkspacePanel::Trash) {
        let context = FileContext::from_model(model);
        model.files.reload_silent(&context);
    }
    model.files.refresh_hardlinks_silent(repo_id);
    model.workspace.effects.push(WorkspaceEffect::RefreshRepositoriesSilent);
    if model.workspace.active_repo_id.is_some() {
        model.workspace.effects.push(WorkspaceEffect::LoadSnapshotSilent { repo_id: repo_id.to_string() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::services::repository::{SystemLogLocation, SystemLogSource};
    use crate::shell::{LibraryCategory, ShellViewModel, WorkspaceRepository};

    fn record(id: &str, timestamp: &str, category: &str, repo_id: &str, message: &str) -> SystemLogRecord {
        SystemLogRecord {
            id: id.into(),
            timestamp: timestamp.into(),
            level: "info".into(),
            category: category.into(),
            action: "sync".into(),
            message: message.into(),
            source: SystemLogSource {
                kind: "host".into(),
                label: None,
                plugin_id: None,
                repo_id: (!repo_id.is_empty()).then(|| repo_id.to_string()),
            },
            location: SystemLogLocation::default(),
            context: serde_json::Value::Null,
        }
    }

    fn repository(status: &str) -> WorkspaceRepository {
        WorkspaceRepository {
            repo_id: "repo".into(),
            name: "库".into(),
            path: "C:/lib".into(),
            status: status.into(),
            backend_plugin_id: "local".into(),
            capabilities: vec!["write".into()],
            cache_required: false,
            cache_status: String::new(),
        }
    }

    fn ready(panel: WorkspacePanel) -> ShellViewModel {
        let mut model = ShellViewModel::default();
        model.workspace.startup.finish();
        model.workspace.repositories.push(repository("ready"));
        model.workspace.active_repo_id = Some("repo".into());
        model.workspace.panel = panel;
        model.bind_sidebar_repository();
        model.sidebar.take_effects();
        model.sidebar.tree_loading = false;
        model.admin.take_effects();
        model.files.take_effects();
        model
    }

    fn send(model: &mut ShellViewModel, message: HostMessage) {
        model.reduce(ShellMessage::Host(message));
    }

    #[test]
    fn logs_merge_by_id_and_keep_newest_first() {
        let mut model = ShellViewModel::default();
        model.admin.log_paused = true;
        send(&mut model, HostMessage::LogRecorded(record("a", "2020", "plugin", "", "旧")));
        send(&mut model, HostMessage::LogRecorded(record("b", "2021", "plugin", "", "新")));
        send(&mut model, HostMessage::LogRecorded(record("a", "2022", "plugin", "", "替换")));
        assert_eq!(
            model.admin.logs.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(model.admin.logs[0].message, "替换");
        assert!(!model.admin.log_would_scroll);
        for index in 0..501 {
            send(
                &mut model,
                HostMessage::LogRecorded(record(&format!("n{index}"), &format!("2030-{index:04}"), "plugin", "", "多")),
            );
        }
        assert_eq!(model.admin.logs.len(), 500);
        assert_eq!(model.admin.logs[0].id, "n500");
    }

    #[test]
    fn startup_sync_logs_follow_the_loading_repository() {
        let mut model = ShellViewModel::default();
        model.workspace.startup.begin();
        model.workspace.active_repo_id = Some("repo".into());
        let begun = vec!["首屏启动流程开始。"];
        let messages = |model: &ShellViewModel| {
            model.workspace.startup.logs.iter().map(|item| item.message.clone()).collect::<Vec<_>>()
        };
        send(&mut model, HostMessage::LogRecorded(record("1", "2020", "plugin", "repo", "插件")));
        assert_eq!(messages(&model), begun);
        send(&mut model, HostMessage::LogRecorded(record("2", "2020", SYNC_LOG_CATEGORY, "other", "别处")));
        assert_eq!(messages(&model), begun);
        send(&mut model, HostMessage::LogRecorded(record("3", "2020", SYNC_LOG_CATEGORY, "", "没有仓库")));
        send(&mut model, HostMessage::LogRecorded(record("4", "2020", SYNC_LOG_CATEGORY, "repo", "当前库")));
        assert_eq!(messages(&model), vec!["首屏启动流程开始。", "没有仓库", "当前库"]);
        model.workspace.startup.finish();
        send(&mut model, HostMessage::LogRecorded(record("5", "2020", SYNC_LOG_CATEGORY, "repo", "完成后")));
        assert_eq!(model.workspace.startup.logs.len(), 3);
        assert_eq!(model.admin.logs.len(), 5);
    }

    #[test]
    fn structure_updates_refresh_the_ready_repository_only() {
        let mut loading = ready(WorkspacePanel::Files);
        loading.workspace.startup.begin();
        send(&mut loading, HostMessage::StructureUpdated { repo_id: "repo".into(), reason: "watcher".into() });
        assert!(loading.sidebar.take_effects().is_empty());
        assert!(loading.workspace.effects.is_empty());

        let mut other = ready(WorkspacePanel::Files);
        send(&mut other, HostMessage::StructureUpdated { repo_id: "other".into(), reason: "watcher".into() });
        assert!(other.sidebar.take_effects().is_empty());
        assert!(other.files.take_effects().is_empty());
        assert!(other.workspace.effects.is_empty());

        let mut missing = ready(WorkspacePanel::Files);
        missing.workspace.repositories[0].status = "missing".into();
        send(&mut missing, HostMessage::StructureUpdated { repo_id: "repo".into(), reason: "watcher".into() });
        assert!(missing.files.take_effects().is_empty());
        assert!(missing.workspace.effects.is_empty());

        let mut files = ready(WorkspacePanel::Files);
        files.files.current_path = "photos".into();
        files.files.selected = vec!["photos/a".into()];
        files.files.primary = Some("photos/a".into());
        files.page = crate::shell::ShellPage::FileList;
        send(&mut files, HostMessage::StructureUpdated { repo_id: "repo".into(), reason: "watcher".into() });
        let sidebar_effects = files.sidebar.take_effects();
        assert!(sidebar_effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::LoadTree { .. })));
        assert!(sidebar_effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::LoadSmartFolders { .. })));
        assert!(sidebar_effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::LoadPlaylists { .. })));
        let browse = files.files.take_effects();
        assert!(matches!(
            browse.as_slice(),
            [
                crate::shell::FilesEffect::Browse { path, trash: false, append: false, .. },
                crate::shell::FilesEffect::RefreshHardlinks { repo_id },
            ] if path == "photos" && repo_id == "repo"
        ));
        assert!(!files.files.loading);
        assert_eq!(files.files.selected, vec!["photos/a".to_string()]);
        assert_eq!(files.files.primary.as_deref(), Some("photos/a"));
        assert!(files.files.note_load_failed("读取失败"));
        assert_eq!(files.page, crate::shell::ShellPage::FileList);
        assert_eq!(files.workspace.startup.status, StartupStatus::Ready);
        assert_eq!(files.workspace.list_generation, 0);
        assert_eq!(
            files.workspace.take_effects(),
            vec![
                WorkspaceEffect::RefreshRepositoriesSilent,
                WorkspaceEffect::LoadSnapshotSilent { repo_id: "repo".into() },
            ]
        );
        assert!(matches!(
            files.admin.take_effects().as_slice(),
            [crate::shell::admin::AdminEffect::LoadActions { .. }]
        ));

        let mut trash = ready(WorkspacePanel::Trash);
        trash.files.current_path = "bin".into();
        send(&mut trash, HostMessage::StructureUpdated { repo_id: "repo".into(), reason: "watcher".into() });
        assert!(matches!(
            trash.files.take_effects().as_slice(),
            [
                crate::shell::FilesEffect::Browse { path, trash: true, .. },
                crate::shell::FilesEffect::RefreshHardlinks { repo_id },
            ] if path == "bin" && repo_id == "repo"
        ));

        let mut category = ready(WorkspacePanel::Files);
        category.workspace.library_category = LibraryCategory::Untagged;
        send(&mut category, HostMessage::StructureUpdated { repo_id: "repo".into(), reason: "watcher".into() });
        assert!(matches!(
            category.files.take_effects().as_slice(),
            [crate::shell::FilesEffect::RefreshHardlinks { repo_id }] if repo_id == "repo"
        ));

        let mut playlist = ready(WorkspacePanel::Playlist);
        playlist.sidebar.active_playlist_id = Some("list".into());
        send(&mut playlist, HostMessage::StructureUpdated { repo_id: "repo".into(), reason: "watcher".into() });
        assert!(playlist.sidebar.take_effects().iter().any(|effect| {
            matches!(effect, crate::shell::SidebarEffect::LoadPlaylistDetail { playlist_id, .. } if playlist_id == "list")
        }));
        assert!(matches!(
            playlist.files.take_effects().as_slice(),
            [crate::shell::FilesEffect::RefreshHardlinks { repo_id }] if repo_id == "repo"
        ));

        let mut busy = ready(WorkspacePanel::Files);
        busy.sidebar.tree_loading = true;
        send(&mut busy, HostMessage::StructureUpdated { repo_id: "repo".into(), reason: "watcher".into() });
        let busy_effects = busy.sidebar.take_effects();
        assert!(!busy_effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::LoadTree { .. })));
        assert!(busy_effects.iter().any(|effect| matches!(effect, crate::shell::SidebarEffect::LoadPlaylists { .. })));
    }
}
