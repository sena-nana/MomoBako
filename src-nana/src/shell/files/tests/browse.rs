//! 分页追加、加载失败、虚拟视图、变更校验、选择范围、显示模式与可写判断的测试。

use super::*;

#[test]
fn append_merges_unique_keys_and_has_more_follows_total() {
    let mut state = FilesState::default();
    let ctx = writable();
    assert!(state.request_browse(&ctx, ""));
    assert!(state.apply_browser(
        &snapshot(
            "",
            4,
            vec![entry("a", "a", "file"), entry("b", "b", "file")]
        ),
        false
    ));
    assert!(state.has_more);
    assert!(state.load_more(&ctx));
    assert!(state.apply_browser(
        &snapshot(
            "",
            4,
            vec![entry("a", "a", "file"), entry("c", "c", "file")]
        ),
        false
    ));
    assert_eq!(
        state.entry_names(),
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    assert!(state.has_more);
}

#[test]
fn load_more_is_ignored_without_more_or_while_loading() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("a", "a", "file")];
    state.has_more = false;
    assert!(!state.load_more(&ctx));
    state.has_more = true;
    state.loading_more = true;
    assert!(!state.load_more(&ctx));
    assert!(state.effects.is_empty());
}

#[test]
fn stale_browse_keeps_entries() {
    let mut state = FilesState::default();
    state.rows = vec![row("keep", "keep", "file")];
    state.pending = Some(super::super::BrowsePending {
        repo_id: "repo".into(),
        path: "photos".into(),
        trash: false,
        append: false,
        silent: false,
        select_after: None,
    });
    state.loading = true;
    assert!(!state.apply_browser(
        &snapshot("other", 1, vec![entry("new", "new", "file")]),
        false
    ));
    assert_eq!(state.entry_names(), vec!["keep".to_string()]);
    assert!(!state.loading);
    assert!(state.pending.is_none());
}

#[test]
fn load_error_keeps_entries() {
    let mut state = FilesState::default();
    state.rows = vec![row("keep", "keep", "file")];
    state.loading = true;
    assert!(state.note_load_failed("读取失败"));
    assert_eq!(state.entry_names(), vec!["keep".to_string()]);
    assert_eq!(state.error, "读取失败");
    assert!(!state.loading);
    assert!(!state.note_load_failed("启动失败"));
}

#[test]
fn virtual_views_reject_mutations_and_choose_their_rows() {
    let mut state = FilesState::default();
    state.rows = vec![row("dir", "dir", "directory"), row("file", "file", "file")];
    state.virtual_rows = vec![row("smart", "smart", "file")];
    state.selected = vec!["file".into()];
    state.primary = Some("file".into());
    let smart = smart_ctx();
    let category = category_ctx();
    assert!(!create_file_for(&mut state, &smart, "note.txt"));
    assert!(!reduce_open(&mut state, &smart, FileDialog::Import));
    assert!(!state.can_rename(&smart));
    assert!(!delete_selected_for(&mut state, &smart));
    assert!(!create_file_for(&mut state, &category, "note.txt"));
    assert!(!state.can_import(&category));
    assert!(state.can_rename(&category));
    assert!(state.can_delete(&category));
    state.category_rows = vec![row("picked", "picked", "file")];
    assert_eq!(
        state
            .visible_rows(&category)
            .iter()
            .map(|row| row.name.clone())
            .collect::<Vec<_>>(),
        vec!["picked".to_string()]
    );
    assert_eq!(
        state
            .visible_rows(&smart)
            .iter()
            .map(|row| row.name.clone())
            .collect::<Vec<_>>(),
        vec!["smart".to_string()]
    );
    assert!(!state.mutating);
    state.rows[1].is_virtual = true;
    assert!(!state.can_delete(&writable()));
}

#[test]
fn trash_delete_is_permanent_and_trash_rejects_create_and_copy() {
    let mut state = FilesState::default();
    state.rows = vec![row("gone", "gone", "file")];
    state.selected = vec!["gone".into()];
    state.primary = Some("gone".into());
    let trash = trash_ctx();
    assert!(!create_file_for(&mut state, &trash, "note.txt"));
    assert!(!reduce_open(&mut state, &trash, FileDialog::Copy));
    assert!(delete_selected_for(&mut state, &trash));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Delete { mode: Some(mode), .. }] if mode == "permanentDelete"
    ));
    state.mutating = false;
    assert!(delete_selected_for(&mut state, &writable()));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Delete { mode: None, .. }]
    ));
}

