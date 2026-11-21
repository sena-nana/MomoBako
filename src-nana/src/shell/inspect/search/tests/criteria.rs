//! 搜索条件、勾选与输入、清空、高级字段、快捷方式、筛选计数与查询防抖的测试。

use super::*;

#[test]
fn search_criteria_follow_the_filter_rules() {
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

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Colors,
            value: " red ".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Shapes,
            value: "square".into(),
        },
    );
    let request = last_search(&mut state);
    assert!(
        request
            .metadata_filters
            .iter()
            .any(|(key, value)| key == "color" && value == "red")
    );
    assert!(
        request
            .metadata_filters
            .iter()
            .any(|(key, value)| key == "shape" && value == "square")
    );
    assert_eq!(
        request.repo_id.as_deref(),
        Some("repo"),
        "筛选生效时收进当前仓库"
    );

    state.results = vec![row("repo", "kept", "kept.png")];
    state.search_generation = 4;
    state.reduce(
        true,
        None,
        InspectMessage::SearchFinished {
            generation: 3,
            result: Ok(Vec::new()),
        },
    );
    assert_eq!(state.results.len(), 1, "过期结果不覆盖");
    state.reduce(
        true,
        None,
        InspectMessage::SearchFinished {
            generation: 4,
            result: Err("搜索失败".into()),
        },
    );
    assert_eq!(state.results.len(), 1, "失败保留上一批结果");
    assert_eq!(state.search_error, "搜索失败");

    let mut missing = InspectState::default();
    missing.results = vec![row("repo", "kept", "kept.png")];
    missing.reduce(
        true,
        None,
        InspectMessage::ToggleFilter {
            key: FilterList::Tags,
            value: "peach".into(),
        },
    );
    assert!(missing.results.is_empty(), "筛选却没有活动仓库时清空结果");
    assert!(missing.effects.is_empty());

    let mut locked = InspectState::default();
    locked.reduce(
        false,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Tags,
            value: "peach".into(),
        },
    );
    assert!(locked.filters.tags.is_empty(), "不可写仓库不改条件");
    assert!(locked.effects.is_empty());
}

/// 跑完的带条件搜索哪怕没有命中也记为已搜索；清空条件、筛选缺仓库或切换资源库后回到等条件。
#[test]
fn finished_search_without_hits_is_not_waiting_for_criteria() {
    let finish = |state: &mut InspectState, result: Result<Vec<SearchRow>, String>| {
        let generation = state.search_generation;
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::SearchFinished { generation, result },
        );
    };
    let mut state = InspectState::default();
    assert!(!state.searched);
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetQuery("不存在的文件".into()),
    );
    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    assert!(state.searching && !state.searched, "请求还没返回");
    finish(&mut state, Ok(Vec::new()));
    assert!(state.searched && state.results.is_empty(), "跑完没有命中");

    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    finish(&mut state, Err("索引损坏".into()));
    assert_eq!(state.search_error, "索引损坏");

    state.reduce(true, Some("repo"), InspectMessage::SetQuery(String::new()));
    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    assert!(
        !state.searched && state.search_error.is_empty(),
        "没有条件时回到等条件"
    );

    state.reduce(true, Some("repo"), InspectMessage::SetQuery("封面".into()));
    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    finish(&mut state, Ok(Vec::new()));
    state.reduce(
        true,
        None,
        InspectMessage::ToggleFilter {
            key: FilterList::Tags,
            value: "参考".into(),
        },
    );
    assert!(!state.searched, "筛选缺活动仓库时没有发请求");

    state.reduce(true, Some("repo"), InspectMessage::RunSearch);
    finish(&mut state, Ok(Vec::new()));
    assert!(state.searched);
    state.reset_search();
    assert!(!state.searched, "切换资源库清空搜索");
}

