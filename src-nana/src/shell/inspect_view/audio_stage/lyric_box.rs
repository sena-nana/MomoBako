//! 歌词行的盒子。
//!
//! Vue `.media-preview__audio-lyric` 写 `font-size: 16px`、`line-height: 1.72`、
//! `padding: 7px 0`、`overflow-wrap: anywhere`，不把长句收成一行省略号。
//! 轨道 `.media-preview__audio-lyrics-track` 的行距是 4px。
//! 单行盒子只用来估算还没排出来的滚动；排出来以后按布局盒居中，折行的行可以更高。

use nana_ui_core::{LayoutStyle, LengthSpec, LineHeightSpec, OverflowWrapSpec, WhiteSpaceSpec};

/// `.media-preview__audio-lyric` 的字号。
pub(super) const FONT: f32 = 16.0;
/// `line-height: 1.72`，按 16px 算成像素。
pub(super) const LEADING: f32 = FONT * 1.72;
/// 上下内边距。左右是 0。
pub(super) const PAD_Y: f32 = 7.0;
/// 单行盒子：行高加上上下 7px。折行以后布局盒会更高。
pub(super) const LINE: f32 = LEADING + PAD_Y * 2.0;
/// `.media-preview__audio-lyrics-track` 的 `gap`。
pub(super) const GAP: f32 = 4.0;

/// 把一行歌词收成 Vue 的盒子。
///
/// 不写死高度，第二行才撑得开。长句在任意位置断开，和 `overflow-wrap: anywhere` 一样，
/// 不再 `nowrap` 加省略号。
pub(super) fn apply(layout: &mut LayoutStyle) {
    layout.font_size = Some(FONT);
    layout.line_height = Some(LineHeightSpec::Absolute(LEADING));
    layout.padding_top = Some(LengthSpec::Px(PAD_Y));
    layout.padding_bottom = Some(LengthSpec::Px(PAD_Y));
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    // Vue `border: 0`。按钮默认的 1px 边会把单行盒子撑高 2px。
    layout.border_width = Some(0.0);
    layout.min_height = Some(LengthSpec::Px(LINE));
    layout.height = None;
    layout.max_height = None;
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.white_space = WhiteSpaceSpec::Normal;
    layout.white_space_nowrap = false;
    layout.text_overflow_ellipsis = false;
    layout.overflow_wrap = Some(OverflowWrapSpec::Anywhere);
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(1.0);
}

#[cfg(test)]
mod tests {
    use super::{FONT, GAP, LEADING, LINE, PAD_Y};
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;
    use nana_ui_core::{LengthSpec, LineHeightSpec, OverflowWrapSpec, WhiteSpaceSpec};

    /// 歌词行是 16px、1.72 行高、上下 7px，行距 4px。
    /// 短句停在单行盒子里；放不下一行的中文和没有空格的长串都折行，不省略成一行。
    #[test]
    fn lyric_lines_wrap_at_the_vue_metrics() {
        let mut model = scene();
        let chinese = "桃".repeat(80);
        let latin = "W".repeat(180);
        model.inspect.lyrics = crate::shell::inspect_lyrics::parse_lrc(&format!(
            "[00:00.00]短句\n{chinese}\n[00:01.00]{latin}"
        ));
        let harness = ShellHarness::mount(model);
        let short = box_of(&harness, "短句");
        let wrapped = box_of(&harness, &chinese);
        let token = box_of(&harness, &latin);
        let list = keyed_box(&harness, "inspect-audio-lyric-list");
        assert!(
            (short.height - LINE).abs() < 1.5,
            "短句应是单行盒子 {LINE}，实际 {}",
            short.height
        );
        assert!(
            wrapped.height > short.height + LEADING - 1.0,
            "中文长句应折行：短句 {}，长句 {}",
            short.height,
            wrapped.height
        );
        assert!(
            token.height > short.height + LEADING - 1.0,
            "没有空格的长串应折行：短句 {}，长串 {}",
            short.height,
            token.height
        );
        assert!(
            wrapped.width <= list.width + 1.0,
            "折行后不应撑出歌词列：行 {}，列 {}",
            wrapped.width,
            list.width
        );
        assert!(
            token.width <= list.width + 1.0,
            "长串折行后不应撑出歌词列：行 {}，列 {}",
            token.width,
            list.width
        );

        for label in ["短句", chinese.as_str(), latin.as_str()] {
            let style = style_of(&harness, label);
            assert_eq!(style.layout.font_size, Some(FONT), "{label} 的字号");
            assert!(
                matches!(style.layout.line_height, Some(LineHeightSpec::Absolute(value)) if (value - LEADING).abs() < 0.01),
                "{label} 的行高应是 {LEADING}"
            );
            assert_eq!(
                style.layout.padding_top,
                Some(LengthSpec::Px(PAD_Y)),
                "{label} 的上内边距"
            );
            assert_eq!(
                style.layout.padding_bottom,
                Some(LengthSpec::Px(PAD_Y)),
                "{label} 的下内边距"
            );
            assert_eq!(style.layout.padding_left, Some(LengthSpec::Px(0.0)));
            assert_eq!(style.layout.padding_right, Some(LengthSpec::Px(0.0)));
            assert_eq!(
                style.layout.white_space,
                WhiteSpaceSpec::Normal,
                "{label} 不该 nowrap"
            );
            assert!(!style.layout.white_space_nowrap, "{label} 不该锁成一行");
            assert!(!style.layout.text_overflow_ellipsis, "{label} 不该省略");
            assert_eq!(
                style.layout.overflow_wrap,
                Some(OverflowWrapSpec::Anywhere),
                "{label} 应能在任意位置断开"
            );
            assert!(
                style.layout.height.is_none(),
                "{label} 不该写死高度，折行才撑得开"
            );
        }
        let list_style = keyed_style(&harness, "inspect-audio-lyric-list");
        assert_eq!(
            list_style.layout.gap,
            Some(LengthSpec::Px(GAP)),
            "行距应是 4px"
        );
    }

    fn scene() -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "preview-audio")
            .unwrap_or_else(|| panic!("没有场景 preview-audio"))
            .1
    }

    fn box_of(harness: &ShellHarness, key: &str) -> nana_ui::runtime::LayoutBox {
        let id = harness.node(key);
        harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局"))
    }

    fn keyed_box(harness: &ShellHarness, key: &str) -> nana_ui::runtime::LayoutBox {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局"))
    }

    fn keyed_style(harness: &ShellHarness, key: &str) -> nana_ui::runtime::NodeStyle {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .node_style(id)
            .unwrap_or_else(|| panic!("{key} 没有样式"))
            .clone()
    }

    fn style_of(harness: &ShellHarness, key: &str) -> nana_ui::runtime::NodeStyle {
        let id = harness.node(key);
        harness
            .document()
            .context()
            .world()
            .node_style(id)
            .unwrap_or_else(|| panic!("{key} 没有样式"))
            .clone()
    }
}
