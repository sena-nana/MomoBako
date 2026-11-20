//! 复制成功后的选择。照 Vue `finishFileTransfer`：重读复制进去的目录，按来源文件名改选。
//!
//! 复制到当前目录时，选择换成源文件名，不把对话框打开以后新点上的条目留在选择里。
//! 复制到别的目录时，列表换成目标目录，再按源文件名选里面的条目。没有源文件名不挑一条。失败不改选。

use super::{
    FileDialog, FilesEffect, FilesMessage, FilesState, entry, reduce_open, row, snapshot,
    submit_for, writable,
};

/// 复制到当前目录、复制到别的目录、没有源文件名、失败，四种结果分开核对。
#[test]
fn copy_reselects_by_source_name_in_the_target_directory() {
    let ctx = writable();
    let ok = FilesMessage::ProtocolFinished {
        result: Ok(()),
        reload: true,
        hardlinks: false,
        folder_tree: None,
    };

    let mut here = FilesState::default();
    here.current_path = "albums".into();
    here.rows = vec![
        row("albums/keep.png", "keep.png", "file"),
        row("albums/cover.png", "cover.png", "file"),
    ];
    here.selected = vec!["albums/cover.png".into()];
    here.primary = Some("albums/cover.png".into());
    assert!(reduce_open(&mut here, &ctx, FileDialog::Copy));
    here.selected.push("albums/keep.png".into());
    here.primary = Some("albums/keep.png".into());
    here.reduce(&ctx, FilesMessage::DraftChanged("albums".into()));
    assert!(submit_for(&mut here, &ctx));
    assert!(
        here.take_effects()
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Copy { .. }))
    );
    here.reduce(&ctx, ok.clone());
    assert!(
        matches!(here.take_effects().as_slice(), [FilesEffect::Browse { path, .. }] if path == "albums")
    );
    assert!(here.apply_browser(
        &snapshot(
            "albums",
            3,
            vec![
                entry("albums/keep.png", "keep.png", "file"),
                entry("albums/cover.png", "cover.png", "file"),
                entry("albums/page.pdf", "page.pdf", "file"),
            ],
        ),
        false,
    ));
    assert_eq!(
        here.selected,
        vec!["albums/cover.png".to_string()],
        "当前目录只留下源文件名"
    );
    assert_eq!(here.primary.as_deref(), Some("albums/cover.png"));

    let mut away = FilesState::default();
    away.rows = vec![
        row("keep.png", "keep.png", "file"),
        row("cover.png", "cover.png", "file"),
    ];
    away.selected = vec!["cover.png".into(), "keep.png".into()];
    away.primary = Some("cover.png".into());
    away.anchor = Some("cover.png".into());
    assert!(reduce_open(&mut away, &ctx, FileDialog::Copy));
    away.reduce(&ctx, FilesMessage::DraftChanged("albums".into()));
    assert!(submit_for(&mut away, &ctx));
    assert!(
        away.take_effects()
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Copy { .. }))
    );
    away.reduce(&ctx, ok);
    assert_eq!(
        away.primary.as_deref(),
        Some("cover.png"),
        "目标目录读回来以前不改选"
    );
    assert!(
        matches!(away.take_effects().as_slice(), [FilesEffect::Browse { path, .. }] if path == "albums")
    );
    assert!(away.apply_browser(
        &snapshot(
            "albums",
            3,
            vec![
                entry("albums/old.png", "old.png", "file"),
                entry("albums/cover.png", "cover.png", "file"),
                entry("albums/keep.png", "keep.png", "file"),
            ],
        ),
        false,
    ));
    assert_eq!(away.current_path, "albums");
    assert_eq!(
        away.selected,
        vec![
            "albums/cover.png".to_string(),
            "albums/keep.png".to_string()
        ]
    );
    assert_eq!(away.primary.as_deref(), Some("albums/cover.png"));
    assert_eq!(away.anchor.as_deref(), Some("albums/cover.png"));

    let mut blank = FilesState::default();
    blank.rows = vec![row("keep.png", "keep.png", "file")];
    blank.selected = vec!["keep.png".into()];
    blank.primary = Some("keep.png".into());
    assert!(reduce_open(&mut blank, &ctx, FileDialog::Copy));
    blank.copy_sources = vec![String::new()];
    blank.reduce(&ctx, FilesMessage::DraftChanged("albums".into()));
    assert!(submit_for(&mut blank, &ctx));
    assert!(
        blank
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Copy { .. }))
    );
    blank.reduce(
        &ctx,
        FilesMessage::ProtocolFinished {
            result: Ok(()),
            reload: true,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert!(
        matches!(blank.take_effects().as_slice(), [FilesEffect::Browse { path, .. }] if path == "albums")
    );
    assert!(blank.apply_browser(
        &snapshot(
            "albums",
            1,
            vec![entry("albums/old.png", "old.png", "file")]
        ),
        false
    ));
    assert!(
        blank.selected.is_empty() && blank.primary.is_none(),
        "没有源文件名不编选择"
    );

    let mut failed = FilesState::default();
    failed.rows = vec![row("cover.png", "cover.png", "file")];
    failed.selected = vec!["cover.png".into()];
    failed.primary = Some("cover.png".into());
    assert!(reduce_open(&mut failed, &ctx, FileDialog::Copy));
    failed.reduce(&ctx, FilesMessage::DraftChanged("albums".into()));
    assert!(submit_for(&mut failed, &ctx));
    assert!(
        failed
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Copy { .. }))
    );
    failed.reduce(
        &ctx,
        FilesMessage::ProtocolFinished {
            result: Err("复制失败".into()),
            reload: false,
            hardlinks: false,
            folder_tree: None,
        },
    );
    assert_eq!(failed.dialog, FileDialog::Copy);
    assert_eq!(failed.selected, vec!["cover.png".to_string()]);
    assert_eq!(failed.primary.as_deref(), Some("cover.png"));
    assert!(failed.current_path.is_empty());
    assert!(failed.effects.is_empty());
}
