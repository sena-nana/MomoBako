//! 导出归档、导入后改选与移动后改选的测试。

use super::*;

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

/// 拖放移动：移到别的目录时，选择换成当前列表里第一个没被移走的条目
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
