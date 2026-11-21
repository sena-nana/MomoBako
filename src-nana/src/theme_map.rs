//! `DESIGN.md` 令牌到 Nana 主题角色的对应。
//!
//! 壳层的按钮和表面只取这里对应的主题角色，不写死 RGB；按钮由 `theme_map/button_tests.rs` 扫源码检查，
//! 设计上写死、两套主题共用的少数颜色（日志级别、页纸、投影……）列在那里的允许清单里。Nana 调色板的
//! 具体数值由 `SemanticPalette` 持有；本模块只锁住颜色角色、字号、侧栏宽度和圆角刻度的对应，另给离屏
//! 清屏色比对用的背景色。

use nana_ui::runtime::{SemanticColorRole, SemanticPaint, Text};
use nana_ui::theme::{SemanticPalette, ThemeAppearance, ThemeMetrics, UI_METRICS, type_scale};
use std::sync::Arc;

use nana_ui_core::{
    ControlSize, LayoutStyle, LineRole, SemanticColorMix, TextWeight, ThemeToken,
    TypeRole as NanaTypeRole, TypographyTokens,
};

/// 一条 CSS 变量到 Nana 调色板字段的对应。
pub struct ColorRole {
    /// `DESIGN.md` 的令牌名。
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

/// 侧栏宽度约束，单位是逻辑像素：拖宽的上下限和默认宽。
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

/// 写一段字的字号和字重。字号落在主题字阶上时用 Nana 的字号角色，跟着装上的主题走，落在台阶之间的照写数值；
/// 字重只取主题的四档。行盒取主题的行盒档：字号到正文档为止是 Line（默认 16），再大是 Tall（默认 18），
/// 和 NanaUI 按字号配的行盒同值，主题换行盒档时跟着变。调用方另写行高时用 [`tiers::text_line_px`]。
pub fn typed_text(node: Text, size: f32, weight: TextWeight) -> Text {
    let mut node = node.font_size(size).weight(weight);
    let line = match ControlSize::nearest_text(size) {
        ControlSize::Small | ControlSize::Medium => LineRole::Line,
        ControlSize::Large => LineRole::Tall,
    };
    tiers::line(Arc::make_mut(&mut node.style.layout), line);
    match tiers::type_role_at(size) {
        Some(role) => node.role(role),
        None => node,
    }
}

/// 写控件样式的字号档：默认主题下的像素值进 `font_size`，主题字号令牌进 `theme.font_size`。
/// 解析时主题令牌盖过数值字号，所以按钮、输入框这类自带字号令牌的控件只改数值不生效，要连令牌一起换；
/// 装上的主题换字阶时控件跟着变。行高不动，由调用方另定。
pub fn control_font(layout: &mut LayoutStyle, role: NanaTypeRole) {
    layout.font_size = Some(TypographyTokens::DEFAULT.size(role));
    layout.theme.font_size = Some(ThemeToken::Type(role).into());
}

/// 写控件样式的字重档：默认主题下的数值进 `font_weight`，主题字重令牌进 `theme.font_weight`。
/// 解析时主题令牌盖过数值字重和字重台阶，所以按钮、勾选框这类自带字重令牌的控件只改数值不生效，要连令牌一起换；
/// 装上的主题换字重时控件跟着变。
pub fn control_weight(layout: &mut LayoutStyle, weight: TextWeight) {
    layout.font_weight = Some(TypographyTokens::DEFAULT.weight(weight));
    layout.theme.font_weight = Some(ThemeToken::Weight(weight).into());
}

/// 停用控件的字色：禁用不透明度（[`tiers::DISABLED_OPACITY`]）整体盖在不透明底色 `base` 上的结果。
///
/// `SemanticColorMix::new` 在 sRGB 通道里按比例混（带预乘透明度，两种颜色都不透明时就是逐通道线性插值），
/// 和浏览器合成 `opacity` 的结果一致，混出来的是不透明色；直接写节点不透明度会被渲染器按线性空间合成，
/// 会偏亮。`colors_from_style` 的 `IconButton` 照写的停用样子画，NanaUI 不再整枚淡化。
pub fn disabled_paint(foreground: SemanticColorRole, base: SemanticColorRole) -> SemanticPaint {
    SemanticPaint {
        foreground_mix: Some(SemanticColorMix::new(
            foreground,
            base,
            tiers::DISABLED_OPACITY,
        )),
        ..SemanticPaint::default()
    }
}

/// 自绘里两个已解析的主题色在 sRGB 里按比例混，结果不透明：`t` 是 `b` 的份额。
/// 渐变色标这类要具体 RGBA 的地方用；能用角色混色（`SemanticColorMix`）的地方不用它。
pub fn mix_rgb(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        1.0,
    ]
}

/// 圆角半径 `--app-corner-radius` 的默认值。
pub const DEFAULT_CORNER_RADIUS_PX: f32 = 8.0;
/// 圆角半径设置的上下限。
pub const CORNER_RADIUS_RANGE: std::ops::RangeInclusive<f32> = 0.0..=20.0;

