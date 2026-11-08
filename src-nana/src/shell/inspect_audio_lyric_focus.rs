//! 音频歌词行的键盘焦点下沿。
//!
//! Vue `.media-preview__audio-lyric:focus-visible:not(:disabled)` 关掉轮廓，字色和悬停同一档，
//! 再画 `box-shadow: 0 2px 0 color-mix(in srgb, var(--accent) 60%, transparent)`。
//! 这是行盒下移 2px、不模糊的一条阴影，不是当前行的 drop-shadow，也不是 Nana 按钮默认的焦点底。
//! 鼠标点上得到的焦点不算 `:focus-visible`，不画。没有时间的行是禁用的，点不了，也不画。

use nana_ui::runtime::{
    Button, LayoutBox, PaintColor, PaintContext, PaintPath, PaintShadow, Painter,
    SemanticColorRole, SemanticPaint,
};
use nana_ui_core::SemanticColorMix;

/// `color-mix(in srgb, var(--accent) 60%, transparent)` 里强调色的比例。
const ACCENT_MIX: f32 = 0.6;
/// `box-shadow: 0 2px 0` 的垂直偏移。模糊和扩展都是 0。
const SHADOW_Y: f32 = 2.0;

/// 有时间的歌词按钮：键盘焦点时字色改成悬停那一档，并挂上下沿阴影。
///
/// 不改三档明暗、放大和当前行投影。Nana 按钮默认的焦点底是整块填充，这里清掉，只留 Vue 这一条阴影。
pub(super) fn mark(button: &mut Button) {
    button.style.interaction.focused = SemanticPaint {
        foreground_mix: super::lyric_mix(Some(super::LYRIC_HOVER_MIX)),
        ..SemanticPaint::default()
    };
    button.style.painter = Some(LyricFocusUnderline.into());
}

/// 键盘焦点时在行盒下沿画 Vue 的 `box-shadow`。其余状态只画按钮自己的字。
struct LyricFocusUnderline;

impl Painter for LyricFocusUnderline {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let state = cx.state();
        if state.focused && !state.disabled {
            underline(cx);
        }
        cx.draw_default();
    }

    fn paint_key(&self) -> u64 {
        1
    }
}

/// 行盒原样下移 2px 的不透明阴影。先画阴影，再画字，字盖在阴影上面。
fn underline(cx: &mut PaintContext<'_>) {
    let [width, height] = cx.size();
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        eprintln!("Nana 歌词焦点下沿没有可画的行盒：{width} x {height}");
        return;
    }
    let mut path = PaintPath::new();
    path.rect(LayoutBox {
        x: 0.0,
        y: 0.0,
        width,
        height,
    });
    cx.shadow(
        &path,
        PaintShadow::Custom {
            color: PaintColor::Mix(SemanticColorMix::alpha(
                SemanticColorRole::Accent,
                ACCENT_MIX,
            )),
            offset: [0.0, SHADOW_Y],
            blur: 0.0,
            spread: 0.0,
            inset: false,
        },
    );
}

#[cfg(test)]
mod tests {
    use crate::shell::view_harness::ShellHarness;
    use crate::shell::{ShellViewModel, inspect_lyrics};
    use nana_ui::runtime::{PaintOp, PaintState};
    use nana_ui_core::{SemanticColorMix, SemanticColorRole, StyleModelRef, ThemeAppearance};

