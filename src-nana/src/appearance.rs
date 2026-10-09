//! 外观：设置里的主题和圆角落到 Nana 主题与宿主。
//!
//! Vue 由 `useTheme` 写 `data-theme`，`useCornerStyle` 写 `--app-corner-radius` 和
//! `data-corners`，整页跟着变。这里对应成一份 Nana `ThemeDefinition`：主题模式交给
//! 宿主（窗口底色、标题栏，经 `ApplicationState::theme`）和文档（调色板），圆角半径换成
//! `RadiusTier` 刻度，「平滑 / 普通」换成主题的圆角形状（CSS `superellipse(2)` / 圆弧）。
//! 对话框的站位、分区、开合动效和身后的遮罩也归主题：照 Vue `.modal-overlay`、`.modal-card` 和
//! `.dialog-card__*` 写进 [`DialogRecipe`] 和 [`EffectTokens`]，深浅两套相同。
//! 只有外观真的变了才重新编译和安装；失败只记日志，宿主和文档都保留上一份主题。

use std::sync::Arc;

use nana_ui::runtime::{Easing, LengthSpec, RuntimeDocument, SemanticColorRole, ViewportAxis};
use nana_ui::theme::{
    builtin_theme_arc, linear_scrim_alpha, CompiledTheme, CornerShape, DialogInsets, DialogMotion, DialogRecipe,
    DialogTransition, EffectTokens, RadiusTier, SemanticColor, ThemeAppearance, ThemeDefinition, ThemeId,
};
use nana_ui_platform::SystemAppearance;

use crate::shell::ShellViewModel;
use crate::theme_map;

/// 设置落到画面上的外观。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    pub mode: ThemeAppearance,
    /// 圆角半径基数，已夹在 0–20。
    pub corner_radius: f32,
    /// 「平滑」画超椭圆（CSS `superellipse(2)`），「普通」画圆弧。
    pub smooth_corners: bool,
}

impl Appearance {
    /// 从壳层设置读出外观。
    ///
    /// 浅色、深色照设置；跟随系统时用宿主报告的系统外观，读不到时用 Vue 的默认深色
    /// （Lilia `useTheme` 的 `DEFAULT_THEME`）。
    pub fn from_shell(shell: &ShellViewModel, system: Option<SystemAppearance>) -> Self {
        Self {
            mode: theme_mode(&shell.settings.theme, system),
            corner_radius: theme_map::clamp_corner_radius(shell.admin.corner_radius as f32),
            smooth_corners: smooth_corners(&shell.admin.corner_style),
        }
    }

    /// 离屏验收用：主题模式由会话指定，圆角半径照设置。
    pub fn for_mode(shell: &ShellViewModel, mode: ThemeAppearance) -> Self {
        Self {
            mode,
            corner_radius: theme_map::clamp_corner_radius(shell.admin.corner_radius as f32),
            smooth_corners: smooth_corners(&shell.admin.corner_style),
        }
    }

    /// 这份外观对应的主题定义。身份按模式区分，半径和圆角形状进度量；对话框配方和遮罩
    /// 深浅两套相同，遮罩以外的效果令牌沿用同模式的内置主题。
    pub fn definition(self) -> ThemeDefinition {
        // 设置只给出浅色和深色；自定义外观按默认的深色处理。
        let (id, mode) = match self.mode {
            ThemeAppearance::Light => (ThemeId::new("momobako.light"), ThemeAppearance::Light),
            ThemeAppearance::Dark | ThemeAppearance::Custom => (ThemeId::new("momobako.dark"), ThemeAppearance::Dark),
        };
        let shape = if self.smooth_corners { CornerShape::SQUIRCLE } else { CornerShape::Round };
        let base = ThemeDefinition::for_appearance(mode);
        let effects = modal_effects(base.effects);
        base.with_metrics(theme_map::corner_metrics(self.corner_radius))
            .with_corner_shape(shape)
            .with_effects(effects)
            .with_dialog(dialog_recipe())
            .with_id(id)
    }
}

