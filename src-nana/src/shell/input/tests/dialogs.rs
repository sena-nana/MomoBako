//! 关闭确认、重新定位与挂载文件夹、下载目录、自定义缩略图与实时导入选择对话框的测试。

use super::*;

#[test]
fn close_confirmation_and_file_dialogs_do_not_pretend_the_host_finished() {
    let mut model = ShellViewModel::default();
    model.settings.close_behavior = "confirm".into();
    model.reduce(ShellMessage::WindowAction(
        crate::shell::WindowAction::Close,
    ));
    assert!(model.input.pending_close);
    assert_eq!(model.input.notice, "确认关闭 MomoBako？");
    assert!(
        model.input.host_requests.is_empty(),
        "确认框由浮层画，不排宿主请求"
    );
    model.reduce(ShellMessage::WindowAction(
        crate::shell::WindowAction::Close,
    ));
    assert!(model.input.host_requests.is_empty());
    assert!(close_prompt(&model).is_some());
    send(&mut model, InputMessage::ConfirmCloseAnswer(false));
    assert!(!model.input.pending_close);
    assert!(model.input.notice.is_empty());
    assert!(close_prompt(&model).is_none());
    send(&mut model, InputMessage::ConfirmCloseAnswer(true));
    assert_eq!(model.input.notice, "");

    model.settings.close_behavior = "quit".into();
    model.reduce(ShellMessage::WindowAction(
        crate::shell::WindowAction::Close,
    ));
    assert!(!model.input.pending_close);
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(
        commands.first(),
        Some(nana_ui_platform::host::WindowCommand::Close(_))
    ));

    model.dirty = true;
    model.reduce(ShellMessage::WindowAction(
        crate::shell::WindowAction::Close,
    ));
    assert!(model.input.pending_close);
    assert_eq!(model.input.notice, "有未保存的修改，确认关闭？");
    send(&mut model, InputMessage::ConfirmCloseAnswer(true));
    assert!(
        !model
            .input
            .take_platform_commands(nana_ui_platform::WindowId(1), false)
            .is_empty()
    );

    model.reduce(ShellMessage::Inspect(InspectMessage::SetComment(
        "未保存".into(),
    )));
    model.dirty = false;
    assert!(model.close_is_dirty());
    model.settings.close_behavior = "quit".into();
    model.reduce(ShellMessage::WindowAction(
        crate::shell::WindowAction::Close,
    ));
    assert!(model.input.pending_close);

    model.settings.close_behavior = "minimizeToTray".into();
    model.reduce(ShellMessage::WindowAction(
        crate::shell::WindowAction::Close,
    ));
    assert!(!model.input.pending_close);
    assert!(model.input.notice.is_empty());
    assert!(matches!(
        model.input.host_requests.last(),
        Some(HostRequest::Input(HostInputRequest::MinimizeToTray))
    ));
    assert!(
        model
            .input
            .take_platform_commands(nana_ui_platform::WindowId(1), false)
            .is_empty()
    );

    let mut acceptance = ShellViewModel::for_page(ShellPage::Settings);
    assert!(close_prompt(&acceptance).is_none());
    acceptance.input.pending_close = true;
    acceptance.input.notice = "确认关闭 MomoBako？".into();
    assert!(close_prompt(&acceptance).is_some());

    model.admin.external_message = "正在选择导出位置…".into();
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_EXPORT_ID,
            paths: Vec::new(),
            failed: None,
        },
    );
    assert!(model.admin.external_message.is_empty());
    assert!(model.admin.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_EXPORT_ID,
            paths: Vec::new(),
            failed: Some("Busy".into()),
        },
    );
    assert_eq!(model.admin.external_error, "导出失败：Busy");
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: 99,
            paths: vec!["x".into()],
            failed: None,
        },
    );
    model.input.queue_save_dialog();
    model.input.queue_plugin_dialog();
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert_eq!(commands.len(), 2);
    assert!(matches!(
        &commands[0],
        nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }
            if request.id == DIALOG_EXPORT_ID
                && request.title.as_deref() == Some("导出外部 API 连接 JSON")
                && request.file_name.as_deref() == Some("external-api.json")
                && request.filters.len() == 1
                && request.filters[0].name.as_ref() == "JSON"
                && request.filters[0].extensions.len() == 1
                && request.filters[0].extensions[0].as_ref() == "json"
    ));
    assert!(matches!(
        &commands[1],
        nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }
            if request.id == DIALOG_PLUGIN_ID
                && request.title.as_deref() == Some("选择 .momoplug 插件包")
                && request.filters.len() == 1
                && request.filters[0].name.as_ref() == "MomoBako 插件"
                && request.filters[0].extensions[0].as_ref() == "momoplug"
    ));
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_PLUGIN_ID,
            paths: vec![" ".into()],
            failed: None,
        },
    );
    assert!(model.admin.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_PLUGIN_ID,
            paths: vec!["plugin.momoplug".into()],
            failed: None,
        },
    );
    assert!(
        matches!(model.admin.take_effects().pop(), Some(crate::shell::admin::AdminEffect::Install(path)) if path == "plugin.momoplug")
    );
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_PLUGIN_ID,
            paths: Vec::new(),
            failed: Some("Unavailable".into()),
        },
    );
    assert_eq!(model.admin.action_error, "插件包选择失败：Unavailable");
}

