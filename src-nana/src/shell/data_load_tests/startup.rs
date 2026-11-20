//! 启动等待设置包与播放类型、失败与崩溃时仍完成启动、插件列表、设置页读取与会话恢复的测试。

use super::*;

/// 首屏启动在离开启动页之前等设置包：读完首屏目录停在第 4 步、换上说明，读设置包；插件列表
/// 到了再等播放器类型，都回来才进工作区。进工作区时播放集分组已经能新建，不会先写一句「没有可用的播放插件类型」。
#[test]
fn startup_waits_for_the_settings_bundle_and_player_types() {
    let mut model = ShellViewModel::default();
    launch(&mut model, REPO);
    let effects = admin_effects(&mut model);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle)),
        "读完首屏目录读设置包：{effects:?}"
    );
    assert!(model.admin.loading_settings);
    assert_eq!(
        RouteKey::of(&model),
        RouteKey::Startup,
        "设置包回来以前留在启动页"
    );
    let text = StartupView::project(&model).text;
    assert_eq!(
        (text.title.as_str(), text.meta.as_str()),
        ("读取首屏目录", "第 4 / 4 步")
    );
    assert_eq!(text.detail, "加载应用配置、插件设置和首屏辅助数据。");

    model.reduce(bundle(vec![plugin(
        "momobako.player.audio",
        serde_json::json!({}),
    )]));
    assert!(
        admin_effects(&mut model)
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadPlaylistPlayers))
    );
    assert_eq!(
        RouteKey::of(&model),
        RouteKey::Startup,
        "插件列表到了，还等播放器类型"
    );

    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(
        vec![audio_player()],
    )));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
    let playlists = SidebarView::project(&model).playlists;
    assert!(!playlists.create_locked, "进工作区时就能新建播放集");
    assert_eq!(playlists.hint, Some("还没有播放集。"));
}

/// 设置包读失败时吞掉错误：插件列表没换、不读播放器类型，启动照样结束，错误留在插件面板。
#[test]
fn a_failed_settings_bundle_still_finishes_the_startup() {
    let mut model = ShellViewModel::default();
    launch(&mut model, REPO);
    admin_effects(&mut model);
    model.reduce(ShellMessage::Admin(AdminMessage::SettingsBundleLoaded {
        plugins: Err("读取插件目录失败".into()),
        hooks: Ok(Vec::new()),
        api: Err("读取插件目录失败".into()),
        external: Err("读取插件目录失败".into()),
    }));
    assert!(
        !admin_effects(&mut model)
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadPlaylistPlayers))
    );
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
}

/// 设置包的任务 panic（插件宿主出错之类）：NanaUI 的任务线程不接 panic，结果消息永远不来，首屏启动会一直停在
/// 第 4 步。派发层（`task_guard::spawn`，照产品的失败消息）接住 panic，按五份都失败交回：启动照常结束，原因留在插件
/// 面板，加载日志照常写完。
#[test]
fn a_panicking_settings_bundle_task_still_finishes_the_startup() {
    let mut model = ShellViewModel::default();
    launch(&mut model, REPO);
    admin_effects(&mut model);
    let failed = crate::dispatch::admin::failed_bundle_message;
    model.reduce(crate::dispatch::task_guard::tests::run_guarded(
        "设置页数据",
        failed,
        || panic!("插件宿主没有响应"),
    ));
    assert!(
        !admin_effects(&mut model)
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadPlaylistPlayers)),
        "插件列表没换，不读播放器类型"
    );
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(RouteKey::of(&model), RouteKey::Files);
    assert_eq!(
        model.admin.load_error,
        "设置页数据任务异常退出：插件宿主没有响应"
    );
    let last = model
        .workspace
        .startup
        .logs
        .last()
        .map(|log| log.message.as_str());
    assert_eq!(last, Some("首屏启动流程完成。"));
}

/// 播放器类型的任务 panic 也一样：按失败交回，启动照常结束，失败写进状态区。
#[test]
fn a_panicking_player_type_task_still_finishes_the_startup() {
    let mut model = ShellViewModel::default();
    launch(&mut model, REPO);
    admin_effects(&mut model);
    model.reduce(bundle(Vec::new()));
    let failed = crate::dispatch::admin::players_failed;
    model.reduce(crate::dispatch::task_guard::tests::run_guarded(
        "播放器类型",
        failed,
        || panic!("播放器清单解析出错"),
    ));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("无法读取播放器类型：播放器类型任务异常退出：播放器清单解析出错")
    );
}

