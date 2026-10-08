//! 预览框里的内容：文本、图片、音视频、PDF / Office、压缩包、3D 模型，以及读取中、失败和不支持。
//!
//! 结构对齐各预览插件（text-preview、media-preview、office-preview、preview-archive、
//! three-model-preview）和播放器挂载（player-audio 运行时）。Vue 里插件文字继承成白色、
//! 浮层定位到外层祖先、唱片尺寸按视口算而溢出预览框这类缺陷，这里按设计意图改正。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{
    Activate, AlignSpec, GpuTextureView, IconGlyph, JustifySpec, LengthSpec, RadiusTier, ScrollAxes, ScrollView,
    SemanticColorRole, Stack, Text, TextHorizontalAlignment, Thumbnail,
};
use nana_ui::ContentFit;

use super::super::inspect::{InspectMessage, PreviewBody, PreviewKind};
use super::super::player_view::icons;
use super::super::{ShellMessage, ShellViewModel};
use super::frame::{preview_box, text};

/// 预览框和里面的内容。
pub(super) fn preview_body(model: &ShellViewModel) -> AnyView {
    if let Some(still) = current_still(model) {
        return preview_box(still, false);
    }
    let inspect = &model.inspect;
    let kind = inspect.kind.clone();
    match &inspect.body {
        PreviewBody::Document { markdown, text, truncated_at } => {
            preview_box(text_plugin(model, *markdown, text, *truncated_at), true)
        }
        PreviewBody::Image => preview_box(image_view(model), true),
        PreviewBody::Media(_) => preview_box(media_mount(model), false),
        PreviewBody::Native { view_id, label, content } => preview_box(native_view(model, view_id, label, content), true),
        PreviewBody::Failed(message) => match kind {
            Some(PreviewKind::Unsupported) | None => preview_box(fallback_view(model), false),
            Some(kind) => preview_box(failed_view(model, &kind, message), !matches!(kind, PreviewKind::Media)),
        },
        PreviewBody::Empty => match kind {
            Some(PreviewKind::Unsupported) | None if !inspect.loading => preview_box(fallback_view(model), false),
            Some(kind) => preview_box(loading_view(model, &kind), !matches!(kind, PreviewKind::Media)),
            None => preview_box(widget(Stack::fill_column(0.0)).key("inspect-pending").into_any(), true),
        },
    }
}

/// 文本插件：上面一条种类和行数，下面是等宽原文。Markdown 也按原文显示。
fn text_plugin(model: &ShellViewModel, markdown: bool, source: &str, truncated_at: Option<u64>) -> AnyView {
    let extension = model.inspect.facts.extension.trim().trim_start_matches('.').to_ascii_uppercase();
    let kind = if markdown {
        "Markdown".to_string()
    } else if extension.is_empty() {
        "TEXT".to_string()
    } else {
        extension
    };
    let lines = if source.is_empty() { 0 } else { source.replace("\r\n", "\n").replace('\r', "\n").split('\n').count() };
    let mut chips = vec![chip(&kind, true, "inspect-text-kind"), chip(&format!("{lines} 行"), false, "inspect-text-lines")];
    if let Some(read) = truncated_at {
        chips.push(chip(&format!("仅显示前 {}", format_byte_count(read)), false, "inspect-text-truncated"));
    }
    let shown = if source.is_empty() { "空文件" } else { source };
    let mut body = Text::new(shown).color(SemanticColorRole::Text).font_size(13.0).line_height(21.45);
    {
        let layout = std::sync::Arc::make_mut(&mut body.style.layout);
        layout.font_family = Some("monospace".into());
        layout.width = Some(LengthSpec::Fill);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    // 全局 `pre`：bg-subtle 底、border-soft 细边、8px 圆角、12/14 内边距，至少铺满内容区。
    let sheet = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Subtle)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md)
            .with_layout(|layout| {
                layout.padding_top = Some(LengthSpec::Px(12.0));
                layout.padding_bottom = Some(LengthSpec::Px(12.0));
                layout.padding_left = Some(LengthSpec::Px(14.0));
                layout.padding_right = Some(LengthSpec::Px(14.0));
                layout.min_height = Some(LengthSpec::Percent(100.0));
                layout.flex_grow = Some(1.0);
            }),
    )
    .children((widget(body).key("inspect-text"),))
    .key("inspect-text-sheet");
    let content = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
        layout.padding_top = Some(LengthSpec::Px(20.0));
        layout.padding_bottom = Some(LengthSpec::Px(20.0));
        layout.padding_left = Some(LengthSpec::Px(22.0));
        layout.padding_right = Some(LengthSpec::Px(22.0));
    }))
    .children((widget(Stack::fill_column(0.0).min_height(LengthSpec::Percent(100.0))).children((sheet,)),))
    .key("inspect-text-scroll");
    widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0)))
        .children((toolbar(chips, "inspect-text-toolbar"), content.into_any()))
        .key("inspect-text-plugin")
        .into_any()
}