#[test]
fn relocate_folder_dialog_queues_pick_folder_and_submits_only_a_chosen_path() {
    let mut model = finished_missing("repo");
    model.workspace.missing_error = "旧错误".into();
    begin_relocate_dialog(&mut model);
    assert!(!model.workspace.path_prompt);
    assert!(!model.workspace.relocating);
    assert!(model.workspace.missing_error.is_empty());
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert_eq!(commands.len(), 1);
    assert!(matches!(
        &commands[0],
        nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }
            if request.id == DIALOG_RELOCATE_ID
                && request.kind == FileDialogKind::PickFolder
                && request.title.as_deref() == Some("重定向资源库位置")
    ));

    let mut idle = ShellViewModel::default();
    idle.workspace.startup.finish();
    idle.workspace.repositories.push(missing_repository("repo"));
    begin_relocate_dialog(&mut idle);
    assert!(
        idle.input
            .take_platform_commands(nana_ui_platform::WindowId(1), false)
            .is_empty()
    );

    let mut busy = finished_missing("repo");
    busy.workspace.relocating = true;
    begin_relocate_dialog(&mut busy);
    assert!(busy.workspace.relocating);
    assert!(
        busy.input
            .take_platform_commands(nana_ui_platform::WindowId(1), false)
            .is_empty()
    );

    let mut cancelled = finished_missing("repo");
    cancelled.workspace.path_draft = "C:/old".into();
    send(
        &mut cancelled,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_RELOCATE_ID,
            paths: Vec::new(),
            failed: None,
        },
    );
    assert!(!cancelled.workspace.relocating);
    assert!(
        !cancelled
            .workspace
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, WorkspaceEffect::RelocateRepository { .. }))
    );
    send(
        &mut cancelled,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_RELOCATE_ID,
            paths: vec![" ".into()],
            failed: None,
        },
    );
    assert!(!cancelled.workspace.relocating);
    assert!(cancelled.workspace.take_effects().is_empty());

    let mut failed = finished_missing("repo");
    failed.workspace.path_draft = "C:/old".into();
    send(
        &mut failed,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_RELOCATE_ID,
            paths: vec!["D:/library".into()],
            failed: Some("Busy".into()),
        },
    );
    assert_eq!(failed.workspace.missing_error, "文件夹选择失败：Busy");
    assert!(!failed.workspace.relocating);
    assert!(failed.workspace.take_effects().is_empty());

    let mut chosen = finished_missing("repo");
    send(
        &mut chosen,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_RELOCATE_ID,
            paths: vec!["D:/library".into()],
            failed: None,
        },
    );
    assert!(chosen.workspace.relocating);
    assert!(matches!(
        chosen.workspace.take_effects().as_slice(),
        [WorkspaceEffect::RelocateRepository { repo_id, path }] if repo_id == "repo" && path == "D:/library"
    ));
}

