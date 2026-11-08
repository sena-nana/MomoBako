//! 音频舞台的内边距。
//!
//! Vue `.media-preview__audio-stage` 在窗口宽于 820px 时写
//! `padding: clamp(12px, 3vh, 30px)`，`@media (max-width: 820px)` 时改成 0。
//! `vh` 相对窗口高度。NanaUI 解析 padding 时不把视口传进去，视口单位会落成 0，
//! 所以四边做成独立的宽高，由布局按窗口视口解析。高度还没量到时不落成 12 或 30。

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{AlignSpec, JustifySpec, LengthSpec, Stack};
use nana_ui_core::{CalcExpr, ViewportAxis};

/// `clamp` 的下限。
const PAD_MIN: f32 = 12.0;
/// `clamp` 的上限。
const PAD_MAX: f32 = 30.0;
/// `3vh`。
const PAD_VH: f32 = 3.0;

/// 窗口高度上的舞台内边距。
///
/// 高度还没量到（非有限，或 ≤ 0）时返回 `None` 并记日志，不把结果说成 12 或 30。
/// 舞台上的边条走 [`stage_pad_spec`]，由布局视口解析；这条给已经拿到高度的调用。
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn stage_padding(viewport_height: f32) -> Option<f32> {
    if !viewport_height.is_finite() || viewport_height <= 0.0 {
        eprintln!("Nana 音频舞台高度还没量到，内边距不夹：{viewport_height}");
        return None;
    }
    Some((viewport_height * PAD_VH / 100.0).clamp(PAD_MIN, PAD_MAX))
}

/// 唱片相对舞台边框顶的距离。
///
/// 唱片钉在内容顶，这段距离就是舞台已有的内边距。
/// 宽窗口的内边距要视口高度；窄窗口内边距写成 0。
/// 高度还没量到时返回 `None` 并记日志，不编一个垂直位置。
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn record_top(viewport_height: f32, narrow: bool) -> Option<f32> {
    if !viewport_height.is_finite() || viewport_height <= 0.0 {
        eprintln!("Nana 音频舞台高度还没量到，唱片不编垂直位置：{viewport_height}");
        return None;
    }
    if narrow {
        Some(0.0)
    } else {
        stage_padding(viewport_height)
    }
}

/// 写进边条宽高的长度。没有视口时解析是 `None`，不落成固定像素。
pub(super) fn stage_pad_spec() -> LengthSpec {
    LengthSpec::from_calc(CalcExpr::Clamp {
        min: Box::new(CalcExpr::Px(PAD_MIN)),
        val: Box::new(CalcExpr::Viewport {
            axis: ViewportAxis::Height,
            value: PAD_VH,
        }),
        max: Box::new(CalcExpr::Px(PAD_MAX)),
    })
}

/// 唱片和标题所在的舞台。
///
/// Vue 写成两行网格：`grid-template-rows: var(--audio-record-size) minmax(0, auto)`，
/// `align-items: center`，行距 18px。第一行高就是唱片直径，唱片钉在内容顶。
/// 第二行吃掉剩下的高度，标题在这一行里居中，不和唱片捆成一组再垂直居中。
/// `@media (max-width: 820px)` 只改直径和 `padding: 0`，顶对齐是同一套。
/// 窗口宽于 820px 时，四边各留一条 [`stage_pad_spec`]。不宽于 820px 时不留这四边。
pub(super) fn stage_view(
    stacked: bool,
    gap: f32,
    measured: bool,
    record: AnyView,
    caption: AnyView,
) -> AnyView {
    let body = widget(
        Stack::column(18.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Start)
            // `Shrink` 会保住内容高，宽舞台那一行的拉伸拉不动它，第二行就吃不到剩余高度。
            .height(LengthSpec::Auto)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children((record, caption_row(caption)))
    .key("inspect-audio-stage-body")
    .into_any();
    let (stack, children) = if stacked {
        (
            Stack::column(0.0)
                .align(AlignSpec::Center)
                .justify(JustifySpec::Start)
                .padding(0.0),
            vec![body],
        )
    } else {
        let middle = widget(
            Stack::row(0.0)
                .align(AlignSpec::Stretch)
                .min_width(LengthSpec::Px(0.0))
                .min_height(LengthSpec::Px(0.0))
                .grow(1.0)
                .shrink(1.0),
        )
        .children((
            pad_bar("inspect-audio-stage-pad-left", false),
            body,
            pad_bar("inspect-audio-stage-pad-right", false),
        ))
        .key("inspect-audio-stage-row")
        .into_any();
        (
            Stack::column(0.0)
                .align(AlignSpec::Stretch)
                .justify(JustifySpec::Start),
            vec![
                pad_bar("inspect-audio-stage-pad-top", true),
                middle,
                pad_bar("inspect-audio-stage-pad-bottom", true),
            ],
        )
    };
    widget(
        stack
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(move |layout| {
                super::split::pin_stage(layout, stacked, gap, measured);
            }),
    )
    .children(children)
    .key("inspect-audio-stage")
    .into_any()
}

/// 标题所在的第二行。
///
/// 行高是唱片下面的剩余空间（Vue 第二行 `minmax(0, auto)` 在默认 `align-content` 下被拉高）。
/// 标题作为这一行的内容居中。行本身从唱片下沿再隔 18px 排起，所以标题不会回到唱片旁边。
fn caption_row(caption: AnyView) -> AnyView {
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Center)
            .justify(JustifySpec::Center)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .grow(1.0)
            .shrink(1.0),
    )
    .children((caption,))
    .key("inspect-audio-stage-caption-row")
    .into_any()
}