/// 插件顶栏：42px 起，8/12 内边距，底边 border-soft，标签可折行。
fn toolbar(chips: Vec<AnyView>, key: &'static str) -> AnyView {
    let bar = super::super::workbench::with_bottom_divider(
        Stack::bar(8.0).wrap(true).align(AlignSpec::Center).min_height(LengthSpec::Px(42.0)).with_layout(|layout| {
            layout.padding_top = Some(LengthSpec::Px(8.0));
            layout.padding_bottom = Some(LengthSpec::Px(8.0));
            layout.padding_left = Some(LengthSpec::Px(12.0));
            layout.padding_right = Some(LengthSpec::Px(12.0));
        }),
    );
    widget(bar).children(chips).key(key).into_any()
}

/// 顶栏标签：bg-elev 底、border 细边、8px 圆角，12px 字。第一枚是种类，正文色半粗。
fn chip(label: &str, emphasis: bool, key: &str) -> AnyView {
    let role = if emphasis { SemanticColorRole::Text } else { SemanticColorRole::Muted };
    let weight = if emphasis { 600 } else { 400 };
    widget(
        Stack::row(6.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(26.0))
            .padding_xy(8.0, 4.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md)
            .grow(0.0)
            .shrink(0.0),
    )
    .children((widget(text(label, 12.0, weight, role, 18.6)),))
    .key(key.to_string())
    .into_any()
}

/// 图片：按原比例装进预览框。宿主已经上传纹理时用纹理槽，离屏只有像素时画成内容图。
fn image_view(model: &ShellViewModel) -> AnyView {
    if model.preview_token.is_some() && model.preview_pixels.is_some() {
        return widget(GpuTextureView::new("file-preview").contain()).key("inspect-image").into_any();
    }
    let Some(pixels) = model.preview_pixels.as_ref() else {
        return loading_view(model, &PreviewKind::Image);
    };
    pixel_image(pixels.width, pixels.height, &pixels.rgba, false, "inspect-image")
}

/// 预览的正是播放列表当前的图片：和 Vue 的播放器挂载一样画幻灯片这一帧，按「适应 / 填充」铺。
/// 没有帧（读不到或解不开）时交回普通图片预览，由它写失败原因。
fn current_still(model: &ShellViewModel) -> Option<AnyView> {
    let still = model.player.still.as_ref()?;
    let frame = still.frame.as_ref()?;
    let same_repo = model.player.repo_id.is_some() && model.player.repo_id == model.workspace.active_repo_id;
    if !same_repo || model.inspect.target_path.as_deref() != Some(still.path.as_str()) {
        return None;
    }
    let cover = model.player.settings.object_fit_cover;
    Some(pixel_image(frame.width, frame.height, &frame.rgba, cover, "inspect-player-still"))
}

/// 把 RGBA 按 contain（或 cover）画进铺满的盒子，底色是 bg。
fn pixel_image(width: u32, height: u32, rgba: &[u8], cover: bool, key: &'static str) -> AnyView {
    let Some(url) = super::super::inspect::rgba_png_data_url(width, height, rgba) else {
        eprintln!("Nana 预览图编码失败：{width}x{height}");
        return widget(Stack::fill_column(0.0)).key(key).into_any();
    };
    let image = nana_ui_core::BackgroundImage::Url {
        url,
        fit: if cover { nana_ui_core::BackgroundImageFit::Cover } else { nana_ui_core::BackgroundImageFit::Contain },
        size_width: None,
        size_height: None,
        position: nana_ui_core::BackgroundPosition::center(),
        repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
        sampling: nana_ui_core::ImageSampling::Resample,
    };
    widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background).with_layout(|layout| {
        layout.paint.content_image = Some(image);
    }))
    .key(key)
    .into_any()
}

/// 播放器挂载：视频是画面（按播放条的「适应 / 填充」铺），音频是唱片舞台。
/// 播放、跳转和音量都在底部播放条上。
fn media_mount(model: &ShellViewModel) -> AnyView {
    let video = model.preview_token.as_deref().is_some_and(|token| token.starts_with("video:"));
    if video && model.preview_pixels.is_some() {
        let fit = if model.player.settings.object_fit_cover { ContentFit::Cover } else { ContentFit::Contain };
        return widget(Stack::fill_column(0.0).with_layout(|layout| layout.background = Some([0.02, 0.02, 0.02, 1.0])))
            .children((widget(GpuTextureView::new("file-preview").fit(fit)).key("inspect-video"),))
            .key("inspect-video-frame")
            .into_any();
    }
    super::audio_stage::audio_stage(model)
}

