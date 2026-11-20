//! 原生导入工具页的测试：Eagle 资源库选择与导入、导入报告与警告、协议结果与菜单、提示与复制标签。

use nana_ui::FileDialogKind;
use nana_ui_platform::WindowId;
use nana_ui_platform::host::WindowCommand;

use super::super::super::files::{
    EagleImportWarning, FileContext, FileDialog, FilesEffect, FilesMessage,
};
use super::super::super::input::InputMessage;
use super::super::super::workspace::WorkspaceRepository;
use super::super::super::{ShellMessage, ShellViewModel};
use super::super::AdminMessage;
use super::super::support::TOOL_EAGLE_IMPORTER;
use super::{
    EagleLibraryImportResponse, EagleLibraryImportSummary, EagleLibraryImportWarning, ImportView,
    block_reason, choose_eagle_library, eagle_import_message, eagle_import_report,
    eagle_path_label, eagle_submit_label, import_block_reason,
};

fn send(model: &mut ShellViewModel, message: ShellMessage) {
    model.reduce(message);
}

/// 可写的当前仓库。没选库目录时导入按钮不能提交。
fn writable() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.active_repo_id = Some("repo".into());
    model.workspace.repositories = vec![WorkspaceRepository {
        repo_id: "repo".into(),
        name: "资料库".into(),
        path: "C:/repo".into(),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: vec!["write".into()],
        cache_required: false,
        cache_status: String::new(),
        cache_path: None,
        login_expired: false,
        logged_out: false,
    }];
    model.files.current_path = "inbox".into();
    model
}

/// 取走刚排队的 EagleLibrary 目录对话框，并核对它是选文件夹。
fn dialog_id(model: &mut ShellViewModel) -> u64 {
    let commands = model.input.take_platform_commands(WindowId(1), false);
    match commands.as_slice() {
        [WindowCommand::OpenFileDialog { request, .. }] => {
            assert_eq!(request.kind, FileDialogKind::PickFolder);
            assert_eq!(request.title.as_deref(), Some("选择 EagleLibrary 目录"));
            request.id
        }
        other => panic!("expected folder dialog, got {other:?}"),
    }
}

#[test]
fn eagle_library_picker_writes_the_chosen_path_and_import_stays_honest() {
    let mut model = ShellViewModel::default();
    assert!(choose_eagle_library(false).is_none());
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
    );
    assert!(
        model
            .input
            .take_platform_commands(WindowId(1), false)
            .is_empty(),
        "没有仓库时不打开目录对话框"
    );

    let mut model = writable();
    let page = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
    assert_eq!(eagle_path_label(&page.library_path), "尚未选择");
    assert!(eagle_import_message(&page, "copy").is_none());
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
    );
    assert_eq!(model.files.error, "请先选择 EagleLibrary 目录。");
    assert!(model.files.take_effects().is_empty());
    assert!(model.operation.shown().is_none());
    assert_eq!(model.files.dialog, FileDialog::Closed);

    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
    );
    let request_id = dialog_id(&mut model);
    send(
        &mut model,
        ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: Vec::new(),
            failed: None,
        }),
    );
    assert!(model.admin.eagle_library_path.is_empty());
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
    );
    let request_id = dialog_id(&mut model);
    send(
        &mut model,
        ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: vec!["  ".into()],
            failed: None,
        }),
    );
    assert!(model.admin.eagle_library_path.is_empty());
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
    );
    let request_id = dialog_id(&mut model);
    send(
        &mut model,
        ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: vec!["D:/Eagle".into()],
            failed: Some("Busy".into()),
        }),
    );
    assert!(
        model.admin.eagle_library_path.is_empty(),
        "选择失败不写入路径"
    );

    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
    );
    let request_id = dialog_id(&mut model);
    send(
        &mut model,
        ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: vec![" D:/Eagle/动画.library ".into()],
            failed: None,
        }),
    );
    assert_eq!(model.admin.eagle_library_path, "D:/Eagle/动画.library");
    let selected = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
    assert_eq!(
        eagle_path_label(&selected.library_path),
        "D:/Eagle/动画.library"
    );
    assert_eq!(
        model.files.error, "请先选择 EagleLibrary 目录。",
        "选中目录还不代表导入成功"
    );
    assert!(eagle_import_message(&selected, "copy").is_some());

    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("sideways".into())),
    );
    assert!(model.files.take_effects().is_empty());
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
    );
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::ImportEagle { library_path, mode, .. }] if library_path == "D:/Eagle/动画.library" && mode == "copy"
    ));
    assert_eq!(model.files.dialog, FileDialog::Closed);
    assert!(model.files.error.is_empty());
    let row = model.operation.shown().expect("导入进行中");
    assert_eq!(
        (row.label.as_str(), row.detail.as_str(), row.value),
        ("复制导入 Eagle", "转换 EagleLibrary", 24.0)
    );
    assert_eq!(model.admin.eagle_library_path, "D:/Eagle/动画.library");

    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("move".into())),
    );
    assert!(model.files.take_effects().is_empty(), "进行中不再提交");
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
    );
    assert!(
        model
            .input
            .take_platform_commands(WindowId(1), false)
            .is_empty(),
        "进行中不再打开目录对话框"
    );
}

