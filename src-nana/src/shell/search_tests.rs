//! 搜索与筛选状态机测试。
//!
//! 分支来自 Vue `search.ts`、`useSearchUi.ts`、`useWorkspaceFilterShortcuts.ts` 和
//! `openSearchHit`。不启动仓库服务，也不写用户目录。

use std::collections::BTreeMap;

use serde_json::Value;

use crate::backend::services::repository::{
    AssetSummary, FileBrowserEntry, FileBrowserSnapshot, RepositoryBackendSummary, RepositoryOverview, RepositorySnapshot,
    RepositoryStructureCacheState, RepositorySummary,
};

use super::super::super::workspace::{LibraryCategory, WorkspacePanel, WorkspaceRepository};
use super::super::super::{FilesEffect, ShellMessage, ShellPage, ShellViewModel};
use super::super::{InspectEffect, InspectMessage, InspectState};
use super::{AdvancedField, FilterList, MatchMode, MetadataInput, SearchRow, SortDirection};

fn row(repo_id: &str, asset_id: &str, path: &str) -> SearchRow {
    SearchRow {
        repo_id: repo_id.into(),
        asset_id: asset_id.into(),
        path: path.into(),
        filename: path.rsplit('/').next().unwrap_or(path).into(),
        repo_name: "图库".into(),
        tags: Vec::new(),
        metadata: BTreeMap::new(),
    }
}

fn repository(repo_id: &str, writable: bool) -> WorkspaceRepository {
    WorkspaceRepository {
        repo_id: repo_id.into(),
        name: format!("库 {repo_id}"),
        path: format!("C:/{repo_id}"),
        status: "ready".into(),
        backend_plugin_id: "filesystem".into(),
        capabilities: if writable { vec!["write".into()] } else { Vec::new() },
        cache_required: false,
        cache_status: String::new(),
    }
}

/// 启动完成、活动仓库是可写的 `repo` 的壳层。
fn shell() -> ShellViewModel {
    let mut model = ShellViewModel::default();
    model.workspace.present_repository(repository("repo", true));
    model.repository_id = Some("repo".into());
    model
}

fn send(model: &mut ShellViewModel, message: InspectMessage) {
    model.reduce(ShellMessage::Inspect(message));
}

fn last_search(state: &mut InspectState) -> super::SearchRequestDraft {
    match state.take_effects().pop() {
        Some(InspectEffect::Search { request, .. }) => request,
        other => panic!("没有搜索请求：{other:?}"),
    }
}

#[test]
fn search_criteria_follow_the_vue_filter_rules() {
    let mut state = InspectState::default();
    state.results = vec![row("repo", "old", "old.png")];
    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    assert!(state.results.is_empty(), "没有条件时清空结果");
    assert!(state.effects.is_empty(), "没有条件时不发请求");

    state.reduce(true, Some("repo"), InspectMessage::SetQuery("peach".into()));
    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    let request = last_search(&mut state);
    assert_eq!(request.query, "peach");
    assert!(request.repo_id.is_none(), "只有查询时是全局搜索");
    assert!(request.match_mode.is_none());

    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Colors, value: " red ".into() });
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Shapes, value: "square".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetMatchMode(MatchMode::Or));
    let request = last_search(&mut state);
    assert!(request.metadata_filters.iter().any(|(key, value)| key == "color" && value == "red"));
    assert!(request.metadata_filters.iter().any(|(key, value)| key == "shape" && value == "square"));
    assert_eq!(request.match_mode.as_deref(), Some("or"));
    assert_eq!(request.repo_id.as_deref(), Some("repo"), "筛选生效时收进当前仓库");

    state.results = vec![row("repo", "kept", "kept.png")];
    state.search_generation = 4;
    state.reduce(true, None, InspectMessage::SearchFinished { generation: 3, result: Ok(Vec::new()) });
    assert_eq!(state.results.len(), 1, "过期结果不覆盖");
    state.reduce(true, None, InspectMessage::SearchFinished { generation: 4, result: Err("搜索失败".into()) });
    assert_eq!(state.results.len(), 1, "失败保留上一批结果");
    assert_eq!(state.search_error, "搜索失败");

    let mut missing = InspectState::default();
    missing.results = vec![row("repo", "kept", "kept.png")];
    missing.reduce(true, None, InspectMessage::ToggleFilter { key: FilterList::Tags, value: "peach".into() });
    assert!(missing.results.is_empty(), "筛选却没有活动仓库时清空结果");
    assert!(missing.effects.is_empty());

    let mut locked = InspectState::default();
    locked.reduce(false, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Tags, value: "peach".into() });
    assert!(locked.filters.tags.is_empty(), "不可写仓库不改条件");
    assert!(locked.effects.is_empty());
}

