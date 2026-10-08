//! 预览、元数据和搜索视图。
//!
//! 图片沿用壳层已经上传的 GPU 纹理。Markdown 用 `NativeMarkdown`，纯文本用
//! `SelectableRichText`。代码类纯文本按语义角色给关键字、字符串和注释上色；txt、log、csv 保持无色。音视频用 `MediaTransportBar`，视频画面走已有预览纹理。

use std::sync::Arc;

use nana_ui::icons_tabler::{ARROW_LEFT, EYE, FOLDER_OPEN, LINK, MESSAGE, PLUS, STAR, VOLUME};
use nana_ui::runtime::view::{button, icon_button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, Chip, ChipDismissed, EmptyState, GpuTextureView, Icon, IconButton, IconGlyph, JustifySpec, LabeledValue,
    LengthSpec, ListItem, MediaTransportBar, MediaTransportEvent, MediaTransportPlacement, NativeMarkdown, RadiusTier,
    RichSpan, ScrollAxes, ScrollView, SelectableRichText, SemanticColorRole, Stack, Text, TextChanged, TextInput,
    ValidationIntent, ValidationMessage,
};
use nana_ui::{ButtonKind, ControlSize};

#[path = "syntax_color.rs"]
mod syntax_color;
#[path = "inspect_preview_page.rs"]
mod preview_page;

use super::inspect::{
    AdvancedField, FilterList, InspectMessage, InspectState, PreviewBody, SearchFilters, SortDirection,
};
use super::{ShellMessage, ShellViewModel};

/// 搜索面板或已选文件时替换验收用的预览占位。
pub(super) fn inspect_surface(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let mut rows = Vec::new();
    if model.workspace.panel == super::workspace::WorkspacePanel::Search || inspect.filter_bar_open {
        rows.push(search_panel(model));
    }
    if inspect.has_target() {
        rows.push(
            widget(
                Stack::row(12.0)
                    .width(LengthSpec::Fill)
                    .height(LengthSpec::Fill)
                    .min_height(LengthSpec::Px(0.0))
                    .grow(1.0)
                    .shrink(1.0)
                    .align(AlignSpec::Stretch)
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
            }),
            )
            .children((
                widget(
                    Stack::column(6.0)
                        .grow(1.0)
                        .shrink(1.0)
                        .min_width(LengthSpec::Px(0.0))
                        .min_height(LengthSpec::Px(0.0))
                        .height(LengthSpec::Fill)
                        .with_layout(|layout| {
                            layout.flex_basis = Some(LengthSpec::Px(0.0));
                        }),
                )
                .children((preview_panel(model),))
                .into_any(),
                widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
                    layout.width = Some(LengthSpec::Px(280.0));
                    layout.min_width = Some(LengthSpec::Px(260.0));
                    layout.flex_grow = Some(0.0);
                    layout.flex_shrink = Some(0.0);
                    layout.height = Some(LengthSpec::Fill);
                    layout.min_height = Some(LengthSpec::Px(0.0));
                    layout.flex_basis = Some(LengthSpec::Px(0.0));
                }))
                .children((widget(Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children((
                    super::inspect_library::fact_column(&inspect.facts),
                    metadata_panel(model),
                )),))
                .key("inspect-preview-stats"),
            ))
            .key("inspect-preview-row")
            .into_any(),
        );
    }
    widget(
        Stack::fill_column(8.0)
            .min_height(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
            }),
    )
    .children(rows)
    .into_any()
}

/// 预览区。页头一行：返回在左，标题块在中，打开和定位在右。
/// 页图接在页头下面，并伸到播放条上方，中间不再留一块空列。
fn preview_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let mut rows = vec![preview_header(model)];
    if let Some(palette) = super::palette::swatches(&inspect.palette, "inspect-preview-palette") {
        rows.push(palette);
    }
    rows.push(
        widget(
            Stack::fill_column(0.0)
                .min_height(LengthSpec::Px(0.0))
                .grow(1.0)
                .shrink(1.0)
                .with_layout(|layout| {
                    layout.flex_basis = Some(LengthSpec::Px(0.0));
                }),
        )
        .key("inspect-preview-stage")
        .children((preview_body(model),))
        .into_any(),
    );
    if let Some(queue) = super::inspect_asmr::preview_panels(model) {
        // 队列贴在预览壳底部，高度按内容，不跟画面一起被播放条裁掉。
        rows.push(
            widget(
                Stack::column(0.0)
                    .grow(0.0)
                    .shrink(0.0)
                    .width(LengthSpec::Fill)
                    .min_width(LengthSpec::Px(0.0)),
            )
            .children((queue,))
            .into_any(),
        );
    }
    if !inspect.activity.is_empty() {
        rows.push(widget(super::workbench::meta(inspect.activity.clone())).key("inspect-activity").into_any());
    }
    if !inspect.error.is_empty() {
        rows.push(widget(ValidationMessage::new(inspect.error.clone(), ValidationIntent::Danger)).key("inspect-error").into_any());
    }
    widget(
        Stack::fill_column(8.0)
            .min_height(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0)
            .with_layout(|layout| {
                layout.flex_basis = Some(LengthSpec::Px(0.0));
            }),
    )
    .children(rows)
    .into_any()
}

