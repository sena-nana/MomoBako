//! 预览工具栏、图片与视频、文档与模型视图、覆盖层、状态层与字节数格式化等小部件。

use super::*;
use crate::theme_map::tiers::{self, Edge, Hairline};
use nana_ui_core::{SpacingStep, TextWeight};

use super::super::super::files_view::style as files_style;

/// 插件顶栏的最小高度 42：一行标签加上下内边距，不是间距档，照写像素。
const TOOLBAR_MIN_HEIGHT: f32 = 42.0;
/// 浮在预览框角上的小件离边的距离档。
const CORNER_STEP: SpacingStep = SpacingStep::Xl;
/// 失败原因的最大行宽 520：阅读宽度，不是间距，照写像素。
const REASON_MAX_WIDTH: f32 = 520.0;

/// 插件顶栏：42px 起，上下 `Md`、左右 `Xl` 内边距，底边 border-soft，标签可折行。
pub(super) fn toolbar(chips: Vec<AnyView>, key: &'static str) -> AnyView {
    let bar = super::super::super::workbench::with_bottom_divider(
        Stack::bar(0.0)
            .gap_theme(SpacingStep::Md)
            .wrap(true)
            .align(AlignSpec::Center)
            .min_height(LengthSpec::Px(TOOLBAR_MIN_HEIGHT))
            .with_layout(|layout| tiers::padding_xy(layout, SpacingStep::Xl, SpacingStep::Md)),
    );
    widget(bar).children(chips).key(key).into_any()
}

/// 顶栏标签：只显示的 NanaUI 标签，第一枚是种类，正文色半粗。
pub(super) fn chip(label: &str, emphasis: bool, key: &str) -> AnyView {
    inspect_library::label_tag(label, emphasis, key)
}

/// 把 RGBA 按 contain（或 cover）画进铺满的盒子，底色是 bg。
pub(super) fn pixel_image(pixels: &Pixels, cover: bool, key: &'static str) -> AnyView {
    frame_pixels(pixels, cover, key, false)
}

/// 宿主纹理槽上的视频画面，和文件预览的 `inspect-video` 同一棵。底是主题的媒体遮罩色，适应或填充跟 `cover`。
pub(in super::super) fn host_video(cover: bool) -> AnyView {
    let fit = if cover {
        ContentFit::Cover
    } else {
        ContentFit::Contain
    };
    widget(Stack::fill_column(0.0).painter(super::super::preview_paint::MediaBackdrop))
        .children((widget(GpuTextureView::new("file-preview").fit(fit)).key("inspect-video"),))
        .key("inspect-video-frame")
        .into_any()
}

/// 把一块像素编成内容图。`video` 时底色是主题的媒体遮罩色，否则是 bg。
/// 像素来自已经解出的缓冲区，编不成图时记日志并留空，不另涂一块同色矩形。
pub(super) fn frame_pixels(
    pixels: &Pixels,
    cover: bool,
    key: &'static str,
    video: bool,
) -> AnyView {
    let Some(url) =
        super::super::super::inspect::rgba_png_data_url(pixels.width, pixels.height, pixels.rgba())
    else {
        eprintln!("Nana 预览图编码失败：{}x{}", pixels.width, pixels.height);
        return widget(Stack::fill_column(0.0)).key(key).into_any();
    };
    let image = nana_ui_core::BackgroundImage::Url {
        url,
        fit: if cover {
            nana_ui_core::BackgroundImageFit::Cover
        } else {
            nana_ui_core::BackgroundImageFit::Contain
        },
        size_width: None,
        size_height: None,
        position: nana_ui_core::BackgroundPosition::center(),
        repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
        sampling: nana_ui_core::ImageSampling::Resample,
    };
    let stack = if video {
        Stack::fill_column(0.0).painter(super::super::preview_paint::MediaBackdrop)
    } else {
        Stack::fill_column(0.0).surface(SemanticColorRole::Background)
    };
    widget(stack.with_layout(|layout| {
        layout.paint.content_image = Some(image);
    }))
    .key(key)
    .into_any()
}

