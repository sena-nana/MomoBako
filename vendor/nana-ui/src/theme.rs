//! Style Model token adapter for the Nana Scene host.
//!
//! [`ThemeMetrics`] / [`ThemeMode`] / [`SemanticPalette`] live in `nana-ui-core`.
//! This module is the L3 token view used by the Scene host. It is **not** a CSS
//! / ThemeTokens factory for arbitrary L1 paint values — see
//! `nana_ui_core::style_model`.

use nana_ui_core::{AppearanceSettings, BackdropTarget};

/// The design system, as a theme author writes it.
///
/// [`ThemeDefinition`] and [`install_theme_definition`] were reachable before
/// this block and the types they are *built from* were not, which made the L3
/// theme surface unusable by construction: a host could hold a definition and
/// compile it, but every `with_*` builder takes a type it could not name, the
/// token structs behind `DesignTokens` could not be spelled, and
/// [`ThemeId`] / [`ThemeSchemaVersion`] / [`ThemeGeneration`] — the three
/// fields a definition needs for identity — were all private to the consumer.
/// The host's only way through was to derive from `ThemeMode::definition()`
/// and mutate public fields, which works but cannot name a step.
///
/// `look.md` tells consumers not to depend on `nana-ui-core` directly, so this
/// is where those names have to surface. The list mirrors
/// `nana_ui_core::lib`'s own theme re-export; keep them in step.
pub use nana_ui_core::{
    AccentRamp, BorderTokens, BorderWidth, ButtonRecipe, ButtonRecipeDraft, ButtonVariantDraft,
    ButtonVariantRecipe, ChromeRadii, CompiledRecipes, CompiledTheme, ComponentRecipe,
    ComponentRecipeDraft, ComponentRecipeId, ComponentThemeRegistry, ControlHeight, ControlPadding,
    DesignTokens, EasingRole, EffectTokens, ElevationRole, FoundationTokens, HAIRLINE, LineRole,
    MotionRole, MotionTokens, OpacityTokens, RadiusTier, SWITCH_METRICS, SemanticColor,
    SemanticPalette, ShadowToken, SpacingStep, SpacingTokens, SquareSize, StateLayer, StatusRecipe,
    SurfaceMaterial, SurfacePadding, SurfaceRole, SurfaceSpec, SurfaceTokens, SwitchMetrics,
    TextWeight, ThemeCompileError, ThemeDefinition, ThemeGeneration, ThemeId, ThemeIdentity,
    ThemeMetrics, ThemeMode, ThemeSchemaVersion, TypeRole, TypographyTokens, UI_BASE_TEXT_SIZE,
    UI_METRICS, space, type_scale,
};
/// Style-model names a host needs to name a node's paint rather than spend a
/// number: the two-role mix behind `NodeStyle::surface_mix` / `outline_mix`,
/// and the CSS-grade paint block reachable through `LayoutStyle::paint`,
/// down to the gradients its `mask` takes.
pub use nana_ui_core::{
    BoxShadowSpec, CssGradient, GradientStop, LinearGradient, MaskImage, PaintStyle,
    RadialGradient, SemanticColorMix,
};

/// Linear RGBA color used by L3 token adapters. Same layout as [`SemanticColor`].
pub type Color = SemanticColor;

/// Identity of a built-in theme after the Scene host has applied its own
/// Appearance policy on top.
const fn host_theme_id(mode: ThemeMode) -> nana_ui_core::ThemeId {
    match mode {
        ThemeMode::Dark => nana_ui_core::ThemeId::new("nana.dark+host"),
        ThemeMode::Light => nana_ui_core::ThemeId::new("nana.light+host"),
    }
}

/// Runtime token bundle: [`SemanticPalette`] + [`ThemeMetrics`] + chrome.
///
/// This is the Style Model Tokens view for the Scene host — not a dump of
/// arbitrary CSS. L1 maps known theme tiers here; unknown business colors must
/// not invent formal token fields.
///
/// The palette is the one in `nana-ui-core`, not a copy of it. There used to
/// be a `Colors` struct here with the same 24 fields and a pair of conversions
/// between the two; one design language does not get two spellings, and a
/// second spelling is where the two drift apart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTokens {
    pub palette: SemanticPalette,
    pub metrics: ThemeMetrics,
    pub workspace_corners_enabled: bool,
    /// Title-bar / chrome strip background.
    ///
    /// Defaults to [`SemanticPalette::surface`], but stays independent so
    /// `titlebar_follows_sidebar=false` can keep the title bar opaque while the
    /// sidebar surface remains translucent.
    pub titlebar: Color,
}

