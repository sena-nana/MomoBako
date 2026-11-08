//! 文件浏览状态机测试。只调用状态方法，不写用户的展示方式文件。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
};

use super::super::ShellViewModel;
use super::{
    DisplayMode, FileContext, FileDialog, FileRow, FilesEffect, FilesMessage, FilesState,
    HardlinkPrompt, SelectionMode,
};

fn entry(path: &str, name: &str, kind: &str) -> FileBrowserEntry {
    FileBrowserEntry {
        path: path.into(),
        name: name.into(),
        kind: kind.into(),
        extension: None,
        size_bytes: None,
        size_label: None,
        modified_at: None,
        asset_id: None,
        status: None,
        thumbnail_path: None,
        thumbnail_custom: false,
        hardlink_group_id: None,
        hardlink_state: None,
        tags: Vec::new(),
        alias_paths: Vec::new(),
        folder_metadata: None,
        metadata: BTreeMap::new(),
        is_virtual: false,
        provider_id: None,
        provider_item_id: None,
        source_payload: None,
        local_absolute_path: None,
    }
}

fn row(path: &str, name: &str, kind: &str) -> FileRow {
    FileRow::from_entry(&entry(path, name, kind))
}

fn snapshot(path: &str, total: usize, entries: Vec<FileBrowserEntry>) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: "repo".into(),
        root_path: "C:/repo".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: path.into(),
        total_entries: total,
        loaded_count: entries.len(),
        next_offset: None,
        has_more: entries.len() < total,
        special_location: None,
        tree: None,
        entries,
    }
}

fn writable() -> FileContext {
    FileContext {
        repo_id: Some("repo".into()),
        writable: true,
        missing: false,
        trash: false,
        smart_folder: false,
        category_virtual: false,
    }
}

fn trash_ctx() -> FileContext {
    FileContext {
        trash: true,
        ..writable()
    }
}

fn smart_ctx() -> FileContext {
    FileContext {
        smart_folder: true,
        ..writable()
    }
}

