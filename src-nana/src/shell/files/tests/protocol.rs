//! 硬链接候选、协议文件夹重读、缩略图预取、复制、目录打开、回收站与 Eagle 复制的测试。

use super::*;

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
        ] if path == "photos" && *limit == super::super::browse::INITIAL_PAGE_SIZE && repo_id == "repo"
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
fn thumbnail_prefetch_waits_for_the_idle_gap() {
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
    let mut slot = super::super::super::repo_operation::RepositoryOperation::default();
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
        [FilesEffect::Browse { path, limit, .. }] if path == "photos" && *limit == super::super::browse::INITIAL_PAGE_SIZE
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
        [FilesEffect::Browse { offset: 1, limit, append: true, .. }] if *limit == super::super::browse::APPEND_PAGE_SIZE
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
