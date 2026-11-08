//! 音频舞台标题的宽度。
//!
//! Vue `.media-preview__audio-caption` 写 `width: min(var(--audio-record-size), 100%)`。
//! `--audio-record-size` 就是唱片直径，没有再减一截。`100%` 是标题所在格子。
//! 格子不比直径窄时，宽度等于直径。
//! 这里用唱片已经在用的那条直径，不另写一套夹取。
//! 直径还没从视口算出来时，这条长度解析不成像素，不改写成 220 或 380。

use nana_ui::runtime::LengthSpec;
use nana_ui_core::LayoutStyle;

/// 已经算出的唱片直径，拿来当标题宽度。
///
/// 没有直径、非有限或 ≤ 0 时返回 `None` 并记日志，不编成 220 或 380。
pub(super) fn title_width(diameter: Option<f32>) -> Option<f32> {
    match diameter {
        Some(px) if px.is_finite() && px > 0.0 => Some(px),
        other => {
            eprintln!("Nana 唱片直径还没算出来，标题宽度不编成 220 或 380：{other:?}");
            None
        }
    }
}

/// 写进标题宽高的长度。和唱片是同一条，窄窗口、宽窗口都跟着它。
/// 没有视口时解析是 `None`，不落成 220 或 380。
pub(super) fn caption_width(narrow: bool) -> LengthSpec {
    super::diameter::record_size(narrow)
}

/// 把一条已经算出的直径钉成标题宽度。解析不出像素时不另填固定值。
pub(super) fn pin_width(layout: &mut LayoutStyle, narrow: bool) {
    let width = caption_width(narrow);
    layout.width = Some(width);
    layout.min_width = Some(width);
    layout.max_width = Some(width);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
}

#[cfg(test)]
mod tests {
    use nana_ui::runtime::LengthSpec;

    use super::{caption_width, title_width};
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    /// 标题宽度用已经算出的唱片直径。
    /// 直径 220 时标题不超过它，直径 380 时宽度就是 380。
    /// 直径还没算出来时不把标题说成 220 或 380。
    #[test]
    fn audio_title_width_follows_the_record_diameter() {
        let wide = caption_width(false);
        assert!(
            wide.resolve_with(None, None).is_none(),
            "没有视口不能编出标题宽度"
        );
        assert!(
            wide.resolve_with(Some(10_000.0), None).is_none(),
            "只有父级百分比底、没有视口，也不能编"
        );
        let narrow = caption_width(true);
        assert!(
            narrow.resolve_with(None, None).is_none(),
            "窄窗口没有视口也不能编成 140 或 230"
        );
        assert!(title_width(None).is_none(), "没有直径时不截断");
        assert!(title_width(super::super::diameter::record_diameter(0.0, 800.0)).is_none());
        for px in [f32::NAN, f32::INFINITY, 0.0, -12.0] {
            assert!(
                title_width(Some(px)).is_none(),
                "还没量到的直径不拿来截断：{px}"
            );
        }

        // 宽公式下限。1000×400：min(320, 192) 夹到 220。标题不得超过这个直径。
        assert_eq!(title_width(Some(220.0)), Some(220.0));
        assert_laid_out(1000.0, 400.0, 220.0, false);
        // 宽公式上限。2000×1600：min(640, 768) 夹到 380。标题就用 380。
        assert_eq!(title_width(Some(380.0)), Some(380.0));
        assert_laid_out(2000.0, 1600.0, 380.0, true);
    }

    /// 挂上窗口后，标题宽度跟着这次算出的唱片直径。
    /// `exact` 时宽度就是这个直径；否则只要求不超过。
    fn assert_laid_out(width: f32, height: f32, diameter: f32, exact: bool) {
        let narrow = width <= super::super::STACK_BREAKPOINT;
        let from_record = super::super::diameter::record_diameter(width, height);
        assert_eq!(from_record, Some(diameter), "这次视口的直径应是 {diameter}");
        assert_eq!(
            title_width(from_record),
            Some(diameter),
            "标题宽度要用已经算出的直径"
        );
        let spec = caption_width(narrow);
        let resolved = spec
            .resolve_with(None, Some((width, height)))
            .expect("视口已量到");
        assert!(
            (resolved - diameter).abs() < 0.05,
            "标题长度应是 {diameter}，实际 {resolved}"
        );
        assert!(
            spec.resolve_with(None, None).is_none(),
            "同一条长度在没有视口时仍不编像素"
        );

        let harness = ShellHarness::mount_at(scene(), width, height);
        let title = box_of(&harness, "inspect-audio-title");
        let record = box_of(&harness, "inspect-audio-record");
        assert!(
            (record.width - diameter).abs() < 0.5,
            "唱片应仍是 {diameter}，实际 {}",
            record.width
        );
        let style = node_style(&harness, "inspect-audio-title");
        let written = style.layout.width.expect("标题写了宽度");
        assert!(
            written.resolve_with(None, None).is_none(),
            "写下的宽度不是 220 或 380 的固定像素"
        );
        assert!(
            !matches!(written, LengthSpec::Px(px) if px > 0.0),
            "没有视口时不把直径落成像素"
        );
        assert!(
            (title.width - record.width).abs() < 0.5,
            "标题要用已经画出的唱片直径：标题 {}，唱片 {}",
            title.width,
            record.width
        );
        if exact {
            assert!(
                (title.width - diameter).abs() < 0.5,
                "直径 {diameter} 时标题要用 {diameter}，实际 {}",
                title.width
            );
        } else {
            assert!(
                title.width <= diameter + 0.5,
                "直径 {diameter} 时标题不得超过它，实际 {}",
                title.width
            );
        }
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