/// 还没开始读取时，音频用和 Vue `FileAudio` 一样的类型标记，不写成正在准备。
fn pending_preview(model: &ShellViewModel) -> AnyView {
    if model.inspect.loading || !is_audio_extension(model) {
        return widget(EmptyState::new("正在准备预览").message("准备好后画面会出现在这里。").compact(true))
            .key("inspect-pending")
            .into_any();
    }
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Fill)
            .min_height(LengthSpec::Px(0.0)),
    )
    .key("inspect-audio-mark")
    .children((widget(IconGlyph::new(VOLUME).size(54.0)),))
    .into_any()
}

fn is_audio_extension(model: &ShellViewModel) -> bool {
    let extension = model.inspect.facts.extension.trim().trim_start_matches('.').to_ascii_lowercase();
    let extension = if extension.is_empty() {
        syntax_color::extension_of(model.inspect.target_path.as_deref().unwrap_or("")).to_ascii_lowercase()
    } else {
        extension
    };
    matches!(extension.as_str(), "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus")
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().filter(|part| !part.is_empty()).unwrap_or(path).to_string()
}

fn preview_body(model: &ShellViewModel) -> AnyView {
    match &model.inspect.body {
        PreviewBody::Image if model.preview_pixels.is_some() => {
            widget(GpuTextureView::new("file-preview").contain()).key("inspect-image").into_any()
        }
        PreviewBody::Document { markdown: true, text: source } => {
            widget(NativeMarkdown::parse(source)).key("inspect-markdown").into_any()
        }
        PreviewBody::Document { markdown: false, text: source } => highlighted_text(model, source),
        PreviewBody::Media(session) => media_panel(model, session),
        PreviewBody::Native { content, label, view_id } => native_preview(model, label, view_id, content),
        PreviewBody::Failed(message) => {
            widget(Text::new(message.clone()).color(SemanticColorRole::Danger).font_size(13.0)).key("inspect-failed").into_any()
        }
        PreviewBody::Empty | PreviewBody::Image => pending_preview(model),
    }
}

fn native_preview(model: &ShellViewModel, label: &str, view_id: &str, content: &str) -> AnyView {
    let mut rows = Vec::new();
    if super::inspect_library::is_office_pdf(view_id) {
        if let Some(bar) = super::inspect_library::document_toolbar(model, label) {
            rows.push(bar);
        }
    }
    if super::inspect_library::is_model(view_id) {
        let extension = syntax_color::extension_of(model.inspect.target_path.as_deref().unwrap_or(""));
        if !extension.is_empty() {
            rows.push(text(extension.to_ascii_uppercase()).key("inspect-model-hud").into_any());
        }
    }
    if let Some(error) = model.inspect.raster_error() {
        rows.push(text(error.to_string()).key("inspect-native-page-error").into_any());
    }
    let nav = model.inspect.page_nav();
    if model.inspect.has_mesh() {
        rows.push(
            widget(Stack::row(8.0)).children((
                button("左转").key("inspect-orbit-left").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::Orbit { yaw: -0.4, zoom: 1.0 }));
                }),
                button("右转").key("inspect-orbit-right").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::Orbit { yaw: 0.4, zoom: 1.0 }));
                }),
                button("放大").key("inspect-orbit-in").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::Orbit { yaw: 0.0, zoom: 1.25 }));
                }),
                button("缩小").key("inspect-orbit-out").on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::Orbit { yaw: 0.0, zoom: 0.8 }));
                }),
            )).into_any(),
        );
    }
    let page = preview_page::page_bitmap(model);
    let show_page = page.is_some() && !super::inspect_library::is_archive(view_id);
    if super::inspect_library::is_archive(view_id) {
        rows.push(super::inspect_library::archive_list(content));
    } else if let Some(page) = page {
        // 翻页在页图上方。页纸按宽高比撑开，超出视口时整列滚动，不裁掉页底。
        rows.push(preview_page::page_scroller(page, nav));
    } else {
        if let Some((index, total)) = nav {
            rows.push(preview_page::page_nav(index, total));
        }
        let body = if content.is_empty() { label.to_string() } else { content.to_string() };
        rows.push(widget(SelectableRichText::new([RichSpan::plain(body)])).key("inspect-native").into_any());
    }
    // 有页位图时这一列占满页头到播放条之间的高度。
    let stack = if show_page {
        Stack::fill_column(8.0).min_height(LengthSpec::Px(0.0))
    } else {
        Stack::column(8.0)
    };
    widget(stack).children(rows).into_any()
}

