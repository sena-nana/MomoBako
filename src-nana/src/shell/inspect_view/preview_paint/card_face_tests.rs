//! 音频舞台卡片的径向浅光和落影。
//!
//! Vue `.media-preview--audio` 在圆角卡片上画
//! `radial-gradient(circle at 28% 32%, color-mix(in srgb, var(--accent) 12%, transparent), transparent 28%)`，
//! 以及 `box-shadow: 0 18px 40px color-mix(in srgb, var(--bg) 84%, transparent)`。
//! 圆心按 1px 边框内侧的内边距盒定位。圆的半径是到该盒最远角的像素距离，格子不是正方形时仍是正圆。

use super::AudioCardFace;
use crate::shell::ShellViewModel;
use crate::shell::view_harness::ShellHarness;
use nana_ui::runtime::{
    GradientShape, LayoutBox, PaintOp, PaintRecording, PaintState, Painter, ResolvedPaint,
};
use nana_ui_core::{
    RadiusTier, SemanticColorMix, SemanticColorRole, StyleModelRef, ThemeAppearance,
};

/// 卡片上有 Vue 的浅光和落影。落影在底色下面，浅光在底色上面、内容下面。
/// 浅色和深色各跟自己的背景色、强调色。800×240 不是正方形，圆仍按像素最远角。
/// 没有面积时不编阴影，也不编浅光。
#[test]
fn audio_card_draws_the_vue_glow_and_shadow() {
    let harness = mount();
    let style = node_style(&harness, "inspect-audio-shell");
    assert!(style.painter.is_some(), "卡片要自绘浅光和落影");
    assert!(
        style.layout.paint.box_shadows.is_empty(),
        "落影写在自绘里，不和样式阴影叠成两层"
    );
    assert_eq!(
        style.background,
        Some(SemanticColorRole::Surface),
        "底色仍是 bg-elev"
    );
    assert_eq!(style.border, Some(SemanticColorRole::BorderSoft));
    assert_eq!(
        style.layout.border_width,
        Some(1.0),
        "浅光按这 1px 边框往里收"
    );
    assert_eq!(style.radius, Some(RadiusTier::Xl), "落影跟着卡片的 xl 圆角");
    let painter = style.painter.as_ref().expect("卡片自绘").painter();
    let size = [800.0, 240.0];
    for (theme, mode) in [
        ("深色", ThemeAppearance::Dark),
        ("浅色", ThemeAppearance::Light),
    ] {
        let recording = record_paint(painter, mode, size);
        let (shadow, shadow_rect, shadow_radii, glow) = layers(&recording.behind_children, theme);
        assert_eq!(
            (
                shadow.offset_x,
                shadow.offset_y,
                shadow.blur_radius,
                shadow.spread_radius,
                shadow.inset
            ),
            (0.0, 18.0, 40.0, 0.0, false),
            "{theme}"
        );
        assert_eq!(
            shadow.color.to_srgb(),
            vue_background(mode),
            "{theme} 应是背景色的 84%"
        );
        assert!(
            (shadow_rect.x).abs() < 0.05
                && (shadow_rect.y).abs() < 0.05
                && (shadow_rect.width - size[0]).abs() < 0.05
                && (shadow_rect.height - size[1]).abs() < 0.05,
            "{theme} 落影要贴着卡片外框：{shadow_rect:?}"
        );
        assert!(
            shadow_radii
                .iter()
                .all(|radius| (*radius - shadow_radii[0]).abs() < 0.05 && *radius > 1.0),
            "{theme} 落影圆角：{shadow_radii:?}"
        );
        let (inner, center, reach) = vue_glow(size);
        assert!(
            (glow.rect.x - inner.x).abs() < 0.05
                && (glow.rect.y - inner.y).abs() < 0.05
                && (glow.rect.width - inner.width).abs() < 0.05
                && (glow.rect.height - inner.height).abs() < 0.05,
            "{theme} 浅光要画在边框内侧：{:?}",
            glow.rect
        );
        assert!(
            glow.radii
                .iter()
                .all(|radius| (*radius - (shadow_radii[0] - 1.0)).abs() < 0.05),
            "{theme} 浅光圆角应比外框少 1px"
        );
        match glow.shape {
            GradientShape::Radial { center: at, radius } => {
                assert!(
                    (at[0] - center[0]).abs() < 0.05 && (at[1] - center[1]).abs() < 0.05,
                    "{theme} 圆心应在 28% 32%：{at:?}"
                );
                assert!(
                    (radius - reach).abs() < 0.05,
                    "{theme} 圆应到内边距盒最远角，得到 {radius}，应是 {reach}"
                );
            }
            other => panic!("{theme} 浅光不是径向圆：{other:?}"),
        }
        assert_eq!(glow.stops.len(), 2, "{theme}");
        assert!(
            (glow.stops[0].0 - 0.0).abs() < 0.001 && (glow.stops[1].0 - 0.28).abs() < 0.001,
            "{theme} 色标应在 0 和 28%"
        );
        assert_eq!(
            glow.stops[0].1,
            vue_accent(mode),
            "{theme} 圆心应是强调色的 12%"
        );
        assert_eq!(
            glow.stops[1].1,
            [0.0, 0.0, 0.0, 0.0],
            "{theme} 28% 处应是透明"
        );
    }
    let blank = AudioCardFace;
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
    let thin = record_paint(&blank, ThemeAppearance::Dark, [1.5, 40.0]);
    assert!(
        thin.behind_children.iter().any(|op| matches!(
            op,
            PaintOp::Shadow { .. }
                | PaintOp::RoundedRect {
                    shadow: Some(_),
                    ..
                }
        )),
        "边框占满时落影还在"
    );
    assert!(
        glow_of(&thin.behind_children).is_none(),
        "没有内边距盒时不编浅光"
    );
}