/// `.modal-overlay { padding-top: 12vh }`：卡片离窗口顶边的距离，窗口高的百分比。
const DIALOG_TOP_VH: f32 = 12.0;
/// `.modal-card { max-height: 72vh }`。
const DIALOG_MAX_HEIGHT_VH: f32 = 72.0;
/// 标题行的最小高度：Vue 正文 14px、行高 1.55 的行盒。NanaUI 的标题按常规行高排，比它矮。
const DIALOG_HEADER_LINE: f32 = 21.7;
/// 标题前图标的格子：Vue 头部图标都是 14px。
const DIALOG_ICON_SIZE: f32 = 14.0;
/// 标题右侧关闭位的格子：`.repository-export-dialog__close` 24 见方。
const DIALOG_CLOSE_SIZE: f32 = 24.0;
/// `.dialog-card__header` 和 `.dialog-card__actions` 的 `gap`。
const DIALOG_GAP: f32 = 8.0;
/// `.modal-overlay { background: rgba(0, 0, 0, .45) }` 的不透明度，装进主题前换算到线性光。
const SCRIM_CSS_ALPHA: f32 = 0.45;
/// `.modal-overlay { backdrop-filter: blur(2px) }`：高斯标准差。
const SCRIM_BLUR: f32 = 2.0;
/// `.modal-enter-active { transition: opacity .16s ease }`，卡片淡入同长。
const DIALOG_FADE_MS: u16 = 160;
/// `.modal-card { transition: transform .18s cubic-bezier(.2, .8, .2, 1) }`。
const DIALOG_MOVE_MS: u16 = 180;
/// `.modal-enter-from .modal-card { transform: translateY(-8px) scale(.98) }` 的位移。
const DIALOG_ENTER_OFFSET: f32 = -8.0;
/// 同上，起点的缩放。
const DIALOG_ENTER_SCALE: f32 = 0.98;
/// CSS 的 `ease`。
const CSS_EASE: Easing = Easing::CubicBezier([0.25, 0.1, 0.25, 1.0]);

/// 对话框卡片的配方，照 Vue `.modal-overlay`、`.modal-card` 和 `.dialog-card__*`：
/// 距顶 12vh、最高 72vh、`--radius-xl`；标题行和正文内边距 12/14、底栏 10/14，标题行下和
/// 底栏上各一条 `--border-soft` 发丝线；标题行 `align-items: center; gap: 8px`；开场遮罩淡入
/// 160ms，卡片从上方 8px、0.98 倍处移回，退场倒放。
fn dialog_recipe() -> DialogRecipe {
    DialogRecipe {
        top: LengthSpec::Viewport { axis: ViewportAxis::Height, value: DIALOG_TOP_VH },
        max_height: LengthSpec::Viewport { axis: ViewportAxis::Height, value: DIALOG_MAX_HEIGHT_VH },
        icon_size: DIALOG_ICON_SIZE,
        close_size: DIALOG_CLOSE_SIZE,
        header_gap: DIALOG_GAP,
        header_min_height: DIALOG_HEADER_LINE,
        radius: RadiusTier::Xl,
        header: DialogInsets::symmetric(12.0, 14.0),
        body: DialogInsets::symmetric(12.0, 14.0),
        body_bottom_alone: None,
        footer: DialogInsets::symmetric(10.0, 14.0),
        action_gap: DIALOG_GAP,
        header_divider: Some(SemanticColorRole::BorderSoft),
        footer_divider: Some(SemanticColorRole::BorderSoft),
        motion: DialogMotion {
            scrim: DialogTransition::new(DIALOG_FADE_MS, CSS_EASE),
            card_fade: DialogTransition::new(DIALOG_FADE_MS, CSS_EASE),
            card_move: DialogTransition::new(DIALOG_MOVE_MS, Easing::CubicBezier([0.2, 0.8, 0.2, 1.0])),
            enter_offset_y: DIALOG_ENTER_OFFSET,
            enter_scale: DIALOG_ENTER_SCALE,
        },
    }
}

/// 对话框身后的遮罩：Vue 的 45% 黑和 2px 背景模糊，深浅主题相同。画家在线性光里合成，
/// CSS 的不透明度换算成同样暗的线性值；别的效果令牌（投影、媒体遮罩）照基础主题。
fn modal_effects(base: EffectTokens) -> EffectTokens {
    EffectTokens {
        modal_scrim: SemanticColor::rgba(0.0, 0.0, 0.0, linear_scrim_alpha(SCRIM_CSS_ALPHA)),
        modal_scrim_blur: SCRIM_BLUR,
        ..base
    }
}