/// 测试用导入结果：快照随便给一份，结果卡只读摘要和警告。
fn response(
    summary: EagleLibraryImportSummary,
    warnings: Vec<EagleLibraryImportWarning>,
) -> EagleLibraryImportResponse {
    EagleLibraryImportResponse {
        snapshot: crate::shell::data_load_tests::browser("repo"),
        summary,
        warnings,
    }
}

fn warning(kind: &str, reason: Option<&str>) -> EagleLibraryImportWarning {
    EagleLibraryImportWarning {
        warning_type: kind.into(),
        reason: reason.map(Into::into),
        ..Default::default()
    }
}

fn settled(summary: EagleLibraryImportSummary) -> ShellMessage {
    ShellMessage::Files(FilesMessage::EagleImportSettled {
        result: Ok(eagle_import_report(&response(summary, Vec::new()))),
    })
}

#[test]
fn eagle_import_report_keeps_the_summary_and_counts_every_warning() {
    let summary = EagleLibraryImportSummary {
        imported_files: 3,
        imported_directories: 2,
        imported_shortcuts: 9,
        ..Default::default()
    };
    let report = eagle_import_report(&response(
        summary.clone(),
        vec![
            warning("skippedSmartFolder", Some("条件为空")),
            warning("", None),
        ],
    ));
    assert_eq!(report.summary, summary);
    assert_eq!(report.warning_count, 2);
    assert_eq!(
        report.warnings,
        vec![
            EagleImportWarning {
                kind: "skippedSmartFolder".into(),
                text: "条件为空".into()
            },
            EagleImportWarning {
                kind: "warning".into(),
                text: String::new()
            },
        ]
    );

    let empty = eagle_import_report(&response(Default::default(), Vec::new()));
    assert_eq!(empty.summary, EagleLibraryImportSummary::default());
    assert_eq!(empty.warning_count, 0);
    assert!(empty.warnings.is_empty());
}