#[test]
fn toggling_checks_the_raw_value_and_stores_the_trimmed_one() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Colors, value: "red".into() });
    // Vue 用原值判断是否已选：带空白的同名值不在列表里，只会再加一次（去重后不变）。
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Colors, value: " red ".into() });
    assert_eq!(state.filters.colors, ["red"]);
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Colors, value: "red".into() });
    assert!(state.filters.colors.is_empty());
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Tags, value: "   ".into() });
    assert!(state.filters.tags.is_empty(), "空白值不改条件");
}

#[test]
fn color_and_shape_inputs_submit_like_add_and_clear_the_field() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::SetMetadataInput { key: MetadataInput::Colors, value: " 青色 ".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetMetadataInput { key: MetadataInput::Shapes, value: "方形".into() });
    assert!(state.filters.colors.is_empty(), "输入不直接生效");
    state.reduce(true, Some("repo"), InspectMessage::SubmitMetadataInput(MetadataInput::Colors));
    assert_eq!(state.filters.colors, ["青色"]);
    assert_eq!(state.search_ui.draft.color, "", "提交后清空颜色输入");
    assert_eq!(state.search_ui.draft.shape, "方形", "形状输入不受影响");
    assert!(state.search_ui.reveal_panel, "提交后切到搜索面板");
    let request = last_search(&mut state);
    assert_eq!(request.metadata_filters, [("color".into(), "青色".into())]);

    // 同名再提交一次是取消，和点芯片一样。
    state.reduce(true, Some("repo"), InspectMessage::SetMetadataInput { key: MetadataInput::Colors, value: "青色".into() });
    state.reduce(true, Some("repo"), InspectMessage::SubmitMetadataInput(MetadataInput::Colors));
    assert!(state.filters.colors.is_empty());

    let mut blank = InspectState::default();
    blank.reduce(true, Some("repo"), InspectMessage::SubmitMetadataInput(MetadataInput::Shapes));
    assert!(blank.effects.is_empty(), "空输入不提交");
    let mut locked = InspectState::default();
    locked.reduce(false, Some("repo"), InspectMessage::SetMetadataInput { key: MetadataInput::Shapes, value: "方形".into() });
    locked.reduce(false, Some("repo"), InspectMessage::SubmitMetadataInput(MetadataInput::Shapes));
    assert!(locked.filters.shapes.is_empty());
    assert_eq!(locked.search_ui.draft.shape, "方形", "不可写时不清空输入");
}

#[test]
fn clear_keeps_the_query_and_resets_every_input() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::SetQuery("peach".into()));
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilterBar);
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilter { key: FilterList::Tags, value: "draft".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetMetadataInput { key: MetadataInput::Colors, value: "红".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::Limit, value: "20".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetSortDirection(SortDirection::Desc));
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::ClearFilters);
    assert_eq!(state.query, "peach");
    assert!(state.filter_bar_open, "清除不收起筛选栏");
    assert!(state.filters.tags.is_empty());
    assert_eq!(state.search_ui.draft, super::SearchDraft::default(), "清除清空所有输入并把排序方向改回升序");
    let request = last_search(&mut state);
    assert_eq!(request.query, "peach");
    assert!(request.repo_id.is_none());

    state.reduce(true, Some("repo"), InspectMessage::ToggleFilterBar);
    assert!(!state.filter_bar_open);
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilterBar);
    state.reduce(true, Some("repo"), InspectMessage::CloseFilterBar);
    assert!(!state.filter_bar_open, "关闭只收起筛选栏");
    state.reduce(true, Some("repo"), InspectMessage::CloseFilterBar);
    assert!(!state.filter_bar_open, "再点关闭不会重新打开");
}

