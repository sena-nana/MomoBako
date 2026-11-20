//! 音频舞台两列（窄窗口改成上下两行）的分法。
//!
//! Vue `.media-preview__audio-layout` 写
//! `grid-template-columns: minmax(240px, 0.92fr) minmax(320px, 1.08fr)`。
//! `@media (max-width: 820px)` 改成一列，
//! `grid-template-rows: minmax(112px, 0.44fr) minmax(0, 1fr)`。
//! 最小像素先占住，多出来的空间再按 `fr` 分。不是把整段宽高按 0.92:1.08 切开。

use nana_ui::runtime::LengthSpec;
use nana_ui_core::{CalcBinOp, CalcExpr, LayoutStyle};

/// 宽窗口唱片列：`minmax(240px, 0.92fr)`。
const WIDE_STAGE: Track = Track {
    min_px: 240.0,
    fr: 0.92,
};
/// 宽窗口歌词列：`minmax(320px, 1.08fr)`。
const WIDE_INFO: Track = Track {
    min_px: 320.0,
    fr: 1.08,
};
/// 窄窗口上行：`minmax(112px, 0.44fr)`。
const NARROW_STAGE: Track = Track {
    min_px: 112.0,
    fr: 0.44,
};
/// 窄窗口下行：`minmax(0, 1fr)`。
const NARROW_INFO: Track = Track {
    min_px: 0.0,
    fr: 1.0,
};

/// 一条 `minmax(最小像素, Nfr)`。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Track {
    min_px: f32,
    fr: f32,
}

/// 唱片列在布局里占的那一条。宽窗口是横向，窄窗口是纵向。
pub(super) fn pin_stage(layout: &mut LayoutStyle, stacked: bool, gap: f32, measured: bool) {
    if stacked {
        pin(layout, false, NARROW_STAGE, NARROW_INFO, gap, measured);
    } else {
        pin(layout, true, WIDE_STAGE, WIDE_INFO, gap, measured);
    }
}

/// 歌词列在布局里占的那一条。份数和唱片列对调。
pub(super) fn pin_info(layout: &mut LayoutStyle, stacked: bool, gap: f32, measured: bool) {
    if stacked {
        pin(layout, false, NARROW_INFO, NARROW_STAGE, gap, measured);
    } else {
        pin(layout, true, WIDE_INFO, WIDE_STAGE, gap, measured);
    }
}

/// 把一条轨道写成主轴长度。`along_width` 时改宽，否则改高。
///
/// 视口还没量到时不写长度，避免落成 240、320 或按整段比例切开。
fn pin(
    layout: &mut LayoutStyle,
    along_width: bool,
    track: Track,
    other: Track,
    gap: f32,
    measured: bool,
) {
    layout.flex_grow = Some(0.0);
    layout.flex_shrink = Some(0.0);
    layout.flex_basis = None;
    if !measured {
        eprintln!("Nana 音频舞台视口还没量到，格子不按 minmax 分成像素");
        return;
    }
    let Some(spec) = track_spec(track, other, gap) else {
        return;
    };
    layout.flex_basis = Some(spec);
    if along_width {
        layout.width = Some(spec);
        layout.max_width = Some(spec);
        layout.min_width = Some(LengthSpec::Px(0.0));
        layout.height = Some(LengthSpec::Fill);
        layout.min_height = Some(LengthSpec::Px(0.0));
    } else {
        layout.height = Some(spec);
        layout.max_height = Some(spec);
        layout.min_height = Some(LengthSpec::Px(0.0));
        layout.min_width = Some(LengthSpec::Px(0.0));
    }
}

/// 两条轨道在已经量到的 `container` 里各占多少。
///
/// 容器或间距还没量到时返回 `None` 并记日志。
/// 算法：每条先拿最小像素；剩余 = 容器 − 两条最小像素 − 间距；
/// 剩余为正时按 `fr / (fr 之和)` 加到最小像素上，否则停在最小像素。
/// 算出来比容器还宽时收到容器里，不留一条比这一段还宽的格子。
pub(super) fn track_sizes(container: f32, gap: f32, wide: bool) -> Option<(f32, f32)> {
    let (first, second) = if wide {
        (WIDE_STAGE, WIDE_INFO)
    } else {
        (NARROW_STAGE, NARROW_INFO)
    };
    if !container.is_finite() || container <= 0.0 {
        eprintln!("Nana 音频舞台还没量到可分的空间，不按 minmax 分成像素：{container}");
        return None;
    }
    if !gap.is_finite() || gap < 0.0 {
        eprintln!("Nana 音频舞台间距无效，不按 minmax 分成像素：{gap}");
        return None;
    }
    let first_size = size_of(container, gap, first, second)?;
    let second_size = size_of(container, gap, second, first)?;
    Some((first_size, second_size))
}

