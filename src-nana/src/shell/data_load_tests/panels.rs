//! 侧栏播放集、新建播放集、日志面板、运行中任务与仓库绑定后读取动作和硬链接的测试。

use super::*;

/// 启动绑定侧栏时读播放集列表；读回的列表同时交给播放器：读成员索引，文件右键有「加入播放列表」。
/// 换了仓库的旧结果不写。
#[test]
fn the_sidebar_playlist_list_reaches_the_player() {
    let mut model = started(REPO);
    let effects = model.sidebar.take_effects();
    assert!(
        effects.iter().any(
            |effect| matches!(effect, SidebarEffect::LoadPlaylists { repo_id } if repo_id == REPO)
        ),
        "启动后侧栏要读播放集：{effects:?}"
    );
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(
        vec![audio_player()],
    )));
    model.reduce(sidebar_playlists(REPO, vec![playlist("pl-1")]));
    assert_eq!(model.sidebar.playlists.len(), 1);
    assert_eq!(
        model.player.playlists.len(),
        1,
        "播放器拿到同一份播放集列表"
    );
    assert!(model.player.take_effects().iter().any(
        |effect| matches!(effect, PlayerEffect::LoadMemberships { repo_id } if repo_id == REPO)
    ));
    let actions = model
        .player
        .membership_actions("file", "mp3", "asset-1", false);
    assert_eq!(
        actions
            .iter()
            .map(|action| action.label.as_str())
            .collect::<Vec<_>>(),
        ["加入 早晨"],
        "文件右键有加入播放列表"
    );

    model.reduce(sidebar_playlists("repo-old", Vec::new()));
    assert_eq!(model.player.playlists.len(), 1, "别的仓库的旧结果不写");
}

/// 新建播放集：对话框打开时清空名称、默认选第一个类型；读回的列表进侧栏和播放器，并接着点开
/// 新建的那个。删除以后点开的那个不在了，回到文件面板。
#[test]
fn a_created_playlist_is_listed_and_opened() {
    let mut model = started(REPO);
    model.sidebar.take_effects();
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(
        vec![audio_player()],
    )));
    model.reduce(ShellMessage::NewPlaylistNameChanged("上次没建完".into()));
    model.reduce(ShellMessage::OpenPlaylistDialog);
    assert!(model.playlist_dialog_open);
    assert!(model.new_playlist_name.is_empty(), "每次打开都清空名称");
    assert_eq!(
        model.selected_new_playlist_player_type_id.as_deref(),
        Some("momobako.playlist.audio-sequence")
    );

    model.reduce(ShellMessage::NewPlaylistNameChanged("早晨".into()));
    model.reduce(ShellMessage::CreatePlaylist);
    assert!(model
        .sidebar
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, SidebarEffect::CreatePlaylist { repo_id, name, .. } if repo_id == REPO && name == "早晨")));
    assert!(model.playlist_dialog_open, "创建结果回来以前对话框留着");
    model.reduce(ShellMessage::PlaylistCreated {
        repo_id: REPO.into(),
        result: Ok((vec![playlist("pl-new")], Some("pl-new".into()))),
    });
    assert!(
        !model.playlist_dialog_open && !model.playlist_creating,
        "创建成功才关"
    );
    assert_eq!(
        model
            .sidebar
            .playlists
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["pl-new"]
    );
    assert_eq!(model.player.playlists.len(), 1);
    assert_eq!(
        model.workspace.panel,
        WorkspacePanel::Playlist,
        "新建以后点开它"
    );
    assert!(model
        .sidebar
        .take_effects()
        .iter()
        .any(|effect| matches!(effect, SidebarEffect::LoadPlaylistDetail { playlist_id, .. } if playlist_id == "pl-new")));

    model.reduce(ShellMessage::PlaylistsLoaded {
        repo_id: REPO.into(),
        result: Ok(Vec::new()),
    });
    assert!(model.sidebar.playlists.is_empty());
    assert!(model.sidebar.active_playlist_id.is_none());
    assert_eq!(
        model.workspace.panel,
        WorkspacePanel::Files,
        "点开的播放集删掉以后回到文件面板"
    );

    model.reduce(ShellMessage::PlaylistsLoaded {
        repo_id: REPO.into(),
        result: Err("磁盘已满".into()),
    });
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("播放集操作失败：磁盘已满")
    );
}

