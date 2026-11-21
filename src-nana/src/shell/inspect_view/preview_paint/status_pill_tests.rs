//! 预览读取中的状态胶囊。
//!
//! 胶囊的底是 `color-mix(in srgb, var(--bg-elev) 84%, transparent)`。
//! 图片读取还带 132 宽的不定进度轨（NanaUI 的 `Progress`）。
//! 文本读取是同一枚胶囊，没有进度条。音视频准备卡不走这里。

use super::loading_card_tests::sweep_track;
use crate::shell::inspect::PreviewBody;
use crate::shell::view_harness::ShellHarness;
use crate::shell::{ShellMessage, ShellViewModel};
use nana_ui::runtime::LengthSpec;
use nana_ui_core::{SemanticColorMix, SemanticColorRole};

const TRACK: f32 = 132.0;
/// `color-mix(in srgb, var(--bg-elev) 84%, transparent)` 里表面色的比例。
const PILL_FILL: f32 = 0.84;

/// 图片读取胶囊用 84% 底，带 132 宽的不定进度轨。
/// 文本读取是同一层底，不画进度。图已经画出以后胶囊收掉。
#[test]
fn image_status_capsule_sweeps_on_its_fill() {
    let mut image = scene("preview-image");
    image.preview_pixels = None;
    image.preview_token = None;
    image.inspect.body = PreviewBody::Empty;
    image.inspect.loading = true;
    image.reduce(ShellMessage::TaskProgressLoaded(Vec::new()));
    let harness = ShellHarness::mount(image);
    assert!(
        harness.find("准备播放").is_some(),
        "图片读取胶囊第一段是准备播放"
    );
    assert!(harness.find("准备媒体").is_some(), "第二段是准备媒体");
    assert_fill(&harness, "inspect-status-pill");
    let track = sweep_track(&harness, "inspect-status-progress");
    assert!(track.indeterminate, "图片读取是不定进度");
    assert_eq!(track.thickness, None, "粗细不写，跟主题的 Sm 间距档");
    assert_eq!(
        super::loading_card_tests::track_thickness(&harness, "inspect-status-progress"),
        crate::theme_map::tiers::space_px(nana_ui_core::SpacingStep::Sm),
    );
    assert_eq!(
        track.style.layout.width,
        Some(LengthSpec::Px(TRACK)),
        "状态胶囊里的轨道是 132px，不铺满准备卡"
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

    let ready = ShellHarness::mount(scene("preview-image"));
    assert!(
        ready.keyed("inspect-status-pill").is_none(),
        "图已经画出以后不留读取胶囊"
    );
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