fn category_ctx() -> FileContext {
    FileContext {
        category_virtual: true,
        ..writable()
    }
}

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
    state.pending = Some(super::BrowsePending {
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

/// Ctrl 点选照 Vue `selectWorkspaceEntry` 的 `toggle`，右侧详情跟着主选中项：拿掉别的项时主选中项和
/// 详情都不动；拿掉主选中项时由剩下的第一项接替，详情跟过去并读它的素材；全拿掉时不跟。
#[test]
fn toggling_rows_keeps_or_hands_over_the_primary_like_vue() {
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
fn hardlink_skip_hides_candidate_and_confirm_failure_keeps_it() {
    let mut state = FilesState::default();
    let ctx = writable();
    let first = HardlinkPrompt {
        id: "one".into(),
        new_path: "n".into(),
        existing_path: "e".into(),
        size_label: "1 KB".into(),
    };
    let second = HardlinkPrompt {
        id: "two".into(),
        new_path: "n2".into(),
        existing_path: "e2".into(),
        size_label: "2 KB".into(),
    };
    state.reduce(
        &ctx,
        FilesMessage::HardlinksLoaded {
            epoch: 0,
            result: Ok(vec![first.clone(), second.clone()]),
        },
    );
    assert_eq!(state.dialog, FileDialog::Hardlink);
    assert_eq!(
        state.current_hardlink().map(|item| item.id.as_str()),
        Some("one")
    );
    state.reduce(&ctx, FilesMessage::SkipHardlink);
    assert_eq!(
        state.current_hardlink().map(|item| item.id.as_str()),
        Some("two")
    );
    state.mutating = true;
    state.reduce(
        &ctx,
        FilesMessage::HardlinkConfirmed {
            repo_id: "repo".into(),
            result: Err("确认失败".into()),
        },
    );
    assert_eq!(
        state.current_hardlink().map(|item| item.id.as_str()),
        Some("two")
    );
    assert_eq!(state.error, "确认失败");
    assert!(!state.mutating);
    state.reduce(
        &ctx,
        FilesMessage::HardlinkConfirmed {
            repo_id: "repo".into(),
            result: Ok("two".into()),
        },
    );
    assert!(state.current_hardlink().is_none());
    assert_eq!(state.dialog, FileDialog::Closed);
    assert!(first.message().contains("确认后会将新文件加入硬链接关联"));
}

#[test]
fn silent_hardlink_refresh_keeps_dialog_skipped_and_error() {
    let mut state = FilesState::default();
    let ctx = writable();
    let kept = HardlinkPrompt {
        id: "kept".into(),
        new_path: "old-new".into(),
        existing_path: "old-existing".into(),
        size_label: "1 KB".into(),
    };
    let fresh = HardlinkPrompt {
        id: "fresh".into(),
        new_path: "new".into(),
        existing_path: "existing".into(),
        size_label: "2 KB".into(),
    };
    state.hardlinks = vec![kept.clone()];
    state.skipped_hardlinks = vec!["kept".into()];
    state.error = "旧错误".into();
    state.loading = true;

    state.reduce(
        &ctx,
        FilesMessage::HardlinksRefreshed {
            epoch: 0,
            result: Ok(vec![fresh.clone()]),
        },
    );
    assert_eq!(state.hardlinks, vec![fresh.clone()]);
    assert_eq!(state.dialog, FileDialog::Closed);
    assert_eq!(state.skipped_hardlinks, vec!["kept".to_string()]);
    assert_eq!(state.error, "旧错误");
    assert!(state.loading);

    state.dialog = FileDialog::Rename;
    state.reduce(
        &ctx,
        FilesMessage::HardlinksRefreshed {
            epoch: 0,
            result: Err("刷新失败".into()),
        },
    );
    assert_eq!(state.hardlinks, vec![fresh]);
    assert_eq!(state.dialog, FileDialog::Rename);
    assert_eq!(state.skipped_hardlinks, vec!["kept".to_string()]);
    assert_eq!(state.error, "旧错误");
    assert!(state.loading);

    state.reduce(
        &ctx,
        FilesMessage::HardlinksLoaded {
            epoch: 0,
            result: Ok(vec![kept.clone()]),
        },
    );
    assert_eq!(state.dialog, FileDialog::Hardlink);
    assert_eq!(state.hardlinks, vec![kept]);
    assert!(state.skipped_hardlinks.is_empty());

    state.dialog = FileDialog::Copy;
    state.loading = false;
    state.refresh_hardlinks_silent("");
    assert!(state.effects.is_empty());
    assert_eq!(state.dialog, FileDialog::Copy);
    assert!(!state.loading);
    assert_eq!(state.error, "旧错误");
    state.refresh_hardlinks_silent("repo");
    assert!(matches!(
        state.effects.as_slice(),
        [FilesEffect::RefreshHardlinks { repo_id, .. }] if repo_id == "repo"
    ));
    assert_eq!(state.dialog, FileDialog::Copy);
    assert!(!state.loading);
    assert_eq!(state.error, "旧错误");
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
    assert!(!super::repository_is_writable("missing", &["write".into()]));
    assert!(!super::repository_is_writable("ready", &["read".into()]));
    assert!(super::repository_is_writable("ready", &["write".into()]));
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

#[test]
fn protocol_success_reloads_without_rewriting_rows() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("keep", "keep", "file")];
    state.selected = vec!["keep".into()];
    state.primary = Some("keep".into());
    state.current_path = "photos".into();
    state.dialog = FileDialog::Copy;
    state.mutating = true;
    state.reduce(
        &ctx,
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: true,
            hardlinks: true,
            folder_tree: None,
        },
    );
    assert_eq!(state.entry_names(), vec!["keep".to_string()]);
    assert_eq!(state.selected, vec!["keep".to_string()]);
    assert_eq!(state.dialog, FileDialog::Closed);
    assert!(!state.mutating);
    assert!(state.loading);
    assert!(matches!(
        state.take_effects().as_slice(),
        [
            FilesEffect::Browse { path, append: false, limit, .. },
            FilesEffect::LoadHardlinks { repo_id, .. },
        ] if path == "photos" && *limit == super::browse::INITIAL_PAGE_SIZE && repo_id == "repo"
    ));
}

#[test]
fn protocol_failure_keeps_rows_and_dialog() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("keep", "keep", "file")];
    state.dialog = FileDialog::Copy;
    state.mutating = true;
    state.reduce(
        &ctx,
        FilesMessage::ProtocolFinished {
            result: Err("复制失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert_eq!(state.entry_names(), vec!["keep".to_string()]);
    assert_eq!(state.dialog, FileDialog::Copy);
    assert_eq!(state.error, "复制失败");
    assert!(!state.mutating);
    assert!(state.effects.is_empty());
}

#[test]
fn protocol_reload_allows_category_and_refuses_smart_folder() {
    let mut category = FilesState::default();
    category.current_path = "photos".into();
    category.rows = vec![row("keep", "keep", "file")];
    category.reduce(
        &category_ctx(),
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: true,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert!(
        matches!(category.take_effects().as_slice(), [FilesEffect::Browse { path, .. }] if path == "photos")
    );

    let mut smart = FilesState::default();
    smart.rows = vec![row("keep", "keep", "file")];
    smart.dialog = FileDialog::Import;
    smart.reduce(
        &smart_ctx(),
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: true,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert!(smart.effects.is_empty());
    assert_eq!(smart.entry_names(), vec!["keep".to_string()]);
    assert_eq!(smart.dialog, FileDialog::Closed);
}

#[test]
fn thumbnail_prefetch_waits_for_the_vue_idle_gap() {
    let mut state = FilesState::default();
    state.schedule_thumbnail_prefetch(true, 0);
    assert!(!state.poll_thumbnail_prefetch(419, &["a.png".into()]));
    assert!(state.poll_thumbnail_prefetch(420, &["a.png".into()]));
    assert!(
        matches!(state.take_effects().as_slice(), [FilesEffect::DecodeThumbnails { paths }] if paths == &["a.png".to_string()])
    );
}

#[test]
fn copy_submits_the_stored_sources() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("a", "a", "file"), row("b", "b", "file")];
    state.selected = vec!["a".into(), "b".into()];
    state.primary = Some("a".into());
    assert!(reduce_open(&mut state, &ctx, FileDialog::Copy));
    state.reduce(&ctx, FilesMessage::DraftChanged("album\\nested".into()));
    assert!(submit_for(&mut state, &ctx));
    let mut slot = super::super::repo_operation::RepositoryOperation::default();
    slot.apply(&mut state.operation);
    let shown = slot.shown().map(|progress| {
        (
            progress.label.as_str(),
            progress.detail.as_str(),
            progress.value,
        )
    });
    assert_eq!(shown, Some(("复制文件", "创建硬链接或复制文件", 32.0)));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Copy { sources, parent: Some(parent), .. }]
            if sources == &["a".to_string(), "b".to_string()] && parent == "album/nested"
    ));
}