/// 新建播放集失败：对话框留着，原因写在对话框里，不进状态区；处理中点取消、遮罩或 Escape 都不关，
/// 失败以后可以改了再建，也可以取消。
#[test]
fn a_failed_playlist_creation_keeps_the_dialog_with_the_error() {
    use crate::shell::view_part_overlay::OverlayKey;
    let mut model = started(REPO);
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(
        vec![audio_player()],
    )));
    model.reduce(ShellMessage::OpenPlaylistDialog);
    model.reduce(ShellMessage::NewPlaylistNameChanged("早晨".into()));
    model.reduce(ShellMessage::CreatePlaylist);
    model.sidebar.take_effects();

    model.reduce(ShellMessage::ClosePlaylistDialog);
    model.reduce(ShellMessage::Sidebar(SidebarMessage::Gap(
        crate::shell::GapMessage::Escape,
    )));
    assert!(model.playlist_dialog_open, "处理中不能关");
    model.reduce(ShellMessage::CreatePlaylist);
    assert!(model.sidebar.take_effects().is_empty(), "处理中不重复提交");

    model.reduce(ShellMessage::PlaylistCreated {
        repo_id: REPO.into(),
        result: Err("磁盘已满".into()),
    });
    assert!(
        model.playlist_dialog_open && !model.playlist_creating,
        "失败时对话框留着"
    );
    assert_eq!(model.playlist_dialog_error, "磁盘已满");
    assert!(
        model.status.failure().is_none(),
        "失败写在对话框里，不进状态区"
    );
    assert_eq!(OverlayKey::of(&model), Some(OverlayKey::PlaylistCreate));

    model.reduce(ShellMessage::CreatePlaylist);
    assert!(
        model.playlist_dialog_error.is_empty(),
        "再次提交时清掉上一次的原因"
    );
    model.reduce(ShellMessage::PlaylistCreated {
        repo_id: REPO.into(),
        result: Err("磁盘已满".into()),
    });
    model.reduce(ShellMessage::ClosePlaylistDialog);
    assert!(!model.playlist_dialog_open, "失败以后可以取消");
    model.reduce(ShellMessage::OpenPlaylistDialog);
    assert!(
        model.playlist_dialog_error.is_empty(),
        "重新打开时不带上一次的原因"
    );
}

/// 侧栏「日志」切到日志面板：每次都读最近 200 条历史日志。
/// 读的时候手上还没有日志就写「正在加载系统日志」，读回以后按时间再 id 降序整份换上。
#[test]
fn opening_the_logs_panel_reads_history() {
    let mut model = started(REPO);
    admin_effects(&mut model);
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    assert!(
        matches!(
            admin_effects(&mut model).as_slice(),
            [AdminEffect::LoadLogs]
        ),
        "切到日志面板应读历史日志"
    );
    let view = LogsView::project(&model);
    assert!(view.loading, "读的时候写正在加载");
    assert!(view.empty.is_none(), "读的时候不写空状态");

    let records = vec![
        log_record("log-1", "2026-10-08T07:51:00Z"),
        log_record("log-3", "2026-10-08T07:53:00Z"),
        log_record("log-2", "2026-10-08T07:53:00Z"),
    ];
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records,
        next_cursor: None,
    })));
    assert_eq!(
        model
            .admin
            .logs
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["log-3", "log-2", "log-1"]
    );
    let view = LogsView::project(&model);
    assert!(!view.loading);
    assert_eq!(view.rows.len(), 3);

    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Files));
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    assert!(
        matches!(
            admin_effects(&mut model).as_slice(),
            [AdminEffect::LoadLogs]
        ),
        "再切回来再读一次"
    );
    assert!(
        !LogsView::project(&model).loading,
        "手上已经有日志时不挡住列表"
    );
    model.reduce(ShellMessage::LogsLoaded(Err("拒绝访问".into())));
    assert!(!model.admin.logs_loading);
    assert_eq!(model.admin.logs.len(), 3, "读失败保留手上的日志");
}