#[test]
fn blank_name_and_blank_import_do_not_mutate() {
    let mut state = FilesState::default();
    let ctx = writable();
    assert!(!create_file_for(&mut state, &ctx, "  "));
    assert!(state.effects.is_empty());
    assert!(reduce_open(&mut state, &ctx, FileDialog::Import));
    state.reduce(&ctx, FilesMessage::DraftChanged(" \n; ".into()));
    assert!(!submit_for(&mut state, &ctx));
    assert!(!state.mutating);
}

/// 工具栏建文件失败时输入框里的名字留着；对话框里的变更失败时对话框和草稿都留着。
#[test]
fn mutation_failure_keeps_names_and_dialog() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("keep", "keep", "file")];
    assert!(create_file_for(&mut state, &ctx, " draft.txt "));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::CreateFile { name, parent: None, .. }] if name == "draft.txt"
    ));
    state.reduce(
        &ctx,
        FilesMessage::MutationSnapshot {
            result: Err("写入失败".into()),
            created_name: Some("draft.txt".into()),
        },
    );
    assert_eq!(state.entry_names(), vec!["keep".to_string()]);
    assert!(!state.mutating);
    assert_eq!(state.create_name, " draft.txt ");
    assert_eq!(state.error, "写入失败");

    state.dialog = FileDialog::Rename;
    state.name_draft = "draft".into();
    state.mutating = true;
    state.reduce(
        &ctx,
        FilesMessage::MutationSnapshot {
            result: Err("改名失败".into()),
            created_name: None,
        },
    );
    assert_eq!(state.dialog, FileDialog::Rename);
    assert_eq!(state.name_draft, "draft");
    assert_eq!(state.error, "改名失败");
}

#[test]
fn mutation_success_uses_snapshot_names_and_category_mismatch_keeps_the_list() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("old", "old", "file")];
    state.selected = vec!["old".into()];
    state.primary = Some("old".into());
    assert!(create_file_for(&mut state, &ctx, "draft.txt"));
    let snap = snapshot(
        "photos",
        1,
        vec![entry("photos/server.txt", "server.txt", "file")],
    );
    state.reduce(
        &ctx,
        FilesMessage::MutationSnapshot {
            result: Ok(snap),
            created_name: Some("draft.txt".into()),
        },
    );
    assert_eq!(state.entry_names(), vec!["server.txt".to_string()]);
    assert!(state.selected.is_empty());
    assert!(state.create_name.is_empty(), "建成以后清空输入框");
    assert!(!state.mutating);

    let mut category = FilesState::default();
    category.rows = vec![row("keep", "keep", "file")];
    category.current_path = "photos".into();
    category.dialog = FileDialog::Rename;
    category.name_draft = "draft".into();
    category.mutating = true;
    let mismatch = snapshot("other", 1, vec![entry("other/new", "new", "file")]);
    category.reduce(
        &category_ctx(),
        FilesMessage::MutationSnapshot {
            result: Ok(mismatch),
            created_name: Some("draft".into()),
        },
    );
    assert_eq!(category.entry_names(), vec!["keep".to_string()]);
    assert_eq!(category.dialog, FileDialog::Closed);
    assert!(category.name_draft.is_empty());
}

