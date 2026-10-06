//! 文件浏览状态机测试。只调用状态方法，不写用户的展示方式文件。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
};

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
    FileContext { trash: true, ..writable() }
}

fn smart_ctx() -> FileContext {
    FileContext { smart_folder: true, ..writable() }
}

fn category_ctx() -> FileContext {
    FileContext { category_virtual: true, ..writable() }
}

#[test]
fn append_merges_unique_keys_and_has_more_follows_total() {
    let mut state = FilesState::default();
    let ctx = writable();
    assert!(state.request_browse(&ctx, ""));
    assert!(state.apply_browser(&snapshot("", 4, vec![entry("a", "a", "file"), entry("b", "b", "file")]), false));
    assert!(state.has_more);
    assert!(state.load_more(&ctx));
    assert!(state.apply_browser(&snapshot("", 4, vec![entry("a", "a", "file"), entry("c", "c", "file")]), false));
    assert_eq!(state.entry_names(), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
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
    });
    state.loading = true;
    assert!(!state.apply_browser(&snapshot("other", 1, vec![entry("new", "new", "file")]), false));
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
    assert!(!reduce_open(&mut state, &smart, FileDialog::CreateDirectory));
    assert!(!reduce_open(&mut state, &smart, FileDialog::Import));
    assert!(!state.can_rename(&smart));
    assert!(!delete_selected_for(&mut state, &smart));
    assert!(!reduce_open(&mut state, &category, FileDialog::CreateFile));
    assert!(!state.can_import(&category));
    assert!(state.can_rename(&category));
    assert!(state.can_delete(&category));
    assert_eq!(state.visible_rows(&category).iter().map(|row| row.name.clone()).collect::<Vec<_>>(), vec!["file".to_string()]);
    assert_eq!(state.visible_rows(&smart).iter().map(|row| row.name.clone()).collect::<Vec<_>>(), vec!["smart".to_string()]);
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
    assert!(!reduce_open(&mut state, &trash, FileDialog::CreateFile));
    assert!(!reduce_open(&mut state, &trash, FileDialog::Copy));
    assert!(delete_selected_for(&mut state, &trash));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Delete { mode: Some(mode), .. }] if mode == "permanentDelete"
    ));
    state.mutating = false;
    assert!(delete_selected_for(&mut state, &writable()));
    assert!(matches!(state.take_effects().as_slice(), [FilesEffect::Delete { mode: None, .. }]));
}

#[test]
fn blank_name_and_blank_import_do_not_mutate() {
    let mut state = FilesState::default();
    let ctx = writable();
    assert!(reduce_open(&mut state, &ctx, FileDialog::CreateFile));
    state.reduce(&ctx, FilesMessage::DraftChanged("  ".into()));
    assert!(!submit_for(&mut state, &ctx));
    assert!(!state.mutating);
    assert!(state.effects.is_empty());
    assert!(reduce_open(&mut state, &ctx, FileDialog::Import));
    state.reduce(&ctx, FilesMessage::DraftChanged(" \n; ".into()));
    assert!(!submit_for(&mut state, &ctx));
    assert!(!state.mutating);
}

#[test]
fn mutation_failure_keeps_names_and_dialog() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("keep", "keep", "file")];
    state.dialog = FileDialog::CreateFile;
    state.name_draft = "draft.txt".into();
    state.mutating = true;
    state.reduce(&ctx, FilesMessage::MutationSnapshot { result: Err("写入失败".into()), created_name: Some("draft.txt".into()) });
    assert_eq!(state.entry_names(), vec!["keep".to_string()]);
    assert!(!state.mutating);
    assert_eq!(state.dialog, FileDialog::CreateFile);
    assert_eq!(state.error, "写入失败");
}