#[test]
fn directory_replace_opens_and_file_activation_loads_the_asset() {
    let mut state = FilesState::default();
    let ctx = writable();
    let mut file = row("photos/a.png", "a.png", "file");
    file.asset_id = Some("asset-1".into());
    state.rows = vec![row("photos", "photos", "directory"), file];
    state.reduce(&ctx, FilesMessage::ActivateRow("photos".into()));
    assert_eq!(state.selected, vec!["photos".to_string()]);
    assert!(state.effects.is_empty());
    state.reduce(&ctx, FilesMessage::OpenRow("photos".into()));
    assert!(state.selected.is_empty());
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Browse { path, limit, .. }] if path == "photos" && *limit == super::browse::INITIAL_PAGE_SIZE
    ));
    state.loading = false;
    state.pending = None;
    state.activate_row(&ctx, "photos", SelectionMode::Toggle);
    assert_eq!(state.selected, vec!["photos".to_string()]);
    assert!(state.effects.is_empty());
    state.activate_row(&ctx, "photos/a.png", SelectionMode::Replace);
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::LoadAsset { asset_id, .. }] if asset_id == "asset-1"
    ));
}

#[test]
fn trash_restore_and_empty_push_trash_effects() {
    let mut state = FilesState::default();
    state.rows = vec![row("gone", "gone", "file")];
    state.selected = vec!["gone".into()];
    state.primary = Some("gone".into());
    let trash = trash_ctx();
    state.reduce(&trash, FilesMessage::RestoreSelected);
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::MutateTrash { action, paths, .. }] if action == "restore" && paths == &["gone".to_string()]
    ));
    state.mutating = false;
    state.reduce(&trash, FilesMessage::EmptyTrash);
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::MutateTrash { action, paths, .. }] if action == "empty" && paths.is_empty()
    ));
    state.mutating = false;
    state.reduce(&writable(), FilesMessage::RestoreSelected);
    assert!(state.effects.is_empty());
    assert!(!state.mutating);
}

