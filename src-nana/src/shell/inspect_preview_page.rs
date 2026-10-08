//! PDF 页纸。翻页在页图上方，页宽按比例撑开，超出视口时滚动。

use nana_ui::runtime::view::{button, text, widget, AnyView, IntoView};
use nana_ui::runtime::{Activate, AlignSpec, LengthSpec, ScrollAxes, ScrollView, SemanticColorRole, Stack};

use super::super::inspect::InspectMessage;
use super::super::{ShellViewModel};
use super::inspect_message;

/// 用页位图按页宽撑开。高度跟宽高比走，不把页底裁在视口外。
pub(super) fn page_bitmap(model: &ShellViewModel) -> Option<AnyView> {
    let (width, height, rgba) = model.inspect.raster_frame()?;
    if width == 0 || height == 0 {
        return None;
    }
    let url = super::super::inspect::rgba_png_data_url(width, height, rgba)?;
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
        .surface(SemanticColorRole::Background)
        .outline(SemanticColorRole::Border, 1.0)
        .with_layout(|layout| {
            layout.aspect_ratio = Some(aspect);
            // 与 `.office-preview__viewer--pdf canvas` 的投影一致。
            layout.paint.box_shadows = vec![nana_ui_core::BoxShadowSpec {
                offset_x: 0.0,
                offset_y: 18.0,
                blur_radius: 48.0,
                spread_radius: 0.0,
                color: [0.0, 0.0, 0.0, 0.24],
                inset: false,
            }];
            layout.paint.content_image = Some(image);
        });
    Some(widget(frame).key("inspect-native-page").into_any())
}

/// 预览列可滚动。18px 内边距里，翻页在上，页纸在下，页纸不被翻页盖住。
pub(super) fn page_scroller(page: AnyView, nav: Option<(usize, usize)>) -> AnyView {
    let mut children = Vec::new();
    if let Some((index, total)) = nav {
        children.push(page_nav(index, total));
    }
    children.push(page);
    let sheet = widget(
        Stack::column(8.0)
            .padding_xy(18.0, 18.0)
            .width(LengthSpec::Fill)
            .min_width(LengthSpec::Px(0.0)),
    )
    .key("inspect-page-sheet")
    .children(children);
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|layout| {
        layout.flex_grow = Some(1.0);
        layout.flex_shrink = Some(1.0);
        layout.flex_basis = Some(LengthSpec::Px(0.0));
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.width = Some(LengthSpec::Fill);
    }))
    .key("inspect-page-scroll")
    .children((sheet,))
    .into_any()
}

/// PDF 插件把上一页、页码和下一页放在页图上方，写在页面之内。
pub(super) fn page_nav(index: usize, total: usize) -> AnyView {
    let row = Stack::row(8.0).align(AlignSpec::Center);
    widget(row).key("inspect-page-nav").children((
        button("上一页").key("inspect-page-prev").disabled(index == 0).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::TurnPage(-1)));
        }),
        text(format!("{} / {total}", index + 1)).key("inspect-page-label"),
        button("下一页").key("inspect-page-next").disabled(index + 1 >= total).on_cx(|_, _: &Activate, cx| {
            cx.dispatch_program(inspect_message(InspectMessage::TurnPage(1)));
        }),
    )).into_any()
}