#[test]
fn mutation_success_uses_snapshot_names_and_category_mismatch_keeps_the_list() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("old", "old", "file")];
    state.selected = vec!["old".into()];
    state.primary = Some("old".into());
    state.dialog = FileDialog::CreateFile;
    state.mutating = true;
    let snap = snapshot("photos", 1, vec![entry("photos/server.txt", "server.txt", "file")]);
    state.reduce(&ctx, FilesMessage::MutationSnapshot { result: Ok(snap), created_name: Some("draft.txt".into()) });
    assert_eq!(state.entry_names(), vec!["server.txt".to_string()]);
    assert!(state.selected.is_empty());
    assert_eq!(state.dialog, FileDialog::Closed);
    assert!(!state.mutating);

    let mut category = FilesState::default();
    category.rows = vec![row("keep", "keep", "file")];
    category.current_path = "photos".into();
    category.dialog = FileDialog::Rename;
    category.name_draft = "draft".into();
    category.mutating = true;
    let mismatch = snapshot("other", 1, vec![entry("other/new", "new", "file")]);
    category.reduce(&category_ctx(), FilesMessage::MutationSnapshot { result: Ok(mismatch), created_name: Some("draft".into()) });
    assert_eq!(category.entry_names(), vec!["keep".to_string()]);
    assert_eq!(category.dialog, FileDialog::Closed);
    assert!(category.name_draft.is_empty());
}

#[test]
fn selection_replace_toggle_range_and_snapshot_prune() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("a", "a", "file"), row("b", "b", "file"), row("c", "c", "file")];
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
    assert_eq!(state.selected, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    state.primary = Some("b".into());
    state.anchor = Some("c".into());
    assert!(state.apply_browser(&snapshot("", 1, vec![entry("a", "a", "file")]), false));
    assert_eq!(state.selected, vec!["a".to_string()]);
    assert_eq!(state.primary.as_deref(), Some("a"));
    assert_eq!(state.anchor.as_deref(), Some("a"));
}

#[test]
fn hardlink_skip_hides_candidate_and_confirm_failure_keeps_it() {
    let mut state = FilesState::default();
    let ctx = writable();
    let first = HardlinkPrompt { id: "one".into(), new_path: "n".into(), existing_path: "e".into(), size_label: "1 KB".into() };
    let second = HardlinkPrompt { id: "two".into(), new_path: "n2".into(), existing_path: "e2".into(), size_label: "2 KB".into() };
    state.reduce(&ctx, FilesMessage::HardlinksLoaded(Ok(vec![first.clone(), second.clone()])));
    assert_eq!(state.dialog, FileDialog::Hardlink);
    assert_eq!(state.current_hardlink().map(|item| item.id.as_str()), Some("one"));
    state.reduce(&ctx, FilesMessage::SkipHardlink);
    assert_eq!(state.current_hardlink().map(|item| item.id.as_str()), Some("two"));
    state.mutating = true;
    state.reduce(&ctx, FilesMessage::HardlinkConfirmed(Err("确认失败".into())));
    assert_eq!(state.current_hardlink().map(|item| item.id.as_str()), Some("two"));
    assert_eq!(state.error, "确认失败");
    assert!(!state.mutating);
    state.reduce(&ctx, FilesMessage::HardlinkConfirmed(Ok("two".into())));
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

    state.reduce(&ctx, FilesMessage::HardlinksRefreshed(Ok(vec![fresh.clone()])));
    assert_eq!(state.hardlinks, vec![fresh.clone()]);
    assert_eq!(state.dialog, FileDialog::Closed);
    assert_eq!(state.skipped_hardlinks, vec!["kept".to_string()]);
    assert_eq!(state.error, "旧错误");
    assert!(state.loading);

    state.dialog = FileDialog::Rename;
    state.reduce(&ctx, FilesMessage::HardlinksRefreshed(Err("刷新失败".into())));
    assert_eq!(state.hardlinks, vec![fresh]);
    assert_eq!(state.dialog, FileDialog::Rename);
    assert_eq!(state.skipped_hardlinks, vec!["kept".to_string()]);
    assert_eq!(state.error, "旧错误");
    assert!(state.loading);

    state.reduce(&ctx, FilesMessage::HardlinksLoaded(Ok(vec![kept.clone()])));
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
        [FilesEffect::RefreshHardlinks { repo_id }] if repo_id == "repo"
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
        FilesMessage::ProtocolFinished { result: Ok(()), reload: true, hardlinks: true },
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
            FilesEffect::LoadHardlinks { repo_id },
        ] if path == "photos" && *limit == super::INITIAL_PAGE_SIZE && repo_id == "repo"
    ));
}

