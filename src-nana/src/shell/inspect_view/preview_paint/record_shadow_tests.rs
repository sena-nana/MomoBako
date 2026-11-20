//! 音频舞台唱片的落影。
//!
//! Vue `.media-preview__audio-record` 在正圆后面画
//! `box-shadow: 0 22px 60px color-mix(in srgb, var(--bg) 78%, transparent)`。
//! 舞台格子不是正方形时，阴影仍跟着较小边画出的圆，不铺成方块。

use super::VinylRecord;
use crate::shell::ShellViewModel;
use crate::shell::view_harness::ShellHarness;
use nana_ui::runtime::{LayoutBox, PaintOp, PaintRecording, PaintState, Painter};
use nana_ui_core::{SemanticColorMix, SemanticColorRole, StyleModelRef, ThemeAppearance};

/// 唱片正圆后面画出 Vue 的落影：下移 22、模糊 60、不扩展、背景色 78%。
/// 浅色和深色各跟自己的背景。没有图和有封面时都画，而且画在底色下面。
/// 格子是 240×180 时圆的直径是 180，贴在左边留白 30，不是整块 240 宽的方阴影。
#[test]
fn audio_record_draws_the_vue_drop_shadow() {
    let harness = mount();
    let style = node_style(&harness, "inspect-audio-record");
    assert!(style.painter.is_some(), "唱片要自绘");
    assert!(
        style.layout.paint.box_shadows.is_empty(),
        "落影不写在外框上，免得格子不是正方形时变成方块"
    );
    let painter = style.painter.as_ref().expect("唱片自绘").painter();
    for (theme, mode) in [
        ("深色", ThemeAppearance::Dark),
        ("浅色", ThemeAppearance::Light),
    ] {
        let recording = record_paint(painter, mode, [240.0, 180.0]);
        let (shadow, bounds) = only_shadow(&recording.behind_children);
        assert_eq!(
            (
                shadow.offset_x,
                shadow.offset_y,
                shadow.blur_radius,
                shadow.spread_radius,
                shadow.inset
            ),
            (0.0, 22.0, 60.0, 0.0, false),
            "{theme}"
        );
        assert_eq!(
            shadow.color.to_srgb(),
            vue_background(mode),
            "{theme} 应是背景色的 78%"
        );
        assert_circle(&bounds, 30.0, 0.0, 180.0, theme);
        assert!(
            shadow_before_fill(&recording.behind_children),
            "{theme} 落影要画在唱片底色下面"
        );
    }
    for art in [None, Some("data:image/png;base64,aaaa")] {
        let painted = VinylRecord::with_art(art);
        let recording = record_paint(&painted, ThemeAppearance::Dark, [240.0, 180.0]);
        let (_, bounds) = only_shadow(&recording.behind_children);
        assert_circle(
            &bounds,
            30.0,
            0.0,
            180.0,
            if art.is_some() {
                "有封面"
            } else {
                "没有图"
            },
        );
        let drawn = recording
            .behind_children
            .iter()
            .any(|op| matches!(op, PaintOp::Image { .. }));
        assert_eq!(drawn, art.is_some(), "只有真实封面才画进圆里");
    }
    let blank = VinylRecord::with_art(None);
    assert!(
        record_paint(&blank, ThemeAppearance::Dark, [0.0, 40.0])
            .behind_children
            .is_empty(),
        "没有面积时不编阴影"
    );
    assert!(
        record_paint(&blank, ThemeAppearance::Dark, [f32::NAN, 40.0])
            .behind_children
            .is_empty(),
        "尺寸无效时不编阴影"
    );
}

/// 和 `color-mix(in srgb, var(--bg) 78%, transparent)` 同一条混合。比例写死，不跟着实现常量走。
fn vue_background(mode: ThemeAppearance) -> [f32; 4] {
    SemanticColorMix::alpha(SemanticColorRole::Background, 0.78)
        .resolve(StyleModelRef::new(mode))
        .as_rgba_array()
}

/// 落影路径的外接矩形应是这个圆。
fn assert_circle(bounds: &LayoutBox, x: f32, y: f32, side: f32, theme: &str) {
    assert!(
        (bounds.x - x).abs() < 0.05 && (bounds.y - y).abs() < 0.05,
        "{theme} 圆心偏了：{bounds:?}"
    );
    assert!(
        (bounds.width - side).abs() < 0.05 && (bounds.height - side).abs() < 0.05,
        "{theme} 落影不是正圆：{bounds:?}"
    );
}

/// 第一条落影出现在第一条填充之前。
fn shadow_before_fill(ops: &[PaintOp]) -> bool {
    let shadow = ops
        .iter()
        .position(|op| matches!(op, PaintOp::Shadow { .. }));
    let fill = ops
        .iter()
        .position(|op| matches!(op, PaintOp::FillPath { .. }));
    shadow.zip(fill).is_some_and(|(shadow, fill)| shadow < fill)
}

/// 录制里唯一的一条落影，以及它的圆。
fn only_shadow(ops: &[PaintOp]) -> (nana_ui::runtime::ComponentElevation, LayoutBox) {
    let mut found = None;
    for op in ops {
        if let PaintOp::Shadow { path, shadow } = op {
            assert!(found.is_none(), "只画一条落影");
            found = Some((shadow.clone(), path.bounds().expect("落影要有圆")));
        }
    }
    found.expect("唱片要有落影")
}

/// 按主题录一次自绘。文字测量用录制器自带的估算。
fn record_paint(painter: &dyn Painter, mode: ThemeAppearance, size: [f32; 2]) -> PaintRecording {
    let appearance = if mode == ThemeAppearance::Light {
        nana_ui::theme::ThemeAppearance::Light
    } else {
        nana_ui::theme::ThemeAppearance::Dark
    };
    let theme = nana_ui::theme::builtin_theme_arc(appearance);
    PaintRecording::record(painter, theme.as_ref(), size, PaintState::default())
}

fn mount() -> ShellHarness {
    ShellHarness::mount(scene("preview-audio"))
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
