//! 预览视图。元数据编辑在 `inspect_metadata_view`，搜索和筛选在 `inspect_search_view`。
//!
//! 图片沿用壳层已经上传的 GPU 纹理。Markdown 用 `NativeMarkdown`，纯文本用
//! `SelectableRichText`。代码类纯文本按语义角色给关键字、字符串和注释上色；txt、log、csv 保持无色。音视频用 `MediaTransportBar`，视频画面走已有预览纹理。


use nana_ui::icons_tabler::{ARROW_LEFT, EYE, FOLDER_OPEN, VOLUME};
use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, Button, EmptyState, GpuTextureView, IconGlyph, JustifySpec,
    LengthSpec, MediaTransportBar, MediaTransportEvent, MediaTransportPlacement, NativeMarkdown,
    RichSpan, ScrollAxes, ScrollView, SelectableRichText, SemanticColorRole, Stack, Text,
    ValidationIntent, ValidationMessage,
};
use nana_ui::ButtonKind;

#[path = "syntax_color.rs"]
mod syntax_color;
#[path = "inspect_preview_page.rs"]
mod preview_page;

use super::inspect::{
    InspectMessage, PreviewBody,
};
use super::{ShellMessage, ShellViewModel};

/// 搜索面板或已选文件时替换验收用的预览占位。
pub(super) fn inspect_surface(model: &ShellViewModel) -> AnyView {
    let inspect = &model.inspect;
    let mut rows = Vec::new();
    if model.workspace.panel == super::workspace::WorkspacePanel::Search || inspect.filter_bar_open {
        rows.push(super::inspect_search_view::search_panel(model));
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
                    super::inspect_metadata_view::metadata_panel(model),
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

fn inspect_message(message: InspectMessage) -> ShellMessage {
    ShellMessage::Inspect(message)
}