impl ThemeTokens {
    pub const fn new(palette: SemanticPalette, metrics: ThemeMetrics) -> Self {
        Self {
            titlebar: palette.surface,
            palette,
            metrics,
            workspace_corners_enabled: true,
        }
    }

    /// The colour and metric slice of a design system.
    ///
    /// This bundle is a *projection* of a [`ThemeDefinition`], not a rival to
    /// it: it carries the two categories the window host needs to apply its
    /// own Appearance policy on top of, and [`Self::definition`] folds the
    /// result back into a full definition so the other categories survive the
    /// round trip.
    pub const fn from_definition(definition: &ThemeDefinition) -> Self {
        Self {
            palette: definition.tokens.palette,
            metrics: definition.tokens.metrics,
            workspace_corners_enabled: true,
            titlebar: definition.tokens.titlebar_color(),
        }
    }

    /// Fold this bundle back onto the built-in definition for `mode`.
    ///
    /// Typography, motion, effects, surfaces and component recipes come from
    /// that definition. A host that wants to move those authors a
    /// [`ThemeDefinition`] and installs it directly.
    ///
    /// The result is **not** NanaDark any more, so it does not claim to be:
    /// a bundle carrying Appearance's radii and a backdrop alpha is a variant,
    /// and a diagnostic that printed `nana.dark` for it would be pointing at
    /// the wrong thing. Two host bundles still share an id — identity answers
    /// "which theme", never "are these equal"; installs compare values.
    pub const fn definition(&self, mode: ThemeMode) -> ThemeDefinition {
        ThemeDefinition::for_mode(mode)
            .with_id(host_theme_id(mode))
            .with_metrics(self.metrics)
            .with_palette(self.palette)
            .with_titlebar(Some(self.titlebar))
    }

    pub const fn with_workspace_corners(mut self, enabled: bool) -> Self {
        self.workspace_corners_enabled = enabled;
        self
    }

    pub const fn with_titlebar(mut self, titlebar: Color) -> Self {
        self.titlebar = titlebar;
        self
    }

    /// Apply Appearance backdrop alphas when the window surface is transparent.
    ///
    /// Covers plain window alpha (`Transparent`) and native blur materials
    /// (Vibrancy / Mica / Acrylic); only a chosen-solid window skips this.
    ///
    /// - [`BackdropTarget::Sidebar`]: translucency on `surface` (sidebar/chrome).
    ///   Title bar follows only when `titlebar_follows_sidebar` is true.
    /// - [`BackdropTarget::Main`]: translucency on `background` (primary content).
    pub fn with_backdrop(
        mut self,
        transparent_surface: bool,
        target: BackdropTarget,
        opacity: f32,
        titlebar_follows_sidebar: bool,
    ) -> Self {
        if !transparent_surface {
            return self;
        }
        let opacity = AppearanceSettings::clamp_backdrop_opacity(opacity);
        // Which palette role a backdrop thins is a surface-role question, not
        // a `match` on the target written here. This bundle carries no surface
        // roles of its own, so it reads the default ones; a host that authors
        // its own installs a `ThemeDefinition` instead of routing through here.
        let spec = SurfaceTokens::DEFAULT.spec(match target {
            BackdropTarget::Sidebar => SurfaceRole::Chrome,
            BackdropTarget::Main => SurfaceRole::Window,
        });
        if spec.material != SurfaceMaterial::Translucent {
            // The theme says this surface stays opaque. A window material is a
            // platform capability; whether a surface may use it is the design
            // system's call, and this is where that call is honoured.
            return self;
        }
        self.apply_surface_alpha(spec, opacity);
        if matches!(target, BackdropTarget::Sidebar) {
            self.titlebar.a = if titlebar_follows_sidebar {
                opacity
            } else {
                1.0
            };
        }
        self
    }

    fn apply_surface_alpha(&mut self, spec: SurfaceSpec, opacity: f32) {
        if let Some(alpha) = self.palette.alpha_mut(spec.paint) {
            *alpha = opacity;
        } else {
            debug_assert!(
                false,
                "surface role {:?} names a derived colour with no alpha of its own",
                spec.paint
            );
        }
    }
}

impl From<SemanticPalette> for ThemeTokens {
    fn from(palette: SemanticPalette) -> Self {
        Self::new(palette, UI_METRICS)
    }
}

impl From<&ThemeDefinition> for ThemeTokens {
    fn from(definition: &ThemeDefinition) -> Self {
        Self::from_definition(definition)
    }
}

