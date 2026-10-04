//! 预览、元数据和搜索视图。
//!
//! 图片沿用壳层已经上传的 GPU 纹理。Markdown 用 `NativeMarkdown`，纯文本用
//! `SelectableRichText`，不开启语法高亮。音视频用 `MediaTransportBar` 反映会话状态。

use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, GpuTextureView, MediaTransportBar, MediaTransportEvent, MediaTransportPlacement, NativeMarkdown, RichSpan,
    SelectableRichText, Stack, TextChanged, TextInput,
};

use super::inspect::{
    AdvancedField, FilterList, InspectMessage, InspectState, MatchMode, PreviewBody, SearchFilters, SortDirection,
};
use super::{ShellMessage, ShellViewModel};

/// 搜索面板或已选文件时替换验收用的预览占位。
pub(super) fn inspect_surface(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let mut rows = Vec::new();
    if model.workspace.panel == super::workspace::WorkspacePanel::Search || inspect.filter_bar_open {
        rows.push(search_panel(inspect));
    }
    if inspect.has_target() {
        rows.push(preview_panel(model));
        rows.push(metadata_panel(inspect));
    }
    widget(Stack::fill_column(8.0)).children(rows).into_any()
}

fn preview_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let mut rows = vec![
        text("文件预览").key("inspect-preview-eyebrow").into_any(),
        text(inspect.target_path.clone().unwrap_or_default()).key("inspect-preview-path").into_any(),
    ];
    if !inspect.activity.is_empty() {
        rows.push(text(inspect.activity.clone()).key("inspect-activity").into_any());
    }
    if !inspect.error.is_empty() {
        rows.push(text(inspect.error.clone()).key("inspect-error").into_any());
    }
    rows.push(preview_body(model));
    widget(Stack::column(8.0)).children(rows).into_any()
}

fn preview_body(model: &ShellViewModel) -> AnyView {
    match &model.inspect.body {
        PreviewBody::Image if model.preview_pixels.is_some() => {
            widget(GpuTextureView::new("file-preview").contain()).key("inspect-image").into_any()
        }
        PreviewBody::Document { markdown: true, text: source } => {
            widget(NativeMarkdown::parse(source)).key("inspect-markdown").into_any()
        }
        PreviewBody::Document { markdown: false, text: source } => {
            widget(SelectableRichText::new([RichSpan::plain(source.clone())])).key("inspect-text").into_any()
        }
        PreviewBody::Media(session) => media_bar(session),
        PreviewBody::Native { view_id, label } => {
            text(format!("原生预览 · {label} · {view_id}")).key("inspect-native").into_any()
        }
        PreviewBody::Failed(message) => text(message.clone()).key("inspect-failed").into_any(),
        PreviewBody::Upgrade(message) => text(message.clone()).key("inspect-upgrade").into_any(),
        PreviewBody::Empty | PreviewBody::Image => text("正在准备预览").key("inspect-pending").into_any(),
    }
}

fn media_bar(session: &crate::backend::services::repository::PlaybackSessionState) -> AnyView {
    let failed = session.status == "failed";
    let duration = session.duration_ms.unwrap_or(0) as f64 / 1000.0;
    let bar = MediaTransportBar::new();
    let mut bar = bar;
    bar.playing = session.status == "playing";
    bar.disabled = failed;
    bar.seekable = session.can_seek && !failed;
    bar.position = session.current_time_ms as f64 / 1000.0;
    bar.duration = duration;
    bar.volume = f64::from(session.volume.clamp(0.0, 1.0) * 100.0);
    bar.placement = MediaTransportPlacement::Inline;
    bar.show_fullscreen = Some(false);
    widget(bar).key("inspect-transport").on_cx(|_, event: &MediaTransportEvent, cx| {
        let message = match event {
            MediaTransportEvent::PlayPause => Some(InspectMessage::PlayPause),
            MediaTransportEvent::Seek(seconds) => Some(InspectMessage::Seek((*seconds * 1000.0).max(0.0) as u64)),
            MediaTransportEvent::Volume(volume) => Some(InspectMessage::SetVolume((*volume / 100.0) as f32)),
            _ => None,
        };
        if let Some(message) = message {
            cx.dispatch_program(inspect_message(message));
        }
    }).into_any()
}