/// 设置里的主题名换成主题模式。未知值按跟随系统处理并记日志。
pub fn theme_mode(theme: &str, system: Option<SystemAppearance>) -> ThemeAppearance {
    match theme {
        "light" => ThemeAppearance::Light,
        "dark" => ThemeAppearance::Dark,
        other => {
            if other != "system" {
                eprintln!("Nana 未知的主题设置，按跟随系统处理：{other}");
            }
            match system {
                Some(SystemAppearance::Light) => ThemeAppearance::Light,
                Some(SystemAppearance::Dark) | None => ThemeAppearance::Dark,
            }
        }
    }
}

/// 设置里的圆角形状。未知值按平滑处理并记日志（Windows 默认平滑）。
fn smooth_corners(style: &str) -> bool {
    match style {
        "smooth" => true,
        "round" => false,
        other => {
            eprintln!("Nana 未知的圆角形状，按平滑处理：{other}");
            true
        }
    }
}

/// 把外观编译后装进文档，返回装上的主题。只改圆角形状时运行时只重画、不重排。
/// 编译或安装失败记日志并返回 `None`，文档保留原主题。
pub fn install(document: &mut RuntimeDocument, appearance: Appearance) -> Option<Arc<CompiledTheme>> {
    let theme = match appearance.definition().compile() {
        Ok(theme) => Arc::new(theme),
        Err(error) => {
            eprintln!("Nana 外观编译失败：{error}");
            return None;
        }
    };
    match document.context_mut().set_theme_tokens(theme.clone()) {
        Ok(_) => Some(theme),
        Err(error) => {
            eprintln!("Nana 外观安装失败：{error}");
            None
        }
    }
}

/// 启动时宿主要用的主题（窗口还没有文档）。编译失败记日志，退回同模式的内置主题。
pub(crate) fn initial_theme(appearance: Appearance) -> Arc<CompiledTheme> {
    match appearance.definition().compile() {
        Ok(theme) => Arc::new(theme),
        Err(error) => {
            eprintln!("Nana 启动外观编译失败，先用内置主题：{error}");
            builtin_theme_arc(appearance.mode)
        }
    }
}

/// 每帧比对设置和已装外观，变了才重新安装，并把新主题交给宿主。装不上时下一帧再试。
pub(crate) fn sync(app: &mut crate::MomoBakoApplication, document: &mut RuntimeDocument) {
    let wanted = Appearance::from_shell(&app.shell, app.system_appearance);
    if app.applied_appearance == Some(wanted) {
        return;
    }
    if let Some(theme) = install(document, wanted) {
        app.applied_appearance = Some(wanted);
        app.theme = theme;
    }
}