#[test]
fn eagle_warning_text_falls_back_in_order_and_stops_at_twenty() {
    let mut items: Vec<_> = (1..=21)
        .map(|index| warning("overflow", Some(&format!("第{index}条"))))
        .collect();
    items[0] = EagleLibraryImportWarning {
        asset_id: Some("asset".into()),
        name: Some("不该出现".into()),
        ..warning("primary", Some("有原因"))
    };
    items[1] = EagleLibraryImportWarning {
        asset_id: Some(" asset-9 ".into()),
        source_id: Some("src".into()),
        details: Some(serde_json::json!({ "folderId": "f-1" })),
        ..warning("fromFallback", Some("  "))
    };
    items[2] = EagleLibraryImportWarning {
        details: Some(serde_json::json!({ "folderId": "f-1", "reason": "条件" })),
        ..warning("", None)
    };
    items[3] = EagleLibraryImportWarning {
        name: Some("封面".into()),
        ..warning("missingThumbnail", None)
    };
    items[4] = EagleLibraryImportWarning {
        source_id: Some("src-2".into()),
        ..warning("", None)
    };
    let report = eagle_import_report(&response(Default::default(), items));
    assert_eq!(report.warning_count, 21);
    assert_eq!(report.warnings.len(), 20);
    let line = |kind: &str, text: &str| EagleImportWarning {
        kind: kind.into(),
        text: text.into(),
    };
    assert_eq!(report.warnings[0], line("primary", "有原因"));
    assert_eq!(report.warnings[1], line("fromFallback", " asset-9 "));
    assert_eq!(
        report.warnings[2],
        line("warning", r#"{"folderId":"f-1","reason":"条件"}"#)
    );
    assert_eq!(report.warnings[3], line("missingThumbnail", ""));
    assert_eq!(report.warnings[4], line("warning", "src-2"));
    assert_eq!(report.warnings[19].text, "第20条");
    assert!(report.warnings.iter().all(|item| item.text != "第21条"));
    assert!(
        report
            .warnings
            .iter()
            .all(|item| !item.text.contains("不该出现") && !item.text.contains("封面"))
    );
}

#[test]
fn eagle_result_follows_the_protocol_and_the_menu_reuses_the_folder_dialog() {
    let mut model = writable();
    assert!(
        ImportView::project(&model, TOOL_EAGLE_IMPORTER)
            .report
            .is_none()
    );
    let payload = response(
        EagleLibraryImportSummary {
            imported_files: 4,
            imported_directories: 1,
            ..Default::default()
        },
        vec![EagleLibraryImportWarning {
            name: Some("封面".into()),
            ..warning("missingThumbnail", Some("缺图"))
        }],
    );
    send(
        &mut model,
        ShellMessage::Files(FilesMessage::EagleImportSettled {
            result: Ok(eagle_import_report(&payload)),
        }),
    );
    let report = ImportView::project(&model, TOOL_EAGLE_IMPORTER)
        .report
        .expect("结果卡用响应里的字段");
    assert_eq!(
        (
            report.summary.imported_files,
            report.summary.imported_directories,
            report.summary.imported_shortcuts,
            report.warning_count
        ),
        (4, 1, 0, 1)
    );
    assert_eq!(
        report.warnings,
        vec![EagleImportWarning {
            kind: "missingThumbnail".into(),
            text: "缺图".into()
        }]
    );
    assert!(model.files.error.is_empty());

    send(
        &mut model,
        ShellMessage::Files(FilesMessage::EagleImportSettled {
            result: Err("库打不开".into()),
        }),
    );
    assert!(
        ImportView::project(&model, TOOL_EAGLE_IMPORTER)
            .report
            .is_none()
    );
    assert_eq!(model.files.error, "库打不开");

    model.files.take_effects();
    model.files.mutating = false;
    send(
        &mut model,
        ShellMessage::Files(FilesMessage::OpenEagle("copy".into())),
    );
    let request_id = dialog_id(&mut model);
    send(
        &mut model,
        ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: Vec::new(),
            failed: None,
        }),
    );
    assert!(model.admin.eagle_library_path.is_empty());
    assert!(model.files.take_effects().is_empty(), "取消选择不导入");

    send(
        &mut model,
        ShellMessage::Files(FilesMessage::OpenEagle("move".into())),
    );
    let request_id = dialog_id(&mut model);
    send(
        &mut model,
        ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: vec![" D:/菜单.library ".into()],
            failed: None,
        }),
    );
    assert_eq!(model.admin.eagle_library_path, "D:/菜单.library");
    assert!(
        ImportView::project(&model, TOOL_EAGLE_IMPORTER)
            .report
            .is_none(),
        "选中目录还不是导入结果"
    );
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::ImportEagle { library_path, mode, .. }] if library_path == "D:/菜单.library" && mode == "move"
    ));
}