/// 一条内边距。`along_height` 时高度是 `3vh` 的夹取，否则宽度是同一条长度。
fn pad_bar(key: &'static str, along_height: bool) -> AnyView {
    let size = stage_pad_spec();
    let bar = if along_height {
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .height(size)
            .min_height(size)
            .grow(0.0)
            .shrink(0.0)
    } else {
        Stack::column(0.0)
            .height(LengthSpec::Fill)
            .width(size)
            .min_width(size)
            .grow(0.0)
            .shrink(0.0)
    };
    widget(bar).key(key).into_any()
}

#[cfg(test)]
mod tests {
    use nana_ui::runtime::{JustifySpec, LengthSpec};

    use super::{record_top, stage_pad_spec, stage_padding};
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    /// 宽舞台四边都是 `clamp(12px, 3vh, 30px)`，跟窗口高度走，不跟宽度。
    /// 窗口不宽于 820px 时不留这四边。高度还没量到时不给出像素。
    #[test]
    fn audio_stage_padding_follows_the_vue_vh() {
        let spec = stage_pad_spec();
        assert!(
            spec.resolve_with(None, None).is_none(),
            "没有视口不能夹出像素"
        );
        assert!(
            spec.resolve_with(Some(10_000.0), None).is_none(),
            "只有父级百分比底、没有视口，也不能夹"
        );
        for height in [0.0, -8.0, f32::NAN, f32::INFINITY] {
            assert!(stage_padding(height).is_none(), "还没量到：{height}");
        }

        // 很矮：3%×400 = 12，停在下限。左右也是 12，不是 3vw。
        assert_eq!(stage_padding(400.0), Some(12.0));
        assert_laid_out(1000.0, 400.0, 12.0);
        // 800 高：3%×800 = 24，和原来写死的 24 重合。
        assert_eq!(stage_padding(800.0), Some(24.0));
        assert_laid_out(1200.0, 800.0, 24.0);
        // 中间：3%×900 = 27。
        assert_eq!(stage_padding(900.0), Some(27.0));
        assert_laid_out(1000.0, 900.0, 27.0);
        // 很高：3%×2000 = 60，夹到 30。
        assert_eq!(stage_padding(2000.0), Some(30.0));
        assert_laid_out(1600.0, 2000.0, 30.0);

        let narrow = ShellHarness::mount_at(scene(), 700.0, 800.0);
        assert!(
            narrow.keyed("inspect-audio-stage").is_some(),
            "窄窗口仍有舞台"
        );
        assert!(
            narrow.keyed("inspect-audio-stage-pad-top").is_none(),
            "不宽于 820px 时不留上边"
        );
        assert!(
            narrow.keyed("inspect-audio-stage-pad-left").is_none(),
            "不宽于 820px 时不留左边"
        );
        let style = node_style(&narrow, "inspect-audio-stage");
        assert_eq!(
            style.layout.padding_top,
            Some(LengthSpec::Px(0.0)),
            "窄舞台不能再写一圈内边距"
        );
    }

    /// 上边的高和左边的宽都是这个内边距。父级百分比底换大也不变。
    fn assert_laid_out(width: f32, height: f32, padding: f32) {
        let spec = stage_pad_spec();
        let from_viewport = spec
            .resolve_with(None, Some((width, height)))
            .expect("视口已量到");
        let ignoring_parent = spec
            .resolve_with(Some(10_000.0), Some((width, height)))
            .expect("不看父级");
        assert!(
            (from_viewport - padding).abs() < 0.05,
            "视口 {width} x {height} 解析成 {from_viewport}，应是 {padding}"
        );
        assert!(
            (ignoring_parent - from_viewport).abs() < 0.05,
            "内边距不该跟着预览区的百分比走"
        );
        assert_eq!(stage_padding(height), Some(padding));

        let harness = ShellHarness::mount_at(scene(), width, height);
        let top = box_of(&harness, "inspect-audio-stage-pad-top");
        let left = box_of(&harness, "inspect-audio-stage-pad-left");
        assert!(
            (top.height - padding).abs() < 0.5,
            "视口高 {height} 的上边应是 {padding}，实际 {top:?}"
        );
        assert!(
            (left.width - padding).abs() < 0.5,
            "左边应跟高度走成 {padding}，不是 3vw，实际 {left:?}"
        );
        let stage = node_style(&harness, "inspect-audio-stage");
        assert!(
            stage.layout.padding_top.is_none(),
            "宽舞台的内边距在四条边上，不写进 padding"
        );
    }

    fn box_of(harness: &ShellHarness, key: &str) -> nana_ui::runtime::LayoutBox {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局"))
    }