/// 一条的像素。`fr` 无效时记日志并返回 `None`。
fn size_of(container: f32, gap: f32, track: Track, other: Track) -> Option<f32> {
    let share = share_of(track, other)?;
    let offset = track.min_px - share * (track.min_px + other.min_px + gap);
    let stretched = container * share + offset;
    let at_least_min = stretched.max(track.min_px);
    if at_least_min > container {
        eprintln!("Nana 音频舞台格子 {at_least_min}px 比容器 {container}px 还宽，收到容器里");
    }
    Some(at_least_min.min(container).max(0.0))
}

/// 写进主轴的长度。没有百分比底时解析不出来，不落成最小像素。
fn track_spec(track: Track, other: Track, gap: f32) -> Option<LengthSpec> {
    if !gap.is_finite()
        || gap < 0.0
        || !track.min_px.is_finite()
        || track.min_px < 0.0
        || !other.min_px.is_finite()
        || other.min_px < 0.0
    {
        eprintln!(
            "Nana 音频舞台轨道写不出来，间距或最小像素无效：间距 {gap}，本条 {}，另一条 {}",
            track.min_px, other.min_px
        );
        return None;
    }
    let share = share_of(track, other)?;
    let offset = track.min_px - share * (track.min_px + other.min_px + gap);
    let grown = CalcExpr::Binary {
        op: CalcBinOp::Add,
        left: Box::new(CalcExpr::Percent(share * 100.0)),
        right: Box::new(CalcExpr::Px(offset)),
    };
    let at_least_min = CalcExpr::Max(Box::new(CalcExpr::Px(track.min_px)), Box::new(grown));
    let capped = CalcExpr::Min(Box::new(CalcExpr::Percent(100.0)), Box::new(at_least_min));
    Some(LengthSpec::from_calc(capped))
}

/// `fr / (两条 fr 之和)`。份数不是有限、或加起来不大于 0 时记日志。
fn share_of(track: Track, other: Track) -> Option<f32> {
    let sum = track.fr + other.fr;
    if !track.fr.is_finite()
        || !other.fr.is_finite()
        || track.fr < 0.0
        || other.fr < 0.0
        || sum <= 0.0
    {
        eprintln!(
            "Nana 音频舞台 fr 无效，不按 minmax 分：{} 和 {}",
            track.fr, other.fr
        );
        return None;
    }
    Some(track.fr / sum)
}

#[cfg(test)]
mod tests {
    use super::{NARROW_INFO, NARROW_STAGE, WIDE_INFO, WIDE_STAGE, track_sizes, track_spec};
    use crate::shell::ShellViewModel;
    use crate::shell::view_harness::ShellHarness;