#[test]
fn selection_replace_toggle_range_and_snapshot_prune() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![
        row("a", "a", "file"),
        row("b", "b", "file"),
        row("c", "c", "file"),
    ];
    state.select_visible(&ctx, "b", SelectionMode::Replace);
    assert_eq!(state.selected, vec!["b".to_string()]);
    state.select_visible(&ctx, "a", SelectionMode::Toggle);
    assert_eq!(state.selected, vec!["b".to_string(), "a".to_string()]);
    state.anchor = None;
    state.select_visible(&ctx, "c", SelectionMode::Range);
    assert_eq!(state.selected, vec!["c".to_string()]);
    state.anchor = Some("missing".into());
    state.select_visible(&ctx, "a", SelectionMode::Range);
    assert_eq!(state.selected, vec!["a".to_string()]);
    state.select_visible(&ctx, "a", SelectionMode::Replace);
    state.select_visible(&ctx, "c", SelectionMode::Range);
    assert_eq!(
        state.selected,
        vec!["a".to_string(), "b".to_string(), "c".to_string()]
    );
    state.primary = Some("b".into());
    state.anchor = Some("c".into());
    assert!(state.apply_browser(&snapshot("", 1, vec![entry("a", "a", "file")]), false));
    assert_eq!(state.selected, vec!["a".to_string()]);
    assert_eq!(state.primary.as_deref(), Some("a"));
    assert_eq!(state.anchor.as_deref(), Some("a"));
}

/// 快照把文件和文件夹穿插着给。画面上先是文件夹一组、再是文件一组。
/// Shift 按这组顺序拉，不按快照里的下标把夹在中间的文件夹算进文件之间。
#[test]
fn shift_range_follows_the_folders_then_files_on_screen() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![
        row("a.mp3", "a.mp3", "file"),
        row("albums", "albums", "directory"),
        row("b.mp3", "b.mp3", "file"),
        row("covers", "covers", "directory"),
    ];
    state.select_visible(&ctx, "albums", SelectionMode::Replace);
    state.select_visible(&ctx, "b.mp3", SelectionMode::Range);
    assert_eq!(
        state.selected,
        vec![
            "albums".to_string(),
            "covers".to_string(),
            "a.mp3".to_string(),
            "b.mp3".to_string()
        ],
        "从第一个文件夹拉到后面的文件，中间的文件夹和文件都在"
    );
    state.select_visible(&ctx, "a.mp3", SelectionMode::Replace);
    state.select_visible(&ctx, "covers", SelectionMode::Range);
    assert_eq!(
        state.selected,
        vec!["covers".to_string(), "a.mp3".to_string()],
        "文件往上拉到它前面的文件夹时，不带上更前面的那个文件夹"
    );

    let mut category = FilesState::default();
    category.category_rows = vec![
        row("a.mp3", "a.mp3", "file"),
        row("albums", "albums", "directory"),
        row("b.mp3", "b.mp3", "file"),
    ];
    category.select_visible(&category_ctx(), "a.mp3", SelectionMode::Replace);
    category.select_visible(&category_ctx(), "b.mp3", SelectionMode::Range);
    assert_eq!(
        category.selected,
        vec![
            "a.mp3".to_string(),
            "albums".to_string(),
            "b.mp3".to_string()
        ],
        "分类视图不拆组，仍按列表本身的顺序"
    );
}