/// 圆角半径基数换成主题半径刻度。
///
/// 刻度都从 `--app-corner-radius` 乘出来：xs=0.5r、sm=0.75r、md=r、lg=1.25r，
/// MomoBako 另加 xl=1.5r、2xl=2r。2xl 落到 `RadiusTier::Xxl`，
/// 播放条、文件卡、预览页和工作台面板这类大块表面写这一档，装主题后跟着设置变。
/// 其余控件尺寸沿用 Nana 默认度量。
pub fn corner_metrics(radius: f32) -> ThemeMetrics {
    let radius = clamp_corner_radius(radius);
    ThemeMetrics {
        radius_xs: radius * 0.5,
        radius_sm: radius * 0.75,
        radius_md: radius,
        radius_lg: radius * 1.25,
        radius_xl: radius * 1.5,
        radius_xxl: radius * 2.0,
        ..UI_METRICS
    }
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

    /// 字阶上的字号和字重档跟主题走；不在档上的字号照写数值。行盒都按数值字号定。
    #[test]
    fn typed_text_follows_the_theme_steps_it_lands_on() {
        let meta = typed_text(Text::new("元信息"), 12.0, TextWeight::Medium);
        assert_eq!(meta.style.type_role, Some(NanaTypeRole::Meta));
        assert_eq!(meta.style.type_weight, Some(TextWeight::Medium));
        assert_eq!(meta.style.layout.font_size, Some(12.0));
        let numeric = Text::new("元信息").font_size(12.0);
        assert_eq!(
            meta.style.layout.line_height,
            numeric.style.layout.line_height
        );

        let odd = typed_text(Text::new("正文"), 24.0, TextWeight::Regular);
        assert_eq!(odd.style.type_role, None, "24px 不在字阶上");
        assert_eq!(odd.style.type_weight, Some(TextWeight::Regular));
        assert_eq!(odd.style.layout.font_size, Some(24.0));
        assert_eq!(odd.style.layout.font_weight, None);

        for (size, role) in [
            (11.0, NanaTypeRole::Hint),
            (13.0, NanaTypeRole::Body),
            (14.0, NanaTypeRole::Section),
            (15.0, NanaTypeRole::CardTitle),
            (16.0, NanaTypeRole::Heading),
            (18.0, NanaTypeRole::Title),
            (20.0, NanaTypeRole::Display),
        ] {
            assert_eq!(
                typed_text(Text::new(""), size, TextWeight::Bold)
                    .style
                    .type_role,
                Some(role)
            );
        }
    }

    /// 控件字号档同时写数值和主题令牌，数值是默认主题下这一档的像素。
    #[test]
    fn control_font_writes_the_step_and_its_theme_token() {
        let mut layout = LayoutStyle::default();
        layout.font_size = Some(22.0);
        control_font(&mut layout, NanaTypeRole::Section);
        assert_eq!(layout.font_size, Some(type_scale::SECTION));
        assert_eq!(
            layout.theme.font_size,
            Some(ThemeToken::Type(NanaTypeRole::Section).into())
        );
    }

    /// 控件字重档同时写数值和主题令牌，数值是默认主题下这一档的字重。
    #[test]
    fn control_weight_writes_the_step_and_its_theme_token() {
        let mut layout = LayoutStyle::default();
        layout.font_weight = Some(650);
        control_weight(&mut layout, TextWeight::Semibold);
        assert_eq!(layout.font_weight, Some(type_scale::SEMIBOLD));
        assert_eq!(
            layout.theme.font_weight,
            Some(ThemeToken::Weight(TextWeight::Semibold).into())
        );
    }

    /// 字的行盒跟着主题的行盒档。
    #[test]
    fn typed_text_line_box_follows_the_line_token() {
        let node = typed_text(Text::new("角标"), 13.0, TextWeight::Medium);
        assert_eq!(
            node.style.layout.theme.line_height,
            Some(ThemeToken::Line(LineRole::Line).into()),
        );
    }

    /// 停用字色按主题的停用档混进底色。
    #[test]
    fn disabled_paint_mixes_by_the_disabled_layer() {
        let paint = disabled_paint(SemanticColorRole::Text, SemanticColorRole::Background);
        assert_eq!(
            paint.foreground_mix,
            Some(SemanticColorMix::new(
                SemanticColorRole::Text,
                SemanticColorRole::Background,
                tiers::DISABLED_OPACITY,
            ))
        );
    }

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
    fn corner_scale_follows_the_radius_tokens() {
        let metrics = corner_metrics(8.0);
        assert_eq!(
            [
                metrics.radius_xs,
                metrics.radius_sm,
                metrics.radius_md,
                metrics.radius_lg,
                metrics.radius_xl,
                metrics.radius_xxl
            ],
            [4.0, 6.0, 8.0, 10.0, 12.0, 16.0]
        );
        assert_eq!(corner_metrics(40.0).radius_xxl, 40.0);
        assert_eq!(corner_metrics(40.0).radius_md, 20.0);
        assert_eq!(corner_metrics(-3.0).radius_md, 0.0);
        assert_eq!(corner_metrics(f32::NAN).radius_md, DEFAULT_CORNER_RADIUS_PX);
        assert_eq!(
            corner_metrics(8.0).control_height,
            UI_METRICS.control_height
        );
    }
}

#[cfg(test)]
mod button_tests;
pub mod tiers;