fn metadata_panel(inspect: &InspectState) -> AnyView {
    let saving = inspect.saving();
    let locked = !inspect.can_edit();
    let mut rows = vec![
        text(if inspect.dirty() { "元数据 · 未保存" } else { "元数据" }).key("inspect-metadata-title").into_any(),
        text(format!("评分 {}", inspect.draft_rating())).key("inspect-rating").into_any(),
        rating_buttons(locked),
        field("注释", "inspect-comment", inspect.draft_comment(), locked, |value| InspectMessage::SetComment(value)),
        field("链接", "inspect-link", inspect.draft_link(), locked, |value| InspectMessage::SetLink(value)),
        text(format!("标签 {}", inspect.draft_tags().join("，"))).key("inspect-tags").into_any(),
        field("添加标签", "inspect-tag-draft", "", locked, |value| InspectMessage::AddTag(value)),
    ];
    for tag in inspect.draft_tags() {
        let remove = tag.clone();
        rows.push(
            button(format!("移除 {tag}"))
                .key(format!("inspect-tag-{tag}"))
                .disabled(locked)
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::RemoveTag(remove.clone()))))
                .into_any(),
        );
    }
    for (key, value) in inspect.draft_custom() {
        rows.push(text(format!("{key} = {value}")).key(format!("inspect-custom-{key}")).into_any());
    }
    rows.push(field("自定义字段", "inspect-custom-key", "", locked, |value| InspectMessage::SetCustom { key: value, value: String::new() }));
    if !inspect.conflict.is_empty() {
        rows.push(text(inspect.conflict.clone()).key("inspect-conflict").into_any());
        rows.push(
            button("采用服务器版本").key("inspect-adopt").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::AdoptConflict));
            }).into_any(),
        );
    }
    rows.push(
        widget(Stack::row(8.0)).children((
            button(if saving { "保存中..." } else { "保存元数据" })
                .key("inspect-save")
                .disabled(locked || !inspect.dirty())
                .on_cx(|_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::SaveMetadata))),
            button("撤销").key("inspect-undo").disabled(saving || inspect.dirty()).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::Undo));
            }),
            button("重做").key("inspect-redo").disabled(saving || inspect.dirty()).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::Redo));
            }),
        )).into_any(),
    );
    widget(Stack::column(8.0)).children(rows).into_any()
}

fn search_panel(inspect: &InspectState) -> AnyView {
    let mut rows = vec![
        text(search_summary(inspect)).key("inspect-search-summary").into_any(),
        field("搜索", "inspect-query", &inspect.query, false, |value| InspectMessage::SetQuery(value)),
        widget(Stack::row(8.0)).children((
            button("搜索").key("inspect-run-search").disabled(inspect.searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::RunSearch));
            }),
            button(if inspect.filter_bar_open { "关闭筛选" } else { "筛选" }).key("inspect-filter-toggle").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::ToggleFilterBar));
            }),
        )).into_any(),
    ];
    if inspect.filter_bar_open {
        rows.push(filter_bar(&inspect.filters, inspect.searching));
    }
    if !inspect.search_error.is_empty() {
        rows.push(text(inspect.search_error.clone()).key("inspect-search-error").into_any());
    }
    if inspect.results.is_empty() && !inspect.searching {
        rows.push(text("没有搜索结果").key("inspect-search-empty").into_any());
    }
    for row in &inspect.results {
        let asset_id = row.asset_id.clone();
        rows.push(
            button(format!("{} · {}", row.filename, row.repo_name))
                .key(format!("inspect-hit-{}", row.asset_id))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::OpenHit(asset_id.clone()))))
                .into_any(),
        );
    }
    widget(Stack::column(8.0)).children(rows).into_any()
}