#[test]
fn eagle_notice_and_copy_label_stay_honest() {
    let open = FileContext {
        repo_id: Some("repo".into()),
        writable: true,
        missing: false,
        trash: false,
        smart_folder: false,
        category_virtual: false,
    };
    assert_eq!(block_reason(&open, false), Some("当前目标不可导入。"));
    assert!(block_reason(&open, true).is_none());

    let mut model = writable();
    model.files.mutating = true;
    assert!(
        import_block_reason(&model).is_none(),
        "进行中不写成不能导入"
    );
    model.files.mutating = false;
    model.admin.eagle_library_path = "D:/Eagle/动画.library".into();

    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
    );
    let busy = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
    assert!(busy.busy && busy.notice.is_empty() && busy.reason.is_none());
    assert_eq!(
        (
            eagle_submit_label(&busy, "copy"),
            eagle_submit_label(&busy, "move")
        ),
        ("处理中...", "剪切导入")
    );
    send(&mut model, settled(Default::default()));
    let copied = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
    assert_eq!(copied.notice, "复制导入完成，目标目录：inbox");
    assert!(!copied.busy && copied.report.is_some());
    assert_eq!(eagle_submit_label(&copied, "copy"), "复制导入");

    let mut moved = writable();
    moved.files.current_path = "inbox/covers".into();
    moved.admin.eagle_library_path = "D:/Eagle/动画.library".into();
    send(
        &mut moved,
        ShellMessage::Admin(AdminMessage::RunEagleImport("move".into())),
    );
    assert_eq!(
        eagle_submit_label(&ImportView::project(&moved, TOOL_EAGLE_IMPORTER), "move"),
        "剪切导入"
    );
    send(&mut moved, settled(Default::default()));
    assert_eq!(
        ImportView::project(&moved, TOOL_EAGLE_IMPORTER).notice,
        "剪切导入完成，目标目录：inbox/covers"
    );

    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("move".into())),
    );
    assert!(
        ImportView::project(&model, TOOL_EAGLE_IMPORTER)
            .notice
            .is_empty(),
        "再次提交先清掉上一次"
    );
    send(
        &mut model,
        ShellMessage::Files(FilesMessage::EagleImportSettled {
            result: Err("库打不开".into()),
        }),
    );
    let failed = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
    assert!(failed.notice.is_empty());
    assert_eq!(failed.error, "库打不开");

    model.files.mutating = false;
    model.files.error.clear();
    model.files.current_path.clear();
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
    );
    send(&mut model, settled(Default::default()));
    assert_eq!(
        ImportView::project(&model, TOOL_EAGLE_IMPORTER).notice,
        "复制导入完成，目标目录：/"
    );

    model.files.mutating = false;
    model.files.current_path = "/".into();
    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::RunEagleImport("copy".into())),
    );
    send(
        &mut model,
        settled(EagleLibraryImportSummary {
            imported_files: 1,
            ..Default::default()
        }),
    );
    let root = ImportView::project(&model, TOOL_EAGLE_IMPORTER);
    assert_eq!(root.notice, "复制导入完成，目标目录：/");
    assert_eq!(
        root.report.map(|report| report.summary.imported_files),
        Some(1)
    );

    send(
        &mut model,
        ShellMessage::Admin(AdminMessage::ChooseEagleLibrary),
    );
    let request_id = dialog_id(&mut model);
    send(
        &mut model,
        ShellMessage::Input(InputMessage::FileDialogCompleted {
            request_id,
            paths: Vec::new(),
            failed: None,
        }),
    );
    assert_eq!(
        ImportView::project(&model, TOOL_EAGLE_IMPORTER).notice,
        "复制导入完成，目标目录：/",
        "取消选择不改已经写出的完成提示"
    );
    model.files.current_path = "inbox".into();
    send(&mut model, settled(Default::default()));
    assert_eq!(
        ImportView::project(&model, TOOL_EAGLE_IMPORTER).notice,
        "复制导入完成，目标目录：/",
        "没提交的成功结果不改写提示"
    );
}
