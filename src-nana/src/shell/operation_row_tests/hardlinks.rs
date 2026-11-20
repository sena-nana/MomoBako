//! 确认硬链接后重读候选、摘要与目录，以及失败、过期与无候选时对话框的测试。

use super::*;

/// 确认硬链接一并刷新目录、候选和仓库摘要：确认本身不出
/// 进度行；成功以后重读候选、静默重读仓库摘要、非静默地重读当前目录，任务弹层是「读取目录」，目录读回来收起。
/// 只有这一条候选时确认框关掉。
#[test]
fn confirming_a_hardlink_reloads_candidates_summary_and_directory() {
    let (mut model, repo_id) = hardlink_page();
    assert_eq!(model.files.dialog, FileDialog::Hardlink);
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert_eq!(
        model.files.take_effects(),
        vec![FilesEffect::ConfirmHardlink {
            repo_id: repo_id.clone(),
            candidate_id: "link-1".into()
        }]
    );
    assert!(model.files.mutating, "确认在途时按钮禁用");
    assert!(operation_row(&model).is_none(), "确认本身不出进度行");

    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: repo_id.clone(),
            result: Ok("link-1".into()),
        },
    );
    assert!(!model.files.mutating);
    assert_eq!(model.files.dialog, FileDialog::Closed);
    let effects = model.files.take_effects();
    assert!(
        effects.contains(&FilesEffect::CheckHardlinks {
            repo_id: repo_id.clone(),
            epoch: epoch + 1
        }),
        "重读候选：{effects:?}"
    );
    assert!(
        effects.iter().any(|effect| matches!(effect, FilesEffect::Browse { path, append: false, trash: false, .. } if path.is_empty())),
        "重读当前目录：{effects:?}"
    );
    assert_eq!(
        model.workspace.take_effects(),
        vec![WorkspaceEffect::LoadSnapshotSilent { repo_id }]
    );
    assert_eq!(
        operation_row(&model),
        Some(("读取目录".into(), "正在读取根目录".into(), 14.0))
    );
    let message = reloaded(&model);
    model.reduce(message);
    assert_eq!(
        operation_row(&model),
        Some(("读取目录".into(), "整理目录条目".into(), 100.0))
    );
}

/// 两条候选，确认第一条成功：整轮刷新都算在变更里，重读的候选回来以前确认框一直处理中（按钮禁用、
/// 点「加入关联」「跳过」和 Escape 都不动），回来以后按新的列表显示下一条。确认以前发出的读取晚到，不会把确认掉的
/// 那一条写回来。
#[test]
fn the_dialog_waits_for_the_reread_and_ignores_reads_from_before_the_confirmation() {
    let (mut model, repo_id) = hardlink_page();
    let prompt = |id: &str, new_path: &str| HardlinkPrompt {
        id: id.into(),
        new_path: new_path.into(),
        existing_path: "cover.png".into(),
        size_label: "2.3 MB".into(),
    };
    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinksChecked {
            epoch,
            result: Ok(vec![
                prompt("link-1", "inbox/cover.png"),
                prompt("link-2", "drafts/cover.png"),
            ]),
        },
    );
    // 确认以前因为结构更新发出去、还没回来的静默重读。
    let host = crate::shell::host_events::HostMessage::StructureUpdated {
        repo_id: repo_id.clone(),
        reason: "watch".into(),
    };
    model.reduce(ShellMessage::Host(host));
    let before = model.files.take_effects();
    assert!(
        before.contains(&FilesEffect::RefreshHardlinks {
            repo_id: repo_id.clone(),
            epoch
        }),
        "{before:?}"
    );

    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: repo_id.clone(),
            result: Ok("link-1".into()),
        },
    );
    assert_eq!(model.files.dialog, FileDialog::Hardlink);
    let view = HardlinkView::project(&model).expect("确认框还开着");
    assert!(view.busy, "重读回来以前处理中");
    assert_eq!(view.confirm, "处理中...");
    assert_eq!(view.new_path, "drafts/cover.png");
    let _ = model.files.take_effects();
    files(&mut model, FilesMessage::ConfirmHardlink);
    files(&mut model, FilesMessage::SkipHardlink);
    assert!(model.files.take_effects().is_empty(), "处理中不能确认");
    assert_eq!(
        model.files.current_hardlink().map(|item| item.id.as_str()),
        Some("link-2"),
        "处理中不能跳过"
    );
    assert!(model.files.escape_layer().is_none(), "处理中 Escape 不跳过");

    // 确认以前发出的静默重读晚到：里面还有 link-1，不认。
    files(
        &mut model,
        FilesMessage::HardlinksRefreshed {
            epoch,
            result: Ok(vec![
                prompt("link-1", "inbox/cover.png"),
                prompt("link-2", "drafts/cover.png"),
            ]),
        },
    );
    assert!(
        HardlinkView::project(&model).expect("确认框").busy,
        "过期的结果不放开确认框"
    );
    assert_eq!(
        model.files.current_hardlink().map(|item| item.id.as_str()),
        Some("link-2"),
        "确认掉的那一条不回来"
    );

    // 确认以后的重读回来：按新列表显示下一条，放开按钮。
    files(
        &mut model,
        FilesMessage::HardlinksChecked {
            epoch: epoch + 1,
            result: Ok(vec![prompt("link-3", "more/cover.png")]),
        },
    );
    let view = HardlinkView::project(&model).expect("确认框");
    assert!(!view.busy);
    assert_eq!(view.new_path, "more/cover.png", "显示新列表里的下一条");
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert_eq!(
        model.files.take_effects(),
        vec![FilesEffect::ConfirmHardlink {
            repo_id,
            candidate_id: "link-3".into()
        }]
    );
}

