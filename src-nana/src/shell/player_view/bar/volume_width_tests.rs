//! 音量滑杆宽度的测试。

use super::volume_slider_width;

/// 空会话也画这条滑杆。窗口不窄于 `120 / 0.18` 时仍是 120；再窄按 18vw，最短 72。
#[test]
fn volume_slider_shrinks_with_the_viewport_down_to_72() {
    assert_eq!(volume_slider_width(1200.0), 120.0);
    assert_eq!(volume_slider_width(1120.0), 120.0);
    assert!((volume_slider_width(600.0) - 108.0).abs() < 0.01);
    assert_eq!(volume_slider_width(300.0), 72.0);
    assert_eq!(volume_slider_width(f32::NAN), 120.0);
}
