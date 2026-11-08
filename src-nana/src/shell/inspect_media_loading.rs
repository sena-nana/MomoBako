//! 音视频准备卡的落影和扫动进度。
//!
//! Vue `.media-playlist-runtime__loading` 的 `box-shadow: 0 12px 32px color-mix(in srgb, var(--bg) 70%, transparent)`。
//! 色条宽 42%，`media-preview-progress-sweep 1.05s ease-in-out infinite` 把 `translateX` 从 -120% 扫到 250%。
//! 百分比相对色条自己的宽度。时间由动效时钟给出，不依赖真窗口。

use super::LoadingCardShadow;
use crate::frame_clock::FrameStep;
use crate::shell::ShellViewModel;
use crate::shell::view_harness::ShellHarness;
use nana_ui::runtime::{LayoutBox, PaintOp, PaintRecording, PaintState, Painter};
use nana_ui_core::{
    RadiusTier, SemanticColorMix, SemanticColorRole, StyleModelRef, ThemeAppearance,
};

const TRACK: f32 = 200.0;
const FILL: f32 = TRACK * 0.42;

/// 准备卡有 Vue 的落影，色条按可控时间停在扫动的起点、中点、四分之一程和循环回到起点。
#[test]
fn loading_card_sweeps_and_drops_the_vue_shadow() {
    let mut harness = ShellHarness::mount(scene("online-preview-preparing"));
    assert_ne!(
        harness.model.motion.preview_sweep_percent(),
        0.0,
        "准备卡在显示时扫动已经接上时钟"
    );
    assert!(harness.model.motion.active(), "扫动循环要帧");

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

    let track = node_style(&harness, "inspect-media-loading-progress");
    assert_eq!(
        track.layout.overflow_x,
        nana_ui_core::OverflowSpec::Hidden,
        "扫出轨道的部分裁掉"
    );
    restart(&mut harness);
    assert!(
        (bar_percent(&harness) - (-120.0)).abs() < 0.05,
        "起点是 translateX(-120%)"
    );
    harness.frame_with(FrameStep::millis(525));
    assert!(
        (bar_percent(&harness) - 65.0).abs() < 0.05,
        "1.05 秒的一半，ease-in-out 正好在 -120 和 250 的中点"
    );
    harness.frame_with(FrameStep::millis(525));
    assert!(
        (bar_percent(&harness) - (-120.0)).abs() < 0.05,
        "走满 1.05 秒回到起点，接着循环"
    );
    restart(&mut harness);
    harness.frame_with(FrameStep::millis(262));
    let quarter = bar_percent(&harness);
    assert!(
        (quarter - (-72.4)).abs() < 1.0,
        "四分之一程应按 ease-in-out，得到 {quarter}"
    );
    assert!((quarter - (-27.5)).abs() > 20.0, "不该是线性插值");
}

/// 从这一刻重新开一条 1.05 秒的扫动，并把起点写进色条。
fn restart(harness: &mut ShellHarness) {
    harness.model.motion.set_preview_sweep(false);
    harness.model.motion.set_preview_sweep(true);
    harness.frame_with(FrameStep::millis(0));
}

/// 色条左缘相对自己宽度的 `translateX` 百分比。轨道宽按 200 录，和布局宽无关。
fn bar_percent(harness: &ShellHarness) -> f32 {
    let style = node_style(harness, "inspect-media-loading-progress");
    let painter = style.painter.as_ref().expect("色条自绘").painter();
    let theme = nana_ui::theme::builtin_theme_arc(nana_ui::theme::ThemeAppearance::Dark);
    let recording =
        PaintRecording::record(painter, theme.as_ref(), [TRACK, 6.0], PaintState::default());
    let rect = bar_rect(&recording.behind_children);
    assert!(
        (rect.width - FILL).abs() < 0.05,
        "色条宽应是轨道的 42%，得到 {}",
        rect.width
    );
    assert!(
        (rect.height - 6.0).abs() < 0.05 && rect.y.abs() < 0.05,
        "色条应贴着轨道：{rect:?}"
    );
    rect.x / FILL * 100.0
}

fn bar_rect(ops: &[PaintOp]) -> LayoutBox {
    let mut found = None;
    let mut saw_default = false;
    for op in ops {
        if matches!(op, PaintOp::DrawDefault) {
            saw_default = true;
        }
        if let PaintOp::RoundedRect {
            rect,
            fill: Some(_),
            ..
        } = op
        {
            assert!(saw_default, "色条应画在轨道底色上面");
            assert!(found.replace(*rect).is_none(), "只画一条色条");
        }
    }
    found.expect("准备卡要有扫动色条")
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
