//! 预览读取中的状态胶囊。
//!
//! Vue `.media-preview__status` / `.text-preview__status` 的底是 `color-mix(in srgb, var(--bg-elev) 84%, transparent)`。
//! 图片读取还带 `.media-preview__download-progress--indeterminate`：轨道宽 132，色条宽 42%，
//! 按 `media-preview-progress-sweep 1.05s ease-in-out infinite` 从 `translateX(-120%)` 扫到 250%。
//! 文本读取是同一枚胶囊，没有进度条。音视频准备卡不走这里。

use crate::frame_clock::FrameStep;
use crate::shell::inspect::PreviewBody;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellViewModel};
use nana_ui::runtime::{LayoutBox, LengthSpec, PaintOp, PaintRecording, PaintState};
use nana_ui_core::{SemanticColorMix, SemanticColorRole};

const TRACK: f32 = 132.0;
const FILL: f32 = TRACK * 0.42;
/// `color-mix(in srgb, var(--bg-elev) 84%, transparent)` 里表面色的比例。
const PILL_FILL: f32 = 0.84;

/// 图片读取胶囊用 Vue 的 84% 底，色条按可控时间停在扫动的起点、中点和循环起点。
/// 文本读取是同一层底，不画进度，也不要扫动帧。图已经画出以后胶囊收掉，扫动停下。
#[test]
fn image_status_capsule_sweeps_on_the_vue_fill() {
    let mut image = scene("preview-image");
    image.preview_pixels = None;
    image.preview_token = None;
    image.inspect.body = PreviewBody::Empty;
    image.inspect.loading = true;
    image.reduce(ShellMessage::TaskProgressLoaded(Vec::new()));
    let mut harness = ShellHarness::mount(image);
    assert!(
        harness.find("准备播放").is_some(),
        "图片读取胶囊第一段是准备播放"
    );
    assert!(harness.find("准备媒体").is_some(), "第二段是准备媒体");
    assert_fill(&harness, "inspect-status-pill");
    assert!(harness.model.motion.active(), "图片读取的扫动要帧");
    assert_ne!(
        harness.model.motion.preview_sweep_percent(),
        0.0,
        "扫动已经接上时钟"
    );
    let track = node_style(&harness, "inspect-status-progress");
    assert_eq!(
        track.layout.width,
        Some(LengthSpec::Px(TRACK)),
        "状态胶囊里的轨道是 132px，不铺满准备卡"
    );
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
        "1.05 秒的一半，ease-in-out 在 -120 和 250 的中点"
    );
    harness.frame_with(FrameStep::millis(525));
    assert!(
        (bar_percent(&harness) - (-120.0)).abs() < 0.05,
        "走满 1.05 秒回到起点"
    );

    let mut text = scene("preview-text");
    text.inspect.body = PreviewBody::Empty;
    text.inspect.loading = true;
    text.reduce(ShellMessage::TaskProgressLoaded(Vec::new()));
    let text = ShellHarness::mount(text);
    assert!(text.find("读取文本").is_some(), "文本读取仍是这枚胶囊");
    assert_fill(&text, "inspect-status-pill");
    assert!(
        text.keyed("inspect-status-progress").is_none(),
        "文本读取不画进度条"
    );
    assert_eq!(
        text.model.motion.preview_sweep_percent(),
        0.0,
        "没有色条时不要扫动"
    );

    let ready = ShellHarness::mount(scene("preview-image"));
    assert!(
        ready.keyed("inspect-status-pill").is_none(),
        "图已经画出以后不留读取胶囊"
    );
    assert_eq!(
        ready.model.motion.preview_sweep_percent(),
        0.0,
        "准备卡和读取胶囊都不在时扫动停下"
    );
}

/// 从这一刻重新开一条 1.05 秒的扫动。
fn restart(harness: &mut ShellHarness) {
    harness.model.motion.set_preview_sweep(false);
    harness.model.motion.set_preview_sweep(true);
    harness.frame_with(FrameStep::millis(0));
}

/// 色条左缘相对自己宽度的 `translateX` 百分比。按轨道的 132 录。
fn bar_percent(harness: &ShellHarness) -> f32 {
    let style = node_style(harness, "inspect-status-progress");
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
    for op in ops {
        if let PaintOp::RoundedRect {
            rect,
            fill: Some(_),
            ..
        } = op
        {
            assert!(found.replace(*rect).is_none(), "只画一条色条");
        }
    }
    found.expect("读取胶囊要有扫动色条")
}

/// 胶囊底是 bg-elev 的 84%，不再铺不透明表面。边框仍是 border-soft。
fn assert_fill(harness: &ShellHarness, key: &str) {
    let style = node_style(harness, key);
    assert_eq!(style.background, None, "{key} 不能再铺不透明的 bg-elev");
    assert_eq!(
        style.interaction.base.background_mix,
        Some(SemanticColorMix::alpha(
            SemanticColorRole::Surface,
            PILL_FILL
        )),
        "{key}"
    );
    assert_eq!(style.border, Some(SemanticColorRole::BorderSoft), "{key}");
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