#[test]
fn attach_folder_dialog_submits_a_chosen_path_and_ignores_cancel() {
    let mut model = ShellViewModel::default();
    // 先打开添加菜单，选「本地文件夹」后才弹系统文件夹对话框。
    model.admin.plugins = vec![super::super::support::test_source_plugin(
        "momobako.local-filesystem",
        "本地文件夹",
        &["localRootPath"],
    )];
    model.reduce(ShellMessage::Sidebar(
        super::super::super::SidebarMessage::ShowRepositoryAddMenu,
    ));
    assert!(
        model
            .input
            .take_platform_commands(nana_ui_platform::WindowId(1), false)
            .is_empty(),
        "添加菜单本身不弹对话框"
    );
    model.reduce(ShellMessage::Sidebar(
        super::super::super::SidebarMessage::SelectRepositoryBackend(
            "momobako.local-filesystem".into(),
        ),
    ));
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(
        commands.as_slice(),
        [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }]
            if request.id == DIALOG_ATTACH_ID
                && request.kind == FileDialogKind::PickFolder
                && request.title.as_deref() == Some("选择资源库文件夹")
                && request.filters.is_empty()
    ));

    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_ATTACH_ID,
            paths: Vec::new(),
            failed: None,
        },
    );
    assert!(model.sidebar.take_effects().is_empty());
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_ATTACH_ID,
            paths: vec![" ".into()],
            failed: None,
        },
    );
    assert!(model.sidebar.take_effects().is_empty(), "空白路径不提交");

    let mut eagle = ShellViewModel::default();
    eagle.admin.plugins = vec![super::super::support::test_source_plugin(
        "momobako.source.eagle-library",
        "Eagle Library",
        &[],
    )];
    eagle.reduce(ShellMessage::Sidebar(
        super::super::super::SidebarMessage::SelectRepositoryBackend(
            "momobako.source.eagle-library".into(),
        ),
    ));
    let eagle_commands = eagle
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(
        matches!(
            eagle_commands.as_slice(),
            [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }]
                if request.id == DIALOG_ATTACH_ID
                    && request.kind == FileDialogKind::PickFolder
                    && request.title.as_deref() == Some("选择 Eagle Library 目录")
                    && request.filters.is_empty()
        ),
        "Eagle 添加资源库的标题"
    );

    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_ATTACH_ID,
            paths: vec!["D:/library".into()],
            failed: Some("Busy".into()),
        },
    );
    let failure = model
        .status
        .failure()
        .expect("弹层已经关了，文件夹选择失败进状态区");
    assert_eq!(
        (failure.source, failure.message.as_str()),
        (FailureSource::Repository, "文件夹选择失败：Busy")
    );
    assert!(model.sidebar.take_effects().is_empty());

    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_ATTACH_ID,
            paths: vec!["D:/library".into()],
            failed: None,
        },
    );
    assert!(matches!(
        model.sidebar.take_effects().as_slice(),
        [SidebarEffect::AttachRepository { path }] if path == "D:/library"
    ));
}

