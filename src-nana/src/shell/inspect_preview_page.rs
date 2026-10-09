//! PDF / Office 的页纸，对齐 office-preview 的 `office-preview__viewer`。
//!
//! 滚动区 18px 内边距，底部一层 bg-subtle 渐变；上面一行翻页（上一页、页码、下一页），
//! 下面是一张白纸：border-soft 细边、大投影，宽度铺满滚动区，高度按页面比例。一次只显示一页。

use nana_ui::runtime::view::{widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, Button, LengthSpec, ScrollAxes, ScrollView, SemanticColorRole, Stack};

use super::super::inspect::InspectMessage;
use super::super::ShellMessage;
use super::body::Pixels;
use super::frame::text;

/// 用页位图画白纸。高度跟宽高比走，超出时滚动区滚动，不裁掉页底。编码失败时是一张空白的纸位。
pub(super) fn page_bitmap(raster: &Pixels) -> AnyView {
    let (width, height) = raster.size();
    let Some(url) = super::super::inspect::rgba_png_data_url(width, height, raster.rgba()) else {
        eprintln!("Nana 页位图编码失败：{width}x{height}");
        return widget(Stack::column(0.0)).key("inspect-native-page").into_any();
    };
    let image = nana_ui_core::BackgroundImage::Url {
        url,
        fit: nana_ui_core::BackgroundImageFit::Length,
        size_width: Some(LengthSpec::Percent(100.0)),
        size_height: Some(LengthSpec::Percent(100.0)),
        position: nana_ui_core::BackgroundPosition::default(),
        repeat: nana_ui_core::BackgroundRepeat::NoRepeat,
        sampling: nana_ui_core::ImageSampling::Resample,
    };
    let aspect = width as f32 / height as f32;
    let frame = Stack::column(0.0)
        .width(LengthSpec::Fill)
        .min_width(LengthSpec::Px(0.0))
        .grow(0.0)
        .shrink(0.0)
        .outline(SemanticColorRole::BorderSoft, 1.0)
        .with_layout(|layout| {
            layout.aspect_ratio = Some(aspect);
            // 纸张在深浅主题里都是白的，和 `.office-preview__viewer--pdf canvas` 一致。
            layout.background = Some([1.0, 1.0, 1.0, 1.0]);
            layout.margin_bottom = Some(LengthSpec::Px(14.0));
            layout.paint.box_shadows = vec![nana_ui_core::BoxShadowSpec {
                paint_color: None,
                offset_x: 0.0,
                offset_y: 18.0,
                blur_radius: 48.0,
                spread_radius: 0.0,
                color: [0.0, 0.0, 0.0, 0.24],
                inset: false,
            }];
            layout.paint.content_image = Some(image);
        });
    widget(frame).key("inspect-native-page").into_any()
}

/// 滚动区：翻页在上，页纸在下。`key` 带文件和页码，翻页或换文件时回到顶部。
pub(super) fn page_scroller(page: AnyView, nav: Option<(usize, usize)>, key: String) -> AnyView {
    let mut children = Vec::new();
    if let Some((index, total)) = nav {
        children.push(page_nav(index, total));
    }
    children.push(page);
    let sheet = widget(Stack::column(0.0).padding(18.0).width(LengthSpec::Fill).min_width(LengthSpec::Px(0.0)))
        .key("inspect-page-sheet")
        .children(children);
    let scroll = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .key(key)
    .children((sheet,));
    // 滚动区背后一层 `office-preview__viewer--pdf` 的底：bg 上从底边往上 34% 淡出的 bg-subtle。
    widget(
        Stack::fill_column(0.0)
            .min_height(LengthSpec::Px(0.0))
            .painter(super::preview_paint::PageViewerBackdrop)
            .with_layout(|layout| layout.flex_basis = Some(LengthSpec::Px(0.0))),
    )
    .children((scroll,))
    .key("inspect-page-viewer")
    .into_any()
}

/// 上一页、「当前 / 总数」、下一页。首页和末页时对应按钮禁用。
pub(super) fn page_nav(index: usize, total: usize) -> AnyView {
    let total = total.max(1);
    widget(Stack::row(0.0).align(AlignSpec::Center))
        .key("inspect-page-nav")
        .children((
            widget(page_button("上一页", index == 0)).key("inspect-page-prev").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::TurnPage(-1)));
            }),
            widget(text(&format!("{} / {total}", index + 1), 14.0, 400, SemanticColorRole::Text, 21.7)).key("inspect-page-label"),
            widget(page_button("下一页", index + 1 >= total)).key("inspect-page-next").on_cx(|_, _: &Activate, cx| {
                cx.dispatch_program_all(ShellMessage::Inspect(InspectMessage::TurnPage(1)));
            }),
        ))
        .into_any()
}

/// 翻页按钮是基础按钮：32px 高、透明底、正文色。禁用时在页纸底色上淡化。
fn page_button(label: &str, disabled: bool) -> Button {
    let mut button = Button::new(label).kind(nana_ui::ButtonKind::Ghost).disabled(disabled);
    button.style.interaction.disabled = nana_ui::runtime::SemanticPaint::default();
    if disabled {
        button.style.interaction.base.foreground_mix = Some(nana_ui_core::SemanticColorMix::new(
            SemanticColorRole::Text,
            SemanticColorRole::Background,
            super::super::player_view::bar::DISABLED_OPACITY,
        ));
    }
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.height = Some(LengthSpec::Px(32.0));
    layout.min_height = Some(LengthSpec::Px(32.0));
    layout.padding_left = Some(LengthSpec::Px(10.0));
    layout.padding_right = Some(LengthSpec::Px(10.0));
    layout.font_size = Some(14.0);
    layout.font_weight = Some(500);
    layout.width = Some(LengthSpec::Shrink);
    button
}