#[test]
fn advanced_inputs_parse_like_vue_and_apply_together() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::ExcludeQuery, value: " nope ".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::ExcludePaths, value: r"a\b,/c/".into() });
    state.reduce(true, Some("repo"), InspectMessage::ApplyAdvanced);
    let request = last_search(&mut state);
    assert_eq!(request.exclude_query.as_deref(), Some("nope"));
    assert_eq!(request.exclude_path_prefixes, ["a/b", "c"]);
    assert!(request.repo_id.is_none(), "排除关键词和排除路径不把搜索收进当前仓库");

    for (field, value) in [
        (AdvancedField::Metadata, "artist=momo, =x, key="),
        (AdvancedField::Number, "rating=1..5,nan=foo..bar,width=..10"),
        (AdvancedField::Date, "added=2020-01-01..2021-01-01"),
        (AdvancedField::ExcludeTags, "旧，草稿,旧"),
        (AdvancedField::SortField, " name "),
        (AdvancedField::Limit, "0"),
    ] {
        state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field, value: value.into() });
    }
    state.reduce(true, Some("repo"), InspectMessage::SetSortDirection(SortDirection::Desc));
    assert!(state.effects.is_empty(), "输入和排序方向只改草稿");
    state.reduce(true, Some("repo"), InspectMessage::ApplyAdvanced);
    let request = last_search(&mut state);
    assert_eq!(request.metadata_filters, [("artist".into(), "momo".into())]);
    assert_eq!(request.number_filters.len(), 2);
    assert_eq!(request.number_filters[0].min.as_deref(), Some("1"));
    assert_eq!(request.number_filters[0].max.as_deref(), Some("5"));
    assert_eq!(request.date_filters[0].from.as_deref(), Some("2020-01-01"));
    assert_eq!(request.exclude_tags, ["旧", "草稿"]);
    assert_eq!(request.sort_field.as_deref(), Some("name"));
    assert_eq!(request.sort_direction.as_deref(), Some("desc"));
    assert!(request.limit.is_none(), "数量 0 视为未设置");
    assert_eq!(request.repo_id.as_deref(), Some("repo"));

    state.reduce(true, Some("repo"), InspectMessage::SetMinimumRating(Some(0.0)));
    assert!(state.filters.min_rating.is_none(), "0 星视为全部");
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::SetMinimumRating(Some(3.0)));
    let request = last_search(&mut state);
    assert_eq!(request.min_rating, Some(3.0));
}

#[test]
fn shortcut_rewrites_metadata_and_sort_and_clears_the_excludes() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::ExcludeTags, value: "旧".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::Limit, value: "5".into() });
    state.reduce(true, Some("repo"), InspectMessage::SetAdvanced { field: AdvancedField::ExcludeQuery, value: "试听".into() });
    state.reduce(true, Some("repo"), InspectMessage::ApplyAdvanced);
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::ApplyShortcut {
        metadata: "libraryKind=asmr\nlisteningStatus=listening".into(),
        sort_field: "metadata.lastListenedAt".into(),
        sort_direction: SortDirection::Desc,
    });
    let draft = &state.search_ui.draft;
    assert_eq!(draft.metadata, "libraryKind=asmr\nlisteningStatus=listening");
    assert_eq!(draft.sort_field, "metadata.lastListenedAt");
    assert_eq!(draft.sort_direction, SortDirection::Desc);
    assert!(draft.exclude_tags.is_empty() && draft.limit.is_empty(), "快捷方式清掉排除标签和数量输入");
    assert_eq!(draft.exclude_query, "试听", "其余输入保留");
    assert!(state.filters.exclude_tags.is_empty());
    assert!(state.filters.limit.is_none());
    assert_eq!(state.filters.exclude_query, "试听");
    let request = last_search(&mut state);
    assert_eq!(request.metadata_filters.len(), 2);
    assert_eq!(request.sort_field.as_deref(), Some("metadata.lastListenedAt"));
    assert_eq!(request.sort_direction.as_deref(), Some("desc"));

    let mut locked = InspectState::default();
    locked.reduce(false, Some("repo"), InspectMessage::ApplyShortcut {
        metadata: "kind=book".into(),
        sort_field: "added".into(),
        sort_direction: SortDirection::Desc,
    });
    assert!(locked.filters.metadata_filters.is_empty(), "不可写仓库不应用快捷方式");
}

