//! 筛选动作切换面板、打开命中结果、跨仓库切换与目录读取失败、快照分面的测试。

use super::*;

#[test]
fn filter_actions_switch_to_the_search_panel_only_when_they_apply() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Files;
    send(&mut model, InspectMessage::ToggleFilterBar);
    assert_eq!(
        model.workspace.panel,
        WorkspacePanel::Files,
        "开关筛选栏本身不切面板，标题栏另发切换"
    );
    send(&mut model, InspectMessage::SetMinimumRating(Some(2.0)));
    assert_eq!(model.workspace.panel, WorkspacePanel::Search);

    model.workspace.panel = WorkspacePanel::Files;
    send(
        &mut model,
        InspectMessage::SetAdvanced {
            field: AdvancedField::Metadata,
            value: "a=b".into(),
        },
    );
    assert_eq!(
        model.workspace.panel,
        WorkspacePanel::Files,
        "只改输入不切面板"
    );
    send(&mut model, InspectMessage::ApplyAdvanced);
    assert_eq!(model.workspace.panel, WorkspacePanel::Search);

    let mut locked = ShellViewModel::default();
    locked
        .workspace
        .present_repository(repository("repo", false));
    locked.workspace.panel = WorkspacePanel::Files;
    send(
        &mut locked,
        InspectMessage::ToggleFilter {
            key: FilterList::Formats,
            value: "png".into(),
        },
    );
    assert_eq!(
        locked.workspace.panel,
        WorkspacePanel::Files,
        "不可写仓库不切面板"
    );
    assert!(locked.inspect.filters.formats.is_empty());
}

#[test]
fn opening_a_hit_browses_its_folder_then_loads_the_asset() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Search;
    model.workspace.library_category = LibraryCategory::Recent;
    model.page = ShellPage::SelectedFile;
    model.inspect.results = vec![
        row("repo", "asset-1", "pics/cover.png"),
        row("repo", "", "pics/empty.png"),
    ];

    send(
        &mut model,
        InspectMessage::OpenHit {
            repo_id: "repo".into(),
            asset_id: "".into(),
        },
    );
    assert_eq!(
        model.workspace.panel,
        WorkspacePanel::Search,
        "缺素材 id 时留在搜索面板"
    );
    assert_eq!(model.inspect.search_error, "搜索结果没有素材 id");

    send(
        &mut model,
        InspectMessage::OpenHit {
            repo_id: "repo".into(),
            asset_id: "asset-1".into(),
        },
    );
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(
        model.workspace.library_category,
        LibraryCategory::All,
        "回到全部文件"
    );
    assert_eq!(model.page, ShellPage::FileList, "先关掉上一份预览");
    let browse = model
        .files
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            FilesEffect::Browse { repo_id, path, .. } => Some((repo_id, path)),
            _ => None,
        });
    assert_eq!(
        browse,
        Some(("repo".into(), "pics".into())),
        "先读命中所在目录"
    );
    assert!(
        !model
            .inspect
            .effects
            .iter()
            .any(|effect| matches!(effect, InspectEffect::LoadAsset { .. }))
    );

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "repo",
        "pics",
        &["pics/cover.png"],
    ))));
    assert_eq!(model.selected_path.as_deref(), Some("pics/cover.png"));
    assert_eq!(
        model.files.selected,
        ["pics/cover.png"],
        "目录到达后选中命中"
    );
    assert!(previewing(&model), "目录里的文件立刻打开预览，不等素材详情");
    assert!(matches!(
        model.inspect.effects.last(),
        Some(InspectEffect::LoadAsset { repo_id, asset_id }) if repo_id == "repo" && asset_id == "asset-1"
    ));
}