    /// 宽窗口两列是 `minmax(240px, 0.92fr)` 和 `minmax(320px, 1.08fr)`，空间够时不小于 240 和 320。
    /// 窄窗口上行是 `minmax(112px, 0.44fr)`，下行是 `minmax(0, 1fr)`。
    /// 还没量到容器时不给出像素。放不下最小像素时收到容器里，不编得比容器还宽。
    #[test]
    fn audio_layout_uses_vue_minmax_tracks() {
        for container in [0.0, -8.0, f32::NAN, f32::INFINITY] {
            assert!(
                track_sizes(container, 24.0, true).is_none(),
                "宽窗口还没量到：{container}"
            );
            assert!(
                track_sizes(container, 14.0, false).is_none(),
                "窄窗口还没量到：{container}"
            );
        }
        assert!(
            track_sizes(1000.0, f32::NAN, true).is_none(),
            "间距无效不分"
        );
        assert!(
            track_spec(WIDE_STAGE, WIDE_INFO, 40.0)
                .unwrap()
                .resolve_with(None, None)
                .is_none(),
            "没有容器不能分出像素"
        );

        // 1000 宽、间距 40：剩余 400。0.92fr 和 1.08fr 分剩余，不是分整段。
        let fr_sum = WIDE_STAGE.fr + WIDE_INFO.fr;
        let free = 1000.0 - WIDE_STAGE.min_px - WIDE_INFO.min_px - 40.0;
        let expect_stage = WIDE_STAGE.min_px + free * WIDE_STAGE.fr / fr_sum;
        let expect_info = WIDE_INFO.min_px + free * WIDE_INFO.fr / fr_sum;
        let (stage, info) = track_sizes(1000.0, 40.0, true).unwrap();
        assert!(
            (stage - expect_stage).abs() < 0.05 && (info - expect_info).abs() < 0.05,
            "宽列应是 {expect_stage} 和 {expect_info}，实际 {stage} 和 {info}"
        );
        assert!(stage >= 240.0 && info >= 320.0);
        let whole = (1000.0 - 40.0) * WIDE_STAGE.fr / fr_sum;
        assert!(
            (stage - whole).abs() > 8.0,
            "不应再按整段的 0.92:1.08 分：minmax {stage}，整段 {whole}"
        );
        let spec = track_spec(WIDE_STAGE, WIDE_INFO, 40.0)
            .unwrap()
            .resolve_with(Some(1000.0), None)
            .unwrap();
        assert!((spec - stage).abs() < 0.05, "写进布局的长度应和分法一致");

        // 400 宽放得下各自的下限，但放不下 240+320+间距。停在下限，两条都不比容器宽。
        let (tight_stage, tight_info) = track_sizes(400.0, 24.0, true).unwrap();
        assert!(
            (tight_stage - 240.0).abs() < 0.05 && (tight_info - 320.0).abs() < 0.05,
            "空间不够时停在 240 和 320"
        );
        assert!(tight_stage <= 400.0 && tight_info <= 400.0);
        // 180 比两条下限都窄：收到容器里，不编出 240 或 320。
        let (squeezed_stage, squeezed_info) = track_sizes(180.0, 14.0, true).unwrap();
        assert!(
            (squeezed_stage - 180.0).abs() < 0.05 && (squeezed_info - 180.0).abs() < 0.05,
            "比容器还宽的下限要收进来"
        );

        // 高 800、间距 14：上行 112 起按 0.44fr，下行从 0 起按 1fr。
        let row_sum = NARROW_STAGE.fr + NARROW_INFO.fr;
        let row_free = 800.0 - NARROW_STAGE.min_px - NARROW_INFO.min_px - 14.0;
        let expect_top = NARROW_STAGE.min_px + row_free * NARROW_STAGE.fr / row_sum;
        let expect_bottom = NARROW_INFO.min_px + row_free * NARROW_INFO.fr / row_sum;
        let (top, bottom) = track_sizes(800.0, 14.0, false).unwrap();
        assert!(
            (top - expect_top).abs() < 0.05 && (bottom - expect_bottom).abs() < 0.05,
            "窄行应是 {expect_top} 和 {expect_bottom}，实际 {top} 和 {bottom}"
        );
        assert!(top >= 112.0 && bottom >= 0.0);
        let row_whole = (800.0 - 14.0) * NARROW_STAGE.fr / row_sum;
        assert!(
            (top - row_whole).abs() > 8.0,
            "上行不应是整段高度的 0.44 份"
        );
        let row_spec = track_spec(NARROW_STAGE, NARROW_INFO, 14.0)
            .unwrap()
            .resolve_with(Some(800.0), None)
            .unwrap();
        assert!((row_spec - top).abs() < 0.05, "上行长度应和分法一致");

        assert_columns(1600.0, 1100.0);
        assert_rows(700.0, 1100.0);
        assert_rows(820.0, 1100.0);
    }