/// 播放器类型读失败也进工作区：失败写进状态区。
#[test]
fn a_failed_player_type_read_still_finishes_the_startup() {
    let mut model = ShellViewModel::default();
    launch(&mut model, REPO);
    admin_effects(&mut model);
    model.reduce(bundle(Vec::new()));
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Err(
        "插件宿主没有响应".into()
    )));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(
        model
            .status
            .failure()
            .map(|failure| failure.message.as_str()),
        Some("无法读取播放器类型：插件宿主没有响应")
    );
}

/// 没有资源库的启动也读设置包（添加资源库的来源列表要用插件清单），等它回来才落到空库页。
#[test]
fn an_empty_library_still_reads_the_settings_bundle() {
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(Vec::new()),
    });
    assert_eq!(model.workspace.startup.status, StartupStatus::Loading);
    assert_eq!(RouteKey::of(&model), RouteKey::Startup);
    assert!(
        admin_effects(&mut model)
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle))
    );
    model.reduce(bundle(Vec::new()));
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(Vec::new())));
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    assert_eq!(model.page, ShellPage::EmptyRepository);
    assert_eq!(RouteKey::of(&model), RouteKey::Empty);
}

/// 缺失仓库的启动等完设置包才落到缺失页，侧栏这时按缺失绑定（导航锁住、写修复提示）。
#[test]
fn a_missing_repository_lands_after_the_settings_bundle() {
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(vec![summary("gone", "missing")]),
    });
    assert_eq!(RouteKey::of(&model), RouteKey::Startup);
    admin_effects(&mut model);
    model.reduce(bundle(Vec::new()));
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(Vec::new())));
    assert_eq!(
        model.workspace.main_region(),
        crate::shell::MainRegion::MissingRepository
    );
    assert_eq!(RouteKey::of(&model), RouteKey::Missing);
    assert_eq!(model.sidebar.bound_repo_id(), Some("gone"));
    assert!(
        SidebarView::project(&model).nav.locked,
        "缺失仓库锁住侧栏导航"
    );
}

/// 首屏启动读完首屏目录、只在等设置包的这段，外面来的结构更新照常静默刷新（读完摘要以后就处理结构更新），
/// 读首屏目录以前的照旧忽略。
#[test]
fn structure_updates_while_waiting_for_settings_refresh_silently() {
    let structure = || {
        ShellMessage::Host(crate::shell::host_events::HostMessage::StructureUpdated {
            repo_id: REPO.into(),
            reason: "watch".into(),
        })
    };
    let silent = |model: &mut ShellViewModel| {
        model.workspace.take_effects().iter().any(|effect| matches!(effect, WorkspaceEffect::LoadSnapshotSilent { repo_id } if repo_id == REPO))
    };
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(vec![summary(REPO, "ready")]),
    });
    model.reduce(structure());
    assert!(!silent(&mut model), "同步以前的结构更新不管");

    let mut waiting = ShellViewModel::default();
    launch(&mut waiting, REPO);
    waiting.workspace.take_effects();
    waiting.reduce(structure());
    assert!(silent(&mut waiting), "等设置包时照常静默刷新");
    assert_eq!(
        waiting.workspace.startup.status,
        StartupStatus::Loading,
        "还在等设置包"
    );
}

/// 插件列表到了（启动、刷新、启停、安装、删除）就重读播放器类型；读回以后侧栏能新建播放集，
/// 插件类型的播放集能播放，新建对话框默认选第一个类型。
#[test]
fn plugin_lists_lead_to_player_types() {
    let mut model = started(REPO);
    admin_effects(&mut model);
    model.reduce(bundle(vec![plugin(
        "momobako.player.audio",
        serde_json::json!({}),
    )]));
    let effects = admin_effects(&mut model);
    assert!(
        matches!(effects.as_slice(), [AdminEffect::LoadPlaylistPlayers]),
        "插件列表到了应读播放器类型：{effects:?}"
    );
    assert!(
        SidebarView::project(&model).playlists.create_locked,
        "播放器类型读回之前不能新建播放集"
    );

    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(
        vec![audio_player()],
    )));
    assert_eq!(
        model.player.contributions.len(),
        1,
        "播放器认得插件登记的类型"
    );
    assert_eq!(
        model.selected_new_playlist_player_type_id.as_deref(),
        Some("momobako.playlist.audio-sequence")
    );
    assert!(
        !SidebarView::project(&model).playlists.create_locked,
        "读回播放器类型以后可以新建播放集"
    );

    model.reduce(ShellMessage::Admin(AdminMessage::PluginsReplaced(Ok(
        Vec::new(),
    ))));
    assert!(
        admin_effects(&mut model)
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadPlaylistPlayers)),
        "启停、安装、删除以后也重读"
    );
    model.reduce(ShellMessage::Admin(AdminMessage::PluginsReplaced(Err(
        "失败".into(),
    ))));
    assert!(
        admin_effects(&mut model).is_empty(),
        "插件操作失败时列表没换，不重读"
    );
}