/// 打开搜索命中：目录快照里找到文件后立刻打开预览，不等选中素材。
/// 单击选中的同一条也要打开。目录还没读回来时预览保持收起。
#[test]
fn opening_a_search_file_shows_the_preview_before_the_detail() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Files;
    model.workspace.library_category = LibraryCategory::All;
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "repo",
        "pics",
        &["pics/cover.png"],
    ))));
    model.reduce(ShellMessage::Files(FilesMessage::ActivateRow(
        "pics/cover.png".into(),
    )));
    assert!(!previewing(&model), "单击只在右侧详情里看");

    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.results = vec![row("repo", "asset-1", "pics/cover.png")];
    send(
        &mut model,
        InspectMessage::OpenHit {
            repo_id: "repo".into(),
            asset_id: "asset-1".into(),
        },
    );
    assert_eq!(
        model.page,
        ShellPage::FileList,
        "先关掉预览，等命中所在目录"
    );
    assert!(!previewing(&model), "目录还没到，不换预览页");

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "repo",
        "pics",
        &["pics/cover.png"],
    ))));
    assert!(previewing(&model), "目录里的文件立刻换成预览页");
    assert_eq!(model.page, ShellPage::SelectedFile);
    assert_eq!(model.inspect.target_path.as_deref(), Some("pics/cover.png"));
    assert_eq!(model.files.selected, ["pics/cover.png"]);
    assert!(
        matches!(
            model.inspect.effects.last(),
            Some(InspectEffect::LoadAsset { repo_id, asset_id }) if repo_id == "repo" && asset_id == "asset-1"
        ),
        "素材详情仍在读，预览不等它"
    );
}

/// 命中在别的仓库时先换仓库，换仓库走完（启动页三步）以后才读
/// 命中所在的目录（带目录树，任务弹层是「读取文件树」），读到了选中命中并读详情。搜索在换仓库时清空。
#[test]
fn opening_a_hit_in_another_repository_switches_first_and_resets_search() {
    let mut model = shell();
    model.workspace.repositories.push(repository("other", true));
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.filter_bar_open = true;
    model.inspect.query = "封面".into();
    model.inspect.results = vec![row("other", "asset-9", "pics/cover.png")];
    send(
        &mut model,
        InspectMessage::OpenHit {
            repo_id: "other".into(),
            asset_id: "asset-9".into(),
        },
    );
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("other"));
    assert!(model.inspect.query.is_empty(), "切换资源库清空查询");
    assert!(!model.inspect.filter_bar_open, "切换资源库收起筛选栏");
    assert_eq!(
        model.workspace.startup.status,
        super::super::super::super::StartupStatus::Loading,
        "回到启动页"
    );
    assert_eq!(model.workspace.startup.step_label, "扫描资源库文件");
    assert!(
        !model
            .files
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Browse { .. })),
        "换仓库走完以前不读命中所在目录"
    );

    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced {
        generation,
        result: Ok(crate::shell::SyncCounts::default()),
    });
    model.reduce(ShellMessage::RepositorySnapshotLoaded {
        generation,
        result: Ok(snapshot("other", &[("pics/cover.png", "png", &[])])),
    });
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "other",
        "",
        &[],
    ))));
    assert_eq!(
        model.workspace.startup.status,
        super::super::super::super::StartupStatus::Ready
    );
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    let browse = model
        .files
        .take_effects()
        .into_iter()
        .find_map(|effect| match effect {
            FilesEffect::Browse { repo_id, path, .. } => Some((repo_id, path)),
            _ => None,
        });
    assert_eq!(
        browse,
        Some(("other".into(), "pics".into())),
        "换完仓库再读命中所在目录"
    );
    assert_eq!(
        model
            .operation
            .shown()
            .map(|row| (row.label.as_str(), row.detail.as_str())),
        Some(("读取文件树", "正在读取 pics"))
    );

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "other",
        "pics",
        &["pics/cover.png"],
    ))));
    assert_eq!(
        model.files.selected,
        ["pics/cover.png"],
        "目录到达后选中命中"
    );
    assert!(previewing(&model), "换完仓库、目录到达后立刻打开预览");
    assert!(matches!(
        model.inspect.effects.last(),
        Some(InspectEffect::LoadAsset { repo_id, asset_id }) if repo_id == "other" && asset_id == "asset-9"
    ));
}

/// 换仓库失败停在启动页：等着的命中不再打开，重试成功以后也不会突然跳过去。
#[test]
fn a_failed_switch_drops_the_pending_hit() {
    let mut model = shell();
    model.workspace.repositories.push(repository("other", true));
    model.inspect.results = vec![row("other", "asset-9", "pics/cover.png")];
    send(
        &mut model,
        InspectMessage::OpenHit {
            repo_id: "other".into(),
            asset_id: "asset-9".into(),
        },
    );
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySynced {
        generation,
        result: Err("拒绝访问。".into()),
    });
    assert_eq!(
        model.workspace.startup.status,
        super::super::super::super::StartupStatus::Error
    );
    model.files.take_effects();
    model.reduce(ShellMessage::StartupRetry);
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::WorkspaceListLoaded {
        generation,
        result: Ok(vec![
            snapshot("repo", &[]).repository,
            snapshot("other", &[]).repository,
        ]),
    });
    model.reduce(ShellMessage::RepositorySynced {
        generation,
        result: Ok(crate::shell::SyncCounts::default()),
    });
    model.reduce(ShellMessage::RepositorySnapshotLoaded {
        generation,
        result: Ok(snapshot("other", &[])),
    });
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "other",
        "",
        &[],
    ))));
    crate::shell::data_load_tests::answer_settings(&mut model);
    assert_eq!(
        model.workspace.startup.status,
        super::super::super::super::StartupStatus::Ready
    );
    assert!(
        !model
            .files
            .take_effects()
            .iter()
            .any(|effect| matches!(effect, FilesEffect::Browse { path, .. } if path == "pics")),
        "失败时放弃的命中不再打开"
    );
}