/// PDF 和 Office：顶栏、翻页和整页；读不出页面时是失败浮层或抽出的文字。
///
/// 画不出来的页也带翻页（`nav`）：翻页一行留在原来的位置，下面是这一页的失败原因，前后的页照样翻得到。
pub(super) fn document_view(
    toolbar: Option<&DocumentBar>,
    nav: Option<(usize, usize)>,
    page: Option<&Pixels>,
    error: Option<&str>,
    body: &str,
    scope: &str,
) -> AnyView {
    let mut rows = Vec::new();
    if let Some(bar) = toolbar {
        rows.push(inspect_library::document_toolbar_view(bar));
    }
    if let Some(raster) = page {
        let index = nav.map(|(index, _)| index).unwrap_or(0);
        let key = format!("preview-page-{scope}-{index}");
        rows.push(super::super::page::page_scroller(
            super::super::page::page_bitmap(raster),
            nav,
            key,
        ));
    } else if let Some(error) = error {
        if let Some((index, total)) = nav {
            rows.push(
                widget(crate::theme_map::tiers::pad_xy(
                    Stack::column(0.0)
                        .width(LengthSpec::Fill)
                        .min_width(LengthSpec::Px(0.0)),
                    SpacingStep::PageTight,
                    SpacingStep::PageTight,
                ))
                .children((super::super::page::page_nav(index, total),))
                .key(format!("inspect-page-failed-{scope}-{index}"))
                .into_any(),
            );
        }
        rows.push(overlay("无法预览该文档", error, "inspect-native-error"));
    } else {
        rows.push(
            widget(crate::theme_map::tiers::pad_xy(
                Stack::fill_column(0.0),
                SpacingStep::PageTight,
                SpacingStep::PageTight,
            ))
            .children((widget(text(
                body,
                13.0,
                TextWeight::Regular,
                SemanticColorRole::Text,
                20.15,
            ))
            .key("inspect-native"),))
            .key("inspect-native-text")
            .into_any(),
        );
    }
    // 插件根自带 bg 底，盖住预览框的插件光晕。
    widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .surface(SemanticColorRole::Background),
    )
    .children(rows)
    .key("inspect-native-view")
    .into_any()
}

/// 3D 模型：画面铺满，左下角是扩展名和尺寸标签。旋转、缩放按钮在右上角。
/// `extent` 是包围盒三轴尺寸，没有网格时为空，不编一个数。
pub(super) fn model_view(
    extension: &str,
    raster: Option<&Pixels>,
    content: &str,
    mesh: bool,
    extent: &str,
) -> AnyView {
    let extension = extension
        .trim()
        .trim_start_matches('.')
        .to_ascii_uppercase();
    let picture = match raster {
        Some(raster) => super::super::page::page_bitmap(raster),
        None => widget(crate::theme_map::tiers::pad_xy(
            Stack::fill_column(0.0),
            SpacingStep::PageTight,
            SpacingStep::PageTight,
        ))
        .children((widget(text(
            content,
            13.0,
            TextWeight::Regular,
            SemanticColorRole::Muted,
            20.15,
        )),))
        .into_any(),
    };
    let mut layers = vec![picture];
    let mut chips = Vec::new();
    if !extension.is_empty() {
        chips.push(hud_chip(&extension, "inspect-model-hud-ext"));
    }
    if !extent.is_empty() {
        chips.push(hud_chip(extent, "inspect-model-hud-size"));
    }
    if !chips.is_empty() {
        layers.push(
            widget(files_style::row(SpacingStep::Md).with_layout(|layout| {
                layout.position = nana_ui_core::PositionSpec::Absolute;
                crate::theme_map::tiers::offset(layout, &[Edge::Left, Edge::Bottom], CORNER_STEP);
            }))
            .children(chips)
            .key("inspect-model-hud")
            .into_any(),
        );
    }
    if mesh {
        layers.push(orbit_controls());
    }
    let surface = Stack::fill_column(0.0).surface(SemanticColorRole::Background);
    if !mesh {
        return widget(surface)
            .children(layers)
            .key("inspect-model")
            .into_any();
    }
    widget(surface.hittable())
        .children(layers)
        .key("inspect-model")
        .on_cx(orbit_on_drag())
        .into_any()
}

/// 在模型画面上拖拽：左右转动、上下缩放。NanaUI 的 `PointerDragged` 报从按下点起的总位移，
/// 这里记住上一采样，把两次采样之间的位移换成一次转动。按在旋转按钮上没拖开时按钮照常点按。
fn orbit_on_drag() -> impl FnMut(
    &mut Stack,
    &nana_ui::runtime::PointerDragged,
    &mut nana_ui::runtime::ViewContext<'_, Stack>,
) + 'static {
    use nana_ui::runtime::PointerDragPhase;
    let mut last = (0.0_f32, 0.0_f32);
    move |_, event, cx| match event.phase {
        PointerDragPhase::Started => last = (0.0, 0.0),
        PointerDragPhase::Moved | PointerDragPhase::Ended => {
            let (yaw, zoom) =
                crate::shell::inspect::orbit_drag(event.dx - last.0, event.dy - last.1);
            last = (event.dx, event.dy);
            if yaw.abs() > f32::EPSILON || (zoom - 1.0).abs() > 0.0001 {
                cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::Orbit { yaw, zoom }));
            }
        }
        PointerDragPhase::Cancelled => {}
    }
}

