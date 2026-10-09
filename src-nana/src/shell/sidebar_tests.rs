//! 侧栏导航回归。覆盖快捷方式、目录、智能文件夹、播放集和仓库切换弹层。

use super::*;

fn workspace_with(repo_id: &str) -> WorkspaceState {
    let mut workspace = WorkspaceState::default();
    workspace.active_repo_id = Some(repo_id.into());
    workspace
}

fn asset(path: &str, untagged: bool, accessed: bool, deleted: bool) -> ShortcutAsset {
    ShortcutAsset { path: path.into(), untagged, accessed, deleted }
}

fn shortcut(id: &str, kind: &str, path: Option<&str>, target_id: Option<&str>) -> SidebarShortcut {
    SidebarShortcut {
        id: id.into(),
        label: id.into(),
        target_kind: kind.into(),
        target_path: path.map(str::to_owned),
        target_id: target_id.map(str::to_owned),
    }
}

#[test]
fn shortcut_counts_skip_deleted_assets_and_use_trash_overview() {
    let counts = count_shortcuts(
        &[
            asset("root.png", true, true, false),
            asset("photos/cat.png", false, false, false),
            asset("gone.png", true, true, true),
        ],
        4,
    );
    assert_eq!(counts.all, 2);
    assert_eq!(counts.uncategorized, 1);
    assert_eq!(counts.untagged, 1);
    assert_eq!(counts.recent, 1);
    assert_eq!(counts.trash, 4);
}

#[test]
fn missing_repository_blocks_shortcut_quick_access_and_folder_open() {
    let mut sidebar = SidebarState::default();
    sidebar.quick_access = vec![shortcut("jump", "file", Some("photos/cat.png"), None)];
    let mut workspace = workspace_with("repo");
    assert!(!sidebar.select_shortcut(&mut workspace, ShortcutId::Trash, true));
    assert!(!sidebar.open_quick_access(&mut workspace, "jump", true));
    assert!(!sidebar.open_folder(&mut workspace, "photos", true));
    assert!(sidebar.effects.is_empty());
    assert_eq!(workspace.panel, WorkspacePanel::Files);
}

#[test]
fn trash_shortcut_browses_trash_and_files_shortcut_leaves_it() {
    let mut sidebar = SidebarState::default();
    let mut workspace = workspace_with("repo");
    assert!(sidebar.select_shortcut(&mut workspace, ShortcutId::Trash, false));
    assert_eq!(workspace.panel, WorkspacePanel::Trash);
    assert!(sidebar.browsing_trash);
    assert!(sidebar.select_shortcut(&mut workspace, ShortcutId::Untagged, false));
    assert_eq!(workspace.panel, WorkspacePanel::Files);
    assert_eq!(workspace.library_category, LibraryCategory::Untagged);
    assert!(!sidebar.browsing_trash);
    let browse: Vec<_> = sidebar.effects.iter().filter_map(|effect| match effect {
        SidebarEffect::Browse { trash, path, .. } => Some((*trash, path.clone())),
        _ => None,
    }).collect();
    assert_eq!(browse, vec![(true, String::new()), (false, String::new())]);
}

#[test]
fn quick_access_opens_smart_folder_file_parent_and_directory() {
    let mut sidebar = SidebarState::default();
    sidebar.quick_access = vec![
        shortcut("smart", "smartFolder", None, Some("sf-1")),
        shortcut("file", "file", Some("photos\\cat.png"), None),
        shortcut("dir", "folder", Some("photos/trips"), None),
        shortcut("empty-file", "file", None, None),
    ];
    let mut workspace = workspace_with("repo");
    assert!(sidebar.open_quick_access(&mut workspace, "smart", false));
    assert_eq!(workspace.panel, WorkspacePanel::SmartFolder);
    assert!(sidebar.effects.iter().any(|effect| matches!(
        effect,
        SidebarEffect::QuerySmartFolder { smart_folder_id, .. } if smart_folder_id == "sf-1"
    )));

    sidebar.effects.clear();
    assert!(sidebar.open_quick_access(&mut workspace, "file", false));
    assert_eq!(sidebar.current_directory, "photos");
    assert_eq!(sidebar.selected_path.as_deref(), Some("photos/cat.png"));
    assert_eq!(workspace.library_category, LibraryCategory::All);

    sidebar.effects.clear();
    assert!(sidebar.open_quick_access(&mut workspace, "dir", false));
    assert_eq!(sidebar.current_directory, "photos/trips");
    assert!(!sidebar.open_quick_access(&mut workspace, "empty-file", false));
    assert!(!sidebar.open_quick_access(&mut workspace, "missing", false));
}