fn filter_bar(filters: &SearchFilters, searching: bool) -> AnyView {
    let mut rows = vec![
        text("当前资源库筛选").key("inspect-filter-title").into_any(),
        text(format!("匹配 {}", if filters.match_mode == MatchMode::Or { "任一" } else { "全部" })).key("inspect-match").into_any(),
        widget(Stack::row(8.0)).children((
            button("全部满足").key("inspect-match-and").disabled(searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetMatchMode(MatchMode::And)));
            }),
            button("任一满足").key("inspect-match-or").disabled(searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetMatchMode(MatchMode::Or)));
            }),
            button("清空筛选").key("inspect-clear-filters").disabled(searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::ClearFilters));
            }),
        )).into_any(),
        chip_row("标签", FilterList::Tags, &filters.tags, searching),
        chip_row("格式", FilterList::Formats, &filters.formats, searching),
        chip_row("颜色", FilterList::Colors, &filters.colors, searching),
        chip_row("形状", FilterList::Shapes, &filters.shapes, searching),
        text(format!("最低评分 {}", filters.min_rating.map(|rating| rating.to_string()).unwrap_or_else(|| "不限".into())))
            .key("inspect-rating-filter")
            .into_any(),
        widget(Stack::row(8.0)).children((
            button("1 星+").key("inspect-rating-1").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetMinimumRating(Some(1.0))));
            }),
            button("不限评分").key("inspect-rating-clear").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetMinimumRating(None)));
            }),
        )).into_any(),
    ];
    for (label, field) in [
        ("排除关键词", AdvancedField::ExcludeQuery),
        ("排除路径", AdvancedField::ExcludePaths),
        ("排除标签", AdvancedField::ExcludeTags),
        ("排除格式", AdvancedField::ExcludeFormats),
        ("元数据", AdvancedField::Metadata),
        ("排除元数据", AdvancedField::ExcludeMetadata),
        ("数值", AdvancedField::Number),
        ("排除数值", AdvancedField::ExcludeNumber),
        ("日期", AdvancedField::Date),
        ("排除日期", AdvancedField::ExcludeDate),
        ("排序字段", AdvancedField::SortField),
        ("条数", AdvancedField::Limit),
    ] {
        rows.push(advanced_field(label, field, searching));
    }
    rows.push(
        widget(Stack::row(8.0)).children((
            button("升序").key("inspect-sort-asc").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetSortDirection(SortDirection::Asc)));
            }),
            button("降序").key("inspect-sort-desc").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetSortDirection(SortDirection::Desc)));
            }),
            button("应用筛选").key("inspect-apply-filters").disabled(searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::ApplyAdvanced));
            }),
            button("加入筛选").key("inspect-submit-filter").disabled(searching).on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SubmitFilterInput));
            }),
        )).into_any(),
    );
    widget(Stack::column(8.0)).children(rows).into_any()
}

fn chip_row(label: &'static str, key: FilterList, values: &[String], disabled: bool) -> AnyView {
    let summary = if values.is_empty() { "无".to_string() } else { values.join("，") };
    widget(Stack::row(8.0)).children((
        text(format!("{label} · {summary}")).key(format!("inspect-filter-label-{label}")),
        widget(TextInput::new(String::new()).label(label).disabled(disabled))
            .key(format!("inspect-filter-{label}"))
            .on_cx(move |_, event: &TextChanged, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::SetFilterInput { key, value: event.value.to_string() }));
            }),
    )).into_any()
}

fn advanced_field(label: &'static str, field: AdvancedField, disabled: bool) -> AnyView {
    widget(TextInput::new(String::new()).label(label).disabled(disabled))
        .key(format!("inspect-advanced-{label}"))
        .on_cx(move |_, event: &TextChanged, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::SetAdvanced { field, value: event.value.to_string() }));
        })
        .into_any()
}

fn field(
    label: &'static str,
    key_name: &'static str,
    value: &str,
    disabled: bool,
    map: impl Fn(String) -> InspectMessage + Send + 'static,
) -> AnyView {
    widget(TextInput::new(value.to_string()).label(label).disabled(disabled)).key(key_name).on_cx(move |_, event: &TextChanged, cx| {
        cx.dispatch_program(inspect_message(map(event.value.to_string())));
    }).into_any()
}

fn rating_buttons(saving: bool) -> AnyView {
    let mut buttons = Vec::new();
    for rating in 1..=5 {
        buttons.push(
            button(format!("{rating} 星"))
                .key(format!("inspect-rate-{rating}"))
                .disabled(saving)
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::SetRating(rating))))
                .into_any(),
        );
    }
    widget(Stack::row(8.0)).children(buttons).into_any()
}

fn search_summary(inspect: &InspectState) -> String {
    if inspect.filters.has_active_filters() {
        if inspect.query.trim().is_empty() {
            "按当前资源库筛选结果。".into()
        } else {
            format!("当前资源库筛选: {}", inspect.query)
        }
    } else if inspect.query.trim().is_empty() {
        "输入关键词、标签或评分条件后，这里会展示跨仓库结果。".into()
    } else {
        format!("当前查询: {}", inspect.query)
    }
}

fn inspect_message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}