/// 模型的旋转和缩放，用按钮操作。
pub(super) fn orbit_controls() -> AnyView {
    let buttons = [
        ("左转", -0.4, 1.0, "inspect-orbit-left"),
        ("右转", 0.4, 1.0, "inspect-orbit-right"),
        ("放大", 0.0, 1.25, "inspect-orbit-in"),
        ("缩小", 0.0, 0.8, "inspect-orbit-out"),
    ]
    .into_iter()
    .map(|(label, yaw, zoom, key)| {
        widget(nana_ui::runtime::Button::new(label).kind(nana_ui::ButtonKind::Ghost))
            .key(key)
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::Orbit { yaw, zoom }));
            })
            .into_any()
    })
    .collect::<Vec<_>>();
    widget(files_style::row(SpacingStep::Xs).with_layout(|layout| {
        layout.position = nana_ui_core::PositionSpec::Absolute;
        crate::theme_map::tiers::offset(layout, &[Edge::Right, Edge::Top], CORNER_STEP);
    }))
    .children(buttons)
    .key("inspect-orbit")
    .into_any()
}

/// 画面上的小标签：NanaUI 的标签，浮在模型画面上，所以底换成 bg-elev；尺寸标签的底是 bg-elev 的 88%。
pub(super) fn hud_chip(label: &str, key: &'static str) -> AnyView {
    let frame = tag_frame();
    let frame = if key == "inspect-model-hud-size" {
        let mut style = frame
            .node_style()
            .surface_mix(nana_ui_core::SemanticColorMix::alpha(
                SemanticColorRole::Surface,
                0.88,
            ));
        style.background = None;
        frame.style(style)
    } else {
        frame.surface(SemanticColorRole::Surface)
    };
    inspect_library::label_tag_in(frame, label, false, key)
}

/// 失败浮层铺满预览框。底是 `color-mix(in srgb, var(--bg) 74%, transparent)`，失败时才画。
pub(super) fn overlay(title: &str, message: &str, key: &'static str) -> AnyView {
    failed_mask(title, message, key, None)
}

/// 在线条目的播放源准备失败：同一个浮层，原因下面多一个「重试」，点了重新准备播放源。
pub(super) fn retry_overlay(title: &str, message: &str) -> AnyView {
    let retry = widget(
        Button::new("重试")
            .kind(ButtonKind::Ghost)
            .icon(REFRESH)
            .icon_size(14.0),
    )
    .key("inspect-failed-retry")
    .on_cx(|_, _: &Activate, cx| {
        cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::RetryMedia))
    })
    .into_any();
    failed_mask(title, message, "inspect-failed", Some(retry))
}

/// 遮罩写在浮层上：背景色占 74%，其余透明。没有失败时不会进到这里。标题和原因仍是调用处的原文。
pub(super) fn failed_mask(
    title: &str,
    message: &str,
    key: &'static str,
    retry: Option<AnyView>,
) -> AnyView {
    let mut children = vec![
        widget(overlay_heading(title)).key(key).into_any(),
        widget(overlay_reason(message))
            .key(format!("{key}-reason"))
            .into_any(),
    ];
    if let Some(retry) = retry {
        children.push(
            widget(Stack::row(0.0).justify(JustifySpec::Center))
                .children((retry,))
                .into_any(),
        );
    }
    let frame = crate::theme_map::tiers::pad_xy(
        Stack::fill_column(0.0)
            .gap_theme(SpacingStep::Md)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center),
        SpacingStep::Page,
        SpacingStep::Page,
    );
    let style = frame
        .node_style()
        .surface_mix(nana_ui_core::SemanticColorMix::alpha(
            SemanticColorRole::Background,
            0.74,
        ));
    widget(frame.style(style))
        .children(children)
        .key(format!("{key}-overlay"))
        .into_any()
}

pub(super) fn overlay_heading(title: &str) -> Text {
    let mut heading = text(
        title,
        14.0,
        TextWeight::Bold,
        SemanticColorRole::Danger,
        21.7,
    );
    heading.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    heading
}