/// PDF 和 Office：顶栏、翻页和整页；压缩包是目录；模型是画面和尺寸标签。
fn native_view(model: &ShellViewModel, view_id: &str, label: &str, content: &str) -> AnyView {
    if super::super::inspect_library::is_archive(view_id) {
        return super::super::inspect_library::archive_list(content);
    }
    if super::super::inspect_library::is_model(view_id) {
        return model_view(model, content);
    }
    let mut rows = Vec::new();
    if let Some(bar) = document_toolbar(model, view_id, label) {
        rows.push(bar);
    }
    let nav = model.inspect.page_nav();
    if let Some(page) = super::page::page_bitmap(model) {
        rows.push(super::page::page_scroller(page, nav));
    } else if let Some(error) = model.inspect.raster_error() {
        rows.push(overlay("无法预览该文档", error, "inspect-native-error"));
    } else {
        let body = if content.is_empty() { label.to_string() } else { content.to_string() };
        rows.push(
            widget(Stack::fill_column(0.0).padding(18.0))
                .children((widget(text(&body, 13.0, 400, SemanticColorRole::Text, 20.15)).key("inspect-native"),))
                .key("inspect-native-text")
                .into_any(),
        );
    }
    widget(Stack::fill_column(0.0).min_height(LengthSpec::Px(0.0))).children(rows).key("inspect-native-view").into_any()
}

/// PDF 和 Office 才有种类、扩展名和页数这条顶栏；别的插件视图没有。
fn document_toolbar(model: &ShellViewModel, view_id: &str, label: &str) -> Option<AnyView> {
    if !super::super::inspect_library::is_office_pdf(view_id) {
        return None;
    }
    super::super::inspect_library::document_toolbar(model, label)
}

/// 3D 模型：画面铺满，左下角是扩展名和尺寸标签。旋转、缩放按钮在右上角。
fn model_view(model: &ShellViewModel, content: &str) -> AnyView {
    let extension = model.inspect.facts.extension.trim().trim_start_matches('.').to_ascii_uppercase();
    let mut hud = Vec::new();
    if !extension.is_empty() {
        hud.push(hud_chip(&extension, "inspect-model-hud-ext"));
    }
    let picture = match super::page::page_bitmap(model) {
        Some(page) => page,
        None => widget(Stack::fill_column(0.0).padding(18.0))
            .children((widget(text(content, 13.0, 400, SemanticColorRole::Muted, 20.15)),))
            .into_any(),
    };
    let mut layers = vec![picture];
    if !hud.is_empty() {
        layers.push(
            widget(Stack::row(8.0).with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                layout.offset_left = Some(LengthSpec::Px(12.0));
                layout.offset_bottom = Some(LengthSpec::Px(12.0));
            }))
            .children(hud)
            .key("inspect-model-hud")
            .into_any(),
        );
    }
    if model.inspect.has_mesh() {
        layers.push(orbit_controls());
    }
    widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background)).children(layers).key("inspect-model").into_any()
}

/// 模型的旋转和缩放。Vue 用拖拽手势，这里给同样功能的按钮。
fn orbit_controls() -> AnyView {
    let buttons = [("左转", -0.4, 1.0, "inspect-orbit-left"), ("右转", 0.4, 1.0, "inspect-orbit-right"), ("放大", 0.0, 1.25, "inspect-orbit-in"), ("缩小", 0.0, 0.8, "inspect-orbit-out")]
        .into_iter()
        .map(|(label, yaw, zoom, key)| {
            widget(nana_ui::runtime::Button::new(label).kind(nana_ui::ButtonKind::Ghost))
                .key(key)
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program(ShellMessage::Inspect(InspectMessage::Orbit { yaw, zoom }));
                })
                .into_any()
        })
        .collect::<Vec<_>>();
    widget(Stack::row(4.0).with_layout(|layout| {
        layout.position = nana_ui_core::PositionSpec::Absolute;
        layout.offset_right = Some(LengthSpec::Px(12.0));
        layout.offset_top = Some(LengthSpec::Px(12.0));
    }))
    .children(buttons)
    .key("inspect-orbit")
    .into_any()
}

