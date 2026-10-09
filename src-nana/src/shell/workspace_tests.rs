//! 工作台状态机回归。这些分支来自 Vue 启动、主区和缺失仓库动作，不依赖离屏画面。

use std::fs;

use super::*;
use crate::theme_map::{SIDEBAR_DEFAULT_PX, SIDEBAR_MAX_PX, SIDEBAR_MIN_PX};

fn repo(id: &str, status: &str) -> WorkspaceRepository {
    WorkspaceRepository {
        repo_id: id.into(),
        name: id.into(),
        path: format!("C:/{id}"),
        status: status.into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: vec!["localRootPath".into()],
        cache_required: false,
        cache_status: String::new(),
    }
}

#[test]
fn failed_middle_step_keeps_earlier_steps_done() {
    let mut startup = StartupState::default();
    startup.begin();
    startup.set_progress(1, "加载仓库列表", "读取列表");
    startup.set_progress(2, "扫描资源库文件", "同步");
    startup.set_progress(3, "读取仓库摘要", "索引");
    let percent = startup.percent;
    startup.fail("索引损坏");
    let states: Vec<_> = startup.step_items().into_iter().map(|item| item.state).collect();
    assert_eq!(
        states,
        vec![
            StartupStepState::Done,
            StartupStepState::Done,
            StartupStepState::Error,
            StartupStepState::Pending,
        ]
    );
    assert_eq!(startup.current_step, 3);
    assert_eq!(startup.percent, percent);
    assert_eq!(startup.step_label, "加载失败");
    assert_eq!(startup.error.as_deref(), Some("索引损坏"));
}

#[test]
fn retry_restarts_from_the_first_step() {
    let mut state = WorkspaceState::default();
    state.startup.set_progress(3, "读取仓库摘要", "索引");
    state.startup.fail("索引损坏");
    assert!(state.retry_startup());
    assert_eq!(state.startup.status, StartupStatus::Loading);
    assert_eq!(state.startup.current_step, 1);
    assert!(state.startup.error.is_none());
    assert!(matches!(
        state.effects.last(),
        Some(WorkspaceEffect::RefreshRepositories { .. })
    ));
    state.startup.finish();
    assert!(!state.retry_startup());
}

#[test]
fn repository_regions_are_mutually_exclusive() {
    let mut state = WorkspaceState::default();
    state.prepare_initial_list();
    assert_eq!(state.main_region(), MainRegion::Startup);

    state.apply_repository_list(Some(state.list_generation), Ok(vec![]));
    assert_eq!(state.main_region(), MainRegion::EmptyRepository);
    assert!(state.last_active_repo_id.is_none());

    let mut missing = WorkspaceState::default();
    missing.prepare_initial_list();
    missing.apply_repository_list(Some(missing.list_generation), Ok(vec![repo("gone", "missing")]));
    assert_eq!(missing.main_region(), MainRegion::MissingRepository);

    let mut ready = WorkspaceState::default();
    ready.prepare_initial_list();
    ready.apply_repository_list(Some(ready.list_generation), Ok(vec![repo("live", "ready")]));
    assert_eq!(ready.main_region(), MainRegion::Startup);
    let generation = ready.list_generation;
    ready.note_sync_finished(generation, Ok(()));
    ready.note_index_finished("live", Ok(()));
    ready.note_first_screen_finished(Ok(()));
    assert_eq!(ready.main_region(), MainRegion::HasRepository);

    let mut failed = WorkspaceState::default();
    failed.prepare_initial_list();
    failed.apply_repository_list(Some(failed.list_generation), Err("列表不可用".into()));
    assert_eq!(failed.main_region(), MainRegion::LoadError);
    assert_ne!(failed.main_region(), MainRegion::EmptyRepository);
}

