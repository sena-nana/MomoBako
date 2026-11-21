//! 歌词行的盒子。
//!
//! 歌词行是 16px（主题的 `Heading` 字号档）、1.72 倍行高、上下各一档 `Md` 内边距、
//! `overflow-wrap: anywhere`，不把长句收成一行省略号。
//! 行与行的间距是 `Xs`。
//! 单行盒子只用来估算还没排出来的滚动；排出来以后按布局盒居中，折行的行可以更高。

use nana_ui_core::{
    LayoutStyle, LengthSpec, OverflowWrapSpec, SpacingStep, TypeRole, WhiteSpaceSpec,
};

use crate::theme_map::tiers::{self, Edge};

/// 歌词行的字号档。
const ROLE: TypeRole = TypeRole::Heading;
/// 歌词行的字号（默认主题 16px）。
pub(super) const FONT: f32 = nana_ui_core::TypographyTokens::DEFAULT.size(ROLE);
/// 行高是字号的 1.72 倍。
const LEADING_RATIO: f32 = 1.72;
/// 行高（默认主题），估算滚动用。
pub(super) const LEADING: f32 = FONT * LEADING_RATIO;
/// 上下内边距档。左右是 0。
const PAD_STEP: SpacingStep = SpacingStep::Md;
/// 上下内边距（默认主题）。
pub(super) const PAD_Y: f32 = tiers::space_px(PAD_STEP);
/// 单行盒子：行高加上上下内边距。折行以后布局盒会更高。
pub(super) const LINE: f32 = LEADING + PAD_Y * 2.0;
/// 歌词行之间的间距档。
pub(super) const GAP_STEP: SpacingStep = SpacingStep::Xs;
/// 歌词行之间的间距（默认主题）。
pub(super) const GAP: f32 = tiers::space_px(GAP_STEP);

/// 把一行歌词收成上面这套盒子。
///
/// 不写死高度，第二行才撑得开。长句在任意位置断开，和 `overflow-wrap: anywhere` 一样，
/// 不再 `nowrap` 加省略号。
pub(super) fn apply(layout: &mut LayoutStyle) {
    crate::theme_map::control_font(layout, ROLE);
    // 按钮自带行高令牌，解析时盖过数值行高，所以行高连令牌一起写成字号档的倍数。
    tiers::line_ratio(layout, ROLE, LEADING_RATIO);
    tiers::padding(layout, &Edge::Y, PAD_STEP);
    layout.padding_left = Some(LengthSpec::Px(0.0));
    layout.padding_right = Some(LengthSpec::Px(0.0));
    layout.theme.padding_left = None;
    layout.theme.padding_right = None;
    // 歌词行不要边框。按钮默认的 1px 边会把单行盒子撑高 2px。
    layout.border_width = Some(0.0);
    layout.theme.border_width = None;
    layout.min_height = Some(LengthSpec::Px(LINE));
    layout.height = None;
    layout.max_height = None;
    layout.width = Some(LengthSpec::Fill);
    layout.min_width = Some(LengthSpec::Px(0.0));
    layout.white_space = WhiteSpaceSpec::Normal;
    layout.white_space_nowrap = false;
    layout.text_overflow_ellipsis = false;
    layout.text_ext.overflow_wrap = Some(OverflowWrapSpec::Anywhere);
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
    fn lyric_lines_wrap_at_their_metrics() {
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
            assert_eq!(used_font(&harness, label), FONT, "{label} 画出来的字号");
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
                style.layout.text_ext.overflow_wrap,
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

    /// 节点画字用的字号：按钮自己画的字看画出来的，纯文字看解析后的。
    fn used_font(harness: &ShellHarness, key: &str) -> f32 {
        let id = harness.node(key);
        let world = harness.document().context().world();
        world
            .widget_paint(id)
            .and_then(|paint| {
                paint.ops.iter().find_map(|op| match &op.op {
                    nana_ui::runtime::WidgetPaintOp::Text(text) => Some(text.font_size),
                    _ => None,
                })
            })
            .or_else(|| {
                world
                    .computed_style(id)
                    .map(|style| style.inherited.font_size)
            })
            .unwrap_or_else(|| panic!("{key} 没有字号"))
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