    fn node_style(harness: &ShellHarness, key: &str) -> nana_ui::runtime::NodeStyle {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .node_style(id)
            .unwrap_or_else(|| panic!("{key} 没有样式"))
            .clone()
    }

    /// 有视口时唱片贴着舞台内容顶，距离就是已经留出的内边距。
    /// 窄窗口和宽窗口同一套顶对齐：唱片在第一行，标题在唱片下面。
    /// 不是把唱片和标题捆在一起，在剩余高度里垂直居中。
    /// 高度还没量到时不编一个垂直位置。
    #[test]
    fn audio_record_pins_to_the_stage_content_top() {
        for height in [0.0, -8.0, f32::NAN, f32::INFINITY] {
            assert!(
                record_top(height, false).is_none(),
                "宽窗口还没量到：{height}"
            );
            assert!(
                record_top(height, true).is_none(),
                "窄窗口还没量到：{height}"
            );
        }
        // 宽窗口。1400×1600：内边距夹到 30，舞台比唱片高一截。
        assert_eq!(record_top(1600.0, false), Some(30.0));
        assert_pinned(1400.0, 1600.0);
        // 窄窗口不留内边距。700×1600 仍从内容顶排，不另写一个垂直位置。
        assert_eq!(record_top(1600.0, true), Some(0.0));
        assert_pinned(700.0, 1600.0);
    }

    /// 挂上窗口后，唱片的顶等于舞台顶加 [`record_top`]。标题在唱片下面。
    fn assert_pinned(width: f32, height: f32) {
        let narrow = width <= super::super::STACK_BREAKPOINT;
        let inset = record_top(height, narrow).expect("视口已量到");
        let harness = ShellHarness::mount_at(scene(), width, height);
        let stage = box_of(&harness, "inspect-audio-stage");
        let record = box_of(&harness, "inspect-audio-record");
        let title = box_of(&harness, "inspect-audio-title");
        let caption = box_of(&harness, "inspect-audio-caption");
        let row = box_of(&harness, "inspect-audio-stage-caption-row");
        let body = node_style(&harness, "inspect-audio-stage-body");
        assert_eq!(
            body.layout.justify_content,
            JustifySpec::Start,
            "唱片从内容顶排起，不整组居中"
        );
        let stage_justify = node_style(&harness, "inspect-audio-stage");
        assert_eq!(
            stage_justify.layout.justify_content,
            JustifySpec::Start,
            "舞台本身也从顶排"
        );

        let (content_top, content_bottom) = if narrow {
            assert!(
                harness.keyed("inspect-audio-stage-pad-top").is_none(),
                "窄窗口没有上边"
            );
            (stage.y, stage.y + stage.height)
        } else {
            let top = box_of(&harness, "inspect-audio-stage-pad-top");
            let bottom = box_of(&harness, "inspect-audio-stage-pad-bottom");
            assert!(
                (top.height - inset).abs() < 0.5,
                "上边应是已有内边距 {inset}，实际 {}",
                top.height
            );
            assert!((top.y - stage.y).abs() < 0.5, "上边贴着舞台顶");
            (top.y + top.height, bottom.y)
        };
        assert!(
            (content_top - (stage.y + inset)).abs() < 1.0,
            "内容顶应是舞台顶加内边距 {inset}，实际 {content_top}，舞台顶 {}",
            stage.y
        );
        assert!(
            (record.y - content_top).abs() < 1.0,
            "视口 {width} x {height} 的唱片应贴着内容顶 {content_top}，实际 {}",
            record.y
        );
        let group = record.height + 18.0 + caption.height;
        let centered = content_top + (content_bottom - content_top - group) / 2.0;
        assert!(
            (record.y - centered).abs() > 24.0,
            "唱片不该和标题整组居中：内容顶 {content_top}，居中会在 {centered}，实际 {}",
            record.y
        );
        assert!(
            title.y + 0.5 >= record.y + record.height,
            "标题应在唱片下面：标题顶 {}，唱片底 {}",
            title.y,
            record.y + record.height
        );
        let row_top = record.y + record.height + 18.0;
        assert!(
            (row.y - row_top).abs() < 1.5,
            "第二行应从唱片下沿再隔 18px 排起：期望 {row_top}，实际 {}",
            row.y
        );
        assert!(
            (row.y + row.height - content_bottom).abs() < 2.0,
            "第二行应吃掉内容底之前的剩余高度：行底 {}，内容底 {content_bottom}",
            row.y + row.height
        );
        let caption_in_row = row.y + (row.height - caption.height) / 2.0;
        assert!(
            (caption.y - caption_in_row).abs() < 1.5,
            "标题应在第二行里居中：期望 {caption_in_row}，实际 {}",
            caption.y
        );
        assert!(
            (title.width - record.width).abs() < 0.5,
            "标题宽度仍用唱片直径：标题 {}，唱片 {}",
            title.width,
            record.width
        );
    }

    fn scene() -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "preview-audio")
            .unwrap_or_else(|| panic!("没有场景 preview-audio"))
            .1
    }
}