fn highlighted_text(model: &ShellViewModel, source: &str) -> AnyView {
    let extension = syntax_color::extension_of(model.inspect.target_path.as_deref().unwrap_or(""));
    match syntax_color::paint(&extension, source) {
        Ok(spans) => widget(SelectableRichText::new(spans)).key("inspect-text").into_any(),
        Err(error) => {
            eprintln!("Nana 文本着色失败：{error}");
            text(error).key("inspect-failed").into_any()
        }
    }
}

fn media_panel(model: &ShellViewModel, session: &crate::backend::services::repository::PlaybackSessionState) -> AnyView {
    let mut rows = Vec::new();
    if model.preview_pixels.is_some() && model.preview_token.as_deref().is_some_and(|token| token.starts_with("video:")) {
        rows.push(widget(GpuTextureView::new("file-preview").contain()).key("inspect-video").into_any());
    }
    rows.push(media_bar(session));
    widget(Stack::column(8.0)).children(rows).into_any()
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

/// 注释、链接、评分，然后是标签组。和 `FileMetadataEditor` 的顺序一致。
/// 注释和链接是标签在上、输入在下。保存交给 260 毫秒自动保存，画面上不放保存、撤销和重做。
pub(super) fn metadata_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let locked = !inspect.can_edit();
    let expanded = inspect.tags_expanded;
    let mut rows = vec![
        meta_row(
            "注释",
            MESSAGE,
            "inspect-comment",
            inspect.draft_comment(),
            "记录这个文件的用途、状态或上下文。",
            locked,
            |value| InspectMessage::SetComment(value),
        ),
        meta_row("链接", LINK, "inspect-link", inspect.draft_link(), "https://example.com", locked, |value| {
            InspectMessage::SetLink(value)
        }),
        rating_buttons(inspect.draft_rating(), locked),
        tag_group_row(inspect.draft_tags().len(), expanded),
    ];
    if expanded {
        let mut tags = Vec::new();
        for tag in inspect.draft_tags() {
            let remove = tag.clone();
            tags.push(
                widget(Chip::new(tag.clone()).selected(true).dismissible(!locked).close_label(format!("移除 {tag}")).disabled(locked))
                    .key(format!("inspect-tag-{tag}"))
                    .on_cx(move |_, _: &ChipDismissed, cx| cx.dispatch_program(inspect_message(InspectMessage::RemoveTag(remove.clone()))))
                    .into_any(),
            );
        }
        if !tags.is_empty() {
            rows.push(widget(Stack::row(4.0).wrap(true)).key("inspect-tags").children(tags).into_any());
        }
        rows.push(tag_add_button(locked));
        if inspect.tag_menu_open() {
            rows.push(tag_draft_field(locked));
            for tag in tag_choices(model) {
                let add = tag.clone();
                rows.push(
                    button(tag.clone())
                        .key(format!("inspect-tag-choice-{tag}"))
                        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::AddTag(add.clone()))))
                        .into_any(),
                );
            }
            rows.push(button("关闭标签").key("inspect-tag-menu-close").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::CloseTagMenu));
            }).into_any());
        }
    }
    rows.extend(super::inspect_library::recorded_rows(model));
    if let Some(palette) = super::palette::swatches(&inspect.palette, "inspect-metadata-palette") {
        rows.push(palette);
    }
    let custom = super::inspect_asmr::display_custom(model);
    let library = super::inspect_library::library_sections(&custom);
    let claimed = !library.is_empty();
    rows.extend(library);
    rows.extend(super::inspect_asmr::candidate_section(model, &custom));
    for (key, value) in inspect.draft_custom() {
        if claimed && super::inspect_library::claimed_metadata_keys().contains(&key.as_str()) {
            continue;
        }
        rows.push(widget(LabeledValue::new(key.clone(), value.clone())).key(format!("inspect-custom-{key}")).into_any());
        let remove = key.clone();
        rows.push(
            widget(Button::new(format!("删除字段 {key}")).kind(ButtonKind::Danger).disabled(locked))
                .key(format!("inspect-custom-remove-{key}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::RemoveCustom(remove.clone()))))
                .into_any(),
        );
    }
    if !inspect.conflict.is_empty() {
        rows.push(widget(ValidationMessage::new(inspect.conflict.clone(), ValidationIntent::Danger)).key("inspect-conflict").into_any());
        rows.push(
            button("采用服务器版本").key("inspect-adopt").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::AdoptConflict));
            }).into_any(),
        );
    }
    // 上边一条发丝线，把统计和编辑分开。不给这一列再套卡片。
    let panel = super::workbench::with_top_divider(
        Stack::column(4.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)),
    );
    widget(panel).children(rows).into_any()
}