/// 宿主报告系统深浅色变化。返回外观是否因此改变，改变时需要重画。
pub(crate) fn note_system_appearance(app: &mut crate::MomoBakoApplication, system: SystemAppearance) -> bool {
    let before = Appearance::from_shell(&app.shell, app.system_appearance);
    app.system_appearance = Some(system);
    Appearance::from_shell(&app.shell, app.system_appearance) != before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_themes_win_and_system_follows_the_host() {
        assert_eq!(theme_mode("light", Some(SystemAppearance::Dark)), ThemeAppearance::Light);
        assert_eq!(theme_mode("dark", Some(SystemAppearance::Light)), ThemeAppearance::Dark);
        assert_eq!(theme_mode("system", Some(SystemAppearance::Light)), ThemeAppearance::Light);
        assert_eq!(theme_mode("system", None), ThemeAppearance::Dark);
        assert_eq!(theme_mode("sepia", Some(SystemAppearance::Light)), ThemeAppearance::Light);
    }

    #[test]
    fn appearance_reads_settings_and_clamps_the_radius() {
        let mut shell = ShellViewModel::default();
        shell.settings.theme = "light".into();
        shell.admin.corner_radius = 32.0;
        shell.admin.corner_style = "round".into();
        let appearance = Appearance::from_shell(&shell, None);
        assert_eq!(appearance.mode, ThemeAppearance::Light);
        assert_eq!(appearance.corner_radius, 20.0);
        assert!(!appearance.smooth_corners);
        let compiled = appearance.definition().compile().expect("主题能编译");
        assert_eq!(compiled.metrics().radius_md, 20.0);
        assert_eq!(compiled.metrics().corner_shape, CornerShape::Round);
        shell.admin.corner_style = "smooth".into();
        let smooth = Appearance::from_shell(&shell, None).definition().compile().expect("主题能编译");
        assert_eq!(smooth.metrics().corner_shape, CornerShape::SQUIRCLE);
    }

    /// 深浅两套都装得上，装进文档以后对话框配方和遮罩是 Vue `.modal-*` 的值；遮罩以外的效果令牌
    /// 照同模式的内置主题。
    #[test]
    fn both_themes_install_with_the_vue_dialog_recipe_and_scrim() {
        for mode in [ThemeAppearance::Light, ThemeAppearance::Dark] {
            let mut shell = ShellViewModel::default();
            shell.settings.theme = if mode == ThemeAppearance::Light { "light" } else { "dark" }.into();
            let mut document = RuntimeDocument::new(nana_ui::runtime::DocumentId::new(1).expect("id"));
            assert!(install(&mut document, Appearance::from_shell(&shell, None)).is_some(), "{mode:?} 主题装不上");
            let theme = document.context().world().theme();
            assert_eq!(theme.appearance(), mode);

            let dialog = theme.recipes().dialog();
            assert!((dialog.top_in(1200.0, 800.0) - 96.0).abs() < 1e-3, "距顶 12vh");
            assert!((dialog.top_in(960.0, 600.0) - 72.0).abs() < 1e-3);
            assert!((dialog.max_height_in(1200.0, 800.0) - 576.0).abs() < 1e-3, "最高 72vh");
            assert_eq!(dialog.radius, RadiusTier::Xl);
            assert_eq!(dialog.header, DialogInsets::new(12.0, 12.0, 14.0));
            assert_eq!(dialog.body, DialogInsets::new(12.0, 12.0, 14.0));
            assert_eq!(dialog.body_bottom(false), 12.0, "没有底栏时正文照 CSS 留 12");
            assert_eq!(dialog.footer, DialogInsets::new(10.0, 10.0, 14.0));
            assert_eq!((dialog.action_gap, dialog.header_gap), (8.0, 8.0));
            assert_eq!((dialog.icon_size, dialog.close_size, dialog.header_min_height), (14.0, 24.0, 21.7));
            assert_eq!(dialog.header_divider, Some(SemanticColorRole::BorderSoft));
            assert_eq!(dialog.footer_divider, Some(SemanticColorRole::BorderSoft));
            let motion = dialog.motion;
            assert_eq!((motion.scrim.millis, motion.card_fade.millis, motion.card_move.millis), (160, 160, 180));
            assert_eq!(motion.scrim.easing, Easing::CubicBezier([0.25, 0.1, 0.25, 1.0]));
            assert_eq!(motion.card_move.easing, Easing::CubicBezier([0.2, 0.8, 0.2, 1.0]));
            assert_eq!((motion.enter_offset_y, motion.enter_scale), (-8.0, 0.98));

            let effects = theme.effects();
            let scrim = effects.modal_scrim;
            assert_eq!((scrim.r, scrim.g, scrim.b), (0.0, 0.0, 0.0));
            assert!((scrim.a - (1.0 - 0.55_f32.powf(2.2))).abs() < 1e-4, "CSS 的 45% 黑换到线性光约 0.732：{}", scrim.a);
            assert_eq!(effects.modal_scrim_blur, 2.0);
            let builtin = builtin_theme_arc(mode).effects();
            assert_eq!(EffectTokens { modal_scrim: builtin.modal_scrim, modal_scrim_blur: builtin.modal_scrim_blur, ..effects }, builtin);
        }
    }

    #[test]
    fn installing_twice_keeps_the_document_on_the_new_radius() {
        let mut document = RuntimeDocument::new(nana_ui::runtime::DocumentId::new(1).expect("id"));
        let mut shell = ShellViewModel::default();
        shell.settings.theme = "dark".into();
        shell.admin.corner_radius = 8.0;
        assert!(install(&mut document, Appearance::from_shell(&shell, None)).is_some());
        shell.admin.corner_radius = 12.0;
        assert!(install(&mut document, Appearance::from_shell(&shell, None)).is_some());
        let metrics = document.context().world().style_model().metrics;
        assert_eq!(metrics.radius_md, 12.0);
    }
}