    /// 有时间的行在键盘焦点时画出 Vue 的下沿阴影：偏移 (0, 2)、不模糊、强调色 60%。
    /// 悬停、按下、禁用和没有焦点都不画。没有时间的行不是按钮，没有这层。浅色和深色各跟自己的强调色。
    #[test]
    fn timed_lyric_keyboard_focus_draws_the_vue_underline() {
        for (theme, mode) in [
            ("dark", ThemeAppearance::Dark),
            ("light", ThemeAppearance::Light),
        ] {
            let harness = mount(theme);
            for key in [
                "inspect-audio-lyric-active",
                "inspect-audio-lyric-0",
                "inspect-audio-lyric-2",
            ] {
                let style = node_style(&harness, key);
                assert!(style.painter.is_some(), "{theme} {key} 要有焦点下沿");
                assert!(
                    style.layout.paint.box_shadows.is_empty(),
                    "{theme} {key} 不用常驻盒阴影冒充"
                );
                assert_eq!(
                    style.interaction.focused.foreground_mix,
                    Some(nana_ui_core::SemanticColorMix::new(
                        SemanticColorRole::Text,
                        SemanticColorRole::Background,
                        super::super::LYRIC_HOVER_MIX
                    )),
                    "{theme} {key} 键盘焦点的字色和悬停同一档"
                );
                assert!(
                    style.interaction.focused.background.is_none(),
                    "{theme} {key} 不铺焦点底"
                );
                assert_eq!(
                    style.interaction.focused.foreground, None,
                    "{theme} {key} 不改成焦点字色"
                );
                let painter = style.painter.as_ref().expect("自绘").painter();
                let appearance = if mode == ThemeAppearance::Light {
                    nana_ui::theme::ThemeAppearance::Light
                } else {
                    nana_ui::theme::ThemeAppearance::Dark
                };
                let theme_arc = nana_ui::theme::builtin_theme_arc(appearance);
                let focused = nana_ui::runtime::PaintRecording::record(
                    painter,
                    theme_arc.as_ref(),
                    [180.0, 22.0],
                    PaintState {
                        focused: true,
                        ..PaintState::default()
                    },
                );
                let shadow = only_shadow(&focused.behind_children);
                assert_eq!(
                    (
                        shadow.offset_x,
                        shadow.offset_y,
                        shadow.blur_radius,
                        shadow.spread_radius,
                        shadow.inset
                    ),
                    (0.0, 2.0, 0.0, 0.0, false),
                    "{theme} {key}"
                );
                assert_eq!(
                    shadow.color.to_srgb(),
                    vue_accent(mode),
                    "{theme} {key} 应是强调色的 60%"
                );
                assert!(
                    matches!(focused.behind_children.last(), Some(PaintOp::DrawDefault)),
                    "{theme} {key} 阴影画在字下面"
                );
                assert!(focused.over_children.is_empty(), "{theme} {key} 不盖住字");
                for state in [
                    PaintState::default(),
                    PaintState {
                        hovered: true,
                        ..PaintState::default()
                    },
                    PaintState {
                        pressed: true,
                        ..PaintState::default()
                    },
                    PaintState {
                        focused: true,
                        disabled: true,
                        ..PaintState::default()
                    },
                ] {
                    let recording = nana_ui::runtime::PaintRecording::record(
                        painter,
                        theme_arc.as_ref(),
                        [180.0, 22.0],
                        state,
                    );
                    assert!(
                        recording
                            .behind_children
                            .iter()
                            .all(|op| !matches!(op, PaintOp::Shadow { .. })),
                        "{theme} {key} {state:?} 不画下沿"
                    );
                    assert!(
                        matches!(recording.behind_children.last(), Some(PaintOp::DrawDefault)),
                        "{theme} {key} 字还在"
                    );
                }
            }
            let plain = node_style(&harness, "inspect-audio-lyric-3");
            assert!(plain.painter.is_none(), "{theme} 没有时间的行不画焦点下沿");
            assert!(plain.layout.paint.box_shadows.is_empty());
            let active = node_style(&harness, "inspect-audio-lyric-active");
            assert!(
                active
                    .layout
                    .paint
                    .filter
                    .and_then(|filter| filter.drop_shadow)
                    .is_some(),
                "{theme} 当前行投影还在"
            );
            assert!(active.layout.transform.is_some(), "{theme} 当前行放大还在");
        }
    }

    /// 和 `color-mix(in srgb, var(--accent) 60%, transparent)` 同一条混合。比例写死，不跟着实现常量走。
    fn vue_accent(mode: ThemeAppearance) -> [f32; 4] {
        SemanticColorMix::alpha(SemanticColorRole::Accent, 0.6)
            .resolve(StyleModelRef::new(mode))
            .as_rgba_array()
    }

    fn only_shadow(ops: &[PaintOp]) -> nana_ui::runtime::ComponentElevation {
        let mut found = None;
        for op in ops {
            if let PaintOp::Shadow { shadow, .. } = op {
                assert!(found.is_none(), "只画一条下沿");
                found = Some(shadow.clone());
            }
        }
        found.expect("键盘焦点要有下沿阴影")
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