fn search_panel(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    if model.workspace.panel != super::workspace::WorkspacePanel::Search {
        return search_controls(inspect, false);
    }
    let mut rows = Vec::new();
    if inspect.filter_bar_open {
        rows.push(filter_bar(model));
    }
    rows.push(search_results(model));
    // 筛选栏按内容高度，结果面板接在下面，不让筛选卡片撑满整页。
    widget(Stack::column(12.0).min_height(LengthSpec::Px(0.0))).children(rows).key("search-workbench").into_any()
}

/// 搜索结果面板。筛选栏在它上面，不塞进结果列表里。
fn search_results(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let scope = if inspect.filters.has_active_filters() {
        let name = model.workspace.active_repository().map(|item| item.name.clone()).unwrap_or_else(|| "当前资源库".into());
        format!("{name}内筛选")
    } else {
        "全局搜索".into()
    };
    let mut body = vec![super::workbench::header(
        &scope,
        "inspect-search-eyebrow",
        "搜索结果",
        "inspect-search-title",
        &search_summary(inspect),
        "inspect-search-summary",
        vec![
            super::workbench::badge(format!("{} 个仓库", model.workspace.repositories.len()), "inspect-search-repos"),
            super::workbench::badge(format!("{} 条结果", inspect.results.len()), "inspect-search-hits"),
        ],
    )];
    if !inspect.search_error.is_empty() {
        body.push(widget(ValidationMessage::new(inspect.search_error.clone(), ValidationIntent::Danger)).key("inspect-search-error").into_any());
    }
    if inspect.searching {
        body.push(widget(super::workbench::meta("正在执行全局搜索")).key("inspect-search-loading").into_any());
    } else if model.workspace.repositories.is_empty() {
        body.push(super::workbench::dashed_empty(
            "还没有可搜索的资源库",
            "先在资源库页面添加一个仓库，再执行跨仓库搜索。",
            "inspect-search-empty",
            "inspect-search-empty-detail",
            false,
            true,
        ));
    } else if inspect.results.is_empty() {
        body.push(super::workbench::dashed_empty(
            "等待搜索条件",
            "输入关键词、标签或评分条件后，这里会展示结果。",
            "inspect-search-empty",
            "inspect-search-empty-detail",
            false,
            true,
        ));
    } else {
        body.extend(inspect.results.iter().map(search_hit));
    }
    super::workbench::panel(body)
}

fn search_hit(row: &super::inspect::SearchRow) -> AnyView {
    let asset_id = row.asset_id.clone();
    let detail = if row.path.is_empty() { row.repo_name.clone() } else { format!("{} / {}", row.repo_name, row.path) };
    let kind = file_kind(&row.filename);
    widget(Stack::row(8.0).align(AlignSpec::Center))
        .key(format!("inspect-hit-{}", row.asset_id))
        .children((
            widget(Chip::new(kind)).key(format!("inspect-hit-kind-{}", row.asset_id)),
            widget(ListItem::new(row.filename.clone()).detail(detail)).on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::OpenHit(asset_id.clone())))
            }),
        ))
        .into_any()
}

/// 结果行的类型标记。没有扩展名时用「文件」，不另造分类。
fn file_kind(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => ext.to_ascii_uppercase(),
        _ => "文件".into(),
    }
}