#[test]
fn folder_expansion_follows_current_directory_and_prunes_stale_paths() {
    let mut sidebar = SidebarState::default();
    sidebar.bind_repository(Some("repo"), false);
    sidebar.note_directory("photos/trips", false);
    assert_eq!(sidebar.expanded_folders, ["photos", "photos/trips"]);
    sidebar.toggle_folder("photos");
    assert!(!sidebar.expanded_folders.iter().any(|path| path == "photos"));
    sidebar.apply_tree("other", Ok(Vec::new().into()));
    assert_eq!(sidebar.expanded_folders, ["photos/trips"]);
    sidebar.apply_tree("repo", Ok(vec![SidebarFolder {
        path: "photos/trips".into(),
        label: "trips".into(),
        children: Vec::new(),
    }].into()));
    assert_eq!(sidebar.expanded_folders, ["photos/trips"]);
    assert!(!sidebar.tree_loading);
    assert!(sidebar.refresh_tree(Some("repo"), false));
    assert!(sidebar.tree_loading);
    sidebar.tree_loading = true;
    assert!(!sidebar.refresh_tree(Some("repo"), false));
}

#[test]
fn smart_folder_query_ignores_stale_results_and_expands_ancestors() {
    let mut sidebar = SidebarState::default();
    sidebar.bind_repository(Some("repo"), false);
    sidebar.apply_smart_folders("repo", Ok(vec![SidebarSmartFolder {
        id: "root".into(),
        parent_id: None,
        name: "根".into(),
        filter: Default::default(),
        children: vec![SidebarSmartFolder {
            id: "child".into(),
            parent_id: Some("root".into()),
            name: "子".into(),
            filter: Default::default(),
            children: Vec::new(),
        }],
    }]));
    let mut workspace = workspace_with("repo");
    assert!(!sidebar.select_smart_folder(&mut workspace, "", false));
    assert!(sidebar.select_smart_folder(&mut workspace, "child", false));
    assert_eq!(sidebar.expanded_smart_folders, ["root"]);
    assert!(sidebar.smart_loading);
    assert!(!sidebar.note_smart_query("repo", "other", Ok(3)));
    assert!(sidebar.smart_loading);
    assert!(sidebar.note_smart_query("repo", "child", Err("查询失败".into())));
    assert!(!sidebar.smart_loading);
    assert_eq!(sidebar.smart_error, "查询失败");
    assert!(sidebar.note_smart_query("repo", "child", Ok(2)));
    assert_eq!(sidebar.smart_result_count, Some(2));
    assert!(sidebar.smart_error.is_empty());
}

#[test]
fn playlist_selection_clears_when_the_list_drops_the_active_id() {
    let mut sidebar = SidebarState::default();
    sidebar.bind_repository(Some("repo"), false);
    sidebar.apply_playlist_players("repo", Ok(vec!["audio".into()]));
    let mut workspace = workspace_with("repo");
    assert!(!sidebar.select_playlist(&mut workspace, ""));
    assert!(sidebar.select_playlist(&mut workspace, "pl-1"));
    assert_eq!(workspace.panel, WorkspacePanel::Playlist);
    sidebar.playlists = vec![SidebarPlaylist {
        id: "pl-1".into(),
        name: "早晨".into(),
        player_label: "音频".into(),
        player_type_id: "audio".into(),
        item_count: 2,
    }];
    assert!(sidebar.playlist_playable("pl-1"));
    sidebar.apply_playlists(&mut workspace, "repo", Ok(vec![]));
    assert!(sidebar.active_playlist_id.is_none());
    assert_eq!(workspace.panel, WorkspacePanel::Files);
    sidebar.toggle_playlists();
    assert!(sidebar.playlists_expanded);
}

#[test]
fn switcher_locks_while_submitting_and_blank_attach_is_ignored() {
    let mut sidebar = SidebarState::default();
    sidebar.open_switcher();
    sidebar.open_switcher();
    assert_eq!(sidebar.popover, PopoverMode::Switcher);
    assert!(sidebar.select_from_switcher("repo"));
    assert_eq!(sidebar.popover, PopoverMode::Closed);
    sidebar.open_switcher();
    assert!(!sidebar.delete_from_switcher(None));
    assert!(sidebar.delete_from_switcher(Some("repo")));
    assert_eq!(sidebar.popover, PopoverMode::Closed);
    sidebar.open_switcher();
    sidebar.show_add_menu();
    sidebar.attach_path = "   ".into();
    assert!(!sidebar.submit_attach());
    sidebar.attach_path = "D:/library".into();
    assert!(sidebar.submit_attach());
    assert!(sidebar.submitting);
    assert!(!sidebar.close_popover());
    assert!(!sidebar.select_from_switcher("other"));
    assert!(!sidebar.show_add_menu());
    sidebar.note_attach_finished(Err("路径无效".into()));
    assert!(!sidebar.submitting);
    assert_eq!(sidebar.popover, PopoverMode::AddMenu);
    assert_eq!(sidebar.popover_error, "路径无效");
    sidebar.note_attach_finished(Ok(()));
    assert_eq!(sidebar.popover, PopoverMode::Closed);
}

