//! 音视频准备卡的落影和不定进度轨。
//!
//! 准备卡的落影是 `box-shadow: 0 12px 32px color-mix(in srgb, var(--bg) 70%, transparent)`。
//! 进度轨是 NanaUI 的不定进度 `Progress`，扫动由组件自己的时间线驱动。

use super::LoadingCardShadow;
use crate::shell::ShellViewModel;
use crate::shell::view_harness::ShellHarness;
use nana_ui::runtime::{
    Entity, LayoutBox, LengthSpec, PaintOp, PaintRecording, PaintState, Painter, Progress,
};
use nana_ui_core::{
    RadiusTier, SemanticColorMix, SemanticColorRole, StyleModelRef, ThemeAppearance,
};

/// 准备卡有落影，下面是铺满卡宽的 6px 不定进度轨。
#[test]
fn loading_card_sweeps_and_drops_the_vue_shadow() {
    let harness = ShellHarness::mount(scene("online-preview-preparing"));
    let card = node_style(&harness, "inspect-media-loading");
    assert!(card.painter.is_some(), "准备卡要自绘落影");
    assert!(
        card.layout.paint.box_shadows.is_empty(),
        "落影写在自绘里，不和样式阴影叠成两层"
    );
    assert_eq!(
        card.background,
        Some(SemanticColorRole::Surface),
        "底色仍是 bg-elev"
    );
    assert_eq!(card.border, Some(SemanticColorRole::BorderSoft));
    assert_eq!(card.radius, Some(RadiusTier::Md), "落影跟着卡片的 md 圆角");
    let painter = card.painter.as_ref().expect("准备卡自绘").painter();
    for (theme, mode) in [
        ("深色", ThemeAppearance::Dark),
        ("浅色", ThemeAppearance::Light),
    ] {
        let recording = record_paint(painter, mode, [360.0, 96.0]);
        let (shadow, rect) = only_shadow(&recording.behind_children, theme);
        assert_eq!(
            (
                shadow.offset_x,
                shadow.offset_y,
                shadow.blur_radius,
                shadow.spread_radius,
                shadow.inset
            ),
            (0.0, 12.0, 32.0, 0.0, false),
            "{theme}"
        );
        assert_eq!(
            shadow.color.to_srgb(),
            vue_background(mode),
            "{theme} 应是背景色的 70%"
        );
        assert!(
            (rect.width - 360.0).abs() < 0.05 && (rect.height - 96.0).abs() < 0.05,
            "{theme} 落影要贴着卡片外框"
        );
        assert!(rect.x.abs() < 0.05 && rect.y.abs() < 0.05, "{theme}");
    }
    assert!(
        record_paint(&LoadingCardShadow, ThemeAppearance::Dark, [0.0, 40.0])
            .behind_children
            .is_empty(),
        "没有面积时不编阴影"
    );
    assert!(
        record_paint(&LoadingCardShadow, ThemeAppearance::Dark, [f32::NAN, 40.0])
            .behind_children
            .is_empty(),
        "尺寸无效时不编阴影"
    );

    let track = sweep_track(&harness, "inspect-media-loading-progress");
    assert!(track.indeterminate, "准备卡不知道要多久，进度轨是不定进度");
    assert_eq!(track.thickness, Some(6.0));
    assert_eq!(track.track, Some(SemanticColorRole::BorderSoft));
    assert_eq!(
        track.style.layout.width,
        Some(LengthSpec::Fill),
        "准备卡的进度轨铺满卡宽"
    );
}

/// 键上的不定进度轨组件。
pub(super) fn sweep_track(harness: &ShellHarness, key: &str) -> Progress {
    let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
    harness
        .document()
        .context()
        .read(Entity::<Progress>::from_stable_id(id), Progress::clone)
        .unwrap_or_else(|_| panic!("{key} 应该是 NanaUI 的 Progress"))
}

/// `color-mix(in srgb, var(--bg) 70%, transparent)`。
fn vue_background(mode: ThemeAppearance) -> [f32; 4] {
    SemanticColorMix::alpha(SemanticColorRole::Background, 0.7)
        .resolve(StyleModelRef::new(mode))
        .as_rgba_array()
}

fn only_shadow(ops: &[PaintOp], theme: &str) -> (nana_ui::runtime::ComponentElevation, LayoutBox) {
    let shadow_at = ops
        .iter()
        .position(|op| {
            matches!(
                op,
                PaintOp::RoundedRect {
                    shadow: Some(_),
                    ..
                }
            )
        })
        .unwrap_or_else(|| panic!("{theme} 准备卡要有落影"));
    let default_at = ops
        .iter()
        .position(|op| matches!(op, PaintOp::DrawDefault))
        .unwrap_or_else(|| panic!("{theme} 底色还要画"));
    assert!(shadow_at < default_at, "{theme} 落影应在底色下面");
    assert!(
        ops.iter().all(|op| !matches!(
            op,
            PaintOp::RoundedRect {
                fill: Some(nana_ui::runtime::ResolvedPaint::Gradient(_)),
                ..
            }
        )),
        "{theme} 准备卡不画浅光"
    );
    let PaintOp::RoundedRect {
        rect, fill, shadow, ..
    } = &ops[shadow_at]
    else {
        unreachable!()
    };
    assert!(fill.is_none(), "{theme} 落影那一层不另铺底色");
    (shadow.clone().expect("落影"), *rect)
}

fn record_paint(painter: &dyn Painter, mode: ThemeAppearance, size: [f32; 2]) -> PaintRecording {
    let appearance = if mode == ThemeAppearance::Light {
        nana_ui::theme::ThemeAppearance::Light
    } else {
        nana_ui::theme::ThemeAppearance::Dark
    };
    let theme = nana_ui::theme::builtin_theme_arc(appearance);
    PaintRecording::record(painter, theme.as_ref(), size, PaintState::default())
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