#[test]
fn toggling_checks_the_raw_value_and_stores_the_trimmed_one() {
    let mut state = InspectState::default();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Colors,
            value: "red".into(),
        },
    );
    // 按原值判断是否已选：带空白的同名值不在列表里，只会再加一次（去重后不变）。
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Colors,
            value: " red ".into(),
        },
    );
    assert_eq!(state.filters.colors, ["red"]);
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Colors,
            value: "red".into(),
        },
    );
    assert!(state.filters.colors.is_empty());
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Tags,
            value: "   ".into(),
        },
    );
    assert!(state.filters.tags.is_empty(), "空白值不改条件");
}

#[test]
fn color_and_shape_inputs_submit_like_add_and_clear_the_field() {
    let mut state = InspectState::default();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetMetadataInput {
            key: MetadataInput::Colors,
            value: " 青色 ".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetMetadataInput {
            key: MetadataInput::Shapes,
            value: "方形".into(),
        },
    );
    assert!(state.filters.colors.is_empty(), "输入不直接生效");
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SubmitMetadataInput(MetadataInput::Colors),
    );
    assert_eq!(state.filters.colors, ["青色"]);
    assert_eq!(state.search_ui.draft.color, "", "提交后清空颜色输入");
    assert_eq!(state.search_ui.draft.shape, "方形", "形状输入不受影响");
    assert!(state.search_ui.reveal_panel, "提交后切到搜索面板");
    let request = last_search(&mut state);
    assert_eq!(request.metadata_filters, [("color".into(), "青色".into())]);

    // 同名再提交一次是取消，和点芯片一样。
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetMetadataInput {
            key: MetadataInput::Colors,
            value: "青色".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SubmitMetadataInput(MetadataInput::Colors),
    );
    assert!(state.filters.colors.is_empty());

    let mut blank = InspectState::default();
    blank.reduce(
        true,
        Some("repo"),
        InspectMessage::SubmitMetadataInput(MetadataInput::Shapes),
    );
    assert!(blank.effects.is_empty(), "空输入不提交");
    let mut locked = InspectState::default();
    locked.reduce(
        false,
        Some("repo"),
        InspectMessage::SetMetadataInput {
            key: MetadataInput::Shapes,
            value: "方形".into(),
        },
    );
    locked.reduce(
        false,
        Some("repo"),
        InspectMessage::SubmitMetadataInput(MetadataInput::Shapes),
    );
    assert!(locked.filters.shapes.is_empty());
    assert_eq!(locked.search_ui.draft.shape, "方形", "不可写时不清空输入");
}

#[test]
fn clear_keeps_the_query_and_resets_every_input() {
    let mut state = InspectState::default();
    state.reduce(true, Some("repo"), InspectMessage::SetQuery("peach".into()));
    state.reduce(true, Some("repo"), InspectMessage::ToggleFilterBar);
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ToggleFilter {
            key: FilterList::Tags,
            value: "draft".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetMetadataInput {
            key: MetadataInput::Colors,
            value: "红".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetAdvanced {
            field: AdvancedField::Limit,
            value: "20".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetSortDirection(SortDirection::Desc),
    );
    state.take_effects();
    state.reduce(true, Some("repo"), InspectMessage::ClearFilters);
    assert_eq!(state.query, "peach");
    assert!(state.filter_bar_open, "清除不收起筛选栏");
    assert!(state.filters.tags.is_empty());
    assert_eq!(
        state.search_ui.draft,
        super::super::SearchDraft::default(),
        "清除清空所有输入并把排序方向改回升序"
    );
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
fn advanced_inputs_parse_and_apply_together() {
    let mut state = InspectState::default();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetAdvanced {
            field: AdvancedField::ExcludeQuery,
            value: " nope ".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetAdvanced {
            field: AdvancedField::ExcludePaths,
            value: r"a\b,/c/".into(),
        },
    );
    state.reduce(true, Some("repo"), InspectMessage::ApplyAdvanced);
    let request = last_search(&mut state);
    assert_eq!(request.exclude_query.as_deref(), Some("nope"));
    assert_eq!(request.exclude_path_prefixes, ["a/b", "c"]);
    assert!(
        request.repo_id.is_none(),
        "排除关键词和排除路径不把搜索收进当前仓库"
    );

    for (field, value) in [
        (AdvancedField::Metadata, "artist=momo, =x, key="),
        (AdvancedField::Number, "rating=1..5,nan=foo..bar,width=..10"),
        (AdvancedField::Date, "added=2020-01-01..2021-01-01"),
        (AdvancedField::ExcludeTags, "旧，草稿,旧"),
        (AdvancedField::SortField, " name "),
        (AdvancedField::Limit, "0"),
    ] {
        state.reduce(
            true,
            Some("repo"),
            InspectMessage::SetAdvanced {
                field,
                value: value.into(),
            },
        );
    }
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetSortDirection(SortDirection::Desc),
    );
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

    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetMinimumRating(Some(0.0)),
    );
    assert!(state.filters.min_rating.is_none(), "0 星视为全部");
    state.take_effects();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetMinimumRating(Some(3.0)),
    );
    let request = last_search(&mut state);
    assert_eq!(request.min_rating, Some(3.0));
}

