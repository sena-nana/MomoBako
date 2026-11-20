//! 音频歌词当前行的放大和投影。
//!
//! Vue `.media-preview__audio-lyric.is-active` 在完整正文之外还有 `transform: scale(1.025)`
//! （`transform-origin: center`）和
//! `filter: drop-shadow(0 8px 20px color-mix(in srgb, var(--accent) 16%, transparent))`。
//! 放大和投影只画在当前行上。已经唱过、还没唱到、没有时间的行保持原大，也不加阴影。
//! 不改三档明暗，也不改滚到正中的公式。

use std::cell::Cell;

use nana_ui::runtime::Button;
use nana_ui_core::{
    ColorFilter, FilterDropShadow, SemanticColorMix, SemanticColorRole, StyleModelRef,
    ThemeAppearance,
};

/// `.is-active { transform: scale(1.025) }`。
const ACTIVE_LYRIC_SCALE: f32 = 1.025;
/// `color-mix(in srgb, var(--accent) 16%, transparent)` 里强调色的比例。
const ACCENT_MIX: f32 = 0.16;
/// `drop-shadow` 的水平偏移，单位是逻辑像素。
const SHADOW_X: f32 = 0.0;
/// `drop-shadow` 的垂直偏移。
const SHADOW_Y: f32 = 8.0;
/// `drop-shadow` 的模糊半径。Vue 写 20px；画进 Nana 的滤镜字段，不另画一层字。
const SHADOW_BLUR: f32 = 20.0;

thread_local! {
    /// 这一次舞台投影用的主题。没记下以前按深色，和 Vue 跟随系统却读不到宿主时一样。
    static ACCENT: Cell<ThemeAppearance> = const { Cell::new(ThemeAppearance::Dark) };
}

/// 记下当前行投影跟的主题。浅色、深色照设置；跟随系统且没有宿主报告时调用方传入深色。
pub(crate) fn note_accent(mode: ThemeAppearance) {
    ACCENT.with(|slot| slot.set(mode));
}

/// 当前行绕自身中心放大到 1.025，并加上 Vue 的 drop-shadow。旁边的行不调用。
pub(super) fn lift_active_line(button: &mut Button) {
    let mode = ACCENT.with(Cell::get);
    let layout = std::sync::Arc::make_mut(&mut button.style.layout);
    layout.transform = Some(crate::shell::motion::shift_scale(0.0, ACTIVE_LYRIC_SCALE));
    layout.paint.filter = Some(active_filter(mode));
}

/// 当前行的 `filter: drop-shadow`。颜色是强调色按 0.16 混进透明。
fn active_filter(mode: ThemeAppearance) -> ColorFilter {
    ColorFilter {
        drop_shadow: Some(FilterDropShadow {
            paint_color: None,
            offset_x: SHADOW_X,
            offset_y: SHADOW_Y,
            blur_radius: SHADOW_BLUR,
            color: accent_shadow(mode),
        }),
        ..ColorFilter::default()
    }
}

/// `color-mix(in srgb, var(--accent) 16%, transparent)`：强调色的 RGB，不透明度 0.16。
fn accent_shadow(mode: ThemeAppearance) -> [f32; 4] {
    SemanticColorMix::alpha(SemanticColorRole::Accent, ACCENT_MIX)
        .resolve(StyleModelRef::new(mode))
        .as_rgba_array()
}

#[cfg(test)]
mod tests {
    use super::{ACTIVE_LYRIC_SCALE, accent_shadow, active_filter};
    use crate::shell::view_harness::ShellHarness;
    use crate::shell::{ShellViewModel, inspect_lyrics};
    use nana_ui_core::{SemanticColorMix, SemanticColorRole, StyleModelRef, ThemeAppearance};

    /// 当前行有 Vue 的 drop-shadow：偏移 (0, 8)、模糊 20、强调色 16% 透明度。
    /// 已经唱过、还没唱到、没有时间的行没有这层投影。浅色和深色各跟自己的强调色。
    #[test]
    fn active_lyric_line_drops_the_vue_shadow() {
        for (theme, mode) in [
            ("dark", ThemeAppearance::Dark),
            ("light", ThemeAppearance::Light),
        ] {
            let harness = mount(theme);
            let filter =
                paint_filter(&harness, "inspect-audio-lyric-active").expect("当前行要有投影");
            let shadow = filter.drop_shadow.expect("当前行是 drop-shadow");
            assert_eq!(
                (shadow.offset_x, shadow.offset_y, shadow.blur_radius),
                (0.0, 8.0, 20.0),
                "{theme} 的偏移和模糊"
            );
            assert_eq!(shadow.color, vue_accent(mode), "{theme} 应是强调色的 16%");
            assert_eq!(
                shadow.color,
                accent_shadow(mode),
                "{theme} 的投影色跟舞台记下的主题"
            );
            assert_eq!(filter, active_filter(mode), "{theme} 的滤镜只有这一层投影");
            assert!(
                paint_filter(&harness, "inspect-audio-lyric-0").is_none(),
                "{theme} 已经唱过的行没有投影"
            );
            assert!(
                paint_filter(&harness, "inspect-audio-lyric-2").is_none(),
                "{theme} 还没唱到的行没有投影"
            );
            assert!(
                paint_filter(&harness, "inspect-audio-lyric-3").is_none(),
                "{theme} 没有时间的行没有投影"
            );
            assert!(
                plain_shadow(&harness, "inspect-audio-lyric-active"),
                "{theme} 不用盒阴影或文字阴影冒充"
            );
            assert_eq!(
                harness
                    .document()
                    .context()
                    .world()
                    .node_style(harness.keyed("inspect-audio-lyric-active").expect("当前行"))
                    .and_then(|style| style.layout.transform),
                Some(crate::shell::motion::shift_scale(0.0, ACTIVE_LYRIC_SCALE)),
                "{theme} 的放大还在"
            );
        }
    }

    /// 和 `color-mix(in srgb, var(--accent) 16%, transparent)` 同一条混合。比例写死，不跟着实现常量走。
    fn vue_accent(mode: ThemeAppearance) -> [f32; 4] {
        SemanticColorMix::alpha(SemanticColorRole::Accent, 0.16)
            .resolve(StyleModelRef::new(mode))
            .as_rgba_array()
    }

    fn mount(theme: &str) -> ShellHarness {
        let mut model = scene("preview-audio");
        model.settings.theme = theme.into();
        model.inspect.lyrics =
            inspect_lyrics::parse_lrc("[00:00.00]开头\n[00:00.50]后来\n[00:01.00]以后\n作词：桃");
        let mut session = model.inspect.media_session().cloned().expect("预览会话");
        session.current_time_ms = 500;
        model.inspect.replace_shared_media(session);
        ShellHarness::mount(model)
    }

    fn paint_filter(harness: &ShellHarness, key: &str) -> Option<nana_ui_core::ColorFilter> {
        node_style(harness, key).layout.paint.filter
    }

    /// 没有盒阴影，也没有文字阴影。
    fn plain_shadow(harness: &ShellHarness, key: &str) -> bool {
        let style = node_style(harness, key);
        let paint = &style.layout.paint;
        paint.box_shadows.is_empty() && paint.text_shadow.is_none() && paint.text_shadows.is_empty()
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

    fn scene(name: &str) -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(scene, _)| *scene == name)
            .unwrap_or_else(|| panic!("没有场景 {name}"))
            .1
    }
}