fn search_controls(inspect: &InspectState, embedded: bool) -> AnyView {
    let mut rows = Vec::new();
    if !embedded {
        rows.push(text(search_summary(inspect)).key("inspect-search-summary").into_any());
    }
    rows.push(field("搜索", "inspect-query", &inspect.query, false, |value| InspectMessage::SetQuery(value)));
    rows.push(widget(Stack::row(8.0)).children((
        button("搜索").key("inspect-run-search").disabled(inspect.searching).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::RunSearch));
        }),
        button(if inspect.filter_bar_open { "关闭筛选" } else { "筛选" }).key("inspect-filter-toggle").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::ToggleFilterBar));
        }),
    )).into_any());
    if !embedded && !inspect.search_error.is_empty() {
        rows.push(widget(ValidationMessage::new(inspect.search_error.clone(), ValidationIntent::Danger)).key("inspect-search-error").into_any());
    }
    if !embedded && inspect.results.is_empty() && !inspect.searching {
        rows.push(
            widget(EmptyState::new("没有搜索结果").message("换一个关键词，或清空筛选后再查。").compact(true))
                .key("inspect-search-empty")
                .into_any(),
        );
    }
    if !embedded {
        rows.extend(inspect.results.iter().map(search_hit));
    }
    widget(Stack::column(8.0)).children(rows).into_any()
}

pub(super) fn filter_bar(model: &ShellViewModel) -> AnyView {
    let filters = &model.inspect.filters;
    let searching = model.inspect.searching;
    let shortcuts = model.inspect.shortcuts.as_slice();
    let name = model.workspace.active_repository().map(|item| item.name.clone()).unwrap_or_else(|| "当前资源库".into());
    let count = filter_count(filters);
    let mut trailing = Vec::new();
    if count > 0 {
        trailing.push(super::workbench::badge(format!("{count} 个条件"), "inspect-filter-count"));
    }
    trailing.push(
        button("清除").disabled(searching || (count == 0 && model.inspect.query.trim().is_empty())).key("inspect-clear-filters").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::ClearFilters));
        }).into_any(),
    );
    trailing.push(
        icon_button(Icon::Close, "关闭筛选栏").key("inspect-filter-close").on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::ToggleFilterBar));
        }).into_any(),
    );
    // 左列用 Shrink，避免按父级 100% 把清除和关闭挤出卡片。
    let mut rows = vec![widget(Stack::bar(8.0).align(AlignSpec::Center))
        .children((
            widget(Stack::column(2.0).width(LengthSpec::Shrink).min_width(LengthSpec::Px(0.0)).grow(1.0).shrink(1.0)).children((
                widget(super::workbench::eyebrow("当前资源库筛选")).key("inspect-filter-title"),
                widget(super::workbench::section_title(name)).key("inspect-filter-repo"),
            )),
            widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0)).children(trailing),
        ))
        .into_any()];
    rows.push(chip_row("格式", FilterList::Formats, &filters.formats, searching, "添加格式"));
    rows.push(chip_row("标签", FilterList::Tags, &filters.tags, searching, "添加标签"));
    rows.push(chip_row("颜色", FilterList::Colors, &filters.colors, searching, "输入颜色"));
    rows.push(chip_row("形状", FilterList::Shapes, &filters.shapes, searching, "输入形状"));
    rows.push(rating_group(filters.min_rating));
    if !shortcuts.is_empty() {
        rows.push(labeled_row("库类型", shortcut_row(shortcuts, searching)));
    }
    rows.push(widget(super::workbench::eyebrow("高级")).key("inspect-filter-advanced").into_any());
    let mut advanced = Vec::new();
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
        advanced.push(advanced_field(label, field, searching));
    }
    advanced.push(
        widget(Stack::row(8.0).wrap(true)).children((
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
    rows.push(widget(Stack::row(8.0).wrap(true)).children(advanced).key("inspect-filter-advanced-fields").into_any());
    widget(
        Stack::column(8.0)
            .surface(SemanticColorRole::Surface)
            .radius(nana_ui::runtime::RadiusTier::Lg)
            .padding_xy(16.0, 12.0)
            .grow(0.0)
            .shrink(0.0),
    )
    .children(rows)
    .key("workspace-filter-bar")
    .into_any()
}

fn filter_count(filters: &SearchFilters) -> usize {
    filters.tags.len()
        + filters.formats.len()
        + filters.colors.len()
        + filters.shapes.len()
        + usize::from(filters.min_rating.is_some())
        + usize::from(!filters.metadata_filters.trim().is_empty())
        + usize::from(!filters.sort_field.trim().is_empty())
}

/// 左标签、右芯片，对应 Vue 筛选栏 `42px + 1fr` 的一行。
fn labeled_row(label: &str, body: AnyView) -> AnyView {
    widget(Stack::row(8.0).align(AlignSpec::Center).wrap(true))
        .children((widget(super::workbench::meta(label)).key(format!("inspect-filter-group-{label}")), body))
        .into_any()
}

fn rating_group(min_rating: Option<f64>) -> AnyView {
    let mut buttons = vec![widget(Chip::new("全部").selected(min_rating.is_none())).key("inspect-rating-clear").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(inspect_message(InspectMessage::SetMinimumRating(None)));
    }).into_any()];
    for rating in 1..=5 {
        let selected = min_rating == Some(f64::from(rating));
        buttons.push(
            widget(Chip::new(format!("{rating} 星+")).selected(selected))
                .key(format!("inspect-rating-{rating}"))
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::SetMinimumRating(Some(f64::from(rating)))));
                })
                .into_any(),
        );
    }
    labeled_row("评分", widget(Stack::row(8.0).wrap(true)).children(buttons).into_any())
}