#[test]
fn binding_same_repository_does_not_request_the_tree_again() {
    let mut sidebar = SidebarState::default();
    sidebar.bind_repository(Some("repo"), false);
    let first = sidebar.effects.len();
    sidebar.apply_snapshot(&[], 0, Vec::new());
    sidebar.bind_repository(Some("repo"), false);
    assert_eq!(sidebar.effects.len(), first);
    sidebar.bind_repository(Some("repo"), true);
    assert!(sidebar.effects.iter().skip(first).all(|effect| !matches!(
        effect,
        SidebarEffect::LoadTree { .. }
    )));
    assert!(sidebar.folders.is_empty());
    assert!(sidebar.quick_access.is_empty());
}

#[test]
fn smart_folder_create_requires_a_name_and_replaces_the_tree() {
    let mut model = super::super::ShellViewModel::default();
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::OpenSmartFolderDialog));
    assert!(!model.sidebar.smart_draft.open);
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SubmitSmartFolder));
    assert_eq!(model.sidebar.smart_draft.error, "先选择一个资源库。");
    assert!(model.sidebar.effects.is_empty());

    model.workspace.active_repo_id = Some("repo".into());
    model.sidebar.bind_repository(Some("repo"), false);
    let _ = model.sidebar.take_effects();
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::OpenSmartFolderDialog));
    assert!(model.sidebar.smart_draft.open);
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SubmitSmartFolder));
    assert_eq!(model.sidebar.smart_draft.error, "名称不能为空");
    assert!(model.sidebar.effects.is_empty());

    set_field(&mut model, SmartFolderField::Name, " 高评分 ");
    set_field(&mut model, SmartFolderField::Parent, "parent");
    set_field(&mut model, SmartFolderField::Formats, "psd，png");
    set_field(&mut model, SmartFolderField::Match, "or");
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SubmitSmartFolder));
    assert!(model.sidebar.smart_draft.busy);
    assert!(model.sidebar.effects.iter().any(|effect| matches!(
        effect,
        SidebarEffect::CreateSmartFolder { repo_id } if repo_id == "repo"
    )));
    let request = model.sidebar.smart_create_request("repo");
    assert_eq!(request.name, "高评分");
    assert_eq!(request.parent_id.as_deref(), Some("parent"));
    assert_eq!(request.smart_folder_id, None);
    assert_eq!(request.filter.formats.unwrap(), ["psd".to_string(), "png".to_string()]);
    assert_eq!(request.filter.match_mode.as_deref(), Some("or"));

    set_field(&mut model, SmartFolderField::Name, "忽略");
    assert_eq!(model.sidebar.smart_draft.name, " 高评分 ");

    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SmartFolderSaved {
        repo_id: "other".into(),
        result: Ok(Vec::new()),
    }));
    assert_eq!(model.sidebar.smart_draft.error, "智能文件夹结果已过期");
    assert!(model.sidebar.smart_draft.open);
    assert!(model.sidebar.smart_folders.is_empty());

    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SubmitSmartFolder));
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SmartFolderSaved {
        repo_id: "repo".into(),
        result: Err("写入失败".into()),
    }));
    assert!(model.sidebar.smart_draft.open);
    assert!(!model.sidebar.smart_draft.busy);
    assert_eq!(model.sidebar.smart_draft.error, "写入失败");

    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SubmitSmartFolder));
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SmartFolderSaved {
        repo_id: "repo".into(),
        result: Ok(vec![SidebarSmartFolder {
            id: "parent".into(),
            parent_id: None,
            name: "父级".into(),
            filter: Default::default(),
            children: vec![SidebarSmartFolder {
                id: "child".into(),
                parent_id: Some("parent".into()),
                name: "高评分".into(),
                filter: Default::default(),
                children: Vec::new(),
            }],
        }]),
    }));
    assert!(!model.sidebar.smart_draft.open);
    assert_eq!(model.sidebar.smart_folders[0].children[0].name, "高评分");
    assert!(model.sidebar.expanded_smart_folders.iter().any(|id| id == "parent"));

    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::OpenSmartFolderDialog));
    model.workspace.active_repo_id = Some("next".into());
    model.sidebar.bind_repository(Some("next"), false);
    assert!(!model.sidebar.smart_draft.open);
}

#[test]
fn clear_recent_only_queues_from_the_recent_view() {
    let mut model = super::super::ShellViewModel::default();
    model.sidebar.counts.recent = 3;
    model.workspace.active_repo_id = Some("repo".into());
    model.workspace.panel = super::super::WorkspacePanel::Files;
    model.workspace.library_category = super::super::workspace::LibraryCategory::All;
    model.clear_recent_access();
    assert!(model.sidebar.take_effects().is_empty());
    model.workspace.library_category = super::super::workspace::LibraryCategory::Recent;
    model.clear_recent_access();
    assert!(matches!(
        model.sidebar.take_effects().as_slice(),
        [SidebarEffect::ClearRecent { repo_id }] if repo_id == "repo"
    ));
}

fn set_field(model: &mut super::super::ShellViewModel, field: SmartFolderField, value: &str) {
    model.reduce(super::super::ShellMessage::Sidebar(SidebarMessage::SetSmartFolderField {
        field,
        value: value.into(),
    }));
}
