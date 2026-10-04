//! `DESIGN.md` 令牌到 Nana 主题角色的对应。
//!
//! 壳层只消费这里的角色，不在按钮或文本上写死 RGB。Nana 调色板的具体数值
//! 由 `SemanticPalette` 持有；本模块只锁住角色、字号和侧栏尺寸的对应关系。

use nana_ui::theme::{SemanticPalette, ThemeMode, type_scale};

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

/// 指定主题的主工作区背景，供离屏清屏色比对。
pub fn background_rgba(mode: ThemeMode) -> [f32; 4] {
    let color = SemanticPalette::for_mode(mode).background;
    [color.r, color.g, color.b, color.a]
}

/// 离屏清屏色是否等于该主题的 `background` 角色，而不是按钮上的写死颜色。
pub fn clear_matches_background(theme: &str, clear: [f32; 4]) -> bool {
    let mode = match theme {
        "light" => ThemeMode::Light,
        "dark" => ThemeMode::Dark,
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
        let light = background_rgba(ThemeMode::Light);
        let dark = background_rgba(ThemeMode::Dark);
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