pub(super) fn overlay_reason(message: &str) -> Text {
    let mut reason = text(
        message,
        14.0,
        TextWeight::Regular,
        SemanticColorRole::Muted,
        21.7,
    );
    reason.style.text_horizontal_alignment = TextHorizontalAlignment::Center;
    {
        let layout = std::sync::Arc::make_mut(&mut reason.style.layout);
        layout.max_width = Some(LengthSpec::Px(REASON_MAX_WIDTH));
        layout.text_ext.line_break = Some(nana_ui_core::LineBreakSpec::Anywhere);
        layout.text_ext.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    }
    reason
}

/// 居中的两行状态：压缩包读取中用它。
pub(super) fn centered_status(title: &str, detail: &str) -> AnyView {
    widget(
        Stack::fill_column(0.0)
            .gap_theme(SpacingStep::Md)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center),
    )
    .children((
        widget(text(
            title,
            14.0,
            TextWeight::Semibold,
            SemanticColorRole::Text,
            21.7,
        )),
        widget(text(
            detail,
            14.0,
            TextWeight::Regular,
            SemanticColorRole::Muted,
            21.7,
        )),
    ))
    .key("inspect-status-center")
    .into_any()
}

/// 状态胶囊盖在预览框左上角，离边 `Xl`（PDF 在顶栏下面再离 `Xl`）。
pub(super) fn status_layer(pill: AnyView, toolbar: Option<AnyView>) -> AnyView {
    let below_toolbar = toolbar.is_some();
    let corner = tiers::space_px(CORNER_STEP);
    let pill = widget(Stack::row(0.0).with_layout(move |layout| {
        layout.position = nana_ui_core::PositionSpec::Absolute;
        crate::theme_map::tiers::offset(layout, &[Edge::Left, Edge::Top], CORNER_STEP);
        if below_toolbar {
            layout.offset_top = Some(LengthSpec::Px(TOOLBAR_MIN_HEIGHT + corner));
            layout.theme.offset_top = Some(nana_ui_core::ThemeExpr {
                constant: TOOLBAR_MIN_HEIGHT,
                terms: vec![(nana_ui_core::ThemeToken::Space(CORNER_STEP), 1.0)],
            });
        }
        layout.max_width = Some(LengthSpec::percent_offset(100.0, -corner * 2.0));
    }))
    .children((pill,))
    .key("inspect-status-layer")
    .into_any();
    let mut rows = Vec::new();
    if let Some(toolbar) = toolbar {
        rows.push(toolbar);
    }
    rows.push(pill);
    widget(Stack::fill_column(0.0))
        .children(rows)
        .key("inspect-loading")
        .into_any()
}

/// 状态胶囊：底是 bg-elev 的 84%，border-soft 发丝边，左右 `Md`、上下 `Sm`。图片读取多一条 132×6 的扫动进度。
pub(super) fn status_pill(title: &str, detail: &str, progress: bool) -> AnyView {
    let mut children = vec![
        widget(text(
            title,
            12.0,
            TextWeight::Semibold,
            SemanticColorRole::Text,
            18.6,
        ))
        .into_any(),
        widget(
            text(
                detail,
                12.0,
                TextWeight::Regular,
                SemanticColorRole::Muted,
                18.6,
            )
            .truncating(),
        )
        .into_any(),
    ];
    if progress {
        children.push(super::super::preview_paint::status_progress());
    }
    let frame = Hairline::hairline(
        crate::theme_map::tiers::pad_xy(
            files_style::row(SpacingStep::Md).align(AlignSpec::Center),
            SpacingStep::Md,
            SpacingStep::Sm,
        ),
        SemanticColorRole::BorderSoft,
    )
    .radius(RadiusTier::Md);
    let fill = nana_ui_core::SemanticColorMix::alpha(SemanticColorRole::Surface, 0.84);
    let style = frame.node_style().surface_mix(fill);
    widget(frame.style(style))
        .children(children)
        .key("inspect-status-pill")
        .into_any()
}

/// 1024 以下写字节，其余换成 KB 或 MB，保留一位小数。
pub(in super::super) fn format_byte_count(value: u64) -> String {
    if value < 1024 {
        return format!("{value} B");
    }
    if value < 1024 * 1024 {
        return format!("{:.1} KB", value as f64 / 1024.0);
    }
    format!("{:.1} MB", value as f64 / 1024.0 / 1024.0)
}