#[test]
fn download_folder_calls_plugin_only_after_a_directory_is_chosen() {
    let mut model = ShellViewModel::default();
    let begin = InputMessage::BeginDownloadFolder {
        plugin_id: "momobako.netease.source".into(),
        method: "media.downloadTrackPackage".into(),
        payload: serde_json::json!({"songId": 9}),
        repository_id: Some("repo".into()),
        progress: download_progress(),
    };
    send(&mut model, begin.clone());
    assert!(model.admin.take_effects().is_empty());
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(
        commands.as_slice(),
        [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }]
            if request.id == DIALOG_DOWNLOAD_ID
                && request.kind == FileDialogKind::PickFolder
                && request.title.as_deref() == Some("选择下载目录")
    ));

    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_DOWNLOAD_ID,
            paths: Vec::new(),
            failed: None,
        },
    );
    assert!(model.admin.take_effects().is_empty());
    assert!(model.input.pending_download.is_none());

    send(&mut model, begin);
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_DOWNLOAD_ID,
            paths: vec!["D:/downloads".into()],
            failed: Some("Busy".into()),
        },
    );
    assert!(model.admin.take_effects().is_empty());
    assert!(model.input.pending_download.is_none());

    assert!(model.operation.shown().is_none(), "没选目录不出进度行");
    send(
        &mut model,
        InputMessage::BeginDownloadFolder {
            plugin_id: "momobako.netease.source".into(),
            method: "media.downloadTrackPackage".into(),
            payload: serde_json::json!({"songId": 9}),
            repository_id: Some("repo".into()),
            progress: download_progress(),
        },
    );
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_DOWNLOAD_ID,
            paths: vec!["D:/downloads".into()],
            failed: None,
        },
    );
    let run = match model.admin.take_effects().as_slice() {
        [
            crate::shell::admin::AdminEffect::CallPlugin {
                method,
                payload,
                repository_id,
                origin,
                ..
            },
        ] => {
            assert_eq!(method, "media.downloadTrackPackage");
            assert_eq!(payload["songId"], 9);
            assert_eq!(payload["destination"]["kind"], "localFolder");
            assert_eq!(payload["destination"]["path"], "D:/downloads");
            assert_eq!(repository_id.as_deref(), Some("repo"));
            match origin {
                crate::shell::admin::PluginCallOrigin::Download(run) => run.clone(),
                other => panic!("下载到本地的结果要回到下载的收尾：{other:?}"),
            }
        }
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(run.progress.subject, "夜曲");
    assert_eq!(
        run.destination,
        crate::shell::admin::source_download::DownloadDestination::LocalFolder(
            "D:/downloads".into()
        )
    );
    // 选好目录以后任务弹层出「下载单曲 · 正在下载 夜曲」36%，不确定进度。
    let row = model.operation.shown().cloned().expect("下载的进度行");
    assert_eq!(
        (
            row.label.as_str(),
            row.detail.as_str(),
            row.value,
            row.indeterminate
        ),
        ("下载单曲", "正在下载 夜曲", 36.0, true)
    );
    send_admin(
        &mut model,
        crate::shell::admin::AdminMessage::DownloadFinished {
            run,
            result: Err("网络断开".into()),
        },
    );
    assert!(model.operation.shown().is_none(), "下载失败时这一行取消");
    assert_eq!(
        model.files.error, "下载单曲「夜曲」失败：网络断开",
        "失败的原因写进文件列"
    );
}

#[test]
fn custom_thumbnail_dialog_is_id_six_and_writes_or_clears() {
    assert_eq!(DIALOG_THUMBNAIL_ID, 6);
    assert_ne!(DIALOG_REPOSITORY_EXPORT_ID, DIALOG_EXPORT_ID);
    assert_ne!(DIALOG_REPOSITORY_EXPORT_ID, DIALOG_THUMBNAIL_ID);
    assert_ne!(DIALOG_THUMBNAIL_ID, DIALOG_EXPORT_ID);
    assert_ne!(DIALOG_THUMBNAIL_ID, DIALOG_PLUGIN_ID);
    assert_ne!(DIALOG_THUMBNAIL_ID, DIALOG_RELOCATE_ID);
    assert_ne!(DIALOG_THUMBNAIL_ID, DIALOG_ATTACH_ID);
    assert_ne!(DIALOG_THUMBNAIL_ID, DIALOG_DOWNLOAD_ID);

    let mut model = ready("repo");
    send(
        &mut model,
        InputMessage::BeginThumbnailFile {
            repo_id: "repo".into(),
            path: "a.png".into(),
            kind: "file".into(),
        },
    );
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(
        commands.as_slice(),
        [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }] if request.id == DIALOG_THUMBNAIL_ID
    ));
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_THUMBNAIL_ID,
            paths: Vec::new(),
            failed: None,
        },
    );
    assert!(model.files.take_effects().is_empty());

    send(
        &mut model,
        InputMessage::BeginThumbnailFile {
            repo_id: "repo".into(),
            path: "a.png".into(),
            kind: "file".into(),
        },
    );
    let _ = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    send(
        &mut model,
        InputMessage::FileDialogCompleted {
            request_id: DIALOG_THUMBNAIL_ID,
            paths: vec!["C:\\cover.png".into()],
            failed: None,
        },
    );
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::MutateThumbnail { action, source_path, image_bytes, path, .. }]
            if action == "save" && source_path.as_deref() == Some("C:\\cover.png") && image_bytes.is_none() && path == "a.png"
    ));

    crate::host_bridge::copy_text("C:\\clip.jpg");
    send(
        &mut model,
        InputMessage::PasteThumbnail {
            repo_id: "repo".into(),
            path: "a.png".into(),
            kind: "file".into(),
        },
    );
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::MutateThumbnail { action, source_path, .. }] if action == "save" && source_path.as_deref() == Some("C:\\clip.jpg")
    ));

    crate::host_bridge::copy_text("data:image/png;base64,AQID");
    send(
        &mut model,
        InputMessage::PasteThumbnail {
            repo_id: "repo".into(),
            path: "a.png".into(),
            kind: "file".into(),
        },
    );
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::MutateThumbnail { action, image_bytes, source_path, .. }]
            if action == "save" && source_path.is_none() && image_bytes.as_deref() == Some(&[1, 2, 3][..])
    ));

    crate::host_bridge::copy_text("notes.txt");
    send(
        &mut model,
        InputMessage::PasteThumbnail {
            repo_id: "repo".into(),
            path: "a.png".into(),
            kind: "file".into(),
        },
    );
    assert!(model.files.take_effects().is_empty());
    assert!(model.files.error.contains("剪贴板"));

    send(
        &mut model,
        InputMessage::ClearThumbnail {
            repo_id: "repo".into(),
            path: "a.png".into(),
            kind: "file".into(),
        },
    );
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::MutateThumbnail { action, source_path, image_bytes, .. }]
            if action == "clear" && source_path.is_none() && image_bytes.is_none()
    ));
}