#[test]
fn a_superseded_folder_read_drops_the_pending_hit() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.results = vec![row("repo", "asset-1", "pics/cover.png")];
    send(
        &mut model,
        InspectMessage::OpenHit {
            repo_id: "repo".into(),
            asset_id: "asset-1".into(),
        },
    );
    // 文件列的点击在搜索归约之前就被消费，这里直接模拟另一处目录已经读完。
    model.files.loading = false;
    send(&mut model, InspectMessage::SetQuery("x".into()));
    model.inspect.take_effects();
    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "repo",
        "pics",
        &["pics/cover.png"],
    ))));
    assert!(
        !model
            .inspect
            .effects
            .iter()
            .any(|effect| matches!(effect, InspectEffect::LoadAsset { .. })),
        "被顶掉的命中不会在以后读到同一目录时突然打开"
    );

    let mut navigated = shell();
    navigated.workspace.panel = WorkspacePanel::Search;
    navigated.inspect.results = vec![row("repo", "asset-1", "pics/cover.png")];
    send(
        &mut navigated,
        InspectMessage::OpenHit {
            repo_id: "repo".into(),
            asset_id: "asset-1".into(),
        },
    );
    navigated.reduce(ShellMessage::OpenDirectory("docs".into()));
    navigated.inspect.take_effects();
    navigated.reduce(ShellMessage::FileBrowserLoaded(Ok(browser(
        "repo",
        "pics",
        &["pics/cover.png"],
    ))));
    assert!(
        !navigated
            .inspect
            .effects
            .iter()
            .any(|effect| matches!(effect, InspectEffect::LoadAsset { .. }))
    );
}

#[test]
fn a_failed_folder_read_still_opens_the_asset() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.results = vec![row("repo", "asset-1", "pics/cover.png")];
    send(
        &mut model,
        InspectMessage::OpenHit {
            repo_id: "repo".into(),
            asset_id: "asset-1".into(),
        },
    );
    model.reduce(ShellMessage::FileBrowserLoaded(Err("目录不可读".into())));
    assert!(
        matches!(model.inspect.effects.last(), Some(InspectEffect::LoadAsset { asset_id, .. }) if asset_id == "asset-1")
    );
}

#[test]
fn snapshots_feed_facets_and_switching_repository_resets_search() {
    let mut model = shell();
    model.workspace.repositories.push(repository("other", true));
    let generation = model.workspace.list_generation;
    model.reduce(ShellMessage::RepositorySnapshotLoaded {
        generation,
        result: Ok(snapshot("repo", &[("cover.png", "png", &["封面"])])),
    });
    assert_eq!(model.inspect.search_ui.facets_repo.as_deref(), Some("repo"));
    assert_eq!(model.inspect.search_ui.facets.len(), 1);
    assert_eq!(model.inspect.search_ui.facets[0].tags, ["封面"]);
    assert_eq!(model.inspect.search_ui.facets[0].extension, "png");

    model.inspect.query = "封面".into();
    model.inspect.filters.formats = vec!["png".into()];
    model.inspect.filter_bar_open = true;
    model.inspect.search_ui.draft.exclude_query = "草稿".into();
    model.reduce(ShellMessage::SelectWorkspaceRepository("repo".into()));
    assert_eq!(model.inspect.query, "封面", "选同一个仓库不清空");
    model.reduce(ShellMessage::SelectWorkspaceRepository("other".into()));
    assert!(model.inspect.query.is_empty());
    assert!(model.inspect.filters.formats.is_empty());
    assert!(!model.inspect.filter_bar_open);
    assert_eq!(
        model.inspect.search_ui.draft.exclude_query, "草稿",
        "输入框草稿和 Vue 一样留着"
    );
}