#[test]
fn selection_prefers_active_then_remembered_then_first() {
    let mut state = WorkspaceState::default();
    state.prepare_initial_list();
    state.active_repo_id = Some("active".into());
    state.last_active_repo_id = Some("remembered".into());
    state.apply_repository_list(
        Some(state.list_generation),
        Ok(vec![repo("first", "ready"), repo("remembered", "ready"), repo("active", "ready")]),
    );
    assert_eq!(state.active_repo_id.as_deref(), Some("active"));
    assert!(state.effects.iter().any(|effect| matches!(
        effect,
        WorkspaceEffect::SyncRepository { repo_id, .. } if repo_id == "active"
    )));

    let mut remembered = WorkspaceState::default();
    remembered.prepare_initial_list();
    remembered.last_active_repo_id = Some("remembered".into());
    remembered.apply_repository_list(
        Some(remembered.list_generation),
        Ok(vec![repo("first", "ready"), repo("remembered", "missing")]),
    );
    assert_eq!(remembered.active_repo_id.as_deref(), Some("remembered"));
    assert_eq!(remembered.main_region(), MainRegion::MissingRepository);
    assert!(remembered.effects.iter().all(|effect| !matches!(effect, WorkspaceEffect::SyncRepository { .. })));
}

#[test]
fn switching_repository_stops_playback_once_previous_id_exists() {
    let mut state = WorkspaceState::default();
    state.prepare_initial_list();
    state.apply_repository_list(Some(state.list_generation), Ok(vec![repo("a", "ready"), repo("b", "ready")]));
    assert!(!state.stop_playback);
    state.effects.clear();
    state.select_repository("a");
    assert!(!state.stop_playback);
    state.select_repository("b");
    assert!(state.stop_playback);
    assert!(state.effects.iter().any(|effect| matches!(
        effect,
        WorkspaceEffect::StopPlayback { previous_repo_id } if previous_repo_id == "a"
    )));
    assert!(state.missing_error.is_empty());
}

#[test]
fn sidebar_storage_matches_vue_ranges_and_keys() {
    let mut state = WorkspaceState::default();
    state.set_sidebar_width(10.0);
    assert_eq!(state.sidebar_width, SIDEBAR_MIN_PX);
    state.set_sidebar_width(900.0);
    assert_eq!(state.sidebar_width, SIDEBAR_MAX_PX);
    state.set_sidebar_width(f32::NAN);
    assert_eq!(state.sidebar_width, SIDEBAR_DEFAULT_PX);
    state.commit_sidebar_width();
    state.toggle_sidebar();
    assert!(state.sidebar_collapsed);
    assert_eq!(state.sidebar_collapsed_storage(), "1");
    assert!(state.effects.iter().filter(|effect| matches!(effect, WorkspaceEffect::PersistSidebar)).count() >= 2);

    let path = std::env::temp_dir().join(format!("momobako-sidebar-{}", std::process::id()));
    let file = path.join("sidebar.json");
    state.sidebar_width = 320.0;
    state.save_prefs_file(&file);
    let mut loaded = WorkspaceState::default();
    loaded.load_prefs_file(&file);
    assert!(loaded.sidebar_collapsed);
    assert_eq!(loaded.sidebar_width, 320.0);
    let raw = fs::read_to_string(&file).expect("sidebar prefs");
    assert!(raw.contains(SIDEBAR_COLLAPSED_KEY));
    assert!(raw.contains(SIDEBAR_WIDTH_KEY));
    let _ = fs::remove_dir_all(path);

    let mut parsed = WorkspaceState::default();
    parsed.apply_sidebar_storage(Some("0"), Some("nope"));
    assert!(!parsed.sidebar_collapsed);
    assert_eq!(parsed.sidebar_width, SIDEBAR_DEFAULT_PX);
}

#[test]
fn missing_actions_reject_duplicates_and_blank_paths() {
    let mut state = WorkspaceState::default();
    state.repositories = vec![repo("missing", "missing")];
    state.active_repo_id = Some("missing".into());
    state.startup.finish();
    state.missing_error = "旧错误".into();
    state.relocating = true;
    state.refresh_missing();
    state.choose_missing_path();
    state.open_delete_dialog();
    assert!(state.dialogs.is_empty());
    assert!(!state.path_prompt);
    assert_eq!(state.missing_error, "旧错误");

    state.relocating = false;
    state.choose_missing_path();
    assert!(state.path_prompt);
    state.path_draft = "   ".into();
    state.submit_missing_path();
    assert!(!state.relocating);
    assert!(state.effects.iter().all(|effect| !matches!(effect, WorkspaceEffect::RelocateRepository { .. })));

    state.path_draft = "D:/library".into();
    state.submit_missing_path();
    assert!(state.relocating);
    state.open_delete_dialog();
    assert!(state.dialogs.is_empty());
    state.note_relocate_finished(Err("路径无效".into()));
    assert!(!state.relocating);
    assert_eq!(state.missing_error, "路径无效");

    state.repositories.push(repo("other", "ready"));
    state.select_repository("other");
    assert!(state.missing_error.is_empty());
    assert!(state.stop_playback);
}