/// 确认失败时静默重读候选，确认框和这一条留着，错误写在文件列，不重读目录和摘要；重读回来以前确认框处理中，
/// 回来以后按新的列表走（候选已经失效、列表空了就收起）。确认回来时已经换了仓库就什么也不读。
#[test]
fn a_failed_or_stale_hardlink_confirmation_does_not_reload_the_workspace() {
    let (mut model, repo_id) = hardlink_page();
    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: repo_id.clone(),
            result: Err("哈希不一致".into()),
        },
    );
    assert_eq!(
        model.files.take_effects(),
        vec![FilesEffect::RefreshHardlinks {
            repo_id: repo_id.clone(),
            epoch: epoch + 1
        }]
    );
    assert!(model.workspace.take_effects().is_empty());
    assert_eq!(model.files.dialog, FileDialog::Hardlink, "这一条留着");
    assert_eq!(model.files.error, "哈希不一致");
    assert!(operation_row(&model).is_none());
    assert!(
        HardlinkView::project(&model).expect("确认框").busy,
        "重读回来以前处理中"
    );
    files(&mut model, FilesMessage::ConfirmHardlink);
    assert!(model.files.take_effects().is_empty(), "处理中不能再点");

    let still = HardlinkPrompt {
        id: "link-1".into(),
        new_path: "inbox/cover.png".into(),
        existing_path: "cover.png".into(),
        size_label: "2.3 MB".into(),
    };
    files(
        &mut model,
        FilesMessage::HardlinksRefreshed {
            epoch: epoch + 1,
            result: Ok(vec![still]),
        },
    );
    assert!(
        !HardlinkView::project(&model).expect("确认框").busy,
        "重读回来放开"
    );
    files(&mut model, FilesMessage::ConfirmHardlink);
    let _ = model.files.take_effects();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id: "other-repo".into(),
            result: Ok("link-1".into()),
        },
    );
    assert!(!model.files.mutating);
    assert!(!model.files.hardlink_busy(), "别的仓库的结果不等重读");
    assert!(
        model.files.take_effects().is_empty(),
        "别的仓库的结果不重读"
    );
    assert!(model.workspace.take_effects().is_empty());
}

/// 确认失败以后的重读里，这一条已经失效（服务端删掉了）：列表空了，确认框收起，不留一个画不出来的打开状态。
#[test]
fn a_reread_without_candidates_closes_the_dialog() {
    let (mut model, repo_id) = hardlink_page();
    files(&mut model, FilesMessage::ConfirmHardlink);
    let epoch = model.files.hardlink_epoch();
    files(
        &mut model,
        FilesMessage::HardlinkConfirmed {
            repo_id,
            result: Err("hardlink candidate is no longer valid".into()),
        },
    );
    files(
        &mut model,
        FilesMessage::HardlinksRefreshed {
            epoch: epoch + 1,
            result: Ok(Vec::new()),
        },
    );
    assert_eq!(model.files.dialog, FileDialog::Closed);
    assert!(!model.files.hardlink_busy());
}
