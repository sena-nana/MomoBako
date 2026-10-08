//! 圆角形状：指数 2 是圆弧，4 是 CSS `corner-shape: superellipse(2)`（squircle）。
//!
//! 着色器在 `color.wgsl` 里用常量 `CORNER_EXPONENT` 算圆角距离和阴影。绘制器创建
//! 着色器模块时按这里的全局值替换那一行；值变了，宿主丢掉旧绘制器重建，所以切换
//! 只在用户改设置时付一次管线编译的代价，平时每帧不多读任何统一变量。

use std::borrow::Cow;
use std::sync::atomic::{AtomicU32, Ordering};

/// `color.wgsl` 里写死的那一行。改着色器时这里要一起改。
const PLACEHOLDER: &str = "const CORNER_EXPONENT: f32 = 2.0;";
/// 2.0 的位模式：默认画圆弧。
static EXPONENT: AtomicU32 = AtomicU32::new(0x4000_0000);

/// 设置之后新建的着色器模块用的圆角指数。非有限值回到 2，范围夹在 2–8。
pub fn set_corner_exponent(exponent: f32) {
    EXPONENT.store(clamp_exponent(exponent).to_bits(), Ordering::Relaxed);
}

fn clamp_exponent(exponent: f32) -> f32 {
    if exponent.is_finite() { exponent.clamp(2.0, 8.0) } else { 2.0 }
}

/// 当前的圆角指数。
pub fn corner_exponent() -> f32 {
    f32::from_bits(EXPONENT.load(Ordering::Relaxed))
}

/// 混进管线缓存键：设备级缓存按着色器标识复用管线，圆角指数不同的不能共用。
pub(crate) fn cache_salt() -> u64 {
    let exponent = corner_exponent();
    if exponent == 2.0 { 0 } else { u64::from(exponent.to_bits()) << 1 }
}

/// 把着色器源码里的圆角指数换成 `exponent`。圆弧时原样借用，不分配。
pub(crate) fn specialize(source: &'static str, exponent: f32) -> Cow<'static, str> {
    if exponent == 2.0 {
        return Cow::Borrowed(source);
    }
    debug_assert!(source.contains(PLACEHOLDER), "着色器没有圆角指数占位行");
    Cow::Owned(source.replacen(PLACEHOLDER, &format!("const CORNER_EXPONENT: f32 = {exponent:.4};"), 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specialize_rewrites_only_the_placeholder_line() {
        let source = "const CORNER_EXPONENT: f32 = 2.0;\nfn f() {}";
        assert!(matches!(specialize(source, 2.0), Cow::Borrowed(_)));
        assert_eq!(specialize(source, 4.0), "const CORNER_EXPONENT: f32 = 4.0000;\nfn f() {}");
    }

    /// 不碰全局值：并行的绘制测试都按默认圆弧建绘制器。
    #[test]
    fn exponent_is_clamped_and_defaults_to_round() {
        assert_eq!(clamp_exponent(f32::NAN), 2.0);
        assert_eq!(clamp_exponent(12.0), 8.0);
        assert_eq!(clamp_exponent(1.0), 2.0);
        assert_eq!(clamp_exponent(4.0), 4.0);
    }
}