#[test]
fn eagle_copy_submits_the_library_path() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.reduce(&ctx, FilesMessage::OpenEagle("sideways".into()));
    assert_eq!(state.dialog, FileDialog::Closed);
    assert!(state.take_eagle_menu_mode().is_none());
    state.reduce(&ctx, FilesMessage::OpenEagle("copy".into()));
    assert_eq!(
        state.dialog,
        FileDialog::Closed,
        "菜单复用系统选目录，不打开路径输入框"
    );
    assert_eq!(state.take_eagle_menu_mode().as_deref(), Some("copy"));
    assert!(state.import_eagle_library(&ctx, "copy", "D:/eagle"));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::ImportEagle { library_path, mode, .. }] if library_path == "D:/eagle" && mode == "copy"
    ));
}

#[test]
fn virtual_snapshot_disables_paging_and_category_browse_is_refused() {
    let mut state = FilesState::default();
    assert!(state.apply_browser(&snapshot("", 4, vec![entry("a", "a", "file")]), true));
    assert!(!state.has_more);
    assert!(!state.load_more(&category_ctx()));
    assert!(!state.request_browse(&category_ctx(), "photos"));
    assert!(state.effects.is_empty());
}

#[test]
fn append_uses_the_larger_page_and_display_mode_persists() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("a", "a", "file")];
    state.has_more = true;
    assert!(state.load_more(&ctx));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Browse { offset: 1, limit, append: true, .. }] if *limit == super::browse::APPEND_PAGE_SIZE
    ));
    state.loading_more = false;
    state.reduce(&ctx, FilesMessage::SetDisplayMode(DisplayMode::List));
    assert_eq!(state.display_mode, DisplayMode::List);
    assert_eq!(state.take_effects(), vec![FilesEffect::PersistDisplayMode]);
}

#[test]
fn custom_thumbnail_result_updates_the_row_and_decodes_the_file() {
    let mut state = FilesState::default();
    state.rows = vec![row("a.png", "a.png", "file")];
    state.reduce(
        &writable(),
        FilesMessage::ThumbnailSaved {
            path: "a.png".into(),
            thumbnail_path: Some("thumbs/a.png".into()),
            custom: true,
        },
    );
    assert_eq!(
        state.rows[0].thumbnail_path.as_deref(),
        Some("thumbs/a.png")
    );
    assert!(state.rows[0].thumbnail_custom);
    assert!(!state.rows[0].texture_ready);
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::DecodeThumbnails { paths }] if paths == &["thumbs/a.png".to_string()]
    ));

    state.reduce(
        &writable(),
        FilesMessage::ThumbnailSaved {
            path: "a.png".into(),
            thumbnail_path: None,
            custom: false,
        },
    );
    assert!(state.rows[0].thumbnail_path.is_none());
    assert!(!state.rows[0].thumbnail_custom);
    assert!(state.take_effects().is_empty());
}