#[test]
fn active_filter_count_matches_vue() {
    let mut state = InspectState::default();
    assert_eq!(state.active_filter_count(), 0);
    state.filters.formats = vec!["png".into()];
    state.filters.colors = vec!["红色".into(), "#3fa796".into()];
    state.filters.min_rating = Some(3.0);
    state.filters.sort_field = "name".into();
    state.filters.exclude_query = "草稿".into();
    state.filters.exclude_path_prefixes = "tmp".into();
    state.filters.exclude_number_filters = "width=0..1".into();
    assert_eq!(state.active_filter_count(), 5, "排除关键词、路径和数值范围不计数");
    assert!(state.filters.has_active_filters());
}

#[test]
fn search_query_waits_250ms_before_running() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::SetQuery("peach".into()));
    assert!(state.effects.is_empty());
    assert!(!state.poll_own(249, true, Some("repo")));
    assert!(state.effects.is_empty());
    assert!(state.poll_own(1, true, Some("repo")));
    assert_eq!(last_search(&mut state).query, "peach");
}

#[test]
fn filter_actions_switch_to_the_search_panel_only_when_they_apply() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Files;
    send(&mut model, InspectMessage::ToggleFilterBar);
    assert_eq!(model.workspace.panel, WorkspacePanel::Files, "开关筛选栏本身不切面板，标题栏另发切换");
    send(&mut model, InspectMessage::SetMinimumRating(Some(2.0)));
    assert_eq!(model.workspace.panel, WorkspacePanel::Search);

    model.workspace.panel = WorkspacePanel::Files;
    send(&mut model, InspectMessage::SetAdvanced { field: AdvancedField::Metadata, value: "a=b".into() });
    assert_eq!(model.workspace.panel, WorkspacePanel::Files, "只改输入不切面板");
    send(&mut model, InspectMessage::ApplyAdvanced);
    assert_eq!(model.workspace.panel, WorkspacePanel::Search);

    let mut locked = ShellViewModel::default();
    locked.workspace.present_repository(repository("repo", false));
    locked.workspace.panel = WorkspacePanel::Files;
    send(&mut locked, InspectMessage::ToggleFilter { key: FilterList::Formats, value: "png".into() });
    assert_eq!(locked.workspace.panel, WorkspacePanel::Files, "不可写仓库不切面板");
    assert!(locked.inspect.filters.formats.is_empty());
}

#[test]
fn opening_a_hit_browses_its_folder_then_loads_the_asset() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Search;
    model.workspace.library_category = LibraryCategory::Recent;
    model.page = ShellPage::SelectedFile;
    model.inspect.results = vec![row("repo", "asset-1", "pics/cover.png"), row("repo", "", "pics/empty.png")];

    send(&mut model, InspectMessage::OpenHit { repo_id: "repo".into(), asset_id: "".into() });
    assert_eq!(model.workspace.panel, WorkspacePanel::Search, "缺素材 id 时留在搜索面板");
    assert_eq!(model.inspect.search_error, "搜索结果没有素材 id");

    send(&mut model, InspectMessage::OpenHit { repo_id: "repo".into(), asset_id: "asset-1".into() });
    assert_eq!(model.workspace.panel, WorkspacePanel::Files);
    assert_eq!(model.workspace.library_category, LibraryCategory::All, "回到全部文件");
    assert_eq!(model.page, ShellPage::FileList, "先关掉上一份预览");
    let browse = model.files.take_effects().into_iter().find_map(|effect| match effect {
        FilesEffect::Browse { repo_id, path, .. } => Some((repo_id, path)),
        _ => None,
    });
    assert_eq!(browse, Some(("repo".into(), "pics".into())), "先读命中所在目录");
    assert!(!model.inspect.effects.iter().any(|effect| matches!(effect, InspectEffect::LoadAsset { .. })));

    model.reduce(ShellMessage::FileBrowserLoaded(Ok(browser("repo", "pics", &["pics/cover.png"]))));
    assert_eq!(model.selected_path.as_deref(), Some("pics/cover.png"));
    assert_eq!(model.files.selected, ["pics/cover.png"], "目录到达后选中命中");
    assert!(matches!(
        model.inspect.effects.last(),
        Some(InspectEffect::LoadAsset { repo_id, asset_id }) if repo_id == "repo" && asset_id == "asset-1"
    ));
}

