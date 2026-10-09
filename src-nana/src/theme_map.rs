//! `DESIGN.md` 令牌到 Nana 主题角色的对应。
//!
//! 壳层只消费这里的角色，不在按钮或文本上写死 RGB。Nana 调色板的具体数值
//! 由 `SemanticPalette` 持有；本模块只锁住角色、字号和侧栏尺寸的对应关系。

use nana_ui::theme::{SemanticPalette, ThemeAppearance, ThemeMetrics, UI_METRICS, type_scale};

/// 一条 CSS 变量到 Nana 调色板字段的对应。
pub struct ColorRole {
    /// `DESIGN.md` 与 `src/styles` 使用的变量名。
    pub css: &'static str,
    /// `SemanticPalette` 上的字段名。
    pub nana: &'static str,
}

/// 表面、文本和语义色的角色对应。数值不在这里复制。
pub const COLOR_ROLES: &[ColorRole] = &[
    ColorRole {
        css: "--bg",
        nana: "background",
    },
    ColorRole {
        css: "--bg-elev",
        nana: "surface",
    },
    ColorRole {
        css: "--bg-subtle",
        nana: "subtle",
    },
    ColorRole {
        css: "--bg-hover",
        nana: "hover",
    },
    ColorRole {
        css: "--bg-active",
        nana: "active",
    },
    ColorRole {
        css: "--border-soft",
        nana: "border_soft",
    },
    ColorRole {
        css: "--border",
        nana: "border",
    },
    ColorRole {
        css: "--border-strong",
        nana: "border_strong",
    },
    ColorRole {
        css: "--text",
        nana: "text",
    },
    ColorRole {
        css: "--text-muted",
        nana: "muted",
    },
    ColorRole {
        css: "--text-faint",
        nana: "faint",
    },
    ColorRole {
        css: "--accent",
        nana: "accent",
    },
    ColorRole {
        css: "--accent-strong",
        nana: "accent_strong",
    },
    ColorRole {
        css: "--accent-soft",
        nana: "accent_soft",
    },
    ColorRole {
        css: "--ok",
        nana: "success",
    },
    ColorRole {
        css: "--warn",
        nana: "warning",
    },
    ColorRole {
        css: "--err",
        nana: "danger",
    },
];

/// Vue 侧栏宽度约束，单位是逻辑像素。Phase 1 接入壳层时使用这组值。
pub const SIDEBAR_MIN_PX: f32 = 220.0;
pub const SIDEBAR_DEFAULT_PX: f32 = 276.0;
pub const SIDEBAR_MAX_PX: f32 = 480.0;

/// 一条 `DESIGN.md` 字号到 Nana `type_scale` 的对应。
pub struct TypeRole {
    /// 设计标准里的用途。
    pub design: &'static str,
    /// 设计标准里的字号。
    pub design_px: f32,
    /// Nana 字号角色。
    pub nana: &'static str,
    /// Nana 默认主题的实际字号。
    pub nana_px: f32,
}

/// 字号对应。Nana 的 body 是 13px、section 是 14px，和设计标准的“正文 14 / 行标签 13”互换。
/// 页面标题用 `title`，行标签用 `body`，默认工作文本用 `section`，直到主题本身调整。
pub const TYPE_ROLES: &[TypeRole] = &[
    TypeRole {
        design: "页面标题",
        design_px: 18.0,
        nana: "title",
        nana_px: type_scale::TITLE,
    },
    TypeRole {
        design: "默认工作文本",
        design_px: 14.0,
        nana: "section",
        nana_px: type_scale::SECTION,
    },
    TypeRole {
        design: "行标签和控件",
        design_px: 13.0,
        nana: "body",
        nana_px: type_scale::BODY,
    },
    TypeRole {
        design: "元信息和徽章",
        design_px: 12.0,
        nana: "meta",
        nana_px: type_scale::META,
    },
    TypeRole {
        design: "微型标签",
        design_px: 11.0,
        nana: "hint",
        nana_px: type_scale::HINT,
    },
];

/// Vue `--app-corner-radius` 的默认值（`src/ui/core/useCornerStyle.ts`）。
pub const DEFAULT_CORNER_RADIUS_PX: f32 = 8.0;
/// 圆角半径设置的上下限，和 Lilia `CORNER_RADIUS_MIN` / `CORNER_RADIUS_MAX` 一致。
pub const CORNER_RADIUS_RANGE: std::ops::RangeInclusive<f32> = 0.0..=20.0;