#[test]
fn delete_dialog_disables_unavailable_modes_while_busy() {
    let mut state = WorkspaceState::default();
    state.repositories = vec![repo("missing", "missing")];
    state.active_repo_id = Some("missing".into());
    state.open_delete_dialog();
    assert!(!state.delete_mode_enabled(DeleteMode::DeleteMetadata));
    assert!(!state.delete_mode_enabled(DeleteMode::DeleteFolder));
    assert!(state.delete_mode_enabled(DeleteMode::RecordOnly));
    assert!(!state.confirm_delete(DeleteMode::DeleteFolder));
    assert!(state.confirm_delete(DeleteMode::RecordOnly));
    assert_eq!(state.deleting_mode, Some(DeleteMode::RecordOnly));
    assert!(!state.confirm_delete(DeleteMode::RecordOnly));
    state.close_delete_dialog();
    assert_eq!(state.dialogs.len(), 1);
    state.note_delete_finished(Err("正在使用".into()));
    assert!(state.deleting_mode.is_none());
    assert!(matches!(
        state.dialogs.last(),
        Some(WorkspaceDialog::Delete(dialog)) if dialog.error == "正在使用"
    ));
    state.note_delete_finished(Ok(()));
    assert_eq!(state.dialogs.len(), 1);

    state.deleting_mode = Some(DeleteMode::RecordOnly);
    state.note_delete_finished(Ok(()));
    assert!(state.dialogs.is_empty());
    assert!(state.repositories.is_empty());
    assert_eq!(state.main_region(), MainRegion::Startup);
    state.startup.finish();
    assert_eq!(state.main_region(), MainRegion::EmptyRepository);
}

#[test]
fn source_cache_issue_requests_settings_instead_of_a_path() {
    let mut repository = repo("remote", "missing");
    repository.cache_required = true;
    repository.cache_status = "unconfigured".into();
    repository.capabilities.clear();
    let mut state = WorkspaceState::default();
    state.repositories = vec![repository];
    state.active_repo_id = Some("remote".into());
    assert_eq!(state.missing_primary_label(), "打开来源设置");
    assert_eq!(state.open_source_settings().as_deref(), Some("filesystem"));
    assert!(state.effects.is_empty(), "打开设置页由壳层归约完成，不再经宿主副作用");
    assert!(!state.path_prompt);
}

#[test]
fn stale_list_generation_does_not_reset_completed_steps() {
    let mut state = WorkspaceState::default();
    let generation = state.prepare_initial_list();
    state.apply_repository_list(Some(generation), Ok(vec![repo("live", "ready")]));
    state.note_sync_finished(generation, Err("同步失败".into()));
    assert_eq!(state.startup.step_state(1), StartupStepState::Done);
    state.apply_repository_list(Some(generation.saturating_sub(1)), Ok(vec![]));
    assert_eq!(state.active_repo_id.as_deref(), Some("live"));
    assert_eq!(state.startup.current_step, 2);
    assert_eq!(state.main_region(), MainRegion::LoadError);
}

#[test]
fn panel_and_category_survive_startup_progress() {
    let mut state = WorkspaceState::default();
    state.panel = WorkspacePanel::Playlist;
    state.library_category = LibraryCategory::Recent;
    state.prepare_initial_list();
    assert_eq!(state.panel, WorkspacePanel::Playlist);
    state.apply_repository_list(Some(state.list_generation), Ok(vec![]));
    assert_eq!(state.panel, WorkspacePanel::Playlist);
    assert_eq!(state.library_category, LibraryCategory::All);
}