/// 库类型快捷方式。点下去走已有的 `ApplyShortcut`。
fn shortcut_row(shortcuts: &[super::inspect_shortcuts::SearchShortcut], searching: bool) -> AnyView {
    let buttons = shortcuts
        .iter()
        .map(|shortcut| {
            let metadata = shortcut.metadata.clone();
            let sort_field = shortcut.sort_field.clone();
            let direction = shortcut.sort_direction;
            button(shortcut.label.clone())
                .key(format!("inspect-shortcut-{}", shortcut.id))
                .disabled(searching)
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::ApplyShortcut {
                        metadata: metadata.clone(),
                        sort_field: sort_field.clone(),
                        sort_direction: direction,
                    }));
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(8.0).wrap(true)).children(buttons).key("inspect-shortcuts").into_any()
}

fn chip_row(label: &'static str, key: FilterList, values: &[String], disabled: bool, placeholder: &'static str) -> AnyView {
    let mut chips = Vec::new();
    for value in values {
        let chip_value = value.clone();
        let chip = widget(Chip::new(value.clone()).selected(true).disabled(disabled))
            .key(format!("inspect-filter-chip-{label}-{value}"))
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program(inspect_message(InspectMessage::ToggleFilter { key, value: chip_value.clone() }));
            })
            .into_any();
        if key == FilterList::Colors {
            chips.push(
                widget(Stack::row(4.0).align(AlignSpec::Center))
                    .key(format!("inspect-filter-color-{value}"))
                    .children((color_swatch(value), chip))
                    .into_any(),
            );
        } else {
            chips.push(chip);
        }
    }
    labeled_row(
        label,
        widget(Stack::row(8.0).align(AlignSpec::Center).wrap(true)).children((
            widget(Stack::row(4.0).wrap(true)).children(chips),
            widget(compact_input(placeholder, disabled))
                .key(format!("inspect-filter-{label}"))
                .on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program(inspect_message(InspectMessage::SetFilterInput { key, value: event.value.to_string() }));
                }),
        )).into_any(),
    )
}

/// 颜色筛选用色块。能认的名字映射到语义色，不把色名再写进另一段说明。
fn color_swatch(name: &str) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(14.0))
            .height(LengthSpec::Px(14.0))
            .min_width(LengthSpec::Px(14.0))
            .min_height(LengthSpec::Px(14.0))
            .grow(0.0)
            .shrink(0.0)
            .surface(color_role(name))
            .radius(RadiusTier::Sm),
    )
    .key(format!("inspect-filter-swatch-{name}"))
    .into_any()
}

fn color_role(name: &str) -> SemanticColorRole {
    match name.trim().to_ascii_lowercase().as_str() {
        "red" | "红色" => SemanticColorRole::Danger,
        "green" | "绿色" => SemanticColorRole::Success,
        "yellow" | "黄色" | "orange" | "橙色" => SemanticColorRole::Warning,
        "blue" | "蓝色" | "purple" | "紫色" | "pink" | "粉色" => SemanticColorRole::Accent,
        _ => SemanticColorRole::Border,
    }
}

