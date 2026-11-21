//! 旧播放条的 8px 进度轨自绘。
//!
//! 播放条已改用 NanaUI 的 `MediaTransportBar`，自己的进度、按钮和滑杆都不再自绘；这里只剩
//! `shell/hot.rs` 的 `TrackRatioField` 还引用的进度轨，等那边一起清掉。

use nana_ui::runtime::{BoxPaint, LayoutBox, PaintContext, PaintPath, Painter, SemanticColorRole};

/// 进度轨：bg-subtle 胶囊，accent 填充从左边按比例铺开，被胶囊裁出圆头。
pub(crate) struct ProgressTrack {
    pub ratio: f32,
}

impl Painter for ProgressTrack {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let [width, height] = cx.size();
        let rect = LayoutBox {
            x: 0.0,
            y: 0.0,
            width,
            height,
        };
        let radius = height / 2.0;
        cx.rounded_rect(rect, radius, BoxPaint::fill(SemanticColorRole::Subtle));
        let filled = self.ratio.clamp(0.0, 1.0) * width;
        if filled <= 0.0 {
            return;
        }
        let mut clip = PaintPath::new();
        clip.rounded_rect(rect, [radius; 4]);
        cx.push_clip(&clip);
        cx.rounded_rect(
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: filled,
                height,
            },
            0.0,
            BoxPaint::fill(SemanticColorRole::Accent),
        );
        cx.pop_clip();
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.ratio.to_bits())
    }
}
