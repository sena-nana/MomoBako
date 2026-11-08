//! 音频舞台唱片的直径。
//!
//! Vue `.media-preview__audio-stage` 在窗口宽于 820px 时写
//! `--audio-record-size: clamp(220px, min(32vw, 48vh), 380px)`。
//! `@media (max-width: 820px)` 时改成 `clamp(140px, min(34vh, 56vw), 230px)`。
//! `vw` / `vh` 相对窗口视口，不相对预览区。宽或高还没量到时不给出像素。

use nana_ui::runtime::LengthSpec;
use nana_ui_core::{CalcExpr, ViewportAxis};

/// 宽窗口 `clamp` 的下限。
const RECORD_MIN: f32 = 220.0;
/// 宽窗口 `clamp` 的上限。
const RECORD_MAX: f32 = 380.0;
/// 宽窗口 `32vw`。
const RECORD_VW: f32 = 32.0;
/// 宽窗口 `48vh`。
const RECORD_VH: f32 = 48.0;
/// 窄窗口 `clamp` 的下限。`@media (max-width: 820px)`。
const NARROW_MIN: f32 = 140.0;
/// 窄窗口 `clamp` 的上限。
const NARROW_MAX: f32 = 230.0;
/// 窄窗口 `56vw`。
const NARROW_VW: f32 = 56.0;
/// 窄窗口 `34vh`。
const NARROW_VH: f32 = 34.0;

/// 窗口视口上的唱片直径。
///
/// 宽度不大于 [`super::STACK_BREAKPOINT`]（Vue `max-width: 820px`）时用窄公式，更宽用宽公式。
/// 一边还没量到（非有限，或 ≤ 0）时返回 `None` 并记日志，不把结果说成 140、230、220 或 380。
/// 唱片节点走 [`record_size`]，由布局视口解析；这条给已经拿到宽高的调用。
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn record_diameter(viewport_width: f32, viewport_height: f32) -> Option<f32> {
    if !viewport_measured(viewport_width) || !viewport_measured(viewport_height) {
        eprintln!(
            "Nana 唱片视口还没量到，直径不夹成 140、230、220 或 380：{viewport_width} x {viewport_height}"
        );
        return None;
    }
    let (min_px, max_px, vw, vh) = if viewport_width <= super::STACK_BREAKPOINT {
        (NARROW_MIN, NARROW_MAX, NARROW_VW, NARROW_VH)
    } else {
        (RECORD_MIN, RECORD_MAX, RECORD_VW, RECORD_VH)
    };
    let preferred = (viewport_width * vw / 100.0).min(viewport_height * vh / 100.0);
    Some(preferred.clamp(min_px, max_px))
}

/// 窗口视口的一边能拿来算 `vw` / `vh`。
fn viewport_measured(side: f32) -> bool {
    side.is_finite() && side > 0.0
}

/// 写进唱片宽高的长度。`narrow` 时是 `@media (max-width: 820px)` 那套。
/// 没有视口时解析是 `None`，宽窗口不落成 220 或 380，窄窗口不落成 140 或 230。
pub(super) fn record_size(narrow: bool) -> LengthSpec {
    if narrow {
        // `min(34vh, 56vw)`，和 Vue 的书写顺序一致。
        clamp_size(
            NARROW_MIN,
            ViewportAxis::Height,
            NARROW_VH,
            ViewportAxis::Width,
            NARROW_VW,
            NARROW_MAX,
        )
    } else {
        clamp_size(
            RECORD_MIN,
            ViewportAxis::Width,
            RECORD_VW,
            ViewportAxis::Height,
            RECORD_VH,
            RECORD_MAX,
        )
    }
}

/// `clamp(下限, min(第一条视口, 第二条视口), 上限)`。
fn clamp_size(
    min_px: f32,
    first_axis: ViewportAxis,
    first: f32,
    second_axis: ViewportAxis,
    second: f32,
    max_px: f32,
) -> LengthSpec {
    LengthSpec::from_calc(CalcExpr::Clamp {
        min: Box::new(CalcExpr::Px(min_px)),
        val: Box::new(CalcExpr::Min(
            Box::new(CalcExpr::Viewport {
                axis: first_axis,
                value: first,
            }),
            Box::new(CalcExpr::Viewport {
                axis: second_axis,
                value: second,
            }),
        )),
        max: Box::new(CalcExpr::Px(max_px)),
    })
}

#[cfg(test)]
mod tests {
    use nana_ui::runtime::{LayoutBox, PaintOp, PaintRecording, PaintState};