/// Install Style Model tokens on a Runtime document after applying window material.
///
/// Goes through [`ThemeTokens::definition`], so the install is validated like
/// any other theme: a host that hands over a NaN radius gets an error instead
/// of a window that lays out wrong.
pub fn install_theme_tokens(
    context: &mut nana_ui_runtime::AppContext,
    mode: ThemeMode,
    tokens: ThemeTokens,
) -> Result<bool, nana_ui_runtime::FrameworkError> {
    context.set_theme_definition(&tokens.definition(mode))
}

/// Install a full design system on a Runtime document.
///
/// The direct path, for hosts that author typography, motion, effects or
/// component recipes rather than only colour and metrics.
pub fn install_theme_definition(
    context: &mut nana_ui_runtime::AppContext,
    definition: &ThemeDefinition,
) -> Result<bool, nana_ui_runtime::FrameworkError> {
    context.set_theme_definition(definition)
}

/// Token helpers for [`ThemeMode`].
///
/// `colors()` is gone: it returned a second palette type with the same fields
/// as [`SemanticPalette`]. Use [`Self::palette`].
pub trait ThemeModeExt: Copy {
    fn tokens(self) -> ThemeTokens;
    fn palette(self) -> SemanticPalette;
    /// The built-in design system for this mode.
    fn definition(self) -> ThemeDefinition;
}

impl ThemeModeExt for ThemeMode {
    fn palette(self) -> SemanticPalette {
        ThemeMode::palette(self)
    }

    fn definition(self) -> ThemeDefinition {
        ThemeDefinition::for_mode(self)
    }

    fn tokens(self) -> ThemeTokens {
        ThemeTokens::from_definition(&ThemeDefinition::for_mode(self))
    }
}

// The faces themselves live in `nana-ui-core` so that the theme, the SVG
// rasterizer and Canvas2D all reference one copy — see `nana_ui_core::fonts`.
#[cfg(feature = "bundled-fonts")]
pub use nana_ui_core::fonts::{
    UI_FONT_BOLD, UI_FONT_MEDIUM, UI_FONT_REGULAR, UI_FONT_SEMIBOLD, ui_font_sources,
};

#[cfg(test)]
mod tests {
    use super::{ThemeMode, ThemeModeExt, ThemeTokens};
    use nana_ui_core::{BackdropTarget, SemanticPalette};

    /// One palette type, not two. The adapter used to expose a `Colors` struct
    /// with the same 24 fields; this asserts the token bundle now carries the
    /// core palette itself, so there is nothing to keep in sync.
    #[test]
    fn the_token_bundle_carries_the_core_palette_itself() {
        assert_eq!(ThemeMode::Dark.palette(), SemanticPalette::dark());
        assert_eq!(ThemeMode::Light.tokens().palette, SemanticPalette::light());
        assert_eq!(
            ThemeTokens::from(SemanticPalette::dark()).palette,
            SemanticPalette::dark()
        );
    }

    #[test]
    fn titlebar_follows_sidebar_controls_titlebar_alpha() {
        let base = ThemeMode::Light.tokens();
        let follows = base.with_backdrop(true, BackdropTarget::Sidebar, 0.5, true);
        assert!((follows.palette.surface.a - 0.5).abs() < f32::EPSILON);
        assert!((follows.titlebar.a - 0.5).abs() < f32::EPSILON);
        assert!((follows.palette.background.a - 1.0).abs() < f32::EPSILON);

        let independent = ThemeTokens::new(ThemeMode::Light.palette(), ThemeMode::Light.metrics())
            .with_backdrop(true, BackdropTarget::Sidebar, 0.5, false);
        assert!((independent.palette.surface.a - 0.5).abs() < f32::EPSILON);
        assert!(
            (independent.titlebar.a - 1.0).abs() < f32::EPSILON,
            "titlebar must stay opaque when follows=false"
        );

        let main = ThemeMode::Light
            .tokens()
            .with_backdrop(true, BackdropTarget::Main, 0.5, true);
        assert!((main.palette.background.a - 0.5).abs() < f32::EPSILON);
        assert!((main.palette.surface.a - 1.0).abs() < f32::EPSILON);
        assert!((main.titlebar.a - 1.0).abs() < f32::EPSILON);

        let solid =
            ThemeMode::Light
                .tokens()
                .with_backdrop(false, BackdropTarget::Sidebar, 0.5, true);
        assert!((solid.palette.surface.a - 1.0).abs() < f32::EPSILON);
        assert!((solid.titlebar.a - 1.0).abs() < f32::EPSILON);
        assert!((solid.palette.background.a - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn space_and_type_scale_are_on_the_theme_surface() {
        assert_eq!(super::space::MD, 8.0);
        assert_eq!(super::type_scale::BODY, super::UI_BASE_TEXT_SIZE);
        assert_eq!(super::type_scale::SEMIBOLD, 600);
    }
}