#[test]
fn export_archive_waits_for_a_real_path_and_git_uses_the_draft() {
    let mut state = FilesState::default();
    state.present_export();
    state.reduce(&writable(), FilesMessage::SubmitExport);
    assert!(state.take_effects().is_empty());
    assert_eq!(state.export.error, "压缩包导出还没有输出路径。");
    assert!(!state.export.busy);

    state.reduce(
        &writable(),
        FilesMessage::SetExportField {
            field: "output".into(),
            value: "D:/out/lib.zip".into(),
        },
    );
    state.reduce(
        &writable(),
        FilesMessage::SetExportField {
            field: "encrypt".into(),
            value: "1".into(),
        },
    );
    state.reduce(
        &writable(),
        FilesMessage::SetExportField {
            field: "password".into(),
            value: "secret".into(),
        },
    );
    state.reduce(&writable(), FilesMessage::SubmitExport);
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::ExportArchive { repo_id, output_path, format, encrypt, password, .. }]
            if repo_id == "repo" && output_path == "D:/out/lib.zip" && format == "zip" && *encrypt && password == "secret"
    ));

    state.reduce(
        &writable(),
        FilesMessage::ExportFinished(Err("磁盘已满".into())),
    );
    assert_eq!(state.export.error, "磁盘已满");
    assert!(!state.export.busy);

    state.reduce(
        &writable(),
        FilesMessage::SetExportField {
            field: "target".into(),
            value: "git".into(),
        },
    );
    state.reduce(
        &writable(),
        FilesMessage::SetExportField {
            field: "remote".into(),
            value: "origin".into(),
        },
    );
    state.reduce(
        &writable(),
        FilesMessage::SetExportField {
            field: "branch".into(),
            value: "main".into(),
        },
    );
    state.reduce(
        &writable(),
        FilesMessage::SetExportField {
            field: "message".into(),
            value: "导出".into(),
        },
    );
    state.reduce(&writable(), FilesMessage::SubmitExport);
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::ExportGit { repo_id, remote, branch, message }]
            if repo_id == "repo" && remote == "origin" && branch == "main" && message == "导出"
    ));
    state.reduce(
        &writable(),
        FilesMessage::ExportFinished(Ok("资源库已推送".into())),
    );
    assert_eq!(state.export.notice, "资源库已推送");
    assert!(state.export.error.is_empty());
}