    /// 窗口宽于 820px 时，两列按容器里的 minmax 排开，并且不小于 240 和 320。
    fn assert_columns(width: f32, height: f32) {
        let gap = (width * 0.04).clamp(24.0, 58.0);
        let harness = ShellHarness::mount_at(scene(), width, height);
        let layout = box_of(&harness, "inspect-audio-layout");
        let stage = box_of(&harness, "inspect-audio-stage");
        let info = box_of(&harness, "inspect-audio-info");
        let (expect_stage, expect_info) =
            track_sizes(layout.width, gap, true).expect("宽窗口容器已量到");
        assert!(
            layout.width + 0.5 >= 240.0 + 320.0 + gap,
            "视口 {width} 的舞台要宽到放得下两列下限，实际 {}",
            layout.width
        );
        assert!(
            (stage.width - expect_stage).abs() < 1.5 && (info.width - expect_info).abs() < 1.5,
            "视口 {width} 的两列应是 {expect_stage:.1} 和 {expect_info:.1}，实际 {} 和 {}（容器 {}）",
            stage.width,
            info.width,
            layout.width
        );
        assert!(
            stage.width + 0.5 >= 240.0 && info.width + 0.5 >= 320.0,
            "空间够时不小于 240 和 320"
        );
        let whole = (layout.width - gap) * WIDE_STAGE.fr / (WIDE_STAGE.fr + WIDE_INFO.fr);
        assert!(
            (stage.width - whole).abs() > 8.0,
            "列宽不应是整段的 0.92:1.08：实际 {}，整段 {whole}",
            stage.width
        );
        assert!((stage.y - info.y).abs() < 1.5, "宽窗口两列同一行");
        let measured_gap = info.x - (stage.x + stage.width);
        assert!(
            (measured_gap - gap).abs() < 1.5,
            "列间距应是 {gap}，实际 {measured_gap}"
        );
        assert_eq!(style_grow(&harness, "inspect-audio-stage"), Some(0.0));
        assert_eq!(style_grow(&harness, "inspect-audio-info"), Some(0.0));
    }

    /// 窗口不宽于 820px 时，上行和下行按容器高度里的 minmax 排开。
    fn assert_rows(width: f32, height: f32) {
        let harness = ShellHarness::mount_at(scene(), width, height);
        let layout = box_of(&harness, "inspect-audio-layout");
        let stage = box_of(&harness, "inspect-audio-stage");
        let info = box_of(&harness, "inspect-audio-info");
        let (expect_top, expect_bottom) =
            track_sizes(layout.height, 14.0, false).expect("窄窗口容器已量到");
        assert!(
            layout.height + 0.5 >= 112.0 + 14.0,
            "视口 {width}×{height} 的舞台要高到能看出 0.44fr，实际 {}",
            layout.height
        );
        assert!(
            (stage.height - expect_top).abs() < 1.5 && (info.height - expect_bottom).abs() < 1.5,
            "视口 {width} 的两行应是 {expect_top:.1} 和 {expect_bottom:.1}，实际 {} 和 {}（容器高 {}）",
            stage.height,
            info.height,
            layout.height
        );
        assert!(stage.height + 0.5 >= 112.0, "上行不小于 112");
        let whole = (layout.height - 14.0) * NARROW_STAGE.fr / (NARROW_STAGE.fr + NARROW_INFO.fr);
        assert!(
            (stage.height - whole).abs() > 8.0,
            "上行不应是整段高度的 0.44 份：实际 {}，整段 {whole}",
            stage.height
        );
        assert!((stage.x - info.x).abs() < 1.5, "窄窗口两行同一列");
        let measured_gap = info.y - (stage.y + stage.height);
        assert!(
            (measured_gap - 14.0).abs() < 1.5,
            "行距应是 14，实际 {measured_gap}"
        );
    }

    fn style_grow(harness: &ShellHarness, key: &str) -> Option<f32> {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .node_style(id)
            .and_then(|style| style.layout.flex_grow)
    }

    fn box_of(harness: &ShellHarness, key: &str) -> nana_ui::runtime::LayoutBox {
        let id = harness.keyed(key).unwrap_or_else(|| panic!("没有 {key}"));
        harness
            .document()
            .context()
            .world()
            .layout_box(id)
            .unwrap_or_else(|| panic!("{key} 没有布局"))
    }

    fn scene() -> ShellViewModel {
        crate::shell::acceptance_gap_models()
            .into_iter()
            .find(|(name, _)| *name == "preview-audio")
            .unwrap_or_else(|| panic!("没有场景 preview-audio"))
            .1
    }
}
