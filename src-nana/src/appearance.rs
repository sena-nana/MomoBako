//! 外观：设置里的主题和圆角落到 Nana 主题与宿主。
//!
//! Vue 由 `useTheme` 写 `data-theme`，`useCornerStyle` 写 `--app-corner-radius` 和
//! `data-corners`，整页跟着变。这里对应成一份 Nana `ThemeDefinition`：主题模式交给
//! 宿主（窗口底色、标题栏）和文档（调色板），圆角半径换成 `RadiusTier` 刻度，
//! 「平滑 / 普通」换成绘制层的圆角指数（超椭圆 4 / 圆弧 2）。
//! 只有外观真的变了才重新安装；安装失败只记日志，文档保留上一份主题。

use nana_ui::runtime::RuntimeDocument;
use nana_ui::theme::{ThemeDefinition, ThemeId, ThemeMode};
use nana_ui_platform::SystemAppearance;

use crate::shell::ShellViewModel;
use crate::theme_map;

/// 「平滑」圆角的超椭圆指数，对应 CSS `corner-shape: superellipse(2)`。
pub const SMOOTH_EXPONENT: f32 = 4.0;
/// 「普通」圆角的圆弧指数。
pub const ROUND_EXPONENT: f32 = 2.0;

/// 设置落到画面上的外观。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    pub mode: ThemeMode,
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
    pub fn for_mode(shell: &ShellViewModel, mode: ThemeMode) -> Self {
        Self {
            mode,
            corner_radius: theme_map::clamp_corner_radius(shell.admin.corner_radius as f32),
            smooth_corners: smooth_corners(&shell.admin.corner_style),
        }
    }

    /// 这份外观对应的主题定义。身份按模式区分，半径进度量。
    pub fn definition(self) -> ThemeDefinition {
        let id = match self.mode {
            ThemeMode::Light => ThemeId::new("momobako.light"),
            ThemeMode::Dark => ThemeId::new("momobako.dark"),
        };
        ThemeDefinition::for_mode(self.mode)
            .with_metrics(theme_map::corner_metrics(self.corner_radius))
            .with_id(id)
    }
}

/// 设置里的主题名换成主题模式。未知值按跟随系统处理并记日志。
pub fn theme_mode(theme: &str, system: Option<SystemAppearance>) -> ThemeMode {
    match theme {
        "light" => ThemeMode::Light,
        "dark" => ThemeMode::Dark,
        other => {
            if other != "system" {
                eprintln!("Nana 未知的主题设置，按跟随系统处理：{other}");
            }
            match system {
                Some(SystemAppearance::Light) => ThemeMode::Light,
                Some(SystemAppearance::Dark) | None => ThemeMode::Dark,
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

/// 把外观装进文档，并设置之后新建绘制器用的圆角指数（宿主发现指数变了会重建绘制器）。
/// 成功返回 true；失败记日志并返回 false，文档保留原主题。
pub fn install(document: &mut RuntimeDocument, appearance: Appearance) -> bool {
    nana_ui::set_corner_exponent(if appearance.smooth_corners { SMOOTH_EXPONENT } else { ROUND_EXPONENT });
    match document.context_mut().set_theme_definition(&appearance.definition()) {
        Ok(_) => true,
        Err(error) => {
            eprintln!("Nana 外观安装失败：{error}");
            false
        }
    }
}

/// 每帧比对设置和已装外观，变了才重新安装。装不上时下一帧再试。
pub(crate) fn sync(app: &mut crate::MomoBakoApplication, document: &mut RuntimeDocument) {
    let wanted = Appearance::from_shell(&app.shell, app.system_appearance);
    if app.applied_appearance == Some(wanted) {
        return;
    }
    if install(document, wanted) {
        app.applied_appearance = Some(wanted);
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
        assert_eq!(theme_mode("light", Some(SystemAppearance::Dark)), ThemeMode::Light);
        assert_eq!(theme_mode("dark", Some(SystemAppearance::Light)), ThemeMode::Dark);
        assert_eq!(theme_mode("system", Some(SystemAppearance::Light)), ThemeMode::Light);
        assert_eq!(theme_mode("system", None), ThemeMode::Dark);
        assert_eq!(theme_mode("sepia", Some(SystemAppearance::Light)), ThemeMode::Light);
    }

    #[test]
    fn appearance_reads_settings_and_clamps_the_radius() {
        let mut shell = ShellViewModel::default();
        shell.settings.theme = "light".into();
        shell.admin.corner_radius = 32.0;
        shell.admin.corner_style = "round".into();
        let appearance = Appearance::from_shell(&shell, None);
        assert_eq!(appearance.mode, ThemeMode::Light);
        assert_eq!(appearance.corner_radius, 20.0);
        assert!(!appearance.smooth_corners);
        let compiled = appearance.definition().compile().expect("主题能编译");
        assert_eq!(compiled.metrics().radius_md, 20.0);
    }

    #[test]
    fn installing_twice_keeps_the_document_on_the_new_radius() {
        let mut document = RuntimeDocument::new(nana_ui::runtime::DocumentId::new(1).expect("id"));
        let mut shell = ShellViewModel::default();
        shell.settings.theme = "dark".into();
        shell.admin.corner_radius = 8.0;
        assert!(install(&mut document, Appearance::from_shell(&shell, None)));
        shell.admin.corner_radius = 12.0;
        assert!(install(&mut document, Appearance::from_shell(&shell, None)));
        let metrics = document.context().world().style_model().metrics;
        assert_eq!(metrics.radius_md, 12.0);
    }
}