/// 导入时用户已经有选择：文件夹和拖放按来源文件名改选，ZIP 和 Eagle 选父目录下新的直接子项。
/// 没有选择或导入失败时不改选。
#[test]
fn import_with_a_prior_selection_selects_the_imported_entries() {
    let ctx = writable();
    let finished = FilesMessage::ProtocolFinished {
        result: Ok(()),
        reload: true,
        hardlinks: false,
        folder_tree: None,
    };

    let mut folder = FilesState::default();
    folder.rows = vec![row("keep.png", "keep.png", "file")];
    folder.selected = vec!["keep.png".into()];
    folder.primary = Some("keep.png".into());
    folder.anchor = Some("keep.png".into());
    assert!(folder.import_picked_sources(
        &ctx,
        vec![
            "D:\\incoming\\cover.png".into(),
            "D:/incoming/notes/".into()
        ]
    ));
    folder.reduce(&ctx, finished.clone());
    assert!(folder.apply_browser(
        &snapshot(
            "",
            3,
            vec![
                entry("keep.png", "keep.png", "file"),
                entry("cover.png", "cover.png", "file"),
                entry("notes", "notes", "directory"),
            ],
        ),
        false,
    ));
    assert_eq!(
        folder.selected,
        vec!["cover.png".to_string(), "notes".to_string()]
    );
    assert_eq!(folder.primary.as_deref(), Some("cover.png"));
    assert_eq!(folder.anchor.as_deref(), Some("cover.png"));

    let mut plain = FilesState::default();
    plain.rows = vec![row("keep.png", "keep.png", "file")];
    assert!(plain.import_picked_sources(&ctx, vec!["D:/incoming/cover.png".into()]));
    plain.reduce(&ctx, finished.clone());
    assert!(plain.apply_browser(
        &snapshot(
            "",
            2,
            vec![
                entry("keep.png", "keep.png", "file"),
                entry("cover.png", "cover.png", "file")
            ]
        ),
        false,
    ));
    assert!(
        plain.selected.is_empty() && plain.primary.is_none(),
        "没有选择时不改选"
    );

    let mut failed = FilesState::default();
    failed.rows = vec![row("keep.png", "keep.png", "file")];
    failed.selected = vec!["keep.png".into()];
    failed.primary = Some("keep.png".into());
    assert!(failed.import_picked_sources(&ctx, vec!["D:/incoming/cover.png".into()]));
    failed.reduce(
        &ctx,
        FilesMessage::ProtocolFinished {
            result: Err("导入失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert_eq!(failed.selected, vec!["keep.png".to_string()]);
    assert!(failed.apply_browser(
        &snapshot(
            "",
            2,
            vec![
                entry("keep.png", "keep.png", "file"),
                entry("cover.png", "cover.png", "file")
            ]
        ),
        false,
    ));
    assert_eq!(
        failed.selected,
        vec!["keep.png".to_string()],
        "失败以后的目录快照不改选"
    );

    let mut zip = FilesState::default();
    zip.current_path = "inbox".into();
    zip.rows = vec![row("inbox/old.png", "old.png", "file")];
    zip.selected = vec!["inbox/old.png".into()];
    zip.primary = Some("inbox/old.png".into());
    assert!(zip.import_picked_archive(&ctx, "D:/归档.zip"));
    zip.reduce(&ctx, finished.clone());
    assert!(zip.apply_browser(
        &snapshot(
            "inbox",
            4,
            vec![
                entry("inbox/old.png", "old.png", "file"),
                entry("inbox/new.png", "new.png", "file"),
                entry("inbox/nested/extra.png", "extra.png", "file"),
            ],
        ),
        false,
    ));
    assert_eq!(
        zip.selected,
        vec!["inbox/new.png".to_string()],
        "只选父目录下新的直接子项"
    );
    assert_eq!(zip.primary.as_deref(), Some("inbox/new.png"));

    let mut eagle = FilesState::default();
    eagle.rows = vec![row("old.png", "old.png", "file")];
    eagle.selected = vec!["old.png".into()];
    eagle.primary = Some("old.png".into());
    assert!(eagle.import_eagle_library(&ctx, "copy", "D:/Eagle.library"));
    eagle.reduce(&ctx, finished.clone());
    assert!(eagle.apply_browser(
        &snapshot(
            "",
            2,
            vec![
                entry("old.png", "old.png", "file"),
                entry("page.png", "page.png", "file")
            ]
        ),
        false,
    ));
    assert_eq!(eagle.selected, vec!["page.png".to_string()]);

    let mut dropped = FilesState::default();
    dropped.rows = vec![row("keep.png", "keep.png", "file")];
    dropped.selected = vec!["keep.png".into()];
    dropped.primary = Some("keep.png".into());
    dropped.enqueue_import("repo".into(), None, vec!["C:/drop/clip.png".into()]);
    dropped.reduce(&ctx, finished);
    assert!(dropped.apply_browser(
        &snapshot(
            "",
            2,
            vec![
                entry("keep.png", "keep.png", "file"),
                entry("clip.png", "clip.png", "file")
            ]
        ),
        false,
    ));
    assert_eq!(
        dropped.selected,
        vec!["clip.png".to_string()],
        "拖放导入也按来源文件名改选"
    );
}

/// 拖放移动照 Vue `moveWorkspaceEntries`：移到别的目录时，选择换成当前列表里第一个没被移走的条目
/// （文件夹在文件前面）；移进正在看的目录时，重读对上以后按来源文件名改选。失败不改选。
#[test]
fn move_reselects_the_next_entry_or_the_arrived_file() {
    let ctx = writable();
    let finished = FilesMessage::ProtocolFinished {
        result: Ok(()),
        reload: true,
        hardlinks: false,
        folder_tree: None,
    };

    let mut away = FilesState::default();
    away.rows = vec![
        row("photos", "photos", "directory"),
        row("cover.png", "cover.png", "file"),
        row("page.pdf", "page.pdf", "file"),
    ];
    away.selected = vec!["cover.png".into()];
    away.primary = Some("cover.png".into());
    away.anchor = Some("cover.png".into());
    away.select_only = Some("cover.png".into());
    away.enqueue_move("repo".into(), vec!["cover.png".into()], "albums".into());
    away.reduce(&ctx, finished.clone());
    assert_eq!(
        away.primary.as_deref(),
        Some("photos"),
        "第一个没被移走的是文件夹"
    );
    assert_eq!(away.selected, vec!["photos".to_string()]);
    assert!(
        away.select_only.is_none(),
        "新的主选是文件夹，不再只选中那个文件"
    );
    assert!(away.apply_browser(
        &snapshot(
            "",
            2,
            vec![
                entry("photos", "photos", "directory"),
                entry("page.pdf", "page.pdf", "file")
            ]
        ),
        false,
    ));
    assert_eq!(
        away.primary.as_deref(),
        Some("photos"),
        "重读以后选择还在这个文件夹上"
    );

    let mut files = FilesState::default();
    files.rows = vec![
        row("cover.png", "cover.png", "file"),
        row("page.pdf", "page.pdf", "file"),
    ];
    files.selected = vec!["cover.png".into()];
    files.primary = Some("cover.png".into());
    files.select_only = Some("cover.png".into());
    files.enqueue_move("repo".into(), vec!["cover.png".into()], "albums".into());
    files.reduce(&ctx, finished.clone());
    assert_eq!(files.primary.as_deref(), Some("page.pdf"));
    assert_eq!(
        files.select_only.as_deref(),
        Some("page.pdf"),
        "只选中跟着剩下的文件"
    );

    let mut last = FilesState::default();
    last.rows = vec![row("cover.png", "cover.png", "file")];
    last.selected = vec!["cover.png".into()];
    last.primary = Some("cover.png".into());
    last.enqueue_move("repo".into(), vec!["cover.png".into()], "albums".into());
    last.reduce(&ctx, finished.clone());
    assert!(
        last.primary.is_none() && last.selected.is_empty(),
        "没有剩下的条目就清空选择"
    );

    let mut here = FilesState::default();
    here.current_path = "albums".into();
    here.rows = vec![row("albums/note.txt", "note.txt", "file")];
    here.selected = vec!["albums/note.txt".into()];
    here.primary = Some("albums/note.txt".into());
    here.enqueue_move(
        "repo".into(),
        vec!["elsewhere/cover.png".into()],
        "albums".into(),
    );
    here.reduce(&ctx, finished);
    assert_eq!(
        here.primary.as_deref(),
        Some("albums/note.txt"),
        "同名条目还没出现时先留着原来的选择"
    );
    assert!(here.apply_browser(
        &snapshot(
            "albums",
            2,
            vec![
                entry("albums/note.txt", "note.txt", "file"),
                entry("albums/cover.png", "cover.png", "file")
            ],
        ),
        false,
    ));
    assert_eq!(
        here.selected,
        vec!["albums/cover.png".to_string()],
        "移进当前目录后按来源文件名改选"
    );
    assert_eq!(here.primary.as_deref(), Some("albums/cover.png"));

    let mut failed = FilesState::default();
    failed.rows = vec![
        row("cover.png", "cover.png", "file"),
        row("page.pdf", "page.pdf", "file"),
    ];
    failed.selected = vec!["cover.png".into()];
    failed.primary = Some("cover.png".into());
    failed.enqueue_move("repo".into(), vec!["cover.png".into()], "albums".into());
    failed.reduce(
        &ctx,
        FilesMessage::ProtocolFinished {
            result: Err("移动失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert_eq!(
        failed.primary.as_deref(),
        Some("cover.png"),
        "移动失败不改选"
    );
    assert!(failed.apply_browser(
        &snapshot(
            "",
            2,
            vec![
                entry("cover.png", "cover.png", "file"),
                entry("page.pdf", "page.pdf", "file")
            ]
        ),
        false,
    ));
    assert_eq!(failed.selected, vec!["cover.png".to_string()]);
}

#[path = "files_copy_tests.rs"]
mod copy_reselect;

fn reduce_open(state: &mut FilesState, ctx: &FileContext, dialog: FileDialog) -> bool {
    let before = state.dialog;
    state.reduce(ctx, FilesMessage::OpenDialog(dialog));
    state.dialog != before && state.dialog == dialog
}

/// 在工具栏输入框里写下名字再点「建文件」，返回是否开始变更。
fn create_file_for(state: &mut FilesState, ctx: &FileContext, name: &str) -> bool {
    state.reduce(ctx, FilesMessage::SetCreateName(name.into()));
    let before = state.mutating;
    state.reduce(ctx, FilesMessage::SubmitCreateFile);
    state.mutating && !before
}

fn submit_for(state: &mut FilesState, ctx: &FileContext) -> bool {
    let before = state.mutating;
    state.reduce(ctx, FilesMessage::SubmitDialog);
    state.mutating && !before
}

fn delete_selected_for(state: &mut FilesState, ctx: &FileContext) -> bool {
    state.reduce(ctx, FilesMessage::DeleteSelected);
    state.mutating
}

/// 点当前这一级面包屑，或侧栏再点正在看的文件夹：照 Vue `keepsCurrentViewVisible` 静默重读。
/// 列表和选择留着，不换成「正在读取目录」；这次读取失败也只记日志。换到别的目录仍显示读取中。
#[test]
fn reloading_the_open_directory_keeps_the_list_on_screen() {
    let mut state = FilesState::default();
    let ctx = writable();
    assert!(state.request_browse(&ctx, "assets"), "第一次打开目录");
    assert!(state.loading, "还没有目录快照时要显示正在读取目录");
    let _ = state.take_effects();
    assert!(state.apply_browser(
        &snapshot(
            "assets",
            2,
            vec![
                entry("assets/a.png", "a.png", "file"),
                entry("assets/b.png", "b.png", "file")
            ]
        ),
        false,
    ));
    state.reduce(&ctx, FilesMessage::ActivateRow("assets/a.png".into()));
    assert_eq!(state.selected, vec!["assets/a.png".to_string()]);

    state.reduce(&ctx, FilesMessage::OpenPath("assets".into()));
    assert!(!state.loading, "再点当前面包屑不把列表换成正在读取目录");
    assert_eq!(
        state.entry_names(),
        vec!["a.png".to_string(), "b.png".to_string()]
    );
    assert_eq!(
        state.selected,
        vec!["assets/a.png".to_string()],
        "静默重读不清选择"
    );
    assert!(
        matches!(
            state.take_effects().as_slice(),
            [FilesEffect::Browse { path, limit, append: false, .. }] if path == "assets" && *limit >= 2
        ),
        "仍会重读这一层，而且不少于已经列出的条数"
    );
    assert!(state.note_load_failed("读取失败"));
    assert!(state.error.is_empty(), "静默重读失败不把列表换成错误");
    assert_eq!(
        state.entry_names(),
        vec!["a.png".to_string(), "b.png".to_string()]
    );
    assert!(!state.loading);

    state.reduce(&ctx, FilesMessage::OpenPath("other".into()));
    assert!(state.loading, "换到别的目录仍显示正在读取目录");
    assert!(state.selected.is_empty(), "离开当前目录才清选择");

    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    assert!(model.files.apply_browser(
        &snapshot("assets", 1, vec![entry("assets/a.png", "a.png", "file")]),
        false
    ));
    model
        .files
        .reduce(&ctx, FilesMessage::ActivateRow("assets/a.png".into()));
    model.note_sidebar_browse("assets", false, false);
    assert!(
        !model.files.loading,
        "侧栏再点正在看的文件夹也不换成正在读取目录"
    );
    assert_eq!(model.files.selected, vec!["assets/a.png".to_string()]);
    assert!(model.files.note_load_failed("读取失败"));
    assert!(
        model.files.error.is_empty(),
        "侧栏这次静默重读失败也不换掉列表"
    );
    model.note_sidebar_browse("covers", false, false);
    assert!(model.files.loading, "侧栏点开别的文件夹仍显示正在读取目录");
}