/// 日志页「清空日志」：点下去以后按钮禁用并换成加载图标，只清空持久化文件；成功后
/// 直接把手上的日志清空，不再重读。失败时日志留着，原因写进状态区。
#[test]
fn clearing_logs_empties_the_list_without_reading_again() {
    let mut model = started(REPO);
    model.reduce(ShellMessage::SetWorkspacePanel(WorkspacePanel::Logs));
    admin_effects(&mut model);
    let records = vec![
        log_record("log-1", "2026-10-08T07:51:00Z"),
        log_record("log-2", "2026-10-08T07:52:00Z"),
    ];
    model.reduce(ShellMessage::LogsLoaded(Ok(SystemLogPage {
        records,
        next_cursor: None,
    })));
    assert!(
        !LogsView::project(&model).toolbar.clear_disabled,
        "有日志时可以清空"
    );

    model.reduce(ShellMessage::ClearLogs);
    assert!(
        matches!(
            admin_effects(&mut model).as_slice(),
            [AdminEffect::ClearLogs]
        ),
        "只排下清空，不排读取"
    );
    let toolbar = LogsView::project(&model).toolbar;
    assert!(
        toolbar.clear_disabled && toolbar.clearing,
        "清空中按钮禁用、换成加载图标"
    );
    model.reduce(ShellMessage::ClearLogs);
    assert!(admin_effects(&mut model).is_empty(), "清空中再点不重复清空");

    model.reduce(ShellMessage::Admin(AdminMessage::LogsCleared(Err(
        "拒绝访问".into(),
    ))));
    assert_eq!(model.admin.logs.len(), 2, "清空失败保留手上的日志");
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("清空系统日志失败：拒绝访问")
    );
    assert!(!LogsView::project(&model).toolbar.clearing);

    model.reduce(ShellMessage::ClearLogs);
    admin_effects(&mut model);
    model.reduce(ShellMessage::Admin(AdminMessage::LogsCleared(Ok(()))));
    assert!(model.admin.logs.is_empty(), "清空成功直接清掉列表");
    assert!(
        admin_effects(&mut model).is_empty(),
        "清空成功不再读历史日志"
    );
    let view = LogsView::project(&model);
    assert!(view.toolbar.clear_disabled, "没有日志时不能再清空");
    assert_eq!(view.empty.map(|(title, _)| title), Some("还没有系统日志"));
}

