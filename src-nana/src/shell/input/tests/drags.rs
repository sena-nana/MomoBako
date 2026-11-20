//! 拖放几何过滤、内部拖动移动与委托、被拒拖动，以及 HTML 与宿主投放的导入、移动、挂载测试。

use super::*;

#[test]
fn drag_geometry_filters_paths_and_self_drops() {
    assert_eq!(normalize_workspace_path(" /a\\b/ "), "a/b");
    assert_eq!(workspace_parent_path("a/b/c"), "a/b");
    assert_eq!(workspace_parent_path("a"), "");
    assert_eq!(internal_drag_distance(0.0, 0.0, 3.0, 4.0), 5.0);
    assert!(!should_delegate_to_external_drag(
        0.0,
        0.0,
        80.0,
        0.0,
        10.0,
        10.0,
        100.0,
        100.0,
        EXTERNAL_DRAG_SWITCH_DISTANCE
    ));
    assert!(!should_delegate_to_external_drag(
        0.0,
        0.0,
        10.0,
        0.0,
        0.0,
        10.0,
        100.0,
        100.0,
        EXTERNAL_DRAG_SWITCH_DISTANCE
    ));
    assert!(should_delegate_to_external_drag(
        0.0,
        0.0,
        72.0,
        0.0,
        100.0,
        10.0,
        100.0,
        100.0,
        EXTERNAL_DRAG_SWITCH_DISTANCE
    ));
    assert_eq!(
        resolve_drop_target(Some("albums"), false, "photos"),
        Some("albums".into())
    );
    assert_eq!(
        resolve_drop_target(None, true, "photos"),
        Some("photos".into())
    );
    assert_eq!(resolve_drop_target(None, false, "photos"), None);
    assert_eq!(
        normalize_move_paths(
            &[
                " ".into(),
                "a".into(),
                "b/c".into(),
                "b".into(),
                "b/c/d".into()
            ],
            "b"
        ),
        vec!["a".to_string(), "b/c/d".to_string()]
    );
    assert_eq!(
        join_repository_path("C:\\repo", "photos", Some("a.png")),
        "C:\\repo\\photos\\a.png"
    );
    assert_eq!(
        normalize_filesystem_path("C:/Repo/a.png"),
        "c:\\repo\\a.png"
    );
    assert_eq!(
        filter_external_import_paths(
            &[
                "C:\\repo\\a.png".into(),
                "D:\\other\\b.png".into(),
                " ".into()
            ],
            "C:\\repo",
            ""
        ),
        vec!["D:\\other\\b.png".to_string()]
    );
    assert!(absolute_drag_paths(&[" ".into()], "").is_empty());
    assert_eq!(
        absolute_drag_paths(&["a.png".into()], "C:\\repo"),
        vec!["C:\\repo\\a.png".to_string()]
    );
    assert!(!can_drag_entries(true, true, false, "filesystem"));
    assert!(!can_drag_entries(true, false, false, "webdav"));
    assert!(can_drag_entries(true, false, false, "filesystem"));
    assert_eq!(decide_close("quit", false), CloseDecision::CloseNow);
    assert_eq!(
        decide_close("quit", true),
        CloseDecision::Ask { dirty: true }
    );
    assert_eq!(
        decide_close("minimizeToTray", true),
        CloseDecision::HoldForTray
    );
    assert_eq!(
        decide_close("confirm", false),
        CloseDecision::Ask { dirty: false }
    );
    assert_eq!(
        decide_close("other", false),
        CloseDecision::Ask { dirty: false }
    );
}

