//! 歌词列的上下留白。
//!
//! Vue `.media-preview__audio-info` 写 `padding: clamp(6px, 2vh, 24px) 0`：
//! 上下是这一截，左右是 0。`vh` 相对窗口高度，不相对预览区。
//! `@media (max-width: 820px)` 把整圈改成 0。
//! NanaUI 解析 padding 时不把视口传进去，视口单位会落成 0，
//! 所以上下做成独立的高度，由布局按窗口视口解析。高度还没量到时不落成 6 或 24。

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{AlignSpec, JustifySpec, LengthSpec, Stack};
use nana_ui_core::{CalcExpr, ViewportAxis};

/// `clamp` 的下限。
const PAD_MIN: f32 = 6.0;
/// `clamp` 的上限。
const PAD_MAX: f32 = 24.0;
/// `2vh`。
const PAD_VH: f32 = 2.0;

/// 窗口视口上的歌词列上下留白。
///
/// 宽度不大于 [`super::STACK_BREAKPOINT`] 时，CSS 把留白写成 0，不再看高度。
/// 更宽、而高度还没量到（非有限，或 ≤ 0）时返回 `None` 并记日志，不把结果说成 6 或 24。
/// 宽度还没量到时同样不夹。左右留白一直是 0，不在这个数里。
/// 列上的边条走 [`column_pad_spec`]，由布局视口解析；这条给已经拿到宽高的调用。
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn column_padding(viewport_width: f32, viewport_height: f32) -> Option<f32> {
    if !measured(viewport_width) {
        eprintln!(
            "Nana 歌词列宽度还没量到，留白不夹成 6 或 24：{viewport_width} x {viewport_height}"
        );
        return None;
    }
    if viewport_width <= super::STACK_BREAKPOINT {
        return Some(0.0);
    }
    if !measured(viewport_height) {
        eprintln!(
            "Nana 歌词列高度还没量到，留白不夹成 6 或 24：{viewport_width} x {viewport_height}"
        );
        return None;
    }
    Some((viewport_height * PAD_VH / 100.0).clamp(PAD_MIN, PAD_MAX))
}

/// 窗口的一边能拿来算断点或 `vh`。
fn measured(side: f32) -> bool {
    side.is_finite() && side > 0.0
}

/// 写进上下边条高度的长度。没有视口时解析是 `None`，不落成 6 或 24。
pub(super) fn column_pad_spec() -> LengthSpec {
    LengthSpec::from_calc(CalcExpr::Clamp {
        min: Box::new(CalcExpr::Px(PAD_MIN)),
        val: Box::new(CalcExpr::Viewport {
            axis: ViewportAxis::Height,
            value: PAD_VH,
        }),
        max: Box::new(CalcExpr::Px(PAD_MAX)),
    })
}

/// 歌词列。`lyrics` 是歌词面板。
///
/// 窗口宽于 820px 时，上下各留一条 [`column_pad_spec`]，左右不留。
/// 不宽于 820px 时不留这上下两条。列宽和行高按 Vue 的 minmax，见 [`super::split`]。
pub(super) fn info_view(stacked: bool, gap: f32, measured: bool, lyrics: AnyView) -> AnyView {
    let children = if stacked {
        vec![lyrics]
    } else {
        vec![
            pad_bar("inspect-audio-info-pad-top"),
            lyrics,
            pad_bar("inspect-audio-info-pad-bottom"),
        ]
    };
    widget(
        Stack::column(0.0)
            .align(AlignSpec::Stretch)
            .justify(JustifySpec::Start)
            .min_width(LengthSpec::Px(0.0))
            .min_height(LengthSpec::Px(0.0))
            .with_layout(move |layout| {
                super::split::pin_info(layout, stacked, gap, measured);
            }),
    )
    .children(children)
    .key("inspect-audio-info")
    .into_any()
}