/// 宿主观察到的运行中任务整份换上：任务弹层的行是仓库操作加这些任务，侧栏「任务」的计数是行数；
/// 任务结束后宿主交来空列表，计数回到只剩仓库操作。
#[test]
fn running_tasks_feed_the_popover_and_the_footer_count() {
    let mut model = started(REPO);
    assert_eq!(SidebarView::project(&model).footer.tasks, 0);
    let task = crate::backend::services::repository::TaskProgressSnapshot {
        task_id: "momobako.task.7".into(),
        protocol_id: "momobako.entry.copy".into(),
        status: "running".into(),
        phase: None,
        label: None,
        current: None,
        total: None,
        percent: None,
        error: None,
        updated_at: "2026-10-08T08:00:00.100Z".into(),
    };
    model.reduce(ShellMessage::TaskProgressLoaded(vec![task.clone()]));
    let rows = model.task_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].label, "复制文件",
        "没有标题时按协议写，不写泛泛的「进度」，也不露协议 id"
    );
    assert_eq!(
        SidebarView::project(&model).footer.tasks,
        1,
        "计数是弹层的行数"
    );

    // 后更新的任务排在前面：快照的更新时间是 RFC 3339，按毫秒比。
    let later = crate::backend::services::repository::TaskProgressSnapshot {
        task_id: "momobako.task.8".into(),
        label: Some("同步资源库".into()),
        updated_at: "2026-10-08T08:00:00.250Z".into(),
        ..task.clone()
    };
    model.reduce(ShellMessage::TaskProgressLoaded(vec![task, later]));
    let labels = model
        .task_rows()
        .into_iter()
        .map(|row| row.label)
        .collect::<Vec<_>>();
    assert_eq!(labels, ["同步资源库", "复制文件"], "任务自己报了标题就用它");
    model.reduce(ShellMessage::TaskProgressLoaded(
        model.task_progress[..1].to_vec(),
    ));

    model.reduce(ShellMessage::Sidebar(SidebarMessage::SidebarTreeLoaded {
        repo_id: REPO.into(),
        result: Ok(Default::default()),
    }));
    model.reduce(ShellMessage::Sidebar(SidebarMessage::RefreshFolderTree));
    assert!(
        model.operation.shown().is_some(),
        "刷新文件夹树是一行仓库操作"
    );
    assert_eq!(
        SidebarView::project(&model).footer.tasks,
        2,
        "仓库操作也算一行"
    );

    model.reduce(ShellMessage::TaskProgressLoaded(Vec::new()));
    assert_eq!(
        SidebarView::project(&model).footer.tasks,
        1,
        "任务结束后只剩仓库操作"
    );
}

/// 启动或换仓库绑定侧栏时，在后台读仓库动作和硬链接候选：
/// 读回的动作让侧栏多出「动作」入口；候选在没有别的对话框时弹出确认，后台读取失败只记日志。
/// 同一个仓库再绑一次不重读。
#[test]
fn binding_a_repository_reads_actions_and_hardlinks() {
    let mut model = started(REPO);
    let admin = admin_effects(&mut model);
    assert!(
        admin.iter().any(
            |effect| matches!(effect, AdminEffect::LoadActions { repo_id } if repo_id == REPO)
        ),
        "绑定仓库要读动作：{admin:?}"
    );
    let files = model.files.take_effects();
    let epoch = files
        .iter()
        .find_map(|effect| match effect {
            FilesEffect::CheckHardlinks { repo_id, epoch } if repo_id == REPO => Some(*epoch),
            _ => None,
        })
        .expect("绑定仓库要查硬链接候选");

    assert_eq!(SidebarView::project(&model).nav.actions, 0);
    model.reduce(ShellMessage::Admin(AdminMessage::ActionsLoaded {
        repo_id: REPO.into(),
        result: Ok(vec![repository_action("act-1")]),
    }));
    assert_eq!(
        SidebarView::project(&model).nav.actions,
        1,
        "读回动作后侧栏有「动作」入口"
    );

    model.reduce(ShellMessage::Files(FilesMessage::HardlinksChecked {
        epoch,
        result: Err("索引被占用".into()),
    }));
    assert!(model.files.error.is_empty(), "后台读取失败不写进文件列表");
    let prompt = HardlinkPrompt {
        id: "c-1".into(),
        new_path: "b.png".into(),
        existing_path: "a.png".into(),
        size_label: "1 KB".into(),
    };
    model.reduce(ShellMessage::Files(FilesMessage::HardlinksChecked {
        epoch,
        result: Ok(vec![prompt]),
    }));
    assert_eq!(model.files.dialog, FileDialog::Hardlink, "有候选就弹出确认");

    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySnapshotLoaded {
        generation,
        result: Ok(snapshot(REPO)),
    });
    assert!(
        !admin_effects(&mut model)
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadActions { .. })),
        "同一个仓库的摘要不重读动作"
    );
}