#[test]
fn internal_drag_selects_moves_and_delegates_outside_the_window() {
    let mut model = ready("repo");
    send(&mut model, begin("solo.png", &["other.png"], 0.0));
    assert_eq!(model.files.selected_paths(), ["solo.png"]);
    assert_eq!(model.files.primary.as_deref(), Some("solo.png"));
    assert_eq!(model.input.hover_folder.as_deref(), Some("photos"));
    send(
        &mut model,
        InputMessage::EntryDragMove {
            x: 20.0,
            y: 10.0,
            bounds_width: 200.0,
            bounds_height: 200.0,
            hover_folder: Some("albums".into()),
            over_browser: false,
        },
    );
    assert_eq!(model.input.hover_folder.as_deref(), Some("albums"));
    assert!(model.input.external_drag_result.is_none());
    send(
        &mut model,
        InputMessage::EntryDragEnd {
            hover_folder: Some("albums".into()),
            over_browser: false,
            has_pointer: true,
        },
    );
    assert!(
        matches!(model.files.take_effects().pop(), Some(FilesEffect::Move { parent, sources, .. }) if parent == "albums" && sources == ["solo.png"])
    );
    assert!(!model.input.internal_active);

    let mut model = ready("repo");
    send(&mut model, begin("keep.png", &["keep.png", "b/c.png"], 0.0));
    assert_eq!(model.input.dragged_paths, ["keep.png", "b/c.png"]);
    model.workspace.panel = WorkspacePanel::Trash;
    send(
        &mut model,
        InputMessage::EntryDragEnd {
            hover_folder: Some("albums".into()),
            over_browser: false,
            has_pointer: true,
        },
    );
    assert!(model.files.take_effects().is_empty());

    let mut model = ready("repo");
    send(&mut model, begin("photos/a.png", &["photos/a.png"], 0.0));
    send(
        &mut model,
        InputMessage::EntryDragEnd {
            hover_folder: Some("photos".into()),
            over_browser: true,
            has_pointer: true,
        },
    );
    assert!(model.files.take_effects().is_empty());

    let mut model = ready("repo");
    send(&mut model, begin("a.png", &["a.png"], 0.0));
    send(
        &mut model,
        InputMessage::EntryDragMove {
            x: 100.0,
            y: 10.0,
            bounds_width: 100.0,
            bounds_height: 100.0,
            hover_folder: None,
            over_browser: false,
        },
    );
    assert_eq!(model.input.external_drag_result, None);
    assert!(model.status.failure().is_none());
    assert!(
        matches!(model.input.host_requests.last(), Some(HostRequest::Input(HostInputRequest::DragOut { paths })) if paths == &["C:\\repo\\a.png".to_string()])
    );
    assert!(model.input.session.is_none());
    send(
        &mut model,
        InputMessage::EntryDragEnd {
            hover_folder: Some("albums".into()),
            over_browser: true,
            has_pointer: true,
        },
    );
    assert!(model.files.take_effects().is_empty());
}

#[test]
fn refused_drags_pointer_leave_and_delegated_end_do_not_move() {
    let mut model = ready("repo");
    send(
        &mut model,
        InputMessage::BeginEntryDrag {
            path: "a.png".into(),
            selected: vec!["a.png".into()],
            x: 0.0,
            y: 0.0,
            writable: false,
            trash: false,
            smart_folder: false,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
            hover_folder: None,
            over_browser: true,
        },
    );
    assert!(model.input.session.is_none());
    send(
        &mut model,
        InputMessage::BeginEntryDrag {
            path: "a.png".into(),
            selected: vec!["a.png".into()],
            x: 0.0,
            y: 0.0,
            writable: true,
            trash: false,
            smart_folder: true,
            backend_kind: "filesystem".into(),
            repo_root: "C:\\repo".into(),
            hover_folder: None,
            over_browser: true,
        },
    );
    assert!(model.input.session.is_none());

    send(&mut model, begin("a.png", &["a.png"], 0.0));
    send(
        &mut model,
        InputMessage::WindowPointerLeave { x: 10.0, y: 0.0 },
    );
    assert!(model.input.session.is_some());
    send(
        &mut model,
        InputMessage::WindowPointerLeave { x: 72.0, y: 0.0 },
    );
    assert!(model.input.session.is_none());
    assert_eq!(model.input.external_drag_result, None);
    assert!(matches!(
        model.input.host_requests.last(),
        Some(HostRequest::Input(HostInputRequest::DragOut { .. }))
    ));

    let mut model = ready("repo");
    send(&mut model, begin("a.png", &["a.png"], 0.0));
    send(
        &mut model,
        InputMessage::EntryDragMove {
            x: 10.0,
            y: 0.0,
            bounds_width: 400.0,
            bounds_height: 400.0,
            hover_folder: None,
            over_browser: true,
        },
    );
    send(&mut model, InputMessage::WindowBlur);
    assert!(model.input.session.is_some());
    send(
        &mut model,
        InputMessage::EntryDragMove {
            x: 80.0,
            y: 0.0,
            bounds_width: 400.0,
            bounds_height: 400.0,
            hover_folder: None,
            over_browser: true,
        },
    );
    model.input.session = Some(InternalSession {
        start_x: 0.0,
        start_y: 0.0,
        last_x: 80.0,
        last_y: 0.0,
        paths: vec!["a.png".into()],
        delegated: true,
        repo_root: "C:\\repo".into(),
        backend_kind: "filesystem".into(),
    });
    model.input.internal_active = true;
    model.input.dragged_paths = vec!["a.png".into()];
    send(&mut model, InputMessage::WindowBlur);
    assert!(model.input.session.is_some());
    send(
        &mut model,
        InputMessage::EntryDragEnd {
            hover_folder: Some("albums".into()),
            over_browser: true,
            has_pointer: false,
        },
    );
    assert!(model.files.take_effects().is_empty());
    assert!(model.input.session.is_none());

    send(
        &mut model,
        InputMessage::EntryDragEnd {
            hover_folder: None,
            over_browser: false,
            has_pointer: true,
        },
    );
    assert!(model.files.take_effects().is_empty());
}