/// 一条上下留白。高度是 `2vh` 的夹取，宽度跟着列走，不另留左右。
fn pad_bar(key: &'static str) -> AnyView {
    let size = column_pad_spec();
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .height(size)
            .min_height(size)
            .grow(0.0)
            .shrink(0.0),
    )
    .key(key)
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::{column_pad_spec, column_padding};
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    /// 宽于 820px 时歌词列上下是 `clamp(6px, 2vh, 24px)`，左右是 0。
    /// `vh` 跟窗口高度走，不跟预览区，也不跟宽度。不宽于 820px 时这圈是 0。
    /// 高度还没量到时不给出 6 或 24。
    #[test]
    fn audio_lyric_column_padding_follows_the_vue_vh() {
        let spec = column_pad_spec();
        assert!(
            spec.resolve_with(None, None).is_none(),
            "没有视口不能夹出像素"
        );
        assert!(
            spec.resolve_with(Some(10_000.0), None).is_none(),
            "只有父级百分比底、没有视口，也不能夹"
        );
        for (width, height) in [
            (0.0, 800.0),
            (f32::NAN, 800.0),
            (900.0, 0.0),
            (900.0, f32::NAN),
            (900.0, f32::INFINITY),
        ] {
            assert!(
                column_padding(width, height).is_none(),
                "还没量到：{width} x {height}"
            );
        }

        // 很矮：2%×250 = 5，停在下限 6。2%×300 = 6。
        assert_eq!(column_padding(1000.0, 250.0), Some(6.0));
        assert_eq!(column_padding(1000.0, 300.0), Some(6.0));
        // 400 高：2%×400 = 8。600 高：12。800 高：16。900 高：18。1100 高：22。
        assert_eq!(column_padding(1000.0, 400.0), Some(8.0));
        assert_eq!(column_padding(1500.0, 600.0), Some(12.0));
        assert_eq!(column_padding(1200.0, 800.0), Some(16.0));
        assert_eq!(column_padding(1000.0, 900.0), Some(18.0));
        assert_eq!(column_padding(900.0, 1100.0), Some(22.0));
        // 很高：2%×1400 = 28、2%×2000 = 40，都夹到 24。
        assert_eq!(column_padding(1000.0, 1400.0), Some(24.0));
        assert_eq!(column_padding(2000.0, 2000.0), Some(24.0));
        // 窄窗口和正好 820 都不留，哪怕高度会把宽公式夹到 24。
        assert_eq!(column_padding(700.0, 2000.0), Some(0.0));
        assert_eq!(column_padding(820.0, 2000.0), Some(0.0));
        assert_eq!(
            column_padding(700.0, f32::NAN),
            Some(0.0),
            "不宽于 820px 时不看高度，也不落成 6 或 24"
        );

        assert_laid_out(1000.0, 400.0, 8.0);
        assert_laid_out(1500.0, 600.0, 12.0);
        assert_laid_out(1200.0, 800.0, 16.0);
        assert_laid_out(1000.0, 900.0, 18.0);
        assert_laid_out(900.0, 1100.0, 22.0);
        assert_laid_out(1000.0, 1400.0, 24.0);
        assert_laid_out(1600.0, 900.0, 18.0);

        assert_flush(700.0, 800.0);
        assert_flush(820.0, 800.0);
    }

    /// 上边和下边都是这个留白，左右没有。父级百分比底换大也不变。
    fn assert_laid_out(width: f32, height: f32, padding: f32) {
        let spec = column_pad_spec();
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
            "留白不该跟着预览区的百分比走"
        );
        assert_eq!(column_padding(width, height), Some(padding));

        let harness = ShellHarness::mount_at(scene(), width, height);
        let info = box_of(&harness, "inspect-audio-info");
        let top = box_of(&harness, "inspect-audio-info-pad-top");
        let bottom = box_of(&harness, "inspect-audio-info-pad-bottom");
        let lyrics = box_of(&harness, "inspect-audio-lyrics");
        assert!(
            harness.keyed("inspect-audio-info-pad-left").is_none(),
            "左右不留"
        );
        assert!(
            (top.height - padding).abs() < 0.5 && (bottom.height - padding).abs() < 0.5,
            "视口高 {height} 的上下应是 {padding}，实际上 {top:?} 下 {bottom:?}"
        );
        assert!((top.y - info.y).abs() < 0.5, "上留白贴着列顶");
        assert!(
            (bottom.y + bottom.height - (info.y + info.height)).abs() < 0.5,
            "下留白贴着列底"
        );
        assert!(
            (lyrics.y - (info.y + padding)).abs() < 0.5,
            "歌词面板从上留白下面开始"
        );
        assert!(
            (lyrics.x - info.x).abs() < 0.5 && (lyrics.width - info.width).abs() < 0.5,
            "左右留白是 0，歌词面板和列一样宽"
        );
        assert!(
            info.height + 1.0 < height,
            "列比窗口矮，留白才不是按预览区高度的 2%"
        );
        let style = node_style(&harness, "inspect-audio-info");
        assert!(
            style.layout.padding_top.is_none(),
            "留白在上下两条边上，不写进 padding"
        );
        assert!(style.layout.padding_left.is_none(), "左右不写 padding");
    }

    /// 不宽于 820px 时列和歌词面板贴齐，没有上下边条。
    fn assert_flush(width: f32, height: f32) {
        assert_eq!(column_padding(width, height), Some(0.0));
        let harness = ShellHarness::mount_at(scene(), width, height);
        assert!(
            harness.keyed("inspect-audio-info").is_some(),
            "窄窗口仍有歌词列"
        );
        assert!(
            harness.keyed("inspect-audio-info-pad-top").is_none(),
            "不宽于 820px 时不留上边"
        );
        assert!(
            harness.keyed("inspect-audio-info-pad-bottom").is_none(),
            "不宽于 820px 时不留下边"
        );
        let info = box_of(&harness, "inspect-audio-info");
        let lyrics = box_of(&harness, "inspect-audio-lyrics");
        assert!(
            (lyrics.y - info.y).abs() < 0.5 && (lyrics.x - info.x).abs() < 0.5,
            "窄列不留白"
        );
        assert!(
            (lyrics.height - info.height).abs() < 0.5 && (lyrics.width - info.width).abs() < 0.5,
            "窄列不留白"
        );
        let style = node_style(&harness, "inspect-audio-info");
        assert!(style.layout.padding_top.is_none(), "窄列不能再写一圈内边距");
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

    fn scene() -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "preview-audio")
            .unwrap_or_else(|| panic!("没有场景 preview-audio"))
            .1
    }
}