#[test]
fn protocol_failure_keeps_rows_and_dialog() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("keep", "keep", "file")];
    state.dialog = FileDialog::Move;
    state.mutating = true;
    state.reduce(
        &ctx,
        FilesMessage::ProtocolFinished { result: Err("移动失败".into()), reload: false, hardlinks: false },
    );
    assert_eq!(state.entry_names(), vec!["keep".to_string()]);
    assert_eq!(state.dialog, FileDialog::Move);
    assert_eq!(state.error, "移动失败");
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
        FilesMessage::ProtocolFinished { result: Ok(()), reload: true, hardlinks: false },
    );
    assert!(matches!(category.take_effects().as_slice(), [FilesEffect::Browse { path, .. }] if path == "photos"));

    let mut smart = FilesState::default();
    smart.rows = vec![row("keep", "keep", "file")];
    smart.dialog = FileDialog::Import;
    smart.reduce(
        &smart_ctx(),
        FilesMessage::ProtocolFinished { result: Ok(()), reload: true, hardlinks: false },
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
    assert!(matches!(state.take_effects().as_slice(), [FilesEffect::DecodeThumbnails { paths }] if paths == &["a.png".to_string()]));
}

#[test]
fn copy_and_move_submit_the_stored_sources() {
    let mut state = FilesState::default();
    let ctx = writable();
    state.rows = vec![row("a", "a", "file"), row("b", "b", "file")];
    state.selected = vec!["a".into(), "b".into()];
    state.primary = Some("a".into());
    assert!(reduce_open(&mut state, &ctx, FileDialog::Copy));
    state.reduce(&ctx, FilesMessage::DraftChanged("album\\nested".into()));
    assert!(submit_for(&mut state, &ctx));
    assert_eq!(state.operation.as_ref().map(|item| item.value), Some(32.0));
    assert_eq!(state.operation.as_ref().map(|item| item.detail.as_str()), Some("创建硬链接或复制文件"));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Copy { sources, parent: Some(parent), .. }]
            if sources == &["a".to_string(), "b".to_string()] && parent == "album/nested"
    ));
    state.mutating = false;
    state.dialog = FileDialog::Closed;
    assert!(reduce_open(&mut state, &ctx, FileDialog::Move));
    state.reduce(&ctx, FilesMessage::DraftChanged("  ".into()));
    assert!(submit_for(&mut state, &ctx));
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Move { parent, .. }] if parent.is_empty()
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
    assert!(state.selected.is_empty());
    assert!(matches!(
        state.take_effects().as_slice(),
        [FilesEffect::Browse { path, limit, .. }] if path == "photos" && *limit == super::INITIAL_PAGE_SIZE
    ));
    state.loading = false;
    state.pending = None;
    state.reduce(&ctx, FilesMessage::SetSelectionMode(SelectionMode::Toggle));
    state.reduce(&ctx, FilesMessage::ActivateRow("photos".into()));
    assert_eq!(state.selected, vec!["photos".to_string()]);
    assert!(state.effects.is_empty());
    state.reduce(&ctx, FilesMessage::SetSelectionMode(SelectionMode::Replace));
    state.reduce(&ctx, FilesMessage::ActivateRow("photos/a.png".into()));
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
    state.reduce(&ctx, FilesMessage::OpenEagle("copy".into()));
    assert_eq!(state.dialog, FileDialog::ImportEagle);
    state.reduce(&ctx, FilesMessage::DraftChanged("  ".into()));
    assert!(!submit_for(&mut state, &ctx));
    state.reduce(&ctx, FilesMessage::DraftChanged("D:/eagle".into()));
    assert!(submit_for(&mut state, &ctx));
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
        [FilesEffect::Browse { offset: 1, limit, append: true, .. }] if *limit == super::APPEND_PAGE_SIZE
    ));
    state.loading_more = false;
    state.reduce(&ctx, FilesMessage::SetDisplayMode(DisplayMode::List));
    assert_eq!(state.display_mode, DisplayMode::List);
    assert_eq!(state.take_effects(), vec![FilesEffect::PersistDisplayMode]);
}

fn reduce_open(state: &mut FilesState, ctx: &FileContext, dialog: FileDialog) -> bool {
    let before = state.dialog;
    state.reduce(ctx, FilesMessage::OpenDialog(dialog));
    state.dialog != before && state.dialog == dialog
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
