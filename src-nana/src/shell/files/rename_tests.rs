//! 重命名成功后的选择。照 Vue `renameWorkspaceEntry`：快照里同名的那一条变成选中。
//!
//! 当前目录对得上时列表换成这份快照，选择落在新名字上。分类视图路径对不上时列表留着，
//! 选择仍换成快照里的新名字。快照里没有这个名字就不编一条。失败不改选。

use std::collections::BTreeMap;

use crate::backend::services::repository::{
    FileBrowserEntry, FileBrowserSnapshot, RepositoryStructureCacheState,
};

use super::{FileContext, FileDialog, FileRow, FilesEffect, FilesMessage, FilesState};

/// 文件对话框、侧栏文件夹和分类视图三条路，成功后都选中新名字。
#[test]
fn rename_selects_the_snapshot_entry_with_the_new_name() {
    let ctx = writable();
    let mut state = FilesState::default();
    state.rows = vec![
        row("cover.png", "cover.png", "file"),
        row("notes.txt", "notes.txt", "file"),
    ];
    state.selected = vec!["cover.png".into()];
    state.primary = Some("cover.png".into());
    state.anchor = Some("cover.png".into());
    state.reduce(&ctx, FilesMessage::OpenDialog(FileDialog::Rename));
    state.reduce(&ctx, FilesMessage::DraftChanged("poster.png".into()));
    assert!(submit(&mut state, &ctx), "新名字非空才提交");
    assert!(state.take_effects().iter().any(
        |effect| matches!(effect, FilesEffect::Rename { new_name, .. } if new_name == "poster.png")
    ));
    state.reduce(
        &ctx,
        FilesMessage::MutationSnapshot {
            result: Ok(snapshot(
                "",
                vec![
                    entry("poster.png", "poster.png", "file"),
                    entry("notes.txt", "notes.txt", "file"),
                ],
            )),
            created_name: None,
        },
    );
    assert_eq!(state.dialog, FileDialog::Closed);
    assert_eq!(
        state.selected,
        vec!["poster.png".to_string()],
        "选中换成快照里的新名字"
    );
    assert_eq!(state.primary.as_deref(), Some("poster.png"));
    assert_eq!(state.anchor.as_deref(), Some("poster.png"));

    let mut failed = FilesState::default();
    failed.rows = vec![row("cover.png", "cover.png", "file")];
    failed.selected = vec!["cover.png".into()];
    failed.primary = Some("cover.png".into());
    failed.reduce(&ctx, FilesMessage::OpenDialog(FileDialog::Rename));
    failed.reduce(&ctx, FilesMessage::DraftChanged("poster.png".into()));
    assert!(submit(&mut failed, &ctx));
    failed.reduce(
        &ctx,
        FilesMessage::MutationSnapshot {
            result: Err("改名失败".into()),
            created_name: None,
        },
    );
    assert_eq!(failed.dialog, FileDialog::Rename, "失败对话框留着");
    assert_eq!(failed.selected, vec!["cover.png".to_string()], "失败不改选");
    assert_eq!(failed.primary.as_deref(), Some("cover.png"));

    let mut missing = FilesState::default();
    missing.rows = vec![row("cover.png", "cover.png", "file")];
    missing.selected = vec!["cover.png".into()];
    missing.primary = Some("cover.png".into());
    missing.reduce(&ctx, FilesMessage::OpenDialog(FileDialog::Rename));
    missing.reduce(&ctx, FilesMessage::DraftChanged("poster.png".into()));
    assert!(submit(&mut missing, &ctx));
    missing.reduce(
        &ctx,
        FilesMessage::MutationSnapshot {
            result: Ok(snapshot("", vec![entry("notes.txt", "notes.txt", "file")])),
            created_name: None,
        },
    );
    assert!(
        missing.selected.is_empty() && missing.primary.is_none(),
        "快照里没有新名字，不挑一条凑数"
    );

    let mut category = FilesState::default();
    category.current_path = "photos".into();
    category.rows = vec![row("keep", "keep", "file")];
    category.selected = vec!["photos/cover.png".into()];
    category.primary = Some("photos/cover.png".into());
    let category_ctx = FileContext {
        category_virtual: true,
        ..writable()
    };
    category.reduce(&category_ctx, FilesMessage::OpenDialog(FileDialog::Rename));
    category.reduce(
        &category_ctx,
        FilesMessage::DraftChanged("poster.png".into()),
    );
    assert!(submit(&mut category, &category_ctx));
    category.reduce(
        &category_ctx,
        FilesMessage::MutationSnapshot {
            result: Ok(snapshot(
                "other",
                vec![entry("other/poster.png", "poster.png", "file")],
            )),
            created_name: None,
        },
    );
    assert_eq!(
        category.entry_names(),
        vec!["keep".to_string()],
        "分类视图路径对不上，列表留着"
    );
    assert_eq!(
        category.selected,
        vec!["other/poster.png".to_string()],
        "选择仍换成快照里的新名字"
    );
    assert_eq!(category.primary.as_deref(), Some("other/poster.png"));
    assert_eq!(category.current_path, "photos");

    let mut sidebar = FilesState::default();
    sidebar.rows = vec![
        row("photos", "photos", "directory"),
        row("cover.png", "cover.png", "file"),
    ];
    sidebar.selected = vec!["cover.png".into()];
    sidebar.primary = Some("cover.png".into());
    sidebar.enqueue(FilesEffect::Rename {
        repo_id: "repo".into(),
        path: "photos".into(),
        new_name: "albums".into(),
    });
    sidebar.reduce(
        &ctx,
        FilesMessage::MutationSnapshot {
            result: Ok(snapshot(
                "",
                vec![
                    entry("albums", "albums", "directory"),
                    entry("cover.png", "cover.png", "file"),
                ],
            )),
            created_name: None,
        },
    );
    assert_eq!(
        sidebar.selected,
        vec!["albums".to_string()],
        "侧栏重命名文件夹也选中新名字"
    );
    assert_eq!(sidebar.primary.as_deref(), Some("albums"));
}

fn submit(state: &mut FilesState, ctx: &FileContext) -> bool {
    let before = state.mutating;
    state.reduce(ctx, FilesMessage::SubmitDialog);
    state.mutating && !before
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

fn snapshot(path: &str, entries: Vec<FileBrowserEntry>) -> FileBrowserSnapshot {
    FileBrowserSnapshot {
        repo_id: "repo".into(),
        root_path: "C:/repo".into(),
        backend_plugin_id: "local".into(),
        backend_kind: "local".into(),
        cache_state: RepositoryStructureCacheState::Ready,
        indexed_at: None,
        current_path: path.into(),
        total_entries: entries.len(),
        loaded_count: entries.len(),
        next_offset: None,
        has_more: false,
        special_location: None,
        tree: None,
        entries,
    }
}