/// 实况导入排队系统选择器。取消、空白和失败都不提交；选中的路径才进入导入。
#[test]
fn live_import_picker_submits_only_a_chosen_path() {
    use super::super::support::{DIALOG_IMPORT_ARCHIVE_ID, DIALOG_IMPORT_FOLDER_ID};
    use crate::shell::files::{FileDialog, FilesMessage};

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
    model.reduce(ShellMessage::Files(FilesMessage::OpenDialog(
        FileDialog::Import,
    )));
    assert_eq!(model.files.dialog, FileDialog::Closed);
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(
        commands.as_slice(),
        [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }]
            if request.id == DIALOG_IMPORT_FOLDER_ID && request.title.as_deref() == Some("选择导入文件夹")
    ));
    model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
        request_id: DIALOG_IMPORT_FOLDER_ID,
        paths: Vec::new(),
        failed: None,
    }));
    assert!(model.files.take_effects().is_empty(), "取消不导入");
    model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
        request_id: DIALOG_IMPORT_FOLDER_ID,
        paths: vec!["  ".into()],
        failed: None,
    }));
    assert!(model.files.take_effects().is_empty(), "空白不导入");
    model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
        request_id: DIALOG_IMPORT_FOLDER_ID,
        paths: vec!["D:/素材".into()],
        failed: Some("Busy".into()),
    }));
    assert!(model.files.take_effects().is_empty(), "对话框失败不导入");
    model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
        request_id: DIALOG_IMPORT_FOLDER_ID,
        paths: vec![" D:/素材 ".into()],
        failed: None,
    }));
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::Import { sources, .. }] if sources == &["D:/素材".to_string()]
    ));

    model.files.mutating = false;
    model.reduce(ShellMessage::Files(FilesMessage::OpenDialog(
        FileDialog::ImportArchive,
    )));
    let commands = model
        .input
        .take_platform_commands(nana_ui_platform::WindowId(1), false);
    assert!(matches!(
        commands.as_slice(),
        [nana_ui_platform::host::WindowCommand::OpenFileDialog { request, .. }] if request.id == DIALOG_IMPORT_ARCHIVE_ID
    ));
    model.reduce(ShellMessage::Input(InputMessage::FileDialogCompleted {
        request_id: DIALOG_IMPORT_ARCHIVE_ID,
        paths: vec!["D:/归档.zip".into()],
        failed: None,
    }));
    assert!(matches!(
        model.files.take_effects().as_slice(),
        [FilesEffect::ImportArchive { archive_path, .. }] if archive_path == "D:/归档.zip"
    ));
}