/// Ctrl 点选切换选择，右侧详情跟着主选中项：拿掉别的项时主选中项和
/// 详情都不动；拿掉主选中项时由剩下的第一项接替，详情跟过去并读它的素材；全拿掉时不跟。
#[test]
fn toggling_rows_keeps_or_hands_over_the_primary() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = ["a.png", "b.png", "c.png"]
        .into_iter()
        .map(|path| {
            let mut file = entry(path, path, "file");
            file.asset_id = Some(format!("asset-{path}"));
            FileRow::from_entry(&file)
        })
        .collect();
    let loads = |state: &mut FilesState| {
        state
            .take_effects()
            .into_iter()
            .filter_map(|effect| match effect {
                FilesEffect::LoadAsset { asset_id, .. } => Some(asset_id),
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    assert_eq!(
        state
            .activate_row(&ctx, "a.png", SelectionMode::Replace)
            .as_deref(),
        Some("a.png")
    );
    assert_eq!(loads(&mut state), ["asset-a.png"]);
    assert_eq!(
        state
            .activate_row(&ctx, "b.png", SelectionMode::Toggle)
            .as_deref(),
        Some("b.png")
    );
    assert_eq!(loads(&mut state), ["asset-b.png"]);

    assert_eq!(
        state.activate_row(&ctx, "a.png", SelectionMode::Toggle),
        None,
        "拿掉的不是主选中项，详情不动"
    );
    assert_eq!(
        (state.selected.as_slice(), state.primary.as_deref()),
        (&["b.png".to_string()][..], Some("b.png"))
    );
    assert!(loads(&mut state).is_empty());

    state.activate_row(&ctx, "c.png", SelectionMode::Toggle);
    loads(&mut state);
    assert_eq!(
        state
            .activate_row(&ctx, "c.png", SelectionMode::Toggle)
            .as_deref(),
        Some("b.png"),
        "主选中项被拿掉，由剩下的第一项接替"
    );
    assert_eq!(
        (state.primary.as_deref(), state.anchor.as_deref()),
        (Some("b.png"), Some("b.png"))
    );
    assert_eq!(loads(&mut state), ["asset-b.png"]);
    assert_eq!(
        state.select_only.as_deref(),
        Some("b.png"),
        "接替的文件只在右侧详情里看"
    );

    assert_eq!(
        state.activate_row(&ctx, "b.png", SelectionMode::Toggle),
        None
    );
    assert!(state.selected.is_empty());
    assert_eq!(
        (state.primary.as_deref(), state.anchor.as_deref()),
        (None, None)
    );
    assert!(loads(&mut state).is_empty());
}

#[test]
fn unknown_display_mode_falls_back_to_adaptive() {
    assert_eq!(DisplayMode::parse("nope"), DisplayMode::Adaptive);
    assert_eq!(DisplayMode::parse("list"), DisplayMode::List);
    assert_eq!(DisplayMode::parse("瀑布流"), DisplayMode::Masonry);
    let dir = std::env::temp_dir().join(format!("momobako-display-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file-display.json");
    std::fs::write(&path, "{\"mode\":\"nope\"}").unwrap();
    let mut state = FilesState::default();
    state.load_display_mode_file(&path);
    assert_eq!(state.display_mode, DisplayMode::Adaptive);
    state.load_display_mode_file(&dir.join("missing.json"));
    assert_eq!(state.display_mode, DisplayMode::Adaptive);
    state.display_mode = DisplayMode::Grid;
    state.save_display_mode_file(&path);
    state.display_mode = DisplayMode::List;
    state.load_display_mode_file(&path);
    assert_eq!(state.display_mode, DisplayMode::Grid);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn multiple_selection_clears_the_rename_draft() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("a", "a.txt", "file"), row("b", "b.txt", "file")];
    state.select_visible(&ctx, "a", SelectionMode::Replace);
    assert!(reduce_open(&mut state, &ctx, FileDialog::Rename));
    assert_eq!(state.name_draft, "a.txt");
    state.select_visible(&ctx, "b", SelectionMode::Toggle);
    assert!(state.selected.len() > 1);
    assert!(state.rename_path.is_none());
    assert!(state.name_draft.is_empty());
    assert_eq!(state.dialog, FileDialog::Closed);

    // 主选从 a 换成 b，选择仍是一项：重命名草稿清掉，重命名框关闭。
    state.select_visible(&ctx, "a", SelectionMode::Replace);
    assert!(reduce_open(&mut state, &ctx, FileDialog::Rename));
    assert_eq!(state.name_draft, "a.txt");
    assert_eq!(state.rename_path.as_deref(), Some("a"));
    state.select_visible(&ctx, "b", SelectionMode::Replace);
    assert_eq!(state.selected, vec!["b".to_string()]);
    assert_eq!(state.primary.as_deref(), Some("b"));
    assert!(state.rename_path.is_none());
    assert!(state.name_draft.is_empty());
    assert_eq!(state.dialog, FileDialog::Closed);
}

#[test]
fn writable_requires_write_capability_and_present_repository() {
    assert!(!super::super::repository_is_writable(
        "missing",
        &["write".into()]
    ));
    assert!(!super::super::repository_is_writable(
        "ready",
        &["read".into()]
    ));
    assert!(super::super::repository_is_writable(
        "ready",
        &["write".into()]
    ));
    let mut locked = writable();
    locked.writable = false;
    locked.missing = true;
    let mut state = FilesState::default();
    state.rows = vec![row("a", "a", "file")];
    state.selected = vec!["a".into()];
    state.primary = Some("a".into());
    assert!(!state.can_create(&locked));
    assert!(!state.can_rename(&locked));
    assert!(!state.can_delete(&locked));
}