/// 圆角半径基数换成主题半径刻度。
///
/// Vue 的刻度都从 `--app-corner-radius` 乘出来：Lilia 给 xs=0.5r、sm=0.75r、md=r、lg=1.25r，
/// MomoBako 另加 xl=1.5r、2xl=2r。Nana `RadiusTier` 只到 Xl，2xl 用 [`radius_2xl`] 按像素给。
/// 其余控件尺寸沿用 Nana 默认度量。
pub fn corner_metrics(radius: f32) -> ThemeMetrics {
    let radius = clamp_corner_radius(radius);
    ThemeMetrics {
        radius_xs: radius * 0.5,
        radius_sm: radius * 0.75,
        radius_md: radius,
        radius_lg: radius * 1.25,
        radius_xl: radius * 1.5,
        ..UI_METRICS
    }
}

/// Vue `--radius-2xl`：两倍基数，用在播放条、详情卡这类大卡片上。
pub fn radius_2xl(radius: f32) -> f32 {
    clamp_corner_radius(radius) * 2.0
}

/// 非有限或越界的半径收回到 0–20，非有限时用默认 8。
pub fn clamp_corner_radius(radius: f32) -> f32 {
    if !radius.is_finite() {
        return DEFAULT_CORNER_RADIUS_PX;
    }
    radius.clamp(*CORNER_RADIUS_RANGE.start(), *CORNER_RADIUS_RANGE.end())
}

/// 外观是不是深色。设置只给出浅色和深色，自定义外观按默认的深色处理。
pub fn is_dark(appearance: ThemeAppearance) -> bool {
    appearance != ThemeAppearance::Light
}

/// 指定主题的主工作区背景，供离屏清屏色比对。
pub fn background_rgba(appearance: ThemeAppearance) -> [f32; 4] {
    let color = SemanticPalette::for_appearance(appearance).background;
    [color.r, color.g, color.b, color.a]
}

/// 离屏清屏色是否等于该主题的 `background` 角色，而不是按钮上的写死颜色。
pub fn clear_matches_background(theme: &str, clear: [f32; 4]) -> bool {
    let mode = match theme {
        "light" => ThemeAppearance::Light,
        "dark" => ThemeAppearance::Dark,
        _ => return false,
    };
    let expected = background_rgba(mode);
    clear
        .iter()
        .zip(expected)
        .all(|(actual, expected)| (actual - expected).abs() < 0.02)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_backgrounds_follow_distinct_palette_roles() {
        let light = background_rgba(ThemeAppearance::Light);
        let dark = background_rgba(ThemeAppearance::Dark);
        let luma = |color: [f32; 4]| color[0] + color[1] + color[2];
        assert!(luma(dark) < luma(light));
        assert!(clear_matches_background("light", light));
        assert!(clear_matches_background("dark", dark));
        assert!(!clear_matches_background("light", dark));
    }

    #[test]
    fn type_roles_keep_the_documented_design_scale() {
        assert!(
            TYPE_ROLES
                .iter()
                .any(|role| role.design == "页面标题" && role.nana_px == 18.0)
        );
        assert!(
            TYPE_ROLES
                .iter()
                .any(|role| role.nana == "section" && role.nana_px == 14.0)
        );
        assert!(
            TYPE_ROLES
                .iter()
                .any(|role| role.nana == "body" && role.nana_px == 13.0)
        );
        assert!(
            TYPE_ROLES
                .iter()
                .any(|role| role.nana == "meta" && role.nana_px == 12.0)
        );
        assert!(
            TYPE_ROLES
                .iter()
                .any(|role| role.nana == "hint" && role.nana_px == 11.0)
        );
        assert!(SIDEBAR_MIN_PX < SIDEBAR_DEFAULT_PX && SIDEBAR_DEFAULT_PX < SIDEBAR_MAX_PX);
    }

    #[test]
    fn corner_scale_follows_the_vue_radius_tokens() {
        let metrics = corner_metrics(8.0);
        assert_eq!(
            [metrics.radius_xs, metrics.radius_sm, metrics.radius_md, metrics.radius_lg, metrics.radius_xl],
            [4.0, 6.0, 8.0, 10.0, 12.0]
        );
        assert_eq!(radius_2xl(8.0), 16.0);
        assert_eq!(corner_metrics(40.0).radius_md, 20.0);
        assert_eq!(corner_metrics(-3.0).radius_md, 0.0);
        assert_eq!(corner_metrics(f32::NAN).radius_md, DEFAULT_CORNER_RADIUS_PX);
        assert_eq!(corner_metrics(8.0).control_height, UI_METRICS.control_height);
    }

    #[test]
    fn shell_buttons_do_not_hardcode_paint_colors() {
        let source = include_str!("shell/render.rs");
        for (index, line) in source.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            let lower = code.to_ascii_lowercase();
            assert!(
                !lower.contains("rgb(")
                    && !lower.contains("rgba(")
                    && !lower.contains("buttonpaintoverride"),
                "shell/render.rs:{} 写死了按钮颜色：{line}",
                index + 1
            );
            assert!(
                !code.contains('#'),
                "shell/render.rs:{} 含有颜色字面量：{line}",
                index + 1
            );
        }
    }
}