#[test]
fn opening_a_hit_in_another_repository_switches_first_and_resets_search() {
    let mut model = shell();
    model.workspace.repositories.push(repository("other", true));
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.filter_bar_open = true;
    model.inspect.query = "封面".into();
    model.inspect.results = vec![row("other", "asset-9", "cover.png")];
    send(&mut model, InspectMessage::OpenHit { repo_id: "other".into(), asset_id: "asset-9".into() });
    assert_eq!(model.workspace.active_repo_id.as_deref(), Some("other"));
    assert!(model.inspect.query.is_empty(), "切换资源库清空查询");
    assert!(!model.inspect.filter_bar_open, "切换资源库收起筛选栏");
    let browse = model.files.take_effects().into_iter().find_map(|effect| match effect {
        FilesEffect::Browse { repo_id, path, .. } => Some((repo_id, path)),
        _ => None,
    });
    assert_eq!(browse, Some(("other".into(), String::new())), "根目录文件的父目录是根");
}

#[test]
fn a_failed_folder_read_still_opens_the_asset() {
    let mut model = shell();
    model.workspace.panel = WorkspacePanel::Search;
    model.inspect.results = vec![row("repo", "asset-1", "pics/cover.png")];
    send(&mut model, InspectMessage::OpenHit { repo_id: "repo".into(), asset_id: "asset-1".into() });
    model.reduce(ShellMessage::FileBrowserLoaded(Err("目录不可读".into())));
    assert!(matches!(model.inspect.effects.last(), Some(InspectEffect::LoadAsset { asset_id, .. }) if asset_id == "asset-1"));
}

#[test]
fn snapshots_feed_facets_and_switching_repository_resets_search() {
    let mut model = shell();
    model.workspace.repositories.push(repository("other", true));
    model.reduce(ShellMessage::RepositorySnapshotLoaded(Ok(snapshot("repo", &[("cover.png", "png", &["封面"])]))));
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
    assert_eq!(model.inspect.search_ui.draft.exclude_query, "草稿", "输入框草稿和 Vue 一样留着");
}

fn browser(repo_id: &str, path: &str, files: &[&str]) -> FileBrowserSnapshot {
    let entries = files
        .iter()
        .map(|file| FileBrowserEntry {
            path: (*file).into(),
            name: file.rsplit('/').next().unwrap_or(file).into(),
            kind: "file".into(),
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
        })
        .collect::<Vec<_>>();
    FileBrowserSnapshot {
        repo_id: repo_id.into(),
        root_path: format!("C:/{repo_id}"),
        backend_plugin_id: "filesystem".into(),
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

fn snapshot(repo_id: &str, assets: &[(&str, &str, &[&str])]) -> RepositorySnapshot {
    let summary = RepositorySummary {
        repo_id: repo_id.into(),
        name: format!("库 {repo_id}"),
        path: format!("C:/{repo_id}"),
        backend: RepositoryBackendSummary {
            plugin_id: "filesystem".into(),
            kind: "local".into(),
            name: "本地".into(),
            capabilities: vec!["write".into()],
        },
        status: "ready".into(),
        asset_count: assets.len() as i64,
        updated_at: String::new(),
        local_cache: None,
        authentication: None,
    };
    RepositorySnapshot {
        repository: summary,
        folder_label: String::new(),
        folders: Vec::new(),
        assets: assets
            .iter()
            .map(|(path, extension, tags)| AssetSummary {
                asset_id: (*path).into(),
                repo_id: repo_id.into(),
                path: (*path).into(),
                filename: (*path).into(),
                extension: (*extension).into(),
                size_bytes: 0,
                size_label: "0 B".into(),
                status: "ready".into(),
                modified_at: String::new(),
                last_accessed_at: None,
                version: 1,
                tags: tags.iter().map(|tag| tag.to_string()).collect(),
                thumbnail_path: None,
                hardlink_group_id: None,
                hardlink_state: None,
                is_virtual: false,
                provider_id: None,
                provider_item_id: None,
                source_payload: None,
                local_absolute_path: None,
            })
            .collect(),
        playlists: Vec::new(),
        quick_access: Vec::new(),
        tag_groups: Vec::new(),
        metadata_fields: Vec::new(),
        recent_revision_count: 0,
        overview: RepositoryOverview {
            total_size_bytes: 0,
            total_size_label: "0 B".into(),
            file_count: assets.len() as i64,
            folder_count: 0,
            trash_count: 0,
            readme_content: None,
        },
    }
}

#[allow(dead_code)]
fn metadata(pairs: &[(&str, Value)]) -> BTreeMap<String, Value> {
    pairs.iter().map(|(key, value)| (key.to_string(), value.clone())).collect()
}