    use super::{record_diameter, record_size};
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    /// 宽于 820px 时唱片边长是 `clamp(220px, min(32vw, 48vh), 380px)`，
    /// 不宽于 820px 时是 `clamp(140px, min(34vh, 56vw), 230px)`。
    /// 单位是窗口的 `vw` / `vh`，不跟预览区的百分比走。宽或高还没量到时不给出直径，也不落成 140 或 230。
    /// 宽公式在 600×400 仍是 220；这个宽度已经不宽于 820，挂上窗口后走窄公式。
    #[test]
    fn audio_record_diameter_follows_the_vue_clamp() {
        let wide = record_size(false);
        assert!(
            wide.resolve_with(None, None).is_none(),
            "没有视口不能夹出像素"
        );
        assert!(
            wide.resolve_with(Some(10_000.0), None).is_none(),
            "只有父级百分比底、没有视口，也不能夹"
        );
        let narrow = record_size(true);
        assert!(
            narrow.resolve_with(None, None).is_none(),
            "窄窗口没有视口不能落成 140 或 230"
        );
        assert!(
            narrow.resolve_with(Some(10_000.0), None).is_none(),
            "窄窗口只有父级百分比底、没有视口，也不能落成 140 或 230"
        );
        for (width, height) in [
            (0.0, 800.0),
            (800.0, 0.0),
            (f32::NAN, 800.0),
            (800.0, f32::INFINITY),
        ] {
            assert!(
                record_diameter(width, height).is_none(),
                "还没量到：{width} x {height}"
            );
        }

        // 宽公式本身，不看断点。这些数必须仍是原来的夹取。
        assert_formula(false, 600.0, 400.0, 220.0);
        assert_formula(false, 2000.0, 1600.0, 380.0);
        assert_formula(false, 1000.0, 900.0, 320.0);
        assert_formula(false, 1500.0, 600.0, 288.0);

        // 窗口宽于 820，画出来的格子就是宽公式。落影仍是 0 22px 60px。
        assert_laid_out(1000.0, 900.0, 320.0);
        assert_laid_out(1500.0, 600.0, 288.0);
        assert_laid_out(2000.0, 1600.0, 380.0);

        // 不宽于 820。600×400 的宽公式仍是 220，窗口本身：min(136, 336) = 136，夹到 140。
        assert_laid_out(600.0, 400.0, 140.0);
        // 高更紧：min(34%×500, 56%×700) = min(170, 392) = 170。
        assert_laid_out(700.0, 500.0, 170.0);
        // 宽更紧：min(34%×700, 56%×400) = min(238, 224) = 224。
        assert_laid_out(400.0, 700.0, 224.0);
        // 很大：min(34%×900, 56%×800) = min(306, 448) = 306，夹到 230。
        assert_laid_out(800.0, 900.0, 230.0);
        // 正好 820，仍走窄公式：min(34%×600, 56%×820) = min(204, 459.2) = 204。
        assert_laid_out(820.0, 600.0, 204.0);
    }

    /// 只解析这一支长度，不挂窗口。宽公式因此可以在 600×400 上仍得到 220，即使这个宽度的窗口已经改走窄公式。
    fn assert_formula(narrow: bool, width: f32, height: f32, diameter: f32) {
        let spec = record_size(narrow);
        let from_viewport = spec
            .resolve_with(None, Some((width, height)))
            .expect("视口已量到");
        let ignoring_parent = spec
            .resolve_with(Some(10_000.0), Some((width, height)))
            .expect("不看父级");
        assert!(
            (from_viewport - diameter).abs() < 0.05,
            "视口 {width} x {height} 解析成 {from_viewport}，应是 {diameter}"
        );
        assert!(
            (ignoring_parent - from_viewport).abs() < 0.05,
            "直径不该跟着预览区的百分比走"
        );
    }

    /// 这条长度解析出来的像素，以及挂上以后唱片格子的宽高，都是这个直径。
    /// 父级百分比底换成很大的数也不变，说明不是预览区的百分比。宽度不大于 820 时用窄公式。
    fn assert_laid_out(width: f32, height: f32, diameter: f32) {
        let narrow = width <= super::super::STACK_BREAKPOINT;
        assert_formula(narrow, width, height, diameter);
        assert_eq!(
            record_diameter(width, height),
            Some(diameter),
            "公式和视口长度不一致"
        );

        let harness = ShellHarness::mount_at(scene(), width, height);
        let id = harness.keyed("inspect-audio-record").expect("唱片");
        let bounds = harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .expect("唱片布局");
        assert!(
            (bounds.width - diameter).abs() < 0.5 && (bounds.height - diameter).abs() < 0.5,
            "视口 {width} x {height} 的唱片格子应是 {diameter}，实际 {bounds:?}"
        );
        assert_circle_follows_the_box(&harness, bounds);
    }

    /// 落影仍是 `0 22px 60px`，并且贴着这次画出的圆，不铺成格子。
    fn assert_circle_follows_the_box(harness: &ShellHarness, bounds: LayoutBox) {
        let id = harness.keyed("inspect-audio-record").expect("唱片");
        let style = harness
            .document()
            .context()
            .world()
            .node_style(id)
            .expect("唱片样式");
        let painter = style.painter.as_ref().expect("唱片自绘").painter();
        let theme = nana_ui::theme::builtin_theme_arc(nana_ui::theme::ThemeAppearance::Dark);
        let recording = PaintRecording::record(
            painter,
            theme.as_ref(),
            [bounds.width, bounds.height],
            PaintState::default(),
        );
        let mut found = None;
        for op in &recording.behind_children {
            if let PaintOp::Shadow { path, shadow } = op {
                assert!(found.is_none(), "只画一条落影");
                found = Some((shadow.clone(), path.bounds().expect("落影要有圆")));
            }
        }
        let (shadow, circle) = found.expect("唱片要有落影");
        assert_eq!(
            (
                shadow.offset_x,
                shadow.offset_y,
                shadow.blur_radius,
                shadow.spread_radius
            ),
            (0.0, 22.0, 60.0, 0.0)
        );
        assert!(
            (circle.width - bounds.width).abs() < 0.5
                && (circle.height - bounds.height).abs() < 0.5,
            "落影应跟着画出的圆：{circle:?}"
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