/// 侧栏底部的「设置」：进设置页，读设置包和应用设置。
#[test]
fn opening_settings_reads_the_bundle_and_app_settings() {
    let mut model = started(REPO);
    admin_effects(&mut model);
    model.reduce(ShellMessage::OpenSettings);
    assert_eq!(model.page, ShellPage::Settings);
    let effects = admin_effects(&mut model);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle))
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadAppSettings))
    );
}

/// 缺失仓库的「打开来源设置」：进设置页并展开来源插件的设置，
/// 不再走宿主副作用。
#[test]
fn source_cache_issue_opens_the_source_plugin_settings() {
    let mut model = ShellViewModel::default();
    let generation = model.workspace.prepare_initial_list();
    let mut remote = summary("remote", "missing");
    remote.backend.plugin_id = "momobako.netease.source".into();
    remote.local_cache = Some(RepositoryLocalCacheStatus {
        required: true,
        path: None,
        status: "unconfigured".into(),
    });
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(vec![remote]),
    });
    model.reduce(bundle(vec![plugin(
        "momobako.netease.source",
        serde_json::json!({}),
    )]));
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(Vec::new())));
    assert_eq!(
        model.workspace.main_region(),
        crate::shell::MainRegion::MissingRepository
    );
    admin_effects(&mut model);
    model.reduce(ShellMessage::MissingOpenSourceSettings);
    assert_eq!(model.page, ShellPage::Settings);
    assert_eq!(
        model.admin.active_settings_plugin_id.as_deref(),
        Some("momobako.netease.source")
    );
    let effects = admin_effects(&mut model);
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, AdminEffect::LoadSettingsBundle))
    );
    assert!(effects.iter().any(
        |effect| matches!(effect, AdminEffect::LoadConfig(id) if id == "momobako.netease.source")
    ));
    assert!(
        !model
            .workspace
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, WorkspaceEffect::RefreshRepositories { .. }))
    );
}

/// 启动时恢复存下的播放会话：播放集列表和播放器类型都读回以后才读它的详情。首屏启动在等播放器类型，
/// 侧栏的播放集列表先到也不读详情；播放器类型回来时启动结束、读存下的播放集详情。
#[test]
fn startup_restores_the_stored_session_after_players_arrive() {
    let dir =
        std::env::temp_dir().join(format!("momobako-data-load-restore-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("临时目录");
    let sessions = dir.join("sessions.json");
    let stored = serde_json::json!({
        "repo-a": {
            "repoId": "repo-a",
            "playlistId": "pl-1",
            "playerTypeId": "momobako.playlist.audio-sequence",
            "currentItemId": "a",
            "currentTimeMs": 61000,
            "durationMs": 180000,
            "mode": "listLoop",
            "volume": 0.5,
            "isPlaying": false,
        }
    });
    std::fs::write(&sessions, stored.to_string()).expect("写会话");
    let mut model = ShellViewModel::default();
    model.player.load_files(
        &dir.join("missing-settings.json"),
        &sessions,
        &dir.join("missing-prefs.json"),
    );
    launch(&mut model, REPO);
    model.reduce(bundle(vec![plugin(
        "momobako.player.audio",
        serde_json::json!({}),
    )]));
    model.reduce(sidebar_playlists(REPO, vec![playlist("pl-1")]));
    let restores = |model: &mut ShellViewModel| {
        model
            .player
            .take_effects()
            .into_iter()
            .filter(|effect| matches!(effect, PlayerEffect::RestoreDetail { .. }))
            .count()
    };
    assert_eq!(restores(&mut model), 0, "插件类型的播放器还没读回");
    assert_eq!(
        model.workspace.startup.status,
        StartupStatus::Loading,
        "首屏启动在等播放器类型"
    );
    model.reduce(ShellMessage::PlaylistPlayersLoaded(Ok(
        vec![audio_player()],
    )));
    assert_eq!(
        restores(&mut model),
        1,
        "播放器类型读回以后读存下的播放集详情"
    );
    assert_eq!(model.workspace.startup.status, StartupStatus::Ready);
    let _ = std::fs::remove_dir_all(dir);
}