/// 高级条件用短输入横排，对应 Vue 筛选栏的自适应网格。
fn compact_input(placeholder: &'static str, disabled: bool) -> TextInput {
    let mut field = TextInput::new(String::new()).placeholder(placeholder).size(ControlSize::Small).disabled(disabled);
    let layout = Arc::make_mut(&mut field.style.layout);
    layout.width = Some(LengthSpec::Px(148.0));
    layout.min_width = Some(LengthSpec::Px(120.0));
    layout.max_width = Some(LengthSpec::Px(180.0));
    layout.flex_grow = Some(0.0);
    field
}

/// 当前文件上、还没写进草稿的标签，最多 18 个。
fn tag_choices(model: &ShellViewModel) -> Vec<String> {
    let Some(path) = model.inspect.target_path.clone() else {
        return Vec::new();
    };
    let ctx = super::files::FileContext::from_model(model);
    let Some(row) = model.files.visible_rows(&ctx).into_iter().find(|row| row.path == path) else {
        eprintln!("Nana 标签菜单找不到当前文件：{path}");
        return Vec::new();
    };
    let draft = model.inspect.draft_tags().to_vec();
    row.tags.into_iter().filter(|tag| !draft.iter().any(|item| item == tag)).take(18).collect()
}

fn advanced_field(label: &'static str, field: AdvancedField, disabled: bool) -> AnyView {
    widget(compact_input(label, disabled))
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

/// 标签在上、输入在下。
/// 注释用 Tabler `message`（方框气泡加两行字，最接近 Lucide MessageSquareText）。
/// 链接用 Tabler `link`。已编译目录里没有 Lucide Link2 那种左右半环加横线。
fn meta_row(
    label: &'static str,
    icon: Icon,
    key_name: &'static str,
    value: &str,
    placeholder: &'static str,
    disabled: bool,
    map: impl Fn(String) -> InspectMessage + Send + 'static,
) -> AnyView {
    let mut input = TextInput::new(value.to_string()).placeholder(placeholder).size(ControlSize::Small).disabled(disabled);
    input.style.border = None;
    input.style.background = None;
    input.style.radius = None;
    input.style.interaction.hovered.border = None;
    input.style.interaction.focused.border = None;
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    widget(Stack::column(2.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .key(format!("{key_name}-row"))
        .children((
            widget(super::workbench::eyebrow(label)).key(format!("{key_name}-label")),
            widget(
                Stack::row(8.0)
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill)
                    .min_width(LengthSpec::Px(0.0))
                    .padding_xy(10.0, 0.0)
                    .min_height(LengthSpec::Px(32.0))
                    .surface(SemanticColorRole::Background)
                    .outline(SemanticColorRole::Border, 1.0)
                    .radius(RadiusTier::Lg),
            )
            .key(format!("{key_name}-field"))
            .children((
                widget(IconGlyph::new(icon).size(14.0)).key(format!("{key_name}-icon")),
                widget(input).key(key_name).on_cx(move |_, event: &TextChanged, cx| {
                    cx.dispatch_program(inspect_message(map(event.value.to_string())));
                }),
            )),
        ))
        .into_any()
}

/// 标签组是一整条：标题在左，数量和箭头在右。展开时箭头向下。
fn tag_group_row(count: usize, expanded: bool) -> AnyView {
    let meta = if count == 0 { "暂无标签".to_string() } else { format!("{count} 个标签") };
    let icon = if expanded { Icon::ChevronDown } else { Icon::ChevronRight };
    let mut item = ListItem::new("标签组").detail(meta).size(ControlSize::Small);
    let layout = Arc::make_mut(&mut item.style.layout);
    layout.width = Some(LengthSpec::Fill);
    layout.flex_grow = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    widget(item)
        .key("inspect-tag-group")
        .trailing(widget(IconGlyph::new(icon).size(14.0)).key("inspect-tag-group-icon"))
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::ToggleTagGroup)))
        .into_any()
}

/// 空标签时只放「添加标签」。输入留在点开的菜单里。
fn tag_add_button(locked: bool) -> AnyView {
    let mut add = super::workbench::ghost_button("添加标签").icon(PLUS).icon_size(16.0).size(ControlSize::Small).disabled(locked);
    let layout = Arc::make_mut(&mut add.style.layout);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.width = Some(LengthSpec::Shrink);
    widget(add).key("inspect-tag-menu").on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program(inspect_message(InspectMessage::OpenTagMenu { x: 8.0, y: 8.0 }));
    }).into_any()
}