#[test]
fn shortcut_rewrites_metadata_and_sort_and_clears_the_excludes() {
    let mut state = InspectState::default();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetAdvanced {
            field: AdvancedField::ExcludeTags,
            value: "旧".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetAdvanced {
            field: AdvancedField::Limit,
            value: "5".into(),
        },
    );
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::SetAdvanced {
            field: AdvancedField::ExcludeQuery,
            value: "试听".into(),
        },
    );
    state.reduce(true, Some("repo"), InspectMessage::ApplyAdvanced);
    state.take_effects();
    state.reduce(
        true,
        Some("repo"),
        InspectMessage::ApplyShortcut {
            metadata: "libraryKind=asmr\nlisteningStatus=listening".into(),
            sort_field: "metadata.lastListenedAt".into(),
            sort_direction: SortDirection::Desc,
        },
    );
    let draft = &state.search_ui.draft;
    assert_eq!(
        draft.metadata,
        "libraryKind=asmr\nlisteningStatus=listening"
    );
    assert_eq!(draft.sort_field, "metadata.lastListenedAt");
    assert_eq!(draft.sort_direction, SortDirection::Desc);
    assert!(
        draft.exclude_tags.is_empty() && draft.limit.is_empty(),
        "快捷方式清掉排除标签和数量输入"
    );
    assert_eq!(draft.exclude_query, "试听", "其余输入保留");
    assert!(state.filters.exclude_tags.is_empty());
    assert!(state.filters.limit.is_none());
    assert_eq!(state.filters.exclude_query, "试听");
    let request = last_search(&mut state);
    assert_eq!(request.metadata_filters.len(), 2);
    assert_eq!(
        request.sort_field.as_deref(),
        Some("metadata.lastListenedAt")
    );
    assert_eq!(request.sort_direction.as_deref(), Some("desc"));

    let mut locked = InspectState::default();
    locked.reduce(
        false,
        Some("repo"),
        InspectMessage::ApplyShortcut {
            metadata: "kind=book".into(),
            sort_field: "added".into(),
            sort_direction: SortDirection::Desc,
        },
    );
    assert!(
        locked.filters.metadata_filters.is_empty(),
        "不可写仓库不应用快捷方式"
    );
}

#[test]
fn active_filter_count_counts_each_active_filter() {
    let mut state = InspectState::default();
    assert_eq!(state.active_filter_count(), 0);
    state.filters.formats = vec!["png".into()];
    state.filters.colors = vec!["红色".into(), "#3fa796".into()];
    state.filters.min_rating = Some(3.0);
    state.filters.sort_field = "name".into();
    state.filters.exclude_query = "草稿".into();
    state.filters.exclude_path_prefixes = "tmp".into();
    state.filters.exclude_number_filters = "width=0..1".into();
    assert_eq!(
        state.active_filter_count(),
        5,
        "排除关键词、路径和数值范围不计数"
    );
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