/// 画面上的小标签：26px 高，bg-elev 底、border 细边、8px 圆角，12px 半粗弱色字。
fn hud_chip(label: &str, key: &'static str) -> AnyView {
    widget(
        Stack::row(0.0)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(26.0))
            .padding_xy(8.0, 4.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::Border, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((widget(text(label, 12.0, 600, SemanticColorRole::Muted, 18.6)),))
    .key(key)
    .into_any()
}

/// 读取中。文本、图片、模型是左上角的状态胶囊；音视频是居中的准备卡；PDF 保留顶栏。
fn loading_view(model: &ShellViewModel, kind: &PreviewKind) -> AnyView {
    let size = model.inspect.facts.size_label.trim().to_string();
    match kind {
        PreviewKind::Markdown | PreviewKind::Text => {
            let detail = if size.is_empty() { "准备文本内容".to_string() } else { format!("准备 {size}") };
            status_layer(status_pill("读取文本", &detail, false), None)
        }
        PreviewKind::Image => status_layer(status_pill("准备播放", "准备媒体", true), None),
        PreviewKind::Media => super::audio_stage::loading_card(model),
        PreviewKind::Native { view_id, label } => {
            let detail = if size.is_empty() { "建立预览".to_string() } else { format!("准备 {size}") };
            if super::super::inspect_library::is_archive(view_id) {
                let detail = if size.is_empty() { "准备内部目录".to_string() } else { format!("准备 {size}") };
                return centered_status("读取压缩包", &detail);
            }
            if super::super::inspect_library::is_model(view_id) {
                return status_layer(status_pill("读取模型", "准备读取文件", false), None);
            }
            let title = if super::super::inspect_library::is_pdf(view_id) {
                "载入 PDF"
            } else if super::super::inspect_library::is_office_pdf(view_id) {
                "转换文档"
            } else {
                "读取预览"
            };
            status_layer(status_pill(title, &detail, false), document_toolbar(model, view_id, label))
        }
        PreviewKind::Unsupported => fallback_view(model),
    }
}

/// 失败：浮层中间一行红色标题和一行弱色原因。PDF 保留顶栏。
fn failed_view(model: &ShellViewModel, kind: &PreviewKind, message: &str) -> AnyView {
    match kind {
        PreviewKind::Markdown | PreviewKind::Text => overlay("无法预览该文本", message, "inspect-failed"),
        PreviewKind::Image | PreviewKind::Media => overlay("无法预览该媒体", message, "inspect-failed"),
        PreviewKind::Native { view_id, label } => {
            if super::super::inspect_library::is_archive(view_id) {
                return overlay("无法预览该压缩包", message, "inspect-failed");
            }
            if super::super::inspect_library::is_model(view_id) {
                return overlay("无法预览该模型", message, "inspect-failed");
            }
            let mut rows = Vec::new();
            if let Some(bar) = document_toolbar(model, view_id, label) {
                rows.push(bar);
            }
            rows.push(overlay("无法预览该文档", message, "inspect-failed"));
            widget(Stack::fill_column(0.0)).children(rows).into_any()
        }
        PreviewKind::Unsupported => fallback_view(model),
    }
}

/// 没有预览插件：有缩略图就按 contain 画缩略图（bg 底），否则画类型图标。
/// 缩略图取自当前列表里同一路径的行；不在列表里的文件（例如从搜索打开）只画图标。
fn fallback_view(model: &ShellViewModel) -> AnyView {
    thumbnail_view(model).unwrap_or_else(|| fallback_icon(model))
}

fn thumbnail_view(model: &ShellViewModel) -> Option<AnyView> {
    let path = model.inspect.target_path.as_deref()?;
    // 素材详情说没有缩略图就不画；有的话像素和纹理取自列表里同一路径的行。
    model.inspect.facts.thumbnail_path.as_deref().map(str::trim).filter(|slot| !slot.is_empty())?;
    let files = &model.files;
    let row = files.rows.iter().chain(files.virtual_rows.iter()).find(|row| row.path == path)?;
    if let Some((width, height, rgba)) = row.thumbnail_rgba.as_ref() {
        return Some(pixel_image(*width, *height, rgba, false, "inspect-thumbnail"));
    }
    let slot = row.thumbnail_path.as_deref().map(str::trim).filter(|slot| !slot.is_empty())?;
    if !row.texture_ready {
        return None;
    }
    let mut thumb = Thumbnail::new(super::super::thumbs::thumbnail_slot(slot)).fit(ContentFit::Contain);
    {
        let layout = std::sync::Arc::make_mut(&mut thumb.style.layout);
        layout.width = Some(LengthSpec::Fill);
        layout.height = Some(LengthSpec::Fill);
    }
    Some(
        widget(Stack::fill_column(0.0).surface(SemanticColorRole::Background))
            .children((widget(thumb).key("inspect-thumbnail-texture"),))
            .key("inspect-thumbnail")
            .into_any(),
    )
}

/// 没有插件也没有缩略图时的类型图标：54px，弱色，居中。
fn fallback_icon(model: &ShellViewModel) -> AnyView {
    let extension = model.inspect.facts.extension.trim().trim_start_matches('.').to_ascii_lowercase();
    let icon = match extension.as_str() {
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" => icons::FILE_VIDEO,
        "mp3" | "wav" | "ogg" | "flac" | "m4a" | "aac" | "opus" => icons::FILE_AUDIO,
        _ => icons::FILE_IMAGE,
    };
    widget(Stack::fill_column(0.0).align(AlignSpec::Center).justify(JustifySpec::Center))
        .children((widget(IconGlyph::new(icon).size(54.0).role(SemanticColorRole::Faint)).key("inspect-fallback-icon"),))
        .key("inspect-fallback")
        .into_any()
}

/// 浮层：铺满预览框，标题红色、原因弱色，最宽 520px，居中。
fn overlay(title: &str, message: &str, key: &'static str) -> AnyView {
    let mut heading = text(title, 14.0, 700, SemanticColorRole::Danger, 21.7);
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    let mut reason = text(message, 14.0, 400, SemanticColorRole::Muted, 21.7);
    reason.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    {
        let layout = std::sync::Arc::make_mut(&mut reason.style.layout);
        layout.max_width = Some(LengthSpec::Px(520.0));
        layout.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
        layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    widget(Stack::fill_column(8.0).align(AlignSpec::Center).justify(JustifySpec::Center).padding(24.0))
        .children((widget(heading).key(key), widget(reason).key(format!("{key}-reason"))))
        .key(format!("{key}-overlay"))
        .into_any()
}

/// 居中的两行状态：压缩包读取中用它。
fn centered_status(title: &str, detail: &str) -> AnyView {
    widget(Stack::fill_column(8.0).align(AlignSpec::Center).justify(JustifySpec::Center))
        .children((
            widget(text(title, 14.0, 600, SemanticColorRole::Text, 21.7)),
            widget(text(detail, 14.0, 400, SemanticColorRole::Muted, 21.7)),
        ))
        .key("inspect-status-center")
        .into_any()
}

/// 状态胶囊盖在预览框左上角（PDF 在顶栏下面）。
fn status_layer(pill: AnyView, toolbar: Option<AnyView>) -> AnyView {
    let top = if toolbar.is_some() { 54.0 } else { 12.0 };
    let pill = widget(Stack::row(0.0).with_layout(move |layout| {
        layout.position = nana_ui_core::PositionSpec::Absolute;
        layout.offset_left = Some(LengthSpec::Px(12.0));
        layout.offset_top = Some(LengthSpec::Px(top));
        layout.max_width = Some(LengthSpec::CalcPercentOffset { percent: 100.0, offset_px: -24.0 });
    }))
    .children((pill,))
    .key("inspect-status-layer")
    .into_any();
    let mut rows = Vec::new();
    if let Some(toolbar) = toolbar {
        rows.push(toolbar);
    }
    rows.push(pill);
    widget(Stack::fill_column(0.0)).children(rows).key("inspect-loading").into_any()
}

/// 状态胶囊：bg-elev 底、border-soft 细边、8px 圆角，第一段正文色半粗。媒体多一条 132×6 的进度。
fn status_pill(title: &str, detail: &str, progress: bool) -> AnyView {
    let mut children = vec![
        widget(text(title, 12.0, 600, SemanticColorRole::Text, 18.6)).into_any(),
        widget(text(detail, 12.0, 400, SemanticColorRole::Muted, 18.6).truncating()).into_any(),
    ];
    if progress {
        children.push(super::audio_stage::progress_track(132.0, "inspect-status-progress"));
    }
    widget(
        Stack::row(8.0)
            .align(AlignSpec::Center)
            .padding_xy(8.0, 5.0)
            .surface(SemanticColorRole::Surface)
            .outline(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children(children)
    .key("inspect-status-pill")
    .into_any()
}

/// 和 Vue `formatByteCount` 一致：1024 以下写字节，其余保留一位小数。
pub(super) fn format_byte_count(value: u64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    if value < 1024 * 1024 {
        return format!("{:.1} KB", value as f64 / 1024.0);
    }
    format!("{:.1} MB", value as f64 / 1024.0 / 1024.0)
}