/// 菜单里的新标签输入。没点开「添加标签」时不画。
fn tag_draft_field(locked: bool) -> AnyView {
    let mut input = TextInput::new(String::new()).placeholder("输入新标签").size(ControlSize::Small).disabled(locked);
    let layout = Arc::make_mut(&mut input.style.layout);
    layout.flex_grow = Some(1.0);
    layout.flex_shrink = Some(1.0);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.width = Some(LengthSpec::Fill);
    widget(input).key("inspect-tag-draft").on_cx(|_, event: &TextChanged, cx| {
        cx.dispatch_program(inspect_message(InspectMessage::AddTag(event.value.to_string())));
    }).into_any()
}

/// 1–5 星标。再点同一颗回到 0，和 `SetRating` 的语义一致。
fn rating_buttons(rating: i64, locked: bool) -> AnyView {
    let mut stars = Vec::new();
    for value in 1_i64..=5 {
        let active = rating >= value;
        stars.push(
            widget(IconButton::new(STAR, format!("{value} 星")).selected(active).disabled(locked))
                .key(format!("inspect-rate-{value}"))
                .on_cx(move |_, _: &Activate, cx| cx.dispatch_program(inspect_message(InspectMessage::SetRating(value))))
                .into_any(),
        );
    }
    widget(Stack::column(2.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0))).children((
        widget(super::workbench::eyebrow("评分")).key("inspect-rating"),
        widget(Stack::row(2.0).align(AlignSpec::Center)).key("inspect-rating-stars").children(stars),
    )).into_any()
}

/// 页头一行。返回靠左，打开和定位靠标题块右侧。没有文件路径时三颗都禁用。
fn preview_header(model: &ShellViewModel) -> AnyView {
    let path = model.inspect.target_path.clone().unwrap_or_default();
    let enabled = !path.trim().is_empty();
    let ctx = super::files::FileContext::from_model(model);
    let root = model.workspace.active_repository().map(|item| item.path.clone()).unwrap_or_default();
    let absolute = super::input::repository_absolute(&root, path.trim());
    let has_repo = model.workspace.active_repository().is_some();
    let can_open = enabled && !absolute.trim().is_empty();
    let can_reveal = can_open && !ctx.trash;
    let directory = model.files.current_path.clone();
    let open_path = absolute.clone();
    let reveal_path = absolute;
    let name = file_name(path.trim());
    let mut title_lines = vec![
        widget(super::workbench::eyebrow("文件预览")).key("inspect-preview-eyebrow").into_any(),
        widget(Text::new(name).font_size(20.0).font_weight(600)).key("inspect-preview-name").into_any(),
    ];
    if path.trim() != file_name(path.trim()) && !path.trim().is_empty() {
        title_lines.push(widget(super::workbench::meta(path.trim())).key("inspect-preview-path").into_any());
    }
    let back = widget(Button::new("返回").kind(ButtonKind::Ghost).icon(ARROW_LEFT).disabled(!enabled))
        .key("inspect-back")
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program(ShellMessage::OpenDirectory(directory.clone()));
        })
        .into_any();
    let actions = widget(Stack::row(8.0).align(AlignSpec::Center).grow(0.0).shrink(0.0))
        .key("inspect-preview-actions")
        .children((
            widget(Button::new("打开").kind(ButtonKind::Ghost).icon(EYE).disabled(!can_open))
                .key("inspect-open")
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Input(super::input::InputMessage::OpenEntry {
                        has_repo,
                        absolute_path: open_path.clone(),
                    }));
                }),
            widget(Button::new("定位").kind(ButtonKind::Ghost).icon(FOLDER_OPEN).disabled(!can_reveal))
                .key("inspect-reveal")
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Input(super::input::InputMessage::RevealEntry {
                        absolute_path: reveal_path.clone(),
                    }));
                }),
        ))
        .into_any();
    let header = super::workbench::with_bottom_divider(
        Stack::bar(14.0)
            .align(AlignSpec::Center)
            .padding_xy(0.0, 8.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    );
    widget(header)
        .key("inspect-preview-header")
        .children((
            back,
            widget(
                Stack::column(4.0)
                    .width(LengthSpec::Shrink)
                    .min_width(LengthSpec::Px(0.0))
                    .grow(1.0)
                    .shrink(1.0),
            )
            .children(title_lines),
            actions,
        ))
        .into_any()
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