#[test]
fn html_and_host_drops_import_move_or_attach() {
    let mut model = ready("repo");
    model.input.drop_effect = "keep".into();
    send(
        &mut model,
        InputMessage::DragOver {
            internal_transfer: false,
            writable: false,
            files_panel: true,
        },
    );
    assert_eq!(model.input.drop_effect, "keep");
    send(
        &mut model,
        InputMessage::DragOver {
            internal_transfer: true,
            writable: true,
            files_panel: true,
        },
    );
    assert_eq!(model.input.drop_effect, "move");
    send(
        &mut model,
        InputMessage::DragOver {
            internal_transfer: false,
            writable: true,
            files_panel: true,
        },
    );
    assert_eq!(model.input.drop_effect, "copy");
    assert!(model.input.external_active && model.input.dragging_files);
    send(&mut model, InputMessage::DragLeave { nested: true });
    assert!(model.input.external_active);
    model.input.internal_active = true;
    send(&mut model, InputMessage::DragLeave { nested: false });
    assert!(model.input.external_active);
    model.input.internal_active = false;
    send(&mut model, InputMessage::DragLeave { nested: false });
    assert!(!model.input.external_active);
    assert!(model.input.hover_folder.is_none());

    send(&mut model, begin("a.png", &["a.png"], 0.0));
    model.input.hover_folder = None;
    send(
        &mut model,
        InputMessage::BrowserDrop {
            internal_transfer: true,
            paths: Vec::new(),
            writable: true,
            trash: false,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(
        matches!(model.files.take_effects().pop(), Some(FilesEffect::Move { parent, .. }) if parent == "photos")
    );

    send(
        &mut model,
        InputMessage::BrowserDrop {
            internal_transfer: false,
            paths: vec!["C:\\repo\\a.png".into(), "D:\\other\\b.png".into()],
            writable: false,
            trash: false,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::BrowserDrop {
            internal_transfer: false,
            paths: vec!["D:\\other\\b.png".into()],
            writable: true,
            trash: true,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::BrowserDrop {
            internal_transfer: false,
            paths: vec![" ".into()],
            writable: true,
            trash: false,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::BrowserDrop {
            internal_transfer: false,
            paths: vec!["C:\\repo\\photos\\a.png".into(), "D:\\other\\b.png".into()],
            writable: true,
            trash: false,
            has_snapshot: false,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    model.input.hover_folder = Some("albums".into());
    send(
        &mut model,
        InputMessage::BrowserDrop {
            internal_transfer: false,
            paths: vec!["C:\\repo\\albums\\a.png".into(), "D:\\other\\b.png".into()],
            writable: true,
            trash: false,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(matches!(
        model.files.take_effects().pop(),
        Some(FilesEffect::Import { parent, sources, .. }) if parent.as_deref() == Some("albums") && sources == ["D:\\other\\b.png"]
    ));

    model.workspace.panel = WorkspacePanel::Trash;
    send(
        &mut model,
        InputMessage::FolderHover {
            path: "albums".into(),
            trash: true,
        },
    );
    assert!(model.input.hover_folder.is_none());
    model.workspace.panel = WorkspacePanel::Files;
    send(
        &mut model,
        InputMessage::FolderHover {
            path: "albums".into(),
            trash: false,
        },
    );
    send(&mut model, InputMessage::FolderLeave("other".into()));
    assert_eq!(model.input.hover_folder.as_deref(), Some("albums"));
    send(&mut model, InputMessage::FolderLeave("albums".into()));
    assert!(model.input.hover_folder.is_none());
    send(
        &mut model,
        InputMessage::FolderDrop {
            path: "albums".into(),
            internal_transfer: false,
            paths: vec![" ".into()],
        },
    );
    assert!(model.files.take_effects().is_empty());
    send(&mut model, begin("a.png", &["a.png"], 0.0));
    model.workspace.panel = WorkspacePanel::Trash;
    send(
        &mut model,
        InputMessage::FolderDrop {
            path: "albums".into(),
            internal_transfer: true,
            paths: Vec::new(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    model.workspace.panel = WorkspacePanel::Files;
    send(
        &mut model,
        InputMessage::FolderDrop {
            path: "albums".into(),
            internal_transfer: false,
            paths: vec!["D:\\other\\a.png".into()],
        },
    );
    assert!(matches!(
        model.files.take_effects().pop(),
        Some(FilesEffect::Import { parent, sources, .. }) if parent.as_deref() == Some("albums") && sources == ["D:\\other\\a.png"]
    ));

    send(
        &mut model,
        InputMessage::EmptyDragOver {
            has_active_id: true,
            has_repository: false,
        },
    );
    assert!(!model.input.dragging_repository_folder);
    send(
        &mut model,
        InputMessage::EmptyDragOver {
            has_active_id: false,
            has_repository: false,
        },
    );
    assert!(model.input.dragging_repository_folder);
    assert_eq!(model.input.drop_effect, "copy");
    send(&mut model, InputMessage::EmptyDragLeave { nested: true });
    assert!(model.input.dragging_repository_folder);
    send(&mut model, InputMessage::EmptyDragLeave { nested: false });
    assert!(!model.input.dragging_repository_folder);
    send(
        &mut model,
        InputMessage::EmptyDrop {
            has_active_id: true,
            has_repository: false,
            paths: vec!["C:\\new".into()],
        },
    );
    assert!(model.sidebar.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::EmptyDrop {
            has_active_id: false,
            has_repository: false,
            paths: vec![" ".into()],
        },
    );
    assert!(model.sidebar.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::EmptyDrop {
            has_active_id: false,
            has_repository: false,
            paths: vec![" C:\\new ".into(), "C:\\other".into()],
        },
    );
    assert!(
        matches!(model.sidebar.take_effects().pop(), Some(SidebarEffect::AttachRepository { path }) if path == "C:\\new")
    );
    assert!(model.input.pending_attach);
    model.reduce(ShellMessage::Sidebar(
        crate::shell::SidebarMessage::RepositoryAttachFinished(Err("附加失败".into())),
    ));
    assert_eq!(model.input.empty_repository_error, "附加失败");
    assert!(!model.input.pending_attach);
    model.reduce(ShellMessage::Sidebar(
        crate::shell::SidebarMessage::RepositoryAttachFinished(Err("忽略".into())),
    ));
    assert_eq!(model.input.empty_repository_error, "附加失败");

    model.input.dragging_files = true;
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Enter,
            paths: Vec::new(),
            has_repository: false,
            missing_repository: false,
            writable: false,
            files_panel: false,
            has_snapshot: false,
            repo_root: String::new(),
        },
    );
    assert!(model.input.dragging_repository_folder);
    assert!(model.input.dragging_files);
    model.input.dragging_files = false;
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Leave,
            paths: Vec::new(),
            has_repository: false,
            missing_repository: false,
            writable: false,
            files_panel: false,
            has_snapshot: false,
            repo_root: String::new(),
        },
    );
    assert!(!model.input.dragging_repository_folder);
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Drop,
            paths: vec!["C:\\library".into()],
            has_repository: false,
            missing_repository: true,
            writable: false,
            files_panel: true,
            has_snapshot: false,
            repo_root: String::new(),
        },
    );
    assert!(model.sidebar.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Over,
            paths: Vec::new(),
            has_repository: true,
            missing_repository: false,
            writable: false,
            files_panel: true,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(!model.input.dragging_files);
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Enter,
            paths: Vec::new(),
            has_repository: true,
            missing_repository: false,
            writable: true,
            files_panel: true,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.input.dragging_files);
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Leave,
            paths: Vec::new(),
            has_repository: true,
            missing_repository: false,
            writable: true,
            files_panel: true,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(!model.input.external_active);
    model.input.hover_folder = Some("photos".into());
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Drop,
            paths: vec!["D:\\other\\c.png".into()],
            has_repository: true,
            missing_repository: false,
            writable: true,
            files_panel: false,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    assert_eq!(model.input.hover_folder.as_deref(), Some("photos"));
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Drop,
            paths: Vec::new(),
            has_repository: true,
            missing_repository: false,
            writable: true,
            files_panel: true,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::HostDrag {
            phase: HostDragPhase::Drop,
            paths: vec!["D:\\other\\c.png".into()],
            has_repository: true,
            missing_repository: false,
            writable: true,
            files_panel: true,
            has_snapshot: true,
            repo_root: "C:\\repo".into(),
        },
    );
    assert!(
        matches!(model.files.take_effects().pop(), Some(FilesEffect::Import { sources, .. }) if sources == ["D:\\other\\c.png"])
    );
}