/// 和 `color-mix(in srgb, var(--bg) 84%, transparent)` 同一条混合。比例写死。
fn vue_background(mode: ThemeAppearance) -> [f32; 4] {
    SemanticColorMix::alpha(SemanticColorRole::Background, 0.84)
        .resolve(StyleModelRef::new(mode))
        .as_rgba_array()
}

/// 和 `color-mix(in srgb, var(--accent) 12%, transparent)` 同一条混合。比例写死。
fn vue_accent(mode: ThemeAppearance) -> [f32; 4] {
    SemanticColorMix::alpha(SemanticColorRole::Accent, 0.12)
        .resolve(StyleModelRef::new(mode))
        .as_rgba_array()
}

/// 内边距盒、圆心（宽 28%、高 32%）、到最远角的像素半径。边框按 1px 从外框往里收。
fn vue_glow(size: [f32; 2]) -> (LayoutBox, [f32; 2], f32) {
    let origin = [1.0, 1.0];
    let inner = [size[0] - 2.0, size[1] - 2.0];
    let center = [origin[0] + inner[0] * 0.28, origin[1] + inner[1] * 0.32];
    let right = origin[0] + inner[0];
    let bottom = origin[1] + inner[1];
    let mut reach = 0.0_f32;
    for corner in [
        origin,
        [right, origin[1]],
        [origin[0], bottom],
        [right, bottom],
    ] {
        reach = reach.max((corner[0] - center[0]).hypot(corner[1] - center[1]));
    }
    (
        LayoutBox {
            x: origin[0],
            y: origin[1],
            width: inner[0],
            height: inner[1],
        },
        center,
        reach,
    )
}

/// 录下来的浅光：边框内侧的圆角矩形，和它上面的径向圆。
struct Glow {
    rect: LayoutBox,
    radii: [f32; 4],
    shape: GradientShape,
    stops: Vec<(f32, [f32; 4])>,
}

/// 落影、它的外框和圆角，以及浅光。落影在内建底色之前，浅光在底色之后。
fn layers(
    ops: &[PaintOp],
    theme: &str,
) -> (
    nana_ui::runtime::ComponentElevation,
    LayoutBox,
    [f32; 4],
    Glow,
) {
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
        .unwrap_or_else(|| panic!("{theme} 卡片要有落影"));
    let default_at = ops
        .iter()
        .position(|op| matches!(op, PaintOp::DrawDefault))
        .unwrap_or_else(|| panic!("{theme} 底色还要画"));
    let glow_at = ops
        .iter()
        .position(|op| glow_of(std::slice::from_ref(op)).is_some())
        .unwrap_or_else(|| panic!("{theme} 卡片要有浅光"));
    assert!(
        shadow_at < default_at && default_at < glow_at,
        "{theme} 顺序应是落影、底色、浅光"
    );
    let PaintOp::RoundedRect {
        rect,
        radii,
        fill,
        shadow,
        ..
    } = &ops[shadow_at]
    else {
        unreachable!()
    };
    assert!(fill.is_none(), "{theme} 落影那一层不另铺底色");
    let glow = glow_of(std::slice::from_ref(&ops[glow_at])).expect("浅光");
    (shadow.clone().expect("落影"), *rect, *radii, glow)
}

/// 第一条径向浅光。没有时是 `None`。
fn glow_of(ops: &[PaintOp]) -> Option<Glow> {
    for op in ops {
        let PaintOp::RoundedRect {
            rect,
            radii,
            fill: Some(ResolvedPaint::Gradient(gradient)),
            shadow: None,
            ..
        } = op
        else {
            continue;
        };
        return Some(Glow {
            rect: *rect,
            radii: *radii,
            shape: gradient.shape,
            stops: gradient.stops.clone(),
        });
    }
    None
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
